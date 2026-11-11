Updates: WR-659

#### WR-659 (update, 2026-11-07). `fillet_corner_face_curve_halves` is removed: an eased corner's patch is bounded by its face curves' halves on every kernel

The flag acted only at a corner whose rounds ease. With `fillet_gn_section_generator` removed
(WR-685's addendum of 2026-11-06), the ease's target is always the order-2 G^n section, and its
at-thirds blend curve is gone. Without the halves, every corner of the 40 mm box rounded r 8, set
back 12 and eased 3 (`issue_1376_setback_ease::every_corner_eased_eases_each_round_at_both_ends`)
is not a manifold solid: the patch's iso-curve boundaries, fitted to the halves only to
tolerance, no longer close against the eased sections. On SolidCraft's set it builds. Narrowing
that set to what the case needs gives this flag and the test's own
`fillet_setback_from_start_vertex`. So the flag is removed and `MakeFaceBrep` bounds each eased
corner's faces by the halves (`fillet_corner::p1`).

- **Results:** an eased corner's patch faces are bounded by the face curves' own halves (one
  quintic span each), as SolidCraft's set built them. Corners without an ease are unchanged.
- **Tests:** `wr_659_eased_corner_face_curve_halves` drops its flag argument. `issue_1376`'s
  every-corner ease passes on the default kernel with its setback fix.
