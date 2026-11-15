//! A taper (draft) extrude breaks its profile at G1 discontinuities through
//! `((SmBSplineCurve *)curve)->SubdivideAtDiscontinuity(..)` (`SmPrimitiveCreation.cpp`'s
//! `CreateTaperExtrude`) and builds its profile from the pieces only. Upstream's `SmLine` and
//! `SmEllipse` are `SmBSplineCurve`s that hold their NURB, so the split of a line returns its
//! one copy. The port builds a line's and a conic's NURB lazily. The split took a copy of
//! the bare B-spline base, which had none, `make_nurb` refused it (1001), and the dropped
//! status left no pieces, so the line was left out of the profile: a profile of lines made
//! "no composite curve" (tangency-labs/solidcraft#15938). The fork returns that status
//! (c8f6a31, WR-662), the only path since #14246 batch S5.
//!
//! Each draft below builds to the frustum's closed-form volume
//! `h/3 (A1 + A2 + sqrt(A1 A2))`, the top inset (draft > 0) or outset (draft < 0) by
//! `h tan |draft|`.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test

use solidcore::curve::curve::Curve;
use solidcore::curve::ellipse::Ellipse;
use solidcore::curve::line::Line;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::Point3;
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::sweep::api_brep::create_draft_sweep;
use solidcore::{Brep, Kernel, Vec3};

const SIDE: f64 = 4.0;
const HEIGHT: f64 = 2.0;
const RADIUS: f64 = 2.0;

fn kernel() -> Kernel {
    Kernel::default()
}

fn volume(k: &Kernel, body: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attributes = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, body, 1e-9, &attributes).expect("a volume")
}

/// The square's corners, closed.
fn corners() -> [Vec3; 5] {
    [
        Vec3::new(0.0, 0.0, 0.0),
        Vec3::new(SIDE, 0.0, 0.0),
        Vec3::new(SIDE, SIDE, 0.0),
        Vec3::new(0.0, SIDE, 0.0),
        Vec3::new(0.0, 0.0, 0.0),
    ]
}

/// The square's four sides: side `i` a `Line` where `lines[i]`, else a B-spline segment.
fn square(lines: [bool; 4]) -> Vec<Curve> {
    corners()
        .windows(2)
        .zip(lines)
        .map(|(w, is_line)| {
            if is_line {
                Curve::from(Line::from_points(w[0], w[1], 3))
            } else {
                Curve::from(BSplineCurve::create_line_segment(3, w[0], w[1]).unwrap())
            }
        })
        .collect()
}

/// The inset of the top per side.
fn inset(angle: f64) -> f64 {
    HEIGHT * angle.to_radians().tan()
}

fn square_frustum(angle: f64) -> f64 {
    let top = SIDE - 2.0 * inset(angle);
    HEIGHT / 3.0 * (SIDE * SIDE + top * top + SIDE * top)
}

fn cone_frustum(angle: f64) -> f64 {
    let top = RADIUS - inset(angle);
    std::f64::consts::PI * HEIGHT / 3.0 * (RADIUS * RADIUS + RADIUS * top + top * top)
}

/// Draft `curves` (linear corners, both caps): faces and volume, printed before any check.
fn draft(label: &str, curves: &[Curve], angle: f64) -> (usize, f64) {
    let mut k = kernel();
    let refs: Vec<Option<&Curve>> = curves.iter().map(Some).collect();
    let made = create_draft_sweep(&mut k, &refs, HEIGHT, angle, false, 3);
    let mut body = match made {
        Ok(body) => body,
        Err(e) => {
            println!("MEASURE {label} angle {angle}: Err {e:?}");
            panic!("{label}, angle {angle}: {e:?}");
        }
    };
    let faces = body.face_list().len();
    let v = volume(&k, &mut body);
    println!(
        "MEASURE {label} angle {angle}: faces {faces} edges {} vertices {} volume {v:?}",
        body.edge_list().len(),
        body.vertex_list().len()
    );
    (faces, v)
}

fn assert_frustum(label: &str, angle: f64, v: f64, want: f64, rel: f64) {
    assert!(
        (v - want).abs() <= rel * want,
        "{label}, angle {angle}: volume {v}, want {want}"
    );
}

/// The square of four lines drafts to the same body as the square of B-spline segments.
#[test]
fn a_square_of_lines_drafts_like_the_square_of_splines() {
    for angle in [5.0, -5.0] {
        let (spline_faces, spline_v) = draft("splines", &square([false; 4]), angle);
        let (faces, v) = draft("lines", &square([true; 4]), angle);
        let want = square_frustum(angle);
        assert_frustum("splines", angle, spline_v, want, 1e-12);
        assert_frustum("lines", angle, v, want, 1e-12);
        assert_eq!(faces, spline_faces, "lines, angle {angle}");
    }
}

/// Three lines and a B-spline segment: no line is left out of the profile.
#[test]
fn a_square_of_lines_and_a_spline_drafts_whole() {
    for angle in [5.0, -5.0] {
        let (spline_faces, _) = draft("splines", &square([false; 4]), angle);
        let (faces, v) = draft("mixed", &square([true, false, true, true]), angle);
        assert_frustum("mixed", angle, v, square_frustum(angle), 1e-12);
        assert_eq!(faces, spline_faces, "mixed, angle {angle}");
    }
}

/// A conic: the analytic circle `Ellipse::circle_from_3_points` makes drafts to a cone frustum.
#[test]
fn a_circle_drafts_to_a_cone_frustum() {
    for angle in [5.0, -5.0] {
        let k = kernel();
        let circle = Curve::from(
            Ellipse::circle_from_3_points(
                &k,
                Point3::new(RADIUS, 0.0, 0.0),
                Point3::new(0.0, RADIUS, 0.0),
                Point3::new(-RADIUS, 0.0, 0.0),
                3,
                true,
            )
            .unwrap(),
        );
        let (_, v) = draft("circle", &[circle], angle);
        assert_frustum("circle", angle, v, cone_frustum(angle), 1e-9);
    }
}
