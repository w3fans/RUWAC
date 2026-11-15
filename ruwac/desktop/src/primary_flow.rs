//! Native PRIMARY publication state. Payloads and desktop input stay outside it.
//! Authorization is an independent deliberate action, never an implicit paste.
const MAX_INTEGER: u64 = (1 << 53) - 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stage {
    Preparing,
    Prepared,
    Committing,
    Committed,
    Confirming,
}

#[derive(Default)]
pub(super) struct Flow {
    epoch: u64,
    active: bool,
    registered: bool,
    enrolling: Option<u64>,
    pending: Option<(u64, Stage)>,
    last_id: u64,
}

impl Flow {
    pub fn context(&mut self, epoch: u64, active: bool) -> bool {
        if epoch == 0 || epoch > MAX_INTEGER || epoch < self.epoch {
            return false;
        }
        if epoch == self.epoch {
            return active == self.active;
        }
        self.epoch = epoch;
        self.active = active;
        self.enrolling = None;
        self.pending = None;
        true
    }

    fn new_intent(&mut self, id: u64) -> bool {
        if !self.active || id == 0 || id > MAX_INTEGER || id <= self.last_id {
            return false;
        }
        self.last_id = id;
        true
    }

    pub fn authorize(&mut self, id: u64) -> bool {
        if self.registered
            || self.enrolling.is_some()
            || self.pending.is_some()
            || !self.new_intent(id)
        {
            return false;
        }
        self.enrolling = Some(id);
        true
    }

    pub fn authorized(&mut self, epoch: u64, id: u64, ok: bool) -> bool {
        if !self.active || epoch != self.epoch || self.enrolling != Some(id) {
            return false;
        }
        self.enrolling = None;
        self.registered = ok;
        true
    }

    pub fn registered(&self) -> bool {
        self.registered
    }

    pub fn enrolling(&self) -> bool {
        self.enrolling.is_some()
    }

    pub fn prepare(&mut self, id: u64) -> bool {
        if !self.registered
            || self.enrolling.is_some()
            || self.pending.is_some()
            || !self.new_intent(id)
        {
            return false;
        }
        self.pending = Some((id, Stage::Preparing));
        true
    }

    pub fn prepared(&mut self, epoch: u64, id: u64, ok: bool) -> bool {
        if !self.active || epoch != self.epoch || self.pending != Some((id, Stage::Preparing)) {
            return false;
        }
        self.pending = ok.then_some((id, Stage::Prepared));
        true
    }

    /// The caller must revalidate focus, source ownership, profile and transport
    /// immediately before this transition, then again before actual mouse input.
    pub fn commit(&mut self, id: u64) -> bool {
        if !self.active || !self.registered || self.pending != Some((id, Stage::Prepared)) {
            return false;
        }
        self.pending = Some((id, Stage::Committing));
        true
    }

    pub fn committed(&mut self, epoch: u64, id: u64, ok: bool) -> Option<bool> {
        if !self.active || epoch != self.epoch || self.pending != Some((id, Stage::Committing)) {
            return None;
        }
        self.pending = ok.then_some((id, Stage::Committed));
        Some(ok)
    }

    pub fn confirm(&mut self, id: u64) -> bool {
        if !self.active || !self.registered || self.pending != Some((id, Stage::Committed)) {
            return false;
        }
        self.pending = Some((id, Stage::Confirming));
        true
    }

    pub fn confirmed(&mut self, epoch: u64, id: u64, ok: bool) -> Option<bool> {
        if !self.active || epoch != self.epoch || self.pending != Some((id, Stage::Confirming)) {
            return None;
        }
        self.pending = None;
        Some(ok)
    }

    pub fn cancel(&mut self, id: u64) -> bool {
        if !self.pending.is_some_and(|(wanted, _)| wanted == id) {
            return false;
        }
        self.pending = None;
        true
    }

