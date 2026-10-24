//! Harness for the constructive seed on lines, arcs and tangency: a filleted rectangle, a slot,
//! a tangent line–arc–line chain (also at axis-aligned line angles), a bolt circle, a mirrored
//! point, collinear lines, points at a distance from a line, equal lines and points on an arc
//! of unknown radius, under scaled dimension edits. Every arc must keep its side of its lines
//! and its sweep, every line its direction, every point its side of its lines.
//! Deterministic (fixed LCG seeds), no timing.

use solidcraft_geom::Vec2;

use crate::construct::{Placement, seed};
use crate::decompose::Budget;
use crate::solver::points_on_curves;
use crate::tests::{Lcg, given, seed_of, seeded, v};
use crate::*;

/// What an edit must keep.
#[derive(Clone, Copy, Debug)]
enum Check {
    /// `p` stays on its side of the line through `a` and `b`.
    Side { p: PointIdx, a: PointIdx, b: PointIdx },
    /// The line from `a` to `b` points within a quarter turn of `d` (where the edit turns it).
    Toward { a: PointIdx, b: PointIdx, d: Vec2 },
    /// `p` stays ahead of or behind `a` along the line from `a` to `b`.
    Along { p: PointIdx, a: PointIdx, b: PointIdx },
    /// The counter-clockwise sweep of the arc `curve` stays in `lo..hi` (radians).
    Sweep { curve: CurveIdx, lo: f64, hi: f64 },
    /// `p` ends at `at`.
    At { p: PointIdx, at: Vec2 },
}

/// A family member: the solved sketch, the dimension edits (constraint index, new value) and
/// what they must keep.
struct Case {
    sk: Sketch,
    edits: Vec<(usize, f64)>,
    checks: Vec<Check>,
}

fn sweep(sk: &Sketch, curve: CurveIdx) -> f64 {
    let CurveKind::Arc { c, a, b } = sk.curves[curve].kind else { return f64::NAN };
    let (c, a, b) = (sk.points[c].pos, sk.points[a].pos, sk.points[b].pos);
    ((b - c).angle() - (a - c).angle()).rem_euclid(std::f64::consts::TAU)
}

/// Does `sk` keep every check (sides as in `before`)?
fn holds(sk: &Sketch, checks: &[Check], before: &Sketch) -> bool {
    let p = |i: PointIdx| sk.points[i].pos;
    checks.iter().all(|c| match *c {
        Check::Side { p: x, a, b } => {
            let side = |s: &Sketch| (s.points[b].pos - s.points[a].pos).cross(s.points[x].pos - s.points[a].pos).signum();
            side(sk) == side(before)
        }
        Check::Toward { a, b, d } => (p(b) - p(a)).dot(d) > 0.0,
        Check::Along { p: x, a, b } => {
            let along = |s: &Sketch| (s.points[b].pos - s.points[a].pos).dot(s.points[x].pos - s.points[a].pos).signum();
            along(sk) == along(before)
        }
        Check::Sweep { curve, lo, hi } => (lo..hi).contains(&sweep(sk, curve)),
        Check::At { p: x, at } => p(x).dist(at) < 1e-6,
    })
}

/// The member's sketch with its edits applied.
fn edited(case: &Case) -> Sketch {
    let mut sk = case.sk.clone();
    for (i, value) in &case.edits {
        sk.constraints[*i].kind.set_value(*value);
    }
    sk
}

/// Apply the edits and solve; (failed, a check broke).
fn run(case: &Case, on: bool) -> (bool, bool) {
    let mut sk = edited(case);
    let rep = seeded(on, || solve(&mut sk));
    if !rep.ok() {
        return (true, false);
    }
    (false, !holds(&sk, &case.checks, &case.sk))
}

fn push(sk: &mut Sketch, kind: ConstraintKind) -> usize {
    sk.add_constraint(kind, None).unwrap();
    sk.constraints.len() - 1
}

/// The four arcs' ends and centres of a rounded rectangle, CCW: bottom, right, top and left
/// lines between quarter arcs at the corners, every line tangent to both its arcs. The bottom
/// line's start is fixed; lengths of the bottom and left lines and the four radii are
/// dimensions, scaled by `lo..hi`. Only edits where the top and right lines keep a length are
/// kept.
fn filleted_rectangle(rnd: &mut Lcg, lo: f64, hi: f64) -> Option<Case> {
    let o = v(rnd.range(-50.0, 50.0), rnd.range(-50.0, 50.0));
    let (w, h) = (rnd.range(60.0, 100.0), rnd.range(40.0, 70.0));
    let r: [f64; 4] = std::array::from_fn(|_| rnd.range(4.0, 12.0)); // BR, TR, TL, BL
    let mut sk = Sketch::new();
    let at = |x: f64, y: f64| o + v(x, y);
    let bottom = sk.add_line(at(r[3], 0.0), at(w - r[0], 0.0), None, None, None).unwrap();
    let right = sk.add_line(at(w, r[0]), at(w, h - r[1]), None, None, None).unwrap();
    let top = sk.add_line(at(w - r[1], h), at(r[2], h), None, None, None).unwrap();
    let left = sk.add_line(at(0.0, h - r[2]), at(0.0, r[3]), None, None, None).unwrap();
    let ends = |sk: &Sketch, l: CurveIdx| sk.curves[l].kind.ends().unwrap();
    let ((b0, b1), (r0, r1), (t0, t1), (l0, l1)) = (ends(&sk, bottom), ends(&sk, right), ends(&sk, top), ends(&sk, left));
    let corners = [
        (at(w - r[0], r[0]), b1, r0, bottom, right),
        (at(w - r[1], h - r[1]), r1, t0, right, top),
        (at(r[2], h - r[2]), t1, l0, top, left),
        (at(r[3], r[3]), l1, b0, left, bottom),
    ];
    let mut checks = Vec::new();
    let mut radius = Vec::new();
    for (k, (c, a, b, la, lb)) in corners.into_iter().enumerate() {
        let (pa, pb) = (sk.points[a].pos, sk.points[b].pos);
        let arc = sk.add_arc(c, pa, pb, [None, Some(a), Some(b)], None).unwrap();
        let CurveKind::Arc { c: centre, .. } = sk.curves[arc].kind else { panic!() };
        push(&mut sk, ConstraintKind::Tangent { a: la, b: arc });
        push(&mut sk, ConstraintKind::Tangent { a: lb, b: arc });
        radius.push((push(&mut sk, ConstraintKind::Radius { c: arc, value: r[k] }), r[k]));
        for l in [la, lb] {
            let (a, b) = ends(&sk, l);
            checks.push(Check::Side { p: centre, a, b });
        }
        checks.push(Check::Sweep { curve: arc, lo: 0.0, hi: std::f64::consts::PI });
    }
    for l in [bottom, right, top, left] {
        let (a, b) = ends(&sk, l);
        checks.push(Check::Toward { a, b, d: sk.points[b].pos - sk.points[a].pos });
    }
    push(&mut sk, ConstraintKind::Fix { p: b0 });
    push(&mut sk, ConstraintKind::Horizontal { l: bottom });
    push(&mut sk, ConstraintKind::Horizontal { l: top });
    push(&mut sk, ConstraintKind::Vertical { l: left });
    push(&mut sk, ConstraintKind::Vertical { l: right });
    let (lb, ll) = (w - r[3] - r[0], h - r[2] - r[3]);
    let len_b = push(&mut sk, ConstraintKind::Length { l: bottom, value: lb });
    let len_l = push(&mut sk, ConstraintKind::Length { l: left, value: ll });
    let mut s = || rnd.range(lo, hi);
    let r2: Vec<f64> = radius.iter().map(|(_, r)| r * s()).collect();
    let (lb2, ll2) = (lb * s(), ll * s());
    // Width and height after the edit; the top and right lines must keep a length.
    let (w2, h2) = (lb2 + r2[3] + r2[0], ll2 + r2[2] + r2[3]);
    if w2 - r2[1] - r2[2] < 2.0 || h2 - r2[0] - r2[1] < 2.0 {
        return None;
    }
    let mut edits: Vec<(usize, f64)> = radius.iter().zip(&r2).map(|((i, _), r)| (*i, *r)).collect();
    edits.extend([(len_b, lb2), (len_l, ll2)]);
    Some(Case { sk, edits, checks })
}

