//! Two-distance and distance-angle chamfers of curved edges: a circular rim between a planar
//! face and a cylinder or cone, convex (a cylinder's top) and concave (a boss's foot), and a
//! hole's rim. Each chamfer's cross-section is a triangle revolved about the axis, so its volume
//! is the triangle's area times the path of its centroid (Pappus).

use std::f64::consts::PI;

use solidcraft_geom::{Loop2, Plane, Region2, Vec2, Vec3};

use crate::build::{box_solid, extrude, extrude_tapered};
use crate::{Body, BoolOp, ChamferSide, KernelError, boolean, chamfer, chamfer_sides};

const TOL: f64 = 1e-3;

fn volume(b: &Body) -> f64 {
    b.volume_at(TOL).unwrap()
}

fn rel(a: f64, b: f64) -> f64 {
    (a - b).abs() / b.abs().max(1e-12)
}

/// A cylinder of radius 10 and height 20 standing on XY, its seam at `seam`.
fn cylinder(seam: f64) -> Body {
    let region = Region2 { outer: Loop2::circle_from(Vec2::ZERO, 10.0, seam), holes: vec![] };
    extrude(&Plane::XY, &[region], 0.0, 20.0).unwrap().pop().unwrap()
}

/// The volume of the ring swept by the triangle `(r, z)` corners about the Z axis.
fn ring(t: [(f64, f64); 3]) -> f64 {
    let [(r0, z0), (r1, z1), (r2, z2)] = t;
    let area = 0.5 * ((r1 - r0) * (z2 - z0) - (r2 - r0) * (z1 - z0)).abs();
    2.0 * PI * (r0 + r1 + r2) / 3.0 * area
}

fn sound(b: &Body) {
    assert!(b.validity().is_empty(), "{:?}", b.validity());
    assert_eq!(b.lumps().unwrap().len(), 1);
}

const RIM: Vec3 = Vec3 { x: 10.0, y: 0.0, z: 20.0 };

#[test]
fn cylinder_rim_by_two_distances() {
    for seam in [0.0, crate::build::SEAM_ANGLE] {
        let c = cylinder(seam);
        let base = volume(&c);
        // 2 across the top (the face facing up), 3 down the wall.
        let out = chamfer_sides(&c, &[RIM], 2.0, ChamferSide::Distance(3.0), false).unwrap();
        sound(&out);
        let removed = ring([(10.0, 20.0), (8.0, 20.0), (10.0, 17.0)]);
        assert!((removed - 56.0 * PI).abs() < 1e-9);
        assert!(rel(volume(&out), base - removed) < 1e-5, "seam {seam}: {} vs {}", volume(&out), base - removed);
        // Cone faces in place of the rim, as the equal chamfer's.
        assert_eq!(out.face_count(), chamfer(&c, &[RIM], 2.0).unwrap().face_count());
    }
}

#[test]
fn cylinder_rim_by_distance_and_angle() {
    let c = cylinder(0.0);
    let base = volume(&c);
    // 2 across the top at 30° from it: down the wall 2·tan 30° (the faces are square).
    let out = chamfer_sides(&c, &[RIM], 2.0, ChamferSide::Angle(30f64.to_radians()), false).unwrap();
    sound(&out);
    let d2 = 2.0 * 30f64.to_radians().tan();
    let expected = base - ring([(10.0, 20.0), (8.0, 20.0), (10.0, 20.0 - d2)]);
    assert!(rel(volume(&out), expected) < 1e-5, "{} vs {expected}", volume(&out));
}

#[test]
fn cylinder_rim_flipped() {
    let c = cylinder(0.0);
    let base = volume(&c);
    // Flipped: the wall is the first face, 2 down it and 3 across the top.
    let out = chamfer_sides(&c, &[RIM], 2.0, ChamferSide::Distance(3.0), true).unwrap();
    sound(&out);
    let expected = base - ring([(10.0, 20.0), (7.0, 20.0), (10.0, 18.0)]);
    assert!((base - expected - 54.0 * PI).abs() < 1e-9);
    assert!(rel(volume(&out), expected) < 1e-5, "{} vs {expected}", volume(&out));
    // At 60° from the wall: across the top 2·tan 60°.
    let out = chamfer_sides(&c, &[RIM], 2.0, ChamferSide::Angle(60f64.to_radians()), true).unwrap();
    let d2 = 2.0 * 60f64.to_radians().tan();
    let expected = base - ring([(10.0, 20.0), (10.0 - d2, 20.0), (10.0, 18.0)]);
    assert!(rel(volume(&out), expected) < 1e-5, "{} vs {expected}", volume(&out));
}

