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
    fitting: FittingLabel,
    icon: gtk::Image,
    indicator: gtk::Image,
    spinner: gtk::Spinner,
    close: gtk::Button,
    number: gtk::Label,
    hint: gtk::Revealer,
    notify: Option<glib::SignalHandlerId>,
}
struct Inner {
    root: gtk::Box,
    rows: [gtk::Box; 2],
    hosts: [gtk::Box; 2],
    scroll: [gtk::ScrolledWindow; 2],
    // Overflow arrows; visible only when a row cannot fit its tabs at minimum width.
    arrows: [(gtk::Button, gtk::Button); 2],
    // Last full row width used for allocation, so unchanged sizes never reflow.
    allocated: [Cell<(i32, usize, usize, bool)>; 2],
    view: glib::WeakRef<adw::TabView>,
    tabs: RefCell<Vec<Tab>>,
    handlers: RefCell<Vec<glib::SignalHandlerId>>,
    end: RefCell<Option<gtk::Widget>>,
    two: Cell<bool>,
    compact: Cell<bool>,
    reveal_pending: Cell<bool>,
    widths_pending: Cell<bool>,
    hints: Cell<bool>,
    menu_generation: Cell<u64>,
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
        let arrows: [(gtk::Button, gtk::Button); 2] = std::array::from_fn(|i| {
            let arrow = |icon: &str, name: &str, tip: &str| {
                let b = gtk::Button::from_icon_name(icon);
                b.add_css_class("flat");
                b.add_css_class("tab-scroll-arrow");
                b.set_widget_name(&format!("{name}-{i}"));
                b.set_tooltip_text(Some(tip));
                b.set_focusable(false);
                b.set_visible(false);
                // Claim the pointer sequence, like the tab close button, so quick
                // repeated clicks never reach the header's double-click maximize.
                let click = gtk::GestureClick::new();
                click.set_button(1);
                click.set_propagation_phase(gtk::PropagationPhase::Capture);
                click.connect_pressed(|gesture, _, _, _| {
                    gesture.set_state(gtk::EventSequenceState::Claimed);
                });
                let weak = b.downgrade();
                click.connect_released(move |_, _, x, y| {
                    if let Some(b) = weak.upgrade() {
                        if b.is_sensitive()
                            && x >= 0.
                            && y >= 0.
                            && x < b.width() as f64
                            && y < b.height() as f64
                        {
                            b.emit_clicked();
                        }
                    }
                });
                b.add_controller(click);
                b
            };
            (
                arrow(
                    "go-previous-symbolic",
                    "tab-scroll-left",
                    "Scroll tabs left",
                ),
                arrow("go-next-symbolic", "tab-scroll-right", "Scroll tabs right"),
            )
        });
        let scroll = std::array::from_fn(|i| {
            // The viewport takes the whole row so its page size is the width
            // available to tabs; tabs stay left-aligned inside it.
            rows[i].set_halign(gtk::Align::Start);
            let s = gtk::ScrolledWindow::builder()
                .child(&rows[i])
                .hscrollbar_policy(gtk::PolicyType::External)
                .vscrollbar_policy(gtk::PolicyType::Never)
                .propagate_natural_height(true)
                .hexpand(true)
                .build();
            hosts[i].append(&arrows[i].0);
            hosts[i].append(&s);
            hosts[i].append(&arrows[i].1);
            root.append(&hosts[i]);
            s
        });
        let inner = Rc::new(Inner {
            root,
            rows,
            hosts,
            scroll,
            arrows,
            allocated: [Cell::new((0, 0, 0, false)), Cell::new((0, 0, 0, false))],
            view: view.downgrade(),
            tabs: RefCell::new(Vec::new()),
            handlers: RefCell::new(Vec::new()),
            end: RefCell::new(None),
            two: Cell::new(false),
            compact: Cell::new(false),
            reveal_pending: Cell::new(false),
            widths_pending: Cell::new(false),
            hints: Cell::new(false),
            menu_generation: Cell::new(0),
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
        let id = view.connect_notify_local(Some("n-pinned-pages"), move |_, _| {
            if let Some(inner) = weak.upgrade() {
                Self(inner).rebuild();
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
        for (row, scroll) in inner.scroll.iter().enumerate() {
            let weak = Rc::downgrade(&inner);
            scroll.hadjustment().connect_changed(move |_| {
                if let Some(inner) = weak.upgrade() {
                    Self(inner).queue_reveal();
                }
            });
            // Only the available width (window size, controls) reallocates tabs.
            let weak = Rc::downgrade(&inner);
            scroll
                .hadjustment()
                .connect_notify_local(Some("page-size"), move |_, _| {
                    if let Some(inner) = weak.upgrade() {
                        Self(inner).queue_widths();
                    }
                });
            let weak = Rc::downgrade(&inner);
            scroll.hadjustment().connect_value_changed(move |_| {
                if let Some(inner) = weak.upgrade() {
                    Self(inner).sync_arrows(row);
                }
            });
            for (arrow, direction) in [(&inner.arrows[row].0, -1.0), (&inner.arrows[row].1, 1.0)] {
                let weak = Rc::downgrade(&inner);
                arrow.connect_clicked(move |_| {
                    if let Some(inner) = weak.upgrade() {
                        Self(inner).scroll_by(row, direction);
                    }
                });
            }
            // A plain mouse wheel over a crowded row scrolls it sideways.
            let wheel = gtk::EventControllerScroll::new(
                gtk::EventControllerScrollFlags::BOTH_AXES
                    | gtk::EventControllerScrollFlags::DISCRETE,
            );
            let weak = Rc::downgrade(&inner);
            wheel.connect_scroll(move |_, dx, dy| {
                let Some(inner) = weak.upgrade() else {
                    return glib::Propagation::Proceed;
                };
                // Only an overflowing row scrolls sideways with the wheel.
                if !inner.arrows[row].0.is_visible() {
                    return glib::Propagation::Proceed;
                }
                let step = if dx != 0.0 { dx } else { dy };
                Self(inner).scroll_by(row, step.signum());
                glib::Propagation::Stop
            });
            scroll.add_controller(wheel);
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
            button.set_hexpand(false);
            let body = gtk::Box::new(gtk::Orientation::Horizontal, 2);
            let icon = gtk::Image::new();
            icon.set_size_request(16, 16);
            let indicator = gtk::Image::new();
            indicator.set_size_request(16, 16);
            let spinner = gtk::Spinner::new();
            let label = gtk::Label::new(None);
            label.set_ellipsize(gtk::pango::EllipsizeMode::End);
            label.set_single_line_mode(true);
            label.set_width_chars(0);
            label.set_max_width_chars(18);
            label.set_hexpand(true);
            label.set_xalign(0.0);
            let close = gtk::Button::from_icon_name("window-close-symbolic");
            close.add_css_class("tab-close");
            close.set_focusable(false);
            // Keep the nested close control's pointer sequence away from the
            // surrounding toggle/drag gestures. Keyboard activation still uses clicked.
            let close_click = gtk::GestureClick::new();
            close_click.set_button(1);
            close_click.set_propagation_phase(gtk::PropagationPhase::Capture);
            close_click.connect_pressed(|gesture, _, _, _| {
                gesture.set_state(gtk::EventSequenceState::Claimed);
            });
            let weak_close = close.downgrade();
            close_click.connect_released(move |_, _, x, y| {
                if let Some(close) = weak_close.upgrade() {
                    if x >= 0. && y >= 0. && x < close.width() as f64 && y < close.height() as f64 {
                        close.emit_clicked();
                    }
                }
            });
            close.add_controller(close_click);
            close.set_tooltip_text(Some("Close tab"));
            let activity = gtk::Overlay::new();
            activity.set_child(Some(&icon));
            spinner.set_halign(gtk::Align::Center);
            spinner.set_valign(gtk::Align::Center);
            activity.add_overlay(&spinner);
            activity.set_measure_overlay(&spinner, false);
            body.append(&activity);
            body.append(&indicator);
            let fitting = FittingLabel::new(&label);
            fitting.set_natural_width(12);
            body.append(&fitting);
            body.append(&close);
            let number = gtk::Label::new(None);
            number.add_css_class("tab-shortcut-hint");
            let hint = gtk::Revealer::builder()
                .child(&number)
                .transition_type(gtk::RevealerTransitionType::Crossfade)
                .transition_duration(120)
                .halign(gtk::Align::Start)
                .valign(gtk::Align::Center)
                .can_target(false)
                .build();
            let overlay = gtk::Overlay::new();
            overlay.set_child(Some(&body));
            overlay.add_overlay(&hint);
            overlay.set_measure_overlay(&hint, false);
            button.set_child(Some(&overlay));
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
            let inner = Rc::downgrade(&self.0);
            menu.connect_pressed(move |_, _, x, y| {
                if let (Some(v), Some(p), Some(b), Some(inner)) =
                    (v.upgrade(), p.upgrade(), b.upgrade(), inner.upgrade())
                {
                    popup(&inner, &v, &p, &b, x, y);
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
                    popup(&inner, &view, &page, &button, 0.0, button.height() as f64);
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
                fitting,
                icon,
                indicator,
                spinner,
                close,
                number,
                hint,
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
        self.sync_hint_numbers();
        self.queue_widths();
    }
    fn queue_widths(&self) {
        if self.0.widths_pending.replace(true) {
            return;
        }
        // Allocate after the add/remove/reorder or resize completes. Title,
        // status, hover and selection changes never call this.
        let weak = Rc::downgrade(&self.0);
        glib::idle_add_local_once(move || {
            let Some(inner) = weak.upgrade() else {
                return;
            };
            inner.widths_pending.set(false);
            let strip = Self(inner);
            strip.allocate_widths();
            strip.queue_reveal();
        });
    }
    /// Equal tab widths per row: at most about a fifth of the row while few
    /// tabs are open, shrinking as tabs are added down to a readable minimum,
    /// beyond which the row scrolls with arrows. Pinned tabs keep icon width.
    fn allocate_widths(&self) {
        let inner = &self.0;
        let compact = inner.compact.get();
        let minimum = if compact { 84 } else { 108 };
        let tabs = inner.tabs.borrow();
        for row in 0..2 {
            let host = &inner.hosts[row];
            if !host.is_visible() {
                inner.allocated[row].set((0, 0, 0, false));
                continue;
            }
            let members = tabs
                .iter()
                .filter(|t| t.button.parent().as_ref() == Some(inner.rows[row].upcast_ref()))
                .collect::<Vec<_>>();
            let pinned = |t: &&Tab| t.page.upgrade().is_some_and(|p| p.is_pinned());
            let flexible = members.iter().filter(|t| !pinned(t)).count();
            let fixed: i32 = members
                .iter()
                .filter(|t| pinned(t))
                .map(|t| t.button.measure(gtk::Orientation::Horizontal, -1).1 + 2)
                .sum();
            let page = inner.scroll[row].hadjustment().page_size() as i32;
            if page <= 0 {
                continue;
            }
            let (left, right) = &inner.arrows[row];
            let arrows = if left.is_visible() {
                left.width() + right.width()
            } else {
                0
            };
            let full = page + arrows;
            // Pinning changes widths without changing the tab count.
            if inner.allocated[row].get() == (full, members.len(), flexible, compact)
                && flexible > 0
            {
                let w = members
                    .iter()
                    .find(|t| !pinned(t))
                    .map(|t| t.button.width_request());
                if w.is_some_and(|w| w > 0) {
                    continue;
                }
            }
            let space = (full - fixed).max(0);
            // Each tab also has a 1 px margin on both sides.
            let per = if flexible == 0 {
                0
            } else {
                space / flexible as i32 - 2
            };
            let cap = ((full as f64) * 0.22) as i32;
            let overflow = flexible > 0 && per < minimum;
            let width = if overflow {
                minimum
            } else {
                per.min(cap.max(minimum))
            };
            for tab in &members {
                tab.button
                    .set_size_request(if pinned(tab) { -1 } else { width }, -1);
            }
            left.set_visible(overflow);
            right.set_visible(overflow);
            inner.allocated[row].set((full, members.len(), flexible, compact));
            self.sync_arrows(row);
        }
    }
    fn sync_arrows(&self, row: usize) {
        let adjustment = self.0.scroll[row].hadjustment();
        let (left, right) = &self.0.arrows[row];
        left.set_sensitive(adjustment.value() > adjustment.lower() + 0.5);
        right.set_sensitive(adjustment.value() + adjustment.page_size() < adjustment.upper() - 0.5);
    }
    fn scroll_by(&self, row: usize, direction: f64) {
        let adjustment = self.0.scroll[row].hadjustment();
        let step = self
            .0
            .tabs
            .borrow()
            .iter()
            .map(|t| t.button.width_request())
            .find(|w| *w > 0)
            .unwrap_or(108) as f64
            + 2.0;
        adjustment.set_value((adjustment.value() + direction * step).clamp(
            adjustment.lower(),
            (adjustment.upper() - adjustment.page_size()).max(adjustment.lower()),
        ));
    }
    fn sync_hint_numbers(&self) {
        for (index, tab) in self.0.tabs.borrow().iter().enumerate() {
            let shortcut = match index {
                0..=8 => (index + 1).to_string(),
                9 => "0".to_owned(),
                _ => String::new(),
            };
            tab.number.set_text(&shortcut);
            tab.hint.set_reveal_child(self.0.hints.get() && index < 10);
        }
    }
    fn set_hints(&self, shown: bool) {
        if self.0.hints.replace(shown) == shown {
            return;
        }
        for (index, tab) in self.0.tabs.borrow().iter().enumerate() {
            tab.hint.set_reveal_child(shown && index < 10);
        }
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
        // Row membership or density changed: reallocate equal widths.
        self.queue_widths();
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
        fn items(
            w: &gtk::Widget,
            window: &adw::ApplicationWindow,
            out: &mut Vec<serde_json::Value>,
        ) {
            if w.type_().name() == "GtkModelButton" && w.is_mapped() {
                out.push(serde_json::json!({"label":w.property::<String>("text"),"sensitive":w.is_sensitive(),"focused":w.has_focus(),"width":w.width(),"height":w.height(),"origin":w.compute_point(window,&gtk::graphene::Point::new(0.,0.)).map(|p|[p.x(),p.y()])}));
            }
            let mut child = w.first_child();
            while let Some(w) = child {
                child = w.next_sibling();
                items(&w, window, out);
            }
        }
        let mut menu_items = Vec::new();
        items(self.0.root.upcast_ref(), window, &mut menu_items);
        serde_json::json!({"menu_items":menu_items,"row_widths":self.0.scroll.iter().map(|s|s.width()).collect::<Vec<_>>(),"scroll":self.0.scroll.iter().map(|s|{let a=s.hadjustment();serde_json::json!({"value":a.value(),"upper":a.upper(),"page":a.page_size()})}).collect::<Vec<_>>(),"arrows":self.0.arrows.iter().map(|(l,r)|serde_json::json!({"visible":l.is_visible()&&r.is_visible(),"left_sensitive":l.is_sensitive(),"right_sensitive":r.is_sensitive(),"left_origin":l.compute_point(window,&gtk::graphene::Point::new(0.,0.)).map(|p|[p.x(),p.y()]),"right_origin":r.compute_point(window,&gtk::graphene::Point::new(0.,0.)).map(|p|[p.x(),p.y()]),"width":l.width(),"height":l.height()})).collect::<Vec<_>>(),"two_rows":self.0.two.get(),"compact":self.0.compact.get(),"rows":self.0.hosts.iter().filter(|h|h.is_visible()).count(),"tabs":self.0.tabs.borrow().iter().map(|tab|{
            let page=tab.page.upgrade();let row=usize::from(tab.button.parent().as_ref()==Some(self.0.rows[1].upcast_ref()));
            let bounds=tab.button.compute_bounds(&self.0.scroll[row]);
            serde_json::json!({"id":page.as_ref().map(|p|p.child().widget_name().to_string()),"title":page.as_ref().map(|p|p.title().to_string()),"selected":tab.button.is_active(),"close_visible":tab.close.is_visible(),"close_width":tab.close.width(),"close_height":tab.close.height(),"close_pick":tab.close.compute_point(window,&gtk::graphene::Point::new(tab.close.width() as f32/2.,tab.close.height() as f32/2.)).and_then(|p|window.pick(p.x()as f64,p.y()as f64,gtk::PickFlags::DEFAULT)).map(|w|w.type_().name().to_string()),"close_origin":tab.close.compute_point(window,&gtk::graphene::Point::new(0.,0.)).map(|p|[p.x(),p.y()]),"font_size":tab.label.pango_context().font_description().map(|d|d.size()),"icon_size":tab.icon.pixel_size(),"indicator_size":tab.indicator.pixel_size(),"hint":tab.number.text().to_string(),"hint_visible":tab.hint.reveals_child(),"role":format!("{:?}",tab.button.accessible_role()),"row":row,"width":tab.button.width(),"width_request":tab.button.width_request(),"height":tab.button.height(),"origin":tab.button.compute_point(window,&gtk::graphene::Point::new(0.,0.)).map(|p|[p.x(),p.y()]),"inside_viewport":bounds.is_some_and(|r|r.x()>=-0.5&&r.y()>=-0.5&&r.x()+r.width()<=self.0.scroll[row].width() as f32+0.5&&r.y()+r.height()<=self.0.scroll[row].height() as f32+0.5)})
        }).collect::<Vec<_>>()})
    }
    fn render_tab(&self, tab: &Tab, page: &adw::TabPage, active: bool) {
        tab.button.set_active(active);
        tab.button
            .update_state(&[gtk::accessible::State::Selected(Some(active))]);
        tab.label.set_text(&page.title());
        tab.label.set_visible(!page.is_pinned());
        tab.fitting.set_visible(!page.is_pinned());
        if let Some(icon) = page.icon() {
            tab.icon.set_from_gicon(&icon);
        } else {
            tab.icon.clear();
        }
        tab.icon
            .set_opacity(if page.icon().is_some() && !page.is_loading() {
                1.0
            } else {
                0.0
            });
        tab.spinner.set_visible(page.is_loading());
        tab.spinner.set_spinning(page.is_loading());
        if let Some(icon) = page.indicator_icon() {
            tab.indicator.set_from_gicon(&icon);
        } else {
            tab.indicator.clear();
        }
        tab.indicator
            .set_opacity(if page.indicator_icon().is_some() {
                1.0
            } else {
                0.0
            });
        tab.icon.set_pixel_size(16);
        tab.indicator.set_pixel_size(16);
        tab.close.set_visible(!page.is_pinned());
        if let Some(tooltip) = page.tooltip() {
            tab.button.set_tooltip_markup(Some(&tooltip));
        } else {
            tab.button.set_tooltip_text(Some(&page.title()));
        }
        tab.button
            .update_property(&[gtk::accessible::Property::Label(page.title().as_str())]);
    }
}
/// Passive overlays follow the existing window-local Map keys policy. They never
/// consume a key, change selection, or retain a repeating animation/timer.
pub fn bind_key_hints(window: &gtk::Window, mode: &Rc<crate::shortcuts::LocalMode>) {
    fn show(window: &gtk::Window, shown: bool) {
        STRIPS.with(|all| {
            for inner in all.borrow().iter().filter_map(Weak::upgrade) {
                if inner.root.root().and_downcast::<gtk::Window>().as_ref() == Some(window) {
                    TabStrip(inner).set_hints(shown);
                }
            }
        });
    }
    let keys = gtk::EventControllerKey::new();
    keys.set_propagation_phase(gtk::PropagationPhase::Capture);
    let weak = window.downgrade();
    let mode = Rc::downgrade(mode);
    keys.connect_key_pressed(move |_, key, _, mods| {
        if let (Some(window), Some(mode)) = (weak.upgrade(), mode.upgrade()) {
            let modifiers = mods - gdk::ModifierType::LOCK_MASK;
            let plain_alt = modifiers == gdk::ModifierType::ALT_MASK
                || (key == gdk::Key::Alt_L && modifiers.is_empty());
            show(
                &window,
                plain_alt && !crate::shortcuts::altgr(key) && !mode.active(),
            );
        }
        glib::Propagation::Proceed
    });
    let weak = window.downgrade();
    keys.connect_key_released(move |_, key, _, _| {
        if matches!(
            key,
            gdk::Key::Alt_L | gdk::Key::Alt_R | gdk::Key::ISO_Level3_Shift
        ) {
            if let Some(window) = weak.upgrade() {
                show(&window, false);
            }
        }
    });
    window.add_controller(keys);
    window.connect_is_active_notify(|window| {
        if !window.is_active() {
            show(window, false);
        }
    });
}

fn popup(
    inner: &Rc<Inner>,
    view: &adw::TabView,
    page: &adw::TabPage,
    button: &gtk::ToggleButton,
    x: f64,
    y: f64,
) {
    let Some(model) = view.menu_model() else {
        return;
    };
    let generation = inner.menu_generation.get().wrapping_add(1);
    inner.menu_generation.set(generation);
    view.emit_by_name::<()>("setup-menu", &[&Some(page.clone())]);
    let pop = gtk::PopoverMenu::from_model(Some(&model));
    pop.set_parent(button);
    pop.set_pointing_to(Some(&gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
    let weak = view.downgrade();
    let owner = Rc::downgrade(inner);
    pop.connect_closed(move |p| {
        // GtkPopoverMenu closes before dispatching the chosen action. Keep its
        // action ancestry and selected page alive until that dispatch finishes.
        let (p, weak, owner) = (p.clone(), weak.clone(), owner.clone());
        // Close tab can remove the toggle before this idle runs. Keep the
        // parent alive until its popover is unparented, including GTK focus /
        // accessibility cleanup during action dispatch.
        let parent = p.parent();
        glib::idle_add_local_once(move || {
            if owner
                .upgrade()
                .is_some_and(|s| s.menu_generation.get() == generation)
            {
                if let Some(v) = weak.upgrade() {
                    v.emit_by_name::<()>("setup-menu", &[&None::<adw::TabPage>]);
                }
            }
            // A dismissed older menu must never clear a newly opened target.
            p.unparent();
            drop(parent);
        });
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

// Cache the natural title width at structural changes. Metadata updates may
// change the text, but cannot change horizontal requisition. Crowded rows may
// ellipsize the label without scaling its glyphs or icons.
mod fitting_label {
    use super::*;
    use gtk::subclass::prelude::*;
    #[derive(Default)]
    pub struct Label {
        pub child: RefCell<Option<gtk::Label>>,
        pub natural: Cell<i32>,
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
            if orientation == gtk::Orientation::Horizontal {
                let width = self.natural.get();
                (width.min(12), width, -1, -1)
            } else {
                self.child
                    .borrow()
                    .as_ref()
                    .map(|child| child.measure(orientation, for_size))
                    .unwrap_or((0, 0, -1, -1))
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
    fn set_natural_width(&self, width: i32) {
        use gtk::subclass::prelude::ObjectSubclassIsExt;
        if self.imp().natural.replace(width) != width {
            self.queue_resize();
        }
    }
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
