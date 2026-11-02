use super::*;

/// A 10 × 20 × 30 mm box called "Block", saved by FreeCAD 1.1.4 (`fixtures/make_block_fcstd.py`).
const BLOCK_FCSTD: &[u8] = include_bytes!("fixtures/block.FCStd");

/// The same box as a Rhino 7 file, written with McNeel's MIT-licensed rhino3dm package
/// (`fixtures/make_block_3dm.py`).
const BLOCK_3DM: &[u8] = include_bytes!("fixtures/block.3dm");

/// STEP text, which these importers refuse.
const STEP_TEXT: &str = "ISO-10303-21;\nHEADER;\nENDSEC;\nDATA;\nENDSEC;\nEND-ISO-10303-21;\n";

/// The file imports as one solid with the box's volume, through the same Import feature as STEP.
fn imports_the_block(bytes: &[u8], file: &str) {
    let step = native_cad_to_step(bytes, file).unwrap().0;
    assert!(step.contains("MANIFOLD_SOLID_BREP"), "{file}: no solid in the restated STEP");
    let sf = crate::step_import_feature(bytes, &format!("dir/{file}")).unwrap();
    assert_eq!(sf.import.bodies.len(), 1, "{file}");
    let solidcraft_doc::FeatureKind::Import { file: held, step, .. } = &sf.kind else { panic!("not an Import feature") };
    assert_eq!(held, file);
    assert!(step.contains("MANIFOLD_SOLID_BREP"), "the feature keeps the restated STEP");
    let v = solidcraft_kernel::measure(&sf.import.bodies[0].body).unwrap().volume;
    assert!((v - 6000.0).abs() < 1e-6, "{file}: volume {v}");
    // FreeCAD's object label does not come through cadmpeg; the product is then named after the file.
    let stem = file.rsplit_once('.').unwrap().0;
    assert!(["Block", stem].contains(&sf.body_names[0].as_str()), "{file}: {:?}", sf.body_names);
    assert!(!step.contains("cadmpeg_model"), "{file}");
}

#[test]
fn extensions() {
    assert!(is_native_cad_path("a/Part.FCStd") && is_native_cad_path("x.3DM") && is_native_cad_path("y.fcstd"));
    for other in ["c.sat", "c.sab", "c.smt", "c.smb", "c.f3d", "c.sldprt", "c.step", "fcstd"] {
        assert!(!is_native_cad_path(other), "{other}");
    }
    assert!(crate::is_step_path("c.FCStd") && crate::is_step_path("c.3dm"), "read as an Import feature like STEP and IGES");
    assert!(is_native_cad_path("Part.IPT") && is_native_cad_path("Assembly.IAM"));
}

// Original, in-memory neutral records: no Inventor application, third-party
// corpus files, or Corliss records are used to generate these tests.
fn synthetic_inventor_history() -> cadmpeg_ir::CadIr {
    use serde_json::json;
    let mut ir = cadmpeg_ir::CadIr::empty();
    let sketch = "synthetic:test:sketch#rectangle";
    let ids: Vec<_> = (0..4).map(|i| format!("synthetic:test:entity#line-{i}")).collect();
    let points = [[0.0, 0.0], [20.0, 0.0], [20.0, 12.0], [0.0, 12.0]];
    ir.model.sketches.push(
        serde_json::from_value(json!({
            "id":sketch, "name":"Rectangle", "placement":{"kind":"resolved","frame":{
                "origin":{"x":0,"y":0,"z":0}, "normal":{"x":0,"y":0,"z":1}, "u_axis":{"x":1,"y":0,"z":0}}},
            "profiles":[ids.iter().map(|id| json!({"entity":id,"reversed":false})).collect::<Vec<_>>()]
        }))
        .unwrap(),
    );
    for i in 0..4 {
        let start = points[i];
        let end = points[(i + 1) % 4];
        ir.model.sketch_entities.push(
            serde_json::from_value(json!({
                "id":ids[i], "sketch":sketch, "geometry":{"kind":"line",
                    "start":{"u":start[0],"v":start[1]}, "end":{"u":end[0],"v":end[1]}}
            }))
            .unwrap(),
        );
        ir.model.sketch_constraints.push(
            serde_json::from_value(json!({
                "id":format!("synthetic:test:constraint#join-{i}"), "sketch":sketch, "active":true,
                "definition":{"kind":"coincident_loci","loci":[
                    {"kind":"end","entity":ids[i]}, {"kind":"start","entity":ids[(i + 1) % 4]}]}
            }))
            .unwrap(),
        );
    }
    ir.model.features.push(
        serde_json::from_value(json!({
            "id":"synthetic:test:feature#sketch", "ordinal":0, "name":"Rectangle", "suppressed":false,
            "definition":{"definition":"sketch","sketch":{"space":"planar","sketch":sketch}}
        }))
        .unwrap(),
    );
    ir.model.features.push(
        serde_json::from_value(json!({
            "id":"synthetic:test:feature#extrusion", "ordinal":1, "name":"Extrusion", "suppressed":false,
            "dependencies":["synthetic:test:feature#sketch"],
            "definition":{"definition":"extrude", "profile":{"kind":"sketch","value":sketch},
                "direction":{"kind":"profile_normal"}, "start":{"kind":"profile_plane"},
                "extent":{"kind":"one_sided","side":{"termination":{"kind":"blind","length":8}}},
                "op":"new_body", "solid":true}
        }))
        .unwrap(),
    );
    ir
}

