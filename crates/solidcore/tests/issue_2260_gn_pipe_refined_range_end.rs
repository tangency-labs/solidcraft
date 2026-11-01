//! The #2252 pipe-on-plate G2 round (r 2, order 2, w 0) by WR-685's G^n section
//! (`fillet_gn_section_generator`) on SolidCraft's flag profile (tangency-labs/solidcraft#8526
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
//! Expected volume: the tangency-labs/smlib d01d7b0 build (which has 8ea6a20) run on the
//! same construction by `SmApiBrepComputeVolume`, 40258.543608517233 at accuracy 1e-7
//! (40258.543597926051 at 1e-8, as quoted on #2260); checked to the 1e-7 accuracy the
//! volume is integrated to.
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
    "approximate_surface_bezier_starter",
    "ascii_doubles_round_trip",
    "bend_volume_bbox_contains_wide_angle_and_inf_u",
    "bend_volume_trim_param_box_across_period",
    "best_fit_line_largest_scatter_eigenvector",
    "boolean2d_plane_covers_far_faces",
    "boolean_cleanup_removes_slits",
    "boolean_lists_virtual_surface_copies",
    "boolean_new_face_closure_both_sides",
    "boolean_outer_shell_first",
    "boolean_skips_faces_apart",
    "boolean_splits_definition_across_split",
    "boolean_temporary_transform_statuses",
    "boolean_union_refuses_solid_leak",
    "boolean_vertex_by_best_crossing",
    "boolean_wire_edge_keeps_faceuse_shell",
    "box_face_failures_propagate",
    "brep_area_between_solid_and_void",
    "brep_edit_statuses_pass_on",
    "bspline_check_min_radius",
    "bspline_simple_offset_matches_parameterization",
    "bspline_trim_keeps_rational_end_spans",
    "bspline_trim_preserves_uv_spans",
    "check_loop_projected_seam_ends",
    "check_loop_vertex_tolerance",
    "check_seam_crossing_unclosed",
    "circle_offset_antiparallel_normal",
    "circle_silhouette_perspective_center",
    "circumscribe_axis_map_by_basis",
    "classify_edge_interval",
    "close_loop_relates_only_coincident_edges",
    "composite_curve_bbox_segment_intervals",
    "cone_create_canonical_preserves_inside_out_and_checks_height",
    "cone_parallel_line_domains",
    "cone_parallel_line_midpoint",
    "conic_evaluate_uses_computed_second_deriv",
    "corner_blend_coons_patch_tangent_to_sides",
    "corner_blend_stays_inside_neighbours",
    "corner_patch_face_side_twist",
    "cover_refuses_folded_continuation",
    "cover_wraps_across_closed_seam",
    "cpoly_remove_poly_face_check_member",
    "crv_in_volume_mults_by_break_index",
    "curve_bounded_surface_read_uv_type_and_implicit_outer",
    "curve_check_zero_speed_tangent_end",
    "curve_contacts_at_curve_ends",
    "curve_loop_normal_from_area",
    "curve_loop_start_after_previous_end",
    "curve_max_distance_single_sample_midpoint",
    "curve_property_gs_branch_bounds",
    "curve_sweep_caps_at_sweep_ends",
    "curve_sweep_closed_path_joins_ends",
    "curve_tight_box_interval_knots",
    "cylinder_plane_section_trims_exact",
    "cylinder_plane_tangent_within_tolerance",
    "cylinder_rebuild_mean_radius",
    "database_io_byte_swaps_read_boolean_and_u16",
    "defeature_refuses_open_base",
    "degenerate_proto_edge_in_one_vertex",
    "degen_param_tiebreak_uses_actual_param",
    "delete_vertex_joins_seam_iso_curve",
    "drop_curve_v_domain_degeneracy",
    "dupin_bottom_hyperbola_axis",
    "edge_check_orders_tangent_faces",
    "edge_reversal_keeps_failed_edge",
    "edgeuse_convexity_propagates_step_off_failure",
    "ellipse_adjust_step_interval_keeps_turns",
    "ellipse_convert_to_2d_uses_both_axes",
    "ellipse_step_interval_checks",
    "ellipse_trim_seam_lines_span_cone_height",
    "extent3d_contains_both_segment_endpoints",
    "extrusion_origin_scaled_zero",
    "extrusion_step_uses_clamped_uv",
    "face_points_span_domain",
    "face_tight_box_searches_trim",
    "fillet_3x2_mixed_concave_side_edge",
    "fillet_3x2_tangent_side_edge_roles",
    "fillet_blend_curve_revolved_on_arc",
    "fillet_chamfer_cone_on_arc",
    "fillet_chamfer_cone_stored_as_cone",
    "fillet_cleanup_keeps_target_faces",
    "fillet_cleanup_removes_input_slits",
    "fillet_cliff_overflow",
    "fillet_corner_ball_center_from_axes",
    "fillet_corner_ball_patch",
    "fillet_corner_face_curve_halves",
    "fillet_corner_per_vertex_fan",
    "fillet_corner_sphere_from_four_arcs",
    "fillet_corner_statuses",
    "fillet_corner_variable_radius_rail_on_opposite_fillet",
    "fillet_edge_curve_domain_statuses",
    "fillet_exact_round_classes",
    "fillet_half_degree_crease",
    "fillet_knife_edge_rounds",
    "fillet_mitre_concave_face_corners",
    "fillet_mitre_gap_patch",
    "fillet_nx1_end_rib_global_merge",
    "fillet_nx1_opposite_continuation_side_edge",
    "fillet_nx1_tangent_corner_edge",
    "fillet_nx1_tangent_side_edge",
    "fillet_nx1_tangent_side_edge_same_convexity",
    "fillet_patch_keeps_fillet_edges",
    "fillet_point_bigger_step_retry",
    "fillet_point_step_per_point",
    "fillet_rail_end_on_continued_surface",
    "fillet_rail_rim_on_face",
    "fillet_rails_meet_at_one_end_point",
    "fillet_rail_trim_at_near_knot",
    "fillet_refuse_ball_not_fitting",
    "fillet_refuse_crossing_rails",
    "fillet_refuse_face_consuming_blends",
    "fillet_refuse_non_manifold_result",
    "fillet_refuse_overlapping_bands",
    "fillet_refuse_rails_leaving_faces",
    "fillet_removal_takes_ring_fillets",
    "fillet_retry_radius_as_first",
    "fillet_round_ends_at_tangent_continuation",
    "fillet_run_on_bspline_trims",
    "fillet_run_on_past_far_end_only",
    "fillet_setback_from_start_vertex",
    "fillet_split_edge_keeps_near_half",
    "fillet_statuses_pass_on",
    "fillet_tangent_point_one_sided_step",
    "fillet_through_extended_rail_end",
    "fillet_torus_center_on_axis",
    "fillet_torus_rail_on_axis",
    "fillet_torus_span_from_start_section",
    "fillet_trace_from_tangent_point_toward_next",
    "fillet_trace_reaches_rails",
    "fillet_trace_rejects_short_step",
    "fillet_trim_inside_by_best_dot",
    "fillet_trim_status",
    "fillet_variable_span_follows_fillet",
    "fillet_vertex_error_diagnostics",
    "flat_corners_test_bits",
    "found_torus_sphere_keep_tested_nurb",
    "gap_array_append_updates_longest",
    "gap_samples_non_bspline_v_stride",
    "gap_samples_use_trimmed_spans",
    "geometric_g2_continuity",
    "global_solver_identical_prefers_larger_when_maximizing",
    "glue_vertices_joins_every_region",
    "good_drop_result_distances_in_order",
    "hash_table_grow_preserves_removed_records",
    "heal_stage3_failures_propagate",
    "imprint_extends_edge_to_proto_vertex",
    "imprint_extension_keeps_analytic_curve",
    "imprint_reuses_coincident_face_edge",
    "imprint_skips_collapsed_definition",
    "imprint_trims_edge_to_proto_vertex",
    "integrator_propagates_failure",
    "internal_pole_row_through_knot",
    "is_iso_curve_at_propagates_canonical_failure",
    "iso_curve_sliver_domain_is_degenerate",
    "iso_curve_uv_line_both_params",
    "is_point_on_curve_propagates_eval_failure",
    "is_topological_vertex_rejects_cusps_and_tangent_errors",
    "join_touching_loops",
    "keep_operand_uv_trims",
    "knot_vector_valid_requires_span",
    "linear_sweep_arc_is_cylinder",
    "linear_sweep_arc_to_cylinder_zero",
    "local_operation_box_holds_new_surfaces",
    "local_operation_closes_walls_to_nothing",
    "local_operation_offset_status",
    "local_operation_refuses_loose_tangent_edges",
    "local_operation_step_walls",
    "local_solve1d_unbounded_newton_step",
    "local_surface_intersect_guards_max_boundary_retry",
    "loop_orientation_floor_from_loop_box",
    "loops_go_on_round_the_vertex",
    "loop_use_reports_open_gaps",
    "mark_lock_pair_advances_and_locks_second_context",
    "masked_array_uses_bit_shift",
    "mirror_curve_keeps_euclidean_control_points",
    "new_edge_vertex_tolerance_covers_gap",
    "nurb_cylinder_inside_out_frame",
    "offset_analytic_mate_edges",
    "offset_clip_right_side_out",
    "offset_closed_curve_trim_arc",
    "offset_closes_loop_gaps",
    "offset_collapsed_rounds_mated",
    "offset_edges_keep_source_tolerance",
    "offset_excess_faces_after_merge",
    "offset_extended_edges_mated",
    "offset_extends_cones_as_themselves",
    "offset_extension_as_itself",
    "offset_extension_from_base",
    "offset_four_face_vertex_split",
    "offset_keep_cavity_regions",
    "offset_missing_surface_fails",
    "offset_open_edge_second_trim_point",
    "offset_point_vertex_caps",
    "offset_resolve_crossing_loops",
    "offset_splits_corner_vertices",
    "offset_stitch_at_body_tolerance",
    "offset_swap_uv_extended_distance",
    "offset_tangent_gap_statuses",
    "offset_walls_touch_refused",
    "oscillating_skips_failed_guesses",
    "periodic_extent_add_empty_interval",
    "perspective_sphere_offsets",
    "planar_path_profile_in_path_frame",
    "planar_path_profile_placement_statuses",
    "planar_path_sweep_statuses",
    "planar_path_uncapped_open_profiles",
    "plane_recognition_origin_after_axes",
    "plane_reparameterize_moves_origin",
    "point_class_gap_3d_propagates_no_drop_error",
    "point_on_start_point_squared_tolerance",
    "point_set_centroid_all_blocks",
    "point_set_tight_pbox_projects_points",
    "point_solve_keeps_triangle_guess",
    "polar_box_region_to_arc_transposes_domain",
    "precise_properties_body_length_scale",
    "precise_properties_span_end_from_below",
    "precise_properties_split_at_surface_knots",
    "precise_properties_trim_base_v",
    "primitive_path_curve_statuses",
    "projected_curve_transform_when_owned",
    "proto_edge_self_loop_on_vertex_degenerate",
    "proto_edge_splits_at_captured_vertices",
    "proto_edge_split_upper_keeps_the_old_max",
    "proto_vertex_miss_is_false",
    "query_statuses_pass_on",
    "refine_cc_range_ends",
    "remove_slit_loops_where_solids_touch",
    "revolution_offset_axis_both_sides",
    "revolution_round_offset_collapses",
    "revolve_analytic_profile_faces",
    "revolve_arc_centre_near_axis_is_sphere",
    "revolve_arc_touching_axis_is_horn_torus",
    "revolve_holed_profile_void",
    "revolve_nested_shells_as_cavities",
    "revolve_splits_curves_on_axis",
    "seam_point_solutions_are_one",
    "sheet_sweep_joins_profile_wires",
    "shell_edges_on_analytics_exact",
    "shell_empty_cavity_fails",
    "shell_inner_shell_of_statuses",
    "simple_coincidence_crossing_is_point",
    "slice_coincident_faces_sheet_side",
    "sphere_from_arcs_common_centre",
    "sphere_small_circle_not_touching",
    "ssi_alignment_statuses",
    "ssi_low_angle_start_on_probation",
    "ssi_no_reverse_of_empty_trace",
    "ssi_pole_point_keeps_its_tangent",
    "ssi_proto_topology_statuses_pass_on",
    "ssi_singular_start_inside_direction",
    "ssi_split_curves_at_singular_points",
    "ssi_start_loop_trace_budget",
    "ssi_tangent_contact_points",
    "ssi_tangent_curve_tracer",
    "ssi_trace_tangent_curves_once",
    "stitch_solid_nests_cavities",
    "stop_tracking_checks_own_callback",
    "surface_continuity_checks_last_v_knot",
    "surface_parallel_to_vector_checks",
    "surface_point_solver_propagates_bbox_failure",
    "sweep_frame_on_planar_path_normal",
    "sweep_statuses_pass_on",
    "taper_pivot_nearest_corner",
    "taper_tangent_corner_planes",
    "taper_wall_closes_at_apex",
    "temporary_radius_setup_errors",
    "thicken_planar_side_walls",
    "topology_failed_evaluations_propagate",
    "torus_check_reads_v_interval",
    "torus_check_spindle_apex",
    "torus_iso_circle_axis_from_midpoint",
    "torus_keeps_inside_out",
    "torus_major_radius_from_origin",
    "torus_signed_gen_curve_interval",
    "transform_aligned_bbox_image",
    "transform_model_size_from_body_box",
    "transform_scales_each_tolerance",
    "translation_sweep_regularize",
    "trim_boundary_seam_wrap",
    "twist_simple_duw_sign",
    "uv_trim_checks_on_surface",
    "uv_trim_corner_end_points_only",
    "uv_trim_joins_drop_pieces",
    "uv_trim_reversal_on_surface",
    "validate_tolerances_fixes_reported_gaps",
    "validation_does_not_make_uv_trims",
    "volume_discontinuity_checks_vw_unaligned",
    "volume_lift_third_derivative_terms",
    "volume_representational_end_continuities",
];

/// The volume d01d7b0 gives the rounded body at accuracy 1e-7.
const FORK_VOLUME: f64 = 40_258.543_608_517_233;

fn kernel(names: &[&str]) -> Kernel {
    let mut k = Kernel::new(&KernelOptions::default());
    k.fixes = UpstreamFixes::default();
    for name in names {
        *k.fixes
            .flag_mut(name)
            .unwrap_or_else(|| panic!("no flag {name}")) = true;
    }
    k.fixes.fillet_gn_section_generator = true;
    k.fixes.fillet_surface_plane_curve_on_plane_equation = true;
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
