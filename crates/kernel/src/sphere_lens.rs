//! A lens extruded from a projected outline (#2750, #3192). One cylinder
//! encloses the complete target and only touches its outline. Removing that
//! redundant constraint gives the same cut without a tangent intersection.

use solidcraft_geom::{Loop2, Plane, Region2, Seg2, Vec2, Vec3};

use crate::boolean_record as recorded;
use crate::history::{Change, FaceOrigin, FaceProv};
use crate::{Body, BoolOp, FaceSource, FaceSurface, Result, face_surface};

/// Maximum signed height of a line or circular arc, including interior extrema.
fn height(segment: Seg2, plane: Plane, cap: Plane) -> Option<f64> {
    let at = |p| cap.height(plane.to_world(p));
    let ends = at(segment.start()).max(at(segment.end()));
    match segment {
        Seg2::Line { .. } => Some(ends),
        Seg2::Arc { center, radius, .. } => {
            let n = cap.normal();
            let direction = Vec2::new(n.dot(plane.x), n.dot(plane.y));
            let probe = center + direction.normalized().unwrap_or(Vec2::X) * radius;
            Some(ends.max(at(segment.point_at(segment.closest_param(probe)))))
        }
        _ => None,
    }
}

/// Recognize a quarter ball by its complete boundary: one spherical face, two
/// perpendicular semicircular caps sharing their diameter, each cap inside the
/// other's outward halfspace. The opposite cap orientation (three quarters of a
/// ball) is refused. Whole-span curve certificates prove the caps, not samples.
fn enclosing_ball(body: &Body, tol: f64) -> Option<(Vec3, f64, Vec<Plane>)> {
    if body.is_mesh() || body.shell_count() != 1 || body.face_count() != 3 {
        return None;
    }
    let surfaces = (0..3).map(|i| face_surface(body, i)).collect::<Option<Vec<_>>>()?;
    let (sphere, center, radius) = surfaces.iter().enumerate().find_map(|(i, surface)| match *surface {
        FaceSurface::Sphere { center, radius } => Some((i, center, radius)),
        _ => None,
    })?;
    let spherical = body.solid.face_iter().nth(sphere)?;
    if spherical.boundaries().len() != 1 || spherical.edge_iter().count() != 2 {
        return None;
    }
    let sphere_edges: std::collections::HashSet<_> = spherical.edge_iter().map(|e| e.id()).collect();
    let caps: Vec<_> = surfaces
        .iter()
        .filter_map(|surface| match *surface {
            FaceSurface::Plane { normal, point } => Plane::from_normal(point, normal),
            _ => None,
        })
        .collect();
    let (cap0, cap1) = (*caps.first()?, *caps.get(1)?);
    if caps.len() != 2 || cap0.normal().dot(cap1.normal()).abs() > 1e-9 || cap0.height(center).abs() > tol || cap1.height(center).abs() > tol {
        return None;
    }
    let mut work = 2_000_000usize;
    let mut diameters = Vec::new();
    for (i, face) in body.solid.face_iter().enumerate() {
        if i == sphere {
            continue;
        }
        let FaceSurface::Plane { normal, point } = *surfaces.get(i)? else { return None };
        let plane = Plane::from_normal(point, normal)?;
        if face.boundaries().len() != 1 || face.edge_iter().count() != 2 {
            return None;
        }
        let c = plane.to_local(center);
        let mut arc = 0.;
        let mut lines = 0;
        for edge in face.edge_iter() {
            if plane.height(crate::body::from_p3(edge.front().point())).abs() > tol
                || plane.height(crate::body::from_p3(edge.back().point())).abs() > tol
            {
                return None;
            }
            for segment in crate::prism_curve::segment(&edge, &plane, tol, &mut work)? {
                match segment {
                    Seg2::Line { a, b } if (a + b - c * 2.).len() <= tol && (a.dist(c) - radius).abs() <= tol => {
                        diameters.push(edge.id());
                        lines += 1;
                    }
                    Seg2::Arc { center: q, radius: r, sweep, .. } if q.dist(c) + (r - radius).abs() <= tol && sphere_edges.contains(&edge.id()) => {
                        arc += sweep.abs();
                    }
                    _ => return None,
                }
                if height(segment, plane, cap0)? > tol || height(segment, plane, cap1)? > tol {
                    return None;
                }
            }
        }
        if lines != 1 || (arc - std::f64::consts::PI).abs() > 1e-8 {
            return None;
        }
    }
    if diameters.len() != 2 || diameters.first()? != diameters.get(1)? {
        return None;
    }
    Some((center, radius, caps))
}

