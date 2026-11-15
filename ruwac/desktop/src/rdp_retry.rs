//! Recovery timing includes connection setup, not just sleeps between attempts.
use std::time::Duration;

pub const FOREGROUND_WINDOW: Duration = Duration::from_secs(60);
pub const SETUP_LIMIT: Duration = Duration::from_secs(30);
const OFFLINE_INTERVAL: Duration = Duration::from_secs(15);
const OFFLINE_SETUP: Duration = Duration::from_secs(10);

#[derive(Debug)]
pub struct Plan {
    pub delay: Duration,
    pub setup: Duration,
    pub background: bool,
}

pub fn plan(elapsed: Duration, failures: u32) -> Plan {
    let remaining = FOREGROUND_WINDOW.saturating_sub(elapsed);
    if remaining < Duration::from_secs(1) {
        return Plan {
            delay: OFFLINE_INTERVAL,
            setup: OFFLINE_SETUP,
            background: true,
        };
    }
    let delay = Duration::from_millis(
        [500, 1000, 2000, 4000, 8000][failures.saturating_sub(1).min(4) as usize],
    )
    .min(remaining / 2);
    Plan {
        delay,
        setup: SETUP_LIMIT.min(remaining - delay),
        background: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn foreground_budget_includes_every_setup_and_delay() {
        let mut elapsed = SETUP_LIMIT;
        for failure in 1..20 {
            let next = plan(elapsed, failure);
            if next.background {
                assert!(elapsed <= FOREGROUND_WINDOW);
                assert!(elapsed >= FOREGROUND_WINDOW - Duration::from_secs(1));
                return;
            }
            elapsed += next.delay + next.setup;
            assert!(elapsed <= FOREGROUND_WINDOW);
        }
        panic!("A blackholed server must leave foreground recovery");
    }

    #[test]
    fn offline_recovery_continues_without_readding_a_profile() {
        for failures in [1, 9, 100, u32::MAX] {
            let next = plan(Duration::from_secs(600), failures);
            assert!(next.background);
            assert_eq!(next.delay, Duration::from_secs(15));
            assert_eq!(next.setup, Duration::from_secs(10));
        }
    }

    #[test]
    fn fast_refusals_get_bounded_backoff_and_fresh_outages_start_fast() {
        assert_eq!(plan(Duration::ZERO, 1).delay, Duration::from_millis(500));
        assert_eq!(plan(Duration::ZERO, 100).delay, Duration::from_secs(8));
        let next = plan(Duration::from_millis(59900), 9);
        assert!(next.background);
        assert_eq!(plan(Duration::ZERO, 1).setup, SETUP_LIMIT);
    }
}
