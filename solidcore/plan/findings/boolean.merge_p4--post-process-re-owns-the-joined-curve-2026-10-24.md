# boolean.merge_p4 / boolean.merge_p1: post-process re-owns the joined curve (2026-10-24)

Found when SmVertexDefinition::Imprint started merging in the arena pair
(ProtoBrep::merge_point_class_into_brep): the far-translation fixtures (wire at
(1e8,-1e8,1e8), isolated vertex at its middle, operations 1, 3, 4) split the wire, then
BooleanPostProcess joins the two edges again (DeleteTopologicalVertex). The C++ result
curve is back at 1e8; the Rust one stayed at the origin-shifted position (dbls[6..20]:
cpp 1e8.., rust -0.5..0.5). Cause: boolean_post_process calls
delete_topological_vertex without the caller's notification identities, so the joined
curve of the surviving edge is owned under a kernel-issued key; the inverse
SmTemporaryBrepTransform (topology::brep::p5::transform with the adapter's keys) walks
curve owners by identity and skipped it. Fix in boolean.merge_p1: the surviving edge's
curve gets back the owner identity it had before the call (upstream the owner is the
same SmEdge*). All three now pass. A cleaner fix is for the manifold adapter to pass its
notification environment to boolean_post_process
(delete_topological_vertex_with_notifications); that needs boolean/merge/p4/kernel_adapter.rs.
