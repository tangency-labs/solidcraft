Updates: WR-260

#### WR-260 (update, 2026-11-12). `edge_reversal_keeps_failed_edge` is removed

The fix is the default and its flag is gone (tangency-labs/solidcraft#14246, batch A15a, which retires four topology flags). `reverse_orientation` (`topology/edge.rs`) returns a failed curve reversal's error before changing the interval or the edge uses, and an edge without curve keeps its interval.

- **Tests:** `issue_521_failed_reversal_preserves_edge_interval_uses_and_trims`, `reversing_an_edge_without_curve_keeps_its_interval` and `wr260_failed_reversal_observable.rs` run on `Kernel::default()`; `failed_reversal_installs_default_interval_by_default` and the default half of the public box test are gone.
- **Results:** the batch survey puts the flag in group A, and the full gate at the retiring change confirms it: no goldens row and no kernel rig case changes.
