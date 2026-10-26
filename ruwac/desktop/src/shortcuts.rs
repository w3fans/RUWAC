//! Window-level shortcuts run before terminal/RDP input controllers.
use gtk::{gdk, prelude::*};
use std::{
    cell::{Cell, RefCell},
    collections::HashSet,
};

/// Ctrl+Shift+T creates a terminal; Ctrl+Shift+R chooses a saved RDP host.
pub fn new_tab(key: gdk::Key, mods: gdk::ModifierType) -> Option<bool> {
    if (mods - gdk::ModifierType::LOCK_MASK)
        != (gdk::ModifierType::CONTROL_MASK | gdk::ModifierType::SHIFT_MASK)
    {
        return None;
    }
    match key.to_unicode()?.to_ascii_lowercase() {
        't' => Some(false),
        'r' => Some(true),
        _ => None,
    }
}

/// Application tab chords belong to the focused child while keys are mapped.
pub fn local_new_tab(key: gdk::Key, mods: gdk::ModifierType, map_keys: bool) -> Option<bool> {
    if map_keys { None } else { new_tab(key, mods) }
}

/// Quick host menus remain available even while keys are mapped to a remote.
pub fn host_menu(key: gdk::Key, mods: gdk::ModifierType) -> Option<bool> {
    if (mods - gdk::ModifierType::LOCK_MASK) != gdk::ModifierType::ALT_MASK {
        return None;
    }
    match key.to_unicode()?.to_ascii_lowercase() {
        'm' => Some(false),
        'a' => Some(true),
        _ => None,
    }
}

pub fn altgr(key: gdk::Key) -> bool {
    matches!(key, gdk::Key::ISO_Level3_Shift | gdk::Key::Alt_R)
}
/// Workspace tab for 1…9 and 0 (tab 10).
pub fn tab_index(key: gdk::Key) -> Option<i32> {
    match key.to_unicode()? {
        c @ '1'..='9' => Some(c as i32 - '1' as i32),
        '0' => Some(9),
        _ => None,
    }
}
pub fn select_tab(tabs: &adw::TabView, index: i32) {
    // Missing tabs are deliberately a no-op.
    if index < tabs.n_pages() {
        tabs.set_selected_page(&tabs.nth_page(index));
    }
}
/// Alt+digit (no other modifiers) selects a RUWAC tab unless keys are mapped.
pub fn alt_tab_index(key: gdk::Key, mods: gdk::ModifierType) -> Option<i32> {
    let allowed = (gdk::ModifierType::ALT_MASK | gdk::ModifierType::LOCK_MASK).bits();
    if !mods.contains(gdk::ModifierType::ALT_MASK) || mods.bits() & !allowed != 0 {
        return None;
    }
    tab_index(key)
}

/// AltGr tap toggles Map keys: a clean press and release of AltGr with no
/// other key in between. AltGr held with a key stays layout input (@, \ …).
/// Off (default): Alt+1…9/0 switch RUWAC tabs and system shortcuts such as
/// Alt+Tab stay with this computer. On: they go to the terminal program or
/// remote desktop (a RUWAC session switches its windows with Alt+N).
#[derive(Default)]
pub struct LocalMode {
    active: Cell<bool>,
    held: Cell<Option<u32>>,
    other_held: RefCell<HashSet<u32>>,
    chorded: Cell<bool>,
    buttons: Cell<u32>,
}
impl LocalMode {
    pub fn active(&self) -> bool {
        self.active.get()
    }
    pub fn press(&self, key: gdk::Key, code: u32) {
        if altgr(key) {
            match self.held.get() {
                None => {
                    self.held.set(Some(code));
                    self.chorded
                        .set(!self.other_held.borrow().is_empty() || self.buttons.get() != 0);
                }
                Some(held) if held != code => self.chorded.set(true),
                _ => {} // Auto-repeat must not erase an already observed chord.
            }
        } else {
            self.other_held.borrow_mut().insert(code);
            self.chord();
        }
    }
    pub fn chord(&self) {
        if self.held.get().is_some() {
            self.chorded.set(true);
        }
    }
    pub fn pointer(&self, button: u32, down: bool) {
        self.chord();
        if (1..=32).contains(&button) {
            let bit = 1u32 << (button - 1);
            self.buttons.set(if down {
                self.buttons.get() | bit
            } else {
                self.buttons.get() & !bit
            });
        }
    }
    pub fn cancel(&self) {
        self.held.set(None);
        self.other_held.borrow_mut().clear();
        self.buttons.set(0);
        self.chorded.set(false);
    }
    /// Only the physical release of a clean AltGr tap toggles the host mode.
    pub fn release(&self, code: u32) -> Option<bool> {
        self.other_held.borrow_mut().remove(&code);
        if self.held.get() != Some(code) {
            return None;
        }
        self.held.set(None);
        if self.chorded.get() {
            return None;
        }
        self.active.set(!self.active.get());
        Some(self.active.get())
    }
}

