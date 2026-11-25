Updates: plan/findings/blend.fillet_executive_p3--sweep5-sphere-diff-e29-vertex-tolerance.md

# blend.fillet_executive_p3: sweep5 sphere_diff_3cylinders e29 vertex tolerance resolved (2026-10-27)

Root cause: the zone tolerance of `SmBSplineCurve::Trim` when `SmFilletGeom::TrimRailCurves`
trims the rail curve and its two UV trim curves.

`Trim` reads `SmTol::GetZoneTol3d(this)`. For a curve held by an edge, or a UV trim curve
held by an edge use, that is the holding edge's `GetTolerance()` (`SmCurve::GetEdge`
follows the edge use to its edge). The rails of the fillet B-rep have tolerance 0, so
C++ trims with a zero tolerance and never removes a degenerate end span. The port used
the system zone tolerance (1e-5) for all three trims.

Rail 1's curve is a copy of a parent curve (0, 117.909) with a knot of multiplicity 3 at
102.85355699527092 (rail 0's end), trimmed to (82.647290569503795, 102.85355973819911).
The last span is 2.7e-6 long, below the 1e-5 mapped tolerance, so the port removed it
(47 knots, against 50 in C++). Later, during `TrimOriginalSurfaces`' face shrink,
`CreateUVTrimCurve` drops this edge's curve onto the cone face: C++ walks the extra span,
the walk reaches the surface boundary at 102.85355699527092 before the end of the edge
interval, `DropCurve` returns no curve, and `DropAndTrimCurve` succeeds with max distance
4.5503e-5 (edge tolerance 2 x 4.5503e-5 = 9.1005e-5). The port's `DropCurve` succeeded
right away with distance 4.5503e-5 and deviation 9.692e-6, combined to 4.6523e-5 (edge
tolerance 9.3047e-5), which cascaded to the vertex.

Fix: those three trims now pass the holding edge's tolerance (system zone tolerance when the
holder has no edge) to `Curve::trim_with_zone_tol`. The case agrees exactly and its
known-mismatch entry is removed.

Not changed: the same pattern of three trims at the end of `SmFilletEdge::CalcFilletIntFillet`
(`blend/fillet_geom` p3). Passing the holding edge's tolerance there too broke
`blend.fillet_geom_p3` generated case gen0252 (value count C++ 125, Rust 130), so whether
those curves have an owning edge in C++ at that point (`SetUVCurve`, the fillet edge's
tolerance) needs its own check.
