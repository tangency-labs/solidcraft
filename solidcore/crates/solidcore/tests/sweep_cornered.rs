//! tangency-labs/solidcore#3191 (solidcraft#13639): a sweep along a path with corners, built
//! piece by piece, its pieces meeting at the corners' mitres ([`cornered_sweep`]), held to
//! closed forms. Each path starts along `z` from
//! the origin, so a profile drawn in the `xy` plane is the section at its start; its first
//! corner is at `c = (0, 0, L1)`, where it turns to the direction `t1`.

#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::disallowed_methods
)] // test: the closed forms use trigonometry

use solidcore::curve::arc::ArcDefinition;
use solidcore::curve::curve::Curve;
use solidcore::curve::ellipse::Ellipse;
use solidcore::error::SolidError;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::{Point3, Vec3};
use solidcore::primitive::api_curves::{create_circle, create_line_segment};
use solidcore::sweep::cornered::cornered_sweep;
use solidcore::sweep::stations::{Laws, Path, Piece};
use solidcore::topology::brep::p1 as bp1;
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::{edge, vertex, NamingScheme};
use solidcore::{Brep, Kernel};
use std::f64::consts::{FRAC_PI_2, FRAC_PI_3, PI, TAU};

const A: f64 = 3.0;
const L1: f64 = 20.0;
const L2: f64 = 15.0;

/// The `a x a` square centred on the path's start, in the `xy` plane.
fn square(a: f64) -> Vec<Curve> {
    let h = a / 2.0;
    let p = [
        Point3::new(-h, -h, 0.0),
        Point3::new(h, -h, 0.0),
        Point3::new(h, h, 0.0),
        Point3::new(-h, h, 0.0),
    ];
    (0..4)
        .map(|i| Curve::from(create_line_segment(p[i], p[(i + 1) % 4]).unwrap()))
        .collect()
}

/// Up `z` by `L1` from the origin, then `L2` along `(cos turn, 0, sin turn)` turned from `z`
/// toward `x` by `turn`.
fn ell(turn: f64) -> (Path, Vec3) {
    let c = Point3::new(0.0, 0.0, L1);
    let t1 = Vec3::new(turn.sin(), 0.0, turn.cos());
    let path = Path {
        pieces: vec![
            Piece::Line {
                a: Point3::new(0.0, 0.0, 0.0),
                b: c,
            },
            Piece::Line {
                a: c,
                b: c + t1 * L2,
            },
        ],
    };
    (path, t1)
}

fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-10, &attrs).unwrap()
}

fn valid(k: &mut Kernel, b: &Brep) {
    let mut copy = b.clone_standing_for_self().unwrap();
    if copy.naming() == NamingScheme::Unknown {
        copy.use_arena_naming().unwrap();
    }
    let report = check_brep(k, &mut copy, CheckLevel::Level2).unwrap();
    assert!(report.lines().is_empty(), "{:?}", report.lines());
}

fn sweep(profile: &[Curve], path: &Path, laws: &Laws<'_>) -> (Kernel, Brep) {
    let mut k = Kernel::default();
    let b = cornered_sweep(&mut k, profile, Vec3::Z, path, laws).unwrap();
    (k, b)
}

fn rel(a: f64, b: f64) -> f64 {
    (a - b).abs() / b.abs()
}

/// The body's vertices on the plane through the corner `c` halving it: its normal `z + t1`.
fn on_mitre(k: &Kernel, b: &Brep, t1: Vec3) -> Vec<Point3> {
    let _ = k;
    let c = Point3::new(0.0, 0.0, L1);
    let n = Vec3::Z + t1;
    bp1::vertices(b, None)
        .unwrap()
        .into_iter()
        .map(|v| vertex::point(b, v).unwrap())
        .filter(|p| ((*p - c).dot(n) / n.length()).abs() <= 1e-9 * L1)
        .collect()
}

