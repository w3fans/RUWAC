//! Explicit SSH rsync transport with a bounded literal file list and verified staging.
use crate::profiles::Profile;
use sha2::{Digest, Sha256};
use ssh2::{FileStat, Sftp};
use std::{
    collections::BTreeMap,
    fs,
    io::{Read, Write},
    os::fd::{AsRawFd, FromRawFd},
    os::unix::{
        ffi::OsStrExt,
        fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt},
        process::CommandExt,
    },
    path::{Component, Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};
const MAX_BYTES: u64 = 32 * 1024 * 1024 * 1024;
const MAX_ENTRIES: usize = 10_000;
#[derive(Clone)]
struct Entry {
    directory: bool,
    mode: u32,
    bytes: u64,
    hash: String,
}
type Manifest = BTreeMap<PathBuf, Entry>;
#[derive(serde::Serialize)]
pub struct Receipt {
    pub bytes: u64,
    pub files: usize,
    pub directories: usize,
    pub sha256: String,
}
#[derive(Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct RootStamp {
    mode: u32,
    uid: u32,
    gid: u32,
    mtime: u64,
    identity: Option<(u64, u64)>,
}
#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Recovery {
    schema: u32,
    pub id: String,
    pub endpoint: String,
    binding: String,
    host_sha256: String,
    pub upload: bool,
    pub destination: PathBuf,
    stage: PathBuf,
    backup: PathBuf,
    pub phase: String,
    expected: String,
    original: Option<String>,
    original_stamp: Option<RootStamp>,
}
impl Recovery {
    pub fn eligible(&self, p: &Profile) -> bool {
        self.endpoint == p.id && self.binding == recovery_binding(p)
    }
}
fn recovery_binding(p: &Profile) -> String {
    format!(
        "{:x}",
        Sha256::digest(
            serde_json::to_vec(&serde_json::json!([
                crate::files_resume::binding(p),
                p.protocol,
                p.secret_reference,
                p.preferences.gateway
            ]))
            .unwrap()
        )
    )
}
fn mutation_guard(cancel: &AtomicBool) -> Result<fs::File, String> {
    let path = recovery_root()?.join(".mutation.lock");
    let f = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
        .map_err(error)?;
    if !crate::files_resume::private_file(&f) {
        return Err("Recovery mutation lock ownership invalid".into());
    }
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        check(cancel)?;
        if unsafe { libc::flock(f.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
            return Ok(f);
        }
        if Instant::now() > deadline {
            return Err("Another rsync publish/recovery is active; retry later".into());
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}
fn backup_unchanged(
    job: &Recovery,
    sftp: Option<&Sftp>,
    cancel: &AtomicBool,
) -> Result<(), String> {
    if job.original_stamp != root_stamp(&job.backup, sftp)?
        || job.original.as_deref()
            != Some(manifest_digest(&scan(&job.backup, sftp, cancel)?).as_str())
    {
        return Err("Original backup changed; retained for inspection".into());
    }
    Ok(())
}
fn recovery_root() -> Result<PathBuf, String> {
    let root = crate::files_resume::root()?
        .parent()
        .unwrap()
        .join("rsync-recovery");
    match fs::DirBuilder::new().mode(0o700).create(&root) {
        Ok(()) => (),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => (),
        Err(e) => return Err(error(e)),
    }
    if !crate::files_resume::private_dir(&root) {
        return Err("Rsync recovery needs an owned nonsymlink 0700 directory".into());
    }
    Ok(root)
}
struct RecoveryJournal {
    root: PathBuf,
    id: String,
    lock: fs::File,
}
impl RecoveryJournal {
    fn open(id: &str) -> Result<Self, String> {
        if !ruwac_protocol::valid_id(id, "rsync") {
            return Err("Invalid recovery ID".into());
        }
        let root = recovery_root()?;
        let lock = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(root.join(format!("{id}.lock")))
            .map_err(error)?;
        if !crate::files_resume::private_file(&lock)
            || unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0
        {
            return Err("Rsync transfer/recovery is active or lock ownership invalid".into());
        }
        Ok(Self {
            root,
            id: id.into(),
            lock,
        })
    }
    fn path(&self) -> PathBuf {
        self.root.join(format!("{}.json", self.id))
    }
    fn save(&self, job: &Recovery) -> Result<(), String> {
        validate_recovery(job)?;
        if !crate::files_resume::private_dir(&self.root) {
            return Err("Recovery directory ownership changed".into());
        }
        let temp = self.root.join(ruwac_protocol::id("rsync-write"));
        let data = serde_json::to_vec(job).map_err(error)?;
        if data.len() > 32768 {
            return Err("Recovery metadata too large".into());
        }
        if self.path().exists() {
            let _ = read_recovery(&self.path())?;
        }
        let result = (|| {
            let mut f = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .custom_flags(libc::O_NOFOLLOW)
                .open(&temp)
                .map_err(error)?;
            f.write_all(&data).map_err(error)?;
            f.sync_all().map_err(error)?;
            fs::rename(&temp, self.path()).map_err(error)?;
            fs::File::open(&self.root)
                .and_then(|f| f.sync_all())
                .map_err(error)
        })();
        if result.is_err() {
            let _ = fs::remove_file(temp);
        }
        result
    }
    fn remove(&self) -> Result<(), String> {
        let _ = read_recovery(&self.path())?;
        let list = self.root.join(format!("{}.files", self.id));
        match fs::symlink_metadata(&list) {
            Ok(_) => {
                let f = fs::OpenOptions::new()
                    .read(true)
                    .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
                    .open(&list)
                    .map_err(error)?;
                if !crate::files_resume::private_file(&f) {
                    return Err("Recovery file-list ownership changed".into());
                }
                fs::remove_file(list).map_err(error)?;
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
            Err(e) => return Err(error(e)),
        }

        fs::remove_file(self.path()).map_err(error)?;
        fs::File::open(&self.root)
            .and_then(|f| f.sync_all())
            .map_err(error)
    }
}
impl Drop for RecoveryJournal {
    fn drop(&mut self) {
        if self.path().exists() {
            return;
        }
        let path = self.root.join(format!("{}.lock", self.id));
        if let (Ok(a), Ok(b)) = (self.lock.metadata(), fs::symlink_metadata(&path)) {
            if a.ino() == b.ino() && a.dev() == b.dev() {
                let _ = fs::remove_file(path);
            }
        }
    }
}
fn validate_recovery(job: &Recovery) -> Result<(), String> {
    let digest = |s: &str| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit());
    if job.schema != 1
        || !ruwac_protocol::valid_id(&job.id, "rsync")
        || job.endpoint.is_empty()
        || job.endpoint.len() > 1024
        || !digest(&job.binding)
        || !digest(&job.host_sha256)
        || !digest(&job.expected)
        || job.original.as_ref().is_some_and(|s| !digest(s))
        || job.original.is_some() != job.original_stamp.is_some()
        || !matches!(
            job.phase.as_str(),
            "copy" | "backup-intent" | "backup-moved" | "published" | "cleaned"
        )
    {
        return Err("Invalid recovery metadata; paths preserved".into());
    }
    let parent = job
        .destination
        .parent()
        .ok_or("Invalid recovery destination")?;
    for path in [&job.destination, &job.stage, &job.backup] {
        valid(path)?;
        if !path.is_absolute() {
            return Err("Recovery paths must be absolute".into());
        }
    }
    if job.stage.parent() != Some(parent)
        || job.backup.parent() != Some(parent)
        || !job
            .stage
            .file_name()
            .and_then(|s| s.to_str())
            .is_some_and(|s| ruwac_protocol::valid_id(s, "ruwac-rsync-part"))
        || !job
            .backup
            .file_name()
            .and_then(|s| s.to_str())
            .is_some_and(|s| ruwac_protocol::valid_id(s, "ruwac-rsync-backup"))
    {
        return Err("Recovery path relationship invalid; paths preserved".into());
    }
    Ok(())
}
fn read_recovery(path: &Path) -> Result<Recovery, String> {
    let f = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
        .map_err(error)?;
    if !crate::files_resume::private_file(&f) || f.metadata().map_err(error)?.len() > 32768 {
        return Err("Recovery journal ownership/size invalid".into());
    }
    let job: Recovery = serde_json::from_reader(f.take(32769))
        .map_err(|_| "Malformed recovery journal; paths preserved")?;
    validate_recovery(&job)?;
    if path.file_name().and_then(|s| s.to_str()) != Some(&format!("{}.json", job.id)) {
        return Err("Recovery journal identity mismatch".into());
    }
    Ok(job)
}
pub fn pending(p: &Profile) -> Result<Vec<Recovery>, String> {
    let mut result = Vec::new();
    let mut count = 0;
    for entry in fs::read_dir(recovery_root()?).map_err(error)? {
        let path = entry.map_err(error)?.path();
        if path.extension().is_none_or(|s| s != "json") {
            continue;
        }
        count += 1;
        if count > 128 {
            return Err("Recovery journal exceeds 128 entries".into());
        }
        let job = read_recovery(&path)?;
        if job.endpoint == p.id {
            result.push(job);
        }
    }
    result.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(result)
}
fn manifest_digest(manifest: &Manifest) -> String {
    let mut hash = Sha256::new();
    for (path, e) in manifest {
        hash.update((path.as_os_str().len() as u64).to_le_bytes());
        hash.update(path.as_os_str().as_bytes());
        hash.update([u8::from(e.directory)]);
        hash.update(e.mode.to_le_bytes());
        hash.update(e.bytes.to_le_bytes());
        hash.update(e.hash.as_bytes());
    }
    format!("{:x}", hash.finalize())
}
fn root_stamp(path: &Path, sftp: Option<&Sftp>) -> Result<Option<RootStamp>, String> {
    if let Some(s) = sftp {
        match s.lstat(path) {
            Ok(m) => {
                if !matches!(m.perm.map(|m| m & 0o170000), Some(0o100000 | 0o040000)) {
                    return Err("Recovery root is not a regular file/directory".into());
                }
                Ok(Some(RootStamp {
                    mode: m.perm.ok_or("Root mode absent")?,
                    uid: m.uid.ok_or("Root owner absent")?,
                    gid: m.gid.ok_or("Root group absent")?,
                    mtime: m.mtime.ok_or("Root time absent")?,
                    identity: None,
                }))
            }
            Err(e) if e.code() == ssh2::ErrorCode::SFTP(2) => Ok(None),
            Err(e) => Err(sftp_error("recovery metadata", e)),
        }
    } else {
        match fs::symlink_metadata(path) {
            Ok(m) => {
                if !m.is_file() && !m.is_dir() {
                    return Err("Recovery root is a symlink/special file".into());
                }
                Ok(Some(RootStamp {
                    mode: m.mode(),
                    uid: m.uid(),
                    gid: m.gid(),
                    mtime: (m.mtime().max(0) as u64)
                        .saturating_mul(1_000_000_000)
                        .saturating_add(m.mtime_nsec().max(0) as u64),
                    identity: Some((m.dev(), m.ino())),
                }))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(error(e)),
        }
    }
}
fn marker(job: &Recovery, sftp: Option<&Sftp>, create: bool) -> Result<(), String> {
    let path = job.stage.join(".ruwac-rsync-owner");
    let data = job.id.as_bytes();
    if create {
        if let Some(s) = sftp {
            let mut f = s
                .open_mode(
                    &path,
                    ssh2::OpenFlags::WRITE | ssh2::OpenFlags::CREATE | ssh2::OpenFlags::EXCLUSIVE,
                    0o600,
                    ssh2::OpenType::File,
                )
                .map_err(error)?;
            f.write_all(data).map_err(error)?;
            f.fsync()
                .map_err(|_| "Remote server cannot sync the recovery marker")?;
            f.close().map_err(error)?;
        } else {
            let mut f = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .custom_flags(libc::O_NOFOLLOW)
                .open(&path)
                .map_err(error)?;
            f.write_all(data).map_err(error)?;
            f.sync_all().map_err(error)?;
            fs::File::open(&job.stage)
                .and_then(|f| f.sync_all())
                .map_err(error)?;
        }
    } else {
        let mut bytes = Vec::new();
        if let Some(s) = sftp {
            let stat = s.lstat(&path).map_err(error)?;
            if stat.perm.is_none_or(|m| m & 0o170000 != 0o100000)
                || stat.size != Some(data.len() as u64)
            {
                return Err("Recovery marker type/size changed; staging preserved".into());
            }
            s.open(&path)
                .map_err(error)?
                .take(128)
                .read_to_end(&mut bytes)
                .map_err(error)?;
        } else {
            let f = fs::OpenOptions::new()
                .read(true)
                .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
                .open(&path)
                .map_err(error)?;
            if !crate::files_resume::private_file(&f) {
                return Err("Recovery marker ownership changed".into());
            }
            f.take(128).read_to_end(&mut bytes).map_err(error)?;
        }
        if bytes != data {
            return Err("Recovery marker changed; staging preserved".into());
        }
    }
    Ok(())
}
/// Explicit recovery never overwrites an unexpected destination or retargets a host.
pub fn recover(p: &Profile, id: &str, cancel: &AtomicBool) -> Result<String, String> {
    check(cancel)?;
    let journal = RecoveryJournal::open(id)?;
    let job = read_recovery(&journal.path())?;
    if !job.eligible(p) {
        return Err("Recovery profile changed; restore original endpoint settings".into());
    }
    if p.protocol != "SSH"
        || (!p.preferences.gateway.is_empty() && p.preferences.ssh_jump_alias.is_empty())
    {
        return Err("Restore original SSH recovery profile".into());
    }
    if !job.upload {
        let parent = job.destination.parent().unwrap();
        if fs::canonicalize(parent).map_err(error)? != parent {
            return Err("Local recovery parent namespace changed; paths preserved".into());
        }
    }
    let session = if job.upload {
        Some(crate::files::connect_original(p, &job.host_sha256)?)
    } else {
        None
    };
    if let Some(s) = session.as_ref() {
        if crate::files_resume::host_key(s)? != job.host_sha256 {
            return Err("Original recovery host key changed".into());
        }
    }
    let remote = if let Some(s) = session.as_ref() {
        Some(s.sftp().map_err(error)?)
    } else {
        None
    };
    let sftp = remote.as_ref();
    if let Some(s) = sftp {
        let _ = crate::files::remote_path(s, &job.destination, true)?;
        let _ = crate::files::remote_path(s, &job.stage, true)?;
        let _ = crate::files::remote_path(s, &job.backup, true)?;
    }
    recover_locked(&job, &journal, sftp, cancel)
}
fn recover_locked(
    job: &Recovery,
    journal: &RecoveryJournal,
    sftp: Option<&Sftp>,
    cancel: &AtomicBool,
) -> Result<String, String> {
    let _mutation = mutation_guard(cancel)?;
    if let Some(stamp) = root_stamp(&job.stage, sftp)? {
        if stamp.mode & 0o170777 != 0o040700 {
            return Err("Recovery staging type/permissions changed; paths preserved".into());
        }
        marker(job, sftp, false)?;
    }
    let mut outcome = "Incomplete copy discarded; destination was not created";
    let backup = root_stamp(&job.backup, sftp)?;
    let destination = root_stamp(&job.destination, sftp)?;
    if let Some(stamp) = backup.as_ref() {
        if job.original_stamp.as_ref() != Some(stamp)
            || job.original.as_deref()
                != Some(manifest_digest(&scan(&job.backup, sftp, cancel)?).as_str())
        {
            return Err("Original backup changed; recovery refused, paths preserved".into());
        }
        check(cancel)?;
        if destination.is_none() {
            outcome = "Original destination restored";
            if let Some(s) = sftp {
                s.rename(
                    &job.backup,
                    &job.destination,
                    Some(ssh2::RenameFlags::empty()),
                )
                .map_err(error)?;
            } else {
                rename_local(&job.backup, &job.destination)?;
            }
        } else {
            outcome = "Verified replacement retained";
            if job.expected != manifest_digest(&scan(&job.destination, sftp, cancel)?) {
                return Err("Destination changed; original backup preserved for inspection".into());
            }
            if let Some(s) = sftp {
                clean_remote(s, &job.backup, 0)?;
            } else {
                clean_local(&job.backup)?;
            }
        }
    } else if let Some(stamp) = destination.as_ref() {
        let current = manifest_digest(&scan(&job.destination, sftp, cancel)?);
        outcome = if current == job.expected {
            "Verified destination retained"
        } else {
            "Original destination retained"
        };
        if current != job.expected
            && (job.original.as_deref() != Some(current.as_str())
                || job.original_stamp.as_ref() != Some(stamp))
        {
            return Err("Destination changed; recovery refused, paths preserved".into());
        }
    } else if job.original.is_some() {
        return Err(
            "Original destination and backup are missing; recovery needs inspection".into(),
        );
    }
    if root_stamp(&job.stage, sftp)?.is_some() {
        marker(&job, sftp, false)?;
        if let Some(s) = sftp {
            clean_remote(s, &job.stage, 0)?;
        } else {
            clean_local(&job.stage)?;
        }
    }
    journal.remove()?;
    Ok(format!(
        "Rsync recovery complete: {outcome}; owned staging/backup cleaned"
    ))
}
fn error(_: impl std::fmt::Display) -> String {
    "Rsync file operation failed; inspect status and recovery records".into()
}
fn check(cancel: &AtomicBool) -> Result<(), String> {
    if cancel.load(Ordering::Relaxed) {
        Err("Rsync cancelled".into())
    } else {
        Ok(())
    }
}
fn valid(path: &Path) -> Result<(), String> {
    if path.to_str().is_none() {
        return Err("Non-UTF-8 transfer paths are unsupported".into());
    }
    if path.as_os_str().len() > 4096
        || path.as_os_str().as_bytes().contains(&0)
        || path.components().any(|c| matches!(c, Component::ParentDir))
    {
        return Err("Invalid transfer path".into());
    }
    Ok(())
}
fn node(root: &Path, relative: &Path) -> PathBuf {
    if relative.as_os_str().is_empty() {
        root.to_owned()
    } else {
        root.join(relative)
    }
}
fn sftp_error(stage: &str, e: ssh2::Error) -> String {
    format!("SFTP {stage} failed (code {:?})", e.code())
}
fn digest(
    mut reader: impl Read,
    cancel: &AtomicBool,
    deadline: Instant,
) -> Result<(u64, String), String> {
    let (mut count, mut hash, mut data) = (0u64, Sha256::new(), [0u8; 65536]);
    loop {
        check(cancel)?;
        if Instant::now() > deadline {
            return Err("Rsync verification timed out".into());
        }
        let n = reader.read(&mut data).map_err(error)?;
        if n == 0 {
            break;
        }
        count += n as u64;
        if count > MAX_BYTES {
            return Err("Rsync file exceeds 32 GiB".into());
        }
        hash.update(&data[..n]);
    }
    Ok((count, format!("{:x}", hash.finalize())))
}
fn scan(root: &Path, sftp: Option<&Sftp>, cancel: &AtomicBool) -> Result<Manifest, String> {
    fn visit(
        root: &Path,
        relative: PathBuf,
        sftp: Option<&Sftp>,
        cancel: &AtomicBool,
        out: &mut Manifest,
        total: &mut u64,
        depth: usize,
        deadline: Instant,
    ) -> Result<(), String> {
        check(cancel)?;
        if Instant::now() > deadline {
            return Err("Rsync tree preparation timed out".into());
        }
        if depth > 64 || out.len() >= MAX_ENTRIES {
            return Err("Rsync tree exceeds 10,000 entries or 64 directory levels".into());
        }
        let path = node(root, &relative);
        valid(&path)?;
        let (mode, size) = if let Some(s) = sftp {
            let m = s.lstat(&path).map_err(error)?;
            (
                m.perm.ok_or("Remote permissions unavailable")?,
                m.size.unwrap_or(0),
            )
        } else {
            let m = fs::symlink_metadata(&path).map_err(error)?;
            (m.mode(), m.len())
        };
        let directory = match mode & 0o170000 {
            0o040000 => true,
            0o100000 => false,
            _ => {
                return Err(
                    "Rsync refuses symlinks, devices and special files; source unchanged".into(),
                );
            }
        };
        if !directory && size > MAX_BYTES.saturating_sub(*total) {
            return Err("Rsync tree exceeds 32 GiB".into());
        }
        let (bytes, hash) = if directory {
            (0, String::new())
        } else if let Some(s) = sftp {
            digest(s.open(&path).map_err(error)?, cancel, deadline)?
        } else {
            digest(
                fs::OpenOptions::new()
                    .read(true)
                    .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
                    .open(&path)
                    .map_err(error)?,
                cancel,
                deadline,
            )?
        };
        if !directory && bytes != size {
            return Err("Source changed while preparing transfer".into());
        }
        *total = total.checked_add(bytes).ok_or("Transfer size overflow")?;
        if *total > MAX_BYTES {
            return Err("Rsync tree exceeds 32 GiB".into());
        }
        out.insert(
            relative.clone(),
            Entry {
                directory,
                mode: mode & 0o777,
                bytes,
                hash,
            },
        );
        if directory {
            let names = if let Some(s) = sftp {
                remote_names(s, &path)?
            } else {
                fs::read_dir(&path)
                    .map_err(error)?
                    .take(MAX_ENTRIES + 1)
                    .map(|e| e.map(|e| e.file_name()).map_err(error))
                    .collect::<Result<Vec<_>, _>>()?
            };
            if names.len() > MAX_ENTRIES {
                return Err("Rsync directory exceeds 10,000 entries".into());
            }
            for name in names {
                if name == "." || name == ".." {
                    continue;
                }
                let part = Path::new(&name);
                if part.components().count() != 1
                    || !matches!(part.components().next(), Some(Component::Normal(_)))
                {
                    return Err("Invalid tree entry".into());
                }
                visit(
                    root,
                    relative.join(name),
                    sftp,
                    cancel,
                    out,
                    total,
                    depth + 1,
                    deadline,
                )?;
            }
        }
        Ok(())
    }
    let mut result = Manifest::new();
    visit(
        root,
        PathBuf::new(),
        sftp,
        cancel,
        &mut result,
        &mut 0,
        0,
        Instant::now() + Duration::from_secs(3600),
    )?;
    Ok(result)
}
// rsync's --rsh parser supports adjacent quoted strings, not backslash escapes.
fn rsh_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}
fn ssh_path(path: &Path) -> Result<String, String> {
    let value = path.to_str().ok_or("SSH key/trust path must be UTF-8")?;
    if value.chars().any(char::is_control) || value.contains('$') {
        return Err(
            "SSH key/trust paths with variable expansion syntax are unsupported; use SFTP".into(),
        );
    }
    Ok(value.replace('%', "%%"))
}
fn ssh_args(p: &Profile) -> Result<(Vec<String>, String), String> {
    p.validate()?;
    if p.protocol != "SSH"
        || (!p.preferences.gateway.is_empty() && p.preferences.ssh_jump_alias.is_empty())
    {
        return Err(
            "Rsync requires SSH with a direct route or an explicit jump alias; gateway metadata alone is not a route".into(),
        );
    }
    let (host, port) = crate::profiles::ssh_endpoint(&p.server)?;
    if host.is_empty()
        || host.starts_with('-')
        || !host
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b".-_:%".contains(&b))
    {
        return Err("Invalid SSH host".into());
    }
    let known = if p.preferences.known_hosts.is_empty() {
        PathBuf::from(std::env::var_os("HOME").ok_or("Home unavailable")?).join(".ssh/known_hosts")
    } else {
        PathBuf::from(&p.preferences.known_hosts)
    };
    let known = ssh_path(&known)?.replace('\\', "\\\\").replace('"', "\\\"");
    let mut args = vec![
        "-F".into(),
        "/dev/null".into(),
        "-T".into(),
        "-p".into(),
        port.to_string(),
        "-o".into(),
        "BatchMode=yes".into(),
        "-o".into(),
        "StrictHostKeyChecking=yes".into(),
        "-o".into(),
        "UpdateHostKeys=no".into(),
        "-o".into(),
        "GlobalKnownHostsFile=/dev/null".into(),
        "-o".into(),
        format!("UserKnownHostsFile=\"{known}\""),
        "-o".into(),
        "ConnectTimeout=8".into(),
        "-o".into(),
        "ServerAliveInterval=5".into(),
        "-o".into(),
        "ServerAliveCountMax=3".into(),
        "-o".into(),
        "LogLevel=ERROR".into(),
    ];
    if !p.preferences.ssh_jump_alias.is_empty() {
        args.extend([
            "-o".into(),
            format!(
                "ProxyCommand={}",
                crate::ssh_jump::proxy_command(&p.preferences.ssh_jump_alias, &host, port)?
            ),
        ]);
    }
    if !p.preferences.identity_file.is_empty() {
        args.extend([
            "-i".into(),
            ssh_path(Path::new(&p.preferences.identity_file))?,
            "-o".into(),
            "IdentitiesOnly=yes".into(),
        ]);
    }
    if !p.username.is_empty() {
        args.extend(["-l".into(), p.username.clone()]);
    }
    Ok((args, host))
}
/// Secondary SSH channels must authenticate the peer already verified by
/// SFTP, rather than reread a possibly changed trust file. A sealed anonymous
/// file needs no persistent cleanup, including after a client crash.
fn bind_original_host(args: &mut [String], session: &ssh2::Session) -> Result<fs::File, String> {
    let (key, kind) = session
        .host_key()
        .ok_or("Original SSH host key unavailable")?;
    let mut known = session.known_hosts().map_err(error)?;
    known
        .add("*", key, "RUWAC original peer", kind.into())
        .map_err(error)?;
    let entries = known.hosts().map_err(error)?;
    let line = known
        .write_string(
            entries.first().ok_or("Original SSH host key unavailable")?,
            ssh2::KnownHostFileKind::OpenSSH,
        )
        .map_err(error)?;
    let fd = unsafe {
        libc::memfd_create(
            c"ruwac-original-host".as_ptr(),
            libc::MFD_CLOEXEC | libc::MFD_ALLOW_SEALING,
        )
    };
    if fd < 0 {
        return Err("Original SSH host pin unavailable".into());
    }
    let mut file = unsafe { fs::File::from_raw_fd(fd) };
    file.set_permissions(fs::Permissions::from_mode(0o600))
        .map_err(error)?;
    file.write_all(line.as_bytes()).map_err(error)?;
    let seals = libc::F_SEAL_WRITE | libc::F_SEAL_SHRINK | libc::F_SEAL_GROW | libc::F_SEAL_SEAL;
    if unsafe { libc::fcntl(fd, libc::F_ADD_SEALS, seals) } < 0 {
        return Err("Original SSH host pin could not be sealed".into());
    }
    let option = args
        .iter_mut()
        .find(|s| s.starts_with("UserKnownHostsFile="))
        .ok_or("SSH host verification option unavailable")?;
    *option = format!(
        "UserKnownHostsFile=\"/proc/{}/fd/{fd}\"",
        std::process::id()
    );
    Ok(file)
}
fn version(text: &[u8]) -> Result<(), String> {
    let text = std::str::from_utf8(text).map_err(error)?;
    let line = text
        .lines()
        .find_map(|l| l.strip_prefix("rsync  version "))
        .ok_or("Remote rsync was not detected")?;
    let mut digits = line.split_whitespace().next().unwrap_or("").split('.');
    let major = digits
        .next()
        .and_then(|v| v.parse::<u32>().ok())
        .ok_or("Unknown rsync version")?;
    let minor = digits
        .next()
        .and_then(|v| v.parse::<u32>().ok())
        .ok_or("Unknown rsync version")?;
    if (major, minor) < (3, 4) {
        return Err("Rsync 3.4 or newer is required on both endpoints".into());
    }
    Ok(())
}
fn progress_line(line: &[u8], progress: &AtomicU64) {
    if let Ok(text) = std::str::from_utf8(line) {
        let mut fields = text.split_whitespace();
        if let (Some(count), Some(percent)) = (fields.next(), fields.next()) {
            if percent.ends_with('%') {
                // rsync 3.5.2 appends "B" to the byte count ("32,768B  0%");
                // earlier releases print the bare number.
                let count = count.strip_suffix('B').unwrap_or(count);
                if let Ok(n) = count.replace(',', "").parse::<u64>() {
                    progress.fetch_max(n.min(MAX_BYTES), Ordering::Relaxed);
                }
            }
        }
    }
}
fn run(
    mut command: Command,
    cancel: &AtomicBool,
    progress: Arc<AtomicU64>,
    probe: bool,
) -> Result<Vec<u8>, String> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0)
        .env("LC_ALL", "C");
    for name in [
        "RSYNC_RSH",
        "RSYNC_OLD_ARGS",
        "RSYNC_PROTECT_ARGS",
        "RSYNC_PASSWORD",
        "RSYNC_PARTIAL_DIR",
        "RSYNC_PROXY",
        "RSYNC_CONNECT_PROG",
        "RSYNC_SHELL",
        "RSYNC_ICONV",
        "RSYNC_CHECKSUM_LIST",
        "RSYNC_COMPRESS_LIST",
        "RSYNC_MAX_ALLOC",
    ] {
        command.env_remove(name);
    }
    let parent = unsafe { libc::getpid() };
    unsafe {
        command.pre_exec(move || {
            if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            if libc::getppid() != parent {
                libc::_exit(125);
            }
            Ok(())
        });
    }
    let mut child = command
        .spawn()
        .map_err(|_| "SSH/rsync executable unavailable")?;
    let pid = child.id() as i32;
    let mut stdout = child.stdout.take().unwrap();
    let mut stderr = child.stderr.take().unwrap();
    let reader = std::thread::spawn(move || {
        let (mut capture, mut line, mut block) = (Vec::new(), Vec::new(), [0; 4096]);
        while let Ok(n) = stdout.read(&mut block) {
            if n == 0 {
                break;
            }
            for &b in &block[..n] {
                if probe && capture.len() < 65536 {
                    capture.push(b);
                }
                if b == b'\r' || b == b'\n' {
                    progress_line(&line, &progress);
                    line.clear();
                } else if line.len() < 1024 {
                    line.push(b);
                }
            }
        }
        capture
    });
    let errors = std::thread::spawn(move || {
        let _ = std::io::copy(&mut stderr, &mut std::io::sink());
    });
    let deadline = Instant::now()
        + if probe {
            Duration::from_secs(30)
        } else {
            Duration::from_secs(3600)
        };
    let mut interrupted = false;
    let result = loop {
        if cancel.load(Ordering::Relaxed) || Instant::now() > deadline {
            interrupted = true;
            unsafe {
                libc::kill(-pid, libc::SIGTERM);
            }
            std::thread::sleep(Duration::from_millis(100));
            unsafe {
                libc::kill(-pid, libc::SIGKILL);
            }
            break child.wait().map_err(error);
        }
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(e) => {
                unsafe {
                    libc::kill(-pid, libc::SIGKILL);
                }
                let _ = child.wait();
                break Err(error(e));
            }
        }
    };
    let captured = reader.join().map_err(|_| "Rsync output worker stopped")?;
    let _ = errors.join();
    if interrupted {
        return Err("Rsync cancelled or timed out; destination preserved".into());
    }
    if !result?.success() {
        return Err(if probe{"SSH/rsync probe failed; use SFTP for saved passwords or unlock the SSH key in your agent"}else{"Rsync transfer failed; destination preserved"}.into());
    }
    Ok(captured)
}
fn permissions(mode: u32) -> FileStat {
    FileStat {
        size: None,
        uid: None,
        gid: None,
        perm: Some(mode),
        atime: None,
        mtime: None,
    }
}
fn remote_names(sftp: &Sftp, path: &Path) -> Result<Vec<std::ffi::OsString>, String> {
    let mut directory = sftp
        .opendir(path)
        .map_err(|e| sftp_error("directory open", e))?;
    let mut names = Vec::new();
    loop {
        match directory.readdir() {
            Ok((p, _)) => {
                if p == Path::new(".") || p == Path::new("..") {
                    continue;
                }
                if names.len() >= MAX_ENTRIES {
                    return Err("Directory exceeds 10,000 entries".into());
                }
                names.push(p.file_name().ok_or("Invalid directory entry")?.to_owned());
            }
            Err(e)
                if e.code() == ssh2::ErrorCode::SFTP(1)
                    || e.code() == ssh2::ErrorCode::Session(-16) =>
            {
                break;
            }
            Err(e) => return Err(error(e)),
        }
    }
    Ok(names)
}
fn clean_remote(sftp: &Sftp, path: &Path, depth: usize) -> Result<(), String> {
    if depth > 80 {
        return Err("Owned staging cleanup exceeded depth limit".into());
    }
    let stat = sftp
        .lstat(path)
        .map_err(|e| sftp_error("cleanup metadata", e))?;
    if stat.perm.is_some_and(|m| m & 0o170000 == 0o040000) {
        sftp.setstat(path, permissions(0o700))
            .map_err(|e| sftp_error("cleanup permissions", e))?;
        for name in remote_names(sftp, path)? {
            let name = name.as_os_str();
            if name == "." || name == ".." {
                continue;
            }
            clean_remote(sftp, &path.join(name), depth + 1)?;
        }
        sftp.rmdir(path)
            .map_err(|e| sftp_error("staging directory removal", e))
    } else {
        sftp.unlink(path)
            .map_err(|e| sftp_error("staging file removal", e))
    }
}
fn clean_local(path: &Path) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path).map_err(error)?;
    if metadata.is_dir() {
        fs::set_permissions(path, fs::Permissions::from_mode(0o700)).map_err(error)?;
        for entry in fs::read_dir(path).map_err(error)? {
            clean_local(&entry.map_err(error)?.path())?;
        }
        fs::remove_dir(path).map_err(error)
    } else {
        fs::remove_file(path).map_err(error)
    }
}
fn rename_local(from: &Path, to: &Path) -> Result<(), String> {
    let from = std::ffi::CString::new(from.as_os_str().as_bytes()).map_err(error)?;
    let to = std::ffi::CString::new(to.as_os_str().as_bytes()).map_err(error)?;
    if unsafe {
        libc::syscall(
            libc::SYS_renameat2,
            libc::AT_FDCWD,
            from.as_ptr(),
            libc::AT_FDCWD,
            to.as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    } != 0
    {
        return Err(error(std::io::Error::last_os_error()));
    }
    for path in [from.as_c_str(), to.as_c_str()] {
        let p = Path::new(std::ffi::OsStr::from_bytes(path.to_bytes()));
        if let Some(parent) = p.parent() {
            fs::File::open(parent)
                .and_then(|f| f.sync_all())
                .map_err(error)?;
        }
    }
    Ok(())
}
pub fn transfer(
    p: &Profile,
    local: &Path,
    remote: &Path,
    upload: bool,
    overwrite: bool,
    cancel: &AtomicBool,
    progress: Arc<AtomicU64>,
) -> Result<Receipt, String> {
    transfer_observed(
        p,
        local,
        remote,
        upload,
        overwrite,
        cancel,
        progress,
        |_, _| Ok(()),
    )
}
pub fn transfer_observed(
    p: &Profile,
    local: &Path,
    remote: &Path,
    upload: bool,
    overwrite: bool,
    cancel: &AtomicBool,
    progress: Arc<AtomicU64>,
    mut observer: impl FnMut(&str, &str) -> Result<(), String>,
) -> Result<Receipt, String> {
    check(cancel)?;
    valid(local)?;
    valid(remote)?;
    let (mut args, host) = ssh_args(p)?;
    let mut local_probe = Command::new("rsync");
    local_probe.arg("--version");
    version(&run(local_probe, cancel, progress.clone(), true)?)?;
    let session = crate::files::connect(p)?;
    let _original_host = bind_original_host(&mut args, &session)?;
    let mut remote_probe = Command::new("ssh");
    remote_probe
        .args(&args)
        .args(["--", &host, "rsync", "--version"]);
    version(&run(remote_probe, cancel, progress.clone(), true)?)?;
    let sftp = session.sftp().map_err(error)?;
    let remote = crate::files::remote_path(&sftp, remote, upload)?;
    let local = if upload {
        if fs::symlink_metadata(local)
            .map_err(error)?
            .file_type()
            .is_symlink()
        {
            return Err("Local source symlinks are refused".into());
        }
        fs::canonicalize(local).map_err(error)?
    } else {
        fs::canonicalize(
            local
                .parent()
                .ok_or("Choose a local destination directory")?,
        )
        .map_err(error)?
        .join(local.file_name().ok_or("Choose a destination name")?)
    };
    let (source, destination) = if upload {
        (&local, &remote)
    } else {
        (&remote, &local)
    };
    if source.file_name().is_none() {
        return Err("Select a source file/folder, not a filesystem root".into());
    }
    let manifest = scan(source, if upload { None } else { Some(&sftp) }, cancel)
        .map_err(|e| format!("Preparing source: {e}"))?;
    let root = manifest.get(Path::new("")).unwrap();
    let exists = if upload {
        match sftp.lstat(destination) {
            Ok(s) => {
                if s.perm.map(|m| m & 0o170000)
                    != Some(if root.directory { 0o040000 } else { 0o100000 })
                {
                    return Err("Destination type differs or is a symlink".into());
                }
                true
            }
            Err(e) if e.code() == ssh2::ErrorCode::SFTP(2) => false,
            Err(e) => return Err(error(e)),
        }
    } else {
        match fs::symlink_metadata(destination) {
            Ok(s) => {
                if s.file_type().is_symlink()
                    || s.is_dir() != root.directory
                    || (!s.is_dir() && !s.is_file())
                {
                    return Err("Destination type differs or is a symlink".into());
                }
                true
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => false,
            Err(e) => return Err(error(e)),
        }
    };
    if exists && !overwrite {
        return Err(
            "Destination exists; explicit whole-destination replacement is required".into(),
        );
    }
    let parent = destination.parent().ok_or("Choose a destination parent")?;
    let stage = parent.join(ruwac_protocol::id("ruwac-rsync-part"));
    let backup = parent.join(ruwac_protocol::id("ruwac-rsync-backup"));
    let original = if exists {
        Some(manifest_digest(&scan(
            destination,
            if upload { Some(&sftp) } else { None },
            cancel,
        )?))
    } else {
        None
    };
    let original_stamp = root_stamp(destination, if upload { Some(&sftp) } else { None })?;
    let id = ruwac_protocol::id("rsync");
    let journal = RecoveryJournal::open(&id)?;
    let mut job = Recovery {
        schema: 1,
        id,
        endpoint: p.id.clone(),
        binding: recovery_binding(p),
        host_sha256: crate::files_resume::host_key(&session)?,
        upload,
        destination: destination.clone(),
        stage: stage.clone(),
        backup: backup.clone(),
        phase: "copy".into(),
        expected: manifest_digest(&manifest),
        original,
        original_stamp,
    };
    let namespace = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(journal.root.join(".namespace.lock"))
        .map_err(error)?;
    if !crate::files_resume::private_file(&namespace)
        || unsafe { libc::flock(namespace.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0
    {
        return Err("Recovery namespace busy".into());
    }
    if fs::read_dir(&journal.root)
        .map_err(error)?
        .filter_map(Result::ok)
        .filter(|e| e.path().extension().is_some_and(|s| s == "json"))
        .take(129)
        .count()
        >= 128
    {
        return Err("Too many pending recovery jobs".into());
    }
    journal.save(&job)?;
    drop(namespace);

    if upload {
        sftp.mkdir(&stage, 0o700).map_err(error)?;
    } else {
        fs::DirBuilder::new()
            .mode(0o700)
            .create(&stage)
            .map_err(error)?;
    }
    marker(&job, if upload { Some(&sftp) } else { None }, true)?;
    observer(&job.id, "copy")?;
    let filelist = journal.root.join(format!("{}.files", job.id));
    let result = (|| {
        let mut list = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&filelist)
            .map_err(error)?;
        let prefix = source
            .strip_prefix("/")
            .map_err(|_| "Rsync source must be absolute")?;
        if prefix.as_os_str().is_empty() {
            return Err("Select one source file or directory, not a filesystem root".into());
        }
        for relative in manifest.keys() {
            list.write_all(node(prefix, relative).as_os_str().as_bytes())
                .map_err(error)?;
            list.write_all(&[0]).map_err(error)?;
        }
        list.sync_all().map_err(error)?;
        drop(list);
        let remote_host = if host.contains(':') {
            format!("[{host}]")
        } else {
            host.clone()
        };
        let remote_arg = if upload {
            format!(
                "{remote_host}:{}/",
                stage.to_str().ok_or("Remote path must be UTF-8")?
            )
        } else {
            format!("{remote_host}:/")
        };
        let rsh = std::iter::once("ssh")
            .chain(args.iter().map(String::as_str))
            .map(rsh_quote)
            .collect::<Vec<_>>()
            .join(" ");
        let mut command = Command::new("rsync");
        command
            .args([
                "--secluded-args",
                "--from0",
                "--dirs",
                "--relative",
                "--no-implied-dirs",
                "--no-links",
                "--no-specials",
                "--no-devices",
                "--perms",
                "--times",
                "--fsync",
                "--chmod=D700,F600",
                "--info=progress2,name0",
                "--no-inc-recursive",
                "--max-alloc=67108864",
                "--max-size=34359738368",
                "--rsh",
            ])
            .arg(rsh)
            .arg("--files-from")
            .arg(&filelist)
            .arg("--");
        if upload {
            command.arg("/").arg(remote_arg);
        } else {
            command.arg(remote_arg).arg(format!("{}/", stage.display()));
        }
        run(command, cancel, progress.clone(), false)?;
        check(cancel)?;
        let payload = stage.join(prefix);
        let received = scan(&payload, if upload { Some(&sftp) } else { None }, cancel)?;
        if received.len() != manifest.len()
            || manifest.iter().any(|(path, e)| {
                received.get(path).is_none_or(|r| {
                    r.directory != e.directory || r.bytes != e.bytes || r.hash != e.hash
                })
            })
        {
            return Err("Rsync SHA-256/tree verification failed; destination preserved".into());
        }
        for (path, e) in manifest.iter().rev() {
            let target = node(&payload, path);
            if upload {
                sftp.setstat(&target, permissions(e.mode))
                    .map_err(|e| sftp_error("destination permissions", e))?;
            } else {
                fs::set_permissions(&target, fs::Permissions::from_mode(e.mode)).map_err(error)?;
            }
        }
        check(cancel)?;
        let _mutation = mutation_guard(cancel)?;
        let side = if upload { Some(&sftp) } else { None };
        if root_stamp(destination, side)? != job.original_stamp
            || job.original.as_ref().is_some_and(|expected| {
                scan(destination, side, cancel)
                    .map(|m| manifest_digest(&m) != *expected)
                    .unwrap_or(true)
            })
        {
            return Err("Original destination changed before commit; paths preserved".into());
        }
        job.phase = "backup-intent".into();
        journal.save(&job)?;
        observer(&job.id, "backup-intent")?;
        if exists {
            if upload {
                sftp.rename(destination, &backup, Some(ssh2::RenameFlags::empty()))
                    .map_err(error)?;
            } else {
                rename_local(destination, &backup)?;
            }
        }
        job.phase = "backup-moved".into();
        journal.save(&job)?;
        observer(&job.id, "backup-moved")?;
        if exists {
            backup_unchanged(&job, side, cancel)?;
        }
        let commit = if upload {
            sftp.rename(&payload, destination, Some(ssh2::RenameFlags::empty()))
                .map_err(error)
        } else {
            rename_local(&payload, destination)
        };
        if let Err(e) = commit {
            if exists {
                let restored = if upload {
                    sftp.rename(&backup, destination, Some(ssh2::RenameFlags::empty()))
                        .map_err(error)
                } else {
                    rename_local(&backup, destination)
                };
                if restored.is_err() {
                    return Err(format!(
                        "Destination changed during commit; original retained at {}",
                        backup.display()
                    ));
                }
            }
            return Err(e);
        }
        job.phase = "published".into();
        journal.save(&job)?;
        observer(&job.id, "published")?;
        if exists {
            backup_unchanged(&job, side, cancel)?;
            if manifest_digest(&scan(destination, side, cancel)?) != job.expected {
                return Err("Published destination changed; original backup retained".into());
            }
            let cleanup = if upload {
                clean_remote(&sftp, &backup, 0)
            } else {
                if root.directory {
                    clean_local(&backup)
                } else {
                    fs::remove_file(&backup).map_err(error)
                }
            };
            if cleanup.is_err() {
                return Err(format!(
                    "Transfer committed and verified; original backup retained at {}",
                    backup.display()
                ));
            }
        }
        job.phase = "cleaned".into();
        journal.save(&job)?;
        observer(&job.id, "cleaned")?;
        let (mut bytes, mut files, mut directories, mut hash) =
            (0u64, 0usize, 0usize, Sha256::new());
        for (path, e) in &manifest {
            hash.update((path.as_os_str().len() as u64).to_le_bytes());
            hash.update(path.as_os_str().as_bytes());
            hash.update([u8::from(e.directory)]);
            hash.update(e.mode.to_le_bytes());
            hash.update(e.bytes.to_le_bytes());
            hash.update(e.hash.as_bytes());
            bytes += e.bytes;
            if e.directory {
                directories += 1
            } else {
                files += 1
            };
        }
        progress.store(bytes, Ordering::Relaxed);
        Ok(Receipt {
            bytes,
            files,
            directories,
            sha256: format!("{:x}", hash.finalize()),
        })
    })();
    let _ = fs::remove_file(filelist);
    if let Err(e) = result.as_ref() {
        match recover_locked(
            &job,
            &journal,
            if upload { Some(&sftp) } else { None },
            &AtomicBool::new(false),
        ) {
            Ok(status) => return Err(format!("{e}; {status}")),
            Err(_) => {
                return Err(format!(
                    "{e}; recovery job {} retained for inspection",
                    job.id
                ));
            }
        }
    } else {
        marker(&job, if upload { Some(&sftp) } else { None }, false)?;
        if upload {
            clean_remote(&sftp, &stage, 0)?;
        } else {
            clean_local(&stage)?;
        }
        journal.remove()?;
    }
    result
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bounded_progress_and_rsync_quote_parser() {
        let n = AtomicU64::new(0);
        progress_line(b" 1,234 23% 5MB/s", &n);
        assert_eq!(n.load(Ordering::Relaxed), 1234);
        progress_line(b"999 filename", &n);
        assert_eq!(n.load(Ordering::Relaxed), 1234);
        // rsync 3.5.2 progress2 suffixes the byte count with "B".
        progress_line(b"    446,791,680B  41%  447MB/s    0:00:01", &n);
        assert_eq!(n.load(Ordering::Relaxed), 446_791_680);
        progress_line(
            b"  1,073,741,824B 100%  593MB/s    0:00:01 (xfr#1, to-chk=0/1)",
            &n,
        );
        assert_eq!(n.load(Ordering::Relaxed), 1_073_741_824);
        // Human-readable units are not byte counts and are ignored.
        let h = AtomicU64::new(0);
        progress_line(b"  1.07G 100%  593MB/s", &h);
        progress_line(b"  12kB  0%", &h);
        assert_eq!(h.load(Ordering::Relaxed), 0);
        assert_eq!(rsh_quote("a'b c"), "'a'\"'\"'b c'");
        assert!(version(b"rsync  version 3.5.1-g123 protocol version 33").is_ok());
        assert!(version(b"rsync  version 2.6.9").is_err());
        assert!(version(b"rsync  version 3.2.7").is_err());
        assert!(valid(Path::new("a/../b")).is_err());
    }
    #[test]
    fn local_no_replace_keeps_existing_destination() {
        let dir = std::env::temp_dir().join(ruwac_protocol::id("rsync-rename-fixture"));
        fs::create_dir(&dir).unwrap();
        let a = dir.join("a");
        let b = dir.join("b");
        fs::write(&a, b"new").unwrap();
        fs::write(&b, b"original").unwrap();
        assert!(rename_local(&a, &b).is_err());
        assert_eq!(fs::read(&b).unwrap(), b"original");
        fs::remove_file(&b).unwrap();
        rename_local(&a, &b).unwrap();
        assert_eq!(fs::read(&b).unwrap(), b"new");
        fs::remove_dir_all(dir).unwrap();
    }
}
