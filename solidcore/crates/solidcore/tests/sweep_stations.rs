//! `sweep::stations` (#3139): a sweep's stations from its laws, against closed forms.
//!
//! - a helix's rotation-minimizing frame after one turn is its start frame turned about the
//!   tangent by `-2 pi b / sqrt(a^2 + b^2)` (radius `a`, rise `b` per radian);
//! - a planar quarter arc turns the frame a quarter turn about its axis, and keeps a direction
//!   in the plane's normal fixed (the direction orientation agrees);
//! - a straight path with twist `T` and end scale `k` turns the profile by `T u` and scales it by
//!   `1 + (k - 1) u` at the fraction `u`;
//! - a right-angle corner: the frame after it is the quarter turn about `t_0 x t_1`, the mitre
//!   at it the eighth turn stretched by `sqrt 2` along `t_1 - t_0`;
//! - a straight rail diverging at slope `s` scales the profile by `(d_0 + s x) / d_0`; an arc
//!   rail of radius `R` about the path's start by `sqrt(R^2 - x^2) / R`;
//! - a rail that stops short is refused, naming it; so are a rail touching a station's plane,
//!   crossing it twice or lying in it; a polyline rail's vertex on a plane is one crossing;
//! - two rails: the profile spans them, scaled by their distance apart;
//! - a helix at a quarter turn: the start's principal normal goes to `cos psi N + sin psi B`,
//!   `psi = -b phi / sqrt(a^2 + b^2)`; right- and left-handed, after a corner;
//! - the rotation-minimizing property itself: from the stations of a tilted helix, a
//!   left-handed one and a tilted arc, each step's rotation has no part about the path;
//! - the parallel and direction orientations (the parallel one with the caller's least
//!   crossing angle), a profile normal against the path, and the laws' and paths' refusals.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::curve::curve::Curve;
use solidcore::math::{Point3, Vec3};
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::nurbs::math::CPoint;
use solidcore::primitive::api_curves::{create_arc, create_line_segment};
use solidcore::sweep::stations::{
    stations, Laws, Orientation, Path, Piece, RailFit, Station, Stop,
};
use solidcore::Kernel;
use std::f64::consts::{FRAC_PI_2, FRAC_PI_4, PI, TAU};

fn apply(s: &Station, v: Vec3) -> Vec3 {
    s.m[0] * v.x + s.m[1] * v.y + s.m[2] * v.z
}

/// Stations asked at fractions of the path's length.
fn stops(us: &[f64]) -> Vec<Stop> {
    us.iter().map(|&u| Stop::At(u)).collect()
}

fn near(a: Vec3, b: Vec3, tol: f64) -> bool {
    a.distance(b) <= tol
}

/// The largest of `| |c_i|^2 - 1 |` and `|c_i . c_j|` over a station's columns.
fn defect(s: &Station) -> f64 {
    let [a, b, c] = s.m;
    [
        (a.dot(a) - 1.0).abs(),
        (b.dot(b) - 1.0).abs(),
        (c.dot(c) - 1.0).abs(),
        a.dot(b).abs(),
        b.dot(c).abs(),
        a.dot(c).abs(),
    ]
    .into_iter()
    .fold(0.0, f64::max)
}

/// The signed angle about the unit `axis` from `a` to `b` (both square to it).
fn angle_about(axis: Vec3, a: Vec3, b: Vec3) -> f64 {
    a.cross(b).dot(axis).atan2(a.dot(b))
}

#[test]
fn a_helix_frame_turns_by_its_torsion_against_the_screw() {
    let k = Kernel::default();
    let (a, b) = (5.0, 2.0);
    let path = Path {
        pieces: vec![Piece::Helix {
            centre: Point3::new(0.0, 0.0, 0.0),
            axis: Vec3::Z,
            start: Point3::new(a, 0.0, 0.0),
            angle: TAU,
            rise: b,
        }],
    };
    let t0 = Vec3::new(0.0, a, b) * (1.0 / a.hypot(b));
    let st = stations(&k, &path, t0, &Laws::default(), &stops(&[0.0, 0.25, 1.0])).unwrap();
    for s in &st {
        assert!(defect(s) < 1e-13, "{}", defect(s));
    }
    let end = st[2];
    // after a whole turn the tangent is the start's; the frame has turned about it
    assert!(near(apply(&end, t0), t0, 1e-12), "{:?}", apply(&end, t0));
    let normal = Vec3::new(-1.0, 0.0, 0.0);
    let want = -TAU * b / a.hypot(b);
    let got = angle_about(t0, normal, apply(&end, normal));
    // the angle modulo a turn
    let diff = (got - want).rem_euclid(TAU);
    assert!(diff.min(TAU - diff) < 1e-12, "{got} vs {want}");
    assert!(near(end.at, Point3::new(a, 0.0, TAU * b), 1e-12));
}

