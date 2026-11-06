//! Short-lived, same-user PNG handoff to an explicitly identified local TUI.
//! This only binds the transport. Callers retain focus, pane, agent and file checks.
use std::{
    fs::{self, File, OpenOptions},
    io,
    net::Shutdown,
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::{
            ffi::OsStrExt,
            fs::{DirBuilderExt, FileTypeExt, MetadataExt, OpenOptionsExt, PermissionsExt},
            net::{UnixListener, UnixStream},
        },
    },
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

pub const MAX_MESSAGE: usize = 4096;
pub const LEASE: Duration = Duration::from_secs(3);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Process {
    pub pid: u32,
    pub start: u64,
}

impl Process {
    pub fn current() -> io::Result<Self> {
        Self::read(std::process::id())
    }

    pub fn read(pid: u32) -> io::Result<Self> {
        let root = PathBuf::from(format!("/proc/{pid}"));
        if pid == 0 || fs::metadata(&root)?.uid() != unsafe { libc::geteuid() } {
            return Err(io::Error::other("PNG handoff process owner rejected"));
        }
        let stat = fs::read_to_string(root.join("stat"))?;
        let start = stat
            .rsplit_once(") ")
            .and_then(|(_, fields)| fields.split_whitespace().nth(19))
            .and_then(|s| s.parse().ok())
            .ok_or_else(|| io::Error::other("PNG handoff process identity unavailable"))?;
        Ok(Self { pid, start })
    }

    pub fn live(self) -> bool {
        Self::read(self.pid).is_ok_and(|now| now == self)
    }
}

fn directory(create: bool) -> io::Result<PathBuf> {
    let runtime = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "No local GUI runtime"))?;
    private(&runtime, 0o700, false)?;
    let root = runtime.join("ruwac-image-offers");
    if create {
        match fs::DirBuilder::new().mode(0o700).create(&root) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e),
        }
    }
    private(&root, 0o700, false)?;
    Ok(root)
}

fn private(path: &Path, mode: u32, socket: bool) -> io::Result<fs::Metadata> {
    let m = fs::symlink_metadata(path)?;
    if m.uid() != unsafe { libc::geteuid() }
        || m.mode() & 0o777 != mode
        || (socket && !m.file_type().is_socket())
        || (!socket && !m.is_dir())
        || m.file_type().is_symlink()
    {
        return Err(io::Error::other("PNG handoff ownership or mode rejected"));
    }
    Ok(m)
}

fn stem(process: Process) -> String {
    format!("{}-{}", process.pid, process.start)
}

fn peer(stream: &UnixStream) -> io::Result<Process> {
    let mut cred: libc::ucred = unsafe { std::mem::zeroed() };
    let mut length = std::mem::size_of_val(&cred) as libc::socklen_t;
    if unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            (&mut cred as *mut libc::ucred).cast(),
            &mut length,
        )
    } != 0
        || cred.uid != unsafe { libc::geteuid() }
        || cred.pid <= 0
    {
        return Err(io::Error::other("PNG handoff peer rejected"));
    }
    Process::read(cred.pid as u32)
}

/// A full listen backlog must not stall the TUI's input loop.
fn connect(path: &Path) -> io::Result<UnixStream> {
    let bytes = path.as_os_str().as_bytes();
    let mut address: libc::sockaddr_un = unsafe { std::mem::zeroed() };
    if bytes.contains(&0) || bytes.len() >= address.sun_path.len() {
        return Err(io::Error::other("PNG handoff socket path rejected"));
    }
    address.sun_family = libc::AF_UNIX as libc::sa_family_t;
    for (out, byte) in address.sun_path.iter_mut().zip(bytes) {
        *out = *byte as libc::c_char;
    }
    let fd = unsafe {
        libc::socket(
            libc::AF_UNIX,
            libc::SOCK_STREAM | libc::SOCK_NONBLOCK | libc::SOCK_CLOEXEC,
            0,
        )
    };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    let stream = unsafe { UnixStream::from_raw_fd(fd) };
    let length = std::mem::offset_of!(libc::sockaddr_un, sun_path) + bytes.len() + 1;
    if unsafe {
        libc::connect(
            stream.as_raw_fd(),
            (&address as *const libc::sockaddr_un).cast(),
            length as libc::socklen_t,
        )
    } != 0
    {
        return Err(io::Error::last_os_error());
    }
    stream.set_nonblocking(false)?;
    stream.set_read_timeout(Some(LEASE))?;
    stream.set_write_timeout(Some(LEASE))?;
    Ok(stream)
}

