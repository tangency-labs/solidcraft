Updates: WR-361

#### WR-361 (update, 2026-11-14). `keep_operand_uv_trims` is removed

A face a Boolean takes over from its second operand keeps that operand's exact UV trims wherever the new edge runs on `MergeEdge`'s copy of the operand edge's curve over the same interval (`copy_uv_trim_curves_of_other_face`, `boolean/merge/p2.rs`). That is the only path now, and the flag is gone. SolidCraft already ran with it on. Removed: the field `UpstreamFixes::keep_operand_uv_trims`, SolidCraft's preset line, its entry in `fixes.json`, its name in the SolidCraft profile list of `issue_2260_gn_pipe_refined_range_end.rs` and every test setting of it.

- **Which path went:** the upstream one, where `MakeFace` builds the face without the operand's trims and each edge use gets a trim later by dropping the 3D curve onto the surface (`Surface::drop_curve`, a fitted cubic within the drop tolerance).
- **Results:** 158 golden rows move (`boolean.api_brep` 7, `boolean.merge_p4` 15, `blend.feature_executive` 11, `blend.fillet_executive_p3` 49, `topology.brep_local_operation` 67, `topology.brep_p7` 9). Each was attributed to this flag alone by a scratch build of the base with only this flag's read site turned on; the PR lists each row and why it moves. No status changes; topology changes only where a later fillet builds on the Boolean's body.
- **Tests:** `issue_488_operand_trims.rs` keeps its exact-trim check and loses its flag-off half; `issue_1981_pocket_chamfer_operand_trims.rs` loses `pocket_chamfer_without_operand_trims`, whose premise (a Boolean without the copied trims) no longer exists.
