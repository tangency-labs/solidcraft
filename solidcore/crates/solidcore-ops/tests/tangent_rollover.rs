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

mod common;

use std::f64::consts::PI;

use common::edge_at;
use solidcore_ops::blend::{blend, Blend, Corners};
use solidcore_ops::boolean::boolean;
use solidcore_ops::{make_box, session, Body, BooleanOp, CheckLevel, Error, RoomReason};

const LENGTH: f64 = 10.0;

/// The corpus's fix set (`kernel_cases/fixes.json`, the set SolidCraft runs).
fn fixes() {
    let fixes: Vec<String> = serde_json::from_str(include_str!(
        "../../solidcore/tests/kernel_cases/fixes.json"
    ))
    .unwrap();
    session::set_thread_fixes(fixes.iter().map(String::as_str)).unwrap();
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

/// The rails of edge 28's round in `made`, checked against the closed forms of the faces it
/// rolls on: every edge the round made (no edge of `input` passes through its middle) that
/// lies on the R3 round's cylinder, the pocket's sphere or the plane z = 7.5 lies, at 17
/// points, within the edge's tolerance (1e-5) of it and within that face (the cylinder
/// between its tangent edges x = 0 and z = 7.5, the plane past the tangent edge x = 3). At
/// least one rail lies on each: the round rolls on the R3 round and the pocket and over the
/// tangent edge onto z = 7.5.
fn assert_b27_rails_on_faces(input: &Body, made: &Body, r: f64) {
    use solidcore::query::api_queries::{edge_closest_point, get_edges};
    let tol = 1e-5;
    let (ki, bi) = common::kernel_copy(input);
    let (k, b) = common::kernel_copy(made);
    let input_edges = get_edges(&bi).unwrap();
    let mut counts = [0; 3];
    for e in get_edges(&b).unwrap() {
        let edge = b.edge(e).unwrap();
        let curve = edge.curve.as_ref().unwrap();
        let ivl = edge.interval;
        let pts: Vec<_> = (0..=16)
            .map(|i| {
                let t = ivl.min() + (ivl.max() - ivl.min()) * f64::from(i) / 16.0;
                curve.evaluate_point(&k, t).unwrap()
            })
            .collect();
        let mid = pts[8];
        let old = input_edges.iter().any(|&f| {
            edge_closest_point(&ki, &bi, f, mid)
                .unwrap()
                .closest
                .distance
                <= tol
        });
        if old {
            continue;
        }
        let on_cylinder = pts
            .iter()
            .all(|p| ((p.x - 3.0).hypot(p.z - 4.5) - 3.0).abs() <= tol);
        let on_sphere = pts.iter().all(|p| {
            ((p.x - 2.5).powi(2) + (p.y - 50.0).powi(2) + p.z * p.z).sqrt() - 10.0 <= tol
                && 10.0 - ((p.x - 2.5).powi(2) + (p.y - 50.0).powi(2) + p.z * p.z).sqrt() <= tol
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
/// closes. (Between 1.945 and the cutoff the trim now succeeds and the End's patch fails as
/// at 1.6, "the smooth end cap is not built": a separate defect.)
#[test]
fn the_size_the_past_an_end_refusal_names_builds() {
    let (input, _) = b27();
    let before = input.volume(1e-9).unwrap();
    for r in [b27_edge_28_cutoff() - B27_FIT_MARGIN, 1.94] {
        let made = b27_edge_28(r).unwrap_or_else(|e| panic!("r {r}: {e:?}"));
        assert!(made.check(CheckLevel::All).unwrap().is_valid(), "r {r}");
        let after = made.volume(1e-9).unwrap();
        assert!(after > 0.0 && after < before, "r {r}: {after} of {before}");
        assert_b27_rails_on_faces(&input, &made, r);
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
        assert_b27_rails_on_faces(&input, &made, r);
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
