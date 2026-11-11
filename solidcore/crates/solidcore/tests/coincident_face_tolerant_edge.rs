#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
//! Coincident faces whose edge lies off their surface within its own tolerance
//! (solidcraft#11997).
//!
//! `FaceIntersect` on coincident surfaces copies both faces' edges as the intersection
//! curves and classifies each on both faces. A tolerant edge (an imported body's, say) is
//! only as close to the surface as its tolerance says: classified at the faces'
//! tolerance, its crossings with the other face's edges are missed, and the piece kept is
//! wrong. In #11997 the far end of such a piece, off the other face's surface, failed its
//! UV solve (status 1001).
use solidcore::curve::curve::Curve;
use solidcore::curve::line::Line;
use solidcore::kernel::KernelOptions;
use solidcore::math::{Point3, Vec3};
use solidcore::primitive::api_primitives as api;
use solidcore::topology::face::p3::{face_intersect_breps, FaceBreps, FaceIntersectOptions};
use solidcore::topology::{Brep, EdgeId, FaceId};
use solidcore::Kernel;

/// The face of `b` whose vertices all lie at height `z`.
fn face_at(b: &Brep, z: f64) -> FaceId {
    b.faces()
        .ids()
        .find(|&f| {
            let vs = solidcore::topology::face::p4::vertices(b, f, None, false, None).unwrap();
            !vs.is_empty()
                && vs
                    .iter()
                    .all(|&v| (b.vertex(v).unwrap().point.z - z).abs() < 1e-9)
        })
        .unwrap()
}

/// The edge of `b` from (x, 0, z) to (x, 1, z).
fn edge_at(b: &Brep, x: f64, z: f64) -> EdgeId {
    b.edges()
        .ids()
        .find(|&e| {
            b.edge(e).unwrap().vertices.iter().all(|&v| {
                let p = b.vertex(v).unwrap().point;
                (p.x - x).abs() < 1e-9 && (p.z - z).abs() < 1e-9
            })
        })
        .unwrap()
}

#[test]
fn coincident_faces_split_a_tolerant_edge_where_it_crosses_the_other_face() {
    let mut k = Kernel::new(&KernelOptions::default());
    // A's top (z = 1) over x 0..1, y 0..1; B's over x 0.5..1.5, y 0.25..0.75.
    let mut a = api::create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 1.0, 1.0, 1.0).unwrap();
    let mut b = api::create_box(&mut k, Point3::new(0.5, 0.25, 0.0), 1.0, 0.5, 1.0).unwrap();
    let (fa, fb) = (face_at(&a, 1.0), face_at(&b, 1.0));

    // A's top edge at x = 1, written 5e-5 above the plane, with a tolerance that covers it:
    // it crosses B's top at y = 0.25 and 0.75, 5e-5 from B's edges there.
    let dz = 5e-5;
    let e = edge_at(&a, 1.0, 1.0);
    let line: Curve = Line::from_points(
        Vec3::new(1.0, 0.0, 1.0 + dz),
        Vec3::new(1.0, 1.0, 1.0 + dz),
        3,
    )
    .into();
    let ends = a.edge(e).unwrap().vertices;
    for v in ends {
        let x = a.vertex_mut(v).unwrap();
        x.point.z += dz;
        x.tolerance = 1e-3;
    }
    let edge = a.edge_mut(e).unwrap();
    edge.interval = line.natural_interval();
    edge.curve = Some(line);
    edge.tolerance = 1e-3;

    let fi = face_intersect_breps(
        &k,
        FaceBreps::Two(&mut a, &mut b),
        fa,
        fb,
        1e-6,
        20.0,
        &FaceIntersectOptions {
            coincidence: Some(true),
            ..FaceIntersectOptions::default()
        },
    )
    .unwrap();

    // The pieces of the raised edge kept: inside B's top, y 0.25 to 0.75.
    let mut kept = Vec::new();
    for c in &fi.curves_3d {
        let ivl = c.natural_interval();
        let (p, q) = (
            c.evaluate_point(&k, ivl.min()).unwrap(),
            c.evaluate_point(&k, ivl.max()).unwrap(),
        );
        if (p.x - 1.0).abs() < 1e-3 && (q.x - 1.0).abs() < 1e-3 && p.z > 1.0 + dz / 2.0 {
            kept.push((p.y.min(q.y), p.y.max(q.y)));
        }
    }
    assert_eq!(kept.len(), 1, "{kept:?}");
    let (lo, hi) = kept[0];
    assert!(
        (lo - 0.25).abs() < 1e-3 && (hi - 0.75).abs() < 1e-3,
        "{kept:?}"
    );
}
