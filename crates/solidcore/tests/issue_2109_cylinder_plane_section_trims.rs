//! Issue #2109 (WR-680, `fixes.cylinder_plane_section_trims_exact`): the UV trims on a
//! cylinder face along an edge that is a plane section of it.
//!
//! A 40 x 40 x 20 plate (top face z = 0) united with an r3 peg through the origin, from
//! 10 below the plate's face to 15 above it along its axis, the peg tilted 0, 15, 30 and
//! 45 degrees about x. The rim where the peg leaves the plate is the plane z = 0 cut
//! through the cylinder: a circle upright, an ellipse tilted. Upstream (and the port by
//! default) drops each tilted rim edge onto the cylinder by tracing and fitting a cubic
//! within the drop tolerance: the trims sit 2e-6 to 1e-5 off their edge, and the peg's
//! side area and the body's volume carry that error (side area -4.8e-6 at 15 degrees).
//! The plate's trims of the same edges are exact. With the fix the cylinder's trims
//! are the exact rational curves, on their edges to round-off, and the side area,
//! plate top and volume agree with the closed forms (the cut goes through the axis
//! point, so 15 of the peg's length is above the plate):
//!
//! - peg side `2 pi R 15`, plate top `1600 - pi R^2 / cos tau`,
//!   volume `32000 + 15 pi R^2`;
//! - rounded r1 (`fillet_exact_round_classes`, whose rails are exact rational curves
//!   on the cylinder): peg side `2 pi R (15 - 1 / cos tau)`.
//!
//! The same holds for a hole: the r3 cylinder through the origin, tilted, cut from the
//! plate 40 x 40 x 20 about the origin, whose wall meets both faces in plane sections
//! (wall `2 pi R 20 / cos tau`, volume `32000 - pi R^2 20 / cos tau`).
//!
//! The upright circle is an iso-parameter line of the cylinder, which the drop already
//! gives exactly; it stays that line either way.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use solidcore::blend::fillet_executive::api::fillet_edges;
use solidcore::boolean::api_brep::{boolean_difference, boolean_union};
use solidcore::curve::curve::{Curve, CurveKind};
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::vector2d::Point2;
use solidcore::mass::integrator::properties::{brep_volume, face_area, PropertyAttributes};
use solidcore::math::{Point3, Vec3, PI};
use solidcore::primitive::api_primitives::{create_box, create_cylinder};
use solidcore::surface::surface::{Surface, SurfaceKind};
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::edgeuse::p1 as eu1;
use solidcore::topology::face::p2 as face;
use solidcore::topology::{edge, EdgeId, FaceId};
use solidcore::{Brep, Kernel};

const R: f64 = 3.0;
const FILLET: f64 = 1.0;
/// The integrations' accuracy.
const ACCURACY: f64 = 1e-12;
/// How far a trim lifted onto its face may be from its edge with the fix: round-off
/// (the trims come out 4e-15 to 1.1e-14 off).
const EXACT_GAP: f64 = 1e-12;
/// How far the peg's side area may be from the closed form with the fix, relative: the
/// integration's own accuracy (1e-12 relative to its estimate; the upright peg, whose
/// trims upstream already makes exact, is 1.2e-12 off, the tilted ones up to 1.9e-12).
const AREA_REL: f64 = 5e-12;

/// The plate and the peg tilted `tau_deg`, united, on a kernel with the fix `exact`
/// (and, for the round, `fillet_exact_round_classes`).
fn plate_and_peg(tau_deg: f64, exact: bool) -> (Kernel, Brep) {
    let mut k = Kernel::default();
    k.fixes.cylinder_plane_section_trims_exact = exact;
    k.fixes.fillet_exact_round_classes = true;
    let plate = create_box(&mut k, Point3::new(-20.0, -20.0, -20.0), 40.0, 40.0, 20.0).unwrap();
    let mut peg = create_cylinder(&mut k, Point3::new(0.0, 0.0, -10.0), R, 25.0).unwrap();
    let tau = tau_deg.to_radians();
    let pl = Axis2Placement::new(
        Point3::new(0.0, 0.0, 0.0),
        Vec3::new(1.0, 0.0, 0.0),
        Vec3::new(0.0, tau.cos(), tau.sin()),
    );
    solidcore::topology::brep::p5::transform(&mut k, &mut peg, &pl, None, false).unwrap();
    let b = boolean_union(&mut k, plate, peg).unwrap();
    (k, b)
}

/// [`plate_and_peg`] with its rim rounded r1.
fn rounded_plate_and_peg(tau_deg: f64, exact: bool) -> (Kernel, Brep) {
    let (mut k, mut b) = plate_and_peg(tau_deg, exact);
    let rim = rim_edges(&k, &b);
    assert_eq!(rim.len(), 2, "the rim's two halves");
    fillet_edges(&mut k, &mut b, &rim, FILLET, 1, 1, 1.0).unwrap();
    (k, b)
}

fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, ACCURACY, &attrs).unwrap()
}

fn is_cylinder(s: &Surface) -> bool {
    matches!(s.kind(), SurfaceKind::Cone(c) if c.is_cylinder())
}

/// The plate 40 x 40 x 20 about the origin with the r3 cylinder through the origin,
/// tilted `tau_deg` about x, cut from it, on a kernel with the fix.
fn plate_with_hole(tau_deg: f64) -> (Kernel, Brep) {
    let mut k = Kernel::default();
    k.fixes.cylinder_plane_section_trims_exact = true;
    let plate = create_box(&mut k, Point3::new(-20.0, -20.0, -10.0), 40.0, 40.0, 20.0).unwrap();
    let mut drill = create_cylinder(&mut k, Point3::new(0.0, 0.0, -30.0), R, 60.0).unwrap();
    let tau = tau_deg.to_radians();
    let pl = Axis2Placement::new(
        Point3::new(0.0, 0.0, 0.0),
        Vec3::new(1.0, 0.0, 0.0),
        Vec3::new(0.0, tau.cos(), tau.sin()),
    );
    solidcore::topology::brep::p5::transform(&mut k, &mut drill, &pl, None, false).unwrap();
    let b = boolean_difference(&mut k, plate, drill).unwrap();
    (k, b)
}

/// The peg's side above the plate (or the hole's wall): the faces on a cylinder.
fn peg_side_area(k: &Kernel, b: &mut Brep) -> f64 {
    let faces: Vec<FaceId> = b
        .faces()
        .ids()
        .filter(|&f| face::surface(b, f).unwrap().is_some_and(is_cylinder))
        .collect();
    assert_eq!(faces.len(), 1, "one cylinder face");
    faces
        .into_iter()
        .map(|f| face_area(k, b, f, ACCURACY).unwrap())
        .sum()
}

/// The plate's top face's area: the plane face through z = 0 with normal along z.
fn plate_top_area(k: &Kernel, b: &mut Brep) -> f64 {
    let tops: Vec<FaceId> = b
        .faces()
        .ids()
        .filter(|&f| match face::surface(b, f).unwrap().map(Surface::kind) {
            Some(SurfaceKind::Plane(p)) => {
                p.position().origin().z.abs() < 1e-9
                    && (p.position().z_axis().z.abs() - 1.0).abs() < 1e-9
            }
            _ => false,
        })
        .collect();
    assert_eq!(tops.len(), 1, "one plate top face");
    face_area(k, b, tops[0], ACCURACY).unwrap()
}

fn midpoint(k: &Kernel, b: &Brep, e: EdgeId) -> Point3 {
    let ivl = edge::interval(b, e).unwrap();
    let curve = edge::curve(b, e).unwrap().unwrap();
    curve
        .evaluate_point(k, 0.5 * (ivl.min() + ivl.max()))
        .unwrap()
}

/// The rim: the edges where the peg leaves the plate's top face.
fn rim_edges(k: &Kernel, b: &Brep) -> Vec<EdgeId> {
    let edges: Vec<EdgeId> = b.edges().ids().collect();
    edges
        .into_iter()
        .filter(|&e| {
            let m = midpoint(k, b, e);
            m.z.abs() < 1e-6 && m.x.abs() < 10.0 && m.y.abs() < 10.0
        })
        .collect()
}

/// The distance from `p` to `curve` over `ivl`: the nearest of 256 samples, then golden
/// section about it.
fn distance_to_curve(k: &Kernel, curve: &Curve, ivl: [f64; 2], p: Point3) -> f64 {
    let n = 256;
    let at = |t: f64| curve.evaluate_point(k, t).unwrap().distance(p);
    let h = (ivl[1] - ivl[0]) / f64::from(n);
    let mut best = 0;
    for m in 1..=n {
        if at(ivl[0] + f64::from(m) * h) < at(ivl[0] + f64::from(best) * h) {
            best = m;
        }
    }
    let mid = ivl[0] + f64::from(best) * h;
    let (mut lo, mut hi) = ((mid - h).max(ivl[0]), (mid + h).min(ivl[1]));
    let g = 0.5 * (5f64.sqrt() - 1.0);
    for _ in 0..100 {
        let (m1, m2) = (hi - g * (hi - lo), lo + g * (hi - lo));
        if at(m1) < at(m2) {
            hi = m2;
        } else {
            lo = m1;
        }
    }
    at(0.5 * (lo + hi))
}

