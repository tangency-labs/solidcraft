//! Bodies into SMLib through their schema 4 records: the kernel's primitives keep their analytic
//! volume.
use std::f64::consts::PI;

use solidcraft_geom::Vec3;

use crate::{Body, box_solid, cylinder, sphere, torus};

fn v(x: f64, y: f64, z: f64) -> Vec3 {
    Vec3::new(x, y, z)
}

/// The body's SMLib volume, and whether SMLib finds it a manifold solid.
fn in_smlib(body: &Body) -> Result<(f64, bool), String> {
    let s = super::smlib_body(body).map_err(|e| e.to_string())?;
    Ok((s.volume(1e-7).map_err(|e| e.to_string())?, s.is_manifold_solid().unwrap_or(false)))
}

#[test]
fn primitives_keep_their_analytic_volume() {
    let (got, solid) = in_smlib(&box_solid(v(0., 0., 0.), v(10., 20., 30.)).unwrap()).unwrap();
    assert!(solid && (got - 6000.0).abs() <= 1e-9 * 6000.0, "box: SMLib volume {got}, manifold {solid}");
    let (got, solid) = in_smlib(&cylinder(v(1., 2., 3.), v(0., 0., 1.), 5.0, 10.0).unwrap()).unwrap();
    assert!(solid && (got - 250.0 * PI).abs() <= 1e-6 * 250.0 * PI, "cylinder: SMLib volume {got}, manifold {solid}");
    let (got, solid) = in_smlib(&cylinder(v(0., 0., 0.), v(1., 1., 1.), 2.0, 7.0).unwrap()).unwrap();
    assert!(solid && (got - 28.0 * PI).abs() <= 1e-6 * 28.0 * PI, "tilted cylinder: SMLib volume {got}, manifold {solid}");
}

/// Truck holds spheres and tori as revolutions (`PlacedRevolution`): SMLib gets them as its
/// analytic sphere and torus, faces turned over where the revolution's normal points inward.
#[test]
fn revolutions_keep_their_analytic_volume() {
    let cases = [
        ("sphere", sphere(v(1., 2., 3.), 5.0).unwrap(), 4.0 / 3.0 * PI * 125.0),
        ("torus", torus(v(0., 0., 0.), 10.0, 2.0).unwrap(), 2.0 * PI * PI * 10.0 * 4.0),
    ];
    for (name, body, exact) in cases {
        let s = super::smlib_body(&body).unwrap_or_else(|e| panic!("{name}: {e}"));
        let check = s.check(solidcraft_smlib::CheckLevel::All).unwrap();
        assert!(check.is_valid(), "{name}: {check}");
        let got = s.volume(1e-7).unwrap();
        assert!(s.is_manifold_solid().unwrap_or(false) && (got - exact).abs() <= 1e-6 * exact, "{name}: SMLib volume {got}, exact {exact}");
    }
}

/// A body SMLib made (analytic faces) comes back as a record and goes into SMLib again as the
/// same analytic faces: a second boolean on a first one's result.
#[test]
fn smlib_results_go_back_into_smlib() {
    for (name, body, exact) in [
        ("cylinder", cylinder(v(1., 2., 3.), v(0., 0., 1.), 5.0, 10.0).unwrap(), 250.0 * PI),
        ("sphere", sphere(v(0., 0., 0.), 3.0).unwrap(), 36.0 * PI),
    ] {
        let first = super::smlib_body(&body).unwrap();
        let back = super::body_from_smlib(&first).unwrap_or_else(|e| panic!("{name}: {e}"));
        let again = super::smlib_body(&back).unwrap_or_else(|e| panic!("{name}: {e}"));
        let check = again.check(solidcraft_smlib::CheckLevel::All).unwrap();
        assert!(check.is_valid(), "{name}: {check}");
        let got = again.volume(1e-7).unwrap();
        assert!((got - exact).abs() <= 1e-6 * exact, "{name}: SMLib volume {got}, exact {exact}");
    }
}

