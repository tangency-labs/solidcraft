# surface.trim_srf_cache: a surface with one rational control point kept the Rust side building the cache where the FFI refuses the C++ (nightly fuzz00295) (2026-10-31)

- **Unit:** `surface.trim_srf_cache` (op `face`: `TrimSrfCache::build_tree_with_face` on a
  constructed face).
- **Inputs:** nightly fuzz case `surface.trim_srf_cache/fuzz00295` of run 41040730716
  (2026-10-30): base `gen0572`, mutation `/srf/srf/net/0/0/3 = 1.0`; a 5 x 2 degree (3, 1)
  B-spline surface whose weights are the non-rational marker `-f64::MAX`
  (`-1.7976931348623157e308`, `NOW` in `solidcore_ffi::validate`) except control point
  (0, 0), now weight 1; `build: true`. The `-1.8e308` "weights" in the reproduction file
  are this marker, not fuzzed values. Verdict: `timeout`, `rust: no result after 20 s`.
- **Both sides:**
  - C++: not run. The FFI (`solidcore-ffi` `surface_trim_srf_cache::validate`,
    `check_points`, `crates/solidcore-ffi/src/units/surface_trim_srf_cache.rs` line 176)
    refuses control points that do not share their rational marker: `invalid argument:
    surface: mixed rationality`. The runner reports it as a harness error, so the fuzz
    driver runs the Rust side on the input on purpose.
  - Rust: the runner's surface builder took the marker as a weight (a rational surface
    with weights 1 and -1.8e308) and built the face's trimmed-surface cache; it was still
    running at 60 s (replayed alone), subdividing (`subdivide_node` /
    `implant_retained_edgeuse` / `surface_singularities`, evaluating the rational surface)
    on those overflowing weights.
- **Cause:** the runner, not the port: the Rust side did not apply the FFI's input checks.
  The input is not a surface upstream is ever given here (the shim never builds it), so no
  missing upstream bound can be shown against the C++. (The subdivision depth is bounded
  by the base cache's documented bounds, but the node count on such a surface makes it
  effectively unbounded in time; `topology.face_p2/fuzz00020` reaches the same
  subdivision through its `init_gaps` walk. Whether upstream does the same on these
  weights is not observable through the harness.)
- **Fix (harness, PR #502's convention, as #1358 and #1861):** the runner's Rust side
  applies `surface_trim_srf_cache::validate` (already public: `eval` calls it) to the same
  input vector `cpp` passes, so the case is a harness error on both sides. An input
  `cpp_input` cannot encode keeps its Rust-side error, as before; the `native_face` and
  `poly_edges` sub-runners are unchanged.
- **Regression:** `crates/difftest/tests/fuzz_corpus/surface.trim_srf_cache.json`
  (`fuzz00295_mixed_rationality_face`) and the runner test
  `mixed_rationality_is_refused_on_both_sides`. A gate fixture is not possible: a harness
  error always fails a case.
