# topology.brep_p4: back-edges on opaque object handles

`topology.brep_p4` (`SmBrep::DeleteEdge`, `DeleteVertex`, `ShrinkGeometry`, `StitchFaces`, `~SmBrep`;
PR #173) wires 18 of the 21 `UNPORTED(topology.brep_p4)` call sites on main. Callers with a
notification environment pass it (`topology/brep/p7.rs` `GlueEdgesGeneral`, `topology/brep/p5.rs`
`*_with_notifications`, `boolean/merge/p5.rs` through its `VertexCopy` records); callers that hold no
notification state run on call-local records (`topology::brep::p4::with_local_env`, as
`heal::stitch::p1::make_vertex_split_edge` and `loopuse::face_notify_edit` do). Three stay stubs,
each `justified:` with this section, for a reason specific to the site:

- **`DeleteEdge` / `DeleteVertex`** in `boolean/topology_intersector/p2.rs` (`brep_delete_edge`,
  `brep_delete_vertex`): the intersector is generic over its object handles `O`; the B-rep, the edge
  (vertex) and the surface to keep are `O` values with no `&mut Brep` or typed id to call the port
  with. Wiring needs the typed adapter that resolves `O` to a B-rep and its ids, which that unit does
  not have yet.
- **`DeleteEdge`** in `topology/proto_topology/p3.rs` (`brep_delete_edge(which, edge)`): the caller
  names the B-rep by an index (`which`) and the edge by a generic handle `O`; it holds no `Brep`.
