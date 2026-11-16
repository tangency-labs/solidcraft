Updates: WR-698

#### WR-698 (update, 2026-11-16). `fillet_round_as_definition` is removed; a blend face always keeps its definition

Constant and variable-radius rounds between NURBS faces, and chamfers by a constant-radius solver's offsets, are always
built as their definition (a `ProceduralBlend` or an exact class): the faces the blend rolls on, its radius or radius
law, the section's order and thumbweight rule, with the certified B-spline net that the surface cache and tessellation
use. The opt-in described in the original record no longer exists. Removed: the field and its docs, SolidCraft's preset
line, its `kernel_cases/fixes.json` entry, every flag-off branch in the blend corners, the corner topology, the exact
classes, the law corners, the geometry, section, solver and surface code, and the tests' flag-off halves. The file
reader reads `SmProceduralBlend_TYPE` on every kernel.

- **Defaults:** SolidCraft's preset had the flag on, so the app's results don't change. `Kernel::default()` changes.
- **Goldens:** `blend.fillet_executive_p3` moves 38 rows; every other unit passes unchanged. Each moved row is replayed
  by the unit's own tests (`round_definition_rows` in `end_to_end.rs` and `two_offset_chamfer.rs`): its blend faces are
  definitions whose nets re-certify within their bounds, its counts are the golden's, and on a closed result the
  volume before and after lies within the golden's requested mass accuracy (`1e-4` relative) of the certified volume
  enclosure of the new body widened by the nets' certified distances. Topology moves: the bottle's rail splits where
  its foot crosses the shoulder face's C1 knot (one vertex and one edge more), and the half rim's chamfer at setbacks
  splits at its crease where its wall foot crosses the wall's G1 boundary (one face more).
- **Tests:** the two `issue_880` half-rim tests expect 4 faces to 8 (the crease split, proved by
  `issue_880_one_half_chamfer_splits_at_its_crease_as_a_definition`).
