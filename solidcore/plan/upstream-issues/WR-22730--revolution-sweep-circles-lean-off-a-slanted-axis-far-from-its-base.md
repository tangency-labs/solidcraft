#### WR-22730. A surface of revolution's sweep circles lean off a slanted axis near the axis and far from its base point
- **Severity:** Wrong result
- **Area:** Sweeps (rotational sweep, analytic faces)
- **Location:** `source/SMLib/src/SmBSplineSurface.cpp`, `SmBSplineSurface::CreateSurfOfRevolution`,
  ported as `nurbs/b_spline_surface/p2.rs` `BSplineSurface::create_surf_of_revolution_with_line_vector`:
  the sweep radius vector of each generator control point is the point less its foot
  `origin + t axis`, `t = (point - origin) · axis`.
- **What happens:** the foot's rounding leaves a part of the vector along the axis, about
  `1e-16` of the distance from the axis base point. For a control point near the axis and
  far from the base point that is a large part of the radius: `4e-16` of a `1.1e-3` radius
  5 from the base. The circle's frame is built from that vector, so its x axis leans `4e-13`
  along the rotation axis and the circle's control points leave the plane of the turn by
  `4e-16`, about 400 times the rounding of points of that size. Here the circle a profile's
  end sweeps near the axis is then off the plane of its own analytic circle (built with an
  exact frame), and the solid's record is refused: "a conic's NURBS is off its own frame
  and radii".
- **Trigger:** a whole turn about the axis through (4.330127018922194, 2.5, 0) along
  (0.8660254037844387, 0.5, 0) of a profile with a control point at
  (5.499999999987182e-4, -9.526279441618435e-4, 0), `1.1e-3` off the axis.
  tangency-labs/solidcore#3389.
- **How found:** tangency-labs/solidcore#3389 (a revolve whose profile arc ends a hair off
  its axis has no record).
- **Confirmed:** no C++ run here; the port shows it: the control points of the first sweep
  circle are `4.0e-16` off the plane square to the axis without the fix.
- **Fix (always on, no flag):** the port takes the radius vector square to the axis before
  measuring it (`x_vec -= axis_dir * x_vec.dot(axis_dir)`). Where the foot leaves nothing
  along the axis (every axis along x, y or z through a base whose rounding is exact, and
  every point whose foot rounds exactly) the surface is unchanged bit for bit; elsewhere
  the sweep circles move by at most the foot's rounding, now square to the axis.
- **Tests:** `nurbs::b_spline_surface::p2::tests::revolution_near_a_slanted_axis_far_from_its_base_stays_square_to_it`;
  `solidcore-ops` `sweep::a_whole_turn_of_an_arc_ending_a_hair_off_its_axis_writes_its_record`
  (which also needs the non-ported `sweep::analytic_revolution` fix, tested by
  `sweep::analytic_revolution::tests::the_radial_direction_of_a_point_near_a_slanted_axis_is_square_to_it`).
