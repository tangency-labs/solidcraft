//! A curve's certified length bracket against closed forms (#3184): every exact length lies
//! in its bracket, and the bracket narrows to the width asked, down to its rounding.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::curve::curve::{Curve, CurveKind};
use solidcore::curve::length_bound::MAX_PIECES;
use solidcore::error::curve::CurveFailure;
use solidcore::error::Reason;
use solidcore::geom::extent1d::Extent1d;
use solidcore::math::Point3;
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::nurbs::crv_basic::Curve as Nurb;
use solidcore::nurbs::math::{CPoint, NOW};
use solidcore::primitive::api_curves as curves;
use solidcore::{Kernel, SolidError};

fn spline(points: &[[f64; 3]], p: usize, knots: &[f64]) -> Curve {
    let cpts = points
        .iter()
        .map(|q| CPoint::new(q[0], q[1], q[2], NOW))
        .collect();
    let nurb = Nurb::new(cpts, p, knots.to_vec()).unwrap();
    Curve::from(BSplineCurve::from_gw_nurb(3, &nurb))
}

fn assert_holds(label: &str, c: &Curve, k: &Kernel, exact: f64, width: f64) {
    let ivl = c.natural_interval();
    let b = c.length_bound(k, &ivl, width).unwrap();
    println!(
        "LEN {label} exact={exact} lower={} upper={} width={:e}",
        b.lower,
        b.upper,
        b.upper - b.lower
    );
    assert!(
        b.lower <= exact && exact <= b.upper,
        "{label}: {exact} not in {b:?}"
    );
    assert!(
        b.upper - b.lower <= width,
        "{label}: width {:e}",
        b.upper - b.lower
    );
}

/// `int_0^1 sqrt(a t^2 + b t + c) dt`, `a > 0`, in closed form.
fn quadratic_root_integral(a: f64, b: f64, c: f64) -> f64 {
    let f = |t: f64| {
        let s = (a * t * t + b * t + c).sqrt();
        let u = 2.0 * a * t + b;
        let disc = 4.0 * a * c - b * b;
        u * s / (4.0 * a) + disc / (8.0 * a.powf(1.5)) * (2.0 * a.sqrt() * s + u).ln()
    };
    f(1.0) - f(0.0)
}

#[test]
fn a_circular_arc_is_its_radius_times_its_angle() {
    let k = Kernel::default();
    for (centre, sweep) in [
        (Point3::new(0.0, 0.0, 0.0), 90.0),
        (Point3::new(0.0, 0.0, 0.0), 270.0),
        (Point3::new(1e5, 1e5, 1e5), 120.0),
    ] {
        let r = 7.5;
        let c = Curve::from(curves::create_arc(centre, r, 10.0, 10.0 + sweep).unwrap());
        // (a rational quadratic B-spline: its pieces' cones, not the arc's closed form)
        assert!(matches!(c.kind(), CurveKind::BSpline(_)));
        let exact = r * sweep.to_radians();
        assert_holds(
            &format!("arc {sweep} at {:e}", centre.x),
            &c,
            &k,
            exact,
            1e-6 * exact,
        );
    }
}

