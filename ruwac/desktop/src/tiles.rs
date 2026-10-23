//! GTK-owned mixed view trees. Reparenting changes presentation, never connections.
use adw::prelude::*;
use gtk::{Orientation, glib};

pub type FocusMemory = std::rc::Rc<std::cell::RefCell<std::collections::HashMap<String, String>>>;
pub type FocusView = std::rc::Rc<dyn Fn(&gtk::Widget)>;

pub fn contains(root: &impl IsA<gtk::Widget>, child: &impl IsA<gtk::Widget>) -> bool {
    root.as_ref() == child.as_ref() || child.as_ref().is_ancestor(root)
}

pub fn page(tabs: &adw::TabView, child: &impl IsA<gtk::Widget>) -> Option<adw::TabPage> {
    (0..tabs.n_pages())
        .map(|i| tabs.nth_page(i))
        .find(|p| contains(&p.child(), child))
}

pub fn append(tabs: &adw::TabView, child: &impl IsA<gtk::Widget>) -> adw::TabPage {
    let body = gtk::Box::new(Orientation::Vertical, 0);
    body.set_hexpand(true);
    body.set_vexpand(true);
    body.set_widget_name(&ruwac_protocol::id("tab"));
    let mut roots = Vec::new();
    leaves(child.as_ref().clone(), &mut roots);
    for root in roots {
        if !ruwac_protocol::valid_id(root.widget_name().as_str(), "view") {
            root.set_widget_name(&ruwac_protocol::id("view"));
        }
    }
    body.append(child);
    let page = tabs.append(&body);
    let weak = body.downgrade();
    page.connect_title_notify(move |p| {
        if let Some(w) = weak.upgrade().and_then(|b| b.first_child()) {
            if !w.is::<gtk::Paned>() {
                w.set_tooltip_text(Some(&p.title()));
            }
        }
    });
    page
}

pub fn join(tabs: &adw::TabView, other: i32, vertical: bool) -> Result<(), String> {
    let current = tabs.selected_page().ok_or("Select a workspace tab")?;
    if other < 0 || other >= tabs.n_pages() {
        return Err("Workspace tab is missing".into());
    }
    let other = tabs.nth_page(other);
    if current == other {
        return Err("Choose a different tab to split with".into());
    }
    let a = current
        .child()
        .downcast::<gtk::Box>()
        .map_err(|_| "Invalid workspace container")?;
    let b = other
        .child()
        .downcast::<gtk::Box>()
        .map_err(|_| "Invalid workspace container")?;
    let first = a.first_child().ok_or("Empty workspace")?;
    let second = b.first_child().ok_or("Empty workspace")?;
    let focus = a
        .root()
        .and_then(|w| w.downcast::<gtk::Window>().ok())
        .and_then(|w| gtk::prelude::GtkWindowExt::focus(&w));
    // Both trees stay alive. Emptying the donor before close avoids resource-close callbacks.
    a.remove(&first);
    b.remove(&second);
    let split = gtk::Paned::new(if vertical {
        Orientation::Vertical
    } else {
        Orientation::Horizontal
    });
    split.set_hexpand(true);
    split.set_vexpand(true);
    split.set_resize_start_child(true);
    split.set_resize_end_child(true);
    split.set_shrink_start_child(false);
    split.set_shrink_end_child(false);
    split.set_wide_handle(true);
    split.set_start_child(Some(&first));
    split.set_end_child(Some(&second));
    // Choose a usable midpoint after allocation; later GTK divider drags remain independent.
    split.connect_map(|p| {
        let weak = p.downgrade();
        glib::idle_add_local_once(move || {
            if let Some(p) = weak.upgrade() {
                let size = if p.orientation() == Orientation::Vertical {
                    p.height()
                } else {
                    p.width()
                };
                if p.position() == 0 && size > 0 {
                    p.set_position(size / 2);
                }
            }
        });
    });
    a.append(&split);
    current.set_title(&format!("{} + {}", current.title(), other.title()));
    tabs.close_page(&other);
    tabs.set_selected_page(&current);
    if let Some(focus) = focus {
        focus.grab_focus();
    }
    Ok(())
}

fn leaves(node: gtk::Widget, out: &mut Vec<gtk::Widget>) {
    if let Some(p) = node.downcast_ref::<gtk::Paned>() {
        if let Some(a) = p.start_child() {
            leaves(a, out);
        }
        if let Some(b) = p.end_child() {
            leaves(b, out);
        }
    } else {
        out.push(node);
    }
}

