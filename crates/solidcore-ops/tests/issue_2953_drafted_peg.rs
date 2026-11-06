//! #2953: the drafted peg's front top edge (convex) and concave arc rounded 2 together, the
//! tangent peg of #2360 extruded with a 5 degree inward taper. The bar's drafted front face runs
//! tangent into the cone along its generator through x = 40, and the top's cone is the same cone
//! split along that generator, so a fourth edge runs on from the seam past the vertex
//! V = (40, y0, 10), y0 = 10 tan 5 degrees. The corner is the Mixed one: the rails on the top
//! cross at X, and every seam within its box is G1.
//!
//! The ball rolls on the top z = 10 and on a wall that leans 5 degrees: the convex round's rail
//! on the top stands r tan(42.5 degrees) in from the front edge, the concave round's the same
//! out from the arc R0 = 8 - y0 about (40, 8). So X lies on the line y = y0 + d and the circle
//! of radius R0 + d, d = r tan(42.5 degrees).

#![allow(
    clippy::disallowed_methods, // closed-form oracles, not ops geometry
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test

mod common;

use common::{edge_at, kernel_copy};
use serde_json::Value;
use solidcore::geom::global_solver::{SolutionRequested, SolverOperation};
use solidcore::math::{Point2, Vec3};
use solidcore::nurbs::knots::Side;
use solidcore::surface::surface::p4::NoObject;
use solidcore::surface::surface::SurfaceKind;
use solidcore::topology::edge;
use solidcore::topology::edgeuse::p1 as eu1;
use solidcore::topology::edgeuse::p3::{evaluate_binormal, BinormalOptions};
use solidcore::topology::{face::p2 as face, vertex, Brep};
use solidcore::Kernel;
use solidcore_ops::blend::{blend, Blend, Corners};
use solidcore_ops::{session, Body, CheckLevel};
use solidcore_record::brep::Brep as Record;
use std::io::Read as _;

const CASE: &str = "report-drafted-peg-front-and-concave-rounds-together-r2";
const CORPUS: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../solidcore/tests/kernel_cases/fillets"
);

/// The peg with the case's two edges rounded 2.
fn rounded() -> Body {
    let fixes: Vec<String> = serde_json::from_str(include_str!(
        "../../solidcore/tests/kernel_cases/fixes.json"
    ))
    .unwrap();
    session::set_fixes(fixes.iter().map(String::as_str)).unwrap();
    let gz = std::fs::File::open(format!("{CORPUS}/{CASE}.bodies.json.gz")).unwrap();
    let mut text = String::new();
    flate2::read::GzDecoder::new(gz)
        .read_to_string(&mut text)
        .unwrap();
    let mut records: Value = serde_json::from_str(&text).unwrap();
    let record: Record = serde_json::from_value(records["b12"].take()).unwrap();
    let (body, _) = Body::from_record(&record, true).unwrap();
    let y0 = 10.0 * 5.0_f64.to_radians().tan();
    let picks = [
        edge_at(&body, [20.0, y0, 10.0]),
        edge_at(&body, [32.0 + y0, 8.0, 10.0]),
    ];
    blend(&body, Blend::Round, &picks, &[2.0], &[], Corners::Filled).unwrap();
    body
}

/// X, the rails' crossing on the top.
fn crossing(r: f64) -> [f64; 3] {
    let y0 = 10.0 * 5.0_f64.to_radians().tan();
    let d = r * 42.5_f64.to_radians().tan();
    let (radius, rail) = (8.0 - y0 + d, y0 + d);
    [
        40.0 - (radius * radius - (8.0 - rail) * (8.0 - rail)).sqrt(),
        rail,
        10.0,
    ]
}

fn angle(a: Vec3, b: Vec3) -> f64 {
    a.cross(b).length().atan2(a.dot(b))
}

