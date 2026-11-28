//! The sweeps against analytic volumes, each result a valid closed solid.

use std::f64::consts::{PI, TAU};

use super::*;
use crate::{CheckLevel, Error};

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
    let b = Brep::extrude(&rect, [0.0, 0.0, 1.0], 5.0, 0.0, DraftCorner::default(), true).unwrap();
    assert_close(solid_volume(&b, "prism"), 300.0, 1e-9, "prism");
    // A disc drafted 5 degrees in over 4 mm: a frustum of radii 5 and 5 - 4 tan 5.
    let r2 = 5.0 - 4.0 * 5f64.to_radians().tan();
    let b = Brep::extrude(&[circle([0.0; 3], 5.0)], [0.0, 0.0, 1.0], 4.0, 5f64.to_radians(), DraftCorner::default(), true).unwrap();
    assert_close(solid_volume(&b, "frustum"), PI * 4.0 / 3.0 * (25.0 + r2 * r2 + 5.0 * r2), 1e-6, "drafted disc");
}

/// Two cubic pieces of a closed spline that run within 3e-7 mm of one straight line, from the
/// design of #22584, closed into a stadium by two half circles and a line, drafted 10 degrees
/// outward over 80 mm. Each piece's wall is a ruled patch a plane fits to within the zone
/// tolerance (1e-5) but not to within half its edges' (5e-6): taken as that plane, the wall
/// missed its corner by 7.03e-6 and the check refused the solid. The volume is the profile's
/// area A, its length L and the offset d = z tan 10 at depth z, integrated over the height
/// (A + L d + pi d^2: the profile is convex, so each section is its parallel curve).
#[test]
fn a_drafted_nearly_straight_spline_keeps_its_walls_on_their_corners() {
    let z = 80.0;
    let cubic = |p: [[f64; 2]; 4]| Curve::Nurbs {
        degree: 3,
        points: p.iter().map(|q| [q[0], q[1], z, 1.0]).collect(),
        knots: vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
    };
    let pieces = [
        [
            [22.9046875, 108.60000000000001],
            [22.1226345485602, 108.59999969569137],
            [21.340581597201535, 108.59999991448386],
            [20.558528645833334, 108.60000000000001],
        ],
        [
            [20.558528645833334, 108.60000000000001],
            [19.740407986132755, 108.60000008946011],
            [18.922287326421745, 108.60000003306719],
            [18.104166666666668, 108.60000000000001],
        ],
    ];
    let (start, end, r) = (pieces[0][0], pieces[1][3], 24.3);
    let arc = |c: [f64; 2], from: f64| Curve::Arc {
        center: [c[0], c[1], z],
        axis: [0.0, 0.0, 1.0],
        reference: [1.0, 0.0, 0.0],
        radius: r,
        start: from,
        end: from + PI,
    };
    let profile = vec![
        cubic(pieces[0]),
        cubic(pieces[1]),
        arc([end[0], end[1] - r], PI / 2.0),
        Curve::Line { start: [end[0], end[1] - 2.0 * r, z], end: [start[0], start[1] - 2.0 * r, z] },
        arc([start[0], start[1] - r], 1.5 * PI),
    ];
    let taper = 10f64.to_radians();
    let b = Brep::extrude(&profile, [0.0, 0.0, -1.0], z, -taper, DraftCorner::Curvature, true).unwrap();
    // A and L by Green's theorem and arc length, 16-point Gauss-Legendre on each piece (exact
    // for the cubics' area, the circles' closed forms for theirs).
    let nodes: Vec<(f64, f64)> = (1..=16)
        .map(|i| {
            let mut x = (PI * (f64::from(i) - 0.25) / 16.5).cos();
            let mut dp = 1.0;
            for _ in 0..100 {
                let (mut p0, mut p1) = (1.0, x);
                for k in 2..=16 {
                    let k = f64::from(k);
                    (p0, p1) = (p1, ((2.0 * k - 1.0) * x * p1 - (k - 1.0) * p0) / k);
                }
                dp = 16.0 * (x * p1 - p0) / (x * x - 1.0);
                x -= p1 / dp;
            }
            (0.5 * (x + 1.0), 1.0 / ((1.0 - x * x) * dp * dp))
        })
        .collect();
    let (mut area, mut length) = (0.0, 0.0);
    for p in pieces {
        for &(t, w) in &nodes {
            let m = 1.0 - t;
            let (b, db) = (
                [m * m * m, 3.0 * m * m * t, 3.0 * m * t * t, t * t * t],
                [-3.0 * m * m, 3.0 * m * m - 6.0 * m * t, 6.0 * m * t - 3.0 * t * t, 3.0 * t * t],
            );
            let at = |c: &[f64; 4], i: usize| c.iter().zip(&p).map(|(c, q)| c * q[i]).sum::<f64>();
            let (x, y, dx, dy) = (at(&b, 0), at(&b, 1), at(&db, 0), at(&db, 1));
            area += w * 0.5 * (x * dy - y * dx);
            length += w * dx.hypot(dy);
        }
    }
    // The half circles about (cx, cy) from angle a over pi: 0.5 (r^2 pi + r (cx (sin b - sin a) - cy (cos b - cos a))).
    for (c, a) in [([end[0], end[1] - r], PI / 2.0), ([start[0], start[1] - r], 1.5 * PI)] {
        let b2 = a + PI;
        area += 0.5 * (r * r * PI + r * (c[0] * (b2.sin() - a.sin()) - c[1] * (b2.cos() - a.cos())));
        length += r * PI;
    }
    let (y0, x0, x1) = (end[1] - 2.0 * r, end[0], start[0]);
    area += 0.5 * (x0 * (start[1] - 2.0 * r) - y0 * x1);
    length += (x1 - x0).hypot(start[1] - end[1]);
    let t = taper.tan();
    let want = z * area + length * t * z * z / 2.0 + PI * t * t * z * z * z / 3.0;
    assert_close(solid_volume(&b, "drafted stadium"), want, 1e-6, "drafted stadium");
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
fn a_stopped_revolve_refuses_before_parsing_its_profile() {
    let stop = crate::stop::Stop::new();
    stop.stop();
    let r = stop.within(|| Brep::revolve_joining_arcs(&[], [0.0; 3], [0.0, 0.0, 1.0], TAU, true));
    assert_eq!(r.err(), Some(Error::Cancelled("smlib_revolve")));
}

#[test]
fn a_revolved_arc_split_into_pieces_keeps_one_spherical_wall() {
    // The open path in issue #18537 splits one circular sketch arc. Its pieces still
    // define one sphere; the extra face boundary used to block the handle's fillet.
    let arc = |start, end| Curve::Arc { center: [0.0; 3], axis: [0.0, -1.0, 0.0], reference: [0.0, 0.0, 1.0], radius: 5.0, start, end };
    let profile = vec![arc(0.0, PI / 3.0), arc(PI / 3.0, PI), Curve::Line { start: [0.0, 0.0, -5.0], end: [0.0, 0.0, 5.0] }];
    for angle in [TAU, PI / 2.0] {
        for offset in 0..profile.len() {
            let mut pieces = profile.clone();
            pieces.rotate_left(offset);
            let b = Brep::revolve_joining_arcs(&pieces, [0.0; 3], [0.0, 0.0, 1.0], angle, true).unwrap();
            let record = b.to_native().unwrap();
            assert_eq!(record.faces.len(), if angle == TAU { 1 } else { 3 }, "angle {angle}, offset {offset}");
            assert_close(solid_volume(&b, "split sphere"), angle / TAU * 4.0 / 3.0 * PI * 125.0, 1e-5, "split sphere");
        }
    }
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
    solidcore_ops::record_check::validate(&record).unwrap();
    assert!(record.faces.iter().all(|f| !matches!(f.surface, solidcore_record::brep::Surface::Torus { .. })), "a torus label on a horn torus");
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
    // A circle to a point 5 above it: the cone, pi r^2 h / 3 (#11216); in its plane it is refused.
    let b = Brep::loft_to_point(&[circle([0.0; 3], 5.0)], [0.0, 0.0, 5.0]).unwrap();
    assert_close(solid_volume(&b, "cone"), 125.0 * PI / 3.0, 1e-6, "cone");
    assert_eq!(Brep::loft_to_point(&[circle([0.0; 3], 5.0)], [1.0, 2.0, 0.0]).err(), Some(Error::Argument), "circle, apex in its plane");
    let tooth = polygon(&[[4.0, 0.0, 0.0], [4.4, 0.0, 0.0], [4.4, 0.0, 0.4], [4.0, 0.0, 0.4]]);
    let b = Brep::screw_sweep(&tooth, [0.0; 3], [0.0, 0.0, 1.0], 1.0, 3.0, 1e-4).unwrap();
    assert_close(solid_volume(&b, "screw"), 0.16 * TAU * 4.2 * 3.0, 1e-4, "screw sweep");
    let historical = b.to_native().unwrap();
    assert!(historical.faces.iter().filter(|f| matches!(f.surface, solidcore_record::brep::Surface::Sweep { .. })).count() > 2);
    let options = crate::sweeps::ScrewSweepOptions { law: None, join_spline_walls: true, tolerance: 1e-4 };
    let b = Brep::screw_sweep_with_options(&tooth, [0.0; 3], [0.0, 0.0, 1.0], 1.0, 3.0, options).unwrap();
    let record = b.to_native().unwrap();
    let flanks = record.faces.iter().filter(|f| matches!(f.surface, solidcore_record::brep::Surface::Sweep { .. })).count();
    assert_eq!(flanks, 2, "each screw flank must remain one face across the cylinder seams");
    // The tooth moved out 1 mm over its three turns, linearly: A 2 pi (c turns + int offset dt).
    let b = Brep::screw_sweep_with_options(
        &tooth,
        [0.0; 3],
        [0.0, 0.0, 1.0],
        1.0,
        3.0,
        crate::sweeps::ScrewSweepOptions { law: Some(&[(0.0, 0.0), (3.0, 1.0)]), join_spline_walls: true, tolerance: 1e-4 },
    )
    .unwrap();
    assert_close(solid_volume(&b, "radial screw"), 0.16 * TAU * (4.2 * 3.0 + 1.5), 1e-4, "radial screw sweep");
    assert_eq!(b.to_native().unwrap().faces.len(), 6, "a linear radial law needs four continuous walls and two caps");
    // Two constant intervals around a sloping interval, with a partial last turn: keep the
    // law's corners as face boundaries, while joining the half-turn pieces within each interval.
    let law = [(0.0, 0.0), (0.65, 0.0), (1.9, 0.4), (3.2, 0.4)];
    let b = Brep::screw_sweep_with_options(
        &tooth,
        [0.0; 3],
        [0.0, 0.0, 1.0],
        1.0,
        3.2,
        crate::sweeps::ScrewSweepOptions { law: Some(&law), join_spline_walls: true, tolerance: 1e-4 },
    )
    .unwrap();
    assert!(b.check(CheckLevel::All).unwrap().is_valid());
    let record = b.to_native().unwrap();
    let splines = record.faces.iter().filter(|f| matches!(f.surface, solidcore_record::brep::Surface::Sweep { .. })).count();
    assert_eq!(splines, 8, "two flanks across three law intervals and two sloping radial walls");
    assert_eq!(Brep::screw_sweep_radial(&tooth, [0.0; 3], [0.0, 0.0, 1.0], 1.0, 3.0, &[(2.0, 0.0), (1.0, 1.0)], 1e-4).err(), Some(Error::Argument));
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
    assert_eq!(Brep::extrude(&nan, [0.0, 0.0, 1.0], 1.0, 0.0, DraftCorner::default(), true).err(), Some(Error::Argument));
    let bad = [Curve::Nurbs { degree: 3, points: vec![[0.0, 0.0, 0.0, 1.0]; 2], knots: vec![0.0; 6] }];
    assert_eq!(Brep::extrude(&bad, [0.0, 0.0, 1.0], 1.0, 0.0, DraftCorner::default(), true).err(), Some(Error::Argument));
    for profile in [&nan[..], &bad[..], &[]] {
        assert_eq!(Brep::revolve_joining_arcs(profile, [0.0; 3], [0.0, 0.0, 1.0], TAU, true).err(), Some(Error::Argument));
    }
    assert_eq!(Brep::loft(&[vec![circle([0.0; 3], 1.0)]], 1, true).err(), Some(Error::Argument));
    let sq = polygon(&[[0.0; 3], [1.0, 0.0, 0.0], [1.0, 1.0, 0.0], [0.0, 1.0, 0.0]]);
    assert_eq!(Brep::revolve_joining_arcs(&sq, [0.0; 3], [0.0, 1.0, 0.0], 7.0, true).err(), Some(Error::Argument));
    assert_eq!(Brep::screw_sweep(&sq, [0.0; 3], [0.0, 0.0, 1.0], 0.0, 1.0, 1e-4).err(), Some(Error::Argument));
}

/// #21092: a circle of radius 2 in the XZ plane, swept round a circle of radius 30 in the same
/// plane, turning with it, moves within its own plane, so its walls lie on top of each other.
/// Fitting a trim curve to them, the drop of an edge onto the folded wall reached a span one
/// parameter step long across the wall's seam, whose midpoint rounds onto its start: split there,
/// its empty half passed, and the span came back unchanged for ever. The sweep must return, and
/// not with a valid sheet. Run on a thread so that a hang fails the test instead of stalling it.
#[test]
fn a_circle_swept_round_a_circle_in_its_own_plane_returns() {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let ring = [Curve::Arc { center: [30.0, 0.0, 0.0], axis: [0.0, -1.0, 0.0], reference: [1.0, 0.0, 0.0], radius: 2.0, start: 0.0, end: TAU }];
        let path = [
            Curve::Arc { center: [0.0; 3], axis: [0.0, 1.0, 0.0], reference: [1.0, 0.0, 0.0], radius: 30.0, start: 0.0, end: PI },
            Curve::Arc { center: [0.0; 3], axis: [0.0, 1.0, 0.0], reference: [-1.0, 0.0, 0.0], radius: 30.0, start: 0.0, end: PI },
        ];
        let valid =
            Brep::sweep(&ring, &path, SweepFrame::Follow, false).ok().and_then(|b| b.check(CheckLevel::All).ok()).is_some_and(|r| r.is_valid());
        let _ = tx.send(valid);
    });
    let valid = rx.recv_timeout(std::time::Duration::from_secs(60)).expect("the sweep did not return within 60 s");
    assert!(!valid, "a circle swept within its own plane is no valid sheet");
}