/// Extruded circles go into SMLib as its cylinder, so a boolean with one intersects a plane with
/// a cylinder, not with a spline. The record holds the wall as the cylinder it is (one face,
/// closed along its seam).
#[test]
fn extruded_circles_go_as_cylinders() {
    use solidcraft_smlib::flat;
    let body = cylinder(v(0., 50., -60.), v(0., 0., 1.), 40.0, 120.0).unwrap();
    let record = super::brep_record(&body).unwrap();
    let walls = record.faces.iter().filter(|f| matches!(f.surface, solidcraft_geom::brep::Surface::Cylinder { .. })).count();
    assert_eq!((walls, record.faces.len()), (1, 3));
    let arrays = solidcraft_smlib::import::arrays(&record, true).unwrap();
    let kinds = &arrays.ints[&flat::FACE_SURFACE_TYPE];
    assert!(kinds.iter().all(|k| *k == i64::from(flat::SURFACE_PLANE) || *k == i64::from(flat::SURFACE_CYLINDER)), "{kinds:?}");
    assert_eq!(kinds.iter().filter(|k| **k == i64::from(flat::SURFACE_CYLINDER)).count(), record.faces.len() - 2);
    // A box cut by it is a valid SMLib solid (as a spline it had edges 0.78 mm off its faces).
    let block = box_solid(v(-14., 0., -14.), v(14., 20., 14.)).unwrap();
    let r = super::smlib_body(&block).unwrap().boolean(&super::smlib_body(&body).unwrap(), solidcraft_smlib::BooleanOp::Subtract).unwrap().unwrap();
    let check = r.check(solidcraft_smlib::CheckLevel::All).unwrap();
    assert!(check.is_valid(), "{check}");
}

/// A cavity cut out of a block: SMLib's result lists the cavity's faces on the void's side with
/// mixed orientations, and every one of them comes back.
#[test]
fn a_cavity_comes_back_from_smlib() {
    let block = box_solid(v(0., 0., 0.), v(40., 40., 40.)).unwrap();
    let cavity = box_solid(v(25., 25., 25.), v(30., 30., 30.)).unwrap();
    let r = super::smlib_body(&block).unwrap().boolean(&super::smlib_body(&cavity).unwrap(), solidcraft_smlib::BooleanOp::Subtract).unwrap().unwrap();
    let back = super::body_from_smlib(&r).unwrap();
    assert_eq!((back.shell_count(), back.face_count()), (2, 12));
    let got = super::smlib_body(&back).unwrap().volume(1e-7).unwrap();
    assert!((got - (64_000.0 - 125.0)).abs() < 1e-6, "{got}");
}

/// A square revolved off the axis through three quarters of a turn: its flat ends are planes
/// square to the axis (annular sectors), its walls cylinders.
#[test]
fn a_revolved_annulus_goes_with_its_flat_faces() {
    use solidcraft_geom::{Loop2, Plane, Region2, Vec2};
    let square =
        Region2 { outer: Loop2::polygon(&[Vec2::new(4., 12.), Vec2::new(12., 12.), Vec2::new(12., 20.), Vec2::new(4., 20.)]), holes: vec![] };
    let plane = Plane { y: v(0., 0., 1.), ..Plane::XZ };
    let body = crate::revolve(&plane, &[square], Vec2::ZERO, Vec2::Y, 1.5 * PI).unwrap().pop().unwrap();
    let s = super::smlib_body(&body).unwrap();
    let check = s.check(solidcraft_smlib::CheckLevel::All).unwrap();
    assert!(check.is_valid(), "{check}");
    let exact = 0.75 * PI * (144.0 - 16.0) * 8.0;
    let got = s.volume(1e-7).unwrap();
    assert!((got - exact).abs() <= 1e-6 * exact, "SMLib volume {got}, exact {exact}");
}
/// A drill point (a cone ending in its apex) through SMLib and back measures as it went in.
#[test]
fn a_cone_apex_comes_back_measuring_as_it_went() {
    use solidcraft_geom::{Loop2, Plane, Region2, Vec2};
    let r = 3.4;
    let h = r / (59f64.to_radians()).tan();
    let profile =
        Region2 { outer: Loop2::polygon(&[Vec2::new(0.0, 3.0 - h), Vec2::new(r, 3.0), Vec2::new(r, 16.0), Vec2::new(0.0, 16.0)]), holes: vec![] };
    let plane = Plane { origin: v(20.0, 15.0, 0.0), x: v(1., 0., 0.), y: v(0., 0., 1.) };
    let tool = crate::revolve(&plane, &[profile], Vec2::ZERO, Vec2::Y, std::f64::consts::TAU).unwrap().pop().unwrap();
    let back = super::body_from_smlib(&super::smlib_body(&tool).unwrap()).unwrap();
    let (m0, m1) = (crate::measure(&tool).unwrap(), crate::measure(&back).unwrap());
    assert!((m1.area - m0.area).abs() < 1e-5 * m0.area, "area {} -> {}", m0.area, m1.area);
    assert!((m1.volume - m0.volume).abs() < 1e-5 * m0.volume, "volume {} -> {}", m0.volume, m1.volume);
}

