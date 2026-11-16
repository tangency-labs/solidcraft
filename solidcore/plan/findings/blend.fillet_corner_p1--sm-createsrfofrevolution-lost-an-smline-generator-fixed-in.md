# blend.fillet_corner_p1: sm_CreateSrfOfRevolution lost an SmLine generator (fixed in PR #641)

- **What.** `create_srf_of_revolution` (`blend/fillet_corner/p1.rs`) passed only the
  generating curve's B-spline part to `BSplineSurface::create_surf_of_revolution`.
  Upstream's `SmBSplineSurface::CreateSurfOfRevolution` checks `pGenCurve->GetType() ==
  SmLine_TYPE` and, for a line parallel to the axis (a cylinder), reparametrises the sweep
  circle with arc length. With an `SmLine` generator the Rust surface's circular knots were
  `[0, 0.5, 1]` where upstream's are `[0, L/2, L]` (`L` the arc length).
- **How found.** `blend.fillet_corner_p2`'s new `analytic` cases with real `SmLine` /
  `SmCircle` edges (four-sided rail-rail corners): 9 cases differed only in the corner
  surface's knots; the same inputs through a no-op corner operation matched.
- **Fix.** The line's `line_vector()` is passed as `gen_line_vector`
  (`create_surf_of_revolution_with_line_vector`), as `rotational_sweep_geometry` and
  `SurfOfRevolution` already do. `blend.fillet_corner_p1`'s difftest is unchanged (its
  cases never revolve an `SmLine`).
