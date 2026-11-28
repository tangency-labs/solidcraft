//! The evaluation entry points against closed-form values (NURBS Book §2.2, §3.2 and §4.5).

use super::basis::{basis_derivatives, binomial, local_basis, local_basis_array, Side};
use super::partials::{orders, tabulate, trig_derivative, MAX_PARTIAL_ORDER};
use super::uv::{
    closed, closed_curve, collapsed, domain, project, project_with, unwrap, Domain, Starts,
};
use super::{
    basis_ders, span_of, EvalCurve, EvalSurface, Vec3, MAX_EVAL_KNOTS, MAX_EVAL_POINTS,
    MAX_SPLINE_DEGREE,
};

const EPS: f64 = 1e-12;

/// Quadratic, five control points, interior knots 1 and 2 (NURBS Book Ex. 2.3's shape).
const KNOTS: [f64; 8] = [0.0, 0.0, 0.0, 1.0, 2.0, 3.0, 3.0, 3.0];

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() <= EPS
}

fn near3(a: Vec3, b: Vec3) -> bool {
    near(a.x, b.x) && near(a.y, b.y) && near(a.z, b.z)
}

#[test]
fn the_bounds_are_the_records() {
    assert_eq!(MAX_SPLINE_DEGREE, solidcore_record::brep::MAX_SPLINE_DEGREE);
    // A degree past the bound does not evaluate.
    let p = MAX_SPLINE_DEGREE + 1;
    let over = EvalCurve {
        degree: p,
        knots: [vec![0.0; p + 1], vec![1.0; p + 1]].concat(),
        ctrl: vec![[0.0, 0.0, 0.0, 1.0]; p + 1],
    };
    assert!(over.eval(0.5).is_none());
    let line = EvalCurve {
        degree: 1,
        knots: vec![0.0, 0.0, 1.0, 1.0],
        ctrl: vec![[0.0, 0.0, 0.0, 1.0], [1.0, 0.0, 0.0, 1.0]],
    };
    assert!(line.knots.len() <= MAX_EVAL_KNOTS && line.ctrl.len() <= MAX_EVAL_POINTS);
}

#[test]
fn span_of_takes_the_last_span_starting_at_or_before_t() {
    assert_eq!(span_of(&KNOTS, 2, 5, 0.0), 2);
    assert_eq!(span_of(&KNOTS, 2, 5, 1.5), 3);
    assert_eq!(span_of(&KNOTS, 2, 5, 2.0), 4);
    // The domain's end keeps the closing span; past either end the end spans extend.
    assert_eq!(span_of(&KNOTS, 2, 5, 3.0), 4);
    assert_eq!(span_of(&KNOTS, 2, 5, 9.0), 4);
    assert_eq!(span_of(&KNOTS, 2, 5, -1.0), 2);
}

#[test]
fn basis_ders_are_a_partition_of_unity_and_its_derivative() {
    for t in [0.0, 0.4, 1.0, 1.5, 2.7, 3.0] {
        let span = span_of(&KNOTS, 2, 5, t);
        let Some(d) = basis_ders(&KNOTS, span, 2, t) else {
            panic!("basis at {t}")
        };
        let sum: f64 = d[0].iter().take(3).sum();
        let dsum: f64 = d[1].iter().take(3).sum();
        assert!(near(sum, 1.0) && near(dsum, 0.0), "{t}: {sum} {dsum}");
    }
    // On [1, 2), N₂,₂ = (u − 1)²/2: at 1.5 it is 1/8, its derivative 1/2 (the third alive).
    let Some(d) = basis_ders(&KNOTS, 3, 2, 1.5) else {
        panic!("basis")
    };
    assert!(near(d[0][2], 0.125) && near(d[1][2], 0.5));
}

#[test]
fn local_basis_is_the_hat_and_its_slope() {
    // Degree 1 on [0, 1, 2]: the hat, 0.5 at 0.5 rising with slope 1, falling past 1.
    assert_eq!(
        local_basis(&[0.0, 1.0, 2.0], 0.5, Side::Right, 1),
        vec![0.5, 1.0]
    );
    assert_eq!(
        local_basis(&[0.0, 1.0, 2.0], 1.5, Side::Right, 1),
        vec![0.5, -1.0]
    );
    // Degree 0 at its right end: zero from the right, one from the left.
    assert_eq!(local_basis(&[0.0, 1.0], 1.0, Side::Right, 0), vec![0.0]);
    assert_eq!(local_basis(&[0.0, 1.0], 1.0, Side::Left, 0), vec![1.0]);
    // Too few knots give zeros.
    assert_eq!(local_basis(&[0.0], 0.0, Side::Right, 2), vec![0.0; 3]);
}

