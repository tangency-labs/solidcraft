//! `sweep::screw` (the SolidCraft fork's `smlib_screw_sweep`, #1162, #1163): a profile in
//! the half-plane `y = 0, x > 0` carried about `+z` through the origin.
//!
//! - Every B-spline wall, sampled five points across each knot span both ways, lies within
//!   the tolerance asked of its true screw surface, end spans included (#1162). A point's
//!   distance is taken in its meridian plane to the profile the motion carries there,
//!   which bounds its distance to the screw surface from above.
//! - A profile line parallel to the axis sweeps exact cylinders of its radius (#1163).
//! - A screw motion keeps volume as a revolution does: a square tooth's sweep is its area
//!   times `2 pi` times its centroid radius times the turns (Pappus).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers
use solidcore::curve::curve::Curve;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::{Point2, Point3, Vec3};
use solidcore::nurbs::b_spline_curve::{BSplineCurve, NurbCircleParam};
use solidcore::primitive::api_curves::create_line_segment;
use solidcore::surface::surface::SurfaceKind;
use solidcore::sweep::screw::{
    screw_sweep, screw_sweep_body, screw_sweep_radial, screw_sweep_radial_body,
};
use solidcore::topology::brep::{p1, p2};
use solidcore::topology::face;
use solidcore::SolidError;
use solidcore::{Brep, Kernel};

use std::f64::consts::PI;

const O: Point3 = Point3::new(0.0, 0.0, 0.0);
const Z: Vec3 = Vec3::new(0.0, 0.0, 1.0);

/// A profile in the meridian half-plane: polygon corners `(r, z)`, or one circle.
enum Meridian {
    Polygon(Vec<(f64, f64)>),
    Circle { r: f64, z: f64, radius: f64 },
}

impl Meridian {
    /// The distance in the meridian plane from `(r, z)` to the profile's boundary.
    fn distance(&self, r: f64, z: f64) -> f64 {
        match self {
            Self::Circle {
                r: cr,
                z: cz,
                radius,
            } => ((r - cr).hypot(z - cz) - radius).abs(),
            Self::Polygon(rz) => {
                let mut best = f64::INFINITY;
                for (&(ax, az), &(bx, bz)) in rz.iter().zip(rz.iter().cycle().skip(1)) {
                    let (dx, dz) = (bx - ax, bz - az);
                    let t = (((r - ax) * dx + (z - az) * dz) / (dx * dx + dz * dz)).clamp(0.0, 1.0);
                    best = best.min((r - ax - t * dx).hypot(z - az - t * dz));
                }
                best
            }
        }
    }

    fn curves(&self) -> Vec<Curve> {
        match self {
            Self::Polygon(rz) => rz
                .iter()
                .zip(rz.iter().cycle().skip(1))
                .map(|(&(ar, az), &(br, bz))| {
                    Curve::from(
                        create_line_segment(Point3::new(ar, 0.0, az), Point3::new(br, 0.0, bz))
                            .unwrap(),
                    )
                })
                .collect(),
            Self::Circle { r, z, radius } => {
                let frame = Axis2Placement::new(Point3::new(*r, 0.0, *z), Vec3::X, Vec3::Z);
                let c = BSplineCurve::create_circle_segment(
                    3,
                    &frame,
                    *radius,
                    0.0,
                    360.0,
                    NurbCircleParam::Quadratic,
                )
                .unwrap();
                vec![Curve::from(c)]
            }
        }
    }
}

/// The radial offset of a law at `turn` (piecewise linear, held beyond).
fn law_offset(law: &[(f64, f64)], turn: f64) -> f64 {
    let Some(&(t0, o0)) = law.first() else {
        return 0.0;
    };
    if turn <= t0 {
        return o0;
    }
    for w in law.windows(2) {
        let ((ta, oa), (tb, ob)) = (w[0], w[1]);
        if turn <= tb {
            return oa + (ob - oa) * (turn - ta) / (tb - ta);
        }
    }
    law.last().unwrap().1
}

