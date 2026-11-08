# topology.loopuse: a B-rep block with a -1e300 surface control point hangs the C++ graph export (fuzz00839, HANG-29) (2026-10-31)

- **Unit:** `topology.loopuse` (the runner's C++ fixture: `topology.edgeuse_p1`'s `Export`).
- **Input:** 2026-10-20 fuzz sweep, `fuzz-out/verify2000/topology.loopuse/fuzz00839.json`:
  base `face30_007_collect` (op `collect`, `validate_only`, `init_gaps`, a rational (3, 1)
  B-spline face), mutation `/brep/surface/bspline/net/1/0/1 = -1e+300`. Verdict `timeout`,
  `cpp: no result after 20 s`; still open on main `d1c6fc48a`.
- **Both sides (before):** C++: no result in 30 s (reproduced alone), also with the value at
  `-1e155`; `-1e154` and `-1e150` return in under 0.1 s and agree. gdb places the hang in the
  graph export (`init_gaps`: `SmVertexuse::GetVertexFaceGap` -> surface cache ->
  `SmFace::CreateUVTrimCurves` -> ... -> `SmCurve::GlobalCoincidenceChecker`), before the op.
  The Rust side was never reached by the fuzz driver.
- **Cause:** upstream HANG-29. The 2026-10-20 sweep's guard in `topology_loopuse::eval` came
  after the runner's `topology_edgeuse_p1::eval(Export)` call, which has no magnitude check,
  so it was dropped (`general--fuzz-sweep-over-all-green-partial-units-2026-10-20.md`).
- **Fix (harness, no kernel change):** `solidcore_ffi::units::topology_loopuse::export`
  (block parse, `topology_edgeuse_p2::check_block_magnitudes`, then the `topology.edgeuse_p1`
  export) is what the runner's `graph_of` now calls, and `topology_loopuse::eval` checks the
  block before its own export. The runner's Rust side applies the same check, so both sides
  give a harness error (`invalid argument: B-rep value above 1e150 (HANG-29)`; the C++ side's
  message carries the runner's `c++ export:` prefix), as `topology.edgeuse_p3` does since
  PR #1873.
- **Coverage:** no fixture or generated case has a block value above `1e150`; the gate is
  unchanged (see the PR). Regression inputs: `crates/difftest/tests/fuzz_corpus/topology.loopuse.json`;
  runner test `huge_block_value_is_refused_on_both_sides` (refusal at `-1e300`, agreement
  at `-1e150`); FFI test `tests::magnitude_refusal_precedes_native_export` (no native call).
- **Record:** `plan/upstream-issues/HANG-29--addendum-2026-10-31-loopuse-export-guard.md`.
