//! Planar boolean classification, copying and cleanup after intersection insertion.
//!
//! Upstream: `source/SMLib/src/SmPrimitiveCreation.cpp` (`Boolean2D`).
//!
//! | Rust | Upstream |
//! |------|----------|
//! | `finish` (private) | `SmPrimitiveCreation::Boolean2D`, after `IntersectInsertRelate` |
//!
//! Deviations: marks are caller-owned records, separately for each arena. Both
//! operands use the passed Kernel. On success the result owns the primary arena;
//! the two consumed input slots become empty arenas. No recursion.

use super::primitives::{detailed_face, stitch_faces, Boolean2d};
use crate::boolean::topology_intersector::MateMap;
use crate::error::{ignore_status, Result, SolidError};
use crate::geom::context::MarkLock;
use crate::kernel::Kernel;
use crate::nurbs::b_spline_surface_n_lib::p2::OrientType;
use crate::topology::edge;
use crate::topology::edgeuse::p1 as eu;
use crate::topology::topology_traverser::{Entity, Marks};
use crate::topology::{Brep, EdgeId, FaceId, VertexId};

#[derive(Default)]
struct PlanarIntersections {
    edges: Vec<(EdgeId, EdgeId)>,
    vertices: Vec<(VertexId, VertexId)>,
    faces: MateMap<FaceId>,
}
fn intersect_insert_relate(
    _k: &mut Kernel,
    _a: &mut Brep,
    _b: &mut Brep,
    _approx_tolerance: f64,
    _angle_radians: f64,
) -> Result<PlanarIntersections> {
    // UNPORTED(boolean.topology_intersector_p2): SmTopologyIntersector::IntersectInsertRelate
    // merge::p4::arena_pair::ArenaPair adapts two Brep arenas, but its handles
    // and notification records use an operand-qualified namespace. This entry
    // has only the primary NotifyEnv's key_of resolver, not a mapping for both
    // operands. Reusing it requires retaining its intersector/relations through
    // DeleteFaces and translating common pairs in native order; bare FaceIds
    // in PlanarIntersections cannot stand in for that identity/session state.
    Err(SolidError::unported(
        "SmTopologyIntersector::IntersectInsertRelate",
        "boolean.topology_intersector_p2",
    ))
}
fn delete_faces(
    _k: &Kernel,
    _a: &mut Brep,
    _intersections: &mut PlanarIntersections,
    _faces: &[FaceId],
) -> Result<()> {
    // UNPORTED(boolean.topology_intersector_p2): SmTopologyIntersector::DeleteFaces
    // ArenaPair's DeleteFaces must reuse the intersector/relations from the
    // intersection stage and the same operand-qualified notification mapping.
    // PlanarIntersections does not yet retain that session; a fresh adapter or
    // a primary-only key_of resolver would lose the corresponding mate state.
    Err(SolidError::unported(
        "SmTopologyIntersector::DeleteFaces",
        "boolean.topology_intersector_p2",
    ))
}
fn remove_topology(
    k: &mut Kernel,
    a: &mut Brep,
    notifications: &mut edge::NotifyEnv<'_>,
) -> Result<()> {
    crate::topology::brep::p5::remove_topological_with_notifications(
        k,
        a,
        None,
        None,
        Some(notifications),
    )
}

