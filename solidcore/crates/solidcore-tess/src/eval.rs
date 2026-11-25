//! The plan evaluator a [`crate::TessPlan`]'s surfaces are evaluated with, for applications that
//! evaluate those forms themselves (a GPU twin, a record evaluator built on the same forms): the
//! B-spline basis and span search, the bounds, the partial derivatives, and a point's
//! parameters on a surface. The kernel computes all of it (`solidcore::eval::plan`); this is the
//! display side's path to it, so CPU, GPU and application evaluators share one implementation.
//! A modelling application takes the same entry points from `solidcore_ops::bspline`.

/// The kernel's 3D vector, which the evaluator takes and returns.
pub use solidcore::Vec3;

/// The highest polynomial degree the evaluator accepts in either direction.
pub use solidcore::eval::plan::MAX_SPLINE_DEGREE;

/// The most knots one direction of an evaluator form may have.
pub use solidcore::eval::plan::MAX_EVAL_KNOTS;

/// The most control points one evaluator form may have.
pub use solidcore::eval::plan::MAX_EVAL_POINTS;

/// The knot span used at `t` (NURBS Book A2.1).
pub use solidcore::eval::plan::span_of;

/// The non-zero basis functions of degree `p` on a span and their first derivatives.
pub use solidcore::eval::plan::basis_ders;

/// B-spline basis functions from their local knots, and all those alive at a parameter.
pub mod basis {
    /// Which one-sided limit a basis evaluation takes at a knot.
    pub use solidcore::eval::plan::basis::Side;

    /// One basis function from its local knots and its derivatives up to order `n`.
    pub use solidcore::eval::plan::basis::local_basis;

    /// [`local_basis`] into a fixed array, with no allocation.
    pub use solidcore::eval::plan::basis::local_basis_array;

    /// The basis functions alive at `t` with their derivatives up to `order`.
    pub use solidcore::eval::plan::basis::basis_derivatives;

    /// The binomial coefficient `C(n, k)` as a float.
    pub use solidcore::eval::plan::basis::binomial;
}

/// Exact partial derivatives of a [`crate::EvalSurface`] up to the fourth order.
pub mod partials {
    /// The highest total order of [`Partials`].
    pub use solidcore::eval::plan::partials::MAX_PARTIAL_ORDER;

    /// A surface's partials at a point: `d[i][j]` is `∂^(i+j) S / ∂u^i ∂v^j`.
    pub use solidcore::eval::plan::partials::Partials;

    /// The pairs `(i, j)` with `i + j ≤ MAX_PARTIAL_ORDER`.
    pub use solidcore::eval::plan::partials::orders;

    /// A table of partials from `f(i, j)`; `None` when one is missing or not finite.
    pub use solidcore::eval::plan::partials::tabulate;

    /// The `k`-th derivatives of cos and sin at `t`.
    pub use solidcore::eval::plan::partials::trig_derivative;
}

/// A surface's parameter box and periods, and a point's parameters on it.
pub mod uv {
    /// The parameter box of a surface and its periods.
    pub use solidcore::eval::plan::uv::Domain;

    /// The parameter box and periods of a surface.
    pub use solidcore::eval::plan::uv::domain;

    /// Whether a rational tensor B-spline closes on itself along each direction.
    pub use solidcore::eval::plan::uv::closed;

    /// Whether a curve closes on itself.
    pub use solidcore::eval::plan::uv::closed_curve;

    /// The grid of surface points a projection searches from, evaluated once on first use.
    pub use solidcore::eval::plan::uv::Starts;

    /// The parameters of a point on a surface within `accept`.
    pub use solidcore::eval::plan::uv::project;

    /// [`project`] with the search grid kept in a [`Starts`].
    pub use solidcore::eval::plan::uv::project_with;

    /// A parameter moved by whole periods to within half a period of another.
    pub use solidcore::eval::plan::uv::unwrap;

    /// Along each direction, whether a point with the given parameter speeds is a pole.
    pub use solidcore::eval::plan::uv::collapsed;
}

