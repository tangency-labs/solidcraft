# blend.feature_executive: the 73 rows #3179 moves, measured before and after (2026-11-08)

Unit: `blend.feature_executive` (73 rows rewritten by #3179, WR-656's open base retired).
The goldens hold each row's status and hashed graph and geometry blocks, not a volume or a
manifold flag, so the bodies were measured with the unit's soundness probe
(`crates/goldens/src/units/blend_feature_executive.rs`, `soundness_probe`), which runs the
Rust side of the named cases and writes, per case, the call's status, `check_brep` level 2
(lines, first line), manifoldness, volume, counts and tolerances of the base body.

Command (the same case list both ways):

```
SC_SOUNDNESS_CASES=single_face_defillet_0,single_face_defillet_1,single_face_defillet_2,single_face_defillet_3,single_face_defillet_4,single_face_defillet_5,single_face_defillet_6,single_face_defillet_7,single_face_inset_cube_1x1fillet_0,single_face_inset_cube_1x1fillet_1,single_face_inset_cube_1x1fillet_2,single_face_inset_cube_1x1fillet_3,single_face_inset_cube_1x1fillet_5,single_face_inset_cube_1x1fillet_6,single_face_inset_cube_1x1fillet_7,single_face_inset_cube_1x1fillet_8,single_face_inset_cube_1x1fillet_9,single_face_flathead_screw_top_0,single_face_flathead_screw_top_1,single_face_flathead_screw_top_2,single_face_flathead_screw_top_3,single_face_flathead_screw_top_4,single_face_flathead_screw_top_5,single_face_flathead_screw_top_6,single_face_box_with_large_fillet_0,single_face_box_with_large_fillet_1,single_face_box_with_large_fillet_2,single_face_box_with_large_fillet_3,single_face_box_with_large_fillet_4,single_face_box_with_large_fillet_5,single_face_filleted_box_0,single_face_filleted_box_1,single_face_filleted_box_2,single_face_filleted_box_3,single_face_filleted_box_4,single_face_filleted_box_5,single_face_cyl_on_box_w_fillet_0,single_face_cyl_on_box_w_fillet_1,single_face_cyl_on_box_w_fillet_2,single_face_cyl_on_box_w_fillet_3,single_face_cyl_on_box_w_fillet_4,single_face_cyl_on_box_w_fillet_5,single_face_cyl_on_box_w_fillet_6,single_face_cyl_on_box_w_fillet_7,single_face_fillet1019_0,single_face_fillet1019_1,single_face_fillet1019_2,single_face_fillet1019_3,single_face_fillet1019_4,single_face_fillet1019_5,single_face_fillet1019_7,single_face_fillet1019_8,rb_33_defillet_8,rb_11_box_with_large_fillet_6,rb_13_box_with_large_fillet_6,rb_33_box_with_large_fillet_6,rb_11_filleted_box_9,rb_13_filleted_box_9,rb_33_filleted_box_9,ext_dist_0.0_box_with_large_fillet,fillet_group_torii_fillet_b_rb11,fillet_each_torii_fillet_b,fillet_group_fillet1019_rb11,fillet_each_fillet1019,fillet_each_cyl_on_box_w_fillet,fillet_group_inset_cube_1x3fillet_rb11,fillet_each_inset_cube_1x3fillet,fillet_group_inset_cube_3x1fillet_rb11,fillet_each_inset_cube_3x1fillet,single_loop_circle_defillet_2,single_loop_outer_defillet_2,multi_rebuild_1_extrusion,multi_rebuild_2_extrusion SC_SOUNDNESS=<out>.txt cargo test --release -p goldens --lib units::blend_feature_executive::soundness_probe::moved_cases_results_measured -- --ignored --exact
```

- **Before**: the open-base refusal off, the #3176 mirror rule in (the base as upstream left it).
- **After**: #3179 (a feature that does not rebuild is put back; none off is refused with the
  base restored from a snapshot).

Before, 72 of the 73 bases are not manifold; the 73rd, `multi_rebuild_2_extrusion`, is closed
by its script's own `RestoreFeatures`. After, all 73 are manifold: 70 refusals whose base is
the input body, restored from its snapshot (its volume is the input's: `box_with_large_fillet`
862.6548 = 1000 - 10 * 8^2 (1 - pi/4), `filleted_box` 991.4159 = 1000 - 10 * 2^2 (1 - pi/4)),
and 3 partial successes. 61 are level-2 clean. The other 12 are refusals of the two inputs that
are not level-2 clean themselves (inferred from the refusal leaving the input untouched, not
measured on the input separately): the 10 `fillet1019` rows (an edge/vertex gap of 1.48e-4
against 1.41e-4) and the 2 `torii_fillet_b` rows (an edge/surface gap of 6.1e-5 against 5.7e-5).

## Before

