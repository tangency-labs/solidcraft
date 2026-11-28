//! The captured second shell of a rounded, already hollow box (solidcore#2954).
//! The case's analytic material-volume interval is independent of the kernel result.

use solidcore::kernel::UpstreamFixes;
use solidcore::mass::integrator::properties::brep_volume_default;
use solidcore::offset::offset_executive::p2::{shell_brep_simple, OffsetOptions, ShellOptions};
use solidcore::query::api_queries::{face_bounding_box, face_internal_point};
use solidcore::topology::brep::p2::is_manifold_solid;
use solidcore::Kernel;
use solidcore_convert::brep::to_kernel;
use solidcore_record::brep::Brep as Record;
use std::io::Read;

fn captured_record() -> Record {
    let bytes = include_bytes!("../../solidcore/tests/kernel_cases/shell/cov-shell-again-open-at-an-inner-wall-of-a-shelled-box.bodies.json.gz");
    let mut text = String::new();
    flate2::read::GzDecoder::new(&bytes[..])
        .read_to_string(&mut text)
        .unwrap();
    let mut records: serde_json::Map<String, serde_json::Value> =
        serde_json::from_str(&text).unwrap();
    serde_json::from_value(records.remove("b12").unwrap()).unwrap()
}

#[test]
#[ignore = "reported failing shell, diagnosis in progress (#2954)"]
fn the_saved_rounded_box_can_be_shelled_again_at_its_inner_wall() {
    solidcore_ops::session::set_fixes(UpstreamFixes::NAMES.iter().copied()).unwrap();
    let record = captured_record();
    if let Some(dir) = std::env::var_os("ISSUE_2954_OUTPUT") {
        let body = solidcore_ops::Body::from_record(&record, true).unwrap().0;
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            std::path::Path::new(&dir).join("second-shell-input.smb"),
            body.to_smb(true).unwrap(),
        )
        .unwrap();
    }
    let mut k = Kernel {
        fixes: UpstreamFixes::all(),
        ..Kernel::default()
    };
    let (input, order) = to_kernel(&mut k, &record, true).unwrap();
    let open = order.faces[14];
    let mut measured = input.clone_standing_for_self().unwrap();
    eprintln!(
        "input topology: {} faces, {} edges, {} vertices",
        input.face_list().len(),
        input.edge_list().len(),
        input.vertex_list().len()
    );
    eprintln!(
        "picked record face 14: {open:?}, native position {:?}, box {:?}, point {:?}",
        input.face_list().iter().position(|&f| f == open),
        face_bounding_box(&k, &measured, open, true),
        face_internal_point(&k, &mut measured, open)
    );
    let before = brep_volume_default(&k, &mut measured, 1e-8).unwrap();
    eprintln!("input volume {before:?}");
    let made = shell_brep_simple(
        &mut k,
        &input,
        ShellOptions {
            offset: OffsetOptions {
                distance: -0.5,
                extend_convex_edges: true,
                merge_results: true,
            },
            create_offset_solid: true,
            merge_sheet_side_walls: true,
        },
        &[open],
    );
    let mut made =
        made.unwrap_or_else(|e| panic!("second shell failed at its kernel operation: {e:?}"));
    assert!(is_manifold_solid(&made).unwrap());
    let volume = brep_volume_default(&k, &mut made, 1e-8).unwrap();
    assert!(
        (9017.6..=9038.3).contains(&volume),
        "second-shell volume {volume}"
    );
}
