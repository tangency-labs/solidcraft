# Issue #504: library bounds for the Rust-side fuzz hangs (fix/bounded-loops-504, 2026-10-22)

The 21 inputs of #504 run on the Rust side alone (runner guard bypassed locally),
with the loops instrumented and the maxima over a full `--green` (upstream fixtures
and committed cases) recorded:

| regime | bound (`InvalidInput` past it) | fixtures' max | corpus inputs, before -> after |
|---|---|---|---|
| `SmFindPCExtremaEFO::Evaluate` scale loop (HANG-47) | `MAX_PC_SCALE_STEPS = 400` (provable: finite needs <= 310) | - | curve_p9 fuzz00144/01222: never -> 0.00 s, no solution |
| `GlobalCoincidenceChecker` calls (HANG-48) | `MAX_COINCIDENCE_CALLS = 2^18` | 15,139 calls, depth 32 | curve_p8 fuzz01456/01856, surface_p8 fuzz01603: never -> 8-9 s, status 1007 |
| `SolveBranch` visits per solve | `MAX_SOLVE_BRANCHES = 2^20` | 56,753 | curve_p9 fuzz01043: never -> 11 s, the 20 solutions found by then (status ignored, as upstream) |
| `CurveCache::Tessellate` halves | `MAX_TESS_SPLITS = 2^16` | 4,024 | none: the default `min_parametric_ratio` caps these at about 1.5e4 |
| `N_ApproxNurbsWithNonRatCrv` samples (HANG-49) | `MAX_APPROX_SAMPLES = 2^17` | 11,104 | nlib.crv_approx fuzz05147: never -> 0.00 s |

- Results below the bounds are unchanged: a full `--green` (Mac) gives case-for-case
  identical reports with and without the bounds; the touched units on Linux pass with
  the same counts and `max_rel_err` 0, and `--perturb` detects every case.
- **Not hangs, slow:** composite_curve_p2 fuzz00198/00435/01111/01988 and
  fuzz00430/01007 finish in 23-171 s: `IntersectAndTrimOffsets` intersects every pair
  of offsets, and the port rebuilds both curve caches for each pair (upstream keeps
  them on the curves); each tessellation stays near the `min_parametric_ratio` cap.
  surface_point_solve_p1 fuzz00979 finishes in 73 s: the `SurfaceCache`
  approximation tree reaches 262,141 nodes, as a committed fixture does (and the
  B-spline fixtures reach `MAX_SUBDIVISION_NODES`), so no bound 10x above the
  fixtures would stop it sooner (p2 fuzz01057 and p3 fuzz00349 build the same kind
  of tree, about 1e5 nodes after 10 s; not run to completion). Unchanged.