/// One cylinder trim against its edge.
#[derive(Debug)]
struct TrimFit {
    edge: EdgeId,
    /// The trim's degree and whether it is rational.
    degree: usize,
    rational: bool,
    /// The largest distance (129 samples) from the trim lifted onto the face to the
    /// edge's point at the same parameter (an upper bound of the distance to the edge).
    same_param: f64,
    /// The largest distance from the trim lifted onto the face to the edge's curve (as
    /// the issue measures it), when asked for.
    nearest: Option<f64>,
}

/// The trims on the peg's side of every edge of it (the mates of the uses holding one
/// read that one).
fn cylinder_trims(k: &Kernel, b: &Brep, nearest: bool) -> Vec<TrimFit> {
    let mut fits = Vec::new();
    for e in b.edges().ids() {
        let curve = edge::curve(b, e).unwrap().unwrap();
        let iv = edge::interval(b, e).unwrap();
        for eu in edge::edgeuses(b, e, None, None).unwrap() {
            let Some(f) = eu1::face(b, eu).unwrap() else {
                continue;
            };
            let surface = face::surface(b, f).unwrap().unwrap();
            if !is_cylinder(surface) {
                continue;
            }
            let Some(trim) = eu1::uv_trim_curve_pointer(b, eu).unwrap() else {
                continue;
            };
            let CurveKind::BSpline(bs) = trim.kind() else {
                panic!("{e:?}: the trim is not a B-spline");
            };
            let tiv = trim.natural_interval();
            let mut same_param = 0.0_f64;
            let mut near = 0.0_f64;
            for i in 0..=128 {
                let t = iv.evaluate(f64::from(i) / 128.0);
                let uv = trim
                    .evaluate_point(k, t.clamp(tiv.min(), tiv.max()))
                    .unwrap();
                let p = surface.evaluate_point(k, Point2::new(uv.x, uv.y)).unwrap();
                same_param = same_param.max(p.distance(curve.evaluate_point(k, t).unwrap()));
                if nearest {
                    near = near.max(distance_to_curve(k, curve, [iv.min(), iv.max()], p));
                }
            }
            fits.push(TrimFit {
                edge: e,
                degree: bs.degree().unwrap(),
                rational: bs.is_rational().unwrap(),
                same_param,
                nearest: nearest.then_some(near),
            });
        }
    }
    fits
}

fn assert_checks_clean(k: &mut Kernel, b: &mut Brep) {
    let c = check_brep(k, b, CheckLevel::Level2).unwrap();
    assert_eq!(c.failure_count(), 0, "{:?}", c.lines());
}

#[test]
fn plane_section_trims_on_a_tilted_peg_are_exact_with_the_fix() {
    let union = 32000.0 + 15.0 * PI * R * R;
    let side = 2.0 * PI * R * 15.0;
    for tau in [0.0_f64, 15.0, 30.0, 45.0] {
        let (mut k, mut b) = plate_and_peg(tau, true);
        let fits = cylinder_trims(&k, &b, false);
        for fit in &fits {
            assert!(
                fit.same_param < EXACT_GAP,
                "{tau} degrees: {fit:?} is off its edge"
            );
        }
        // the rim: one circle upright, the ellipse's two halves tilted
        let rim = rim_edges(&k, &b);
        assert_eq!(rim.len(), if tau == 0.0 { 1 } else { 2 });
        let rim_fits: Vec<&TrimFit> = fits.iter().filter(|f| rim.contains(&f.edge)).collect();
        assert_eq!(rim_fits.len(), rim.len(), "{fits:?}");
        for fit in rim_fits {
            if tau == 0.0 {
                // the upright rim is an iso line, the drop's own exact line
                assert_eq!((fit.degree, fit.rational), (1, false), "{fit:?}");
            } else {
                // the ellipse's rational quadratic NURB gives a rational quartic
                assert_eq!((fit.degree, fit.rational), (4, true), "{fit:?}");
            }
        }

        let got = peg_side_area(&k, &mut b);
        assert!(
            (got - side).abs() < AREA_REL * side,
            "{tau} degrees: peg side {got} != {side} (off {:e})",
            got - side
        );
        let top = 1600.0 - PI * R * R / tau.to_radians().cos();
        let got = plate_top_area(&k, &mut b);
        assert!(
            (got - top).abs() < 1e-13 * top,
            "{tau} degrees: plate top {got} != {top}"
        );
        let got = volume(&k, &mut b);
        assert!(
            (got - union).abs() < 1e-13 * union,
            "{tau} degrees: volume {got} != {union} (off {:e})",
            got - union
        );
        assert_checks_clean(&mut k, &mut b);
    }
}

