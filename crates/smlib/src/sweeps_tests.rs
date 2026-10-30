//! The sweeps against analytic volumes, each result a valid closed solid (built with SMLib only).
#![cfg(any(feature = "native", feature = "solidcore"))]

use std::f64::consts::{PI, TAU};

use super::*;
use crate::CheckLevel;

fn solid_volume(b: &Brep, what: &str) -> f64 {
    let report = b.check(CheckLevel::All).unwrap();
    assert!(report.is_valid(), "{what}: {} failed validity tests:\n{}", report.failures, report.text);
    b.volume(1e-7).unwrap()
}

fn assert_close(got: f64, want: f64, rel: f64, what: &str) {
    assert!((got - want).abs() <= rel * want.abs(), "{what}: {got} vs analytic {want}");
}

fn polygon(pts: &[[f64; 3]]) -> Vec<Curve> {
    (0..pts.len()).map(|i| Curve::Line { start: pts[i], end: pts[(i + 1) % pts.len()] }).collect()
}

fn circle(center: [f64; 3], radius: f64) -> Curve {
    Curve::Arc { center, axis: [0.0, 0.0, 1.0], reference: [1.0, 0.0, 0.0], radius, start: 0.0, end: TAU }
}

#[cfg(any(feature = "native", feature = "solidcore"))]
#[test]
fn extrusions_straight_and_drafted() {
    let rect = polygon(&[[0.0, 0.0, 0.0], [10.0, 0.0, 0.0], [10.0, 6.0, 0.0], [0.0, 6.0, 0.0]]);
    let b = Brep::extrude(&rect, [0.0, 0.0, 1.0], 5.0, 0.0, DraftCorner::default(), true).unwrap();
    assert_close(solid_volume(&b, "prism"), 300.0, 1e-9, "prism");
    // A disc drafted 5 degrees in over 4 mm: a frustum of radii 5 and 5 - 4 tan 5.
    let r2 = 5.0 - 4.0 * 5f64.to_radians().tan();
    let b = Brep::extrude(&[circle([0.0; 3], 5.0)], [0.0, 0.0, 1.0], 4.0, 5f64.to_radians(), DraftCorner::default(), true).unwrap();
    assert_close(solid_volume(&b, "frustum"), PI * 4.0 / 3.0 * (25.0 + r2 * r2 + 5.0 * r2), 1e-6, "drafted disc");
}

#[test]
fn revolutions_whole_and_partial() {
    // A half disc on the z axis, in the xz plane: a sphere; a quarter turn of a square off it.
    let half = vec![
        Curve::Line { start: [0.0, 0.0, -5.0], end: [0.0, 0.0, 5.0] },
        Curve::Arc { center: [0.0; 3], axis: [0.0, -1.0, 0.0], reference: [0.0, 0.0, 1.0], radius: 5.0, start: 0.0, end: PI },
    ];
    let b = Brep::revolve(&half, [0.0; 3], [0.0, 0.0, 1.0], TAU, true).unwrap();
    assert_close(solid_volume(&b, "sphere"), 4.0 / 3.0 * PI * 125.0, 1e-5, "sphere");
    let sq = polygon(&[[6.0, 0.0, 0.0], [8.0, 0.0, 0.0], [8.0, 0.0, 2.0], [6.0, 0.0, 2.0]]);
    let b = Brep::revolve(&sq, [0.0; 3], [0.0, 0.0, 1.0], PI / 2.0, true).unwrap();
    assert_close(solid_volume(&b, "quarter ring"), PI / 2.0 * 7.0 * 4.0, 1e-6, "quarter ring");
}

/// A half disc of radius 14 centred 14 off the axis, its rim outward, three quarters of a turn
/// (rig sweeps/4007): a horn torus, which SMLib gives a major radius a rounding below its minor.
/// The record's torus is a ring torus, so the face comes out as its NURBS form, unlabelled, and
/// the record validates. Pappus: 3/4 turn · 2π (14 + 4·14 / 3π) · π 14² / 2.
#[test]
fn a_horn_torus_comes_out_as_its_nurbs() {
    let half = vec![
        Curve::Arc { center: [14.0, 0.0, 0.0], axis: [0.0, -1.0, 0.0], reference: [1.0, 0.0, 0.0], radius: 14.0, start: -PI / 2.0, end: PI / 2.0 },
        Curve::Line { start: [14.0, 0.0, 14.0], end: [14.0, 0.0, -14.0] },
    ];
    let b = Brep::revolve(&half, [0.0; 3], [0.0, 0.0, 1.0], 1.5 * PI, true).unwrap();
    let want = 1.5 * PI * (14.0 + 56.0 / (3.0 * PI)) * PI * 98.0;
    assert_close(solid_volume(&b, "horn torus"), want, 1e-6, "horn torus");
    let record = b.to_native().unwrap();
    record.validate().unwrap();
    assert!(record.faces.iter().all(|f| !matches!(f.surface, solidcraft_geom::brep::Surface::Torus { .. })), "a torus label on a horn torus");
}

