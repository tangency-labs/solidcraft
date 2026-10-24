//! A circular face cut inward on a cone widening toward that face (#3303).
//! The cylinder touches the cone at its rim, but its wall cuts no material. A
//! certified enclosing cylinder can be replaced by a wider square prism, keeping
//! every cap that cuts material in its original plane.

use solidcraft_geom::{Loop2, Plane, Region2, Seg2, Vec2, Vec3};
use truck_modeling::{self as mt, BoundedCurve, ParametricCurve};

use crate::body::from_p3;
use crate::history::{Change, FaceProv};
use crate::{Body, BoolOp, FaceSource, Result, boolean_record as recorded};

/// One full circle, certified by its segments rather than sampled points.
fn circle(region: &Region2, tol: f64) -> Option<(Vec2, f64)> {
    if !region.holes.is_empty() || region.outer.segs.is_empty() || region.outer.segs.len() > 32 {
        return None;
    }
    let Seg2::Arc { center, radius, .. } = *region.outer.segs.first()? else { return None };
    let mut turn = 0.;
    for segment in &region.outer.segs {
        let Seg2::Arc { center: c, radius: r, sweep, .. } = *segment else { return None };
        if !c.is_finite() || !r.is_finite() || c.dist(center) + (r - radius).abs() > tol || !sweep.is_finite() || sweep <= 0. {
            return None;
        }
        turn += sweep;
    }
    (center.is_finite() && radius.is_finite() && radius > 0. && (turn - std::f64::consts::TAU).abs() < 1e-10 && region.outer.is_closed(1e-8))
        .then_some((center, radius))
}

/// Read a complete circular cap from its current geometry, independently of
/// source metadata (which can survive an affine placement).
fn circular_cap(face: &mt::Face, tol: f64, work: &mut usize) -> Option<(Vec3, f64, Vec3)> {
    if face.boundaries().len() != 1 || face.edge_iter().count() > 32 {
        return None;
    }
    let mt::Surface::Plane(p) = face.oriented_surface() else { return None };
    let n = p.normal();
    let normal = Vec3::new(n.x, n.y, n.z).normalized()?;
    let plane = Plane::from_normal(from_p3(p.origin()), normal)?;
    if face.vertex_iter().any(|v| plane.height(from_p3(v.point())).abs() > tol) {
        return None;
    }
    let segments = face.edge_iter().map(|edge| crate::prism_curve::segment(&edge, &plane, tol, work)).collect::<Option<Vec<_>>>()?;
    let mut segments: Vec<_> = segments.into_iter().flatten().collect();
    if segments.iter().all(|s| matches!(s, Seg2::Arc { sweep, .. } if *sweep < 0.)) {
        segments = segments.into_iter().rev().map(|s| s.reversed()).collect();
    }
    let (center, radius) = circle(&Region2 { outer: Loop2 { segs: segments }, holes: vec![] }, tol)?;
    Some((plane.to_world(center), radius, normal))
}

/// The actual cutter has coaxial circular caps and walls swept along their
/// normal. Source segments alone cannot certify a scaled or sheared cutter.
fn cylindrical(tool: &Body, frame: Plane, radius: f64, height: f64, top: usize, bottom: usize, tol: f64) -> Option<()> {
    let axis = frame.normal();
    let mut work = 2_000_000usize;
    for (i, face) in tool.solid.face_iter().enumerate() {
        if i == top || i == bottom {
            let (center, r, normal) = circular_cap(face, tol, &mut work)?;
            let (h, sign) = if i == top { (0., 1.) } else { (height, -1.) };
            if (frame.height(center) - h).abs() > tol
                || (center - frame.origin).cross(axis).len() + (r - radius).abs() > tol
                || normal.dot(axis) * sign < 1. - 1e-10
            {
                return None;
            }
        } else if !crate::prism::wall_along(&face.oriented_surface(), axis, tol, &mut work) {
            return None;
        }
    }
    Some(())
}

