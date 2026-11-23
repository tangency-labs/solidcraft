//! Exact edge evaluators bound to one immutable body's shared screw owners.
//! Owner graphs are validated once before allocation; ordinary edges retain their
//! standalone evaluator and refusal behavior. No geometry is computed in this layer.

use solidcore::eval::{CurveEval, EvalError};
use solidcore::surface::procedural_sweep::ProceduralSweep;
use solidcore::surface::surface::Surface;
use solidcore_record::brep::{Brep, Curve};
use std::sync::{Arc, OnceLock};

type CachedCurve = OnceLock<Result<Arc<CurveEval>, EvalError>>;

/// A body-scoped owner pool and exact-edge cache. Borrowing the immutable body
/// prevents reuse of an edge index after an edit or with another record.
pub struct BodyCurveContext<'a> {
    body: &'a Brep,
    owners: Vec<Arc<Surface>>,
    exact: Vec<CachedCurve>,
}

impl<'a> BodyCurveContext<'a> {
    /// Validate an exact-owner graph's cumulative resource budget and certify
    /// each owner once. Ordinary bodies keep their existing per-edge refusals.
    pub fn new(body: &'a Brep) -> Result<Self, EvalError> {
        let has_exact = crate::brep::needs_evaluator_budget(body);
        if has_exact {
            body.validate()
                .map_err(|e| EvalError::Invalid(e.to_string()))?;
        }
        let mut owners = Vec::new();
        for owner in &body.screw_owners {
            let surface = ProceduralSweep::from_record(&crate::sweep::sweep(owner))
                .map_err(|e| EvalError::Invalid(e.to_string()))?;
            owners.push(Arc::new(surface));
        }
        let exact = if has_exact {
            body.edges.iter().map(|_| OnceLock::new()).collect()
        } else {
            Vec::new()
        };
        Ok(Self {
            body,
            owners,
            exact,
        })
    }

    /// The evaluator for this body's edge. An exact edge's UV evaluator and
    /// owner are reused for initial sampling, splitting, and relaxed sampling.
    pub fn edge(&self, index: usize) -> Result<Arc<CurveEval>, EvalError> {
        let edge = self
            .body
            .edges
            .get(index)
            .ok_or_else(|| EvalError::Invalid("no such body edge".into()))?;
        let curve = edge.parameters();
        let make = || {
            if let Curve::OnSurfaceRef {
                owner,
                uv_curve,
                domain,
            } = curve
            {
                let owner = usize::try_from(*owner)
                    .ok()
                    .and_then(|index| self.owners.get(index))
                    .ok_or_else(|| EvalError::Invalid("screw owner index".into()))?;
                CurveEval::on_screw_surface(Arc::clone(owner), &crate::curve2(uv_curve), *domain)
                    .map(Arc::new)
            } else {
                let value = crate::curve(curve).map_err(|e| EvalError::Invalid(e.to_string()))?;
                CurveEval::new(&value).map(Arc::new)
            }
        };
        if matches!(curve, Curve::OnSurface { .. } | Curve::OnSurfaceRef { .. }) {
            self.exact
                .get(index)
                .ok_or_else(|| EvalError::Invalid("missing exact-edge cache".into()))?
                .get_or_init(make)
                .clone()
        } else {
            make()
        }
    }
}
