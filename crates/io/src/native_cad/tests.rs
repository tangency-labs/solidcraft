use super::*;

/// A 10 × 20 × 30 mm box called "Block", saved by FreeCAD 1.1.4 (`fixtures/make_block_fcstd.py`).
const BLOCK_FCSTD: &[u8] = include_bytes!("fixtures/block.FCStd");

/// The same box as a Rhino 7 file, written with McNeel's MIT-licensed rhino3dm package
/// (`fixtures/make_block_3dm.py`).
const BLOCK_3DM: &[u8] = include_bytes!("fixtures/block.3dm");

/// A PartDesign body from sketches: a 40 × 20 × 10 mm pad, a 6 mm hole pocketed through it and
/// its four vertical edges filleted 2 mm (`fixtures/make_bracket_fcstd.py`, FreeCAD 26.3).
const BRACKET_FCSTD: &[u8] = include_bytes!("fixtures/bracket.FCStd");

/// A PartDesign body placed off the origin and turned: a sketch on its XZ plane revolved 270°, a
/// groove and a chamfer (`fixtures/make_knob_fcstd.py`, FreeCAD 26.3).
const KNOB_FCSTD: &[u8] = include_bytes!("fixtures/knob.FCStd");

/// A PartDesign body saved from FreeCAD's window, so its tip's shape stays shown beside the
/// body's: a sketch with two holes padded, a slot pocketed along the X axis and mirrored, holes
/// repeated about Z and along X, a shape binder and filleted corners (`fixtures/make_plate_fcstd.py`,
/// FreeCAD 26.3).
const PLATE_FCSTD: &[u8] = include_bytes!("fixtures/plate.FCStd");

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
/// cadmpeg (since 0.6.3) has no work-unit or collection-item budgets, and its service profile already
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

fn types(d: &NativeDesign) -> Vec<&'static str> {
    d.doc.features.iter().map(|f| f.kind.type_name()).collect()
}

fn measured(d: &NativeDesign) -> Vec<solidcraft_kernel::BodyMeasure> {
    design::evaluate(&d.doc).unwrap().iter().map(|b| solidcraft_kernel::measure(b).unwrap()).collect()
}

/// A FreeCAD body's history comes in as features: its sketches, with every constraint FreeCAD
/// solved them with, the pad and pocket that use them, and the fillet, building the volume FreeCAD
/// built (7840 − 50π mm³).
#[test]
fn freecad_history_rebuilds_as_features() {
    let d = native_cad_design(BRACKET_FCSTD, "bracket.FCStd").unwrap();
    assert_eq!(types(&d), ["Sketch", "ExtrudeFeature", "Sketch", "ExtrudeFeature", "FilletFeature"]);
    let names: Vec<&str> = d.doc.features.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(names, ["Profile", "Pad", "HoleSketch", "Pocket", "Fillet"]);
    assert_eq!(d.doc.features[1].body_names, ["Bracket"], "the pad makes the body FreeCAD calls Bracket");
    assert!(d.warnings.is_empty(), "every constraint came over: {:?}", d.warnings);
    let solidcraft_doc::FeatureKind::Sketch { sketch, .. } = &d.doc.features[0].kind else { panic!("not a sketch") };
    assert_eq!(solidcraft_doc::sketch::solve(&mut sketch.clone()).dof, 0, "the rectangle is fully constrained, as in FreeCAD");
    assert_eq!(sketch.constraints.iter().filter(|c| c.param.is_some()).count(), 2, "its 40 and 20 mm dimensions are parameters");
    let m = measured(&d);
    assert_eq!(m.len(), 1);
    let expected = 7840.0 - 50.0 * std::f64::consts::PI;
    assert!((m[0].volume - expected).abs() < 0.05, "volume {}", m[0].volume);
}

/// A revolved body placed off the origin: the sketch on the body's XZ plane, the revolution and
/// groove turning the way FreeCAD turns them, and the chamfered edge found where the body puts it.
#[test]
fn freecad_revolutions_rebuild_in_their_body_placement() {
    let d = native_cad_design(KNOB_FCSTD, "knob.FCStd").unwrap();
    assert_eq!(types(&d), ["Sketch", "RevolveFeature", "Sketch", "RevolveFeature", "ChamferFeature"]);
    let m = measured(&d);
    // FreeCAD: 2564.878 mm³; the body is turned 30° about Z and moved 50 mm along X.
    assert!((m[0].volume - 2564.878).abs() < 0.3, "volume {}", m[0].volume);
    assert!(m[0].bbox.min.x > 37.9 && m[0].bbox.max.x < 60.5 && (m[0].bbox.max.z - 8.0).abs() < 1e-6, "{:?}", m[0].bbox);
}

