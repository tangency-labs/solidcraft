//! Sew conforming patches to a rim whose original edges have fewer vertices.
use crate::{Body, KernelError, Result};

/// Split existing edges at the operands' vertices before sewing. Faces retain
/// their surfaces and regions. The result's history is over `bodies`, including
/// origins of split rim edges. This is bounded independently of tessellation.
pub fn stitch_split(bodies: &[&Body], tolerance: f64) -> Result<Body> {
    if bodies.is_empty() || bodies.len() > 64 || !tolerance.is_finite() || !(1e-9..=1.0).contains(&tolerance) {
        return Err(KernelError::Invalid("sew 1..64 bodies with a finite bounded tolerance".into()));
    }
    for body in bodies {
        body.require_brep("split and stitch")?;
    }
    crate::guard("split and stitch", || {
        let mut edges = 0usize;
        let mut vertices = 0usize;
        for body in bodies {
            edges = edges.saturating_add(body.edge_count());
            vertices = vertices.saturating_add(body.record().vertices.len());
            if vertices > 16_384 {
                return Err(KernelError::TooLarge("sewing vertex budget exceeded".into()));
            }
            for vertex in &body.record().vertices {
                if vertex.point.iter().any(|coordinate| !coordinate.is_finite()) {
                    return Err(KernelError::Invalid("a sewing vertex is not finite".into()));
                }
            }
        }
        if edges > 100_000 || edges.saturating_mul(vertices) > 5_000_000 {
            return Err(KernelError::TooLarge("split sewing work budget exceeded".into()));
        }
        // The current stitch path splits exact record edges at captured
        // cross-operand vertices and carries their native history.
        crate::stitch(bodies, tolerance)
    })
}
