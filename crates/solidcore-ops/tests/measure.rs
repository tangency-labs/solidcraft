//! The measure family on closed forms: mass properties, area, box, point class, gap samples, and
//! the picks' distances and normals.
#![allow(clippy::unwrap_used)]

use std::f64::consts::PI;

use solidcore_ops::session;
use solidcore_ops::{make_box, make_sphere, make_torus, PointClass};

fn all_fixes() {
    session::set_fixes(solidcore::kernel::UpstreamFixes::NAMES.iter().copied()).unwrap();
}

fn near(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol * b.abs().max(1.0)
}

#[test]
fn a_box_mass_properties_are_its_closed_form() {
    all_fixes();
    let b = make_box([1.0, -2.0, 3.0], [1.0, 2.0, 3.0]).unwrap();
    let m = b.mass_properties(1e-8).unwrap();
    assert_eq!(m.requested_accuracy, 1e-8);
    assert!(
        near(m.volume, 6.0, 1e-9) && near(m.area, 22.0, 1e-9),
        "{m:?}"
    );
    for (c, e) in m.centroid.iter().zip([1.5, -1.0, 4.5]) {
        assert!(near(*c, e, 1e-9), "{m:?}");
    }
    for (i, e) in m.moments.iter().zip([6.5, 5.0, 2.5]) {
        assert!(near(*i, e, 1e-8), "{m:?}");
    }
    assert!(m.products.iter().all(|p| p.abs() <= 1e-8), "{m:?}");
    assert_eq!(m.inertia_tensor()[0][0], m.moments[0]);
}

#[test]
fn a_sphere_mass_properties_and_area() {
    all_fixes();
    let s = make_sphere([1.0, 2.0, 3.0], 2.0).unwrap();
    let m = s.mass_properties(1e-8).unwrap();
    let v = 4.0 / 3.0 * PI * 8.0;
    assert!(
        near(m.volume, v, 1e-6) && near(m.area, 16.0 * PI, 1e-6),
        "{m:?}"
    );
    for i in m.moments {
        assert!(near(i, 0.4 * v * 4.0, 1e-6), "{m:?}");
    }
    let a = s.area(1e-8).unwrap();
    assert!(near(a, 16.0 * PI, 1e-6), "{a}");
    assert!(s.area(f64::NAN).is_err() && s.area(0.0).is_err() && s.mass_properties(-1.0).is_err());
}

#[test]
fn a_box_bounds_class_and_counts() {
    all_fixes();
    let b = make_box([0.0, 0.0, 0.0], [1.0, 2.0, 3.0]).unwrap();
    assert_eq!(b.bounds().unwrap(), ([0.0, 0.0, 0.0], [1.0, 2.0, 3.0]));
    assert_eq!(
        b.classify_point([0.5, 1.0, 1.5]).unwrap(),
        PointClass::Inside
    );
    assert_eq!(
        b.classify_point([5.0, 1.0, 1.5]).unwrap(),
        PointClass::Outside
    );
    assert_eq!(
        b.classify_point([1.0, 1.0, 1.5]).unwrap(),
        PointClass::OnBoundary
    );
    assert!(b.classify_point([f64::INFINITY, 0.0, 0.0]).is_err());
    assert!(b.is_manifold_solid().unwrap());
    assert_eq!(b.solid_count().unwrap(), 1);
}

#[test]
fn a_torus_area_is_its_closed_form() {
    all_fixes();
    let t = make_torus([0.0, 0.0, 0.0], 4.0, 1.0).unwrap();
    let a = t.area(1e-8).unwrap();
    assert!(near(a, 16.0 * PI * PI, 1e-6), "{a}");
}
