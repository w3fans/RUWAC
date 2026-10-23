//! Native overlay/docked machine sidebar and private desktop preferences.
use adw::prelude::*;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{Read, Write},
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::PathBuf,
};

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    pub schema: u32,
    #[serde(default = "default_scheme")]
    pub scheme: String,
    #[serde(default = "default_font")]
    pub term_font: String,
    #[serde(default = "default_size")]
    pub term_size_pt: u8,
    #[serde(default = "default_height")]
    pub line_height: f32,
    #[serde(default = "default_terminal_shortcuts")]
    pub terminal_shortcuts: String,
    #[serde(default)]
    pub copy_selection_to_clipboard: bool,
    #[serde(default = "default_labels")]
    pub bar_labels: bool,
    /// Standard (GNOME Terminal) colours for the 16 ANSI colours instead of
    /// the scheme's softer ones.
    #[serde(default = "default_vivid")]
    pub vivid_colors: bool,
    #[serde(default)]
    pub actions_collapsed: bool,
    #[serde(default)]
    pub controls_docked: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub primary_receive: bool,

    pinned: bool,
    width: u16,
    #[serde(default, skip_serializing_if = "std::collections::BTreeSet::is_empty")]
    collapsed_groups: std::collections::BTreeSet<String>,
}
fn default_terminal_shortcuts() -> String {
    "desktop".into()
}
fn default_scheme() -> String {
    "graphite".into()
}
fn default_font() -> String {
    "JetBrains Mono".into()
}
fn default_size() -> u8 {
    12
}
/// Standard terminal line spacing (as GNOME Terminal and most others).
fn default_height() -> f32 {
    1.0
}
pub const LINE_HEIGHTS: [f32; 5] = [0.95, 1.0, 1.15, 1.30, 1.45];
fn default_labels() -> bool {
    true
}
fn default_vivid() -> bool {
    true
}
pub const FONTS: [&str; 6] = [
    "JetBrains Mono",
    "IBM Plex Mono",
    "Fira Code",
    "Source Code Pro",
    "Red Hat Mono",
    "Ubuntu Mono",
];
impl Settings {
    fn validate(&self) -> bool {
        [1, 2, 3].contains(&self.schema)
            && [
                "graphite", "nocturne", "fjord", "ember", "paper", "contrast",
            ]
            .contains(&self.scheme.as_str())
            && FONTS.contains(&self.term_font.as_str())
            && (9..=18).contains(&self.term_size_pt)
            && LINE_HEIGHTS.contains(&self.line_height)
            && ["desktop", "traditional"].contains(&self.terminal_shortcuts.as_str())
            && (200..=420).contains(&self.width)
            && self.collapsed_groups.len() <= 1000
            && !self
                .collapsed_groups
                .iter()
                .any(|g| g.len() > 1024 || g.contains('\0'))
    }
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            schema: 3,
            scheme: default_scheme(),
            term_font: default_font(),
            term_size_pt: default_size(),
            line_height: default_height(),
            terminal_shortcuts: default_terminal_shortcuts(),
            copy_selection_to_clipboard: false,
            bar_labels: true,
            vivid_colors: default_vivid(),
            actions_collapsed: false,
            controls_docked: false,
            primary_receive: false,
            pinned: false,
            width: 260,
            collapsed_groups: Default::default(),
        }
    }
}
fn path() -> Result<PathBuf, String> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .ok_or("Private preferences unavailable")?
        .join("RUWAC");
    let m = fs::symlink_metadata(&base).map_err(|_| "Private preferences unavailable")?;
    if !m.is_dir() || m.uid() != unsafe { libc::geteuid() } || m.mode() & 0o077 != 0 {
        return Err("Private preferences unavailable".into());
    }
    Ok(base.join("ui.json"))
}
pub fn load() -> Result<Settings, String> {
    let path = path()?;
    let file = match fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
    {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Settings::default()),
        Err(_) => return Err("Existing desktop preferences protected".into()),
    };
    let m = file.metadata().map_err(|_| "Preferences unavailable")?;
    if !m.is_file()
        || m.uid() != unsafe { libc::geteuid() }
        || m.mode() & 0o077 != 0
        || m.nlink() != 1
        || m.len() > 1024 * 1024
    {
        return Err("Existing desktop preferences protected".into());
    }
    let mut bytes = Vec::new();
    file.take(1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "Preferences unreadable")?;
    let mut value: Settings =
        serde_json::from_slice(&bytes).map_err(|_| "Existing desktop preferences protected")?;
    if !value.validate() {
        return Err("Existing desktop preferences protected".into());
    }
    // Before schema 3 the default spacing was 1.30; such files never chose
    // it deliberately, so they get the standard 1.0. Later choices persist.
    if value.schema < 3 && value.line_height == 1.30 {
        value.line_height = default_height();
    }
    value.schema = 3;
    Ok(value)
}

