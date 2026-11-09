// SPDX-License-Identifier: Apache-2.0

use crate::history_records::{AsmHistoricalTopology, AsmHistoricalTransition};
use crate::records::feature::scope::DesignParameterScope;
use cadmpeg_ir::features::{
    BodySelection, BooleanKind, BooleanOp, EdgeSelection, FaceSelection, FeatureId,
    FeatureOperation, SolidSweepOperation, SplitBodyTool, SweepShape,
};
use cadmpeg_ir::math::{Point3, Vector3};
use std::collections::{BTreeMap, BTreeSet, HashMap};

use super::stable_ref;

pub(super) const EPS_HISTORY_BODY_BBOX_GROWTH_E3: f64 = 1.0e-3;
pub(super) const EPS_HISTORY_BODY_BBOX_UNCHANGED_E4: f64 = 1.0e-4;
pub(super) const EPS_HISTORY_BODY_BBOX_CONTAINMENT_E2: f64 = 1.0e-2;
const EPS_HISTORY_SPLIT_PLANE_SEPARATES_E4: f64 = 1.0e-4;
const EPS_HISTORY_SPLIT_PLANE_CROSS_SQ_E5: f64 = 1.0e-5;
const EPS_HISTORY_SPLIT_PLANE_DIST_E4: f64 = 1.0e-4;
pub(super) const EPS_HISTORY_VERTEX_ABSORBED_DIST_SQ_E4: f64 = 1.0e-4;

fn planes_are_equivalent(o1: Point3, n1: Vector3, o2: Point3, n2: Vector3) -> bool {
    let Some(u1) = n1.unit() else {
        return false;
    };
    let Some(u2) = n2.unit() else {
        return false;
    };
    let cross = u1.cross(u2);
    let cross_len_sq = cross.x * cross.x + cross.y * cross.y + cross.z * cross.z;
    if cross_len_sq > EPS_HISTORY_SPLIT_PLANE_CROSS_SQ_E5 {
        return false;
    }
    let dist = o1.vector_from(o2).dot(u2).abs();
    dist < EPS_HISTORY_SPLIT_PLANE_DIST_E4
}

pub(super) enum InsertedFaceClassification {
    Plane(Point3, Vector3),
    NonPlanar,
    Unclassifiable,
}

pub(super) fn classify_face_plane(
    topology: &AsmHistoricalTopology,
    face_id: i64,
) -> Option<(Point3, Vector3)> {
    let mut bindings = topology
        .face_surfaces
        .iter()
        .filter(|f| f.entity == face_id);
    let fs = bindings.next()?;
    if bindings.next().is_some() {
        return None;
    }
    let carrier = fs.carrier;
    let mut planes = topology
        .surface_planes
        .iter()
        .filter(|s| s.surface == carrier);
    let has_cylinder = topology
        .surface_cylinders
        .iter()
        .any(|s| s.surface == carrier && s.axis.unit().is_some() && s.radius > 0.0);
    let has_axis = topology
        .surface_axes
        .iter()
        .any(|s| s.surface == carrier && s.direction.unit().is_some());
    let has_radius = topology
        .surface_radii
        .iter()
        .any(|s| s.surface == carrier && s.radius > 0.0 && s.radius.is_finite());
    let sp = planes.next()?;
    if planes.next().is_some()
        || has_cylinder
        || has_axis
        || has_radius
        || sp.normal.unit().is_none()
    {
        return None;
    }
    Some((sp.origin, sp.normal))
}

pub(super) fn classify_inserted_face(
    topology: &AsmHistoricalTopology,
    transition: &AsmHistoricalTransition,
    face_id: i64,
) -> InsertedFaceClassification {
    if let Some((origin, normal)) = classify_face_plane(topology, face_id) {
        return InsertedFaceClassification::Plane(origin, normal);
    }
    let mut bindings = topology
        .face_surfaces
        .iter()
        .filter(|f| f.entity == face_id);
    let Some(fs) = bindings.next() else {
        return InsertedFaceClassification::Unclassifiable;
    };
    if bindings.next().is_some() {
        return InsertedFaceClassification::Unclassifiable;
    }
    let carrier = fs.carrier;
    let has_plane = topology.surface_planes.iter().any(|s| s.surface == carrier);
    let has_cylinder = topology
        .surface_cylinders
        .iter()
        .any(|s| s.surface == carrier && s.axis.unit().is_some() && s.radius > 0.0);
    let has_axis = topology
        .surface_axes
        .iter()
        .any(|s| s.surface == carrier && s.direction.unit().is_some());
    let has_radius = topology
        .surface_radii
        .iter()
        .any(|s| s.surface == carrier && s.radius > 0.0 && s.radius.is_finite());
    let is_non_planar = has_cylinder || has_axis || has_radius;
    if !has_plane && is_non_planar && !transition.topology.surfaces.inserted.contains(&carrier) {
        InsertedFaceClassification::NonPlanar
    } else {
        InsertedFaceClassification::Unclassifiable
    }
}