#[test]
fn a_planar_quarter_arc_turns_the_frame_a_quarter_turn() {
    let k = Kernel::default();
    let r = 7.0;
    let path = Path {
        pieces: vec![Piece::Arc {
            centre: Point3::new(0.0, 0.0, 0.0),
            axis: Vec3::Z,
            start: Point3::new(r, 0.0, 0.0),
            angle: FRAC_PI_2,
        }],
    };
    let at = [0.0, 0.5, 1.0];
    for orientation in [Orientation::Perpendicular, Orientation::Direction(Vec3::Z)] {
        let laws = Laws {
            orientation,
            ..Laws::default()
        };
        let st = stations(&k, &path, Vec3::Y, &laws, &stops(&at)).unwrap();
        for (s, u) in st.iter().zip(at) {
            let phi = FRAC_PI_2 * u;
            assert!(near(
                s.at,
                Point3::new(r * phi.cos(), r * phi.sin(), 0.0),
                1e-12
            ));
            assert!(near(apply(s, Vec3::Z), Vec3::Z, 1e-12), "{orientation:?}");
            let want = Vec3::new(phi.cos(), phi.sin(), 0.0);
            assert!(near(apply(s, Vec3::X), want, 1e-12), "{orientation:?} {u}");
            assert!(defect(s) < 1e-13);
        }
    }
}

#[test]
fn a_straight_path_twists_and_scales_linearly() {
    let k = Kernel::default();
    let len = 12.0;
    let path = Path {
        pieces: vec![Piece::Line {
            a: Point3::new(0.0, 0.0, 0.0),
            b: Point3::new(0.0, 0.0, len),
        }],
    };
    let (twist, scale) = (1.5 * PI, 3.0);
    let laws = Laws {
        twist,
        end_scale: scale,
        ..Laws::default()
    };
    let at = [0.0, 0.3, 0.5, 1.0];
    let st = stations(&k, &path, Vec3::Z, &laws, &stops(&at)).unwrap();
    for (s, u) in st.iter().zip(at) {
        let k_u = 1.0 + (scale - 1.0) * u;
        let turn = twist * u;
        let want = Vec3::new(turn.cos(), turn.sin(), 0.0) * k_u;
        assert!(near(apply(s, Vec3::X), want, 1e-12), "{u}");
        assert!(near(apply(s, Vec3::Z), Vec3::Z, 1e-12), "{u}");
        assert!(near(s.at, Point3::new(0.0, 0.0, len * u), 1e-12));
    }
}

#[test]
fn a_right_angle_corner_turns_the_frame_and_mitres_its_station() {
    let k = Kernel::default();
    let path = Path {
        pieces: vec![
            Piece::Line {
                a: Point3::new(0.0, 0.0, 0.0),
                b: Point3::new(0.0, 0.0, 10.0),
            },
            Piece::Line {
                a: Point3::new(0.0, 0.0, 10.0),
                b: Point3::new(10.0, 0.0, 10.0),
            },
        ],
    };
    let joints = path.corner_fractions().unwrap();
    assert_eq!(joints.len(), 1);
    assert!((joints[0] - 0.5).abs() < 1e-15);
    assert_eq!(path.joints().unwrap(), joints);
    let at = [Stop::Corner(0), Stop::At(1.0)];
    let st = stations(&k, &path, Vec3::Z, &Laws::default(), &at).unwrap();
    let (mitre, end) = (st[0], st[1]);
    assert!(near(apply(&end, Vec3::Z), Vec3::X, 1e-12));
    assert!(near(apply(&end, Vec3::X), Vec3::Z * -1.0, 1e-12));
    // at the corner: the eighth turn about y, and x (along t_1 - t_0 there) stretched by sqrt 2
    let half = Vec3::new(FRAC_PI_4.sin(), 0.0, FRAC_PI_4.cos());
    assert!(
        near(apply(&mitre, Vec3::Z), half, 1e-12),
        "{:?}",
        apply(&mitre, Vec3::Z)
    );
    // x is the eighth turn of x, (x - z) / sqrt 2, along t_1 - t_0, stretched by sqrt 2: x - z
    let stretched = Vec3::X - Vec3::Z;
    assert!(
        near(apply(&mitre, Vec3::X), stretched, 1e-12),
        "{:?}",
        apply(&mitre, Vec3::X)
    );
    assert!(near(apply(&mitre, Vec3::Y), Vec3::Y, 1e-12));
    // a fraction is never a mitre: at the corner's fraction the station is the second
    // piece's start (turned the whole quarter), one ulp before it the first piece's end
    let half_way = joints[0];
    let before = f64::from_bits(half_way.to_bits() - 1);
    let at = stops(&[before, half_way]);
    let st = stations(&k, &path, Vec3::Z, &Laws::default(), &at).unwrap();
    assert!(near(apply(&st[0], Vec3::Z), Vec3::Z, 1e-12));
    assert!(near(apply(&st[1], Vec3::Z), Vec3::X, 1e-12));
    for s in &st {
        assert!(near(apply(s, Vec3::Y), Vec3::Y, 1e-12));
        assert!(
            (apply(s, Vec3::X).length() - 1.0).abs() < 1e-12,
            "no stretch"
        );
    }
    // a corner the path does not have
    let err = stations(&k, &path, Vec3::Z, &Laws::default(), &[Stop::Corner(1)]).unwrap_err();
    assert!(
        format!("{err:?}").contains("corner the path does not have"),
        "{err:?}"
    );
}

