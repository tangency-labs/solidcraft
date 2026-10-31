//! Regression test for WR-591 (the fork's tangency-labs/smlib@9209e76, capi test
//! `ops_kernel` `loops_touching_at_a_vertex_in_any_order`): a 10 x 6 outline and a
//! triangular hole whose corner touches it at (0, 3), as eight lines, ordered into loops
//! by `OrderCurvesIntoLoops` from every rotation of the list, with the outline or the
//! hole reversed (24 orders). Four curve ends meet at (0, 3). Upstream goes on from the
//! first of them by index, so the order decides whether the loops close; with
//! `loops_go_on_round_the_vertex` every order gives closed loops of areas {60, 1} (the
//! outline and the hole apart) or {59} (one loop through the corner twice), never the
//! hole run the wrong way round inside the outline.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::curve::curve::Curve;
use solidcore::nurbs::b_spline_surface_n_lib::p2::OrientType;
use solidcore::primitive::api_curves::create_line_segment;
use solidcore::topology::trimming_tools::order_curves_into_loops;
use solidcore::{Kernel, Point3};

const OUTLINE: [[f64; 2]; 5] = [[0.0, 0.0], [10.0, 0.0], [10.0, 6.0], [0.0, 6.0], [0.0, 3.0]];
const HOLE: [[f64; 2]; 3] = [[0.0, 3.0], [2.0, 3.5], [2.0, 2.5]];

fn p(q: [f64; 2]) -> Point3 {
    Point3::new(q[0], q[1], 0.0)
}

/// The lines of a closed polygon, each reversed (and in reverse order of travel)
/// when `reversed`.
fn polygon(points: &[[f64; 2]], reversed: bool) -> Vec<(Point3, Point3)> {
    let n = points.len();
    (0..n)
        .map(|i| {
            let (a, b) = (p(points[i]), p(points[(i + 1) % n]));
            if reversed {
                (b, a)
            } else {
                (a, b)
            }
        })
        .collect()
}

/// The 24 input orders: the variant (none, outline or hole reversed) and the list
/// rotated by `turn`.
fn orders() -> Vec<(usize, usize, Vec<Curve>)> {
    let mut out = Vec::new();
    for variant in 0..3 {
        let mut lines = polygon(&OUTLINE, variant == 1);
        lines.extend(polygon(&HOLE, variant == 2));
        let n = lines.len();
        for turn in 0..n {
            let curves = (0..n)
                .map(|k| {
                    let (a, b) = lines[(k + turn) % n];
                    Curve::from(create_line_segment(a, b).unwrap())
                })
                .collect();
            out.push((variant, turn, curves));
        }
    }
    out
}

/// The loops' absolute areas (sorted), or `None` when the ordering failed or a loop
/// does not run end to end and close.
fn loop_areas(k: &Kernel, curves: &[Curve]) -> Option<Vec<f64>> {
    let r = order_curves_into_loops(k, curves, 1e-6).ok()?;
    r.status().ok()?;
    if r.ordered.len() != curves.len() {
        return None;
    }
    let ends = |i: usize| {
        let e = curves[r.ordered[i]].ends(k).unwrap();
        if r.orients[i] == OrientType::Same {
            (e.start, e.end)
        } else {
            (e.end, e.start)
        }
    };
    let mut areas = Vec::new();
    let mut first = 0;
    for &count in &r.loop_counts {
        let mut area = 0.0;
        let mut end = Point3::new(0.0, 0.0, 0.0);
        for i in first..first + count {
            let (a, b) = ends(i);
            if i != first && a.distance(end) >= 1e-12 {
                return None;
            }
            area += 0.5 * (a.x * b.y - b.x * a.y);
            end = b;
        }
        if end.distance(ends(first).0) >= 1e-12 {
            return None;
        }
        areas.push(f64::abs(area));
        first += count;
    }
    areas.sort_by(f64::total_cmp);
    Some(areas)
}

fn sound(areas: &[f64]) -> bool {
    let apart =
        areas.len() == 2 && (areas[0] - 1.0).abs() < 1e-12 && (areas[1] - 60.0).abs() < 1e-12;
    let joined = areas.len() == 1 && (areas[0] - 59.0).abs() < 1e-12;
    apart || joined
}

