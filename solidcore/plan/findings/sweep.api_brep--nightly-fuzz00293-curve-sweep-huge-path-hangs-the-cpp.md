# sweep.api_brep: a curve sweep along a path with a 1e300 point hangs the C++ (nightly fuzz00293, HANG-43 / HANG-5) (2026-10-31)

- **Unit:** `sweep.api_brep` (`SmApiCreateCurveSweepFromFaces`, op `from_faces`).
- **Inputs:** nightly fuzz run 41040730716 (2026-10-30), `sweep.api_brep/fuzz00293`: base
  `gen0003` (one disc of radius 2.39 swept along a four-point `SmApiCreateCurve` path, a line
  scale curve, a four-point scale reference, `NULL` options) with the mutation
  `/sweep/path/points/2/2 = 1e+300`. Verdict: `timeout`, `cpp: no result after 20 s`.
- **Both sides (before):** C++: no result in 30 s (reproduced alone); gdb shows
  `N_CreateSweepScale` -> `N_ApproxCrvWithPolyline` (the path polyline within
  `SM_ZONE_TOL_3D / 2`). Rust: not reached in the fuzz driver on main; run alone (main's
  runner, `--digest-dir`) it returns status 1001 in under 1 s.
- **Cause:** upstream HANG-43, now located: the HANG-5 polyline approximation of the sweep
  trajectory does not return for a path that dwarfs the fixed tolerance. The spike regime is
  erratic in the magnitude (`1e15`..`1e30` return in 0.02 s, `1e40`..`1e80` and
  `1e140`..`1e300` do not); the curvature regime is smooth (an arc path of radius `1e6`
  takes 3 s, `1e7` 13.5 s, `1e8` and above more than 25 s). Timings in the addendum.
- **Fix (harness, no kernel change):** the FFI refuses a curve-sweep path other than a line
  segment whose extent exceeds `MAX_PATH_EXTENT = 1e8` (`check_path_extent`, applied in
  `push_sweep_args`, so `CurveSweep`, `FromFaces`, `FromEdges`, `FromBoxFace` and the
  attributed ops); the runner's Rust side applies the same check, so both sides give
  `harness: invalid argument: curve sweep: path extent above 1e8 for the fixed tolerance (HANG-5)`.
  Line paths (degree 1, never subdivided) take any size.
- **Bound choice:** fixtures reach a path extent of about 10, generated cases about `1e7`
  (`gen_scale` up to `1e6` times a length factor up to 10), so `1e8` keeps every existing
  case; arcs between `1e7` and `5e7` radius are slow (13.5 s at `1e7`) but accepted, like
  the slow sub-overflow inputs of HANG-29. The large fuzz mutation values (`±1e300`, `2^31`, `±2^63`) on a
  path coordinate or radius all exceed the bound.
- **Coverage:** the gate is unchanged (see the PR). Regression inputs:
  `crates/difftest/tests/fuzz_corpus/sweep.api_brep.json` (new `fuzz00293_from_faces_path_1e300`;
  `fuzz01218_cpp_hang` and `fuzz01458_cpp_hang`, HANG-43's original inputs, no longer skip the
  C++ side); runner test `huge_curve_sweep_path_is_refused_on_both_sides` (refusal at
  `1e300`, agreement at `1e7`); FFI test `huge_curved_path_is_refused`.
- **Not covered:** `SmApiCreatePipeSweep` with a `1e300` path point still crashes the C++
  (SIGFPE, UB-232, corpus `fuzz01378_cpp_crash`); the planar-path sweep is not guarded.
  `sweep.api_surfaces`' HANG-5 guard (control polygon above `1e12`) admits arc paths of
  radius `1e8`..`1e12`, which hang here; not checked in that unit.
- **Record:** `plan/upstream-issues/HANG-43--addendum-2026-10-31-located-in-n-createsweepscale-polyline.md`.