/// Certify an entire rational surface inside the cylinder. For a fixed parameter,
/// positive tensor-product weights make the surface a convex combination of its
/// control-row curves. Certify those curves over their whole spans; checking the
/// seam vertices or fitting a cone through samples would miss an outward bulge.
fn surface_bound(s: &mt::NurbsSurface<mt::Vector4>, frame: Plane, radius: f64, tol: f64, work: &mut usize) -> Option<(f64, f64)> {
    let net = s.control_points();
    let (rows, cols) = (net.len(), net.first()?.len());
    if rows < 2 || cols < 2 || rows.saturating_mul(cols) > 4096 || net.iter().any(|r| r.len() != cols) {
        return None;
    }
    let max_w = net.iter().flatten().map(|p| p.w).fold(0., f64::max);
    let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
    for p in net.iter().flatten() {
        if ![p.x, p.y, p.z, p.w].iter().all(|v| v.is_finite()) || p.w <= 0. || p.w / max_w < 1e-12 {
            return None;
        }
        let h = frame.height(Vec3::new(p.x / p.w, p.y / p.w, p.z / p.w));
        lo = lo.min(h);
        hi = hi.max(h);
    }
    for transpose in [false, true] {
        let (count, length, knots) = if transpose { (cols, rows, s.uknot_vec()) } else { (rows, cols, s.vknot_vec()) };
        if knots.len() > 8192 {
            return None;
        }
        let mut accepted = true;
        for i in 0..count {
            let controls =
                (0..length).map(|j| if transpose { net.get(j)?.get(i).copied() } else { net.get(i)?.get(j).copied() }).collect::<Option<Vec<_>>>()?;
            let curve = mt::Curve::NurbsCurve(mt::NurbsCurve::new(mt::BSplineCurve::try_new(knots.clone(), controls).ok()?));
            let (a, b) = curve.range_tuple();
            let edge = mt::Edge::new(&mt::Vertex::new(curve.subs(a)), &mt::Vertex::new(curve.subs(b)), curve);
            let Some(segments) = crate::prism_curve::segment(&edge, &frame, tol, work) else {
                accepted = false;
                break;
            };
            if !segments.iter().all(|segment| match *segment {
                Seg2::Arc { center, radius: r, .. } => center.len() + r <= radius + tol,
                Seg2::Line { a, b } => a.len().max(b.len()) <= radius + tol,
                _ => false,
            }) {
                accepted = false;
                break;
            }
        }
        if accepted {
            return Some((lo, hi));
        }
    }
    None
}

/// Two circular caps plus surfaces certified within their axial slab and the
/// tool's cylinder bound the complete closed solid, including between seams.
fn enclosed(target: &Body, frame: Plane, radius: f64, tol: f64) -> Option<(f64, f64)> {
    if target.is_mesh() || target.shell_count() != 1 || target.face_count() != 4 {
        return None;
    }
    let axis = frame.normal();
    let mut caps = Vec::new();
    let mut spans = Vec::new();
    let mut work = 2_000_000usize;
    for face in target.solid.face_iter() {
        if face.boundaries().len() != 1 || face.edge_iter().count() > 32 {
            return None;
        }
        match face.oriented_surface() {
            mt::Surface::Plane(_) => {
                let (center, r, normal) = circular_cap(face, tol, &mut work)?;
                if normal.cross(axis).len() > 1e-10 {
                    return None;
                }
                if (center - frame.origin).cross(axis).len() + r > radius + tol {
                    return None;
                }
                caps.push((frame.height(center), normal.dot(axis)));
            }
            mt::Surface::NurbsSurface(s) => spans.push(surface_bound(&s, frame, radius, tol, &mut work)?),
            _ => return None,
        }
    }
    caps.sort_by(|a, b| a.0.total_cmp(&b.0));
    let [(lo, lower), (hi, upper)] = caps.as_slice() else { return None };
    if hi - lo <= tol || *lower > -1. + 1e-10 || *upper < 1. - 1e-10 || spans.len() != 2 {
        return None;
    }
    spans.iter().all(|(a, b)| *a >= lo - tol && *b <= hi + tol).then_some((*lo, *hi))
}