    pub fn revoke(&mut self) {
        self.active = false;
        self.registered = false;
        self.enrolling = None;
        self.pending = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ready() -> Flow {
        let mut value = Flow::default();
        assert!(value.context(1, true));
        assert!(value.authorize(1));
        assert!(value.enrolling());
        assert!(value.authorized(1, 1, true));
        assert!(value.registered());
        value
    }

    #[test]
    fn approval_is_separate_from_publication() {
        let mut value = Flow::default();
        assert!(!value.authorize(1));
        assert!(value.context(1, true));
        assert!(!value.prepare(1));
        assert!(value.authorize(1));
        assert!(!value.prepare(2));
        assert!(!value.authorized(1, 2, true));
        assert!(value.authorized(1, 1, false));
        assert!(!value.registered());
        assert!(!value.prepare(2));
        assert!(value.authorize(2));
        assert!(value.authorized(1, 2, true));
        assert!(!value.commit(3));
    }

    #[test]
    fn prepare_commit_and_reply_are_distinct_exact_intents() {
        let mut value = ready();
        assert!(value.prepare(2));
        assert!(!value.commit(2));
        assert_eq!(value.committed(1, 2, true), None);
        assert!(!value.prepared(1, 3, true));
        assert!(!value.prepared(2, 2, true));
        assert!(value.prepared(1, 2, true));
        assert!(!value.commit(3));
        assert!(value.commit(2));
        assert!(!value.commit(2));
        assert_eq!(value.committed(1, 3, true), None);
        assert_eq!(value.committed(1, 2, true), Some(true));
        assert_eq!(value.committed(1, 2, true), None);
        assert!(!value.prepare(3));
        assert!(!value.confirm(3));
        assert!(value.confirm(2));
        assert!(!value.confirm(2));
        assert_eq!(value.confirmed(2, 2, true), None);
        assert_eq!(value.confirmed(1, 3, true), None);
        assert_eq!(value.confirmed(1, 2, true), Some(true));
        assert_eq!(value.confirmed(1, 2, true), None);
        assert!(value.prepare(3));
    }

    #[test]
    fn context_change_cancels_every_publication_stage() {
        for stage in 0..5 {
            let mut value = ready();
            assert!(value.prepare(2));
            if stage >= 1 {
                assert!(value.prepared(1, 2, true));
            }
            if stage >= 2 {
                assert!(value.commit(2));
            }
            if stage >= 3 {
                assert_eq!(value.committed(1, 2, true), Some(true));
            }
            if stage >= 4 {
                assert!(value.confirm(2));
            }
            assert!(value.context(2, false));
            assert!(!value.prepared(1, 2, true));
            assert!(!value.commit(2));
            assert_eq!(value.committed(1, 2, true), None);
            assert!(!value.confirm(2));
            assert_eq!(value.confirmed(1, 2, true), None);
            assert!(value.context(3, true));
            assert!(!value.commit(2));
            assert!(!value.prepare(2));
            assert!(value.prepare(3));
        }
    }

    #[test]
    fn old_approval_cannot_authorize_a_new_context_or_request() {
        let mut value = Flow::default();
        assert!(value.context(1, true));
        assert!(value.authorize(1));
        assert!(value.context(2, true));
        assert!(value.authorize(2));
        assert!(!value.authorized(1, 1, true));
        assert!(!value.registered());
        assert!(value.authorized(2, 2, true));
    }

    #[test]
    fn cancellation_and_receiver_refusal_require_a_new_gesture() {
        for stage in 0..5 {
            let mut value = ready();
            assert!(value.prepare(2));
            if stage >= 1 {
                assert!(value.prepared(1, 2, true));
            }
            if stage >= 2 {
                assert!(value.commit(2));
            }
            if stage >= 3 {
                assert_eq!(value.committed(1, 2, true), Some(true));
            }
            if stage >= 4 {
                assert!(value.confirm(2));
            }
            assert!(!value.cancel(3));
            assert!(value.cancel(2));
            assert!(!value.commit(2));
            assert_eq!(value.committed(1, 2, true), None);
            assert_eq!(value.confirmed(1, 2, true), None);
            assert!(!value.prepare(2));
            assert!(value.prepare(3));
            assert!(value.prepared(1, 3, false));
            assert!(!value.commit(3));
            assert!(value.prepare(4));
        }
    }

    #[test]
    fn revocation_cannot_be_revived_by_a_delayed_result() {
        for stage in 0..3 {
            let mut value = ready();
            assert!(value.prepare(2));
            assert!(value.prepared(1, 2, true));
            assert!(value.commit(2));
            if stage >= 1 {
                assert_eq!(value.committed(1, 2, true), Some(true));
            }
            if stage >= 2 {
                assert!(value.confirm(2));
            }
            value.revoke();
            assert_eq!(value.committed(1, 2, true), None);
            assert_eq!(value.confirmed(1, 2, true), None);
            assert!(value.context(2, true));
            assert!(!value.registered());
            assert!(!value.prepare(3));
            assert!(value.authorize(3));
        }
    }

    #[test]
    fn heartbeat_and_integer_bounds_preserve_context() {
        let mut value = ready();
        assert!(value.prepare(2));
        assert!(value.context(1, true));
        assert!(value.prepared(1, 2, true));
        assert!(!value.context(1, false));
        assert!(!value.context(0, true));
        assert!(!value.context(MAX_INTEGER + 1, true));
        assert!(value.commit(2));
        assert_eq!(value.committed(1, 2, false), Some(false));
        assert!(!value.prepare(0));
        assert!(!value.prepare(MAX_INTEGER + 1));
        assert!(value.prepare(MAX_INTEGER));
    }
}
