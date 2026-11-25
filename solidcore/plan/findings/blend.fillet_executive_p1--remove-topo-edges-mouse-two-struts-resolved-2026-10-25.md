# blend.fillet_executive_p1: `remove_topo_edges_mouse_two_struts` resolved (2026-10-25)

- **Root cause.** After deleting Edge 1 of `Mouse.smb`, `EdgeId(0)`'s stored zone
  tolerance grows to `2.613347926652509`. When deleting Edge 2, `DeleteTopologicalVertex`
  joins `EdgeId(0)` and `EdgeId(4)` into a closed 250-cpt cubic `BSplineCurve` over
  `[0.0, 32.5553347035394]` and calls `GetOrCreateUVTrimCurve` on `EdgeId(0)`'s edge uses:
  first `Dome` (`FaceId(2)`, `SmBSplineSurface`, not the plane face) attaches a 250-cpt
  closed UV curve, then `SideWall` (`FaceId(1)`) drops `[0.8477229801834114,
  27.525179625915907]` at `ii = 3` and triggers `bEditUVMin` / `bEditUVMax`, calling
  `trim_other_uv_curves` (`topology::edgeuse::p3`) to `Trim` `Dome`'s already-attached UV
  curve twice. Upstream `SmBSplineCurve::Trim` reads `SmTol::GetZoneTol3d(this)` ->
  `SmCurve::GetEdge()` -> `pEdge->GetTolerance()` (`2.613347926652509`), triggering
  `N_CrvRemoveDegenSegs` on the ~80 short Dome UV spans (`~0.35 < 2.6133`) and collapsing
  `Dome`'s UV curve to a 1-span 4-cpt cubic Bezier, whereas owner-less `Curve::trim` read
  the system zone tolerance (`1e-5`) from `k`.
- **Fix.** `topology::edgeuse::p3::trim_other_uv_curves` now trims sibling UV curves with
  an edge-tolerance `Kernel` (`zone_tol3d(k, edge::tolerance(b, e)?)`, matching
  `sm_TrimRail` in `blend::fillet_solver::p1`). `remove_topo_edges_mouse_two_struts` now
  passes bit-identically (`max_rel_err = 0.0`, `155/155` pass, `0` `known_mismatch` in
  `blend.fillet_executive_p1`).
