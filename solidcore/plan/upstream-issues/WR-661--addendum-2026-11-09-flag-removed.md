Updates: WR-661

#### WR-661 (update, 2026-11-09). `fillet_corner_statuses and fillet_edge_curve_domain_statuses` is removed

Both flags are removed: nothing read them, since the fork's soft failures are the only path (`general--status-audit-483--addendum-2026-11-07-soft-statuses-by-default.md`). The fixes live in `CheckAndFixDegeneracy` / `sm_TestG1Continuity` (`crates/solidcore/src/blend/fillet_corner/`) and `sm_CheckEdgeCurveDomains` (`crates/solidcore/src/blend/fillet_solver/p1.rs`). Removed: both fields and every setting of them. Goldens: no case changed.
