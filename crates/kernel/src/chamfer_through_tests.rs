//! Convex chamfers whose setbacks run past the faces they join, cut through the faces they reach
//! ([`crate::chamfer_through`]): a hole's rim chamfered deeper than the plate is thick or wider
//! than the plate round it, a cylinder's rim chamfered past its foot, a box's edge chamfered past
//! the bottom or across the whole top. Each result is compared with a volume worked out on its
//! own (a numeric integral of the cut's cross-sections) and must stay within the body's bounds.

use std::f64::consts::PI;

use solidcraft_geom::{Loop2, Plane, Region2, Vec2, Vec3};

use crate::build::{box_solid, extrude};
use crate::{Body, BoolOp, ChamferSide, KernelError, boolean, chamfer, chamfer_sides};

/// The user's plate: 50 × 50 centred on the origin, 3/4 in thick, with a 12.7 mm hole through it.
const HALF: f64 = 25.0;
const THICK: f64 = 19.05;
const HOLE: f64 = 6.35;
/// The hole's bottom rim, picked as its two halves.
const BOTTOM_RIM: [Vec3; 2] = [Vec3 { x: 0.0, y: HOLE, z: 0.0 }, Vec3 { x: 0.0, y: -HOLE, z: 0.0 }];

fn plate() -> Body {
    let square = Loop2::polygon(&[Vec2::new(-HALF, -HALF), Vec2::new(HALF, -HALF), Vec2::new(HALF, HALF), Vec2::new(-HALF, HALF)]);
    let region = Region2 { outer: square, holes: vec![Loop2::circle(Vec2::ZERO, HOLE)] };
    extrude(&Plane::XY, &[region], 0.0, THICK).unwrap().pop().unwrap()
}

fn volume(b: &Body) -> f64 {
    b.volume_at(1e-4).unwrap()
}

fn sound(b: &Body) {
    assert!(b.validity().is_empty(), "{:?}", b.validity());
    assert_eq!(b.lumps().unwrap().len(), 1);
}

/// The area of the square of half-width `h` about the origin within `rho` of it.
fn square_disc(h: f64, rho: f64) -> f64 {
    if rho <= h {
        PI * rho * rho
    } else if rho < h * 2f64.sqrt() {
        // Less the four caps of the disc beyond the sides.
        PI * rho * rho - 4.0 * (rho * rho * (h / rho).acos() - h * (rho * rho - h * h).sqrt())
    } else {
        4.0 * h * h
    }
}

/// Simpson's rule over [a, b] in `n` (even) steps.
fn integral(f: impl Fn(f64) -> f64, a: f64, b: f64, n: usize) -> f64 {
    let h = (b - a) / n as f64;
    let mut sum = f(a) + f(b);
    for i in 1..n {
        sum += f(a + h * i as f64) * if i % 2 == 1 { 4.0 } else { 2.0 };
    }
    sum * h / 3.0
}

/// What a chamfer of the plate's bottom rim takes away: `sw` up the hole's wall and `sf` out
/// across the bottom, the chamfer line from (R + sf, 0) to (R, sw) turned about the axis and run
/// on through whatever it reaches. At height z the cut is the square within ρ(z) = R + sf (1 − z/sw)
/// of the axis, less the hole.
fn bottom_rim_removed(sf: f64, sw: f64) -> f64 {
    let rho = |z: f64| HOLE + sf * (1.0 - z / sw);
    integral(|z| (square_disc(HALF, rho(z)) - PI * HOLE * HOLE).max(0.0), 0.0, THICK.min(sw), 200_000)
}

fn within_plate(b: &Body) {
    // From a mesh within 1e-3 mm: where the cut's cone meets the plate's sides (along curves the
    // boolean fits) its points may lie a fraction of that out.
    let bb = b.tessellate(1e-3).unwrap().bounds();
    let slack = 1e-3;
    assert!(bb.min.x >= -HALF - slack && bb.min.y >= -HALF - slack && bb.min.z >= -slack, "{bb:?}");
    assert!(bb.max.x <= HALF + slack && bb.max.y <= HALF + slack && bb.max.z <= THICK + slack, "{bb:?}");
}

