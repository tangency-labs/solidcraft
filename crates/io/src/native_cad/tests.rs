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

/// An 80 × 50 × 5 mm plate whose holes two MultiTransforms repeat: a hole through all along X,
/// along Y and mirrored (18 holes), and a hole 2 mm deep turned about Z then moved along Y (6 holes)
/// (`fixtures/make_grid_fcstd.py`, FreeCAD 1.1.4).
const GRID_FCSTD: &[u8] = include_bytes!("fixtures/grid.FCStd");

/// A 40 × 20 × 5 mm plate with a hole a MultiTransform repeats along X and then scales
/// (`fixtures/make_grid_fcstd.py`, FreeCAD 1.1.4).
const SCALED_FCSTD: &[u8] = include_bytes!("fixtures/scaled.FCStd");

/// STEP text, which these importers refuse.
const STEP_TEXT: &str = "ISO-10303-21;\nHEADER;\nENDSEC;\nDATA;\nENDSEC;\nEND-ISO-10303-21;\n";

/// Both readers of a FreeCAD or Rhino file: the STEP route (the default) and the records.
const READERS: [crate::StepReader; 2] = [crate::StepReader::SolidCraft, crate::StepReader::Cadmpeg];

/// `bytes` (a file called `file`) read into an Import feature by `reader`.
fn read(bytes: &[u8], file: &str, reader: crate::StepReader) -> crate::StepFeature {
    crate::step_import_feature_read(bytes, file, crate::StepNames::default(), reader).unwrap_or_else(|e| panic!("{file} ({reader:?}): {e}"))
}

