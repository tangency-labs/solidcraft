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
    for other in ["c.sat", "c.sab", "c.smt", "c.smb", "c.f3d", "c.step", "fcstd"] {
        assert!(!is_native_cad_path(other), "{other}");
    }
    assert!(is_native_cad_path("part.SLDPRT") && crate::is_step_path("part.sldprt"));
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

/// Keep 0.6.3's service caps (eight million entities, below the desktop allowance),
/// with our STEP size limit on input and unchanged decompression/retention/depth limits.
#[test]
fn decoding_is_capped() {
    let limits = convert::policy().limits;
    let service = cadmpeg_core::decode::ResourceLimits::service();
    let desktop = cadmpeg_core::decode::ResourceLimits::desktop();
    assert_eq!(limits.max_input_bytes, crate::MAX_STEP_BYTES as u64);
    assert_eq!(limits.max_entities, 8_000_000);
    assert!(limits.max_entities < desktop.max_entities);
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

#[test]
#[cfg(not(target_arch = "wasm32"))]
fn sldprt_arc_direction_uses_evidence_and_keeps_long_arcs() {
    let q = std::f64::consts::FRAC_1_SQRT_2;
    let resolve =
        |reference: &[[f64; 3]]| sldprt::arc_reversed([0.0; 3], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0], 1.0, [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], reference);
    assert!(!resolve(&[[q, q, 0.0]]).unwrap(), "the 90-degree arc");
    assert!(resolve(&[[-q, -q, 0.0]]).unwrap(), "the 270-degree arc must not be shortened");
    assert!(resolve(&[[q, q, 0.0], [-q, -q, 0.0]]).is_err(), "conflicting evidence");
    assert!(resolve(&[[1.0, 0.0, 0.0], [0.0, 1.0, 0.0]]).is_err(), "endpoints alone do not select an arc");
    assert!(resolve(&[[q, q, 1.0]]).is_err(), "evidence must lie in the circle plane");
    assert!(resolve(&[[f64::NAN, 0.0, 0.0]]).is_err());
}

#[test]
#[cfg(not(target_arch = "wasm32"))]
fn sldprt_exterior_is_geometric_and_ambiguous_loops_are_refused() {
    let outer = [[100.0, 20.0, 5.0], [110.0, 20.0, 5.0], [110.0, 40.0, 5.0], [100.0, 40.0, 5.0]];
    let hole = [[102.0, 22.0, 5.0], [102.0, 24.0, 5.0], [104.0, 24.0, 5.0], [104.0, 22.0, 5.0]];
    assert_eq!(sldprt::polygon_area(&outer), 200.0);
    assert_eq!(sldprt::polygon_area(&hole), 4.0);
    assert_eq!(sldprt::outer_loop(&[4.0, 200.0]).unwrap(), 1);
    assert!(sldprt::outer_loop(&[200.0, 200.0]).is_err());
    assert!(sldprt::outer_loop(&[]).is_err());
}

#[test]
#[cfg(not(target_arch = "wasm32"))]
fn broken_sldprt_is_an_error_without_a_panic() {
    for bytes in [b"".as_slice(), b"\x9d\x51\x9b\xc0\0\0\0\x04", b"not a CAD file"] {
        let result = std::panic::catch_unwind(|| crate::step_import_feature(bytes, "broken.SLDPRT"));
        assert!(result.is_ok());
        assert!(result.unwrap().is_err());
    }
}

/// A ten-millimeter cube generated by Apache-2.0 cadmpeg; no vendor model is shipped.
#[test]
#[cfg(not(target_arch = "wasm32"))]
fn generated_sldprt_box_retains_millimeters_and_a_closed_solid() {
    use cadmpeg_ir::codec::write::{EncodeInput, target::TargetRequest};
    use cadmpeg_ir::topology::Sense;
    use cadmpeg_registry::{Format, build_encoder};
    use std::collections::BTreeMap;
    let mut ir = cadmpeg_ir::examples::unit_cube().unwrap();
    ir.model.bodies.iter_mut().for_each(|b| b.name = None);
    ir.model.faces.iter_mut().for_each(|f| f.name = None);
    ir.model.edges.iter_mut().for_each(|e| e.set_param_range(None));
    let p: BTreeMap<_, _> = ir.model.points.iter().map(|p| (p.id.clone(), p.position().get())).collect();
    let v: BTreeMap<_, _> = ir.model.vertices.iter().map(|v| (v.id.clone(), p[&v.point])).collect();
    let edges: BTreeMap<_, _> = ir.model.edges.iter().map(|e| (e.id.clone(), e)).collect();
    let coedges: BTreeMap<_, _> = ir.model.coedges.iter().map(|c| (c.id.clone(), c)).collect();
    let loops: BTreeMap<_, _> = ir.model.loops.iter().map(|l| (l.id.clone(), l)).collect();
    let mut display = Vec::new();
    for (i, face) in ir.model.faces.iter().enumerate() {
        let lp = loops[face.loops.iter().next().unwrap()];
        let points: Vec<_> = lp
            .coedges()
            .iter()
            .map(|id| {
                let c = coedges[id];
                let e = edges[&c.edge];
                v[match c.sense {
                    Sense::Forward => &e.start,
                    Sense::Reversed => &e.end,
                }]
            })
            .collect();
        let triangles: Vec<_> = (1..points.len() - 1).map(|k| [0, k, k + 1]).collect();
        let a = points[0];
        let b = points[1];
        let c = points[2];
        let u = [b.x - a.x, b.y - a.y, b.z - a.z];
        let v = [c.x - a.x, c.y - a.y, c.z - a.z];
        let n = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
        let length = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
        let shaded: Vec<_> =
            points.iter().map(|p| serde_json::json!({"position":p,"normal":{"x":n[0]/length,"y":n[1]/length,"z":n[2]/length}})).collect();
        display.push(
            serde_json::from_value(
                serde_json::json!({"id":format!("sldprt:display:mesh#{i}"),"mesh":{"kind":"shaded_list","vertices":shaded,"triangles":triangles}}),
            )
            .unwrap(),
        );
    }
    ir.model.tessellations = display;
    let encoder = build_encoder(Format::Sldprt);
    let plan = encoder.plan(EncodeInput::new(&ir, None), TargetRequest::Explicit("sldprt:unknown")).unwrap();
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let feature = crate::step_import_feature(&bytes, "block.SLDPRT").unwrap();
    assert_eq!(feature.import.bodies.len(), 1);
    let body = &feature.import.bodies[0].body;
    assert!(body.validity().is_empty());
    let m = solidcraft_kernel::measure(body).unwrap();
    assert!((m.volume - 1000.0).abs() < 1e-6, "volume {}", m.volume);
    assert!((m.bbox.size() - solidcraft_geom::Vec3::new(10.0, 10.0, 10.0)).len() < 1e-6);
    for n in (0..bytes.len()).step_by((bytes.len() / 25).max(1)) {
        let result = std::panic::catch_unwind(|| crate::step_import_feature(&bytes[..n], "cut.SLDPRT"));
        assert!(result.is_ok(), "truncation {n}");
    }
}

/// Shared curve/loop ids must not multiply the work beyond the bounded import scope.
#[test]
#[cfg(not(target_arch = "wasm32"))]
fn sldprt_work_limits_count_shared_curves_and_reused_loops() {
    let mut value = serde_json::to_value(cadmpeg_ir::examples::unit_cube().unwrap()).unwrap();
    let id = value["model"]["curves"][0]["id"].clone();
    value["model"]["curves"] = serde_json::json!([{"id":id.clone(),"geometry":{
        "kind":"circle","center":{"x":0.0,"y":0.0,"z":0.0},
        "axis":{"x":0.0,"y":0.0,"z":1.0},"ref_direction":{"x":1.0,"y":0.0,"z":0.0},"radius":1.0}}]);
    let mut edge = value["model"]["edges"][0].clone();
    edge["carrier"] = serde_json::json!({"curve":id});
    value["model"]["edges"] = serde_json::json!(
        (0..100)
            .map(|i| {
                let mut edge = edge.clone();
                edge["id"] = serde_json::json!(format!("budget:brep:edge#{i}"));
                edge
            })
            .collect::<Vec<_>>()
    );
    let mut ir: cadmpeg_ir::CadIr = serde_json::from_value(value).unwrap();
    ir.model.edges.iter_mut().for_each(|edge| edge.set_param_range(None));
    let error = sldprt::normalize_cached(&mut ir, &vec![[0.0; 3]; 100_000]).unwrap_err();
    assert!(error.contains("work limit"), "{error}");

    let mut ir = cadmpeg_ir::examples::unit_cube().unwrap();
    let id = ir.model.loops[0].id.clone();
    ir.model.faces[0].loops = cadmpeg_ir::topology::FaceLoops::Unspecified { loops: vec![id; 5000] };
    let error = sldprt::normalize_cached(&mut ir, &[]).unwrap_err();
    assert!(error.contains("boundary") && error.contains("work limit"), "{error}");
}

