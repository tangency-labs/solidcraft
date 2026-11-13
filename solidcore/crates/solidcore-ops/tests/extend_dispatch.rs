//! One Extend for any sheet: `extend_sheet` takes the sheet as it is and routes it to the one
//! path that fits it. A sheet of one face is extended as that face; several faces that are the
//! pieces of one surface as that surface; plane faces that merge into one plane face as that
//! face, its trim grown; a curved face with the plane faces beside it as one side across them.
//! Anything else is refused with its reason.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::float_arithmetic
)]

use std::f64::consts::{FRAC_1_SQRT_2, PI};

use solidcore_ops::extend::{extend_sheet, Continuation};
use solidcore_ops::sweep::{loft, Curve};
use solidcore_ops::{Body, Error, Order};

/// How far each sheet is extended.
const DISTANCE: f64 = 5.0;
/// `1 / sqrt 2` (ops' tests compute no roots).
const W: f64 = FRAC_1_SQRT_2;

fn line(start: [f64; 3], end: [f64; 3]) -> Curve {
    Curve::Line { start, end }
}

/// The half circle of radius `r` about `c`, counter-clockwise about +z from +x.
fn half_circle(c: [f64; 3], r: f64) -> Curve {
    Curve::Arc {
        center: c,
        axis: [0.0, 0.0, 1.0],
        reference: [1.0, 0.0, 0.0],
        radius: r,
        start: 0.0,
        end: PI,
    }
}

/// The skin through `sections`, degree 1, open, and its order.
fn skin(sections: &[Vec<Curve>]) -> (Body, Order) {
    let body = loft(sections, 1, false).unwrap();
    let (_, order) = body.to_record().unwrap();
    (body, order)
}

/// The record edge of `body` through `p`.
fn edge_through(body: &Body, order: &Order, p: [f64; 3]) -> usize {
    (0..order.edges.len())
        .find(|&i| {
            body.edge_closest_point(order, i, p)
                .unwrap()
                .is_some_and(|at| at.distance < 1e-9)
        })
        .unwrap_or_else(|| panic!("no edge through {p:?}"))
}

/// The record's vertices of `body`.
fn vertices(body: &Body) -> Vec<[f64; 3]> {
    let (r, _) = body.to_record().unwrap();
    r.vertices.iter().map(|v| v.point).collect()
}

/// Whether `body` has a vertex within `1e-6` of `p`.
fn has_vertex(body: &Body, p: [f64; 3]) -> bool {
    vertices(body)
        .iter()
        .any(|v| (0..3).all(|i| (v[i] - p[i]).abs() < 1e-6))
}

/// The plane sheet x from 0 to 9, y from 0 to 8, at z = 0: one face.
fn flat() -> (Body, Order) {
    skin(&[
        vec![line([0.0, 0.0, 0.0], [9.0, 0.0, 0.0])],
        vec![line([0.0, 8.0, 0.0], [9.0, 8.0, 0.0])],
    ])
}

/// A quarter circle at height `z`: one rational quadratic span from `p0` to `p2` about the
/// corner `p1`.
fn quarter(p0: [f64; 2], p1: [f64; 2], p2: [f64; 2], z: f64) -> Curve {
    Curve::Nurbs {
        degree: 2,
        points: vec![
            [p0[0], p0[1], z, 1.0],
            [p1[0], p1[1], z, W],
            [p2[0], p2[1], z, 1.0],
        ],
        knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
    }
}

/// The compound arch, 10 high: the quarter circle of radius 5 about the origin from (5, 0) to
/// (0, 5) running on, tangent, into the quarter circle of radius 3 about (0, 2) to (-3, 2). The
/// skin splits it at the join: two curved faces, the pieces of one surface.
fn compound_arch() -> (Body, Order) {
    let section = |z: f64| {
        vec![
            quarter([5.0, 0.0], [5.0, 5.0], [0.0, 5.0], z),
            quarter([0.0, 5.0], [-3.0, 5.0], [-3.0, 2.0], z),
        ]
    };
    skin(&[section(0.0), section(10.0)])
}

/// The flat arch, in z = 0: the line from (10, 0) to (0, 0) skinned to the half circle about
/// (5, 5) of radius 5 from (10, 5) over (5, 10) to (0, 5). The skin splits the wall at the half
/// circle's middle: two plane faces of one plane, their seam x = 5.
fn flat_arch() -> (Body, Order) {
    skin(&[
        vec![line([10.0, 0.0, 0.0], [0.0, 0.0, 0.0])],
        vec![half_circle([5.0, 5.0, 0.0], 5.0)],
    ])
}

/// The arch on legs, 10 high: the leg from (5, -5) up to (5, 0), the half circle of radius 5
/// about the origin over to (-5, 0), the leg down to (-5, -5). The skin makes the legs planes
/// and the top a B-spline: three faces, two seams.
fn legs_arch() -> (Body, Order) {
    let section = |z: f64| {
        vec![
            line([5.0, -5.0, z], [5.0, 0.0, z]),
            half_circle([0.0, 0.0, z], 5.0),
            line([-5.0, 0.0, z], [-5.0, -5.0, z]),
        ]
    };
    skin(&[section(0.0), section(10.0)])
}

