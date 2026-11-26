//! solidcore's integration tests of booleans, imprints, defeaturing and gluing, as one test binary: the files
//! in `tests/` are modules here, so the kernel is linked once per area, not once per file
//! (tangency-labs/solidcraft#14926).
//!
//! The files stay where they are and come in by `#[path]`, so their `#[path]` support
//! modules, their `include_str!`/`include_bytes!` paths and their data files read as before.
//! A test's name gains its file as a prefix: `cargo test -p solidcore --test booleans <file>::`
//! runs what `--test <file>` ran.
//!
//! A new test file in `tests/` is one `#[path]` line in the main.rs of its area, in order;
//! `every_test_file_is_built` (in `tests/topology/main.rs`) fails until it is in one.

// Each file keeps the `#[path]` support modules it had as a binary of its own.
#![allow(clippy::duplicate_mod)]

#[path = "../boolean_2d_coincident_edges_work.rs"]
mod boolean_2d_coincident_edges_work;
#[path = "../boolean_api.rs"]
mod boolean_api;
#[path = "../boolean_coplanar_merge.rs"]
mod boolean_coplanar_merge;
#[path = "../boolean_no_dangling_vertex_refs.rs"]
mod boolean_no_dangling_vertex_refs;
#[path = "../boolean_uv_trim_owners.rs"]
mod boolean_uv_trim_owners;
#[path = "../curve_owner_zone_tol.rs"]
mod curve_owner_zone_tol;
#[path = "../defeature_glue_failure_goes_on.rs"]
mod defeature_glue_failure_goes_on;
#[path = "../defeature_imprint_without_vertex_sector.rs"]
mod defeature_imprint_without_vertex_sector;
#[path = "../fork_661f79c_glue_every_region.rs"]
mod fork_661f79c_glue_every_region;
#[path = "../fork_67b8a1e_four_far_disks.rs"]
mod fork_67b8a1e_four_far_disks;
#[path = "../fork_edf7414_perforated_plate_work.rs"]
mod fork_edf7414_perforated_plate_work;
#[path = "../hang52_defeature_trace_budget.rs"]
mod hang52_defeature_trace_budget;
#[path = "../hang_swing_huge_scale.rs"]
mod hang_swing_huge_scale;
#[path = "../integrity_public_outputs.rs"]
mod integrity_public_outputs;
#[path = "../issue_1120_boolean_slits.rs"]
mod issue_1120_boolean_slits;
#[path = "../issue_1462_boolean_2d_attributes.rs"]
mod issue_1462_boolean_2d_attributes;
#[path = "../issue_1462_boolean_attributes.rs"]
mod issue_1462_boolean_attributes;
#[path = "../issue_1462_boolean_family_attributes.rs"]
mod issue_1462_boolean_family_attributes;
#[path = "../issue_1462_split_attributes.rs"]
mod issue_1462_split_attributes;
#[path = "../issue_1513_crescent_on_rim_union.rs"]
mod issue_1513_crescent_on_rim_union;
#[path = "../issue_1526_boolean_2d_faces_apart.rs"]
mod issue_1526_boolean_2d_faces_apart;
#[path = "../issue_1631_caller_keyed_ops_check.rs"]
mod issue_1631_caller_keyed_ops_check;
#[path = "../issue_1730_application_attribute_handlers.rs"]
mod issue_1730_application_attribute_handlers;
#[path = "../issue_18166_g2_round_cut_by_carried_body.rs"]
mod issue_18166_g2_round_cut_by_carried_body;
#[path = "../issue_2010_plain_brep_copy.rs"]
mod issue_2010_plain_brep_copy;
#[path = "../issue_2110_tangent_cylinders_union.rs"]
mod issue_2110_tangent_cylinders_union;
#[path = "../issue_21432_boolean_tolerance_cache.rs"]
mod issue_21432_boolean_tolerance_cache;
#[path = "../issue_2177_crescent_on_top_of_rim.rs"]
mod issue_2177_crescent_on_top_of_rim;
#[path = "../issue_2273_slit_edge_is_topological.rs"]
mod issue_2273_slit_edge_is_topological;
#[path = "../issue_2538_tilted_notch_on_a_slant_edge.rs"]
mod issue_2538_tilted_notch_on_a_slant_edge;
#[path = "../issue_2975_stitch_gap_report.rs"]
mod issue_2975_stitch_gap_report;
#[path = "../issue_3218_defeature_limits.rs"]
mod issue_3218_defeature_limits;
#[path = "../issue_388_island_in_void.rs"]
mod issue_388_island_in_void;
#[path = "../issue_480_ring_lip_union.rs"]
mod issue_480_ring_lip_union;
#[path = "../issue_488_operand_trims.rs"]
mod issue_488_operand_trims;
#[path = "../issue_528_slice_by_a_sheet_in_a_face.rs"]
mod issue_528_slice_by_a_sheet_in_a_face;
#[path = "../issue_575_split_rim_join.rs"]
mod issue_575_split_rim_join;
#[path = "../issue_746_slit_loops.rs"]
mod issue_746_slit_loops;
#[path = "../wr490_equal_rods_and_tubes.rs"]
mod wr490_equal_rods_and_tubes;
#[path = "../wr579_near_coincident_tube_ends_join.rs"]
mod wr579_near_coincident_tube_ends_join;
#[path = "../wr580_tube_joint_no_coincident_edges.rs"]
mod wr580_tube_joint_no_coincident_edges;
#[path = "../wr73_proto_vertex_miss_is_false.rs"]
mod wr73_proto_vertex_miss_is_false;
#[path = "../wr_530_local_operation_loose_tangent_edge.rs"]
mod wr_530_local_operation_loose_tangent_edge;
#[path = "../wr_530_local_operation_step_walls.rs"]
mod wr_530_local_operation_step_walls;
#[path = "../wr_536_boolean_2d_far_disks.rs"]
mod wr_536_boolean_2d_far_disks;
#[path = "../wr_619_imprint_skips_collapsed_definition.rs"]
mod wr_619_imprint_skips_collapsed_definition;
#[path = "../wr_619_union_solid_leak.rs"]
mod wr_619_union_solid_leak;
#[path = "../wr_656_defeature_open_base.rs"]
mod wr_656_defeature_open_base;
#[path = "../wr_797_far_union_beside_a_narrow_first_face.rs"]
mod wr_797_far_union_beside_a_narrow_first_face;