/// A slot at an angle: two semicircular caps of equal radius joined by two lines tangent to
/// both, the bottom line at a line angle and the top one parallel to it, cap A's centre fixed.
/// The angle turns by up to ±145°, the radius and the cap distance (centre distance, or the
/// overall length) scale by `lo..hi`.
fn slot(rnd: &mut Lcg, lo: f64, hi: f64, overall: bool) -> Option<Case> {
    let th = rnd.range(-3.1, 3.1);
    let (len, r) = (rnd.range(30.0, 60.0), rnd.range(5.0, 13.0));
    let (u, n) = (Vec2::from_angle(th), Vec2::from_angle(th).perp());
    let mut sk = Sketch::new();
    let ca = v(rnd.range(-40.0, 40.0), rnd.range(-40.0, 40.0));
    let cb = ca + u * len;
    let bottom = sk.add_line(ca - n * r, cb - n * r, None, None, None).unwrap();
    let top = sk.add_line(cb + n * r, ca + n * r, None, None, None).unwrap();
    let (ba, bb) = sk.curves[bottom].kind.ends().unwrap();
    let (tb, ta) = sk.curves[top].kind.ends().unwrap();
    let arc_b = sk.add_arc(cb, cb - n * r, cb + n * r, [None, Some(bb), Some(tb)], None).unwrap();
    let arc_a = sk.add_arc(ca, ca + n * r, ca - n * r, [None, Some(ta), Some(ba)], None).unwrap();
    let centre = |sk: &Sketch, a: CurveIdx| match sk.curves[a].kind {
        CurveKind::Arc { c, .. } => c,
        _ => panic!(),
    };
    let (pa, pb) = (centre(&sk, arc_a), centre(&sk, arc_b));
    push(&mut sk, ConstraintKind::Fix { p: pa });
    let angle = push(&mut sk, ConstraintKind::LineAngle { l: bottom, value: th });
    push(&mut sk, ConstraintKind::Parallel { a: top, b: bottom });
    for (l, a) in [(bottom, arc_a), (bottom, arc_b), (top, arc_a), (top, arc_b)] {
        push(&mut sk, ConstraintKind::Tangent { a: l, b: a });
    }
    let radius = push(&mut sk, ConstraintKind::Radius { c: arc_a, value: r });
    push(&mut sk, ConstraintKind::Equal { a: arc_a, b: arc_b });
    let length = if overall {
        push(&mut sk, ConstraintKind::SlotOverallLength { a: arc_a, b: arc_b, value: len + 2.0 * r })
    } else {
        push(&mut sk, ConstraintKind::Distance { p: pa, q: pb, value: len })
    };
    let (r2, len2) = (r * rnd.range(lo, hi), len * rnd.range(lo, hi));
    let th2 = th + rnd.range(-2.5, 2.5);
    let half = std::f64::consts::FRAC_PI_2;
    let checks = vec![
        Check::Side { p: pa, a: ba, b: bb },
        Check::Side { p: pb, a: ba, b: bb },
        Check::Side { p: pa, a: tb, b: ta },
        Check::Side { p: pb, a: tb, b: ta },
        Check::Sweep { curve: arc_a, lo: half, hi: 3.0 * half },
        Check::Sweep { curve: arc_b, lo: half, hi: 3.0 * half },
        Check::Toward { a: pa, b: pb, d: Vec2::from_angle(th2) },
    ];
    let edits = vec![(angle, th2), (radius, r2), (length, if overall { len2 + 2.0 * r2 } else { len2 })];
    Some(Case { sk, edits, checks })
}

/// Line – arc – line, tangent at shared ends: L1 from a fixed point at a line angle with a
/// length, the arc turning left or right by β with a radius, L2 at β from L1 with a length.
/// Lengths and radius scale by `lo..hi`, the line angle turns by up to ±115°, β scales within
/// 15°–165°.
fn tangent_chain(rnd: &mut Lcg, lo: f64, hi: f64) -> Option<Case> {
    let alpha = rnd.range(-3.1, 3.1);
    let alpha2 = alpha + rnd.range(-2.0, 2.0);
    chain(rnd, lo, hi, alpha, alpha2)
}

/// [`tangent_chain`] with L1 along an axis, turned to another axis (a quarter or half turn
/// either way).
fn axis_chain(rnd: &mut Lcg, lo: f64, hi: f64) -> Option<Case> {
    let quarter = std::f64::consts::FRAC_PI_2;
    let alpha = quarter * (rnd.next() * 4.0).floor() - std::f64::consts::PI;
    let turn = [-2.0, -1.0, 1.0, 2.0][(rnd.next() * 4.0) as usize % 4];
    chain(rnd, lo, hi, alpha, alpha + quarter * turn)
}

