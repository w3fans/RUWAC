//! Explicit connection-local non-sizing attachments. Legacy clients still vote.
use super::*;
pub(super) fn role(v: &Value) -> Result<bool> {
    match v.get("size_role") {
        None => Ok(false),
        Some(Value::String(s)) if s == "legacy" => Ok(false),
        Some(Value::String(s)) if s == "viewer" => Ok(true),
        _ => Err(Error::new(
            "INVALID_PARAMS",
            "size_role must be legacy or viewer",
        )),
    }
}
impl State {
    pub(super) fn is_viewer(&self, client: &str, session: &str) -> bool {
        self.clients
            .get(client)
            .is_some_and(|c| c.non_sizing && c.session == session)
    }
    /// Role and namespace checks precede cache lookup: a denied viewer call
    /// cannot poison an owner's shared key or replay its cached layout result.
    pub(super) fn check_viewer(&self, m: &str, v: &Value, client: &str) -> Result<()> {
        let Some(c) = self.clients.get(client).filter(|c| c.non_sizing) else {
            return Ok(());
        };
        if c.non_sizing
            && (matches!(m, "window.create" | "pane.create" | "pane.close")
                || (m == "pane.layout" && v.get("split").is_some()))
        {
            return Err(Error::new(
                "SIZE_NOT_OWNED",
                "non-sizing viewers cannot change the shared layout; use a sizing attachment",
            ));
        }
        let session = match m {
            "pane.select" => Some(self.windows[&self.pane(v)?.window].session.clone()),
            "pane.layout" | "window.select" | "window.rename" => {
                let w = target(v, "window", "window")?;
                Some(
                    self.windows
                        .get(&w)
                        .ok_or_else(|| Error::new("STALE_ID", "window does not exist"))?
                        .session
                        .clone(),
                )
            }
            "window.cycle" | "window.previous" => Some(target(v, "session", "session")?),
            _ => None,
        };
        if session.is_some_and(|s| s != c.session) {
            return Err(Error::new(
                "TARGET_SESSION",
                "viewer navigation belongs to its attached session",
            ));
        }
        Ok(())
    }
}