pub fn primary_receive_allowed() -> bool {
    load().is_ok_and(|p| p.primary_receive)
}
pub fn set_primary_receive(enabled: bool) -> Result<(), String> {
    let mut settings = load()?;
    settings.primary_receive = enabled;
    save(&settings)
}

pub fn collapsed_groups() -> std::collections::BTreeSet<String> {
    load().unwrap_or_default().collapsed_groups
}
pub fn set_group_collapsed(group: &str, collapsed: bool) -> Result<(), String> {
    let mut settings = load()?;
    if collapsed {
        if group.len() > 1024 || group.contains('\0') || settings.collapsed_groups.len() >= 1000 {
            return Err("Group display preference limit reached".into());
        }
        settings.collapsed_groups.insert(group.into());
    } else {
        settings.collapsed_groups.remove(group);
    }
    save(&settings)
}
fn save(value: &Settings) -> Result<(), String> {
    if !value.validate() || value.schema != 3 {
        return Err("Invalid desktop preferences".into());
    }
    load()?; // Refuse unknown or unsafe existing settings rather than replacing them.
    let bytes = serde_json::to_vec(value).map_err(|_| "Invalid preferences")?;
    if bytes.len() > 1024 * 1024 {
        return Err("Desktop preferences exceed the size limit".into());
    }
    let target = path()?;
    let tmp = target.with_file_name(format!("{}.tmp", ruwac_protocol::id("ui")));
    let result = (|| {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&tmp)
            .map_err(|_| "Preferences cannot be saved")?;
        file.write_all(&bytes)
            .map_err(|_| "Preferences cannot be saved")?;
        file.sync_all().map_err(|_| "Preferences cannot be saved")?;
        load()?;
        fs::rename(&tmp, &target).map_err(|_| "Preferences cannot be saved")?;
        fs::File::open(target.parent().unwrap())
            .and_then(|f| f.sync_all())
            .map_err(|_| "Preferences cannot be saved")?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(tmp);
    }
    result
}

pub struct Shell {
    pub view: adw::OverlaySplitView,
    pub button: gtk::ToggleButton,
    pub pinned: gtk::CheckButton,
    pub width: gtk::SpinButton,
}
impl Shell {
    pub fn prefs() -> Settings {
        load().unwrap_or_default()
    }
    pub fn update(change: impl FnOnce(&mut Settings)) -> Result<Settings, String> {
        let mut prefs = load()?;
        change(&mut prefs);
        prefs.schema = 3;
        save(&prefs)?;
        Ok(prefs)
    }

