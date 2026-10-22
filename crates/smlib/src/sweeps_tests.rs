//! The sweeps against analytic volumes, each result a valid closed solid (built with SMLib only).
#![cfg(feature = "native")]

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

#[test]
fn extrusions_straight_and_drafted() {
    let rect = polygon(&[[0.0, 0.0, 0.0], [10.0, 0.0, 0.0], [10.0, 6.0, 0.0], [0.0, 6.0, 0.0]]);
    let b = Brep::extrude(&rect, [0.0, 0.0, 1.0], 5.0, 0.0, true).unwrap();
    assert_close(solid_volume(&b, "prism"), 300.0, 1e-9, "prism");
    // A disc drafted 5 degrees in over 4 mm: a frustum of radii 5 and 5 - 4 tan 5.
    let r2 = 5.0 - 4.0 * 5f64.to_radians().tan();
    let b = Brep::extrude(&[circle([0.0; 3], 5.0)], [0.0, 0.0, 1.0], 4.0, 5f64.to_radians(), true).unwrap();
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
    let tooth = polygon(&[[4.0, 0.0, 0.0], [4.4, 0.0, 0.0], [4.4, 0.0, 0.4], [4.0, 0.0, 0.4]]);
    let b = Brep::screw_sweep(&tooth, [0.0; 3], [0.0, 0.0, 1.0], 1.0, 3.0, 1e-4).unwrap();
    assert_close(solid_volume(&b, "screw"), 0.16 * TAU * 4.2 * 3.0, 1e-4, "screw sweep");
}

/// `x` turned by `a` about z, then lifted `z`.
fn turn_up(a: f64, k: f64, z: f64) -> Affine {
    let (s, c) = a.sin_cos();
    Affine { m: [[k * c, k * s, 0.0], [-k * s, k * c, 0.0], [0.0, 0.0, 1.0]], t: [0.0, 0.0, z] }
}

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

#[test]
fn hostile_curve_lists_are_refused() {
    let nan = [Curve::Line { start: [0.0, f64::NAN, 0.0], end: [1.0, 0.0, 0.0] }];
    assert_eq!(Brep::extrude(&nan, [0.0, 0.0, 1.0], 1.0, 0.0, true).err(), Some(Error::Argument));
    let bad = [Curve::Nurbs { degree: 3, points: vec![[0.0, 0.0, 0.0, 1.0]; 2], knots: vec![0.0; 6] }];
    assert_eq!(Brep::extrude(&bad, [0.0, 0.0, 1.0], 1.0, 0.0, true).err(), Some(Error::Argument));
    assert_eq!(Brep::loft(&[vec![circle([0.0; 3], 1.0)]], 1, true).err(), Some(Error::Argument));
    let sq = polygon(&[[0.0; 3], [1.0, 0.0, 0.0], [1.0, 1.0, 0.0], [0.0, 1.0, 0.0]]);
    assert_eq!(Brep::revolve(&sq, [0.0; 3], [0.0, 1.0, 0.0], 7.0, true).err(), Some(Error::Argument));
    assert_eq!(Brep::screw_sweep(&sq, [0.0; 3], [0.0, 0.0, 1.0], 0.0, 1.0, 1e-4).err(), Some(Error::Argument));
}
