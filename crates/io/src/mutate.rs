//! Every reader of bytes the user didn't write, run over mutations of a valid file (#469): each
//! case gives an error or a result, never a panic, within the time budget and the reader's own
//! caps. The mutations and the check are `solidcraft_testkit`'s.

use solidcraft_geom::{Mesh, Vec2, Vec3};
use solidcraft_testkit::{mutations, survives};

fn cube() -> Mesh {
    let positions = (0..8).map(|i| Vec3::new(f64::from(i & 1) * 10.0, f64::from((i >> 1) & 1) * 10.0, f64::from((i >> 2) & 1) * 10.0)).collect();
    let t = [[0, 2, 1], [1, 2, 3], [4, 5, 6], [5, 7, 6], [0, 1, 4], [1, 5, 4], [2, 6, 3], [3, 6, 7], [0, 4, 2], [2, 4, 6], [1, 3, 5], [3, 7, 5]];
    Mesh { positions, triangles: t.to_vec(), ..Default::default() }
}

#[test]
fn stl_survives_mutation() {
    let mesh = cube();
    let binary = crate::stl_binary(std::slice::from_ref(&mesh), "fuzz");
    let ascii = crate::stl_ascii(std::slice::from_ref(&mesh), "fuzz").into_bytes();
    assert_eq!(crate::read_stl(&binary).map(|t| t.len()).ok(), Some(12));
    assert_eq!(crate::read_stl(&ascii).map(|t| t.len()).ok(), Some(12));
    let read = |b: &[u8]| crate::read_stl(b).ok().map(|t| t.len());
    survives("binary STL", &mutations(&binary, false, 0x5717), crate::MAX_STL_TRIANGLES, read);
    survives("ASCII STL", &mutations(&ascii, true, 0x5718), crate::MAX_STL_TRIANGLES, read);
}

#[test]
fn obj_survives_mutation() {
    let good = crate::obj(&[("Cube".into(), cube())]).into_bytes();
    assert!(crate::read_obj(&good, "fuzz").is_ok());
    survives("OBJ", &mutations(&good, true, 0x0b1), usize::MAX, |b| {
        crate::read_obj(b, "fuzz").ok().map(|(objects, _)| objects.iter().map(|o| o.triangles.len()).sum())
    });
}

#[test]
fn svg_survives_mutation() {
    let good = br##"<svg xmlns="http://www.w3.org/2000/svg" width="100mm" height="50mm" viewBox="0 0 100 50">
<g transform="translate(10 5) rotate(15) scale(1.5)">
<rect x="1" y="2" width="30" height="4" rx="1"/>
<circle cx="20" cy="20" r="5"/>
<ellipse cx="40" cy="20" rx="6" ry="3"/>
<polyline points="0,0 10,10 20,0"/>
<path d="M0 0 L5 5 H10 V0 C12 2 14 2 16 0 Q18 3 20 0 A5 5 0 0 1 30 0 Z"/>
</g>
<use href="#a"/><g id="a"><line x1="0" y1="0" x2="5" y2="5"/></g>
</svg>
"##;
    assert!(crate::read_svg(std::str::from_utf8(good).unwrap()).is_ok());
    survives("SVG", &mutations(good, true, 0x5f6), usize::MAX, |b| crate::read_svg(&String::from_utf8_lossy(b)).ok().map(|g| g.len()));
}