/// A straight path along x, the profile in the (y, z) plane.
fn along_x(len: f64) -> Path {
    Path {
        pieces: vec![Piece::Line {
            a: Point3::new(0.0, 0.0, 0.0),
            b: Point3::new(len, 0.0, 0.0),
        }],
    }
}

#[test]
fn a_diverging_straight_rail_scales_the_profile() {
    let k = Kernel::default();
    let (len, d0, slope) = (20.0, 4.0, 0.25);
    let rail = Curve::from(
        create_line_segment(
            Point3::new(0.0, d0, 0.0),
            Point3::new(len, d0 + slope * len, 0.0),
        )
        .unwrap(),
    );
    let at = [0.0, 0.5, 1.0];
    for fit in [RailFit::Scale, RailFit::Stretch] {
        let laws = Laws {
            rails: vec![&rail],
            rail_fit: fit,
            ..Laws::default()
        };
        let st = stations(&k, &along_x(len), Vec3::X, &laws, &stops(&at)).unwrap();
        for (s, u) in st.iter().zip(at) {
            let x = len * u;
            let k_u = (d0 + slope * x) / d0;
            assert!(near(apply(s, Vec3::Y), Vec3::Y * k_u, 1e-12), "{fit:?} {u}");
            let z = if fit == RailFit::Scale { k_u } else { 1.0 };
            assert!(near(apply(s, Vec3::Z), Vec3::Z * z, 1e-12), "{fit:?} {u}");
            assert!(near(s.at, Point3::new(x, 0.0, 0.0), 1e-12));
        }
    }
}

#[test]
fn an_arc_rail_scales_by_its_distance_across() {
    let k = Kernel::default();
    let (len, r): (f64, f64) = (6.0, 10.0);
    let start = (len / r).acos().to_degrees();
    let rail = Curve::from(create_arc(Point3::new(0.0, 0.0, 0.0), r, start, 90.0).unwrap());
    let laws = Laws {
        rails: vec![&rail],
        ..Laws::default()
    };
    let at = [0.0, 0.25, 0.5, 1.0];
    let st = stations(&k, &along_x(len), Vec3::X, &laws, &stops(&at)).unwrap();
    for (s, u) in st.iter().zip(at) {
        let x = len * u;
        let k_u = (r * r - x * x).sqrt() / r;
        assert!(
            near(apply(s, Vec3::Y), Vec3::Y * k_u, 1e-12),
            "{u}: {:?}",
            apply(s, Vec3::Y)
        );
    }
}

#[test]
fn a_rail_that_stops_short_is_refused() {
    let k = Kernel::default();
    let rail = Curve::from(
        create_line_segment(Point3::new(0.0, 3.0, 0.0), Point3::new(5.0, 3.0, 0.0)).unwrap(),
    );
    let laws = Laws {
        rails: vec![&rail],
        ..Laws::default()
    };
    let err = stations(&k, &along_x(20.0), Vec3::X, &laws, &stops(&[0.0, 1.0])).unwrap_err();
    assert!(format!("{err:?}").contains("does not reach"), "{err:?}");
}

