//! A rolling-ball round between a plane and a cylinder square to it is a piece of a torus (the
//! exact circle class): its faces say so, with the torus its net is, certified against its
//! definition, and keep saying so after the body is saved and opened again and after it goes
//! through SMLib again.
//!
//! The torus is known in closed form: the ball of radius r rolls on the plane z = h and the
//! cylinder of radius R about the axis through (cx, cy); its centre runs on the circle of radius
//! R + r (the round outside the cylinder's material, a bore's rim or a boss's root) at the height
//! h - r below the plane's material or h + r above it, and the round is the tube of radius r
//! about that circle. A closed round may be two faces (split at its surface's seam), each on
//! that torus.

use solidcore_record::Vec3;
use solidcraft_kernel::*;

/// The tori among a body's faces: centre, axis, major and minor radius.
fn tori(body: &Body) -> Vec<(Vec3, Vec3, f64, f64)> {
    (0..body.face_count())
        .filter_map(|i| match face_surface(body, i) {
            Some(FaceSurface::Torus { center, axis, major, minor }) => Some((center, *axis, major, minor)),
            _ => None,
        })
        .collect()
}

/// `body` has torus faces, every one of centre `center`, axis +Z or -Z and radii `major` and
/// `minor`, each component within `tol`.
fn assert_torus(body: &Body, center: Vec3, major: f64, minor: f64, tol: f64, what: &str) {
    let found = tori(body);
    assert!(!found.is_empty(), "{what}: no torus face");
    let near = |got: f64, want: f64| (got - want).abs() <= tol;
    for (c, a, big, small) in found {
        assert!(near(c.x, center.x) && near(c.y, center.y) && near(c.z, center.z), "{what}: centre {c:?} against {center:?}");
        let along_z = near(a.x, 0.0) && near(a.y, 0.0) && (near(a.z, 1.0) || near(a.z, -1.0));
        assert!(along_z, "{what}: axis {a:?} against +Z or -Z");
        assert!(near(big, major), "{what}: major radius {big} against {major}");
        assert!(near(small, minor), "{what}: minor radius {small} against {minor}");
    }
}

/// The torus survives the body's save and reopen (its record, number for number) and a pass
/// through SMLib (the body built again from its record and written out once more). SMLib reads
/// the face's form back as its own torus (`IsNurbSurfaceTorus`), which finds the torus from the
/// form's circles: to their rounding, not bit for bit, so that pass is held to 1e-9.
fn assert_kept(body: &Body, center: Vec3, major: f64, minor: f64) {
    let reopened = decode_native_body(&encode_native_body(body).unwrap()).unwrap();
    assert_torus(&reopened, center, major, minor, 1e-12, "reopened");
    let rebuilt = body_from_smlib(&smlib_body(&reopened).unwrap()).unwrap();
    assert_torus(&rebuilt, center, major, minor, 1e-9, "rebuilt in SMLib");
}

/// A box 30 x 30 x 10 with a bore of radius 5 through it about (15, 15), its top rim rounded by
/// 0.3: the ball's centre runs on the circle of radius 5.3 at z = 9.7.
#[test]
fn a_bore_rim_round_is_a_torus() {
    let block = box_solid(Vec3::ZERO, Vec3::new(30.0, 30.0, 10.0)).unwrap();
    let bore = cylinder(Vec3::new(15.0, 15.0, 0.0), Vec3::Z, 5.0, 10.0).unwrap();
    let drilled = boolean(&block, &bore, BoolOp::Cut).unwrap().unwrap();
    let rounded = fillet(&drilled, &[Vec3::new(20.0, 15.0, 10.0)], 0.3).unwrap();
    let (center, major, minor) = (Vec3::new(15.0, 15.0, 9.7), 5.3, 0.3);
    assert_torus(&rounded, center, major, minor, 1e-12, "built");
    assert_kept(&rounded, center, major, minor);
}

/// The same box with a boss of radius 5 and height 5 standing on its top about (15, 15), its root
/// rounded by 0.3: the ball's centre runs on the circle of radius 5.3 at z = 10.3.
#[test]
fn a_boss_root_round_is_a_torus() {
    let block = box_solid(Vec3::ZERO, Vec3::new(30.0, 30.0, 10.0)).unwrap();
    let boss = cylinder(Vec3::new(15.0, 15.0, 10.0), Vec3::Z, 5.0, 5.0).unwrap();
    let joined = boolean(&block, &boss, BoolOp::Union).unwrap().unwrap();
    let rounded = fillet(&joined, &[Vec3::new(20.0, 15.0, 10.0)], 0.3).unwrap();
    let (center, major, minor) = (Vec3::new(15.0, 15.0, 10.3), 5.3, 0.3);
    assert_torus(&rounded, center, major, minor, 1e-12, "built");
    assert_kept(&rounded, center, major, minor);
}
