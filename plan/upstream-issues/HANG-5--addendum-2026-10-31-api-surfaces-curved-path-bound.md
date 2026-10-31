Updates: HANG-5

#### HANG-5 (update, 2026-10-31). `SmApiCreateSweep` hangs on curved paths far below the overflow range; the `sweep.api_surfaces` FFI bounds curved paths at `1e8`
- **Trigger:** `SmApiCreateSweep` (fixed `dTol = 1e-5`, `SmApiSurfaces.cpp:326`) of a unit circle along a quarter-arc path (`SmApiCreateArc`, `0`..`90` degrees). `SmBSplineSurface::CreateSweepSurface` -> `N_CreateSweepScale` (`NL_SrfAdv.cpp:15516`) approximates the path with `N_ApproxCrvWithPolyline` within `dTol / 2` and only relaxes the tolerance (`d1 *= 10`) after a pass returns with more than `MAXPTS` points, so a long curved path makes the first pass run for minutes (its arrays grow by 150 points per reallocation). No overflow is needed: the 2026-10-20 record's triggers were path points at `±1e300`.
- **Timing (prebuilt v1.0.1 reference, x86-64, per call, shared machine at load 15-30):**

  | quarter-arc radius | C++ | Rust port |
  |---|---|---|
  | `1e5` | 0.44 s | 0.04 s |
  | `1e6` | 4.8 s | 0.11 s |
  | `3e6` | 26 s | 0.38 s |
  | `1e7` | no result in 30 s | not reached |
  | `1e8` | no result in 30 s | not reached |
  | `1e9` | no result in 30 s | not reached |

  The same growth as the `sweep.api_brep` curve sweeps (HANG-43 addendum of 2026-10-31: 3 s at `1e6`, 13.5 s at `1e7`, no result at `1e8`). Straight paths return at once at any length: a degree-1 NURB path and a four-point collinear `SmApiCreateCurve` path of length `1e10` and `1e12` take 0.01 s.
- **Rust port:** the `sweep.api_surfaces` FFI (`check_path_extent`, public) keeps refusing a `Sweep` path whose control-polygon length is not finite or exceeds `MAX_TRAJECTORY_LENGTH = 1e12`, and now also a curved one (any path but a degree-1 NURB) above `MAX_PATH_EXTENT = 1e8`, the `sweep.api_brep` bound; the runner's Rust side applies the same check (it had run these inputs), so both sides give `invalid argument: sweep: trajectory too long for the fixed tolerance (HANG-5)`. The 2026-10-20 trigger `fuzz00746` (path point `1e300`), refused by the C++ side's `1e12` bound since 2026-10-24 but still `cpp_skip` in the corpus, now runs both sides. Arcs between radius `3e6` and about `6e7` stay accepted (slow, bounded), like the `sweep.api_brep` arcs below its bound. Regression: `crates/difftest/tests/fuzz_corpus/sweep.api_surfaces.json`.
- **Confirmed:** yes (C++ timeouts above, each run alone under `timeout 30`).