/// SolidCraft's station count: a station per degree of turn. A quarter arc twisted a quarter
/// turn turns 180 degrees in all: 180 steps (as the rule's ceiling rounds pi over a degree),
/// 0 and 1 among the fractions; a straight path with no
/// twist takes the least, 16 steps.
#[test]
fn station_fractions_are_a_degree_of_turn_each() {
    use solidcore::sweep::stations::{fractions, MIN_SKIN_STATIONS};
    let arc = Path {
        pieces: vec![Piece::Arc {
            centre: Point3::new(0.0, 0.0, 0.0),
            axis: Vec3::Z,
            start: Point3::new(5.0, 0.0, 0.0),
            angle: FRAC_PI_2,
        }],
    };
    let laws = Laws {
        twist: FRAC_PI_2,
        ..Laws::default()
    };
    let f = fractions(&arc, &laws).unwrap();
    // the count as the rule rounds it: ceil(pi / (pi / 180)) steps
    let n = (PI / (PI / 180.0)).ceil() as usize;
    assert!(n == 180 || n == 181, "{n}");
    assert_eq!(f.len(), n + 1, "{}", f.len());
    assert_eq!((f[0], f[n]), (0.0, 1.0));
    let line = Path {
        pieces: vec![Piece::Line {
            a: Point3::new(0.0, 0.0, 0.0),
            b: Point3::new(0.0, 0.0, 3.0),
        }],
    };
    let f = fractions(&line, &Laws::default()).unwrap();
    assert_eq!(f.len(), MIN_SKIN_STATIONS + 1);
    assert!(!line.is_closed().unwrap());
}

/// A non-rational B-spline rail of degree `degree` through its control points (clamped knots
/// `inner` between 0 and 1).
fn bspline(points: &[[f64; 3]], degree: usize, inner: &[f64]) -> Curve {
    let cpw: Vec<CPoint> = points
        .iter()
        .map(|p| CPoint::new(p[0], p[1], p[2], 1.0))
        .collect();
    let mut knots = vec![0.0; degree + 1];
    knots.extend_from_slice(inner);
    knots.extend(std::iter::repeat_n(1.0, degree + 1));
    let n = cpw.len() - 1;
    Curve::from(BSplineCurve::from_data(3, n, knots.len() - 1, degree, &cpw, &knots).unwrap())
}

fn one_rail(rail: &Curve) -> Laws<'_> {
    Laws {
        rails: vec![rail],
        ..Laws::default()
    }
}

/// A polyline rail with its vertex exactly on a station's plane crosses once there: the root
/// at the knot both spans share is one root, and the profile scales by 5 / 3 there.
#[test]
fn a_polyline_rail_s_vertex_on_a_plane_is_one_crossing() {
    let k = Kernel::default();
    let rail = bspline(
        &[[0.0, 3.0, 0.0], [5.0, 5.0, 0.0], [10.0, 3.0, 0.0]],
        1,
        &[0.5],
    );
    let st = stations(
        &k,
        &along_x(10.0),
        Vec3::X,
        &one_rail(&rail),
        &stops(&[0.5]),
    )
    .unwrap();
    assert!(near(apply(&st[0], Vec3::Y), Vec3::Y * (5.0 / 3.0), 1e-12));
}

/// A cubic rail `x(s) = 18 s - 24 s^2 + 8 s^3` (control x 0, 6, 4, 2): `x - 4 = -4 (1 - 2 s)^2
/// (1 - s / 2)`, so it touches the plane `x = 4` at `s = 1/2` without crossing (a double
/// root; the distance is negative at both ends), and crosses `x = 3` twice (it rises to 4 and
/// falls to 2). It meets `x = 0` once, at its start, and `x = 1` once.
#[test]
fn a_rail_touching_or_crossing_twice_is_refused() {
    let k = Kernel::default();
    let pts = [
        [0.0, 3.0, 0.0],
        [6.0, 3.0, 0.0],
        [4.0, 3.0, 0.0],
        [2.0, 3.0, 0.0],
    ];
    let rail = bspline(&pts, 3, &[]);
    let laws = one_rail(&rail);
    let path = along_x(4.0);
    let st = stations(&k, &path, Vec3::X, &laws, &stops(&[0.0, 0.25])).unwrap();
    assert_eq!(st.len(), 2);
    let err = stations(&k, &path, Vec3::X, &laws, &stops(&[1.0])).unwrap_err();
    assert!(format!("{err:?}").contains("touching"), "{err:?}");
    let err = stations(&k, &path, Vec3::X, &laws, &stops(&[0.75])).unwrap_err();
    assert!(format!("{err:?}").contains("more than once"), "{err:?}");
}

