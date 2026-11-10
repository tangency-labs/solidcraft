//! tangency-labs/solidcore#1450 (WR-522, WR-523; the SolidCraft fork's tangency-labs/smlib
//! 2dd71a0 and 5d3939e): `SmApiCreateCurveSweep` moves the profile's reference point (its
//! first curve's start) onto the path's start before sweeping, so a profile that lies round
//! the path, as a sketch profile does, is swept off its place: round a bend at the wrong
//! radius. `SweepOptions::profile_at_path_start` (off by default, upstream has no such
//! option) makes the path's start the reference point, so the profile keeps its place, and
//! `create_curve_sweep_with_placements` reports the rigid motions carrying the profile onto
//! the first, middle and last sections.
//!
//! The case is the issue's: a disc of radius 1 centred on the path's start, square to it,
//! swept along 10 along x and then a quarter arc of radius 10 turning to +y.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers
use solidcore::curve::curve::Curve;
use solidcore::curve::ellipse::Ellipse;
use solidcore::curve::line::Line;
use solidcore::error::SolidError;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::extent1d::Extent1d;
use solidcore::kernel::{Kernel, KernelOptions};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::primitive::primitive_creation::SweepOptions;
use solidcore::sweep::api_brep::{
    create_curve_sweep, create_curve_sweep_with_placements, CurveSweep,
};
use solidcore::Vec3;

/// `pi r^2 (10 + 10 pi / 2)`: the tube of radius 1 round the 10 + 5 pi long path.
const TUBE_VOLUME: f64 = 80.763_948_54;

/// The disc profile and the line-then-arc path of the issue.
fn disc_and_path(k: &Kernel) -> (Curve, Curve) {
    let disc = Axis2Placement::new(
        Vec3::new(0.0, 0.0, 0.0),
        Vec3::new(0.0, 1.0, 0.0),
        Vec3::new(0.0, 0.0, 1.0),
    );
    let circle = Curve::from(Ellipse::create_canonical_circle(k, &disc, 1.0, None, None).unwrap());
    let line =
        Line::create_line_segment(3, Vec3::new(0.0, 0.0, 0.0), Vec3::new(10.0, 0.0, 0.0), None)
            .unwrap();
    let arc_frame = Axis2Placement::new(
        Vec3::new(10.0, 10.0, 0.0),
        Vec3::new(0.0, -1.0, 0.0),
        Vec3::new(1.0, 0.0, 0.0),
    );
    let arc = Ellipse::create_canonical_circle(
        k,
        &arc_frame,
        10.0,
        Some(&Extent1d::from_min_max(0.0, 90.0)),
        None,
    )
    .unwrap();
    let mut parts = [
        line.base_with_nurb().unwrap().into_owned(),
        arc.base().clone(),
    ];
    let path = Curve::from(BSplineCurve::create_by_joining(&mut parts, None).unwrap());
    (circle, path)
}

fn options(profile_at_path_start: bool) -> SweepOptions {
    SweepOptions {
        cap_ends: true,
        profile_at_path_start,
        ..SweepOptions::default()
    }
}

fn volume(k: &Kernel, s: &mut CurveSweep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, &mut s.brep, 1e-6, &attrs).unwrap()
}

fn sweep(profile_at_path_start: bool) -> (Kernel, CurveSweep) {
    let mut k = Kernel::new(&KernelOptions::default());
    let (circle, path) = disc_and_path(&k);
    let s = create_curve_sweep(
        &mut k,
        &[Some(&circle)],
        Some(&path),
        None,
        None,
        Some(&options(profile_at_path_start)),
    )
    .unwrap();
    (k, s)
}

fn assert_near(a: Vec3, b: Vec3, tol: f64, what: &str) {
    assert!(
        a.distance(b) <= tol,
        "{what}: {a:?} is not {b:?} (gap {})",
        a.distance(b)
    );
}

/// Upstream (the option off): the disc's first point, on its rim, is moved onto the path's
/// start, so the tube runs 1 off the path and round the bend at radius 11 or 9.
#[test]
fn default_sweep_moves_the_profile_rim_onto_the_path() {
    let (k, mut s) = sweep(false);
    let v = volume(&k, &mut s);
    assert!(
        (v - TUBE_VOLUME).abs() > 1.0,
        "upstream's off-centre tube, volume {v}"
    );
    assert!((v - 85.698_749_05).abs() < 1e-3 * v, "volume {v}");
}

