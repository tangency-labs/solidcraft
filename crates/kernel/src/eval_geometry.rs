//! Exact read adapters over canonical B-rep records.
use crate::{Body, KernelError, Result};

mod trim_orientation;
use solidcore_ops::eval_curve::ExactCurve;
use solidcore_record::Vec3;
use solidcore_record::brep::{self as b, Curve, Loop};
use solidcraft_geom::{
    brep::eval::CurveEval,
    prelude::*,
    tess::{EvalCurve, EvalSurface},
};

/// An exact edge and its oriented original parameter interval.
#[derive(Clone, Debug)]
pub struct CurveGeometry {
    /// A rational shape representation for control hulls. Its parameter may differ
    /// from an analytic record's angle. Use at() for positions and derivatives.
    pub curve: EvalCurve,
    pub range: [f64; 2],
    shape_range: [f64; 2],
    exact: ExactCurve,
    intersection: bool,
    breaks: Vec<f64>,
}
impl CurveGeometry {
    pub fn control_points(&self) -> Option<&[[f64; 4]]> {
        (!self.intersection).then_some(self.curve.ctrl.as_slice())
    }
    pub fn is_linear(&self) -> bool {
        self.exact.is_linear()
    }
    pub fn parameter_breaks(&self) -> &[f64] {
        &self.breaks
    }
    /// Position and derivative with respect to normalized interval 0..1.
    pub fn at(&self, fraction: f64) -> Option<(Vec3, Vec3)> {
        self.exact.at(self.range, fraction)
    }

    /// A bounded polygon of this exact edge, with certified chord deviation.
    pub fn path_samples(&self, tolerance: f64, limit: usize) -> Result<Vec<Vec3>> {
        if self.intersection {
            return Err(invalid("cannot certify path sampling on a traced surface intersection"));
        }
        crate::form_curves::chord_samples(self.curve.degree, &self.curve.knots, &self.curve.ctrl, self.shape_range, tolerance, limit)
            .ok_or_else(|| invalid("could not bound pipe path sampling within the tolerance and work budget"))
    }

