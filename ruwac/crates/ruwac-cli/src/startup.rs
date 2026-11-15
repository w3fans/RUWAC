//! Explicit on-demand startup. Automation commands never create a daemon implicitly.
use ruwac_protocol::{read_frame, request, write_frame};
use std::{
    fs, io,
    os::unix::{
        fs::{DirBuilderExt, FileTypeExt, MetadataExt},
        io::AsRawFd,
        net::UnixStream,
        process::CommandExt,
    },
    path::Path,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

pub fn connect(path: &Path) -> io::Result<UnixStream> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other("socket parent missing"))?;
    let m = fs::symlink_metadata(parent)?;
    if !m.is_dir() || m.uid() != unsafe { libc::geteuid() } || m.mode() & 0o777 != 0o700 {
        return Err(io::Error::other(
            "socket parent must be owned, nonsymlink and 0700",
        ));
    }
    let m = fs::symlink_metadata(path)?;
    if !m.file_type().is_socket()
        || m.uid() != unsafe { libc::geteuid() }
        || m.mode() & 0o777 != 0o600
    {
        return Err(io::Error::other("refusing unsafe daemon socket"));
    }
    let stream = UnixStream::connect(path)?;
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
        || cred.uid != unsafe { libc::geteuid() }
    {
        return Err(io::Error::other("daemon peer identity rejected"));
    }
    Ok(stream)
}
fn probe(path: &Path) -> io::Result<bool> {
    let mut stream = match connect(path) {
        Ok(s) => s,
        Err(e)
            if matches!(
                e.kind(),
                io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused
            ) =>
        {
            return Ok(false);
        }
        Err(e) => return Err(e),
    };
    stream.set_read_timeout(Some(Duration::from_millis(500)))?;
    stream.set_write_timeout(Some(Duration::from_millis(500)))?;
    let req = request("health", serde_json::json!({}));
    write_frame(&mut stream, &req)?;
    let response = read_frame(&mut stream)?;
    if response["version"] != 0 || response["id"] != req.id || response["result"]["ok"] != true {
        return Err(io::Error::other(
            "existing listener is not a compatible RUWAC daemon",
        ));
    }
    Ok(true)
}

pub fn ensure(path: &Path) -> io::Result<bool> {
    ensure_with_state(path, None)
}

/// An explicit side-by-side engine may have its own durable checkpoint. The
/// override belongs only to the spawned child, never to this frontend's process.
pub fn ensure_with_state(path: &Path, state: Option<&Path>) -> io::Result<bool> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .ok_or_else(|| io::Error::other("socket needs an explicit private parent"))?;
    match fs::DirBuilder::new().mode(0o700).create(parent) {
        Ok(()) => (),
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => (),
        Err(e) => return Err(e),
    }
    let m = fs::symlink_metadata(parent)?;
    if !m.is_dir() || m.uid() != unsafe { libc::geteuid() } || m.mode() & 0o777 != 0o700 {
        return Err(io::Error::other(
            "daemon directory must be owned, nonsymlink and 0700",
        ));
    }
    if probe(path)? {
        return Ok(false);
    }
    let mut command = Command::new(std::env::current_exe()?);
    command
        .arg("--socket")
        .arg(path)
        .arg("serve")
        .env("RUWAC_SOCKET", path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if let Some(state) = state {
        command.env("RUWAC_STATE_FILE", state);
    }
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() < 0 {
                Err(io::Error::last_os_error())
            } else {
                Ok(())
            }
        });
    }
    let mut child = command.spawn()?;
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        if probe(path)? {
            return Ok(true);
        }
        if Instant::now() >= deadline {
            return Err(io::Error::other(
                "daemon did not become ready within eight seconds; inspect socket permissions and start lock",
            ));
        }
        // Another concurrent launcher may have won the daemon's start lock.
        let _ = child.try_wait()?;
        std::thread::sleep(Duration::from_millis(20));
    }
}