#[test]
fn a_rail_in_a_station_s_plane_is_refused() {
    let k = Kernel::default();
    let rail = Curve::from(
        create_line_segment(Point3::new(0.0, 1.0, 0.0), Point3::new(0.0, 3.0, 0.0)).unwrap(),
    );
    let err = stations(&k, &along_x(5.0), Vec3::X, &one_rail(&rail), &stops(&[0.0])).unwrap_err();
    assert!(format!("{err:?}").contains("lying in"), "{err:?}");
}

/// Two rails `y = 2` and `y = -2 - s x`: the profile spans them, so `y` scales by
/// `k = (4 + s x) / 4` (and `z` by `k` too when scaled evenly), and the map's origin goes to
/// the first rail less the map of its start, `(x, 2 - 2 k, 0)`.
#[test]
fn two_rails_span_the_profile() {
    let k = Kernel::default();
    let (len, slope) = (8.0, 0.5);
    let a = Curve::from(
        create_line_segment(Point3::new(0.0, 2.0, 0.0), Point3::new(len, 2.0, 0.0)).unwrap(),
    );
    let b = Curve::from(
        create_line_segment(
            Point3::new(0.0, -2.0, 0.0),
            Point3::new(len, -2.0 - slope * len, 0.0),
        )
        .unwrap(),
    );
    let at = [0.0, 0.5, 1.0];
    for fit in [RailFit::Scale, RailFit::Stretch] {
        let laws = Laws {
            rails: vec![&a, &b],
            rail_fit: fit,
            ..Laws::default()
        };
        let st = stations(&k, &along_x(len), Vec3::X, &laws, &stops(&at)).unwrap();
        for (s, u) in st.iter().zip(at) {
            let x = len * u;
            let k_u = (4.0 + slope * x) / 4.0;
            assert!(near(apply(s, Vec3::Y), Vec3::Y * k_u, 1e-12), "{fit:?} {u}");
            let z = if fit == RailFit::Scale { k_u } else { 1.0 };
            assert!(near(apply(s, Vec3::Z), Vec3::Z * z, 1e-12), "{fit:?} {u}");
            assert!(near(apply(s, Vec3::X), Vec3::X, 1e-12), "{fit:?} {u}");
            let origin = Point3::new(x, 2.0 - 2.0 * k_u, 0.0);
            assert!(near(s.at, origin, 1e-12), "{fit:?} {u}: {:?}", s.at);
        }
    }
}

#[test]
fn rails_along_a_path_with_a_corner_are_unsupported() {
    let k = Kernel::default();
    let path = Path {
        pieces: vec![
            Piece::Line {
                a: Point3::new(0.0, 0.0, 0.0),
                b: Point3::new(5.0, 0.0, 0.0),
            },
            Piece::Line {
                a: Point3::new(5.0, 0.0, 0.0),
                b: Point3::new(5.0, 0.0, 5.0),
            },
        ],
    };
    let rail = Curve::from(
        create_line_segment(Point3::new(0.0, 3.0, 0.0), Point3::new(5.0, 3.0, 5.0)).unwrap(),
    );
    let err = stations(&k, &path, Vec3::X, &one_rail(&rail), &stops(&[0.0])).unwrap_err();
    assert!(
        matches!(err, solidcore::SolidError::Unsupported { .. }),
        "{err:?}"
    );
}

/// A helix of radius `a` and rise `b` per radian about `axis` (unit) through the origin,
/// starting at `a e` (`e` unit, square to the axis), for `angle` radians.
fn helix(axis: Vec3, e: Vec3, a: f64, b: f64, angle: f64) -> Piece {
    Piece::Helix {
        centre: Point3::new(0.0, 0.0, 0.0),
        axis,
        start: e * a,
        angle,
        rise: b,
    }
}

/// A helix's tangent and principal normal at `phi` (axis `z`, starting on `x`).
fn helix_frame(a: f64, b: f64, phi: f64) -> (Vec3, Vec3) {
    let c = a.hypot(b);
    let (s, co) = phi.sin_cos();
    (
        Vec3::new(-a * s, a * co, b) * (1.0 / c),
        Vec3::new(-co, -s, 0.0),
    )
}