#[test]
fn dxf_survives_mutation() {
    let geom = [
        crate::Geom2::Line(Vec2::new(0.0, 0.0), Vec2::new(10.0, 0.0)),
        crate::Geom2::Circle(Vec2::new(5.0, 5.0), 2.5),
        crate::Geom2::Arc { c: Vec2::new(0.0, 0.0), a: Vec2::new(3.0, 0.0), b: Vec2::new(0.0, 3.0) },
        crate::Geom2::Point(Vec2::new(1.0, 1.0)),
    ];
    let written = crate::write_dxf(&geom).into_bytes();
    assert!(crate::read_dxf(std::str::from_utf8(&written).unwrap()).is_ok_and(|g| g.len() >= 3));
    survives("DXF (written)", &mutations(&written, true, 0xdf), usize::MAX, |b| crate::read_dxf(&String::from_utf8_lossy(b)).ok().map(|g| g.len()));
    // Blocks inside blocks, an array insert and a bulged polyline: what the block recursion,
    // the array cells and the work budget guard.
    let pairs: &[(&str, &str)] = &[
        ("0", "SECTION"),
        ("2", "BLOCKS"),
        ("0", "BLOCK"),
        ("2", "inner"),
        ("10", "0"),
        ("20", "0"),
        ("0", "LINE"),
        ("10", "0"),
        ("20", "0"),
        ("11", "1"),
        ("21", "1"),
        ("0", "ENDBLK"),
        ("0", "BLOCK"),
        ("2", "outer"),
        ("10", "0"),
        ("20", "0"),
        ("0", "INSERT"),
        ("2", "inner"),
        ("10", "2"),
        ("20", "2"),
        ("0", "CIRCLE"),
        ("10", "0"),
        ("20", "0"),
        ("40", "1"),
        ("0", "ENDBLK"),
        ("0", "ENDSEC"),
        ("0", "SECTION"),
        ("2", "ENTITIES"),
        ("0", "INSERT"),
        ("2", "outer"),
        ("10", "5"),
        ("20", "5"),
        ("41", "2"),
        ("42", "2"),
        ("50", "30"),
        ("70", "3"),
        ("71", "2"),
        ("44", "10"),
        ("45", "10"),
        ("0", "LWPOLYLINE"),
        ("90", "3"),
        ("70", "1"),
        ("10", "0"),
        ("20", "0"),
        ("42", "0.5"),
        ("10", "5"),
        ("20", "0"),
        ("10", "5"),
        ("20", "5"),
        ("0", "ENDSEC"),
        ("0", "EOF"),
    ];
    let blocks: String = pairs
        .iter()
        .map(|(c, v)| {
            format!(
                "{c}
{v}
"
            )
        })
        .collect();
    let n = crate::read_dxf(&blocks).map(|g| g.len()).unwrap_or(0);
    assert!(n >= 12, "the array insert expands its nested blocks: {n} entities");
    survives("DXF (blocks)", &mutations(blocks.as_bytes(), true, 0xdb), usize::MAX, |b| {
        crate::read_dxf(&String::from_utf8_lossy(b)).ok().map(|g| g.len())
    });
    // The two lies that need each other, written out: a block that inserts itself (the
    // depth limit), and an array of 10,000 by 10,000 copies (the cell and work budgets).
    let looped = blocks.replacen(
        "2
inner
10
2",
        "2
outer
10
2",
        1,
    );
    assert_ne!(looped, blocks, "the outer block now inserts itself");
    let huge = blocks.replacen(
        "70
3
71
2",
        "70
10000
71
10000",
        1,
    );
    assert_ne!(huge, blocks);
    survives("DXF (self-insert, huge array)", &[looped.into_bytes(), huge.into_bytes()], usize::MAX, |b| {
        crate::read_dxf(&String::from_utf8_lossy(b)).ok().map(|g| g.len())
    });
}

