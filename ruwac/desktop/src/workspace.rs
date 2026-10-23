//! Only view identities are saved; terminal output and secrets stay outside workspace metadata.
use gtk::prelude::*;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{self, Read, Write},
    os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
};

static WINDOW: std::sync::OnceLock<String> = std::sync::OnceLock::new();
pub fn configure_window(id: &str) -> io::Result<()> {
    if !id.is_empty() && !ruwac_protocol::valid_id(id, "window") {
        return Err(io::Error::other("invalid window identity"));
    }
    WINDOW
        .set(id.into())
        .map_err(|_| io::Error::other("window identity already configured"))
}
#[derive(Default, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Workspace {
    pub schema: u32,
    pub socket: PathBuf,
    pub sessions: Vec<String>,
    pub selected: Option<String>,
    #[serde(default)]
    pub tabs: Vec<Tab>,
    #[serde(default)]
    pub selected_tab: Option<usize>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Tab {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub focused_view: Option<String>,
    pub title: String,
    pub layout: Layout,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Layout {
    Terminal {
        #[serde(default)]
        view_id: String,
        session: String,
    },
    Rdp {
        #[serde(default)]
        view_id: String,
        profile: Option<crate::profiles::Profile>,
    },
    Files {
        #[serde(default)]
        view_id: String,
        profile: crate::profiles::Profile,
        directory: String,
    },
    Web {
        #[serde(default)]
        view_id: String,
        /// Route identity only; no URL, cookie, token, control path or ephemeral port.
        endpoint: Option<String>,
    },
    Split {
        vertical: bool,
        ratio: f64,
        first: Box<Layout>,
        second: Box<Layout>,
    },
}
impl Layout {
    pub fn view_id(&self) -> Option<&str> {
        match self {
            Self::Terminal { view_id, .. }
            | Self::Rdp { view_id, .. }
            | Self::Files { view_id, .. }
            | Self::Web { view_id, .. } => Some(view_id),
            Self::Split { .. } => None,
        }
    }
    fn assign_ids(&mut self) {
        match self {
            Self::Terminal { view_id, .. }
            | Self::Rdp { view_id, .. }
            | Self::Files { view_id, .. }
            | Self::Web { view_id, .. } => *view_id = ruwac_protocol::id("view"),
            Self::Split { first, second, .. } => {
                first.assign_ids();
                second.assign_ids();
            }
        }
    }
}
pub(super) fn validate(w: &Workspace) -> io::Result<()> {
    fn walk(
        l: &Layout,
        depth: usize,
        count: &mut usize,
        ids: &mut std::collections::HashSet<String>,
    ) -> io::Result<()> {
        *count += 1;
        if depth > 16 || *count > 127 {
            return Err(io::Error::other("workspace layout exceeds limit"));
        }
        match l {
            Layout::Terminal { session, .. } => {
                if !ruwac_protocol::valid_id(session, "session") || !ids.insert(session.clone()) {
                    return Err(io::Error::other("invalid or duplicate terminal identity"));
                }
            }
            Layout::Rdp {
                profile: Some(p), ..
            } => {
                p.validate().map_err(io::Error::other)?;
                if p.protocol != "RDP" {
                    return Err(io::Error::other(
                        "RDP view has a different profile protocol",
                    ));
                }
            }
            Layout::Files { profile: p, .. } => {
                p.validate().map_err(io::Error::other)?;
                if p.protocol != "SSH" || p.id.is_empty() {
                    return Err(io::Error::other("Files view needs an SSH profile identity"));
                }
            }
            Layout::Web {
                endpoint: Some(id), ..
            } if !ruwac_protocol::valid_id(id, "endpoint") => {
                return Err(io::Error::other("Invalid Web endpoint identity"));
            }
            Layout::Split {
                ratio,
                first,
                second,
                ..
            } => {
                if !ratio.is_finite() || !(0.05..=0.95).contains(ratio) {
                    return Err(io::Error::other("invalid split ratio"));
                }
                walk(first, depth + 1, count, ids)?;
                walk(second, depth + 1, count, ids)?;
            }
            _ => (),
        }
        if let Layout::Files { directory, .. } = l {
            if directory.len() > 4096 || directory.chars().any(char::is_control) {
                return Err(io::Error::other("invalid file directory"));
            }
        }
        Ok(())
    }
    if !matches!(w.schema, 1 | 2 | 3)
        || w.sessions.len() > 64
        || w.tabs.len() > 64
        || w.sessions
            .iter()
            .any(|s| !ruwac_protocol::valid_id(s, "session"))
        || w.selected.as_ref().is_some_and(|s| !w.sessions.contains(s))
        || w.selected_tab.is_some_and(|i| i >= w.tabs.len())
    {
        return Err(io::Error::other("invalid workspace schema or identities"));
    }
    if w.schema == 1 && (!w.tabs.is_empty() || w.selected_tab.is_some()) {
        return Err(io::Error::other(
            "version-one workspace has unsupported fields",
        ));
    }
    fn identities(
        l: &Layout,
        own: &mut std::collections::HashSet<String>,
        all: &mut std::collections::HashSet<String>,
        schema: u32,
    ) -> io::Result<()> {
        if let Some(id) = l.view_id() {
            if schema == 3 {
                if !ruwac_protocol::valid_id(id, "view") || !all.insert(id.to_owned()) {
                    return Err(io::Error::other("invalid or duplicate view identity"));
                }
                own.insert(id.to_owned());
            } else if !id.is_empty() {
                return Err(io::Error::other(
                    "legacy workspace contains view identity fields",
                ));
            }
        }
        if let Layout::Split { first, second, .. } = l {
            identities(first, own, all, schema)?;
            identities(second, own, all, schema)?;
        }
        Ok(())
    }
    let mut tab_ids = std::collections::HashSet::new();
    let mut view_ids = std::collections::HashSet::new();
    for tab in &w.tabs {
        let mut own = std::collections::HashSet::new();
        identities(&tab.layout, &mut own, &mut view_ids, w.schema)?;
        if w.schema == 3 {
            if !ruwac_protocol::valid_id(&tab.id, "tab")
                || !tab_ids.insert(tab.id.clone())
                || tab
                    .focused_view
                    .as_ref()
                    .is_some_and(|id| !own.contains(id))
            {
                return Err(io::Error::other("invalid tab or focused-view identity"));
            }
        } else if !tab.id.is_empty() || tab.focused_view.is_some() {
            return Err(io::Error::other(
                "legacy workspace contains tab identity fields",
            ));
        }
    }
    let mut count = 0;
    fn web_count(l: &Layout) -> usize {
        match l {
            Layout::Web { .. } => 1,
            Layout::Split { first, second, .. } => web_count(first) + web_count(second),
            _ => 0,
        }
    }
    if w.tabs.iter().map(|t| web_count(&t.layout)).sum::<usize>() > 8 {
        return Err(io::Error::other("Workspace exceeds eight Web views"));
    }
    let mut ids = std::collections::HashSet::new();
    for tab in &w.tabs {
        if tab.title.len() > 1024 || tab.title.chars().any(char::is_control) {
            return Err(io::Error::other("invalid workspace title"));
        }
        walk(&tab.layout, 0, &mut count, &mut ids)?;
    }
    if w.schema >= 2
        && !w.tabs.is_empty()
        && (ids.len() != w.sessions.len() || w.sessions.iter().any(|s| !ids.contains(s)))
    {
        return Err(io::Error::other(
            "workspace terminal identities do not match the view tree",
        ));
    }
    Ok(())
}
pub fn identity(socket: &Path) -> String {
    use sha2::{Digest, Sha256};
    let hash = format!(
        "{:x}",
        Sha256::digest(socket.as_os_str().as_encoded_bytes())
    );
    format!("workspace_{}", &hash[..32])
}
pub(super) fn directory() -> io::Result<PathBuf> {
    let p = shared_directory()?;
    if let Some(id) = WINDOW.get().filter(|id| !id.is_empty()) {
        let windows = p.join("windows");
        for child in [&windows, &windows.join(id)] {
            if !child.exists() {
                fs::DirBuilder::new().mode(0o700).create(child)?;
            }
            let m = fs::symlink_metadata(child)?;
            if !m.is_dir() || m.uid() != unsafe { libc::geteuid() } || m.mode() & 0o777 != 0o700 {
                return Err(io::Error::other("unsafe window workspace directory"));
            }
        }
        return Ok(windows.join(id));
    }
    Ok(p)
}
/// The private configuration root shared by every desktop window.
pub(super) fn shared_directory() -> io::Result<PathBuf> {
    let p = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|p| PathBuf::from(p).join(".config")))
        .ok_or_else(|| io::Error::other("configuration directory unavailable"))?
        .join("RUWAC");
    if !p.exists() {
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&p)?;
    }
    let m = fs::symlink_metadata(&p)?;
    if !m.is_dir() || m.uid() != unsafe { libc::geteuid() } || m.mode() & 0o777 != 0o700 {
        return Err(io::Error::other("unsafe workspace directory"));
    }
    Ok(p)
}
pub fn load(socket: &Path) -> io::Result<Workspace> {
    let p = existing_path(&directory()?, socket);
    let mut file = match fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(&p)
    {
        Ok(f) => f,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Workspace::default()),
        Err(e) => return Err(e),
    };
    let m = file.metadata()?;
    if !m.is_file()
        || m.uid() != unsafe { libc::geteuid() }
        || m.mode() & 0o777 != 0o600
        || m.len() > 1048576
    {
        return Err(io::Error::other("unsafe workspace file"));
    }
    let mut data = Vec::new();
    Read::by_ref(&mut file)
        .take(1048577)
        .read_to_end(&mut data)?;
    if data.len() > 1048576 {
        return Err(io::Error::other("workspace exceeds limit"));
    }
    let mut w: Workspace = serde_json::from_slice(&data)?;
    validate(&w)?;
    if w.schema == 1 {
        w.schema = 2;
        w.tabs = w
            .sessions
            .iter()
            .map(|s| Tab {
                id: String::new(),
                focused_view: None,
                title: "Terminal".into(),
                layout: Layout::Terminal {
                    view_id: String::new(),
                    session: s.clone(),
                },
            })
            .collect();
        w.selected_tab = w
            .selected
            .as_ref()
            .and_then(|s| w.sessions.iter().position(|x| x == s));
        validate(&w)?;
    }
    if w.schema < 3 {
        for tab in &mut w.tabs {
            tab.id = ruwac_protocol::id("tab");
            tab.layout.assign_ids();
        }
        w.schema = 3;
        validate(&w)?;
    }
    if w.socket != socket {
        if p.file_name().is_none_or(|n| n != "workspace.json") {
            return Err(io::Error::other("socket workspace identity mismatch"));
        }
        return Ok(Workspace::default());
    }
    Ok(w)
}

