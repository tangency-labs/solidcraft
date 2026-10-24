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
