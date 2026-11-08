# blend.fillet_geom_p1: gen1440's rail trim reads the rail edge's tolerance (2026-10-31)

Updates: plan/findings/blend.fillet_geom_p1--fillet-trim-status-483--addendum-2026-10-30-opt-in.md

**Cause of gen1440:** a port divergence, not a C++ defect.

- `SmFilletGeom::Trim` calls `pRailCurve->Trim(rTrimInterval)` on each rail.
- `SmBSplineCurve::Trim` (`SmBSplineCurveNLib.cpp:4469`) takes `sZoneTol3d = SmTol::GetZoneTol3d(this)`. It maps that to parameter space at both ends and removes a trimmed end span shorter than the result with `N_CrvRemoveDegenSegs`.
- For a curve, `SmTol::GetZoneTol3d(const SmObject*)` (`SmTol.cpp:760-790`, old tolerance model) is `pCurve->GetEdge()->GetTolerance()`. Only an edgeless curve falls back to the system zone tolerance.
- A fillet B-rep's rail edges have tolerance 0, so C++ never removes a span there.
- The port's `FilletBrep::trim_geom` called `Curve::trim`, which uses the system zone tolerance.
- gen1440's first rail (edge 2, rational degree 2, interior knot `0.6125828265124486`) was trimmed to `[0.4916704865259837, 0.619432454122106]`. That leaves a last span of 0.0068 in parameter on a curve about 1e-3 in size. Under the system tolerance that span counted as degenerate and was removed, leaving 3 control points and 6 knots. C++ keeps 4 and 7.

**Fix:** `trim_geom` passes `held_curve_zone_tol_3d(rail)` (`SmEdge::GetTolerance`) to `Curve::trim_with_zone_tol`. The `TrimRailCurves` and `TrimFilletEdge` ports (`blend::fillet_geom::p5`, `p3`) already do this.

- The centre line and the fillet surface are unchanged: the centre line has no edge, so C++ uses the system tolerance too.
- No tolerance, guard or shim changes.
- The known mismatch is removed, which deletes `crates/difftest/known_mismatches/blend.fillet_geom_p1.toml`.
- Regression: `trim_geom_maps_the_rail_edges_tolerance` in `blend::fillet_geom::p1`.

**Follow-up audit:** other `Curve::trim` calls on edge-held curves may have the same divergence. Each should be checked against `SmTol::GetZoneTol3d(this)`, which resolves to the holding edge.
