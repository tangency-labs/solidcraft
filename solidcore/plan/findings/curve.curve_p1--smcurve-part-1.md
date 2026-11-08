# curve.curve_p1 (`SmCurve`, part 1)

Difftest: 3083 cases (83 fixture: smCurveTest.cpp `my_test_point_extrema` / `my_test_drop_point`
on the unit line segment and the radius-1 circle, the circle centre (every point a normal
foot), SM_API_test rectangle and hexagon edges through `Tessellate`, `TessellateByBisection`,
`GlobalPointSolve` and `ApproximateCurve`, degenerate / closed / short-span edge cases + 3000
generated), all non-deferred cases bit-identical (max_rel_err 0); a 12,083-case run was also
clean; `--perturb` SENSITIVE. 590 deferred on `curve.curve_p5`: `LocalPropertyAnalysis` (the
ray, directed and projected `GlobalPointSolve` operations, 310), `SmFindCCNormalENFO::Evaluate`
(`ClassifyIntersectingRangeSolution` past its point-solution exit, 163),
`SmCCIntersectENFO::Evaluate` (`sm_RefineCCSolutions` on range solutions, 73) and `IsArc`
(minimise / intersect on curved B-splines reach the cache's analytic conversion, 44).

Wiring its stubs made `nurbs.b_spline_curve_p1` green (4 -> 0: `TessellateByBisection`) and
removed every `curve.curve_p1` deferral of `curve.hyperbola` (70 -> 0, all pass),
`curve.curve_p4` (53 -> 0: 28 pass, 25 now wait on `curve.ellipse_p1`) and
`curve.ellipse_p2` (233 -> 0: 68 pass, 164 now wait on `curve.ellipse_p1`, 1 on
`curve.curve_p5`). Notes for later units:

- **Solution objects must report their curve's interval.** `SmGlobalSolver::IdenticalSolutions`
  treats parameters within `1e-4` of the object's parameter interval as equal
  (`SmSolution::GetParameterInterval` reads the object, a curve). With opaque handles the Rust
  port had no interval, kept near-duplicates apart and disagreed. `SolutionObject` gained a
  default method `natural_interval()` (default `None`, which keeps the old
  `UNPORTED(curve.curve_p2)` path); `GetParameterInterval` of a curve object uses it. Handles
  passed to `Curve::global_point_solve` (and any solver that sorts curve solutions) should
  answer it. `GetObjectTolerance` (Rayfire) still needs `SmTol::GetZoneTol3d(object)`
  (`topology.brep_p1`); ray solves defer on `curve.curve_p5` before reaching it today.
- **`IdenticalSolutions` prefers the smaller value even when maximising.** A
  `SM_SO_MAXIMIZE` solve can replace an interior maximum by a span-boundary point `5e-5`
  away with a smaller distance (seen on a closed Hermite curve: `0.42572...` replaced by
  `0.42578125`). Kept.
- **`SmBSplineCurve::CreateSimpleOffset`** calls `sOff.ApproximateCurve(ctx, breaks, tol, gap,
  approx, bOptMatchParameterization, FALSE)`: the caller's match flag lands in the
  `bOptCreateAnalytics` slot and matching is always off. Kept. `ApproximateCurve` of a
  degenerate curve does not write `rdAchievedTol`, so `CreateSimpleOffset` would copy an
  uninitialised `dMaxGap3d`; the port returns `InvalidInput` there.
- **Co-ports from `curve.curve_p5`** (in `curve/curve/p5.rs`): the `Curve` dispatch of `IsLine`
  (with the base body over `Curve`) and the new-cache branch of `CacheMakeOrValidate`
  (`make_curve_cache`). Its porter should route `a_object::get_or_create_object_cache`'s
  `CacheMaker::Curve` through `make_curve_cache`, not port it again.
- `GlobalPointSolve` on an `SmEllipse` needs the ellipse as a `CurveKind` (its `DropPointFast`
  override and cache); the ellipse stub now reads `UNPORTED(curve.ellipse_p1)`. The line's
  `GlobalPointSolve(SM_SO_INTERSECT)` in `SmEllipse::IntersectWithLine` is the line's
  `DropPointFast` and is wired.
- `SmCPGlobalSolver::LocalSolve` reuses one `SmSolution` across guesses; `LocalPointSolve`
  writes only type, value and first parameter on success, so stale fields of earlier guesses
  feed the duplicate test. Ported as written. `sm_GetProjDist` returns a failing status as the
  distance (see `curve.curve_p4`), kept.
- Harness: `sm_TestSpanAccuracy` and `sm_RefineCCSolutions` are file-static, so
  `harness/cpp/shim/curve_curve_p1.cpp` holds verbatim copies (line ranges noted);
  `SmCPGlobalSolver::LocalSolve` is exercised through the public `GlobalPointSolve`.
