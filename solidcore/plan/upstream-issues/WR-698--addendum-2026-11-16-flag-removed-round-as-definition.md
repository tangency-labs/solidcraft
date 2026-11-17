Updates: WR-698

#### WR-698 (update, 2026-11-16). `fillet_round_as_definition` is removed; a blend face keeps its definition on every kernel

Constant and variable-radius rounds between NURBS faces, and chamfers by a constant-radius solver's offsets, are built
as their definition on every kernel (a `ProceduralBlend` or an exact class): the faces the blend rolls on, its radius or radius
law, the section's order and thumbweight rule, with the certified B-spline net that the surface cache and tessellation
use. The opt-in described in the original record no longer exists. Removed: the field and its docs, SolidCraft's preset
line, its `kernel_cases/fixes.json` entry, every flag-off branch in the blend corners, the corner topology, the exact
classes, the law corners, the geometry, section, solver and surface code, and the tests' flag-off halves. The file
reader reads `SmProceduralBlend_TYPE` on every kernel.

- **Where the definition is declined:** as before, a blend whose centre line or rails have corners keeps its net. New
  with this change (the standard solver's `definition_surface`): a centre line or a rail of no length, judged on its
  control polygon (`is_degenerate`, a bound, not samples), declines the definition and keeps the net, since its
  stations are one point. A cylinder's rim chamfered by its radius (a cone to its apex) was refused with the
  definition ("the rails have no length", code 3 at the corner edges, or a "rail without curve" with
  `fillet_refuse_face_consuming_blends`); it now builds, as it did without the definition (`fillet_closed_rim_rail`,
  `fillet_wr300_320`). That turns a refusal on SolidCraft's preset into a valid result.
- **Defaults:** SolidCraft's preset had the flag on; its results change only by the case above. `Kernel::default()`
  changes.
- **Goldens:** `blend.fillet_executive_p3` moves 38 rows; every other unit passes unchanged. Each moved row is replayed
  by the unit's own tests (`round_definition_rows` in `end_to_end.rs` and `two_offset_chamfer.rs`): its blend faces are
  definitions whose nets re-certify within their bounds, no logged blend face is a bare net (a face without a
  definition is an exact plane, sphere, cone or torus, or a face of a pinned corner), its counts are the
  golden's, and on a closed result the integrator's converged volume lies in the certified volume enclosure of the new
  body widened by the nets' certified distances. Topology moves: the bottle's rail splits where
  its foot crosses the shoulder face's C1 knot (one vertex and one edge more), and the half rim's chamfer at setbacks
  splits at its crease where its wall foot crosses the wall's G1 boundary (one face more).
- **Tests:** the two `issue_880` half-rim tests expect 4 faces to 8 (the crease split, proved by
  `issue_880_one_half_chamfer_splits_at_its_crease_as_a_definition`).
