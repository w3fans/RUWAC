//! Private launch identities, independent of window layout and display names.
use crate::profiles::Profile;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{self, Read, Write},
    os::fd::AsRawFd,
    os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
};

const LIMIT: usize = 512;
const MAX_BYTES: u64 = 16384;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Recipe {
    Ssh,
    Agent {
        provider: String,
        command: String,
        backend: String,
        session: String,
        binary: String,
        socket: String,
        install_agent: bool,
        install_ruwac: bool,
    },
}
impl Recipe {
    pub fn agent(profile: &Profile, kind: u32, binary: &str, socket: &str) -> io::Result<Self> {
        let (provider, configured, default) = match kind {
            0 => (
                "codex",
                &profile.preferences.codex_command,
                "codex resume --all",
            ),
            1 => (
                "claude",
                &profile.preferences.claude_command,
                "claude --resume",
            ),
            5 => ("kimi", &profile.preferences.kimi_command, "kimi --session"),
            _ => return Err(io::Error::other("This action is not an agent launch")),
        };
        let recipe = Self::Agent {
            provider: provider.into(),
            command: if configured.is_empty() {
                default.into()
            } else {
                configured.clone()
            },
            backend: profile.preferences.agent_backend.clone(),
            session: profile.preferences.agent_session.clone(),
            binary: binary.into(),
            socket: socket.into(),
            install_agent: profile.preferences.agent_install_missing,
            install_ruwac: profile.preferences.ruwac_install_missing,
        };
        recipe.validate()?;
        Ok(recipe)
    }
    fn validate(&self) -> io::Result<()> {
        if let Self::Agent {
            provider,
            command,
            backend,
            session,
            binary,
            socket,
            ..
        } = self
        {
            if !["codex", "claude", "kimi"].contains(&provider.as_str())
                || !["", "ruwac", "screen", "tmux", "direct"].contains(&backend.as_str())
                || binary.is_empty()
                || command.is_empty()
                || command.len() > 1024
                || [session, binary, socket].iter().any(|s| s.len() > 512)
                || [provider, command, backend, session, binary, socket]
                    .iter()
                    .any(|s| s.chars().any(char::is_control))
            {
                return Err(io::Error::other("Invalid saved launch recipe"));
            }
        }
        Ok(())
    }
    pub fn provider(&self) -> Option<&str> {
        match self {
            Self::Ssh => None,
            Self::Agent { provider, .. } => Some(provider),
        }
    }
    pub fn argv(&self, profile: &Profile) -> Result<Vec<String>, String> {
        self.validate().map_err(|e| e.to_string())?;
        match self {
            Self::Ssh => crate::profiles::terminal_argv(profile)
                .map(|a| crate::profiles::session_argv(profile, a)),
            Self::Agent {
                provider,
                command,
                backend,
                session,
                binary,
                socket,
                install_agent,
                install_ruwac,
            } => {
                let mut p = profile.clone();
                let kind = match provider.as_str() {
                    "codex" => {
                        p.preferences.codex_command = command.clone();
                        0
                    }
                    "claude" => {
                        p.preferences.claude_command = command.clone();
                        1
                    }
                    _ => {
                        p.preferences.kimi_command = command.clone();
                        5
                    }
                };
                p.preferences.agent_backend = backend.clone();
                p.preferences.agent_session = session.clone();
                p.preferences.agent_install_missing = *install_agent;
                p.preferences.ruwac_install_missing = *install_ruwac;
                crate::agent_launch::argv(&p, kind, binary, socket)
                    .map(|a| crate::profiles::session_argv(&p, a))
            }
        }
    }
}

