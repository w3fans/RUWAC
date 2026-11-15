//! Optional per-paste rendezvous with the GUI owning this local nested TUI.
//! The attached client chooses its actual pane; the GUI cannot inject a path
//! through a footer, shell, chooser or stale foreground process.
use super::call;
use ruwac_pty::{attachment as guard, image_offer as offer};
use serde_json::{Value, json};
use std::{io, os::unix::net::UnixStream, path::PathBuf, sync::mpsc};

pub struct Pending {
    session: Value,
    window: Value,
    pane: Value,
    process: offer::Process,
    receive: mpsc::Receiver<io::Result<(UnixStream, PathBuf)>>,
    _cancel: offer::Cancel,
}
impl Pending {
    pub fn begin(rpc: &mut UnixStream, frame: &Value) -> io::Result<Option<Self>> {
        let Some((mut stream, cancel)) = offer::claim()? else {
            return Ok(None);
        };
        let worker = offer::Worker::reserve()?;
        let job = call(rpc, "job.query", json!({"pane":frame["active_pane"]}))?;
        let pid = job["foreground"]["pid"]
            .as_u64()
            .and_then(|n| u32::try_from(n).ok())
            .ok_or_else(|| io::Error::other("PNG handoff requires a live agent"))?;
        let process = offer::Process::read(pid)?;
        if guard::foreground_agent(pid).is_none()
            || job["session"] != frame["session"]
            || job["selected"] != true
        {
            return Err(io::Error::other(
                "PNG handoff requires the selected local agent",
            ));
        }
        let (send, receive) = mpsc::sync_channel(1);
        std::thread::spawn(move || {
            let _worker = worker;
            let result = (|| {
                offer::send(&mut stream, br#"{"version":1,"kind":"local-image"}"#)?;
                let reply: Value = serde_json::from_slice(&offer::receive(&mut stream, || true)?)?;
                if reply["version"] != 1 {
                    return Err(io::Error::other("PNG handoff version rejected"));
                }
                let path = reply["path"]
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .map(PathBuf::from)
                    .filter(|p| p.is_absolute())
                    .ok_or_else(|| io::Error::other("PNG handoff path rejected"))?;
                Ok((stream, path))
            })();
            let _ = send.try_send(result);
        });
        Ok(Some(Self {
            session: frame["session"].clone(),
            window: frame["window"].clone(),
            pane: frame["active_pane"].clone(),
            process,
            receive,
            _cancel: cancel,
        }))
    }
    pub fn current(&self, frame: &Value) -> bool {
        frame["session"] == self.session
            && frame["window"] == self.window
            && frame["active_pane"] == self.pane
            && self.process.live()
    }
    /// No waiting for the desktop clipboard in the input loop. On delivery,
    /// re-read this client's frame and foreground before its normal guarded RPC.
    pub fn poll(&self, rpc: &mut UnixStream) -> Option<io::Result<Value>> {
        let result = match self.receive.try_recv() {
            Ok(result) => result,
            Err(mpsc::TryRecvError::Empty) => return None,
            Err(_) => return Some(Err(io::Error::other("PNG handoff worker closed"))),
        };
        Some((|| {
            let (mut stream, path) = result?;
            let fresh = call(rpc, "client.render", json!({}))?;
            if !self.current(&fresh) {
                return Err(io::Error::other("PNG handoff pane changed"));
            }
            let job = call(rpc, "job.query", json!({"pane":self.pane}))?;
            let agent = guard::validate(
                path.to_str()
                    .ok_or_else(|| io::Error::other("PNG path is not UTF-8"))?,
                self.process.pid,
            )?;
            let screen = call(rpc, "pane.capture", json!({"pane":self.pane,"lines":100}))?;
            if job["selected"] != true
                || job["session"] != self.session
                || job["foreground"]["pid"].as_u64() != Some(u64::from(self.process.pid))
                || !self.process.live()
                || !guard::ready(agent, screen["text"].as_str().unwrap_or(""))
                || !fresh["panes"].as_array().is_some_and(|panes| {
                    panes.iter().any(|p| {
                        p["id"] == self.pane
                            && p["ended"] != true
                            && p["grid"]["bracketed_paste"] == true
                    })
                })
                || !offer::consumer_live(&stream)
            {
                return Err(io::Error::other("PNG handoff target or focus changed"));
            }
            // The engine repeats agent/readiness/file guards. Never send Enter.
            let legacy_claude = agent == guard::Agent::Claude
                && call(rpc, "agent.probe", json!({"pane":self.pane}))
                    .map_or(true, |probe| probe["adapter"] != "claude");
            if !self.process.live() || !offer::consumer_live(&stream) {
                return Err(io::Error::other("PNG handoff target or focus changed"));
            }
            let receipt = if legacy_claude {
                // Same explicit legacy route used by image-attach on pre-54
                // engines. Target/file/readiness have just been validated.
                call(
                    rpc,
                    "client.input",
                    json!({"pane":self.pane,"kind":"paste","text":path}),
                )?;
                json!({"adapter":"claude","legacy_daemon":true})
            } else {
                call(
                    rpc,
                    "pane.attachImage",
                    json!({"pane":self.pane,"path":path,"foreground_pid":self.process.pid}),
                )?
            };
            offer::send(
                &mut stream,
                &serde_json::to_vec(
                    &json!({"version":1,"queued":true,"submitted":false,"adapter":receipt["adapter"]}),
                )?,
            )?;
            Ok(receipt)
        })())
    }
}
