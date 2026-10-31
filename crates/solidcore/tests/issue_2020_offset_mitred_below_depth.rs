//! #2020 (WR-615): mitred rounds on a moved face, the top moved below the rounds' depth.
//! A 40 x 30 x 10 box has its four top edges rounded r 3 (mitred at the top corners) and
//! its top offset by `d` (`OffsetFacesExecutive::offset_faces`): the top goes to
//! z = 10 + d and the rounds' axes to z = 7 + d. Below d = -7 the axes go under the
//! floor, and the rounds meet it.
//!
//! The volume is analytic: each section at height z is the 34 x 24 top moved out by
//! `w(z) = sqrt(9 - (z - z_axis)^2)` (3 below the axis), its corners mitred, so
//! V = integral of (34 + 2w)(24 + 2w) dz over 0..10 + d.
//!
//! With SolidCraft's flags (183 `UpstreamFixes`, among them the fork's cliff-overflow
//! and side-edge run-on fixes), every depth but -9 builds. At -9 the rail meets the
//! side edge's run-on line exactly at the end of its extended interval (top 1, rail
//! at z -2, run-on reach 2 at the doubled extension factor), and the put-back fails;
//! the fork (2fc375f) builds it with 6 faces and V 1006.320632128887. That stays open
//! (#2020). With the flags off, the put-back fails at -7.5 (a corner edge) and at -9
//! (a rollover). Each such failure is now a typed refusal,
//! `OffsetFacesRefusal::NotPutBack`, where it had none.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::blend::fillet_executive::api::fillet_edges;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::Point3;
use solidcore::offset::offset_faces::{OffsetFacesExecutive, OffsetFacesRefusal};
use solidcore::primitive::api_primitives::create_box;
use solidcore::query::api_queries::brep_is_manifold_solid;
use solidcore::surface::plane::Plane;
use solidcore::topology::face::p2 as face2;
use solidcore::topology::{edge, vertex, Brep, EdgeId, FaceId};
use solidcore::Kernel;

/// The fork's volume at d = -9 (2fc375f, `smlib_offset_faces`): 6 faces.
const FORK_VOLUME_D9: f64 = 1006.320632128887;

/// The edge whose midpoint is `p`.
fn edge_through(b: &Brep, p: Point3) -> EdgeId {
    b.edges()
        .ids()
        .find(|&e| {
            let (s, t) = edge::start_end_vertices(b, e).unwrap();
            let a = vertex::point(b, s.unwrap()).unwrap();
            let c = vertex::point(b, t.unwrap()).unwrap();
            (0.5 * (a.x + c.x) - p.x).abs() < 1e-6
                && (0.5 * (a.y + c.y) - p.y).abs() < 1e-6
                && (0.5 * (a.z + c.z) - p.z).abs() < 1e-6
        })
        .unwrap()
}

/// The plane face whose vertices are all at height `z`.
fn top(b: &Brep, z: f64) -> FaceId {
    *b.face_list()
        .iter()
        .find(|&&f| {
            face2::surface(b, f)
                .unwrap()
                .unwrap()
                .is_kind_of(Plane::TYPE)
                .unwrap()
                && solidcore::topology::face::p4::edges(b, f, None, false, None)
                    .unwrap()
                    .into_iter()
                    .all(|e| {
                        let (s, _) = edge::start_end_vertices(b, e).unwrap();
                        (vertex::point(b, s.unwrap()).unwrap().z - z).abs() < 1e-6
                    })
        })
        .unwrap()
}

fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-9, &attrs).unwrap()
}

/// The analytic volume after the top moved by `d` (see the module docs).
fn analytic(d: f64) -> f64 {
    let top = 10.0 + d;
    let axis = top - 3.0;
    // below the axis the walls are the box's: w = 3
    let below = axis.max(0.0);
    let mut v = 40.0 * 30.0 * below;
    // over the axis: 816 L + 116 integral(w) + 4 integral(w^2), u = z - axis
    let (u0, u1) = (below - axis, top - axis);
    let w_int = |u: f64| 0.5 * (u * (9.0 - u * u).max(0.0).sqrt() + 9.0 * (u / 3.0).asin());
    let w2_int = |u: f64| 9.0 * u - u * u * u / 3.0;
    v += 816.0 * (u1 - u0) + 116.0 * (w_int(u1) - w_int(u0)) + 4.0 * (w2_int(u1) - w2_int(u0));
    v
}

