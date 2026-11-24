Updates: WR-20239

#### WR-20239 (update, 2026-11-24). Fixed in the port

Fixed in the port by https://github.com/tangency-labs/solidcraft/pull/21498 (https://github.com/tangency-labs/solidcraft/issues/20239), with no flag.

- `Surface::domain_with_continuity` (`crates/solidcore/src/surface/surface/p1.rs`, `domain_with_continuity_using`) compares the start point's parameter with the first and last knot (`uv.x < ki`, `uv.x > ki`, and the same for `v`), not the direction's component.
  - A start point inside the knot range gets the same answer as before: there the first and last knot are the natural bounds, so the old comparison never moved them.
  - A start point off the domain now gets the span next to it, between the end knot and the nearest break. The direction neither shortens nor collapses it, so scaling or reversing the direction no longer changes the answer.
- `Surface::lift_curve` (`p11.rs`) refuses a segment of the UV curve that provably lies wholly outside the natural domain of a B-spline surface whose evaluation is clamped to that domain (out-of-bounds evaluation not enabled; there the evaluation gives only the points of the domain's edge), in a direction in which the surface is not periodic. It refuses with `SurfaceFailure::SegmentOffDomain`, status `SM_ERR`, class `OutsideDomain`.
  - The proof bisects the segment over certified boxes (`Curve::certified_bounding_box`), with an explicit stack of at most 256 boxes.
  - It refuses nothing it cannot prove. A segment that only starts off the domain is lifted as before.
  - Before this, such a segment was refused only when its continuity domain collapsed, which depended on the derivative's length.
- Verification:
  - Regression tests:
    - `domain_with_continuity_off_the_domain_is_the_span_next_to_the_point` (p1). It covers directions scaled by 1e-3, 1 and 1e3, and their reverses.
    - `lift_curve_refuses_a_segment_wholly_off_the_domain` (p11). It covers the same scales.
  - Both fail without the fix.
  - The 30 `surface.surface_p1` `DomCont` cases are no longer known failing. Their golden lines are blessed, and the goldens test `domcont_answers_are_the_span_next_to_the_start_point` derives every off-domain answer from the knots and continuities.
  - `intersect.surface_drop_curve` `adv_trace2_start_outside` now works on the plane's whole domain `[0, 10]²` instead of the point `(0, 0)` (test `a_trace_from_outside_the_plane_works_on_its_whole_domain`).
  - `offset.offset_geometry_creation` gen0152 and gen0258 change; gen0266, gen0274 and gen0577 are unchanged.
    - All four of gen0152, gen0258, gen0274 and gen0577 lift, at `ExtendTrimEdges`, an extended UV trim curve that lies wholly outside the natural domain of a surface that is not a clamped B-spline (its evaluation continues past that domain), so `LiftCurve` lifts each of them.
    - Before the fix gen0152 and gen0258 were refused by the collapsed continuity domain (the WR-20239 bug: the answer depended on the length of the curve's derivative), while gen0274 and gen0577 were lifted. Now the class is lifted alike, and no valid input becomes a refusal.
    - Test: `extend_trim_edges_lifts_every_curve_of_the_off_domain_class_alike` (goldens).
    - The off-domain refusal of `lift_curve` is tested on a clamped bilinear patch, which no longer refuses once out-of-bounds evaluation is enabled, and on a plane past its sentinel, which is never refused (`lift_curve_refuses_off_the_domain_only_where_evaluation_is_clamped`).
