//! GLB export tests derived from glTF 2.0 and analytically known box geometry.

use super::*;
use solidcore_record::Vec3;
use solidcraft_color::srgb::decode8;
use solidcraft_doc::appearance::{FaceLook, Look};
use solidcraft_doc::pbr_appearance::PbrParameters;
use solidcraft_doc::{FeatureKind, Model, Operation, point_expr};
use solidcraft_geom::Vec3Geom;

fn model(body_count: usize) -> (Document, Model) {
    let mut doc = Document::new("GLB box");
    for i in 0..body_count {
        doc.add_feature(
            FeatureKind::Box {
                corner: point_expr(Vec3::new(i as f64 * 40.0, 0.0, 0.0)),
                length: "10".into(),
                width: "20".into(),
                height: "30".into(),
                operation: Operation::NewBody,
                centred: false,
            },
            None,
        )
        .unwrap();
    }
    let mut model = Model::new();
    model.evaluate(&doc);
    assert_eq!(model.state().bodies.len(), body_count);
    (doc, model)
}

fn export(doc: &Document, model: &Model) -> Vec<u8> {
    export_glb(doc, &model.state(), &model.state(), &[], "GLB box", MeshQuality::Standard).unwrap()
}

/// Validate references with gltf, retaining raw JSON for untyped extensions.
fn parse(bytes: &[u8]) -> (Value, Vec<u8>) {
    let parsed = gltf::Gltf::from_slice(bytes).unwrap();
    assert_eq!(parsed.document.meshes().count(), json(bytes)["meshes"].as_array().unwrap().len());
    (json(bytes), parsed.blob.unwrap())
}

fn json(bytes: &[u8]) -> Value {
    let container = gltf::binary::Glb::from_slice(bytes).unwrap();
    serde_json::from_slice(&container.json).unwrap()
}

fn word(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

/// Read the tightly packed accessors the exporter promises, without gltf's optional utils.
fn accessor_bytes<'a>(doc: &Value, bin: &'a [u8], accessor: &Value, width: usize) -> &'a [u8] {
    let view = &doc["bufferViews"][accessor["bufferView"].as_u64().unwrap() as usize];
    assert!(view.get("byteStride").is_none());
    let start = view["byteOffset"].as_u64().unwrap_or(0) as usize + accessor["byteOffset"].as_u64().unwrap_or(0) as usize;
    let size = accessor["count"].as_u64().unwrap() as usize * width;
    assert!(size <= view["byteLength"].as_u64().unwrap() as usize);
    &bin[start..start + size]
}

fn vectors(doc: &Value, bin: &[u8], index: &Value) -> Vec<Vec3> {
    let a = &doc["accessors"][index.as_u64().unwrap() as usize];
    assert_eq!(a["componentType"], FLOAT);
    assert_eq!(a["type"], "VEC3");
    accessor_bytes(doc, bin, a, 12)
        .as_chunks::<12>()
        .0
        .iter()
        .map(|p| Vec3::new(f32::from_bits(word(p, 0)) as f64, f32::from_bits(word(p, 4)) as f64, f32::from_bits(word(p, 8)) as f64))
        .collect()
}

fn indices(doc: &Value, bin: &[u8], primitive: &Value) -> Vec<usize> {
    let a = &doc["accessors"][primitive["indices"].as_u64().unwrap() as usize];
    assert_eq!(a["componentType"], UNSIGNED_INT);
    assert_eq!(a["type"], "SCALAR");
    accessor_bytes(doc, bin, a, 4).as_chunks::<4>().0.iter().map(|v| word(v, 0) as usize).collect()
}

fn bounds(points: impl IntoIterator<Item = Vec3>) -> ([f64; 3], [f64; 3]) {
    let (mut min, mut max) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
    for p in points {
        for (i, x) in [p.x, p.y, p.z].into_iter().enumerate() {
            min[i] = min[i].min(x);
            max[i] = max[i].max(x);
        }
    }
    (min, max)
}

fn near(actual: f64, expected: f64) {
    assert!((actual - expected).abs() < 1e-7, "{actual} != {expected}");
}

fn face(doc: &mut Document, body: &str, look: Look) {
    doc.appearances.faces.push(FaceLook {
        body: body.into(),
        point: Vec3::new(5.0, 10.0, 30.0),
        look,
        name: None,
        strict_name: false,
        certificate: None,
    });
}

