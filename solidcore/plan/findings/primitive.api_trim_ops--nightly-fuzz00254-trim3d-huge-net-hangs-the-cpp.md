# primitive.api_trim_ops: trimming a lifted net with a 1e300 control point hangs the C++ (nightly fuzz00254, HANG-29) (2026-10-31)

- **Unit:** `primitive.api_trim_ops` (`SmApiTrimSurfaceWith3dCurves`, op `trim3d`).
- **Inputs:** nightly fuzz run 41040730716 (2026-10-30), `primitive.api_trim_ops/fuzz00254`:
  base `trim3d_net_surface` (4 x 4 flat net over 10 x 10, one radius-3 circle loop) with the
  mutations `/surface/points/1/2 = -9223372036854775808` and `/surface/points/8/1 = 1e+300`.
  Verdict: `timeout`, `cpp: no result after 20 s`.
- **Both sides (before):** C++: no result in 40 s (reproduced alone with
  `difftest --fuzz-exec`); gdb shows the HANG-29 recursion in
  `SmCurve::GlobalCoincidenceChecker` under `SmTrimmingTools::CreateUVTrimCurves` ->
  `SmEdgeuse::CreateUVTrimCurve`. Rust: never reached in the fuzz driver (it runs the Rust
  side only after the C++ side returns); run alone (main's runner, `--digest-dir`) it returns
  in about 1 s with status 0 and a one-face B-rep whose record holds `inf` values.
- **Cause:** upstream HANG-29 (squared distances overflow, the coincidence checker keeps
  splitting). Both mutations are needed: the 1e300 point alone (flat net) and the lifted
  point alone return at once. The onset is the overflow range: with the lifted point,
  `1e150` returns in 0.1 s, `1e154` in 6 s, `1e155` and above never.
- **Fix (harness, no kernel change):** the FFI refuses a trimmed surface (net or corner
  points) with a coordinate above `MAX_TRIM_SURFACE_COORD = 1e150`; the runner's Rust side
  refuses the same input through `ffi::validate`, so both sides report
  `rejected: ffi guard: invalid argument: trimmed surface coordinate above 1e150 (HANG-29)`.
  Trimming curves and the other three calls are not bounded (a `1e300` circle radius or
  centre on the lifted net, and `SmApiTrimProjectParallel` on the huge net, return at once).
- **Coverage:** no fixture or generated case has a surface coordinate above `1e150`; the
  unit's gate is unchanged (see the PR). Regression input:
  `crates/difftest/tests/fuzz_corpus/primitive.api_trim_ops.json`; runner test
  `huge_trimmed_surface_is_refused_on_both_sides` (refusal at `1e300`, agreement at `1e150`).
- **Record:** `plan/upstream-issues/HANG-29--addendum-2026-10-31-smapitrimsurfacewith3dcurves-trigger.md`.
