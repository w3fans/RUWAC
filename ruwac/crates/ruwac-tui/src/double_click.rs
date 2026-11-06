use std::time::{Duration, Instant};

#[derive(Default)]
pub struct Clicks {
    down: Option<(String, usize, usize, Instant)>,
    released: Option<(String, usize, usize, Instant)>,
}
impl Clicks {
    pub fn clear(&mut self) {
        self.down = None;
        self.released = None;
    }
    pub fn press(&mut self, pane: &str, x: usize, y: usize, now: Instant) -> bool {
        let double = self.released.take().is_some_and(|(p, px, py, at)| {
            p == pane && (px, py) == (x, y) && now.duration_since(at) <= Duration::from_millis(400)
        });
        self.down = (!double).then(|| (pane.into(), x, y, now));
        double
    }
    pub fn release(&mut self, pane: &str, x: usize, y: usize, now: Instant) {
        self.released = self.down.take().filter(|(p, px, py, at)| {
            p == pane
                && (*px, *py) == (x, y)
                && now.duration_since(*at) <= Duration::from_millis(400)
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn needs_two_complete_clicks_at_same_pane_and_cell() {
        let now = Instant::now();
        let mut c = Clicks::default();
        assert!(!c.press("a", 2, 3, now));
        assert!(!c.press("a", 2, 3, now));
        c.release("a", 2, 3, now);
        assert!(c.press("a", 2, 3, now + Duration::from_millis(100)));
        c.release("a", 2, 3, now + Duration::from_millis(120));
        assert!(!c.press("a", 2, 3, now + Duration::from_millis(150)));
    }
    #[test]
    fn motion_target_changes_input_and_timeout_cancel_pairing() {
        let now = Instant::now();
        for (pane, x, elapsed, cancel) in [
            ("b", 2, 100, false),
            ("a", 3, 100, false),
            ("a", 2, 401, false),
            ("a", 2, 100, true),
        ] {
            let mut c = Clicks::default();
            c.press("a", 2, 3, now);
            c.release("a", 2, 3, now);
            if cancel {
                c.clear();
            }
            assert!(!c.press(pane, x, 3, now + Duration::from_millis(elapsed)));
        }
        let mut c = Clicks::default();
        c.press("a", 2, 3, now);
        c.release("a", 3, 3, now);
        assert!(!c.press("a", 2, 3, now));
    }
}
