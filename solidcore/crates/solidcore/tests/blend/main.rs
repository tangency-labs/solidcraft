//! solidcore's integration tests of fillets, rounds, chamfers, setbacks and blend corners, as one test binary: the files
//! in `tests/` are modules here, so the kernel is linked once per area, not once per file
//! (tangency-labs/solidcraft#14926).
//!
//! The files stay where they are and come in by `#[path]`, so their `#[path]` support
//! modules, their `include_str!`/`include_bytes!` paths and their data files read as before.
//! A test's name gains its file as a prefix: `cargo test -p solidcore --test blend <file>::`
//! runs what `--test <file>` ran.
//!
//! A new test file in `tests/` is one `#[path]` line in the main.rs of its area, in order;
//! `every_test_file_is_built` (in `tests/topology/main.rs`) fails until it is in one.

// Each file keeps the `#[path]` support modules it had as a binary of its own.
#![allow(clippy::duplicate_mod)]

#[path = "../blend_bead_section.rs"]
mod blend_bead_section;
#[path = "../blend_refusal.rs"]
mod blend_refusal;
#[path = "../brep_file_round_trip.rs"]
mod brep_file_round_trip;
#[path = "../chamfer_band_scale.rs"]
mod chamfer_band_scale;
#[path = "../chamfer_knife_edge_equal_distances.rs"]
mod chamfer_knife_edge_equal_distances;
#[path = "../chamfer_past_wall_foot_trimmed.rs"]
mod chamfer_past_wall_foot_trimmed;
#[path = "../continuity_measure.rs"]
mod continuity_measure;
#[path = "../fillet_approximation_tolerance.rs"]
mod fillet_approximation_tolerance;
#[path = "../fillet_closed_rim_rail.rs"]
mod fillet_closed_rim_rail;
#[path = "../fillet_copied_body.rs"]
mod fillet_copied_body;
#[path = "../fillet_face_owned_surface_context.rs"]
mod fillet_face_owned_surface_context;
#[path = "../fillet_law_face_owned_drop_point.rs"]
mod fillet_law_face_owned_drop_point;
#[path = "../fillet_offset_shares_face_surface.rs"]
mod fillet_offset_shares_face_surface;
#[path = "../fillet_removal_check.rs"]
mod fillet_removal_check;
#[path = "../fillet_round_failure_notes_status.rs"]
mod fillet_round_failure_notes_status;
#[path = "../fillet_rounds_meeting_at_a_knife_edge.rs"]
mod fillet_rounds_meeting_at_a_knife_edge;
#[path = "../fillet_session.rs"]
mod fillet_session;
#[path = "../fillet_variable_radius_closes_onto_vertex.rs"]
mod fillet_variable_radius_closes_onto_vertex;
#[path = "../fillet_vertex_on_split_rim.rs"]
mod fillet_vertex_on_split_rim;
#[path = "../fillet_wr300_320.rs"]
mod fillet_wr300_320;
#[path = "../fork_16bf1cb_full_turn_cover_across_seam.rs"]
mod fork_16bf1cb_full_turn_cover_across_seam;
#[path = "../fork_351c57c_stitch_nested_solid.rs"]
mod fork_351c57c_stitch_nested_solid;
#[path = "../fork_4a58f3d_round_ending_on_a_cylinder.rs"]
mod fork_4a58f3d_round_ending_on_a_cylinder;
#[path = "../fork_52bfef0_ring_fillet_removal.rs"]
mod fork_52bfef0_ring_fillet_removal;
#[path = "../fork_5b0534b_exact_fillet_removal.rs"]
mod fork_5b0534b_exact_fillet_removal;
#[path = "../fork_64e5632_variable_round_definitions.rs"]
mod fork_64e5632_variable_round_definitions;
#[path = "../fork_701da4b_small_box_variable_corner.rs"]
mod fork_701da4b_small_box_variable_corner;
#[path = "../fork_7959c0e_round_on_a_round.rs"]
mod fork_7959c0e_round_on_a_round;
#[path = "../fork_856b54e_tangent_end.rs"]
mod fork_856b54e_tangent_end;
#[path = "../fork_870c970_blend_setback.rs"]
mod fork_870c970_blend_setback;
#[path = "../fork_95bf434_rounded_corner_tight_box.rs"]
mod fork_95bf434_rounded_corner_tight_box;
#[path = "../fork_9bebf34_tangent_end_cases.rs"]
mod fork_9bebf34_tangent_end_cases;
#[path = "../fork_cac11ea_fillet_cleanup_removes_input_slits.rs"]
mod fork_cac11ea_fillet_cleanup_removes_input_slits;
#[path = "../fork_cd47efd_tangent_end_rib_sqrt_s.rs"]
mod fork_cd47efd_tangent_end_rib_sqrt_s;
#[path = "../fork_d305e69_arc_wall_into_its_tip.rs"]
mod fork_d305e69_arc_wall_into_its_tip;
#[path = "../fork_ea59e9c_section_orders.rs"]
mod fork_ea59e9c_section_orders;
#[path = "../fork_eb883ad_keep_features.rs"]
mod fork_eb883ad_keep_features;
#[path = "../fork_smlib104_setback_corner.rs"]
mod fork_smlib104_setback_corner;
#[path = "../fork_smlib104_unequal_box_corner.rs"]
mod fork_smlib104_unequal_box_corner;
#[path = "../fork_smlib125_four_round_corners.rs"]
mod fork_smlib125_four_round_corners;
#[path = "../fork_smlib125_open_setbacks.rs"]
mod fork_smlib125_open_setbacks;
#[path = "../g2_all_along.rs"]
mod g2_all_along;
#[path = "../issue_1060_offset_mitred_rounded_box_top.rs"]
mod issue_1060_offset_mitred_rounded_box_top;
#[path = "../issue_1073_opposite_rounds_cross.rs"]
mod issue_1073_opposite_rounds_cross;
#[path = "../issue_1170_corner_patch_inside.rs"]
mod issue_1170_corner_patch_inside;
#[path = "../issue_1176_fillet_cleanup_keeps_by_identity.rs"]
mod issue_1176_fillet_cleanup_keeps_by_identity;
#[path = "../issue_1376_setback_ease.rs"]
mod issue_1376_setback_ease;
#[path = "../issue_1439_mark_corner_patches.rs"]
mod issue_1439_mark_corner_patches;
#[path = "../issue_1462_fillet_attributes.rs"]
mod issue_1462_fillet_attributes;
#[path = "../issue_1474_nx1_corner_tangent_round_end.rs"]
mod issue_1474_nx1_corner_tangent_round_end;
#[path = "../issue_1475_mixed_corner_concave_side_edge.rs"]
mod issue_1475_mixed_corner_concave_side_edge;
#[path = "../issue_15059_fillet_cleanup_keeps_a_coplanar_split.rs"]
mod issue_15059_fillet_cleanup_keeps_a_coplanar_split;
#[path = "../issue_1580_middle_off_face_run_past_edge_end.rs"]
mod issue_1580_middle_off_face_run_past_edge_end;
#[path = "../issue_16686_fillet_surface_not_skinned.rs"]
mod issue_16686_fillet_surface_not_skinned;
#[path = "../issue_16735_volume_moves_are_the_old_leak.rs"]
mod issue_16735_volume_moves_are_the_old_leak;
#[path = "../issue_17633_crease_along_edge_geometry.rs"]
mod issue_17633_crease_along_edge_geometry;
#[path = "../issue_1945_fork_1207_step4.rs"]
mod issue_1945_fork_1207_step4;
#[path = "../issue_1980_cliff_overflow_edge_attributes.rs"]
mod issue_1980_cliff_overflow_edge_attributes;
#[path = "../issue_1981_pocket_chamfer_operand_trims.rs"]
mod issue_1981_pocket_chamfer_operand_trims;
#[path = "../issue_1999_shallow_crease_round_contacts.rs"]
mod issue_1999_shallow_crease_round_contacts;
#[path = "../issue_2020_mitred_put_back_trimmed_at_creases.rs"]
mod issue_2020_mitred_put_back_trimmed_at_creases;
#[path = "../issue_2088_chamfer_second_offsets.rs"]
mod issue_2088_chamfer_second_offsets;
#[path = "../issue_2089_tangent_tracer_crease_round.rs"]
mod issue_2089_tangent_tracer_crease_round;
#[path = "../issue_2109_cylinder_plane_section_trims.rs"]
mod issue_2109_cylinder_plane_section_trims;
#[path = "../issue_2175_chamfer_rail_past_tangent_rim_end.rs"]
mod issue_2175_chamfer_rail_past_tangent_rim_end;
#[path = "../issue_2176_round_of_arc_between_tangent_straights.rs"]
mod issue_2176_round_of_arc_between_tangent_straights;
#[path = "../issue_2223_section_end_trim_at_creases.rs"]
mod issue_2223_section_end_trim_at_creases;
#[path = "../issue_2260_gn_pipe_refined_range_end.rs"]
mod issue_2260_gn_pipe_refined_range_end;
#[path = "../issue_2307_peg_chamfers.rs"]
mod issue_2307_peg_chamfers;
#[path = "../issue_2307_tab_run_after_hole_rim.rs"]
mod issue_2307_tab_run_after_hole_rim;
#[path = "../issue_2314_tab_corner_arcs.rs"]
mod issue_2314_tab_corner_arcs;
#[path = "../issue_2343_curved_setback.rs"]
mod issue_2343_curved_setback;
#[path = "../issue_2349_closed_angle.rs"]
mod issue_2349_closed_angle;
#[path = "../issue_2358_mixed_end_overflow.rs"]
mod issue_2358_mixed_end_overflow;
#[path = "../issue_2360_mixed_corner.rs"]
mod issue_2360_mixed_corner;
#[path = "../issue_2364_alpha1_ends.rs"]
mod issue_2364_alpha1_ends;
#[path = "../issue_2364_round_ending_in_an_earlier_round.rs"]
mod issue_2364_round_ending_in_an_earlier_round;
#[path = "../issue_2365_nxn_setback_corners.rs"]
mod issue_2365_nxn_setback_corners;
#[path = "../issue_2368_together_r2_keeps_corner_reason.rs"]
mod issue_2368_together_r2_keeps_corner_reason;
#[path = "../issue_2369_g2_setback_corner.rs"]
mod issue_2369_g2_setback_corner;
#[path = "../issue_2370_keep_features_band_clear_of_boss.rs"]
mod issue_2370_keep_features_band_clear_of_boss;
#[path = "../issue_2371_gn_section_cancel.rs"]
mod issue_2371_gn_section_cancel;
#[path = "../issue_2385_crease_along_edge.rs"]
mod issue_2385_crease_along_edge;
#[path = "../issue_2401_refusal_sizes.rs"]
mod issue_2401_refusal_sizes;
#[path = "../issue_2403_chain_over_end_curve.rs"]
mod issue_2403_chain_over_end_curve;
#[path = "../issue_2404_build_near_size_bound.rs"]
mod issue_2404_build_near_size_bound;
#[path = "../issue_2405_blend_follow_ups.rs"]
mod issue_2405_blend_follow_ups;
#[path = "../issue_2426_round_over_a_hole.rs"]
mod issue_2426_round_over_a_hole;
#[path = "../issue_2431_851_chain_after_top_front_round.rs"]
mod issue_2431_851_chain_after_top_front_round;
#[path = "../issue_2489_blend_faces_are_definitions.rs"]
mod issue_2489_blend_faces_are_definitions;
#[path = "../issue_2656_blend_regressions.rs"]
mod issue_2656_blend_regressions;
#[path = "../issue_2704_gn_side_curve.rs"]
mod issue_2704_gn_side_curve;
#[path = "../issue_2706_point_radius_law.rs"]
mod issue_2706_point_radius_law;
#[path = "../issue_2715_refused_fillet_keeps_geometry.rs"]
mod issue_2715_refused_fillet_keeps_geometry;
#[path = "../issue_2794_planar_path_mitred_elbow.rs"]
mod issue_2794_planar_path_mitred_elbow;
#[path = "../issue_2826_round_across_seams.rs"]
mod issue_2826_round_across_seams;
#[path = "../issue_2904_setback_beside_a_chamfer.rs"]
mod issue_2904_setback_beside_a_chamfer;
#[path = "../issue_2905_spline_chamfer.rs"]
mod issue_2905_spline_chamfer;
#[path = "../issue_2907c_rim_chamfer.rs"]
mod issue_2907c_rim_chamfer;
#[path = "../issue_2933_setback_from_the_filleted_vertex.rs"]
mod issue_2933_setback_from_the_filleted_vertex;
#[path = "../issue_2972_volume_sign_enclosure.rs"]
mod issue_2972_volume_sign_enclosure;
#[path = "../issue_379_chamfer_hole_rim_closed.rs"]
mod issue_379_chamfer_hole_rim_closed;
#[path = "../issue_398_shell_box_rounds_chamfer_fillet.rs"]
mod issue_398_shell_box_rounds_chamfer_fillet;
#[path = "../issue_416_fillet_seam.rs"]
mod issue_416_fillet_seam;
#[path = "../issue_418_fillet_edge_on_tangent_seam.rs"]
mod issue_418_fillet_edge_on_tangent_seam;
#[path = "../issue_420_chamfer_on_arc.rs"]
mod issue_420_chamfer_on_arc;
#[path = "../issue_422_round_over_through_hole.rs"]
mod issue_422_round_over_through_hole;
#[path = "../issue_423_fillet_cleanup_keeps_faces.rs"]
mod issue_423_fillet_cleanup_keeps_faces;
#[path = "../issue_453_bore_rim_smooth_end.rs"]
mod issue_453_bore_rim_smooth_end;
#[path = "../issue_880_half_arc_rim_chamfer.rs"]
mod issue_880_half_arc_rim_chamfer;
#[path = "../issue_919_full_width_blend_curve.rs"]
mod issue_919_full_width_blend_curve;
#[path = "../offset_fillet2_rounded_outset.rs"]
mod offset_fillet2_rounded_outset;
#[path = "../round_end_past_side_edge.rs"]
mod round_end_past_side_edge;
#[path = "../round_past_its_wall_on_the_continued_wall.rs"]
mod round_past_its_wall_on_the_continued_wall;
#[path = "../section_blend.rs"]
mod section_blend;
#[path = "../setback_on_an_arc_is_certified.rs"]
mod setback_on_an_arc_is_certified;
#[path = "../solidcraft_1986_join_cylindrical_wall.rs"]
mod solidcraft_1986_join_cylindrical_wall;
#[path = "../variable_round_law_corner_refused.rs"]
mod variable_round_law_corner_refused;
#[path = "../wr212_refusal_keeps_owners.rs"]
mod wr212_refusal_keeps_owners;
#[path = "../wr300_variable_rounds_meet_at_law_radii.rs"]
mod wr300_variable_rounds_meet_at_law_radii;
#[path = "../wr492_mitred_cylinders.rs"]
mod wr492_mitred_cylinders;
#[path = "../wr512_singular_start_inside_direction.rs"]
mod wr512_singular_start_inside_direction;
#[path = "../wr567_revolution_round_offset_collapses.rs"]
mod wr567_revolution_round_offset_collapses;
#[path = "../wr603_trace_from_tangent_point.rs"]
mod wr603_trace_from_tangent_point;
#[path = "../wr605_ring_fillet_fixtures.rs"]
mod wr605_ring_fillet_fixtures;
#[path = "../wr631_a307480_end_section_off_its_end_face.rs"]
mod wr631_a307480_end_section_off_its_end_face;
#[path = "../wr631_cliff_overflow_choice.rs"]
mod wr631_cliff_overflow_choice;
#[path = "../wr631_cliff_overflow_construction.rs"]
mod wr631_cliff_overflow_construction;
#[path = "../wr634_mitred_edges.rs"]
mod wr634_mitred_edges;
#[path = "../wr634_overflow_first.rs"]
mod wr634_overflow_first;
#[path = "../wr635_636_cliff_overflow_closing.rs"]
mod wr635_636_cliff_overflow_closing;
#[path = "../wr636_db4a655_closed_rim_past_its_wall.rs"]
mod wr636_db4a655_closed_rim_past_its_wall;
#[path = "../wr642_mitred_concave_face_corners.rs"]
mod wr642_mitred_concave_face_corners;
#[path = "../wr643_run_on_past_far_end_only.rs"]
mod wr643_run_on_past_far_end_only;
#[path = "../wr644_mitre_gap_patch.rs"]
mod wr644_mitre_gap_patch;
#[path = "../wr646_variable_round_follows_law.rs"]
mod wr646_variable_round_follows_law;
#[path = "../wr650_tabbed_plate_mitred_rounds.rs"]
mod wr650_tabbed_plate_mitred_rounds;
#[path = "../wr655_ascii_doubles_round_trip.rs"]
mod wr655_ascii_doubles_round_trip;
#[path = "../wr665_corner_per_vertex_fan.rs"]
mod wr665_corner_per_vertex_fan;
#[path = "../wr669_exact_conic_rounds.rs"]
mod wr669_exact_conic_rounds;
#[path = "../wr669_exact_nodal_quartic_round.rs"]
mod wr669_exact_nodal_quartic_round;
#[path = "../wr669_exact_round_classes.rs"]
mod wr669_exact_round_classes;
#[path = "../wr685_gn_equal_area_per_station.rs"]
mod wr685_gn_equal_area_per_station;
#[path = "../wr685_gn_section.rs"]
mod wr685_gn_section;
#[path = "../wr685_law_corners.rs"]
mod wr685_law_corners;
#[path = "../wr685_variable_round_touch_circle.rs"]
mod wr685_variable_round_touch_circle;
#[path = "../wr695_overflow_trim_at_creases.rs"]
mod wr695_overflow_trim_at_creases;
#[path = "../wr696_ball_regular.rs"]
mod wr696_ball_regular;
#[path = "../wr696_band_cuts.rs"]
mod wr696_band_cuts;
#[path = "../wr696_default_variable_rounds.rs"]
mod wr696_default_variable_rounds;
#[path = "../wr696_fit_sizes.rs"]
mod wr696_fit_sizes;
#[path = "../wr697_ball_corner.rs"]
mod wr697_ball_corner;
#[path = "../wr697_blend_corner_turn.rs"]
mod wr697_blend_corner_turn;
#[path = "../wr697_end_and_join.rs"]
mod wr697_end_and_join;
#[path = "../wr697_four_edge_turn.rs"]
mod wr697_four_edge_turn;
#[path = "../wr698_blend_net_scope_cache.rs"]
mod wr698_blend_net_scope_cache;
#[path = "../wr698_circle_round_definition.rs"]
mod wr698_circle_round_definition;
#[path = "../wr698_circle_round_gate.rs"]
mod wr698_circle_round_gate;
#[path = "../wr698_gn_round_is_its_definition.rs"]
mod wr698_gn_round_is_its_definition;
#[path = "../wr698_procedural_blend_definition.rs"]
mod wr698_procedural_blend_definition;
#[path = "../wr698_variable_round_definition.rs"]
mod wr698_variable_round_definition;
#[path = "../wr707_round_breaking_through.rs"]
mod wr707_round_breaking_through;
#[path = "../wr707_rounds_running_off_their_faces.rs"]
mod wr707_rounds_running_off_their_faces;
#[path = "../wr708_exact_arc_sections.rs"]
mod wr708_exact_arc_sections;
#[path = "../wr_468_overhanging_boss_fork_parity.rs"]
mod wr_468_overhanging_boss_fork_parity;
#[path = "../wr_542_fillet_overlapping_bands.rs"]
mod wr_542_fillet_overlapping_bands;
#[path = "../wr_546_setback_patch_keeps_fillet_edges.rs"]
mod wr_546_setback_patch_keeps_fillet_edges;
#[path = "../wr_547_coons_patch_tangent_to_sides.rs"]
mod wr_547_coons_patch_tangent_to_sides;
#[path = "../wr_568_round_along_an_arc_span.rs"]
mod wr_568_round_along_an_arc_span;
#[path = "../wr_570_knife_edge_rounds.rs"]
mod wr_570_knife_edge_rounds;
#[path = "../wr_572_chamfer_rim_stored_as_cone.rs"]
mod wr_572_chamfer_rim_stored_as_cone;
#[path = "../wr_593_nx1_tangent_corner_edge.rs"]
mod wr_593_nx1_tangent_corner_edge;
#[path = "../wr_611_steep_fillet_trims_its_face.rs"]
mod wr_611_steep_fillet_trims_its_face;
#[path = "../wr_621_half_degree_crease_rounds.rs"]
mod wr_621_half_degree_crease_rounds;
#[path = "../wr_659_eased_corner_face_curve_halves.rs"]
mod wr_659_eased_corner_face_curve_halves;
