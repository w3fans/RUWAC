//! An implicit upload reuses the terminal's authenticated OpenSSH transport.
//! No alias/configuration lookup may create a replacement connection.
use std::{
    fs, io,
    os::fd::{AsRawFd, FromRawFd, OwnedFd},
    os::unix::fs::{DirBuilderExt, FileTypeExt, MetadataExt},
    path::{Path, PathBuf},
};

const MAX_CONTROLS: usize = 512;

fn private_dir(path: &Path) -> io::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_dir()
        || metadata.uid() != unsafe { libc::geteuid() }
        || metadata.mode() & 0o777 != 0o700
    {
        return Err(io::Error::other("Unsafe SSH transport directory"));
    }
    Ok(())
}

fn runtime() -> PathBuf {
    // A short kernel-managed path also works when the desktop's private
    // XDG_RUNTIME_DIR is too long for an OpenSSH Unix control socket.
    PathBuf::from(format!("/run/user/{}", unsafe { libc::geteuid() }))
}

fn folder() -> io::Result<PathBuf> {
    let root = runtime();
    private_dir(&root)?;
    let folder = root.join("ruwac-ssh");
    match fs::DirBuilder::new().mode(0o700).create(&folder) {
        Ok(()) => (),
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => (),
        Err(e) => return Err(e),
    }
    private_dir(&folder)?;
    Ok(folder)
}

pub fn valid(token: &str) -> bool {
    ruwac_protocol::valid_id(token, "transport")
}

fn path(token: &str) -> io::Result<PathBuf> {
    if !valid(token) {
        return Err(io::Error::other("Invalid SSH transport identity"));
    }
    let path = folder()?.join(token);
    if path.as_os_str().as_encoded_bytes().len() > 99 {
        return Err(io::Error::other("SSH transport path is too long"));
    }
    Ok(path)
}

/// Call before adding the optional SSH_ASKPASS environment wrapper. OpenSSH
/// command-line options come first so saved/user config cannot override them.
pub fn prepare(argv: &[String]) -> io::Result<Vec<String>> {
    if argv.first().map(String::as_str) != Some("ssh") {
        return Err(io::Error::other("Expected an SSH launch"));
    }
    let folder = folder()?;
    if fs::read_dir(&folder)?.take(MAX_CONTROLS).count() >= MAX_CONTROLS {
        return Err(io::Error::other("SSH transport capacity reached"));
    }
    let token = ruwac_protocol::id("transport");
    let path = path(&token)?;
    if fs::symlink_metadata(&path).is_ok() {
        return Err(io::Error::other("SSH transport identity already exists"));
    }
    let mut planned = vec![
        "ssh".into(),
        "-o".into(),
        "ControlMaster=yes".into(),
        "-o".into(),
        "ControlPersist=no".into(),
        "-o".into(),
        format!("ControlPath={}", path.display()),
    ];
    planned.extend_from_slice(&argv[1..]);
    Ok(planned)
}

/// Store only the generated identity, never resolved addresses or credentials.
pub fn identity(argv: &[String]) -> Option<String> {
    let parent = runtime().join("ruwac-ssh");
    argv.windows(2).find_map(|pair| {
        if pair[0] != "-o" {
            return None;
        }
        let path = Path::new(pair[1].strip_prefix("ControlPath=")?);
        if path.parent()? != parent {
            return None;
        }
        let token = path.file_name()?.to_str()?;
        valid(token).then(|| token.into())
    })
}

/// The only permitted network operation is opening a channel on this socket.
/// Missing/closed masters fail through /bin/false; current user/system SSH
/// config is ignored, including aliases, ProxyJump and remote commands.
pub fn client(token: &str, pid: u32) -> io::Result<Vec<String>> {
    let path = path(token)?;
    let metadata = fs::symlink_metadata(&path)?;
    if !metadata.file_type().is_socket() || metadata.uid() != unsafe { libc::geteuid() } {
        return Err(io::Error::other("Original SSH transport is unavailable"));
    }
    verify_peer(&path, pid)?;
    Ok([
        "ssh",
        "-F",
        "/dev/null",
        "-S",
        path.to_str()
            .ok_or_else(|| io::Error::other("Invalid SSH socket path"))?,
        "-o",
        "ControlMaster=no",
        "-o",
        "ProxyCommand=/bin/false",
        "-o",
        "BatchMode=yes",
        "-o",
        "ForwardAgent=no",
        "-o",
        "ClearAllForwardings=yes",
        "-T",
        "--",
        "ruwac-bound-transport",
    ]
    .into_iter()
    .map(String::from)
    .collect())
}

