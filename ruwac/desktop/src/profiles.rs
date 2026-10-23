//! Non-secret machine metadata and reviewed Remmina imports.
#[path = "inventory.rs"]
mod inventory;
use gtk::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    cell::{Cell, RefCell},
    io::Read,
    path::{Path, PathBuf},
    rc::Rc,
};
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub machine_id: String,
    #[serde(default)]
    pub preferences: Preferences,
    #[serde(default)]
    pub inherit: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub inherit_credential: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub inherit_connection: bool,
    pub name: String,
    pub group: String,
    pub protocol: String,
    pub server: String,
    pub username: String,
    pub secret_reference: String,
    pub favorite: bool,
    pub last_used: u64,
    pub tags: Vec<String>,
}
/// Nonsecret endpoint overrides. Empty optional values inherit application defaults.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Preferences {
    #[serde(skip_serializing_if = "String::is_empty")]
    pub rdp_mode: String,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub primary_sync: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub primary_paste: bool,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub primary_user: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub primary_display: String,
    /// Zero keeps the existing SSH configuration port.
    #[serde(skip_serializing_if = "zero_port")]
    pub primary_port: u16,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub codex_command: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub claude_command: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub kimi_command: String,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub agent_install_missing: bool,
    /// Install the RUWAC CLI (verified prebuilt package) when an agent
    /// launch finds it missing on the machine.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub ruwac_install_missing: bool,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub agent_backend: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub agent_session: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub gnome_username: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub gnome_secret_reference: String,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub gnome_auto_login: bool,
    pub label: String,
    pub network: String,
    pub gateway: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub ssh_jump_alias: String,
    pub clipboard: u32,
    pub display: String,
    pub zoom: f64,
    pub fullscreen: bool,
    pub reconnect: bool,
    pub trust: String,
    pub identity_file: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub identity_secret_reference: String,
    pub known_hosts: String,
    /// "password" answers SSH password prompts from the endpoint's saved
    /// Secret Service credential; empty means key (or SSH configuration).
    #[serde(skip_serializing_if = "String::is_empty")]
    pub ssh_auth: String,
}
fn zero_port(port: &u16) -> bool {
    *port == 0
}
impl Default for Preferences {
    fn default() -> Self {
        Self {
            rdp_mode: String::new(),
            primary_sync: false,
            primary_paste: false,
            primary_user: String::new(),
            primary_display: String::new(),
            primary_port: 0,
            codex_command: String::new(),
            claude_command: String::new(),
            kimi_command: String::new(),
            agent_install_missing: false,
            ruwac_install_missing: false,
            agent_backend: String::new(),
            agent_session: String::new(),
            gnome_username: String::new(),
            gnome_secret_reference: String::new(),
            gnome_auto_login: false,
            label: String::new(),
            network: String::new(),
            gateway: String::new(),
            ssh_jump_alias: String::new(),
            clipboard: 0,
            display: "dynamic".into(),
            zoom: 1.0,
            fullscreen: false,
            reconnect: true,
            trust: String::new(),
            identity_file: String::new(),
            identity_secret_reference: String::new(),
            known_hosts: String::new(),
            ssh_auth: String::new(),
        }
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Library {
    schema: u32,
    items: Vec<Profile>,
    #[serde(default)]
    groups: Vec<Group>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Group {
    id: String,
    path: String,
    defaults: Preferences,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    username: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    credential_profile: String,
}
impl Profile {
    fn identify(&mut self) {
        if self.id.is_empty() {
            self.id = ruwac_protocol::id("endpoint");
        }
        if self.machine_id.is_empty() {
            self.machine_id = ruwac_protocol::id("machine");
        }
        if self.secret_reference.is_empty() {
            self.secret_reference = self.id.clone();
        }
    }
}
pub fn endpoint(address: &str, default_port: u16) -> Result<(String, u16), String> {
    if address.is_empty()
        || address.len() > 1024
        || address.chars().any(|c| c.is_whitespace() || c.is_control())
        || address.starts_with('-')
    {
        return Err("Invalid endpoint".into());
    }
    if let Ok(socket) = address.parse::<std::net::SocketAddr>() {
        if socket.port() == 0 {
            return Err("Endpoint port cannot be zero".into());
        }
        return Ok((socket.ip().to_string(), socket.port()));
    }
    if address.parse::<std::net::Ipv6Addr>().is_ok() {
        return Ok((address.into(), default_port));
    }
    if let Some((host, port)) = address.rsplit_once(':') {
        let port = port.parse::<u16>().map_err(|_| "Invalid endpoint port")?;
        if host.is_empty() || host.contains(':') || port == 0 {
            return Err("Invalid endpoint".into());
        }
        return Ok((host.into(), port));
    }
    Ok((address.into(), default_port))
}
/// Host and port for clients that cannot read ~/.ssh/config (SFTP, rsync
/// with `-F /dev/null`): an explicit `host:port` wins, otherwise OpenSSH's
/// own resolution (`ssh -G`) supplies HostName and Port, else port 22.
pub fn ssh_endpoint(server: &str) -> Result<(String, u16), String> {
    let (host, port) = endpoint(server, 0)?;
    if port != 0 {
        return Ok((host, port));
    }
    if host.starts_with('-') {
        return Err("Invalid SSH host".into());
    }
    let resolved = std::process::Command::new("ssh")
        .args(["-G", "--", &host])
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default();
    Ok(resolved_endpoint(&resolved, &host))
}
fn resolved_endpoint(ssh_g: &str, host: &str) -> (String, u16) {
    let value = |key: &str| {
        ssh_g.lines().find_map(|l| {
            l.split_once(' ')
                .filter(|(k, _)| k.eq_ignore_ascii_case(key))
                .map(|(_, v)| v.trim().to_string())
        })
    };
    let hostname = value("hostname").filter(|h| !h.is_empty() && !h.starts_with('-'));
    let port = value("port").and_then(|p| p.parse().ok()).unwrap_or(22);
    (hostname.unwrap_or_else(|| host.into()), port)
}
pub fn ssh_argv(p: &Profile) -> Result<Vec<String>, String> {
    // Without an explicit port, ~/.ssh/config (Port, HostName, ProxyJump…)
    // decides, exactly as for `ssh host` typed in a terminal.
    let (host, port) = endpoint(&p.server, 0)?;
    let mut argv = vec!["ssh".into()];
    if port != 0 {
        argv.extend(["-p".into(), port.to_string()]);
    }
    argv.extend(["-o".into(), "StrictHostKeyChecking=ask".into()]);
    if !p.preferences.known_hosts.is_empty() {
        argv.extend([
            "-o".into(),
            format!("UserKnownHostsFile={}", p.preferences.known_hosts),
        ]);
    }
    if !p.preferences.identity_file.is_empty() {
        argv.extend([
            "-i".into(),
            p.preferences.identity_file.clone(),
            "-o".into(),
            "IdentitiesOnly=yes".into(),
        ]);
    }
    if !p.username.is_empty() {
        argv.extend(["-l".into(), p.username.clone()]);
    }
    if !p.preferences.ssh_jump_alias.is_empty() {
        // The bastion cannot read this computer's ssh config: give it the
        // resolved HostName and Port when no explicit port was entered.
        let (target, target_port) = if port == 0 {
            ssh_endpoint(&p.server)?
        } else {
            (host.clone(), port)
        };
        let proxy =
            crate::ssh_jump::proxy_command(&p.preferences.ssh_jump_alias, &target, target_port)?;
        argv.extend([
            "-o".into(),
            format!("ProxyCommand={proxy}"),
            "-o".into(),
            "ForwardAgent=no".into(),
        ]);
    }
    argv.extend(["--".into(), host]);
    Ok(argv)
}
/// Session argv for an SSH command line built from this profile. Password
/// logins run ssh with the desktop binary as SSH_ASKPASS, which reads the
/// endpoint's saved credential; the secret never enters argv or the profile.
pub fn session_argv(p: &Profile, argv: Vec<String>) -> Vec<String> {
    // Keep the terminal available if private transport setup is unavailable.
    // Implicit image paste then explains that an explicit reopen is required.
    let argv = crate::ssh_transport::prepare(&argv).unwrap_or(argv);
    let askpass = std::env::current_exe()
        .ok()
        .filter(|_| p.preferences.ssh_auth == "password");
    match askpass {
        Some(exe) => [
            "env".to_string(),
            format!("SSH_ASKPASS={}", exe.display()),
            "SSH_ASKPASS_REQUIRE=force".into(),
            format!("{}={}", crate::askpass::REFERENCE, p.secret_reference),
        ]
        .into_iter()
        .chain(argv)
        .collect(),
        None => argv,
    }
}
impl Profile {
    pub fn matches(&self, s: &str) -> bool {
        self.search_text().contains(&s.to_lowercase())
    }
    pub(crate) fn search_text(&self) -> String {
        format!(
            "{} {} {} {} {} {} {}",
            self.name,
            self.group,
            self.server,
            self.preferences.label,
            self.preferences.network,
            self.protocol,
            self.tags.join(" ")
        )
        .to_lowercase()
    }
    pub(crate) fn validate(&self) -> Result<(), String> {
        if self.name.is_empty()
            || self.server.is_empty()
            || !matches!(self.protocol.as_str(), "RDP" | "SSH")
            || [
                &self.name,
                &self.group,
                &self.server,
                &self.username,
                &self.secret_reference,
            ]
            .iter()
            .any(|v| v.len() > 1024 || v.chars().any(char::is_control))
            || self.server.starts_with('-')
            || self.tags.len() > 64
        {
            return Err("Invalid profile metadata".into());
        }
        if !self.preferences.agent_session.is_empty()
            && (self.preferences.agent_session.len() > 64
                || !self
                    .preferences
                    .agent_session
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
                || !self.preferences.agent_session.as_bytes()[0].is_ascii_alphanumeric())
        {
            return Err("Shared session name must start with a letter or digit and contain at most64 letters, digits, dots, underscores or hyphens".into());
        }
        endpoint(&self.server, if self.protocol == "SSH" { 22 } else { 3389 })?;
        if ![
            "",
            "standard",
            "gnome-remote-login",
            "gnome-desktop-sharing",
        ]
        .contains(&self.preferences.rdp_mode.as_str())
            || (!self.preferences.gnome_secret_reference.is_empty()
                && self.preferences.gnome_secret_reference == self.secret_reference)
            || (self.inherit_credential && !self.inherit_connection)
            || !["", "ruwac", "screen", "tmux", "direct"]
                .contains(&self.preferences.agent_backend.as_str())
            || self.preferences.clipboard > 4
            || !["fit", "native", "stretch", "dynamic"].contains(&self.preferences.display.as_str())
            || !self.preferences.zoom.is_finite()
            || !(0.25..=4.0).contains(&self.preferences.zoom)
            || self
                .tags
                .iter()
                .any(|s| s.len() > 1024 || s.chars().any(char::is_control))
            || [
                &self.preferences.codex_command,
                &self.preferences.claude_command,
                &self.preferences.kimi_command,
                &self.preferences.gnome_username,
                &self.preferences.gnome_secret_reference,
                &self.preferences.label,
                &self.preferences.network,
                &self.preferences.gateway,
                &self.preferences.ssh_jump_alias,
                &self.preferences.trust,
                &self.preferences.identity_file,
                &self.preferences.identity_secret_reference,
                &self.preferences.known_hosts,
                &self.id,
                &self.machine_id,
            ]
            .iter()
            .any(|s| s.len() > 1024 || s.chars().any(char::is_control))
        {
            return Err("Invalid endpoint preferences".into());
        }
        crate::primary::validate(&self.preferences)?;
        if !self.preferences.ssh_jump_alias.is_empty() {
            crate::ssh_jump::validate(&self.preferences.ssh_jump_alias)?;
        }
        Ok(())
    }
}
/// Only explicitly reviewed imported metadata changes; local identity and policy survive.
fn merge_metadata(current: &Profile, imported: &Profile) -> Result<Profile, String> {
    imported.validate()?;
    let mut merged = current.clone();
    merged.name = imported.name.clone();
    merged.group = imported.group.clone();
    merged.protocol = imported.protocol.clone();
    merged.server = imported.server.clone();
    merged.username = imported.username.clone();
    merged.tags = imported.tags.clone();
    merged.validate()?;
    Ok(merged)
}
/// Standard native, legacy and Flatpak profile directories; discovery never imports.
fn discover_remmina(home: &Path, data: &Path) -> Vec<(PathBuf, String)> {
    let mut found = Vec::new();
    for directory in [
        data.join("remmina"),
        home.join(".remmina"),
        home.join(".var/app/org.remmina.Remmina/data/remmina"),
    ] {
        let Ok(meta) = std::fs::symlink_metadata(&directory) else {
            continue;
        };
        if !meta.is_dir() || meta.file_type().is_symlink() {
            continue;
        }
        let Ok(entries) = std::fs::read_dir(&directory) else {
            continue;
        };
        for entry in entries.take(1001).flatten() {
            if found.len() >= 1000 {
                break;
            }
            let path = entry.path();
            if path.extension().is_none_or(|v| v != "remmina")
                || entry.file_type().is_ok_and(|v| !v.is_file())
            {
                continue;
            }
            if let Ok(profile) = preview(&path) {
                found.push((
                    path,
                    format!(
                        "{} / {} · {}",
                        profile.group, profile.name, profile.protocol
                    ),
                ));
            }
        }
    }
    found.sort_by(|a, b| a.1.cmp(&b.1).then_with(|| a.0.cmp(&b.0)));
    found.dedup_by(|a, b| a.0 == b.0);
    found
}

/// Import only the selected profiles' saved service passwords, after metadata
/// was committed. Existing RUWAC credentials and GNOME credentials are retained.
fn import_passwords(jobs: Vec<(Profile, PathBuf)>, status: &gtk::Label) {
    use std::sync::atomic::{AtomicBool, Ordering};
    static BUSY: AtomicBool = AtomicBool::new(false);
    if BUSY.swap(true, Ordering::AcqRel) {
        status.set_text("Another password import is running; retry after it finishes");
        return;
    }
    let (tx, rx) = std::sync::mpsc::sync_channel(1);
    std::thread::spawn(move || {
        struct Finish;
        impl Drop for Finish {
            fn drop(&mut self) {
                BUSY.store(false, Ordering::Release);
            }
        }
        let _finish = Finish;
        let began = std::time::Instant::now();
        let (mut copied, mut missing) = (0, 0);
        for (target, path) in jobs {
            if began.elapsed() > std::time::Duration::from_secs(60) {
                missing += 1;
                continue;
            }
            let matching = preview(&path).is_ok_and(|p| {
                p.server == target.server
                    && p.username == target.username
                    && p.protocol == target.protocol
            });
            if !matching {
                missing += 1;
                continue;
            }
            // Unsupported file-password formats fail before touching a keyring.
            let secret = match crate::credentials::lookup_remmina(&path) {
                Ok(secret) => secret,
                Err(_) => {
                    missing += 1;
                    continue;
                }
            };
            if crate::credentials::lookup(&target.secret_reference).is_ok() {
                continue;
            }
            // Recheck the source and destination after the asynchronous lookup.
            let matching = preview(&path).is_ok_and(|p| {
                p.server == target.server
                    && p.username == target.username
                    && p.protocol == target.protocol
            });
            let current = Store::load().items.into_iter().any(|p| {
                p.id == target.id
                    && p.server == target.server
                    && p.username == target.username
                    && p.protocol == target.protocol
                    && p.secret_reference == target.secret_reference
            });
            if !matching || !current {
                missing += 1;
                continue;
            }
            match crate::credentials::set(&target.secret_reference, secret) {
                Ok(()) => copied += 1,
                Err(_) => missing += 1,
            }
        }
        let _ = tx.send((copied, missing));
    });
    let status = status.downgrade();
    gtk::glib::timeout_add_local(std::time::Duration::from_millis(100), move || {
        match rx.try_recv() {
            Ok((copied, missing)) => {
                if let Some(status) = status.upgrade() {
                    status.set_text(&format!("Remmina import complete · {copied} saved passwords copied · {missing} unavailable; set missing passwords in Edit. Existing credentials retained."));
                }
                gtk::glib::ControlFlow::Break
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => gtk::glib::ControlFlow::Continue,
            Err(_) => gtk::glib::ControlFlow::Break,
        }
    });
}

/// This parser deliberately ignores credentials, certificates and key paths.
pub fn preview(path: &Path) -> Result<Profile, String> {
    use std::os::unix::fs::OpenOptionsExt;
    let mut f = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
        .map_err(|_| "Cannot open selected Remmina file")?;
    if !f
        .metadata()
        .map_err(|_| "Cannot inspect selected file")?
        .is_file()
    {
        return Err("Selected path is not a regular file".into());
    }
    let mut s = String::new();
    f.by_ref()
        .take(65537)
        .read_to_string(&mut s)
        .map_err(|_| "Invalid Remmina text")?;
    if s.len() > 65536 {
        return Err("Remmina file exceeds cap".into());
    }
    let mut fields = std::collections::HashMap::new();
    let mut selected = false;
    for line in s.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            selected = line == "[remmina]";
            continue;
        }
        if selected {
            if let Some((k, v)) = line.split_once('=') {
                if matches!(
                    k,
                    "name" | "group" | "protocol" | "server" | "username" | "labels"
                ) {
                    fields.insert(k, v);
                }
            }
        }
    }
    let field = |name| fields.get(name).copied().unwrap_or("").to_string();
    let p = Profile {
        id: String::new(),
        machine_id: String::new(),
        preferences: Preferences::default(),
        inherit: false,
        inherit_credential: false,
        inherit_connection: false,
        name: field("name"),
        group: field("group"),
        protocol: field("protocol"),
        server: field("server"),
        username: field("username"),
        tags: field("labels")
            .split(',')
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .collect(),
        secret_reference: String::new(),
        favorite: false,
        last_used: 0,
    };
    p.validate()?;
    Ok(p)
}
#[derive(Default)]
struct Store {
    items: Vec<Profile>,
    path: Option<PathBuf>,
    error: Option<String>,
    groups: Vec<Group>,
    active: std::collections::HashMap<String, usize>,
    database_revision: Cell<i64>,
    mirror_hash: RefCell<String>,
}
impl Store {
    fn load() -> Self {
        use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt};
        let base = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
            .unwrap_or_default()
            .join("RUWAC");
        if !base.exists() {
            let _ = std::fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(&base);
        }
        let Ok(m) = std::fs::symlink_metadata(&base) else {
            return Self::default();
        };
        if !m.is_dir() || m.uid() != unsafe { libc::geteuid() } || m.mode() & 0o077 != 0 {
            return Self::default();
        }
        let path = base.join("machines.json");
        let mut items = Vec::new();
        let mut error = None;
        let mut groups = Vec::new();
        let mut migrate = false;
        let mut source_hash = String::new();
        match std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(&path)
        {
            Ok(f) => {
                let valid = f.metadata().is_ok_and(|m| {
                    m.is_file()
                        && m.uid() == unsafe { libc::geteuid() }
                        && m.mode() & 0o077 == 0
                        && m.len() <= 1024 * 1024
                });
                let mut b = Vec::new();
                if valid && f.take(1024 * 1024 + 1).read_to_end(&mut b).is_ok() {
                    match parse_library(&b) {
                        Ok(v) => {
                            source_hash = inventory::hash(&b);
                            migrate = serde_json::from_slice::<serde_json::Value>(&b)
                                .is_ok_and(|v| v.is_array());
                            items = v;
                            groups = serde_json::from_slice::<Library>(&b)
                                .map(|l| l.groups)
                                .unwrap_or_default();
                        }
                        Err(e) => error = Some(e),
                    }
                } else {
                    error =
                        Some("Unsafe or unreadable metadata; existing library protected".into());
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
            Err(_) => {
                error = Some("Cannot open private metadata; existing library protected".into())
            }
        }
        let mut store = Self {
            items,
            path: Some(path.clone()),
            error,
            groups,
            active: Default::default(),
            database_revision: Cell::new(0),
            mirror_hash: RefCell::new(String::new()),
        };
        if store.error.is_none() {
            let document = serde_json::to_vec_pretty(&Library {
                schema: 1,
                items: store.items.clone(),
                groups: store.groups.clone(),
            });
            match document
                .map_err(|_| "Invalid inventory".to_string())
                .and_then(|doc| {
                    inventory::load_legacy(
                        &path.with_file_name("machines.sqlite3"),
                        &path,
                        &doc,
                        &source_hash,
                    )
                }) {
                Ok(snapshot) => match serde_json::from_slice::<Library>(&snapshot.document) {
                    Ok(library) => {
                        store.items = library.items;
                        store.groups = library.groups;
                        store.database_revision.set(snapshot.revision);
                        *store.mirror_hash.borrow_mut() = snapshot.mirror_hash;
                        migrate |= snapshot.pending;
                    }
                    Err(_) => {
                        store.error = Some("Invalid SQLite library; database protected".into())
                    }
                },
                Err(e) => store.error = Some(e),
            }
        }
        if store.error.is_some() {
            if let Ok(snapshot) = inventory::read(&path.with_file_name("machines.sqlite3")) {
                if let Ok(library) = serde_json::from_slice::<Library>(&snapshot.document) {
                    store.items = library.items;
                    store.groups = library.groups;
                    store.database_revision.set(snapshot.revision);
                    *store.mirror_hash.borrow_mut() = snapshot.mirror_hash;
                }
            }
        }
        if store.error.is_none() && migrate {
            if let Err(e) = store.save() {
                store.error = Some(e);
            }
        }
        store
    }
    fn save(&self) -> Result<(), String> {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        if let Some(e) = &self.error {
            return Err(e.clone());
        }
        parse_library(
            &serde_json::to_vec(&Library {
                schema: 1,
                items: self.items.clone(),
                groups: self.groups.clone(),
            })
            .map_err(|_| "Invalid library")?,
        )?;
        let path = self
            .path
            .as_ref()
            .ok_or("Private profile storage unavailable")?;
        let tmp = path.with_file_name(format!("machines-{}.tmp", ruwac_protocol::id("save")));
        let result = (|| {
            let mut f = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
                .open(&tmp)
                .map_err(|e| e.to_string())?;
            let bytes = serde_json::to_vec_pretty(&Library {
                schema: 1,
                items: self.items.clone(),
                groups: self.groups.clone(),
            })
            .map_err(|e| e.to_string())?;
            if bytes.len() > 1024 * 1024 {
                return Err("Profile storage exceeds cap".into());
            }
            let expected = self.mirror_hash.borrow().clone();
            if inventory::legacy_hash(path)? != expected {
                return Err("Legacy library changed; refresh before saving".into());
            }
            let database = path.with_file_name("machines.sqlite3");
            let revision =
                inventory::save(&database, &bytes, self.database_revision.get(), &expected)?;
            self.database_revision.set(revision);
            f.write_all(&bytes).map_err(|e| e.to_string())?;
            f.sync_all().map_err(|e| e.to_string())?;
            if path.exists() {
                let old = std::fs::OpenOptions::new()
                    .read(true)
                    .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
                    .open(path)
                    .map_err(|_| "Cannot back up metadata")?;
                let mut bytes = Vec::new();
                old.take(1024 * 1024 + 1)
                    .read_to_end(&mut bytes)
                    .map_err(|_| "Cannot back up metadata")?;
                let backup = path.with_file_name("machines.previous.json");
                let backup_tmp = tmp.with_extension("backup");
                let mut out = std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .mode(0o600)
                    .custom_flags(libc::O_NOFOLLOW)
                    .open(&backup_tmp)
                    .map_err(|_| "Cannot create metadata backup")?;
                out.write_all(&bytes)
                    .map_err(|_| "Cannot write metadata backup")?;
                out.sync_all().map_err(|_| "Cannot sync metadata backup")?;
                std::fs::rename(&backup_tmp, backup)
                    .map_err(|_| "Cannot commit metadata backup")?;
            }
            if inventory::legacy_hash(path)? != expected {
                return Err(
                    "Legacy library changed during publication; SQLite revision preserved".into(),
                );
            }
            std::fs::rename(&tmp, path).map_err(|e| e.to_string())?;
            std::fs::File::open(path.parent().unwrap())
                .and_then(|d| d.sync_all())
                .map_err(|_| "Cannot sync metadata directory".to_string())?;
            let observed = inventory::hash(&bytes);
            inventory::mirrored(&database, revision, &observed)?;
            *self.mirror_hash.borrow_mut() = observed;
            Ok(())
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(tmp);
        }
        result
    }
}
fn parse_library(b: &[u8]) -> Result<Vec<Profile>, String> {
    let value: serde_json::Value =
        serde_json::from_slice(b).map_err(|_| "Malformed metadata; existing library protected")?;
    let legacy = value.is_array();
    let mut items = if legacy {
        serde_json::from_value::<Vec<Profile>>(value).map_err(|_| "Invalid legacy metadata")?
    } else {
        let lib: Library = serde_json::from_value(value).map_err(|error| {
            if error.to_string().contains("unknown field") {
                "These machine settings need a newer RUWAC window. Reopen the installed application; saved connections were preserved."
            } else {
                "Invalid machine settings; saved connections were preserved"
            }
        })?;
        if lib.groups.len() > 1000
            || lib.groups.iter().any(|g| {
                g.id.is_empty()
                    || g.path.is_empty()
                    || g.path.len() > 1024
                    || g.path.chars().any(char::is_control)
                    || [
                        &g.id,
                        &g.username,
                        &g.credential_profile,
                        &g.defaults.network,
                        &g.defaults.gateway,
                        &g.defaults.ssh_jump_alias,
                        &g.defaults.identity_file,
                        &g.defaults.identity_secret_reference,
                        &g.defaults.known_hosts,
                        &g.defaults.trust,
                    ]
                    .iter()
                    .any(|v| v.len() > 1024 || v.chars().any(char::is_control))
                    || (!g.defaults.ssh_jump_alias.is_empty()
                        && crate::ssh_jump::validate(&g.defaults.ssh_jump_alias).is_err())
                    || g.defaults.clipboard > 4
                    || !["fit", "native", "stretch", "dynamic"]
                        .contains(&g.defaults.display.as_str())
                    || !g.defaults.zoom.is_finite()
                    || !(0.25..=4.0).contains(&g.defaults.zoom)
            })
        {
            return Err("Invalid network groups".into());
        }
        let mut ids = std::collections::HashSet::new();
        let mut paths = std::collections::HashSet::new();
        if lib
            .groups
            .iter()
            .any(|g| !ids.insert(g.id.clone()) || !paths.insert(g.path.clone()))
        {
            return Err("Conflicting network groups".into());
        }
        if lib.schema != 1 {
            return Err("Unsupported metadata schema; existing library protected".into());
        }
        lib.items
    };
    if items.len() > 1000 {
        return Err("Profile limit exceeded".into());
    }
    let mut ids = std::collections::HashSet::new();
    for p in &mut items {
        p.validate()?;
        if !legacy && (p.id.is_empty() || p.machine_id.is_empty()) {
            return Err("Missing stable endpoint identity".into());
        }
        p.identify();
        if !ids.insert(p.id.clone()) {
            return Err("Conflicting endpoint identities".into());
        }
    }
    let first: std::collections::HashSet<_> =
        items.iter().map(|p| p.secret_reference.as_str()).collect();
    if items.iter().any(|p| {
        !p.preferences.gnome_secret_reference.is_empty()
            && first.contains(p.preferences.gnome_secret_reference.as_str())
    }) {
        return Err(
            "GNOME and RDP service credential references must be separate across the library"
                .into(),
        );
    }
    Ok(items)
}
fn resolve(profile: &Profile, groups: &[Group], accounts: &[Profile]) -> Result<Profile, String> {
    let mut p = profile.clone();
    {
        let mut chain: Vec<_> = groups
            .iter()
            .filter(|g| p.group == g.path || p.group.starts_with(&(g.path.clone() + "/")))
            .collect();
        chain.sort_by_key(|g| g.path.len());
        if let Some(g) = chain.last().filter(|_| p.inherit) {
            p.preferences.display = g.defaults.display.clone();
            p.preferences.clipboard = g.defaults.clipboard;
        }
        if p.inherit_connection {
            let nearest = |field: fn(&Group) -> &String| {
                chain
                    .iter()
                    .rev()
                    .map(|g| field(g))
                    .find(|v| !v.is_empty())
                    .cloned()
            };
            if p.username.is_empty() {
                p.username = nearest(|g| &g.username).unwrap_or_default();
            }
            if p.preferences.network.is_empty() {
                p.preferences.network = nearest(|g| &g.defaults.network).unwrap_or_default();
            }
            if p.preferences.gateway.is_empty() {
                p.preferences.gateway = nearest(|g| &g.defaults.gateway).unwrap_or_default();
            }
            if p.protocol == "SSH" && p.preferences.ssh_jump_alias.is_empty() {
                p.preferences.ssh_jump_alias =
                    nearest(|g| &g.defaults.ssh_jump_alias).unwrap_or_default();
            }
            if p.preferences.known_hosts.is_empty() {
                p.preferences.known_hosts =
                    nearest(|g| &g.defaults.known_hosts).unwrap_or_default();
            }
            if p.preferences.trust.is_empty() {
                p.preferences.trust = nearest(|g| &g.defaults.trust).unwrap_or_default();
            }
            if p.preferences.identity_file.is_empty() {
                if let Some(g) = chain
                    .iter()
                    .rev()
                    .find(|g| !g.defaults.identity_file.is_empty())
                {
                    p.preferences.identity_file = g.defaults.identity_file.clone();
                    p.preferences.identity_secret_reference =
                        g.defaults.identity_secret_reference.clone();
                }
            }
            if p.inherit_credential {
                let account = nearest(|g| &g.credential_profile)
                    .ok_or("Select a group credential profile before connecting")?;
                let account = accounts
                    .iter()
                    .find(|a| a.id == account)
                    .ok_or("Group credential profile was removed; select another account")?;
                if account.secret_reference.is_empty() {
                    return Err("Group credential profile has no credential reference".into());
                }
                p.secret_reference = account.secret_reference.clone();
                if p.username.is_empty() {
                    p.username = account.username.clone();
                }
            }
        } else if p.inherit_credential {
            return Err("Group credentials require connection inheritance".into());
        }
    }
    if p.protocol == "SSH" && p.username.is_empty() {
        p.username = gtk::glib::user_name()
            .into_string()
            .map_err(|_| "Local account name is not UTF-8")?;
    }
    p.validate()?;
    Ok(p)
}

pub struct ConnectionLease {
    store: Rc<RefCell<Store>>,
    id: String,
    released: Cell<bool>,
}
impl ConnectionLease {
    pub fn trust_sink(&self) -> TrustSink {
        let (store, id) = (self.store.clone(), self.id.clone());
        Rc::new(move |server, previous, pin| {
            let (host, port) = endpoint(server, 3389)?;
            if !crate::rdp_trust::Trust::parse(Some(pin), &host, port)?.is_pinned() {
                return Err("Certificate approval requires an endpoint-bound fingerprint".into());
            }
            // Merge only this trust field into fresh disk metadata, preserving
            // changes made by another visible owner window since the tab opened.
            let mut fresh = Store::load();
            if fresh.error.is_some() || fresh.path != store.borrow().path {
                return Err(
                    "Profile storage changed or is unavailable; trust was not saved".into(),
                );
            }
            let effective = fresh
                .items
                .iter()
                .find(|p| p.id == id)
                .ok_or("Endpoint removed; certificate trust was not saved")?;
            let previous_effective = resolve(effective, &fresh.groups, &fresh.items)?
                .preferences
                .trust;
            let p = fresh
                .items
                .iter_mut()
                .find(|p| p.id == id)
                .ok_or("Endpoint removed; certificate trust was not saved")?;
            if p.protocol != "RDP" || p.server != server || previous_effective != previous {
                return Err(
                    "Endpoint or certificate trust changed; reopen the profile before saving trust"
                        .into(),
                );
            }
            p.preferences.trust = pin.into();
            fresh.save()?;
            let mut cached = store.borrow_mut();
            cached.items = fresh.items;
            cached.groups = fresh.groups;
            cached.database_revision.set(fresh.database_revision.get());
            *cached.mirror_hash.borrow_mut() = fresh.mirror_hash.borrow().clone();
            cached.error = None;
            Ok(())
        })
    }
    pub fn fork(&self) -> Self {
        *self
            .store
            .borrow_mut()
            .active
            .entry(self.id.clone())
            .or_default() += 1;
        Self {
            store: self.store.clone(),
            id: self.id.clone(),
            released: Cell::new(false),
        }
    }
    fn release(&self) {
        if self.released.replace(true) {
            return;
        }
        let mut s = self.store.borrow_mut();
        if let Some(n) = s.active.get_mut(&self.id) {
            *n -= 1;
            if *n == 0 {
                s.active.remove(&self.id);
            }
        }
    }
    pub fn bind(self, widget: &impl IsA<gtk::Widget>) {
        let lease = Rc::new(self);
        widget.connect_unrealize(move |w| {
            let (weak, lease) = (w.downgrade(), lease.clone());
            gtk::glib::idle_add_local_once(move || {
                if weak.upgrade().is_none_or(|w| w.root().is_none()) {
                    lease.release();
                }
            });
        });
    }
}
impl Drop for ConnectionLease {
    fn drop(&mut self) {
        self.release();
    }
}
pub type TrustSink = Rc<dyn Fn(&str, &str, &str) -> Result<(), String>>;
pub type PreferenceSink = Rc<dyn Fn(&str, f64, bool, u32)>;
pub type ImageProfileReader = Rc<dyn Fn(&str) -> Result<Profile, String>>;
pub type InventoryReader = Rc<dyn Fn() -> Result<Vec<serde_json::Value>, ruwac_protocol::Error>>;
pub type ProfileRestorer = Rc<dyn Fn(&str) -> Option<(Profile, PreferenceSink, ConnectionLease)>>;
fn acquire(
    s: &Rc<RefCell<Store>>,
    st: &gtk::Label,
    id: &str,
) -> Option<(Profile, PreferenceSink, ConnectionLease)> {
    let store_handle = s.clone();
    let mut s = s.borrow_mut();
    let i = s.items.iter().position(|p| p.id == id)?;
    s.items[i].last_used = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let p = match resolve(&s.items[i], &s.groups, &s.items) {
        Ok(p) => p,
        Err(e) => {
            st.set_text(&e);
            return None;
        }
    };
    *s.active.entry(p.id.clone()).or_default() += 1;
    if let Err(e) = s.save() {
        st.set_text(&e);
    }
    drop(s);
    let (store, status, id) = (store_handle, st.clone(), p.id.clone());
    let lease = ConnectionLease {
        store: store.clone(),
        id: id.clone(),
        released: Cell::new(false),
    };
    let sink: PreferenceSink = Rc::new(move |mode, zoom, full, policy| {
        let mut s = store.borrow_mut();
        let old = s.items.clone();
        if let Some(p) = s.items.iter_mut().find(|p| p.id == id) {
            p.preferences.display = mode.into();
            p.preferences.zoom = zoom;
            p.preferences.fullscreen = full;
            p.preferences.clipboard = policy;
            p.inherit = false;
        } else {
            status.set_text("Endpoint removed; connection preferences not saved");
            return;
        }
        if let Err(e) = s.save() {
            s.items = old;
            status.set_text(&e);
        } else {
            status.set_text("Connection preferences saved");
        }
    });

    Some((p, sink, lease))
}
fn bulk_import_plan(existing: &[Profile], incoming: &[Profile]) -> Result<Vec<Profile>, String> {
    let mut result = existing.to_vec();
    for profile in incoming {
        if result.iter().any(|p| {
            p.group == profile.group
                && p.name == profile.name
                && p.server == profile.server
                && p.protocol == profile.protocol
        }) {
            continue;
        }
        if result.len() >= 1000 {
            return Err("Import exceeds the 1000-profile limit; no profiles imported".into());
        }
        let mut profile = profile.clone();
        profile.identify();
        result.push(profile);
    }
    Ok(result)
}
pub type AgentLauncher = Rc<dyn Fn(Profile, u32)>;
pub type ShortcutMenus = Rc<dyn Fn(bool) -> gtk::Box>;
type ConnectionOpener = Rc<dyn Fn(Profile, PreferenceSink, ConnectionLease)>;

fn ssh_seed(source: &Profile) -> Profile {
    let mut p = source.clone();
    p.id = ruwac_protocol::id("endpoint");
    p.protocol = "SSH".into();
    p.server = endpoint(&source.server, 3389)
        .map(|(host, _)| {
            if host.contains(':') {
                format!("[{host}]")
            } else {
                host
            }
        })
        .unwrap_or_default();
    p.username = gtk::glib::user_name().to_string_lossy().into_owned();
    p.secret_reference = p.id.clone();
    p.inherit = false;
    p.inherit_connection = false;
    p.inherit_credential = false;
    p.last_used = 0;
    p.preferences = Preferences {
        codex_command: source.preferences.codex_command.clone(),
        claude_command: source.preferences.claude_command.clone(),
        kimi_command: source.preferences.kimi_command.clone(),
        agent_install_missing: source.preferences.agent_install_missing,
        ruwac_install_missing: source.preferences.ruwac_install_missing,
        agent_backend: source.preferences.agent_backend.clone(),
        agent_session: source.preferences.agent_session.clone(),
        identity_file: source.preferences.identity_file.clone(),
        identity_secret_reference: source.preferences.identity_secret_reference.clone(),
        known_hosts: source.preferences.known_hosts.clone(),
        ssh_jump_alias: source.preferences.ssh_jump_alias.clone(),
        ..Preferences::default()
    };
    p
}

/// A reviewed source and its effective connection. Never edit the source when
/// checking access or when adding a separate endpoint.
#[derive(Clone)]
struct SshReviewTarget {
    source_id: String,
    probe: Profile,
    new: bool,
}

fn ssh_review_snapshot(s: &Store) -> Vec<u8> {
    serde_json::to_vec(&(&s.items, &s.groups)).unwrap_or_default()
}

/// No OpenSSH command runs while constructing this list.
fn ssh_review_targets(s: &Store) -> Vec<SshReviewTarget> {
    let mut targets = Vec::new();
    for source in &s.items {
        let new = source.protocol != "SSH";
        if new
            && (source.protocol != "RDP"
                || s.items
                    .iter()
                    .any(|p| p.machine_id == source.machine_id && p.protocol == "SSH")
                || targets
                    .iter()
                    .any(|t: &SshReviewTarget| t.new && t.probe.machine_id == source.machine_id))
        {
            continue;
        }
        let Ok(effective) = resolve(source, &s.groups, &s.items) else {
            continue;
        };
        let probe = if new {
            let mut seed = ssh_seed(&effective);
            // RDP service credentials are not Linux SSH credentials.
            seed.username.clear();
            seed
        } else {
            effective
        };
        if !probe.server.is_empty() {
            targets.push(SshReviewTarget {
                source_id: source.id.clone(),
                probe,
                new,
            });
        }
    }
    targets
}

fn ssh_access_review(
    button: &gtk::Button,
    store: &Rc<RefCell<Store>>,
    status: &gtk::Label,
    refresh: Rc<dyn Fn()>,
) {
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    use std::time::{Duration, Instant};
    let targets = Rc::new(ssh_review_targets(&store.borrow()));
    let snapshot = Rc::new(ssh_review_snapshot(&store.borrow()));
    let allowed = Arc::new(AtomicBool::new(true));
    let dialog = gtk::Window::builder()
        .title("SSH key access")
        .modal(true)
        .default_width(760)
        .default_height(520)
        .build();
    dialog.set_transient_for(button.root().and_downcast::<gtk::Window>().as_ref());
    let cancel = allowed.clone();
    dialog.connect_unrealize(move |_| {
        cancel.store(false, Ordering::Release);
    });
    let body = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(10)
        .margin_top(16)
        .margin_bottom(16)
        .margin_start(16)
        .margin_end(16)
        .build();
    let intro = gtk::Label::new(Some(&format!(
        "{} eligible saved endpoints. Check explicitly using existing SSH configuration and keys. \
         Public-key authentication only; strict existing host trust, no password, no agent forwarding or shared connection. \
         Checks do not change remote hosts or saved machines. Select verified additions, then review and confirm.",
        targets.len()
    )));
    intro.set_wrap(true);
    intro.set_xalign(0.0);
    body.append(&intro);
    let grid = gtk::Grid::builder()
        .column_spacing(12)
        .row_spacing(6)
        .build();
    grid.set_widget_name("ssh-access-table");
    for (col, heading) in ["Add SSH", "Machine / saved account", "Result"]
        .iter()
        .enumerate()
    {
        let l = gtk::Label::new(Some(heading));
        l.add_css_class("heading");
        l.set_xalign(0.0);
        grid.attach(&l, col as i32, 0, 1, 1);
    }
    let mut rows = Vec::new();
    for (i, target) in targets.iter().enumerate() {
        let pick = gtk::CheckButton::new();
        pick.set_widget_name(&format!("ssh-access-pick-{i}"));
        pick.set_sensitive(false);
        pick.set_visible(target.new);
        let p = &target.probe;
        let label = gtk::Label::new(Some(&format!(
            "{} · {}{}",
            p.name,
            if p.username.is_empty() {
                String::new()
            } else {
                format!("{}@", p.username)
            },
            p.server
        )));
        label.set_xalign(0.0);
        let result = gtk::Label::new(Some("Not checked"));
        result.set_widget_name(&format!("ssh-access-result-{i}"));
        result.set_xalign(0.0);
        result.set_wrap(true);
        result.set_hexpand(true);
        grid.attach(&pick, 0, i as i32 + 1, 1, 1);
        grid.attach(&label, 1, i as i32 + 1, 1, 1);
        grid.attach(&result, 2, i as i32 + 1, 1, 1);
        rows.push((pick, result));
    }
    let rows = Rc::new(rows);
    body.append(
        &gtk::ScrolledWindow::builder()
            .child(&grid)
            .vexpand(true)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .build(),
    );
    let actions = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    actions.set_halign(gtk::Align::End);
    let close = gtk::Button::with_label("Close");
    let check = gtk::Button::with_label("Check now");
    check.set_widget_name("ssh-access-check");
    check.add_css_class("suggested-action");
    let add = gtk::Button::with_label("Review selected SSH endpoints…");
    add.set_widget_name("ssh-access-add");
    add.set_sensitive(false);
    actions.append(&close);
    actions.append(&add);
    actions.append(&check);
    body.append(&actions);
    dialog.set_child(Some(&body));
    let d = dialog.downgrade();
    close.connect_clicked(move |_| {
        if let Some(d) = d.upgrade() {
            d.close();
        }
    });
    let results = Rc::new(RefCell::new(std::collections::HashMap::<
        usize,
        crate::ssh_probe::Checked,
    >::new()));
    for (pick, _) in rows.iter() {
        let add = add.downgrade();
        let rows = Rc::downgrade(&rows);
        pick.connect_toggled(move |_| {
            if let (Some(add), Some(rows)) = (add.upgrade(), rows.upgrade()) {
                add.set_sensitive(
                    rows.iter()
                        .any(|(p, _)| p.is_active() && p.is_sensitive() && p.is_visible()),
                );
            }
        });
    }
    {
        let (targets, rows, add, allowed, results) = (
            targets.clone(),
            rows.clone(),
            add.clone(),
            allowed.clone(),
            results.clone(),
        );
        let weak = dialog.downgrade();
        check.connect_clicked(move |check| {
            check.set_sensitive(false);
            add.set_sensitive(false);
            results.borrow_mut().clear();
            for (pick, label) in rows.iter() {
                pick.set_active(false);
                pick.set_sensitive(false);
                label.set_text("Waiting…");
            }
            let mut cache = crate::ssh_probe::load();
            let now = crate::ssh_probe::now();
            let mut work: Vec<_> = targets.iter().cloned().enumerate().collect();
            // A bounded batch may not reach every endpoint. Prioritize never
            // checked/oldest rows on the next explicit Check, including after
            // reopening, rather than repeatedly checking the same tail.
            work.sort_by_key(|(_, target)| {
                cache
                    .get(&target.source_id)
                    .filter(|c| c.checked <= now)
                    .map_or(0, |c| c.checked)
            });
            let queue = Arc::new(std::sync::Mutex::new(std::collections::VecDeque::from(
                work,
            )));
            let (tx, rx) = std::sync::mpsc::sync_channel(16);
            let cancel = allowed.clone();
            std::thread::spawn(move || {
                let deadline = Instant::now() + Duration::from_secs(120);
                let keys = Arc::new(crate::ssh_probe::identities_cancel(&cancel));
                std::thread::scope(|scope| {
                    for _ in 0..4 {
                        let (queue, tx, keys, cancel) =
                            (queue.clone(), tx.clone(), keys.clone(), cancel.clone());
                        scope.spawn(move || {
                            loop {
                                if !cancel.load(Ordering::Acquire) || Instant::now() >= deadline {
                                    break;
                                }
                                let Some((i, target)) = queue.lock().unwrap().pop_front() else {
                                    break;
                                };
                                let before =
                                    crate::ssh_probe::inspect(&target.probe, &keys, &cancel);
                                let mut outcome =
                                    crate::ssh_probe::probe_cancel(&target.probe, &keys, &cancel);
                                let after =
                                    crate::ssh_probe::inspect(&target.probe, &keys, &cancel);
                                if before.is_none() || before != after {
                                    outcome.access = crate::ssh_probe::Access::NeedsInteraction;
                                    outcome.accepted_key = None;
                                    outcome.reason = Some(
                                        "Connection, key or trust inputs changed; check again"
                                            .into(),
                                    );
                                }
                                let (inputs, resolved) =
                                    after.map_or((String::new(), None), |(h, t)| (h, Some(t)));
                                let checked = crate::ssh_probe::Checked {
                                    access: outcome.access,
                                    accepted_key: outcome.accepted_key.clone(),
                                    checked: crate::ssh_probe::now(),
                                    inputs,
                                    source_id: target.source_id,
                                    target: resolved,
                                    reason: outcome.reason.clone(),
                                };
                                if tx.send((i, outcome.describe(), checked)).is_err() {
                                    break;
                                }
                            }
                        });
                    }
                });
            });
            let (rows, results, cancel, weak, check) = (
                rows.clone(),
                results.clone(),
                allowed.clone(),
                weak.clone(),
                check.downgrade(),
            );
            gtk::glib::timeout_add_local(Duration::from_millis(100), move || {
                if !cancel.load(Ordering::Acquire) || weak.upgrade().is_none() {
                    return gtk::glib::ControlFlow::Break;
                }
                loop {
                    match rx.try_recv() {
                        Ok((i, description, checked)) => {
                            let (pick, result) = &rows[i];
                            result.set_text(&description);
                            pick.set_sensitive(
                                pick.is_visible()
                                    && checked.access == crate::ssh_probe::Access::Verified,
                            );
                            // Successful checks never preselect an addition.
                            cache.insert(checked.source_id.clone(), checked.clone());
                            results.borrow_mut().insert(i, checked);
                        }
                        Err(std::sync::mpsc::TryRecvError::Empty) => {
                            return gtk::glib::ControlFlow::Continue;
                        }
                        Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                            for (_, label) in rows.iter() {
                                if label.text() == "Waiting…" {
                                    label.set_text("Not run — batch time limit");
                                }
                            }
                            if let Some(check) = check.upgrade() {
                                check.set_sensitive(true);
                            }
                            if let Err(e) = crate::ssh_probe::save(&cache) {
                                if let Some(d) = weak.upgrade() {
                                    d.set_title(Some(&format!(
                                        "SSH key access · result cache unavailable: {e}"
                                    )));
                                }
                            }
                            return gtk::glib::ControlFlow::Break;
                        }
                    }
                }
            });
        });
    }
    let (store, status, d) = (store.clone(), status.clone(), dialog.downgrade());
    add.connect_clicked(move |add| {
        let selected: Vec<_> = targets.iter().zip(rows.iter()).enumerate()
            .filter(|(_, (t, (p, _)))| t.new && p.is_active() && p.is_sensitive())
            .filter_map(|(i, (t, _))| results.borrow().get(&i).cloned().map(|c| (t.clone(), c))).collect();
        if selected.is_empty() { return; }
        let Some(parent) = d.upgrade() else { return; };
        let confirm = gtk::Window::builder().title("Confirm SSH endpoint additions").modal(true)
            .transient_for(&parent).default_width(600).build();
        let content = gtk::Box::builder().orientation(gtk::Orientation::Vertical).spacing(12)
            .margin_top(16).margin_bottom(16).margin_start(16).margin_end(16).build();
        let description = selected.iter().map(|(t,c)| {
            let dest = c.target.as_ref().map(|v| format!("{}@{}:{} via {:?}",v.account,v.host,v.port,v.route)).unwrap_or_default();
            format!("{} · {dest}\n{}", t.probe.name, c.accepted_key.as_deref().unwrap_or("Exact key unknown"))
        }).collect::<Vec<_>>().join("\n\n");
        let label = gtk::Label::new(Some(&format!("Add only these {} SSH endpoints? Existing endpoints and credentials are preserved.\n\n{description}",selected.len())));
        label.set_wrap(true); label.set_xalign(0.0); content.append(&label);
        let controls = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let no = gtk::Button::with_label("Cancel");
        let yes = gtk::Button::with_label("Confirm additions"); yes.set_widget_name("ssh-access-confirm");
        controls.append(&no); controls.append(&yes); content.append(&controls); confirm.set_child(Some(&content));
        let pending = Arc::new(AtomicBool::new(true));
        let revoke = pending.clone(); confirm.connect_unrealize(move |_| { revoke.store(false, Ordering::Release); });
        let c = confirm.downgrade(); no.connect_clicked(move |_| { if let Some(c) = c.upgrade() { c.close(); } });
        let (s, st, original, refresh, parent, c, allowed, add) = (store.clone(), status.clone(), snapshot.clone(), refresh.clone(), d.clone(), confirm.downgrade(), allowed.clone(), add.downgrade());
        yes.connect_clicked(move |yes| {
            yes.set_sensitive(false);
            let (tx, rx) = std::sync::mpsc::sync_channel(1);
            let (selected, cancel, owner) = (selected.clone(), pending.clone(), allowed.clone());
            std::thread::spawn(move || {
                let keys = crate::ssh_probe::identities_cancel(&cancel);
                let valid = selected.iter().all(|(t,c)| owner.load(Ordering::Acquire) && cancel.load(Ordering::Acquire)
                    && c.access == crate::ssh_probe::Access::Verified
                    && crate::ssh_probe::now().checked_sub(c.checked).is_some_and(|age| age <= 300)
                    && crate::ssh_probe::inspect(&t.probe, &keys, &cancel).is_some_and(|(hash,target)|
                        hash == c.inputs && Some(&target) == c.target.as_ref()));
                let _ = tx.send((valid, selected));
            });
            let (s, st, original, refresh, parent, c, allowed, pending, add) = (s.clone(), st.clone(), original.clone(), refresh.clone(), parent.clone(), c.clone(), allowed.clone(), pending.clone(), add.clone());
            gtk::glib::timeout_add_local(Duration::from_millis(100), move || {
                if !allowed.load(Ordering::Acquire) || !pending.load(Ordering::Acquire) { return gtk::glib::ControlFlow::Break; }
                let (valid, selected) = match rx.try_recv() {
                    Ok(v) => v,
                    Err(std::sync::mpsc::TryRecvError::Empty) => return gtk::glib::ControlFlow::Continue,
                    Err(_) => { st.set_text("SSH validation stopped; nothing added"); return gtk::glib::ControlFlow::Break; }
                };
                let mut s = s.borrow_mut();
                if !valid || ssh_review_snapshot(&s) != *original || s.error.is_some()
                    || s.items.len() + selected.len() > 1000 || selected.iter().any(|(t,_)|
                        s.items.iter().any(|p| p.machine_id == t.probe.machine_id && p.protocol == "SSH")) {
                    st.set_text("SSH check is stale or inventory changed; nothing added. Close and check again.");
                    if let Some(add) = add.upgrade() { add.set_sensitive(false); }
                } else {
                    let old = s.items.clone();
                    s.items.extend(selected.iter().map(|(t,_)| t.probe.clone()));
                    match s.save() {
                        Ok(()) => { st.set_text(&format!("Added {} reviewed SSH endpoints",selected.len())); drop(s); refresh(); if let Some(p)=parent.upgrade(){p.close();} }
                        Err(e) => { s.items=old; st.set_text(&e); }
                    }
                }
                if let Some(c)=c.upgrade(){c.close();}
                gtk::glib::ControlFlow::Break
            });
        });
        confirm.present();
    });
    dialog.present();
}

/// Show in a password box whether a secret is already saved, so nothing has
/// to be typed again to find out. The secret itself is discarded at once.
fn credential_hint(entry: &gtk::PasswordEntry, reference: String, what: &'static str) {
    if reference.trim().is_empty() {
        entry.set_placeholder_text(Some(&format!("No {what} saved — type one, then Set")));
        return;
    }
    let (tx, rx) = std::sync::mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let saved = crate::credentials::lookup(&reference).is_ok();
        let _ = tx.send(saved);
    });
    let entry = entry.downgrade();
    gtk::glib::timeout_add_local(std::time::Duration::from_millis(50), move || {
        let Some(entry) = entry.upgrade() else {
            return gtk::glib::ControlFlow::Break;
        };
        match rx.try_recv() {
            Ok(saved) => {
                entry.set_placeholder_text(Some(&if saved {
                    format!("●●●●●● {what} saved — leave empty to keep, type to replace")
                } else {
                    format!("No {what} saved (or keyring locked) — type one, then Set")
                }));
                entry.set_tooltip_text(Some(if saved {
                    "Saved in Secret Service"
                } else {
                    "Nothing saved for this reference"
                }));
                gtk::glib::ControlFlow::Break
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => gtk::glib::ControlFlow::Continue,
            Err(_) => gtk::glib::ControlFlow::Break,
        }
    });
}

fn shortcut_action(
    button: &gtk::Button,
    store: &Rc<RefCell<Store>>,
    status: &gtk::Label,
    open: &ConnectionOpener,
    launch: &AgentLauncher,
    id: &str,
    kind: u32,
) {
    let target = {
        let s = store.borrow();
        let Some(p) = s.items.iter().find(|p| p.id == id) else {
            status.set_text("Saved endpoint no longer exists");
            return;
        };
        if p.protocol == "SSH" {
            Some(p.clone())
        } else {
            s.items
                .iter()
                .find(|x| x.machine_id == p.machine_id && x.protocol == "SSH")
                .cloned()
        }
    };
    if let Some(target) = target {
        if let Some((mut p, sink, lease)) = acquire(store, status, &target.id) {
            // Commands enabled on the machine's RDP bookmark also apply to its
            // explicitly linked SSH endpoint; SSH login and trust stay separate.
            let machine = store
                .borrow()
                .items
                .iter()
                .filter(|x| x.machine_id == p.machine_id)
                .cloned()
                .collect::<Vec<_>>();
            if p.preferences.codex_command.is_empty() {
                p.preferences.codex_command = machine
                    .iter()
                    .find(|x| !x.preferences.codex_command.is_empty())
                    .map(|x| x.preferences.codex_command.clone())
                    .unwrap_or_default();
            }
            if p.preferences.claude_command.is_empty() {
                p.preferences.claude_command = machine
                    .iter()
                    .find(|x| !x.preferences.claude_command.is_empty())
                    .map(|x| x.preferences.claude_command.clone())
                    .unwrap_or_default();
            }
            if p.preferences.agent_session.is_empty() {
                p.preferences.agent_session = machine
                    .iter()
                    .find(|x| !x.preferences.agent_session.is_empty())
                    .map(|x| x.preferences.agent_session.clone())
                    .unwrap_or_default();
            }
            if p.preferences.kimi_command.is_empty() {
                p.preferences.kimi_command = machine
                    .iter()
                    .find(|x| !x.preferences.kimi_command.is_empty())
                    .map(|x| x.preferences.kimi_command.clone())
                    .unwrap_or_default();
            }
            p.preferences.agent_install_missing |=
                machine.iter().any(|x| x.preferences.agent_install_missing);
            p.preferences.ruwac_install_missing |=
                machine.iter().any(|x| x.preferences.ruwac_install_missing);
            if kind == 3 || kind == 6 {
                FILES_REQUEST.with(|f| f.set(kind == 6));
                open(p, sink, lease);
            } else {
                launch(p, kind);
            }
        }
    } else {
        let seed = store
            .borrow()
            .items
            .iter()
            .find(|p| p.id == id)
            .map(ssh_seed);
        if let Some(mut seed) = seed {
            seed.username.clear();
            editor_seed(
                button,
                store.clone(),
                None,
                status.clone(),
                open.clone(),
                launch.clone(),
                Some(seed),
            );
        }
    }
}
thread_local! {
    /// The next SSH open shows the SFTP file browser instead of a terminal.
    static FILES_REQUEST: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}
pub fn take_files_request() -> bool {
    FILES_REQUEST.with(|f| f.replace(false))
}

fn shortcut_button(
    icon: &str,
    tip: &str,
    name: &str,
    id: &str,
    kind: u32,
    store: Rc<RefCell<Store>>,
    status: gtk::Label,
    open: ConnectionOpener,
    launch: AgentLauncher,
) -> gtk::Button {
    let b = if icon == "ruwac-codex" {
        // RUWAC's own recognisable shortcut glyph, not a vendor trademark.
        let icon = gtk::gio::BytesIcon::new(&gtk::glib::Bytes::from_static(include_bytes!(
            "../../packaging/codex-shortcut-symbolic.svg"
        )));
        let image = gtk::Image::from_gicon(&icon);
        image.set_pixel_size(19);
        gtk::Button::builder().child(&image).build()
    } else {
        gtk::Button::from_icon_name(icon)
    };
    b.add_css_class("flat");
    b.set_widget_name(name);
    b.set_tooltip_text(Some(tip));
    b.update_property(&[gtk::accessible::Property::Label(tip)]);
    let id = id.to_string();
    b.connect_clicked(move |b| {
        shortcut_action(b, &store, &status, &open, &launch, &id, kind);
        if let Some(pop) = b
            .ancestor(gtk::Popover::static_type())
            .and_then(|w| w.downcast::<gtk::Popover>().ok())
        {
            pop.popdown();
        }
    });
    b
}

fn shortcut_menu(
    store: &Rc<RefCell<Store>>,
    status: &gtk::Label,
    open: &ConnectionOpener,
    launch: &AgentLauncher,
    agents_only: bool,
) -> gtk::Box {
    let body = gtk::Box::new(gtk::Orientation::Vertical, 6);
    body.set_widget_name("host-shortcuts-list");
    let items = store.borrow().items.clone();
    let mut saved = items
        .iter()
        .filter(|p| {
            // Terminal menus list machines that can open SSH directly.
            (!agents_only
                && items
                    .iter()
                    .any(|x| x.machine_id == p.machine_id && x.protocol == "SSH"))
                || p.protocol == "SSH"
                || !p.preferences.codex_command.is_empty()
                || !p.preferences.claude_command.is_empty()
                || !p.preferences.kimi_command.is_empty()
        })
        .cloned()
        .collect::<Vec<_>>();
    saved.sort_by_key(|p| {
        (
            !p.favorite,
            p.group.clone(),
            p.name.clone(),
            p.protocol != "SSH",
        )
    });
    let mut seen = std::collections::HashSet::new();
    saved.retain(|p| seen.insert(p.machine_id.clone()));
    let mut recent = items
        .iter()
        .filter(|p| p.protocol == "SSH" && p.last_used > 0)
        .cloned()
        .collect::<Vec<_>>();
    recent.sort_by_key(|p| std::cmp::Reverse(p.last_used));
    recent.truncate(8);
    let sections = if agents_only {
        vec![("Saved agent hosts", saved)]
    } else {
        vec![
            ("Recent SSH connections", recent),
            ("Saved SSH hosts", saved),
        ]
    };
    for (heading, rows) in sections {
        let label = gtk::Label::new(Some(heading));
        label.set_xalign(0.0);
        label.add_css_class("heading");
        body.append(&label);
        if rows.is_empty() {
            let l = gtk::Label::new(Some("No saved connections yet"));
            l.add_css_class("dim-label");
            body.append(&l);
        }
        for p in rows {
            let row = gtk::Box::new(gtk::Orientation::Horizontal, 4);
            let label = gtk::Label::new(Some(&p.name));
            label.set_xalign(0.0);
            label.set_hexpand(true);
            label.set_ellipsize(gtk::pango::EllipsizeMode::End);
            label.set_tooltip_text(Some(&format!("{} · {}", p.group, p.server)));
            row.append(&label);
            let linked = items
                .iter()
                .filter(|x| x.machine_id == p.machine_id)
                .collect::<Vec<_>>();
            for (kind, icon, tip, name, enabled) in [
                (
                    3,
                    "utilities-terminal-symbolic",
                    "Open SSH terminal (configure a separate SSH endpoint if needed)",
                    "saved-host-terminal",
                    !agents_only,
                ),
                (
                    0,
                    "ruwac-codex",
                    "Open Codex",
                    "saved-host-codex",
                    linked
                        .iter()
                        .any(|x| !x.preferences.codex_command.is_empty()),
                ),
                (
                    1,
                    "applications-science-symbolic",
                    "Open Claude",
                    "saved-host-claude",
                    linked
                        .iter()
                        .any(|x| !x.preferences.claude_command.is_empty()),
                ),
                (
                    5,
                    "face-smile-symbolic",
                    "Open Kimi",
                    "saved-host-kimi",
                    linked
                        .iter()
                        .any(|x| !x.preferences.kimi_command.is_empty()),
                ),
            ] {
                if enabled {
                    row.append(&shortcut_button(
                        icon,
                        tip,
                        name,
                        &p.id,
                        kind,
                        store.clone(),
                        status.clone(),
                        open.clone(),
                        launch.clone(),
                    ));
                }
            }
            body.append(&row);
        }
    }
    body
}
pub fn sidebar(
    agent_launcher: AgentLauncher,
    open: impl Fn(Profile, PreferenceSink, ConnectionLease) + 'static,
) -> (
    gtk::Box,
    gtk::SearchEntry,
    ProfileRestorer,
    ProfileRestorer,
    InventoryReader,
    ImageProfileReader,
    ShortcutMenus,
) {
    let root = gtk::Box::new(gtk::Orientation::Vertical, 6);
    root.set_width_request(180);
    root.add_css_class("machines-sidebar");
    root.set_margin_start(8);
    root.set_margin_end(8);
    root.set_margin_top(8);
    let heading = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    heading.append(&gtk::Image::from_icon_name("network-workgroup-symbolic"));
    let label = gtk::Label::new(Some("Machines"));
    label.add_css_class("heading");
    heading.append(&label);
    root.append(&heading);
    let search = gtk::SearchEntry::new();
    search.set_placeholder_text(Some("Machines, groups, tags (Ctrl+K)"));
    search.set_width_chars(12);
    root.append(&search);
    let filter = gtk::DropDown::from_strings(&["All machines", "Favorites", "Recent"]);
    root.append(&filter);
    let status = gtk::Label::new(None);
    status.set_widget_name("machines-status");
    status.set_wrap(true);
    status.set_max_width_chars(28);
    status.set_xalign(0.0);
    status.set_selectable(true);
    status.add_css_class("dim-label");
    status.connect_label_notify(|label| label.set_visible(!label.text().is_empty()));
    status.set_visible(false);
    let list = gtk::Box::new(gtk::Orientation::Vertical, 4);
    list.set_widget_name("machines-list-box");
    list.set_margin_end(10);
    let scroll = gtk::ScrolledWindow::builder()
        .vexpand(true)
        .child(&list)
        .build();
    scroll.set_widget_name("machines-list-scroll");
    scroll.set_min_content_height(64);
    root.append(&scroll);
    let store = Rc::new(RefCell::new(Store::load()));
    if let Some(e) = &store.borrow().error {
        status.set_text(e);
    }
    let callback: Rc<dyn Fn(Profile, PreferenceSink, ConnectionLease)> = Rc::new(open);
    let restore: ProfileRestorer = {
        let (s, st) = (store.clone(), status.clone());
        Rc::new(move |id| acquire(&s, &st, id))
    };
    let refresh: Rc<dyn Fn()> = {
        let list = list.clone();
        let store = store.clone();
        let search = search.clone();
        let filter = filter.clone();
        let status = status.clone();
        let callback = callback.clone();
        let agent_launcher = agent_launcher.clone();
        Rc::new(move || {
            while let Some(c) = list.first_child() {
                list.remove(&c);
            }
            let ids = {
                let s = store.borrow();
                s.path
                    .as_ref()
                    .ok_or("Private library unavailable".to_string())
                    .and_then(|path| {
                        inventory::search(
                            &path.with_file_name("machines.sqlite3"),
                            &search.text(),
                            filter.selected(),
                            s.database_revision.get(),
                        )
                    })
            };
            let ids = match ids {
                Ok(ids) => ids,
                Err(e) => {
                    status.set_text(&e);
                    store
                        .borrow()
                        .items
                        .iter()
                        .filter(|p| p.matches(&search.text()))
                        .map(|p| p.id.clone())
                        .collect()
                }
            };
            let mut rows: Vec<_> = store
                .borrow()
                .items
                .iter()
                .enumerate()
                .filter(|(_, p)| {
                    ids.contains(&p.id)
                        && p.matches(&search.text())
                        && match filter.selected() {
                            1 => p.favorite,
                            2 => p.last_used > 0,
                            _ => true,
                        }
                })
                .map(|(i, p)| (i, p.clone()))
                .collect();
            rows.sort_by_key(|(_, p)| {
                (
                    p.group.clone(),
                    !p.favorite,
                    std::cmp::Reverse(p.last_used),
                    p.name.clone(),
                )
            });
            // One row per machine: its RDP endpoint (else the first listed)
            // stands for the machine; other endpoints appear in the subtitle.
            let mut primary: std::collections::HashMap<String, String> = Default::default();
            for (_, p) in &rows {
                let entry = primary
                    .entry(p.machine_id.clone())
                    .or_insert_with(|| p.id.clone());
                if p.protocol == "RDP"
                    && !rows
                        .iter()
                        .any(|(_, x)| x.id == *entry && x.protocol == "RDP")
                {
                    *entry = p.id.clone();
                }
            }
            rows.retain(|(_, p)| primary.get(&p.machine_id) == Some(&p.id));
            for g in &store.borrow().groups {
                if !rows.iter().any(|(_, p)| p.group == g.path)
                    && (search.text().is_empty()
                        || g.path
                            .to_lowercase()
                            .contains(&search.text().to_lowercase()))
                {
                    list.append(&gtk::Label::new(Some(&g.path.replace('/', " › "))));
                }
            }
            let collapsed = crate::sidebar_shell::collapsed_groups();
            let searching = !search.text().is_empty();
            let mut previous = None;
            let mut group_rows = gtk::Box::new(gtk::Orientation::Vertical, 4);
            for (i, p) in rows {
                if previous.as_ref() != Some(&p.group) {
                    let title = if p.group.is_empty() {
                        "Ungrouped".into()
                    } else {
                        p.group.replace('/', " › ")
                    };
                    let group = gtk::Expander::new(Some(&title));
                    group.set_widget_name("machine-group");
                    group.set_expanded(searching || !collapsed.contains(&p.group));
                    group_rows = gtk::Box::new(gtk::Orientation::Vertical, 4);
                    group.set_child(Some(&group_rows));
                    let path = p.group.clone();
                    let message = status.clone();
                    group.connect_expanded_notify(move |group| {
                        // Searching temporarily reveals matches without erasing
                        // the owner's normal collapsed-group preferences.
                        if !searching {
                            if let Err(error) = crate::sidebar_shell::set_group_collapsed(
                                &path,
                                !group.is_expanded(),
                            ) {
                                message.set_text(&error);
                            }
                        }
                    });
                    list.append(&group);
                    previous = Some(p.group.clone());
                }
                let row = gtk::Box::new(gtk::Orientation::Horizontal, 2);
                row.add_css_class("machine-row");
                let star = gtk::Button::with_label(if p.favorite { "★" } else { "☆" });
                let button = gtk::Button::with_label(&format!(
                    "{} · {} · {}",
                    p.name,
                    p.protocol,
                    if p.preferences.label.is_empty() {
                        &p.server
                    } else {
                        &p.preferences.label
                    }
                ));
                let details = button.label().unwrap_or_default();
                button.set_widget_name("machine-connect");
                button.set_tooltip_text(Some(&format!("{details}\n{} · {}", p.server, p.id)));
                let content = gtk::Box::new(gtk::Orientation::Horizontal, 6);
                content.append(&gtk::Image::from_icon_name(if p.protocol == "RDP" {
                    "computer-symbolic"
                } else {
                    "utilities-terminal-symbolic"
                }));
                let text = gtk::Box::new(gtk::Orientation::Vertical, 0);
                let name = gtk::Label::new(Some(&p.name));
                name.set_xalign(0.0);
                name.set_ellipsize(gtk::pango::EllipsizeMode::End);
                name.add_css_class("heading");
                let mut others: Vec<String> = store
                    .borrow()
                    .items
                    .iter()
                    .filter(|x| x.machine_id == p.machine_id && x.protocol != p.protocol)
                    .map(|x| x.protocol.clone())
                    .collect();
                others.sort();
                others.dedup();
                let endpoint = gtk::Label::new(Some(&format!(
                    "{} · {}{}",
                    p.protocol,
                    if p.preferences.label.is_empty() {
                        &p.server
                    } else {
                        &p.preferences.label
                    },
                    others.iter().map(|o| format!(" + {o}")).collect::<String>()
                )));
                endpoint.set_xalign(0.0);
                endpoint.set_ellipsize(gtk::pango::EllipsizeMode::End);
                endpoint.add_css_class("dim-label");
                text.append(&name);
                text.append(&endpoint);
                content.append(&text);
                button.set_child(Some(&content));
                button.update_property(&[gtk::accessible::Property::Label(details.as_str())]);
                button.set_hexpand(true);
                row.append(&star);
                row.append(&button);
                let edit = gtk::Button::with_label("Edit");
                edit.set_icon_name("document-edit-symbolic");
                edit.set_tooltip_text(Some("Edit"));
                for b in [&star, &button, &edit] {
                    b.add_css_class("flat");
                }
                let (s, st) = (store.clone(), status.clone());
                let id = p.id.clone();
                let cb = callback.clone();
                let launcher = agent_launcher.clone();
                edit.connect_clicked(move |b| {
                    editor(
                        b,
                        s.clone(),
                        Some(id.clone()),
                        st.clone(),
                        cb.clone(),
                        launcher.clone(),
                    )
                });
                row.append(&edit);
                let entry = gtk::Box::new(gtk::Orientation::Vertical, 0);
                entry.add_css_class("machine-entry");
                entry.append(&row);
                // Labelled actions on their own line under the name: clear
                // targets that never sit under the sidebar's scrollbar.
                let quick = gtk::Box::new(gtk::Orientation::Horizontal, 3);
                quick.set_margin_start(28);
                let linked = store
                    .borrow()
                    .items
                    .iter()
                    .filter(|x| x.machine_id == p.machine_id)
                    .cloned()
                    .collect::<Vec<_>>();
                // Colour by what is ready to use: AI agents, SSH, or neither.
                let agents = linked.iter().any(|x| {
                    !x.preferences.codex_command.is_empty()
                        || !x.preferences.claude_command.is_empty()
                        || !x.preferences.kimi_command.is_empty()
                });
                let ssh = linked.iter().any(|x| x.protocol == "SSH");
                if agents {
                    entry.add_css_class("machine-agents");
                    entry.set_tooltip_text(Some("AI agents configured (Codex / Claude / Kimi)"));
                } else if ssh {
                    entry.add_css_class("machine-ssh");
                    entry.set_tooltip_text(Some("SSH endpoint configured"));
                }
                for (kind, icon, tip, name, enabled) in [
                    (
                        3,
                        "utilities-terminal-symbolic",
                        "Open SSH terminal",
                        "machine-terminal",
                        true,
                    ),
                    (
                        6,
                        "folder-symbolic",
                        "Open files (SFTP)",
                        "machine-files",
                        true,
                    ),
                    (
                        0,
                        "ruwac-codex",
                        "Open Codex",
                        "machine-codex",
                        linked
                            .iter()
                            .any(|x| !x.preferences.codex_command.is_empty()),
                    ),
                    (
                        1,
                        "applications-science-symbolic",
                        "Open Claude",
                        "machine-claude",
                        linked
                            .iter()
                            .any(|x| !x.preferences.claude_command.is_empty()),
                    ),
                    (
                        5,
                        "face-smile-symbolic",
                        "Open Kimi",
                        "machine-kimi",
                        linked
                            .iter()
                            .any(|x| !x.preferences.kimi_command.is_empty()),
                    ),
                ] {
                    if enabled {
                        let b = shortcut_button(
                            icon,
                            tip,
                            name,
                            &p.id,
                            kind,
                            store.clone(),
                            status.clone(),
                            callback.clone(),
                            agent_launcher.clone(),
                        );
                        quick.append(&b);
                    }
                }
                quick.add_css_class("machine-shortcuts");
                entry.append(&quick);
                group_rows.append(&entry);
                let s = store.clone();
                let label = star.clone();
                let st = status.clone();
                star.connect_clicked(move |_| {
                    let mut s = s.borrow_mut();
                    let id = s.items[i].machine_id.clone();
                    let favorite = !s.items[i].favorite;
                    for p in &mut s.items {
                        if p.machine_id == id {
                            p.favorite = favorite;
                        }
                    }
                    label.set_label(if s.items[i].favorite { "★" } else { "☆" });
                    if let Err(e) = s.save() {
                        st.set_text(&e);
                    }
                });
                let s = store.clone();
                let cb = callback.clone();
                let st = status.clone();
                button.connect_clicked(move |_| {
                    if let Some((p, sink, lease)) = acquire(&s, &st, &p.id) {
                        cb(p, sink, lease);
                    }
                });
            }
        })
    };
    let maintenance = gtk::Box::new(gtk::Orientation::Vertical, 6);
    let maintenance_panel = gtk::Expander::new(Some("Machine tools"));
    maintenance_panel.set_widget_name("machines-tools-panel");
    maintenance_panel.set_child(Some(&maintenance));
    let groups = gtk::Button::with_label("Network groups / defaults…");
    maintenance.append(&groups);
    let (s, st) = (store.clone(), status.clone());
    groups.connect_clicked(move |b| group_editor(b, s.clone(), st.clone()));
    let add = gtk::Button::with_label("New machine / endpoint…");
    add.add_css_class("new-machine");
    if let Some(label) = add.child().and_then(|w| w.downcast::<gtk::Label>().ok()) {
        label.set_ellipsize(gtk::pango::EllipsizeMode::End);
    }
    root.append(&add);
    let (s, st) = (store.clone(), status.clone());
    let cb = callback.clone();
    let launcher = agent_launcher.clone();
    add.connect_clicked(move |b| {
        editor(b, s.clone(), None, st.clone(), cb.clone(), launcher.clone())
    });
    let detect = gtk::Button::with_label("Check SSH key access…");
    detect.set_widget_name("machines-detect-ssh");
    detect.set_tooltip_text(Some(
        "Check which machines already accept your existing SSH keys; review before anything is added",
    ));
    maintenance.append(&detect);
    let (s, st, r) = (store.clone(), status.clone(), refresh.clone());
    detect.connect_clicked(move |b| ssh_access_review(b, &s, &st, r.clone()));
    let refresh_button = gtk::Button::with_label("Refresh machines");
    maintenance.append(&refresh_button);
    let r = refresh.clone();
    refresh_button.connect_clicked(move |_| r());
    let undo = gtk::Button::with_label("Undo last metadata change");
    undo.set_widget_name("machines-undo");
    maintenance.append(&undo);
    root.append(&maintenance_panel);
    let (s, st, r) = (store.clone(), status.clone(), refresh.clone());
    undo.connect_clicked(move |_| {
        let mut s = s.borrow_mut();
        let result = (|| -> Result<(), String> {
            if !s.active.is_empty() {
                return Err(
                    "Close/restart workspace before restoring metadata used by active tabs".into(),
                );
            }
            let path = s
                .path
                .as_ref()
                .ok_or("Storage unavailable")?
                .with_file_name("machines.previous.json");
            use std::os::unix::fs::OpenOptionsExt;
            let f = std::fs::OpenOptions::new()
                .read(true)
                .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
                .open(path)
                .map_err(|_| "No metadata backup")?;
            if !f
                .metadata()
                .is_ok_and(|m| m.is_file() && m.len() <= 1024 * 1024)
            {
                return Err("Unsafe backup".into());
            }
            let mut b = Vec::new();
            f.take(1024 * 1024 + 1)
                .read_to_end(&mut b)
                .map_err(|_| "Cannot read backup")?;
            let previous = s.items.clone();
            let prior_groups = s.groups.clone();
            let prior_error = s.error.clone();
            s.items = parse_library(&b)?;
            s.error = None;
            s.groups = serde_json::from_slice::<Library>(&b)
                .map(|l| l.groups)
                .unwrap_or_default();
            if let Err(e) = s.save() {
                s.items = previous;
                s.groups = prior_groups;
                s.error = prior_error;
                return Err(e);
            }
            Ok(())
        })();
        st.set_text(result.err().as_deref().unwrap_or("Metadata restored"));
        drop(s);
        r();
    });
    let r = refresh.clone();
    filter.connect_selected_notify(move |_| {
        r();
    });
    let r = refresh.clone();
    search.connect_changed(move |_| {
        r();
    });
    let import_controls = gtk::Box::new(gtk::Orientation::Vertical, 6);
    let import_scroll = gtk::ScrolledWindow::builder()
        .child(&import_controls)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .propagate_natural_height(true)
        .max_content_height(160)
        .build();
    let import_panel = gtk::Expander::new(Some("Import from Remmina"));
    import_panel.set_widget_name("machines-import-panel");
    let import_box = gtk::Box::new(gtk::Orientation::Vertical, 6);
    import_box.append(&import_scroll);
    import_panel.set_child(Some(&import_box));
    root.append(&import_panel);
    let selected = Rc::new(RefCell::new(None::<Profile>));
    let path = gtk::Entry::builder()
        .placeholder_text("Optional custom Remmina file")
        .build();
    path.set_widget_name("machines-import-path");
    let password_path = path.clone();
    import_controls.append(&path);
    let passwords = gtk::CheckButton::with_label("Include saved passwords from Remmina's keyring");
    passwords.set_widget_name("machines-import-passwords");
    passwords.set_active(true);
    import_controls.append(&passwords);
    let preview_button = gtk::Button::with_label("Preview selected Remmina file");
    preview_button.set_widget_name("machines-import-preview");
    import_controls.append(&preview_button);
    let discovered_model = gtk::StringList::new(&["Finding saved Remmina profiles…"]);
    let discovered = gtk::DropDown::new(Some(discovered_model.clone()), None::<gtk::Expression>);
    discovered.set_enable_search(true);
    discovered.set_widget_name("machines-import-discovered");
    import_controls.prepend(&discovered);
    let discovered_paths = Rc::new(RefCell::new(Vec::<PathBuf>::new()));
    let (entry, paths, preview_action) = (
        path.clone(),
        discovered_paths.clone(),
        preview_button.clone(),
    );
    discovered.connect_selected_notify(move |list| {
        let index = list.selected() as usize;
        if let Some(file) = index
            .checked_sub(1)
            .and_then(|i| paths.borrow().get(i).cloned())
        {
            entry.set_text(&file.to_string_lossy());
            preview_action.emit_clicked();
        }
    });
    let (tx, rx) = std::sync::mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let found = std::env::var_os("HOME")
            .map(PathBuf::from)
            .map(|home| {
                let data = std::env::var_os("XDG_DATA_HOME")
                    .map(PathBuf::from)
                    .filter(|p| p.is_absolute())
                    .unwrap_or_else(|| home.join(".local/share"));
                discover_remmina(&home, &data)
            })
            .unwrap_or_default();
        let _ = tx.send(found);
    });
    let bulk_model = discovered_model.clone();
    let bulk_paths = discovered_paths.clone();
    let list = discovered.downgrade();
    gtk::glib::timeout_add_local(std::time::Duration::from_millis(20), move || {
        let Some(list) = list.upgrade() else {
            return gtk::glib::ControlFlow::Break;
        };
        match rx.try_recv() {
            Ok(found) => {
                let mut labels = vec![if found.is_empty() {
                    "No saved Remmina profiles found; choose a custom file"
                } else {
                    "Select a saved Remmina profile"
                }];
                labels.extend(found.iter().map(|(_, label)| label.as_str()));
                *discovered_paths.borrow_mut() =
                    found.iter().map(|(path, _)| path.clone()).collect();
                discovered_model.splice(0, discovered_model.n_items(), &labels);
                list.set_selected(0);
                gtk::glib::ControlFlow::Break
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => gtk::glib::ControlFlow::Continue,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => gtk::glib::ControlFlow::Break,
        }
    });
    let bulk = gtk::Button::with_label("Import all Remmina profiles…");
    bulk.set_widget_name("machines-import-all");
    if let Some(label) = bulk.child().and_then(|w| w.downcast::<gtk::Label>().ok()) {
        label.set_wrap(true);
        label.set_max_width_chars(24);
    }
    bulk.set_sensitive(false);
    import_box.prepend(&bulk);
    let available = bulk_model;
    let enabled = bulk.clone();
    available
        .connect_items_changed(move |model, _, _, _| enabled.set_sensitive(model.n_items() > 1));
    let (paths, state, message, refresh_all) =
        (bulk_paths, store.clone(), status.clone(), refresh.clone());
    let bulk_passwords = passwords.clone();
    bulk.connect_clicked(move |button| {
        let paths = paths.borrow().clone();
        let mut reviewed = Vec::new();
        let mut unreadable = 0;
        for path in paths { match preview(&path) { Ok(profile) => reviewed.push((path, profile)), Err(_) => unreadable += 1 } }
        if reviewed.is_empty() { message.set_text("No readable supported Remmina profiles to import"); return; }
        let before = state.borrow().items.clone();
        let incoming: Vec<_> = reviewed.iter().map(|(_,p)|p.clone()).collect();
        let planned = match bulk_import_plan(&before, &incoming) { Ok(p) => p, Err(e) => { message.set_text(&e); return; } };
        let added = planned.len()-before.len();
        let duplicates = incoming.len()-added;
        let copy_passwords = bulk_passwords.is_active();
        let dialog = gtk::Dialog::builder().title("Import all Remmina profiles").modal(true).default_width(520).build();
        dialog.set_widget_name("machines-import-all-dialog");
        dialog.set_transient_for(button.root().and_then(|r|r.downcast::<gtk::Window>().ok()).as_ref());
        let label = gtk::Label::new(Some(&format!("{} new profiles · {} duplicates skipped · {} unreadable/unsupported files skipped\n\n{} Existing passwords are never replaced. Certificates and key files are excluded; review server identity before connecting.",added,duplicates,unreadable,if copy_passwords { "Copy available service passwords from Remmina's keyring, including missing passwords for matching existing profiles." } else { "Import metadata only; passwords excluded." })));
        label.set_wrap(true);dialog.content_area().append(&label);
        dialog.add_button("Cancel",gtk::ResponseType::Cancel);
        dialog.add_button("Import all",gtk::ResponseType::Accept);
        dialog.set_default_response(gtk::ResponseType::Cancel);
        let (state,message,refresh_all)=(state.clone(),message.clone(),refresh_all.clone());
        dialog.connect_response(move |dialog,response| {
            if response == gtk::ResponseType::Accept {
                let unchanged = reviewed.iter().all(|(path,p)|preview(path).ok().is_some_and(|current|serde_json::to_value(current).ok()==serde_json::to_value(p).ok()));
                let mut state=state.borrow_mut();
                if !unchanged || serde_json::to_value(&state.items).ok()!=serde_json::to_value(&before).ok() {
                    message.set_text("Profiles changed after review; review Import all again");dialog.close();return;
                }
                state.items=planned.clone();
                let saved = match state.save() {
                    Ok(())=>{message.set_text(&format!("Imported {added} profiles · skipped {duplicates} duplicates and {unreadable} unreadable files"));true},
                    Err(error)=>{state.items=before.clone();message.set_text(&error);false}
                };
                drop(state);refresh_all();
                if saved && copy_passwords {
                    let jobs = reviewed.iter().filter_map(|(path, imported)| {
                        let mut matches = planned.iter().filter(|p| p.group==imported.group && p.name==imported.name
                            && p.server==imported.server && p.protocol==imported.protocol && p.username==imported.username);
                        let target=matches.next()?.clone();
                        if matches.next().is_some() {None} else {Some((target,path.clone()))}
                    }).collect();
                    import_passwords(jobs, &message);
                }
            }
            dialog.close();
        });
        dialog.present();
    });
    let conflict = gtk::DropDown::from_strings(&[
        "Skip duplicate endpoint",
        "Import duplicate as new identity",
    ]);
    import_controls.append(&conflict);
    let targets = gtk::StringList::new(&["Select an existing endpoint explicitly"]);
    let merge_target = gtk::DropDown::new(Some(targets.clone()), None::<gtk::Expression>);
    merge_target.set_enable_search(true);
    let factory = gtk::SignalListItemFactory::new();
    factory.connect_setup(|_, object| {
        if let Some(item) = object.downcast_ref::<gtk::ListItem>() {
            item.set_child(Some(
                &gtk::Label::builder()
                    .xalign(0.0)
                    .ellipsize(gtk::pango::EllipsizeMode::End)
                    .max_width_chars(28)
                    .build(),
            ));
        }
    });
    factory.connect_bind(|_, object| {
        if let Some(item) = object.downcast_ref::<gtk::ListItem>() {
            if let (Some(value), Some(label)) = (
                item.item()
                    .and_then(|o| o.downcast::<gtk::StringObject>().ok()),
                item.child().and_then(|w| w.downcast::<gtk::Label>().ok()),
            ) {
                label.set_text(&value.string());
                label.set_tooltip_text(Some(&value.string()));
            }
        }
    });
    merge_target.set_factory(Some(&factory));

    merge_target.set_widget_name("machines-merge-target");
    let target_ids = Rc::new(RefCell::new(Vec::<String>::new()));
    import_controls.append(&merge_target);
    let review_merge = gtk::Button::with_label("Review merge into selected endpoint");
    review_merge.set_widget_name("machines-merge-review");
    review_merge.set_sensitive(false);
    import_controls.append(&review_merge);
    let import = gtk::Button::with_label("Import selected profile");
    import.set_widget_name("machines-import-apply");
    import.set_sensitive(false);
    import_box.append(&import);
    status.set_wrap(true);
    status.set_max_width_chars(24);
    root.append(&status);
    let choose = gtk::Button::with_label("Choose file…");
    import_controls.append(&choose);
    let mut child = import_controls.first_child();
    while let Some(widget) = child {
        widget.set_hexpand(true);
        if let Ok(button) = widget.clone().downcast::<gtk::Button>() {
            if let Some(label) = button.child().and_then(|w| w.downcast::<gtk::Label>().ok()) {
                label.set_wrap(true);
                label.set_max_width_chars(24);
            }
        }
        child = widget.next_sibling();
    }
    discovered.set_factory(Some(&factory));
    conflict.set_factory(Some(&factory));
    let entry = path.clone();
    choose.connect_clicked(move |b| {
        let dialog = gtk::FileDialog::new();
        let entry = entry.clone();
        let parent = b.root().and_then(|r| r.downcast::<gtk::Window>().ok());
        dialog.open(
            parent.as_ref(),
            None::<&gtk::gio::Cancellable>,
            move |result| {
                if let Ok(file) = result {
                    if let Some(path) = file.path() {
                        entry.set_text(&path.to_string_lossy());
                    }
                }
            },
        );
    });
    let (stale, disable_import, disable_merge) =
        (selected.clone(), import.clone(), review_merge.clone());
    path.connect_changed(move |_| {
        stale.borrow_mut().take();
        disable_import.set_sensitive(false);
        disable_merge.set_sensitive(false);
    });
    let sel = selected.clone();
    let st = status.clone();
    let enable = import.clone();
    let (merge_enable, merge_model, merge_ids, merge_choice, merge_store) = (
        review_merge.clone(),
        targets.clone(),
        target_ids.clone(),
        merge_target.clone(),
        store.clone(),
    );
    preview_button.connect_clicked(move |_| match preview(Path::new(path.text().as_str())) {
        Ok(p) => {
            st.set_text(&format!(
                "{} / {} · {} {}\nPassword copying follows the keyring checkbox; certificates and keys excluded",
                p.group, p.name, p.protocol, p.server
            ));
            merge_model.splice(1, merge_model.n_items().saturating_sub(1), &[]);
            merge_ids.borrow_mut().clear();
            for item in &merge_store.borrow().items {
                merge_model.append(&format!(
                    "{} / {} · {} · {}",
                    item.group, item.name, item.server, item.id
                ));
                merge_ids.borrow_mut().push(item.id.clone());
            }
            merge_choice.set_selected(0);
            merge_enable.set_sensitive(true);
            *sel.borrow_mut() = Some(p);
            enable.set_sensitive(true);
        }
        Err(e) => {
            st.set_text(&e);
            sel.borrow_mut().take();
            enable.set_sensitive(false);
            merge_enable.set_sensitive(false);
        }
    });
    let (s, selected_preview, ids, choice, st, refresh_merge) = (
        store.clone(),
        selected.clone(),
        target_ids.clone(),
        merge_target.clone(),
        status.clone(),
        refresh.clone(),
    );
    review_merge.connect_clicked(move |button| {
        let chosen = choice.selected() as usize;
        let Some(id) = chosen.checked_sub(1).and_then(|i| ids.borrow().get(i).cloned()) else {st.set_text("Select an existing endpoint explicitly; names are never used to merge"); return;};
        let Some(imported) = selected_preview.borrow().clone() else {st.set_text("Preview a Remmina file first"); return;};
        let current = {let state=s.borrow(); if state.active.contains_key(&id) {st.set_text("Close this endpoint's active connection tabs before merging metadata"); return;} state.items.iter().find(|p|p.id==id).cloned()};
        let Some(current) = current else {st.set_text("Selected endpoint no longer exists; preview again"); return;};
        let merged=match merge_metadata(&current,&imported) {Ok(p)=>p,Err(e)=>{st.set_text(&e);return;}};
        let dialog=gtk::Dialog::builder().title("Review metadata merge").modal(true).build();
        dialog.set_transient_for(button.root().and_then(|r|r.downcast::<gtk::Window>().ok()).as_ref());
        dialog.set_widget_name("machines-merge-dialog");
        let details=format!("Target ID: {}\nName: {} → {}\nGroup: {} → {}\nProtocol: {} → {}\nServer: {} → {}\nUsername: {} → {}\nTags: {} → {}\n\nExisting machine/endpoint identity, credential reference, trust, key paths, preferences, favorites and recency are preserved. No credentials are imported. Group changes may alter inherited defaults. Verify the changed endpoint and effective settings before connecting. Undo restores the previous library.", current.id,current.name,merged.name,current.group,merged.group,current.protocol,merged.protocol,current.server,merged.server,current.username,merged.username,current.tags.join(", "),merged.tags.join(", "));
        let label=gtk::Label::new(Some(&details));label.set_wrap(true);label.set_selectable(true);dialog.content_area().append(&label);
        dialog.add_button("Cancel",gtk::ResponseType::Cancel);dialog.add_button("Apply reviewed metadata",gtk::ResponseType::Accept);
        let (s,st,r)=(s.clone(),st.clone(),refresh_merge.clone());
        dialog.connect_response(move |d,response| {
            if response==gtk::ResponseType::Accept {
                let mut state=s.borrow_mut();
                let index=state.items.iter().position(|p|p.id==current.id);
                if state.active.contains_key(&current.id) || index.is_none_or(|i|serde_json::to_value(&state.items[i]).ok()!=serde_json::to_value(&current).ok()) {st.set_text("Endpoint changed or became active after review; preview and review again"); d.close();return;}
                let index=index.unwrap();state.items[index]=merged.clone();
                match state.save(){Ok(())=>st.set_text("Reviewed metadata merged; original identity and credential reference preserved"),Err(e)=>{state.items[index]=current.clone();st.set_text(&e);}}
                drop(state);r();
            }
            d.close();
        });dialog.present();
    });
    let st = status.clone();
    let r = refresh.clone();
    let import_store = store.clone();
    import.connect_clicked(move |_| {
        if let Some(mut p) = selected.borrow_mut().take() {
            p.identify();
            let mut s = import_store.borrow_mut();
            if s.items.len() >= 1000 {
                st.set_text("Profile limit reached");
                return;
            }
            if conflict.selected() == 1
                || !s.items.iter().any(|x| {
                    x.group == p.group
                        && x.name == p.name
                        && x.server == p.server
                        && x.protocol == p.protocol
                })
            {
                s.items.push(p.clone());
            }
            let targets: Vec<_> = s
                .items
                .iter()
                .filter(|target| {
                    target.group == p.group
                        && target.name == p.name
                        && target.server == p.server
                        && target.protocol == p.protocol
                        && target.username == p.username
                })
                .cloned()
                .collect();
            let mut saved = false;
            match s.save() {
                Ok(()) => {
                    st.set_text("Machine metadata saved");
                    saved = true;
                }
                Err(e) => st.set_text(&e),
            }
            drop(s);
            r();
            if saved && passwords.is_active() && targets.len() == 1 {
                import_passwords(
                    vec![(
                        targets[0].clone(),
                        PathBuf::from(password_path.text().as_str()),
                    )],
                    &st,
                );
            }
        }
    });
    let (s, st) = (store.clone(), status.clone());
    let remote: ProfileRestorer = Rc::new(move |id| {
        if !s.borrow().items.iter().any(|p| {
            p.id == id
                && p.protocol == "RDP"
                && !p.username.is_empty()
                && !p.secret_reference.is_empty()
        }) {
            return None;
        }
        acquire(&s, &st, id)
    });
    // Atomic saves from another desktop window replace the inode. Reload only
    // when the private metadata changes; keep active connection leases and the
    // last valid list if a writer leaves unsafe or malformed metadata behind.
    let weak_root = root.downgrade();
    let weak_store = Rc::downgrade(&store);
    let weak_refresh = Rc::downgrade(&refresh);
    let metadata_stamp = |path: &Path| {
        use std::os::unix::fs::MetadataExt;
        std::fs::symlink_metadata(path).ok().map(|m| {
            (
                m.dev(),
                m.ino(),
                m.len(),
                m.mtime(),
                m.mtime_nsec(),
                m.mode(),
            )
        })
    };
    let path = store.borrow().path.clone();
    let mut stamp = path.as_deref().and_then(metadata_stamp);
    let reload_status = status.clone();
    gtk::glib::timeout_add_local(std::time::Duration::from_secs(1), move || {
        let (Some(_root), Some(store), Some(refresh)) = (
            weak_root.upgrade(),
            weak_store.upgrade(),
            weak_refresh.upgrade(),
        ) else {
            return gtk::glib::ControlFlow::Break;
        };
        let next = path.as_deref().and_then(metadata_stamp);
        if next != stamp {
            stamp = next;
            let fresh = Store::load();
            if fresh.path == path && fresh.error.is_none() {
                let mut current = store.borrow_mut();
                // Our own atomic save also changes the file stamp. Preserve
                // its useful result (including password-import counts) when
                // the refreshed metadata is identical.
                let changed = current.error.is_some()
                    || serde_json::to_value((&current.items, &current.groups)).ok()
                        != serde_json::to_value((&fresh.items, &fresh.groups)).ok();
                current.items = fresh.items;
                current.groups = fresh.groups;
                current.database_revision.set(fresh.database_revision.get());
                *current.mirror_hash.borrow_mut() = fresh.mirror_hash.borrow().clone();
                current.error = None;
                drop(current);
                refresh();
                if changed {
                    reload_status.set_text("Machine library refreshed");
                }
            } else {
                let error = fresh.error.unwrap_or_else(|| {
                    "Private profile storage unavailable; existing library protected".into()
                });
                store.borrow_mut().error = Some(error.clone());
                reload_status.set_text(&error);
            }
        }
        gtk::glib::ControlFlow::Continue
    });
    refresh();
    let image_store = store.clone();
    let image_profile: ImageProfileReader = Rc::new(move |id| {
        let s = image_store.borrow();
        if s.error.is_some() || s.path.is_none() {
            return Err("Machine library unavailable".into());
        }
        let p = s
            .items
            .iter()
            .find(|p| p.id == id)
            .ok_or("SSH profile no longer exists")?;
        let p = resolve(p, &s.groups, &s.items)?;
        if p.protocol != "SSH"
            || (!p.preferences.gateway.is_empty() && p.preferences.ssh_jump_alias.is_empty())
        {
            return Err("A direct registered SSH endpoint is required".into());
        }
        Ok(p)
    });
    let inventory_store = store.clone();
    let inventory: InventoryReader = Rc::new(move || {
        let s = inventory_store.borrow();
        if s.path.is_none() || s.error.is_some() {
            return Err(ruwac_protocol::Error::new(
                "INVENTORY_UNAVAILABLE",
                "Machine library unavailable; inspect the native Machines panel",
            ));
        }
        Ok(s.items.iter().map(|p| {
            let effective = resolve(p, &s.groups, &s.items);
            let valid = effective.is_ok();
            let e = effective.as_ref().unwrap_or(p);
            // Explicit allowlist: credentials, login names, trust and private
            // key/configuration paths never enter automation responses.
            json!({"id":p.id,"machine_id":p.machine_id,"name":p.name,"group":p.group,"protocol":p.protocol,"server":p.server,"favorite":p.favorite,"last_used":p.last_used,"tags":p.tags,"label":e.preferences.label,"network":e.preferences.network,"configuration_valid":valid})
        }).collect())
    });
    let menus: ShortcutMenus = Rc::new(move |agents_only| {
        shortcut_menu(&store, &status, &callback, &agent_launcher, agents_only)
    });
    (
        root,
        search,
        restore,
        remote,
        inventory,
        image_profile,
        menus,
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ssh_config_port_and_hostname_are_used_without_explicit_port() {
        let g = "user generated-user\nhostname host.example.net\nport 2222\nidentityfile ~/.ssh/id_ed25519\n";
        assert_eq!(
            resolved_endpoint(g, "alias"),
            ("host.example.net".into(), 2222)
        );
        assert_eq!(resolved_endpoint("", "alias"), ("alias".into(), 22));
    }
    #[test]
    fn seventy_nested_profiles_read_only() {
        let dir = std::env::temp_dir().join(ruwac_protocol::id("profiles"));
        std::fs::create_dir(&dir).unwrap();
        for i in 0..70 {
            let path = dir.join(format!("{i}.remmina"));
            let original = format!(
                "[remmina]\nname=Same leaf\ngroup=Team{}/Nested\nprotocol={}\nserver=127.0.0.1:{}\nusername=fixture\npassword=DO-NOT-IMPORT\nssh_privatekey=/private/key\nlabels=linux,test\n",
                i,
                if i % 2 == 0 { "RDP" } else { "SSH" },
                3000 + i
            );
            std::fs::write(&path, &original).unwrap();
            let p = preview(&path).unwrap();
            assert_eq!(p.group, format!("Team{i}/Nested"));
            assert!(p.matches("linux"));
            assert!(p.secret_reference.is_empty());
            assert!(!serde_json::to_string(&p).unwrap().contains("DO-NOT-IMPORT"));
            assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
        }
        std::fs::remove_dir_all(dir).unwrap();
    }
}

/// Exercises real sidebar widgets only within the desktop's explicit fixture mode.
fn sidebar_widget(root: &impl IsA<gtk::Widget>, name: &str) -> Option<gtk::Widget> {
    let w = root.as_ref();
    if w.widget_name() == name {
        return Some(w.clone());
    }
    let mut child = w.first_child();
    while let Some(c) = child {
        child = c.next_sibling();
        if let Some(found) = sidebar_widget(&c, name) {
            return Some(found);
        }
    }
    None
}
fn expand_sidebar_panel(root: &gtk::Box, name: &str, expanded: bool) {
    if let Some(panel) = sidebar_widget(root, name).and_then(|w| w.downcast::<gtk::Expander>().ok())
    {
        panel.set_expanded(expanded);
    }
}
pub fn fixture(
    root: &gtk::Box,
    file: Option<&Path>,
    query: Option<&str>,
    favorite: bool,
    open: bool,
) -> serde_json::Value {
    if let Some(query) = query {
        let mut child = root.first_child();
        while let Some(w) = child {
            child = w.next_sibling();
            if let Some(search) = w.downcast_ref::<gtk::SearchEntry>() {
                search.set_text(query);
            }
        }
    }
    if let Some(file) = file {
        expand_sidebar_panel(root, "machines-import-panel", true);
        if let Some(entry) = sidebar_widget(root, "machines-import-path")
            .and_then(|w| w.downcast::<gtk::Entry>().ok())
        {
            entry.set_text(&file.to_string_lossy());
        }
        for name in ["machines-import-preview", "machines-import-apply"] {
            if let Some(button) =
                sidebar_widget(root, name).and_then(|w| w.downcast::<gtk::Button>().ok())
            {
                button.emit_clicked();
            }
        }
    }
    fn collect(w: &gtk::Widget, out: &mut Vec<(gtk::Button, gtk::Button)>) {
        if let Some(row) = w.downcast_ref::<gtk::Box>() {
            if let (Some(a), Some(b)) = (
                row.first_child(),
                row.first_child().and_then(|w| w.next_sibling()),
            ) {
                if let (Ok(a), Ok(b)) = (a.downcast::<gtk::Button>(), b.downcast::<gtk::Button>()) {
                    if a.label().is_some_and(|l| l == "☆" || l == "★") {
                        out.push((a, b));
                    }
                }
            }
        }
        let mut child = w.first_child();
        while let Some(c) = child {
            child = c.next_sibling();
            collect(&c, out);
        }
    }
    let mut rows = Vec::new();
    collect(root.upcast_ref(), &mut rows);
    if open {
        if let Some((_, button)) = rows.first() {
            button.emit_clicked();
        }
    }
    if favorite {
        if let Some((star, _)) = rows.first() {
            star.emit_clicked();
        }
    }
    serde_json::json!({"rows":rows.len(),"favorites":rows.iter().filter(|(s,_)|s.label().as_deref()==Some("★")).count(),"labels":rows.iter().map(|(_,b)|b.tooltip_text().or(b.label()).unwrap_or_default().to_string()).collect::<Vec<_>>()})
}
#[cfg(test)]
mod special_file_tests {
    use super::*;
    #[test]
    fn fifo_preview_is_rejected_without_blocking() {
        use std::os::unix::ffi::OsStrExt;
        let root = std::env::temp_dir().join(ruwac_protocol::id("preview_fifo"));
        std::fs::create_dir(&root).unwrap();
        let fifo = root.join("special.remmina");
        let name = std::ffi::CString::new(fifo.as_os_str().as_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        std::thread::spawn(move || {
            let _ = tx.send(preview(&fifo));
        });
        assert!(
            rx.recv_timeout(std::time::Duration::from_secs(1))
                .unwrap()
                .is_err()
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}

fn editor(
    button: &gtk::Button,
    store: Rc<RefCell<Store>>,
    id: Option<String>,
    status: gtk::Label,
    open: Rc<dyn Fn(Profile, PreferenceSink, ConnectionLease)>,
    agent_launcher: AgentLauncher,
) {
    editor_seed(button, store, id, status, open, agent_launcher, None);
}
fn editor_seed(
    button: &gtk::Button,
    store: Rc<RefCell<Store>>,
    id: Option<String>,
    status: gtk::Label,
    open: ConnectionOpener,
    agent_launcher: AgentLauncher,
    seed: Option<Profile>,
) {
    let existing = id
        .as_ref()
        .and_then(|id| store.borrow().items.iter().find(|p| &p.id == id).cloned());
    let is_ssh_setup = seed.is_some();
    let mut original = existing.clone().or(seed).unwrap_or(Profile {
        id: String::new(),
        machine_id: String::new(),
        preferences: Preferences::default(),
        inherit: false,
        inherit_credential: false,
        inherit_connection: false,
        name: String::new(),
        group: String::new(),
        protocol: "RDP".into(),
        server: String::new(),
        username: String::new(),
        secret_reference: String::new(),
        favorite: false,
        last_used: 0,
        tags: vec![],
    });
    original.identify();
    let parent = button.root().and_then(|w| w.downcast::<gtk::Window>().ok());
    let dialog = gtk::Dialog::builder()
        .title(if is_ssh_setup {
            "Set up SSH terminal and agent shortcuts"
        } else {
            "Machine / connection properties"
        })
        .modal(true)
        .default_width(540)
        .default_height(620)
        .build();
    dialog.set_transient_for(parent.as_ref());
    dialog.set_widget_name(if is_ssh_setup {
        "ssh-login-setup"
    } else {
        "machine-connection-editor"
    });
    dialog.add_button("Cancel", gtk::ResponseType::Cancel);
    dialog.add_button("Save", gtk::ResponseType::Accept);
    dialog.add_button("Save & Connect", gtk::ResponseType::Other(5));
    let grid = gtk::Grid::builder()
        .row_spacing(5)
        .column_spacing(8)
        .margin_top(8)
        .margin_bottom(8)
        .margin_start(8)
        .margin_end(8)
        .build();
    let sc = gtk::ScrolledWindow::builder()
        .vexpand(true)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .min_content_height(240)
        .child(&grid)
        .build();
    grid.set_widget_name("machine-editor-grid");
    dialog.content_area().append(&sc);
    let vals = [
        ("Machine name", original.name.clone()),
        ("Group / nested path", original.group.clone()),
        (
            "Machine identity (shared by endpoints)",
            original.machine_id.clone(),
        ),
        ("Address / port", original.server.clone()),
        ("Username", original.username.clone()),
        ("Endpoint label", original.preferences.label.clone()),
        ("Network", original.preferences.network.clone()),
        (
            "Gateway (metadata only)",
            original.preferences.gateway.clone(),
        ),
        ("Tags (comma separated)", original.tags.join(",")),
        ("Credential reference", original.secret_reference.clone()),
        (
            "RDP CA file or approved certificate fingerprint",
            original.preferences.trust.clone(),
        ),
        (
            "SSH identity file",
            original.preferences.identity_file.clone(),
        ),
        (
            "SSH known_hosts file",
            original.preferences.known_hosts.clone(),
        ),
    ];
    let mut fields = Vec::new();
    for (i, (label, value)) in vals.iter().enumerate() {
        let e = gtk::Entry::builder()
            .text(value)
            .hexpand(true)
            .width_chars(12)
            .max_width_chars(24)
            .build();
        e.set_widget_name(&format!("machine-field-{i}"));
        grid.attach(&gtk::Label::new(Some(label)), 0, i as i32, 1, 1);
        grid.attach(&e, 1, i as i32, 1, 1);
        if i == 2 {
            e.set_editable(false);
        }
        fields.push(e);
    }
    let protocol = gtk::DropDown::from_strings(&["RDP", "SSH"]);
    protocol.set_widget_name("machine-field-13");
    protocol.set_selected(if original.protocol == "SSH" { 1 } else { 0 });
    let display = gtk::DropDown::from_strings(&["fit", "native", "stretch", "dynamic"]);
    display.set_widget_name("machine-field-14");
    display.set_selected(
        ["fit", "native", "stretch", "dynamic"]
            .iter()
            .position(|v| *v == original.preferences.display)
            .unwrap_or(0) as u32,
    );
    let clipboard = gtk::DropDown::from_strings(&[
        "Bidirectional",
        "Local → remote",
        "Remote → local",
        "Manual",
        "Disabled",
    ]);
    clipboard.set_selected(original.preferences.clipboard);
    clipboard.set_widget_name("machine-field-15");
    for (i, (label, widget)) in [
        ("Protocol", &protocol),
        ("RDP display mode", &display),
        ("Clipboard policy", &clipboard),
    ]
    .iter()
    .enumerate()
    {
        grid.attach(&gtk::Label::new(Some(label)), 0, 13 + i as i32, 1, 1);
        grid.attach(*widget, 1, 13 + i as i32, 1, 1);
    }
    let zoom = gtk::SpinButton::with_range(0.25, 4.0, 0.25);
    zoom.set_widget_name("machine-field-16");
    zoom.set_value(original.preferences.zoom);
    grid.attach(&gtk::Label::new(Some("Zoom")), 0, 16, 1, 1);
    grid.attach(&zoom, 1, 16, 1, 1);
    let full = gtk::CheckButton::with_label("Fullscreen on connect");
    full.set_widget_name("machine-field-17");
    full.set_active(original.preferences.fullscreen);
    grid.attach(&full, 1, 17, 1, 1);
    let retry = gtk::CheckButton::with_label("Reconnect after transient transport loss");
    retry.set_widget_name("machine-field-18");
    retry.set_active(original.preferences.reconnect);
    grid.attach(&retry, 1, 18, 1, 1);
    let inherit = gtk::CheckButton::with_label(
        "Inherit RDP defaults from nearest network group (saved overrides retained)",
    );
    inherit.set_active(original.inherit);
    inherit.set_widget_name("machine-field-22");
    grid.attach(&inherit, 0, 22, 2, 1);
    let inherit_connection =
        gtk::CheckButton::with_label("Fill empty connection fields from network groups");
    inherit_connection.set_active(original.inherit_connection);
    inherit_connection.set_widget_name("machine-field-24");
    grid.attach(&inherit_connection, 0, 24, 2, 1);
    let inherit_credential = gtk::CheckButton::with_label("Use group's saved credential profile");
    inherit_credential.set_active(original.inherit_credential);
    inherit_credential.set_widget_name("machine-field-25");
    inherit_credential.set_sensitive(original.inherit_connection);
    grid.attach(&inherit_credential, 0, 25, 2, 1);
    let ic = inherit_credential.clone();
    inherit_connection.connect_toggled(move |c| {
        ic.set_sensitive(c.is_active());
        if !c.is_active() {
            ic.set_active(false);
        }
    });
    let secret = gtk::PasswordEntry::builder()
        .show_peek_icon(true)
        .placeholder_text("Set / replace credential in Secret Service")
        .build();
    secret.set_widget_name("rdp-password");
    grid.attach(&secret, 1, 19, 1, 1);
    let actions = gtk::Box::new(gtk::Orientation::Horizontal, 4);
    grid.attach(&actions, 1, 20, 1, 1);
    let info = gtk::Label::new(Some(
        "Endpoint ID is stable. Share a machine identity to add another endpoint. SSH verifies host keys; unknown keys prompt in the terminal. SSH jump aliases use the gateway’s own SSH configuration and trusted key, independently of this endpoint. RDP gateways remain unsupported.",
    ));
    info.set_wrap(true);
    info.set_widget_name("profile-credential-status");
    grid.attach(&info, 0, 21, 2, 1);
    let rdp_mode = gtk::DropDown::from_strings(&[
        "Standard / Windows RDP",
        "GNOME Remote Login",
        "GNOME Desktop Sharing",
    ]);
    rdp_mode.set_widget_name("rdp-connection-mode");
    rdp_mode.set_sensitive(original.protocol == "RDP");
    rdp_mode.set_selected(match original.preferences.rdp_mode.as_str() {
        "gnome-remote-login" => 1,
        "gnome-desktop-sharing" => 2,
        _ => 0,
    });
    grid.attach(&gtk::Label::new(Some("RDP connection mode")), 0, 26, 1, 1);
    grid.attach(&rdp_mode, 1, 26, 1, 1);
    let service_heading = gtk::Label::new(Some("RDP Service Login"));
    grid.attach(&service_heading, 0, 27, 2, 1);
    let gnome = gtk::Box::new(gtk::Orientation::Vertical, 6);
    gnome.append(&gtk::Label::new(Some(
        "GNOME User Login — separate GDM credentials",
    )));
    let gnome_user = gtk::Entry::builder()
        .text(&original.preferences.gnome_username)
        .placeholder_text("Linux username at GDM")
        .build();
    gnome_user.set_widget_name("gnome-username");
    let gnome_ref = gtk::Entry::builder()
        .text(if original.preferences.gnome_secret_reference.is_empty() {
            format!("{}.gnome", original.id)
        } else {
            original.preferences.gnome_secret_reference.clone()
        })
        .placeholder_text("Separate Secret Service reference")
        .build();
    gnome_ref.set_widget_name("gnome-secret-reference");
    let gnome_password = gtk::PasswordEntry::builder()
        .show_peek_icon(true)
        .placeholder_text("GNOME user password; stored only in Secret Service")
        .build();
    gnome_password.set_widget_name("gnome-password");
    gnome.append(&gnome_user);
    gnome.append(&gnome_ref);
    gnome.append(&gnome_password);
    let gnome_auto =
        gtk::CheckButton::with_label("Sign in automatically to the saved Linux account");
    gnome_auto.set_widget_name("gnome-auto-login");
    gnome_auto.set_active(original.preferences.gnome_auto_login);
    gnome_auto.set_tooltip_text(Some("Recognizes supported Fedora GDM forms locally. Unknown fields, changed focus and user input stop sign-in. Requires local OCR tools."));
    gnome.append(&gnome_auto);
    let gnome_actions = gtk::Box::new(gtk::Orientation::Horizontal, 4);
    gnome.append(&gnome_actions);
    for (label, forget) in [
        ("Set / replace GNOME password", false),
        ("Forget GNOME password", true),
    ] {
        let b = gtk::Button::with_label(label);
        gnome_actions.append(&b);
        let credentials_store = store.clone();
        let (entry, reference, stage_one, info) = (
            gnome_password.clone(),
            gnome_ref.clone(),
            fields[9].clone(),
            info.clone(),
        );
        b.connect_clicked(move |b| {
            let reference = reference.text().to_string();
            if reference.is_empty()
                || reference == stage_one.text()
                || credentials_store
                    .borrow()
                    .items
                    .iter()
                    .any(|p| p.secret_reference == reference)
            {
                info.set_text("GNOME and RDP service credential references must be separate");
                entry.set_text("");
                return;
            }
            let value = if forget {
                String::new()
            } else {
                entry.text().to_string()
            };
            entry.set_text("");
            let (tx, rx) = std::sync::mpsc::sync_channel(1);
            b.set_sensitive(false);
            std::thread::spawn(move || {
                let _ = tx.send(if forget {
                    crate::credentials::forget(&reference)
                } else {
                    crate::credentials::set(&reference, value)
                });
            });
            let (b, info) = (b.clone(), info.clone());
            gtk::glib::timeout_add_local(std::time::Duration::from_millis(25), move || {
                match rx.try_recv() {
                    Ok(r) => {
                        b.set_sensitive(true);
                        info.set_text(
                            r.err().as_deref().unwrap_or(
                                "GNOME credential operation completed in Secret Service",
                            ),
                        );
                        gtk::glib::ControlFlow::Break
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => gtk::glib::ControlFlow::Continue,
                    Err(_) => {
                        b.set_sensitive(true);
                        info.set_text("Secret Service worker unavailable");
                        gtk::glib::ControlFlow::Break
                    }
                }
            });
        });
    }
    grid.attach(&gnome, 0, 28, 2, 1);
    gnome.set_visible(protocol.selected() == 0 && rdp_mode.selected() == 1);
    service_heading.set_visible(true);
    let (section, heading, proto) = (gnome.clone(), service_heading.clone(), protocol.clone());
    rdp_mode.connect_selected_notify(move |mode| {
        let enabled = proto.selected() == 0 && mode.selected() == 1;
        section.set_visible(enabled);
        heading.set_text(if proto.selected() == 1 {
            "SSH Login"
        } else if mode.selected() == 1 {
            "RDP Service Login"
        } else {
            "RDP Login"
        });
    });
    let (section, heading, mode) = (gnome.clone(), service_heading.clone(), rdp_mode.clone());
    protocol.connect_selected_notify(move |proto| {
        let enabled = proto.selected() == 0 && mode.selected() == 1;
        section.set_visible(enabled);
        heading.set_text(if proto.selected() == 1 {
            "SSH Login"
        } else if mode.selected() == 1 {
            "RDP Service Login"
        } else {
            "RDP Login"
        });
        mode.set_sensitive(proto.selected() == 0);
    });
    for (label, forget) in [
        ("Set / replace credential", false),
        ("Forget credential", true),
    ] {
        let b = gtk::Button::with_label(label);
        actions.append(&b);
        let credentials_store = store.clone();
        let (entry, reference, other, info) = (
            secret.clone(),
            fields[9].clone(),
            gnome_ref.clone(),
            info.clone(),
        );
        b.connect_clicked(move |b| {
            let value = if forget {
                String::new()
            } else {
                entry.text().to_string()
            };
            entry.set_text("");
            let reference = reference.text().to_string();
            if reference == other.text()
                || credentials_store
                    .borrow()
                    .items
                    .iter()
                    .any(|p| p.preferences.gnome_secret_reference == reference)
            {
                info.set_text("GNOME and RDP service credential references must be separate");
                return;
            }
            let (tx, rx) = std::sync::mpsc::sync_channel(1);
            b.set_sensitive(false);
            std::thread::spawn(move || {
                let _ = tx.send(if forget {
                    crate::credentials::forget(&reference)
                } else {
                    crate::credentials::set(&reference, value)
                });
            });
            let (b, info) = (b.clone(), info.clone());
            gtk::glib::timeout_add_local(std::time::Duration::from_millis(25), move || {
                match rx.try_recv() {
                    Ok(r) => {
                        b.set_sensitive(true);
                        info.set_text(
                            r.err()
                                .as_deref()
                                .unwrap_or("Credential operation completed in Secret Service"),
                        );
                        gtk::glib::ControlFlow::Break
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => gtk::glib::ControlFlow::Continue,
                    Err(_) => {
                        b.set_sensitive(true);
                        info.set_text("Secret Service worker unavailable");
                        gtk::glib::ControlFlow::Break
                    }
                }
            });
        });
    }
    // Keep the two authentication stages in separate visible native sections.
    let mut child = grid.first_child();
    while let Some(w) = child {
        child = w.next_sibling();
        let (column, row, _, _) = grid.query_child(&w);
        if column == 0 && matches!(row, 4 | 9) {
            w.set_visible(false);
        }
    }
    for widget in [
        fields[4].upcast_ref::<gtk::Widget>(),
        fields[9].upcast_ref(),
        secret.upcast_ref(),
        actions.upcast_ref(),
        service_heading.upcast_ref(),
    ] {
        grid.remove(widget);
    }
    fields[4].set_widget_name("endpoint-username");
    fields[9].set_widget_name("endpoint-secret-reference");
    let service = gtk::Box::new(gtk::Orientation::Vertical, 6);
    service.append(&service_heading);
    service.append(&gtk::Label::new(Some("Username")));
    service.append(&fields[4]);
    service.append(&gtk::Label::new(Some("Saved Secret Service reference")));
    service.append(&fields[9]);
    service.append(&secret);
    service.append(&actions);
    grid.attach(&service, 0, 27, 2, 1);
    let identity_reference = gtk::Entry::builder()
        .text(&original.preferences.identity_secret_reference)
        .placeholder_text("Reference for this key's passphrase; leave empty for an unencrypted key")
        .hexpand(true)
        .build();
    grid.attach(
        &gtk::Label::new(Some("SSH key passphrase reference")),
        0,
        29,
        1,
        1,
    );
    identity_reference.set_widget_name("ssh-key-passphrase-reference");
    grid.attach(&identity_reference, 1, 29, 1, 1);
    let key_secret = gtk::PasswordEntry::builder()
        .show_peek_icon(true)
        .placeholder_text("Key passphrase — stored only in Secret Service")
        .build();
    key_secret.set_widget_name("ssh-key-passphrase");
    // Saved/not-saved status in every password box, refreshed after Set/Forget
    // (each reports in `info`).
    let refresh_hints = {
        let (secret, rdp_ref, gnome_password, gnome_ref, key_secret, key_ref) = (
            secret.downgrade(),
            fields[9].downgrade(),
            gnome_password.downgrade(),
            gnome_ref.downgrade(),
            key_secret.downgrade(),
            identity_reference.downgrade(),
        );
        move || {
            if let (Some(e), Some(r)) = (secret.upgrade(), rdp_ref.upgrade()) {
                credential_hint(&e, r.text().into(), "password");
            }
            if let (Some(e), Some(r)) = (gnome_password.upgrade(), gnome_ref.upgrade()) {
                credential_hint(&e, r.text().into(), "GNOME password");
            }
            if let (Some(e), Some(r)) = (key_secret.upgrade(), key_ref.upgrade()) {
                credential_hint(&e, r.text().into(), "key passphrase");
            }
        }
    };
    refresh_hints();
    info.connect_label_notify(move |_| refresh_hints());
    grid.attach(&key_secret, 1, 30, 1, 1);
    let key_actions = gtk::Box::new(gtk::Orientation::Horizontal, 4);
    grid.attach(&key_actions, 1, 31, 1, 1);
    for (label, forget) in [
        ("Set / replace key passphrase", false),
        ("Forget key passphrase", true),
    ] {
        let button = gtk::Button::with_label(label);
        key_actions.append(&button);
        let (entry, reference, info) =
            (key_secret.clone(), identity_reference.clone(), info.clone());
        button.connect_clicked(move |button| {
            let value = if forget {
                String::new()
            } else {
                entry.text().to_string()
            };
            entry.set_text("");
            let reference = reference.text().to_string();
            let (tx, rx) = std::sync::mpsc::sync_channel(1);
            button.set_sensitive(false);
            std::thread::spawn(move || {
                let _ = tx.send(if forget {
                    crate::credentials::forget(&reference)
                } else {
                    crate::credentials::set(&reference, value)
                });
            });
            let (button, info) = (button.clone(), info.clone());
            gtk::glib::timeout_add_local(std::time::Duration::from_millis(25), move || {
                match rx.try_recv() {
                    Ok(result) => {
                        button.set_sensitive(true);
                        info.set_text(
                            result.err().as_deref().unwrap_or(
                                "SSH key passphrase operation completed in Secret Service",
                            ),
                        );
                        gtk::glib::ControlFlow::Break
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => gtk::glib::ControlFlow::Continue,
                    Err(_) => {
                        button.set_sensitive(true);
                        info.set_text("Secret Service worker unavailable");
                        gtk::glib::ControlFlow::Break
                    }
                }
            });
        });
    }
    // Stack labels above controls: long host names and credential references
    // must not push the label column outside a narrow native dialog.
    fn form_row(form: &gtk::Box, label: &str, widget: &impl IsA<gtk::Widget>) {
        let title = gtk::Label::new(Some(label));
        title.set_xalign(0.0);
        title.set_wrap(true);
        let row = gtk::Box::new(gtk::Orientation::Vertical, 4);
        row.append(&title);
        row.append(widget);
        form.append(&row);
    }
    let mut old_children = Vec::new();
    let mut child = grid.first_child();
    while let Some(w) = child {
        child = w.next_sibling();
        let (col, row, _, _) = grid.query_child(&w);
        old_children.push((col, row, w));
    }
    for (_, _, w) in &old_children {
        grid.remove(w);
    }
    sc.set_child(gtk::Widget::NONE);
    let form = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(12)
        .margin_top(12)
        .margin_bottom(12)
        .margin_start(16)
        .margin_end(16)
        .build();
    form.set_widget_name("machine-editor-form");
    form_row(&form, "Machine name", &fields[0]);
    form_row(&form, "Host / address and port", &fields[3]);
    form_row(&form, "Connection type", &protocol);
    let mode_row = gtk::Box::new(gtk::Orientation::Vertical, 4);
    form_row(&mode_row, "RDP connection mode", &rdp_mode);
    mode_row.set_visible(protocol.selected() == 0);
    form.append(&mode_row);
    let mr = mode_row.clone();
    protocol.connect_selected_notify(move |p| mr.set_visible(p.selected() == 0));
    // Keep username/password adjacent; implementation references are secondary.
    for w in [
        fields[9].upcast_ref::<gtk::Widget>(),
        secret.upcast_ref(),
        actions.upcast_ref(),
    ] {
        service.remove(w);
    }
    // Remove the old reference label before placing it beneath the password.
    if let Some(label) = fields[4].next_sibling() {
        service.remove(&label);
    }
    service.append(&secret);
    service.append(&actions);
    let service_reference = gtk::Expander::builder()
        .label("Saved credential reference")
        .child(&fields[9])
        .build();
    service.append(&service_reference);
    form.append(&service);
    gnome.remove(&gnome_ref);
    let gnome_reference = gtk::Expander::builder()
        .label("Saved GNOME credential reference")
        .child(&gnome_ref)
        .build();
    gnome.append(&gnome_reference);
    form.append(&gnome);
    form_row(&form, "Group / nested path", &fields[1]);
    let advanced = gtk::Box::new(gtk::Orientation::Vertical, 12);
    for i in [5, 8, 10, 12, 6, 7, 2] {
        form_row(&advanced, vals[i].0, &fields[i]);
    }
    let ssh_jump = gtk::Entry::new();
    ssh_jump.set_widget_name("ssh-jump-alias");
    ssh_jump.set_text(&original.preferences.ssh_jump_alias);
    ssh_jump.set_placeholder_text(Some("Existing SSH config alias; blank connects directly"));
    form_row(&advanced, "SSH jump / bastion alias", &ssh_jump);
    form_row(&advanced, "RDP display mode", &display);
    form_row(&advanced, "Clipboard policy", &clipboard);
    let primary_sync =
        gtk::CheckButton::with_label("Share mouse selections with this Linux desktop");
    primary_sync.set_widget_name("primary-sync");
    primary_sync.set_active(original.preferences.primary_sync);
    let primary_paste = gtk::CheckButton::with_label(
        "Middle-click pastes local terminal selections into remote RUWAC",
    );
    primary_paste.set_widget_name("primary-paste");
    primary_paste.set_active(original.preferences.primary_paste);
    let primary_user = gtk::Entry::new();
    primary_user.set_widget_name("primary-user");
    primary_user.set_placeholder_text(Some(
        "Linux SSH account (separate from the RDP service login)",
    ));
    let primary_account = if !original.preferences.primary_user.is_empty() {
        original.preferences.primary_user.clone()
    } else if !original.preferences.gnome_username.is_empty() {
        original.preferences.gnome_username.clone()
    } else {
        let users: std::collections::HashSet<String> = store
            .borrow()
            .items
            .iter()
            .filter(|p| {
                p.protocol == "SSH" && p.machine_id == original.machine_id && !p.username.is_empty()
            })
            .map(|p| p.username.clone())
            .collect();
        if users.len() == 1 {
            users.into_iter().next().unwrap()
        } else {
            String::new()
        }
    };
    primary_user.set_text(&primary_account);
    let primary_display = gtk::Entry::new();
    primary_display.set_widget_name("primary-display");
    primary_display.set_text(if original.preferences.primary_display.is_empty() {
        "wayland-0"
    } else {
        &original.preferences.primary_display
    });
    let primary_port = gtk::SpinButton::with_range(0.0, 65535.0, 1.0);
    primary_port.set_value(original.preferences.primary_port as f64);
    form_row(&advanced, "Mouse selection sharing", &primary_sync);
    form_row(&advanced, "Terminal selection paste", &primary_paste);
    form_row(&advanced, "Selection SSH account", &primary_user);
    form_row(&advanced, "Remote Wayland session", &primary_display);
    form_row(
        &advanced,
        "Selection SSH port (0 uses SSH config)",
        &primary_port,
    );
    let primary_help = gtk::Label::new(Some(
        "Receive selected remote text for local middle-click paste, separately from Ctrl+C. Requires a trusted SSH key and Python3/GTK4/Xwayland on the remote Linux desktop. Follows the receive clipboard policy and stops on disconnect. No password prompts or global clipboard synchronization.",
    ));
    primary_help.set_wrap(true);
    advanced.append(&primary_help);

    form_row(&advanced, "Zoom", &zoom);
    for w in [
        &full,
        &retry,
        &inherit,
        &inherit_connection,
        &inherit_credential,
    ] {
        if let Some(label) = w.child().and_downcast::<gtk::Label>() {
            label.set_wrap(true);
            label.set_xalign(0.0);
        }
        advanced.append(w);
    }
    form_row(
        &advanced,
        "SSH key passphrase reference",
        &identity_reference,
    );
    advanced.append(&key_secret);
    advanced.append(&key_actions);
    for buttons in [&actions, &gnome_actions, &key_actions] {
        buttons.set_orientation(gtk::Orientation::Vertical);
    }
    if existing.is_some() {
        for (label, response) in [
            ("Duplicate machine", 1),
            ("Add endpoint", 3),
            ("Delete endpoint…", 2),
            ("Delete machine…", 4),
        ] {
            let b = gtk::Button::with_label(label);
            let d = dialog.clone();
            b.connect_clicked(move |_| d.response(gtk::ResponseType::Other(response)));
            advanced.append(&b);
        }
    }
    let agents = gtk::Box::new(gtk::Orientation::Vertical, 6);
    let key_row = gtk::Box::new(gtk::Orientation::Horizontal, 4);
    fields[4].set_placeholder_text(Some(&format!(
        "Default: {}",
        gtk::glib::user_name().to_string_lossy()
    )));
    fields[11].set_hexpand(true);
    fields[11].set_placeholder_text(Some("Use your SSH configuration, or choose a key"));
    key_row.append(&fields[11]);
    let choose_key = gtk::Button::with_label("Choose key…");
    choose_key.set_widget_name("ssh-choose-key");
    let entry = fields[11].clone();
    choose_key.connect_clicked(move |b| {
        let parent = b.root().and_then(|w| w.downcast::<gtk::Window>().ok());
        let chooser = gtk::FileChooserNative::new(
            Some("Choose SSH identity file"),
            parent.as_ref(),
            gtk::FileChooserAction::Open,
            Some("Use key"),
            Some("Cancel"),
        );
        let entry = entry.clone();
        chooser.connect_response(move |c, response| {
            if response == gtk::ResponseType::Accept {
                if let Some(path) = c.file().and_then(|f| f.path()) {
                    entry.set_text(&path.to_string_lossy());
                }
            }
            c.destroy();
        });
        chooser.show();
    });
    key_row.append(&choose_key);
    // Offer keys this user already has; an empty field keeps OpenSSH's own
    // choice (agent, ~/.ssh/config IdentityFile, default keys).
    let detected = crate::ssh_keys::detect();
    if !detected.is_empty() {
        let list = gtk::Box::new(gtk::Orientation::Vertical, 2);
        let pop = gtk::Popover::new();
        for key in &detected {
            let b = gtk::Button::with_label(&key.label());
            b.add_css_class("flat");
            b.set_widget_name("ssh-detected-key");
            b.set_tooltip_text(Some(&key.path.to_string_lossy()));
            let (entry, pop, path) = (fields[11].clone(), pop.clone(), key.path.clone());
            b.connect_clicked(move |_| {
                entry.set_text(&path.to_string_lossy());
                pop.popdown();
            });
            list.append(&b);
        }
        pop.set_child(Some(&list));
        let menu = gtk::MenuButton::builder()
            .label(format!("Detected keys ({})", detected.len()))
            .popover(&pop)
            .tooltip_text("SSH keys found in ~/.ssh and ~/.ssh/config")
            .build();
        menu.set_widget_name("ssh-detected-keys");
        key_row.append(&menu);
    }
    let login = gtk::DropDown::from_strings(&[
        "SSH key (saved key, agent or ~/.ssh)",
        "Password (saved in Secret Service)",
        "Install my SSH key on this machine now…",
    ]);
    login.set_widget_name("ssh-login-method");
    login.set_selected(u32::from(original.preferences.ssh_auth == "password"));
    if original.protocol == "SSH" {
        form_row(&agents, "Login", &login);
        let password_hint = gtk::Label::new(Some(
            "Enter the password in the password field above and choose Set / replace; it is supplied to ssh from Secret Service.",
        ));
        password_hint.set_wrap(true);
        password_hint.set_xalign(0.0);
        password_hint.add_css_class("dim-label");
        password_hint.set_visible(login.selected() == 1);
        agents.append(&password_hint);
        let hint = password_hint.clone();
        login.connect_selected_notify(move |l| hint.set_visible(l.selected() == 1));
        form_row(
            &agents,
            "SSH identity file (saved path; private key stays on this computer)",
            &key_row,
        );
        // Explicit check: does a key this user already has log in here?
        let access = gtk::Label::new(None);
        access.set_widget_name("ssh-key-access");
        access.set_wrap(true);
        access.set_xalign(0.0);
        let check = gtk::Button::with_label("Check key login");
        check.set_widget_name("ssh-key-check");
        check.set_tooltip_text(Some(
            "One batch-mode OpenSSH login with your existing keys: no password, no new host keys",
        ));
        let check_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        check_row.append(&check);
        check_row.append(&access);
        agents.append(&check_row);
        let (fields_check, original_check, jump_check) =
            (fields.clone(), original.clone(), ssh_jump.clone());
        let (connection_check, credential_check, store_check) = (
            inherit_connection.clone(),
            inherit_credential.clone(),
            store.clone(),
        );
        let allowed = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
        let cancel = allowed.clone();
        check.connect_unrealize(move |_| {
            cancel.store(false, std::sync::atomic::Ordering::Release);
        });
        let access_weak = access.downgrade();
        check.connect_clicked(move |check| {
            let mut probe = original_check.clone();
            probe.server = fields_check[3].text().into();
            probe.username = fields_check[4].text().into();
            probe.preferences.identity_file = fields_check[11].text().into();
            probe.preferences.known_hosts = fields_check[12].text().into();
            probe.preferences.ssh_jump_alias = jump_check.text().into();
            probe.group = fields_check[1].text().into();
            probe.inherit_connection = connection_check.is_active();
            probe.inherit_credential = credential_check.is_active();
            let probe = {
                let metadata = store_check.borrow();
                match resolve(&probe, &metadata.groups, &metadata.items) {
                    Ok(probe) => probe,
                    Err(error) => {
                        if let Some(label) = access_weak.upgrade() {
                            label.set_text(&error);
                        }
                        return;
                    }
                }
            };
            if probe.server.is_empty() {
                if let Some(a) = access_weak.upgrade() {
                    a.set_text("Enter the host first");
                }
                return;
            }
            check.set_sensitive(false);
            if let Some(a) = access_weak.upgrade() {
                a.set_text("Checking…");
            }
            let (tx, rx) = std::sync::mpsc::sync_channel(1);
            let cancel = allowed.clone();
            std::thread::spawn(move || {
                let keys = crate::ssh_probe::identities_cancel(&cancel);
                let _ = tx.send(crate::ssh_probe::probe_cancel(&probe, &keys, &cancel));
            });
            let (access, check, cancel) = (access_weak.clone(), check.downgrade(), allowed.clone());
            gtk::glib::timeout_add_local(std::time::Duration::from_millis(100), move || {
                if !cancel.load(std::sync::atomic::Ordering::Acquire) {
                    return gtk::glib::ControlFlow::Break;
                }
                match rx.try_recv() {
                    Ok(outcome) => {
                        if let Some(a) = access.upgrade() {
                            a.set_text(&outcome.describe());
                        }
                        if let Some(c) = check.upgrade() {
                            c.set_sensitive(true);
                        }
                        gtk::glib::ControlFlow::Break
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => gtk::glib::ControlFlow::Continue,
                    Err(_) => gtk::glib::ControlFlow::Break,
                }
            });
        });
    } else {
        let edit_ssh = gtk::Button::with_label("SSH login and key…");
        edit_ssh.set_widget_name("ssh-endpoint-properties");
        edit_ssh.set_sensitive(existing.is_some());
        let (s, st, cb, launcher, source) = (
            store.clone(),
            status.clone(),
            open.clone(),
            agent_launcher.clone(),
            original.clone(),
        );
        edit_ssh.connect_clicked(move |button| {
            let id = s
                .borrow()
                .items
                .iter()
                .find(|p| p.machine_id == source.machine_id && p.protocol == "SSH")
                .map(|p| p.id.clone());
            if let Some(id) = id {
                editor(
                    button,
                    s.clone(),
                    Some(id),
                    st.clone(),
                    cb.clone(),
                    launcher.clone(),
                );
            } else {
                editor_seed(
                    button,
                    s.clone(),
                    None,
                    st.clone(),
                    cb.clone(),
                    launcher.clone(),
                    Some(ssh_seed(&source)),
                );
            }
        });
        let ssh_note = gtk::Label::new(Some(
            "SSH uses its own account and key, separately from the RDP service login.",
        ));
        ssh_note.set_wrap(true);
        agents.append(&ssh_note);
        agents.append(&edit_ssh);
    }

    // A linked SSH editor edits transport/login only. Agent preferences are
    // machine-wide, displayed once on its RDP/machine properties form.
    let agent_options = gtk::Box::new(gtk::Orientation::Vertical, 8);
    let linked_login = original.protocol == "SSH"
        && store
            .borrow()
            .items
            .iter()
            .any(|p| p.machine_id == original.machine_id && p.protocol == "RDP");
    agent_options.set_visible(!linked_login);
    if linked_login {
        dialog.set_title(Some("SSH login and key"));
        let note = gtk::Label::new(Some(
            "Agent commands and update settings are in this machine's main connection properties.",
        ));
        note.set_wrap(true);
        agents.append(&note);
    }
    agents.append(&agent_options);
    let codex_command = gtk::Entry::new();
    codex_command.set_widget_name("machine-codex-command");
    codex_command.set_text(&original.preferences.codex_command);
    codex_command.set_placeholder_text(Some("codex resume --all"));
    let claude_command = gtk::Entry::new();
    claude_command.set_widget_name("machine-claude-command");
    claude_command.set_text(&original.preferences.claude_command);
    claude_command.set_placeholder_text(Some("claude --resume"));
    let kimi_command = gtk::Entry::new();
    kimi_command.set_widget_name("machine-kimi-command");
    kimi_command.set_text(&original.preferences.kimi_command);
    kimi_command.set_placeholder_text(Some("kimi --session"));
    for (label, entry, default) in [
        (
            "Enable Codex shortcut",
            &codex_command,
            "codex resume --all",
        ),
        ("Enable Claude shortcut", &claude_command, "claude --resume"),
        ("Enable Kimi shortcut", &kimi_command, "kimi --session"),
    ] {
        let enable = gtk::CheckButton::with_label(label);
        enable.set_active(!entry.text().is_empty());
        let entry = entry.clone();
        enable.connect_toggled(move |b| {
            if b.is_active() && entry.text().is_empty() {
                entry.set_text(default);
            } else if !b.is_active() {
                entry.set_text("");
            }
        });
        agent_options.append(&enable);
    }
    form_row(
        &agent_options,
        "Codex command (no passwords)",
        &codex_command,
    );
    form_row(
        &agent_options,
        "Claude command (no passwords)",
        &claude_command,
    );
    form_row(&agent_options, "Kimi command (no passwords)", &kimi_command);
    let auto_install = gtk::CheckButton::with_label(
        "Automatically install missing Codex / Claude / Kimi on launch",
    );
    auto_install.set_widget_name("machine-agent-auto-install");
    auto_install.set_active(original.preferences.agent_install_missing);
    agent_options.append(&auto_install);
    let ruwac_install = gtk::CheckButton::with_label(
        "Check and update RUWAC on agent connection (verified prebuilt package)",
    );
    ruwac_install.set_widget_name("machine-ruwac-auto-install");
    ruwac_install.set_active(original.preferences.ruwac_install_missing);
    agent_options.append(&ruwac_install);
    let note = gtk::Label::new(Some(
        "Opt-in per-user official installers run in the SSH terminal only for a new agent process. Existing tools and agent logins are preserved. Sign in inside the agent when requested.",
    ));
    note.set_wrap(true);
    agent_options.append(&note);
    let backend = gtk::DropDown::from_strings(&[
        "RUWAC",
        "GNU Screen",
        "tmux",
        "Direct (no persistent remote session)",
    ]);
    backend.set_widget_name("machine-agent-backend");
    backend.set_selected(
        ["ruwac", "screen", "tmux", "direct"]
            .iter()
            .position(|s| *s == original.preferences.agent_backend)
            .unwrap_or(0) as u32,
    );
    form_row(&agent_options, "Remote session backend", &backend);
    let shared =
        gtk::CheckButton::with_label("Share one remote RUWAC session (separate agent windows)");
    shared.set_widget_name("machine-agent-share-session");
    shared.set_active(!original.preferences.agent_session.is_empty());
    let agent_session = gtk::Entry::new();
    agent_session.set_widget_name("machine-agent-session");
    agent_session.set_text(&original.preferences.agent_session);
    agent_session.set_placeholder_text(Some(
        "Existing session name, or agents for a new shared session",
    ));
    let entry = agent_session.clone();
    shared.connect_toggled(move |b| {
        if b.is_active() && entry.text().is_empty() {
            entry.set_text("agents");
        } else if !b.is_active() {
            entry.set_text("");
        }
    });
    agent_options.append(&shared);
    form_row(
        &agent_options,
        "Shared session name (blank keeps one session per agent)",
        &agent_session,
    );

    let setup = gtk::Button::with_label("Install / update remote RUWAC CLI…");
    setup.set_widget_name("machine-install-ruwac");
    setup.set_sensitive(existing.is_some());
    setup.set_tooltip_text(Some("Install or update through the linked SSH endpoint. If needed, configure a separate SSH login first. Uses the verified headless installer; does not install Codex or Claude."));
    let (s, st, cb, launcher, saved_id) = (
        store.clone(),
        status.clone(),
        open.clone(),
        agent_launcher.clone(),
        original.id.clone(),
    );
    setup.connect_clicked(move |button| {
        shortcut_action(button, &s, &st, &cb, &launcher, &saved_id, 2)
    });
    agent_options.append(&setup);
    let authorize = gtk::Button::with_label("Authorize SSH public key on machine…");
    authorize.set_widget_name("ssh-authorize-key");
    // The third login choice only starts the confirmed install action for the
    // saved, explicitly chosen key; the login method stays as it was, so
    // cancelling changes nothing. Choose "SSH key" once the key is installed.
    let install_now = authorize.downgrade();
    let previous = std::rc::Rc::new(std::cell::Cell::new(login.selected().min(1)));
    let install_info = info.downgrade();
    login.connect_selected_notify(move |l| {
        if l.selected() != 2 {
            previous.set(l.selected());
            return;
        }
        l.set_selected(previous.get());
        let Some(b) = install_now.upgrade() else {
            return;
        };
        if !b.is_sensitive() {
            if let Some(info) = install_info.upgrade() {
                info.set_text("Save this endpoint first; key installation uses its saved route");
            }
            return;
        }
        b.emit_clicked();
    });
    authorize.set_sensitive(existing.is_some());
    authorize.set_tooltip_text(Some("Save the selected key first. Opens ssh-copy-id in a terminal; only your public key is installed after confirmation and SSH authentication."));
    let (s, st, cb, launcher, saved_id) = (
        store.clone(),
        status.clone(),
        open.clone(),
        agent_launcher.clone(),
        original.id.clone(),
    );
    authorize.connect_clicked(move |button| {
        shortcut_action(button, &s, &st, &cb, &launcher, &saved_id, 4)
    });
    agents.append(&authorize);

    let agent_panel = gtk::Expander::builder()
        .label("Remote terminals / agents (SSH)")
        .child(&agents)
        .build();
    agent_panel.set_expanded(
        is_ssh_setup
            || original.protocol == "SSH"
            || !original.preferences.codex_command.is_empty()
            || !original.preferences.claude_command.is_empty()
            || !original.preferences.kimi_command.is_empty(),
    );
    form.append(&agent_panel);
    let expander = gtk::Expander::builder()
        .label("Advanced connection settings")
        .child(&advanced)
        .build();
    form.append(&expander);
    info.set_xalign(0.0);
    form.append(&info);
    sc.set_child(Some(&form));
    dialog.connect_response(move|d,response|{
        if response==gtk::ResponseType::Other(2) || response==gtk::ResponseType::Other(4) {
            let machine=response==gtk::ResponseType::Other(4);if store.borrow().items.iter().any(|p|(p.id==original.id||(machine&&p.machine_id==original.machine_id))&&store.borrow().active.contains_key(&p.id)){info.set_text("This profile is used by an opened tab. Close all of its connection tabs before deleting it.");return;}
            let confirm=gtk::Dialog::builder().title("Delete endpoint metadata?").transient_for(d).modal(true).build();confirm.content_area().append(&gtk::Label::new(Some("Credentials remain in Secret Service. Undo restores metadata.")));confirm.add_button("Cancel",gtk::ResponseType::Cancel);confirm.add_button("Delete",gtk::ResponseType::Accept);
            let(s,id,machine_id,st,parent)=(store.clone(),original.id.clone(),original.machine_id.clone(),status.clone(),d.clone());confirm.connect_response(move|c,r|{if r==gtk::ResponseType::Accept {let mut s=s.borrow_mut();let old=s.items.clone();s.items.retain(|p|p.id!=id&&(!machine||p.machine_id!=machine_id));if let Err(e)=s.save(){s.items=old;st.set_text(&e);}else{drop(s);st.set_text("Metadata deleted; previous library retained for Undo");refresh_widgets(parent.transient_for().as_ref());parent.close();}}c.close();});confirm.present();return;
        }
        if response!=gtk::ResponseType::Accept && response!=gtk::ResponseType::Other(1) && response!=gtk::ResponseType::Other(3) && response!=gtk::ResponseType::Other(5){d.close();return;}
        let mut p=original.clone();p.preferences.primary_sync=primary_sync.is_active();p.preferences.primary_paste=primary_paste.is_active();p.preferences.primary_user=primary_user.text().into();p.preferences.primary_display=primary_display.text().into();p.preferences.primary_port=primary_port.value() as u16;p.preferences.ssh_auth=if p.protocol=="SSH"&&login.selected()==1{"password".into()}else{String::new()};p.preferences.kimi_command=kimi_command.text().into();p.preferences.agent_install_missing=auto_install.is_active();p.preferences.ruwac_install_missing=ruwac_install.is_active();p.preferences.agent_session=agent_session.text().into();p.preferences.codex_command=codex_command.text().into();p.preferences.claude_command=claude_command.text().into();p.preferences.agent_backend=["ruwac","screen","tmux","direct"][backend.selected()as usize].into();p.name=fields[0].text().into();p.group=fields[1].text().into();p.machine_id=fields[2].text().into();p.server=fields[3].text().into();p.username=fields[4].text().into();p.preferences.label=fields[5].text().into();p.preferences.network=fields[6].text().into();p.preferences.gateway=fields[7].text().into();p.preferences.ssh_jump_alias=ssh_jump.text().into();p.tags=fields[8].text().split(',').map(str::trim).filter(|s|!s.is_empty()).map(str::to_owned).collect();p.secret_reference=fields[9].text().into();p.preferences.trust=fields[10].text().into();p.preferences.identity_file=fields[11].text().into();p.preferences.identity_secret_reference=identity_reference.text().into();p.preferences.known_hosts=fields[12].text().into();p.protocol=if protocol.selected()==1{"SSH"}else{"RDP"}.into();p.preferences.display=["fit","native","stretch","dynamic"][display.selected() as usize].into();p.preferences.clipboard=clipboard.selected();p.preferences.zoom=zoom.value();p.preferences.fullscreen=full.is_active();p.preferences.reconnect=retry.is_active();p.inherit=inherit.is_active();p.inherit_connection=inherit_connection.is_active();p.inherit_credential=inherit_credential.is_active();p.preferences.rdp_mode=["","gnome-remote-login","gnome-desktop-sharing"][rdp_mode.selected() as usize].into();p.preferences.gnome_auto_login=gnome_auto.is_active();p.preferences.gnome_username=gnome_user.text().into();p.preferences.gnome_secret_reference=if p.preferences.rdp_mode=="gnome-remote-login"||!original.preferences.gnome_secret_reference.is_empty(){gnome_ref.text().into()}else{String::new()};
        if response==gtk::ResponseType::Other(1){p.id=ruwac_protocol::id("endpoint");p.machine_id=ruwac_protocol::id("machine");p.secret_reference=p.id.clone();if !p.preferences.gnome_secret_reference.is_empty(){p.preferences.gnome_secret_reference=format!("{}.gnome",p.id);}p.name.push_str(" (copy)");}
        if response==gtk::ResponseType::Other(3){p.name=original.name.clone();p.group=original.group.clone();p.id=ruwac_protocol::id("endpoint");p.secret_reference=p.id.clone();if !p.preferences.gnome_secret_reference.is_empty(){p.preferences.gnome_secret_reference=format!("{}.gnome",p.id);}}
        if let Err(e)=p.validate(){info.set_text(&e);return;}
        let saved_id=p.id.clone();let mut s=store.borrow_mut();let old=s.items.clone();
        if let Some(i)=s.items.iter().position(|x|x.id==p.id){for item in &mut s.items{if item.machine_id==p.machine_id{item.name=p.name.clone();item.group=p.group.clone();item.tags=p.tags.clone();}}s.items[i]=p;}else{if s.items.len()>=1000{info.set_text("Profile limit reached");return;}let new_machine=p.machine_id.clone();s.items.push(p);
            if response==gtk::ResponseType::Other(1){for endpoint in old.iter().filter(|x|x.machine_id==original.machine_id&&x.id!=original.id){if s.items.len()>=1000{s.items=old.clone();info.set_text("Profile limit reached");return;}let mut copy=endpoint.clone();copy.id=ruwac_protocol::id("endpoint");copy.secret_reference=copy.id.clone();if !copy.preferences.gnome_secret_reference.is_empty(){copy.preferences.gnome_secret_reference=format!("{}.gnome",copy.id);}copy.machine_id=new_machine.clone();copy.name.push_str(" (copy)");s.items.push(copy);}}
        }
        if !linked_login { if let Some(saved)=s.items.iter().find(|x|x.id==saved_id).cloned() {
            for sibling in s.items.iter_mut().filter(|x|x.machine_id==saved.machine_id) {
                sibling.preferences.codex_command=saved.preferences.codex_command.clone();
                sibling.preferences.claude_command=saved.preferences.claude_command.clone();
                sibling.preferences.kimi_command=saved.preferences.kimi_command.clone();
                sibling.preferences.agent_backend=saved.preferences.agent_backend.clone();
                sibling.preferences.agent_session=saved.preferences.agent_session.clone();
                sibling.preferences.agent_install_missing=saved.preferences.agent_install_missing;
                sibling.preferences.ruwac_install_missing=saved.preferences.ruwac_install_missing;
            }
        }}
        if let Err(e)=s.save(){s.items=old;info.set_text(&e);return;}
        status.set_text("Machine metadata saved; existing tabs retain their connection settings");drop(s);
        // Release the editor's text-input focus before destroying its entries.
        // Open the connection after the response unwinds, rather than switching
        // the parent tab while Wayland still owns this dialog's input context.
        gtk::prelude::GtkWindowExt::set_focus(d,None::<&gtk::Widget>);
        refresh_widgets(parent.as_ref().map(|w|w.upcast_ref::<gtk::Widget>()));d.close();
        if response==gtk::ResponseType::Other(5){let(store,status,open)=(store.clone(),status.clone(),open.clone());gtk::glib::idle_add_local_once(move||{if let Some((profile,sink,lease))=acquire(&store,&status,&saved_id){open(profile,sink,lease);}});}
    });
    dialog.present();
}
fn refresh_widgets(root: Option<&impl IsA<gtk::Widget>>) {
    fn visit(w: &gtk::Widget) {
        // A collapsed Expander retains its content without parenting it into
        // the visible widget tree. Refresh lives in Tools, so visit that content
        // explicitly instead of requiring the owner to search or expand Tools.
        if let Some(expander) = w.downcast_ref::<gtk::Expander>() {
            if let Some(child) = expander.child() {
                visit(&child);
            }
            return;
        }
        if let Some(b) = w.downcast_ref::<gtk::Button>() {
            if b.label().as_deref() == Some("Refresh machines") {
                b.emit_clicked();
                return;
            }
        }
        let mut c = w.first_child();
        while let Some(w) = c {
            c = w.next_sibling();
            visit(&w);
        }
    }
    if let Some(root) = root {
        visit(root.as_ref());
    }
}

fn group_editor(button: &gtk::Button, store: Rc<RefCell<Store>>, status: gtk::Label) {
    let parent = button.root().and_then(|w| w.downcast::<gtk::Window>().ok());
    let dialog = gtk::Dialog::builder()
        .title("Network groups and inherited defaults")
        .modal(true)
        .default_width(480)
        .build();
    dialog.set_transient_for(parent.as_ref());
    dialog.add_button("Cancel", gtk::ResponseType::Cancel);
    dialog.add_button("Save", gtk::ResponseType::Accept);
    dialog.add_button("Delete…", gtk::ResponseType::Other(2));
    dialog.add_button("Duplicate group", gtk::ResponseType::Other(3));
    let choose = gtk::DropDown::from_strings(
        &std::iter::once("New group".to_string())
            .chain(store.borrow().groups.iter().map(|g| g.path.clone()))
            .collect::<Vec<_>>()
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>(),
    );
    let content = gtk::Box::new(gtk::Orientation::Vertical, 6);
    let scroll = gtk::ScrolledWindow::builder()
        .propagate_natural_height(true)
        .max_content_height(500)
        .min_content_height(200)
        .child(&content)
        .build();
    dialog.content_area().append(&scroll);
    choose.set_widget_name("group-choice");
    content.append(&choose);
    let path = gtk::Entry::builder()
        .placeholder_text("Full group path e.g. Organization/Region/Team")
        .build();
    path.set_widget_name("group-path");
    content.append(&path);
    let mode = gtk::DropDown::from_strings(&["fit", "native", "stretch", "dynamic"]);
    mode.set_widget_name("group-display");
    mode.set_selected(3);
    content.append(&gtk::Label::new(Some("Inherited RDP display mode")));
    content.append(&mode);
    let clip = gtk::DropDown::from_strings(&[
        "Bidirectional",
        "Local → remote",
        "Remote → local",
        "Manual",
        "Disabled",
    ]);
    content.append(&gtk::Label::new(Some("Inherited clipboard policy")));
    content.append(&clip);
    let names = [
        ("username", "Default username"),
        ("network", "Network label"),
        ("gateway", "Gateway (metadata only)"),
        ("identity_file", "SSH key file"),
        ("identity_secret_reference", "SSH key passphrase reference"),
        ("known_hosts", "SSH known-hosts file"),
        ("trust", "RDP CA file"),
        (
            "ssh_jump_alias",
            "SSH jump / bastion alias (existing SSH config)",
        ),
    ];
    let mut connection_fields = Vec::new();
    for (name, label) in names {
        content.append(&gtk::Label::new(Some(label)));
        let entry = gtk::Entry::new();
        entry.set_widget_name(&format!("group-{name}"));
        content.append(&entry);
        connection_fields.push(entry);
    }
    let account_ids: Vec<_> = store.borrow().items.iter().map(|p| p.id.clone()).collect();
    let account_names: Vec<_> = std::iter::once("No shared credential profile".to_string())
        .chain(
            store
                .borrow()
                .items
                .iter()
                .map(|p| format!("{} · {} · {}", p.name, p.username, p.id)),
        )
        .collect();
    let account =
        gtk::DropDown::from_strings(&account_names.iter().map(String::as_str).collect::<Vec<_>>());
    account.set_widget_name("group-credential-profile");
    content.append(&gtk::Label::new(Some(
        "Credential profile (existing Secret Service reference)",
    )));
    content.append(&account);
    clip.set_widget_name("group-clipboard");
    let info = gtk::Label::new(Some(
        "Rename/move updates group paths and keeps machine, endpoint and group identities. Explicit endpoint overrides remain unchanged.",
    ));
    info.set_wrap(true);
    content.append(&info);
    let (s, p, m, c) = (store.clone(), path.clone(), mode.clone(), clip.clone());
    let (cf, ac, ids) = (
        connection_fields.clone(),
        account.clone(),
        account_ids.clone(),
    );
    choose.connect_selected_notify(move |dropdown| {
        if let Some(g) = s
            .borrow()
            .groups
            .get(dropdown.selected().saturating_sub(1) as usize)
            .filter(|_| dropdown.selected() > 0)
        {
            p.set_text(&g.path);
            m.set_selected(
                ["fit", "native", "stretch", "dynamic"]
                    .iter()
                    .position(|v| *v == g.defaults.display)
                    .unwrap_or(0) as u32,
            );
            c.set_selected(g.defaults.clipboard);
            for (entry, value) in cf.iter().zip([
                &g.username,
                &g.defaults.network,
                &g.defaults.gateway,
                &g.defaults.identity_file,
                &g.defaults.identity_secret_reference,
                &g.defaults.known_hosts,
                &g.defaults.trust,
                &g.defaults.ssh_jump_alias,
            ]) {
                entry.set_text(value);
            }
            ac.set_selected(
                ids.iter()
                    .position(|id| id == &g.credential_profile)
                    .map(|i| i + 1)
                    .unwrap_or(0) as u32,
            );
        } else {
            p.set_text("");
            for entry in &cf {
                entry.set_text("");
            }
            ac.set_selected(0);
        }
    });
    dialog.connect_response(move |d, r| {
        if r != gtk::ResponseType::Accept
            && r != gtk::ResponseType::Other(2)
            && r != gtk::ResponseType::Other(3)
        {
            d.close();
            return;
        }
        let path = path.text().trim_matches('/').to_string();
        if path.is_empty()
            || path.len() > 1024
            || path.chars().any(char::is_control)
            || path
                .split('/')
                .any(|p| p.is_empty() || p == "." || p == "..")
        {
            info.set_text("Enter a valid nonempty full group path");
            return;
        }
        let index = choose.selected();
        let mut s = store.borrow_mut();
        let old = s.groups.clone();
        let items = s.items.clone();
        if r == gtk::ResponseType::Other(2) {
            if index == 0 {
                info.set_text("Select a saved group");
                return;
            }
            let original = s.groups[(index - 1) as usize].path.clone();
            if s.items.iter().any(|p| {
                (p.group == original || p.group.starts_with(&(original.clone() + "/")))
                    && s.active.contains_key(&p.id)
            }) {
                info.set_text("Group is used by an opened tab; deletion protected");
                return;
            }
            drop(s);
            let confirm = gtk::Dialog::builder()
                .title("Delete group and endpoint metadata?")
                .transient_for(d)
                .modal(true)
                .build();
            confirm.add_button("Cancel", gtk::ResponseType::Cancel);
            confirm.add_button("Delete", gtk::ResponseType::Accept);
            let (s, st, parent) = (store.clone(), status.clone(), d.clone());
            confirm.connect_response(move |c, r| {
                if r == gtk::ResponseType::Accept {
                    let mut s = s.borrow_mut();
                    let (old, items) = (s.groups.clone(), s.items.clone());
                    s.groups.retain(|g| {
                        g.path != original && !g.path.starts_with(&(original.clone() + "/"))
                    });
                    s.items.retain(|p| {
                        p.group != original && !p.group.starts_with(&(original.clone() + "/"))
                    });
                    if let Err(e) = s.save() {
                        s.groups = old;
                        s.items = items;
                        st.set_text(&e);
                    } else {
                        drop(s);
                        st.set_text("Group deleted; metadata backup retained");
                        refresh_widgets(parent.transient_for().as_ref());
                        parent.close();
                    }
                }
                c.close();
            });
            confirm.present();
            return;
        }
        if s.groups
            .iter()
            .enumerate()
            .any(|(i, g)| g.path == path && i + 1 != index as usize)
        {
            info.set_text("That full group path already exists");
            return;
        }
        let defaults = Preferences {
            display: ["fit", "native", "stretch", "dynamic"][mode.selected() as usize].into(),
            clipboard: clip.selected(),
            network: connection_fields[1].text().into(),
            gateway: connection_fields[2].text().into(),
            identity_file: connection_fields[3].text().into(),
            identity_secret_reference: connection_fields[4].text().into(),
            known_hosts: connection_fields[5].text().into(),
            trust: connection_fields[6].text().into(),
            ssh_jump_alias: connection_fields[7].text().into(),
            ..Preferences::default()
        };
        if !defaults.ssh_jump_alias.is_empty() {
            if let Err(e) = crate::ssh_jump::validate(&defaults.ssh_jump_alias) {
                info.set_text(&e);
                return;
            }
        }
        let username = connection_fields[0].text().to_string();
        let credential_profile = account_ids
            .get(account.selected().saturating_sub(1) as usize)
            .filter(|_| account.selected() > 0)
            .cloned()
            .unwrap_or_default();
        if r == gtk::ResponseType::Other(3) && index > 0 {
            let original = s.groups[(index - 1) as usize].path.clone();
            if path == original {
                info.set_text("Enter a distinct full path for the duplicate group");
                return;
            }
            let mut machines = std::collections::HashMap::new();
            for group in &old {
                if group.path == original || group.path.starts_with(&(original.clone() + "/")) {
                    let mut g = group.clone();
                    g.id = ruwac_protocol::id("group");
                    g.path = format!("{}{}", path, &g.path[original.len()..]);
                    s.groups.push(g);
                }
            }
            for item in &items {
                if item.group == original || item.group.starts_with(&(original.clone() + "/")) {
                    let mut p = item.clone();
                    p.id = ruwac_protocol::id("endpoint");
                    p.secret_reference = p.id.clone();
                    p.machine_id = machines
                        .entry(p.machine_id.clone())
                        .or_insert_with(|| ruwac_protocol::id("machine"))
                        .clone();
                    p.group = format!("{}{}", path, &p.group[original.len()..]);
                    s.items.push(p);
                }
            }
        } else if index == 0 {
            s.groups.push(Group {
                id: ruwac_protocol::id("group"),
                path,
                defaults,
                username,
                credential_profile,
            });
        } else {
            let prior = s.groups[(index - 1) as usize].path.clone();
            for p in &mut s.items {
                if p.group == prior || p.group.starts_with(&(prior.clone() + "/")) {
                    p.group = format!("{}{}", path, &p.group[prior.len()..]);
                }
            }
            for g in &mut s.groups {
                if g.path == prior || g.path.starts_with(&(prior.clone() + "/")) {
                    g.path = format!("{}{}", path, &g.path[prior.len()..]);
                }
            }
            s.groups[(index - 1) as usize].defaults = defaults;
            s.groups[(index - 1) as usize].username = username;
            s.groups[(index - 1) as usize].credential_profile = credential_profile;
        }
        if let Err(e) = s.save() {
            s.groups = old;
            s.items = items;
            info.set_text(&e);
            return;
        }
        drop(s);
        status.set_text("Network groups saved; explicit endpoint preferences retained");
        refresh_widgets(parent.as_ref());
        d.close();
    });
    dialog.present();
}

/// Drives the same native dialogs as the owner, only in the explicitly private fixture mode.
pub fn fixture_editor(root: &gtk::Box, q: &serde_json::Value) -> serde_json::Value {
    fn find(w: &gtk::Widget, label: &str) -> Option<gtk::Button> {
        if let Some(b) = w.downcast_ref::<gtk::Button>() {
            if b.label().as_deref() == Some(label) || b.tooltip_text().as_deref() == Some(label) {
                return Some(b.clone());
            }
        }
        if let Some(expander) = w.downcast_ref::<gtk::Expander>() {
            return expander.child().and_then(|child| find(&child, label));
        }
        let mut c = w.first_child();
        while let Some(w) = c {
            c = w.next_sibling();
            if let Some(b) = find(&w, label) {
                return Some(b);
            }
        }
        None
    }
    let action = q["operation"].as_str().unwrap_or("new");
    if action == "layout" {
        fn inspect(w: &gtk::Widget, cells: &mut Vec<serde_json::Value>, conflicts: &mut usize) {
            if w.widget_name() == "machine-editor-grid" {
                let grid = w.downcast_ref::<gtk::Grid>().unwrap();
                let mut occupied = std::collections::HashSet::new();
                let mut child = w.first_child();
                while let Some(c) = child {
                    child = c.next_sibling();
                    if !c.is_visible() {
                        continue;
                    }
                    let (col, row, width, height) = grid.query_child(&c);
                    for x in col..col + width {
                        for y in row..row + height {
                            if !occupied.insert((x, y)) {
                                *conflicts += 1;
                            }
                        }
                    }
                    cells.push(json!({"column":col,"row":row,"width":width,"height":height}));
                }
            }
            let mut child = w.first_child();
            while let Some(c) = child {
                child = c.next_sibling();
                inspect(&c, cells, conflicts);
            }
        }
        let mut cells = Vec::new();
        let mut conflicts = 0;
        let mut dialogs = Vec::new();
        for window in gtk::Window::list_toplevels() {
            inspect(&window, &mut cells, &mut conflicts);
            if let Some(d) = window.downcast_ref::<gtk::Dialog>() {
                if matches!(
                    d.title().as_deref(),
                    Some(
                        "Machine / connection properties"
                            | "Set up SSH terminal and agent shortcuts"
                            | "SSH login and key"
                    )
                ) {
                    fn bounds(
                        w: &gtk::Widget,
                        root: &gtk::Widget,
                        rows: &mut Vec<serde_json::Value>,
                    ) {
                        if w.is_mapped()
                            && (w.widget_name().starts_with("machine-field-")
                                || matches!(
                                    w.widget_name().as_str(),
                                    "endpoint-username"
                                        | "rdp-password"
                                        | "gnome-username"
                                        | "gnome-password"
                                        | "rdp-connection-mode"
                                ))
                        {
                            if let Some(b) = w.compute_bounds(root) {
                                rows.push(json!({"widget":w.widget_name().as_str(),"x":b.x(),"y":b.y(),"width":b.width(),"height":b.height()}));
                            }
                        }
                        let mut c = w.first_child();
                        while let Some(child) = c {
                            c = child.next_sibling();
                            bounds(&child, root, rows);
                        }
                    }
                    let mut rows = Vec::new();
                    bounds(d.upcast_ref(), d.content_area().upcast_ref(), &mut rows);
                    // Inspect only these non-secret booleans, including when
                    // their Advanced section is collapsed. The fixture can
                    // wait for the editor's actual snapshot after a writer.
                    fn selection_options(
                        w: &gtk::Widget,
                        values: &mut serde_json::Map<String, serde_json::Value>,
                    ) {
                        if matches!(w.widget_name().as_str(), "primary-sync" | "primary-paste") {
                            if let Some(button) = w.downcast_ref::<gtk::CheckButton>() {
                                values
                                    .insert(w.widget_name().to_string(), json!(button.is_active()));
                            }
                        }
                        // GtkExpander keeps its collapsed content outside the
                        // visible widget child tree; inspect its owned child.
                        if let Some(expander) = w.downcast_ref::<gtk::Expander>() {
                            if let Some(child) = expander.child() {
                                selection_options(&child, values);
                            }
                            return;
                        }
                        let mut child = w.first_child();
                        while let Some(w) = child {
                            child = w.next_sibling();
                            selection_options(&w, values);
                        }
                    }
                    let mut options = serde_json::Map::new();
                    selection_options(d.content_area().upcast_ref(), &mut options);
                    dialogs.push(json!({"width":d.content_area().width(),"height":d.content_area().height(),"fields":rows,"selection_options":options}));
                }
            }
        }
        return json!({"cells":cells,"overlapping_grid_cells":conflicts,"dialogs":dialogs});
    }
    if action == "group" {
        expand_sidebar_panel(root, "machines-tools-panel", true);
    }
    if action == "new" || action == "group" || action == "edit" {
        if let Some(b) = find(
            root.upcast_ref(),
            match action {
                "new" => "New machine / endpoint…",
                "group" => "Network groups / defaults…",
                _ => "Edit",
            },
        ) {
            b.emit_clicked();
        }
    }
    let title = if q["ssh_setup"] == true {
        "Set up SSH terminal and agent shortcuts"
    } else if q["group"].as_bool() == Some(true) || action == "group" {
        "Network groups and inherited defaults"
    } else {
        "Machine / connection properties"
    };
    let mut applied = false;
    let mut credential_status = None;
    for w in gtk::Window::list_toplevels() {
        if let Ok(d) = w.downcast::<gtk::Dialog>() {
            let named_editor = d.widget_name()
                == if q["ssh_setup"] == true {
                    "ssh-login-setup"
                } else {
                    "machine-connection-editor"
                };
            if d.title().as_deref() != Some(title)
                && !(title != "Network groups and inherited defaults" && named_editor)
            {
                continue;
            }
            if let Some(size) = q["window_size"].as_array() {
                if size.len() == 2 {
                    d.set_default_size(
                        size[0].as_i64().unwrap_or(540) as i32,
                        size[1].as_i64().unwrap_or(620) as i32,
                    );
                }
            }
            fn update(w: &gtk::Widget, q: &serde_json::Value) {
                if let Some(e) = w.downcast_ref::<gtk::Entry>() {
                    let widget_name = w.widget_name();
                    let row = match widget_name.as_str() {
                        "endpoint-username" => Some("4"),
                        "endpoint-secret-reference" => Some("9"),
                        "ssh-key-passphrase-reference" => Some("29"),
                        name => name.strip_prefix("machine-field-"),
                    };
                    if let Some(value) = row.and_then(|r| q["fields"][r].as_str()) {
                        e.set_text(value);
                    }
                }
                if let Some(row) = w.widget_name().strip_prefix("machine-field-") {
                    if let Some(d) = w.downcast_ref::<gtk::DropDown>() {
                        if let Some(value) = q["fields"][row].as_u64() {
                            d.set_selected(value as u32);
                        }
                    }
                    if let Some(c) = w.downcast_ref::<gtk::CheckButton>() {
                        if let Some(value) = q["fields"][row].as_bool() {
                            c.set_active(value);
                        }
                    }
                }
                if let Some(expander) = w.downcast_ref::<gtk::Expander>() {
                    if let Some(child) = expander.child() {
                        update(&child, q);
                    }
                    return;
                }
                if let Some(e) = w.downcast_ref::<gtk::Entry>() {
                    if let Some(v) = q["named"][w.widget_name().as_str()].as_str() {
                        e.set_text(v);
                    }
                }
                if let Some(c) = w.downcast_ref::<gtk::CheckButton>() {
                    if let Some(v) = q["named"][w.widget_name().as_str()].as_bool() {
                        c.set_active(v);
                    }
                }
                if let Some(d) = w.downcast_ref::<gtk::DropDown>() {
                    if w.widget_name() == "rdp-connection-mode" {
                        if let Some(v) = q["fields"]["26"].as_u64() {
                            d.set_selected(v as u32);
                        }
                    }
                    if let Some(v) = q["named"][w.widget_name().as_str()].as_u64() {
                        d.set_selected(v as u32);
                    }
                }
                if let Some(e) = w.downcast_ref::<gtk::PasswordEntry>() {
                    if let Some(v) = q["named"][w.widget_name().as_str()].as_str() {
                        e.set_text(v);
                    }
                }
                if w.widget_name() == "ssh-key-passphrase" {
                    if let Some(entry) = w.downcast_ref::<gtk::PasswordEntry>() {
                        if let Some(secret) = q["key_passphrase"].as_str() {
                            entry.set_text(secret);
                        }
                    }
                }
                if let Some(grid) = w.downcast_ref::<gtk::Grid>() {
                    let mut c = grid.first_child();
                    while let Some(child) = c {
                        c = child.next_sibling();
                        let (_, row, _, _) = grid.query_child(&child);
                        if let Some(e) = child.downcast_ref::<gtk::Entry>() {
                            if let Some(value) = q["fields"][row.to_string()].as_str() {
                                e.set_text(value);
                            }
                        }
                        if let Some(c) = child.downcast_ref::<gtk::CheckButton>() {
                            if let Some(value) = q["fields"][row.to_string()].as_bool() {
                                c.set_active(value);
                            }
                        }
                        if let Some(d) = child.downcast_ref::<gtk::DropDown>() {
                            if let Some(value) = q["fields"][row.to_string()].as_u64() {
                                d.set_selected(value as u32);
                            }
                        }
                    }
                }
                let mut c = w.first_child();
                while let Some(w) = c {
                    c = w.next_sibling();
                    update(&w, q);
                }
            }
            update(d.upcast_ref(), q);
            fn status(w: &gtk::Widget) -> Option<String> {
                if w.widget_name() == "profile-credential-status" {
                    return w.downcast_ref::<gtk::Label>().map(|l| l.text().to_string());
                }
                let mut child = w.first_child();
                while let Some(c) = child {
                    child = c.next_sibling();
                    if let Some(s) = status(&c) {
                        return Some(s);
                    }
                }
                None
            }
            credential_status = status(d.upcast_ref());
            if let Some(action) = q["credential_action"].as_str() {
                if let Some(button) = find(d.upcast_ref(), action) {
                    button.emit_clicked();
                }
            }
            if title.starts_with("Network") {
                fn group_fields(w: &gtk::Widget, q: &serde_json::Value) {
                    let name = w.widget_name();
                    if let Some(e) = w.downcast_ref::<gtk::Entry>() {
                        let value = if name == "group-path" {
                            q["path"].as_str()
                        } else {
                            q["defaults"][name.strip_prefix("group-").unwrap_or("")].as_str()
                        };
                        if let Some(value) = value {
                            e.set_text(value);
                        }
                    }
                    if let Some(d) = w.downcast_ref::<gtk::DropDown>() {
                        if name == "group-choice" {
                            if let Some(i) = q["index"].as_u64() {
                                d.set_selected(i as u32);
                            }
                        }
                        if name == "group-clipboard" {
                            if let Some(i) = q["defaults"]["clipboard"].as_u64() {
                                d.set_selected(i as u32);
                            }
                        }
                        if name == "group-display" {
                            if let Some(mode) = q["defaults"]["display"].as_str() {
                                if let Some(i) = ["fit", "native", "stretch", "dynamic"]
                                    .iter()
                                    .position(|v| *v == mode)
                                {
                                    d.set_selected(i as u32);
                                }
                            }
                        }
                        if name == "group-credential-profile" {
                            if let Some(id) = q["credential_profile"].as_str() {
                                if let Some(model) = d.model() {
                                    for i in 0..model.n_items() {
                                        if model
                                            .item(i)
                                            .and_then(|o| o.downcast::<gtk::StringObject>().ok())
                                            .is_some_and(|o| {
                                                o.string().ends_with(&format!(" · {id}"))
                                            })
                                        {
                                            d.set_selected(i);
                                            break;
                                        }
                                    }
                                }
                            }
                        }
                    }
                    let mut child = w.first_child();
                    while let Some(c) = child {
                        child = c.next_sibling();
                        group_fields(&c, q);
                    }
                }
                group_fields(d.content_area().upcast_ref(), q);
            }
            let response = match q["response"].as_str().unwrap_or("save") {
                "cancel" => gtk::ResponseType::Cancel,
                "connect" => gtk::ResponseType::Other(5),
                "duplicate" => gtk::ResponseType::Other(1),
                "endpoint" => gtk::ResponseType::Other(3),
                "delete" => gtk::ResponseType::Other(2),
                "delete-machine" => gtk::ResponseType::Other(4),
                _ => gtk::ResponseType::Accept,
            };
            if q["response"] != "keep" {
                d.response(response);
            }
            applied = true;
        }
    }
    if q["confirm"] == true {
        for w in gtk::Window::list_toplevels() {
            if let Ok(d) = w.downcast::<gtk::Dialog>() {
                if d.title().is_some_and(|s| s.starts_with("Delete ")) {
                    d.response(gtk::ResponseType::Accept);
                }
            }
        }
    }
    let s = Store::load();
    let effective = q["effective"]
        .as_str()
        .and_then(|id| s.items.iter().find(|p| p.id == id))
        .map(|p| resolve(p, &s.groups, &s.items));
    serde_json::json!({"applied":applied,"credential_status":credential_status,"items":s.items,"groups":s.groups,"error":s.error,"effective":effective})
}
#[cfg(test)]
mod workspace_tests {
    use super::*;
    #[test]
    fn migrate_thousand_preserve_identity_and_reject_corruption() {
        let original = preview_test();
        let legacy = serde_json::to_vec(&vec![original; 1000]).unwrap();
        let start = std::time::Instant::now();
        let mut items = parse_library(&legacy).unwrap();
        assert_eq!(items.len(), 1000);
        let ids: std::collections::HashSet<_> = items.iter().map(|p| p.id.clone()).collect();
        assert_eq!(ids.len(), 1000);
        let id = items[0].id.clone();
        let machine = items[0].machine_id.clone();
        items[0].name = "Łódź ž 🦀".into();
        items[0].group = "Organization/Region/Team".into();
        items[0].preferences.display = "native".into();
        items[0].preferences.zoom = 1.25;
        let bytes = serde_json::to_vec(&Library {
            schema: 1,
            items,
            groups: vec![],
        })
        .unwrap();
        assert!(bytes.len() < 1024 * 1024);
        let saved = parse_library(&bytes).unwrap();
        assert_eq!(saved[0].id, id);
        assert_eq!(saved[0].machine_id, machine);
        assert_eq!(saved[0].preferences.zoom, 1.25);
        assert!(parse_library(br#"{"schema":9,"items":[]}"#).is_err());
        assert!(parse_library(b"{broken").is_err());
        let mut duplicate = saved.clone();
        duplicate[1].id = duplicate[0].id.clone();
        assert!(parse_library(&serde_json::to_vec(&duplicate).unwrap()).is_err());
        println!(
            "1000-profile migration/search/serialization {} ms",
            start.elapsed().as_millis()
        );
        assert_eq!(saved.iter().filter(|p| p.matches("Team")).count(), 1);
    }
    #[test]
    fn bulk_metadata_import_is_idempotent_and_enforces_the_limit() {
        let mut saved = preview_test();
        saved.identify();
        saved.secret_reference = "generated-existing-reference".into();
        let mut second = preview_test();
        second.name = "Generated second".into();
        let plan =
            bulk_import_plan(&[saved.clone()], &[preview_test(), second.clone(), second]).unwrap();
        assert_eq!(plan.len(), 2);
        assert_eq!(plan[0].id, saved.id);
        assert_eq!(plan[0].secret_reference, saved.secret_reference);
        assert_eq!(plan[1].secret_reference, plan[1].id);
        let many: Vec<_> = (0..1001)
            .map(|i| {
                let mut p = preview_test();
                p.name = format!("Generated {i}");
                p
            })
            .collect();
        assert!(bulk_import_plan(&[], &many).is_err());
    }
    fn preview_test() -> Profile {
        Profile {
            id: String::new(),
            machine_id: String::new(),
            preferences: Preferences::default(),
            inherit: false,
            inherit_credential: false,
            inherit_connection: false,
            name: "server".into(),
            group: "network".into(),
            protocol: "RDP".into(),
            server: "localhost:3389".into(),
            username: "fixture".into(),
            secret_reference: String::new(),
            favorite: false,
            last_used: 0,
            tags: vec!["generated".into()],
        }
    }
    #[test]
    fn invalid_preferences_and_ports() {
        let mut p = preview_test();
        p.server = "localhost:0".into();
        assert!(p.validate().is_err());
        p.server = "localhost:22".into();
        p.preferences.zoom = f64::NAN;
        assert!(p.validate().is_err());
    }
}

/// Native library controls exercised only by the private fixture dispatcher.
pub fn fixture_library(
    root: &gtk::Box,
    file: Option<&Path>,
    q: &serde_json::Value,
) -> serde_json::Value {
    fn find(w: &gtk::Widget, name: &str) -> Option<gtk::Widget> {
        if w.widget_name() == name {
            return Some(w.clone());
        }
        let mut child = w.first_child();
        while let Some(c) = child {
            child = c.next_sibling();
            if let Some(w) = find(&c, name) {
                return Some(w);
            }
        }
        None
    }
    if let Some(file) = file {
        expand_sidebar_panel(root, "machines-import-panel", true);
        if let Some(entry) = find(root.upcast_ref(), "machines-import-path")
            .and_then(|w| w.downcast::<gtk::Entry>().ok())
        {
            entry.set_text(&file.to_string_lossy());
        }
        if let Some(button) = find(root.upcast_ref(), "machines-import-preview")
            .and_then(|w| w.downcast::<gtk::Button>().ok())
        {
            button.emit_clicked();
        }
    }
    if let Some(enabled) = q["import_passwords"].as_bool() {
        if let Some(button) = find(root.upcast_ref(), "machines-import-passwords")
            .and_then(|w| w.downcast::<gtk::CheckButton>().ok())
        {
            button.set_active(enabled);
        }
    }
    if let Some(index) = q["discovered_index"].as_u64() {
        expand_sidebar_panel(root, "machines-import-panel", true);
        if let Some(dropdown) = find(root.upcast_ref(), "machines-import-discovered")
            .and_then(|w| w.downcast::<gtk::DropDown>().ok())
        {
            dropdown.set_selected(index.saturating_add(1).min(u32::MAX as u64) as u32);
        }
    }
    if let Some(expanded) = q["import_expanded"].as_bool() {
        expand_sidebar_panel(root, "machines-import-panel", expanded);
    }
    if let Some(expanded) = q["tools_expanded"].as_bool() {
        expand_sidebar_panel(root, "machines-tools-panel", expanded);
    }
    if let Some(id) = q["target"].as_str() {
        if let Some(w) = find(root.upcast_ref(), "machines-merge-target") {
            let dropdown = w.downcast::<gtk::DropDown>().unwrap();
            let model = dropdown.model().unwrap();
            let selected = (1..model.n_items()).find(|i| {
                model
                    .item(*i)
                    .and_then(|o| o.downcast::<gtk::StringObject>().ok())
                    .is_some_and(|o| o.string().ends_with(id))
            });
            dropdown.set_selected(selected.unwrap_or(0));
        }
    }
    let mut group_states = Vec::new();
    if let Some(list) = find(root.upcast_ref(), "machines-list-box") {
        let mut child = list.first_child();
        while let Some(widget) = child {
            child = widget.next_sibling();
            if let Some(group) = widget.downcast_ref::<gtk::Expander>() {
                if q["group"].as_str() == group.label().as_deref() {
                    if let Some(expanded) = q["expanded"].as_bool() {
                        group.set_expanded(expanded);
                    }
                }
                group_states.push(json!({"label":group.label().map(|l|l.to_string()),"expanded":group.is_expanded()}));
            }
        }
    }
    let action = q["operation"].as_str().unwrap_or("inspect");
    if action == "undo" {
        expand_sidebar_panel(root, "machines-tools-panel", true);
    }
    let name = match action {
        "next" => "machines-page-next",
        "previous" => "machines-page-previous",
        "import" => "machines-import-apply",
        "import-all" => "machines-import-all",
        "review" => "machines-merge-review",
        "undo" => "machines-undo",
        _ => "",
    };
    if let Some(w) = find(root.upcast_ref(), name) {
        if let Some(b) = w.downcast_ref::<gtk::Button>() {
            if b.is_sensitive() {
                b.emit_clicked();
            }
        }
    }
    let mut review_text = None;
    for window in gtk::Window::list_toplevels() {
        if window.widget_name() == "machines-merge-dialog" {
            if let Ok(dialog) = window.downcast::<gtk::Dialog>() {
                review_text = dialog
                    .content_area()
                    .first_child()
                    .and_then(|w| w.downcast::<gtk::Label>().ok())
                    .map(|l| l.text().to_string());
                if action == "apply" || action == "cancel" {
                    dialog.response(if action == "apply" {
                        gtk::ResponseType::Accept
                    } else {
                        gtk::ResponseType::Cancel
                    });
                }
            }
        }
    }
    if let Some(response) = q["bulk_response"].as_str() {
        for window in gtk::Window::list_toplevels() {
            if let Ok(dialog) = window.downcast::<gtk::Dialog>() {
                if dialog.widget_name() == "machines-import-all-dialog" {
                    dialog.response(if response == "import" {
                        gtk::ResponseType::Accept
                    } else {
                        gtk::ResponseType::Cancel
                    });
                }
            }
        }
    }
    let text = |name| {
        find(root.upcast_ref(), name)
            .and_then(|w| w.downcast::<gtk::Label>().ok())
            .map(|l| l.text().to_string())
    };
    let sensitive = |name| find(root.upcast_ref(), name).is_some_and(|w| w.is_sensitive());
    let panel = |name| {
        find(root.upcast_ref(), name)
            .and_then(|w| w.downcast::<gtk::Expander>().ok())
            .is_some_and(|w| w.is_expanded())
    };
    let list_height = find(root.upcast_ref(), "machines-list-scroll")
        .map(|w| w.height())
        .unwrap_or(0);
    let import_outside_scroll =
        find(root.upcast_ref(), "machines-import-apply").is_some_and(|button| {
            let mut parent = button.parent();
            while let Some(w) = parent {
                if w.is::<gtk::ScrolledWindow>() {
                    return false;
                }
                parent = w.parent();
            }
            true
        });
    let discovered_count = find(root.upcast_ref(), "machines-import-discovered")
        .and_then(|w| w.downcast::<gtk::DropDown>().ok())
        .and_then(|w| w.model())
        .map(|m| m.n_items().saturating_sub(1))
        .unwrap_or(0);
    json!({"groups":group_states,"import_outside_scroll":import_outside_scroll,"discovered_count":discovered_count,"import_enabled":sensitive("machines-import-apply"),"import_expanded":panel("machines-import-panel"),"tools_expanded":panel("machines-tools-panel"),"list_height":list_height,"sidebar_height":root.height(),"page":text("machines-page-label"),"previous":sensitive("machines-page-previous"),"next":sensitive("machines-page-next"),"status":text("machines-status"),"review_text":review_text,"visible":fixture(root,None,None,false,false)})
}
#[cfg(test)]
mod merge_tests {
    use super::*;
    #[test]
    fn import_merge_preserves_identity_secrets_and_local_policy() {
        let imported:Profile=serde_json::from_value(json!({"name":"Imported","group":"New/Group","protocol":"SSH","server":"host.example:22","username":"other","secret_reference":"IMPORTED-IGNORED","favorite":false,"last_used":0,"tags":["remote"]})).unwrap();
        let mut current = imported.clone();
        current.identify();
        current.name = "Old".into();
        current.machine_id = "machine_original".into();
        current.secret_reference = "existing-secret-ref".into();
        current.favorite = true;
        current.last_used = 123;
        current.inherit = true;
        current.preferences.trust = "existing-trust".into();
        current.preferences.identity_file = "existing-key".into();
        let merged = merge_metadata(&current, &imported).unwrap();
        assert_eq!(merged.id, current.id);
        assert_eq!(merged.machine_id, current.machine_id);
        assert_eq!(merged.secret_reference, current.secret_reference);
        assert_eq!(
            serde_json::to_value(&merged.preferences).unwrap(),
            serde_json::to_value(&current.preferences).unwrap()
        );
        assert!(merged.favorite && merged.inherit);
        assert_eq!(merged.last_used, 123);
        assert_eq!(merged.name, "Imported");
        let mut bad = imported;
        bad.server = "-bad".into();
        assert!(merge_metadata(&current, &bad).is_err());
        assert_eq!(current.name, "Old");
    }
}

#[cfg(test)]
mod inherited_tests {
    use super::*;
    fn profile() -> Profile {
        serde_json::from_value(json!({"id":"endpoint_target","machine_id":"machine_target","name":"Target","group":"Network/Child","protocol":"SSH","server":"localhost","username":"endpoint-user","secret_reference":"endpoint-secret-reference","favorite":false,"last_used":0,"tags":[]})).unwrap()
    }
    fn group(path: &str) -> Group {
        Group {
            id: format!("group_{}", path.replace('/', "_")),
            path: path.into(),
            defaults: Preferences::default(),
            username: String::new(),
            credential_profile: String::new(),
        }
    }
    #[test]
    fn ssh_review_lists_distinct_saved_accounts_without_mutating_sources() {
        let first = profile();
        let mut second = first.clone();
        second.id = "endpoint_second".into();
        second.username = "second-account".into();
        second.preferences.identity_file = "/generated/manual-key".into();
        let mut rdp = first.clone();
        rdp.id = "endpoint_rdp".into();
        rdp.protocol = "RDP".into();
        let store = Store {
            items: vec![first, second, rdp],
            ..Store::default()
        };
        let before = ssh_review_snapshot(&store);
        let review = ssh_review_targets(&store);
        assert_eq!(review.len(), 2);
        assert!(review.iter().all(|t| !t.new));
        assert_eq!(review[1].probe.username, "second-account");
        assert_eq!(
            review[1].probe.preferences.identity_file,
            "/generated/manual-key"
        );
        assert_eq!(before, ssh_review_snapshot(&store));
    }
    #[test]
    fn ssh_review_seed_preserves_manual_route_without_reusing_rdp_credentials() {
        let mut rdp = profile();
        rdp.protocol = "RDP".into();
        rdp.server = "generated-alias:3389".into();
        rdp.username = "rdp-service-only".into();
        rdp.preferences.ssh_jump_alias = "generated-jump".into();
        rdp.preferences.identity_file = "/generated/manual-key".into();
        let store = Store {
            items: vec![rdp.clone()],
            ..Store::default()
        };
        let before = ssh_review_snapshot(&store);
        let review = ssh_review_targets(&store);
        assert_eq!(review.len(), 1);
        let target = &review[0];
        assert!(target.new);
        assert!(target.probe.username.is_empty());
        assert_eq!(target.source_id, rdp.id);
        assert_ne!(target.probe.secret_reference, rdp.secret_reference);
        assert_eq!(target.probe.preferences.ssh_jump_alias, "generated-jump");
        assert_eq!(
            target.probe.preferences.identity_file,
            "/generated/manual-key"
        );
        assert_eq!(target.probe.server, "generated-alias");
        assert_eq!(before, ssh_review_snapshot(&store));
    }
    #[test]
    fn explicit_jump_inherits_only_for_ssh_and_endpoint_override_wins() {
        let mut p = profile();
        p.inherit_connection = true;
        let mut g = group("Network");
        g.defaults.ssh_jump_alias = "generated-bastion".into();
        let result = resolve(&p, &[g.clone()], &[]).unwrap();
        assert_eq!(result.preferences.ssh_jump_alias, "generated-bastion");
        assert!(p.preferences.ssh_jump_alias.is_empty());
        p.preferences.ssh_jump_alias = "generated-override".into();
        assert_eq!(
            resolve(&p, &[g.clone()], &[])
                .unwrap()
                .preferences
                .ssh_jump_alias,
            "generated-override"
        );
        p.preferences.ssh_jump_alias.clear();
        p.protocol = "RDP".into();
        assert!(
            resolve(&p, &[g], &[])
                .unwrap()
                .preferences
                .ssh_jump_alias
                .is_empty()
        );
    }
    #[test]
    fn legacy_gateway_metadata_does_not_enable_a_jump_route() {
        let mut p = profile();
        p.preferences.gateway = "stored-descriptive-metadata".into();
        assert!(p.validate().is_ok());
        assert!(p.preferences.ssh_jump_alias.is_empty());
        assert!(
            !serde_json::to_string(&p)
                .unwrap()
                .contains("ssh_jump_alias")
        );
        assert!(
            !ssh_argv(&p)
                .unwrap()
                .iter()
                .any(|v| v.starts_with("ProxyCommand="))
        );
        p.preferences.ssh_jump_alias = "generated-bastion".into();
        assert!(
            ssh_argv(&p)
                .unwrap()
                .iter()
                .any(|v| v.starts_with("ProxyCommand="))
        );
        let round: Profile = serde_json::from_slice(&serde_json::to_vec(&p).unwrap()).unwrap();
        assert_eq!(
            round.preferences.ssh_jump_alias,
            p.preferences.ssh_jump_alias
        );
        p.preferences.ssh_jump_alias = "-bad".into();
        assert!(p.validate().is_err());
    }
    #[test]
    fn ancestors_fill_empty_fields_with_nearest_values_and_path_boundaries() {
        let mut p = profile();
        p.inherit = true;
        p.inherit_connection = true;
        p.username.clear();
        let mut parent = group("Network");
        parent.username = "parent-user".into();
        parent.defaults.network = "parent-network".into();
        let mut child = group("Network/Child");
        child.username = "child-user".into();
        child.defaults.display = "stretch".into();
        child.defaults.clipboard = 3;
        let mut neighbor = group("Network/Child-other");
        neighbor.username = "wrong-user".into();
        let result = resolve(&p, &[neighbor, child, parent], &[]).unwrap();
        assert_eq!(result.username, "child-user");
        assert_eq!(result.preferences.network, "parent-network");
        assert_eq!(result.preferences.display, "stretch");
        assert_eq!(result.preferences.clipboard, 3);
    }
    #[test]
    fn endpoint_overrides_and_key_passphrase_pairs_are_preserved() {
        let mut p = profile();
        p.inherit_connection = true;
        p.preferences.identity_file = "endpoint-key".into();
        p.preferences.known_hosts = "endpoint-known".into();
        p.preferences.trust = "endpoint-ca".into();
        p.preferences.network = "endpoint-network".into();
        let original = serde_json::to_value(&p).unwrap();
        let mut g = group("Network");
        g.username = "group-user".into();
        g.defaults.identity_file = "group-key".into();
        g.defaults.identity_secret_reference = "group-passphrase-reference".into();
        g.defaults.known_hosts = "group-known".into();
        g.defaults.trust = "group-ca".into();
        g.defaults.network = "group-network".into();
        let result = resolve(&p, &[g], &[]).unwrap();
        assert_eq!(result.username, p.username);
        assert_eq!(result.preferences.identity_file, "endpoint-key");
        assert!(result.preferences.identity_secret_reference.is_empty());
        assert_eq!(result.preferences.known_hosts, "endpoint-known");
        assert_eq!(result.preferences.trust, "endpoint-ca");
        assert_eq!(result.preferences.network, "endpoint-network");
        assert_eq!(serde_json::to_value(&p).unwrap(), original);
    }
    #[test]
    fn inherited_ssh_key_and_reference_are_selected_as_one_pair() {
        let mut p = profile();
        p.inherit_connection = true;
        let mut parent = group("Network");
        parent.defaults.identity_file = "parent-key".into();
        parent.defaults.identity_secret_reference = "parent-reference".into();
        let mut child = group("Network/Child");
        child.defaults.identity_file = "child-key".into();
        let result = resolve(&p, &[parent, child], &[]).unwrap();
        assert_eq!(result.preferences.identity_file, "child-key");
        assert!(result.preferences.identity_secret_reference.is_empty());
    }
    #[test]
    fn shared_account_uses_only_existing_reference_without_mutating_metadata() {
        let mut p = profile();
        p.username.clear();
        p.inherit_connection = true;
        p.inherit_credential = true;
        let original = serde_json::to_value(&p).unwrap();
        let mut account = profile();
        account.id = "endpoint_account".into();
        account.username = "account-user".into();
        account.secret_reference = "generated-shared-reference".into();
        let mut g = group("Network");
        g.credential_profile = account.id.clone();
        let result = resolve(&p, &[g], &[account]).unwrap();
        assert_eq!(result.username, "account-user");
        assert_eq!(result.secret_reference, "generated-shared-reference");
        assert_eq!(serde_json::to_value(&p).unwrap(), original);
    }
    #[test]
    fn missing_account_and_disabled_inheritance_refuse_credential_fallback() {
        let mut p = profile();
        p.inherit_connection = true;
        p.inherit_credential = true;
        let mut g = group("Network");
        g.credential_profile = "removed-account".into();
        assert!(resolve(&p, &[g], &[]).is_err());
        p.inherit_connection = false;
        assert!(resolve(&p, &[], &[]).is_err());
    }
    #[test]
    fn disabled_inheritance_keeps_existing_connection_settings() {
        let p = profile();
        let mut g = group("Network");
        g.username = "group-user".into();
        g.defaults.trust = "group-ca".into();
        g.defaults.display = "native".into();
        assert_eq!(
            serde_json::to_value(resolve(&p, &[g], &[]).unwrap()).unwrap(),
            serde_json::to_value(p).unwrap()
        );
    }
    #[test]
    fn alias_network_and_protocol_search_use_unicode_lowercase() {
        let mut p = profile();
        p.preferences.label = "WínAlias".into();
        p.preferences.network = "Organization/Živjo".into();
        assert!(p.matches("WÍNALIAS"));
        assert!(p.matches("živjo"));
        assert!(p.matches("ssh"));
        assert!(!p.matches("%"));
    }
}

/// Explicit headless setup for an existing RDP profile; never reads credentials.
pub(crate) fn configure_dynamic_rdp() -> Result<usize, String> {
    let mut store = Store::load();
    if let Some(error) = &store.error {
        return Err(error.clone());
    }
    let mut changed = 0;
    for profile in store.items.iter_mut().filter(|p| p.protocol == "RDP") {
        if profile.preferences.display != "dynamic" {
            profile.preferences.display = "dynamic".into();
            changed += 1;
        }
    }
    // A profile may inherit its display setting from its group.
    for group in &mut store.groups {
        if group.defaults.display != "dynamic" {
            group.defaults.display = "dynamic".into();
            changed += 1;
        }
    }
    if changed > 0 {
        store.save()?;
    }
    Ok(changed)
}

pub(crate) fn configure_primary(
    id: &str,
    user: &str,
    display: &str,
    port: u16,
    enabled: bool,
) -> Result<(), String> {
    let mut store = Store::load();
    if let Some(error) = &store.error {
        return Err(error.clone());
    }
    let profile = store
        .items
        .iter_mut()
        .find(|p| p.id == id && p.protocol == "RDP")
        .ok_or("Choose an existing RDP endpoint ID")?;
    profile.preferences.primary_sync = enabled;
    if !enabled {
        profile.preferences.primary_paste = false;
    }
    if enabled {
        profile.preferences.primary_user = user.into();
        profile.preferences.primary_display = display.into();
        profile.preferences.primary_port = port;
    }
    profile.validate()?;
    store.save()
}

/// Reload saved options for explicit reconnect, without retargeting credentials.
pub(crate) fn saved_reconnect_profile(expected: &Profile) -> Result<Option<Profile>, String> {
    let store = Store::load();
    if let Some(error) = store.error {
        return Err(error);
    }
    let Some(saved) = store.items.iter().find(|p| p.id == expected.id) else {
        return Ok(None);
    };
    let fresh = resolve(saved, &store.groups, &store.items)?;
    if fresh.protocol != expected.protocol
        || fresh.server != expected.server
        || fresh.username != expected.username
        || fresh.secret_reference != expected.secret_reference
        || fresh.preferences.trust != expected.preferences.trust
    {
        return Err("Saved connection identity changed; reopen it from Machines".into());
    }
    Ok(Some(fresh))
}

/// Stop selection helpers when their saved account, route or desktop changes.
pub(crate) fn primary_profile_current(expected: &Profile) -> bool {
    let store = Store::load();
    store
        .items
        .iter()
        .find(|p| p.id == expected.id)
        .and_then(|p| resolve(p, &store.groups, &store.items).ok())
        .is_some_and(|p| {
            let a = &p.preferences;
            let b = &expected.preferences;
            p.protocol == expected.protocol
                && p.server == expected.server
                && a.primary_sync
                && a.primary_paste == b.primary_paste
                && a.primary_user == b.primary_user
                && a.primary_port == b.primary_port
                && a.primary_display == b.primary_display
                && a.ssh_jump_alias == b.ssh_jump_alias
                && a.gateway == b.gateway
                && a.identity_file == b.identity_file
                && a.known_hosts == b.known_hosts
                && a.trust == b.trust
        })
}

/// Revalidate a saved endpoint before an explicit sensitive input operation.
pub(crate) fn assist_profile_current(expected: &Profile) -> bool {
    let store = Store::load();
    store
        .items
        .iter()
        .find(|p| p.id == expected.id)
        .and_then(|p| resolve(p, &store.groups, &store.items).ok())
        .is_some_and(|p| {
            p.server == expected.server
                && p.username == expected.username
                && p.secret_reference == expected.secret_reference
                && p.preferences.rdp_mode == expected.preferences.rdp_mode
                && p.preferences.gnome_auto_login == expected.preferences.gnome_auto_login
                && p.preferences.gnome_username == expected.preferences.gnome_username
                && p.preferences.gnome_secret_reference
                    == expected.preferences.gnome_secret_reference
                && p.preferences.trust == expected.preferences.trust
        })
}

#[cfg(test)]
mod dual_login_tests {
    use super::*;
    fn legacy() -> Profile {
        serde_json::from_value(json!({"id":"endpoint_dual","machine_id":"machine_dual","name":"Fedora","group":"","protocol":"RDP","server":"localhost:3389","username":"service-user","secret_reference":"service-ref","favorite":false,"last_used":0,"tags":[]})).unwrap()
    }
    #[test]
    fn legacy_windows_roundtrip_has_no_second_credential() {
        let p = legacy();
        p.validate().unwrap();
        let v = serde_json::to_value(p).unwrap();
        assert!(v["preferences"].get("rdp_mode").is_none());
        assert!(v["preferences"].get("gnome_secret_reference").is_none());
        assert!(v["preferences"].get("gnome_auto_login").is_none());
    }
    #[test]
    fn independent_gnome_credentials_roundtrip() {
        let mut p = legacy();
        p.preferences.rdp_mode = "gnome-remote-login".into();
        p.preferences.gnome_username = "linux-user".into();
        p.preferences.gnome_secret_reference = "endpoint_dual.gnome".into();
        p.preferences.gnome_auto_login = true;
        p.validate().unwrap();
        let copy: Profile = serde_json::from_str(&serde_json::to_string(&p).unwrap()).unwrap();
        assert!(copy.preferences.gnome_auto_login);
        assert_eq!(copy.username, "service-user");
        assert_eq!(copy.secret_reference, "service-ref");
        assert_eq!(copy.preferences.gnome_username, "linux-user");
        assert_eq!(
            copy.preferences.gnome_secret_reference,
            "endpoint_dual.gnome"
        );
        assert_eq!(copy.id, p.id);
    }
    #[test]
    fn collisions_invalid_modes_and_control_characters_rejected() {
        let mut p = legacy();
        p.preferences.rdp_mode = "autologin".into();
        assert!(p.validate().is_err());
        p.preferences.rdp_mode = "gnome-remote-login".into();
        p.preferences.gnome_secret_reference = p.secret_reference.clone();
        assert!(p.validate().is_err());
        p.preferences.gnome_secret_reference = "endpoint_dual.gnome".into();
        p.preferences.gnome_username = "user\npassword".into();
        assert!(p.validate().is_err());
    }
    #[test]
    fn remmina_merge_keeps_second_stage_and_identity() {
        let mut p = legacy();
        p.preferences.rdp_mode = "gnome-remote-login".into();
        p.preferences.gnome_username = "linux-user".into();
        p.preferences.gnome_secret_reference = "endpoint_dual.gnome".into();
        let mut imported = legacy();
        imported.username = "different-service".into();
        let merged = merge_metadata(&p, &imported).unwrap();
        assert_eq!(merged.preferences.gnome_username, "linux-user");
        assert_eq!(
            merged.preferences.gnome_secret_reference,
            p.preferences.gnome_secret_reference
        );
        assert_eq!(merged.secret_reference, p.secret_reference);
        assert_eq!(merged.id, p.id);
    }
}

#[cfg(test)]
mod cross_stage_library_tests {
    use super::*;
    #[test]
    fn second_stage_cannot_alias_another_endpoints_service() {
        let first:Profile=serde_json::from_value(json!({"id":"endpoint_one","machine_id":"machine_one","name":"one","group":"","protocol":"RDP","server":"localhost","username":"service","secret_reference":"shared-service","favorite":false,"last_used":0,"tags":[]})).unwrap();
        let mut second = first.clone();
        second.id = "endpoint_two".into();
        second.secret_reference = "other-service".into();
        second.preferences.gnome_secret_reference = "shared-service".into();
        assert!(
            parse_library(
                &serde_json::to_vec(&Library {
                    schema: 1,
                    items: vec![first, second],
                    groups: vec![]
                })
                .unwrap()
            )
            .is_err()
        );
    }
}

#[cfg(test)]
mod discovery_tests {
    use super::*;
    #[test]
    fn native_legacy_flatpak_discovery_is_read_only_and_excludes_links() {
        use std::os::unix::fs::symlink;
        let root =
            std::env::temp_dir().join(format!("ruwac-remmina-discovery-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let home = root.join("home");
        let data = root.join("data");
        let dirs = [
            data.join("remmina"),
            home.join(".remmina"),
            home.join(".var/app/org.remmina.Remmina/data/remmina"),
        ];
        let text = "[remmina]\nname=Generated profile\nprotocol=RDP\nserver=127.0.0.1\npassword=GENERATED-NOT-IMPORTED\n";
        for (i, dir) in dirs.iter().enumerate() {
            std::fs::create_dir_all(dir).unwrap();
            std::fs::write(dir.join(format!("{i}.remmina")), text).unwrap();
        }
        symlink(dirs[0].join("0.remmina"), dirs[0].join("linked.remmina")).unwrap();
        std::fs::write(dirs[0].join("bad.remmina"), "invalid").unwrap();
        let found = discover_remmina(&home, &data);
        assert_eq!(found.len(), 3);
        for (path, _) in found {
            assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
            let p = preview(&path).unwrap();
            assert!(p.secret_reference.is_empty());
        }
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[cfg(test)]
mod launch_preferences_tests {
    use super::*;
    #[test]
    fn new_rdp_defaults_dynamic_but_saved_modes_survive() {
        assert_eq!(Preferences::default().display, "dynamic");
        let p: Preferences = serde_json::from_str(r#"{"display":"fit"}"#).unwrap();
        assert_eq!(p.display, "fit");
        assert!(p.codex_command.is_empty() && p.claude_command.is_empty());
    }
}

#[cfg(test)]
mod host_shortcut_tests {
    use super::*;
    #[test]
    fn ssh_setup_keeps_machine_and_commands_but_never_rdp_credentials() {
        let mut source: Profile = serde_json::from_value(serde_json::json!({"id":"endpoint_demo", "machine_id":"machine_demo", "name":"Demo", "group":"Test", "protocol":"RDP", "server":"localhost:3389", "username":"service-user", "secret_reference":"service-reference", "favorite":false, "last_used":0, "tags":[], "preferences":{"gnome_username":"linux-user","gnome_secret_reference":"linux-reference","codex_command":"codex resume --all", "claude_command":"claude --resume", "trust":"system"}})).unwrap();
        source.inherit_credential = true;
        let target = ssh_seed(&source);
        assert_eq!(target.machine_id, source.machine_id);
        assert_ne!(target.id, source.id);
        assert_eq!(target.protocol, "SSH");
        assert_eq!(target.server, "localhost");
        assert_eq!(target.username, gtk::glib::user_name().to_string_lossy());
        assert_eq!(target.secret_reference, target.id);
        assert!(target.preferences.gnome_secret_reference.is_empty());
        assert!(!target.inherit_credential);
        assert_eq!(
            target.preferences.codex_command,
            source.preferences.codex_command
        );
        assert_eq!(
            target.preferences.claude_command,
            source.preferences.claude_command
        );
        source.preferences.gnome_username.clear();
        assert!(!ssh_seed(&source).username.is_empty());
        source.server = "[::1]:3389".into();
        assert_eq!(ssh_seed(&source).server, "[::1]");
    }
}

/// Explicit send opt-in; the existing reviewed receiver supplies the same SSH route.
pub(crate) fn configure_primary_paste(id: &str) -> Result<(), String> {
    let mut store = Store::load();
    if let Some(error) = &store.error {
        return Err(error.clone());
    }
    let profile = store
        .items
        .iter_mut()
        .find(|p| p.id == id && p.protocol == "RDP")
        .ok_or("Choose an existing RDP endpoint ID")?;
    if !profile.preferences.primary_sync {
        return Err("Configure this desktop's Linux selection-sharing SSH route first".into());
    }
    profile.preferences.primary_paste = true;
    profile.validate()?;
    store.save()
}
