//! tangency-labs/solidcore#379 (WR-421): `SmApiChamferFillet` with a constant distance on a
//! box minus a through cylinder left the hole's rims open. The issue expects a closed solid
//! whose chamfer faces join the top and bottom faces to the hole's wall. With
//! `Kernel::fixes.fillet_chamfer_cone_on_arc` the rims' linear sections are revolved into
//! cones, as the fork's tangency-labs/smlib@f9299b7 does.
//!
//! The case is the issue's: every edge of a 10 cube with a radius 2 through hole, chamfered
//! by a chord of 1/sqrt(2) (legs 0.5).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::blend::fillet_executive::api::chamfer_fillet;
use solidcore::boolean::api_brep::boolean_difference;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::{Point3, PI};
use solidcore::primitive::api_primitives::{create_box, create_cylinder};
use solidcore::surface::surface::p2::SURF_OF_REVOLUTION_TYPE;
use solidcore::topology::brep::p1;
use solidcore::topology::brep::p2::is_manifold_solid;
use solidcore::{Brep, Kernel, Result};

fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-6, &attrs).unwrap()
}

/// The holed cube before the chamfer, the chamfer's status and the body after it.
fn chamfered(fix: bool) -> (f64, Result<()>, Kernel, Brep) {
    let mut k = Kernel::default();
    k.fixes.fillet_chamfer_cone_on_arc = fix;
    let cube = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 10.0, 10.0, 10.0).unwrap();
    let hole = create_cylinder(&mut k, Point3::new(5.0, 5.0, -1.0), 2.0, 12.0).unwrap();
    let mut body = boolean_difference(&mut k, cube, hole).unwrap();
    let before = volume(&k, &mut body);
    let r = chamfer_fillet(&mut k, &mut body, std::f64::consts::FRAC_1_SQRT_2);
    (before, r, k, body)
}

/// The number of the body's faces on a surface of revolution (the rims' chamfers with the
/// fix; upstream builds them as B-splines).
fn revolved_faces(b: &Brep) -> usize {
    p1::faces(b, None)
        .unwrap()
        .into_iter()
        .filter(|&f| {
            b.face(f)
                .unwrap()
                .surface
                .as_ref()
                .is_some_and(|s| s.type_code() == SURF_OF_REVOLUTION_TYPE)
        })
        .count()
}

/// The volume the cube's own chamfers remove: its 12 edges lose a triangle of legs `a`
/// along 10 - 2a, and each of its 8 corners three quarters of an `a` cube.
fn cube_chamfers(a: f64) -> f64 {
    12.0 * (10.0 - 2.0 * a) * a * a / 2.0 + 8.0 * 0.75 * a * a * a
}

#[test]
fn issue_379_flag_is_off_by_default() {
    assert!(!Kernel::default().fixes.fillet_chamfer_cone_on_arc);
}

#[test]
fn issue_379_the_rims_are_chamfered_into_a_closed_solid_with_the_fix() {
    let (before, r, k, mut body) = chamfered(true);
    r.unwrap();
    solidcore::topology::integrity::check(&body).unwrap();
    assert!(is_manifold_solid(&body).unwrap(), "not a closed solid");
    // one revolved chamfer joining each of the top and bottom faces to the hole's wall
    assert_eq!(revolved_faces(&body), 2);
    // each rim (radius 2) loses the triangle of legs a (centroid at 2 + a / 3) turned once
    // about the axis
    let a = 0.5;
    let rims = 2.0 * 2.0 * PI * (2.0 + a / 3.0) * a * a / 2.0;
    let removed = before - volume(&k, &mut body);
    let want = cube_chamfers(a) + rims;
    assert!(
        (removed - want).abs() <= 1e-6 * before,
        "{removed} vs {want}"
    );
}

#[test]
fn issue_379_upstream_leaves_the_rims_open() {
    let (before, r, k, mut body) = chamfered(false);
    // upstream reports success, chamfers the cube's own edges, and leaves a body that is
    // not a closed solid
    r.unwrap();
    assert!(
        !is_manifold_solid(&body).unwrap(),
        "upstream gave a closed solid"
    );
    assert_eq!(revolved_faces(&body), 0);
    let removed = before - volume(&k, &mut body);
    assert!(
        (removed - cube_chamfers(0.5)).abs() <= 1e-6 * before,
        "{removed}"
    );
}
