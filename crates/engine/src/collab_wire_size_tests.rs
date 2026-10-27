//! What the co-editing mesh codec sends for a real design (#5184, the plan's "Network weight"):
//! the sensor enclosure's bodies whole, at the reference mesh's and the display plan's quality,
//! and after a local edit, as a delta against the shape before. A measurement, so it only prints:
//! `cargo test -p solidcraft-engine --lib collab_wire_size -- --ignored --nocapture`.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use serde_json::{Value, json};
use solidcraft_collab_wire::{FaceChunk, Hash, Held, Shape};
use solidcraft_doc::display_quality::DisplayQuality;
use solidcraft_doc::{ModelBody, naming};
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

/// The mesh at `quality`: the reference mesh, or the display plan's at Normal quality.
fn mesh(body: &ModelBody, quality: &str) -> Arc<Mesh> {
    if quality == "display" { body.plan_mesh_at(DisplayQuality::Normal).unwrap() } else { body.mesh() }
}

#[test]
#[ignore = "a measurement: prints the codec's byte counts for the sensor enclosure"]
fn collab_wire_sizes_of_the_sensor_enclosure() {
    let mut s = Session::default();
    let script: Value = serde_json::from_str(include_str!("../../../examples/acceptance/sensor_enclosure.json")).unwrap();
    s.run_script(&script).unwrap();
    let mut held: HashMap<(String, &str), Base> = HashMap::new();
    for body in &s.world_state().unwrap().bodies {
        for quality in ["reference", "display"] {
            let mesh = mesh(body, quality);
            let t = Instant::now();
            let sent = shape(body, &mesh);
            let bytes = sent.encode(|_| false);
            let encode = t.elapsed();
            let t = Instant::now();
            let got = Shape::decode(&bytes, &()).unwrap().to_mesh().unwrap();
            let decode = t.elapsed();
            assert_eq!(got.triangles.len(), mesh.triangles.len());
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
    for body in &s.world_state().unwrap().bodies {
        for quality in ["reference", "display"] {
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
