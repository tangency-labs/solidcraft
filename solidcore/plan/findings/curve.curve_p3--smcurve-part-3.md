# curve.curve_p3 (`SmCurve`, part 3)

After wiring `mass.integrator` (PR #119), all 3076 cases pass bit-identically with
zero deferred: the 85 remaining curved-length / arc-length inversion cases now
run. The unit is green.


- **`SmCurve::FindParameterAtArcLength`** stepping backwards (`dArcLength < 0`) from a
  start parameter within `dParamTol` *below* the first knot leaves `ik = -1` and reads
  `sKnots[(ULONG)-1]` (out of bounds). The port returns `InvalidInput`; the ffi only
  sends backwards steps from the first knot on. `lOffEndCount` is a `ULONG`: the first
  Newton step that leaves the curve below its start wraps it to `ULONG_MAX`, so the
  iteration stops at once with `*pbResultPastEnd = TRUE` (above the end it needs three
  steps); the port keeps the unsigned wrap. A failed `SetMinMax` (start parameter
  slightly past the last knot) leaves the span interval in its `Init()` state, as in
  the port. On curves other than lines the base `SmCurve::Length` (unit
  `curve.curve_p5`) defers every case.
- **`SmCurvePropertyGS::LocalSolve`** returns `TRUE` (status `1`) from an `SmStatus`
  function when a projected pseudo box corner of the angle properties lies on the
  rotation origin; the port returns `SolidError::Upstream { code: 1 }`. Every guess
  goes through `SmCurve::LocalPropertyAnalysis` (unit `curve.curve_p5`), so only the
  early exits (angle box rejection, disjoint / zero-length intervals, a plane
  intersection span lying in the plane) are difftested now.
- **`SmCurve::CrvsFixGaps`** casts every curve to `SmBSplineCurve` and calls the
  non-virtual `SetControlPoint`: for another kind that writes through a bad cast. The
  port moves end points of `CurveKind::BSpline` curves only (`InvalidInput` otherwise);
  the ffi sends B-splines only. Its `FixChainCurvesList` call deletes duplicates (see
  `curve.curve_p4` above): such cases are not generated.
- **`SmCurve::LocalCurveSolve` / `LocalCurveIntersect`** reach `SmFindCCNormalENFO::
  Evaluate` / `SmCCIntersectENFO::Evaluate` (unit `curve.curve_p5`) on every path, so
  all their cases are deferred; the C++ side runs them (no crash on the generated
  inputs). `SmBestLineFitENFO` (marked "UNDER CONSTRUCTION" upstream, no caller) is
  ported and difftested on its own.
