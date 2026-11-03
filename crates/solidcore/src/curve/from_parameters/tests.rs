use super::*;
use core::f64::consts::{FRAC_PI_2, PI, TAU};

fn k() -> Kernel {
    Kernel::default()
}

fn frame(o: [f64; 3]) -> Frame {
    Frame {
        origin: o,
        x_axis: [1.0, 0.0, 0.0],
        y_axis: [0.0, 1.0, 0.0],
    }
}

fn at(k: &Kernel, c: &Curve, t: f64) -> Vec3 {
    c.evaluate_point(k, t).unwrap()
}

fn near(a: Vec3, b: [f64; 3], tol: f64) {
    assert!(a.distance(p3(b)) <= tol, "{a:?} != {b:?}");
}

#[test]
fn a_line_runs_through_its_two_points() {
    let k = k();
    let e = from_parameters(
        &k,
        &Record::Line {
            start: [1.0, 2.0, 3.0],
            end: [4.0, 6.0, 3.0],
        },
        [0.0, 1.0],
    )
    .unwrap();
    near(at(&k, &e.curve, 0.0), [1.0, 2.0, 3.0], 0.0);
    near(at(&k, &e.curve, 1.0), [4.0, 6.0, 3.0], 0.0);
    assert_eq!((e.interval.min(), e.interval.max()), (0.0, 1.0));
}

/// The closed form: a circle's point at angle t is `o + r (cos t x + sin t y)`. The kernel's
/// parameter is its NURBS's, set to the interval in degrees, which is the angle on the quarter
/// grid (each quarter one rational span) and on the circle between; a quarter turn in radians is
/// exactly 90 degrees.
#[test]
fn a_circle_is_its_closed_form_in_degrees() {
    let k = k();
    let r = 3.0;
    let e = from_parameters(
        &k,
        &Record::Circle {
            frame: frame([1.0, 1.0, 2.0]),
            radius: r,
        },
        [0.0, FRAC_PI_2 * 3.0],
    )
    .unwrap();
    assert_eq!((e.interval.min(), e.interval.max()), (0.0, 270.0));
    for (deg, want) in [
        (0.0, [4.0, 1.0, 2.0]),
        (90.0, [1.0, 4.0, 2.0]),
        (180.0, [-2.0, 1.0, 2.0]),
        (270.0, [1.0, -2.0, 2.0]),
    ] {
        near(at(&k, &e.curve, deg), want, 1e-15 * 8.0);
    }
    for deg in [30.0, 200.0] {
        let p = at(&k, &e.curve, deg);
        assert!(
            (p.distance(Point3::new(1.0, 1.0, 2.0)) - r).abs() <= 1e-15 * 8.0 && p.z == 2.0,
            "{p:?}"
        );
    }
}

#[test]
fn an_arc_past_a_turn_moves_back_by_whole_turns() {
    let k = k();
    let e = from_parameters(
        &k,
        &Record::Circle {
            frame: frame([0.0; 3]),
            radius: 1.0,
        },
        [TAU, TAU + FRAC_PI_2],
    )
    .unwrap();
    assert_eq!((e.interval.min(), e.interval.max()), (0.0, 90.0));
    let e = from_parameters(
        &k,
        &Record::Ellipse {
            frame: frame([0.0; 3]),
            radii: [2.0, 1.0],
        },
        [-TAU - PI, -TAU],
    )
    .unwrap();
    assert_eq!((e.interval.min(), e.interval.max()), (-180.0, 0.0));
    near(at(&k, &e.curve, -90.0), [0.0, -1.0, 0.0], 1e-15 * 8.0);
}

/// A frame not orthonormal is refused (the kernel would silently rebuild it), as is a radius
/// of no length.
#[test]
fn a_skew_frame_or_no_radius_is_refused() {
    let k = k();
    let skew = Frame {
        origin: [0.0; 3],
        x_axis: [1.0, 0.0, 0.0],
        y_axis: [0.1, 1.0, 0.0],
    };
    assert!(from_parameters(
        &k,
        &Record::Circle {
            frame: skew,
            radius: 1.0
        },
        [0.0, 1.0]
    )
    .is_err());
    assert!(from_parameters(
        &k,
        &Record::Circle {
            frame: frame([0.0; 3]),
            radius: 0.0
        },
        [0.0, 1.0]
    )
    .is_err());
    assert!(from_parameters(
        &k,
        &Record::Line {
            start: [0.0; 3],
            end: [1.0, 0.0, 0.0]
        },
        [1.0, 0.0]
    )
    .is_err());
}