/// A named color attribute points to its RGB asset, not necessarily the adjacent
/// REAL_VALUES record. Face order and the compact bridge's attribute order vary.
#[test]
#[cfg(not(target_arch = "wasm32"))]
fn cached_sldprt_colors_follow_owner_and_asset_ids_through_ap214() {
    use cadmpeg_ir::codec::write::{EncodeInput, target::TargetRequest};
    use cadmpeg_registry::{Format, build_encoder};
    let (mut ir, bytes) = cached_color_example();
    assert_eq!(sldprt::restore_colors(&mut ir, &bytes).unwrap(), 2);
    // Face records are shuffled in the source, with grey adjacent to the blue assignment.
    assert_eq!(ir.model.faces[0].color.unwrap().r(), 0.0);
    assert_eq!(ir.model.faces[1].color.unwrap().r(), 192.0 / 255.0);
    let plan = build_encoder(Format::Step).plan(EncodeInput::new(&ir, None), TargetRequest::Explicit("ap214")).unwrap();
    let mut step = Vec::new();
    plan.write_to(&mut step).unwrap();
    let feature = crate::step_import_feature(&step, "colors.step").unwrap();
    let colors: Vec<_> = feature.import.bodies[0].body.paint().unwrap().faces.iter().map(|f| f.color).collect();
    assert_eq!(colors.len(), 2);
    assert!(colors.contains(&[0.0, 0.0, 160.0 / 255.0]));
    assert!(colors.contains(&[192.0 / 255.0; 3]));
    let mut doc = solidcraft_doc::Document::new("Colored imported cube");
    doc.add_feature(feature.kind, Some("Cube")).unwrap();
    let saved = crate::write_design_container(&doc).unwrap();
    assert_eq!(crate::read_design(&saved).unwrap(), doc);
}

#[test]
#[cfg(not(target_arch = "wasm32"))]
fn cached_sldprt_color_conflicts_and_missing_assets_are_errors() {
    let (mut ir, bytes) = cached_color_example();
    ir.model.appearances.clear();
    assert!(sldprt::restore_colors(&mut ir, &bytes).unwrap_err().contains("asset is missing"));
    assert!(ir.model.faces.iter().all(|f| f.color.is_none()));
    let (mut ir, mut bytes) = cached_color_example();
    // A second assignment to the same face with a different RGB id is ambiguous.
    bytes.extend(color_attribute(203, 77, 101, 22, 2));
    assert!(sldprt::restore_colors(&mut ir, &bytes).unwrap_err().contains("conflicting"));
    assert!(ir.model.faces.iter().all(|f| f.color.is_none()));
    let (mut ir, mut bytes) = cached_color_example();
    // A same-sequence conflict remains unresolved even after its first record repeats.
    let first = color_attribute(201, 77, 101, 21, 1);
    bytes.extend(color_attribute(201, 77, 101, 22, 1));
    bytes.extend(first);
    assert!(sldprt::restore_colors(&mut ir, &bytes).is_err());
    let (ir, bytes) = cached_color_example();
    for end in 0..bytes.len() {
        assert!(std::panic::catch_unwind(|| sldprt::restore_colors(&mut ir.clone(), &bytes[..end])).is_ok());
    }
    let (mut ir, mut bytes) = cached_color_example();
    bytes.extend(color_attribute(201, 77, 101, 22, 2));
    bytes.extend(color_attribute(201, 77, 101, 21, 1));
    assert_eq!(sldprt::restore_colors(&mut ir, &bytes).unwrap(), 2);
    assert_eq!(ir.model.faces[0].color.unwrap().r(), 192.0 / 255.0, "an older repeated version must not replace the current color");
}

#[cfg(not(target_arch = "wasm32"))]
fn color_attribute(id: u16, definition: u16, face: u16, color: u16, sequence: u32) -> Vec<u8> {
    let mut b = vec![0, 0x51];
    b.extend(1_u32.to_be_bytes());
    b.extend(id.to_be_bytes());
    b.extend(sequence.to_be_bytes());
    b.extend(definition.to_be_bytes());
    for value in [face, 1, 1, 1, 1, color] {
        b.extend(value.to_be_bytes());
    }
    b
}

#[cfg(not(target_arch = "wasm32"))]
fn cached_color_example() -> (cadmpeg_ir::CadIr, Vec<u8>) {
    let ir = cadmpeg_ir::examples::unit_cube().unwrap();
    let mut json = serde_json::to_string(&ir).unwrap();
    for (i, face) in ir.model.faces.iter().enumerate() {
        json = json.replace(face.id.as_str(), &format!("sldprt:brep:face#{}", 101 + i));
    }
    let mut ir: cadmpeg_ir::CadIr = serde_json::from_str(&json).unwrap();
    for face in &mut ir.model.faces {
        face.color = None;
    }
    ir.model.appearances = serde_json::from_value(serde_json::json!([
        {"id":"sldprt:appearance:entity53#21","base_color":{"r":0.0,"g":0.0,"b":160.0/255.0,"a":1.0}},
        {"id":"sldprt:appearance:entity53#22","base_color":{"r":192.0/255.0,"g":192.0/255.0,"b":192.0/255.0,"a":1.0}}
    ]))
    .unwrap();
    ir.model.appearance_bindings.clear();
    let mut b = color_attribute(202, 77, 102, 22, 1);
    b.extend(color_attribute(201, 77, 101, 21, 1));
    // This adjacent record must never select a face color.
    b.extend([0, 0x53]);
    b.extend(3_u32.to_be_bytes());
    b.extend(22_u16.to_be_bytes());
    for _ in 0..3 {
        b.extend((192.0_f64 / 255.0).to_be_bytes());
    }
    // Definition and name are linked by id, stored in reversed order and separated.
    b.extend([0, 0x50]);
    b.extend(1_u32.to_be_bytes());
    b.extend(77_u16.to_be_bytes());
    b.extend(1_u16.to_be_bytes());
    b.extend(78_u16.to_be_bytes());
    b.extend([0; 20]);
    b.extend([0, 0x4f, 0xff]);
    b.extend(15_u32.to_be_bytes());
    b.extend(78_u16.to_be_bytes());
    b.extend(b"SDL/TYSA_COLOUR");
    (ir, b)
}

#[test]
#[cfg(not(target_arch = "wasm32"))]
fn solidworks_assembly_keeps_repeated_occurrences_and_metric_placements() {
    let xml = assembly_xml("1 0 0 0 0 1 0 0 0 0 1 0 0.012 -0.034 0.056 1");
    let bytes = assembly_container(xml.as_bytes());
    let root = super::assembly::plan(&bytes, None).unwrap();
    assert_eq!(root.children.len(), 2);
    assert_eq!(root.children[0].file, "Pin.SLDPRT");
    assert_eq!(root.children[0].configuration, "Short");
    assert_eq!(root.children[0].transform[3], [12., -34., 56., 1.]);
    assert_eq!(root.children[1].configuration, "Long");
    assert!(root.children[1].hidden);
    assert_eq!(super::assembly::plan(&bytes, Some("Built")).unwrap(), root);
    assert!(super::assembly::plan(&bytes, Some("Missing")).is_err());
}

