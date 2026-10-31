//! Regression test for WR-650 (the SolidCraft fork's tangency-labs/smlib@93db777 and
//! @b298cc2, #1986), from SolidCraft's tabbed-plate recipe: a disc of radius 25 with a
//! tab (two R2 corners) on top, extruded 2, its six top edges rounded r 0.5. The mitred
//! concave corners at (±4, y), where the tab's sides meet the R25 wall, trace onto the
//! wall's continuation past its arc, which `offset_extension_as_itself` carries on as the
//! cylinder itself (WR-633), in arc spans that stop at the closed turn (WR-650).
//!
//! SolidCraft runs it with its whole preset of fixes on; the round of the mitred corners
//! needs `fillet_mitre_gap_patch` and `fillet_mitre_concave_face_corners` (without them
//! the level 2 check reports `SmSurfOfRevolution` test 7, and 3.8e-4 of the volume is
//! missing), and with `offset_extension_as_itself` off the fillet fails (status 8).
//!
//! The body must be sound, the plate's volume twice its outline's area, and the removed
//! volume within 1e-6 relative of the round's closed form, integrated over the quarter
//! circle of the round's section.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use std::f64::consts::PI;

use solidcore::blend::fillet_executive::session::FilletSession;
use solidcore::curve::curve::Curve;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::{Point3, Vec3};
use solidcore::primitive::api_curves::{create_arc, create_line_segment};
use solidcore::sweep::api_brep::create_taper_extrude;
use solidcore::topology::brep::p2::is_manifold_solid;
use solidcore::topology::brep::p5::{stitch_and_orient, StitchAndOrientOptions};
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::edge;
use solidcore::topology::EdgeId;
use solidcore::{Brep, Kernel};

const H: f64 = 2.0;
const R: f64 = 0.5;

fn theta() -> f64 {
    (4.0_f64 / 25.0).acos()
}

fn y0() -> f64 {
    (625.0_f64 - 16.0).sqrt()
}

fn cy() -> f64 {
    280.0 / 9.0
}

fn line(a: (f64, f64), b: (f64, f64)) -> Curve {
    Curve::from(create_line_segment(Vec3::new(a.0, a.1, 0.0), Vec3::new(b.0, b.1, 0.0)).unwrap())
}

fn arc(c: (f64, f64), r: f64, from: f64, to: f64) -> Curve {
    Curve::from(
        create_arc(
            Vec3::new(c.0, c.1, 0.0),
            r,
            from.to_degrees(),
            to.to_degrees(),
        )
        .unwrap(),
    )
}

/// The tabbed plate, its outline chained head to tail, extruded to z 0..2.
fn plate(k: &mut Kernel) -> Brep {
    let (t, y, cy) = (theta(), y0(), cy());
    let curves = [
        // pi - theta to 2 pi + theta, a turn back (create_arc takes angles in [-360, 360])
        arc((0.0, 0.0), 25.0, PI - t - 2.0 * PI, t),
        line((4.0, y), (4.0, cy)),
        arc((2.0, cy), 2.0, 0.0, PI / 2.0),
        line((2.0, cy + 2.0), (-2.0, cy + 2.0)),
        arc((-2.0, cy), 2.0, PI / 2.0, PI),
        line((-4.0, cy), (-4.0, y)),
    ];
    let refs: Vec<Option<&Curve>> = curves.iter().map(Some).collect();
    create_taper_extrude(k, &refs, H, 0.0, 3, false).unwrap()
}

/// SolidCraft's outline area, its R25 wall moved in by `da` and the tab's walls by `dt`.
fn area(da: f64, dt: f64) -> f64 {
    let cy = cy();
    let big = 25.0 - da;
    let rho = 2.0 - dt;
    let tab = rho * rho * PI + 2.0 * rho * (2.0 + cy) + 4.0 * (cy + rho);
    let yc = (big * big - (4.0 - dt) * (4.0 - dt)).sqrt();
    let at = yc.atan2(4.0 - dt);
    0.5 * (big * big * (PI + 2.0 * at) + tab) + (4.0 - dt) * (cy - yc)
}

/// The volume the r 0.5 rounds take off, SolidCraft's integral over the round's section.
fn want_removed() -> f64 {
    let n = 8192;
    let dphi = (PI / 2.0) / f64::from(n);
    (0..n)
        .map(|i| {
            let phi = (f64::from(i) + 0.5) * dphi;
            let d = R * (1.0 - phi.cos());
            (area(0.0, 0.0) - area(d, d)) * R * phi.cos() * dphi
        })
        .sum()
}

fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-11, &attrs).unwrap()
}

