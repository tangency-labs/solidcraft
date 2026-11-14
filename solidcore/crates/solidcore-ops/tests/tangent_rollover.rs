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
/// it crosses the slot's edges. It is not proved on the foot round
/// ([`RoomReason::RailLeavesFace`]'s refusal, which without the corpus's fixes reads as a rail
/// leaving its face without the cliff overflow fix). Without that proof the blend failed later,
/// in the trace's rollover, with status 1001; with the corpus's fixes it fails before the trim.
#[test]
fn a_rim_round_whose_top_reaches_the_contact_between_sections_is_refused() {
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
    match result {
        Err(Error::Unsupported(reason)) if reason.contains("rail leaves its face") => {}
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
/// "the smooth end cap is not built": the End at the edge's end on x = 0 never built its
/// edges. Its rail on the R3 round is the trace's Hermite approximation, whose spans the
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
/// did, "the smooth end cap is not built": the End on x = 0 found its rail end on the R3 round
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
    for r4 in [8.0, 10.0, 10.6] {
        assert!(-65.0 - r4 > top_end_on_side());
        for corpus in [true, false] {
            assert_corner_round_under_foot_round_builds(r4, corpus);
        }
    }
}
