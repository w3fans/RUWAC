//! Same-UID owned PTY process metadata; no argv/environment scans or agent inference.
use crate::{Pane, State};
use ruwac_protocol::{Error, Result, target};
use serde_json::{Value, json};
use std::{
    fs,
    os::unix::fs::MetadataExt,
    time::{Instant, SystemTime, UNIX_EPOCH},
};
fn epoch_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64
}
fn bounded(s: &str, limit: usize) -> String {
    s.chars().filter(|c| !c.is_control()).take(limit).collect()
}
struct Process {
    start: u64,
    session: u32,
    command: String,
    cwd: Option<String>,
}
fn process(pid: u32) -> Option<Process> {
    let root = format!("/proc/{pid}");
    if fs::metadata(&root).ok()?.uid() != unsafe { libc::geteuid() } {
        return None;
    }
    let stat = fs::read_to_string(format!("{root}/stat")).ok()?;
    let (_, tail) = stat.rsplit_once(')')?;
    let fields: Vec<_> = tail.split_whitespace().collect();
    let session = fields.get(3)?.parse().ok()?;
    let start = fields.get(19)?.parse().ok()?;
    let command = fs::read_link(format!("{root}/exe"))
        .ok()
        .and_then(|p| p.file_name().map(|p| bounded(&p.to_string_lossy(), 256)))
        .or_else(|| {
            fs::read_to_string(format!("{root}/comm"))
                .ok()
                .map(|s| bounded(&s, 256))
        })?;
    let cwd = fs::read_link(format!("{root}/cwd"))
        .ok()
        .map(|p| bounded(&p.to_string_lossy(), 1024));
    // Check the kernel start token again before accepting any PID-derived data.
    let second = fs::read_to_string(format!("{root}/stat")).ok()?;
    if second
        .rsplit_once(')')?
        .1
        .split_whitespace()
        .nth(19)?
        .parse::<u64>()
        .ok()?
        != start
    {
        return None;
    }
    Some(Process {
        start,
        session,
        command,
        cwd,
    })
}
pub(super) struct Job {
    command: String,
    host: String,
    started: Instant,
    started_ms: u64,
    start_token: Option<u64>,
    session: Option<u32>,
    cwd: Option<String>,
    last_output: Option<(Instant, u64)>,
}
impl Job {
    pub(super) fn new(command: &str, pid: u32) -> Self {
        let proc = process(pid);
        let mut hostname = [0u8; 256];
        let host = if unsafe { libc::gethostname(hostname.as_mut_ptr().cast(), hostname.len()) }
            == 0
        {
            bounded(
                &String::from_utf8_lossy(hostname.split(|b| *b == 0).next().unwrap_or_default()),
                128,
            )
        } else {
            "local".into()
        };
        Self {
            command: bounded(
                std::path::Path::new(command)
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .as_ref(),
                256,
            ),
            host,
            started: Instant::now(),
            started_ms: epoch_ms(),
            start_token: proc.as_ref().map(|p| p.start),
            session: proc.as_ref().map(|p| p.session),
            cwd: proc.and_then(|p| p.cwd),
            last_output: None,
        }
    }
    pub(super) fn output(&mut self) {
        self.last_output = Some((Instant::now(), epoch_ms()));
    }
}
fn metadata(p: &Pane, state: &State) -> Value {
    let window = &state.windows[&p.window];
    let session = &state.sessions[&window.session];
    let code = p
        .exit_code
        .or_else(|| p.pty.as_ref().and_then(ruwac_pty::Pty::exit_code));
    let phase = if p.pty.is_none() {
        "restored_history"
    } else if code.is_some() {
        "exited"
    } else if p.ended {
        "ended"
    } else {
        "running"
    };
    let mut result = json!({"id":p.id,"pane":p.id,"window":p.window,"session":session.id,"session_name":bounded(&session.name,256),"window_name":bounded(&window.name,256),"pid":p.pty.as_ref().map(|p|p.pid),"phase":phase,"pane_ended":p.ended,"exit_code":code,"selected":session.active==window.id&&window.active==p.id,"agent_state":"unknown","command":null,"host":null,"cwd":null,"started_ms":null,"age_ms":null,"last_output_ms":null,"last_output_age_ms":null,"foreground":null,"process_metadata_limit":{"command_chars":256,"path_chars":1024,"name_chars":256,"argv":false,"environment":false}});
    if let Some(job) = &p.job {
        let root = p
            .pty
            .as_ref()
            .filter(|_| code.is_none() && !p.ended)
            .and_then(|pty| process(pty.pid))
            .filter(|proc| {
                Some(proc.start) == job.start_token && Some(proc.session) == job.session
            });
        result["command"] = json!(
            root.as_ref()
                .map(|p| p.command.as_str())
                .unwrap_or(&job.command)
        );
        result["host"] = json!(job.host);
        result["cwd"] = json!(
            root.as_ref()
                .and_then(|p| p.cwd.as_ref())
                .or(job.cwd.as_ref())
        );
        result["started_ms"] = json!(job.started_ms);
        result["age_ms"] = json!(job.started.elapsed().as_millis().min(u64::MAX as u128) as u64);
        if let Some((instant, epoch)) = job.last_output {
            result["last_output_ms"] = json!(epoch);
            result["last_output_age_ms"] =
                json!(instant.elapsed().as_millis().min(u64::MAX as u128) as u64);
        }
        if root.is_some() {
            if let Some(proc) = p
                .pty
                .as_ref()
                .and_then(|pty| pty.foreground_group())
                .filter(|pid| *pid > 0)
                .and_then(|pid| process(pid as u32).map(|p| (pid, p)))
                .filter(|(_, p)| Some(p.session) == job.session)
            {
                result["foreground"] =
                    json!({"pid":proc.0,"command":proc.1.command,"cwd":proc.1.cwd});
            }
        }
    }
    result
}
impl State {
    pub(super) fn job_query(&self, v: &Value) -> Result<Value> {
        let object = v
            .as_object()
            .ok_or_else(|| Error::new("INVALID_PARAMS", "job query requires a pane target"))?;
        if object.len() != 1 || !object.contains_key("pane") {
            return Err(Error::new("INVALID_PARAMS", "job query takes only pane"));
        }
        let id = target(v, "pane", "pane")?;
        let p = self
            .panes
            .get(&id)
            .ok_or_else(|| Error::new("STALE_ID", "pane does not exist"))?;
        Ok(metadata(p, self))
    }
    pub(super) fn agent_probe(&self, v: &Value) -> Result<Value> {
        let job = self.job_query(v)?;
        let pid = job["foreground"]["pid"].as_u64();
        let agent = pid
            .and_then(|p| u32::try_from(p).ok())
            .and_then(ruwac_pty::attachment::foreground_agent);
        Ok(
            json!({"pane":job["pane"],"adapter":agent.map_or("codex", |a| a.name()),"foreground_pid":pid,"process_detected":agent.is_some(),"selected":job["selected"]}),
        )
    }
    pub(super) fn job_list(&self, v: &Value) -> Result<Value> {
        let empty = serde_json::Map::new();
        let object = if v.is_null() {
            &empty
        } else {
            v.as_object().ok_or_else(|| {
                Error::new("INVALID_PARAMS", "job list parameters must be an object")
            })?
        };
        if object
            .keys()
            .any(|k| !matches!(k.as_str(), "session" | "query" | "offset" | "limit"))
        {
            return Err(Error::new("INVALID_PARAMS", "unknown job list parameter"));
        }
        let session = if object.contains_key("session") {
            let id = target(v, "session", "session")?;
            if !self.sessions.contains_key(&id) {
                return Err(Error::new("STALE_ID", "session does not exist"));
            }
            Some(id)
        } else {
            None
        };
        let query = object
            .get("query")
            .map(|v| {
                v.as_str()
                    .filter(|s| s.len() <= 256 && !s.chars().any(char::is_control))
                    .ok_or_else(|| Error::new("INVALID_PARAMS", "query must be bounded plain text"))
            })
            .transpose()?
            .unwrap_or("")
            .to_lowercase();
        let number = |key, default, max| -> Result<usize> {
            object
                .get(key)
                .map(|v| {
                    v.as_u64()
                        .filter(|v| *v <= max)
                        .map(|v| v as usize)
                        .ok_or_else(|| Error::new("INVALID_PARAMS", "invalid job list bound"))
                })
                .unwrap_or(Ok(default))
        };
        let offset = number("offset", 0, 128)?;
        let limit = number("limit", 50, 100)?;
        if limit == 0 {
            return Err(Error::new("INVALID_PARAMS", "limit must be 1..100"));
        }
        let mut jobs: Vec<_> = self
            .panes
            .values()
            .filter(|p| {
                session
                    .as_ref()
                    .is_none_or(|id| self.windows[&p.window].session == *id)
            })
            .map(|p| metadata(p, self))
            .filter(|p| {
                query.is_empty()
                    || [
                        "id",
                        "command",
                        "host",
                        "cwd",
                        "session_name",
                        "window_name",
                    ]
                    .iter()
                    .any(|k| {
                        p[*k]
                            .as_str()
                            .is_some_and(|s| s.to_lowercase().contains(&query))
                    })
                    || p["foreground"]["command"]
                        .as_str()
                        .is_some_and(|s| s.to_lowercase().contains(&query))
            })
            .collect();
        jobs.sort_by_key(|p| p["id"].as_str().unwrap().to_owned());
        let total = jobs.len();
        // Cursor offsets refer to this membership, not a mutable display title.
        use std::hash::{Hash, Hasher};
        let mut membership = std::collections::hash_map::DefaultHasher::new();
        for row in &jobs {
            row["id"].as_str().unwrap().hash(&mut membership);
        }
        let membership = format!("{:016x}", membership.finish());
        const PAGE_BYTES: usize = 64 * 1024;
        let mut bytes = 512;
        let mut rows = Vec::new();
        for row in jobs.into_iter().skip(offset).take(limit) {
            let size = serde_json::to_vec(&row)
                .map_err(|_| Error::new("LIMIT", "job metadata encoding failed"))?
                .len()
                + 1;
            if bytes + size > PAGE_BYTES {
                if rows.is_empty() {
                    return Err(Error::new(
                        "LIMIT",
                        "one job exceeds the metadata page budget",
                    ));
                }
                break;
            }
            bytes += size;
            rows.push(row);
        }
        let end = offset.saturating_add(rows.len());
        Ok(
            json!({"jobs":rows,"total":total,"offset":offset,"next_offset":if end<total{Some(end)}else{None},"max_jobs":128,"page_byte_limit":PAGE_BYTES,"membership":membership,"agent_state_inference":false}),
        )
    }
}
