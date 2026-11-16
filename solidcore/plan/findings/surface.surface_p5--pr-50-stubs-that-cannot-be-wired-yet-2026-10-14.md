# surface.surface_p5 (PR #50): stubs that cannot be wired yet (2026-10-14)

- **`SmIsoCurve` over non-B-spline surfaces.** `IsoCurve` holds a `BSplineSurface`, so
  `Surface::create_iso_parametric_curve` (the `SmSurface` body, used for curve-bounded,
  STEP and the other non-overriding kinds) and `sp_local_solve`'s boundary curves return
  `Unported` (`curve.iso_curve`) for those kinds. Upstream builds the iso curve over the
  surface itself; fixing it means `IsoCurve` holding a `Surface` (a change to
  `curve.iso_curve` and its users).
- **Plane iso curves.** Upstream's `SmIsoCurve` over an `SmPlane` evaluates through the plane's analytic
  `Evaluate`; the Rust `IsoCurve` holds the plane's NURB, and the two differ in the last bits (a
  difftest of `sm_SpLocalSolve`'s boundary solves on planes disagreed at 1e-9 on values that converge to
  zero). `iso_curve_of` therefore defers on planes (`curve.iso_curve`) until `IsoCurve` holds a `Surface`.
- **Not wired (no stub tagged to surface.surface_p5 is left):** the `SmCrvOnSurf` branch of `curve_p6` needs the dropped
  `pOptSurfCrvOnSurf` and `pOptCrvOrientation` arguments of `DropPoint`; `SmSurface::DropPoint` and
  `LocalPointSolve` of the generic callers (`curve::curve::p2`, `nurbs::b_spline_curve_n_lib`) go through
  `SurfaceEvaluate::{drop_point_minimize, local_point_solve_minimize}`.
- **Kernel arguments.** PR #106 threads the caller's kernel through `Surface::drop_point`,
  its fast/local/global solves and their callers. `Surface::cover_other_coincident_surface`
  still calls `convert_domain_from_*` with a default `Kernel`.