/// A revolved cap's dome comes back from SMLib as a rational spline closed around the axis,
/// bounded by its rim, its seam both ways and the pole; a ball centred on the dome near the seam
/// cuts a hole in it. Opened at the pole the outer loop spans v in [-1, 0] while the hole
/// projects into [0, 1]: the mesh covered the hole as well (+1694 mm³) until the hole is moved
/// a period to lie inside the outer loop (tangency-labs/solidcore#433).
#[test]
fn a_hole_in_a_dome_by_its_seam_measures_as_smlib_cuts_it() {
    use solidcraft_smlib::sweeps::Curve;
    let a0 = 6.9f64.atan2(26.0);
    let line = |a: [f64; 2], b: [f64; 2]| Curve::Line { start: [a[0], 0.0, a[1]], end: [b[0], 0.0, b[1]] };
    let profile = [
        line([0., 0.], [7., 0.]),
        line([7., 0.], [7., 20.]),
        line([7., 20.], [26., 39.]),
        Curve::Arc { center: [0., 0., 32.1], axis: [0., -1., 0.], reference: [1., 0., 0.], radius: 26.9, start: a0, end: PI / 2.0 },
        line([0., 59.], [0., 0.]),
    ];
    // Turned about -z, as the engine revolves the sketch (its normal is -y): the dome's v runs
    // clockwise seen from above, which puts the hole at v near 1.
    let revolved = solidcraft_smlib::Brep::revolve(&profile, [0.; 3], [0., 0., -1.], std::f64::consts::TAU, true).unwrap();
    let cap = super::smlib_body(&super::body_from_smlib(&revolved).unwrap()).unwrap();
    let ball = super::smlib_body(&sphere(v(17.38629136, 4.658642729, 52.090595805), 4.0).unwrap()).unwrap();
    let cut = cap.tagged(0).unwrap().boolean(&ball.tagged(1).unwrap(), solidcraft_smlib::BooleanOp::Subtract).unwrap().unwrap();
    // The cap less the lens of two spheres, radii 4 and 26.9 at 26.9 apart.
    let (big, small) = (26.9f64, 4.0f64);
    let cap_volume = PI * (49.0 * 20.0 + (26f64.powi(3) - 343.0) / 3.0 + big * big * 20.0 - (big.powi(3) - 6.9f64.powi(3)) / 3.0);
    let lens = PI * small.powi(2) * (8.0 * big * small - 3.0 * small * small) / (12.0 * big);
    let exact = cap_volume - lens;
    let got = cut.volume(1e-7).unwrap();
    assert!((got - exact).abs() <= 1e-6 * exact, "SMLib volume {got}, exact {exact}");
    let back = super::body_from_smlib(&cut).unwrap();
    let m = crate::measure(&back).unwrap();
    assert!((m.volume - exact).abs() <= 2e-4 * exact, "mesh volume {} ({}), exact {exact}", m.volume, m.measured_by);
}
