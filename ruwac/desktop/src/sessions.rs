use crate::{clipboard::Broker, view::View, worker::Rpc};
use adw::prelude::*;
use gtk::glib;
use serde_json::{Value, json};
use std::{cell::RefCell, path::PathBuf, rc::Rc, time::Duration};

pub fn append(
    tabs: &adw::TabView,
    views: &Rc<RefCell<Vec<Rc<View>>>>,
    socket: &PathBuf,
    broker: &Broker,
    id: &str,
    name: &str,
) {
    if let Some(v) = views.borrow().iter().find(|v| v.session == id).cloned() {
        if let Some(page) = crate::tiles::page(tabs, &v.root) {
            tabs.set_selected_page(&page);
        }
        v.focus();
        return;
    }
    let v = View::new(socket.clone(), id.into(), broker.clone());
    let page = crate::tiles::append(tabs, &v.root);
    page.set_title(&terminal_title(name));
    views.borrow_mut().push(v.clone());
    tabs.set_selected_page(&page);
    v.focus();
}
/// Create the replacement before closing an ended view, including the only tab.
pub fn install_recovery(
    window: &adw::ApplicationWindow,
    tabs: &adw::TabView,
    views: &Rc<RefCell<Vec<Rc<View>>>>,
    socket: &PathBuf,
    broker: &Broker,
) {
    let group = gtk::gio::SimpleActionGroup::new();
    let busy = Rc::new(std::cell::Cell::new(false));
    for (name, replace) in [("new", false), ("replace", true)] {
        let action = gtk::gio::SimpleAction::new(name, Some(&String::static_variant_type()));
        let (t, v, s, b) = (
            tabs.downgrade(),
            views.clone(),
            socket.clone(),
            broker.clone(),
        );
        let busy = busy.clone();
        action.connect_activate(move |_, parameter| {
            let Some(old) = parameter.and_then(|p| p.str()).map(str::to_owned) else { return; };
            let Some(tabs) = t.upgrade() else { return; };
            let previous = v.borrow().iter().find(|view| view.session == old && crate::tiles::page(&tabs, &view.root).is_some()).cloned();
            let Some(previous) = previous else { return; };
            if busy.replace(true) { return; }
            let (tx, rx) = std::sync::mpsc::sync_channel(1);
            let worker_socket = s.clone();
            std::thread::spawn(move || {
                let result = (|| {
                    let mut rpc = Rpc::connect(&worker_socket)?;
                    rpc.call("session.create", json!({"name":"Terminal","argv":crate::shell::default_terminal_argv().map_err(std::io::Error::other)?}))
                })();
                let _ = tx.send(result);
            });
            let (t, v, s, b) = (t.clone(), v.clone(), s.clone(), b.clone());
            let busy = busy.clone();
            glib::timeout_add_local(Duration::from_millis(20), move || match rx.try_recv() {
                Ok(Ok(reply)) => {
                    busy.set(false);
                    if let (Some(tabs), Some(id)) = (t.upgrade(), reply["session"].as_str()) {
                        append(&tabs, &v, &s, &b, id, "Terminal");
                        if replace {
                            if let Some(page) = crate::tiles::page(&tabs, &previous.root) { tabs.close_page(&page); }
                        }
                    } else { previous.warn("New terminal was not created; the previous tab remains open"); }
                    glib::ControlFlow::Break
                }
                Ok(Err(error)) => { busy.set(false); previous.warn(&format!("New terminal failed: {error}")); glib::ControlFlow::Break }
                Err(std::sync::mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
                Err(_) => { busy.set(false); previous.warn("New terminal request failed; the previous tab remains open"); glib::ControlFlow::Break }
            });
        });
        group.add_action(&action);
    }
    window.insert_action_group("terminal", Some(&group));
}
pub fn controls(
    tabs: adw::TabView,
    views: Rc<RefCell<Vec<Rc<View>>>>,
    socket: PathBuf,
    broker: Broker,
) -> gtk::Box {
    let root = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    let add = gtk::Button::with_label("New terminal");
    add.set_widget_name("terminal-new");
    let reopen = gtk::Button::with_label("Sessions…");
    root.append(&add);
    root.append(&reopen);
    let (t, v, s, b) = (tabs.clone(), views.clone(), socket.clone(), broker.clone());
    add.connect_clicked(move |button| {
        button.set_sensitive(false);
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        let worker_socket = s.clone();
        std::thread::spawn(move || {
            let r = (|| {
                let mut rpc = Rpc::connect(&worker_socket)?;
                rpc.call("session.create", json!({"name":"Terminal","argv":crate::shell::default_terminal_argv().map_err(|e| std::io::Error::other(e))?}))
            })();
            let _ = tx.send(r);
        });
        let (t, v, s, b, button) = (t.clone(), v.clone(), s.clone(), b.clone(), button.clone());
        glib::timeout_add_local(Duration::from_millis(20), move || match rx.try_recv() {
            Ok(r) => {
                button.set_sensitive(true);
                match r {
                    Ok(r) => {
                        if let Some(id) = r["session"].as_str() {
                            append(&t, &v, &s, &b, id, "Terminal");
                        }
                    }
                    Err(e) => button.set_tooltip_text(Some(&e.to_string())),
                }
                glib::ControlFlow::Break
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
            Err(_) => {
                button.set_sensitive(true);
                button.set_tooltip_text(Some("Session request failed"));
                glib::ControlFlow::Break
            }
        });
    });
    let (t, v, s, b) = (tabs, views, socket.clone(), broker);
    reopen.connect_clicked(move |button| {
        let dialog = gtk::Dialog::builder()
            .title("Persistent terminal sessions")
            .modal(true)
            .default_width(520)
            .build();
        if let Some(w) = button.root().and_then(|w| w.downcast::<gtk::Window>().ok()) {
            dialog.set_transient_for(Some(&w));
        }
        dialog.add_button("Close", gtk::ResponseType::Close);
        dialog.connect_response(|d, _| d.close());
        let status = gtk::Label::new(Some("Loading…"));
        dialog.content_area().append(&status);
        dialog.present();
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        let socket = s.clone();
        std::thread::spawn(move || {
            let r = (|| {
                let mut rpc = Rpc::connect(&socket)?;
                rpc.call("session.list", json!({}))
            })();
            let _ = tx.send(r);
        });
        let (t, v, s, b, d) = (
            t.clone(),
            v.clone(),
            s.clone(),
            b.clone(),
            dialog.downgrade(),
        );
        glib::timeout_add_local(Duration::from_millis(20), move || {
            let Some(d) = d.upgrade().filter(|d| d.is_visible()) else {
                return glib::ControlFlow::Break;
            };
            match rx.try_recv() {
                Ok(r) => {
                    match r {
                        Ok(Value::Array(rows)) => {
                            status.set_text(
                                "Reopen a view. Closing a tab leaves its processes running.",
                            );
                            for row in rows {
                                if let (Some(id), Some(name)) =
                                    (row["id"].as_str(), row["name"].as_str())
                                {
                                    let button = gtk::Button::with_label(&format!(
                                        "{} · {} · {} windows{}",
                                        name,
                                        &id[..id.len().min(16)],
                                        row["windows"].as_array().map_or(0, Vec::len),
                                        if row["restored"] == true {
                                            " · restored history"
                                        } else {
                                            ""
                                        }
                                    ));
                                    button.set_tooltip_text(Some(id));
                                    d.content_area().append(&button);
                                    let (t, v, s, b, id, name, d) = (
                                        t.clone(),
                                        v.clone(),
                                        s.clone(),
                                        b.clone(),
                                        id.to_owned(),
                                        name.to_owned(),
                                        d.clone(),
                                    );
                                    button.connect_clicked(move |_| {
                                        append(&t, &v, &s, &b, &id, &name);
                                        d.close();
                                    });
                                }
                            }
                        }
                        Ok(_) => status.set_text("Invalid session response"),
                        Err(e) => status.set_text(&e.to_string()),
                    };
                    glib::ControlFlow::Break
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
                Err(_) => {
                    status.set_text("Session list failed");
                    glib::ControlFlow::Break
                }
            }
        });
    });
    root
}

/// Agent sessions for the agent menu, filled asynchronously.
pub fn agent_sessions(
    tabs: adw::TabView,
    views: Rc<RefCell<Vec<Rc<View>>>>,
    socket: PathBuf,
    broker: Broker,
) -> gtk::Box {
    crate::launches::refresh();
    let body = gtk::Box::new(gtk::Orientation::Vertical, 4);
    body.set_widget_name("agent-sessions");
    let heading = gtk::Label::new(Some("Agent sessions"));
    heading.set_xalign(0.0);
    heading.add_css_class("heading");
    body.append(&heading);
    let status = gtk::Label::new(Some("Loading…"));
    status.set_xalign(0.0);
    status.add_css_class("dim-label");
    body.append(&status);
    let (tx, rx) = std::sync::mpsc::sync_channel(1);
    let list_socket = socket.clone();
    std::thread::spawn(move || {
        let r = Rpc::connect(&list_socket).and_then(|mut rpc| rpc.call("session.list", json!({})));
        let _ = tx.send(r);
    });
    let weak = body.downgrade();
    glib::timeout_add_local(Duration::from_millis(20), move || {
        let Some(body) = weak.upgrade() else {
            return glib::ControlFlow::Break;
        };
        if crate::launches::loading() {
            return glib::ControlFlow::Continue;
        }
        let rows = match rx.try_recv() {
            Err(std::sync::mpsc::TryRecvError::Empty) => return glib::ControlFlow::Continue,
            Ok(Ok(Value::Array(rows))) => rows,
            _ => {
                status.set_text("Session list unavailable");
                return glib::ControlFlow::Break;
            }
        };
        let rows: Vec<_> = rows
            .iter()
            .filter_map(|r| Some((r["id"].as_str()?.to_owned(), r["name"].as_str()?.to_owned())))
            .filter(|(id, _)| crate::launches::is_agent(id))
            .collect();
        status.set_text(if rows.is_empty() {
            "None yet — start one from a host below"
        } else {
            "Open a saved agent session; ended connections offer Reconnect"
        });
        for (id, name) in rows {
            let open = views
                .borrow()
                .iter()
                .any(|v| v.session == id && !v.finished.get());
            let button = gtk::Button::with_label(&if open {
                format!("{name} · open")
            } else {
                name.clone()
            });
            button.set_widget_name("agent-session");
            button.add_css_class("flat");
            button.set_tooltip_text(Some(&id));
            if let Some(label) = button.child().and_downcast::<gtk::Label>() {
                label.set_xalign(0.0);
                label.set_ellipsize(gtk::pango::EllipsizeMode::End);
            }
            let (t, v, s, b) = (tabs.clone(), views.clone(), socket.clone(), broker.clone());
            button.connect_clicked(move |button| {
                append(&t, &v, &s, &b, &id, &name);
                if let Some((window, pane, pid)) = crate::launches::agent_target(&id) {
                    if let Some(view) = v.borrow().iter().find(|v| v.session == id) {
                        view.focus_launch_pane(&window, &pane, pid);
                    }
                }
                if let Some(pop) = button
                    .ancestor(gtk::Popover::static_type())
                    .and_downcast::<gtk::Popover>()
                {
                    pop.popdown();
                }
            });
            body.append(&button);
        }
        glib::ControlFlow::Break
    });
    body
}

pub fn fixture(widget: &impl IsA<gtk::Widget>, label: &str) -> bool {
    let widget = widget.as_ref();
    if let Some(button) = widget.downcast_ref::<gtk::Button>() {
        if (label == "New terminal" && button.widget_name() == "terminal-new")
            || button.label().as_deref() == Some(label)
            || button
                .tooltip_text()
                .is_some_and(|t| t.lines().next() == Some(label))
        {
            button.emit_clicked();
            return true;
        }
    }
    let mut child = widget.first_child();
    while let Some(w) = child {
        if fixture(&w, label) {
            return true;
        }
        child = w.next_sibling();
    }
    false
}

/// Only the generic local tab gets a host suffix; named remote/agent sessions
/// retain their explicit endpoint names. No shell command or network lookup.
pub fn terminal_title(name: &str) -> String {
    if name != "Terminal" {
        return name.into();
    }
    static HOST: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    let host = HOST.get_or_init(|| {
        let mut buf = [0u8; 256];
        if unsafe { libc::gethostname(buf.as_mut_ptr().cast(), buf.len()) } != 0 {
            return "local".into();
        }
        let end = buf.iter().position(|b| *b == 0).unwrap_or(buf.len());
        let text: String = String::from_utf8_lossy(&buf[..end])
            .chars()
            .filter(|c| !c.is_control())
            .take(80)
            .collect();
        if text.is_empty() {
            "local".into()
        } else {
            text
        }
    });
    format!("Terminal · {host}")
}