/// The chain of [`tangent_chain`] with L1 at `alpha`, edited to `alpha2`.
fn chain(rnd: &mut Lcg, lo: f64, hi: f64, alpha: f64, alpha2: f64) -> Option<Case> {
    let (a, b, r) = (rnd.range(20.0, 40.0), rnd.range(20.0, 40.0), rnd.range(5.0, 15.0));
    let beta = rnd.range(30f64.to_radians(), 150f64.to_radians());
    let turn = rnd.sign();
    let p0 = v(rnd.range(-30.0, 30.0), rnd.range(-30.0, 30.0));
    let u1 = Vec2::from_angle(alpha);
    let u2 = Vec2::from_angle(alpha + turn * beta);
    let e1 = p0 + u1 * a;
    let c = e1 + u1.perp() * (turn * r);
    let e2 = c - u2.perp() * (turn * r);
    let p3 = e2 + u2 * b;
    let mut sk = Sketch::new();
    let l1 = sk.add_line(p0, e1, None, None, None).unwrap();
    let (i0, i1) = sk.curves[l1].kind.ends().unwrap();
    let l2 = sk.add_line(e2, p3, None, None, None).unwrap();
    let (i2, i3) = sk.curves[l2].kind.ends().unwrap();
    let arc = if turn > 0.0 {
        sk.add_arc(c, e1, e2, [None, Some(i1), Some(i2)], None).unwrap()
    } else {
        sk.add_arc(c, e2, e1, [None, Some(i2), Some(i1)], None).unwrap()
    };
    let CurveKind::Arc { c: ic, .. } = sk.curves[arc].kind else { panic!() };
    push(&mut sk, ConstraintKind::Fix { p: i0 });
    let angle = push(&mut sk, ConstraintKind::LineAngle { l: l1, value: alpha });
    let len1 = push(&mut sk, ConstraintKind::Length { l: l1, value: a });
    let bend = push(&mut sk, ConstraintKind::Angle { a: l1, b: l2, value: turn * beta, flip: false });
    let len2 = push(&mut sk, ConstraintKind::Length { l: l2, value: b });
    let radius = push(&mut sk, ConstraintKind::Radius { c: arc, value: r });
    push(&mut sk, ConstraintKind::Tangent { a: l1, b: arc });
    push(&mut sk, ConstraintKind::Tangent { a: l2, b: arc });
    let mut s = || rnd.range(lo, hi);
    let beta2 = (beta * s()).clamp(15f64.to_radians(), 165f64.to_radians());
    let edits = vec![(len1, a * s()), (len2, b * s()), (radius, r * s()), (bend, turn * beta2), (angle, alpha2)];
    let checks = vec![
        Check::Side { p: ic, a: i0, b: i1 },
        Check::Side { p: ic, a: i2, b: i3 },
        Check::Sweep { curve: arc, lo: 0.0, hi: std::f64::consts::PI },
        Check::Toward { a: i0, b: i1, d: Vec2::from_angle(alpha2) },
        Check::Toward { a: i2, b: i3, d: Vec2::from_angle(alpha2 + turn * beta2) },
    ];
    Some(Case { sk, edits, checks })
}

/// Bolt circle: `k` points on a circle (diameter dimension, centre fixed), each on a line from
/// the centre; the first line at a line angle, each next at 360°/k from the one before. The
/// diameter scales by `lo..hi`, the first angle turns by up to ±170°; every point must end where
/// the dimensions put it.
fn bolt_circle(rnd: &mut Lcg, lo: f64, hi: f64) -> Option<Case> {
    let k = 3 + (rnd.next() * 6.0) as usize;
    let r = rnd.range(15.0, 40.0);
    let phi = rnd.range(-3.1, 3.1);
    let step = std::f64::consts::TAU / k as f64;
    let mut sk = Sketch::new();
    let centre = sk.add_point(v(rnd.range(-30.0, 30.0), rnd.range(-30.0, 30.0)), None).unwrap();
    let c = sk.points[centre].pos;
    let circle = sk.add_circle(c, r, Some(centre), None).unwrap();
    push(&mut sk, ConstraintKind::Fix { p: centre });
    let diameter = push(&mut sk, ConstraintKind::Diameter { c: circle, value: 2.0 * r });
    let mut lines = Vec::new();
    let mut holes = Vec::new();
    for i in 0..k {
        let h = sk.add_point(c + Vec2::from_angle(phi + i as f64 * step) * r, None).unwrap();
        lines.push(sk.add_line_pts(centre, h, None).unwrap());
        holes.push(h);
        push(&mut sk, ConstraintKind::PointOnCurve { p: h, c: circle });
    }
    let angle = push(&mut sk, ConstraintKind::LineAngle { l: lines[0], value: phi });
    for w in lines.windows(2) {
        push(&mut sk, ConstraintKind::Angle { a: w[0], b: w[1], value: step, flip: false });
    }
    let (r2, phi2) = (r * rnd.range(lo, hi), phi + rnd.range(-3.0, 3.0));
    let checks = holes.iter().enumerate().map(|(i, h)| Check::At { p: *h, at: c + Vec2::from_angle(phi2 + i as f64 * step) * r2 }).collect();
    Some(Case { sk, edits: vec![(diameter, 2.0 * r2), (angle, phi2)], checks })
}