pub(super) fn finish(
    k: &mut Kernel,
    a: &mut Brep,
    b: &mut Brep,
    first: FaceId,
    operation: Boolean2d,
    result: &mut Option<Brep>,
    marks: (&mut Marks, &mut Marks),
    notifications: &mut edge::NotifyEnv<'_>,
) -> Result<()> {
    let tolerance = crate::math::max2(a.tolerance(), b.tolerance());
    // Preserve the constructor's explicit tolerances when wiring the pending
    // edit adapter. Its output retains native common-edge/vertex visit order.
    let mut intersections = intersect_insert_relate(
        k,
        a,
        b,
        tolerance / 2.0,
        20.0 * core::f64::consts::PI / 180.0,
    )?;
    let mut lock = MarkLock::new_pair(core::slice::from_mut(&mut k.context), 0, 0)?;
    let mark_a = lock.mark_type1();
    let mark_b = lock.mark_type2();
    let (marks_a, marks_b) = marks;
    let outcome = (|| {
        for (primary, other) in &intersections.edges {
            marks_a.mark(k, Entity::Edge(*primary), mark_a);
            marks_b.mark(k, Entity::Edge(*other), mark_b);
        }
        for (primary, other) in &intersections.vertices {
            marks_a.mark(k, Entity::Vertex(*primary), mark_a);
            marks_b.mark(k, Entity::Vertex(*other), mark_b);
        }
        // The primary side propagates a failed radial-sector status; the other
        // side skips it. Mark each face BEFORE the radial query, as upstream.
        for (primary, other) in intersections.edges.clone() {
            for edgeuse in edge::edgeuses(a, primary, None, None)?
                .into_iter()
                .step_by(2)
            {
                let face = eu::face(a, edgeuse)?
                    .ok_or_else(|| SolidError::null_pointer("primitive::boolean_2d"))?;
                if marks_a.is_marked(k, Entity::Face(face), mark_a) {
                    continue;
                }
                marks_a.mark(k, Entity::Face(face), mark_a);
                let sector = edge::find_radial_sector_between(
                    k,
                    b,
                    other,
                    Some(a),
                    edgeuse,
                    None,
                    OrientType::Same,
                    None,
                )?;
                if let Some(faceuse) = sector.coincident_faceuse {
                    let mate = eu::faceuse_face(b, faceuse)?
                        .ok_or_else(|| SolidError::null_pointer("primitive::boolean_2d"))?;
                    marks_b.mark(k, Entity::Face(mate), mark_b);
                    ignore_status(intersections.faces.record(face, mate))?;
                }
            }
            for edgeuse in edge::edgeuses(b, other, None, None)?.into_iter().step_by(2) {
                let face = eu::face(b, edgeuse)?
                    .ok_or_else(|| SolidError::null_pointer("primitive::boolean_2d"))?;
                if marks_b.is_marked(k, Entity::Face(face), mark_b) {
                    continue;
                }
                marks_b.mark(k, Entity::Face(face), mark_b);
                let sector = match edge::find_radial_sector_between(
                    k,
                    a,
                    primary,
                    Some(b),
                    edgeuse,
                    None,
                    OrientType::Same,
                    None,
                ) {
                    Ok(value) => value,
                    Err(e) if e.is_unported() => return Err(e),
                    Err(_) => continue,
                };
                if let Some(faceuse) = sector.coincident_faceuse {
                    let mate = eu::faceuse_face(a, faceuse)?
                        .ok_or_else(|| SolidError::null_pointer("primitive::boolean_2d"))?;
                    marks_a.mark(k, Entity::Face(mate), mark_a);
                    intersections.faces.record(mate, face)?;
                }
            }
        }
        let mut delete = Vec::new();
        for face in crate::topology::brep::p1::faces(a, None)? {
            let common = intersections.faces.other(face).is_some();
            if matches!(operation, Boolean2d::ExclusiveOr | Boolean2d::Difference) && common
                || operation == Boolean2d::Intersection && !common
            {
                delete.push(face);
            }
        }
        // Keep the original A faces until every B classification/copy is done.
        let mut covering = first;
        for face in crate::topology::brep::p1::faces(b, None)? {
            if matches!(
                operation,
                Boolean2d::ExclusiveOr | Boolean2d::Union | Boolean2d::Merge
            ) && intersections.faces.primary(face).is_none()
            {
                let boundary = crate::topology::face::p2::create_curves_from_face(
                    k,
                    b,
                    face,
                    OrientType::Same,
                    true,
                    false,
                )?;
                let original = a
                    .face(covering)?
                    .surface
                    .as_ref()
                    .ok_or_else(|| SolidError::null_pointer("primitive::boolean_2d"))?;
                let surface = if original.owner().is_some() {
                    original.copy(k)?
                } else {
                    original.clone_for_evaluation()?
                };
                let domain = surface.natural_uv_domain();
                covering = detailed_face(
                    k,
                    a,
                    a.infinite_region(),
                    boundary,
                    surface,
                    domain,
                    notifications,
                )?;
            }
        }
        ignore_status(delete_faces(k, a, &mut intersections, &delete))?;
        if operation != Boolean2d::Merge {
            stitch_faces(k, a, a.tolerance())?;
            remove_topology(k, a, notifications)?;
        }
        *result = Some(core::mem::take(a));
        *b = Brep::new();
        Ok(())
    })();
    lock.release(core::slice::from_mut(&mut k.context))?;
    outcome
}