/// Only the connection identity is fingerprinted; no saved password/reference,
/// account, address or generated SSH argv is duplicated into this catalog.
pub fn binding(p: &Profile) -> String {
    let v = serde_json::json!([
        p.id,
        p.machine_id,
        p.protocol,
        p.server,
        p.username,
        p.secret_reference,
        p.preferences.gateway,
        p.preferences.ssh_jump_alias,
        p.preferences.ssh_auth,
        p.preferences.identity_file,
        p.preferences.identity_secret_reference,
        p.preferences.known_hosts
    ]);
    format!("{:x}", Sha256::digest(v.to_string().as_bytes()))
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    pub schema: u8,
    pub daemon: String,
    pub session: String,
    pub pane: String,
    pub window: String,
    pub pid: u32,
    pub endpoint: String,
    pub binding: String,
    pub recipe: Recipe,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transport: Option<String>,
}
fn digest(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}
impl Entry {
    pub fn new(
        daemon: String,
        session: &str,
        pane: &str,
        window: &str,
        pid: u32,
        profile: &Profile,
        recipe: Recipe,
    ) -> io::Result<Self> {
        let entry = Self {
            schema: 1,
            daemon,
            session: session.into(),
            pane: pane.into(),
            window: window.into(),
            pid,
            endpoint: profile.id.clone(),
            binding: binding(profile),
            recipe,
            transport: None,
        };
        entry.validate()?;
        Ok(entry)
    }
    fn validate(&self) -> io::Result<()> {
        if self.schema != 1
            || !digest(&self.daemon)
            || !digest(&self.binding)
            || !ruwac_protocol::valid_id(&self.session, "session")
            || !ruwac_protocol::valid_id(&self.pane, "pane")
            || !ruwac_protocol::valid_id(&self.window, "window")
            || !ruwac_protocol::valid_id(&self.endpoint, "endpoint")
            || self
                .transport
                .as_ref()
                .is_some_and(|t| !crate::ssh_transport::valid(t))
            || self.pid == 0
        {
            return Err(io::Error::other("Invalid launch identity"));
        }
        self.recipe.validate()
    }
    pub fn matches(&self, profile: &Profile) -> bool {
        profile.protocol == "SSH" && profile.id == self.endpoint && binding(profile) == self.binding
    }
}