/// A point mirrored across a line: the line from a fixed point at a line angle with a length,
/// `p` at two distances from its ends (one side), `q` its mirror image. The angle turns by up
/// to ±115°, the dimensions scale by `lo..hi` (only edits where the triangle still exists).
fn mirrored(rnd: &mut Lcg, lo: f64, hi: f64) -> Option<Case> {
    let theta = rnd.range(-3.1, 3.1);
    let len = rnd.range(30.0, 60.0);
    let (u, a) = (Vec2::from_angle(theta), v(rnd.range(-30.0, 30.0), rnd.range(-30.0, 30.0)));
    let (t, h) = (rnd.range(0.2, 0.8) * len, rnd.range(5.0, 20.0) * rnd.sign());
    let (p, q) = (a + u * t + u.perp() * h, a + u * t - u.perp() * h);
    let mut sk = Sketch::new();
    let axis = sk.add_line(a, a + u * len, None, None, None).unwrap();
    let (ia, ib) = sk.curves[axis].kind.ends().unwrap();
    let (ip, iq) = (sk.add_point(p, None).unwrap(), sk.add_point(q, None).unwrap());
    push(&mut sk, ConstraintKind::Fix { p: ia });
    let angle = push(&mut sk, ConstraintKind::LineAngle { l: axis, value: theta });
    let length = push(&mut sk, ConstraintKind::Length { l: axis, value: len });
    let (d1, d2) = (p.dist(a), p.dist(a + u * len));
    let k1 = push(&mut sk, ConstraintKind::Distance { p: ia, q: ip, value: d1 });
    let k2 = push(&mut sk, ConstraintKind::Distance { p: ib, q: ip, value: d2 });
    push(&mut sk, ConstraintKind::Symmetric { p: ip, q: iq, l: axis });
    let theta2 = theta + rnd.range(-2.0, 2.0);
    let mut s = || rnd.range(lo, hi);
    let (len2, e1, e2) = (len * s(), d1 * s(), d2 * s());
    if e1 + e2 < len2 * 1.02 || (e1 - e2).abs() > len2 * 0.98 {
        return None;
    }
    let checks =
        vec![Check::Side { p: ip, a: ia, b: ib }, Check::Side { p: iq, a: ia, b: ib }, Check::Toward { a: ia, b: ib, d: Vec2::from_angle(theta2) }];
    Some(Case { sk, edits: vec![(angle, theta2), (length, len2), (k1, e1), (k2, e2)], checks })
}

/// Two collinear lines: L1 from a fixed point at a line angle with a length, L2 collinear with
/// it, its start a distance past L1's end, with a length. The angle turns by up to ±115°, the
/// dimensions scale by `lo..hi`; both lines and the gap keep their direction.
fn collinear(rnd: &mut Lcg, lo: f64, hi: f64) -> Option<Case> {
    let theta = rnd.range(-3.1, 3.1);
    let (a, g, b) = (rnd.range(20.0, 40.0), rnd.range(5.0, 30.0), rnd.range(20.0, 40.0));
    let (u, o) = (Vec2::from_angle(theta), v(rnd.range(-30.0, 30.0), rnd.range(-30.0, 30.0)));
    let mut sk = Sketch::new();
    let l1 = sk.add_line(o, o + u * a, None, None, None).unwrap();
    let l2 = sk.add_line(o + u * (a + g), o + u * (a + g + b), None, None, None).unwrap();
    let ((i0, i1), (i2, i3)) = (sk.curves[l1].kind.ends().unwrap(), sk.curves[l2].kind.ends().unwrap());
    push(&mut sk, ConstraintKind::Fix { p: i0 });
    let angle = push(&mut sk, ConstraintKind::LineAngle { l: l1, value: theta });
    let len1 = push(&mut sk, ConstraintKind::Length { l: l1, value: a });
    push(&mut sk, ConstraintKind::Collinear { a: l1, b: l2 });
    let gap = push(&mut sk, ConstraintKind::Distance { p: i1, q: i2, value: g });
    let len2 = push(&mut sk, ConstraintKind::Length { l: l2, value: b });
    let theta2 = theta + rnd.range(-2.0, 2.0);
    let u2 = Vec2::from_angle(theta2);
    let mut s = || rnd.range(lo, hi);
    let edits = vec![(angle, theta2), (len1, a * s()), (gap, g * s()), (len2, b * s())];
    let checks = vec![Check::Toward { a: i0, b: i1, d: u2 }, Check::Toward { a: i1, b: i2, d: u2 }, Check::Toward { a: i2, b: i3, d: u2 }];
    Some(Case { sk, edits, checks })
}

/// Points at a distance from a line: the line from a fixed point at a line angle with a
/// length; `p` at a distance from it (left) and from its start, `r` at half a linear diameter
/// from it (right) and at a distance from its end. The angle turns by up to ±115°, the
/// dimensions scale by `lo..hi` (only edits where both points still exist).
fn line_distance(rnd: &mut Lcg, lo: f64, hi: f64) -> Option<Case> {
    let theta = rnd.range(-3.1, 3.1);
    let len = rnd.range(30.0, 60.0);
    let (u, o) = (Vec2::from_angle(theta), v(rnd.range(-30.0, 30.0), rnd.range(-30.0, 30.0)));
    let n = u.perp();
    let (h1, h2) = (rnd.range(5.0, 20.0), rnd.range(5.0, 20.0));
    let p = o + u * rnd.range(5.0, 25.0) + n * h1;
    let r = o + u * (len - rnd.range(5.0, 25.0)) - n * h2;
    let mut sk = Sketch::new();
    let l = sk.add_line(o, o + u * len, None, None, None).unwrap();
    let (ia, ib) = sk.curves[l].kind.ends().unwrap();
    let (ip, ir) = (sk.add_point(p, None).unwrap(), sk.add_point(r, None).unwrap());
    push(&mut sk, ConstraintKind::Fix { p: ia });
    let angle = push(&mut sk, ConstraintKind::LineAngle { l, value: theta });
    let length = push(&mut sk, ConstraintKind::Length { l, value: len });
    let kh1 = push(&mut sk, ConstraintKind::PointLineDistance { p: ip, l, value: h1 });
    let (d1, d2) = (p.dist(o), r.dist(o + u * len));
    let kd1 = push(&mut sk, ConstraintKind::Distance { p: ia, q: ip, value: d1 });
    let kh2 = push(&mut sk, ConstraintKind::LinearDiameter { p: ir, l, value: 2.0 * h2 });
    let kd2 = push(&mut sk, ConstraintKind::Distance { p: ib, q: ir, value: d2 });
    let theta2 = theta + rnd.range(-2.0, 2.0);
    let mut s = || rnd.range(lo, hi);
    let (len2, g1, e1, g2, e2) = (len * s(), h1 * s(), d1 * s(), h2 * s(), d2 * s());
    if e1 < g1 * 1.1 || e2 < g2 * 1.1 {
        return None;
    }
    let edits = vec![(angle, theta2), (length, len2), (kh1, g1), (kd1, e1), (kh2, 2.0 * g2), (kd2, e2)];
    let checks = vec![
        Check::Side { p: ip, a: ia, b: ib },
        Check::Side { p: ir, a: ia, b: ib },
        Check::Along { p: ip, a: ia, b: ib },
        Check::Along { p: ir, a: ib, b: ia },
        Check::Toward { a: ia, b: ib, d: Vec2::from_angle(theta2) },
    ];
    Some(Case { sk, edits, checks })
}

