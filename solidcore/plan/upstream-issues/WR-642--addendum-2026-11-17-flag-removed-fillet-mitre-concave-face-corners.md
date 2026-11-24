Updates: WR-642

#### WR-642 (update, 2026-11-17). `fillet_mitre_concave_face_corners` is removed

`SmFilletNx2Corner::MakeCornerTopology` no longer sweeps a 3-sided face round the side edge at a face corner of the other convexity (`bMixedConvexity && !m_bBevel && dAngleDeg > 5.0`). Such a corner is now always mitred, as a corner of uniform convexity is (the fork's 4f06e72, the owner's ruling of 2026-10-30). The corner's angle is still read, as upstream's test read it, so an unset angle is still an input error. The fix is unconditional and the flag is gone; SolidCraft already ran with it on. Removed: the field `UpstreamFixes::fillet_mitre_concave_face_corners`, SolidCraft's preset line, its entries in `fixes.json`, `issue_2260`'s flag list and the goldens name map, the swept branch, and every test setting of it.

- **Results:** 10 `blend.fillet_executive_p3` goldens rows move. They are the `stubs3-union-*` two-concave-round cases at a boss-root corner: inset-side-step edges 13-15, 13-17, 14-15, 14-18, 16-17 and 16-18; overhang-corner 12-13 and 14-15; overhang-front 14-15 and 15-16. Each has one face and two edges fewer.
  - Each body is one closed manifold solid with no Level 2 report.
  - Its enclosed volume holds the mitred closed form `2600 + A L + r^3 (5/3 - pi/2)`, with `A = r^2 (1 - pi/4)`.
  - The enclosure excludes main's swept form `2600 + A L + (pi/2) A d`, which is 0.0206 away.
  - Test: `g5_blend_corner_certificates::mitred_rows_hold_the_mitred_closed_form` in the goldens crate.
- **Tests:** `tests/wr642_mitred_concave_face_corners.rs`, `tests/wr634_mitred_edges.rs`, `tests/wr644_mitre_gap_patch.rs`, `tests/wr650_tabbed_plate_mitred_rounds.rs` and `tests/issue_1981_pocket_chamfer_operand_trims.rs`.
  - Removed with the flag: `wr642`'s checks that the pocket and boss corners are swept without it.
  - `wr634`'s no-mitre check now chamfers a plain box's top rim, whose corners are convex, instead of the pocket with the flag off.
  - `wr457_certificates`' closed form for its four two-concave-round cases (the same corners) is now the mitred one.