#[test]
fn local_basis_array_matches_local_basis() {
    let knots = [0.0, 1.0, 2.0, 3.0];
    let a = local_basis_array(&knots, 1.25, Side::Right, 2);
    let v = local_basis(&knots, 1.25, Side::Right, 2);
    assert_eq!(&a[..3], v.as_slice());
    assert!(a[3..].iter().all(|x| *x == 0.0));
}

#[test]
fn basis_derivatives_sum_to_one_and_zero() {
    let Some((first, ders)) = basis_derivatives(&KNOTS, 2, 5, 1.5, 2) else {
        panic!("basis")
    };
    assert_eq!((first, ders.len()), (1, 3));
    for k in 0..=2 {
        let sum: f64 = ders.iter().map(|d| d[k]).sum();
        assert!(
            near(sum, if k == 0 { 1.0 } else { 0.0 }),
            "order {k}: {sum}"
        );
    }
    assert!(
        basis_derivatives(&KNOTS, 2, 4, 1.5, 1).is_none(),
        "knot count mismatch"
    );
    assert!(basis_derivatives(&KNOTS, 2, 5, f64::NAN, 1).is_none());
}

#[test]
fn binomial_is_pascals_triangle() {
    assert_eq!(binomial(5, 2), 10.0);
    assert_eq!(binomial(4, 0), 1.0);
    assert_eq!(binomial(4, 4), 1.0);
    assert_eq!(binomial(2, 3), 0.0);
}

/// The quarter circle as a rational quadratic: (1, 0), (1, 1) with weight √½, (0, 1).
fn quarter() -> EvalCurve {
    let w = std::f64::consts::FRAC_1_SQRT_2;
    EvalCurve {
        degree: 2,
        knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        ctrl: vec![[1.0, 0.0, 0.0, 1.0], [w, w, 0.0, w], [0.0, 1.0, 0.0, 1.0]],
    }
}

#[test]
fn eval_curve_is_exact_on_a_rational_arc() {
    let c = quarter();
    for t in [0.0, 0.3, 0.5, 1.0] {
        let Some((p, d)) = c.eval(t) else {
            panic!("eval at {t}")
        };
        assert!(near(p.x * p.x + p.y * p.y, 1.0), "{t}: off the circle");
        assert!(near(p.x * d.x + p.y * d.y, 0.0), "{t}: not tangent");
    }
    // The start tangent is 2·w·(P₁ − P₀)/w₀ = (0, 2√½).
    let Some((_, d)) = c.eval(0.0) else {
        panic!("eval")
    };
    assert!(near3(
        d,
        Vec3::new(0.0, 2.0 * std::f64::consts::FRAC_1_SQRT_2, 0.0)
    ));
    assert!(EvalCurve {
        degree: 2,
        knots: vec![0.0; 3],
        ctrl: c.ctrl.clone()
    }
    .eval(0.5)
    .is_none());
    assert!(!closed_curve(&c));
}

/// The bilinear patch over the unit square with corner heights 0, 0, 0, 1: z = u·v.
fn saddle() -> EvalSurface {
    let ctrl = vec![
        [0.0, 0.0, 0.0, 1.0],
        [1.0, 0.0, 0.0, 1.0],
        [0.0, 1.0, 0.0, 1.0],
        [1.0, 1.0, 1.0, 1.0],
    ];
    EvalSurface::nurbs(
        [1, 1],
        [vec![0.0, 0.0, 1.0, 1.0], vec![0.0, 0.0, 1.0, 1.0]],
        [2, 2],
        ctrl,
    )
}

