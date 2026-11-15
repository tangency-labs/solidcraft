Updates: WR-499

#### WR-499 (update, 2026-11-14). `fillet_nx1_opposite_continuation_side_edge` is removed

WR-457's tangent side-edge skip in `FilletCorner::make_nx1_corner_topology` now always passes, once, the filleted edge's G1 continuation of the opposite convexity, as tangency-labs/smlib@420e47d. The skip itself stays behind `UpstreamFixes::fillet_nx1_tangent_side_edge`, which is still a flag; without it nothing changes. SolidCraft already ran with both on. Removed: the field `UpstreamFixes::fillet_nx1_opposite_continuation_side_edge`, SolidCraft's preset line, its entry in `fixes.json`, the flag-off branch, and every test setting of it (the issue test's case with WR-457 on and the continuation off, which pinned upstream's status 1001, with it).

- **Results:** none change in the goldens: no goldens unit's case turns on `fillet_nx1_tangent_side_edge`. The one fixture that does, `fixtures/solidcraft/3369-fillet-edge-on-tangent-seam.json` (`edge_ends_on_seam`, the `--solidcraft-cases` correctness report, not the gate), still passes against its closed-form volume: 9763.665662831 against 9763.665662773, off by 5.9e-8. The kernel rig's cases fillets/4174, 4365 and 4369 run with the SolidCraft preset, where both flags were on, so their path is unchanged.
- **Tests:** `crates/solidcore/tests/issue_1474_nx1_corner_tangent_round_end.rs`, `wr_468_overhanging_boss_fork_parity.rs`.
