//! Saved host menus. No connection is made until an explicit action is clicked.
use adw::prelude::*;
use std::cell::RefCell;

thread_local! {
    static MENUS: RefCell<Option<(gtk::glib::WeakRef<adw::ApplicationWindow>, crate::profiles::ShortcutMenus)>> = const { RefCell::new(None) };
}

fn scroll(body: &gtk::Box) -> gtk::ScrolledWindow {
    gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .min_content_width(290)
        .max_content_height(480)
        .propagate_natural_height(true)
        .child(body)
        .build()
}

pub fn install(
    window: &adw::ApplicationWindow,
    menus: crate::profiles::ShortcutMenus,
    sessions: std::rc::Rc<dyn Fn() -> gtk::Box>,
) -> gtk::MenuButton {
    MENUS.with(|state| *state.borrow_mut() = Some((window.downgrade(), menus.clone())));
    let button = gtk::MenuButton::new();
    button.set_widget_name("agent-host-menu");
    button.set_icon_name("applications-science-symbolic");
    button.set_tooltip_text(Some(
        "Agents · Alt+A · saved hosts · Codex, Claude and Kimi",
    ));
    button.update_property(&[gtk::accessible::Property::Label("Saved agent hosts")]);
    let pop = gtk::Popover::new();
    pop.connect_show(move |pop| {
        // Running sessions first, then hosts that can start new ones.
        let body = gtk::Box::new(gtk::Orientation::Vertical, 8);
        body.append(&sessions());
        body.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
        body.append(&menus(true));
        pop.set_child(Some(&scroll(&body)));
    });
    button.set_popover(Some(&pop));
    button
}

