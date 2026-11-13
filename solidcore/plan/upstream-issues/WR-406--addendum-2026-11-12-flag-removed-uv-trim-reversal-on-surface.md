Updates: WR-406

#### WR-406 (update, 2026-11-12). `uv_trim_reversal_on_surface` is removed

The fix is the default and its flag is gone (tangency-labs/solidcraft#14246, batch A15a, which retires four topology flags). `validate_pointers` (`topology/brep/validation/pointers.rs`) always judges a UV trim's end turns on its face's surface.

- **Tests:** `issue_891_kernel_fix_keeps_the_exact_trim` and the WR-446 tests in `pointers.rs` and `wr406_uv_trim_reversal_check.rs` run on `Kernel::default()`; the tests of the raw (u, v) straightening (`validate_pointers_straightens_the_exact_trim_by_default` and the off half of `wr406_bored_plate_check_reverses_or_keeps_the_hole_trims`, now `wr406_bored_plate_check_keeps_the_hole_trims`) are gone.
- **Results:** the batch survey puts the flag in group A, and the full gate at the retiring change confirms it: no goldens row and no kernel rig case changes.
