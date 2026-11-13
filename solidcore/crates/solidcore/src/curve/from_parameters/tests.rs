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
    let t = uv_trim(&c, &Extent1d::from_raw(0.0, 90.0), [1.0, 1.0]).unwrap();
    let crate::curve::curve::CurveKind::BSpline(b) = t.kind() else {
        panic!("not a B-spline")
    };
    assert_eq!(b.gw_nurb().unwrap().knots(), &[0.0, 0.0, 90.0, 90.0]);
    let p = at(&k, &t, 45.0);
    assert_eq!((p.x, p.y), (1.0, 2.0));
}

/// A kernel line goes to the record as the kernel holds it, its point and its point plus its
/// vector, over the STEP parameters of its interval (#3137: as SolidCraft's flat export wrote
/// it), and back again unchanged.
#[test]
fn a_line_goes_back_as_the_kernel_holds_it() {
    let k = k();
    let line = Curve::from(Line::create_canonical(
        Point3::new(1.0, 2.0, 3.0),
        Vec3::new(2.0, 0.0, 0.0),
    ));
    let r = record_of(&k, &line, &Extent1d::from_raw(0.5, 2.0)).unwrap();
    assert_eq!(
        r.curve,
        Record::Line {
            start: [1.0, 2.0, 3.0],
            end: [3.0, 2.0, 3.0],
        }
    );
    assert_eq!((&r.nurbs, r.interval), (&None, [0.5, 2.0]));
    let back = from_parameters(&k, &r.curve, r.interval).unwrap();
    assert_eq!(record_of(&k, &back.curve, &back.interval).unwrap(), r);
}

/// A line whose STEP parameter runs against its NURBS parameter (`inside_out`: `s = -t`) goes to
/// the record with its vector reversed and its interval negated, so the interval increases; the
/// record's point at each end of it is the kernel's at that end, bit for bit, and the round
/// trip is the identity.
#[test]
fn a_reversed_line_goes_back_with_its_vector_turned() {
    let k = k();
    let line = Curve::from(Line::infinite(
        Point3::new(1.0, 2.0, 3.0),
        Vec3::new(2.0, 0.0, 0.0),
        3,
        true,
    ));
    let (lo, hi) = (0.5, 2.0);
    let r = record_of(&k, &line, &Extent1d::from_raw(lo, hi)).unwrap();
    let Record::Line { start, end } = r.curve else {
        panic!("not a line: {:?}", r.curve)
    };
    assert!(r.interval[0] < r.interval[1], "{:?}", r.interval);
    let on = |s: f64| {
        Point3::new(
            start[0] + s * (end[0] - start[0]),
            start[1] + s * (end[1] - start[1]),
            start[2] + s * (end[2] - start[2]),
        )
    };
    // the record's interval runs the kernel's ends in order
    assert_eq!(on(r.interval[0]), at(&k, &line, lo));
    assert_eq!(on(r.interval[1]), at(&k, &line, hi));
    let back = from_parameters(&k, &r.curve, r.interval).unwrap();
    assert_eq!(record_of(&k, &back.curve, &back.interval).unwrap(), r);
}

