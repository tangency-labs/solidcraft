//! Truck bodies into SMLib through their schema 4 records: the kernel's primitives keep their
//! analytic volume. Truck holds cylinders, spheres and tori as rational B-splines, which SMLib
//! integrates to a few 1e-7 (truck's own mesh measure is 1e-4 off).
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

/// A body SMLib made (analytic faces) comes back from truck as revolutions, and goes into SMLib
/// again as the same analytic faces: a second boolean on a first one's result.
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
