//! The #2252 pipe-on-plate G2 round (r 2, order 2, w 0) by WR-685's G^n section on
//! SolidCraft's flag profile (tangency-labs/solidcraft#8526
//! at c494a18, `crates/smlib/src/core.rs` `fixes()`, 292 flags), reported on #2260.
//!
//! With `query_statuses_pass_on` (tangency-labs/smlib 8311f8e) `sm_RefineCCSolutions`
//! passed on `Inversion`'s `SM_ERR` for a curve/curve range solution's end refined
//! 2e-15 past the range (12.149466823226815 against the range's max 12.149466823226813,
//! while the round's insertion classifies a curve on a face), so the fillet failed with
//! 1001. The body the boolean builds with `keep_operand_uv_trims` and the offset extension
//! flags is what puts an end there. The fork's later 8ea6a20 takes such a parameter as
//! the answer it is and leaves an end alone only for a range with no fraction; solidcore
//! now does that under the same flag (#2264).
//!
//! Expected volume: the tangency-labs/smlib 729f893 build run on the same construction by
//! `SmApiBrepComputeVolume`, 40267.709270590589 at accuracy 1e-7 (3bc54a4 and c596548
//! give the same), checked to the 1e-7 accuracy the volume is integrated to.
//!
//! The round is the plate's and the pipe's section swept along x, so its volume is the
//! union's, `24000 + 60 (100 pi - (100 acos 0.6 - 48)) = 40165.784613529`, plus 60 times the
//! section's area. The closed form G^n section (n 2, w 0, ball 2; as `gn_section` in
//! `fork_ea59e9c_section_orders.rs`), integrated over its plane, gives:
//! - with the jets since smlib#79's 3bc54a4 (WR-685: the order-n jet is the normal part,
//!   0 on the plate and `-|c1|^2 / (2 R)` radially on the pipe), area 1.698743952036 and
//!   volume 40267.7092506513, which the fork's value matches to 5e-10 and solidcore's to
//!   3e-9;
//! - with the feet's Taylor jets before it, area 1.545983045903 and volume 40258.5435962833.
//!   That was the d01d7b0 build's 40258.543608517233, the value this test used to expect.
//!
//! Both sections run from the ball's feet (31.3137, 10) to (29.4281, 12.6667) and meet
//! the plate and the pipe G2 (end curvatures 0 and 1/R); they differ by a tangential
//! term at order 2.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // test helpers

use solidcore::blend::fillet_executive::api::fillet_edges;
use solidcore::boolean::api_brep::boolean_union;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::kernel::{KernelOptions, UpstreamFixes};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::{Point3, Vec3};
use solidcore::primitive::api_primitives::{create_box, create_cylinder};
use solidcore::query::api_queries::edge_closest_point;
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::Kernel;