/// The six top edges, each the one passing within 0.01 of SolidCraft's pick point.
fn top_edges(k: &Kernel, b: &Brep) -> Vec<EdgeId> {
    let cy = cy();
    let picks = [
        Point3::new(0.0, -25.0, H),
        Point3::new(4.0, 27.8945, H),
        Point3::new(3.41421356, 32.5253247, H),
        Point3::new(0.0, cy + 2.0, H),
        Point3::new(-3.41421356, 32.5253247, H),
        Point3::new(-4.0, 27.8945, H),
    ];
    let near = |e: EdgeId, p: Point3| {
        let curve = edge::curve(b, e).unwrap().unwrap();
        let ivl = edge::interval(b, e).unwrap();
        (0..=4000).any(|i| {
            let t = ivl.min() + (ivl.max() - ivl.min()) * f64::from(i) / 4000.0;
            (curve.evaluate_point(k, t).unwrap() - p).length() < 0.01
        })
    };
    picks
        .iter()
        .map(|&p| {
            let found: Vec<EdgeId> = b
                .edge_list()
                .iter()
                .copied()
                .filter(|&e| near(e, p))
                .collect();
            assert_eq!(found.len(), 1, "edges near the pick {p:?}: {found:?}");
            found[0]
        })
        .collect()
}