/// Prove containment in an infinite cylinder from complete surface representations.
/// A revolution preserves distance to its axis; positive rational control hulls
/// bound its whole generating curve. A planar cap lies in the convex disc that
/// contains every boundary segment. Unsupported surfaces decline the retry.
fn enclosed_in_cylinder(body: &Body, frame: Plane, center: Vec2, radius: f64, tol: f64) -> bool {
    use crate::body::from_p3;
    use truck_modeling as mt;

    if body.is_mesh()
        || body.shell_count() != 1
        || body.face_count() == 0
        || body.face_count() > 128
        || !radius.is_finite()
        || radius <= 0.
        || !tol.is_finite()
        || tol <= 0.
    {
        return false;
    }
    let origin = frame.to_world(center);
    let axis = frame.normal();
    if !origin.is_finite() {
        return false;
    }
    let mut work = 2_000_000usize;
    for face in body.solid.face_iter() {
        match face.surface() {
            mt::Surface::RevolutedCurve(processor) => {
                // The identity placement keeps this proof independent of sampled
                // analytic surface recognition and of arbitrary affine scales.
                if *processor.transform() != <mt::Matrix4 as mt::One>::one() {
                    return false;
                }
                let surface = processor.entity();
                let direction = surface.axis();
                let Some(direction) = Vec3::new(direction.x, direction.y, direction.z).normalized() else { return false };
                let source_origin = from_p3(surface.origin());
                let delta = source_origin - origin;
                let lean = direction.cross(axis).len();
                let offset = (delta - axis * delta.dot(axis)).len();
                if !source_origin.is_finite() || lean > 1e-10 || offset > tol {
                    return false;
                }
                let inside = |point: Vec3| {
                    let local = point - source_origin;
                    let height = local.dot(direction);
                    // Include the placement residual over the whole rotation,
                    // rather than treating nearly coaxial axes as identical.
                    let bound = (local - direction * height).len() + offset + height.abs() * lean;
                    point.is_finite() && bound <= radius + tol
                };
                let points: Vec<Vec3> = match surface.entity_curve() {
                    mt::Curve::Line(line) => vec![from_p3(line.0), from_p3(line.1)],
                    mt::Curve::BSplineCurve(curve) if curve.control_points().len() <= 4096 => {
                        curve.control_points().iter().map(|p| from_p3(*p)).collect()
                    }
                    mt::Curve::NurbsCurve(curve) if curve.control_points().len() <= 4096 => {
                        if curve.control_points().iter().any(|p| !p.w.is_finite() || p.w <= 0.) {
                            return false;
                        }
                        curve.control_points().iter().map(|p| Vec3::new(p.x / p.w, p.y / p.w, p.z / p.w)).collect()
                    }
                    _ => return false,
                };
                let Some(left) = work.checked_sub(points.len()) else { return false };
                work = left;
                if points.is_empty() || !points.into_iter().all(inside) {
                    return false;
                }
            }
            mt::Surface::Plane(plane) => {
                let normal = plane.normal();
                let normal = Vec3::new(normal.x, normal.y, normal.z);
                if normal.cross(axis).len() > 1e-10 || face.edge_iter().count() > 4096 {
                    return false;
                }
                for edge in face.edge_iter() {
                    let Some(segments) = crate::prism_curve::segment(&edge, &frame, tol, &mut work) else { return false };
                    for segment in segments {
                        let contained = match segment {
                            Seg2::Line { a, b } => a.dist(center).max(b.dist(center)) <= radius + tol,
                            Seg2::Arc { center: c, radius: r, .. } => c.dist(center) + r <= radius + tol,
                            _ => false,
                        };
                        if !contained {
                            return false;
                        }
                    }
                }
            }
            _ => return false,
        }
    }
    true
}

