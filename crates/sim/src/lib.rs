//! Analysis of a model's behaviour in heat and in moving air.
//!
//! Modelling says what a part *is*; this says what happens to it. Two questions are answered
//! here, and they are the same question twice: where does energy pile up, and where does it get
//! away. Heat conduction ([`thermal`]) answers it inside the solid, from the material's own
//! conductivity; air flow ([`flow`]) answers it outside, from the fluid's viscosity.
//!
//! The two are not yet coupled: a flow does not hand the conduction a film coefficient, and a
//! conduction does not tell the flow about buoyancy. A convection boundary takes the film
//! coefficient it is given.
//!
//! Everything runs on a lattice ([`grid`]): the analysis box cut into cubic cells, with each
//! cell told whether the model fills it. A lattice is not as sharp at a curved wall as a
//! body-fitted mesh of tetrahedra, and the trade is deliberate -- it costs accuracy at the
//! boundary and buys a method that cannot fail to mesh, the same lattice for both solvers, and
//! a discretisation whose error is the same along every axis. Where the shape is flat and
//! square to an axis, the lattice is exact.
//!
//! Both solvers are honest about what they cannot do. Conduction refuses a study nothing
//! settles, and reports the energy balance so a caller can see the answer close. A flow reports
//! the Reynolds number it actually ran, which at engineering speeds is well below the one asked
//! for, because the method cannot hold such a flow on any affordable lattice -- see
//! `docs/analysis-verification.md`, which is as much about the limits as about the checks.
//!
//! The mathematics is from published method descriptions, and every solver is checked against
//! a closed-form answer: a result nobody has compared with an exact solution is not a result.
//! See `docs/analysis-verification.md` for the cases and the errors they hold to.
//!
//! Nothing here reads or writes a document. A study is a plain description -- geometry,
//! materials, boundary conditions -- and a result is numbers over a lattice. The engine owns
//! turning a user's picks into one and the answer back into something to look at.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

pub mod cut;
pub mod elastic;
pub mod flow;
pub mod grid;
pub mod library;
pub mod linalg;
pub mod material;
pub(crate) mod par;
pub mod snap;
pub mod tag;
pub mod thermal;

#[cfg(test)]
mod test_shapes;

pub use grid::{Grid3, Occupancy};
pub use linalg::{CgReport, Stencil7};

/// Why an analysis could not be set up or could not be trusted.
///
/// Every one of these is a refusal, not a guess: an analysis that reports a number it cannot
/// stand behind is worse than one that reports nothing.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum SimError {
    /// The model cannot carry an analysis: nothing to analyse, or a box that is not a box.
    #[error("{0}")]
    Domain(String),
    /// The study is incomplete or says two things at once.
    #[error("{0}")]
    Setup(String),
    /// A material property the solver needs is absent from the card.
    #[error("{0}")]
    Material(String),
    /// The lattice would hold more cells than an analysis may.
    #[error("too fine: {cells} cells, and an analysis may hold {limit}; use a coarser lattice")]
    TooLarge { cells: u64, limit: u64 },
    /// The solver ran its iterations out without the residual falling far enough.
    #[error("did not settle: {iterations} iterations left a relative residual of {residual:e}")]
    NoConvergence { iterations: u32, residual: f64 },
    /// Nothing in the study fixes the answer's level, so it has no single solution.
    #[error("{0}")]
    Underdetermined(String),
    /// The equations are not positive definite, so conjugate gradients has nothing to find.
    /// Either a boundary condition is missing or the assembly contradicts itself.
    #[error("the equations are not positive definite at {0}; a boundary condition is missing or contradictory")]
    NotDefinite(&'static str),
    /// A number stopped being a number. The solver reports where, and keeps nothing.
    #[error("the solver lost the numbers at {0}")]
    NotFinite(&'static str),
}

pub type Result<T> = std::result::Result<T, SimError>;

/// Helper for the refusals above, so a call site reads as one line.
pub(crate) fn setup<T>(msg: impl Into<String>) -> Result<T> {
    Err(SimError::Setup(msg.into()))
}

pub(crate) fn domain<T>(msg: impl Into<String>) -> Result<T> {
    Err(SimError::Domain(msg.into()))
}
