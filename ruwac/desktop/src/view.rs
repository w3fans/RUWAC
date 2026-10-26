use crate::terminal_history::History;
use crate::{
    clipboard::{self, Broker},
    worker::{Command, Worker},
};
use adw::prelude::*;
use gtk::{gdk, glib};
use serde_json::{Value, json};
use std::{
    cell::{Cell, RefCell},
    path::PathBuf,
    rc::Rc,
};
thread_local! {
    static HISTORY_STYLE: RefCell<Option<gtk::CssProvider>> = const { RefCell::new(None) };
}
fn selection_text(grid: &Value, mut a: (usize, usize), mut z: (usize, usize)) -> String {
    if a > z {
        std::mem::swap(&mut a, &mut z);
    }
    let Some(rows) = grid["cells"].as_array() else {
        return String::new();
    };
    let mut result = String::new();
    for r in a.0..=z.0.min(rows.len().saturating_sub(1)) {
        let Some(cells) = rows.get(r).and_then(Value::as_array) else {
            break;
        };
        let start = if r == a.0 { a.1 } else { 0 };
        let end = if r == z.0 {
            z.1.saturating_add(1)
        } else {
            cells.len()
        };
        let mut line = String::new();
        for cell in cells.iter().take(end).skip(start) {
            if cell[1] != true {
                line.push_str(cell[0].as_str().unwrap_or(""));
            }
        }
        let wrapped = grid["wrapped"][r].as_bool().unwrap_or(false);
        if r != z.0 && !wrapped {
            // Grid padding after a shell continuation must not become text.
            result.push_str(line.trim_end_matches(' '));
            result.push('\n');
        } else {
            result.push_str(&line);
        }
    }
    result
}
fn selection_changed(old: &Value, new: &Value, a: (usize, usize), z: (usize, usize)) -> bool {
    old["rows"] != new["rows"]
        || old["cols"] != new["cols"]
        || old["alternate_screen"] != new["alternate_screen"]
        || selection_text(old, a, z) != selection_text(new, a, z)
}
#[cfg(test)]
mod selection_tests {
    use super::*;
    fn grid(lines: &[&str], wrapped: &[bool]) -> Value {
        json!({"cells":lines.iter().map(|s|s.chars().map(|c|json!([c.to_string(),false])).collect::<Vec<_>>()).collect::<Vec<_>>(),"wrapped":wrapped})
    }
    #[test]
    fn inline_history_preserves_live_geometry_and_unicode_without_mutating_daemon_grid() {
        let live =
            json!({"rows":2,"cols":6,"cells":[[["live"]]],"cursor":{"visible":true},"mouse":false});
        let mut history = History::default();
        history.set("old\n界e\u{301}\nnew");
        assert_eq!(history.grid(&live, 0).0, live);
        let g = history.grid(&live, 1).0;
        assert_eq!(g["rows"], 2);
        assert_eq!(g["cols"], 6);
        assert_eq!(g["cursor"]["visible"], false);
        assert_eq!(g["cells"][1][0][0], "界");
        assert_eq!(g["cells"][1][1][1], true);
        assert_eq!(g["cells"][1][2][0], "e\u{301}");
        assert_eq!(live["cursor"]["visible"], true);
    }
    #[test]
    fn selected_wrapped_install_command_keeps_one_logical_line_in_both_directions() {
        let g = grid(
            &["curl -fsSL https:", "//example.test/i", "nstall | sh"],
            &[true, true, false],
        );
        assert_eq!(
            selection_text(&g, (0, 0), (2, 10)),
            "curl -fsSL https://example.test/install | sh"
        );
        assert_eq!(
            selection_text(&g, (2, 10), (0, 0)),
            "curl -fsSL https://example.test/install | sh"
        );
    }
    #[test]
    fn selected_explicit_continuation_drops_grid_padding_but_keeps_real_newline() {
        let g = grid(
            &["sudo dnf install \\    ", "  gh python3"],
            &[false, false],
        );
        assert_eq!(
            selection_text(&g, (0, 0), (1, 11)),
            "sudo dnf install \\\n  gh python3"
        );
    }
    #[test]
    fn legacy_grids_keep_hard_lines_and_partial_selection_boundaries() {
        let mut g = grid(&["echo first   ", "echo second"], &[false, false]);
        g.as_object_mut().unwrap().remove("wrapped");
        assert_eq!(selection_text(&g, (0, 5), (1, 3)), "first\necho");
        assert_eq!(selection_text(&g, (10, 0), (11, 0)), "");
    }
    #[test]
    fn selection_is_invalidated_when_its_text_or_screen_changes() {
        let old = grid(&["selected text", "prompt"], &[false, false]);
        let mut changed = old.clone();
        changed["cells"][0][0][0] = json!("X");
        assert!(selection_changed(&old, &changed, (0, 0), (0, 7)));
        let mut changed = old.clone();
        changed["alternate_screen"] = json!(true);
        assert!(selection_changed(&old, &changed, (0, 0), (0, 7)));
        let mut changed = old.clone();
        changed["cols"] = json!(120);
        assert!(selection_changed(&old, &changed, (0, 0), (0, 7)));
    }
    #[test]
    fn cursor_and_unrelated_output_do_not_displace_selected_text() {
        let old = grid(&["selected text", "prompt"], &[false, false]);
        let mut changed = old.clone();
        changed["cells"][1][0][0] = json!("X");
        changed["cursor"] = json!({"row":1,"col":2});
        assert!(!selection_changed(&old, &changed, (0, 0), (0, 7)));
    }
}
/// One display-wide stylesheet for every history view, replaced on change.
fn history_style(css: &str) {
    HISTORY_STYLE.with(|style| {
        let mut style = style.borrow_mut();
        let provider = style.get_or_insert_with(|| {
            let provider = gtk::CssProvider::new();
            if let Some(display) = gdk::Display::default() {
                gtk::style_context_add_provider_for_display(
                    &display,
                    &provider,
                    gtk::STYLE_PROVIDER_PRIORITY_APPLICATION + 1,
                );
            }
            provider
        });
        provider.load_from_string(css);
    });
}
pub struct View {
    pub session: String,
    pub root: gtk::Box,
    pub area: gtk::DrawingArea,
    pub status: gtk::Label,
    pub history_controls: gtk::Box,
    cell_width: Cell<f64>,
    cell_height: Cell<f64>,
    font: RefCell<String>,
    font_size: Cell<u8>,
    line_height: Cell<f32>,
    pub worker: RefCell<Worker>,
    socket: PathBuf,
    dead: gtk::Box,
    dead_title: gtk::Label,
    dead_reason: gtk::Label,
    /// The card offers to reopen an ended remote session rather than reattach.
    ended: Cell<bool>,
    pub frame: RefCell<Option<Value>>,
    pub target: RefCell<String>,
    pub drawn: Cell<u64>,
    pub prefix: Cell<bool>,
    pub detached: Cell<bool>,
    pub finished: Cell<bool>,
    seen_running: Cell<bool>,
    retiring: Cell<bool>,
    /// Set once the person types or pastes here.
    typed: Cell<bool>,
    pub broker: Broker,
    image_paste: RefCell<Option<std::sync::Arc<std::sync::atomic::AtomicBool>>>,
    window_index: Cell<Option<usize>>,
    pub warning: RefCell<Option<String>>,
    selection: RefCell<Option<((usize, usize), (usize, usize))>>,
    primary_provider: RefCell<Option<gdk::ContentProvider>>,
    link_click: Cell<bool>,
    /// Last handled OSC 52 PRIMARY (offer, request) serials of the target pane.
    selection_serials: Cell<(u64, u64)>,
    /// The user's last forwarded left release (0) or middle press (1).
    mouse_gesture: Cell<Option<(u32, std::time::Instant)>>,
    selection_middle: Cell<bool>,
    primary_handler: RefCell<Option<glib::SignalHandlerId>>,
    drag_start: Cell<(f64, f64)>,
    pressed: Cell<Option<u32>>,
    mouse_position: Cell<(f64, f64)>,
    pub copy: gtk::TextView,
    copy_anchor: Cell<Option<i32>>,
    pub search: gtk::SearchEntry,
    early: RefCell<Vec<String>>,
    navigation: Cell<bool>,
    focus_epoch: Cell<u64>,
    expected_window: RefCell<Option<String>>,
    divider: RefCell<Option<(String, bool, f64, f64, f64)>>,
    remaps: std::collections::HashMap<String, String>,
    shortcuts: std::collections::HashMap<String, String>,
    wheel_delta: Cell<(f64, f64)>,
    stack: gtk::Stack,
    desktop_shortcuts: Cell<bool>,
    copy_on_select: Cell<bool>,
    history_pending: Cell<bool>,
    history_request: Cell<u64>,
    history_inline: Cell<bool>,
    history_offset: Cell<usize>,
    history_lines: RefCell<History>,
    history_pane: RefCell<String>,
}
impl View {
    pub fn new(socket: PathBuf, session: String, broker: Broker) -> Rc<Self> {
        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let status = gtk::Label::new(Some("Connecting to daemon…"));
        status.set_xalign(0.0);
        status.set_ellipsize(gtk::pango::EllipsizeMode::End);

        let area = gtk::DrawingArea::builder()
            .hexpand(true)
            .vexpand(true)
            .focusable(true)
            .content_width(200)
            .content_height(80)
            .build();
        let stack = gtk::Stack::builder().hexpand(true).vexpand(true).build();
        stack.set_transition_type(gtk::StackTransitionType::None);
        stack.add_named(&area, Some("terminal"));
        let copy = gtk::TextView::builder()
            .editable(false)
            .monospace(true)
            .wrap_mode(gtk::WrapMode::None)
            .build();
        copy.add_css_class("ruwac-history");
        // Internal copy selection must not claim compositor PRIMARY implicitly.
        copy.connect_realize(|view| {
            view.buffer()
                .remove_selection_clipboard(&view.display().primary_clipboard());
        });
        let scroll = gtk::ScrolledWindow::builder()
            .hexpand(true)
            .vexpand(true)
            .child(&copy)
            .build();
        let search = gtk::SearchEntry::new();
        search.set_placeholder_text(Some("Search canonical history (bounded 8 MiB copy view)"));
        let cb = copy.buffer();
        search.connect_changed(move |entry| {
            let needle = entry.text();
            if needle.is_empty() {
                return;
            }
            let all = cb.text(&cb.start_iter(), &cb.end_iter(), true);
            if let Some(byte) = all.find(needle.as_str()) {
                let start = all[..byte].chars().count() as i32;
                cb.select_range(
                    &cb.iter_at_offset(start),
                    &cb.iter_at_offset(start + needle.chars().count() as i32),
                );
            }
        });
        let copybox = gtk::Box::new(gtk::Orientation::Vertical, 4);
        let choices = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        let active_history = gtk::Button::with_label("Active screen history");
        active_history.set_widget_name("history-active-buffer");
        let primary_history = gtk::Button::with_label("Primary screen history");
        primary_history.set_widget_name("history-primary-buffer");
        choices.append(&active_history);
        choices.append(&primary_history);
        copybox.append(&choices);
        copybox.append(&search);
        copybox.append(&scroll);
        stack.add_named(&copybox, Some("copy"));
        // Shown only when the daemon connection is lost while the session may
        // still exist; ended or removed sessions close their tab instead.
        let dead = gtk::Box::new(gtk::Orientation::Vertical, 12);
        dead.set_widget_name("terminal-disconnected");
        dead.add_css_class("card");
        dead.add_css_class("ruwac-disconnected");
        dead.set_halign(gtk::Align::Center);
        dead.set_valign(gtk::Align::Center);
        let dead_title = gtk::Label::new(Some("Terminal disconnected"));
        dead_title.add_css_class("title-3");
        let dead_reason = gtk::Label::new(None);
        dead_reason.set_wrap(true);
        dead_reason.set_max_width_chars(48);
        dead_reason.set_justify(gtk::Justification::Center);
        dead_reason.add_css_class("dim-label");
        let reconnect = gtk::Button::with_label("Reconnect");
        reconnect.set_widget_name("terminal-reconnect");
        reconnect.add_css_class("suggested-action");
        reconnect.add_css_class("pill");
        reconnect.set_halign(gtk::Align::Center);
        let close_tab = gtk::Button::with_label("Close tab");
        close_tab.set_widget_name("terminal-close-ended");
        close_tab.add_css_class("pill");
        let dead_actions = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        dead_actions.set_halign(gtk::Align::Center);
        dead_actions.append(&reconnect);
        dead_actions.append(&close_tab);
        dead.append(&dead_title);
        dead.append(&dead_reason);
        dead.append(&dead_actions);
        dead.set_visible(false);
        let overlay = gtk::Overlay::new();
        overlay.set_child(Some(&stack));
        overlay.add_overlay(&dead);
        root.append(&overlay);
        let worker = RefCell::new(Worker::start(socket.clone(), session.clone()));
        let v = Rc::new(Self {
            desktop_shortcuts: Cell::new(true),
            copy_on_select: Cell::new(false),
            history_pending: Cell::new(false),
            history_request: Cell::new(0),
            history_inline: Cell::new(false),
            history_offset: Cell::new(0),
            history_lines: RefCell::new(History::default()),
            history_pane: RefCell::new(String::new()),
            session,
            root,
            area,
            status,
            history_controls: choices.clone(),
            cell_width: Cell::new(10.0),
            cell_height: Cell::new(20.0),
            font: RefCell::new("Monospace".into()),
            font_size: Cell::new(12),
            line_height: Cell::new(1.0),
            worker,
            socket,
            dead,
            dead_title,
            dead_reason,
            ended: Cell::new(false),
            frame: RefCell::new(None),
            target: RefCell::new(String::new()),
            drawn: Cell::new(0),
            prefix: Cell::new(false),
            detached: Cell::new(false),
            finished: Cell::new(false),
            seen_running: Cell::new(false),
            retiring: Cell::new(false),
            typed: Cell::new(false),
            broker,
            image_paste: RefCell::new(None),
            window_index: Cell::new(None),
            warning: RefCell::new(None),
            selection: RefCell::new(None),
            primary_provider: RefCell::new(None),
            link_click: Cell::new(false),
            selection_serials: Cell::new((0, 0)),
            mouse_gesture: Cell::new(None),
            selection_middle: Cell::new(false),
            primary_handler: RefCell::new(None),
            drag_start: Cell::new((0.0, 0.0)),
            pressed: Cell::new(None),
            mouse_position: Cell::new((0.0, 0.0)),
            copy,
            search,
            copy_anchor: Cell::new(None),
            early: RefCell::new(Vec::new()),
            navigation: Cell::new(false),
            focus_epoch: Cell::new(0),
            expected_window: RefCell::new(None),
            divider: RefCell::new(None),
            shortcuts: std::env::var("RUWAC_SHORTCUTS")
                .ok()
                .and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or_default(),
            wheel_delta: Cell::new((0.0, 0.0)),
            remaps: std::env::var("RUWAC_KEYMAP")
                .ok()
                .and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or_default(),
            stack,
        });
        let weak = Rc::downgrade(&v);
        let primary = v.area.display().primary_clipboard();
        let handler = primary.connect_changed(move |clipboard| {
            let Some(v) = weak.upgrade() else {
                return;
            };
            let lost = v
                .primary_provider
                .borrow()
                .as_ref()
                .is_some_and(|ours| clipboard.content().as_ref() != Some(ours));
            if lost {
                v.primary_provider.borrow_mut().take();
                v.selection.borrow_mut().take();
                let buffer = v.copy.buffer();
                buffer.place_cursor(&buffer.end_iter());
                v.area.queue_draw();
            }
        });
        *v.primary_handler.borrow_mut() = Some(handler);
        // Keyboard copy-mode marks stay internal. A deliberate mouse selection
        // in the history widget must support ordinary desktop middle-click paste.
        let release = gtk::EventControllerLegacy::new();
        release.set_propagation_phase(gtk::PropagationPhase::Capture);
        let weak = Rc::downgrade(&v);
        release.connect_event(move |_, event| {
            if event.event_type() == gdk::EventType::ButtonRelease
                && event
                    .downcast_ref::<gdk::ButtonEvent>()
                    .is_some_and(|e| e.button() == 1)
            {
                let weak = weak.clone();
                glib::idle_add_local_once(move || {
                    if let Some(v) = weak.upgrade() {
                        if v.copy.is_mapped()
                            && v.stack.visible_child_name().as_deref() == Some("copy")
                            && v.copy.buffer().has_selection()
                        {
                            v.copy_clipboard(true);
                        }
                    }
                });
            }
            glib::Propagation::Proceed
        });
        v.copy.add_controller(release);
        v.apply_appearance(&crate::sidebar_shell::Shell::prefs());
        let weak = Rc::downgrade(&v);
        reconnect.connect_clicked(move |_| {
            if let Some(v) = weak.upgrade() {
                if v.ended.get() {
                    if !crate::launches::reopen(&v.session, &v.target.borrow()) {
                        v.status
                            .set_text("This session cannot be reopened; use New term");
                    }
                } else {
                    v.reconnect();
                }
            }
        });
        let weak = Rc::downgrade(&v);
        close_tab.connect_clicked(move |_| {
            if let Some(v) = weak.upgrade() {
                v.finished.set(true);
            }
        });
        let weak = Rc::downgrade(&v);
        active_history.connect_clicked(move |_| {
            if let Some(v) = weak.upgrade() {
                v.open_history();
            }
        });
        let weak = Rc::downgrade(&v);
        primary_history.connect_clicked(move |_| {
            if let Some(v) = weak.upgrade() {
                v.open_primary_history();
            }
        });
        let w = Rc::downgrade(&v);
        v.area.set_draw_func(move |area, cr, width, height| {
            if let Some(v) = w.upgrade() {
                v.draw(area, cr, width, height)
            }
        });
        let key = gtk::EventControllerKey::new();
        let w = Rc::downgrade(&v);
        key.connect_key_pressed(move |_, key, _, mods| {
            if let Some(v) = w.upgrade() {
                v.key(key, mods);
            }
            glib::Propagation::Stop
        });
        v.area.add_controller(key);
        let key = gtk::EventControllerKey::new();
        key.set_propagation_phase(gtk::PropagationPhase::Capture);
        let w = Rc::downgrade(&v);
        key.connect_key_pressed(move |_, key, _, mods| {
            let Some(v) = w.upgrade() else {
                return glib::Propagation::Proceed;
            };
            if v.copy_key(key, mods) {
                glib::Propagation::Stop
            } else {
                glib::Propagation::Proceed
            }
        });
        v.copy.add_controller(key);
        let click = gtk::GestureClick::new();
        click.set_button(0);
        let w = Rc::downgrade(&v);
        click.connect_pressed(move |g, _, x, y| {
            if let Some(v) = w.upgrade() {
                v.click(x, y, g.current_button(), g.current_event_state(), false)
            }
        });
        let w = Rc::downgrade(&v);
        click.connect_released(move |g, _, x, y| {
            if let Some(v) = w.upgrade() {
                v.click(x, y, g.current_button(), g.current_event_state(), true)
            }
        });
        v.area.add_controller(click);
        let drag = gtk::GestureDrag::new();
        let w = Rc::downgrade(&v);
        drag.connect_drag_begin(move |g, x, y| {
            if let Some(v) = w.upgrade() {
                if g.current_event_state()
                    .contains(gdk::ModifierType::CONTROL_MASK)
                    && v.url_at(x, y).is_some()
                {
                    return;
                }
                v.drag_start.set((x, y));
                if let Some(divider) = v.find_divider(x, y) {
                    *v.divider.borrow_mut() = Some(divider);
                    return;
                }
                if v.mouse_enabled()
                    && !g
                        .current_event_state()
                        .contains(gdk::ModifierType::SHIFT_MASK)
                {
                    return;
                }
                v.focus_at(x, y);
                if let Some((_, r, c)) = v.point(x, y) {
                    *v.selection.borrow_mut() = Some(((r, c), (r, c)));
                    v.area.queue_draw();
                }
            }
        });
        let w = Rc::downgrade(&v);
        drag.connect_drag_update(move |g, dx, dy| {
            if let Some(v) = w.upgrade() {
                let (x, y) = v.drag_start.get();
                if v.divider.borrow().is_some() {
                    return;
                }
                if v.mouse_enabled()
                    && !g
                        .current_event_state()
                        .contains(gdk::ModifierType::SHIFT_MASK)
                {
                    // Motion reports are emitted once by the motion controller.
                    return;
                }
                if let Some((pane, r, c)) = v.point(x + dx, y + dy) {
                    if pane == *v.target.borrow() {
                        if let Some(selection) = v.selection.borrow_mut().as_mut() {
                            selection.1 = (r, c);
                        }
                        v.area.queue_draw();
                    }
                }
            }
        });
        let w = Rc::downgrade(&v);
        drag.connect_drag_end(move |g, dx, dy| {
            if let Some(v) = w.upgrade() {
                if let Some((id, vertical, start, size, initial)) = v.divider.borrow_mut().take() {
                    let movement = if vertical { dx } else { dy };
                    let ratio =
                        ((initial + movement - start) / size * 1000.0).clamp(100.0, 900.0) as u16;
                    if let Some(f) = v.frame.borrow().as_ref() {
                        v.command(
                            "pane.layout",
                            json!({"window":f["window"],"split":id,"ratio":ratio}),
                        );
                    }
                    return;
                }
                if !v.mouse_enabled()
                    || g.current_event_state()
                        .contains(gdk::ModifierType::SHIFT_MASK)
                {
                    v.copy_clipboard(true);
                    if v.copy_on_select.get() {
                        v.copy_clipboard(false);
                    }
                }
            }
        });
        v.area.add_controller(drag);
        let motion = gtk::EventControllerMotion::new();
        let w = Rc::downgrade(&v);
        motion.connect_enter(move |_, x, y| {
            if let Some(v) = w.upgrade() {
                v.mouse_position.set((x, y));
            }
        });
        let w = Rc::downgrade(&v);
        motion.connect_motion(move |g, x, y| {
            if let Some(v) = w.upgrade() {
                v.mouse_position.set((x, y));
                let mods = g.current_event_state();
                if mods.contains(gdk::ModifierType::SHIFT_MASK) {
                    return;
                }
                if let Some(grid) = v.grid() {
                    if grid["mouse_motion"] == true
                        || (grid["mouse_drag"] == true && v.pressed.get().is_some())
                    {
                        v.mouse_with_mods(32 + v.pressed.get().unwrap_or(3), x, y, false, mods);
                    }
                }
            }
        });
        v.area.add_controller(motion);
        let focus = gtk::EventControllerFocus::new();
        let w = Rc::downgrade(&v);
        focus.connect_leave(move |_| {
            if let Some(v) = w.upgrade() {
                v.cancel_image_paste();
                v.release_mouse();
                if v.prefix.replace(false) {
                    v.input(&v.literal_prefix());
                }
            }
        });
        v.area.add_controller(focus);
        let wheel = gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::BOTH_AXES);
        let w = Rc::downgrade(&v);
        wheel.connect_scroll(move |g, dx, dy| {
            if let Some(v) = w.upgrade() {
                if v.mouse_enabled()
                    && !g
                        .current_event_state()
                        .contains(gdk::ModifierType::SHIFT_MASK)
                {
                    // Wayland scroll events need not contain a position. The
                    // motion controller supplies widget-local coordinates;
                    // event surface coordinates would also misroute split views.
                    {
                        let (x, y) = v.mouse_position.get();
                        let (ax, ay) = v.wheel_delta.get();
                        let (mut ax, mut ay) = (ax + dx.clamp(-8.0, 8.0), ay + dy.clamp(-8.0, 8.0));
                        while ay.abs() >= 1.0 {
                            v.mouse_with_mods(
                                if ay < 0.0 { 64 } else { 65 },
                                x,
                                y,
                                false,
                                g.current_event_state(),
                            );
                            ay -= ay.signum();
                        }
                        while ax.abs() >= 1.0 {
                            v.mouse_with_mods(
                                if ax < 0.0 { 66 } else { 67 },
                                x,
                                y,
                                false,
                                g.current_event_state(),
                            );
                            ax -= ax.signum();
                        }
                        v.wheel_delta.set((ax, ay));
                    }
                } else if v.grid().is_some_and(|g| g["alternate_screen"] == true)
                    && !g
                        .current_event_state()
                        .contains(gdk::ModifierType::SHIFT_MASK)
                {
                    let (_, old) = v.wheel_delta.get();
                    let mut delta = old + dy.clamp(-8.0, 8.0);
                    while delta.abs() >= 1.0 {
                        v.input(if delta < 0.0 { "\x1b[A" } else { "\x1b[B" });
                        delta -= delta.signum();
                    }
                    v.wheel_delta.set((0.0, delta));
                } else {
                    v.scroll_history(dy);
                }
            }
            glib::Propagation::Stop
        });
        v.area.add_controller(wheel);
        let w = Rc::downgrade(&v);
        v.area.connect_resize(move|_,width,height|if let Some(v)=w.upgrade(){if width>0&&height>0{v.command("client.resize",json!({"rows":((height as f64/v.cell_height.get())as i32).clamp(4,u16::MAX as i32),"cols":((width as f64/v.cell_width.get())as i32).clamp(10,u16::MAX as i32)}));}});
        v
    }
    fn scroll_history(&self, dy: f64) {
        let (_, previous) = self.wheel_delta.get();
        let delta = previous + dy.clamp(-8.0, 8.0) * 3.0;
        let lines = delta.trunc() as isize;
        self.wheel_delta.set((0.0, delta - delta.trunc()));
        if lines == 0 {
            return;
        }
        let offset = self.history_offset.get().saturating_add_signed(-lines);
        if offset == self.history_offset.get() {
            return;
        }
        self.history_offset.set(offset);
        *self.selection.borrow_mut() = None;
        if offset > 0 && self.history_lines.borrow().is_empty() && !self.history_pending.get() {
            self.history_inline.set(true);
            self.request_history(false);
        }
        if offset == 0 {
            self.cancel_inline_history();
        }
        let _ = self.grid();
        self.area.queue_draw();
    }
    fn request_history(&self, primary: bool) {
        if self.history_pending.replace(true) {
            return;
        }
        let Some(generation) = self.history_request.get().checked_add(1) else {
            self.history_pending.set(false);
            return;
        };
        self.history_request.set(generation);
        *self.history_pane.borrow_mut() = self.target.borrow().clone();
        if let Err(e) = self.worker.borrow().send(Command::History(
            self.target.borrow().clone(),
            primary,
            generation,
        )) {
            self.history_pending.set(false);
            self.warn(&e);
        }
    }
    fn cancel_inline_history(&self) {
        if self.history_inline.replace(false) {
            self.history_request
                .set(self.history_request.get().saturating_add(1));
            self.history_pending.set(false);
        }
        self.history_lines.borrow_mut().clear();
    }
    pub fn show_terminal(&self) {
        self.stack.set_visible_child_name("terminal");
        self.area.grab_focus();
    }
    pub fn open_history(&self) {
        self.open_history_buffer(false);
    }
    pub fn fixture_history_viewport(&self) -> Value {
        json!({"offset":self.history_offset.get(),"pending":self.history_pending.get(),"lines":self.history_lines.borrow().len(),"terminal_canvas":self.stack.visible_child_name().as_deref()==Some("terminal"),"focus":self.area.has_focus(),"cells":self.grid().map(|g|g["cells"].clone())})
    }
    pub fn fixture_history_buffer(&self, primary: bool) -> bool {
        fn find(w: &gtk::Widget, name: &str) -> Option<gtk::Button> {
            if w.widget_name() == name {
                return w.clone().downcast::<gtk::Button>().ok();
            }
            let mut c = w.first_child();
            while let Some(w) = c {
                c = w.next_sibling();
                if let Some(b) = find(&w, name) {
                    return Some(b);
                }
            }
            None
        }
        let root = self.history_controls.clone().upcast::<gtk::Widget>();
        let name = if primary {
            "history-primary-buffer"
        } else {
            "history-active-buffer"
        };
        if let Some(button) = find(&root, name) {
            button.emit_clicked();
            true
        } else {
            false
        }
    }
    pub fn open_primary_history(&self) {
        self.open_history_buffer(true);
    }
    fn open_history_buffer(&self, primary: bool) {
        if self.history_pending.get() {
            return;
        }
        self.history_inline.set(false);
        self.request_history(primary);
        self.status.set_text(if primary {
            "Loading canonical primary-screen history…"
        } else {
            "Loading bounded canonical scrollback…"
        });
    }
    fn navigate(&self, method: &str, mut params: Value) {
        self.focus_epoch.set(self.focus_epoch.get().wrapping_add(1));
        if method == "client.focusLaunch" {
            params["focus_epoch"] = json!(self.focus_epoch.get());
        }
        self.release_mouse();
        self.navigation.set(true);
        *self.selection.borrow_mut() = None;
        self.command(method, params);
    }
    fn split(&self, vertical: bool) {
        self.focus_epoch.set(self.focus_epoch.get().wrapping_add(1));
        self.release_mouse();
        if let Some(f) = self.frame.borrow().as_ref() {
            self.navigation.set(true);
            self.command("pane.create",json!({"window":f["window"],"pane":*self.target.borrow(),"orientation":if vertical{"vertical"}else{"horizontal"}}));
        }
    }
    fn zoom(&self) {
        if let Some(f) = self.frame.borrow().as_ref() {
            self.command(
                "pane.layout",
                json!({"window":f["window"],"zoom":f["zoom"]!=true}),
            );
        }
    }
    fn literal_prefix(&self) -> String {
        let binding = self
            .shortcuts
            .get("prefix")
            .map(String::as_str)
            .unwrap_or("<Control><Alt>a");
        gtk::accelerator_parse(binding)
            .and_then(|(key, flags)| {
                key.to_unicode().filter(|c| {
                    flags.contains(gdk::ModifierType::CONTROL_MASK) && c.is_ascii_alphabetic()
                })
            })
            .map(|c| ((c.to_ascii_lowercase() as u8 & 31) as char).to_string())
            .unwrap_or_else(|| "\x01".into())
    }
    fn binding(&self, key: gdk::Key, mods: gdk::ModifierType, name: &str, default: &str) -> bool {
        let text = self
            .shortcuts
            .get(name)
            .map(String::as_str)
            .unwrap_or(default);
        let Some((wanted, flags)) = gtk::accelerator_parse(text) else {
            return false;
        };
        let mask = gdk::ModifierType::CONTROL_MASK
            | gdk::ModifierType::SHIFT_MASK
            | gdk::ModifierType::ALT_MASK
            | gdk::ModifierType::SUPER_MASK;
        (key == wanted
            || key
                .to_unicode()
                .zip(wanted.to_unicode())
                .is_some_and(|(a, b)| a.to_lowercase().to_string() == b.to_lowercase().to_string()))
            && (mods & mask) == (flags & mask)
    }
    fn show_help(&self) {
        let text = format!(
            "Prefix: {}\nDesktop copy: {}\nDesktop paste: {}\n\nPrefix commands: double prefix forwards to nested terminal · o previous window · c new · n/p windows · 0–9 select · \" chooser · s/S horizontal / | vertical split · Tab pane · z zoom/restore · [ copy/search · ] internal paste · : command · d detach · a literal prefix.\n\nCopy mode: arrows/hjkl, Page Up/Down, g/G, Space mark, Enter save internally, / search, Escape exit. Drag dividers to resize. Shift+drag overrides application mouse.\n\nCustom prefix command mappings: {}",
            self.shortcuts
                .get("prefix")
                .map(String::as_str)
                .unwrap_or("Ctrl+Alt+A"),
            self.shortcuts
                .get("copy")
                .map(String::as_str)
                .unwrap_or("Ctrl+Shift+C"),
            self.shortcuts
                .get("paste")
                .map(String::as_str)
                .unwrap_or("Ctrl+Shift+V"),
            self.remaps
                .iter()
                .map(|(k, v)| format!("{k} → {v}"))
                .collect::<Vec<_>>()
                .join(", ")
        );
        let label = gtk::Label::new(Some(&text));
        label.set_wrap(true);
        label.set_selectable(true);
        label.set_margin_start(20);
        label.set_margin_end(20);
        label.set_margin_top(20);
        label.set_margin_bottom(20);
        let window = gtk::Window::builder()
            .title("RUWAC — current shortcuts")
            .default_width(600)
            .default_height(360)
            .child(&label)
            .build();
        if let Some(parent) = self.parent() {
            window.set_transient_for(Some(&parent));
        }
        window.present();
    }
    fn parent(&self) -> Option<gtk::Window> {
        self.root
            .root()
            .and_then(|r| r.downcast::<gtk::Window>().ok())
    }
    fn show_chooser(self: &Rc<Self>, windows: &Value) {
        let dialog = gtk::Dialog::builder()
            .title("Windows — choose a persistent window")
            .modal(true)
            .default_width(480)
            .build();
        if let Some(parent) = self.parent() {
            dialog.set_transient_for(Some(&parent));
        }
        dialog.add_button("Cancel", gtk::ResponseType::Cancel);
        let list = gtk::ListBox::new();
        list.set_selection_mode(gtk::SelectionMode::Single);
        let mut ids = Vec::new();
        if let Some(windows) = windows.as_array() {
            for (i, w) in windows.iter().enumerate() {
                ids.push(w["id"].as_str().unwrap_or("").to_string());
                let label = gtk::Label::new(Some(&format!(
                    "{i}    {}",
                    w["name"].as_str().unwrap_or("Shell")
                )));
                label.set_xalign(0.0);
                label.set_margin_top(8);
                label.set_margin_bottom(8);
                list.append(&label);
            }
        }
        dialog.content_area().append(&list);
        let weak = Rc::downgrade(self);
        let d = dialog.clone();
        list.connect_row_activated(move |_, row| {
            if let Some(v) = weak.upgrade() {
                if let Some(id) = ids.get(row.index() as usize) {
                    v.navigate("window.select", json!({"window":id}));
                }
            }
            d.close();
        });
        dialog.connect_response(|d, _| d.close());
        dialog.present();
        list.grab_focus();
        if let Some(row) = list.row_at_index(0) {
            list.select_row(Some(&row));
            row.grab_focus();
        }
    }
    fn show_prompt(self: &Rc<Self>) {
        self.show_prompt_seed("");
    }
    fn show_prompt_seed(self: &Rc<Self>, seed: &str) {
        let dialog = gtk::Dialog::builder()
            .title("RUWAC command")
            .modal(true)
            .default_width(520)
            .build();
        if let Some(p) = self.parent() {
            dialog.set_transient_for(Some(&p));
        }
        let entry = gtk::Entry::new();
        entry.set_text(seed);
        entry.set_position(-1);
        entry.set_placeholder_text(Some("new · next · prev · select N · split horizontal/vertical · zoom · copy · paste · detach · help"));
        dialog.content_area().append(&entry);
        dialog.add_button("Cancel", gtk::ResponseType::Cancel);
        dialog.add_button("Run", gtk::ResponseType::Accept);
        let weak = Rc::downgrade(self);
        let e = entry.clone();
        dialog.connect_response(move |d, r| {
            if r == gtk::ResponseType::Accept {
                if let Some(v) = weak.upgrade() {
                    v.run_command(e.text().as_str());
                }
            }
            d.close();
        });
        let d = dialog.clone();
        entry.connect_activate(move |_| d.response(gtk::ResponseType::Accept));
        dialog.present();
        entry.grab_focus();
    }
    fn confirm_close_window(self: &Rc<Self>) {
        let Some(frame) = self.frame.borrow().clone() else {
            return;
        };
        let panes: Vec<_> = frame["panes"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|p| p["id"].as_str().map(str::to_owned))
            .collect();
        let dialog = gtk::Dialog::builder()
            .title("Kill this terminal window?")
            .modal(true)
            .build();
        if let Some(parent) = self.parent() {
            dialog.set_transient_for(Some(&parent));
        }
        dialog.content_area().append(&gtk::Label::new(Some(
            "All programs in this terminal window will be closed.",
        )));
        dialog.add_button("Cancel", gtk::ResponseType::Cancel);
        dialog.add_button("Kill window", gtk::ResponseType::Accept);
        dialog.set_default_response(gtk::ResponseType::Cancel);
        let weak = Rc::downgrade(self);
        dialog.connect_response(move |dialog, response| {
            if response == gtk::ResponseType::Accept {
                if let Some(view) = weak.upgrade() {
                    for pane in &panes {
                        view.command("pane.close", json!({"pane":pane}));
                    }
                }
            }
            dialog.close();
        });
        dialog.present();
    }
    pub fn run_command(self: &Rc<Self>, command: &str) {
        let mut words: Vec<_> = command.split_whitespace().collect();
        if let Some(name) = words.first_mut() {
            *name = ruwac_protocol::screen_command(name);
        }
        match words.as_slice(){
            ["new"]=>self.navigate("window.create",json!({"session":self.session})),
            ["next"]|["prev"]=>self.navigate("window.cycle",json!({"session":self.session,"previous":words[0]=="prev"})),
            ["select",n]=>match n.parse::<usize>(){Ok(n)=>{self.window_index.set(Some(n));self.command("window.list",json!({"session":self.session}));},Err(_)=>self.status.set_text("select requires a nonnegative window index")},
            ["split"]|["split","horizontal"]=>self.split(false),["split","vertical"]|["split","-v"]=>self.split(true),
            ["only"]=>{if let Some(f)=self.frame.borrow().as_ref(){self.command("pane.layout",json!({"window":f["window"],"zoom":true}));}},
            ["zoom"]=>self.zoom(),["copy"]=>self.open_history(),["paste"]=>self.command("buffer.get",json!({"session":self.session})),
            ["kill"]=>self.confirm_close_window(),
            ["rename", rest @ ..] if !rest.is_empty()=>{if let Some(f)=self.frame.borrow().as_ref(){self.command("window.rename",json!({"window":f["window"],"name":rest.join(" ")}));}},
            ["other"]=>self.navigate("window.previous",json!({"session":self.session})),
            ["windows"]=>self.command("window.list",json!({"session":self.session})),
            ["focus"]=>{self.prefix.set(true);self.key(gdk::Key::Tab,gdk::ModifierType::empty());},
            ["detach"]=>self.close(),["help"]=>self.show_help(),
            _=>self.warn("Unknown command. Use: new, next, prev, select N, split horizontal/vertical, zoom, copy, paste, detach, help"),
        }
    }
    pub fn copy_key(self: &Rc<Self>, key: gdk::Key, mods: gdk::ModifierType) -> bool {
        if key == gdk::Key::Escape {
            self.show_terminal();
            return true;
        }
        if key == gdk::Key::Return {
            self.internal_copy();
            return true;
        }
        if self.binding(key, mods, "copy", "<Control><Shift>c") {
            self.copy_clipboard(false);
            return true;
        }
        if self.binding(key, mods, "paste", "<Control><Shift>v") {
            self.paste_clipboard(false);
            return true;
        }
        if key == gdk::Key::slash {
            self.search.grab_focus();
            return true;
        }
        let b = self.copy.buffer();
        let mut pos = b.iter_at_mark(&b.get_insert());
        if key == gdk::Key::space {
            if self.copy_anchor.get().is_some() {
                self.internal_copy();
            } else {
                self.copy_anchor.set(Some(pos.offset()));
                self.status
                    .set_text("Copy mark set; navigate then Enter to save internal buffer");
            }
            return true;
        }
        let moved = match key {
            gdk::Key::Left | gdk::Key::h => pos.backward_cursor_position(),
            gdk::Key::Right | gdk::Key::l => pos.forward_cursor_position(),
            gdk::Key::Up | gdk::Key::k => pos.backward_line(),
            gdk::Key::Down | gdk::Key::j => pos.forward_line(),
            gdk::Key::Page_Up => pos.backward_lines(20),
            gdk::Key::Page_Down => pos.forward_lines(20),
            gdk::Key::Home => {
                pos.set_line_offset(0);
                true
            }
            gdk::Key::End => pos.forward_to_line_end(),
            gdk::Key::g => {
                pos = b.start_iter();
                true
            }
            gdk::Key::G => {
                pos = b.end_iter();
                true
            }
            _ => return false,
        };
        if moved {
            if let Some(anchor) = self.copy_anchor.get() {
                b.select_range(&pos, &b.iter_at_offset(anchor));
            } else {
                b.place_cursor(&pos);
            }
            self.copy.scroll_to_iter(&mut pos, 0.1, false, 0.0, 0.0);
        }
        true
    }
    fn find_divider(&self, x: f64, y: f64) -> Option<(String, bool, f64, f64, f64)> {
        fn walk(
            cw: f64,
            ch: f64,
            node: &Value,
            r: (f64, f64, f64, f64),
            x: f64,
            y: f64,
        ) -> Option<(String, bool, f64, f64, f64)> {
            let id = node["id"].as_str()?;
            let vertical = node["orientation"] == "vertical";
            let ratio = node["ratio"].as_f64().unwrap_or(500.0) / 1000.0;
            let (rx, ry, w, h) = r;
            let size = if vertical { w } else { h };
            let start = if vertical { rx } else { ry };
            let initial = start
                + ((size / (if vertical { cw } else { ch }) * ratio).floor())
                    * (if vertical { cw } else { ch });
            if x >= rx
                && x < rx + w
                && y >= ry
                && y < ry + h
                && (if vertical {
                    (x - initial).abs() < 4.0
                } else {
                    (y - initial).abs() < 4.0
                })
            {
                return Some((id.into(), vertical, start, size, initial));
            }
            let (a, b) = if vertical {
                (
                    (rx, ry, initial - rx, h),
                    (initial, ry, rx + w - initial, h),
                )
            } else {
                (
                    (rx, ry, w, initial - ry),
                    (rx, initial, w, ry + h - initial),
                )
            };
            walk(cw, ch, &node["first"], a, x, y).or_else(|| walk(cw, ch, &node["second"], b, x, y))
        }
        let f = self.frame.borrow();
        let f = f.as_ref()?;
        if f["zoom"] == true {
            return None;
        }
        let panes = f["panes"].as_array()?;
        let width = panes
            .iter()
            .map(|p| {
                p["rect"]["x"].as_f64().unwrap_or(0.0) + p["rect"]["cols"].as_f64().unwrap_or(0.0)
            })
            .fold(0.0, f64::max)
            * self.cell_width.get();
        let height = panes
            .iter()
            .map(|p| {
                p["rect"]["y"].as_f64().unwrap_or(0.0) + p["rect"]["rows"].as_f64().unwrap_or(0.0)
            })
            .fold(0.0, f64::max)
            * self.cell_height.get();
        walk(
            self.cell_width.get(),
            self.cell_height.get(),
            &f["layout"],
            (0.0, 0.0, width, height),
            x,
            y,
        )
    }

    // Test-only caller reaches the same compiled GTK gesture callbacks as pointer drags.
    pub fn fixture_root_drag(&self) -> Value {
        let (x, y, dx, dy) = {
            let frame = self.frame.borrow();
            let Some(f) = frame.as_ref() else {
                return json!({"error":"no frame"});
            };
            let Some(panes) = f["panes"].as_array() else {
                return json!({"error":"no panes"});
            };
            let width = panes
                .iter()
                .map(|p| {
                    p["rect"]["x"].as_f64().unwrap_or(0.) + p["rect"]["cols"].as_f64().unwrap_or(0.)
                })
                .fold(0., f64::max)
                * self.cell_width.get();
            let height = panes
                .iter()
                .map(|p| {
                    p["rect"]["y"].as_f64().unwrap_or(0.) + p["rect"]["rows"].as_f64().unwrap_or(0.)
                })
                .fold(0., f64::max)
                * self.cell_height.get();
            let ratio = f["layout"]["ratio"].as_f64().unwrap_or(500.) / 1000.;
            if f["layout"]["orientation"] == "vertical" {
                (
                    (width / self.cell_width.get() * ratio).floor() * self.cell_width.get(),
                    height / 2.,
                    width * 0.1,
                    0.,
                )
            } else {
                (
                    width / 2.,
                    (height / self.cell_height.get() * ratio).floor() * self.cell_height.get(),
                    0.,
                    height * 0.1,
                )
            }
        };
        let found = self.find_divider(x, y);
        let controllers = self.area.observe_controllers();
        for i in 0..controllers.n_items() {
            if let Some(gesture) = controllers
                .item(i)
                .and_then(|o| o.downcast::<gtk::GestureDrag>().ok())
            {
                gesture.emit_by_name::<()>("drag-begin", &[&x, &y]);
                gesture.emit_by_name::<()>("drag-end", &[&dx, &dy]);
            }
        }
        json!({"coordinates":[x,y,dx,dy],"divider":found,"controllers":controllers.n_items()})
    }
    pub fn warn(&self, message: &str) {
        *self.warning.borrow_mut() = Some(message.into());
        self.status.set_text(message);
    }
    fn set_target(&self, target: String) {
        if *self.target.borrow() != target {
            self.history_request
                .set(self.history_request.get().saturating_add(1));
            self.history_pending.set(false);
            self.history_offset.set(0);
            self.cancel_inline_history();
            self.cancel_image_paste();
        }
        *self.target.borrow_mut() = target;
    }
    pub fn focus_launch_pane(&self, window: &str, pane: &str, pid: u32) {
        self.cancel_image_paste();
        self.navigate(
            "client.focusLaunch",
            json!({"session":self.session,"window":window,"pane":pane,"pid":pid}),
        );
        self.focus();
    }
    pub fn image_paste_destination_alive(&self, pane: &str) -> bool {
        let worker = self.worker.borrow();
        let state = worker.state.lock().unwrap();
        !state.disconnected
            && !state.session_gone
            && self
                .frame
                .borrow()
                .as_ref()
                .and_then(|f| f["panes"].as_array())
                .is_some_and(|panes| {
                    panes
                        .iter()
                        .any(|p| p["id"] == pane && p["ended"] != true && p["restored"] != true)
                })
    }
    pub fn image_paste_pending(&self) -> bool {
        self.image_paste
            .borrow()
            .as_ref()
            .is_some_and(|v| v.load(std::sync::atomic::Ordering::Acquire))
    }
    pub fn end_image_paste(&self, ticket: &std::sync::Arc<std::sync::atomic::AtomicBool>) -> bool {
        if self
            .image_paste
            .borrow()
            .as_ref()
            .is_some_and(|v| std::sync::Arc::ptr_eq(v, ticket))
        {
            self.image_paste.borrow_mut().take();
            ticket.store(false, std::sync::atomic::Ordering::Release);
            true
        } else {
            false
        }
    }
    pub fn cancel_image_paste(&self) {
        if let Some(ticket) = self.image_paste.borrow_mut().take() {
            ticket.store(false, std::sync::atomic::Ordering::Release);
            self.status
                .set_text("Image paste cancelled; no new target selected");
        }
    }
    pub fn begin_image_paste(&self) -> std::sync::Arc<std::sync::atomic::AtomicBool> {
        self.cancel_image_paste();
        let ticket = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
        *self.image_paste.borrow_mut() = Some(ticket.clone());
        ticket
    }
    pub fn guarded_image_path(
        &self,
        pane: String,
        text: String,
        allowed: std::sync::Arc<std::sync::atomic::AtomicBool>,
    ) -> Result<std::sync::mpsc::Receiver<Result<(), String>>, String> {
        self.typed.set(true);
        let (done, rx) = std::sync::mpsc::sync_channel(1);
        self.worker.borrow().send(Command::GuardedPaste {
            pane,
            text,
            allowed,
            done,
        })?;
        Ok(rx)
    }
    pub fn command(&self, m: &str, mut p: Value) {
        if matches!(
            m,
            "client.focus"
                | "pane.select"
                | "window.select"
                | "window.cycle"
                | "window.previous"
                | "window.create"
                | "pane.create"
                | "pane.close"
        ) {
            self.cancel_image_paste();
        }
        if matches!(m, "window.create" | "pane.create") && p.get("argv").is_none() {
            match crate::shell::default_terminal_argv() {
                Ok(argv) => p["argv"] = json!(argv),
                Err(e) => {
                    self.navigation.set(false);
                    self.warn(&e);
                    return;
                }
            }
        }
        if let Err(e) = self.worker.borrow().send(Command::Rpc(m.into(), p)) {
            self.warn(&e);
        }
    }
    pub fn input(&self, text: &str) {
        if self.history_offset.replace(0) > 0 {
            self.cancel_inline_history();
            self.area.queue_draw();
        }
        self.typed.set(true);
        if self.target.borrow().is_empty() || self.navigation.get() {
            let mut pending = self.early.borrow_mut();
            if pending.iter().map(String::len).sum::<usize>() + text.len() > 256 * 1024 {
                self.warn("Pending input budget reached; input not queued");
            } else {
                pending.push(text.into());
            }
            return;
        }
        self.command(
            "client.input",
            json!({"pane":*self.target.borrow(),"kind":"keyboard","text":text}),
        );
    }
    pub fn paste(&self, text: String) {
        self.history_offset.set(0);
        self.cancel_inline_history();
        self.area.queue_draw();
        self.cancel_image_paste();
        if text.len() > ruwac_clipboard::MAX_DESKTOP_TEXT
            || text.contains('\0')
            || text.contains("\x1b[201~")
        {
            self.status
                .set_text("Paste rejected: text cap, NUL or end marker");
            return;
        }
        self.typed.set(true);
        if let Err(e) = self
            .worker
            .borrow()
            .send(Command::Paste(self.target.borrow().clone(), text))
        {
            self.warn(&e);
        }
    }
    /// Replace a disconnected worker, starting the daemon when it is absent.
    pub fn reconnect(&self) {
        self.cancel_image_paste();
        if self.detached.get() || self.finished.get() {
            return;
        }
        self.dead.set_visible(false);
        *self.warning.borrow_mut() = None;
        // A restarted daemon numbers render revisions from the beginning.
        *self.frame.borrow_mut() = None;
        // History restored without its process stays readable after an
        // explicit reconnect instead of closing like a fresh exit.
        self.seen_running.set(false);
        self.status.set_text("Reconnecting to daemon…");
        let old = self
            .worker
            .replace(Worker::restart(self.socket.clone(), self.session.clone()));
        old.close();
        if self.area.width() > 0 && self.area.height() > 0 {
            self.command("client.resize",json!({"rows":((self.area.height()as f64/self.cell_height.get())as i32).clamp(4,u16::MAX as i32),"cols":((self.area.width()as f64/self.cell_width.get())as i32).clamp(10,u16::MAX as i32)}));
        }
        self.focus();
    }
    pub fn tab_status(&self) -> crate::tiles::TabStatus {
        use crate::tiles::TabStatus;
        if self.warning.borrow().is_some() {
            TabStatus::Error
        } else if self.ended.get() || self.finished.get() || self.dead.is_visible() {
            TabStatus::Offline
        } else if self.frame.borrow().is_none() {
            TabStatus::Busy
        } else {
            TabStatus::Ready
        }
    }
    pub fn update(self: &Rc<Self>) {
        let worker = self.worker.borrow();
        worker.set_visible(self.area.is_mapped());
        let mut s = worker.state.lock().unwrap();
        if let Some(error) = s.error.take() {
            self.history_pending.set(false);
            self.history_inline.set(false);
            self.warn(&error);
        }
        if s.session_gone {
            // Removed by exit, kill-session or another client: same as exit.
            self.finished.set(true);
            return;
        }
        if s.disconnected && !self.detached.get() && !self.dead.is_visible() {
            self.ended.set(false);
            self.dead_title.set_text("Terminal disconnected");
            self.dead.set_valign(gtk::Align::Center);
            self.dead_reason.set_text(&format!(
                "{}\nThe session may still be running. Reconnect starts the daemon if needed and reattaches; closing this tab does not end the session.",
                self.warning.borrow().as_deref().unwrap_or("The daemon connection ended.")
            ));
            self.dead.set_visible(true);
        }
        if let Some(frame) = s.frame.take() {
            if self
                .frame
                .borrow()
                .as_ref()
                .and_then(|v| v["revision"].as_u64())
                .is_some_and(|r| frame["revision"].as_u64().unwrap_or(0) < r)
            {
                self.status.set_text("Stale frame rejected");
                return;
            }
            let panes = frame["panes"].as_array().unwrap();
            if panes
                .iter()
                .any(|p| p["ended"] != true && p["restored"] != true)
            {
                self.seen_running.set(true);
            }
            if s.session_ended && !self.finished.get() && !self.retiring.get() {
                // A local shell the person exited closes. Remote sessions (a
                // dropped SSH connection looks the same) and programs that
                // ended by themselves stay readable with Reconnect / Close.
                if self.seen_running.get()
                    && self.typed.get()
                    && !crate::launches::is_remote(&self.session, &self.target.borrow())
                {
                    let ids: Vec<_> = panes
                        .iter()
                        .filter(|p| p["ended"] == true && p["restored"] != true)
                        .filter_map(|p| p["id"].as_str())
                        .collect();
                    if ids.is_empty() {
                        self.finished.set(true);
                    } else {
                        match worker.send(Command::Rpc(
                            "client.retireEnded".into(),
                            json!({"panes":ids}),
                        )) {
                            Ok(()) => self.retiring.set(true),
                            Err(error) => self.warn(&error),
                        }
                    }
                } else if !self.dead.is_visible() {
                    self.ended.set(true);
                    self.dead_title.set_text("Session ended");
                    self.dead_reason.set_text(if crate::launches::is_remote(&self.session,&self.target.borrow()) {
                        "The remote program or SSH connection ended. Reconnect opens it again in a new tab."
                    } else {
                        "The program ended. Its output stays readable above this card."
                    });
                    self.dead.set_valign(gtk::Align::End);
                    self.dead.set_margin_bottom(24);
                    self.dead.set_visible(true);
                }
            }
            if !panes
                .iter()
                .any(|p| p["id"].as_str() == Some(self.target.borrow().as_str()))
            {
                self.set_target(frame["active_pane"].as_str().unwrap_or("").into());
            }
            if self.warning.borrow().is_none() {
                let active = panes.iter().find(|p| p["id"] == frame["active_pane"]);
                let state = match active {
                    Some(p) if p["restored"] == true => {
                        "Restored history · process ended · New terminal starts a session"
                    }
                    Some(p) if p["ended"] == true => "Process ended · history remains available",
                    _ => "Ctrl+Alt+A controls · Ctrl+A to application · Shift+drag selects",
                };
                let rows = frame["rows"].as_u64();
                let cols = frame["cols"].as_u64();
                let viewport_rows =
                    (self.area.height() as f64 / self.cell_height.get()).floor() as u64;
                let viewport_cols =
                    (self.area.width() as f64 / self.cell_width.get()).floor() as u64;
                let shared = if let Some(notice) = frame["size_notice"].as_str() {
                    format!(" · {notice}")
                } else {
                    match (rows, cols) {
                        (Some(rows), Some(cols))
                            if rows < viewport_rows || cols < viewport_cols =>
                        {
                            format!(
                                " · shared PTY {cols}×{rows}; view {viewport_cols}×{viewport_rows}"
                            )
                        }
                        _ => String::new(),
                    }
                };
                self.status.set_text(&format!(
                    "{} · {}:{} — {state}{shared}",
                    frame["session_name"].as_str().unwrap_or("Terminal"),
                    frame["window_index"].as_u64().unwrap_or(0),
                    frame["name"].as_str().unwrap_or("shell")
                ));
            }
            if self.navigation.get() {
                if self.expected_window.borrow().as_deref() == frame["window"].as_str() {
                    self.set_target(frame["active_pane"].as_str().unwrap_or("").into());
                    self.navigation.set(false);
                    *self.expected_window.borrow_mut() = None;
                }
            }
            let changed = self.frame.borrow().as_ref() != Some(&frame);
            let selection = *self.selection.borrow();
            if let Some((a, z)) = selection {
                let target = self.target.borrow();
                let incoming = panes
                    .iter()
                    .find(|p| p["id"].as_str() == Some(target.as_str()));
                let stale = self.grid().zip(incoming).is_none_or(|(old, new)| {
                    selection_changed(&old, &self.viewport_grid(&new["grid"]), a, z)
                });
                if stale {
                    self.selection.borrow_mut().take();
                }
            }
            let serials = {
                let target = self.target.borrow();
                panes
                    .iter()
                    .find(|p| p["id"].as_str() == Some(target.as_str()))
                    .map(|p| {
                        (
                            p["selection_offer"].as_u64().unwrap_or(0),
                            p["selection_request"].as_u64().unwrap_or(0),
                        )
                    })
            };
            *self.frame.borrow_mut() = Some(frame);
            if let Some(serials) = serials {
                self.program_selection(serials);
            }
            if !self.navigation.get() && !self.target.borrow().is_empty() {
                let pending = std::mem::take(&mut *self.early.borrow_mut());
                for text in pending {
                    self.input(&text);
                }
            }
            if changed {
                self.area.queue_draw();
            }
        }
        if let Some((method, v)) = s.reply.pop_front() {
            if method == "pane.selection" {
                if let Some(text) = v["text"].as_str() {
                    self.offer_program_selection(text);
                }
            }
            if method == "client.retireEnded" && v["closed"] == true && self.retiring.get() {
                self.finished.set(true);
            }
            if method == "client.focusLaunch"
                && v["focus_epoch"].as_u64() == Some(self.focus_epoch.get())
            {
                if let Some(pane) = v["pane"].as_str() {
                    self.set_target(pane.into());
                    self.navigation.set(false);
                    *self.expected_window.borrow_mut() = None;
                    let pending = std::mem::take(&mut *self.early.borrow_mut());
                    for text in pending {
                        self.input(&text);
                    }
                }
            }

            if method == "pane.create" {
                if let Some(pane) = v["pane"].as_str() {
                    self.set_target(pane.into());
                    self.navigation.set(false);
                    let pending = std::mem::take(&mut *self.early.borrow_mut());
                    for text in pending {
                        self.input(&text);
                    }
                }
            }
            if method == "window.create"
                || method == "window.select"
                || method == "window.cycle"
                || method == "window.previous"
            {
                *self.expected_window.borrow_mut() = v["window"]
                    .as_str()
                    .or(v["selected"].as_str())
                    .map(str::to_string);
                if let Some(frame) = self.frame.borrow().as_ref() {
                    if self.expected_window.borrow().as_deref() == frame["window"].as_str() {
                        self.set_target(frame["active_pane"].as_str().unwrap_or("").into());
                        self.navigation.set(false);
                        *self.expected_window.borrow_mut() = None;
                        let pending = std::mem::take(&mut *self.early.borrow_mut());
                        for text in pending {
                            self.input(&text);
                        }
                    }
                }
            }
            if method == "pane.history" || method == "pane.capture" {
                if !self.history_pending.get()
                    || v["generation"].as_u64() != Some(self.history_request.get())
                    || v["pane"].as_str() != Some(self.target.borrow().as_str())
                {
                    return;
                }
                self.history_pending.set(false);
                if let Some(error) = v["error"].as_str() {
                    self.history_inline.set(false);
                    self.warn(error);
                    return;
                }
                if self.history_inline.replace(false) {
                    self.history_lines
                        .borrow_mut()
                        .set(v["text"].as_str().unwrap_or(""));
                    let _ = self.grid();
                    self.area.queue_draw();
                    return;
                }
                if v["truncated"] == true {
                    self.warn("Copy view reached its 8 MiB limit; only the loaded history can be selected");
                }
                // The unused rows below the prompt are history too; drop them
                // so the view opens on the newest output.
                let text = v["text"].as_str().unwrap_or("");
                let text = text.trim_end_matches(|c: char| c == '\n' || c == ' ');
                self.copy.buffer().set_text(text);
                self.stack.set_visible_child_name("copy");
                self.copy_anchor.set(None);
                self.copy
                    .buffer()
                    .place_cursor(&self.copy.buffer().end_iter());
                self.copy.set_cursor_visible(true);
                self.copy.grab_focus();
                // Open at the newest output (where the wheel left the live
                // view) once the text is laid out: bottom of the scroller.
                let scroller = self.copy.parent().and_downcast::<gtk::ScrolledWindow>();
                if let Some(scroller) = scroller {
                    let adjustment = scroller.vadjustment();
                    let pending = Rc::new(Cell::new(true));
                    let weak = adjustment.downgrade();
                    let p = pending.clone();
                    let handler = adjustment.connect_changed(move |a| {
                        if p.get() {
                            a.set_value(a.upper() - a.page_size());
                        }
                    });
                    glib::timeout_add_local_once(
                        std::time::Duration::from_millis(400),
                        move || {
                            pending.set(false);
                            if let Some(a) = weak.upgrade() {
                                a.disconnect(handler);
                                a.set_value(a.upper() - a.page_size());
                            }
                        },
                    );
                }
            } else if method == "window.list" {
                if let Some(index) = self.window_index.take() {
                    if let Some(window) = v.as_array().and_then(|a| a.get(index)) {
                        self.navigate("window.select", json!({"window":window["id"]}));
                    } else {
                        self.status.set_text("No window at that index");
                    }
                } else {
                    self.show_chooser(&v);
                }
            } else if method == "buffer.get" {
                drop(s);
                self.paste(v["text"].as_str().unwrap_or("").into());
            }
        }
    }
    pub fn focus(&self) {
        if self.stack.visible_child_name().as_deref() == Some("copy") {
            self.copy.grab_focus();
        } else {
            self.area.grab_focus();
        }
        let foreground = self
            .root
            .root()
            .and_then(|r| r.downcast::<gtk::Window>().ok())
            .is_some_and(|w| w.is_active());
        if foreground {
            self.broker.borrow_mut().focus(Some(&self.session));
        } else if self.broker.borrow().active() == Some(self.session.as_str()) {
            self.broker.borrow_mut().focus(None);
        }
    }
    pub fn focus_at(&self, x: f64, y: f64) {
        if let Some((p, _, _)) = self.point(x, y) {
            if p != *self.target.borrow() || self.navigation.get() {
                self.focus_epoch.set(self.focus_epoch.get().wrapping_add(1));
                self.navigation.set(false);
                *self.expected_window.borrow_mut() = None;
                self.early.borrow_mut().clear();
            }

            if p != *self.target.borrow() {
                self.release_mouse();
                *self.selection.borrow_mut() = None;
            }
            self.set_target(p.clone());
            self.command("pane.select", json!({"pane":p}));
        }
        self.focus();
    }
    fn point(&self, x: f64, y: f64) -> Option<(String, usize, usize)> {
        let frame = self.frame.borrow();
        let frame = frame.as_ref()?;
        for p in frame["panes"].as_array()? {
            let rows = p["grid"]["rows"].as_u64()? as usize;
            let cols = p["grid"]["cols"].as_u64()? as usize;
            let px = p["rect"]["x"].as_f64().unwrap_or(0.0) * self.cell_width.get();
            let py = p["rect"]["y"].as_f64().unwrap_or(0.0) * self.cell_height.get();
            if y >= py
                && y < py + rows as f64 * self.cell_height.get()
                && x >= px
                && x < px + cols as f64 * self.cell_width.get()
            {
                return Some((
                    p["id"].as_str()?.into(),
                    ((y - py) / self.cell_height.get()) as usize,
                    ((x - px) / self.cell_width.get()) as usize,
                ));
            }
        }
        None
    }
    fn grid(&self) -> Option<Value> {
        self.frame
            .borrow()
            .as_ref()?
            .get("panes")?
            .as_array()?
            .iter()
            .find(|p| p["id"].as_str() == Some(self.target.borrow().as_str()))
            .map(|p| self.viewport_grid(&p["grid"]))
    }
    fn viewport_grid(&self, live: &Value) -> Value {
        if self.history_lines.borrow().is_empty() {
            return live.clone();
        }
        let (grid, offset) = self
            .history_lines
            .borrow_mut()
            .grid(live, self.history_offset.get());
        self.history_offset.set(offset);
        grid
    }
    fn mouse_enabled(&self) -> bool {
        self.grid().is_some_and(|g| g["mouse"] == true)
    }
    pub fn mouse(&self, button: u32, x: f64, y: f64, release: bool) {
        self.mouse_with_mods(button, x, y, release, gdk::ModifierType::empty());
    }
    pub fn mouse_with_mods(
        &self,
        button: u32,
        x: f64,
        y: f64,
        release: bool,
        mods: gdk::ModifierType,
    ) {
        if !self.mouse_enabled() {
            return;
        }
        self.mouse_position.set((x, y));
        if let Some((pane, r, c)) = self.point(x, y) {
            if pane != *self.target.borrow() {
                return;
            }
            let button = button
                + if mods.contains(gdk::ModifierType::SHIFT_MASK) {
                    4
                } else {
                    0
                }
                + if mods.contains(gdk::ModifierType::ALT_MASK) {
                    8
                } else {
                    0
                }
                + if mods.contains(gdk::ModifierType::CONTROL_MASK) {
                    16
                } else {
                    0
                };
            match (button & !28, release) {
                (0, true) | (1, false) => self
                    .mouse_gesture
                    .set(Some((button & !28, std::time::Instant::now()))),
                _ => (),
            }
            self.command("client.input",json!({"pane":pane,"kind":"mouse","text":"","mouse_event":{"button":button,"x":c+1,"y":r+1,"release":release}}));
        }
    }
    pub fn click(
        self: &Rc<Self>,
        x: f64,
        y: f64,
        button: u32,
        mods: gdk::ModifierType,
        release: bool,
    ) {
        if button == 1 && release && self.link_click.replace(false) {
            return;
        }
        if button == 1
            && !release
            && mods.contains(gdk::ModifierType::CONTROL_MASK)
            && (!self.mouse_enabled() || mods.contains(gdk::ModifierType::SHIFT_MASK))
        {
            if let Some(url) = self.url_at(x, y) {
                self.link_click.set(true);
                self.open_url(&url);
                return;
            }
        }
        if !release {
            self.focus_at(x, y);
        }
        if button == 2 {
            if release && self.selection_middle.replace(false) {
                return;
            }
            if !release {
                self.selection_middle.set(false);
                // A deliberate selection from another RUWAC tab already has
                // a compositor-owned provider. Deliver it as a normal paste,
                // including through a nested TUI on an older preserved engine
                // that cannot answer OSC 52 PRIMARY. Other application mouse
                // events and external PRIMARY owners keep their usual routing.
                if self.clipboard_action_enabled("paste")
                    && self.area.has_focus()
                    && self
                        .root
                        .root()
                        .and_downcast::<gtk::Window>()
                        .is_some_and(|w| w.is_active())
                {
                    if let Some(text) =
                        crate::terminal_primary::selected(&self.area.display().primary_clipboard())
                    {
                        self.selection_middle.set(true);
                        self.paste(text);
                        return;
                    }
                }
            }
        }
        if self.mouse_enabled() && !mods.contains(gdk::ModifierType::SHIFT_MASK) {
            self.pressed.set(if release {
                None
            } else {
                Some(button.saturating_sub(1))
            });
            self.mouse_with_mods(
                match button {
                    1 => 0,
                    2 => 1,
                    3 => 2,
                    _ => return,
                },
                x,
                y,
                release,
                mods,
            );
        } else if button == 3 && !release {
            crate::host_shortcuts::popup(self, x, y);
        } else if button == 2 && !release {
            self.paste_clipboard(true);
        }
    }
    fn url_at(&self, x: f64, y: f64) -> Option<String> {
        let (pane, row, column) = self.point(x, y)?;
        if pane != *self.target.borrow() {
            return None;
        }
        let grid = self.grid()?;
        let rows = grid["cells"].as_array()?;
        let mut first = row;
        while first > 0 && grid["wrapped"][first - 1] == true {
            first -= 1;
        }
        let mut last = row;
        while last + 1 < rows.len() && grid["wrapped"][last] == true {
            last += 1;
        }
        let mut line = String::new();
        let mut index = 0;
        for (r, cells) in rows.iter().enumerate().take(last + 1).skip(first) {
            for (c, cell) in cells.as_array()?.iter().enumerate() {
                if r < row || (r == row && c < column) {
                    if cell[1] != true {
                        index += cell[0].as_str().unwrap_or("").chars().count();
                    }
                }
                if cell[1] != true {
                    line.push_str(cell[0].as_str().unwrap_or(""));
                }
            }
        }
        crate::web_route::link_at(&line, index)
    }
    pub fn link_menu(self: &Rc<Self>, x: f64, y: f64) -> Option<gtk::Box> {
        let url = self.url_at(x, y)?;
        let source = crate::web::Source::capture(self);
        let remote = crate::launches::is_remote(&self.session, &self.target.borrow());
        let tabs = self
            .root
            .ancestor(adw::TabView::static_type())
            .and_downcast::<adw::TabView>()?
            .downgrade();
        let body = gtk::Box::new(gtk::Orientation::Vertical, 2);
        let preview = gtk::Button::with_label("Open in Web Preview");
        preview.add_css_class("flat");
        let u = url.clone();
        let weak = Rc::downgrade(self);
        let route = source.clone();
        preview.connect_clicked(move |_| {
            if let Some(view) = weak.upgrade() {
                let local = crate::web_route::parse(&u)
                    .ok()
                    .and_then(|u| crate::web_route::destination(&u).ok())
                    .is_some_and(|d| matches!(d, crate::web_route::Destination::Loopback { .. }));
                if route.is_none() && remote && local {
                    view.status
                        .set_text("Remote loopback preview requires the original bound SSH pane");
                    return;
                }
                if let Some(t) = tabs.upgrade() {
                    if let Err(e) = crate::web::open(&t, Some(&u), route.clone()) {
                        view.status.set_text(&e);
                    }
                }
            }
        });
        body.append(&preview);
        let external = gtk::Button::with_label("Open in external browser");
        external.add_css_class("flat");
        external.set_sensitive(
            !remote
                || crate::web_route::parse(&url)
                    .ok()
                    .and_then(|u| crate::web_route::destination(&u).ok())
                    == Some(crate::web_route::Destination::Public),
        );
        external.set_tooltip_text(Some("For remote loopback, open Web Preview first, then use its external-browser button to keep the verified tunnel"));
        let u = url.clone();
        external.connect_clicked(move |_| {
            gtk::gio::AppInfo::launch_default_for_uri_async(
                &u,
                None::<&gtk::gio::AppLaunchContext>,
                None::<&gtk::gio::Cancellable>,
                move |_| {},
            )
        });
        body.append(&external);
        let copy = gtk::Button::with_label("Copy URL");
        copy.add_css_class("flat");
        let weak = Rc::downgrade(self);
        copy.connect_clicked(move |_| {
            if let Some(v) = weak.upgrade() {
                if v.broker.borrow().desktop_enabled() {
                    v.broker.borrow_mut().local_copy();
                    v.area.display().clipboard().set_text(&url);
                }
            }
        });
        body.append(&copy);
        Some(body)
    }
    fn open_url(self: &Rc<Self>, url: &str) {
        let source = crate::web::Source::capture(self);
        if source.is_none()
            && crate::launches::is_remote(&self.session, &self.target.borrow())
            && crate::web_route::parse(url)
                .ok()
                .and_then(|u| crate::web_route::destination(&u).ok())
                .is_some_and(|d| matches!(d, crate::web_route::Destination::Loopback { .. }))
        {
            self.status.set_text("Remote loopback preview needs this pane's original verified SSH launch; no local URL was opened");
            return;
        }
        if let Some(tabs) = self
            .root
            .ancestor(adw::TabView::static_type())
            .and_downcast::<adw::TabView>()
        {
            if let Err(e) = crate::web::open(&tabs, Some(url), source) {
                self.status.set_text(&e);
            }
        } else {
            self.status.set_text("Web Preview requires a workspace tab");
        }
    }
    pub fn paste_desktop(self: &Rc<Self>) {
        if !crate::remote_paste::try_paste(self) {
            self.paste_clipboard(false);
        }
    }
    pub fn clipboard_action_enabled(&self, name: &str) -> bool {
        if name == "select-text" {
            return !self.detached.get() && !self.target.borrow().is_empty();
        }
        self.broker.borrow().desktop_enabled()
            && match name {
                "copy" => !self.selected().is_empty(),
                _ => !self.detached.get() && !self.ended.get() && !self.target.borrow().is_empty(),
            }
    }
    pub fn clipboard_menu(self: &Rc<Self>) -> gtk::Box {
        let body = gtk::Box::new(gtk::Orientation::Vertical, 2);
        for (name, label) in [
            ("copy", "Copy"),
            ("paste", "Paste"),
            ("paste-selection", "Paste selection"),
            ("select-text", "Select text…"),
        ] {
            let button = gtk::Button::with_label(label);
            button.add_css_class("flat");
            button.set_widget_name(&format!("terminal-{name}"));
            button.set_sensitive(self.clipboard_action_enabled(name));
            let weak = Rc::downgrade(self);
            button.connect_clicked(move |button| {
                if let Some(popover) = button
                    .ancestor(gtk::Popover::static_type())
                    .and_downcast::<gtk::Popover>()
                {
                    popover.popdown();
                }
                if let Some(view) = weak.upgrade() {
                    view.focus();
                    match name {
                        "copy" => view.copy_clipboard(false),
                        "paste" => view.paste_desktop(),
                        "paste-selection" => view.paste_clipboard(true),
                        _ => view.open_history(),
                    }
                }
            });
            body.append(&button);
        }
        body
    }
    pub fn selected(&self) -> String {
        if self.stack.visible_child_name().as_deref() == Some("copy") {
            let b = self.copy.buffer();
            return b
                .selection_bounds()
                .map(|(a, z)| b.text(&a, &z, true).to_string())
                .unwrap_or_default();
        }
        let Some((a, z)) = *self.selection.borrow() else {
            return String::new();
        };
        let Some(g) = self.grid() else {
            return String::new();
        };
        selection_text(&g, a, z)
    }
    pub fn copy_clipboard(&self, primary: bool) {
        if !self.broker.borrow().desktop_enabled() {
            self.status.set_text("Desktop clipboard disabled");
            return;
        }
        let text = self.selected();
        if text.is_empty() {
            self.status.set_text("Select terminal text before copying");
            return;
        }
        if text.len() > ruwac_clipboard::MAX_DESKTOP_TEXT {
            self.status.set_text("Selected text exceeds clipboard cap");
            return;
        }
        self.broker.borrow_mut().local_copy();
        let display = self.area.display();
        if primary {
            let provider = gdk::ContentProvider::for_value(&text.to_value());
            *self.primary_provider.borrow_mut() = Some(provider.clone());
            if display
                .primary_clipboard()
                .set_content(Some(&provider))
                .is_err()
            {
                self.primary_provider.borrow_mut().take();
                self.status
                    .set_text("The desktop could not publish this PRIMARY selection");
                return;
            }
            crate::terminal_primary::remember(&provider, &text);
        } else {
            display.clipboard().set_text(&text);
        }
        self.status.set_text(if primary {
            "Selection offered to PRIMARY"
        } else {
            "Selection copied to CLIPBOARD"
        });
    }
    /// A program in the target pane (typically a nested RUWAC client) used
    /// OSC 52 PRIMARY. Its selection reaches the desktop, and the desktop
    /// selection reaches it, only right after the user's own mouse gesture
    /// in this pane; otherwise the program keeps its internal fallback.
    fn program_selection(self: &Rc<Self>, (offer, request): (u64, u64)) {
        let (seen_offer, seen_request) = self.selection_serials.get();
        if (offer, request) == (seen_offer, seen_request) {
            return;
        }
        self.selection_serials.set((offer, request));
        let recent = |button: u32, within: u64| {
            self.mouse_gesture.get().is_some_and(|(b, at)| {
                b == button && at.elapsed() < std::time::Duration::from_millis(within)
            })
        };
        if !self.broker.borrow().desktop_enabled() || !self.area.has_focus() {
            return;
        }
        let pane = self.target.borrow().clone();
        if offer > seen_offer && recent(0, 2000) {
            self.mouse_gesture.set(None);
            self.command("pane.selection", json!({"pane":pane,"serial":offer}));
        }
        if request > seen_request && recent(1, 1500) {
            self.mouse_gesture.set(None);
            let weak = Rc::downgrade(self);
            let display = self.area.display();
            let epoch = self.broker.borrow().epoch();
            clipboard::read_text(
                &display.primary_clipboard(),
                self.broker.clone(),
                epoch,
                ruwac_clipboard::ClipScope::Primary,
                std::time::Duration::ZERO,
                move |result| {
                    if let (Some(v), Ok(text)) = (weak.upgrade(), result) {
                        if *v.target.borrow() == pane && text.len() <= 128 * 1024 {
                            v.command(
                                "pane.selectionReply",
                                json!({"pane":pane,"serial":request,"text":text}),
                            );
                        }
                    }
                },
            );
        }
    }
    fn offer_program_selection(&self, text: &str) {
        if text.is_empty() || text.len() > ruwac_clipboard::MAX_DESKTOP_TEXT {
            return;
        }
        self.broker.borrow_mut().local_copy();
        let provider = gdk::ContentProvider::for_value(&text.to_value());
        *self.primary_provider.borrow_mut() = Some(provider.clone());
        if self
            .area
            .display()
            .primary_clipboard()
            .set_content(Some(&provider))
            .is_err()
        {
            self.primary_provider.borrow_mut().take();
            self.status
                .set_text("The desktop could not publish this PRIMARY selection");
            return;
        }
        crate::terminal_primary::remember(&provider, text);
        self.status.set_text("Selection offered to PRIMARY");
    }
    pub fn paste_clipboard(self: &Rc<Self>, primary: bool) {
        let display = self.area.display();
        let clip = if primary {
            display.primary_clipboard()
        } else {
            display.clipboard()
        };
        let epoch = self.broker.borrow().epoch();
        let pane = self.target.borrow().clone();
        let weak = Rc::downgrade(self);
        clipboard::read_text(
            &clip,
            self.broker.clone(),
            epoch,
            if primary {
                ruwac_clipboard::ClipScope::Primary
            } else {
                ruwac_clipboard::ClipScope::Clipboard
            },
            std::time::Duration::ZERO,
            move |result| {
                if let Some(v) = weak.upgrade() {
                    if v.detached.get() || *v.target.borrow() != pane {
                        v.status.set_text("Paste cancelled: target changed");
                        return;
                    }
                    match result {
                        Ok(text) => v.paste(text),
                        Err(e) => v.status.set_text(&e),
                    }
                }
            },
        );
    }
    fn internal_copy(&self) {
        let text = self.selected();
        if text.is_empty() {
            self.status.set_text("Select text in copy mode, then Enter");
            return;
        }
        self.command("buffer.set", json!({"session":self.session,"text":text}));
        self.stack.set_visible_child_name("terminal");
        self.area.grab_focus();
    }
    pub fn key(self: &Rc<Self>, key: gdk::Key, mods: gdk::ModifierType) {
        if self.detached.get() {
            return;
        }
        if self.binding(key, mods, "copy", "<Control><Shift>c") {
            self.copy_clipboard(false);
            return;
        }
        let plain_control = mods.contains(gdk::ModifierType::CONTROL_MASK)
            && !mods.intersects(
                gdk::ModifierType::ALT_MASK
                    | gdk::ModifierType::SUPER_MASK
                    | gdk::ModifierType::SHIFT_MASK,
            );
        if self.desktop_shortcuts.get()
            && plain_control
            && key == gdk::Key::c
            && !self.selected().is_empty()
        {
            self.copy_clipboard(false);
            return;
        }
        // An image pasted into an SSH tab is sent to the host as a file.
        if matches!(key, gdk::Key::v | gdk::Key::V)
            && mods.contains(gdk::ModifierType::CONTROL_MASK)
            && !mods.contains(gdk::ModifierType::ALT_MASK)
            && crate::remote_paste::try_paste(self)
        {
            return;
        }
        if self.binding(key, mods, "paste", "<Control><Shift>v") {
            self.paste_clipboard(false);
            return;
        }
        if self.desktop_shortcuts.get() && plain_control && matches!(key, gdk::Key::v | gdk::Key::V)
        {
            self.paste_clipboard(false);
            return;
        }
        if !self.prefix.get() && self.binding(key, mods, "prefix", "<Control><Alt>a") {
            self.prefix.set(true);
            self.status
                .set_text("Prefix command pending · ? help · a literal prefix");
            return;
        }
        if self.stack.visible_child_name().as_deref() == Some("copy") {
            self.copy_key(key, mods);
            return;
        }
        if matches!(
            key,
            gdk::Key::Shift_L
                | gdk::Key::Shift_R
                | gdk::Key::Control_L
                | gdk::Key::Control_R
                | gdk::Key::Alt_L
                | gdk::Key::Alt_R
                | gdk::Key::Super_L
                | gdk::Key::Super_R
                | gdk::Key::ISO_Level3_Shift
        ) {
            return;
        }
        let ctrl = mods.contains(gdk::ModifierType::CONTROL_MASK);
        let ch = key.to_unicode();
        if self.prefix.replace(false) {
            if self.binding(key, mods, "prefix", "<Control><Alt>a") {
                if self
                    .shortcuts
                    .get("double_prefix")
                    .is_some_and(|v| v == "previous")
                {
                    if self
                        .worker
                        .borrow()
                        .state
                        .lock()
                        .unwrap()
                        .supports_previous_window
                    {
                        self.navigate("window.previous", json!({"session":self.session}));
                    } else {
                        self.warn("Previous-window navigation requires a newer daemon; the active session is preserved");
                    }
                } else {
                    self.input(&self.literal_prefix());
                }
                return;
            }
            let binding = if key == gdk::Key::Tab {
                "Tab".into()
            } else {
                ch.map(|c| c.to_string()).unwrap_or_default()
            };
            let binding = self.remaps.get(&binding).cloned().unwrap_or(binding);
            let binding = if key == gdk::Key::Escape {
                "[".into()
            } else if binding.chars().count() == 1 {
                ruwac_protocol::screen_key(binding.chars().next().unwrap()).to_string()
            } else {
                binding
            };
            match binding.as_str() {
                "k" => self.confirm_close_window(),
                "A" => self.show_prompt_seed("title "),
                "d" => self.close(),
                "c" => self.navigate("window.create", json!({"session":self.session})),
                "n" | "p" => self.navigate(
                    "window.cycle",
                    json!({"session":self.session,"previous":binding=="p"}),
                ),
                "a" => self.input(&self.literal_prefix()),
                "o" => self.navigate("window.previous", json!({"session":self.session})),
                "S" | "s" | "|" => self.split(binding == "|"),
                "[" => self.open_history(),
                "]" => self.command("buffer.get", json!({"session":self.session})),
                "?" => self.show_help(),
                "\"" => self.command("window.list", json!({"session":self.session})),
                ":" => self.show_prompt(),
                "z" => self.zoom(),
                "Tab" => {
                    self.release_mouse();
                    if let Some(f) = self.frame.borrow().as_ref() {
                        if let Some(p) = f["panes"].as_array() {
                            let i = p
                                .iter()
                                .position(|p| {
                                    p["id"].as_str() == Some(self.target.borrow().as_str())
                                })
                                .unwrap_or(0);
                            let next = p[(i + 1) % p.len()]["id"].as_str().unwrap().to_string();
                            self.set_target(next.clone());
                            self.command("pane.select", json!({"pane":next}));
                        }
                    }
                }
                digit if digit.len() == 1 && digit.as_bytes()[0].is_ascii_digit() => {
                    self.window_index.set(digit.parse().ok());
                    self.command("window.list", json!({"session":self.session}));
                }
                _ => {
                    self.status
                        .set_text("Unknown prefix command forwarded to application");
                    self.input(&self.literal_prefix());
                    self.key(key, mods);
                }
            }
            return;
        }
        let mut text = match key {
            gdk::Key::Return | gdk::Key::KP_Enter => "\r".into(),
            gdk::Key::BackSpace => "\x7f".into(),
            gdk::Key::Tab => "\t".into(),
            gdk::Key::Escape => "\x1b".into(),
            gdk::Key::Up => "\x1b[A".into(),
            gdk::Key::Down => "\x1b[B".into(),
            gdk::Key::Left => "\x1b[D".into(),
            gdk::Key::Right => "\x1b[C".into(),
            gdk::Key::Home => "\x1b[H".into(),
            gdk::Key::End => "\x1b[F".into(),
            gdk::Key::Delete => "\x1b[3~".into(),
            gdk::Key::Page_Up => "\x1b[5~".into(),
            gdk::Key::Page_Down => "\x1b[6~".into(),
            _ => ch.map(|c| c.to_string()).unwrap_or_default(),
        };
        if self.grid().is_some_and(|g| g["application_cursor"] == true)
            && matches!(
                key,
                gdk::Key::Up
                    | gdk::Key::Down
                    | gdk::Key::Left
                    | gdk::Key::Right
                    | gdk::Key::Home
                    | gdk::Key::End
            )
        {
            text = text.replacen("\x1b[", "\x1bO", 1);
        }
        if ctrl {
            if let Some(c) = ch {
                if ('@'..='_').contains(&c.to_ascii_uppercase()) {
                    text = (((c as u8) & 31) as char).to_string();
                }
            }
        }
        if text.contains('\0') {
            self.status.set_text("NUL key unsupported");
            return;
        }
        if mods.contains(gdk::ModifierType::ALT_MASK) {
            text.insert(0, '\x1b');
        }
        if !text.is_empty() {
            self.history_offset.set(0);
            self.cancel_inline_history();
            self.area.queue_draw();
            self.input(&text);
        }
    }
    fn release_mouse(&self) {
        if let Some(button) = self.pressed.take() {
            let (x, y) = self.mouse_position.get();
            self.mouse(button, x, y, true);
        }
    }
    pub fn close(&self) {
        self.cancel_image_paste();
        self.release_mouse();
        if self.prefix.replace(false) {
            self.input(&self.literal_prefix());
        }
        *self.selection.borrow_mut() = None;
        self.detached.set(true);
        self.worker.borrow().close();
        self.broker.borrow_mut().disconnect(&self.session);
        self.status.set_text("Detached; daemon PTYs preserved");
    }
    pub fn apply_appearance(&self, prefs: &crate::sidebar_shell::Settings) {
        self.desktop_shortcuts
            .set(prefs.terminal_shortcuts == "desktop");
        self.copy_on_select.set(prefs.copy_selection_to_clipboard);
        let font = if crate::chrome::font_available(&prefs.term_font) {
            prefs.term_font.as_str()
        } else {
            "Monospace"
        };
        *self.font.borrow_mut() = font.into();
        self.font_size.set(prefs.term_size_pt);
        self.line_height.set(prefs.line_height);
        let description =
            gtk::pango::FontDescription::from_string(&format!("{font} {}", prefs.term_size_pt));
        let layout = self.area.create_pango_layout(Some("M"));
        layout.set_font_description(Some(&description));
        self.cell_width
            .set((layout.size().0 as f64 / gtk::pango::SCALE as f64).max(1.));
        let metrics = self.area.pango_context().metrics(Some(&description), None);
        self.cell_height.set(
            (((metrics.ascent() + metrics.descent()) as f64 / gtk::pango::SCALE as f64)
                * prefs.line_height as f64)
                .round()
                .max(1.),
        );
        if self.area.width() > 0 && self.area.height() > 0 {
            self.command("client.resize",json!({"rows":((self.area.height()as f64/self.cell_height.get())as i32).clamp(4,u16::MAX as i32),"cols":((self.area.width()as f64/self.cell_width.get())as i32).clamp(10,u16::MAX as i32)}));
        }
        // Scrollback (history view) looks like the terminal: same font, size,
        // line spacing and colours.
        let text_height = (metrics.ascent() + metrics.descent()) as f64 / gtk::pango::SCALE as f64;
        let gap = (self.cell_height.get() - text_height).max(0.0).round() as i32;
        self.copy.set_pixels_above_lines(gap / 2);
        self.copy.set_pixels_below_lines(gap - gap / 2);
        let (bg, fg) = (
            crate::schemes::current().bg(),
            crate::schemes::current().fg(),
        );
        let hex = |c: (f64, f64, f64)| {
            format!(
                "#{:02x}{:02x}{:02x}",
                (c.0 * 255.0).round() as u8,
                (c.1 * 255.0).round() as u8,
                (c.2 * 255.0).round() as u8
            )
        };
        history_style(&format!(
            "textview.ruwac-history, textview.ruwac-history text {{ font-family: \"{font}\"; font-size: {}pt; background-color: {}; color: {}; }}",
            prefs.term_size_pt,
            hex(bg),
            hex(fg)
        ));
        self.area.queue_draw();
    }
    /// Test hook: press this view's Reconnect button; reports the card state.
    pub fn fixture_focus_launch_race(&self, q: &Value) -> Value {
        let local = q["local_pane"].as_str().unwrap_or("");
        let position = self
            .frame
            .borrow()
            .as_ref()
            .and_then(|f| f["panes"].as_array())
            .and_then(|rows| rows.iter().find(|p| p["id"] == local))
            .map(|p| {
                (
                    (p["rect"]["x"].as_f64().unwrap_or(0.0)
                        + p["rect"]["cols"].as_f64().unwrap_or(1.0) / 2.0)
                        * self.cell_width.get(),
                    (p["rect"]["y"].as_f64().unwrap_or(0.0)
                        + p["rect"]["rows"].as_f64().unwrap_or(1.0) / 2.0)
                        * self.cell_height.get(),
                )
            });
        if let Some((x, y)) = position {
            self.focus_launch_pane(
                q["window"].as_str().unwrap_or(""),
                q["pane"].as_str().unwrap_or(""),
                q["pid"].as_u64().unwrap_or(0) as u32,
            );
            self.focus_at(x, y);
            json!({"queued":true})
        } else {
            json!({"queued":false})
        }
    }
    pub fn fixture_reconnect(&self) -> Value {
        let visible = self.dead.is_visible();
        let ended = self.ended.get();
        let found: RefCell<Option<gtk::Button>> = RefCell::new(None);
        crate::chrome::walk(self.dead.upcast_ref(), &|w| {
            if w.widget_name() == "terminal-reconnect" {
                if let Some(b) = w.downcast_ref::<gtk::Button>() {
                    found.borrow_mut().get_or_insert(b.clone());
                }
            }
        });
        let clicked = found.into_inner().map(|b| b.emit_clicked()).is_some();
        json!({"card_visible": visible, "ended": ended, "clicked": clicked})
    }
    pub fn cell_metrics(&self) -> (f64, f64) {
        (self.cell_width.get(), self.cell_height.get())
    }
    fn draw(&self, area: &gtk::DrawingArea, cr: &gtk::cairo::Context, _width: i32, _height: i32) {
        self.drawn.set(self.drawn.get() + 1);
        let canvas_bg = crate::schemes::current().bg();
        cr.set_source_rgb(canvas_bg.0, canvas_bg.1, canvas_bg.2);
        let _ = cr.paint();
        let frame = self.frame.borrow();
        let Some(frame) = frame.as_ref() else {
            return;
        };
        let Some(panes) = frame["panes"].as_array() else {
            return;
        };
        let layout = area.create_pango_layout(None);
        let normal = gtk::pango::FontDescription::from_string(&format!(
            "{} {}",
            self.font.borrow(),
            self.font_size.get()
        ));
        let bold = gtk::pango::FontDescription::from_string(&format!(
            "{} Bold {}",
            self.font.borrow(),
            self.font_size.get()
        ));
        let italic = gtk::pango::FontDescription::from_string(&format!(
            "{} Italic {}",
            self.font.borrow(),
            self.font_size.get()
        ));
        let bold_italic = gtk::pango::FontDescription::from_string(&format!(
            "{} Bold Italic {}",
            self.font.borrow(),
            self.font_size.get()
        ));
        let attributes = gtk::pango::AttrList::new();
        attributes.insert(gtk::pango::AttrFontFeatures::new("liga=0, calt=0"));
        layout.set_attributes(Some(&attributes));
        let mut last_glyph = String::new();
        let mut last_font = None;
        for p in panes {
            let _ = cr.save();
            cr.translate(
                p["rect"]["x"].as_f64().unwrap_or(0.0) * self.cell_width.get(),
                p["rect"]["y"].as_f64().unwrap_or(0.0) * self.cell_height.get(),
            );
            let y = 0.0;
            let viewport = (self.history_offset.get() > 0
                && p["id"].as_str() == Some(self.target.borrow().as_str()))
            .then(|| self.viewport_grid(&p["grid"]));
            let g = viewport.as_ref().unwrap_or(&p["grid"]);
            if panes.len() > 1 {
                cr.set_source_rgb(0.30, 0.38, 0.48);
                cr.rectangle(
                    0.0,
                    0.0,
                    g["cols"].as_f64().unwrap_or(0.0) * self.cell_width.get(),
                    g["rows"].as_f64().unwrap_or(0.0) * self.cell_height.get(),
                );
                let _ = cr.stroke();
            }
            if let Some(rows) = g["cells"].as_array() {
                for (r, row) in rows.iter().enumerate() {
                    if let Some(cells) = row.as_array() {
                        for (c, cell) in cells.iter().enumerate() {
                            let mut bg = color(&cell[3], false);
                            if cell[5] == true {
                                bg = color(&cell[2], true);
                            }
                            if p["id"].as_str() == Some(self.target.borrow().as_str())
                                && self.selection.borrow().is_some_and(|(a, z)| {
                                    let (lo, hi) = if a <= z { (a, z) } else { (z, a) };
                                    lo <= (r, c) && (r, c) <= hi
                                })
                            {
                                bg = (0.18, 0.28, 0.48);
                            }
                            if bg != canvas_bg {
                                crate::terminal_style::background(
                                    cr,
                                    c as f64 * self.cell_width.get(),
                                    y + r as f64 * self.cell_height.get(),
                                    self.cell_width.get(),
                                    self.cell_height.get(),
                                    bg,
                                );
                            }
                        }
                        for (c, cell) in cells.iter().enumerate() {
                            if cell[1] != true && cell[6]["hidden"] != true {
                                let mut fg = if cell[5] == true {
                                    color(&cell[3], false)
                                } else {
                                    foreground(cell)
                                };
                                if cell[6]["dim"] == true {
                                    fg = (fg.0 * 0.65, fg.1 * 0.65, fg.2 * 0.65);
                                }
                                let glyph = cell[0].as_str().unwrap_or("");
                                if !matches!(glyph, "" | " ") {
                                    let font_key = (cell[4] == true, cell[6]["italic"] == true);
                                    if last_font != Some(font_key) {
                                        layout.set_font_description(Some(match font_key {
                                            (true, true) => &bold_italic,
                                            (true, false) => &bold,
                                            (false, true) => &italic,
                                            _ => &normal,
                                        }));
                                        last_font = Some(font_key);
                                    }
                                    if glyph != last_glyph {
                                        layout.set_text(glyph);
                                        last_glyph.clear();
                                        last_glyph.push_str(glyph);
                                    }
                                    cr.set_source_rgb(fg.0, fg.1, fg.2);
                                    cr.move_to(
                                        c as f64 * self.cell_width.get(),
                                        y + r as f64 * self.cell_height.get()
                                            + (self.cell_height.get()
                                                - layout.pixel_size().1 as f64)
                                                / 2.,
                                    );
                                    pangocairo::functions::show_layout(cr, &layout);
                                }
                                let ul = if cell[6]["underline_color"].is_null() {
                                    fg
                                } else {
                                    color(&cell[6]["underline_color"], true)
                                };
                                crate::terminal_style::decoration(
                                    cr,
                                    &cell[6],
                                    c as f64 * self.cell_width.get(),
                                    y + r as f64 * self.cell_height.get(),
                                    self.cell_width.get()
                                        * cell[6]["width"].as_f64().unwrap_or(1.0),
                                    self.cell_height.get(),
                                    fg,
                                    ul,
                                );
                            }
                        }
                    }
                }
            }
            if p["id"].as_str() == Some(self.target.borrow().as_str())
                && g["cursor"]["visible"] == true
            {
                cr.set_source_rgb(0.5, 0.7, 0.9);
                cr.rectangle(
                    g["cursor"]["col"].as_u64().unwrap_or(0) as f64 * self.cell_width.get(),
                    y + g["cursor"]["row"].as_u64().unwrap_or(0) as f64 * self.cell_height.get(),
                    self.cell_width.get(),
                    self.cell_height.get(),
                );
                let _ = cr.stroke();
            }
            let _ = cr.restore();
        }
        // Draw dividers after every pane background and glyph, so an adjacent
        // pane's repaint cannot erase the separator (including older engines).
        if panes.len() > 1 {
            cr.set_source_rgb(0.47, 0.67, 0.80);
            cr.set_line_width(1.0);
            for p in panes {
                let r = &p["rect"];
                let x = r["x"].as_f64().unwrap_or(0.0) * self.cell_width.get();
                let y = r["y"].as_f64().unwrap_or(0.0) * self.cell_height.get();
                let w = r["cols"].as_f64().unwrap_or(0.0) * self.cell_width.get();
                let h = r["rows"].as_f64().unwrap_or(0.0) * self.cell_height.get();
                if x > 0.0 {
                    cr.move_to(x.floor() + 0.5, y);
                    cr.line_to(x.floor() + 0.5, y + h);
                }
                if y > 0.0 {
                    cr.move_to(x, y.floor() + 0.5);
                    cr.line_to(x + w, y.floor() + 0.5);
                }
            }
            let _ = cr.stroke();
        }
    }
    pub fn select_grid_fixture(&self) {
        self.stack.set_visible_child_name("terminal");
        *self.selection.borrow_mut() = Some(((0, 0), (0, 19)));
        self.area.queue_draw();
    }
    pub fn select_fixture(&self) {
        self.copy
            .buffer()
            .set_text("Native selection žółć 🧪\nline\t'quotes'");
        let b = self.copy.buffer();
        b.select_range(&b.start_iter(), &b.end_iter());
        self.stack.set_visible_child_name("copy");
    }
}
fn foreground(cell: &Value) -> (f64, f64, f64) {
    if cell[4] == true {
        if let Some(index) = cell[2].as_u64().filter(|i| *i < 8) {
            return color(&json!(index + 8), true);
        }
    }
    color(&cell[2], true)
}
fn color(v: &Value, fg: bool) -> (f64, f64, f64) {
    if let Some(a) = v.as_array() {
        return (
            a[0].as_f64().unwrap_or(0.0) / 255.0,
            a[1].as_f64().unwrap_or(0.0) / 255.0,
            a[2].as_f64().unwrap_or(0.0) / 255.0,
        );
    }
    let n = v.as_u64().unwrap_or(256);
    if n >= 256 {
        return if fg {
            crate::schemes::current().fg()
        } else {
            crate::schemes::current().bg()
        };
    }
    let rgb = if n < 16 {
        return crate::schemes::current().ansi(n as usize);
    } else if n >= 232 {
        let l = 8 + 10 * (n - 232);
        l * 0x010101
    } else {
        let n = n - 16;
        let l = [0, 95, 135, 175, 215, 255];
        l[(n / 36) as usize] * 65536 + l[((n / 6) % 6) as usize] * 256 + l[(n % 6) as usize]
    };
    (
        ((rgb >> 16) & 255) as f64 / 255.0,
        ((rgb >> 8) & 255) as f64 / 255.0,
        (rgb & 255) as f64 / 255.0,
    )
}

#[cfg(test)]
mod palette_tests {
    use super::*;
    #[test]
    fn bold_ansi_uses_bright_palette_without_changing_indexed_or_truecolor() {
        assert_eq!(
            foreground(&json!(["x", false, 2, 257, true, false])),
            color(&json!(10), true)
        );
        assert_eq!(
            foreground(&json!(["x", false, 123, 257, true, false])),
            color(&json!(123), true)
        );
        assert_eq!(
            foreground(&json!(["x", false, [13, 27, 49], 257, true, false])),
            color(&json!([13, 27, 49]), true)
        );
        assert_eq!(
            foreground(&json!(["x", false, 2, 257, false, false])),
            color(&json!(2), true)
        );
    }
}

impl Drop for View {
    fn drop(&mut self) {
        if let Some(handler) = self.primary_handler.borrow_mut().take() {
            self.area.display().primary_clipboard().disconnect(handler);
        }
        self.cancel_image_paste();
    }
}
