Updates: WR-680

#### WR-680 (update, 2026-11-03). A joint crossing within a pole of a knot is no break of the exact trim (#2427)

- **What was wrong.** `pieces` in `crates/solidcore/src/intersect/surface_drop_lib/cylinder_section.rs` cut the curve at its knots and at every crossing of a joint between the cylinder circle's rational quadratic segments, and merged only breaks closer than `1e-12` of the interval. Where a crossing fell just past a knot, the piece between them stayed in the UV curve. All its control points lie within `SM_ZONE_TOL_3D / 10` (1e-6) of each other, so check test #15 on `SmBSplineCurve` ("Bad Repeated CPts", `HasInternalPole`) reports the trim. #1529's two r16 tubes with a 1e-6 gap and every fix on show it: the seat's ellipse on the top tube's wall has its knot 0.25 and a joint crossing at 0.2500000019812843.
- **The construction now.** A joint crossing is a break only when the piece it would close is not a pole as check test #15 measures one: the Bezier control points of the span from the previous break to the crossing, or from the crossing to the next knot or interval end, are not all within `POLE = ZONE_TOL_3D / 10` of the first (`is_pole`). Knots and the interval's ends are never moved or dropped. The piece next to a dropped crossing runs past the joint by less than a pole on the extension of its own segment's rational quadratic, which is still the circle; `exact_uv_curve` keeps measuring the lifted curve against the curve and uses it only where it is on the curve to round-off.
- **Measured** (`a_joint_within_a_pole_of_a_knot_leaves_no_pole_span`, `a_joint_well_clear_of_a_knot_is_a_break`, an r16 cylinder and a tilted section over `[0.05, 0.95]`):

  | section | before | after |
  |---|---|---|
  | knots 1.6e-7 (`R * 1e-8`) from the joint crossings | 7 spans, the shortest 1.6e-7 long | 4 spans (the knots alone), no internal pole, lifted gap under 1e-12 |
  | knots 1.6e-2 (`R * 1e-3`) from the crossings | 7 spans | 7 spans (unchanged), lifted gap under 1e-12 |

  `wr575_every_fix_at_1e_6_adds_no_check_failure` (`crates/solidcore/tests/wr579_near_coincident_tube_ends_join.rs`, un-ignored): before, the flag-on check reports `["SmBSplineCurve::Test #15", "SmFace::Test #36"]` against the flag-off `["SmEllipse::Test #1", "SmEllipse::Test #2", "SmFace::Test #36"]`; after, the flag adds no failure and the test passes.
- **Scope.** Only `cylinder_plane_section_trims_exact` on reaches `pieces`; the default kernel is unchanged. No tolerance is loosened: `POLE` is check test #15's own tolerance, and the round-off gate of `exact_uv_curve` is unchanged.
