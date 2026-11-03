//! Explicit Codex draft attachment. Never submits a turn or falls back to shell input.
use crate::{Error, Result, State};
use serde_json::{Value, json};

impl State {
    pub(super) fn attach_image(&self, v: &Value) -> Result<Value> {
        if v.as_object().is_none_or(|o| o.len() != 3) {
            return Err(Error::new(
                "INVALID_PARAMS",
                "pane, path and foreground_pid required",
            ));
        }
        let p = self.pane(v)?;
        let path = crate::text(v, "path")?;
        ruwac_pty::attachment::validate_file(&path).map_err(|e| {
            Error::new(
                if e.kind() == std::io::ErrorKind::InvalidInput {
                    "INVALID_PATH"
                } else {
                    "INVALID_IMAGE"
                },
                e.to_string(),
            )
        })?;
        let w = &self.windows[&p.window];
        let s = &self.sessions[&w.session];
        let pty = p.live_pty()?;
        let group = pty
            .foreground_group()
            .filter(|g| *g > 0)
            .ok_or_else(|| Error::new("STALE_TARGET", "foreground unavailable"))?;
        let agent = ruwac_pty::attachment::validate_process(group as u32)
            .map_err(|e| Error::new("STALE_TARGET", e.to_string()))?;
        let screen = p.term.capture(100);
        // An agent can paint its empty composer before it accepts images (Codex
        // loads model capabilities first). Refuse that startup state rather
        // than inserting a literal filename into the draft.
        if v["foreground_pid"].as_i64() != Some(group as i64)
            || w.active != p.id
            || s.active != w.id
            || !ruwac_pty::attachment::ready(agent, &screen)
            || !p.term.paste()
        {
            return Err(Error::new(
                "STALE_TARGET",
                "selected foreground Codex or Claude Code with bracketed paste required",
            ));
        }
        if pty.foreground_group() != Some(group) {
            return Err(Error::new("STALE_TARGET", "foreground changed"));
        }
        pty.send(format!("\x1b[200~{path}\x1b[201~").into_bytes())
            .map_err(|e| Error::new("BACKPRESSURE", e))?;
        Ok(
            json!({"pane":p.id,"queued":true,"attached":false,"submitted":false,"adapter":agent.name(),"foreground_pid":group}),
        )
    }
}
