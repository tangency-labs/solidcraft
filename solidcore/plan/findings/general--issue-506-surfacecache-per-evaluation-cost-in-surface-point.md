# Issue #506: SurfaceCache per-evaluation cost in surface_point_solve_p2 (fix/point-solve-perf, 2026-10-24)

Reproduction note: the original `fuzz00474` reproduction file (`fuzz-out/n2000/`, not
committed) is not available; the deterministic mutation numbering has drifted since the
sweep (the unit's base-case list changed), so the regenerated `fuzz00474` runs in 0.02 s
on both sides. The regime is instead reproduced with constructed near-degenerate cases
(3 x 5 rational B-spline spanning `1e-4`, `wrap = true`, `solver = 10`, smallest v-knot
span `2.5e-5`), run through `FUZZ_CORPUS_DIR` (both sides agree, as required):

| op | C++ | Rust |
|---|---|---|
| rough | 51.0 s | 35.5 s |
| push | ~40 s | ~40 s |
| norms | ~43 s | ~43 s |
| adjust | ~39 s | ~38 s |
| solve | ~68 s | ~67 s |

The port's sample profile (macOS `sample`, the Rust phase of `rough`, 10 s) puts the
Rust time in `surface::surface_cache::{p1::compute_bezier_approximation_memo,
p3::net_constants, p3::patch_boxes}` -> `evaluate_point`/`derivs`/`eval()` ->
`nurbs::nurbs_srf::compute_surf_constants` over the subdivision tree, i.e. the same
structure the 2026-10-21 entry ("Fuzz 'Rust-only' hangs") measured: identical call
counts on both sides, the gap is per-evaluation cost, not more work. On this
constructed case the C++ is actually slower (51 s to 35 s); the exact original input
made C++ 11 s / Rust 21 s, so the levers below are checked against bit-identical
output rather than against wall-clock parity on every input.

The first candidate - the memo being ineffective, which is what the profile suggested
- is excluded. `compute_bezier_approximation_memo`'s `PoleMemo` is per cache, not per node:
`SurfaceCache::pole_memo` is built once and passed by reference through every
`SubdivideNode`/`SubdivideNodeSplit` corner call (`split_node`, `p3.rs`, both `d1` and
`d2` children share it), and `PoleMemo::poles` recomputes `GetSingularities` only while
the memo is still `Empty`. It hits on every repeated corner evaluation of the
subdivision tree. Upstream is worse, not better: `sm_ComputeBezierApproximation`
(`SmSurfaceCache.cpp:707/754/800/846`) calls `SmSurface::IsSingularity(uv, eSingDir)`
without pole points, so every corner whose direction is still `SM_SP_NEITHER`
recomputes `GetSingularities(&poles)` inside `IsSingularity`
(`SmSurface.cpp:23239-23242`). The port is deliberately faster here (a documented
performance-only deviation that returns identical values), which is why the
constructed cases put the C++ side behind: 51.0/35.5 s (`rough`), 22.7/11.2 ms
(`scratch_flat2`), 42.2/26.3 s (`scratch_s1e5`). Levers checked, per the plan:

| lever | observed evidence | expected speedup | bit-identity risk | verdict |
|---|---|---|---|---|
| `PoleMemo` effectiveness | computes `GetSingularities` once per cache and hits on every corner of every node; upstream recomputes it per corner (`SM_SP_NEITHER` path); all constructed cases C++ >= Rust | already realised | none (existing, values identical by construction) | no change needed |
| duplicate evaluation dedup in `compute_surf_constants` (chord phase and angle phase both evaluate at `at(0)` of each row) | only reached with a non-B-spline `surface_to_test`; in the reproduced regime `net_constants` reads nets / Bezier patches (`compute_net_constants`, no evaluations) | <= 5% where applicable, 0 here | low (same values, cached recomputation) | not applicable to this workload |
| allocation churn in `evaluate_point`/`derivs`/`eval` | micro-benchmark on the constructed 3 x 5 rational surface (20000 iterations): `evaluate(uv, 1, 1)` = 0.54 us per call, `evaluate_point` = 0.072 us; the NLib-level paths already keep the basis arrays on the stack (`basis_eval_into`, `derivative_matrix::<SMALL_NP>`), so per-eval cost is dominated by the derivative algorithm itself, which both sides run identically | <= 10-20% best case, speculative | restructure of NLib internals (`N_SrfDerivs` equivalents) | not implemented: no evidence, only risk |
| patch-invariant recomputation per evaluation (`net_constants` / `compute_surf_constants`) | `test_against_tolerances` calls `net_constants` at most twice per node (chord heights only when the pseudo-box fails, angles only when not already subdividing), as upstream | 0 | none | no recomputation asymmetry found |
| bounds-check / `Result` threading overhead (`rd`/`sx`/`ux`/`get`/`at_u` helpers) | pervasive in the NLib leaves, but every reproduced configuration has the C++ side slower; removing the checks is `unsafe`/unchecked indexing, denied by the workspace lints | unknown | forbidden by repo rules (no `unsafe`, no unchecked `[i]`) | not implementable |

No lever shows a measurable, bit-identical improvement on any reproducible
configuration; the practice between the port's cache and upstream's is the opposite of
the profiled hypothesis (the port's memo is strictly cheaper). The remaining unexplained
gap is the unreproducible `fuzz00474` 11 s / 21 s direction, which none of the five
regeneration attempts (scale `1e-5`, knot span `1e-7`, tight `1e-13`, `1e-6` scale,
amplified non-polar nets, all with `FUZZ_CORPUS_DIR` agreement - `scratch_s1e5` 42.2/26.3
s, `scratch_vknot` 42.3/26.3 s, `scratch_t13` 25.9 s Rust-only) reproduces; #506 stays
open upstream with this evidence.
