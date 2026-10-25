//! Optional authenticated Linux PRIMARY bridge. CLIPBOARD remains independent.
use crate::profiles::{self, Profile};
use gtk::prelude::*;
use std::{
    io::{BufRead, BufReader, Read, Write},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};
const HELPER: &str = include_str!("../../../scripts/primary-selection.py");
fn peer_command(display: &str, label: &str) -> Result<String, String> {
    let sources = serde_json::json!({
        "compositor_identity": include_str!("../../../scripts/compositor_identity.py"),
        "native_publisher": include_str!("../../../scripts/native_publisher.py"),
        "framed_publisher": include_str!("../../../scripts/framed_publisher.py"),
        "legacy_primary": HELPER,
        "primary_native_peer": include_str!("../../../scripts/primary_native_peer.py"),
        "primary_entry": include_str!("../../../scripts/primary_entry.py"),
    });
    let data = serde_json::to_vec(&sources).map_err(|_| "Invalid selection peer sources")?;
    // Encode source, not selection data. Keep stdin dedicated to protocol frames
    // and stay below Linux's per-argument bound without a remote staging file.
    let encoded = gtk::glib::base64_encode(&data);
    let loader = format!(
        "import sys,types,json,base64\nsources=json.loads(base64.b64decode('{}'))\nfor name in ('compositor_identity','native_publisher','framed_publisher','legacy_primary','primary_native_peer','primary_entry'):\n module=types.ModuleType(name);sys.modules[name]=module\n exec(compile(sources[name],'<ruwac-primary/'+name+'>','exec'),module.__dict__)\nraise SystemExit(sys.modules['primary_entry'].main())\n",
        encoded
    );
    let command = format!(
        "python3 -c {} --desktop {} --label {}",
        quote(&loader),
        quote(display),
        quote(label)
    );
    if command.len() > 120 * 1024 {
        return Err("Selection peer command exceeds the safe exec bound".into());
    }
    Ok(command)
}
#[path = "primary_flow.rs"]
mod flow;
fn quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}
pub fn validate(p: &profiles::Preferences) -> Result<(), String> {
    if p.primary_paste && !p.primary_sync {
        return Err(
            "Configure the Linux desktop selection-sharing route before enabling terminal paste"
                .into(),
        );
    }
    if !p.primary_sync {
        return Ok(());
    }
    if p.primary_user.is_empty()
        || p.primary_user.len() > 64
        || !p
            .primary_user
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
        || p.primary_user.starts_with('-')
        || !p
            .primary_display
            .strip_prefix("wayland-")
            .is_some_and(|s| !s.is_empty() && s.len() <= 3 && s.bytes().all(|b| b.is_ascii_digit()))
    {
        return Err("Mouse selection sharing needs a Linux SSH account and exact Wayland session (for example wayland-0)".into());
    }
    Ok(())
}
pub fn ssh_argv(p: &Profile) -> Result<Vec<String>, String> {
    validate(&p.preferences)?;
    if p.protocol != "RDP" || !p.preferences.primary_sync {
        return Err("Mouse selection sharing is disabled".into());
    }
    if !p.preferences.gateway.is_empty() && p.preferences.ssh_jump_alias.is_empty() {
        return Err("Selection sharing needs an explicit SSH route".into());
    }
    let (host, _) = profiles::endpoint(&p.server, 3389)?;
    let mut ssh = p.clone();
    ssh.protocol = "SSH".into();
    ssh.username = p.preferences.primary_user.clone();
    ssh.server = if p.preferences.primary_port == 0 {
        host
    } else if host.contains(':') {
        format!("[{host}]:{}", p.preferences.primary_port)
    } else {
        format!("{host}:{}", p.preferences.primary_port)
    };
    let mut args = profiles::ssh_argv(&ssh)?;
    for v in &mut args {
        if v == "StrictHostKeyChecking=ask" {
            *v = "StrictHostKeyChecking=yes".into();
        }
    }
    let i = args
        .iter()
        .position(|s| s == "--")
        .ok_or("Invalid SSH route")?;
    args.splice(
        i..i,
        [
            "-T".into(),
            "-o".into(),
            "BatchMode=yes".into(),
            "-o".into(),
            "ForwardAgent=no".into(),
            "-o".into(),
            "ClearAllForwardings=yes".into(),
            "-o".into(),
            "ConnectTimeout=8".into(),
            "-o".into(),
            "ServerAliveInterval=3".into(),
            "-o".into(),
            "ServerAliveCountMax=2".into(),
        ],
    );
    // The remote approval label is bounded in UTF-8 bytes, including its prefix.
    let mut label = format!("RUWAC · {}", p.name);
    let mut end = label.len().min(160);
    while !label.is_char_boundary(end) {
        end -= 1;
    }
    label.truncate(end);
    args.push(peer_command(&p.preferences.primary_display, &label)?);
    Ok(args)
}
#[derive(Default)]
struct Incoming {
    ready: bool,
    failed: bool,
    text: Option<(u64, String, bool)>,
    offer: Option<(u64, u64, bool)>,
    native: bool,
    observes: bool,
    replies: std::collections::VecDeque<(u8, u64, u64, bool)>,
}
struct ClipboardWatch {
    clipboard: gtk::gdk::Clipboard,
    handler: Option<gtk::glib::SignalHandlerId>,
    revision: std::rc::Rc<std::cell::Cell<u64>>,
    observed: u64,
}
impl ClipboardWatch {
    fn new(clipboard: &gtk::gdk::Clipboard) -> Self {
        let revision = std::rc::Rc::new(std::cell::Cell::new(0u64));
        let changed = revision.clone();
        let handler = clipboard.connect_changed(move |_| {
            changed.set(changed.get().saturating_add(1));
        });
        Self {
            clipboard: clipboard.clone(),
            handler: Some(handler),
            revision,
            observed: 0,
        }
    }
}
impl Drop for ClipboardWatch {
    fn drop(&mut self) {
        if let Some(handler) = self.handler.take() {
            self.clipboard.disconnect(handler);
        }
    }
}
pub struct Bridge {
    child: Option<Child>,
    input: Option<std::sync::mpsc::SyncSender<Vec<u8>>>,
    incoming: std::sync::Arc<std::sync::Mutex<Incoming>>,
    selection_origin: std::sync::Arc<()>,
    clipboard_watch: Option<ClipboardWatch>,
    local_selection_changed: bool,
    last: Instant,
    modes: Option<(bool, bool)>,
    next_offer: u64,
    epoch: u64,
    pub ready: bool,
    pub failed: bool,
    native: bool,
    flow: flow::Flow,
}
impl Bridge {
    pub fn start(profile: &Profile) -> Result<Self, String> {
        let args = ssh_argv(profile)?;
        let mut child = Command::new(&args[0])
            .args(&args[1..])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|_| "SSH is unavailable for mouse selection sharing")?;
        let mut input = child.stdin.take().unwrap();
        let output = child.stdout.take().unwrap();
        let incoming = std::sync::Arc::new(std::sync::Mutex::new(Incoming::default()));
        let writer_state = incoming.clone();
        let (sender, output_frames) = std::sync::mpsc::sync_channel::<Vec<u8>>(2);
        std::thread::spawn(move || {
            for frame in output_frames {
                if input.write_all(&frame).and_then(|_| input.flush()).is_err() {
                    writer_state.lock().unwrap().failed = true;
                    break;
                }
            }
        });
        let state = incoming.clone();
        std::thread::spawn(move || {
            let mut reader = BufReader::new(output);
            loop {
                let mut line = Vec::new();
                let Ok(n) = (&mut reader)
                    .take((1024 * 1024 * 6 + 1025) as u64)
                    .read_until(b'\n', &mut line)
                else {
                    break;
                };
                if n == 0 || n > 1024 * 1024 * 6 + 1024 || line.last() != Some(&b'\n') {
                    break;
                }
                let Ok(value) = serde_json::from_slice::<serde_json::Value>(&line) else {
                    break;
                };
                match value["type"].as_str() {
                    Some("hello") => {
                        let native = value["protocol"] == 3
                            && value["publisher"] == "native-primary"
                            && value["max_offer_bytes"] == 131072
                            && value["prepare_commit"] == true
                            && value["confirm_before_input"] == true
                            && value["observes_selection"].is_boolean();
                        let legacy = value.as_object().is_some_and(|v| v.len() == 1);
                        let mut state = state.lock().unwrap();
                        if state.ready || (!native && !legacy) {
                            break;
                        }
                        state.native = native;
                        state.observes = legacy || value["observes_selection"] == true;
                        state.ready = true;
                    }
                    Some("text") => {
                        let (Some(epoch), Some(text)) =
                            (value["epoch"].as_u64(), value["text"].as_str())
                        else {
                            break;
                        };
                        if text.is_empty() || text.len() > 1024 * 1024 || text.contains('\0') {
                            break;
                        }
                        state.lock().unwrap().text = Some((
                            epoch,
                            text.to_owned(),
                            value["initial"].as_bool().unwrap_or(false),
                        ));
                    }
                    Some("offer-result") => {
                        if state.lock().unwrap().native {
                            break;
                        }
                        let (Some(epoch), Some(id), Some(ok)) = (
                            value["epoch"].as_u64(),
                            value["id"].as_u64(),
                            value["ok"].as_bool(),
                        ) else {
                            break;
                        };
                        state.lock().unwrap().offer = Some((epoch, id, ok));
                    }
                    Some(
                        "enroll-result" | "prepare-result" | "commit-result" | "confirm-result"
                        | "cancel-result",
                    ) => {
                        let kind = match value["type"].as_str() {
                            Some("enroll-result") => 0,
                            Some("prepare-result") => 1,
                            Some("commit-result") => 2,
                            Some("confirm-result") => 4,
                            _ => 3,
                        };
                        let (Some(epoch), Some(id), Some(ok)) = (
                            value["epoch"].as_u64(),
                            value["id"].as_u64(),
                            value["ok"].as_bool(),
                        ) else {
                            break;
                        };
                        let mut state = state.lock().unwrap();
                        if !state.native
                            || epoch == 0
                            || id == 0
                            || epoch >= 1 << 53
                            || id >= 1 << 53
                            || state.replies.len() >= 8
                        {
                            break;
                        }
                        if kind != 3 {
                            state.replies.push_back((kind, epoch, id, ok));
                        }
                    }
                    Some("mode-result") => {
                        if !state.lock().unwrap().native
                            || !value["ok"].is_boolean()
                            || !value["registered"].is_boolean()
                            || !value["epoch"].is_u64()
                            || value["ok"] != true
                        {
                            break;
                        }
                    }
                    _ => break,
                }
            }
            state.lock().unwrap().failed = true;
        });
        Ok(Self {
            child: Some(child),
            input: Some(sender),
            incoming,
            selection_origin: std::sync::Arc::new(()),
            clipboard_watch: None,
            local_selection_changed: false,
            last: Instant::now() - Duration::from_secs(2),
            modes: None,
            next_offer: 0,
            epoch: 0,
            ready: false,
            failed: false,
            native: false,
            flow: flow::Flow::default(),
        })
    }
    pub fn tick(&mut self, send: bool, receive: bool) {
        {
            let state = self.incoming.lock().unwrap();
            self.ready = state.ready && !state.failed;
            self.failed = state.failed;
            self.native = state.native;
            let mut state = state;
            let mut retained = std::collections::VecDeque::new();
            while let Some((kind, epoch, id, ok)) = state.replies.pop_front() {
                if kind == 0 {
                    self.flow.authorized(epoch, id, ok);
                } else {
                    retained.push_back((kind, epoch, id, ok));
                }
            }
            state.replies = retained;
        }
        if self
            .child
            .as_mut()
            .is_some_and(|c| c.try_wait().ok().flatten().is_some())
        {
            self.failed = true;
            self.ready = false;
        }
        if self.failed {
            self.flow.revoke();
            self.input.take();
            return;
        }
        // Protocol capabilities must be known before sending a mode frame.
        if !self.ready {
            return;
        }
        let changed = self.modes != Some((send, receive));
        if !changed && self.last.elapsed() < Duration::from_secs(1) {
            return;
        }
        if changed {
            self.epoch = self.epoch.saturating_add(1);
            self.incoming.lock().unwrap().text.take();
            self.incoming.lock().unwrap().offer.take();
            self.incoming.lock().unwrap().replies.clear();
        }
        self.modes = Some((send, receive));
        self.last = Instant::now();
        let mode = if self.native {
            if !self.flow.context(self.epoch, send) {
                self.failed = true;
                self.ready = false;
                self.flow.revoke();
                self.input.take();
                return;
            }
            serde_json::json!({"type":"mode", "epoch":self.epoch, "active":send, "observe":receive})
        } else {
            serde_json::json!({"type":"mode", "epoch":self.epoch, "send":receive, "receive":false, "paste":send})
        };
        if self.queue(mode).is_err() {
            self.failed = true;
            self.ready = false;
            self.input.take();
        }
    }
    fn queue(&self, value: serde_json::Value) -> Result<(), String> {
        let mut frame = serde_json::to_vec(&value).map_err(|_| "Invalid selection message")?;
        frame.push(b'\n');
        self.input
            .as_ref()
            .ok_or("Selection connection closed")?
            .try_send(frame)
            .map_err(|_| "Selection connection is busy or unavailable".into())
    }
    pub fn offer_primary(&mut self, text: String) -> Result<u64, String> {
        if !self.ready || self.failed || !self.modes.is_some_and(|(send, _)| send) {
            return Err("Selection sharing is not ready for sending".into());
        }
        if text.is_empty() || text.len() > crate::terminal_primary::MAX_OFFER || text.contains('\0')
        {
            return Err(
                "Terminal selection exceeds the 128 KiB paste limit or contains NUL".into(),
            );
        }
        self.next_offer = self
            .next_offer
            .checked_add(1)
            .ok_or("Selection sequence exhausted")?;
        let id = self.next_offer;
        if self.native && !self.flow.prepare(id) {
            return Err("Authorize this selection connection before pasting; finish or cancel the current request first".into());
        }
        if let Err(error) = self.queue(serde_json::json!({"type":if self.native {"prepare"} else {"offer"}, "epoch":self.epoch, "id":id, "text":text})) {
            self.flow.cancel(id);
            return Err(error);
        }
        Ok(id)
    }
    pub fn native(&self) -> bool {
        self.native
    }
    pub fn observes(&self) -> bool {
        self.incoming.lock().unwrap().observes
    }
    pub fn authorized(&self) -> bool {
        self.flow.registered()
    }
    pub fn authorizing(&self) -> bool {
        self.flow.enrolling()
    }
    pub fn authorize(&mut self) -> Result<(), String> {
        if !self.native || !self.ready || self.failed || !self.modes.is_some_and(|(send, _)| send) {
            return Err(
                "The focused desktop does not have an available native selection companion".into(),
            );
        }
        self.next_offer = self
            .next_offer
            .checked_add(1)
            .ok_or("Selection sequence exhausted")?;
        let id = self.next_offer;
        if !self.flow.authorize(id) {
            return Err("Connection is already authorized or has a pending request".into());
        }
        if let Err(error) =
            self.queue(serde_json::json!({"type":"enroll","epoch":self.epoch,"id":id}))
        {
            self.flow.revoke();
            self.failed = true;
            self.ready = false;
            self.input.take();
            return Err(error);
        }
        Ok(())
    }
    fn native_reply(&self, kind: u8, wanted: u64) -> Option<(u64, bool)> {
        let mut state = self.incoming.lock().unwrap();
        let at = state
            .replies
            .iter()
            .position(|(k, e, i, _)| *k == kind && *e == self.epoch && *i == wanted)?;
        state.replies.remove(at).map(|(_, e, _, ok)| (e, ok))
    }
    pub fn take_prepare_result(&mut self, wanted: u64) -> Option<bool> {
        let (epoch, ok) = self.native_reply(1, wanted)?;
        self.flow.prepared(epoch, wanted, ok).then_some(ok)
    }
    pub fn commit_primary(&mut self, wanted: u64) -> Result<(), String> {
        if self.failed || !self.ready || !self.flow.commit(wanted) {
            return Err("Selection preparation expired or its context changed".into());
        }
        if let Err(error) =
            self.queue(serde_json::json!({"type":"commit","epoch":self.epoch,"id":wanted}))
        {
            self.flow.cancel(wanted);
            return Err(error);
        }
        Ok(())
    }
    pub fn cancel_primary(&mut self, wanted: u64) {
        if self.native && self.flow.cancel(wanted) {
            if self
                .queue(serde_json::json!({"type":"cancel","epoch":self.epoch,"id":wanted}))
                .is_err()
            {
                self.failed = true;
                self.ready = false;
                self.flow.revoke();
                self.input.take();
            }
        }
    }
    pub fn confirm_primary(&mut self, wanted: u64) -> Result<(), String> {
        if self.failed || !self.ready || !self.flow.confirm(wanted) {
            return Err(
                "Selection publication or its context changed before input confirmation".into(),
            );
        }
        if let Err(error) =
            self.queue(serde_json::json!({"type":"confirm","epoch":self.epoch,"id":wanted}))
        {
            self.flow.cancel(wanted);
            return Err(error);
        }
        Ok(())
    }
    pub fn take_confirm_result(&mut self, wanted: u64) -> Option<bool> {
        let (epoch, ok) = self.native_reply(4, wanted)?;
        self.flow.confirmed(epoch, wanted, ok)
    }
    pub fn take_offer_result(&mut self, wanted: u64) -> Option<bool> {
        if self.native {
            let (epoch, ok) = self.native_reply(2, wanted)?;
            return self.flow.committed(epoch, wanted, ok);
        }
        self.incoming
            .lock()
            .unwrap()
            .offer
            .take()
            .filter(|(epoch, id, _)| {
                *epoch == self.epoch && *id == wanted && self.modes.is_some_and(|(send, _)| send)
            })
            .map(|(_, _, ok)| ok)
    }
    fn refresh_primary_context(&mut self, clipboard: &gtk::gdk::Clipboard) {
        let changed = if let Some(watch) = self.clipboard_watch.as_mut() {
            if watch.clipboard == *clipboard {
                let revision = watch.revision.get();
                let changed = watch.observed != revision;
                watch.observed = revision;
                changed
            } else {
                self.clipboard_watch = Some(ClipboardWatch::new(clipboard));
                true
            }
        } else {
            self.clipboard_watch = Some(ClipboardWatch::new(clipboard));
            // A restarted helper has no history of this desktop's existing
            // owner. Preserve any advertised selection, including selections
            // from other native apps that have no RUWAC content provider.
            let formats = clipboard.formats();
            self.local_selection_changed =
                !formats.mime_types().is_empty() || !formats.types().is_empty();
            false
        };
        if !changed || self.selection_from_this_desktop(clipboard) {
            return;
        }
        self.local_selection_changed = true;
        // A locally observed owner change supersedes messages still in transit.
        // Advance the existing protocol epoch before taking any incoming text.
        // The helper's refreshed initial snapshot cannot displace that copy,
        // while subsequent genuine remote selections remain eligible.
        if let Some((send, receive)) = self.modes.take() {
            self.tick(send, receive);
        }
    }
    pub fn publish_primary(
        &mut self,
        clipboard: &gtk::gdk::Clipboard,
    ) -> Result<Option<usize>, String> {
        self.refresh_primary_context(clipboard);
        let Some((text, initial)) = self.take_incoming() else {
            return Ok(None);
        };
        if initial
            && (self.local_selection_changed
                || crate::terminal_primary::selected_elsewhere(clipboard, &self.selection_origin))
        {
            return Ok(None);
        }
        let size = text.len();
        let provider = gtk::gdk::ContentProvider::for_bytes(
            "text/plain;charset=utf-8",
            &gtk::glib::Bytes::from_owned(text.as_bytes().to_vec()),
        );
        clipboard
            .set_content(Some(&provider))
            .map_err(|_| "The desktop refused the remote mouse selection".to_string())?;
        // Keep its origin while it is still this compositor's actual PRIMARY
        // owner. Another tab must be able to deliberately paste an RDP-origin
        // selection just as it can paste a terminal-origin selection.
        crate::terminal_primary::remember_remote(&provider, &text, &self.selection_origin);
        self.local_selection_changed = false;
        Ok(Some(size))
    }
    /// This desktop already owns its native selection. A middle-click within
    /// the same connection needs no cross-desktop publication or receiver.
    pub fn selection_from_this_desktop(&self, clipboard: &gtk::gdk::Clipboard) -> bool {
        crate::terminal_primary::selected(clipboard).is_some()
            && !crate::terminal_primary::selected_elsewhere(clipboard, &self.selection_origin)
    }
    fn take_incoming(&self) -> Option<(String, bool)> {
        self.incoming
            .lock()
            .unwrap()
            .text
            .take()
            .filter(|(epoch, _, _)| {
                self.modes.is_some_and(|(_, receive)| receive) && *epoch == self.epoch
            })
            .map(|(_, text, initial)| (text, initial))
    }
    pub fn take_text(&self) -> Option<String> {
        self.take_incoming().map(|(text, _)| text)
    }
}
impl Drop for Bridge {
    fn drop(&mut self) {
        self.input.take();
        if let Some(mut child) = self.child.take() {
            std::thread::spawn(move || {
                let end = Instant::now() + Duration::from_secs(5);
                loop {
                    if child.try_wait().ok().flatten().is_some() {
                        break;
                    }
                    if Instant::now() >= end {
                        let _ = child.kill();
                        let _ = child.wait();
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(25));
                }
            });
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unicode_approval_labels_fit_remote_byte_limit() {
        let mut p: Profile = serde_json::from_value(serde_json::json!({"name":"test","group":"","protocol":"RDP","server":"server:3389","username":"service","secret_reference":"x","favorite":false,"last_used":0,"tags":[]})).unwrap();
        p.preferences.primary_sync = true;
        p.preferences.primary_user = "desktop".into();
        p.preferences.primary_display = "wayland-0".into();
        for (name, expected) in [
            ("🧪".repeat(80), format!("RUWAC · {}", "🧪".repeat(37))),
            ("é".repeat(80), format!("RUWAC · {}", "é".repeat(75))),
            ("a".repeat(200), format!("RUWAC · {}", "a".repeat(151))),
            ("Bob's desktop".into(), "RUWAC · Bob's desktop".into()),
        ] {
            p.name = name;
            assert!(
                ssh_argv(&p)
                    .unwrap()
                    .last()
                    .unwrap()
                    .ends_with(&quote(&expected))
            );
        }
    }
    #[test]
    fn strict_route_and_session() {
        let mut p:Profile=serde_json::from_value(serde_json::json!({"name":"test","group":"","protocol":"RDP","server":"server:3389","username":"service","secret_reference":"x","favorite":false,"last_used":0,"tags":[]})).unwrap();
        p.preferences.primary_sync = true;
        p.preferences.primary_user = "desktop".into();
        p.preferences.primary_display = "wayland-0".into();
        p.preferences.primary_port = 9223;
        let a = ssh_argv(&p).unwrap();
        assert!(a.windows(2).any(|v| v == ["-l", "desktop"]));
        assert!(a.windows(2).any(|v| v == ["-p", "9223"]));
        assert!(a.iter().any(|s| s == "StrictHostKeyChecking=yes"));
        assert!(
            !a.iter()
                .take(a.len() - 1)
                .any(|s| s.contains("ask") || s == "service")
        );
        for display in ["", "wayland-0;evil", "/tmp/wayland-0", "wayland-1000"] {
            p.preferences.primary_display = display.into();
            assert!(ssh_argv(&p).is_err());
        }
    }
}