/// Track cancellation and pointer chords consistently in main and detached windows.
pub fn bind_mode(window: &gtk::Window, mode: &std::rc::Rc<LocalMode>) {
    let cancelled = mode.clone();
    window.connect_is_active_notify(move |w| {
        if !w.is_active() {
            cancelled.cancel();
        }
    });
    let cancelled = mode.clone();
    window.connect_focus_widget_notify(move |_| cancelled.cancel());
    let pointer_mode = mode.clone();
    let pointer = gtk::EventControllerLegacy::new();
    pointer.set_propagation_phase(gtk::PropagationPhase::Capture);
    pointer.connect_event(move |_, event| {
        match event.event_type() {
            gdk::EventType::ButtonPress | gdk::EventType::ButtonRelease => {
                if let Some(button) = event.downcast_ref::<gdk::ButtonEvent>() {
                    pointer_mode.pointer(
                        button.button(),
                        event.event_type() == gdk::EventType::ButtonPress,
                    );
                }
            }
            gdk::EventType::Scroll | gdk::EventType::TouchBegin => pointer_mode.chord(),
            _ => (),
        }
        gtk::glib::Propagation::Proceed
    });
    window.add_controller(pointer);
}

/// Explicit host chords leave Right Alt/AltGr available to the remote layout.
pub fn local_escape(key: gdk::Key, mods: gdk::ModifierType) -> bool {
    key == gdk::Key::Home
        && (mods - gdk::ModifierType::LOCK_MASK)
            == (gdk::ModifierType::CONTROL_MASK | gdk::ModifierType::ALT_MASK)
}
pub fn fullscreen(key: gdk::Key, mods: gdk::ModifierType) -> bool {
    key == gdk::Key::Return
        && (mods - gdk::ModifierType::LOCK_MASK)
            == (gdk::ModifierType::CONTROL_MASK | gdk::ModifierType::ALT_MASK)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn host_chords_do_not_steal_altgr() {
        let chord = gdk::ModifierType::CONTROL_MASK | gdk::ModifierType::ALT_MASK;
        assert!(local_escape(gdk::Key::Home, chord));
        assert!(fullscreen(gdk::Key::Return, chord));
        for k in [
            gdk::Key::Alt_R,
            gdk::Key::ISO_Level3_Shift,
            gdk::Key::a,
            gdk::Key::F11,
        ] {
            assert!(!local_escape(k, chord) && !fullscreen(k, chord));
        }
    }
}

#[cfg(test)]
mod new_tab_tests {
    use super::*;
    #[test]
    fn new_tabs_require_exact_host_chords() {
        let chord = gdk::ModifierType::CONTROL_MASK | gdk::ModifierType::SHIFT_MASK;
        assert_eq!(new_tab(gdk::Key::T, chord), Some(false));
        assert_eq!(
            new_tab(gdk::Key::r, chord | gdk::ModifierType::LOCK_MASK),
            Some(true)
        );
        assert_eq!(new_tab(gdk::Key::t, gdk::ModifierType::CONTROL_MASK), None);
        assert_eq!(
            new_tab(gdk::Key::R, chord | gdk::ModifierType::ALT_MASK),
            None
        );
        assert_eq!(
            new_tab(gdk::Key::R, chord | gdk::ModifierType::SUPER_MASK),
            None
        );
    }
}

