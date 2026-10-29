# topology.assert_valid: moved sphere pole, lazily created pole trims diverge in ValidatePointers (2026-10-29)

- **Unit:** found by `topology.assert_valid` (PR #1459); the divergence is in
  `topology.brep_validation` (`SmBrep::ValidatePointers` -> `SmTrimmingTools::CheckFace`
  -> `SmFace::ComputeLoopOrientation`), not in the AssertValid port.
- **Case:** `sphere_mutation6_capi` (`fixtures/topology.assert_valid/mutations.json`):
  `SmApiCreateSphere((0,0,0), 1)`, vertex 0 (the pole at (0,0,1)) moved by +0.1 in x
  without notification, then the capi sequence at level 2 (ValidatePointers, SM_NO_WALK).
- **C++:** three pointer reports: `SmTrimmingTools::Test #3` (face loop has bad
  orientation), `#4` (zero area) and `SmBrep::Test #61` (edge fails
  `ValidateGeometry(FALSE)`). AssertValid is skipped.
- **Rust:** one pointer report, `SmBrep::Test #82` (vertex tolerance 0.2 relative to
  brep tolerance 1e-5): the moved pole vertex's tolerance has grown from 1e-5 to 0.2.
- **Where:** the Rust sphere has no stored UV trim curve on two of its four edge uses
  (the pole edge uses). `face::p3::compute_loop_orientation` (`do_3d = false`) creates
  them through `get_or_create_uv_trim_curve`, which raises the vertex tolerance to 0.2,
  and then the UV walk returns `Same`. The C++ returns `Unknown` (CheckFace tests 3/4
  fail). With `do_3d = true` the Rust also returns `Unknown` and leaves the tolerance
  alone; `check_loop` alone leaves it alone too.
- **Suspected cause:** either the C++ primitive stores all four trims (so no lazy
  creation and the moved vertex breaks the UV connection), or C++ `CreateUVTrimCurve`
  does not grow the vertex tolerance here. Not yet resolved; a debug print in the shim
  that called `SmEdgeuse::GetUVTrimCurve()` before the mutation showed all four trims
  present, but that getter may itself create them lazily.
- **Status:** listed in `crates/difftest/known_mismatches/topology.assert_valid.toml`
  (a Rust divergence, no `cpp_defect`). The other 9 mutation kinds on the sphere and
  every unmutated body agree.
