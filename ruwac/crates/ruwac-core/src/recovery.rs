//! Private canonical checkpoints. Restored panes own no process or input channel.
use super::*;
use ruwac_protocol::valid_id;
use ruwac_term::Archive;
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    os::unix::{
        fs::{MetadataExt, OpenOptionsExt},
        io::AsRawFd,
    },
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
const MAX_FILE: u64 = 128 * 1024 * 1024;
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Checkpoint {
    schema: u8,
    revision: u64,
    sessions: Vec<Session>,
    windows: Vec<Window>,
    panes: Vec<SavedPane>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SavedPane {
    id: String,
    window: String,
    #[serde(with = "shared_terminal")]
    terminal: Arc<Archive>,
    exit_code: Option<u32>,
}

// Sharing is only an in-memory checkpoint implementation detail. Keep the
// existing canonical JSON/schema and its ordinary Archive deserializer.
mod shared_terminal {
    use super::*;
    pub fn serialize<S: serde::Serializer>(
        terminal: &Arc<Archive>,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        terminal.as_ref().serialize(serializer)
    }
    pub fn deserialize<'de, D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Arc<Archive>, D::Error> {
        Archive::deserialize(deserializer).map(Arc::new)
    }
}

impl Pane {
    fn checkpoint_archive(&mut self) -> Arc<Archive> {
        // Feed (including mode resets) and resize advance this canonical epoch.
        // Selection replies change neither cells nor archived terminal modes.
        // At most one current snapshot is retained per pane; an in-flight writer
        // holds immutable references. Removing the pane releases its cache.
        let epoch = self.term.history_epoch;
        if self
            .checkpoint
            .as_ref()
            .is_none_or(|(saved, _)| *saved != epoch)
        {
            self.checkpoint = Some((epoch, Arc::new(self.term.checkpoint_archive())));
        }
        Arc::clone(&self.checkpoint.as_ref().unwrap().1)
    }
}

