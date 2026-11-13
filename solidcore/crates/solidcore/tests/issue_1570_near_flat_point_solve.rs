//! #1570 (mirror case `mat0076`): a body flattened by a near-singular matrix transform
//! leaves a bilinear face about 1e-10 wide along `v`. There the evaluator's zero-tangent
//! correction fails, and upstream's `SmSurfaceEval::EvalNormal` ignores `Eval`'s status: it
//! makes the normal from the matrix the failed evaluation wrote. The port returned the
//! error and gave the local solve's first `Converged` test a stale normal. That test then
//! failed, the solve stepped away from its (already converged) guess, and
//! `GlobalPointSolve` landed on another `v` of the flat direction (-4.4668e-5 against the
//! C++ -2.6622e-5). The brute-force UV trim drop of the mirrored body's edges uses these
//! solves.
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

#[test]
fn global_point_solve_keeps_the_converged_guess_on_a_near_flat_patch() {
    let k = Kernel::default();
    let s = flat_patch();
    // the brute-force drop's first sample point of the mirrored edge (C++ values)
    let p0 = Vec3::new(4.88235812709649, -0.19264343447977827, 14.765413029181282);
    assert_eq!(
        closest(&k, &s, p0),
        (-6.217248937900877e-15, -2.6622142923926617e-05)
    );
}

#[test]
fn normal_after_a_failed_evaluation_uses_the_written_derivatives() {
    let k = Kernel::default();
    let s = flat_patch();
    let mut e = SurfaceEval::with_uv(
        &k,
        Some(&s),
        -6.217248937900877e-15,
        -2.6622142923926617e-05,
    );
    // the zero-tangent correction fails at this point, upstream and in the port
    assert!(e.eval().is_err());
    // ... but the normal is still made from the matrix (a unit vector, not an error)
    let n = e.normal(&k).expect("EvalNormal ignores Eval's status");
    assert!((n.length() - 1.0).abs() < 1e-12, "{n:?}");
}