fn material() -> GlbMaterial {
    GlbMaterial { name: "test".into(), color: [128; 3], metallic: 0.0, roughness: 0.5, ior: 1.5 }
}

fn triangle() -> GlbBody {
    GlbBody {
        name: "triangle".into(),
        mesh: Mesh {
            positions: vec![Vec3::ZERO, Vec3::new(1.0, 0.0, 0.0), Vec3::new(0.0, 1.0, 0.0)],
            normals: vec![Vec3::new(0.0, 0.0, 2.0); 3],
            triangles: vec![[0, 1, 2]],
            tri_face: vec![7],
            ..Default::default()
        },
        material: 0,
        faces: BTreeMap::new(),
        parent: None,
    }
}

fn direct(body: GlbBody) -> Result<Vec<u8>> {
    glb("hostile", &[], &[body], &[material()])
}

fn assert_invalid(result: Result<Vec<u8>>, words: &[&str]) {
    let Err(IoError::Invalid(message)) = result else { panic!("expected IoError::Invalid, got {result:?}") };
    for word in words {
        assert!(message.contains(word), "{message:?} does not contain {word:?}");
    }
}

/// GLB headers and chunks describe exactly the emitted bytes.
#[test]
fn container_has_version_two_and_aligned_json_then_bin() {
    let (doc, model) = model(1);
    let bytes = export(&doc, &model);
    parse(&bytes);
    assert_eq!(&bytes[..4], b"glTF");
    assert_eq!(word(&bytes, 4), 2);
    assert_eq!(word(&bytes, 8) as usize, bytes.len());
    let json_len = word(&bytes, 12) as usize;
    assert_eq!(json_len % 4, 0);
    assert_eq!(&bytes[16..20], b"JSON");
    let bin_header = 20 + json_len;
    let bin_len = word(&bytes, bin_header) as usize;
    assert_eq!(bin_len % 4, 0);
    assert_eq!(&bytes[bin_header + 4..bin_header + 8], b"BIN\0");
    assert_eq!(bin_header + 8 + bin_len, bytes.len());
}

/// Every possible padding length uses JSON spaces and binary zeros.
#[test]
fn container_pads_json_with_spaces_and_bin_with_zeros() {
    for padding in 0..4 {
        let mut raw_json = br#"{"asset":{"version":"2.0"}}"#.to_vec();
        raw_json.extend(std::iter::repeat_n(b' ', padding));
        let raw_bin = vec![0xab; padding + 1];
        let bytes = container(&raw_json, &raw_bin).unwrap();
        let jlen = word(&bytes, 12) as usize;
        assert_eq!(&bytes[20..20 + raw_json.len()], raw_json);
        assert!(bytes[20 + raw_json.len()..20 + jlen].iter().all(|b| *b == b' '));
        let start = 20 + jlen + 8;
        assert_eq!(&bytes[start..start + raw_bin.len()], raw_bin);
        assert!(bytes[start + raw_bin.len()..].iter().all(|b| *b == 0));
        gltf::binary::Glb::from_slice(&bytes).unwrap();
    }
}

/// Repeated exports of one evaluated design are byte-for-byte deterministic.
#[test]
fn exporting_twice_produces_identical_bytes() {
    let (doc, model) = model(1);
    assert_eq!(export(&doc, &model), export(&doc, &model));
}

/// Box accessor extrema match the actual f32 positions in millimetres.
#[test]
fn box_position_bounds_are_exact_millimetres() {
    let (doc, model) = model(1);
    let (doc, bin) = parse(&export(&doc, &model));
    for primitive in doc["meshes"][0]["primitives"].as_array().unwrap() {
        let index = &primitive["attributes"]["POSITION"];
        let a = &doc["accessors"][index.as_u64().unwrap() as usize];
        assert_eq!(a["min"], json!([0.0, 0.0, 0.0]));
        assert_eq!(a["max"], json!([10.0, 20.0, 30.0]));
        assert_eq!(bounds(vectors(&doc, &bin, index)), ([0.0; 3], [10.0, 20.0, 30.0]));
    }
}

/// All triangle corners reference a position in the emitted accessor.
#[test]
fn box_indices_are_within_position_count() {
    let (doc, model) = model(1);
    let (doc, bin) = parse(&export(&doc, &model));
    for p in doc["meshes"][0]["primitives"].as_array().unwrap() {
        let count = vectors(&doc, &bin, &p["attributes"]["POSITION"]).len();
        let indices = indices(&doc, &bin, p);
        assert!(!indices.is_empty());
        assert_eq!(indices.len() % 3, 0);
        assert!(indices.into_iter().all(|i| i < count));
    }
}

