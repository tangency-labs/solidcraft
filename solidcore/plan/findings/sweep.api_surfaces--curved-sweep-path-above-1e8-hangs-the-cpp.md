# sweep.api_surfaces: a curved sweep path of radius 1e7 and above hangs the C++ under the 1e12 guard (HANG-5) (2026-10-31)

- **Unit:** `sweep.api_surfaces` (`SmApiCreateSweep`, op `sweep`).
- **Inputs:** constructed (follow-up of PR #1873): a unit `SmApiCreateCircle` profile swept
  along a quarter-arc `SmApiCreateArc` path of radius `1e5`, `1e6`, `3e6`, `1e7`, `1e8`,
  `1e9`; straight controls (a degree-1 NURB path and a collinear four-point
  `SmApiCreateCurve` path of length `1e10` and `1e12`). Each run alone through
  `difftest --fuzz-exec sweep.api_surfaces` under `timeout 30`.
- **Both sides (before):** the FFI's existing HANG-5 guard refused only a control-polygon
  length above `1e12` (or not finite), so all of these reached the C++. C++: 0.44 s, 4.8 s,
  26 s, then no result in 30 s for `1e7`, `1e8` and `1e9`; Rust: 0.04 s, 0.11 s, 0.38 s
  (not reached for the larger ones). Straight paths: 0.01 s on both sides. The runner's Rust
  side applied no guard at all (corpus `fuzz00746_cpp_hang` recorded `rust: ok` and still
  skipped the C++ side, which the `1e12` bound had refused since 2026-10-24).
- **Cause:** upstream HANG-5: `N_CreateSweepScale` polygonizes the path to `dTol / 2 =
  5e-6` before it relaxes the tolerance; see
  `plan/upstream-issues/HANG-5--addendum-2026-10-31-api-surfaces-curved-path-bound.md`.
- **Fix (harness, no kernel change):** `check_path_extent` (public) in the FFI keeps the
  `1e12` bound for every path and refuses a curved path (any but a degree-1 NURB; a line
  segment is refused as a path earlier) whose control-polygon length exceeds
  `MAX_PATH_EXTENT = 1e8`, the bound `sweep.api_brep` uses for its curve sweeps (PR #1873).
  The runner's Rust side calls it first, so both sides give
  `harness: invalid argument: sweep: trajectory too long for the fixed tolerance (HANG-5)`.
- **Bound choice:** generated paths stay below the bound (scale up to `1e6`; the gate is
  unchanged, see the PR). Curved paths between `3e6` and the bound are slow (26 s at a
  `3e6` quarter arc) but accepted, as in `sweep.api_brep`; the large fuzz mutation values
  (`±1e300`, `2^31`, `±2^63`) all exceed it. The measure is the control polygon of the stored
  control points (for a rational NURB, the weighted ones), as the earlier guard measured it.
- **Coverage:** regression inputs in `crates/difftest/tests/fuzz_corpus/sweep.api_surfaces.json`
  (`fuzz00746_cpp_hang` now runs both sides and expects the refusal; new
  `hang5_quarter_arc_path_radius_1e8`); runner test
  `huge_curved_sweep_path_is_refused_on_both_sides` (refusal of `fuzz00746` and of the
  radius-`1e8` arc, agreement of `fuzz00746` with its spike at `4.9e7`, control polygon
  `9.8e7`); FFI test `tests::huge_curved_path_is_refused` (no native call).
