//! solidcore's integration tests of topology edits, copies, attributes, healing and the rest, as one test binary: the files
//! in `tests/` are modules here, so the kernel is linked once per area, not once per file
//! (tangency-labs/solidcraft#14926).
//!
//! The files stay where they are and come in by `#[path]`, so their `#[path]` support
//! modules, their `include_str!`/`include_bytes!` paths and their data files read as before.
//! A test's name gains its file as a prefix: `cargo test -p solidcore --test topology <file>::`
//! runs what `--test <file>` ran.
//!
//! A new test file in `tests/` is one `#[path]` line in the main.rs of its area, in order;
//! `every_test_file_is_built` (in `tests/topology/main.rs`) fails until it is in one.

// Each file keeps the `#[path]` support modules it had as a binary of its own.
#![allow(clippy::duplicate_mod)]

#[path = "../copy_faces_merge_key_aliasing.rs"]
mod copy_faces_merge_key_aliasing;
#[path = "../copy_keeps_attributes_default_naming.rs"]
mod copy_keeps_attributes_default_naming;
#[path = "../curve_point_handles.rs"]
mod curve_point_handles;
#[path = "../cut_stitch_under_kernel_callbacks.rs"]
mod cut_stitch_under_kernel_callbacks;
#[path = "../delete_inner_loop.rs"]
mod delete_inner_loop;
#[path = "../edge_properties_hold_their_accuracy.rs"]
mod edge_properties_hold_their_accuracy;
#[path = "../face37_winding_hulls.rs"]
mod face37_winding_hulls;
#[path = "../face_loop_orientation_pole_point_test.rs"]
mod face_loop_orientation_pole_point_test;
#[path = "../face_spline_sheet.rs"]
mod face_spline_sheet;
#[path = "../fork_577f7be_hair_off_pin_seam.rs"]
mod fork_577f7be_hair_off_pin_seam;
#[path = "../issue_1056_plane_recognition_origin_after_axes.rs"]
mod issue_1056_plane_recognition_origin_after_axes;
#[path = "../issue_1057_cylinder_grazing_box_edge.rs"]
mod issue_1057_cylinder_grazing_box_edge;
#[path = "../issue_1440_identity_of_copy.rs"]
mod issue_1440_identity_of_copy;
#[path = "../issue_1452_sm_status.rs"]
mod issue_1452_sm_status;
#[path = "../issue_1523_tiny_hole_loop_orientation.rs"]
mod issue_1523_tiny_hole_loop_orientation;
#[path = "../issue_1528_orphan_records.rs"]
mod issue_1528_orphan_records;
#[path = "../issue_1556_unbounded_v_face_properties.rs"]
mod issue_1556_unbounded_v_face_properties;
#[path = "../issue_1570_flat_body_ray_fire.rs"]
mod issue_1570_flat_body_ray_fire;
#[path = "../issue_1570_transform_matrix.rs"]
mod issue_1570_transform_matrix;
#[path = "../issue_1628_copy_faces_into_nonempty_brep.rs"]
mod issue_1628_copy_faces_into_nonempty_brep;
#[path = "../issue_1766_notify_ref_public.rs"]
mod issue_1766_notify_ref_public;
#[path = "../issue_1869_copy_faces_merge_into_empty.rs"]
mod issue_1869_copy_faces_merge_into_empty;
#[path = "../issue_1875_local_operation_marks.rs"]
mod issue_1875_local_operation_marks;
#[path = "../issue_1954_wr382_wire_check.rs"]
mod issue_1954_wr382_wire_check;
#[path = "../issue_2009_group_a_statuses.rs"]
mod issue_2009_group_a_statuses;
#[path = "../issue_2011_edit_brep.rs"]
mod issue_2011_edit_brep;
#[path = "../issue_2013_brep_check_warnings.rs"]
mod issue_2013_brep_check_warnings;
#[path = "../issue_2014_upstream_fixes_flags.rs"]
mod issue_2014_upstream_fixes_flags;
#[path = "../issue_2058_ascii_junk_read.rs"]
mod issue_2058_ascii_junk_read;
#[path = "../issue_2115_kernel_edit_owners_by_scheme.rs"]
mod issue_2115_kernel_edit_owners_by_scheme;
#[path = "../issue_2857_smb_writers.rs"]
mod issue_2857_smb_writers;
#[path = "../issue_431_check_toothed_plate.rs"]
mod issue_431_check_toothed_plate;
#[path = "../issue_711_overlapping_holes.rs"]
mod issue_711_overlapping_holes;
#[path = "../issue_933_geometric_g2.rs"]
mod issue_933_geometric_g2;
#[path = "../join_owns_curve_by_edge_identity.rs"]
mod join_owns_curve_by_edge_identity;
#[path = "../list_operand_merge_work.rs"]
mod list_operand_merge_work;
#[path = "../merge_after_copy_faces_keeps_attributes.rs"]
mod merge_after_copy_faces_keeps_attributes;
#[path = "../patterned_cut.rs"]
mod patterned_cut;
#[path = "../precise_area_holds_its_accuracy.rs"]
mod precise_area_holds_its_accuracy;
#[path = "../uv_trim_edge_tolerance.rs"]
mod uv_trim_edge_tolerance;
#[path = "../wr260_failed_reversal_observable.rs"]
mod wr260_failed_reversal_observable;
#[path = "../wr69_dupin_bottom_hyperbola.rs"]
mod wr69_dupin_bottom_hyperbola;
#[path = "../wr96_curve_loop_start.rs"]
mod wr96_curve_loop_start;
#[path = "../wr_450_box_minus_rational_cylinder.rs"]
mod wr_450_box_minus_rational_cylinder;
#[path = "../wr_528_crescent_on_rim_join.rs"]
mod wr_528_crescent_on_rim_join;
#[path = "../wr_534_local_operation_moves_a_face_far.rs"]
mod wr_534_local_operation_moves_a_face_far;
#[path = "../wr_537_pin_a_hair_off_a_seam.rs"]
mod wr_537_pin_a_hair_off_a_seam;
#[path = "../wr_540_tangent_cylinder_edge_order.rs"]
mod wr_540_tangent_cylinder_edge_order;
#[path = "../wr_566_local_operation_closes_a_wall.rs"]
mod wr_566_local_operation_closes_a_wall;
#[path = "../wr_591_loops_touching_at_a_vertex.rs"]
mod wr_591_loops_touching_at_a_vertex;
#[path = "../wr_596_tiny_box_fails.rs"]
mod wr_596_tiny_box_fails;
#[path = "../wr_660_planar_path_pose.rs"]
mod wr_660_planar_path_pose;

