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

// ---------------------------------------------------------------------------
// Engine inventory, new-work routing and safe retirement.
//
// Everything here is read-only except `route_for_new_work` (which may start
// the installed engine or retire an *idle* engine through the atomic
// `daemon.stopIfIdle`) and `stop` (explicit user request). No engine with a
// live PTY is ever stopped automatically, and engines older than the safe-stop
// capability are never stopped automatically at all.

const ROUTE_FILE: &str = "route.json";

fn runtime_directory(default: &Path) -> io::Result<&Path> {
    let parent = default
        .parent()
        .filter(|p| p.is_absolute())
        .ok_or_else(|| io::Error::other("engine socket needs an absolute private parent"))?;
    let m = fs::symlink_metadata(parent)?;
    if !m.is_dir() || m.uid() != unsafe { libc::geteuid() } || m.mode() & 0o777 != 0o700 {
        return Err(io::Error::other(
            "engine directory must be owned, nonsymlink and 0700",
        ));
    }
    Ok(parent)
}

fn call(socket: &Path, method: &str, params: Value) -> io::Result<Value> {
    let mut stream = super::startup::connect(socket)?;
    stream.set_read_timeout(Some(Duration::from_secs(3)))?;
    stream.set_write_timeout(Some(Duration::from_secs(3)))?;
    let req = request(method, params);
    write_frame(&mut stream, &req)?;
    let reply = read_frame(&mut stream)?;
    if reply["version"] != 0 || reply["id"] != req.id {
        return Err(io::Error::other("engine response rejected"));
    }
    if let Some(error) = reply.get("error") {
        return Err(io::Error::other(format!(
            "{}: {}",
            error["code"].as_str().unwrap_or("ERROR"),
            error["message"].as_str().unwrap_or("request failed")
        )));
    }
    Ok(reply["result"].clone())
}

fn peer_pid(socket: &Path) -> Option<u32> {
    use std::os::unix::io::AsRawFd;
    let stream = super::startup::connect(socket).ok()?;
    let mut cred: libc::ucred = unsafe { std::mem::zeroed() };
    let mut size = std::mem::size_of_val(&cred) as libc::socklen_t;
    let ok = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            &mut cred as *mut _ as *mut _,
            &mut size,
        )
    } == 0;
    (ok && cred.pid > 0).then_some(cred.pid as u32)
}

fn executable(pid: u32) -> (Option<String>, bool) {
    match fs::read_link(format!("/proc/{pid}/exe")) {
        Ok(path) => {
            let text = path.to_string_lossy().into_owned();
            let replaced = text.ends_with(" (deleted)");
            (
                Some(text.trim_end_matches(" (deleted)").to_owned()),
                replaced,
            )
        }
        Err(_) => (None, false),
    }
}

/// One engine's read-only description. Never creates, attaches or stops.
pub fn describe_engine(socket: &Path) -> Value {
    let version = match call(socket, "version", json!({})) {
        Ok(v) => v,
        Err(e)
            if matches!(
                e.kind(),
                io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused
            ) =>
        {
            return json!({"socket":socket,"running":false});
        }
        Err(e) => return json!({"socket":socket,"running":false,"error":e.to_string()}),
    };
    if version["name"] != "ruwac" || version["protocol"] != 0 {
        return json!({"socket":socket,"running":false,"error":"not a compatible RUWAC engine"});
    }
    let health = call(socket, "health", json!({})).unwrap_or(Value::Null);
    let capabilities = call(socket, "capabilities", json!({})).unwrap_or(Value::Null);
    let sessions = call(socket, "session.list", json!({}))
        .ok()
        .and_then(|v| v.as_array().cloned())
        .unwrap_or_default();
    // Older engines lack live/ended counts in health; derive them read-only.
    let (mut live, mut ended) = (0u64, 0u64);
    for session in &sessions {
        if let Ok(panes) = call(socket, "pane.list", json!({"session":session["id"]})) {
            for pane in panes.as_array().into_iter().flatten() {
                if pane["ended"] == true || pane["restored"] == true {
                    ended += 1;
                } else {
                    live += 1;
                }
            }
        }
    }
    let pid = health["pid"]
        .as_u64()
        .map(|p| p as u32)
        .or_else(|| peer_pid(socket));
    let (binary, replaced) = pid.map(executable).unwrap_or((None, false));
    json!({
        "socket":socket,"running":true,"version":version["package"],"pid":pid,
        "binary":binary,"binary_replaced_on_disk":replaced,
        "sessions":sessions.len(),"live_terminals":live,"ended_terminals":ended,
        "attached_clients":health["clients"],
        "safe_stop":capabilities["safe_stop"] == "daemon.stopIfIdle",
        "checkpoint_error":health["persistence"]["error"],
    })
}

