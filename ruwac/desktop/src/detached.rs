//! Tab transfers reparent the live widget tree; PTYs and RDP transports stay alive.
use crate::{clipboard::Broker, rdp, sessions, shortcuts, system_shortcuts, tiles, view::View};
use adw::prelude::*;
use gtk::glib;
use std::{cell::RefCell, path::PathBuf, rc::Rc, time::Duration};

pub struct Windows {
    app: glib::WeakRef<adw::Application>,
    entries: RefCell<
        Vec<(
            glib::WeakRef<adw::ApplicationWindow>,
            glib::WeakRef<adw::TabView>,
            tiles::FocusMemory,
        )>,
    >,
    terminal: Rc<RefCell<Vec<Rc<View>>>>,
    remote: Rc<RefCell<Vec<Rc<rdp::RdpView>>>>,
    broker: Broker,
    socket: PathBuf,
    record: RefCell<Option<crate::workspace::Recorder>>,
    close_auxiliary: tiles::CloseView,
}
impl Windows {
    pub fn new(
        app: &adw::Application,
        terminal: Rc<RefCell<Vec<Rc<View>>>>,
        remote: Rc<RefCell<Vec<Rc<rdp::RdpView>>>>,
        broker: Broker,
        socket: PathBuf,
        close_auxiliary: tiles::CloseView,
    ) -> Rc<Self> {
        Rc::new(Self {
            app: app.downgrade(),
            entries: RefCell::new(Vec::new()),
            terminal,
            remote,
            broker,
            socket,
            record: RefCell::new(None),
            close_auxiliary,
        })
    }
    pub fn set_recorder(&self, recorder: crate::workspace::Recorder) {
        *self.record.borrow_mut() = Some(recorder);
    }
    pub fn active_window(&self) -> Option<adw::ApplicationWindow> {
        self.app
            .upgrade()?
            .active_window()?
            .downcast::<adw::ApplicationWindow>()
            .ok()
            .filter(|w| w.is_active())
    }
    pub fn page(&self, root: &impl IsA<gtk::Widget>) -> Option<adw::TabPage> {
        self.tabs().iter().find_map(|(t, _)| tiles::page(t, root))
    }
    pub fn inspect(&self) -> serde_json::Value {
        let entries = self.tabs();
        let active = self.active_window();
        let chosen = entries
            .iter()
            .find(|(t, _)| {
                active
                    .as_ref()
                    .is_some_and(|w| t.root().as_ref() == Some(w.upcast_ref()))
            })
            .or_else(|| entries.first());
        let mut value = chosen
            .map(|(t, m)| tiles::inspect(t, m))
            .unwrap_or_else(|| serde_json::json!({}));
        let windows: Vec<_> = entries.iter().map(|(t, m)| tiles::inspect(t, m)).collect();
        value["tabs"] = serde_json::json!(
            windows
                .iter()
                .flat_map(|w| w["tabs"].as_array().into_iter().flatten().cloned())
                .collect::<Vec<_>>()
        );
        value["windows"] = serde_json::json!(windows);
        value
    }
    pub fn focus_id(
        self: &Rc<Self>,
        id: &str,
        tab: bool,
    ) -> Result<serde_json::Value, &'static str> {
        for (tabs, memory) in self.tabs() {
            let page = if tab {
                (0..tabs.n_pages()).map(|i| tabs.nth_page(i)).find(|p| {
                    ruwac_protocol::valid_id(id, "tab") && p.child().widget_name().as_str() == id
                })
            } else {
                tiles::views(&tabs)
                    .into_iter()
                    .find(|v| {
                        ruwac_protocol::valid_id(id, "view") && v.widget_name().as_str() == id
                    })
                    .and_then(|v| tiles::page(&tabs, &v))
            };
            if let Some(page) = page {
                // An explicit, permission-checked focus request must activate
                // the window that now owns this view after a tab transfer.
                if let Some(window) = tabs.root().and_downcast::<gtk::Window>() {
                    window.present();
                }
                tabs.set_selected_page(&page);
                let group = self.clone();
                let focus: tiles::FocusView = Rc::new(move |root| group.focus(root));
                if tab {
                    tiles::focus_selected(&tabs, &memory, &focus);
                } else {
                    tiles::focus_id(&tabs, id, &focus).map_err(|_| "View closed")?;
                }
                return Ok(
                    serde_json::json!({"selected_tab":page.child().widget_name().as_str(),"focused_view":tiles::focused_id(&tabs,&memory)}),
                );
            }
        }
        Err("Tab or view ID is invalid or closed")
    }
    pub fn tabs(&self) -> Vec<(adw::TabView, tiles::FocusMemory)> {
        self.entries
            .borrow()
            .iter()
            .filter_map(|(_, t, m)| Some((t.upgrade()?, m.clone())))
            .filter(|(t, _)| {
                t.root()
                    .and_downcast::<gtk::Window>()
                    .is_some_and(|w| w.is_visible())
            })
            .collect()
    }
    pub fn bind(
        self: &Rc<Self>,
        window: &adw::ApplicationWindow,
        tabs: &adw::TabView,
        memory: tiles::FocusMemory,
    ) {
        self.entries
            .borrow_mut()
            .push((window.downgrade(), tabs.downgrade(), memory));
        // Native libadwaita implements the drag, transfer and cancellation. The
        // return value is the destination for this exact existing TabPage.
        let group = self.clone();
        tabs.connect_create_window(move |_| group.create());
    }
    fn focus(self: &Rc<Self>, root: &gtk::Widget) {
        for (tabs, memory) in self.tabs() {
            if tiles::page(&tabs, root).is_some() {
                tiles::remember(&tabs, &memory, root);
                break;
            }
        }
        if let Some(v) = self
            .terminal
            .borrow()
            .iter()
            .find(|v| v.root.upcast_ref::<gtk::Widget>() == root)
        {
            v.focus();
        } else if let Some(v) = self
            .remote
            .borrow()
            .iter()
            .find(|v| v.root.upcast_ref::<gtk::Widget>() == root)
        {
            v.focus();
        } else {
            root.child_focus(gtk::DirectionType::TabForward);
        }
    }
    fn sync_focus(&self, window: &adw::ApplicationWindow) {
        let focus = gtk::prelude::GtkWindowExt::focus(window);
        if !window.is_active() {
            for v in self
                .remote
                .borrow()
                .iter()
                .filter(|v| v.root.root().as_ref() == Some(window.upcast_ref()))
            {
                v.inactive();
            }
            return;
        }
        self.broker.borrow_mut().focus(None);
        for v in self.remote.borrow().iter() {
            if focus.as_ref().is_some_and(|f| v.owns_focus(f)) {
                v.activate_clipboard();
            } else {
                v.inactive();
            }
        }
        if let Some(v) = self
            .terminal
            .borrow()
            .iter()
            .find(|v| focus.as_ref().is_some_and(|f| tiles::contains(&v.root, f)))
        {
            self.broker.borrow_mut().focus(Some(&v.session));
        }
    }
    fn close_tree(&self, root: &gtk::Widget) {
        self.terminal.borrow_mut().retain(|v| {
            if tiles::contains(root, &v.root) {
                v.close();
                false
            } else {
                true
            }
        });
        self.remote.borrow_mut().retain(|v| {
            if tiles::contains(root, &v.root) {
                v.close();
                false
            } else {
                true
            }
        });
        (self.close_auxiliary)(root);
    }
    pub fn close_window(&self, window: &adw::ApplicationWindow) {
        for (tabs, _) in self
            .tabs()
            .into_iter()
            .filter(|(t, _)| t.root().as_ref() == Some(window.upcast_ref()))
        {
            for i in 0..tabs.n_pages() {
                self.close_tree(&tabs.nth_page(i).child());
            }
        }
    }
    pub fn create(self: &Rc<Self>) -> Option<adw::TabView> {
        let app = self.app.upgrade()?;
        let window = adw::ApplicationWindow::builder()
            .application(&app)
            .title(format!("RUWAC {}", env!("CARGO_PKG_VERSION")))
            .default_width(1040)
            .default_height(780)
            .build();
        window.set_icon_name(Some("si.z4.ruwac"));
        window.add_css_class("ruwac");
        let body = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let header = adw::HeaderBar::new();
        let tabs = adw::TabView::new();
        tabs.set_vexpand(true);
        tabs.remove_shortcuts(adw::TabViewShortcuts::ALT_DIGITS | adw::TabViewShortcuts::ALT_ZERO);
        let bar = adw::TabBar::builder()
            .view(&tabs)
            .autohide(false)
            .expand_tabs(true)
            .build();
        bar.add_css_class("ruwac-tabs");
        header.set_title_widget(Some(&bar));
        body.append(&header);
        body.append(&tabs);
        window.set_content(Some(&body));
        let memory: tiles::FocusMemory = Rc::new(RefCell::new(Default::default()));
        self.bind(&window, &tabs, memory.clone());
        let group = self.clone();
        let focus: tiles::FocusView = Rc::new(move |root| group.focus(root));
        let (m, f) = (memory.clone(), focus.clone());
        tabs.connect_selected_page_notify(move |t| tiles::focus_selected(t, &m, &f));
        let (m, f) = (memory.clone(), focus.clone());
        tabs.connect_page_attached(move |t, _, _| tiles::focus_selected(t, &m, &f));
        let group = self.clone();
        tabs.connect_close_page(move |_, p| {
            group.close_tree(&p.child());
            glib::Propagation::Proceed
        });
        let group = self.clone();
        window.connect_focus_widget_notify(move |w| group.sync_focus(w));
        let group = self.clone();
        window.connect_is_active_notify(move |w| group.sync_focus(w));
        let group = self.clone();
        let t = tabs.clone();
        window.connect_close_request(move |_| {
            if let Some(record) = group.record.borrow().as_ref() {
                if let Ok(snapshot) = record() {
                    let _ = crate::workspace::save(&snapshot);
                }
            }
            for i in 0..t.n_pages() {
                group.close_tree(&t.nth_page(i).child());
            }
            glib::Propagation::Proceed
        });
        if let Some(record) = self.record.borrow().as_ref() {
            crate::workspace::watch(&window, record.clone());
        }
        rdp::install_tab_menu(&window, &tabs, self.remote.clone(), self.terminal.clone());
        let controls = gtk::MenuButton::builder()
            .icon_name("open-menu-symbolic")
            .tooltip_text("Terminal and remote desktop controls")
            .build();
        let pop = gtk::Popover::new();
        let actions = gtk::Box::new(gtk::Orientation::Vertical, 6);
        actions.append(&sessions::controls(
            tabs.clone(),
            self.terminal.clone(),
            self.socket.clone(),
            self.broker.clone(),
        ));
        pop.set_child(Some(&actions));
        controls.set_popover(Some(&pop));
        header.pack_end(&controls);
        let (t, g) = (tabs.clone(), self.clone());
        let c: tiles::CloseView = Rc::new(move |root| {
            g.close_tree(root);
            if let Some(page) = t.selected_page() {
                g.focus(&page.child());
            }
        });
        actions.append(&tiles::controls(&tabs, c, focus.clone(), memory.clone()));
        // Toolbar controls follow the selected RDP/terminal into this window.
        let (t, g, host) = (tabs.downgrade(), self.clone(), actions.clone());
        let mut previous = String::new();
        glib::timeout_add_local(Duration::from_millis(100), move || {
            let Some(t) = t.upgrade().filter(|t| t.root().is_some()) else {
                return glib::ControlFlow::Break;
            };
            if let Some(page) = t.selected_page() {
                let id = page.child().widget_name().to_string();
                if previous != id {
                    let remote = g.remote.borrow();
                    let terminal = g.terminal.borrow();
                    let controls = remote
                        .iter()
                        .find(|v| tiles::contains(&page.child(), &v.root))
                        .map(|v| &v.toolbar)
                        .or_else(|| {
                            terminal
                                .iter()
                                .find(|v| tiles::contains(&page.child(), &v.root))
                                .map(|v| &v.history_controls)
                        });
                    if let Some(controls) = controls {
                        crate::chrome::detach(controls);
                        host.append(controls);
                    }
                    previous = id;
                }
            }
            glib::ControlFlow::Continue
        });
        let grab = system_shortcuts::Grab::new(&window, {
            let remote = self.remote.clone();
            move |f| remote.borrow().iter().any(|r| r.owns_focus(f))
        });
        let g = grab.clone();
        window.connect_focus_widget_notify(move |_| g.update());
        let g = grab.clone();
        window.connect_is_active_notify(move |_| g.update());
        let mode = Rc::new(shortcuts::LocalMode::default());
        shortcuts::bind_mode(window.upcast_ref(), &mode);
        let keys = gtk::EventControllerKey::new();
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        let (m, g) = (mode.clone(), grab.clone());
        keys.connect_key_released(move |_, _, code, _| {
            if let Some(mapped) = m.release(code) {
                g.set_local(!mapped);
            }
        });
        let (t, w, c) = (tabs.clone(), window.downgrade(), controls.clone());
        keys.connect_key_pressed(move |_, key, code, mods| {
            mode.press(key, code);
            if !mode.active() {
                if let Some(index) = shortcuts::alt_tab_index(key, mods) {
                    shortcuts::select_tab(&t, index);
                    return glib::Propagation::Stop;
                }
            }
            if shortcuts::fullscreen(key, mods) {
                if let Some(w) = w.upgrade() {
                    if w.is_fullscreen() {
                        w.unfullscreen();
                    } else {
                        w.fullscreen();
                    }
                }
                return glib::Propagation::Stop;
            }
            if shortcuts::local_escape(key, mods) || crate::chrome::controls_shortcut(key, mods) {
                c.popup();
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
        window.add_controller(keys);
        window.present();
        Some(tabs)
    }
    pub fn fixture(
        self: &Rc<Self>,
        from: &adw::TabView,
        q: &serde_json::Value,
    ) -> serde_json::Value {
        match q["operation"].as_str().unwrap_or("inspect") {
            "detach" => {
                let i = q["tab"].as_i64().unwrap_or(0) as i32;
                if i >= 0 && i < from.n_pages() {
                    if let Some(to) =
                        from.emit_by_name::<Option<adw::TabView>>("create-window", &[])
                    {
                        from.transfer_page(&from.nth_page(i), &to, 0);
                    }
                }
            }
            "return" => {
                let list = self.tabs();
                if let Some((source, _)) = list.get(q["window"].as_u64().unwrap_or(1) as usize) {
                    if source != from && source.n_pages() > 0 {
                        source.transfer_page(&source.nth_page(0), from, from.n_pages());
                    }
                }
            }
            "close" => {
                if let Some((w, _, _)) = self
                    .entries
                    .borrow()
                    .get(q["window"].as_u64().unwrap_or(1) as usize)
                {
                    if let Some(w) = w.upgrade() {
                        w.close();
                    }
                }
            }
            _ => (),
        }
        serde_json::json!({"windows":self.tabs().iter().map(|(t,m)|tiles::inspect(t,m)).collect::<Vec<_>>()})
    }
}
