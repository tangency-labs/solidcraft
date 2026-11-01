//! Designs and measures the contract tests share. Each design is built with the document API and
//! opened through `solidcraft_exchange::open`; expected values come from the geometry it
//! describes, not from SolidCraft's own output.
#![allow(dead_code)]

use std::collections::BTreeMap;

use solidcraft_doc::{Dir, Document, FeatureKind, Operation, point_expr};
use solidcraft_exchange::{Body, Edit, Error, MeshVertex, Options, Point, Snapshot, Tolerance, open};
use solidcraft_geom::Vec3;

/// A `length` × 10 × 5 mm box at the origin, its length the user parameter `L`.
pub fn box_doc(length: &str) -> Document {
    let mut doc = Document::new("block");
    doc.set_param("L", length, Some("mm"), None).unwrap();
    doc.add_feature(
        FeatureKind::Box {
            corner: point_expr(Vec3::ZERO),
            length: "L".into(),
            width: "10".into(),
            height: "5".into(),
            operation: Operation::NewBody,
        },
        None,
    )
    .unwrap();
    doc
}

/// A 20 × 20 × 10 mm box with a radius-3 hole through it along Z, at (10, 10).
pub fn holed_doc() -> Document {
    let mut doc = Document::new("plate");
    doc.add_feature(
        FeatureKind::Box {
            corner: point_expr(Vec3::ZERO),
            length: "20".into(),
            width: "20".into(),
            height: "10".into(),
            operation: Operation::NewBody,
        },
        None,
    )
    .unwrap();
    doc.add_feature(
        FeatureKind::Cylinder {
            base: point_expr(Vec3::new(10.0, 10.0, -1.0)),
            axis: Dir(Vec3::Z),
            radius: "3".into(),
            height: "12".into(),
            operation: Operation::Cut,
            references: Vec::new(),
        },
        None,
    )
    .unwrap();
    doc
}

/// A radius-8 sphere centred at (1, 2, 3).
pub fn sphere_doc() -> Document {
    let mut doc = Document::new("ball");
    doc.add_feature(FeatureKind::Sphere { center: point_expr(Vec3::new(1.0, 2.0, 3.0)), radius: "8".into(), operation: Operation::NewBody }, None)
        .unwrap();
    doc
}

pub fn bytes(doc: &Document) -> Vec<u8> {
    doc.to_json().into_bytes()
}

pub fn snap(doc: &Document) -> Snapshot {
    open(&bytes(doc)).unwrap().snapshot(&Options::default()).unwrap()
}

/// A snapshot at a 0.01 mm chord tolerance with `edits`.
pub fn snap_with(doc: &Document, edits: Vec<Edit>) -> Result<Snapshot, Error> {
    open(&bytes(doc))?.snapshot(&Options::default().with_tolerance(Tolerance::Absolute(0.01)).with_edits(edits))
}

pub fn at(b: &Body, v: MeshVertex) -> [f64; 3] {
    b.positions[v.index()].0
}

pub fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

pub fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

pub fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

pub fn dist(a: [f64; 3], b: [f64; 3]) -> f64 {
    dot(sub(a, b), sub(a, b)).sqrt()
}

/// Volume a body's closed mesh encloses (divergence theorem).
pub fn volume(b: &Body) -> f64 {
    b.faces
        .iter()
        .flat_map(|f| f.triangles.iter())
        .map(|t| {
            let [a, c, d] = t.map(|v| at(b, v));
            dot(a, cross(c, d))
        })
        .sum::<f64>()
        / 6.0
}

/// Each undirected side is shared by exactly two triangles, once in each direction.
pub fn watertight(b: &Body) -> bool {
    let mut count: BTreeMap<(MeshVertex, MeshVertex), i32> = BTreeMap::new();
    for [a, c, d] in b.faces.iter().flat_map(|f| f.triangles.iter().copied()) {
        for (p, q) in [(a, c), (c, d), (d, a)] {
            *count.entry((p.min(q), p.max(q))).or_default() += if p < q { 1 } else { 1000 };
        }
    }
    !count.is_empty() && count.values().all(|n| *n == 1001)
}

pub fn bbox(b: &Body) -> ([f64; 3], [f64; 3]) {
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    for Point(p) in &b.positions {
        for k in 0..3 {
            lo[k] = lo[k].min(p[k]);
            hi[k] = hi[k].max(p[k]);
        }
    }
    (lo, hi)
}
