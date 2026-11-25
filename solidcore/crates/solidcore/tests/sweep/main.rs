//! solidcore's integration tests of sweeps, extrusions, revolves, lofts and drafts, as one test binary: the files
//! in `tests/` are modules here, so the kernel is linked once per area, not once per file
//! (tangency-labs/solidcraft#14926).
//!
//! The files stay where they are and come in by `#[path]`, so their `#[path]` support
//! modules, their `include_str!`/`include_bytes!` paths and their data files read as before.
//! A test's name gains its file as a prefix: `cargo test -p solidcore --test sweep <file>::`
//! runs what `--test <file>` ran.
//!
//! A new test file in `tests/` is one `#[path]` line in the main.rs of its area, in order;
//! `every_test_file_is_built` (in `tests/topology/main.rs`) fails until it is in one.

// Each file keeps the `#[path]` support modules it had as a binary of its own.
#![allow(clippy::duplicate_mod)]

#[path = "../curve_cache_memo.rs"]
mod curve_cache_memo;
#[path = "../curve_sweep_from_faces_box.rs"]
mod curve_sweep_from_faces_box;
#[path = "../derive_revolved_supports.rs"]
mod derive_revolved_supports;
#[path = "../fork_3ae7bfa_drafted_cone_wall.rs"]
mod fork_3ae7bfa_drafted_cone_wall;
#[path = "../fork_5df5b27_ring_of_arcs.rs"]
mod fork_5df5b27_ring_of_arcs;
#[path = "../general_path_sweep.rs"]
mod general_path_sweep;
#[path = "../hang_51_extrusion_old_offset.rs"]
mod hang_51_extrusion_old_offset;
#[path = "../hang_taper_extrude_past_centre.rs"]
mod hang_taper_extrude_past_centre;
#[path = "../hang_unify_normals_samples.rs"]
mod hang_unify_normals_samples;
#[path = "../issue_1024_rail_through_boundary_vertex.rs"]
mod issue_1024_rail_through_boundary_vertex;
#[path = "../issue_1036_coincident_skin_sides.rs"]
mod issue_1036_coincident_skin_sides;
#[path = "../issue_1058_pleated_skins_speck_face.rs"]
mod issue_1058_pleated_skins_speck_face;
#[path = "../issue_1449_taper_corner_rules.rs"]
mod issue_1449_taper_corner_rules;
#[path = "../issue_1450_curve_sweep_profile_placement.rs"]
mod issue_1450_curve_sweep_profile_placement;
#[path = "../issue_1462_sweep_attributes.rs"]
mod issue_1462_sweep_attributes;
#[path = "../issue_1463_revolve_inside_out_cylinder.rs"]
mod issue_1463_revolve_inside_out_cylinder;
#[path = "../issue_1467_rail_end_past_side_edge_end.rs"]
mod issue_1467_rail_end_past_side_edge_end;
#[path = "../issue_1476_swept_bar_shell.rs"]
mod issue_1476_swept_bar_shell;
#[path = "../issue_1477_arc_sweep_cylinder.rs"]
mod issue_1477_arc_sweep_cylinder;
#[path = "../issue_1480_pleated_loft_union_edge_ends.rs"]
mod issue_1480_pleated_loft_union_edge_ends;
#[path = "../issue_1490_revolve_torus_inside_out.rs"]
mod issue_1490_revolve_torus_inside_out;
#[path = "../issue_1491_revolve_circle_seam_at_profile.rs"]
mod issue_1491_revolve_circle_seam_at_profile;
#[path = "../issue_1507_1477_arc_flags_compose.rs"]
mod issue_1507_1477_arc_flags_compose;
#[path = "../issue_1507_linear_sweep_arc_zero.rs"]
mod issue_1507_linear_sweep_arc_zero;
#[path = "../issue_1508_extrusion_origin_scaled_zero.rs"]
mod issue_1508_extrusion_origin_scaled_zero;
#[path = "../issue_1511_revolve_disc_touching_axis.rs"]
mod issue_1511_revolve_disc_touching_axis;
#[path = "../issue_1671_uncapped_revolve_owner.rs"]
mod issue_1671_uncapped_revolve_owner;
#[path = "../issue_1776_concave_taper.rs"]
mod issue_1776_concave_taper;
#[path = "../issue_1937_bottle_sweep.rs"]
mod issue_1937_bottle_sweep;
#[path = "../issue_1945_rail_uv_trim.rs"]
mod issue_1945_rail_uv_trim;
#[path = "../issue_1952_extended_rail_end_unsolved_section.rs"]
mod issue_1952_extended_rail_end_unsolved_section;
#[path = "../issue_2009_group_c_sweep_p3.rs"]
mod issue_2009_group_c_sweep_p3;
#[path = "../issue_2009_group_c_sweep_statuses.rs"]
mod issue_2009_group_c_sweep_statuses;
#[path = "../issue_2192_closed_curve_sweep.rs"]
mod issue_2192_closed_curve_sweep;
#[path = "../issue_2406_end_rail_rollover.rs"]
mod issue_2406_end_rail_rollover;
#[path = "../issue_2540_near_straight_spline_extrusion.rs"]
mod issue_2540_near_straight_spline_extrusion;
#[path = "../issue_2568_curve_analysis_shortcuts.rs"]
mod issue_2568_curve_analysis_shortcuts;
#[path = "../issue_2621_m16_groove.rs"]
mod issue_2621_m16_groove;
#[path = "../issue_2905_revolved_vase.rs"]
mod issue_2905_revolved_vase;
#[path = "../issue_3093_extrusion_defeature.rs"]
mod issue_3093_extrusion_defeature;
#[path = "../issue_410_435_thread_groove.rs"]
mod issue_410_435_thread_groove;
#[path = "../issue_410_screw_thread_vertex_gaps.rs"]
mod issue_410_screw_thread_vertex_gaps;
#[path = "../issue_435_screw_thread_groove.rs"]
mod issue_435_screw_thread_groove;
#[path = "../issue_479_taper_pivot.rs"]
mod issue_479_taper_pivot;
#[path = "../issue_948_ruled_skin_difference.rs"]
mod issue_948_ruled_skin_difference;
#[path = "../issue_968_drafted_pocket_floor_cut.rs"]
mod issue_968_drafted_pocket_floor_cut;
#[path = "../loft_rails.rs"]
mod loft_rails;
#[path = "../prism_fill.rs"]
mod prism_fill;
#[path = "../screw_sweep.rs"]
mod screw_sweep;
#[path = "../skin_parallel_sections.rs"]
mod skin_parallel_sections;
#[path = "../skin_sources.rs"]
mod skin_sources;
#[path = "../sweep_closed_rail.rs"]
mod sweep_closed_rail;
#[path = "../sweep_cornered.rs"]
mod sweep_cornered;
#[path = "../sweep_profile_line_keeps_its_nurb.rs"]
mod sweep_profile_line_keeps_its_nurb;
#[path = "../sweep_stations.rs"]
mod sweep_stations;
#[path = "../sweep_two_rails.rs"]
mod sweep_two_rails;
#[path = "../sweep_upright.rs"]
mod sweep_upright;
#[path = "../taper_profile_line_keeps_its_nurb.rs"]
mod taper_profile_line_keeps_its_nurb;
#[path = "../wr302_rail_returns_past_end_beyond_boss.rs"]
mod wr302_rail_returns_past_end_beyond_boss;
#[path = "../wr460_untrimmed_rails.rs"]
mod wr460_untrimmed_rails;
#[path = "../wr591_touching_loops_sweep_any_order.rs"]
mod wr591_touching_loops_sweep_any_order;
#[path = "../wr609_trace_reaches_rails.rs"]
mod wr609_trace_reaches_rails;
#[path = "../wr610_779cb9b_rail_meets_seam_line_past_its_end.rs"]
mod wr610_779cb9b_rail_meets_seam_line_past_its_end;
#[path = "../wr630_rail_end_on_continued_surface.rs"]
mod wr630_rail_end_on_continued_surface;
#[path = "../wr649_band_between_rail_ends.rs"]
mod wr649_band_between_rail_ends;
#[path = "../wr697_swept_turn.rs"]
mod wr697_swept_turn;
#[path = "../wr_407_swept_arc_walls_join_without_fold.rs"]
mod wr_407_swept_arc_walls_join_without_fold;
#[path = "../wr_440_revolved_line_seam_check.rs"]
mod wr_440_revolved_line_seam_check;
#[path = "../wr_533_lip_swept_flush_round_a_corner.rs"]
mod wr_533_lip_swept_flush_round_a_corner;
#[path = "../wr_552_revolve_arc_touching_axis.rs"]
mod wr_552_revolve_arc_touching_axis;
#[path = "../wr_571_draft_wall_closes_at_apex.rs"]
mod wr_571_draft_wall_closes_at_apex;
#[path = "../wr_590_revolve_splits_curve_on_axis.rs"]
mod wr_590_revolve_splits_curve_on_axis;
#[path = "../wr_594_rails_meet_at_one_end_point.rs"]
mod wr_594_rails_meet_at_one_end_point;
#[path = "../wr_645_drafted_pocket_corner_rounds.rs"]
mod wr_645_drafted_pocket_corner_rounds;
