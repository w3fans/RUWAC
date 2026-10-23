//! Ephemeral first-class browser leaves. Nothing here runs in the daemon/TUI.
use crate::{
    view::View,
    web_route::{self, Destination},
    workspace::Layout,
};
use adw::prelude::*;
use gtk::{gdk, glib};
use std::{
    cell::{Cell, RefCell},
    rc::{Rc, Weak},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use webkit6::prelude::*;
thread_local! { static VIEWS:RefCell<Vec<Rc<Web>>> = const {RefCell::new(Vec::new())}; }
thread_local! {static TERMINALS:RefCell<Option<Weak<RefCell<Vec<Rc<View>>>>>>=const {RefCell::new(None)};}
pub fn configure(views: &Rc<RefCell<Vec<Rc<View>>>>) {
    TERMINALS.with(|v| *v.borrow_mut() = Some(Rc::downgrade(views)));
}
const MAX_VIEWS: usize = 8;
thread_local! {static SCRIPT_RESULT:RefCell<Option<serde_json::Value>>=const {RefCell::new(None)};}
#[derive(Clone)]
pub struct Source {
    view: Weak<View>,
    session: String,
    pane: String,
    pid: u32,
    endpoint: String,
    binding: String,
    token: String,
    label: String,
}
impl Source {
    pub fn capture(view: &Rc<View>) -> Option<Self> {
        let pane = view.target.borrow().clone();
        let pid = view
            .frame
            .borrow()
            .as_ref()?
            .get("panes")?
            .as_array()?
            .iter()
            .find(|p| p["id"] == pane && p["ended"] != true && p["restored"] != true)?["pid"]
            .as_u64()? as u32;
        let p = crate::launches::image_profile(&view.session, &pane, pid)?;
        let token = crate::launches::image_transport(&view.session, &pane, pid)?;
        Some(Self {
            view: Rc::downgrade(view),
            session: view.session.clone(),
            pane,
            pid,
            endpoint: p.id.clone(),
            binding: crate::launch_context::binding(&p),
            token,
            label: p.name.clone(),
        })
    }
    fn valid(&self) -> bool {
        self.view
            .upgrade()
            .is_some_and(|v| !v.detached.get() && v.image_paste_destination_alive(&self.pane))
            && crate::launches::image_profile(&self.session, &self.pane, self.pid).is_some_and(
                |p| p.id == self.endpoint && crate::launch_context::binding(&p) == self.binding,
            )
            && crate::launches::image_transport(&self.session, &self.pane, self.pid).as_deref()
                == Some(&self.token)
    }
}
struct Remote {
    source: Source,
    original: url::Url,
    forward: crate::ssh_transport::LocalForward,
}
struct Web {
    root: gtk::Box,
    browser: webkit6::WebView,
    address: gtk::Entry,
    status: gtk::Label,
    remote: RefCell<Option<Remote>>,
    pending: RefCell<Option<Arc<AtomicBool>>>,
    binding: RefCell<Option<String>>,
    closed: Cell<bool>,
    endpoint: RefCell<Option<String>>,
}
impl Web {
    fn new(endpoint: Option<String>) -> Rc<Self> {
        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        root.set_hexpand(true);
        root.set_vexpand(true);
        root.set_widget_name(&ruwac_protocol::id("view"));
        root.add_css_class("ruwac-web");
        let bar = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        bar.set_margin_start(4);
        bar.set_margin_end(4);
        let back = gtk::Button::from_icon_name("go-previous-symbolic");
        back.set_widget_name("web-back");
        back.set_tooltip_text(Some("Back"));
        let next = gtk::Button::from_icon_name("go-next-symbolic");
        next.set_widget_name("web-forward");
        next.set_tooltip_text(Some("Forward"));
        let reload = gtk::Button::from_icon_name("view-refresh-symbolic");
        reload.set_widget_name("web-reload");
        reload.set_tooltip_text(Some("Reload"));
        let address = gtk::Entry::new();
        address.set_hexpand(true);
        address.set_placeholder_text(Some("https://example.org — Ctrl+L to edit"));
        address.set_widget_name("web-address");
        let external = gtk::Button::from_icon_name("external-link-symbolic");
        external.set_tooltip_text(Some("Open current page in external browser"));
        for b in [&back, &next, &reload] {
            bar.append(b);
        }
        bar.append(&address);
        bar.append(&external);
        let reconnect = gtk::Button::from_icon_name("network-connect-symbolic");
        reconnect.set_widget_name("web-reconnect");
        reconnect.set_tooltip_text(Some(
            "Reconnect restored remote preview using an explicitly chosen current SSH terminal",
        ));
        bar.append(&reconnect);
        let session = webkit6::NetworkSession::new_ephemeral();
        let browser = webkit6::WebView::builder()
            .network_session(&session)
            .hexpand(true)
            .vexpand(true)
            .build();
        if let Some(settings) = webkit6::prelude::WebViewExt::settings(&browser) {
            settings.set_enable_developer_extras(true);
        }
        let status = gtk::Label::new(Some(if endpoint.is_some() {
            "Remote preview restored disconnected. Open its current SSH terminal and Ctrl+click the URL to reconnect."
        } else {
            "Enter a web address. Browsing storage is private to this view and discarded when it closes."
        }));
        status.set_xalign(0.0);
        status.set_ellipsize(gtk::pango::EllipsizeMode::End);
        root.append(&bar);
        root.append(&browser);
        root.append(&status);
        let web = Rc::new(Self {
            root,
            browser,
            address,
            status,
            remote: RefCell::new(None),
            pending: RefCell::new(None),
            binding: RefCell::new(None),
            closed: Cell::new(false),
            endpoint: RefCell::new(endpoint),
        });
        let w = Rc::downgrade(&web);
        back.connect_clicked(move |_| {
            if let Some(w) = w.upgrade() {
                if w.navigation_ready() {
                    w.browser.go_back();
                } else {
                    w.disconnect();
                }
            }
        });
        let w = Rc::downgrade(&web);
        next.connect_clicked(move |_| {
            if let Some(w) = w.upgrade() {
                if w.navigation_ready() {
                    w.browser.go_forward();
                } else {
                    w.disconnect();
                }
            }
        });
        let w = Rc::downgrade(&web);
        reload.connect_clicked(move |_| {
            if let Some(w) = w.upgrade() {
                if w.navigation_ready() {
                    w.browser.reload();
                } else {
                    w.disconnect();
                }
            }
        });
        let w = Rc::downgrade(&web);
        reconnect.connect_clicked(move |_| {
            if let Some(w) = w.upgrade() {
                w.choose_source();
            }
        });
        let w = Rc::downgrade(&web);
        web.address.connect_activate(move |entry| {
            if let Some(w) = w.upgrade() {
                w.navigate(entry.text().as_str(), None);
            }
        });
        let w = Rc::downgrade(&web);
        external.connect_clicked(move |_| {
            if let Some(w) = w.upgrade() {
                if !w.navigation_ready() {
                    w.disconnect();
                    return;
                }
                if let Some(uri) = w.browser.uri().filter(|u| web_route::parse(u).is_ok()) {
                    gtk::gio::AppInfo::launch_default_for_uri_async(
                        &uri,
                        None::<&gtk::gio::AppLaunchContext>,
                        None::<&gtk::gio::Cancellable>,
                        move |_| {},
                    );
                }
            }
        });
        let w = Rc::downgrade(&web);
        web.browser.connect_uri_notify(move |browser| {
            if let Some(w) = w.upgrade() {
                if let Some(uri) = browser.uri() {
                    w.address.set_text(&uri);
                }
            }
        });
        let w = Rc::downgrade(&web);
        web.browser.connect_load_changed(move |_, event| {
            if let Some(w) = w.upgrade() {
                if event == webkit6::LoadEvent::Finished {
                    if let Some(r) = w.remote.borrow().as_ref() {
                        w.status.set_text(&format!(
                            "{} · remote {}:{} → localhost:{} · ephemeral browser",
                            r.source.label,
                            r.original.host_str().unwrap_or("localhost"),
                            r.original.port_or_known_default().unwrap_or(0),
                            r.forward.local_port
                        ));
                    } else {
                        w.status.set_text("Local web · ephemeral browser");
                    }
                }
            }
        });
        let w = Rc::downgrade(&web);
        web.browser.connect_load_failed(move |_,_,_,_|{if let Some(w)=w.upgrade(){w.status.set_text("Page could not load. Check the URL, server and certificate; certificate validation remains enabled.");}true});
        let w = Rc::downgrade(&web);
        session.connect_download_started(move |_,download|{download.cancel();if let Some(w)=w.upgrade(){w.status.set_text("Downloads are disabled in Preview; use Open externally for an explicit browser download.");}});
        web.browser.connect_permission_request(|_, request| {
            request.deny();
            true
        });
        web.browser.connect_run_file_chooser(|_, request| {
            request.cancel();
            true
        });
        let w = Rc::downgrade(&web);
        web.browser.connect_create(move |_, action| {
            if let Some(w) = w.upgrade() {
                if action.is_user_gesture() {
                    if let Some(uri) = action.request().and_then(|r| r.uri()) {
                        w.navigate(&uri, None);
                    }
                }
            }
            None
        });
        let w = Rc::downgrade(&web);
        web.browser.connect_decide_policy(move |_,decision,kind|{
            if !matches!(kind,webkit6::PolicyDecisionType::NavigationAction|webkit6::PolicyDecisionType::NewWindowAction) {return false;}
            let Some(w)=w.upgrade() else {decision.ignore();return true;};
            if w.endpoint.borrow().is_some() && w.remote.borrow().is_none() {
                decision.ignore();w.status.set_text("Remote preview disconnected. Choose its current SSH terminal to reconnect.");return true;
            }
            let uri=decision.downcast_ref::<webkit6::NavigationPolicyDecision>().and_then(|d|d.navigation_action()).and_then(|a|a.request()).and_then(|r|r.uri());
            let Some(uri)=uri.and_then(|u|web_route::parse(&u).ok()) else {decision.ignore();w.status.set_text("Navigation blocked: only HTTP and HTTPS pages are allowed");return true;};
            if let Some(remote)=w.remote.borrow().as_ref() {
                if !remote.source.valid()||!remote.forward.valid(){decision.ignore();w.browser.stop_loading();w.status.set_text("Original SSH connection changed. Reopen the preview explicitly from its terminal.");return true;}
                let mapped=web_route::mapped(&remote.original,remote.forward.local_port);
                if uri.origin()==mapped.origin() {return false;}
                if uri.origin()==remote.original.origin() {let mapped=web_route::mapped(&uri,remote.forward.local_port);decision.ignore();w.browser.load_uri(mapped.as_str());return true;}
                decision.ignore();w.status.set_text("Remote preview stays on its approved origin/port. Open another URL explicitly in a new Web view.");return true;
            }
            false
        });
        let key = gtk::EventControllerKey::new();
        key.set_propagation_phase(gtk::PropagationPhase::Capture);
        let w = Rc::downgrade(&web);
        key.connect_key_pressed(move |_, key, _, mods| {
            if matches!(key, gdk::Key::l | gdk::Key::L)
                && mods.contains(gdk::ModifierType::CONTROL_MASK)
                && !mods.intersects(
                    gdk::ModifierType::ALT_MASK
                        | gdk::ModifierType::SUPER_MASK
                        | gdk::ModifierType::SHIFT_MASK,
                )
            {
                if let Some(w) = w.upgrade() {
                    w.address.grab_focus();
                    w.address.select_region(0, -1);
                }
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
        web.root.add_controller(key);
        let w = Rc::downgrade(&web);
        glib::timeout_add_local(Duration::from_secs(1), move || {
            let Some(w) = w.upgrade() else {
                return glib::ControlFlow::Break;
            };
            if w.closed.get() {
                return glib::ControlFlow::Break;
            }
            let lost = w
                .remote
                .borrow()
                .as_ref()
                .is_some_and(|r| !r.source.valid() || !r.forward.valid());
            if lost {
                w.disconnect();
            }
            glib::ControlFlow::Continue
        });
        web
    }
    fn choose_source(self: &Rc<Self>) {
        let Some(endpoint) = self.endpoint.borrow().clone() else {
            self.status.set_text(
                "Open a remote localhost link from its SSH terminal with Ctrl+click first",
            );
            return;
        };
        if crate::web_route::parse(self.address.text().as_str()).is_err() {
            self.status.set_text(
                "Enter the original remote HTTP/HTTPS URL, then choose its current SSH terminal",
            );
            return;
        }
        let sources = TERMINALS.with(|v| {
            v.borrow()
                .as_ref()
                .and_then(Weak::upgrade)
                .map(|views| {
                    views
                        .borrow()
                        .iter()
                        .filter_map(Source::capture)
                        .filter(|s| s.endpoint == endpoint && s.valid())
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default()
        });
        if sources.is_empty() {
            self.status.set_text(
                "Open this machine's saved SSH terminal first; no replacement route was inferred",
            );
            return;
        }
        let dialog = gtk::Dialog::builder()
            .title("Choose original SSH terminal")
            .modal(true)
            .build();
        if let Some(window) = self.root.root().and_downcast::<gtk::Window>() {
            dialog.set_transient_for(Some(&window));
        }
        for source in sources {
            let button =
                gtk::Button::with_label(&format!("{} · SSH process {}", source.label, source.pid));
            let weak = Rc::downgrade(self);
            let d = dialog.downgrade();
            button.connect_clicked(move |_| {
                if let Some(d) = d.upgrade() {
                    d.close();
                }
                if let Some(w) = weak.upgrade() {
                    let url = w.address.text();
                    w.navigate(&url, Some(source.clone()));
                }
            });
            dialog.content_area().append(&button);
        }
        dialog.add_button("Cancel", gtk::ResponseType::Cancel);
        dialog.connect_response(|d, _| d.close());
        dialog.present();
    }
    fn navigation_ready(&self) -> bool {
        self.remote.borrow().as_ref().map_or_else(
            || self.endpoint.borrow().is_none(),
            |r| r.source.valid() && r.forward.valid(),
        )
    }
    fn disconnect(&self) {
        if let Some(cancel) = self.pending.borrow_mut().take() {
            cancel.store(true, Ordering::Release);
        }
        self.browser.stop_loading();
        if self.remote.borrow_mut().take().is_some() {
            self.browser.terminate_web_process();
        }
        self.status.set_text("Remote preview disconnected. Reopen from the current SSH terminal to revalidate its route.");
    }
    fn navigate(self: &Rc<Self>, text: &str, source: Option<Source>) {
        if let Some(old) = self.pending.borrow_mut().take() {
            old.store(true, Ordering::Release);
        }
        let uri = match web_route::parse(text) {
            Ok(u) => u,
            Err(e) => {
                self.status.set_text(&e);
                return;
            }
        };
        if let Some(remote) = self.remote.borrow().as_ref() {
            let mapped = web_route::mapped(&remote.original, remote.forward.local_port);
            if source.is_none()
                && (uri.origin() == mapped.origin() || uri.origin() == remote.original.origin())
                && remote.source.valid()
                && remote.forward.valid()
            {
                self.browser
                    .load_uri(web_route::mapped(&uri, remote.forward.local_port).as_str());
                return;
            }
            if source.is_none() {
                self.status.set_text("Open a new Web view for a different origin; this preview belongs to its original SSH host.");
                return;
            }
        }
        if source.is_none() && self.endpoint.borrow().is_some() {
            self.status.set_text("Reconnect this remote preview from its SSH terminal; no route is restored automatically.");
            return;
        }
        match (source, web_route::destination(&uri)) {
            (_, Err(e)) => self.status.set_text(&e),
            (Some(source), Ok(Destination::Loopback { host, port })) => {
                if !source.valid() {
                    self.status.set_text("Original SSH pane or saved endpoint changed; open a current SSH terminal first");
                    return;
                }
                if self
                    .binding
                    .borrow()
                    .as_ref()
                    .is_some_and(|binding| binding != &source.binding)
                {
                    self.status.set_text("Connection details changed. Open a new Web Preview from the current SSH terminal.");
                    return;
                }
                self.disconnect();
                *self.binding.borrow_mut() = Some(source.binding.clone());
                self.status.set_text(
                    "Opening the explicit loopback port through the original verified SSH master…",
                );
                let cancel = Arc::new(AtomicBool::new(false));
                *self.pending.borrow_mut() = Some(cancel.clone());
                let (token, pid) = (source.token.clone(), source.pid);
                let (tx, rx) = std::sync::mpsc::sync_channel(1);
                std::thread::spawn(move || {
                    let result = crate::ssh_transport::LocalForward::open(&token, pid, &host, port);
                    let _ = tx.send(result);
                });
                let weak = Rc::downgrade(self);
                glib::timeout_add_local(Duration::from_millis(20), move || match rx.try_recv() {
                    Ok(result) => {
                        if let Some(w) = weak.upgrade() {
                            if w.closed.get() || cancel.load(Ordering::Acquire) || !source.valid() {
                                return glib::ControlFlow::Break;
                            }
                            w.pending.borrow_mut().take();
                            match result{Ok(forward)=>{let mapped=web_route::mapped(&uri,forward.local_port);*w.endpoint.borrow_mut()=Some(source.endpoint.clone());*w.remote.borrow_mut()=Some(Remote{source:source.clone(),original:uri.clone(),forward});w.browser.load_uri(mapped.as_str());},Err(_)=>w.status.set_text("SSH preview failed. The original master must be live and permit local forwarding; no replacement connection was opened.")}
                        }
                        glib::ControlFlow::Break
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
                    Err(_) => {
                        if let Some(w) = weak.upgrade() {
                            w.status.set_text("SSH preview worker stopped");
                        }
                        glib::ControlFlow::Break
                    }
                });
            }
            (_, Ok(_)) => {
                self.remote.borrow_mut().take();
                self.endpoint.borrow_mut().take();
                self.browser.load_uri(uri.as_str());
            }
        }
    }
    fn close(&self) {
        if self.closed.replace(true) {
            return;
        }
        if let Some(cancel) = self.pending.borrow_mut().take() {
            cancel.store(true, Ordering::Release);
        }
        self.disconnect();
        self.browser.terminate_web_process();
    }
}
pub fn open(tabs: &adw::TabView, url: Option<&str>, source: Option<Source>) -> Result<(), String> {
    let web = register(None)?;
    let page = crate::tiles::append(tabs, &web.root);
    page.set_title("Web Preview");
    tabs.set_selected_page(&page);
    if let Some(url) = url {
        web.navigate(url, source);
    } else {
        web.address.grab_focus();
    }
    Ok(())
}
fn register(endpoint: Option<String>) -> Result<Rc<Web>, String> {
    VIEWS.with(|views| {
        let mut views = views.borrow_mut();
        views.retain(|w| !w.closed.get());
        if views.len() >= MAX_VIEWS {
            return Err("Close a Web view before opening another (limit 8)".into());
        }
        let web = Web::new(endpoint);
        views.push(web.clone());
        Ok(web)
    })
}
pub fn restore(endpoint: Option<String>) -> gtk::Widget {
    match register(endpoint) {
        Ok(w) => w.root.clone().upcast(),
        Err(e) => gtk::Label::new(Some(&e)).upcast(),
    }
}
pub fn describe(root: &gtk::Widget) -> Option<Layout> {
    VIEWS.with(|v| {
        v.borrow()
            .iter()
            .find(|w| w.root.upcast_ref::<gtk::Widget>() == root)
            .map(|w| Layout::Web {
                view_id: root.widget_name().into(),
                endpoint: w.endpoint.borrow().clone(),
            })
    })
}
pub fn close_tree(root: &gtk::Widget) {
    VIEWS.with(|views| {
        let mut views = views.borrow_mut();
        views.retain(|w| {
            if crate::tiles::contains(root, &w.root) {
                w.close();
                false
            } else {
                true
            }
        });
    });
}

/// Available only from the existing private, explicit GUI fixture channel.
pub fn fixture(tabs: &adw::TabView, q: &serde_json::Value) -> serde_json::Value {
    use serde_json::json;
    if q["operation"] == "open" {
        let uri = q["url"].as_str().unwrap_or("");
        if !web_route::parse(uri)
            .is_ok_and(|u| matches!(u.host(),Some(url::Host::Ipv4(ip)) if ip.is_loopback()))
        {
            return json!({"error":"Owned loopback fixture URL required"});
        }
        if let Err(e) = open(tabs, Some(uri), None) {
            return json!({"error":e});
        }
    }
    VIEWS.with(|views|{let views=views.borrow();let Some(web)=views.get(q["index"].as_u64().unwrap_or(0) as usize) else{return json!({"count":views.len()});};
        if q["operation"]=="focus" {
            if let Some(page)=crate::tiles::page(tabs,&web.root){tabs.set_selected_page(&page);}
            web.browser.grab_focus();
        }
        if q["operation"]=="address" {
            let text=q["text"].as_str().unwrap_or("");
            if let Err(error)=web_route::parse(text){return json!({"error":error});}
            web.address.set_text(text);
        }
        if q["operation"]=="choose-source" {web.choose_source();}
        if q["operation"]=="control" {
            let name=q["name"].as_str().unwrap_or("");
            if !["web-back","web-forward","web-reload","web-reconnect"].contains(&name) {return json!({"error":"Unknown preview control"});}
            crate::chrome::walk(web.root.upcast_ref(),&|w| {if w.widget_name()==name {if let Some(button)=w.downcast_ref::<gtk::Button>() {button.emit_clicked();}}});
        }
        if q["operation"]=="choose-terminal" {
            let suffix=format!("SSH process {}",q["pid"].as_u64().unwrap_or(0));
            let chosen=Cell::new(false);
            for window in gtk::Window::list_toplevels(){
                if let Ok(window)=window.downcast::<gtk::Window>() {
                    if window.title().as_deref()==Some("Choose original SSH terminal") {
                        crate::chrome::walk(window.upcast_ref(),&|w|{
                            if let Some(button)=w.downcast_ref::<gtk::Button>() {
                                if !chosen.get()&&button.label().is_some_and(|label|label.ends_with(&suffix))&&button.is_sensitive(){chosen.set(true);button.emit_clicked();}
                            }
                        });
                    }
                }
            }
            if !chosen.get(){return json!({"error":"Matching SSH chooser button unavailable"});}
        }
        if q["operation"]=="script" {
            if !web.browser.uri().and_then(|u|web_route::parse(&u).ok()).is_some_and(|u|matches!(u.host(),Some(url::Host::Ipv4(ip)) if ip.is_loopback())) {return json!({"error":"Owned loopback page required"});}
            let script=q["script"].as_str().unwrap_or("");if script.len()>8192{return json!({"error":"Fixture script limit"});}
            SCRIPT_RESULT.with(|v|v.borrow_mut().take());
            web.browser.evaluate_javascript(script,None,None,None::<&gtk::gio::Cancellable>,|result|{SCRIPT_RESULT.with(|v|*v.borrow_mut()=Some(result.ok().and_then(|v|v.to_json(0)).and_then(|s|serde_json::from_str(s.as_str()).ok()).unwrap_or(serde_json::Value::Null)));});
        }
        let window=web.root.root().and_downcast::<gtk::Window>();
        let focused=window.as_ref().and_then(gtk::prelude::GtkWindowExt::focus);
        json!({"script_result":SCRIPT_RESULT.with(|v|v.borrow().clone()),"count":views.len(),"status":web.status.text().as_str(),"loading":web.browser.is_loading(),"ephemeral":web.browser.network_session().is_some_and(|s|s.is_ephemeral()),"remote":web.remote.borrow().is_some(),"layout":describe(web.root.upcast_ref()),"origin":web.browser.uri().and_then(|u|web_route::parse(&u).ok()).map(|u|u.origin().ascii_serialization()),"address_focused":focused.as_ref().is_some_and(|f|crate::tiles::contains(&web.address,f)),"browser_focused":focused.as_ref().is_some_and(|f|crate::tiles::contains(&web.browser,f)),"browser_origin":window.as_ref().and_then(|w|web.browser.compute_point(w,&gtk::graphene::Point::new(0.,0.))).map(|p|[p.x(),p.y()])})
    })
}
