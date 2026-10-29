//! Read-only adapters to the kernel's existing exact evaluators and trim curves.
use solidcraft_geom::{
    Vec3,
    tess::{EvalCurve, EvalSurface},
};
use truck_modeling::{self as mt, BoundedCurve};

use crate::{Body, KernelError, Result, guard};

/// One oriented edge, with its original curve parameter interval.
#[derive(Clone, Debug)]
pub struct CurveGeometry {
    pub curve: EvalCurve,
    pub range: [f64; 2],
}
impl CurveGeometry {
    /// Position and derivative with respect to the normalized interval 0..1.
    pub fn at(&self, fraction: f64) -> Option<(Vec3, Vec3)> {
        if !fraction.is_finite() || !(0.0..=1.0).contains(&fraction) {
            return None;
        }
        let [a, b] = self.range;
        if !a.is_finite() || !b.is_finite() || b == a {
            return None;
        }
        let (point, tangent) = self.curve.eval(a + (b - a) * fraction)?;
        let tangent = tangent * (b - a);
        (point.is_finite() && tangent.is_finite()).then_some((point, tangent))
    }
}
/// An exact surface and its oriented trim loops in the surface's own sense.
#[derive(Clone, Debug)]
pub struct FaceGeometry {
    pub surface: EvalSurface,
    pub loops: Vec<Vec<CurveGeometry>>,
    pub outer: usize,
    /// Whether the face's outward sense agrees with its evaluator's normal.
    pub outward: bool,
}
/// One boundary edge in display order, and the unique face across it, if any.
#[derive(Clone, Debug)]
pub struct BoundaryEdge {
    pub edge: usize,
    pub neighbor: Option<usize>,
}
/// Topological adjacency in the same loop/edge order as [`face_geometry`].
pub fn face_boundaries(body: &Body, face: usize) -> Result<Vec<Vec<BoundaryEdge>>> {
    body.require_brep("read boundary adjacency")?;
    guard("read boundary adjacency", || {
        let selected = body.solid.face_iter().nth(face).ok_or_else(|| KernelError::Invalid("face index is outside the body".into()))?;
        let wires = selected.absolute_boundaries();
        if wires.len() > 64 || wires.iter().map(|w| w.len()).sum::<usize>() > 1024 || body.face_count() > 100_000 {
            return Err(KernelError::TooLarge("boundary adjacency budget exceeded".into()));
        }
        let mut wanted = std::collections::HashMap::<mt::EdgeID, Vec<usize>>::new();
        for edge in selected.edge_iter() {
            wanted.entry(edge.id()).or_default();
        }
        for (index, candidate) in body.solid.face_iter().enumerate() {
            if index == face {
                continue;
            }
            for edge in candidate.edge_iter() {
                if let Some(neighbors) = wanted.get_mut(&edge.id())
                    && neighbors.len() < 2
                    && !neighbors.contains(&index)
                {
                    neighbors.push(index);
                }
            }
        }
        let edges = Body::unique_edges(&body.solid);
        if edges.len() > 100_000 {
            return Err(KernelError::TooLarge("boundary edge-index budget exceeded".into()));
        }
        let indices: std::collections::HashMap<_, _> =
            edges.iter().enumerate().filter(|(_, e)| wanted.contains_key(&e.id())).map(|(i, e)| (e.id(), i)).collect();
        wires
            .iter()
            .map(|wire| {
                wire.edge_iter()
                    .map(|edge| {
                        let index = *indices.get(&edge.id()).ok_or_else(|| KernelError::Invalid("boundary edge lost its display index".into()))?;
                        let neighbor = match wanted.get(&edge.id()).map(Vec::as_slice) {
                            Some([only]) => Some(*only),
                            _ => None,
                        };
                        Ok(BoundaryEdge { edge: index, neighbor })
                    })
                    .collect()
            })
            .collect()
    })
}
impl FaceGeometry {
    /// Resolve a point on the surface through the same bounded projection as tessellation.
    pub fn parameters(&self, point: Vec3, near: Option<[f64; 2]>, tolerance: f64) -> Result<[f64; 2]> {
        if !point.is_finite()
            || near.is_some_and(|p| p.iter().any(|v| !v.is_finite()))
            || !tolerance.is_finite()
            || !(1e-9..=1.0).contains(&tolerance)
        {
            return Err(KernelError::Invalid("surface parameters require finite points and a tolerance in 1e-9..1 mm".into()));
        }
        crate::face_params::parameters(&self.surface, &crate::tess::uv::domain(&self.surface), point, near, tolerance)
    }
}
fn curve_geometry(curve: &mt::Curve) -> Result<CurveGeometry> {
    let (a, b) = curve.range_tuple();
    if !a.is_finite() || !b.is_finite() || b <= a {
        return Err(KernelError::Invalid("the edge has no finite parameter interval".into()));
    }
    let curve = crate::tess::surface::eval_curve(curve).ok_or_else(|| KernelError::Invalid("this edge has no exact rational evaluator".into()))?;
    Ok(CurveGeometry { curve, range: [a, b] })
}
/// Read a face in the same stable order as the body's display face IDs.
/// This does not fit, rebuild, or alter the supporting geometry.
pub fn face_geometry(body: &Body, face: usize) -> Result<FaceGeometry> {
    body.require_brep("read face geometry")?;
    guard("read face geometry", || {
        let face = body.solid.face_iter().nth(face).ok_or_else(|| KernelError::Invalid("face index is outside the body".into()))?;
        let wires = face.absolute_boundaries();
        if wires.is_empty() || wires.len() > 64 || wires.iter().map(|w| w.len()).sum::<usize>() > 1024 {
            return Err(KernelError::TooLarge("face trim geometry budget exceeded".into()));
        }
        // Kernel fills retain the original rim and qualify its distance to the
        // fitted surface within face_tolerance. Reading that valid face must use
        // the same boundary contract, including an imported declaration.
        let accept = crate::fill::face_tolerance(body).max(body.boundary_tol.unwrap_or(0.0));
        let data = crate::face_params::FaceLoops::of(face, accept, "read")?;
        let loops = data.wires.iter().map(|w| w.edge_iter().map(|e| curve_geometry(&e.oriented_curve())).collect()).collect::<Result<_>>()?;
        Ok(FaceGeometry { surface: data.eval, loops, outer: data.outer, outward: face.orientation() })
    })
}
/// The bounded parameter span reached by the face's exterior trim, with periodic
/// directions unwrapped in trim order. The surface's larger untrimmed domain is not used.
pub fn face_parameter_span(body: &Body, face: usize) -> Result<[[f64; 2]; 2]> {
    body.require_brep("read face parameter span")?;
    if face >= 100_000 {
        return Err(KernelError::TooLarge("face parameter-span index budget exceeded".into()));
    }
    guard("read face parameter span", || {
        let face = body.solid.face_iter().nth(face).ok_or_else(|| KernelError::Invalid("face index is outside the body".into()))?;
        let wires = face.absolute_boundaries();
        if wires.is_empty() || wires.len() > 64 || wires.iter().map(|w| w.len()).sum::<usize>() > 1024 {
            return Err(KernelError::TooLarge("face parameter-span trim budget exceeded".into()));
        }
        let tolerance = crate::fill::face_tolerance(body).max(body.boundary_tol.unwrap_or(0.0));
        let data = crate::face_params::FaceLoops::of(face, tolerance, "read span")?;
        let outer = data.loops.get(data.outer).ok_or_else(|| KernelError::Invalid("face has no exterior trim".into()))?;
        let span =
            std::array::from_fn(|d| [false, true].map(|maximum| crate::face_params::extreme(&data.eval, &data.dom, outer, d, maximum, tolerance)));
        if span.iter().any(|r: &[f64; 2]| r.iter().any(|v| !v.is_finite()) || r[1] - r[0] <= 1e-12 || r[1] - r[0] > 1e8) {
            return Err(KernelError::Invalid("the face has no finite nondegenerate parameter span".into()));
        }
        Ok(span)
    })
}
/// Read selected edges in the same stable order as `Body::edges` and display edge IDs.
pub fn edge_geometry(body: &Body, edges: &[usize]) -> Result<Vec<CurveGeometry>> {
    body.require_brep("read edge geometry")?;
    if edges.is_empty() || edges.len() > 4096 {
        return Err(KernelError::TooLarge("read 1..4096 edges".into()));
    }
    guard("read edge geometry", || {
        let all = Body::unique_edges(&body.solid);
        edges
            .iter()
            .map(|&i| {
                let edge = all.get(i).ok_or_else(|| KernelError::Invalid("edge index is outside the body".into()))?;
                curve_geometry(&edge.oriented_curve())
            })
            .collect()
    })
}