/// The option on: the disc keeps its place, the tube is centred on the path (Pappus).
#[test]
fn profile_at_path_start_sweeps_the_profile_where_it_lies() {
    let (k, mut s) = sweep(true);
    let v = volume(&k, &mut s);
    // within the walls' fit to the exact tube
    assert!(
        (v - TUBE_VOLUME).abs() < 1e-4 * TUBE_VOLUME,
        "volume {v}, tube {TUBE_VOLUME}"
    );
}

/// The placements: with the option on the first motion is the identity and the last the
/// quarter turn about the arc's axis carrying the path's start onto its end.
#[test]
fn placements_report_the_profile_motions() {
    let mut k = Kernel::new(&KernelOptions::default());
    let (circle, path) = disc_and_path(&k);
    let placed = create_curve_sweep_with_placements(
        &mut k,
        &[Some(&circle)],
        Some(&path),
        None,
        None,
        Some(&options(true)),
    )
    .unwrap();
    let [first, middle, last] = placed.placements;
    assert_near(first.origin(), Vec3::ZERO, 1e-9, "first origin");
    assert_near(first.x_axis(), Vec3::X, 1e-9, "first x");
    assert_near(first.y_axis(), Vec3::Y, 1e-9, "first y");
    assert_near(
        last.origin(),
        Vec3::new(20.0, 10.0, 0.0),
        1e-6,
        "last origin",
    );
    assert_near(last.x_axis(), Vec3::Y, 1e-6, "last x");
    assert_near(last.y_axis(), -Vec3::X, 1e-6, "last y");
    // the middle section lies on the path, square to it, the disc round it
    let on_path = middle.origin();
    let on_line =
        on_path.y.abs() < 1e-9 && on_path.z.abs() < 1e-9 && (0.0..=10.0).contains(&on_path.x);
    let on_arc = ((on_path - Vec3::new(10.0, 10.0, 0.0)).length() - 10.0).abs() < 1e-6;
    assert!(on_line || on_arc, "middle origin {on_path:?}");
    // a rim point of the disc lands at distance 1 from the section's centre
    let rim = middle.transform_point(Vec3::new(0.0, 1.0, 0.0));
    assert!((rim.distance(on_path) - 1.0).abs() < 1e-9);
    // the solid is the one create_curve_sweep makes
    let (k2, mut plain) = sweep(true);
    let mut with = placed.sweep;
    assert_eq!(volume(&k, &mut with), volume(&k2, &mut plain));

    // the option off: the first motion carries the disc's first (rim) point onto the
    // path's start
    let off = create_curve_sweep_with_placements(
        &mut k,
        &[Some(&circle)],
        Some(&path),
        None,
        None,
        Some(&options(false)),
    )
    .unwrap();
    let rim = off.placements[0].transform_point(Vec3::new(0.0, 1.0, 0.0));
    assert_near(rim, Vec3::ZERO, 1e-9, "upstream's first motion");
}

/// A translational sweep's placements are the path's points at its start, middle and end.
#[test]
fn translational_placements_are_the_path_points() {
    let mut k = Kernel::new(&KernelOptions::default());
    let (circle, path) = disc_and_path(&k);
    let opts = SweepOptions {
        translational_sweep: true,
        ..options(true)
    };
    let placed = create_curve_sweep_with_placements(
        &mut k,
        &[Some(&circle)],
        Some(&path),
        None,
        None,
        Some(&opts),
    )
    .unwrap();
    assert_near(placed.placements[0].origin(), Vec3::ZERO, 1e-9, "start");
    assert_near(
        placed.placements[2].origin(),
        Vec3::new(20.0, 10.0, 0.0),
        1e-9,
        "end",
    );
    for p in placed.placements {
        assert_near(p.x_axis(), Vec3::X, 1e-12, "unturned x");
        assert_near(p.y_axis(), Vec3::Y, 1e-12, "unturned y");
    }
}

/// A scaled sweep's sections are no rigid images of the profile: refused.
#[test]
fn placements_of_a_scaled_sweep_are_refused() {
    let mut k = Kernel::new(&KernelOptions::default());
    let (circle, path) = disc_and_path(&k);
    let reference = Curve::from(
        Line::create_line_segment(3, Vec3::new(0.0, 0.0, 0.0), Vec3::new(10.0, 0.0, 0.0), None)
            .unwrap(),
    );
    let scale = Curve::from(
        Line::create_line_segment(3, Vec3::new(0.0, 1.0, 0.0), Vec3::new(10.0, 2.0, 0.0), None)
            .unwrap(),
    );
    let r = create_curve_sweep_with_placements(
        &mut k,
        &[Some(&circle)],
        Some(&path),
        Some(&reference),
        Some(&scale),
        Some(&options(true)),
    );
    assert!(matches!(r, Err(SolidError::InvalidInput { .. })), "{r:?}");
}