pub fn views(tabs: &adw::TabView) -> Vec<gtk::Widget> {
    let mut out = Vec::new();
    for i in 0..tabs.n_pages() {
        if let Some(tree) = tabs.nth_page(i).child().first_child() {
            leaves(tree, &mut out);
        }
    }
    out
}
pub fn remember(tabs: &adw::TabView, memory: &FocusMemory, focus: &gtk::Widget) {
    if let Some(leaf) = views(tabs).into_iter().find(|root| contains(root, focus)) {
        if let Some(p) = page(tabs, &leaf) {
            memory.borrow_mut().insert(
                p.child().widget_name().to_string(),
                leaf.widget_name().to_string(),
            );
        }
    }
    memory.borrow_mut().retain(|id, _| {
        (0..tabs.n_pages()).any(|i| tabs.nth_page(i).child().widget_name().as_str() == id)
    });
}
pub fn focused_id(tabs: &adw::TabView, memory: &FocusMemory) -> Option<String> {
    let page = tabs.selected_page()?;
    let id = memory
        .borrow()
        .get(page.child().widget_name().as_str())
        .cloned()?;
    views(tabs)
        .iter()
        .any(|w| w.widget_name().as_str() == id && contains(&page.child(), w))
        .then_some(id)
}
pub fn focus_selected(tabs: &adw::TabView, memory: &FocusMemory, focus: &FocusView) {
    let Some(page) = tabs.selected_page() else {
        return;
    };
    let wanted = memory
        .borrow()
        .get(page.child().widget_name().as_str())
        .cloned();
    let mut own = Vec::new();
    if let Some(tree) = page.child().first_child() {
        leaves(tree, &mut own);
    }
    let leaf = own
        .iter()
        .find(|w| wanted.as_deref() == Some(w.widget_name().as_str()))
        .or_else(|| own.first());
    if let Some(leaf) = leaf {
        focus(leaf);
    }
}
pub fn focus_id(tabs: &adw::TabView, id: &str, focus: &FocusView) -> Result<(), String> {
    if !ruwac_protocol::valid_id(id, "view") {
        return Err("An explicit valid view ID is required".into());
    }
    let leaf = views(tabs)
        .into_iter()
        .find(|w| w.widget_name().as_str() == id)
        .ok_or("This view has closed")?;
    let page = page(tabs, &leaf).ok_or("This view is detached")?;
    tabs.set_selected_page(&page);
    focus(&leaf);
    Ok(())
}

pub fn join_focused(
    tabs: &adw::TabView,
    other: i32,
    vertical: bool,
    memory: &FocusMemory,
    focus: &FocusView,
) -> Result<(), String> {
    let wanted = focused_id(tabs, memory);
    join(tabs, other, vertical)?;
    if let Some(id) = wanted {
        focus_id(tabs, &id, focus)?;
    } else {
        focus_selected(tabs, memory, focus);
    }
    Ok(())
}
pub fn separate_focused(
    tabs: &adw::TabView,
    memory: &FocusMemory,
    focus: &FocusView,
) -> Result<usize, String> {
    let wanted = focused_id(tabs, memory);
    let count = separate(tabs)?;
    if let Some(id) = wanted {
        focus_id(tabs, &id, focus)?;
    } else {
        focus_selected(tabs, memory, focus);
    }
    Ok(count)
}

pub fn separate(tabs: &adw::TabView) -> Result<usize, String> {
    let current = tabs.selected_page().ok_or("Select a workspace tab")?;
    let body = current
        .child()
        .downcast::<gtk::Box>()
        .map_err(|_| "Invalid workspace container")?;
    let tree = body.first_child().ok_or("Empty workspace")?;
    if !tree.is::<gtk::Paned>() {
        return Ok(1);
    }
    let mut views = Vec::new();
    leaves(tree.clone(), &mut views);
    let focus = body
        .root()
        .and_then(|w| w.downcast::<gtk::Window>().ok())
        .and_then(|w| gtk::prelude::GtkWindowExt::focus(&w));
    body.remove(&tree);
    for leaf in &views {
        if let Some(parent) = leaf.parent().and_then(|p| p.downcast::<gtk::Paned>().ok()) {
            if parent.start_child().as_ref() == Some(leaf) {
                parent.set_start_child(None::<&gtk::Widget>);
            } else {
                parent.set_end_child(None::<&gtk::Widget>);
            }
        }
    }
    for (i, leaf) in views.iter().enumerate() {
        if i == 0 {
            body.append(leaf);
            current.set_title(leaf.tooltip_text().as_deref().unwrap_or("Workspace view"));
        } else {
            let page = append(tabs, leaf);
            page.set_title(leaf.tooltip_text().as_deref().unwrap_or("Workspace view"));
        }
    }
    tabs.set_selected_page(&current);
    if let Some(focus) = focus {
        focus.grab_focus();
    }
    Ok(views.len())
}

