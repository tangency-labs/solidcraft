Updates: WR-20239

#### WR-20239 (update, 2026-11-24). Fixed in the port

Fixed in the port by https://github.com/tangency-labs/solidcraft/pull/21498 (https://github.com/tangency-labs/solidcraft/issues/20239), with no flag.

- `Surface::domain_with_continuity` (`crates/solidcore/src/surface/surface/p1.rs`, `domain_with_continuity_using`) compares the start point's parameter with the first and last knot (`uv.x < ki`, `uv.x > ki`, and the same for `v`), not the direction's component.
  - A start point inside the knot range gets the same answer as before: there the first and last knot are the natural bounds, so the old comparison never moved them.
  - A start point off the domain now gets the span next to it, between the end knot and the nearest break. The direction neither shortens nor collapses it, so scaling or reversing the direction no longer changes the answer.
- `Surface::lift_curve` (`p11.rs`) is unchanged: with the continuity domain no longer collapsing, a UV segment off the domain is lifted onto the surface's own evaluation there whatever the speed of its parameterisation (onto the domain's edge for a B-spline whose evaluation is clamped, onto the continued surface otherwise). No new refusal is added.
- Verification:
  - Regression tests:
    - `domain_with_continuity_off_the_domain_is_the_span_next_to_the_point` (p1). It covers directions scaled by 1e-3, 1 and 1e3, and their reverses.
    - `lift_curve_off_the_domain_is_the_same_whatever_the_speed` (p11). It covers the scales 1e-3 and 1 (1e3 hits a separate ApproximateCurve parameter-scale issue, #21657, as on main) on a dome with out-of-bounds evaluation enabled, checking every lift against the dome's closed-form polynomial; on main the slow segments were refused (`NoContinuousDomain`).
  - Both fail without the fix.
  - `lift_curve_off_the_domain_follows_the_surface_s_evaluation` (p11) lifts the same off-domain segment onto a clamped patch's edge, onto the plane with out-of-bounds evaluation on, and on an analytic plane past its sentinel.
  - The 30 `surface.surface_p1` `DomCont` cases are no longer known failing. Their golden lines are blessed, and the goldens test `domcont_answers_are_the_span_next_to_the_start_point` derives every off-domain answer from the knots and continuities.
  - `intersect.surface_drop_curve` `adv_trace2_start_outside` now works on the plane's whole domain `[0, 10]²` instead of the point `(0, 0)` (test `a_trace_from_outside_the_plane_works_on_its_whole_domain`, which also rebuilds main's rec block bit for bit from the new record with the domain's maximum set back to `(0, 0)`: the move is `(+20, +20, +850)`).
  - `offset.offset_geometry_creation` gen0152 and gen0258 change; gen0266, gen0274 and gen0577 are unchanged.
    - All four of gen0152, gen0258, gen0274 and gen0577 lift, at `ExtendTrimEdges`, an extended UV trim curve that lies wholly outside the natural domain of a surface that is not a clamped B-spline (its evaluation continues past that domain), so `LiftCurve` lifts each of them.
    - Before the fix gen0152 and gen0258 were refused by the collapsed continuity domain (the WR-20239 bug: the answer depended on the length of the curve's derivative), while gen0274 and gen0577 were lifted. Now the class is lifted alike, and no valid input becomes a refusal.
    - Test: `extend_trim_edges_lifts_every_curve_of_the_off_domain_class_alike` (goldens): every `ExtendTrimEdges` of the four returns status 0, and each UV trim of its offset edge is certified (an enclosure, `trim_bounds`) within the edge's tolerance of the edge's curve.
