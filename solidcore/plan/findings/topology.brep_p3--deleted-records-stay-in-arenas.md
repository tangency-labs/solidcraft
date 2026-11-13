# topology.brep_p3: DeleteFace and SqueezeEdge left their deleted records in the arenas (2026-10-30)

**Unit:** topology.brep_p3 (`crates/solidcore/src/topology/brep/p3.rs`: `delete_face_in`,
`destroy_face`, `squeeze_edge_in`). Narrows the general
`plan/findings/topology.brep--orphaned-arena-records-2026-10-26.md` for these two paths.

**Inputs.** The #1528 shell operand (`crates/solidcore/tests/issue_1528_write_operand.rs`):
`create_linear_sweep` of the profile (0,0)-(42.5,0)-(42.5,20)-(0,20) plus the half
circle r 10 centred (0,10), 27.5 along +z, less `create_cylinder((-10,12.5,12.5), 5,
17.5)` rotated 90 degrees about +y at its base (`boolean_difference`).

**Observed (origin/main 076c6033d).** The result's lists hold 8 faces and 15 edges (the
closed form: top, bottom, three planar sides, the half-cylinder side, the hole wall and
its blind end; the plate's 12 edges plus the hole's three). Its arenas held 12 faces and
17 edges: faces 4, 6, 8 and 9 live, on no list, with no face use, surface or shell
(`destroy_face`'s end state); edges 32 and 33 live, on no list, with no edge uses
(`squeeze_edge_in`'s end state). A write and read back (`write_brep_to_file`,
`read_brep_from_file`) gives 8 and 15 in both: the writer walks the lists. So
`faces().len()`, `edges().len()` and every arena walk disagreed with the B-rep and with
C++, and the arenas grew with every Boolean.

**Cause.** Upstream `SmBrep::DeleteFace` (`SmBrep.cpp` 7072-) deletes the face, both
face uses, every loop and both its loop uses, the vertex uses of its loops
(`pOtherVU`, `pVU` when the vertex keeps other uses, `pVU1`/`pVU2` of each non-lamina
edge use pair) and, with region nesting, the merged-away region; `SmBrep::SqueezeEdge`
(27040-) deletes the squeezed edge's uses and their vertex uses, the deleted vertex and
the edge. The port unlinked all of them but removed only the edge uses (and shells)
from the arenas: its header recorded "records stay in their arenas, unreachable" as a
deviation, while `topology.brep_p4`'s deletions already removed their records.

**Fix (PR: orphan-records p3).** Each of those records is removed
(`Brep::remove_*_record`) at upstream's `delete`, after its last read (the face's and
the deleted vertex's after their destruction notifications). Arena ids are never
reused, so other records keep theirs; traversal order, numerics and notifications are
unchanged. `crates/solidcore/tests/issue_1528_orphan_records.rs` checks that the face
and edge arenas hold exactly the B-rep's lists after the #1528 difference and a box
less a through and a blind cylinder (the only list-only face and edge removals were
these two paths); it prints the vertex, region, face-use, loop and use-record counts,
which the follow-up checks once its paths are fixed.

**Still open (follow-up PR).** The other list-only deletions: `topology.brep_p7`
GlueEdges' deleted edge (when it keeps edge uses), `topology.brep_p5`'s vertex removal
and `topology.brep_p2`, plus the construction paths of the 2026-10-26 finding (the test
prints loop-use, edge-use and vertex-use counts without checking them). The destruction
notifications of loops, loop uses, face uses and vertex uses (`~SmLoop`, `~SmLoopuse`,
`~SmFaceuse`, `~SmVertexuse` each `Notify(SM_NO_DESTRUCTION)` upstream) and of
SqueezeEdge's edge uses (`~SmEdgeuse`) are not sent by these paths, before or after this
fix; a separate finding if a runner observes them.
