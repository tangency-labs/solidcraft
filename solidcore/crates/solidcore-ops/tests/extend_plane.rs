//! Extend on a skin whose face is a plane: a loft of two coplanar sections (degree 1, open) is a
//! plane face since the skin makes flat walls planes, trimmed by its edges. Extending an edge by
//! 5 mm grows the trim within the plane: the picked edge moves 5 mm outward (a curved one is
//! offset by 5 mm), its neighbours carried along their end tangents to meet it. A rectangle, a
//! trapezoid, and a sheet with one arc edge; a neighbour that leaves its corner along the picked
//! edge's own line cannot meet the moved edge and is refused.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::float_arithmetic
)]

use std::f64::consts::{FRAC_1_SQRT_2, FRAC_PI_2};

use solidcore_ops::extend::{extend_sheet, Continuation};
use solidcore_ops::sweep::{loft, Curve};
use solidcore_ops::{Body, Error, Order};
use solidcore_record::brep::{Brep, Surface};

fn line(start: [f64; 3], end: [f64; 3]) -> Curve {
    Curve::Line { start, end }
}

/// sqrt 3, 5 and 10 (ops' tests compute no roots).
const SQRT_2: f64 = std::f64::consts::SQRT_2;
/// `sqrt(50 + 50 sqrt 2)`: where x = 0 and x = 10 meet the circle about (5, 0) of radius
/// `5 sqrt 2 + 5` (the arched sheet's arc moved out 5).
const ARCH_TOP: f64 = 10.986_841_134_678_1;
/// `5 sqrt 2 + 5`: that circle's radius, its top at `(5, ARCH_RADIUS)`.
const ARCH_RADIUS: f64 = 12.071_067_811_865_476;
const SQRT_5: f64 = 2.236_067_977_499_79;
const SQRT_10: f64 = 3.162_277_660_168_379_5;

/// The skin between sections `a` and `b`, at z = 0.
fn skin(a: Curve, b: Curve) -> (Body, Order) {
    let body = loft(&[vec![a], vec![b]], 1, false).unwrap();
    let (_, order) = body.to_record().unwrap();
    (body, order)
}

/// The skin between y = 0 and y = 8, x from 0 to 9, at z = 0.
fn flat_skin() -> (Body, Order) {
    skin(
        line([0.0, 0.0, 0.0], [9.0, 0.0, 0.0]),
        line([0.0, 8.0, 0.0], [9.0, 8.0, 0.0]),
    )
}

/// The trapezoid between (0, 0)-(10, 0) and (2, 6)-(7, 6): its sides lean in at different
/// angles.
fn trapezoid() -> (Body, Order) {
    skin(
        line([0.0, 0.0, 0.0], [10.0, 0.0, 0.0]),
        line([2.0, 6.0, 0.0], [7.0, 6.0, 0.0]),
    )
}

/// The sheet between (0, 0)-(10, 0) and the quarter circle about (5, 0) of radius 5 sqrt 2 from
/// (0, 5) over (5, 5 sqrt 2) to (10, 5): one arc edge, its sides x = 0 and x = 10 from y = 0 to
/// 5. (A quarter circle is one rational Bezier span. The half circle this sheet first had is two
/// spans meeting at a double knot, and the skin made its wall two faces: CI's "a sheet of more
/// than one face".)
fn arched() -> (Body, Order) {
    skin(
        line([0.0, 0.0, 0.0], [10.0, 0.0, 0.0]),
        Curve::Arc {
            center: [5.0, 0.0, 0.0],
            axis: [0.0, 0.0, -1.0],
            reference: [-FRAC_1_SQRT_2, FRAC_1_SQRT_2, 0.0],
            radius: 5.0 * SQRT_2,
            start: 0.0,
            end: FRAC_PI_2,
        },
    )
}

/// The sheet between (0, 0)-(5, 0) and the quarter circle about (5, 5) of radius 5 from (0, 5)
/// up to (5, 10): the arc leaves (0, 5) along +y, the side x = 0's own line.
fn quarter() -> (Body, Order) {
    skin(
        line([0.0, 0.0, 0.0], [5.0, 0.0, 0.0]),
        Curve::Arc {
            center: [5.0, 5.0, 0.0],
            axis: [0.0, 0.0, -1.0],
            reference: [-1.0, 0.0, 0.0],
            radius: 5.0,
            start: 0.0,
            end: FRAC_PI_2,
        },
    )
}