#[test]
fn one_face_is_extended_as_that_face() {
    let (sheet, order) = flat();
    assert_eq!(sheet.counts().unwrap()[0], 1);
    let bottom = edge_through(&sheet, &order, [4.5, 0.0, 0.0]);
    let out = extend_sheet(&sheet, &order, bottom, DISTANCE, Continuation::Natural).unwrap();
    assert_eq!(out.counts().unwrap(), [1, 4, 4]);
    for p in [
        [0.0, -5.0, 0.0],
        [9.0, -5.0, 0.0],
        [0.0, 8.0, 0.0],
        [9.0, 8.0, 0.0],
    ] {
        assert!(has_vertex(&out, p), "{p:?}");
    }
}

#[test]
fn pieces_of_one_surface_are_extended_as_that_surface() {
    let (sheet, order) = compound_arch();
    assert_eq!(sheet.counts().unwrap(), [2, 7, 6]);
    let top = edge_through(&sheet, &order, [5.0 * W, 5.0 * W, 10.0]);
    let out = extend_sheet(&sheet, &order, top, DISTANCE, Continuation::Natural).unwrap();
    assert_eq!(out.counts().unwrap(), [2, 7, 6]);
    for p in [[5.0, 0.0], [0.0, 5.0], [-3.0, 2.0]] {
        assert!(has_vertex(&out, [p[0], p[1], 15.0]), "{p:?}");
        assert!(!has_vertex(&out, [p[0], p[1], 10.0]), "{p:?}");
    }
}

/// The flat arch's two plane faces merge into one plane face, whose trim grows: its bottom
/// (picked on either half) moves 5 outward, the straight sides x = 0 and x = 10 carried along
/// to meet it. One face again, of four edges, 10 by 5 more of area.
#[test]
fn coplanar_faces_are_extended_as_one_plane_face() {
    for through in [[2.5, 0.0, 0.0], [7.5, 0.0, 0.0]] {
        let (sheet, order) = flat_arch();
        assert_eq!(sheet.counts().unwrap()[0], 2, "{through:?}");
        let (input, _) = sheet.to_record().unwrap();
        let bottom = edge_through(&sheet, &order, through);
        let out = extend_sheet(&sheet, &order, bottom, DISTANCE, Continuation::Natural)
            .unwrap_or_else(|e| panic!("{through:?}: {e:?}"));
        assert_eq!(out.counts().unwrap(), [1, 4, 4], "{through:?}");
        for p in [
            [0.0, -5.0, 0.0],
            [10.0, -5.0, 0.0],
            [0.0, 5.0, 0.0],
            [10.0, 5.0, 0.0],
        ] {
            assert!(has_vertex(&out, p), "{through:?}: {p:?}");
        }
        assert!(!has_vertex(&out, [0.0, 0.0, 0.0]), "{through:?}");
        let grown = out.area(1e-9).unwrap() - sheet.area(1e-9).unwrap();
        assert!((grown - 50.0).abs() < 1e-6, "{through:?}: grew {grown}");
        // the input is unchanged
        assert_eq!(sheet.to_record().unwrap().0, input, "{through:?}");
    }
}

/// The arch on legs extended 5 at its top, from the curved face's top edge or a leg's: the whole
/// top rises to z = 15, the legs stay planes, each seam one edge, the bottom where it was.
#[test]
fn a_curved_face_and_the_planes_beside_it_are_extended_as_one_side() {
    for through in [[5.0 * W, 5.0 * W, 10.0], [5.0, -2.5, 10.0]] {
        let (sheet, order) = legs_arch();
        assert_eq!(sheet.counts().unwrap(), [3, 10, 8], "{through:?}");
        let top = edge_through(&sheet, &order, through);
        let out = extend_sheet(&sheet, &order, top, DISTANCE, Continuation::Natural)
            .unwrap_or_else(|e| panic!("{through:?}: {e:?}"));
        assert_eq!(out.counts().unwrap(), [3, 10, 8], "{through:?}");
        for v in vertices(&out) {
            assert!(
                v[2].abs() < 1e-6 || (v[2] - 15.0).abs() < 1e-6,
                "{through:?}: {v:?}"
            );
        }
        let (_, out_order) = out.to_record().unwrap();
        for p in [
            [5.0, 0.0, 12.5],
            [-5.0, 0.0, 12.5],
            [5.0, -2.5, 15.0],
            [5.0 * W, 5.0 * W, 15.0],
            [-5.0, -2.5, 15.0],
        ] {
            edge_through(&out, &out_order, p);
        }
    }
}

/// Two plane walls at a right angle (a skin of an L) do not merge into one plane face, and a
/// leg's outer edge does not run on into the curved face: each refused with its reason.
#[test]
fn a_sheet_no_route_fits_is_refused_with_its_reason() {
    let (walls, order) = skin(&[
        vec![
            line([0.0, 0.0, 0.0], [5.0, 0.0, 0.0]),
            line([5.0, 0.0, 0.0], [5.0, 5.0, 0.0]),
        ],
        vec![
            line([0.0, 0.0, 10.0], [5.0, 0.0, 10.0]),
            line([5.0, 0.0, 10.0], [5.0, 5.0, 10.0]),
        ],
    ]);
    assert_eq!(walls.counts().unwrap()[0], 2);
    let top = edge_through(&walls, &order, [2.5, 0.0, 10.0]);
    assert_eq!(
        extend_sheet(&walls, &order, top, DISTANCE, Continuation::Natural).err(),
        Some(Error::Unsupported("plane faces that do not merge into one"))
    );

    let (arch, order) = legs_arch();
    let outer = edge_through(&arch, &order, [5.0, -5.0, 5.0]);
    assert!(matches!(
        extend_sheet(&arch, &order, outer, DISTANCE, Continuation::Natural),
        Err(Error::Unsupported(_))
    ));
}
