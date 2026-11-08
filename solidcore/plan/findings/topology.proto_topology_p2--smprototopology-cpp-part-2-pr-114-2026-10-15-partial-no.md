# topology.proto_topology_p2 (`SmProtoTopology.cpp` part 2, PR #114, 2026-10-15; partial, no disagreements)

- **B-rep access without UNPORTED stubs into partial units.** The members these functions
  read or write through `SmEdge*` / `SmFace*` / `SmVertex*` (`GetInterval`, `GetCurve`,
  `GetTolerance`, `GetUVDomain`, `SetUVDomain`, `GetSurface`, `GetEdgeuseOfFace`,
  `IsConnectedToFace`) and the allocation of curve copies go through the new trait
  `topology::proto_topology::ProtoBrep` (extends `ProtoGeometry`), implemented by the owner
  of the handles. A `topology::Brep`-backed implementation is the natural next step when
  the topology intersector is ported. The difftest pool implements it with the members the
  shim gives its stand-ins (new pool codes: an edge with interval and zone tolerance, a
  vertex with zone tolerance). `SmTol::GetZoneTol3d` of a vertex/edge/face uses the trait;
  of anything else `intersect::point_class::object_zone_tol_3d`.
- **Harness.** The proto-script engine of `topology.proto_topology_p1` is shared: ops 91 to
  108 run in its shim (`run_p2_op`) and are interpreted by
  `crates/difftest/src/units/topology_proto_topology_p2.rs::run_op` (the p1 runner exposes
  its engine `pub(super)`; its own generator and report are unchanged). The shim refuses
  exactly the calls that would crash or read uninitialised values, worked out by replaying
  the call's control flow with const queries (`sm_FindPositionFromPCs` on copies, the
  pairwise resolve loops with `CanCombine*`, the `GetFinalVertex` calls, the evaluations);
  the port reports those as `InvalidInput`, which the interpreter turns into the same
  refusal, so a refusal on one side only is a mismatch. Two conservative guards are
  mirrored explicitly: duplicated entries in a list a resolve deletes from, and zero-length
  intervals where a reparameterisation can follow (HANG-12). Objects a call makes are
  numbered after the op in the port's order (`register_new`; only new definitions carry
  new curves, so a curve freed by `Destruct` is never re-registered).
- **Not comparable on bare stand-ins (deferred or refused):** `MergeIntoObject` on
  regions/faces/edges (no B-rep; `topology.brep_p9`), `MakeEdgeInFace`
  (`topology.brep_p5`), `UpdateFaces` (`CalculateUVDomainFromUVTrimCurves`,
  `topology.face_p1`; the shim never runs it), point classifications on bare topology
  (`FindObjPoint3d`/`GetTolerance` of edges, vertices, faces in `intersect::point_class`,
  dispatch gaps there), `SmProtoVertex::ContainsPoint` (`query.face_p2`).
- **Deviations:** upstream leaks the proto edge / curve copies on the error returns of
  `CreateProtoEdge` and `AddEdgeDefinition(crCI1, crCI2)` (and returns the leaked proto
  edge in `rpNewPE`); the port makes nothing on those paths, and the shim reports `-1`
  for the proto edge on an error return. `FindMatchingEdgeDefinition`'s `rbRev` is always
  `FALSE` and dropped. `UpdateEdges`' `cbiDoSplit` branch (`static constexpr FALSE`) is not
  ported.
- **Upstream defects:** UB-115 (`CreateVertexDefinition` / `CreateProtoVertex` read an
  unset position and gap), UB-116 (`AddEdgeDefinition` reads uninitialised tangents),
  UB-117 (`UpdateEdges` dereferences its optional calling definition), UB-118 (null and
  deleted definitions in the two `Resolve`s), WR-95 (`CombineProtoEdge` moves every other
  definition; confirmed by `edge_pe_combine_every_other_*`).