pub fn popup(view: &std::rc::Rc<crate::view::View>, x: f64, y: f64) {
    let area = &view.area;
    MENUS.with(|state| {
        let state = state.borrow();
        let Some((window, menus)) = state.as_ref() else {
            return;
        };
        let Some(window) = window.upgrade() else {
            return;
        };
        if area.root().as_ref() != Some(window.upcast_ref::<gtk::Root>()) {
            return;
        }
        let pop = gtk::Popover::new();
        pop.set_widget_name("terminal-host-menu");
        pop.set_parent(area);
        pop.set_pointing_to(Some(&gtk::gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
        let body = view.clipboard_menu();
        if let Some(links) = view.link_menu(x, y) {
            body.append(&links);
        }
        body.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
        body.append(&menus(false));
        pop.set_child(Some(&scroll(&body)));
        pop.connect_closed(|pop| pop.unparent());
        pop.popup();
    });
}

/// Right-click on a button lists SSH hosts that open a terminal directly.
pub fn attach_ssh_menu(button: &gtk::Button) {
    let click = gtk::GestureClick::builder().button(3).build();
    let target = button.downgrade();
    click.connect_pressed(move |gesture, _, _, _| {
        gesture.set_state(gtk::EventSequenceState::Claimed);
        let Some(button) = target.upgrade() else {
            return;
        };
        MENUS.with(|state| {
            let Some((_, menus)) = state.borrow().as_ref().map(|(w, m)| (w.clone(), m.clone()))
            else {
                return;
            };
            let pop = gtk::Popover::new();
            pop.set_widget_name("new-terminal-ssh-menu");
            pop.set_parent(&button);
            pop.set_child(Some(&scroll(&menus(false))));
            pop.connect_closed(|pop| pop.unparent());
            pop.popup();
        });
    });
    button.add_controller(click);
    let tip = button
        .tooltip_text()
        .or_else(|| button.label())
        .map(|t| t.to_string())
        .unwrap_or_else(|| "New terminal".into());
    button.set_tooltip_text(Some(&format!("{tip}\nRight-click: SSH hosts")));
}

/// Native fixture drives the actual menus without connecting to any saved host.
pub fn fixture(window: &adw::ApplicationWindow, q: &serde_json::Value) -> serde_json::Value {
    let widgets = RefCell::new(Vec::new());
    let sections = RefCell::new(Vec::new());
    let dialogs = RefCell::new(Vec::new());
    crate::chrome::walk(window.upcast_ref(), &|w| {
        if w.widget_name() == "agent-host-menu" && q["operation"] == "header" {
            if let Some(b) = w.downcast_ref::<gtk::MenuButton>() {
                b.popup();
            }
        }
    });
    let clicked: RefCell<Option<gtk::Button>> = RefCell::new(None);
    for top in gtk::Window::list_toplevels() {
        if let Some(window) = top.downcast_ref::<gtk::Window>() {
            if let Some(title) = window.title() {
                dialogs.borrow_mut().push(title.to_string());
            }
            if q["close_ssh_review"] == true
                && matches!(
                    window.title().as_deref(),
                    Some("SSH key access" | "Confirm SSH endpoint additions")
                )
            {
                window.close();
                continue;
            }
        }
        if q["cancel_confirmation"] == true {
            if let Some(dialog) = top.downcast_ref::<gtk::Dialog>() {
                if matches!(
                    dialog.title().as_deref(),
                    Some("Install remote RUWAC CLI" | "Authorize SSH public key")
                ) {
                    dialog.response(gtk::ResponseType::Cancel);
                }
            }
        }
        crate::chrome::walk(&top, &|w| {
            if q["focus"].as_str() == Some(w.widget_name().as_str()) {
                w.grab_focus();
            }
            if let Some(e) = w.downcast_ref::<gtk::Entry>() {
                if let Some(value) = q["fields"][w.widget_name().as_str()].as_str() {
                    e.set_text(value);
                }
            }
            if let Some(d) = w.downcast_ref::<gtk::DropDown>() {
                if let Some(value) = q["fields"][w.widget_name().as_str()].as_u64() {
                    d.set_selected(value as u32);
                }
            }
            if let Some(label) = w.downcast_ref::<gtk::Label>() {
                if [
                    "Recent SSH connections",
                    "Saved SSH hosts",
                    "Saved agent hosts",
                    "Agent sessions",
                ]
                .contains(&label.text().as_str())
                {
                    sections.borrow_mut().push(label.text().to_string());
                }
            }
            if let Some(pick) = w.downcast_ref::<gtk::CheckButton>() {
                if let Some(index) = q["review_pick"].as_u64() {
                    if w.widget_name() == format!("ssh-access-pick-{index}") && pick.is_sensitive()
                    {
                        pick.set_active(q["active"].as_bool().unwrap_or(true));
                    }
                }
            }
            if let Some(b) = w.downcast_ref::<gtk::Button>() {
                if clicked.borrow().is_none()
                    && q["click"].as_str() == Some(w.widget_name().as_str())
                {
                    *clicked.borrow_mut() = Some(b.clone());
                }
            }
            if w.widget_name().starts_with("saved-host-")
                || w.widget_name() == "agent-session"
                || w.widget_name().starts_with("machine-")
                || w.widget_name().starts_with("ssh-")
                || w.widget_name() == "terminal-host-menu"
                || w.widget_name().starts_with("quick-ssh-")
            {
                widgets.borrow_mut().push(serde_json::json!({"name":w.widget_name().to_string(),"visible":w.is_visible(),"sensitive":w.is_sensitive(), "selected":w.downcast_ref::<gtk::DropDown>().map(|p|p.selected()), "focused":w.has_focus(), "entry":w.downcast_ref::<gtk::Entry>().map(|p|p.text().to_string()), "active":w.downcast_ref::<gtk::CheckButton>().map(|p| p.is_active()), "text":w.downcast_ref::<gtk::Label>().map(|p| p.text().to_string()), "bounds":w.compute_bounds(window).map(|r| [r.x(),r.y(),r.width(),r.height()])}));
            }
        });
        if top.widget_name() == "quick-ssh-machine" {
            if let Some(d) = top.downcast_ref::<gtk::Dialog>() {
                match q["quick_response"].as_str() {
                    Some("save") => d.response(gtk::ResponseType::Accept),
                    Some("cancel") => d.response(gtk::ResponseType::Cancel),
                    _ => (),
                }
            }
        }
    }
    if let Some(button) = clicked.into_inner() {
        button.emit_clicked();
    }
    if q["close"] == true {
        crate::chrome::walk(window.upcast_ref(), &|w| {
            if let Some(p) = w.downcast_ref::<gtk::Popover>() {
                p.popdown();
            }
        });
    }
    serde_json::json!({"widgets":widgets.into_inner(), "sections":sections.into_inner(), "dialogs":dialogs.into_inner()})
}