/// The file imports as one solid with the box's volume, through the same Import feature as STEP.
/// By default cadmpeg's STEP writer restates it and the feature keeps that STEP text, as on main;
/// with the cadmpeg reader the feature keeps the records the file was read into and no STEP text.
fn imports_the_block(bytes: &[u8], file: &str) {
    let step = native_cad_to_step(bytes, file).unwrap().0;
    assert!(step.contains("MANIFOLD_SOLID_BREP"), "{file}: no solid in the restated STEP");
    let default = crate::step_import_feature(bytes, &format!("dir/{file}")).unwrap();
    assert!(
        matches!(&default.kind, solidcraft_doc::FeatureKind::Import { step, records: None, .. } if step.contains("MANIFOLD_SOLID_BREP")),
        "{file}: by default the feature keeps the restated STEP"
    );
    for reader in READERS {
        let sf = read(bytes, &format!("dir/{file}"), reader);
        assert_eq!(sf.import.bodies.len(), 1, "{file} ({reader:?})");
        let solidcraft_doc::FeatureKind::Import { file: held, step, records, .. } = &sf.kind else { panic!("not an Import feature") };
        assert_eq!(held, file);
        match reader {
            crate::StepReader::SolidCraft => {
                assert!(step.contains("MANIFOLD_SOLID_BREP"), "{file}: the feature keeps the restated STEP");
                assert!(!step.contains("cadmpeg_model"), "{file}");
                assert!(records.is_none(), "{file}: no records on the STEP route");
            }
            crate::StepReader::Cadmpeg => {
                assert!(step.is_empty(), "{file}: no STEP text in between");
                assert_eq!(records.as_ref().map(|r| r.bodies.len()), Some(1), "{file}: the feature keeps the records");
            }
        }
        let v = solidcraft_kernel::measure(&sf.import.bodies[0].body).unwrap().volume;
        assert!((v - 6000.0).abs() < 1e-6, "{file} ({reader:?}): volume {v}");
        // A Rhino object keeps its name; FreeCAD's object label does not come through cadmpeg, so
        // the body is named after the file.
        let stem = file.rsplit_once('.').unwrap().0;
        assert!(["Block", stem].contains(&sf.body_names[0].as_str()), "{file} ({reader:?}): {:?}", sf.body_names);
    }
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

#[test]
fn a_file_name_with_umlauts_names_the_product() {
    imports_the_block(BLOCK_FCSTD, "Hülse.FCStd");
    imports_the_block(BLOCK_3DM, "Würfel ä.3dm");
    let step = "#1=PRODUCT('cadmpeg_model','cadmpeg_model','',(#2));".to_string();
    assert_eq!(
        convert::step_route::name_unnamed_products(step, "dir/Hülse Bob's.FCStd"),
        "#1=PRODUCT('H\\X2\\00FC\\X0\\lse Bob''s','H\\X2\\00FC\\X0\\lse Bob''s','',(#2));"
    );
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
        let e = native_cad_records(bytes, name).expect_err(name);
        assert!(e.starts_with(name), "{name}: the message names the file: {e}");
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
            for reader in READERS {
                let r = std::panic::catch_unwind(|| crate::step_import_feature_read(bytes, name, crate::StepNames::default(), reader));
                assert!(r.is_ok(), "{name} ({reader:?}): case {i} panicked");
            }
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

/// A raised "Largest file to open" limit raises every decode cap in proportion, not only the input size.
#[test]
fn a_raised_limit_raises_the_decode_caps() {
    const TEST: &str = "native_cad::tests::a_raised_limit_raises_the_decode_caps";
    const CHILD: &str = "SOLIDCRAFT_IO_RAISED_LIMIT_CHILD";
    // The limit is process-wide, so it is raised in a child process of its own.
    if std::env::var_os(CHILD).is_none() {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", TEST, "--nocapture", "--test-threads=1"])
            .env(CHILD, "1")
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(output.status.success(), "{}\n{stdout}\n{stderr}", output.status);
        assert!(stdout.contains("test result: ok. 1 passed; 0 failed;"), "the child must run the test: {stdout}");
        return;
    }
    crate::limits::set_override(Some(crate::limits::GB)).unwrap();
    let limits = convert::policy().limits;
    let service = cadmpeg_core::decode::ResourceLimits::service();
    assert_eq!(limits.max_input_bytes, crate::limits::GB);
    assert_eq!(limits.max_retained_bytes, 2 * service.max_retained_bytes);
    assert_eq!(limits.max_decompressed_bytes_total, 2 * service.max_decompressed_bytes_total);
    assert_eq!(limits.max_decompressed_bytes_per_expand, 2 * service.max_decompressed_bytes_per_expand);
    assert_eq!(limits.max_entities, 2 * service.max_entities);
    assert_eq!(limits.max_recursion_depth, service.max_recursion_depth);
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
    assert!(native_cad_records(&bytes, "bomb.FCStd").is_err());
    assert!(native_cad_to_step(&bytes, "bomb.FCStd").is_err());
}

/// Records of which not one body can be made are refused as a file with no solids is, with the
/// reason, not imported as an Import feature with no bodies; the same records whole import.
#[test]
fn records_that_make_no_body_are_refused_with_why() {
    let read = native_cad_records(BLOCK_FCSTD, "block.FCStd").unwrap();
    let mut broken = read.clone();
    for b in &mut broken.records.bodies {
        b.face_origin.clear();
    }
    let e = crate::records_import_feature(broken, "block.FCStd", crate::StepNames::default()).err().map(|e| e.to_string());
    assert!(e.as_deref().is_some_and(|e| e.contains("no bodies could be made") && e.contains("face origins")), "{e:?}");
    let f = crate::records_import_feature(read, "block.FCStd", crate::StepNames::default()).unwrap();
    assert!(!f.import.bodies.is_empty());
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
    for reader in READERS {
        let sf = read(BRACKET_FCSTD, "bracket.FCStd", reader);
        let solidcraft_doc::FeatureKind::Import { skip, .. } = &sf.kind else { panic!("not an Import feature") };
        let built: Vec<_> = sf.import.bodies.iter().zip(0u64..).filter(|(_, i)| !skip.contains(i)).map(|(b, _)| b).collect();
        assert_eq!(built.len(), 1, "{reader:?}: {} bodies in the file, {} skipped", sf.import.bodies.len(), skip.len());
        let v = solidcraft_kernel::measure(&built[0].body).unwrap().volume;
        assert!((v - (7840.0 - 50.0 * std::f64::consts::PI)).abs() < 0.05, "{reader:?}: volume {v}");
        // A Part workbench file has nothing hidden.
        let sf = read(BLOCK_FCSTD, "block.FCStd", reader);
        assert!(matches!(&sf.kind, solidcraft_doc::FeatureKind::Import { skip, .. } if skip.is_empty()), "{reader:?}");
    }
}

/// The hidden-shape skip follows the format detected in the file's contents, not its name: a
/// FreeCAD file called .3dm is still a FreeCAD file, so its hidden intermediate shapes are left
/// out; a Rhino file called .FCStd is still a Rhino file, whose hidden objects would import.
#[test]
fn hidden_items_follow_the_detected_format_not_the_file_name() {
    // The STEP route, on the INVISIBILITY the STEP writer writes.
    let step = "#1=MANIFOLD_SOLID_BREP('body',#2);\n#9=INVISIBILITY((#1));\n";
    assert!(crate::native_cad::hidden_items("part.FCStd", BLOCK_FCSTD, step).contains(&1), "a FreeCAD body's hidden shape is left out");
    assert!(crate::native_cad::hidden_items("renamed.3dm", BLOCK_FCSTD, step).contains(&1), "the contents say FreeCAD, not the name");
    assert!(crate::native_cad::hidden_items("renamed.FCStd", BLOCK_3DM, step).is_empty(), "a Rhino file's hidden object still imports");
    assert!(crate::native_cad::hidden_items("part.3dm", BLOCK_3DM, step).is_empty());
    // Both routes, on the files.
    for reader in READERS {
        let skipped = |bytes: &[u8], file: &str| match read(bytes, file, reader).kind {
            solidcraft_doc::FeatureKind::Import { skip, .. } => skip.len(),
            _ => panic!("not an Import feature"),
        };
        assert_eq!(skipped(BRACKET_FCSTD, "bracket.FCStd"), 7, "{reader:?}: a FreeCAD body's hidden shapes are left out");
        assert_eq!(skipped(BRACKET_FCSTD, "renamed.3dm"), 7, "{reader:?}: the contents say FreeCAD, not the name");
        assert_eq!(skipped(BLOCK_3DM, "renamed.FCStd"), 0, "{reader:?}: a Rhino file leaves nothing out");
    }
}

/// FreeCAD places a PartDesign body's shape by the body's placement: the knob, placed off the
/// origin and turned, comes in where FreeCAD shows it (the file's own history, rebuilt, agrees).
#[test]
fn a_placed_freecad_body_imports_where_freecad_shows_it() {
    for reader in READERS {
        let sf = read(KNOB_FCSTD, "knob.FCStd", reader);
        let solidcraft_doc::FeatureKind::Import { skip, .. } = &sf.kind else { panic!("not an Import feature") };
        let built: Vec<_> = sf.import.bodies.iter().zip(0u64..).filter(|(_, i)| !skip.contains(i)).map(|(b, _)| b).collect();
        assert_eq!(built.len(), 1, "{reader:?}");
        let m = solidcraft_kernel::measure(&built[0].body).unwrap();
        assert!((m.bbox.min.x - 38.0).abs() < 1e-6 && (m.bbox.min.y + 12.0).abs() < 1e-6, "{reader:?}: {:?}", m.bbox);
        assert!((m.volume - 2564.8775).abs() < 1e-3, "{reader:?}: volume {}", m.volume);
    }
}

/// A FreeCAD design is detected from its contents, not from the file's name.
#[test]
fn detects_freecad_from_contents() {
    assert!(detects_freecad(BLOCK_FCSTD), "a FreeCAD design");
    assert!(detects_freecad(BRACKET_FCSTD), "a FreeCAD design built from sketches");
    assert!(!detects_freecad(BLOCK_3DM), "a Rhino file");
    assert!(!detects_freecad(b"not a CAD file"), "unrecognised contents");
}

/// The history rebuild reads hostile files without a panic.
#[test]
fn hostile_freecad_history_never_panics() {
    for good in [BRACKET_FCSTD, KNOB_FCSTD, GRID_FCSTD] {
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

/// `bytes` (a FreeCAD file) with its `Document.xml` passed through `edit`.
fn with_document(bytes: &[u8], edit: impl Fn(&str) -> String) -> Vec<u8> {
    use std::io::{Read, Write};
    let mut from = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    let mut to = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for i in 0..from.len() {
        let mut entry = from.by_index(i).unwrap();
        let name = entry.name().unwrap().to_string();
        let mut data = Vec::new();
        entry.read_to_end(&mut data).unwrap();
        if name == "Document.xml" {
            data = edit(&String::from_utf8(data).unwrap()).into_bytes();
        }
        to.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
        to.write_all(&data).unwrap();
    }
    to.finish().unwrap().into_inner()
}

/// A MultiTransform whose steps are missing, repeated, out of order or far too many never
/// panics and is not rebuilt: the file's solid imports instead.
#[test]
fn hostile_multitransform_steps_never_panic() {
    let steps = "<LinkList count=\"3\">\n                        <Link value=\"AlongX\"/>\n                        <Link value=\"AlongY\"/>\n                        <Link value=\"AcrossYZ\"/>";
    let many = format!("<LinkList count=\"1000\">{}", "<Link value=\"AlongX\"/>".repeat(1000));
    let cases = [
        "<LinkList count=\"3\"><Link value=\"AlongX\"/><Link value=\"Nowhere\"/><Link value=\"AcrossYZ\"/>".to_string(),
        "<LinkList count=\"3\"><Link value=\"AcrossYZ\"/><Link value=\"AlongY\"/><Link value=\"AlongX\"/>".to_string(),
        "<LinkList count=\"3\"><Link value=\"AlongX\"/><Link value=\"AlongX\"/><Link value=\"GridHoles\"/>".to_string(),
        "<LinkList count=\"0\"><Link/><Link/><Link/>".to_string(),
        many,
    ];
    for (i, case) in cases.iter().enumerate() {
        let bytes = with_document(GRID_FCSTD, |xml| {
            assert!(xml.contains(steps), "the fixture lists GridHoles' steps");
            xml.replacen(steps, case, 1)
        });
        let r = std::panic::catch_unwind(|| native_cad_design(&bytes, "hostile.FCStd"));
        // Reordered steps build other holes than FreeCAD's stored solid, so the comparison
        // refuses them; the other cases are refused as they are read.
        assert!(r.as_ref().is_ok_and(Result::is_err), "case {i}: {:?}", r.map(|d| d.map(|d| types(&d))));
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

/// A MultiTransform rebuilds as one Mirror or Pattern per step, each copying the features and the
/// steps before it, so each step applies to every copy made so far. The steps make no feature of
/// their own. FreeCAD's volume is 20000 − 18·π·1.5²·5 − 6·π·1²·2 mm³; the centre (off the X axis,
/// as turning before moving puts the holes) matches FreeCAD's too.
#[test]
fn freecad_multitransforms_rebuild_as_chained_patterns() {
    let d = native_cad_design(GRID_FCSTD, "grid.FCStd").unwrap();
    let names: Vec<&str> = d.doc.features.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(names, ["Profile", "Pad", "GridHoleSketch", "GridHole", "AlongX", "AlongY", "GridHoles", "SpokeSketch", "Spoke", "AboutZ", "Spokes"]);
    let kinds = types(&d);
    assert_eq!(
        kinds[4..7],
        ["RectangularPatternFeature", "RectangularPatternFeature", "MirrorFeature"],
        "along X, along Y, then mirrored: {kinds:?}"
    );
    assert_eq!(kinds[9..], ["CircularPatternFeature", "RectangularPatternFeature"], "{kinds:?}");
    // Each step copies the hole and every step before it.
    let copied = |name: &str| -> Vec<&str> {
        let f = d.doc.features.iter().find(|f| f.name == name).unwrap();
        let ids = match &f.kind {
            solidcraft_doc::FeatureKind::Pattern { features, .. } | solidcraft_doc::FeatureKind::Mirror { features, .. } => features,
            other => panic!("{name}: {other:?}"),
        };
        ids.iter().map(|id| d.doc.feature(id.0).unwrap().name.as_str()).collect()
    };
    assert_eq!(copied("AlongX"), ["GridHole"]);
    assert_eq!(copied("AlongY"), ["GridHole", "AlongX"]);
    assert_eq!(copied("GridHoles"), ["GridHole", "AlongX", "AlongY"]);
    assert_eq!(copied("Spokes"), ["Spoke", "AboutZ"]);
    let m = measured(&d);
    assert_eq!(m.len(), 1);
    let pi = std::f64::consts::PI;
    let expected = 20000.0 - 18.0 * pi * 1.5 * 1.5 * 5.0 - 6.0 * pi * 2.0;
    assert!((m[0].volume - expected).abs() < 0.05, "volume {} vs {expected}", m[0].volume);
    assert!((m[0].volume - 19326.128).abs() < 0.05, "FreeCAD's volume: {}", m[0].volume);
    let c = m[0].centroid;
    assert!(c.x.abs() < 1e-3 && (c.y - 0.38331).abs() < 1e-3 && (c.z - 2.49707).abs() < 1e-3, "FreeCAD's centre: {c:?}");
}

/// A MultiTransform with a Scaled step is not rebuilt: the error names the step, and the file's
/// solid still imports.
#[test]
fn a_freecad_scaled_step_is_not_rebuilt() {
    let e = native_cad_design(SCALED_FCSTD, "scaled.FCStd").err().unwrap();
    assert!(e.contains("Scaled step (Grow)"), "{e}");
    let f = crate::step_import_feature(SCALED_FCSTD, "scaled.FCStd").unwrap();
    let solidcraft_doc::FeatureKind::Import { skip, .. } = &f.kind else { panic!("not an Import feature") };
    let built: Vec<_> = f.import.bodies.iter().zip(0u64..).filter(|(_, i)| !skip.contains(i)).map(|(b, _)| b).collect();
    assert_eq!(built.len(), 1, "{} bodies in the file, {} skipped", f.import.bodies.len(), skip.len());
    let v = solidcraft_kernel::measure(&built[0].body).unwrap().volume;
    assert!((v - 3956.018).abs() < 0.05, "FreeCAD's volume: {v}");
}

/// In a file saved from FreeCAD's window the tip feature's shape is shown, and so is a binder;
/// neither is imported beside the body's own shape.
#[test]
fn freecad_shown_tip_and_binders_are_not_imported_twice() {
    for reader in READERS {
        let sf = read(PLATE_FCSTD, "plate.FCStd", reader);
        let solidcraft_doc::FeatureKind::Import { skip, .. } = &sf.kind else { panic!("not an Import feature") };
        let built: Vec<_> = sf.import.bodies.iter().zip(0u64..).filter(|(_, i)| !skip.contains(i)).map(|(b, _)| b).collect();
        assert_eq!(built.len(), 1, "{reader:?}: {} bodies in the file, {} skipped", sf.import.bodies.len(), skip.len());
        let v = solidcraft_kernel::measure(&built[0].body).unwrap().volume;
        assert!((v - 7818.009).abs() < 0.05, "{reader:?}: volume {v}");
    }
}

/// The route a FreeCAD or Rhino file takes by default (the STEP route), on the neutral model, to
/// compare with the records: cadmpeg's STEP writer (AP214), then SolidCraft's STEP reader. Returns the
/// import, or why there is none, and the writer's losses.
fn by_step_text(ir: &cadmpeg_ir::document::CadIr) -> (Result<solidcraft_kernel::StepImport, String>, Vec<String>) {
    use cadmpeg_ir::codec::write::{EncodeInput, target::TargetRequest};
    let plan =
        match cadmpeg_registry::build_encoder(cadmpeg_registry::Format::Step).plan(EncodeInput::new(ir, None), TargetRequest::Explicit("ap214")) {
            Ok(plan) => plan,
            Err(e) => return (Err(e.to_string()), Vec::new()),
        };
    let losses: Vec<String> = plan.report().losses.iter().map(|l| format!("{}: {}", l.code, l.message)).collect();
    let mut out = Vec::new();
    if let Err(e) = plan.write_to(&mut out) {
        return (Err(e.to_string()), losses);
    }
    (solidcraft_kernel::step_import(&String::from_utf8_lossy(&out)).map_err(|e| e.to_string()), losses)
}

/// The route they take with the cadmpeg reader: the neutral model read into records, the records made into bodies.
fn by_records(ir: &cadmpeg_ir::document::CadIr) -> (Result<solidcraft_kernel::StepImport, String>, Vec<String>) {
    let records = cadir_records(ir, "compared", "test", true);
    let warnings = records.warnings.clone();
    (solidcraft_kernel::records_import(&records).map_err(|e| e.to_string()), warnings)
}

/// A file decoded by cadmpeg into its neutral model: FreeCAD, Rhino or (for comparison) STEP.
fn neutral(bytes: &[u8], name: &str) -> cadmpeg_ir::document::CadIr {
    let ext = name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
    let (_, decoded) = convert::decode_at(name, &ext, bytes, &["fcstd", "rhino", "step"], cadmpeg_core::decode::DecodeDepth::Full).unwrap();
    decoded.into_parts().0
}

/// What an import built, body by body, ordered by volume: whether it is whole (not a surface body
/// of the faces that read), its faces, volume and bounding box.
fn built(imp: &solidcraft_kernel::StepImport) -> Vec<(bool, usize, f64, [f64; 6])> {
    let mut out: Vec<_> = imp
        .bodies
        .iter()
        .map(|b| {
            let m = solidcraft_kernel::measure(&b.body).unwrap();
            (b.refused.is_none(), m.faces, m.volume, [m.bbox.min.x, m.bbox.min.y, m.bbox.min.z, m.bbox.max.x, m.bbox.max.y, m.bbox.max.z])
        })
        .collect();
    // By volume and then box, each to a thousandth, so two bodies of nearly one volume (a part
    // placed twice) keep one order whatever the last digits of their volumes are.
    let key = |b: &(bool, usize, f64, [f64; 6])| {
        let mut k = vec![(b.2 * 1e3).round() as i64];
        k.extend(b.3.iter().map(|v| (v * 1e3).round() as i64));
        k
    };
    out.sort_by_key(key);
    out
}

/// Whether two imports built the same bodies: the same number, each as whole, with as many faces,
/// the same volume and the same box (to a millionth of the part's size).
fn same_bodies(a: &[(bool, usize, f64, [f64; 6])], b: &[(bool, usize, f64, [f64; 6])]) -> bool {
    a.len() == b.len()
        && a.iter().zip(b).all(|(a, b)| {
            let size = a.3.iter().chain(&b.3).fold(1.0f64, |m, v| m.max(v.abs()));
            a.0 == b.0
                && a.1 == b.1
                && (a.2 - b.2).abs() <= 1e-6 * a.2.abs().max(b.2.abs()).max(1.0)
                && a.3.iter().zip(&b.3).all(|(x, y)| (x - y).abs() <= 1e-6 * size)
        })
}

/// The parts compared between the two routes: every FreeCAD and Rhino fixture, and every
/// step-conformance part decoded by cadmpeg's STEP decoder (as the cadmpeg STEP reader reads it),
/// and the import kernel cases whose surfaces the records once read differently
/// ([`KERNEL_CASES`]).
fn compared_parts() -> Vec<(String, Vec<u8>)> {
    let mut parts: Vec<(String, Vec<u8>)> = [
        ("block.FCStd", BLOCK_FCSTD),
        ("block.3dm", BLOCK_3DM),
        ("bracket.FCStd", BRACKET_FCSTD),
        ("knob.FCStd", KNOB_FCSTD),
        ("plate.FCStd", PLATE_FCSTD),
    ]
    .into_iter()
    .map(|(n, b)| (n.to_string(), b.to_vec()))
    .collect();
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/step-conformance");
    let mut steps: Vec<_> =
        std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().path()).filter(|p| p.extension().is_some_and(|e| e == "step")).collect();
    steps.sort();
    assert!(steps.len() >= 17, "{} step-conformance parts", steps.len());
    parts.extend(steps.iter().map(|p| (p.file_name().unwrap().to_string_lossy().to_string(), std::fs::read(p).unwrap())));
    let cases = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/kernel-cases/import");
    parts.extend(KERNEL_CASES.iter().map(|c| (format!("{c}.step"), std::fs::read(cases.join(c).join("input.step")).unwrap())));
    parts
}

/// Import kernel cases on surfaces whose record form depends on the face, which the records route
/// once refused: a SURFACE_OF_LINEAR_EXTRUSION of an ellipse across the ellipse's parameter origin
/// (cadmpeg reads it as a procedural surface with no solved form), and a cone of nearly a right
/// angle that is a plane.
const KERNEL_CASES: [&str; 2] = ["3208-extruded-ellipse-face-over-parameter-seam", "4006-plane-written-as-cone-of-nearly-right-angle"];

/// Reading the neutral model into records builds the bodies the STEP route built (cadmpeg's
/// STEP writer, then our STEP reader): the same bodies, each as whole, with as many faces, the
/// same volume and the same box, for every part in [`compared_parts`], an assembly's parts at
/// each of their placements.
#[test]
fn records_build_the_bodies_the_step_route_built() {
    let mut differ = Vec::new();
    for (name, bytes) in &compared_parts() {
        let ir = neutral(bytes, name);
        let (old, _) = by_step_text(&ir);
        let (new, warnings) = by_records(&ir);
        let old = built(&old.unwrap_or_else(|e| panic!("{name}: the STEP route: {e}")));
        let new = built(&new.unwrap_or_else(|e| panic!("{name}: the records: {e} {warnings:?}")));
        if !same_bodies(&old, &new) {
            differ.push(name.clone());
        }
    }
    assert!(differ.is_empty(), "parts whose bodies differ between the two routes: {differ:?}");
}

/// An assembly read through cadmpeg's STEP decoder has the product tree SolidCraft's own STEP
/// reader gives it: the same products nested the same way, each placed in its parent, with its
/// bodies, and each body under its products' names and its occurrence's. A part used twice is
/// two bodies, one at each placement.
#[test]
fn an_assembly_read_into_records_has_the_tree_our_step_reader_gives_it() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/step-conformance");
    for (name, bodies) in [("cylinder_placed.step", 1), ("cylinder_twice.step", 2)] {
        let bytes = std::fs::read(dir.join(name)).unwrap();
        let ours = solidcraft_kernel::step_import(&String::from_utf8_lossy(&bytes)).unwrap();
        let (theirs, warnings) = by_records(&neutral(&bytes, name));
        let theirs = theirs.unwrap_or_else(|e| panic!("{name}: {e} {warnings:?}"));
        assert_eq!(theirs.bodies.len(), bodies, "{name}");
        assert_eq!(tree_of(&theirs), tree_of(&ours), "{name}");
        assert!(same_bodies(&built(&theirs), &built(&ours)), "{name}: the bodies differ from our reader's");
        // The same part placed twice is one part to the design, as with our reader.
        let parts = |imp: &solidcraft_kernel::StepImport| imp.bodies.iter().map(|b| b.source_item).collect::<std::collections::BTreeSet<_>>().len();
        assert_eq!(parts(&theirs), parts(&ours), "{name}: parts");
    }
}

/// An import's product tree, one line a node: its depth and name, its placement in its parent
/// (rounded to a micrometre), and each of its bodies' product path and occurrence.
fn tree_of(imp: &solidcraft_kernel::StepImport) -> Vec<String> {
    fn walk(nodes: &[solidcraft_kernel::StepNode], imp: &solidcraft_kernel::StepImport, depth: usize, out: &mut Vec<String>) {
        for n in nodes {
            let m: Vec<String> = n.transform.iter().flatten().map(|v| format!("{:.3}", v + 0.0)).collect();
            let bodies: Vec<String> = n
                .bodies
                .iter()
                .filter_map(|b| imp.bodies.get(*b))
                .map(|b| format!("{}|{}", b.path.join("/"), b.instance.clone().unwrap_or_default()))
                .collect();
            out.push(format!("{depth} {} [{}] {bodies:?}", n.name, m.join(" ")));
            walk(&n.children, imp, depth + 1, out);
        }
    }
    let mut out = Vec::new();
    walk(&imp.tree, imp, 0, &mut out);
    out
}

/// The import kernel cases read into records build what each case expects: one whole body, its
/// faces and volume as `expected.json` gives them.
#[test]
fn kernel_cases_read_into_records_build_their_expected_bodies() {
    let cases = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/kernel-cases/import");
    for c in KERNEL_CASES {
        let expected: serde_json::Value = serde_json::from_slice(&std::fs::read(cases.join(c).join("expected.json")).unwrap()).unwrap();
        let ir = neutral(&std::fs::read(cases.join(c).join("input.step")).unwrap(), &format!("{c}.step"));
        let (imp, warnings) = by_records(&ir);
        let bodies = built(&imp.unwrap_or_else(|e| panic!("{c}: {e} {warnings:?}")));
        assert_eq!(bodies.len(), 1, "{c}: {bodies:?} {warnings:?}");
        let (whole, faces, volume, _) = bodies[0];
        assert!(whole, "{c}: {warnings:?}");
        assert_eq!(faces as u64, expected["faces"].as_u64().unwrap(), "{c}");
        let want = expected["volume_mm3"].as_f64().unwrap();
        assert!((volume - want).abs() < 1e-3 * want, "{c}: {volume} vs {want}");
    }
}

/// What the STEP route did with what the records now leave out, each with the reason. A body placed
/// by a mirror: cadmpeg's STEP writer could not write the placement, so the body came in unplaced
/// (a loss noted, the geometry in the wrong place). An elliptical cone: the writer wrote a circular
/// cone, its ratio dropped, so the body came in as if the cone were round.
#[test]
fn what_the_records_leave_out_the_step_route_imported_wrongly() {
    // The 10 × 20 × 30 mm block, mirrored in x and moved 100 mm along it: it belongs at x 90..100.
    let mut ir = neutral(BLOCK_FCSTD, "block.FCStd");
    let unplaced = built(&by_step_text(&ir).0.unwrap());
    for b in &mut ir.model.bodies {
        b.transform = Some(cadmpeg_ir::transform::Transform::affine([[-1.0, 0.0, 0.0, 100.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0]]).unwrap());
    }
    let (old, losses) = by_step_text(&ir);
    assert!(same_bodies(&built(&old.unwrap()), &unplaced), "the STEP route imported the mirrored block where it was before the move");
    assert!(!losses.is_empty(), "with a loss noted");
    let (new, warnings) = by_records(&ir);
    assert!(new.is_ok_and(|imp| imp.bodies.is_empty()), "{warnings:?}");
    assert!(warnings.iter().any(|w| w.contains("not imported: its placement:")), "{warnings:?}");

    // The knob's chamfer, a cone, made elliptical (the ratio of its minor to its major radius halved).
    let ir = neutral(KNOB_FCSTD, "knob.FCStd");
    let round = built(&by_step_text(&ir).0.unwrap());
    let mut json = serde_json::to_value(&ir).unwrap();
    fn halve_ratio(v: &mut serde_json::Value) -> usize {
        match v {
            serde_json::Value::Object(m) if m.contains_key("half_angle") && m.contains_key("ratio") => {
                m.insert("ratio".into(), 0.5.into());
                1
            }
            serde_json::Value::Object(m) => m.values_mut().map(halve_ratio).sum(),
            serde_json::Value::Array(a) => a.iter_mut().map(halve_ratio).sum(),
            _ => 0,
        }
    }
    assert!(halve_ratio(&mut json) > 0, "a cone in the model");
    let ir: cadmpeg_ir::document::CadIr = serde_json::from_value(json).unwrap();
    let (old, _) = by_step_text(&ir);
    assert!(same_bodies(&built(&old.unwrap()), &round), "the STEP route imported the elliptical cone as the round one");
    let (_, warnings) = by_records(&ir);
    assert!(warnings.iter().any(|w| w.contains("an elliptical cone")), "{warnings:?}");
}

/// A component whose placement mirrors is left out with the reason, and the part's other
/// placement still comes in.
#[test]
fn a_mirrored_component_is_left_out_with_why() {
    let bytes = std::fs::read(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/step-conformance/cylinder_twice.step")).unwrap();
    let mut ir = neutral(&bytes, "cylinder_twice.step");
    let mirror = cadmpeg_ir::transform::Transform::affine([[-1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0]]).unwrap();
    let leaf = ir.model.occurrences.iter_mut().find(|o| !matches!(o.parent, cadmpeg_ir::products::OccurrenceParent::Root {})).unwrap();
    leaf.transform = mirror;
    let records = cadir_records(&ir, "mirrored", "test", true);
    assert_eq!(records.bodies.len(), 1, "{:?}", records.warnings);
    assert!(records.warnings.iter().any(|w| w.contains("mirrors")), "{:?}", records.warnings);
}

/// A part whose every placement is left out (each mirrored, or the assembly that holds them)
/// does not come in at all, rather than where its file defines it, which the warning would deny.
#[test]
fn a_part_whose_every_placement_is_left_out_does_not_come_in() {
    use cadmpeg_ir::products::{Occurrence, OccurrenceParent};
    let bytes = std::fs::read(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/step-conformance/cylinder_twice.step")).unwrap();
    let mirror = cadmpeg_ir::transform::Transform::affine([[-1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0]]).unwrap();
    let leaves = |o: &Occurrence| !matches!(o.parent, OccurrenceParent::Root {});
    let root = |o: &Occurrence| matches!(o.parent, OccurrenceParent::Root {});
    for (which, refused) in [("every placement", &leaves as &dyn Fn(&Occurrence) -> bool), ("the assembly", &root)] {
        let mut ir = neutral(&bytes, "cylinder_twice.step");
        for o in ir.model.occurrences.iter_mut().filter(|o| refused(o)) {
            o.transform = mirror;
        }
        let (imp, warnings) = by_records(&ir);
        let imp = imp.unwrap();
        assert!(imp.bodies.is_empty(), "{which} mirrored: {} bodies", imp.bodies.len());
        assert!(warnings.iter().any(|w| w.contains("mirrors")), "{which} mirrored: {warnings:?}");
    }
}

/// Occurrences that contain each other end the walk with a warning, never a hang or a panic.
#[test]
fn occurrences_in_a_cycle_end_with_a_warning() {
    let bytes = std::fs::read(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/step-conformance/cylinder_twice.step")).unwrap();
    let mut ir = neutral(&bytes, "cylinder_twice.step");
    let ids: Vec<_> = ir.model.occurrences.iter().map(|o| o.id.clone()).collect();
    // Each occurrence's parent the next: a cycle with no root, and one with the root kept.
    for (k, o) in ir.model.occurrences.iter_mut().enumerate() {
        o.parent = cadmpeg_ir::products::OccurrenceParent::Occurrence { occurrence: ids[(k + 1) % ids.len()].clone() };
    }
    let records = cadir_records(&ir, "cycle", "test", true);
    assert!(solidcraft_kernel::records_import(&records).is_ok());
    if let Some(first) = ir.model.occurrences.first_mut() {
        first.parent = cadmpeg_ir::products::OccurrenceParent::Root {};
    }
    let records = cadir_records(&ir, "cycle", "test", true);
    assert!(solidcraft_kernel::records_import(&records).is_ok(), "{:?}", records.warnings);
}

/// The washer with a named colour on the solid, a see-through named colour on one face, and a
/// pre-defined colour (`red`) on another: the see-through one's
/// transparency in a SURFACE_STYLE_RENDERING_WITH_PROPERTIES, as the CAx-IF recommended practice
/// for colours writes it.
fn coloured_washer() -> String {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/step-conformance");
    let text = std::fs::read_to_string(dir.join("washer.step")).unwrap();
    let style = |id: u64, item: &str, colour: &str, extra: &str| {
        let n = |k: u64| id + k;
        format!(
            "#{}=STYLED_ITEM('',(#{}),{item});\n#{}=PRESENTATION_STYLE_ASSIGNMENT((#{}));\n#{}=SURFACE_STYLE_USAGE(.BOTH.,#{});\n#{}=SURFACE_SIDE_STYLE('',(#{}{extra}));\n#{}=SURFACE_STYLE_FILL_AREA(#{});\n#{}=FILL_AREA_STYLE('',(#{}));\n#{}=FILL_AREA_STYLE_COLOUR('',#{});\n#{}={colour};\n",
            n(0),
            n(1),
            n(1),
            n(2),
            n(2),
            n(3),
            n(3),
            n(4),
            n(4),
            n(5),
            n(5),
            n(6),
            n(6),
            n(7),
            n(7)
        )
    };
    let styles = style(200, "#99", "COLOUR_RGB('Body grey',0.5,0.5,0.5)", "")
        + &style(220, "#73", "COLOUR_RGB('Accent',0.,0.,1.)", ",#240")
        + &style(250, "#85", "DRAUGHTING_PRE_DEFINED_COLOUR('red')", "")
        + "#240=SURFACE_STYLE_RENDERING_WITH_PROPERTIES(.NORMAL_SHADING.,#227,(#242));\n#242=SURFACE_STYLE_TRANSPARENT(0.25);\n#241=MECHANICAL_DESIGN_GEOMETRIC_PRESENTATION_REPRESENTATION('',(#200,#220,#250),#5);\n";
    let at = text.rfind("ENDSEC;").unwrap();
    format!("{}{styles}{}", &text[..at], &text[at..])
}

/// A STEP file's colours read through cadmpeg come in as SolidCraft's reader brings them in: the
/// body's colour and its name, and each face's own colour, opacity and name, a pre-defined
/// colour's name being its own.
#[test]
fn colours_read_into_records_are_the_ones_our_step_reader_reads() {
    let text = coloured_washer();
    let ours = solidcraft_kernel::step_import(&text).unwrap();
    let (theirs, warnings) = by_records(&neutral(text.as_bytes(), "washer.step"));
    let theirs = theirs.unwrap_or_else(|e| panic!("{e} {warnings:?}"));
    let look = |imp: &solidcraft_kernel::StepImport| {
        let b = &imp.bodies[0].body;
        let p = b.paint().cloned().unwrap_or(solidcraft_kernel::Paint { opacity: 1.0, faces: Vec::new(), name: None });
        let mut faces: Vec<String> = p.faces.iter().map(|f| format!("{:?} {:.3} {:?}", f.color, f.opacity, f.name)).collect();
        faces.sort();
        (b.color(), p.name, format!("{:.3}", p.opacity), faces)
    };
    let (o, t) = (look(&ours), look(&theirs));
    assert_eq!(o.0, Some([0.5, 0.5, 0.5]), "our reader: {o:?}");
    assert_eq!(o.3.len(), 2, "our reader: {o:?}");
    assert_eq!(t, o);
}

/// An IGES file read by either reader: by SolidCraft's the Import feature keeps the file restated
/// as STEP text, by cadmpeg's (its IGES decoder) the records the neutral model was read into,
/// with no STEP text in between, as a STEP file read by cadmpeg is kept.
fn read_iges(bytes: &[u8], file: &str, reader: crate::StepReader) -> crate::StepFeature {
    let sf = read(bytes, file, reader);
    let solidcraft_doc::FeatureKind::Import { file: held, step, records, .. } = &sf.kind else { panic!("{file}: not an Import feature") };
    assert_eq!(held, std::path::Path::new(file).file_name().unwrap().to_str().unwrap());
    match reader {
        crate::StepReader::SolidCraft => {
            assert!(step.starts_with("ISO-10303-21"), "{file}: the feature keeps the IGES file restated as STEP");
            assert!(records.is_none(), "{file}: no records with SolidCraft's reader");
        }
        crate::StepReader::Cadmpeg => {
            assert!(step.is_empty(), "{file}: no STEP text in between");
            assert_eq!(records.as_ref().map(|r| r.bodies.len()), Some(sf.import.bodies.len()), "{file}: the feature keeps the records");
        }
    }
    sf
}

/// A solid of known size in an IGES file: its name, colour, volume, area and box.
struct KnownSolid {
    name: &'static str,
    color: Option<[f32; 3]>,
    volume: f64,
    area: f64,
    bbox: [f64; 6],
}

/// Three manifold solid B-reps (186) of known size, each with its name and the first two with a
/// colour (314), as SolidCraft's IGES export writes them: a 10 × 20 × 30 mm box (V = 6000,
/// A = 2(200 + 300 + 600) = 2200), a cylinder of radius 5 and height 12 on the XY plane
/// (V = 300π, A = 2π·5·12 + 2·25π = 170π) and a ball of radius 10 at (50, 0, 0) (V = 4000π/3,
/// A = 400π). Each colour definition is made as IGES 5.3 (4.77) has it, which cadmpeg's decoder
/// requires and SolidCraft's export does not yet write: its status a definition (use flag 02),
/// and its empty name left out rather than written as a zero-length Hollerith string (`0H`).
fn known_solids_iges() -> (String, [KnownSolid; 3]) {
    use solidcore_record::Vec3;
    use std::f64::consts::PI;
    let block = solidcraft_kernel::box_solid(Vec3::ZERO, Vec3::new(10.0, 20.0, 30.0)).unwrap();
    let pin = solidcraft_kernel::cylinder(Vec3::new(30.0, 0.0, 0.0), Vec3::Z, 5.0, 12.0).unwrap();
    let ball = solidcraft_kernel::sphere(Vec3::new(50.0, 0.0, 0.0), 10.0).unwrap();
    let known = [
        KnownSolid { name: "Block", color: Some([1.0, 0.5, 0.0]), volume: 6000.0, area: 2200.0, bbox: [0.0, 0.0, 0.0, 10.0, 20.0, 30.0] },
        KnownSolid { name: "Pin", color: Some([0.0, 0.0, 1.0]), volume: 300.0 * PI, area: 170.0 * PI, bbox: [25.0, -5.0, 0.0, 35.0, 5.0, 12.0] },
        KnownSolid { name: "Ball", color: None, volume: 4000.0 * PI / 3.0, area: 400.0 * PI, bbox: [40.0, -10.0, -10.0, 60.0, 10.0, 10.0] },
    ];
    let bodies: Vec<solidcraft_kernel::ExportBody> = [&block, &pin, &ball]
        .iter()
        .zip(&known)
        .map(|(body, k)| solidcraft_kernel::ExportBody { name: k.name.into(), body, color: k.color })
        .collect();
    let header = solidcraft_kernel::StepHeader { file_name: "solids.igs".into(), ..Default::default() };
    let text = solidcraft_kernel::iges_export_bodies(&bodies, &header).unwrap();
    let text: String = text
        .lines()
        .map(|l| {
            let section = l.get(72..73);
            let seq: u64 = l.get(73..).and_then(|s| s.trim().parse().ok()).unwrap_or(0);
            if section == Some("P") && l.contains(",0H;") {
                // A parameter line ending in an empty name (`,0H;`) ends with the field left out.
                format!("{}\n", l.replacen(",0H;", ";   ", 1))
            } else if section == Some("D") && seq % 2 == 1 && l.get(..8).is_some_and(|t| t.trim() == "314") {
                format!("{}00000200{}\n", &l[..64], &l[72..])
            } else {
                format!("{l}\n")
            }
        })
        .collect();
    (text, known)
}

/// IGES solids come in with their names, colours, volumes, areas and boxes through either reader,
/// the reader chosen as for STEP: SolidCraft's keeps the STEP it restates the file as, cadmpeg's
/// IGES decoder the records.
#[test]
fn iges_solids_keep_their_names_colours_and_sizes_with_either_reader() {
    let (text, known) = known_solids_iges();
    for reader in READERS {
        let sf = read_iges(text.as_bytes(), "parts/solids.IGS", reader);
        assert_eq!(sf.import.bodies.len(), known.len(), "{reader:?}: {:?}", sf.warnings);
        for k in &known {
            let what = format!("{} ({reader:?})", k.name);
            let i = sf.body_names.iter().position(|n| n == k.name).unwrap_or_else(|| panic!("{what}: names {:?}", sf.body_names));
            let b = &sf.import.bodies[i];
            assert!(b.refused.is_none(), "{what}: a whole solid");
            assert_eq!(b.body.color(), k.color, "{what}: {:?}", sf.warnings);
            let m = solidcraft_kernel::measure(&b.body).unwrap();
            assert!((m.volume - k.volume).abs() < 1e-3 * k.volume, "{what}: volume {} vs {}", m.volume, k.volume);
            assert!((m.area - k.area).abs() < 1e-3 * k.area, "{what}: area {} vs {}", m.area, k.area);
            let got = [m.bbox.min.x, m.bbox.min.y, m.bbox.min.z, m.bbox.max.x, m.bbox.max.y, m.bbox.max.z];
            assert!(got.iter().zip(k.bbox).all(|(g, w)| (g - w).abs() < 1e-3), "{what}: box {got:?} vs {:?}", k.bbox);
        }
    }
    let by = READERS.map(|r| built(&read(text.as_bytes(), "solids.igs", r).import));
    assert!(same_bodies(&by[0], &by[1]), "the two readers built different bodies: {:?} vs {:?}", by[0], by[1]);
}

/// The IGES import kernel cases: boxes of trimmed planes (one, or two apart), a dome of a
/// revolved arc and a patch with a square hole, each of a size `expected.json` derives by hand.
const IGES_CASES: [&str; 7] = [
    "3359-iges-trim-pieces-written-end-to-start",
    "3806-iges-revolved-arc-spherical-cap-dome",
    "3845-iges-trim-pieces-miss-by-a-gap",
    "3886-iges-bspline-curve-over-tiny-knot-span",
    "3912-iges-hole-trim-first-piece-reversed",
    "3913-iges-trim-curve-first-piece-reversed",
    "3972-iges-two-separate-closed-boxes",
];

/// Each IGES kernel case read by SolidCraft's reader builds what the case expects (its bodies,
/// faces, area, volume and, where given, box). cadmpeg's IGES decoder (0.6.12) projects none of
/// these hand-written trimmed surfaces (144): it takes a composite curve's pieces and a trimmed
/// surface's carrier only when their directory entries mark them physically dependent, and does
/// not sew trimmed surfaces into a solid, so with the cadmpeg reader the file is refused with a
/// message naming it, never imported as something else; should cadmpeg read one, it must build
/// the same bodies.
#[test]
fn iges_kernel_cases_with_either_reader() {
    let cases = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/kernel-cases/import");
    for c in IGES_CASES {
        let expected: serde_json::Value = serde_json::from_slice(&std::fs::read(cases.join(c).join("expected.json")).unwrap()).unwrap();
        let bytes = std::fs::read(cases.join(c).join("input.igs")).unwrap();
        let file = format!("dir/{c}.igs");
        let sf = read_iges(&bytes, &file, crate::StepReader::SolidCraft);
        let measured: Vec<_> = sf.import.bodies.iter().map(|b| solidcraft_kernel::measure(&b.body).unwrap()).collect();
        let what = format!("{c}: {:?}", sf.warnings);
        assert_eq!(measured.len() as u64, expected["body_count"].as_u64().unwrap(), "{what}");
        assert_eq!(measured.iter().map(|m| m.faces as u64).sum::<u64>(), expected["faces"].as_u64().unwrap(), "{what}");
        let area: f64 = measured.iter().map(|m| m.area).sum();
        let want = expected["area_mm2"].as_f64().unwrap();
        assert!((area - want).abs() < 1e-3 * want, "{what}: area {area} vs {want}");
        if let Some(want) = expected["volume_mm3"].as_f64() {
            let volume: f64 = measured.iter().map(|m| m.volume).sum();
            assert!((volume - want).abs() < 1e-3 * want, "{what}: volume {volume} vs {want}");
        }
        if let Some(bbox) = expected.get("bbox") {
            let m = &measured[0].bbox;
            let got = [m.min.x, m.min.y, m.min.z, m.max.x, m.max.y, m.max.z];
            let want: Vec<f64> = ["min", "max"].iter().flat_map(|k| bbox[k].as_array().unwrap().iter().map(|v| v.as_f64().unwrap())).collect();
            assert!(got.iter().zip(&want).all(|(g, w)| (g - w).abs() < 1e-3), "{what}: box {got:?} vs {want:?}");
        }
        match crate::step_import_feature_read(&bytes, &file, crate::StepNames::default(), crate::StepReader::Cadmpeg) {
            Err(e) => assert!(e.to_string().contains(&format!("{c}.igs")), "{c}: the message names the file: {e}"),
            Ok(theirs) => assert!(same_bodies(&built(&theirs.import), &built(&sf.import)), "{c}: cadmpeg built different bodies"),
        }
    }
}

/// The cadmpeg IGES reader reads only IGES: STEP text named `.igs`, garbage and an empty file
/// are refused with a message naming the file, and IGES contents are not read as STEP.
#[test]
fn the_cadmpeg_iges_reader_refuses_what_is_not_iges() {
    for (bytes, name) in [(STEP_TEXT.as_bytes(), "x.igs"), (b"\0\0garbage".as_slice(), "x.iges"), (b"".as_slice(), "x.igs")] {
        let e = cadmpeg_iges_records(bytes, name).expect_err(name);
        assert!(e.starts_with(name), "{name}: the message names the file: {e}");
    }
    let (iges, _) = known_solids_iges();
    assert!(cadmpeg_iges_records(iges.as_bytes(), "solids.step").is_err(), "only .igs and .iges");
    assert!(cadmpeg_step_records(iges.as_bytes(), "solids.step").is_err(), "IGES contents are not STEP");
}

/// A 10 × 20 × 30 mm box as SolidCraft's STEP export writes it, its header saying it is from
/// `description` (FILE_DESCRIPTION), `preprocessor` and `system` (FILE_NAME's preprocessor version
/// and originating system), each written as it stands between the quotes.
fn box_step(description: &str, preprocessor: &str, system: &str) -> String {
    let block = solidcraft_kernel::box_solid(solidcore_record::Vec3::ZERO, solidcore_record::Vec3::new(10.0, 20.0, 30.0)).unwrap();
    let text = solidcraft_kernel::step_export(&[&block], "").unwrap();
    let (described, named) = ("FILE_DESCRIPTION(('SolidCraft model')", "'SolidCraft','SolidCraft'");
    assert!(text.contains(described) && text.contains(named), "{text}");
    text.replacen(described, &format!("FILE_DESCRIPTION(('{description}')"), 1).replacen(named, &format!("'{preprocessor}','{system}'"), 1)
}

/// The same box as SolidCraft's IGES export writes it, its global section's native system ID
/// (field 5) `native` as a Hollerith string (left out when empty). The global section is
/// rewritten whole: its fields run on from line to line, 72 columns each.
fn box_iges(native: &str) -> String {
    let block = solidcraft_kernel::box_solid(solidcore_record::Vec3::ZERO, solidcore_record::Vec3::new(10.0, 20.0, 30.0)).unwrap();
    let body = solidcraft_kernel::ExportBody { name: "Block".into(), body: &block, color: None };
    let header = solidcraft_kernel::StepHeader { file_name: "box.igs".into(), ..Default::default() };
    let text = solidcraft_kernel::iges_export_bodies(&[body], &header).unwrap();
    let section = |l: &str| l.get(72..73).unwrap_or("").to_string();
    let global: String = text.lines().filter(|l| section(l) == "G").map(|l| l[..72].trim_end()).collect();
    // Field 4 (the file name) then field 5 (the native system ID) then 6 (the preprocessor).
    let field = "7Hbox.igs,10HSolidCraft,19H";
    assert!(global.contains(field), "{global}");
    let holl = if native.is_empty() { String::new() } else { format!("{}H{native}", native.len()) };
    let global: Vec<char> = global.replacen(field, &format!("7Hbox.igs,{holl},19H"), 1).chars().collect();
    let mut out = String::new();
    let mut lines = 0;
    for l in text.lines() {
        match section(l).as_str() {
            "G" if lines == 0 => {
                for chunk in global.chunks(72) {
                    lines += 1;
                    out.push_str(&format!("{:<72}G{lines:>7}\n", chunk.iter().collect::<String>()));
                }
            }
            "G" => {}
            "T" => out.push_str(&format!("{}G{lines:>7}{}\n", &l[..8], &l[16..])),
            _ => out.push_str(&format!("{l}\n")),
        }
    }
    out
}

/// The program that wrote a STEP file is read by the cadmpeg route as SolidCraft's reader reads
/// it, so a file from a Y-up program is offered the same stand-up: the originating system and
/// preprocessor version of FILE_NAME and the FILE_DESCRIPTION text, from the headers written here,
/// a doubled quote, an escaped character and an `ENDSEC;` inside a string included.
#[test]
fn both_readers_name_the_program_that_wrote_a_step_file() {
    let cases = [
        (("SolidCraft model", "SolidCraft", "SolidCraft"), ("SolidCraft model", "SolidCraft", "SolidCraft")),
        (("Part", "SwSTEP 2.0", "SolidWorks 2021"), ("Part", "SwSTEP 2.0", "SolidWorks 2021")),
        (("SolidWorks export", "Example CAD", "Example CAD"), ("SolidWorks export", "Example CAD", "Example CAD")),
        (("Part", "Example CAD", "Example CAD"), ("Part", "Example CAD", "Example CAD")),
        (("Part", "", ""), ("Part", "", "")),
        (("It''s ENDSEC; here", "Caf\\X2\\00E9\\X0\\ CAD", "Example CAD"), ("It's ENDSEC; here", "Café CAD", "Example CAD")),
    ];
    for ((description, preprocessor, system), want) in cases {
        let text = box_step(description, preprocessor, system);
        for reader in READERS {
            let sf = read(text.as_bytes(), "dir/box.step", reader);
            let i = &sf.import;
            assert_eq!((i.description.as_str(), i.preprocessor.as_str(), i.system.as_str()), want, "{reader:?}");
        }
        let records = cadmpeg_step_records(text.as_bytes(), "box.step").unwrap();
        assert_eq!(records.records.system, want.2, "the records keep the program");
    }
}

/// A header field's text may hold the `ENDSEC` keyword many times: each is inside a string and
/// ends nothing, so the header's real end is still found and the program named, and the keyword
/// text does not use up the tries.
#[test]
fn keyword_text_in_a_header_field_does_not_hide_the_header_end() {
    let noise = "ENDSEC ".repeat(super::STEP_HEADER_ENDS + 8);
    let text = box_step(&noise, "Example CAD", "SolidWorks 2021");
    let got = super::step_producer(text.as_bytes());
    assert_eq!((got.system.as_str(), got.preprocessor.as_str()), ("SolidWorks 2021", "Example CAD"));
}

/// A header comment may hold the `ENDSEC` keyword too: each ends nothing, so the header's real
/// end is still found and the program named, and the keyword text does not use up the tries.
#[test]
fn keyword_text_in_a_header_comment_does_not_hide_the_header_end() {
    let noise = format!("/* {} */", "ENDSEC ".repeat(super::STEP_HEADER_ENDS + 8));
    let text = box_step("Part", "Example CAD", "SolidWorks 2021");
    let text = text.replacen("FILE_DESCRIPTION(", &format!("{noise}FILE_DESCRIPTION("), 1);
    let got = super::step_producer(text.as_bytes());
    assert_eq!((got.system.as_str(), got.preprocessor.as_str()), ("SolidWorks 2021", "Example CAD"));
}

/// Every STEP file of the conformance set and the import kernel cases: its header read alone is
/// the header SolidCraft's parser reads with the whole file.
#[test]
fn a_step_header_read_alone_is_the_whole_files() {
    let mut files: Vec<std::path::PathBuf> = Vec::new();
    for dir in ["../../tests/step-conformance", "../../tests/kernel-cases/import"] {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(dir);
        for e in std::fs::read_dir(&dir).unwrap() {
            let p = e.unwrap().path();
            if p.extension().is_some_and(|e| e == "step") {
                files.push(p);
            } else if p.join("input.step").is_file() {
                files.push(p.join("input.step"));
            }
        }
    }
    assert!(files.len() > 30, "{} files", files.len());
    for p in files {
        let bytes = std::fs::read(&p).unwrap();
        let Ok(ex) = solidcraft_kernel::services::step::p21::parse(&crate::step_text(&bytes)) else { continue };
        let got = super::step_producer(&bytes);
        assert_eq!((got.system, got.preprocessor, got.description), (ex.originating_system, ex.preprocessor, ex.description), "{}", p.display());
    }
}

/// The program that wrote an IGES file (its global section's native system ID) is read by the
/// cadmpeg route, from the cards cadmpeg keeps, as SolidCraft's reader reads it: a Y-up program,
/// another one, a name with a backslash and spaces around it (ASCII without control characters, a
/// backslash a space, trimmed), one that runs on to the next line, and none.
#[test]
fn both_readers_name_the_program_that_wrote_an_iges_file() {
    let long = "A long name that runs on past the end of the global section's line, onto the next";
    for (native, want) in
        [("SolidWorks 2021", "SolidWorks 2021"), ("Example CAD", "Example CAD"), ("  Solid\\Works 2019 ", "Solid Works 2019"), (long, long), ("", "")]
    {
        let text = box_iges(native);
        assert_eq!(solidcraft_kernel::iges_native_system(&text), want, "{native:?}: {text}");
        for reader in READERS {
            let sf = read_iges(text.as_bytes(), "dir/box.igs", reader);
            assert_eq!(sf.import.system, want, "{native:?} ({reader:?}): {:?}", sf.warnings);
        }
    }
}

/// Hostile headers never panic either reader: a very long program name, control characters,
/// bytes that are not UTF-8, no FILE_NAME or no header at all, `ENDSEC` again and again where the
/// header should end; and an IGES global section that is long, has control characters, or is
/// missing. Where both readers read the file, they name the same program.
#[test]
fn hostile_headers_never_panic_either_reader() {
    let long = "W".repeat(200_000);
    let full = box_step("Part", "Example CAD", "Example CAD");
    let data = full.find("DATA;").unwrap();
    let mut steps: Vec<Vec<u8>> = [
        box_step("Part", "Example CAD", &long),
        box_step("Part\u{1}\u{7f}", "Solid\u{0}Works", "SolidWorks\u{8}\u{1b}[2J"),
        box_step("Part", "Example CAD", "Solid\\Works\\"),
        full.replacen("FILE_NAME(", "FILE_NAMES(", 1),
        full.replacen("FILE_DESCRIPTION((", "/* ENDSEC ENDSEC ENDSEC ENDSEC ENDSEC ENDSEC ENDSEC ENDSEC ENDSEC */FILE_DESCRIPTION((", 1),
        format!("ISO-10303-21;\n{}", &full[data..]),
    ]
    .into_iter()
    .map(String::into_bytes)
    .collect();
    let mut latin = box_step("Part", "Example CAD", "SolidWorks Caf_").into_bytes();
    let at = latin.windows(4).position(|w| w == b"Caf_").unwrap() + 3;
    latin[at] = 0xe9;
    steps.push(latin);
    let header = |i: &solidcraft_kernel::StepImport| (i.system.clone(), i.preprocessor.clone(), i.description.clone());
    for b in &steps {
        let _ = super::step_producer(b);
        let ours = crate::step_import_feature_read(b, "x.step", crate::StepNames::default(), crate::StepReader::SolidCraft);
        let theirs = crate::step_import_feature_read(b, "x.step", crate::StepNames::default(), crate::StepReader::Cadmpeg);
        if let (Ok(o), Ok(t)) = (ours, theirs) {
            assert_eq!(header(&o.import), header(&t.import));
        }
    }
    let mut igs = vec![box_iges(&"W".repeat(100_000)), box_iges("Solid\u{1}Works\u{7f}"), box_iges("SolidWorks")];
    let no_global: String = igs[2].lines().filter(|l| l.get(72..73) != Some("G")).map(|l| format!("{l}\n")).collect();
    igs.push(no_global);
    for text in &igs {
        let _ = solidcraft_kernel::iges_native_system(text);
        let ours = crate::step_import_feature_read(text.as_bytes(), "x.igs", crate::StepNames::default(), crate::StepReader::SolidCraft);
        let theirs = crate::step_import_feature_read(text.as_bytes(), "x.igs", crate::StepNames::default(), crate::StepReader::Cadmpeg);
        if let (Ok(o), Ok(t)) = (ours, theirs) {
            assert_eq!(o.import.system, t.import.system);
        }
    }
    for garbage in [b"".as_slice(), b"ENDSEC;ENDSEC;", b"ISO-10303-21;\nHEADER;FILE_NAME('", &[0xff; 300]] {
        assert_eq!(super::step_producer(garbage), super::Producer::default());
        let _ = solidcraft_kernel::iges_native_system(&String::from_utf8_lossy(garbage));
    }
}

/// A header followed by one line longer than the bytes read for it (a file written without line
/// breaks after its first) is read, and so is one where the limit splits a UTF-8 character: the
/// program is the one SolidCraft's reader names from the whole file.
#[test]
fn a_step_header_before_a_long_line_is_read() {
    let text = box_step("Part", "SwSTEP 2.0", "SolidWorks 2021");
    let (first, rest) = text.split_once('\n').unwrap();
    let rest = rest.replace(['\r', '\n'], "");
    let data = rest.find("DATA;").unwrap() + "DATA;".len();
    for pad in ["x", "é"] {
        // A comment in the data section takes the line past the limit; with "é" the limit falls
        // inside a two-byte character.
        let before = first.len() + 1 + data + "/*".len();
        let mut comment = "x".repeat(super::STEP_HEADER_BYTES - before - 1);
        comment.push_str(&pad.repeat(300 << 10));
        let long = format!("{first}\n{}/*{comment}*/{}", &rest[..data], &rest[data..]);
        assert!(long.len() > super::STEP_HEADER_BYTES && long.lines().nth(1).unwrap().len() > super::STEP_HEADER_BYTES);
        if pad == "é" {
            assert!(!long.is_char_boundary(super::STEP_HEADER_BYTES), "the limit splits a character");
        }
        let got = super::step_producer(long.as_bytes());
        assert_eq!((got.system.as_str(), got.preprocessor.as_str(), got.description.as_str()), ("SolidWorks 2021", "SwSTEP 2.0", "Part"), "{pad:?}");
        let ours = solidcraft_kernel::step_import(&long).unwrap();
        assert_eq!(got.system, ours.system, "{pad:?}");
    }
}

/// A part whose body was not imported (its conversion refused) is not compared with the figures
/// its file states, as if what did import were the whole part: the warning says it was not checked.
#[test]
fn a_part_whose_body_was_not_imported_is_said_not_checked() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/step-conformance");
    let ours = solidcraft_kernel::step_import(&std::fs::read_to_string(dir.join("washer.step")).unwrap()).unwrap();
    let m = solidcraft_kernel::measure(&ours.bodies[0].body).unwrap();
    let text = washer_stating(m.volume, [m.centroid.x, m.centroid.y, m.centroid.z]);
    let mut ir = neutral(text.as_bytes(), "washer.step");
    assert!(!ir.model.declared_measures.is_empty());
    // Every region refused: a coedge missing from each.
    ir.model.coedges.clear();
    let (records, origins) = traced_records(&ir, "washer", "", true, false);
    assert!(records.bodies.is_empty(), "{:?}", records.warnings);
    let imp = solidcraft_kernel::records_import(&records).unwrap();
    let notes = validation::check(&ir, &records, &origins, &imp);
    assert!(!notes.is_empty() && notes.iter().all(|n| n.contains("was not checked")), "{notes:?}");
}

/// A part of two bodies, one imported and one whose region the reader refused, is said not
/// checked against the figures its file states for both: the imported body alone measures what
/// the file states here, so a comparison without the other would pass, unsaid.
#[test]
fn a_part_with_one_body_imported_and_one_not_is_said_not_checked() {
    use cadmpeg_ir::declared_measures::DeclaredTarget;
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/step-conformance");
    let ours = solidcraft_kernel::step_import(&std::fs::read_to_string(dir.join("washer.step")).unwrap()).unwrap();
    let m = solidcraft_kernel::measure(&ours.bodies[0].body).unwrap();
    let text = washer_stating(m.volume, [m.centroid.x, m.centroid.y, m.centroid.z]);
    let mut ir = neutral(text.as_bytes(), "washer.step");
    assert!(!ir.model.declared_measures.is_empty());
    // A second body of the part, on a region the model does not have: refused.
    let mut refused = ir.model.bodies[0].clone();
    refused.id = cadmpeg_ir::ids::BodyId::try_from("test:refused:body#0".to_string()).unwrap();
    refused.regions = vec![cadmpeg_ir::ids::RegionId::try_from("test:refused:region#0".to_string()).unwrap()];
    let model = &mut ir.model;
    for stated in &mut model.declared_measures {
        match &mut stated.target {
            DeclaredTarget::Definition { definition } => {
                if let Some(d) = model.product_definitions.iter_mut().find(|d| d.id == *definition) {
                    d.bodies.push(refused.id.clone());
                }
            }
            DeclaredTarget::Bodies { bodies } => bodies.push(refused.id.clone()),
            DeclaredTarget::Unresolved {} => {}
        }
    }
    model.bodies.push(refused);
    let (records, origins) = traced_records(&ir, "washer", "", true, false);
    assert!(!records.bodies.is_empty(), "the washer itself comes in: {:?}", records.warnings);
    let imp = solidcraft_kernel::records_import(&records).unwrap();
    let notes = validation::check(&ir, &records, &origins, &imp);
    assert!(!notes.is_empty() && notes.iter().all(|n| n.contains("was not checked")), "{notes:?}");
}

/// A part whose solid could not be closed, come in as an open surface of the faces that read, is
/// said not checked against the volume and centre its file states, not left out of them unsaid.
#[test]
fn a_part_whose_solid_came_in_open_is_said_not_checked() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/step-conformance");
    let ours = solidcraft_kernel::step_import(&std::fs::read_to_string(dir.join("washer.step")).unwrap()).unwrap();
    let m = solidcraft_kernel::measure(&ours.bodies[0].body).unwrap();
    // One face's plane moved 5 mm off its edges: the reader names that face, and the solid comes
    // in open.
    let text = washer_stating(m.volume, [m.centroid.x, m.centroid.y, m.centroid.z])
        .replace("#50=CARTESIAN_POINT('',(0.0,0.0,0.0));", "#50=CARTESIAN_POINT('',(0.0,0.0,-5.0));");
    let ir = neutral(text.as_bytes(), "washer.step");
    let (records, origins) = traced_records(&ir, "washer", "", true, false);
    assert!(records.bodies.iter().any(|b| b.sheet), "{:?}", records.warnings);
    let imp = solidcraft_kernel::records_import(&records).unwrap();
    let notes = validation::check(&ir, &records, &origins, &imp);
    assert!(!notes.is_empty() && notes.iter().all(|n| n.contains("came in as open surfaces")), "{notes:?}");
}

/// The washer with the CAx-IF validation properties a CAD system writes for it: its volume and its
/// centre, as `volume` and `centre` give them.
fn washer_stating(volume: f64, centre: [f64; 3]) -> String {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/step-conformance");
    let text = std::fs::read_to_string(dir.join("washer.step")).unwrap();
    let [x, y, z] = centre;
    let props = format!(
        "#300=PROPERTY_DEFINITION('geometric validation property','volume and centre',#13);\n\
         #301=REPRESENTATION('',(#302,#303),#5);\n\
         #302=MEASURE_REPRESENTATION_ITEM('volume measure',VOLUME_MEASURE({volume:?}),#304);\n\
         #303=CARTESIAN_POINT('centre point',({x:?},{y:?},{z:?}));\n\
         #304=DERIVED_UNIT((#305));\n#305=DERIVED_UNIT_ELEMENT(#1,3.);\n\
         #306=PROPERTY_DEFINITION_REPRESENTATION(#300,#301);\n"
    );
    let at = text.rfind("ENDSEC;").unwrap();
    format!("{}{props}{}", &text[..at], &text[at..])
}

/// A part whose bodies measure what its file states imports with no warning about it; one whose
/// file states another volume or centre is named, with both figures, as an issue about the file in
/// the import's report (what the History tooltip, the status line and Copy Warning Message show)
/// and once among the open's warnings.
#[test]
fn a_part_that_measures_other_than_its_file_says_is_named() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/step-conformance");
    let ours = solidcraft_kernel::step_import(&std::fs::read_to_string(dir.join("washer.step")).unwrap()).unwrap();
    let m = solidcraft_kernel::measure(&ours.bodies[0].body).unwrap();
    let (v, c) = (m.volume, [m.centroid.x, m.centroid.y, m.centroid.z]);
    let checked = |text: String| {
        let f = crate::step_import_feature_read(text.as_bytes(), "washer.step", crate::StepNames::default(), crate::StepReader::Cadmpeg).unwrap();
        let issues: Vec<String> = f
            .import
            .report
            .issues
            .iter()
            .filter(|i| i.kind == solidcraft_kernel::IssueKind::File && i.detail.contains("the file states"))
            .map(|i| i.detail.clone())
            .collect();
        let warned: Vec<&String> = f.warnings.iter().filter(|w| w.contains("the file states")).collect();
        assert_eq!(warned, issues.iter().collect::<Vec<_>>(), "the open's warnings say each once");
        // The design keeps it with its records: an evaluation of them reports it too.
        let solidcraft_doc::FeatureKind::Import { records: Some(records), .. } = &f.kind else { panic!("an import kept as records") };
        assert_eq!(solidcraft_kernel::records_import(records).unwrap().report, f.import.report);
        issues
    };
    assert_eq!(checked(washer_stating(v, c)), Vec::<String>::new(), "the washer as its file states it");
    let volume = checked(washer_stating(v * 1.1, c));
    assert!(volume.len() == 1 && volume[0].contains("volume") && volume[0].contains("9.091% off"), "{volume:?}");
    let centre = checked(washer_stating(v, [c[0] + 5.0, c[1], c[2]]));
    assert!(centre.len() == 1 && centre[0].contains("centre") && centre[0].contains("5.0000 mm away"), "{centre:?}");
}

/// In an assembly the file states a part's centre in the part's own coordinates: a part placed
/// turned and moved is checked there, not where the assembly puts it.
#[test]
fn a_placed_parts_centre_is_checked_in_its_own_coordinates() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/step-conformance");
    let text = std::fs::read_to_string(dir.join("cylinder_placed.step")).unwrap();
    // Our reader's world centre, taken back through our reader's placement of the part.
    let ours = solidcraft_kernel::step_import(&text).unwrap();
    let m = solidcraft_kernel::measure(&ours.bodies[0].body).unwrap();
    let t = ours.tree[0].children[0].transform;
    let p = [m.centroid.x - t[3][0], m.centroid.y - t[3][1], m.centroid.z - t[3][2]];
    let own: [f64; 3] = std::array::from_fn(|i| (0..3).map(|j| t[i][j] * p[j]).sum());
    let world = [m.centroid.x, m.centroid.y, m.centroid.z];
    assert!((0..3).map(|i| (own[i] - world[i]).powi(2)).sum::<f64>().sqrt() > 1.0, "the part is placed away from where it is defined");
    let stating = |c: [f64; 3]| {
        let [x, y, z] = c;
        let props = format!(
            "#300=PROPERTY_DEFINITION('geometric validation property','centre',#21);\n\
             #301=REPRESENTATION('',(#303),#5);\n\
             #303=CARTESIAN_POINT('centre point',({x:?},{y:?},{z:?}));\n\
             #306=PROPERTY_DEFINITION_REPRESENTATION(#300,#301);\n"
        );
        let at = text.rfind("ENDSEC;").unwrap();
        let records = cadmpeg_step_records(format!("{}{props}{}", &text[..at], &text[at..]).as_bytes(), "cylinder_placed.step").unwrap().records;
        records.warnings.into_iter().filter(|w| w.contains("the file states")).collect::<Vec<_>>()
    };
    assert_eq!(stating(own), Vec::<String>::new());
    assert_eq!(stating(world).len(), 1, "the world centre is not the part's own");
    // A figure on the assembly (#13) is a total over its placed parts, which the neutral model does
    // not compose: it is not compared, rather than against the assembly's own bodies (none).
    let props = "#300=PROPERTY_DEFINITION('geometric validation property','volume',#13);\n\
         #301=REPRESENTATION('',(#302),#5);\n\
         #302=MEASURE_REPRESENTATION_ITEM('volume measure',VOLUME_MEASURE(1.),#304);\n\
         #304=DERIVED_UNIT((#305));\n#305=DERIVED_UNIT_ELEMENT(#1,3.);\n\
         #306=PROPERTY_DEFINITION_REPRESENTATION(#300,#301);\n";
    let at = text.rfind("ENDSEC;").unwrap();
    let super::RecordsRead { records, warnings, .. } =
        cadmpeg_step_records(format!("{}{props}{}", &text[..at], &text[at..]).as_bytes(), "cylinder_placed.step").unwrap();
    assert!(!records.warnings.iter().chain(&warnings).any(|w| w.contains("the file states")), "{warnings:?}");
}

// Export through cadmpeg. Expected boxes, volumes, names and colours come from the designs'
// own dimensions and looks; nothing is taken from SolidCraft's output.

/// A box `size` with its corner at `at`, as a new body in component `component`.
fn add_box(doc: &mut solidcraft_doc::Document, at: solidcore_record::Vec3, size: [f64; 3], component: u64) {
    let id = doc
        .add_feature(
            solidcraft_doc::FeatureKind::Box {
                corner: solidcraft_doc::point_expr(at),
                length: size[0].to_string().into(),
                width: size[1].to_string().into(),
                height: size[2].to_string().into(),
                operation: solidcraft_doc::Operation::NewBody,
                centred: false,
            },
            None,
        )
        .unwrap();
    doc.feature_mut(id).unwrap().component = component;
}

/// A 6 mm cylinder 15 mm high standing at x = 80 mm, as a new body.
fn add_cylinder(doc: &mut solidcraft_doc::Document) {
    doc.add_feature(
        solidcraft_doc::FeatureKind::Cylinder {
            base: solidcraft_doc::point_expr(solidcore_record::Vec3::new(80.0, 0.0, 0.0)),
            axis: solidcraft_doc::dir_expr(solidcore_record::Vec3::Z),
            radius: "6".into(),
            height: "15".into(),
            operation: solidcraft_doc::Operation::NewBody,
            references: Vec::new(),
        },
        None,
    )
    .unwrap();
}

fn evaluated(doc: &solidcraft_doc::Document) -> solidcraft_doc::Model {
    let mut model = solidcraft_doc::Model::new();
    model.evaluate(doc);
    model
}

/// `doc` exported through cadmpeg as `file` in `format`, every body, at standard quality.
fn exported(doc: &solidcraft_doc::Document, file: &str, format: crate::Format) -> Result<(Vec<u8>, Vec<String>), String> {
    let model = evaluated(doc);
    let world = solidcraft_doc::world_state(doc, &model.state()).unwrap();
    cadmpeg_export(doc, &model.state(), &world, &[], &crate::CadmpegOptions::default(), file, crate::MeshQuality::Standard, format)
}

/// Each imported body's volume and box (`[min, max]`), sorted, rounded to a micrometre.
fn solids(sf: &crate::StepFeature) -> Vec<(i64, [i64; 6])> {
    let mut out: Vec<(i64, [i64; 6])> = sf
        .import
        .bodies
        .iter()
        .map(|b| {
            let m = solidcraft_kernel::measure(&b.body).unwrap();
            let bb = [m.bbox.min.x, m.bbox.min.y, m.bbox.min.z, m.bbox.max.x, m.bbox.max.y, m.bbox.max.z];
            ((m.volume * 1e3).round() as i64, bb.map(|v| (v * 1e3).round() as i64))
        })
        .collect();
    out.sort();
    out
}

fn solid(volume: f64, bb: [f64; 6]) -> (i64, [i64; 6]) {
    ((volume * 1e3).round() as i64, bb.map(|v| (v * 1e3).round() as i64))
}

/// An imported body's box, in micrometres.
fn body_box(b: &solidcraft_kernel::ImportedBody) -> [i64; 6] {
    let bb = solidcraft_kernel::measure(&b.body).unwrap().bbox;
    [bb.min.x, bb.min.y, bb.min.z, bb.max.x, bb.max.y, bb.max.z].map(|v| (v * 1e3).round() as i64)
}

/// A Rhino file as cadmpeg reads it back.
fn rhino_model(bytes: &[u8]) -> cadmpeg_ir::document::CadIr {
    let (format, decoded) = convert::decode_at("x.3dm", "3dm", bytes, &["rhino"], cadmpeg_core::decode::DecodeDepth::Full).unwrap();
    assert_eq!(format, "rhino");
    decoded.into_parts().0
}

/// A frame holding a needle placed twice: the first needle turned a quarter about Z and moved
/// 100 mm along X, the second moved 50 mm along Y. Each box is 10 × 20 × 30 mm; the needle's
/// corner is at x = 5 mm in its component.
fn galvanometer() -> solidcraft_doc::Document {
    use solidcore_record::Vec3;
    let mut doc = solidcraft_doc::Document::new("Galvanometer");
    let frame = doc.add_component(Some("Frame"), 0).unwrap();
    let needle = doc.add_component(Some("Needle"), frame).unwrap();
    add_box(&mut doc, Vec3::ZERO, [10.0, 20.0, 30.0], frame);
    add_box(&mut doc, Vec3::new(5.0, 0.0, 0.0), [10.0, 20.0, 30.0], needle);
    let turned = solidcraft_doc::rigid(Vec3::new(100.0, 0.0, 0.0), Vec3::ZERO, Vec3::Z, std::f64::consts::FRAC_PI_2);
    doc.occurrences.iter_mut().find(|o| o.component == needle).unwrap().transform = turned;
    doc.add_occurrence(needle, frame, solidcraft_doc::rigid(Vec3::new(0.0, 50.0, 0.0), Vec3::ZERO, Vec3::Z, 0.0)).unwrap();
    doc
}

/// Where the galvanometer's three boxes are: a quarter turn about Z takes the needle's x 5..15,
/// y 0..20 to x -20..-0, y 5..15, then 100 mm along X.
fn galvanometer_solids() -> Vec<(i64, [i64; 6])> {
    let mut want = vec![
        solid(6000.0, [0.0, 0.0, 0.0, 10.0, 20.0, 30.0]),
        solid(6000.0, [80.0, 5.0, 0.0, 100.0, 15.0, 30.0]),
        solid(6000.0, [5.0, 50.0, 0.0, 15.0, 70.0, 30.0]),
    ];
    want.sort();
    want
}

/// A 10 × 20 × 30 mm box exports to Rhino as an exact solid, named after the body, and reads back
/// through both of SolidCraft's Rhino readers as that box, with nothing reported lost.
#[test]
fn a_box_exports_to_rhino_and_reads_back() {
    let mut doc = solidcraft_doc::Document::new("Block");
    add_box(&mut doc, solidcore_record::Vec3::ZERO, [10.0, 20.0, 30.0], 0);
    let (bytes, warnings) = exported(&doc, "block.3dm", crate::Format::Rhino).unwrap();
    assert!(bytes.starts_with(b"3D Geometry File Format "), "{:?}", bytes.get(..32));
    assert!(warnings.is_empty(), "{warnings:?}");
    let name = evaluated(&doc).state().bodies[0].name.clone();
    for reader in READERS {
        let sf = read(&bytes, "block.3dm", reader);
        assert_eq!(solids(&sf), vec![solid(6000.0, [0.0, 0.0, 0.0, 10.0, 20.0, 30.0])], "{reader:?}");
        // `Body1` reads as a placeholder, so by default the body is named after the file; the
        // records keep the object's own name.
        assert_eq!(sf.body_names, vec!["block".to_string()], "{reader:?}");
        if reader == crate::StepReader::Cadmpeg {
            let own = crate::step_import_feature_read(&bytes, "block.3dm", crate::StepNames::Solid, reader).unwrap();
            assert_eq!(own.body_names, vec![name.clone()]);
        }
    }
    let ir = rhino_model(&bytes);
    let names: Vec<_> = ir.model.bodies.iter().filter_map(|b| b.name.clone()).collect();
    assert_eq!(names, vec![name]);
}

/// A body's look is its colour in the Rhino file, sRGB as SolidCraft keeps it (a Rhino colour is
/// the bytes themselves); a cylinder, whose round faces cadmpeg's Rhino writer cannot write
/// exactly yet, is written as its mesh, with warnings naming it, and that mesh spans the
/// cylinder: radius 6 mm, 15 mm high, at x = 80 mm.
#[test]
fn rhino_export_keeps_colours_and_writes_a_cylinder_as_its_mesh() {
    let mut doc = solidcraft_doc::Document::new("Parts");
    add_box(&mut doc, solidcore_record::Vec3::ZERO, [10.0, 20.0, 30.0], 0);
    add_cylinder(&mut doc);
    let names: Vec<String> = evaluated(&doc).state().bodies.iter().map(|b| b.name.clone()).collect();
    doc.appearances.bodies.insert(names[0].clone(), solidcraft_doc::appearance::Look::custom([255, 128, 0], 1.0));
    doc.appearances.bodies.insert(names[1].clone(), solidcraft_doc::appearance::Look::custom([0, 64, 255], 1.0));
    let (bytes, warnings) = exported(&doc, "parts.3dm", crate::Format::Rhino).unwrap();
    let cylinder = &names[1];
    assert!(warnings.iter().any(|w| w.starts_with(&format!("{cylinder}: written as its mesh only"))), "{warnings:?}");
    assert!(warnings.iter().any(|w| w.starts_with(&format!("{cylinder}: written as a mesh without its name or colour"))), "{warnings:?}");
    let ir = rhino_model(&bytes);
    let block = ir.model.bodies.iter().find(|b| b.name.as_deref() == Some(names[0].as_str())).unwrap();
    let c = block.color.unwrap();
    assert_eq!([c.r(), c.g(), c.b(), c.a()], [1.0, 128.0 / 255.0, 0.0, 1.0]);
    let [mesh] = ir.model.tessellations.as_slice() else { panic!("one mesh: {}", ir.model.tessellations.len()) };
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    for p in mesh.vertices() {
        let p = p.get();
        for (k, v) in [p.x, p.y, p.z].into_iter().enumerate() {
            lo[k] = lo[k].min(v);
            hi[k] = hi[k].max(v);
        }
    }
    for (got, want) in lo.iter().chain(&hi).zip([74.0, -6.0, 0.0, 86.0, 6.0, 15.0]) {
        // Within the mesh's chord tolerance (2e-4 of the cylinder's 22.6 mm diagonal).
        assert!((got - want).abs() < 0.01, "{lo:?} {hi:?}");
    }
    // The box reads back through SolidCraft's readers. The mesh is not a solid: SolidCraft's own
    // Rhino reader leaves it out, and cadmpeg's brings it in as a mesh body spanning the cylinder.
    for reader in READERS {
        let sf = read(&bytes, "parts.3dm", reader);
        let mut got = solids(&sf);
        if reader == crate::StepReader::Cadmpeg {
            let mesh = sf.import.bodies.iter().position(|b| b.body.is_mesh()).unwrap_or_else(|| panic!("no mesh body: {got:?}"));
            let m = solidcraft_kernel::measure(&sf.import.bodies[mesh].body).unwrap();
            let bb = [m.bbox.min.x, m.bbox.min.y, m.bbox.min.z, m.bbox.max.x, m.bbox.max.y, m.bbox.max.z];
            for (got, want) in bb.iter().zip([74.0, -6.0, 0.0, 86.0, 6.0, 15.0]) {
                assert!((got - want).abs() < 0.01, "{bb:?}");
            }
            got.retain(|s| s.1 != bb.map(|v| (v * 1e3).round() as i64));
        }
        assert_eq!(got, vec![solid(6000.0, [0.0, 0.0, 0.0, 10.0, 20.0, 30.0])], "{reader:?}");
    }
}

/// A hidden body Rhino writes as its exact shape is hidden in the file; a hidden cylinder, which
/// cadmpeg's Rhino writer writes as a mesh with no visibility, is said to be written shown.
#[test]
fn a_hidden_body_rhino_writes_as_a_mesh_is_said_to_be_shown() {
    let mut doc = solidcraft_doc::Document::new("Parts");
    add_box(&mut doc, solidcore_record::Vec3::ZERO, [10.0, 20.0, 30.0], 0);
    add_cylinder(&mut doc);
    let model = evaluated(&doc);
    let names: Vec<String> = model.state().bodies.iter().map(|b| b.name.clone()).collect();
    let world = solidcraft_doc::world_state(&doc, &model.state()).unwrap();
    let options =
        crate::CadmpegOptions { hidden: crate::Hidden { bodies: names.iter().cloned().collect(), ..Default::default() }, ..Default::default() };
    let (bytes, warnings) =
        cadmpeg_export(&doc, &model.state(), &world, &[], &options, "parts.3dm", crate::MeshQuality::Standard, crate::Format::Rhino).unwrap();
    let shown = |name: &str| warnings.iter().any(|w| w.starts_with(&format!("{name}: hidden, but written shown")));
    assert!(shown(&names[1]), "{warnings:?}");
    assert!(!shown(&names[0]), "{warnings:?}");
    let ir = rhino_model(&bytes);
    let block = ir.model.bodies.iter().find(|b| b.name.as_deref() == Some(names[0].as_str())).unwrap();
    assert_eq!(block.visible, Some(false));
}

/// Rhino has no assemblies here, so every body of the galvanometer is written where the design
/// shows it, with a warning, and reads back there.
#[test]
fn an_assembly_exports_to_rhino_where_the_design_shows_it() {
    let (bytes, warnings) = exported(&galvanometer(), "galvanometer.3dm", crate::Format::Rhino).unwrap();
    assert!(warnings.iter().any(|w| w.contains("components and placements are not written")), "{warnings:?}");
    for reader in READERS {
        let sf = read(&bytes, "galvanometer.3dm", reader);
        assert_eq!(solids(&sf), galvanometer_solids(), "{reader:?}");
    }
}

/// FreeCAD is refused with why: cadmpeg 0.6.12 only writes back a FreeCAD file it read. A design
/// with nothing to write, an unknown body and a format cadmpeg does not write here are refused;
/// odd names and placements never panic.
#[test]
fn cadmpeg_export_refuses_what_it_cannot_write() {
    let mut doc = solidcraft_doc::Document::new("Block");
    add_box(&mut doc, solidcore_record::Vec3::ZERO, [10.0, 20.0, 30.0], 0);
    let e = exported(&doc, "block.FCStd", crate::Format::FreeCad).unwrap_err();
    assert!(e.contains("FreeCAD files cannot be exported yet"), "{e}");
    let e = exported(&doc, "block.obj", crate::Format::Obj).unwrap_err();
    assert!(e.contains("not written through cadmpeg"), "{e}");
    // The plain export says the same of FreeCAD, and sends Rhino to `file.export`.
    let state = evaluated(&doc).state();
    let painted = doc.painted(&state);
    let e = crate::export(&painted, &[], crate::Format::FreeCad, "Block").unwrap_err().to_string();
    assert!(e.contains("FreeCAD files cannot be exported yet"), "{e}");
    let e = crate::export(&painted, &[], crate::Format::Rhino, "Block").unwrap_err().to_string();
    assert!(e.contains("use file.export"), "{e}");
    assert!(exported(&solidcraft_doc::Document::new("Empty"), "empty.3dm", crate::Format::Rhino).is_err());
    let model = evaluated(&doc);
    let st = model.state();
    assert!(
        cadmpeg_export(
            &doc,
            &st,
            &st,
            &["Nothing".into()],
            &crate::CadmpegOptions::default(),
            "x.3dm",
            crate::MeshQuality::Standard,
            crate::Format::Rhino
        )
        .is_err()
    );
    let mut odd = solidcraft_doc::Document::new("\0\u{7}  ");
    add_box(&mut odd, solidcore_record::Vec3::new(1e9, -1e9, 0.0), [1e-3, 1e6, 1.0], 0);
    let r = std::panic::catch_unwind(|| exported(&odd, "\0.3dm", crate::Format::Rhino));
    assert!(r.is_ok(), "an odd design panicked");
}

/// `bytes`, an IGES file, read by cadmpeg's IGES decoder into records and an Import feature, as
/// a STEP file is with the cadmpeg reader (there is no cadmpeg IGES import route yet).
fn iges_by_cadmpeg(bytes: &[u8]) -> crate::StepFeature {
    let (format, decoded) = convert::decode_at("x.igs", "igs", bytes, &["iges"], cadmpeg_core::decode::DecodeDepth::Full).unwrap();
    assert_eq!(format, "iges");
    let records = cadir_records(&decoded.into_parts().0, "x", "", true);
    let read = crate::RecordsRead { records, warnings: Vec::new(), freecad: false, preprocessor: String::new(), description: String::new() };
    crate::records_import_feature(read, "x.igs", crate::StepNames::default()).unwrap()
}

/// An imported body's colour: its own, else the one colour every face of it has (cadmpeg's STEP
/// writer styles a body's faces, each with the body's colour).
fn colour_of(b: &solidcraft_kernel::ImportedBody) -> Option<[f32; 3]> {
    b.color.or_else(|| {
        let faces = &b.body.paint()?.faces;
        let first = faces.first()?.color;
        (faces.len() == b.body.face_count() && faces.iter().all(|f| f.color == first)).then_some(first)
    })
}

/// A 10 × 20 × 30 mm box and a cylinder of radius 6 mm, 15 mm high at x = 80 mm (volume
/// π·36·15), coloured #0040FF.
fn box_and_cylinder() -> (solidcraft_doc::Document, Vec<(i64, [i64; 6])>) {
    let mut doc = solidcraft_doc::Document::new("Parts");
    add_box(&mut doc, solidcore_record::Vec3::ZERO, [10.0, 20.0, 30.0], 0);
    add_cylinder(&mut doc);
    let names: Vec<String> = evaluated(&doc).state().bodies.iter().map(|b| b.name.clone()).collect();
    doc.appearances.bodies.insert(names[1].clone(), solidcraft_doc::appearance::Look::custom([0, 64, 255], 1.0));
    let mut want =
        vec![solid(6000.0, [0.0, 0.0, 0.0, 10.0, 20.0, 30.0]), solid(std::f64::consts::PI * 36.0 * 15.0, [74.0, -6.0, 0.0, 86.0, 6.0, 15.0])];
    want.sort();
    (doc, want)
}

/// cadmpeg writes STEP (AP242 edition 1, as SolidCraft's own writer declares) and IGES 5.3 from
/// the design: a box with a coloured cylinder, and the galvanometer (a part placed twice). Each
/// file reads back through SolidCraft's own reader and through cadmpeg's, every solid with the
/// volume and box the design's dimensions give, the cylinder in its colour. STEP keeps the
/// assembly: each needle under Galvanometer › Frame › Needle; IGES has none here, so its bodies
/// are where the design shows them, with a warning.
#[test]
fn cadmpeg_writes_step_and_iges_that_both_readers_read_back() {
    let (parts, parts_want) = box_and_cylinder();
    let blue = [0.0, 64.0 / 255.0, 1.0];
    for (label, doc, want) in [("parts", parts, parts_want), ("galvanometer", galvanometer(), galvanometer_solids())] {
        for (file, format) in [("x.step", crate::Format::Step), ("x.igs", crate::Format::Iges)] {
            let (bytes, warnings) = exported(&doc, file, format).unwrap_or_else(|e| panic!("{label} {file}: {e}"));
            let assembly = label == "galvanometer";
            let flattened = warnings.iter().any(|w| w.contains("components and placements are not written"));
            assert_eq!(flattened, assembly && format == crate::Format::Iges, "{label} {file}: {warnings:?}");
            // STEP names the components but not the solids; nothing else is reported lost.
            let unnamed = warnings.iter().any(|w| w.contains("the bodies' names are not written"));
            assert_eq!(unnamed, format == crate::Format::Step, "{label} {file}: {warnings:?}");
            assert_eq!(warnings.len(), usize::from(flattened) + usize::from(unnamed), "{label} {file}: {warnings:?}");
            let ours = crate::step_import_feature_read(&bytes, file, crate::StepNames::default(), crate::StepReader::SolidCraft).unwrap();
            assert_eq!(solids(&ours), want, "{label} {file}: SolidCraft's reader");
            let theirs = match format {
                crate::Format::Step => {
                    crate::step_import_feature_read(&bytes, file, crate::StepNames::default(), crate::StepReader::Cadmpeg).unwrap()
                }
                _ => iges_by_cadmpeg(&bytes),
            };
            assert_eq!(solids(&theirs), want, "{label} {file}: cadmpeg's reader");
            let theirs: Vec<_> = theirs.import.bodies.iter().map(|b| (colour_of(b), b.path.clone(), body_box(b))).collect();
            if format == crate::Format::Step {
                assert!(ours.import.schema.contains("AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF"), "{}", ours.import.schema);
            }
            let ours: Vec<_> = ours.import.bodies.iter().map(|b| (colour_of(b), b.path.clone(), body_box(b))).collect();
            for (reader, bodies) in [("SolidCraft", ours), ("cadmpeg", theirs)] {
                if !assembly {
                    // Names need not survive a writer: the cylinder is the body with its box.
                    let cylinder = bodies
                        .iter()
                        .find(|(_, _, bb)| *bb == [74_000, -6_000, 0, 86_000, 6_000, 15_000])
                        .unwrap_or_else(|| panic!("{label} {file} {reader}: no cylinder body: {bodies:?}"));
                    let colour = cylinder.0.unwrap_or_else(|| panic!("{label} {file} {reader}: the cylinder is not coloured"));
                    for (got, want) in colour.iter().zip(blue) {
                        assert!((f64::from(*got) - want).abs() < 1e-3, "{label} {file} {reader}: the cylinder's colour: {colour:?}");
                    }
                    assert_eq!(
                        bodies.iter().filter(|(c, _, _)| c.is_some()).count(),
                        1,
                        "{label} {file} {reader}: only the cylinder is coloured: {bodies:?}"
                    );
                } else if format == crate::Format::Step {
                    let needles = bodies.iter().filter(|(_, p, _)| p.iter().map(String::as_str).eq(["Galvanometer", "Frame", "Needle"])).count();
                    assert_eq!(needles, 2, "{label} {file} {reader}: {bodies:?}");
                }
            }
        }
    }
}

/// STEP written by cadmpeg has no mesh beside the exact shape, so a face's own look is bound to
/// its B-rep face: a grey box with a red top face reads back, through both readers, with one red
/// face and the others grey.
#[test]
fn cadmpeg_step_keeps_a_faces_own_colour() {
    let mut doc = solidcraft_doc::Document::new("Block");
    add_box(&mut doc, solidcore_record::Vec3::ZERO, [10.0, 20.0, 30.0], 0);
    let body = evaluated(&doc).state().bodies[0].name.clone();
    doc.appearances.bodies.insert(body.clone(), solidcraft_doc::appearance::Look::custom([128, 128, 128], 1.0));
    doc.appearances.faces.push(solidcraft_doc::appearance::FaceLook {
        body,
        point: solidcore_record::Vec3::new(5.0, 10.0, 30.0),
        look: solidcraft_doc::appearance::Look::custom([255, 0, 0], 1.0),
        name: None,
        strict_name: false,
        certificate: None,
    });
    let (bytes, warnings) = exported(&doc, "x.step", crate::Format::Step).unwrap();
    assert!(warnings.iter().all(|w| w.contains("the bodies' names are not written")), "{warnings:?}");
    for reader in [crate::StepReader::SolidCraft, crate::StepReader::Cadmpeg] {
        let read = crate::step_import_feature_read(&bytes, "x.step", crate::StepNames::default(), reader).unwrap();
        let [b] = read.import.bodies.as_slice() else { panic!("{reader:?}: one body") };
        let faces = &b.body.paint().unwrap_or_else(|| panic!("{reader:?}: no paint")).faces;
        let red = faces.iter().filter(|f| f.color[0] > 0.99 && f.color[1] < 0.01 && f.color[2] < 0.01).count();
        assert_eq!(red, 1, "{reader:?}: {faces:?}");
    }
}

/// A part an AP242 file gives as triangles alone: a 1 cm tetrahedron, as a TRIANGULATED_SURFACE_SET
/// in a TESSELLATED_SHAPE_REPRESENTATION, in centimetres.
const TETRAHEDRON_PART: &str = "ISO-10303-21;\nHEADER;\nFILE_DESCRIPTION((''),'2;1');\nFILE_NAME('tetrahedron','',(''),(''),'','','');\n\
FILE_SCHEMA(('AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF { 1 0 10303 442 1 1 4 }'));\nENDSEC;\nDATA;\n\
#1=(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.CENTI.,.METRE.));\n#2=(NAMED_UNIT(*) PLANE_ANGLE_UNIT() SI_UNIT($,.RADIAN.));\n\
#3=(GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNIT_ASSIGNED_CONTEXT((#1,#2)) REPRESENTATION_CONTEXT('','3D'));\n\
#4=APPLICATION_CONTEXT('managed model based 3d engineering');\n#5=PRODUCT_CONTEXT('',#4,'mechanical');\n\
#6=PRODUCT('Tetrahedron','Tetrahedron','',(#5));\n#7=PRODUCT_DEFINITION_FORMATION('','',#6);\n\
#8=PRODUCT_DEFINITION_CONTEXT('part definition',#4,'design');\n#9=PRODUCT_DEFINITION('design','',#7,#8);\n\
#10=PRODUCT_DEFINITION_SHAPE('','',#9);\n\
#11=COORDINATES_LIST('',4,((0.,0.,0.),(1.,0.,0.),(0.,1.,0.),(0.,0.,1.)));\n\
#12=TRIANGULATED_SURFACE_SET('',#11,4,(),(),((1,3,2),(1,2,4),(2,3,4),(3,1,4)));\n\
#13=TESSELLATED_SHAPE_REPRESENTATION('',(#12),#3);\n#14=SHAPE_DEFINITION_REPRESENTATION(#10,#13);\n\
ENDSEC;\nEND-ISO-10303-21;\n";

/// A part given as triangles alone comes in as a mesh body, in millimetres, with its triangles,
/// its volume and its box, where it was left out before.
#[test]
fn a_part_given_as_triangles_comes_in_as_a_mesh_body() {
    let super::RecordsRead { records, warnings, .. } = cadmpeg_step_records(TETRAHEDRON_PART.as_bytes(), "tetrahedron.step").unwrap();
    let imp = solidcraft_kernel::records_import(&records).unwrap();
    assert_eq!(imp.bodies.len(), 1, "{warnings:?} {:?}", imp.warnings);
    let b = &imp.bodies[0];
    assert!(b.body.is_mesh() && b.closed, "{b:?}");
    let m = solidcraft_kernel::measure(&b.body).unwrap();
    assert_eq!(m.faces, 4);
    assert!((m.volume - 1000.0 / 6.0).abs() < 1e-6, "{}", m.volume);
    let (lo, hi) = ([m.bbox.min.x, m.bbox.min.y, m.bbox.min.z], [m.bbox.max.x, m.bbox.max.y, m.bbox.max.z]);
    assert!(lo.iter().all(|v| v.abs() < 1e-9) && hi.iter().all(|v| (v - 10.0).abs() < 1e-9), "{lo:?} {hi:?}");
}

/// The cylinder placed twice (`cylinder_twice.step`) with its part given as the tetrahedron's
/// triangles instead of its B-rep: a body with no regions that a tessellation is of.
fn mesh_placed_twice() -> cadmpeg_ir::document::CadIr {
    let bytes = std::fs::read(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/step-conformance/cylinder_twice.step")).unwrap();
    let mut ir = neutral(&bytes, "cylinder_twice.step");
    let mut t = neutral(TETRAHEDRON_PART.as_bytes(), "tetrahedron.step").model.tessellations[0].clone();
    let body = ir.model.bodies.iter_mut().find(|b| !b.regions.is_empty()).unwrap();
    body.regions.clear();
    t.body = Some(body.id.clone());
    ir.model.tessellations.push(t);
    ir
}

/// A part given as triangles that an assembly places twice is one mesh placed again: the copy
/// holds no triangles of its own, and both placements come in as mesh bodies.
#[test]
fn a_mesh_part_placed_twice_keeps_its_triangles_once() {
    let (records, _) = traced_records(&mesh_placed_twice(), "twice", "", true, false);
    let shape: Vec<(bool, Option<usize>)> = records.bodies.iter().map(|b| (b.mesh.is_some(), b.copy_of)).collect();
    assert_eq!(shape, [(true, None), (false, Some(0))], "{:?}", records.warnings);
    let imp = solidcraft_kernel::records_import(&records).unwrap();
    assert_eq!(imp.bodies.len(), 2, "{:?}", imp.warnings);
    assert!(imp.bodies.iter().all(|b| b.body.is_mesh()));
}

/// A mesh kept as records is hostile input like any record: too many vertices, a triangle naming
/// a vertex the mesh lacks, or a vertex that is not finite leaves the body out with the reason.
#[test]
fn a_broken_mesh_record_is_left_out_with_why() {
    let super::RecordsRead { records, .. } = cadmpeg_step_records(TETRAHEDRON_PART.as_bytes(), "tetrahedron.step").unwrap();
    for (breaking, why) in [
        (
            Box::new(|m: &mut solidcraft_kernel::RecordMesh| m.triangles[0][0] = 99) as Box<dyn Fn(&mut solidcraft_kernel::RecordMesh)>,
            "names a vertex",
        ),
        (Box::new(|m: &mut solidcraft_kernel::RecordMesh| m.positions[0][0] = f64::NAN), "not finite"),
        (Box::new(|m: &mut solidcraft_kernel::RecordMesh| m.positions.resize(1_000_001, [0.0; 3])), "at most"),
        (Box::new(|m: &mut solidcraft_kernel::RecordMesh| m.triangles.clear()), "at most"),
    ] {
        let mut r = records.clone();
        if let Some(mesh) = r.bodies[0].mesh.as_mut() {
            breaking(mesh);
        }
        let imp = solidcraft_kernel::records_import(&r).unwrap();
        assert!(imp.bodies.is_empty() && imp.warnings.iter().any(|w| w.contains(why)), "{why}: {:?}", imp.warnings);
    }
}

/// A part given as triangles of a body comes in hidden when a hidden layer names the triangles
/// (the tessellation), as when it names the body; on no layer it comes in shown.
#[test]
fn triangles_on_a_hidden_layer_come_in_hidden() {
    for on_layer in [false, true] {
        let mut ir = mesh_placed_twice();
        if on_layer {
            let t = ir.model.tessellations.last().unwrap().id.to_string();
            let layer = serde_json::json!({"id": "test:hidden:layer#0", "name": "Hidden parts", "visible": false, "items": [{"kind": "tessellation", "tessellation": t}]});
            ir.model.presentation_layers.push(serde_json::from_value(layer).unwrap());
        }
        let (records, _) = traced_records(&ir, "twice", "", true, false);
        assert_eq!(records.bodies.len(), 2, "{:?}", records.warnings);
        assert!(records.bodies.iter().all(|b| b.hidden == on_layer), "on a hidden layer: {on_layer}");
    }
}

/// A part on a layer the file hides (INVISIBILITY naming a PRESENTATION_LAYER_ASSIGNMENT) comes in
/// hidden, as with SolidCraft's reader; on a layer that is not hidden it comes in shown.
#[test]
fn a_part_on_a_hidden_layer_comes_in_hidden() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/step-conformance");
    let text = std::fs::read_to_string(dir.join("washer.step")).unwrap();
    let with = |extra: &str| {
        let at = text.rfind("ENDSEC;").unwrap();
        let t = format!("{}{extra}{}", &text[..at], &text[at..]);
        let super::RecordsRead { records, warnings, .. } = cadmpeg_step_records(t.as_bytes(), "washer.step").unwrap();
        assert_eq!(records.bodies.len(), 1, "{warnings:?}");
        records.bodies[0].hidden
    };
    assert!(with("#300=PRESENTATION_LAYER_ASSIGNMENT('Hidden parts','',(#99));\n#301=INVISIBILITY((#300));\n"), "on a hidden layer");
    assert!(!with("#300=PRESENTATION_LAYER_ASSIGNMENT('Parts','',(#99));\n"), "on a shown layer");
    assert!(!with(""), "on no layer");
}

/// Names come in trimmed, as SolidCraft's STEP reader trims them: a product written with a
/// trailing space ("Guma NR 60ShA černá ", as a real assembly writes one) is the same product to
/// both readers.
#[test]
fn product_names_come_in_trimmed() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/step-conformance");
    let text = std::fs::read_to_string(dir.join("cylinder_placed.step"))
        .unwrap()
        .replace("PRODUCT('cylinder_placed','cylinder_placed'", "PRODUCT(' cylinder_placed  ',' cylinder_placed  '");
    let ours = solidcraft_kernel::step_import(&text).unwrap();
    let (theirs, warnings) = by_records(&neutral(text.as_bytes(), "cylinder_placed.step"));
    let theirs = theirs.unwrap_or_else(|e| panic!("{e} {warnings:?}"));
    assert_eq!(tree_of(&theirs), tree_of(&ours));
    let tree = tree_of(&theirs);
    assert!(tree.get(1).is_some_and(|l| l.starts_with("1 cylinder_placed [") && l.contains("/cylinder_placed|")), "{tree:?}");
}

/// A body with an exact B-rep whose display mesh cannot be made is written as its B-rep alone,
/// with a warning naming it; the meshes' size cap stays fatal, and so does a failed mesh of a body
/// with no exact shape, which has nothing else to be written as.
#[test]
fn a_failed_display_mesh_keeps_the_exact_body() {
    use cadir_out::{Unmeshed, kept_without_mesh};
    let mut warnings = Vec::new();
    let failed = || Err(Unmeshed::Failed("could not be meshed: no face meshed".into()));
    assert!(matches!(kept_without_mesh(failed(), true, "Body1", &mut warnings), Ok(None)));
    assert_eq!(warnings, vec!["Body1: its mesh was not written (could not be meshed: no face meshed), so the file's writer meshes it".to_string()]);
    let e = kept_without_mesh(failed(), false, "Body1", &mut warnings).err();
    assert_eq!(e.as_deref(), Some("Body1: could not be meshed: no face meshed"));
    let large = Err(Unmeshed::TooLarge("Body1: the meshes are too large for the file".into()));
    let e = kept_without_mesh(large, true, "Body1", &mut warnings).err();
    assert_eq!(e.as_deref(), Some("Body1: the meshes are too large for the file"));
    assert_eq!(warnings.len(), 1);
    assert!(kept_without_mesh(Ok(solidcraft_geom::Mesh::default()), false, "Body1", &mut warnings).unwrap().is_some());
}

/// In a meshes-only export, a body whose mesh fails is written as its exact shape, when it has
/// one; a mesh body has none, a mesh that passes the size cap stays fatal, and an export with exact
/// shapes keeps its own rule.
#[test]
fn a_meshes_only_export_falls_back_to_the_exact_shape() {
    use cadir_out::{Unmeshed, shape_instead};
    let failed = Err(Unmeshed::Failed("no face meshed".into()));
    let large = Err(Unmeshed::TooLarge("too large".into()));
    assert!(shape_instead(&failed, true, false));
    assert!(!shape_instead(&failed, true, true));
    assert!(!shape_instead(&failed, false, false));
    assert!(!shape_instead(&large, true, false));
    assert!(!shape_instead(&Ok(solidcraft_geom::Mesh::default()), true, false));
}

/// cadmpeg's single-precision loss is kept quiet for a mesh beside an exact B-rep, and for a mesh
/// alone whose rounding is within the weld distance; it is said for a mesh alone so far from the
/// origin for its size that its points round past it. A 1 mm triangle at x = 1e9 mm: single
/// precision rounds there to within 1e9 · 2⁻²⁴ ≈ 60 mm, past its weld distance of 1e-6 mm.
#[test]
fn single_precision_is_said_for_a_mesh_alone_that_it_moves() {
    use cadmpeg_ir::ids::BodyId;
    use cadmpeg_ir::math::Point3;
    use cadmpeg_ir::tessellation::{Tessellation, TessellationId, TessellationMesh};
    use cadmpeg_ir::topology::{Body, BodyKind};
    let triangle = |at: f64, body: Option<&str>| {
        let positions = vec![Point3::new(at, 0.0, 0.0), Point3::new(at + 1.0, 0.0, 0.0), Point3::new(at, 1.0, 0.0)];
        let lanes = TessellationMesh::from_list_lanes(positions, vec![[0, 1, 2]], None).unwrap();
        let id = TessellationId::try_from("test:model:tessellation#0".to_string()).unwrap();
        Tessellation::new(id, lanes, Vec::new()).unwrap().with_body(body.map(|b| BodyId::try_from(b.to_string()).unwrap()))
    };
    let body = |id: &str, regions: Vec<cadmpeg_ir::ids::RegionId>| Body {
        id: BodyId::try_from(id.to_string()).unwrap(),
        kind: BodyKind::General,
        regions,
        transform: None,
        name: None,
        color: None,
        visible: None,
    };
    let with = |t: Tessellation, b: Option<Body>| {
        let mut ir = cadmpeg_ir::document::CadIr::empty();
        ir.model.tessellations.push(t);
        ir.model.bodies.extend(b);
        ir
    };
    let region = cadmpeg_ir::ids::RegionId::try_from("test:model:region#0".to_string()).unwrap();
    // Alone, near the origin and far from it.
    assert!(!export::mesh_only_rounds(&with(triangle(10.0, None), None)));
    assert!(export::mesh_only_rounds(&with(triangle(1e9, None), None)));
    // A bound mesh of a body with no regions is the body's only geometry too.
    assert!(export::mesh_only_rounds(&with(triangle(1e9, Some("test:model:body#0")), Some(body("test:model:body#0", Vec::new())))));
    // Beside an exact B-rep, the B-rep keeps double precision.
    assert!(!export::mesh_only_rounds(&with(triangle(1e9, Some("test:model:body#0")), Some(body("test:model:body#0", vec![region])))));
}