#[test]
fn a_collinear_cubic_is_its_chord_whatever_its_speed() {
    let k = Kernel::default();
    // control points along x at 0, 1, 7, 10: the speed varies, the length is 10
    let c = spline(
        &[
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [7.0, 0.0, 0.0],
            [10.0, 0.0, 0.0],
        ],
        3,
        &[0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
    );
    assert_holds("collinear cubic", &c, &k, 10.0, 1e-9);
}

#[test]
fn a_parabola_and_its_degree_elevated_cubic_have_the_closed_form_length() {
    let k = Kernel::default();
    // B(t) = (1-t)^2 P0 + 2 t (1-t) P1 + t^2 P2, B' = 2 (P1 - P0) + 2 t (P2 - 2 P1 + P0)
    let (p0, p1, p2) = ([0.0, 0.0, 0.0], [1.0, 2.0, 0.0], [3.0, 0.0, 0.0]);
    let d0 = [2.0 * (p1[0] - p0[0]), 2.0 * (p1[1] - p0[1])];
    let dd = [
        2.0 * (p2[0] - 2.0 * p1[0] + p0[0]),
        2.0 * (p2[1] - 2.0 * p1[1] + p0[1]),
    ];
    let a = dd[0] * dd[0] + dd[1] * dd[1];
    let b = 2.0 * (d0[0] * dd[0] + d0[1] * dd[1]);
    let c = d0[0] * d0[0] + d0[1] * d0[1];
    let exact = quadratic_root_integral(a, b, c);
    let quad = spline(&[p0, p1, p2], 2, &[0.0, 0.0, 0.0, 1.0, 1.0, 1.0]);
    assert_holds("parabola", &quad, &k, exact, 1e-8 * exact);
    // the same curve as a cubic: Q0 = P0, Q1 = (P0 + 2 P1) / 3, Q2 = (2 P1 + P2) / 3, Q3 = P2
    let third = |x: [f64; 3], y: [f64; 3]| {
        [
            (x[0] + 2.0 * y[0]) / 3.0,
            (x[1] + 2.0 * y[1]) / 3.0,
            (x[2] + 2.0 * y[2]) / 3.0,
        ]
    };
    let cubic = spline(
        &[p0, third(p0, p1), third(p2, p1), p2],
        3,
        &[0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
    );
    assert_holds("elevated cubic", &cubic, &k, exact, 1e-8 * exact);
    // and over a sub-interval of a curve with an interior knot
    let split = spline(
        &[p0, third(p0, p1), third(p2, p1), p2],
        3,
        &[0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
    );
    let first_half = split
        .length_bound(&k, &Extent1d::from_min_max(0.0, 0.5), 1e-8)
        .unwrap();
    let half = {
        // int_0^0.5 of the same speed: t = s / 2
        0.5 * quadratic_root_integral(a * 0.25, b * 0.5, c)
    };
    // asked 1e-9 wide: below the floor its rounding allows over the pieces it needs
    // (a few 1e-9), so refused, naming that floor
    match split.length_bound(&k, &Extent1d::from_min_max(0.0, 0.5), 1e-9) {
        Err(SolidError::Failed(f)) => match f.reason {
            Reason::Curve(CurveFailure::LengthBelowFloor { asked, floor, .. }) => {
                assert_eq!(asked, 1e-9);
                assert!(floor > 1e-9 && floor < 1e-8, "{floor}");
            }
            other => panic!("another reason: {other:?}"),
        },
        other => panic!("not refused: {other:?}"),
    }
    assert!(
        first_half.lower <= half && half <= first_half.upper,
        "{half} {first_half:?}"
    );
}

/// The floor a refusal names: asked for a bracket 0 wide, a curve is halved until its widest
/// piece is mostly its own rounding and refused, naming the narrowest bracket it proves (the
/// pieces' derived rounding, summed) and their count; asked for that width, it returns a
/// bracket that wide, holding the exact length.
#[test]
fn a_bracket_asked_below_its_floor_is_refused_naming_it() {
    let k = Kernel::default();
    let floor_of = |c: &Curve| -> (f64, usize) {
        match c.length_bound(&k, &c.natural_interval(), 0.0) {
            Err(SolidError::Failed(f)) => match f.reason {
                Reason::Curve(CurveFailure::LengthBelowFloor {
                    asked,
                    floor,
                    pieces,
                }) => {
                    assert_eq!(asked, 0.0);
                    (floor, pieces)
                }
                other => panic!("another reason: {other:?}"),
            },
            other => panic!("not refused: {other:?}"),
        }
    };
    let arc = Curve::from(curves::create_arc(Point3::new(0.0, 0.0, 0.0), 2.0, 0.0, 60.0).unwrap());
    let parabola = spline(
        &[[0.0, 0.0, 0.0], [1.0, 2.0, 0.0], [3.0, 0.0, 0.0]],
        2,
        &[0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
    );
    for (label, c, exact) in [
        ("arc", &arc, 2.0 * 60.0_f64.to_radians()),
        ("parabola", &parabola, 3.754_636_412_317_107),
    ] {
        let (floor, pieces) = floor_of(c);
        println!("FLOOR {label} floor={floor:e} pieces={pieces}");
        assert!(
            floor > 0.0 && floor < 1e-7 * exact && pieces > 1,
            "{label}: {floor} {pieces}"
        );
        let b = c.length_bound(&k, &c.natural_interval(), floor).unwrap();
        assert!(b.lower <= exact && exact <= b.upper, "{label}: {b:?}");
        assert!(b.upper - b.lower <= floor, "{label}: {b:?}");
    }
}

/// The cap a refusal names: a cubic zigzag of 200 control points 0.01 apart, alternating 1
/// up and down, turns sharply about 200 times over a length near 400. Asked for a bracket
/// 1e-8 wide, which its rounding allows (its pieces are small), it needs more pieces than
/// the cap and is refused, naming the width asked, the width reached and the cap.
#[test]
fn a_bracket_past_the_piece_cap_is_refused_naming_it() {
    let k = Kernel::default();
    let n: i32 = 200;
    let points: Vec<[f64; 3]> = (0..n)
        .map(|i| {
            [
                0.01 * f64::from(i),
                if i % 2 == 0 { 1.0 } else { -1.0 },
                0.0,
            ]
        })
        .collect();
    let mut knots = vec![0.0; 4];
    knots.extend((1..n - 3).map(f64::from));
    knots.extend(vec![f64::from(n - 3); 4]);
    let zigzag = spline(&points, 3, &knots);
    match zigzag.length_bound(&k, &zigzag.natural_interval(), 1e-8) {
        Err(SolidError::Failed(f)) => match f.reason {
            Reason::Curve(CurveFailure::LengthPastPieceCap {
                asked,
                reached,
                pieces,
            }) => {
                println!("CAP asked={asked:e} reached={reached:e} pieces={pieces}");
                assert_eq!(asked, 1e-8);
                assert!(reached > asked && reached.is_finite(), "{reached}");
                assert_eq!(pieces, MAX_PIECES);
            }
            other => panic!("another reason: {other:?}"),
        },
        other => panic!("not refused: {other:?}"),
    }
}

/// The cone frustum's top arc (`issue_2343`'s `cone_frustum_corner_is_a_setback_beside_the_cone`,
/// as its corner meets it mid-session): a rational quadratic quarter circle of radius
/// `10 - 6 tan 30` at z = 6 over `[1, 1.25]`, the knots `0.75, 1, 1.25` (a span of 0.25 whose
/// largest knot is 1.25), its homogeneous points as dumped (#14434).
fn cone_arc() -> Curve {
    let s = std::f64::consts::FRAC_1_SQRT_2;
    let cpts = [
        [-1.2006250554860857e-15, -6.535898384862246, 6.0, 1.0],
        [4.621578069082297, -4.6215780690822985, 4.242640687119286, s],
        [6.535898384862246, 0.0, 6.0, 1.0],
        [4.621578069082298, 4.621578069082298, 4.242640687119286, s],
        [4.0020835182869525e-16, 6.535898384862246, 6.0, 1.0],
    ]
    .iter()
    .map(|q| CPoint::new(q[0], q[1], q[2], q[3]))
    .collect();
    let knots = vec![0.75, 0.75, 0.75, 1.0, 1.0, 1.25, 1.25, 1.25];
    let nurb = Nurb::new(cpts, 2, knots).unwrap();
    Curve::from(BSplineCurve::from_gw_nurb(3, &nurb))
}

/// The floor law (#14434): halving stops where the widest piece's narrowable part `g`, about
/// `kappa c^3` for a piece of chord `c`, is no more than its rounding `r`; with `x = g / r` in
/// `(1/8, 1]` there (a halving divides `g` by 8), the floor over a length `L` is `L (kappa
/// r^2)^(1/3) (1 + x) / x^(1/3)`, the last factor within `[1.89, 2.25]`. So the floor goes as
/// `r^(2/3)`. #3184 bounded a blossom level by `gamma_3 (1 + 6 K / S) 2 M`: on this arc (`p`
/// 2, `K / S` 5) the points' rounding `375 u M`; #14434 by `gamma_(9p+3) M`, `21 u M`. Its
/// floor is then #3184's, 9.672e-8 at 10784 pieces (measured, 3184-12), times `(21 /
/// 375)^(2/3)`, within the ratio of the last factor's ends, `[0.84, 1.19]`; and a bracket a
/// thousandth of the corner's tolerance (4e-5) wide, which #3184 refused, is certified.
#[test]
fn the_cone_frustum_arc_floor_follows_the_floor_law() {
    let k = Kernel::default();
    let arc = cone_arc();
    let ivl = Extent1d::from_min_max(1.0, 1.25);
    let rt = 10.0 - 6.0 * (std::f64::consts::PI / 6.0).tan();
    let exact = 0.5 * std::f64::consts::PI * rt;
    let b = arc.length_bound(&k, &ivl, 4e-8).unwrap();
    println!(
        "LEN cone arc exact={exact} {b:?} width={:e}",
        b.upper - b.lower
    );
    assert!(b.lower <= exact && exact <= b.upper, "{exact} not in {b:?}");
    assert!(b.upper - b.lower <= 4e-8, "{b:?}");
    let (floor, pieces) = match arc.length_bound(&k, &ivl, 0.0) {
        Err(SolidError::Failed(f)) => match f.reason {
            Reason::Curve(CurveFailure::LengthBelowFloor { floor, pieces, .. }) => (floor, pieces),
            other => panic!("another reason: {other:?}"),
        },
        other => panic!("not refused: {other:?}"),
    };
    let predicted = 9.672_260_325_999_105e-8 * (21.0_f64 / 375.0).powf(2.0 / 3.0);
    println!("FLOOR cone arc floor={floor:e} pieces={pieces} predicted={predicted:e}");
    let ratio = floor / predicted;
    assert!(
        (0.84..=1.19).contains(&ratio),
        "floor {floor:e}, predicted {predicted:e}"
    );
    assert!(pieces <= MAX_PIECES, "{pieces}");
}