#[test]
#[cfg(not(target_arch = "wasm32"))]
fn solidworks_assembly_refuses_bad_crc_entities_cycles_and_nonrigid_placements() {
    let xml = assembly_xml("1 0 0 0 0 1 0 0 0 0 1 0 0 0 0 1");
    let mut crc = assembly_container(xml.as_bytes());
    crc[18] ^= 1;
    assert!(super::assembly::plan(&crc, None).unwrap_err().contains("CRC"));
    for bad in [
        xml.replace("<swSolidWorks", "<!DOCTYPE swSolidWorks [<!ENTITY x SYSTEM 'file:///private'>]><swSolidWorks"),
        xml.replace("swModelRef=\"2\"", "swModelRef=\"1\""),
        assembly_xml("-1 0 0 0 0 1 0 0 0 0 1 0 0 0 0 1"),
        assembly_xml("1 0 0 0 0 1 0 0 0 0 1 0 nan 0 0 1"),
        assembly_xml("1 0 0 0 0 1 0 0 0 0 1 0 0 0 0 2"),
    ] {
        assert!(super::assembly::plan(&assembly_container(bad.as_bytes()), None).is_err());
    }
    for end in 0..crc.len() {
        assert!(std::panic::catch_unwind(|| super::assembly::plan(&crc[..end], None)).is_ok());
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn assembly_xml(transform: &str) -> String {
    format!(
        r#"<?xml version="1.0"?><swSolidWorks xmlns="http://www.solidworks.com/sw2003/schema"><swHeader>
<swFile id="a" swPath="C:\private\Test.SLDASM"/><swFile id="b" swPath="C:\private\Pin.SLDPRT"/>
</swHeader><swModelList><swModel id="1" swFileRef="a" swConfigurationName="Built">
<swReference swName="Pin" swReferenceNumber="1" swModelRef="2" swTransform="{transform}"/>
<swReference swName="Pin" swReferenceNumber="2" swModelRef="3" swHidden="YES"/>
</swModel><swModel id="2" swFileRef="b" swConfigurationName="Short"/><swModel id="3" swFileRef="b" swConfigurationName="Long"/></swModelList>
<swConfigurationList><swConfiguration swName="Built" swMostRecentConfiguration="YES" swModelRef="1"/></swConfigurationList></swSolidWorks>"#
    )
}

#[cfg(not(target_arch = "wasm32"))]
fn assembly_container(xml: &[u8]) -> Vec<u8> {
    use std::io::Write;
    let mut encoder = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(xml).unwrap();
    let compressed = encoder.finish().unwrap();
    let name = b"swXmlContents/COMPINSTANCETREE";
    let mut bytes = vec![0x9d, 0x51, 0x9b, 0xc0, 0, 0, 0, 4];
    bytes.extend([0x14, 0, 6, 0, 8, 0]);
    for n in [0, crc32fast::hash(xml), compressed.len() as u32, xml.len() as u32, name.len() as u32] {
        bytes.extend(n.to_le_bytes());
    }
    bytes.extend(name.iter().map(|b| b.rotate_left(4)));
    bytes.extend(compressed);
    bytes
}

#[test]
#[cfg(not(target_arch = "wasm32"))]
fn solidworks_assembly_reuses_geometry_but_preserves_configuration_placement_and_color() {
    use solidcraft_geom::Vec3;
    use solidcraft_kernel::{ExportBody, StepHeader};
    let mut root = super::assembly::plan(&assembly_container(assembly_xml("1 0 0 0 0 1 0 0 0 0 1 0 0.012 -0.034 0.056 1").as_bytes()), None).unwrap();
    root.children[1].hidden = false;
    let mut again = root.children[0].clone();
    again.name = "Pin-3".into();
    again.transform[3][0] = 20.;
    root.children.push(again);
    let mut calls = Vec::new();
    let step = super::assembly::restate(&root, |file, config| {
        calls.push((file.to_string(), config.to_string()));
        let size = if config == "Short" { 1. } else { 2. };
        let color = if config == "Short" { [1., 0., 0.] } else { [0., 0., 1.] };
        let body = solidcraft_kernel::box_solid(Vec3::ZERO, Vec3::new(size, size, size)).unwrap().with_color(Some(color));
        solidcraft_kernel::step_export_bodies(&[ExportBody { name: "Pin".into(), body: &body, color: Some(color) }], &StepHeader::default())
            .map_err(|e| e.to_string())
    })
    .unwrap();
    assert_eq!(calls.len(), 2, "one resolution per file and configuration");
    let part = solidcraft_kernel::step_import_shared(&step).unwrap();
    assert!(part.warnings.is_empty());
    assert_eq!(part.bodies.len(), 3);
    assert_eq!(part.tree[0].children.len(), 3);
    let measured: Vec<_> = part.bodies.iter().map(|b| solidcraft_kernel::measure(&b.body).unwrap()).collect();
    assert!((measured[0].bbox.min - Vec3::new(12., -34., 56.)).len() < 1e-8);
    assert!((measured[1].volume - 8.).abs() < 1e-8);
    assert!((measured[2].bbox.min.x - 20.).abs() < 1e-8);
    assert_eq!(part.bodies[0].body.color(), Some([1., 0., 0.]));
    assert_eq!(part.bodies[1].body.color(), Some([0., 0., 1.]));
}

#[test]
#[cfg(not(target_arch = "wasm32"))]
fn solidworks_recovers_conics_from_incident_supports_with_exact_dimensions() {
    use super::sldprt::analytic::{Support, conic};
    use cadmpeg_ir::geometry::SolvedCurveGeometry;
    let plane = Support::Plane { p: [2., 0., 0.], n: [1., 0., 0.], u: [0., 0., 1.] };
    let cone = Support::Rotation { p: [0.; 3], z: [0., 0., 1.], r: 0., angle: std::f64::consts::FRAC_PI_4 };
    let recovered = conic(&plane, &cone, [2., 0., 2.], [2., 3., 13_f64.sqrt()]).unwrap().unwrap();
    let SolvedCurveGeometry::Hyperbola(curve) = recovered else { panic!("cone section must be a hyperbola") };
    let center = curve.center().get();
    assert!((center.x - 2.).abs() < 1e-10 && center.y.abs() < 1e-10 && center.z.abs() < 1e-10);
    assert!((curve.major_radius().get() - 2.).abs() < 1e-10);
    assert!((curve.minor_radius().get() - 2.).abs() < 1e-10);
    assert!(conic(&plane, &cone, [2., 0., 2.], [2., 3., -13_f64.sqrt()]).unwrap().is_none(), "opposite branches cannot form one edge");
    assert!(conic(&plane, &cone, [2., 0., 2.], [2., 3., 4.]).unwrap().is_none(), "both endpoint support equations must agree");
    let plane = Support::Plane { p: [0., 0., 1.], n: [0., 0., 1.], u: [1., 0., 0.] };
    let cylinder = Support::Rotation { p: [0.; 3], z: [0., 0., 1.], r: 3., angle: 0. };
    let SolvedCurveGeometry::Ellipse(curve) = conic(&plane, &cylinder, [3., 0., 1.], [0., 3., 1.]).unwrap().unwrap() else {
        panic!("cylinder section must be an ellipse")
    };
    assert!((curve.major_radius().get() - 3.).abs() < 1e-10 && (curve.minor_radius().get() - 3.).abs() < 1e-10);
    assert!(conic(&plane, &cylinder, [f64::NAN, 0., 1.], [0., 3., 1.]).is_err());
}

#[test]
#[cfg(not(target_arch = "wasm32"))]
fn solidworks_configuration_selection_uses_declared_index_and_ignores_cache_cells() {
    let xml = br#"<swSolidWorks swVersion="17000"><swModel id="m" swConfigurationName="M2x5" swConfigurationId="9"/>
    <swConfiguration swName="M2x5" swID="9" swMostRecentConfiguration="NO" swConfigurationNeedsUpdate="NO" swModelRef="m"/>
    <swConfiguration swName="M2x4" swID="10" swMostRecentConfiguration="YES" swConfigurationNeedsUpdate="NO"/></swSolidWorks>"#;
    let mut bytes = vec![0x9d, 0x51, 0x9b, 0xc0, 0, 0, 0, 4];
    let name = "Contents/Config-9-Partition";
    // A cache index cell with the same name precedes the real compressed frame.
    bytes.extend([0x14, 0, 6, 0, 8, 0]);
    for word in [0u32, 1936, 484, 968, name.len() as u32] {
        bytes.extend(word.to_le_bytes());
    }
    bytes.extend(name.bytes().map(|v| v.rotate_left(4)));
    super::selection::append_section(&mut bytes, "swXmlContents/Features", xml).unwrap();
    super::selection::append_section(&mut bytes, name, b"selected source geometry").unwrap();
    super::selection::append_section(&mut bytes, "Contents/Config-10-Partition", b"wrong active geometry").unwrap();
    let selected = super::selection::select(&bytes, "M2x5").unwrap();
    assert_eq!(selected.source_index, 9);
    assert_eq!(selected.partition, b"selected source geometry");
    assert!(super::selection::select(&bytes, "Missing").is_err());
    let mut bad = bytes;
    bad.pop();
    assert!(super::selection::select(&bad, "M2x4").is_err());
}

#[test]
#[cfg(not(target_arch = "wasm32"))]
fn solidworks_configuration_selection_rejects_stale_and_conflicting_identity() {
    let xml = r#"<swSolidWorks swVersion="17000"><swModel id="m" swConfigurationName="A" swConfigurationId="9"/>
    <swConfiguration swName="A" swID="9" swConfigurationNeedsUpdate="NO" swModelRef="m"/></swSolidWorks>"#;
    for bad in [
        xml.replace("NeedsUpdate=\"NO\"", "NeedsUpdate=\"YES\""),
        xml.replace("swConfigurationId=\"9\"", "swConfigurationId=\"10\""),
        xml.replace("swID=\"9\"", "swID=\"-9\""),
        xml.replace("<swSolidWorks", "<!DOCTYPE swSolidWorks [<!ENTITY x SYSTEM 'file:///private'>]><swSolidWorks"),
    ] {
        let mut bytes = vec![0x9d, 0x51, 0x9b, 0xc0, 0, 0, 0, 4];
        super::selection::append_section(&mut bytes, "swXmlContents/Features", bad.as_bytes()).unwrap();
        super::selection::append_section(&mut bytes, "Contents/Config-9-Partition", b"geometry").unwrap();
        assert!(super::selection::select(&bytes, "A").is_err());
    }
}

#[test]
#[cfg(not(target_arch = "wasm32"))]
fn solidworks_torus_and_sphere_sections_preserve_analytic_dimensions() {
    use super::sldprt::analytic::{Support, conic};
    use cadmpeg_ir::geometry::SolvedCurveGeometry;
    let torus = Support::Torus { p: [0.; 3], z: [0., 0., 1.], major: 3., minor: 1. };
    let plane = Support::Plane { p: [0., 0., 0.6], n: [0., 0., 1.], u: [1., 0., 0.] };
    let SolvedCurveGeometry::Circle(circle) = conic(&plane, &torus, [3.8, 0., 0.6], [0., 3.8, 0.6]).unwrap().unwrap() else {
        panic!("expected circle")
    };
    assert!((circle.radius().get() - 3.8).abs() < 1e-12);
    assert!(conic(&plane, &torus, [3.8, 0., 0.6], [0., 2.2, 0.6]).unwrap().is_none());
    let sphere = Support::Sphere { p: [0.; 3], radius: 5. };
    let plane = Support::Plane { p: [0., 0., 3.], n: [0., 0., 1.], u: [1., 0., 0.] };
    let SolvedCurveGeometry::Circle(circle) = conic(&plane, &sphere, [4., 0., 3.], [0., 4., 3.]).unwrap().unwrap() else { panic!("expected circle") };
    assert!((circle.radius().get() - 4.).abs() < 1e-12);
    let normal = torus.normal([3.8, 0., 0.6]).unwrap();
    assert!((normal[0] - 0.8).abs() < 1e-12 && (normal[2] - 0.6).abs() < 1e-12);
}

#[test]
#[cfg(not(target_arch = "wasm32"))]
fn cached_face_colors_do_not_consume_body_attribute_layouts() {
    let (ir, mut bytes) = cached_color_example();
    let old = ir.model.bodies[0].id.as_str();
    let json = serde_json::to_string(&ir).unwrap().replace(old, "sldprt:brep:body#3");
    let mut ir: cadmpeg_ir::CadIr = serde_json::from_str(&json).unwrap();
    // A body's RGB reference is not one of the compact bridge's face assets.
    bytes.extend(color_attribute(204, 77, 3, 999, 1));
    bytes.extend([0, 0x50]);
    bytes.extend(1_u32.to_be_bytes());
    bytes.extend(80_u16.to_be_bytes());
    bytes.extend(1_u16.to_be_bytes());
    bytes.extend(79_u16.to_be_bytes());
    bytes.extend([0, 0x4f]);
    bytes.extend(17_u32.to_be_bytes());
    bytes.extend(79_u16.to_be_bytes());
    bytes.extend(b"SDL/TYSA_COLOUR_2");
    bytes.extend(color_attribute(205, 80, 101, 999, 1));
    assert_eq!(sldprt::restore_colors(&mut ir, &bytes).unwrap(), 2);
    assert!(ir.model.bodies[0].color.is_none());
    assert_eq!(ir.model.faces[0].color.unwrap().b(), 160. / 255.);
    assert_eq!(ir.model.faces[1].color.unwrap().r(), 192. / 255.);
}

#[test]
#[cfg(not(target_arch = "wasm32"))]
fn solidworks_partition_frames_validate_sizes_and_checksums() {
    use std::io::Write;
    fn chain(members: &[&[u8]]) -> Vec<u8> {
        let mut data = vec![0x23, 0x1d, 0xd5, 0x71, 0xda, 0x81, 0x48, 0xa2, 0xa8, 0x58, 0x98, 0xb2, 0x1b, 0x89, 0xef, 0x99];
        for payload in members {
            let mut e = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
            e.write_all(payload).unwrap();
            let z = e.finish().unwrap();
            data.extend((payload.len() as u32).to_le_bytes());
            data.extend((z.len() as u32).to_le_bytes());
            data.extend(z);
        }
        data.extend([0; 8]);
        let mut result = (data.len() as u32).to_le_bytes().to_vec();
        result.extend(data);
        result
    }
    let mut bytes = chain(&[b"PS\0\0first", b" part"]);
    bytes.extend(chain(&[b"PS\0\0second"]));
    assert_eq!(super::selection::geometry_streams(&bytes).unwrap(), [b"PS\0\0first part".to_vec(), b"PS\0\0second".to_vec()]);
    let mut corrupt = bytes.clone();
    corrupt[31] ^= 1;
    assert!(super::selection::geometry_streams(&corrupt).is_err());
    let mut bomb = bytes.clone();
    bomb[20..24].copy_from_slice(&(65u32 << 20).to_le_bytes());
    assert!(super::selection::geometry_streams(&bomb).unwrap_err().contains("limit"));
    for end in 0..bytes.len() {
        assert!(std::panic::catch_unwind(|| super::selection::geometry_streams(&bytes[..end])).is_ok());
    }
}

#[test]
#[cfg(not(target_arch = "wasm32"))]
fn solidworks_assembly_retains_configuration_number_and_independent_source_bounds() {
    let xml = assembly_xml("1 0 0 0 0 1 0 0 0 0 1 0 0 0 0 1").replace(
        "swConfigurationName=\"Short\"",
        "swConfigurationName=\"Short\" swConfigurationId=\"9\" swBoundingBox=\"-0.001 0.002 0.003 0.004 0.005 0.006\"",
    );
    let plan = super::assembly::plan(&assembly_container(xml.as_bytes()), None).unwrap();
    assert_eq!(plan.children[0].source_index, Some(9));
    assert_eq!(plan.children[0].bounds_mm, Some([[-1., 2., 3.], [4., 5., 6.]]));
    for bad in [
        xml.replace("swConfigurationId=\"9\"", "swConfigurationId=\"-1\""),
        xml.replace("-0.001 0.002", "NaN 0.002"),
        xml.replace("-0.001 0.002", "1 0.002"),
    ] {
        assert!(super::assembly::plan(&assembly_container(bad.as_bytes()), None).is_err());
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn source_ref(index: u32, kind: u16) -> parasolid_core::brep::SourceNodeRef {
    parasolid_core::brep::SourceNodeRef { node_index: index, node_type: kind, type_name: "authored fixture".into(), node_id: None, byte_range: 0..0 }
}
#[cfg(not(target_arch = "wasm32"))]
fn native_box_fixture() -> parasolid_core::brep::BrepModel {
    use parasolid_core::brep as p;
    let mut m = p::BrepModel {
        source_format: p::BrepSourceFormat::Binary,
        schema_key: "SCH_3501251_35102_13006".into(),
        complete: true,
        bodies: vec![p::Body {
            id: 0,
            kind: p::BodyKind::Solid,
            size_resolution: 1000.,
            linear_resolution: 1e-8,
            regions: vec![0, 1],
            edges: vec![],
            vertices: vec![],
            source: source_ref(70_000, 12),
        }],
        regions: vec![
            p::Region { id: 0, kind: p::RegionKind::Void, body: 0, shells: vec![], source: source_ref(10, 19) },
            p::Region { id: 1, kind: p::RegionKind::Solid, body: 0, shells: vec![], source: source_ref(11, 19) },
        ],
        shells: vec![],
        faces: vec![],
        loops: vec![],
        half_edges: vec![],
        edges: vec![],
        vertices: vec![],
        points: vec![],
        curves: vec![],
        surfaces: vec![],
        topology: p::TopologyValidation { valid: true, closed_loop_count: 0, closed_edge_ring_count: 0, euler_characteristic: 2 },
        metrics: p::BrepMetrics { bounding_box: None, surface_area: None, volume: None },
        diagnostics: vec![],
    };
    push_native_box(&mut m, [0., 0., 0.], [0.002, 0.003, 0.004], 0, false);
    m
}
#[cfg(not(target_arch = "wasm32"))]
fn push_native_box(m: &mut parasolid_core::brep::BrepModel, lo: [f64; 3], hi: [f64; 3], opposite: u32, cavity: bool) {
    use parasolid_core::brep as p;
    let base = m.vertices.len() as u32;
    let material_shell = m.shells.len() as u32;
    let void_shell = material_shell + 1;
    let points = [
        [lo[0], lo[1], lo[2]],
        [hi[0], lo[1], lo[2]],
        [hi[0], hi[1], lo[2]],
        [lo[0], hi[1], lo[2]],
        [lo[0], lo[1], hi[2]],
        [hi[0], lo[1], hi[2]],
        [hi[0], hi[1], hi[2]],
        [lo[0], hi[1], hi[2]],
    ];
    for (i, q) in points.iter().enumerate() {
        let id = base + i as u32;
        m.points.push(p::PointGeometry { id, position: p::Vector3::from_array(*q), owner: None, source: source_ref(1000 + id, 29) });
        m.vertices.push(p::Vertex { id, point: id, tolerance: Some(1e-8), owner: source_ref(70_000, 12), source: source_ref(2000 + id, 18) });
        m.bodies[0].vertices.push(id);
    }
    let mut edge_ids = std::collections::BTreeMap::new();
    let mut face_ids = Vec::new();
    for ring in [[0u32, 3, 2, 1], [4, 5, 6, 7], [0, 1, 5, 4], [1, 2, 6, 5], [2, 3, 7, 6], [3, 0, 4, 7]] {
        let fi = m.faces.len() as u32;
        let si = m.surfaces.len() as u32;
        let li = m.loops.len() as u32;
        face_ids.push(fi);
        let position = |i: u32| solidcraft_geom::Vec3::new(points[i as usize][0], points[i as usize][1], points[i as usize][2]);
        let a = position(ring[0]);
        let u = (position(ring[1]) - a).normalized().unwrap();
        let normal = u.cross(position(ring[2]) - a).normalized().unwrap();
        m.surfaces.push(p::SurfaceGeometry {
            id: si,
            sense: p::Sense::Positive,
            owner: None,
            kind: p::SurfaceKind::Plane {
                point: p::Vector3::from_array([a.x, a.y, a.z]),
                normal: p::Vector3::from_array([normal.x, normal.y, normal.z]),
                x_axis: p::Vector3::from_array([u.x, u.y, u.z]),
            },
            source: source_ref(3000 + si, 50),
        });
        let first = m.half_edges.len() as u32;
        let mut fins = Vec::new();
        for j in 0..4 {
            let start = base + ring[j];
            let end = base + ring[(j + 1) % 4];
            let key = (start.min(end), start.max(end));
            let ei = if let Some(id) = edge_ids.get(&key) {
                *id
            } else {
                let ei = m.edges.len() as u32;
                edge_ids.insert(key, ei);
                let c = m.curves.len() as u32;
                let point = m.points[start as usize].position;
                let q = m.points[end as usize].position;
                let d = solidcraft_geom::Vec3::new(q.x - point.x, q.y - point.y, q.z - point.z).normalized().unwrap();
                m.curves.push(p::CurveGeometry {
                    id: c,
                    sense: p::Sense::Positive,
                    owner: None,
                    kind: p::CurveKind::Line { point, direction: p::Vector3::from_array([d.x, d.y, d.z]) },
                    source: source_ref(4000 + c, 30),
                });
                m.edges.push(p::Edge {
                    id: ei,
                    owner: source_ref(70_000, 12),
                    half_edges: vec![],
                    start_vertex: Some(start),
                    end_vertex: Some(end),
                    curve: Some(c),
                    tolerance: None,
                    source: source_ref(5000 + ei, 16),
                });
                m.bodies[0].edges.push(ei);
                ei
            };
            let id = m.half_edges.len() as u32;
            let sense = if m.edges[ei as usize].start_vertex == Some(start) { p::Sense::Positive } else { p::Sense::Negative };
            m.edges[ei as usize].half_edges.push(id);
            fins.push(id);
            m.half_edges.push(p::HalfEdge {
                id,
                loop_id: Some(li),
                forward: Some(first + (j as u32 + 1) % 4),
                backward: Some(first + (j as u32 + 3) % 4),
                vertex: Some(end),
                other: None,
                edge: Some(ei),
                curve: None,
                sense,
                dummy: false,
                source: source_ref(6000 + id, 17),
            });
        }
        m.loops.push(p::Loop { id: li, face: fi, half_edges: fins, source: source_ref(7000 + li, 15) });
        m.faces.push(p::Face {
            id: fi,
            back_shell: if cavity { void_shell } else { material_shell },
            front_shell: if cavity { material_shell } else { void_shell },
            loops: vec![li],
            surface: Some(si),
            sense: p::Sense::Positive,
            source: source_ref(80_000 + fi, 14),
        });
    }
    for ei in edge_ids.values() {
        let h = m.edges[*ei as usize].half_edges.clone();
        assert_eq!(h.len(), 2);
        m.half_edges[h[0] as usize].other = Some(h[1]);
        m.half_edges[h[1] as usize].other = Some(h[0]);
    }
    m.shells.push(p::Shell {
        id: material_shell,
        region: 1,
        back_faces: if cavity { vec![] } else { face_ids.clone() },
        front_faces: if cavity { face_ids.clone() } else { vec![] },
        wire_edges: vec![],
        isolated_vertex: None,
        source: source_ref(8000 + material_shell, 13),
    });
    m.shells.push(p::Shell {
        id: void_shell,
        region: opposite,
        back_faces: if cavity { face_ids.clone() } else { vec![] },
        front_faces: if cavity { vec![] } else { face_ids },
        wire_edges: vec![],
        isolated_vertex: None,
        source: source_ref(8000 + void_shell, 13),
    });
    m.regions[1].shells.insert(0, material_shell);
    m.regions[opposite as usize].shells.push(void_shell);
}
#[test]
#[cfg(not(target_arch = "wasm32"))]
fn solidworks_native_source_keeps_dimensions_and_body_face_colors() {
    let m = native_box_fixture();
    let colors = std::collections::BTreeMap::from([(70_000, [0.1, 0.4, 0.2]), (80_000, [1., 0., 0.])]);
    let step = super::parasolid::test_emit(&m, &colors).unwrap();
    let imported = solidcraft_kernel::step_import_shared(&step).unwrap();
    assert!(imported.warnings.is_empty());
    assert_eq!(imported.bodies.len(), 1);
    assert!(imported.bodies[0].closed);
    let b = &imported.bodies[0].body;
    let measured = solidcraft_kernel::measure_mass(b).unwrap();
    assert!((measured.volume - 24.).abs() < 1e-8);
    assert!((measured.bbox.max - solidcraft_geom::Vec3::new(2., 3., 4.)).len() < 1e-8);
    let mut found = vec![];
    if let Some(c) = b.color() {
        found.push(c)
    }
    if let Some(paint) = b.paint() {
        found.extend(paint.faces.iter().map(|f| f.color));
    }
    assert!(found.contains(&[1., 0., 0.]));
    assert!(found.contains(&[0.1, 0.4, 0.2]));
}
#[test]
#[cfg(not(target_arch = "wasm32"))]
fn solidworks_native_void_uses_exterior_adjacency_even_when_inner_shell_is_first() {
    use parasolid_core::brep as p;
    let mut m = native_box_fixture();
    m.regions.push(p::Region { id: 2, kind: p::RegionKind::Void, body: 0, shells: vec![], source: source_ref(12, 19) });
    m.bodies[0].regions.insert(1, 2);
    push_native_box(&mut m, [0.0005, 0.0005, 0.0005], [0.0015, 0.0025, 0.0035], 2, true);
    assert_eq!(m.regions[1].shells, [2, 0]);
    let step = super::parasolid::test_emit(&m, &Default::default()).unwrap();
    assert!(step.contains("BREP_WITH_VOIDS"));
    let imported = solidcraft_kernel::step_import_shared(&step).unwrap();
    assert!(imported.warnings.is_empty());
    assert_eq!(imported.bodies.len(), 1);
    assert!(imported.bodies[0].closed);
    assert!((solidcraft_kernel::measure_mass(&imported.bodies[0].body).unwrap().volume - 18.).abs() < 1e-8);
}
#[test]
#[cfg(not(target_arch = "wasm32"))]
fn solidworks_native_tolerant_pcurve_keeps_a_solid_and_rejects_bad_controls() {
    use parasolid_core::brep as p;
    let mut m = native_box_fixture();
    let uv = m.curves.len() as u32;
    m.curves.push(p::CurveGeometry {
        id: uv,
        sense: p::Sense::Positive,
        owner: None,
        kind: p::CurveKind::Nurbs(p::NurbsCurve {
            degree: 1,
            control_vertex_count: 2,
            vertex_dimension: 2,
            knot_type: 0,
            periodic: false,
            closed: false,
            rational: false,
            curve_form: 0,
            control_vertices: vec![vec![0., 0.], vec![0.003, 0.]],
            knots: vec![0., 1.],
            knot_multiplicities: vec![2, 2],
            sources: vec![],
        }),
        source: source_ref(9000, 134),
    });
    m.curves.push(p::CurveGeometry {
        id: uv + 1,
        sense: p::Sense::Positive,
        owner: None,
        kind: p::CurveKind::SurfaceParametric { surface: 0, parameter_curve: uv, original_curve: None, tolerance_to_original: None },
        source: source_ref(9001, 137),
    });
    let start = m.points[0].position;
    let end = m.points[3].position;
    m.curves.push(p::CurveGeometry {
        id: uv + 2,
        sense: p::Sense::Positive,
        owner: None,
        kind: p::CurveKind::Trimmed { basis_curve: uv + 1, start_point: start, end_point: end, start_parameter: 0., end_parameter: 1. },
        source: source_ref(9002, 133),
    });
    m.edges[0].curve = None;
    m.edges[0].tolerance = Some(1e-7);
    for id in m.edges[0].half_edges.clone() {
        m.half_edges[id as usize].curve = Some(uv + 2);
    }
    let step = super::parasolid::test_emit(&m, &Default::default()).unwrap();
    let imported = solidcraft_kernel::step_import_shared(&step).unwrap();
    assert!(imported.warnings.is_empty());
    assert!(imported.bodies[0].closed);
    assert!((solidcraft_kernel::measure_mass(&imported.bodies[0].body).unwrap().volume - 24.).abs() < 1e-8);
    let p::CurveKind::Trimmed { start_point, end_point, start_parameter, end_parameter, .. } = &mut m.curves[(uv + 2) as usize].kind else {
        panic!("fixture")
    };
    std::mem::swap(start_point, end_point);
    std::mem::swap(start_parameter, end_parameter);
    let reverse_step = super::parasolid::test_emit(&m, &Default::default()).unwrap();
    let reverse_import = solidcraft_kernel::step_import_shared(&reverse_step).unwrap();
    assert!(reverse_import.warnings.is_empty());
    assert!(reverse_import.bodies[0].closed);
    assert!((solidcraft_kernel::measure_mass(&reverse_import.bodies[0].body).unwrap().volume - 24.).abs() < 1e-8);
    let p::CurveKind::Nurbs(n) = &mut m.curves[uv as usize].kind else { panic!("fixture") };
    n.control_vertices = vec![vec![0.], vec![1.]];
    let result = std::panic::catch_unwind(|| super::parasolid::test_emit(&m, &Default::default()));
    assert!(result.unwrap().is_err());
}

#[test]
#[cfg(not(target_arch = "wasm32"))]
fn solidworks_intersection_uses_supports_instead_of_its_coarse_chords() {
    use parasolid_core::brep as p;
    let mut m = native_box_fixture();
    for (i, axis, radius) in [(0, [0., 0., 1.], 0.001_f64), (1, [1., 0., 0.], 0.002)] {
        m.surfaces[i].kind = p::SurfaceKind::Cylinder {
            point: p::Vector3::from_array([0.; 3]),
            axis: p::Vector3::from_array(axis),
            radius,
            x_axis: p::Vector3::from_array(if i == 0 { [1., 0., 0.] } else { [0., 1., 0.] }),
        };
    }
    let chart = (0..=8)
        .map(|i| {
            let a = std::f64::consts::TAU * i as f64 / 8.;
            let (s, c) = a.sin_cos();
            p::Vector3::from_array([0.001 * c, 0.001 * s, (4e-6 - 1e-6 * s * s).sqrt()])
        })
        .collect();
    let id = m.curves.len() as u32;
    m.curves.push(p::CurveGeometry {
        id,
        sense: p::Sense::Positive,
        owner: None,
        kind: p::CurveKind::Intersection {
            surfaces: [0, 1],
            chart: source_ref(100, 40),
            chart_points: chart,
            start: source_ref(101, 41),
            start_points: vec![],
            end: source_ref(102, 41),
            end_points: vec![],
            intersection_data: None,
        },
        source: source_ref(103, 38),
    });
    m.edges[0].curve = Some(id);
    m.edges[0].end_vertex = m.edges[0].start_vertex;
    for [x, y, z] in super::parasolid::test_intersection_samples(m, id).unwrap() {
        assert!((x * x + y * y - 1e-6).abs() < 5e-12);
        assert!((y * y + z * z - 4e-6).abs() < 5e-12);
        assert!(z > 0.0017);
    }
}
#[test]
#[cfg(not(target_arch = "wasm32"))]
fn solidworks_intersection_projects_onto_a_nurbs_support_with_source_precision() {
    use parasolid_core::brep as p;
    let mut m = native_box_fixture();
    m.surfaces[0].kind = p::SurfaceKind::Nurbs(p::NurbsSurface {
        u_degree: 1,
        v_degree: 1,
        u_control_vertex_count: 2,
        v_control_vertex_count: 2,
        vertex_dimension: 3,
        u_knot_type: 0,
        v_knot_type: 0,
        u_periodic: false,
        v_periodic: false,
        u_closed: false,
        v_closed: false,
        rational: false,
        surface_form: 0,
        control_vertices: vec![vec![-0.003, -0.003, 0.001], vec![-0.003, 0.003, 0.001], vec![0.003, -0.003, 0.001], vec![0.003, 0.003, 0.001]],
        u_knots: vec![0., 1.],
        v_knots: vec![0., 1.],
        u_knot_multiplicities: vec![2, 2],
        v_knot_multiplicities: vec![2, 2],
        sources: vec![],
    });
    m.surfaces[1].kind = p::SurfaceKind::Cylinder {
        point: p::Vector3::from_array([0.; 3]),
        axis: p::Vector3::from_array([0., 0., 1.]),
        radius: 0.002,
        x_axis: p::Vector3::from_array([1., 0., 0.]),
    };
    let chart = (0..=8)
        .map(|i| {
            let a = std::f64::consts::TAU * i as f64 / 8.;
            p::Vector3::from_array([0.0020002 * a.cos(), 0.0020002 * a.sin(), 0.00100005])
        })
        .collect();
    let id = m.curves.len() as u32;
    m.curves.push(p::CurveGeometry {
        id,
        sense: p::Sense::Positive,
        owner: None,
        kind: p::CurveKind::Intersection {
            surfaces: [0, 1],
            chart: source_ref(100, 40),
            chart_points: chart,
            start: source_ref(101, 41),
            start_points: vec![],
            end: source_ref(102, 41),
            end_points: vec![],
            intersection_data: None,
        },
        source: source_ref(103, 38),
    });
    m.edges[0].curve = Some(id);
    m.edges[0].end_vertex = m.edges[0].start_vertex;
    for [x, y, z] in super::parasolid::test_intersection_samples(m, id).unwrap() {
        assert!((x * x + y * y - 4e-6).abs() < 5e-12);
        assert!((z - 0.001).abs() < 1e-10);
    }
}

#[test]
#[cfg(not(target_arch = "wasm32"))]
fn solidworks_reused_part_rejects_conflicting_configuration_identity() {
    let mut root = super::assembly::plan(&assembly_container(assembly_xml("1 0 0 0 0 1 0 0 0 0 1 0 0 0 0 1").as_bytes()), None).unwrap();
    let mut duplicate = root.children[0].clone();
    duplicate.source_index = Some(99);
    root.children[1].hidden = false;
    root.children.push(duplicate);
    let body = solidcraft_kernel::box_solid(solidcraft_geom::Vec3::ZERO, solidcraft_geom::Vec3::new(1., 1., 1.)).unwrap();
    let step = solidcraft_kernel::step_export(&[&body], "native-test").unwrap();
    assert!(super::assembly::restate(&root, |_, _| Ok(step.clone())).unwrap_err().contains("conflicting configuration"));
}

#[test]
#[cfg(not(target_arch = "wasm32"))]
fn solidworks_cache_metadata_is_crc_checked_and_excludes_unrelated_payloads() {
    let mut input = vec![0x9d, 0x51, 0x9b, 0xc0, 0, 0, 0, 4];
    let original = br#"<swSolidWorks swVersion="17000"/>"#;
    super::selection::append_section(&mut input, "swXmlContents/Features", original).unwrap();
    super::selection::append_section(&mut input, "Contents/CMgr", b"feature classes").unwrap();
    super::selection::append_section(&mut input, "Config-0-FeatureBodies/LocalBodies", &[42; 8192]).unwrap();
    super::selection::append_section(&mut input, "Contents/Config-1-Partition", &[43; 8192]).unwrap();
    let before = input.clone();
    let projection = super::selection::metadata(&input, 0).unwrap();
    assert_eq!(input, before);
    assert!(projection.len() < input.len());
    assert_eq!(super::assembly::section(&projection, "swXmlContents/Features", 1024).unwrap().unwrap(), original);
    assert!(super::assembly::section(&projection, "Config-0-FeatureBodies/LocalBodies", 8192).unwrap().is_none());
    assert!(super::assembly::section(&projection, "Contents/Config-1-Partition", 8192).unwrap().is_none());
    // Corrupt the original XML block's CRC. A fresh projection must not repair it.
    input[18] ^= 1;
    assert!(super::selection::metadata(&input, 0).is_err());
}

#[test]
#[cfg(not(target_arch = "wasm32"))]
fn solidworks_metadata_projection_handles_the_official_default_namespace() {
    let mut input = vec![0x9d, 0x51, 0x9b, 0xc0, 0, 0, 0, 4];
    let xml = br#"<swSolidWorks xmlns="http://www.solidworks.com/sw2003/schema" swVersion="17000"><swModel id="m" swConfigurationName="Main" swConfigurationId="0"/><swConfiguration swName="Main" swID="0" swModelRef="m" swMostRecentConfiguration="YES" swConfigurationNeedsUpdate="NO"/></swSolidWorks>"#;
    super::selection::append_section(&mut input, "swXmlContents/Features", xml).unwrap();
    super::selection::append_section(
        &mut input,
        "swXmlContents/KeyWords",
        br#"<Keywords id="1" Name="Test"><Configuration id="0" Name="Main" Type="ConfigurationManager" SourceIndex="0"/></Keywords>"#,
    )
    .unwrap();
    let projection = super::selection::metadata(&input, 0).unwrap();
    let raw = super::assembly::section(&projection, "swXmlContents/Features", 8192).unwrap().unwrap();
    let document = String::from_utf8(raw).unwrap();
    assert!(!document.contains("xmlns="));
    assert!(document.contains("swConfigurationName=\"Main\""));
    let catalog = cadmpeg_registry::InputCatalog::with_builtins();
    let codec = catalog.descriptors().find(|d| d.format_id().as_str() == "sldprt").and_then(|d| d.codec()).unwrap();
    let decoded = codec.decode(&mut std::io::Cursor::new(&projection), &cadmpeg_ir::codec::DecodeOptions::default()).unwrap();
    let (ir, _, _) = decoded.into_parts();
    assert!(ir.model.configurations.iter().any(|c| c.active && c.source_index == Some(0)));
}

#[test]
#[cfg(not(target_arch = "wasm32"))]
fn solidworks_native_body_color_v2_resolves_its_owner_and_rejects_conflicts() {
    use parasolid_core::{BuiltinProfileRegistry, FieldValue as V, RawField, RawNode, SchemaKey, SchemaProvider};
    let registry = BuiltinProfileRegistry::compiled().unwrap();
    let key = SchemaKey::parse("SCH_3501251_35102_13006").unwrap();
    let provider = registry.provider_for_key(&key).unwrap();
    let node = |kind, index, fields: Vec<(&str, Vec<V>)>| {
        let definition = provider.type_definition("13006", kind).unwrap().clone();
        let fields = fields
            .into_iter()
            .map(|(name, values)| RawField {
                definition: definition.fields.iter().find(|f| f.name == name).unwrap().clone(),
                values,
                byte_range: 0..0,
            })
            .collect();
        RawNode { node_type: kind, index, variable_length: None, definition, first_schema: None, fields, byte_range: 0..0, user_fields: vec![] }
    };
    let mut nodes = vec![
        node(79, 10, vec![("identifier_bytes", b"SDL/TYSA_COLOUR_2".iter().copied().map(V::Character).collect())]),
        node(80, 11, vec![("identifier_ref", vec![V::PointerIndex(10)])]),
        node(
            81,
            12,
            vec![
                ("definition_ref", vec![V::PointerIndex(11)]),
                ("owner_ref", vec![V::PointerIndex(70_000)]),
                ("value_arrays", vec![V::PointerIndex(13)]),
            ],
        ),
        node(83, 13, vec![("reals", vec![V::Double(Some(0.)), V::Double(Some(0.5)), V::Double(Some(0.))])]),
    ];
    assert_eq!(super::parasolid::test_colors(&nodes).unwrap().get(&70_000), Some(&[0., 0.5, 0.]));
    nodes.push(node(
        81,
        14,
        vec![
            ("definition_ref", vec![V::PointerIndex(11)]),
            ("owner_ref", vec![V::PointerIndex(70_000)]),
            ("value_arrays", vec![V::PointerIndex(15)]),
        ],
    ));
    nodes.push(node(83, 15, vec![("reals", vec![V::Double(Some(1.)), V::Double(Some(0.)), V::Double(Some(0.))])]));
    assert!(super::parasolid::test_colors(&nodes).unwrap_err().contains("conflicting"));
}

#[test]
#[cfg(not(target_arch = "wasm32"))]
fn solidworks_nurbs_surface_basis_preserves_rational_cylinder_and_rejects_bad_dimensions() {
    use parasolid_core::brep as p;
    let mut m = native_box_fixture();
    let w = std::f64::consts::FRAC_1_SQRT_2;
    m.surfaces[0].kind = p::SurfaceKind::Nurbs(p::NurbsSurface {
        u_degree: 2,
        v_degree: 1,
        u_control_vertex_count: 3,
        v_control_vertex_count: 2,
        vertex_dimension: 4,
        u_knot_type: 0,
        v_knot_type: 0,
        u_periodic: false,
        v_periodic: false,
        u_closed: false,
        v_closed: false,
        rational: true,
        surface_form: 0,
        control_vertices: vec![
            vec![0.002, 0., 0., 1.],
            vec![0.002, 0., 0.003, 1.],
            vec![0.002 * w, 0.002 * w, 0., w],
            vec![0.002 * w, 0.002 * w, 0.003 * w, w],
            vec![0., 0.002, 0., 1.],
            vec![0., 0.002, 0.003, 1.],
        ],
        u_knots: vec![0., 1.],
        v_knots: vec![0., 1.],
        u_knot_multiplicities: vec![3, 3],
        v_knot_multiplicities: vec![2, 2],
        sources: vec![],
    });
    for u in [0., 0.13, 0.37, 0.83, 1.] {
        for v in [0., 0.31, 1.] {
            let [x, y, z] = super::parasolid::test_surface(&m, [u, v, 0.]).unwrap();
            assert!((x * x + y * y - 4e-6).abs() < 1e-18);
            assert!((z - 0.003 * v).abs() < 1e-15);
        }
    }
    let p::SurfaceKind::Nurbs(n) = &mut m.surfaces[0].kind else { panic!("fixture") };
    n.u_degree = 33;
    assert!(std::panic::catch_unwind(|| super::parasolid::test_surface(&m, [0., 0., 0.])).unwrap().is_err());
}

#[test]
#[cfg(not(target_arch = "wasm32"))]
fn solidworks_intersection_only_fits_the_interval_used_by_its_edge_vertices() {
    use parasolid_core::brep as p;
    let mut m = native_box_fixture();
    let y = 3_f64.sqrt() * 0.001;
    m.surfaces[0].kind = p::SurfaceKind::Nurbs(p::NurbsSurface {
        u_degree: 1,
        v_degree: 1,
        u_control_vertex_count: 2,
        v_control_vertex_count: 2,
        vertex_dimension: 3,
        u_knot_type: 0,
        v_knot_type: 0,
        u_periodic: false,
        v_periodic: false,
        u_closed: false,
        v_closed: false,
        rational: false,
        surface_form: 0,
        control_vertices: vec![vec![0.001, -0.003, -0.001], vec![0.001, -0.003, 0.001], vec![0.001, 0.003, -0.001], vec![0.001, 0.003, 0.001]],
        u_knots: vec![0., 1.],
        v_knots: vec![0., 1.],
        u_knot_multiplicities: vec![2, 2],
        v_knot_multiplicities: vec![2, 2],
        sources: vec![],
    });
    m.surfaces[1].kind = p::SurfaceKind::Cylinder {
        point: p::Vector3::from_array([0.; 3]),
        axis: p::Vector3::from_array([0., 0., 1.]),
        x_axis: p::Vector3::from_array([1., 0., 0.]),
        radius: 0.002,
    };
    for (v, z) in [(m.edges[0].start_vertex.unwrap(), -0.0005), (m.edges[0].end_vertex.unwrap(), 0.0005)] {
        let p = m.vertices[v as usize].point as usize;
        m.points[p].position = p::Vector3::from_array([0.001, y, z]);
    }
    let id = m.curves.len() as u32;
    m.curves.push(p::CurveGeometry {
        id,
        sense: p::Sense::Positive,
        owner: None,
        kind: p::CurveKind::Intersection {
            surfaces: [0, 1],
            chart: source_ref(100, 40),
            chart_points: [-0.01, -0.001, 0., 0.001, 0.01].into_iter().map(|z| p::Vector3::from_array([0.001, y, z])).collect(),
            start: source_ref(101, 41),
            start_points: vec![],
            end: source_ref(102, 41),
            end_points: vec![],
            intersection_data: None,
        },
        source: source_ref(103, 38),
    });
    m.edges[0].curve = Some(id);
    let samples = super::parasolid::test_intersection_samples(m, id).unwrap();
    for [x, q, z] in samples {
        assert!((x - 0.001).abs() < 1e-10);
        assert!((q - y).abs() < 1e-10);
        assert!(z.abs() < 0.000502);
    }
}

#[test]
#[cfg(not(target_arch = "wasm32"))]
fn solidworks_saved_configuration_must_match_independent_assembly_dimensions() {
    let body = solidcraft_kernel::box_solid(solidcraft_geom::Vec3::ZERO, solidcraft_geom::Vec3::new(2., 3., 4.)).unwrap();
    let text = solidcraft_kernel::step_export(&[&body], "saved bounds").unwrap();
    super::assembly::check_saved_bounds(&text, [[0.; 3], [2., 3., 4.]]).unwrap();
    assert!(super::assembly::check_saved_bounds(&text, [[0.; 3], [2., 3., 5.]]).unwrap_err().contains("dimensions disagree"));
}

#[test]
#[cfg(not(target_arch = "wasm32"))]
fn solidworks_source_assembly_keeps_voids_and_owned_colors_without_rewriting_geometry() {
    use parasolid_core::brep as p;
    let mut m = native_box_fixture();
    m.regions.push(p::Region { id: 2, kind: p::RegionKind::Void, body: 0, shells: vec![], source: source_ref(12, 19) });
    m.bodies[0].regions.insert(1, 2);
    push_native_box(&mut m, [0.0005, 0.0005, 0.0005], [0.0015, 0.0025, 0.0035], 2, true);
    let colors = std::collections::BTreeMap::from([(m.bodies[0].source.node_index, [0., 1., 0.]), (m.faces[0].source.node_index, [1., 0., 0.])]);
    let source = super::parasolid::test_emit(&m, &colors).unwrap();
    let mut root = super::assembly::plan(&assembly_container(assembly_xml("1 0 0 0 0 1 0 0 0 0 1 0 0.012 -0.034 0.056 1").as_bytes()), None).unwrap();
    root.children[1].hidden = false;
    let text = super::assembly::restate_nodes(&root, |_| Ok(source.clone()), true).unwrap();
    assert_eq!(text.matches("BREP_WITH_VOIDS").count(), 2);
    let imported = solidcraft_kernel::step_import_shared(&text).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert_eq!(imported.tree[0].children.len(), 2);
    assert_eq!(imported.bodies.len(), 2);
    for body in &imported.bodies {
        assert!(body.closed);
        assert_eq!(body.body.shell_count(), 2);
        assert!((solidcraft_kernel::measure_mass(&body.body).unwrap().volume - 18.).abs() < 1e-8);
        let mut found = Vec::new();
        if let Some(c) = body.body.color() {
            found.push(c);
        }
        if let Some(p) = body.body.paint() {
            found.extend(p.faces.iter().map(|f| f.color));
        }
        assert!(found.contains(&[0., 1., 0.]));
        assert!(found.contains(&[1., 0., 0.]));
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn native_sheet_fixture() -> parasolid_core::brep::BrepModel {
    use parasolid_core::brep as p;
    let mut m = native_box_fixture();
    m.bodies[0].kind = p::BodyKind::Sheet;
    m.bodies[0].regions = vec![0];
    m.regions.truncate(1);
    m.regions[0].shells = vec![0];
    m.shells.truncate(1);
    m.shells[0].region = 0;
    m.shells[0].back_faces = vec![0];
    m.shells[0].front_faces = vec![0];
    m.faces.truncate(1);
    m.faces[0].back_shell = 0;
    m.faces[0].front_shell = 0;
    m
}

#[test]
#[cfg(not(target_arch = "wasm32"))]
fn solidworks_native_sheet_keeps_one_face_both_sides_and_colors_without_volume() {
    let m = native_sheet_fixture();
    let colors = [(70_000, [0.8, 0.8, 0.8]), (80_000, [1., 1., 1.])].into_iter().collect();
    let step = super::parasolid::test_emit(&m, &colors).unwrap();
    assert_eq!(step.matches("OPEN_SHELL(").count(), 1);
    assert!(!step.contains("MANIFOLD_SOLID_BREP("));
    assert!(!step.contains("CLOSED_SHELL("));
    let imported = solidcraft_kernel::step_import_shared(&step).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert_eq!(imported.bodies.len(), 1);
    let b = &imported.bodies[0];
    assert!(!b.closed);
    assert_eq!(b.file_faces, 1, "front/back membership must not duplicate faces");
    assert_eq!(b.body.face_count(), 1);
    assert_eq!(b.body.color(), Some([0.8, 0.8, 0.8]));
    assert_eq!(b.body.paint().unwrap().faces[0].color, [1., 1., 1.]);
    let bounds = b.body.outline_bounds();
    assert!((bounds.max.x - 2.).abs() < 1e-8 && (bounds.max.y - 3.).abs() < 1e-8);
    assert!(bounds.min.z.abs() < 1e-8 && bounds.max.z.abs() < 1e-8);
}

#[test]
#[cfg(not(target_arch = "wasm32"))]
fn solidworks_native_pcurve_ring_remains_an_open_surface_and_rejects_open_trims() {
    use parasolid_core::brep as p;
    let mut m = native_sheet_fixture();
    let circle = m.curves.len() as u32;
    let w = std::f64::consts::FRAC_1_SQRT_2;
    m.curves.push(p::CurveGeometry {
        id: circle,
        sense: p::Sense::Positive,
        owner: None,
        kind: p::CurveKind::Nurbs(p::NurbsCurve {
            degree: 2,
            control_vertex_count: 9,
            vertex_dimension: 3,
            knot_type: 0,
            periodic: false,
            closed: true,
            rational: true,
            curve_form: 0,
            control_vertices: vec![
                vec![0.001, 0., 1.],
                vec![0.001 * w, 0.001 * w, w],
                vec![0., 0.001, 1.],
                vec![-0.001 * w, 0.001 * w, w],
                vec![-0.001, 0., 1.],
                vec![-0.001 * w, -0.001 * w, w],
                vec![0., -0.001, 1.],
                vec![0.001 * w, -0.001 * w, w],
                vec![0.001, 0., 1.],
            ],
            knots: vec![0., 1., 2., 3., 4.],
            knot_multiplicities: vec![3, 2, 2, 2, 3],
            sources: vec![],
        }),
        source: source_ref(9000, 134),
    });
    m.curves.push(p::CurveGeometry {
        id: circle + 1,
        sense: p::Sense::Positive,
        owner: None,
        kind: p::CurveKind::SurfaceParametric { surface: 0, parameter_curve: circle, original_curve: None, tolerance_to_original: None },
        source: source_ref(9001, 137),
    });
    // Surface 0's local X is world +Y, local Y is world +X.
    let start = p::Vector3::from_array([0., 0.001, 0.]);
    m.curves.push(p::CurveGeometry {
        id: circle + 2,
        sense: p::Sense::Positive,
        owner: None,
        kind: p::CurveKind::Trimmed { basis_curve: circle + 1, start_point: start, end_point: start, start_parameter: 0., end_parameter: 4. },
        source: source_ref(9002, 133),
    });
    m.edges.truncate(1);
    m.edges[0].curve = None;
    m.edges[0].start_vertex = None;
    m.edges[0].end_vertex = None;
    m.edges[0].tolerance = Some(1e-6);
    m.edges[0].half_edges = vec![0];
    m.half_edges.truncate(1);
    m.half_edges[0].curve = Some(circle + 2);
    m.half_edges[0].sense = p::Sense::Positive;
    m.loops.truncate(1);
    m.loops[0].half_edges = vec![0];
    let step = super::parasolid::test_emit(&m, &Default::default()).unwrap();
    let imported = solidcraft_kernel::step_import_shared(&step).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert_eq!(imported.bodies.len(), 1);
    assert!(!imported.bodies[0].closed);
    assert_eq!(imported.bodies[0].body.face_count(), 1);
    let b = imported.bodies[0].body.outline_bounds();
    assert!((b.max.x - 1.).abs() < 1e-4 && (b.min.y + 1.).abs() < 1e-4);
    let p::CurveKind::Trimmed { end_parameter, .. } = &mut m.curves[(circle + 2) as usize].kind else { panic!("fixture") };
    *end_parameter = 2.;
    assert!(super::parasolid::test_emit(&m, &Default::default()).unwrap_err().contains("not closed"));
}
