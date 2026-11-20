Updates: WR-75

#### WR-75 (update, 2026-11-19). The tangent curve tracer runs by default; its crossing probe and its nodes need the surfaces to meet (#18930)

Not the fork's. `Kernel::fixes.ssi_tangent_curve_tracer` is retired: the tangential curve pass
runs on every surface/surface intersection but a self intersection whose two domains overlap or
touch (the first update of 2026-11-18), with the probe and node checks of the earlier update of
2026-11-19. Before that, two more of the fork's tests are replaced, and the pass polls for
cancellation.

- **A crossing direction needs a point where the surfaces meet** (`TangentTracer::probe`,
  `meets`). For each of its 24 directions the probe solves for a contact on a plane across the
  direction, and the fork took the solution when its gap was under a tenth of the tolerance.
  Where two surfaces stay within tolerance of each other without meeting, that takes points
  that are on no intersection. In `issue_394_ssi_tangent_contact_points` (tangency-labs/solidcraft#18610)
  a quartic graph touches a plane at two points `5e-5` apart and stays within `1.6e-10` of it
  between them; the probe radius, `2e-3` model sizes, is `4e-5`. At the first touching point
  the tangential probe found no direction, and the crossing probe found three or four, with
  gaps from `1e-11` to `5e-9`. The point became a node, and a crossing branch was traced
  through both touching points and reported as a near-tangent curve, where the tests ask for
  two distinct touching points. Now, in the crossing mode, a solution counts only when the two
  surface points, as `Surface::evaluate_point` gives them, lie within the counted rounding of
  those two evaluations (`point_rounding`; a plane's evaluation counted in
  `plane_point_rounding`; a curve bounded surface's that of its basis) plus one ulp step of
  each parameter, and at least the gap at which the solver stops (`1e-17`). The tangential
  mode is judged as the fork judges: its solutions also need parallel normals, and a tangent
  curve may hold only within tolerance, as where one surface is fitted to another.
- **A node needs a point where the surfaces meet** (`find_tangent_curves`, `march`). A contact
  with crossing directions on more than one line was a node even where the surfaces only came
  within tolerance of each other there: `z = xy + 1e-9` against the plane `z = 0` gives the
  contact at the origin, `1e-9` off, with parallel normals, between the two branches of the
  hyperbola `xy = -1e-9`. Now a contact becomes a node, at a seed or where the march finds one,
  only where the surfaces meet as above.
- **Cancellation:** the pass polls `Kernel::check_cancelled` before each seed, node and
  refinement pass and at each march step; the fork's pass ran without it, and it now runs on
  every intersection.
- **Measured** with logs on the tests that run the pass: accepted solutions lie at most at
  `0.055` of their bound (`issue_395_tangential_contact_corpus` 260, `wr75_ssi_tangent_curves`
  40, `issue_394` 12), the solutions refused in `issue_394` at `1.3e5` times it or more.
  Moved by `(c, c, 0)`, the `issue_394` geometry keeps its two touching points up to
  `c = 1e3`; from `c = 1e4` it gives three touching points (which pass adds the third is not
  traced).
- **Limits:** a surface with no counted rounding bound (a cone, an offset, procedural or STEP
  surface, a surface in a volume, a power-basis net) is judged as the fork judges. The bound
  grows with the coordinates, as the evaluations' rounding does: surfaces closer than their
  evaluations' rounding cannot be told from surfaces that meet. The test is of the solver's
  result: a crossing the solver leaves farther than the bound from its point is not continued.
  The march's steps, the refinement of a traced curve and the tangential mode still accept
  points within a tenth of the tolerance. This change leaves the three torus rows below as they
  were; which step of the pass gives their curve is not traced here (tangency-labs/solidcraft#19274).
- **Tests:** the three `issue_394` tests of tangency-labs/solidcraft#18610
  (`nearby_distinct_tangent_contacts_are_not_one_contact`,
  `a_distant_c0_knot_preserves_nearby_distinct_contacts`,
  `nearby_distinct_contacts_survive_certification_against_a_trim_edge`), red with the tracer on
  before this change and green after, unchanged;
  `tangent_tracer::tests::the_probe_continues_only_where_the_surfaces_meet`: two parallel
  patches `1e-9` apart give no direction (before, every direction, and the probe called them
  coincident), a patch crossing the plane `z = 0` at `1e-3` rad gives the two directions of
  their crossing line, at the origin and `1e6` from it, with the plane as a patch and as a
  plane; `tangent_curves::tests::a_contact_where_the_surfaces_do_not_meet_is_no_node` and
  `the_tangent_curve_pass_stops_when_cancelled`.
- **Goldens that move with the tracer on by default:** the 13 rows of `surface.surface_p2` the
  earlier update of 2026-11-19 names, where the pass adds touching points at the nodes where
  crossing branches meet (`ssi_tor_purpen_tor_sing`, `ssi_bumppy_plane`, `ssi_cyl_cyl_sing`,
  `ssi_cyl_cyl_sing2`, `ssi_clasic_ruled`, the corpus' `steinmetz_equal_cylinders`) or traces
  the contact curve (the corpus' `quartic_plane_circle`, `cubic_plane_line`,
  `quartic_ridge_plane_line`); the changes above move none of them. 3 rows of
  `topology.brep_planar_section`, where a plane at the torus' bounding box touches it at one
  point, get a curve 0.175 long through that point; neither that nor the empty section of main
  is the point, so they are listed as known failing (tangency-labs/solidcraft#19274), not
  blessed. `offset.offset_geometry_creation`, `surface.surf_of_revolution_p2` and
  `blend.fillet_executive_p3` do not move.

![A section through the two touching points of a quartic graph and a plane, with the band where the graph lies within tolerance of the plane and the probe's circle around the first touching point. Before: the probe takes solutions in the band where its circle crosses the plane, and a crossing branch is traced through both touching points. After: the same solutions are refused because the surfaces do not meet there, and the two touching points stay](WR-75--addendum-2026-11-19-tracer-by-default-probe-meets.svg)

*Look at the probe's solutions on its circle: inside the band, but on no intersection.*

![Four kinds of goldens rows, main above and this change below, drawn in the plane of contact. Crossing branches: two crossing curves on main; the same curves and a touching point where they cross with the tracer. Contact circle: no curve on main; one tangent circle with the tracer. Contact line: two touching end points on main; one tangent curve between them with the tracer. Plane on a torus: no curve on main; a curve 0.175 long through the touching point with the tracer, where the section is that point (known failing)](WR-75--addendum-2026-11-19-tracer-by-default-rows.svg)

*Look at what the tracer adds in each kind of row: a touching point, a circle, a line, and on the torus a curve where a point is right.*