pub(super) struct StateSplitCandidates {
    pub(super) all_faces_classified: bool,
    pub(super) has_bodies: bool,
    pub(super) all_bodies_complete: bool,
    pub(super) inserted_planar_faces: Vec<(i64, Point3, Vector3)>,
    pub(super) body_faces: BTreeMap<i64, BTreeSet<i64>>,
}

pub(super) struct SplitResultPlaneContext<'a> {
    pub(super) candidates: &'a StateSplitCandidates,
    pub(super) centers: &'a [(i64, Point3)],
    pub(super) vertices: &'a BTreeMap<i64, Vec<Point3>>,
    pub(super) solid_outputs: Option<&'a [i64]>,
    pub(super) before_solid: Option<&'a BTreeSet<i64>>,
    pub(super) after_solid: &'a BTreeSet<i64>,
    pub(super) targets: &'a BodySelection,
}

fn collect_selection_slots<'a>(ids: impl Iterator<Item = &'a str>) -> Option<Vec<i64>> {
    let mut slots = Vec::new();
    for id in ids {
        let slot = stable_ref(id)?;
        if !slots.contains(&slot) {
            slots.push(slot);
        }
    }
    (!slots.is_empty()).then_some(slots)
}

fn resolved_face_selection_slots(selection: &FaceSelection) -> Option<Vec<i64>> {
    match selection {
        FaceSelection::Faces(faces) | FaceSelection::Resolved { faces, .. } => {
            collect_selection_slots(faces.iter().map(cadmpeg_ir::ids::FaceId::as_str))
        }
        FaceSelection::Historical { faces, .. } => collect_selection_slots(
            faces
                .as_slice()
                .iter()
                .map(cadmpeg_ir::ids::HistoricalFaceId::as_str),
        ),
        FaceSelection::HistoricalPartial { .. }
        | FaceSelection::Native(_)
        | FaceSelection::Unresolved
        | FaceSelection::Generated { .. } => None,
    }
}

fn resolved_body_selection_slots(selection: &BodySelection) -> Option<Vec<i64>> {
    match selection {
        BodySelection::Bodies(bodies) | BodySelection::Resolved { bodies, .. } => {
            collect_selection_slots(
                bodies
                    .as_slice()
                    .iter()
                    .map(cadmpeg_ir::ids::BodyId::as_str),
            )
        }
        BodySelection::Historical { bodies, .. } => collect_selection_slots(
            bodies
                .as_slice()
                .iter()
                .map(cadmpeg_ir::ids::HistoricalBodyId::as_str),
        ),
        BodySelection::ResolvedSet { members } => {
            collect_selection_slots(members.bodies().map(cadmpeg_ir::ids::BodyId::as_str))
        }
        BodySelection::HistoricalSet { members, .. } => collect_selection_slots(
            members
                .bodies()
                .map(cadmpeg_ir::ids::HistoricalBodyId::as_str),
        ),
        BodySelection::Native(_)
        | BodySelection::NativeSet(_)
        | BodySelection::Unresolved
        | BodySelection::Generated { .. }
        | BodySelection::Local { .. } => None,
    }
}

pub(super) type StateFacePlanes = HashMap<(usize, i64), BTreeMap<i64, (Point3, Vector3)>>;

pub(super) fn resolve_split_tool_plane(
    tool: &SplitBodyTool,
    datum_plane_frames: &HashMap<FeatureId, (Point3, Vector3)>,
    state_face_planes: &StateFacePlanes,
    history_index: Option<usize>,
    previous_state_id: i64,
    state_id: i64,
) -> Option<(Point3, Vector3)> {
    match tool {
        SplitBodyTool::Plane { plane } => datum_plane_frames.get(plane).copied(),
        SplitBodyTool::Faces(selection) => {
            let history_index = history_index?;
            let slots = resolved_face_selection_slots(selection)?;
            for candidate_state in [previous_state_id, state_id] {
                let Some(face_planes) = state_face_planes.get(&(history_index, candidate_state))
                else {
                    continue;
                };
                let mut planes = Vec::with_capacity(slots.len());
                if slots.iter().all(|slot| {
                    face_planes.get(slot).is_some_and(|&plane| {
                        planes.push(plane);
                        true
                    })
                }) {
                    if let Some(&(first_o, first_n)) = planes.first() {
                        if planes
                            .iter()
                            .all(|&(o, n)| planes_are_equivalent(first_o, first_n, o, n))
                        {
                            return Some((first_o, first_n));
                        }
                    }
                }
            }
            None
        }
    }
}