/// Every exported box vertex has a unit-length normal.
#[test]
fn box_normals_are_present_and_unit_length() {
    let (doc, model) = model(1);
    let (doc, bin) = parse(&export(&doc, &model));
    for p in doc["meshes"][0]["primitives"].as_array().unwrap() {
        let normals = vectors(&doc, &bin, &p["attributes"]["NORMAL"]);
        assert_eq!(normals.len(), vectors(&doc, &bin, &p["attributes"]["POSITION"]).len());
        assert!(normals.iter().all(|n| (n.dot(*n).sqrt() - 1.0).abs() < 1e-5));
    }
}

/// A sole root owns the unit/axis conversion and named body mesh children.
#[test]
fn scene_has_one_conversion_root_and_named_body_children() {
    let (doc, model) = model(2);
    let (doc, _) = parse(&export(&doc, &model));
    assert_eq!(doc["scene"], 0);
    assert_eq!(doc["scenes"][0]["nodes"], json!([0]));
    let root = &doc["nodes"][0];
    assert!(root.get("mesh").is_none());
    assert_eq!(root["scale"], json!([0.001, 0.001, 0.001]));
    let half = std::f64::consts::FRAC_1_SQRT_2;
    assert_eq!(root["rotation"], json!([-half, 0.0, 0.0, half]));
    let children = root["children"].as_array().unwrap();
    assert_eq!(children.len(), 2);
    assert_eq!(doc["nodes"].as_array().unwrap().len(), 3);
    for (child, body) in children.iter().zip(&model.state().bodies) {
        let node = &doc["nodes"][child.as_u64().unwrap() as usize];
        assert_eq!(node["name"], body.name);
        let mesh = &doc["meshes"][node["mesh"].as_u64().unwrap() as usize];
        assert_eq!(mesh["name"], body.name);
        assert!(node.get("scale").is_none() && node.get("rotation").is_none() && node.get("translation").is_none());
    }
}

/// Applying the emitted transform turns the box into metres with Y up.
#[test]
fn root_transform_maps_box_corners_to_metre_scene_bounds() {
    let (doc, model) = model(1);
    let bytes = export(&doc, &model);
    let gltf = gltf::Gltf::from_slice(&bytes).unwrap();
    let matrix = gltf.document.nodes().next().unwrap().transform().matrix();
    let corners = (0..8).map(|i| {
        let p = [if i & 1 == 0 { 0.0 } else { 10.0 }, if i & 2 == 0 { 0.0 } else { 20.0 }, if i & 4 == 0 { 0.0 } else { 30.0 }];
        let q: [f64; 3] = std::array::from_fn(|row| (0..3).map(|col| matrix[col][row] as f64 * p[col]).sum::<f64>() + matrix[3][row] as f64);
        Vec3::new(q[0], q[1], q[2])
    });
    let (min, max) = bounds(corners);
    for i in 0..3 {
        near(min[i], [0.0, 0.0, -0.02][i]);
        near(max[i], [0.01, 0.03, 0.0][i]);
    }
}

/// Every triangle is counter-clockwise when seen along its outward normals.
#[test]
fn box_triangle_winding_agrees_with_vertex_normals() {
    let (doc, model) = model(1);
    let (doc, bin) = parse(&export(&doc, &model));
    for p in doc["meshes"][0]["primitives"].as_array().unwrap() {
        let positions = vectors(&doc, &bin, &p["attributes"]["POSITION"]);
        let normals = vectors(&doc, &bin, &p["attributes"]["NORMAL"]);
        for t in indices(&doc, &bin, p).as_chunks::<3>().0 {
            let [a, b, c] = [positions[t[0]], positions[t[1]], positions[t[2]]];
            let average = (normals[t[0]] + normals[t[1]] + normals[t[2]]) / 3.0;
            assert!((b - a).cross(c - a).dot(average) > 0.0);
            let centroid = (a + b + c) / 3.0;
            assert!(average.dot(centroid - Vec3::new(5.0, 10.0, 15.0)) > 0.0);
        }
    }
}