fn check_plate(out: &Body, base: f64, sf: f64, sw: f64, what: &str) {
    sound(out);
    within_plate(out);
    let want = base - bottom_rim_removed(sf, sw);
    let got = volume(out);
    assert!((got - want).abs() < want * 2e-5, "{what}: {got} vs {want}");
}

/// The user's report: the bottom rim of the plate's hole chamfered by a distance up the wall (the
/// wall is the first face: the bottom faces down) at 45°, so as far out across the bottom.
#[test]
fn plate_hole_bottom_rim_at_45_degrees() {
    let p = plate();
    let base = volume(&p);
    assert!((base - (4.0 * HALF * HALF - PI * HOLE * HOLE) * THICK).abs() < 1e-6 * base);
    let angle = ChamferSide::Angle(PI / 4.0);
    // 10 mm fits: the exact chamfer, a cone between the bottom and the wall, as before.
    let out = chamfer_sides(&p, &BOTTOM_RIM, 10.0, angle, false).unwrap();
    check_plate(&out, base, 10.0, 10.0, "10 mm");
    assert_eq!(out.face_count(), p.face_count() + 2, "a cone face in place of each half of the rim");
    // 19 mm up the wall fits it, but 19 mm across the bottom reaches past the plate's sides
    // (6.35 + 19 > 25): it used to build a cone sticking out of them. Now cut through them.
    for d in [19.0, THICK, 25.0] {
        let out = chamfer_sides(&p, &BOTTOM_RIM, d, angle, false).unwrap();
        check_plate(&out, base, d, d, &format!("{d} mm at 45°"));
    }
}

/// 25 mm up the wall at 30° to it: past the top, a countersink through the top face, and
/// 25 · sin 30° / sin 120° across the bottom.
#[test]
fn plate_hole_bottom_rim_past_the_top_at_30_degrees() {
    let p = plate();
    let base = volume(&p);
    let a = 30f64.to_radians();
    let sf = 25.0 * a.sin() / (PI / 2.0 + a).sin();
    let out = chamfer_sides(&p, &BOTTOM_RIM, 25.0, ChamferSide::Angle(a), false).unwrap();
    check_plate(&out, base, sf, 25.0, "25 mm at 30°");
    // 19 mm at 60°: 19 · tan 60° across the bottom, past the plate's corners: the whole bottom
    // goes up to where the cone leaves the corners.
    let sf = 19.0 * 60f64.to_radians().tan();
    let out = chamfer_sides(&p, &BOTTOM_RIM, 19.0, ChamferSide::Angle(60f64.to_radians()), false).unwrap();
    check_plate(&out, base, sf, 19.0, "19 mm at 60°");
}

/// Equal-distance chamfers and the top rim take the same way.
#[test]
fn plate_hole_rims_equal_and_top() {
    let p = plate();
    let base = volume(&p);
    let out = chamfer(&p, &BOTTOM_RIM, 25.0).unwrap();
    check_plate(&out, base, 25.0, 25.0, "equal 25 mm");
    // The top rim: the top faces up, so it is the first face; mirrored, the same volume.
    let top = [Vec3::new(0.0, HOLE, THICK), Vec3::new(0.0, -HOLE, THICK)];
    let out = chamfer_sides(&p, &top, 25.0, ChamferSide::Angle(PI / 4.0), false).unwrap();
    check_plate(&out, base, 25.0, 25.0, "top 25 mm at 45°");
    // Flipped: the wall first, 10 down it and 20 across the top (past the sides).
    let out = chamfer_sides(&p, &top, 10.0, ChamferSide::Distance(20.0), true).unwrap();
    check_plate(&out, base, 20.0, 10.0, "top 10 mm down, 20 mm across");
}