#[test]
fn threemf_survives_mutation() {
    let mesh = cube();
    let object = crate::MeshObject {
        name: "Cube".into(),
        positions: mesh.positions,
        triangles: mesh.triangles,
        color: Some([0.8, 0.2, 0.1]),
        ..Default::default()
    };
    let good = crate::write_3mf(&[object]).unwrap();
    assert!(crate::read_3mf(&good).is_ok());
    survives("3MF", &mutations(&good, false, 0x3f), usize::MAX, |b| {
        crate::read_3mf(b).ok().map(|(objects, _)| objects.iter().map(|o| o.triangles.len()).sum())
    });
    // Objects in another model part (production extension), as slicers save projects.
    let production = crate::threemf::tests::production_package();
    assert!(crate::read_3mf(&production).is_ok());
    survives("3MF (production extension)", &mutations(&production, false, 0x2089), usize::MAX, |b| {
        crate::read_3mf(b).ok().map(|(objects, _)| objects.iter().map(|o| o.triangles.len()).sum())
    });
    // Its model parts' XML mutated and zipped again: mutations of the zip mostly stop at its
    // checksums, these reach the `p:path` and `objectid` handling.
    let parts = crate::threemf::tests::production_parts();
    for (k, (name, xml)) in parts.iter().enumerate() {
        let rezipped: Vec<Vec<u8>> = mutations(xml.as_bytes(), true, 0x2089 + k as u64)
            .iter()
            .map(|m| {
                let entries: Vec<(&str, &[u8])> = parts.iter().map(|(n, x)| (*n, if n == name { m.as_slice() } else { x.as_bytes() })).collect();
                crate::threemf::tests::package_bytes(&entries)
            })
            .collect();
        survives(&format!("3MF (production extension, {name} mutated)"), &rezipped, usize::MAX, |b| {
            crate::read_3mf(b).ok().map(|(objects, _)| objects.iter().map(|o| o.triangles.len()).sum())
        });
    }
}

#[test]
fn gltf_survives_mutation() {
    let mut f = crate::gltf_in::tests::Fixture::default();
    let m = f.cube_mesh("Cube", 0.01);
    let child = f.node(serde_json::json!({"name": "C", "mesh": m, "translation": [0.0, 0.1, 0.0]}));
    let n = f.node(
        serde_json::json!({"mesh": m, "children": [child], "rotation": [0.0, 0.0, std::f64::consts::FRAC_1_SQRT_2, std::f64::consts::FRAC_1_SQRT_2]}),
    );
    f.roots = vec![n];
    let mut no_files = |name: &str| -> std::result::Result<Vec<u8>, String> { Err(format!("no file `{name}`")) };
    let glb = f.glb();
    assert!(crate::read_gltf(&glb, "fuzz.glb", &mut no_files).is_ok());
    survives("glTF (.glb)", &mutations(&glb, false, 0x91b), usize::MAX, |b| crate::read_gltf(b, "fuzz.glb", &mut no_files).ok().map(|_| 1));
    // The JSON form, its buffer embedded as a data URI.
    let uri = format!("data:application/octet-stream;base64,{}", base64(&f.bin));
    let gltf = f.json(Some(&uri)).to_string().into_bytes();
    assert!(crate::read_gltf(&gltf, "fuzz.gltf", &mut no_files).is_ok());
    survives("glTF (.gltf)", &mutations(&gltf, true, 0x917), usize::MAX, |b| crate::read_gltf(b, "fuzz.gltf", &mut no_files).ok().map(|_| 1));
}

#[test]
fn designs_survive_mutation() {
    let mut doc = solidcraft_doc::Document::new("Fuzz");
    doc.set_param("width", "40", Some("mm"), None).unwrap();
    let mesh = solidcraft_doc::MeshData {
        name: "Preview".into(),
        positions: vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0].into(),
        triangles: vec![0, 1, 2].into(),
        color: None,
    };
    let preview = crate::DesignPreview { meshes: vec![mesh] };
    let container = crate::write_design_container_with_preview(&doc, crate::DesignCompression::Fast, Some(&preview)).unwrap();
    assert!(crate::read_design(&container).is_ok() && crate::read_design_preview(&container).is_some());
    survives("design container", &mutations(&container, false, 0xde5), usize::MAX, |b| {
        let _ = crate::read_design_preview(b);
        let _ = crate::read_design_thumbnail(b);
        crate::read_design(b).ok().map(|d| d.features.len())
    });
    let json = crate::write_design(&doc);
    assert!(crate::read_design(&json).is_ok());
    survives("design JSON", &mutations(&json, true, 0xde6), usize::MAX, |b| crate::read_design(b).ok().map(|d| d.features.len()));
}

