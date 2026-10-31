use std::collections::HashMap;

use serde_json::{Value, json};

use crate::Session;
use crate::geom::Vec3;

fn decode(s: &str) -> Vec<u8> {
    const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let digits: Vec<u32> = s.bytes().filter(|&c| c != b'=').map(|c| B64.iter().position(|&d| d == c).unwrap() as u32).collect();
    let mut out = Vec::new();
    for chunk in digits.chunks(4) {
        let n = chunk.iter().enumerate().fold(0u32, |n, (k, d)| n | (d << (18 - 6 * k)));
        out.extend_from_slice(&n.to_be_bytes()[1..chunk.len()]);
    }
    out
}

/// A mesh as the receiving program reads it back.
struct Read {
    positions: Vec<Vec3>,
    sizes: Vec<u32>,
    loops: Vec<u32>,
    normals: Vec<Vec3>,
    flags: Vec<u8>,
    edges: Vec<[u32; 2]>,
    stats: Value,
}

impl Read {
    fn of(m: &Value) -> Read {
        let bytes = |k: &str| decode(m[k].as_str().unwrap_or_else(|| panic!("{k} missing")));
        let u32s = |k: &str| bytes(k).chunks(4).map(|c| u32::from_le_bytes(c.try_into().unwrap())).collect::<Vec<_>>();
        let vec3s = |k: &str| {
            let f: Vec<f64> = bytes(k).chunks(4).map(|c| f64::from(f32::from_le_bytes(c.try_into().unwrap()))).collect();
            f.chunks(3).map(|c| Vec3::new(c[0], c[1], c[2])).collect::<Vec<_>>()
        };
        let o = &m["origin"];
        let origin = Vec3::new(o[0].as_f64().unwrap(), o[1].as_f64().unwrap(), o[2].as_f64().unwrap());
        let edges = u32s("edges").chunks(2).map(|c| [c[0], c[1]]).collect();
        Read {
            positions: vec3s("positions").into_iter().map(|p| p + origin).collect(),
            sizes: u32s("poly_sizes"),
            loops: u32s("loops"),
            normals: if m.get("loop_normals").is_some() { vec3s("loop_normals") } else { vec![] },
            flags: bytes("edge_flags"),
            edges,
            stats: m["stats"].clone(),
        }
    }

    fn polygons(&self) -> Vec<&[u32]> {
        let mut at = 0;
        self.sizes
            .iter()
            .map(|&n| {
                let p = &self.loops[at..at + n as usize];
                at += n as usize;
                p
            })
            .collect()
    }

    /// Uses of every edge of the polygons.
    fn edge_uses(&self) -> HashMap<(u32, u32), usize> {
        let mut uses = HashMap::new();
        for p in self.polygons() {
            for (i, &a) in p.iter().enumerate() {
                let b = p[(i + 1) % p.len()];
                *uses.entry((a.min(b), a.max(b))).or_insert(0) += 1;
            }
        }
        uses
    }

    /// V − E + F, and whether every edge has exactly two polygons.
    fn euler_closed(&self) -> (i64, bool) {
        let uses = self.edge_uses();
        (self.positions.len() as i64 - uses.len() as i64 + self.sizes.len() as i64, uses.values().all(|&n| n == 2))
    }

    fn volume(&self) -> f64 {
        self.polygons()
            .iter()
            .map(|p| {
                (1..p.len() - 1)
                    .map(|i| self.positions[p[0] as usize].dot(self.positions[p[i] as usize].cross(self.positions[p[i + 1] as usize])) / 6.0)
                    .sum::<f64>()
            })
            .sum()
    }
}

fn meshes(s: &mut Session, p: Value) -> Value {
    s.execute("live.meshes", &p).unwrap_or_else(|e| panic!("{e}"))
}

fn only_mesh(r: &Value) -> Read {
    let meshes = r["meshes"].as_array().unwrap();
    assert_eq!(meshes.len(), 1, "{r}");
    Read::of(&meshes[0])
}

#[test]
fn a_box_is_six_quads_on_eight_corners() {
    let mut s = Session::default();
    s.execute("solid.box", &json!({"length": 10, "width": 20, "height": 30, "body_name": "Block"})).unwrap();
    let r = meshes(&mut s, json!({}));
    assert_eq!((r["unit"].as_str(), r["up"].as_str()), (Some("mm"), Some("Z")));
    let objects = r["objects"].as_array().unwrap();
    assert_eq!(objects.len(), 1);
    assert_eq!(objects[0]["key"], "Block");
    assert_eq!(objects[0]["matrix"], json!([1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0]));
    let m = only_mesh(&r);
    assert_eq!(objects[0]["mesh"], r["meshes"][0]["key"]);
    assert_eq!((m.positions.len(), m.sizes.len(), m.edge_uses().len()), (8, 6, 12));
    assert_eq!(m.euler_closed(), (2, true));
    assert!((m.volume() - 6000.0).abs() < 1e-3, "{}", m.volume());
    assert_eq!(m.edges.len(), 12);
    assert!(m.flags.iter().all(|&f| f == 3), "every box edge a sharp B-rep edge: {:?}", m.flags);
    assert_eq!(m.stats["quads"], 6);
    let names = r["meshes"][0]["face_names"].as_array().unwrap();
    assert_eq!(names.len(), 6);
}

