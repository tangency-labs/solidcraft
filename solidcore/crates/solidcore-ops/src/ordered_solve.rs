//! Portable opaque ordered spline-fit solve operators.
pub use kernel::MAX_SHARED_UNKNOWNS;
use solidcore::geom::ordered_solve as kernel;
/// Portable numerical-stage refusal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Failure {
    /// Buffer or owner dimensions disagree.
    InvalidShape,
    /// Banded Cholesky pivot refusal.
    FitSingular,
    /// Banded substitution is nonfinite.
    FitNonFinite,
    /// Dense Cholesky pivot refusal.
    PatchSingular,
    /// Dense substitution is nonfinite.
    PatchNonFinite,
    /// Schur reconstruction is nonfinite.
    BlocksNonFinite,
    /// Too many shared unknowns.
    SharedLimit {
        /// Number of shared scalar unknowns.
        given: usize,
    },
    /// Too many scalar unknowns.
    TooLarge,
    /// Bounded storage could not be allocated.
    Allocation,
}
fn classify(error: &solidcore::SolidError) -> Failure {
    use solidcore::error::nurbs::{LinearSolveFailure as F, NurbsFailure};
    match error.nurbs_failure() {
        Some(NurbsFailure::LinearSolve(reason)) => match reason {
            F::InvalidShape => Failure::InvalidShape,
            F::FitSingular => Failure::FitSingular,
            F::FitNonFinite => Failure::FitNonFinite,
            F::PatchSingular => Failure::PatchSingular,
            F::PatchNonFinite => Failure::PatchNonFinite,
            F::BlocksNonFinite => Failure::BlocksNonFinite,
            F::SharedLimit { given } => Failure::SharedLimit { given: *given },
            F::TooLarge => Failure::TooLarge,
            F::Allocation => Failure::Allocation,
        },
        _ => Failure::InvalidShape,
    }
}
/// Opaque symmetric upper-band storage and factor.
#[derive(Clone)]
pub struct Band(kernel::Band);
impl Band {
    /// Allocate a bounded matrix.
    pub fn new(n: usize, bw: usize) -> Result<Self, Failure> {
        kernel::Band::new(n, bw)
            .map(Self)
            .map_err(|error| classify(&error))
    }
    /// Add an upper-half entry; mirrored lower entries are ignored.
    pub fn add(&mut self, r: usize, s: usize, v: f64) {
        self.0.add(r, s, v);
    }
    /// Row-wise factorization and substitution.
    pub fn solve(self, b: Vec<f64>) -> Result<Vec<f64>, Failure> {
        self.0.solve(b).map_err(|error| classify(&error))
    }
    /// Right-looking factorization, kept for several right-hand sides.
    pub fn factor_rows(self) -> Result<Factor, Failure> {
        self.0
            .factor_rows()
            .map(Factor)
            .map_err(|error| classify(&error))
    }
}
/// Opaque Cholesky factor of a band.
pub struct Factor(kernel::Band);
impl Factor {
    /// Forward and back substitution of one right-hand side.
    pub fn substitute(&self, b: Vec<f64>) -> Result<Vec<f64>, Failure> {
        self.0.substitute(b).map_err(|error| classify(&error))
    }
}
/// Opaque dense, banded or patch-block normal equations.
pub struct Normal(kernel::Normal);
impl Normal {
    /// Choose the unchanged bandwidth-based representation.
    pub fn new(n: usize, bw: usize) -> Result<Self, Failure> {
        kernel::Normal::new(n, bw)
            .map(Self)
            .map_err(|error| classify(&error))
    }
    /// Retain scalar coordinate owners for the ordered Schur reduction.
    pub fn blocks(owner: Vec<Option<usize>>) -> Result<Self, Failure> {
        kernel::Normal::blocks(owner)
            .map(Self)
            .map_err(|error| classify(&error))
    }
    /// Add a matrix entry; callers provide both symmetric halves.
    pub fn add(&mut self, r: usize, s: usize, v: f64) {
        self.0.add(r, s, v);
    }
    /// Right-looking band, dense or ordered Schur solve.
    pub fn solve(self, b: Vec<f64>) -> Result<Vec<f64>, Failure> {
        self.0.solve(b).map_err(|error| classify(&error))
    }
}