pub(super) fn resolve_split_result_plane(
    ctx: &SplitResultPlaneContext<'_>,
) -> Option<(Point3, Vector3)> {
    if !ctx.candidates.all_faces_classified || ctx.candidates.inserted_planar_faces.is_empty() {
        return None;
    }
    if (!ctx.candidates.has_bodies && ctx.after_solid.is_empty())
        || !ctx.candidates.all_bodies_complete
        || ctx.centers.len() < 2
    {
        let &(_, first_o, first_n) = ctx.candidates.inserted_planar_faces.first()?;
        return ctx
            .candidates
            .inserted_planar_faces
            .iter()
            .all(|&(_, o, n)| planes_are_equivalent(first_o, first_n, o, n))
            .then_some((first_o, first_n));
    }
    let target_slots = resolved_body_selection_slots(ctx.targets);
    let non_empty_before = ctx.before_solid.filter(|before| !before.is_empty());
    let new_bodies: BTreeSet<i64> = match non_empty_before {
        Some(before) => ctx.after_solid.difference(before).copied().collect(),
        None => ctx.after_solid.iter().copied().collect(),
    };
    if new_bodies.is_empty() {
        return None;
    }
    let mut valid_planes = Vec::new();
    for &(_, origin, normal) in &ctx.candidates.inserted_planar_faces {
        let unit = normal.unit()?;
        let body_owns_equivalent_inserted_face = |body: i64| -> bool {
            let Some(faces) = ctx.candidates.body_faces.get(&body) else {
                return false;
            };
            ctx.candidates
                .inserted_planar_faces
                .iter()
                .any(|&(fid, fo, fn_)| {
                    faces.contains(&fid) && planes_are_equivalent(origin, normal, fo, fn_)
                })
        };
        let mut matched_new_bodies = BTreeSet::new();
        let mut any_pair = false;
        for (i, &(b1, c1)) in ctx.centers.iter().enumerate() {
            for &(b2, c2) in &ctx.centers[i + 1..] {
                if let Some(solid_outputs) = ctx.solid_outputs {
                    if !solid_outputs.contains(&b1) || !solid_outputs.contains(&b2) {
                        continue;
                    }
                }
                if let Some(before) = non_empty_before {
                    let b1_was_before = before.contains(&b1);
                    let b2_was_before = before.contains(&b2);
                    let b1_is_new = new_bodies.contains(&b1);
                    let b2_is_new = new_bodies.contains(&b2);
                    if !((b1_was_before && b2_is_new) || (b2_was_before && b1_is_new)) {
                        continue;
                    }
                }
                if let Some(ref targets) = target_slots {
                    if !targets.contains(&b1) && !targets.contains(&b2) {
                        continue;
                    }
                }
                if !body_owns_equivalent_inserted_face(b1)
                    || !body_owns_equivalent_inserted_face(b2)
                {
                    continue;
                }
                let d1 = c1.vector_from(origin).dot(unit);
                let d2 = c2.vector_from(origin).dot(unit);
                if d1 * d2 >= -EPS_HISTORY_SPLIT_PLANE_SEPARATES_E4 {
                    continue;
                }
                let (Some(v1_list), Some(v2_list)) = (ctx.vertices.get(&b1), ctx.vertices.get(&b2))
                else {
                    continue;
                };
                let vertices_partitioned = if d1 < 0.0 {
                    v1_list
                        .iter()
                        .all(|v| v.vector_from(origin).dot(unit) <= EPS_HISTORY_SPLIT_PLANE_DIST_E4)
                        && v2_list.iter().all(|v| {
                            v.vector_from(origin).dot(unit) >= -EPS_HISTORY_SPLIT_PLANE_DIST_E4
                        })
                } else {
                    v1_list.iter().all(|v| {
                        v.vector_from(origin).dot(unit) >= -EPS_HISTORY_SPLIT_PLANE_DIST_E4
                    }) && v2_list
                        .iter()
                        .all(|v| v.vector_from(origin).dot(unit) <= EPS_HISTORY_SPLIT_PLANE_DIST_E4)
                };
                if vertices_partitioned {
                    any_pair = true;
                    if new_bodies.contains(&b1) {
                        matched_new_bodies.insert(b1);
                    }
                    if new_bodies.contains(&b2) {
                        matched_new_bodies.insert(b2);
                    }
                }
            }
        }
        if any_pair && matched_new_bodies.len() == new_bodies.len() {
            valid_planes.push((origin, normal));
        }
    }
    let &(first_o, first_n) = valid_planes.first()?;
    valid_planes
        .iter()
        .all(|&(o, n)| planes_are_equivalent(first_o, first_n, o, n))
        .then_some((first_o, first_n))
}

