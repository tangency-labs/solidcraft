//! #1060 (WR-615, fork tangency-labs/smlib a96ce9f; SolidCraft rig 4492): mitred rounds
//! on a moved face follow it together. A 40 x 30 x 10 box has its four top edges rounded
//! r 3 (mitred where they meet at the top corners), and the top face is offset by +19
//! (`OffsetFacesExecutive::offset_faces`). The four rounds form one group that comes off
//! at once through `SmFeatureExecutive` (`SM_RB_EXTEND_SURFACE`), because every face
//! round it is a plane. The top then moves to z 29 and the rounds are put back.
//!
//! The fork (2fc375f) gives 10 faces, 20 edges and 12 vertices, with volume
//! 34539.95627294846 and area 6274.637126053606. Before the group removal was ported,
//! the move was refused as `OffsetFacesRefusal::Unsupported`.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::blend::fillet_executive::api::fillet_edges;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::mass::integrator::properties::{brep_area, brep_volume, PropertyAttributes};
use solidcore::math::Point3;
use solidcore::offset::offset_faces::OffsetFacesExecutive;
use solidcore::primitive::api_primitives::create_box;
use solidcore::query::api_queries::brep_is_manifold_solid;
use solidcore::surface::plane::Plane;
use solidcore::topology::face::p2 as face2;
use solidcore::topology::{edge, vertex, Brep, EdgeId, FaceId};
use solidcore::Kernel;

/// The fork's volume and area (2fc375f).
const FORK_VOLUME: f64 = 34539.95627294846;
const FORK_AREA: f64 = 6274.637126053606;

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

/// The rig: the rounded box with its top moved +19.
fn rounded_box_top_up(k: &mut Kernel) -> Brep {
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
    let r = ex.offset_faces(k, &mut b, &[t], 19.0);
    assert!(r.is_ok(), "{r:?}: {:?} {}", ex.refusal(), ex.reason());
    assert_eq!(ex.followed_blends(), 4);
    b
}

fn check(k: &mut Kernel) {
    let mut b = rounded_box_top_up(k);
    assert!(brep_is_manifold_solid(&b).unwrap());
    assert_eq!(b.face_list().len(), 10);
    assert_eq!(b.edges().ids().count(), 20);
    assert_eq!(b.vertices().ids().count(), 12);
    // the moved top: a plane at z 29
    let _ = top(&b, 29.0);
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    let v = brep_volume(k, &mut b, 1e-9, &attrs).unwrap();
    let a = brep_area(k, &mut b, 1e-9).unwrap();
    assert!(
        (v - FORK_VOLUME).abs() <= 1e-9 * FORK_VOLUME,
        "volume {v}, fork {FORK_VOLUME}"
    );
    assert!(
        (a - FORK_AREA).abs() <= 1e-9 * FORK_AREA,
        "area {a}, fork {FORK_AREA}"
    );
}

#[test]
fn mitred_rounds_follow_the_moved_top() {
    check(&mut Kernel::default());
}

/// SolidCraft's flag preset for the rig (148 flags).
#[test]
fn mitred_rounds_follow_the_moved_top_under_solidcraft_preset() {
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
    k.fixes.boolean_vertex_by_best_crossing = true;
    k.fixes.boolean_wire_edge_keeps_faceuse_shell = true;
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
    k.fixes.curve_contacts_at_curve_ends = true;
    k.fixes.curve_sweep_caps_at_sweep_ends = true;
    k.fixes.curve_tight_box_interval_knots = true;
    k.fixes.cylinder_plane_tangent_within_tolerance = true;
    k.fixes.cylinder_rebuild_mean_radius = true;
    k.fixes.degenerate_proto_edge_in_one_vertex = true;
    k.fixes.edge_check_orders_tangent_faces = true;
    k.fixes.edge_reversal_keeps_failed_edge = true;
    k.fixes.ellipse_trim_seam_lines_span_cone_height = true;
    k.fixes.extrusion_origin_scaled_zero = true;
    k.fixes.fillet_3x2_mixed_concave_side_edge = true;
    k.fixes.fillet_chamfer_cone_on_arc = true;
    k.fixes.fillet_chamfer_cone_stored_as_cone = true;
    k.fixes.fillet_cleanup_keeps_target_faces = true;
    k.fixes.fillet_corner_ball_center_from_axes = true;
    k.fixes.fillet_corner_ball_patch = true;
    k.fixes.fillet_corner_sphere_from_four_arcs = true;
    k.fixes.fillet_knife_edge_rounds = true;
    k.fixes.fillet_nx1_end_rib_global_merge = true;
    k.fixes.fillet_nx1_opposite_continuation_side_edge = true;
    k.fixes.fillet_nx1_tangent_corner_edge = true;
    k.fixes.fillet_nx1_tangent_side_edge = true;
    k.fixes.fillet_patch_keeps_fillet_edges = true;
    k.fixes.fillet_point_bigger_step_retry = true;
    k.fixes.fillet_point_step_per_point = true;
    k.fixes.fillet_rail_rim_on_face = true;
    k.fixes.fillet_refuse_ball_not_fitting = true;
    k.fixes.fillet_refuse_crossing_rails = true;
    k.fixes.fillet_refuse_face_consuming_blends = true;
    k.fixes.fillet_refuse_non_manifold_result = true;
    k.fixes.fillet_refuse_overlapping_bands = true;
    k.fixes.fillet_refuse_rails_leaving_faces = true;
    k.fixes.fillet_retry_radius_as_first = true;
    k.fixes.fillet_setback_from_start_vertex = true;
    k.fixes.fillet_through_extended_rail_end = true;
    k.fixes.fillet_torus_center_on_axis = true;
    k.fixes.fillet_torus_rail_on_axis = true;
    k.fixes.fillet_torus_span_from_start_section = true;
    k.fixes.fillet_trace_rejects_short_step = true;
    k.fixes.fillet_vertex_error_diagnostics = true;
    k.fixes.flat_corners_test_bits = true;
    k.fixes.found_torus_sphere_keep_tested_nurb = true;
    k.fixes.geometric_g2_continuity = true;
    k.fixes.heal_stage3_failures_propagate = true;
    k.fixes.imprint_extends_edge_to_proto_vertex = true;
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
    k.fixes.offset_extension_from_base = true;
    k.fixes.offset_missing_surface_fails = true;
    k.fixes.offset_point_vertex_caps = true;
    k.fixes.offset_resolve_crossing_loops = true;
    k.fixes.offset_stitch_at_body_tolerance = true;
    k.fixes.offset_walls_touch_refused = true;
    k.fixes.oscillating_skips_failed_guesses = true;
    k.fixes.planar_path_uncapped_open_profiles = true;
    k.fixes.point_solve_keeps_triangle_guess = true;
    k.fixes.precise_properties_body_length_scale = true;
    k.fixes.precise_properties_split_at_surface_knots = true;
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
    k.fixes.seam_point_solutions_are_one = true;
    k.fixes.sheet_sweep_joins_profile_wires = true;
    k.fixes.shell_edges_on_analytics_exact = true;
    k.fixes.shell_empty_cavity_fails = true;
    k.fixes.shell_inner_shell_of_statuses = true;
    k.fixes.slice_coincident_faces_sheet_side = true;
    k.fixes.sphere_small_circle_not_touching = true;
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
    check(&mut k);
}