/// The bottom rim, as a box's bottom edge: the bottom faces down, so the wall is the first face.
#[test]
fn cylinder_bottom_rim_takes_the_wall_first() {
    let c = cylinder(0.0);
    let base = volume(&c);
    let out = chamfer_sides(&c, &[Vec3::new(10.0, 0.0, 0.0)], 2.0, ChamferSide::Distance(3.0), false).unwrap();
    sound(&out);
    let expected = base - ring([(10.0, 0.0), (7.0, 0.0), (10.0, 2.0)]);
    assert!(rel(volume(&out), expected) < 1e-5, "{} vs {expected}", volume(&out));
}

#[test]
fn equal_distances_are_unchanged() {
    let c = cylinder(0.0);
    let base = volume(&c);
    let equal = chamfer(&c, &[RIM], 2.0).unwrap();
    let expected = base - ring([(10.0, 20.0), (8.0, 20.0), (10.0, 18.0)]);
    assert!(rel(volume(&equal), expected) < 1e-5, "{} vs {expected}", volume(&equal));
    // Two equal distances, and 45° on square faces, are the same chamfer.
    for side in [ChamferSide::Distance(2.0), ChamferSide::Angle(PI / 4.0)] {
        let out = chamfer_sides(&c, &[RIM], 2.0, side, false).unwrap();
        assert!(rel(volume(&out), volume(&equal)) < 1e-7, "{side:?}: {} vs {}", volume(&out), volume(&equal));
        assert_eq!(out.face_count(), equal.face_count());
    }
}

/// A 40 × 40 × 10 plate with a hole of radius 5 through it at (20, 20).
fn holed_plate() -> Body {
    let plate = box_solid(Vec3::ZERO, Vec3::new(40.0, 40.0, 10.0)).unwrap();
    let hole = crate::build::cylinder(Vec3::new(20.0, 20.0, -5.0), Vec3::Z, 5.0, 20.0).unwrap();
    boolean(&plate, &hole, BoolOp::Cut).unwrap().unwrap()
}

#[test]
fn hole_rim() {
    let p = holed_plate();
    let base = volume(&p);
    let rim = Vec3::new(25.0, 20.0, 10.0);
    // 2 across the top, away from the hole, and 3 down its wall: a countersink.
    let out = chamfer_sides(&p, &[rim], 2.0, ChamferSide::Distance(3.0), false).unwrap();
    sound(&out);
    let removed = ring([(5.0, 10.0), (7.0, 10.0), (5.0, 7.0)]);
    assert!((removed - 34.0 * PI).abs() < 1e-9);
    assert!(rel(volume(&out), base - removed) < 1e-5, "{} vs {}", volume(&out), base - removed);
    // Flipped, at 30° from the hole's wall.
    let out = chamfer_sides(&p, &[rim], 2.0, ChamferSide::Angle(30f64.to_radians()), true).unwrap();
    sound(&out);
    let d2 = 2.0 * 30f64.to_radians().tan();
    let expected = base - ring([(5.0, 10.0), (5.0 + d2, 10.0), (5.0, 8.0)]);
    assert!(rel(volume(&out), expected) < 1e-5, "{} vs {expected}", volume(&out));
}