/// A Part workbench primitive is a primitive feature; Rhino files have no history to rebuild.
#[test]
fn freecad_primitives_rebuild_and_rhino_has_no_history() {
    let d = native_cad_design(BLOCK_FCSTD, "block.FCStd").unwrap();
    assert_eq!(types(&d), ["BoxFeature"]);
    assert_eq!(d.doc.features[0].body_names, ["Block"]);
    assert!((measured(&d)[0].volume - 6000.0).abs() < 1e-6);
    let e = native_cad_design(BLOCK_3DM, "block.3dm").err().unwrap();
    assert!(e.contains("no feature history"), "{e}");
}

/// A PartDesign file's solids are its bodies' own shapes: FreeCAD keeps each pad's, pocket's and
/// tool's shape hidden in the file, and those are not imported (they were before).
#[test]
fn freecad_hidden_intermediate_shapes_are_not_imported() {
    let sf = crate::step_import_feature(BRACKET_FCSTD, "bracket.FCStd").unwrap();
    let solidcraft_doc::FeatureKind::Import { skip, .. } = &sf.kind else { panic!("not an Import feature") };
    let built: Vec<_> = sf.import.bodies.iter().zip(0u64..).filter(|(_, i)| !skip.contains(i)).map(|(b, _)| b).collect();
    assert_eq!(built.len(), 1, "{} bodies in the file, {} skipped", sf.import.bodies.len(), skip.len());
    let v = solidcraft_kernel::measure(&built[0].body).unwrap().volume;
    assert!((v - (7840.0 - 50.0 * std::f64::consts::PI)).abs() < 0.05, "volume {v}");
    // A Part workbench file has nothing hidden.
    let sf = crate::step_import_feature(BLOCK_FCSTD, "block.FCStd").unwrap();
    assert!(matches!(&sf.kind, solidcraft_doc::FeatureKind::Import { skip, .. } if skip.is_empty()));
}

/// The history rebuild reads hostile files without a panic.
#[test]
fn hostile_freecad_history_never_panics() {
    for good in [BRACKET_FCSTD, KNOB_FCSTD] {
        for i in (0..good.len()).step_by((good.len() / 40).max(1)) {
            let mut b = good.to_vec();
            b[i] ^= 0x5a;
            let r = std::panic::catch_unwind(|| native_cad_design(&b, "hostile.FCStd"));
            assert!(r.is_ok(), "byte {i} panicked");
        }
        let r = std::panic::catch_unwind(|| native_cad_design(&good[..good.len() / 2], "hostile.FCStd"));
        assert!(r.is_ok());
    }
}

/// A body with holes in its sketch, a pocket along an origin axis, a mirror, polar and linear
/// patterns and a binder rebuilds step for step: FreeCAD's volume, 7818.009 mm³.
#[test]
fn freecad_patterns_and_mirrors_rebuild() {
    let d = native_cad_design(PLATE_FCSTD, "plate.FCStd").unwrap();
    let kinds = types(&d);
    for kind in ["MirrorFeature", "CircularPatternFeature", "RectangularPatternFeature", "FilletFeature"] {
        assert!(kinds.contains(&kind), "{kind}: {kinds:?}");
    }
    assert_eq!(kinds.iter().filter(|k| **k == "Sketch").count(), 4, "the binder makes no feature: {kinds:?}");
    let m = measured(&d);
    assert_eq!(m.len(), 1);
    assert!((m[0].volume - 7818.009).abs() < 0.05, "volume {}", m[0].volume);
}

/// In a file saved from FreeCAD's window the tip feature's shape is shown, and so is a binder;
/// neither is imported beside the body's own shape.
#[test]
fn freecad_shown_tip_and_binders_are_not_imported_twice() {
    let sf = crate::step_import_feature(PLATE_FCSTD, "plate.FCStd").unwrap();
    let solidcraft_doc::FeatureKind::Import { skip, .. } = &sf.kind else { panic!("not an Import feature") };
    let built: Vec<_> = sf.import.bodies.iter().zip(0u64..).filter(|(_, i)| !skip.contains(i)).map(|(b, _)| b).collect();
    assert_eq!(built.len(), 1, "{} bodies in the file, {} skipped", sf.import.bodies.len(), skip.len());
    let v = solidcraft_kernel::measure(&built[0].body).unwrap().volume;
    assert!((v - 7818.009).abs() < 0.05, "volume {v}");
}