/// Replace a construction-certified cylindrical tool only when it encloses the
/// entire target radially. The replacement's generated walls must never survive
/// in the result: their empty histories also prevent inventing face provenance.
fn wider_tool(target: &Body, tool: &Body) -> Option<Body> {
    if tool.is_mesh() || tool.shell_count() != 1 || tool.face_count() != 4 {
        return None;
    }
    let sources = tool.face_sources()?;
    let (top, region, plane) = sources.iter().enumerate().find_map(|(i, source)| match source {
        Some(FaceSource::ExtrudeCap { high: true, region, plane }) => Some((i, region, *plane)),
        _ => None,
    })?;
    let (bottom, base) = sources.iter().enumerate().find_map(|(i, source)| match source {
        Some(FaceSource::ExtrudeCap { high: false, region: r, plane }) if r == region => Some((i, *plane)),
        _ => None,
    })?;
    let size = target.size().max(tool.size());
    let tol = (size * 1e-10).max(1e-10);
    let (center, radius) = circle(region, tol)?;
    if sources.iter().enumerate().any(|(i, source)| i != top && i != bottom && !matches!(source, Some(FaceSource::ExtrudeSide { segment: Seg2::Arc { center: c, radius: r, .. }, .. }) if c.dist(center) + (*r - radius).abs() <= tol)) {
        return None;
    }
    let frame = Plane { origin: plane.to_world(center), ..plane };
    let height = plane.height(base.origin);
    if !height.is_finite() || height >= -tol || base.normal().dot(plane.normal()) > -1. + 1e-10 {
        return None;
    }
    cylindrical(tool, frame, radius, height, top, bottom, tol)?;
    let (low, high) = enclosed(target, frame, radius, tol)?;
    let margin = (size * 0.1).max(1e-4);
    let lo = if height <= low + tol { height - margin } else { height };
    let hi = if high <= tol { margin } else { 0. };
    let r = radius + margin;
    let square = Region2 { outer: Loop2::polygon(&[Vec2::new(-r, -r), Vec2::new(r, -r), Vec2::new(r, r), Vec2::new(-r, r)]), holes: vec![] };
    let widened = crate::extrude(&frame, &[square], lo, hi).ok()?.pop()?;
    let faces = widened
        .face_sources()?
        .iter()
        .map(|source| match source {
            Some(FaceSource::ExtrudeCap { high: true, .. }) if hi == 0. => crate::record::from_face(0, top, Change::Modified),
            Some(FaceSource::ExtrudeCap { high: false, .. }) if lo == height => crate::record::from_face(0, bottom, Change::Modified),
            _ => FaceProv { origins: vec![], change: Change::Generated },
        })
        .collect();
    let history = crate::record::from_faces(&widened.solid, 1, faces);
    Some(widened.with_checked_history(Some(history), &crate::record::counts(&[tool])))
}

pub(crate) fn cut(a: &Body, b: &Body, run: &dyn Fn(&Body, &Body, BoolOp) -> Result<Option<Body>>) -> Option<Result<Option<Body>>> {
    let tool = crate::guard("enclosing cylinder cut", || Ok(wider_tool(a, b))).ok()??;
    let result = run(a, &tool, BoolOp::Cut).ok()?;
    crate::boolean_guard::check_retry(a, b, BoolOp::Cut, result.as_ref()).ok()?;
    let result = match result {
        Some(body) => {
            let history = recorded::through(&body, 1, tool.history())?;
            if history.faces.iter().any(|f| f.origins.is_empty()) {
                return None;
            }
            Some(body.with_checked_history(Some(history), &crate::record::counts(&[a, b])))
        }
        None => None,
    };
    Some(Ok(result))
}

#[cfg(test)]
#[path = "frustum_cut_tests.rs"]
mod tests;
