Updates: WR-662

#### WR-662 (update, 2026-10-31). The remaining surface-intersection and proto-topology sites

`Kernel::fixes.ssi_proto_topology_statuses_pass_on` (tangency-labs/smlib bcc6f01) now also covers the plain pass-on sites the first port left as upstream. With the flag off every site is unchanged.

- **`SmProtoTopology.cpp`, now passed on:**
  - `SmVertexDefinition::SetPointParameters`, and `Resolve`'s `FindObjPoint3d` and `SetMinMax`.
  - `SmEdgeDefinition::GetEndPosition` (both overloads) and `Reparameterize`'s `ReverseOrientation`.
  - `SmProtoVertex::Resolve`'s definition `Resolve` and `CanCombineVertexDefinitions`. The definitions deleted so far are unlisted before the error is returned.
  - `SmProtoEdge::AddEdgeDefinition`, both overloads: `EvaluateMid`, `Reparameterize`, the owner's `RemoveEdgeDefinition`, the copy's `Trim` and the inner `AddEdgeDefinition`.
  - `SmProtoEdge::Resolve`: `AddVertexDefinition`, `CanCombineEdgeDefinitions` and both `RemoveEdgeDefinition`s.
  - `CanCombineEdgeDefinitions`' `sm_FindEDOnDegenFace`.
  - `CreateVertexDefinition(rPC1, rPC2)`, `CreateProtoVertex` and `FindOrCreateProtoVertex`: `sm_FindPositionFromPCs`. `FindOrCreateProtoVertex` also passes on `AddVertexDefinition`.
  - `CreateProtoEdge`: the UV `Trim`s and `AddEdgeDefinition`.
  - The manager's `RemoveVertexDefinition`, `RemoveEdgeDefinition` and `RemoveProtoEdge`: the owner's removal and the empty owner's removal. These now take the kernel.
  - `FindOrCreateProtoEdge`: `CheckOverlappingProtoEdges` and `AddEdgeDefinition(rCI1, rCI2)`.
  - `UpdateEdges`: `SetSrcTopo`, the split point's `EvaluatePoint` and `EvaluateMid`.
  - `UpdateFaces`: `SetSrcTopo`, both `EvaluateMid`s and the three `EvaluateBinormal`s.
  - `GlueCoincidentBrepVertices`: `GlueVertices`, `RemoveRelationship`, and `RemoveEdgeDefinition` / `RemoveVertexDefinition`.
  - The overlap-removal block: the `RemoveEdgeDefinition` / `RemoveProtoEdge` / `RemoveVertexDefinition` calls, `GlobalCoincidenceChecker`, `CurveOnClassify`, `SetPointClassParameters` and `FindOrCreateProtoEdge`.
  - The fork-only WR-452 end trim's `SetMin` / `SetMax`.
- **`SmSurfaceIntersector.cpp`, now passed on:**
  - `DoBoundaryPlaneTest`'s `Trim`.
  - `FindBoundaryStartPoints`: the iso-curve intervals' `SetMinMax`, the small-range `Intersect` and the surface-edge `AddStartPoint`.
  - The boundary gap walk (`EvaluatePoint`, `Evaluate`).
  - `ComputeLawPointValues`: both `Evaluate`s, the first `Inversion`, and the second `Inversion` only when it leaves the parameter undefined, as the fork does.
  - `DoPointIntersection`'s `AddStartPoint` / `AddThroughPoint`.
  - `DoIntersection`'s box `Intersect` / `SetMinMax`, and the degenerate curve's `EvaluatePoint`.
- **No Rust counterpart, or nothing to pass on:**
  - `SetFinalVertex` and `Destruct` cannot fail in Rust.
  - `GlueVertexDefinitions` and the `CreateVertexDefinition(SmTopology*, ...)` overload are not ported.
  - `sm_FindEDOnDegenFace`'s `DeleteFace` is `UNPORTED(topology.face_p4)`, and unreachable.
  - The removed `RemoveEdgeDefinition` after `CombineProtoEdge`'s `AddEdgeDefinition` was already ignored. It found nothing to remove, so the result is the same.
  - The boundary start points of face edges (`lCrv >= 4`: `GetOrCreateUVTrimCurve`, `CalculateBoundingBox`, the edge `LocalPointSolve`s, `AddThroughPoint`) are not ported.
  - `ApproximateCurve`: the port copies the boundary curve as a B-spline.
  - `AddPointToCurve` already propagates.
  - `IsSameTangentCluster`'s drop already answers "no" on a failure.
  - Kept `(void)` by the fork: the constructors, `ExtendEndToProtoVertex`'s `SetMin/SetMax`, `Unitize`, the Hermite boxes and evaluations, `sm_IsPointUnique` and `PointClassify` in `sm_ClassifyParamOnFaces`.
- **Tests** (`topology::proto_topology::p2::tests`):
  - Discriminating: `manager_removals_pass_on_unlisted_entries`, `add_edge_definition_passes_on_a_failed_removal_from_its_owner` and `end_position_of_an_unevaluable_curve_is_passed_on`. Each fails with the pass-on disabled.
  - Coverage only: `intersect::surface_intersector::p5::tests::intersections_are_the_same_with_statuses_passed_on`.