/// An unpainted body uses the linear workspace colour and plain PBR defaults.
#[test]
fn unassigned_body_uses_linear_default_color_and_plain_pbr() {
    let (doc, model) = model(1);
    let doc = json(&export(&doc, &model));
    let pbr = &doc["materials"][0]["pbrMetallicRoughness"];
    let [r, g, b] = DEFAULT_COLOR.map(decode8);
    assert_eq!(pbr["baseColorFactor"], json!([r, g, b, 1.0]));
    assert_eq!(pbr["metallicFactor"], 0.0);
    assert_eq!(pbr["roughnessFactor"], 0.5);
}

/// A saturated red body look remains opaque red in linear glTF factors.
#[test]
fn custom_red_body_look_sets_base_color() {
    let (mut doc, model) = model(1);
    doc.appearances.bodies.insert(model.state().bodies[0].name.clone(), Look::custom([255, 0, 0], 1.0));
    let (doc, _) = parse(&export(&doc, &model));
    assert_eq!(doc["materials"][0]["pbrMetallicRoughness"]["baseColorFactor"], json!([1.0, 0.0, 0.0, 1.0]));
}

/// Mid-grey sRGB codes are decoded rather than copied as linear fractions.
#[test]
fn custom_mid_grey_is_decoded_to_linear_color() {
    let (mut doc, model) = model(1);
    doc.appearances.bodies.insert(model.state().bodies[0].name.clone(), Look::custom([128; 3], 1.0));
    let (doc, _) = parse(&export(&doc, &model));
    let factor = &doc["materials"][0]["pbrMetallicRoughness"]["baseColorFactor"];
    assert_eq!(*factor, json!([decode8(128), decode8(128), decode8(128), 1.0]));
    near(factor[0].as_f64().unwrap(), 0.2158605);
}

fn pbr_export(ior: f64) -> Value {
    let (mut doc, model) = model(1);
    let mut look = Look::custom([128; 3], 1.0);
    look.pbr = Some(PbrParameters::new(1.0, 0.2, ior).unwrap());
    doc.appearances.bodies.insert(model.state().bodies[0].name.clone(), look);
    parse(&export(&doc, &model)).0
}

/// The default IOR needs no extension while authored metallic and roughness survive.
#[test]
fn pbr_default_ior_omits_extensions() {
    let doc = pbr_export(1.5);
    let m = &doc["materials"][0];
    assert_eq!(m["pbrMetallicRoughness"]["metallicFactor"], 1.0);
    assert_eq!(m["pbrMetallicRoughness"]["roughnessFactor"], 0.2);
    assert!(m.get("extensions").is_none());
    assert!(doc.get("extensionsUsed").is_none());
}

/// A nondefault IOR declares and populates KHR_materials_ior.
#[test]
fn pbr_nondefault_ior_writes_declared_extension() {
    let doc = pbr_export(2.0);
    assert_eq!(doc["materials"][0]["extensions"]["KHR_materials_ior"]["ior"], 2.0);
    assert!(doc["extensionsUsed"].as_array().unwrap().contains(&json!("KHR_materials_ior")));
}

/// Only triangles on the authored top face receive its override material.
#[test]
fn face_look_overrides_body_only_on_top_face() {
    let (mut doc, model) = model(1);
    let name = model.state().bodies[0].name.clone();
    doc.appearances.bodies.insert(name.clone(), Look::custom([255, 0, 0], 1.0));
    face(&mut doc, &name, Look::custom([0, 255, 0], 1.0));
    let (doc, bin) = parse(&export(&doc, &model));
    assert_eq!(doc["materials"].as_array().unwrap().len(), 2);
    let primitives = doc["meshes"][0]["primitives"].as_array().unwrap();
    assert_eq!(primitives.len(), 2);
    let mut counts = [0; 2];
    for p in primitives {
        let material = p["material"].as_u64().unwrap() as usize;
        let factor = &doc["materials"][material]["pbrMetallicRoughness"]["baseColorFactor"];
        let is_face = *factor == json!([0.0, 1.0, 0.0, 1.0]);
        if !is_face {
            assert_eq!(*factor, json!([1.0, 0.0, 0.0, 1.0]));
        }
        let positions = vectors(&doc, &bin, &p["attributes"]["POSITION"]);
        for t in indices(&doc, &bin, p).as_chunks::<3>().0 {
            let on_top = t.iter().all(|i| positions[*i].z == 30.0);
            assert_eq!(on_top, is_face);
            counts[usize::from(is_face)] += 1;
        }
    }
    assert!(counts.iter().all(|count| *count > 0));
}