pub struct Store {
    path: PathBuf,
    _lock: File,
    written: Mutex<Option<u64>>,
}
fn private_file(file: &File) -> io::Result<()> {
    let m = file.metadata()?;
    if !m.is_file()
        || m.uid() != unsafe { libc::geteuid() }
        || m.mode() & 0o777 != 0o600
        || m.nlink() != 1
    {
        return Err(io::Error::other(
            "checkpoint file must be owned, regular, singly linked and 0600",
        ));
    }
    Ok(())
}
impl Store {
    /// Parent must already be a private directory. A separate lock prevents two
    /// different sockets from sharing an explicit checkpoint simultaneously.
    pub fn open(path: &Path) -> io::Result<Self> {
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .ok_or_else(|| io::Error::other("checkpoint needs private parent"))?;
        let m = fs::symlink_metadata(parent)?;
        if !m.is_dir() || m.uid() != unsafe { libc::geteuid() } || m.mode() & 0o777 != 0o700 {
            return Err(io::Error::other(
                "checkpoint parent must be owned, nonsymlink and 0700",
            ));
        }
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
            .open(path.with_extension("checkpoint.lock"))?;
        private_file(&lock)?;
        if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
            return Err(io::Error::other(
                "checkpoint already owned by another daemon",
            ));
        }
        let store = Self {
            path: path.into(),
            _lock: lock,
            written: Mutex::new(None),
        };
        // Even an empty or corrupt existing file is preserved and rejected.
        if let Some(file) = store.read_file()? {
            private_file(&file)?;
        }
        Ok(store)
    }
    fn read_file(&self) -> io::Result<Option<File>> {
        match OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC)
            .open(&self.path)
        {
            Ok(f) => {
                private_file(&f)?;
                Ok(Some(f))
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e),
        }
    }
    fn load(&self) -> io::Result<Option<Checkpoint>> {
        let Some(file) = self.read_file()? else {
            return Ok(None);
        };
        if file.metadata()?.len() > MAX_FILE {
            return Err(io::Error::other("checkpoint exceeds 128 MiB"));
        }
        let mut bytes = Vec::new();
        file.take(MAX_FILE + 1).read_to_end(&mut bytes)?;
        if bytes.len() as u64 > MAX_FILE {
            return Err(io::Error::other("checkpoint exceeds 128 MiB"));
        }
        let value: Checkpoint = serde_json::from_slice(&bytes)
            .map_err(|_| io::Error::other("checkpoint JSON/schema rejected; original preserved"))?;
        Ok(Some(value))
    }
    fn save(&self, checkpoint: Checkpoint) -> std::result::Result<(), String> {
        let mut written = self
            .written
            .lock()
            .map_err(|_| "checkpoint writer failed")?;
        if written.is_some_and(|r| r >= checkpoint.revision) {
            return Ok(());
        }
        for pane in &checkpoint.panes {
            pane.terminal
                .validate()
                .map_err(|e| format!("checkpoint validation failed: {e}"))?;
        }
        let parent = self.path.parent().unwrap();
        let temp = parent.join(format!(".{}.tmp", id("checkpoint")));
        let result = (|| -> io::Result<()> {
            // Refuse a replaced unsafe destination instead of following it.
            self.read_file()?;
            let file = OpenOptions::new()
                .create_new(true)
                .write(true)
                .mode(0o600)
                .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
                .open(&temp)?;
            let mut limited = std::io::BufWriter::with_capacity(
                64 * 1024,
                LimitedWriter {
                    file,
                    remaining: MAX_FILE,
                },
            );
            serde_json::to_writer(&mut limited, &checkpoint)?;
            limited.flush()?;
            limited.get_ref().file.sync_all()?;
            self.read_file()?;
            fs::rename(&temp, &self.path)?;
            File::open(parent)?.sync_all()?;
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temp);
        }
        result.map_err(|e| format!("checkpoint write failed: {e}"))?;
        *written = Some(checkpoint.revision);
        Ok(())
    }
}
struct LimitedWriter {
    file: File,
    remaining: u64,
}
impl Write for LimitedWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() as u64 > self.remaining {
            return Err(io::Error::other("checkpoint exceeds 128 MiB"));
        }
        let n = self.file.write(bytes)?;
        self.remaining -= n as u64;
        Ok(n)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.file.flush()
    }
}
impl State {
    fn saved(&mut self) -> Checkpoint {
        // Clipboard registers, requests, input, argv, client identities and
        // authentication are intentionally absent from this schema.
        let mut checkpoint = Checkpoint {
            schema: 5,
            revision: self.revision,
            sessions: self
                .sessions
                .values()
                .map(|s| Session {
                    id: s.id.clone(),
                    name: s.name.clone(),
                    windows: s.windows.clone(),
                    active: s.active.clone(),
                    rows: s.rows,
                    cols: s.cols,
                    buffer: String::new(),
                    previous: s.previous.clone(),
                })
                .collect(),
            windows: self
                .windows
                .values()
                .map(|w| Window {
                    id: w.id.clone(),
                    session: w.session.clone(),
                    name: w.name.clone(),
                    panes: w.panes.clone(),
                    active: w.active.clone(),
                    layout: w.layout.clone(),
                    zoom: w.zoom,
                })
                .collect(),
            panes: self
                .panes
                .values_mut()
                .map(|p| SavedPane {
                    id: p.id.clone(),
                    window: p.window.clone(),
                    terminal: p.checkpoint_archive(),
                    exit_code: p
                        .exit_code
                        .or_else(|| p.pty.as_ref().and_then(Pty::exit_code)),
                })
                .collect(),
        };
        let legacy = ruwac_protocol::TerminalLimits::LEGACY;
        if checkpoint
            .sessions
            .iter()
            .any(|s| !legacy.valid(s.rows, s.cols))
            || checkpoint.panes.iter().any(|p| {
                !legacy.valid(p.terminal.rows, p.terminal.cols)
                    || p.terminal
                        .primary
                        .as_ref()
                        .is_some_and(|g| !legacy.valid(g.rows, g.cols))
            })
        {
            checkpoint.schema = 6;
        }
        checkpoint
    }
    pub(super) fn restore(&mut self) -> io::Result<()> {
        let Some(store) = &self.store else {
            return Ok(());
        };
        let Some(saved) = store.load()? else {
            return Ok(());
        };
        let invalid = || io::Error::other("checkpoint resource graph rejected; original preserved");
        if !matches!(saved.schema, 1 | 2 | 3 | 4 | 5 | 6)
            || saved.sessions.len() > 64
            || saved.windows.len() > 128
            || saved.panes.len() > 128
        {
            return Err(invalid());
        }
        for s in saved.sessions {
            if !valid_id(&s.id, "session")
                || checked_text(&s.name).is_err()
                || s.rows < 4
                || s.cols < 10
                || !ruwac_protocol::TerminalLimits::CURRENT.valid(s.rows, s.cols)
                || s.windows.is_empty()
                || s.windows.len() > 128
                || self.sessions.insert(s.id.clone(), s).is_some()
            {
                return Err(invalid());
            }
        }
        for w in saved.windows {
            if !valid_id(&w.id, "window")
                || checked_text(&w.name).is_err()
                || w.panes.is_empty()
                || w.panes.len() > 16
                || self.windows.insert(w.id.clone(), w).is_some()
            {
                return Err(invalid());
            }
        }
        for p in saved.panes {
            if !valid_id(&p.id, "pane") || self.panes.contains_key(&p.id) {
                return Err(invalid());
            }
            let archive = Arc::try_unwrap(p.terminal).unwrap_or_else(|a| a.as_ref().clone());
            let term = Terminal::restore(archive).map_err(|_| invalid())?;
            self.panes.insert(
                p.id.clone(),
                Pane {
                    id: p.id,
                    window: p.window,
                    pty: None,
                    exit_code: p.exit_code,
                    term,
                    checkpoint: None,
                    ended: true,
                    foreground: None,
                    job: None,
                },
            );
        }
        let mut windows = std::collections::HashSet::new();
        let mut panes = std::collections::HashSet::new();
        for s in self.sessions.values() {
            if !s.windows.contains(&s.active)
                || s.previous
                    .as_ref()
                    .is_some_and(|p| !s.windows.contains(p) || *p == s.active)
            {
                return Err(invalid());
            }
            for wid in &s.windows {
                let w = self.windows.get(wid).ok_or_else(invalid)?;
                if w.session != s.id || !windows.insert(wid) || !w.panes.contains(&w.active) {
                    return Err(invalid());
                }
                let mut leaves = Vec::new();
                validate_layout(&w.layout, 0, &mut leaves).map_err(|_| invalid())?;
                let set: std::collections::HashSet<_> = leaves.iter().collect();
                if set.len() != leaves.len()
                    || leaves.len() != w.panes.len()
                    || !w.panes.iter().all(|p| set.contains(p))
                {
                    return Err(invalid());
                }
                for pid in &w.panes {
                    let p = self.panes.get(pid).ok_or_else(invalid)?;
                    if p.window != w.id || !panes.insert(pid) {
                        return Err(invalid());
                    }
                }
            }
        }
        if windows.len() != self.windows.len() || panes.len() != self.panes.len() {
            return Err(invalid());
        }
        self.revision = saved.revision.checked_add(1).ok_or_else(invalid)?;
        self.checkpoint_revision = saved.revision;
        Ok(())
    }
    pub(super) fn schedule_checkpoint(&mut self) {
        if self.checkpoint_pending || self.revision == self.checkpoint_revision {
            return;
        }
        let Some(store) = self.store.clone() else {
            return;
        };
        let saved = self.saved();
        let revision = saved.revision;
        let tx = self.tx.clone();
        self.checkpoint_pending = true;
        tokio::task::spawn_blocking(move || {
            let result = store.save(saved);
            let _ = tx.blocking_send(Message::Saved(revision, result));
        });
    }
    pub(super) async fn checkpoint(&mut self) -> std::result::Result<(), String> {
        let Some(store) = self.store.clone() else {
            return Ok(());
        };
        let saved = self.saved();
        let revision = saved.revision;
        let result = tokio::task::spawn_blocking(move || store.save(saved))
            .await
            .map_err(|_| "checkpoint worker stopped".to_string())?;
        match result {
            Ok(()) => {
                self.checkpoint_revision = self.checkpoint_revision.max(revision);
                self.checkpoint_error = None;
                Ok(())
            }
            Err(e) => {
                self.checkpoint_error = Some(e.clone());
                Err(e)
            }
        }
    }
}
fn validate_layout(l: &Layout, depth: usize, out: &mut Vec<String>) -> std::result::Result<(), ()> {
    if depth > 15 || out.len() >= 16 {
        return Err(());
    }
    match l {
        Layout::Leaf(p) => {
            out.push(p.clone());
        }
        Layout::Split {
            id,
            ratio,
            first,
            second,
            ..
        } => {
            if !id
                .strip_prefix("split_")
                .is_some_and(|p| valid_id(p, "pane"))
                || !(100..=900).contains(ratio)
            {
                return Err(());
            }
            validate_layout(first, depth + 1, out)?;
            validate_layout(second, depth + 1, out)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod checkpoint_snapshot_tests {
    use super::*;

    fn pane() -> Pane {
        Pane {
            id: id("pane"),
            window: id("window"),
            pty: None,
            exit_code: None,
            term: Terminal::new(4, 24),
            checkpoint: None,
            ended: false,
            foreground: None,
            job: None,
        }
    }

    #[test]
    fn unchanged_snapshots_share_without_changing_an_inflight_writer() {
        let mut p = pane();
        p.term.feed(b"before");
        let old = p.checkpoint_archive();
        let old_json = serde_json::to_value(old.as_ref()).unwrap();
        assert!(Arc::ptr_eq(&old, &p.checkpoint_archive()));
        p.term.feed(b"\r\nafter");
        let new = p.checkpoint_archive();
        assert!(!Arc::ptr_eq(&old, &new));
        assert_eq!(serde_json::to_value(old.as_ref()).unwrap(), old_json);
        assert_ne!(serde_json::to_value(new.as_ref()).unwrap(), old_json);
        let weak = Arc::downgrade(&new);
        drop(new);
        drop(p);
        assert!(weak.upgrade().is_none(), "no cache survives its pane");
    }

    #[test]
    fn snapshots_follow_output_modes_resize_and_independent_panes() {
        let mut p = pane();
        let mut other = pane();
        let quiet = other.checkpoint_archive();
        for bytes in [
            "Že\u{301}界\r\n".as_bytes(),
            b"\x1b[?1049h\x1b[1;38;2;1;2;3mALT",
            b"\x1b[?2004h\x1b[?1006h",
            b"\x1b[?1049lPRIMARY",
        ] {
            let prior = p.checkpoint_archive();
            p.term.feed(bytes);
            let snapshot = p.checkpoint_archive();
            assert!(!Arc::ptr_eq(&prior, &snapshot));
            assert_eq!(
                serde_json::to_value(snapshot.as_ref()).unwrap(),
                serde_json::to_value(p.term.checkpoint_archive()).unwrap()
            );
            assert!(Arc::ptr_eq(&quiet, &other.checkpoint_archive()));
        }
        for mutation in 0..2 {
            let prior = p.checkpoint_archive();
            if mutation == 0 {
                p.term.resize(6, 18);
            } else {
                p.term.reset_input_modes();
            }
            let snapshot = p.checkpoint_archive();
            assert!(!Arc::ptr_eq(&prior, &snapshot));
            assert_eq!(
                serde_json::to_value(snapshot.as_ref()).unwrap(),
                serde_json::to_value(p.term.checkpoint_archive()).unwrap()
            );
        }
    }

    #[test]
    fn shared_snapshots_keep_the_existing_plain_archive_wire_shape() {
        let mut p = pane();
        p.term.feed(b"\x1b[31mprimary\r\n\x1b[?1049halternate");
        let expected = serde_json::to_value(p.term.checkpoint_archive()).unwrap();
        let saved = SavedPane {
            id: p.id.clone(),
            window: p.window.clone(),
            terminal: p.checkpoint_archive(),
            exit_code: Some(0),
        };
        let encoded = serde_json::to_value(&saved).unwrap();
        assert_eq!(encoded["terminal"], expected);
        assert_eq!(encoded.as_object().unwrap().len(), 4);
        let decoded: SavedPane = serde_json::from_value(encoded).unwrap();
        decoded.terminal.validate().unwrap();
        let restored = Terminal::restore(decoded.terminal.as_ref().clone()).unwrap();
        assert_eq!(restored.capture(20), p.term.capture(20));
        assert_eq!(restored.capture_primary(20), p.term.capture_primary(20));
    }
}