/// A circle goes to the record as its label on its own frame with its NURBS as its form, proved
/// on each other (bound 0), and comes back through `conic_on_form` to the same record. A circle
/// whose NURBS runs against its angle reverses the frame's y; a NURBS moved off its circle is
/// refused.
#[test]
fn a_circle_goes_back_as_its_label_on_its_form() {
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
    let r = record_of(&k, &c, &ivl).unwrap();
    assert_eq!(r.curve, circle);
    assert_eq!(r.nurbs, Some(quarter_form([1.0, 2.0, 0.0], 2.0)));
    assert_eq!(r.interval, [ivl.min(), ivl.max()]);
    let again = conic_on_form(&k, &r.curve, r.nurbs.as_ref().unwrap()).unwrap();
    assert_eq!(record_of(&k, &again, &ivl).unwrap(), r);

    // the same quarter, its NURBS running clockwise about the frame's z
    let opts = EllipseOptions {
        dim: 3,
        nurb: None,
        inside_out: true,
    };
    let x = Vec3::new(1.0, 0.0, 0.0);
    let y = Vec3::new(0.0, 1.0, 0.0);
    let turned = Ellipse::new_circle(
        &k,
        Point3::new(1.0, 2.0, 0.0),
        x,
        y,
        Extent1d::from_raw(0.0, 90.0),
        2.0,
        &opts,
    )
    .unwrap();
    let turned = Curve::from(turned);
    let crate::curve::curve::CurveKind::Ellipse(e) = turned.kind() else {
        panic!("not a conic")
    };
    let r = record_of(&k, &turned, &e.base().natural_interval()).unwrap();
    let Record::Circle { frame: f, .. } = &r.curve else {
        panic!("{:?}", r.curve)
    };
    assert_eq!(f.y_axis, [-0.0, -1.0, -0.0]);
    conic_on_form(&k, &r.curve, r.nurbs.as_ref().unwrap()).unwrap();

    // a NURBS 1e-9 off the circle's plane
    let Record::RationalSpline { knots, homogeneous } = quarter_form([1.0, 2.0, 1e-9], 2.0) else {
        unreachable!()
    };
    let moved = spline(&knots, &homogeneous, 3, OP).unwrap();
    let opts = EllipseOptions {
        dim: 3,
        nurb: Some(&moved),
        inside_out: false,
    };
    let off = Ellipse::new_circle(
        &k,
        Point3::new(1.0, 2.0, 0.0),
        x,
        y,
        Extent1d::from_raw(0.0, 90.0),
        2.0,
        &opts,
    )
    .unwrap();
    let off = Curve::from(off);
    assert!(record_of(&k, &off, &moved.natural_interval()).is_err());
}

/// A trim's record keeps its knots where the edge's record interval is its kernel one, maps
/// them onto [0, 1] for a line's, and scales its points by a plane's scales; `uv_trim` takes it
/// back to the same trim.
#[test]
fn a_trim_goes_back_onto_its_edges_record_interval() {
    let k = k();
    let c = Record2::Spline {
        knots: vec![0.0, 0.0, 4.0, 4.0],
        points: vec![[0.0, 0.0], [2.0, 4.0]],
    };
    let kernel = Extent1d::from_raw(0.0, 4.0);
    let t = uv_trim(&c, &kernel, [1.0, 1.0]).unwrap();
    assert_eq!(
        record_trim(&k, &t, &kernel, [0.0, 4.0], [1.0, 1.0]).unwrap(),
        c
    );
    let on_line = record_trim(&k, &t, &kernel, [0.0, 1.0], [2.0, 0.5]).unwrap();
    assert_eq!(
        on_line,
        Record2::Spline {
            knots: vec![0.0, 0.0, 1.0, 1.0],
            points: vec![[0.0, 0.0], [4.0, 2.0]],
        }
    );
    let back = uv_trim(&on_line, &kernel, [0.5, 2.0]).unwrap();
    assert_eq!(
        record_trim(&k, &back, &kernel, [0.0, 4.0], [1.0, 1.0]).unwrap(),
        c
    );
}

/// The unit circle about the origin of `frame([0, 0, 0])`, as the rational form of
/// `((1 - s²), 2 s, 0) / (1 + s²)` with `s(t) = (t - a)²` over [0, 1]: degree 4 in t, its
/// Bernstein coefficients from the power ones. `s` runs back from `a²` to 0 on [0, a]
/// and out to `(1 - a)²` after: the form turns clockwise, then anticlockwise.
fn backtracking_form(a: f64) -> Record {
    // power coefficients of s = t² - 2a t + a²
    let s = [a * a, -2.0 * a, 1.0];
    let mul = |p: &[f64], q: &[f64]| {
        let mut r = vec![0.0; p.len() + q.len() - 1];
        for (i, x) in p.iter().enumerate() {
            for (j, y) in q.iter().enumerate() {
                r[i + j] += x * y;
            }
        }
        r
    };
    let s2 = mul(&s, &s);
    let pad = |p: &[f64]| {
        (0..5)
            .map(|i| p.get(i).copied().unwrap_or(0.0))
            .collect::<Vec<_>>()
    };
    let one = pad(&[1.0]);
    let (s2, s1) = (pad(&s2), pad(&s));
    let xh: Vec<f64> = one.iter().zip(&s2).map(|(o, q)| o - q).collect();
    let yh: Vec<f64> = s1.iter().map(|v| 2.0 * v).collect();
    let wh: Vec<f64> = one.iter().zip(&s2).map(|(o, q)| o + q).collect();
    // power to Bernstein of degree 4: b_i = sum_{j<=i} C(i, j) / C(4, j) a_j
    let c = |n: usize, k: usize| -> f64 {
        (0..k).fold(1.0, |r, i| r * (n - i) as f64 / (i + 1) as f64)
    };
    let bern = |p: &[f64]| -> Vec<f64> {
        (0..5)
            .map(|i| (0..=i).map(|j| c(i, j) / c(4, j) * p[j]).sum())
            .collect()
    };
    let (bx, by, bw) = (bern(&xh), bern(&yh), bern(&wh));
    Record::RationalSpline {
        knots: vec![0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0, 1.0],
        homogeneous: (0..5).map(|i| [bx[i], by[i], 0.0, bw[i]]).collect(),
    }
}