    /// Construct the exact tensor between two translated copies of this spline edge.
    pub fn planar_wall_strip(&self, offsets: [Vec3; 2], work: &mut usize) -> Result<Body> {
        use solidcraft_geom::bspline::{HPoint, NurbsSurface};
        if self.intersection {
            return Err(invalid("unsupported exact planar Form boundary"));
        }
        let (knots, controls) =
            crate::form_curves::swept_controls(self.curve.degree, &self.curve.knots, &self.curve.ctrl, self.shape_range, offsets, work)
                .ok_or_else(|| invalid("could not construct the exact planar Form boundary within its control budget"))?;
        let controls = controls.into_iter().map(|[x, y, z, w]| HPoint { x, y, z, w }).collect();
        let surface = NurbsSurface::from_homogeneous([self.curve.degree, 1], [knots, vec![0., 0., 1., 1.]], controls).map_err(invalid)?;
        crate::nurbs_sheet(&surface)
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
        solidcore_ops::face_surface::chart_parameters(&self.surface, point, near, tolerance)
            .ok_or_else(|| invalid("point cannot be resolved within the exact face surface tolerance"))
    }
}
/// Report which guide points come within tolerance of the actual trimmed
/// material of this body. The kernel prepares the trimmed faces in one batch.
pub fn trimmed_guides(body: &Body, points: &[Vec3], tolerance: f64) -> Result<Vec<bool>> {
    if points.is_empty() {
        return Ok(Vec::new());
    }
    let (body, order) = body.ops_body().ok_or_else(|| invalid("could not read the native trimmed face geometry"))?;
    let xyz: Vec<_> = points.iter().map(|p| [p.x, p.y, p.z]).collect();
    let distances = body.face_distances_within(order, &xyz, tolerance).map_err(invalid)?;
    let mut covered = vec![false; points.len()];
    for face in distances {
        if face.len() != covered.len() {
            return Err(invalid("incomplete trimmed face distances"));
        }
        for (seen, distance) in covered.iter_mut().zip(face) {
            *seen |= distance.is_some_and(|d| d.is_finite() && d <= tolerance);
        }
    }
    Ok(covered)
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
fn shape(source: &Curve, interval: [f64; 2]) -> Result<(EvalCurve, [f64; 2])> {
    match CurveEval::new(source).map_err(invalid)? {
        CurveEval::Spline(curve) => Ok((curve, interval)),
        CurveEval::Polyline(points) => {
            if points.len() < 2 {
                return Err(invalid("polyline needs two points"));
            }
            let last = points.len() - 1;
            let mut knots = vec![0.0, 0.0];
            knots.extend((1..last).map(|i| i as f64));
            knots.extend([last as f64; 2]);
            Ok((EvalCurve { degree: 1, knots, ctrl: points.iter().map(|p| [p.x, p.y, p.z, 1.0]).collect() }, interval))
        }
        _ => {
            if let Curve::SurfaceIntersection { leader, .. } = source {
                return shape(leader, interval);
            }
            let (form, shape_range) =
                solidcraft_geom::brep::form::curve(source, interval).ok_or_else(|| invalid("edge has no exact rational shape representation"))?;
            let CurveEval::Spline(curve) = CurveEval::new(&form).map_err(invalid)? else {
                return Err(invalid("invalid rational edge form"));
            };
            Ok((curve, shape_range))
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
    let exact = ExactCurve::new(source).map_err(invalid)?;
    let (curve, [sa, sb]) = shape(source, interval)?;
    let breaks = original_parameter_breaks(source, interval);
    let intersection = matches!(source, Curve::SurfaceIntersection { .. });
    Ok(CurveGeometry {
        curve,
        range: if forward { interval } else { [b, a] },
        shape_range: if forward { [sa, sb] } else { [sb, sa] },
        exact,
        intersection,
        breaks,
    })
}
pub fn face_geometry(body: &Body, face: usize) -> Result<FaceGeometry> {
    let source = exact_chart(body, face)?;
    let (surface, _) = solidcore_ops::face_surface::eval_surface(source.parameters(), source.domain).map_err(invalid)?;
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
    let source = exact_chart(body, face)?;
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
    let (_, map) = solidcore_ops::face_surface::eval_surface(source.parameters(), source.domain).map_err(invalid)?;
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

/// Check a raw tensor chart's complete trim against its parameter rectangle.
pub fn has_rectangular_trim(body: &Body, face: usize, span: [[f64; 2]; 2]) -> Result<bool> {
    let source = exact_chart(body, face)?;
    Ok(matches!(source.parameters(), b::Surface::Spline { .. } | b::Surface::RationalSpline { .. })
        && solidcore_ops::face_surface::rectangular_trim(source, span))
}

fn exact_chart(body: &Body, face: usize) -> Result<&b::Face> {
    let source = selected(body, face)?;
    if matches!(&source.surface, b::Surface::Blend { definition } if matches!(definition.as_ref(), b::blend::BlendDefinition::Procedural(_))) {
        return Err(invalid("the procedural blend has no exact rational chart"));
    }
    Ok(source)
}
/// A plane certified for the complete curves of these edges. Traced
/// intersections and other unproved wires return no planar support.
pub fn edge_wire_plane(body: &Body, edges: &[usize]) -> Result<Option<solidcore_record::Plane>> {
    body.require_brep("read exact wire support plane")?;
    if edges.is_empty() || edges.len() > 256 {
        return Err(KernelError::TooLarge("read 1..256 edges for a wire support plane".into()));
    }
    let mut budget = b::Budget::input();
    let mut curves = Vec::with_capacity(edges.len());
    for &id in edges {
        let curve = body.record().edges.get(id).ok_or_else(|| invalid("missing wire support edge"))?.parameters();
        if matches!(curve, Curve::SurfaceIntersection { .. }) || b::check_curve(curve, &mut budget).is_err() {
            return Ok(None);
        }
        curves.push(curve.clone());
    }
    let plane = solidcore_ops::curve_plane::plane_of(&curves).map_err(invalid)?;
    Ok(plane.and_then(|p| {
        let [x, y, z] = p.origin;
        let [a, b, c] = p.normal;
        solidcore_record::Plane::from_normal(Vec3::new(x, y, z), Vec3::new(a, b, c))
    }))
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
    fn a_point_on_the_support_outside_the_trim_is_not_covered() {
        let region = solidcore_record::Region2 {
            outer: solidcore_record::Loop2 {
                segs: [[0.0, 0.0], [3.0, 0.0], [3.0, 2.0], [0.0, 2.0]]
                    .into_iter()
                    .zip([[3.0, 0.0], [3.0, 2.0], [0.0, 2.0], [0.0, 0.0]])
                    .map(|([ax, ay], [bx, by])| solidcore_record::Seg2::Line {
                        a: solidcore_record::Vec2::new(ax, ay),
                        b: solidcore_record::Vec2::new(bx, by),
                    })
                    .collect(),
            },
            holes: Vec::new(),
        };
        let plane = solidcore_record::Plane { origin: Vec3::ZERO, x: Vec3::X, y: Vec3::Y };
        let body = crate::patch_region(&plane, &region).unwrap();
        let outside = Vec3::new(4.0, 1.0, 0.0);
        assert!(face_geometry(&body, 0).unwrap().parameters(outside, None, 1e-6).is_ok());
        assert_eq!(trimmed_guides(&body, &[outside, Vec3::new(1.0, 1.0, 0.0)], 1e-6).unwrap(), vec![false, true]);
    }

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
