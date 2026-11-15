//! #1570 (mirror case `mat0076`): a body flattened by a near-singular matrix transform
//! leaves a bilinear face about 1e-10 wide along `v`. There the evaluator's zero-tangent
//! correction fails, and upstream's `SmSurfaceEval::EvalNormal` ignores `Eval`'s status: it
//! makes the normal from the matrix the failed evaluation wrote. The fork passes that
//! status on (tangency-labs/smlib 21dcf69), and so does the port: the local solve's first
//! `Converged` test then fails, the solve steps away from its (already converged) guess,
//! and `GlobalPointSolve` lands on another `v` of the flat direction (-4.4668e-5 against
//! the C++ -2.6622e-5), the same point to rounding. The brute-force UV trim drop of the
//! mirrored body's edges uses these solves.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers
use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::global_solver::{SolutionRequested, SolverOperation};
use solidcore::geom::vector2d::Point2;
use solidcore::geom::vector3d::Vec3;
use solidcore::kernel::Kernel;
use solidcore::nurbs::b_spline_surface::BSplineSurface;
use solidcore::nurbs::math::{CPoint, NOW};
use solidcore::nurbs::srf_basic::Surface as NurbSurface;
use solidcore::surface::surface::p2::SurfaceEval;
use solidcore::surface::surface::p4::NoObject;
use solidcore::surface::surface::Surface;

/// The flattened face's surface (degree 1 x 1).
fn flat_patch() -> Surface {
    let p = |x: f64, y: f64, z: f64| CPoint::new(x, y, z, NOW);
    let net = vec![
        vec![
            p(-1.564586024562466, 2.129471446876365, 14.76541302939685),
            p(-1.5645860252756822, 2.1294714478470844, 14.765413029181278),
        ],
        vec![
            p(6.494094165902751, -0.7731721560322133, 14.765413029396854),
            p(6.494094165189537, -0.7731721550614927, 14.765413029181282),
        ],
    ];
    let u = vec![-8.000000000000002, -8.000000000000002, 2.0, 2.0];
    let v = vec![-12.23703147038545, -12.23703147038545, 0.0, 0.0];
    let nurb = NurbSurface::new(net, 1, 1, u, v).unwrap();
    Surface::from(BSplineSurface::from_nurb(nurb, false))
}

fn domain() -> Extent2d {
    Extent2d::from_points(
        Point2::new(-8.000000000000002, -12.23703147038545),
        Point2::new(2.0, 0.0),
    )
}

/// The minimizing solution's `(u, v)` for `p`, as the brute-force drop asks for it.
fn closest(k: &Kernel, s: &Surface, p: Vec3) -> (f64, f64) {
    let sols = s
        .global_point_solve::<NoObject>(
            k,
            Some(NoObject),
            &domain(),
            SolverOperation::Minimize,
            p,
            1e-5,
            None,
            SolutionRequested::All,
        )
        .unwrap();
    let sol = sols.as_slice().first().expect("a solution");
    (sol.start.param(0).unwrap(), sol.start.param(1).unwrap())
}

/// The pinned C++'s solution (upstream's `EvalNormal` ignores `Eval`'s status).
const CPP_UV: (f64, f64) = (-6.217248937900877e-15, -2.6622142923926617e-05);

/// The fork passes `Eval`'s status on (`SER( Eval() )` in `SmSurfaceEval::EvalNormal`,
/// tangency-labs/smlib 21dcf69), so the first `Converged` test has no normal and the
/// solve steps along the flat direction to another `v`. The patch is bilinear and its
/// control points differ along `v` by under 1.3e-9 (each row's two points: at most
/// 7.2e-10, 9.8e-10 and 2.2e-10 per coordinate) over 12.24 parameter units, so its `v`
/// derivative is below 1.1e-10 everywhere and two solutions `|dv| < 5e-5` apart are the
/// same point to 5.5e-15. The solution is checked as that: the same point and the same
/// distance as C++'s, to 1e-14.
#[test]
fn global_point_solve_lands_on_the_flat_direction_of_a_near_flat_patch() {
    let k = Kernel::default();
    let s = flat_patch();
    // the brute-force drop's first sample point of the mirrored edge (C++ values)
    let p0 = Vec3::new(4.88235812709649, -0.19264343447977827, 14.765413029181282);
    let (u, v) = closest(&k, &s, p0);
    assert!(
        u.abs() < 1e-13 && (v - CPP_UV.1).abs() < 5e-5,
        "{u:e} {v:e}"
    );
    let at = |u: f64, v: f64| s.evaluate_point(&k, Point2::new(u, v)).unwrap();
    let (ours, cpp) = (at(u, v), at(CPP_UV.0, CPP_UV.1));
    assert!((ours - cpp).length() <= 1e-14, "{ours:?} {cpp:?}");
    assert!(
        ((ours - p0).length() - (cpp - p0).length()).abs() <= 1e-14,
        "{ours:?} {cpp:?}"
    );
}

#[test]
fn normal_after_a_failed_evaluation_is_the_evaluation_error() {
    let k = Kernel::default();
    let s = flat_patch();
    let mut e = SurfaceEval::with_uv(&k, Some(&s), CPP_UV.0, CPP_UV.1);
    // the zero-tangent correction fails at this point, upstream and in the port
    assert!(e.eval().is_err());
    // ... and the normal is that failure, as the fork's `SER( Eval() )` returns it
    let err = e.normal(&k).unwrap_err();
    assert_eq!(
        err.operation(),
        "surface::surface::p2::SurfaceEval::eval",
        "{err:?}"
    );
}
