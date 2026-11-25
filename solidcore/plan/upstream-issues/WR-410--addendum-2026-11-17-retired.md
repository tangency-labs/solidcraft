Updates: WR-410

#### WR-410 (update, 2026-11-17). The certified best crossing is the default; the flag is retired

`Kernel::fixes.boolean_vertex_by_best_crossing` is removed (tangency-labs/solidcraft#14246, carrying
tangency-labs/solidcraft#15004 by @bherbruck). `FindOrCreateProtoEdge` (`topology/proto_topology/p3.rs`) always
passes each end's proto vertex to `add_crossing` with the angle at which the face pair's curve crosses the edge and
that curve. The first crossing places the vertex definition. A later crossing whose misplacement bound along the edge
(gap over sine) is smaller moves it only when, for each side that moves along an edge, two certified bounds
(`query::curve_surface_gap::curve_piece_gap_bound`, every rounding included) are within the definition's tolerance:
the edge stretch between the old and the new point against the other object (a point, a surface with a NURBS net, or
a curve), and the same stretch against the first crossing's curve piece between its first point and the new point
dropped onto it. Anything the bound cannot certify (an offset surface, a failed drop, no first curve, a face uv that
moves without the other side's edge stretch to bound it) keeps the first placement. Both bounds are taken from the first placement, so the result does not depend on the order in which
later crossings arrive. The pure upstream behaviour (the first crossing always places the vertex) is no longer
reachable in the Rust kernel; the C++ defect is unchanged in v1.0.1, as recorded above. SolidCraft's preset already
had the uncertified move on; it now gets the certified one.

Effect on the default kernel (goldens checked on solidcraft main a39b74ec5 merged with the change): two rows move, both in
`offset.offset_executive_p2`, with their counts and status unchanged.
- `extrude_holes1_face0_inset24_7_rounded`: vertex bounds lo.x, lo.y by about 1e-7, lo.z by 2.8e-13, hi.y by 1.0e-6,
  hi.z by 9.8e-7.
- `shell_file_problemShell10_face1_round`: hi.z by 1.03e-5 (a different vertex is now highest).

Every moved bound's vertex is closer to the surfaces of the faces it bounds (largest gap 2.75e-7 -> 2.1e-12, 9.20e-7
-> 3.66e-7, 3.3e-13 -> 1.5e-14, 9.73e-6 -> 1.9e-14; one unchanged at 7.93e-7):
`crates/solidcore/tests/wr_410_best_crossing_offset_vertices.rs`. The two extrudeHoles1 hi bounds are also checked
against an exact reference, the common point of the surfaces the vertex belongs on (Newton's method, its own error
bound under 1e-10 given exact surface distances). For hi.y every distance is a closed form. For hi.z the revolution's
distance is `surface_closest_point` with its normal, a numerical projection, not a closed form, and the error bound
assumes that projection exact:
- hi.y: its faces lie on a plane, a cone and a sphere. Their common point is 1.44e-6 from the first crossing's vertex
  (1.20e-6 in y) and 5.68e-7 from the moved one (1.65e-7 in y). The moved vertex is closer, not exact: it is still
  3.66e-7 off the cone. Its gap to one edge grows from 2.43e-6 to 2.62e-6 (under the vertex tolerance 1e-5).
- hi.z: its faces lie on one sphere and one surface of revolution, and one of its edges is an exact ellipse, the
  sphere's section by the plane x = 27.5. The common point of the sphere, that plane and the revolution is 1.88e-6
  from the first crossing's z (4.72e-6 in all) and 9.03e-7 from the moved z (9.69e-7 in all). Edge 125, a fit on the revolution,
  passes 3.63e-6 from that point, so its gap to the vertex grows from 6.83e-7 to 2.85e-6 as the vertex moves toward
  it; the vertex's largest edge gap falls from 4.70e-6 to 2.85e-6.

In problemShell10 the vertex (-50.8, -114.3) would move against a face on an offset surface, which has no certified
bound, and keeps its first placement (7.49e-7 off a face).

Results that the uncertified move changed and the certified one does not:
- `issue_435_m44x1_edges_end_at_their_vertices` (`tests/issue_410_435_thread_groove.rs`): the worst edge ends
  3.5414e-6 from its vertex, as on main.
- The WR-614 strip unions (slopes 0.02 to 0.5, d from 5e-6 to 1e-4): every result equals main's, including slope 0.2
  at d = 2e-5 (10 faces, Level 2 clean), which the uncertified move had left with 11 faces and SmFace test 33.
- A sphere cut by a box edge across a shallow cap (`sphere_box_edge_cutting_a_shallow_cap`): both crossings have sine
  0.70; the move of 1.18e-4 is refused by both bounds (1.75e-4 each, tolerance 1e-5).
- The loose-spline join (`solidcore-ops` `a_join_whose_wall_crosses_a_loose_spline_edge_at_a_shallow_angle_keeps_the_tool`):
  the first crossing is steep (sine about 1.0, gap 1.57e-3), the later one has gap 7.6e-12; the move of 5.69e-2 is
  refused because the first crossing's curve leaves the edge at once, so the stretch is not within the tolerance
  9.13e-3 of it (its bound against the face alone, 1.64e-3, would have passed).