/// Equal lines: L1 from a fixed point at a line angle, its end at a distance (no length
/// dimension), L2 from L1's end at an angle β from it and equal to it. The angle turns by up
/// to ±115°, the distance scales by `lo..hi`, β within 15°–165° either way.
fn equal_lines(rnd: &mut Lcg, lo: f64, hi: f64) -> Option<Case> {
    let theta = rnd.range(-3.1, 3.1);
    let a = rnd.range(20.0, 40.0);
    let beta = rnd.range(30f64.to_radians(), 150f64.to_radians()) * rnd.sign();
    let o = v(rnd.range(-30.0, 30.0), rnd.range(-30.0, 30.0));
    let b = o + Vec2::from_angle(theta) * a;
    let mut sk = Sketch::new();
    let l1 = sk.add_line(o, b, None, None, None).unwrap();
    let (i0, i1) = sk.curves[l1].kind.ends().unwrap();
    let l2 = sk.add_line(b, b + Vec2::from_angle(theta + beta) * a, Some(i1), None, None).unwrap();
    let (_, i2) = sk.curves[l2].kind.ends().unwrap();
    push(&mut sk, ConstraintKind::Fix { p: i0 });
    let angle = push(&mut sk, ConstraintKind::LineAngle { l: l1, value: theta });
    let dist = push(&mut sk, ConstraintKind::Distance { p: i0, q: i1, value: a });
    let bend = push(&mut sk, ConstraintKind::Angle { a: l1, b: l2, value: beta, flip: false });
    push(&mut sk, ConstraintKind::Equal { a: l1, b: l2 });
    let theta2 = theta + rnd.range(-2.0, 2.0);
    let beta2 = beta.signum() * (beta.abs() * rnd.range(lo, hi)).clamp(15f64.to_radians(), 165f64.to_radians());
    let edits = vec![(angle, theta2), (dist, a * rnd.range(lo, hi)), (bend, beta2)];
    let checks = vec![
        Check::Toward { a: i0, b: i1, d: Vec2::from_angle(theta2) },
        Check::Toward { a: i1, b: i2, d: Vec2::from_angle(theta2 + beta2) },
        Check::Side { p: i2, a: i0, b: i1 },
    ];
    Some(Case { sk, edits, checks })
}

/// Points on an arc of unknown radius: the arc's centre fixed, its start on a line from the
/// centre at a line angle and a distance (no radius dimension), its end and a point on it on
/// lines from the centre at angles σ and τ from the first. The angle turns by up to ±115°, the
/// distance scales by `lo..hi`, σ within 20°–170° (τ keeps its share of σ).
fn on_arc(rnd: &mut Lcg, lo: f64, hi: f64) -> Option<Case> {
    let phi = rnd.range(-3.1, 3.1);
    let r = rnd.range(15.0, 40.0);
    let sigma = rnd.range(40f64.to_radians(), 150f64.to_radians());
    let share = rnd.range(0.2, 0.8);
    let o = v(rnd.range(-30.0, 30.0), rnd.range(-30.0, 30.0));
    let at = |a: f64| o + Vec2::from_angle(a) * r;
    let mut sk = Sketch::new();
    let arc = sk.add_arc(o, at(phi), at(phi + sigma), [None, None, None], None).unwrap();
    let CurveKind::Arc { c, a: s0, b: e0 } = sk.curves[arc].kind else { panic!() };
    let p = sk.add_point(at(phi + share * sigma), None).unwrap();
    let (ls, le, lp) = (sk.add_line_pts(c, s0, None).unwrap(), sk.add_line_pts(c, e0, None).unwrap(), sk.add_line_pts(c, p, None).unwrap());
    push(&mut sk, ConstraintKind::Fix { p: c });
    let angle = push(&mut sk, ConstraintKind::LineAngle { l: ls, value: phi });
    let dist = push(&mut sk, ConstraintKind::Distance { p: c, q: s0, value: r });
    let ke = push(&mut sk, ConstraintKind::Angle { a: ls, b: le, value: sigma, flip: false });
    let kp = push(&mut sk, ConstraintKind::Angle { a: ls, b: lp, value: share * sigma, flip: false });
    push(&mut sk, ConstraintKind::PointOnCurve { p, c: arc });
    let phi2 = phi + rnd.range(-2.0, 2.0);
    let sigma2 = (sigma * rnd.range(lo, hi)).clamp(20f64.to_radians(), 170f64.to_radians());
    let edits = vec![(angle, phi2), (dist, r * rnd.range(lo, hi)), (ke, sigma2), (kp, share * sigma2)];
    let checks = vec![
        Check::Sweep { curve: arc, lo: 0.0, hi: std::f64::consts::PI },
        Check::Toward { a: c, b: e0, d: Vec2::from_angle(phi2 + sigma2) },
        Check::Toward { a: c, b: p, d: Vec2::from_angle(phi2 + share * sigma2) },
    ];
    Some(Case { sk, edits, checks })
}

type Family = fn(&mut Lcg, f64, f64) -> Option<Case>;

fn families() -> Vec<(&'static str, Family)> {
    vec![
        ("filleted rectangle", filleted_rectangle),
        ("slot (centre distance)", |r, lo, hi| slot(r, lo, hi, false)),
        ("slot (overall length)", |r, lo, hi| slot(r, lo, hi, true)),
        ("tangent chain", tangent_chain),
        ("axis-aligned chain", axis_chain),
        ("bolt circle", bolt_circle),
        ("mirrored point", mirrored),
        ("collinear lines", collinear),
        ("distance from a line", line_distance),
        ("equal lines", equal_lines),
        ("points on an arc", on_arc),
    ]
}

const EDITS: usize = 200;

/// `n` feasible members of `family` at scales `lo..hi`; fails the test when the family yields
/// fewer than one in fifty.
fn sample(rnd: &mut Lcg, family: Family, lo: f64, hi: f64, n: usize) -> Vec<Case> {
    let mut out = Vec::new();
    for _ in 0..n.saturating_mul(50) {
        if out.len() == n {
            break;
        }
        out.extend(family(rnd, lo, hi));
    }
    assert_eq!(out.len(), n, "the family yields too few feasible members at {lo}–{hi}×");
    out
}