#[test]
fn planar_faces_with_a_hole() {
    // A 10 x 10 square with a 4 x 4 hole, the loops in any order and orientation: 100 - 16.
    let mut loops = polygon(&[[0.0, 0.0, 0.0], [10.0, 0.0, 0.0], [10.0, 10.0, 0.0], [0.0, 10.0, 0.0]]);
    loops.extend(polygon(&[[3.0, 3.0, 0.0], [3.0, 7.0, 0.0], [7.0, 7.0, 0.0], [7.0, 3.0, 0.0]]));
    let b = Brep::planar_faces(&loops).unwrap();
    // A sheet measures its faces' area, in `area` and in the mass properties.
    assert_close(b.area(1e-9).unwrap(), 84.0, 1e-6, "sheet area");
    let m = b.mass_properties(1e-9).unwrap();
    assert_close(m.area, 84.0, 1e-6, "sheet mass properties");
    assert_close(m.centroid[0] + m.centroid[1], 10.0, 1e-6, "centroid of the area");
    // Thickened by 1 it is a slab of 84 mm³: the hole is a hole, the faces are one sheet.
    let slab = b.thicken(1.0, false).unwrap();
    assert_close(solid_volume(&slab, "slab"), 84.0, 1e-6, "square with a hole");
    let open = [Curve::Line { start: [0.0; 3], end: [1.0, 0.0, 0.0] }, Curve::Line { start: [1.0, 0.0, 0.0], end: [1.0, 1.0, 0.0] }];
    assert!(Brep::planar_faces(&open).is_err(), "an open loop bounds no face");
}

