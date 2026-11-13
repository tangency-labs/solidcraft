//! Planar Form walls assembled from the exact native sheet and its boundary curves.
use crate::kernel::history::{BodyHistory, Change, EdgeOrigin, EdgeProv, FaceOrigin, FaceProv, History};
use crate::{DocError, Result, kernel};
use solidcore_record::Vec3;
use solidcore_record::brep::{Curve, Edge, Shell};
use solidcraft_geom::brep::eval::CurveEval;
use solidcraft_geom::bspline::{HPoint, NurbsSurface, segment};
use solidcraft_geom::{PlaneGeom, Vec3Geom};
use std::collections::BTreeSet;

const MAX_EDGES: usize = 4096;
const MAX_CONTROLS: usize = 262_144;

fn invalid(why: impl Into<String>) -> DocError {
    DocError::Invalid(why.into())
}

fn planar_normal(body: &kernel::Body) -> Option<Vec3> {
    let record = body.record();
    if !body.is_surface()
        || body.is_mesh()
        || record.edges.len() > MAX_EDGES
        || !matches!(record.shells.as_slice(), [Shell::Faces { wires, .. }] if wires.is_empty())
    {
        return None;
    }
    let plane = body.planar_face(0)?;
    let normal = plane.normal();
    for index in 1..body.face_count() {
        let next = body.planar_face(index)?;
        let height_tolerance = (64.0 * f64::EPSILON * (1.0 + plane.origin.len().max(next.origin.len()))).min(1e-7);
        if normal.dist(next.normal()) > 64.0 * f64::EPSILON || plane.height(next.origin).abs() > height_tolerance {
            return None;
        }
    }
    let mut spline_boundary = false;
    let mut free = 0;
    for edge in &record.edges {
        match edge.uses.len() {
            1 if edge.vertices[0] != edge.vertices[1] => {
                free += 1;
                match edge.parameters() {
                    Curve::Line { .. } => {}
                    Curve::Spline { .. } | Curve::RationalSpline { .. } => spline_boundary = true,
                    _ => return None,
                }
            }
            2 => {}
            _ => return None,
        }
    }
    (spline_boundary && free >= 3).then_some(normal)
}

/// A direct wall for coplanar fitted Forms with spline boundaries. Ordinary
/// analytic and curved-surface Thicken keep their existing construction.
pub fn can_build_planar_wall(body: &kernel::Body) -> bool {
    planar_normal(body).is_some()
}

/// The two cap operands describe opposite sides of the same source body.
/// Keeping them distinct in the history gives both offsets stable names.
pub(crate) fn build(body: &kernel::Body, thickness: f64, symmetric: bool) -> Result<(kernel::Body, History)> {
    let normal = planar_normal(body).ok_or_else(|| invalid("Thicken requires the saved Form's coplanar spline boundary"))?;
    if !(thickness.is_finite() && thickness.abs() > 1e-9) {
        return Err(invalid("Thicken requires a finite nonzero thickness"));
    }
    let delta = normal * if symmetric { thickness.abs() } else { thickness };
    let start = if symmetric { -delta / 2.0 } else { Vec3::ZERO };
    let first = kernel::transform(body, start, Vec3::ZERO, Vec3::Z, 0.0)?;
    let second = kernel::transform(body, start + delta, Vec3::ZERO, Vec3::Z, 0.0)?;
    let mut remaining = MAX_CONTROLS;
    let mut side_edges = Vec::new();
    let mut sides = Vec::new();
    for (index, edge) in body.record().edges.iter().enumerate().filter(|(_, edge)| edge.uses.len() == 1) {
        sides.push(strip(edge, start, delta, &mut remaining)?);
        side_edges.push(index);
    }
    let parts: Vec<_> = [&first, &second].into_iter().chain(&sides).collect();
    let wall = kernel::stitch(&parts, (thickness.abs() * 0.01).min(1e-7))?;
    if wall.is_surface()
        || wall.shell_count() != 1
        || wall.record().material_regions().len() != 1
        || wall.face_count() != parts.iter().map(|part| part.face_count()).sum::<usize>()
        || !kernel::validity(&wall).ok()
    {
        return Err(invalid("Thicken could not join the planar Form's complete wall into one valid solid"));
    }
    let history = lineage(&wall, body, &parts, &side_edges, symmetric)?;
    Ok((wall, history))
}

