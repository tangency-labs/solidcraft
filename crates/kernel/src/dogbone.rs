//! Dogbone reliefs: the bite a router's round bit cuts into an inside corner so that a square
//! part seats in it, through `solidcore_ops::dogbone` (the recipe) and the kernel's
//! `solidcore::dogbone` (every point and length). This crate computes nothing of its own here:
//! it guards the calls and words the refusals.
//!
//! - [`plan`]: the relief cylinders of a body's picked inside edges, or of the inside corners of
//!   the planar faces at picked points (a pocket's floor), and the edges skipped with the reason.
//!   The caller cuts the cylinders from the body.

pub use solidcore_ops::dogbone::{BodyOptions, Cylinder, Options, Plan, Side, Skip, Skipped, Style, Tool, Why};

use solidcore_record::Vec3;

use crate::body::Body;
use crate::{KernelError, Result, guard};

/// The reliefs of `body` at its record edges `edges` and at the corners of the faces under the
/// points `faces` (see `solidcore_ops::dogbone::plan`). Each point picks the face nearest it on
/// the exact faces as trimmed, no farther than `reach` (`solidcore_ops::dogbone::faces_at`).
///
/// # Errors
/// [`KernelError::Invalid`] for a mesh body, an index past the body's edges, a point with no face
/// within `reach`, or options out of range; [`KernelError::Failed`] for a body the kernel cannot
/// take; [`KernelError::Internal`] for a panic in the kernel.
pub fn plan(body: &Body, edges: &[usize], faces: &[Vec3], reach: f64, options: &BodyOptions) -> Result<Plan> {
    body.require_brep("a dogbone")?;
    guard("dogbone", || {
        let face_idx = if faces.is_empty() {
            Vec::new()
        } else {
            let (ops, order) = body.ops_body().ok_or_else(|| KernelError::Failed("dogbone: the kernel cannot take this body".into()))?;
            let points: Vec<[f64; 3]> = faces.iter().map(|p| [p.x, p.y, p.z]).collect();
            let found = solidcore_ops::dogbone::faces_at(ops, order, &points, reach)
                .map_err(|e| KernelError::Invalid(format!("dogbone: picking faces: {e}")))?;
            let mut idx = Vec::with_capacity(found.len());
            for (p, f) in faces.iter().zip(found) {
                idx.push(f.ok_or_else(|| KernelError::Invalid(format!("no face at [{}, {}, {}]", p.x, p.y, p.z)))?);
            }
            idx
        };
        solidcore_ops::dogbone::plan(body.record(), edges, &face_idx, options).map_err(|e| KernelError::Invalid(format!("dogbone: {e}")))
    })
}
