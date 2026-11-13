//! A skin through sections on parallel planes stays between its first and last planes.
//!
//! The skin takes upstream's chord-length parameters across the sections. Only where that
//! skin's control points leave the slab between the first and last planes does it take the
//! planes' distances from the first instead (`SectionPlanes` in
//! `primitive_creation::p3::construct`): every section's height above the first plane is then
//! one linear function of the parameter, which the interpolating skin reproduces exactly, so
//! every control point of the side faces lies between the two end planes and, by the convex
//! hull, so does the skin. Checked on the control points, never on samples. A skin whose
//! chord-length control points stay in the slab is built as before, bit for bit.
//!
//! The carved top's sections take wide steps sideways for small steps up near the first
//! section (0.6 high, up to 16 in), larger ones after. Chord-length parameters come from the
//! whole motion of the control points, so the first step's parameter is far larger than its
//! share of the height; the cubic interpolant of the height overshoots: control points fall
//! 0.24 below the first plane, the skin dips behind it and crosses its own start cap, and its
//! union with a slab whose top is that cap's plane comes out with a negative volume.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use solidcore::boolean::api_brep::boolean_union;
use solidcore::curve::curve::Curve;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::Point3;
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::nurbs::b_spline_surface::SurfParamType;
use solidcore::nurbs::crv_fit::p1::ParamMethod;
use solidcore::primitive::api_primitives::create_box;
use solidcore::surface::surface::SurfaceKind;
use solidcore::sweep::api_surfaces::create_skin_primitive;
use solidcore::topology::Brep;
use solidcore::Kernel;

/// (height, half-length, half-width, shift along x) of each section, as a carved top's.
const SECTIONS: [(f64, f64, f64, f64); 6] = [
    (0.0, 200.0, 150.0, 0.0),
    (0.6, 184.0, 132.0, 6.0),
    (3.2, 160.0, 105.0, 23.0),
    (6.4, 120.0, 68.0, 43.0),
    (8.8, 90.0, 45.0, 58.0),
    (10.0, 66.0, 30.0, 68.0),
];

/// A closed cubic through 16 points of an ellipse bulging toward -x (the waist of a
/// body), at height `z`.
fn section(z: f64, a: f64, b: f64, dx: f64) -> Curve {
    let mut pts: Vec<Point3> = (0..16)
        .map(|j| {
            let t = 2.0 * std::f64::consts::PI * f64::from(j) / 16.0;
            let r = 1.0 + 0.12 * (2.0 * t).cos();
            Point3::new(dx + a * r * t.cos(), b * r * t.sin(), z)
        })
        .collect();
    pts.extend(pts.first().copied());
    Curve::from(
        BSplineCurve::interpolate_points(&pts, None, 3, None, None, true, ParamMethod::ChordLength)
            .unwrap(),
    )
}

fn carved_top(k: &mut Kernel) -> Brep {
    let cs: Vec<Curve> = SECTIONS
        .iter()
        .map(|&(z, a, b, dx)| section(z, a, b, dx))
        .collect();
    let refs: Vec<Option<&Curve>> = cs.iter().map(Some).collect();
    create_skin_primitive(k, &vec![1; cs.len()], &refs, 3, true)
        .unwrap()
        .brep
}

fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-9, &attrs).unwrap()
}

#[test]
fn a_skin_through_parallel_sections_stays_between_its_end_planes() {
    let mut k = Kernel::default();
    let b = carved_top(&mut k);
    let top = SECTIONS[5].0;
    // the heights' rounding: a few ulps of the largest coordinate
    let slack = 64.0 * f64::EPSILON * 300.0;
    let mut sides = 0;
    for &f in b.face_list() {
        let Some(s) = b.face(f).unwrap().surface.clone() else {
            continue;
        };
        let SurfaceKind::BSpline(bs) = s.kind() else {
            continue;
        };
        sides += 1;
        // cubic across the six sections, and polynomial: its hull is its control points'
        assert!(bs.degree(SurfParamType::U).max(bs.degree(SurfParamType::V)) >= 3);
        assert!(!bs.is_rational(), "{f:?}");
        let (_, _, cps) = bs.control_points().unwrap();
        for p in cps {
            let p = p.to_euclid();
            assert!(
                p.z >= -slack && p.z <= top + slack,
                "{f:?}: a control point at height {} leaves the slab [0, {top}]",
                p.z
            );
        }
    }
    assert!(sides > 0, "no B-spline side face");
}

#[test]
fn a_skin_through_parallel_sections_joins_a_slab_under_its_first_section() {
    // The slab's top face is the skin's start cap's plane; they share only that face, so
    // the union's volume is the sum of theirs.
    let mut k = Kernel::default();
    let mut skin = carved_top(&mut k);
    let v_skin = volume(&k, &mut skin);
    let slab = create_box(&mut k, Point3::new(-260.0, -200.0, -5.0), 520.0, 400.0, 5.0).unwrap();
    let mut u = boolean_union(&mut k, slab, skin).unwrap();
    let v = volume(&k, &mut u);
    let want = 520.0 * 400.0 * 5.0 + v_skin;
    assert!(
        (v - want).abs() <= 1e-7 * want,
        "union volume {v} against slab + skin {want}"
    );
}

/// A closed cubic through 16 points of the circle of radius `r` about the z axis at height
/// `z`.
fn circle(z: f64, r: f64) -> Curve {
    let mut pts: Vec<Point3> = (0..16)
        .map(|j| {
            let t = 2.0 * std::f64::consts::PI * f64::from(j) / 16.0;
            Point3::new(r * t.cos(), r * t.sin(), z)
        })
        .collect();
    pts.extend(pts.first().copied());
    Curve::from(
        BSplineCurve::interpolate_points(&pts, None, 3, None, None, true, ParamMethod::ChordLength)
            .unwrap(),
    )
}

/// The capped skin through circles given as (height, radius), and its volume.
fn round_loft(sections: &[(f64, f64)]) -> f64 {
    let mut k = Kernel::default();
    let cs: Vec<Curve> = sections.iter().map(|&(z, r)| circle(z, r)).collect();
    let refs: Vec<Option<&Curve>> = cs.iter().map(Some).collect();
    let mut b = create_skin_primitive(&mut k, &vec![1; cs.len()], &refs, 3, true)
        .unwrap()
        .brep;
    volume(&k, &mut b)
}

/// The volumes of the chord-length skins below, measured with the skin as it was before
/// plane distances (solidcore at origin/main of 2026-11-11).
const WAIST_VOLUME: f64 = 16791.357595192487;
const BULGE_VOLUME: f64 = 272283.3927004977;

/// Lofts whose chord-length skins stay between their end planes keep their shape: a waist
/// (r 20, 10, 20 at z 0, 5, 40) that plane distances would pinch through its axis, and four
/// unevenly spaced sections that plane distances would grow by 29%. Their volumes are the
/// chord-length skins', as built before plane distances existed.
#[test]
fn lofts_that_stay_between_their_end_planes_keep_their_chord_length_shape() {
    let waist = round_loft(&[(0.0, 20.0), (5.0, 10.0), (40.0, 20.0)]);
    assert!(
        (waist - WAIST_VOLUME).abs() <= 1e-9 * WAIST_VOLUME,
        "waist {waist}"
    );
    let bulge = round_loft(&[(0.0, 30.0), (10.0, 50.0), (60.0, 15.0), (80.0, 25.0)]);
    assert!(
        (bulge - BULGE_VOLUME).abs() <= 1e-9 * BULGE_VOLUME,
        "bulge {bulge}"
    );
}