    pub fn new(
        sidebar: &gtk::Box,
        tabs: &adw::TabView,
        preferences: &gtk::Box,
        show: bool,
    ) -> Self {
        let loaded = load();
        let settings = loaded.clone().unwrap_or_default();
        let view = adw::OverlaySplitView::builder()
            .sidebar(sidebar)
            .content(tabs)
            .collapsed(!settings.pinned)
            .show_sidebar(show || settings.pinned)
            .min_sidebar_width(settings.width as f64)
            .max_sidebar_width(settings.width as f64)
            .sidebar_width_unit(adw::LengthUnit::Px)
            .vexpand(true)
            .hexpand(true)
            .build();
        let button = gtk::ToggleButton::with_label("Machines");
        button.set_widget_name("machines-toggle");
        button.set_tooltip_text(Some(
            "Show/hide Machines · Alt+M / Ctrl+K searches · Escape closes the overlay",
        ));
        button.set_active(view.shows_sidebar());
        let v = view.downgrade();
        button.connect_toggled(move |b| {
            if let Some(v) = v.upgrade() {
                v.set_show_sidebar(b.is_active());
            }
        });
        let b = button.downgrade();
        view.connect_show_sidebar_notify(move |v| {
            if let Some(b) = b.upgrade() {
                b.set_active(v.shows_sidebar());
            }
        });
        let pinned = gtk::CheckButton::with_label("Keep Machines sidebar pinned");
        pinned.set_widget_name("machines-pinned");
        pinned.set_active(settings.pinned);
        preferences.append(&pinned);
        preferences.append(&gtk::Label::new(Some("Machines sidebar width (pixels)")));
        let width = gtk::SpinButton::with_range(200.0, 420.0, 10.0);
        width.set_widget_name("machines-width");
        width.set_value(settings.width as f64);
        preferences.append(&width);
        let status = gtk::Label::new(None);
        status.set_wrap(true);
        preferences.append(&status);
        if loaded.is_err() {
            status.set_text("Preferences are unavailable; changes apply to this window");
        }
        let (v, w, s) = (view.downgrade(), width.downgrade(), status.clone());
        pinned.connect_toggled(move |p| {
            let (Some(v), Some(w)) = (v.upgrade(), w.upgrade()) else {
                return;
            };
            v.set_collapsed(!p.is_active());
            if p.is_active() {
                v.set_show_sidebar(true);
            }
            if save(&Settings {
                schema: 3,
                pinned: p.is_active(),
                width: w.value_as_int() as u16,
                ..load().unwrap_or_default()
            })
            .is_err()
            {
                s.set_text("Preferences could not be saved; changes apply to this window");
            }
        });
        let (v, p, s) = (view.downgrade(), pinned.downgrade(), status);
        width.connect_value_changed(move |w| {
            let (Some(v), Some(p)) = (v.upgrade(), p.upgrade()) else {
                return;
            };
            v.set_min_sidebar_width(w.value());
            v.set_max_sidebar_width(w.value());
            if save(&Settings {
                schema: 3,
                pinned: p.is_active(),
                width: w.value_as_int() as u16,
                ..load().unwrap_or_default()
            })
            .is_err()
            {
                s.set_text("Preferences could not be saved; changes apply to this window");
            }
        });
        let v = view.downgrade();
        tabs.connect_selected_page_notify(move |_| {
            let Some(v) = v.upgrade() else {
                return;
            };
            if v.is_collapsed() {
                v.set_show_sidebar(false);
            }
        });
        Self {
            view,
            button,
            pinned,
            width,
        }
    }
    pub fn fixture(&self, q: &serde_json::Value) -> serde_json::Value {
        if q["toggle"] == true {
            self.button.emit_clicked();
        }
        if let Some(pinned) = q["pinned"].as_bool() {
            self.pinned.set_active(pinned);
        }
        if let Some(width) = q["width"].as_f64() {
            if (200.0..=420.0).contains(&width) {
                self.width.set_value(width);
            }
        }
        if let Some(shown) = q["shown"].as_bool() {
            self.button.set_active(shown);
        }
        serde_json::json!({"shown":self.view.shows_sidebar(),"overlay":self.view.is_collapsed(),"pinned":self.pinned.is_active(),"width":self.width.value_as_int(),"allocated_sidebar_width":self.view.sidebar().map(|w|w.width()),"allocated_content_width":self.view.content().map(|w|w.width()),"allocated_width":self.view.width()})
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn old_default_spacing_migrates_once_to_standard() {
        let old: Settings =
            serde_json::from_str(r#"{"schema":2,"pinned":false,"width":260,"line_height":1.3}"#)
                .unwrap();
        assert!(old.validate());
        assert_eq!(default_height(), 1.0);
        assert!(LINE_HEIGHTS.contains(&1.0) && LINE_HEIGHTS.contains(&1.30));
    }
    #[test]
    fn appearance_schema_defaults_and_rejection() {
        let mut v: Settings =
            serde_json::from_str(r#"{"schema":1,"pinned":false,"width":260}"#).unwrap();
        assert!(v.validate());
        assert_eq!(v.scheme, "graphite");
        assert_eq!(v.term_size_pt, 12);
        assert_eq!(v.line_height, 1.0);
        v.schema = 3;
        assert!(v.validate());
        v.scheme = "unknown".into();
        assert!(!v.validate());
        v.scheme = "paper".into();
        v.term_size_pt = 19;
        assert!(!v.validate());
        v.term_size_pt = 12;
        v.line_height = f32::NAN;
        assert!(!v.validate());
        v.line_height = 1.45;
        v.term_font = "unconfigured".into();
        assert!(!v.validate());
        assert!(
            serde_json::from_str::<Settings>(
                r#"{"schema":2,"pinned":false,"width":260,"unknown":true}"#
            )
            .is_err()
        );
    }
    #[test]
    fn old_sidebar_preferences_preserve_width_and_default_groups_open() {
        let old: Settings =
            serde_json::from_str(r#"{"schema":1,"pinned":true,"width":320}"#).unwrap();
        assert!(old.pinned && old.collapsed_groups.is_empty());
        assert_eq!(old.width, 320);
        let mut current = old;
        current.collapsed_groups.insert("Generated/Group".into());
        let saved: Settings =
            serde_json::from_slice(&serde_json::to_vec(&current).unwrap()).unwrap();
        assert_eq!(saved.collapsed_groups, current.collapsed_groups);
        assert!(saved.pinned);
        assert_eq!(saved.width, 320);
    }
}