/// Remove one leaf and collapse its immediate split; neighboring widgets keep their roots.
fn take(tabs: &adw::TabView, leaf: &gtk::Widget) -> Result<(), String> {
    if leaf.is::<gtk::Paned>() {
        return Err("Choose an individual view".into());
    }
    let page = page(tabs, leaf).ok_or("This view has already closed")?;
    let body = page
        .child()
        .downcast::<gtk::Box>()
        .map_err(|_| "Invalid workspace container")?;
    let parent = leaf.parent().ok_or("The view is detached")?;
    if let Some(split) = parent.downcast_ref::<gtk::Paned>() {
        let start = split.start_child().as_ref() == Some(leaf);
        let sibling = if start {
            split.end_child()
        } else {
            split.start_child()
        }
        .ok_or("Incomplete workspace split")?;
        let grand = split.parent().ok_or("Detached workspace split")?;
        if let Some(outer) = grand.downcast_ref::<gtk::Paned>() {
            let first = outer.start_child().as_ref() == Some(&parent);
            split.set_start_child(None::<&gtk::Widget>);
            split.set_end_child(None::<&gtk::Widget>);
            if first {
                outer.set_start_child(Some(&sibling));
            } else {
                outer.set_end_child(Some(&sibling));
            }
        } else if grand == body.clone().upcast::<gtk::Widget>() {
            split.set_start_child(None::<&gtk::Widget>);
            split.set_end_child(None::<&gtk::Widget>);
            body.remove(split);
            body.append(&sibling);
        } else {
            return Err("Invalid workspace split parent".into());
        }
    } else if parent == body.clone().upcast::<gtk::Widget>() {
        body.remove(leaf);
    } else {
        return Err("Invalid workspace view parent".into());
    }
    if let Some(tree) = body.first_child() {
        let mut remaining = Vec::new();
        leaves(tree, &mut remaining);
        page.set_title(
            &remaining
                .iter()
                .map(|w| {
                    w.tooltip_text()
                        .map(|t| t.to_string())
                        .unwrap_or_else(|| "Workspace view".into())
                })
                .collect::<Vec<_>>()
                .join(" + "),
        );
    } else {
        // Empty donor callbacks cannot close the extracted view or its neighbors.
        tabs.close_page(&page);
    }
    Ok(())
}

pub type CloseView = std::rc::Rc<dyn Fn(&gtk::Widget)>;

pub fn detach(tabs: &adw::TabView, leaf: &gtk::Widget) -> Result<(), String> {
    take(tabs, leaf)
}

pub fn move_view(tabs: &adw::TabView, leaf: &gtk::Widget) -> Result<(), String> {
    take(tabs, leaf)?;
    let p = append(tabs, leaf);
    p.set_title(leaf.tooltip_text().as_deref().unwrap_or("Workspace view"));
    tabs.set_selected_page(&p);
    Ok(())
}

pub fn close_view(
    tabs: &adw::TabView,
    leaf: &gtk::Widget,
    close: &CloseView,
) -> Result<(), String> {
    take(tabs, leaf)?;
    close(leaf);
    Ok(())
}

