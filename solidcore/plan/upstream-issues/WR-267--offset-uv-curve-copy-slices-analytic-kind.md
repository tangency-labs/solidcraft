#### WR-267. Offset UV-curve fallback copies slice an `SmLine` to a plain B-spline
- **Severity:** Wrong result
- **Area:** Topology
- **Location:** `source/SMLib/src/SmOffsetGeometryCreation.cpp:1655`, `SmOffsetGeometryCreation::EdgeuseOffset` / its `TrimExtendedEdge` setup, and `:1181`, `ExtendTrimEdges`.
- **What happens:** The original `pUVCurve->Copy(crContext, (SmCurve*&)pUVCurve)` (a virtual copy that keeps the curve's kind) was commented out as a "temp workaround to appease Linux gcc" and replaced by `pUVCurve->Copy(crContext, pUVCurve)`. On an `SmBSplineCurve*`, that call binds the non-virtual `SmBSplineCurve::Copy(const SmContext&, SmBSplineCurve*&)` overload (`new SmBSplineCurve(*this)`), so an `SmLine` UV trim curve loses its analytic kind and the offset edge uses get a plain B-spline. The geometry is the same; the kind (and anything that dispatches on it) is not.
- **Trigger:** `EdgeuseOffset` when `DropAndTrimCurve` of the lifted curve onto the other extended surface returns no curve and the edge use's UV trim curve is an `SmLine` (cases `gen0248`, `gen0346`, `gen0350`, `gen0508`).
- **How found:** generated difftest case; the port's virtual copy kept the `SmLine` and the recorded edge-use states differed in curve type.
- **Confirmed:** yes, by the C++ build (the reference records type 14033 `SmBSplineCurve` where the input was 14010 `SmLine`). The port reproduces the upstream behaviour.
