# heal.heal_data_p3: split_face_seam cone cuts keep trims past the seam, off their edges (2026-11-17)

**Unit:** `heal.heal_data_p3` (goldens runner `units::heal_heal_data_p3`), cases
`split_face_seam_cone_*_s90_*` (12 of them) and `split_face_seam_cone_wide_10_350`,
`split_face_seam_cone_wide_200_340`.

**What:** after the face is split at its seam, the cut's two trims (edge uses 1 and 11)
run in `u` from 0 to 1.086..1.096, past the cone's `[0, 1]` NURB domain. Evaluating them
with the kernel's own `Surface::evaluate_point` (201 points per trim, against 4001 points of
the edge curve) puts them off their edges:

| cases                                  | edge use 1 | edge use 11 |
|----------------------------------------|-----------:|------------:|
| `r0.02_0.03_h{1,25}_s90_q0`            | 0.0370     | 0.0555      |
| `r1_0.5_h{1,25}_s90_q0`                | 1.851      | 0.925       |
| `r1_1_h{1,25}_s90_q0`                  | 1.851      | 1.851       |
| `r1_1_h{1,25}_s90_q1`                  | 1.854      | 1.854       |
| `wide_10_350`                          | 1.253      | 1.253       |
| `wide_200_340`                         | 1.645      | 1.645       |

The edge tolerance is 1e-5. The edge curves themselves lie on the surface: the kernel's
`calc_max_edge_face_gap` is about 1e-12. Only the trims are wrong. The same cases on main
(7d4445c0c) also hold edge uses 5 and 8 off their edges, by up to 0.356 (`r1_1_*_s90_q1`).

**Same on main:** main (7d4445c0c, `keep_operand_uv_trims` still present) writes these
off trims on edge uses 1 and 11 (the table is main's). On this branch they sample the same
(checked bit for bit for `r0.02_0.03_h1_s90_q0`). #17896 (retiring the flag)
drops the trims on edge uses 2, 5, 8 and 14 (the four moved owner entries in
`r0.02_0.03_h1_s90_q0`) and keeps those on 1, 4, 11 and 12. The trims made again from
those edges are certified on their edges.

**Suspected cause:** the split keeps the cut's trim in the parameter period past the seam
(`u > 1`) instead of shifting it into the domain. The certified enclosure in
`crates/goldens/src/trims_on_edges.rs` continues the end polynomial piece there, and its
bounds (0.046 and 0.097 for `r0.02`) agree with the kernel's sampling that the trims do not
follow their edges.

**Where it is used:** `operand_trim_rows::trim_blocks::off_on_main` skips exactly these
edge uses when certifying the `heal.heal_data_p3` drop rows, and asserts that each of them
is present.

**PR:** tangency-labs/solidcraft#17896. Not fixed there: the trims on edge uses 1 and 11
are main's, unchanged.

**Issue:** tangency-labs/solidcraft#18197 tracks the fix.