/// The flags SolidCraft's `fixes()` turns on at tangency-labs/solidcraft c494a18.
const SOLIDCRAFT_C494A18: &[&str] = &[
    "analytic_copy_keeps_parameterization",
    "analytic_corner_failure_is_false",
    "analytic_inversion_answers_apart",
    "ascii_doubles_round_trip",
    "boolean2d_plane_covers_far_faces",
    "boolean_cleanup_removes_slits",
    "boolean_new_face_closure_both_sides",
    "boolean_outer_shell_first",
    "boolean_skips_faces_apart",
    "boolean_splits_definition_across_split",
    "boolean_union_refuses_solid_leak",
    "boolean_vertex_by_best_crossing",
    "box_face_failures_propagate",
    "brep_edit_statuses_pass_on",
    "bspline_trim_keeps_rational_end_spans",
    "bspline_trim_preserves_uv_spans",
    "check_loop_projected_seam_ends",
    "check_loop_vertex_tolerance",
    "check_seam_crossing_unclosed",
    "classify_edge_interval",
    "close_loop_relates_only_coincident_edges",
    "cone_create_canonical_preserves_inside_out_and_checks_height",
    "corner_patch_face_side_twist",
    "cover_refuses_folded_continuation",
    "cover_wraps_across_closed_seam",
    "curve_bounded_surface_read_uv_type_and_implicit_outer",
    "curve_contacts_at_curve_ends",
    "curve_loop_normal_from_area",
    "curve_loop_start_after_previous_end",
    "curve_sweep_closed_path_joins_ends",
    "cylinder_rebuild_mean_radius",
    "database_io_byte_swaps_read_boolean_and_u16",
    "degenerate_proto_edge_in_one_vertex",
    "degen_param_tiebreak_uses_actual_param",
    "delete_vertex_joins_seam_iso_curve",
    "drop_curve_v_domain_degeneracy",
    "dupin_bottom_hyperbola_axis",
    "edge_check_orders_tangent_faces",
    "edgeuse_convexity_propagates_step_off_failure",
    "ellipse_step_interval_checks",
    "ellipse_trim_seam_lines_span_cone_height",
    "extent3d_contains_both_segment_endpoints",
    "extrusion_origin_scaled_zero",
    "face_points_span_domain",
    "face_tight_box_searches_trim",
    "fillet_3x2_mixed_concave_side_edge",
    "fillet_3x2_tangent_side_edge_roles",
    "fillet_chamfer_cone_on_arc",
    "fillet_chamfer_cone_stored_as_cone",
    "fillet_cleanup_keeps_target_faces",
    "fillet_cliff_overflow",
    "fillet_corner_ball_center_from_axes",
    "fillet_corner_ball_patch",
    "fillet_corner_per_vertex_fan",
    "fillet_corner_variable_radius_rail_on_opposite_fillet",
    "fillet_exact_round_classes",
    "fillet_half_degree_crease",
    "fillet_knife_edge_rounds",
    "fillet_mitre_concave_face_corners",
    "fillet_mitre_gap_patch",
    "fillet_nx1_end_rib_global_merge",
    "fillet_nx1_opposite_continuation_side_edge",
    "fillet_nx1_tangent_corner_edge",
    "fillet_nx1_tangent_side_edge",
    "fillet_patch_keeps_fillet_edges",
    "fillet_point_step_per_point",
    "fillet_rail_end_on_continued_surface",
    "fillet_rails_meet_at_one_end_point",
    "fillet_rail_trim_at_near_knot",
    "fillet_refuse_face_consuming_blends",
    "fillet_refuse_overlapping_bands",
    "fillet_refuse_rails_leaving_faces",
    "fillet_removal_takes_ring_fillets",
    "fillet_round_ends_at_tangent_continuation",
    "fillet_run_on_bspline_trims",
    "fillet_run_on_past_far_end_only",
    "fillet_split_edge_keeps_near_half",
    "fillet_statuses_pass_on",
    "fillet_trace_reaches_rails",
    "fillet_trim_inside_by_best_dot",
    "fillet_trim_status",
    "fillet_variable_span_follows_fillet",
    "fillet_vertex_error_diagnostics",
    "flat_corners_test_bits",
    "found_torus_sphere_keep_tested_nurb",
    "geometric_g2_continuity",
    "glue_vertices_joins_every_region",
    "good_drop_result_distances_in_order",
    "imprint_extends_edge_to_proto_vertex",
    "imprint_reuses_coincident_face_edge",
    "internal_pole_row_through_knot",
    "join_touching_loops",
    "keep_operand_uv_trims",
    "linear_sweep_arc_is_cylinder",
    "local_operation_box_holds_new_surfaces",
    "local_operation_closes_walls_to_nothing",
    "local_operation_refuses_loose_tangent_edges",
    "local_operation_step_walls",
    "local_surface_intersect_guards_max_boundary_retry",
    "loops_go_on_round_the_vertex",
    "loop_use_reports_open_gaps",
    "mirror_curve_keeps_euclidean_control_points",
    "new_edge_vertex_tolerance_covers_gap",
    "nurb_cylinder_inside_out_frame",
    "offset_analytic_mate_edges",
    "offset_clip_right_side_out",
    "offset_collapsed_rounds_mated",
    "offset_edges_keep_source_tolerance",
    "offset_excess_faces_after_merge",
    "offset_extended_edges_mated",
    "offset_extension_as_itself",
    "offset_extension_from_base",
    "offset_keep_cavity_regions",
    "offset_missing_surface_fails",
    "offset_open_edge_second_trim_point",
    "offset_point_vertex_caps",
    "offset_resolve_crossing_loops",
    "offset_stitch_at_body_tolerance",
    "offset_swap_uv_extended_distance",
    "offset_walls_touch_refused",
    "oscillating_skips_failed_guesses",
    "planar_path_profile_placement_statuses",
    "planar_path_sweep_statuses",
    "plane_recognition_origin_after_axes",
    "plane_reparameterize_moves_origin",
    "point_class_gap_3d_propagates_no_drop_error",
    "point_on_start_point_squared_tolerance",
    "point_solve_keeps_triangle_guess",
    "proto_edge_self_loop_on_vertex_degenerate",
    "proto_edge_splits_at_captured_vertices",
    "proto_edge_split_upper_keeps_the_old_max",
    "proto_vertex_miss_is_false",
    "query_statuses_pass_on",
    "refine_cc_range_ends",
    "revolution_round_offset_collapses",
    "revolve_analytic_profile_faces",
    "revolve_holed_profile_void",
    "revolve_nested_shells_as_cavities",
    "revolve_splits_curves_on_axis",
    "shell_edges_on_analytics_exact",
    "shell_empty_cavity_fails",
    "simple_coincidence_crossing_is_point",
    "slice_coincident_faces_sheet_side",
    "ssi_low_angle_start_on_probation",
    "ssi_pole_point_keeps_its_tangent",
    "ssi_proto_topology_statuses_pass_on",
    "ssi_singular_start_inside_direction",
    "ssi_split_curves_at_singular_points",
    "ssi_tangent_contact_points",
    "ssi_tangent_curve_tracer",
    "ssi_trace_tangent_curves_once",
    "surface_continuity_checks_last_v_knot",
    "surface_parallel_to_vector_checks",
    "sweep_statuses_pass_on",
    "taper_tangent_corner_planes",
    "temporary_radius_setup_errors",
    "thicken_planar_side_walls",
    "torus_check_spindle_apex",
    "torus_iso_circle_axis_from_midpoint",
    "torus_keeps_inside_out",
    "torus_major_radius_from_origin",
    "torus_signed_gen_curve_interval",
    "transform_model_size_from_body_box",
    "transform_scales_each_tolerance",
    "twist_simple_duw_sign",
    "uv_trim_corner_end_points_only",
    "validate_tolerances_fixes_reported_gaps",
    "validation_does_not_make_uv_trims",
    "volume_lift_third_derivative_terms",
    "volume_representational_end_continuities",
];

