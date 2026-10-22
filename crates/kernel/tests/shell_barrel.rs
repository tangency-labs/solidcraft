//! #2421: shelling a circular-meridian barrel must retain its curved wall through the rim.

use std::f64::consts::PI;

use solidcraft_geom::{Loop2, Plane, Region2, Seg2, Vec2, Vec3};
use solidcraft_kernel::history::FaceOrigin;
use solidcraft_kernel::*;

const RADIUS: f64 = 24.0;
const CENTRE: f64 = -10.0;
const HALF_HEIGHT: f64 = 13.4;

fn barrel() -> Body {
    let angle = (HALF_HEIGHT / RADIUS).asin();
    let end_radius = CENTRE + RADIUS * angle.cos();
    let region = Region2 {
        outer: Loop2 {
            segs: vec![
                Seg2::Line { a: Vec2::ZERO, b: Vec2::new(end_radius, 0.0) },
                Seg2::Arc { center: Vec2::new(CENTRE, HALF_HEIGHT), radius: RADIUS, start: -angle, sweep: 2.0 * angle },
                Seg2::Line { a: Vec2::new(end_radius, 2.0 * HALF_HEIGHT), b: Vec2::new(0.0, 2.0 * HALF_HEIGHT) },
                Seg2::Line { a: Vec2::new(0.0, 2.0 * HALF_HEIGHT), b: Vec2::ZERO },
            ],
        },
        holes: vec![],
    };
    let plane = Plane::new(Vec3::ZERO, Vec3::X, Vec3::Z).unwrap();
    revolve(&plane, &[region], Vec2::ZERO, Vec2::Y, 2.0 * PI).unwrap().pop().unwrap()
}

// Integral of pi * (CENTRE + sqrt(r^2 - (z-HALF_HEIGHT)^2))^2.
fn integral(r: f64, z: f64) -> f64 {
    let u = z - HALF_HEIGHT;
    PI * ((CENTRE * CENTRE + r * r) * u - u.powi(3) / 3.0 + CENTRE * (u * (r * r - u * u).sqrt() + r * r * (u / r).asin()))
}

fn check_volume(body: &Body, lo: f64, hi: f64) {
    let want = integral(RADIUS, 2.0 * HALF_HEIGHT) - integral(RADIUS, 0.0) - (integral(RADIUS - 2.0, hi) - integral(RADIUS - 2.0, lo));
    let volume = measure(body).unwrap().volume;
    assert!((volume - want).abs() < want * 1e-3, "{volume} vs {want}");
}

#[test]
fn shell_of_a_revolved_barrel() {
    let barrel = barrel();
    let side = (0..barrel.face_count())
        .find_map(|i| match face_surface(&barrel, i) {
            Some(s @ FaceSurface::Torus { major, minor, .. }) if (major - CENTRE).abs() < 1e-6 && (minor - RADIUS).abs() < 1e-6 => Some(s),
            _ => None,
        })
        .expect("barrel side is the inner sheet of a spindle torus");
    assert!(side.nearest(Vec3::new(15.0, 0.0, HALF_HEIGHT)).unwrap().dist(Vec3::new(14.0, 0.0, HALF_HEIGHT)) < 1e-6);
    let result = shell(&barrel, &[Vec3::new(0.0, 0.0, 2.0 * HALF_HEIGHT)], 2.0).unwrap();
    assert!(result.validity().is_empty(), "{:?}", result.validity());
    assert_eq!(result.shell_count(), 1);
    let mesh = result.mesh(&crate::MeshTolerance::chord(0.005)).unwrap();
    assert!(mesh.contains(Vec3::new(0.0, 0.0, 1.0)), "closed floor");
    assert!(!mesh.contains(Vec3::new(0.0, 0.0, 3.0)), "open cavity");
    assert!(mesh.contains(Vec3::new(13.0, 0.0, HALF_HEIGHT)), "barrel wall");
    assert!(!mesh.contains(Vec3::new(11.0, 0.0, HALF_HEIGHT)), "inside wall");
    assert!(mesh.contains(Vec3::new(8.0, 0.0, 26.7)), "curved wall reaches the rim without a cylindrical lip cutting it away");
    check_volume(&result, 2.0, 2.0 * HALF_HEIGHT);
    let history = result.history().expect("exact shell face history");
    assert_eq!(history.operands, 1);
    assert_eq!(history.faces.iter().filter(|f| matches!(f.origins.as_slice(), [FaceOrigin::Offset { operand: 0, .. }])).count(), 4);
    let step = step_export(&[&result], "barrel shell").unwrap();
    let imported = step_import(&step).unwrap();
    assert_eq!(imported.bodies.len(), 1);
    let round_trip = &imported.bodies.first().unwrap().body;
    assert!(round_trip.validity().is_empty());
    check_volume(round_trip, 2.0, 2.0 * HALF_HEIGHT);
}

#[test]
fn barrel_shell_opens_either_or_both_caps_and_refuses_a_vanished_cavity() {
    let barrel = barrel();
    for both in [false, true] {
        let mut opened = vec![Vec3::ZERO];
        if both {
            opened.push(Vec3::new(0.0, 0.0, 2.0 * HALF_HEIGHT));
        }
        let result = shell(&barrel, &opened, 2.0).unwrap();
        assert!(result.validity().is_empty());
        let mesh = result.mesh(&crate::MeshTolerance::chord(0.005)).unwrap();
        assert!(!mesh.contains(Vec3::new(0.0, 0.0, 1.0)));
        assert_eq!(mesh.contains(Vec3::new(0.0, 0.0, 25.8)), !both);
        check_volume(&result, 0.0, 2.0 * HALF_HEIGHT - if both { 0.0 } else { 2.0 });
    }
    for thickness in [11.0, 24.0] {
        assert!(shell(&barrel, &[Vec3::ZERO], thickness).is_err());
    }
}
