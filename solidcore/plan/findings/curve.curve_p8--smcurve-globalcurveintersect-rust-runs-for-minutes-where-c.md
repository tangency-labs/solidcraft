# curve.curve_p8 (`SmCurve::GlobalCurveIntersect`): Rust runs for minutes where C++ returns at once on a 1e300 control point (found by `offset.offset_geometry_creation`'s fuzz, 2026-10-20)

Draws of `offset.offset_geometry_creation`'s fuzz, each a `trim_extended_edge` script with
one control point of an `add_curve` 2D cubic moved to about 1e300, where the Rust
`global_curve_intersect` is far slower than the C++ `GlobalCurveIntersect` on the same input:
`fuzz01278` and `fuzz00405` below, `fuzz01915` (`gen0330`, 29 s, the stack of instance 2;
in the unit's fuzz paragraph), and `fuzz00504`, recorded under `curve.curve_p5` ("Cost of
rebuilding the curve cache per call", PR #149). **Shared explanation:** `curve.curve_p5`'s Caches deviation, where
`make_curve_cache` builds a fresh curve cache for every caller while upstream's
`CacheMakeOrValidate` keeps it on the curve until the curve changes. A 1e300 control point
makes the cache's tessellation (`CurveCache::tessellate` -> `curve_cache::compute_constants`
-> `LocalSolve1d::solve_it::<FindCurveDistanceExtrema>` -> `BSplineCurve::evaluate`, in
`curve.curve_cache`) expensive, and the port pays it on every intersection call where C++
pays it once per curve. Neither instance is an upstream defect (no UB/HANG id) and neither is
guarded in `offset.offset_geometry_creation`; the fix belongs to the curve cache's owners.

**Instance 1, `fuzz01278` (minutes; a divergence beyond the cost).**

- **Inputs.** `offset.offset_geometry_creation` fuzz draw `fuzz01278` (base `gen0332` on
  `disk.smb`, mutation `/ops/0/curve/cpts/3/1 = 1e+300`): a 2D polynomial cubic (one span,
  control points about `(-0.01, 0.51)`, `(-0.96, 0.51)`, `(0.58, 0.26)`, `(0.80, 1e300)`) added
  to the store, then `TrimExtendedEdge`, whose UV tangent curve is intersected with that
  curve (`pTanUV->GlobalCurveIntersect(.., *pCurveExtended2, ..)`, cpp:1202; port
  `offset_geometry_creation.rs` `tan_curve.global_curve_intersect`).
- **C++.** `TrimExtendedEdge` returns status 0 in under a second. Under gdb (hardware
  breakpoints on `SmCurve::GlobalCurveIntersect`, `SmGlobalSolver::SolveIt` and
  `SmCurve::GlobalCoincidenceChecker`), each `GlobalCurveIntersect` of the op is followed by
  one `SolveIt` (the `SM_SO_INTERSECTION_TEST`) and then the next `GlobalCurveIntersect`:
  the coincidence checker is never entered from it, so the test finds no leaf pair (or the
  single-pair local solve finishes).
- **Rust.** `global_curve_intersect`'s `IntersectionTest` returns one solution with
  `param(6) = 11` leaf pairs, so after the local solve it calls
  `global_coincidence_checker`, where the 1e300 curve's `is_split_needed` is true on every
  sub-interval: its tangent's `length_squared()` overflows to `inf`, `unitize` makes the polar
  box's first basis vector `(0, 0, 0)` (`x / inf`), every later tangent reads as parallel and
  opposite (`cross` length 0, `dot` 0 < `EFF_ZERO`), the box is `Unbounded`, and the checker
  splits in three until the interval is below `ZONE_TOL_3D / 10`: 16 levels, 3^16 leaves,
  each with bounding boxes, polar boxes and evaluations; `MAX_COINCIDENCE_DEPTH` (200) never
  stops it. Measured > 180 s; the fuzz budget is 20 s a side.
- **Suspected cause.** The shared explanation above makes every cache build on the 1e300
  curve expensive, but this instance also enters a path C++ does not: the curve-cache trees
  or the `SmCCIGlobalSolver` intersection test disagree on the 1e300 span (the same
  arithmetic in the polar box would give the same `Unbounded` box on the C++ side, so the
  difference is before the checker). Not located within the time box; the C++ tree / solver
  were not instrumented.
- **Status.** Open. Reproduce with the fuzz output file as a fixture (`fuzz01278` above).

**Instance 2, `fuzz00405` (36 to 41 s against 5 s; the cost alone).**

- **Inputs.** Fuzz draw `fuzz00405` (base `gen0650` on `surf_sphere150.smb`, `dist 100`,
  mutations `/ops/0/curve/cpts/3/0 = -1e+300` and `/approx = -1`; the script is
  `add_curve`, `trim_extended_edge`, `add_curve_from`, `getters`). The FFI refuses the
  negative tolerance, which the runner now mirrors; the slow path needs only the control
  point.
- **Timings** (reviewer's, pod sc-builder-7, the script with the mutated point at each
  tolerance): `approx = -1` C++ refused in 0.03 s, Rust 36 to 40 s; `approx` at `0`,
  `1e-300`, `1e-12`, `1e-6`, `1e-4`, `1e-2`: C++ 4.4 to 5.2 s, Rust 36 to 41 s, the values
  equal; with the point back in range (`x = 0.5`) both sides return in 0.02 s.
- **Stack** (gdb on the Rust side, 8 s and 22 s in, both samples the same):
  `OffsetGeometryCreation::trim_extended_edge` -> `Curve::global_curve_intersect` ->
  `curve::curve::p5::make_curve_cache` -> `CurveCache::tessellate` ->
  `curve_cache::compute_constants` -> `LocalSolve1d::solve_it::<FindCurveDistanceExtrema>` ->
  `Curve::evaluate` -> `BSplineCurve::evaluate` -> `nurbs_crv::crv_derivs`: the cache
  tessellation, before any intersection test, as in `fuzz00504`.
- **Status.** Open, with `fuzz00504`. Reproduce with the fuzz output file as a fixture and
  the tolerance set to any accepted value.
- **Difftest `graph()` compared arena records upstream deletes (topology.brep_p3, PR #204).** The shared
  difftest helper `graph()` / `Canon::of` (`crates/difftest/src/units/boolean_merge_p1.rs`, used by
  several runners) counted raw arena lengths and appended the records its walk did not reach, while the
  C++ side is a fresh import of the shim's export walk, which emits only live objects. Upstream deletes
  records the Rust arenas keep, unlinked (topology.brep_p2's documented deviation: "deleted records ...
  stay in the arena, unlinked", e.g. `MakeEdgeVertexTopology` deleting a vertex shell's vertex use,
  SmBrep.cpp:13850). It surfaced when #339 wired `GetInfiniteRegion` into topology.brep_p5, so
  `CreateWireEdgeFromCurve` (op 13) ran for the first time: 5 vertex uses vs 4, geometry 29 vs 27, graph
  57 vs 54, with equal topology. `graph()` now numbers, compares and counts only the records the walk
  reaches from the B-rep's roots (regions and edge list, as the export walk): reachability is the
  definition of the B-rep's content upstream, and a record that is wrongly still linked is reached and
  compared. The unreached count is a Rust-side report diagnostic (`diagnostics.unreached_records`,
  never compared) so leaks stay visible.
