# topology.proto_topology_p3 (PR #121): wiring notes

- `SmFace::CurveOnClassify(TRUE, cc)` (called from `ResolveOverlappingProtoEdges`) is wired
  through `ProtoBrep::face_curve_on_classify(k, face, do_point_classify, cc)`; the B-rep owner
  implements it with `query::face::p2::curve_on_classify` (upstream's other arguments are
  `FALSE`). The difftest stand-in has no faces and returns `Unported(query.face_p2)`.
- The callees of `topology.proto_topology_p2` (merged, PR #114) are wired. The p3 functions read
  the B-reps through p2's `ProtoBrep` trait, which gained `vertex_point`, `edge_faces`,
  `vertex_common_edges`, `edge_other_vertex` and `face_curve_on_classify` for them; there is
  no second trait.
- `ResolveOverlappingProtoEdges` crashes the C++ (exit 139) when an overlap relation is one-way
  (an edge outside its own group is read after it was deleted) or an overlap list repeats an
  edge (the group `[1,1]` processes a deleted edge again). `AddOverlappingPE` and
  `AppendOverlappingPEs` do not de-duplicate. The shim and the Rust runner refuse RESOLVE unless
  every overlap is symmetric, no list repeats an edge and each edge is in its own group, and
  when a proto edge is listed twice in the manager (`AddProtoEdge` does not de-duplicate,
  `RemoveProtoEdge` removes the first copy only; UB-132, found by the maintainer); the crashing
  scripts are the cases `edge_proto_p3_rejected_*`. The guard over-rejects three shapes that do
  not crash (a duplicate in a non-first edge's list, a one-way 3-cycle, a hub 0<->1 plus 0->2);
  narrowing it is left for later. The glue guard now refuses a pair of distinct final vertices
  whose distance is within their combined zone tolerance (what the port compares), not a fixed
  1e-3.
- Coverage: vertices with a point and a zone tolerance (pool kind 10, `VERTEX_AT`) let
  `GlueCoincidentBrepVertices` compare the paths where the final vertices are one vertex or far
  apart (no glue). Near distinct vertices would glue (needs a B-rep) and are refused. Resolve is
  still compared only for definition-less groups: a group with definitions stops at the
  `SmCurveClassification` constructor stub (`UNPORTED(intersect.curve_class_p4)`) before it
  reaches `CurveOnClassify`, which the shim could not run on bare faces anyway.