#[derive(Clone)]
pub struct Catalog {
    folder: PathBuf,
    daemon: String,
}
fn directory(path: &Path) -> io::Result<()> {
    match fs::DirBuilder::new().mode(0o700).create(path) {
        Ok(()) => (),
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => (),
        Err(e) => return Err(e),
    }
    let m = fs::symlink_metadata(path)?;
    if !m.is_dir()
        || m.file_type().is_symlink()
        || m.uid() != unsafe { libc::geteuid() }
        || m.mode() & 0o777 != 0o700
    {
        return Err(io::Error::other("Unsafe launch catalog directory"));
    }
    Ok(())
}
impl Catalog {
    pub fn new(root: &Path, daemon: &str) -> io::Result<Self> {
        if !digest(daemon) {
            return Err(io::Error::other("Invalid daemon identity"));
        }
        directory(root)?;
        let base = root.join("launches");
        directory(&base)?;
        let folder = base.join(daemon);
        directory(&folder)?;
        Ok(Self {
            folder,
            daemon: daemon.into(),
        })
    }
    fn read(&self, pane: &str) -> io::Result<Entry> {
        if !ruwac_protocol::valid_id(pane, "pane") {
            return Err(io::Error::other("Invalid pane identity"));
        }
        let file = fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(self.folder.join(format!("{pane}.json")))?;
        let m = file.metadata()?;
        if !m.is_file()
            || m.uid() != unsafe { libc::geteuid() }
            || m.mode() & 0o777 != 0o600
            || m.nlink() != 1
            || m.len() > MAX_BYTES
        {
            return Err(io::Error::other("Unsafe launch record"));
        }
        let mut data = Vec::new();
        file.take(MAX_BYTES + 1).read_to_end(&mut data)?;
        if data.len() as u64 > MAX_BYTES {
            return Err(io::Error::other("Oversized launch record"));
        }
        let value: Entry = serde_json::from_slice(&data)?;
        value.validate()?;
        if value.daemon != self.daemon || value.pane != pane {
            return Err(io::Error::other("Launch daemon or pane differs"));
        }
        Ok(value)
    }
    pub fn list(&self) -> io::Result<Vec<Entry>> {
        let mut values = Vec::new();
        for (n, file) in fs::read_dir(&self.folder)?.enumerate() {
            if n >= LIMIT {
                return Err(io::Error::other("Launch catalog capacity reached"));
            }
            let name = file?.file_name();
            let Some(pane) = name.to_str().and_then(|n| n.strip_suffix(".json")) else {
                continue;
            };
            values.push(self.read(pane)?);
        }
        Ok(values)
    }
    pub fn write(&self, entry: &Entry) -> io::Result<()> {
        entry.validate()?;
        let lock = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(self.folder.join(".lock"))?;
        let meta = lock.metadata()?;
        if !meta.is_file()
            || meta.uid() != unsafe { libc::geteuid() }
            || meta.mode() & 0o777 != 0o600
            || meta.nlink() != 1
        {
            return Err(io::Error::other("Unsafe launch lock"));
        }
        if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX) } != 0 {
            return Err(io::Error::last_os_error());
        }
        if entry.daemon != self.daemon {
            return Err(io::Error::other("Launch belongs to another daemon"));
        }
        let final_path = self.folder.join(format!("{}.json", entry.pane));
        if final_path.exists() {
            return if self.read(&entry.pane)? == *entry {
                Ok(())
            } else {
                Err(io::Error::other("Launch identity already recorded"))
            };
        }
        if fs::read_dir(&self.folder)?.take(LIMIT).count() >= LIMIT {
            return Err(io::Error::other("Launch catalog capacity reached"));
        }
        let data = serde_json::to_vec(entry)?;
        if data.len() as u64 > MAX_BYTES {
            return Err(io::Error::other("Launch record exceeds cap"));
        }
        let temporary = self.folder.join(ruwac_protocol::id("pending"));
        let result = (|| {
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .custom_flags(libc::O_NOFOLLOW)
                .open(&temporary)?;
            file.write_all(&data)?;
            file.sync_all()?;
            let from = std::ffi::CString::new(temporary.as_os_str().as_encoded_bytes())
                .map_err(io::Error::other)?;
            let to = std::ffi::CString::new(final_path.as_os_str().as_encoded_bytes())
                .map_err(io::Error::other)?;
            if unsafe {
                libc::renameat2(
                    libc::AT_FDCWD,
                    from.as_ptr(),
                    libc::AT_FDCWD,
                    to.as_ptr(),
                    libc::RENAME_NOREPLACE,
                )
            } == 0
            {
                fs::File::open(&self.folder)?.sync_all()
            } else {
                Err(io::Error::last_os_error())
            }
        })();
        let _ = fs::remove_file(temporary);
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::{PermissionsExt, symlink};
    fn fixture() -> (PathBuf, Profile, Entry) {
        let root = std::env::temp_dir().join(ruwac_protocol::id("launch-test"));
        fs::DirBuilder::new().mode(0o700).create(&root).unwrap();
        let p:Profile=serde_json::from_value(serde_json::json!({"id":ruwac_protocol::id("endpoint"),"machine_id":ruwac_protocol::id("machine"),"name":"Generated","group":"","protocol":"SSH","server":"127.0.0.1:22222","username":"generated-user","secret_reference":"generated-reference","favorite":false,"last_used":0,"tags":[]})).unwrap();
        let e = Entry::new(
            "a".repeat(64),
            &ruwac_protocol::id("session"),
            &ruwac_protocol::id("pane"),
            &ruwac_protocol::id("window"),
            42,
            &p,
            Recipe::Ssh,
        )
        .unwrap();
        (root, p, e)
    }
    #[test]
    fn private_atomic_identity_and_no_credential_duplication() {
        let (root, _, e) = fixture();
        let c = Catalog::new(&root, &e.daemon).unwrap();
        c.write(&e).unwrap();
        c.write(&e).unwrap();
        assert_eq!(c.list().unwrap(), vec![e.clone()]);
        let bytes = fs::read_to_string(c.folder.join(format!("{}.json", e.pane))).unwrap();
        assert!(
            !bytes.contains("generated-user")
                && !bytes.contains("generated-reference")
                && !bytes.contains("127.0.0.1")
        );
        let mut wrong = e.clone();
        wrong.pid += 1;
        assert!(c.write(&wrong).is_err());
        assert!(
            Catalog::new(&root, &"b".repeat(64))
                .unwrap()
                .list()
                .unwrap()
                .is_empty()
        );
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn transport_identity_roundtrips_and_old_entries_stay_unbound() {
        let (root, _, mut e) = fixture();
        let old = serde_json::to_vec(&e).unwrap();
        assert_eq!(
            serde_json::from_slice::<Entry>(&old).unwrap().transport,
            None
        );
        e.transport = Some(ruwac_protocol::id("transport"));
        let c = Catalog::new(&root, &e.daemon).unwrap();
        c.write(&e).unwrap();
        assert_eq!(c.list().unwrap(), vec![e.clone()]);
        let mut invalid = e;
        invalid.transport = Some("../../other-socket".into());
        assert!(invalid.validate().is_err());
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn profile_binding_allows_cosmetic_edits_and_refuses_route_identity_changes() {
        let (root, mut p, e) = fixture();
        p.name = "Other".into();
        p.favorite = true;
        p.last_used = 123;
        p.preferences.codex_command = "generated-command".into();
        assert!(e.matches(&p));
        p.username = "another-account".into();
        assert!(!e.matches(&p));
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn refuses_symlink_fifo_hardlink_and_nonprivate_records() {
        let (root, _, e) = fixture();
        let c = Catalog::new(&root, &e.daemon).unwrap();
        c.write(&e).unwrap();
        let path = c.folder.join(format!("{}.json", e.pane));
        let backup = root.join("record");
        fs::rename(&path, &backup).unwrap();
        symlink(&backup, &path).unwrap();
        assert!(c.list().is_err());
        fs::remove_file(&path).unwrap();
        fs::hard_link(&backup, &path).unwrap();
        assert!(c.list().is_err());
        fs::remove_file(&path).unwrap();
        let name = std::ffi::CString::new(path.as_os_str().as_encoded_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
        assert!(c.list().is_err());
        fs::remove_file(&path).unwrap();
        fs::rename(backup, &path).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(c.list().is_err());
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn concurrent_distinct_records_do_not_lose_updates() {
        let (root, _, e) = fixture();
        let c = Catalog::new(&root, &e.daemon).unwrap();
        let tasks = (0..8)
            .map(|_| {
                let c = c.clone();
                let mut e = e.clone();
                e.pane = ruwac_protocol::id("pane");
                std::thread::spawn(move || c.write(&e).unwrap())
            })
            .collect::<Vec<_>>();
        for t in tasks {
            t.join().unwrap();
        }
        assert_eq!(c.list().unwrap().len(), 8);
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn reconstructed_agent_recipes_preserve_provider_backend_and_custom_command() {
        let (root, mut p, _) = fixture();
        for kind in [0, 1, 5] {
            for backend in ["ruwac", "screen", "tmux", "direct"] {
                p.preferences.agent_backend = backend.into();
                p.preferences.codex_command = "printf '%s' 'generated quoted command'".into();
                let recipe =
                    Recipe::agent(&p, kind, "generated ruwac", "/generated/private.sock").unwrap();
                let saved: Recipe =
                    serde_json::from_slice(&serde_json::to_vec(&recipe).unwrap()).unwrap();
                assert_eq!(saved, recipe);
                assert_eq!(
                    saved.provider(),
                    Some(match kind {
                        0 => "codex",
                        1 => "claude",
                        _ => "kimi",
                    })
                );
                let actual = saved.argv(&p).unwrap();
                let expected = crate::profiles::session_argv(
                    &p,
                    crate::agent_launch::argv(
                        &p,
                        kind,
                        "generated ruwac",
                        "/generated/private.sock",
                    )
                    .unwrap(),
                );
                // Recreating a recipe preserves its command, but each launch
                // needs an independent transport instead of attaching to the
                // previous SSH process through a reused control socket.
                let route = crate::ssh_transport::identity(&actual).unwrap();
                let other = crate::ssh_transport::identity(&expected).unwrap();
                assert_ne!(route, other);
                let without_identity = |argv: Vec<String>| {
                    argv.into_iter()
                        .filter(|a| !a.starts_with("ControlPath="))
                        .collect::<Vec<_>>()
                };
                assert_eq!(without_identity(actual), without_identity(expected));
            }
        }
        assert!(Recipe::agent(&p, 2, "ruwac", "").is_err());
        assert!(Recipe::agent(&p, 4, "ruwac", "").is_err());
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn refuses_retargeted_record_and_unbounded_recipe() {
        let (root, mut p, e) = fixture();
        let c = Catalog::new(&root, &e.daemon).unwrap();
        c.write(&e).unwrap();
        let path = c.folder.join(format!("{}.json", e.pane));
        let mut changed = e.clone();
        changed.daemon = "b".repeat(64);
        fs::write(&path, serde_json::to_vec(&changed).unwrap()).unwrap();
        assert!(c.list().is_err());
        p.preferences.codex_command = "x".repeat(1025);
        assert!(Recipe::agent(&p, 0, "ruwac", "").is_err());
        p.preferences.codex_command = "generated\ncommand".into();
        assert!(Recipe::agent(&p, 0, "ruwac", "").is_err());
        fs::remove_dir_all(root).unwrap();
    }
}