/// A cylinder's top rim chamfered 2 across the top and 25 down its 20 mm wall: through the
/// bottom, a frustum.
#[test]
fn cylinder_rim_taller_than_the_cylinder() {
    let region = Region2 { outer: Loop2::circle_from(Vec2::ZERO, 10.0, 0.0), holes: vec![] };
    let c = extrude(&Plane::XY, &[region], 0.0, 20.0).unwrap().pop().unwrap();
    let rim = [Vec3::new(10.0, 0.0, 20.0)];
    let out = chamfer_sides(&c, &rim, 2.0, ChamferSide::Distance(25.0), false).unwrap();
    sound(&out);
    // From radius 8 at the top to 8 + 2 · 20/25 at the bottom.
    let (r1, r0) = (8.0, 8.0 + 2.0 * 20.0 / 25.0);
    let want = PI / 3.0 * 20.0 * (r0 * r0 + r0 * r1 + r1 * r1);
    let got = volume(&out);
    assert!((got - want).abs() < want * 2e-5, "{got} vs {want}");
    let bb = crate::measure(&out).unwrap().bbox;
    assert!(bb.max.x <= r0 + 1e-6 && bb.min.z >= -1e-6 && bb.max.z <= 20.0 + 1e-6, "{bb:?}");
    // Across the whole top and on past the axis: the cone would end in a point. Refused.
    assert!(chamfer_sides(&c, &rim, 10.0, ChamferSide::Distance(3.0), false).is_err());
    assert!(chamfer_sides(&c, &rim, 25.0, ChamferSide::Distance(3.0), false).is_err());
}

/// A box's top front edge (40 long, the box 30 deep and 10 high).
const BOX_EDGE: [Vec3; 1] = [Vec3 { x: 20.0, y: 0.0, z: 10.0 }];

fn bx() -> Body {
    box_solid(Vec3::ZERO, Vec3::new(40.0, 30.0, 10.0)).unwrap()
}

/// What a chamfer of the box's top front edge takes away: `s1` across the top and `s2` down the
/// front, the chamfer plane run on through the box. At height z the cut runs back from the front
/// to the chamfer line, as far as the back.
fn box_removed(s1: f64, s2: f64) -> f64 {
    let depth = |z: f64| (s1 * (1.0 - (10.0 - z) / s2)).clamp(0.0, 30.0);
    40.0 * integral(depth, 0.0, 10.0, 200_000)
}

fn check_box(out: &Body, s1: f64, s2: f64, what: &str) {
    sound(out);
    let bb = crate::measure(out).unwrap().bbox;
    assert!(bb.min.x >= -1e-6 && bb.min.y >= -1e-6 && bb.min.z >= -1e-6, "{what}: {bb:?}");
    assert!(bb.max.x <= 40.0 + 1e-6 && bb.max.y <= 30.0 + 1e-6 && bb.max.z <= 10.0 + 1e-6, "{what}: {bb:?}");
    let want = 12000.0 - box_removed(s1, s2);
    let got = volume(out);
    assert!((got - want).abs() < 1e-6 * 12000.0, "{what}: {got} vs {want}");
}

/// A box's edge chamfered further than the box is high: the plane runs out through the bottom.
#[test]
fn box_edge_past_the_bottom() {
    let b = bx();
    // Equal 15: 15 across the top, 15 down the front (5 past the bottom).
    check_box(&chamfer(&b, &BOX_EDGE, 15.0).unwrap(), 15.0, 15.0, "equal 15");
    // 8 across the top, 15 down.
    check_box(&chamfer_sides(&b, &BOX_EDGE, 8.0, ChamferSide::Distance(15.0), false).unwrap(), 8.0, 15.0, "8 and 15");
    // 25 across the top at 30° to it: 25 · tan 30° down the front.
    let s2 = 25.0 * 30f64.to_radians().tan();
    check_box(&chamfer_sides(&b, &BOX_EDGE, 25.0, ChamferSide::Angle(30f64.to_radians()), false).unwrap(), 25.0, s2, "25 at 30°");
    // Flipped: 15 down the front, 8 across the top.
    check_box(&chamfer_sides(&b, &BOX_EDGE, 15.0, ChamferSide::Distance(8.0), true).unwrap(), 8.0, 15.0, "flipped");
    // Wider than the top: 35 across it (30 deep) and 5 down, through the back.
    check_box(&chamfer_sides(&b, &BOX_EDGE, 35.0, ChamferSide::Distance(5.0), false).unwrap(), 35.0, 5.0, "35 and 5");
}