fn imported_history_volume(doc: &solidcraft_doc::Document) -> f64 {
    let mut model = solidcraft_doc::Model::new();
    model.evaluate(doc);
    assert!(model.results.iter().all(|r| r.error.is_none()), "{:?}", model.results.iter().filter_map(|r| r.error.as_ref()).collect::<Vec<_>>());
    assert_eq!(model.state().bodies.len(), 1);
    solidcraft_kernel::measure(&model.state().bodies[0].body).unwrap().volume
}

#[test]
fn inventor_neutral_history_rebuilds_owned_editable_features() {
    let ir = synthetic_inventor_history();
    let mut doc = crate::inventor::rebuild(&ir, "part.ipt").unwrap();
    assert_eq!(doc.features.len(), 2);
    assert_eq!(doc.components.len(), 1);
    assert!(doc.features.iter().all(|f| f.component == doc.components[0].id));
    assert!((imported_history_volume(&doc) - 1920.0).abs() < 1e-6);
    let solidcraft_doc::FeatureKind::Extrude { extent, .. } = &mut doc.features[1].kind else { panic!("not editable extrusion") };
    extent.distance = "16".into();
    assert!((imported_history_volume(&doc) - 3840.0).abs() < 1e-6, "editing the imported feature rebuilds the solid");
    let reopened = solidcraft_doc::Document::from_json(&doc.to_json()).unwrap();
    assert!((imported_history_volume(&reopened) - 3840.0).abs() < 1e-6);
}

#[test]
fn inventor_neutral_symmetric_extent_is_total_distance() {
    use cadmpeg_ir::features::{ExtrudeExtent, FeatureDefinition, FeatureOperation};
    let mut ir = synthetic_inventor_history();
    ir.model.features[1].evaluation.edit(|definition, _| {
        let FeatureDefinition::Operation(FeatureOperation::Extrude { extent, .. }) = definition else { panic!("extrude") };
        let ExtrudeExtent::OneSided { side } = extent else { panic!("one sided") };
        *extent = ExtrudeExtent::Symmetric { side: side.clone() };
    });
    let doc = crate::inventor::rebuild(&ir, "symmetric.ipt").unwrap();
    assert!((imported_history_volume(&doc) - 1920.0).abs() < 1e-6, "8 mm total, not 8 mm on each side");
}

#[test]
fn inventor_parameters_preserve_units_and_dependency_expressions() {
    use serde_json::json;
    let mut ir = synthetic_inventor_history();
    ir.model.parameters.push(
        serde_json::from_value(json!({"id":"synthetic:test:parameter#twice",
        "name":"twice", "expression":"2 * depth", "value":{"kind":"length","value":16},
        "dependencies":["synthetic:test:parameter#depth"]}))
        .unwrap(),
    );
    ir.model.parameters.push(
        serde_json::from_value(json!({"id":"synthetic:test:parameter#depth",
        "name":"depth", "expression":"0.8 cm", "value":{"kind":"length","value":8}}))
        .unwrap(),
    );
    let mut doc = crate::inventor::rebuild(&ir, "parameters.ipt").unwrap();
    assert_eq!(doc.params.iter().find(|p| p.name == "twice").unwrap().expr, "2 * depth");
    assert!((doc.eval("twice", solidcraft_doc::expr::Kind::Length).unwrap() - 16.0).abs() < 1e-9);
    doc.set_param("depth", "12", Some("mm"), None).unwrap();
    assert!((doc.eval("twice", solidcraft_doc::expr::Kind::Length).unwrap() - 24.0).abs() < 1e-9);
}