#[test]
fn a_cylinder_is_two_caps_and_a_band_of_quads() {
    let mut s = Session::default();
    s.execute("solid.cylinder", &json!({"radius": 5, "height": 10})).unwrap();
    let m = only_mesh(&meshes(&mut s, json!({})));
    assert_eq!(m.euler_closed(), (2, true));
    assert_eq!(m.stats["ngons"], 2, "{}", m.stats);
    assert_eq!(m.stats["triangles"], 0, "{}", m.stats);
    assert!(m.stats["quads"].as_u64().unwrap() >= 8, "{}", m.stats);
    let volume = std::f64::consts::PI * 25.0 * 10.0;
    assert!((m.volume() - volume).abs() < volume * 0.02, "{} vs {volume}", m.volume());
    // The band's corners take the surface's normal: radial, whatever the facets.
    for (k, n) in m.normals.iter().enumerate() {
        if n.z.abs() < 0.5 {
            let p = m.positions[m.loops[k] as usize];
            assert!((Vec3::new(p.x, p.y, 0.0).normalized().unwrap().dot(*n) - 1.0).abs() < 1e-5, "{p:?} {n:?}");
        }
    }
    // The rims are creases.
    assert!(m.flags.iter().filter(|&&f| f & 2 != 0).count() >= 16);
}

#[test]
fn fillets_are_quads_and_their_edges_are_tangent() {
    let mut s = Session::default();
    s.execute("solid.box", &json!({"length": 20, "width": 20, "height": 20})).unwrap();
    s.execute("solid.fillet", &json!({"edges": [[10, 0, 20]], "radius": 4})).unwrap();
    let m = only_mesh(&meshes(&mut s, json!({})));
    assert_eq!(m.euler_closed(), (2, true));
    assert_eq!(m.stats["triangles"], 0, "{}", m.stats);
    let tangent = m.flags.iter().filter(|&&f| f & 4 != 0).count();
    assert!(tangent >= 2, "the fillet's two long edges are tangent: {:?}", m.flags);
    assert!(m.flags.iter().all(|&f| f & 2 == 0 || f & 4 == 0), "never both sharp and tangent");
}

#[test]
fn a_hole_through_a_block_has_genus_one() {
    let mut s = Session::default();
    s.execute("solid.box", &json!({"length": 30, "width": 30, "height": 10, "body_name": "Plate"})).unwrap();
    s.execute("solid.cylinder", &json!({"radius": 5, "height": 10, "base": [15, 15, 0], "operation": "cut", "targets": ["Plate"]})).unwrap();
    let m = only_mesh(&meshes(&mut s, json!({})));
    assert_eq!(m.euler_closed(), (0, true));
    let volume = 9000.0 - std::f64::consts::PI * 250.0;
    assert!((m.volume() - volume).abs() < volume * 0.01, "{} vs {volume}", m.volume());
}

#[test]
fn a_sphere_closes() {
    let mut s = Session::default();
    s.execute("solid.sphere", &json!({"radius": 10})).unwrap();
    let m = only_mesh(&meshes(&mut s, json!({})));
    assert!(m.euler_closed().1, "{}", m.stats);
    let volume = 4.0 / 3.0 * std::f64::consts::PI * 1000.0;
    assert!((m.volume() - volume).abs() < volume * 0.03, "{} vs {volume}", m.volume());
}

#[test]
fn placements_of_one_part_share_a_mesh() {
    let mut s = Session::default();
    let part = s.execute("component.create", &json!({"name": "Bracket", "grounded": false})).unwrap();
    s.execute("solid.box", &json!({"length": 10, "width": 10, "height": 10})).unwrap();
    s.execute("component.activate", &json!({"component": 0})).unwrap();
    s.execute("occurrence.copy", &json!({"component": part["component"], "translate": [30, 0, 0]})).unwrap();
    let r = meshes(&mut s, json!({}));
    let objects = r["objects"].as_array().unwrap();
    assert_eq!(objects.len(), 2, "{r}");
    assert_eq!(objects[0]["mesh"], objects[1]["mesh"]);
    assert_ne!(objects[0]["matrix"], objects[1]["matrix"]);
    assert_eq!(r["meshes"].as_array().unwrap().len(), 1);
    assert!(r["components"].as_array().unwrap().iter().any(|c| c["name"] == "Bracket"));
    assert!(objects.iter().all(|o| o["component"] == part["component"]));
}

