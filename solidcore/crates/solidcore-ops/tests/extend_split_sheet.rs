//! Extend on a sheet the skin split into faces. The skin splits its wall at every join between
//! two curves of a section, tangent or not (the kernel's `skin_sources` tests: a rounded square's
//! eight curves make eight walls). Here each section is a compound arch: a quarter circle of
//! radius 5 running on, tangent, into a quarter circle of radius 3, so the wall is two curved
//! faces, the pieces of one surface. (A straight leg would not do: the skin makes a flat wall a
//! plane, which is not a piece of the curved wall's surface.) Extending its top edge from either
//! face grows the whole top by the distance: the faces stay two, split where they were, and the
//! seam between them stays one edge. A sheet whose faces are not the pieces of one surface is
//! refused, as before.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::float_arithmetic
)]

use std::f64::consts::FRAC_1_SQRT_2;

use solidcore_ops::extend::{extend_sheet, Continuation};
use solidcore_ops::sweep::{loft, planar_faces, Curve};
use solidcore_ops::{Body, Error, Order};

/// The wall's height, and how far its top is extended.
const HEIGHT: f64 = 10.0;
const DISTANCE: f64 = 5.0;
/// `1 / sqrt 2` (ops' tests compute no roots).
const W: f64 = FRAC_1_SQRT_2;

/// A quarter circle at height `z`: one rational quadratic span from `p0` to `p2` about the
/// corner `p1` (weights 1, 1 / sqrt 2, 1).
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

/// The compound arch's section at height `z`: the quarter circle of radius 5 about the origin
/// from (5, 0) to (0, 5), then, tangent there (both run along -x), the quarter circle of radius 3
/// about (0, 2) from (0, 5) to (-3, 2). Their curvatures differ, so the join is a tangent joint
/// and no smoother, however the curves are parameterised.
fn section(z: f64) -> Vec<Curve> {
    vec![
        quarter([5.0, 0.0], [5.0, 5.0], [0.0, 5.0], z),
        quarter([0.0, 5.0], [-3.0, 5.0], [-3.0, 2.0], z),
    ]
}

/// The arch: the wall skinned between the sections at z = 0 and z = HEIGHT.
fn arch() -> (Body, Order) {
    let body = loft(&[section(0.0), section(HEIGHT)], 1, false).unwrap();
    let (_, order) = body.to_record().unwrap();
    (body, order)
}

/// A point of each top edge: the radius-5 quarter's at 45 degrees, the radius-3 quarter's at 135.
fn tops(z: f64) -> [[f64; 3]; 2] {
    [[5.0 * W, 5.0 * W, z], [-3.0 * W, 2.0 + 3.0 * W, z]]
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

/// Whether the record of `body` has a vertex within `1e-6` of `p`.
fn has_vertex(body: &Body, p: [f64; 3]) -> bool {
    let (r, _) = body.to_record().unwrap();
    r.vertices
        .iter()
        .any(|v| (0..3).all(|i| (v.point[i] - p[i]).abs() < 1e-6))
}

#[test]
fn the_arch_is_two_faces_split_at_its_join() {
    let (body, order) = arch();
    // two faces, their seam one edge: 2 bottom, 2 top, 2 ends and the seam; 6 vertices
    assert_eq!(body.counts().unwrap(), [2, 7, 6]);
    // the seam stands at the join (0, 5)
    edge_through(&body, &order, [0.0, 5.0, 0.5 * HEIGHT]);
}

#[test]
fn the_arch_top_grows_whole_from_either_face() {
    for through in tops(HEIGHT) {
        let (body, order) = arch();
        let (input, _) = body.to_record().unwrap();
        let edge = edge_through(&body, &order, through);
        let out = extend_sheet(&body, &order, edge, DISTANCE, Continuation::Natural)
            .unwrap_or_else(|e| panic!("the top through {through:?}: {e:?}"));
        // the same split: two faces, one seam, the seam grown with the top
        assert_eq!(out.counts().unwrap(), [2, 7, 6], "{through:?}");
        let (_, out_order) = out.to_record().unwrap();
        edge_through(&out, &out_order, [0.0, 5.0, HEIGHT + 0.5 * DISTANCE]);
        // the whole top moved up by the distance, the bottom where it was
        let top = HEIGHT + DISTANCE;
        for p in [[5.0, 0.0], [0.0, 5.0], [-3.0, 2.0]] {
            assert!(
                has_vertex(&out, [p[0], p[1], top]),
                "{through:?}: {p:?} at {top}"
            );
            assert!(
                has_vertex(&out, [p[0], p[1], 0.0]),
                "{through:?}: {p:?} at 0"
            );
            assert!(
                !has_vertex(&out, [p[0], p[1], HEIGHT]),
                "{through:?}: {p:?} at {HEIGHT}"
            );
        }
        for p in tops(top) {
            edge_through(&out, &out_order, p);
        }
        // the input is unchanged
        assert_eq!(body.to_record().unwrap().0, input, "{through:?}");
    }
}

#[test]
fn a_sheet_whose_faces_are_not_one_surfaces_pieces_is_refused() {
    // two squares apart, each a plane face
    let square = |x: f64| {
        let c = [[x, 0.0], [x + 4.0, 0.0], [x + 4.0, 4.0], [x, 4.0], [x, 0.0]];
        c.windows(2)
            .map(|w| Curve::Line {
                start: [w[0][0], w[0][1], 0.0],
                end: [w[1][0], w[1][1], 0.0],
            })
            .collect::<Vec<_>>()
    };
    let loops: Vec<Curve> = square(0.0).into_iter().chain(square(10.0)).collect();
    let body = planar_faces(&loops).unwrap();
    let (_, order) = body.to_record().unwrap();
    assert_eq!(body.counts().unwrap()[0], 2);
    let edge = edge_through(&body, &order, [2.0, 0.0, 0.0]);
    assert_eq!(
        extend_sheet(&body, &order, edge, DISTANCE, Continuation::Natural).err(),
        Some(Error::Unsupported("a sheet of more than one face"))
    );
}