pub(super) fn feature_supports_body_absorption(op_ref: &FeatureOperation) -> bool {
    matches!(
        op_ref,
        FeatureOperation::Combine {
            op: BooleanKind::Join,
            keep_tools: false,
            ..
        } | FeatureOperation::Extrude {
            op: BooleanOp::Join,
            ..
        } | FeatureOperation::Revolve {
            op: BooleanOp::Join,
            ..
        } | FeatureOperation::Sweep {
            shape: SweepShape::Solid {
                op: SolidSweepOperation::Join,
                ..
            },
            ..
        } | FeatureOperation::Loft {
            op: BooleanOp::Join,
            ..
        }
    )
}

pub(super) fn feature_consumes_tool_body(
    op_ref: &FeatureOperation,
    scope: Option<&DesignParameterScope>,
    state_deleted_bodies: Option<&Vec<i64>>,
    host: i64,
    tool_body: i64,
) -> bool {
    if state_deleted_bodies.is_some_and(|deleted| deleted.contains(&tool_body)) {
        return true;
    }
    if matches!(
        op_ref,
        FeatureOperation::Extrude {
            op: BooleanOp::Join,
            ..
        } | FeatureOperation::Revolve {
            op: BooleanOp::Join,
            ..
        } | FeatureOperation::Sweep {
            shape: SweepShape::Solid {
                op: SolidSweepOperation::Join,
                ..
            },
            ..
        } | FeatureOperation::Loft {
            op: BooleanOp::Join,
            ..
        }
    ) {
        return true;
    }
    if let FeatureOperation::Combine {
        operands,
        op: BooleanKind::Join,
        keep_tools: false,
    } = op_ref
    {
        if resolved_body_selection_slots(operands.target()).as_deref() == Some(&[host])
            && resolved_body_selection_slots(operands.tools())
                .is_some_and(|tools| tools.contains(&tool_body))
        {
            return true;
        }
    }
    if let Some(operation) = scope.and_then(DesignParameterScope::combine_operation) {
        if operation.operation == BooleanKind::Join && !operation.keep_tools {
            let target_matches = operation
                .target_body_node
                .is_some_and(|node| i64::from(node.body_entity_suffix) == host);
            let tool_matches = operation.tools.iter().any(|tool| {
                tool.body_node
                    .is_some_and(|node| i64::from(node.body_entity_suffix) == tool_body)
            });
            if target_matches && tool_matches {
                return true;
            }
        }
    }
    false
}

pub(super) struct EdgeMidpointRequest {
    pub(super) feature_index: usize,
    pub(super) slots: Vec<i64>,
}

pub(super) fn collect_edge_midpoint_slots(
    feature_id: &FeatureId,
    previous_state_id: i64,
    selections: &[&EdgeSelection],
) -> (Vec<i64>, bool) {
    let mut slots = Vec::new();
    let mut has_unresolved = false;
    for sel in selections {
        let mut extract_slots = |edge_ids: &[cadmpeg_ir::ids::HistoricalEdgeId]| {
            if edge_ids.is_empty() {
                has_unresolved = true;
            }
            for edge_id in edge_ids {
                if let Some(slot) =
                    crate::ids::feature_input_edge_slot(feature_id, previous_state_id, edge_id)
                {
                    if !slots.contains(&slot) {
                        slots.push(slot);
                    }
                } else {
                    has_unresolved = true;
                }
            }
        };
        match sel {
            EdgeSelection::Historical { edges, .. } => {
                extract_slots(edges.as_slice());
            }
            EdgeSelection::HistoricalPartial {
                edges, unresolved, ..
            } => {
                extract_slots(edges.as_slice());
                if !unresolved.is_empty() {
                    has_unresolved = true;
                }
            }
            EdgeSelection::Resolved { edges, .. } | EdgeSelection::Edges(edges) => {
                if edges.is_empty() {
                    has_unresolved = true;
                }
                for edge_id in edges {
                    if let Some(slot) = stable_ref(edge_id.as_str()) {
                        if !slots.contains(&slot) {
                            slots.push(slot);
                        }
                    } else {
                        has_unresolved = true;
                    }
                }
            }
            EdgeSelection::Native(_)
            | EdgeSelection::Unresolved
            | EdgeSelection::All
            | EdgeSelection::Generated { .. } => {
                has_unresolved = true;
            }
        }
    }
    (slots, has_unresolved)
}
