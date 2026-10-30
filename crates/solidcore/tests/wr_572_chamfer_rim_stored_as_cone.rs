//! Regression test for WR-572 (tangency-labs/solidcore#1503; the fork's
//! tangency-labs/smlib@b5743ec, capi test `chamfered_hole_rim_is_a_cone`): with
//! `fillet_chamfer_cone_on_arc` (WR-421) `CalcTorusFilletGeom` revolves a chamfer's
//! linear section about the rim's axis, but keeps the revolution as an
//! `SmSurfOfRevolution`, which reads back as a rational B-spline, not the cone it is.
//! With `fillet_chamfer_cone_stored_as_cone` the revolution's NURBS form goes through
//! `CopyAndAddAnalytics` and the cone it gives is stored, the rails made on it.
//!
//! The body is #379's: a 10 cube with a radius 2 through hole, every edge chamfered by a
//! chord of 1/sqrt(2) (legs 0.5). Each rim's chamfer is the cone between the circles of
//! radius 2.5 on the face and radius 2 half a unit in.
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
use solidcore::surface::surface::SurfaceKind;
use solidcore::topology::brep::p1;
use solidcore::topology::brep::p2::is_manifold_solid;
use solidcore::{Brep, Kernel};

fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-6, &attrs).unwrap()
}

/// The holed cube chamfered with WR-421 on and WR-572 as given: the volume before, the
/// kernel and the body.
fn chamfered(stored_as_cone: bool) -> (f64, Kernel, Brep) {
    let mut k = Kernel::default();
    k.fixes.fillet_chamfer_cone_on_arc = true;
    k.fixes.fillet_chamfer_cone_stored_as_cone = stored_as_cone;
    let cube = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 10.0, 10.0, 10.0).unwrap();
    let hole = create_cylinder(&mut k, Point3::new(5.0, 5.0, -1.0), 2.0, 12.0).unwrap();
    let mut body = boolean_difference(&mut k, cube, hole).unwrap();
    let before = volume(&k, &mut body);
    chamfer_fillet(&mut k, &mut body, std::f64::consts::FRAC_1_SQRT_2).unwrap();
    (before, k, body)
}

/// The body's faces on a surface of revolution, and on a cone that is no cylinder (its
/// radii differ).
fn revolved_and_cones(b: &Brep) -> (usize, usize) {
    let mut revolved = 0;
    let mut cones = 0;
    for f in p1::faces(b, None).unwrap() {
        let Some(s) = b.face(f).unwrap().surface.as_ref() else {
            continue;
        };
        if s.type_code() == SURF_OF_REVOLUTION_TYPE {
            revolved += 1;
        }
        if let SurfaceKind::Cone(c) = s.kind() {
            if (c.bot_radius() - c.top_radius()).abs() > 0.1 {
                cones += 1;
            }
        }
    }
    (revolved, cones)
}

/// The volume the chamfers remove: the cube's 12 edges lose a triangle of legs `a` along
/// 10 - 2a and its 8 corners three quarters of an `a` cube; each rim (radius 2) loses
/// the triangle of legs `a` (centroid at 2 + a / 3) turned once about the axis.
fn removed_want(a: f64) -> f64 {
    let cube = 12.0 * (10.0 - 2.0 * a) * a * a / 2.0 + 8.0 * 0.75 * a * a * a;
    let rims = 2.0 * 2.0 * PI * (2.0 + a / 3.0) * a * a / 2.0;
    cube + rims
}

#[test]
fn wr_572_flag_is_off_by_default() {
    assert!(!Kernel::default().fixes.fillet_chamfer_cone_stored_as_cone);
}

#[test]
fn wr_572_rim_chamfers_are_stored_as_cones() {
    let (before, k, mut body) = chamfered(true);
    solidcore::topology::integrity::check(&body).unwrap();
    assert!(is_manifold_solid(&body).unwrap(), "not a closed solid");
    assert_eq!(revolved_and_cones(&body), (0, 2));
    let removed = before - volume(&k, &mut body);
    let want = removed_want(0.5);
    assert!(
        (removed - want).abs() <= 1e-6 * before,
        "{removed} vs {want}"
    );
}

#[test]
fn wr_572_without_the_opt_in_the_rims_stay_revolutions() {
    let (_, _, body) = chamfered(false);
    assert_eq!(revolved_and_cones(&body), (2, 0));
}
