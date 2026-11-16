# primitive.primitive_creation_p2 planar-path union failures: narrowed to an edge definition's proto vertices (2026-10-25)

Follow-up to "planar-path sweeps at 1e4-1e6 scale fail their union after
ProjectAndTrim" (cases `gen0003`, `gen0028`, `gen0039`, `gen0178`, `gen0200`,
`gen0208`, `gen0252`). Traced on `gen0003` (an L-shaped path, profile circle of radius
1e5, tolerance 10), with temporary instrumentation in which the C++ shim wrapped
`SmBrep::ProjectAndTrim` (`-Wl,--wrap`) and both sides dumped the B-rep before and
after each call:
- Both pieces agree with C++ before and after each `ProjectAndTrim`: the same
  status (`SM_ERR` from the reference point's `sm_FindSideOfCurve` on both sides),
  2 -> 3 faces, 3 -> 5 edges, 2 -> 3 vertices, and the same vertex points and
  tolerances, edge tolerances and curve types, and face tolerances and surface types.
  So the divergence is inside the union (`SmMerge::BooleanTrees` ->
  `ManifoldBoolean`), not in the merge.
- In Rust the union fails at `SmEdgeDefinition::Imprint`
  (`ProtoTopologyManager::imprint_edge_definition`, `SM_ERR_NULL_POINTER`; mapped to
  `SM_ERR` by `boolean_trees`). Edge definition 13 has Edge/Edge sources (the two
  pieces' cut edges, from about `(-358578.64, -1e5, 358578.64)` to
  `(-358578.64, 1e5, 358578.64)`). Its start and end are the same proto vertex. That
  proto vertex has a single vertex definition, on existing vertices of both B-reps at
  `(-258578.64, 0, 258578.64)`, and those vertices are not on the source edges, so
  `GetFinalVertex` finds no connected vertex for any of the four ends.
- Suspected cause: the intersector's proto-vertex combining or the edge-definition
  endpoints for overlapping edges at this scale, i.e. the topology intersector's
  edge/edge overlap. Not compared with C++ yet: the next step is to wrap the
  intersector entry (`SmTopologyIntersector::IntersectInsertRelate`) and dump its
  vertex and edge definitions. The cases stay known mismatches.

`DoingBoolean` left set after the failed union is faithful. Upstream
`SmMerge::BooleanManifold` sets `SetDoingBoolean(TRUE)` (`SmMerge.cpp:2551`) and
clears it only on its success exits (`:3114`, `:4148`, `:5095`); its `SER` error
returns leave it set, as the port's `manifold_boolean_translated` does. No change.
