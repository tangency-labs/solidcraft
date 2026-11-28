//! solidcore's integration tests of curves, surfaces, NURBS, intersections and queries, as one test binary: the files
//! in `tests/` are modules here, so the kernel is linked once per area, not once per file
//! (tangency-labs/solidcraft#14926).
//!
//! The files stay where they are and come in by `#[path]`, so their `#[path]` support
//! modules, their `include_str!`/`include_bytes!` paths and their data files read as before.
//! A test's name gains its file as a prefix: `cargo test -p solidcore --test geometry <file>::`
//! runs what `--test <file>` ran.
//!
//! A new test file in `tests/` is one `#[path]` line in the main.rs of its area, in order;
//! `every_test_file_is_built` (in `tests/topology/main.rs`) fails until it is in one.

// Each file keeps the `#[path]` support modules it had as a binary of its own.
#![allow(clippy::duplicate_mod)]

#[path = "../adv_surface_intersector_get_tree.rs"]
mod adv_surface_intersector_get_tree;
#[path = "../arena_uv_trim_owner.rs"]
mod arena_uv_trim_owner;
#[path = "../assert_valid_level0_helpers.rs"]
mod assert_valid_level0_helpers;
#[path = "../brep_query_cache_lifetime.rs"]
mod brep_query_cache_lifetime;
#[path = "../check_memo_incremental.rs"]
mod check_memo_incremental;
#[path = "../constructed_transforms.rs"]
mod constructed_transforms;
#[path = "../contact_between_bodies.rs"]
mod contact_between_bodies;
#[path = "../contains_uv_point_edge_drops_work.rs"]
mod contains_uv_point_edge_drops_work;
#[path = "../curve_assert_valid.rs"]
mod curve_assert_valid;
#[path = "../curve_length_bound.rs"]
mod curve_length_bound;
#[path = "../curve_length_holds_its_accuracy.rs"]
mod curve_length_holds_its_accuracy;
#[path = "../drop_clamped_sliver.rs"]
mod drop_clamped_sliver;
#[path = "../extend_short_arc_long_rulings.rs"]
mod extend_short_arc_long_rulings;
#[path = "../face_closest_point_live_box.rs"]
mod face_closest_point_live_box;
#[path = "../face_editable_sheet.rs"]
mod face_editable_sheet;
#[path = "../far_sphere_recognised.rs"]
mod far_sphere_recognised;
#[path = "../fork_099cf96_exact_class_cases.rs"]
mod fork_099cf96_exact_class_cases;
#[path = "../fork_1279539_cone_iso_circle_at_axial_height.rs"]
mod fork_1279539_cone_iso_circle_at_axial_height;
#[path = "../fork_6eecf04_law_corner_parameters.rs"]
mod fork_6eecf04_law_corner_parameters;
#[path = "../fork_7048946_ball_curved_faces.rs"]
mod fork_7048946_ball_curved_faces;
#[path = "../fork_b518bbe_cover_wraps_seam_on_trims_only.rs"]
mod fork_b518bbe_cover_wraps_seam_on_trims_only;
#[path = "../fork_b9d6a72_equal_area_wedges.rs"]
mod fork_b9d6a72_equal_area_wedges;
#[path = "../fork_c3f8c80_ball_sphere_turned_round.rs"]
mod fork_c3f8c80_ball_sphere_turned_round;
#[path = "../fork_c9db18e_grazing_edge_crossing.rs"]
mod fork_c9db18e_grazing_edge_crossing;
#[path = "../fork_cae8618_l_box_corner_patch.rs"]
mod fork_cae8618_l_box_corner_patch;
#[path = "../fork_e36feb1_boundary_row_trim_pace.rs"]
mod fork_e36feb1_boundary_row_trim_pace;
#[path = "../heal_edge_tolerance_covers_edge_face_gap.rs"]
mod heal_edge_tolerance_covers_edge_face_gap;
#[path = "../intersect_faces_read_only.rs"]
mod intersect_faces_read_only;
#[path = "../iso_curve_continuity.rs"]
mod iso_curve_continuity;
#[path = "../issue_1054_tight_box_interval_knots.rs"]
mod issue_1054_tight_box_interval_knots;
#[path = "../issue_1059_grazing_edge_crossing_probe.rs"]
mod issue_1059_grazing_edge_crossing_probe;
#[path = "../issue_1105_trim_knot_properties.rs"]
mod issue_1105_trim_knot_properties;
#[path = "../issue_1419_brep_check.rs"]
mod issue_1419_brep_check;
#[path = "../issue_1435_precise_properties_split_at_knots.rs"]
mod issue_1435_precise_properties_split_at_knots;
#[path = "../issue_1570_near_flat_point_solve.rs"]
mod issue_1570_near_flat_point_solve;
#[path = "../issue_1584_check_brep_naming.rs"]
mod issue_1584_check_brep_naming;
#[path = "../issue_1606_lean_cylinder_is_a_cone.rs"]
mod issue_1606_lean_cylinder_is_a_cone;
#[path = "../issue_16669_pole_trace_span_certified.rs"]
mod issue_16669_pole_trace_span_certified;
#[path = "../issue_1672_torus_minus_z.rs"]
mod issue_1672_torus_minus_z;
#[path = "../issue_16735_trims_past_domain.rs"]
mod issue_16735_trims_past_domain;
#[path = "../issue_17062_ellipse_rounding_only_interval.rs"]
mod issue_17062_ellipse_rounding_only_interval;
#[path = "../issue_1767_bspline_surface_part.rs"]
mod issue_1767_bspline_surface_part;
#[path = "../issue_1799_tight_box_divides_at_knots.rs"]
mod issue_1799_tight_box_divides_at_knots;
#[path = "../issue_18325_iso_drop_exact_ends.rs"]
mod issue_18325_iso_drop_exact_ends;
#[path = "../issue_1979_ring_lip_crossing_not_coincidence.rs"]
mod issue_1979_ring_lip_crossing_not_coincidence;
#[path = "../issue_2009_group_c_inversion.rs"]
mod issue_2009_group_c_inversion;
#[path = "../issue_2015_helpers.rs"]
mod issue_2015_helpers;
// Public, as when it was a crate root: its `pub use` paths are what it checks, and nothing
// here uses them.
#[path = "../issue_2015_public_paths.rs"]
pub mod issue_2015_public_paths;
#[path = "../issue_2050_crossing_rails_query_no_solution.rs"]
mod issue_2050_crossing_rails_query_no_solution;
#[path = "../issue_2091_precise_volume_spans.rs"]
mod issue_2091_precise_volume_spans;
#[path = "../issue_2136_cooperative_cancellation.rs"]
mod issue_2136_cooperative_cancellation;
#[path = "../issue_2313_unbounded_cone_iso_circle.rs"]
mod issue_2313_unbounded_cone_iso_circle;
#[path = "../issue_2356_smooth_interior.rs"]
mod issue_2356_smooth_interior;
#[path = "../issue_2393_face_query_coordinate_bound.rs"]
mod issue_2393_face_query_coordinate_bound;
#[path = "../issue_2479_pcurve_ends_in_domain.rs"]
mod issue_2479_pcurve_ends_in_domain;
#[path = "../issue_2574_straight_trim_min_radius.rs"]
mod issue_2574_straight_trim_min_radius;
#[path = "../issue_2652_ball_on_a_bspline_round.rs"]
mod issue_2652_ball_on_a_bspline_round;
#[path = "../issue_2656_point_solve_at_interval_end.rs"]
mod issue_2656_point_solve_at_interval_end;
#[path = "../issue_2690_uv_bound_stretched_face.rs"]
mod issue_2690_uv_bound_stretched_face;
#[path = "../issue_2867_uv_line_end_past_coordinate_bound.rs"]
mod issue_2867_uv_line_end_past_coordinate_bound;
#[path = "../issue_2928_cone_whole_turn_window.rs"]
mod issue_2928_cone_whole_turn_window;
#[path = "../issue_383_tangent_torus_volume.rs"]
mod issue_383_tangent_torus_volume;
#[path = "../issue_392_tangent_twice.rs"]
mod issue_392_tangent_twice;
#[path = "../issue_394_ssi_tangent_contact_points.rs"]
mod issue_394_ssi_tangent_contact_points;
#[path = "../issue_395_tangential_contact_corpus.rs"]
mod issue_395_tangential_contact_corpus;
#[path = "../issue_396_probation_start_points.rs"]
mod issue_396_probation_start_points;
#[path = "../issue_396_small_loops.rs"]
mod issue_396_small_loops;
#[path = "../issue_417_torus_center_on_axis.rs"]
mod issue_417_torus_center_on_axis;
#[path = "../issue_423_sphere_cut_round_edge_check.rs"]
mod issue_423_sphere_cut_round_edge_check;
#[path = "../keepuv_intersection_pcurves.rs"]
mod keepuv_intersection_pcurves;
#[path = "../lazy_analytic_nurb.rs"]
mod lazy_analytic_nurb;
#[path = "../lazy_generic_surface_domain.rs"]
mod lazy_generic_surface_domain;
#[path = "../max_solve_branches_tiny_torus.rs"]
mod max_solve_branches_tiny_torus;
#[path = "../net_ties.rs"]
mod net_ties;
#[path = "../parity_variable_radius_corner_not_analytic.rs"]
mod parity_variable_radius_corner_not_analytic;
#[path = "../primitive_infinite_region_searches.rs"]
mod primitive_infinite_region_searches;
#[path = "../primitives_solid.rs"]
mod primitives_solid;
#[path = "../public_copy_transforms.rs"]
mod public_copy_transforms;
#[path = "../public_scale_ownership.rs"]
mod public_scale_ownership;
#[path = "../query_normal_definition.rs"]
mod query_normal_definition;
#[path = "../recognise_analytic.rs"]
mod recognise_analytic;
#[path = "../section_frames.rs"]
mod section_frames;
#[path = "../sphere_drop_spans_certified.rs"]
mod sphere_drop_spans_certified;
#[path = "../subdivide_face_v_knots.rs"]
mod subdivide_face_v_knots;
#[path = "../tangent_edge.rs"]
mod tangent_edge;
#[path = "../tight_box_planar_face_edges.rs"]
mod tight_box_planar_face_edges;
#[path = "../trim_boundary_seam_wrap.rs"]
mod trim_boundary_seam_wrap;
#[path = "../ub_285_nurb_fillet_shell.rs"]
mod ub_285_nurb_fillet_shell;
#[path = "../unstitch_faces.rs"]
mod unstitch_faces;
#[path = "../uv_trim_owner_default_naming.rs"]
mod uv_trim_owner_default_naming;
#[path = "../wr211_ellipse_step_interval_full_turn.rs"]
mod wr211_ellipse_step_interval_full_turn;
#[path = "../wr263_public_uv_domain_failed_trim_evaluation.rs"]
mod wr263_public_uv_domain_failed_trim_evaluation;
#[path = "../wr266_public_heal_stage3_failure_uv_trims.rs"]
mod wr266_public_heal_stage3_failure_uv_trims;
#[path = "../wr3_segment_box_containment.rs"]
mod wr3_segment_box_containment;
#[path = "../wr406_uv_trim_reversal_check.rs"]
mod wr406_uv_trim_reversal_check;
#[path = "../wr46_twist_simple_duw.rs"]
mod wr46_twist_simple_duw;
#[path = "../wr493_class_proofs.rs"]
mod wr493_class_proofs;
#[path = "../wr493_corner_sphere_polar_domain.rs"]
mod wr493_corner_sphere_polar_domain;
#[path = "../wr501_torus_major_radius_from_origin.rs"]
mod wr501_torus_major_radius_from_origin;
#[path = "../wr507_508_transform_matrix_tolerances.rs"]
mod wr507_508_transform_matrix_tolerances;
#[path = "../wr592_torus_rail_on_axis.rs"]
mod wr592_torus_rail_on_axis;
#[path = "../wr598_delete_vertex_seam_iso_curve.rs"]
mod wr598_delete_vertex_seam_iso_curve;
#[path = "../wr682_surface_plane_curve.rs"]
mod wr682_surface_plane_curve;
#[path = "../wr697_ball_patch.rs"]
mod wr697_ball_patch;
#[path = "../wr700_shrink_keeps_exact_trims.rs"]
mod wr700_shrink_keeps_exact_trims;
#[path = "../wr701_swap_uv_keeps_analytic_definition.rs"]
mod wr701_swap_uv_keeps_analytic_definition;
#[path = "../wr713_cone_root_fixes.rs"]
mod wr713_cone_root_fixes;
#[path = "../wr75_ssi_tangent_curves.rs"]
mod wr75_ssi_tangent_curves;
#[path = "../wr_410_best_crossing_offset_vertices.rs"]
mod wr_410_best_crossing_offset_vertices;
#[path = "../wr_550_projection_through_sphere_poles.rs"]
mod wr_550_projection_through_sphere_poles;
#[path = "../wr_614_coincident_strip_crossing.rs"]
mod wr_614_coincident_strip_crossing;
#[path = "../wr_647_concentric_arcs_nearest_point.rs"]
mod wr_647_concentric_arcs_nearest_point;
#[path = "../wr_80_ssi_empty_trace_not_reversed.rs"]
mod wr_80_ssi_empty_trace_not_reversed;
