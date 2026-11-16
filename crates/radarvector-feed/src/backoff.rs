//! Refresh interval and retry backoff policy.

use std::time::Duration;

/// Never poll the data feed more often than this (VATSIM regenerates it ~every 15 s).
pub const MIN_INTERVAL: Duration = Duration::from_secs(15);
/// Default automatic refresh interval.
pub const DEFAULT_INTERVAL: Duration = Duration::from_secs(60);
/// Longest retry delay after repeated failures.
pub const MAX_BACKOFF: Duration = Duration::from_secs(300);

/// Clamp a user-chosen interval to the allowed minimum.
pub fn clamp_interval(d: Duration) -> Duration {
    d.max(MIN_INTERVAL)
}

/// Exponential backoff: 15 s, 30 s, 60 s, ... capped at 5 min.
#[derive(Debug, Clone, Default)]
pub struct Backoff {
    failures: u32,
}

impl Backoff {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn failures(&self) -> u32 {
        self.failures
    }
    /// Record a failure and return the delay before the next attempt.
    pub fn fail(&mut self) -> Duration {
        let exp = self.failures.min(10);
        self.failures = self.failures.saturating_add(1);
        MIN_INTERVAL.saturating_mul(1 << exp).min(MAX_BACKOFF)
    }
    pub fn succeed(&mut self) {
        self.failures = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sequence() {
        let mut b = Backoff::new();
        let s: Vec<u64> = (0..7).map(|_| b.fail().as_secs()).collect();
        assert_eq!(s, vec![15, 30, 60, 120, 240, 300, 300]);
        b.succeed();
        assert_eq!(b.fail().as_secs(), 15);
        assert_eq!(clamp_interval(Duration::from_secs(5)), MIN_INTERVAL);
    }
}