#[test]
fn meshes_the_caller_has_are_not_sent_again() {
    let mut s = Session::default();
    s.execute("solid.box", &json!({"length": 10, "width": 10, "height": 10, "body_name": "A"})).unwrap();
    s.execute("solid.cylinder", &json!({"radius": 3, "height": 5, "base": [20, 0, 0], "body_name": "B"})).unwrap();
    let first = meshes(&mut s, json!({}));
    let keys: Vec<Value> = first["meshes"].as_array().unwrap().iter().map(|m| m["key"].clone()).collect();
    assert_eq!(keys.len(), 2);
    let again = meshes(&mut s, json!({"have": keys.clone()}));
    assert!(again["meshes"].as_array().unwrap().is_empty());
    assert_eq!(again["objects"], first["objects"], "the same objects, the same keys");
    // A hidden body is left out unless asked for.
    s.execute("browser.visibility", &json!({"items": ["A"], "visible": false})).unwrap();
    let after = meshes(&mut s, json!({"have": keys}));
    let objects = after["objects"].as_array().unwrap();
    assert_eq!(objects.len(), 1, "{after}");
    assert_eq!(objects[0]["key"], "B");
    let all = meshes(&mut s, json!({"include_hidden": true, "have": keys}));
    assert!(all["objects"].as_array().unwrap().iter().any(|o| o["key"] == "A" && o["visible"] == false));
    assert!(all["meshes"].as_array().unwrap().is_empty());
}

#[test]
fn a_body_can_be_meshed_its_own_way() {
    let mut s = Session::default();
    s.execute("solid.cylinder", &json!({"radius": 5, "height": 10, "body_name": "Pin"})).unwrap();
    let r = meshes(&mut s, json!({"overrides": [{"body": "Pin", "topology": "triangles"}]}));
    let m = only_mesh(&r);
    assert_eq!((m.stats["quads"].as_u64(), m.stats["ngons"].as_u64()), (Some(0), Some(0)));
    assert_eq!(m.euler_closed(), (2, true));
    let coarse = only_mesh(&meshes(&mut s, json!({"quality": "coarse"})));
    let fine = only_mesh(&meshes(&mut s, json!({"chord_relative": 1e-4, "angle_deg": 5})));
    assert!(fine.positions.len() > coarse.positions.len(), "{} vs {}", fine.positions.len(), coarse.positions.len());
    let quads = only_mesh(&meshes(&mut s, json!({"topology": "quads", "normals": false})));
    assert_eq!(quads.stats["ngons"], 0);
    assert!(quads.normals.is_empty());
    assert!(meshes(&mut s, json!({"bodies": ["Nothing"]}))["objects"].as_array().unwrap().is_empty());
}

#[test]
fn bad_settings_are_refused() {
    let mut s = Session::default();
    s.execute("solid.box", &json!({"length": 10, "width": 10, "height": 10})).unwrap();
    for p in [
        json!({"chord_mm": 0}),
        json!({"chord_mm": -1}),
        json!({"chord_relative": 0.5}),
        json!({"angle_deg": 0}),
        json!({"angle_deg": 400}),
        json!({"topology": "hexagons"}),
        json!({"quality": "best"}),
        json!({"overrides": [{"body": "Body1", "chord_mm": -2}]}),
        json!({"bodies": "Body1"}),
    ] {
        assert!(s.execute("live.meshes", &p).is_err(), "{p}");
    }
    assert!(s.execute("live.meshes", &json!({"max_mb": 1e9})).is_ok(), "clamped");
}

#[test]
fn a_sketch_being_dragged_is_meshed_as_it_stands() {
    let mut s = Session::default();
    for (id, p) in [
        ("sketch.create", json!({"plane":"XY"})),
        ("sketch.rectangle.two_point", json!({"p0":[5,5],"p1":[25,20]})),
        ("sketch.finish", json!({})),
        ("solid.extrude", json!({"distance":5})),
        ("sketch.edit", json!({"sketch":"Sketch1"})),
        ("sketch.drag", json!({"entity":"l1.start","from":[5,5],"to":[4,5],"live":true})),
    ] {
        s.execute(id, &p).unwrap();
    }
    let m = only_mesh(&meshes(&mut s, json!({})));
    assert_eq!(m.euler_closed(), (2, true));
}
