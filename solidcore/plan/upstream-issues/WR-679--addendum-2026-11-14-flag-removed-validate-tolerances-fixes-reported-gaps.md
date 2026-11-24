Updates: WR-679

#### WR-679 (update, 2026-11-14). `validate_tolerances_fixes_reported_gaps` is removed; the update grows past half the tolerance

`SmBrep::ValidateAndUpdateTolerances` (`topology/brep/validation/assert_valid/brep.rs`) always counts a vertex/edge or
vertex/face gap over half the vertex tolerance as bad, the test its own report applies, and in update mode grows the
vertex to twice its larger gap (still within the existing growth cap). The flag is gone. Removed: the field,
SolidCraft's preset line, its entries in the kernel rig's `fixes.json` and in
`issue_2260_gn_pipe_refined_range_end.rs`, the flag-off branch, and the tests' flag settings.

- **Defaults:** SolidCraft's preset had the flag on, so the app's results don't change. `Kernel::default()` changes:
  the update fixes the half-tolerance gaps it reports, and `violated_or_updated` agrees with the report.
- **Goldens:** 5 rows of `topology.assert_valid` (`update_patch_sheet_*`) move. Each row's proof is in the removing PR.
- **Tests:** `tests/issue_2111_offset_patch_sheet_gaps.rs` runs on the default kernel; the loop over the flag's two
  values is removed and the expectations are unchanged.