fn socket_path(dir: &Path, socket: &Path) -> PathBuf {
    use sha2::{Digest, Sha256};
    dir.join(format!(
        "workspace-{:x}.json",
        Sha256::digest(socket.as_os_str().as_encoded_bytes())
    ))
}
fn existing_path(dir: &Path, socket: &Path) -> PathBuf {
    let own = socket_path(dir, socket);
    if fs::symlink_metadata(&own).is_ok() {
        own
    } else {
        dir.join("workspace.json")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn terminal() -> Layout {
        Layout::Terminal {
            view_id: ruwac_protocol::id("view"),
            session: ruwac_protocol::id("session"),
        }
    }
    fn workspace(layout: Layout) -> Workspace {
        fn ids(l: &Layout, out: &mut Vec<String>) {
            match l {
                Layout::Terminal { session, .. } => out.push(session.clone()),
                Layout::Split { first, second, .. } => {
                    ids(first, out);
                    ids(second, out);
                }
                _ => (),
            }
        }
        let mut sessions = Vec::new();
        ids(&layout, &mut sessions);
        Workspace {
            schema: 3,
            socket: "private.sock".into(),
            sessions,
            selected: None,
            tabs: vec![Tab {
                id: ruwac_protocol::id("tab"),
                focused_view: None,
                title: "Mixed workspace".into(),
                layout,
            }],
            selected_tab: Some(0),
        }
    }
    #[test]
    fn mixed_metadata_roundtrip_and_duplicates() {
        let w = workspace(Layout::Split {
            vertical: false,
            ratio: 0.37,
            first: Box::new(terminal()),
            second: Box::new(Layout::Rdp {
                view_id: ruwac_protocol::id("view"),
                profile: None,
            }),
        });
        validate(&w).unwrap();
        let decoded: Workspace = serde_json::from_slice(&serde_json::to_vec(&w).unwrap()).unwrap();
        validate(&decoded).unwrap();
        let t = terminal();
        let duplicate = workspace(Layout::Split {
            vertical: true,
            ratio: 0.5,
            first: Box::new(t.clone()),
            second: Box::new(t),
        });
        assert!(validate(&duplicate).is_err());
    }
    #[test]
    fn corrupt_tree_and_unsupported_fields_fail_closed() {
        let mut w = workspace(terminal());
        w.sessions.clear();
        assert!(validate(&w).is_err());
        let mut w = workspace(Layout::Rdp {
            view_id: ruwac_protocol::id("view"),
            profile: None,
        });
        w.schema = 99;
        assert!(validate(&w).is_err());
        let mut raw = serde_json::to_value(workspace(terminal())).unwrap();
        raw["password"] = serde_json::json!("never permitted");
        assert!(serde_json::from_value::<Workspace>(raw).is_err());
        assert!(
            serde_json::from_str::<Layout>(
                r#"{"kind":"rdp","profile":null,"password":"never permitted"}"#
            )
            .is_err()
        );
    }
    #[test]
    fn graphical_identities_and_focus_are_validated() {
        let mut w = workspace(terminal());
        w.tabs[0].focused_view = w.tabs[0].layout.view_id().map(str::to_owned);
        validate(&w).unwrap();
        w.tabs[0].focused_view = Some(ruwac_protocol::id("view"));
        assert!(validate(&w).is_err());
        w.tabs[0].focused_view = None;
        let mut second = w.tabs[0].clone();
        second.id = ruwac_protocol::id("tab");
        if let Layout::Terminal { session, .. } = &mut second.layout {
            *session = ruwac_protocol::id("session");
            w.sessions.push(session.clone());
        }
        w.tabs.push(second);
        assert!(validate(&w).is_err()); // same graphical view ID, different valid sessions
        w.tabs[1].layout.assign_ids();
        validate(&w).unwrap();
        w.tabs[1].id = w.tabs[0].id.clone();
        assert!(validate(&w).is_err());
    }
    #[test]
    fn deep_and_invalid_ratio_layouts_are_bounded() {
        let bad = workspace(Layout::Split {
            vertical: false,
            ratio: 1.2,
            first: Box::new(terminal()),
            second: Box::new(Layout::Rdp {
                view_id: ruwac_protocol::id("view"),
                profile: None,
            }),
        });
        assert!(validate(&bad).is_err());
        let mut tree = terminal();
        for _ in 0..18 {
            tree = Layout::Split {
                vertical: false,
                ratio: 0.5,
                first: Box::new(tree),
                second: Box::new(Layout::Rdp {
                    view_id: ruwac_protocol::id("view"),
                    profile: None,
                }),
            };
        }
        assert!(validate(&workspace(tree)).is_err());
    }
}
pub fn save(w: &Workspace) -> io::Result<()> {
    validate(w)?;
    let dir = directory()?;
    let mut path = existing_path(&dir, &w.socket);
    // Preserve corrupt/unrecognized metadata instead of silently overwriting it.
    let previous = load(&w.socket)?;
    // Starting a GUI for another daemon must not overwrite the first daemon's workspace.
    if fs::symlink_metadata(&path).is_ok() && previous.socket != w.socket {
        path = socket_path(&dir, &w.socket);
    }
    backup_previous(&dir, &path)?;
    let temp = dir.join(ruwac_protocol::id("workspace-tmp"));
    let result = (|| {
        let mut file = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&temp)?;
        let data = serde_json::to_vec_pretty(w)?;
        if data.len() > 1048576 {
            return Err(io::Error::other("workspace exceeds limit"));
        }
        file.write_all(&data)?;
        file.sync_all()?;
        fs::rename(&temp, &path)?;
        fs::File::open(&dir)?.sync_all()
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

fn backup_previous(dir: &Path, path: &Path) -> io::Result<()> {
    let mut file = match fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
    {
        Ok(f) => f,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e),
    };
    let m = file.metadata()?;
    if !m.is_file()
        || m.uid() != unsafe { libc::geteuid() }
        || m.mode() & 0o777 != 0o600
        || m.len() > 1048576
    {
        return Err(io::Error::other("unsafe previous workspace"));
    }
    let mut raw = Vec::new();
    Read::by_ref(&mut file)
        .take(1048577)
        .read_to_end(&mut raw)?;
    let previous: Workspace = serde_json::from_slice(&raw)?;
    if !matches!(previous.schema, 1 | 2) {
        return Ok(());
    }
    validate(&previous)?;
    let stem = path
        .file_stem()
        .ok_or_else(|| io::Error::other("workspace filename missing"))?
        .to_string_lossy();
    let backup = dir.join(format!("{stem}-v{}.json", previous.schema));
    if let Ok(m) = fs::symlink_metadata(&backup) {
        if !m.is_file() || m.uid() != unsafe { libc::geteuid() } || m.mode() & 0o777 != 0o600 {
            return Err(io::Error::other(
                "unsafe migration backup; existing workspace preserved",
            ));
        }
        return Ok(());
    }
    let temp = dir.join(ruwac_protocol::id("workspace-backup"));
    let result = (|| {
        let mut out = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&temp)?;
        out.write_all(&raw)?;
        out.sync_all()?;
        // An exclusive link publishes the complete backup without replacing any existing file.
        fs::hard_link(&temp, &backup)?;
        fs::File::open(dir)?.sync_all()
    })();
    let _ = fs::remove_file(temp);
    result
}

pub type Recorder = std::rc::Rc<dyn Fn() -> Result<Workspace, String>>;
/// Debounced private metadata checkpointing; unchanged frames never trigger disk writes.
pub fn watch(window: &impl IsA<gtk::Window>, record: Recorder) {
    let window = window.as_ref().downgrade();
    let mut previous = Vec::new();
    let mut reported = None;
    gtk::glib::timeout_add_local(std::time::Duration::from_millis(750), move || {
        let Some(window) = window.upgrade().filter(|w| w.is_visible()) else {
            return gtk::glib::ControlFlow::Break;
        };
        let _ = window;
        let result = (|| -> Result<(), String> {
            let w = record()?;
            let signature = serde_json::to_vec(&w).map_err(|e| e.to_string())?;
            if signature != previous {
                save(&w).map_err(|e| e.to_string())?;
                previous = signature;
            }
            Ok(())
        })();
        match result {
            Ok(()) => reported = None,
            Err(e) => {
                if reported.as_ref() != Some(&e) {
                    eprintln!("Workspace autosave failed: {e}");
                    reported = Some(e);
                }
            }
        }
        gtk::glib::ControlFlow::Continue
    });
}
