//! The co-editing mesh codec on real bodies (#5184, the plan's "Network weight"). Small bodies
//! (a box, the box filleted, a short modelled thread) check that their triangles come back and
//! that each edit, sent as a delta, rebuilds the shape after it. The measurements are ignored and
//! only print: the sensor enclosure's bodies whole, at the reference mesh's and the display plan's
//! quality, and after a local edit as a delta; and an imported 10k-triangle mesh as it comes, a
//! face per triangle, and without its normals:
//! `cargo test -p solidcraft-engine --lib collab_wire_size -- --ignored --nocapture`.

use std::collections::HashMap;
use std::f64::consts::TAU;
use std::sync::Arc;
use std::time::Instant;

use serde_json::{Value, json};
use solidcraft_collab_wire::{FaceChunk, Hash, Held, Shape};
use solidcraft_doc::display_quality::DisplayQuality;
use solidcraft_doc::{Document, FeatureKind, MeshData, ModelBody, naming};
use solidcraft_geom::Mesh;

use crate::Session;

fn shape(body: &ModelBody, mesh: &Mesh) -> Shape {
    Shape::from_mesh(mesh, &naming::face_names(body)).unwrap()
}

/// A receiver that holds one shape, the base of a delta.
struct Base(Arc<Shape>);

impl Held for Base {
    fn shape(&self, hash: &Hash) -> Option<Arc<Shape>> {
        (self.0.hash() == hash).then(|| self.0.clone())
    }

    fn face(&self, hash: &Hash) -> Option<Arc<FaceChunk>> {
        self.0.faces().iter().find(|c| c.hash() == hash).cloned()
    }
}

/// What the sender passes by audience: the reference mesh to collaborators, the display plan's at
/// Normal quality to Viewers.
const QUALITIES: [&str; 2] = ["reference", "display"];

/// The mesh at `quality`: the reference mesh, or the display plan's at Normal quality.
fn mesh(body: &ModelBody, quality: &str) -> Arc<Mesh> {
    if quality == "display" { body.plan_mesh_at(DisplayQuality::Normal).unwrap() } else { body.mesh() }
}

/// The triangles the wire keeps: those whose corners land on three different 1 µm grid points.
/// A sliver narrower than that has nothing to draw, and the codec leaves it out.
fn drawn(mesh: &Mesh) -> usize {
    let grid = |i: u32| mesh.positions.get(i as usize).map(|p| [p.x, p.y, p.z].map(|x| (x * 1e3).round() as i64));
    mesh.triangles.iter().filter(|[a, b, c]| grid(*a) != grid(*b) && grid(*b) != grid(*c) && grid(*a) != grid(*c)).count()
}

#[test]
fn small_bodies_come_back_and_each_edit_goes_as_a_delta() {
    let designs: [&[(&str, Value)]; 2] = [
        &[
            ("solid.box", json!({"length": 20, "width": 20, "height": 10, "body_name": "Part"})),
            ("solid.fillet", json!({"body": "Part", "radius": 2, "edges": [[0, 10, 10]]})),
        ],
        &[
            ("solid.cylinder", json!({"radius": 3, "height": 4, "body_name": "Part"})),
            ("solid.thread", json!({"face": [3, 0, 3.5], "designation": "M6x1", "length": 3, "modeled": true})),
        ],
    ];
    for steps in designs {
        let mut s = Session::default();
        let mut before: Vec<Base> = Vec::new();
        for (id, params) in steps {
            s.execute(id, params).unwrap_or_else(|e| panic!("{id}: {e}"));
            let state = s.world_state();
            let body = state.bodies.iter().find(|b| b.name == "Part").unwrap();
            let mut after = Vec::new();
            for (k, quality) in QUALITIES.into_iter().enumerate() {
                let mesh = mesh(body, quality);
                let sent = shape(body, &mesh);
                let whole = sent.encode(|_| false);
                let got = Shape::decode(&whole, &()).unwrap();
                assert_eq!(got.hash(), sent.hash(), "{id} ({quality})");
                assert_eq!(got.to_mesh().unwrap().triangles.len(), drawn(&mesh), "{id} ({quality}): triangles");
                let delta = before.get(k).map(|base| {
                    let delta = sent.encode_delta(&base.0, |_| false);
                    assert_eq!(Shape::decode(&delta, base).unwrap().hash(), sent.hash(), "{id} ({quality}): the delta");
                    delta.len()
                });
                println!(
                    "{id} ({quality}): {} triangles ({} slivers left out), {} faces; {} B whole, {delta:?} B as a delta",
                    mesh.triangles.len(),
                    mesh.triangles.len() - drawn(&mesh),
                    sent.faces().len(),
                    whole.len()
                );
                after.push(Base(Arc::new(sent)));
            }
            before = after;
        }
    }
}