/// Identical authored body and face looks share one glTF material.
#[test]
fn identical_body_and_face_looks_are_deduplicated() {
    let (mut doc, model) = model(2);
    let look = Look::custom([12, 34, 56], 1.0);
    for b in &model.state().bodies {
        doc.appearances.bodies.insert(b.name.clone(), look.clone());
    }
    face(&mut doc, &model.state().bodies[0].name, look);
    let (doc, _) = parse(&export(&doc, &model));
    assert_eq!(doc["materials"].as_array().unwrap().len(), 1);
    for mesh in doc["meshes"].as_array().unwrap() {
        assert_eq!(mesh["primitives"].as_array().unwrap().len(), 1);
        assert_eq!(mesh["primitives"][0]["material"], 0);
    }
}

/// Transparent body refusal identifies the body and unsupported transparency.
#[test]
fn transparent_body_is_refused_with_its_name() {
    let (mut doc, model) = model(1);
    let name = model.state().bodies[0].name.clone();
    doc.appearances.bodies.insert(name.clone(), Look::custom([255, 0, 0], 0.5));
    assert_invalid(export_glb(&doc, &model.state(), &model.state(), &[], "test", MeshQuality::Standard), &[&name, "transparent"]);
}

/// Transparent authored faces cannot silently become opaque.
#[test]
fn transparent_face_is_refused_with_face_context() {
    let (mut doc, model) = model(1);
    let name = model.state().bodies[0].name.clone();
    face(&mut doc, &name, Look::custom([0, 255, 0], 0.5));
    assert_invalid(export_glb(&doc, &model.state(), &model.state(), &[], "test", MeshQuality::Standard), &[&name, "face", "transparent"]);
}

/// A filter matching no body reports an empty export.
#[test]
fn nonexistent_body_filter_returns_empty() {
    let (doc, model) = model(1);
    assert!(matches!(export_glb(&doc, &model.state(), &model.state(), &["nope".into()], "test", MeshQuality::Standard), Err(IoError::Empty)));
}

/// A body filter selects exactly its named mesh from a two-body design.
#[test]
fn named_body_filter_exports_only_selected_mesh() {
    let (doc, model) = model(2);
    let name = model.state().bodies[1].name.clone();
    let bytes = export_glb(&doc, &model.state(), &model.state(), std::slice::from_ref(&name), "test", MeshQuality::Standard).unwrap();
    let (doc, _) = parse(&bytes);
    assert_eq!(doc["meshes"].as_array().unwrap().len(), 1);
    assert_eq!(doc["meshes"][0]["name"], name);
}

/// Out-of-range triangle indices return an error rather than indexing positions.
#[test]
fn hostile_triangle_index_is_refused() {
    let mut body = triangle();
    body.mesh.triangles[0][2] = 3;
    assert_invalid(direct(body), &["triangle"]);
}

/// NaN coordinates cannot enter a GLB position buffer.
#[test]
fn hostile_nan_position_is_refused() {
    let mut body = triangle();
    body.mesh.positions[0].x = f64::NAN;
    assert_invalid(direct(body), &["finite"]);
}

/// A finite f64 that overflows f32 is rejected before serialization.
#[test]
fn hostile_position_overflowing_f32_is_refused() {
    let mut body = triangle();
    body.mesh.positions[0].x = 1e39;
    assert_invalid(direct(body), &["finite"]);
}

/// Missing body materials are refused even when the mesh is otherwise valid.
#[test]
fn hostile_body_material_index_is_refused() {
    let mut body = triangle();
    body.material = 1;
    assert_invalid(direct(body), &["material"]);
}

/// A face override cannot reference a nonexistent material.
#[test]
fn hostile_face_material_index_is_refused() {
    let mut body = triangle();
    body.faces.insert(7, 1);
    assert_invalid(direct(body), &["material"]);
}

/// Mismatched normal counts leave the entire NORMAL attribute unwritten.
#[test]
fn mismatched_normal_count_omits_normal_attribute() {
    let mut body = triangle();
    body.mesh.normals.pop();
    let (doc, _) = parse(&direct(body).unwrap());
    assert!(doc["meshes"][0]["primitives"][0]["attributes"].get("NORMAL").is_none());
}

/// A single zero direction leaves the entire NORMAL attribute unwritten.
#[test]
fn zero_normal_omits_normal_attribute() {
    let mut body = triangle();
    body.mesh.normals[1] = Vec3::ZERO;
    let (doc, _) = parse(&direct(body).unwrap());
    assert!(doc["meshes"][0]["primitives"][0]["attributes"].get("NORMAL").is_none());
}