#[test]
fn upstream_fits_the_tilted_peg_trims() {
    let side = 2.0 * PI * R * 15.0;
    for tau in [0.0_f64, 15.0, 30.0, 45.0] {
        let (k, mut b) = plate_and_peg(tau, false);
        let rim = rim_edges(&k, &b);
        for fit in cylinder_trims(&k, &b, true)
            .iter()
            .filter(|f| rim.contains(&f.edge))
        {
            let near = fit.nearest.unwrap();
            if tau == 0.0 {
                assert_eq!((fit.degree, fit.rational), (1, false), "{fit:?}");
                assert!(near < EXACT_GAP, "{fit:?}");
            } else {
                // the traced cubic, 2.3e-6 to 9.7e-6 off its edge
                assert_eq!((fit.degree, fit.rational), (3, false), "{fit:?}");
                assert!(near > 1e-6 && near < 1e-4, "{tau} degrees: {fit:?}");
            }
        }
        if tau == 15.0 {
            // the error does not cancel round the loop here (-4.8e-6)
            let got = peg_side_area(&k, &mut b);
            assert!((got - side).abs() > 1e-6, "peg side {got} vs {side}");
        }
    }
}

#[test]
fn rail_trims_on_a_rounded_tilted_peg_are_exact_with_the_fix() {
    for tau in [15.0_f64, 30.0, 45.0] {
        let side = 2.0 * PI * R * (15.0 - FILLET / tau.to_radians().cos());
        let (mut k, mut b) = rounded_plate_and_peg(tau, true);
        let fits = cylinder_trims(&k, &b, false);
        // the rails' quartic rational curves give rational trims of degree 8
        let rails: Vec<&TrimFit> = fits.iter().filter(|f| f.rational).collect();
        assert_eq!(rails.len(), 2, "{fits:?}");
        assert!(rails.iter().all(|f| f.degree == 8), "{fits:?}");
        for fit in &fits {
            assert!(
                fit.same_param < EXACT_GAP,
                "{tau} degrees: {fit:?} is off its edge"
            );
        }
        let got = peg_side_area(&k, &mut b);
        assert!(
            (got - side).abs() < AREA_REL * side,
            "{tau} degrees: peg side {got} != {side} (off {:e})",
            got - side
        );
        assert_checks_clean(&mut k, &mut b);
    }
}

#[test]
fn upstream_fits_the_rounded_tilted_peg_rail_trims() {
    for tau in [15.0_f64, 30.0, 45.0] {
        let side = 2.0 * PI * R * (15.0 - FILLET / tau.to_radians().cos());
        let (k, mut b) = rounded_plate_and_peg(tau, false);
        let fits = cylinder_trims(&k, &b, false);
        // the rails' traced cubics, 4e-6 to 3.4e-5 off their edges
        let rails: Vec<&TrimFit> = fits.iter().filter(|f| f.degree == 3).collect();
        assert_eq!(rails.len(), 2, "{fits:?}");
        for fit in rails {
            assert!(!fit.rational, "{fit:?}");
            assert!(
                fit.same_param > 1e-6 && fit.same_param < 1e-4,
                "{tau} degrees: {fit:?}"
            );
        }
        let got = peg_side_area(&k, &mut b);
        assert!(
            (got - side).abs() > 1e-6,
            "{tau} degrees: peg side {got} vs {side}"
        );
    }
}

#[test]
fn plane_section_trims_in_a_tilted_hole_are_exact_with_the_fix() {
    for tau in [0.0_f64, 15.0, 30.0, 45.0] {
        let length = 20.0 / tau.to_radians().cos();
        let (wall, cut) = (2.0 * PI * R * length, 32000.0 - PI * R * R * length);
        let (mut k, mut b) = plate_with_hole(tau);
        let fits = cylinder_trims(&k, &b, false);
        // at least the two rims (each one circle, or two halves of an ellipse)
        assert!(fits.len() >= 2, "{fits:?}");
        for fit in &fits {
            assert!(
                fit.same_param < EXACT_GAP,
                "{tau} degrees: {fit:?} is off its edge"
            );
        }
        if tau > 0.0 {
            assert!(
                fits.iter().filter(|f| f.rational && f.degree == 4).count() >= 4,
                "{fits:?}"
            );
        }
        let got = peg_side_area(&k, &mut b);
        assert!(
            (got - wall).abs() < AREA_REL * wall,
            "{tau} degrees: hole wall {got} != {wall} (off {:e})",
            got - wall
        );
        let got = volume(&k, &mut b);
        assert!(
            (got - cut).abs() < 1e-13 * cut,
            "{tau} degrees: volume {got} != {cut} (off {:e})",
            got - cut
        );
        assert_checks_clean(&mut k, &mut b);
    }
}
