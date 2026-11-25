//! Tangent rollover: a concave round whose contact line reaches a smooth edge of the face it
//! rolls on, and rolls on across it onto the face beyond.
//!
//! Also a convex round that, past a radius, rolls on its edge's faces only past an end of the
//! edge (SolidCraft's fuzz `atomic_commands` seed 2, body `b27`): refused, naming the radius at
//! which it first has a section between the edge's ends, and that radius builds.
//!
//! The body is an L step swept 10 along z, its section in the xy plane: a wall x = 0 standing on
//! a strip y = 0 that is `w` wide, then a cliff x = w down to y = -5. The cliff's rim (w, 0) is
//! rounded `r1` first, which leaves the strip flat for `a = w - r1` and runs it on tangentially
//! into the rim's round. The wall's foot (0, 0) is then rounded `r`: past `a`, the ball rolls off
//! the flat onto the rim's round, the whole length of the edge, and the strip is consumed.

#![allow(clippy::disallowed_methods)] // closed-form oracle, not ops geometry
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test

#[path = "../../solidcore/tests/support/solidcraft_fixes.rs"]
mod solidcraft_fixes;

mod common;

use std::f64::consts::PI;

use common::edge_at;
use solidcore_ops::blend::{blend, Blend, Corners};
use solidcore_ops::boolean::boolean;
use solidcore_ops::{
    blend_with, chamfer_sides_facing, make_box, session, BlendOptions, Body, BooleanOp,
    ChamferSecond, CheckLevel, Contract, Error, RoomReason,
};

const LENGTH: f64 = 10.0;

/// SolidCraft's fix set (`solidcraft_fixes.rs`, the set SolidCraft runs).
fn fixes() {
    session::set_thread_fixes(solidcraft_fixes::solidcraft_fix_names()).unwrap();
}

/// The L step with a strip `w` wide.
fn step(w: f64) -> Body {
    let base = make_box([-10.0, -5.0, 0.0], [10.0 + w, 5.0, LENGTH]).unwrap();
    let wall = make_box([-10.0, 0.0, 0.0], [10.0, 10.0, LENGTH]).unwrap();
    boolean(base, wall, BooleanOp::Union).unwrap().unwrap()
}

/// The section area a concave round `r` at the wall's foot adds, the strip flat for `a` and then
/// a convex round of radius `r1` (centre (a, -r1)): the ball's quarter-square spandrel while it
/// stays on the flat; past it, the region bounded by the wall, the flat, the rim's round up to the
/// ball's contact and the ball (its centre (r, c), (r - a)^2 + (c + r1)^2 = (r + r1)^2), by
/// Green's theorem over its arcs.
fn added_area(a: f64, r1: f64, r: f64) -> f64 {
    if r <= a {
        return r * r * (1.0 - PI / 4.0);
    }
    let c = ((r + r1).powi(2) - (r - a).powi(2)).sqrt() - r1;
    // the contact on the rim's round, at angle t1 about its centre
    let t1 = (c + r1).atan2(r - a);
    // the ball's contact with the round, at angle s0 about the ball's centre
    let s0 = t1 - PI;
    // ∮ (x dy - y dx) / 2: segments wall (0,c)->(0,0) and flat (0,0)->(a,0) give nothing (x = 0,
    // y = 0); the arcs give r^2 dt/2 + (centre x sin-ish terms)
    let arc = |cx: f64, cy: f64, rad: f64, from: f64, to: f64| {
        // ∫ (x y' - y x') / 2 dt for x = cx + rad cos t, y = cy + rad sin t
        0.5 * (rad * rad * (to - from) + cx * rad * (to.sin() - from.sin())
            - cy * rad * (to.cos() - from.cos()))
    };
    // the boundary runs clockwise: wall down, flat out, round from pi/2 down to t1, ball from s0 to -pi
    let total = arc(a, -r1, r1, PI / 2.0, t1) + arc(r, c, r, s0, -PI);
    total.abs()
}

/// The volume after rounding the rim `r1` and the foot `r` of the step `w` wide, checked.
fn rounded(w: f64, r1: f64, r: f64) -> f64 {
    let b = step(w);
    if r1 > 0.0 {
        let e = edge_at(&b, [w, 0.0, LENGTH / 2.0]);
        blend(&b, Blend::Round, &[e], &[r1], &[], Corners::Filled).unwrap();
    }
    let e = edge_at(&b, [0.0, 0.0, LENGTH / 2.0]);
    blend(&b, Blend::Round, &[e], &[r], &[], Corners::Filled).unwrap();
    let report = b.check(CheckLevel::All).unwrap();
    assert!(report.is_valid(), "w {w} r1 {r1} r {r}: {report}");
    b.volume(1e-9).unwrap()
}

fn expected(w: f64, r1: f64, r: f64) -> f64 {
    let step = (10.0 + w) * 5.0 * LENGTH + 10.0 * 10.0 * LENGTH;
    step - LENGTH * r1 * r1 * (1.0 - PI / 4.0) + LENGTH * added_area(w - r1, r1, r)
}

#[test]
fn the_area_oracle_agrees_with_a_polygon() {
    // the closed form against a fine polygon of the same boundary
    for &(a, r1, r) in &[
        (1.0_f64, 2.0_f64, 1.5_f64),
        (1.0, 2.0, 2.0),
        (1.0, 2.0, 3.0),
        (0.5, 1.0, 2.0),
    ] {
        let c = ((r + r1).powi(2) - (r - a).powi(2)).sqrt() - r1;
        let t1 = (c + r1).atan2(r - a);
        let mut pts = vec![(0.0, c), (0.0, 0.0), (a, 0.0)];
        let n = 20000;
        for i in 1..=n {
            let t = PI / 2.0 + (t1 - PI / 2.0) * f64::from(i) / f64::from(n);
            pts.push((a + r1 * t.cos(), -r1 + r1 * t.sin()));
        }
        for i in 1..=n {
            let t = (t1 - PI) + (-PI - (t1 - PI)) * f64::from(i) / f64::from(n);
            pts.push((r + r * t.cos(), c + r * t.sin()));
        }
        let mut twice = 0.0;
        for (i, p) in pts.iter().enumerate() {
            let q = pts[(i + 1) % pts.len()];
            twice += p.0 * q.1 - q.0 * p.1;
        }
        let polygon = twice.abs() / 2.0;
        let closed = added_area(a, r1, r);
        assert!(
            (polygon - closed).abs() < 1e-6,
            "{a} {r1} {r}: {polygon} vs {closed}"
        );
    }
}

/// Controls: the round within the flat, and the round with no rim round, build exactly today.
#[test]
fn controls_build_exactly() {
    fixes();
    for &(w, r1, r) in &[(3.0, 2.0, 1.0), (3.0, 2.0, 0.5), (3.0, 0.0, 2.0)] {
        let v = rounded(w, r1, r);
        let e = expected(w, r1, r);
        assert!(
            (v - e).abs() < 1e-6,
            "w {w} r1 {r1} r {r}: {v}, expected {e}"
        );
    }
}

/// The rollover cases: strip width, rim radius, foot radius (flat left `w - r1` below `r`).
const ROLLOVERS: &[(f64, f64, f64)] = &[
    (3.0, 2.0, 1.1),
    (3.0, 2.0, 1.5),
    (3.0, 2.0, 2.0),
    (3.0, 2.0, 3.0),
    (2.0, 1.0, 2.0),
];

/// The ball rolls off the flat onto the rim's round along the whole edge: built, its volume the
/// closed form, across the threshold r = a and past the strip's whole width.
#[test]
fn full_length_rollover_builds() {
    fixes();
    for &(w, r1, r) in ROLLOVERS {
        let v = rounded(w, r1, r);
        let e = expected(w, r1, r);
        assert!(
            (v - e).abs() < 1e-6,
            "w {w} r1 {r1} r {r}: {v}, expected {e}"
        );
    }
}

/// A convex round on a block `w` deep along z: its top y = 0 runs from the back face x = 0 to
/// the front face x = w, whose rim (w, 0) is rounded `r1` first, so the top is flat for
/// `a = w - r1` and then runs on tangentially into the rim's round.
fn block(w: f64) -> Body {
    make_box([0.0, -10.0, 0.0], [w, 10.0, LENGTH]).unwrap()
}

/// The section area a convex round `r` on the back edge (0, 0) removes, the top flat for `a`
/// and then the rim's round of radius `r1` (centre (a, -r1)): the quarter-square spandrel while
/// the ball stays on the flat; past it, the region bounded by the back face, the flat, the rim's
/// round up to the ball's contact and the ball, which touches the rim's round from inside (its
/// centre (r, c), (r - a)^2 + (c + r1)^2 = (r1 - r)^2), by Green's theorem over its arcs.
fn removed_area(a: f64, r1: f64, r: f64) -> f64 {
    if r <= a {
        return r * r * (1.0 - PI / 4.0);
    }
    let c = ((r1 - r).powi(2) - (r - a).powi(2)).sqrt() - r1;
    // the contact, at angle t about both centres (an inside tangency)
    let t = (c + r1).atan2(r - a);
    let arc = |cx: f64, cy: f64, rad: f64, from: f64, to: f64| {
        0.5 * (rad * rad * (to - from) + cx * rad * (to.sin() - from.sin())
            - cy * rad * (to.cos() - from.cos()))
    };
    // back face and flat give nothing (x = 0, y = 0); the rim's round from pi/2 down to t, then
    // the ball from t up over its top to pi
    (arc(a, -r1, r1, PI / 2.0, t) + arc(r, c, r, t, PI)).abs()
}

/// The convex cases: block depth, rim radius, back-edge radius (flat `w - r1` below `r`, and the
/// ball inside the rim's round: `r <= (r1 + w - r1) / 2`).
const CONVEX_ROLLOVERS: &[(f64, f64, f64)] = &[
    (3.0, 2.0, 1.1),
    (3.0, 2.0, 1.3),
    (3.0, 2.0, 1.4),
    (4.0, 3.0, 1.5),
    (4.0, 3.0, 1.8),
];

#[test]
fn the_removed_area_oracle_agrees_with_a_polygon() {
    for &(w, r1, r) in CONVEX_ROLLOVERS {
        let a = w - r1;
        let c = ((r1 - r).powi(2) - (r - a).powi(2)).sqrt() - r1;
        let t = (c + r1).atan2(r - a);
        let mut pts = vec![(0.0, 0.0), (a, 0.0)];
        let n = 20000;
        for i in 1..n {
            let u = PI / 2.0 + (t - PI / 2.0) * f64::from(i) / f64::from(n);
            pts.push((a + r1 * u.cos(), -r1 + r1 * u.sin()));
        }
        for i in 0..=n {
            let u = t + (PI - t) * f64::from(i) / f64::from(n);
            pts.push((r + r * u.cos(), c + r * u.sin()));
        }
        let mut twice = 0.0;
        for (i, p) in pts.iter().enumerate() {
            let q = pts[(i + 1) % pts.len()];
            twice += p.0 * q.1 - q.0 * p.1;
        }
        let polygon = twice.abs() / 2.0;
        let closed = removed_area(a, r1, r);
        assert!(
            (polygon - closed).abs() < 1e-6,
            "{w} {r1} {r}: {polygon} vs {closed}"
        );
    }
}

/// The convex round's ball rolls off the top's flat onto the rim's round along the whole edge:
/// built and valid at the full check. Its faces are the closed form: the round is the ball's
/// cylinder from the contact over its top to the back face (area and centroid), the rim's round
/// keeps its band below the contact, and the back face ends at the ball's centre height. The
/// volume is the closed form within the end faces' trims: the integrator reads them, and they lie
/// within the body's tolerance of their edges (the edges themselves are exact), so it is graded
/// to that tolerance times the length and the end face's perimeter. The control within the flat
/// builds as today.
#[test]
fn full_length_convex_rollover_builds() {
    fixes();
    let tol = 1e-5;
    for &(w, r1, r) in CONVEX_ROLLOVERS {
        let a = w - r1;
        let b = block(w);
        let e = edge_at(&b, [w, 0.0, LENGTH / 2.0]);
        blend(&b, Blend::Round, &[e], &[r1], &[], Corners::Filled).unwrap();
        let e = edge_at(&b, [0.0, 0.0, LENGTH / 2.0]);
        blend(&b, Blend::Round, &[e], &[r], &[], Corners::Filled)
            .unwrap_or_else(|err| panic!("w {w} r1 {r1} r {r}: {err:?}"));
        let report = b.check(CheckLevel::All).unwrap();
        assert!(report.is_valid(), "w {w} r1 {r1} r {r}: {report}");
        let c = ((r1 - r).powi(2) - (r - a).powi(2)).sqrt() - r1;
        let t = (c + r1).atan2(r - a);
        let order = b.to_record().unwrap().1;
        let faces: Vec<_> = (0..order.faces.len())
            .map(|i| {
                let fc = b.face_centroid(&order, i, 1e-10).unwrap();
                (fc, b.face_plane_normal(&order, i).unwrap())
            })
            .collect();
        let curved: Vec<_> = faces
            .iter()
            .filter(|f| f.1.is_none())
            .map(|f| &f.0)
            .collect();
        assert_eq!(
            curved.len(),
            2,
            "w {w} r1 {r1} r {r}: the rim's round and the new round"
        );
        let (round, rim) = if curved[0].centroid[0] < curved[1].centroid[0] {
            (curved[0], curved[1])
        } else {
            (curved[1], curved[0])
        };
        let close = |x: f64, y: f64, what: &str| {
            assert!(
                (x - y).abs() < 1e-9,
                "w {w} r1 {r1} r {r}: {what} {x}, expected {y}"
            );
        };
        close(round.area, r * (PI - t) * LENGTH, "round's area");
        close(
            round.centroid[0],
            r - r * t.sin() / (PI - t),
            "round's centroid x",
        );
        close(
            round.centroid[1],
            c + r * (t.cos() + 1.0) / (PI - t),
            "round's centroid y",
        );
        close(rim.area, r1 * t * LENGTH, "rim round's area");
        let back = faces
            .iter()
            .find(|f| f.1.is_some_and(|n| n.value[0] < -0.5))
            .unwrap();
        close(back.0.area, (10.0 + c) * LENGTH, "back face's area");
        let v = b.volume(1e-9).unwrap();
        let e = w * 10.0 * LENGTH
            - LENGTH * r1 * r1 * (1.0 - PI / 4.0)
            - LENGTH * removed_area(a, r1, r);
        let perimeter = w + (10.0 - r1) + r1 * t + r * (PI - t) + (10.0 + c);
        assert!(
            (v - e).abs() < tol * LENGTH * perimeter,
            "w {w} r1 {r1} r {r}: {v}, expected {e}"
        );
    }
    let (w, r1, r) = (3.0, 2.0, 0.8);
    let b = block(w);
    let e = edge_at(&b, [w, 0.0, LENGTH / 2.0]);
    blend(&b, Blend::Round, &[e], &[r1], &[], Corners::Filled).unwrap();
    let e = edge_at(&b, [0.0, 0.0, LENGTH / 2.0]);
    blend(&b, Blend::Round, &[e], &[r], &[], Corners::Filled).unwrap();
    let v = b.volume(1e-9).unwrap();
    let e = w * 10.0 * LENGTH
        - LENGTH * r1 * r1 * (1.0 - PI / 4.0)
        - LENGTH * removed_area(w - r1, r1, r);
    assert!((v - e).abs() < 1e-6, "control: {v}, expected {e}");
}
/// A convex round larger than the convex round beyond the top's flat cannot roll onto it: its
/// ball, inside the material, would have to fit inside that round, and it does not (that round's
/// offset by the ball folds). The rollover declines it, and it is refused as without the
/// rollover: too large, fitting the flat the rim's round leaves (less the fit margin).
#[test]
fn a_convex_round_larger_than_the_round_beyond_is_not_rolled_over() {
    fixes();
    for &(w, r1, r) in &[(3.0, 1.0, 2.5), (3.0, 2.0, 2.4)] {
        let b = block(w);
        let e = edge_at(&b, [w, 0.0, LENGTH / 2.0]);
        blend(&b, Blend::Round, &[e], &[r1], &[], Corners::Filled).unwrap();
        let e = edge_at(&b, [0.0, 0.0, LENGTH / 2.0]);
        let flat = w - r1;
        match blend(&b, Blend::Round, &[e], &[r], &[], Corners::Filled) {
            Err(Error::TooLarge { size, .. }) => assert!(
                size < flat && size > flat - 0.02,
                "w {w} r1 {r1} r {r}: fit {size}, the flat {flat}"
            ),
            other => panic!("w {w} r1 {r1} r {r}: {other:?}"),
        }
    }
}

/// A concave round between a plane and a boss, past a face that runs on tangentially into the
/// boss (#15767, the round of SolidCraft #15641's test9 between its arm and its R18 boss). The
/// section, in the xy plane and swept 5 along z: a rectangle x -72..0, y -65..-40, joined below
/// by x -72..-36 down to y -90 and the bottom half of the circle of radius 18 about
/// B = (-54, -90). The side x = -36 is 25 long and runs into the circle tangentially at
/// (-36, -90). The corner C = (-36, -65), between the side and the face y = -65, is rounded
/// r > 25: the ball touches y = -65 and the boss, so its centre is at y = -65 - r and 18 + r from
/// B, x = -54 + sqrt((18 + r)^2 - (r - 25)^2) (for r = 28, -54 + sqrt(46^2 - 3^2)). Its contact
/// on the boss lies past the side's end, and the side is consumed: its whole rail rolls over the
/// tangent edge.
fn arm_and_boss() -> Body {
    use solidcore::curve::curve::Curve as KCurve;
    use solidcore::math::Point3;
    use solidcore::primitive::api_curves::{create_arc, create_line_segment};
    use solidcore::sweep::api_brep::create_linear_sweep;
    common::built(|k| {
        let p = |x: f64, y: f64| Point3::new(x, y, 0.0);
        let curves = [
            KCurve::from(create_line_segment(p(0.0, -65.0), p(0.0, -40.0))?),
            KCurve::from(create_line_segment(p(0.0, -40.0), p(-72.0, -40.0))?),
            KCurve::from(create_line_segment(p(-72.0, -40.0), p(-72.0, -90.0))?),
            KCurve::from(create_arc(p(-54.0, -90.0), 18.0, 180.0, 360.0)?),
            KCurve::from(create_line_segment(p(-36.0, -90.0), p(-36.0, -65.0))?),
            KCurve::from(create_line_segment(p(-36.0, -65.0), p(0.0, -65.0))?),
        ];
        let refs: Vec<Option<&KCurve>> = curves.iter().map(Some).collect();
        create_linear_sweep(k, &refs, solidcore::Vec3::new(0.0, 0.0, 1.0), 5.0, true)
    })
}

/// The section area the round `r` adds at C: bounded by the face y = -65 from C to the ball's
/// contact T1, the ball's arc to its contact T2 on the boss, the boss's arc from T2 to (-36, -90)
/// and the side back to C, by Green's theorem (`1/2 ∮ x dy - y dx`).
fn boss_round_area(r: f64) -> f64 {
    let (bx, by, rb) = (-54.0, -90.0, 18.0);
    let cy = -65.0 - r;
    let cx = bx + ((rb + r).powi(2) - (cy - by).powi(2)).sqrt();
    let line = |a: (f64, f64), b: (f64, f64)| 0.5 * (a.0 * b.1 - b.0 * a.1);
    let arc = |ox: f64, oy: f64, rad: f64, from: f64, to: f64| {
        0.5 * (rad * rad * (to - from) + ox * rad * (to.sin() - from.sin())
            - oy * rad * (to.cos() - from.cos()))
    };
    // T1 straight above the ball's centre, T2 towards B: the ball's arc from T1 (angle pi/2) on
    // to T2, then the boss's arc from T2 (seen from B, the opposite direction) to angle 0 at
    // (-36, -90)
    let toward_b = (by - cy).atan2(bx - cx);
    let from_b = toward_b - PI;
    let total = line((-36.0, -65.0), (cx, -65.0))
        + arc(cx, cy, r, PI / 2.0, toward_b)
        + arc(bx, by, rb, from_b, 0.0)
        + line((-36.0, -90.0), (-36.0, -65.0));
    total.abs()
}

#[test]
fn the_boss_round_area_oracle_agrees_with_a_polygon() {
    for r in [26.0_f64, 28.0, 30.0] {
        let cy = -65.0 - r;
        let cx = -54.0 + ((18.0 + r).powi(2) - (cy + 90.0).powi(2)).sqrt();
        let toward_b = (-90.0 - cy).atan2(-54.0 - cx);
        let from_b = toward_b - PI;
        let n = 20000;
        let mut pts = vec![(-36.0, -65.0)];
        for i in 0..=n {
            let t = PI / 2.0 + (toward_b - PI / 2.0) * f64::from(i) / f64::from(n);
            pts.push((cx + r * t.cos(), cy + r * t.sin()));
        }
        for i in 1..=n {
            let t = from_b - from_b * f64::from(i) / f64::from(n);
            pts.push((-54.0 + 18.0 * t.cos(), -90.0 + 18.0 * t.sin()));
        }
        let mut twice = 0.0;
        for (i, p) in pts.iter().enumerate() {
            let q = pts[(i + 1) % pts.len()];
            twice += p.0 * q.1 - q.0 * p.1;
        }
        let polygon = twice.abs() / 2.0;
        let closed = boss_round_area(r);
        assert!(
            (polygon - closed).abs() < 1e-5,
            "{r}: {polygon} vs {closed}"
        );
    }
}

/// The round `r` at C on the arm and boss: built, valid at the full check, its rails the straight
/// lines along z through the ball's contacts on y = -65 and on the boss (the latter past the
/// side's end), and its volume the closed form.
fn assert_boss_round_builds(r: f64) {
    use solidcore::query::api_queries::edge_closest_point;
    let height = 5.0;
    let profile = 72.0 * 25.0 + 36.0 * 25.0 + PI * 18.0 * 18.0 / 2.0;
    let b = arm_and_boss();
    let e = edge_at(&b, [-36.0, -65.0, height / 2.0]);
    blend(&b, Blend::Round, &[e], &[r], &[], Corners::Filled)
        .unwrap_or_else(|err| panic!("r {r}: {err:?}"));
    let report = b.check(CheckLevel::All).unwrap();
    assert!(report.is_valid(), "r {r}: {report}");
    // the rails: straight lines along z through the ball's contacts
    let cy = -65.0 - r;
    let cx = -54.0 + ((18.0 + r).powi(2) - (cy + 90.0).powi(2)).sqrt();
    let d = ((cx + 54.0).powi(2) + (cy + 90.0).powi(2)).sqrt();
    let t2 = (
        -54.0 + 18.0 * (cx + 54.0) / d,
        -90.0 + 18.0 * (cy + 90.0) / d,
    );
    assert!(
        t2.1 < -90.0,
        "r {r}: the contact on the boss lies past the side's end"
    );
    let (k, brep) = common::kernel_copy(&b);
    for (x, y) in [(cx, -65.0), t2] {
        for z in [0.5, 2.5, 4.5] {
            let q = solidcore::Vec3::new(x, y, z);
            let on_edge = brep.edge_list().iter().any(|&e| {
                edge_closest_point(&k, &brep, e, q)
                    .unwrap()
                    .closest
                    .distance
                    < 1e-7
            });
            assert!(on_edge, "r {r}: no rail through ({x}, {y}, {z})");
        }
    }
    let v = b.volume(1e-9).unwrap();
    let expected = height * (profile + boss_round_area(r));
    assert!(
        (v - expected).abs() < 1e-6,
        "r {r}: {v}, expected {expected}"
    );
}

#[test]
fn a_round_past_a_side_running_into_a_boss_rolls_onto_the_boss() {
    fixes();
    let height = 5.0;
    for r in [26.0, 28.0, 30.0] {
        assert_boss_round_builds(r);
    }
    // Past r = 3217 / 86 (37.41) the ball's contact on y = -65 lies past that face's end at
    // x = 0 ((18 + r)^2 - (r - 25)^2 = 86 r - 301 > 54^2): no ball fits, and it is refused as a
    // rail leaving its face.
    for r in [38.0_f64, 40.0] {
        let cx = -54.0 + ((18.0 + r).powi(2) - (r - 25.0).powi(2)).sqrt();
        assert!(cx > 0.0, "r {r}: the contact {cx} lies past x = 0");
        let b = arm_and_boss();
        let e = edge_at(&b, [-36.0, -65.0, height / 2.0]);
        match blend(&b, Blend::Round, &[e], &[r], &[], Corners::Filled) {
            Err(Error::TooLarge {
                why: Some(RoomReason::RailLeavesFace),
                ..
            }) => {}
            other => panic!("r {r}: {other:?}"),
        }
    }
}

/// The arm and boss with the boss standing (#15767 case 2, SolidCraft #15641's test9): the arm of
/// [`arm_and_boss`] joined with the boss's cylinder, radius 18 about B = (-54, -90) and 20 tall,
/// whose foot on the arm's top z = 5 is rounded 5 (the foot round: the torus of tube radius 5
/// about the circle of radius 23 at z = 10 about B's axis, which touches the top along the
/// contact circle of radius 23 at z = 5), then the arm's inner corner C = (-36, -65) rounded `r4`
/// (the cylinder of radius `r4` about A = (-36 + r4, -65 - r4)). The arm's top then ends on
/// x = -36 at the contact circle, at W = (-36, -90 + sqrt(205), 5), and the side x = -36 runs on
/// past W into the foot round tangentially.
fn arm_boss_and_foot_round(r4: f64) -> Body {
    let boss = solidcore_ops::make_cylinder([-54.0, -90.0, 0.0], 18.0, 20.0).unwrap();
    let b = boolean(arm_and_boss(), boss, BooleanOp::Union)
        .unwrap()
        .unwrap();
    let foot = edge_at(&b, [-54.0, -72.0, 5.0]);
    blend(&b, Blend::Round, &[foot], &[5.0], &[], Corners::Filled).unwrap();
    let corner = edge_at(&b, [-36.0, -65.0, 2.5]);
    blend(&b, Blend::Round, &[corner], &[r4], &[], Corners::Filled).unwrap();
    b
}

/// W's y: the arm's top ends on x = -36 where it meets the contact circle, 18 from B's axis in x.
fn top_end_on_side() -> f64 {
    -90.0 + 205.0_f64.sqrt()
}