/// (failed, broke a check) over `EDITS` kept edits of `family` at scales `lo..hi`.
fn tally(family: Family, lo: f64, hi: f64, on: bool) -> (usize, usize) {
    let mut rnd = Lcg(7);
    let (mut failed, mut broke) = (0, 0);
    for case in sample(&mut rnd, family, lo, hi, EDITS) {
        let (f, b) = run(&case, on);
        failed += f as usize;
        broke += b as usize;
    }
    (failed, broke)
}

fn table(on: bool) -> Vec<(&'static str, f64, f64, usize, usize)> {
    let mut out = Vec::new();
    for (name, family) in families() {
        for (lo, hi) in [(0.8, 1.25), (0.5, 2.0), (0.3, 3.0)] {
            let (failed, broke) = tally(family, lo, hi, on);
            eprintln!("seed {} {name:<24} {lo}–{hi}×: {failed} failed, {broke}/{EDITS} lost a side", if on { "on " } else { "off" });
            out.push((name, lo, hi, failed, broke));
        }
    }
    out
}

#[test]
fn rounds_and_tangents_keep_their_sides_on_dimension_edits() {
    let rows = table(true);
    assert!(rows.iter().all(|r| r.3 == 0 && r.4 == 0), "{rows:?}");
}

/// The measurement the seed is judged against: without it, LM folds arcs and lines over.
#[test]
#[ignore = "measurement: run with --ignored --nocapture"]
fn rounds_and_tangents_without_the_seed() {
    table(false);
}

/// The solved members of every family are left exactly where they are.
#[test]
fn solved_members_stay_put() {
    let mut rnd = Lcg(3);
    for (name, family) in families() {
        for case in sample(&mut rnd, family, 1.0, 1.0, 10) {
            let mut sk = case.sk.clone();
            let rep = solve(&mut sk);
            assert!(rep.ok() && rep.dof == 0, "{name}: {rep:?}");
            for (p, q) in sk.points.iter().zip(&case.sk.points) {
                assert!(p.pos.dist(q.pos) < 1e-7, "{name}: {} moved {:?} → {:?}", p.id, q.pos, p.pos);
            }
        }
    }
}

/// The seed only moves the start: through the crate-wide seed-on/seed-off comparison (`solve`
/// in tests), the edits give the same status, degrees of freedom and determined flags.
#[test]
fn seed_differential_on_every_family() {
    let mut rnd = Lcg(5);
    for (_, family) in families() {
        for case in sample(&mut rnd, family, 0.5, 2.0, 15) {
            let mut sk = edited(&case);
            solve(&mut sk);
        }
    }
}

/// Every point of every family is placed by the seed (not left to LM), so edits solve in
/// few iterations.
#[test]
fn every_point_of_every_family_is_constructed() {
    let mut rnd = Lcg(9);
    for (name, family) in families() {
        for case in sample(&mut rnd, family, 0.5, 2.0, 10) {
            let sk = edited(&case);
            let s = seed_of(&sk);
            let used: Vec<PointIdx> = sk.curves.iter().flat_map(|c| c.kind.point_ids()).collect();
            for p in used {
                assert!(s.placed_by[p.0].is_some_and(Placement::anchors), "{name}: point {p} ({}) not placed: {s:?}", sk.points[p].id);
            }
        }
    }
}

/// The seed does not depend on the order of the constraints.
#[test]
fn seed_on_lines_and_rounds_is_independent_of_constraint_order() {
    let mut rnd = Lcg(13);
    for (name, family) in families() {
        let case = sample(&mut rnd, family, 0.5, 2.0, 1).remove(0);
        let mut sk = edited(&case);
        let forward = seed_of(&sk);
        sk.constraints.reverse();
        assert_eq!(seed_of(&sk), forward, "{name}");
        let n = sk.constraints.len();
        sk.constraints.rotate_left(n / 3);
        assert_eq!(seed_of(&sk), forward, "{name}");
    }
}

/// The canonical bracket with the third dimension on a point the rigid triangle carries
/// through a constraint the decomposition reads from the shared loci: the midpoint of P R, or
/// the centre of a circle tangent to P R at a known distance from Q. No propagation order
/// reaches P, Q, R; the decomposition grows the triangle by that point and places all four.
#[test]
fn decomposition_grows_clusters_through_lines_and_tangency() {
    for tangent in [false, true] {
        let truth = [v(0.0, 0.0), v(60.0, 0.0), v(10.0, 30.0), v(45.0, 35.0), v(25.0, 55.0)];
        let mut sk = Sketch::new();
        let ids: Vec<PointIdx> = truth
            .iter()
            .enumerate()
            .map(|(i, t)| sk.add_point(*t + if i < 2 { v(0.0, 0.0) } else { v(0.7 * i as f64, -0.4 * i as f64) }, None).unwrap())
            .collect();
        let [o1, o2, p, q, r] = [ids[0], ids[1], ids[2], ids[3], ids[4]];
        for x in [o1, o2] {
            push(&mut sk, ConstraintKind::Fix { p: x });
        }
        let pr = sk.add_line_pts(p, r, None).unwrap();
        for (a, b, ta, tb) in [(p, q, 2, 3), (q, r, 3, 4), (o1, p, 0, 2), (o1, r, 0, 4)] {
            push(&mut sk, ConstraintKind::Distance { p: a, q: b, value: truth[ta].dist(truth[tb]) });
        }
        push(&mut sk, ConstraintKind::Distance { p, q: r, value: truth[2].dist(truth[4]) });
        let m = if tangent {
            // Circle of radius 6 tangent to P R on Q's side, 25 from Q.
            let (a, b) = (truth[2], truth[4]);
            let d = (b - a).normalized().unwrap();
            let n = d.perp() * (d.cross(truth[3] - a).signum());
            let foot = a + d * 6.0 + n * 6.0;
            let k_at = {
                // On the offset line, 25 from Q.
                let rel = foot - truth[3];
                let t = -rel.dot(d) + (625.0 - rel.cross(d).powi(2)).sqrt();
                foot + d * t
            };
            let circle = sk.add_circle(k_at + v(0.5, 0.3), 6.0, None, None).unwrap();
            let CurveKind::Circle { c, .. } = sk.curves[circle].kind else { panic!() };
            push(&mut sk, ConstraintKind::Radius { c: circle, value: 6.0 });
            push(&mut sk, ConstraintKind::Tangent { a: pr, b: circle });
            push(&mut sk, ConstraintKind::Distance { p: q, q: c, value: 25.0 });
            push(&mut sk, ConstraintKind::Distance { p: o2, q: c, value: truth[1].dist(k_at) });
            c
        } else {
            let m = sk.add_point((truth[2] + truth[4]) * 0.5 + v(0.4, 0.2), None).unwrap();
            push(&mut sk, ConstraintKind::Midpoint { p: m, l: pr });
            push(&mut sk, ConstraintKind::Distance { p: o2, q: m, value: truth[1].dist((truth[2] + truth[4]) * 0.5) });
            m
        };
        let out = seed_of(&sk);
        for x in [p, q, r, m] {
            assert_eq!(out.placed_by[x.0], Some(Placement::Cluster), "tangent {tangent}, point {x}: {out:?}");
        }
        let rep = solve(&mut sk);
        assert!(rep.ok() && rep.dof == 0 && rep.iterations == 0, "tangent {tangent}: {rep:?}");
        for (i, t) in [(p, 2), (q, 3), (r, 4)] {
            assert!(sk.points[i].pos.dist(truth[t]) < 1e-9, "tangent {tangent}: {:?}", sk.points[i].pos);
        }
    }
}

