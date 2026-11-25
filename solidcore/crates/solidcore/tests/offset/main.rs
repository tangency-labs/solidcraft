//! solidcore's integration tests of offsets, shells and thickening, as one test binary: the files
//! in `tests/` are modules here, so the kernel is linked once per area, not once per file
//! (tangency-labs/solidcraft#14926).
//!
//! The files stay where they are and come in by `#[path]`, so their `#[path]` support
//! modules, their `include_str!`/`include_bytes!` paths and their data files read as before.
//! A test's name gains its file as a prefix: `cargo test -p solidcore --test offset <file>::`
//! runs what `--test <file>` ran.
//!
//! A new test file in `tests/` is one `#[path]` line in the main.rs of its area, in order;
//! `every_test_file_is_built` (in `tests/topology/main.rs`) fails until it is in one.

// Each file keeps the `#[path]` support modules it had as a binary of its own.
#![allow(clippy::duplicate_mod)]

#[path = "../face_turn_over.rs"]
mod face_turn_over;
#[path = "../faces_continue.rs"]
mod faces_continue;
#[path = "../fork_1b016ca_cavity_floor_under_opening.rs"]
mod fork_1b016ca_cavity_floor_under_opening;
#[path = "../fork_2e1bb8c_four_face_vertex_split.rs"]
mod fork_2e1bb8c_four_face_vertex_split;
#[path = "../fork_37af643_inset_keeps_cavity_regions.rs"]
mod fork_37af643_inset_keeps_cavity_regions;
#[path = "../fork_69c5198_closure_both_sides.rs"]
mod fork_69c5198_closure_both_sides;
#[path = "../fork_a96ce9f_offset_faces.rs"]
mod fork_a96ce9f_offset_faces;
#[path = "../fork_be8ac73_wire_edge_keeps_faceuse_shell.rs"]
mod fork_be8ac73_wire_edge_keeps_faceuse_shell;
#[path = "../issue_1060_offset_mitred_moved_down.rs"]
mod issue_1060_offset_mitred_moved_down;
#[path = "../issue_1061_moved_boss_top_extends_cylinder.rs"]
mod issue_1061_moved_boss_top_extends_cylinder;
#[path = "../issue_1130_shelled_dome_rim.rs"]
mod issue_1130_shelled_dome_rim;
#[path = "../issue_1253_shell_under_rounded_top.rs"]
mod issue_1253_shell_under_rounded_top;
#[path = "../issue_1441_offset_surface_export.rs"]
mod issue_1441_offset_surface_export;
#[path = "../issue_1442_local_operation_offset_status.rs"]
mod issue_1442_local_operation_offset_status;
#[path = "../issue_1442_offset_walls_touch.rs"]
mod issue_1442_offset_walls_touch;
#[path = "../issue_1453_shelled_cylinder_exact_edges.rs"]
mod issue_1453_shelled_cylinder_exact_edges;
#[path = "../issue_1462_offset_attributes.rs"]
mod issue_1462_offset_attributes;
#[path = "../issue_1473_inner_shell.rs"]
mod issue_1473_inner_shell;
#[path = "../issue_1502_thicken_rim_halves.rs"]
mod issue_1502_thicken_rim_halves;
#[path = "../issue_1502_z5_cap_thicken.rs"]
mod issue_1502_z5_cap_thicken;
#[path = "../issue_15282_tilt_cyl_mate_pair.rs"]
mod issue_15282_tilt_cyl_mate_pair;
#[path = "../issue_1528_write_operand.rs"]
mod issue_1528_write_operand;
#[path = "../issue_1599_thicken_planar_side_walls.rs"]
mod issue_1599_thicken_planar_side_walls;
#[path = "../issue_1670_offset_result_owner.rs"]
mod issue_1670_offset_result_owner;
#[path = "../issue_1795_extended_edges_mated.rs"]
mod issue_1795_extended_edges_mated;
#[path = "../issue_1795_shell_hole_through_arc.rs"]
mod issue_1795_shell_hole_through_arc;
#[path = "../issue_1818_mated_shell_wall.rs"]
mod issue_1818_mated_shell_wall;
#[path = "../issue_1950_shell_analytic_edges_face_owned.rs"]
mod issue_1950_shell_analytic_edges_face_owned;
#[path = "../issue_1973_thicken_flat_sheet_edge_attributes.rs"]
mod issue_1973_thicken_flat_sheet_edge_attributes;
#[path = "../issue_1973_thicken_sheet_edge_attributes.rs"]
mod issue_1973_thicken_sheet_edge_attributes;
#[path = "../issue_2012_public_status_codes.rs"]
mod issue_2012_public_status_codes;
#[path = "../issue_2020_offset_mitred_below_depth.rs"]
mod issue_2020_offset_mitred_below_depth;
#[path = "../issue_2047_offset_faces_notify_owners.rs"]
mod issue_2047_offset_faces_notify_owners;
#[path = "../issue_2048_check_offset_surface_face.rs"]
mod issue_2048_check_offset_surface_face;
#[path = "../issue_2111_offset_patch_sheet_gaps.rs"]
mod issue_2111_offset_patch_sheet_gaps;
#[path = "../issue_2174_cap_outward_ub_241.rs"]
mod issue_2174_cap_outward_ub_241;
#[path = "../issue_2174_cap_rim_two_edges.rs"]
mod issue_2174_cap_rim_two_edges;
#[path = "../issue_2350_shell_offset_construction.rs"]
mod issue_2350_shell_offset_construction;
#[path = "../issue_2372_offset_shell_sweep_cancel.rs"]
mod issue_2372_offset_shell_sweep_cancel;
#[path = "../issue_2393_extended_offset_tiny_approx_tol.rs"]
mod issue_2393_extended_offset_tiny_approx_tol;
#[path = "../issue_2484_shell_keeps_ball_patches.rs"]
mod issue_2484_shell_keeps_ball_patches;
#[path = "../issue_2537_tilted_cylinder_cap_on_face.rs"]
mod issue_2537_tilted_cylinder_cap_on_face;
#[path = "../issue_2611_thicken_arc_trim.rs"]
mod issue_2611_thicken_arc_trim;
#[path = "../issue_2954_curved_seam_shell_tangent.rs"]
mod issue_2954_curved_seam_shell_tangent;
#[path = "../issue_2978_local_reach_beside_nets.rs"]
mod issue_2978_local_reach_beside_nets;
#[path = "../issue_3094_offset_refuses_lost_solid.rs"]
mod issue_3094_offset_refuses_lost_solid;
#[path = "../issue_400_shell_cylinder_seam_on_face.rs"]
mod issue_400_shell_cylinder_seam_on_face;
#[path = "../issue_460_open_shell_rounds_thicker_than_wall.rs"]
mod issue_460_open_shell_rounds_thicker_than_wall;
#[path = "../issue_865_whole_offset_of_holed_plate.rs"]
mod issue_865_whole_offset_of_holed_plate;
#[path = "../offset_negative_approximation_tolerance.rs"]
mod offset_negative_approximation_tolerance;
#[path = "../offset_resolve_overlapping_point_class.rs"]
mod offset_resolve_overlapping_point_class;
#[path = "../offset_shell_extrusion_extend.rs"]
mod offset_shell_extrusion_extend;
#[path = "../offset_shell_lamina_face_delete.rs"]
mod offset_shell_lamina_face_delete;
#[path = "../one_edge_or_two_symmetric_walls.rs"]
mod one_edge_or_two_symmetric_walls;
#[path = "../surface_assert_valid_offset_srf_in_volume.rs"]
mod surface_assert_valid_offset_srf_in_volume;
#[path = "../wr573_offset_wave_export.rs"]
mod wr573_offset_wave_export;
#[path = "../wr_342_vase_bottom_clipped_right_side_out.rs"]
mod wr_342_vase_bottom_clipped_right_side_out;
#[path = "../wr_344_half_frustum_shell_keeps_its_rim.rs"]
mod wr_344_half_frustum_shell_keeps_its_rim;
#[path = "../wr_531_shell_drilled_hole_tip.rs"]
mod wr_531_shell_drilled_hole_tip;
#[path = "../wr_532_shell_rounds_stay_on_radius.rs"]
mod wr_532_shell_rounds_stay_on_radius;
#[path = "../wr_545_shell_rim_arcs_exact.rs"]
mod wr_545_shell_rim_arcs_exact;
#[path = "../wr_578_offset_closes_loop_gaps.rs"]
mod wr_578_offset_closes_loop_gaps;
#[path = "../wr_648_offset_faces_closed_wall_join.rs"]
mod wr_648_offset_faces_closed_wall_join;
#[path = "../wr_657_offset_splits_corner_vertices.rs"]
mod wr_657_offset_splits_corner_vertices;
