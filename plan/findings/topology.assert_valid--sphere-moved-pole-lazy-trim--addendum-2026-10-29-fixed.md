# topology.assert_valid: moved sphere pole, lazily created pole trims diverge in ValidatePointers — fixed (2026-10-29)

Updates: plan/findings/topology.assert_valid--sphere-moved-pole-lazy-trim.md

- **Cause:** not the lazy trims. `SmApiCreateSphere` leaves the face surface with its
  trimmed surface cache (built while `MakeFaceTopology` made the face, editing enabled).
  The port's legacy `MakeFace` built the face topology on a `Clone` of the surface and
  then installed the original, and a `Clone` gets its own cache slot, so that cache was
  dropped with the copy. With the pole moved, `ValidatePointers` -> `CheckFace` ->
  `ComputeLoopOrientation` -> `sm_AreConnected` -> `ComputeUVPoint` falls back to the
  global point solve, which in Rust found no cache and built a new one; the new tree's
  `CreateUVTrimCurves(TRUE)` set the moved vertex's tolerance to `2 * gap` (0.2). The C++
  reuses the cache made during creation, so nothing grows, the UV walk fails and the 3D
  walk gives `SM_OT_UNKNOWN` (CheckFace tests 3/4), and edge test 61 fails.
- **Fix:** the legacy `MakeFace` copy now shares the original surface's cache slot
  (`Surface::clone_sharing_cache`, `topology::brep::p7`), as upstream builds the face on
  the one surface it keeps. Rust now reports the C++ lines.
- **Evidence:** `sphere_mutation6_capi` passes and its known-mismatch entry is removed;
  regression test `moved_sphere_pole_reports_upstream_lines` in
  `crates/solidcore/tests/issue_1419_brep_check.rs`.