/// The rotation-minimizing frame turns the start's principal normal `N_0` into
/// `cos psi N + sin psi B` at `phi`, `psi = -b phi / sqrt(a^2 + b^2)` (Frenet: `U' = 0 mod T`
/// iff `psi' = -tau`), here at a quarter turn, right- and left-handed.
#[test]
fn a_helix_at_a_quarter_turn_turns_its_normal_by_minus_the_torsion() {
    let k = Kernel::default();
    let a = 2.0_f64;
    for b in [0.5, -0.5] {
        let path = Path {
            pieces: vec![helix(Vec3::Z, Vec3::X, a, b, TAU)],
        };
        let (t0, n0) = helix_frame(a, b, 0.0);
        let st = stations(&k, &path, t0, &Laws::default(), &stops(&[0.25, 0.5])).unwrap();
        for (s, u) in st.iter().zip([0.25, 0.5]) {
            let phi = TAU * u;
            let (t, n) = helix_frame(a, b, phi);
            let psi = -b * phi / a.hypot(b);
            let want = n * psi.cos() + t.cross(n) * psi.sin();
            assert!(near(apply(s, t0), t, 1e-12), "{b} {u}");
            assert!(
                near(apply(s, n0), want, 1e-12),
                "{b} {u}: {:?}",
                apply(s, n0)
            );
            let (x, y) = (a * phi.cos(), a * phi.sin());
            assert!(near(s.at, Point3::new(x, y, b * phi), 1e-12), "{b} {u}");
        }
    }
}

/// A line along `z` into a helix about `z` (radius 2, rise 1/2) whose start tangent is
/// `t_h = (0, 2, 1/2) / c`: the corner is the least rotation about `z x t_h` (along `-x`), so
/// `x` stays; one helix turn is then the turn about `t_h` by `psi = -2 pi b / c`, so at the
/// end `z` goes to `t_h` and `x` to `x cos psi + (t_h x x) sin psi`.
#[test]
fn a_corner_then_a_helix_compose() {
    let k = Kernel::default();
    let (a, b) = (2.0_f64, 0.5);
    let c = a.hypot(b);
    let path = Path {
        pieces: vec![
            Piece::Line {
                a: Point3::new(a, 0.0, -5.0),
                b: Point3::new(a, 0.0, 0.0),
            },
            helix(Vec3::Z, Vec3::X, a, b, TAU),
        ],
    };
    let st = stations(&k, &path, Vec3::Z, &Laws::default(), &[Stop::At(1.0)]).unwrap();
    let th = Vec3::new(0.0, a, b) * (1.0 / c);
    let psi = -TAU * b / c;
    let want = Vec3::X * psi.cos() + th.cross(Vec3::X) * psi.sin();
    assert!(near(apply(&st[0], Vec3::Z), th, 1e-12));
    assert!(
        near(apply(&st[0], Vec3::X), want, 1e-12),
        "{:?}",
        apply(&st[0], Vec3::X)
    );
}

/// The rotation-minimizing property: the frame turns about no axis along the path. Over each
/// of 1000 steps the stations' rotation `R = M_(i+1) M_i^T` has the skew vector
/// `w = sin(theta) axis`; the property is `w . T = 0`, `T` the step's chord. For an exact
/// rotation-minimizing frame the chord and the axis are square to fourth order in the step
/// (2.5 degrees here: a computed 8e-14 relative); a frame that turns with the path's
/// torsion, the direction orientation along the helix's axis, gives `b / sqrt(a^2 + b^2)`,
/// 0.24. Tilted and left-handed helices and a tilted arc.
#[test]
fn the_frames_turn_about_no_axis_along_the_path() {
    let k = Kernel::default();
    let tilt = Vec3::new(1.0, 1.0, 1.0) * (1.0 / 3.0_f64.sqrt());
    let across = Vec3::new(1.0, -1.0, 0.0) * (1.0 / 2.0_f64.sqrt());
    let n = 1000;
    let at: Vec<f64> = (0..=n).map(|i| f64::from(i) / f64::from(n)).collect();
    let worst = |piece: Piece, laws: &Laws<'_>| -> f64 {
        let path = Path {
            pieces: vec![piece],
        };
        let st = stations(&k, &path, tilt, laws, &stops(&at)).unwrap();
        let mut worst = 0.0_f64;
        for w in st.windows(2) {
            let (m0, m1) = (w[0].m, w[1].m);
            // R = M1 M0^T, rows of M0^T are M0's columns: R e_j = sum_i M1 e_i (M0 e_i)_j
            let r = |row: usize, col: usize| -> f64 {
                (0..3)
                    .map(|i| {
                        let a = [m1[i].x, m1[i].y, m1[i].z][row];
                        let b = [m0[i].x, m0[i].y, m0[i].z][col];
                        a * b
                    })
                    .sum()
            };
            let skew = Vec3::new(r(2, 1) - r(1, 2), r(0, 2) - r(2, 0), r(1, 0) - r(0, 1)) * 0.5;
            let chord = w[1].at - w[0].at;
            let along = skew.dot(chord) / (skew.length() * chord.length());
            worst = worst.max(along.abs());
        }
        worst
    };
    let plain = Laws::default();
    let pieces = [
        helix(tilt, across, 2.0, 0.5, TAU),
        helix(tilt, across, 2.0, -0.5, TAU),
        Piece::Arc {
            centre: Point3::new(0.0, 0.0, 0.0),
            axis: tilt,
            start: across * 3.0,
            angle: 3.0,
        },
    ];
    for piece in pieces {
        let w = worst(piece, &plain);
        assert!(w < 1e-9, "{piece:?}: {w}");
    }
    let turning = Laws {
        orientation: Orientation::Direction(tilt),
        ..Laws::default()
    };
    let w = worst(helix(tilt, across, 2.0, 0.5, TAU), &turning);
    assert!((w - 0.5 / 2.0_f64.hypot(0.5)).abs() < 1e-3, "{w}");
}

