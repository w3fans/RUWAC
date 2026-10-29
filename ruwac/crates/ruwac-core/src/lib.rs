mod image;
mod jobs;
mod layout;
mod paste;
mod recovery;
use layout::{Layout, Rect};
pub use recovery::Store;
use ruwac_protocol::{Error, Request, Result, checked_text, id, target, text};
use ruwac_pty::Pty;
use ruwac_term::Terminal;
use serde_json::{Value, json};
use std::collections::{HashMap, VecDeque};
use tokio::sync::{mpsc, oneshot, watch};
struct Pane {
    id: String,
    window: String,
    pty: Option<Pty>,
    exit_code: Option<u32>,
    term: Terminal,
    ended: bool,
    foreground: Option<i32>,
    job: Option<jobs::Job>,
}
impl Pane {
    fn live_pty(&self) -> Result<&Pty> {
        if self.ended {
            return Err(Error::new(
                "PANE_EXITED",
                "child exited; history is read-only",
            ));
        }
        self.pty
            .as_ref()
            .ok_or_else(|| Error::new("PANE_EXITED", "restored history has no live process"))
    }
}
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Window {
    id: String,
    session: String,
    name: String,
    panes: Vec<String>,
    active: String,
    layout: Layout,
    zoom: bool,
}
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Session {
    id: String,
    name: String,
    windows: Vec<String>,
    active: String,
    rows: u16,
    cols: u16,
    #[serde(skip)]
    buffer: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    previous: Option<String>,
}
impl Session {
    fn select_window(&mut self, window: String) {
        if self.active != window {
            self.previous = (!self.active.is_empty()).then(|| self.active.clone());
            self.active = window;
        }
    }
}
struct Client {
    // Last full projection, excluding the daemon-wide output revision. Other
    // sessions can advance that revision without changing this client's view.
    render_stamp: Option<(u64, Value)>,
    paste: Option<paste::Upload>,
    scope: String,
    session: String,
    rows: u16,
    cols: u16,
    window: String,
    previous: Option<String>,
    panes: HashMap<String, String>,
    zooms: HashMap<String, bool>,
}
impl Client {
    fn select_window(&mut self, window: String) {
        if self.window != window {
            self.previous = Some(self.window.clone());
            self.window = window;
        }
    }
}
pub enum Message {
    Call(Request, String, oneshot::Sender<Result<Value>>),
    Output(String, Vec<u8>),
    Disconnect(String),
    Saved(u64, std::result::Result<(), String>),
}
#[derive(Clone)]
pub struct Handle {
    pub tx: mpsc::Sender<Message>,
    pub revision: watch::Receiver<u64>,
}
impl Handle {
    pub async fn call(&self, r: Request, client: &str) -> Result<Value> {
        let (tx, rx) = oneshot::channel();
        self.tx
            .send(Message::Call(r, client.into(), tx))
            .await
            .map_err(|_| Error::new("UNAVAILABLE", "daemon stopped"))?;
        rx.await
            .map_err(|_| Error::new("UNAVAILABLE", "daemon stopped"))?
    }
}
struct HistoryPin {
    owner: String,
    pane: String,
    read_at: std::time::Instant,
    data: ruwac_term::HistorySnapshot,
}
struct State {
    panes: HashMap<String, Pane>,
    windows: HashMap<String, Window>,
    sessions: HashMap<String, Session>,
    clients: HashMap<String, Client>,
    histories: HashMap<String, HistoryPin>,
    tx: mpsc::Sender<Message>,
    cache: HashMap<(Option<String>, String), (String, Result<Value>)>,
    order: VecDeque<(Option<String>, String)>,
    revision: u64,
    store: Option<std::sync::Arc<Store>>,
    checkpoint_revision: u64,
    checkpoint_pending: bool,
    checkpoint_error: Option<String>,
}
pub fn start() -> Handle {
    start_with_store(None).expect("in-memory state initialization")
}
pub fn start_with_store(store: Option<Store>) -> std::io::Result<Handle> {
    let (tx, mut rx) = mpsc::channel(128);
    let (rev, watch) = watch::channel(0);
    let handle = Handle {
        tx: tx.clone(),
        revision: watch,
    };
    let mut s = State {
        panes: HashMap::new(),
        windows: HashMap::new(),
        sessions: HashMap::new(),
        clients: HashMap::new(),
        histories: HashMap::new(),
        tx,
        cache: HashMap::new(),
        order: VecDeque::new(),
        revision: 0,
        store: store.map(std::sync::Arc::new),
        checkpoint_revision: 0,
        checkpoint_pending: false,
        checkpoint_error: None,
    };
    s.restore()?;
    let _ = rev.send(s.revision);
    tokio::spawn(async move {
        let mut timer = tokio::time::interval(std::time::Duration::from_secs(2));
        timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            let m = tokio::select! {
                m=rx.recv()=>match m {Some(m)=>m,None=>break},
                _=timer.tick()=>{s.expire_histories();s.expire_pastes();s.schedule_checkpoint(); continue;},
            };
            match m {
                Message::Call(r, c, reply) => {
                    let answer = if matches!(r.method.as_str(), "daemon.checkpoint" | "daemon.stop")
                        && r.version == 0
                        && (r.params.is_object() || r.params.is_null())
                        && r.idempotency_key.is_none()
                    {
                        match s.checkpoint().await {
                            Ok(()) => Ok(if r.method == "daemon.stop" {
                                json!({"stopped":true})
                            } else {
                                json!({"checkpointed":true,"revision":s.checkpoint_revision})
                            }),
                            Err(e) => Err(Error::new("CHECKPOINT", e)),
                        }
                    } else {
                        s.call(&r, &c)
                    };
                    let stop = r.method == "daemon.stop" && answer.is_ok();
                    let _ = reply.send(answer);
                    if !matches!(
                        r.method.as_str(),
                        "health"
                            | "daemon.checkpoint"
                            | "version"
                            | "capabilities"
                            | "schema"
                            | "describe"
                            | "session.list"
                            | "window.list"
                            | "pane.list"
                            | "job.list"
                            | "job.query"
                            | "agent.probe"
                            | "pane.history"
                            | "pane.history.freeze"
                            | "pane.history.release"
                            | "pane.capture"
                            | "pane.wait"
                            | "buffer.get"
                            | "pane.selection"
                            | "client.render"
                    ) {
                        s.revision += 1;
                        let _ = rev.send(s.revision);
                    }
                    if stop {
                        break;
                    }
                }
                Message::Output(p, b) => {
                    if let Some(p) = s.panes.get_mut(&p) {
                        let foreground = p.pty.as_ref().and_then(Pty::foreground_group);
                        if p.foreground != foreground {
                            p.term.reset_input_modes();
                            p.foreground = foreground;
                        }
                        if b.is_empty() {
                            p.ended = true;
                            p.exit_code = p.pty.as_ref().and_then(Pty::exit_code);
                            p.term.reset_input_modes();
                        } else {
                            if let Some(job) = p.job.as_mut() {
                                job.output();
                            }
                            for reply in p.term.feed(&b) {
                                if let Some(pty) = &p.pty {
                                    let _ = pty.send(reply.into_bytes());
                                }
                            }
                        }
                        s.revision += 1;
                        let _ = rev.send(s.revision);
                    }
                }
                Message::Saved(revision, result) => {
                    s.checkpoint_pending = false;
                    match result {
                        Ok(()) => {
                            s.checkpoint_revision = s.checkpoint_revision.max(revision);
                            s.checkpoint_error = None;
                        }
                        Err(e) => {
                            s.checkpoint_error = Some(e);
                        }
                    }
                }
                Message::Disconnect(c) => {
                    s.histories.retain(|_, pin| pin.owner != c);
                    if let Some(c) = s.clients.remove(&c) {
                        s.layout(&c.session);
                        s.revision += 1;
                        let _ = rev.send(s.revision);
                    }
                }
            }
        }
    });
    Ok(handle)
}
fn num(v: &Value, k: &str, default: u64, min: u64, max: u64) -> Result<u16> {
    let n = match v.get(k) {
        Some(x) => x
            .as_u64()
            .ok_or_else(|| Error::new("INVALID_PARAMS", format!("{k} must be an integer")))?,
        None => default,
    };
    if n < min || n > max {
        return Err(Error::new("LIMIT", format!("{k} must be {min}..{max}")));
    }
    Ok(n as u16)
}
fn terminal_dimensions(v: &Value) -> Result<(u16, u16)> {
    let limits = ruwac_protocol::TerminalLimits::CURRENT;
    let rows = num(v, "rows", 23, 4, limits.rows as u64)?;
    let cols = num(v, "cols", 80, 10, limits.cols as u64)?;
    if !limits.valid(rows, cols) {
        return Err(Error::new("LIMIT", "terminal grid exceeds the cell budget"));
    }
    Ok((rows, cols))
}
fn argv(v: &Value) -> Result<Vec<String>> {
    match v.get("argv") {
        None => ruwac_pty::shell::default_argv().map_err(|e| Error::new("INVALID_SHELL", e)),
        Some(Value::Array(a)) if !a.is_empty() && a.len() <= 64 => a
            .iter()
            .map(|x| {
                let s = x
                    .as_str()
                    .ok_or_else(|| Error::new("INVALID_PARAMS", "argv elements must be strings"))?;
                checked_text(s)?;
                Ok(s.into())
            })
            .collect(),
        _ => Err(Error::new(
            "INVALID_PARAMS",
            "argv must be a nonempty string array, max 64",
        )),
    }
}
impl State {
    fn call(&mut self, r: &Request, client: &str) -> Result<Value> {
        if r.version != 0 {
            return Err(Error::new("VERSION", "supported protocol version is 0"));
        }
        if !r.params.is_object() && !r.params.is_null() {
            return Err(Error::new("INVALID_PARAMS", "params must be an object"));
        }
        if let Some(k) = &r.idempotency_key {
            if k.is_empty() || k.len() > 128 {
                return Err(Error::new(
                    "INVALID_PARAMS",
                    "idempotency key must be 1..128 bytes",
                ));
            }
            if matches!(
                r.method.as_str(),
                "session.attach"
                    | "session.detach"
                    | "client.render"
                    | "client.input"
                    | "client.paste"
                    | "job.list"
                    | "job.query"
                    | "agent.probe"
                    | "pane.history.freeze"
                    | "pane.history.release"
                    | "daemon.stop"
                    | "daemon.checkpoint"
            ) || (r.method == "pane.history" && r.params.get("snapshot").is_some())
            {
                return Err(Error::new(
                    "INVALID_PARAMS",
                    "idempotency unsupported for connection-local operations",
                ));
            }
            let scope = self.clients.contains_key(client)
                && matches!(
                    r.method.as_str(),
                    "window.select"
                        | "window.cycle"
                        | "window.previous"
                        | "pane.select"
                        | "pane.layout"
                        | "client.resize"
                        | "window.list"
                        | "session.list"
                );
            let cache_key = (scope.then(|| self.clients[client].scope.clone()), k.clone());
            let signature = format!("{}:{}", r.method, r.params);
            if let Some((old, result)) = self.cache.get(&cache_key) {
                return if *old == signature {
                    result.clone()
                } else {
                    Err(Error::new(
                        "IDEMPOTENCY_CONFLICT",
                        "key reused with different request",
                    ))
                };
            }
            let result = self.dispatch(&r.method, &r.params, client);
            self.cache
                .insert(cache_key.clone(), (signature, result.clone()));
            self.order.push_back(cache_key);
            if self.order.len() > 1024 {
                if let Some(k) = self.order.pop_front() {
                    self.cache.remove(&k);
                }
            }
            return result;
        }
        self.dispatch(&r.method, &r.params, client)
    }
    fn pane(&self, v: &Value) -> Result<&Pane> {
        let p = target(v, "pane", "pane")?;
        self.panes
            .get(&p)
            .ok_or_else(|| Error::new("STALE_ID", "pane does not exist"))
    }
    fn create_pane(&mut self, w: &str, args: Vec<String>, rows: u16, cols: u16) -> Result<String> {
        if self.panes.len() >= 128 {
            return Err(Error::new("LIMIT", "maximum 128 panes"));
        }
        let pid = id("pane");
        let tx = self.tx.clone();
        let outid = pid.clone();
        let pty = Pty::spawn(&args, rows, cols, move |b| {
            let _ = tx.blocking_send(Message::Output(outid.clone(), b));
        })
        .map_err(|e| Error::new("SPAWN", e))?;
        self.panes.insert(
            pid.clone(),
            Pane {
                id: pid.clone(),
                window: w.into(),
                foreground: pty.foreground_group(),
                job: Some(jobs::Job::new(&args[0], pty.pid)),
                pty: Some(pty),
                exit_code: None,
                term: Terminal::new(rows, cols),
                ended: false,
            },
        );
        Ok(pid)
    }
    fn create_window(&mut self, s: &str, v: &Value) -> Result<Value> {
        let session = self
            .sessions
            .get(s)
            .ok_or_else(|| Error::new("STALE_ID", "session does not exist"))?;
        let (rows, cols) = (session.rows, session.cols);
        if let Some(name) = v.get("name").and_then(Value::as_str) {
            checked_text(name)?;
        }
        let w = id("window");
        let p = self.create_pane(&w, argv(v)?, rows, cols)?;
        let name = v
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("shell")
            .to_owned();
        self.windows.insert(
            w.clone(),
            Window {
                id: w.clone(),
                session: s.into(),
                name,
                panes: vec![p.clone()],
                active: p.clone(),
                layout: Layout::Leaf(p.clone()),
                zoom: false,
            },
        );
        let session = self.sessions.get_mut(s).unwrap();
        session.windows.push(w.clone());
        session.select_window(w.clone());
        Ok(json!({"window":w,"pane":p,"pid":self.panes[&p].pty.as_ref().map(|p|p.pid)}))
    }
    fn focused_pane(&self, client: &str, window: &Window) -> String {
        self.clients
            .get(client)
            .filter(|c| c.session == window.session)
            .and_then(|c| c.panes.get(&window.id))
            .filter(|p| window.panes.contains(p))
            .unwrap_or(&window.active)
            .clone()
    }
    fn focused_zoom(&self, client: &str, window: &Window) -> bool {
        self.clients
            .get(client)
            .filter(|c| c.session == window.session)
            .and_then(|c| c.zooms.get(&window.id))
            .copied()
            .unwrap_or(window.zoom)
    }
    fn select_client_window(&mut self, client: &str, session: &str, window: &str) {
        self.sessions
            .get_mut(session)
            .unwrap()
            .select_window(window.into());
        if let Some(c) = self
            .clients
            .get_mut(client)
            .filter(|c| c.session == session)
        {
            c.select_window(window.into());
        }
    }
    fn normalize_client_focus(&mut self, session: &str) {
        let Some(s) = self.sessions.get(session) else {
            return;
        };
        for c in self.clients.values_mut().filter(|c| c.session == session) {
            c.panes.retain(|w, _| s.windows.contains(w));
            c.zooms.retain(|w, _| s.windows.contains(w));
            if !s.windows.contains(&c.window) {
                c.window = c
                    .previous
                    .clone()
                    .filter(|w| s.windows.contains(w))
                    .unwrap_or_else(|| s.active.clone());
            }
            if c.previous
                .as_ref()
                .is_some_and(|w| !s.windows.contains(w) || w == &c.window)
            {
                c.previous = None;
            }
            for id in &s.windows {
                let w = &self.windows[id];
                let selected = c
                    .panes
                    .entry(id.clone())
                    .or_insert_with(|| w.active.clone());
                if !w.panes.contains(selected) {
                    *selected = w.active.clone();
                    c.zooms.insert(id.clone(), false);
                }
                c.zooms.entry(id.clone()).or_insert(w.zoom);
            }
        }
    }
    fn layout(&mut self, s: &str) {
        self.normalize_client_focus(s);
        let sizes: Vec<_> = self
            .clients
            .values()
            .filter(|c| c.session == s)
            .map(|c| (c.rows, c.cols))
            .collect();
        let Some(session) = self.sessions.get_mut(s) else {
            return;
        };
        if !sizes.is_empty() {
            session.rows = sizes.iter().map(|s| s.0).min().unwrap();
            session.cols = sizes.iter().map(|s| s.1).min().unwrap();
        }
        let rows = session.rows;
        let cols = session.cols;
        let windows = session.windows.clone();
        for w in windows {
            let Some(w) = self.windows.get(&w) else {
                continue;
            };
            let full = Rect {
                x: 0,
                y: 0,
                rows,
                cols,
            };
            let mut rects = Vec::new();
            let viewers: Vec<_> = self
                .clients
                .values()
                .filter(|c| c.session == s && c.window == w.id)
                .collect();
            if viewers.is_empty() {
                if w.zoom {
                    rects.push((w.active.clone(), full));
                } else {
                    w.layout.rects(full, &mut rects);
                }
            } else {
                for c in viewers {
                    if c.zooms.get(&w.id).copied().unwrap_or(w.zoom) {
                        rects.push((c.panes.get(&w.id).unwrap_or(&w.active).clone(), full));
                    } else {
                        w.layout.rects(full, &mut rects);
                    }
                }
            }
            let mut minimum: HashMap<String, Rect> = HashMap::new();
            for (id, rect) in rects {
                minimum
                    .entry(id)
                    .and_modify(|r| {
                        r.rows = r.rows.min(rect.rows);
                        r.cols = r.cols.min(rect.cols);
                    })
                    .or_insert(rect);
            }
            for (id, r) in minimum {
                if let Some(p) = self.panes.get_mut(&id) {
                    if p.term.rows != r.rows || p.term.cols != r.cols {
                        if let Some(pty) = &p.pty {
                            let _ = pty.resize(r.rows, r.cols);
                        }
                        p.term.resize(r.rows, r.cols);
                    }
                }
            }
        }
    }
    fn expire_histories(&mut self) {
        self.histories
            .retain(|_, pin| pin.read_at.elapsed() < std::time::Duration::from_secs(30));
    }
    fn expire_pastes(&mut self) {
        for c in self.clients.values_mut() {
            if c.paste.as_ref().is_some_and(paste::Upload::expired) {
                c.paste = None;
            }
        }
    }
    fn paste(&mut self, v: &Value, client: &str) -> Result<Value> {
        self.expire_pastes();
        let operation = text(v, "operation")?;
        if !self.clients.contains_key(client) {
            return Err(Error::new("NOT_ATTACHED", "attach first"));
        }
        if operation == "begin" {
            let pane = self.pane(v)?;
            pane.live_pty()?;
            if self.windows[&pane.window].session != self.clients[client].session {
                return Err(Error::new(
                    "TARGET_SESSION",
                    "pane belongs to another session",
                ));
            }
            let pane = pane.id.clone();
            let total = paste::size(v, "bytes")?;
            if self.clients.values().filter(|c| c.paste.is_some()).count() >= 16 {
                return Err(Error::new("LIMIT", "maximum sixteen staged pastes"));
            }
            let c = self.clients.get_mut(client).unwrap();
            if c.paste.is_some() {
                return Err(Error::new("PASTE_BUSY", "abort existing paste first"));
            }
            let token = id("paste");
            c.paste = Some(paste::Upload::new(token.clone(), pane, total));
            return Ok(json!({"token":token}));
        }
        let token = text(v, "token")?;
        let c = self.clients.get_mut(client).unwrap();
        let upload = c
            .paste
            .as_mut()
            .filter(|u| u.token == token)
            .ok_or_else(|| {
                Error::new(
                    "STALE_ID",
                    "paste absent, expired or owned by another connection",
                )
            })?;
        match operation.as_str() {
            "chunk" => {
                let offset = paste::size(v, "offset")?;
                upload.append(offset, &text(v, "text")?)?;
                Ok(json!({"staged":true}))
            }
            "abort" => {
                c.paste = None;
                Ok(json!({"aborted":true}))
            }
            "commit" => {
                let upload = c.paste.take().unwrap();
                let p = self
                    .panes
                    .get(&upload.pane)
                    .ok_or_else(|| Error::new("STALE_ID", "paste target closed"))?;
                let bytes = upload.finish(p.term.paste())?;
                let count = bytes.len();
                p.live_pty()?
                    .send(bytes)
                    .map_err(|e| Error::new("BACKPRESSURE", e))?;
                Ok(json!({"queued":true,"bytes":count}))
            }
            _ => Err(Error::new("INVALID_PARAMS", "unknown paste operation")),
        }
    }
    fn dispatch(&mut self, m: &str, v: &Value, client: &str) -> Result<Value> {
        match m {
            "health" => Ok(
                json!({"ok":true,"panes":self.panes.len(),"clients":self.clients.len(),"revision":self.revision,"history_snapshots":self.histories.len(),"history_snapshot_text_bytes":self.histories.values().map(|p|p.data.bytes).sum::<usize>(),"persistence":{"enabled":self.store.is_some(),"checkpoint_revision":self.checkpoint_revision,"pending":self.checkpoint_pending,"error":self.checkpoint_error,"interval_ms":2000}}),
            ),
            "version" => Ok(
                json!({"name":"ruwac","implementation":"spike","package":env!("CARGO_PKG_VERSION"),"protocol":0}),
            ),
            "capabilities" => Ok(
                json!({"terminal_size_limits":ruwac_protocol::TerminalLimits::CURRENT,"max_render_response_bytes":ruwac_protocol::MAX_RENDER_FRAME,"job_metadata":true,"job_metadata_no_argv":true,"structured_agent_state":false,"recovery":self.store.is_some(),"primary_buffer_history":true,"extended_terminal_styles":true,"kitty_keyboard_input":false,"history_checkpoints":"private canonical active and hidden primary tail, 50000 lines or 512 KiB per pane; processes restore ended","desktop_clipboard":false,"primary":false,"graphics":false,"image_attachment":true,"image_attachment_adapters":["codex draft path"],"image_attachment_submits":false,"ssh_transport":"authenticated OpenSSH through CLI --host","shared_focus":false,"independent_client_focus":true,"pane_divider_cells":true,"client_zoom":true,"atomic_paste":true,"max_paste_bytes":paste::MAX_BYTES,"pty_sizing":"minimum session dimensions and minimum visible pane size across attached clients","split":"persistent horizontal/vertical tree, sixteen panes","capture_limit":2000,"history_page_limit":500,"scrollback_default":50000,"max_text_bytes":ruwac_protocol::MAX_TEXT}),
            ),
            "schema" => Ok(schema()),
            "describe" => Ok(
                json!({"protocol":0,"native_describe":true,"schema":schema(),"capabilities":self.dispatch("capabilities",v,client)?,"version":self.dispatch("version",v,client)?,"transport":{"local":"same-UID authenticated Unix socket","remote":"OpenSSH --host, explicit targets, verified host key","gui":"independent optional service; not owned by the headless daemon"}}),
            ),
            "daemon.stop" | "daemon.checkpoint" => Err(Error::new(
                "INVALID_PARAMS",
                "checkpoint/stop do not accept idempotency keys",
            )),
            "session.create" => {
                if self.sessions.len() >= 64 {
                    return Err(Error::new("LIMIT", "maximum 64 sessions"));
                }
                let s = id("session");
                let name = text(v, "name")?;
                checked_text(&name)?;
                let (rows, cols) = terminal_dimensions(v)?;
                self.sessions.insert(
                    s.clone(),
                    Session {
                        id: s.clone(),
                        name,
                        windows: vec![],
                        active: String::new(),
                        rows,
                        cols,
                        buffer: String::new(),
                        previous: None,
                    },
                );
                match self.create_window(&s, v) {
                    Ok(w) => Ok(
                        json!({"session":s,"window":w["window"],"pane":w["pane"],"pid":w["pid"]}),
                    ),
                    Err(e) => {
                        self.sessions.remove(&s);
                        Err(e)
                    }
                }
            }
            "session.list" => {
                let mut a:Vec<_>=self.sessions.values().map(|s|json!({"id":s.id,"name":s.name,"active_window":self.clients.get(client).filter(|c| c.session==s.id).map(|c|&c.window).unwrap_or(&s.active),"windows":s.windows,"rows":s.rows,"cols":s.cols,"previous_window":self.clients.get(client).filter(|c| c.session==s.id).map(|c|&c.previous).unwrap_or(&s.previous),"restored":s.windows.iter().filter_map(|w|self.windows.get(w)).flat_map(|w|w.panes.iter()).all(|p|self.panes.get(p).is_some_and(|p|p.pty.is_none()))})).collect();
                a.sort_by_key(|v| v["id"].as_str().unwrap().to_owned());
                Ok(json!(a))
            }
            "session.attach" => {
                let s = target(v, "session", "session")?;
                if !self.sessions.contains_key(&s) {
                    return Err(Error::new("STALE_ID", "session does not exist"));
                }
                let (rows, cols) = terminal_dimensions(v)?;
                if let Some(old) = self.clients.remove(client) {
                    self.layout(&old.session);
                }
                self.clients.insert(
                    client.into(),
                    Client {
                        render_stamp: None,
                        paste: None,
                        scope: id("display"),
                        session: s.clone(),
                        rows,
                        cols,
                        window: self.sessions[&s].active.clone(),
                        previous: self.sessions[&s].previous.clone(),
                        panes: self.sessions[&s]
                            .windows
                            .iter()
                            .map(|w| (w.clone(), self.windows[w].active.clone()))
                            .collect(),
                        zooms: self.sessions[&s]
                            .windows
                            .iter()
                            .map(|w| (w.clone(), self.windows[w].zoom))
                            .collect(),
                    },
                );
                self.layout(&s);
                Ok(json!({"client":client,"session":s}))
            }
            "session.detach" => {
                self.histories.retain(|_, pin| pin.owner != client);
                if let Some(c) = self.clients.remove(client) {
                    self.layout(&c.session);
                }
                Ok(json!({"detached":true}))
            }
            "window.create" => {
                let s = target(v, "session", "session")?;
                let result = self.create_window(&s, v)?;
                self.select_client_window(client, &s, result["window"].as_str().unwrap());
                self.layout(&s);
                Ok(result)
            }
            "window.list" => {
                let s = target(v, "session", "session")?;
                let s = self
                    .sessions
                    .get(&s)
                    .ok_or_else(|| Error::new("STALE_ID", "session does not exist"))?;
                Ok(json!(s.windows.iter().filter_map(|w|self.windows.get(w)).map(|w|json!({"id":w.id,"name":w.name,"panes":w.panes,"active_pane":self.focused_pane(client,w)})).collect::<Vec<_>>()))
            }
            "window.rename" => {
                let w = target(v, "window", "window")?;
                let name = text(v, "name")?;
                checked_text(&name)?;
                let w = self
                    .windows
                    .get_mut(&w)
                    .ok_or_else(|| Error::new("STALE_ID", "window does not exist"))?;
                w.name = name;
                Ok(json!({"renamed":true}))
            }
            "window.select" => {
                let w = target(v, "window", "window")?;
                let w = self
                    .windows
                    .get(&w)
                    .ok_or_else(|| Error::new("STALE_ID", "window does not exist"))?;
                let (session, window) = (w.session.clone(), w.id.clone());
                self.select_client_window(client, &session, &window);
                self.layout(&session);
                Ok(json!({"selected":window}))
            }
            "window.previous" | "window.cycle" => {
                let session = target(v, "session", "session")?;
                let s = self
                    .sessions
                    .get(&session)
                    .ok_or_else(|| Error::new("STALE_ID", "session does not exist"))?;
                let c = self.clients.get(client).filter(|c| c.session == session);
                let active = c.map(|c| &c.window).unwrap_or(&s.active);
                let index = s.windows.iter().position(|w| w == active).unwrap_or(0);
                let previous = c
                    .map(|c| c.previous.as_ref())
                    .unwrap_or(s.previous.as_ref());
                let selected = if m == "window.previous" {
                    previous
                        .filter(|id| s.windows.contains(id))
                        .cloned()
                        .unwrap_or_else(|| s.windows[(index + 1) % s.windows.len()].clone())
                } else {
                    let n = s.windows.len();
                    let j = if v.get("previous").and_then(Value::as_bool).unwrap_or(false) {
                        (index + n - 1) % n
                    } else {
                        (index + 1) % n
                    };
                    s.windows[j].clone()
                };
                self.select_client_window(client, &session, &selected);
                self.layout(&session);
                Ok(json!({"selected":selected}))
            }
            "pane.create" => {
                let w = target(v, "window", "window")?;
                let window = self
                    .windows
                    .get(&w)
                    .ok_or_else(|| Error::new("STALE_ID", "window does not exist"))?;
                if window.panes.len() >= 16 {
                    return Err(Error::new("LIMIT", "maximum sixteen panes per window"));
                }
                let orientation = v
                    .get("orientation")
                    .and_then(Value::as_str)
                    .unwrap_or("horizontal");
                if !matches!(orientation, "horizontal" | "vertical") {
                    return Err(Error::new(
                        "INVALID_PARAMS",
                        "orientation must be horizontal or vertical",
                    ));
                }
                let focused = self.focused_pane(client, window);
                let selected = v
                    .get("pane")
                    .and_then(Value::as_str)
                    .unwrap_or(&focused)
                    .to_string();
                if !window.panes.contains(&selected) {
                    return Err(Error::new("TARGET_WINDOW", "split target is not in window"));
                }
                let source = &self.panes[&selected];
                if (orientation == "vertical" && source.term.cols < 12)
                    || (orientation == "horizontal" && source.term.rows < 6)
                {
                    return Err(Error::new("LIMIT", "pane too small to split"));
                }
                let s = window.session.clone();
                let ss = &self.sessions[&s];
                let p = self.create_pane(&w, argv(v)?, ss.rows / 2, ss.cols)?;
                let w = self.windows.get_mut(&w).unwrap();
                w.layout
                    .split(&selected, p.clone(), orientation == "vertical");
                w.zoom = false;
                w.panes.push(p.clone());
                w.active = p.clone();
                if let Some(c) = self.clients.get_mut(client).filter(|c| c.session == s) {
                    c.panes.insert(w.id.clone(), p.clone());
                    c.zooms.insert(w.id.clone(), false);
                }
                self.layout(&s);
                Ok(json!({"pane":p,"pid":self.panes[&p].pty.as_ref().map(|p|p.pid)}))
            }
            "job.list" => self.job_list(v),
            "job.query" => self.job_query(v),
            "agent.probe" => self.agent_probe(v),
            "pane.attachImage" => self.attach_image(v),
            "pane.list" => {
                let s = target(v, "session", "session")?;
                let s = self
                    .sessions
                    .get(&s)
                    .ok_or_else(|| Error::new("STALE_ID", "session does not exist"))?;
                Ok(json!(s.windows.iter().filter_map(|w|self.windows.get(w)).flat_map(|w|w.panes.iter()).filter_map(|p|self.panes.get(p)).map(|p|json!({"id":p.id,"window":p.window,"pid":p.pty.as_ref().map(|p|p.pid),"ended":p.ended,"restored":p.pty.is_none(),"exit_code":p.exit_code.or_else(||p.pty.as_ref().and_then(Pty::exit_code)),"rows":p.term.rows,"cols":p.term.cols})).collect::<Vec<_>>()))
            }
            "pane.close" => {
                let p = self.pane(v)?;
                if v.get("if_ended").is_some_and(|value| !value.is_boolean()) {
                    return Err(Error::new("INVALID_REQUEST", "if_ended must be a boolean"));
                }
                if v["if_ended"] == true && !p.ended && p.pty.is_some() {
                    return Err(Error::new("PROCESS_RUNNING", "Live terminal was preserved"));
                }
                let pid = p.id.clone();
                let w = p.window.clone();
                let session = self.windows[&w].session.clone();
                self.panes.remove(&pid);
                let window = self.windows.get_mut(&w).unwrap();
                window.panes.retain(|p| p != &pid);
                if let Some(l) = window.layout.clone().remove(&pid) {
                    window.layout = l;
                }
                window.zoom = false;
                if let Some(first) = window.panes.first() {
                    if window.active == pid {
                        window.active = first.clone();
                    }
                } else {
                    self.windows.remove(&w);
                    let ss = self.sessions.get_mut(&session).unwrap();
                    ss.windows.retain(|x| x != &w);
                    if ss.previous.as_deref() == Some(w.as_str()) {
                        ss.previous = None;
                    }
                    if let Some(first) = ss.windows.first() {
                        if ss.active == w {
                            ss.select_window(first.clone());
                            ss.previous = None;
                        }
                    } else {
                        self.sessions.remove(&session);
                        self.clients.retain(|_, c| c.session != session);
                    }
                }
                self.layout(&session);
                Ok(json!({"closed":pid}))
            }
            "pane.select" => {
                let p = self.pane(v)?;
                let id = p.id.clone();
                let w = p.window.clone();
                self.windows.get_mut(&w).unwrap().active = id.clone();
                let session = self.windows[&w].session.clone();
                if let Some(c) = self
                    .clients
                    .get_mut(client)
                    .filter(|c| c.session == session)
                {
                    c.panes.insert(w, id.clone());
                }
                self.layout(&session);
                Ok(json!({"selected":id}))
            }
            "pane.layout" => {
                let id = target(v, "window", "window")?;
                let w = self
                    .windows
                    .get_mut(&id)
                    .ok_or_else(|| Error::new("STALE_ID", "window does not exist"))?;
                if let Some(zoom) = v.get("zoom").and_then(Value::as_bool) {
                    w.zoom = zoom;
                    if let Some(c) = self
                        .clients
                        .get_mut(client)
                        .filter(|c| c.session == w.session)
                    {
                        c.zooms.insert(id.clone(), zoom);
                    }
                }
                if let Some(split) = v.get("split").and_then(Value::as_str) {
                    let ratio = num(v, "ratio", 500, 100, 900)?;
                    if !w.layout.resize(split, ratio) {
                        return Err(Error::new("STALE_ID", "split does not exist"));
                    }
                }
                let session = w.session.clone();
                self.layout(&session);
                Ok(json!({"updated":true}))
            }
            "pane.history.freeze" => {
                self.expire_histories();
                if self
                    .histories
                    .values()
                    .filter(|pin| pin.owner != client)
                    .count()
                    >= 8
                {
                    return Err(Error::new(
                        "LIMIT",
                        "eight simultaneous copy snapshots; retry after another copy finishes",
                    ));
                }
                let p = self.pane(v)?;
                let pane = p.id.clone();
                let data = if primary_buffer(v)? {
                    p.term.freeze_primary_history().ok_or_else(|| {
                        Error::new(
                            "BUFFER_UNAVAILABLE",
                            "Legacy alternate-screen checkpoint has no primary buffer",
                        )
                    })?
                } else {
                    p.term.freeze_history()
                };
                if data.rows.is_empty() {
                    return Err(Error::new(
                        "LIMIT",
                        "newest history row exceeds the bounded copy snapshot row cap",
                    ));
                }
                self.histories.retain(|_, pin| pin.owner != client);
                let snapshot = id("history");
                let mut metadata = data.metadata();
                metadata["snapshot"] = json!(snapshot);
                self.histories.insert(
                    snapshot,
                    HistoryPin {
                        owner: client.into(),
                        pane,
                        read_at: std::time::Instant::now(),
                        data,
                    },
                );
                Ok(metadata)
            }
            "pane.history.release" => {
                let snapshot = target(v, "snapshot", "history")?;
                if !self
                    .histories
                    .get(&snapshot)
                    .is_some_and(|pin| pin.owner == client)
                {
                    return Err(Error::new(
                        "STALE_SNAPSHOT",
                        "copy snapshot is not owned by this connection",
                    ));
                }
                self.histories.remove(&snapshot);
                Ok(json!({"released":true}))
            }
            "pane.history" => {
                if v.get("snapshot").is_some() {
                    self.expire_histories();
                    let snapshot = target(v, "snapshot", "history")?;
                    let pane = target(v, "pane", "pane")?;
                    let pin=self.histories.get_mut(&snapshot).filter(|pin|pin.owner==client&&pin.pane==pane)
                        .ok_or_else(||Error::new("STALE_SNAPSHOT","copy snapshot expired, released or belongs to another connection/pane"))?;
                    if v.get("buffer").is_some()
                        && primary_buffer(v)? != (pin.data.buffer == "primary")
                    {
                        return Err(Error::new(
                            "INVALID_PARAMS",
                            "Buffer selector differs from frozen snapshot",
                        ));
                    }
                    pin.read_at = std::time::Instant::now();
                    let offset = v
                        .get("offset")
                        .map(|x| {
                            x.as_u64().ok_or_else(|| {
                                Error::new("INVALID_PARAMS", "offset must be an integer")
                            })
                        })
                        .transpose()?
                        .unwrap_or(0)
                        .min(1000000) as usize;
                    let lines = num(v, "lines", 500, 1, 500)? as usize;
                    return Ok(pin.data.page(offset, lines));
                }
                let p = self.pane(v)?;
                if v.get("epoch")
                    .and_then(Value::as_u64)
                    .is_some_and(|e| e != p.term.history_epoch)
                {
                    return Err(Error::new(
                        "STALE_HISTORY",
                        "History changed while paging; retry copy mode",
                    ));
                }
                let offset = v
                    .get("offset")
                    .and_then(Value::as_u64)
                    .unwrap_or(0)
                    .min(1000000) as usize;
                let n = num(v, "lines", 500, 1, 500)? as usize;
                if primary_buffer(v)? {
                    p.term.primary_history_page(offset, n).ok_or_else(|| {
                        Error::new(
                            "BUFFER_UNAVAILABLE",
                            "Legacy alternate-screen checkpoint has no primary buffer",
                        )
                    })
                } else {
                    Ok(p.term.history_page(offset, n))
                }
            }
            "pane.capture" | "pane.wait" => {
                let p = self.pane(v)?;
                let n = num(v, "lines", 250, 1, 2000)?;
                let primary = primary_buffer(v)?;
                let t = if primary {
                    p.term.capture_primary(n as usize).ok_or_else(|| {
                        Error::new(
                            "BUFFER_UNAVAILABLE",
                            "Legacy alternate-screen checkpoint has no primary buffer",
                        )
                    })?
                } else {
                    p.term.capture(n as usize)
                };
                let matched = v
                    .get("contains")
                    .and_then(Value::as_str)
                    .map(|s| t.contains(s));
                Ok(
                    json!({"pane":p.id,"text":t,"lines":t.lines().count(),"matched":matched,"ended":p.ended,"buffer":if primary {"primary"}else{"active"}}),
                )
            }
            "pane.sendText" => {
                let t = text(v, "text")?;
                checked_text(&t)?;
                let p = self.pane(v)?;
                if p.ended {
                    return Err(Error::new("PANE_EXITED", "child exited"));
                }
                p.live_pty()?
                    .send(t.as_bytes().to_vec())
                    .map_err(|e| Error::new("BACKPRESSURE", e))?;
                Ok(json!({"queued_bytes":t.len()}))
            }
            "pane.sendKey" => {
                let key = text(v, "key")?;
                let b = match key.as_str() {
                    "Enter" => b"\r".to_vec(),
                    "Tab" => b"\t".to_vec(),
                    "Escape" => b"\x1b".to_vec(),
                    "CtrlA" => vec![1],
                    "CtrlC" => vec![3],
                    "Up" => b"\x1b[A".to_vec(),
                    "Down" => b"\x1b[B".to_vec(),
                    _ => {
                        return Err(Error::new(
                            "INVALID_KEY",
                            "Enter, Tab, Escape, CtrlA, CtrlC, Up, Down supported",
                        ));
                    }
                };
                let p = self.pane(v)?;
                if p.ended {
                    return Err(Error::new("PANE_EXITED", "child exited"));
                }
                p.live_pty()?
                    .send(b)
                    .map_err(|e| Error::new("BACKPRESSURE", e))?;
                Ok(json!({"queued":true}))
            }
            "buffer.set" => {
                let s = target(v, "session", "session")?;
                let t = text(v, "text")?;
                checked_text(&t)?;
                let s = self
                    .sessions
                    .get_mut(&s)
                    .ok_or_else(|| Error::new("STALE_ID", "session does not exist"))?;
                s.buffer = t;
                Ok(json!({"stored":true}))
            }
            "buffer.get" => {
                let s = target(v, "session", "session")?;
                let s = self
                    .sessions
                    .get(&s)
                    .ok_or_else(|| Error::new("STALE_ID", "session does not exist"))?;
                Ok(json!({"text":s.buffer}))
            }
            // OSC 52 PRIMARY from a program (e.g. a nested RUWAC client's mouse
            // selection). The desktop fetches/answers only behind a user gesture.
            "pane.selection" => {
                let p = self.pane(v)?;
                let serial = v["serial"]
                    .as_u64()
                    .ok_or_else(|| Error::new("INVALID_PARAMS", "serial must be an integer"))?;
                let text = p
                    .term
                    .selection_offer(serial)
                    .ok_or_else(|| Error::new("STALE_ID", "selection offer replaced"))?;
                Ok(json!({"text":text}))
            }
            "pane.selectionReply" => {
                let id = target(v, "pane", "pane")?;
                let serial = v["serial"]
                    .as_u64()
                    .ok_or_else(|| Error::new("INVALID_PARAMS", "serial must be an integer"))?;
                let t = text(v, "text")?;
                if t.len() > ruwac_term::MAX_SELECTION || t.contains('\0') {
                    return Err(Error::new(
                        "LIMIT",
                        "selection exceeds 128 KiB or contains NUL",
                    ));
                }
                let p = self
                    .panes
                    .get_mut(&id)
                    .ok_or_else(|| Error::new("STALE_ID", "pane does not exist"))?;
                let reply = p
                    .term
                    .selection_reply(serial, &t)
                    .ok_or_else(|| Error::new("STALE_ID", "selection request replaced"))?;
                p.live_pty()?
                    .send(reply.into_bytes())
                    .map_err(|e| Error::new("BACKPRESSURE", e))?;
                Ok(json!({"queued":true}))
            }
            "buffer.paste" => {
                let p = self.pane(v)?;
                let w = &self.windows[&p.window];
                let t = self.sessions[&w.session].buffer.clone();
                let mut b = t.into_bytes();
                if p.term.paste() {
                    let mut framed = b"\x1b[200~".to_vec();
                    framed.extend(b);
                    framed.extend(b"\x1b[201~");
                    b = framed;
                }
                p.live_pty()?
                    .send(b)
                    .map_err(|e| Error::new("BACKPRESSURE", e))?;
                Ok(json!({"queued":true}))
            }
            "client.resize" => {
                let (rows, cols) = terminal_dimensions(v)?;
                let c = self
                    .clients
                    .get_mut(client)
                    .ok_or_else(|| Error::new("NOT_ATTACHED", "attach first"))?;
                c.rows = rows;
                c.cols = cols;
                let s = c.session.clone();
                self.layout(&s);
                Ok(json!({"resized":true}))
            }
            "client.render" => {
                for p in self.panes.values_mut() {
                    let group = p.pty.as_ref().and_then(Pty::foreground_group);
                    if group != p.foreground {
                        p.term.reset_input_modes();
                        p.foreground = group;
                        self.revision += 1;
                    }
                }
                let c = self
                    .clients
                    .get(client)
                    .ok_or_else(|| Error::new("NOT_ATTACHED", "attach first"))?;
                let s = &self.sessions[&c.session];
                let w = &self.windows[&c.window];
                let active = self.focused_pane(client, w);
                let zoom = self.focused_zoom(client, w);
                if v.get("revision").and_then(Value::as_u64) == Some(self.revision) {
                    return Ok(json!({"revision":self.revision,"unchanged":true}));
                }
                let mut rects = Vec::new();
                if zoom {
                    rects.push((
                        active.clone(),
                        Rect {
                            x: 0,
                            y: 0,
                            rows: s.rows,
                            cols: s.cols,
                        },
                    ));
                } else {
                    w.layout.rects(
                        Rect {
                            x: 0,
                            y: 0,
                            rows: s.rows,
                            cols: s.cols,
                        },
                        &mut rects,
                    );
                }
                // This is deliberately metadata-only: no cell matrix is built
                // or retained for a quiet view when a hidden pane emits output.
                // Include every field of the projected frame and each visible
                // terminal's epoch, foreground identity and selection serials.
                let stamp = json!({
                    "session":s.id,"session_name":s.name,"window":w.id,
                    "window_index":s.windows.iter().position(|id|id==&w.id),
                    "windows":s.windows.iter().filter_map(|id|self.windows.get(id)).enumerate().map(|(index,w)|json!({"id":w.id,"index":index,"name":w.name})).collect::<Vec<_>>(),
                    "name":w.name,"rows":s.rows,"cols":s.cols,"active_pane":active,
                    "layout":w.layout.json(),"zoom":zoom,
                    "panes":rects.iter().filter_map(|(id,r)|self.panes.get(id).map(|p|json!({
                        "id":p.id,"epoch":p.term.history_epoch,"foreground":p.foreground,
                        "selection":p.term.selection_serials(),"ended":p.ended,
                        "restored":p.pty.is_none(),"exit_code":p.exit_code.or_else(||p.pty.as_ref().and_then(Pty::exit_code)),
                        "rect":{"x":r.x,"y":r.y,"rows":r.rows,"cols":r.cols}
                    }))).collect::<Vec<_>>()
                });
                if c.render_stamp.as_ref().is_some_and(|(base, previous)| {
                    v.get("revision")
                        .and_then(Value::as_u64)
                        .is_some_and(|requested| {
                            requested >= *base && requested <= self.revision && previous == &stamp
                        })
                }) {
                    return Ok(json!({"revision":self.revision,"unchanged":true}));
                }
                let panes=rects.iter().filter_map(|(id,r)|self.panes.get(id).map(|p|json!({"id":p.id,"grid":p.term.snapshot(),"selection_offer":p.term.selection_serials().0,"selection_request":p.term.selection_serials().1,"ended":p.ended,"restored":p.pty.is_none(),"exit_code":p.exit_code.or_else(||p.pty.as_ref().and_then(Pty::exit_code)),"rect":{"x":r.x,"y":r.y,"rows":r.rows,"cols":r.cols}}))).collect::<Vec<_>>();
                let frame = json!({"revision":self.revision,"session":s.id,"session_name":s.name,"window":w.id,"window_index":s.windows.iter().position(|id|id==&w.id),"windows":s.windows.iter().filter_map(|id|self.windows.get(id)).enumerate().map(|(index,w)|json!({"id":w.id,"index":index,"name":w.name})).collect::<Vec<_>>(),"name":w.name,"rows":s.rows,"cols":s.cols,"active_pane":active,"layout":w.layout.json(),"zoom":zoom,"panes":panes});
                self.clients.get_mut(client).unwrap().render_stamp = Some((self.revision, stamp));
                Ok(frame)
            }

            "client.paste" => self.paste(v, client),
            "client.input" => {
                let p = self.pane(v)?;
                let c = self
                    .clients
                    .get(client)
                    .ok_or_else(|| Error::new("NOT_ATTACHED", "attach first"))?;
                if self.windows[&p.window].session != c.session {
                    return Err(Error::new(
                        "TARGET_SESSION",
                        "pane belongs to another session",
                    ));
                }
                let kind = text(v, "kind")?;
                let t = text(v, "text")?;
                checked_text(&t)?;
                let b = match kind.as_str() {
                    "mouse" if !p.term.mouse() => return Ok(json!({"discarded":"mouse disabled"})),
                    "mouse" if v.get("mouse_event").is_some() => {
                        let event = &v["mouse_event"];
                        let button = num(event, "button", 0, 0, 127)?;
                        let x = num(event, "x", 1, 1, p.term.cols as u64)?;
                        let y = num(event, "y", 1, 1, p.term.rows as u64)?;
                        let release = event["release"] == true;
                        if p.term.sgr_mouse() {
                            format!("\x1b[<{button};{x};{y}{}", if release { 'm' } else { 'M' })
                                .into_bytes()
                        } else {
                            if x > 223 || y > 223 {
                                return Ok(json!({"discarded":"legacy mouse coordinate overflow"}));
                            }
                            vec![
                                27,
                                b'[',
                                b'M',
                                (if release { 3 + (button & 28) } else { button }) as u8 + 32,
                                x as u8 + 32,
                                y as u8 + 32,
                            ]
                        }
                    }
                    "mouse" | "keyboard" => t.into_bytes(),
                    "paste" => {
                        if t.contains("\x1b[201~") {
                            return Err(Error::new(
                                "INVALID_TEXT",
                                "paste contains embedded end marker",
                            ));
                        }
                        if p.term.paste() {
                            format!("\x1b[200~{t}\x1b[201~").into_bytes()
                        } else {
                            t.into_bytes()
                        }
                    }
                    _ => {
                        return Err(Error::new(
                            "INVALID_PARAMS",
                            "kind must be keyboard, mouse or paste",
                        ));
                    }
                };
                if p.ended {
                    return Err(Error::new("PANE_EXITED", "child exited"));
                }
                p.live_pty()?
                    .send(b)
                    .map_err(|e| Error::new("BACKPRESSURE", e))?;
                Ok(json!({"queued":true}))
            }
            _ => Err(Error::new(
                "METHOD_NOT_FOUND",
                format!("unknown method {m}"),
            )),
        }
    }
}
pub fn schema() -> Value {
    json!({"protocol":0,"framing":"u32 big-endian byte length + UTF-8 JSON","max_frame_bytes":ruwac_protocol::MAX_FRAME,"max_render_response_bytes":ruwac_protocol::MAX_RENDER_FRAME,"terminal_size_limits":ruwac_protocol::TerminalLimits::CURRENT,"methods":["job.list","job.query","agent.probe","health","version","capabilities","schema","describe","session.create","session.list","session.attach","session.detach","window.create","window.list","window.rename","window.select","window.cycle","window.previous","pane.create","pane.layout","pane.history","pane.history.freeze","pane.history.release","pane.close","pane.list","pane.select","pane.attachImage","pane.sendText","pane.sendKey","pane.capture","pane.wait","pane.selection","pane.selectionReply","buffer.set","buffer.get","buffer.paste","client.resize","client.render","client.input","client.paste","events.subscribe","daemon.checkpoint","daemon.stop"],"targets":{"session":"session_ + 32 hex","window":"window_ + 32 hex","pane":"pane_ + 32 hex"},"request":{"version":0,"id":"JSON request correlation value","method":"method name","params":"object","idempotency_key":"optional 1..128 byte daemon-lifetime key"}})
}

fn primary_buffer(v: &Value) -> std::result::Result<bool, Error> {
    match v.get("buffer") {
        None => Ok(false),
        Some(Value::String(value)) if value == "active" => Ok(false),
        Some(Value::String(value)) if value == "primary" => Ok(true),
        _ => Err(Error::new(
            "INVALID_PARAMS",
            "buffer must be active or primary",
        )),
    }
}
