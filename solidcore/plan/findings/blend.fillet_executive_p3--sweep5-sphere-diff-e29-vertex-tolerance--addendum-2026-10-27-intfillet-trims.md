Updates: plan/findings/blend.fillet_executive_p3--sweep5-sphere-diff-e29-vertex-tolerance--addendum-2026-10-27-resolved.md

# blend.fillet_geom_p3: CalcFilletIntFillet's trims use the holding edge's tolerance (2026-10-27)

The three `SmBSplineCurve::Trim` calls at the end of `SmFilletEdge::CalcFilletIntFillet`
now pass the holding edge's tolerance too. The earlier attempt broke `blend.fillet_geom_p3`
gen0252 (value count C++ 125, Rust 130) because of the difftest shim, not the port.

C++ probes on gen0252 (before the call): the edge's tolerance is 0; its 3D curve has no
owner (`GetEdge()` NULL), so `SmTol::GetZoneTol3d` gave the system 1e-5; both UV trim
curves are owned by their edge uses (`SmFilletEdgeuse::SetUVCurve` sets the owner) and gave
0. The Rust probe gave 0 for all three. The shim built the edge's curve with
`SmEdge::SetCurve`, which never sets the owner. Every upstream producer of a fillet edge's
curve calls `SetOwner(edge)` right after `SetCurve` (SmFilletGeom.cpp 2616, 2763, 3098,
6082, 6308; SmFilletCorner.cpp 2970, 2984, 3324, 3336, 3609), so in a real fillet B-rep
the curve is owned by its edge. The p3 shim now does the same. With that change gen0252
agrees and the unit passes 318/325 (7 rejected) as before.

Not checked: the `blend_fillet_geom_p1` and `blend_fillet_geom_p6` shims also build edge
curves with `SetCurve` and no owner; their ports have no zone-tolerance trims that read
the owner today.
