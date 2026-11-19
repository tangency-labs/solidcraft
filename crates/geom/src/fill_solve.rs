//! Thin portable ordered-solve façade for the fill assemblers.
use crate::bspline::{SplineError, SplineResult};
use solidcore_ops::ordered_solve as ops;
fn failure(error: ops::Failure) -> SplineError {
    let text = match error {
        ops::Failure::InvalidShape => "the fit's equations are out of shape",
        ops::Failure::FitSingular => "the fit's equations are singular",
        ops::Failure::FitNonFinite => "the fit is not finite",
        ops::Failure::PatchSingular => "the patch's equations are singular",
        ops::Failure::PatchNonFinite => "the patch's equations have no finite solution",
        ops::Failure::BlocksNonFinite => "the patches' equations have no finite solution",
        ops::Failure::SharedLimit { given } => {
            return SplineError::TooLarge(format!("{given} unknowns shared by the patches about the centre (at most 4000)"));
        }
        ops::Failure::TooLarge => return SplineError::TooLarge("the ordered solve dimension exceeds its limit".into()),
        ops::Failure::Allocation => return SplineError::TooLarge("the ordered solve storage cannot be allocated".into()),
    };
    SplineError::Invalid(text.into())
}
#[derive(Clone)]
pub(crate) struct Band(ops::Band);
impl Band {
    pub(crate) fn new(n: usize, bw: usize) -> SplineResult<Self> {
        ops::Band::new(n, bw).map(Self).map_err(failure)
    }
    pub(crate) fn add(&mut self, r: usize, s: usize, v: f64) {
        self.0.add(r, s, v);
    }
    pub(crate) fn solve(self, b: Vec<f64>) -> SplineResult<Vec<f64>> {
        self.0.solve(b).map_err(failure)
    }
    pub(crate) fn factor_rows(self) -> SplineResult<Factor> {
        self.0.factor_rows().map(Factor).map_err(failure)
    }
}
pub(crate) struct Factor(ops::Factor);
impl Factor {
    pub(crate) fn substitute(&self, b: Vec<f64>) -> SplineResult<Vec<f64>> {
        self.0.substitute(b).map_err(failure)
    }
}
pub(crate) struct Normal(ops::Normal);
impl Normal {
    pub(crate) fn new(n: usize, bw: usize) -> SplineResult<Self> {
        ops::Normal::new(n, bw).map(Self).map_err(failure)
    }
    pub(crate) fn blocks(owner: Vec<Option<usize>>) -> SplineResult<Self> {
        ops::Normal::blocks(owner).map(Self).map_err(failure)
    }
    pub(crate) fn add(&mut self, r: usize, s: usize, v: f64) {
        self.0.add(r, s, v);
    }
    pub(crate) fn solve(self, b: Vec<f64>) -> SplineResult<Vec<f64>> {
        self.0.solve(b).map_err(failure)
    }
}
