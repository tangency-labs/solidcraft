# intersect.surface_drop_curve (`SmSurfaceDropCurve`, PR #107 review, 2026-10-15; partial, no disagreements)

- **Iso lines at C0 knots dispatch to the ported overrides.** `FindBoundaryStartPoints`
  builds the surface's interior C0 iso lines through the virtual
  `CreateIsoParametricCurve`. The first port stubbed it for every kind but the plane;
  it now calls `BSplineSurface::create_iso_parametric_curve` (default options: no domain,
  no UV line) and `SurfOfRevolution::create_iso_parametric_curve` (no domain), as
  `surface/surface/p6.rs` does. Only curve-bounded, step and volume surfaces stay on the
  `surface.surface_p5` stub. A `Boundary` op on a B-spline surface with a C0 u-knot (on
  and off surface) now gets past the iso lines and the curve intersections and is
  deferred later, at `SmSurface::GlobalPointSolve` (`surface.surface_p4`) or
  `intersect.curve_class_p4`.
- **`TraceCurve2` without a knot vector segfaults the C++.** When the plane `DropCurve`
  does not return exactly one curve, `TraceCurve2` falls back to
  `SmSurfaceTracer::TraceCurve`, whose `SmSurfaceDropCurve::ComputeStepSize` reads the
  null `m_pCurveKnotVector`. Trigger: plane over `[0,10]^2`, segment
  `(-5,-5,0)-(-1,-1,0)`, breaks `[0,1]`, no knots, ops `cpv(0, [-5,-5])` then `trace2`
  (difftest exit 139). The FFI now rejects `Trace2` unless a non-empty knot vector is
  given or made by an earlier `Boundary`/`DoTrace` (`invalid argument: trace without a
  knot vector`), like `Step`. Upstream callers always set the knot vector before tracing.
- **Off-surface `FindBoundaryStartPoints` on a plane** returns
  `Unported(curve.crv_on_surf)`: the Rust `CrvOnSurf` holds only a `BSplineSurface`,
  while upstream `SmPlane` is an `SmBSplineSurface` (dispatch gap, as in
  `intersect.curve_class_p1`). Unreachable today, because `surface.surface_cache_p1`
  defers the plane's iso boundary curves first. Revisit when `crv_on_surf` takes a
  general `Surface`.
- **Coverage is thin because of dependencies.** Every B-spline `Cpv` is deferred
  (`IsSingularity`), and `Lps`/`Span`/`Next` past their early exits, `Boundary` and
  `DoTrace` are always deferred (surface point solves). The real comparisons are `Cpv`,
  the `Next` early exits, `Trace2` and `Interior` on planes, plus `Step`, `AddBreak`,
  `TestPoint`, `Graze` and the hooks. `OP_SPAN` only runs with `curr == next`; add a real
  two-point span once `LocalPointSolve` lands.
- **Review fixtures** (`fixtures/intersect.surface_drop_curve/review_adversarial.json`,
  33 cases, 30 compared, all bit-exact): vertical and near-vertical dropped tangents, a
  B-spline curve with a zero start tangent, unsorted, short and out-of-range knot vectors
  with negative and zero steps, antiparallel and near-parallel grazes, `sm_TestPoint` at a
  collapsed B-spline edge (`EvaluateNormal` path), a C0 B-spline surface `Boundary`, the
  `Next` boundary and singular early exits, `Trace2` with breaks below the start and with
  the start outside the domain, and `Interior` on curves crossing or outside the domain,
  at 1e6 and 1e-6 scales.
- **Back-edges wired at merge (2026-10-15).** After `surface.surface_p4`, `surface.surface_p5`
  and `intersect.surface_drop_lib_p1` landed on main, the stubs for `IsSingularity`
  (`EFF_ZERO`, no point-only test), `LocalPointSolve` (`SM_SO_NORMALIZE`),
  `GlobalPointSolve`, `IsOnSeam` (no tolerance), the base `CreateIsoParametricCurve` and
  `DropIsoCurve` now call the ported code; `ComputePointValues`, `LocalPointSolve`,
  `ComputeNextPoint` and `ComputeSpanDeviation` take the kernel. Difftest: 473 passed,
  0 failed, 74 deferred (was 341/206), now deferred at `surface.surface_point_solve_p3`
  (56), `surface.surface_cache_p1` (11), `intersect.curve_class_p4` (5) and
  `intersect.surface_drop_lib_p3` (2).
- **The start list after `DoTrace` shares nodes with the curve list (WR-182, PR #136).**
  `TraceCurve2` appends the start node it traces to the intrusive `m_vCurvePoints`
  without taking it off `m_vStartPoints`, so the C++ start list then walks the curve
  ring (gen0354: after `do_trace`, two `next` ops and a `trace2`, it held the three new
  curve points). The port's lists are id `Vec`s and do not model it; both sides report
  such a start list as `-3` (a start point that is also on the curve list) instead of
  reading it. A `TraceCurve` fallback inside `DoTrace` walks the start list too (end
  test, `AddStartPoint`): if those cases ever disagree, this is the first suspect.
