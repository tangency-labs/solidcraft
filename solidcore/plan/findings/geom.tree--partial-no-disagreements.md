# geom.tree (partial; no disagreements)

618 default cases (18 fixture + 600 generated): every non-deferred case agrees exactly
(`max_rel_err = 0`); 7 are deferred (`GetFirstBezierPatch` paths). Notes for later agents:

- **Arenas mirror `SmMemBlockMgr`.** Nodes and object-list items live in `Tree::nodes` /
  `Tree::lists` in allocation order, which is the order `GetAllTreeNodes` and
  `SmTree::GetObjectList` report (and therefore the order a forced rebuild in
  `AddToSpatialTree` re-inserts objects). Fresh nodes are zero-filled like `smos_Calloc`
  memory (all-zero box, `SM_NG_NONE`); a failed child `SetMinMax` leaves that zero box.
- **Objects are a generic handle `O`.** `SmObject*` becomes `Option<O>`; the shim uses fake,
  never dereferenced pointers into a char array.
- **Subdivision data.** `SurfaceAux` holds only the `SmBezierAux2d` fields this unit reads
  (`m_eSplitDir`, `m_bIsMarked`, `m_eNodeClass`, `m_pFace`, `mBA_pSurface`); the surface
  cache units own the rest and should extend it. Upstream casts parent/sibling `m_pData`
  without checking the aux type; Rust returns `InvalidInput` where that cast would be invalid.
- **Back-edges.** `GetOwnerObject` (curve/surface cache owners), `GetFirstBezierPatch`
  (`SmBezierPatch::GetBezierPtr`, `SmBSplineSurface::GetGwNurbPointer`) are `UNPORTED` stubs;
  the shim skips exactly those paths (owner is always NULL there).
- **Upstream quirks kept.** `SmObjectList::FindObject` returns the last item (not NULL) as
  "previous" when the object is not found. A point tree (`SmTree(point, size, ...)`) never
  initialises its object-list memory manager, so `AddToSpatialTree` on it divides by zero
  upstream; the ffi wrapper refuses that op. `1 << lSplitCount` in the neighbour searches is
  a C `int` shift (sign-extended into `ULONG`); Rust mirrors it.
