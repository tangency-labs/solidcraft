//! Sew conforming patches to a rim whose original edges have fewer vertices.
use std::collections::HashSet;

use solidcraft_geom::Vec3;

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
        let mut seen = HashSet::new();
        let mut points = Vec::<Vec3>::new();
        let mut edges = 0usize;
        for body in bodies {
            edges = edges.saturating_add(body.edge_count());
            for vertex in body.solid.vertex_iter() {
                let p = crate::body::from_p3(vertex.point());
                if !p.is_finite() {
                    return Err(KernelError::Invalid("a sewing vertex is not finite".into()));
                }
                let bits = |v: f64| if v == 0.0 { 0 } else { v.to_bits() };
                if seen.insert((bits(p.x), bits(p.y), bits(p.z))) {
                    points.push(p);
                }
                if points.len() > 16_384 {
                    return Err(KernelError::TooLarge("sewing vertex budget exceeded".into()));
                }
            }
        }
        if edges > 100_000 || edges.saturating_mul(points.len()) > 5_000_000 {
            return Err(KernelError::TooLarge("split sewing work budget exceeded".into()));
        }
        let counts: Vec<_> = bodies.iter().map(|b| (b.face_count(), b.edge_count())).collect();
        let mut split = Vec::with_capacity(bodies.len());
        for (i, body) in bodies.iter().enumerate() {
            let source = crate::boolean_record::operand(body, i, &counts);
            let changed = crate::splitface::split_edges_at(&source, &points)?;
            split.push(if changed.same_shape(&source) { source } else { crate::record::after(changed, &source, &counts) });
        }
        let refs: Vec<_> = split.iter().collect();
        let result = crate::stitch(&refs, tolerance)?;
        let histories: Option<Vec<_>> = split.iter().map(Body::history).collect();
        let history = result.history().zip(histories).and_then(|(h, inner)| crate::boolean_record::composed(h.clone().over(&inner)));
        Ok(result.with_checked_history(history, &counts))
    })
}
