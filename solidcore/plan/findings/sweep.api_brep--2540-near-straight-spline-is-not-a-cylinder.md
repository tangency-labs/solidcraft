# sweep.api_brep: #2540's near-straight spline extrudes, and its swept face is no cylinder (2026-11-03)

- **Unit:** `sweep.api_brep`, `create_linear_sweep`, with the recognition in
  `nurbs::b_spline_surface::BSplineSurface::copy_and_add_analytics`. Found while recording
  tangency-labs/solidcore#2540 as a kernel case.
- **Inputs:** the issue's profile, read into model space as the plane x = -22 (sketch origin
  (-22, 0, 44), sketch x along +Z, sketch y along -Y): a four-point clamped cubic B-spline
  (knots 0,0,0,0,1,1,1,1) over a 0.892 mm span that stands 7.1e-7 mm off its chord, then two
  straight segments closing the triangle. Extruded 44 mm along +X, capped, as a new body.
  Entry `crates/solidcore/tests/kernel_cases/sweeps/2540-extrude-near-straight-spline-recognised-as-a-huge-cylinder.json`,
  test `crates/solidcore/tests/issue_2540_near_straight_spline_extrusion.rs`.
- **Values:** the body builds -- 5 faces, 9 edges, 6 vertices, and every face passes
  `Surface::assert_valid` at level 0 with no failed test. The swept face of the spline span
  is a surface of extrusion. `brep_volume` at accuracy 1e-9 gives 98.130681517195555 against
  the profile's closed form 98.130681517195640 (Green's theorem on the cubic Bezier plus two
  segments, exactly 4906534075859782031523/50000000000000000000), 8.689e-16 relative. The
  issue's refusal -- `SmSurfOfRevolution` test 10 "Dist U", 4.15e-3 against a scaled zero of
  2.6e-9, owner `SmCylinder`, from a recognised cylinder of radius 216985 mm -- does not
  appear: `copy_and_add_analytics` on that span's extrusion net (degree 3 by 1) returns a
  surface of extrusion under every fix set the test runs -- `UpstreamFixes::default()`, the
  pinned C++ behaviour; SolidCraft's own profile (`all()` without the two it holds back,
  `fillet_keep_features` and `fillet_refuse_rounds_breaking_through`), the set the issue was
  measured through; `all()`, every flag on; and each flag of `UpstreamFixes::NAMES` alone on
  top of the default, 323 of them at this head. That is 326 sets of the 2^323 the type can
  hold, and the flags are independent fields, so a combination that is not among them can
  still differ; the sets not run are not measured here.
- **Also measured:** the same profile with the span replaced by the straight segment p0-p3
  builds the same 5 faces, 9 edges, 6 vertices, volume 98.130670362208278 -- 1.137e-7
  relative to the spline variant, the area of the 7.1e-7 mm bulge, and the 98.1307 the issue
  measures for its straight variant on both kernels.
- **Where the cylinder can still come from (unmeasured):** the issue measured through
  SolidCraft on an older solidcore pin (2dbd88b6e9 / 8ddce9016), so either that pin, or
  SolidCraft's own composition -- the face recognition when a record is read back with
  `MakeTopologyOptions::add_analytics`, or a DXF fit that hands the kernel control points
  other than the four in the issue, or a flag combination outside the 326 sets measured
  above. Not determined here: no bisection over solidcore pins, and no sweep over
  combinations of two or more flags.
- **Side observation, not reported as a defect:** with `UpstreamFixes::all()` the sweep of
  this profile fails with `Upstream { operation:
  "nurbs::b_spline_curve::p1::BSplineCurve::make_nurb", code: 1001 }`. AGENTS.md calls
  `all()` a combination that does not hold together, and this is not the issue's symptom, so
  it was not pursued. The recognition probe above runs on the net directly and is unaffected.
- **How checked:** WSL, Rust 1.98.0, `main` at ce1c7f975, worktree
  `~/dev/solidcore/wt/2026-11-03-2540-sweep-cyl`. The closed form was recomputed
  independently in exact rationals and cross-checked by Gauss-Legendre quadrature of the
  Bezier (2.8e-15 relative, the quadrature's own f64 noise).
- **PR:** tangency-labs/solidcore#2608.
