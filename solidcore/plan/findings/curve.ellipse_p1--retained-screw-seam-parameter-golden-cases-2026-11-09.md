# curve.ellipse_p1: the 95 golden cases the retained screw sweeps' seam-parameter change moves (2026-11-09)

- **Unit:** `curve.ellipse_p1` (`PolarConversion`), with `surface.cone_p1` (`Cone::evaluate_point`); PR #13891.
- **Change:** #13891's thread commit keeps the angle of a NURBS parameter near a closed curve's
  seam instead of setting it to the seam (`PolarConversion`, `Ellipse::parameter_angle`), maps the
  exact NURBS ends to the analytic domain's own ends (0° and 360°, not 0° twice), and evaluates a
  cylinder through that angle (`Cone::evaluate_point`). Every case below moves for that reason.
- **How measured:** on one machine (macOS, Apple silicon), `goldens --goldens` of this branch
  against `main` `b73b603b5` re-recorded on the same machine, so platform rounding cancels; 97
  cases differ by the goldens' own comparison. Each was checked against an independent reference:
  a cone's NURBS form is exact, so an answer's error is its distance from that form's point at the
  same parameter, relative to the point's size; a surface's state was read field by field.
- **Recorded here:** 95 cases, each blessed with `--bless --filter <case>` and nothing else,
  after checking that `main` on this machine reproduces the Linux-recorded value of the case (so
  the new value carries no platform rounding). On this machine 80 other units differ from the
  recorded values on `main` either way; `main`'s Linux goldens job passes. Linux CI is the check of
  record.
- **Not recorded here:** `topology.assert_valid` `l2_file_Solids_extrusion_tests`, `topology.assert_valid` `l2_file_Solids_off_extrusion_tests`: `main` on this machine already
  differs from their Linux-recorded values, so their new values are taken from Linux CI.
- **Verdicts:** 42 corrections, where the old answer was wrong (full-turn domains collapsed to
  zero length with NaN knots; near-seam parameters moved onto the seam by up to 5.7e-9 relative; a
  missed second solution), and 53 equivalent answers (the same point written differently: 0°
  vs 360° at a seam, an exact 0 vs a one-ulp sin 2π residue, a circle traced the other way, gaps
  moving within rounding). None is a regression: the one angle that leaves its domain does so by
  8.3e-10°, inside the 1e-4° angular tolerance, with the same point error as before.

