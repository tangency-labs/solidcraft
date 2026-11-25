//! A dome on a planar face of a body (#19532): the face raised along its outward normal into a
//! cap. The cap is built and sewn onto the body by solidcore-ops ([`solidcore_ops::dome`]); here
//! its refusals become the kernel's own error.

use solidcore_ops::dome::DomeShape;

use crate::body::Body;
use crate::{KernelError, Result};

/// The dome of `height` on face `face` of `b` (a face index of the body's record): `shape` is
/// the cap's shape, and `reverse` would cut it into the body (refused until it is built).
///
/// # Errors
/// [`KernelError::Dome`] for a face or height the dome cannot take; [`KernelError::Failed`] when
/// the kernel cannot take the body or the result does not close.
pub fn dome(b: &Body, face: usize, height: f64, shape: DomeShape, reverse: bool) -> Result<Body> {
    b.require_brep("dome")?;
    let form = b.ops_body().ok_or_else(|| KernelError::Failed("dome: the kernel could not take the body".into()))?;
    let (ops, order) = &*form;
    let raised = solidcore_ops::dome::dome(ops, order, face, height, shape, reverse).map_err(|e| match e {
        solidcore_ops::Error::Dome(refusal) => KernelError::Dome(refusal),
        e => KernelError::Failed(format!("dome: {e}")),
    })?;
    let (record, _) = raised.to_record().map_err(|e| KernelError::Failed(format!("dome: {e}")))?;
    Body::from_record(record)
}
