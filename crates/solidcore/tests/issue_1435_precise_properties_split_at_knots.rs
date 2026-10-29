//! #1435 (WR-494): with `UpstreamFixes::precise_properties_split_at_surface_knots`
//! (the SolidCraft fork's tangency-labs/smlib@08d9a14) `SmFace::ComputePreciseProperties`
//! splits both integration levels at the surface's knots, so curved primitives' areas and
//! inertia hold 1e-9 of their closed forms at a requested accuracy of 1e-8.
#![allow(clippy::unwrap_used, clippy::indexing_slicing)]
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::mass::integrator::properties::{
    brep_area, brep_mass_properties, face_area, PropertyAttributes,
};
use solidcore::math::Vec3;
use solidcore::primitive::api_primitives::{
    create_cone, create_cylinder, create_sphere, create_torus,
};
use solidcore::topology::brep::p1::faces;
use solidcore::Kernel;
use std::f64::consts::PI;

const ACCURACY: f64 = 1e-8;
const REL: f64 = 1e-9;

fn fixed() -> Kernel {
    let mut k = Kernel::default();
    k.fixes.precise_properties_split_at_surface_knots = true;
    k
}

fn assert_rel(name: &str, got: f64, exact: f64) {
    let err = (got - exact).abs() / exact.abs();
    assert!(err <= REL, "{name}: {got} vs {exact} ({err:e} relative)");
}

#[test]
fn flag_is_off_by_default() {
    assert!(
        !Kernel::default()
            .fixes
            .precise_properties_split_at_surface_knots
    );
}

/// The issue's repro: the cylindrical face of `create_cylinder`.
#[test]
fn cylinder_side_face_area_holds_its_accuracy() {
    let mut k = fixed();
    let mut b = create_cylinder(&mut k, Vec3::new(1.0, 2.0, -3.0), 5.0, 12.0).unwrap();
    let side = faces(&b, None).unwrap()[0];
    let a = face_area(&k, &mut b, side, ACCURACY).unwrap();
    assert_rel("cylinder side", a, 2.0 * PI * 5.0 * 12.0);
}

/// The issue's table: the whole brep's area of each curved primitive.
#[test]
fn curved_primitive_areas_hold_their_accuracy() {
    let mut k = fixed();
    let origin = Vec3::new(1.0, 2.0, -3.0);
    let s_cone = (36.0_f64 + 81.0).sqrt();
    let s_frustum = (16.0_f64 + 81.0).sqrt();
    let cases: [(&str, solidcore::topology::Brep, f64); 5] = [
        (
            "cylinder r5 h12",
            create_cylinder(&mut k, origin, 5.0, 12.0).unwrap(),
            2.0 * PI * 25.0 + 2.0 * PI * 5.0 * 12.0,
        ),
        (
            "sphere r7",
            create_sphere(&mut k, origin, 7.0).unwrap(),
            4.0 * PI * 49.0,
        ),
        (
            "cone r6 h9",
            create_cone(&mut k, origin, 6.0, 0.0, 9.0).unwrap(),
            PI * (36.0 + 6.0 * s_cone),
        ),
        (
            "frustum r6..2 h9",
            create_cone(&mut k, origin, 6.0, 2.0, 9.0).unwrap(),
            PI * (36.0 + 4.0 + 8.0 * s_frustum),
        ),
        (
            "torus R10 r3",
            create_torus(&mut k, origin, 10.0, 3.0).unwrap(),
            4.0 * PI * PI * 30.0,
        ),
    ];
    for (name, mut b, exact) in cases {
        let a = brep_area(&k, &mut b, ACCURACY).unwrap();
        assert_rel(name, a, exact);
    }
}

/// The issue's `brep_properties` case: a torus R10 r3 at (1, -1, 0.5); its axial
/// inertia about the centre against V (R^2/2 + 5 r^2/8).
#[test]
fn torus_mass_properties_hold_their_accuracy() {
    let mut k = fixed();
    let centre = Vec3::new(1.0, -1.0, 0.5);
    let (rr, r) = (10.0, 3.0);
    let mut b = create_torus(&mut k, centre, rr, r).unwrap();
    let store = AttributeStore::default();
    let key_of = |_| ObjectKey(0);
    let attributes = PropertyAttributes {
        store: &store,
        key_of: &key_of,
    };
    let m = brep_mass_properties(&k, &mut b, ACCURACY, 1.0, centre, &attributes).unwrap();
    let volume = 2.0 * PI * PI * rr * r * r;
    assert_rel("torus area", m.properties.area, 4.0 * PI * PI * rr * r);
    assert_rel("torus volume", m.properties.volume, volume);
    let ixx = volume * (rr * rr / 2.0 + 5.0 * r * r / 8.0);
    let izz = volume * (rr * rr + 3.0 * r * r / 4.0);
    assert_rel("torus Ixx", m.inertia.x, ixx);
    assert_rel("torus Iyy", m.inertia.y, ixx);
    assert_rel("torus Izz", m.inertia.z, izz);
}

/// Flag off (upstream v1.0.1, the default): the issue's repro misses the accuracy it
/// asks for, as the C++ reference does: the cylinder's side face 5.2e-8 off 2πrh.
#[test]
fn upstream_cylinder_side_face_area_misses_its_accuracy() {
    let mut k = Kernel::default();
    let mut b = create_cylinder(&mut k, Vec3::new(1.0, 2.0, -3.0), 5.0, 12.0).unwrap();
    let side = faces(&b, None).unwrap()[0];
    let a = face_area(&k, &mut b, side, ACCURACY).unwrap();
    let exact = 2.0 * PI * 5.0 * 12.0;
    let err = (a - exact).abs() / exact;
    assert!(err > ACCURACY, "{a} vs {exact} ({err:e} relative)");
}

/// Flag off: the issue's torus inertia, 1.7e-9 off V (R^2/2 + 5 r^2/8).
#[test]
fn upstream_torus_inertia_misses_1e_9() {
    let mut k = Kernel::default();
    let centre = Vec3::new(1.0, -1.0, 0.5);
    let (rr, r) = (10.0, 3.0);
    let mut b = create_torus(&mut k, centre, rr, r).unwrap();
    let store = AttributeStore::default();
    let key_of = |_| ObjectKey(0);
    let attributes = PropertyAttributes {
        store: &store,
        key_of: &key_of,
    };
    let m = brep_mass_properties(&k, &mut b, ACCURACY, 1.0, centre, &attributes).unwrap();
    let volume = 2.0 * PI * PI * rr * r * r;
    let ixx = volume * (rr * rr / 2.0 + 5.0 * r * r / 8.0);
    let err = (m.inertia.x - ixx).abs() / ixx;
    assert!(err > REL, "{} vs {ixx} ({err:e} relative)", m.inertia.x);
}