struct Lock {
    path: PathBuf,
    inode: u64,
    _file: File,
}

impl Lock {
    fn acquire(path: PathBuf) -> io::Result<Self> {
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(&path)?;
        let m = file.metadata()?;
        if !m.is_file()
            || m.uid() != unsafe { libc::geteuid() }
            || m.mode() & 0o777 != 0o600
            || m.nlink() != 1
        {
            return Err(io::Error::other("PNG handoff lock rejected"));
        }
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
            return Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "A PNG handoff is already pending",
            ));
        }
        Ok(Self {
            path,
            inode: m.ino(),
            _file: file,
        })
    }
}

fn unlink_owned(path: &Path, inode: u64) {
    if fs::symlink_metadata(path).is_ok_and(|m| {
        m.ino() == inode && m.uid() == unsafe { libc::geteuid() } && !m.file_type().is_symlink()
    }) {
        let _ = fs::remove_file(path);
    }
}

impl Drop for Lock {
    fn drop(&mut self) {
        unlink_owned(&self.path, self.inode);
    }
}

pub struct Offer {
    listener: UnixListener,
    expected: Process,
    path: PathBuf,
    inode: u64,
    until: Instant,
    _lock: Lock,
}

impl Offer {
    /// Bind before forwarding the user's Ctrl+V. No PNG content is advertised.
    pub fn new(expected: Process) -> io::Result<Self> {
        if !expected.live() {
            return Err(io::Error::other("PNG handoff recipient changed"));
        }
        let root = directory(true)?;
        Self::in_directory(&root, expected)
    }

    fn in_directory(root: &Path, expected: Process) -> io::Result<Self> {
        private(root, 0o700, false)?;
        if !expected.live() {
            return Err(io::Error::other("PNG handoff recipient changed"));
        }
        let name = stem(expected);
        let lock = Lock::acquire(root.join(format!("{name}.lock")))?;
        let path = root.join(format!("{name}.sock"));
        if path.symlink_metadata().is_ok() {
            let m = private(&path, 0o600, true)?;
            match connect(&path) {
                Err(e) if e.raw_os_error() == Some(libc::ECONNREFUSED) => {
                    // The process lock excludes another legitimate producer.
                    unlink_owned(&path, m.ino());
                }
                _ => return Err(io::Error::other("PNG handoff socket is busy")),
            }
        }
        let listener = UnixListener::bind(&path)?;
        let inode = fs::symlink_metadata(&path)?.ino();
        let result = (|| {
            fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
            listener.set_nonblocking(true)?;
            Ok(Self {
                listener,
                expected,
                path: path.clone(),
                inode,
                until: Instant::now() + LEASE,
                _lock: lock,
            })
        })();
        if result.is_err() {
            unlink_owned(&path, inode);
        }
        result
    }