fn manage(tabs: &adw::TabView, button: &gtk::Button, close: CloseView, focus: FocusView) {
    button.grab_focus();
    let d = gtk::Dialog::builder()
        .title("Workspace views")
        .modal(true)
        .default_width(600)
        .build();
    if let Some(w) = button.root().and_then(|w| w.downcast::<gtk::Window>().ok()) {
        d.set_transient_for(Some(&w));
    }
    d.add_button("Done", gtk::ResponseType::Close);
    d.connect_response(|d, _| d.close());
    let message = gtk::Label::new(Some(
        "Focus, move or close one view. Closing a terminal detaches it; its programs keep running. Closing RDP disconnects only that view. Closing Files cancels its transfers.",
    ));
    message.set_wrap(true);
    d.content_area().append(&message);
    let status = gtk::Label::new(None);
    status.set_wrap(true);
    d.content_area().append(&status);
    if let Some(tree) = tabs.selected_page().and_then(|p| p.child().first_child()) {
        let mut views = Vec::new();
        leaves(tree, &mut views);
        for (index, leaf) in views.into_iter().enumerate() {
            let row = gtk::Box::new(Orientation::Horizontal, 6);
            let title = gtk::Label::new(Some(&format!(
                "{}\n{}",
                leaf.tooltip_text().as_deref().unwrap_or("Workspace view"),
                leaf.widget_name()
            )));
            title.set_hexpand(true);
            title.set_xalign(0.0);
            title.set_ellipsize(gtk::pango::EllipsizeMode::End);
            row.append(&title);
            for (name, action) in [
                ("Focus", "focus"),
                ("Move to own tab", "move"),
                ("Close view", "close"),
            ] {
                let b = gtk::Button::with_label(name);
                b.set_widget_name(&format!("{action}-view-{index}"));
                let (t, leaf, close, status, dialog) = (
                    tabs.clone(),
                    leaf.clone(),
                    close.clone(),
                    status.clone(),
                    d.downgrade(),
                );
                let focus = focus.clone();
                b.connect_clicked(move |_| {
                    if action == "focus" {
                        if page(&t, &leaf).is_none() {
                            status.set_text("This view has closed");
                            return;
                        }
                        if let Some(d) = dialog.upgrade() {
                            d.close();
                        }
                        let (t, leaf, focus) = (t.clone(), leaf.clone(), focus.clone());
                        glib::idle_add_local_once(move || {
                            let _ = focus_id(&t, leaf.widget_name().as_str(), &focus);
                        });
                        return;
                    }
                    let result = if action == "move" {
                        move_view(&t, &leaf)
                    } else {
                        close_view(&t, &leaf, &close)
                    };
                    match result {
                        Ok(()) => {
                            if let Some(d) = dialog.upgrade() {
                                d.close();
                            }
                        }
                        Err(e) => status.set_text(&e),
                    }
                });
                row.append(&b);
            }
            d.content_area().append(&row);
        }
    }
    d.present();
}

/// Test driver invokes the same native dialog buttons as a user.
pub fn fixture(action: &str, index: usize) -> bool {
    fn find(w: &gtk::Widget, name: &str) -> Option<gtk::Button> {
        if w.widget_name() == name {
            return w.clone().downcast().ok();
        }
        let mut child = w.first_child();
        while let Some(c) = child {
            if let Some(b) = find(&c, name) {
                return Some(b);
            }
            child = c.next_sibling();
        }
        None
    }
    for w in gtk::Window::list_toplevels() {
        if let Ok(w) = w.downcast::<gtk::Window>() {
            if w.title().as_deref() == Some("Workspace views") {
                if let Some(b) = find(w.upcast_ref(), &format!("{action}-view-{index}")) {
                    b.grab_focus();
                    b.emit_clicked();
                    return true;
                }
            }
        }
    }
    false
}

