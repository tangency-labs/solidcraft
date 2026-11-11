# topology.brep: the remaining deleted records leave their arenas (2026-10-30)

**Units:** topology.brep_p1, topology.brep_p2, topology.brep_p6, topology.brep_p7,
topology.brep_p9. Follows `plan/findings/topology.brep_p3--deleted-records-stay-in-arenas.md`
(DeleteFace, SqueezeEdge) and resolves the paths of
`plan/findings/topology.brep--orphaned-arena-records-2026-10-26.md`.

**Inputs.** `create_box` (10 cube), `create_cylinder` (r 2, h 12), the #1528 plate
(`create_linear_sweep` of three lines and a half circle), the rotated #1528 drill, the
plate less the drill, and the cube less a through and a blind cylinder. A diagnostic
walked every arena and listed the live records nothing in the B-rep reaches (from the
region, face, edge and vertex lists and their links), on origin/main with the p3 fix.

**Observed.** Faces, face uses, edges, edge uses, vertices, shells and regions: none
left (unreached / arena):
- vertex uses: box 6/54, cylinder 3/15, plate sweep 22/70, drill 3/15, plate less
  drill 29/89, cube less the through cylinder 10/70, less the blind one 9/69. Two kinds
  (the dump lists the first 12 of each arena): vertex uses still on a shell
  (`owner: Shell(..)`, vertex set), and reset ones (`vertex: None, owner: Unknown`);
- loops 6/12 and loop uses 12/24 on the plate sweep, 8/17 and 16/34 on the plate less
  the drill, reset (`edges: [], loopuse: None`; loop uses with every field empty).

**Cause.** Each is a record upstream `delete`s and the port only unlinked or reset:
- `MakeEdgeVertexTopology` / `MakeEdgeTopology` (`SmBrep.cpp` 13990, 14028, 14147):
  `delete pVU` of a vertex shell's vertex use (port: p2
  `remove_vertexuse_from_vertex`, "the record stays in the arena");
- `sm_RmAndDel_OldLoopVertices` (10611-10661): `delete` of both loop vertex uses (port:
  p1, records reset to `Vertexuse::default()`);
- `MakeEdgeInFaceWithVU`'s loop removal: `delete` of the loop and both loop uses (port:
  p9 `delete_loop_records`, records reset);
- the shell and region clean-ups of `TestClosureSplitRegion` / `MakeFaceTopology`
  (`delete pShell`, `delete pRegion`; port: p6 `delete_shell` / `delete_region`, records
  reset) and the shell merge's `delete pReg` (port: p7 `remove_emptied_region`, record
  kept off the list). These did not show in the inputs above; same class.

**Fix (follow-up PR).** Each of those records is removed (`Brep::remove_*_record`)
where upstream deletes it, after its last read; notifications are unchanged (the
records' destruction notifications the port already sent stay, none is added).
`crates/solidcore/tests/issue_1528_orphan_records.rs` now asserts every arena (faces,
edges, vertices, regions, face uses, loops, loop uses, edge uses, vertex uses) for the
box, the cylinder, the plate sweep, the #1528 difference and both cube differences;
`topology.brep_p2`'s merged-shell unit test also checks the two remaining vertex uses.