#[test]
fn lofts_frustum_and_tube() {
    let sections = vec![vec![circle([0.0; 3], 5.0)], vec![circle([0.0, 0.0, 6.0], 3.0)]];
    let b = Brep::loft(&sections, 1, true).unwrap();
    assert_close(solid_volume(&b, "frustum"), PI * 2.0 * (25.0 + 9.0 + 15.0), 1e-5, "lofted frustum");
    // A ring lofted to a ring: the outline and the hole are one section, lofted loop to loop.
    let ring = |z: f64| {
        let hole = Curve::Arc { center: [0.0, 0.0, z], axis: [0.0, 0.0, -1.0], reference: [1.0, 0.0, 0.0], radius: 12.0, start: 0.0, end: TAU };
        vec![circle([0.0, 0.0, z], 20.0), hole]
    };
    let b = Brep::loft(&[ring(0.0), ring(20.0)], 1, true).unwrap();
    assert_close(solid_volume(&b, "tube"), PI * (400.0 - 144.0) * 20.0, 1e-5, "lofted tube");
}
#[cfg(feature = "native")]
#[test]
fn sweeps_lofts_and_screws() {
    let disc = [circle([5.0, 0.0, 0.0], 2.0)];
    let bend =
        [Curve::Arc { center: [-20.0, 0.0, 0.0], axis: [0.0, -1.0, 0.0], reference: [1.0, 0.0, 0.0], radius: 20.0, start: 0.0, end: PI / 2.0 }];
    // The disc keeps its 5 mm offset from the path: Pappus about the arc's centre at 25.
    let b = Brep::sweep(&disc, &bend, SweepFrame::Follow, true).unwrap();
    assert_close(solid_volume(&b, "bent tube"), PI * 4.0 * (PI / 2.0 * 25.0), 1e-4, "bent tube");
    let sections = vec![vec![circle([0.0; 3], 5.0)], vec![circle([0.0, 0.0, 6.0], 3.0)]];
    let b = Brep::loft(&sections, 1, true).unwrap();
    assert_close(solid_volume(&b, "frustum"), PI * 2.0 * (25.0 + 9.0 + 15.0), 1e-5, "lofted frustum");
    // A ring lofted to a ring: the outline and the hole are one section, lofted loop to loop.
    let ring = |z: f64| {
        let hole = Curve::Arc { center: [0.0, 0.0, z], axis: [0.0, 0.0, -1.0], reference: [1.0, 0.0, 0.0], radius: 12.0, start: 0.0, end: TAU };
        vec![circle([0.0, 0.0, z], 20.0), hole]
    };
    let b = Brep::loft(&[ring(0.0), ring(20.0)], 1, true).unwrap();
    assert_close(solid_volume(&b, "tube"), PI * (400.0 - 144.0) * 20.0, 1e-5, "lofted tube");
    // A square to a point 15 above it: a pyramid, A h / 3.
    let base = polygon(&[[-10.0, -10.0, 0.0], [10.0, -10.0, 0.0], [10.0, 10.0, 0.0], [-10.0, 10.0, 0.0]]);
    let b = Brep::loft_to_point(&base, [0.0, 0.0, 15.0]).unwrap();
    assert_close(solid_volume(&b, "pyramid"), 2000.0, 1e-9, "pyramid");
    assert_eq!(Brep::loft_to_point(&base, [3.0, 4.0, 0.0]).err(), Some(Error::Argument), "apex in the base's plane");
    assert_eq!(Brep::loft_to_point(&[circle([0.0; 3], 5.0)], [0.0, 0.0, 5.0]).err(), Some(Error::Argument), "curved base");
    let tooth = polygon(&[[4.0, 0.0, 0.0], [4.4, 0.0, 0.0], [4.4, 0.0, 0.4], [4.0, 0.0, 0.4]]);
    let b = Brep::screw_sweep(&tooth, [0.0; 3], [0.0, 0.0, 1.0], 1.0, 3.0, 1e-4).unwrap();
    assert_close(solid_volume(&b, "screw"), 0.16 * TAU * 4.2 * 3.0, 1e-4, "screw sweep");
    // The tooth moved out 1 mm over its three turns, linearly: A 2 pi (c turns + int offset dt).
    let b = Brep::screw_sweep_radial(&tooth, [0.0; 3], [0.0, 0.0, 1.0], 1.0, 3.0, &[(0.0, 0.0), (3.0, 1.0)], 1e-4).unwrap();
    assert_close(solid_volume(&b, "radial screw"), 0.16 * TAU * (4.2 * 3.0 + 1.5), 1e-4, "radial screw sweep");
    assert_eq!(Brep::screw_sweep_radial(&tooth, [0.0; 3], [0.0, 0.0, 1.0], 1.0, 3.0, &[(2.0, 0.0), (1.0, 1.0)], 1e-4).err(), Some(Error::Argument));
}

/// `x` turned by `a` about z, then lifted `z`.
#[cfg(feature = "native")]
fn turn_up(a: f64, k: f64, z: f64) -> Affine {
    let (s, c) = a.sin_cos();
    Affine { m: [[k * c, k * s, 0.0], [-k * s, k * c, 0.0], [0.0, 0.0, 1.0]], t: [0.0, 0.0, z] }
}

#[cfg(feature = "native")]
#[test]
fn sweeps_through_stations() {
    // A 10 x 10 square up 40, turning 90 degrees: the section area stays, V = 100 * 40.
    let sq = polygon(&[[-5.0, -5.0, 0.0], [5.0, -5.0, 0.0], [5.0, 5.0, 0.0], [-5.0, 5.0, 0.0]]);
    // Stations 1.4 degrees of twist apart: the skin's free ends keep it within 2e-7 of V = A L.
    let n = 65;
    let f = |i: usize| i as f64 / (n - 1) as f64;
    let twist: Vec<Affine> = (0..n).map(|i| turn_up(PI / 2.0 * f(i), 1.0, 40.0 * f(i))).collect();
    let b = Brep::sweep_stations(&sq, &twist, Ends::Capped).unwrap();
    assert_close(solid_volume(&b, "twisted square"), 4000.0, 1e-6, "twisted square");
    // A disc of radius 5 grown to 10 over 30: a cone frustum.
    let grow: Vec<Affine> = (0..n).map(|i| turn_up(0.0, 1.0 + f(i), 30.0 * f(i))).collect();
    let b = Brep::sweep_stations(&[circle([0.0; 3], 5.0)], &grow, Ends::Capped).unwrap();
    assert_close(solid_volume(&b, "frustum"), 10.0 * PI * 175.0, 1e-6, "scaled disc");
    // The square turned a quarter about z at every station: it ends on itself, joined.
    let ring = polygon(&[[26.0, 0.0, -4.0], [34.0, 0.0, -4.0], [34.0, 0.0, 4.0], [26.0, 0.0, 4.0]]);
    let m = 97;
    let round: Vec<Affine> = (0..m).map(|i| turn_up(TAU * i as f64 / (m - 1) as f64, 1.0, 0.0)).collect();
    let b = Brep::sweep_stations(&ring, &round, Ends::Joined).unwrap();
    assert_close(solid_volume(&b, "ring"), 64.0 * TAU * 30.0, 1e-5, "joined ring");
    // A singular station, too few stations.
    let mut flat = twist.clone();
    flat[4].m[0] = [0.0; 3];
    assert_eq!(Brep::sweep_stations(&sq, &flat, Ends::Capped).err(), Some(Error::Argument));
    assert_eq!(Brep::sweep_stations(&sq, &twist[..1], Ends::Capped).err(), Some(Error::Argument));
}