/// The volume 729f893 gives the rounded body at accuracy 1e-7.
const FORK_VOLUME: f64 = 40_267.709_270_590_589;

fn kernel(names: &[&str]) -> Kernel {
    let mut k = Kernel::new(&KernelOptions::default());
    k.fixes = UpstreamFixes::default();
    for name in names {
        *k.fixes
            .flag_mut(name)
            .unwrap_or_else(|| panic!("no flag {name}")) = true;
    }
    k
}

/// Unites the pipe and the plate, rounds the root line at (30, 28, 10) and returns the
/// result with the body.
fn round_pipe_root(k: &mut Kernel) -> (solidcore::Result<()>, solidcore::Brep) {
    let plate = create_box(k, Point3::new(0.0, 0.0, 0.0), 60.0, 40.0, 10.0).unwrap();
    let mut pipe = create_cylinder(k, Point3::new(0.0, 0.0, 0.0), 10.0, 60.0).unwrap();
    let pl = Axis2Placement::new(
        Point3::new(0.0, 20.0, 16.0),
        Vec3::new(0.0, 0.0, -1.0),
        Vec3::new(0.0, 1.0, 0.0),
    );
    solidcore::topology::brep::p5::transform(k, &mut pipe, &pl, None, false).unwrap();
    let mut body = boolean_union(k, plate, pipe).unwrap();
    let q = Point3::new(30.0, 28.0, 10.0);
    let mut best = (0.1_f64, None);
    for &e in body.edge_list() {
        if let Ok(c) = edge_closest_point(k, &body, e, q) {
            if c.closest.distance <= best.0 {
                best = (c.closest.distance, Some(e));
            }
        }
    }
    let e = best.1.expect("root edge");
    let r = fillet_edges(k, &mut body, &[e], 2.0, 2, 2, 0.0);
    (r, body)
}

fn check_round(names: &[&str]) {
    let mut k = kernel(names);
    let (r, mut body) = round_pipe_root(&mut k);
    assert!(r.is_ok(), "pipe G2 round failed: {r:?}");
    let report = check_brep(&mut k, &mut body, CheckLevel::Level2)
        .unwrap()
        .lines();
    assert!(report.is_empty(), "{report:?}");
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    let v = brep_volume(&k, &mut body, 1e-7, &attrs).unwrap();
    eprintln!("pipe root volume {v:.17e}");
    assert!(
        (v - FORK_VOLUME).abs() <= 1e-7 * FORK_VOLUME,
        "{v} want {FORK_VOLUME}"
    );
}

/// SolidCraft's whole profile plus the G^n section: the round completes with the fork's
/// volume.
#[test]
fn solidcraft_profile_pipe_gn_round_completes() {
    check_round(SOLIDCRAFT_C494A18);
}

/// The four flags the failure needed together (`keep_operand_uv_trims`,
/// `query_statuses_pass_on`, `offset_extension_from_base`, `offset_extension_as_itself`).
#[test]
fn reduced_profile_pipe_gn_round_completes() {
    check_round(&[
        "keep_operand_uv_trims",
        "query_statuses_pass_on",
        "offset_extension_from_base",
        "offset_extension_as_itself",
    ]);
}

/// Without `query_statuses_pass_on` the status is dropped, as upstream, and the round
/// completes with the same volume.
#[test]
fn without_pass_on_pipe_gn_round_completes() {
    check_round(&[
        "keep_operand_uv_trims",
        "offset_extension_from_base",
        "offset_extension_as_itself",
    ]);
}
