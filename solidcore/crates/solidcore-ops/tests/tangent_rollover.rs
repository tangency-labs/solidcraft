//! Tangent rollover: a concave round whose contact line reaches a smooth edge of the face it
//! rolls on, and rolls on across it onto the face beyond.
//!
//! The body is an L step swept 10 along z, its section in the xy plane: a wall x = 0 standing on
//! a strip y = 0 that is `w` wide, then a cliff x = w down to y = -5. The cliff's rim (w, 0) is
//! rounded `r1` first, which leaves the strip flat for `a = w - r1` and runs it on tangentially
//! into the rim's round. The wall's foot (0, 0) is then rounded `r`: past `a`, the ball rolls off
//! the flat onto the rim's round, the whole length of the edge, and the strip is consumed.

#![allow(clippy::disallowed_methods)] // closed-form oracle, not ops geometry
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // test

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