/// The farthest the B-spline walls of `b` stray from the true screw surfaces of `m`,
/// and the cylinder faces' radii.
fn screw_deviation(
    k: &Kernel,
    b: &Brep,
    m: &Meridian,
    pitch: f64,
    turns: f64,
    law: &[(f64, f64)],
) -> (f64, Vec<f64>) {
    let mut worst: f64 = 0.0;
    let mut cylinders = Vec::new();
    for f in p1::faces(b, None).unwrap() {
        let surface = face::p2::surface(b, f).unwrap().unwrap();
        match surface.kind() {
            SurfaceKind::Cone(c) => {
                assert!(c.is_cylinder(), "a cone face");
                cylinders.push(c.radius(0.0));
            }
            SurfaceKind::BSpline(s) => {
                let can = s.canonical().unwrap();
                for u in can.u_knots.windows(2) {
                    for v in can.v_knots.windows(2) {
                        if u[1] <= u[0] || v[1] <= v[0] {
                            continue;
                        }
                        for a in 0..=4 {
                            for c in 0..=4 {
                                let uu = u[0] + (u[1] - u[0]) * f64::from(a) / 4.0;
                                let vv = v[0] + (v[1] - v[0]) * f64::from(c) / 4.0;
                                let q = surface.evaluate_point(k, Point2::new(uu, vv)).unwrap();
                                let r = q.x.hypot(q.y);
                                let mut theta = q.y.atan2(q.x);
                                if theta < 0.0 {
                                    theta += 2.0 * PI;
                                }
                                let mut best = f64::INFINITY;
                                for kk in -1..=(turns as i32 + 1) {
                                    let angle = theta + 2.0 * PI * f64::from(kk);
                                    if !(-1e-9..=2.0 * PI * turns + 1e-9).contains(&angle) {
                                        continue;
                                    }
                                    let turn = angle / (2.0 * PI);
                                    best = best.min(
                                        m.distance(r - law_offset(law, turn), q.z - pitch * turn),
                                    );
                                }
                                worst = worst.max(best);
                            }
                        }
                    }
                }
            }
            // the caps
            SurfaceKind::Plane(_) => {}
            _ => panic!("unexpected face surface"),
        }
    }
    (worst, cylinders)
}

fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-6, &attrs).unwrap()
}

/// SolidCraft's coil of case 2552: a 0.9 square from r 4.1 to 5, pitch 1.5, 10 high.
fn coil() -> Meridian {
    Meridian::Polygon(vec![(4.1, 0.0), (5.0, 0.0), (5.0, 0.9), (4.1, 0.9)])
}

/// The groove of an M10x1.5 thread (ISO 68-1).
fn m10_groove(p: f64) -> Meridian {
    let h = p * 3f64.sqrt() / 2.0;
    let slope = (PI / 6.0).tan();
    let r0 = 5.0 - 5.0 * h / 8.0;
    let w0 = p / 8.0;
    let r1 = 5.0 + 0.05 * p;
    let w1 = (p / 4.0 + 2.0 * (r1 - r0) * slope) / 2.0;
    Meridian::Polygon(vec![
        (r0, -p - w0),
        (r1, -p - w1),
        (r1, -p + w1),
        (r0, -p + w0),
    ])
}

#[test]
fn issue_1162_coil_walls_stay_within_the_tolerance() {
    let mut k = Kernel::default();
    let (p, turns) = (1.5, 10.0 / 1.5);
    let tol = p * 1e-4;
    let m = coil();
    let b = screw_sweep(&mut k, &m.curves(), O, Z, p, turns, tol).unwrap();
    assert!(p2::is_manifold_solid(&b).unwrap());
    let (dev, cylinders) = screw_deviation(&k, &b, &m, p, turns, &[]);
    assert!(dev <= tol, "a wall strays {dev}, {tol} asked");
    assert!(!cylinders.is_empty());
}

#[test]
fn issue_1162_round_wire_walls_stay_within_the_tolerance() {
    let mut k = Kernel::default();
    let (p, turns) = (1.5, 2.25);
    let tol = p * 1e-4;
    let m = Meridian::Circle {
        r: 4.55,
        z: 0.45,
        radius: 0.4,
    };
    let b = screw_sweep(&mut k, &m.curves(), O, Z, p, turns, tol).unwrap();
    assert!(p2::is_manifold_solid(&b).unwrap());
    let (dev, cylinders) = screw_deviation(&k, &b, &m, p, turns, &[]);
    assert!(dev <= tol, "a wall strays {dev}, {tol} asked");
    // a curved profile sweeps no cylinder
    assert!(cylinders.is_empty());
}

#[test]
fn issue_1163_axial_lines_sweep_exact_cylinders() {
    let mut k = Kernel::default();
    let (p, turns) = (1.5, 3.0);
    let tol = p * 1e-4;
    for (m, radii) in [
        (coil(), vec![4.1, 5.0]),
        (m10_groove(p), {
            let h = p * 3f64.sqrt() / 2.0;
            vec![5.0 - 5.0 * h / 8.0, 5.0 + 0.05 * p]
        }),
    ] {
        let b = screw_sweep(&mut k, &m.curves(), O, Z, p, turns, tol).unwrap();
        assert!(p2::is_manifold_solid(&b).unwrap());
        let (dev, cylinders) = screw_deviation(&k, &b, &m, p, turns, &[]);
        assert!(dev <= tol, "a wall strays {dev}, {tol} asked");
        // both lines along the axis, in half turns: at least 2 x 6 cylinder faces
        assert!(cylinders.len() >= 12, "{} cylinder faces", cylinders.len());
        for r in cylinders {
            assert!(
                radii.iter().any(|&x| (r - x).abs() <= 1e-12),
                "cylinder radius {r}, profile lines at {radii:?}"
            );
        }
    }
}

