Updates: WR-495

#### WR-495 (update, 2026-11-23). `offset_analytic_mate_edges` is removed; the exact analytic mate is always used

`OffsetGeometryCreation::mate_edges` (`crates/solidcore/src/offset/offset_geometry_creation.rs`) now always replaces a marched piece that lies on an exact line or conic of its two analytic surfaces with that curve, and the flag is gone. Removed: the field, its `fixes` doc file, SolidCraft's preset line, the tests' flag toggles and the flag-off test halves (`issue_1061`'s flag-off test is deleted; `issue_1453` keeps only the exact-edge assertions).

Turning the swap on everywhere exposed three defects. Each is fixed where it arises:

- **Exact curves swapped for other exact curves.** Pieces that `clip_exact_curves_with_context` already cut as lines or conics are kept. Swapping them for differently anchored lines made the three pyramid bodies in `topology::brep::local_operation`'s tests unsound (Check 14010).
- **Lines sized by an unbounded plane.** The exact curves are sought only over the marched pieces' UV box, grown by a quarter each way, with a plane's copy cut to that box. A face given a new plane has a domain of about 1.2e6, and `IntersectWithPlane` sizes its line by the planes' boxes, so the line's ends were off both planes by that size times the unit direction's rounding (3.4e-11 in the tilted-box test), where the marched piece lies on both to 1e-15.
- **A crease cut of no length** (`crates/solidcore/src/offset/curve_on_surfaces.rs`, `cut_at_creases`). A crease crossing whose parameter rounded onto a segment's end, while its fraction stayed below 1, left a zero-length piece that no halving could prove. That made `curve_on_surfaces` refuse the exact ellipse on the tilted cylinder's offset (`issue_15282`, edge 25), so `MateEdges` kept the marched curve and the result failed the check. Such a crossing now moves that end's point onto the crease line, as a crossing whose fraction rounds to the end already did. The cause was not the curve's interval: the ellipse's interval lies inside its domain.

The exact ellipse's edge carries the offset's own B-rep tolerance (0.001339890721639), the tolerance of every edge of the inward offset and the floor of the outward one; the swap does not raise it.

- **Tests:** `issue_1453_shelled_cylinder_exact_edges`, `issue_1442_offset_walls_touch`, `issue_1061_moved_boss_top_extends_cylinder`, `issue_15282_tilt_cyl_mate_pair` (`tilt_cyl_fixture_mate_is_the_exact_ellipse`: the mate is the closed-form ellipse of the fixture's cylinder and plane, within a derived rounding allowance, and is certified on both surfaces), `offset::curve_on_surfaces::tests::a_crease_crossing_that_rounds_onto_the_end_moves_the_end`, and the three pyramid tests in `topology::brep::local_operation`.
- **Goldens:** the moved rows, their classes and the row-by-row replay proofs are listed in #21301.