/// The largest angle between the faces' surface normals, turned as their face uses, at 31
/// stations inside each edge within `inside`, and how many edges have a station there.
fn worst_seam(k: &Kernel, b: &mut Brep, inside: impl Fn(Vec3) -> bool) -> (f64, usize) {
    let (mut worst, mut seams) = (0.0_f64, 0);
    for e in b.edge_list().to_vec() {
        let Some(curve) = edge::curve(b, e).unwrap().cloned() else {
            continue;
        };
        if edge::start_vertex(b, e).unwrap() == edge::end_vertex(b, e).unwrap() {
            continue;
        }
        let ivl = edge::interval(b, e).unwrap();
        let prim = eu1::primary_edgeuse(b, e).unwrap().unwrap();
        let rad = eu1::radial(b, prim).unwrap().unwrap();
        assert_ne!(prim, rad, "edge with one face");
        let mut counted = false;
        for st in 1..32 {
            let t = ivl.evaluate(f64::from(st) / 32.0);
            let m = curve.evaluate_point(k, t).unwrap();
            if !inside(m) {
                continue;
            }
            if !counted {
                counted = true;
                seams += 1;
            }
            let mut ns = Vec::new();
            for eu in [prim, rad] {
                let bi =
                    evaluate_binormal(k, b, eu, t, false, &BinormalOptions::default()).unwrap();
                let f = eu1::face(b, eu).unwrap().unwrap();
                let srf = b.face(f).unwrap().surface.clone().unwrap();
                let dom = srf.natural_uv_domain_with_nurb(k).unwrap();
                let sols = srf
                    .global_point_solve_point_test_off::<NoObject>(
                        k,
                        None,
                        &dom,
                        SolverOperation::Minimize,
                        m,
                        1e-9,
                        None,
                        SolutionRequested::Single,
                    )
                    .unwrap();
                let sol = sols.get(0).unwrap();
                let uv = Point2::new(sol.start.param(0).unwrap(), sol.start.param(1).unwrap());
                let mut n = srf.evaluate_normal(k, uv, Side::Left, Side::Left).unwrap();
                if n.dot(bi.face_normal) < 0.0 {
                    n = -n;
                }
                ns.push(n);
            }
            worst = worst.max(angle(ns[0], ns[1]));
        }
    }
    (worst, seams)
}

#[test]
fn the_corner_is_the_mixed_one_and_g1() {
    let body = rounded();
    let report = body.check(CheckLevel::All).unwrap();
    assert!(report.is_valid(), "{}", report.text);
    let (k, mut b) = kernel_copy(&body);
    // the peg's 8, the two rounds, the corner's two
    assert_eq!(b.face_list().len(), 12);

    let x = crossing(2.0);
    let (nearest, at) = b
        .vertex_list()
        .iter()
        .map(|&v| {
            let p = vertex::point(&b, v).unwrap();
            let d = (p.x - x[0]).hypot(p.y - x[1]).hypot(p.z - x[2]);
            (d, v)
        })
        .min_by(|a, c| a.0.total_cmp(&c.0))
        .unwrap();
    assert!(
        nearest < 1e-7,
        "X {x:?} is {nearest} from the nearest vertex"
    );
    assert!(vertex::edges(&b, at, None).unwrap().len() >= 3);

    // the corner's box: past X, and short of the rounds' ends and the sharp edges
    let (worst, seams) = worst_seam(&k, &mut b, |m| {
        m.x > x[0] - 2.0 && m.x <= 40.0 + 1e-6 && m.y < 8.0 && m.z > 5.0 && m.z < 15.0
    });
    assert!(seams >= 7, "{seams} seams");
    assert!(worst < 1e-8, "{worst} rad");

    // no patch stands out of the front face: below the top, every point of a free-form face
    // is behind the plane through (x, 0, 0) and (x, y0, 10)
    let tan = 5.0_f64.to_radians().tan();
    let mut stands_out = f64::NEG_INFINITY;
    for f in b.face_list().to_vec() {
        let s = face::surface(&b, f).unwrap().unwrap();
        if !matches!(s.kind(), SurfaceKind::BSpline(_)) {
            continue;
        }
        let d = face::uv_domain(&b, f).unwrap();
        for i in 0..=20 {
            for j in 0..=20 {
                let uv = Point2::new(
                    d.min().x + (d.max().x - d.min().x) * f64::from(i) / 20.0,
                    d.min().y + (d.max().y - d.min().y) * f64::from(j) / 20.0,
                );
                let p = s.evaluate_point(&k, uv).unwrap();
                if p.z < 10.0 && p.x <= 40.0 {
                    stands_out = stands_out.max(tan * p.z - p.y);
                }
            }
        }
    }
    assert!(stands_out < 4e-5, "{stands_out}");
}

/// The result as `.smb` under `ISSUE_2953_OUT`, for the case's render.
#[test]
#[ignore = "writes the body for the render"]
fn body_for_the_picture() {
    let out = std::env::var("ISSUE_2953_OUT").unwrap();
    std::fs::write(format!("{out}/{CASE}.smb"), rounded().to_smb(true).unwrap()).unwrap();
}
