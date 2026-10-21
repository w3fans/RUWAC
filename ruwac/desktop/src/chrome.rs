//! Compact native desktop chrome. Existing action widgets retain their handlers.
use crate::{
    schemes,
    sidebar_shell::{Settings, Shell},
};
use adw::prelude::*;
use gtk::{gdk, glib};
use std::{cell::RefCell, rc::Rc};

pub fn font_available(name: &str) -> bool {
    pangocairo::FontMap::default()
        .list_families()
        .iter()
        .any(|f| f.name() == name)
}
pub fn bar_child(icon: &str, label: &str) -> gtk::Box {
    let b = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    let i = gtk::Image::from_icon_name(icon);
    i.set_pixel_size(15);
    b.append(&i);
    let l = gtk::Label::new(Some(label));
    l.add_css_class("bar-label");
    b.append(&l);
    b
}
pub fn walk(w: &gtk::Widget, f: &dyn Fn(&gtk::Widget)) {
    f(w);
    let mut c = w.first_child();
    while let Some(w) = c {
        c = w.next_sibling();
        walk(&w, f);
    }
}
pub fn action(button: &gtk::Button, icon: &str, label: &str, tooltip: &str) {
    button.set_child(Some(&bar_child(icon, label)));
    button.set_tooltip_text(Some(tooltip));
    button.add_css_class("flat");
    button.update_property(&[gtk::accessible::Property::Label(tooltip)]);
}
pub fn detach(w: &impl IsA<gtk::Widget>) {
    let w = w.as_ref();
    if let Some(p) = w.parent() {
        if let Some(b) = p.downcast_ref::<gtk::Box>() {
            b.remove(w);
        } else if let Some(g) = p.downcast_ref::<gtk::Grid>() {
            g.remove(w);
        } else if let Some(s) = p.downcast_ref::<gtk::ScrolledWindow>() {
            s.set_child(None::<&gtk::Widget>);
        } else if let Some(p) = p.downcast_ref::<gtk::Popover>() {
            p.set_child(None::<&gtk::Widget>);
        }
    }
}
pub struct Chrome {
    pub actions: gtk::Box,
    pub controls: gtk::MenuButton,
    header: adw::HeaderBar,
    bar: adw::TabBar,
    reveal: gtk::Revealer,
    collapse: gtk::Button,
    pop: gtk::Popover,
    grid: gtk::Grid,
    dock: gtk::ScrolledWindow,
    dock_toggle: gtk::ToggleButton,
    prefs: RefCell<Settings>,
    scheme: gtk::CssProvider,
    status: gtk::Label,
    changed: Rc<dyn Fn(&Settings)>,
}
impl Chrome {
    pub fn new(
        window: &adw::ApplicationWindow,
        header: &adw::HeaderBar,
        bar: &adw::TabBar,
        body: &gtk::Box,
        changed: Rc<dyn Fn(&Settings)>,
    ) -> Rc<Self> {
        let prefs = Shell::prefs();
        schemes::select(&prefs.scheme);
        window.add_css_class("ruwac");
        header.add_css_class("ruwac-bar");
        bar.add_css_class("ruwac-tabs");
        header.set_show_start_title_buttons(false);
        header.set_centering_policy(adw::CenteringPolicy::Loose);
        header.set_title_widget(None::<&gtk::Widget>);
        let title = TabTitle::new(bar);
        header.set_title_widget(Some(&title));
        bar.set_expand_tabs(true);
        bar.set_hexpand(true);
        let static_css = gtk::CssProvider::new();
        static_css.load_from_string(include_str!("chrome.css"));
        let scheme = gtk::CssProvider::new();
        scheme.load_from_string(&schemes::css(schemes::current()));
        for provider in [&static_css, &scheme] {
            gtk::style_context_add_provider_for_display(
                &gtk::prelude::WidgetExt::display(window),
                provider,
                gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
            );
        }
        let actions = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        let reveal = gtk::Revealer::builder()
            .child(&actions)
            .transition_type(gtk::RevealerTransitionType::SlideRight)
            .transition_duration(150)
            .reveal_child(!prefs.actions_collapsed)
            .build();
        let collapse = gtk::Button::from_icon_name("go-first-symbolic");
        collapse.set_widget_name("actions-collapse");
        header.pack_start(&collapse);
        header.pack_start(&reveal);
        header.pack_start(&gtk::Separator::new(gtk::Orientation::Vertical));
        let controls = gtk::MenuButton::new();
        controls.set_widget_name("controls-button");
        controls.set_child(Some(&bar_child("emblem-system-symbolic", "Controls")));
        controls.set_tooltip_text(Some("Session controls · F10 / Ctrl+. · Ctrl+Alt+Home releases remote focus · Ctrl+Alt+Enter fullscreen"));
        let pop = gtk::Popover::new();
        pop.add_css_class("ruwac-pop");
        controls.set_popover(Some(&pop));
        header.pack_end(&controls);
        let grid = gtk::Grid::builder()
            .column_spacing(12)
            .row_spacing(6)
            .build();
        let dock = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Automatic)
            .vscrollbar_policy(gtk::PolicyType::Never)
            .height_request(30)
            .build();
        dock.set_widget_name("controls-dock");
        body.insert_child_after(&dock, Some(header));
        let dock_toggle = gtk::ToggleButton::with_label("Dock controls");
        dock_toggle.set_widget_name("controls-dock-toggle");
        let status = gtk::Label::new(None);
        status.set_wrap(true);
        let this = Rc::new(Self {
            actions,
            controls,
            header: header.clone(),
            bar: bar.clone(),
            reveal,
            collapse,
            pop,
            grid,
            dock,
            dock_toggle,
            prefs: RefCell::new(prefs),
            scheme,
            status,
            changed,
        });
        let weak = Rc::downgrade(&this);
        this.collapse.connect_clicked(move |_| {
            if let Some(c) = weak.upgrade() {
                c.update(|p| p.actions_collapsed = !p.actions_collapsed);
            }
        });
        let weak = Rc::downgrade(&this);
        this.dock_toggle.connect_toggled(move |b| {
            if let Some(c) = weak.upgrade() {
                if c.prefs.borrow().controls_docked != b.is_active() {
                    c.update(|p| p.controls_docked = b.is_active());
                }
            }
        });
        let weak = Rc::downgrade(&this);
        header.add_tick_callback(move |h, _| {
            let Some(c) = weak.upgrade() else {
                return glib::ControlFlow::Break;
            };
            let show = c.prefs.borrow().bar_labels && h.width() >= 1100;
            walk(h.upcast_ref(), &|w| {
                if w.has_css_class("start") || w.has_css_class("end") {
                    if let Some(b) = w.downcast_ref::<gtk::Box>() {
                        if b.spacing() != 0 {
                            b.set_spacing(0);
                        }
                    }
                }
                if w.has_css_class("bar-label") {
                    let visible =
                        show && !(c.prefs.borrow().actions_collapsed && w.is_ancestor(&c.controls));
                    if w.is_visible() != visible {
                        w.set_visible(visible);
                    }
                }
            });
            glib::ControlFlow::Continue
        });
        this.appearance(header);
        this.apply();
        this
    }
    fn update(&self, f: impl FnOnce(&mut Settings)) {
        let mut p = self.prefs.borrow().clone();
        f(&mut p);
        match Shell::update(|saved| {
            saved.scheme = p.scheme.clone();
            saved.term_font = p.term_font.clone();
            saved.term_size_pt = p.term_size_pt;
            saved.line_height = p.line_height;
            saved.bar_labels = p.bar_labels;
            saved.vivid_colors = p.vivid_colors;
            saved.actions_collapsed = p.actions_collapsed;
            saved.controls_docked = p.controls_docked;
        }) {
            Ok(p) => {
                *self.prefs.borrow_mut() = p;
                self.status.set_text("");
            }
            Err(e) => {
                *self.prefs.borrow_mut() = p;
                self.status
                    .set_text(&format!("{e}; appearance applies to this window only"));
            }
        }
        self.apply();
    }
    fn apply(&self) {
        let p = self.prefs.borrow();
        let s = schemes::select(&p.scheme).unwrap_or(&schemes::SCHEMES[0]);
        schemes::set_vivid(p.vivid_colors);
        self.scheme.load_from_string(&schemes::css(s));
        adw::StyleManager::default().set_color_scheme(if s.id == "paper" {
            adw::ColorScheme::ForceLight
        } else {
            adw::ColorScheme::ForceDark
        });
        self.reveal.set_reveal_child(!p.actions_collapsed);
        self.collapse.set_icon_name(if p.actions_collapsed {
            "view-app-grid-symbolic"
        } else {
            "go-first-symbolic"
        });
        self.collapse.set_tooltip_text(Some(&format!(
            "{} · RUWAC {}",
            if p.actions_collapsed {
                "Show actions"
            } else {
                "Collapse actions"
            },
            env!("CARGO_PKG_VERSION")
        )));
        let docked = p.controls_docked;
        drop(p);
        self.position_controls(docked);
        (self.changed)(&self.prefs.borrow());
    }
    fn position_controls(&self, docked: bool) {
        // Explicit docking changes content height once. Opening the popover never does.
        detach(&self.grid);
        self.pop.set_child(None::<&gtk::Widget>);
        self.dock.set_child(None::<&gtk::Widget>);
        self.dock_toggle.set_active(docked);
        let container = gtk::Box::new(gtk::Orientation::Vertical, 4);
        detach(&self.dock_toggle);
        container.append(&self.dock_toggle);
        if docked {
            let row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
            row.append(&self.grid);
            let undock = self.dock_toggle.clone();
            let button = gtk::Button::with_label("Undock");
            button.connect_clicked(move |_| undock.set_active(false));
            row.append(&button);
            self.dock.set_child(Some(&row));
            container.append(&gtk::Label::new(Some(
                "Controls are docked below the header",
            )));
        } else {
            container.append(&self.grid);
        }
        self.pop.set_child(Some(&container));
        self.dock.set_visible(docked);
    }
    /// A keyboard menu must also work when header actions are collapsed.
    pub fn show_actions(&self) {
        self.reveal.set_reveal_child(true);
    }
    pub fn dock_toggle_active(&self) -> bool {
        self.prefs.borrow().controls_docked
    }
    pub fn rebuild(
        &self,
        session: Option<&gtk::Box>,
        clipboard: &gtk::Box,
        layout: Option<&gtk::Box>,
    ) {
        while let Some(c) = self.grid.first_child() {
            self.grid.remove(&c);
        }
        for (i, (name, widget)) in [
            ("Session", session),
            ("Clipboard", Some(clipboard)),
            ("Layout", layout),
        ]
        .into_iter()
        .enumerate()
        {
            if let Some(widget) = widget {
                detach(widget);
                widget.set_orientation(if self.prefs.borrow().controls_docked {
                    gtk::Orientation::Horizontal
                } else {
                    gtk::Orientation::Vertical
                });
                widget.set_margin_top(0);
                widget.set_margin_bottom(0);
                let section = gtk::Box::new(gtk::Orientation::Vertical, 2);
                let title = gtk::Label::new(Some(name));
                title.add_css_class("section-title");
                title.set_visible(!self.prefs.borrow().controls_docked);
                section.append(&title);
                section.append(widget);
                self.grid.attach(&section, i as i32, 0, 1, 1);
            }
        }
    }
    fn appearance(self: &Rc<Self>, header: &adw::HeaderBar) {
        let button = gtk::MenuButton::builder()
            .icon_name("applications-graphics-symbolic")
            .tooltip_text("Appearance")
            .build();
        button.set_widget_name("appearance-button");
        header.pack_end(&button);
        let pop = gtk::Popover::new();
        pop.add_css_class("ruwac-pop");
        let body = gtk::Box::new(gtk::Orientation::Vertical, 6);
        pop.set_child(Some(&body));
        button.set_popover(Some(&pop));
        body.append(&gtk::Label::new(Some("Color scheme")));
        let grid = gtk::Grid::builder()
            .column_spacing(4)
            .row_spacing(4)
            .build();
        let mut group = None::<gtk::ToggleButton>;
        for (i, s) in schemes::SCHEMES.iter().enumerate() {
            let swatch = gtk::ToggleButton::new();
            if let Some(g) = &group {
                swatch.set_group(Some(g));
            } else {
                group = Some(swatch.clone());
            }
            let b = gtk::Box::new(gtk::Orientation::Vertical, 2);
            let canvas = gtk::DrawingArea::builder()
                .content_width(96)
                .content_height(26)
                .build();
            canvas.set_draw_func(move |_, cr, w, h| {
                let c = s.bg();
                cr.set_source_rgb(c.0, c.1, c.2);
                let _ = cr.paint();
                let c = schemes::rgb(s.colors[1]);
                cr.set_source_rgb(c.0, c.1, c.2);
                cr.rectangle(0., 0., w as f64, 5.);
                let _ = cr.fill();
                for (j, k) in [4, 2, 3, 1].into_iter().enumerate() {
                    let c = s.ansi(k);
                    cr.set_source_rgb(c.0, c.1, c.2);
                    cr.arc(
                        16. + j as f64 * 21.,
                        h as f64 * 0.6,
                        3.,
                        0.,
                        std::f64::consts::TAU,
                    );
                    let _ = cr.fill();
                }
            });
            b.append(&canvas);
            b.append(&gtk::Label::new(Some(s.name)));
            swatch.set_child(Some(&b));
            swatch.set_active(self.prefs.borrow().scheme == s.id);
            let weak = Rc::downgrade(self);
            swatch.connect_toggled(move |b| {
                if b.is_active() {
                    if let Some(c) = weak.upgrade() {
                        c.update(|p| p.scheme = s.id.into());
                    }
                }
            });
            grid.attach(&swatch, (i % 3) as i32, (i / 3) as i32, 1, 1);
        }
        body.append(&grid);
        body.append(&gtk::Label::new(Some("Terminal font")));
        for name in crate::sidebar_shell::FONTS {
            let b = gtk::Button::new();
            let l = gtk::Label::new(Some(&format!("{name} · ls ~/work 0O lI1 {{}}")));
            let attrs = gtk::pango::AttrList::new();
            attrs.insert(gtk::pango::AttrFontDesc::new(
                &gtk::pango::FontDescription::from_string(&format!("{name} 11")),
            ));
            l.set_attributes(Some(&attrs));
            b.set_child(Some(&l));
            b.set_sensitive(font_available(name));
            if !b.is_sensitive() {
                b.set_tooltip_text(Some("Not installed"));
            }
            let weak = Rc::downgrade(self);
            b.connect_clicked(move |_| {
                if let Some(c) = weak.upgrade() {
                    c.update(|p| p.term_font = name.into());
                }
            });
            body.append(&b);
        }
        let size = gtk::SpinButton::with_range(9., 18., 1.);
        size.set_value(self.prefs.borrow().term_size_pt as f64);
        body.append(&gtk::Label::new(Some("Size (pt) · was Monospace 11")));
        body.append(&size);
        let weak = Rc::downgrade(self);
        size.connect_value_changed(move |s| {
            if let Some(c) = weak.upgrade() {
                c.update(|p| p.term_size_pt = s.value_as_int() as u8);
            }
        });
        let height = gtk::DropDown::from_strings(&[
            "Tight · 0.95",
            "Standard · 1.00",
            "Relaxed · 1.15",
            "Normal · 1.30",
            "Relaxed · 1.45",
        ]);
        height.set_selected(
            crate::sidebar_shell::LINE_HEIGHTS
                .iter()
                .position(|h| *h == self.prefs.borrow().line_height)
                .unwrap_or(0) as u32,
        );
        body.append(&height);
        let weak = Rc::downgrade(self);
        height.connect_selected_notify(move |d| {
            if let Some(c) = weak.upgrade() {
                c.update(|p| {
                    p.line_height = crate::sidebar_shell::LINE_HEIGHTS[d.selected().min(4) as usize]
                });
            }
        });
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        row.append(&gtk::Label::new(Some("Bar labels")));
        let labels = gtk::Switch::new();
        labels.set_active(self.prefs.borrow().bar_labels);
        row.append(&labels);
        body.append(&row);
        let weak = Rc::downgrade(self);
        labels.connect_active_notify(move |s| {
            if let Some(c) = weak.upgrade() {
                c.update(|p| p.bar_labels = s.is_active());
            }
        });
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        row.append(&gtk::Label::new(Some("Vivid terminal colours")));
        let vivid = gtk::Switch::new();
        vivid.set_widget_name("vivid-terminal-colours");
        vivid.set_tooltip_text(Some(
            "Standard GNOME Terminal colours for programs like btm; off uses the scheme's softer colours",
        ));
        vivid.set_active(self.prefs.borrow().vivid_colors);
        row.append(&vivid);
        body.append(&row);
        let weak = Rc::downgrade(self);
        vivid.connect_active_notify(move |s| {
            if let Some(c) = weak.upgrade() {
                c.update(|p| p.vivid_colors = s.is_active());
            }
        });
        body.append(&self.status);
    }
    pub fn fixture(
        &self,
        window: &adw::ApplicationWindow,
        content: &impl IsA<gtk::Widget>,
        q: &serde_json::Value,
    ) -> serde_json::Value {
        if let (Some(w), Some(h)) = (q["width"].as_i64(), q["height"].as_i64()) {
            window.set_default_size(w.clamp(500, 3200) as i32, h.clamp(400, 2000) as i32);
        }
        if let Some(value) = q["maximized"].as_bool() {
            if value {
                window.maximize();
            } else {
                window.unmaximize();
            }
        }
        if let Some(value) = q["fullscreen"].as_bool() {
            if value {
                window.fullscreen();
            } else {
                window.unfullscreen();
            }
        }
        if q["open"] == true {
            self.controls.popup();
        } else if q["open"] == false {
            self.controls.popdown();
        }
        if [
            "collapse",
            "dock",
            "scheme",
            "font",
            "size",
            "labels",
            "line_height",
        ]
        .iter()
        .any(|k| !q[k].is_null())
        {
            self.update(|p| {
                if let Some(v) = q["collapse"].as_bool() {
                    p.actions_collapsed = v;
                }
                if let Some(v) = q["dock"].as_bool() {
                    p.controls_docked = v;
                }
                if let Some(v) = q["scheme"]
                    .as_str()
                    .filter(|s| schemes::SCHEMES.iter().any(|c| c.id == *s))
                {
                    p.scheme = v.into();
                }
                if let Some(v) = q["font"]
                    .as_str()
                    .filter(|f| crate::sidebar_shell::FONTS.contains(f))
                {
                    p.term_font = v.into();
                }
                if let Some(v) = q["size"].as_u64().filter(|v| (9..=18).contains(v)) {
                    p.term_size_pt = v as u8;
                }
                if let Some(v) = q["labels"].as_bool() {
                    p.bar_labels = v;
                }
                if let Some(v) = q["line_height"]
                    .as_f64()
                    .filter(|v| crate::sidebar_shell::LINE_HEIGHTS.contains(&(*v as f32)))
                {
                    p.line_height = v as f32;
                }
            });
        }
        let p = self.prefs.borrow();
        let tree = RefCell::new(Vec::new());
        walk(self.header.upcast_ref(), &|w| {
            if w.is_visible() {
                tree.borrow_mut().push(serde_json::json!({"type":w.type_().name(),"name":w.widget_name().as_str(),"height":w.height(),"width":w.width(),"css":w.css_classes().iter().map(|s|s.to_string()).collect::<Vec<_>>()}));
            }
        });
        serde_json::json!({"header_tree":tree.into_inner(),"header_height":self.header.height(),"tab_bar_width":self.bar.width(),"actions_revealed":self.reveal.reveals_child(),"docked":p.controls_docked,"dock_height":self.dock.height(),"scheme":p.scheme,"term_font":p.term_font,"term_size_pt":p.term_size_pt,"content_height":content.as_ref().height(),"maximized":window.is_maximized(),"fullscreen":window.is_fullscreen(),"window_height":window.height(),"window_width":window.width(),"terminal_background":schemes::current().colors[8],"open":self.pop.is_visible()})
    }
}
pub fn controls_shortcut(key: gdk::Key, mods: gdk::ModifierType) -> bool {
    let m = mods - gdk::ModifierType::LOCK_MASK;
    (key == gdk::Key::F10 && m.is_empty())
        || (key == gdk::Key::period && m == gdk::ModifierType::CONTROL_MASK)
}

