Updates: WR-254 (`WR-254--manifoldboolean-keeps-a-face-lying-in-the-slicing-sheet-in.md`, not the `--dup2` record that shares the ID)

#### WR-254 (update, 2026-11-14). `slice_coincident_faces_sheet_side` is removed

`SM_BO_SLICE` keeps a coincident pair whose primary face bounds material on one side only when that material lies on the side the sheet's `SM_OT_SAME` faceuse bounds (`slice_same` in `coincident_keep`, `boolean/merge/p4.rs`, steps 7.3e and 7.B), instead of the difference rule, which keeps a face lying in the slicing sheet in both parts. That is the only path now, and the flag is gone. SolidCraft already ran with it on. Removed: the field `UpstreamFixes::slice_coincident_faces_sheet_side`, SolidCraft's preset line, its entry in `fixes.json`, its name in the SolidCraft profile list of `issue_2260_gn_pipe_refined_range_end.rs` and every test setting of it.

- **Which path went:** the upstream difference rule for every coincident pair of a slice.
- **Results:** 2 golden rows of `boolean.api_brep` move, each attributed to this flag alone: `wr254_slice_box_by_its_top_face` (the lamina face in the sheet is no longer kept: F/E/V 1/4/4 becomes an empty part) and `wr254_slice_slab_tower_in_slab_face` (the slab's top face lying in the sheet is no longer kept: the part is the 6-face manifold solid, not 7 faces).
- **Tests:** `issue_528_slice_by_a_sheet_in_a_face.rs` loses its flag-off halves.