    pub fn accept(&self, allowed: impl Fn() -> bool) -> io::Result<Option<UnixStream>> {
        let mut refused = 0;
        while Instant::now() < self.until && allowed() && self.expected.live() {
            match self.listener.accept() {
                Ok((stream, _)) if peer(&stream).is_ok_and(|p| p == self.expected) => {
                    let remaining = self.until.saturating_duration_since(Instant::now());
                    if remaining.is_zero() {
                        return Ok(None);
                    }
                    stream.set_read_timeout(Some(remaining))?;
                    stream.set_write_timeout(Some(remaining))?;
                    return Ok(Some(stream));
                }
                Ok(_) => {
                    refused += 1;
                    if refused >= 16 {
                        return Err(io::Error::other("PNG handoff peer limit reached"));
                    }
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {}
                Err(e) => return Err(e),
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        Ok(None)
    }
}

impl Drop for Offer {
    fn drop(&mut self) {
        unlink_owned(&self.path, self.inode);
    }
}

/// Closing either direction cancels the lease without waiting for a worker read.
pub struct Cancel(UnixStream);
impl Drop for Cancel {
    fn drop(&mut self) {
        let _ = self.0.shutdown(Shutdown::Both);
    }
}

pub fn claim() -> io::Result<Option<(UnixStream, Cancel)>> {
    let root = match directory(false) {
        Ok(root) => root,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    };
    claim_in(&root)
}

fn claim_in(root: &Path) -> io::Result<Option<(UnixStream, Cancel)>> {
    private(root, 0o700, false)?;
    let path = root.join(format!("{}.sock", stem(Process::current()?)));
    match private(&path, 0o600, true) {
        Ok(_) => {}
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    }
    let stream = connect(&path)?;
    peer(&stream)?;
    let cancel = Cancel(stream.try_clone()?);
    Ok(Some((stream, cancel)))
}

/// A producer must check this and its GUI focus ticket before every delivery RPC.
pub fn consumer_live(stream: &UnixStream) -> bool {
    let mut byte = 0u8;
    let count = unsafe {
        libc::recv(
            stream.as_raw_fd(),
            (&mut byte as *mut u8).cast(),
            1,
            libc::MSG_PEEK | libc::MSG_DONTWAIT,
        )
    };
    // After the single request, extra input is invalid; EOF revokes the claim.
    count < 0 && io::Error::last_os_error().kind() == io::ErrorKind::WouldBlock
}

/// Small framed messages retain partial reads and revoke promptly on focus loss.
/// Only worker threads wait here; a frame can never allocate more than 4 KiB.
pub fn receive(stream: &mut UnixStream, allowed: impl Fn() -> bool) -> io::Result<Vec<u8>> {
    use std::io::Read;
    stream.set_read_timeout(Some(Duration::from_millis(16)))?;
    let end = Instant::now() + Duration::from_secs(8);
    let mut header = [0; 4];
    let mut offset = 0;
    let mut body = Vec::new();
    loop {
        if !allowed() || Instant::now() >= end {
            return Err(io::Error::other("PNG handoff cancelled or expired"));
        }
        let output = if offset < 4 {
            &mut header[offset..]
        } else {
            &mut body[offset - 4..]
        };
        match stream.read(output) {
            Ok(0) => {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "PNG handoff closed",
                ));
            }
            Ok(n) => offset += n,
            Err(e)
                if matches!(
                    e.kind(),
                    io::ErrorKind::WouldBlock
                        | io::ErrorKind::TimedOut
                        | io::ErrorKind::Interrupted
                ) =>
            {
                continue;
            }
            Err(e) => return Err(e),
        }
        if offset == 4 && body.is_empty() {
            let length = u32::from_be_bytes(header) as usize;
            if length == 0 || length > MAX_MESSAGE {
                return Err(io::Error::other("PNG handoff message size rejected"));
            }
            body.resize(length, 0);
        }
        if offset == 4 + body.len() {
            return Ok(body);
        }
    }
}

pub fn send(stream: &mut UnixStream, message: &[u8]) -> io::Result<()> {
    use std::io::Write;
    if message.is_empty() || message.len() > MAX_MESSAGE {
        return Err(io::Error::other("PNG handoff message size rejected"));
    }
    stream.set_write_timeout(Some(Duration::from_millis(16)))?;
    stream.write_all(&(message.len() as u32).to_be_bytes())?;
    stream.write_all(message)
}

/// At most four short-lived workers per frontend, including cancelled workers
/// which have not yet returned from their bounded read.
static WORKERS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
pub struct Worker;
impl Worker {
    pub fn reserve() -> io::Result<Self> {
        use std::sync::atomic::Ordering;
        WORKERS
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |count| {
                (count < 4).then_some(count + 1)
            })
            .map(|_| Self)
            .map_err(|_| io::Error::other("PNG handoff workers busy"))
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        WORKERS.fetch_sub(1, std::sync::atomic::Ordering::AcqRel);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{io::Read, process::Child, sync::mpsc};