/// A circle's radius known from a dimension is part of the seed: the circle's radius starts
/// there.
#[test]
fn a_dimensioned_radius_is_seeded() {
    let mut rnd = Lcg(17);
    let case = bolt_circle(&mut rnd, 2.0, 2.0).unwrap();
    let sk = edited(&case);
    let s = seed_of(&sk);
    let ConstraintKind::Diameter { c, value } = sk.constraints[1].kind else { panic!() };
    assert_eq!(s.radius[c.0], Some(value / 2.0));
}

/// Hostile curve references and values in the constraints the new loci read neither panic
/// nor change the report (every solve runs through the seed-on/seed-off comparison).
#[test]
fn hostile_lines_and_rounds_neither_panic_nor_change_the_report() {
    let base = || tangent_chain(&mut Lcg(21), 0.5, 2.0).unwrap();
    let raw = |sk: &mut Sketch, kind: ConstraintKind| {
        sk.constraints.push(Constraint { id: format!("h{}", sk.constraints.len()), kind, param: None, driven: false, text: None })
    };
    let cases: Vec<Box<dyn Fn(&mut Sketch)>> = vec![
        Box::new(|sk| raw(sk, ConstraintKind::Midpoint { p: PointIdx(1), l: CurveIdx(999) })),
        Box::new(|sk| raw(sk, ConstraintKind::Midpoint { p: PointIdx(usize::MAX), l: CurveIdx(0) })),
        Box::new(|sk| raw(sk, ConstraintKind::Symmetric { p: PointIdx(1), q: PointIdx(2), l: CurveIdx(usize::MAX) })),
        Box::new(|sk| raw(sk, ConstraintKind::Tangent { a: CurveIdx(0), b: CurveIdx(0) })),
        Box::new(|sk| raw(sk, ConstraintKind::Tangent { a: CurveIdx(2), b: CurveIdx(77) })),
        Box::new(|sk| raw(sk, ConstraintKind::Radius { c: CurveIdx(2), value: f64::NAN })),
        Box::new(|sk| raw(sk, ConstraintKind::Radius { c: CurveIdx(2), value: -3.0 })),
        Box::new(|sk| raw(sk, ConstraintKind::Diameter { c: CurveIdx(0), value: 1e300 })),
        Box::new(|sk| raw(sk, ConstraintKind::PointOnCurve { p: PointIdx(1), c: CurveIdx(2) })),
        Box::new(|sk| raw(sk, ConstraintKind::PointOnCurve { p: PointIdx(1), c: CurveIdx(usize::MAX) })),
        Box::new(|sk| raw(sk, ConstraintKind::PointLineDistance { p: PointIdx(1), l: CurveIdx(1), value: f64::INFINITY })),
        Box::new(|sk| raw(sk, ConstraintKind::LineAngle { l: CurveIdx(0), value: f64::NAN })),
        Box::new(|sk| raw(sk, ConstraintKind::Angle { a: CurveIdx(0), b: CurveIdx(1), value: 1e300, flip: true })),
        Box::new(|sk| raw(sk, ConstraintKind::Parallel { a: CurveIdx(0), b: CurveIdx(0) })),
        Box::new(|sk| raw(sk, ConstraintKind::Perpendicular { a: CurveIdx(0), b: CurveIdx(2) })),
        Box::new(|sk| raw(sk, ConstraintKind::Equal { a: CurveIdx(0), b: CurveIdx(2) })),
        Box::new(|sk| raw(sk, ConstraintKind::Concentric { a: CurveIdx(2), b: CurveIdx(2) })),
        Box::new(|sk| raw(sk, ConstraintKind::SlotOverallLength { a: CurveIdx(2), b: CurveIdx(2), value: 5.0 })),
        Box::new(|sk| raw(sk, ConstraintKind::LinearDiameter { p: PointIdx(3), l: CurveIdx(0), value: -1.0 })),
        Box::new(|sk| sk.points[PointIdx(2)].pos = v(f64::NAN, 0.0)),
        Box::new(|sk| sk.points[PointIdx(3)].pos = v(1e300, -1e300)),
    ];
    for (i, case) in cases.iter().enumerate() {
        let mut c = base();
        case(&mut c.sk);
        let sk = edited(&c);
        let s = seed(&sk, &given(&sk), Some(&[v(f64::NAN, 0.0)]), false, &points_on_curves(&sk), &mut Budget::full());
        assert!(s.pos.iter().flatten().all(|p| p.is_finite()), "case {i}: {s:?}");
        assert!(s.radius.iter().flatten().all(|r| r.is_finite() && *r > 0.0), "case {i}: {s:?}");
        let mut t = c.sk.clone();
        solve(&mut t);
    }
}

/// An axis-aligned line angle turned half a turn (0° → 180°) with a line at an angle to it and
/// a tangent arc: the explicit angle decides the direction, not the reference's axis sign, so
/// the seed builds the chain forward along the new angle and nothing folds.
#[test]
fn an_axis_aligned_line_angle_turned_half_a_turn_is_built_along_it() {
    let case = chain(&mut Lcg(31), 1.0, 1.0, 0.0, std::f64::consts::PI).unwrap();
    let sk = edited(&case);
    let (i0, i1) = sk.curves[CurveIdx(0)].kind.ends().unwrap();
    let s = seed_of(&sk);
    let (p0, e1) = (sk.points[i0].pos, s.pos[i1.0].unwrap_or_else(|| panic!("L1's end unplaced: {s:?}")));
    assert!((e1 - p0).dot(Vec2::X) < 0.0, "L1 built along the reference's axis, not its angle: {p0:?} → {e1:?}");
    assert_eq!(run(&case, true), (false, false));
}