/// Every `tests/*.rs` file is a module of one area's `main.rs` or a test target of its own in
/// `Cargo.toml`, and so is every `tests/<dir>/main.rs`. With `autotests = false` cargo builds
/// no file it is not told about, so a file left out would silently run nothing.
#[test]
#[allow(clippy::unwrap_used)] // test
fn every_test_file_is_built() {
    let mains = [
        include_str!("../blend/main.rs"),
        include_str!("../booleans/main.rs"),
        include_str!("../geometry/main.rs"),
        include_str!("../offset/main.rs"),
        include_str!("../sweep/main.rs"),
        include_str!("main.rs"),
    ];
    let manifest = include_str!("../../Cargo.toml");
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
    let mut missing: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .filter(|name| {
            if is_rust(name) {
                let line = format!("#[path = \"../{name}\"]");
                !mains.iter().any(|main| main.contains(&line))
                    && !manifest.contains(&format!("path = \"tests/{name}\""))
            } else {
                dir.join(name).join("main.rs").is_file()
                    && !manifest.contains(&format!("path = \"tests/{name}/main.rs\""))
            }
        })
        .collect();
    missing.sort();
    assert!(
        missing.is_empty(),
        "no area's main.rs builds a module for {missing:?}: add `#[path = \"../<file>.rs\"] mod <file>;` to one"
    );
}

/// Whether a file name in `tests/` is a Rust source file.
fn is_rust(name: &str) -> bool {
    std::path::Path::new(name)
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("rs"))
}