#[test]
fn eval_surface_and_its_partials_are_exact() {
    let s = saddle();
    let Some((p, du, dv)) = s.derivs(0.25, 0.5) else {
        panic!("derivs")
    };
    assert!(near3(p, Vec3::new(0.25, 0.5, 0.125)));
    assert!(near3(du, Vec3::new(1.0, 0.0, 0.5)) && near3(dv, Vec3::new(0.0, 1.0, 0.25)));
    let Some(d) = s.partials(0.25, 0.5) else {
        panic!("partials")
    };
    // S_uv = (0, 0, 1); everything of order two in one direction is zero.
    assert!(near3(d[1][1], Vec3::new(0.0, 0.0, 1.0)));
    assert!(near3(d[2][0], Vec3::ZERO) && near3(d[0][2], Vec3::ZERO));
    let plane = EvalSurface::Plane {
        origin: Vec3::ZERO,
        du: Vec3::X,
        dv: Vec3::Y,
    };
    assert!(near3(plane.eval(2.0, 3.0).0, Vec3::new(2.0, 3.0, 0.0)));
}

#[test]
fn partial_orders_and_tables() {
    assert_eq!(MAX_PARTIAL_ORDER, 4);
    assert_eq!(orders().count(), 15);
    assert!(orders().all(|(i, j)| i + j <= MAX_PARTIAL_ORDER));
    let Some(t) = tabulate(|i, j| Some(Vec3::new(i as f64, j as f64, 0.0))) else {
        panic!("table")
    };
    assert_eq!(t[3][1], Vec3::new(3.0, 1.0, 0.0));
    assert_eq!(t[4][4], Vec3::ZERO, "past the order the table is zero");
    assert!(tabulate(|_, _| Some(Vec3::new(f64::NAN, 0.0, 0.0))).is_none());
    assert!(tabulate(|i, _| (i < 2).then_some(Vec3::ZERO)).is_none());
}

#[test]
fn trig_derivatives_turn_by_quarter_periods() {
    // At t = 0: cos, sin = 1, 0; each derivative turns them by a quarter period.
    assert_eq!(trig_derivative(0.0, 0), (1.0, 0.0));
    assert_eq!(trig_derivative(0.0, 1), (0.0, 1.0));
    assert_eq!(trig_derivative(0.0, 2), (-1.0, 0.0));
    assert_eq!(trig_derivative(0.0, 3), (0.0, -1.0));
    assert_eq!(trig_derivative(0.3, 4), trig_derivative(0.3, 0));
    assert_eq!(trig_derivative(0.3, 5), trig_derivative(0.3, 1));
}

#[test]
fn domains_and_closure() {
    let plane = EvalSurface::Plane {
        origin: Vec3::ZERO,
        du: Vec3::X,
        dv: Vec3::Y,
    };
    assert_eq!(
        domain(&plane),
        Domain {
            range: [None, None],
            period: [None, None]
        }
    );
    let d = domain(&saddle());
    assert_eq!(d.range, [Some((0.0, 1.0)), Some((0.0, 1.0))]);
    assert_eq!(d.period, [None, None]);
    let EvalSurface::Nurbs {
        degree,
        knots,
        count,
        ctrl,
        ..
    } = saddle()
    else {
        panic!("nurbs")
    };
    assert_eq!(closed(degree, &knots, count, &ctrl), [false, false]);
}

#[test]
fn projection_finds_the_parameters() {
    let s = saddle();
    let dom = domain(&s);
    let Some((p, _, _)) = s.derivs(0.3, 0.7) else {
        panic!("derivs")
    };
    let Some([u, v]) = project(&s, &dom, p, None, 1e-9) else {
        panic!("projects")
    };
    assert!((u - 0.3).abs() < 1e-7 && (v - 0.7).abs() < 1e-7, "{u} {v}");
    let starts = Starts::default();
    let Some([u2, v2]) = project_with(&s, &dom, &starts, p, Some([0.5, 0.5]), 1e-9) else {
        panic!("projects")
    };
    assert!((u2 - 0.3).abs() < 1e-7 && (v2 - 0.7).abs() < 1e-7);
    assert!(
        project(&s, &dom, Vec3::new(0.5, 0.5, 5.0), None, 1e-9).is_none(),
        "far off the surface"
    );
}

#[test]
fn unwrap_and_poles() {
    let tau = std::f64::consts::TAU;
    assert!(near(unwrap(0.1 + tau, 0.0, Some(tau)), 0.1));
    assert_eq!(unwrap(7.0, 0.0, None), 7.0);
    let dom = Domain {
        range: [Some((0.0, tau)), Some((0.0, 1.0))],
        period: [Some(tau), None],
    };
    assert_eq!(collapsed([0.0, 1.0], &dom, 1e-6), [true, false]);
    assert_eq!(collapsed([1.0, 1.0], &dom, 1e-6), [false, false]);
}