/// A square along an L at a right angle and at 60 degrees: the volume is `a^2 (L1 + L2)` (the
/// wedge the mitre adds to one run it takes from the other), 10 faces, valid; the corner's 4
/// vertices lie on the plane halving it, a rectangle `a` by `a / cos(theta / 2)`.
#[test]
fn a_square_along_an_l_keeps_its_area_times_the_length() {
    for turn in [FRAC_PI_2, FRAC_PI_3] {
        let (path, t1) = ell(turn);
        let (mut k, mut b) = sweep(&square(A), &path, &Laws::default());
        valid(&mut k, &b);
        assert_eq!(bp1::faces(&b, None).unwrap().len(), 10);
        let v = volume(&k, &mut b);
        assert!(rel(v, A * A * (L1 + L2)) <= 1e-9, "{turn}: {v}");
        let corner = on_mitre(&k, &b, t1);
        assert_eq!(corner.len(), 4, "{corner:?}");
        // the rectangle's sides: a across the turning plane, a / cos(theta / 2) in it
        let mut sides: Vec<f64> = (0..4)
            .flat_map(|i| (i + 1..4).map(move |j| (i, j)))
            .map(|(i, j)| corner[i].distance(corner[j]))
            .collect();
        sides.sort_by(f64::total_cmp);
        let stretched = A / (turn / 2.0).cos();
        assert!((sides[0] - A).abs() <= 1e-12 * L1 && (sides[1] - A).abs() <= 1e-12 * L1);
        assert!((sides[2] - stretched).abs() <= 1e-12 * L1, "{sides:?}");
        assert!((sides[3] - stretched).abs() <= 1e-12 * L1, "{sides:?}");
    }
}

/// A circle of radius `r` along the right-angled L: `pi r^2 (L1 + L2)`; its mitre edge is an
/// ellipse in the plane halving the corner, semi-axes `r` and `r / cos 45`.
#[test]
fn a_circle_along_an_l_meets_itself_in_an_ellipse() {
    let r = 2.0;
    let circle = vec![Curve::from(create_circle(Point3::ZERO, r).unwrap())];
    let (path, t1) = ell(FRAC_PI_2);
    let (mut k, mut b) = sweep(&circle, &path, &Laws::default());
    valid(&mut k, &b);
    let v = volume(&k, &mut b);
    assert!(rel(v, PI * r * r * (L1 + L2)) <= 1e-9, "{v}");
    let c = Point3::new(0.0, 0.0, L1);
    let n = (Vec3::Z + t1) * (1.0 / (Vec3::Z + t1).length());
    let mut found = false;
    for e in bp1::edges(&b, None).unwrap() {
        let curve = edge::curve(&b, e).unwrap().unwrap().clone();
        let ivl = edge::interval(&b, e).unwrap();
        let points: Vec<Point3> = (0..64)
            .map(|i| {
                curve
                    .evaluate_point(&k, ivl.evaluate(f64::from(i) / 64.0))
                    .unwrap()
            })
            .collect();
        if !points.iter().all(|p| ((*p - c).dot(n)).abs() <= 1e-9 * L1) {
            continue;
        }
        found = true;
        let d: Vec<f64> = points.iter().map(|p| p.distance(c)).collect();
        let (lo, hi) = d
            .iter()
            .fold((f64::MAX, 0.0_f64), |(l, h), &x| (l.min(x), h.max(x)));
        assert!((lo - r).abs() <= 1e-9 * r, "{lo}");
        assert!((hi - r * 2.0_f64.sqrt()).abs() <= 1e-9 * r, "{hi}");
    }
    assert!(found, "no mitre edge on the plane halving the corner");
}

/// A centred square scaled from 1 to `k` along the L: the scale is linear in arc length and
/// the mitre's wedges still cancel (each run's volume is `A L (s0^2 + s0 s1 + s1^2) / 3` for
/// a profile whose first moment about the path is zero), so the body is
/// `a^2 (L1 + L2) (k^2 + k + 1) / 3`.
#[test]
fn an_end_scale_along_an_l_takes_the_integral_of_the_area() {
    let scale = 2.0;
    let (path, _) = ell(FRAC_PI_2);
    let laws = Laws {
        end_scale: scale,
        ..Laws::default()
    };
    let (mut k, mut b) = sweep(&square(A), &path, &laws);
    valid(&mut k, &b);
    let v = volume(&k, &mut b);
    let want = A * A * (L1 + L2) * (scale * scale + scale + 1.0) / 3.0;
    assert!(rel(v, want) <= 1e-9, "{v} vs {want}");
}