/// Repeated-index triangles have no drawable area and yield an empty export.
#[test]
fn all_repeated_index_triangles_return_empty() {
    let mut body = triangle();
    body.mesh.triangles = vec![[0, 0, 1], [0, 1, 1], [0, 1, 0]];
    assert!(matches!(direct(body), Err(IoError::Empty)));
}

/// An absent body list cannot create an empty glTF scene.
#[test]
fn no_bodies_returns_empty() {
    assert!(matches!(glb("empty", &[], &[], &[material()]), Err(IoError::Empty)));
}

/// Invalid IOR values omit both the extension payload and its declaration.
#[test]
fn hostile_ior_is_not_written_as_extension() {
    for ior in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 0.9, -1.0] {
        let mut material = material();
        material.ior = ior;
        let (doc, _) = parse(&glb("hostile", &[], &[triangle()], &[material]).unwrap());
        assert!(doc["materials"][0].get("extensions").is_none(), "ior={ior}");
        assert!(doc.get("extensionsUsed").is_none(), "ior={ior}");
    }
}

/// NaN metallic and roughness values are sanitized to finite zero factors.
#[test]
fn hostile_nan_metallic_and_roughness_are_written_as_zero() {
    let mut material = material();
    material.metallic = f64::NAN;
    material.roughness = f64::NAN;
    let (doc, _) = parse(&glb("hostile", &[], &[triangle()], &[material]).unwrap());
    let pbr = &doc["materials"][0]["pbrMetallicRoughness"];
    assert_eq!(pbr["metallicFactor"], 0.0);
    assert_eq!(pbr["roughnessFactor"], 0.0);
}

/// Import restores millimetres and keeps the export's Y-up geometry.
#[test]
fn round_trip_preserves_box_bounds_in_importer_frame() {
    let (doc, model) = model(1);
    let bytes = export(&doc, &model);
    let mut imported = crate::read_gltf(&bytes, "box.glb", &mut |uri| panic!("unexpected external buffer: {uri}")).unwrap();
    imported.flatten();
    let FeatureKind::MeshImport { meshes, .. } = imported.feature.kind else { panic!("expected imported mesh geometry") };
    let points = meshes.iter().flat_map(|m| m.positions.as_chunks::<3>().0.iter().map(|p| Vec3::new(p[0], p[1], p[2])));
    let (min, max) = bounds(points);
    for i in 0..3 {
        // The gltf API reads the 0.001 scale as f32 before import converts to mm.
        assert!((min[i] - [0.0, 0.0, -20.0][i]).abs() < 1e-5, "min={min:?}");
        assert!((max[i] - [10.0, 30.0, 0.0][i]).abs() < 1e-5, "max={max:?}");
    }
}

/// The size budget is refused once the bodies kept so far pass it, before more are meshed.
#[test]
fn a_body_past_the_size_budget_is_refused_naming_it() {
    let body = triangle();
    assert_eq!(add_to_budget(0, &body).unwrap(), 3 * 24 + 12);
    assert_invalid(add_to_budget(MAX_GLB_BYTES - 10, &body).map(|_| Vec::new()), &["triangle", "too large"]);
    assert_invalid(add_to_budget(usize::MAX, &body).map(|_| Vec::new()), &["too large"]);
}

/// A galvanometer-like assembly: a frame part holding a needle part placed twice, the first needle
/// turned a quarter about Z and moved 100 mm along X, the second moved 50 mm along Y. Returns the
/// design, its model and the model placed in the world.
fn assembly() -> (Document, Model, ModelState) {
    let mut doc = Document::new("Galvanometer");
    let frame = doc.add_component(Some("Frame"), 0).unwrap();
    let needle = doc.add_component(Some("Needle"), frame).unwrap();
    for (component, x) in [(frame, 0.0), (needle, 5.0)] {
        let id = doc
            .add_feature(
                FeatureKind::Box {
                    corner: point_expr(Vec3::new(x, 0.0, 0.0)),
                    length: "10".into(),
                    width: "20".into(),
                    height: "30".into(),
                    operation: Operation::NewBody,
                    centred: false,
                },
                None,
            )
            .unwrap();
        doc.feature_mut(id).unwrap().component = component;
    }
    let turned = solidcraft_doc::rigid(Vec3::new(100.0, 0.0, 0.0), Vec3::ZERO, Vec3::Z, std::f64::consts::FRAC_PI_2);
    doc.occurrences.iter_mut().find(|o| o.component == needle).unwrap().transform = turned;
    doc.add_occurrence(needle, frame, solidcraft_doc::rigid(Vec3::new(0.0, 50.0, 0.0), Vec3::ZERO, Vec3::Z, 0.0)).unwrap();
    let mut model = Model::new();
    model.evaluate(&doc);
    let world = solidcraft_doc::world_state(&doc, &model.state()).unwrap();
    assert_eq!(world.bodies.len(), 3);
    (doc, model, world)
}