/// The parallel orientation keeps the profile as it is, and refuses a station the path crosses
/// at no more than the least angle asked (here SolidCraft's 5 degrees) to its plane: a quarter
/// arc from `y` reaches `-x`, square to it.
#[test]
fn the_parallel_orientation_keeps_the_profile() {
    let k = Kernel::default();
    let path = Path {
        pieces: vec![Piece::Arc {
            centre: Point3::new(0.0, 0.0, 0.0),
            axis: Vec3::Z,
            start: Point3::new(5.0, 0.0, 0.0),
            angle: FRAC_PI_2,
        }],
    };
    let laws = Laws {
        orientation: Orientation::Parallel,
        min_crossing: 5.0_f64.to_radians(),
        ..Laws::default()
    };
    let st = stations(&k, &path, Vec3::Y, &laws, &stops(&[0.0, 0.5])).unwrap();
    for s in &st {
        for e in [Vec3::X, Vec3::Y, Vec3::Z] {
            assert!(near(apply(s, e), e, 1e-15));
        }
    }
    let p = 5.0 * FRAC_PI_4.cos();
    assert!(near(st[1].at, Point3::new(p, p, 0.0), 1e-12));
    let err = stations(&k, &path, Vec3::Y, &laws, &stops(&[1.0])).unwrap_err();
    assert!(format!("{err:?}").contains("least angle asked"), "{err:?}");
    // 84 degrees round, the tangent is 6 degrees off the plane: crossed at 5, not at 7
    let u = 84.0 / 90.0;
    stations(&k, &path, Vec3::Y, &laws, &stops(&[u])).unwrap();
    let seven = Laws {
        min_crossing: 7.0_f64.to_radians(),
        ..laws
    };
    assert!(stations(&k, &path, Vec3::Y, &seven, &stops(&[u])).is_err());
}

/// The direction orientation along a helix about `z` with the direction `z`: the frame is
/// `[t, r, t x r]`, `r` the unit part of `z` square to the tangent, carried from the start's.
#[test]
fn the_direction_orientation_along_a_helix_keeps_the_direction_across() {
    let k = Kernel::default();
    let (a, b) = (2.0_f64, 0.5);
    let path = Path {
        pieces: vec![helix(Vec3::Z, Vec3::X, a, b, TAU)],
    };
    let laws = Laws {
        orientation: Orientation::Direction(Vec3::Z),
        ..Laws::default()
    };
    let across = |t: Vec3| {
        let r = Vec3::Z - t * t.dot(Vec3::Z);
        r * (1.0 / r.length())
    };
    let (t0, _) = helix_frame(a, b, 0.0);
    let at = [0.25, 0.6];
    let st = stations(&k, &path, t0, &laws, &stops(&at)).unwrap();
    for (s, u) in st.iter().zip(at) {
        let (t, _) = helix_frame(a, b, TAU * u);
        assert!(near(apply(s, t0), t, 1e-12), "{u}");
        assert!(near(apply(s, across(t0)), across(t), 1e-12), "{u}");
    }
}

/// A profile normal against the path is the same profile plane: the stations agree, twist
/// included (it turns about the path's own direction either way).
#[test]
fn a_profile_normal_against_the_path_gives_the_same_stations() {
    let k = Kernel::default();
    let laws = Laws {
        twist: 1.0,
        end_scale: 2.0,
        ..Laws::default()
    };
    let at = stops(&[0.0, 0.3, 1.0]);
    let with = stations(&k, &along_x(4.0), Vec3::X, &laws, &at).unwrap();
    let against = stations(&k, &along_x(4.0), Vec3::X * -1.0, &laws, &at).unwrap();
    assert_eq!(with, against);
}

