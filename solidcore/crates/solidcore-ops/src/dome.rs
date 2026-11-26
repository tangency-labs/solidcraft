//! A dome on a planar face of a body (#19532): the face raised along its outward normal into a
//! cap of height `height` whose outline is the face's own edges. The cap is built by the kernel
//! ([`solidcore::topology::brep::dome`]); here the face goes and the cap is sewn onto the edges
//! it leaves open, so the body stays closed.

pub use solidcore::topology::brep::dome::{DomeRefusal, DomeShape};

use solidcore::topology::Brep;

use crate::body::Body;
use crate::error::status;
use crate::order::{self, Order};
use crate::session::with_body;
use crate::stitch::sew;
use crate::trim::remove_faces;
use crate::{Error, Result};
use solidcore::topology::brep::dome::dome_cap;

/// The dome of height `height` on record face `face` of `body`: `shape` is the cap's shape, and
/// `reverse` would cut it into the body (not supported yet, so it is refused).
///
/// # Errors
/// [`Error::Dome`] for a face or height the dome cannot take (see [`DomeRefusal`]);
/// [`Error::Argument`] for a face index not the body's; [`Error::NotSolid`] when the sewn
/// result does not close; the kernel's status.
pub fn dome(
    body: &Body,
    order: &Order,
    face: usize,
    height: f64,
    shape: DomeShape,
    reverse: bool,
) -> Result<Body> {
    let f = order::face(order, face)?;
    let cap: Brep = with_body("dome", body, |k, b| {
        Ok(match dome_cap(k, b, f, height, shape, reverse) {
            Ok(Ok(cap)) => Ok(cap.sheet),
            Ok(Err(refusal)) => Err(Error::Dome(refusal)),
            Err(e) => Err(status(e)),
        })
    })??;
    let tol = cap.tolerance();
    let open = remove_faces(body, order, &[face])?;
    let sewn = sew(&[&open, &Body::from_kernel(cap)], tol)?;
    if !sewn.closed {
        return Err(Error::NotSolid);
    }
    Ok(sewn.body)
}