#[test]
fn inventor_incomplete_or_unsupported_history_is_refused_as_a_whole() {
    let mut ir = synthetic_inventor_history();
    ir.model.features[1].suppressed = None;
    assert!(crate::inventor::rebuild(&ir, "unknown-state.ipt").unwrap_err().contains("suppression"));
    ir.model.features[1].suppressed = Some(false);
    ir.model.features[1].dependencies.clear();
    ir.model.features[1].ordinal = 0;
    assert!(crate::inventor::rebuild(&ir, "duplicate-order.ipt").unwrap_err().contains("history position"));
    ir.model.features[1].ordinal = 1;
    ir.model.features[1].evaluation.set_definition(
        serde_json::from_value(serde_json::json!({
            "definition":"unresolved", "family":"revolve"
        }))
        .unwrap(),
    );
    assert!(crate::inventor::rebuild(&ir, "unsupported.ipt").unwrap_err().contains("supported editable history subset"));
    ir.model.features.clear();
    assert!(crate::inventor::rebuild(&ir, "empty.ipt").is_err());
}

#[test]
fn inventor_radius_dimension_remains_bound_to_its_parameter() {
    use serde_json::json;
    let mut ir = synthetic_inventor_history();
    let sketch = ir.model.sketches[0].id.as_str().to_string();
    let circle = "synthetic:test:entity#circle";
    let radius = "synthetic:test:parameter#radius";
    ir.model.sketch_entities.clear();
    ir.model.sketch_entities.push(
        serde_json::from_value(json!({"id":circle,"sketch":sketch,
        "geometry":{"kind":"circle","center":{"u":0,"v":0},"radius":5}}))
        .unwrap(),
    );
    ir.model.sketches[0].profiles = serde_json::from_value(json!([[{"entity":circle,"reversed":false}]])).unwrap();
    ir.model.parameters.push(
        serde_json::from_value(json!({"id":radius,"name":"radius","expression":"5 mm",
        "value":{"kind":"length","value":5}}))
        .unwrap(),
    );
    ir.model.sketch_constraints.clear();
    ir.model.sketch_constraints.push(
        serde_json::from_value(json!({"id":"synthetic:test:constraint#radius",
        "sketch":sketch,"active":true,"driving":true,
        "definition":{"kind":"radius","entity":circle,"parameter":radius}}))
        .unwrap(),
    );
    let mut doc = crate::inventor::rebuild(&ir, "circle.ipt").unwrap();
    // Circular mass properties use tessellation; allow 0.1% relative error
    // against the analytic cylinder, while parameter changes must still rebuild it.
    let before = imported_history_volume(&doc);
    let expected = 200.0 * std::f64::consts::PI;
    assert!((before - expected).abs() < expected * 1e-3, "{before} vs analytic {expected}");
    doc.set_param("radius", "7 mm", Some("mm"), None).unwrap();
    let after = imported_history_volume(&doc);
    let expected = 392.0 * std::f64::consts::PI;
    assert!((after - expected).abs() < expected * 1e-3, "{after} vs analytic {expected}");
}

#[test]
fn inventor_hostile_bytes_fail_cleanly_without_a_corliss_dependency() {
    for data in [b"".as_slice(), b"not a compound file", b"\xd0\xcf\x11\xe0\xa1\xb1\x1a\xe1", &[0u8; 512]] {
        let outcome = std::panic::catch_unwind(|| crate::import_inventor(data, "hostile.ipt"));
        let error = outcome.expect("no escaping panic").expect_err("no invented history");
        assert!(error.to_string().contains("hostile.ipt"));
    }
    assert!(crate::import_inventor(b"", "assembly.iam").is_err());
}

