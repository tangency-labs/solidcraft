Updates: WR-704

#### WR-704 (update, 2026-11-17). `cone_iso_circle_at_axial_height` is removed

The fix is the default and its flag is gone (tangency-labs/solidcraft#14246, #18160, together with WR-713's `cone_unbounded_parameterized_as_nurb`). `Cone::create_iso_circle_from_point` (`surface/cone/p1.rs`) takes the circle's STEP height as the point's projection onto the axis, clamped to the height interval, with no inversion, and passes on the error of a failed `ConvertUVFromSTEPToNURBS`. Removed: the field `UpstreamFixes::cone_iso_circle_at_axial_height`, SolidCraft's preset line, its entry in `fixes.json`, and every test setting of it.

- **Results:** the goldens rows that move are counted with WR-713's addendum of the same date. Both flags were retired in one change, and the rows are certified together by `crates/goldens/tests/wr704_wr713_cone_moves.rs`. Each moved iso circle is checked by closed form: the circle lies on the cone at the point's axial height (or at the clamped end), its centre is on the axis and its radius is the cone's radius there.
- **Tests:** `crates/solidcore/tests/fork_1279539_cone_iso_circle_at_axial_height.rs` now asserts only the fixed behaviour.