| Unit | Case | Change | Verdict |
|---|---|---|---|
| `boolean.topology_intersector_p1` | `gen0235` | `.values.state[15]` 1.0 → 2.0; `.values.state[16]` 0.0889886545595933 → 6.280369834735101e-16 | correction: residual 0.089 → 6.3e-16, the second solution found |
| `intersect.api_intersectors` | `ssi-sphere-cylinder-coaxial` | `.values.record[6]` 0.0 → -5.66553889764798e-16; `.values.record[10]` 0.9999851245300476 → -0.9999851245300472 | equivalent: the same intersection circle traced the other way |
| `surface.srf_in_volume` | `inline_cylinder_full_info_step` | `.values.out[21]` 0.0 → 360.0 | correction: the old update collapsed the full turn to zero length |
| `surface.surf_of_extrusion_p1` | `review3_circle_0_360_infinite_sw1_upd1` | `.values.out[21]` 0.0 → 360.0; `.values.out[84]` 'NaN' → -0.75 | correction: the old update collapsed the full turn to zero length, leaving NaN knots |
| `surface.surf_of_extrusion_p1` | `review3_circle_0_360_infinite_sw1_upd2` | `.values.out[21]` 0.0 → 360.0; `.values.out[84]` 'NaN' → -0.75 | correction: the old update collapsed the full turn to zero length, leaving NaN knots |
| `surface.surf_of_extrusion_p1` | `review3_circle_-360_0_infinite_sw1_upd1` | `.values.out[21]` -360.0 → 0.0; `.values.out[84]` 'NaN' → -0.75 | correction: the old update collapsed the full turn to zero length, leaving NaN knots |
| `surface.surf_of_extrusion_p1` | `review3_circle_-360_0_infinite_sw1_upd2` | `.values.out[21]` -360.0 → 0.0; `.values.out[84]` 'NaN' → -0.75 | correction: the old update collapsed the full turn to zero length, leaving NaN knots |
| `surface.surf_of_extrusion_p1` | `review3_circle_-180_180_infinite_sw1_upd1` | `.values.out[21]` -180.0 → 180.0; `.values.out[84]` 'NaN' → -0.75 | correction: the old update collapsed the full turn to zero length, leaving NaN knots |
| `surface.surf_of_extrusion_p1` | `review3_circle_-180_180_infinite_sw1_upd2` | `.values.out[21]` -180.0 → 180.0; `.values.out[84]` 'NaN' → -0.75 | correction: the old update collapsed the full turn to zero length, leaving NaN knots |
| `surface.surf_of_extrusion_p1` | `review3_ellipse_0_360_infinite_sw1_upd1` | `.values.out[21]` 0.0 → 360.0; `.values.out[84]` 'NaN' → -0.75 | correction: the old update collapsed the full turn to zero length, leaving NaN knots |
| `surface.surf_of_extrusion_p1` | `review3_ellipse_0_360_infinite_sw1_upd2` | `.values.out[21]` 0.0 → 360.0; `.values.out[84]` 'NaN' → -0.75 | correction: the old update collapsed the full turn to zero length, leaving NaN knots |
| `surface.surf_of_extrusion_p1` | `review3_ellipse_-360_0_infinite_sw1_upd1` | `.values.out[21]` -360.0 → 0.0; `.values.out[84]` 'NaN' → -0.75 | correction: the old update collapsed the full turn to zero length, leaving NaN knots |
| `surface.surf_of_extrusion_p1` | `review3_ellipse_-360_0_infinite_sw1_upd2` | `.values.out[21]` -360.0 → 0.0; `.values.out[84]` 'NaN' → -0.75 | correction: the old update collapsed the full turn to zero length, leaving NaN knots |
| `surface.surf_of_extrusion_p1` | `review3_ellipse_-180_180_infinite_sw1_upd1` | `.values.out[21]` -180.0 → 180.0; `.values.out[84]` 'NaN' → -0.75 | correction: the old update collapsed the full turn to zero length, leaving NaN knots |
| `surface.surf_of_extrusion_p1` | `review3_ellipse_-180_180_infinite_sw1_upd2` | `.values.out[21]` -180.0 → 180.0; `.values.out[84]` 'NaN' → -0.75 | correction: the old update collapsed the full turn to zero length, leaving NaN knots |
| `surface.surf_of_revolution_p1` | `edge_to_step_outside` | `.values.out[0]` 0.0 → 360.0 | equivalent: the seam end reported as 360° instead of 0° |
| `surface.surf_of_revolution_p3` | `r3_upd_torus_False_[-0.5, -0.5, 1.5, 1.5]` | `.values.out[17]` 0.0 → 360.0; `.values.out[18]` 0.0 → 360.0 | correction: the old update collapsed the full turn to zero length, leaving NaN knots |
| `surface.surf_of_revolution_p3` | `r3_upd_torus_True_[-0.5, -0.5, 1.5, 1.5]` | `.values.out[17]` 0.0 → 360.0; `.values.out[18]` 0.0 → 360.0 | correction: the old update collapsed the full turn to zero length, leaving NaN knots |
| `surface.surf_of_revolution_p3` | `r3_upd_sph_False_[-0.5, -0.5, 1.5, 1.5]` | `.values.out[17]` 0.0 → 360.0; `.values.out[20]` 0.0 → 360.0 | correction: the old update collapsed the full turn to zero length |
| `surface.surf_of_revolution_p3` | `r3_upd_sph_True_[-0.5, -0.5, 1.5, 1.5]` | `.values.out[17]` 0.0 → 360.0; `.values.out[20]` 0.0 → 360.0 | correction: the old update collapsed the full turn to zero length |
| `surface.surface_p2` | `common_cone_review_c123_ep[1000.0, -1000.0]` | `.values.out[1]` 0.0 → -2.4492935982947064e-16 | equivalent: within machine precision of the old value |
| `surface.surface_p2` | `common_cone_review_c123_ts[1000.0, -1000.0]` | `.values.out[0]` 0.0 → 360.0 | equivalent: the seam end reported as 360° instead of 0° |
| `surface.surface_p2` | `common_cone_review_c123_swio_ep[-0.5, 2.0]` | `.values.out[1]` 0.0 → -4.898587196589413e-16 | equivalent: within machine precision of the old value |
| `surface.surface_p2` | `common_cone_review_c123_swio_ts[-0.5, 2.0]` | `.values.out[0]` 0.0 → 360.0 | equivalent: the seam end reported as 360° instead of 0° |
| `surface.surface_p2` | `common_cone_review_unb30r1_ts[1000.0, -1000.0]` | `.values.out[0]` 0.0 → 360.0 | equivalent: the seam end reported as 360° instead of 0° |
| `surface.surface_p2` | `common_cone_review_cyl_ep[1000.0, -1000.0]` | `.values.out[1]` 0.0 → -3.6739403974420594e-16 | equivalent: within machine precision of the old value |
| `surface.surface_p2` | `common_cone_review_cyl_ts[1000.0, -1000.0]` | `.values.out[0]` 0.0 → 360.0 | equivalent: the seam end reported as 360° instead of 0° |
| `surface.surface_p2` | `common_cone_review_toapex_ep[1000.0, -1000.0]` | `.values.out[1]` 0.0 → -4.898587196589413e-16 | equivalent: within machine precision of the old value |
| `surface.surface_p2` | `common_cone_review_toapex_ts[1000.0, -1000.0]` | `.values.out[0]` 0.0 → 360.0 | equivalent: the seam end reported as 360° instead of 0° |
| `surface.surface_p2` | `common_cone_review_huge_ts[1000.0, -1000.0]` | `.values.out[0]` 0.0 → 360.0 | equivalent: the seam end reported as 360° instead of 0° |
| `surface.surface_p2` | `common_cone_review_tiny_ep[1000.0, -1000.0]` | `.values.out[1]` 0.0 → -2.4492935982947065e-22 | equivalent: within machine precision of the old value |
| `surface.surface_p2` | `common_cone_review_tiny_ts[1000.0, -1000.0]` | `.values.out[0]` 0.0 → 360.0 | equivalent: the seam end reported as 360° instead of 0° |
| `surface.surface_p2` | `common_cone_review_nonunit_ep[1000.0, -1000.0]` | `.values.out[1]` 0.0 → -2.4492935982947064e-16 | equivalent: within machine precision of the old value |
| `surface.surface_p2` | `common_cone_review_nonunit_ts[1000.0, -1000.0]` | `.values.out[0]` 0.0 → 360.0 | equivalent: the seam end reported as 360° instead of 0° |
| `surface.surface_p2` | `common_cone_review2_skew_can_semi_h_ev_-0.1_1.2_11` | `.values.out[1]` 0.0 → -2.4492935982947064e-16 | equivalent: the same point (0° vs 360° at the seam, or an exact 0 vs sin 2π residue) |
| `surface.surface_p2` | `common_cone_review2_skew_can_semi_h_oob_ev_-0.1_1.2_11` | `.values.out[1]` 0.0 → -2.2043642384652358e-16 | equivalent: the same point (0° vs 360° at the seam, or an exact 0 vs sin 2π residue) |
| `surface.surface_p2` | `common_cone_review2_skew_can_h0_ep_-0.1_1.2` | `.values.out[1]` 0.0 → -2.3836649741257277e-16 | equivalent: the same point (0° vs 360° at the seam, or an exact 0 vs sin 2π residue) |
| `surface.surface_p2` | `common_cone_review2_far_semi_unb_swin_ts_-0.1_1.2` | `.values.out[0]` 0.0 → 360.0 | equivalent: the same point (0° vs 360° at the seam, or an exact 0 vs sin 2π residue) |
| `surface.surface_p2` | `common_cone_review2_far_can_semi_h_ts_-0.1_1.2` | `.values.out[0]` 0.0 → 360.0 | equivalent: the same point (0° vs 360° at the seam, or an exact 0 vs sin 2π residue) |
| `surface.surface_p2` | `common_cone_r3_00097_ts_canon_std_in_1e-12_0.999999999999` | `.values.out[0]` 0.0 → 3.241138738169379e-10 | correction: relative error 4.5e-12 → 6.7e-17 against the exact NURBS form |
| `surface.surface_p2` | `common_cone_r3_01121_ts_canon_far_in_1e-12_0.999999999999` | `.values.out[0]` 0.0 → -8.33779267850332e-10 | equivalent: −8.3e-10° just past 0°, inside the 1e-4° angular tolerance; point error unchanged |
| `surface.surface_p2` | `common_cone_r3_01625_ep_canon_sw_std_in_-0.1_1.1` | `.values.out[1]` 0.0 → -2.4492935982947064e-16 | equivalent: the same point (0° vs 360° at the seam, or an exact 0 vs sin 2π residue) |
| `surface.surface_p2` | `common_cone_r3_02650_ts_canon_sw_far_in_-0.1_1.1` | `.values.out[0]` 0.0 → 360.0 | equivalent: the same point (0° vs 360° at the seam, or an exact 0 vs sin 2π residue) |
| `surface.surface_p2` | `common_cone_r3_02906_ts_canon_sw_far_oob_-0.1_1.1` | `.values.out[0]` 0.0 → 360.0 | equivalent: the same point (0° vs 360° at the seam, or an exact 0 vs sin 2π residue) |
| `surface.surface_p2` | `common_cone_r3_03425_ts_canon_io_std_oob_1e-12_0.999999999999` | `.values.out[0]` 0.0 → 3.241138738169379e-10 | correction: relative error 3.6e-12 → 1.7e-16 against the exact NURBS form |
| `surface.surface_p2` | `common_cone_r3_03681_ts_canon_io_rot_in_1e-12_0.999999999999` | `.values.out[0]` 0.0 → 3.241017025958138e-10 | correction: relative error 1.8e-12 → 8.3e-17 against the exact NURBS form |
| `surface.surface_p2` | `common_cone_r3_04449_ts_canon_io_far_oob_1e-12_0.999999999999` | `.values.out[0]` 0.0 → 1.667501208395266e-10 | equivalent: the same point (0° vs 360° at the seam, or an exact 0 vs sin 2π residue) |
| `surface.surface_p2` | `common_cone_r3_04954_ts_canon_swio_std_oob_-0.1_1.1` | `.values.out[0]` 0.0 → 360.0 | equivalent: the same point (0° vs 360° at the seam, or an exact 0 vs sin 2π residue) |
| `surface.surface_p2` | `common_cone_r3_04956_ev_canon_swio_std_oob_1e-12_0.999999999999_110000` | `.values.out[1]` -4.898587196588596e-16 → -1.1314106568857968e-11 | correction: relative error 3.1e-12 → 2.2e-16 against the exact NURBS form |
| `surface.surface_p2` | `common_cone_r3_07777_ts_apex_top_std_in_1e-12_0.999999999999` | `.values.out[0]` 0.0 → 3.241138738169379e-10 | correction: relative error 4.0e-12 → 2.2e-16 against the exact NURBS form |
| `surface.surface_p2` | `common_cone_r3_09307_ev_apex_bot_std_in_1e-12_0.999999999999_001101` | `.values.out[1]` 0.0 → 5.656854249504664e-12 | correction: relative error 4.0e-12 → 3.5e-16 against the exact NURBS form |
| `surface.surface_p2` | `common_cone_r3_09308_ev_apex_bot_std_in_1e-12_0.999999999999_110000` | `.values.out[1]` 0.0 → 5.656854249504664e-12 | correction: relative error 4.0e-12 → 3.5e-16 against the exact NURBS form |
| `surface.surface_p2` | `common_cone_r3_09312_ep_apex_bot_std_in_1e-12_0.999999999999` | `.values.out[1]` 0.0 → 5.656854249504664e-12 | correction: relative error 4.0e-12 → 3.5e-16 against the exact NURBS form |
| `surface.surface_p2` | `common_cone_r3_11105_ts_unb45_std_oob_1e-12_0.999999999999` | `.values.out[0]` 0.0 → 3.24113873816938e-10 | correction: relative error 5.1e-12 → 5.0e-17 against the exact NURBS form |
| `surface.surface_p2` | `common_cone_r3_11617_ts_unb45_rot_oob_1e-12_0.999999999999` | `.values.out[0]` 0.0 → 3.2410712442308e-10 | correction: relative error 2.2e-12 → 6.2e-17 against the exact NURBS form |
| `surface.surface_p2` | `common_cone_r3_12129_ts_unb45_far_oob_1e-12_0.999999999999` | `.values.out[0]` 0.0 → 6.576271025361176e-10 | equivalent: the same point (0° vs 360° at the seam, or an exact 0 vs sin 2π residue) |
| `surface.surface_p2` | `common_cone_r3_12378_ts_unb45_sw_std_in_-0.1_1.1` | `.values.out[0]` 0.0 → 360.0 | equivalent: the same point (0° vs 360° at the seam, or an exact 0 vs sin 2π residue) |
| `surface.surface_p2` | `common_cone_r3_13146_ts_unb45_sw_rot_oob_-0.1_1.1` | `.values.out[0]` 0.0 → 360.0 | equivalent: the same point (0° vs 360° at the seam, or an exact 0 vs sin 2π residue) |
| `surface.surface_p2` | `common_cone_r3_13402_ts_unb45_sw_far_in_-0.1_1.1` | `.values.out[0]` 3.3350857223506405e-10 → 360.0 | equivalent: the same point (0° vs 360° at the seam, or an exact 0 vs sin 2π residue) |
| `surface.surface_p2` | `common_cone_r3_14176_ep_unb0base_std_oob_1e-12_0.999999999999` | `.values.out[1]` 0.0 → 3.265986323711464e-12 | correction: relative error 2.8e-12 → 0.0e+00 against the exact NURBS form |
| `surface.surface_p2` | `common_cone_r3_14689_ts_unb0base_rot_oob_1e-12_0.999999999999` | `.values.out[0]` 0.0 → 3.241111193664678e-10 | correction: relative error 7.5e-13 → 7.3e-17 against the exact NURBS form |
| `surface.surface_p2` | `common_cone_r3_15456_ep_unbneg_std_in_1e-12_0.999999999999` | `.values.out[1]` 0.0 → 5.429571749113059e-14 | equivalent: the same point (0° vs 360° at the seam, or an exact 0 vs sin 2π residue) |
| `surface.surface_p2` | `common_cone_r3_15457_ts_unbneg_std_in_1e-12_0.999999999999` | `.values.out[0]` 0.0 → 5.15843251425777e-11 | equivalent: the same point (0° vs 360° at the seam, or an exact 0 vs sin 2π residue) |
| `surface.surface_p2` | `common_cone_r3_16481_ts_unbneg_far_in_1e-12_0.999999999999` | `.values.out[0]` 0.0 → -1.6677859093761072e-10 | equivalent: the same point (0° vs 360° at the seam, or an exact 0 vs sin 2π residue) |
| `surface.surface_p2` | `common_cone_r3_17248_ep_unbtiny_std_oob_1e-12_0.999999999999` | `.values.out[1]` 0.0 → 9.003163161572899e-13 | equivalent: the same point (0° vs 360° at the seam, or an exact 0 vs sin 2π residue) |
| `surface.surface_p2` | `common_cone_r3_18784_ep_unbsteep_std_oob_1e-12_0.999999999999` | `.values.out[1]` 0.0 → 3.2411443948706e-06 | correction: relative error 5.7e-12 → 4.1e-16 against the exact NURBS form |
| `surface.surface_p2` | `common_cone_r3_19546_ts_unbsteep_far_in_-0.1_1.1` | `.values.out[0]` 1.0986336145517584e-14 → 0.0 | equivalent: the same point (0° vs 360° at the seam, or an exact 0 vs sin 2π residue) |
| `surface.surface_p2` | `common_cone_r3_20058_ts_semi_h_std_in_-0.1_1.1` | `.values.out[0]` 0.0 → 360.0 | equivalent: the same point (0° vs 360° at the seam, or an exact 0 vs sin 2π residue) |
| `surface.surface_p2` | `common_cone_r3_21338_ts_semi_h_far_oob_-0.1_1.1` | `.values.out[0]` 1.111625064873689e-10 → 360.0 | equivalent: the same point (0° vs 360° at the seam, or an exact 0 vs sin 2π residue) |
| `surface.surface_p2` | `common_cone_r3_21601_ts_semi_inf_std_in_1e-12_0.999999999999` | `.values.out[0]` 0.0 → 3.2411387381693793e-10 | correction: relative error 5.2e-12 → 4.5e-17 against the exact NURBS form |
| `surface.surface_p2` | `common_cone_r3_22113_ts_semi_inf_rot_in_1e-12_0.999999999999` | `.values.out[0]` 0.0 → 3.2409891296280653e-10 | correction: relative error 2.4e-12 → 8.8e-17 against the exact NURBS form |
| `surface.surface_p2` | `common_cone_r3_23136_ep_cyl_std_in_1e-12_0.999999999999` | `.values.out[1]` 0.0 → 9.0031631615719e-13 | equivalent: the same point (0° vs 360° at the seam, or an exact 0 vs sin 2π residue) |
| `surface.surface_p2` | `common_cone_r3_23137_ts_cyl_std_in_1e-12_0.999999999999` | `.values.out[0]` 0.0 → 2.5792162571286438e-11 | equivalent: the same point (0° vs 360° at the seam, or an exact 0 vs sin 2π residue) |
| `surface.surface_p2` | `common_cone_r3_32352_ep_partial_mng_std_in_1e-12_0.999999999999` | `.values.out[1]` -2.449293598295931e-16 → -5.657053284432756e-12 | correction: relative error 5.7e-12 → 3.2e-16 against the exact NURBS form |
| `surface.surface_p2` | `common_cone_r3_32601_ep_partial_mng_std_oob_-0.1_1.1` | `.values.out[1]` 0.0 → -2.326828918379971e-16 | equivalent: the same point (0° vs 360° at the seam, or an exact 0 vs sin 2π residue) |
| `surface.surface_p2` | `common_cone_r3_32608_ep_partial_mng_std_oob_1e-12_0.999999999999` | `.values.out[1]` -2.449293598295931e-16 → -5.657053284432756e-12 | correction: relative error 5.7e-12 → 3.2e-16 against the exact NURBS form |
| `surface.surface_p2` | `common_cone_r3_33114_ts_partial_mng_rot_oob_-0.1_1.1` | `.values.out[0]` 0.0 → 360.0 | equivalent: the same point (0° vs 360° at the seam, or an exact 0 vs sin 2π residue) |
| `surface.surface_p2` | `common_cone_r3_33889_ts_big_std_in_1e-12_0.999999999999` | `.values.out[0]` 0.0 → 3.24113873816938e-10 | correction: relative error 5.7e-12 → 3.3e-20 against the exact NURBS form |
| `surface.surface_p2` | `common_cone_r3_34145_ts_big_std_oob_1e-12_0.999999999999` | `.values.out[0]` 0.0 → 3.24113873816938e-10 | correction: relative error 5.7e-12 → 3.3e-20 against the exact NURBS form |
| `surface.surface_p2` | `common_cone_r3_35169_ts_big_far_oob_1e-12_0.999999999999` | `.values.out[0]` 0.0 → 3.240838049785098e-10 | correction: relative error 3.9e-13 → 2.5e-17 against the exact NURBS form |
| `surface.surface_p2` | `common_cone_r3_35424_ep_small_std_in_1e-12_0.999999999999` | `.values.out[1]` 0.0 → 1.1313708498998022e-15 | correction: relative error 3.1e-12 → 1.7e-16 against the exact NURBS form |
| `surface.surface_p2` | `common_cone_r3_36705_ts_small_far_oob_1e-12_0.999999999999` | `.values.out[0]` 0.0 → -1.5007740046257823e-05 | equivalent: the same point (0° vs 360° at the seam, or an exact 0 vs sin 2π residue) |
| `surface.surface_p2` | `common_cone_r4_11375_tos_semi5.7e-11_rot_0_-0.5_2` | `.values.out[0]` 3.180554681463516e-15 → 0.0 | equivalent: the same point (0° vs 360° at the seam, or an exact 0 vs sin 2π residue) |
| `surface.surface_p2` | `common_cone_r4_14419_tos_semisw01_6e-11_rot_0_-0.5_2` | `.values.out[0]` 7.067873546061804e-09 → 0.0 | equivalent: the same point (0° vs 360° at the seam, or an exact 0 vs sin 2π residue) |
| `surface.surface_p2` | `common_cone_r4_31251_tos_sweep-360_0_std_0_1_1e-10` | `.values.out[0]` 0.0 → -3.2411384154329426e-07 | correction: relative error 5.7e-09 → 4.1e-16 against the exact NURBS form |
| `surface.surface_p2` | `common_cone_r4_31352_tos_sweep-360_0_std_1_1_1e-10` | `.values.out[0]` 0.0 → -3.2411384154329426e-07 | correction: relative error 5.7e-09 → 4.1e-16 against the exact NURBS form |
| `surface.surface_p2` | `rev_step_inv_torus_p0_s0_i1_nonplanar_guess1` | `.values.out[1]` 0.0 → 7.916451076855127e-13 | equivalent: within machine precision of the old value |
| `surface.surface_p2` | `rev_step_inv_torus_p0_s1_i1_nonplanar_guess1` | `.values.out[1]` 0.0 → 7.916451076855127e-13 | equivalent: within machine precision of the old value |
| `surface.surface_p2` | `rev_step_inv_torus_p1_s0_i1_nonplanar_guess1` | `.values.out[1]` 0.0 → 5.66138733300506e-13 | equivalent: within machine precision of the old value |
| `surface.surface_p2` | `rev_step_inv_torus_p1_s1_i1_nonplanar_guess1` | `.values.out[1]` 0.0 → 5.66138733300506e-13 | equivalent: within machine precision of the old value |
| `surface.torus` | `gen0484` | `.values.out[1]` 0.0 → 360.0 | equivalent: the seam end reported as 360° instead of 0° |
| `topology.assert_valid` | `l2_file_Solids_OffsetBoxCylOnCorner_tests` | `.ints.vertex_test1_max_gap_face[2]` 0 → 7; `.values.vertex_test1_max_gap_3d[2]` 1.464821375527116e-14 → 4.322062729257126e-14 | equivalent: largest vertex gaps move within 1e-13 (rounding) |
| `topology.assert_valid` | `l2_file_Solids_OffsetBoxWithBoss_tests` | `.values.vertex_test1_max_gap_3d[9]` 1.6280579355608324e-14 → 3.197442310920451e-14 | equivalent: largest vertex gaps move within 1e-13 (rounding) |
| `topology.assert_valid` | `l2_file_box_with_large_fillet_tests` | `.values.vertex_test1_max_gap_3d[8]` 7.093829940178991e-14 → 1.4323167916492475e-14; `.values.vertex_test1_max_gap_3d[9]` 8.10921975650084e-14 → 3.9120201381495503e-14 | equivalent: largest vertex gaps move within 1e-13 (rounding) |
| `topology.face_p2` | `extra00861` | `.values.state[128]` 6.456448613119753e-14 → 0.0; `.values.state[177]` 6.456448613119753e-14 → 0.0 | correction: gaps 6.5e-14 → 0 |
