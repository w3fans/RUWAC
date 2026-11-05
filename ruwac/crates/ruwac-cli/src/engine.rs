//! Explicit engine discovery and coexistence. Never replace a live listener.
use ruwac_protocol::{read_frame, request, write_frame};
use serde_json::{Value, json};
use std::{
    fs, io,
    os::unix::fs::{DirBuilderExt, MetadataExt},
    path::{Path, PathBuf},
    time::Duration,
};

fn query(socket: &Path, method: &str) -> io::Result<Value> {
    let mut stream = super::startup::connect(socket)?;
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    stream.set_write_timeout(Some(Duration::from_secs(2)))?;
    let req = request(method, json!({}));
    write_frame(&mut stream, &req)?;
    let reply = read_frame(&mut stream)?;
    if reply["version"] != 0 || reply["id"] != req.id || reply.get("error").is_some() {
        return Err(io::Error::other("engine discovery response rejected"));
    }
    Ok(reply["result"].clone())
}

pub fn status(socket: &Path) -> io::Result<Value> {
    let installed = env!("CARGO_PKG_VERSION");
    let version = match query(socket, "version") {
        Ok(v) => v,
        Err(e)
            if matches!(
                e.kind(),
                io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused
            ) =>
        {
            return Ok(json!({"installed_cli":installed,"socket":socket,"running":false}));
        }
        Err(e) => return Err(e),
    };
    let package = version["package"]
        .as_str()
        .filter(|p| !p.is_empty())
        .ok_or_else(|| io::Error::other("engine version missing"))?;
    if version["name"] != "ruwac" || version["protocol"] != 0 {
        return Err(io::Error::other("engine identity or protocol rejected"));
    }
    Ok(
        json!({"installed_cli":installed,"socket":socket,"running":true,
        "running_engine":package,"matches_installed":package==installed,
        "capabilities":query(socket,"capabilities")?,
        "activation":"Existing terminals stay on this engine. Use engine new for a separate updated engine; no sessions are migrated."}),
    )
}

fn private_directory(path: &Path) -> io::Result<()> {
    match fs::DirBuilder::new().mode(0o700).create(path) {
        Ok(()) => (),
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => (),
        Err(e) => return Err(e),
    }
    let m = fs::symlink_metadata(path)?;
    if !m.is_dir() || m.uid() != unsafe { libc::geteuid() } || m.mode() & 0o777 != 0o700 {
        return Err(io::Error::other(
            "engine directory must be owned, nonsymlink and 0700",
        ));
    }
    Ok(())
}

/// Caller selects the current endpoint, but cannot silently reuse a custom
/// checkpoint. The sibling socket and durable history have versioned identities.
pub fn start_separate(current: &Path) -> io::Result<Value> {
    if std::env::var_os("RUWAC_STATE_FILE").is_some() {
        return Err(io::Error::other(
            "engine new refuses RUWAC_STATE_FILE: unset it to create independent history",
        ));
    }
    let parent = current
        .parent()
        .filter(|p| p.is_absolute())
        .ok_or_else(|| {
            io::Error::other("engine new requires an absolute socket with a private parent")
        })?;
    private_directory(parent)?;
    let version = env!("CARGO_PKG_VERSION");
    let socket = parent.join(format!("engine-{version}.sock"));
    if socket == current {
        return Err(io::Error::other(
            "already addressing the separate engine; use engine status",
        ));
    }
    // Inspect an existing listener before touching checkpoint directories.
    let existing = status(&socket)?;
    if existing["running"] == true && existing["matches_installed"] != true {
        return Err(io::Error::other(
            "separate socket belongs to another engine version; listener preserved",
        ));
    }
    let state_home = std::env::var_os("XDG_STATE_HOME")
        .filter(|p| !p.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME")
                .filter(|p| !p.is_empty())
                .map(|p| PathBuf::from(p).join(".local/state"))
        })
        .ok_or_else(|| {
            io::Error::other("HOME or XDG_STATE_HOME required for durable engine history")
        })?;
    if !state_home.is_absolute() {
        return Err(io::Error::other("state home must be absolute"));
    }
    // Follow the existing default daemon's state-home convention; validate each
    // RUWAC-owned child before descending into it.
    fs::create_dir_all(&state_home)?;
    let directory = state_home.join("RUWAC");
    private_directory(&directory)?;
    let engines = directory.join("engines");
    private_directory(&engines)?;
    // Include the socket identity so two explicit private endpoints cannot share
    // a recovery writer. Encode bytes without lossy UTF-8 conversion.
    let identity: String = socket
        .as_os_str()
        .as_encoded_bytes()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    if identity.len() > 200 {
        return Err(io::Error::other(
            "engine socket path exceeds supported length",
        ));
    }
    let history = engines.join(format!("{identity}.json"));
    let started = super::startup::ensure_with_state(&socket, Some(&history))?;
    let report = status(&socket)?;
    if report["running"] != true || report["matches_installed"] != true {
        return Err(io::Error::other(
            "separate engine version verification failed; existing listener preserved",
        ));
    }
    Ok(
        json!({"engine":report,"start_requested":started,"previous_socket":current,
        "history":history,"sessions_migrated":false,
        "gui_argv":["env",format!("RUWAC_STATE_FILE={}",history.display()),format!("RUWAC_SOCKET={}",socket.display()),"ruwac","--socket",socket,"gui","--new-instance"],
        "terminal_argv":["env",format!("RUWAC_STATE_FILE={}",history.display()),format!("RUWAC_SOCKET={}",socket.display()),"ruwac","--socket",socket,"-S","new"]}),
    )
}
