Updates: WR-713

#### WR-713 (update, 2026-11-17). `cone_unbounded_parameterized_as_nurb` is removed

The fix is the default and its flag is gone (tangency-labs/solidcraft#14246, #18160, together with WR-704's `cone_iso_circle_at_axial_height`). Removed: the field `UpstreamFixes::cone_unbounded_parameterized_as_nurb`, SolidCraft's preset line, its entry in `fixes.json`, and every test setting of it.

Running with the fix always on exposed further defects in the same constructors. They are fixed at the root (`surface/cone/p1.rs`, `surface/cone/p2.rs`):

- **The canonical frame.** `Cone::canonical_frame` takes tiny and huge axes as directions. `SetCanonical` unitizes an axis with a coordinate of 1e300 to zero and still reports success, so a frame built that way is checked and refused.
- **`Cone::new_unbounded`** refuses inputs that define no cone, each decided in closed form from the input numbers:
  - a non-finite origin or radius;
  - a semi-angle of 90 degrees modulo 180, or one that is not finite: `tan` has no value there, and the generator perpendicular to the axis sweeps a plane, every point at height 0;
  - a zero or non-finite axis: it has no direction, so there is no axis `Z`.

  A semi-angle outside (−90, 90) is taken modulo 180 into it. `s` and `s ∓ 180` have the same tangent and the same generator line, so they are the same cone; the class documents its angle between −90 and 90. Upstream kept such an angle, and a semi-angle of 120 ran its domain from the apex away from the base circle.
- **The unbounded generator.**
  - It is built in the canonical frame.
  - It is rotated as a vector for every cone that is not a cylinder. Upstream rotates the point `origin + Z` and then subtracts `origin`, and at an origin around 1e5 that rotation is lost in rounding.
  - The apex end of the domain is clamped to `SM_INFINITE_PARAMETER`.
  - `transform` resets the generator's speed and domain when it scales.
- **`Cone::new_bounded`** takes its generator axes from the canonical frame. With axes that are not orthonormal, upstream put the generator at `origin + radius X`, which gives a cylinder of radius `radius |X|`.
- **`Cone::create_offset_surface`.**
  - The kept cone of a truncated offset runs from the cone's own STEP heights. Upstream placed it as if the bottom were at the origin, which for an unbounded cone gives another cone.
  - A copied offset rebuilds its polar circle on the offset cone. Upstream kept the original's circle, which is off the offset by the offset distance.

- **Results:** 4,541 goldens rows move (against main after the WR-662 retirement, #18066):

  | Unit | Rows |
  |---|---|
  | `surface.cone_p1` | 3,561 |
  | `surface.surface_p2` | 899 |
  | `surface.cone_p2` | 27 |
  | `surface.cone_p3` | 14 |
  | `intersect.api_intersectors` | 8 |
  | `nurbs.b_spline_surface_n_lib_p2` | 24 |
  | `primitive.api_primitives` | 6 |
  | `surface.surf_of_extrusion_p2` | 1 |
  | `topology.brep_p1` | 1 |

  `crates/goldens/tests/wr704_wr713_cone_moves.rs` (ignored; it runs on the inventory of moved rows) certifies every row by closed form or certificate:
  - 3,921 rows are CLOSER: the new output meets the closed form and the old one does not, by at least 10 times its bound (`CLOSER_MARGIN`; the bounds' `γ_n` constants are not derived to the last factor, so a smaller failure is classed EQUAL). The report prints the distribution of those ratios.
  - 620 rows are EQUAL: both outputs meet it, or the old one fails it by less than 10 times.
  - No row is FARTHER, and no row rests on an argument.

  179 rows change a built cone into a refusal, each for inputs that define no cone, decided in closed form from the input numbers: 165 unbounded cones with a semi-angle of exactly 90 degrees (a plane), 13 with a zero axis (no direction), and one cylinder of radius 0 (a line).

  32 moved rows (2 `cone_p1`, 16 `cone_p2`, 14 `cone_p3`, listed in the proof's module documentation) have an axis upstream cannot unitize: `(0, -1e-300, 0)`, `(0, -1e300, 1)`, or `5e-13` (below upstream's unitize threshold). Upstream's definition unitizes both axes, so the cone it defines does not change when an axis is scaled by a positive number. The proof re-runs each such row with each axis scaled by the power of two that brings its largest coordinate into `[1, 2)` (exact), certifies that run with the ordinary certificate, and requires the row's new value to equal it (26 bit for bit, 6 within the goldens' comparison: two `cone_p1` builds and four `cone_p2` offsets). So these rows are proven by scale invariance. Two `surface.cone_p2` trims refuse where main succeeded: on the cone the inputs define, the trim ellipse lies inside the solid nappe, away from the surface, while main replaced the tiny axis by `Z` and built another cone.

  The 24 `nurbs.b_spline_surface_n_lib_p2` corner blends are certified for G0. For G1 they are certified by the sup-norm angle on each side against the neighbour's implicit normal. That sup is corner-saturated (the shared loop curves fix it at the corners), so EQUAL there says the corners agree, not the side interiors.

  12 rows moved because the neighbour did. The other 12 moved because the Coons refinement in `make_coons_patch_tangent_to_sides` now compares copies side by side instead of by one rounded maximum:
  - Every copy is measured at the same stations: the union of the bare patch's and every refined copy's knot stations, against side normals read once from the bare patch.
  - A station counts only where its angle bound is certified (error below a right angle).
  - No corner stations are placed at a collapsed corner (the apex of a three-sided corner, next to a side with no surface). Those stations assume a corner normal the loop curves fix, which an apex lacks; next to it the patch normal is a cross product of a vanishing derivative, and the stations read rounding. All of that end's corner stations go (from `2^-2.25` of the end gap down to `2^-16`), so a rise inside the last gap of the common span stations is unseen there.
  - A copy is taken only when, against every copy held so far, each side the held copy certifies is certified and either no worse (its upper bound at most the held lower bound plus a resolution of 0.5 degrees) or rises: certainly above the held copy, but by at most the resolution. A rising side is accepted only in exchange for a lower seam: the copy's largest angle, at the stations the held copy certifies, must be certainly lower than the held copy's by more than the resolution. Otherwise a side certainly worse vetoes the copy even when the overall maximum improves, an undecided side counts as not no-worse, and comparing against every held copy keeps a chain of ties from drifting.

  The proof replays each refinement:
  - Every station's derivative and angle bounds are recomputed in independent interval arithmetic. They enclose the exact values at most 0.23 to 0.38 of their width.
  - Every candidate is measured at the bare patch's stations, and an independent replay of the rule picks the logged copy, which the output holds bit for bit.
  - Main's patch is found among the candidates. A side counts against a patch only with a station where it is certainly farther from tangent and none where it is certainly nearer.

  analytic064 (torus) and analytic111 no longer move:
  - analytic064's refined copies are certainly worse along two sides, so it keeps main's bare patch.
  - analytic111 takes three refinements and then loses at 64 spans, so it takes the 32-span copy, which is the one main took.

  `crates/goldens/tests/wr713_coons_tie_finer.rs` runs analytic028 and analytic064 with the kernel's Coons log on, checks the logged choice (the 64-span copy; the bare patch), and checks that the output holds that candidate bit for bit. The `p2.rs` unit tests cover:
  - a worse side hidden by a tie (`a_tie_does_not_hide_a_worse_side`);
  - a worse side with a better maximum (`a_worse_side_vetoes_a_better_maximum`);
  - an undecided side (`an_undecided_side_is_not_no_worse`);
  - a lost certification (`a_lost_certification_is_not_better`);
  - a chain of ties (`a_chain_of_ties_does_not_drift`);
  - a bound of a right angle (`a_bound_of_a_right_angle_certifies_nothing`);
  - common stations (`candidates_are_measured_at_common_stations`);
  - a patch with no certified station, and a collapsed corner;
  - no corner stations at a collapsed corner (`no_corner_stations_at_a_collapsed_corner`), which the L-shaped box's three-sided corner needs (`fork_cae8618_l_box_corner_patch_is_tangent`, a regression test that samples the seam);
  - the rise-for-a-lower-seam exchange (`a_certain_rise_within_the_resolution_trades_only_for_a_lower_seam`), which `blend_crossing`'s crease corner needs, and which enclosure ends it compares (`lowers_sup_compares_the_candidate_upper_end_with_the_held_lower_end`).

  The rule also moves one `topology.brep_p1` row, `Endcap_corner_blend_3`, a whole-solid corner blend:
  - Main took a refined copy whose bottom side reaches 1.1353 ± 2.6e-5 at a station where the bare patch is certified at 1.0752 ± 3e-4. Every refined copy is certainly worse there, so the bare patch is kept.
  - Its neighbours have no implicit form, so only the replay and the pointwise comparison apply. Main's copy is also certainly nearer at other stations, so neither copy dominates the other: the row is EQUAL as certified pointwise incomparability, not as equal sup-norm angles.
  - `vase_rev_corner_blend_3` is vetoed the same way and equals main.

  The certificates:
  - Built cones are checked net by net with `implicit::lies_on`.
  - Intersections of cylinders, of a cylinder and a cone, and of a cylinder and a plane are checked against their closed-form lines or emptiness.
  - The corner blends are checked side by side against their loop curves, by Bézier blossoms, and the curves on their cylinder.
  - The extrusion and cone curves are checked by convex-hull plane distances and by Bernstein bounds of the implicit cone.
- **Tests:**
  - `crates/solidcore/tests/issue_2313_unbounded_cone_iso_circle.rs` asserts the fixed behaviour only.
  - `crates/solidcore/tests/wr713_cone_root_fixes.rs` has a test for each root fix above against its closed form. Each was checked to fail with its fix reverted, except the refusals, which `unbounded_cone_refuses_inputs_that_define_no_cone` asserts directly.
