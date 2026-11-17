//! #13455: a plate joined to a tube whose outer wall one of the plate's arcs lies on, the
//! plate's straight sides meeting that wall tangentially.

use std::f64::consts::TAU;

use solidcore_record::{Loop2, Plane, Region2, Seg2, Vec2};

use crate::*;

const BOSS: Vec2 = Vec2 { x: -87.5, y: 0.0 };
const R_BIG: f64 = 52.5;
const R_BOSS: f64 = 23.665;

fn circle(center: Vec2, radius: f64) -> Loop2 {
    Loop2 { segs: vec![Seg2::Arc { center, radius, start: 0.0, sweep: TAU }] }
}

/// The tube: radii 11.665 to 23.665 round (-87.5, 0), 25 tall.
fn tube() -> Body {
    let region = Region2 { outer: circle(BOSS, R_BOSS), holes: vec![circle(BOSS, 11.665)] };
    extrude(&Plane::XY, &[region], 0.0, 25.0).unwrap().remove(0)
}

/// The plate, 12 thick: between the outer tangent lines of the 52.5 circle round the origin and
/// the tube's outer circle, the big circle's far arc and the tube circle's near arc (on the
/// tube's wall), less a 40.5 hole round the origin.
fn plate() -> Body {
    let theta = (-(R_BIG - R_BOSS) / BOSS.x.abs()).acos();
    let at = |c: Vec2, r: f64, a: f64| c + Vec2::new(a.cos(), a.sin()) * r;
    let (p2, p3) = (at(BOSS, R_BOSS, theta), at(Vec2::ZERO, R_BIG, theta));
    let (p5, p6) = (at(BOSS, R_BOSS, -theta), at(Vec2::ZERO, R_BIG, -theta));
    let segs = vec![
        Seg2::Line { a: p2, b: p3 },
        Seg2::Arc { center: Vec2::ZERO, radius: R_BIG, start: theta, sweep: -2.0 * theta },
        Seg2::Line { a: p6, b: p5 },
        Seg2::Arc { center: BOSS, radius: R_BOSS, start: -theta, sweep: 2.0 * theta },
    ];
    let region = Region2 { outer: Loop2 { segs }, holes: vec![circle(Vec2::ZERO, 40.5)] };
    extrude(&Plane::XY, &[region], 0.0, 12.0).unwrap().remove(0)
}

#[test]
fn a_plate_on_a_tubes_wall_with_tangent_sides_joins() {
    let (t, p) = (tube(), plate());
    let (vt, vp) = (measure(&t).unwrap().volume, measure(&p).unwrap().volume);
    let joined = boolean(&t, &p, BoolOp::Union).unwrap_or_else(|e| panic!("{e}")).expect("a body");
    crate::validation::check_kept(&joined).unwrap_or_else(|e| panic!("{e}"));
    let m = measure(&joined).unwrap();
    assert!((m.volume - (vt + vp)).abs() < (vt + vp) * 1e-6, "{} vs {}", m.volume, vt + vp);
}

/// The ring: radii 40.5 to 52.5 round the origin, 57 tall.
fn ring() -> Body {
    let region = Region2 { outer: circle(Vec2::ZERO, R_BIG), holes: vec![circle(Vec2::ZERO, 40.5)] };
    extrude(&Plane::XY, &[region], 0.0, 57.0).unwrap().remove(0)
}

/// The web, 12 thick, as in the reporter's design: between the outer tangent lines of the ring's
/// outer circle and the tube's, bounded by the ring circle's near arc and the tube circle's near
/// arc, each on that body's wall, the straight sides tangent to both walls.
fn web() -> Body {
    let theta = (-(R_BIG - R_BOSS) / BOSS.x.abs()).acos();
    let at = |c: Vec2, r: f64, a: f64| c + Vec2::new(a.cos(), a.sin()) * r;
    let (p2, p3) = (at(BOSS, R_BOSS, theta), at(Vec2::ZERO, R_BIG, theta));
    let (p5, p6) = (at(BOSS, R_BOSS, -theta), at(Vec2::ZERO, R_BIG, -theta));
    let segs = vec![
        Seg2::Line { a: p2, b: p3 },
        Seg2::Arc { center: Vec2::ZERO, radius: R_BIG, start: theta, sweep: TAU - 2.0 * theta },
        Seg2::Line { a: p6, b: p5 },
        Seg2::Arc { center: BOSS, radius: R_BOSS, start: -theta, sweep: 2.0 * theta },
    ];
    extrude(&Plane::XY, &[Region2 { outer: Loop2 { segs }, holes: vec![] }], 0.0, 12.0).unwrap().remove(0)
}

/// The reporter's join: the web joined to the tube, then to the ring.
#[test]
fn a_web_between_two_walls_with_tangent_sides_joins_both() {
    let (t, r, w) = (tube(), ring(), web());
    let v: f64 = [&t, &r, &w].iter().map(|b| measure(b).unwrap().volume).sum();
    let one = boolean(&w, &t, BoolOp::Union).unwrap_or_else(|e| panic!("web + tube: {e}")).expect("a body");
    let joined = boolean(&one, &r, BoolOp::Union).unwrap_or_else(|e| panic!("+ ring: {e}")).expect("a body");
    crate::validation::check_kept(&joined).unwrap_or_else(|e| panic!("{e}"));
    let m = measure(&joined).unwrap();
    assert!((m.volume - v).abs() < v * 1e-6, "{} vs {v}", m.volume);
}

/// The web joined to the ring first: the face SMLib deletes, the web's arc on the ring's wall,
/// has the outside on one side and the web on the other, with the ring just across it.
#[test]
fn a_web_joined_to_the_ring_first_joins() {
    let (r, w) = (ring(), web());
    let v = measure(&r).unwrap().volume + measure(&w).unwrap().volume;
    let joined = boolean(&w, &r, BoolOp::Union).unwrap_or_else(|e| panic!("web + ring: {e}")).expect("a body");
    crate::validation::check_kept(&joined).unwrap_or_else(|e| panic!("{e}"));
    let m = measure(&joined).unwrap();
    assert!((m.volume - v).abs() < v * 1e-6, "{} vs {v}", m.volume);
}
