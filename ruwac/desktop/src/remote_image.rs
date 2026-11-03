//! Explicit registered SSH destinations reuse the binary transfer and credential components.
use crate::{
    profiles::{ImageProfileReader, InventoryReader, Profile},
    view::View,
};
use gtk::prelude::*;
use serde_json::{Value, json};
use std::{
    cell::RefCell,
    io::{self, Read, Write},
    path::{Path, PathBuf},
    rc::Rc,
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};
#[derive(Clone)]
pub struct Target {
    pub profile: Profile,
    pub socket: String,
    pub pane: String,
    pub directory: String,
    pub foreground_pid: Option<u64>,
}
fn validate(t: &Target) -> Result<(), String> {
    t.profile.validate()?;
    if t.profile.protocol != "SSH" || !t.profile.preferences.gateway.is_empty() {
        return Err("Choose a direct SSH profile".into());
    }
    if !ruwac_protocol::valid_id(&t.pane, "pane") {
        return Err("Explicit remote pane ID required".into());
    }
    for value in [&t.socket, &t.directory] {
        if !Path::new(value).is_absolute()
            || value.len() > 512
            || value.chars().any(char::is_control)
            || Path::new(value)
                .components()
                .any(|p| matches!(p, std::path::Component::ParentDir))
        {
            return Err("Absolute remote socket and private directory required".into());
        }
    }
    Ok(())
}
fn check(ticket: &AtomicBool) -> io::Result<()> {
    if ticket.load(Ordering::Acquire) {
        Ok(())
    } else {
        Err(io::Error::other(
            "Remote attachment cancelled; inspect the remote draft before retrying",
        ))
    }
}
fn quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}
fn exchange(
    session: &ssh2::Session,
    socket: &str,
    verb: &str,
    bytes: &[u8],
    ticket: &AtomicBool,
) -> io::Result<Value> {
    check(ticket)?;
    let mut channel = session.channel_session().map_err(io::Error::other)?;
    channel
        .exec(&format!("ruwac --socket {} {verb}", quote(socket)))
        .map_err(io::Error::other)?;
    for block in bytes.chunks(65536) {
        check(ticket)?;
        channel.write_all(block)?;
    }
    channel.send_eof().map_err(io::Error::other)?;
    let mut response = Vec::new();
    let mut block = [0u8; 8192];
    loop {
        check(ticket)?;
        let n = channel.read(&mut block)?;
        if n == 0 {
            break;
        }
        if response.len() + n > ruwac_protocol::MAX_FRAME {
            return Err(io::Error::other("Remote response exceeded bound"));
        }
        response.extend_from_slice(&block[..n]);
    }
    check(ticket)?;
    channel.wait_close().map_err(io::Error::other)?;
    let result: Value = serde_json::from_slice(&response)
        .map_err(|_| io::Error::other("Remote RUWAC response unavailable"))?;
    if channel.exit_status().map_err(io::Error::other)? != 0 || result.get("error").is_some() {
        return Err(io::Error::other(
            "Remote RUWAC request failed; verify the daemon, pane and agent draft",
        ));
    }
    Ok(result)
}
static WORKERS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
struct Count;
impl Count {
    fn acquire() -> Result<Self, String> {
        WORKERS
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| {
                (n < 4).then_some(n + 1)
            })
            .map_err(|_| "Remote image worker limit reached")?;
        Ok(Self)
    }
}
impl Drop for Count {
    fn drop(&mut self) {
        WORKERS.fetch_sub(1, Ordering::AcqRel);
    }
}
pub fn send(target: &Target, path: &Path, ticket: &AtomicBool) -> Result<Value, String> {
    let _count = Count::acquire()?;
    validate(target)?;
    check(ticket).map_err(|e| e.to_string())?;
    let session = crate::files::connect(&target.profile)?;
    let rpc = |q: &ruwac_protocol::Request| -> io::Result<Value> {
        let v = exchange(
            &session,
            &target.socket,
            "request",
            &serde_json::to_vec(q)?,
            ticket,
        )?;
        if v["version"] != 0 || v["id"] != q.id {
            return Err(io::Error::other("Remote correlation rejected"));
        }
        Ok(v)
    };
    if let Some(expected) = target.foreground_pid {
        let current = rpc(&ruwac_protocol::request(
            "job.query",
            json!({"pane":target.pane}),
        ))
        .map_err(|e| e.to_string())?;
        if current["result"]["foreground"]["pid"].as_u64() != Some(expected) {
            return Err("Discovered remote agent process changed; refresh targets".into());
        }
    }
    let binary =
        |bytes: Vec<u8>| exchange(&session, &target.socket, "image-receive", &bytes, ticket);
    crate::image_transfer::send(
        &target.profile.server,
        &rpc,
        &binary,
        &target.pane,
        path.to_str().ok_or("UTF-8 staging path required")?,
        &target.directory,
    )
    .map(|v| v["result"].clone())
    .map_err(|e| e.to_string())
}
#[derive(Clone)]
pub struct Choice {
    pub pane: String,
    pub title: String,
    pub pid: u64,
}
fn choices(rows: Vec<Value>) -> Vec<Choice> {
    rows.into_iter()
        .filter_map(|r| {
            let command = r["foreground"]["command"].as_str()?;
            if r["phase"] != "running"
                || !(r["codex_foreground"] == true
                    || (r.get("codex_foreground").is_none()
                        && matches!(
                            Path::new(command).file_name().and_then(|n| n.to_str()),
                            Some("codex" | "codex-cli")
                        )))
            {
                return None;
            }
            let pane = r["id"]
                .as_str()
                .filter(|p| ruwac_protocol::valid_id(p, "pane"))?
                .to_owned();
            let pid = r["foreground"]["pid"].as_u64().filter(|p| *p > 0)?;
            let title = format!(
                "{} / {} · PID {} · {}",
                r["session_name"].as_str().unwrap_or("Remote session"),
                r["window_name"].as_str().unwrap_or("Window"),
                pid,
                pane
            );
            Some(Choice { pane, title, pid })
        })
        .collect()
}
fn discover(profile: &Profile, socket: &str, ticket: &AtomicBool) -> Result<Vec<Choice>, String> {
    let _count = Count::acquire()?;
    let target = Target {
        profile: profile.clone(),
        socket: socket.into(),
        pane: ruwac_protocol::id("pane"),
        directory: "/".into(),
        foreground_pid: None,
    };
    validate(&target)?;
    check(ticket).map_err(|e| e.to_string())?;
    let session = crate::files::connect(profile)?;
    let call = |method: &str, params: Value| -> io::Result<Value> {
        let q = ruwac_protocol::request(method, params);
        let v = exchange(
            &session,
            socket,
            "request",
            &serde_json::to_vec(&q)?,
            ticket,
        )?;
        if v["version"] != 0 || v["id"] != q.id {
            return Err(io::Error::other("Remote discovery correlation rejected"));
        }
        Ok(v["result"].clone())
    };
    let mut rows = crate::jobs::fetch_with(|method, params| {
        let q = ruwac_protocol::request(method, params);
        let v = exchange(
            &session,
            socket,
            "request",
            &serde_json::to_vec(&q)?,
            ticket,
        )?;
        if v["version"] != 0 || v["id"] != q.id {
            return Err(io::Error::other("Remote discovery correlation rejected"));
        }
        Ok(v["result"].clone())
    })
    .map_err(|e| e.to_string())?;
    let schema = call("schema", json!({})).map_err(|e| e.to_string())?;
    if schema["methods"]
        .as_array()
        .is_some_and(|m| m.iter().any(|m| m == "agent.probe"))
    {
        for row in &mut rows {
            row["codex_foreground"] = json!(false);
            if row["phase"] != "running" || row["foreground"]["pid"].as_u64().is_none() {
                continue;
            }
            let probe =
                call("agent.probe", json!({"pane":row["id"]})).map_err(|e| e.to_string())?;
            if probe["pane"] != row["id"]
                || probe["foreground_pid"] != row["foreground"]["pid"]
                || !probe["process_detected"].is_boolean()
            {
                return Err("Remote foreground changed; refresh discovery".into());
            }
            row["codex_foreground"] = probe["process_detected"].clone();
        }
    }
    Ok(choices(rows))
}
pub fn button(
    parent: &impl IsA<gtk::Window>,
    views: Rc<RefCell<Vec<Rc<View>>>>,
    tabs: adw::TabView,
    stage: Option<PathBuf>,
    inventory: InventoryReader,
    reader: ImageProfileReader,
) -> gtk::Button {
    let b = gtk::Button::with_label("Attach PNG to remote agent…");
    let parent = parent.as_ref().downgrade();
    b.connect_clicked(move |_|{
 let Some(parent)=parent.upgrade()else{return;};let Some(page)=tabs.selected_page()else{return;};
 let Some(view)=views.borrow().iter().find(|v|crate::tiles::contains(&page.child(),&v.root)).cloned()else{return;};
 let Some(directory)=stage.clone()else{view.warn("Private image staging unavailable");return;};
 let rows=match inventory(){Ok(rows)=>rows.into_iter().filter(|r|r["protocol"]=="SSH"&&r["configuration_valid"]==true).collect::<Vec<_>>(),Err(e)=>{view.warn(&e.message);return;}};
 if rows.is_empty(){view.warn("Add an SSH machine profile before selecting a remote Codex target");return;}
 let names=rows.iter().map(|r|format!("{} / {} · {}",r["group"].as_str().unwrap_or(""),r["name"].as_str().unwrap_or("SSH"),r["id"].as_str().unwrap_or(""))).collect::<Vec<_>>();
 let choices=gtk::DropDown::from_strings(&names.iter().map(String::as_str).collect::<Vec<_>>());choices.set_widget_name("remote-image-profile");
 let dialog=gtk::Window::builder().title("Remote agent image destination").transient_for(&parent).modal(true).default_width(650).build();
 let body=gtk::Box::new(gtk::Orientation::Vertical,10);body.set_margin_start(16);body.set_margin_end(16);body.set_margin_top(16);body.set_margin_bottom(16);
 let text=gtk::Label::new(Some("Select the intended SSH endpoint and already-running remote Codex or Claude Code pane. This sends a PNG to that remote draft; it does not submit a turn or infer a destination from terminal text."));text.set_wrap(true);body.append(&text);body.append(&choices);
 let mut fields=Vec::new();for(name,label)in[("socket","Remote daemon socket (absolute path)"),("pane","Remote pane ID"),("directory","Existing remote private PNG directory (0700)")]{body.append(&gtk::Label::new(Some(label)));let e=gtk::Entry::builder().max_length(512).build();e.set_widget_name(&format!("remote-image-{name}"));body.append(&e);fields.push(e);}
 let discovered:Rc<RefCell<Option<(String,u64)>>>=Rc::new(RefCell::new(None));
 let targets=gtk::Box::new(gtk::Orientation::Vertical,4);let scroll=gtk::ScrolledWindow::builder().child(&targets).min_content_height(40).max_content_height(200).propagate_natural_height(true).build();body.append(&scroll);
 let refresh_targets=gtk::Button::with_label("Find remote agent panes");refresh_targets.set_widget_name("remote-image-discover");body.append(&refresh_targets);
 let status=gtk::Label::new(None);status.set_wrap(true);status.set_widget_name("remote-image-status");body.append(&status);
 let confirm=gtk::CheckButton::with_label("I verified this endpoint, daemon and remote agent pane");confirm.set_widget_name("remote-image-confirm");body.append(&confirm);
 let reviewed:Rc<RefCell<Option<Profile>>>=Rc::new(RefCell::new(None));
 let detail=gtk::Label::new(None);detail.set_wrap(true);body.append(&detail);
 let review:Rc<dyn Fn(u32)>=Rc::new({let(rows,reader,detail,confirm,reviewed,discovered)=(rows.clone(),reader.clone(),detail.downgrade(),confirm.downgrade(),reviewed.clone(),discovered.clone());move|index|{
   let profile=rows.get(index as usize).and_then(|r|r["id"].as_str()).and_then(|id|reader(id).ok());
   if let Some(detail)=detail.upgrade(){detail.set_text(&profile.as_ref().map(|p|format!("Endpoint {} · {}@{}",p.id,p.username,p.server)).unwrap_or("SSH endpoint unavailable".into()));}
   *reviewed.borrow_mut()=profile;*discovered.borrow_mut()=None;if let Some(confirm)=confirm.upgrade(){confirm.set_active(false);}
 }});review(choices.selected());let refresh=review.clone();choices.connect_selected_notify(move|c|refresh(c.selected()));
 for(index,entry)in fields.iter().enumerate(){let(confirm,discovered)=(confirm.downgrade(),discovered.clone());entry.connect_changed(move |_|{if let Some(c)=confirm.upgrade(){c.set_active(false);}if index<2{*discovered.borrow_mut()=None;}});}
 {
  let(reader,reviewed,fields,status,targets,confirm,window,discovered)=(reader.clone(),reviewed.clone(),fields.clone(),status.clone(),targets.clone(),confirm.clone(),dialog.downgrade(),discovered.clone());
  refresh_targets.connect_clicked(move|button|{
   let Some(profile)=reviewed.borrow().clone()else{status.set_text("Review an SSH endpoint first");return;};
   let socket=fields[0].text().to_string();
   let validation=Target{profile:profile.clone(),socket:socket.clone(),pane:ruwac_protocol::id("pane"),directory:"/".into(),foreground_pid:None};if let Err(e)=validate(&validation){status.set_text(&e);return;}
   confirm.set_active(false);*discovered.borrow_mut()=None;while let Some(child)=targets.first_child(){targets.remove(&child);}
   button.set_sensitive(false);status.set_text("Reading authenticated remote process metadata…");
   let ticket=std::sync::Arc::new(AtomicBool::new(true));let(worker,ticket_monitor)=(ticket.clone(),ticket.clone());let(tx,rx)=std::sync::mpsc::sync_channel(1);
   let profile_worker=profile.clone();let socket_worker=socket.clone();std::thread::spawn(move||{let _=tx.send(discover(&profile_worker,&socket_worker,&worker));});
   let(button,reader,reviewed,fields,status,targets,confirm,window,discovered)=(button.downgrade(),reader.clone(),reviewed.clone(),fields.clone(),status.clone(),targets.clone(),confirm.clone(),window.clone(),discovered.clone());let end=std::time::Instant::now()+Duration::from_secs(20);
   gtk::glib::timeout_add_local(Duration::from_millis(20),move||{
    let expected=serde_json::to_value(&profile).ok();
    let current=window.upgrade().is_some_and(|w|w.is_visible())&&fields[0].text().as_str()==socket&&reviewed.borrow().as_ref().and_then(|p|serde_json::to_value(p).ok())==expected&&reader(&profile.id).is_ok_and(|p|serde_json::to_value(p).ok()==expected);
    if !current||std::time::Instant::now()>=end{ticket_monitor.store(false,Ordering::Release);if let Some(b)=button.upgrade(){b.set_sensitive(true);}status.set_text("Remote discovery cancelled; endpoint, socket or window changed");return gtk::glib::ControlFlow::Break;}
    match rx.try_recv(){Ok(result)=>{
       ticket_monitor.store(false,Ordering::Release);if let Some(b)=button.upgrade(){b.set_sensitive(true);}
       match result{Err(e)=>status.set_text(&e),Ok(rows)=>{
        status.set_text(if rows.is_empty(){"No live Codex or Claude Code foreground found. Start one in the chosen remote daemon."}else{"Choose a remote pane, then confirm the destination. Delivery rechecks the process identity."});
        for row in rows{let b=gtk::Button::with_label(&row.title);b.set_widget_name(&format!("remote-image-choice-{}",row.pane));let(pane,confirm,discovered,reader,reviewed,socket_entry,expected,socket,status,id)=(fields[1].clone(),confirm.clone(),discovered.clone(),reader.clone(),reviewed.clone(),fields[0].clone(),expected.clone(),socket.clone(),status.clone(),profile.id.clone());b.connect_clicked(move|_|{
          if socket_entry.text().as_str()!=socket||reviewed.borrow().as_ref().and_then(|p|serde_json::to_value(p).ok())!=expected||!reader(&id).is_ok_and(|p|serde_json::to_value(p).ok()==expected){status.set_text("Discovered target is stale; refresh remote panes");confirm.set_active(false);return;}
          pane.set_text(&row.pane);*discovered.borrow_mut()=Some((row.pane.clone(),row.pid));confirm.set_active(false);
        });targets.append(&b);}
       }};gtk::glib::ControlFlow::Break},Err(std::sync::mpsc::TryRecvError::Empty)=>gtk::glib::ControlFlow::Continue,Err(_)=>{status.set_text("Remote discovery worker stopped");gtk::glib::ControlFlow::Break}}
   });
  });
 }
 let send=gtk::Button::with_label("Attach clipboard PNG to this remote draft");send.set_widget_name("remote-image-send");body.append(&send);let close=gtk::Button::with_label("Cancel");body.append(&close);dialog.set_child(Some(&body));
 let weak=dialog.downgrade();close.connect_clicked(move |_|{if let Some(w)=weak.upgrade(){w.close();}});
 let weak=dialog.downgrade();let reader=reader.clone();let tabs=tabs.downgrade();
 send.connect_clicked(move |_|{
 if !confirm.is_active(){status.set_text("Confirm the intended remote destination first");return;}
 let Some(row)=rows.get(choices.selected()as usize)else{return;};let id=row["id"].as_str().unwrap_or("");let profile=match reader(id){Ok(p)=>p,Err(e)=>{status.set_text(&e);return;}};
 if reviewed.borrow().as_ref().and_then(|p|serde_json::to_value(p).ok())!=serde_json::to_value(&profile).ok(){review(choices.selected());status.set_text("SSH profile changed; review the endpoint and confirm again");return;}
 let target=Target{profile:profile.clone(),socket:fields[0].text().into(),pane:fields[1].text().into(),directory:fields[2].text().into(),foreground_pid:discovered.borrow().as_ref().filter(|(pane,_)|pane==fields[1].text().as_str()).map(|(_,pid)|*pid)};if let Err(e)=validate(&target){status.set_text(&e);return;}
 let(tabs,root,read,expected,id)=(tabs.clone(),view.root.downgrade(),reader.clone(),serde_json::to_value(&profile).unwrap_or(Value::Null),id.to_owned());
 let selected:Rc<dyn Fn()->bool>=Rc::new(move||tabs.upgrade().zip(root.upgrade()).is_some_and(|(t,r)|t.selected_page().is_some_and(|p|crate::tiles::contains(&p.child(),&r)))&&read(&id).is_ok_and(|p|serde_json::to_value(p).ok().as_ref()==Some(&expected)));
 if !selected(){status.set_text("Terminal or endpoint changed; select the target again");return;}
 if let Some(w)=weak.upgrade(){w.close();}
 let(view,directory)=(view.clone(),directory.clone());
 gtk::glib::timeout_add_local_once(Duration::from_millis(150),move||{view.focus();let weak=Rc::downgrade(&view);crate::image_attach::clipboard_route(&view,directory,selected,Duration::ZERO,Some(target),move|result|if let Some(view)=weak.upgrade(){match result{Ok(v)if v["draft_attachment_observed"]==true&&v["focus_current"]==true=>view.warn("PNG attached to the verified remote draft; submit it there when ready."),Ok(_)=>view.warn("Remote image transfer returned without a current draft confirmation; inspect before retrying."),Err(e)=>view.warn(&e)}});});
 });dialog.present();
 });
    b
}
pub fn fixture(parent: &impl IsA<gtk::Window>, q: &Value) -> Value {
    if q["operation"] == "show" {
        return json!({"clicked":crate::sessions::fixture(parent.as_ref(),"Attach PNG to remote agent…")});
    }
    fn find(w: &gtk::Widget, name: &str) -> Option<gtk::Widget> {
        if w.widget_name() == name {
            return Some(w.clone());
        }
        let mut child = w.first_child();
        while let Some(c) = child {
            if let Some(w) = find(&c, name) {
                return Some(w);
            }
            child = c.next_sibling();
        }
        None
    }
    for w in gtk::Window::list_toplevels() {
        if let Ok(w) = w.downcast::<gtk::Window>() {
            if w.title().as_deref() != Some("Remote agent image destination") {
                continue;
            }
            let root: gtk::Widget = w.clone().upcast();
            if q["operation"] == "close" {
                w.close();
                return json!({"closed":true});
            }
            for name in ["socket", "pane", "directory"] {
                if let Some(value) = q[name].as_str() {
                    if let Some(e) = find(&root, &format!("remote-image-{name}"))
                        .and_then(|e| e.downcast::<gtk::Entry>().ok())
                    {
                        e.set_text(value);
                    }
                }
            }
            if let Some(e) = find(&root, "remote-image-confirm")
                .and_then(|e| e.downcast::<gtk::CheckButton>().ok())
            {
                if let Some(value) = q["confirm"].as_bool() {
                    e.set_active(value);
                }
            }
            if q["operation"] == "discover" || q["operation"] == "choose" {
                let name = if q["operation"] == "discover" {
                    "remote-image-discover".to_owned()
                } else {
                    format!("remote-image-choice-{}", q["choice"].as_str().unwrap_or(""))
                };
                let clicked = find(&root, &name)
                    .and_then(|e| e.downcast::<gtk::Button>().ok())
                    .map(|b| {
                        b.emit_clicked();
                        true
                    })
                    .unwrap_or(false);
                return json!({"clicked":clicked});
            }
            if q["operation"] == "send" {
                if let Some(b) =
                    find(&root, "remote-image-send").and_then(|e| e.downcast::<gtk::Button>().ok())
                {
                    b.emit_clicked();
                }
            }
            return json!({"open":true,"status":find(&root,"remote-image-status").and_then(|e|e.downcast::<gtk::Label>().ok()).map(|e|e.text().to_string())});
        }
    }
    json!({"open":false})
}
#[cfg(test)]
mod tests {
    use super::*;
    fn target() -> Target {
        Target {profile:serde_json::from_value(json!({"id":"endpoint_fixture","machine_id":"machine_fixture","name":"Fixture","group":"Test","protocol":"SSH","server":"localhost","username":"fixture","secret_reference":"unused","favorite":false,"last_used":0,"tags":[]})).unwrap(),socket:"/private/control.sock".into(),pane:ruwac_protocol::id("pane"),directory:"/private/images".into(),foreground_pid:None}
    }
    #[test]
    fn explicit_remote_identity_and_cancellation_are_required() {
        let t = target();
        assert!(validate(&t).is_ok());
        assert!(
            send(&t, Path::new("/never/read.png"), &AtomicBool::new(false))
                .unwrap_err()
                .contains("cancelled")
        );
        let mut t = target();
        t.socket = "../control.sock".into();
        assert!(validate(&t).is_err());
        t = target();
        t.directory = "/private/../elsewhere".into();
        assert!(validate(&t).is_err());
        t = target();
        t.pane = "pane".into();
        assert!(validate(&t).is_err());
        t = target();
        t.profile.preferences.gateway = "unsupported gateway".into();
        assert!(validate(&t).is_err());
    }
}