#[cfg(test)]
mod local_mode_tests {
    use super::*;
    #[test]
    fn altgr_tap_toggles_and_altgr_chords_do_not() {
        let mode = LocalMode::default();
        mode.press(gdk::Key::ISO_Level3_Shift, 108);
        assert_eq!(mode.release(108), Some(true));
        assert!(mode.active());
        // AltGr+v types @ on many layouts: no toggle.
        mode.press(gdk::Key::ISO_Level3_Shift, 108);
        mode.press(gdk::Key::v, 55);
        assert_eq!(mode.release(108), None);
        assert_eq!(mode.release(55), None);
        assert!(mode.active());
        mode.press(gdk::Key::Alt_R, 108);
        assert_eq!(mode.release(108), Some(false));
        assert_eq!(
            alt_tab_index(gdk::Key::_2, gdk::ModifierType::ALT_MASK),
            Some(1)
        );
        assert_eq!(
            alt_tab_index(gdk::Key::_0, gdk::ModifierType::ALT_MASK),
            Some(9)
        );
        assert_eq!(
            alt_tab_index(gdk::Key::_2, gdk::ModifierType::empty()),
            None
        );
    }
}

#[cfg(test)]
mod menu_tests {
    use super::*;
    #[test]
    fn menu_chords_exclude_remote_altgr_ctrl_and_super() {
        assert_eq!(
            host_menu(gdk::Key::m, gdk::ModifierType::ALT_MASK),
            Some(false)
        );
        assert_eq!(
            host_menu(
                gdk::Key::a,
                gdk::ModifierType::ALT_MASK | gdk::ModifierType::LOCK_MASK
            ),
            Some(true)
        );
        for mods in [
            gdk::ModifierType::empty(),
            gdk::ModifierType::CONTROL_MASK | gdk::ModifierType::ALT_MASK,
            gdk::ModifierType::ALT_MASK | gdk::ModifierType::SUPER_MASK,
            gdk::ModifierType::ALT_MASK | gdk::ModifierType::SHIFT_MASK,
        ] {
            assert_eq!(host_menu(gdk::Key::m, mods), None);
        }
    }
}

#[cfg(test)]
mod mapped_tab_tests {
    use super::*;
    #[test]
    fn mapped_tab_chords_are_not_host_actions() {
        let mods = gdk::ModifierType::CONTROL_MASK | gdk::ModifierType::SHIFT_MASK;
        for key in [gdk::Key::t, gdk::Key::T, gdk::Key::r, gdk::Key::R] {
            assert_eq!(local_new_tab(key, mods, true), None);
            assert_eq!(
                local_new_tab(key, mods | gdk::ModifierType::LOCK_MASK, true),
                None
            );
            assert!(local_new_tab(key, mods, false).is_some());
        }
    }
}

#[cfg(test)]
mod physical_tap_tests {
    use super::*;
    #[test]
    fn modifier_chord_and_repeat_never_become_a_tap() {
        let m = LocalMode::default();
        m.press(gdk::Key::Alt_R, 108);
        m.press(gdk::Key::Shift_L, 50);
        m.press(gdk::Key::Alt_R, 108);
        assert_eq!(m.release(108), None);
        m.release(50);
        assert!(!m.active());
        m.press(gdk::Key::Control_L, 37);
        m.press(gdk::Key::Alt_R, 108);
        assert_eq!(m.release(108), None);
        m.release(37);
    }
    #[test]
    fn physical_release_survives_changed_key_symbol() {
        let m = LocalMode::default();
        m.press(gdk::Key::A, 38);
        m.release(38);
        m.press(gdk::Key::Alt_R, 108);
        assert_eq!(m.release(108), Some(true));
    }
    #[test]
    fn focus_loss_and_pointer_chords_cancel_tap_without_changing_mode() {
        let m = LocalMode::default();
        m.press(gdk::Key::Alt_R, 108);
        m.cancel();
        assert_eq!(m.release(108), None);
        assert!(!m.active());
        m.pointer(1, true);
        m.press(gdk::Key::Alt_R, 108);
        assert_eq!(m.release(108), None);
        m.pointer(1, false);
        m.press(gdk::Key::Alt_R, 108);
        m.pointer(1, true);
        assert_eq!(m.release(108), None);
        m.pointer(1, false);
        m.press(gdk::Key::Alt_R, 108);
        assert_eq!(m.release(108), Some(true));
        m.cancel();
        assert!(m.active());
    }
}
