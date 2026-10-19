use solidcraft_geom::{Loop2, Plane, Region2, Seg2, Vec2, Vec3};

use crate::Body;

const X0: f64 = 41.0 + 1.0 / 6.0;
/// The arc's axis.
const CX: f64 = X0 + 21.0;

/// The arc part: a 41⅙ × 21 rectangle and, beside it, the corner a circle of radius 21 about
/// (CX, −21) leaves against the top line it touches (its arc square to the front, a knife edge
/// at the tip), extruded 10.
fn arc_part() -> Body {
    scaled_arc_part(1.0)
}

/// [`arc_part`] scaled by `k` about the origin.
fn scaled_arc_part(k: f64) -> Body {
    let p = |x: f64, y: f64| Vec2::new(x * k, y * k);
    let segs = vec![
        Seg2::Line { a: p(0.0, 0.0), b: p(0.0, -21.0) },
        Seg2::Line { a: p(0.0, -21.0), b: p(X0, -21.0) },
        Seg2::Arc { center: p(CX, -21.0), radius: 21.0 * k, start: std::f64::consts::PI, sweep: -std::f64::consts::FRAC_PI_2 },
        Seg2::Line { a: p(CX, 0.0), b: p(0.0, 0.0) },
    ];
    crate::extrude(&Plane::XY, &[Region2 { outer: Loop2 { segs }, holes: vec![] }], 0.0, 10.0 * k).unwrap().pop().unwrap()
}

/// Points on the edges at the top: the long front edge, the arc, the back edge; the uprights
/// between the front and the arc, and at the tip (a knife edge); the left top edge and the
/// left front upright.
const LONG: Vec3 = Vec3::new(20.0, -21.0, 10.0);
fn arc_top() -> Vec3 {
    let a = std::f64::consts::FRAC_PI_4;
    Vec3::new(CX - 21.0 * a.cos(), -21.0 + 21.0 * a.sin(), 10.0)
}
const BACK: Vec3 = Vec3::new(30.0, 0.0, 10.0);
const UPRIGHT: Vec3 = Vec3::new(X0, -21.0, 5.0);
const TIP: Vec3 = Vec3::new(CX, 0.0, 5.0);
const LEFT: Vec3 = Vec3::new(0.0, -10.0, 10.0);
const LEFT_FRONT: Vec3 = Vec3::new(0.0, -21.0, 5.0);

/// The volume of the body's display mesh, asserting it is closed (the legacy mesher can misread
/// a zero-angle face corner such as the tip's).
fn volume(b: &Body) -> f64 {
    crate::issue_851_tests::planned_volume(b)
}

/// What a blend (radius or distance 2) of the edges at `pts` takes off the part.
fn removed(round: bool, pts: &[Vec3]) -> f64 {
    let body = arc_part();
    let b = if round { crate::fillet(&body, pts, 2.0) } else { crate::chamfer(&body, pts, 2.0) }.unwrap();
    volume(&body) - volume(&b)
}

/// The part's material at `p`.
fn in_part(p: Vec3) -> bool {
    (0.0..=CX).contains(&p.x) && (-21.0..=0.0).contains(&p.y) && (0.0..=10.0).contains(&p.z) && (p.x <= X0 || (p.x - CX).hypot(p.y + 21.0) >= 21.0)
}

/// Rounds of radius 2 about the top front corner of the arc: what each edge's round alone
/// takes off there (between the edge and the ball, on the edge's side of its contacts).
fn off_long(p: Vec3) -> bool {
    in_part(p) && p.y < -19.0 && p.z > 8.0 && (p.y + 19.0).hypot(p.z - 8.0) > 2.0
}
/// The upright's ball, 2 from the front and 23 from the arc's axis.
fn upright_centre() -> f64 {
    CX - (23.0f64 * 23.0 - 4.0).sqrt()
}
fn off_upright(p: Vec3) -> bool {
    let (dx, dy) = (p.x - upright_centre(), p.y + 19.0);
    let a = dy.atan2(dx);
    in_part(p) && a >= -std::f64::consts::FRAC_PI_2 && a <= (-2.0f64).atan2(CX - upright_centre()) && dx.hypot(dy) > 2.0
}
fn off_arc(p: Vec3) -> bool {
    let rho = (p.x - CX).hypot(p.y + 21.0);
    in_part(p) && p.y >= -21.0 && rho < 23.0 && p.z > 8.0 && (rho - 23.0).hypot(p.z - 8.0) > 2.0
}
/// Kept when all three are rounded and the corner with them: within 2 of a ball centre 2 from
/// all three faces (z ≤ 8, y ≥ −19, 23 from the arc's axis), the nearest on a face, an edge
/// or the corner of that region.
fn kept_by_balls(p: Vec3) -> bool {
    let c = Vec3::new(upright_centre(), -19.0, 8.0);
    let inside = |q: Vec3| q.z <= 8.0 + 1e-12 && q.y >= -19.0 - 1e-12 && (q.x - CX).hypot(q.y + 21.0) >= 23.0 - 1e-12;
    let mut near =
        vec![c, Vec3::new(p.x.min(c.x), -19.0, 8.0), Vec3::new(c.x, -19.0, p.z.min(8.0)), Vec3::new(p.x, p.y, 8.0), Vec3::new(p.x, -19.0, p.z)];
    let (dx, dy) = (p.x - CX, p.y + 21.0);
    let d = dx.hypot(dy);
    if d > 1e-9 {
        let (ux, uy) = (dx / d, dy / d);
        near.push(Vec3::new(CX + ux * 23.0, -21.0 + uy * 23.0, p.z));
        near.push(Vec3::new(CX + ux * 23.0, -21.0 + uy * 23.0, 8.0));
    }
    in_part(p) && (inside(p) || near.iter().filter(|q| inside(**q)).any(|q| q.dist(p) <= 2.0))
}

