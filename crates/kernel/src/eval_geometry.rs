//! Exact read adapters over canonical B-rep records.
use crate::{Body, KernelError, Result};

mod trim_orientation;
use solidcore_record::Vec3;
use solidcore_record::brep::{self as b, Curve, Loop};
use solidcraft_geom::{
    brep::eval::{CurveEval, SurfaceEval},
    prelude::*,
    tess::{EvalCurve, EvalSurface, uv},
};

/// An exact edge and its oriented original parameter interval.
#[derive(Clone, Debug)]
pub struct CurveGeometry {
    /// A rational shape representation for control hulls. Its parameter may differ
    /// from an analytic record's angle. Use at() for positions and derivatives.
    pub curve: EvalCurve,
    pub range: [f64; 2],
    exact: CurveEval,
    intersection: Option<Box<IntersectionDerivative>>,
    breaks: Vec<f64>,
}
#[derive(Clone, Debug)]
struct IntersectionDerivative {
    leader: CurveEval,
    surfaces: [SurfaceEval; 2],
}
impl CurveGeometry {
    pub fn control_points(&self) -> Option<&[[f64; 4]]> {
        self.intersection.is_none().then_some(self.curve.ctrl.as_slice())
    }
    pub fn is_linear(&self) -> bool {
        matches!(self.exact, CurveEval::Line { .. }) || matches!(&self.exact, CurveEval::Spline(c) if c.degree == 1 && c.ctrl.len() == 2)
    }
    pub fn parameter_breaks(&self) -> &[f64] {
        &self.breaks
    }
    /// Position and derivative with respect to normalized interval 0..1.
    pub fn at(&self, fraction: f64) -> Option<(Vec3, Vec3)> {
        if !fraction.is_finite() || !(0.0..=1.0).contains(&fraction) {
            return None;
        }
        let [a, b] = self.range;
        let span = b - a;
        if !a.is_finite() || !b.is_finite() || span == 0.0 {
            return None;
        }
        let t = a * (1.0 - fraction) + b * fraction;
        let (point, tangent) = if let Some(source) = &self.intersection {
            let point = self.exact.point(t)?;
            let (lead, first) = source.leader.derivs(t)?;
            let second = match &source.leader {
                CurveEval::Spline(c) => *c.derivatives(t, 2)?.get(2)?,
                CurveEval::Polyline(_) | CurveEval::Line { .. } => Vec3::ZERO,
                _ => return None,
            };
            let mut normals = [Vec3::ZERO; 2];
            for (normal, surface) in normals.iter_mut().zip(&source.surfaces) {
                let uv = surface.project(point, None, f64::INFINITY)?;
                let (_, u, v) = surface.derivs(uv[0], uv[1])?;
                *normal = u.cross(v).normalized()?;
            }
            let along = normals[0].cross(normals[1]);
            let denominator = first.dot(along);
            if denominator.abs() <= 1e-12 * first.len() * along.len() {
                return None;
            }
            // Differentiate L'(t)·(X(t)-L(t))=0 together with both surface constraints.
            (point, along * ((first.len2() - second.dot(point - lead)) / denominator))
        } else {
            self.exact.derivs(t)?
        };
        let tangent = tangent * span;
        (point.is_finite() && tangent.is_finite()).then_some((point, tangent))
    }
}
/// A face's exact evaluator, trim loops in natural surface order, and shell sense.
#[derive(Clone, Debug)]
pub struct FaceGeometry {
    pub surface: EvalSurface,
    pub loops: Vec<Vec<CurveGeometry>>,
    pub outer: usize,
    pub outward: bool,
}
impl FaceGeometry {
    pub fn parameters(&self, point: Vec3, near: Option<[f64; 2]>, tolerance: f64) -> Result<[f64; 2]> {
        if !point.is_finite()
            || near.is_some_and(|q| q.iter().any(|x| !x.is_finite()))
            || !tolerance.is_finite()
            || !(1e-9..=1.0).contains(&tolerance)
        {
            return Err(invalid("surface parameters require finite points and a tolerance in 1e-9..1 mm"));
        }
        let domain = uv::domain(&self.surface);
        let mut parameters = uv::project(&self.surface, &domain, point, near, tolerance)
            .ok_or_else(|| invalid("point cannot be resolved on the exact face surface"))?;
        if let Some(near) = near {
            for d in 0..2 {
                parameters[d] = uv::unwrap(parameters[d], near[d], domain.period[d]);
            }
        }
        let (on, _, _) =
            self.surface.derivs(parameters[0], parameters[1]).ok_or_else(|| invalid("resolved surface parameters cannot be evaluated"))?;
        if on.dist(point) > tolerance {
            return Err(invalid("point is outside the face surface tolerance"));
        }
        Ok(parameters)
    }
}
#[derive(Clone, Debug)]
pub struct BoundaryEdge {
    pub edge: usize,
    pub neighbor: Option<usize>,
}
fn invalid(why: impl std::fmt::Display) -> KernelError {
    KernelError::Invalid(why.to_string())
}
fn selected(body: &Body, face: usize) -> Result<&b::Face> {
    body.require_brep("read exact face geometry")?;
    let face = body.record().faces.get(face).ok_or_else(|| invalid("face index is outside the body"))?;
    if face.loops.is_empty()
        || face.loops.len() > 64
        || face
            .loops
            .iter()
            .map(|l| match l {
                Loop::Edges { edges } => edges.len(),
                _ => 0,
            })
            .sum::<usize>()
            > 1024
    {
        return Err(KernelError::TooLarge("face trim geometry budget exceeded".into()));
    }
    Ok(face)
}
pub fn face_boundaries(body: &Body, face: usize) -> Result<Vec<Vec<BoundaryEdge>>> {
    let source = selected(body, face)?;
    if body.face_count() > 100_000 || body.edge_count() > 100_000 {
        return Err(KernelError::TooLarge("boundary adjacency budget exceeded".into()));
    }
    source
        .loops
        .iter()
        .map(|wire| {
            let Loop::Edges { edges } = wire else { return Ok(Vec::new()) };
            edges
                .iter()
                .map(|use_| {
                    let edge = body.record().edges.get(use_.edge as usize).ok_or_else(|| invalid("missing boundary edge record"))?;
                    let neighbors: std::collections::BTreeSet<_> = edge.uses.iter().map(|u| u.face as usize).filter(|&i| i != face).collect();
                    let neighbor = if neighbors.len() == 1 { neighbors.first().copied() } else { None };
                    Ok(BoundaryEdge { edge: use_.edge as usize, neighbor })
                })
                .collect()
        })
        .collect()
}
fn shape(source: &Curve, interval: [f64; 2]) -> Result<EvalCurve> {
    match CurveEval::new(source).map_err(invalid)? {
        CurveEval::Spline(curve) => Ok(curve),
        CurveEval::Polyline(points) => {
            if points.len() < 2 {
                return Err(invalid("polyline needs two points"));
            }
            let last = points.len() - 1;
            let mut knots = vec![0.0, 0.0];
            knots.extend((1..last).map(|i| i as f64));
            knots.extend([last as f64; 2]);
            Ok(EvalCurve { degree: 1, knots, ctrl: points.iter().map(|p| [p.x, p.y, p.z, 1.0]).collect() })
        }
        _ => {
            if let Curve::SurfaceIntersection { leader, .. } = source {
                return shape(leader, interval);
            }
            let (form, _) =
                solidcraft_geom::brep::form::curve(source, interval).ok_or_else(|| invalid("edge has no exact rational shape representation"))?;
            let CurveEval::Spline(curve) = CurveEval::new(&form).map_err(invalid)? else {
                return Err(invalid("invalid rational edge form"));
            };
            Ok(curve)
        }
    }
}
fn original_parameter_breaks(source: &Curve, [a, b]: [f64; 2]) -> Vec<f64> {
    match source {
        Curve::Spline { knots, .. } | Curve::RationalSpline { knots, .. } => knots.clone(),
        Curve::Polyline { points } => (0..points.len()).map(|i| i as f64).collect(),
        Curve::Circle { .. } | Curve::Ellipse { .. } => {
            let n = ((b - a) / std::f64::consts::FRAC_PI_2).ceil().clamp(1.0, 4096.0) as usize;
            (0..=n).map(|i| a + (b - a) * i as f64 / n as f64).collect()
        }
        Curve::SurfaceIntersection { leader, .. } => original_parameter_breaks(leader, [a, b]),
        _ => vec![a, b],
    }
}
fn curve_geometry(source: &Curve, interval: [f64; 2], forward: bool) -> Result<CurveGeometry> {
    let [a, b] = interval;
    if !a.is_finite() || !b.is_finite() || b <= a {
        return Err(invalid("edge has no finite parameter interval"));
    }
    if let Curve::SurfaceIntersection { leader, .. } = source
        && !matches!(leader.as_ref(), Curve::Spline { .. } | Curve::RationalSpline { .. } | Curve::Polyline { .. })
    {
        return Err(invalid("intersection derivatives require a spline or polyline leader in its original parameters"));
    }
    let exact = CurveEval::new(source).map_err(invalid)?;
    let curve = shape(source, interval)?;
    let breaks = original_parameter_breaks(source, interval);
    let intersection = if let Curve::SurfaceIntersection { leader, surfaces, .. } = source {
        Some(Box::new(IntersectionDerivative {
            leader: CurveEval::new(leader).map_err(invalid)?,
            surfaces: [SurfaceEval::new(&surfaces[0]).map_err(invalid)?, SurfaceEval::new(&surfaces[1]).map_err(invalid)?],
        }))
    } else {
        None
    };
    Ok(CurveGeometry { curve, range: if forward { interval } else { [b, a] }, exact, intersection, breaks })
}
pub fn face_geometry(body: &Body, face: usize) -> Result<FaceGeometry> {
    let source = selected(body, face)?;
    let (surface, _) = crate::tess::surface::eval_surface(source.parameters(), source.domain)?;
    let loops = source
        .loops
        .iter()
        .map(|wire| {
            let Loop::Edges { edges } = wire else { return Ok(Vec::new()) };
            edges
                .iter()
                .map(|use_| {
                    let edge = body.record().edges.get(use_.edge as usize).ok_or_else(|| invalid("missing face edge record"))?;
                    curve_geometry(edge.parameters(), edge.interval, use_.forward)
                })
                .collect()
        })
        .collect::<Result<Vec<_>>>()?;
    let outer = trim_orientation::positive_loop(source, body)?;
    let outward = crate::surfaces::face_sides(body.record()).get(face).copied().ok_or_else(|| invalid("missing face shell orientation"))?;
    Ok(FaceGeometry { surface, loops, outer, outward })
}
pub fn face_parameter_span(body: &Body, face: usize) -> Result<[[f64; 2]; 2]> {
    let source = selected(body, face)?;
    let outer = trim_orientation::positive_loop(source, body)?;
    let Loop::Edges { edges } = source.loops.get(outer).ok_or_else(|| invalid("missing outer boundary"))? else {
        return Err(invalid("outer boundary is a point"));
    };
    let curves = edges
        .iter()
        .map(|use_| {
            let edge = body.record().edges.get(use_.edge as usize).ok_or_else(|| invalid("missing exterior edge"))?;
            Ok((edge.parameters(), edge.interval, use_.forward))
        })
        .collect::<Result<Vec<_>>>()?;
    let span = solidcraft_geom::brep::form::extent(source.parameters(), &[curves]).ok_or_else(|| invalid("face has no bounded parameter span"))?;
    let (_, map) = crate::tess::surface::eval_surface(source.parameters(), source.domain)?;
    let corners = [
        map.to_eval([span[0][0], span[1][0]]),
        map.to_eval([span[0][0], span[1][1]]),
        map.to_eval([span[0][1], span[1][0]]),
        map.to_eval([span[0][1], span[1][1]]),
    ];
    let out = std::array::from_fn(|d| corners.iter().fold([f64::INFINITY, f64::NEG_INFINITY], |[a, b], q| [a.min(q[d]), b.max(q[d])]));
    if out.iter().any(|r: &[f64; 2]| !r[0].is_finite() || !r[1].is_finite() || r[1] - r[0] <= 1e-12 || r[1] - r[0] > 1e8) {
        return Err(invalid("face has no finite nondegenerate parameter span"));
    }
    Ok(out)
}
pub fn edge_geometry(body: &Body, edges: &[usize]) -> Result<Vec<CurveGeometry>> {
    body.require_brep("read edge geometry")?;
    if edges.is_empty() || edges.len() > 4096 {
        return Err(KernelError::TooLarge("read 1..4096 edges".into()));
    }
    edges
        .iter()
        .map(|&i| {
            let edge = body.record().edges.get(i).ok_or_else(|| invalid("edge index is outside the body"))?;
            curve_geometry(edge.parameters(), edge.interval, true)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intersection_adapter_retains_leader_parameters_and_rejects_analytic_leaders() {
        let surfaces = [
            Box::new(b::Surface::Plane { origin: [0.0; 3], u_axis: [1.0, 0.0, 0.0], v_axis: [0.0, 1.0, 0.0] }),
            Box::new(b::Surface::Plane { origin: [0.0; 3], u_axis: [1.0, 0.0, 0.0], v_axis: [0.0, 0.0, 1.0] }),
        ];
        let knots = vec![10.0, 10.0, 10.0, 20.0, 20.0, 20.0];
        let leader = Curve::Spline { knots: knots.clone(), points: vec![[0.0, 0.01, 0.02], [1.0, 0.01, 0.02], [3.0, 0.01, 0.02]] };
        let exact = CurveEval::new(&leader).unwrap();
        let record = Curve::SurfaceIntersection { surfaces: surfaces.clone(), leader: Box::new(leader), tolerance: 0.1 };
        let curve = curve_geometry(&record, [10.0, 20.0], true).unwrap();
        assert_eq!(curve.parameter_breaks(), knots);
        assert!(curve.control_points().is_none());
        for fraction in [0.0, 0.25, 0.5, 0.75, 1.0] {
            let (lead, derivative) = exact.derivs(10.0 + 10.0 * fraction).unwrap();
            let actual = curve.at(fraction).unwrap();
            assert!(actual.0.dist(Vec3::new(lead.x, 0.0, 0.0)) < 1e-10);
            assert!(actual.1.dist(Vec3::new(10.0 * derivative.x, 0.0, 0.0)) < 1e-10);
        }
        let analytic = Curve::Circle { frame: b::Frame { origin: [0.0; 3], x_axis: [1.0, 0.0, 0.0], y_axis: [0.0, 1.0, 0.0] }, radius: 1.0 };
        let record = Curve::SurfaceIntersection { surfaces, leader: Box::new(analytic), tolerance: 0.1 };
        assert!(curve_geometry(&record, [0.0, std::f64::consts::TAU], true).is_err());
        assert_eq!(
            original_parameter_breaks(&record, [0.0, std::f64::consts::TAU]),
            vec![0.0, std::f64::consts::FRAC_PI_2, std::f64::consts::PI, 3.0 * std::f64::consts::FRAC_PI_2, std::f64::consts::TAU]
        );
    }
}