/// The plate's volume, then the rounded body's soundness (a manifold solid passing the
/// level 2 check) and the volume the rounds took off, or the fillet error code, with
/// SolidCraft's preset of fixes (all 182 of `solidcraft-flags/612ac9a36`) on.
fn round() -> (f64, Result<(bool, f64), i32>) {
    let mut k = Kernel::default();
    k.fixes.analytic_copy_keeps_parameterization = true;
    k.fixes.approximate_surface_bezier_starter = true;
    k.fixes.boolean2d_plane_covers_far_faces = true;
    k.fixes.boolean_cleanup_removes_slits = true;
    k.fixes.boolean_lists_virtual_surface_copies = true;
    k.fixes.boolean_new_face_closure_both_sides = true;
    k.fixes.boolean_outer_shell_first = true;
    k.fixes.boolean_skips_faces_apart = true;
    k.fixes.boolean_splits_definition_across_split = true;
    k.fixes.boolean_temporary_transform_statuses = true;
    k.fixes.boolean_vertex_by_best_crossing = true;
    k.fixes.boolean_wire_edge_keeps_faceuse_shell = true;
    k.fixes.box_face_failures_propagate = true;
    k.fixes.brep_area_between_solid_and_void = true;
    k.fixes.bspline_check_min_radius = true;
    k.fixes.bspline_trim_preserves_uv_spans = true;
    k.fixes.check_loop_projected_seam_ends = true;
    k.fixes.check_loop_vertex_tolerance = true;
    k.fixes.classify_edge_interval = true;
    k.fixes.cone_parallel_line_domains = true;
    k.fixes.cone_parallel_line_midpoint = true;
    k.fixes.corner_blend_coons_patch_tangent_to_sides = true;
    k.fixes.corner_blend_stays_inside_neighbours = true;
    k.fixes.cover_refuses_folded_continuation = true;
    k.fixes.cover_wraps_across_closed_seam = true;
    k.fixes.curve_contacts_at_curve_ends = true;
    k.fixes.curve_loop_normal_from_area = true;
    k.fixes.curve_sweep_caps_at_sweep_ends = true;
    k.fixes.curve_tight_box_interval_knots = true;
    k.fixes.cylinder_plane_tangent_within_tolerance = true;
    k.fixes.cylinder_rebuild_mean_radius = true;
    k.fixes.degenerate_proto_edge_in_one_vertex = true;
    k.fixes.delete_vertex_joins_seam_iso_curve = true;
    k.fixes.edge_check_orders_tangent_faces = true;
    k.fixes.edge_reversal_keeps_failed_edge = true;
    k.fixes.ellipse_adjust_step_interval_keeps_turns = true;
    k.fixes.ellipse_trim_seam_lines_span_cone_height = true;
    k.fixes.extrusion_origin_scaled_zero = true;
    k.fixes.face_tight_box_searches_trim = true;
    k.fixes.fillet_3x2_mixed_concave_side_edge = true;
    k.fixes.fillet_3x2_tangent_side_edge_roles = true;
    k.fixes.fillet_chamfer_cone_on_arc = true;
    k.fixes.fillet_chamfer_cone_stored_as_cone = true;
    k.fixes.fillet_cleanup_keeps_target_faces = true;
    k.fixes.fillet_cleanup_removes_input_slits = true;
    k.fixes.fillet_cliff_overflow = true;
    k.fixes.fillet_corner_ball_center_from_axes = true;
    k.fixes.fillet_corner_ball_patch = true;
    k.fixes.fillet_corner_sphere_from_four_arcs = true;
    k.fixes.fillet_half_degree_crease = true;
    k.fixes.fillet_knife_edge_rounds = true;
    k.fixes.fillet_mitre_concave_face_corners = true;
    k.fixes.fillet_mitre_gap_patch = true;
    k.fixes.fillet_nx1_end_rib_global_merge = true;
    k.fixes.fillet_nx1_opposite_continuation_side_edge = true;
    k.fixes.fillet_nx1_tangent_corner_edge = true;
    k.fixes.fillet_nx1_tangent_side_edge = true;
    k.fixes.fillet_nx1_tangent_side_edge_same_convexity = true;
    k.fixes.fillet_patch_keeps_fillet_edges = true;
    k.fixes.fillet_point_bigger_step_retry = true;
    k.fixes.fillet_point_step_per_point = true;
    k.fixes.fillet_rail_end_on_continued_surface = true;
    k.fixes.fillet_rail_rim_on_face = true;
    k.fixes.fillet_rails_meet_at_one_end_point = true;
    k.fixes.fillet_refuse_ball_not_fitting = true;
    k.fixes.fillet_refuse_crossing_rails = true;
    k.fixes.fillet_refuse_face_consuming_blends = true;
    k.fixes.fillet_refuse_non_manifold_result = true;
    k.fixes.fillet_refuse_overlapping_bands = true;
    k.fixes.fillet_refuse_rails_leaving_faces = true;
    k.fixes.fillet_removal_takes_ring_fillets = true;
    k.fixes.fillet_retry_radius_as_first = true;
    k.fixes.fillet_run_on_past_far_end_only = true;
    k.fixes.fillet_setback_from_start_vertex = true;
    k.fixes.fillet_split_edge_keeps_near_half = true;
    k.fixes.fillet_tangent_point_one_sided_step = true;
    k.fixes.fillet_through_extended_rail_end = true;
    k.fixes.fillet_torus_center_on_axis = true;
    k.fixes.fillet_torus_rail_on_axis = true;
    k.fixes.fillet_torus_span_from_start_section = true;
    k.fixes.fillet_trace_from_tangent_point_toward_next = true;
    k.fixes.fillet_trace_reaches_rails = true;
    k.fixes.fillet_trace_rejects_short_step = true;
    k.fixes.fillet_trim_inside_by_best_dot = true;
    k.fixes.fillet_trim_status = true;
    k.fixes.fillet_variable_span_follows_fillet = true;
    k.fixes.fillet_vertex_error_diagnostics = true;
    k.fixes.flat_corners_test_bits = true;
    k.fixes.found_torus_sphere_keep_tested_nurb = true;
    k.fixes.glue_vertices_joins_every_region = true;
    k.fixes.good_drop_result_distances_in_order = true;
    k.fixes.heal_stage3_failures_propagate = true;
    k.fixes.imprint_extends_edge_to_proto_vertex = true;
    k.fixes.imprint_extension_keeps_analytic_curve = true;
    k.fixes.imprint_reuses_coincident_face_edge = true;
    k.fixes.imprint_trims_edge_to_proto_vertex = true;
    k.fixes.internal_pole_row_through_knot = true;
    k.fixes.iso_curve_sliver_domain_is_degenerate = true;
    k.fixes.join_touching_loops = true;
    k.fixes.keep_operand_uv_trims = true;
    k.fixes.linear_sweep_arc_is_cylinder = true;
    k.fixes.linear_sweep_arc_to_cylinder_zero = true;
    k.fixes.local_operation_box_holds_new_surfaces = true;
    k.fixes.local_operation_closes_walls_to_nothing = true;
    k.fixes.local_operation_offset_status = true;
    k.fixes.local_operation_refuses_loose_tangent_edges = true;
    k.fixes.local_operation_step_walls = true;
    k.fixes.loop_orientation_floor_from_loop_box = true;
    k.fixes.loops_go_on_round_the_vertex = true;
    k.fixes.new_edge_vertex_tolerance_covers_gap = true;
    k.fixes.nurb_cylinder_inside_out_frame = true;
    k.fixes.offset_analytic_mate_edges = true;
    k.fixes.offset_clip_right_side_out = true;
    k.fixes.offset_closed_curve_trim_arc = true;
    k.fixes.offset_closes_loop_gaps = true;
    k.fixes.offset_collapsed_rounds_mated = true;
    k.fixes.offset_excess_faces_after_merge = true;
    k.fixes.offset_extended_edges_mated = true;
    k.fixes.offset_extends_cones_as_themselves = true;
    k.fixes.offset_extension_as_itself = true;
    k.fixes.offset_extension_from_base = true;
    k.fixes.offset_missing_surface_fails = true;
    k.fixes.offset_point_vertex_caps = true;
    k.fixes.offset_resolve_crossing_loops = true;
    k.fixes.offset_stitch_at_body_tolerance = true;
    k.fixes.offset_walls_touch_refused = true;
    k.fixes.oscillating_skips_failed_guesses = true;
    k.fixes.planar_path_profile_placement_statuses = true;
    k.fixes.planar_path_sweep_statuses = true;
    k.fixes.planar_path_uncapped_open_profiles = true;
    k.fixes.point_solve_keeps_triangle_guess = true;
    k.fixes.precise_properties_body_length_scale = true;
    k.fixes.precise_properties_split_at_surface_knots = true;
    k.fixes.primitive_path_curve_statuses = true;
    k.fixes.proto_edge_self_loop_on_vertex_degenerate = true;
    k.fixes.proto_edge_splits_at_captured_vertices = true;
    k.fixes.proto_vertex_miss_is_false = true;
    k.fixes.refine_cc_range_ends = true;
    k.fixes.remove_slit_loops_where_solids_touch = true;
    k.fixes.revolution_offset_axis_both_sides = true;
    k.fixes.revolution_round_offset_collapses = true;
    k.fixes.revolve_analytic_profile_faces = true;
    k.fixes.revolve_arc_centre_near_axis_is_sphere = true;
    k.fixes.revolve_arc_touching_axis_is_horn_torus = true;
    k.fixes.revolve_holed_profile_void = true;
    k.fixes.revolve_nested_shells_as_cavities = true;
    k.fixes.revolve_splits_curves_on_axis = true;
    k.fixes.seam_point_solutions_are_one = true;
    k.fixes.sheet_sweep_joins_profile_wires = true;
    k.fixes.shell_edges_on_analytics_exact = true;
    k.fixes.shell_empty_cavity_fails = true;
    k.fixes.shell_inner_shell_of_statuses = true;
    k.fixes.simple_coincidence_crossing_is_point = true;
    k.fixes.slice_coincident_faces_sheet_side = true;
    k.fixes.sphere_small_circle_not_touching = true;
    k.fixes.ssi_low_angle_start_on_probation = true;
    k.fixes.ssi_no_reverse_of_empty_trace = true;
    k.fixes.ssi_pole_point_keeps_its_tangent = true;
    k.fixes.ssi_singular_start_inside_direction = true;
    k.fixes.ssi_tangent_contact_points = true;
    k.fixes.ssi_tangent_curve_tracer = true;
    k.fixes.ssi_trace_tangent_curves_once = true;
    k.fixes.stitch_solid_nests_cavities = true;
    k.fixes.sweep_frame_on_planar_path_normal = true;
    k.fixes.taper_pivot_nearest_corner = true;
    k.fixes.taper_tangent_corner_planes = true;
    k.fixes.taper_wall_closes_at_apex = true;
    k.fixes.thicken_planar_side_walls = true;
    k.fixes.topology_failed_evaluations_propagate = true;
    k.fixes.torus_check_reads_v_interval = true;
    k.fixes.torus_check_spindle_apex = true;
    k.fixes.torus_iso_circle_axis_from_midpoint = true;
    k.fixes.torus_keeps_inside_out = true;
    k.fixes.torus_major_radius_from_origin = true;
    k.fixes.torus_signed_gen_curve_interval = true;
    k.fixes.transform_model_size_from_body_box = true;
    k.fixes.transform_scales_each_tolerance = true;
    k.fixes.trim_boundary_seam_wrap = true;
    k.fixes.uv_trim_checks_on_surface = true;
    k.fixes.uv_trim_corner_end_points_only = true;
    k.fixes.uv_trim_joins_drop_pieces = true;
    k.fixes.uv_trim_reversal_on_surface = true;
    let mut b = plate(&mut k);
    let before = volume(&k, &mut b);
    let edges = top_edges(&k, &b);
    {
        let mut session = FilletSession::new(&mut k, &mut b).unwrap();
        session
            .prepare_constant_radius_edges(&mut k, &edges, R, 1, 1, 1.0)
            .unwrap();
        if session.do_fillet(&mut k).is_err() {
            return (before, Err(session.error_info().code));
        }
    }
    stitch_and_orient(&mut k, &mut b, &StitchAndOrientOptions::default()).unwrap();
    let sound = is_manifold_solid(&b).unwrap()
        && check_brep(&mut k, &mut b, CheckLevel::Level2)
            .unwrap()
            .is_valid();
    let after = volume(&k, &mut b);
    (before, Ok((sound, before - after)))
}

#[test]
fn wr650_tabbed_plate_rounds_mitred_on_the_arcs_continuation() {
    let plate_want = H * area(0.0, 0.0);
    let want = want_removed();
    let (before, rounded) = round();
    assert!(
        (before - plate_want).abs() <= 1e-10 * plate_want,
        "plate volume {before}, expected {plate_want}"
    );
    let (sound, removed) = rounded.unwrap_or_else(|code| panic!("the rounds failed: {code}"));
    assert!(sound, "the rounded plate is not sound");
    assert!(
        (removed - want).abs() <= 1e-6 * want,
        "removed {removed}, expected {want}"
    );
}