/// The rounded box with its top moved by `d`: the B-rep, the executive and the result.
fn moved(k: &mut Kernel, d: f64) -> (Brep, OffsetFacesExecutive, solidcore::Result<()>) {
    let mut b = create_box(k, Point3::new(0.0, 0.0, 0.0), 40.0, 30.0, 10.0).unwrap();
    let edges: Vec<EdgeId> = [
        Point3::new(20.0, 0.0, 10.0),
        Point3::new(20.0, 30.0, 10.0),
        Point3::new(0.0, 15.0, 10.0),
        Point3::new(40.0, 15.0, 10.0),
    ]
    .into_iter()
    .map(|p| edge_through(&b, p))
    .collect();
    fillet_edges(k, &mut b, &edges, 3.0, 1, 1, 1.0).unwrap();
    let t = top(&b, 10.0);
    let mut ex = OffsetFacesExecutive::new();
    let r = ex.offset_faces(k, &mut b, &[t], d);
    (b, ex, r)
}

#[test]
fn analytic_volume_matches_the_issue() {
    // the issue's table and its -9 derivation (816 + 116 * 1.54873 + 4 * 8/3)
    for (d, v) in [
        (-6.9, 3459.955684),
        (-7.0, 3339.955684),
        (-7.5, 2740.931296),
        (-8.0, 2147.845376),
        (-9.0, 1006.320628),
        (-9.5, 476.105890),
    ] {
        assert!(
            (analytic(d) - v).abs() < 1e-5,
            "d {d}: {} vs {v}",
            analytic(d)
        );
    }
}

#[test]
fn solidcraft_flags_build_the_depths_below_the_rounds() {
    for (d, faces) in [(-6.9, 10), (-7.0, 6), (-7.5, 6), (-8.0, 6), (-9.5, 6)] {
        let k = &mut Kernel::default();
        solidcraft_flags(k);
        let (mut b, ex, r) = moved(k, d);
        assert!(
            r.is_ok(),
            "d {d}: {r:?}: {:?} {}",
            ex.refusal(),
            ex.reason()
        );
        assert_eq!(ex.refusal(), OffsetFacesRefusal::None, "d {d}");
        assert!(brep_is_manifold_solid(&b).unwrap(), "d {d}");
        assert_eq!(b.face_list().len(), faces, "d {d}");
        let v = volume(k, &mut b);
        let want = analytic(d);
        assert!(
            (v - want).abs() <= 1e-6 * want,
            "d {d}: V {v} vs analytic {want}"
        );
    }
}

#[test]
fn solidcraft_flags_refuse_minus_nine_typed() {
    // #2020 stays open here: the fork builds it (FORK_VOLUME_D9)
    let k = &mut Kernel::default();
    solidcraft_flags(k);
    let (_, ex, r) = moved(k, -9.0);
    assert!(r.is_err());
    assert_eq!(
        ex.refusal(),
        OffsetFacesRefusal::NotPutBack,
        "{}",
        ex.reason()
    );
    assert!(
        ex.reason().starts_with("putting back a blend"),
        "{}",
        ex.reason()
    );
}

#[test]
#[ignore = "#2020: the fork builds d = -9; the port refuses it (see the module docs)"]
fn solidcraft_flags_build_minus_nine_as_the_fork() {
    let k = &mut Kernel::default();
    solidcraft_flags(k);
    let (mut b, ex, r) = moved(k, -9.0);
    assert!(r.is_ok(), "{r:?}: {:?} {}", ex.refusal(), ex.reason());
    assert_eq!(b.face_list().len(), 6);
    let v = volume(k, &mut b);
    assert!((v - FORK_VOLUME_D9).abs() <= 1e-9 * FORK_VOLUME_D9, "V {v}");
}

#[test]
fn flags_off_put_back_failures_are_typed() {
    // refusals A (-7.5, a corner edge) and B (-9, a rollover) of the issue
    for (d, comment) in [(-7.5, "Fillet Corner Error"), (-9.0, "Fillet Solver Error")] {
        let k = &mut Kernel::default();
        let (_, ex, r) = moved(k, d);
        assert!(r.is_err(), "d {d}");
        assert_eq!(
            ex.refusal(),
            OffsetFacesRefusal::NotPutBack,
            "d {d}: {}",
            ex.reason()
        );
        assert!(ex.reason().contains(comment), "d {d}: {}", ex.reason());
    }
}

#[test]
fn flags_off_build_the_depths_above_the_floor() {
    for (d, faces) in [(-6.9, 10), (-7.0, 6)] {
        let k = &mut Kernel::default();
        let (mut b, ex, r) = moved(k, d);
        assert!(
            r.is_ok(),
            "d {d}: {r:?}: {:?} {}",
            ex.refusal(),
            ex.reason()
        );
        assert_eq!(b.face_list().len(), faces, "d {d}");
        let v = volume(k, &mut b);
        let want = analytic(d);
        assert!(
            (v - want).abs() <= 1e-6 * want,
            "d {d}: V {v} vs analytic {want}"
        );
    }
}

/// SolidCraft's `UpstreamFixes` preset (183 flags).
fn solidcraft_flags(k: &mut Kernel) {
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
    k.fixes.perspective_sphere_offsets = true;
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
}