#[cfg(test)]
mod tests {
    use super::basis::{Side, basis_derivatives, binomial, local_basis, local_basis_array};
    use super::partials::{MAX_PARTIAL_ORDER, orders, tabulate, trig_derivative};
    use super::uv::{
        Starts, closed, closed_curve, collapsed, domain, project, project_with, unwrap,
    };
    use super::{MAX_EVAL_KNOTS, MAX_EVAL_POINTS, MAX_SPLINE_DEGREE, Vec3, basis_ders, span_of};
    use crate::{EvalCurve, EvalSurface};

    const KNOTS: [f64; 8] = [0.0, 0.0, 0.0, 1.0, 2.0, 3.0, 3.0, 3.0];

    /// The basis through this crate: span, partition of unity, `N₂,₂ = (u−1)²/2` at 1.5, the
    /// hat, Pascal.
    #[test]
    fn the_basis_is_the_kernels() {
        assert_eq!(MAX_SPLINE_DEGREE, solidcore_record::brep::MAX_SPLINE_DEGREE);
        let line = EvalCurve {
            degree: 1,
            knots: vec![0.0, 0.0, 1.0, 1.0],
            ctrl: vec![[0.0; 4]; 2],
        };
        assert!(line.knots.len() <= MAX_EVAL_KNOTS && line.ctrl.len() <= MAX_EVAL_POINTS);
        assert_eq!(span_of(&KNOTS, 2, 5, 1.5), 3);
        let d = basis_ders(&KNOTS, 3, 2, 1.5).unwrap();
        assert!((d[0][2] - 0.125).abs() < 1e-15 && (d[1][2] - 0.5).abs() < 1e-15);
        let (first, ders) = basis_derivatives(&KNOTS, 2, 5, 1.5, 1).unwrap();
        assert_eq!(first, 1);
        assert!((ders.iter().map(|d| d[0]).sum::<f64>() - 1.0).abs() < 1e-15);
        assert_eq!(
            local_basis(&[0.0, 1.0, 2.0], 0.5, Side::Right, 1),
            vec![0.5, 1.0]
        );
        assert_eq!(local_basis_array(&[0.0, 1.0], 1.0, Side::Left, 0)[0], 1.0);
        assert_eq!(binomial(5, 2), 10.0);
    }

    /// Partials and parameters through this crate, on the saddle `z = uv`.
    #[test]
    fn partials_and_parameters_are_the_kernels() {
        assert_eq!(orders().count(), 15);
        assert_eq!(MAX_PARTIAL_ORDER, 4);
        assert!(tabulate(|_, _| Some(Vec3::ZERO)).is_some());
        assert_eq!(trig_derivative(0.0, 1), (-0.0, 1.0));
        let ctrl = vec![
            [0.0, 0.0, 0.0, 1.0],
            [1.0, 0.0, 0.0, 1.0],
            [0.0, 1.0, 0.0, 1.0],
            [1.0, 1.0, 1.0, 1.0],
        ];
        let unit = || vec![0.0, 0.0, 1.0, 1.0];
        let s = EvalSurface::nurbs([1, 1], [unit(), unit()], [2, 2], ctrl.clone());
        let d = s.partials(0.25, 0.5).unwrap();
        assert!((d[1][1] - Vec3::new(0.0, 0.0, 1.0)).length() < 1e-15);
        let dom = domain(&s);
        assert_eq!(
            closed([1, 1], &[unit(), unit()], [2, 2], &ctrl),
            [false, false]
        );
        let p = Vec3::new(0.3, 0.7, 0.21);
        let [u, v] = project(&s, &dom, p, None, 1e-9).unwrap();
        assert!((u - 0.3).abs() < 1e-7 && (v - 0.7).abs() < 1e-7);
        assert!(project_with(&s, &dom, &Starts::default(), p, None, 1e-9).is_some());
        assert_eq!(unwrap(7.0, 0.0, None), 7.0);
        assert_eq!(collapsed([1.0, 1.0], &dom, 1e-6), [false, false]);
        let line = EvalCurve {
            degree: 1,
            knots: unit(),
            ctrl: vec![[0.0, 0.0, 0.0, 1.0], [2.0, 0.0, 0.0, 1.0]],
        };
        assert!(!closed_curve(&line));
    }
}