/// A concave rim: the foot of a boss of radius 5 standing 10 high on a 40 × 40 × 5 plate.
#[test]
fn boss_foot_fills() {
    let plate = box_solid(Vec3::ZERO, Vec3::new(40.0, 40.0, 5.0)).unwrap();
    let boss = crate::build::cylinder(Vec3::new(20.0, 20.0, 5.0), Vec3::Z, 5.0, 10.0).unwrap();
    let b = boolean(&plate, &boss, BoolOp::Union).unwrap().unwrap();
    let base = volume(&b);
    let foot = Vec3::new(25.0, 20.0, 5.0);
    // The equal chamfer fills a ring.
    let equal = chamfer(&b, &[foot], 2.0).unwrap();
    let filled = ring([(5.0, 5.0), (7.0, 5.0), (5.0, 7.0)]);
    assert!(rel(volume(&equal), base + filled) < 1e-5, "{} vs {}", volume(&equal), base + filled);
    // 2 across the plate (facing up), 3 up the boss.
    let out = chamfer_sides(&b, &[foot], 2.0, ChamferSide::Distance(3.0), false).unwrap();
    sound(&out);
    let filled = ring([(5.0, 5.0), (7.0, 5.0), (5.0, 8.0)]);
    assert!(rel(volume(&out), base + filled) < 1e-5, "{} vs {}", volume(&out), base + filled);
    // 2 up the boss at 30° from it.
    let out = chamfer_sides(&b, &[foot], 2.0, ChamferSide::Angle(30f64.to_radians()), true).unwrap();
    sound(&out);
    let d2 = 2.0 * 30f64.to_radians().tan();
    let filled = ring([(5.0, 5.0), (5.0 + d2, 5.0), (5.0, 7.0)]);
    assert!(rel(volume(&out), base + filled) < 1e-5, "{} vs {}", volume(&out), base + filled);
}

/// A frustum's top rim, between the top and a cone: the faces meet at 90° + the taper.
#[test]
fn cone_rim() {
    let taper = 10f64.to_radians();
    let region = Region2 { outer: Loop2::circle_from(Vec2::ZERO, 10.0, 0.0), holes: vec![] };
    let f = extrude_tapered(&Plane::XY, &region, 20.0, 1.0, -taper, crate::DraftCorner::default()).unwrap();
    let base = volume(&f);
    let top = 10.0 - 20.0 * taper.tan();
    let rim = Vec3::new(top, 0.0, 20.0);
    // Down the cone's slant, away from the top: (sin, −cos) of the taper.
    let down = |d: f64| (top + d * taper.sin(), 20.0 - d * taper.cos());
    // The cone is meshed less closely than the cylinder: compare what the chamfer takes away
    // (within the mesh's error, about 1e-3 of it at this tolerance).
    let out = chamfer_sides(&f, &[rim], 2.0, ChamferSide::Distance(3.0), false).unwrap();
    sound(&out);
    let removed = ring([(top, 20.0), (top - 2.0, 20.0), down(3.0)]);
    assert!(rel(base - volume(&out), removed) < 3e-3, "{} vs {removed}", base - volume(&out));
    // By an angle: the triangle edge–A–B has φ = 90° + taper at the edge and 40° at A.
    let a = 40f64.to_radians();
    let phi = PI / 2.0 + taper;
    let d2 = 2.0 * a.sin() / (phi + a).sin();
    let out = chamfer_sides(&f, &[rim], 2.0, ChamferSide::Angle(a), false).unwrap();
    sound(&out);
    let removed = ring([(top, 20.0), (top - 2.0, 20.0), down(d2)]);
    assert!(rel(base - volume(&out), removed) < 3e-3, "{} vs {removed}", base - volume(&out));
    // The equal chamfer, as before.
    let out = chamfer(&f, &[rim], 2.0).unwrap();
    let removed = ring([(top, 20.0), (top - 2.0, 20.0), down(2.0)]);
    assert!(rel(base - volume(&out), removed) < 3e-3, "{} vs {removed}", base - volume(&out));
}

