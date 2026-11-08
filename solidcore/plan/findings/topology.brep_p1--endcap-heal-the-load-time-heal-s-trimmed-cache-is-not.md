# topology.brep_p1 Endcap_heal: the load-time heal's trimmed cache is not imported (2026-10-25)

Root cause of the Endcap_heal known mismatch (values.out length C++ 245, port 256). It is a harness limitation, not a port bug.
- **Where it splits:** HealBrep of Endcap.smb through each healer op (temporary fixture cases with healer_op
  1..26): ops 1-20 agree exactly and ops 21-26 disagree, so the split is at `Cache_FaceProps_Stage3`. The port's
  output has a UV trim curve on edge use 0 (a line on [6.5, 14.5]) where C++ has none, and an edge and a vertex
  tolerance of 0.007557289128801149 where C++ keeps 0.0040511335586128185. The topology is the same.
- **Why:** in the port, `SetProps_Stage3` -> `ClassifyLoops` -> loop bisector -> `EdgeCurveOnClassify` ->
  `SectorClassifyCurveOn` -> `EvaluateBinormal` -> `smsurf_GetSurfaceCache` finds the face surface's cache slot
  empty. It builds a trimmed cache with the face (`BuildTree` -> `CreateUVTrimCurves`), which makes the trim curve and
  raises the tolerances. gdb on the C++ reference: in the same op-21 HealBrep, `smsurf_GetSurfaceCache` constructs no
  `SmTrimSrfCache` and `CreateUVTrimCurve(s)` / `SmEdge::SetTolerance` are never called. The cache it reuses was made
  earlier, by the heal inside `SmBrep::ReadFromFile` -> `MakeTopologyFromData` -> `HealBrep`. There,
  `Cache_VertexProps` -> `ComputeUVPoint` falls back to `GlobalPointSolve`, because the local solve's distance
  7.692503008381534e-4 exceeds the vertex tolerance of that time; `Fix_TolSizes` only raises it to 0.0040511335586128185 later.
  The local solve values are identical on both sides (uv 0.016865304464 / 0.742855272858).
- **Harness gap:** the runner's Rust B-rep is imported from the C++ export of the already-read (and healed)
  B-rep (`topology_brep_p1::import_file`). The export carries no surface cache, so the cache the load-time heal
  left on the face's surface is missing on the Rust side. A fix would export that cache state (`GetCacheObj()`,
  `m_bHaveTSurfaceCache`, `m_bFaceContainmentDone`) and rebuild an equivalent kept cache on import. It stays a
  known mismatch until then.
