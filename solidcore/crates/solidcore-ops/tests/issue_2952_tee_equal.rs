//! #2952: the equal pipes' tee, R 8, one half ellipse of the crossing rounded. The pipes run
//! along x and z through the origin, so their intersection is the two ellipses x = z and
//! x = -z, crossing at (0, +-8, 0) where the pipes touch. The ball of radius r rolls on the
//! offsets of radius R = 8 + r, centre (t, y, t), y^2 + t^2 = R^2, and its feet are
//! (t, 8 y / R, 8 t / R) on the pipe along x and (8 t / R, 8 y / R, t) on the one along z.
//! At the crossing they meet, so the round fades out at both ends.
//!
//! A station at a crossing lies on both conics of the spine's pair. Reading the plane there
//! left the choice to rounding, which put the round on x = -z, across the pipe from its edge;
//! the plane is now read off the edge.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::disallowed_methods
)] // test: the closed forms use roots and trigonometry

mod common;

use common::{edge_at, kernel_copy};
use serde_json::Value;
use solidcore::math::Point3;
use solidcore::nurbs::knots::Side;
use solidcore::query::api_queries::{edge_closest_point, surface_closest_point};
use solidcore::surface::surface::Surface;
use solidcore::topology::face::p2 as face;
use solidcore::topology::{edge, vertex, Brep, EdgeId};
use solidcore::Kernel;
use solidcore_ops::blend::{blend, Blend, Corners};
use solidcore_ops::{session, Body, CheckLevel};
use solidcore_record::brep::Brep as Record;
use std::io::Read as _;

const CASE: &str = "cov-junction-tee-equal-round";

/// The tee with the case's half ellipse rounded `r`.
fn rounded(r: f64) -> Body {
    let fixes: Vec<String> = serde_json::from_str(include_str!(
        "../../solidcore/tests/kernel_cases/fixes.json"
    ))
    .unwrap();
    session::set_thread_fixes(fixes.iter().map(String::as_str)).unwrap();
    let gz = std::fs::File::open(format!(
        "{}/../solidcore/tests/kernel_cases/fillets/{CASE}.bodies.json.gz",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap();
    let mut text = String::new();
    flate2::read::GzDecoder::new(gz)
        .read_to_string(&mut text)
        .unwrap();
    let mut records: Value = serde_json::from_str(&text).unwrap();
    let record: Record = serde_json::from_value(records["b12"].take()).unwrap();
    let (body, _) = Body::from_record(&record, true).unwrap();
    // the crease x = z at t = 8 sin(th), y = 8 cos(th), on each of its three edges
    let picks: Vec<usize> = [30.0_f64, 45.0, 120.0]
        .iter()
        .map(|d| {
            let (s, c) = d.to_radians().sin_cos();
            edge_at(&body, [8.0 * s, 8.0 * c, 8.0 * s])
        })
        .collect();
    blend(&body, Blend::Round, &picks, &[r], &[], Corners::Filled).unwrap();
    body
}

/// The edge of `b` nearest `q`, and how far it is.
fn nearest(k: &Kernel, b: &Brep, q: Point3) -> (EdgeId, f64) {
    b.edge_list()
        .iter()
        .map(|&e| {
            let d = edge_closest_point(k, b, e, q).unwrap().closest.distance;
            (e, d)
        })
        .min_by(|x, y| x.1.total_cmp(&y.1))
        .unwrap()
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

fn graded(r: f64) {
    let body = rounded(r);
    let report = body.check(CheckLevel::All).unwrap();
    assert!(report.is_valid(), "r {r}: {}", report.text);
    let (k, b) = kernel_copy(&body);
    // the tee's 5, one round per edge of the half ellipse
    assert_eq!(b.face_list().len(), 8, "r {r}");

    // both rails on the half ellipse x = z, G1, at stations along the spine
    let big_r = 8.0 + r;
    let scale = 8.0 / big_r;
    for i in 1..24 {
        let th = std::f64::consts::PI * f64::from(i) / 24.0;
        let (t, y) = (big_r * th.sin(), big_r * th.cos());
        for q in [
            Point3::new(t, y * scale, t * scale),
            Point3::new(t * scale, y * scale, t),
        ] {
            let (e, d) = nearest(&k, &b, q);
            assert!(
                d < 1e-6,
                "r {r}: the foot {q:?} is {d} from the nearest edge"
            );
            let angle = angle_at(&k, &b, e, q);
            assert!(angle < 1e-6, "r {r}: the seam at {q:?} turns {angle} rad");
        }
    }

    // the round fades out at the crossings: the rails end on the tee's own vertices
    let on = |p: [f64; 3]| {
        let q = Point3::new(p[0], p[1], p[2]);
        b.vertex_list()
            .iter()
            .any(|&v| vertex::point(&b, v).unwrap().distance(q) < 1e-9)
    };
    assert!(on([0.0, 8.0, 0.0]) && on([0.0, -8.0, 0.0]), "r {r}");
}

#[test]
fn rounds_the_half_ellipse_r1() {
    graded(1.0);
}

#[test]
fn rounds_the_half_ellipse_r_quarter() {
    graded(0.25);
}

/// The result as `.smb` under `ISSUE_2952_OUT`, for the case's render.
#[test]
#[ignore = "writes the body for the render"]
fn body_for_the_picture() {
    let out = std::env::var("ISSUE_2952_OUT").unwrap();
    let bytes = rounded(1.0).to_smb(true).unwrap();
    std::fs::write(format!("{out}/{CASE}.smb"), bytes).unwrap();
}
