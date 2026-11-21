//! solidcore-ops' integration tests as one test binary: the files in `tests/` are modules
//! here, so the kernel is linked once, not once per file (tangency-labs/solidcraft#14926).
//!
//! The files stay where they are and come in by `#[path]`, so their `mod common;`, their
//! `include_str!`/`include_bytes!` paths and their data files read as before. A test's name
//! gains its file as a prefix: `cargo test -p solidcore-ops --test ops blend::` runs what
//! `--test blend` ran. The files that are test targets of their own are listed in
//! `Cargo.toml`, with why.
//!
//! A new test file in `tests/` is one `#[path]` line below, in order;
//! `every_test_file_is_built` fails until it is there or in `Cargo.toml`.

// Each file keeps the `mod common;` (two their `mod volume_leak;`) it had as a binary of its own.
#![allow(clippy::duplicate_mod)]

#[path = "../analytic_cv_edit.rs"]
mod analytic_cv_edit;
#[path = "../birail.rs"]
mod birail;
#[path = "../blend.rs"]
mod blend;
#[path = "../blend_chamfer_corner.rs"]
mod blend_chamfer_corner;
#[path = "../blend_crossing.rs"]
mod blend_crossing;
#[path = "../blend_hollow.rs"]
mod blend_hollow;
#[path = "../blend_overflow_first.rs"]
mod blend_overflow_first;
#[path = "../blend_room.rs"]
mod blend_room;
#[path = "../blend_surface.rs"]
mod blend_surface;
#[path = "../classify_point_grazing_ray.rs"]
mod classify_point_grazing_ray;
#[path = "../coincidence_gate_definitions.rs"]
mod coincidence_gate_definitions;
#[path = "../contact.rs"]
mod contact;
#[path = "../continuity_report.rs"]
mod continuity_report;
#[path = "../controlled_sweep.rs"]
mod controlled_sweep;
#[path = "../curvature_deviation.rs"]
mod curvature_deviation;
#[path = "../curve_blend.rs"]
mod curve_blend;
#[path = "../curve_closest_point.rs"]
mod curve_closest_point;
#[path = "../curve_closest_to_ray.rs"]
mod curve_closest_to_ray;
#[path = "../curve_crossings.rs"]
mod curve_crossings;
#[path = "../curve_extras.rs"]
mod curve_extras;
#[path = "../curve_join.rs"]
mod curve_join;
#[path = "../curve_offset.rs"]
mod curve_offset;
#[path = "../curve_offset_on_surface.rs"]
mod curve_offset_on_surface;
#[path = "../curve_tube.rs"]
mod curve_tube;
#[path = "../curved_sliver_coincidence.rs"]
mod curved_sliver_coincidence;
#[path = "../edge_as_curve_sections.rs"]
mod edge_as_curve_sections;
#[path = "../edge_match.rs"]
mod edge_match;
#[path = "../ep_curve_ends.rs"]
mod ep_curve_ends;
#[path = "../extend_dispatch.rs"]
mod extend_dispatch;
#[path = "../extend_plane.rs"]
mod extend_plane;
#[path = "../extend_split_sheet.rs"]
mod extend_split_sheet;
#[path = "../face_cvs.rs"]
mod face_cvs;
#[path = "../face_surface_eval.rs"]
mod face_surface_eval;
#[path = "../failure_code.rs"]
mod failure_code;
#[path = "../fillet_sheets_long_run.rs"]
mod fillet_sheets_long_run;
#[path = "../heal.rs"]
mod heal;
#[path = "../issue_2843_round_on_tangent_line.rs"]
mod issue_2843_round_on_tangent_line;
#[path = "../issue_2844_loft_rim.rs"]
mod issue_2844_loft_rim;
#[path = "../issue_2845_stepped_wall.rs"]
mod issue_2845_stepped_wall;
#[path = "../issue_2878_spline_round_draft.rs"]
mod issue_2878_spline_round_draft;
#[path = "../issue_2905_gear_tooth.rs"]
mod issue_2905_gear_tooth;
#[path = "../issue_2906_loft_cut_rim.rs"]
mod issue_2906_loft_cut_rim;
#[path = "../issue_2947_basin_mitre_creases.rs"]
mod issue_2947_basin_mitre_creases;
#[path = "../issue_2952_tee_equal.rs"]
mod issue_2952_tee_equal;
#[path = "../issue_2953_drafted_peg.rs"]
mod issue_2953_drafted_peg;
#[path = "../measure.rs"]
mod measure;
#[path = "../normal_drop.rs"]
mod normal_drop;
#[path = "../pie_slices_off_centre_arc.rs"]
mod pie_slices_off_centre_arc;
#[path = "../plane_cv_edit.rs"]
mod plane_cv_edit;
#[path = "../plane_match.rs"]
mod plane_match;
#[path = "../prism.rs"]
mod prism;
#[path = "../profile_hook.rs"]
mod profile_hook;
#[path = "../proof_reuse.rs"]
mod proof_reuse;
#[path = "../railed.rs"]
mod railed;
#[path = "../round_ending_at_a_slight_bend.rs"]
mod round_ending_at_a_slight_bend;
#[path = "../round_on_noisy_cylinders.rs"]
mod round_on_noisy_cylinders;
#[path = "../round_runs_out_past_its_side_edge.rs"]
mod round_runs_out_past_its_side_edge;
#[path = "../sheet_reverse.rs"]
mod sheet_reverse;
#[path = "../step_assembly_real.rs"]
mod step_assembly_real;
#[path = "../stitch.rs"]
mod stitch;
#[path = "../stretch_cvs.rs"]
mod stretch_cvs;
#[path = "../sweep.rs"]
mod sweep;
#[path = "../sweep_upright.rs"]
mod sweep_upright;
#[path = "../tangent_chain.rs"]
mod tangent_chain;
#[path = "../tangent_rollover.rs"]
mod tangent_rollover;
#[path = "../trim.rs"]
mod trim;
#[path = "../turn_over.rs"]
mod turn_over;

/// Every `tests/*.rs` file is a module above or a test target of its own in `Cargo.toml`, and so
/// is every `tests/<dir>/main.rs`. With `autotests = false` cargo builds no file it is not told
/// about, so a file left out would silently run nothing.
#[test]
#[allow(clippy::unwrap_used)] // test
fn every_test_file_is_built() {
    let main = include_str!("main.rs");
    let manifest = include_str!("../../Cargo.toml");
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
    let mut missing: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .filter(|name| {
            if is_rust(name) {
                !main.contains(&format!("#[path = \"../{name}\"]"))
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
        "tests/ops/main.rs builds no module for {missing:?}: add `#[path = \"../<file>.rs\"] mod <file>;` there"
    );
}

/// No module above sets the process's fix set (`session::set_fixes`): the tests here that name
/// no fixes of their own run with it, so such a file is a test target of its own in `Cargo.toml`.
#[test]
#[allow(clippy::unwrap_used)] // test
fn no_module_sets_the_process_fixes() {
    let main = include_str!("main.rs");
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
    let mut setting: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .filter(|name| {
            is_rust(name)
                && main.contains(&format!("#[path = \"../{name}\"]"))
                && std::fs::read_to_string(dir.join(name))
                    .unwrap()
                    .contains("set_fixes(")
        })
        .collect();
    setting.sort();
    assert!(
        setting.is_empty(),
        "{setting:?} set the process's fixes: make each a [[test]] of its own in Cargo.toml"
    );
}

/// Whether a file name in `tests/` is a Rust source file.
fn is_rust(name: &str) -> bool {
    std::path::Path::new(name)
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("rs"))
}