/// Chamfers that fit are built as before; ones running into other geometry, past concave
/// corners or onto concave edges are still refused.
#[test]
fn fitting_chamfers_unchanged_and_others_refused() {
    let b = bx();
    let out = chamfer_sides(&b, &BOX_EDGE, 8.0, ChamferSide::Distance(5.0), false).unwrap();
    assert_eq!(out.face_count(), 7);
    assert!((volume(&out) - (12000.0 - 0.5 * 8.0 * 5.0 * 40.0)).abs() < 1e-6);
    // A pocket's rim edge, chamfered past the pocket's floor: its ends are the pocket's corners,
    // which a cut along it would run into. Refused.
    let pocket = box_solid(Vec3::new(10.0, 10.0, 4.0), Vec3::new(30.0, 20.0, 11.0)).unwrap();
    let pocketed = boolean(&b, &pocket, BoolOp::Cut).unwrap().unwrap();
    assert!(chamfer(&pocketed, &[Vec3::new(20.0, 10.0, 10.0)], 8.0).is_err());
    // A concave edge (a boss's foot) chamfered past the boss's top: filling outside the faces.
    let plate = box_solid(Vec3::ZERO, Vec3::new(40.0, 40.0, 5.0)).unwrap();
    let boss = crate::build::cylinder(Vec3::new(20.0, 20.0, 5.0), Vec3::Z, 5.0, 10.0).unwrap();
    let bossed = boolean(&plate, &boss, BoolOp::Union).unwrap().unwrap();
    assert!(chamfer_sides(&bossed, &[Vec3::new(25.0, 20.0, 5.0)], 2.0, ChamferSide::Distance(15.0), false).is_err());
    // A step's riser beyond the top face: the cut would undercut it. Refused.
    let step = boolean(&b, &box_solid(Vec3::new(0.0, 15.0, 10.0), Vec3::new(40.0, 30.0, 20.0)).unwrap(), BoolOp::Union).unwrap().unwrap();
    match chamfer_sides(&step, &BOX_EDGE, 20.0, ChamferSide::Distance(15.0), false) {
        Err(KernelError::TooLarge(_) | KernelError::Failed(_)) => {}
        other => panic!("{:?}", other.map(|b| volume(&b))),
    }
}

/// All four top edges chamfered past the bottom, and two of them: the cuts mitre where they
/// meet. At each point of the top the cut leaves the box up to `d − 5` high (`d` the distance to
/// the nearest chamfered side), so by layers the box keeps ∫₀¹⁰ (area where d − 5 > t) dt. And a
/// vertical edge chamfered 35 each way: through the back face (30 deep).
#[test]
fn box_edges_past_the_bottom_mitre() {
    let b = bx();
    let top = [Vec3::new(20.0, 0.0, 10.0), Vec3::new(40.0, 15.0, 10.0), Vec3::new(20.0, 30.0, 10.0), Vec3::new(0.0, 15.0, 10.0)];
    let layers = |area: &dyn Fn(f64) -> f64| integral(area, 0.0, 10.0, 1000);
    let out = chamfer(&b, &top, 15.0).unwrap();
    sound(&out);
    let want = layers(&|t| (30.0 - 2.0 * t) * (20.0 - 2.0 * t));
    assert!((volume(&out) - want).abs() < 1e-6 * 12000.0, "{} vs {want}", volume(&out));
    let out = chamfer(&b, &top[..2], 15.0).unwrap();
    sound(&out);
    let want = layers(&|t| (25.0 - t) * (35.0 - t));
    assert!((volume(&out) - want).abs() < 1e-6 * 12000.0, "{} vs {want}", volume(&out));
    // The triangle x + y < 35 of the 40 × 30 footprint, less its corner past y = 30, 10 high.
    let out = chamfer(&b, &[Vec3::new(0.0, 0.0, 5.0)], 35.0).unwrap();
    sound(&out);
    let want = 12000.0 - (35.0 * 35.0 / 2.0 - 5.0 * 5.0 / 2.0) * 10.0;
    assert!((volume(&out) - want).abs() < 1e-6 * 12000.0, "{} vs {want}", volume(&out));
    // Exactly to the bottom edge and the back's top edge: the plane through both halves the box.
    let out = chamfer_sides(&b, &BOX_EDGE, 30.0, ChamferSide::Distance(10.0), false).unwrap();
    sound(&out);
    assert!((volume(&out) - 6000.0).abs() < 1e-6 * 12000.0, "{}", volume(&out));
    // Exactly to the bottom edge and past the back: through the back.
    check_box(&chamfer_sides(&b, &BOX_EDGE, 35.0, ChamferSide::Distance(10.0), false).unwrap(), 35.0, 10.0, "35 and 10");
}