    struct Root(PathBuf);
    impl Root {
        fn new() -> Self {
            let stamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let path = std::env::temp_dir().join(format!("rio-{}-{stamp}", std::process::id()));
            fs::DirBuilder::new().mode(0o700).create(&path).unwrap();
            Self(path)
        }
        fn socket(&self) -> PathBuf {
            self.0
                .join(format!("{}.sock", stem(Process::current().unwrap())))
        }
    }
    impl Drop for Root {
        fn drop(&mut self) {
            if private(&self.0, 0o700, false).is_ok() {
                let _ = fs::remove_dir_all(&self.0);
            }
        }
    }
    struct ChildGuard(Child);
    impl Drop for ChildGuard {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    #[test]
    fn explicit_process_claim_is_revoked_by_control_shutdown() {
        let root = Root::new();
        let offer = Offer::in_directory(&root.0, Process::current().unwrap()).unwrap();
        let (stream, cancel) = claim_in(&root.0).unwrap().unwrap();
        let mut accepted = offer.accept(|| true).unwrap().unwrap();
        assert!(consumer_live(&accepted));
        drop(cancel);
        assert_eq!(accepted.read(&mut [0]).unwrap(), 0);
        assert!(!consumer_live(&accepted));
        drop(stream);
        drop(offer);
        assert!(!root.socket().exists());
    }

    #[test]
    fn process_start_time_mismatch_creates_no_offer() {
        let root = Root::new();
        let mut process = Process::current().unwrap();
        process.start += 1;
        assert!(!process.live());
        assert!(Offer::in_directory(&root.0, process).is_err());
        assert_eq!(fs::read_dir(&root.0).unwrap().count(), 0);
    }

    #[test]
    fn another_live_process_cannot_claim_the_offer() {
        let root = Root::new();
        let child = ChildGuard(
            std::process::Command::new("/usr/bin/sleep")
                .arg("5")
                .spawn()
                .unwrap(),
        );
        let mut offer = Offer::in_directory(&root.0, Process::read(child.0.id()).unwrap()).unwrap();
        offer.until = Instant::now() + Duration::from_millis(50);
        let _unrelated = connect(&offer.path).unwrap();
        assert!(offer.accept(|| true).unwrap().is_none());
    }

    #[test]
    fn a_second_offer_does_not_remove_the_live_listener() {
        let root = Root::new();
        let process = Process::current().unwrap();
        let offer = Offer::in_directory(&root.0, process).unwrap();
        let inode = private(&offer.path, 0o600, true).unwrap().ino();
        assert!(matches!(
            Offer::in_directory(&root.0, process),
            Err(e) if e.kind() == io::ErrorKind::WouldBlock
        ));
        assert_eq!(private(&offer.path, 0o600, true).unwrap().ino(), inode);
        let _claim = claim_in(&root.0).unwrap().unwrap();
        assert!(offer.accept(|| true).unwrap().is_some());
    }

    #[test]
    fn a_dead_producer_socket_can_be_replaced_under_the_lock() {
        let root = Root::new();
        let path = root.socket();
        let old = UnixListener::bind(&path).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        drop(old);
        let offer = Offer::in_directory(&root.0, Process::current().unwrap()).unwrap();
        let _claim = claim_in(&root.0).unwrap().unwrap();
        assert!(offer.accept(|| true).unwrap().is_some());
    }

