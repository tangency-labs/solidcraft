#### Updates: WR-212 (2026-10-26). The closed-manifold refusal is now opt-in, off by default

- The Rust port's closed-manifold check at the end of `DoFillet` is now behind
  `Kernel::fixes.fillet_refuse_non_manifold_result` (off by default), following the
  opt-in policy for upstream fixes. By default `do_fillet` returns success as upstream
  does, and `SmApiFilletEdges` goes on to `StitchAndOrient`.
- **Why:** the unconditional check made the port disagree with the C++ on
  `Fillet/box_diff_cyl_big_rad.smb` edge 12 at radius 28 through `SmApiFilletEdges`.
  Both kernels leave a target that is not a closed manifold after `DoFilleting` (three
  lamina edges remain after the five stitch attempts, and `OrientTrimmedSurfaces`
  returns `SM_ERR_ASSERT_FAILURE`). The C++ then runs `StitchAndOrient`, which reverses
  the cylinder face, and returns `SM_SUCCESS`. The port refused with status 1001
  before `StitchAndOrient` and kept the unreversed face.
- **Code:** `FilletExecutive::do_fillet` in
  `crates/solidcore/src/blend/fillet_executive/p3.rs` and `FilletSession::do_fillet` in
  `crates/solidcore/src/blend/fillet_executive/session/finish.rs`.
- **Tests:** the refusal tests in `crates/solidcore/src/blend/fillet_executive/p3_tests.rs`
  now turn the fix on; `open_result_is_accepted_without_the_opt_in_check` covers the
  default.