/// A square path of side `s`, starting half way along a side (so it closes along a straight
/// side, not at a corner): 4 corners, no caps, `a^2 * 4 s`, valid.
#[test]
fn a_closed_square_path_is_a_frame() {
    let s = 30.0;
    let p = |x: f64, y: f64| Point3::new(x, y, 0.0);
    let corners = [
        p(s / 2.0, 0.0),
        p(s, 0.0),
        p(s, s),
        p(0.0, s),
        p(0.0, 0.0),
        p(s / 2.0, 0.0),
    ];
    let path = Path {
        pieces: corners
            .windows(2)
            .map(|w| Piece::Line { a: w[0], b: w[1] })
            .collect(),
    };
    // the profile square to the path's start (along x), centred on it
    let h = A / 2.0;
    let q = [
        Point3::new(s / 2.0, -h, -h),
        Point3::new(s / 2.0, h, -h),
        Point3::new(s / 2.0, h, h),
        Point3::new(s / 2.0, -h, h),
    ];
    let profile: Vec<Curve> = (0..4)
        .map(|i| Curve::from(create_line_segment(q[i], q[(i + 1) % 4]).unwrap()))
        .collect();
    let mut k = Kernel::default();
    let mut b = cornered_sweep(&mut k, &profile, Vec3::X, &path, &Laws::default()).unwrap();
    valid(&mut k, &b);
    let v = volume(&k, &mut b);
    assert!(rel(v, A * A * 4.0 * s) <= 1e-9, "{v}");
}

/// A twist along a path with corners is refused by name.
#[test]
fn what_is_not_built_is_refused_by_name() {
    let (path, _) = ell(FRAC_PI_2);
    let mut k = Kernel::default();
    let twist = Laws {
        twist: 1.0,
        ..Laws::default()
    };
    let refused = |r: solidcore::Result<Brep>| matches!(r, Err(SolidError::Unsupported { .. }));
    assert!(refused(cornered_sweep(
        &mut k,
        &square(A),
        Vec3::Z,
        &path,
        &twist
    )));
}

/// Up `z` by `L1`, a quarter arc of radius `r` turning to `x`, `L2` along `x`, a right-angled
/// corner to `y`, 12 along `y`: the path and its corner.
fn bend_then_corner(r: f64) -> (Path, Point3) {
    let z1 = Point3::new(0.0, 0.0, L1);
    let arc_end = Point3::new(r, 0.0, L1 + r);
    let corner = Point3::new(r + L2, 0.0, L1 + r);
    let path = Path {
        pieces: vec![
            Piece::Line {
                a: Point3::new(0.0, 0.0, 0.0),
                b: z1,
            },
            Piece::Arc {
                centre: Point3::new(r, 0.0, L1),
                axis: Vec3::Y,
                start: z1,
                angle: FRAC_PI_2,
            },
            Piece::Line {
                a: arc_end,
                b: corner,
            },
            Piece::Line {
                a: corner,
                b: corner + Vec3::Y * 12.0,
            },
        ],
    };
    (path, corner)
}

/// The surface kinds of the body's faces, as their debug names.
fn kinds(b: &Brep) -> Vec<String> {
    bp1::faces(b, None)
        .unwrap()
        .into_iter()
        .map(|f| {
            let s = solidcore::topology::face::p2::surface(b, f)
                .unwrap()
                .unwrap();
            format!("{:?}", s.kind())
        })
        .collect()
}

/// A square along a line, a quarter bend of radius 5, a line and a right-angled corner: the
/// volume is the area times the path's length (Pappus on the bend, the profile's centroid on
/// the path; the mitre's wedges cancel), valid, the bend's walls analytic (its two sides along
/// its axis cylinders, the kernel's `Cone` class (`surface/cone.rs`, "Cones and cylinders"), its
/// two square to it planes), and the corner's
/// 4 vertices on the plane halving it, a rectangle `a` by `a sqrt 2`.
#[test]
fn a_square_round_a_bend_and_a_corner_keeps_its_area_times_the_length() {
    let r = 5.0;
    let (path, corner) = bend_then_corner(r);
    let (mut k, mut b) = sweep(&square(A), &path, &Laws::default());
    valid(&mut k, &b);
    let length = L1 + FRAC_PI_2 * r + L2 + 12.0;
    let v = volume(&k, &mut b);
    assert!(rel(v, A * A * length) <= 1e-9, "{v}");
    let kinds = kinds(&b);
    let cylinders = kinds.iter().filter(|s| s.starts_with("Cone(")).count();
    assert_eq!(cylinders, 2, "{kinds:?}");
    let n = Vec3::X + Vec3::Y;
    let mitre: Vec<Point3> = bp1::vertices(&b, None)
        .unwrap()
        .into_iter()
        .map(|v| vertex::point(&b, v).unwrap())
        .filter(|p| ((*p - corner).dot(n) / n.length()).abs() <= 1e-9 * L1)
        .collect();
    assert_eq!(mitre.len(), 4, "{mitre:?}");
    let mut sides: Vec<f64> = (0..4)
        .flat_map(|i| (i + 1..4).map(move |j| (i, j)))
        .map(|(i, j)| mitre[i].distance(mitre[j]))
        .collect();
    sides.sort_by(f64::total_cmp);
    assert!((sides[0] - A).abs() <= 1e-12 * L1 && (sides[1] - A).abs() <= 1e-12 * L1);
    let stretched = A * 2.0_f64.sqrt();
    assert!((sides[2] - stretched).abs() <= 1e-12 * L1, "{sides:?}");
    assert!((sides[3] - stretched).abs() <= 1e-12 * L1, "{sides:?}");
}

