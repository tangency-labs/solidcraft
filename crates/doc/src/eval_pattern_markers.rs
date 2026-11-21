//! Anchor pattern selection markers on the geometry of each tool.

use solidcore_record::Vec3;
use solidcraft_geom::Aabb3;
use solidcraft_geom::prelude::*;

use super::{
    Body, DocError, Document, Feature, FeatureKind, ModelState, Operation, Result, constructor_profile_in_frame, hole_drill_scope, hole_on_surface,
    kernel, line_meets_box, miss_scope,
};

/// The two ends of an extrusion's profile centre, in the feature's frame.
/// Showing the near one keeps a circular cut's marker at its visible opening.
pub(super) fn tool_positions(doc: &Document, st: &ModelState, src: &Feature, tool: &Body) -> Result<[Vec3; 2]> {
    let bounds = tool.exact_bounds()?;
    let centre = bounds.center();
    let FeatureKind::Extrude { sketch, .. } = &src.kind else { return Ok([centre; 2]) };
    let sketch = st.sketch(sketch.0).ok_or_else(|| DocError::Unknown(format!("sketch {sketch}")))?;
    let direction = constructor_profile_in_frame(doc, sketch, src)?.plane.normal();
    let reach = bounds.diagonal().max(1.0);
    // A round profile has an exact centre on its cylindrical wall's axis. For other profiles,
    // keep the bounds centre, even when a rounded corner also has a cylindrical face.
    let axis = (0..tool.face_count()).find_map(|face| {
        let kernel::FaceSurface::Cylinder { origin, axis, .. } = kernel::face_surface(tool, face)? else { return None };
        let point = origin + direction * (centre - origin).dot(direction);
        (axis.vec().cross(direction).len() <= 1e-7 && point.dist(centre) <= reach * 1e-7).then_some(point)
    });
    let centre = axis.unwrap_or(centre);
    let mut caps = (0..tool.face_count()).filter_map(|face| {
        let plane = tool.planar_face(face)?;
        (plane.normal().cross(direction).len() <= 1e-7).then_some((plane.origin - centre).dot(direction))
    });
    let cap_range = caps.next().map(|first| caps.fold([first; 2], |[lo, hi], level| [lo.min(level), hi.max(level)]));
    if let Some([lo, hi]) = cap_range.filter(|[lo, hi]| hi > lo) {
        return Ok([centre + direction * lo, centre + direction * hi]);
    }
    // A curved or slanted end needs the exact tool boundary at the profile centre. A centre
    // outside the material (for example an annulus) can still use the available cap plane.
    let fallback = cap_range.map_or(centre, |[level, _]| centre + direction * level);
    Ok([-direction, direction]
        .map(|outward| tool.fire_ray(centre + outward * (2.0 * reach), -outward).ok().flatten().map_or(fallback, |hit| hit.point)))
}

/// The entrance of a hole at its translated spot, using the drilling direction and target scope.
pub(super) fn hole_positions(st: &ModelState, src: &Feature, at: Vec3, dir: Vec3) -> [Vec3; 2] {
    [hole_on_surface(st, at, dir, hole_drill_scope(src)); 2]
}

/// Put an overlong cut's translated cap markers on the target surfaces the cut enters.
/// A cap already in the material, or a cut whose centreline meets no target, stays put.
pub(super) fn clip_cut_positions(st: &ModelState, src: &Feature, positions: [Vec3; 2]) -> [Vec3; 2] {
    let FeatureKind::Extrude { operation: Operation::Cut, targets, .. } = &src.kind else { return positions };
    let [first, second] = positions;
    let along = second - first;
    let Some(dir) = along.normalized() else { return positions };
    let span = along.len();
    let margin = (span * 1e-10).clamp(1e-7, 1e-4);
    let segment: Aabb3 = positions.into_iter().collect();
    let targets = crate::plain(targets);

    [(first, dir), (second, -dir)].map(|(cap, inward)| {
        let mut nearest: Option<kernel::RayPick> = None;
        for target in miss_scope(st, src, &targets) {
            let Ok(bounds) = target.body.exact_bounds() else { continue };
            let bounds = bounds.inflated(margin);
            if !bounds.meets(&segment) || !line_meets_box(cap, inward, &bounds) {
                continue;
            }
            if bounds.contains(cap) && matches!(target.body.classify_point(cap), Ok(kernel::PointClass::Inside | kernel::PointClass::OnBoundary)) {
                return cap;
            }
            if let Ok(Some(hit)) = target.body.fire_ray(cap - inward * margin, inward)
                && (margin..=span + margin).contains(&hit.depth)
                && hit.normal.dot(inward) < -1e-9
                && nearest.is_none_or(|previous| hit.depth < previous.depth)
            {
                nearest = Some(hit);
            }
        }
        nearest.map_or(cap, |hit| hit.point)
    })
}