/// The rim of the arm's top on [`arm_boss_and_foot_round`], a smooth chain: its edges on y = -65,
/// on the corner's round and on x = -36 (from the corner's round to W), then on from W the edge
/// between x = -36 and the foot round, down to (-36, -90, 10). That last one is a plane-torus
/// intersection, a spline fitted to it, found by its point halfway round the tube (23 - 5 / sqrt 2
/// from B's axis) within its fit ([`edge_near`]).
fn top_rim(b: &Body, r4: f64) -> Vec<usize> {
    use std::f64::consts::FRAC_1_SQRT_2;
    let rho = 23.0 - 5.0 * FRAC_1_SQRT_2;
    [
        [-15.5, -65.0, 5.0],
        [
            -36.0 + r4 - r4 * FRAC_1_SQRT_2,
            -65.0 - r4 + r4 * FRAC_1_SQRT_2,
            5.0,
        ],
        [-36.0, (-65.0 - r4 + top_end_on_side()) / 2.0, 5.0],
        [
            -36.0,
            -90.0 + (rho * rho - 18.0 * 18.0).sqrt(),
            10.0 - 5.0 * FRAC_1_SQRT_2,
        ],
    ]
    .iter()
    .map(|&p| edge_near(b, p))
    .collect()
}

/// The ball of radius `d` with centre at (`x`, `y`) in plan that touches the foot round (the
/// torus of tube radius 5 about the circle of radius 23 at z = 10 about B): its centre and its
/// contact on the torus.
fn ball_on_foot_round(x: f64, y: f64, d: f64) -> ([f64; 3], [f64; 3]) {
    let (bx, by) = (-54.0, -90.0);
    let rho = ((x - bx).powi(2) + (y - by).powi(2)).sqrt();
    let reach = 5.0 + d;
    let z = 10.0 - (reach * reach - (23.0 - rho).powi(2)).sqrt();
    let q = [bx + 23.0 * (x - bx) / rho, by + 23.0 * (y - by) / rho, 10.0];
    let c = [x, y, z];
    let t = [
        q[0] + 5.0 / reach * (c[0] - q[0]),
        q[1] + 5.0 / reach * (c[1] - q[1]),
        q[2] + 5.0 / reach * (c[2] - q[2]),
    ];
    (c, t)
}

