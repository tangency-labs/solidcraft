//! Raw portable values for clamped space operators; all arithmetic stays in the kernel.
use solidcore::geom::spline_space as kernel;

pub use kernel::{Cond, MAX_SPACE_DIMENSION};
/// Portable refusal categories for a spline-space operation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SplineFailure {
    /// Arguments do not define the operation.
    InvalidInput,
    /// Knot spaces are invalid or incompatible.
    InvalidKnots,
    /// Control, condition, or matrix dimensions disagree.
    InvalidCount,
    /// A parameter is nonfinite or outside its domain.
    InvalidParameter,
    /// The interpolation equations do not determine a spline.
    Singular,
    /// The operation did not produce finite values.
    NonFinite,
    /// The requested degree relationship is unsupported.
    UnsupportedDegree,
    /// A declared dimension bound was exceeded.
    TooLarge,
    /// Bounded storage could not be allocated.
    Allocation,
}

pub(crate) fn classify(error: solidcore::SolidError) -> SplineFailure {
    use solidcore::error::nurbs::NurbsFailure;
    let reason = match error {
        solidcore::SolidError::Failed(failure) => match failure.reason {
            solidcore::error::Reason::Nurbs(reason) => Some(reason),
            _ => None,
        },
        _ => None,
    };
    match reason {
        Some(NurbsFailure::Degree | NurbsFailure::DegreeTooHigh { .. }) => {
            SplineFailure::UnsupportedDegree
        }
        Some(NurbsFailure::Storage) => SplineFailure::TooLarge,
        Some(NurbsFailure::Memory) => SplineFailure::Allocation,
        Some(NurbsFailure::Knots | NurbsFailure::KnotsDecrease { .. }) => {
            SplineFailure::InvalidKnots
        }
        Some(NurbsFailure::CurveDefinition | NurbsFailure::SurfaceDefinition) => {
            SplineFailure::InvalidCount
        }
        Some(NurbsFailure::Parameter | NurbsFailure::ParameterOutside { .. }) => {
            SplineFailure::InvalidParameter
        }
        Some(NurbsFailure::Singular) => SplineFailure::Singular,
        Some(NurbsFailure::Numerical) => SplineFailure::NonFinite,
        _ => SplineFailure::InvalidInput,
    }
}

/// An opaque clamped spline space.
#[derive(Clone, Debug, PartialEq)]
pub struct Space(kernel::Space);
impl Space {
    /// Validate a clamped bounded space.
    pub fn new(degree: usize, knots: Vec<f64>) -> Result<Self, SplineFailure> {
        kernel::Space::new(degree, knots)
            .map(Self)
            .map_err(classify)
    }
    /// One polynomial interval.
    pub fn polynomial(degree: usize, a: f64, b: f64) -> Result<Self, SplineFailure> {
        kernel::Space::polynomial(degree, a, b)
            .map(Self)
            .map_err(classify)
    }
    /// A simple interior knot at each supplied break.
    pub fn with_breaks(degree: usize, breaks: &[f64]) -> Result<Self, SplineFailure> {
        kernel::Space::with_breaks(degree, breaks)
            .map(Self)
            .map_err(classify)
    }
    /// Polynomial degree.
    pub fn degree(&self) -> usize {
        self.0.degree()
    }
    /// Retained knots.
    pub fn knots(&self) -> &[f64] {
        self.0.knots()
    }
    /// Control count.
    pub fn count(&self) -> usize {
        self.0.count()
    }
    /// Closed domain.
    pub fn domain(&self) -> [f64; 2] {
        self.0.domain()
    }
    /// Right-owned local derivative row, except at the domain end.
    pub fn basis(&self, t: f64, der: usize) -> Result<(usize, Vec<f64>), SplineFailure> {
        self.0.basis(t, der).map_err(classify)
    }
    /// Left-owned local derivative row.
    pub fn basis_left(&self, t: f64, der: usize) -> Result<(usize, Vec<f64>), SplineFailure> {
        self.0.basis_left(t, der).map_err(classify)
    }
    /// Evaluate four linear raw components.
    pub fn eval<T>(
        &self,
        values: &[T],
        t: f64,
        der: usize,
        value: impl Fn(&T) -> [f64; 4],
    ) -> Result<[f64; 4], SplineFailure> {
        self.0.eval_mapped(values, t, der, value).map_err(classify)
    }
    /// Add simple interior breaks.
    pub fn refined(&self, breaks: &[f64]) -> Result<Self, SplineFailure> {
        self.0.refined(breaks).map(Self).map_err(classify)
    }
}
/// A factored confluent interpolation operator.
#[derive(Clone, Debug)]
pub struct Interpolant(kernel::Interpolant);
impl Interpolant {
    /// Factor in the retained condition order.
    pub fn new(space: Space, conds: Vec<Cond>) -> Result<Self, SplineFailure> {
        kernel::Interpolant::new(space.0, conds)
            .map(Self)
            .map_err(classify)
    }
    /// Lowest supported polynomial or averaged-knot confluent space.
    pub fn lowest(conds: Vec<Cond>, a: f64, b: f64) -> Result<Self, SplineFailure> {
        kernel::Interpolant::lowest(conds, a, b)
            .map(Self)
            .map_err(classify)
    }
    /// Copy the small immutable space for a caller retaining its own façade.
    pub fn space(&self) -> Space {
        Space(self.0.space().clone())
    }
    /// Ordered condition identities.
    pub fn conds(&self) -> &[Cond] {
        self.0.conds()
    }
    /// Solve four raw components through the retained factor.
    pub fn fit(&self, values: &[[f64; 4]]) -> Result<Vec<[f64; 4]>, SplineFailure> {
        self.0.fit(values).map_err(classify)
    }
}
/// Exact degree elevation and separate-difference Boehm embedding.
pub fn embed(
    from: &Space,
    values: &[[f64; 4]],
    to: &Space,
) -> Result<Vec<[f64; 4]>, SplineFailure> {
    kernel::embed(&from.0, values, &to.0).map_err(classify)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn portable_refusal_categories_preserve_unsupported_limit_and_invalid_input() {
        assert_eq!(
            Space::polynomial(usize::MAX, 0.0, 1.0).unwrap_err(),
            SplineFailure::UnsupportedDegree
        );
        assert_eq!(
            Space::with_breaks(1, &vec![0.0; MAX_SPACE_DIMENSION + 1]).unwrap_err(),
            SplineFailure::TooLarge
        );
        assert_eq!(
            Space::new(1, vec![0.0, 0.0, f64::NAN, 1.0]).unwrap_err(),
            SplineFailure::InvalidKnots
        );
    }
}