/// Each `(a point the picked edge runs through, the box the face's vertices should grow to)`
/// of `sheet`, extended by 5 mm Natural, is met: the face stays a plane.
fn grows(name: &str, (body, order): (Body, Order), cases: &[([f64; 3], [[f64; 3]; 2])]) {
    let (input, _) = body.to_record().unwrap();
    for &(through, want) in cases {
        let edge = edge_through(&body, &order, through);
        let out = extend_sheet(&body, &order, edge, 5.0, Continuation::Natural)
            .unwrap_or_else(|e| panic!("{name}, the edge through {through:?}: {e:?}"));
        let (r, _) = out.to_record().unwrap();
        assert_eq!(r.faces.len(), 1, "{name}");
        assert!(
            matches!(r.faces[0].surface, Surface::Plane { .. }),
            "{name}, the edge through {through:?}: {:?}",
            r.faces[0].surface
        );
        let got = bounds(&r);
        assert!(
            near(got, want),
            "{name}, the edge through {through:?}: {got:?}, not {want:?}"
        );
    }
    // the input is unchanged
    assert_eq!(body.to_record().unwrap().0, input, "{name}");
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

/// The box of the record's vertices: `[min, max]`.
fn bounds(r: &Brep) -> [[f64; 3]; 2] {
    let mut b = [[f64::INFINITY; 3], [f64::NEG_INFINITY; 3]];
    for v in &r.vertices {
        for (i, &x) in v.point.iter().enumerate() {
            b[0][i] = b[0][i].min(x);
            b[1][i] = b[1][i].max(x);
        }
    }
    b
}

fn near(a: [[f64; 3]; 2], b: [[f64; 3]; 2]) -> bool {
    (0..2).all(|j| (0..3).all(|i| (a[j][i] - b[j][i]).abs() < 1e-9))
}

#[test]
fn the_flat_skin_is_a_plane_face() {
    let (body, _) = flat_skin();
    let (r, _) = body.to_record().unwrap();
    assert_eq!(r.faces.len(), 1);
    assert!(
        matches!(r.faces[0].surface, Surface::Plane { .. }),
        "{:?}",
        r.faces[0].surface
    );
}

#[test]
fn each_edge_of_a_flat_skin_extends_by_five_mm_on_its_side() {
    grows(
        "the rectangle",
        flat_skin(),
        &[
            ([4.5, 0.0, 0.0], [[0.0, -5.0, 0.0], [9.0, 8.0, 0.0]]),
            ([4.5, 8.0, 0.0], [[0.0, 0.0, 0.0], [9.0, 13.0, 0.0]]),
            ([0.0, 4.0, 0.0], [[-5.0, 0.0, 0.0], [9.0, 8.0, 0.0]]),
            ([9.0, 4.0, 0.0], [[0.0, 0.0, 0.0], [14.0, 8.0, 0.0]]),
        ],
    );
}

/// The trapezoid's sides lean in, so they carry the moved edge's ends along them: the top
/// edge at y = 11 runs from (2 + 2 * 5/6, 11) to (7 - 3 * 5/6, 11), the bottom at y = -5 from
/// (-2 * 5/6, -5) to (10 + 3 * 5/6, -5); a side moved 5 mm out along its normal meets the top
/// and bottom lines continued.
#[test]
fn each_edge_of_a_trapezoid_extends_with_its_neighbours_carried_along() {
    grows(
        "the trapezoid",
        trapezoid(),
        &[
            // bottom: the sides continued down to y = -5
            ([5.0, 0.0, 0.0], [[-5.0 / 3.0, -5.0, 0.0], [12.5, 6.0, 0.0]]),
            // top: the sides continued up to y = 11 (they lean in, the edge gets shorter)
            ([4.5, 6.0, 0.0], [[0.0, 0.0, 0.0], [10.0, 11.0, 0.0]]),
            // left side (0, 0)-(2, 6), out along (-3, 1) / sqrt 10: meets y = 0 at
            // x = -5 sqrt 10 / 3, y = 6 at x = (6 - 5 sqrt 10) / 3
            (
                [1.0, 3.0, 0.0],
                [[-5.0 * SQRT_10 / 3.0, 0.0, 0.0], [10.0, 6.0, 0.0]],
            ),
            // right side (10, 0)-(7, 6), out along (2, 1) / sqrt 5: meets y = 0 at
            // x = 10 + 5 sqrt 5 / 2
            (
                [8.5, 3.0, 0.0],
                [[0.0, 0.0, 0.0], [10.0 + 5.0 * SQRT_5 / 2.0, 6.0, 0.0]],
            ),
        ],
    );
}

/// The arc edge is offset 5 mm out (the circle about (5, 0) of radius 5 sqrt 2 + 5), the sides
/// carried up along x = 0 and x = 10 to meet it at y = sqrt(50 + 50 sqrt 2); the bottom edge
/// moves down with the sides carried along.
#[test]
fn a_curved_edge_is_offset_and_its_neighbours_meet_it() {
    let apex = [5.0, 5.0 * SQRT_2, 0.0];
    grows(
        "the arched sheet",
        arched(),
        &[
            (apex, [[0.0, 0.0, 0.0], [10.0, ARCH_TOP, 0.0]]),
            ([5.0, 0.0, 0.0], [[0.0, -5.0, 0.0], [10.0, 5.0, 0.0]]),
        ],
    );
    // the moved arc is that circle: its top, (5, 5 sqrt 2 + 5), lies on an edge of the result
    // (whatever kind the record gives its curve)
    let (body, order) = arched();
    let edge = edge_through(&body, &order, apex);
    let out = extend_sheet(&body, &order, edge, 5.0, Continuation::Natural).unwrap();
    let (_, out_order) = out.to_record().unwrap();
    edge_through(&out, &out_order, [5.0, ARCH_RADIUS, 0.0]);
}

/// The quarter sheet's side x = 0: the arc leaves its corner (0, 5) along that side's own line,
/// so carried along its end tangent it never meets the side moved out: refused, not guessed.
#[test]
fn a_neighbour_along_the_picked_edge_s_line_is_refused() {
    let (body, order) = quarter();
    let edge = edge_through(&body, &order, [0.0, 2.5, 0.0]);
    assert!(matches!(
        extend_sheet(&body, &order, edge, 5.0, Continuation::Natural),
        Err(Error::Unsupported(_))
    ));
}

/// On a plane every continuation is the plane itself: Tangent and Curvature give Natural's sheet.
#[test]
fn every_continuation_of_a_plane_is_the_same() {
    let (body, order) = trapezoid();
    let edge = edge_through(&body, &order, [4.5, 6.0, 0.0]);
    let record = |how| {
        extend_sheet(&body, &order, edge, 5.0, how)
            .unwrap()
            .to_record()
            .unwrap()
            .0
    };
    let natural = record(Continuation::Natural);
    assert_eq!(record(Continuation::Tangent), natural);
    assert_eq!(record(Continuation::Curvature), natural);
}
