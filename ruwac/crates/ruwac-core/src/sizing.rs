//! Explicit session sizing ownership. Connection leases are never checkpointed.
use super::*;

#[derive(Clone, Copy, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Policy {
    #[default]
    LegacyMinimum,
    ControllerOwned,
}
impl Policy {
    pub(super) fn is_legacy(&self) -> bool {
        *self == Self::LegacyMinimum
    }
    pub(super) fn parse(v: &Value) -> Result<Self> {
        match v.get("size_policy") {
            None => Ok(Self::LegacyMinimum),
            Some(value) => serde_json::from_value(value.clone()).map_err(|_| {
                Error::new(
                    "INVALID_PARAMS",
                    "size_policy must be legacy_minimum or controller_owned",
                )
            }),
        }
    }
}
impl State {
    /// Older TUIs automatically retire completed panes. A controlled viewer may
    /// acknowledge that cleanup without removing shared history or layout.
    pub(super) fn retain_viewer_cleanup(&self, v: &Value, client: &str) -> bool {
        // Some older clients omit if_ended even in automatic exit cleanup.
        // Retain an already-ended pane for any viewer close request; a live
        // pane still requires sizing ownership and cannot reach this path.
        if v.get("if_ended").is_some_and(|value| !value.is_boolean()) {
            return false;
        }
        let Ok(pane) = self.pane(v) else {
            return false;
        };
        let session = &self.sessions[&self.windows[&pane.window].session];
        pane.ended
            && !session.size_policy.is_legacy()
            && session.size_owner.as_deref() != Some(client)
            && self
                .clients
                .get(client)
                .is_some_and(|c| c.session == session.id)
    }
    fn size_token(&self, session: &Session) -> String {
        format!(
            "{}:{}:{}",
            self.size_nonce, session.id, session.size_generation
        )
    }
    pub(super) fn size_status(&self, id: &str) -> Result<Value> {
        let s = self
            .sessions
            .get(id)
            .ok_or_else(|| Error::new("STALE_ID", "session does not exist"))?;
        Ok(
            json!({"session":s.id,"policy":s.size_policy,"rows":s.rows,"cols":s.cols,
            "generation":self.size_token(s),"controller_present":s.size_owner.is_some(),
            "lease_scope":"live connection; never restored","policy_version":1}),
        )
    }
    /// Compare-and-swap ownership, including the explicit takeover decision.
    pub(super) fn claim_sizing(&mut self, v: &Value, client: &str) -> Result<Value> {
        let id = target(v, "session", "session")?;
        let c = self
            .clients
            .get(client)
            .filter(|c| c.session == id)
            .ok_or_else(|| {
                Error::new(
                    "NOT_ATTACHED",
                    "attach to this session before claiming sizing",
                )
            })?;
        let _ = c;
        let s = self.sessions.get(&id).unwrap();
        if s.size_policy.is_legacy() {
            return Err(Error::new(
                "SIZE_POLICY",
                "sizing leases require a controller_owned session",
            ));
        }
        let expected = text(v, "expected_generation")?;
        if expected != self.size_token(s) {
            return Err(Error::new(
                "SIZE_STALE",
                "sizing generation changed; inspect before claiming again",
            ));
        }
        let takeover = match v.get("takeover") {
            Some(Value::Bool(value)) => *value,
            _ => {
                return Err(Error::new(
                    "INVALID_PARAMS",
                    "takeover must be an explicit boolean",
                ));
            }
        };
        if s.size_owner.as_deref() == Some(client) {
            return Err(Error::new(
                "SIZE_ALREADY_OWNED",
                "this connection already controls sizing",
            ));
        }
        if s.size_owner.is_some() && !takeover {
            return Err(Error::new(
                "SIZE_BUSY",
                "another connection controls sizing; explicit takeover required",
            ));
        }
        let next = s
            .size_generation
            .checked_add(1)
            .ok_or_else(|| Error::new("LIMIT", "sizing generation exhausted"))?;
        let s = self.sessions.get_mut(&id).unwrap();
        if let Some(old) = s.size_owner.replace(client.into()) {
            if let Some(c) = self.clients.get_mut(&old) {
                c.non_sizing = true;
            }
        }
        s.size_generation = next;
        self.clients.get_mut(client).unwrap().non_sizing = false;
        // Claiming itself never changes the canonical grid or signals the PTY.
        self.size_status(&id)
    }
    pub(super) fn release_sizing(&mut self, client: &str) {
        for s in self.sessions.values_mut() {
            if s.size_owner.as_deref() == Some(client) {
                s.size_owner = None;
                // The daemon identity also changes on restoration. Exhaustion
                // can never wrap back to a formerly valid generation.
                s.size_generation = s.size_generation.saturating_add(1);
                if let Some(c) = self.clients.get_mut(client) {
                    c.non_sizing = true;
                }
            }
        }
    }
    pub(super) fn require_sizer(&self, id: &str, v: &Value, client: &str) -> Result<()> {
        let s = self
            .sessions
            .get(id)
            .ok_or_else(|| Error::new("STALE_ID", "session does not exist"))?;
        if s.size_owner.as_deref() != Some(client)
            || !self.clients.get(client).is_some_and(|c| c.session == id)
        {
            return Err(Error::new(
                "SIZE_NOT_OWNED",
                "explicit sizing control required",
            ));
        }
        if v.get("size_generation").and_then(Value::as_str) != Some(self.size_token(s).as_str()) {
            return Err(Error::new(
                "SIZE_STALE",
                "current sizing generation required",
            ));
        }
        Ok(())
    }
    /// Ownership checks precede cache lookup and every PTY/layout mutation.
    pub(super) fn check_sizing(&self, method: &str, v: &Value, client: &str) -> Result<()> {
        let id = match method {
            "window.create" => Some(target(v, "session", "session")?),
            "pane.create" => {
                let w = target(v, "window", "window")?;
                self.windows.get(&w).map(|w| w.session.clone())
            }
            "pane.close" => self
                .pane(v)
                .ok()
                .map(|p| self.windows[&p.window].session.clone()),
            "pane.layout" if v.get("split").is_some() => {
                let w = target(v, "window", "window")?;
                self.windows.get(&w).map(|w| w.session.clone())
            }
            "client.resize" => self
                .clients
                .get(client)
                .filter(|c| !c.non_sizing || v.get("size_generation").is_some())
                .map(|c| c.session.clone()),
            _ => None,
        };
        if let Some(id) = id {
            if self
                .sessions
                .get(&id)
                .is_some_and(|s| !s.size_policy.is_legacy())
            {
                self.require_sizer(&id, v, client)?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn legacy_checkpoint_fields_remain_absent_and_leases_never_serialize() {
        let legacy: Session = serde_json::from_value(
            json!({"id":"s","name":"n","windows":[],"active":"", "rows":42,"cols":120}),
        )
        .unwrap();
        let value = serde_json::to_value(&legacy).unwrap();
        assert!(value.get("size_policy").is_none());
        assert!(value.get("size_generation").is_none());
        assert!(value.get("size_owner").is_none());
        let controlled: Session = serde_json::from_value(json!({"id":"s","name":"n","windows":[],"active":"", "rows":42,"cols":120,"size_policy":"controller_owned"})).unwrap();
        assert_eq!(
            serde_json::to_value(&controlled).unwrap()["size_policy"],
            "controller_owned"
        );
        assert!(controlled.size_owner.is_none());
        assert_eq!(controlled.size_generation, 0);
        assert!(Policy::parse(&json!({"size_policy":"automatic_takeover"})).is_err());
    }
}
