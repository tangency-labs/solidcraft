Updates: WR-600

#### WR-600 (update, 2026-11-04). `fillet_nx1_tangent_side_edge_same_convexity` is removed

With the blend corner always on (tangency-labs/solidcore#2400, WR-697's addendum of 2026-11-04),
`fillet_corner::topology::tangent_seam_ends` (this flag or `fillet_blend_corner`, the addendum of
2026-11-03) was always true, so the flag no longer changed anything. It is removed, and
`tangent_seam_ends` with it: its four read sites (the side edges' convexity and tangent sides, the
run-on past the corner's vertex in `fillet_solver/p2.rs`, the tangent end rib and the cap edge
along the seam in `fillet_geom/p5.rs`) take the fix's side unconditionally.

- **Results:** none change. SolidCraft's set had the flag on; the default had the blend corner,
  which turned the same reads on.
- **Tests:** the unit test of the side edge's convexity keeps its flag-on case alone;
  `issue_2089_tangent_tracer_crease_round` and the other tests that set the flag drop the setting.