#[test]
fn freecad_box_imports_as_a_solid() {
    let bytes = BLOCK_FCSTD;
    assert!(bytes.starts_with(b"PK"), "an FCStd file is a ZIP archive");
    imports_the_block(bytes, "block.FCStd");
}

#[test]
fn rhino_box_imports_as_a_solid() {
    let bytes = BLOCK_3DM;
    assert!(bytes.starts_with(b"3D Geometry File Format"), "a 3dm file starts with its signature");
    imports_the_block(bytes, "block.3dm");
}

/// Only FreeCAD and Rhino contents are read: STEP text, garbage, a bad ZIP and empty input are
/// refused with a message, as are ACIS, ShapeManager and commercial formats whatever their bytes.
#[test]
fn other_contents_are_refused() {
    let step = STEP_TEXT;
    let fcstd = BLOCK_FCSTD;
    let cases: [(&[u8], &str); 9] = [
        (step.as_bytes(), "x.3dm"),
        (step.as_bytes(), "x.fcstd"),
        (b"\0\0garbage", "x.3dm"),
        (b"PK\x03\x04junk", "x.fcstd"),
        (b"", "x.3dm"),
        (b"700 0 1 0\n", "x.sat"),
        (b"ASM BinaryFile4", "x.smb"),
        (fcstd, "x.f3d"),
        (fcstd, "x.sldprt"),
    ];
    for (bytes, name) in cases {
        let e = native_cad_to_step(bytes, name).expect_err(name);
        assert!(e.starts_with(name), "{name}: the message names the file: {e}");
    }
    // A Rhino file named .FCStd is still read as what it is.
    imports_the_block(BLOCK_3DM, "renamed.FCStd");
}

/// Truncated, bit-flipped and garbage-tailed variants of valid FreeCAD and Rhino files give an
/// error or a solid, never a panic.
#[test]
fn hostile_native_cad_never_panics() {
    for (good, name) in [(BLOCK_FCSTD, "hostile.FCStd"), (BLOCK_3DM, "hostile.3dm")] {
        let mut cases: Vec<Vec<u8>> = (0..good.len()).step_by((good.len() / 60).max(1)).map(|n| good[..n].to_vec()).collect();
        for i in (0..good.len()).step_by((good.len() / 80).max(1)) {
            let mut b = good.to_vec();
            b[i] ^= 0x5a;
            cases.push(b);
            let mut b = good.to_vec();
            b[i] = 0xff;
            cases.push(b);
        }
        let mut tail = good.to_vec();
        tail.extend((0..4096u32).map(|i| (i.wrapping_mul(2_654_435_761) >> 24) as u8));
        cases.push(tail);
        cases.push(good.iter().rev().copied().collect());
        for (i, bytes) in cases.iter().enumerate() {
            let r = std::panic::catch_unwind(|| crate::step_import_feature(bytes, name));
            assert!(r.is_ok(), "{name}: case {i} panicked");
        }
    }
}

/// cadmpeg decodes under its tight `service` caps, with our STEP size limit on the input.
/// cadmpeg 0.6.3 has no work-unit or collection-item budgets, and its service profile already
/// admits the 8 million entities a detailed part needed (#1783).
#[test]
fn decoding_is_capped() {
    let limits = convert::policy().limits;
    let service = cadmpeg_core::decode::ResourceLimits::service();
    assert_eq!(limits.max_input_bytes, crate::MAX_STEP_BYTES as u64);
    let same = cadmpeg_core::decode::ResourceLimits { max_input_bytes: service.max_input_bytes, ..limits };
    assert_eq!(same, service);
}

/// A FreeCAD archive whose Document.xml inflates past the per-entry cap (a ZIP bomb, 300 MB of
/// zeros in a few hundred KB) is refused.
#[test]
fn a_zip_bomb_is_refused() {
    use std::io::Write;
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    zip.start_file("Document.xml", zip::write::SimpleFileOptions::default().large_file(true)).unwrap();
    let zeros = vec![0u8; 1 << 20];
    for _ in 0..300 {
        zip.write_all(&zeros).unwrap();
    }
    let bytes = zip.finish().unwrap().into_inner();
    assert!(bytes.len() < 2 << 20, "{} bytes", bytes.len());
    assert!(native_cad_to_step(&bytes, "bomb.FCStd").is_err());
}
