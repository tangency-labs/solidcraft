# PR #463: 27 Rust-vs-C++ mismatches newly reachable once `SmSurface::DropCurve` is wired (known mismatches, follow-ups)

Main's wiring of `SmFace::CreateUVTrimCurves` (`topology.face_p3`) and `SmCrvOnSurf` over
any surface (#554) brought about 1,860 more difftest cases up to `SmSurface::DropCurve`,
where main defers them. With `intersect.surface_drop_lib_p3` wired (PR #463, the unit is
green, 1529/1529) they run through; most pass, and the 27 cases below disagree with the
C++. Merges were frozen to land #463 with these listed in
`crates/difftest/known_mismatches.toml` (they still run and report as `known_mismatch`;
their units stay `partial`). Each group needs its own follow-up; the causes are not yet
traced. Values below are from CI run 39872546065 on #463.

### topology.edgeuse_p3: `EvaluateBinormal` state length (23 cases)

Cases: `adversarial_b33_eu33_binormal_out`, `review_Endcap_eu0_3d`, `review_Endcap_eu1_3d`, `review_Endcap_eu2_3d`, `review_Endcap_eu3_3d`, `review_Endcap_eu4_3d`, `review_Endcap_eu5_3d`, `review_Endcap_eu6_3d`, `review_Endcap_eu7_3d`, `upstream_files_b2_eu0_binormal_3d`, `upstream_files_b2_eu0_binormal_out`, `upstream_files_b2_eu1_binormal_3d`, `upstream_files_b2_eu1_binormal_out`, `upstream_files_b2_eu5_binormal_3d`, `upstream_files_b2_eu5_binormal_out`, `upstream_files_b2_eu9_binormal_3d`, `upstream_files_b2_eu9_binormal_out`, `upstream_files_b3_eu2_binormal_3d`, `upstream_files_b3_eu2_binormal_out`, `upstream_files_b3_eu7_binormal_3d`, `upstream_files_b3_eu7_binormal_out`, `upstream_files_b31_eu3_binormal_3d`, `upstream_files_b31_eu3_binormal_out`.

All 23 give `values.state.len: cpp=33 rust=44` (`review_Endcap_eu*_3d`
are `SmEdgeuse::EvaluateBinormal` with `pOptUVVector`, `SmEdgeuse.cpp:2740`): Rust records
more state than the C++, i.e. it continues (or reports more outputs) where the C++ stops.
Likely one cause in the binormal / UV-trim-curve path now that the drop succeeds.

### topology.face_p3: `cyl_bspline_uv_trim_plain`, `cyl_bspline_uv_trim_own` (2 cases)

`values.out[2]`: C++ `9.10934752180535e-17`, Rust `0` (`SmFace.cpp:7965`). A round-off
sized value that one side computes as exactly zero: an operation order or a skipped
step in the face UV-trim path on the B-spline cylinder fixture.

### blend.fillet_solver_p4: `gen0512` (1 case)

`values.g1.len: cpp=0 rust=1`: Rust produces a fillet geometry where the C++ produces none.

### boolean.merge_p2: `make_faceuse_sphere` (1 case)

`ints.topology.len: cpp=278 rust=274` (fixture `SmMerge.cpp:4367`, closed loop
orientation from tangents): the merged topology differs.
- `Surface::base_global_line_intersect` built its own cache (no caller cache) without
  `GetTree()`: a cache made while a boolean operation delays its tree (the primitive
  sweeps set `doing_boolean`) had no tree and the solve returned `SM_ERR_NULL_POINTER`
  where upstream's `pSurfaceCache->GetTree()` builds it. The fresh cache now goes through
  `held_cache` like the caller's, so the B-rep ray fire of a sphere's region split
  reaches the face-bound tree build (deferred, `surface.trim_srf_cache`) instead of
  failing, and `sweep.api_brep` `rotational_sweep_semicircle_sphere` no longer keeps a
  half-built analytic face (it defers rather than mismatch).
