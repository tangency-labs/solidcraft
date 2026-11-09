# boolean.topology_intersector_p1: GlueVertices of vertices of two B-reps is wired in the runner (2026-10-25)

The runner adapter `Two` (two B-rep arenas) returned `Unported`
(`SmBrep::GlueVertices (vertices of two B-reps)`, `topology.brep_p5`) for
`rectangle_face_vertex_face_intersect`, because `topology::brep::p5::glue_vertices` edits
one arena. No port of the edit is needed: upstream never edits in that case.
`SmBrep::GlueVertices` (`SmBrep.cpp:9574-9797`) notifies, reads the faces of the vertex to
delete and the shells of both vertices, and looks for a shell pair in one region. A region
belongs to one B-rep, so a pair that shares none ends the call at `NER(pSurvVShell)` with
`SM_ERR_NULL_POINTER`, before `SetTolerance`, the vertex use moves or any shell merge. The
callers ignore the status (`sm_TestAndSplitTwoEdges`, `FaceFaceIntersect`).

`Two::glue_vertices_of_two_breps` returns that error after the same lookups. The one side
effect that outlives the call is `Notify(SM_NO_PRE_EDIT, this)`, which deletes the cache of
the B-rep the call is on (`SmBrep::Notify` -> `DeleteObjectsCache`); `Two::glue_vertices`
deletes it first (unit test `glue_of_two_breps_drops_this_cache_and_returns_null_pointer`,
which failed before). The other notifications are not delivered across the two arenas: the
shim registers no context callbacks, and each face of the vertex to delete was notified
`PRE_EDIT` when the vertex was made. A shell pair with no region on both sides (`NULL ==
NULL` upstream, which would go on to merge shells across B-reps) is `InvalidInput`; no
harness B-rep has one.

Result: on the base before #890, `boolean.topology_intersector_p1` went from 827 passed /
3 deferred to 828 / 2 (`with_values` 824 to 825), the 2 deferred being
`SmEdge::GetTolerance` (`topology.edge`). On the tree merged with main after #890 it is
830 of 830, 0 deferred, `with_values` 827, perturb SENSITIVE 830/830; `--seeds 6000`
6030 of 6030. The same-B-rep branch of `Two::glue_vertices`
does not delete the B-rep cache either (p5's `notify_pre_edit` holds none); not changed
here because no case needs it.

Cost if the ruling is wrong: a cross-B-rep glue that upstream completes (impossible by the
region argument above) would be reported as an error by the port. With #890 on main this
was the unit's last deferral, so the unit is promoted to `green`.
