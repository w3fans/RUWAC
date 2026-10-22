//! All potentially blocking RPC and paste delivery run outside the GTK thread.
use ruwac_protocol::{request, write_frame};
use serde_json::{Value, json};
use std::{
    collections::VecDeque,
    fs, io,
    os::unix::{
        fs::{FileTypeExt, MetadataExt},
        io::AsRawFd,
        net::UnixStream,
    },
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc::{self, SyncSender},
    },
    thread,
    time::{Duration, Instant},
};
pub struct Rpc {
    stream: UnixStream,
    limits: ruwac_protocol::TerminalLimits,
    requested_size: Option<(u16, u16)>,
    pane_pids: std::collections::HashMap<String, Value>,
    legacy_wrap_history: bool,
}
impl Rpc {
    pub fn connect(path: &Path) -> io::Result<Self> {
        for (p, mode) in [
            (
                path.parent()
                    .ok_or_else(|| io::Error::other("socket parent missing"))?,
                0o700,
            ),
            (path, 0o600),
        ] {
            let m = fs::symlink_metadata(p)?;
            if m.uid() != unsafe { libc::geteuid() }
                || m.mode() & 0o777 != mode
                || m.file_type().is_symlink()
            {
                return Err(io::Error::other("socket ownership/mode rejected"));
            }
        }
        if !fs::symlink_metadata(path)?.file_type().is_socket() {
            return Err(io::Error::other("not a socket"));
        }
        let s = UnixStream::connect(path)?;
        s.set_read_timeout(Some(Duration::from_secs(2)))?;
        s.set_write_timeout(Some(Duration::from_secs(2)))?;
        let mut cred: libc::ucred = unsafe { std::mem::zeroed() };
        let mut n = std::mem::size_of_val(&cred) as libc::socklen_t;
        if unsafe {
            libc::getsockopt(
                s.as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_PEERCRED,
                &mut cred as *mut _ as *mut _,
                &mut n,
            )
        } != 0
            || cred.uid != unsafe { libc::geteuid() }
        {
            return Err(io::Error::other("server peer UID rejected"));
        }
        Ok(Self {
            stream: s,
            limits: ruwac_protocol::TerminalLimits::LEGACY,
            requested_size: None,
            pane_pids: Default::default(),
            legacy_wrap_history: true,
        })
    }
    /// Linux peer identity binds private launch records to this running engine,
    /// including older engines without a daemon-instance protocol field.
    pub fn daemon_identity(&self) -> io::Result<String> {
        use sha2::{Digest, Sha256};
        let mut cred: libc::ucred = unsafe { std::mem::zeroed() };
        let mut size = std::mem::size_of_val(&cred) as libc::socklen_t;
        if unsafe {
            libc::getsockopt(
                self.stream.as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_PEERCRED,
                &mut cred as *mut _ as *mut _,
                &mut size,
            )
        } != 0
            || cred.uid != unsafe { libc::geteuid() }
            || cred.pid <= 0
        {
            return Err(io::Error::other("Daemon peer identity unavailable"));
        }
        let stat = fs::read_to_string(format!("/proc/{}/stat", cred.pid))?;
        let start = stat
            .rsplit_once(')')
            .and_then(|(_, s)| s.split_whitespace().nth(19))
            .ok_or_else(|| io::Error::other("Daemon start identity unavailable"))?;
        let boot = fs::read_to_string("/proc/sys/kernel/random/boot_id")?;
        Ok(format!(
            "{:x}",
            Sha256::digest(format!("{}:{start}:{}", cred.pid, boot.trim()).as_bytes())
        ))
    }
    fn request(&mut self, method: &str, params: Value) -> io::Result<Value> {
        let r = request(method, params);
        write_frame(&mut self.stream, &r)?;
        self.stream
            .set_read_timeout(Some(Duration::from_secs(if method == "client.render" {
                15
            } else {
                2
            })))?;
        let mut v = ruwac_protocol::read_frame_with_limit(
            &mut self.stream,
            if method == "client.render" {
                ruwac_protocol::MAX_RENDER_FRAME
            } else {
                ruwac_protocol::MAX_FRAME
            },
        )?;
        if v["version"] != 0 || v["id"] != r.id {
            return Err(io::Error::other("protocol/correlation rejected"));
        }
        if let Some(e) = v.get("error") {
            return Err(io::Error::other(e.to_string()));
        }
        Ok(v["result"].take())
    }
    pub fn call(&mut self, method: &str, mut params: Value) -> io::Result<Value> {
        if method == "client.focusLaunch" {
            let panes = self.request("pane.list", json!({"session":params["session"]}))?;
            let valid = panes.as_array().is_some_and(|rows| {
                rows.iter().any(|p| {
                    p["id"] == params["pane"]
                        && p["window"] == params["window"]
                        && p["pid"] == params["pid"]
                        && p["restored"] != true
                })
            });
            if !valid {
                return Err(io::Error::other(
                    "Saved agent pane no longer belongs to this live engine/session",
                ));
            }
            self.request("window.select", json!({"window":params["window"]}))?;
            let reply = self.request("pane.select", json!({"pane":params["pane"]}))?;
            if reply["selected"] != params["pane"] {
                return Err(io::Error::other(
                    "Agent pane selection was not acknowledged",
                ));
            }
            return Ok(
                json!({"window":params["window"],"pane":params["pane"],"focus_epoch":params["focus_epoch"]}),
            );
        }

        if matches!(method, "session.attach" | "client.resize") {
            let raw = (
                params["rows"].as_u64().unwrap_or(23).min(u16::MAX as u64) as u16,
                params["cols"].as_u64().unwrap_or(80).min(u16::MAX as u64) as u16,
            );
            self.requested_size = Some(raw);
            let (rows, cols) = self.limits.clamp(raw.0, raw.1);
            params["rows"] = json!(rows);
            params["cols"] = json!(cols);
        }
        let mut result = ruwac_protocol::client_call(method, params, |m, v| self.request(m, v))?;
        if method == "session.create" {
            let identity = self
                .daemon_identity()
                .map(Value::String)
                .unwrap_or(Value::Null);
            if let Some(result) = result.as_object_mut() {
                result.insert("_launch_daemon".into(), identity);
            }
        }
        if method == "client.render"
            && result["unchanged"] != true
            && result.get("session_name").is_none()
        {
            if let Ok(sessions) = self.call("session.list", json!({})) {
                if let Some(session) = sessions
                    .as_array()
                    .and_then(|rows| rows.iter().find(|row| row["id"] == result["session"]))
                {
                    result["session_name"] = session["name"].clone();
                    result["rows"] = session["rows"].clone();
                    result["cols"] = session["cols"].clone();
                }
            }
            if let Ok(windows) = self.call("window.list", json!({"session":result["session"]})) {
                if let Some(rows) = windows.as_array() {
                    result["window_index"] =
                        json!(rows.iter().position(|row| row["id"] == result["window"]));
                    result["windows"]=json!(rows.iter().enumerate().map(|(index,row)|json!({"id":row["id"],"index":index,"name":row["name"]})).collect::<Vec<_>>());
                }
            }
        }
        if method == "client.render" && result["unchanged"] != true {
            let missing = result["panes"].as_array().is_some_and(|panes| {
                panes.iter().any(|p| {
                    p["id"]
                        .as_str()
                        .is_some_and(|id| !self.pane_pids.contains_key(id))
                })
            });
            if missing {
                if let Ok(Value::Array(panes)) =
                    self.request("pane.list", json!({"session":result["session"]}))
                {
                    if self.pane_pids.len() > 1024 {
                        self.pane_pids.clear();
                    }
                    for pane in panes.into_iter().take(1024) {
                        if let Some(id) = pane["id"].as_str() {
                            self.pane_pids.insert(id.into(), pane["pid"].clone());
                        }
                    }
                }
            }
            if let Some(panes) = result["panes"].as_array_mut() {
                for pane in panes {
                    if let Some(pid) = pane["id"].as_str().and_then(|id| self.pane_pids.get(id)) {
                        pane["pid"] = pid.clone();
                    }
                }
            }
        }
        if method == "client.render" && result["unchanged"] != true {
            if self
                .requested_size
                .is_some_and(|raw| self.limits.clamp(raw.0, raw.1) != raw)
            {
                result["size_notice"] = json!(
                    if self.limits.rows == 100 && self.limits.cols == 200 {
                        "Running older daemon limits the grid to 100×200; preserve sessions and update the daemon when ready"
                    } else {
                        "Terminal cell budget reached; use a larger font or a smaller view"
                    }
                );
            }
        }
        if method == "client.render" && result["unchanged"] != true && self.legacy_wrap_history {
            let selected = result["active_pane"].as_str().map(str::to_owned);
            let legacy = selected.as_ref().and_then(|id| {
                result["panes"]
                    .as_array()?
                    .iter()
                    .find(|p| {
                        p["id"].as_str() == Some(id.as_str()) && p["grid"].get("wrapped").is_none()
                    })
                    .map(|p| p["grid"].clone())
            });
            if let Some((pane, grid)) = selected.zip(legacy) {
                let rows = grid["rows"].as_u64().unwrap_or(0);
                if (1..=500).contains(&rows) {
                    // Ask beyond the bounded history for metadata only, then
                    // read the visible tail. No live daemon restart is needed.
                    if let Ok(head) = self.request(
                        "pane.history",
                        json!({"pane":pane,"offset":1000000,"lines":1}),
                    ) {
                        if !head["rows"].is_array() && !head["wrapped"].is_array() {
                            self.legacy_wrap_history = false;
                        } else if let (Some(total), Some(epoch)) =
                            (head["total_lines"].as_u64(), head["epoch"].as_u64())
                        {
                            if total >= rows {
                                if let Ok(page)=self.request("pane.history",json!({"pane":pane,"offset":total-rows,"lines":rows,"epoch":epoch})) {
                                    if page["epoch"]==epoch && page["total_lines"]==total
                                        && page["offset"]==total-rows && page["next_offset"]==total {
                                        if let Some(wrapped)=matching_history_wraps(&grid,&page) {
                                            if let Some(target)=result["panes"].as_array_mut().and_then(|panes|panes.iter_mut().find(|p|p["id"]==pane)) {
                                                target["grid"]["wrapped"]=wrapped;
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        Ok(result)
    }
}
fn matching_history_wraps(grid: &Value, page: &Value) -> Option<Value> {
    let cells = grid["cells"].as_array()?;
    if grid["alternate_screen"] != page["alternate_screen"] {
        return None;
    }
    // Early engines provide logical text plus wrap flags rather than row
    // objects. Reconstruct their exact encoding from the displayed cells;
    // accept flags only when the entire page matches, including whitespace.
    if page.get("rows").is_none() {
        let flags = page["wrapped"].as_array()?;
        if flags.len() != cells.len() {
            return None;
        }
        let mut text = String::new();
        for (cells, flag) in cells.iter().zip(flags) {
            let wrap = flag.as_bool()?;
            let mut row = String::new();
            for cell in cells.as_array()? {
                if cell[1] != true {
                    row.push_str(cell[0].as_str()?);
                }
            }
            text.push_str(if wrap { &row } else { row.trim_end() });
            if !wrap {
                text.push('\n');
            }
        }
        return (page["text"].as_str()? == text).then(|| json!(flags));
    }
    let rows = page["rows"].as_array()?;
    if cells.len() != rows.len() {
        return None;
    }
    let mut wrapped = Vec::new();
    for (cells, row) in cells.iter().zip(rows) {
        let mut text = String::new();
        for cell in cells.as_array()? {
            if cell[1] != true {
                text.push_str(cell[0].as_str()?);
            }
        }
        if text.replace('\t', " ").trim_end_matches(' ')
            != row["text"].as_str()?.trim_end_matches(' ')
        {
            return None;
        }
        wrapped.push(row["wrapped"].as_bool()?);
    }
    Some(json!(wrapped))
}
fn paste(
    rpc: &mut Rpc,
    pane: &str,
    text: &str,
    atomic: bool,
    halt: &AtomicBool,
    allowed: Option<&AtomicBool>,
) -> io::Result<()> {
    if text.len() > 1024 * 1024 || text.contains('\0') || text.contains("\x1b[201~") {
        return Err(io::Error::other(
            "paste text rejected before input delivery",
        ));
    }
    if halt.load(Ordering::Relaxed) || allowed.is_some_and(|a| !a.load(Ordering::Acquire)) {
        return Err(io::Error::other("paste cancelled before delivery"));
    }
    if !atomic {
        if text.len() > 32 * 1024 {
            return Err(io::Error::other(
                "This daemon cannot accept a single large paste; update the daemon before retrying",
            ));
        }
        rpc.call(
            "client.input",
            json!({"pane":pane,"kind":"paste","text":text}),
        )?;
        return Ok(());
    }
    let metadata = rpc.call(
        "client.paste",
        json!({"operation":"begin","pane":pane,"bytes":text.len()}),
    )?;
    let token = metadata["token"]
        .as_str()
        .ok_or_else(|| io::Error::other("paste token missing"))?;
    let result = (|| -> io::Result<()> {
        let mut offset = 0;
        while offset < text.len() {
            if halt.load(Ordering::Relaxed) || allowed.is_some_and(|a| !a.load(Ordering::Acquire)) {
                return Err(io::Error::other("paste cancelled; no input committed"));
            }
            let mut end = (offset + 32 * 1024).min(text.len());
            while !text.is_char_boundary(end) {
                end -= 1;
            }
            rpc.call("client.paste", json!({"operation":"chunk","token":token,"offset":offset,"text":&text[offset..end]}))?;
            offset = end;
        }
        if halt.load(Ordering::Relaxed) || allowed.is_some_and(|a| !a.load(Ordering::Acquire)) {
            return Err(io::Error::other("paste cancelled; no input committed"));
        }
        rpc.call("client.paste", json!({"operation":"commit","token":token}))?;
        Ok(())
    })();
    if result.is_err() {
        // A timed-out commit may have reached the daemon. Never replay it.
        let _ = rpc.call("client.paste", json!({"operation":"abort","token":token}));
    }
    result
}
#[derive(Debug)]
pub enum Command {
    Rpc(String, Value),
    Paste(String, String),
    GuardedPaste {
        pane: String,
        text: String,
        allowed: Arc<AtomicBool>,
        done: SyncSender<Result<(), String>>,
    },
    History(String, bool),
    AttachImage {
        pane: String,
        path: std::path::PathBuf,
        allowed: Arc<AtomicBool>,
        done: SyncSender<Result<Value, String>>,
    },
    Close,
}
impl Command {
    fn bytes(&self) -> usize {
        match self {
            Self::Rpc(_, v) => v.to_string().len(),
            Self::Paste(_, s) | Self::GuardedPaste { text: s, .. } => s.len(),
            Self::History(s, _) => s.len(),
            Self::AttachImage { pane, path, .. } => pane.len() + path.as_os_str().len(),
            Self::Close => 0,
        }
    }
}
#[derive(Default)]
pub struct State {
    pub frame: Option<Value>,
    pub session_ended: bool,
    /// The daemon no longer lists this session (killed or removed elsewhere).
    pub session_gone: bool,
    /// The connection failed while the session may still exist.
    pub disconnected: bool,
    pub error: Option<String>,
    pub reply: VecDeque<(String, Value)>,
    pub acknowledgements: u64,
    pub coalesced: u64,
    pub supports_previous_window: bool,
}
pub struct Worker {
    tx: SyncSender<Command>,
    pub state: Arc<Mutex<State>>,
    stop: Arc<AtomicBool>,
    bytes: Arc<AtomicUsize>,
    visible: Arc<AtomicBool>,
}
/// Two render frames show the same content (revision numbers aside). The
/// revision counts changes in every session of the daemon.
fn same_frame(a: &Value, b: &Value) -> bool {
    let (Some(a), Some(b)) = (a.as_object(), b.as_object()) else {
        return false;
    };
    a.len() == b.len()
        && a.iter()
            .all(|(k, v)| k == "revision" || b.get(k).is_some_and(|w| w == v))
}
/// True only when a reachable daemon positively no longer lists the session.
fn session_missing(path: &Path, session: &str) -> bool {
    Rpc::connect(path)
        .and_then(|mut rpc| rpc.call("session.list", json!({})))
        .ok()
        .and_then(|rows| {
            rows.as_array()
                .map(|rows| !rows.iter().any(|r| r["id"].as_str() == Some(session)))
        })
        .unwrap_or(false)
}
/// Start the daemon through the installed sibling CLI, which owns startup.
fn start_daemon(path: &Path) -> io::Result<()> {
    let cli = std::env::current_exe()?
        .parent()
        .map(|dir| dir.join("ruwac"))
        .filter(|cli| cli.is_file())
        .ok_or_else(|| io::Error::other("ruwac CLI not installed beside the desktop"))?;
    let status = std::process::Command::new(cli)
        .arg("--socket")
        .arg(path)
        .arg("start")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()?;
    if !status.success() {
        return Err(io::Error::other("daemon start failed"));
    }
    Ok(())
}
impl Worker {
    pub fn start(path: std::path::PathBuf, session: String) -> Self {
        Self::spawn(path, session, false)
    }
    /// Reconnect after a lost daemon connection, starting the daemon if absent.
    pub fn restart(path: std::path::PathBuf, session: String) -> Self {
        Self::spawn(path, session, true)
    }
    fn spawn(path: std::path::PathBuf, session: String, ensure_daemon: bool) -> Self {
        let (tx, rx) = mpsc::sync_channel::<Command>(128);
        let state = Arc::new(Mutex::new(State::default()));
        let stop = Arc::new(AtomicBool::new(false));
        let bytes = Arc::new(AtomicUsize::new(0));
        let visible = Arc::new(AtomicBool::new(true));
        let (st, halt, budget, seen) =
            (state.clone(), stop.clone(), bytes.clone(), visible.clone());
        thread::spawn(move || {
            let result = (|| -> io::Result<()> {
                let mut rpc = match Rpc::connect(&path) {
                    Err(_) if ensure_daemon => {
                        start_daemon(&path)?;
                        Rpc::connect(&path)?
                    }
                    other => other?,
                };
                let schema = rpc.call("schema", json!({}))?;
                if schema["protocol"] != 0 {
                    return Err(io::Error::other("unsupported daemon API"));
                }
                st.lock().unwrap().supports_previous_window = schema["methods"]
                    .as_array()
                    .is_some_and(|methods| methods.iter().any(|m| m == "window.previous"));
                let capabilities = rpc.call("capabilities", json!({}))?;
                rpc.limits = ruwac_protocol::TerminalLimits::advertised(&capabilities);
                // A not-yet-allocated or hidden tab must not impose a 24x80
                // transient resize on an already shared session.
                let mut early = VecDeque::new();
                let mut attached = false;
                while !halt.load(Ordering::Relaxed) {
                    match rx.recv_timeout(Duration::from_millis(20)) {
                        Ok(Command::Rpc(method, params)) if method == "client.resize" => {
                            budget.fetch_sub(
                                Command::Rpc(method, params.clone()).bytes(),
                                Ordering::Relaxed,
                            );
                            rpc.call("session.attach",json!({"session":session,"rows":params["rows"],"cols":params["cols"]}))?;
                            attached = true;
                            break;
                        }
                        Ok(Command::Close) => {
                            halt.store(true, Ordering::Relaxed);
                        }
                        Ok(command) => {
                            if early.len() >= 128 {
                                return Err(io::Error::other(
                                    "Unmapped terminal command queue full",
                                ));
                            }
                            early.push_back(command);
                        }
                        Err(mpsc::RecvTimeoutError::Disconnected) => return Ok(()),
                        Err(mpsc::RecvTimeoutError::Timeout) => (),
                    }
                }
                if !attached {
                    return Ok(());
                }
                let mut revision = None;
                // Last frame handed to the view; identical frames are not redrawn.
                let mut shown: Option<Value> = None;
                // Poll fast while output or input is recent, slowly when quiet.
                let mut active = Instant::now();
                while !halt.load(Ordering::Relaxed) {
                    for _ in 0..16 {
                        let Some(cmd) = early.pop_front().or_else(|| rx.try_recv().ok()) else {
                            break;
                        };
                        let size = cmd.bytes();
                        let result = match cmd {
                            Command::Rpc(m, p) if m == "client.retireEnded" => {
                                (|| -> io::Result<_> {
                                    let ids = p["panes"]
                                        .as_array()
                                        .filter(|ids| ids.len() <= 128)
                                        .ok_or_else(|| {
                                            io::Error::other("Invalid completed window")
                                        })?;
                                    for id in ids {
                                        let id = id
                                            .as_str()
                                            .filter(|id| ruwac_protocol::valid_id(id, "pane"))
                                            .ok_or_else(|| {
                                                io::Error::other("Invalid completed pane")
                                            })?;
                                        if let Err(error) = rpc
                                            .call("pane.close", json!({"pane":id,"if_ended":true}))
                                        {
                                            let stale =
                                                serde_json::from_str::<Value>(&error.to_string())
                                                    .is_ok_and(|value| value["code"] == "STALE_ID");
                                            if !stale {
                                                return Err(error);
                                            }
                                        }
                                    }
                                    Ok((m, json!({"closed":true})))
                                })()
                            }
                            Command::Rpc(m, p) => rpc.call(&m, p).map(|v| (m, v)),
                            Command::Close => {
                                halt.store(true, Ordering::Relaxed);
                                Ok(("close".into(), json!({})))
                            }
                            Command::AttachImage {
                                pane,
                                path,
                                allowed,
                                done,
                            } => {
                                let native_api = schema["methods"]
                                    .as_array()
                                    .is_some_and(|m| m.iter().any(|m| m == "pane.attachImage"));
                                let result = crate::image_attach::attach(
                                    &mut rpc, &session, &pane, &path, &allowed, native_api,
                                )
                                .map_err(|e| e.to_string());
                                let _ = done.try_send(result.clone());
                                result
                                    .map(|v| ("image-attach".into(), v))
                                    .map_err(io::Error::other)
                            }
                            Command::History(pane, primary) => {
                                let mut text = String::new();
                                let mut offset = 0;
                                let mut total = 0;
                                let mut truncated = false;
                                let mut epoch = None;
                                let mut pin = None;
                                let mut result = (|| -> io::Result<()> {
                                    if primary && capabilities["primary_buffer_history"] != true {
                                        return Err(io::Error::other(
                                            "This daemon does not expose primary history; update it before selecting that buffer",
                                        ));
                                    }

                                    if schema["methods"].as_array().is_some_and(|methods| {
                                        methods.iter().any(|m| m == "pane.history.freeze")
                                    }) {
                                        let metadata = rpc.call(
                                            "pane.history.freeze",
                                            if primary {
                                                json!({"pane":pane,"buffer":"primary"})
                                            } else {
                                                json!({"pane":pane})
                                            },
                                        )?;
                                        pin = Some(
                                            metadata["snapshot"]
                                                .as_str()
                                                .ok_or_else(|| {
                                                    io::Error::other("Copy snapshot ID missing")
                                                })?
                                                .to_owned(),
                                        );
                                        truncated =
                                            metadata["dropped_lines"].as_u64().unwrap_or(0) > 0;
                                    }
                                    loop {
                                        let mut params = json!({"pane":pane,"offset":offset,"lines":500,"epoch":epoch});
                                        if let Some(pin) = &pin {
                                            params["snapshot"] = json!(pin);
                                        }
                                        if primary {
                                            params["buffer"] = json!("primary");
                                        }
                                        let page = rpc.call("pane.history", params)?;
                                        epoch = page["epoch"].as_u64();
                                        let part = page["text"].as_str().unwrap_or("");
                                        let next = page["next_offset"].as_u64().unwrap_or(0);
                                        total = page["total_lines"].as_u64().unwrap_or(0);
                                        if text.len() + part.len() > 8 * 1024 * 1024 {
                                            truncated = true;
                                            break;
                                        }
                                        text.push_str(part);
                                        if next >= total || halt.load(Ordering::Relaxed) {
                                            break;
                                        }
                                        if next <= offset {
                                            return Err(io::Error::other(
                                                "Copy history paging stalled",
                                            ));
                                        }
                                        offset = next;
                                    }
                                    Ok(())
                                })();
                                if let Some(pin) = pin {
                                    let release =
                                        rpc.call("pane.history.release", json!({"snapshot":pin}));
                                    if result.is_ok() {
                                        result = release.map(|_| ());
                                    }
                                }
                                result.map(|()|("pane.history".into(),json!({"pane":pane,"text":text,"total_lines":total,"truncated":truncated,"buffer":if primary {"primary"}else{"active"}})))
                            }
                            Command::GuardedPaste {
                                pane,
                                text,
                                allowed,
                                done,
                            } => {
                                let result = paste(
                                    &mut rpc,
                                    &pane,
                                    &text,
                                    capabilities["atomic_paste"] == true,
                                    &halt,
                                    Some(&allowed),
                                );
                                let _ = done.try_send(
                                    result.as_ref().map(|_| ()).map_err(|e| e.to_string()),
                                );
                                result
                                    .map(|()| ("guarded-paste".into(), json!({"bytes":text.len()})))
                            }
                            Command::Paste(pane, text) => paste(
                                &mut rpc,
                                &pane,
                                &text,
                                capabilities["atomic_paste"] == true,
                                &halt,
                                None,
                            )
                            .map(|()| ("paste".into(), json!({"bytes":text.len()}))),
                        };
                        budget.fetch_sub(size, Ordering::Relaxed);
                        let mut s = st.lock().unwrap();
                        match result {
                            Ok(reply) => {
                                s.acknowledgements += 1;
                                if reply.0 == "pane.history"
                                    || reply.0 == "client.retireEnded"
                                    || reply.0 == "client.focusLaunch"
                                    || reply.0.starts_with("window.")
                                    || reply.0 == "pane.create"
                                    || reply.0 == "pane.capture"
                                    || reply.0 == "buffer.get"
                                    || reply.0 == "pane.selection"
                                    || reply.0 == "paste"
                                    || reply.0 == "window.list"
                                {
                                    if s.reply.len() < 32 {
                                        s.reply.push_back(reply);
                                    } else {
                                        s.error = Some(
                                            "Reply queue full; command response unavailable".into(),
                                        );
                                    }
                                }
                            }
                            Err(e) => s.error = Some(format!("Input/command delivery failed: {e}")),
                        };
                    }
                    let mut frame = rpc.call(
                        "client.render",
                        revision.map(|r| json!({"revision":r})).unwrap_or(json!({})),
                    )?;
                    if frame["unchanged"] != true {
                        st.lock().unwrap().session_ended = false;
                        if frame["panes"].as_array().is_some_and(|panes| {
                            !panes.is_empty() && panes.iter().all(|p| p["ended"] == true)
                        }) {
                            let panes = rpc.call("pane.list", json!({"session":session}))?;
                            if let Some(live) = panes
                                .as_array()
                                .and_then(|rows| rows.iter().find(|p| p["ended"] != true))
                            {
                                rpc.call("window.select", json!({"window":live["window"]}))?;
                                rpc.call("pane.select", json!({"pane":live["id"]}))?;
                                frame = rpc.call("client.render", json!({}))?;
                            } else {
                                st.lock().unwrap().session_ended = true;
                            }
                        }
                        let r = frame["revision"]
                            .as_u64()
                            .ok_or_else(|| io::Error::other("missing render revision"))?;
                        if revision.is_some_and(|old| r < old) {
                            return Err(io::Error::other("stale render revision"));
                        }
                        revision = Some(r);
                        if !shown.as_ref().is_some_and(|last| same_frame(last, &frame)) {
                            active = Instant::now();
                            shown = Some(frame.clone());
                            let mut s = st.lock().unwrap();
                            if s.frame.is_some() {
                                s.coalesced += 1;
                            }
                            s.frame = Some(frame);
                        }
                    }
                    // Tabs out of view poll slowly; a quiet screen polls at 10 Hz.
                    // Any command (typing, resize) ends the wait immediately.
                    let wait = if !seen.load(Ordering::Relaxed) {
                        250
                    } else if active.elapsed() < Duration::from_secs(1) {
                        20
                    } else {
                        100
                    };
                    if let Ok(cmd) = rx.recv_timeout(Duration::from_millis(wait)) {
                        early.push_back(cmd);
                        active = Instant::now();
                    }
                }
                rpc.call("session.detach", json!({}))?;
                Ok(())
            })();
            if let Err(e) = result {
                let gone = session_missing(&path, &session);
                let mut s = st.lock().unwrap();
                if gone {
                    s.session_gone = true;
                } else {
                    s.disconnected = true;
                    s.error = Some(format!(
                        "Disconnected: {e}; queued input may be undelivered"
                    ));
                }
            }
            halt.store(true, Ordering::Relaxed);
        });
        Self {
            tx,
            state,
            stop,
            bytes,
            visible,
        }
    }
    /// Whether the view is on screen (polls fast) or hidden (polls slowly).
    pub fn set_visible(&self, visible: bool) {
        self.visible.store(visible, Ordering::Relaxed);
    }
    pub fn send(&self, cmd: Command) -> Result<(), String> {
        if self.stop.load(Ordering::Relaxed) {
            return Err("View disconnected; input not sent".into());
        }
        let size = cmd.bytes();
        if self
            .bytes
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| {
                n.checked_add(size).filter(|n| *n <= 2 * 1024 * 1024)
            })
            .is_err()
        {
            return Err("Queue byte limit reached; input not sent".into());
        }
        if self.tx.try_send(cmd).is_err() {
            self.bytes.fetch_sub(size, Ordering::Relaxed);
            return Err("Queue full; input not sent".into());
        }
        Ok(())
    }
    pub fn close(&self) {
        if self.tx.try_send(Command::Close).is_err() {
            self.stop.store(true, Ordering::Relaxed);
            self.state.lock().unwrap().error =
                Some("Detach queue saturated; pending input may be undelivered".into());
        }
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        self.close();
    }
}

#[cfg(test)]
mod launch_focus_tests {
    use super::*;
    #[test]
    fn launch_focus_checks_membership_before_any_selection() {
        for fault in ["none", "pid", "pane", "window", "restored"] {
            let (client, mut server) = UnixStream::pair().unwrap();
            client
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let target = json!({"session":"generated-session","window":"generated-window","pane":"generated-pane","pid":42,"focus_epoch":7});
            let mut row = json!({"id":"generated-pane","window":"generated-window","pid":42,"restored":false});
            match fault {
                "pid" => row["pid"] = json!(43),
                "pane" => row["id"] = json!("different-pane"),
                "window" => row["window"] = json!("different-window"),
                "restored" => row["restored"] = json!(true),
                _ => (),
            }
            let expected = target.clone();
            let peer = thread::spawn(move || {
                let request =
                    ruwac_protocol::read_frame_with_limit(&mut server, ruwac_protocol::MAX_FRAME)
                        .unwrap();
                assert_eq!(request["method"], "pane.list");
                assert_eq!(request["params"]["session"], expected["session"]);
                write_frame(
                    &mut server,
                    &json!({"version":0,"id":request["id"],"result":[row]}),
                )
                .unwrap();
                if fault == "none" {
                    for (method, key) in [("window.select", "window"), ("pane.select", "pane")] {
                        let request = ruwac_protocol::read_frame_with_limit(
                            &mut server,
                            ruwac_protocol::MAX_FRAME,
                        )
                        .unwrap();
                        assert_eq!(request["method"], method);
                        assert_eq!(request["params"][key], expected[key]);
                        write_frame(&mut server,&json!({"version":0,"id":request["id"],"result":{"selected":expected[key]}})).unwrap();
                    }
                }
                // Closing the client must reveal no unreviewed mutation request.
                let mut byte = [0u8; 1];
                server
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                assert_eq!(std::io::Read::read(&mut server, &mut byte).unwrap(), 0);
            });
            let mut rpc = Rpc {
                stream: client,
                limits: ruwac_protocol::TerminalLimits::CURRENT,
                requested_size: None,
                pane_pids: Default::default(),
                legacy_wrap_history: true,
            };
            let result = rpc.call("client.focusLaunch", target);
            assert_eq!(result.is_ok(), fault == "none");
            if fault == "none" {
                let reply = result.unwrap();
                assert_eq!(reply["pane"], "generated-pane");
                assert_eq!(reply["window"], "generated-window");
                assert_eq!(reply["focus_epoch"], 7);
            }
            drop(rpc);
            peer.join().unwrap();
        }
    }
}

#[cfg(test)]
mod legacy_wrap_tests {
    use super::*;
    #[test]
    fn early_engine_logical_text_must_match_cells_and_wrap_flags_exactly() {
        let grid = json!({"alternate_screen":false,"cells":[[["a",false],[" ",false]],[["界",false],["",true]],[["\t",false],[" ",false]]]});
        let page = json!({"alternate_screen":false,"wrapped":[true,false,false],"text":"a 界\n\n"});
        assert_eq!(
            matching_history_wraps(&grid, &page),
            Some(json!([true, false, false]))
        );
        for (key, value) in [
            ("text", json!("a界\n\n")),
            ("wrapped", json!([true, false])),
            ("wrapped", json!([true, false, 0])),
            ("alternate_screen", json!(true)),
        ] {
            let mut changed = page.clone();
            changed[key] = value;
            assert!(matching_history_wraps(&grid, &changed).is_none());
        }
    }
    #[test]
    fn verified_history_rows_supply_wraps_without_changing_the_screen() {
        let grid = json!({"alternate_screen":false,"cells":[[["a",false],[" ",false]],[["界",false],["",true]]]});
        let page = json!({"alternate_screen":false,"rows":[{"text":"a ","wrapped":true},{"text":"界","wrapped":false}]});
        assert_eq!(
            matching_history_wraps(&grid, &page),
            Some(json!([true, false]))
        );
    }
    #[test]
    fn stale_incomplete_and_other_buffer_history_never_supply_wraps() {
        let grid = json!({"alternate_screen":false,"cells":[[["a",false]]]});
        for page in [
            json!({"alternate_screen":false,"rows":[]}),
            json!({"alternate_screen":true,"rows":[{"text":"a","wrapped":true}]}),
            json!({"alternate_screen":false,"rows":[{"text":"b","wrapped":true}]}),
            json!({"alternate_screen":false,"rows":[{"text":"a","wrapped":0}]}),
        ] {
            assert!(matching_history_wraps(&grid, &page).is_none());
        }
    }
}
