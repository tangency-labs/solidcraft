//! Regression test for WR-407 (tangency-labs/solidcore#434, the fork's
//! tangency-labs/smlib@1b6f5fa, SolidCraft kernel case sweeps/844): two walls 12 thick
//! and 60 tall swept along half circles (wall A r 44..56 over 0..180 degrees, wall B
//! r 52..64 over 90..270) overlap over a quarter turn and share the planes z = 0 and
//! z = 60, and are joined.
//!
//! With the sweep's frame as upstream builds it (`sweep_frame_on_planar_path_normal`
//! off), wall A's flat top is a B-spline 2.8e-5 off its plane, so it is not planar and
//! its search domain reaches past the fold of its continuation. Upstream's
//! `CoverCoincidentSurface` merges wall B's top onto that folded continuation and the
//! join loses 5573 mm3. With `cover_refuses_folded_continuation` the cover across the
//! fold is refused and the join has the closed-form volume
//! `V_A + V_B - overlap = 60 (pi/2)(56^2 - 44^2) + 60 (pi/2)(64^2 - 52^2) - 60 (pi/4)(56^2 - 52^2)`.
//! The walls' radial faces are the sweep's approximation of the arcs, so the volume is
//! held to 1 mm3 (the SolidCraft case's tolerance).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::boolean::api_brep::boolean_union;
use solidcore::curve::curve::Curve;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::{Point3, Vec3, PI};
use solidcore::primitive::api_curves::{create_arc, create_line_segment};
use solidcore::primitive::primitive_creation::SweepOptions;
use solidcore::query::api_queries::brep_is_manifold_solid;
use solidcore::sweep::api_brep::create_curve_sweep;
use solidcore::topology::proto_topology::p1::api_transform::{api_rotate, TransformObject};
use solidcore::{Brep, Kernel};

/// The profile (u across the wall, v up), turned into the plane `turn` (axis, degrees)
/// maps the XY plane to.
fn profile(k: &mut Kernel, turn: (Vec3, f64)) -> Vec<Curve> {
    let pts = [
        [0.0, 0.0],
        [6.0, 0.0],
        [6.0, 60.0],
        [-6.0, 60.0],
        [-6.0, 0.0],
    ];
    let at = |[u, v]: [f64; 2]| Point3::new(u, v, 0.0);
    let mut out: Vec<Curve> = pts
        .iter()
        .zip(pts.iter().cycle().skip(1))
        .map(|(a, b)| Curve::from(create_line_segment(at(*a), at(*b)).unwrap()))
        .collect();
    for c in &mut out {
        api_rotate(
            k,
            Some(TransformObject::Curve(c)),
            Point3::ZERO,
            turn.0,
            turn.1,
        )
        .unwrap();
    }
    out
}

/// The profile in `turn`'s plane swept along the XY arc of `radius` from `start` to
/// `end` degrees, the profile turning with the path's frame (`SMLIB_SWEEP_FOLLOW`).
fn wall(k: &mut Kernel, turn: (Vec3, f64), radius: f64, start: f64, end: f64) -> Brep {
    let curves = profile(k, turn);
    let refs: Vec<Option<&Curve>> = curves.iter().map(Some).collect();
    let path = Curve::from(create_arc(Point3::ZERO, radius, start, end).unwrap());
    let options = SweepOptions {
        cap_ends: true,
        translational_sweep: false,
        move_profile_center_to_path: false,
        orient_profile_perpendicular_to_path: false,
        move_to_profile: false,
        profile_at_path_start: false,
    };
    create_curve_sweep(k, &refs, Some(&path), None, None, Some(&options))
        .unwrap()
        .brep
}

/// The two walls joined, with WR-407's fix `fixed`.
fn joined(fixed: bool) -> (Kernel, Brep) {
    let mut k = Kernel::default();
    k.fixes.cover_refuses_folded_continuation = fixed;
    // XZ is XY turned 90 degrees about X; YZ is XY turned 120 degrees about (1, 1, 1).
    let a = wall(&mut k, (Vec3::X, 90.0), 50.0, 0.0, 180.0);
    let b = wall(&mut k, (Vec3::new(1.0, 1.0, 1.0), 120.0), 58.0, 90.0, 270.0);
    let joined = boolean_union(&mut k, a, b).unwrap();
    (k, joined)
}

fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-8, &attrs).unwrap()
}

/// `V_A + V_B - overlap`.
fn closed_form() -> f64 {
    let ring = |r0: f64, r1: f64| r1 * r1 - r0 * r0;
    60.0 * (PI / 2.0) * ring(44.0, 56.0) + 60.0 * (PI / 2.0) * ring(52.0, 64.0)
        - 60.0 * (PI / 4.0) * ring(52.0, 56.0)
}

#[test]
fn wr_407_walls_join_to_the_closed_form_with_the_fix() {
    let (k, mut b) = joined(true);
    assert!(brep_is_manifold_solid(&b).unwrap());
    let v = volume(&k, &mut b);
    assert!(
        (v - closed_form()).abs() < 1.0,
        "{v} against {}",
        closed_form()
    );
}

/// Upstream merges onto the folded continuation: the join is short by 5573 mm3.
#[test]
fn wr_407_default_join_loses_the_folded_cover() {
    let (k, mut b) = joined(false);
    let v = volume(&k, &mut b);
    assert!(closed_form() - v > 1000.0, "{v} against {}", closed_form());
}
