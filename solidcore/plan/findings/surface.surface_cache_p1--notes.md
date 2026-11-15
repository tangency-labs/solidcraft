# surface.surface_cache_p1

- **`GetSurfaceContinuity` skips the last internal v knot.** The v loop runs
  `iv + 1 < size - 1` (the u loop `iu + 1 < size`), so the continuity at the last
  internal v knot is never looked at. Kept; the difftest covers it.
- **`sm_CullArray` on an empty array** ends with `SetSize(1)`, which callocs one `0.0`
  element; the port returns `[0.0]` too.
- **`BuildTreeTop` re-initialises `m_sAuxMgr`** (active count 0, blocks kept), so the
  head nodes it makes reuse the aux memory of any earlier `SM_AD_AUX_DATA` /
  `SM_AD_BEZIER_SURFACE_HEAD` nodes; upstream only calls it on first generation
  `SmBezierPatch` nodes, which the difftest (and the ffi) keep to.
- **`SmBezierAux2d::BuildPolarBox`** dereferences `mBA_pSurface` of every node on the
  chain; `BuildTreeTop` heads have a null one until `BuildTree` resets them to the owner
  surface. The port returns `InvalidInput` for a node without surface.
- **`SmTreeNode::GetFirstBezierPatch` from a bare tree.** The decomposition data
  (`SmBezierPatch` in `m_sBezMgr`, `SmBezierAux2d` in `m_sAuxMgr`, the subdivision
  surfaces) lives in the `SurfaceCache` arenas; tree nodes carry the arena slot
  (`SurfaceAux::slot`). `SurfaceCache::first_bezier_patch` answers from the cache, the
  tree-only `Tree::first_bezier_patch` (and `SmSurfaceGridElement::Init`,
  `sm_CsArePerpenDisjoint`, which start from a bare tree node) stay `Unported` until the
  tree construction (`surface.surface_cache_p3`) hands the data to the tree, as
  `CurveCache::build_tree` does with `set_curve_data`.
- **Deferred cases**: every polar box (`SmBSplineSurface::CalculateBoundingBox` samples
  `SmSurface::EvaluateNormal`, `surface.surface_p7`), boundary bounding planes with
  collinear samples (straight boundaries: `EvaluateNormal`), every
  `sm_ComputeBezierApproximation` (`SmSurface::IsSingularity`, `surface.surface_p5`) and
  `GetFace` (`surface.surface_p1`).