pub fn controls(
    tabs: &adw::TabView,
    close: CloseView,
    focus: FocusView,
    memory: FocusMemory,
) -> gtk::Box {
    let root = gtk::Box::new(Orientation::Horizontal, 4);
    let button = gtk::Button::with_label("Split tabs…");
    let separate_button = gtk::Button::with_label("Separate views");
    separate_button.set_tooltip_text(Some(
        "Move every mixed pane into its own tab; connections keep running",
    ));
    root.append(&button);
    root.append(&separate_button);
    let manage_button = gtk::Button::with_label("Views…");
    manage_button.set_tooltip_text(Some("Move or close an individual workspace view"));
    root.append(&manage_button);
    let t = tabs.clone();
    let manage_focus = focus.clone();
    manage_button
        .connect_clicked(move |button| manage(&t, button, close.clone(), manage_focus.clone()));
    let (join_focus, join_memory) = (focus.clone(), memory.clone());
    let t = tabs.clone();
    button.connect_clicked(move |button| {
        let d = gtk::Dialog::builder()
            .title("Combine tabs into a mixed workspace")
            .modal(true)
            .default_width(500)
            .build();
        if let Some(w) = button.root().and_then(|w| w.downcast::<gtk::Window>().ok()) {
            d.set_transient_for(Some(&w));
        }
        d.add_button("Cancel", gtk::ResponseType::Cancel);
        d.connect_response(|d, _| d.close());
        let orientation = gtk::DropDown::from_strings(&["Side by side", "Top and bottom"]);
        d.content_area().append(&orientation);
        let status = gtk::Label::new(Some(
            "Choose another live tab. Its views keep their connections.",
        ));
        status.set_wrap(true);
        d.content_area().append(&status);
        let selected = t.selected_page();
        for index in 0..t.n_pages() {
            let page = t.nth_page(index);
            if Some(&page) == selected.as_ref() {
                continue;
            }
            let add = gtk::Button::with_label(&page.title());
            let (tabs, dialog, o, status) = (
                t.clone(),
                d.downgrade(),
                orientation.clone(),
                status.clone(),
            );
            let (focus, memory) = (join_focus.clone(), join_memory.clone());
            add.connect_clicked(move |_| {
                if crate::tiles::page(&tabs, &page.child()).is_none() {
                    status.set_text("The selected tab closed");
                    return;
                }
                match join_focused(
                    &tabs,
                    tabs.page_position(&page),
                    o.selected() == 1,
                    &memory,
                    &focus,
                ) {
                    Ok(()) => {
                        if let Some(d) = dialog.upgrade() {
                            d.close();
                        }
                        let (tabs, memory, focus) = (tabs.clone(), memory.clone(), focus.clone());
                        glib::idle_add_local_once(move || focus_selected(&tabs, &memory, &focus));
                    }
                    Err(e) => status.set_text(&e),
                }
            });
            d.content_area().append(&add);
        }
        d.present();
    });
    let t = tabs.clone();
    separate_button.connect_clicked(move |button| {
        if let Err(e) = separate_focused(&t, &memory, &focus) {
            button.set_tooltip_text(Some(&e));
        }
    });
    root
}

pub fn inspect(tabs: &adw::TabView, memory: &FocusMemory) -> serde_json::Value {
    fn node(w: gtk::Widget) -> serde_json::Value {
        if let Some(p) = w.downcast_ref::<gtk::Paned>() {
            serde_json::json!({"split":if p.orientation()==Orientation::Vertical{"vertical"}else{"horizontal"},"position":p.position(),"width":p.width(),"height":p.height(),"first":p.start_child().map(node),"second":p.end_child().map(node)})
        } else {
            let mut metadata = serde_json::json!({"id":w.widget_name().as_str(),"view":w.type_().name(),"mapped":w.is_mapped(),"width":w.width(),"height":w.height()});
            if w.has_css_class("ruwac-web") {
                metadata["kind"] = serde_json::json!("web");
            }
            metadata
        }
    }
    serde_json::json!({"focused_view":focused_id(tabs,memory),"selected_tab":tabs.selected_page().map(|p|p.child().widget_name().to_string()),"tabs":(0..tabs.n_pages()).map(|i|{let p=tabs.nth_page(i);serde_json::json!({"id":p.child().widget_name().as_str(),"title":p.title().as_str(),"layout":p.child().first_child().map(node)})}).collect::<Vec<_>>()})
}

pub fn snapshot(
    tabs: &adw::TabView,
    describe: impl Fn(&gtk::Widget) -> Option<crate::workspace::Layout>,
    memory: &FocusMemory,
) -> Result<Vec<crate::workspace::Tab>, String> {
    fn walk(
        w: &gtk::Widget,
        describe: &impl Fn(&gtk::Widget) -> Option<crate::workspace::Layout>,
    ) -> Result<crate::workspace::Layout, String> {
        if let Some(p) = w.downcast_ref::<gtk::Paned>() {
            let size = if p.orientation() == Orientation::Vertical {
                p.height()
            } else {
                p.width()
            };
            let ratio = if size > 0 {
                (p.position() as f64 / size as f64).clamp(0.05, 0.95)
            } else {
                0.5
            };
            Ok(crate::workspace::Layout::Split {
                vertical: p.orientation() == Orientation::Vertical,
                ratio,
                first: Box::new(walk(&p.start_child().ok_or("Empty split")?, describe)?),
                second: Box::new(walk(&p.end_child().ok_or("Empty split")?, describe)?),
            })
        } else {
            describe(w)
                .ok_or_else(|| "Unrecognized workspace view; existing metadata preserved".into())
        }
    }
    (0..tabs.n_pages())
        .map(|i| {
            let p = tabs.nth_page(i);
            let id = p.child().widget_name().to_string();
            let focused_view = memory.borrow().get(&id).cloned().filter(|id| {
                views(tabs)
                    .iter()
                    .any(|w| w.widget_name().as_str() == id && contains(&p.child(), w))
            });
            Ok(crate::workspace::Tab {
                id,
                focused_view,
                title: p.title().to_string(),
                layout: walk(&p.child().first_child().ok_or("Empty tab")?, &describe)?,
            })
        })
        .collect()
}