/// The unit circle about the origin as `arcs` rational quadratic arcs of `deg` degrees each
/// from the x axis, on the knots `breaks` (`arcs + 1` of them).
fn arcs_form(deg: f64, breaks: &[f64]) -> Record {
    let arcs = breaks.len() - 1;
    let half = (deg / 2.0).to_radians();
    let w = half.cos();
    let mut h = Vec::new();
    for i in 0..arcs {
        let t0 = (deg * i as f64).to_radians();
        if i == 0 {
            h.push([t0.cos(), t0.sin(), 0.0, 1.0]);
        }
        h.push([(t0 + half).cos(), (t0 + half).sin(), 0.0, w]);
        let t1 = t0 + 2.0 * half;
        h.push([t1.cos(), t1.sin(), 0.0, 1.0]);
    }
    let mut knots = vec![breaks[0]; 3];
    for b in &breaks[1..arcs] {
        knots.extend([*b, *b]);
    }
    knots.extend([breaks[arcs]; 3]);
    Record::RationalSpline {
        knots,
        homogeneous: h,
    }
}

fn unit_circle() -> Record {
    Record::Circle {
        frame: frame([0.0, 0.0, 0.0]),
        radius: 1.0,
    }
}

/// B1: a form that turns back between two of sixteen even samples of its parameter (on
/// [0, 0.02], where every sample after the first is further round) is refused: its angular
/// speed's Bernstein coefficients are proved negative on a piece, not sampled. Its point set is
/// on the circle, so only the sense check can refuse it.
#[test]
fn a_form_that_turns_back_between_samples_is_refused() {
    let k = k();
    let form = backtracking_form(0.02);
    let Record::RationalSpline { knots, homogeneous } = &form else {
        unreachable!()
    };
    let nurbs = spline(knots, homogeneous, 3, OP).unwrap();
    let (o, x, y) = axes(&frame([0.0, 0.0, 0.0])).unwrap();
    assert!(curve_on_quadrics(&nurbs, &quadrics(o, x, y, 1.0, 1.0, true)).unwrap());
    let err = conic_on_form(&k, &unit_circle(), &form).unwrap_err();
    assert!(err.to_string().contains("anticlockwise"), "{err}");
}

/// B1: a form that runs 400° round (five 80° arcs, each step of sixteen less than half a
/// turn) is refused, not read as a 40° arc.
#[test]
fn a_form_past_a_whole_turn_is_refused() {
    let k = k();
    let form = arcs_form(80.0, &[0.0, 1.0, 2.0, 3.0, 4.0, 5.0]);
    let err = conic_on_form(&k, &unit_circle(), &form).unwrap_err();
    assert!(err.to_string().contains("anticlockwise"), "{err}");
    // three of them, 240°, go
    let c = conic_on_form(&k, &unit_circle(), &arcs_form(80.0, &[0.0, 1.0, 2.0, 3.0])).unwrap();
    let crate::curve::curve::CurveKind::Ellipse(e) = c.kind() else {
        panic!("not a conic")
    };
    let d = e.step_interval();
    assert!((d.max() - d.min() - 240.0).abs() < 1e-9, "{d:?}");
}

/// B1: a closed form on strongly uneven knots (three quarters on [0, 0.03], the last on
/// [0.03, 1]: one sixteenth of the parameter spans 270°) is the whole turn, not refused.
#[test]
fn an_unevenly_parameterised_closed_form_is_the_whole_turn() {
    let k = k();
    let form = arcs_form(90.0, &[0.0, 0.01, 0.02, 0.03, 1.0]);
    let c = conic_on_form(&k, &unit_circle(), &form).unwrap();
    let crate::curve::curve::CurveKind::Ellipse(e) = c.kind() else {
        panic!("not a conic")
    };
    let d = e.step_interval();
    assert_eq!(d.max() - d.min(), 360.0, "{d:?}");
}