#[test]
#[ignore = "a measurement: prints the codec's byte counts for the sensor enclosure"]
fn collab_wire_sizes_of_the_sensor_enclosure() {
    let mut s = Session::default();
    let script: Value = serde_json::from_str(include_str!("../../../examples/acceptance/sensor_enclosure.json")).unwrap();
    s.run_script(&script).unwrap();
    let mut held: HashMap<(String, &str), Base> = HashMap::new();
    for body in &s.world_state().bodies {
        let names = naming::face_names(body);
        let longest = names.iter().max_by_key(|n| n.len()).unwrap();
        println!("{}: the longest face name is {} bytes: {longest}", body.name, longest.len());
        for quality in QUALITIES {
            let mesh = mesh(body, quality);
            let t = Instant::now();
            let sent = shape(body, &mesh);
            let bytes = sent.encode(|_| false);
            let encode = t.elapsed();
            let t = Instant::now();
            let got = Shape::decode(&bytes, &()).unwrap().to_mesh().unwrap();
            let decode = t.elapsed();
            assert_eq!(got.triangles.len(), drawn(&mesh));
            let bare = Mesh { normals: Vec::new(), ..(*mesh).clone() };
            let positions = shape(body, &bare).encode(|_| false).len();
            println!(
                "{} ({quality}): {} triangles, {} vertices sent ({} in the mesh), {} faces; {} B whole = {positions} B positions and triangles + {} B normals; encode {encode:?}, decode {decode:?}",
                body.name,
                mesh.triangles.len(),
                got.positions.len(),
                mesh.positions.len(),
                sent.faces().len(),
                bytes.len(),
                bytes.len() - positions,
            );
            held.insert((body.name.clone(), quality), Base(Arc::new(sent)));
        }
    }
    // A local edit: the USB cut-out moves 2 mm. Only the faces that changed go.
    s.execute("parameters.change", &json!({"name": "usb_offset", "expression": "12 mm"})).unwrap();
    for body in &s.world_state().bodies {
        for quality in QUALITIES {
            let after = shape(body, &mesh(body, quality));
            let base = &held[&(body.name.clone(), quality)];
            let delta = after.encode_delta(&base.0, |_| false);
            let by_hash = after.encode(|h| base.face(h).is_some());
            let changed = after.faces().iter().filter(|c| base.face(c.hash()).is_none()).count();
            assert_eq!(Shape::decode(&delta, base).unwrap().hash(), after.hash());
            assert_eq!(Shape::decode(&by_hash, base).unwrap().hash(), after.hash());
            println!(
                "usb_offset 10 -> 12 mm, {} ({quality}): {changed} of {} faces changed; {} B as a delta, {} B with held faces by hash, {} B whole",
                body.name,
                after.faces().len(),
                delta.len(),
                by_hash.len(),
                after.encode(|_| false).len()
            );
        }
    }
}

#[test]
#[ignore = "a measurement: prints what an imported mesh costs as one face and a face per triangle"]
fn collab_wire_sizes_of_an_imported_mesh() {
    // A torus of 100 x 50 quads, welded as an STL import leaves it: 10,000 triangles.
    let (around, across) = (100u32, 50u32);
    let mut positions = Vec::new();
    for i in 0..around {
        let (si, ci) = (TAU * f64::from(i) / f64::from(around)).sin_cos();
        for j in 0..across {
            let (sj, cj) = (TAU * f64::from(j) / f64::from(across)).sin_cos();
            positions.extend([(40.0 + 10.0 * cj) * ci, (40.0 + 10.0 * cj) * si, 10.0 * sj]);
        }
    }
    let at = |i: u32, j: u32| (i % around) * across + j % across;
    let triangles: Vec<u32> = (0..around)
        .flat_map(|i| (0..across).flat_map(move |j| [at(i, j), at(i + 1, j), at(i + 1, j + 1), at(i, j), at(i + 1, j + 1), at(i, j + 1)]))
        .collect();
    let mut doc = Document::default();
    let meshes = vec![MeshData { name: "Scan".into(), positions: positions.into(), triangles: triangles.into(), color: None }];
    doc.add_feature(FeatureKind::MeshImport { file: "torus.stl".into(), meshes }, Some("Imported mesh")).unwrap();
    let s = Session::new(doc);
    let state = s.world_state();
    let body = state.bodies.iter().find(|b| b.name == "Scan").unwrap();
    let mesh = body.mesh();
    // A mesh body is one face already (its triangles all on face 0); a face per triangle, each
    // named, is what it would cost otherwise. Its normals are flat, one per triangle, so without
    // them the corners weld and the receiver works them out again.
    let as_it_comes = shape(body, &mesh);
    let n = mesh.triangles.len() as u32;
    let names: Vec<String> = (0..n).map(|k| format!("{}:face:{k}", body.name)).collect();
    let per_triangle = Shape::from_mesh(&Mesh { tri_face: (0..n).collect(), ..(*mesh).clone() }, &names).unwrap();
    let bare = shape(body, &Mesh { normals: Vec::new(), ..(*mesh).clone() });
    for (how, sent) in [("as it comes", &as_it_comes), ("a face per triangle", &per_triangle), ("one face without normals", &bare)] {
        let bytes = sent.encode(|_| false);
        let got = Shape::decode(&bytes, &()).unwrap().to_mesh().unwrap();
        assert_eq!(got.triangles.len(), mesh.triangles.len());
        println!(
            "imported mesh, {how}: {} triangles, {} vertices in the mesh, {} sent, {} faces; {} B whole",
            mesh.triangles.len(),
            mesh.positions.len(),
            got.positions.len(),
            sent.faces().len(),
            bytes.len()
        );
    }
}
