Updates: WR-723

#### WR-723 (update, 2026-11-14). `planar_section_face_tolerance` is removed; a face's section is traced to the face's tolerance

`face_planar_section_curves` (`topology/brep/planar_section.rs`), the per-face step of `create_planar_section_curves`,
always passes the face's tolerance on as the approximation tolerance when none is given. The opt-in described in the
original record no longer exists. Removed: the field, SolidCraft's preset line, its entry in the
`issue_2656_blend_regressions.rs` flag list, the flag-off branch, and the test setting in
`wr698_gn_round_is_its_definition.rs`.

- **Defaults:** SolidCraft's preset had the flag on, so the app's results don't change. `Kernel::default()` changes.
- **Goldens:** 15 `topology.brep_planar_section` rows move: cone_slant, nurb_fillet_section, the three fillet_box_y98
  cases and faces_fillet_box_y98, and gen0015/0025/0038/0056/0070/0071/0073/0074/0089/0093. In every row the section
  curves keep their count, type and topology; only the traced points move. MEASURED (401 points per curve, each
  projected on its face's surface): the traced curves that moved were up to 1.6e-2 off their face before (1631 times
  its 1e-5 tolerance; cone_slant 2.8e-3, gen0070 1.9e-3) and are within 9.6e-6 after. No other row of any unit moves.
- **Tests:** `a_plane_section_of_a_round_is_within_its_tolerance` runs on the default kernel.
