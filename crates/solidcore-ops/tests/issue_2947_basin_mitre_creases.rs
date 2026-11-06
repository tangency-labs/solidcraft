//! #2947: the sloped basins' R4 rounds, mitred at the rim's corners. Each round meets its faces
//! tangentially along its contact lines, and the mitres' creases are G0 by the mitre ruling
//! (`docs/generic-blend.md`, §3.2 and §5), so the cases' continuity names the contact lines.
//!
//! A round of radius r on a wall leaning k across per unit down touches the rim's top plane
//! r (sqrt(1 + k^2) - k) from the rim and the wall r (1 - k / sqrt(1 + k^2)) below the top
//! (k / sqrt(1 + k^2) is the sine of the lean). Each case's `expected.continuity.edges` is read
//! here and each point is required on an edge of the result, with the two faces there tangent.
//! The front rim is 1 wide, under the round's 1.717 or 2.472 on the top, so the front round has
//! no top seam: it is cut by the slab's front face, a crease.

#![allow(
    clippy::disallowed_methods, // closed-form oracles, not ops geometry
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test

mod common;

use common::{edge_at, kernel_copy, rel, volume};
use serde_json::Value;
use solidcore::math::Point3;
use solidcore::nurbs::knots::Side;
use solidcore::query::api_queries::{edge_closest_point, surface_closest_point};
use solidcore::surface::surface::Surface;
use solidcore::topology::face::p2 as face;
use solidcore::topology::{edge, Brep, EdgeId};
use solidcore::Kernel;
use solidcore_ops::blend::{blend, Blend, Corners};
use solidcore_ops::{session, Body};
use solidcore_record::brep::Brep as Record;
use std::io::Read as _;

const CASES: [&str; 3] = [
    "1080-fillet-sloped-basin-1-mm-rim-r4-2-edges",
    "1080-fillet-sloped-basin-1-mm-rim-r4-4-edges",
    "1080-fillet-equal-slope-basin-1-mm-rim-loop-4",
];

const CORPUS: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../solidcore/tests/kernel_cases/fillets"
);

/// The case's entry and its operand.
fn case(name: &str) -> (Value, Body) {
    let fixes: Vec<String> = serde_json::from_str(include_str!(
        "../../solidcore/tests/kernel_cases/fixes.json"
    ))
    .unwrap();
    session::set_fixes(fixes.iter().map(String::as_str)).unwrap();
    let entry = std::fs::read_to_string(format!("{CORPUS}/{name}.json")).unwrap();
    let e: Value = serde_json::from_str(&entry).unwrap();
    let gz = std::fs::File::open(format!("{CORPUS}/{name}.bodies.json.gz")).unwrap();
    let mut text = String::new();
    flate2::read::GzDecoder::new(gz)
        .read_to_string(&mut text)
        .unwrap();
    let mut records: Value = serde_json::from_str(&text).unwrap();
    let record: Record = serde_json::from_value(records["b12"].take()).unwrap();
    (e, Body::from_record(&record, true).unwrap().0)
}

/// The angle between the two faces of `e` at `p`, in radians, either way round.
fn angle_at(k: &Kernel, b: &Brep, e: EdgeId, p: Point3) -> f64 {
    let normal = |s: &Surface| {
        let uv = surface_closest_point(k, s, p).unwrap().uvs[0];
        s.evaluate_normal(k, uv, Side::Left, Side::Left).unwrap()
    };
    let faces = edge::faces(b, e, None).unwrap();
    let [f, g] = faces.as_slice() else {
        panic!("edge at {p:?} has {} faces", faces.len());
    };
    let n = normal(face::surface(b, *f).unwrap().unwrap());
    let m = normal(face::surface(b, *g).unwrap().unwrap());
    n.cross(m).length().atan2(n.dot(m).abs())
}

/// `body` with the entry's edges rounded 4, picked at their points.
fn rounded(e: &Value, body: &Body) {
    let picks: Vec<usize> = e["calls"][3]["params"]["edges"]["points"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| {
            let c: Vec<f64> = serde_json::from_value(p.clone()).unwrap();
            edge_at(body, [c[0], c[1], c[2]])
        })
        .collect();
    blend(body, Blend::Round, &picks, &[4.0], &[], Corners::Filled).unwrap();
}

/// The case rounded as its entry says: every listed seam tangent, its volume the closed form's
/// to the entry's tolerance, and the edges between two rounds (the mitres' creases, at the
/// `corners` corners where two rounds meet) G0.
fn seams(name: &str, corners: usize) {
    let (e, body) = case(name);
    rounded(&e, &body);

    let want = e["expected"]["volume_mm3"].as_f64().unwrap();
    assert!(
        rel(volume(&body), want) <= e["expected"]["tolerance"]["rel"].as_f64().unwrap(),
        "{name}: volume {} against {want}",
        volume(&body)
    );

    let (k, b) = kernel_copy(&body);
    let edges = e["expected"]["continuity"]["edges"].as_array().unwrap();
    assert!(!edges.is_empty());
    for p in edges {
        let c: Vec<f64> = serde_json::from_value(p.clone()).unwrap();
        let q = Point3::new(c[0], c[1], c[2]);
        let (e, d) = b
            .edge_list()
            .iter()
            .map(|&e| {
                (
                    e,
                    edge_closest_point(&k, &b, e, q).unwrap().closest.distance,
                )
            })
            .min_by(|x, y| x.1.total_cmp(&y.1))
            .unwrap();
        assert!(d < 1e-5, "{name}: {q:?} is {d} from the nearest edge");
        let angle = angle_at(&k, &b, e, q);
        assert!(angle < 1e-7, "{name}: the seam at {q:?} turns {angle} rad");
    }

    let mut found = 0;
    for &e in b.edge_list() {
        let faces = edge::faces(&b, e, None).unwrap();
        // the walls are lofted planes
        let plane = |&f: &solidcore::FaceId| {
            let s = face::surface(&b, f).unwrap().unwrap();
            s.is_planar(&k, 1e-6).unwrap().is_planar
        };
        if faces.len() != 2 || faces.iter().any(plane) {
            continue;
        }
        let c = edge::curve(&b, e).unwrap().unwrap();
        let p = c
            .evaluate_point(&k, edge::interval(&b, e).unwrap().evaluate(0.5))
            .unwrap();
        let angle = angle_at(&k, &b, e, p);
        assert!(
            angle > 1e-2,
            "{name}: the crease at {p:?} turns {angle} rad"
        );
        found += 1;
    }
    assert!(
        found >= corners,
        "{name}: {found} creases between two rounds"
    );
}

/// The three results as `.smb` under `ISSUE_2947_OUT`, for the cases' renders.
#[test]
#[ignore = "writes the bodies for the renders"]
fn bodies_for_the_pictures() {
    let out = std::env::var("ISSUE_2947_OUT").unwrap();
    for name in CASES {
        let (e, body) = case(name);
        rounded(&e, &body);
        std::fs::write(format!("{out}/{name}.smb"), body.to_smb(true).unwrap()).unwrap();
    }
}

#[test]
fn sloped_basin_two_edges() {
    seams("1080-fillet-sloped-basin-1-mm-rim-r4-2-edges", 1);
}

#[test]
fn sloped_basin_four_edges() {
    seams("1080-fillet-sloped-basin-1-mm-rim-r4-4-edges", 4);
}

#[test]
fn equal_slope_basin_loop() {
    seams("1080-fillet-equal-slope-basin-1-mm-rim-loop-4", 4);
}
