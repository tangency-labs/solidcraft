Updates: WR-263 (and the WR-262 / WR-264 branches of the same #521 class)

2026-11-02 (PR #2433, audit #1954): public-operation reachability.

The #521 stack's guards were reached so far only by tests that call the guarded
routines directly (class B-unit for #1954). One branch now has a public-operation
measurement: `crates/solidcore/tests/wr263_public_uv_domain_failed_trim_evaluation.rs`
reaches WR-263's `GetStartPointUVLoopGap` guard through public APIs only and pins two
different exact outcomes on the same face:

- `topology_failed_evaluations_propagate = false` (upstream/default):
  `max_uv_loop_gap = 1.0307764064044151` (the failed endpoint evaluation's unset value
  is measured);
- `topology_failed_evaluations_propagate = true` (the fork's WR-263 behaviour):
  `max_uv_loop_gap = 1e20` (`SM_BIG_DOUBLE`: the gap cannot be measured).

The domain (`[0,1] x [0,1]`) is identical in both states. Forcing the flag off makes the
test's flag-on assertion fail, so the WR-263 branch's evidence moves from class B-unit to
class A for #1954.

How the public inputs are made:

1. `primitive::api_primitives::create_box` gives a public solid with trimmed faces.
2. The loop's edge intervals are moved to `[0.25, 0.75]` through `Brep::edge_mut`, so the
   gap samples the trim at an interior parameter.
3. `topology::edgeuse::p1::set_uv_trim_curve` (the public record editor) replaces one edge
   use's UV trim record with a degree-1, two-control-point, two-knot NURB. The public
   NURB constructor accepts it and its control-point bounding box is defined, but
   `BSplineCurve::evaluate_point` fails at interior parameters (at 0 and 1 it still
   returns the control points).
4. `topology::face::p1::calculate_uv_domain_from_uv_trim_curves(..., want_max_gap = true)`
   is the public operation that reaches `GetStartPointUVLoopGap`.

Limits: the edited trim record is public-API constructible but not a valid NURB
configuration (fewer knots than degree and control points require); this is a
public-input isolation of the guard, not a valid-model case. The C++ reference was not
run on it, as with the other Rust-only #521 regressions whose inputs are undefined in
C++.

The other two branches have no public-operation difference with the public inputs
measured here, so no test was added for them:

- **WR-262** (`GetUVCornerBoundaries`): its only public caller,
  `query::loop::contains_uv_point`, evaluates the same trim with derivatives in its main
  winding walk and returns that error before the corner walk in both flag states. With a
  malformed trim at an inside, a boundary and an outside UV the result is identical:
  `Err(Upstream { operation: "nurbs::b_spline_curve::p1::BSplineCurve::evaluate", code:
  1001 })`. A public face whose surface evaluation fails gives the same
  `Err(Upstream { operation: "nurbs::b_spline_surface_n_lib::p1::BSplineSurface::evaluate_point",
  code: 1001 })` in both states. The existing routine-level
  `issue_521_uv_corners_skip_an_edgeuse_whose_ends_fail` remains the WR-262 evidence.
- **WR-264** (`sm_CalcDistAndDev`): the public caller
  `topology::edgeuse::p3::get_or_create_uv_trim_curve` and
  `topology::face::p1::calculate_uv_domain_from_uv_trim_curves` are identical in both
  flag states for every sequence measured on these public inputs: with the trims
  removed, both calculate-first and fetch-then-calculate return
  `Ok(UvDomainFromTrim { domain [0,0]-[0,0], max_uv_loop_gap 0.0, ... })`, the fetch
  completes with `(Ok, dist_to_surf 0.0, deviation 0.0)` either way, and with the trims
  present the same fetch outputs hold. No disjoint-extents refusal or a `-1`/
  `-1` candidate pair is observable through the public callers, so the guard's distinct
  outcomes are not exposed; the routine-level `issue_521_dist_and_dev_is_none...`
  evidence stands.
