# curve.curve_p7 (`SmCCIGlobalSolver::LocalSolve`, `SmCurve::GlobalPropertyAnalysis`)

- **Pruned curve trees.** `LocalSolve` casts every leaf's `m_pData` to `SmBezierSpan`
  (the `m_eAuxDataType == SM_AD_BEZIER_CURVE` check is an `SM_ASSERT`). A cache built with
  aux data but without a full tree (`bComputeAuxTreeData && !bMakeFullTree`) turns parents
  into leaves holding `SmBezierAux1d` data, so the cast reads past the object (the pseudo
  box is garbage). The curves' object caches always build full trees, so upstream never
  reaches this; the port returns `InvalidInput` and the ffi rejects such caches.
- **Degenerate curves.** When either (projected) curve is degenerate, upstream re-adds the
  last end point solution built (`sSolution2`) once per accumulated end point solution
  instead of each accumulated one; kept.
- **Ignored statuses.** The span end `EvaluatePoint`s, the closed-curve
  `EvaluateContinuity`, `SmProjectedCurve::SetCanonical`, the in-place
  `SmPseudoBox::ProjectToPlane`s and the end point `LocalPointSolve`s ignore their status;
  the port does too (a failed point solve is no answer).
- **`GlobalPropertyAnalysis` comb.** `EvaluateGeometric`'s status is ignored, so for a curve
  whose second derivative vanishes (degree 1) the comb uses what
  `smgu_FrenetFromDerivatives` wrote before failing (position, tangent, zero curvature);
  the port reproduces those partial outputs (`evaluate_geometric_partial`).
- **`SmBSplineCurve::AssertValid`** ignores `GlobalPropertyAnalysis`'s status; on a failed
  solve upstream keeps the solutions found before the failure, the port none (the solve
  only fails for errors upstream asserts on).
- **Deferred.** Every leaf pair that reaches a Newton intersection
  (`LocalCurveIntersect` -> `SmCCIntersectENFO::Evaluate`) and every property solve that
  reaches `SmCurve::LocalPropertyAnalysis` is deferred on `curve.curve_p5`; the co-ported
  `sm_FindAdditionalCcTangentGuess` is only reached after such an intersection.
