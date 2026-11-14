Updates: WR-446

#### WR-446 (update, 2026-11-14). `validation_does_not_make_uv_trims` is removed; validation makes no UV trims

`sm_CheckEdgeuseGeometry` (`topology/brep/validation/pointers.rs`) always evaluates the clockwise neighbour with
`ok_to_make_uv_trim_curve = FALSE`, so `ValidatePointers` no longer stores a new UV trim curve on a neighbour that had
none. The 3D evaluation, its drop onto the existing face and every status are kept. The flag is gone. Removed: the
field, SolidCraft's preset line, its entries in the kernel rig's `fixes.json` and in
`issue_2260_gn_pipe_refined_range_end.rs`, and the flag-off argument.

- **Defaults:** SolidCraft's preset had the flag on, so the app's results don't change. `Kernel::default()` changes:
  a check (for example `SmBrepConstructor::EndShell(TRUE)`) no longer adds trims to the body it checks.
- **Goldens:** 3 rows of `topology.brep_constructor` (`fan_four_shared_stitched`, `fan_three_shared_stitched`,
  `gen0011`) move. Each row's proof is in the removing PR.
- **Tests:** the `wr446_tests` module (`pointers.rs`) runs on the default kernel. The witness that upstream's
  evaluation creates the chord's trim is kept by running that evaluation directly; the check's run asserts that no
  trim is made or removed and that the existing arc trims are unchanged.
