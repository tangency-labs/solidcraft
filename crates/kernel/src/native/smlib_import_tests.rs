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

/// Truck holds spheres and tori as revolutions (`PlacedRevolution`), which the SMLib import does
/// not carry yet: a clear error, never a wrong body.
#[test]
fn revolutions_are_refused_for_now() {
    for (name, body) in [("sphere", sphere(v(0., 0., 0.), 5.0).unwrap()), ("torus", torus(v(0., 0., 0.), 10.0, 2.0).unwrap())] {
        let e = super::smlib_body(&body).err().map(|e| e.to_string()).unwrap_or_default();
        assert!(e.contains("not supported yet: revolved surfaces"), "{name}: {e}");
    }
}
