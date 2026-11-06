use std::time::Duration;

pub(super) fn interval(since_input: Duration) -> Duration {
    Duration::from_millis(if since_input < Duration::from_millis(150) {
        8
    } else {
        25
    })
}

pub(super) fn input_wait(since_input: Duration, since_render: Duration) -> Duration {
    let input_poll = Duration::from_millis(if since_input < Duration::from_millis(150) {
        4
    } else {
        10
    });
    // Keep both polling budgets, but do not sleep past a frame already due.
    input_poll.min(interval(since_input).saturating_sub(since_render))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn input_wait_respects_frame_deadline_between_poll_ticks() {
        assert_eq!(
            input_wait(Duration::ZERO, Duration::from_micros(7500)),
            Duration::from_micros(500)
        );
        assert_eq!(
            input_wait(Duration::from_secs(1), Duration::from_micros(24500)),
            Duration::from_micros(500)
        );
    }

    #[test]
    fn due_or_late_frame_does_not_wait_for_another_input_tick() {
        assert_eq!(
            input_wait(Duration::ZERO, Duration::from_millis(8)),
            Duration::ZERO
        );
        assert_eq!(
            input_wait(Duration::from_secs(1), Duration::from_millis(30)),
            Duration::ZERO
        );
    }

    #[test]
    fn input_poll_and_quiet_transition_keep_existing_budgets() {
        assert_eq!(
            input_wait(Duration::ZERO, Duration::ZERO),
            Duration::from_millis(4)
        );
        assert_eq!(
            input_wait(Duration::from_secs(1), Duration::ZERO),
            Duration::from_millis(10)
        );
        assert_eq!(
            interval(Duration::from_micros(149999)),
            Duration::from_millis(8)
        );
        assert_eq!(
            interval(Duration::from_millis(150)),
            Duration::from_millis(25)
        );
    }
}
