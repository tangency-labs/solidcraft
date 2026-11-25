//! A KiCad board opens through `doc.open`: the substrate as a solid, the copper and silkscreen
//! chosen by the options, the board's own thickness unless overridden, and every option checked
//! before the board is read.

use serde_json::json;

use super::{run, volume};
use crate::*;

fn board_file(tag: &str, thickness: f64) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("solidcraft-kicad-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("board.kicad_pcb");
    std::fs::write(
        &path,
        format!(
            r#"(kicad_pcb (general (thickness {thickness}))
  (gr_rect (start 0 0) (end 20 10) (layer "Edge.Cuts"))
  (segment (start 1 2) (end 10 2) (width 0.3) (layer "F.Cu") (net 1)))"#
        ),
    )
    .unwrap();
    path
}

#[test]
fn a_board_opens_as_a_solid_substrate() {
    let path = board_file("solid", 2.0);
    let mut s = Session::default();
    let r = run(&mut s, "doc.open", json!({"path": path.to_string_lossy()}));
    assert_eq!(r["board_solid"], json!(true), "{r}");
    let names: Vec<String> = s.model.state().bodies.iter().map(|b| b.name.clone()).collect();
    assert!(names.iter().any(|name| name == "board"), "{names:?}");
    // The copper overlay is on by default: the 9 mm × 0.3 mm track is a flat face over the board,
    // so it adds no volume, and the measure says so by counting it among the open bodies.
    let measured = run(&mut s, "inspect.measure", json!({}));
    assert_eq!(measured["total"]["open_bodies_skipped"], json!(1), "{measured}");
    let got = volume(&mut s);
    assert!((got - 400.0).abs() < 1e-6, "{got}");
    let open = measured["bodies"].as_array().into_iter().flatten().filter(|b| b["name"] == json!("Copper F.Cu")).all(|b| b["closed"] == json!(false));
    assert!(open, "{measured}");
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn the_options_choose_the_parts_and_override_the_thickness() {
    let path = board_file("options", 2.0);
    let mut s = Session::default();
    let r = run(&mut s, "doc.open", json!({"path": path.to_string_lossy(), "copper": "mesh", "silkscreen": false, "thickness": 1.0}));
    assert_eq!(r["board_solid"], json!(true), "{r}");
    let names: Vec<String> = s.model.state().bodies.iter().map(|b| b.name.clone()).collect();
    assert!(names.iter().any(|name| name == "Copper F.Cu"), "{names:?}");
    // The solid substrate is 200 mm³ with the overriding 1 mm thickness, plus the 9 mm × 0.3 mm
    // × 0.035 mm copper track.
    let got = volume(&mut s);
    assert!((got - (200.0 + 9.0 * 0.3 * 0.035)).abs() < 1e-3, "{got}");
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn the_board_stands_with_its_lower_left_corner_at_the_origin() {
    // Drawn far from KiCad's origin, as a board on a project sheet usually is.
    let dir = std::env::temp_dir().join(format!("solidcraft-kicad-origin-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("board.kicad_pcb");
    std::fs::write(
        &path,
        r#"(kicad_pcb (general (thickness 2))
  (gr_rect (start 100 50) (end 120 60) (layer "Edge.Cuts")))"#,
    )
    .unwrap();
    let mut s = Session::default();
    let r = run(&mut s, "doc.open", json!({"path": path.to_string_lossy()}));
    assert_eq!(r["board_solid"], json!(true), "{r}");
    let world = s.world_state().unwrap();
    let bbox = world
        .bodies
        .iter()
        .find(|b| b.name == "board")
        .map(|b| solidcraft_kernel::measure(&b.body).map(|m| m.bbox))
        .expect("no board body")
        .expect("the board would not measure");
    // KiCad's Y is negated, so the board covers Y -60..-50 and is moved up to -10..0.
    assert!(bbox.min.x.abs() < 1e-6 && bbox.min.y.abs() < 1e-6, "{bbox:?}");
    assert!((bbox.max.x - 20.0).abs() < 1e-6 && (bbox.max.y - 10.0).abs() < 1e-6, "{bbox:?}");
    // The shift is flat: the board keeps its thickness and stands at the same height.
    assert!((bbox.max.z - bbox.min.z - 2.0).abs() < 1e-6, "{bbox:?}");
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

/// A folder holding `board.kicad_pcb`, whose one footprint (U1) names `model`, and the model
/// files given beside it.
fn board_with_model_files(tag: &str, model: &str, files: &[(&str, Vec<u8>)]) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("solidcraft-kicad-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    for (name, bytes) in files {
        std::fs::write(dir.join(name), bytes).unwrap();
    }
    let path = dir.join("board.kicad_pcb");
    std::fs::write(
        &path,
        format!(
            r#"(kicad_pcb (general (thickness 1.6))
  (gr_rect (start 0 0) (end 20 10) (layer "Edge.Cuts"))
  (footprint "part" (at 5 5) (layer "F.Cu") (property "Reference" "U1")
    (model "${{KIPRJMOD}}/{model}")))"#
        ),
    )
    .unwrap();
    path
}

/// KiCad 10's `….stpZ`: a zip archive holding one STEP file.
fn stpz() -> Vec<u8> {
    use std::io::Write;
    let mut out = std::io::Cursor::new(Vec::new());
    let mut zip = zip::ZipWriter::new(&mut out);
    zip.start_file("part.step", zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated)).unwrap();
    zip.write_all(include_bytes!("../../../../tests/step-conformance/cylinder.step")).unwrap();
    zip.finish().unwrap();
    out.into_inner()
}

/// Open the board and check that U1's model came in with no warning about it.
fn assert_the_model_comes_in(path: &std::path::Path) {
    let mut s = Session::default();
    let r = run(&mut s, "doc.open", json!({"path": path.to_string_lossy()}));
    assert_eq!(r["subcomponents"], json!(1), "{r}");
    let warnings = r["warnings"].as_array().cloned().unwrap_or_default();
    assert!(warnings.iter().all(|w| !w.as_str().unwrap_or("").contains("part.")), "{r}");
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

/// KiCad 10's 3D library has each model only as a zipped `….stpZ`, while a board drawn with older
/// footprints names `….wrl` or `….step`: the reference is answered by the `….stpZ` beside it.
#[test]
fn a_model_named_in_another_format_is_read_from_the_stpz_beside_it() {
    assert_the_model_comes_in(&board_with_model_files("stpz", "part.wrl", &[("part.stpZ", stpz())]));
}

/// A file system that tells `….stpZ` from `….stpz` still finds a lower-case `….stpz`.
#[test]
fn a_lower_case_stpz_answers_a_missing_model() {
    assert_the_model_comes_in(&board_with_model_files("stpz-lower", "part.wrl", &[("part.stpz", stpz())]));
}

/// A missing `….step` is answered by the `….wrl` beside it before a `….stpZ`: a damaged `….stpZ`
/// does not take a model away from a board that found its VRML model before.
#[test]
fn a_damaged_stpz_does_not_hide_the_wrl_beside_it() {
    let wrl = b"#VRML V2.0 utf8\nShape { geometry Box { size 2 2 2 } }".to_vec();
    assert_the_model_comes_in(&board_with_model_files("stpz-damaged", "part.step", &[("part.wrl", wrl), ("part.stpZ", b"not a zip".to_vec())]));
}

/// Three footprints share one model: one component named after the model, and each placement
/// is named after its own footprint's reference. Two footprints that both say `R1` keep apart.
#[test]
fn each_part_is_named_after_its_own_reference() {
    let dir = std::env::temp_dir().join(format!("solidcraft-kicad-references-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("R_0603.wrl"), b"#VRML V2.0 utf8\nShape { geometry Box { size 2 2 2 } }").unwrap();
    let footprint = |x: u32, reference: &str| {
        format!(r#"(footprint "R" (at {x} 5) (layer "F.Cu") (property "Reference" "{reference}") (model "${{KIPRJMOD}}/R_0603.wrl"))"#)
    };
    let path = dir.join("board.kicad_pcb");
    std::fs::write(
        &path,
        format!(
            "(kicad_pcb (general (thickness 1.6)) (gr_rect (start 0 0) (end 30 10) (layer \"Edge.Cuts\")) {} {} {})",
            footprint(5, "R1"),
            footprint(12, "R33"),
            footprint(19, "R1")
        ),
    )
    .unwrap();
    let mut s = Session::default();
    let r = run(&mut s, "doc.open", json!({"path": path.to_string_lossy()}));
    assert_eq!(r["subcomponents"], json!(1), "one model, one component: {r}");
    let part = s.doc.components.iter().find(|c| c.name == "R_0603").map(|c| c.id).expect("the model's component");
    let mut names: Vec<&str> = s.doc.occurrences.iter().filter(|o| o.component == part).map(|o| o.name.as_str()).collect();
    names.sort_unstable();
    assert_eq!(names, vec!["R1", "R1 (2)", "R33"]);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A file can give thousands of footprints one reference: each still gets its own name, and
/// naming them does not search every name again for each suffix.
#[test]
fn thousands_of_footprints_sharing_a_reference_are_numbered() {
    const COUNT: u32 = 3000;
    let dir = std::env::temp_dir().join(format!("solidcraft-kicad-same-reference-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("R_0603.wrl"), b"#VRML V2.0 utf8\nShape { geometry Box { size 0.5 0.5 0.5 } }").unwrap();
    let footprints: String = (0..COUNT)
        .map(|i| {
            format!(
                r#"(footprint "R" (at {} {} ) (layer "F.Cu") (property "Reference" "R1") (model "${{KIPRJMOD}}/R_0603.wrl"))"#,
                1 + i % 60,
                1 + i / 60
            )
        })
        .collect();
    let path = dir.join("board.kicad_pcb");
    std::fs::write(&path, format!("(kicad_pcb (general (thickness 1.6)) (gr_rect (start 0 0) (end 62 52) (layer \"Edge.Cuts\")) {footprints})"))
        .unwrap();
    let mut s = Session::default();
    let started = std::time::Instant::now();
    run(&mut s, "doc.open", json!({"path": path.to_string_lossy(), "copper": "none", "silkscreen": false}));
    eprintln!("{COUNT} footprints sharing a reference opened in {:?}", started.elapsed());
    let part = s.doc.components.iter().find(|c| c.name == "R_0603").map(|c| c.id).expect("the model's component");
    let names: std::collections::BTreeSet<&str> = s.doc.occurrences.iter().filter(|o| o.component == part).map(|o| o.name.as_str()).collect();
    assert_eq!(names.len(), COUNT as usize, "every placement has a name of its own");
    assert!(names.contains("R1") && names.contains("R1 (2)") && names.contains(format!("R1 ({COUNT})").as_str()));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_bad_option_is_refused_before_the_board_is_read() {
    let path = board_file("bad", 2.0);
    let mut s = Session::default();
    let error = s.execute("doc.open", &json!({"path": path.to_string_lossy(), "copper": "brep"})).unwrap_err();
    assert!(format!("{error}").contains("copper"), "{error}");
    assert!(s.doc.features.is_empty(), "a refused option left a feature behind");
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

fn rewrite_board(path: &std::path::Path, width: f64) {
    std::fs::write(
        path,
        format!(
            r#"(kicad_pcb (general (thickness 2))
  (gr_rect (start 0 0) (end {width} 10) (layer "Edge.Cuts"))
  (segment (start 1 2) (end 10 2) (width 0.3) (layer "F.Cu") (net 1)))"#
        ),
    )
    .unwrap();
}

#[test]
fn an_opened_board_stays_linked_and_update_links_reads_it_again_with_its_options() {
    let path = board_file("live-link", 2.0);
    let mut s = Session::default();
    run(&mut s, "doc.open", json!({"path": path.to_string_lossy(), "copper": "mesh", "silkscreen": false, "thickness": 1.0}));
    let component = *s.doc.links.keys().next().expect("the board is linked");
    let listed = run(&mut s, "component.links", json!({}));
    let link = &listed["links"][0];
    assert_eq!(link["component"], json!(component), "{listed}");
    assert_eq!(link["importer"]["format"], json!("kicad"), "{listed}");
    assert_eq!(link["changed"], json!(false), "{listed}");
    let kept = s.doc.links[&component].importer.as_ref().expect("the options are kept").options.clone();
    assert_eq!(kept["copper"], json!("mesh"), "{kept}");
    assert_eq!(kept["thickness"], json!(1.0), "{kept}");
    let unchanged = run(&mut s, "component.update_links", json!({}));
    assert_eq!(unchanged["unchanged"].as_array().map(Vec::len), Some(1), "{unchanged}");

    // The board file changes: the link says so, and Update Links brings the new board in.
    rewrite_board(&path, 30.5);
    let listed = run(&mut s, "component.links", json!({}));
    assert_eq!(listed["links"][0]["changed"], json!(true), "{listed}");
    let done = run(&mut s, "component.update_links", json!({}));
    assert_eq!(done["updated"].as_array().map(Vec::len), Some(1), "{done}");
    // The same options: the overriding 1 mm thickness and the modelled copper, not the file's 2 mm.
    let got = volume(&mut s);
    assert!((got - (30.5 * 10.0 * 1.0 + 9.0 * 0.3 * 0.035)).abs() < 1e-3, "{got}");
    assert_eq!(s.doc.links.keys().copied().collect::<Vec<_>>(), [component], "the component keeps its link");
    assert_eq!(s.doc.features.iter().filter(|f| f.component == component).count(), 2, "one board feature and its solid, not two of each");
    let listed = run(&mut s, "component.links", json!({}));
    assert_eq!(listed["links"][0]["changed"], json!(false), "{listed}");
    let again = run(&mut s, "component.update_links", json!({}));
    assert_eq!(again["unchanged"].as_array().map(Vec::len), Some(1), "{again}");
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn a_board_that_cannot_be_read_again_keeps_its_bodies() {
    let path = board_file("live-link-broken", 2.0);
    let mut s = Session::default();
    run(&mut s, "doc.open", json!({"path": path.to_string_lossy()}));
    let before = volume(&mut s);
    let features = s.doc.features.len();
    std::fs::write(&path, "(kicad_pcb (general").unwrap();
    let error = s.execute("component.update_links", &json!({})).unwrap_err();
    assert!(format!("{error}").contains("board.kicad_pcb"), "{error}");
    assert_eq!(s.doc.features.len(), features);
    assert!((volume(&mut s) - before).abs() < 1e-9);
    assert_eq!(s.doc.links.len(), 1, "the link stays");
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}
