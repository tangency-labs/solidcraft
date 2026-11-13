//! Conservative work quanta selected from actual submission completion latency.
//! This is a bounded controller, not a guarantee about a driver's scheduling deadline.
use std::time::Duration;

pub(super) const MIN_PATHS: u32 = 64;
pub(super) const MAX_PATHS: u32 = 16_384;
const TARGET: Duration = Duration::from_millis(20);

#[derive(Clone, Copy, Debug)]
pub(super) struct Budget {
    paths: u32,
}
impl Default for Budget {
    fn default() -> Self {
        Self { paths: MIN_PATHS }
    }
}
impl Budget {
    pub(super) fn count(&self, remaining: u32) -> u32 {
        remaining.min(self.paths)
    }
    pub(super) fn observe(&mut self, completed: u32, elapsed: Duration) {
        if completed == 0 {
            return;
        }
        let ideal = u128::from(completed).saturating_mul(TARGET.as_nanos()) / elapsed.as_nanos().max(1);
        let bounded = ideal.clamp(u128::from(MIN_PATHS), u128::from(MAX_PATHS));
        let requested = u32::try_from(bounded).unwrap_or(MIN_PATHS);
        // Never jump from the first tiny/fast dispatch to a large unmeasured one.
        self.paths = requested.min(self.paths.saturating_mul(2)).clamp(MIN_PATHS, MAX_PATHS);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cold_dispatch_starts_small_and_growth_is_bounded() {
        let mut budget = Budget::default();
        assert_eq!(budget.count(u32::MAX), 64);
        for _ in 0..100 {
            let before = budget.count(u32::MAX);
            budget.observe(before, Duration::from_nanos(1));
            let after = budget.count(u32::MAX);
            assert!(after <= before * 2 && after <= MAX_PATHS);
        }
        assert_eq!(budget.count(u32::MAX), MAX_PATHS);
        budget.observe(MAX_PATHS, Duration::from_secs(10));
        assert_eq!(budget.count(u32::MAX), MIN_PATHS);
        assert_eq!(budget.count(17), 17);
        assert_eq!(budget.count(0), 0);
    }
    #[test]
    fn observed_cost_controls_the_quantum_without_overflow_or_zero_work() {
        let mut budget = Budget::default();
        for _ in 0..10 {
            budget.observe(budget.paths, Duration::from_millis(1));
        }
        let before = budget.paths;
        budget.observe(before, Duration::from_millis(80));
        assert_eq!(budget.paths, before / 4);
        let unchanged = budget.paths;
        budget.observe(0, Duration::ZERO);
        assert_eq!(budget.paths, unchanged);
        budget.observe(u32::MAX, Duration::MAX);
        assert_eq!(budget.paths, MIN_PATHS);
        budget.observe(u32::MAX, Duration::ZERO);
        assert_eq!(budget.paths, MIN_PATHS * 2);
    }
}
