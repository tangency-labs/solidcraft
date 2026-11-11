# geom.point_set (green; no disagreements)

2029 difftest cases (29 fixtures + 2000 generated) are bit-identical. Upstream
behaviour worth knowing:

- **`SetUpTightPseudoBox` never projects its points.** The "extra planar work" means to
  project the boundary and neighbour points onto the box mid-plane before measuring
  angles, but `SmPoint3d::ProjectPointToPlane` is `const` and returns the projection;
  the results are discarded (`sBegPt0.ProjectPointToPlane(..) ;`). The angles are
  measured between raw points. A port that projects differs in the last bits of the
  rotation angle, which shows up as a ~1e-15 vs ~1e-16 basis component on slightly
  non-planar sets. Ported as is.
- **`SetUpCentroid` block sums.** The first block holds `size % 50` points and the loop
  runs `size / 50 + (extras ? 1 : 0)` times. For a size that is a positive multiple of 50,
  the first block is empty and the last 50 points are never added (50 points give a
  centroid of `(0,0,0)`). `SetUpCentroid` also never sets `m_bCentroidSet`, so
  `GetCentroid` recomputes every time. After `GetNormal` (or any box query), however,
  the centroid is the best-fit plane's centroid (`smgu_BestFitPlaneToPoints`), which
  is computed correctly. So `GetCentroid` depends on the call order. Kept (fixtures
  `centroid_blocks_*`).
- **Reads outside the array (shim rejects).** `GetArea` on an empty sequence calls
  `smgu_PolygonArea`, which reads `m_pData[-1]`. `GetTightPBox` on an empty set built
  with a normal (classified planar) reads `m_pData[0]`, which segfaulted in difftest.
  The Rust port returns area `-1` and skips the extra work, using `(0,0,0)` for the
  missing points.
- `SetUpAxes` returns the point-set type as its `SmStatus`; every caller ignores it.
  `SmPointSet3d::SetNormal` assigns to its own argument (`sNormal = m_sNormal`) and so
  only clears the cache. It is not reached in scope and is not ported.
