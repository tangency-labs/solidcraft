# topology.vertexuse (`SmVertexuse`, 2026-10-14; partial, no disagreements)

- **B-rep import.** As `topology.edgeuse_p1`: the shim builds real `SmBrep`s (15 upstream
  `.smb` files, wire edges, faces from B-spline surfaces and `SmPlane`s, the wire network
  of prog_test `my_test_topology`) and exports the graph (now with each vertex use's
  vertex/edge gap state and `m_sUVPoint`); the runner imports it into `topology::Brep`.
- **What is compared.** Owner queries, predicates, `IsConnectedTo` against every target
  kind, the gap getters/calculators/`InitVertexFaceGap` and `Notify` (cache states of
  every vertex use afterwards, so sibling resets are checked), `ComputeUVPoint` /
  `ComputeUVSector` / `IsSectorMissingSeam` / `IsOnPole` on shell vertex uses and wire edge
  vertex uses (statuses and outputs). On faces these defer: `SmFace::GetUVDomain`
  (`topology.face_p2`) is read before anything else, `GetSingularities`
  (`surface.surface_p3`) gates `IsOnPole`, and setting a gap needs `SmVertexEdgeGap::Set`
  / `SmVertexFaceGap::Set` (`heal.gap`). The rest of `ComputeUVPoint` / `ComputeUVSector`
  is ported behind further stubs (`NormalizedEvaluate`, `LocalPointSolve`,
  `GlobalPointSolve`, `IsSingularity`, `IsOnSeam`, `SmLoopuse::GetOrientation`) and is
  not yet compared; re-run the difftest when those land.
- **Vertex use lists.** `SmVertex` keeps no use list in the port yet; the uses of a vertex
  are found by scanning the B-rep's vertex uses (only used to reset caches, so order is
  not observable). `topology.vertex` should add the list.
- **Upstream bug.** `ComputeUVSector`'s opposite-orientation count has an operator
  precedence bug (`plan/upstream-issues.md` WR-83); ported as parsed.
- **Not wired**, still `UNPORTED(topology.vertexuse)`: `surface::trim_srf_cache`
  (`ComputeUVPoint`, `GetVertex`) and `intersect::point_class` (`IsConnectedTo`). Those
  callers hold opaque object handles without a `Brep` (as for `topology.edgeuse_p1`).
