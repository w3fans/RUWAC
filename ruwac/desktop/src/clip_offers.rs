//! Format-list ACKs have no wire ID. Correlate them by allowing one in flight.
#[derive(Default)]
pub struct Offers<T> {
    pub inflight: Option<T>,
    pub waiting: Option<T>,
}
impl<T> Offers<T> {
    pub fn submit(&mut self, offer: T) {
        self.waiting = Some(offer);
    }
    pub fn ready(&mut self) -> Option<T> {
        if self.inflight.is_some() {
            None
        } else {
            self.waiting.take()
        }
    }
    pub fn sent(&mut self, offer: T) {
        assert!(self.inflight.is_none());
        self.inflight = Some(offer);
    }
    pub fn acknowledge(&mut self) -> Option<T> {
        self.inflight.take()
    }
    pub fn clear(&mut self) {
        self.inflight.take();
        self.waiting.take();
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn old_ack_never_satisfies_a_new_waiting_paste_intent() {
        let mut offers = Offers::default();
        offers.submit((1, false));
        let old = offers.ready().unwrap();
        offers.sent(old);
        offers.submit((2, true));
        assert!(offers.ready().is_none());
        assert_eq!(offers.acknowledge(), Some((1, false)));
        // Duplicate/unsolicited ACK in the same drain has no outstanding offer.
        assert_eq!(offers.acknowledge(), None);
        let new = offers.ready().unwrap();
        offers.sent(new);
        assert_eq!(offers.acknowledge(), Some((2, true)));
        assert_eq!(offers.acknowledge(), None);
    }
    #[test]
    fn skipped_or_rejected_enqueue_never_arms_an_intent() {
        let mut offers = Offers::default();
        offers.submit(1);
        let _rejected = offers.ready();
        assert_eq!(offers.acknowledge(), None);
        offers.submit(2);
        offers.clear();
        assert_eq!(offers.ready(), None);
    }
    #[test]
    fn latest_source_wins_before_submission_and_reset_cancels_both() {
        let mut offers = Offers::default();
        offers.submit(1);
        offers.submit(2);
        let second = offers.ready().unwrap();
        assert_eq!(second, 2);
        offers.sent(second);
        offers.submit(3);
        offers.clear();
        assert_eq!(offers.acknowledge(), None);
        assert_eq!(offers.ready(), None);
    }
}