// A header title otherwise receives only TabBar's small natural width, or a
// symmetric allocation wasting the side opposite the actions. This public GTK
// wrapper requests the available width while retaining a small minimum, so
// resizing the window remains possible. No private Adwaita widgets are changed.
mod tab_title {
    use super::*;
    use gtk::subclass::prelude::*;
    #[derive(Default)]
    pub struct Title {
        pub child: RefCell<Option<adw::TabBar>>,
    }
    #[glib::object_subclass]
    impl ObjectSubclass for Title {
        const NAME: &'static str = "RuwacTabTitle";
        type Type = super::TabTitle;
        type ParentType = gtk::Widget;
    }
    impl ObjectImpl for Title {
        fn dispose(&self) {
            if let Some(c) = self.child.borrow_mut().take() {
                c.unparent();
            }
        }
    }
    impl WidgetImpl for Title {
        fn measure(&self, o: gtk::Orientation, for_size: i32) -> (i32, i32, i32, i32) {
            if o == gtk::Orientation::Horizontal {
                (60, 16384, -1, -1)
            } else {
                self.child
                    .borrow()
                    .as_ref()
                    .map(|c| c.measure(o, for_size))
                    .unwrap_or((0, 0, -1, -1))
            }
        }
        fn size_allocate(&self, w: i32, h: i32, b: i32) {
            if let Some(c) = self.child.borrow().as_ref() {
                c.allocate(w, h, b, None);
            }
        }
        fn snapshot(&self, s: &gtk::Snapshot) {
            if let Some(c) = self.child.borrow().as_ref() {
                self.obj().snapshot_child(c, s);
            }
        }
    }
}
glib::wrapper! {pub struct TabTitle(ObjectSubclass<tab_title::Title>) @extends gtk::Widget, @implements gtk::Accessible,gtk::Buildable,gtk::ConstraintTarget;}
impl TabTitle {
    fn new(bar: &adw::TabBar) -> Self {
        use gtk::subclass::prelude::ObjectSubclassIsExt;
        let s: Self = glib::Object::new();
        bar.set_parent(&s);
        *s.imp().child.borrow_mut() = Some(bar.clone());
        s
    }
}