pub fn restore(
    tabs: &adw::TabView,
    saved: &[crate::workspace::Tab],
    memory: &FocusMemory,
    mut create: impl FnMut(&crate::workspace::Layout) -> gtk::Widget,
) {
    fn build(
        layout: &crate::workspace::Layout,
        create: &mut impl FnMut(&crate::workspace::Layout) -> gtk::Widget,
    ) -> gtk::Widget {
        if let crate::workspace::Layout::Split {
            vertical,
            ratio,
            first,
            second,
        } = layout
        {
            let p = gtk::Paned::new(if *vertical {
                Orientation::Vertical
            } else {
                Orientation::Horizontal
            });
            p.set_hexpand(true);
            p.set_vexpand(true);
            p.set_wide_handle(true);
            p.set_resize_start_child(true);
            p.set_resize_end_child(true);
            p.set_shrink_start_child(false);
            p.set_shrink_end_child(false);
            p.set_start_child(Some(&build(first, create)));
            p.set_end_child(Some(&build(second, create)));
            let ratio = *ratio;
            p.connect_map(move |p| {
                let weak = p.downgrade();
                glib::idle_add_local_once(move || {
                    if let Some(p) = weak.upgrade() {
                        let size = if p.orientation() == Orientation::Vertical {
                            p.height()
                        } else {
                            p.width()
                        };
                        if size > 0 {
                            p.set_position((size as f64 * ratio).round() as i32);
                        }
                    }
                });
            });
            p.upcast()
        } else {
            let root = create(layout);
            if let Some(id) = layout.view_id() {
                root.set_widget_name(id);
            }
            root
        }
    }
    for tab in saved {
        let tree = build(&tab.layout, &mut create);
        let page = append(tabs, &tree);
        page.child().set_widget_name(&tab.id);
        if let Some(id) = &tab.focused_view {
            memory.borrow_mut().insert(tab.id.clone(), id.clone());
        }
        page.set_title(&tab.title);
    }
}

/// Tab decorations change only when their value does: every set redraws the
/// tab bar, and the status timer runs ten times a second.
pub fn set_page_icon(page: &adw::TabPage, name: &str) {
    let same = page
        .icon()
        .and_downcast::<gtk::gio::ThemedIcon>()
        .is_some_and(|i| i.names().first().map(|n| n.as_str()) == Some(name));
    if !same {
        page.set_icon(Some(&gtk::gio::ThemedIcon::new(name)));
    }
}
pub fn set_page_tooltip(page: &adw::TabPage, text: &str) {
    if page.tooltip().as_deref() != Some(text) {
        page.set_tooltip(text);
    }
}
pub fn set_page_title(page: &adw::TabPage, title: &str) {
    if page.title().as_str() != title {
        page.set_title(title);
    }
}
pub fn set_page_loading(page: &adw::TabPage, loading: bool) {
    if page.is_loading() != loading {
        page.set_loading(loading);
    }
}

/// Full-colour, shape-distinct indicators, independent of GTK's private tab CSS.
#[derive(Clone, Copy)]
pub enum TabStatus {
    Ready,
    Busy,
    Error,
    Offline,
}
pub fn set_page_status(page: &adw::TabPage, status: TabStatus) {
    use gtk::gio::prelude::*;
    thread_local! {
        static ICONS: Vec<gtk::gio::BytesIcon> = [
            ("#34d399", "M4 8l3 3 5-6"),
            ("#f5c451", "M8 4v4l3 2"),
            ("#f0737a", "M8 4v5m0 2v1"),
            ("#9b9ca3", "M4 8h8"),
        ].iter().map(|(color,path)| {
            let svg = format!(r##"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 16 16"><circle cx="8" cy="8" r="7" fill="{color}"/><path d="{path}" stroke="#131416" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" fill="none"/></svg>"##);
            gtk::gio::BytesIcon::new(&gtk::glib::Bytes::from_owned(svg.into_bytes()))
        }).collect();
    }
    ICONS.with(|icons| {
        let icon = &icons[status as usize];
        if !page
            .indicator_icon()
            .is_some_and(|current| current.equal(Some(icon)))
        {
            page.set_indicator_icon(Some(icon));
        }
    });
}