#[test]
fn square_tooth_volume_is_pappus() {
    let mut k = Kernel::default();
    let tooth = Meridian::Polygon(vec![(4.0, 0.0), (4.4, 0.0), (4.4, 0.4), (4.0, 0.4)]);
    let expected = 0.16 * 2.0 * PI * 4.2 * 3.0;
    for pitch in [1.0, -1.0] {
        let mut b = screw_sweep(&mut k, &tooth.curves(), O, Z, pitch, 3.0, 1e-4).unwrap();
        let v = volume(&k, &mut b);
        assert!(
            (v - expected).abs() <= 2e-3 * expected,
            "pitch {pitch}: volume {v}, expected {expected}"
        );
    }
}

#[test]
fn radial_runout_stays_within_the_tolerance() {
    let mut k = Kernel::default();
    let (p, turns) = (1.5, 4.0);
    let tol = p * 1e-4;
    let m = Meridian::Circle {
        r: 4.55,
        z: 0.45,
        radius: 0.4,
    };
    // full depth after a turn and a half, out again over the last
    let law = [(0.0, -0.3), (1.5, 0.0), (2.5, 0.0), (4.0, -0.3)];
    let b = screw_sweep_radial(&mut k, &m.curves(), O, Z, p, turns, &law, tol).unwrap();
    assert!(p2::is_manifold_solid(&b).unwrap());
    let (dev, _) = screw_deviation(&k, &b, &m, p, turns, &law);
    assert!(dev <= tol, "a wall strays {dev}, {tol} asked");
}

#[test]
fn out_of_range_inputs_are_refused() {
    let mut k = Kernel::default();
    let c = coil().curves();
    assert!(screw_sweep(&mut k, &c, O, Z, 0.0, 3.0, 1e-4).is_err());
    assert!(screw_sweep(&mut k, &c, O, Z, 1.0, 0.0, 1e-4).is_err());
    assert!(screw_sweep(&mut k, &c, O, Z, 1.0, 3.0, 0.0).is_err());
    assert!(screw_sweep(&mut k, &c, O, Vec3::ZERO, 1.0, 3.0, 1e-4).is_err());
    assert!(screw_sweep(&mut k, &[], O, Z, 1.0, 3.0, 1e-4).is_err());
    // a law with turns out of order, or past the turns
    let bad = [(1.0, 0.1), (0.5, 0.0)];
    assert!(screw_sweep_radial(&mut k, &c, O, Z, 1.0, 3.0, &bad, 1e-4).is_err());
    let late = [(0.0, 0.1), (4.0, 0.0)];
    assert!(screw_sweep_radial(&mut k, &c, O, Z, 1.0, 3.0, &late, 1e-4).is_err());
}

#[test]
fn body_variants_stop_before_the_solid_check() {
    // The `_body` variants give the same body as the checked entry points where it is a
    // solid, and refuse out-of-range input as `InvalidInput` (the C API's
    // `SMLIB_ERR_ARG`).
    let mut k = Kernel::default();
    let c = coil().curves();
    let checked = screw_sweep(&mut k, &c, O, Z, 1.5, 2.0, 1.5e-4).unwrap();
    let body = screw_sweep_body(&mut k, &c, O, Z, 1.5, 2.0, 1.5e-4).unwrap();
    assert!(p2::is_manifold_solid(&body).unwrap());
    assert_eq!(
        p1::faces(&body, None).unwrap().len(),
        p1::faces(&checked, None).unwrap().len()
    );
    let law = [(0.0, -0.2), (2.0, 0.0)];
    let body = screw_sweep_radial_body(&mut k, &c, O, Z, 1.5, 2.0, &law, 1.5e-4).unwrap();
    assert!(p2::is_manifold_solid(&body).unwrap());
    assert!(matches!(
        screw_sweep_body(&mut k, &c, O, Z, 0.0, 2.0, 1.5e-4),
        Err(SolidError::InvalidInput { .. })
    ));
    assert!(matches!(
        screw_sweep_radial_body(&mut k, &c, O, Z, 1.5, 2.0, &[(3.0, 0.0)], 1.5e-4),
        Err(SolidError::InvalidInput { .. })
    ));
}
