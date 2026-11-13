# blend.fillet_geom p2-p5: corner accessors wired through CornerLookup (2026-10-24, PR #747)

The `UNPORTED(blend.fillet_corner_p1)` stubs of the fillet vertex and edge calculators
(`GetFilletedVertex`, `GetSetBackDist`, `GetCornerType`, `IsBevelCorner`,
`IsBlendingCorner`, `IsCalcAsConstRad`, `IsExtendedSurfacePatch`, `GetFilletEdges`,
`GetThisApproxTol3d`, `GetExtendedSurface`, `TestRailRailInterpolationCurve`) now read the
`FilletCorner` record through `blend::fillet_geom::p1::CornerLookup`: the executive's corner
table (indexed by `FilletCornerId.0`), or the running corner for
`SmFilletCorner::CalcCornerVertGeom` / `CalcCornerEdgeGeom` (`CornerLookup::of_corner`,
which also carries the executive's extended surfaces). A caller with no corner store
(`CornerLookup::default()`) gets `Unported` tagged `blend.fillet_executive_p1`, which owns
`m_vFilletCorners`: the fillet solvers' `TopologySplit` (`AddFilletVertex`,
`GetCornerType`, `MakeRailEdgeuse`'s corner of a non-closed rail) and the rollover
recovery's `CalcRailIntEdgeuse`. Those paths already stopped at
`SmFilletExecutive::GetFilletCornerOfVertex` (same unit) or were deferred before; the tag
moves from `blend.fillet_corner_p1` (fully ported) to the unit that owns the store.
`RecordFilletError` (`blend.fillet_corner_p2`) still needs the executive and its object
identities and stays stubbed (entry "RecordFilletError call sites").

Corner types: the shims' stand-in corners (`blend_fillet_geom_p3.cpp`'s `ShimCorner`, the
new `P2Corner` of `blend_fillet_geom_p2_static.inc`) are `SM_FCR_UNKNOWN`, which no
`FilletCornerKind` has; the runners use `OneByOne` records. On the compared paths the type
is only tested against `SM_FCR_N_x_N_CONCAVE`, which neither is. No disagreement.