/// Replace only a construction-certified, convex two-circle lens. Its cutting cylinder stays exact; a cap moves
/// only into a halfspace proved to be outside the target. Added material is outside the target.
fn relaxed_tool(a: &Body, b: &Body) -> Option<Body> {
    if b.is_mesh() || b.shell_count() != 1 || !(4..=18).contains(&b.face_count()) {
        return None;
    }
    let tol = (a.size().max(b.size()) * 1e-9).max(1e-8);
    let sources = b.face_sources()?;
    let (region, plane) = sources.iter().find_map(|source| match source {
        Some(FaceSource::ExtrudeCap { high: true, region, plane }) => Some((region, *plane)),
        _ => None,
    })?;
    let bottom = sources.iter().find_map(|source| match source {
        Some(FaceSource::ExtrudeCap { high: false, region: r, plane }) if r == region => Some(*plane),
        _ => None,
    })?;
    if !region.holes.is_empty() || !(2..=16).contains(&region.outer.segs.len()) || !region.outer.is_closed(tol) {
        return None;
    }
    // A circular wall may be split at the sketch's seams. Recognize the two
    // supporting circles across those pieces without changing either circle.
    let mut circles: Vec<(Vec2, f64)> = Vec::new();
    for segment in &region.outer.segs {
        let Seg2::Arc { center, radius, sweep, .. } = *segment else { return None };
        if !(sweep > 0. && sweep < std::f64::consts::TAU - 1e-9) {
            return None;
        }
        if !circles.iter().any(|(c, r)| c.dist(center) + (r - radius).abs() <= tol) {
            if circles.len() == 2 {
                return None;
            }
            circles.push((center, radius));
        }
    }
    if circles.len() != 2 {
        return None;
    }
    let ((c0, r0), (c1, r1)) = (*circles.first()?, *circles.get(1)?);
    let distance = c0.dist(c1);
    if distance <= (r0 - r1).abs() + tol || distance >= r0 + r1 - tol {
        return None;
    }
    // Positive arcs alone also describe a union. Prove that every whole arc
    // lies inside the other disc, including an extremum between its ends.
    for segment in &region.outer.segs {
        let Seg2::Arc { center, radius, .. } = *segment else { return None };
        let other = if center.dist(c0) + (radius - r0).abs() <= tol { (c1, r1) } else { (c0, r0) };
        let direction = (center - other.0).normalized()?;
        let probe = center + direction * radius;
        let far = segment.point_at(segment.closest_param(probe));
        if segment.start().dist(other.0).max(segment.end().dist(other.0)).max(far.dist(other.0)) > other.1 + tol {
            return None;
        }
    }
    let (redundant, caps) = if let Some((center, radius, caps)) = enclosing_ball(a, tol) {
        let projected = plane.to_local(center);
        (circles.iter().position(|(c, r)| projected.dist(*c) + radius <= *r + tol)?, caps)
    } else {
        (circles.iter().position(|(c, r)| enclosed_in_cylinder(a, plane, *c, *r, tol))?, Vec::new())
    };
    let (c, r) = *circles.get(1 - redundant)?;
    let height = plane.height(bottom.origin);
    if !height.is_finite() || height >= -tol || plane.normal().dot(bottom.normal()) > -1.0 + 1e-9 {
        return None;
    }
    // The proven quarter ball lies behind each cap. Extend a coincident tool
    // end outward into that empty halfspace; this avoids the generic nudge's
    // paper-thin lid across the opening, without moving the cutting cylinder.
    let on_cap = |end: Plane| caps.iter().any(|cap| cap.normal().dot(end.normal()) > 1.0 - 1e-9 && cap.height(end.origin).abs() <= tol);
    let (low, high) = (on_cap(bottom), on_cap(plane));
    if !caps.is_empty() && !low && !high {
        return None;
    }
    let margin = (a.size().max(b.size()) * 0.01).max(1e-5);
    let lo = height - if low { margin } else { 0. };
    let hi = if high { margin } else { 0. };
    let mut tool =
        crate::extrude(&plane, &[Region2 { outer: Loop2::circle_from(c, r, crate::build::SEAM_ANGLE), holes: vec![] }], lo, hi).ok()?.pop()?;
    // Each rebuilt surface names its original tool face. Extended portions never
    // meet the target, so every face used in the cut still lies in that input face.
    let mut faces = Vec::new();
    for i in 0..tool.face_count() {
        let surface = face_surface(&tool, i)?;
        let origins: Vec<_> = (0..b.face_count())
            .filter(|j| match (surface, face_surface(b, *j)) {
                (FaceSurface::Plane { normal: n, .. }, Some(FaceSurface::Plane { normal: m, .. })) => n.dot(m) > 1.0 - 1e-9,
                (FaceSurface::Cylinder { origin: p, axis: n, radius: r }, Some(FaceSurface::Cylinder { origin: q, axis: m, radius: s })) => {
                    n.dot(m).abs() > 1.0 - 1e-9 && (p - q).cross(n).len() + (r - s).abs() <= tol
                }
                _ => false,
            })
            .map(|face| FaceOrigin::Face { operand: 0, face })
            .collect();
        if origins.is_empty() {
            return None;
        }
        let change = if origins.len() > 1 { Change::Merged } else { Change::Modified };
        faces.push(FaceProv { origins, change });
    }
    let history = crate::record::from_faces(&tool.solid, 1, faces);
    tool = tool.with_checked_history(Some(history), &crate::record::counts(&[b]));
    Some(tool)
}

pub(crate) fn cut(a: &Body, b: &Body, run: &dyn Fn(&Body, &Body, BoolOp) -> Result<Option<Body>>) -> Option<Result<Option<Body>>> {
    let tool = crate::guard("sphere lens tool", || Ok(relaxed_tool(a, b))).ok()??;
    let result = run(a, &tool, BoolOp::Cut).ok()?;
    crate::boolean_guard::check_retry(a, b, BoolOp::Cut, result.as_ref()).ok()?;
    Some(Ok(result.map(|body| {
        let history = recorded::through(&body, 1, tool.history());
        body.with_checked_history(history, &crate::record::counts(&[a, b]))
    })))
}

#[cfg(test)]
#[path = "sphere_lens_tests.rs"]
mod tests;