    #[test]
    fn a_symlink_or_wrong_mode_socket_is_not_removed() {
        let root = Root::new();
        let destination = root.0.join("private.sock");
        let _listener = UnixListener::bind(&destination).unwrap();
        fs::set_permissions(&destination, fs::Permissions::from_mode(0o600)).unwrap();
        std::os::unix::fs::symlink(&destination, root.socket()).unwrap();
        assert!(Offer::in_directory(&root.0, Process::current().unwrap()).is_err());
        assert!(
            root.socket()
                .symlink_metadata()
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert!(claim_in(&root.0).is_err());
        fs::remove_file(root.socket()).unwrap();
        fs::rename(&destination, root.socket()).unwrap();
        fs::set_permissions(root.socket(), fs::Permissions::from_mode(0o666)).unwrap();
        assert!(Offer::in_directory(&root.0, Process::current().unwrap()).is_err());
        assert!(claim_in(&root.0).is_err());
        assert!(root.socket().exists());
    }

    #[test]
    fn a_hardlinked_lock_is_refused_without_changing_its_content() {
        let root = Root::new();
        let original = root.0.join("original.lock");
        fs::write(&original, b"preserve").unwrap();
        fs::set_permissions(&original, fs::Permissions::from_mode(0o600)).unwrap();
        let name = format!("{}.lock", stem(Process::current().unwrap()));
        fs::hard_link(&original, root.0.join(name)).unwrap();
        assert!(Offer::in_directory(&root.0, Process::current().unwrap()).is_err());
        assert_eq!(fs::read(&original).unwrap(), b"preserve");
        assert_eq!(fs::metadata(&original).unwrap().nlink(), 2);
    }

    #[test]
    fn drop_preserves_a_replacement_socket_inode() {
        let root = Root::new();
        let offer = Offer::in_directory(&root.0, Process::current().unwrap()).unwrap();
        fs::rename(root.socket(), root.0.join("original.sock")).unwrap();
        let _replacement = UnixListener::bind(root.socket()).unwrap();
        let inode = fs::symlink_metadata(root.socket()).unwrap().ino();
        drop(offer);
        assert_eq!(fs::symlink_metadata(root.socket()).unwrap().ino(), inode);
    }

    #[test]
    fn revoked_or_expired_gui_lease_accepts_no_claim() {
        let root = Root::new();
        let mut offer = Offer::in_directory(&root.0, Process::current().unwrap()).unwrap();
        assert!(offer.accept(|| false).unwrap().is_none());
        offer.until = Instant::now();
        assert!(offer.accept(|| true).unwrap().is_none());
    }

    #[test]
    fn full_backlog_does_not_block_the_input_thread() {
        let root = Root::new();
        let offer = Offer::in_directory(&root.0, Process::current().unwrap()).unwrap();
        assert_eq!(unsafe { libc::listen(offer.listener.as_raw_fd(), 0) }, 0);
        let _first = UnixStream::connect(&offer.path).unwrap();
        let listener = offer.listener.try_clone().unwrap();
        listener.set_nonblocking(false).unwrap();
        let (release, wait) = mpsc::channel();
        let server = std::thread::spawn(move || {
            let _ = wait.recv_timeout(Duration::from_secs(2));
            let _ = listener.accept().unwrap();
        });
        let began = Instant::now();
        assert!(claim_in(&root.0).is_err());
        let elapsed = began.elapsed();
        release.send(()).unwrap();
        server.join().unwrap();
        assert!(elapsed < Duration::from_secs(1));
    }
    #[test]
    fn framed_message_retains_split_header_and_body() {
        use std::io::Write;
        let (mut producer, mut consumer) = UnixStream::pair().unwrap();
        let writer = std::thread::spawn(move || {
            for b in [0, 0, 0, 3, b'a', b'b', b'c'] {
                producer.write_all(&[b]).unwrap();
                std::thread::sleep(Duration::from_millis(20));
            }
        });
        assert_eq!(receive(&mut consumer, || true).unwrap(), b"abc");
        writer.join().unwrap();
    }

    #[test]
    fn framed_message_rejects_zero_oversize_and_partial_eof() {
        use std::io::Write;
        for data in [vec![0, 0, 0, 0], vec![0, 0, 16, 1], vec![0, 0, 0, 3, b'a']] {
            let (mut producer, mut consumer) = UnixStream::pair().unwrap();
            producer.write_all(&data).unwrap();
            drop(producer);
            assert!(receive(&mut consumer, || true).is_err());
        }
    }

    #[test]
    fn framed_message_revocation_does_not_wait_for_sender() {
        use std::sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        };
        let (mut producer, mut consumer) = UnixStream::pair().unwrap();
        use std::io::Write;
        producer.write_all(&[0, 0]).unwrap();
        let allowed = Arc::new(AtomicBool::new(true));
        let flag = allowed.clone();
        let reader =
            std::thread::spawn(move || receive(&mut consumer, || flag.load(Ordering::Acquire)));
        let began = Instant::now();
        allowed.store(false, Ordering::Release);
        assert!(reader.join().unwrap().is_err());
        assert!(began.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn worker_budget_releases_after_failure() {
        let workers: Vec<_> = (0..4).map(|_| Worker::reserve().unwrap()).collect();
        assert!(Worker::reserve().is_err());
        drop(workers);
        assert!(Worker::reserve().is_ok());
    }
}
