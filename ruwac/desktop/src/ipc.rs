//! Same-UID, bounded GUI IPC. Socket workers never access GTK or the compositor’s clipboards.
use ruwac_protocol::{Error, Request, read_frame, response, write_frame};
use serde_json::Value;
use std::{
    fs,
    io::{self, Read},
    os::unix::{
        fs::{FileTypeExt, MetadataExt, OpenOptionsExt, PermissionsExt},
        io::AsRawFd,
        net::{UnixListener, UnixStream},
    },
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc::{self, Receiver, SyncSender},
    },
    thread,
    time::{Duration, Instant},
};
pub struct Task {
    pub request: Request,
    pub reply: SyncSender<ruwac_protocol::Result<Value>>,
    pub actor_uid: u32,
    pub actor_pid: i32,
    pub expires: Instant,
}
pub struct Server {
    stop: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
    path: PathBuf,
    inode: u64,
    _lock: fs::File,
}
struct Count(Arc<AtomicUsize>);
impl Drop for Count {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}
struct Deadline<'a> {
    stream: &'a mut UnixStream,
    until: Instant,
}
impl Read for Deadline<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let remaining = self
            .until
            .checked_duration_since(Instant::now())
            .ok_or_else(|| io::Error::new(io::ErrorKind::TimedOut, "GUI request deadline"))?;
        self.stream
            .set_read_timeout(Some(remaining.max(Duration::from_millis(1))))?;
        self.stream.read(buf)
    }
}
pub fn socket_path(daemon: &Path) -> PathBuf {
    daemon.with_extension("gui.sock")
}
// Startup must never block GTK behind a live listener's full backlog.
fn stale_socket(path: &Path) -> io::Result<bool> {
    use std::os::{
        fd::{FromRawFd, OwnedFd},
        unix::ffi::OsStrExt,
    };
    let bytes = path.as_os_str().as_bytes();
    let mut addr: libc::sockaddr_un = unsafe { std::mem::zeroed() };
    if bytes.len() >= addr.sun_path.len() || bytes.contains(&0) {
        return Err(io::Error::other("invalid GUI socket pathname"));
    }
    addr.sun_family = libc::AF_UNIX as libc::sa_family_t;
    for (slot, byte) in addr.sun_path.iter_mut().zip(bytes) {
        *slot = *byte as libc::c_char;
    }
    let raw = unsafe {
        libc::socket(
            libc::AF_UNIX,
            libc::SOCK_STREAM | libc::SOCK_NONBLOCK | libc::SOCK_CLOEXEC,
            0,
        )
    };
    if raw < 0 {
        return Err(io::Error::last_os_error());
    }
    let fd = unsafe { OwnedFd::from_raw_fd(raw) };
    let code = unsafe {
        libc::connect(
            fd.as_raw_fd(),
            &addr as *const _ as *const libc::sockaddr,
            std::mem::size_of_val(&addr) as libc::socklen_t,
        )
    };
    Ok(code != 0 && io::Error::last_os_error().raw_os_error() == Some(libc::ECONNREFUSED))
}
impl Server {
    pub fn start(daemon: &Path) -> io::Result<(Self, Receiver<Task>)> {
        let path = socket_path(daemon);
        let parent = path
            .parent()
            .ok_or_else(|| io::Error::other("GUI socket parent missing"))?;
        let uid = unsafe { libc::geteuid() };
        let meta = fs::symlink_metadata(parent)?;
        if !meta.is_dir() || meta.uid() != uid || meta.mode() & 0o777 != 0o700 {
            return Err(io::Error::other(
                "GUI socket requires owned nonsymlink private directory",
            ));
        }
        let lock = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(path.with_extension("lock"))?;
        let m = lock.metadata()?;
        if !m.is_file() || m.uid() != uid || m.mode() & 0o777 != 0o600 {
            return Err(io::Error::other("unsafe GUI socket lock"));
        }
        if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
            return Err(io::Error::other("GUI endpoint already owned"));
        }
        match fs::symlink_metadata(&path) {
            Ok(m) => {
                if !m.file_type().is_socket() || m.uid() != uid || m.mode() & 0o777 != 0o600 {
                    return Err(io::Error::other("unsafe existing GUI socket preserved"));
                }
                if !stale_socket(&path)? {
                    return Err(io::Error::other(
                        "live or indeterminate GUI socket preserved",
                    ));
                }
                let now = fs::symlink_metadata(&path)?;
                if now.ino() != m.ino() {
                    return Err(io::Error::other("GUI socket changed; preserved"));
                }
                fs::remove_file(&path)?;
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => (),
            Err(e) => return Err(e),
        }
        let listener = UnixListener::bind(&path)?;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
        let inode = fs::symlink_metadata(&path)?.ino();
        listener.set_nonblocking(true)?;
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = stop.clone();
        let count = Arc::new(AtomicUsize::new(0));
        let (tx, rx) = mpsc::sync_channel::<Task>(16);
        let worker = thread::spawn(move || {
            while !stopped.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        let mut cred: libc::ucred = unsafe { std::mem::zeroed() };
                        let mut size = std::mem::size_of_val(&cred) as libc::socklen_t;
                        if unsafe {
                            libc::getsockopt(
                                stream.as_raw_fd(),
                                libc::SOL_SOCKET,
                                libc::SO_PEERCRED,
                                &mut cred as *mut _ as *mut _,
                                &mut size,
                            )
                        } != 0
                            || cred.uid != uid
                        {
                            continue;
                        }
                        if count.fetch_add(1, Ordering::SeqCst) >= 8 {
                            count.fetch_sub(1, Ordering::SeqCst);
                            continue;
                        }
                        let guard = Count(count.clone());
                        let tx = tx.clone();
                        let stopped = stopped.clone();
                        thread::spawn(move || {
                            let _guard = guard;
                            let _ = stream.set_write_timeout(Some(Duration::from_secs(1)));
                            let parsed = read_frame(&mut Deadline {
                                stream: &mut stream,
                                until: Instant::now() + Duration::from_secs(2),
                            })
                            .and_then(|v| {
                                serde_json::from_value::<Request>(v).map_err(io::Error::other)
                            });
                            let (id, result) = match parsed {
                                Ok(r) => {
                                    let id = r.id.clone();
                                    let result = if r.version != 0 {
                                        Err(Error::new(
                                            "UNSUPPORTED_VERSION",
                                            "GUI protocol requires version 0",
                                        ))
                                    } else if r.idempotency_key.as_ref().is_some_and(|k| {
                                        k.is_empty()
                                            || k.len() > 128
                                            || k.chars().any(char::is_control)
                                            || !matches!(
                                                r.method.as_str(),
                                                "workspace.save"
                                                    | "workspace.open"
                                                    | "workspace.restore"
                                                    | "workspace.openWindow"
                                            )
                                    }) {
                                        Err(Error::new(
                                            "INVALID_REQUEST",
                                            "GUI operations do not accept idempotency keys",
                                        ))
                                    } else if stopped.load(Ordering::SeqCst) {
                                        Err(Error::new("GUI_UNAVAILABLE", "GUI is closing"))
                                    } else {
                                        let (reply, answer) = mpsc::sync_channel(1);
                                        match tx.try_send(Task {
                                            request: r,
                                            reply,
                                            actor_uid: cred.uid,
                                            actor_pid: cred.pid,
                                            expires: Instant::now() + Duration::from_secs(2),
                                        }) {
                                            Ok(()) => answer
                                                .recv_timeout(Duration::from_secs(2))
                                                .unwrap_or_else(|_| {
                                                    Err(Error::new(
                                                        "GUI_TIMEOUT",
                                                        "GUI dispatch deadline",
                                                    ))
                                                }),
                                            Err(_) => Err(Error::new(
                                                "BUSY",
                                                "GUI request queue unavailable",
                                            )),
                                        }
                                    };
                                    (id, result)
                                }
                                Err(_) => (
                                    Value::Null,
                                    Err(Error::new(
                                        "INVALID_REQUEST",
                                        "Malformed, oversized or timed-out GUI request",
                                    )),
                                ),
                            };
                            let _ = write_frame(&mut stream, &response(&id, result));
                        });
                    }
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(10))
                    }
                    Err(_) => break,
                }
            }
        });
        Ok((
            Self {
                stop,
                thread: Some(worker),
                path,
                inode,
                _lock: lock,
            },
            rx,
        ))
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
        if fs::symlink_metadata(&self.path)
            .is_ok_and(|m| m.file_type().is_socket() && m.ino() == self.inode)
        {
            let _ = fs::remove_file(&self.path);
        }
    }
}