fn export_assembly(bodies: &[String]) -> (Document, Model, ModelState, Vec<u8>) {
    let (doc, model, world) = assembly();
    let bytes = export_glb(&doc, &model.state(), &world, bodies, "Galvanometer", MeshQuality::Standard).unwrap();
    (doc, model, world, bytes)
}

/// The node named `name` and its index (the first one when several share it).
fn node_named<'a>(doc: &'a Value, name: &str) -> (usize, &'a Value) {
    doc["nodes"].as_array().unwrap().iter().enumerate().find(|(_, n)| n["name"] == name).unwrap_or_else(|| panic!("no node named {name}"))
}

fn children_of(node: &Value) -> Vec<usize> {
    node.get("children").and_then(Value::as_array).map(|c| c.iter().map(|i| i.as_u64().unwrap() as usize).collect()).unwrap_or_default()
}

/// Each occurrence is a node named after it, in the node of the occurrence it sits in, placed as
/// the occurrence is; each body sits in its occurrence's node under the name its part gives it.
#[test]
fn an_assembly_writes_each_occurrence_as_a_named_node_holding_its_bodies() {
    let (_, model, _, bytes) = export_assembly(&[]);
    let (doc, _) = parse(&bytes);
    let (frame_body, needle_body) = (&model.state().bodies[0].name, &model.state().bodies[1].name);
    let (frame, frame_node) = node_named(&doc, "Frame:1");
    assert_eq!(children_of(&doc["nodes"][0]), vec![frame]);
    let (first, first_node) = node_named(&doc, "Needle:1");
    let (second, second_node) = node_named(&doc, "Needle:2");
    let mut kids = children_of(frame_node);
    kids.sort_unstable();
    let bodies: Vec<&Value> = kids.iter().map(|k| &doc["nodes"][*k]["name"]).filter(|n| n.as_str() == Some(frame_body.as_str())).collect();
    assert_eq!(bodies.len(), 1, "the frame's own body sits in the frame's node");
    assert!(kids.contains(&first) && kids.contains(&second));
    let half = std::f64::consts::FRAC_1_SQRT_2;
    let t = first_node["translation"].as_array().unwrap();
    let q = first_node["rotation"].as_array().unwrap();
    for (got, want) in t.iter().zip([100.0, 0.0, 0.0]).chain(q.iter().zip([0.0, 0.0, half, half])) {
        near(got.as_f64().unwrap(), want);
    }
    near(second_node["translation"][1].as_f64().unwrap(), 50.0);
    for needle in [first_node, second_node] {
        let kids = children_of(needle);
        assert_eq!(kids.len(), 1);
        assert_eq!(doc["nodes"][kids[0]]["name"], needle_body.as_str());
    }
}

/// A body under an occurrence is written in its part's frame: both needles' meshes are the needle
/// as its part has it, wherever each is placed.
#[test]
fn an_occurrence_s_body_mesh_is_in_its_part_s_frame() {
    let (_, _, _, bytes) = export_assembly(&[]);
    let (doc, bin) = parse(&bytes);
    for name in ["Needle:1", "Needle:2"] {
        let body = &doc["nodes"][children_of(node_named(&doc, name).1)[0]];
        let mesh = &doc["meshes"][body["mesh"].as_u64().unwrap() as usize];
        let (min, max) = bounds(vectors(&doc, &bin, &mesh["primitives"][0]["attributes"]["POSITION"]));
        for i in 0..3 {
            assert!((min[i] - [5.0, 0.0, 0.0][i]).abs() < 1e-4, "{name} min={min:?}");
            assert!((max[i] - [15.0, 20.0, 30.0][i]).abs() < 1e-4, "{name} max={max:?}");
        }
    }
}