```
single_face_defillet_0	status=Some(1.0)	sound=true check_lines=0 first=None manifold=Some(false) volume=None faces=8 edges=16 vertices=11
single_face_defillet_1	status=Some(1.0)	sound=true check_lines=0 first=None manifold=Some(false) volume=None faces=8 edges=17 vertices=11
single_face_defillet_2	status=Some(1.0)	sound=true check_lines=0 first=None manifold=Some(false) volume=None faces=8 edges=17 vertices=11
single_face_defillet_3	status=Some(1.0)	sound=true check_lines=0 first=None manifold=Some(false) volume=None faces=8 edges=17 vertices=11
single_face_defillet_4	status=Some(1.0)	sound=true check_lines=0 first=None manifold=Some(false) volume=None faces=8 edges=17 vertices=11
single_face_defillet_5	status=Some(1.0)	sound=true check_lines=0 first=None manifold=Some(false) volume=None faces=8 edges=17 vertices=11
single_face_defillet_6	status=Some(1.0)	sound=true check_lines=0 first=None manifold=Some(false) volume=None faces=8 edges=17 vertices=11
single_face_defillet_7	status=Some(1.0)	sound=true check_lines=0 first=None manifold=Some(false) volume=None faces=8 edges=17 vertices=11
single_face_inset_cube_1x1fillet_0	status=Some(1.0)	sound=true check_lines=0 first=None manifold=Some(false) volume=None faces=9 edges=24 vertices=16
single_face_inset_cube_1x1fillet_1	status=Some(1.0)	sound=true check_lines=0 first=None manifold=Some(false) volume=None faces=9 edges=24 vertices=16
single_face_inset_cube_1x1fillet_2	status=Some(1.0)	sound=true check_lines=0 first=None manifold=Some(false) volume=None faces=9 edges=24 vertices=16
single_face_inset_cube_1x1fillet_3	status=Some(1.0)	sound=true check_lines=0 first=None manifold=Some(false) volume=None faces=9 edges=24 vertices=16
single_face_inset_cube_1x1fillet_5	status=Some(1.0)	sound=true check_lines=0 first=None manifold=Some(false) volume=None faces=9 edges=24 vertices=16
single_face_inset_cube_1x1fillet_6	status=Some(1.0)	sound=true check_lines=0 first=None manifold=Some(false) volume=None faces=9 edges=24 vertices=16
single_face_inset_cube_1x1fillet_7	status=Some(1.0)	sound=true check_lines=0 first=None manifold=Some(false) volume=None faces=9 edges=24 vertices=16
single_face_inset_cube_1x1fillet_8	status=Some(1.0)	sound=true check_lines=0 first=None manifold=Some(false) volume=None faces=9 edges=24 vertices=16
single_face_inset_cube_1x1fillet_9	status=Some(1.0)	sound=true check_lines=0 first=None manifold=Some(false) volume=None faces=9 edges=24 vertices=16
single_face_flathead_screw_top_0	status=Some(1.0)	sound=true check_lines=0 first=None manifold=Some(false) volume=None faces=6 edges=15 vertices=10
single_face_flathead_screw_top_1	status=Some(1.0)	sound=true check_lines=0 first=None manifold=Some(false) volume=None faces=6 edges=15 vertices=10
single_face_flathead_screw_top_2	status=Some(1.0)	sound=true check_lines=0 first=None manifold=Some(false) volume=None faces=6 edges=15 vertices=10
single_face_flathead_screw_top_3	status=Some(1.0)	sound=true check_lines=0 first=None manifold=Some(false) volume=None faces=6 edges=14 vertices=10
single_face_flathead_screw_top_4	status=Some(1.0)	sound=true check_lines=0 first=None manifold=Some(false) volume=None faces=6 edges=15 vertices=10
single_face_flathead_screw_top_5	status=Some(1.0)	sound=true check_lines=0 first=None manifold=Some(false) volume=None faces=6 edges=15 vertices=10
single_face_flathead_screw_top_6	status=Some(1.0)	sound=true check_lines=0 first=None manifold=Some(false) volume=None faces=6 edges=15 vertices=10
single_face_box_with_large_fillet_0	status=Some(1.0)	sound=true check_lines=0 first=None manifold=Some(false) volume=None faces=6 edges=15 vertices=10
single_face_box_with_large_fillet_1	status=Some(1.0)	sound=true check_lines=0 first=None manifold=Some(false) volume=None faces=6 edges=15 vertices=10
single_face_box_with_large_fillet_2	status=Some(1.0)	sound=true check_lines=0 first=None manifold=Some(false) volume=None faces=6 edges=15 vertices=10
single_face_box_with_large_fillet_3	status=Some(1.0)	sound=true check_lines=0 first=None manifold=Some(false) volume=None faces=6 edges=15 vertices=10
single_face_box_with_large_fillet_4	status=Some(1.0)	sound=true check_lines=0 first=None manifold=Some(false) volume=None faces=6 edges=15 vertices=10
single_face_box_with_large_fillet_5	status=Some(1.0)	sound=true check_lines=0 first=None manifold=Some(false) volume=None faces=6 edges=15 vertices=10
single_face_filleted_box_0	status=Some(1.0)	sound=true check_lines=0 first=None manifold=Some(false) volume=None faces=9 edges=24 vertices=16
single_face_filleted_box_1	status=Some(1.0)	sound=true check_lines=0 first=None manifold=Some(false) volume=None faces=9 edges=24 vertices=16
single_face_filleted_box_2	status=Some(1.0)	sound=true check_lines=0 first=None manifold=Some(false) volume=None faces=9 edges=24 vertices=16
single_face_filleted_box_3	status=Some(1.0)	sound=true check_lines=0 first=None manifold=Some(false) volume=None faces=9 edges=24 vertices=16
single_face_filleted_box_4	status=Some(1.0)	sound=true check_lines=0 first=None manifold=Some(false) volume=None faces=9 edges=24 vertices=16
single_face_filleted_box_5	status=Some(1.0)	sound=true check_lines=0 first=None manifold=Some(false) volume=None faces=9 edges=24 vertices=16
single_face_cyl_on_box_w_fillet_0	status=Some(1.0)	sound=true check_lines=0 first=None manifold=Some(false) volume=None faces=8 edges=18 vertices=12
single_face_cyl_on_box_w_fillet_1	status=Some(1.0)	sound=true check_lines=0 first=None manifold=Some(false) volume=None faces=8 edges=18 vertices=12
single_face_cyl_on_box_w_fillet_2	status=Some(1.0)	sound=true check_lines=0 first=None manifold=Some(false) volume=None faces=8 edges=18 vertices=12
single_face_cyl_on_box_w_fillet_3	status=Some(1.0)	sound=true check_lines=0 first=None manifold=Some(false) volume=None faces=8 edges=17 vertices=12
single_face_cyl_on_box_w_fillet_4	status=Some(1.0)	sound=true check_lines=0 first=None manifold=Some(false) volume=None faces=8 edges=18 vertices=12
single_face_cyl_on_box_w_fillet_5	status=Some(1.0)	sound=true check_lines=0 first=None manifold=Some(false) volume=None faces=8 edges=18 vertices=12
single_face_cyl_on_box_w_fillet_6	status=Some(1.0)	sound=true check_lines=0 first=None manifold=Some(false) volume=None faces=8 edges=18 vertices=12
single_face_cyl_on_box_w_fillet_7	status=Some(1.0)	sound=true check_lines=0 first=None manifold=Some(false) volume=None faces=8 edges=18 vertices=12
single_face_fillet1019_0	status=Some(1.0)	sound=false check_lines=7 first=Some("SmBSplineCurve::Test #15 Bad Repeated CPts: BSplineCurve has Internal Coincident ControlPoints (causes internal point si") manifold=Some(false) volume=None faces=8 edges=21 vertices=14
single_face_fillet1019_1	status=Some(1.0)	sound=false check_lines=8 first=Some("SmBrep::Test #2 Bad Gap: Edge/Vertex gap across Edgeuse end larger than tol (offset=0.00014845343, tolerance=0.000140976") manifold=Some(false) volume=None faces=8 edges=21 vertices=14
single_face_fillet1019_2	status=Some(1.0)	sound=false check_lines=7 first=Some("SmBSplineCurve::Test #15 Bad Repeated CPts: BSplineCurve has Internal Coincident ControlPoints (causes internal point si") manifold=Some(false) volume=None faces=8 edges=21 vertices=14
single_face_fillet1019_3	status=Some(1.0)	sound=false check_lines=6 first=Some("SmBSplineCurve::Test #15 Bad Repeated CPts: BSplineCurve has Internal Coincident ControlPoints (causes internal point si") manifold=Some(false) volume=None faces=8 edges=21 vertices=14
single_face_fillet1019_4	status=Some(1.0)	sound=false check_lines=7 first=Some("SmBSplineCurve::Test #15 Bad Repeated CPts: BSplineCurve has Internal Coincident ControlPoints (causes internal point si") manifold=Some(false) volume=None faces=8 edges=21 vertices=14
single_face_fillet1019_5	status=Some(1.0)	sound=false check_lines=9 first=Some("SmBrep::Test #0 Bad Gap: Edge/Vertex gap across Edgeuse start larger than tol (offset=0.00014845343, tolerance=0.0001409") manifold=Some(false) volume=None faces=8 edges=21 vertices=14
single_face_fillet1019_7	status=Some(1.0)	sound=false check_lines=9 first=Some("SmBrep::Test #0 Bad Gap: Edge/Vertex gap across Edgeuse start larger than tol (offset=0.00014845343, tolerance=0.0001409") manifold=Some(false) volume=None faces=8 edges=21 vertices=14
single_face_fillet1019_8	status=Some(1.0)	sound=false check_lines=8 first=Some("SmBrep::Test #0 Bad Gap: Edge/Vertex gap across Edgeuse start larger than tol (offset=0.00014845343, tolerance=0.0001409") manifold=Some(false) volume=None faces=8 edges=21 vertices=14
rb_33_defillet_8	status=Some(1.0)	sound=true check_lines=0 first=None manifold=Some(false) volume=None faces=8 edges=16 vertices=11
rb_11_box_with_large_fillet_6	status=Some(1.0)	sound=true check_lines=0 first=None manifold=Some(false) volume=None faces=6 edges=15 vertices=10
rb_13_box_with_large_fillet_6	status=Some(1.0)	sound=true check_lines=0 first=None manifold=Some(false) volume=None faces=6 edges=15 vertices=10
rb_33_box_with_large_fillet_6	status=Some(1.0)	sound=true check_lines=0 first=None manifold=Some(false) volume=None faces=6 edges=15 vertices=10
rb_11_filleted_box_9	status=Some(1.0)	sound=true check_lines=0 first=None manifold=Some(false) volume=None faces=9 edges=24 vertices=16
rb_13_filleted_box_9	status=Some(1.0)	sound=true check_lines=0 first=None manifold=Some(false) volume=None faces=9 edges=24 vertices=16
rb_33_filleted_box_9	status=Some(1.0)	sound=true check_lines=0 first=None manifold=Some(false) volume=None faces=9 edges=24 vertices=16
ext_dist_0.0_box_with_large_fillet	status=Some(1.0)	sound=true check_lines=0 first=None manifold=Some(false) volume=None faces=6 edges=15 vertices=10
fillet_group_torii_fillet_b_rb11	status=Some(1.0)	sound=false check_lines=2 first=Some("SmBrep::Test #4 Bad Gap: Edge/Surface gap across Edgeuse larger than tol (offset=6.1119608e-05, tolerance=5.7231569e-05)") manifold=Some(false) volume=None faces=11 edges=32 vertices=22
fillet_each_torii_fillet_b	status=Some(1.0)	sound=false check_lines=2 first=Some("SmBrep::Test #4 Bad Gap: Edge/Surface gap across Edgeuse larger than tol (offset=6.1119608e-05, tolerance=5.7231569e-05)") manifold=Some(false) volume=None faces=12 edges=33 vertices=22
fillet_group_fillet1019_rb11	status=Some(1.0)	sound=false check_lines=1 first=Some("SmBrep::Test #0 Bad Gap: Edge/Vertex gap across Edgeuse start larger than tol (offset=0.00014845343, tolerance=0.0001409") manifold=Some(false) volume=None faces=6 edges=19 vertices=14
fillet_each_fillet1019	status=Some(1.0)	sound=false check_lines=6 first=Some("SmBSplineCurve::Test #15 Bad Repeated CPts: BSplineCurve has Internal Coincident ControlPoints (causes internal point si") manifold=Some(false) volume=None faces=8 edges=21 vertices=14
fillet_each_cyl_on_box_w_fillet	status=Some(1.0)	sound=true check_lines=0 first=None manifold=Some(false) volume=None faces=11 edges=22 vertices=14
fillet_group_inset_cube_1x3fillet_rb11	status=Some(1.0)	sound=true check_lines=0 first=None manifold=Some(false) volume=None faces=9 edges=27 vertices=19
fillet_each_inset_cube_1x3fillet	status=Some(1.0)	sound=true check_lines=0 first=None manifold=Some(false) volume=None faces=11 edges=30 vertices=20
fillet_group_inset_cube_3x1fillet_rb11	status=Some(1.0)	sound=true check_lines=0 first=None manifold=Some(false) volume=None faces=9 edges=27 vertices=19
fillet_each_inset_cube_3x1fillet	status=Some(1.0)	sound=true check_lines=0 first=None manifold=Some(false) volume=None faces=11 edges=30 vertices=20
single_loop_circle_defillet_2	status=Some(1.0)	sound=true check_lines=0 first=None manifold=Some(false) volume=None faces=3 edges=5 vertices=3
single_loop_outer_defillet_2	status=Some(1.0)	sound=true check_lines=0 first=None manifold=Some(false) volume=None faces=5 edges=12 vertices=8
multi_rebuild_1_extrusion	status=Some(1.0)	sound=true check_lines=0 first=None manifold=Some(false) volume=None faces=9 edges=22 vertices=18
multi_rebuild_2_extrusion	status=Some(1.0)	sound=true check_lines=0 first=None manifold=Some(true) volume=Some(757.499499825162) faces=10 edges=22 vertices=18
```

