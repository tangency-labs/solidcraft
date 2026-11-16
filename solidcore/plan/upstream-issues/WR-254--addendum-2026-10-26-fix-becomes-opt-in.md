#### Updates: WR-254 (2026-10-26). The fix becomes opt-in; the default is upstream's

Updates `WR-254--manifoldboolean-keeps-a-face-lying-in-the-slicing-sheet-in.md`, not
the `WR-254--smfilletcorner-...--dup2.md` record that shares the ID.

- **Change (following tangency-labs/solidcore#1077, as WR-255 / WR-259):** the `SM_BO_SLICE`
  side rule in `coincident_keep` (`slice_same`, `crates/solidcore/src/boolean/merge/p4.rs`,
  upstream steps 7.3e / 7.B) applies only with the opt-in
  `Kernel::fixes.slice_coincident_faces_sheet_side` (`UpstreamFixes`, off by default). By
  default a slice falls through to the difference rule, as upstream does. Step 7.4d's own
  slice rule (`region_keep`) is upstream and is unchanged.
- **Confirmed on the C++ build** through `boolean.api_brep` (the runner now builds sheets
  with `SmApiCreatePlane` and operands that are the result of an `SmApiBoolean`):
  `wr254_slice_box_by_its_top_face` (a box 10^3 sliced by a 20 x 20 sheet in the plane of
  its top face: C++ keeps one lamina face, F/E/V 1/4/4, not a manifold solid) and
  `wr254_slice_slab_tower_in_slab_face` (the #528 configuration: a slab 40 x 40 x 5 with a
  tower 10 x 10 x 10, sliced at z = 5) pass at the default. Both would fail with the field
  on. The controls `wr254_slice_box_mid` and `wr254_slice_slab_tower_mid_tower` pass
  either way.
- **Correction:** the record's "No difftest case reaches these branches yet" no longer
  holds (see above).
- **Tests:** `issue_528_slice_in_a_face_keeps_a_manifold_part` (field on: 6 faces, a
  manifold solid), `issue_528_slice_in_a_face_keeps_the_coincident_face_by_default`
  (7 faces, not manifold) and `issue_528_box_sliced_by_its_top_face`, in
  `crates/solidcore/tests/issue_528_slice_by_a_sheet_in_a_face.rs`.
- **Callers** that relied on the previous default (e.g. SolidCraft, #528) must set the
  field.