/// Placed through their nodes, the bodies land where the world has them.
#[test]
fn an_assembly_s_bodies_land_where_the_world_has_them() {
    let (_, _, world, bytes) = export_assembly(&[]);
    let gltf = gltf::Gltf::from_slice(&bytes).unwrap();
    let (doc, bin) = parse(&bytes);
    let mul = |a: [[f32; 4]; 4], b: [[f32; 4]; 4]| -> [[f32; 4]; 4] {
        std::array::from_fn(|c| std::array::from_fn(|r| (0..4).map(|k| a[k][r] * b[c][k]).sum()))
    };
    // Each mesh node's transform below the root, walking down from it.
    let identity: [[f32; 4]; 4] = std::array::from_fn(|c| std::array::from_fn(|r| if c == r { 1.0 } else { 0.0 }));
    let mut stack: Vec<(usize, [[f32; 4]; 4])> = children_of(&doc["nodes"][0]).into_iter().map(|c| (c, identity)).collect();
    let mut placed = Vec::new();
    while let Some((i, above)) = stack.pop() {
        let node = gltf.document.nodes().nth(i).unwrap();
        let m = mul(above, node.transform().matrix());
        stack.extend(children_of(&doc["nodes"][i]).into_iter().map(|c| (c, m)));
        if let Some(mesh) = doc["nodes"][i].get("mesh") {
            let points = vectors(&doc, &bin, &doc["meshes"][mesh.as_u64().unwrap() as usize]["primitives"][0]["attributes"]["POSITION"]);
            let world = points.into_iter().map(|p| {
                let q: [f64; 3] = std::array::from_fn(|r| (0..3).map(|c| m[c][r] as f64 * [p.x, p.y, p.z][c]).sum::<f64>() + m[3][r] as f64);
                Vec3::new(q[0], q[1], q[2])
            });
            placed.push(bounds(world));
        }
    }
    let mut expected: Vec<_> = world.bodies.iter().map(|b| bounds(export_mesh_at(&b.body, MeshQuality::Standard).unwrap().positions)).collect();
    let key = |b: &([f64; 3], [f64; 3])| (b.0.map(|x| (x * 1000.0).round() as i64), b.1.map(|x| (x * 1000.0).round() as i64));
    placed.sort_by_key(key);
    expected.sort_by_key(key);
    assert_eq!(placed.len(), expected.len());
    for (got, want) in placed.iter().zip(&expected) {
        for i in 0..3 {
            assert!((got.0[i] - want.0[i]).abs() < 1e-3 && (got.1[i] - want.1[i]).abs() < 1e-3, "{got:?} != {want:?}");
        }
    }
}

/// Choosing one placed body writes only the occurrences it is placed through.
#[test]
fn a_chosen_body_brings_only_its_own_occurrences() {
    let (doc, model, world) = assembly();
    let second = doc.occurrences.iter().find(|o| o.name == "Needle:2").unwrap().id;
    let name = doc.body_frames(&model.state()).into_iter().find(|(_, _, path, _)| path.last() == Some(&second)).unwrap().0;
    assert!(world.bodies.iter().any(|b| b.name == name), "{name} is not a world body");
    let bytes = export_glb(&doc, &model.state(), &world, std::slice::from_ref(&name), "one", MeshQuality::Standard).unwrap();
    let (gltf, _) = parse(&bytes);
    let names: Vec<&str> = gltf["nodes"].as_array().unwrap().iter().filter_map(|n| n["name"].as_str()).collect();
    assert!(names.contains(&"Frame:1") && names.contains(&"Needle:2"), "{names:?}");
    assert!(!names.contains(&"Needle:1"), "{names:?}");
    assert_eq!(gltf["meshes"].as_array().unwrap().len(), 1);
}

/// The glTF importer reads the occurrence nodes back as components.
#[test]
fn an_exported_assembly_imports_as_components_again() {
    let (_, _, _, bytes) = export_assembly(&[]);
    let imported = crate::read_gltf(&bytes, "galvanometer.glb", &mut |uri| panic!("unexpected external buffer: {uri}")).unwrap();
    let plan = &imported.plan;
    let slot = |name: &str| plan.slots.iter().position(|s| s == name).unwrap_or_else(|| panic!("no component {name}: {:?}", plan.slots));
    let parent = |name: &str| plan.places.iter().find(|p| p.slot == slot(name)).unwrap().parent;
    assert_eq!(parent("Frame:1"), None);
    assert_eq!(parent("Needle:1"), Some(slot("Frame:1")));
    assert_eq!(parent("Needle:2"), Some(slot("Frame:1")));
    assert_eq!(plan.bodies.len(), 3);
}
