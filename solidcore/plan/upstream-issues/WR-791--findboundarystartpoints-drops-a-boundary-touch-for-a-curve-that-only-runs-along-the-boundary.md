#### WR-791. `FindBoundaryStartPoints` drops a boundary touch for a start point whose traced curve only runs along the boundary
- **Severity:** Wrong result (a cut keeps a whole corner of the body)
- **Area:** Intersection
- **Location:** `source/SMLib/src/SmSurfaceIntersector.cpp:2066` (`SmSurfaceIntersector::FindBoundaryStartPoints`,
  def. `:1078`) skips a touching point that `IsPointOnStartPoint` finds on a start point, on the premise that the
  curve traced from that start point covers it. solidcore `intersect/surface_intersector/p3.rs`
  `find_boundary_start_points_body`, `add_deferred_touching_points`.
- **What happens:** a surface's boundary curve touches the other surface at one point, and the boundary curve's
  solve with the other surface gives that point (here a short range, refined to one: one surface straight along the
  boundary, the other curving away from it to second order, so the two stay within the tolerance of each other for
  a short stretch). The other surface's boundary gives a start point on the same point. When that start point lands
  just inside the domain, the tracer follows the near contact along the boundary and returns a short crossing curve
  that never leaves the tolerance band of the boundary curve; the touching point is dropped for it. An edge on that
  curve cuts from the face a sliver narrower than the tolerance along its boundary edge, and the boolean keeps or
  drops a whole corner of the result with it. Whether the start point lands inside depends on the last bits of the
  solve, so the same construction at different sizes gives different bodies.
- **Trigger:** a half pipe along x from -40 to 40 (the region 23 ≤ r ≤ 25, z ≥ 0 of the YZ plane, closed below
  z = 0 by two half discs of radius 1.0000125) cut by a ring from z = -h to h (an 84 × 54 rectangle less a rounded
  rectangle whose sides y = ±25 and x = ±40 are joined by arcs of radius 24 about (±16, ±1)). A cap cylinder of
  the pipe and the ring's corner cylinder touch at (-16, -25, 0). At h = 30 the corner cylinder's start point
  lands at z = +3e-15 and traces nothing; at h = 36.75 it lands at z = -1.4e-12, inside the cap, and traces a
  crossing curve 5e-4 long that stays within 3.1e-7 of the cap's boundary line (the tolerance is 5e-6), and the
  cut keeps that corner of the pipe whole (9 faces instead of 10, +575.19 mm³). At h = 250 both corners
  (-16, -25) and (16, 25) do.
- **How found:** tangency-labs/solidcraft#11749 (a symmetric extrude cut gives a different body for different
  lengths).
- **Confirmed:** yes, `crates/solidcore-ops/tests/boolean.rs`
  `a_symmetric_cut_through_a_half_pipe_is_the_same_for_every_length_past_it` (h = 30, 36.65, 36.75, 50 and
  250: 10015.883451427 mm³, 10 faces, 28 edges and 20 vertices at each).
- **Fix (always on, no flag):** the touching point is kept aside (WR-51 addendum of 2026-11-16) and, at the end
  of `DoIntersection`, every crossing curve (type crossing or near tangent) through it that stays within the
  tolerance of the boundary curve it lies on (each distinct knot of the curve and each midpoint between two of
  them drops onto the boundary curve closer than the tolerance) is removed; the touching point takes its place
  (`SurfaceIntersector::runs_along_boundary`). Tangent, coincident and region boundary curves stay. Test:
  `intersect::surface_intersector::p3::tests::a_crossing_curve_along_the_boundary_of_a_deferred_contact_gives_way_to_it`.
