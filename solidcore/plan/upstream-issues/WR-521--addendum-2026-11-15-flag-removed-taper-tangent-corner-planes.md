Updates: WR-521

#### WR-521 (update, 2026-11-15). `taper_tangent_corner_planes` is removed; the tangent rule is the only path, and a section that shrinks to one point closes there

A taper extrude with linear-extension (tangent) corners of a profile of lines and circular arcs with a sharp corner always
continues each wall as its tangent plane (`primitive/primitive_creation/p2/taper.rs`, `taper_walls.rs`). The opt-in
described in the original record no longer exists. Removed: the field `taper_tangent_corner_planes` and its docs,
SolidCraft's preset line, its entries in `kernel_cases/fixes.json` and the `issue_2260_gn_pipe_refined_range_end.rs` flag
list, the test settings in `wr650`, `issue_2020`, `issue_1945`, `issue_968` and `wr_571`, and the flag-off halves of
`issue_1449_taper_corner_rules.rs`.

- **What blocked it.** The rule follows the walls' sections up the height and closes one wall at a time (WR-571). Where
  the whole section shrinks to nothing below the far end, it refused the draft once fewer than three walls were left
  (`WallCloses`): a regular polygon drafted past its centre (an octagon, a 16-gon), and the issue's strip, whose short
  walls close and whose long walls then meet along a ridge. SMLib's offset method, the default, built both.
- **The fix.** At each closing height `f`, and again after two walls close face to face along a ridge, the rule checks
  whether every live wall's section has shrunk to one point `q`: both crossings of each live wall lie within
  `reach = tol s / (s + |h|)` of `q`, `s` the least speed at which a live wall moves into the material, and an arc's
  section between them is the short arc (followed up in the closing search's steps). The section is then gone within
  `reach / s` of that height, so every point of the true walls' meeting above it lies within the B-rep's tolerance `tol`
  of the apex: the kernel's coincidence, points within it of one vertex are that vertex. Then every live
  wall's seams end at `q` and the section closes: a pyramid's apex, or the far end of a ridge. A section that closes with
  walls still apart, a wall closing within the tolerance of the far end (the far end would pinch), and an arc shrinking to
  its centre are still refused.
- **Defaults:** SolidCraft's preset had the flag on; its results change only where it refused a section closing at one
  point, which now builds. `Kernel::default()` changes from SMLib's offset method to the tangent rule.
- **Goldens:** two rows of `sweep.api_brep` move (the PR's table): `gen0216`, a rectangle drafted outward, from 12 faces
  to 4 (the corner triangles were coplanar with the walls beside them; the surface is the same), and
  `review_draft_square_h0`, a square of height 0, from a two-face body of zero volume to a refusal (a height within the
  tolerance is refused as such before the walls are built).
- **Tests:** the octagon and the 16-gon build their pyramids and match the closed-form apex and volume
  (`hang_taper_extrude_past_centre.rs`); the strip builds its hip roof, `b^2 (3a - b) / (12 tan 10)`, under both rules
  (`issue_1449_taper_corner_rules.rs`, SolidCraft's `extrude_draft_wall_closes`); an apex or a ridge on the far end is
  still refused.