#[cfg(feature = "native")]
#[test]
fn hostile_curve_lists_are_refused() {
    let nan = [Curve::Line { start: [0.0, f64::NAN, 0.0], end: [1.0, 0.0, 0.0] }];
    assert_eq!(Brep::extrude(&nan, [0.0, 0.0, 1.0], 1.0, 0.0, DraftCorner::default(), true).err(), Some(Error::Argument));
    let bad = [Curve::Nurbs { degree: 3, points: vec![[0.0, 0.0, 0.0, 1.0]; 2], knots: vec![0.0; 6] }];
    assert_eq!(Brep::extrude(&bad, [0.0, 0.0, 1.0], 1.0, 0.0, DraftCorner::default(), true).err(), Some(Error::Argument));
    assert_eq!(Brep::loft(&[vec![circle([0.0; 3], 1.0)]], 1, true).err(), Some(Error::Argument));
    let sq = polygon(&[[0.0; 3], [1.0, 0.0, 0.0], [1.0, 1.0, 0.0], [0.0, 1.0, 0.0]]);
    assert_eq!(Brep::revolve(&sq, [0.0; 3], [0.0, 1.0, 0.0], 7.0, true).err(), Some(Error::Argument));
    assert_eq!(Brep::screw_sweep(&sq, [0.0; 3], [0.0, 0.0, 1.0], 0.0, 1.0, 1e-4).err(), Some(Error::Argument));
}

/// What solidcore takes today: a straight extrusion (drafted ones are refused), hostile numbers.
#[cfg(feature = "solidcore")]
#[test]
fn solidcore_extrudes_straight_and_refuses_hostile_input() {
    let rect = polygon(&[[0.0, 0.0, 0.0], [10.0, 0.0, 0.0], [10.0, 6.0, 0.0], [0.0, 6.0, 0.0]]);
    let b = Brep::extrude(&rect, [0.0, 0.0, 1.0], 5.0, 0.0, DraftCorner::default(), true).unwrap();
    assert_close(solid_volume(&b, "prism"), 300.0, 1e-9, "prism");
    let disc = Brep::extrude(&[circle([1.0, 2.0, 3.0], 5.0)], [0.0, 0.0, 1.0], 4.0, 0.0, DraftCorner::default(), true).unwrap();
    assert_close(solid_volume(&disc, "cylinder"), PI * 25.0 * 4.0, 1e-6, "cylinder");
    // A drafted polygon is a prismoid: its top is the outline mitred in by 5 tan(0.1).
    let d = 5.0 * 0.1_f64.tan();
    let drafted = Brep::extrude(&rect, [0.0, 0.0, 1.0], 5.0, 0.1, DraftCorner::default(), true).unwrap();
    let area = |k: f64| (10.0 - 2.0 * d * k) * (6.0 - 2.0 * d * k);
    assert_close(solid_volume(&drafted, "prismoid"), 5.0 / 6.0 * (area(0.0) + 4.0 * area(0.5) + area(1.0)), 1e-9, "prismoid");
    let nan = [Curve::Line { start: [0.0, f64::NAN, 0.0], end: [1.0, 0.0, 0.0] }];
    assert_eq!(Brep::extrude(&nan, [0.0, 0.0, 1.0], 1.0, 0.0, DraftCorner::default(), true).err(), Some(Error::Argument));
    assert_eq!(Brep::revolve(&rect, [0.0; 3], [0.0, 1.0, 0.0], 7.0, true).err(), Some(Error::Argument));
}