#[test]
fn wr_591_on_every_order_closes_round_the_touching_vertex() {
    let mut k = Kernel::default();
    k.fixes.loops_go_on_round_the_vertex = true;
    for (variant, turn, curves) in orders() {
        let areas = loop_areas(&k, &curves)
            .unwrap_or_else(|| panic!("variant {variant}, turn {turn}: no closed loops"));
        assert!(
            sound(&areas),
            "variant {variant}, turn {turn}: areas {areas:?}"
        );
    }
}

#[test]
fn wr_591_off_the_input_order_decides() {
    let k = Kernel::default();
    let mut bad = Vec::new();
    for (variant, turn, curves) in orders() {
        match loop_areas(&k, &curves) {
            Some(areas) if sound(&areas) => {}
            other => bad.push((variant, turn, other)),
        }
    }
    eprintln!("upstream order failures: {bad:?}");
    assert_eq!(bad.len(), 15, "upstream order failures: {bad:?}");
}

/// Loops that touch nowhere are ordered the same with the fix on and off.
#[test]
fn wr_591_loops_apart_are_unchanged() {
    let mut lines = polygon(&OUTLINE, false);
    lines.extend(polygon(&[[3.0, 2.0], [5.0, 2.0], [4.0, 4.0]], true));
    let curves: Vec<Curve> = lines
        .iter()
        .map(|&(a, b)| Curve::from(create_line_segment(a, b).unwrap()))
        .collect();
    let off = order_curves_into_loops(&Kernel::default(), &curves, 1e-6).unwrap();
    let mut k = Kernel::default();
    k.fixes.loops_go_on_round_the_vertex = true;
    let on = order_curves_into_loops(&k, &curves, 1e-6).unwrap();
    assert_eq!(on.ordered, off.ordered);
    assert_eq!(on.loop_counts, off.loop_counts);
    assert_eq!(on.orients, off.orients);
    assert!(on.closed && off.closed);
}

/// The 32 input orders of two unit squares touching at the corner (1, 1) (the fork's
/// tangency-labs/smlib@b0ee3f9, capi test `loops_touching_from_outside_close_apart`):
/// both counterclockwise or both clockwise, either square's lines first, every
/// rotation of the list.
fn squares_orders() -> Vec<(bool, bool, usize, Vec<Curve>)> {
    const LOW: [[f64; 2]; 4] = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
    const HIGH: [[f64; 2]; 4] = [[1.0, 1.0], [2.0, 1.0], [2.0, 2.0], [1.0, 2.0]];
    let mut out = Vec::new();
    for clockwise in [false, true] {
        for high_first in [false, true] {
            let (a, b) = if high_first { (HIGH, LOW) } else { (LOW, HIGH) };
            let mut lines = polygon(&a, clockwise);
            lines.extend(polygon(&b, clockwise));
            let n = lines.len();
            for turn in 0..n {
                let curves = (0..n)
                    .map(|k| {
                        let (a, b) = lines[(k + turn) % n];
                        Curve::from(create_line_segment(a, b).unwrap())
                    })
                    .collect();
                out.push((clockwise, high_first, turn, curves));
            }
        }
    }
    out
}

/// WR-591 addendum (fork b0ee3f9): loops touching at a vertex from outside each
/// other close apart, two loops of area 1, in every order. Turning about a normal
/// taken from the rays at the vertex in list order gave the figure-eight (one loop
/// of area 2) in 12 of the 32 orders.
#[test]
fn wr_591_loops_touching_from_outside_close_apart() {
    let mut k = Kernel::default();
    k.fixes.loops_go_on_round_the_vertex = true;
    let mut bad = Vec::new();
    for (clockwise, high_first, turn, curves) in squares_orders() {
        match loop_areas(&k, &curves) {
            Some(areas) if areas.len() == 2 && areas.iter().all(|a| (a - 1.0).abs() < 1e-12) => {}
            other => bad.push((clockwise, high_first, turn, other)),
        }
    }
    assert!(bad.is_empty(), "{} of 32 orders: {bad:?}", bad.len());
}