fn engine_sockets(directory: &Path) -> Vec<std::path::PathBuf> {
    use std::os::unix::fs::FileTypeExt;
    let mut sockets = fs::read_dir(directory)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
            // GUI API sockets (*.gui.sock) live beside engines and must never
            // receive engine requests.
            (name == "control.sock"
                || (name.starts_with("engine-")
                    && name.ends_with(".sock")
                    && !name.ends_with(".gui.sock")))
                && fs::symlink_metadata(p).is_ok_and(|m| {
                    m.file_type().is_socket()
                        && m.uid() == unsafe { libc::geteuid() }
                        && m.mode() & 0o777 == 0o600
                })
        })
        .collect::<Vec<_>>();
    sockets.sort();
    sockets
}

fn read_route(directory: &Path) -> Option<std::path::PathBuf> {
    let path = directory.join(ROUTE_FILE);
    let m = fs::symlink_metadata(&path).ok()?;
    if !m.is_file()
        || m.uid() != unsafe { libc::geteuid() }
        || m.mode() & 0o777 != 0o600
        || m.len() > 4096
    {
        return None;
    }
    let value: Value = serde_json::from_slice(&fs::read(&path).ok()?).ok()?;
    let socket = std::path::PathBuf::from(value["socket"].as_str()?);
    (value["version"] == 1 && socket.parent() == Some(directory)).then_some(socket)
}

fn write_route(directory: &Path, socket: &Path, engine: &str) -> io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let temporary = directory.join(format!(".route-{}.tmp", std::process::id()));
    let _ = fs::remove_file(&temporary);
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(&temporary)?;
    file.write_all(
        json!({"version":1,"socket":socket,"engine":engine})
            .to_string()
            .as_bytes(),
    )?;
    file.sync_all()?;
    fs::rename(&temporary, directory.join(ROUTE_FILE))
}

fn running_version(socket: &Path) -> Option<String> {
    call(socket, "version", json!({}))
        .ok()
        .filter(|v| v["name"] == "ruwac" && v["protocol"] == 0)
        .and_then(|v| v["package"].as_str().map(str::to_owned))
}

fn newer(a: &str, b: &str) -> bool {
    let parse = |v: &str| -> Vec<u64> { v.split('.').filter_map(|n| n.parse().ok()).collect() };
    parse(a) > parse(b)
}

/// Where existing work is looked up: the recorded route if its engine still
/// runs, otherwise the default socket. Read-only.
pub fn current_route(default: &Path) -> std::path::PathBuf {
    let Ok(directory) = runtime_directory(default) else {
        return default.to_path_buf();
    };
    read_route(directory)
        .filter(|s| running_version(s).is_some())
        .unwrap_or_else(|| default.to_path_buf())
}