/// A rational spline keeps the record's homogeneous points as they are.
#[test]
fn a_rational_spline_keeps_its_homogeneous_points() {
    let k = k();
    let w = core::f64::consts::FRAC_1_SQRT_2;
    let h = vec![[1.0, 0.0, 0.0, 1.0], [w, w, 0.0, w], [0.0, 1.0, 0.0, 1.0]];
    let e = from_parameters(
        &k,
        &Record::RationalSpline {
            knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            homogeneous: h.clone(),
        },
        [0.0, 1.0],
    )
    .unwrap();
    let crate::curve::curve::CurveKind::BSpline(b) = e.curve.kind() else {
        panic!("not a B-spline")
    };
    let got: Vec<[f64; 4]> = b
        .gw_nurb()
        .unwrap()
        .cpts()
        .iter()
        .map(|c| [c.x, c.y, c.z, c.w])
        .collect();
    assert_eq!(got, h);
    near(at(&k, &e.curve, 0.5), [w, w, 0.0], 1e-15 * 4.0);
}

#[test]
fn a_polyline_has_a_segment_per_unit() {
    let k = k();
    let e = from_parameters(
        &k,
        &Record::Polyline {
            points: vec![[0.0; 3], [1.0, 0.0, 0.0], [1.0, 2.0, 0.0]],
        },
        [0.0, 2.0],
    )
    .unwrap();
    near(at(&k, &e.curve, 1.0), [1.0, 0.0, 0.0], 0.0);
    near(at(&k, &e.curve, 1.5), [1.0, 1.0, 0.0], 0.0);
}

fn quarter_form(o: [f64; 3], r: f64) -> Record {
    let w = core::f64::consts::FRAC_1_SQRT_2;
    let h = vec![
        [o[0] + r, o[1], o[2], 1.0],
        [(o[0] + r) * w, (o[1] + r) * w, o[2] * w, w],
        [o[0], o[1] + r, o[2], 1.0],
    ];
    Record::RationalSpline {
        knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        homogeneous: h,
    }
}

/// A circle on its NURBS form keeps that NURBS, over the angles of its ends; the bound is 0
/// (proved on the record's circle). A form moved off the circle is refused.
#[test]
fn a_circle_on_its_form_keeps_it_and_a_moved_form_is_refused() {
    let k = k();
    let circle = Record::Circle {
        frame: frame([1.0, 2.0, 0.0]),
        radius: 2.0,
    };
    let c = conic_on_form(&k, &circle, &quarter_form([1.0, 2.0, 0.0], 2.0)).unwrap();
    let crate::curve::curve::CurveKind::Ellipse(e) = c.kind() else {
        panic!("not a conic")
    };
    let ivl = e.base().natural_interval();
    near(at(&k, &c, ivl.min()), [3.0, 2.0, 0.0], 1e-15 * 8.0);
    near(at(&k, &c, ivl.max()), [1.0, 4.0, 0.0], 1e-15 * 8.0);
    assert!(conic_on_form(&k, &circle, &quarter_form([1.0, 2.0, 1e-9], 2.0)).is_err());
    assert!(conic_on_form(&k, &circle, &quarter_form([1.0, 2.0, 0.0], 2.0 + 1e-9)).is_err());
}

/// A pcurve's knots go affinely onto its edge's kernel interval; its points are the record's.
#[test]
fn a_trim_runs_over_its_edges_kernel_interval() {
    let k = k();
    let c = Record2::Spline {
        knots: vec![1.0, 1.0, 3.0, 3.0],
        points: vec![[0.0, 0.0], [2.0, 4.0]],
    };
    let t = uv_trim(&c, &Extent1d::from_raw(0.0, 90.0)).unwrap();
    let crate::curve::curve::CurveKind::BSpline(b) = t.kind() else {
        panic!("not a B-spline")
    };
    assert_eq!(b.gw_nurb().unwrap().knots(), &[0.0, 0.0, 90.0, 90.0]);
    let p = at(&k, &t, 45.0);
    assert_eq!((p.x, p.y), (1.0, 2.0));
}