#[test]
fn out_of_range_paths_laws_and_fractions_are_refused() {
    let k = Kernel::default();
    let line = |a: [f64; 3], b: [f64; 3]| Piece::Line {
        a: Point3::new(a[0], a[1], a[2]),
        b: Point3::new(b[0], b[1], b[2]),
    };
    let refused = |path: &Path, laws: &Laws<'_>, at: &[Stop], why: &str| {
        let err = stations(&k, path, Vec3::X, laws, at).unwrap_err();
        assert!(format!("{err:?}").contains(why), "{why}: {err:?}");
    };
    let plain = Laws::default();
    let ends = [Stop::At(0.0), Stop::At(1.0)];
    let back = Path {
        pieces: vec![
            line([0.0; 3], [5.0, 0.0, 0.0]),
            line([5.0, 0.0, 0.0], [0.0; 3]),
        ],
    };
    refused(&back, &plain, &ends, "doubles back");
    let apart = Path {
        pieces: vec![
            line([0.0; 3], [5.0, 0.0, 0.0]),
            line([6.0, 0.0, 0.0], [9.0, 0.0, 0.0]),
        ],
    };
    refused(&apart, &plain, &ends, "do not meet");
    let path = along_x(5.0);
    let twist = Laws {
        twist: f64::INFINITY,
        ..Laws::default()
    };
    refused(&path, &twist, &ends, "twist not finite");
    for end_scale in [0.0, -1.0, f64::NAN] {
        let scale = Laws {
            end_scale,
            ..Laws::default()
        };
        refused(&path, &scale, &ends, "end scale");
    }
    let rail = Curve::from(
        create_line_segment(Point3::new(0.0, 3.0, 0.0), Point3::new(5.0, 3.0, 0.0)).unwrap(),
    );
    let three = Laws {
        rails: vec![&rail, &rail, &rail],
        ..Laws::default()
    };
    refused(&path, &three, &ends, "more than two");
    let twisted = Laws {
        rails: vec![&rail],
        twist: 1.0,
        ..Laws::default()
    };
    refused(&path, &twisted, &ends, "the rails set them");
    for u in [-0.1, 1.1, f64::NAN] {
        refused(&path, &plain, &[Stop::At(u)], "outside [0, 1]");
    }
}

/// A joint is a corner by the procedural sweep's G1 rule: unit tangents more than `1e-8`
/// apart. Two lines kinked by `1e-10` radians meet G1 (no corner, so rails may follow them);
/// kinked by `1e-7`, they meet at a corner.
#[test]
fn a_joint_is_a_corner_past_the_procedural_sweep_s_g1_rule() {
    let kinked = |a: f64| Path {
        pieces: vec![
            Piece::Line {
                a: Point3::new(0.0, 0.0, 0.0),
                b: Point3::new(5.0, 0.0, 0.0),
            },
            Piece::Line {
                a: Point3::new(5.0, 0.0, 0.0),
                b: Point3::new(5.0 + 5.0 * a.cos(), 5.0 * a.sin(), 0.0),
            },
        ],
    };
    assert!(kinked(1e-10).corner_fractions().unwrap().is_empty());
    assert_eq!(kinked(1e-7).corner_fractions().unwrap().len(), 1);
}

/// The caller's limits on twist and end scale are refused past, by name; the defaults take
/// any finite twist and positive scale.
#[test]
fn the_caller_s_twist_and_scale_limits_refuse_past_them() {
    let k = Kernel::default();
    let path = along_x(5.0);
    let at = stops(&[0.0, 1.0]);
    let free = Laws {
        twist: 5.0 * TAU,
        end_scale: 200.0,
        ..Laws::default()
    };
    stations(&k, &path, Vec3::X, &free, &at).unwrap();
    let limited = |twist: f64, end_scale: f64| Laws {
        twist,
        end_scale,
        max_twist: 4.0 * TAU,
        end_scale_within: [0.01, 100.0],
        ..Laws::default()
    };
    let err = stations(&k, &path, Vec3::X, &limited(5.0 * TAU, 1.0), &at).unwrap_err();
    assert!(format!("{err:?}").contains("twist past"), "{err:?}");
    for s in [0.005, 200.0] {
        let err = stations(&k, &path, Vec3::X, &limited(0.0, s), &at).unwrap_err();
        assert!(format!("{err:?}").contains("end scale outside"), "{err:?}");
    }
    stations(&k, &path, Vec3::X, &limited(-4.0 * TAU, 100.0), &at).unwrap();
}