/// Engine for new sessions: the installed version, never by stopping work.
pub fn route_for_new_work(default: &Path) -> io::Result<std::path::PathBuf> {
    let own = env!("CARGO_PKG_VERSION");
    let Ok(directory) = runtime_directory(default) else {
        // First run: the default directory does not exist yet.
        return Ok(default.to_path_buf());
    };
    let recorded = read_route(directory);
    for candidate in recorded.iter().map(|p| p.as_path()).chain([default]) {
        if running_version(candidate).as_deref() == Some(own) {
            write_route(directory, candidate, own)?;
            return Ok(candidate.to_path_buf());
        }
    }
    let Some(default_version) = running_version(default) else {
        write_route(directory, default, own)?;
        return Ok(default.to_path_buf());
    };
    if newer(&default_version, own) {
        // A newer engine already serves the default: never downgrade new work.
        return Ok(default.to_path_buf());
    }
    // An idle, safe-stop-capable older default engine is retired atomically;
    // the replacement on the same socket keeps its ended history readable.
    if describe_engine(default)["safe_stop"] == true
        && call(default, "daemon.stopIfIdle", json!({})).is_ok()
    {
        wait_until_stopped(default);
        // Never leave the default without an engine after retiring one.
        super::startup::ensure(default)?;
        write_route(directory, default, own)?;
        return Ok(default.to_path_buf());
    }
    // Busy or legacy: preserve it untouched and start the installed engine beside it.
    let separate = start_separate(default)?;
    let socket = std::path::PathBuf::from(
        separate["engine"]["socket"]
            .as_str()
            .ok_or_else(|| io::Error::other("separate engine socket missing"))?,
    );
    write_route(directory, &socket, own)?;
    Ok(socket)
}

fn wait_until_stopped(socket: &Path) {
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while std::time::Instant::now() < deadline && running_version(socket).is_some() {
        std::thread::sleep(Duration::from_millis(25));
    }
}

/// Engines (in lookup order) that may hold an existing named session.
pub fn lookup_order(default: &Path) -> Vec<std::path::PathBuf> {
    let route = current_route(default);
    let mut order = vec![route.clone()];
    if let Ok(directory) = runtime_directory(default) {
        for socket in std::iter::once(default.to_path_buf()).chain(engine_sockets(directory)) {
            if !order.contains(&socket) {
                order.push(socket);
            }
        }
    }
    order
}

fn installed_rpm() -> Value {
    std::process::Command::new("rpm")
        .args(["-q", "--qf", "%{VERSION}-%{RELEASE}", "ruwac"])
        .stderr(std::process::Stdio::null())
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map_or(Value::Null, Value::from)
}

fn path_cli() -> Value {
    std::env::var_os("PATH")
        .and_then(|path| {
            std::env::split_paths(&path)
                .map(|d| d.join("ruwac"))
                .find(|c| c.is_file())
        })
        .map_or(Value::Null, |p| json!(p))
}

fn running_guis() -> Vec<Value> {
    let uid = unsafe { libc::geteuid() };
    let mut out = Vec::new();
    for entry in fs::read_dir("/proc").into_iter().flatten().flatten() {
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|n| n.parse::<u32>().ok())
        else {
            continue;
        };
        if fs::metadata(entry.path()).map(|m| m.uid()).ok() != Some(uid) {
            continue;
        }
        let (binary, replaced) = executable(pid);
        if !binary
            .as_deref()
            .is_some_and(|b| b.ends_with("/ruwac-desktop"))
        {
            continue;
        }
        let argv = fs::read(entry.path().join("cmdline")).unwrap_or_default();
        let args: Vec<String> = argv
            .split(|b| *b == 0)
            .map(|a| String::from_utf8_lossy(a).into_owned())
            .collect();
        let socket = args
            .iter()
            .position(|a| a == "--socket")
            .and_then(|i| args.get(i + 1).cloned());
        out.push(
            json!({"pid":pid,"socket":socket,"binary":binary,"binary_replaced_on_disk":replaced}),
        );
    }
    out
}