/// Nonblocking local connect avoids an unbounded wait on a full accept queue.
/// The kernel identifies the SSH process owning the listener, independently of
/// the pathname. Inspecting the peer never sends an SSH command or image data.
fn verify_peer(path: &Path, expected_pid: u32) -> io::Result<()> {
    if expected_pid == 0 {
        return Err(io::Error::other("Original SSH process is unavailable"));
    }
    let raw = unsafe {
        libc::socket(
            libc::AF_UNIX,
            libc::SOCK_STREAM | libc::SOCK_CLOEXEC | libc::SOCK_NONBLOCK,
            0,
        )
    };
    if raw < 0 {
        return Err(io::Error::last_os_error());
    }
    let fd = unsafe { OwnedFd::from_raw_fd(raw) };
    let mut address: libc::sockaddr_un = unsafe { std::mem::zeroed() };
    address.sun_family = libc::AF_UNIX as libc::sa_family_t;
    let bytes = path.as_os_str().as_encoded_bytes();
    if bytes.len() >= address.sun_path.len() {
        return Err(io::Error::other("SSH transport path is too long"));
    }
    for (dst, src) in address.sun_path.iter_mut().zip(bytes) {
        *dst = *src as libc::c_char;
    }
    let length =
        (std::mem::offset_of!(libc::sockaddr_un, sun_path) + bytes.len() + 1) as libc::socklen_t;
    if unsafe {
        libc::connect(
            fd.as_raw_fd(),
            (&address as *const libc::sockaddr_un).cast(),
            length,
        )
    } != 0
    {
        return Err(io::Error::last_os_error());
    }
    let mut peer: libc::ucred = unsafe { std::mem::zeroed() };
    let mut length = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    if unsafe {
        libc::getsockopt(
            fd.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            (&mut peer as *mut libc::ucred).cast(),
            &mut length,
        )
    } != 0
    {
        return Err(io::Error::last_os_error());
    }
    if length as usize != std::mem::size_of::<libc::ucred>()
        || peer.uid != unsafe { libc::geteuid() }
        || u32::try_from(peer.pid).ok() != Some(expected_pid)
    {
        return Err(io::Error::other("Original SSH transport process changed"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_generated_transport_identities_are_accepted() {
        assert!(valid(&ruwac_protocol::id("transport")));
        for token in ["", "../socket", "/tmp/socket", "transport_x", "pane_123"] {
            assert!(!valid(token));
        }
    }
    #[test]
    fn external_control_paths_are_not_catalog_identities() {
        assert_eq!(
            identity(&["ssh".into(), "-o".into(), "ControlPath=/tmp/socket".into()]),
            None
        );
    }
}

/// A forwarding request is scoped to the exact existing master. No alias or
/// config lookup and no new network connection is permitted, even on failure.
struct ForwardLease {
    token: String,
    pid: u32,
    specification: String,
    pub local_port: u16,
}
fn forwarding_argv(
    token: &str,
    pid: u32,
    operation: &str,
    specification: &str,
) -> io::Result<Vec<String>> {
    // Independently verifies the private socket's actual kernel peer PID.
    let _ = client(token, pid)?;
    Ok(vec![
        "ssh".into(),
        "-F".into(),
        "/dev/null".into(),
        "-S".into(),
        path(token)?.to_string_lossy().into(),
        "-o".into(),
        "ControlMaster=no".into(),
        "-o".into(),
        "ProxyCommand=/bin/false".into(),
        "-o".into(),
        "BatchMode=yes".into(),
        "-o".into(),
        "ForwardAgent=no".into(),
        "-o".into(),
        "ExitOnForwardFailure=yes".into(),
        "-O".into(),
        operation.into(),
        "-L".into(),
        specification.into(),
        "--".into(),
        "ruwac-bound-transport".into(),
    ])
}
fn bounded_forward(argv: Vec<String>) -> io::Result<()> {
    use std::{
        process::{Command, Stdio},
        time::{Duration, Instant},
    };
    let mut child = Command::new(&argv[0])
        .args(&argv[1..])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if let Some(status) = child.try_wait()? {
            return if status.success() {
                Ok(())
            } else {
                Err(io::Error::other(
                    "Original SSH master refused the loopback forward",
                ))
            };
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(io::Error::other("SSH forwarding request timed out"));
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}
impl ForwardLease {
    pub fn open(token: &str, pid: u32, host: &str, remote_port: u16) -> io::Result<Self> {
        if !matches!(host, "127.0.0.1" | "::1") || remote_port == 0 {
            return Err(io::Error::other(
                "Only an explicit remote loopback TCP port is permitted",
            ));
        }
        let remote_host = if host == "::1" { "[::1]" } else { host };
        for _ in 0..3 {
            let listener = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))?;
            let local_port = listener.local_addr()?.port();
            drop(listener);
            let specification = format!("127.0.0.1:{local_port}:{remote_host}:{remote_port}");
            let argv = forwarding_argv(token, pid, "forward", &specification)?;
            let forward = Self {
                token: token.into(),
                pid,
                specification,
                local_port,
            };
            if bounded_forward(argv).is_ok() {
                return Ok(forward);
            }
            drop(forward);
        }
        Err(io::Error::other(
            "Could not reserve a private loopback forward on the original SSH transport",
        ))
    }
    pub fn valid(&self) -> bool {
        client(&self.token, self.pid).is_ok()
    }
}
impl Drop for ForwardLease {
    fn drop(&mut self) {
        let (token, pid, specification) =
            (self.token.clone(), self.pid, self.specification.clone());
        // A dead master has already removed its forwards; never replace it.
        if let Ok(argv) = forwarding_argv(&token, pid, "cancel", &specification) {
            let _ = bounded_forward(argv);
        }
    }
}

/// A small helper owns the mux forward and watches a parent-only pipe. Even
/// abrupt GUI termination closes that pipe and cancels the exact forward;
/// cancelling a preview never terminates its persistent SSH master/PTY.
pub struct LocalForward {
    token: String,
    pid: u32,
    pub local_port: u16,
    worker: std::cell::RefCell<Option<std::process::Child>>,
    specification: String,
}
fn pipe_line<R: std::io::Read + AsRawFd>(reader: &mut R, seconds: u64) -> io::Result<Vec<u8>> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(seconds);
    let mut bytes = Vec::new();
    loop {
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() || bytes.len() >= 4096 {
            return Err(io::Error::other("Forwarding helper response limit"));
        }
        let mut poll = libc::pollfd {
            fd: reader.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        let ready = unsafe {
            libc::poll(
                &mut poll,
                1,
                remaining.as_millis().min(i32::MAX as u128) as i32,
            )
        };
        if ready < 0 {
            let e = io::Error::last_os_error();
            if e.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(e);
        }
        if ready == 0 {
            return Err(io::Error::other("Forwarding helper timed out"));
        }
        let mut byte = [0u8];
        if reader.read(&mut byte)? == 0 {
            return Err(io::Error::other("Forwarding helper stopped"));
        }
        if byte[0] == b'\n' {
            return Ok(bytes);
        }
        bytes.push(byte[0]);
    }
}
fn reap_forward_worker(mut worker: std::process::Child) {
    worker.stdin.take();
    std::thread::spawn(move || {
        let _ = worker.wait();
    });
}
impl LocalForward {
    pub fn open(token: &str, pid: u32, host: &str, port: u16) -> io::Result<Self> {
        use std::io::Write;
        let _ = client(token, pid)?;
        if !matches!(host, "127.0.0.1" | "::1") || port == 0 {
            return Err(io::Error::other("Explicit remote loopback port required"));
        }
        let mut worker = std::process::Command::new(std::env::current_exe()?)
            .env("RUWAC_WEB_FORWARD_WORKER", "1")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()?;
        let response = (|| -> io::Result<u16> {
            let packet = serde_json::json!({"token":token,"pid":pid,"host":host,"port":port})
                .to_string()
                + "\n";
            worker
                .stdin
                .as_mut()
                .ok_or_else(|| io::Error::other("Forwarding pipe unavailable"))?
                .write_all(packet.as_bytes())?;
            let stdout = worker
                .stdout
                .as_mut()
                .ok_or_else(|| io::Error::other("Forwarding result unavailable"))?;
            let bytes = pipe_line(stdout, 30)?;
            let response: serde_json::Value = serde_json::from_slice(&bytes)?;
            if response["ok"] != true {
                return Err(io::Error::other("Original SSH master refused forwarding"));
            }
            response["port"]
                .as_u64()
                .and_then(|p| u16::try_from(p).ok())
                .filter(|p| *p > 0)
                .ok_or_else(|| io::Error::other("Invalid forwarding port"))
        })();
        match response {
            Ok(local_port) => Ok(Self {
                token: token.into(),
                pid,
                local_port,
                worker: std::cell::RefCell::new(Some(worker)),
                specification: format!(
                    "127.0.0.1:{local_port}:{}:{port}",
                    if host == "::1" { "[::1]" } else { host }
                ),
            }),
            Err(e) => {
                reap_forward_worker(worker);
                Err(e)
            }
        }
    }
    pub fn valid(&self) -> bool {
        client(&self.token, self.pid).is_ok()
            && self
                .worker
                .borrow_mut()
                .as_mut()
                .is_some_and(|w| matches!(w.try_wait(), Ok(None)))
    }
}
impl Drop for LocalForward {
    fn drop(&mut self) {
        if let Some(mut worker) = self.worker.get_mut().take() {
            let stopped = !matches!(worker.try_wait(), Ok(None));
            reap_forward_worker(worker);
            if stopped {
                let (token, pid, specification) =
                    (self.token.clone(), self.pid, self.specification.clone());
                std::thread::spawn(move || {
                    if let Ok(argv) = forwarding_argv(&token, pid, "cancel", &specification) {
                        let _ = bounded_forward(argv);
                    }
                });
            }
        }
    }
}
/// Runs before GTK/askpass startup, only in the explicitly spawned helper.
pub fn run_forward_worker_if_requested() -> Option<i32> {
    if std::env::var_os("RUWAC_WEB_FORWARD_WORKER").as_deref() != Some(std::ffi::OsStr::new("1")) {
        return None;
    }
    let result = (|| -> io::Result<()> {
        use std::io::{Read, Write};
        // Poll the same unbuffered descriptor we read. StdinLock can read ahead,
        // leaving bytes in its buffer while poll waits on an empty pipe.
        let fd = unsafe { libc::fcntl(libc::STDIN_FILENO, libc::F_DUPFD_CLOEXEC, 3) };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        let mut input = unsafe { fs::File::from_raw_fd(fd) };
        let packet = pipe_line(&mut input, 3)?;
        let q: serde_json::Value = serde_json::from_slice(&packet)?;
        if q.as_object().is_none_or(|q| q.len() != 4) {
            return Err(io::Error::other("Invalid forwarding request"));
        }
        let token = q["token"]
            .as_str()
            .ok_or_else(|| io::Error::other("Transport identity required"))?;
        let pid = q["pid"]
            .as_u64()
            .and_then(|p| u32::try_from(p).ok())
            .ok_or_else(|| io::Error::other("SSH process required"))?;
        let host = q["host"]
            .as_str()
            .ok_or_else(|| io::Error::other("Loopback host required"))?;
        let port = q["port"]
            .as_u64()
            .and_then(|p| u16::try_from(p).ok())
            .ok_or_else(|| io::Error::other("Remote port required"))?;
        let lease = ForwardLease::open(token, pid, host, port)?;
        let response = serde_json::json!({"ok":true,"port":lease.local_port}).to_string() + "\n";
        io::stdout().write_all(response.as_bytes())?;
        io::stdout().flush()?;
        let mut cancel = [0];
        let _ = input.read(&mut cancel);
        drop(lease);
        Ok(())
    })();
    Some(if result.is_ok() { 0 } else { 1 })
}