/// Volume of the points of the box about the corner where `f` holds (midpoint rule).
fn corner_volume(f: impl Fn(Vec3) -> bool) -> f64 {
    let h = 0.02;
    let (lo, hi) = (Vec3::new(38.0, -21.0, 7.5), Vec3::new(42.5, -17.5, 10.0));
    let n = |a: f64, b: f64| ((b - a) / h).round() as usize;
    let mut count = 0usize;
    for i in 0..n(lo.x, hi.x) {
        for j in 0..n(lo.y, hi.y) {
            for k in 0..n(lo.z, hi.z) {
                if f(lo + Vec3::new((i as f64 + 0.5) * h, (j as f64 + 0.5) * h, (k as f64 + 0.5) * h)) {
                    count += 1;
                }
            }
        }
    }
    count as f64 * h * h * h
}

/// Two or three of the edges at the arc's top front corner rounded together: the strips cut
/// each other in a crease (the long one running on past it to end in the arc wall, as far as
/// its contact reaches along the arc's edge), and with all three a piece of the ball touching
/// the three faces fills the corner. What they take off: each round alone, less what each alone
/// takes in a box about the corner, plus what they take there together (rolling-ball rounds
/// trim each other where each one's corner region ends; with three, what no ball inside the
/// part reaches).
#[test]
fn rounds_meeting_at_a_corner_of_an_arc_wall() {
    let offs: [(Vec3, fn(Vec3) -> bool); 3] = [(LONG, off_long), (arc_top(), off_arc), (UPRIGHT, off_upright)];
    let alone: Vec<f64> = offs.iter().map(|(p, f)| removed(true, &[*p]) - corner_volume(f)).collect();
    for which in [vec![0, 2], vec![0, 1], vec![1, 2], vec![0, 1, 2]] {
        let pts: Vec<Vec3> = which.iter().map(|i| offs[*i].0).collect();
        let fs: Vec<fn(Vec3) -> bool> = which.iter().map(|i| offs[*i].1).collect();
        let together = if which.len() == 3 { corner_volume(|p| in_part(p) && !kept_by_balls(p)) } else { corner_volume(|p| fs.iter().any(|f| f(p))) };
        let want = which.iter().map(|i| alone[*i]).sum::<f64>() + together;
        let got = removed(true, &pts);
        assert!((got - want).abs() < 0.03, "{which:?}: removed {got} vs {want}");
    }
}

/// The ball at the knife-edge tip: 2 below the top line and 23 from the arc's axis.
fn tip_ball() -> Vec2 {
    Vec2::new(CX - (23.0f64 * 23.0 - 19.0 * 19.0).sqrt(), -2.0)
}

/// Integral over the cusp between the arc and the top line (midpoint rule, the last cell of
/// each column cut at the top line), from `x0` to the tip.
fn over_cusp(x0: f64, f: impl Fn(f64, f64) -> f64) -> f64 {
    let (hx, hy) = (0.001, 0.0002);
    let mut sum = 0.0;
    let mut x = x0 + hx / 2.0;
    while x < CX {
        let lo = -21.0 + (441.0 - (x - CX) * (x - CX)).max(0.0).sqrt();
        let n = ((0.0 - lo) / hy).ceil().max(0.0) as usize;
        sum += (0..n)
            .map(|j| {
                let (y0, y1) = (lo + j as f64 * hy, (lo + (j + 1) as f64 * hy).min(0.0));
                f(x, (y0 + y1) * 0.5) * (y1 - y0)
            })
            .sum::<f64>()
            * hx;
        x += hx;
    }
    sum
}

