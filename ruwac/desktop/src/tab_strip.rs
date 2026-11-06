//! Responsive presentation of the existing TabView; never owns terminal content.
use adw::prelude::*;
use gtk::{gdk, glib};
use std::{
    cell::{Cell, RefCell},
    rc::{Rc, Weak},
};

thread_local! {
    static STRIPS: RefCell<Vec<Weak<Inner>>> = const { RefCell::new(Vec::new()) };
}

#[derive(Clone)]
pub struct TabStrip(Rc<Inner>);
struct Tab {
    page: glib::WeakRef<adw::TabPage>,
    button: gtk::ToggleButton,
    label: gtk::Label,
    icon: gtk::Image,
    indicator: gtk::Image,
    spinner: gtk::Spinner,
    close: gtk::Button,
    hover: Cell<bool>,
    notify: Option<glib::SignalHandlerId>,
}
struct Inner {
    root: gtk::Box,
    rows: [gtk::Box; 2],
    hosts: [gtk::Box; 2],
    scroll: [gtk::ScrolledWindow; 2],
    view: glib::WeakRef<adw::TabView>,
    tabs: RefCell<Vec<Tab>>,
    handlers: RefCell<Vec<glib::SignalHandlerId>>,
    end: RefCell<Option<gtk::Widget>>,
    two: Cell<bool>,
    compact: Cell<bool>,
    reveal_pending: Cell<bool>,
}
impl Drop for Inner {
    fn drop(&mut self) {
        if let Some(view) = self.view.upgrade() {
            for id in self.handlers.get_mut().drain(..) {
                view.disconnect(id);
            }
        }
        for tab in self.tabs.get_mut().drain(..) {
            if let (Some(page), Some(id)) = (tab.page.upgrade(), tab.notify) {
                page.disconnect(id);
            }
        }
    }
}
impl TabStrip {
    pub fn new(view: &adw::TabView, window: &adw::ApplicationWindow) -> Self {
        let root = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(0)
            .accessible_role(gtk::AccessibleRole::TabList)
            .build();
        root.add_css_class("flexible-tabs");
        root.set_hexpand(true);
        let rows = std::array::from_fn(|_| gtk::Box::new(gtk::Orientation::Horizontal, 0));
        let hosts = std::array::from_fn(|_| gtk::Box::new(gtk::Orientation::Horizontal, 0));
        let scroll = std::array::from_fn(|i| {
            let s = gtk::ScrolledWindow::builder()
                .child(&rows[i])
                .hscrollbar_policy(gtk::PolicyType::Automatic)
                .vscrollbar_policy(gtk::PolicyType::Never)
                .propagate_natural_height(true)
                .hexpand(true)
                .build();
            hosts[i].append(&s);
            root.append(&hosts[i]);
            s
        });
        let inner = Rc::new(Inner {
            root,
            rows,
            hosts,
            scroll,
            view: view.downgrade(),
            tabs: RefCell::new(Vec::new()),
            handlers: RefCell::new(Vec::new()),
            end: RefCell::new(None),
            two: Cell::new(false),
            compact: Cell::new(false),
            reveal_pending: Cell::new(false),
        });
        let strip = Self(inner.clone());
        // The window retains presentation state. It holds only weak model/page
        // references, so closing a window cannot retain terminal trees in a cycle.
        let keep = strip.clone();
        window.connect_destroy(move |_| {
            let _ = &keep;
        });
        let weak = Rc::downgrade(&inner);
        let id = view.connect_page_attached(move |_, _, _| {
            if let Some(s) = weak.upgrade() {
                Self(s).rebuild();
            }
        });
        inner.handlers.borrow_mut().push(id);
        let weak = Rc::downgrade(&inner);
        let id = view.connect_page_detached(move |_, _, _| {
            if let Some(s) = weak.upgrade() {
                Self(s).rebuild();
            }
        });
        inner.handlers.borrow_mut().push(id);
        let weak = Rc::downgrade(&inner);
        let id = view.connect_page_reordered(move |_, _, _| {
            if let Some(s) = weak.upgrade() {
                Self(s).rebuild();
            }
        });
        inner.handlers.borrow_mut().push(id);
        let weak = Rc::downgrade(&inner);
        let id = view.connect_selected_page_notify(move |_| {
            if let Some(s) = weak.upgrade() {
                Self(s).sync();
            }
        });
        inner.handlers.borrow_mut().push(id);
        for row in &inner.rows {
            let target = gtk::DropTarget::new(adw::TabPage::static_type(), gdk::DragAction::MOVE);
            let view = view.downgrade();
            target.connect_drop(move |_, value, _, _| {
                let Some(view) = view.upgrade() else {
                    return false;
                };
                place(&view, view.n_pages(), value)
            });
            row.add_controller(target);
        }
        STRIPS.with(|all| {
            let mut all = all.borrow_mut();
            all.retain(|w| w.strong_count() > 0);
            all.push(Rc::downgrade(&inner));
        });
        let prefs = crate::sidebar_shell::Shell::prefs();
        inner.two.set(prefs.two_tab_rows);
        inner.compact.set(prefs.compact_tabs);
        let weak = Rc::downgrade(&inner);
        inner.root.connect_map(move |_| {
            if let Some(inner) = weak.upgrade() {
                Self(inner).queue_reveal();
            }
        });
        for scroll in &inner.scroll {
            let weak = Rc::downgrade(&inner);
            scroll.hadjustment().connect_changed(move |_| {
                if let Some(inner) = weak.upgrade() {
                    Self(inner).queue_reveal();
                }
            });
        }
        strip.rebuild();
        strip
    }
    pub fn widget(&self) -> &gtk::Box {
        &self.0.root
    }
    pub fn width(&self) -> i32 {
        self.0.root.width()
    }
    pub fn add_css_class(&self, name: &str) {
        self.0.root.add_css_class(name);
    }
    pub fn end_action_widget(&self) -> Option<gtk::Widget> {
        self.0.end.borrow().clone()
    }
    pub fn set_end_action_widget(&self, widget: Option<&impl IsA<gtk::Widget>>) {
        if let Some(old) = self.0.end.borrow_mut().take() {
            if let Some(row) = old.parent().and_downcast::<gtk::Box>() {
                row.remove(&old);
            }
        }
        *self.0.end.borrow_mut() = widget.map(|w| w.as_ref().clone());
        self.layout();
    }
    pub fn apply_preferences(prefs: &crate::sidebar_shell::Settings) {
        let strips = STRIPS.with(|all| {
            all.borrow()
                .iter()
                .filter_map(Weak::upgrade)
                .collect::<Vec<_>>()
        });
        for inner in strips {
            inner.two.set(prefs.two_tab_rows);
            inner.compact.set(prefs.compact_tabs);
            let strip = Self(inner);
            strip.layout();
            strip.sync();
        }
    }
    fn rebuild(&self) {
        let Some(view) = self.0.view.upgrade() else {
            return;
        };
        let mut old = self.0.tabs.borrow_mut().drain(..).collect::<Vec<_>>();
        let mut next = Vec::with_capacity(view.n_pages() as usize);
        for i in 0..view.n_pages() {
            let page = view.nth_page(i);
            if let Some(index) = old
                .iter()
                .position(|tab| tab.page.upgrade().as_ref() == Some(&page))
            {
                next.push(old.remove(index));
                continue;
            }

            let button = gtk::ToggleButton::builder()
                .accessible_role(gtk::AccessibleRole::Tab)
                .build();
            button.add_css_class("flexible-tab");
            button.set_hexpand(true);
            let body = gtk::Box::new(gtk::Orientation::Horizontal, 1);
            let icon = gtk::Image::new();
            let indicator = gtk::Image::new();
            let spinner = gtk::Spinner::new();
            let label = gtk::Label::new(None);
            label.set_ellipsize(gtk::pango::EllipsizeMode::End);
            label.set_width_chars(0);
            label.set_max_width_chars(18);
            label.set_hexpand(true);
            label.set_xalign(0.0);
            let close = gtk::Button::from_icon_name("window-close-symbolic");
            close.add_css_class("tab-close");
            close.set_focusable(false);
            close.set_tooltip_text(Some("Close tab"));
            body.append(&icon);
            body.append(&spinner);
            body.append(&indicator);
            body.append(&FittingLabel::new(&label));
            body.append(&close);
            button.set_child(Some(&body));
            let v = view.downgrade();
            let p = page.downgrade();
            button.connect_clicked(move |b| {
                if let (Some(v), Some(p)) = (v.upgrade(), p.upgrade()) {
                    v.set_selected_page(&p);
                    b.set_active(true);
                    p.child().grab_focus();
                }
            });
            let v = view.downgrade();
            let p = page.downgrade();
            close.connect_clicked(move |_| {
                if let (Some(v), Some(p)) = (v.upgrade(), p.upgrade()) {
                    v.close_page(&p);
                }
            });
            let middle = gtk::GestureClick::new();
            middle.set_button(2);
            let v = view.downgrade();
            let p = page.downgrade();
            middle.connect_released(move |_, _, _, _| {
                if let (Some(v), Some(p)) = (v.upgrade(), p.upgrade()) {
                    v.close_page(&p);
                }
            });
            button.add_controller(middle);
            let menu = gtk::GestureClick::new();
            menu.set_button(3);
            let v = view.downgrade();
            let p = page.downgrade();
            let b = button.downgrade();
            menu.connect_pressed(move |_, _, x, y| {
                if let (Some(v), Some(p), Some(b)) = (v.upgrade(), p.upgrade(), b.upgrade()) {
                    popup(&v, &p, &b, x, y);
                }
            });
            button.add_controller(menu);
            let keys = gtk::EventControllerKey::new();
            let weak = Rc::downgrade(&self.0);
            let p = page.downgrade();
            let b = button.downgrade();
            keys.connect_key_pressed(move |_, key, _, modifiers| {
                let (Some(inner), Some(page), Some(button)) =
                    (weak.upgrade(), p.upgrade(), b.upgrade())
                else {
                    return glib::Propagation::Proceed;
                };
                let Some(view) = inner.view.upgrade() else {
                    return glib::Propagation::Proceed;
                };
                if key == gdk::Key::Menu
                    || (key == gdk::Key::F10 && modifiers == gdk::ModifierType::SHIFT_MASK)
                {
                    popup(&view, &page, &button, 0.0, button.height() as f64);
                    return glib::Propagation::Stop;
                }
                if modifiers.intersects(
                    gdk::ModifierType::CONTROL_MASK
                        | gdk::ModifierType::ALT_MASK
                        | gdk::ModifierType::SUPER_MASK,
                ) {
                    return glib::Propagation::Proceed;
                }
                let index = view.page_position(&page);
                let rtl = button.direction() == gtk::TextDirection::Rtl;
                let next = match key {
                    gdk::Key::Left => index + if rtl { 1 } else { -1 },
                    gdk::Key::Right => index + if rtl { -1 } else { 1 },
                    gdk::Key::Home => 0,
                    gdk::Key::End => view.n_pages() - 1,
                    _ => return glib::Propagation::Proceed,
                };
                if (0..view.n_pages()).contains(&next) {
                    let target = view.nth_page(next);
                    if let Some(tab) = inner
                        .tabs
                        .borrow()
                        .iter()
                        .find(|tab| tab.page.upgrade().as_ref() == Some(&target))
                    {
                        tab.button.grab_focus();
                    }
                }
                glib::Propagation::Stop
            });
            button.add_controller(keys);
            let motion = gtk::EventControllerMotion::new();
            let weak = Rc::downgrade(&self.0);
            let p = page.downgrade();
            motion.connect_enter(move |_, _, _| {
                if let (Some(s), Some(p)) = (weak.upgrade(), p.upgrade()) {
                    Self(s).hover(&p, true);
                }
            });
            let weak = Rc::downgrade(&self.0);
            let p = page.downgrade();
            motion.connect_leave(move |_| {
                if let (Some(s), Some(p)) = (weak.upgrade(), p.upgrade()) {
                    Self(s).hover(&p, false);
                }
            });
            button.add_controller(motion);
            let drag = gtk::DragSource::new();
            drag.set_actions(gdk::DragAction::MOVE);
            let p = page.downgrade();
            drag.connect_prepare(move |_, _, _| {
                p.upgrade()
                    .map(|p| gdk::ContentProvider::for_value(&p.to_value()))
            });
            let v = view.downgrade();
            let p = page.downgrade();
            drag.connect_drag_cancel(move |_, _, reason| {
                if reason != gdk::DragCancelReason::NoTarget {
                    return false;
                }
                let (Some(v), Some(p)) = (v.upgrade(), p.upgrade()) else {
                    return false;
                };
                let Some(to) = v.emit_by_name::<Option<adw::TabView>>("create-window", &[]) else {
                    return false;
                };
                v.transfer_page(&p, &to, 0);
                true
            });
            button.add_controller(drag);
            let drop = gtk::DropTarget::new(adw::TabPage::static_type(), gdk::DragAction::MOVE);
            let v = view.downgrade();
            let p = page.downgrade();
            drop.connect_drop(move |_, value, _, _| {
                let (Some(v), Some(p)) = (v.upgrade(), p.upgrade()) else {
                    return false;
                };
                place(&v, v.page_position(&p), value)
            });
            button.add_controller(drop);
            let weak = Rc::downgrade(&self.0);
            let notify = page.connect_notify_local(None, move |page, _| {
                if let Some(s) = weak.upgrade() {
                    Self(s).sync_page(page);
                }
            });
            next.push(Tab {
                page: page.downgrade(),
                button,
                label,
                icon,
                indicator,
                spinner,
                close,
                hover: Cell::new(false),
                notify: Some(notify),
            });
        }
        for tab in old {
            if let (Some(page), Some(id)) = (tab.page.upgrade(), tab.notify) {
                page.disconnect(id);
            }
            if let Some(row) = tab.button.parent().and_downcast::<gtk::Box>() {
                row.remove(&tab.button);
            }
        }
        *self.0.tabs.borrow_mut() = next;
        self.layout();
        self.sync();
    }
    fn hover(&self, page: &adw::TabPage, hovering: bool) {
        if let Some(tab) = self
            .0
            .tabs
            .borrow()
            .iter()
            .find(|t| t.page.upgrade().as_ref() == Some(page))
        {
            tab.hover.set(hovering);
        }
        self.sync_page(page);
    }
    fn layout(&self) {
        let buttons = self
            .0
            .tabs
            .borrow()
            .iter()
            .map(|t| t.button.clone())
            .collect::<Vec<_>>();
        let split = if self.0.two.get() && buttons.len() > 1 {
            buttons.len().div_ceil(2)
        } else {
            buttons.len()
        };
        self.0.hosts[1].set_visible(split < buttons.len());
        if self.0.compact.get() {
            self.0.root.add_css_class("compact-tabs");
        } else {
            self.0.root.remove_css_class("compact-tabs");
        }
        let mut previous: [Option<gtk::Widget>; 2] = [None, None];
        for (i, button) in buttons.iter().enumerate() {
            let index = usize::from(i >= split);
            let row = &self.0.rows[index];
            if button.parent().as_ref() != Some(row.upcast_ref()) {
                if let Some(old) = button.parent().and_downcast::<gtk::Box>() {
                    old.remove(button);
                }
                row.append(button);
            }
            row.reorder_child_after(button, previous[index].as_ref());
            previous[index] = Some(button.clone().upcast());
        }
        if let Some(end) = self.0.end.borrow().as_ref() {
            // The action belongs outside the viewport, so overflow never hides it.
            let host = &self.0.hosts[usize::from(split < buttons.len())];
            if end.parent().as_ref() != Some(host.upcast_ref()) {
                if let Some(old) = end.parent().and_downcast::<gtk::Box>() {
                    old.remove(end);
                }
                host.append(end);
            }
        }
    }
    fn sync_page(&self, page: &adw::TabPage) {
        let Some(view) = self.0.view.upgrade() else {
            return;
        };
        let active = view.selected_page().as_ref() == Some(page);
        if let Some(tab) = self
            .0
            .tabs
            .borrow()
            .iter()
            .find(|tab| tab.page.upgrade().as_ref() == Some(page))
        {
            self.render_tab(tab, page, active);
        }
    }
    fn sync(&self) {
        let Some(view) = self.0.view.upgrade() else {
            return;
        };
        let selected = view.selected_page();
        for tab in self.0.tabs.borrow().iter() {
            if let Some(page) = tab.page.upgrade() {
                self.render_tab(tab, &page, selected.as_ref() == Some(&page));
            }
        }
        self.queue_reveal();
    }
    fn queue_reveal(&self) {
        if self.0.reveal_pending.replace(true) {
            return;
        }
        let Some(clock) = self.0.root.frame_clock() else {
            self.0.reveal_pending.set(false);
            return;
        };
        // Allocation may still be pending when selection/row count changes.
        // Observe one completed frame, rather than old bounds from an idle
        // callback. No repeating frame callback or polling timer is retained.
        let weak = Rc::downgrade(&self.0);
        let slot = Rc::new(RefCell::new(None));
        let owned = slot.clone();
        let id = clock.connect_after_paint(move |clock| {
            if let Some(id) = owned.borrow_mut().take() {
                clock.disconnect(id);
            }
            let Some(inner) = weak.upgrade() else {
                return;
            };
            inner.reveal_pending.set(false);
            Self(inner).reveal_selected();
        });
        *slot.borrow_mut() = Some(id);
        self.0.root.queue_draw();
    }
    fn reveal_selected(&self) {
        let inner = &self.0;
        let Some(page) = inner.view.upgrade().and_then(|v| v.selected_page()) else {
            return;
        };
        let button = inner
            .tabs
            .borrow()
            .iter()
            .find(|t| t.page.upgrade().as_ref() == Some(&page))
            .map(|t| t.button.clone());
        let Some(button) = button else {
            return;
        };
        let row = usize::from(button.parent().as_ref() == Some(inner.rows[1].upcast_ref()));
        let Some(bounds) = button.compute_bounds(&inner.rows[row]) else {
            return;
        };
        let adjustment = inner.scroll[row].hadjustment();
        let start = bounds.x() as f64;
        let end = start + bounds.width() as f64;
        if bounds.width() <= 0.0 || adjustment.page_size() <= 0.0 {
            return;
        }
        let value = if start < adjustment.value() {
            start
        } else if end > adjustment.value() + adjustment.page_size() {
            end - adjustment.page_size()
        } else {
            return;
        };
        adjustment.set_value(value.clamp(
            adjustment.lower(),
            (adjustment.upper() - adjustment.page_size()).max(adjustment.lower()),
        ));
    }
    pub fn inspect(&self, window: &adw::ApplicationWindow) -> serde_json::Value {
        serde_json::json!({"two_rows":self.0.two.get(),"compact":self.0.compact.get(),"rows":self.0.hosts.iter().filter(|h|h.is_visible()).count(),"tabs":self.0.tabs.borrow().iter().map(|tab|{
            let page=tab.page.upgrade();let row=usize::from(tab.button.parent().as_ref()==Some(self.0.rows[1].upcast_ref()));
            let bounds=tab.button.compute_bounds(&self.0.scroll[row]);
            serde_json::json!({"id":page.as_ref().map(|p|p.child().widget_name().to_string()),"title":page.as_ref().map(|p|p.title().to_string()),"selected":tab.button.is_active(),"role":format!("{:?}",tab.button.accessible_role()),"row":row,"width":tab.button.width(),"height":tab.button.height(),"origin":tab.button.compute_point(window,&gtk::graphene::Point::new(0.,0.)).map(|p|[p.x(),p.y()]),"inside_viewport":bounds.is_some_and(|r|r.x()>=-0.5&&r.y()>=-0.5&&r.x()+r.width()<=self.0.scroll[row].width() as f32+0.5&&r.y()+r.height()<=self.0.scroll[row].height() as f32+0.5)})
        }).collect::<Vec<_>>()})
    }
    fn render_tab(&self, tab: &Tab, page: &adw::TabPage, active: bool) {
        tab.button.set_active(active);
        tab.button
            .update_state(&[gtk::accessible::State::Selected(Some(active))]);
        tab.label.set_text(&page.title());
        tab.label.set_visible(!page.is_pinned());
        if let Some(icon) = page.icon() {
            tab.icon.set_from_gicon(&icon);
        } else {
            tab.icon.clear();
        }
        tab.icon
            .set_visible(page.icon().is_some() && !page.is_loading());
        tab.spinner.set_visible(page.is_loading());
        tab.spinner.set_spinning(page.is_loading());
        if let Some(icon) = page.indicator_icon() {
            tab.indicator.set_from_gicon(&icon);
        } else {
            tab.indicator.clear();
        }
        tab.indicator.set_visible(page.indicator_icon().is_some());
        tab.icon
            .set_pixel_size(if self.0.compact.get() { 12 } else { 14 });
        tab.indicator.set_pixel_size(10);
        tab.close
            .set_visible(!page.is_pinned() && (active || tab.hover.get()));
        if let Some(tooltip) = page.tooltip() {
            tab.button.set_tooltip_markup(Some(&tooltip));
        } else {
            tab.button.set_tooltip_text(Some(&page.title()));
        }
        tab.button
            .update_property(&[gtk::accessible::Property::Label(page.title().as_str())]);
    }
}
fn popup(view: &adw::TabView, page: &adw::TabPage, button: &gtk::ToggleButton, x: f64, y: f64) {
    let Some(model) = view.menu_model() else {
        return;
    };
    view.emit_by_name::<()>("setup-menu", &[&Some(page.clone())]);
    let pop = gtk::PopoverMenu::from_model(Some(&model));
    pop.set_parent(button);
    pop.set_pointing_to(Some(&gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
    let weak = view.downgrade();
    let anchor = button.downgrade();
    pop.connect_closed(move |p| {
        if let Some(v) = weak.upgrade() {
            v.emit_by_name::<()>("setup-menu", &[&None::<adw::TabPage>]);
        }
        p.unparent();
        if let Some(button) = anchor.upgrade().filter(|b| b.is_mapped()) {
            button.grab_focus();
        }
    });
    pop.popup();
}
fn place(view: &adw::TabView, position: i32, value: &glib::Value) -> bool {
    let Ok(page) = value.get::<adw::TabPage>() else {
        return false;
    };
    let Some(from) = page
        .child()
        .ancestor(adw::TabView::static_type())
        .and_downcast::<adw::TabView>()
    else {
        return false;
    };
    let pinned = view.n_pinned_pages();
    let position = if page.is_pinned() {
        position.clamp(0, pinned)
    } else {
        position.clamp(pinned, view.n_pages())
    };
    if from == *view {
        view.reorder_page(&page, position.min(view.n_pages() - 1));
    } else {
        from.transfer_page(&page, view, position);
    }
    true
}

// GtkLabel reserves space for an ellipsis even when a tab needs less room.
// Keep its natural width, but permit allocation down to zero without scaling
// glyphs or icons. The button's tooltip and accessible name retain the title.
mod fitting_label {
    use super::*;
    use gtk::subclass::prelude::*;
    #[derive(Default)]
    pub struct Label {
        pub child: RefCell<Option<gtk::Label>>,
    }
    #[glib::object_subclass]
    impl ObjectSubclass for Label {
        const NAME: &'static str = "RuwacFittingTabLabel";
        type Type = super::FittingLabel;
        type ParentType = gtk::Widget;
    }
    impl ObjectImpl for Label {
        fn dispose(&self) {
            if let Some(child) = self.child.borrow_mut().take() {
                child.unparent();
            }
        }
    }
    impl WidgetImpl for Label {
        fn measure(&self, orientation: gtk::Orientation, for_size: i32) -> (i32, i32, i32, i32) {
            let measured = self
                .child
                .borrow()
                .as_ref()
                .map(|c| c.measure(orientation, for_size))
                .unwrap_or((0, 0, -1, -1));
            if orientation == gtk::Orientation::Horizontal {
                (0, measured.1, -1, -1)
            } else {
                measured
            }
        }
        fn size_allocate(&self, width: i32, height: i32, baseline: i32) {
            if let Some(child) = self.child.borrow().as_ref() {
                child.allocate(width, height, baseline, None);
            }
        }
        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            if let Some(child) = self.child.borrow().as_ref() {
                self.obj().snapshot_child(child, snapshot);
            }
        }
    }
}
glib::wrapper! {pub struct FittingLabel(ObjectSubclass<fitting_label::Label>) @extends gtk::Widget, @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;}
impl FittingLabel {
    fn new(label: &gtk::Label) -> Self {
        use gtk::subclass::prelude::ObjectSubclassIsExt;
        let widget: Self = glib::Object::new();
        widget.set_hexpand(true);
        widget.set_overflow(gtk::Overflow::Hidden);
        label.set_parent(&widget);
        *widget.imp().child.borrow_mut() = Some(label.clone());
        widget
    }
}