#[test]
fn too_large_or_unsupported_is_refused() {
    let c = cylinder(0.0);
    // Across the whole top: the cone would end in a point.
    assert!(chamfer_sides(&c, &[RIM], 10.0, ChamferSide::Distance(3.0), false).is_err());
    assert!(chamfer_sides(&c, &[RIM], 12.0, ChamferSide::Distance(3.0), false).is_err());
    // Down past the bottom.
    assert!(chamfer_sides(&c, &[RIM], 2.0, ChamferSide::Distance(25.0), false).is_err());
    // An angle that runs past the wall.
    assert!(chamfer_sides(&c, &[RIM], 2.0, ChamferSide::Angle(PI / 2.0 + 0.01), false).is_err());
    // A cylinder's seam is smooth: nothing to chamfer.
    assert!(chamfer_sides(&c, &[Vec3::new(10.0, 0.0, 10.0)], 2.0, ChamferSide::Distance(3.0), false).is_err());
    // A tilted cylinder cut flat: its rim on the cut is an ellipse, which the loop chamfer does
    // not take. Refused as not supported, never a wrong solid.
    let tilted = crate::build::cylinder(Vec3::new(0.0, 0.0, -10.0), Vec3::new(1.0, 0.0, 2.0), 5.0, 40.0).unwrap();
    let cut = box_solid(Vec3::new(-50.0, -50.0, 10.0), Vec3::new(50.0, 50.0, 50.0)).unwrap();
    let t = boolean(&tilted, &cut, BoolOp::Cut).unwrap().unwrap();
    let rim = t
        .record()
        .edges
        .iter()
        .filter_map(|e| solidcraft_geom::brep::eval::CurveEval::new(&e.curve).ok()?.point(0.5 * (e.interval[0] + e.interval[1])))
        .find(|m| (m.z - 10.0).abs() < 1e-6)
        .unwrap();
    match chamfer_sides(&t, &[rim], 1.0, ChamferSide::Distance(2.0), false) {
        Err(KernelError::Failed(m)) => assert!(m.starts_with("not supported yet"), "{m}"),
        Err(e) => panic!("{e}"),
        Ok(_) => panic!("an unequal chamfer of an elliptical rim was built"),
    }
}

/// The top loop of a plate with rounded corners: lines and quarter circles, planar and
/// cylindrical walls, chamfered all round (every edge of the loop picked).
#[test]
fn rounded_plate_top_loop() {
    let b = box_solid(Vec3::ZERO, Vec3::new(40.0, 30.0, 10.0)).unwrap();
    let corners = [Vec3::new(0.0, 0.0, 5.0), Vec3::new(40.0, 0.0, 5.0), Vec3::new(40.0, 30.0, 5.0), Vec3::new(0.0, 30.0, 5.0)];
    let b = crate::fillet(&b, &corners, 5.0).unwrap();
    let base = volume(&b);
    let k = 5.0 * (1.0 - std::f64::consts::FRAC_1_SQRT_2);
    let picks = [
        Vec3::new(20.0, 0.0, 10.0),
        Vec3::new(40.0, 15.0, 10.0),
        Vec3::new(20.0, 30.0, 10.0),
        Vec3::new(0.0, 15.0, 10.0),
        Vec3::new(k, k, 10.0),
        Vec3::new(40.0 - k, k, 10.0),
        Vec3::new(40.0 - k, 30.0 - k, 10.0),
        Vec3::new(k, 30.0 - k, 10.0),
    ];
    // Every pick is taken on the plate as given: cutting the lines first used to leave the arcs'
    // picks nearest to the new chamfer faces' edges, which were cut instead (corner wedges).
    let out = chamfer_sides(&b, &picks, 2.0, ChamferSide::Distance(3.0), false).unwrap();
    sound(&out);
    // 100 of straight sides with the triangle (2 × 3) and four quarter rings about the corners.
    let removed = 100.0 * 3.0 + ring([(5.0, 10.0), (3.0, 10.0), (5.0, 7.0)]);
    assert!(rel(base - volume(&out), removed) < 2e-3, "{} vs {removed}", base - volume(&out));
    // Flipped: 2 down the walls, 3 across the top.
    let out = chamfer_sides(&b, &picks, 2.0, ChamferSide::Distance(3.0), true).unwrap();
    sound(&out);
    let removed = 100.0 * 3.0 + ring([(5.0, 10.0), (2.0, 10.0), (5.0, 8.0)]);
    assert!(rel(base - volume(&out), removed) < 2e-3, "{} vs {removed}", base - volume(&out));
    // Equal distances: as the equal chamfer.
    let equal = chamfer(&b, &picks, 2.0).unwrap();
    let out = chamfer_sides(&b, &picks, 2.0, ChamferSide::Distance(2.0), false).unwrap();
    assert!(rel(volume(&out), volume(&equal)) < 1e-9);
}