## After

```
single_face_defillet_0	status=Some(1.0)	sound=true check_lines=0 first=None manifold=Some(true) volume=Some(8.814775315168667) faces=9 edges=17 vertices=11 tol brep=Some(1e-5) edge_max=Some(1e-5) vertex_max=Some(1e-5)
single_face_defillet_1	status=Some(1.0)	sound=true check_lines=0 first=None manifold=Some(true) volume=Some(8.814775315168667) faces=9 edges=17 vertices=11 tol brep=Some(1e-5) edge_max=Some(1e-5) vertex_max=Some(1e-5)
single_face_defillet_2	status=Some(1.0)	sound=true check_lines=0 first=None manifold=Some(true) volume=Some(8.814775315168667) faces=9 edges=17 vertices=11 tol brep=Some(1e-5) edge_max=Some(1e-5) vertex_max=Some(1e-5)
single_face_defillet_3	status=Some(1.0)	sound=true check_lines=0 first=None manifold=Some(true) volume=Some(8.814775315168667) faces=9 edges=17 vertices=11 tol brep=Some(1e-5) edge_max=Some(1e-5) vertex_max=Some(1e-5)
single_face_defillet_4	status=Some(1.0)	sound=true check_lines=0 first=None manifold=Some(true) volume=Some(8.814775315168667) faces=9 edges=17 vertices=11 tol brep=Some(1e-5) edge_max=Some(1e-5) vertex_max=Some(1e-5)
single_face_defillet_5	status=Some(1.0)	sound=true check_lines=0 first=None manifold=Some(true) volume=Some(8.814775315168667) faces=9 edges=17 vertices=11 tol brep=Some(1e-5) edge_max=Some(1e-5) vertex_max=Some(1e-5)
single_face_defillet_6	status=Some(1.0)	sound=true check_lines=0 first=None manifold=Some(true) volume=Some(8.814775315168667) faces=9 edges=17 vertices=11 tol brep=Some(1e-5) edge_max=Some(1e-5) vertex_max=Some(1e-5)
single_face_defillet_7	status=Some(1.0)	sound=true check_lines=0 first=None manifold=Some(true) volume=Some(8.814775315168667) faces=9 edges=17 vertices=11 tol brep=Some(1e-5) edge_max=Some(1e-5) vertex_max=Some(1e-5)
single_face_inset_cube_1x1fillet_0	status=Some(1.0)	sound=true check_lines=0 first=None manifold=Some(true) volume=Some(6.9799999999999995) faces=10 edges=24 vertices=16 tol brep=Some(1e-5) edge_max=Some(1e-5) vertex_max=Some(1e-5)
single_face_inset_cube_1x1fillet_1	status=Some(1.0)	sound=true check_lines=0 first=None manifold=Some(true) volume=Some(6.9799999999999995) faces=10 edges=24 vertices=16 tol brep=Some(1e-5) edge_max=Some(1e-5) vertex_max=Some(1e-5)
single_face_inset_cube_1x1fillet_2	status=Some(1.0)	sound=true check_lines=0 first=None manifold=Some(true) volume=Some(6.9799999999999995) faces=10 edges=24 vertices=16 tol brep=Some(1e-5) edge_max=Some(1e-5) vertex_max=Some(1e-5)
single_face_inset_cube_1x1fillet_3	status=Some(1.0)	sound=true check_lines=0 first=None manifold=Some(true) volume=Some(6.9799999999999995) faces=10 edges=24 vertices=16 tol brep=Some(1e-5) edge_max=Some(1e-5) vertex_max=Some(1e-5)
single_face_inset_cube_1x1fillet_5	status=Some(1.0)	sound=true check_lines=0 first=None manifold=Some(true) volume=Some(6.9799999999999995) faces=10 edges=24 vertices=16 tol brep=Some(1e-5) edge_max=Some(1e-5) vertex_max=Some(1e-5)
single_face_inset_cube_1x1fillet_6	status=Some(1.0)	sound=true check_lines=0 first=None manifold=Some(true) volume=Some(6.9799999999999995) faces=10 edges=24 vertices=16 tol brep=Some(1e-5) edge_max=Some(1e-5) vertex_max=Some(1e-5)
single_face_inset_cube_1x1fillet_7	status=Some(1.0)	sound=true check_lines=0 first=None manifold=Some(true) volume=Some(6.9799999999999995) faces=10 edges=24 vertices=16 tol brep=Some(1e-5) edge_max=Some(1e-5) vertex_max=Some(1e-5)
single_face_inset_cube_1x1fillet_8	status=Some(1.0)	sound=true check_lines=0 first=None manifold=Some(true) volume=Some(6.9799999999999995) faces=10 edges=24 vertices=16 tol brep=Some(1e-5) edge_max=Some(1e-5) vertex_max=Some(1e-5)
single_face_inset_cube_1x1fillet_9	status=Some(1.0)	sound=true check_lines=0 first=None manifold=Some(true) volume=Some(6.9799999999999995) faces=10 edges=24 vertices=16 tol brep=Some(1e-5) edge_max=Some(1e-5) vertex_max=Some(1e-5)
single_face_flathead_screw_top_0	status=Some(1.0)	sound=true check_lines=0 first=None manifold=Some(true) volume=Some(101.15312478936164) faces=7 edges=15 vertices=10 tol brep=Some(1e-5) edge_max=Some(1e-5) vertex_max=Some(1e-5)
single_face_flathead_screw_top_1	status=Some(1.0)	sound=true check_lines=0 first=None manifold=Some(true) volume=Some(101.15312478936164) faces=7 edges=15 vertices=10 tol brep=Some(1e-5) edge_max=Some(1e-5) vertex_max=Some(1e-5)
single_face_flathead_screw_top_2	status=Some(1.0)	sound=true check_lines=0 first=None manifold=Some(true) volume=Some(101.15312478936164) faces=7 edges=15 vertices=10 tol brep=Some(1e-5) edge_max=Some(1e-5) vertex_max=Some(1e-5)
single_face_flathead_screw_top_3	status=Some(1.0)	sound=true check_lines=0 first=None manifold=Some(true) volume=Some(101.15312478936164) faces=7 edges=15 vertices=10 tol brep=Some(1e-5) edge_max=Some(1e-5) vertex_max=Some(1e-5)
single_face_flathead_screw_top_4	status=Some(1.0)	sound=true check_lines=0 first=None manifold=Some(true) volume=Some(101.15312478936164) faces=7 edges=15 vertices=10 tol brep=Some(1e-5) edge_max=Some(1e-5) vertex_max=Some(1e-5)
single_face_flathead_screw_top_5	status=Some(1.0)	sound=true check_lines=0 first=None manifold=Some(true) volume=Some(101.15312478936164) faces=7 edges=15 vertices=10 tol brep=Some(1e-5) edge_max=Some(1e-5) vertex_max=Some(1e-5)
single_face_flathead_screw_top_6	status=Some(1.0)	sound=true check_lines=0 first=None manifold=Some(true) volume=Some(101.15312478936164) faces=7 edges=15 vertices=10 tol brep=Some(1e-5) edge_max=Some(1e-5) vertex_max=Some(1e-5)
single_face_box_with_large_fillet_0	status=Some(1.0)	sound=true check_lines=0 first=None manifold=Some(true) volume=Some(862.654824581879) faces=7 edges=15 vertices=10 tol brep=Some(1e-5) edge_max=Some(0.002374395088828545) vertex_max=Some(0.002374395088828545)
single_face_box_with_large_fillet_1	status=Some(1.0)	sound=true check_lines=0 first=None manifold=Some(true) volume=Some(862.654824581879) faces=7 edges=15 vertices=10 tol brep=Some(1e-5) edge_max=Some(0.002374395088828545) vertex_max=Some(0.002374395088828545)
single_face_box_with_large_fillet_2	status=Some(1.0)	sound=true check_lines=0 first=None manifold=Some(true) volume=Some(862.654824581879) faces=7 edges=15 vertices=10 tol brep=Some(1e-5) edge_max=Some(0.002374395088828545) vertex_max=Some(0.002374395088828545)
single_face_box_with_large_fillet_3	status=Some(1.0)	sound=true check_lines=0 first=None manifold=Some(true) volume=Some(862.654824581879) faces=7 edges=15 vertices=10 tol brep=Some(1e-5) edge_max=Some(0.002374395088828545) vertex_max=Some(0.002374395088828545)
single_face_box_with_large_fillet_4	status=Some(1.0)	sound=true check_lines=0 first=None manifold=Some(true) volume=Some(862.654824581879) faces=7 edges=15 vertices=10 tol brep=Some(1e-5) edge_max=Some(0.002374395088828545) vertex_max=Some(0.002374395088828545)
single_face_box_with_large_fillet_5	status=Some(1.0)	sound=true check_lines=0 first=None manifold=Some(true) volume=Some(862.654824581879) faces=7 edges=15 vertices=10 tol brep=Some(1e-5) edge_max=Some(0.002374395088828545) vertex_max=Some(0.002374395088828545)
single_face_filleted_box_0	status=Some(1.0)	sound=true check_lines=0 first=None manifold=Some(true) volume=Some(991.4159278854712) faces=10 edges=24 vertices=16 tol brep=Some(1e-5) edge_max=Some(1e-5) vertex_max=Some(1e-5)
single_face_filleted_box_1	status=Some(1.0)	sound=true check_lines=0 first=None manifold=Some(true) volume=Some(991.4159278854712) faces=10 edges=24 vertices=16 tol brep=Some(1e-5) edge_max=Some(1e-5) vertex_max=Some(1e-5)
single_face_filleted_box_2	status=Some(1.0)	sound=true check_lines=0 first=None manifold=Some(true) volume=Some(991.4159278854712) faces=10 edges=24 vertices=16 tol brep=Some(1e-5) edge_max=Some(1e-5) vertex_max=Some(1e-5)
single_face_filleted_box_3	status=Some(1.0)	sound=true check_lines=0 first=None manifold=Some(true) volume=Some(991.4159278854712) faces=10 edges=24 vertices=16 tol brep=Some(1e-5) edge_max=Some(1e-5) vertex_max=Some(1e-5)
single_face_filleted_box_4	status=Some(1.0)	sound=true check_lines=0 first=None manifold=Some(true) volume=Some(991.4159278854712) faces=10 edges=24 vertices=16 tol brep=Some(1e-5) edge_max=Some(1e-5) vertex_max=Some(1e-5)
single_face_filleted_box_5	status=Some(1.0)	sound=true check_lines=0 first=None manifold=Some(true) volume=Some(991.4159278854712) faces=10 edges=24 vertices=16 tol brep=Some(1e-5) edge_max=Some(1e-5) vertex_max=Some(1e-5)
single_face_cyl_on_box_w_fillet_0	status=Some(1.0)	sound=true check_lines=0 first=None manifold=Some(true) volume=Some(133.74446786794297) faces=9 edges=18 vertices=12 tol brep=Some(1e-5) edge_max=Some(1e-5) vertex_max=Some(1e-5)
single_face_cyl_on_box_w_fillet_1	status=Some(1.0)	sound=true check_lines=0 first=None manifold=Some(true) volume=Some(133.74446786794297) faces=9 edges=18 vertices=12 tol brep=Some(1e-5) edge_max=Some(1e-5) vertex_max=Some(1e-5)
single_face_cyl_on_box_w_fillet_2	status=Some(1.0)	sound=true check_lines=0 first=None manifold=Some(true) volume=Some(133.74446786794297) faces=9 edges=18 vertices=12 tol brep=Some(1e-5) edge_max=Some(1e-5) vertex_max=Some(1e-5)
single_face_cyl_on_box_w_fillet_3	status=Some(1.0)	sound=true check_lines=0 first=None manifold=Some(true) volume=Some(133.74446786794297) faces=9 edges=18 vertices=12 tol brep=Some(1e-5) edge_max=Some(1e-5) vertex_max=Some(1e-5)
single_face_cyl_on_box_w_fillet_4	status=Some(1.0)	sound=true check_lines=0 first=None manifold=Some(true) volume=Some(133.74446786794297) faces=9 edges=18 vertices=12 tol brep=Some(1e-5) edge_max=Some(1e-5) vertex_max=Some(1e-5)
single_face_cyl_on_box_w_fillet_5	status=Some(1.0)	sound=true check_lines=0 first=None manifold=Some(true) volume=Some(133.74446786794297) faces=9 edges=18 vertices=12 tol brep=Some(1e-5) edge_max=Some(1e-5) vertex_max=Some(1e-5)
single_face_cyl_on_box_w_fillet_6	status=Some(1.0)	sound=true check_lines=0 first=None manifold=Some(true) volume=Some(133.74446786794297) faces=9 edges=18 vertices=12 tol brep=Some(1e-5) edge_max=Some(1e-5) vertex_max=Some(1e-5)
single_face_cyl_on_box_w_fillet_7	status=Some(1.0)	sound=true check_lines=0 first=None manifold=Some(true) volume=Some(133.74446786794297) faces=9 edges=18 vertices=12 tol brep=Some(1e-5) edge_max=Some(1e-5) vertex_max=Some(1e-5)
single_face_fillet1019_0	status=Some(1.0)	sound=false check_lines=9 first=Some("SmBrep::Test #0 Bad Gap: Edge/Vertex gap across Edgeuse start larger than tol (offset=0.00014845343, tolerance=0.0001409") manifold=Some(true) volume=Some(59314.64524122541) faces=9 edges=21 vertices=14 tol brep=Some(1e-5) edge_max=Some(0.000281953169796288) vertex_max=Some(0.000281953169796288)
single_face_fillet1019_1	status=Some(1.0)	sound=false check_lines=9 first=Some("SmBrep::Test #0 Bad Gap: Edge/Vertex gap across Edgeuse start larger than tol (offset=0.00014845343, tolerance=0.0001409") manifold=Some(true) volume=Some(59314.64524122541) faces=9 edges=21 vertices=14 tol brep=Some(1e-5) edge_max=Some(0.000281953169796288) vertex_max=Some(0.000281953169796288)
single_face_fillet1019_2	status=Some(1.0)	sound=false check_lines=9 first=Some("SmBrep::Test #0 Bad Gap: Edge/Vertex gap across Edgeuse start larger than tol (offset=0.00014845343, tolerance=0.0001409") manifold=Some(true) volume=Some(59314.64524122541) faces=9 edges=21 vertices=14 tol brep=Some(1e-5) edge_max=Some(0.000281953169796288) vertex_max=Some(0.000281953169796288)
single_face_fillet1019_3	status=Some(1.0)	sound=false check_lines=9 first=Some("SmBrep::Test #0 Bad Gap: Edge/Vertex gap across Edgeuse start larger than tol (offset=0.00014845343, tolerance=0.0001409") manifold=Some(true) volume=Some(59314.64524122541) faces=9 edges=21 vertices=14 tol brep=Some(1e-5) edge_max=Some(0.000281953169796288) vertex_max=Some(0.000281953169796288)
single_face_fillet1019_4	status=Some(1.0)	sound=false check_lines=9 first=Some("SmBrep::Test #0 Bad Gap: Edge/Vertex gap across Edgeuse start larger than tol (offset=0.00014845343, tolerance=0.0001409") manifold=Some(true) volume=Some(59314.64524122541) faces=9 edges=21 vertices=14 tol brep=Some(1e-5) edge_max=Some(0.000281953169796288) vertex_max=Some(0.000281953169796288)
single_face_fillet1019_5	status=Some(1.0)	sound=false check_lines=9 first=Some("SmBrep::Test #0 Bad Gap: Edge/Vertex gap across Edgeuse start larger than tol (offset=0.00014845343, tolerance=0.0001409") manifold=Some(true) volume=Some(59314.64524122541) faces=9 edges=21 vertices=14 tol brep=Some(1e-5) edge_max=Some(0.000281953169796288) vertex_max=Some(0.000281953169796288)
single_face_fillet1019_7	status=Some(1.0)	sound=false check_lines=9 first=Some("SmBrep::Test #0 Bad Gap: Edge/Vertex gap across Edgeuse start larger than tol (offset=0.00014845343, tolerance=0.0001409") manifold=Some(true) volume=Some(59314.64524122541) faces=9 edges=21 vertices=14 tol brep=Some(1e-5) edge_max=Some(0.000281953169796288) vertex_max=Some(0.000281953169796288)
single_face_fillet1019_8	status=Some(1.0)	sound=false check_lines=9 first=Some("SmBrep::Test #0 Bad Gap: Edge/Vertex gap across Edgeuse start larger than tol (offset=0.00014845343, tolerance=0.0001409") manifold=Some(true) volume=Some(59314.64524122541) faces=9 edges=21 vertices=14 tol brep=Some(1e-5) edge_max=Some(0.000281953169796288) vertex_max=Some(0.000281953169796288)
rb_33_defillet_8	status=Some(1.0)	sound=true check_lines=0 first=None manifold=Some(true) volume=Some(8.814775315168667) faces=9 edges=17 vertices=11 tol brep=Some(1e-5) edge_max=Some(1e-5) vertex_max=Some(1e-5)
rb_11_box_with_large_fillet_6	status=Some(1.0)	sound=true check_lines=0 first=None manifold=Some(true) volume=Some(862.654824581879) faces=7 edges=15 vertices=10 tol brep=Some(1e-5) edge_max=Some(0.002374395088828545) vertex_max=Some(0.002374395088828545)
rb_13_box_with_large_fillet_6	status=Some(1.0)	sound=true check_lines=0 first=None manifold=Some(true) volume=Some(862.654824581879) faces=7 edges=15 vertices=10 tol brep=Some(1e-5) edge_max=Some(0.002374395088828545) vertex_max=Some(0.002374395088828545)
rb_33_box_with_large_fillet_6	status=Some(1.0)	sound=true check_lines=0 first=None manifold=Some(true) volume=Some(862.654824581879) faces=7 edges=15 vertices=10 tol brep=Some(1e-5) edge_max=Some(0.002374395088828545) vertex_max=Some(0.002374395088828545)
rb_11_filleted_box_9	status=Some(1.0)	sound=true check_lines=0 first=None manifold=Some(true) volume=Some(991.4159278854712) faces=10 edges=24 vertices=16 tol brep=Some(1e-5) edge_max=Some(1e-5) vertex_max=Some(1e-5)
rb_13_filleted_box_9	status=Some(1.0)	sound=true check_lines=0 first=None manifold=Some(true) volume=Some(991.4159278854712) faces=10 edges=24 vertices=16 tol brep=Some(1e-5) edge_max=Some(1e-5) vertex_max=Some(1e-5)
rb_33_filleted_box_9	status=Some(1.0)	sound=true check_lines=0 first=None manifold=Some(true) volume=Some(991.4159278854712) faces=10 edges=24 vertices=16 tol brep=Some(1e-5) edge_max=Some(1e-5) vertex_max=Some(1e-5)
ext_dist_0.0_box_with_large_fillet	status=Some(1.0)	sound=true check_lines=0 first=None manifold=Some(true) volume=Some(862.654824581879) faces=7 edges=15 vertices=10 tol brep=Some(1e-5) edge_max=Some(0.002374395088828545) vertex_max=Some(0.002374395088828545)
fillet_group_torii_fillet_b_rb11	status=Some(1.0)	sound=false check_lines=2 first=Some("SmBrep::Test #4 Bad Gap: Edge/Surface gap across Edgeuse larger than tol (offset=6.1119608e-05, tolerance=5.7231569e-05)") manifold=Some(true) volume=Some(3937.4426919798007) faces=13 edges=33 vertices=22 tol brep=Some(1e-5) edge_max=Some(0.00016066848697267357) vertex_max=Some(0.00016066848697267357)
fillet_each_torii_fillet_b	status=Some(1.0)	sound=false check_lines=2 first=Some("SmBrep::Test #4 Bad Gap: Edge/Surface gap across Edgeuse larger than tol (offset=6.1119608e-05, tolerance=5.7231569e-05)") manifold=Some(true) volume=Some(3937.4426919798007) faces=13 edges=33 vertices=22 tol brep=Some(1e-5) edge_max=Some(0.00016066848697267357) vertex_max=Some(0.00016066848697267357)
fillet_group_fillet1019_rb11	status=Some(1.0)	sound=false check_lines=9 first=Some("SmBrep::Test #0 Bad Gap: Edge/Vertex gap across Edgeuse start larger than tol (offset=0.00014845343, tolerance=0.0001409") manifold=Some(true) volume=Some(59314.64524122541) faces=9 edges=21 vertices=14 tol brep=Some(1e-5) edge_max=Some(0.000281953169796288) vertex_max=Some(0.000281953169796288)
fillet_each_fillet1019	status=Some(1.0)	sound=false check_lines=9 first=Some("SmBrep::Test #0 Bad Gap: Edge/Vertex gap across Edgeuse start larger than tol (offset=0.00014845343, tolerance=0.0001409") manifold=Some(true) volume=Some(59314.64524122541) faces=9 edges=21 vertices=14 tol brep=Some(1e-5) edge_max=Some(0.000281953169796288) vertex_max=Some(0.000281953169796288)
fillet_each_cyl_on_box_w_fillet	status=Some(1.0)	sound=true check_lines=0 first=None manifold=Some(true) volume=Some(134.8174770412367) faces=12 edges=23 vertices=14 tol brep=Some(1e-5) edge_max=Some(1e-5) vertex_max=Some(1e-5)
fillet_group_inset_cube_1x3fillet_rb11	status=Some(1.0)	sound=true check_lines=0 first=None manifold=Some(true) volume=Some(6.9399999999999995) faces=12 edges=30 vertices=20 tol brep=Some(1e-5) edge_max=Some(1e-5) vertex_max=Some(1e-5)
fillet_each_inset_cube_1x3fillet	status=Some(1.0)	sound=true check_lines=0 first=None manifold=Some(true) volume=Some(6.9399999999999995) faces=12 edges=30 vertices=20 tol brep=Some(1e-5) edge_max=Some(1e-5) vertex_max=Some(1e-5)
fillet_group_inset_cube_3x1fillet_rb11	status=Some(1.0)	sound=true check_lines=0 first=None manifold=Some(true) volume=Some(6.939999999999999) faces=12 edges=30 vertices=20 tol brep=Some(1e-5) edge_max=Some(1e-5) vertex_max=Some(1e-5)
fillet_each_inset_cube_3x1fillet	status=Some(1.0)	sound=true check_lines=0 first=None manifold=Some(true) volume=Some(6.939999999999999) faces=12 edges=30 vertices=20 tol brep=Some(1e-5) edge_max=Some(1e-5) vertex_max=Some(1e-5)
single_loop_circle_defillet_2	status=Some(1.0)	sound=true check_lines=0 first=None manifold=Some(true) volume=Some(8.814775315168667) faces=9 edges=17 vertices=11 tol brep=Some(1e-5) edge_max=Some(1e-5) vertex_max=Some(1e-5)
single_loop_outer_defillet_2	status=Some(1.0)	sound=true check_lines=0 first=None manifold=Some(true) volume=Some(8.814775315168667) faces=9 edges=17 vertices=11 tol brep=Some(1e-5) edge_max=Some(1e-5) vertex_max=Some(1e-5)
multi_rebuild_1_extrusion	status=Some(1.0)	sound=true check_lines=0 first=None manifold=Some(true) volume=Some(757.499499825162) faces=10 edges=22 vertices=18 tol brep=Some(1e-5) edge_max=Some(1e-5) vertex_max=Some(1e-5)
multi_rebuild_2_extrusion	status=Some(1.0)	sound=true check_lines=0 first=None manifold=Some(true) volume=Some(757.499499825162) faces=10 edges=22 vertices=18 tol brep=Some(1e-5) edge_max=Some(1e-5) vertex_max=Some(1e-5)
```
