# blend.fillet_geom_p6 (`SmFilletGeom.cpp` part 6, PR #247, 2026-10-19; partial)

- **`sm_IntersectAndSplit` handle (fixed, unit `surface.surface_p2`).** `intersect_and_split`
  ran `GlobalPropertyAnalysis` with no curve handle. Upstream's `LocalSolve` names the curve
  as each solution's object, so `IdenticalSolutions` scales its duplicate test by the curve's
  interval (`dScaledZero` at least 1e-4) and keeps the root with the smaller residual. With
  no object the port's scale was about 1e-10, so two roots ~1e-10 apart both survived and
  `insert_split` kept the smaller parameter. `LiftCurve` then split UV curves at C0 surface
  knot lines ~1e-10 off upstream, and every interior knot of the joined 3D curve moved by
  ~3e-10 relative (control points up to 1.4e-7). Found by this unit's `CalcFilletEnd` cases
  (1 of 3000 generated); `LiftCurve` had never been compared by a difftest before
  (`surface.surface_p11` has no runner for it). Fixed on main by #444 (the solver solutions
  name their operands; `intersect_and_split` takes the curve's handle), which this PR now
  uses in place of its own equivalent fix.
- **Analytic target geometry.** Ops that evaluate a target face surface or edge curve of an
  analytic kind are deferred under `harness:analytic-geometry-export` (the shared B-rep
  export carries those as their B-spline part; see the harness entry above). Before the
  deferral, `CalcBlendingRail` on `vase_rev.smb` faces differed by ~1e-13 at the end points
  dropped onto the face (`GlobalPointSolve` on the surface of revolution's B-spline part vs
  the analytic surface), and on `cone.smb` / `box_with_large_fillet.smb` faces `LiftCurve`
  produced curves of different sizes (909 vs 4650 control points).
- **Casts.** `SM_CAST_PTR(SmBSplineCurve, ...)` keeps `SmLine`, `SmEllipse` / `SmCircle`,
  `SmParabola` and `SmHyperbola` (all `SmBSplineCurve` subclasses); the port tests
  `Curve::is_kind_of(BSPLINE_CURVE_TYPE)` (a `CurveKind::BSpline` match missed line edges
  in `CalcEdgeOnEdge`). `blend::fillet_geom::p3::calc_fillet_int_fillet`'s
  `has_3d_curve` still matches `CurveKind::BSpline` only: a line or conic fillet/fillet
  curve takes the `sm_SrfSrfIntersection` path there, where upstream trims it (not
  reached by p3's difftest, which generates B-spline curves).
- **Coverage.** `TrimOriginalSurfaces` is kernel-tested only: it needs a fillet B-rep,
  the executive's topology-intersector mates and per-context marks, which the shim does not
  build (its control paths are unit-tested; the binormal comparison and face collection
  are not compared). The curve-classification branch of `ShouldSwapFaces` is not generated
  (the shim builds no `SmCurveClassification`). `RailEdgeuseIntersect` (unit
  `blend.fillet_solver_p2`) and `RecordFilletError` (`NoteFilletError`, unit
  `blend.fillet_executive_p1`) defer; the shim gives each model a real `SmFilletExecutive`
  so the C++ side runs them.
- **File-statics.** `JoinTwoFilletVertsByCurves` and `sm_MoveTSP` are reached through
  wrappers defined in the translation unit of `harness/cpp/shim/blend_fillet_geom_p1.cpp`
  (the only one that compiles `SmFilletGeom.cpp`), included from
  `blend_fillet_geom_p6_static.inc`.
- **Small scales.** At scale 1e-4 the Rust `LiftCurve` in `CalcBlendingRail` returns
  `SM_ERR_NULL_POINTER` where C++ lifts the curve: the surface cache refuses to subdivide past
  `MAX_SUBDIVISION_NODES` (see `surface.surface_p11` above; reproduced on main with a plain
  3x3 quadratic patch scaled by 1e-4, review of PR #247). The generator keeps this op at
  scale 1 and above; extending it below needs that limit raised or the cases deferred.
- **Wiring part 6 into parts 2/4, the corner and the executive exposed three harness or
  ordering gaps, all fixed (PR #247).** (1) `blend.fillet_geom_p4`'s `MakeFace` runner built
  a bare `Brep::new()`; upstream's `new SmBrep()` has a void infinite region, so
  `TrimSurfaceWithModelSpaceCurves` went on in C++ (`SM_ERR`) but stopped at `NER(pRegion)`
  in Rust. (2) `CalcBlendingGeom` read both rails before its loop; upstream reads
  `m_vRails[i]` per iteration, so a missing second rail after a failing first one is
  `SM_ERR`, not `SM_ERR_NULL_POINTER` (`--seeds 2000`, 6 cases). (3) `blend.fillet_corner_p3`'s
  shim stopped `AdjustTangentCorner` at `GetRailIndexOfVertex`; it now mirrors the split loop
  up to `SplitFilletGeomAtSideEdge`. Also `GetRailIndexOfVertexuse(NULL)` is
  `SM_ERR_NULL_POINTER` (`NER(pFVU)`), not a rail-end comparison.
- **Back-edges not wired.** `GetRailIndexOfVertexuse`, `GetRailVertex` and `SetUpRailPoint`
  in `blend/fillet_solver/p1.rs` (`split_fillet_geom_at_side_edge`, `split_fillet_geom`)
  stay stubs: that code holds the solver's opaque `FilletGeomRef`s and a plain pseudo
  `Brep`, not a `FilletBrep` (as for part 3's `TopologySplit`).