/// Sweeps along a path on solidcore: along a straight line and an arc (the exact walls), along a
/// cubic (the curve sweep, the volume that of the tube along the curve), and what it refuses.
#[cfg(feature = "solidcore")]
#[test]
fn solidcore_sweeps_along_paths() {
    let disc = [circle([5.0, 0.0, 0.0], 2.0)];
    let bend =
        [Curve::Arc { center: [-20.0, 0.0, 0.0], axis: [0.0, -1.0, 0.0], reference: [1.0, 0.0, 0.0], radius: 20.0, start: 0.0, end: PI / 2.0 }];
    // The disc keeps its 5 mm offset from the path: Pappus about the arc's centre at 25.
    let b = Brep::sweep(&disc, &bend, SweepFrame::Follow, true).unwrap();
    assert_close(solid_volume(&b, "bent tube"), PI * 4.0 * (PI / 2.0 * 25.0), 1e-4, "bent tube");
    let (b, at) = Brep::sweep_placed(&disc, &bend, SweepFrame::Follow, true).unwrap();
    assert_close(solid_volume(&b, "placed tube"), PI * 4.0 * (PI / 2.0 * 25.0), 1e-4, "placed tube");
    // The last section is the disc turned a quarter about the arc's axis through its centre.
    assert!((at[2].t[0] + 20.0).abs() < 1e-9 && (at[2].t[2] - 20.0).abs() < 1e-9 && (at[2].m[0][2] - 1.0).abs() < 1e-9, "last motion {:?}", at[2]);
    let line = [Curve::Line { start: [0.0; 3], end: [0.0, 0.0, 10.0] }, Curve::Line { start: [0.0, 0.0, 10.0], end: [0.0, 0.0, 25.0] }];
    let b = Brep::sweep(&[circle([0.0; 3], 3.0)], &line, SweepFrame::Follow, true).unwrap();
    assert_close(solid_volume(&b, "straight tube"), PI * 9.0 * 25.0, 1e-9, "straight tube");
    // A gentle S in the xz plane, the disc round its start (the path leaves it along z).
    let s = [Curve::Nurbs {
        degree: 3,
        points: vec![[0.0, 0.0, 0.0, 1.0], [0.0, 0.0, 10.0, 1.0], [10.0, 0.0, 20.0, 1.0], [10.0, 0.0, 30.0, 1.0]],
        knots: vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
    }];
    let length: f64 = (0..2000)
        .map(|i| {
            let at = |t: f64| {
                let u = 1.0 - t;
                let c = |a: f64, b: f64, c: f64, d: f64| u * u * u * a + 3.0 * u * u * t * b + 3.0 * u * t * t * c + t * t * t * d;
                [c(0.0, 0.0, 10.0, 10.0), c(0.0, 10.0, 20.0, 30.0)]
            };
            let (a, b) = (at(f64::from(i) / 2000.0), at(f64::from(i + 1) / 2000.0));
            ((b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2)).sqrt()
        })
        .sum();
    let b = Brep::sweep(&[circle([0.0; 3], 1.0)], &s, SweepFrame::Follow, true).unwrap();
    assert_close(solid_volume(&b, "S tube"), PI * length, 2e-3, "S tube");
    // Its motions are rebuilt from the sweep's frames: the profile's centre, which sits at the
    // path's start, goes to the path's end by the last motion and to the path's parameter
    // midpoint (the cubic at 1/2) by the middle one, each a rigid motion (unit columns).
    let (_, at) = Brep::sweep_placed(&[circle([0.0; 3], 1.0)], &s, SweepFrame::Follow, true).unwrap();
    for (motion, want) in [(&at[0], [0.0, 0.0, 0.0]), (&at[1], [5.0, 0.0, 15.0]), (&at[2], [10.0, 0.0, 30.0])] {
        assert!(motion.t.iter().zip(want).all(|(a, b)| (a - b).abs() < 1e-6), "motion {motion:?} against {want:?}");
        for c in &motion.m {
            assert!((c.iter().map(|x| x * x).sum::<f64>() - 1.0).abs() < 1e-9, "column {c:?}");
        }
    }
    // A profile elsewhere than at the start is not the curve sweep's own.
    assert_eq!(Brep::sweep(&[circle([4.0, 0.0, 0.0], 1.0)], &s, SweepFrame::Follow, true).err(), Some(Error::Unsupported));
}