/// Complete, read-only machine view for `ruwac engine status`.
pub fn inventory(default: &Path) -> io::Result<Value> {
    let own = env!("CARGO_PKG_VERSION");
    let directory = runtime_directory(default).ok();
    let engines = directory
        .map(engine_sockets)
        .unwrap_or_default()
        .iter()
        .map(|s| describe_engine(s))
        .filter(|e| e["running"] == true)
        .collect::<Vec<_>>();
    let route = current_route(default);
    let route_version = engines
        .iter()
        .find(|e| e["socket"] == json!(route))
        .and_then(|e| e["version"].as_str().map(str::to_owned));
    let preserved = engines
        .iter()
        .filter(|e| e["version"] != json!(own) && e["socket"] != json!(route))
        .count();
    let summary = match (&route_version, preserved) {
        (Some(v), 0) if v == own => "All engines run the installed version.".to_owned(),
        (Some(v), n) if v == own => format!(
            "New sessions use the installed engine {own}. {n} older engine(s) are preserved with their terminals; they are never stopped while programs run."
        ),
        (Some(v), _) => format!(
            "New sessions currently use engine {v}; the next new session starts the installed engine {own} without stopping existing terminals."
        ),
        (None, _) => {
            format!("No engine is running; the next session starts the installed engine {own}.")
        }
    };
    Ok(json!({
        "invoking_cli":{"version":own,"path":std::env::current_exe().ok()},
        "path_cli":path_cli(),
        "installed_rpm":installed_rpm(),
        "default_socket":default,
        "route_for_new_sessions":route,
        "route_engine":route_version,
        "engines":engines,
        "guis":running_guis(),
        "summary":summary,
    }))
}

/// Explicit stop of one engine. Safe engines refuse while anything runs.
/// Legacy engines (before safe stop) require the caller to name the engine PID
/// and are re-checked for live terminals immediately before stopping.
pub fn stop(socket: &Path, legacy_confirm_pid: Option<u32>) -> io::Result<Value> {
    let engine = describe_engine(socket);
    if engine["running"] != true {
        return Ok(json!({"socket":socket,"running":false,"stopped":false}));
    }
    if engine["safe_stop"] == true {
        let result = call(socket, "daemon.stopIfIdle", json!({}))?;
        wait_until_stopped(socket);
        return Ok(json!({"socket":socket,"engine":engine,"result":result}));
    }
    let pid = engine["pid"].as_u64().map(|p| p as u32);
    if legacy_confirm_pid.is_none() || legacy_confirm_pid != pid {
        return Err(io::Error::other(format!(
            "engine {} predates safe stop. If it runs no programs you may stop it explicitly with: ruwac --socket {} engine stop --legacy-confirm {}",
            engine["version"].as_str().unwrap_or("unknown"),
            socket.display(),
            pid.map_or("PID".into(), |p| p.to_string())
        )));
    }
    let fresh = describe_engine(socket);
    if fresh["live_terminals"] != 0 || fresh["attached_clients"].as_u64().unwrap_or(1) != 0 {
        return Err(io::Error::other(format!(
            "engine not stopped: {} live terminal(s), {} attached client(s)",
            fresh["live_terminals"], fresh["attached_clients"]
        )));
    }
    let result = call(socket, "daemon.stop", json!({}))?;
    wait_until_stopped(socket);
    Ok(json!({"socket":socket,"engine":fresh,"result":result,"legacy":true}))
}

/// Concise terminal rendering of `inventory` for `ruwac engine status`.
pub fn human(machine: &Value) -> String {
    let text = |v: &Value| v.as_str().map_or("—".to_owned(), str::to_owned);
    let mut out = String::from("RUWAC on this machine\n");
    out.push_str(&format!(
        "  Installed RPM     {}\n",
        text(&machine["installed_rpm"])
    ));
    out.push_str(&format!(
        "  This CLI          {}  {}\n",
        text(&machine["invoking_cli"]["version"]),
        text(&machine["invoking_cli"]["path"])
    ));
    for engine in machine["engines"].as_array().into_iter().flatten() {
        let role = if engine["socket"] == machine["route_for_new_sessions"] {
            "new sessions"
        } else {
            "preserved"
        };
        out.push_str(&format!(
            "  Engine {:<10} {:<12} {} live · {} ended · {} client(s) · pid {}{}\n",
            text(&engine["version"]),
            role,
            engine["live_terminals"],
            engine["ended_terminals"],
            engine["attached_clients"].as_u64().unwrap_or(0),
            engine["pid"],
            if engine["binary_replaced_on_disk"] == true {
                " · binary updated on disk"
            } else {
                ""
            }
        ));
    }
    for gui in machine["guis"].as_array().into_iter().flatten() {
        out.push_str(&format!(
            "  GUI pid {:<9} {}{}\n",
            gui["pid"],
            text(&gui["socket"]),
            if gui["binary_replaced_on_disk"] == true {
                " · reopen to load the update"
            } else {
                ""
            }
        ));
    }
    out.push_str(&format!("  {}", text(&machine["summary"])));
    out
}

