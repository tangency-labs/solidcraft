# topology.face_p2: a face surface with one rational control point kept the Rust side building the B-rep where the FFI refuses the C++ (nightly fuzz00020) (2026-10-31)

- **Unit:** `topology.face_p2` (op `best_uv` on a `face` B-rep source).
- **Inputs:** nightly fuzz case `topology.face_p2/fuzz00020` of run 41040730716
  (2026-10-30): base `extra00485`, mutations `/brep/surface/bspline/net/0/0/3 = 1.0`,
  `/cands/1/curve/cpts/0/2 = -1e+300`; a 5 x 3 degree (3, 1) B-spline face surface whose
  weights are the non-rational marker `-f64::MAX` (`-1.7976931348623157e308`, `NOW` in
  `solidcore_ffi::validate`) except control point (0, 0), now weight 1; `init_gaps`. The
  `-1.8e308` "weights" in the reproduction file are this marker (and the `1.8e308` z of
  the 2D candidate curves the `NOZ` marker), not fuzzed values. Verdict: `timeout`,
  `rust: no result after 20 s`.
- **Both sides:**
  - C++: not run. The FFI (`solidcore-ffi` `topology_face_p2::eval`, B-rep block
    `check_points`, `crates/solidcore-ffi/src/units/topology_face_p2.rs` line 181)
    refuses control points that do not share their rational marker before the export
    build: `invalid argument: surface: mixed rationality`. The runner reports it as the
    harness error `c++ export: invalid argument: ...`, so the fuzz driver runs the Rust
    side on the input on purpose.
  - Rust: the runner's own builder took the marker as a weight (a rational surface with
    weights 1 and -1.8e308) and, in the `init_gaps` walk (`vertex_face_gap` ->
    `compute_uv_point` -> `global_point_solve` -> the face's trimmed-surface cache), was
    still subdividing that cache (`TrimSrfCache::subdivide_node` /
    `implant_retained_edgeuse` / `CurveCache::tessellate`) after 10 minutes (replayed
    alone, difftest profile).
- **Cause:** the runner, not the port: the Rust side did not apply the FFI's input checks.
  The shim never builds this surface, so no missing upstream bound can be shown against
  the C++. (The same trimmed-surface-cache subdivision runs in
  `surface.trim_srf_cache/fuzz00295`; its depth is bounded by the base cache's documented
  bounds, but the node count on such a surface makes it effectively unbounded in time.
  Whether upstream does the same on these weights is not observable through the harness.)
- **Fix (harness, PR #502's convention, as #1358 and #1861):** the checks of
  `topology_face_p2::eval` are split out as `validate(op, path, input, counts)` (`eval`
  calls it before each shim call; its behaviour is identical up to an `Export` with
  trailing input, refused before rather than after the export build). The runner's Rust
  side applies it to the same input vectors `run_cpp` passes: a geometry op's arguments;
  a B-rep op's B-rep block before its build (`export: ...`, as the C++ export), then the
  resolved op's arguments against the Rust B-rep's own counts.
- **Regression:** `crates/difftest/tests/fuzz_corpus/topology.face_p2.json`
  (`fuzz00020_best_uv_mixed_rationality_surface`) and the runner test
  `mixed_rationality_is_refused_on_both_sides`. A gate fixture is not possible: a harness
  error always fails a case.