fn strip(edge: &Edge, start: Vec3, delta: Vec3, remaining: &mut usize) -> Result<kernel::Body> {
    let [a, b] = edge.interval;
    let (degree, knots, controls) = match edge.parameters() {
        Curve::Line { .. } => {
            let eval = CurveEval::new(edge.parameters()).map_err(|error| invalid(error.to_string()))?;
            let point = |t| eval.point(t).map(|point| HPoint::weighted(point, 1.0)).ok_or_else(|| invalid("invalid planar Form line station"));
            (1, vec![a, a, b, b], vec![point(a)?, point(b)?])
        }
        Curve::Spline { knots, points } => {
            spend(remaining, points.len())?;
            let points = points.iter().map(|&[x, y, z]| HPoint::weighted(Vec3::new(x, y, z), 1.0)).collect();
            trimmed(knots, points, a, b)?
        }
        Curve::RationalSpline { knots, homogeneous } => {
            spend(remaining, homogeneous.len())?;
            let points = homogeneous.iter().map(|&[x, y, z, w]| HPoint { x, y, z, w }).collect();
            trimmed(knots, points, a, b)?
        }
        _ => return Err(invalid("unsupported exact planar Form boundary")),
    };
    spend(remaining, controls.len().saturating_mul(2))?;
    let row = |at: Vec3| controls.iter().map(move |p| HPoint { x: p.x + p.w * at.x, y: p.y + p.w * at.y, z: p.z + p.w * at.z, w: p.w });
    let surface = NurbsSurface::from_homogeneous([degree, 1], [knots, vec![0.0, 0.0, 1.0, 1.0]], row(start).chain(row(start + delta)).collect())
        .map_err(|error| invalid(error.to_string()))?;
    Ok(kernel::nurbs_sheet(&surface)?)
}

fn trimmed(knots: &[f64], controls: Vec<HPoint>, a: f64, b: f64) -> Result<(usize, Vec<f64>, Vec<HPoint>)> {
    let degree = knots.len().checked_sub(controls.len() + 1).ok_or_else(|| invalid("invalid planar Form spline degree"))?;
    let (knots, controls) = segment(degree, knots, &controls, a, b).map_err(|error| invalid(error.to_string()))?;
    Ok((degree, knots, controls))
}

fn spend(remaining: &mut usize, amount: usize) -> Result<()> {
    *remaining = remaining.checked_sub(amount).ok_or_else(|| invalid("Thicken exceeded its planar wall control budget"))?;
    Ok(())
}

fn lineage(wall: &kernel::Body, source: &kernel::Body, parts: &[&kernel::Body], side_edges: &[usize], symmetric: bool) -> Result<History> {
    let missing = || invalid("Thicken did not retain the planar Form's exact face and edge history");
    let raw = wall.history().ok_or_else(missing)?;
    raw.check(wall, parts)?;
    let mut seen = BTreeSet::new();
    let faces = raw
        .faces
        .iter()
        .map(|provenance| {
            let [FaceOrigin::Face { operand, face }] = *provenance.origins.as_slice() else { return Err(missing()) };
            if !seen.insert((operand, face)) {
                return Err(missing());
            }
            let (origin, change) = match operand {
                0 if !symmetric => (FaceOrigin::Face { operand, face }, Change::Kept),
                0 | 1 => (FaceOrigin::Offset { operand, face }, Change::Generated),
                side if face == 0 => (FaceOrigin::Edge { operand: 0, edge: *side_edges.get(side - 2).ok_or_else(missing)? }, Change::Generated),
                _ => return Err(missing()),
            };
            Ok(FaceProv { origins: vec![origin], change })
        })
        .collect::<Result<Vec<_>>>()?;
    let edges = raw
        .edges
        .iter()
        .map(|edge| {
            let mut kept = edge.origins.iter().filter_map(|origin| match *origin {
                EdgeOrigin::Edge { operand: 0, edge } if !symmetric => Some(edge),
                _ => None,
            });
            let origin = if let Some(edge) = kept.next() {
                if kept.next().is_some() {
                    return Err(missing());
                }
                EdgeOrigin::Edge { operand: 0, edge }
            } else {
                let [a, b] = *edge.faces.as_slice() else { return Err(missing()) };
                let face = |index: usize| faces.get(index).and_then(|p| p.origins.first()).copied().ok_or_else(missing);
                EdgeOrigin::meet(face(a)?, face(b)?)
            };
            Ok(EdgeProv { faces: edge.faces.clone(), origins: vec![origin] })
        })
        .collect::<Result<Vec<_>>>()?;
    let history = History { operands: 2, faces, edges };
    history.check(wall, &[source, source])?;
    Ok(history)
}