/// Package-update status (fleet timer) plus whether engines still run an older
/// version than the installed package. Read-only; works without a GUI.
pub fn update_status(default: &Path) -> Value {
    let path = std::env::var_os("RUWAC_PKGUPDATE_STATUS")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| "/var/lib/ruwac/package-update.json".into());
    let last = fs::read(&path)
        .ok()
        .filter(|b| b.len() < 16 * 1024)
        .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
        .unwrap_or(Value::Null);
    let timer = std::process::Command::new("systemctl")
        .args([
            "show",
            "--property=UnitFileState,ActiveState,NextElapseUSecRealtime,LastTriggerUSec",
            "ruwac-package-update.timer",
        ])
        .stderr(std::process::Stdio::null())
        .output()
        .ok()
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .filter_map(|l| l.split_once('='))
                .map(|(k, v)| (k.to_owned(), json!(v)))
                .collect::<serde_json::Map<_, _>>()
        })
        .map_or(Value::Null, Value::Object);
    let ring = fs::read_to_string("/etc/ruwac/update.conf")
        .ok()
        .and_then(|t| {
            t.lines()
                .find_map(|l| l.strip_prefix("RING=").map(str::to_owned))
        });
    let installed = installed_rpm();
    let installed_version = installed
        .as_str()
        .and_then(|v| v.split('-').next())
        .map(str::to_owned);
    let engines = inventory(default)
        .map(|m| m["engines"].clone())
        .unwrap_or(json!([]));
    let older = engines
        .as_array()
        .into_iter()
        .flatten()
        .filter(|e| installed_version.as_deref().is_some_and(|v| e["version"] != json!(v)))
        .map(|e| json!({"version":e["version"],"live_terminals":e["live_terminals"],"socket":e["socket"]}))
        .collect::<Vec<_>>();
    json!({
        "installed_rpm":installed,
        "automatic":timer["UnitFileState"] == "enabled",
        "ring":ring,
        "timer":timer,
        "last":last,
        "engines_on_older_version":older,
    })
}

pub fn update_human(report: &Value) -> String {
    let text = |v: &Value| v.as_str().map_or("—".to_owned(), str::to_owned);
    let mut out = format!("Installed RPM      {}\n", text(&report["installed_rpm"]));
    out.push_str(&format!(
        "Automatic updates  {}{}\n",
        if report["automatic"] == true {
            "enabled"
        } else {
            "not enabled (run /usr/libexec/ruwac/fleet-setup)"
        },
        report["ring"]
            .as_str()
            .map_or(String::new(), |r| format!(" · {r} ring"))
    ));
    if let Some(next) = report["timer"]["NextElapseUSecRealtime"]
        .as_str()
        .filter(|n| !n.is_empty())
    {
        out.push_str(&format!("Next check         {next}\n"));
    }
    if report["last"].is_object() {
        out.push_str(&format!(
            "Last check         {} · {} · {}\n",
            text(&report["last"]["checked_at"]),
            text(&report["last"]["result"]),
            text(&report["last"]["detail"])
        ));
    }
    for e in report["engines_on_older_version"]
        .as_array()
        .into_iter()
        .flatten()
    {
        out.push_str(&format!(
            "Engine {} still serves {} running terminal(s); new sessions use the installed version.\n",
            text(&e["version"]),
            e["live_terminals"]
        ));
    }
    out.trim_end().to_owned()
}
