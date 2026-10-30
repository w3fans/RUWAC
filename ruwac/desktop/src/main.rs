mod about;
mod agent_launch;
mod askpass;
mod chrome;
mod clip_offers;
mod clipboard;
mod cliprdr;
mod compact;
mod credentials;
mod detached;
mod files;
mod files_resume;
mod files_rsync;
#[path = "../../crates/ruwac-cli/src/flags.rs"]
mod flags;
mod gdm;
mod gfx;
mod gui_events;
mod handover;
mod host_api;
mod host_shortcuts;
#[path = "../../crates/ruwac-cli/src/image.rs"]
mod image;
mod image_attach;
#[path = "../../crates/ruwac-cli/src/image_transfer.rs"]
mod image_transfer;
mod ipc;
mod jobs;
mod launch_context;
mod launches;
mod primary;
mod primary_setup;
mod profiles;
mod rdp;
mod rdp_retry;
mod rdp_trust;
mod remote_content;
mod remote_image;
mod remote_keys;
mod remote_paste;
mod remote_paste_upload;
mod schemes;
mod sessions;
#[path = "../../crates/ruwac-pty/src/shell.rs"]
mod shell;
mod shortcuts;
mod sidebar_shell;
mod ssh_jump;
mod ssh_keys;
mod ssh_probe;
mod ssh_transport;
mod staging;
mod system_shortcuts;
mod terminal_history;
mod terminal_primary;
mod terminal_style;
mod tiles;
mod updates;
mod view;
mod web;
mod web_route;
mod window_state;
mod worker;
mod workspace;
mod workspace_control;
mod workspace_picker;
mod workspace_store;
use adw::prelude::*;
use gtk::{gdk, glib};
use serde_json::{Value, json};
use std::{
    cell::{Cell, RefCell},
    path::PathBuf,
    rc::Rc,
    time::{Duration, Instant},
};
use view::View;
fn main() {
    if let Some(status) = ssh_transport::run_forward_worker_if_requested() {
        std::process::exit(status);
    }
    if let Some(status) = askpass::run_if_requested() {
        std::process::exit(status);
    }
    if let Err(e) = run() {
        eprintln!("ruwac-desktop: {e}");
        std::process::exit(1);
    }
}
fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut socket = None;
    let mut sessions = Vec::new();
    let mut fixture = None;
    let mut stage = None;
    let mut clipboard_disabled = false;
    let mut allow_workspace_control = false;
    let mut allow_rdp_control = false;
    let mut new_instance = false;
    let mut window_id = String::new();
    let mut show_machines = false;
    let mut theme = adw::ColorScheme::PreferDark;
    let mut args: Vec<_> = std::env::args().skip(1).collect();
    flags::options(&mut args, flags::DESKTOP);
    if args.first().is_some_and(|arg| arg == "--primary-companion") {
        if args.len() != 2 {
            return Err("Usage: ruwac-desktop -G install|status|preferences|uninstall".into());
        }
        match primary_setup::run(&args[1]) {
            Ok(value) => println!("{}", serde_json::to_string(&value)?),
            Err(message) => {
                println!("{}", json!({"ok":false,"message":message}));
                return Err("Companion setup failed".into());
            }
        }
        return Ok(());
    }

    if args.first().is_some_and(|a| {
        matches!(
            a.as_str(),
            "--enable-primary-receive" | "--disable-primary-receive"
        )
    }) {
        if args.len() != 1 {
            return Err("Expected only the mouse-selection receive switch".into());
        }
        sidebar_shell::set_primary_receive(args[0] == "--enable-primary-receive")?;
        println!("Remote mouse-selection reception preference saved");
        return Ok(());
    }
    if args.first().is_some_and(|a| a == "--dynamic-rdp") {
        if args.len() != 1 {
            return Err("Usage: ruwac-desktop --dynamic-rdp".into());
        }
        let changed = profiles::configure_dynamic_rdp()?;
        println!(
            "Updated {changed} RDP endpoint/group display settings; reopen the GUI and reconnect RDP tabs to load the updated library"
        );
        return Ok(());
    }
    if args.first().is_some_and(|a| a == "--paste-primary") {
        if args.len() != 2 {
            return Err("Usage: ruwac-desktop --paste-primary ENDPOINT_ID".into());
        }
        profiles::configure_primary_paste(&args[1])?;
        println!(
            "Local terminal selections may be pasted into the remote RUWAC desktop; reconnect this RDP tab"
        );
        return Ok(());
    }
    if args.first().is_some_and(|a| a == "--receive-primary") {
        if args.len() != 5 {
            return Err("Usage: ruwac-desktop --receive-primary ENDPOINT_ID LINUX_SSH_USER WAYLAND_SESSION SSH_PORT (0 uses SSH config)".into());
        }
        profiles::configure_primary(&args[1], &args[2], &args[3], args[4].parse::<u16>()?, true)?;
        println!("Mouse selection reception configured; reconnect this RDP tab to apply it");
        return Ok(());
    }
    if args.first().is_some_and(|a| a == "--disable-primary") {
        if args.len() != 2 {
            return Err("Usage: ruwac-desktop --disable-primary ENDPOINT_ID".into());
        }
        profiles::configure_primary(&args[1], "", "", 0, false)?;
        println!("Mouse selection reception disabled");
        return Ok(());
    }

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--socket" => {
                i += 1;
                socket = Some(PathBuf::from(args.get(i).ok_or("missing socket")?));
            }
            "--session" => {
                i += 1;
                sessions.push(args.get(i).ok_or("missing session")?.clone());
            }
            "--fixture-dir" => {
                i += 1;
                fixture = Some(PathBuf::from(
                    args.get(i).ok_or("missing fixture directory")?,
                ));
            }
            "--staging-dir" => {
                i += 1;
                stage = Some(PathBuf::from(
                    args.get(i).ok_or("missing staging directory")?,
                ));
            }
            "--window-id" => {
                i += 1;
                window_id = args.get(i).ok_or("missing window identity")?.clone();
                new_instance = true;
            }
            "--new-instance" => new_instance = true,
            "--show-machines" => show_machines = true,
            "--clipboard-disabled" => clipboard_disabled = true,
            "--allow-workspace-control" => allow_workspace_control = true,
            "--allow-rdp-control" => allow_rdp_control = true,
            "--theme" => {
                i += 1;
                theme = match args.get(i).map(String::as_str) {
                    Some("dark") => adw::ColorScheme::PreferDark,
                    Some("light") => adw::ColorScheme::PreferLight,
                    Some("system") => adw::ColorScheme::Default,
                    _ => return Err("--theme requires dark, light or system".into()),
                };
            }
            "--version" => {
                println!("RUWAC desktop {}", env!("CARGO_PKG_VERSION"));
                return Ok(());
            }
            "--help" => {
                println!("Short options: {}", flags::help(flags::DESKTOP));
                println!(
                    "ruwac-desktop --socket PRIVATE_SOCKET [--session SESSION]... [--staging-dir PRIVATE_DIRECTORY]\nOptional --new-instance opens a separate desktop process while preserving existing windows. --window-id window_HEX uses an independent saved workspace; the New window button supplies it.\nOptional --allow-workspace-control authorizes workspace lifecycle and focus operations; --allow-rdp-control separately authorizes RDP lifecycle control.\nAlt+1…9/0 selects workspace tabs; AltGr tap toggles Map keys. Alt+M machines; Alt+A agents; --theme dark|light|system (default dark).\nCtrl+Alt+A GUI controls; Ctrl+A goes to applications; Ctrl+Shift+C/V desktop selection; Shift+drag overrides application mouse.\nMachines overlays the workspace and hides after selection; pin and width controls are in Preferences. Optional --show-machines opens it at startup.\nOptional independent GTK client; daemon owns PTYs. Attach PNG to Codex targets the selected local Codex draft; no automatic submission."
                );
                return Ok(());
            }
            _ => return Err("unknown option".into()),
        }
        i += 1;
    }
    let app = adw::Application::builder()
        .application_id("si.z4.ruwac")
        .flags(if fixture.is_some() || new_instance {
            gtk::gio::ApplicationFlags::NON_UNIQUE
        } else {
            gtk::gio::ApplicationFlags::FLAGS_NONE
        })
        .build();
    app.register(None::<&gtk::gio::Cancellable>)?;
    if app.is_remote() {
        app.activate();
        return Ok(());
    }
    workspace::configure_window(&window_id)?;
    let socket = socket.ok_or("--socket required")?;
    let mut rpc = worker::Rpc::connect(&socket)?;
    let available = rpc.call("session.list", json!({}))?;
    let saved = workspace::load(&socket);
    let workspace_warning = saved
        .as_ref()
        .err()
        .map(|e| format!("Workspace restore: {e}"));
    let selected = saved.as_ref().ok().and_then(|w| w.selected.clone());
    let restored_tabs = if sessions.is_empty() {
        saved
            .as_ref()
            .ok()
            .map(|w| w.tabs.clone())
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    let restored_selected = saved.as_ref().ok().and_then(|w| w.selected_tab);
    if sessions.is_empty() {
        if let Ok(w) = saved.as_ref() {
            sessions.extend(
                w.sessions
                    .iter()
                    .filter(|id| {
                        available
                            .as_array()
                            .is_some_and(|rows| rows.iter().any(|r| r["id"] == id.as_str()))
                    })
                    .cloned(),
            );
        }
        if sessions.is_empty() {
            sessions.push(
                rpc.call(
                    "session.create",
                    json!({"name":"Terminal","argv":shell::default_terminal_argv()?}),
                )?["session"]
                    .as_str()
                    .ok_or("no session ID")?
                    .into(),
            );
        }
    }
    let session_names: std::collections::HashMap<String, String> = available
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|r| Some((r["id"].as_str()?.into(), r["name"].as_str()?.into())))
        .collect();
    let launch_daemon = rpc.daemon_identity().ok();
    drop(rpc);
    for id in &sessions {
        if !ruwac_protocol::valid_id(id, "session") {
            return Err("invalid session ID".into());
        }
    }
    if let Some(dir) = fixture.as_ref() {
        use std::os::unix::fs::MetadataExt;
        let m = std::fs::symlink_metadata(dir)?;
        if !m.is_dir() || m.uid() != unsafe { libc::geteuid() } || m.mode() & 0o777 != 0o700 {
            return Err("fixture directory must be owned and 0700".into());
        }
    }
    if stage.is_none() {
        if let Some(runtime) = std::env::var_os("XDG_RUNTIME_DIR") {
            stage = Some(PathBuf::from(runtime).join("ruwac-image-stage"));
        }
    }
    if let Some(dir) = stage.as_ref() {
        use std::os::unix::fs::{DirBuilderExt, MetadataExt};
        if !dir.exists() {
            std::fs::DirBuilder::new().mode(0o700).create(dir)?;
        }
        let m = std::fs::symlink_metadata(dir)?;
        if !m.is_dir() || m.uid() != unsafe { libc::geteuid() } || m.mode() & 0o777 != 0o700 {
            return Err("staging directory must be owned and 0700".into());
        }
    }
    let stage_guard = stage.as_ref().map(|p| staging::Stage::new(p)).transpose()?;
    stage = stage_guard.as_ref().map(|s| s.path.clone());
    let views: Rc<RefCell<Vec<Rc<View>>>> = Rc::new(RefCell::new(Vec::new()));
    let all = views.clone();
    let services: Rc<RefCell<Vec<(ipc::Server, glib::SourceId)>>> =
        Rc::new(RefCell::new(Vec::new()));
    let active_services = services.clone();
    app.connect_activate(move|app|{
  app.style_manager().set_color_scheme(theme);
  if let Some(window)=app.active_window(){window.present();return;}
  let window=adw::ApplicationWindow::builder().application(app).title(format!("RUWAC {}", env!("CARGO_PKG_VERSION"))).default_width(1040).default_height(780).build();
  window.set_icon_name(Some("si.z4.ruwac"));window_state::bind(&window);
  let input_probe=Rc::new(Cell::new(0u64));if fixture.is_some(){let counter=input_probe.clone();let key=gtk::EventControllerKey::new();key.set_propagation_phase(gtk::PropagationPhase::Capture);key.connect_key_pressed(move|_,key,_,_|{if key==gdk::Key::F24{counter.set(counter.get()+1);glib::Propagation::Stop}else{glib::Propagation::Proceed}});window.add_controller(key);}

  let box_=gtk::Box::new(gtk::Orientation::Vertical,0);let header=adw::HeaderBar::new();box_.append(&header);let tabs=adw::TabView::new();tabs.remove_shortcuts(adw::TabViewShortcuts::ALT_DIGITS|adw::TabViewShortcuts::ALT_ZERO);/* Window capture handles Alt+digit according to Map keys. */let focus_memory:tiles::FocusMemory=Rc::new(RefCell::new(std::collections::HashMap::new()));tabs.set_vexpand(true);let bar=adw::TabBar::builder().view(&tabs).autohide(false).build();header.set_title_widget(Some(&bar));let tools=gtk::Box::new(gtk::Orientation::Horizontal,6);tools.set_margin_start(6);tools.set_margin_end(6);tools.set_margin_top(4);tools.set_margin_bottom(4);let workspace_overlay=gtk::Overlay::new();workspace_overlay.set_vexpand(true);box_.append(&workspace_overlay);
  let appearance_views=all.clone();let chrome=chrome::Chrome::new(&window,&header,&bar,&box_,Rc::new(move|prefs|for view in appearance_views.borrow().iter(){view.apply_appearance(prefs);}));
  updates::attach(&header,&window,fixture.is_none());
  if let Some(message)=workspace_warning.as_ref(){box_.append(&gtk::Label::new(Some(message)));}
  let new_window=gtk::Button::from_icon_name("window-new-symbolic");new_window.set_widget_name("new-workspace-window");new_window.set_tooltip_text(Some("New RUWAC window — independent tabs, same persistent daemon"));chrome.actions.append(&new_window);
  let window_socket=socket.clone();let window_theme=theme;new_window.connect_clicked(move|button|{
    let result=(||->Result<(),Box<dyn std::error::Error>>{let mut cmd=std::process::Command::new(std::env::current_exe()?);cmd.arg("--new-instance").arg("--window-id").arg(ruwac_protocol::id("window")).arg("--socket").arg(&window_socket).arg("--theme").arg(match window_theme {adw::ColorScheme::PreferLight=>"light",adw::ColorScheme::Default=>"system",_=>"dark"});if clipboard_disabled{cmd.arg("--clipboard-disabled");}if allow_workspace_control{cmd.arg("--allow-workspace-control");}if allow_rdp_control{cmd.arg("--allow-rdp-control");}let mut child=cmd.spawn()?;std::thread::spawn(move||{let _=child.wait();});Ok(())})();if result.is_err(){button.set_tooltip_text(Some("Unable to open a new window"));}
  });
  let control_policy=Rc::new(Cell::new(allow_workspace_control));let control_gate=std::sync::Arc::new(std::sync::atomic::AtomicBool::new(allow_workspace_control));let consent=gtk::CheckButton::with_label("Allow automation to open, restore and focus workspace views");consent.set_active(control_policy.get());let policy=control_policy.clone();let gate=control_gate.clone();consent.connect_toggled(move|c|{policy.set(c.is_active());gate.store(c.is_active(),std::sync::atomic::Ordering::Release);});let remote_policy=Rc::new(Cell::new(allow_rdp_control));let remote_consent=gtk::CheckButton::with_label("Allow automation to open/control RDP connections");remote_consent.set_active(remote_policy.get());let rp=remote_policy.clone();remote_consent.connect_toggled(move|c|rp.set(c.is_active()));let primary_receive=gtk::CheckButton::with_label("Receive mouse selections from another RUWAC desktop");primary_receive.set_widget_name("allow-primary-receive");primary_receive.set_active(sidebar_shell::primary_receive_allowed());primary_receive.connect_toggled(|button|{if let Err(error)=sidebar_shell::set_primary_receive(button.is_active()){button.set_tooltip_text(Some(&error));button.set_active(false);}});let choices=gtk::Box::new(gtk::Orientation::Vertical,6);choices.append(&consent);choices.append(&remote_consent);choices.append(&primary_receive);choices.append(&primary_setup::controls());let preferences=gtk::Popover::new();let menu_body=gtk::Box::new(gtk::Orientation::Vertical,6);menu_body.append(&gtk::Label::new(Some(&format!("RUWAC {}",env!("CARGO_PKG_VERSION")))));let pref_button=gtk::Button::with_label("Preferences…");menu_body.append(&pref_button);preferences.set_child(Some(&menu_body));let options=gtk::MenuButton::builder().icon_name("open-menu-symbolic").tooltip_text("Main menu").popover(&preferences).build();header.pack_end(&options);
  let (pref_window,pref_choices,pref_pop)=(window.downgrade(),choices.clone(),preferences.clone());pref_button.connect_clicked(move |_|{pref_pop.popdown();let Some(window)=pref_window.upgrade()else{return;};chrome::detach(&pref_choices);let dialog=gtk::Window::builder().title("Preferences").transient_for(&window).modal(true).default_width(480).child(&pref_choices).build();dialog.present();});

  let about_button=gtk::Button::with_label("About RUWAC / Support…");about_button.set_widget_name("ruwac-about-button");menu_body.append(&about_button);
  let(about_window,about_pop)=(window.downgrade(),preferences.clone());about_button.connect_clicked(move |_|{about_pop.popdown();if let Some(w)=about_window.upgrade(){about::present(&w);}});
  chrome::walk(header.upcast_ref(),&|w|{if w.widget_name()=="ruwac-updates"{header.remove(w);menu_body.append(w);}});
  let broker=Rc::new(RefCell::new(ruwac_clipboard::FocusBroker::default()));
  let last_focus:Rc<RefCell<Option<String>>>=Rc::new(RefCell::new(None));
  broker.borrow_mut().set_desktop_disabled(clipboard_disabled);
  let b=broker.clone();gtk::prelude::WidgetExt::display(&window).clipboard().connect_changed(move|_|b.borrow_mut().local_copy());
  let b=broker.clone();gtk::prelude::WidgetExt::display(&window).primary_clipboard().connect_changed(move|_|{let pending=b.borrow().last_transfer.as_ref().is_some_and(|t|t.scope==ruwac_clipboard::ClipScope::Primary&&t.state==ruwac_clipboard::TransferState::Streaming);if pending{b.borrow_mut().local_copy();}});
  let rdp_views:Rc<RefCell<Vec<Rc<rdp::RdpView>>>>=Rc::new(RefCell::new(Vec::new()));
  web::configure(&all);
  let file_views:Rc<RefCell<Vec<(gtk::Box,profiles::Profile)>>> = Rc::new(RefCell::new(Vec::new()));
  let placeholders:Rc<RefCell<Vec<(gtk::Widget,workspace::Layout)>>> = Rc::new(RefCell::new(Vec::new()));
  let (terminal_focus,rdp_focus,focus_tabs,focus_state)=(all.clone(),rdp_views.clone(),tabs.clone(),focus_memory.clone());let focus_view:tiles::FocusView=Rc::new(move|root|{
    tiles::remember(&focus_tabs,&focus_state,root);
    if root.is_mapped(){
      if let Some(v)=terminal_focus.borrow().iter().find(|v|v.root.upcast_ref::<gtk::Widget>()==root){v.focus();}
      else if let Some(v)=rdp_focus.borrow().iter().find(|v|v.root.upcast_ref::<gtk::Widget>()==root){v.focus();}
      else if !root.child_focus(gtk::DirectionType::TabForward){root.grab_focus();}
    }else{
      let(root,t,m,terminal,remote)=(root.downgrade(),focus_tabs.clone(),focus_state.clone(),terminal_focus.clone(),rdp_focus.clone());
      let deadline=Instant::now()+Duration::from_secs(1);glib::timeout_add_local(Duration::from_millis(16),move||{let Some(root)=root.upgrade().filter(|r|t.selected_page().is_some_and(|p|tiles::contains(&p.child(),r)))else{return glib::ControlFlow::Break;};if !root.is_mapped(){return if Instant::now()<deadline{glib::ControlFlow::Continue}else{glib::ControlFlow::Break};}tiles::remember(&t,&m,&root);if let Some(v)=terminal.borrow().iter().find(|v|v.root.upcast_ref::<gtk::Widget>()==&root){v.focus();}else if let Some(v)=remote.borrow().iter().find(|v|v.root.upcast_ref::<gtk::Widget>()==&root){v.focus();}else if !root.child_focus(gtk::DirectionType::TabForward){root.grab_focus();}glib::ControlFlow::Break});
    }
  });
  let web_button=gtk::Button::from_icon_name("web-browser-symbolic");web_button.set_tooltip_text(Some("New Web Preview"));web_button.set_widget_name("new-web-preview");chrome.actions.append(&web_button);let web_tabs=tabs.clone();web_button.connect_clicked(move|button|{if let Err(e)=web::open(&web_tabs,None,None){button.set_tooltip_text(Some(&e));}});
  let rdp_button=gtk::Button::with_label("New RDP tab");chrome.actions.append(&rdp_button);let rv=rdp_views.clone();let t=tabs.clone();let b=broker.clone();rdp_button.connect_clicked(move|_|{let v=rdp::RdpView::new(b.clone());let weak_tabs=t.downgrade();let root=v.root.downgrade();v.bind_selection(Rc::new(move||weak_tabs.upgrade().zip(root.upgrade()).is_some_and(|(t,r)|t.selected_page().is_some_and(|p|tiles::contains(&p.child(),&r)))));let p=tiles::append(&t,&v.root);p.set_title("RDP · embedded IronRDP");rv.borrow_mut().push(v.clone());t.set_selected_page(&p);v.focus();});
  let rv=rdp_views.clone();let b=broker.clone();let (memory,focus)=(focus_memory.clone(),focus_view.clone());tabs.connect_selected_page_notify(move|t|{if let Some(page)=t.selected_page(){for v in rv.borrow().iter(){if !tiles::contains(&page.child(),&v.root){v.inactive();}}b.borrow_mut().focus(None);tiles::focus_selected(t,&memory,&focus);}});
  let rv=rdp_views.clone();tabs.connect_close_page(move|_,page|{let mut views=rv.borrow_mut();for v in views.iter(){if tiles::contains(&page.child(),&v.root){v.close();}}views.retain(|v|!tiles::contains(&page.child(),&v.root));glib::Propagation::Proceed});
  if let Some(namespace)=stage.clone(){glib::timeout_add_local(Duration::from_secs(60),move||{let _=staging::expire(&namespace);glib::ControlFlow::Continue});}
  let owned_images:Rc<RefCell<Vec<PathBuf>>>=Rc::new(RefCell::new(Vec::new()));
  let clear=gtk::Button::with_label("Clear message");tools.append(&clear);let vc=all.clone();let tc=tabs.clone();clear.connect_clicked(move|_|{if let Some(page)=tc.selected_page(){for v in vc.borrow().iter(){if tiles::contains(&page.child(),&v.root){*v.warning.borrow_mut()=None;v.status.set_text("Delivery warning acknowledged");}}}});
  let image_button=gtk::Button::with_label("Stage clipboard PNG");tools.append(&image_button);
  let vb=all.clone();let tb=tabs.clone();let sb=stage.clone();let owned=owned_images.clone();let last=last_focus.clone();
  image_button.connect_clicked(move|_|{let Some(page)=tb.selected_page()else{return;};let Some(v)=vb.borrow().iter().find(|v|tiles::contains(&page.child(),&v.root)&&last.borrow().as_deref()==Some(v.session.as_str())).cloned()else{return;};let Some(dir)=sb.clone()else{v.status.set_text("Private staging directory is unavailable");return;};v.focus();let epoch=v.broker.borrow().epoch();let weak=Rc::downgrade(&v);let owned=owned.clone();clipboard::ingest_png(&v.area.display().clipboard(),dir,v.broker.clone(),epoch,ruwac_clipboard::MAX_IMAGE_BYTES,Duration::ZERO,move|r|if let Some(v)=weak.upgrade(){match r{Ok(r)=>{owned.borrow_mut().push(r.path);v.warn(&format!("PNG staged: {}×{}, {} bytes; no agent attachment",r.width,r.height,r.bytes));},Err(e)=>v.warn(&e)}});});
  let attach_button=gtk::Button::with_label("Attach PNG to Codex");attach_button.set_tooltip_text(Some("Attach the desktop clipboard PNG to the selected Codex draft without submitting it"));tools.append(&attach_button);
  let (views,image_tabs,image_stage,image_last)=(all.clone(),tabs.clone(),stage.clone(),last_focus.clone());
  attach_button.connect_clicked(move|button|{
    let Some(page)=image_tabs.selected_page()else{return;};
    let Some(view)=views.borrow().iter().find(|v|tiles::contains(&page.child(),&v.root)&&image_last.borrow().as_deref()==Some(v.session.as_str())).cloned()else{return;};
    let Some(directory)=image_stage.clone()else{view.warn("Private image staging is unavailable");return;};
    let (tabs,root)=(image_tabs.downgrade(),view.root.downgrade());
    let selected:Rc<dyn Fn()->bool>=Rc::new(move||tabs.upgrade().zip(root.upgrade()).is_some_and(|(tabs,root)|tabs.selected_page().is_some_and(|p|tiles::contains(&p.child(),&root))));
    button.set_sensitive(false);view.warn("Reading clipboard PNG for the selected Codex draft…");
    let (button,weak)=(button.downgrade(),Rc::downgrade(&view));
    image_attach::clipboard(&view,directory,selected,Duration::ZERO,move|result|{
      if let Some(button)=button.upgrade(){button.set_sensitive(true);}
      if let Some(view)=weak.upgrade(){match result{
        Ok(receipt) if receipt["draft_attachment_observed"]==true&&receipt["focus_current"]==true=>view.warn("PNG attached to the Codex draft; submit it in Codex when ready. File retained for 24 hours."),
        Ok(_)=>view.warn("Image path queued; attachment not confirmed. Inspect the Codex draft before retrying."),
        Err(error)=>view.warn(&error),
      }}
    });
  });
  for session in &sessions{sessions::append(&tabs,&all,&socket,&broker,session,session_names.get(session).map(String::as_str).unwrap_or("Terminal"));}
  if let Some(id)=selected.as_ref(){if let Some(v)=all.borrow().iter().find(|v|&v.session==id){if let Some(page)=tiles::page(&tabs,&v.root){tabs.set_selected_page(&page);}}}
  let terminal_controls=sessions::controls(tabs.clone(),all.clone(),socket.clone(),broker.clone());
  let new_terminal=terminal_controls.first_child().and_downcast::<gtk::Button>().expect("New terminal action");
  chrome.actions.append(&terminal_controls);
  let job_button=jobs::control(app,tabs.clone(),all.clone(),socket.clone(),broker.clone());chrome.actions.append(&job_button);
  let (vv,rv,fv,ph,t,b)=(all.clone(),rdp_views.clone(),file_views.clone(),placeholders.clone(),tabs.clone(),broker.clone());
  let (close_focus,close_memory)=(focus_view.clone(),focus_memory.clone());
  let close_view:tiles::CloseView=Rc::new(move|root|{
    web::close_tree(root);
    {let mut views=vv.borrow_mut();for v in views.iter().filter(|v|v.root.upcast_ref::<gtk::Widget>()==root){v.close();}views.retain(|v|v.root.upcast_ref::<gtk::Widget>()!=root);}
    {let mut views=rv.borrow_mut();for v in views.iter().filter(|v|v.root.upcast_ref::<gtk::Widget>()==root){v.close();}views.retain(|v|v.root.upcast_ref::<gtk::Widget>()!=root);}
    fv.borrow_mut().retain(|(v,_)|v.upcast_ref::<gtk::Widget>()!=root);ph.borrow_mut().retain(|(v,_)|v!=root);
    b.borrow_mut().focus(None);
    tiles::focus_selected(&t,&close_memory,&close_focus);
  });
  let connection_actions=rdp::install_tab_menu(&window,&tabs,rdp_views.clone(),all.clone());
  let(fv,ph)=(file_views.clone(),placeholders.clone());let close_auxiliary:tiles::CloseView=Rc::new(move|root|{web::close_tree(root);fv.borrow_mut().retain(|(v,_)|!tiles::contains(root,v));ph.borrow_mut().retain(|(v,_)|!tiles::contains(root,v));});let detached_windows=detached::Windows::new(app,all.clone(),rdp_views.clone(),broker.clone(),socket.clone(),close_auxiliary);detached_windows.bind(&window,&tabs,focus_memory.clone());
  let layout_controls=tiles::controls(&tabs,close_view.clone(),focus_view.clone(),focus_memory.clone());
  let (focus_tabs,focus_state)=(tabs.clone(),focus_memory.clone());
  let(rv,vv,b,last,h,actions)=(rdp_views.clone(),all.clone(),broker.clone(),last_focus.clone(),header.clone(),tools.clone());window.connect_focus_widget_notify(move|w|{let focus=gtk::prelude::GtkWindowExt::focus(w);if let Some(f)=&focus{tiles::remember(&focus_tabs,&focus_state,f);}if !w.is_active(){b.borrow_mut().focus(None);for r in rv.borrow().iter().filter(|r|r.root.root().as_ref()==Some(w.upcast_ref())){r.inactive();}return;}if focus.as_ref().is_some_and(|f|tiles::contains(&h,f)||tiles::contains(&actions,f)){for r in rv.borrow().iter(){r.pause_keyboard();}return;}let mut owner=false;for r in rv.borrow().iter(){if focus.as_ref().is_some_and(|f|r.owns_focus(f)){r.activate_clipboard();*last.borrow_mut()=b.borrow().active().map(str::to_owned);owner=true;}else{r.inactive();}}if !owner{if let Some(v)=vv.borrow().iter().find(|v|focus.as_ref().is_some_and(|f|tiles::contains(&v.root,f))){*last.borrow_mut()=Some(v.session.clone());if b.borrow().active()!=Some(v.session.as_str()){b.borrow_mut().focus(Some(&v.session));}}else{if !focus.as_ref().is_some_and(|f|(tiles::contains(&h,f)||tiles::contains(&actions,f))){*last.borrow_mut()=None;}if b.borrow().active().is_some(){b.borrow_mut().focus(None);}}}});

  let (vv,rv,b,last)=(all.clone(),rdp_views.clone(),broker.clone(),last_focus.clone());window.connect_is_active_notify(move|w|{
    let focus=gtk::prelude::GtkWindowExt::focus(w);b.borrow_mut().focus(None);
    for r in rv.borrow().iter().filter(|r|r.root.root().as_ref()==Some(w.upcast_ref())){if w.is_active()&&focus.as_ref().is_some_and(|f|r.owns_focus(f)){r.activate_clipboard();*last.borrow_mut()=b.borrow().active().map(str::to_owned);}else{r.inactive();}}
    if w.is_active(){if let Some(v)=vv.borrow().iter().find(|v|focus.as_ref().is_some_and(|f|tiles::contains(&v.root,f))){b.borrow_mut().focus(Some(&v.session));*last.borrow_mut()=Some(v.session.clone());}}
  });
  let rv=rdp_views.clone();let t=tabs.clone();let b=broker.clone();let(sock,views)=(socket.clone(),all.clone());let fv=file_views.clone();
  let (agent_parent,agent_tabs,agent_views,agent_socket,agent_broker)=(window.clone(),tabs.clone(),all.clone(),socket.clone(),broker.clone());let launch:profiles::AgentLauncher=Rc::new(move|profile,kind|agent_launch::launch(&agent_parent,profile,kind,&agent_tabs,&agent_views,&agent_socket,&agent_broker));
  let (sidebar,search,restore_profile,open_rdp_profile,host_inventory,image_profile,shortcut_menus)=profiles::sidebar(launch,move|profile,persist,lease|{if profile.protocol=="RDP"{let v=rdp::RdpView::new(b.clone());let weak_tabs=t.downgrade();let root=v.root.downgrade();v.bind_selection(Rc::new(move||weak_tabs.upgrade().zip(root.upgrade()).is_some_and(|(t,r)|t.selected_page().is_some_and(|p|tiles::contains(&p.child(),&r)))));v.configure(&profile);v.set_preferences_sink(persist);v.set_trust_sink(lease.trust_sink());lease.bind(&v.root);let page=tiles::append(&t,&v.root);page.set_title(&profile.name);rv.borrow_mut().push(v.clone());t.set_selected_page(&page);v.focus();v.connect_profile();}else{if profiles::take_files_request(){let file_panel=files::panel(profile.clone());fv.borrow_mut().push((file_panel.clone(),profile.clone()));lease.fork().bind(&file_panel);let file_page=tiles::append(&t,&file_panel);file_page.set_title(&format!("{} · Files",profile.name));t.set_selected_page(&file_page);return;}let socket=sock.clone();let name=profile.name.clone();let argv=profiles::ssh_argv(&profile).map(|a|profiles::session_argv(&profile,a));let launch_argv=argv.clone().unwrap_or_default();let(tx,rx)=std::sync::mpsc::sync_channel(1);std::thread::spawn(move||{let result=(||->Result<Value,String>{let mut client=worker::Rpc::connect(&socket).map_err(|e|e.to_string())?;let reply=client.call("session.create",json!({"name":name,"argv":argv?})).map_err(|e|e.to_string())?;if reply["session"].is_string(){Ok(reply)}else{Err("SSH session creation failed".into())}})();let _=tx.send(result);});let(t,b,views,socket)=(t.clone(),b.clone(),views.clone(),sock.clone());let mut pending_lease=Some(lease);glib::timeout_add_local(Duration::from_millis(10),move||{match rx.try_recv(){Ok(Ok(created))=>{let session=created["session"].as_str().unwrap().to_string();launches::remember_remote(&created,&profile,launch_context::Recipe::Ssh,&profile.name,&launch_argv);let v=View::new(socket.clone(),session,b.clone());if let Some(lease)=pending_lease.take(){lease.bind(&v.root);}let page=tiles::append(&t,&v.root);page.set_title(&profile.name);views.borrow_mut().push(v.clone());t.set_selected_page(&page);v.focus();glib::ControlFlow::Break},Ok(Err(_))|Err(std::sync::mpsc::TryRecvError::Disconnected)=>glib::ControlFlow::Break,Err(std::sync::mpsc::TryRecvError::Empty)=>glib::ControlFlow::Continue}});}});
  if let Some(identity)=launch_daemon.clone(){let views=all.clone();let sink:Rc<dyn Fn(&str,&str)>=Rc::new(move|session,message|{for view in views.borrow().iter().filter(|v|session.is_empty()||v.session==session){view.warn(message);}});match workspace::shared_directory(){Ok(root)=>{if let Err(e)=launches::configure(&root,identity,image_profile.clone(),sink){for view in all.borrow().iter(){view.warn(&e);}}},Err(_)=>{for view in all.borrow().iter(){view.warn("Saved SSH launch contexts unavailable");}}}}
  // Reconnect on an ended SSH/agent tab: same command in a new tab, old tab closed.
  let (reopen_tabs,reopen_views,reopen_socket,reopen_broker)=(tabs.clone(),all.clone(),socket.clone(),broker.clone());
  launches::set_reopen(Rc::new(move|old:&str,old_pane:&str,name:&str,argv:&[String]|{
    let (tx,rx)=std::sync::mpsc::sync_channel(1);let (sock,create_name,create_argv)=(reopen_socket.clone(),name.to_string(),argv.to_vec());
    std::thread::spawn(move||{let result=(||->Result<Value,String>{let mut client=worker::Rpc::connect(&sock).map_err(|e|e.to_string())?;let reply=client.call("session.create",json!({"name":create_name,"argv":create_argv})).map_err(|e|e.to_string())?;if reply["session"].is_string(){Ok(reply)}else{Err("session creation failed".into())}})();let _=tx.send(result);});
    let (t,v,s,b,old,old_pane,name,argv)=(reopen_tabs.clone(),reopen_views.clone(),reopen_socket.clone(),reopen_broker.clone(),old.to_string(),old_pane.to_string(),name.to_string(),argv.to_vec());
    glib::timeout_add_local(Duration::from_millis(20),move||match rx.try_recv(){
      Ok(Ok(created))=>{let id=created["session"].as_str().unwrap().to_string();launches::recreated(&old,&old_pane,&created,&name,&argv);let previous=v.borrow().iter().find(|x|x.session==old).cloned();sessions::append(&t,&v,&s,&b,&id,&name);if let Some(previous)=previous{if let Some(page)=tiles::page(&t,&previous.root){t.close_page(&page);}}glib::ControlFlow::Break}
      Ok(Err(e))=>{if let Some(previous)=v.borrow().iter().find(|x|x.session==old){previous.warn(&format!("Reconnect failed: {e}"));}glib::ControlFlow::Break}
      Err(std::sync::mpsc::TryRecvError::Empty)=>glib::ControlFlow::Continue,
      Err(_)=>glib::ControlFlow::Break});
  }));
  let (agent_tabs,agent_views,agent_socket,agent_broker)=(tabs.clone(),all.clone(),socket.clone(),broker.clone());let agent_menu=host_shortcuts::install(&window,shortcut_menus,Rc::new(move||sessions::agent_sessions(agent_tabs.clone(),agent_views.clone(),agent_socket.clone(),agent_broker.clone())));chrome.actions.append(&agent_menu);host_shortcuts::attach_ssh_menu(&new_terminal);
  sidebar.append(&agent_launch::button(&window,host_inventory.clone(),image_profile.clone(),tabs.clone(),all.clone(),socket.clone(),broker.clone()));
  tools.append(&remote_image::button(&window,all.clone(),tabs.clone(),stage.clone(),host_inventory.clone(),image_profile.clone()));
  let known_sessions:Rc<RefCell<std::collections::HashSet<String>>>=Rc::new(RefCell::new(available.as_array().into_iter().flatten().filter_map(|s|s["id"].as_str().map(str::to_owned)).collect()));
  let create_saved_view={
  let (all,rdp_views,file_views,placeholders,broker,tabs,socket,restore_profile,known)=(all.clone(),rdp_views.clone(),file_views.clone(),placeholders.clone(),broker.clone(),tabs.clone(),socket.clone(),restore_profile.clone(),known_sessions.clone());
  let create:Rc<dyn Fn(&workspace::Layout)->gtk::Widget>=Rc::new(move|layout|match layout{
      workspace::Layout::Terminal{session,..}=>{
        let view=all.borrow().iter().find(|v|&v.session==session).cloned();
        if let Some(v)=view{if let Some(page)=tiles::page(&tabs,&v.root){if let Ok(body)=page.child().downcast::<gtk::Box>(){body.remove(&v.root);tabs.close_page(&page);}}v.root.clone().upcast()}
        else if known.borrow().contains(session){let view=View::new(socket.clone(),session.clone(),broker.clone());all.borrow_mut().push(view.clone());view.root.clone().upcast()}
        else{let label=gtk::Label::new(Some("This terminal session ended. Use New terminal to start another; the old process was not resurrected."));label.set_wrap(true);label.set_focusable(true);let root:gtk::Widget=label.upcast();placeholders.borrow_mut().push((root.clone(),layout.clone()));root}
      },
      workspace::Layout::Rdp{profile,..}=>{
        let v=rdp::RdpView::new(broker.clone());let weak_tabs=tabs.downgrade();let root=v.root.downgrade();v.bind_selection(Rc::new(move||weak_tabs.upgrade().zip(root.upgrade()).is_some_and(|(t,r)|t.selected_page().is_some_and(|p|tiles::contains(&p.child(),&r)))));
        if let Some(p)=profile{
          if p.id.is_empty(){v.configure(p);}
          else if let Some((current,sink,lease))=restore_profile(&p.id).filter(|(p,_,_)|p.protocol=="RDP"){v.configure(&current);v.set_preferences_sink(sink);v.set_trust_sink(lease.trust_sink());lease.bind(&v.root);}
          else{let label=gtk::Label::new(Some("This RDP profile was removed. Reopen an authorized profile from Machines."));label.set_wrap(true);label.set_focusable(true);let root:gtk::Widget=label.upcast();placeholders.borrow_mut().push((root.clone(),layout.clone()));return root;}
        }
        rdp_views.borrow_mut().push(v.clone());v.root.clone().upcast()
      },
      workspace::Layout::Files{profile,directory,..}=>{
        if let Some((current,_,lease))=restore_profile(&profile.id).filter(|(p,_,_)|p.protocol=="SSH"){let root=files::panel(current.clone());files::directory(&root,Some(directory));lease.bind(&root);file_views.borrow_mut().push((root.clone(),current));root.upcast()}
        else{let label=gtk::Label::new(Some("This file profile was removed. Reopen an authorized SSH profile from Machines."));label.set_wrap(true);label.set_focusable(true);let root:gtk::Widget=label.upcast();placeholders.borrow_mut().push((root.clone(),layout.clone()));root}
      },
      workspace::Layout::Web{endpoint,..}=>web::restore(endpoint.clone()),
      workspace::Layout::Split{..}=>unreachable!(),
    });
  create};
  if !restored_tabs.is_empty(){
    tiles::restore(&tabs,&restored_tabs,&focus_memory,|layout|create_saved_view(layout));
    if let Some(index)=restored_selected{let offset=tabs.n_pages()-restored_tabs.len()as i32;let i=offset+index as i32;if i>=0&&i<tabs.n_pages(){tabs.set_selected_page(&tabs.nth_page(i));}}
  }
  let(fv,ph)=(file_views.clone(),placeholders.clone());tabs.connect_close_page(move|_,p|{web::close_tree(&p.child());fv.borrow_mut().retain(|(v,_)|!tiles::contains(&p.child(),v));ph.borrow_mut().retain(|(v,_)|!tiles::contains(&p.child(),v));glib::Propagation::Proceed});
  remote_paste::configure(stage.clone(),image_profile.clone());
  let paste_views=all.clone();window.connect_is_active_notify(move|w|{if !w.is_active(){for v in paste_views.borrow().iter(){v.cancel_image_paste();}}});
  let machine_shell=Rc::new(sidebar_shell::Shell::new(&sidebar,&tabs,&choices,show_machines));workspace_overlay.set_child(Some(&machine_shell.view));chrome.actions.append(&machine_shell.button);tabs.set_hexpand(true);let (t,m,f)=(tabs.clone(),focus_memory.clone(),focus_view.clone());machine_shell.view.connect_show_sidebar_notify(move|v|{if !v.shows_sidebar(){tiles::focus_selected(&t,&m,&f);let(v,t,m,f)=(v.downgrade(),t.clone(),m.clone(),f.clone());glib::timeout_add_local_once(Duration::from_millis(250),move||{if v.upgrade().is_some_and(|v|!v.shows_sidebar()&&v.root().and_downcast::<gtk::Window>().is_some_and(|w|w.is_active())){tiles::focus_selected(&t,&m,&f);}});}});
  // RDP focus sends system shortcuts to the remote; AltGr returns the next one to this desktop.
  let grab=system_shortcuts::Grab::new(&window,{let rv=rdp_views.clone();move|f|rv.borrow().iter().any(|r|r.owns_focus(f))});
  let g=grab.clone();window.connect_focus_widget_notify(move|_|g.update());let g=grab.clone();window.connect_is_active_notify(move|_|g.update());let key_grab=grab.clone();
  let shortcut_terminal=new_terminal.clone();let shortcut_window=window.downgrade();let shortcut_controls=chrome.controls.clone();let shell=machine_shell.clone();let search=search.clone();let keys=gtk::EventControllerKey::new();keys.set_propagation_phase(gtk::PropagationPhase::Capture);let mode=Rc::new(shortcuts::LocalMode::default());let tab_switch=tabs.clone();
  // AltGr tap: Map keys (Alt+#, Alt+Tab… go to the terminal/remote) until the next tap.
  let badge=gtk::Label::new(Some("Map keys"));badge.add_css_class("local-mode");badge.set_tooltip_text(Some("Map keys: Alt+1…9 and system shortcuts such as Alt+Tab and Super go to the terminal program or remote desktop (a RUWAC session switches its windows with Alt+N). Tap AltGr again for RUWAC tabs and this computer's shortcuts."));badge.set_visible(false);header.pack_end(&badge);
  let (released,release_grab,release_badge)=(mode.clone(),key_grab.clone(),badge.clone());keys.connect_key_released(move|_,_,code,_|{if let Some(on)=released.release(code){release_grab.set_local(!on);release_badge.set_visible(on);}});

  shortcuts::bind_mode(window.upcast_ref(),&mode);
  let menu_chrome=chrome.clone();let menu_epoch=Rc::new(Cell::new(0u64));
  keys.connect_key_pressed(move|_,key,code,mods|{
    // Host menu chords precede application input, in either Map keys mode.
    mode.press(key,code);
    if key==gdk::Key::Escape{menu_epoch.set(menu_epoch.get().wrapping_add(1));}
    if let Some(agents)=shortcuts::host_menu(key,mods){
      menu_epoch.set(menu_epoch.get().wrapping_add(1));let ticket=menu_epoch.get();
      if agents{menu_chrome.show_actions();if agent_menu.is_mapped(){agent_menu.popup();}else{
        let epoch=menu_epoch.clone();let slot=Rc::new(RefCell::new(None));let pending=slot.clone();
        let handler=agent_menu.connect_map(move|menu|{if let Some(id)=pending.borrow_mut().take(){menu.disconnect(id);}if epoch.get()==ticket&&menu.root().and_downcast::<gtk::Window>().is_some_and(|w|w.is_active()){menu.popup();}});*slot.borrow_mut()=Some(handler);
      }}
      else{shell.view.set_show_sidebar(true);search.grab_focus();}
      return glib::Propagation::Stop;
    }
    if !mode.active(){if let Some(index)=shortcuts::alt_tab_index(key,mods){shortcuts::select_tab(&tab_switch,index);return glib::Propagation::Stop;}}
    if let Some(rdp)=shortcuts::local_new_tab(key,mods,mode.active()){if rdp{shell.view.set_show_sidebar(true);search.set_text("RDP");search.grab_focus();}else if shortcut_terminal.is_sensitive(){shortcut_terminal.emit_clicked();}return glib::Propagation::Stop;}if shortcuts::local_escape(key,mods){shortcut_controls.grab_focus();return glib::Propagation::Stop;}if shortcuts::fullscreen(key,mods){if let Some(w)=shortcut_window.upgrade(){if w.is_fullscreen(){w.unfullscreen();}else{w.fullscreen();}}return glib::Propagation::Stop;}if chrome::controls_shortcut(key,mods){shortcut_controls.popup();return glib::Propagation::Stop;}if key==gdk::Key::Escape&&shell.view.is_collapsed()&&shell.view.shows_sidebar(){shell.view.set_show_sidebar(false);return glib::Propagation::Stop;}glib::Propagation::Proceed});window.add_controller(keys);window.set_content(Some(&box_));


  let vv=all.clone();tabs.connect_close_page(move|_,page|{let mut views=vv.borrow_mut();for v in views.iter(){if tiles::contains(&page.child(),&v.root){v.close();}}views.retain(|v|!tiles::contains(&page.child(),&v.root));glib::Propagation::Proceed});
  let(vv,rv,fv,ph,saved_socket)=(all.clone(),rdp_views.clone(),file_views.clone(),placeholders.clone(),socket.clone());
  let recorded_windows=Rc::downgrade(&detached_windows);
  let recorder:workspace::Recorder=Rc::new(move||{
    let group=recorded_windows.upgrade().ok_or_else(||"Workspace windows closed".to_string())?;let windows=group.tabs();let state=group.inspect();let selected_id=state["selected_tab"].as_str();let open=vv.borrow();let selected=windows.iter().filter_map(|(t,_)|t.selected_page()).find(|p|Some(p.child().widget_name().as_str())==selected_id).and_then(|page|open.iter().find(|v|tiles::contains(&page.child(),&v.root)).map(|v|v.session.clone()));let describe=|root:&gtk::Widget|{
      if let Some(v)=open.iter().find(|v|v.root.upcast_ref::<gtk::Widget>()==root){return Some(workspace::Layout::Terminal{view_id:root.widget_name().to_string(),session:v.session.clone()});}
      if let Some(v)=rv.borrow().iter().find(|v|v.root.upcast_ref::<gtk::Widget>()==root){return Some(workspace::Layout::Rdp{view_id:root.widget_name().to_string(),profile:v.configuration()});}
      if let Some((view,profile))=fv.borrow().iter().find(|(v,_)|v.upcast_ref::<gtk::Widget>()==root){return Some(workspace::Layout::Files{view_id:root.widget_name().to_string(),profile:profile.clone(),directory:files::directory(view,None)});}
      if let Some(layout)=web::describe(root){return Some(layout);}
      ph.borrow().iter().find(|(w,_)|w==root).map(|(_,l)|l.clone())
    };
    let mut layouts=Vec::new();for (window_tabs,memory) in windows{layouts.extend(tiles::snapshot(&window_tabs,&describe,&memory)?);}
    fn sessions(layout:&workspace::Layout,ids:&mut Vec<String>){match layout{workspace::Layout::Terminal{session,..}=>{if !ids.contains(session){ids.push(session.clone());}},workspace::Layout::Split{first,second,..}=>{sessions(first,ids);sessions(second,ids);},_=>()}}
    let mut ids=Vec::new();for tab in &layouts{sessions(&tab.layout,&mut ids);}let selected_tab=layouts.iter().position(|tab|Some(tab.id.as_str())==selected_id);
    Ok(workspace::Workspace{schema:3,socket:saved_socket.clone(),sessions:ids,selected,tabs:layouts,selected_tab})
  });
  detached_windows.set_recorder(recorder.clone());
  workspace::watch(&window,recorder.clone());
  let workspace_lifecycle=workspace_store::folder(&socket).ok().map(|folder|Rc::new(workspace_control::Control{tabs:tabs.clone(),memory:focus_memory.clone(),permission:control_policy.clone(),gate:control_gate.clone(),record:recorder.clone(),create:create_saved_view.clone(),known_sessions:known_sessions.clone(),socket:socket.clone(),folder,state:RefCell::new(Default::default()),workers:std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0))}));
  if let Some(control)=workspace_lifecycle.as_ref(){chrome.actions.append(&workspace_picker::button(&window,control.clone()));}
  chrome::walk(chrome.actions.upcast_ref(),&|w|{if let Some(b)=w.downcast_ref::<gtk::Button>(){let label=b.label().unwrap_or_default();let item=match label.as_str(){"New RDP tab"=>Some(("video-display-symbolic","New RDP")),"New terminal"=>Some(("utilities-terminal-symbolic","New term")),"Sessions…"=>Some(("view-list-symbolic","Sessions")),"Jobs…"=>Some(("checkbox-checked-symbolic","Jobs")),"Machines"=>Some(("computer-symbolic","Machines")),"Workspaces…"=>Some(("view-grid-symbolic","Workspaces")),_=>None};if let Some((icon,name))=item{let tip=b.tooltip_text().map(|s|s.to_string()).unwrap_or(label.to_string());chrome::action(b,icon,name,&tip);}}});
  let (exit_tabs,exit_views,exit_close,exit_window)=(tabs.clone(),all.clone(),close_view.clone(),window.downgrade());
  glib::timeout_add_local(Duration::from_millis(50),move||{
    if exit_tabs.root().is_none(){return glib::ControlFlow::Break;}
    let ended:Vec<gtk::Widget>=exit_views.borrow().iter().filter(|v|v.finished.get()||v.detached.get()).map(|v|v.root.clone().upcast()).collect();
    let had_exit=!ended.is_empty();
    for root in ended { let _=tiles::close_view(&exit_tabs,&root,&exit_close); }
    if had_exit && exit_tabs.n_pages()==0 { if let Some(window)=exit_window.upgrade(){window.close();} return glib::ControlFlow::Break; }
    glib::ControlFlow::Continue
  });
  let (chrome_tabs,chrome_remote,chrome_terminal,chrome_state,chrome_tools,chrome_layout)=(tabs.clone(),rdp_views.clone(),all.clone(),chrome.clone(),tools.clone(),layout_controls.clone());
  let mut chrome_selected=String::new();let mut chrome_docked=false;
  glib::timeout_add_local(Duration::from_millis(100),move||{
    if chrome_tabs.root().is_none(){return glib::ControlFlow::Break;}
    let selected=chrome_tabs.selected_page();let key=selected.as_ref().map(|p|p.child().widget_name().to_string()).unwrap_or_default();let docked=chrome_state.dock_toggle_active();
    if key!=chrome_selected||docked!=chrome_docked{
      let remote=chrome_remote.borrow();let terminal=chrome_terminal.borrow();let session=selected.as_ref().and_then(|p|remote.iter().find(|v|tiles::contains(&p.child(),&v.root)).map(|v|&v.toolbar).or_else(||terminal.iter().find(|v|tiles::contains(&p.child(),&v.root)).map(|v|&v.history_controls)));
      chrome_state.rebuild(session,&chrome_tools,Some(&chrome_layout));chrome_selected=key;chrome_docked=docked;
    }
    for i in 0..chrome_tabs.n_pages(){let page=chrome_tabs.nth_page(i);let remote=chrome_remote.borrow();let terminal=chrome_terminal.borrow();
      if let Some(v)=remote.iter().find(|v|tiles::contains(&page.child(),&v.root)){
        tiles::set_page_icon(&page,"video-display-symbolic");let (phase,summary)=v.tab_summary();
        tiles::set_page_tooltip(&page,&glib::markup_escape_text(&summary));
        let status=match phase.as_str(){"connected"=>tiles::TabStatus::Ready,"connecting"|"reconnecting"|"display_change"=>tiles::TabStatus::Busy,"error"=>tiles::TabStatus::Error,_=>tiles::TabStatus::Offline};
        tiles::set_page_status(&page,status);tiles::set_page_loading(&page,matches!(phase.as_str(),"connecting"|"reconnecting"|"display_change"));
        if page.child().first_child().as_ref()==Some(v.root.upcast_ref()){tiles::set_page_title(&page,&v.tab_title());}
      }else if let Some(v)=terminal.iter().find(|v|tiles::contains(&page.child(),&v.root)){
        tiles::set_page_icon(&page,"utilities-terminal-symbolic");
        if page.child().first_child().as_ref()==Some(v.root.upcast_ref()) && page.title()=="Terminal"{tiles::set_page_title(&page,&sessions::terminal_title("Terminal"));}
        tiles::set_page_tooltip(&page,&glib::markup_escape_text(&format!("{}\n{}",page.title(),v.status.text())));
        tiles::set_page_status(&page,v.tab_status());
      }
    }
    glib::ControlFlow::Continue
  });
  match ipc::Server::start(&socket){
   Ok((server,requests))=>{
    let (t,memory,focus,policy,terminal,remote,files)=(tabs.clone(),focus_memory.clone(),focus_view.clone(),control_policy.clone(),all.clone(),rdp_views.clone(),file_views.clone());
    let (rdp_policy,rdp_opener,rdp_broker)=(remote_policy.clone(),open_rdp_profile.clone(),broker.clone());
    let api_windows=detached_windows.clone();let workspace_id=workspace::identity(&socket);let gui_id=ruwac_protocol::id("gui");let mut event_hub=gui_events::Hub::new(gui_id.clone());let mut event_observed=Instant::now()-Duration::from_secs(1);
    let source=glib::timeout_add_local(Duration::from_millis(16),move||{
     if event_observed.elapsed()>=Duration::from_millis(100){
       let layout=api_windows.inspect();let mut snapshot=gui_events::Snapshot::default();
       if let Some(rows)=layout["tabs"].as_array(){for row in rows{if let Some(id)=row["id"].as_str(){snapshot.tabs.insert(id.to_owned(),row["layout"].clone());}}}
       snapshot.selected=layout["selected_tab"].as_str().map(str::to_owned);snapshot.active=api_windows.active_window().is_some();
       let input_focus=api_windows.active_window().and_then(|w|gtk::prelude::GtkWindowExt::focus(&w));snapshot.focus=api_windows.tabs().iter().flat_map(|(t,_)|tiles::views(t)).collect::<Vec<_>>().iter().find(|root|input_focus.as_ref().is_some_and(|f|tiles::contains(*root,f))).map(|r|r.widget_name().to_string());
       for view in remote.borrow().iter(){let(phase,frame)=view.event_state();let phase=if ["connecting","connected","disconnected","error"].contains(&phase.as_str()){phase}else{"unknown".into()};snapshot.rdp.insert(view.root.widget_name().to_string(),(phase,frame));}
       event_hub.observe(snapshot);event_observed=Instant::now();
     }
     for _ in 0..4 {let Ok(task)=requests.try_recv()else{break;};if matches!(task.request.method.as_str(),"workspace.saved"|"workspace.save"|"workspace.open"|"workspace.restore") {if Instant::now()>=task.expires{let _=task.reply.try_send(Err(ruwac_protocol::Error::new("GUI_TIMEOUT","Expired workspace request")));}else if let Some(control)=workspace_lifecycle.as_ref(){control.submit(task);}else{let _=task.reply.try_send(Err(ruwac_protocol::Error::new("WORKSPACE_UNAVAILABLE","Private snapshot store unavailable")));}continue;}let req=&task.request;let params=req.params.as_object();let empty=params.is_some_and(|p|p.is_empty());let target=params.and_then(|p|p.get("id")).and_then(Value::as_str);let explicit=params.is_some_and(|p|p.len()==1)&&target.is_some();
      let result=if Instant::now()>=task.expires{Err(ruwac_protocol::Error::new("GUI_TIMEOUT","Expired GUI request was not executed"))}else{match req.method.as_str(){
       "workspace.describe" if empty=>Ok(json!({"protocol":0,"gui_api":4,"gui_version":env!("CARGO_PKG_VERSION"),"workspace_id":workspace_id,"gui_instance":gui_id,"methods":["clipboard.primary.offer","host.list","host.query","workspace.describe","workspace.list","workspace.events.subscribe","workspace.events.poll","workspace.events.unsubscribe","workspace.activate","workspace.focus","workspace.saved","workspace.save","workspace.open","workspace.restore","rdp.list","rdp.query","rdp.open","rdp.reconnect","rdp.disconnect"],"permissions":{"metadata_read":true,"focus_control":policy.get(),"workspace_lifecycle_control":policy.get(),"rdp_lifecycle_control":rdp_policy.get(),"clipboard_read":false,"primary_write":sidebar_shell::primary_receive_allowed()&&rdp_broker.borrow().desktop_enabled(),"remote_input":false},"transport":{"same_uid":true,"frame_limit":ruwac_protocol::MAX_FRAME,"peer_limit":8,"queue_limit":16,"max_rdp_views":64}})),
       "clipboard.primary.offer"=>api_windows.active_window().ok_or_else(||ruwac_protocol::Error::new("PRIMARY_REFUSED","Focus the remote RUWAC window before pasting a local selection")).and_then(|window|terminal_primary::receive(&window,&req.params,rdp_broker.borrow().desktop_enabled())),
       "host.list"|"host.query"=>host_inventory().and_then(|rows|host_api::exchange(&req.method,&req.params,rows)),
       "workspace.events.subscribe"|"workspace.events.poll"|"workspace.events.unsubscribe"=>event_hub.exchange(&req.method,&req.params,task.actor_uid,Instant::now()),
       "workspace.list" if empty=>{let mut state=api_windows.inspect();let input_focus=api_windows.active_window().and_then(|w|gtk::prelude::GtkWindowExt::focus(&w));state["input_focus_view"]=json!(api_windows.tabs().iter().flat_map(|(t,_)|tiles::views(t)).collect::<Vec<_>>().iter().find(|root|input_focus.as_ref().is_some_and(|f|tiles::contains(*root,f))).map(|r|r.widget_name().to_string()));state["window_active"]=json!(api_windows.active_window().is_some());state["workspace_id"]=json!(workspace_id);state["gui_instance"]=json!(gui_id);state["windows"]=json!(api_windows.tabs().iter().map(|(t,m)|tiles::inspect(t,m)).collect::<Vec<_>>());state["views"]=json!(api_windows.tabs().iter().flat_map(|(t,_)|tiles::views(t)).collect::<Vec<_>>().iter().map(|root|{let session=terminal.borrow().iter().find(|v|v.root.upcast_ref::<gtk::Widget>()==root).map(|v|v.session.clone());let kind=if session.is_some(){"terminal"}else if remote.borrow().iter().any(|v|v.root.upcast_ref::<gtk::Widget>()==root){"rdp"}else if files.borrow().iter().any(|(w,_)|w.upcast_ref::<gtk::Widget>()==root){"files"}else if web::describe(root).is_some(){"web"}else{"ended"};json!({"id":root.widget_name().as_str(),"tab":api_windows.page(root).map(|p|p.child().widget_name().to_string()),"kind":kind,"session":session})}).collect::<Vec<_>>());Ok(state)},
       "rdp.list" if empty=>Ok(json!({"views":remote.borrow().iter().filter(|v|api_windows.page(&v.root).is_some()).map(|v|{let mut value=v.metadata();value["tab"]=json!(api_windows.page(&v.root).map(|p|p.child().widget_name().to_string()));value}).collect::<Vec<_>>()})),
       "rdp.list"=>Err(ruwac_protocol::Error::new("INVALID_REQUEST","RDP list takes an empty parameter object")),
       "rdp.query"|"rdp.reconnect"|"rdp.disconnect" if !explicit=>Err(ruwac_protocol::Error::new("INVALID_TARGET","An explicit RDP view ID is required")),
       "rdp.reconnect"|"rdp.disconnect" if !rdp_policy.get()=>Err(ruwac_protocol::Error::new("PERMISSION_DENIED","Enable RDP lifecycle control in the desktop permissions menu")),
       "rdp.query"|"rdp.reconnect"|"rdp.disconnect"=>{
        let id=target.unwrap();let view=remote.borrow().iter().find(|v|v.root.widget_name().as_str()==id&&ruwac_protocol::valid_id(id,"view")&&api_windows.page(&v.root).is_some()).cloned();
        match view {None=>Err(ruwac_protocol::Error::new("STALE_TARGET","RDP view is invalid, detached or closed")),Some(v)=>{if req.method=="rdp.disconnect"{v.close();Ok(v.metadata())}else if req.method=="rdp.reconnect"{v.automation_reconnect().map(|_|v.metadata()).map_err(|_|ruwac_protocol::Error::new("INVALID_PROFILE","RDP view needs a configured endpoint, user and credential reference"))}else{Ok(v.metadata())}}}
       },
       "rdp.open"=>{
        let id=params.and_then(|p|p.get("profile")).and_then(Value::as_str);let activate=params.and_then(|p|p.get("activate")).and_then(Value::as_bool).unwrap_or(false);
        if !params.is_some_and(|p|p.keys().all(|k|k=="profile"||k=="activate")&&p.get("activate").is_none_or(Value::is_boolean))||!id.is_some_and(|id|ruwac_protocol::valid_id(id,"endpoint")){Err(ruwac_protocol::Error::new("INVALID_TARGET","RDP open requires an explicit endpoint ID and optional boolean activate"))}
        else if !rdp_policy.get()||activate&&!policy.get(){Err(ruwac_protocol::Error::new("PERMISSION_DENIED","RDP lifecycle control is required; activation also requires focus control"))}
        else if remote.borrow().len()>=64 {Err(ruwac_protocol::Error::new("LIMIT","RDP automation view limit reached"))}
        else if let Some((profile,persist,lease))=rdp_opener(id.unwrap()) {let v=rdp::RdpView::new(rdp_broker.clone());let weak_tabs=t.downgrade();let root=v.root.downgrade();v.bind_selection(Rc::new(move||weak_tabs.upgrade().zip(root.upgrade()).is_some_and(|(t,r)|t.selected_page().is_some_and(|p|tiles::contains(&p.child(),&r)))));v.configure(&profile);v.set_preferences_sink(persist);v.set_trust_sink(lease.trust_sink());lease.bind(&v.root);let page=tiles::append(&t,&v.root);page.set_title(&profile.name);remote.borrow_mut().push(v.clone());let result=v.automation_reconnect().map(|_|v.metadata()).map_err(|_|ruwac_protocol::Error::new("INVALID_PROFILE","Configured RDP profile is incomplete"));if activate{t.set_selected_page(&page);v.focus();}result}
        else{Err(ruwac_protocol::Error::new("PROFILE_UNAVAILABLE","Existing RDP endpoint unavailable; no host or credentials are accepted in automation requests"))}
       },
       "workspace.activate"|"workspace.focus" if !explicit=>Err(ruwac_protocol::Error::new("INVALID_TARGET","An explicit target ID is required; focus is never an implicit target")),
       "workspace.activate"|"workspace.focus" if !policy.get()=>Err(ruwac_protocol::Error::new("PERMISSION_DENIED","Enable workspace focus control in the desktop permissions menu")),
       "workspace.focus"|"workspace.activate"=>api_windows.focus_id(target.unwrap(),req.method=="workspace.activate").map_err(|_|ruwac_protocol::Error::new("STALE_TARGET","Tab or view ID is invalid or closed")),
       "workspace.describe"|"workspace.list"=>Err(ruwac_protocol::Error::new("INVALID_REQUEST","This operation takes an empty parameter object")),
       _=>Err(ruwac_protocol::Error::new("UNSUPPORTED_METHOD","GUI method unavailable")),
      }};
      let operation=match req.method.as_str(){"workspace.events.subscribe"|"workspace.events.poll"|"workspace.events.unsubscribe"=>req.method.as_str(),"clipboard.primary.offer"|"host.list"|"host.query"|"workspace.describe"|"workspace.list"|"workspace.activate"|"workspace.focus"|"rdp.list"|"rdp.query"|"rdp.open"|"rdp.reconnect"|"rdp.disconnect"=>req.method.as_str(),_=>"unsupported"};let safe_target=target.or_else(||params.and_then(|p|p.get("profile")).and_then(Value::as_str)).filter(|id|ruwac_protocol::valid_id(id,"view")||ruwac_protocol::valid_id(id,"tab")||ruwac_protocol::valid_id(id,"endpoint"));let verdict=result.as_ref().err().map(|e|e.code.as_str()).unwrap_or("allowed");eprintln!("{}",json!({"audit":"workspace_ipc","actor_uid":task.actor_uid,"actor_pid":task.actor_pid,"operation":operation,"target":safe_target,"verdict":verdict}));let _=task.reply.try_send(result);
     }
     glib::ControlFlow::Continue
    });active_services.borrow_mut().push((server,source));
   },Err(e)=>eprintln!("Workspace API unavailable; desktop remains usable: {e}"),
  }

  let(owned,closing_windows,closing_app)=(owned_images.clone(),detached_windows.clone(),app.clone());window.connect_close_request(move|w|{
    match recorder(){Ok(w)=>{if let Err(e)=workspace::save(&w){eprintln!("Workspace save failed: {e}");}},Err(e)=>eprintln!("Workspace save failed: {e}")};
    closing_windows.close_window(w);if closing_app.windows().len()<=1{for path in owned.borrow_mut().drain(..){let _=std::fs::remove_file(path);}closing_app.quit();}glib::Propagation::Proceed
  });window.present();tiles::focus_selected(&tabs,&focus_memory,&focus_view);
  let vv=all.clone();let tabs=tabs.clone();let app=app.clone();let fixture=fixture.clone();let staging=stage.clone();let owned=owned_images.clone();let tick=Rc::new(Cell::new(0u64));let last=Rc::new(Cell::new(Instant::now()));let max_gap=Rc::new(Cell::new(0u128));let sequence=Rc::new(Cell::new(0u64));
  glib::timeout_add_local(Duration::from_millis(16),move||{
   tick.set(tick.get()+1);let now=Instant::now();max_gap.set(max_gap.get().max(now.duration_since(last.replace(now)).as_millis()));for v in vv.borrow().iter(){v.update();}
   if let Some(dir)=fixture.as_ref(){let request=dir.join("gui-request.json");if let Ok(bytes)=std::fs::read(&request){if let Ok(q)=serde_json::from_slice::<Value>(&bytes){let seq=q["seq"].as_u64().unwrap_or(0);if seq>sequence.get(){sequence.set(seq);let index=q["view"].as_u64().unwrap_or(0)as usize;let view=vv.borrow().get(index).cloned();let reply_dir=dir.clone();let reply=move|result:Value|{let path=reply_dir.join("gui-response.json");let temp=reply_dir.join("gui-response.tmp");let _=std::fs::write(&temp,json!({"seq":seq,"result":result}).to_string());let _=std::fs::rename(temp,path);};if view.is_none(){match q["action"].as_str().unwrap_or(""){
    "web-preview"=>reply(web::fixture(&tabs,&q)),
    "tab-status"=>reply(json!({"tabs":(0..tabs.n_pages()).map(|i|{let p=tabs.nth_page(i);let icon=p.indicator_icon().and_downcast::<gtk::gio::BytesIcon>();json!({"title":p.title().as_str(),"colour_icon":icon.is_some(),"svg":icon.map(|i|String::from_utf8_lossy(i.bytes().as_ref()).to_string())})}).collect::<Vec<_>>()})),
    "compact-controls"=>reply(compact::fixture(window.upcast_ref(),&q)),
    "chrome"=>reply(chrome.fixture(&window,&machine_shell.view,&q)),
    "rdp-tab-menu"=>reply(rdp::menu_fixture(&window,&tabs,&connection_actions,&q)),
    "detach-tab"=>reply(detached_windows.fixture(&tabs,&q)),
    "about"=>{let d=about::present(&window);if q["close"]==true{d.close();}reply(json!({"version":env!("CARGO_PKG_VERSION"),"donation_url":about::DONATION_URL,"qr_modules":about::QR.len(),"visible":d.is_visible()}));},
    "input-probe"=>reply(json!({"events":input_probe.get(),"active":window.is_active()})),
    "inspect"=>reply(json!({"display":gtk::prelude::WidgetExt::display(&window).type_().name(),"active":window.is_active(),"tabs":tabs.n_pages(),"selected_tab":tabs.selected_page().map(|p|tabs.page_position(&p)),"dark":app.style_manager().is_dark(),"focused_view":tiles::focused_id(&tabs,&focus_memory),"clipboard_owner":broker.borrow().active(),"terminal_views":0,"rdp_views":rdp_views.borrow().len(),"panes":[],"ticks":tick.get()})),
    "new-terminal"=>reply(json!({"clicked":sessions::fixture(&window,"New terminal")})),
    "workspace-close"=>{reply(json!({"closing":true}));window.close();},
    "primary-setup"=>{if q["operation"]=="open"{pref_button.emit_clicked();}reply(primary_setup::fixture(window.upcast_ref(),q["operation"]=="close"));},
    "updates"=>reply(updates::fixture(&window,&q)),
    _=>reply(json!({"error":"This terminal fixture action requires an open terminal view"})),
   }}else if let Some(v)=view{match q["action"].as_str().unwrap_or(""){
    "new-terminal"=>reply(json!({"clicked":sessions::fixture(&window,"New terminal")})),
    "sessions-open"=>{let label=q["session"].as_str().unwrap_or("Sessions…");let mut clicked=sessions::fixture(&window,label);if !clicked{for widget in gtk::Window::list_toplevels(){if let Ok(w)=widget.downcast::<gtk::Window>(){if w.title().as_deref()==Some("Persistent terminal sessions"){clicked=sessions::fixture(&w,label);}}}}reply(json!({"clicked":clicked}));},
    "primary-setup"=>{if q["operation"]=="open"{pref_button.emit_clicked();}reply(primary_setup::fixture(window.upcast_ref(),q["operation"]=="close"));},
    "updates"=>reply(updates::fixture(&window,&q)),
    "close-rdp"=>{let r=rdp_views.borrow().get(q["rdp"].as_u64().unwrap_or(0)as usize).cloned();if let Some(r)=r{if let Some(page)=tiles::page(&tabs,&r.root){tabs.close_page(&page);}}reply(json!({"rdp_views":rdp_views.borrow().len()}));},
    "close-tab"=>{let i=q["tab"].as_i64().unwrap_or(0)as i32;if i>=0&&i<tabs.n_pages(){let page=tabs.nth_page(i);tabs.close_page(&page);}reply(json!({"tabs":tabs.n_pages(),"selected_tab":tabs.selected_page().map(|p|tabs.page_position(&p)),"dark":app.style_manager().is_dark(),"focused_view":tiles::focused_id(&tabs,&focus_memory),"clipboard_owner":broker.borrow().active(),"terminal_views":vv.borrow().len()}));},
    "tab-select"=>{let i=q["tab"].as_i64().unwrap_or(0)as i32;if i>=0&&i<tabs.n_pages(){tabs.set_selected_page(&tabs.nth_page(i));}reply(json!({"tabs":tabs.n_pages()}));},
    "focus-view"=>{if let Some(page)=tiles::page(&tabs,&v.root){tabs.set_selected_page(&page);}v.focus();reply(json!({"focused":v.session}));},
    "stage-png"=>{image_button.grab_focus();image_button.emit_clicked();reply(json!({"queued":true,"native_toolbar":true}));},
    "mixed-join"=>{reply(match tiles::join_focused(&tabs,q["tab"].as_i64().unwrap_or(-1)as i32,q["vertical"]==true,&focus_memory,&focus_view){Ok(())=>tiles::inspect(&tabs,&focus_memory),Err(e)=>json!({"error":e})});},
    "mixed-separate"=>{reply(match tiles::separate_focused(&tabs,&focus_memory,&focus_view){Ok(_)=>tiles::inspect(&tabs,&focus_memory),Err(e)=>json!({"error":e})});},
    "rdp-permission"=>{remote_consent.set_active(q["enabled"]==true);reply(json!({"rdp_lifecycle_control":remote_policy.get(),"native_checkbutton":true}));},
    "host-shortcuts"=>{if q["operation"]=="context"{v.click(20.0,20.0,3,gtk::gdk::ModifierType::SHIFT_MASK,false);}reply(host_shortcuts::fixture(&window,&q));},
    "agent-launch-dialog"=>{if q["operation"]=="show"{sessions::fixture(&window,"Open remote agent…");}reply(agent_launch::fixture(&q));},
    "remote-image-dialog"=>reply(remote_image::fixture(&window,&q)),
    "ssh-image-paste"=>reply(remote_paste::fixture(&v,&q)),
    "launch-contexts"=>reply(launches::inspect()),
    "launch-focus-race"=>reply(v.fixture_focus_launch_race(&q)),
    "input-probe"=>reply(json!({"events":input_probe.get(),"active":window.is_active()})),
    "workspace-picker"=>{if q["operation"]=="show"{reply(json!({"clicked":sessions::fixture(&window,"Workspaces…")}));}else{reply(workspace_picker::fixture(q["operation"].as_str().unwrap_or("inspect"),q["value"].as_str()));}},
    "workspace-permission"=>{consent.set_active(q["enabled"]==true);reply(json!({"focus_control":control_policy.get(),"native_checkbutton":true}));},
    "ipc-delay"=>{reply(json!({"synthetic_main_loop_delay":true}));std::thread::sleep(Duration::from_millis(q["ms"].as_u64().unwrap_or(0).min(4000)));},
    "view-focus"=>{reply(match tiles::focus_id(&tabs,q["id"].as_str().unwrap_or(""),&focus_view){Ok(())=>tiles::inspect(&tabs,&focus_memory),Err(e)=>json!({"error":e})});},
    "mixed-inspect"=>reply(tiles::inspect(&tabs,&focus_memory)),
    "views-manage"=>reply(json!({"clicked":sessions::fixture(&window,"Views…")})),
    "views-action"=>reply(json!({"clicked":tiles::fixture(q["operation"].as_str().unwrap_or(""),q["index"].as_u64().unwrap_or(0)as usize),"layout":tiles::inspect(&tabs,&focus_memory),"terminal_views":vv.borrow().len(),"rdp_views":rdp_views.borrow().len()})),
    "mixed-focus"=>{if q["kind"]=="rdp"{if let Some(r)=rdp_views.borrow().get(q["index"].as_u64().unwrap_or(0)as usize){r.focus();}}else if let Some(v)=vv.borrow().get(q["index"].as_u64().unwrap_or(0)as usize){v.focus();}reply(json!({"owner":broker.borrow().active()}));},
    "workspace-close"=>{reply(json!({"closing":true}));window.close();},
    "switch"=>{v.show_terminal();if let Some(page)=tiles::page(&tabs,&v.root){tabs.set_selected_page(&page);}if let Some(pane)=q["pane"].as_str(){*v.target.borrow_mut()=pane.into();v.command("pane.select",json!({"pane":pane}));}v.focus();if let Some(text)=q["text"].as_str(){for c in text.chars(){let key=unsafe{glib::translate::from_glib(gdk::ffi::gdk_unicode_to_keyval(c as u32))};v.key(key,gdk::ModifierType::empty());}}reply(json!({"queued":true}));},
    "rdp-open"=>{rdp_button.emit_clicked();if let Some(r)=rdp_views.borrow().last(){if let Ok(profile)=serde_json::from_value::<profiles::Profile>(q["profile"].clone()){if profile.protocol=="RDP" && profiles::endpoint(&profile.server,3389).is_ok_and(|(host,_)|host=="127.0.0.1"){r.configure(&profile);}}if let Some(port)=q["port"].as_u64().filter(|p|*p>0&&*p<=65535){r.fixture_connect(port as u16,&dir.join("cert.pem"));}else{r.fixture_frame();}}reply(json!({"tabs":tabs.n_pages(),"selected_tab":tabs.selected_page().map(|p|tabs.page_position(&p)),"dark":app.style_manager().is_dark(),"focused_view":tiles::focused_id(&tabs,&focus_memory),"clipboard_owner":broker.borrow().active(),"backend":"IronRDP","synthetic_frame":q["port"].is_null()}));},
    "rdp-assist"=>{if let Some(r)=rdp_views.borrow().get(q["rdp"].as_u64().unwrap_or(0)as usize){reply(r.fixture_assist(&q));}else{reply(json!({"error":"RDP tab missing"}));}},
    "rdp-inspect"|"rdp-switch"|"rdp-send"|"rdp-receive"|"rdp-policy"=>{if let Some(r)=rdp_views.borrow().get(q["rdp"].as_u64().unwrap_or(0)as usize){match q["action"].as_str().unwrap_or(""){"rdp-switch"=>{if let Some(page)=tiles::page(&tabs,&r.root){tabs.set_selected_page(&page);}r.focus();},"rdp-policy"=>r.fixture_policy(q["policy"].as_u64().unwrap_or(0)as u32),"rdp-send"=>r.fixture_clipboard(false),"rdp-receive"=>r.fixture_clipboard(true),_=>()}let mut result=r.fixture_inspect();result["tab"]=tiles::page(&tabs,&r.root).map(|p|json!(tabs.page_position(&p))).unwrap_or(Value::Null);reply(result);}else{reply(json!({"error":"RDP tab missing"}));}},
    "rdp-trust"=>{if let Some(r)=rdp_views.borrow().get(q["rdp"].as_u64().unwrap_or(0)as usize){reply(r.fixture_trust(&q));}else{reply(json!({"error":"RDP tab missing"}));}},
    "rdp-toolbar"=>{if let Some(r)=rdp_views.borrow().get(q["rdp"].as_u64().unwrap_or(0)as usize){reply(r.fixture_toolbar(q["control"].as_str()));}else{reply(json!({"error":"RDP tab missing"}));}},
    "rdp-pointer"=>{if let Some(r)=rdp_views.borrow().get(q["rdp"].as_u64().unwrap_or(0)as usize){reply(r.fixture_pointer(&q));}},
    "rdp-display"=>{if let Some(v)=rdp_views.borrow().get(q["rdp"].as_u64().unwrap_or(0) as usize){if let(Some(w),Some(h))=(q["window_width"].as_i64(),q["window_height"].as_i64()){v.fixture_window(w.clamp(700,3200)as i32,h.clamp(500,2000)as i32);}
v.set_display(q["mode"].as_str().unwrap_or("fit"),q["zoom"].as_f64().unwrap_or(1.0),q["width"].as_u64().unwrap_or(0) as u16,q["height"].as_u64().unwrap_or(0) as u16);reply(v.fixture_inspect());}},
    "files-open"=>{match serde_json::from_value::<profiles::Profile>(q["profile"].clone()){Ok(profile) if profiles::endpoint(&profile.server,22).is_ok_and(|(host,_)|host=="127.0.0.1")=>{let panel=files::panel(profile.clone());file_views.borrow_mut().push((panel.clone(),profile));let page=tiles::append(&tabs,&panel);page.set_title("Fixture · Files");tabs.set_selected_page(&page);reply(files::fixture(&panel,q["path"].as_str(),true));},_=>reply(json!({"error":"Loopback fixture profile required"}))}},
    "files-directory"=>{if let Some((root,_))=file_views.borrow().get(q["index"].as_u64().unwrap_or(0)as usize){reply(json!({"directory":files::directory(root,q["path"].as_str())}));}else{reply(json!({"error":"Files view missing"}));}},
    "files-inspect"=>{if let Some(page)=tabs.selected_page(){if page.title().ends_with("Files"){if let Some(root)=page.child().first_child().and_then(|c|c.downcast::<gtk::Box>().ok()){reply(files::fixture(&root,q["path"].as_str(),q["refresh"]==true));}}}},
    "files-rsync-recovery"=>{if let Some(page)=tabs.selected_page(){if let Some((root,profile))=file_views.borrow().iter().find(|(root,_)|tiles::contains(&page.child(),root)){if profiles::endpoint(&profile.server,22).is_ok_and(|(host,_)|host=="127.0.0.1"){reply(files::fixture_recovery(root,&q));}else{reply(json!({"error":"Loopback fixture profile required"}));}}}},
    "files-rsync"=>{let local=q["local"].as_str().map(std::path::Path::new);if let Some(local)=local.filter(|p|!p.is_absolute()&&!p.components().any(|c|matches!(c,std::path::Component::ParentDir))){let path=dir.join(local);if path.canonicalize().ok().zip(dir.canonicalize().ok()).is_some_and(|(p,d)|p.starts_with(d)){if let Some(page)=tabs.selected_page(){if let Some((root,profile))=file_views.borrow().iter().find(|(root,_)|tiles::contains(&page.child(),root)){if profiles::endpoint(&profile.server,22).is_ok_and(|(host,_)|host=="127.0.0.1"){let mut request=q.clone();request["local"]=json!(path.to_string_lossy());reply(files::fixture_rsync(root,&request));}else{reply(json!({"error":"Loopback fixture profile required"}));}}}}else{reply(json!({"error":"Owned fixture path missing"}));}}else{reply(json!({"error":"Owned fixture-relative path required"}));}},
    "files-transfer"=>{if let Some(page)=tabs.selected_page(){if page.title().ends_with("Files"){if let Some(root)=page.child().first_child().and_then(|c|c.downcast::<gtk::Box>().ok()){reply(files::fixture_transfer(&root,&q));}}}},
    "jobs"=>{if q["operation"]=="open"{job_button.emit_clicked();}reply(jobs::fixture(&q));},
    "window-close"=>{reply(json!({"closing":true}));window.close();},
    "sidebar-shell"=>reply(machine_shell.fixture(&q)),
    "profile-editor"=>reply(profiles::fixture_editor(&sidebar,&q)),
    "profiles-control"=>{let path=q["file"].as_str().filter(|s|!s.contains('/')&&!s.contains("..")).map(|s|dir.join(s));reply(profiles::fixture_library(&sidebar,path.as_deref(),&q));},
    "profiles"=>{let path=q["file"].as_str().filter(|s|!s.contains('/')&&!s.contains("..")).map(|s|dir.join(s));reply(profiles::fixture(&sidebar,path.as_deref(),q["search"].as_str(),q["favorite"]==true,q["open"]==true));},
    "command"=>{v.run_command(q["text"].as_str().unwrap_or(""));reply(json!({"queued":true}));},
    "divider-drag"=>{reply(v.fixture_root_drag());},
    "reconnect"=>{reply(v.fixture_reconnect());},
    "search"=>{v.search.set_text(q["text"].as_str().unwrap_or(""));reply(json!({"queued":true}));},
    "history-viewport"=>reply(v.fixture_history_viewport()),
    "history-buffer"=>{reply(json!({"clicked":v.fixture_history_buffer(q["buffer"]=="primary")}));},
    "copy-state"=>{let b=v.copy.buffer();let text=v.selected();use sha2::{Digest,Sha256};let digest=format!("{:x}",Sha256::digest(b.text(&b.start_iter(),&b.end_iter(),true).as_bytes()));reply(json!({"origin":v.copy.compute_point(&window,&gtk::graphene::Point::new(0.,0.)).map(|p|[p.x(),p.y()]),"size":[v.copy.width(),v.copy.height()],"selection":text,"visible":v.copy.is_mapped(),"search":v.search.text().as_str(),"contains_ready":b.text(&b.start_iter(),&b.end_iter(),true).contains("RUWAC"),"text_sha256":digest,"chars":b.char_count(),"cursor":b.iter_at_mark(&b.get_insert()).offset()}));},
    "key"=>{if let Some(key)=q["key"].as_str().and_then(gdk::Key::from_name){let mods=gdk::ModifierType::from_bits_truncate(q["mods"].as_u64().unwrap_or(0)as u32);v.key(key,mods);}reply(json!({"handler":"synthetic GTK callback"}));},
    "copy"=>{v.select_fixture();v.copy_clipboard(q["primary"]==true);reply(json!({"offered":true}));},
    "paste"=>{v.paste_clipboard(q["primary"]==true);reply(json!({"async":true}));},
    "read-text"=>{let epoch=v.broker.borrow().epoch();let primary=q["primary"]==true;let display=v.area.display();let buffer=if primary{display.primary_clipboard()}else{display.clipboard()};let scope=if primary{ruwac_clipboard::ClipScope::Primary}else{ruwac_clipboard::ClipScope::Clipboard};clipboard::read_text(&buffer,v.broker.clone(),epoch,scope,Duration::from_millis(q["delay_ms"].as_u64().unwrap_or(0).min(100)),move|r|{use sha2::{Digest,Sha256};reply(match r{Ok(t)=>json!({"bytes":t.len(),"sha256":format!("{:x}",Sha256::digest(t.as_bytes()))}),Err(e)=>json!({"error":e})});});},
    "attach-image-button"=>reply(json!({"clicked":sessions::fixture(&window,"Attach PNG to Codex")})),
    "attach-image"=>{if let Some(stage)=staging.clone(){let(t,root)=(tabs.downgrade(),v.root.downgrade());let selected:Rc<dyn Fn()->bool>=Rc::new(move||t.upgrade().zip(root.upgrade()).is_some_and(|(t,r)|t.selected_page().is_some_and(|p|tiles::contains(&p.child(),&r))));image_attach::clipboard(&v,stage,selected,Duration::from_millis(q["delay_ms"].as_u64().unwrap_or(0).min(100)),move|result|reply(match result{Ok(v)=>v,Err(error)=>json!({"error":error})}));}else{reply(json!({"error":"Private stage required"}));}},
    "read-image"=>{if let Some(stage)=staging.clone(){let epoch=v.broker.borrow().epoch();let owned=owned.clone();clipboard::ingest_png(&v.area.display().clipboard(),stage,v.broker.clone(),epoch,q["limit"].as_u64().unwrap_or(ruwac_clipboard::MAX_IMAGE_BYTES as u64)as usize,Duration::from_millis(q["delay_ms"].as_u64().unwrap_or(0).min(100)),move|r|reply(match r{Ok(r)=>{owned.borrow_mut().push(r.path.clone());json!({"path":r.path,"sha256":r.sha256,"bytes":r.bytes,"width":r.width,"height":r.height})},Err(e)=>json!({"error":e})}));}else{reply(json!({"error":"staging directory required"}));}},
    "click"=>{v.click(q["x"].as_f64().unwrap_or(1.0),q["y"].as_f64().unwrap_or(1.0),q["button"].as_u64().unwrap_or(1)as u32,gdk::ModifierType::from_bits_truncate(q["mods"].as_u64().unwrap_or(0)as u32),q["release"]==true);reply(json!({"handler":"synthetic GTK click dispatch"}));},
    "mouse"=>{v.mouse(q["button"].as_u64().unwrap_or(0)as u32,q["x"].as_f64().unwrap_or(1.0),q["y"].as_f64().unwrap_or(1.0),q["release"]==true);reply(json!({"handler":"synthetic mouse callback"}));},
    "delay-queue"=>{v.command("pane.wait",json!({"pane":*v.target.borrow(),"contains":"not-present-delayed-fixture","timeout_ms":1000}));for _ in 0..160{v.command("pane.select",json!({"pane":*v.target.borrow()}));}reply(json!({"requested":160}));},
    "policy"=>{v.broker.borrow_mut().set_desktop_disabled(q["disabled"]==true);reply(json!({"disabled":q["disabled"]==true}));},
    "grid-copy"=>{v.select_grid_fixture();v.copy_clipboard(false);reply(json!({"offered":true}));},
    "cancel"=>{v.broker.borrow_mut().local_copy();reply(json!({"epoch":v.broker.borrow().epoch()}));},
    "snapshot"=>{if let Some(clock)=window.frame_clock(){let paint=gtk::WidgetPaintable::new(Some(&window));let window=window.clone();let dir=dir.clone();let slot=Rc::new(RefCell::new(None));let owned=slot.clone();let attempts=Rc::new(Cell::new(0u8));window.queue_draw();let id=clock.connect_after_paint(move|clock|{attempts.set(attempts.get()+1);let snap=gtk::Snapshot::new();paint.snapshot(&snap,window.width()as f64,window.height()as f64);let node=snap.to_node();let renderer=window.renderer();let mut saved=false;if let(Some(node),Some(renderer))=(&node,&renderer){let texture=renderer.render_texture(node,None);saved=texture.save_to_png(dir.join("native-rust.png")).is_ok();}if saved||attempts.get()>=5{if let Some(id)=owned.borrow_mut().take(){clock.disconnect(id);}reply(json!({"image":"native-rust.png","saved":saved,"mapped":window.is_mapped(),"width":window.width(),"height":window.height(),"node":node.is_some(),"renderer":renderer.is_some(),"after_paint":true}));}else{window.queue_draw();}});*slot.borrow_mut()=Some(id);}else{reply(json!({"saved":false,"error":"Frame clock unavailable"}));}},
    "web-preview"=>reply(web::fixture(&tabs,&q)),
    "tab-status"=>reply(json!({"tabs":(0..tabs.n_pages()).map(|i|{let p=tabs.nth_page(i);let icon=p.indicator_icon().and_downcast::<gtk::gio::BytesIcon>();json!({"title":p.title().as_str(),"colour_icon":icon.is_some(),"svg":icon.map(|i|String::from_utf8_lossy(i.bytes().as_ref()).to_string())})}).collect::<Vec<_>>()})),
    "compact-controls"=>reply(compact::fixture(window.upcast_ref(),&q)),
    "chrome"=>reply(chrome.fixture(&window,&machine_shell.view,&q)),
    "rdp-tab-menu"=>reply(rdp::menu_fixture(&window,&tabs,&connection_actions,&q)),
    "detach-tab"=>reply(detached_windows.fixture(&tabs,&q)),
    "about"=>{let d=about::present(&window);if q["close"]==true{d.close();}reply(json!({"version":env!("CARGO_PKG_VERSION"),"donation_url":about::DONATION_URL,"qr_modules":about::QR.len(),"visible":d.is_visible()}));},
    "inspect"=>reply(json!({"tab_status":format!("{:?}",v.tab_status()),"display":v.area.display().type_().name(),"active":window.is_active(),"tabs":tabs.n_pages(),"selected_tab":tabs.selected_page().map(|p|tabs.page_position(&p)),"dark":app.style_manager().is_dark(),"focused_view":tiles::focused_id(&tabs,&focus_memory),"clipboard_owner":broker.borrow().active(),"cell_metrics":v.cell_metrics(),"revision":v.frame.borrow().as_ref().and_then(|f|f["revision"].as_u64()),"area_size":[v.area.width(),v.area.height()],"area_origin":v.area.compute_point(&window,&gtk::graphene::Point::new(0.,0.)).map(|p|[p.x(),p.y()]),"grid_size":v.frame.borrow().as_ref().and_then(|f|f["panes"].as_array()).and_then(|p|p.first()).map(|p|[p["grid"]["cols"].as_u64().unwrap_or(0),p["grid"]["rows"].as_u64().unwrap_or(0)]),"layout":v.frame.borrow().as_ref().map(|f|f["layout"].clone()),"zoom":v.frame.borrow().as_ref().map(|f|f["zoom"].clone()),"target":*v.target.borrow(),"drawn":vv.borrow().iter().map(|v|v.drawn.get()).collect::<Vec<_>>(),"panes":vv.borrow().iter().map(|v|v.frame.borrow().as_ref().and_then(|f|f["panes"].as_array()).map(|a|a.len()).unwrap_or(0)).collect::<Vec<_>>(),"ticks":tick.get(),"max_tick_gap_ms":max_gap.get(),"acknowledgements":v.worker.borrow().state.lock().unwrap().acknowledgements,"status":v.status.text().to_string()})),
    "quit"=>{for v in vv.borrow().iter(){v.close();}for path in owned.borrow_mut().drain(..){let _=std::fs::remove_file(path);}reply(json!({"detached":true}));app.quit();return glib::ControlFlow::Break;},
    _=>reply(json!({"error":"unknown fixture action"}))
   }}else{reply(json!({"error":"view not found"}));}}}}}
   glib::ControlFlow::Continue
  });
 });
    app.connect_shutdown(|_| remote_paste_upload::shutdown());
    app.run_with_args::<&str>(&[]);
    for (server, source) in services.borrow_mut().drain(..) {
        source.remove();
        drop(server);
    }
    for view in views.borrow().iter() {
        view.close();
    }
    Ok(())
}