/// A drawing, as a container and as plain JSON: every mutation gives an error or a drawing,
/// never a panic, and `file_kind` always answers.
#[test]
fn drawings_survive_mutation() {
    use solidcraft_geom::drawing_sheet::{DrawingBorderProfile, Orientation, PaperSize, Sheet};
    let mut ids = solidcraft_doc::ids::IdSource::seeded(0xd4a);
    let a3 = Sheet::standard(PaperSize::A3, Orientation::Landscape);
    let mut drawing = solidcraft_doc::drawing::Drawing::new("Fuzz", "en", "Sheet 1", a3, Some(DrawingBorderProfile::iso()), &mut ids).unwrap();
    let id = drawing.allocate_id(&mut ids).unwrap();
    drawing.sources.push(solidcraft_doc::drawing::Source { id, path: "plate.sc".into() });
    let container = crate::write_drawing_container(&drawing).unwrap();
    assert!(crate::read_drawing(&container).is_ok());
    survives("drawing container", &mutations(&container, false, 0xd4b), usize::MAX, |b| {
        let _ = crate::file_kind(b);
        let _ = crate::read_design(b);
        crate::read_drawing(b).ok().map(|d| d.sheets.len())
    });
    let json = drawing.to_json().unwrap().into_bytes();
    assert!(crate::read_drawing(&json).is_ok());
    survives("drawing JSON", &mutations(&json, true, 0xd4c), usize::MAX, |b| {
        let _ = crate::file_kind(b);
        crate::read_drawing(b).ok().map(|d| d.sheets.len())
    });
}

/// A paper scene's JSON, mutated, read, prepared and written as SVG: an error or a document,
/// never a panic.
#[test]
fn paper_scenes_survive_mutation_into_svg() {
    use solidcraft_geom::paper::{Group, Item, ObjectKind, Scene};
    use solidcraft_geom::{Loop2, Region2, Seg2, Vec2};
    let p = Vec2::new;
    let border = Loop2::polygon(&[p(20.0, 10.0), p(410.0, 10.0), p(410.0, 287.0), p(20.0, 287.0)]);
    let items = vec![
        Item::Stroke { path: border.segs.clone(), closed: true, width_mm: 0.7 },
        Item::Stroke { path: vec![Seg2::Arc { center: p(50.0, 50.0), radius: 10.0, start: 0.0, sweep: 4.0 }], closed: false, width_mm: 0.35 },
        Item::Fill { region: Region2 { outer: border, holes: vec![] } },
    ];
    let scene = Scene { size_mm: p(420.0, 297.0), groups: vec![Group { object: 7, kind: ObjectKind::DrawingBorder, items }] };
    let json = serde_json::to_vec(&scene).unwrap();
    let write = |b: &[u8]| -> Option<usize> {
        let scene: Scene = serde_json::from_slice(b).ok()?;
        let prepared = scene.prepare().ok()?;
        crate::paper::paper_svg(&prepared).ok().map(|(bytes, _)| bytes.len())
    };
    assert!(write(&json).is_some());
    survives("paper scene to SVG", &mutations(&json, true, 0x5a3), usize::MAX, write);
}

/// Standard base64 (RFC 4648) of `bytes`, for a data URI.
fn base64(bytes: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut s = String::new();
    for c in bytes.chunks(3) {
        let v = (u32::from(c[0]) << 16) | (u32::from(*c.get(1).unwrap_or(&0)) << 8) | u32::from(*c.get(2).unwrap_or(&0));
        for k in 0..4 {
            if k <= c.len() {
                s.push(char::from(A[(v >> (18 - 6 * k)) as usize & 63]));
            } else {
                s.push('=');
            }
        }
    }
    s
}