fn edge_near(b: &Body, p: [f64; 3]) -> usize {
    use solidcore::query::api_queries::edge_closest_point;
    let (k, brep) = common::kernel_copy(b);
    let q = solidcore::Vec3::new(p[0], p[1], p[2]);
    let (i, d) = brep
        .edge_list()
        .iter()
        .enumerate()
        .map(|(i, &e)| {
            (
                i,
                edge_closest_point(&k, &brep, e, q)
                    .unwrap()
                    .closest
                    .distance,
            )
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .unwrap();
    assert!(d < 1e-4, "no edge near {p:?}: {d}");
    i
}

/// Whether the top's contact x = -36 - d of the ball rounding the rim `d` at V = (-36, -65 - r4),
/// where the corner's round meets x = -36, lies inside the contact circle: (18 - d)^2 + (25 - r4)^2
/// < 23^2. Along x = -36 the contact only nears B from V on, so the top is then narrower than the
/// round the whole length from V to W.
fn wholly_past(r4: f64, d: f64) -> bool {
    (18.0 - d).powi(2) + (25.0 - r4).powi(2) < 23.0 * 23.0
}

/// The rim of the arm's top rounded `d` along [`top_rim`] on [`arm_boss_and_foot_round`]: built,
/// valid at the full check, and its rails the closed forms, within the body's tolerance (each
/// rail is a curve fitted to the ball's traced contacts): on the corner's round, the ball's
/// centre r4 + d from A, before the top's contact (straight above it) meets the contact circle,
/// its contacts on the top and on the corner's round, r4 from A. Where [`wholly_past`], also
/// that contact meeting the contact circle, and on from there to V, and along x = -36 from V to
/// W, the ball's contacts on the foot round ([`ball_on_foot_round`]) and on the corner's round
/// or x = -36. (Elsewhere the rail along x = -36 rolls onto the foot round partway, in the
/// trace's own rollover: the foot round's edge's round runs on past W to where the top's contact
/// meets the contact circle. Its rail there lies about 1e-4 from the closed form, as before this
/// rollover was added, and is not checked here.) No volume: the round's faces on the foot round
/// bound no closed-form solid.
///
/// The model is built with the corpus's fix set ([`fixes`]; its foot round fails without it),
/// the rim's round with that set (`corpus`) or with none, solidcore's default. Without the
/// corpus's fixes the corner's round's piece on the foot round, which the trace's own rollover
/// traces again from the crossing (`ProcessRollOver`), lies up to about 1.2e-4 from the closed
/// form (r4 10, d 2) and is not checked; the rail rolled before the trace along x = -36 is.
fn assert_rim_round_builds(r4: f64, d: f64, corpus: bool) {
    use solidcore::query::api_queries::edge_closest_point;
    use std::f64::consts::{FRAC_PI_2, PI};
    fixes();
    let b = arm_boss_and_foot_round(r4);
    let rim = top_rim(&b, r4);
    if !corpus {
        session::set_thread_fixes(std::iter::empty::<&str>()).unwrap();
    }
    let result = blend(&b, Blend::Round, &rim, &[d], &[], Corners::Filled);
    fixes();
    result.unwrap_or_else(|err| panic!("r4 {r4} d {d} corpus {corpus}: {err:?}"));
    let report = b.check(CheckLevel::All).unwrap();
    assert!(report.is_valid(), "r4 {r4} d {d} corpus {corpus}: {report}");
    let (k, brep) = common::kernel_copy(&b);
    let tolerance = brep.tolerance();
    let on_rail = |p: [f64; 3], what: &str| {
        let q = solidcore::Vec3::new(p[0], p[1], p[2]);
        let distance = brep
            .edge_list()
            .iter()
            .map(|&e| {
                edge_closest_point(&k, &brep, e, q)
                    .unwrap()
                    .closest
                    .distance
            })
            .fold(f64::INFINITY, f64::min);
        assert!(
            distance < tolerance,
            "r4 {r4} d {d} corpus {corpus}: {what} {p:?} is {distance} from the nearest edge \
             (tolerance {tolerance})"
        );
    };
    // along x = -36, from V to W
    if wholly_past(r4, d) {
        let v = -65.0 - r4;
        let w = top_end_on_side();
        for f in [0.1, 0.5, 0.9] {
            let y = v + (w - v) * f;
            let (c, t) = ball_on_foot_round(-36.0 - d, y, d);
            on_rail(t, "the contact on the foot round");
            on_rail([-36.0, y, c[2]], "the contact on x = -36");
        }
    }
    // on the corner's round: the top's contact A + (r4 + d) u(th) at z = 5 meets the contact
    // circle where |A - B + (r4 + d) u|^2 = 23^2, that is m cos(th - phi) = (23^2 - m^2 -
    // (r4 + d)^2) / (2 (r4 + d)), with m and phi A - B's length and angle
    let (ax, ay) = (-36.0 + r4, -65.0 - r4);
    let reach = r4 + d;
    let (px, py) = (ax + 54.0, ay + 90.0);
    let m = (px * px + py * py).sqrt();
    let cosine = (23.0 * 23.0 - m * m - reach * reach) / (2.0 * reach * m);
    let cross = py.atan2(px) + cosine.acos();
    assert!(
        cross > FRAC_PI_2 && (cross < PI) == wholly_past(r4, d),
        "r4 {r4} d {d}: crossing at {cross}"
    );
    let along = |th: f64, r: f64| (ax + r * th.cos(), ay + r * th.sin());
    let before = (FRAC_PI_2 + cross.min(PI)) / 2.0;
    let (x, y) = along(before, reach);
    on_rail([x, y, 5.0], "the contact on the top");
    let (x, y) = along(before, r4);
    on_rail([x, y, 5.0 - d], "the contact on the corner's round");
    if wholly_past(r4, d) {
        let (x, y) = along(cross, reach);
        on_rail([x, y, 5.0], "the crossing of the contact circle");
    }
    // the corner's round's piece on the foot round, traced in the trace's own rollover
    // (`ProcessRollOver`): checked with the corpus's fixes only
    if wholly_past(r4, d) && corpus {
        for f in [0.25, 0.5, 0.75] {
            let th = cross + (PI - cross) * f;
            let (x, y) = along(th, reach);
            let (c, t) = ball_on_foot_round(x, y, d);
            on_rail(t, "the contact on the foot round");
            let (x, y) = along(th, r4);
            on_rail([x, y, c[2]], "the contact on the corner's round");
        }
    }
}

/// #15767 case 2: where [`wholly_past`], the round's rail on the arm's top along x = -36 lies off
/// the top the whole length, past the foot round's smooth edge; it is rolled over onto the foot
/// round before the trace, and the corner's round's rail rolls onto the foot round where it
/// meets the contact circle, joining it at V. Before, with the corpus's fixes, it was refused
/// as having no room after the trace ([`Error::NoRoom`], or [`Error::TooLarge`] with its rail
/// leaving its face; SolidCraft reports the fit 1.476 at r4 = 9), and without them it failed in
/// the trace's rollover (upstream's unimplemented tangent case, status 1001).
#[test]
fn a_rim_round_whose_rail_lies_wholly_on_the_foot_round_rolls_onto_it() {
    for (r4, d) in [(9.0, 1.6), (9.0, 2.0), (9.0, 2.5), (10.0, 1.4), (10.0, 2.0)] {
        assert!(wholly_past(r4, d), "r4 {r4} d {d}");
        for corpus in [true, false] {
            assert_rim_round_builds(r4, d, corpus);
        }
    }
}

/// Controls: where the top's contact at V lies outside the contact circle, the rail stays on the
/// top at V and rolls onto the foot round partway along x = -36 (the trace's own rollover), as
/// before. (8, 2.5) is the limit: 15.5^2 + 17^2 = 529.25, a quarter over 23^2.
#[test]
fn a_rim_round_partly_on_the_top_rolls_onto_the_foot_round_partway() {
    for (r4, d) in [(8.0, 2.0), (8.0, 2.5), (9.0, 1.4)] {
        assert!(!wholly_past(r4, d), "r4 {r4} d {d}");
        for corpus in [true, false] {
            assert_rim_round_builds(r4, d, corpus);
        }
    }
}

/// A real miss between the sections that propose the rollover (#15767 review): the foot round
/// is cut by a slot 0.04 wide and 2 deep, from the top z = 5 up through the foot round, across
/// x = -38.6 to -36.3 at y = `ys`, halfway between two of the 17 sections along x = -36 (0.105
/// apart at r4 = 9), so its floor is the top reaching under the ball's contact x = -38. At every
/// section the contact lies off the top and the foot round carries the ball, and the rail is
/// rolled onto the foot round; in the slot the foot round is cut away, and the rail rolled onto
/// it crosses the slot's edges. The rail on the top crosses the slot's rims, cliffs: the cliff
/// overflow (the only path since tangency-labs/solidcraft#13841, as it was with the corpus's
/// fixes) takes it before the trim. The overflowing rim edge beside the slot, from (-36, -74)
/// to (-36, w), goes first with its neighbour toward the boss; its end at w is an End there,
/// and the trace past it runs on to where the rails meet at (-36, -90, 10). Its rail on the
/// foot round leaves the face at the slot, between the edge's ends: the blend is refused as
/// its rail leaving its face ([`RoomReason::RailLeavesFace`]), found for that edge (`at` is
/// the edge's middle). Before tangency-labs/solidcraft#16686 the trace's last section
/// there, 1.66e-5 wide, did not skin (the rails' ends were not made to meet), and the pass
/// failed internally with status 1001. The fit size is not pinned: it is named above the
/// size asked (3.25 for 2), tangency-labs/solidcraft#16782.
#[test]
fn a_rim_round_whose_top_reaches_the_contact_between_sections_is_not_built() {
    let r4 = 9.0;
    let d = 2.0;
    let (v, w) = (-65.0 - r4, top_end_on_side());
    let step = (v - w) / 16.0;
    let ys = v - 8.5 * step;
    let width = 0.04;
    assert!(width < step / 2.0, "the slot lies between two sections");
    fixes();
    let b = arm_boss_and_foot_round(r4);
    let slot = make_box([-38.6, ys - width / 2.0, 5.0], [2.3, width, 2.0]).unwrap();
    let b = boolean(b, slot, BooleanOp::Subtract).unwrap().unwrap();
    assert!(b.check(CheckLevel::All).unwrap().is_valid());
    let rim = top_rim(&b, r4);
    session::set_thread_fixes(std::iter::empty::<&str>()).unwrap();
    let result = blend(&b, Blend::Round, &rim, &[d], &[], Corners::Filled);
    fixes();
    // the middle of the rim edge beside the slot, (-36, -74, 5) to (-36, w, 5)
    let mid = [-36.0, 0.5 * (v + w), 5.0];
    match result {
        // the size waits on tangency-labs/solidcraft#16782
        Err(Error::TooLarge {
            at: Some(at),
            why: Some(RoomReason::RailLeavesFace),
            ..
        }) => {
            let off = at
                .iter()
                .zip(mid)
                .map(|(a, m)| (a - m).abs())
                .fold(0.0, f64::max);
            assert!(
                off < 1e-9,
                "found for {at:?}, not the edge beside the slot {mid:?}"
            );
        }
        other => panic!("the slot between the sections: {other:?}"),
    }
}

/// The largest round at C that builds on the arm and boss, in closed form. The ball touches
/// y = -65 and rolls on the boss: its centre is at (x, -65 - r), 18 + r from B = (-54, -90), so
/// (x + 54)^2 + (r - 25)^2 = (18 + r)^2, that is (x + 54)^2 = 86 r - 301, and its contact on
/// y = -65 is straight above it at x = -54 + sqrt(86 r - 301). That contact grows with r and stays
/// on the face y = -65 (x from -36 to 0) while sqrt(86 r - 301) <= 54: r <= (54^2 + 301) / 86 =
/// 3217 / 86. Its contact on the boss lies past the side's end and short of the boss's bottom
/// for every r > 25, and the side's own boundary (its end y = -90 at r = 25) does not bound it:
/// the side is consumed, its rail rolled over onto the boss.
const BOSS_ROUND_FITS: f64 = 3217.0 / 86.0;

/// A round at C too large for the face y = -65 is refused as its rail leaving its face, and the
/// refusal names the largest size at which its rails stay on the faces it rolls on, y = -65 and
/// the boss it rolled onto: the closed form [`BOSS_ROUND_FITS`], open, so less the fit margin
/// (the bound of the convex case above). Not the width of the side it rolled off (25, less the
/// margin), at which it builds. The named size builds, as do radii between the built ones and
/// it, with the closed-form rails and volume.
#[test]
fn a_boss_round_too_large_names_the_size_that_fits_the_boss() {
    fixes();
    // the closed form puts the contact on y = -65 at that face's end, x = 0
    let at = -54.0 + (86.0 * BOSS_ROUND_FITS - 301.0).sqrt();
    assert!(at.abs() < 1e-12, "the closed form's contact {at}");
    let height = 5.0;
    let mut named = Vec::new();
    for r in [38.0_f64, 40.0] {
        let b = arm_and_boss();
        let e = edge_at(&b, [-36.0, -65.0, height / 2.0]);
        match blend(&b, Blend::Round, &[e], &[r], &[], Corners::Filled) {
            Err(Error::TooLarge {
                size,
                why: Some(RoomReason::RailLeavesFace),
                ..
            }) => {
                assert!(
                    size < BOSS_ROUND_FITS && size > BOSS_ROUND_FITS - 0.02,
                    "r {r}: fit {size}, the closed form {BOSS_ROUND_FITS}"
                );
                // exactly: the fit margin is a thousand of the edge's tolerances (1e-5)
                assert!(
                    (size - (BOSS_ROUND_FITS - 0.01)).abs() < 1e-9,
                    "r {r}: fit {size}, the closed form {BOSS_ROUND_FITS} less 0.01"
                );
                named.push(size);
            }
            other => panic!("r {r}: {other:?}"),
        }
    }
    for r in [32.0, 34.0, 36.0, 37.0, 37.4] {
        assert!(r < BOSS_ROUND_FITS);
        assert_boss_round_builds(r);
    }
    for size in named {
        assert_boss_round_builds(size);
    }
}

/// SolidCraft's fuzz `atomic_commands` seed 2: its body `b27` as the rig's export gives it, and
/// the record's canonical order (edge 28 is at record index 28).
fn b27() -> (Body, solidcore_ops::Order) {
    use std::io::Read;
    fixes();
    let gz: &[u8] = include_bytes!("data/atomic_seed2_b27.bodies.json.gz");
    let mut text = String::new();
    flate2::read::GzDecoder::new(gz)
        .read_to_string(&mut text)
        .unwrap();
    let mut records: serde_json::Value = serde_json::from_str(&text).unwrap();
    let record: solidcore_record::brep::Brep =
        serde_json::from_value(records["b27"].take()).unwrap();
    let (input, order) = Body::from_record(&record, true).unwrap();
    input.copy_with_order(&order).unwrap()
}

/// Edge 28 of `b27` rounded `r` alone, on a fresh copy of the body; the body when it builds.
fn b27_edge_28(r: f64) -> Result<Body, Error> {
    use solidcore_ops::blend::{blend_ordered, BlendOptions};
    let (body, order) = b27();
    blend_ordered(
        &body,
        &order,
        Blend::Round,
        &[28],
        &[r],
        &[],
        Corners::Filled,
        BlendOptions::default(),
    )
    .map(|_| body)
}

/// The fit margin: a thousand of edge 28's tolerance, 1e-5.
const B27_FIT_MARGIN: f64 = 1000.0 * 1e-5;

/// In closed form, the radius above which no ball touching both of edge 28's faces lies
/// between the edge's ends. Edge 28 is the crease of an R3 convex round (the cylinder about
/// the line x = 3, z = 4.5 along y, between the planes x = 0 and z = 7.5, which it meets
/// tangentially along x = 0, z = 4.5 and x = 3, z = 7.5) with a spherical pocket (radius 10
/// about C = (2.5, 50, 0)). Its end V = (0, 50 - sqrt(73.5), 4.5) on the plane x = 0 has
/// tangent `n_cyl x n_sphere` = (-1, 0, 0) x (V - C), along (0, 4.5, sqrt(73.5)): its End's
/// plane is 4.5 (y - `V_y`) + sqrt(73.5) (z - 4.5) = 0. A ball of radius r touching the round
/// along its far tangent edge (x = 3, z = 7.5) has its centre at (3, y, 7.5 - r), and touching
/// the pocket from outside |centre - C| = 10 + r: (y - 50)^2 = (10 + r)^2 - 0.25 - (7.5 - r)^2
/// = 43.5 + 35 r. That centre lies in V's plane where
/// 4.5 (sqrt(73.5) - sqrt(43.5 + 35 r)) + sqrt(73.5) (3 - r) = 0, i.e.
/// (7.5 - r)^2 73.5 / 20.25 = 43.5 + 35 r, i.e. 98 r^2 - 2415 r + 4338 = 0, whose smaller
/// root is (2415 - sqrt(4131729)) / 196 = 1.950686...
fn b27_edge_28_cutoff() -> f64 {
    (2415.0 - 4_131_729.0_f64.sqrt()) / 196.0
}

/// Edge 28 of `b27` rounded at or above the cutoff (and up to r 3, where the round's offset
/// collapses onto its axis) is refused: every ball on its faces lies past its end on x = 0,
/// so the round has no section between its ends. It used to fail with status 1001 (at 2.99
/// with no cross sections to trace from, at 2.5 to 1.97 on the End's cap edge, its rail 0.81
/// off the cap's plane), and r 3's refusal named 2.99, saying any radius below 3 rolls. Each
/// refusal now names the closed-form cutoff less the fit margin. Below r 3 the kernel's
/// reason is `EndRollsIntoSmoothEnd`, which ops names no `RoomReason` for.
#[test]
fn a_round_whose_every_ball_lies_past_an_end_names_the_radius_that_has_a_section() {
    let want = b27_edge_28_cutoff() - B27_FIT_MARGIN;
    for (r, reason) in [
        (3.0, Some(RoomReason::OffsetCollapses)),
        (2.99, None),
        (2.5, None),
        (2.0, None),
        (1.97, None),
    ] {
        match b27_edge_28(r) {
            Err(Error::TooLarge { size, why, .. }) => {
                assert_eq!(why, reason, "r {r}");
                assert!((size - want).abs() <= 1e-9, "r {r}: {size} against {want}");
            }
            Err(e) => panic!("r {r}: {e:?}"),
            Ok(_) => panic!("r {r}: built"),
        }
    }
}

/// The premise of those refusals, sampled across the whole of the R3 round: below r 3 and at
/// or above the cutoff, the ball touching the round at angle t about its axis (t from 90
/// degrees, its tangent edge z = 7.5, to 180, its tangent edge x = 0) and the pocket from
/// outside has its centre past the End's plane at V = (0, 50 - sqrt(73.5), 4.5), at 4001
/// angles each. Below the cutoff the ball on the tangent edge z = 7.5 is on the span's side.
#[test]
fn every_ball_on_the_r3_round_lies_past_the_end_at_or_above_the_cutoff() {
    let rc = b27_edge_28_cutoff();
    let vy = 50.0 - 73.5_f64.sqrt();
    // the End's plane through V, its normal along the edge into the span
    let past = |c: [f64; 3]| -(4.5 * (c[1] - vy) + 73.5_f64.sqrt() * (c[2] - 4.5));
    let centre = |r: f64, t: f64| {
        let (x, z) = (3.0 + (3.0 - r) * t.cos(), 4.5 + (3.0 - r) * t.sin());
        let y = 50.0 - ((10.0 + r).powi(2) - (x - 2.5).powi(2) - z * z).sqrt();
        [x, y, z]
    };
    for r in [rc, 1.97, 2.0, 2.5, 2.99] {
        for i in 0..=4000 {
            let t = PI / 2.0 + PI / 2.0 * f64::from(i) / 4000.0;
            let d = past(centre(r, t));
            assert!(d >= -1e-12, "r {r}, t {t}: {d} on the span's side");
        }
    }
    assert!(past(centre(rc - B27_FIT_MARGIN, PI / 2.0)) < 0.0);
}

/// The size those refusals name builds a valid solid that is smaller than the input, its rails
/// on the faces it rolls on, and so does 1.94. Both failed with "negative interval" in the
/// rails' trim (#15905). Just below the cutoff the section at the end on x = 0 is solved with
/// its centre in the End's plane, but the rail on the R3 round crosses that plane only some
/// 60 degrees round the round from it (the round runs across the cylinder there, nearly in
/// the plane), on the trace's extension back from its start. On the closed curve the offsets
/// meet in, the extension past the end had stopped at the trace's domain, short of the start,
/// keeping 250 of the gap's 351 degrees, and the extension back kept half of the rest: the
/// rail's only crossing of the plane was 67 degrees past the other end, and the trim's
/// interval was inverted. Each end now keeps what it keeps when the extension past the end
/// closes. (Between 1.945 and the cutoff the trim now succeeds, and the End's cap is built
/// there too: [`a_round_in_the_sliver_below_the_cutoff_builds_its_end_cap`].)
#[test]
fn the_size_the_past_an_end_refusal_names_builds() {
    let (input, _) = b27();
    let before = input.volume(1e-9).unwrap();
    for r in [b27_edge_28_cutoff() - B27_FIT_MARGIN, 1.94] {
        let made = b27_edge_28(r).unwrap_or_else(|e| panic!("r {r}: {e:?}"));
        assert!(made.check(CheckLevel::All).unwrap().is_valid(), "r {r}");
        let after = made.volume(1e-9).unwrap();
        assert!(after > 0.0 && after < before, "r {r}: {after} of {before}");
        assert_b27_rails_on_faces(&input, &made, r, 1e-5);
    }
}

/// A radius part of whose round lies between the edge's ends is not refused as rolling past
/// an end, and builds (1.9 and 1.7: the ball touching the R3 round on its tangent edge
/// z = 7.5 lies on the span's side of the End's plane on x = 0, by the closed form of
/// [`b27_edge_28_cutoff`]; the ball rolls on the R3 round near that end and over the
/// tangent edge onto z = 7.5 towards the other).
#[test]
fn a_round_partly_between_the_ends_is_not_refused_past_an_end() {
    let (input, _) = b27();
    for r in [1.9, 1.7] {
        assert!(r < b27_edge_28_cutoff());
        let made = b27_edge_28(r).unwrap_or_else(|e| panic!("r {r}: {e:?}"));
        assert!(made.check(CheckLevel::All).unwrap().is_valid(), "r {r}");
        assert_b27_rails_on_faces(&input, &made, r, 1e-5);
    }
}

/// The arm and boss of [`arm_and_boss`] with the boss standing 20 tall over the 5 thick arm
/// (SolidCraft #15641's test9: its boss rises from the arm's base plane past the arm's top face):
/// the cylinder of radius 18 about B from z = 0 to 20, joined to the arm. `seam_deg`: the angle
/// about B its seam runs up at, built as a swept circle; `None`: the cylinder `make_cylinder`
/// makes, whose seam runs up at 0 degrees, through the tangent edge's line (-36, -90), as
/// test9's does.
fn arm_and_tall_boss(seam_deg: Option<f64>) -> Body {
    use solidcore::curve::curve::Curve as KCurve;
    use solidcore::math::Point3;
    use solidcore::primitive::api_curves::create_arc;
    use solidcore::sweep::api_brep::create_linear_sweep;
    let boss = match seam_deg {
        None => solidcore_ops::make_cylinder([-54.0, -90.0, 0.0], 18.0, 20.0).unwrap(),
        Some(seam) => common::built(|k| {
            let circle = KCurve::from(create_arc(
                Point3::new(-54.0, -90.0, 0.0),
                18.0,
                seam - 360.0,
                seam,
            )?);
            create_linear_sweep(
                k,
                &[Some(&circle)],
                solidcore::Vec3::new(0.0, 0.0, 1.0),
                20.0,
                true,
            )
        }),
    };
    boolean(arm_and_boss(), boss, BooleanOp::Union)
        .unwrap()
        .unwrap()
}

/// A round past a side running into a boss that stands taller than the arm (#15767 case 1):
/// the rolled contact line on the boss meets the arm's top plane z = 5 inside the boss's face,
/// at angle theta2 < 0 about B, where the boss's foot edge on the arm's top face starts at
/// angle 0 and runs away from it (theta 0 -> 180 deg). The round ends there: the end rib runs
/// on z = 5 to that point, and a new edge on the boss face runs along the foot circle from it
/// back to (-36, -90, 5); the boss loses the strip between theta2 and 0 below z = 5 and keeps
/// everything above. Closed forms: the volume 5 (section + the round's area) + pi 18^2 15 (the
/// boss above the arm), and the boss face's area 2 pi 18 15 + pi 18 5 - 18 |theta2| 5. With the
/// boss seam at the tangent edge's line (test9's, and `make_cylinder`'s) the foot circle's
/// parameter starts at the new edge's end, and the rolled ball's section at the corner is
/// guessed on that seam; with the seam inside the removed strip the new edge runs through the
/// circle's closure.
#[test]
fn a_round_rolled_onto_a_taller_boss_ends_inside_the_boss_face() {
    use solidcore::query::api_queries::edge_closest_point;
    use solidcore_record::brep::Surface as RecordSurface;
    fixes();
    let height = 5.0;
    let profile = 72.0 * 25.0 + 36.0 * 25.0 + PI * 18.0 * 18.0 / 2.0;
    // the seam at the tangent edge's line (make_cylinder's, test9's), a quarter turn on, at the
    // foot edge's far end (-72, -90), and inside the strip the round removes (theta2 < seam < 0:
    // 359 deg for r >= 26, 357 deg for r >= 28)
    for seam in [None, Some(90.0), Some(180.0), Some(359.0), Some(357.0)] {
        for r in [25.5_f64, 26.0, 27.0, 28.0, 30.0] {
            let b = arm_and_tall_boss(seam);
            let e = edge_at(&b, [-36.0, -65.0, height / 2.0]);
            blend(&b, Blend::Round, &[e], &[r], &[], Corners::Filled)
                .unwrap_or_else(|err| panic!("seam {seam:?} r {r}: {err:?}"));
            let report = b.check(CheckLevel::All).unwrap();
            assert!(report.is_valid(), "seam {seam:?} r {r}: {report}");
            // the ball's centre (cx, -65 - r) and its contact on the boss at theta2
            let cy = -65.0 - r;
            let cx = -54.0 + ((18.0 + r).powi(2) - (cy + 90.0).powi(2)).sqrt();
            let theta2 = (cy + 90.0).atan2(cx + 54.0);
            assert!(theta2 < 0.0, "seam {seam:?} r {r}: theta2 {theta2}");
            let t2 = (-54.0 + 18.0 * theta2.cos(), -90.0 + 18.0 * theta2.sin());
            let (k, brep) = common::kernel_copy(&b);
            for (x, y) in [(cx, -65.0), t2] {
                for z in [0.5, 2.5, 4.5] {
                    let q = solidcore::Vec3::new(x, y, z);
                    let on_edge = brep.edge_list().iter().any(|&e| {
                        edge_closest_point(&k, &brep, e, q)
                            .unwrap()
                            .closest
                            .distance
                            < 1e-7
                    });
                    assert!(
                        on_edge,
                        "seam {seam:?} r {r}: no rail through ({x}, {y}, {z})"
                    );
                }
            }
            let v = b.volume(1e-9).unwrap();
            let expected = height * (profile + boss_round_area(r)) + PI * 18.0 * 18.0 * 15.0;
            assert!(
                (v - expected).abs() < 1e-6,
                "seam {seam:?} r {r}: volume {v}, expected {expected}"
            );
            // the boss's faces: those on the cylinder of radius 18 about B
            let (record, order) = b.to_record().unwrap();
            let mut boss_area = 0.0;
            let mut boss_faces = 0;
            for (i, face) in record.faces.iter().enumerate() {
                let RecordSurface::Cylinder { frame, radius } = &face.surface else {
                    continue;
                };
                if (radius - 18.0).abs() < 1e-9
                    && (frame.origin[0] + 54.0).abs() < 1e-9
                    && (frame.origin[1] + 90.0).abs() < 1e-9
                {
                    boss_area += b.face_area(&order, i, 1e-12).unwrap();
                    boss_faces += 1;
                }
            }
            assert!(boss_faces > 0, "seam {seam:?} r {r}: no face on the boss");
            let expected_area =
                2.0 * PI * 18.0 * 15.0 + PI * 18.0 * height - 18.0 * theta2.abs() * height;
            assert!(
                (boss_area - expected_area).abs() < 1e-6,
                "seam {seam:?} r {r}: boss area {boss_area}, expected {expected_area}"
            );
        }
    }
}

/// The rails of edge 28's round in `made`, checked against the closed forms of the faces it
/// rolls on: every edge the round made (no edge of `input` passes through its middle) that
/// lies, at 65 points, within `tol` of the R3 round's cylinder (about x = 3, z = 4.5 along y),
/// the pocket's sphere (radius 10 about (2.5, 50, 0)) or the plane z = 7.5 lies within that
/// face (the cylinder between its tangent edges x = 0 and z = 7.5, the plane past the tangent
/// edge x = 3). At least one lies on each: the round rolls on the R3 round and the pocket and
/// over the tangent edge onto z = 7.5. `tol` is the edge's tolerance (1e-5) where the rails
/// are that close, else the rail's own tolerance (the blend's approximation tolerance, 4e-5).
fn assert_b27_rails_on_faces(input: &Body, made: &Body, r: f64, tol: f64) {
    use solidcore::query::api_queries::{edge_closest_point, get_edges};
    let (ki, bi) = common::kernel_copy(input);
    let (k, b) = common::kernel_copy(made);
    let input_edges = get_edges(&bi).unwrap();
    let mut counts = [0; 3];
    for e in get_edges(&b).unwrap() {
        let edge = b.edge(e).unwrap();
        let curve = edge.curve.as_ref().unwrap();
        let ivl = edge.interval;
        let pts: Vec<_> = (0..=64)
            .map(|i| {
                let t = ivl.min() + (ivl.max() - ivl.min()) * f64::from(i) / 64.0;
                curve.evaluate_point(&k, t).unwrap()
            })
            .collect();
        let old = input_edges.iter().any(|&f| {
            edge_closest_point(&ki, &bi, f, pts[32])
                .unwrap()
                .closest
                .distance
                <= 1e-5
        });
        if old {
            continue;
        }
        let on_cylinder = pts
            .iter()
            .all(|p| ((p.x - 3.0).hypot(p.z - 4.5) - 3.0).abs() <= tol);
        let on_sphere = pts.iter().all(|p| {
            (((p.x - 2.5).powi(2) + (p.y - 50.0).powi(2) + p.z * p.z).sqrt() - 10.0).abs() <= tol
        });
        let on_top = pts.iter().all(|p| (p.z - 7.5).abs() <= tol);
        if on_cylinder {
            counts[0] += 1;
            for p in &pts {
                assert!(
                    p.x <= 3.0 + tol && p.z >= 4.5 - tol,
                    "r {r}: a rail on the R3 round leaves it at {p:?}"
                );
            }
        }
        if on_sphere {
            counts[1] += 1;
        }
        if on_top {
            counts[2] += 1;
            for p in &pts {
                assert!(
                    p.x >= 3.0 - tol,
                    "r {r}: a rail on z = 7.5 leaves it at {p:?}"
                );
            }
        }
    }
    assert!(
        counts.iter().all(|&n| n > 0),
        "r {r}: rails on the R3 round, the pocket and z = 7.5: {counts:?}"
    );
}

/// Edge 28 of `b27` rounded 1.6 builds a valid solid (#15906). It failed with status 1001,
/// then reported as "the smooth end cap is not built": the End at the edge's end on x = 0
/// never built its edges. Its rail on the R3 round is the trace's Hermite approximation, whose spans the
/// trace accepts by a sampled check; it crosses the End's plane 1.9e-5 off the round, past
/// the round's zone (1e-5), so the rail end classified off its face and the End waited for a
/// rollover over the round's tangent edge x = 0 that never comes (the round rolls over only
/// its other tangent edge, z = 7.5, at the other end), in the rails' adjusted pass too. A rail
/// end off its face at the face's zone is now solved on the faces (the point of the round on
/// the End's plane where the ball touches it) and classified there. At 1.5 and 1.7 the crossing fell
/// within 1e-5 of the round; those builds are unchanged, and the three volumes fall as the
/// radius grows (a larger ball removes more of a convex edge).
#[test]
fn a_round_whose_rail_end_is_off_its_face_by_the_rails_tolerance_builds_its_end_cap() {
    let (input, _) = b27();
    let mut volume = input.volume(1e-9).unwrap();
    for r in [1.5, 1.6, 1.7] {
        let made = b27_edge_28(r).unwrap_or_else(|e| panic!("r {r}: {e:?}"));
        let report = made.check(CheckLevel::All).unwrap();
        assert!(report.is_valid(), "r {r}: {report}");
        let after = made.volume(1e-9).unwrap();
        assert!(
            after > 0.0 && after < volume,
            "r {r}: {after} after {volume}"
        );
        volume = after;
        // the rails lie on their faces to the tolerance the trace builds them to
        assert_b27_rails_on_faces(&input, &made, r, 4e-5);
    }
}

/// Edge 28 of `b27` rounded in the band just below the cutoff (1.945 to 1.9505), where the
/// round is a sliver near the end on x = 0 and rolls over onto z = 7.5 for the rest, builds
/// valid solids (#15906). Once the rails' trim succeeded there (#15905), these failed as 1.6
/// did (then reported as "the smooth end cap is not built"): the End on x = 0 found its rail end on the R3 round
/// off the round by more than its zone and less than the rail's tolerance. The volumes fall as
/// the radius grows, and the rails lie on their faces to the rail's tolerance.
#[test]
fn a_round_in_the_sliver_below_the_cutoff_builds_its_end_cap() {
    let (input, _) = b27();
    let mut volume = input.volume(1e-9).unwrap();
    for r in [1.945, 1.948, 1.95, 1.9505] {
        assert!(r < b27_edge_28_cutoff());
        let made = b27_edge_28(r).unwrap_or_else(|e| panic!("r {r}: {e:?}"));
        let report = made.check(CheckLevel::All).unwrap();
        assert!(report.is_valid(), "r {r}: {report}");
        let after = made.volume(1e-9).unwrap();
        assert!(
            after > 0.0 && after < volume,
            "r {r}: {after} after {volume}"
        );
        volume = after;
        assert_b27_rails_on_faces(&input, &made, r, 4e-5);
    }
}

/// [`arm_boss_and_foot_round`]'s body before its corner round: the arm and the 20 tall boss,
/// the boss's foot on the arm's top rounded 5, built with the corpus's fix set. The foot round
/// runs on past the arm's side x = -36 at the foot edge's end (-36, -90, 5): the side's plane
/// cuts it off, so the side carries a lobe above the top z = 5, between the tangent edge
/// x = -36, y = -90 (up to z = 10) and the plane-torus edge from W = (-36, -90 + sqrt 205, 5)
/// up to (-36, -90, 10). The top's edge on the side runs from C's top (-36, -65, 5) to W, and
/// W's edge between the top and the foot round (the contact circle, radius 23 about B) leaves W
/// back towards y = -65.
fn arm_boss_foot_only() -> Body {
    fixes();
    let boss = solidcore_ops::make_cylinder([-54.0, -90.0, 0.0], 18.0, 20.0).unwrap();
    let b = boolean(arm_and_boss(), boss, BooleanOp::Union)
        .unwrap()
        .unwrap();
    let foot = edge_at(&b, [-54.0, -72.0, 5.0]);
    blend(&b, Blend::Round, &[foot], &[5.0], &[], Corners::Filled).unwrap();
    b
}

/// The arm's inner corner C = (-36, -65) rounded `r4` alone on [`arm_boss_foot_only`] (test9's
/// Fillet4 with its Fillet3 present), with the corpus's fix set (`corpus`) or none: built,
/// valid at the full check, its rails the lines along z through the ball's contacts
/// (-36 + r4, -65) on y = -65 and (-36, -65 - r4) on the side x = -36, and its volume the
/// input's plus the round's section r4^2 (1 - pi / 4) times the arm's height 5 (the notch it
/// fills is empty up to z = 5 and the round removes nothing). Past r4 = 25 - sqrt 205 (10.68)
/// the side's rail reaches z = 5 past W, inside the side's lobe under the foot round: its end
/// cap on z = 5 then has an edge on the side's plane from the rail's end to W, which is
/// checked too.
fn assert_corner_round_under_foot_round_builds(r4: f64, corpus: bool) {
    use solidcore::query::api_queries::edge_closest_point;
    let b = arm_boss_foot_only();
    let before = b.volume(1e-9).unwrap();
    let e = edge_at(&b, [-36.0, -65.0, 2.5]);
    if !corpus {
        session::set_thread_fixes(std::iter::empty::<&str>()).unwrap();
    }
    let result = blend(&b, Blend::Round, &[e], &[r4], &[], Corners::Filled);
    fixes();
    result.unwrap_or_else(|err| panic!("r4 {r4} corpus {corpus}: {err:?}"));
    let report = b.check(CheckLevel::All).unwrap();
    assert!(report.is_valid(), "r4 {r4} corpus {corpus}: {report}");
    let (k, brep) = common::kernel_copy(&b);
    let on_edge = |p: [f64; 3]| {
        let q = solidcore::Vec3::new(p[0], p[1], p[2]);
        brep.edge_list().iter().any(|&e| {
            edge_closest_point(&k, &brep, e, q)
                .unwrap()
                .closest
                .distance
                < 1e-7
        })
    };
    for (x, y) in [(-36.0 + r4, -65.0), (-36.0, -65.0 - r4)] {
        for z in [0.5, 2.5, 4.5] {
            assert!(
                on_edge([x, y, z]),
                "r4 {r4} corpus {corpus}: no rail through ({x}, {y}, {z})"
            );
        }
    }
    let w = top_end_on_side();
    if -65.0 - r4 < w {
        let y = (-65.0 - r4 + w) / 2.0;
        assert!(
            on_edge([-36.0, y, 5.0]),
            "r4 {r4} corpus {corpus}: no cap edge through (-36, {y}, 5)"
        );
    }
    let after = b.volume(1e-9).unwrap();
    let expected = before + 5.0 * r4 * r4 * (1.0 - PI / 4.0);
    assert!(
        (after - expected).abs() < 1e-6,
        "r4 {r4} corpus {corpus}: volume {after}, expected {expected}"
    );
}

/// #15767's band with the foot round (test9's Fillet4 alone, with Fillet3 present): past
/// r4 = 25 - sqrt 205 (10.68) the round's rail on the side x = -36 meets the top's plane z = 5
/// past the top's edge on the side, which ends at W where the foot round's contact circle
/// turns back from it, so the rail ends inside the side's lobe under the foot round. Before,
/// with the corpus's fixes the end corner failed (status 1001), and without them it was
/// refused, naming 10.68. Up to r4 = 25, where the rail reaches the side's tangent edge into
/// the boss.
#[test]
fn a_corner_round_whose_rail_ends_past_the_top_under_the_foot_round_builds() {
    for r4 in [10.8, 12.0, 15.0, 20.0, 24.0] {
        assert!(-65.0 - r4 < top_end_on_side());
        for corpus in [true, false] {
            assert_corner_round_under_foot_round_builds(r4, corpus);
        }
    }
}

/// Controls: below r4 = 25 - sqrt 205 (10.68) the rail on the side meets z = 5 on the top's
/// edge, short of W, and the round builds as before, with the same closed forms.
#[test]
fn a_corner_round_whose_rail_ends_on_the_top_s_edge_builds() {
    for r4 in [8.0, 10.0, 10.6, CORNER_ROUND_REACHES_W - 1e-3] {
        assert!(-65.0 - r4 > top_end_on_side());
        for corpus in [true, false] {
            assert_corner_round_under_foot_round_builds(r4, corpus);
        }
    }
}

/// The radius at which the corner round's rail on the side x = -36 meets z = 5 at W:
/// -65 - r4 = -90 + sqrt 205.
const CORNER_ROUND_REACHES_W: f64 = 25.0 - 14.317_821_063_276_353;

/// Width of the certified volume enclosure asked of the round built next to W: asked for 1e-2,
/// the enclosure stops at 0.16 there; the round adds 122.5.
const NEXT_TO_W_ENCLOSURE: f64 = 0.2;

/// The corner round `r4` on [`arm_boss_foot_only`] (or on `b`), with the corpus's fix set
/// (`corpus`) or none, under [`Contract::Proved`]: its result, the fix set restored after.
fn corner_round(b: &Body, r4: f64, corpus: bool) -> Result<(), Error> {
    let e = edge_at(b, [-36.0, -65.0, 2.5]);
    if !corpus {
        session::set_thread_fixes(std::iter::empty::<&str>()).unwrap();
    }
    // proved: the contract under which a blend refuses rounds that cross
    let proved = BlendOptions {
        contract: Contract::Proved,
        ..BlendOptions::default()
    };
    let result = blend_with(b, Blend::Round, &[e], &[r4], &[], Corners::Filled, proved);
    fixes();
    result.map(|_| ())
}

/// Next to r4 = 25 - sqrt 205, where the rail meets z = 5 at W:
/// - 1e-3 past it, R = (-36, -65 - r4, 5) lies past W by 1e-3, 6.2e-4 inside the contact
///   circle, where the side's lobe under the foot round is 3.875e-8 thick (the torus on
///   x = -36 at R is at z = 5 + (5 - sqrt(25 - (23 - rho)^2)), rho = sqrt(18^2 + (y + 90)^2)).
///   The rail and the lobe's edge END there, 3.9e-8 apart: the rounds meet, they do not cross
///   (the corner round's material is at x >= -36, z <= 5, the foot round's at x <= -36,
///   z >= 5; they share only the side's segment from R to W on z = 5).
///   - The round builds, with the corpus's fixes or without them, as it does past W by more
///     ([`a_corner_round_whose_rail_ends_past_the_top_under_the_foot_round_builds`]): valid at
///     the full check, its rails on edges and its cap edge on z = 5 next to R, and the
///     kernel's certified enclosure of its volume, at most [`NEXT_TO_W_ENCLOSURE`] wide, holds
///     the closed form (the input's plus 5 r4^2 (1 - pi / 4)), as does the computed volume.
///     (The round adds 122.5: an enclosure that narrow rules out a round lost, doubled or
///     cut by more than that width.) Main refused it
///     as the rounds crossing at R: the joint there is judged by the vertex, left at W (1e-3
///     from R) under the vertex's tolerance, and the lobe's edge kept 4.6e-5 under a trim that
///     upstream's corner fix (`sm_FixUVTrimCurves`) had moved 1e-3 off its curve, uncovered.
///     Its tolerance now covers its trim (#18166), and the joint at R is seen.
///     The clean-up after filleting leaves the target's own edges and faces (WR-459, the
///     default since #21270): the rail and the lobe's edge then keep their tolerances
///     (4e-5, 4.6e-5) and end at that vertex, left at W with its tolerance 2e-3, 1e-3 from
///     where they meet at R. They meet at the vertex all the same: within its tolerance, the
///     zone in which the topology has its edges meet (`boundary_crossings`), so that is
///     their joint, as the geometry above says, not a crossing (#21270: refused as crossing
///     at R before, the false refusal this test pinned as main's outcome with the corpus).
/// - 5e-6 past it, within the body's tolerance (1e-5), R is not past W: the rail ends on the
///   top's edge as on main, and the round builds valid (its volume, MEASURED, 1.4e-5 from the
///   closed form, which the end at W within the tolerance does not bound).
#[test]
fn a_corner_round_whose_rail_meets_the_top_next_to_w_builds_where_the_rounds_meet() {
    let w = top_end_on_side();
    assert!((-65.0 - CORNER_ROUND_REACHES_W - w).abs() < 1e-12);
    for corpus in [true, false] {
        let r4 = CORNER_ROUND_REACHES_W + 1e-3;
        let b = arm_boss_foot_only();
        let before = b.volume(1e-9).unwrap();
        match (corpus, corner_round(&b, r4, corpus)) {
            (_, Ok(())) => {
                let report = b.check(CheckLevel::All).unwrap();
                assert!(report.is_valid(), "r4 {r4} corpus {corpus}: {report}");
                let expected = before + 5.0 * r4 * r4 * (1.0 - PI / 4.0);
                let after = b.volume(1e-9).unwrap();
                let ry = -65.0 - r4;
                let (k, mut brep) = common::kernel_copy(&b);
                let on_edge = |brep: &solidcore::Brep, p: [f64; 3], within: f64| {
                    let q = solidcore::Vec3::new(p[0], p[1], p[2]);
                    brep.edge_list().iter().any(|&e| {
                        solidcore::query::api_queries::edge_closest_point(&k, brep, e, q)
                            .unwrap()
                            .closest
                            .distance
                            < within
                    })
                };
                for (x, y) in [(-36.0 + r4, -65.0), (-36.0, ry)] {
                    for z in [0.5, 2.5, 4.5] {
                        assert!(
                            on_edge(&brep, [x, y, z], 1e-7),
                            "r4 {r4} corpus {corpus}: no rail through ({x}, {y}, {z})"
                        );
                    }
                }
                // The cap edge lies on z = 5 exactly (the top's plane). The foot round's contact
                // edge on the side, which the round keeps (its material is at z >= 5), runs
                // above it by (23 - rho)^2 / 10: 3.1e-8 a tenth of the way from R to W, so a
                // point there within 1e-9 of an edge is on the cap edge, not on that one.
                let near_r = ry + 0.1 * (w - ry);
                assert!(
                    on_edge(&brep, [-36.0, near_r, 5.0], 1e-9),
                    "r4 {r4} corpus {corpus}: no cap edge through (-36, {near_r}, 5)"
                );
                let e =
                    solidcore::mass::enclosure::enclose_volume(&k, &mut brep, NEXT_TO_W_ENCLOSURE)
                        .unwrap();
                assert!(
                    e.hi - e.lo <= NEXT_TO_W_ENCLOSURE,
                    "r4 {r4}: enclosure [{}, {}] wider than {NEXT_TO_W_ENCLOSURE}",
                    e.lo,
                    e.hi
                );
                assert!(
                    e.lo <= expected && expected <= e.hi,
                    "r4 {r4}: closed form {expected} outside [{}, {}]",
                    e.lo,
                    e.hi
                );
                assert!(
                    e.lo <= after && after <= e.hi,
                    "r4 {r4}: volume {after} outside [{}, {}]",
                    e.lo,
                    e.hi
                );
            }
            (_, other) => panic!("r4 {r4} corpus {corpus}: {other:?}"),
        }
        let r4 = CORNER_ROUND_REACHES_W + 5e-6;
        let b = arm_boss_foot_only();
        corner_round(&b, r4, corpus).unwrap_or_else(|e| panic!("r4 {r4} corpus {corpus}: {e:?}"));
        let report = b.check(CheckLevel::All).unwrap();
        assert!(report.is_valid(), "r4 {r4} corpus {corpus}: {report}");
    }
}

/// A real miss next to the accepted case: [`arm_boss_foot_only`] with a hole 1 x 1 through the
/// side round R = (-36, -80, 5), from x = -37 to -30, so the rail of the corner round r4 = 15
/// meets z = 5 past W but off the side (in the hole), and the hole's edges on the side cross
/// the cap edge's place from W to R. The turned-back end is not taken, and main's outcome
/// stands: refused, with the corpus's fixes as its rail leaving its face (fit 14.49). Without
/// them the round is now refused the same way (the rails-fit judgement is no longer opt-in);
/// the retired upstream path named 10.68 instead, the size at which the rail first passes W.
/// The fit is a closed form: the rail on the side is the line x = -36, y = -65 - r4, z in
/// [0, 5], and the first boundary of the side it meets is the hole's edge y = -79.5 (z 4.5 to
/// 5), at r4 = 14.5; the refusal names that open bound less the fit margin, a thousand of the
/// corner edge's tolerances. That size builds: valid at the full check, and the volume the
/// input's plus the round's section r^2 (1 - pi / 4) times the arm's height 5 (the round's
/// region, y in [-65 - r, -65], stays above the hole's y <= -79.5). MEASURED: with the
/// membership conditions (R inside the side, no edge crossing W to R) removed, the turned-back
/// end is taken and the round without the corpus's fixes fails with status 1001 instead.
#[test]
fn a_corner_round_whose_rail_ends_past_w_off_the_side_is_refused_as_on_main() {
    let r4 = 15.0;
    let b = arm_boss_foot_only();
    let hole = make_box([-37.0, -80.5, 4.5], [7.0, 1.0, 1.0]).unwrap();
    let b = boolean(b, hole, BooleanOp::Subtract).unwrap().unwrap();
    assert!(b.check(CheckLevel::All).unwrap().is_valid());
    assert!(-65.0 - r4 < top_end_on_side());
    match corner_round(&b, r4, true) {
        Err(Error::TooLarge {
            why: Some(RoomReason::RailLeavesFace),
            ..
        }) => {}
        other => panic!("corpus: {other:?}"),
    }
    let e = edge_at(&b, [-36.0, -65.0, 2.5]);
    let (_, kb) = common::kernel_copy(&b);
    let margin = 1000.0 * solidcore::topology::edge::tolerance(&kb, kb.edge_list()[e]).unwrap();
    // the hole's edge y = -79.5 on the side meets the rail y = -65 - r4 at r4 = 14.5
    let want = 79.5 - 65.0 - margin;
    let size = match corner_round(&b, r4, false) {
        Err(Error::TooLarge {
            size,
            why: Some(RoomReason::RailLeavesFace),
            ..
        }) => size,
        other => panic!("no fixes: {other:?}"),
    };
    assert!(
        (size - want).abs() <= 1e-9,
        "no fixes: fit {size}, the closed form 14.5 less the fit margin {margin}: {want}"
    );
    let before = b.volume(1e-9).unwrap();
    corner_round(&b, size, false).unwrap_or_else(|e| panic!("no fixes, r4 {size}: {e:?}"));
    let report = b.check(CheckLevel::All).unwrap();
    assert!(report.is_valid(), "no fixes, r4 {size}: {report}");
    let after = b.volume(1e-9).unwrap();
    let expected = before + 5.0 * size * size * (1.0 - PI / 4.0);
    assert!(
        (after - expected).abs() < 1e-6,
        "no fixes, r4 {size}: volume {after}, expected {expected}"
    );
}

/// A real miss of the segment certificate alone: [`arm_boss_foot_only`] with a slot 1 x 1
/// through the side, y -78.5 to -77.5, z 4.5 to 5.5, from x = -37 to -30, between W
/// (y -75.68) and the rail's end R = (-36, -80, 5) of the corner round r4 = 15. R stays inside
/// the side, but the slot's edges on the side cross the cap edge's place from W to R, so the
/// turned-back end is not taken and main's outcome stands: without the corpus's fixes
/// refused, naming 10.68 (the closed form less the fit margin); with them it fails as on main
/// (status 1001, not asserted beyond failing). MEASURED: with only the certificate removed,
/// the turned-back end is taken and the round without the corpus's fixes fails with status
/// 1001 instead.
#[test]
fn a_corner_round_whose_cap_edge_crosses_a_slot_in_the_side_is_refused_as_on_main() {
    let r4 = 15.0;
    let b = arm_boss_foot_only();
    let slot = make_box([-37.0, -78.5, 4.5], [7.0, 1.0, 1.0]).unwrap();
    let b = boolean(b, slot, BooleanOp::Subtract).unwrap().unwrap();
    assert!(b.check(CheckLevel::All).unwrap().is_valid());
    let w = top_end_on_side();
    assert!(
        -65.0 - r4 < -78.5 && -77.5 < w,
        "the slot lies between W and R"
    );
    assert!(corner_round(&b, r4, true).is_err(), "corpus: built");
    match corner_round(&b, r4, false) {
        Err(Error::TooLarge {
            size, why: None, ..
        }) => assert!(
            size < CORNER_ROUND_REACHES_W && size > CORNER_ROUND_REACHES_W - 1e-4,
            "no fixes: fit {size}, the closed form {CORNER_ROUND_REACHES_W} less the fit margin"
        ),
        other => panic!("no fixes: {other:?}"),
    }
}

/// #17590's part: a box x in [-40, 40], y in [-20, 20], z in [0, 30] with a boss of radius 5.5
/// about (0, -19), from z = 28 to 40, joined across the box's edge y = -20, z = 30. The boss
/// overhangs the side y = -20 and meets it along the line E, x = -[`boss_side_x`], from the top
/// z = 30 down to W at z = 28, where the boss's bottom face starts.
fn box_and_boss() -> Body {
    fixes();
    let b = make_box([-40.0, -20.0, 0.0], [80.0, 40.0, 30.0]).unwrap();
    let boss = solidcore_ops::make_cylinder([0.0, -19.0, 28.0], 5.5, 12.0).unwrap();
    boolean(b, boss, BooleanOp::Union).unwrap().unwrap()
}

/// Where the boss's wall crosses the side y = -20: x = ±sqrt(5.5^2 - 1).
fn boss_side_x() -> f64 {
    (5.5_f64 * 5.5 - 1.0).sqrt()
}

/// What the round `r` on [`box_and_boss`]'s edge y = -20, z = 30 takes, from x = -40 to the
/// boss's wall: its section (the corner's square less the quarter disc about (-20 + r, 30 - r))
/// at angle `phi` from the top, y = -20 + r - r sin phi, is r - r cos phi high and ends at
/// x = -sqrt(5.5^2 - (y + 19)^2) on the wall. Simpson's rule in `phi`, where the integrand is
/// smooth.
fn boss_edge_round_removed(r: f64) -> f64 {
    let f = |phi: f64| {
        let y = -20.0 + r - r * phi.sin();
        let end = -(5.5_f64 * 5.5 - (y + 19.0).powi(2)).sqrt();
        r * (1.0 - phi.cos()) * (end + 40.0) * r * phi.cos()
    };
    let n = 2000;
    let h = PI / 2.0 / f64::from(n);
    let mut sum = f(0.0) + f(PI / 2.0);
    for i in 1..n {
        sum += f64::from(if i % 2 == 1 { 4 } else { 2 }) * f(f64::from(i) * h);
    }
    sum * h / 3.0
}

/// #17590: the box's edge y = -20, z = 30 of [`box_and_boss`] rounded `r`, up to the boss. The
/// round's rail on the side runs at z = 30 - r: up to r = 2 it ends on E; past it, it meets E's
/// line at R = (-[`boss_side_x`], -20, 30 - r), below W, under the boss. The end face there is
/// the boss's wall, a cylinder whose ruling E is, and the edge leaving W on it is the boss's
/// bottom circle, square to E. The round ends on the wall continued below the boss's bottom,
/// with a cap edge from R to W on the side. Before, the end corner's patch on the wall failed:
/// "Fillet Corner Error: Corner Geometry Computation Failure" (status 1001), with the corpus's
/// fix set and with none. Now, with either, from r = 2.05 (a cap edge 0.05 long) to r = 6 (the
/// round's top rail, y = -14, still on the boss, which reaches y = -13.5): valid at the full
/// check, the round one face on its
/// exact cylinder with its rails the lines along x through (x, -20 + r, 30) and (x, -20, 30 - r),
/// the cap edge through R and W, and the volume the input's less [`boss_edge_round_removed`].
/// 10 faces, 24 edges and 16 vertices: the input's 9, 21 and 14 with the round's face; its two
/// rails, its two end curves and the cap edge in place of the rounded edge and E (the cap lies
/// on the boss's wall, one face with it); the rails' four ends, R one of them, in place of the
/// rounded edge's two. The shells are the input's.
///
/// Control: at r = 2 the rail ends at W itself (R not past W), the corner is main's, and the
/// round builds with one edge and one vertex fewer, 10, 23 and 15.
///
/// Only the edge left of the boss: its mirror, right of it, still fails (status 1001) on this
/// boss, on main as here.
#[test]
fn a_round_ending_at_a_boss_across_its_edge_ends_on_the_boss_s_wall_below_its_bottom() {
    for (r, topology) in [
        (2.0, (10, 23, 15)),
        (2.05, BOSS_EDGE_ROUND_TOPOLOGY),
        (3.0, BOSS_EDGE_ROUND_TOPOLOGY),
        (4.0, BOSS_EDGE_ROUND_TOPOLOGY),
        (5.0, BOSS_EDGE_ROUND_TOPOLOGY),
        (6.0, BOSS_EDGE_ROUND_TOPOLOGY),
    ] {
        for corpus in [true, false] {
            assert_round_into_boss_builds(
                box_and_boss,
                boss_side_x(),
                boss_edge_round_removed(r),
                topology,
                (r, corpus, -1.0),
            );
        }
    }
}

/// The topology of [`box_and_boss`] with its edge rounded past r = 2: faces, edges, vertices.
const BOSS_EDGE_ROUND_TOPOLOGY: (usize, usize, usize) = (10, 24, 16);

/// [`box_and_boss`] with a block x in [-5, 5], y in [-24, -16], z in [28, 40] for the boss: the
/// end face is the block's side x = -5, a plane, and the edge leaving W on it is the block's
/// bottom edge along y, square to E, not turned back.
fn box_and_block() -> Body {
    fixes();
    let b = make_box([-40.0, -20.0, 0.0], [80.0, 40.0, 30.0]).unwrap();
    let block = make_box([-5.0, -24.0, 28.0], [10.0, 8.0, 12.0]).unwrap();
    boolean(b, block, BooleanOp::Union).unwrap().unwrap()
}

/// The same with a block for the boss ([`box_and_block`]), up to r = 3.5 (the round's top rail
/// on the block's side, which ends at y = -16): its side x = -5 ends the round, and the round
/// takes its section r^2 (1 - pi / 4) along the 35 from x = -40 (or to x = 40). Before, as with
/// the boss (status 1001), with the corpus's fix set and with none; now built the same way, the
/// input's 12 faces, 30 edges and 20 vertices with the same changes.
#[test]
fn a_round_ending_at_a_block_across_its_edge_ends_on_the_block_s_side_below_its_bottom() {
    for r in [2.5, 3.0, 3.5] {
        for corpus in [true, false] {
            for side in [-1.0, 1.0] {
                assert_round_into_boss_builds(
                    box_and_block,
                    5.0,
                    35.0 * r * r * (1.0 - PI / 4.0),
                    BLOCK_EDGE_ROUND_TOPOLOGY,
                    (r, corpus, side),
                );
            }
        }
    }
}

/// The topology of [`box_and_block`] with its edge rounded past r = 2: faces, edges, vertices.
const BLOCK_EDGE_ROUND_TOPOLOGY: (usize, usize, usize) = (13, 33, 22);

/// #22188: chamfers on [`box_and_block`]'s edge y = -20, z = 30 past the side edge's end at
/// z = 28 (equal, 45-degree distance-angle, and unequal two-distance): before the fix,
/// `rail_passes_side_edge_end` rejected `FilletSurfaceGeneratorType::Linear` and the chamfer
/// failed with status 1001. Each builds valid with the exact prism volume removed, the same
/// 13 faces / 33 edges / 22 vertices topology as the round, and the cap edge on the side.
#[test]
fn a_chamfer_ending_at_a_block_across_its_edge_ends_on_the_block_s_side_below_its_bottom() {
    use solidcore::query::api_queries::edge_closest_point;
    for &(d_top, d_side, angle_45) in &[(2.5, 2.5, false), (2.5, 2.5, true), (1.5, 3.0, false)] {
        for side in [-1.0, 1.0] {
            let what = format!("d_top {d_top} d_side {d_side} angle_45 {angle_45} side {side}");
            let b = box_and_block();
            let before = b.volume(1e-9).unwrap();
            let shells = common::kernel_copy(&b).1.shells().len();
            let e = edge_at(&b, [30.0 * side, -20.0, 30.0]);
            if angle_45 {
                chamfer_sides_facing(
                    &b,
                    &[e],
                    d_top,
                    ChamferSecond::Angle(PI / 4.0),
                    [0.0, 0.0, 1.0],
                )
                .unwrap_or_else(|err| panic!("{what}: {err:?}"));
            } else if (d_top - d_side).abs() < 1e-12 {
                blend(&b, Blend::Chamfer, &[e], &[d_top], &[], Corners::Filled)
                    .unwrap_or_else(|err| panic!("{what}: {err:?}"));
            } else {
                chamfer_sides_facing(
                    &b,
                    &[e],
                    d_top,
                    ChamferSecond::Distance(d_side),
                    [0.0, 0.0, 1.0],
                )
                .unwrap_or_else(|err| panic!("{what}: {err:?}"));
            }
            let report = b.check(CheckLevel::All).unwrap();
            assert!(report.is_valid(), "{what}: {report}");
            let (k, brep) = common::kernel_copy(&b);
            let counts = (
                brep.face_list().len(),
                brep.edge_list().len(),
                brep.vertex_list().len(),
            );
            assert_eq!(
                counts, BLOCK_EDGE_ROUND_TOPOLOGY,
                "{what}: faces, edges, vertices"
            );
            assert_eq!(brep.shells().len(), shells, "{what}: shells");
            let on_edge = |p: [f64; 3]| {
                let q = solidcore::Vec3::new(p[0], p[1], p[2]);
                brep.edge_list().iter().any(|&edge_id| {
                    edge_closest_point(&k, &brep, edge_id, q)
                        .unwrap()
                        .closest
                        .distance
                        < 1e-7
                })
            };
            for along in [35.0, 20.0, 10.0] {
                let x = along * side;
                for p in [[x, -20.0 + d_top, 30.0], [x, -20.0, 30.0 - d_side]] {
                    assert!(on_edge(p), "{what}: no rail through {p:?}");
                }
            }
            let z = (30.0 - d_side + 28.0) / 2.0;
            assert!(
                on_edge([5.0 * side, -20.0, z]),
                "{what}: no cap edge through z {z}"
            );
            let after = b.volume(1e-9).unwrap();
            let expected = before - 35.0 * 0.5 * d_top * d_side;
            assert!(
                (after - expected).abs() < 1e-6,
                "{what}: volume {after}, expected {expected}"
            );
        }
    }
}

/// A body with a vertical convex edge `(10, 10, 5) -> (10, 10, 15)` meeting a recess floor at
/// `z = 5` that drops down to `z = 2` at `y = 10 + side_edge_len`, so the side edge along `x = 10`
/// on the floor has length `side_edge_len` while the wall `x = 10` continues past it to `y = 20`.
fn widened_recess_floor_step(side_edge_len: f64) -> Body {
    fixes();
    let slab = make_box([0.0, 0.0, 0.0], [20.0, 20.0, 5.0]).unwrap();
    let wall = make_box([10.0, 10.0, 5.0], [10.0, 10.0, 10.0]).unwrap();
    let step = boolean(slab, wall, BooleanOp::Union).unwrap().unwrap();
    let deeper = make_box(
        [0.0, 10.0 + side_edge_len, 2.0],
        [10.0, 10.0 - side_edge_len, 3.0],
    )
    .unwrap();
    boolean(step, deeper, BooleanOp::Subtract).unwrap().unwrap()
}

/// #22188: a 1.5 x 45-degree chamfer (and equal / unequal chamfers and a 1.5 round) on a vertical
/// edge meeting a widened recess floor whose side edge stops at `W` shorter than 1.5 mm.
/// At `side_edge_len = 1.0`, the chamfer failed because `rail_passes_side_edge_end` rejected
/// `Linear`. At `side_edge_len = 0.2`, `past = 1.3 > 0.75 * 1.5 = 1.125`, so both chamfers and
/// rounds also failed because `finish_nx1_end` scaled `ext_dist` by `0.1` when `extend_edge` was
/// `None`. Both now build valid solids with the exact closed-form volume.
#[test]
fn a_chamfer_on_an_edge_meeting_a_widened_recess_floor_step_builds() {
    use solidcore::query::api_queries::edge_closest_point;
    for side_edge_len in [1.0, 0.2] {
        let input_vol = 3000.0 - 30.0 * (10.0 - side_edge_len);
        // 1.5 equal chamfer, 1.5 x 45 deg chamfer, 1.5 x 2.5 unequal chamfer, and 1.5 round
        for case in 0..4 {
            let b = widened_recess_floor_step(side_edge_len);
            let before = b.volume(1e-9).unwrap();
            assert!(
                (before - input_vol).abs() < 1e-6,
                "side_edge_len {side_edge_len}: input volume {before} vs {input_vol}"
            );
            let e = edge_at(&b, [10.0, 10.0, 10.0]);
            let (d_x_wall, removed) = match case {
                0 => {
                    blend(&b, Blend::Chamfer, &[e], &[1.5], &[], Corners::Filled)
                        .unwrap_or_else(|err| panic!("len {side_edge_len} equal: {err:?}"));
                    (1.5, 10.0 * 0.5 * 1.5 * 1.5)
                }
                1 => {
                    chamfer_sides_facing(
                        &b,
                        &[e],
                        1.5,
                        ChamferSecond::Angle(PI / 4.0),
                        [-1.0, 0.0, 0.0],
                    )
                    .unwrap_or_else(|err| panic!("len {side_edge_len} 1.5x45: {err:?}"));
                    (1.5, 10.0 * 0.5 * 1.5 * 1.5)
                }
                2 => {
                    chamfer_sides_facing(
                        &b,
                        &[e],
                        2.5,
                        ChamferSecond::Distance(1.5),
                        [-1.0, 0.0, 0.0],
                    )
                    .unwrap_or_else(|err| panic!("len {side_edge_len} 2.5x1.5: {err:?}"));
                    (2.5, 10.0 * 0.5 * 2.5 * 1.5)
                }
                _ => {
                    blend(&b, Blend::Round, &[e], &[1.5], &[], Corners::Filled)
                        .unwrap_or_else(|err| panic!("len {side_edge_len} round: {err:?}"));
                    (1.5, 10.0 * 1.5 * 1.5 * (1.0 - PI / 4.0))
                }
            };
            let report = b.check(CheckLevel::All).unwrap();
            assert!(
                report.is_valid(),
                "len {side_edge_len} case {case}: {report}"
            );
            let after = b.volume(1e-9).unwrap();
            let expected = before - removed;
            assert!(
                (after - expected).abs() < 1e-6,
                "len {side_edge_len} case {case}: volume {after}, expected {expected}"
            );
            let (k, brep) = common::kernel_copy(&b);
            let mid_y = 10.0 + 0.5 * (side_edge_len + d_x_wall);
            let q = solidcore::Vec3::new(10.0, mid_y, 5.0);
            let has_cap_edge = brep.edge_list().iter().any(|&edge_id| {
                edge_closest_point(&k, &brep, edge_id, q)
                    .unwrap()
                    .closest
                    .distance
                    < 1e-7
            });
            assert!(
                has_cap_edge,
                "len {side_edge_len} case {case}: missing cap edge through (10, {mid_y}, 5)"
            );
        }
    }
}

/// An L-step `[0, 20] x [0, 20] x [0, 5]` + `[10, 20] x [0, 20] x [5, 10]` whose front-left corner
/// below the recess floor `z = 5` is bevelled by a plane through `V = (10, 0, 5)` (or its mirror at
/// `V = (10, 20, 5)` when `mirrored`), giving a four-edge vertex at `V` where one side edge lies on
/// the recess floor (`same == false`) and the other has the same convexity as the vertical edge
/// (`same == true`).
fn four_edge_recess_floor_vertex(mirrored: bool) -> Body {
    fixes();
    let slab = make_box([0.0, 0.0, 0.0], [20.0, 20.0, 5.0]).unwrap();
    let wall = make_box([10.0, 0.0, 5.0], [10.0, 20.0, 5.0]).unwrap();
    let step = boolean(slab, wall, BooleanOp::Union).unwrap().unwrap();
    let cutter = make_box([0.0, -20.0, -20.0], [30.0, 40.0, 40.0]).unwrap();
    let s2 = 2.0_f64.sqrt();
    let s3 = 3.0_f64.sqrt();
    let s6 = 6.0_f64.sqrt();
    let matrix = if mirrored {
        [
            [-1.0 / s3, 1.0 / s3, 1.0 / s3, 0.0],
            [-1.0 / s2, -1.0 / s2, 0.0, 0.0],
            [1.0 / s6, -1.0 / s6, 2.0 / s6, 0.0],
            [10.0, 20.0, 5.0, 1.0],
        ]
    } else {
        [
            [-1.0 / s3, -1.0 / s3, 1.0 / s3, 0.0],
            [1.0 / s2, -1.0 / s2, 0.0, 0.0],
            [1.0 / s6, 1.0 / s6, 2.0 / s6, 0.0],
            [10.0, 0.0, 5.0, 1.0],
        ]
    };
    cutter.transform(&matrix).unwrap();
    boolean(step, cutter, BooleanOp::Subtract).unwrap().unwrap()
}

/// #22188: a 1.5 x 45-degree chamfer (and equal chamfer and 1.5 round) on a vertical edge meeting
/// a four-edge vertex on a recess floor where one side edge is concave (on the recess floor) and
/// the other is convex (same convexity as the blended edge). Tested in both orientations so both
/// `same = [true, false]` and `same = [false, true]` are exercised. Before the fix, `finish_nx1_end`
/// picked the same-convexity side (whose face is the fillet's own side face) and ran the early gap
/// check on an unprocessed extended vertex, failing with status 1001.
#[test]
fn a_chamfer_on_an_edge_meeting_a_four_edge_recess_floor_vertex_builds() {
    let expected_input = 3000.0 - 875.0 / 6.0;
    for mirrored in [false, true] {
        let edge_y = if mirrored { 20.0 } else { 0.0 };
        for case in 0..3 {
            let b = four_edge_recess_floor_vertex(mirrored);
            let before = b.volume(1e-9).unwrap();
            assert!(
                (before - expected_input).abs() < 1e-6,
                "mirrored {mirrored}: input volume {before} vs {expected_input}"
            );
            let e = edge_at(&b, [10.0, edge_y, 7.5]);
            let removed = match case {
                0 => {
                    blend(&b, Blend::Chamfer, &[e], &[1.5], &[], Corners::Filled)
                        .unwrap_or_else(|err| panic!("mirrored {mirrored} equal: {err:?}"));
                    5.0 * 0.5 * 1.5 * 1.5
                }
                1 => {
                    chamfer_sides_facing(
                        &b,
                        &[e],
                        1.5,
                        ChamferSecond::Angle(PI / 4.0),
                        [-1.0, 0.0, 0.0],
                    )
                    .unwrap_or_else(|err| panic!("mirrored {mirrored} 1.5x45: {err:?}"));
                    5.0 * 0.5 * 1.5 * 1.5
                }
                _ => {
                    blend(&b, Blend::Round, &[e], &[1.5], &[], Corners::Filled)
                        .unwrap_or_else(|err| panic!("mirrored {mirrored} round: {err:?}"));
                    5.0 * 1.5 * 1.5 * (1.0 - PI / 4.0)
                }
            };
            let report = b.check(CheckLevel::All).unwrap();
            assert!(
                report.is_valid(),
                "mirrored {mirrored} case {case}: {report}"
            );
            let after = b.volume(1e-9).unwrap();
            let expected = before - removed;
            assert!(
                (after - expected).abs() < 1e-6,
                "mirrored {mirrored} case {case}: volume {after}, expected {expected}"
            );
        }
    }
}

/// The box's edge y = -20, z = 30 of `input` rounded `r` on the side `side` of the boss (-1 its
/// left, x < 0; 1 its mirror), with the corpus's fix set (`corpus`) or none: built, valid at the
/// full check, `topology` (faces, edges, vertices) and the input's shells, the round one face on
/// its exact class, the cylinder of radius r along x about (-20 + r, 30 - r), its rails the
/// lines along x through (x, -20 + r, 30) and (x, -20, 30 - r), the cap edge on the side through
/// x = `side` `side_x`, z = (30 - r + 28) / 2, and the volume the input's less `removed`.
fn assert_round_into_boss_builds(
    input: fn() -> Body,
    side_x: f64,
    removed: f64,
    topology: (usize, usize, usize),
    (r, corpus, side): (f64, bool, f64),
) {
    use solidcore::query::api_queries::edge_closest_point;
    use solidcore::surface::surface::SurfaceKind;
    let what = format!("r {r} corpus {corpus} side {side}");
    let b = input();
    let before = b.volume(1e-9).unwrap();
    let shells = common::kernel_copy(&b).1.shells().len();
    let e = edge_at(&b, [30.0 * side, -20.0, 30.0]);
    if !corpus {
        session::set_thread_fixes(std::iter::empty::<&str>()).unwrap();
    }
    let result = blend(&b, Blend::Round, &[e], &[r], &[], Corners::Filled);
    fixes();
    result.unwrap_or_else(|err| panic!("{what}: {err:?}"));
    let report = b.check(CheckLevel::All).unwrap();
    assert!(report.is_valid(), "{what}: {report}");
    let (k, brep) = common::kernel_copy(&b);
    let counts = (
        brep.face_list().len(),
        brep.edge_list().len(),
        brep.vertex_list().len(),
    );
    assert_eq!(counts, topology, "{what}: faces, edges, vertices");
    assert_eq!(brep.shells().len(), shells, "{what}: shells");
    // the round's face is its exact class: the cylinder of radius r along x about
    // (-20 + r, 30 - r)
    let rounds = brep
        .face_list()
        .iter()
        .filter(|&&f| {
            let surface = brep.face(f).unwrap().surface.as_ref().unwrap();
            match surface.kind() {
                SurfaceKind::Cone(c) if c.is_cylinder() && (c.bot_radius() - r).abs() < 1e-9 => {
                    let place = c.rev().position();
                    let (o, a) = (place.origin(), place.z_axis());
                    (o.y - (-20.0 + r)).hypot(o.z - (30.0 - r)) < 1e-9
                        && a.y.hypot(a.z) < 1e-9 * a.length()
                }
                _ => false,
            }
        })
        .count();
    assert_eq!(rounds, 1, "{what}: the round's cylinder");
    let on_edge = |p: [f64; 3]| {
        let q = solidcore::Vec3::new(p[0], p[1], p[2]);
        brep.edge_list().iter().any(|&e| {
            edge_closest_point(&k, &brep, e, q)
                .unwrap()
                .closest
                .distance
                < 1e-7
        })
    };
    for along in [35.0, 20.0, 10.0] {
        let x = along * side;
        for p in [[x, -20.0 + r, 30.0], [x, -20.0, 30.0 - r]] {
            assert!(on_edge(p), "{what}: no rail through {p:?}");
        }
    }
    let z = (30.0 - r + 28.0) / 2.0;
    assert!(
        on_edge([side_x * side, -20.0, z]),
        "{what}: no cap edge through z {z}"
    );
    let after = b.volume(1e-9).unwrap();
    let expected = before - removed;
    assert!(
        (after - expected).abs() < 1e-6,
        "{what}: volume {after}, expected {expected}"
    );
}

/// Edge 28 of `b27` rounded past the R3 round's own radius (3.5 and 4): the ball does not fit
/// the R3 round, and the irregular-ball refusal used to name 2.99 (the round's radius less the
/// fit margin), at which every ball on the edge's faces still lies past the End on x = 0 (2.99
/// is refused there). The refusal now names the smaller of the two, the past-an-end cutoff
/// less the fit margin, and that size builds: a valid solid, smaller than the input, its rails
/// on the faces it rolls on.
#[test]
fn an_irregular_ball_refusal_names_the_past_an_end_size_when_that_is_smaller() {
    let want = b27_edge_28_cutoff() - B27_FIT_MARGIN;
    // the irregular ball's own size: the R3 round's radius less the fit margin
    let irregular = 3.0 - B27_FIT_MARGIN;
    let mut named = Vec::new();
    for r in [3.5, 4.0] {
        match b27_edge_28(r) {
            Err(Error::TooLarge { size, why, .. }) => {
                assert_eq!(why, Some(RoomReason::BallDoesNotFit), "r {r}");
                assert!((size - want).abs() <= 1e-9, "r {r}: {size} against {want}");
                assert!(size < irregular, "r {r}: {size} not below {irregular}");
                named.push(size);
            }
            Err(e) => panic!("r {r}: {e:?}"),
            Ok(_) => panic!("r {r}: built"),
        }
    }
    let (input, _) = b27();
    let before = input.volume(1e-9).unwrap();
    let size = named[0];
    let made = b27_edge_28(size).unwrap_or_else(|e| panic!("r {size}: {e:?}"));
    assert!(made.check(CheckLevel::All).unwrap().is_valid(), "r {size}");
    let after = made.volume(1e-9).unwrap();
    assert!(
        after > 0.0 && after < before,
        "r {size}: {after} of {before}"
    );
    assert_b27_rails_on_faces(&input, &made, size, 1e-5);
}

/// The plane of the oblique end cut in [`obliquely_cut_bar`]: `2x + y - 2z = -12`, its unit
/// normal (2, 1, -2) / 3 pointing out of the bar.
const CUT: ([f64; 3], f64) = ([2.0 / 3.0, 1.0 / 3.0, -2.0 / 3.0], -4.0);

/// A bar x in [0, 6], z in [0, 7.5] along y from -30, its rim x = 0, z = 7.5 rounded R3 (the
/// cylinder about x = 3, z = 4.5 along y, between its tangent edges x = 0, z = 4.5 and x = 3,
/// z = 7.5), its end cut off by the plane [`CUT`] (the half-space beyond removed). The cut
/// meets the R3 round in an ellipse arc from (0, -3, 4.5) to (3, -3, 7.5) that bends back on
/// itself: it reaches y = -0.515 at 135 degrees about the axis.
fn obliquely_cut_bar() -> Body {
    use solidcore_ops::affine::{rotation, then, translation};
    fixes();
    let bar = make_box([0.0, -30.0, 0.0], [6.0, 40.0, 7.5]).unwrap();
    let rim = edge_at(&bar, [0.0, 0.0, 7.5]);
    blend(&bar, Blend::Round, &[rim], &[3.0], &[], Corners::Filled).unwrap();
    // a box whose face y = 0 turns onto the cut's plane: (0, 1, 0) onto the normal n about
    // (0, 1, 0) x n, then out along n to the plane
    let (n, d) = CUT;
    let cutter = make_box([-60.0, 0.0, -60.0], [120.0, 120.0, 120.0]).unwrap();
    let turn = rotation([0.0; 3], [n[2], 0.0, -n[0]], n[1].acos()).unwrap();
    let out = translation([n[0] * d, n[1] * d, n[2] * d]).unwrap();
    cutter.transform(&then(&turn, &out).unwrap()).unwrap();
    boolean(bar, cutter, BooleanOp::Subtract).unwrap().unwrap()
}

/// The index of the edge of `b` passing within 1e-6 of `p`.
fn edge_through(b: &Body, p: [f64; 3]) -> usize {
    use solidcore::query::api_queries::{edge_closest_point, get_edges};
    let (k, brep) = common::kernel_copy(b);
    let q = solidcore::math::Vec3::new(p[0], p[1], p[2]);
    get_edges(&brep)
        .unwrap()
        .iter()
        .position(|&e| {
            edge_closest_point(&k, &brep, e, q)
                .unwrap()
                .closest
                .distance
                <= 1e-6
        })
        .unwrap()
}

/// The ball of radius r touching the R3 round of [`obliquely_cut_bar`] at angle t about its
/// axis and the cut's plane from inside the bar: its centre lies on the radial line at 3 - r
/// from the axis, and r inside the plane, `n . c = d - r`.
fn cut_bar_ball(r: f64, t: f64) -> [f64; 3] {
    let (n, d) = CUT;
    let (x, z) = (3.0 + (3.0 - r) * t.cos(), 4.5 + (3.0 - r) * t.sin());
    [x, (d - r - n[0] * x - n[2] * z) / n[1], z]
}

/// The crease's End planes, in closed form: at its end V on x = 0 (t = 180 degrees) and its
/// end W on z = 7.5 (t = 90), each through the end, square to the crease there, signed
/// positive into the edge's span. The crease is (3 + 3 cos t, y(t), 4.5 + 3 sin t) with
/// y(t) = -12 - 2 x + 2 z; into the span from V it runs as t falls, from W as t rises.
fn cut_bar_end_distances(c: [f64; 3]) -> [f64; 2] {
    let point = |t: f64| {
        let (x, z) = (3.0 + 3.0 * t.cos(), 4.5 + 3.0 * t.sin());
        [x, -12.0 - 2.0 * x + 2.0 * z, z]
    };
    // d/dt of the crease: (-3 sin t, 6 sin t + 6 cos t, 3 cos t)
    let into = |t: f64, sense: f64| {
        let v = [-3.0 * t.sin(), 6.0 * t.sin() + 6.0 * t.cos(), 3.0 * t.cos()];
        let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
        [sense * v[0] / l, sense * v[1] / l, sense * v[2] / l]
    };
    let signed = |t: f64, sense: f64| {
        let (p, u) = (point(t), into(t, sense));
        (c[0] - p[0]) * u[0] + (c[1] - p[1]) * u[1] + (c[2] - p[2]) * u[2]
    };
    [signed(PI, -1.0), signed(PI / 2.0, 1.0)]
}

/// The round of the oblique cut's crease on the R3 round at r 0.3 and 0.4 (#16156). The balls
/// touching the R3 round along its two tangent edges both lie past the End plane at V, by
/// more than 0.1 (closed form), as the past-an-end refusal's first test asks; but the crease
/// bends back on itself, and the ball touching the round at 135 degrees lies on the span's
/// side of both End planes, by more than 0.9: the round has sections between the edge's
/// ends. The refusal used to judge from the two tangent edges' balls alone, assuming the
/// balls between them lie no nearer the span, and refused it (`EndRollsIntoSmoothEnd`). It
/// now refuses only where every ball on the R3 round is proven past the plane, which this is
/// not: the round builds, a valid solid smaller than the bar.
#[test]
fn a_round_whose_balls_bend_back_between_the_ends_is_not_refused_past_an_end() {
    let input = obliquely_cut_bar();
    assert!(input.check(CheckLevel::All).unwrap().is_valid());
    let before = input.volume(1e-9).unwrap();
    let t = 3.0 * PI / 4.0;
    let (x, z) = (3.0 + 3.0 * t.cos(), 4.5 + 3.0 * t.sin());
    let crease = [x, -12.0 - 2.0 * x + 2.0 * z, z];
    for r in [0.3, 0.4] {
        // the premise, in closed form
        let [at_v, _] = cut_bar_end_distances(cut_bar_ball(r, PI));
        let [far_v, _] = cut_bar_end_distances(cut_bar_ball(r, PI / 2.0));
        assert!(at_v < -0.1 && far_v < -0.1, "r {r}: {at_v}, {far_v}");
        let [mid_v, mid_w] = cut_bar_end_distances(cut_bar_ball(r, t));
        assert!(mid_v > 0.9 && mid_w > 0.9, "r {r}: {mid_v}, {mid_w}");
        let body = input.copy().unwrap();
        let e = edge_through(&body, crease);
        match blend(&body, Blend::Round, &[e], &[r], &[], Corners::Filled) {
            Ok(_) => {
                assert!(body.check(CheckLevel::All).unwrap().is_valid(), "r {r}");
                let after = body.volume(1e-9).unwrap();
                assert!(after > 0.0 && after < before, "r {r}: {after} of {before}");
            }
            Err(e) => panic!("r {r}: {e:?}"),
        }
    }
}

/// Where the two lobes of [`two_lobes`] start their circles, as SolidCraft's `solid.cylinder`
/// does (`SEAM_ANGLE`, radians from the x axis).
const LOBE_SEAM: f64 = 0.6131;

/// The lobes' centres and radii: ø82 at (40, 0) and ø60 at (-20, 15).
const LOBES: [([f64; 2], f64); 2] = [([40.0, 0.0], 41.0), ([-20.0, 15.0], 30.0)];

/// The lobes' height.
const LOBE_HEIGHT: f64 = 15.0;

/// SolidCraft #17345's figure-8 boss: the two cylinders of [`LOBES`], each a circle starting at
/// [`LOBE_SEAM`] swept 15 along z, joined.
fn two_lobes() -> Body {
    lobes_seamed([LOBE_SEAM.to_degrees(); 2])
}

/// The cylinder of radius `r` about `(cx, cy)` from `z0` to `z1`, its circle starting at
/// `seam_deg` (degrees about its axis).
fn seamed_cylinder(([cx, cy], r): ([f64; 2], f64), seam_deg: f64, z0: f64, z1: f64) -> Body {
    use solidcore::curve::curve::Curve as KCurve;
    use solidcore::math::Point3;
    use solidcore::primitive::api_curves::create_arc;
    use solidcore::sweep::api_brep::create_linear_sweep;
    common::built(|k| {
        let circle = KCurve::from(create_arc(
            Point3::new(cx, cy, z0),
            r,
            seam_deg - 360.0,
            seam_deg,
        )?);
        create_linear_sweep(
            k,
            &[Some(&circle)],
            solidcore::Vec3::new(0.0, 0.0, 1.0),
            z1 - z0,
            true,
        )
    })
}

/// The cylinders of [`LOBES`], each a circle starting at `seams_deg` (degrees about its axis)
/// swept 15 along z, joined.
fn lobes_seamed(seams_deg: [f64; 2]) -> Body {
    boolean(
        seamed_cylinder(LOBES[0], seams_deg[0], 0.0, LOBE_HEIGHT),
        seamed_cylinder(LOBES[1], seams_deg[1], 0.0, LOBE_HEIGHT),
        BooleanOp::Union,
    )
    .unwrap()
    .unwrap()
}

/// The point where the lobes' circles cross on the side the round of #17345 is on (above the
/// line through the centres, (8.036, 25.677)).
fn lobe_crossing() -> (f64, f64) {
    let p = circles_cross(LOBES[0], LOBES[1]);
    assert!(
        (p.0 - 8.03584).abs() < 1e-4 && (p.1 - 25.67669).abs() < 1e-4,
        "{p:?}"
    );
    p
}

/// The point where the circles `a` and `b` cross to the right of the line from `a`'s centre to
/// `b`'s (looking along it from `a`), in closed form.
fn circles_cross(a: ([f64; 2], f64), b: ([f64; 2], f64)) -> (f64, f64) {
    let ([ax, ay], ra) = a;
    let ([bx, by], rb) = b;
    let (dx, dy) = (bx - ax, by - ay);
    let d = (dx * dx + dy * dy).sqrt();
    let along = (d * d + ra * ra - rb * rb) / (2.0 * d);
    let h = (ra * ra - along * along).sqrt();
    let (ux, uy) = (dx / d, dy / d);
    (ax + ux * along + uy * h, ay + uy * along - ux * h)
}

/// The ball of radius `r` of the round at [`lobe_crossing`]: its centre, outside both lobes and
/// touching each (`r1 + r` from the first centre, `r2 + r` from the second).
fn lobe_ball(r: f64) -> (f64, f64) {
    let ([ax, ay], ra) = LOBES[0];
    let ([bx, by], rb) = LOBES[1];
    circles_cross(([ax, ay], ra + r), ([bx, by], rb + r))
}

/// The area the round `r` adds to the lobes' section: the region bounded by the first lobe's
/// circle from the crossing to the ball's contact, the ball from there to its contact on the
/// second lobe, and the second lobe's circle back to the crossing, by Green's theorem over its
/// three arcs (each the shorter way round).
fn lobe_round_area(r: f64) -> f64 {
    let p = lobe_crossing();
    let c = lobe_ball(r);
    let [([ax, ay], ra), ([bx, by], rb)] = LOBES;
    let angle = |cx: f64, cy: f64, q: (f64, f64)| (q.1 - cy).atan2(q.0 - cx);
    let short = |from: f64, to: f64| {
        let mut d = to - from;
        while d > PI {
            d -= 2.0 * PI;
        }
        while d < -PI {
            d += 2.0 * PI;
        }
        from + d
    };
    let arc = |cx: f64, cy: f64, rad: f64, from: f64, to: f64| {
        0.5 * (rad * rad * (to - from) + cx * rad * (to.sin() - from.sin())
            - cy * rad * (to.cos() - from.cos()))
    };
    // the contacts: on each lobe's circle towards the ball's centre
    let toward = |cx: f64, cy: f64, rad: f64| {
        let (dx, dy) = (c.0 - cx, c.1 - cy);
        let l = (dx * dx + dy * dy).sqrt();
        (cx + rad * dx / l, cy + rad * dy / l)
    };
    let t1 = toward(ax, ay, ra);
    let t2 = toward(bx, by, rb);
    let (p1, q1) = (angle(ax, ay, p), angle(ax, ay, t1));
    let (s1, s2) = (angle(c.0, c.1, t1), angle(c.0, c.1, t2));
    let (q2, p2) = (angle(bx, by, t2), angle(bx, by, p));
    let total = arc(ax, ay, ra, p1, short(p1, q1))
        + arc(c.0, c.1, r, s1, short(s1, s2))
        + arc(bx, by, rb, q2, short(q2, p2));
    total.abs()
}

/// The lobes' section area: both discs less their lens, in closed form.
fn lobes_area() -> f64 {
    let [([ax, ay], ra), ([bx, by], rb)] = LOBES;
    let d = ((bx - ax).powi(2) + (by - ay).powi(2)).sqrt();
    let lens = ra * ra * ((d * d + ra * ra - rb * rb) / (2.0 * d * ra)).acos()
        + rb * rb * ((d * d + rb * rb - ra * ra) / (2.0 * d * rb)).acos()
        - 0.5 * ((-d + ra + rb) * (d + ra - rb) * (d - ra + rb) * (d + ra + rb)).sqrt();
    PI * (ra * ra + rb * rb) - lens
}

#[test]
fn the_lobe_round_area_oracle_agrees_with_the_triangle_less_its_sectors() {
    // a second closed form: the triangle of the three centres less the three sectors at its
    // corners, plus the half of the lens on the ball's side (counted in both lobes' sectors)
    let [([ax, ay], ra), ([bx, by], rb)] = LOBES;
    let d = ((bx - ax).powi(2) + (by - ay).powi(2)).sqrt();
    let lens = PI * (ra * ra + rb * rb) - lobes_area();
    // the R10 ball #17345 names
    let (cx, cy) = lobe_ball(10.0);
    assert!(
        (cx - 10.115).abs() < 1e-3 && (cy - 41.327).abs() < 1e-3,
        "({cx}, {cy})"
    );
    for r in [2.0, 5.0, 6.02, 6.03, 10.0, 14.0] {
        let (cx, cy) = lobe_ball(r);
        let (e, f) = (ra + r, rb + r);
        assert!(((cx - ax).hypot(cy - ay) - e).abs() < 1e-12);
        assert!(((cx - bx).hypot(cy - by) - f).abs() < 1e-12);
        let triangle = 0.5 * ((bx - ax) * (cy - ay) - (cx - ax) * (by - ay)).abs();
        let corner = |opposite: f64, s: f64, t: f64| {
            ((s * s + t * t - opposite * opposite) / (2.0 * s * t)).acos()
        };
        let (alpha, beta, gamma) = (corner(f, d, e), corner(e, d, f), corner(d, e, f));
        let sectors = 0.5 * (ra * ra * alpha + rb * rb * beta + r * r * gamma);
        let expected = triangle - sectors + 0.5 * lens;
        let got = lobe_round_area(r);
        assert!(
            (got - expected).abs() < 1e-9 * expected.max(1.0),
            "r {r}: {got} vs {expected}"
        );
    }
}

/// The angle about the second lobe's axis, in degrees within [0, 360), where the ball of radius
/// `r` touches it.
fn second_lobe_contact_deg(r: f64) -> f64 {
    let (cx, cy) = lobe_ball(r);
    let ([bx, by], _) = LOBES[1];
    (cy - by).atan2(cx - bx).to_degrees().rem_euclid(360.0)
}

/// The radius at which the ball's contact on the second lobe reaches its seam: there the
/// contact angle is [`LOBE_SEAM`], by bisection on the closed form (the angle grows with `r`).
fn seam_reach() -> f64 {
    let seam = LOBE_SEAM.to_degrees();
    let (mut lo, mut hi) = (1.0_f64, 10.0_f64);
    assert!(second_lobe_contact_deg(lo) < seam && second_lobe_contact_deg(hi) > seam);
    for _ in 0..200 {
        let mid = 0.5 * (lo + hi);
        if second_lobe_contact_deg(mid) < seam {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    lo
}

/// SolidCraft #17345 (#15767): a lone round along the straight edge where two joined cylinders
/// meet, the ø60 lobe's seam (35.13 deg about its axis) a little past the edge on that lobe's
/// wall. Past r = 6.0024 (the closed form's `seam_reach`) the ball's contact on that wall lies
/// past the seam all along the edge: the face beside the round is the sliver between the edge
/// and the seam, and the ball rolls over the seam onto the rest of the wall. On main the round
/// was refused as larger than its faces, naming 5.992, at every radius past it. Each radius,
/// below the seam's reach and above it (the ball solved past the seam stopped between 6.02 and
/// 6.03 on the closed surface's domain end), builds, checks valid at every level, and has the
/// closed-form volume: the lobes' section plus the round's area, times the height. The round's
/// rails lie at the ball's contacts.
///
/// Also every radius from 6.001 to 6.015 in steps of 0.001, and 6.025 to 6.04: just past the
/// reach the rail on the ø60 wall lies 0.0005 to 0.03 past the seam, and the round's cap edges
/// run along the body's cap edge between the seam's vertex and the rail's for that short
/// length. That run was dropped as a tangent touch, being under a twentieth of the cap edge, and
/// the piece then classified by a point test on the cap's boundary: by rounding outside the
/// cap, when the trim collected the whole body and was refused as leaking ("the faces the rails
/// enclose reach past the rails"; 12 to 22 of the radii 5.980 to 6.040, both modes, depending on
/// rounding). Past the reach the round takes the seam's edge with the sliver: no edge runs through
/// the seam's line, and the body has 6 faces (each lobe's wall in two, less the sliver, the round
/// and the caps); below it, the seam's edge stays and the sliver's remainder makes 7.
#[test]
fn a_lone_round_between_two_cylinders_rolls_over_the_seam_of_one() {
    use solidcore::query::api_queries::edge_closest_point;
    let reach = seam_reach();
    assert!((reach - 6.0024).abs() < 1e-4, "seam reach {reach}");
    let band = (1..=15).map(|i| 6.0 + 0.001 * f64::from(i));
    let radii: Vec<f64> = [2.0, 5.0, 5.99, 6.0, 6.01, 6.02, 6.03, 6.1, 8.0, 10.0, 14.0]
        .into_iter()
        .chain(band)
        .chain([6.025, 6.035, 6.04])
        .collect();
    let seam = on_second_lobe(LOBE_SEAM.to_degrees());
    for corpus in [true, false] {
        if corpus {
            fixes();
        } else {
            session::set_thread_fixes(std::iter::empty::<&str>()).unwrap();
        }
        for &r in &radii {
            let b = two_lobes();
            let (px, py) = lobe_crossing();
            let e = edge_at(&b, [px, py, LOBE_HEIGHT / 2.0]);
            blend(&b, Blend::Round, &[e], &[r], &[], Corners::Filled)
                .unwrap_or_else(|err| panic!("corpus {corpus} r {r}: {err:?}"));
            let report = b.check(CheckLevel::All).unwrap();
            assert!(report.is_valid(), "corpus {corpus} r {r}: {report}");
            let v = b.volume(1e-9).unwrap();
            let expected = LOBE_HEIGHT * (lobes_area() + lobe_round_area(r));
            assert!(
                (v - expected).abs() < 1e-6,
                "corpus {corpus} r {r}: volume {v}, expected {expected}"
            );
            // a rail through each contact, at three heights
            let (cx, cy) = lobe_ball(r);
            let (k, brep) = common::kernel_copy(&b);
            for ([lx, ly], lr) in LOBES {
                let (dx, dy) = (cx - lx, cy - ly);
                let l = dx.hypot(dy);
                let (tx, ty) = (lx + lr * dx / l, ly + lr * dy / l);
                for z in [0.5, 7.5, 14.5] {
                    let q = solidcore::Vec3::new(tx, ty, z);
                    let on_edge = brep.edge_list().iter().any(|&e| {
                        edge_closest_point(&k, &brep, e, q)
                            .unwrap()
                            .closest
                            .distance
                            < 1e-7
                    });
                    assert!(
                        on_edge,
                        "corpus {corpus} r {r}: no rail through ({tx}, {ty}, {z})"
                    );
                }
            }
            // the seam's edge, kept below the reach and taken by the round past it
            let q = solidcore::Vec3::new(seam.0, seam.1, 7.5);
            let seam_edge = brep.edge_list().iter().any(|&e| {
                edge_closest_point(&k, &brep, e, q)
                    .unwrap()
                    .closest
                    .distance
                    < 1e-7
            });
            assert_eq!(seam_edge, r < reach, "corpus {corpus} r {r}: seam edge");
            let faces = if r < reach { 7 } else { 6 };
            assert_eq!(brep.face_list().len(), faces, "corpus {corpus} r {r}");
        }
    }
}

/// The point at `deg` degrees about the second lobe's axis, on its wall.
fn on_second_lobe(deg: f64) -> (f64, f64) {
    let ([bx, by], rb) = LOBES[1];
    let a = deg.to_radians();
    (bx + rb * a.cos(), by + rb * a.sin())
}

/// A sharp edge where the seam was (#17345's lobes, a real defect next to the seam's
/// rollover): the ø60 lobe seamed at 200 deg, away from the round, and its wall cut flat along
/// the chord from 35.13 deg (where its seam was) to 95 deg about its axis. The round's face on
/// that wall is again the sliver from the edge to 35.13 deg, now ending in a sharp edge on the
/// flat (the chord turns 30 deg from the wall). Past the radius at which the ball's contact on
/// the wall reaches that edge, the wall continued no longer carries the ball and nothing rolls
/// over: the round is refused as its rail leaving its face, as before. Below it, it builds with
/// the lobes' closed-form volume less the cut segment.
#[test]
fn a_lone_round_between_two_cylinders_does_not_roll_over_a_sharp_edge() {
    fixes();
    let (a1, a2) = (LOBE_SEAM.to_degrees(), 95.0_f64);
    let (p1, p2) = (on_second_lobe(a1), on_second_lobe(a2));
    // the cutter: the region beyond the chord, swept through the lobes' height
    let mid = 0.5 * (a1 + a2);
    let out = (mid.to_radians().cos() * 30.0, mid.to_radians().sin() * 30.0);
    let quad = [
        p1,
        p2,
        (p2.0 + out.0, p2.1 + out.1),
        (p1.0 + out.0, p1.1 + out.1),
    ];
    let cutter = common::built(|k| {
        use solidcore::curve::curve::Curve as KCurve;
        use solidcore::math::Point3;
        use solidcore::primitive::api_curves::create_line_segment;
        use solidcore::sweep::api_brep::create_linear_sweep;
        let sides: Vec<KCurve> = (0..4)
            .map(|i| {
                let (a, b) = (quad[i], quad[(i + 1) % 4]);
                KCurve::from(
                    create_line_segment(Point3::new(a.0, a.1, -1.0), Point3::new(b.0, b.1, -1.0))
                        .unwrap(),
                )
            })
            .collect();
        let refs: Vec<Option<&KCurve>> = sides.iter().map(Some).collect();
        create_linear_sweep(
            k,
            &refs,
            solidcore::Vec3::new(0.0, 0.0, 1.0),
            LOBE_HEIGHT + 2.0,
            true,
        )
    });
    let input = boolean(
        lobes_seamed([LOBE_SEAM.to_degrees(), 200.0]),
        cutter,
        BooleanOp::Subtract,
    )
    .unwrap()
    .unwrap();
    assert!(input.check(CheckLevel::All).unwrap().is_valid());
    // the segment the chord cuts off the ø60 disc
    let theta = (a2 - a1).to_radians();
    let segment = 0.5 * 30.0 * 30.0 * (theta - theta.sin());
    let reach = seam_reach();
    let (px, py) = lobe_crossing();
    for r in [5.0, 5.99] {
        assert!(r < reach);
        let b = input.copy().unwrap();
        let e = edge_at(&b, [px, py, LOBE_HEIGHT / 2.0]);
        blend(&b, Blend::Round, &[e], &[r], &[], Corners::Filled)
            .unwrap_or_else(|err| panic!("r {r}: {err:?}"));
        assert!(b.check(CheckLevel::All).unwrap().is_valid(), "r {r}");
        let v = b.volume(1e-9).unwrap();
        let expected = LOBE_HEIGHT * (lobes_area() - segment + lobe_round_area(r));
        assert!(
            (v - expected).abs() < 1e-6,
            "r {r}: volume {v}, expected {expected}"
        );
    }
    for r in [6.1, 8.0, 10.0] {
        assert!(r > reach);
        let b = input.copy().unwrap();
        let e = edge_at(&b, [px, py, LOBE_HEIGHT / 2.0]);
        // the size named is the radius at which the contact reaches the sharp edge, open: at
        // most the closed form's reach and within the fit margin of it
        match blend(&b, Blend::Round, &[e], &[r], &[], Corners::Filled) {
            Err(Error::TooLarge {
                size,
                why: Some(RoomReason::RailLeavesFace),
                ..
            }) => assert!(
                size <= reach && size > reach - 0.02,
                "r {r}: named {size}, the contact reaches the edge at {reach}"
            ),
            other => panic!("r {r}: {other:?}"),
        }
    }
}

/// #17345's lobes as a pocket: the box [-60, 90] x [-50, 55] x [0, 15] less both cylinders
/// (each run through it, seamed at 35.13 deg about its axis). The edge where the two hole walls
/// cross at (8.036, 25.677) is convex; its round removes the region between the walls and the
/// ball, the same region the boss's round adds (the ball outside both circles, touching each),
/// and past the seam's reach its contact on the ø60 hole wall rolls over that wall's seam. Each
/// radius builds, checks valid at every level, and has the closed-form volume: the box less the
/// lobes' section and the round's area, times the height.
#[test]
fn a_lone_round_between_two_hole_walls_rolls_over_the_seam_of_one() {
    let reach = seam_reach();
    let (px, py) = lobe_crossing();
    let seam = LOBE_SEAM.to_degrees();
    for corpus in [true, false] {
        if corpus {
            fixes();
        } else {
            session::set_thread_fixes(std::iter::empty::<&str>()).unwrap();
        }
        let mut input = make_box([-60.0, -50.0, 0.0], [150.0, 105.0, LOBE_HEIGHT]).unwrap();
        for lobe in LOBES {
            let hole = seamed_cylinder(lobe, seam, -1.0, LOBE_HEIGHT + 1.0);
            input = boolean(input, hole, BooleanOp::Subtract).unwrap().unwrap();
        }
        assert!(input.check(CheckLevel::All).unwrap().is_valid());
        // The round removes 15 times its area from whatever the input measures: the input's
        // own measured volume differs from 15 (150 x 105 - lobes) by ~4e-3 (3.5e-8 relative,
        // the boolean's walls), which is not this round's business, so the round is judged on
        // the difference alone. A convex round's surface is fitted to the kernel's
        // approximation tolerance (5e-6), so the removed volume may differ from the exact
        // ball's by at most that times the round's area (under 15 r pi); it measures 2.6e-6 at
        // r 5 and 2.2e-4 at r 10, the same at any volume accuracy, the same on either side of
        // the seam, and bit-identical on main at R5 and at R8, past the seam (built there with
        // the corpus's fixes): it is the fit's, not this change's.
        let before = input.volume(1e-9).unwrap();
        for r in [5.0, 5.99, 6.03, 8.0, 10.0] {
            assert_eq!(r > reach, r > 6.0, "r {r}");
            let b = input.copy().unwrap();
            let e = edge_at(&b, [px, py, LOBE_HEIGHT / 2.0]);
            blend(&b, Blend::Round, &[e], &[r], &[], Corners::Filled)
                .unwrap_or_else(|err| panic!("corpus {corpus} r {r}: {err:?}"));
            let report = b.check(CheckLevel::All).unwrap();
            assert!(report.is_valid(), "corpus {corpus} r {r}: {report}");
            let v = b.volume(1e-9).unwrap();
            let expected = LOBE_HEIGHT * lobe_round_area(r);
            assert!(
                (before - v - expected).abs() < 5e-6 * LOBE_HEIGHT * r * PI,
                "corpus {corpus} r {r}: removed {}, expected {expected}",
                before - v
            );
        }
    }
}

/// The ø60 lobe of #17345 on a slab: the cylinder of radius 30 about (-20, 15), seamed at
/// 35.13 deg, joined to the box [-60, 60] x [-20, 30] x [0, 15], whose top face y = 30 meets
/// the wall at 30 deg about the axis, (5.981, 30). The lone round along that edge, between a
/// plane and a cylinder: its ball, centre (bx, 30 + r) with (bx + 20)^2 + (15 + r)^2 = (30 +
/// r)^2, touches the top at (bx, 30) and the wall towards its centre; past the radius at which
/// that contact reaches the seam, it rolls over it.
fn slab_round(r: f64) -> ((f64, f64), (f64, f64), (f64, f64)) {
    let bx = -20.0 + ((30.0 + r).powi(2) - (15.0 + r).powi(2)).sqrt();
    let centre = (bx, 30.0 + r);
    let (dx, dy) = (bx + 20.0, 15.0 + r);
    let l = dx.hypot(dy);
    (
        (bx, 30.0),
        (-20.0 + 30.0 * dx / l, 15.0 + 30.0 * dy / l),
        centre,
    )
}

/// The area the slab's round adds: the region bounded by the top from the edge's point to the
/// ball's contact, the ball to its contact on the wall, and the wall back to the edge's point,
/// by Green's theorem over its line and its two arcs.
fn slab_round_area(r: f64) -> f64 {
    let p = (-20.0 + 30.0 * 30.0_f64.to_radians().cos(), 30.0);
    let (t1, t2, c) = slab_round(r);
    let angle = |cx: f64, cy: f64, q: (f64, f64)| (q.1 - cy).atan2(q.0 - cx);
    let short = |from: f64, to: f64| {
        let mut d = to - from;
        while d > PI {
            d -= 2.0 * PI;
        }
        while d < -PI {
            d += 2.0 * PI;
        }
        from + d
    };
    let arc = |cx: f64, cy: f64, rad: f64, from: f64, to: f64| {
        0.5 * (rad * rad * (to - from) + cx * rad * (to.sin() - from.sin())
            - cy * rad * (to.cos() - from.cos()))
    };
    let (s1, s2) = (angle(c.0, c.1, t1), angle(c.0, c.1, t2));
    let (q2, p2) = (angle(-20.0, 15.0, t2), angle(-20.0, 15.0, p));
    let total = 0.5 * (p.0 * t1.1 - t1.0 * p.1)
        + arc(c.0, c.1, r, s1, short(s1, s2))
        + arc(-20.0, 15.0, 30.0, q2, short(q2, p2));
    total.abs()
}

#[test]
fn a_lone_round_between_a_plane_and_a_cylinder_rolls_over_the_seam() {
    let seam = LOBE_SEAM.to_degrees();
    // the radius at which the wall contact reaches the seam, by bisection on the closed form
    let deg = |r: f64| {
        let (_, t2, _) = slab_round(r);
        (t2.1 - 15.0).atan2(t2.0 + 20.0).to_degrees()
    };
    let (mut lo, mut hi) = (0.5_f64, 20.0_f64);
    assert!(deg(lo) < seam && deg(hi) > seam);
    for _ in 0..200 {
        let mid = 0.5 * (lo + hi);
        if deg(mid) < seam {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    let reach = lo;
    assert!(reach > 3.0 && reach < 8.0, "reach {reach}");
    // the area oracle against a fine polygon of the same boundary
    for r in [2.0, 8.0] {
        let p = (-20.0 + 30.0 * 30.0_f64.to_radians().cos(), 30.0);
        let (t1, t2, c) = slab_round(r);
        let n = 20000;
        let mut pts = vec![p, t1];
        let (b0, b1) = (
            (t1.1 - c.1).atan2(t1.0 - c.0),
            (t2.1 - c.1).atan2(t2.0 - c.0),
        );
        for i in 1..=n {
            let a = b0 + (b1 - b0) * f64::from(i) / f64::from(n);
            pts.push((c.0 + r * a.cos(), c.1 + r * a.sin()));
        }
        let (w0, w1) = ((t2.1 - 15.0).atan2(t2.0 + 20.0), 30.0_f64.to_radians());
        for i in 1..n {
            let a = w0 + (w1 - w0) * f64::from(i) / f64::from(n);
            pts.push((-20.0 + 30.0 * a.cos(), 15.0 + 30.0 * a.sin()));
        }
        let mut twice = 0.0;
        for (i, q) in pts.iter().enumerate() {
            let w = pts[(i + 1) % pts.len()];
            twice += q.0 * w.1 - w.0 * q.1;
        }
        let polygon = twice.abs() / 2.0;
        let got = slab_round_area(r);
        assert!(
            (got - polygon).abs() < 1e-5 * got,
            "r {r}: {got} vs {polygon}"
        );
    }
    // the slab and the disc's segment above y = 30, h = 15 above the centre
    let segment = 900.0 * 0.5_f64.acos() - 15.0 * (900.0_f64 - 225.0).sqrt();
    let section = 120.0 * 50.0 + segment;
    let edge = (-20.0 + 30.0 * 30.0_f64.to_radians().cos(), 30.0);
    for corpus in [true, false] {
        if corpus {
            fixes();
        } else {
            session::set_thread_fixes(std::iter::empty::<&str>()).unwrap();
        }
        let input = boolean(
            make_box([-60.0, -20.0, 0.0], [120.0, 50.0, LOBE_HEIGHT]).unwrap(),
            seamed_cylinder(([-20.0, 15.0], 30.0), seam, 0.0, LOBE_HEIGHT),
            BooleanOp::Union,
        )
        .unwrap()
        .unwrap();
        for r in [2.0, reach - 0.01, reach + 0.02, 8.0, 10.0] {
            let b = input.copy().unwrap();
            let e = edge_at(&b, [edge.0, edge.1, LOBE_HEIGHT / 2.0]);
            let built = blend(&b, Blend::Round, &[e], &[r], &[], Corners::Filled);
            // Without the corpus fixes, a contact within 0.03 past the seam is still refused, as
            // the blend failure matched exactly here (`SM_ERR`, "the faces the rails enclose
            // reach past the rails"; main refused every r past the reach here, in both modes):
            // a refusal and never a wrong body. Everything else must build.
            if !corpus
                && r > reach
                && r < reach + 0.03
                && matches!(
                    &built,
                    Err(Error::BlendFailed { status: 1001, phrase, .. })
                        // the kernel's statement, then what the session noted of the step
                        if phrase.starts_with("trim_leaks: the faces the rails enclose reach past the rails")
                )
            {
                continue;
            }
            built.unwrap_or_else(|err| panic!("corpus {corpus} r {r}: {err:?}"));
            let report = b.check(CheckLevel::All).unwrap();
            assert!(report.is_valid(), "corpus {corpus} r {r}: {report}");
            let v = b.volume(1e-9).unwrap();
            let expected = LOBE_HEIGHT * (section + slab_round_area(r));
            assert!(
                (v - expected).abs() < 1e-6,
                "corpus {corpus} r {r}: volume {v}, expected {expected}"
            );
        }
    }
}

/// The area an R `r` round takes from the corner where the disc of radius `big` about the
/// origin meets the line y = `h` (0 < h < big), the corner at (sqrt(big^2 - h^2), h): the region
/// bounded by the line from the corner to the ball's contact, the ball to its contact on the
/// circle, and the circle back to the corner, by Green's theorem over its line and two arcs.
fn disc_corner_round_area(big: f64, h: f64, r: f64) -> f64 {
    let corner = ((big * big - h * h).sqrt(), h);
    let centre = (((big - r).powi(2) - (h - r).powi(2)).sqrt(), h - r);
    let t1 = (centre.0, h);
    let l = (big - r) / big;
    let t2 = (centre.0 / l, centre.1 / l);
    let angle = |cx: f64, cy: f64, q: (f64, f64)| (q.1 - cy).atan2(q.0 - cx);
    let short = |from: f64, to: f64| {
        let mut d = to - from;
        while d > PI {
            d -= 2.0 * PI;
        }
        while d < -PI {
            d += 2.0 * PI;
        }
        from + d
    };
    let arc = |cx: f64, cy: f64, rad: f64, from: f64, to: f64| {
        0.5 * (rad * rad * (to - from) + cx * rad * (to.sin() - from.sin())
            - cy * rad * (to.cos() - from.cos()))
    };
    let (s1, s2) = (angle(centre.0, centre.1, t1), angle(centre.0, centre.1, t2));
    let (q2, p2) = (angle(0.0, 0.0, t2), angle(0.0, 0.0, corner));
    let total = 0.5 * (corner.0 * t1.1 - t1.0 * corner.1)
        + arc(centre.0, centre.1, r, s1, short(s1, s2))
        + arc(0.0, 0.0, big, q2, short(q2, p2));
    total.abs()
}

/// #17349's cover, its first fillet (`CornerR18`): R18 on the four straight edges where a disc
/// of radius 100, seamed at 35.13 deg as SolidCraft seams its cylinders, meets the flats
/// y = +-60. The (80, 60) round's contact on the disc passes its seam, 1.74 deg from the edge.
/// A round of a blend of several is proposed rolled over a smooth far edge only where its
/// acceptance after the trace can prove it, which a rail rolled across a seam onto the rest of
/// its surface is not: wrapping that proposal's sections across the seam refused this fillet
/// (`RailLeavesFace`), where the rollover recovery after the trace builds it, as on main.
#[test]
fn four_rounds_on_a_seamed_disc_build_across_its_seam() {
    fixes();
    let mut input = seamed_cylinder(([0.0, 0.0], 100.0), LOBE_SEAM.to_degrees(), 0.0, 30.0);
    for y in [60.0, -120.0] {
        let cut = make_box([-110.0, y, -5.0], [220.0, 60.0, 40.0]).unwrap();
        input = boolean(input, cut, BooleanOp::Subtract).unwrap().unwrap();
    }
    // the premise: the (80, 60) round's contact on the disc lies past the seam
    let centre_angle = (42.0_f64 / 82.0).asin();
    assert!(centre_angle < LOBE_SEAM && 0.6_f64.atan2(0.8) > LOBE_SEAM);
    let corners = [[80.0, 60.0], [-80.0, 60.0], [80.0, -60.0], [-80.0, -60.0]];
    let b = input.copy().unwrap();
    let edges: Vec<usize> = corners
        .iter()
        .map(|c| edge_at(&b, [c[0], c[1], 15.0]))
        .collect();
    blend(&b, Blend::Round, &edges, &[18.0; 4], &[], Corners::Filled)
        .unwrap_or_else(|err| panic!("{err:?}"));
    let report = b.check(CheckLevel::All).unwrap();
    assert!(report.is_valid(), "{report}");
    // the band |y| <= 60 of the disc, less the four corners' rounds; a convex round's surface
    // is fitted to the kernel's approximation tolerance (5e-6), so the volume may differ from
    // the exact balls' by at most that times the four rounds' area (each under 30 x 18 pi)
    let band = 2.0 * (60.0 * 80.0 + 10000.0 * 0.6_f64.asin());
    let expected = 30.0 * (band - 4.0 * disc_corner_round_area(100.0, 60.0, 18.0));
    let v = b.volume(1e-9).unwrap();
    assert!(
        (v - expected).abs() < 5e-6 * 4.0 * 30.0 * 18.0 * PI,
        "volume {v}, expected {expected}"
    );
}

/// SolidCraft #18411's part: a block 56.5 x 10 x 27.5 with a pocket 28.2 by 11.8 and 4 deep cut
/// into its top y = 10, the pocket's two floor edges along z (x = 12.8 and x = 41, y = 6, 11.8
/// long) rounded R3. Each of those walls keeps a 1 wide flat (y = 9 to 10) above its floor
/// round, which runs on into it tangentially.
fn pocket_with_floor_rounds() -> Body {
    let block = make_box([1.5, 0.0, 0.0], [56.5, 10.0, 27.5]).unwrap();
    let pocket = make_box([12.8, 6.0, 7.4], [28.2, 5.0, 11.8]).unwrap();
    let b = boolean(block, pocket, BooleanOp::Subtract)
        .unwrap()
        .unwrap();
    let floor: Vec<usize> = [12.8, 41.0]
        .iter()
        .map(|&x| edge_at(&b, [x, 6.0, 13.3]))
        .collect();
    blend(&b, Blend::Round, &floor, &[3.0; 2], &[], Corners::Filled).unwrap();
    b
}

/// The section area a rim round `r` of [`pocket_with_floor_rounds`] takes, past the 1 wide flat:
/// in the wall's frame (the wall x = 0, the top y = 0, the material x > 0, y < 0), the floor
/// round's centre is (-3, -1) and the ball's (c, -r), (c + 3)^2 + (1 - r)^2 = (3 + r)^2; the
/// region bounded by the top from the wall to the ball's contact (c, 0), the ball to its contact
/// on the floor round, the floor round up to the flat's end (0, -1) and the flat, by Green's
/// theorem over its two lines and two arcs.
fn pocket_rim_area(r: f64) -> f64 {
    let c = ((3.0 + r).powi(2) - (r - 1.0).powi(2)).sqrt() - 3.0;
    let to_floor = (-1.0 + r).atan2(-3.0 - c);
    let floor_contact = (-3.0 - 3.0 * to_floor.cos(), -1.0 - 3.0 * to_floor.sin());
    let on_floor = (floor_contact.1 + 1.0).atan2(floor_contact.0 + 3.0);
    let arc = |cx: f64, cy: f64, rad: f64, from: f64, to: f64| {
        0.5 * (rad * rad * (to - from) + cx * rad * (to.sin() - from.sin())
            - cy * rad * (to.cos() - from.cos()))
    };
    let ball_to = (floor_contact.1 + r)
        .atan2(floor_contact.0 - c)
        .rem_euclid(2.0 * PI);
    let total = arc(c, -r, r, PI / 2.0, ball_to) + arc(-3.0, -1.0, 3.0, on_floor, 0.0);
    total.abs()
}

/// The rims along z of [`pocket_with_floor_rounds`]'s walls at `xs` rounded R2.5 in one blend:
/// built, valid at the full check, the input's faces, edges, vertices and shells (each rim's
/// round takes its wall's flat and adds one face), each round its exact class, the cylinder of
/// radius 2.5 along z about (41 + c, 7.5) or (12.8 - c, 7.5), and the volume the input's less
/// [`pocket_rim_area`] along the pocket's 11.8 for each.
fn assert_pocket_rims_round(xs: &[f64]) {
    use solidcore::surface::surface::SurfaceKind;
    let r = 2.5;
    fixes();
    let b = pocket_with_floor_rounds();
    let before = b.volume(1e-9).unwrap();
    let (_, input) = common::kernel_copy(&b);
    let topology = (
        input.face_list().len(),
        input.edge_list().len(),
        input.vertex_list().len(),
        input.shells().len(),
    );
    assert_eq!(topology, (13, 30, 20, 2), "the input");
    let rims: Vec<usize> = xs.iter().map(|&x| edge_at(&b, [x, 10.0, 13.3])).collect();
    blend(
        &b,
        Blend::Round,
        &rims,
        &vec![r; xs.len()],
        &[],
        Corners::Filled,
    )
    .unwrap_or_else(|err| panic!("rims {xs:?}: {err:?}"));
    let report = b.check(CheckLevel::All).unwrap();
    assert!(report.is_valid(), "rims {xs:?}: {report}");
    let (_, brep) = common::kernel_copy(&b);
    let counts = (
        brep.face_list().len(),
        brep.edge_list().len(),
        brep.vertex_list().len(),
        brep.shells().len(),
    );
    assert_eq!(
        counts, topology,
        "rims {xs:?}: faces, edges, vertices, shells"
    );
    let c = ((3.0_f64 + r).powi(2) - (r - 1.0).powi(2)).sqrt() - 3.0;
    for &x in xs {
        let axis_x = if x > 20.0 { x + c } else { x - c };
        let rounds = brep
            .face_list()
            .iter()
            .filter(|&&f| {
                let surface = brep.face(f).unwrap().surface.as_ref().unwrap();
                match surface.kind() {
                    SurfaceKind::Cone(cone)
                        if cone.is_cylinder() && (cone.bot_radius() - r).abs() < 1e-9 =>
                    {
                        let place = cone.rev().position();
                        let (o, a) = (place.origin(), place.z_axis());
                        (o.x - axis_x).hypot(o.y - (10.0 - r)) < 1e-9
                            && a.x.hypot(a.y) < 1e-9 * a.length()
                    }
                    _ => false,
                }
            })
            .count();
        assert_eq!(rounds, 1, "rims {xs:?}: the round's cylinder at x {x}");
    }
    let expected = before - xs.len() as f64 * 11.8 * pocket_rim_area(r);
    let v = b.volume(1e-9).unwrap();
    assert!(
        (v - expected).abs() < 1e-6,
        "rims {xs:?}: volume {v}, expected {expected}"
    );
}

/// SolidCraft #18411: R2.5 on both rims along z of the pocket in one fillet. The 1 wide flat under
/// each rim is narrower than the ball reaches, so each round rolls over the flat's smooth lower
/// edge onto the floor round, the whole length. Each rim alone builds (a lone round, its rollover
/// decided in closed form); both in one blend were refused as having no room (SolidCraft: "the
/// largest radius that fits is 1 mm"), because each rolled rail ends on the pocket's end walls,
/// faces across its floor round's edges.
#[test]
fn both_pocket_rims_roll_onto_their_floor_rounds_in_one_fillet() {
    assert_pocket_rims_round(&[41.0]);
    assert_pocket_rims_round(&[12.8]);
    assert_pocket_rims_round(&[41.0, 12.8]);
}

#[test]
fn the_pocket_rim_area_oracle_agrees_with_a_polygon() {
    // the closed form against a fine polygon of the same boundary, in the wall's frame
    for r in [1.5_f64, 2.0, 2.5, 3.0] {
        let c = ((3.0 + r).powi(2) - (r - 1.0).powi(2)).sqrt() - 3.0;
        let ball = (c, -r);
        let to_floor = (ball.1 + 1.0).atan2(ball.0 + 3.0);
        let contact = (-3.0 + 3.0 * to_floor.cos(), -1.0 + 3.0 * to_floor.sin());
        let (from, to) = (
            PI / 2.0,
            (contact.1 - ball.1)
                .atan2(contact.0 - ball.0)
                .rem_euclid(2.0 * PI),
        );
        let mut pts = vec![(0.0, -1.0), (0.0, 0.0)];
        let n = 20000;
        for i in 0..=n {
            let t = from + (to - from) * f64::from(i) / f64::from(n);
            pts.push((ball.0 + r * t.cos(), ball.1 + r * t.sin()));
        }
        for i in 1..n {
            let t = to_floor + (0.0 - to_floor) * f64::from(i) / f64::from(n);
            pts.push((-3.0 + 3.0 * t.cos(), -1.0 + 3.0 * t.sin()));
        }
        let mut twice = 0.0;
        for (i, p) in pts.iter().enumerate() {
            let q = pts[(i + 1) % pts.len()];
            twice += p.0 * q.1 - q.0 * p.1;
        }
        let polygon = twice.abs() / 2.0;
        let closed = pocket_rim_area(r);
        assert!(
            (polygon - closed).abs() < 1e-6,
            "r {r}: {polygon} vs {closed}"
        );
    }
}

/// Controls for #18411: a rim along z of [`pocket_with_floor_rounds`] rounded R2.5 together with
/// a round that meets it, at its edge (a rim along x, R2.5 or R1) or on a face it may roll off
/// or onto (the floor round's end arc at z = 7.4, R1), is not apart from it: its rollover is
/// proposed from the ball's sections and proved after the trace, as on main. Each is refused
/// today (the three rims as running into the material, naming 0.99 mm; the others as having no
/// room); the check here is that none builds an invalid body.
#[test]
fn pocket_rim_rounds_meeting_another_round_build_valid_or_are_refused() {
    fixes();
    let arc = [
        38.0 + 3.0 * std::f64::consts::FRAC_1_SQRT_2,
        9.0 - 3.0 * std::f64::consts::FRAC_1_SQRT_2,
        7.4,
    ];
    let cases: [(&[[f64; 3]], &[f64]); 4] = [
        (
            &[[41.0, 10.0, 13.3], [12.8, 10.0, 13.3], [26.9, 10.0, 7.4]],
            &[2.5; 3],
        ),
        (&[[41.0, 10.0, 13.3], [26.9, 10.0, 19.2]], &[2.5, 1.0]),
        (&[[41.0, 10.0, 13.3], arc], &[2.5, 1.0]),
        (
            &[[41.0, 10.0, 13.3], [12.8, 10.0, 13.3], arc],
            &[2.5, 2.5, 1.0],
        ),
    ];
    for (points, radii) in cases {
        let b = pocket_with_floor_rounds();
        let edges: Vec<usize> = points.iter().map(|&p| edge_at(&b, p)).collect();
        if blend(&b, Blend::Round, &edges, radii, &[], Corners::Filled).is_ok() {
            let report = b.check(CheckLevel::All).unwrap();
            assert!(report.is_valid(), "{points:?}: {report}");
        }
    }
}

/// The hole's radius, the slots' width and the heights of the hole's floor and the block's
/// top in [`slotted_hole`].
const HOLE_R: f64 = 2.94;
const SLOT_W: f64 = 1.0;
const HOLE_FLOOR: f64 = 4.4;
const BLOCK_TOP: f64 = 8.0;

/// SolidCraft #18830's part: a block 20 x 20 x 8 with a blind hole of radius 2.94 down to
/// z = 4.4 (a magnet pocket; its cylinder's seam runs along x > 0), and a radial slot 1 wide
/// cut to the hole's floor at each angle of `slots` (degrees from x), each reaching 0.5 past
/// the hole with a round end. A slot at 14.8 degrees has its sides at 5 and 24.6 degrees on
/// the hole's wall, so the seam leaves a strip of wall 0.26 wide (5 degrees) beside the slot.
fn slotted_hole(slots: &[f64]) -> Body {
    use solidcore_ops::make_cylinder;
    use solidcore_ops::sweep::{extrude, Curve, DraftCorner};
    let block = make_box([-10.0, -10.0, 0.0], [20.0, 20.0, BLOCK_TOP]).unwrap();
    let hole = make_cylinder([0.0, 0.0, HOLE_FLOOR], HOLE_R, 4.0).unwrap();
    let mut b = boolean(block, hole, BooleanOp::Subtract).unwrap().unwrap();
    for &deg in slots {
        let (s, c) = deg.to_radians().sin_cos();
        let at =
            |along: f64, across: f64| [along * c - across * s, along * s + across * c, HOLE_FLOOR];
        let reach = HOLE_R + 0.5;
        let outline = [
            Curve::Line {
                start: at(0.0, -SLOT_W / 2.0),
                end: at(reach, -SLOT_W / 2.0),
            },
            Curve::Arc {
                center: at(reach, 0.0),
                axis: [0.0, 0.0, 1.0],
                reference: [s, -c, 0.0],
                radius: SLOT_W / 2.0,
                start: 0.0,
                end: PI,
            },
            Curve::Line {
                start: at(reach, SLOT_W / 2.0),
                end: at(0.0, SLOT_W / 2.0),
            },
            Curve::Line {
                start: at(0.0, SLOT_W / 2.0),
                end: at(0.0, -SLOT_W / 2.0),
            },
        ];
        let slot = extrude(
            &outline,
            [0.0, 0.0, 1.0],
            4.0,
            0.0,
            DraftCorner::default(),
            true,
        )
        .unwrap();
        b = boolean(b, slot, BooleanOp::Subtract).unwrap().unwrap();
    }
    b
}

/// The edges where the slot at `deg` meets the hole's wall, on its side at `+0.5` across and
/// at `-0.5`, as `(side, edge index)`.
fn slot_roots(b: &Body, deg: f64) -> [usize; 2] {
    let (s, c) = deg.to_radians().sin_cos();
    let along = (HOLE_R * HOLE_R - SLOT_W * SLOT_W / 4.0).sqrt();
    let mid = (HOLE_FLOOR + BLOCK_TOP) / 2.0;
    [SLOT_W / 2.0, -SLOT_W / 2.0]
        .map(|across| edge_at(b, [along * c - across * s, along * s + across * c, mid]))
}

/// The section area a round `r` takes where a slot's side (a plane `h` from the slot's middle)
/// meets the hole's wall (a circle of radius `R`, the material outside it): the ball's centre
/// is `r` from the side, into the material, and `R + r` from the hole's axis; the area is the
/// triangle of the corner and the ball's two contacts, less the ball's segment and the hole's
/// segment between them.
fn root_area(big_r: f64, r: f64, h: f64) -> f64 {
    let corner = ((big_r * big_r - h * h).sqrt(), h);
    let centre = (((big_r + r).powi(2) - (h + r).powi(2)).sqrt(), h + r);
    let on_side = (centre.0, h);
    let on_hole = (
        centre.0 * big_r / (big_r + r),
        centre.1 * big_r / (big_r + r),
    );
    let triangle = ((on_side.0 - corner.0) * (on_hole.1 - corner.1)
        - (on_hole.0 - corner.0) * (on_side.1 - corner.1))
        .abs()
        / 2.0;
    let segment = |rad: f64, p: (f64, f64), q: (f64, f64)| {
        let chord = (p.0 - q.0).hypot(p.1 - q.1);
        let angle = 2.0 * (chord / (2.0 * rad)).asin();
        rad * rad / 2.0 * (angle - angle.sin())
    };
    triangle - segment(r, on_side, on_hole) - segment(big_r, on_hole, corner)
}

/// SolidCraft #18830: R0.5 on both edges where a slot meets the hole, in one fillet, the slot's
/// side at 5 degrees past the hole's seam. The 0.26 wide strip of wall between that side and
/// the seam is narrower than the ball reaches, so that round rolls over the seam onto the rest
/// of the wall, the whole length. Each edge alone builds (a lone round, decided in closed form);
/// both together were refused ("Corner Edge Computation Failure"), because the other round's
/// edge is an edge of the wall it rolls onto, which sent it to the ball's sections, and they do
/// not roll a rail over a seam onto its own surface.
///
/// Closed form: the rounds are apart (the slot's two sides), each takes `root_area` along the
/// hole's depth, and each is a cylinder of radius 0.5 along z about its ball's centre.
#[test]
fn slot_roots_beside_a_hole_s_seam_round_in_one_fillet() {
    use solidcore::surface::surface::SurfaceKind;
    let r = 0.5;
    fixes();
    for deg in [14.8, -14.8] {
        let b = slotted_hole(&[deg]);
        let before = b.volume(1e-9).unwrap();
        let roots = slot_roots(&b, deg);
        blend(&b, Blend::Round, &roots, &[r; 2], &[], Corners::Filled)
            .unwrap_or_else(|err| panic!("slot at {deg}: {err:?}"));
        let report = b.check(CheckLevel::All).unwrap();
        assert!(report.is_valid(), "slot at {deg}: {report}");
        let expected = before - 2.0 * (BLOCK_TOP - HOLE_FLOOR) * root_area(HOLE_R, r, SLOT_W / 2.0);
        let v = b.volume(1e-9).unwrap();
        assert!(
            (v - expected).abs() < 1e-6,
            "slot at {deg}: volume {v}, expected {expected}"
        );
        let (_, brep) = common::kernel_copy(&b);
        let centre_along = ((HOLE_R + r).powi(2) - (SLOT_W / 2.0 + r).powi(2)).sqrt();
        let (s, c) = f64::to_radians(deg).sin_cos();
        for across in [SLOT_W / 2.0 + r, -(SLOT_W / 2.0 + r)] {
            let (x, y) = (centre_along * c - across * s, centre_along * s + across * c);
            let rounds = brep
                .face_list()
                .iter()
                .filter(|&&f| {
                    let surface = brep.face(f).unwrap().surface.as_ref().unwrap();
                    match surface.kind() {
                        SurfaceKind::Cone(cone)
                            if cone.is_cylinder() && (cone.bot_radius() - r).abs() < 1e-9 =>
                        {
                            let place = cone.rev().position();
                            let (o, a) = (place.origin(), place.z_axis());
                            (o.x - x).hypot(o.y - y) < 1e-9 && a.x.hypot(a.y) < 1e-9 * a.length()
                        }
                        _ => false,
                    }
                })
                .count();
            assert_eq!(rounds, 1, "slot at {deg}: the round about ({x}, {y})");
        }
    }
}

/// The angle on the hole's wall between a slot's middle and its side, in degrees: the side
/// is the line `SLOT_W / 2` from the slot's middle.
fn slot_half_angle() -> f64 {
    (SLOT_W / 2.0 / HOLE_R).asin().to_degrees()
}

/// Controls for #18830: a second slot on the far side of the hole's seam, all four roots of
/// both slots rounded R0.5 in one fillet. The first slot (at 14.8 degrees, a side 5 degrees
/// past the seam) rolls one round over the seam onto the wall the second slot's near side
/// stands on. With that side `past` degrees the other way from the seam, the round on it
/// takes about 7.1 degrees of wall (its ball touches the wall 0.36 from its edge) and the
/// rolled round about 2.1 degrees past the seam, so the bands overlap below about 9.2 degrees:
/// at 8 and 9 degrees the fillet is refused as having no room; at 12 and 20 degrees all four
/// rounds build, each its exact [`root_area`] along the hole's depth. At 5 degrees (slots at
/// 14.8 and -14.8) both near sides roll over the seam, from opposite sides, and their bands
/// overlap: [`slot_roots_rolling_over_one_seam_from_both_sides_are_not_apart`].
#[test]
fn slot_roots_on_both_sides_of_a_hole_s_seam_build_apart_or_are_refused_overlapping() {
    let r = 0.5;
    fixes();
    let area = root_area(HOLE_R, r, SLOT_W / 2.0);
    for past in [8.0, 9.0, 12.0, 20.0] {
        let slots = [14.8, -(past + slot_half_angle())];
        let b = slotted_hole(&slots);
        let before = b.volume(1e-9).unwrap();
        let mut edges = slot_roots(&b, slots[0]).to_vec();
        edges.extend(slot_roots(&b, slots[1]));
        let result = blend(&b, Blend::Round, &edges, &[r; 4], &[], Corners::Filled);
        if past < 10.0 {
            assert!(
                matches!(result, Err(Error::NoRoom)),
                "side {past} degrees past the seam: {result:?}"
            );
            continue;
        }
        result.unwrap_or_else(|err| panic!("side {past} degrees past the seam: {err:?}"));
        let report = b.check(CheckLevel::All).unwrap();
        assert!(
            report.is_valid(),
            "side {past} degrees past the seam: {report}"
        );
        let expected = before - 4.0 * (BLOCK_TOP - HOLE_FLOOR) * area;
        let v = b.volume(1e-9).unwrap();
        assert!(
            (v - expected).abs() < 1e-6,
            "side {past} degrees past the seam: volume {v}, expected {expected}"
        );
    }
}

/// Controls for #18830: slots at 14.8 and -14.8 degrees put a side 5 degrees either side of
/// the hole's seam, and both those rounds roll over the seam, from opposite sides, so their
/// bands overlap. Each round's closed form rolls over the seam, so neither is apart from the
/// other: both go to the ball's sections, as on main, and the fillet is refused there with the
/// corner error, in either order of the edges. (Taken as apart, each would roll in closed form
/// and the overlap would surface later, as an edge of the fillet that maps back to no edge.)
#[test]
fn slot_roots_rolling_over_one_seam_from_both_sides_are_not_apart() {
    fixes();
    let slots = [14.8, -14.8];
    for reversed in [false, true] {
        let b = slotted_hole(&slots);
        let mut edges = slot_roots(&b, slots[0]).to_vec();
        edges.extend(slot_roots(&b, slots[1]));
        if reversed {
            edges.reverse();
        }
        match blend(&b, Blend::Round, &edges, &[0.5; 4], &[], Corners::Filled) {
            Err(Error::BlendFailed { phrase, .. })
                if phrase.contains("Corner Edge Computation Failure") => {}
            other => panic!("edges reversed {reversed}: {other:?}"),
        }
    }
}

/// #19242: the slots of [`slot_roots_beside_a_hole_s_seam_round_in_one_fillet`] and
/// [`slot_roots_on_both_sides_of_a_hole_s_seam_build_apart_or_are_refused_overlapping`]. The
/// round that rolls over the seam is taken as apart although another round stands on the wall
/// it rolls onto, so only the bands' check compares the two (it always runs: WR-542 is no
/// longer a flag). Where the bands overlap (the second slot's near side 8 or 9 degrees past the
/// seam) the fillet is refused as having no room (with the check skipped, the overlap surfaced
/// later as "no fillet B-rep edge matches the fillet edge"). Where they don't, the fillet
/// builds, valid.
#[test]
fn slot_roots_beside_a_hole_s_seam_have_their_bands_checked() {
    let r = 0.5;
    let one_slot = |deg: f64| (vec![deg], 2, true);
    let two_slots = |past: f64| (vec![14.8, -(past + slot_half_angle())], 4, past > 10.0);
    fixes();
    for (slots, n, builds) in [
        one_slot(14.8),
        one_slot(-14.8),
        two_slots(8.0),
        two_slots(9.0),
        two_slots(12.0),
        two_slots(20.0),
    ] {
        let b = slotted_hole(&slots);
        let mut edges = Vec::new();
        for &deg in &slots {
            edges.extend(slot_roots(&b, deg));
        }
        let result = blend(&b, Blend::Round, &edges, &vec![r; n], &[], Corners::Filled).map(|_| {
            let report = b.check(CheckLevel::All).unwrap();
            assert!(report.is_valid(), "slots {slots:?}: {report}");
            b.volume(1e-9).unwrap()
        });
        match result {
            Err(Error::NoRoom) if !builds => {}
            Ok(v) if builds => assert!(v.is_finite() && v > 0.0, "slots {slots:?}: volume {v}"),
            other => panic!("slots {slots:?}: {other:?}"),
        }
    }
}

/// The tube's radius, the Root rounds' radius, the fin's thickness and the height of its top.
const TUBE: f64 = 10.0;
const ROOT: f64 = 1.0;
const FIN: f64 = 0.62;
const FIN_TOP: f64 = 55.0;

/// #19476's part with its fin at `y0`: a tube of radius [`TUBE`] and height 75 about z (its seam
/// the ruling x = 10, y = 0) with a fin 6 long, [`FIN`] thick and [`FIN_TOP`] high standing on it
/// at x = 9..15, its sides the planes y = `y0` and y = `y0` + [`FIN`]. The fin's root, where its
/// sides meet the tube, is rounded [`ROOT`]: two rounds the height of the fin, each tangent into
/// the tube and into its side, ending flush with the fin's top.
fn tube_with_fin(y0: f64) -> Body {
    use solidcore_ops::make_cylinder;
    let tube = make_cylinder([0.0, 0.0, 0.0], TUBE, 75.0).unwrap();
    let fin = make_box([9.0, y0, 0.0], [6.0, FIN, FIN_TOP]).unwrap();
    let b = boolean(tube, fin, BooleanOp::Union).unwrap().unwrap();
    let on_tube = |y: f64| [(TUBE * TUBE - y * y).sqrt(), y, 30.0];
    blend(
        &b,
        Blend::Round,
        &[edge_at(&b, on_tube(y0 + FIN)), edge_at(&b, on_tube(y0))],
        &[ROOT, ROOT],
        &[],
        Corners::Filled,
    )
    .unwrap();
    b
}

/// One side of the fin (the plane y = `y`, its Root round's axis at y = `yc`) under the fin top's
/// round `r`: W, where the Root round's top arc meets the side's top edge (the Root round's axis
/// moved onto the side); V, where that arc meets the tube; and W's distance from the tube's axis.
fn fin_side(y: f64, yc: f64) -> ([f64; 2], [f64; 2], f64) {
    let d = TUBE + ROOT;
    let cx = (d * d - yc * yc).sqrt();
    ([cx, y], [TUBE / d * cx, TUBE / d * yc], cx.hypot(y))
}

/// The volume the fin top's round `r` adds to [`tube_with_fin`]: the round is the torus the ball
/// sweeps about the tube's axis (its centre [`TUBE`] + r from the axis, r above the fin's top),
/// over the fin top's face, cut flush with the fin's sides and the Root rounds continued upward
/// (docs/generic-blend.md 3.4). At distance p from the axis the torus stands
/// r - sqrt(r^2 - (TUBE + r - p)^2) above the top, and the face spans the angle from one side to
/// the other: on each side its boundary is the Root round's arc (the circle of radius [`ROOT`]
/// about the Root ball's centre, [`TUBE`] + [`ROOT`] from the axis) up to W, then the side's
/// line. With p = TUBE + r - r sin(phi) the integrand is smooth; Simpson's rule between the
/// values of phi where a side's boundary passes W. For r up to 2 [`ROOT`] (the circle of radius
/// p meets the Root rounds' circles) and a fin longer than r.
fn fin_top_round_volume(y0: f64, r: f64) -> f64 {
    let y1 = y0 + FIN;
    let d = TUBE + ROOT;
    let (_, _, wp) = fin_side(y1, y1 + ROOT);
    let (_, _, wm) = fin_side(y0, y0 - ROOT);
    let centre = |yc: f64| yc.atan2((d * d - yc * yc).sqrt());
    let (ap, am) = (centre(y1 + ROOT), centre(y0 - ROOT));
    let span = |p: f64| {
        // the angle at the tube's axis between a Root ball's centre and where the circle of
        // radius p meets that ball's circle
        let delta = ((p * p + d * d - ROOT * ROOT) / (2.0 * d * p))
            .min(1.0)
            .acos();
        let hi = if p < wp { ap - delta } else { (y1 / p).asin() };
        let lo = if p < wm { am + delta } else { (y0 / p).asin() };
        hi - lo
    };
    let f = |phi: f64| {
        let p = TUBE + r - r * phi.sin();
        r * (1.0 - phi.cos()) * p * span(p) * r * phi.cos()
    };
    let mut cuts = vec![0.0, PI / 2.0];
    for w in [wp, wm] {
        let s = (TUBE + r - w) / r;
        if s > 0.0 && s < 1.0 {
            cuts.push(s.asin());
        }
    }
    cuts.sort_by(f64::total_cmp);
    let simpson = |a: f64, b: f64| {
        let n = 4000;
        let h = (b - a) / f64::from(n);
        let inner: f64 = (1..n)
            .map(|i| f(a + h * f64::from(i)) * if i % 2 == 1 { 4.0 } else { 2.0 })
            .sum();
        (f(a) + f(b) + inner) * h / 3.0
    };
    cuts.windows(2).map(|w| simpson(w[0], w[1])).sum()
}

/// The edge of the fin's top against the tube that the fin top's round is asked on: the piece
/// of the tube's circle at z = [`FIN_TOP`] through y = `y0` + 0.2.
fn fin_top_edge(b: &Body, y0: f64) -> usize {
    let y = y0 + 0.2;
    edge_at(b, [(TUBE * TUBE - y * y).sqrt(), y, FIN_TOP])
}

/// The fin top's round `r` on [`tube_with_fin`]`(y0)`, checked: built, a valid closed solid, its
/// volume the closed form's, its faces and edges where the torus, the tube, the fin's top, the
/// fin's sides and the Root rounds continued upward put them. Returns the body's total face,
/// edge and vertex counts (checked here against Euler's formula for a closed genus 0 solid).
fn assert_fin_top_round_rolls_over(y0: f64, r: f64) -> (usize, usize, usize) {
    use solidcore::query::api_queries::{brep_is_closed_manifold, face_internal_point};
    let b = tube_with_fin(y0);
    let before = b.volume(1e-9).unwrap();
    blend(
        &b,
        Blend::Round,
        &[fin_top_edge(&b, y0)],
        &[r],
        &[],
        Corners::Filled,
    )
    .unwrap_or_else(|err| panic!("y0 {y0} r {r}: {err:?}"));
    let report = b.check(CheckLevel::All).unwrap();
    assert!(report.is_valid(), "y0 {y0} r {r}: {report}");
    let added = b.volume(1e-9).unwrap() - before;
    let want = fin_top_round_volume(y0, r);
    // each volume is asked to 1e-9
    assert!(
        (added - want).abs() <= 2e-9,
        "y0 {y0} r {r}: added {added}, the closed form {want}"
    );

    let (k, mut brep) = common::kernel_copy(&b);
    assert!(
        brep_is_closed_manifold(&brep).unwrap(),
        "y0 {y0} r {r}: not closed"
    );
    let y1 = y0 + FIN;
    let sides = [(y1, y1 + ROOT), (y0, y0 - ROOT)];
    let torus = |p: &solidcore::Vec3| (p.x.hypot(p.y) - (TUBE + r)).hypot(p.z - (FIN_TOP + r)) - r;
    // the distance from the Root round whose axis is at y = yc
    let root = |p: &solidcore::Vec3, yc: f64| {
        let ([cx, _], _, _) = fin_side(yc, yc);
        (p.x - cx).hypot(p.y - yc) - ROOT
    };
    // on the fin's top face: between the sides, or beside one within its Root round's cap
    let on_top = |p: &solidcore::Vec3, tol: f64| {
        (p.y >= y0 - tol && p.y <= y1 + tol)
            || sides.iter().any(|&(y, yc)| {
                let (w, _, _) = fin_side(y, yc);
                (p.y - y) * (yc - y) > 0.0 && p.x <= w[0] + tol && root(p, yc) >= -tol
            })
    };
    let near = |p: &solidcore::Vec3, q: [f64; 3], tol: f64| {
        (p.x - q[0]).abs() <= tol && (p.y - q[1]).abs() <= tol && (p.z - q[2]).abs() <= tol
    };

    // the edges on the torus, each within its own tolerance of it
    let (mut tube_rail, mut top_rail) = (Vec::new(), 0);
    let mut ribs = [0; 2];
    let mut walls = [0; 2];
    for &e in brep.edge_list() {
        let edge = brep.edge(e).unwrap();
        let curve = edge.curve.as_ref().unwrap();
        let tol = solidcore::topology::edge::tolerance(&brep, e).unwrap();
        let pts: Vec<_> = (0..=64)
            .map(|i| {
                curve
                    .evaluate_point(&k, edge.interval.evaluate(f64::from(i) / 64.0))
                    .unwrap()
            })
            .collect();
        if !pts.iter().all(|p| torus(p).abs() <= tol) {
            continue;
        }
        let (first, last) = (&pts[0], &pts[64]);
        if pts.iter().all(|p| (p.x.hypot(p.y) - TUBE).abs() <= tol) {
            // the rail on the tube: r above the fin's top
            assert!(
                pts.iter().all(|p| (p.z - FIN_TOP - r).abs() <= tol),
                "y0 {y0} r {r}: {e:?}"
            );
            tube_rail.extend(pts.iter().map(|p| p.y.atan2(p.x)));
        } else if pts.iter().all(|p| (p.z - FIN_TOP).abs() <= tol) {
            // the rail on the fin's top: on that face all along
            for p in &pts {
                assert!(
                    on_top(p, tol),
                    "y0 {y0} r {r}: the rail on the fin's top leaves it at {p:?}"
                );
            }
            top_rail += 1;
        }
        for (i, &(y, yc)) in sides.iter().enumerate() {
            let (w, v, rho) = fin_side(y, yc);
            // where the torus crosses the Root round's line through W: past W the round ends
            // on the side, before it on the Root round's top arc
            let past = rho < TUBE + r;
            let h = ((rho - TUBE) * (TUBE + 2.0 * r - rho)).max(0.0).sqrt();
            let s = [w[0], w[1], FIN_TOP + r - h];
            let on_tube = [v[0], v[1], FIN_TOP + r];
            if pts.iter().all(|p| root(p, yc).abs() <= tol)
                && pts.iter().any(|p| p.z > FIN_TOP + tol)
            {
                // the rib on the Root round continued: from the tube rail's end down to S
                let ends = |a: &solidcore::Vec3, z: &solidcore::Vec3| {
                    near(a, on_tube, tol) && (!past || near(z, s, tol))
                };
                assert!(
                    ends(first, last) || ends(last, first),
                    "y0 {y0} r {r}: rib {first:?} {last:?}"
                );
                ribs[i] += 1;
            }
            if past
                && pts.iter().all(|p| (p.y - y).abs() <= tol)
                && pts.iter().any(|p| p.z > FIN_TOP + tol)
            {
                // the rib on the side continued: from S down to the rail's end on the side's edge
                let rail_end = [((TUBE + r) * (TUBE + r) - y * y).sqrt(), y, FIN_TOP];
                let ends = |a: &solidcore::Vec3, z: &solidcore::Vec3| {
                    near(a, s, tol) && near(z, rail_end, tol)
                };
                assert!(
                    ends(first, last) || ends(last, first),
                    "y0 {y0} r {r}: wall {first:?} {last:?}"
                );
                walls[i] += 1;
            }
        }
    }
    assert!(top_rail > 0, "y0 {y0} r {r}: no rail on the fin's top");
    // the rail on the tube runs from one Root round's tangent line to the other's
    let (lo, hi) = tube_rail
        .iter()
        .fold((f64::MAX, f64::MIN), |(lo, hi), &a| (lo.min(a), hi.max(a)));
    let angle = |(y, yc): (f64, f64)| {
        let (_, v, _) = fin_side(y, yc);
        v[1].atan2(v[0])
    };
    assert!(
        (hi - angle(sides[0])).abs() <= 1e-9 && (lo - angle(sides[1])).abs() <= 1e-9,
        "y0 {y0} r {r}: the tube rail spans {lo} to {hi}"
    );
    for (i, &(y, yc)) in sides.iter().enumerate() {
        let past = fin_side(y, yc).2 < TUBE + r;
        assert_eq!(ribs[i], 1, "y0 {y0} r {r}: ribs on the Root round at y {y}");
        assert_eq!(
            walls[i],
            usize::from(past),
            "y0 {y0} r {r}: ribs on the side y {y}"
        );
    }

    // the faces above the fin's top: the torus, the tube and its top, and the Root rounds and
    // the sides continued upward; nothing else
    let mut on_torus = 0;
    for &f in &brep.face_list().to_vec() {
        let p = face_internal_point(&k, &mut brep, f).unwrap();
        let tol = brep.face(f).unwrap().tolerance.max(brep.tolerance());
        if p.z <= FIN_TOP + tol {
            continue;
        }
        if torus(&p).abs() <= tol {
            on_torus += 1;
            continue;
        }
        let known = (p.x.hypot(p.y) - TUBE).abs() <= tol
            || (p.z - 75.0).abs() <= tol
            || sides
                .iter()
                .any(|&(y, yc)| root(&p, yc).abs() <= tol || (p.y - y).abs() <= tol);
        assert!(
            known,
            "y0 {y0} r {r}: a face above the fin's top through {p:?}"
        );
    }
    assert!(on_torus > 0, "y0 {y0} r {r}: no face on the torus");
    let counts = (
        brep.face_list().len(),
        brep.edge_list().len(),
        brep.vertex_list().len(),
    );
    assert_eq!(
        counts.2 + counts.0,
        counts.1 + 2,
        "y0 {y0} r {r}: V - E + F is not 2"
    );
    counts
}

/// The closed form of the fin top's round is the kernel's where the round already builds: below
/// the radius at which its rail on the fin's top reaches a side's top edge (the ball over W,
/// [`fin_side`]), with the fin on the seam's plane (#19476's part) and off it.
#[test]
fn a_fin_top_round_that_stays_on_the_root_rounds_adds_the_closed_form() {
    fixes();
    for (y0, r) in [(-0.62, 0.5), (-0.62, 0.85), (-0.31, 0.85)] {
        let b = tube_with_fin(y0);
        let before = b.volume(1e-9).unwrap();
        blend(
            &b,
            Blend::Round,
            &[fin_top_edge(&b, y0)],
            &[r],
            &[],
            Corners::Filled,
        )
        .unwrap_or_else(|err| panic!("y0 {y0} r {r}: {err:?}"));
        assert!(
            b.check(CheckLevel::All).unwrap().is_valid(),
            "y0 {y0} r {r}"
        );
        let added = b.volume(1e-9).unwrap() - before;
        let want = fin_top_round_volume(y0, r);
        assert!(
            (added - want).abs() <= 2e-9,
            "y0 {y0} r {r}: added {added}, the closed form {want}"
        );
    }
}

/// #19476: the fin top's round at 1, past the radius at which its rail reaches the fin's sides'
/// top edges (0.926 with the fin across the seam), is the Roll-over's concave chain asked on its
/// own, the tube in the convex round's place: it rolls over the Root rounds and the fin's sides
/// continued. The tube's seam splits the asked edge between the fin's sides, so the round runs
/// on across the seam's vertex and has a cliff at each end.
#[test]
fn a_fin_top_round_past_the_fin_s_sides_rolls_over_the_root_rounds() {
    fixes();
    // 13 faces: the tube's side, top and bottom, the fin's end and two sides and top, the two Root
    // rounds, their two continuations above the fin's top, and the torus, which the seam (the
    // fin's sides straddle it) cuts in two. The edge and vertex totals follow from those faces'
    // boundaries and are tied to them by V - E + F = 2 (21 - 32 + 13), which the helper asserts.
    assert_eq!(
        assert_fin_top_round_rolls_over(-0.31, 1.0),
        (13, 32, 21),
        "faces, edges, vertices"
    );
}

/// #19476 with the fin clear of the tube's seam (y from 2 to 2.62): one edge, a cliff at each
/// end. At the side y = 2.62 the rail passes W (0.713 from the tube) onto the side's top edge;
/// at y = 2 it ends on the Root round's top arc (W is 1.136 from the tube).
#[test]
fn a_fin_top_round_on_a_tube_with_its_seam_elsewhere_rolls_over_the_root_rounds() {
    fixes();
    // 12 faces: the same as with the seam in the fin, but the torus is one face (the seam is
    // clear of the fin), so one face, and with it the seam's two edges and one vertex on the
    // torus, fewer. V - E + F = 2 holds: 19 - 29 + 12.
    assert_eq!(
        assert_fin_top_round_rolls_over(2.0, 1.0),
        (12, 29, 19),
        "faces, edges, vertices"
    );
}

/// The kernel's decision: the fin top's edge, with the piece it runs on into across the seam's
/// vertex, is the Roll-over's concave chain, a V at each Root round's tangent line.
#[test]
fn a_fin_top_edge_on_a_tube_is_the_roll_overs_concave_chain() {
    use solidcore::blend::blend_corner::rollover::{concave_chain_ends, is_concave_chain};
    fixes();
    for y0 in [-0.62, -0.31, 2.0] {
        let b = tube_with_fin(y0);
        let pick = fin_top_edge(&b, y0);
        let (k, mut brep) = common::kernel_copy(&b);
        let edges = [brep.edge_list()[pick]];
        assert!(is_concave_chain(&k, &mut brep, &edges).unwrap(), "y0 {y0}");
        let ends = concave_chain_ends(&k, &mut brep, &edges).unwrap();
        let mut at: Vec<[f64; 2]> = ends
            .iter()
            .map(|&v| {
                let p = brep.vertex(v).unwrap().point;
                assert!((p.z - FIN_TOP).abs() < 1e-9);
                [p.x, p.y]
            })
            .collect();
        at.sort_by(|a, b| a[1].total_cmp(&b[1]));
        let y1 = y0 + FIN;
        let want = [fin_side(y0, y0 - ROOT).1, fin_side(y1, y1 + ROOT).1];
        assert_eq!(at.len(), 2, "y0 {y0}: {at:?}");
        for (a, w) in at.iter().zip(&want) {
            assert!(
                (a[0] - w[0]).abs() < 1e-9 && (a[1] - w[1]).abs() < 1e-9,
                "y0 {y0}: {a:?} {w:?}"
            );
        }
    }
}

/// #19505, #19476's own part: the fin's side y = 0 lies in the plane of the tube's seam, so the
/// seam's vertex on the asked edge is where the round's rail on the fin's top crosses the side's
/// top edge, and the round has no whole section there for the two pieces to join at. The pass
/// turns the tube's seam off the edge in its own copy, rolls over the Root rounds as on a tube
/// whose seam is elsewhere, and puts the seam back.
#[test]
fn a_fin_top_round_whose_seam_lies_in_the_fin_s_side_rolls_over_the_root_rounds() {
    fixes();
    assert_fin_top_round_rolls_over(-0.62, 1.0);
}

/// The seam edges of `b` (both sides on one face), each as its two ends, the lower first.
fn seams(b: &Body) -> Vec<[[f64; 3]; 2]> {
    use solidcore::topology::edge;
    let (k, mut brep) = common::kernel_copy(b);
    let mut out = Vec::new();
    for &e in &brep.edge_list().to_vec() {
        if edge::faces(&brep, e, None).unwrap().len() != 1
            || !edge::is_seam(&k, &mut brep, e).unwrap()
        {
            continue;
        }
        let (s, z) = edge::start_end_vertices(&brep, e).unwrap();
        let p = brep.vertex(s.unwrap()).unwrap().point;
        let q = brep.vertex(z.unwrap()).unwrap().point;
        let (lo, hi) = if p.z <= q.z { (p, q) } else { (q, p) };
        out.push([[lo.x, lo.y, lo.z], [hi.x, hi.y, hi.z]]);
    }
    out
}

fn assert_seam(b: &Body, from_z: f64, what: &str) {
    let got = seams(b);
    assert_eq!(got.len(), 1, "{what}: {got:?}");
    let want = [[TUBE, 0.0, from_z], [TUBE, 0.0, 75.0]];
    for (p, w) in got[0].iter().zip(&want) {
        for (a, c) in p.iter().zip(w) {
            assert!((a - c).abs() <= 1e-9, "{what}: {got:?}");
        }
    }
}

/// #19505: the round built with the tube's seam turned off its edge leaves the seam on the
/// ruling it was on (x = 10, y = 0), from the tube's top down to the round's rail on the tube.
#[test]
fn a_fin_top_round_built_off_the_tube_s_seam_leaves_the_seam_on_its_ruling() {
    fixes();
    let b = tube_with_fin(-0.62);
    assert_seam(&b, FIN_TOP, "the input");
    blend(
        &b,
        Blend::Round,
        &[fin_top_edge(&b, -0.62)],
        &[1.0],
        &[],
        Corners::Filled,
    )
    .unwrap();
    assert_seam(&b, FIN_TOP + 1.0, "the result");
}

/// The kernel's decision (#19505): the chain is taken off a seam only where the round has no
/// whole section at the seam's vertex. With the fin's side in the seam's plane the contact on
/// the fin's top at the vertex is on the side's top edge at 1 (the seam is turned: the chain
/// is one edge from V to V, the body has as many edges as before, the seam's edge and the
/// vertex it split the fin top's edge at gone, the turned seam's edge and the vertex it splits
/// the tube's bottom arc at made) and inside the fin's top at 0.9; with the fin across the
/// seam it is inside at 1; with the fin elsewhere the chain has no seam's vertex.
#[test]
fn the_chain_is_taken_off_a_seam_only_where_the_round_has_no_whole_section_there() {
    use solidcore::blend::blend_corner::rollover::chain_off_seams;
    use solidcore::topology::edge;
    fixes();
    for (y0, r, turned) in [
        (-0.62, 1.0, true),
        (-0.62, 0.9, false),
        (-0.31, 1.0, false),
        (2.0, 1.0, false),
    ] {
        let b = tube_with_fin(y0);
        let pick = fin_top_edge(&b, y0);
        let (mut k, mut brep) = common::kernel_copy(&b);
        let edges = [brep.edge_list()[pick]];
        let before = brep.edge_list().len();
        let off = chain_off_seams(&mut k, &mut brep, &edges, &[r]).unwrap();
        assert_eq!(off.is_some(), turned, "y0 {y0} r {r}");
        assert_eq!(brep.edge_list().len(), before, "y0 {y0} r {r}");
        let Some(off) = off else {
            continue;
        };
        assert_eq!(off.turned.len(), 1, "y0 {y0} r {r}");
        assert_eq!(off.sizes, vec![r], "y0 {y0} r {r}");
        let [chain] = off.edges.as_slice() else {
            panic!("y0 {y0} r {r}: {:?}", off.edges);
        };
        let (s, z) = edge::start_end_vertices(&brep, *chain).unwrap();
        let mut at: Vec<[f64; 2]> = [s, z]
            .into_iter()
            .map(|v| {
                let p = brep.vertex(v.unwrap()).unwrap().point;
                [p.x, p.y]
            })
            .collect();
        at.sort_by(|a, b| a[1].total_cmp(&b[1]));
        let y1 = y0 + FIN;
        let want = [fin_side(y0, y0 - ROOT).1, fin_side(y1, y1 + ROOT).1];
        for (a, w) in at.iter().zip(&want) {
            assert!(
                (a[0] - w[0]).abs() < 1e-9 && (a[1] - w[1]).abs() < 1e-9,
                "y0 {y0} r {r}: {a:?} {w:?}"
            );
        }
    }
}

/// #19505: a seam turned off the chain and put back leaves the body as it was: valid, its
/// faces and edges as many, its volume the same, its seam on the same ruling between the
/// same ends.
#[test]
fn a_seam_turned_off_the_chain_and_back_leaves_the_body_as_it_was() {
    use solidcore::blend::blend_corner::rollover::{chain_off_seams, seams_back};
    fixes();
    let b = tube_with_fin(-0.62);
    let volume = b.volume(1e-9).unwrap();
    let pick = fin_top_edge(&b, -0.62);
    let (mut k, mut brep) = common::kernel_copy(&b);
    let edges = [brep.edge_list()[pick]];
    let (faces, edge_count) = (brep.face_list().len(), brep.edge_list().len());
    let off = chain_off_seams(&mut k, &mut brep, &edges, &[1.0])
        .unwrap()
        .unwrap();
    seams_back(&mut k, &mut brep, &off.turned).unwrap();
    assert_eq!(
        (brep.face_list().len(), brep.edge_list().len()),
        (faces, edge_count)
    );
    let back = common::built(|_| Ok(std::mem::take(&mut brep)));
    let report = back.check(CheckLevel::All).unwrap();
    assert!(report.is_valid(), "{report}");
    // each volume is asked to 1e-9
    assert!((back.volume(1e-9).unwrap() - volume).abs() <= 2e-9);
    assert_seam(&back, FIN_TOP, "turned and back");
}

/// #19505: the same part at 0.93 stays refused as on main. The seam's vertex on the chain has
/// no whole section at this size either, but the round's rail leaves the fin's top at the
/// seam's section (the decision's trigger, see `docs/generic-blend.md`), so the body is
/// untouched and the refusal stands.
#[test]
fn a_fin_top_round_whose_seam_lies_in_the_fin_s_side_stays_refused_at_0_93() {
    fixes();
    let b = tube_with_fin(-0.62);
    let before = b.volume(1e-9).unwrap();
    match blend(
        &b,
        Blend::Round,
        &[fin_top_edge(&b, -0.62)],
        &[0.93],
        &[],
        Corners::Filled,
    ) {
        Err(Error::TooLarge {
            why: Some(RoomReason::RailLeavesFace),
            ..
        }) => {}
        other => panic!("{other:?}"),
    }
    assert_eq!(b.volume(1e-9).unwrap().to_bits(), before.to_bits());
}

/// SolidCraft #15767 (j4py's repro): a bar 32 long along y whose section in the xz plane is the
/// rectangle x = 0..10, z = 0..7 with its corner (10, 7) rounded R3 (axis x = 7, z = 4), the arc
/// tangent to the wall x = 10 at z = 4; everything below the plane z = 1.9 + 0.11875 y cut away.
/// The flat wall's height above the bottom falls from 4 at y = -16 to 0.2 at y = 16.
fn narrowing_wall() -> Body {
    narrowing_wall_between(0.0, 3.8)
}

/// [`narrowing_wall`] with its cut from height `a` at y = -16 to `b` at y = 16.
fn narrowing_wall_between(a: f64, b: f64) -> Body {
    use solidcore_ops::prism::extrude_planar_regions;
    use solidcore_record::{Loop2, Plane, Region2, Seg2, Vec2};
    let v = Vec2::new;
    let section = Region2 {
        outer: Loop2 {
            segs: vec![
                Seg2::Arc {
                    center: v(7.0, -4.0),
                    radius: 3.0,
                    start: -PI / 2.0,
                    sweep: PI / 2.0,
                },
                Seg2::Line {
                    a: v(10.0, -4.0),
                    b: v(10.0, 0.0),
                },
                Seg2::Line {
                    a: v(10.0, 0.0),
                    b: v(0.0, 0.0),
                },
                Seg2::Line {
                    a: v(0.0, 0.0),
                    b: v(0.0, -7.0),
                },
                Seg2::Line {
                    a: v(0.0, -7.0),
                    b: v(7.0, -7.0),
                },
            ],
        },
        holes: vec![],
    };
    let bar = extrude_planar_regions(&Plane::XZ, &[section], -16.0, 16.0).unwrap();
    let below = Region2 {
        outer: Loop2::polygon(&[v(-16.0, a), v(-16.0, -2.0), v(16.0, -2.0), v(16.0, b)]),
        holes: vec![],
    };
    let cutter = extrude_planar_regions(&Plane::YZ, &[below], -20.0, 20.0).unwrap();
    boolean(bar, cutter, BooleanOp::Subtract).unwrap().unwrap()
}

/// The slope of [`narrowing_wall`]'s cut, and `q = sqrt(1 + s^2)`: a point `d` above the cut
/// plane, along its normal `(0, -s, 1) / q`, stands `d q` above it vertically.
const SLOPE: f64 = 0.11875;

fn slope_q() -> f64 {
    (1.0 + SLOPE * SLOPE).sqrt()
}

/// [`narrowing_wall`]'s volume: the section's area `70 - 9 (1 - pi / 4)` times 32, less the cut
/// away below the plane, whose mean height over the bar is 1.9 (the plane is 1.9 high at
/// y = 0, and odd in y about it), over the bar's x = 0..10 and 32 length: `10 * 1.9 * 32`.
fn narrowing_wall_volume() -> f64 {
    32.0 * (70.0 - 9.0 * (1.0 - PI / 4.0)) - 10.0 * 1.9 * 32.0
}

/// The round `r`'s two vertices on the end face y = 16 of [`narrowing_wall`], in closed form.
/// The cut's normal `(0, -s, 1) / q` and the wall's `(-1, 0, 0)` are square, and the cylinder's
/// axis is x = 7, z = 4, radius 3, so a ball of radius r touching the cut and the cylinder from
/// inside has its centre `r q` above the plane vertically and `3 - r` from the axis.
/// - On the cylinder: the corner's section in the end face y = 16, its centre at
///   `z = 3.8 + r q` (the cut is 3.8 high there), `x = 7 + sqrt((3 - r)^2 - (z - 4)^2)`; its
///   contact is the axis's point plus `3 / (3 - r)` of the way to the centre.
/// - On the cut: the round's section square to its edge through the edge's end (10, 16, 3.8),
///   whose centre lies `s r / q` back along y; its contact is the centre's foot on the plane,
///   at (x, 16, 3.8).
fn end_vertices(r: f64) -> [[f64; 3]; 2] {
    end_vertices_between(0.0, 3.8, r)
}

/// [`end_vertices`] for [`narrowing_wall_between`]`(a, b)`: its cut has slope
/// `s = (b - a) / 32` and is `c + s y` high, `c = a + 16 s` (1.9 and 0.11875 for
/// [`narrowing_wall`]), and `b` at the end face.
fn end_vertices_between(a: f64, b: f64, r: f64) -> [[f64; 3]; 2] {
    let s = (b - a) / 32.0;
    let c = a + 16.0 * s;
    let q = (1.0 + s * s).sqrt();
    let x_at = |z: f64| 7.0 + ((3.0 - r).powi(2) - (z - 4.0).powi(2)).sqrt();
    let zc = b + q * r;
    let xc = x_at(zc);
    let f = 3.0 / (3.0 - r);
    let on_cylinder = [7.0 + f * (xc - 7.0), 16.0, 4.0 + f * (zc - 4.0)];
    let yb = 16.0 - s * r / q;
    let zb = c + s * yb + q * r;
    let on_cut = [x_at(zb), 16.0, b];
    [on_cylinder, on_cut]
}

/// A lower bound on the material the round `r` takes off [`narrowing_wall`]: in each section
/// square to the edge whose wall contact lies on the flat wall (`z <= 4`) and which lies wholly
/// in the bar (`y >= -16`), the round takes `(1 - pi / 4) r^2` (the faces are square). That
/// section through the edge's point at y spans y down to `y - s r / q` and has its wall contact
/// `r / q` above the edge's `1.9 + s y`, so y runs from `-16 + s r / q` to `(2.1 - r / q) / s`;
/// along the edge that is `q` times as long (Cavalieri over the parallel sections).
fn flat_wall_removal(r: f64) -> f64 {
    flat_wall_removal_between(0.0, 3.8, r)
}

/// [`flat_wall_removal`] for [`narrowing_wall_between`]`(a, b)` (its cut as in
/// [`end_vertices_between`]): y runs from `-16 + s r / q` to `(4 - c - r / q) / s`.
fn flat_wall_removal_between(a: f64, b: f64, r: f64) -> f64 {
    let s = (b - a) / 32.0;
    let c = a + 16.0 * s;
    let q = (1.0 + s * s).sqrt();
    let from = -16.0 + s * r / q;
    let to = (4.0 - c - r / q) / s;
    (1.0 - PI / 4.0) * r * r * q * (to - from)
}

/// The distance from `want` to the nearest vertex of `b`.
fn nearest_vertex(b: &Body, want: [f64; 3]) -> f64 {
    let (_, brep) = common::kernel_copy(b);
    brep.vertex_list()
        .iter()
        .map(|&v| {
            let p = solidcore::topology::vertex::point(&brep, v).unwrap();
            ((p.x - want[0]).powi(2) + (p.y - want[1]).powi(2) + (p.z - want[2]).powi(2)).sqrt()
        })
        .fold(f64::INFINITY, f64::min)
}

/// SolidCraft #15767 (j4py's repro): the round of the edge between the sloped cut and the wall
/// x = 10. Its contact on the wall stands `r q` above the cut vertically, and the flat wall is
/// lower than that from y = (2.1 - r q) / s on (9.204 for r = 1), so from there its contact
/// line runs on the R3 cylinder across the smooth edge at z = 4, partway along the round's
/// edge. It rolls over (before the fix: refused as too large at 0.1914, the rail leaving the
/// wall). The round's end vertices are the closed-form ones ([`end_vertices`]) and it takes at
/// least the flat wall's part ([`flat_wall_removal`]).
#[test]
fn a_round_whose_wall_contact_crosses_a_tangent_edge_partway_rolls_onto_the_cylinder() {
    fixes();
    for r in [0.5, 1.0, 1.5] {
        let b = narrowing_wall();
        let v0 = b.volume(1e-9).unwrap();
        let want = narrowing_wall_volume();
        assert!((v0 - want).abs() <= 1e-9 * want, "{v0} against {want}");
        let e = edge_at(&b, [10.0, 0.0, 1.9]);
        blend(&b, Blend::Round, &[e], &[r], &[], Corners::Filled)
            .unwrap_or_else(|err| panic!("r {r}: {err:?}"));
        let report = b.check(CheckLevel::All).unwrap();
        assert!(report.is_valid(), "r {r}: {report}");
        let (_, brep) = common::kernel_copy(&b);
        let points: Vec<[f64; 3]> = brep
            .vertex_list()
            .iter()
            .map(|&v| {
                let p = solidcore::topology::vertex::point(&brep, v).unwrap();
                [p.x, p.y, p.z]
            })
            .collect();
        for want in end_vertices(r) {
            let near = points
                .iter()
                .map(|p| {
                    ((p[0] - want[0]).powi(2) + (p[1] - want[1]).powi(2) + (p[2] - want[2]).powi(2))
                        .sqrt()
                })
                .fold(f64::INFINITY, f64::min);
            assert!(near <= 1e-6, "r {r}: no vertex at {want:?} ({near})");
        }
        let taken = v0 - b.volume(1e-9).unwrap();
        let least = flat_wall_removal(r);
        assert!(taken > least, "r {r}: takes {taken}, at least {least}");
    }
}

/// The fit margin: a thousand of the edge's tolerance, 1e-5.
const NARROWING_FIT_MARGIN: f64 = 1000.0 * 1e-5;

/// SolidCraft #15767 (review of #20757): the app-built [`narrowing_wall`] rounded wider than
/// its R3 cylinder. The rail leaves the wall across the smooth edge onto the cylinder at the
/// end face's side edge, but no ball of radius r > 3 touches the convex cylinder there from the
/// round's side: the cylinder's offset by the ball, `1 - r / 3` times its motion around it,
/// folds back. So the rail does not roll over there, and the round stays refused as on main,
/// naming the radius on the wall of
/// [`a_round_whose_rolled_rail_cannot_be_rolled_is_refused_naming_the_radius_on_the_wall`] with
/// the same derivation and allowance (the app body's edge has the same length; measured: 4.5e-16
/// from the test body's size at r 3.5). Before the fold test, r 3.5 rolled over and failed to
/// build the round's side intersection (a bare `BlendFailed`).
#[test]
fn the_app_built_narrowing_wall_wider_than_its_cylinder_stays_refused_naming_the_radius_on_the_wall(
) {
    fixes();
    let q = slope_q();
    let want = 0.2 * q - NARROWING_FIT_MARGIN + 2e-9 * 32.0 * q * SLOPE;
    for r in [3.25, 3.5, 3.75] {
        let b = Body::from_smb(
            include_bytes!("data/issue_15767_narrowing_wall_from_the_app.smb").to_vec(),
            false,
        )
        .unwrap();
        let before = b.volume(1e-9).unwrap();
        let e = edge_at(&b, [10.0, 0.0, 1.9]);
        match blend(&b, Blend::Round, &[e], &[r], &[], Corners::Filled) {
            Err(Error::TooLarge { size, why, .. }) => {
                assert_eq!(why, Some(RoomReason::RailLeavesFace), "r {r}");
                assert!((size - want).abs() <= 1e-12, "r {r}: {size} against {want}");
            }
            Err(err) => panic!("r {r}: {err:?}"),
            Ok(_) => panic!("r {r}: built"),
        }
        assert_eq!(b.volume(1e-9).unwrap().to_bits(), before.to_bits(), "r {r}");
    }
}

/// SolidCraft #15767: a round of [`narrowing_wall`] whose rail, rolled onto the R3 cylinder,
/// cannot be rolled (the rollover recovery fails: from r 2 on, past the largest radius
/// `3.2 q / (1 + q)` = 1.605601 at which the ball, square to the edge at its end y = 16, still
/// touches the cylinder short of its edge x = 7, z = 7 onto the top) stays refused as too large
/// (with the fix's rollover alone, r 2 to 3.5 failed instead), naming the largest radius at
/// which its rail stays on the wall: the ball touching the wall's top edge z = 4 and the cut, in
/// the section square to the edge through its end (10, 16, 3.8). Its centre is r from the wall
/// at z = 4 and that section's plane `(y - 16) + s (z - 3.8) = 0` puts it at y = 16 - 0.2 s,
/// where the cut is `1.9 + s y` high: `(4 - 1.9 - s y) / q = (0.2 + 0.2 s^2) / q = 0.2 q`. Less
/// the fit margin, as on main (0.1914), plus the search's inset (below). The named size builds.
#[test]
fn a_round_whose_rolled_rail_cannot_be_rolled_is_refused_naming_the_radius_on_the_wall() {
    fixes();
    // The search (`largest_size_on_its_faces`) keeps the ball's centre inside the rounded
    // edge's ends by `1e-9 (1 + length)` of its parameter. That interval is [0, 1], so 2e-9 of
    // it. The edge is 32 q long, so that is 2e-9 * 32 q along it, and the limiting radius
    // `(2.1 - s y) q` falls by s per unit along it (y runs 1 / q per unit). The inset raises
    // the size by 2e-9 * 32 q s = 7.653e-9 (measured: 2.4e-14 off it).
    let q = slope_q();
    let want = 0.2 * q - NARROWING_FIT_MARGIN + 2e-9 * 32.0 * q * SLOPE;
    for r in [2.0, 2.5, 3.0, 3.5, 5.0] {
        let b = narrowing_wall();
        let before = b.volume(1e-9).unwrap();
        let e = edge_at(&b, [10.0, 0.0, 1.9]);
        match blend(&b, Blend::Round, &[e], &[r], &[], Corners::Filled) {
            Err(Error::TooLarge { size, why, .. }) => {
                assert_eq!(why, Some(RoomReason::RailLeavesFace), "r {r}");
                // The allowance, term by term:
                // - the golden-section bracket: 2 / 32 of the side edge's [0, 1] after 32
                //   samples, times 0.618^60 = 2.9e-13 after 60 steps, along 32 q at rate s:
                //   (2 / 32) 2.9e-13 * 32 * q * s = 7e-14;
                // - the centre's drop onto the rounded edge, to 1e-12 (EFF_ZERO) of distance,
                //   at rate s: 1.2e-13;
                // - rounding of the sizes near 0.2 through about 64 operations on coordinates
                //   up to 16: 64 * 2.2e-16 * 16 = 2.3e-13.
                // Together 4.2e-13; allowed 1e-12.
                assert!((size - want).abs() <= 1e-12, "r {r}: {size} against {want}");
            }
            Err(err) => panic!("r {r}: {err:?}"),
            Ok(_) => panic!("r {r}: built"),
        }
        assert_eq!(b.volume(1e-9).unwrap().to_bits(), before.to_bits(), "r {r}");
    }
    let b = narrowing_wall();
    let e = edge_at(&b, [10.0, 0.0, 1.9]);
    blend(&b, Blend::Round, &[e], &[want], &[], Corners::Filled).unwrap();
    let report = b.check(CheckLevel::All).unwrap();
    assert!(report.is_valid(), "{report}");
}

/// SolidCraft #15767 (review of #20757): a round of [`narrowing_wall_between`] whose rail,
/// rolled onto the R3 cylinder, nearly reaches the cylinder's far edge onto the top at the end
/// face: the cut is only 0.01 under the wall's top at y = 16, so the ball of radius 1.5 nearly
/// fills the end face's 3.01 between the cut and the top. Its end rib on the face turns through
/// more than a half turn, so the chord between the rib's ends heads against the rib's start;
/// the rib is traced from its start into the round's surface. It builds, valid; its end
/// vertices are the closed-form ones ([`end_vertices_between`]) and it takes at least the flat
/// wall's part ([`flat_wall_removal_between`]). Main refused these as too large at 5.9e-5, the
/// rail leaving the 0.01-high wall.
#[test]
fn a_round_nearly_filling_its_end_face_builds_its_end_rib_through_more_than_a_half_turn() {
    fixes();
    let (a, cut_b) = (0.5, 3.99);
    for r in [0.1, 1.0, 1.5] {
        let b = narrowing_wall_between(a, cut_b);
        let v0 = b.volume(1e-9).unwrap();
        let e = edge_at(&b, [10.0, 0.0, 0.5 * (a + cut_b)]);
        blend(&b, Blend::Round, &[e], &[r], &[], Corners::Filled)
            .unwrap_or_else(|err| panic!("r {r}: {err:?}"));
        let report = b.check(CheckLevel::All).unwrap();
        assert!(report.is_valid(), "r {r}: {report}");
        for want in end_vertices_between(a, cut_b, r) {
            let near = nearest_vertex(&b, want);
            assert!(near <= 1e-6, "r {r}: no vertex at {want:?} ({near})");
        }
        let taken = v0 - b.volume(1e-9).unwrap();
        let least = flat_wall_removal_between(a, cut_b, r);
        assert!(taken > least, "r {r}: takes {taken}, at least {least}");
    }
}

/// SolidCraft #15767 (review of #20757): [`narrowing_wall`] as SolidCraft builds it
/// (`data/issue_15767_narrowing_wall_from_the_app.smb`: the same solid, its volume the closed
/// form, with other entity order, plane parametrisations and edge tolerances). The roll-over
/// splits the round into the rolled piece and the piece after it, joined by a cross section.
/// The section's use on the rolled piece had no UV curve of its own and read the other piece's,
/// on another surface, so the rolled piece's face capped the section with iso lines of its own
/// domain; here that cap ran down the domain's boundary to y = -0.9 and the result was not a
/// closed solid (main refused these as too large at 0.2). The section is dropped onto the
/// rolled piece: each builds, valid, with the closed-form end vertices ([`end_vertices`]),
/// taking at least the flat wall's part ([`flat_wall_removal`]).
#[test]
fn the_app_built_narrowing_wall_rounds_with_the_section_on_the_rolled_piece() {
    fixes();
    let bytes = include_bytes!("data/issue_15767_narrowing_wall_from_the_app.smb");
    for r in [0.25, 0.5, 1.0] {
        let b = Body::from_smb(bytes.to_vec(), false).unwrap();
        let v0 = b.volume(1e-9).unwrap();
        let want = narrowing_wall_volume();
        assert!((v0 - want).abs() <= 1e-9 * want, "{v0} against {want}");
        let e = edge_at(&b, [10.0, 0.0, 1.9]);
        blend(&b, Blend::Round, &[e], &[r], &[], Corners::Filled)
            .unwrap_or_else(|err| panic!("r {r}: {err:?}"));
        let report = b.check(CheckLevel::All).unwrap();
        assert!(report.is_valid(), "r {r}: {report}");
        for want in end_vertices(r) {
            let near = nearest_vertex(&b, want);
            assert!(near <= 1e-6, "r {r}: no vertex at {want:?} ({near})");
        }
        let taken = v0 - b.volume(1e-9).unwrap();
        let least = flat_wall_removal(r);
        assert!(taken > least, "r {r}: takes {taken}, at least {least}");
    }
}

/// SolidCraft #15767 (review of #20757): a round of [`narrowing_wall_between`]`(a, b)` whose
/// rail crosses the wall's top edge z = 4 so near the end face y = 16 that its roll-over does
/// not lie wholly before the end face. The end face's wall is `h = 4 - b` high. In the band
/// `h / q < r < h q` the rail passes the side edge's top P = (10, 16, 4), yet the ball of the
/// round touching the cut at its end on the end face still touches the flat wall below z = 4:
/// the section square to the edge through the rail's roll-over point meets the cut past the end
/// face. The round's two pieces would then part along a seam that crosses the end face, the
/// rolled piece three-sided, which the corner does not build. It stays refused as on main,
/// naming the radius at which the rail reaches P: the ball touching the wall at P has its centre
/// at (10 - rho, 16, 4), whose distance to the cut is `h / q` whatever rho, so `rho = h / q`.
/// That root is bisected until its bracket is under `1e-12 (1 + r)` (`ball_touching_on`), and
/// the distances near 4 carry rounding under 1e-14. A round a fiftieth smaller builds.
#[test]
fn a_round_whose_roll_over_crosses_the_end_face_stays_refused_naming_the_radius_reaching_its_side_edge(
) {
    fixes();
    for (a, cut_b, r) in [
        (0.0_f64, 3.8_f64, 0.2_f64),
        (0.0, 3.0, 1.0),
        (0.0, 3.95, 0.05),
        (1.0, 3.9, 0.1),
        (0.0, 2.0, 2.0),
        (2.0, 3.6, 0.4),
    ] {
        let s = (cut_b - a) / 32.0;
        let q = (1.0 + s * s).sqrt();
        let want = (4.0 - cut_b) / q;
        assert!(
            want < r && r < (4.0 - cut_b) * q,
            "({a}, {cut_b}) r {r}: not in the band"
        );
        let b = narrowing_wall_between(a, cut_b);
        let before = b.volume(1e-9).unwrap();
        let e = edge_at(&b, [10.0, 0.0, 0.5 * (a + cut_b)]);
        match blend(&b, Blend::Round, &[e], &[r], &[], Corners::Filled) {
            Err(Error::TooLarge { size, why, .. }) => {
                assert_eq!(why, None, "({a}, {cut_b}) r {r}");
                let allowed = 1e-12 * (1.0 + r) + 1e-14;
                assert!(
                    (size - want).abs() <= allowed,
                    "({a}, {cut_b}) r {r}: {size} against {want}"
                );
            }
            Err(err) => panic!("({a}, {cut_b}) r {r}: {err:?}"),
            Ok(_) => panic!("({a}, {cut_b}) r {r}: built"),
        }
        assert_eq!(b.volume(1e-9).unwrap().to_bits(), before.to_bits());
        let smaller = narrowing_wall_between(a, cut_b);
        let e = edge_at(&smaller, [10.0, 0.0, 0.5 * (a + cut_b)]);
        blend(
            &smaller,
            Blend::Round,
            &[e],
            &[0.98 * want],
            &[],
            Corners::Filled,
        )
        .unwrap_or_else(|err| panic!("({a}, {cut_b}) {}: {err:?}", 0.98 * want));
        let report = smaller.check(CheckLevel::All).unwrap();
        assert!(report.is_valid(), "({a}, {cut_b}): {report}");
    }
}
