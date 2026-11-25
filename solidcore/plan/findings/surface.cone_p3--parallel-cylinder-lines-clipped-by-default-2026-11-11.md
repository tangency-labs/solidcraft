# surface.cone_p3: parallel-cylinder lines clipped to both faces move 151 golden rows (2026-11-11)

- Unit: surface.cone_p3 (WR-259; the flags `cone_parallel_line_midpoint` and
  `cone_parallel_line_domains` retired, see
  `plan/upstream-issues/WR-259--addendum-2026-11-11-flags-removed-cone-parallel-line-midpoint-and-domains.md`).
- No other unit moves: `cargo run --profile goldens -p goldens -- --all --jobs 0 --goldens`
  at the fix: 283 units, 282 pass, `surface.cone_p3` changed=151 added=0 removed=0 broken=0.
- A row of this unit records the status, the curve count and a digest of the curves'
  numbers. A row with an infinite number in its record (a line's NURB) digests to
  `[-inf, inf, -inf]`, so the golden sees whether a case has lines, not where they end.
- How the rows were explained: a temporary test (not committed) ran every case of the
  unit on the kernel before this change twice, with both flags off (main's default) and
  with both on (the path that is now the only one), and printed each case whose output
  differs: 156 of 1490. 151 of them are the rows that move. The other 5 change only where
  their lines end, which the digest cannot see (below).
- Independent check of every new line: the two faces' axial ranges come from evaluating
  each surface at its domain's four corners and projecting the points onto the first
  cylinder's axis. Every new line starts and ends at the overlap of the two ranges, to
  the case's tolerance. Main's code built each line over the first cylinder's whole
  height and tested it at its bottom end.

## Why the new rows are right

- **Main missed the lines (140 rows).** The lines' bottom end lies outside a face whose
  domain starts above it (an axial trim, or the other cylinder starting higher), so main
  skipped both lines although they run through both faces. The new lines span the overlap.
  `issue711_*` unit tests in `surface/cone/p3.rs` and `tests/issue_711_overlapping_holes.rs`
  check such cases against closed forms; on main they fail.
- **A degenerate cone's lines are gone (3 rows, `fuzz_axis_overflow_zero_y*`).** The second
  cone's y axis `(0, -1e300, 1)` unitizes to zero (WR-159), so its surface evaluates to
  points `(cos u, 1.6, z)`, z in [1.6, 5.6], all 1.6 or more from the first cylinder's axis
  (radius 1, tolerance 0.1). The faces share no point. Main returned the two lines of the
  full circles' construction; the new path tests them inside both faces' domains and drops
  them, so the result is empty.
- **A refusal reports another status (8 rows, `rejected_*`).** These cones have a zero or
  tiny axis and no polar converter (inputs the C++ crashes on). Main refused with
  `status_1007` (`InvalidInput` from `convert_uv_from_step_to_nurbs`); the new path
  converts the face domains first and refuses there with `status_1006` from
  `convert_uv_from_nurbs_to_step`. Both refuse.
- **Lines that ran past a face (5 cases, rows unchanged).** `issue711_upper_trim_crossing_0`,
  `_1`, `issue711_upper_trim_tangent_0`, `_1` and `parallel_cyl_a_vmax_only`: main's lines
  ran over the first cylinder's whole height (0 to 12, 0 to 11) past the shorter face's top
  (4, 10.5); the new ones end there. Their rows digest to `[-inf, inf, -inf]` before and
  after. `issue711_upper_trim_shortens_lines_and_tangent_lines` and
  `issue711_lines_end_where_either_face_ends` check these ends.

## The rows, old to new

Curves old to new (C crossing, T touching, Tg tangent), then the new lines' span along the
first cylinder's axis and the overlap of the two faces' axial ranges.


### Main missed the lines (140 rows)

- `gen0015`: 0 [] to 2 [C, C]; lines 3.182573e-1..2.227801e0; overlap 3.182573e-1..2.227801e0
- `gen0019`: 0 [] to 2 [C, C]; lines 3.599498e5..2.519649e6; overlap 3.599498e5..2.519649e6
- `gen0020`: 0 [] to 2 [C, C]; lines 5.496506e5..3.847554e6; overlap 5.496506e5..3.847554e6
- `gen0023`: 0 [] to 2 [C, C]; lines 1.296698e-1..9.076883e-1; overlap 1.296698e-1..9.076883e-1
- `gen0025`: 0 [] to 2 [C, C]; lines 4.544201e-1..3.180941e0; overlap 4.544201e-1..3.180941e0
- `gen0027`: 0 [] to 2 [C, C]; lines 3.593987e-8..2.515791e-7; overlap 3.593987e-8..2.515791e-7
- `gen0028`: 0 [] to 2 [C, C]; lines 1.518690e5..1.063083e6; overlap 1.518690e5..1.063083e6
- `gen0034`: 0 [] to 1 [C]; lines 4.596114e5..3.217280e6; overlap 4.596114e5..3.217280e6
- `gen0043`: 0 [] to 2 [C, C]; lines 4.636612e5..3.245629e6; overlap 4.636612e5..3.245629e6
- `gen0068`: 0 [] to 2 [C, C]; lines 3.992569e-1..2.794798e0; overlap 3.992569e-1..2.794798e0
- `gen0073`: 0 [] to 2 [C, C]; lines 5.249940e-1..3.674958e0; overlap 5.249940e-1..3.674958e0
- `gen0079`: 0 [] to 2 [C, C]; lines 2.097979e-1..1.468585e0; overlap 2.097979e-1..1.468585e0
- `gen0131`: 0 [] to 2 [C, C]; lines 6.138905e-8..4.297233e-7; overlap 6.138905e-8..4.297233e-7
- `gen0140`: 0 [] to 2 [C, C]; lines 5.010659e-8..3.507461e-7; overlap 5.010659e-8..3.507461e-7
- `gen0146`: 0 [] to 2 [C, C]; lines 3.067042e-1..2.146930e0; overlap 3.067042e-1..2.146930e0
- `gen0152`: 0 [] to 1 [C]; lines 4.929265e-8..3.450486e-7; overlap 4.929265e-8..3.450486e-7
- `gen0186`: 0 [] to 2 [C, C]; lines 3.341642e5..2.339150e6; overlap 3.341642e5..2.339150e6
- `gen0190`: 0 [] to 2 [C, C]; lines 2.084278e5..1.458995e6; overlap 2.084278e5..1.458995e6
- `gen0193`: 0 [] to 2 [C, C]; lines 5.160779e-8..3.612545e-7; overlap 5.160779e-8..3.612545e-7
- `gen0200`: 0 [] to 2 [C, C]; lines 3.673134e5..2.571194e6; overlap 3.673134e5..2.571194e6
- `gen0201`: 0 [] to 2 [C, C]; lines 6.108034e-1..4.275624e0; overlap 6.108034e-1..4.275624e0
- `gen0207`: 0 [] to 2 [C, C]; lines 1.898361e5..1.328853e6; overlap 1.898361e5..1.328853e6
- `gen0226`: 0 [] to 2 [C, C]; lines 3.135189e-8..2.194632e-7; overlap 3.135189e-8..2.194632e-7
- `gen0245`: 0 [] to 2 [C, C]; lines 5.663788e-1..3.964652e0; overlap 5.663788e-1..3.964652e0
- `gen0252`: 0 [] to 2 [C, C]; lines 5.555357e-1..3.888750e0; overlap 5.555357e-1..3.888750e0
- `gen0262`: 0 [] to 2 [C, C]; lines 1.536667e-8..1.075667e-7; overlap 1.536667e-8..1.075667e-7
- `gen0273`: 0 [] to 2 [C, C]; lines 2.511265e5..1.757886e6; overlap 2.511265e5..1.757886e6
- `gen0274`: 0 [] to 2 [C, C]; lines 5.490642e-8..3.843449e-7; overlap 5.490642e-8..3.843449e-7
- `gen0284`: 0 [] to 1 [C]; lines 5.756805e5..4.029763e6; overlap 5.756805e5..4.029763e6
- `gen0309`: 0 [] to 2 [C, C]; lines 5.507287e-1..3.855101e0; overlap 5.507287e-1..3.855101e0
- `gen0313`: 0 [] to 2 [C, C]; lines 2.466866e-8..1.726806e-7; overlap 2.466866e-8..1.726806e-7
- `gen0318`: 0 [] to 2 [C, C]; lines 2.100953e-8..1.470667e-7; overlap 2.100953e-8..1.470667e-7
- `gen0319`: 0 [] to 2 [C, C]; lines 4.081490e-1..2.857043e0; overlap 4.081490e-1..2.857043e0
- `gen0331`: 0 [] to 2 [C, C]; lines 4.152405e5..2.906684e6; overlap 4.152405e5..2.906684e6
- `gen0348`: 0 [] to 2 [C, C]; lines 5.969010e-1..4.178307e0; overlap 5.969010e-1..4.178307e0
- `gen0354`: 0 [] to 2 [C, C]; lines 3.171153e5..2.219807e6; overlap 3.171153e5..2.219807e6
- `gen0356`: 0 [] to 2 [C, C]; lines 2.702618e-1..1.891833e0; overlap 2.702618e-1..1.891833e0
- `gen0363`: 0 [] to 2 [C, C]; lines 4.420061e-1..3.094043e0; overlap 4.420061e-1..3.094043e0
- `gen0364`: 0 [] to 2 [C, C]; lines 2.649141e5..1.854399e6; overlap 2.649141e5..1.854399e6
- `gen0390`: 0 [] to 2 [C, C]; lines 1.308555e-1..9.159883e-1; overlap 1.308555e-1..9.159883e-1
- `gen0413`: 0 [] to 2 [C, C]; lines 4.797655e5..3.358359e6; overlap 4.797655e5..3.358359e6
- `gen0415`: 0 [] to 2 [C, C]; lines 2.239471e5..1.567630e6; overlap 2.239471e5..1.567630e6
- `gen0418`: 0 [] to 2 [C, C]; lines 4.788200e-8..3.351740e-7; overlap 4.788200e-8..3.351740e-7
- `gen0461`: 0 [] to 2 [C, C]; lines 5.358997e-8..3.751298e-7; overlap 5.358997e-8..3.751298e-7
- `gen0468`: 0 [] to 2 [C, C]; lines 2.329144e-1..1.630401e0; overlap 2.329144e-1..1.630401e0
- `gen0488`: 0 [] to 2 [C, C]; lines 1.939113e-1..1.357379e0; overlap 1.939113e-1..1.357379e0
- `gen0505`: 0 [] to 2 [C, C]; lines 1.665912e-8..1.166139e-7; overlap 1.665912e-8..1.166139e-7
- `gen0527`: 0 [] to 2 [C, C]; lines 4.511508e-8..3.158056e-7; overlap 4.511508e-8..3.158056e-7
- `gen0543`: 0 [] to 2 [C, C]; lines 3.191933e5..2.234353e6; overlap 3.191933e5..2.234353e6
- `gen0546`: 0 [] to 1 [C]; lines 3.654562e5..2.558193e6; overlap 3.654562e5..2.558193e6
- `gen0585`: 0 [] to 1 [C]; lines 2.319943e-1..1.623960e0; overlap 2.319943e-1..1.623960e0
- `gen0593`: 0 [] to 2 [C, C]; lines 4.726422e-8..3.308495e-7; overlap 4.726422e-8..3.308495e-7
- `gen0601`: 0 [] to 2 [C, C]; lines 5.993562e-8..4.195493e-7; overlap 5.993562e-8..4.195493e-7
- `gen0638`: 0 [] to 2 [C, C]; lines 5.512721e5..3.858905e6; overlap 5.512721e5..3.858905e6
- `gen0639`: 0 [] to 2 [C, C]; lines 4.983692e-8..3.488584e-7; overlap 4.983692e-8..3.488584e-7
- `gen0651`: 0 [] to 2 [C, C]; lines 5.294729e5..3.706311e6; overlap 5.294729e5..3.706311e6
- `gen0719`: 0 [] to 2 [C, C]; lines 2.569253e-1..1.798477e0; overlap 2.569253e-1..1.798477e0
- `gen0728`: 0 [] to 2 [C, C]; lines 4.691283e-1..3.283898e0; overlap 4.691283e-1..3.283898e0
- `gen0751`: 0 [] to 2 [C, C]; lines 4.286086e-8..3.000260e-7; overlap 4.286086e-8..3.000260e-7
- `gen0764`: 0 [] to 2 [C, C]; lines 2.373064e5..1.661145e6; overlap 2.373064e5..1.661145e6
- `gen0792`: 0 [] to 2 [C, C]; lines 4.433017e-1..3.103112e0; overlap 4.433017e-1..3.103112e0
- `gen0797`: 0 [] to 2 [C, C]; lines 1.514262e-1..1.059983e0; overlap 1.514262e-1..1.059983e0
- `gen0812`: 0 [] to 1 [C]; lines 1.682713e-1..1.177899e0; overlap 1.682713e-1..1.177899e0
- `gen0816`: 0 [] to 2 [C, C]; lines 3.481611e5..2.437127e6; overlap 3.481611e5..2.437127e6
- `gen0824`: 0 [] to 2 [C, C]; lines 4.785684e-1..3.349978e0; overlap 4.785684e-1..3.349978e0
- `gen0838`: 0 [] to 2 [C, C]; lines 2.969099e-1..2.078369e0; overlap 2.969099e-1..2.078369e0
- `gen0839`: 0 [] to 2 [C, C]; lines 4.109076e-1..2.876353e0; overlap 4.109076e-1..2.876353e0
- `gen0851`: 0 [] to 1 [C]; lines 2.624300e-1..1.837010e0; overlap 2.624300e-1..1.837010e0
- `gen0880`: 0 [] to 2 [C, C]; lines 5.905500e-8..4.133850e-7; overlap 5.905500e-8..4.133850e-7
- `gen0932`: 0 [] to 2 [C, C]; lines 3.096451e-8..2.167515e-7; overlap 3.096451e-8..2.167515e-7
- `gen0938`: 0 [] to 2 [C, C]; lines 4.970888e5..3.479622e6; overlap 4.970888e5..3.479622e6
- `gen0939`: 0 [] to 2 [C, C]; lines 6.084928e-8..4.259450e-7; overlap 6.084928e-8..4.259450e-7
- `gen0940`: 0 [] to 2 [C, C]; lines 2.762642e5..1.933850e6; overlap 2.762642e5..1.933850e6
- `gen0951`: 0 [] to 2 [C, C]; lines 5.524247e5..3.866973e6; overlap 5.524247e5..3.866973e6
- `gen0955`: 0 [] to 2 [C, C]; lines 2.760604e-1..1.932423e0; overlap 2.760604e-1..1.932423e0
- `gen0991`: 0 [] to 2 [C, C]; lines 2.292708e-1..1.604896e0; overlap 2.292708e-1..1.604896e0
- `gen0999`: 0 [] to 2 [C, C]; lines 2.835286e-1..1.984700e0; overlap 2.835286e-1..1.984700e0
- `gen1001`: 0 [] to 2 [C, C]; lines 3.151752e5..2.206227e6; overlap 3.151752e5..2.206227e6
- `gen1009`: 0 [] to 2 [C, C]; lines 2.139897e-8..1.497928e-7; overlap 2.139897e-8..1.497928e-7
- `gen1029`: 0 [] to 2 [C, C]; lines 4.211442e5..2.948010e6; overlap 4.211442e5..2.948010e6
- `gen1038`: 0 [] to 2 [C, C]; lines 2.184325e-8..1.529027e-7; overlap 2.184325e-8..1.529027e-7
- `gen1048`: 0 [] to 2 [C, C]; lines 4.845414e-1..3.391790e0; overlap 4.845414e-1..3.391790e0
- `gen1051`: 0 [] to 2 [C, C]; lines 5.956476e-8..4.169534e-7; overlap 5.956476e-8..4.169534e-7
- `gen1055`: 0 [] to 2 [C, C]; lines 4.508630e-1..3.156041e0; overlap 4.508630e-1..3.156041e0
- `gen1105`: 0 [] to 1 [C]; lines 5.257274e5..3.680092e6; overlap 5.257274e5..3.680092e6
- `gen1114`: 0 [] to 2 [C, C]; lines 4.732951e-1..3.313065e0; overlap 4.732951e-1..3.313065e0
- `gen1127`: 0 [] to 2 [C, C]; lines 2.042579e-1..1.429805e0; overlap 2.042579e-1..1.429805e0
- `gen1132`: 0 [] to 2 [C, C]; lines 4.664443e5..3.265110e6; overlap 4.664443e5..3.265110e6
- `gen1148`: 0 [] to 2 [C, C]; lines 3.493887e-8..2.445721e-7; overlap 3.493887e-8..2.445721e-7
- `gen1170`: 0 [] to 2 [C, C]; lines 5.688214e5..3.981750e6; overlap 5.688214e5..3.981750e6
- `gen1180`: 0 [] to 2 [C, C]; lines 1.844086e5..1.290860e6; overlap 1.844086e5..1.290860e6
- `gen1181`: 0 [] to 2 [C, C]; lines 5.181819e5..3.627273e6; overlap 5.181819e5..3.627273e6
- `gen1190`: 0 [] to 2 [C, C]; lines 1.738812e-1..1.217169e0; overlap 1.738812e-1..1.217169e0
- `gen1198`: 0 [] to 2 [C, C]; lines 1.328718e-8..9.301026e-8; overlap 1.328718e-8..9.301026e-8
- `issue711_both_trims_class0_flags0`: 0 [] to 2 [C, C]; lines 3.000000e0..9.000000e0; overlap 3.000000e0..9.000000e0
- `issue711_both_trims_class0_flags1`: 0 [] to 2 [C, C]; lines 3.000000e0..9.000000e0; overlap 3.000000e0..9.000000e0
- `issue711_both_trims_class0_flags10`: 0 [] to 2 [C, C]; lines 3.000000e0..9.000000e0; overlap 3.000000e0..9.000000e0
- `issue711_both_trims_class0_flags11`: 0 [] to 2 [C, C]; lines 3.000000e0..9.000000e0; overlap 3.000000e0..9.000000e0
- `issue711_both_trims_class0_flags12`: 0 [] to 2 [C, C]; lines 3.000000e0..9.000000e0; overlap 3.000000e0..9.000000e0
- `issue711_both_trims_class0_flags13`: 0 [] to 2 [C, C]; lines 3.000000e0..9.000000e0; overlap 3.000000e0..9.000000e0
- `issue711_both_trims_class0_flags14`: 0 [] to 2 [C, C]; lines 3.000000e0..9.000000e0; overlap 3.000000e0..9.000000e0
- `issue711_both_trims_class0_flags15`: 0 [] to 2 [C, C]; lines 3.000000e0..9.000000e0; overlap 3.000000e0..9.000000e0
- `issue711_both_trims_class0_flags2`: 0 [] to 2 [C, C]; lines 3.000000e0..9.000000e0; overlap 3.000000e0..9.000000e0
- `issue711_both_trims_class0_flags3`: 0 [] to 2 [C, C]; lines 3.000000e0..9.000000e0; overlap 3.000000e0..9.000000e0
- `issue711_both_trims_class0_flags4`: 0 [] to 2 [C, C]; lines 3.000000e0..9.000000e0; overlap 3.000000e0..9.000000e0
- `issue711_both_trims_class0_flags5`: 0 [] to 2 [C, C]; lines 3.000000e0..9.000000e0; overlap 3.000000e0..9.000000e0
- `issue711_both_trims_class0_flags6`: 0 [] to 2 [C, C]; lines 3.000000e0..9.000000e0; overlap 3.000000e0..9.000000e0
- `issue711_both_trims_class0_flags7`: 0 [] to 2 [C, C]; lines 3.000000e0..9.000000e0; overlap 3.000000e0..9.000000e0
- `issue711_both_trims_class0_flags8`: 0 [] to 2 [C, C]; lines 3.000000e0..9.000000e0; overlap 3.000000e0..9.000000e0
- `issue711_both_trims_class0_flags9`: 0 [] to 2 [C, C]; lines 3.000000e0..9.000000e0; overlap 3.000000e0..9.000000e0
- `issue711_both_trims_class1_flags0`: 0 [] to 2 [C, C]; lines 3.000000e0..9.000000e0; overlap 3.000000e0..9.000000e0
- `issue711_both_trims_class1_flags1`: 0 [] to 2 [C, C]; lines 3.000000e0..9.000000e0; overlap 3.000000e0..9.000000e0
- `issue711_both_trims_class1_flags10`: 0 [] to 2 [C, C]; lines 3.000000e0..9.000000e0; overlap 3.000000e0..9.000000e0
- `issue711_both_trims_class1_flags11`: 0 [] to 2 [C, C]; lines 3.000000e0..9.000000e0; overlap 3.000000e0..9.000000e0
- `issue711_both_trims_class1_flags12`: 0 [] to 2 [C, C]; lines 3.000000e0..9.000000e0; overlap 3.000000e0..9.000000e0
- `issue711_both_trims_class1_flags13`: 0 [] to 2 [C, C]; lines 3.000000e0..9.000000e0; overlap 3.000000e0..9.000000e0
- `issue711_both_trims_class1_flags14`: 0 [] to 2 [C, C]; lines 3.000000e0..9.000000e0; overlap 3.000000e0..9.000000e0
- `issue711_both_trims_class1_flags15`: 0 [] to 2 [C, C]; lines 3.000000e0..9.000000e0; overlap 3.000000e0..9.000000e0
- `issue711_both_trims_class1_flags2`: 0 [] to 2 [C, C]; lines 3.000000e0..9.000000e0; overlap 3.000000e0..9.000000e0
- `issue711_both_trims_class1_flags3`: 0 [] to 2 [C, C]; lines 3.000000e0..9.000000e0; overlap 3.000000e0..9.000000e0
- `issue711_both_trims_class1_flags4`: 0 [] to 2 [C, C]; lines 3.000000e0..9.000000e0; overlap 3.000000e0..9.000000e0
- `issue711_both_trims_class1_flags5`: 0 [] to 2 [C, C]; lines 3.000000e0..9.000000e0; overlap 3.000000e0..9.000000e0
- `issue711_both_trims_class1_flags6`: 0 [] to 2 [C, C]; lines 3.000000e0..9.000000e0; overlap 3.000000e0..9.000000e0
- `issue711_both_trims_class1_flags7`: 0 [] to 2 [C, C]; lines 3.000000e0..9.000000e0; overlap 3.000000e0..9.000000e0
- `issue711_both_trims_class1_flags8`: 0 [] to 2 [C, C]; lines 3.000000e0..9.000000e0; overlap 3.000000e0..9.000000e0
- `issue711_both_trims_class1_flags9`: 0 [] to 2 [C, C]; lines 3.000000e0..9.000000e0; overlap 3.000000e0..9.000000e0
- `issue711_global_equal_radius_cones`: 0 [] to 2 [C, C]; lines 3.000000e0..9.000000e0; overlap 3.000000e0..9.000000e0
- `issue711_opposite_axis_flags0`: 0 [] to 2 [C, C]; lines 3.000000e0..9.000000e0; overlap 3.000000e0..9.000000e0
- `issue711_opposite_axis_flags1`: 0 [] to 2 [C, C]; lines 3.000000e0..9.000000e0; overlap 3.000000e0..9.000000e0
- `issue711_opposite_axis_flags2`: 0 [] to 2 [C, C]; lines 3.000000e0..9.000000e0; overlap 3.000000e0..9.000000e0
- `issue711_opposite_axis_flags3`: 0 [] to 2 [C, C]; lines 3.000000e0..9.000000e0; overlap 3.000000e0..9.000000e0
- `issue711_overlapping_holes`: 0 [] to 2 [C, C]; lines 1.000000e0..1.100000e1; overlap 1.000000e0..1.100000e1
- `issue711_overlapping_holes_reversed`: 0 [] to 2 [C, C]; lines 1.000000e0..1.100000e1; overlap 1.000000e0..1.100000e1
- `issue711_scale_large`: 0 [] to 2 [C, C]; lines 3.000000e6..9.000000e6; overlap 3.000000e6..9.000000e6
- `issue711_scale_small`: 0 [] to 2 [C, C]; lines 3.000000e-5..9.000000e-5; overlap 3.000000e-5..9.000000e-5
- `issue711_touch_or_disjoint_4`: 0 [] to 2 [T, T]; lines 4.000000e0..4.000000e0; overlap 4.000000e0..4.000000e0
- `parallel_cyl_a_vmin_only`: 0 [] to 2 [C, C]; lines 8.000000e-1..1.100000e1; overlap 8.000000e-1..1.100000e1
- `parallel_cyl_a_vtrim`: 0 [] to 2 [C, C]; lines 8.000000e-1..1.050000e1; overlap 8.000000e-1..1.050000e1
- `parallel_cyl_b_vtrim`: 0 [] to 2 [C, C]; lines 8.000000e-1..1.050000e1; overlap 8.000000e-1..1.050000e1
- `parallel_cyl_both_vtrim`: 0 [] to 2 [C, C]; lines 8.000000e-1..1.050000e1; overlap 8.000000e-1..1.050000e1

### Degenerate cone, lines gone (3 rows)

- `fuzz_axis_overflow_zero_y`: 2 [C, C] to 0 []; overlap 1.600000e0..4.000000e0
- `fuzz_axis_overflow_zero_y_swapped`: 2 [C, C] to 0 []; overlap 1.600000e0..4.000000e0
- `fuzz_axis_overflow_zero_y_trimmed`: 1 [C] to 0 []; overlap 1.600000e0..4.000000e0

### Refusal, other status (8 rows)

- `rejected_below_unitize_threshold_axis`: `status_1007` to `status_1006`
- `rejected_tiny_axis_0_cone`: `status_1007` to `status_1006`
- `rejected_tiny_axis_0_other`: `status_1007` to `status_1006`
- `rejected_tiny_axis_3_cone`: `status_1007` to `status_1006`
- `rejected_tiny_axis_3_other`: `status_1007` to `status_1006`
- `rejected_tiny_axis_cylinders_cone`: `status_1007` to `status_1006`
- `rejected_tiny_axis_cylinders_other`: `status_1007` to `status_1006`
- `rejected_zero_axis`: `status_1007` to `status_1006`