/// Horizontal and vertical lines tied by a contradictory parallel (or a perpendicular between
/// two horizontal lines) are still read as horizontal and vertical one by one: their rows and
/// columns are seeded.
#[test]
fn contradictory_levels_still_read_each_line_on_its_own() {
    for perpendicular in [false, true] {
        let mut sk = Sketch::new();
        let l1 = sk.add_line(v(0.0, 0.0), v(40.0, 0.5), None, None, None).unwrap();
        let (a, b) = sk.curves[l1].kind.ends().unwrap();
        let l2 = if perpendicular {
            sk.add_line(v(0.0, 30.0), v(30.0, 30.4), None, None, None).unwrap()
        } else {
            sk.add_line(v(0.0, 30.0), v(0.3, 60.0), None, None, None).unwrap()
        };
        let (c, d) = sk.curves[l2].kind.ends().unwrap();
        for p in [a, c] {
            push(&mut sk, ConstraintKind::Fix { p });
        }
        push(&mut sk, ConstraintKind::Horizontal { l: l1 });
        push(&mut sk, ConstraintKind::Length { l: l1, value: 40.0 });
        push(&mut sk, if perpendicular { ConstraintKind::Horizontal { l: l2 } } else { ConstraintKind::Vertical { l: l2 } });
        push(&mut sk, ConstraintKind::Length { l: l2, value: 30.0 });
        push(&mut sk, if perpendicular { ConstraintKind::Perpendicular { a: l1, b: l2 } } else { ConstraintKind::Parallel { a: l1, b: l2 } });
        let s = seed_of(&sk);
        assert!(s.pos[b.0].is_some_and(|p| p.dist(v(40.0, 0.0)) < 1e-9), "perpendicular {perpendicular}: {s:?}");
        let want = if perpendicular { v(30.0, 30.0) } else { v(0.0, 60.0) };
        assert!(s.pos[d.0].is_some_and(|p| p.dist(want) < 1e-9), "perpendicular {perpendicular}: {s:?}");
    }
}

/// A dimensioned circle's radius is seeded also when nothing is given to build on.
#[test]
fn a_dimensioned_radius_is_seeded_with_nothing_given() {
    let mut sk = Sketch::new();
    let circle = sk.add_circle(v(10.0, 5.0), 10.0, None, None).unwrap();
    push(&mut sk, ConstraintKind::Diameter { c: circle, value: 30.0 });
    let s = seed(&sk, &vec![false; sk.points.len()], None, true, &[], &mut Budget::full());
    assert_eq!(s.radius[circle.0], Some(15.0));
}

/// A line's end constrained onto a circle is the tangency's touch point only through the
/// solve's point-on-curve index: with it the circle's centre is placed on the normal there,
/// without it only a touching pair of loci is left.
#[test]
fn the_touch_point_comes_from_the_point_on_curve_index() {
    let mut sk = Sketch::new();
    let l = sk.add_line(v(0.0, 0.0), v(40.0, 12.0), None, None, None).unwrap();
    let (a, b) = sk.curves[l].kind.ends().unwrap();
    let d = (v(40.0, 12.0) - v(0.0, 0.0)).normalized().unwrap();
    let circle = sk.add_circle(v(40.0, 12.0) + d.perp() * 8.5, 8.0, None, None).unwrap();
    let CurveKind::Circle { c, .. } = sk.curves[circle].kind else { panic!() };
    push(&mut sk, ConstraintKind::Fix { p: a });
    push(&mut sk, ConstraintKind::LineAngle { l, value: 12f64.atan2(40.0) });
    push(&mut sk, ConstraintKind::Length { l, value: v(40.0, 12.0).len() });
    push(&mut sk, ConstraintKind::Radius { c: circle, value: 8.0 });
    push(&mut sk, ConstraintKind::Tangent { a: l, b: circle });
    push(&mut sk, ConstraintKind::PointOnCurve { p: b, c: circle });
    let on_curve = crate::solver::points_on_curves(&sk);
    let with = seed(&sk, &given(&sk), None, true, &on_curve, &mut Budget::full());
    assert!(with.placed_by[c.0].is_some_and(Placement::anchors), "{with:?}");
    assert!(with.pos[c.0].unwrap().dist(v(40.0, 12.0) + d.perp() * 8.0) < 1e-9, "{with:?}");
    let without = seed(&sk, &given(&sk), None, true, &[], &mut Budget::full());
    assert!(!without.placed_by[c.0].is_some_and(Placement::anchors), "{without:?}");
}

/// Many pair clusters each grown by one shared point: a fixed point on every line from another
/// fixed point to a point at a distance from it. Each cluster grows the shared point and then
/// merges with the ground, and every growth requeues the clusters holding that point, so the
/// steps outnumber any multiple of the starting clusters: the merge loop is bounded by its
/// work budget, not by a count of steps, and places every point.
#[test]
fn long_growth_chains_are_not_starved() {
    let (o, u) = (v(5.0, -3.0), Vec2::from_angle(0.4));
    let mut sk = Sketch::new();
    let p = sk.add_point(o, None).unwrap();
    let z = sk.add_point(o + u * 12.0, None).unwrap();
    for x in [p, z] {
        push(&mut sk, ConstraintKind::Fix { p: x });
    }
    push(&mut sk, ConstraintKind::Distance { p, q: z, value: 12.0 });
    let qs: Vec<PointIdx> = (0..40)
        .map(|j| {
            let d = 20.0 + 3.0 * j as f64;
            let q = sk.add_point(o + u * d + v(0.5, -0.4), None).unwrap();
            let l = sk.add_line_pts(p, q, None).unwrap();
            push(&mut sk, ConstraintKind::Distance { p, q, value: d });
            push(&mut sk, ConstraintKind::PointOnCurve { p: z, c: l });
            q
        })
        .collect();
    let s = seed_of(&sk);
    for (j, q) in qs.iter().enumerate() {
        assert_eq!(s.placed_by[q.0], Some(Placement::Cluster), "point {j}: {s:?}");
        assert!(s.pos[q.0].is_some_and(|at| at.dist(o + u * (20.0 + 3.0 * j as f64)) < 1e-9), "point {j}: {s:?}");
    }
}
