//! tangency-labs/solidcore#2192: a curve sweep along a closed path capped both ends,
//! so the two caps coincided where the end met the start. The body had a second region
//! and cutting it from the torus it lay inside came out empty. SolidCraft #2005 (fixed
//! there in tangency-labs/solidcraft#2010); the cases are
//! `fixtures/solidcraft/2005-closed-sweep-cuts-torus.json`.
//!
//! An 8 mm square in the XZ plane, starting at the middle of its inner side, swept with
//! the follow frame along the full circle of radius 30 about Z. The sweep puts that
//! first point on the path start (30, 0, 0), so the square spans x 26..34, z 0..8.
//! With `curve_sweep_closed_path_joins_ends` the sweep leaves the caps off and the
//! stitch joins the ends: one closed tube of 64 * 2 pi * 30 (Pappus). The torus around
//! it (R 30, r 8, centre circle at z = 4) less the tube is (64 pi - 64) * 2 pi * 30.
//! The pinned default still caps both ends (two regions), as upstream does.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use std::f64::consts::PI;

use solidcore::boolean::api_brep::boolean_difference;
use solidcore::curve::curve::Curve;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::vector3d::{Point3, Vec3};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::primitive::api_curves as curves;
use solidcore::primitive::primitive_creation::SweepOptions;
use solidcore::sweep::api_brep as sweeps;
use solidcore::topology::brep::p1::regions;
use solidcore::topology::proto_topology::p1::api_transform::{
    api_rotate, api_translate, TransformObject,
};
use solidcore::{Brep, Kernel};

/// The square tube swept a full turn round the circle of radius 30 about Z.
fn closed_tube(k: &mut Kernel) -> Brep {
    let corners = [
        Point3::new(0.0, 0.0, -4.0),
        Point3::new(4.0, 0.0, -4.0),
        Point3::new(4.0, 0.0, 4.0),
        Point3::new(-4.0, 0.0, 4.0),
        Point3::new(-4.0, 0.0, -4.0),
    ];
    let profile: Vec<Curve> = corners
        .iter()
        .zip(corners.iter().cycle().skip(1))
        .map(|(a, b)| Curve::from(curves::create_line_segment(*a, *b).unwrap()))
        .collect();
    let path =
        Curve::from(curves::create_arc(Point3::new(0.0, 0.0, 0.0), 30.0, 0.0, 360.0).unwrap());
    let refs: Vec<Option<&Curve>> = profile.iter().map(Some).collect();
    // the difftest runner's options: the profile turns with the path's frame
    let options = SweepOptions {
        cap_ends: true,
        translational_sweep: false,
        move_profile_center_to_path: false,
        orient_profile_perpendicular_to_path: false,
        move_to_profile: false,
        profile_at_path_start: false,
    };
    sweeps::create_curve_sweep(k, &refs, Some(&path), None, None, Some(&options))
        .unwrap()
        .brep
}

/// The torus of major radius 30 and tube radius 8 about Z, its centre circle at z = 4.
fn torus(k: &mut Kernel) -> Brep {
    let mut circle =
        Curve::from(curves::create_analytic_circle(k, Point3::new(0.0, 0.0, 0.0), 8.0).unwrap());
    // into the XZ plane, then out to (30, 0, 4)
    api_rotate(
        k,
        Some(TransformObject::Curve(&mut circle)),
        Vec3::new(0.0, 0.0, 0.0),
        Vec3::X,
        90.0,
    )
    .unwrap();
    api_translate(
        k,
        Some(TransformObject::Curve(&mut circle)),
        Point3::new(30.0, 0.0, 4.0),
    )
    .unwrap();
    sweeps::create_rotational_sweep(
        k,
        &[Some(&circle)],
        Point3::new(0.0, 0.0, 0.0),
        Vec3::new(0.0, 0.0, 1.0),
        360.0,
        true,
    )
    .unwrap()
}

fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let key_of = |_| ObjectKey(0);
    let attributes = PropertyAttributes {
        store: &store,
        key_of: &key_of,
    };
    brep_volume(k, b, 1e-8, &attributes).unwrap()
}

/// Bounded regions (the infinite region left out).
fn bounded_regions(b: &Brep) -> usize {
    let infinite = b.infinite_region();
    regions(b, None)
        .unwrap()
        .into_iter()
        .filter(|r| Some(*r) != infinite)
        .count()
}

#[test]
fn closed_sweep_joins_its_end_to_its_start() {
    let mut k = Kernel::default();
    k.fixes.curve_sweep_closed_path_joins_ends = true;
    let mut tube = closed_tube(&mut k);
    assert_eq!(bounded_regions(&tube), 1);
    // five walls and no caps, closed on itself: V - E + F = 0
    assert_eq!(
        [
            tube.face_list().len(),
            tube.edge_list().len(),
            tube.vertex_list().len()
        ],
        [5, 10, 5]
    );
    let v = volume(&k, &mut tube);
    let want = 64.0 * 2.0 * PI * 30.0;
    assert!((v - want).abs() < 0.01, "volume {v}, expected {want}");
}

#[test]
fn closed_sweep_cuts_the_torus_around_it() {
    let mut k = Kernel::default();
    k.fixes.curve_sweep_closed_path_joins_ends = true;
    let body = torus(&mut k);
    let tool = closed_tube(&mut k);
    let mut cut = boolean_difference(&mut k, body, tool).unwrap();
    let v = volume(&k, &mut cut);
    let want = (64.0 * PI - 64.0) * 2.0 * PI * 30.0;
    assert!((v - want).abs() < 0.01, "volume {v}, expected {want}");
}

#[test]
fn default_caps_both_ends_as_upstream() {
    let mut k = Kernel::default();
    let tube = closed_tube(&mut k);
    assert_eq!(bounded_regions(&tube), 2);
}
