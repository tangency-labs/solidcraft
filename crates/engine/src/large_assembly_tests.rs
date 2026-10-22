//! A generated many-part assembly, shaped like a large design built through the MCP tools: each
//! part is a few primitives, and every body is then moved into its own
//! placed component. Opening one runs a long timeline over many bodies, which is where costs
//! that grow with the number of bodies after every feature show up (#255, #662).
//!
//! The test keeps it small. The benchmark times the open phases at any size:
//! `SOLIDCRAFT_BENCH_PARTS=400 cargo test --release -p solidcraft-engine bench_generated_assembly -- --ignored --nocapture`

use serde_json::json;
use solidcraft_doc::BodyName;
use solidcraft_doc::{Document, FeatureKind, Operation, mat_inverse, point_expr, rigid};
use solidcraft_geom::Vec3;

use crate::Session;

/// `parts` parts, four bodies each, in `parts / 4` (at least one) placed components.
fn generated_assembly(parts: usize) -> Document {
    let mut doc = Document::new("generated assembly");
    let groups = parts.div_ceil(4).max(1);
    let comps: Vec<(u64, solidcraft_doc::Mat)> = (0..groups)
        .map(|g| {
            let id = doc.add_component(Some(&format!("Module {g}")), 0).unwrap();
            let m = rigid(Vec3::new(5.0 * g as f64, -3.0, 2.0), Vec3::ZERO, Vec3::Z, 0.1 * g as f64);
            doc.occurrences.iter_mut().find(|o| o.component == id).unwrap().transform = m;
            (id, m)
        })
        .collect();
    let mm = |v: f64| solidcraft_doc::Expr::new(format!("{v} mm"));
    let mut made = Vec::new();
    for i in 0..parts {
        let o = Vec3::new(30.0 * (i % 20) as f64, 30.0 * (i / 20) as f64, 0.0);
        let block = FeatureKind::Box { corner: point_expr(o), length: mm(20.0), width: mm(12.0), height: mm(6.0), operation: Operation::NewBody };
        made.push((i, doc.add_feature(block, Some(&format!("Plate {i}"))).unwrap()));
        for (k, x) in [2.0, 9.0, 16.0].into_iter().enumerate() {
            let pin = FeatureKind::Cylinder {
                base: point_expr(o + Vec3::new(x, 20.0, 0.0)),
                axis: solidcraft_doc::Dir(Vec3::Z),
                radius: mm(1.5),
                height: mm(4.0 + k as f64),
                operation: Operation::NewBody,
            };
            made.push((i, doc.add_feature(pin, Some(&format!("Pin {i}.{k}"))).unwrap()));
        }
    }
    // Every body moves into its part's component, keeping its place in the world.
    let s = Session::new(doc.clone());
    let state = s.model.state();
    for (part, feature) in made {
        let (to, m) = comps[part / 4 % groups];
        for b in state.bodies.iter().filter(|b| b.feature == feature) {
            let kind = FeatureKind::BodyTransfer { bodies: vec![BodyName::from(&b.name)], from: 0, to, transform: mat_inverse(&m).unwrap() };
            doc.add_feature(kind, None).unwrap();
        }
    }
    doc
}

fn measured(s: &mut Session) -> serde_json::Value {
    let mut m = s.execute("inspect.measure", &json!({})).unwrap();
    for b in m["bodies"].as_array_mut().unwrap() {
        b.as_object_mut().unwrap().remove("name");
    }
    m
}

#[test]
fn generated_assembly_opens_with_every_body_in_place() {
    let parts = 12;
    let doc = generated_assembly(parts);
    let mut s = Session::new(doc.clone());
    assert!(s.model.results.iter().all(|r| r.error.is_none()), "{:?}", s.model.results.iter().find(|r| r.error.is_some()));
    let world = s.world_state().unwrap();
    assert_eq!(world.bodies.len(), parts * 4);
    assert!(world.bodies.iter().all(|b| b.component != 0 && b.transferred));
    world.warm_meshes(|| true);
    assert!(world.bodies.iter().all(|b| !b.mesh().triangles.is_empty()));
    // Moving bodies into components keeps them where they were.
    let before = measured(&mut Session::new({
        let mut d = doc.clone();
        d.features.retain(|f| !matches!(f.kind, FeatureKind::BodyTransfer { .. }));
        d
    }));
    let after = measured(&mut s);
    for (a, b) in before["bodies"].as_array().unwrap().iter().zip(after["bodies"].as_array().unwrap()) {
        for k in ["volume_mm3", "area_mm2"] {
            assert!((a[k].as_f64().unwrap() - b[k].as_f64().unwrap()).abs() < 1e-6, "{k}: {a} vs {b}");
        }
        for (p, q) in a["bbox"]["min"].as_array().unwrap().iter().zip(b["bbox"]["min"].as_array().unwrap()) {
            assert!((p.as_f64().unwrap() - q.as_f64().unwrap()).abs() < 1e-6);
        }
    }
    // A saved copy opens to the same model.
    let reopened = solidcraft_io::read_design(&solidcraft_io::write_design(&doc)).unwrap();
    assert_eq!(measured(&mut Session::new(reopened)), after);
}

/// Open-phase timings of a generated assembly (`SOLIDCRAFT_BENCH_PARTS` parts, default 200).
#[test]
#[ignore = "benchmark: run with --release --ignored --nocapture"]
fn bench_generated_assembly() {
    let parts = std::env::var("SOLIDCRAFT_BENCH_PARTS").ok().and_then(|v| v.parse().ok()).unwrap_or(200);
    let bytes = solidcraft_io::write_design(&generated_assembly(parts));
    let ms = |t: std::time::Instant| t.elapsed().as_secs_f64() * 1000.0;
    let t = std::time::Instant::now();
    let doc = solidcraft_io::read_design(&bytes).unwrap();
    let parse = ms(t);
    let t = std::time::Instant::now();
    let s = Session::new(doc);
    let evaluate = ms(t);
    let features: f64 = s.model.results.iter().map(|r| r.ms).sum();
    let t = std::time::Instant::now();
    let world = s.world_state().unwrap();
    world.warm_meshes(|| true);
    let mesh = ms(t);
    let t = std::time::Instant::now();
    let mut s = s;
    let m = measured(&mut s);
    let measure = ms(t);
    println!(
        "{}",
        json!({"parts": parts, "features": s.doc.features.len(), "bodies": world.bodies.len(), "parse_ms": parse, "evaluate_ms": evaluate,
               "in_features_ms": features, "outside_features_ms": evaluate - features, "mesh_ms": mesh, "measure_ms": measure,
               "volume_mm3": m["total"]["volume_mm3"]})
    );
}