/// Whether the tip's round takes off (x, y), all the way up: the cusp beyond the ball, on the
/// tip's side of its contacts.
fn off_tip(x: f64, y: f64) -> bool {
    let k = tip_ball();
    let a = (y - k.y).atan2(x - k.x);
    (x - k.x).hypot(y - k.y) > 2.0 && a >= (-21.0 - k.y).atan2(CX - k.x) && a <= std::f64::consts::FRAC_PI_2
}

/// The knife-edge tip (the arc and the top line meeting at no angle) rounds to the ball 2 from
/// both, 13 back from the tip; its chamfer cuts the cusp at the chord between the same contacts.
#[test]
fn a_knife_edge_tip_rounds_where_the_ball_fits() {
    let k = tip_ball();
    let round = over_cusp(k.x - 1.0, |x, y| if off_tip(x, y) { 1.0 } else { 0.0 }) * 10.0;
    let got = removed(true, &[TIP]);
    assert!((got - round).abs() < round * 1e-3, "round: removed {got} vs {round}");
    // The chord from the top line's contact to the arc's.
    let (a, b) = (Vec2::new(k.x, 0.0), k + (Vec2::new(CX, -21.0) - k) * (2.0 / 23.0));
    let beyond = |x: f64, y: f64| (b.x - a.x) * (y - a.y) - (b.y - a.y) * (x - a.x) > 0.0;
    let flat = over_cusp(k.x - 1.0, |x, y| if beyond(x, y) { 1.0 } else { 0.0 }) * 10.0;
    let got = removed(false, &[TIP]);
    assert!((got - flat).abs() < flat * 1e-3, "chamfer: removed {got} vs {flat}");
}

/// The tip's round with the arc's or the back edge's at the top, where all three meet: as at
/// the front corner, each alone less what both would take twice — over the tip's corner region,
/// the other's (above 8 + √(4 − d²), d across from its ball's path).
#[test]
fn rounds_meeting_at_a_knife_edge_tip() {
    let k = tip_ball();
    let tip = removed(true, &[TIP]);
    let arc_d = |x: f64, y: f64| 23.0 - (x - CX).hypot(y + 21.0);
    let back_d = |_: f64, y: f64| y + 2.0;
    for (edge, d) in [(arc_top(), &arc_d as &dyn Fn(f64, f64) -> f64), (BACK, &back_d)] {
        let both = over_cusp(k.x - 1.0, |x, y| {
            let d = d(x, y);
            if off_tip(x, y) && d > 0.0 && d < 2.0 { 2.0 - (4.0 - d * d).sqrt() } else { 0.0 }
        });
        let want = tip + removed(true, &[edge]) - both;
        let got = removed(true, &[edge, TIP]);
        assert!((got - want).abs() < want * 1e-3, "{edge:?}: removed {got} vs {want}");
    }
}

/// Chamfers of the long top edge and the arc's top edge meeting at the front corner: their
/// planes (the arc's a cone) cut each other. Each alone, less what both take: over the top's
/// corner, 10 − z above both 8 + d (d across from the front face, or from the arc toward its axis).
#[test]
fn chamfers_meeting_at_a_corner_of_an_arc_wall() {
    let h = 0.002;
    let mut both = 0.0;
    for i in 0..((44.0 - 36.0) / h) as usize {
        for j in 0..(3.0 / h) as usize {
            let (x, y) = (36.0 + (i as f64 + 0.5) * h, -21.0 + (j as f64 + 0.5) * h);
            let rho = (x - CX).hypot(y + 21.0);
            if in_part(Vec3::new(x, y, 9.99)) && y < -19.0 && rho < 23.0 {
                both += (2.0 - (y + 21.0).max(rho - 21.0)).max(0.0) * h * h;
            }
        }
    }
    let want = removed(false, &[LONG]) + removed(false, &[arc_top()]) - both;
    let got = removed(false, &[LONG, arc_top()]);
    assert!((got - want).abs() < want * 1e-3, "removed {got} vs {want}");
}

/// A fillet over an earlier one at the end of its edge, as a feature of its own: the new round
/// ends in the earlier one (running on past it across the next face when the earlier one's end
/// is shorter than the new round), taking off what the two take together.
#[test]
fn a_round_ending_in_an_earlier_round_matches_both_together() {
    for (first, then) in [
        (LONG, LEFT),
        (LONG, LEFT_FRONT),
        (LONG, arc_top()),
        (arc_top(), LONG),
        (LONG, UPRIGHT),
        (arc_top(), UPRIGHT),
        // At the knife-edge tip, where the first round's end takes in the tip's corner.
        (arc_top(), TIP),
        (BACK, TIP),
        (arc_top(), BACK),
    ] {
        one_then_the_other(first, then);
    }
}

/// The back edge rounded first, then the arc's top edge: the same solid as both together, or the
/// other order (the arc's round once left 0.05 mm³ more at the knife-edge tip).
#[test]
fn the_arc_rounded_after_the_back_edge_matches_both_together() {
    one_then_the_other(BACK, arc_top());
}

