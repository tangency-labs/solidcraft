Updates: WR-588

#### WR-588 (update, 2026-11-02). Gated at operation level through a boolean cut and the body's tight box (#1954)

The #1954 flag audit (tdamsma, `flag-reachability-2026-10-31`) classed `face_tight_box_searches_trim` as B-unit: the test that failed with the flag forced off, `tight_box_finds_a_trimmed_faces_peak_when_the_surface_peaks_outside`, calls the face's tight box on a trimmed sheet built directly. No test showed an operation reaching it.

`wr_588_barrel_cut_below_its_waist_is_boxed_to_its_upper_bulge` in `crates/solidcore/tests/issue_1799_tight_box_divides_at_knots.rs` now does. A cubic B-spline profile from (5, 0, 0) to (5, 0, 10), with a tall bulge near z = 2 and a lower one near z = 8, is closed round a bore of radius 1 and revolved a whole turn about z with `create_rotational_sweep`. `boolean_difference` then removes the box over y >= 0, z <= 5, and `brep_bounding_box(tight)` boxes the result. Only this flag differs between the two runs, on a default kernel otherwise. The expected values come from the profile curve itself (golden-section search for the crests, bisection for the waist), not from the box code.

| input | flag off (pinned upstream) | flag on (the fork) |
|---|---|---|
| barrel cut below its waist on one side (7 faces) | y max 5.052528, the cut edge's radius at z = 5 | y max 6.988166, the upper bulge's crest |
| control: the uncut barrel | box +-9.175417 in x and y (the tall bulge), z 0 to 10 | the same |

The other five sides of the cut body's box agree either way, all within 1e-9 of the profile's values. Off, the global solve finds the tall bulge on the cut-away side, classifies it outside the face and drops it, so the box comes from the edges, as the record says.

Mutation: with the production read in `calculate_tight_bounding_box_into` (`crates/solidcore/src/query/face/p3.rs`) forced off, the test fails at the flag-on assertion with y max 5.052528. Restored, it passes. (The two older tests in that file both pass under the same mutation: their trimmed cylinder's box is its edges' either way.)

No library code, tolerance, unit status or known mismatch changes. Per the policy in #2392 the fork is the reference here, so this test is the parity gate for when the flag becomes the default.