/// A circle of radius `rho` about the origin in the `xy` plane, the kernel's analytic circle
/// class (`Ellipse::arc`).
fn analytic_circle(k: &Kernel, rho: f64) -> Vec<Curve> {
    let d = ArcDefinition {
        centre: Point3::ZERO,
        axis: Vec3::Z,
        reference: Vec3::X,
        radius: rho,
        start: 0.0,
        end: TAU,
    };
    vec![Curve::from(Ellipse::arc(k, &d).unwrap())]
}

/// The volume, validity and wall classes of a circle of radius 2 swept round the bend and the
/// corner: `pi r^2` times the path's length.
fn circle_round_the_bend(circle: &[Curve]) -> Vec<String> {
    let (r, rho) = (5.0, 2.0);
    let (path, _) = bend_then_corner(r);
    let (mut k, mut b) = sweep(circle, &path, &Laws::default());
    valid(&mut k, &b);
    let length = L1 + FRAC_PI_2 * r + L2 + 12.0;
    let v = volume(&k, &mut b);
    assert!(rel(v, PI * rho * rho * length) <= 1e-9, "{v}");
    kinds(&b)
}

/// The kernel's analytic circle round the bend: the bend's wall is a torus, the straight wall
/// before it a cylinder (the kernel files a cylinder as its `Cone` class:
/// `surface/cone.rs`, "Cones and cylinders"). A part turn of the rotational sweep is the
/// torus too (tangency-labs/solidcraft#14425).
#[test]
fn an_analytic_circle_round_a_bend_and_a_corner_turns_a_torus() {
    let k = Kernel::default();
    let kinds = circle_round_the_bend(&analytic_circle(&k, 2.0));
    let count = |name: &str| kinds.iter().filter(|s| s.starts_with(name)).count();
    assert_eq!((count("Torus("), count("Cone(")), (1, 1), "{kinds:?}");
}

/// A B-spline circle (`create_circle`) round the bend, a separate case: the sweep recognises
/// the circle in its B-spline form, so the bend's wall is the torus too
/// (tangency-labs/solidcraft#14425).
#[test]
fn a_b_spline_circle_round_a_bend_turns_a_torus() {
    let circle = vec![Curve::from(create_circle(Point3::ZERO, 2.0).unwrap())];
    let kinds = circle_round_the_bend(&circle);
    let count = |name: &str| kinds.iter().filter(|s| s.starts_with(name)).count();
    assert_eq!(
        (count("Torus("), count("SurfOfRevolution(")),
        (1, 0),
        "{kinds:?}"
    );
}

/// A corner straight after a bend, and a helix, are refused by name.
#[test]
fn a_corner_after_a_bend_and_a_helix_are_refused_by_name() {
    let r = 5.0;
    let z1 = Point3::new(0.0, 0.0, L1);
    let arc_end = Point3::new(r, 0.0, L1 + r);
    let corner_after_bend = Path {
        pieces: vec![
            Piece::Line {
                a: Point3::ZERO,
                b: z1,
            },
            Piece::Arc {
                centre: Point3::new(r, 0.0, L1),
                axis: Vec3::Y,
                start: z1,
                angle: FRAC_PI_2,
            },
            Piece::Line {
                a: arc_end,
                b: arc_end + Vec3::Y * 10.0,
            },
        ],
    };
    let mut k = Kernel::default();
    let refused = |r: solidcore::Result<Brep>| matches!(r, Err(SolidError::Unsupported { .. }));
    assert!(refused(cornered_sweep(
        &mut k,
        &square(A),
        Vec3::Z,
        &corner_after_bend,
        &Laws::default()
    )));
    let helix = Path {
        pieces: vec![Piece::Helix {
            centre: Point3::new(5.0, 0.0, 0.0),
            axis: Vec3::Z,
            start: Point3::ZERO,
            angle: PI,
            rise: 1.0,
        }],
    };
    assert!(refused(cornered_sweep(
        &mut k,
        &square(A),
        Vec3::Y,
        &helix,
        &Laws::default()
    )));
}
