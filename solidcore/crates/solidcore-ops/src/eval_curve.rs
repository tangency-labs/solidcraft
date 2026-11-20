//! Exact evaluation of a portable curve in its record parameters.

use solidcore::eval::CurveEval;
use solidcore_record::{brep::Curve, Vec3};

use crate::{Error, Result};

/// A curve evaluator that owns its geometry.
#[derive(Clone, Debug)]
pub struct ExactCurve(CurveEval);

impl ExactCurve {
    /// Read a portable curve record.
    pub fn new(curve: &Curve) -> Result<Self> {
        let curve = solidcore_convert::curve(curve).map_err(|_| Error::Argument)?;
        CurveEval::new(&curve)
            .map(Self)
            .map_err(|_| Error::Argument)
    }

    /// Whether the curve has linear geometry.
    pub fn is_linear(&self) -> bool {
        matches!(self.0, CurveEval::Line { .. })
            || matches!(&self.0, CurveEval::Spline(c) if c.degree() == 1 && c.ctrl().len() == 2)
    }

    /// Position and derivative at a fraction of an oriented record interval.
    pub fn at(&self, range: [f64; 2], fraction: f64) -> Option<(Vec3, Vec3)> {
        self.0
            .interval_derivs(range, fraction)
            .map(solidcore_convert::record_pair)
    }
}
