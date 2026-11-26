//! Curve and surface evaluation for applications: the kernel's B-spline basis, its evaluator
//! forms of a curve and a surface, their exact partial derivatives, and finding a point's
//! parameters on a surface (RFC 0003, item 1.8).
//!
//! The kernel computes all of it (`solidcore::eval::plan`, Piegl & Tiller, *The NURBS Book*,
//! 2nd ed., A2.1, A2.3, eq. 2.5, 2.9, 4.8, 4.20); this module is the stable path an application
//! takes to it, so an application's splines, fills and evaluators share the kernel's one
//! implementation instead of keeping their own. Nothing here holds application types: knots and
//! weights are plain numbers, control points homogeneous `[w·x, w·y, w·z, w]`, vectors the
//! kernel's [`Vec3`].

/// The kernel's 3D vector, which the evaluator takes and returns.
pub use solidcore::Vec3;

/// The highest polynomial degree the evaluator accepts in either direction (the B-rep record's
/// own bound).
pub use solidcore::eval::plan::MAX_SPLINE_DEGREE;

/// The most knots one direction of an evaluator form may have.
pub use solidcore::eval::plan::MAX_EVAL_KNOTS;

/// The most control points one evaluator form may have.
pub use solidcore::eval::plan::MAX_EVAL_POINTS;

/// A rational B-spline curve: degree, knots and homogeneous control points. Its
/// [`EvalCurve::eval`] is the point and first derivative at a parameter; outside its domain it
/// continues as the polynomial of the end span.
pub use solidcore::eval::plan::EvalCurve;

/// A surface in the evaluator's forms: a plane, a rational tensor B-spline
/// ([`EvalSurface::nurbs`]), or a profile curve turned about an axis. Point and normal, first and
/// second derivatives, and the partials up to [`partials::MAX_PARTIAL_ORDER`].
pub use solidcore::eval::plan::EvalSurface;

/// The knot span used at `t` for degree `p` and `n` control points: the last non-empty span
/// starting at or before `t`, clamped to `p ≤ k < n` (NURBS Book A2.1).
pub use solidcore::eval::plan::span_of;

/// The non-zero basis functions of degree `p` on a span and their first derivatives (NURBS
/// Book eq. 2.7); `ders[0][j]` is `N_{span−p+j}`, `ders[1][j]` its derivative.
pub use solidcore::eval::plan::basis_ders;

/// B-spline basis functions one at a time from their local knots, and all those alive at a
/// parameter (Cox–de Boor, eq. 2.5 and 2.9, with 0/0 = 0).
pub mod basis {
    /// Which one-sided limit a basis evaluation takes at a knot: `Right` on half-open spans
    /// `[uᵢ, uᵢ₊₁)`, `Left` on `(uᵢ, uᵢ₊₁]` (the closing span at a domain's right end).
    pub use solidcore::eval::plan::basis::Side;

    /// One basis function from its local knots `[ξ₀ … ξₚ₊₁]` and its derivatives up to order
    /// `n` (element `k` the `k`-th).
    pub use solidcore::eval::plan::basis::local_basis;

    /// [`local_basis`] into a fixed array, with no allocation.
    pub use solidcore::eval::plan::basis::local_basis_array;

    /// The `degree + 1` basis functions alive at `t` with their derivatives up to `order`: the
    /// first one's index and each one's derivatives.
    pub use solidcore::eval::plan::basis::basis_derivatives;

    /// The binomial coefficient `C(n, k)` as a float (zero for `k > n`).
    pub use solidcore::eval::plan::basis::binomial;
}

/// Exact partial derivatives of an [`EvalSurface`] up to the fourth order, and what tables of
/// them are built from.
pub mod partials {
    /// The highest total order of [`Partials`].
    pub use solidcore::eval::plan::partials::MAX_PARTIAL_ORDER;

    /// A surface's partials at a point: `d[i][j]` is `∂^(i+j) S / ∂u^i ∂v^j` for
    /// `i + j ≤ MAX_PARTIAL_ORDER`, zero past it.
    pub use solidcore::eval::plan::partials::Partials;

    /// The pairs `(i, j)` with `i + j ≤ MAX_PARTIAL_ORDER`, in table order.
    pub use solidcore::eval::plan::partials::orders;

    /// A table of partials from `f(i, j)`; `None` when one is missing or not finite.
    pub use solidcore::eval::plan::partials::tabulate;

    /// The `k`-th derivatives of cos and sin at `t`.
    pub use solidcore::eval::plan::partials::trig_derivative;
}

/// A surface's parameter box and periods, and a point's parameters on it.
pub mod uv {
    /// The parameter box of a surface (`None`: unbounded) and its periods.
    pub use solidcore::eval::plan::uv::Domain;

    /// The parameter box and periods of a surface.
    pub use solidcore::eval::plan::uv::domain;

    /// Whether a rational tensor B-spline closes on itself along each direction.
    pub use solidcore::eval::plan::uv::closed;

    /// Whether a curve closes on itself.
    pub use solidcore::eval::plan::uv::closed_curve;

    /// The grid of surface points a projection searches from, evaluated once on first use: keep
    /// one per surface when projecting many points onto it.
    pub use solidcore::eval::plan::uv::Starts;

    /// The parameters of a point on the surface within `accept`, from a hint when that
    /// converges, else from a grid over the domain.
    pub use solidcore::eval::plan::uv::project;

    /// [`project`] with the search grid kept in a [`Starts`].
    pub use solidcore::eval::plan::uv::project_with;

    /// A parameter moved by whole periods to within half a period of another.
    pub use solidcore::eval::plan::uv::unwrap;

    /// Along each direction, whether a point with the given parameter speeds is a pole or within
    /// `reach` of one.
    pub use solidcore::eval::plan::uv::collapsed;
}

#[cfg(test)]
#[path = "bspline_tests.rs"]
mod tests;