/// The edge at `first` rounded (2), then the one at `then` as a fillet of its own, against both
/// rounded together.
fn one_then_the_other(first: Vec3, then: Vec3) {
    let body = arc_part();
    let step = crate::fillet(&body, &[first], 2.0).unwrap();
    let after = crate::fillet(&step, &[then], 2.0).unwrap();
    let v = volume(&after);
    let together = volume(&crate::fillet(&body, &[first, then], 2.0).unwrap());
    // Within the display meshes' accuracy (two different B-reps, meshed apart).
    assert!((v - together).abs() < 5e-3, "{first:?} then {then:?}: {v} vs together {together}");
}

/// Edges rounded together come out the same in any order: a round made first doesn't carry the
/// next one on along its arc into edges nobody selected.
#[test]
fn rounds_together_in_any_order() {
    let body = arc_part();
    for (a, b) in [(UPRIGHT, LONG), (TIP, arc_top()), (TIP, BACK)] {
        let v = |x: &[Vec3]| volume(&crate::fillet(&body, x, 2.0).unwrap());
        let (ab, ba) = (v(&[a, b]), v(&[b, a]));
        assert!((ab - ba).abs() < 1e-3, "{a:?} + {b:?}: {ab} vs {ba}");
    }
}

/// The blends this part needs the corner build for (and the back edge, rolled into the tip) at
/// 0.01 and 100 times the part's size, the radius scaled with it: they take off what they take at
/// full size, scaled by the cube, so the build's tolerances and limits follow the model's size.
#[test]
fn corner_blends_hold_at_any_scale() {
    let cases: [(&str, Vec<Vec<Vec3>>); 6] = [
        ("tip and arc", vec![vec![TIP, arc_top()]]),
        ("arc and tip", vec![vec![arc_top(), TIP]]),
        ("back into the tip", vec![vec![BACK]]),
        ("arc, then tip", vec![vec![arc_top()], vec![TIP]]),
        ("back, then tip", vec![vec![BACK], vec![TIP]]),
        ("arc, then back", vec![vec![arc_top()], vec![BACK]]),
    ];
    // What a blend takes off the part scaled by `k`, over k³, through a display plan of matching
    // accuracy.
    let removed = |k: f64, steps: &[Vec<Vec3>]| {
        let volume = |b: &Body| b.display_plan(2e-4 * k).unwrap().evaluate().measure().volume;
        let body = scaled_arc_part(k);
        let mut b = body.clone();
        for pts in steps {
            let pts: Vec<Vec3> = pts.iter().map(|p| *p * k).collect();
            b = crate::fillet(&b, &pts, 2.0 * k).unwrap_or_else(|e| panic!("at {k}x: {steps:?}: {e}"));
            assert!(b.validity().is_empty(), "at {k}x: {steps:?}: {:?}", b.validity());
        }
        (volume(&body) - volume(&b)) / (k * k * k)
    };
    for (name, steps) in &cases {
        let full = removed(1.0, steps);
        for k in [0.01, 100.0] {
            let got = removed(k, steps);
            assert!((got - full).abs() < full * 1e-4, "{name} at {k}x: removed {got} (scaled back) vs {full}");
        }
    }
}

#[test]
fn the_corner_refused_first_is_the_same_every_time() {
    // An L-shaped block chamfered at its inner upright with a bottom edge from that upright's foot
    // (a concave edge meeting another at a corner) and at the three edges of a far top corner:
    // both corners are refused, each in its own way. The corners are checked by position, not in
    // hash order, so the first refused, nearest x = 0, is the one reported, every time.
    let pts = [(0.0, 0.0), (10.0, 0.0), (10.0, 4.0), (4.0, 4.0), (4.0, 10.0), (0.0, 10.0)];
    let segs = pts.iter().zip(pts.iter().cycle().skip(1)).map(|(a, b)| Seg2::Line { a: Vec2::new(a.0, a.1), b: Vec2::new(b.0, b.1) }).collect();
    let body = crate::extrude(&Plane::XY, &[Region2 { outer: Loop2 { segs }, holes: vec![] }], 0.0, 5.0).unwrap().pop().unwrap();
    let picks = [Vec3::new(4.0, 4.0, 2.5), Vec3::new(7.0, 4.0, 0.0), Vec3::new(10.0, 2.0, 5.0), Vec3::new(5.0, 0.0, 5.0), Vec3::new(10.0, 0.0, 2.5)];
    let ids: Vec<_> = picks.iter().map(|p| crate::surfaces::edge_near(&body, *p).unwrap().id()).collect();
    for _ in 0..16 {
        let e = super::corner_blend_style(&body.solid, &ids, 10.0, 1.0, false, false).err().unwrap();
        assert_eq!(e, crate::blend::unsupported("concave blends meeting at a corner"));
    }
}
