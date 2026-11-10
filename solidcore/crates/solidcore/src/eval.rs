//! Evaluation of curves and surfaces held as plain data, in their own parameters: points,
//! partial derivatives to the fourth order, normals, curvature and normal sections, the
//! parameters of a point on a surface, and the exact NURBS forms of the analytic kinds.
//!
//! Not upstream. This is the kernel's one evaluator for geometry given by value (the
//! [`geometry`] types: a frame and radii, a knot vector and homogeneous control points),
//! as applications built on solidcore hold it, with no [`crate::Kernel`] and no
//! [`crate::Brep`]. Cylinders, cones, spheres and tori are evaluated and inverted in
//! closed form in angle and height; planes in closed form; B-splines and NURBS through
//! the NLib evaluator ([`crate::nurbs::srf_basic::Surface::derivs`],
//! [`crate::nurbs::crv_basic::Curve::derivs`]), the one every kernel surface evaluates
//! with; revolutions turn their profile's derivatives by the rotation's own.
//!
//! References: Piegl & Tiller, *The NURBS Book* (2nd ed.), A2.1–A2.3, §4.5, A7.1 (the
//! rational quadratic arcs of [`form`]); do Carmo, *Differential Geometry of Curves and
//! Surfaces*, §3.2 (normal sections, Meusnier).

pub mod agreement;
pub mod check;
pub mod collapse;
pub mod curve;
pub mod form;
pub mod geometry;
pub mod label;
pub mod normal_section;
pub mod pcurve;
#[rustfmt::skip]
pub mod plan;
pub mod pole;
pub mod sphere;
pub mod spline;
pub mod surface;
pub mod uv;
mod vec;

pub use collapse::Collapse;
pub use curve::{CurveEval, CurveJet};
pub use geometry::{Curve, Curve2, Frame, Surface};
pub use spline::{
    EvalCurve, EvalSurface, Partials, MAX_EVAL_KNOTS, MAX_EVAL_POINTS, MAX_PARTIAL_ORDER,
    MAX_SPLINE_DEGREE,
};
pub use surface::{face_flux, Axes, SurfaceEval};
pub use uv::{Domain, Starts};

/// Why a curve or surface cannot be evaluated.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum EvalError {
    /// A kind or degree the evaluator does not take.
    #[error("not supported yet: {0}")]
    Unsupported(String),
    /// Malformed data (a knot count that does not match, a zero axis).
    #[error("invalid geometry: {0}")]
    Invalid(String),
    /// More knots or control points than the evaluator takes.
    #[error("too large: {0}")]
    TooLarge(String),
    /// No value at the parameters asked: not finite, or a vanishing weight there.
    #[error("cannot be evaluated: {0}")]
    Unevaluable(String),
    /// A degenerate point with no limit for the quantity asked (a pole where the normal's
    /// limit from the second derivatives vanishes too).
    #[error("degenerate: {0}")]
    Degenerate(String),
}

#[cfg(test)]
mod tests;
