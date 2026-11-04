// SPDX-License-Identifier: Apache-2.0
//! Edit validators that diff the target against the baseline and build edit sets.

use cadmpeg_core::decode::index_from_u32;

use cadmpeg_asm::brep::records::{EndpointSlot, EvaluatedToleranceSlot};
use std::collections::{BTreeMap, BTreeSet, HashMap};

use crate::history_records::{AsmBulletinBoard, AsmDeltaState, AsmEntityChange};
use crate::records::{
    act::{ActComponentLink, ActEntity, ActGuid, ActRegistryChannel},
    references::{DesignMaterialAssignment, LostEdgeReference},
    sketch_geometry::SketchCurveGeometry,
};
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::{CadIr, Model};
use cadmpeg_ir::geometry::{
    nurbs::{NurbsCurve, NurbsSurface},
    pcurve::{PcurveGeometry, PcurveNurbs},
    BlendRadiusLaw, Curve, ProceduralCurve, ProceduralSurfaceDefinition, SolvedCurveGeometry,
    SolvedSurfaceGeometry, Surface,
};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::topology::{Body, Coedge, Color, Edge, Face, Sense};
use cadmpeg_ir::transform::Transform;

use super::{
    geometry::{unchanged_unique_knot_count, writable_nurbs_degree},
    records::native_stream,
};
use crate::native::F3dNative;
use crate::writer::generate::native_geometry::{
    native_support_pcurve, pcurve_support_geometry, pcurve_support_surface,
};
use crate::writer::primitives::normalized_face_sense_to_native;
use cadmpeg_asm::nurbs::reader::LEN_TO_MM;

/// The base-type GUID text and its location, when the entry stores the field.
fn located_base_guid(guid: &crate::records::entity_header::BaseTypeGuid) -> Option<(&str, u64)> {
    match guid {
        crate::records::entity_header::BaseTypeGuid::Absent => None,
        crate::records::entity_header::BaseTypeGuid::EmptyRoot { offset } => Some(("", *offset)),
        crate::records::entity_header::BaseTypeGuid::Guid { value, offset } => {
            Some((value.as_str(), *offset))
        }
    }
}

/// The before-value carried at the after-record's location, for comparing an
/// edited record against the record it replaces.
fn normalized_token<T: Clone>(
    before: Option<&crate::records::identity::RecordedValue<T>>,
    after: Option<&crate::records::identity::RecordedValue<T>>,
) -> Option<crate::records::identity::RecordedValue<T>> {
    match (before, after) {
        (Some(before), Some(after)) => Some(crate::records::identity::RecordedValue {
            value: before.value.clone(),
            offset: after.offset,
        }),
        _ => after.cloned(),
    }
}

#[derive(Clone, Copy)]
pub(in crate::writer) struct PatchNatives<'a> {
    pub(in crate::writer) baseline: Option<&'a F3dNative>,
    pub(in crate::writer) target: Option<&'a F3dNative>,
}

pub(super) struct Edit<T> {
    pub(super) offset: u64,
    pub(super) value: T,
}

pub(super) struct ByteEdit {
    pub(super) offset: u64,
    pub(super) value: Vec<u8>,
}

pub(super) struct SketchPointEdit {
    pub(super) offset: u64,
    pub(super) coordinate_offset: u32,
    pub(super) coordinates: cadmpeg_ir::math::Point2,
}

pub(super) struct PersistentReferenceEdit {
    pub(super) offset: u64,
    pub(super) identity_offset: u32,
    pub(super) identity: u64,
}

pub(super) struct BodyMemberEdit {
    pub(super) offset: u64,
    pub(super) entity_suffix: u64,
    pub(super) flags: u16,
}

pub(crate) struct SketchCurveEdit {
    pub(crate) offset: u64,
    pub(crate) geometry_offset: u32,
    pub(crate) geometry: SketchCurveGeometry,
}

pub(super) fn validate_creation_timestamp_edits(
    native: PatchNatives<'_>,
) -> Result<BTreeMap<usize, f64>, CodecError> {
    let baseline = native
        .baseline
        .map_or(&[][..], |native| native.creation_timestamps.as_slice());
    let target = native
        .target
        .map_or(&[][..], |native| native.creation_timestamps.as_slice());
    let by_id = baseline
        .iter()
        .map(|timestamp| (timestamp.id.as_str(), timestamp))
        .collect::<BTreeMap<_, _>>();
    if by_id
        .keys()
        .copied()
        .ne(target.iter().map(|timestamp| timestamp.id.as_str()))
    {
        return Err(CodecError::NotImplemented(
            "F3D timestamp regeneration requires the unchanged timestamp-id set".into(),
        ));
    }
    let mut edits = BTreeMap::new();
    for (before, timestamp) in by_id.into_values().zip(target) {
        let mut normalized = timestamp.clone();
        normalized.unix_microseconds = before.unix_microseconds;
        if &normalized != before {
            return Err(CodecError::NotImplemented(format!(
                "F3D timestamp edit changes structural fields: {}",
                timestamp.id
            )));
        }
        if timestamp.unix_microseconds == before.unix_microseconds {
            continue;
        }
        let record_index = usize::try_from(timestamp.record_index).map_err(|_| {
            CodecError::malformed(format_args!(
                "F3D timestamp record index exceeds usize: {}",
                timestamp.id
            ))
        })?;
        edits.insert(record_index, timestamp.unix_microseconds.get());
    }
    Ok(edits)
}

pub(super) fn validate_edge_continuity_edits(
    native: PatchNatives<'_>,
) -> Result<BTreeMap<usize, (Sense, String)>, CodecError> {
    let baseline = native
        .baseline
        .map_or(&[][..], |native| native.edge_continuities.as_slice());
    let target = native
        .target
        .map_or(&[][..], |native| native.edge_continuities.as_slice());
    let baseline_by_id = baseline
        .iter()
        .map(|metadata| (metadata.id(), metadata))
        .collect::<BTreeMap<_, _>>();
    let target_by_id = target
        .iter()
        .map(|metadata| (metadata.id(), metadata))
        .collect::<BTreeMap<_, _>>();
    if baseline_by_id.keys().ne(target_by_id.keys()) {
        return Err(CodecError::NotImplemented(
            "F3D edge-continuity regeneration requires the unchanged metadata-id set".into(),
        ));
    }
    let mut edits = BTreeMap::new();
    for (id, before) in baseline_by_id {
        let after = target_by_id[&id];
        let mut normalized = after.clone();
        normalized.sense = before.sense;
        normalized.continuity.clone_from(&before.continuity);
        if &normalized != before {
            return Err(CodecError::NotImplemented(format!(
                "F3D edge-continuity edit changes structural fields: {id}"
            )));
        }
        if after.continuity == before.continuity && after.sense == before.sense {
            continue;
        }
        if !matches!(after.continuity.as_str(), "tangent" | "unknown") {
            return Err(CodecError::malformed(format_args!(
                "F3D edge continuity {id} has unsupported token {}",
                after.continuity
            )));
        }
        edits.insert(
            index_from_u32(after.record_index),
            (after.sense, after.continuity.clone()),
        );
    }
    Ok(edits)
}

pub(super) fn validate_edge_ownership_edits(
    native: PatchNatives<'_>,
    target: &CadIr,
) -> Result<BTreeMap<usize, i64>, CodecError> {
    let baseline = native
        .baseline
        .map_or(&[][..], |native| native.edge_ownerships.as_slice());
    let target_ownerships = native
        .target
        .map_or(&[][..], |native| native.edge_ownerships.as_slice());
    let baseline_by_id = baseline
        .iter()
        .map(|ownership| (ownership.id(), ownership))
        .collect::<BTreeMap<_, _>>();
    let target_by_id = target_ownerships
        .iter()
        .map(|ownership| (ownership.id(), ownership))
        .collect::<BTreeMap<_, _>>();
    if baseline_by_id.keys().ne(target_by_id.keys()) {
        return Err(CodecError::NotImplemented(
            "F3D edge-ownership regeneration requires the unchanged metadata-id set".into(),
        ));
    }
    let mut edits = BTreeMap::new();
    for (id, before) in baseline_by_id {
        let after = target_by_id[&id];
        let mut normalized = after.clone();
        normalized.owner_coedge.clone_from(&before.owner_coedge);
        if &normalized != before {
            return Err(CodecError::NotImplemented(format!(
                "F3D edge-ownership edit changes structural fields: {id}"
            )));
        }
        if after.owner_coedge == before.owner_coedge {
            continue;
        }
        let owner = if let Some(owner) = &after.owner_coedge {
            let coedge = target
                .model
                .coedges
                .iter()
                .find(|coedge| coedge.id == *owner)
                .ok_or_else(|| {
                    CodecError::malformed(format_args!(
                        "F3D edge ownership {id} references missing coedge {owner}"
                    ))
                })?;
            if coedge.edge != after.edge {
                return Err(CodecError::malformed(format_args!(
                    "F3D edge ownership {id} selects a coedge of another edge"
                )));
            }
            owner
                .as_str()
                .rsplit_once('#')
                .and_then(|(_, index)| index.parse::<i64>().ok())
                .ok_or_else(|| {
                    CodecError::malformed(format_args!(
                        "F3D owning coedge {owner} has no native record index"
                    ))
                })?
        } else {
            -1
        };
        edits.insert(index_from_u32(after.record_index), owner);
    }
    Ok(edits)
}

pub(super) fn validate_vertex_ownership_edits(
    native: PatchNatives<'_>,
    target: &CadIr,
) -> Result<BTreeMap<usize, (i64, EndpointSlot)>, CodecError> {
    let baseline = native
        .baseline
        .map_or(&[][..], |native| native.vertex_ownerships.as_slice());
    let target_native = native
        .target
        .map_or(&[][..], |native| native.vertex_ownerships.as_slice());
    let baseline_by_id = baseline
        .iter()
        .map(|metadata| (metadata.id(), metadata))
        .collect::<BTreeMap<_, _>>();
    let target_by_id = target_native
        .iter()
        .map(|metadata| (metadata.id(), metadata))
        .collect::<BTreeMap<_, _>>();
    if baseline_by_id.keys().ne(target_by_id.keys()) {
        return Err(CodecError::NotImplemented(
            "F3D vertex-ownership regeneration requires the unchanged metadata-id set".into(),
        ));
    }
    let mut edits = BTreeMap::new();
    for (id, before) in baseline_by_id {
        let after = target_by_id[&id];
        let mut normalized = after.clone();
        normalized.owning_edge.clone_from(&before.owning_edge);
        normalized.endpoint_index = before.endpoint_index;
        if &normalized != before {
            return Err(CodecError::NotImplemented(format!(
                "F3D vertex-ownership edit changes structural fields: {id}"
            )));
        }
        if after.owning_edge == before.owning_edge && after.endpoint_index == before.endpoint_index
        {
            continue;
        }
        let edge = target
            .model
            .edges
            .iter()
            .find(|edge| edge.id == after.owning_edge)
            .ok_or_else(|| {
                CodecError::malformed(format_args!(
                    "F3D vertex ownership {id} references missing edge {}",
                    after.owning_edge
                ))
            })?;
        let valid = match after.endpoint_index {
            EndpointSlot::Start => edge.start == after.vertex,
            EndpointSlot::End => edge.end == after.vertex,
        };
        if !valid {
            return Err(CodecError::malformed(format_args!(
                "F3D vertex ownership {id} has an inconsistent endpoint slot"
            )));
        }
        let edge_record = after
            .owning_edge
            .as_str()
            .rsplit_once('#')
            .and_then(|(_, index)| index.parse::<i64>().ok())
            .ok_or_else(|| {
                CodecError::malformed(format_args!(
                    "F3D owning edge {} has no native record index",
                    after.owning_edge
                ))
            })?;
        edits.insert(
            index_from_u32(after.record_index),
            (edge_record, after.endpoint_index),
        );
    }
    Ok(edits)
}

pub(super) fn validate_face_sidedness_edits(
    native: PatchNatives<'_>,
) -> Result<BTreeMap<usize, cadmpeg_asm::brep::records::FaceContainment>, CodecError> {
    let baseline = native
        .baseline
        .map_or(&[][..], |native| native.face_sidedness.as_slice());
    let target = native
        .target
        .map_or(&[][..], |native| native.face_sidedness.as_slice());
    let baseline_by_id = baseline
        .iter()
        .map(|metadata| (metadata.id(), metadata))
        .collect::<BTreeMap<_, _>>();
    let target_by_id = target
        .iter()
        .map(|metadata| (metadata.id(), metadata))
        .collect::<BTreeMap<_, _>>();
    if baseline_by_id.keys().ne(target_by_id.keys()) {
        return Err(CodecError::NotImplemented(
            "F3D face-sidedness regeneration requires the unchanged metadata-id set".into(),
        ));
    }
    let mut edits = BTreeMap::new();
    for (id, before) in baseline_by_id {
        let after = target_by_id[&id];
        let mut normalized = after.clone();
        normalized.containment = before.containment;
        if &normalized != before {
            return Err(CodecError::NotImplemented(format!(
                "F3D face-sidedness edit changes structural fields: {id}"
            )));
        }
        if after.containment == before.containment {
            continue;
        }
        match (before.containment, after.containment) {
            (Some(_), Some(containment)) => {
                edits.insert(index_from_u32(after.record_index), containment);
            }
            _ => {
                return Err(CodecError::NotImplemented(format!(
                    "F3D face sidedness {id} cannot change record width"
                )));
            }
        }
    }
    Ok(edits)
}

pub(super) fn validate_tolerant_vertex_edits(
    native: PatchNatives<'_>,
    baseline: &CadIr,
    target: &CadIr,
) -> Result<BTreeMap<usize, (f64, [f64; 2])>, CodecError> {
    let baseline_vertices = baseline
        .model
        .vertices
        .iter()
        .map(|vertex| (vertex.id.as_str(), vertex))
        .collect::<BTreeMap<_, _>>();
    let target_vertices = target
        .model
        .vertices
        .iter()
        .map(|vertex| (vertex.id.as_str(), vertex))
        .collect::<BTreeMap<_, _>>();
    if baseline_vertices.keys().ne(target_vertices.keys()) {
        return Err(CodecError::NotImplemented(
            "F3D tolerant-vertex regeneration requires the unchanged vertex-id set".into(),
        ));
    }
    let baseline_tails = native
        .baseline
        .map_or(&[][..], |native| native.tolerant_vertex_tails.as_slice());
    let target_tails = native
        .target
        .map_or(&[][..], |native| native.tolerant_vertex_tails.as_slice());
    let tolerant_vertices: std::collections::BTreeSet<&str> = baseline_tails
        .iter()
        .chain(target_tails)
        .map(|tail| tail.vertex.as_str())
        .collect();
    for (id, before) in &baseline_vertices {
        let after = target_vertices[id];
        let mut normalized = after.clone();
        normalized.tolerance = before.tolerance;
        if &normalized != *before {
            return Err(CodecError::NotImplemented(format!(
                "F3D vertex edit changes fields other than tolerance: {id}"
            )));
        }
        // A record changes width when its tolerant identity changes: a
        // tolerance appearing on or leaving a plain vertex. A tolerant
        // vertex whose tail marks the evaluated slot unset carries no
        // neutral tolerance and keeps its width.
        if after.tolerance != before.tolerance
            && (before.tolerance.is_none() || after.tolerance.is_none())
            && !tolerant_vertices.contains(id)
        {
            return Err(CodecError::NotImplemented(format!(
                "F3D vertex tolerance {id} cannot change record width"
            )));
        }
    }
    let baseline_by_id = baseline_tails
        .iter()
        .map(|tail| (tail.id(), tail))
        .collect::<BTreeMap<_, _>>();
    let target_by_id = target_tails
        .iter()
        .map(|tail| (tail.id(), tail))
        .collect::<BTreeMap<_, _>>();
    if baseline_by_id.keys().ne(target_by_id.keys()) {
        return Err(CodecError::NotImplemented(
            "F3D tolerant-vertex regeneration requires the unchanged tail-id set".into(),
        ));
    }
    let mut edits = BTreeMap::new();
    for (id, before) in baseline_by_id {
        let after = target_by_id[&id];
        let mut normalized = after.clone();
        normalized.leading_tolerances = before.leading_tolerances;
        if &normalized != before {
            return Err(CodecError::NotImplemented(format!(
                "F3D tolerant-vertex tail edit changes structural fields: {id}"
            )));
        }
        if matches!(before.evaluated_slot, EvaluatedToleranceSlot::Absent {})
            != matches!(after.evaluated_slot, EvaluatedToleranceSlot::Absent {})
        {
            return Err(CodecError::NotImplemented(format!(
                "F3D tolerant-vertex tail edit changes record width: {id}"
            )));
        }
        let target_vertex = target_vertices.get(after.vertex.as_str()).ok_or_else(|| {
            CodecError::malformed(format_args!(
                "tolerant vertex {id} tail has no target vertex"
            ))
        })?;
        let tolerance = match (target_vertex.tolerance, after.evaluated_slot) {
            (Some(tolerance), EvaluatedToleranceSlot::Evaluated { .. }) => tolerance.get(),
            (None, EvaluatedToleranceSlot::Unset { .. }) => -1.0,
            // The record ends before the slot; there is nothing to patch.
            (None, EvaluatedToleranceSlot::Absent {}) => continue,
            (Some(_), EvaluatedToleranceSlot::Absent {} | EvaluatedToleranceSlot::Unset { .. })
            | (None, EvaluatedToleranceSlot::Evaluated { .. }) => {
                return Err(CodecError::malformed(format_args!(
                    "tolerant vertex {id} tail disagrees with its vertex tolerance"
                )))
            }
        };
        let baseline_vertex = baseline_vertices
            .get(after.vertex.as_str())
            .ok_or_else(|| {
                CodecError::malformed(format_args!(
                    "tolerant vertex {id} tail has no baseline vertex"
                ))
            })?;
        if tolerance
            != baseline_vertex.tolerance.map_or(
                if matches!(before.evaluated_slot, EvaluatedToleranceSlot::Unset { .. }) {
                    -1.0
                } else {
                    tolerance
                },
                cadmpeg_ir::scalar::PositiveReal::get,
            )
            || after.leading_tolerances != before.leading_tolerances
        {
            // A negative tolerance is the unevaluated sentinel, stored
            // verbatim; a non-negative tolerance converts back from
            // millimetres to centimetres.
            let stored = if tolerance < 0.0 {
                tolerance
            } else {
                tolerance / LEN_TO_MM
            };
            edits.insert(
                index_from_u32(after.record_index),
                (
                    stored,
                    after
                        .leading_tolerances
                        .map(cadmpeg_ir::scalar::FiniteReal::get),
                ),
            );
        }
    }
    Ok(edits)
}

pub(super) fn validate_tolerant_edge_edits(
    native: PatchNatives<'_>,
    baseline: &CadIr,
    target: &CadIr,
) -> Result<BTreeMap<usize, f64>, CodecError> {
    let baseline_edges = baseline
        .model
        .edges
        .iter()
        .map(|edge| (edge.id.as_str(), edge))
        .collect::<BTreeMap<_, _>>();
    let target_edges = target
        .model
        .edges
        .iter()
        .map(|edge| (edge.id.as_str(), edge))
        .collect::<BTreeMap<_, _>>();
    if baseline_edges.keys().ne(target_edges.keys()) {
        return Err(CodecError::NotImplemented(
            "F3D tolerant-edge regeneration requires the unchanged edge-id set".into(),
        ));
    }
    for (id, before) in &baseline_edges {
        let after = target_edges[id];
        if after.tolerance != before.tolerance
            && (before.tolerance.is_none() || after.tolerance.is_none())
        {
            return Err(CodecError::NotImplemented(format!(
                "F3D edge tolerance {id} cannot change record width"
            )));
        }
    }
    let baseline_tails = native
        .baseline
        .map_or(&[][..], |native| native.tolerant_edge_tails.as_slice());
    let target_tails = native
        .target
        .map_or(&[][..], |native| native.tolerant_edge_tails.as_slice());
    let baseline_by_id = baseline_tails
        .iter()
        .map(|tail| (tail.id(), tail))
        .collect::<BTreeMap<_, _>>();
    let target_by_id = target_tails
        .iter()
        .map(|tail| (tail.id(), tail))
        .collect::<BTreeMap<_, _>>();
    if baseline_by_id.keys().ne(target_by_id.keys()) {
        return Err(CodecError::NotImplemented(
            "F3D tolerant-edge regeneration requires the unchanged tail-id set".into(),
        ));
    }
    let mut edits = BTreeMap::new();
    for (id, before) in baseline_by_id {
        let after = target_by_id[&id];
        if after != before {
            return Err(CodecError::NotImplemented(format!(
                "F3D tolerant-edge tail edit changes retained fields: {id}"
            )));
        }
        let tolerance = target_edges[after.edge.as_str()].tolerance.ok_or_else(|| {
            CodecError::malformed(format_args!("tolerant edge {id} has no tolerance"))
        })?;
        if baseline_edges[after.edge.as_str()].tolerance != Some(tolerance) {
            edits.insert(
                index_from_u32(after.record_index),
                tolerance.get() / LEN_TO_MM,
            );
        }
    }
    Ok(edits)
}

pub(super) fn validate_tolerant_coedge_edits(
    native: PatchNatives<'_>,
    baseline: &CadIr,
    target: &CadIr,
) -> Result<BTreeMap<usize, [f64; 2]>, CodecError> {
    let baseline_coedges = baseline
        .model
        .coedges
        .iter()
        .map(|coedge| (coedge.id.as_str(), coedge))
        .collect::<BTreeMap<_, _>>();
    let target_coedges = target
        .model
        .coedges
        .iter()
        .map(|coedge| (coedge.id.as_str(), coedge))
        .collect::<BTreeMap<_, _>>();
    if baseline_coedges.keys().ne(target_coedges.keys()) {
        return Err(CodecError::NotImplemented(
            "F3D tolerant-coedge regeneration requires the unchanged coedge-id set".into(),
        ));
    }
    for (id, before) in &baseline_coedges {
        let after = target_coedges[id];
        if after.use_curve != before.use_curve {
            return Err(CodecError::NotImplemented(format!(
                "F3D coedge use-curve edit changes embedded record structure: {id}"
            )));
        }
    }
    let baseline_parameters = native.baseline.map_or(&[][..], |native| {
        native.tolerant_coedge_parameters.as_slice()
    });
    let target_parameters = native.target.map_or(&[][..], |native| {
        native.tolerant_coedge_parameters.as_slice()
    });
    let baseline_by_id = baseline_parameters
        .iter()
        .map(|parameters| (parameters.id(), parameters))
        .collect::<BTreeMap<_, _>>();
    let target_by_id = target_parameters
        .iter()
        .map(|parameters| (parameters.id(), parameters))
        .collect::<BTreeMap<_, _>>();
    if baseline_by_id.keys().ne(target_by_id.keys()) {
        return Err(CodecError::NotImplemented(
            "F3D tolerant-coedge regeneration requires the unchanged metadata-id set".into(),
        ));
    }
    let mut edits = BTreeMap::new();
    for (id, before) in baseline_by_id {
        let after = target_by_id[&id];
        let mut normalized = after.clone();
        normalized.parameter_range = before.parameter_range;
        if &normalized != before {
            return Err(CodecError::NotImplemented(format!(
                "F3D tolerant-coedge edit changes structural fields: {id}"
            )));
        }
        if after.parameter_range != before.parameter_range {
            edits.insert(
                index_from_u32(after.record_index),
                after.parameter_range.get(),
            );
        }
    }
    Ok(edits)
}

pub(super) fn validate_wire_topology_edits(
    native: PatchNatives<'_>,
) -> Result<BTreeMap<usize, cadmpeg_asm::brep::records::WireSide>, CodecError> {
    let baseline_wires = native
        .baseline
        .map_or(&[][..], |native| native.wire_topologies.as_slice());
    let target_wires = native
        .target
        .map_or(&[][..], |native| native.wire_topologies.as_slice());
    let baseline_by_id = baseline_wires
        .iter()
        .map(|wire| (wire.id(), wire))
        .collect::<BTreeMap<_, _>>();
    let target_by_id = target_wires
        .iter()
        .map(|wire| (wire.id(), wire))
        .collect::<BTreeMap<_, _>>();
    if baseline_by_id.keys().ne(target_by_id.keys()) {
        return Err(CodecError::NotImplemented(
            "F3D wire regeneration requires the unchanged metadata-id set".into(),
        ));
    }
    let mut edits = BTreeMap::new();
    for (id, before) in baseline_by_id {
        let after = target_by_id[&id];
        let mut normalized = after.clone();
        normalized.side = before.side;
        if &normalized != before {
            return Err(CodecError::NotImplemented(format!(
                "F3D wire edit changes structural fields: {id}"
            )));
        }
        if after.side != before.side {
            edits.insert(index_from_u32(after.record_index), after.side);
        }
    }
    Ok(edits)
}

pub(super) enum ProceduralSurfaceEdit {
    Extrusion {
        parameter_interval: [f64; 2],
        direction: Vector3,
        native_position: Point3,
    },
    BlendRadii([f64; 2]),
}

pub(super) struct NurbsSurfaceEdit {
    pub(super) surface: NurbsSurface,
    pub(super) periodic: Option<[bool; 2]>,
}

pub(super) struct NurbsCurveEdit {
    pub(super) curve: NurbsCurve,
    pub(super) periodic: Option<bool>,
}

pub(super) enum PcurveEdit {
    Inline {
        /// The surface of the faces that use the pcurve.
        support: String,
        native_geometry: PcurveNurbs,
        periodic: Option<bool>,
        wrapper_reversed: Option<bool>,
        native_tail_flags: Option<[bool; 4]>,
        uv_offset: Option<[f64; 2]>,
        stored_uv_offset: [f64; 2],
        fit_tolerance: Option<f64>,
    },
    Ref {
        /// The surface of the faces that use the pcurve.
        support: String,
        native_geometry: PcurveNurbs,
        periodic: Option<bool>,
        uv_offset: Option<[f64; 2]>,
        stored_uv_offset: [f64; 2],
    },
}

impl PcurveEdit {
    /// The surface of the faces that use the pcurve.
    pub(super) fn support(&self) -> &str {
        match self {
            Self::Inline { support, .. } | Self::Ref { support, .. } => support,
        }
    }

    pub(super) fn stored_uv_offset(&self) -> [f64; 2] {
        match self {
            Self::Inline {
                stored_uv_offset, ..
            }
            | Self::Ref {
                stored_uv_offset, ..
            } => *stored_uv_offset,
        }
    }

    /// The edited pcurve in the writer's native support chart.
    pub(super) fn native_geometry(&self) -> &PcurveNurbs {
        match self {
            Self::Inline {
                native_geometry, ..
            }
            | Self::Ref {
                native_geometry, ..
            } => native_geometry,
        }
    }
}

#[derive(Clone)]
pub(super) struct ProceduralCurveEdit {
    definition: Option<cadmpeg_ir::geometry::ProceduralCurveDefinition>,
    fit_tolerance: Option<f64>,
}

impl ProceduralCurveEdit {
    /// An edit that changes at least one of the definition and the fit tolerance.
    fn new(
        definition: Option<cadmpeg_ir::geometry::ProceduralCurveDefinition>,
        fit_tolerance: Option<f64>,
    ) -> Option<Self> {
        (definition.is_some() || fit_tolerance.is_some()).then_some(Self {
            definition,
            fit_tolerance,
        })
    }

    /// Replacement procedural definition, when the edit changes it.
    pub(super) fn definition(&self) -> Option<&cadmpeg_ir::geometry::ProceduralCurveDefinition> {
        self.definition.as_ref()
    }

    /// Replacement cache fit tolerance, when the edit changes it.
    pub(super) fn fit_tolerance(&self) -> Option<f64> {
        self.fit_tolerance
    }
}

pub(super) fn validate_material_assignment_appearances(
    native: PatchNatives<'_>,
    baseline: &CadIr,
    target: &CadIr,
) -> Result<BTreeMap<String, crate::materials::ProteinAppearanceEdit>, CodecError> {
    let baseline_native = native.baseline;
    let target_native = native.target;
    let baseline_appearances = baseline
        .model
        .appearances
        .iter()
        .map(|appearance| (appearance.id.as_str(), appearance))
        .collect::<BTreeMap<_, _>>();
    let target_appearances = target
        .model
        .appearances
        .iter()
        .map(|appearance| (appearance.id.as_str(), appearance))
        .collect::<BTreeMap<_, _>>();
    if baseline_appearances.keys().ne(target_appearances.keys()) {
        return Err(CodecError::NotImplemented(
            "F3D material regeneration requires the unchanged appearance-id set".into(),
        ));
    }
    let target_assignments = target_native
        .as_ref()
        .map(|native| &native.design_material_assignments[..])
        .unwrap_or_default();
    let baseline_assignments = baseline_native
        .as_ref()
        .map(|native| &native.design_material_assignments[..])
        .unwrap_or_default();
    let mut appearance_edits = BTreeMap::new();
    for (id, before) in baseline_appearances {
        let after = target_appearances[id];
        let mut normalized = after.clone();
        normalized.physical_token.clone_from(&before.physical_token);
        normalized.base_color = before.base_color;
        normalized.properties.clone_from(&before.properties);
        if &normalized != before {
            return Err(CodecError::NotImplemented(format!(
                "F3D appearance edit changes fields outside physical token or Protein values: {id}"
            )));
        }
        if before.properties.keys().ne(after.properties.keys()) {
            return Err(CodecError::NotImplemented(format!(
                "F3D Protein property regeneration requires the unchanged property set: {id}"
            )));
        }
        let mut edit = crate::materials::ProteinAppearanceEdit::default();
        if after.base_color != before.base_color {
            let color = after.base_color.ok_or_else(|| {
                CodecError::NotImplemented(format!("cannot remove F3D appearance color: {id}"))
            })?;
            if before.base_color.is_none() || color.a() != 1.0 {
                return Err(CodecError::NotImplemented(format!(
                    "F3D Protein color {id} must replace an existing opaque finite RGBA color"
                )));
            }
            edit.color = Some(color);
        }
        for (name, before_value) in &before.properties {
            let after_value = after.properties[name].get();
            if after_value == before_value.get() {
                continue;
            }
            let valid = match name.as_str() {
                "reflectivity_at_0deg" | "surface_roughness" => (0.0..=1.0).contains(&after_value),
                "refraction_index" => (1.0..=4.0).contains(&after_value),
                _ => false,
            };
            if !valid {
                return Err(CodecError::NotImplemented(format!(
                    "F3D Protein property {id}.{name} is outside its writable range"
                )));
            }
            edit.properties
                .insert(name.as_str().to_owned(), after_value);
        }
        if edit.color.is_some() || !edit.properties.is_empty() {
            let guid = after.visual_guid.clone().ok_or_else(|| {
                CodecError::NotImplemented(format!("F3D appearance {id} has no visual token"))
            })?;
            appearance_edits.insert(guid, edit);
        }
        if after.physical_token == before.physical_token {
            continue;
        }
        let mut synchronized = false;
        for assignment in target_assignments {
            let selected =
                crate::materials::appearance_for_assignment(&target.model.appearances, assignment)?;
            if selected.is_some_and(|appearance| appearance.id == after.id)
                && after.physical_token.as_deref()
                    == assignment
                        .physical_token
                        .as_ref()
                        .map(|field| field.value.as_str())
            {
                synchronized = true;
                break;
            }
        }
        if !synchronized {
            return Err(CodecError::NotImplemented(format!(
                "F3D appearance {id} changed without a synchronized material assignment"
            )));
        }
    }
    for before in baseline_assignments {
        let Some(after) = target_assignments
            .iter()
            .find(|assignment| assignment.id == before.id)
        else {
            continue;
        };
        let selected =
            crate::materials::appearance_for_assignment(&target.model.appearances, after)?;
        let before_token = before
            .physical_token
            .as_ref()
            .map(|field| field.value.as_str());
        let after_token = after
            .physical_token
            .as_ref()
            .map(|field| field.value.as_str());
        if after_token != before_token
            && selected.is_none_or(|appearance| appearance.physical_token.as_deref() != after_token)
        {
            return Err(CodecError::NotImplemented(format!(
                "F3D material assignment {} changed without its appearance physical token",
                before.id
            )));
        }
    }
    Ok(appearance_edits)
}

pub(super) fn validate_material_assignment_edits(
    native: PatchNatives<'_>,
) -> Result<BTreeMap<String, Vec<DesignMaterialAssignment>>, CodecError> {
    let baseline_native = native.baseline;
    let target_native = native.target;
    let baseline = baseline_native
        .as_ref()
        .map(|native| &native.design_material_assignments[..])
        .unwrap_or_default();
    let target = target_native
        .as_ref()
        .map(|native| &native.design_material_assignments[..])
        .unwrap_or_default();
    let baseline_by_id = baseline
        .iter()
        .map(|assignment| (assignment.id.as_str(), assignment))
        .collect::<BTreeMap<_, _>>();
    let target_by_id = target
        .iter()
        .map(|assignment| (assignment.id.as_str(), assignment))
        .collect::<BTreeMap<_, _>>();
    if baseline_by_id.keys().ne(target_by_id.keys()) {
        return Err(CodecError::NotImplemented(
            "F3D material-assignment regeneration requires the unchanged assignment-id set".into(),
        ));
    }
    let mut edits: BTreeMap<String, Vec<DesignMaterialAssignment>> = BTreeMap::new();
    for (id, before) in baseline_by_id {
        let after = target_by_id[id];
        if after.entity_id != before.entity_id {
            return Err(CodecError::NotImplemented(format!(
                "F3D material-assignment identity edit requires synchronized body-presentation, browser-node, B-rep, and scene graphs: {id}"
            )));
        }
        let mut normalized = after.clone();
        normalized.visual_guid.clone_from(&before.visual_guid);
        normalized.physical_token = normalized_token(
            before.physical_token.as_ref(),
            after.physical_token.as_ref(),
        );
        normalized.visual_preset =
            normalized_token(before.visual_preset.as_ref(), after.visual_preset.as_ref());
        if &normalized != before {
            return Err(CodecError::NotImplemented(format!(
                "F3D material-assignment edit changes fields outside writable strings: {id}"
            )));
        }
        if after == before {
            continue;
        }
        validate_utf16_replacement(id, &before.visual_guid, &after.visual_guid)?;
        validate_optional_utf16_replacement(
            id,
            before
                .physical_token
                .as_ref()
                .map(|field| field.value.as_str()),
            after
                .physical_token
                .as_ref()
                .map(|field| field.value.as_str()),
        )?;
        validate_optional_utf16_replacement(
            id,
            before
                .visual_preset
                .as_ref()
                .map(|field| field.value.as_str()),
            after
                .visual_preset
                .as_ref()
                .map(|field| field.value.as_str()),
        )?;
        edits
            .entry(native_stream(id, ":material-assignment#")?)
            .or_default()
            .push(after.clone());
    }
    Ok(edits)
}

fn validate_utf16_replacement(id: &str, before: &str, after: &str) -> Result<(), CodecError> {
    if before.encode_utf16().count() != after.encode_utf16().count() {
        return Err(CodecError::NotImplemented(format!(
            "F3D native string {id} must retain its UTF-16 length"
        )));
    }
    Ok(())
}

fn validate_optional_utf16_replacement(
    id: &str,
    before: Option<&str>,
    after: Option<&str>,
) -> Result<(), CodecError> {
    match (before, after) {
        (Some(before), Some(after)) => validate_utf16_replacement(id, before, after),
        (None, None) => Ok(()),
        _ => Err(CodecError::NotImplemented(format!(
            "cannot add or remove F3D native string carrier: {id}"
        ))),
    }
}

pub(super) fn validate_lost_edge_edits(
    native: PatchNatives<'_>,
) -> Result<BTreeMap<String, Vec<LostEdgeReference>>, CodecError> {
    let baseline_native = native.baseline;
    let target_native = native.target;
    let baseline = baseline_native
        .as_ref()
        .map(|native| &native.lost_edge_references[..])
        .unwrap_or_default();
    let target = target_native
        .as_ref()
        .map(|native| &native.lost_edge_references[..])
        .unwrap_or_default();
    let baseline_by_id = baseline
        .iter()
        .map(|reference| (reference.id.as_str(), reference))
        .collect::<BTreeMap<_, _>>();
    let target_by_id = target
        .iter()
        .map(|reference| (reference.id.as_str(), reference))
        .collect::<BTreeMap<_, _>>();
    if baseline_by_id.keys().ne(target_by_id.keys()) {
        return Err(CodecError::NotImplemented(
            "F3D lost-edge regeneration requires the unchanged reference-id set".into(),
        ));
    }
    let mut edits: BTreeMap<String, Vec<LostEdgeReference>> = BTreeMap::new();
    for (id, before) in baseline_by_id {
        let after = target_by_id[id];
        let mut normalized = after.clone();
        normalized.class_tag.clone_from(&before.class_tag);
        normalized.record_index = before.record_index;
        if &normalized != before {
            return Err(CodecError::NotImplemented(format!(
                "F3D lost-edge edit changes fields outside its fixed payload: {id}"
            )));
        }
        if after == before {
            continue;
        }
        edits
            .entry(native_stream(id, ":lost-edge-reference#")?)
            .or_default()
            .push(after.clone());
    }
    Ok(edits)
}

pub(super) fn validate_act_appearance_bindings(
    native: PatchNatives<'_>,
    baseline: &CadIr,
    target: &CadIr,
) -> Result<(), CodecError> {
    let baseline_native = native.baseline;
    let target_native = native.target;
    let baseline_entities = baseline_native
        .as_ref()
        .map(|native| &native.act_entities[..])
        .unwrap_or_default();
    let target_entities = target_native
        .as_ref()
        .map(|native| &native.act_entities[..])
        .unwrap_or_default();
    if baseline.model.appearance_bindings.len() != target.model.appearance_bindings.len() {
        return Err(CodecError::NotImplemented(
            "F3D ACT regeneration requires the unchanged appearance-binding count".into(),
        ));
    }
    let target_bindings = target
        .model
        .appearance_bindings
        .iter()
        .map(|binding| {
            (
                (binding.target.clone(), binding.appearance.clone()),
                binding,
            )
        })
        .collect::<HashMap<_, _>>();
    let mut baseline_entities_by_source = HashMap::<_, Vec<_>>::new();
    for entity in baseline_entities {
        baseline_entities_by_source
            .entry(entity.entity_id())
            .or_default()
            .push(entity);
    }
    let target_entities_by_id = target_entities
        .iter()
        .map(|entity| (entity.id().as_str(), entity))
        .collect::<HashMap<_, _>>();
    for before in &baseline.model.appearance_bindings {
        let after = target_bindings
            .get(&(before.target.clone(), before.appearance.clone()))
            .copied()
            .ok_or_else(|| {
                CodecError::NotImplemented(format!(
                    "F3D appearance binding target or appearance changed: {}",
                    before.id
                ))
            })?;
        let mut normalized = after.clone();
        normalized.id.clone_from(&before.id);
        normalized
            .source_entity_id
            .clone_from(&before.source_entity_id);
        normalized.channels.clone_from(&before.channels);
        if &normalized != before {
            return Err(CodecError::NotImplemented(format!(
                "F3D appearance binding edit changes fields outside its derived identity, source, or channels: {}",
                before.id
            )));
        }
        if after == before {
            continue;
        }
        let before_entity = before
            .source_entity_id
            .as_deref()
            .and_then(|source| baseline_entities_by_source.get(source))
            .and_then(|entities| {
                entities.iter().copied().find(|entity| {
                    before
                        .channels
                        .iter()
                        .map(|(name, guid)| (name.as_str(), guid.as_str()))
                        .eq(act_channel_values(entity))
                })
            });
        let after_entity = before_entity.and_then(|before_entity| {
            target_entities_by_id
                .get(before_entity.id().as_str())
                .copied()
                .filter(|entity| {
                    after
                        .channels
                        .iter()
                        .map(|(name, guid)| (name.as_str(), guid.as_str()))
                        .eq(act_channel_values(entity))
                })
        });
        if before_entity.is_none() || after_entity.is_none() {
            return Err(CodecError::NotImplemented(format!(
                "F3D appearance binding {} must remain synchronized with one ACT entity",
                before.id
            )));
        }
        if after.source_entity_id != before.source_entity_id
            && after
                .source_entity_id
                .as_deref()
                .is_none_or(|source| !after.id.as_str().contains(source))
        {
            return Err(CodecError::malformed(format_args!(
                "F3D appearance binding id does not contain its changed source entity: {}",
                after.id
            )));
        }
    }
    let derived_bindings = baseline
        .model
        .appearance_bindings
        .iter()
        .filter_map(|binding| {
            Some((
                binding.source_entity_id.clone()?,
                binding.channels.clone(),
                binding,
            ))
        })
        .fold(HashMap::<_, Vec<_>>::new(), |mut grouped, entry| {
            grouped.entry(entry.0).or_default().push((entry.1, entry.2));
            grouped
        });
    let assignment_entities = target_native
        .as_ref()
        .into_iter()
        .flat_map(|native| &native.design_material_assignments)
        .map(|assignment| assignment.entity_id.as_str())
        .collect::<std::collections::HashSet<_>>();
    for (before, after) in baseline_entities.iter().zip(target_entities) {
        let matching_bindings = derived_bindings.get(before.entity_id());
        let derived_binding = matching_bindings.is_some_and(|bindings| {
            bindings.iter().any(|(channels, _)| {
                channels
                    .iter()
                    .map(|(name, guid)| (name.as_str(), guid.as_str()))
                    .eq(act_channel_values(before))
            })
        });
        let assignment_synchronized = assignment_entities.contains(after.entity_id());
        if before.entity_id() != after.entity_id() && derived_binding && !assignment_synchronized {
            return Err(CodecError::NotImplemented(format!(
                "F3D ACT entity {} changed without its material-assignment carrier",
                before.id()
            )));
        }
        let synchronized = matching_bindings.is_some_and(|bindings| {
            bindings.iter().any(|(_, before_binding)| {
                target_bindings
                    .get(&(
                        before_binding.target.clone(),
                        before_binding.appearance.clone(),
                    ))
                    .is_some_and(|binding| {
                        binding.source_entity_id.as_deref() == Some(after.entity_id())
                            && binding
                                .channels
                                .iter()
                                .map(|(name, guid)| (name.as_str(), guid.as_str()))
                                .eq(act_channel_values(after))
                    })
            })
        });
        if (before.entity_id() != after.entity_id()
            || act_channel_values(before).ne(act_channel_values(after)))
            && derived_binding
            && !synchronized
        {
            return Err(CodecError::NotImplemented(format!(
                "F3D ACT entity {} changed without a synchronized appearance binding",
                before.id()
            )));
        }
    }
    Ok(())
}

fn act_channel_values(entity: &ActEntity) -> impl Iterator<Item = (&str, &str)> {
    entity
        .channel_group()
        .channels()
        .iter()
        .map(|(name, guid)| (name.as_str(), guid.value.as_str()))
}

pub(super) fn validate_act_entity_edits(
    native: PatchNatives<'_>,
) -> Result<BTreeMap<String, Vec<ActEntity>>, CodecError> {
    let baseline_native = native.baseline;
    let target_native = native.target;
    let baseline = baseline_native
        .as_ref()
        .map(|native| &native.act_entities[..])
        .unwrap_or_default();
    let target = target_native
        .as_ref()
        .map(|native| &native.act_entities[..])
        .unwrap_or_default();
    let baseline_by_id = baseline
        .iter()
        .map(|entity| (entity.id().as_str(), entity))
        .collect::<BTreeMap<_, _>>();
    let target_by_id = target
        .iter()
        .map(|entity| (entity.id().as_str(), entity))
        .collect::<BTreeMap<_, _>>();
    if baseline_by_id.keys().ne(target_by_id.keys()) {
        return Err(CodecError::NotImplemented(
            "F3D ACT entity regeneration requires the unchanged entity-id set".into(),
        ));
    }
    let mut edits: BTreeMap<String, Vec<ActEntity>> = BTreeMap::new();
    for (id, before) in baseline_by_id {
        let after = target_by_id[id];
        let mut normalized = after.clone();
        normalized
            .try_set_entity_id(before.entity_id().to_owned())
            .map_err(CodecError::malformed)?;
        for (name, guid) in before.channel_group().channels() {
            if normalized.channel_group().channels().contains_key(name) {
                normalized
                    .set_channel_guid(name, guid.value.clone())
                    .map_err(CodecError::malformed)?;
            }
        }
        if &normalized != before {
            return Err(CodecError::NotImplemented(format!(
                "F3D ACT entity edit changes fields other than entity_id or channel GUIDs: {id}"
            )));
        }
        if after == before {
            continue;
        }
        if after.entity_id().encode_utf16().count() != before.entity_id().encode_utf16().count() {
            return Err(CodecError::NotImplemented(format!(
                "F3D ACT entity id {id} must retain its UTF-16 length"
            )));
        }
        edits
            .entry(native_stream(id, ":act-entity#")?)
            .or_default()
            .push(after.clone());
    }
    Ok(edits)
}

pub(super) fn validate_act_guid_edits(
    native: PatchNatives<'_>,
) -> Result<BTreeMap<String, Vec<ByteEdit>>, CodecError> {
    let baseline_native = native.baseline;
    let target_native = native.target;
    let baseline = baseline_native
        .as_ref()
        .map(|native| &native.act_guids[..])
        .unwrap_or_default();
    let target = target_native
        .as_ref()
        .map(|native| &native.act_guids[..])
        .unwrap_or_default();
    let baseline_by_id = baseline
        .iter()
        .map(|guid| (guid.id().as_str(), guid))
        .collect::<BTreeMap<_, _>>();
    let target_by_id = target
        .iter()
        .map(|guid| (guid.id().as_str(), guid))
        .collect::<BTreeMap<_, _>>();
    if baseline_by_id.keys().ne(target_by_id.keys()) {
        return Err(CodecError::NotImplemented(
            "F3D ACT GUID regeneration requires the unchanged GUID-id set".into(),
        ));
    }
    let mut edits: BTreeMap<String, Vec<ByteEdit>> = BTreeMap::new();
    for (id, before) in baseline_by_id {
        let after = target_by_id[id];
        let mut normalized: ActGuid = after.clone();
        normalized.guid.clone_from(&before.guid);
        if &normalized != before {
            return Err(CodecError::NotImplemented(format!(
                "F3D ACT GUID edit changes fields other than guid: {id}"
            )));
        }
        if after.guid == before.guid {
            continue;
        }
        let encoded = after
            .guid
            .as_str()
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>();
        let stream = native_stream(id, ":act-guid#")?;
        edits.entry(stream).or_default().push(ByteEdit {
            offset: after.guid_offset(),
            value: encoded,
        });
    }
    Ok(edits)
}

pub(super) fn validate_act_registry_channel_edits(
    native: PatchNatives<'_>,
) -> Result<BTreeMap<String, Vec<ByteEdit>>, CodecError> {
    let baseline = native
        .baseline
        .map(|native| &native.act_registry_channels[..])
        .unwrap_or_default();
    let target = native
        .target
        .map(|native| &native.act_registry_channels[..])
        .unwrap_or_default();
    let baseline_by_id = baseline
        .iter()
        .map(|channel| (channel.id().as_str(), channel))
        .collect::<BTreeMap<_, _>>();
    let target_by_id = target
        .iter()
        .map(|channel| (channel.id().as_str(), channel))
        .collect::<BTreeMap<_, _>>();
    if baseline_by_id.keys().ne(target_by_id.keys()) {
        return Err(CodecError::NotImplemented(
            "F3D ACT channel-registry regeneration requires the unchanged entry-id set".into(),
        ));
    }
    let mut edits: BTreeMap<String, Vec<ByteEdit>> = BTreeMap::new();
    for (id, before) in baseline_by_id {
        let after = target_by_id[id];
        let mut normalized: ActRegistryChannel = after.clone();
        normalized.guid.clone_from(&before.guid);
        if &normalized != before {
            return Err(CodecError::NotImplemented(format!(
                "F3D ACT channel-registry edit changes fields other than guid: {id}"
            )));
        }
        if after.guid == before.guid {
            continue;
        }
        let encoded = after
            .guid
            .as_str()
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>();
        edits
            .entry(native_stream(id, ":act-registry-channel#")?)
            .or_default()
            .push(ByteEdit {
                offset: after.guid_offset(),
                value: encoded,
            });
    }
    Ok(edits)
}

pub(super) fn validate_act_link_edits(
    native: PatchNatives<'_>,
) -> Result<BTreeMap<String, Vec<ActComponentLink>>, CodecError> {
    let baseline_native = native.baseline;
    let target_native = native.target;
    let baseline = baseline_native
        .as_ref()
        .map(|native| &native.act_component_links[..])
        .unwrap_or_default();
    let target = target_native
        .as_ref()
        .map(|native| &native.act_component_links[..])
        .unwrap_or_default();
    let baseline_by_id = baseline
        .iter()
        .map(|root| (root.id().as_str(), root))
        .collect::<BTreeMap<_, _>>();
    let target_by_id = target
        .iter()
        .map(|root| (root.id().as_str(), root))
        .collect::<BTreeMap<_, _>>();
    if baseline_by_id.keys().ne(target_by_id.keys()) {
        return Err(CodecError::NotImplemented(
            "F3D ACT component-link regeneration requires the unchanged link-id set".into(),
        ));
    }
    let mut edits: BTreeMap<String, Vec<ActComponentLink>> = BTreeMap::new();
    for (id, before) in baseline_by_id {
        let after = target_by_id[id];
        let mut normalized = after.clone();
        normalized.references.clone_from(&before.references);
        normalized.registry_flag = before.registry_flag;
        normalized
            .try_set_strings(
                before.layout().entity_id().into(),
                before.layout().display_name().into(),
            )
            .map_err(CodecError::NotImplemented)?;
        if &normalized != before {
            return Err(CodecError::NotImplemented(format!(
                "F3D ACT component-link edit changes fields outside supported graph links and fixed-length strings: {id}"
            )));
        }
        if after == before {
            continue;
        }
        if after.layout().entity_id().encode_utf16().count()
            != before.layout().entity_id().encode_utf16().count()
            || after.layout().display_name().encode_utf16().count()
                != before.layout().display_name().encode_utf16().count()
        {
            return Err(CodecError::NotImplemented(format!(
                "F3D ACT component-link strings must retain their UTF-16 lengths: {id}"
            )));
        }
        edits
            .entry(native_stream(id, ":act-component-link#")?)
            .or_default()
            .push(after.clone());
    }
    Ok(edits)
}

pub(super) struct DesignTypeEdit {
    pub(super) integers: Vec<(u64, Vec<u8>)>,
    pub(super) strings: Vec<(u64, Vec<u8>)>,
}

pub(super) fn validate_design_type_edits(
    native: PatchNatives<'_>,
) -> Result<BTreeMap<String, Vec<DesignTypeEdit>>, CodecError> {
    let baseline_native = native.baseline;
    let target_native = native.target;
    let baseline = baseline_native
        .as_ref()
        .map(|native| &native.design_types[..])
        .unwrap_or_default();
    let target = target_native
        .as_ref()
        .map(|native| &native.design_types[..])
        .unwrap_or_default();
    let baseline_by_id = baseline
        .iter()
        .map(|design_type| (design_type.id().as_str(), design_type))
        .collect::<BTreeMap<_, _>>();
    let target_by_id = target
        .iter()
        .map(|design_type| (design_type.id().as_str(), design_type))
        .collect::<BTreeMap<_, _>>();
    if baseline_by_id.keys().ne(target_by_id.keys()) {
        return Err(CodecError::NotImplemented(
            "F3D design-type regeneration requires the unchanged type-id set".into(),
        ));
    }
    let mut edits: BTreeMap<String, Vec<DesignTypeEdit>> = BTreeMap::new();
    for (id, before) in baseline_by_id {
        let after = target_by_id[id];
        if before.type_guid_offset != after.type_guid_offset
            || before.version_offset != after.version_offset
            || before.module != after.module
            || before.base_type_guid.offset() != after.base_type_guid.offset()
        {
            return Err(CodecError::NotImplemented(format!(
                "F3D design-type edit changes fields outside its fixed type payload: {id}"
            )));
        }
        let (before_entities, after_entities): (
            &[crate::records::identity::Located<u64>],
            &[crate::records::identity::Located<u64>],
        ) = match (
            before.entities.located_rows(),
            after.entities.located_rows(),
        ) {
            (Some(before), Some(after)) => (before, after),
            _ => {
                return Err(CodecError::NotImplemented(format!(
                    "F3D design type {id} must retain its entity-id cardinality"
                )))
            }
        };
        if after_entities.len() != before_entities.len() {
            return Err(CodecError::NotImplemented(format!(
                "F3D design type {id} must retain its entity-id cardinality"
            )));
        }
        if before_entities
            .iter()
            .map(|row| row.offset)
            .ne(after_entities.iter().map(|row| row.offset))
        {
            return Err(CodecError::NotImplemented(format!(
                "F3D design-type edit changes fields outside its fixed type payload: {id}"
            )));
        }
        let mut integers = after_entities
            .iter()
            .zip(before_entities)
            .filter(|(after, before)| after.value != before.value)
            .map(|(after, _)| (after.offset, after.value.to_le_bytes().to_vec()))
            .collect::<Vec<_>>();
        if after.version != before.version {
            integers.push((after.version_offset, after.version.to_le_bytes().to_vec()));
        }
        let mut strings = Vec::new();
        if after.type_guid != before.type_guid {
            validate_fixed_design_string(id, before.type_guid.as_str(), after.type_guid.as_str())?;
            strings.push((
                after.type_guid_offset,
                after.type_guid.as_str().as_bytes().to_vec(),
            ));
        }
        if after.base_type_guid != before.base_type_guid {
            let (before_base, _) = located_base_guid(&before.base_type_guid).ok_or_else(|| {
                CodecError::NotImplemented(format!("cannot add F3D base type GUID: {id}"))
            })?;
            let (after_base, after_offset) =
                located_base_guid(&after.base_type_guid).ok_or_else(|| {
                    CodecError::NotImplemented(format!("cannot remove F3D base type GUID: {id}"))
                })?;
            validate_fixed_design_string(id, before_base, after_base)?;
            strings.push((after_offset, after_base.as_bytes().to_vec()));
        }
        if integers.is_empty() && strings.is_empty() {
            continue;
        }
        let stream = id
            .strip_prefix(crate::ids::SCHEME_PREFIX)
            .and_then(|id| id.rsplit_once(":design-type#"))
            .map(|(stream, _)| stream.to_owned())
            .ok_or_else(|| CodecError::malformed(format_args!("invalid design-type id {id}")))?;
        edits
            .entry(stream)
            .or_default()
            .push(DesignTypeEdit { integers, strings });
    }
    Ok(edits)
}

fn validate_fixed_design_string(id: &str, before: &str, after: &str) -> Result<(), CodecError> {
    if before.len() != after.len()
        || !after
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
    {
        return Err(CodecError::NotImplemented(format!(
            "F3D object string {id} must retain its encoded length and alphabet"
        )));
    }
    Ok(())
}

pub(super) fn validate_configuration_edits(
    native: PatchNatives<'_>,
) -> Result<BTreeMap<String, Vec<u8>>, CodecError> {
    let baseline = native
        .baseline
        .map_or(&[][..], |native| native.design_configurations.as_slice());
    let target = native
        .target
        .map_or(&[][..], |native| native.design_configurations.as_slice());
    let baseline = baseline
        .iter()
        .map(|configuration| (configuration.entry_name().as_str(), configuration))
        .collect::<BTreeMap<_, _>>();
    let target = target
        .iter()
        .map(|configuration| (configuration.entry_name().as_str(), configuration))
        .collect::<BTreeMap<_, _>>();
    if baseline.keys().ne(target.keys()) {
        return Err(CodecError::NotImplemented(
            "retained F3D configuration editing requires the unchanged entry-name set".into(),
        ));
    }
    let mut edits = BTreeMap::new();
    for (name, before) in baseline {
        let after = target[name];
        if before.id() != after.id() || before.kind() != after.kind() {
            return Err(CodecError::NotImplemented(format!(
                "retained F3D configuration edit changes entry identity: {name}"
            )));
        }
        if before.active_row() != after.active_row() {
            return Err(CodecError::NotImplemented(format!(
                "retained F3D configuration edit changes the Design root record: {name}"
            )));
        }
        if before != after {
            edits.insert(
                name.to_owned(),
                crate::records::configuration::encode_configuration_payload(after)?,
            );
        }
    }
    Ok(edits)
}

pub(super) struct EntityHeaderEdit {
    pub(super) record_reference: Option<Edit<u32>>,
    pub(super) references: Vec<Edit<u32>>,
}

pub(super) fn validate_entity_header_edits(
    native: PatchNatives<'_>,
) -> Result<BTreeMap<String, Vec<EntityHeaderEdit>>, CodecError> {
    let baseline_native = native.baseline;
    let target_native = native.target;
    let baseline = baseline_native
        .as_ref()
        .map(|native| &native.design_entity_headers[..])
        .unwrap_or_default();
    let target = target_native
        .as_ref()
        .map(|native| &native.design_entity_headers[..])
        .unwrap_or_default();
    let baseline_by_id = baseline
        .iter()
        .map(|header| (header.id.as_str(), header))
        .collect::<BTreeMap<_, _>>();
    let target_by_id = target
        .iter()
        .map(|header| (header.id.as_str(), header))
        .collect::<BTreeMap<_, _>>();
    if baseline_by_id.keys().ne(target_by_id.keys()) {
        return Err(CodecError::NotImplemented(
            "F3D entity-header regeneration requires the unchanged header-id set".into(),
        ));
    }
    let mut edits: BTreeMap<String, Vec<EntityHeaderEdit>> = BTreeMap::new();
    for (id, before) in baseline_by_id {
        let after = target_by_id[id];
        let mut normalized = after.clone();
        if let (Some(normalized), Some(before)) = (
            normalized.sketch_references_mut(),
            before.sketch_references(),
        ) {
            normalized.record_reference = before.record_reference;
            for (row, before) in normalized.references.iter_mut().zip(&before.references) {
                row.value = before.value;
            }
        }
        if &normalized != before {
            return Err(CodecError::NotImplemented(format!(
                "F3D entity-header edit changes fields outside fixed record references: {id}"
            )));
        }
        let (Some(before), Some(after)) = (before.sketch_references(), after.sketch_references())
        else {
            continue;
        };
        if after == before {
            continue;
        }
        let record_reference = if after.record_reference == before.record_reference {
            None
        } else {
            Some(Edit {
                offset: after.record_reference_offset,
                value: after.record_reference.ok_or_else(|| {
                    CodecError::NotImplemented(format!(
                        "cannot remove F3D entity-header record reference: {id}"
                    ))
                })?,
            })
        };
        let references = after
            .references
            .iter()
            .zip(&before.references)
            .filter_map(|(after, before)| {
                (after.value != before.value).then_some(Edit {
                    offset: after.offset,
                    value: after.value,
                })
            })
            .collect();
        let stream = id
            .strip_prefix(crate::ids::SCHEME_PREFIX)
            .and_then(|id| id.rsplit_once(":design-entity-header#"))
            .map(|(stream, _)| stream.to_owned())
            .ok_or_else(|| {
                CodecError::malformed(format_args!("invalid design-entity-header id {id}"))
            })?;
        edits.entry(stream).or_default().push(EntityHeaderEdit {
            record_reference,
            references,
        });
    }
    Ok(edits)
}

pub(super) fn validate_body_member_edits(
    native: PatchNatives<'_>,
) -> Result<BTreeMap<String, Vec<BodyMemberEdit>>, CodecError> {
    let baseline_native = native.baseline;
    let target_native = native.target;
    let baseline = baseline_native
        .as_ref()
        .map(|native| &native.design_body_members[..])
        .unwrap_or_default();
    let target = target_native
        .as_ref()
        .map(|native| &native.design_body_members[..])
        .unwrap_or_default();
    let baseline_by_id = baseline
        .iter()
        .map(|member| (member.id().as_str(), member))
        .collect::<BTreeMap<_, _>>();
    let target_by_id = target
        .iter()
        .map(|member| (member.id().as_str(), member))
        .collect::<BTreeMap<_, _>>();
    if baseline_by_id.keys().ne(target_by_id.keys()) {
        return Err(CodecError::NotImplemented(
            "F3D body-membership regeneration requires the unchanged member-id set".into(),
        ));
    }
    let mut edits: BTreeMap<String, Vec<BodyMemberEdit>> = BTreeMap::new();
    for (id, before) in baseline_by_id {
        let after = target_by_id[id];
        let mut normalized = after.clone();
        normalized.entity_suffix = before.entity_suffix;
        normalized.flags = before.flags;
        if &normalized != before {
            return Err(CodecError::NotImplemented(format!(
                "F3D body-member edit changes fields outside its fixed payload: {id}"
            )));
        }
        if after.entity_suffix == before.entity_suffix && after.flags == before.flags {
            continue;
        }
        let stream = id
            .strip_prefix(crate::ids::SCHEME_PREFIX)
            .and_then(|id| id.rsplit_once(":design-body-member#"))
            .map(|(stream, _)| stream.to_owned())
            .ok_or_else(|| {
                CodecError::malformed(format_args!("invalid design-body-member id {id}"))
            })?;
        edits.entry(stream).or_default().push(BodyMemberEdit {
            offset: after.byte_offset(),
            entity_suffix: after.entity_suffix,
            flags: after.flags,
        });
    }
    Ok(edits)
}

pub(super) fn validate_body_visibility_edits(
    native: PatchNatives<'_>,
    baseline: &CadIr,
    target: &CadIr,
) -> Result<BTreeMap<String, Vec<(u64, bool)>>, CodecError> {
    let baseline_bodies = baseline
        .model
        .bodies
        .iter()
        .map(|body| (&body.id, body))
        .collect::<BTreeMap<_, _>>();
    let target_bodies = target
        .model
        .bodies
        .iter()
        .map(|body| (&body.id, body))
        .collect::<BTreeMap<_, _>>();
    if baseline_bodies.keys().ne(target_bodies.keys()) {
        return Err(CodecError::NotImplemented(
            "F3D body-visibility regeneration requires the unchanged body-id set".into(),
        ));
    }
    let metadata = native
        .baseline
        .map(|native| {
            native
                .body_visibilities
                .iter()
                .map(|item| (item.body.clone(), item))
                .collect::<BTreeMap<_, _>>()
        })
        .unwrap_or_default();
    let mut edits = BTreeMap::<String, Vec<(u64, bool)>>::new();
    for (id, before) in baseline_bodies {
        let after = target_bodies[id];
        if after.visible == before.visible {
            continue;
        }
        let visible = after.visible.ok_or_else(|| {
            CodecError::NotImplemented(format!("cannot remove F3D body visibility: {id}"))
        })?;
        let record = metadata.get(id).ok_or_else(|| {
            CodecError::NotImplemented(format!(
                "F3D body visibility {id} has no joined Design browser node"
            ))
        })?;
        edits
            .entry(record.stream().to_owned())
            .or_default()
            .push((record.byte_offset, visible));
    }
    Ok(edits)
}

pub(super) fn validate_transform_hint_edits(
    native: PatchNatives<'_>,
) -> Result<BTreeMap<usize, [bool; 3]>, CodecError> {
    let baseline = native
        .baseline
        .map_or(&[][..], |native| native.transform_hints.as_slice());
    let target = native
        .target
        .map_or(&[][..], |native| native.transform_hints.as_slice());
    let baseline = baseline
        .iter()
        .map(|hints| (hints.id(), hints))
        .collect::<BTreeMap<_, _>>();
    let target = target
        .iter()
        .map(|hints| (hints.id(), hints))
        .collect::<BTreeMap<_, _>>();
    if baseline.keys().ne(target.keys()) {
        return Err(CodecError::NotImplemented(
            "F3D transform-hint regeneration requires the unchanged metadata-id set".into(),
        ));
    }
    let mut edits = BTreeMap::new();
    for (id, before) in baseline {
        let after = target[&id];
        let mut normalized = after.clone();
        normalized.rotation = before.rotation;
        normalized.reflection = before.reflection;
        normalized.shear = before.shear;
        if &normalized != before {
            return Err(CodecError::NotImplemented(format!(
                "F3D transform-hint edit changes structural fields: {id}"
            )));
        }
        let flags = [after.rotation, after.reflection, after.shear];
        if flags != [before.rotation, before.reflection, before.shear] {
            edits.insert(index_from_u32(after.record_index), flags);
        }
    }
    Ok(edits)
}

pub(in crate::writer) fn validate_body_native_key_edits(
    native: PatchNatives<'_>,
) -> Result<BodyNativeKeyEdits, CodecError> {
    let baseline_native = native.baseline;
    let target_native = native.target;
    let baseline_body_visibilities =
        baseline_native.map_or(&[][..], |native| native.body_visibilities.as_slice());
    let baseline_material_assignments = baseline_native.map_or(&[][..], |native| {
        native.design_material_assignments.as_slice()
    });
    let baseline = baseline_native
        .map_or(&[][..], |native| native.body_native_keys.as_slice())
        .iter()
        .map(|key| (key.id(), key))
        .collect::<BTreeMap<_, _>>();
    let target = target_native
        .map_or(&[][..], |native| native.body_native_keys.as_slice())
        .iter()
        .map(|key| (key.id(), key))
        .collect::<BTreeMap<_, _>>();
    if baseline.keys().ne(target.keys()) {
        return Err(CodecError::NotImplemented(
            "F3D body-key regeneration requires the unchanged metadata-id set".into(),
        ));
    }
    let mut edits = BodyNativeKeyEdits::default();
    for (id, before) in baseline {
        let after = target[&id];
        let mut normalized = after.clone();
        normalized.asm_body_key = before.asm_body_key;
        if &normalized != before {
            return Err(CodecError::NotImplemented(format!(
                "F3D body-key edit changes structural fields: {id}"
            )));
        }
        if after.asm_body_key != before.asm_body_key {
            let key = after.asm_body_key.map_or(Ok(-1), |key| {
                i64::try_from(key).map_err(|_| {
                    CodecError::NotImplemented(format!("F3D ASM body key exceeds i64::MAX: {key}"))
                })
            })?;
            edits.asm.insert(index_from_u32(after.record_index), key);
            let mut joined = Vec::new();
            joined.extend(
                baseline_body_visibilities
                    .iter()
                    .filter(|visibility| {
                        visibility.body == before.body
                            && before.asm_body_key == Some(visibility.asm_body_key())
                    })
                    .map(|visibility| {
                        (
                            visibility.stream().to_owned(),
                            visibility.asm_body_key_offset,
                        )
                    }),
            );
            if let Some(old_key) = before.asm_body_key {
                joined.extend(
                    baseline_material_assignments
                        .iter()
                        .filter(|assignment| assignment.asm_body_key == old_key)
                        .map(|assignment| {
                            Ok((
                                native_stream(&assignment.id, ":material-assignment#")?,
                                assignment.asm_body_key_offset,
                            ))
                        })
                        .collect::<Result<Vec<_>, CodecError>>()?,
                );
            }
            if !joined.is_empty() {
                let design_key = after.asm_body_key.ok_or_else(|| {
                    CodecError::NotImplemented(format!(
                        "cannot remove joined F3D ASM body key: {id}"
                    ))
                })?;
                for (stream, offset) in joined {
                    edits
                        .design
                        .entry(stream)
                        .or_default()
                        .insert((offset, design_key));
                }
            }
        }
    }
    Ok(edits)
}

#[derive(Default)]
pub(in crate::writer) struct BodyNativeKeyEdits {
    pub(in crate::writer) asm: BTreeMap<usize, i64>,
    pub(in crate::writer) design: BTreeMap<String, BTreeSet<(u64, u64)>>,
}

pub(super) struct ConstructionRecipeEdit {
    pub(super) record_index: Option<Edit<i32>>,
    pub(super) design_id: Option<ByteEdit>,
}

pub(super) fn validate_construction_recipe_edits(
    native: PatchNatives<'_>,
) -> Result<BTreeMap<String, Vec<ConstructionRecipeEdit>>, CodecError> {
    let baseline_native = native.baseline;
    let target_native = native.target;
    let baseline = baseline_native
        .as_ref()
        .map(|native| &native.construction_recipes[..])
        .unwrap_or_default();
    let target = target_native
        .as_ref()
        .map(|native| &native.construction_recipes[..])
        .unwrap_or_default();
    let baseline_by_id = baseline
        .iter()
        .map(|recipe| (recipe.id.as_str(), recipe))
        .collect::<BTreeMap<_, _>>();
    let target_by_id = target
        .iter()
        .map(|recipe| (recipe.id.as_str(), recipe))
        .collect::<BTreeMap<_, _>>();
    if baseline_by_id.keys().ne(target_by_id.keys()) {
        return Err(CodecError::NotImplemented(
            "F3D construction-recipe regeneration requires the unchanged recipe-id set".into(),
        ));
    }
    let mut edits: BTreeMap<String, Vec<ConstructionRecipeEdit>> = BTreeMap::new();
    for (id, before) in baseline_by_id {
        let after = target_by_id[id];
        let mut normalized = after.clone();
        normalized.record_index =
            before
                .record_index
                .zip(after.record_index)
                .map(
                    |(before_index, after_index)| crate::records::identity::RecordedValue {
                        value: before_index.value,
                        offset: after_index.offset,
                    },
                );
        normalized.design =
            before
                .design
                .as_ref()
                .zip(after.design.as_ref())
                .map(
                    |(design, after_design)| crate::records::recipes::ConstructionRecipeDesign {
                        id: crate::records::identity::RecordedValue {
                            value: design.id.value.clone(),
                            offset: after_design.id.offset,
                        },
                        selector: design.selector,
                    },
                );
        if &normalized != before
            || before.design.as_ref().and_then(|design| design.selector)
                != after.design.as_ref().and_then(|design| design.selector)
        {
            return Err(CodecError::NotImplemented(format!(
                "F3D construction-recipe edit changes fields other than record_index or design_id: {id}"
            )));
        }
        if after.record_index == before.record_index && after.design == before.design {
            continue;
        }
        let record_index = (after.record_index != before.record_index)
            .then(|| {
                after
                    .record_index
                    .map(|record_index| Edit {
                        offset: record_index.offset,
                        value: record_index.value,
                    })
                    .ok_or_else(|| {
                        CodecError::NotImplemented(format!(
                            "F3D construction recipe {id} has no writable record-index carrier"
                        ))
                    })
            })
            .transpose()?;
        let design_id = if after.design == before.design {
            None
        } else {
            let before_value = before
                .design
                .as_ref()
                .map(|design| design.id.value.as_str())
                .ok_or_else(|| {
                    CodecError::NotImplemented(format!("cannot add F3D recipe design id: {id}"))
                })?;
            let after_design = after.design.as_ref().ok_or_else(|| {
                CodecError::NotImplemented(format!("cannot remove F3D recipe design id: {id}"))
            })?;
            let after_field = &after_design.id;
            let after_value = after_field.value.as_str();
            let offset = after_field.offset;
            if after_value.len() != before_value.len()
                || !after_value.bytes().all(|byte| byte.is_ascii_alphanumeric())
            {
                return Err(CodecError::NotImplemented(format!(
                    "ASCII F3D recipe design id {id} must retain its encoded length"
                )));
            }
            let encoded = after_value.as_bytes().to_vec();
            Some(ByteEdit {
                offset,
                value: encoded,
            })
        };
        let stream = id
            .strip_prefix(crate::ids::SCHEME_PREFIX)
            .and_then(|id| id.rsplit_once(":construction-recipe#"))
            .map(|(stream, _)| stream.to_owned())
            .ok_or_else(|| {
                CodecError::malformed(format_args!("invalid construction-recipe id {id}"))
            })?;
        edits
            .entry(stream)
            .or_default()
            .push(ConstructionRecipeEdit {
                record_index,
                design_id,
            });
    }
    Ok(edits)
}

pub(super) fn validate_persistent_reference_edits(
    native: PatchNatives<'_>,
) -> Result<BTreeMap<String, Vec<PersistentReferenceEdit>>, CodecError> {
    let baseline_native = native.baseline;
    let target_native = native.target;
    let baseline = baseline_native
        .as_ref()
        .map(|native| &native.persistent_references[..])
        .unwrap_or_default();
    let target = target_native
        .as_ref()
        .map(|native| &native.persistent_references[..])
        .unwrap_or_default();
    let baseline_by_id = baseline
        .iter()
        .map(|reference| (reference.id.as_str(), reference))
        .collect::<BTreeMap<_, _>>();
    let target_by_id = target
        .iter()
        .map(|reference| (reference.id.as_str(), reference))
        .collect::<BTreeMap<_, _>>();
    if baseline_by_id.keys().ne(target_by_id.keys()) {
        return Err(CodecError::NotImplemented(
            "F3D persistent-reference regeneration requires the unchanged reference-id set".into(),
        ));
    }
    let mut edits: BTreeMap<String, Vec<PersistentReferenceEdit>> = BTreeMap::new();
    for (id, before) in baseline_by_id {
        let after = target_by_id[id];
        let mut normalized = after.clone();
        normalized.value = before.value;
        if &normalized != before {
            return Err(CodecError::NotImplemented(format!(
                "F3D persistent-reference edit changes fields other than value: {id}"
            )));
        }
        if after.value == before.value {
            continue;
        }
        let stream = id
            .strip_prefix(crate::ids::SCHEME_PREFIX)
            .and_then(|id| id.rsplit_once(":persistent-reference#"))
            .map(|(stream, _)| stream.to_owned())
            .ok_or_else(|| {
                CodecError::malformed(format_args!("invalid persistent-reference id {id}"))
            })?;
        edits
            .entry(stream)
            .or_default()
            .push(PersistentReferenceEdit {
                offset: after.byte_offset,
                identity_offset: after.value_offset,
                identity: after.value,
            });
    }
    Ok(edits)
}

/// The three history-preamble fields a patch writes back, carried as
/// non-optional values. [`validate_history_state_edits`] only builds one after
/// it has confirmed both `stream_size` and `history_entry_count` are present.
pub(in crate::writer::patch) struct PreambleEdit {
    pub(super) byte_offset: u64,
    pub(super) stream_size: i64,
    pub(super) history_entry_count: i64,
}

#[derive(Default)]
pub(super) struct HistoryEdits {
    pub(super) preamble: Option<PreambleEdit>,
    pub(super) states: Vec<AsmDeltaState>,
    pub(super) boards: Vec<AsmBulletinBoard>,
    pub(super) changes: Vec<AsmEntityChange>,
}

pub(super) fn validate_history_state_edits(
    native: PatchNatives<'_>,
) -> Result<BTreeMap<String, HistoryEdits>, CodecError> {
    let baseline_native = native.baseline;
    let target_native = native.target;
    let baseline = baseline_native
        .as_ref()
        .map(|native| &native.asm_histories[..])
        .unwrap_or_default();
    let target = target_native
        .as_ref()
        .map(|native| &native.asm_histories[..])
        .unwrap_or_default();
    let baseline_by_id = baseline
        .iter()
        .map(|history| (history.id.as_str(), history))
        .collect::<BTreeMap<_, _>>();
    if baseline_by_id
        .keys()
        .copied()
        .ne(target.iter().map(|history| history.id.as_str()))
    {
        return Err(CodecError::NotImplemented(
            "F3D history regeneration requires the unchanged history-id set".into(),
        ));
    }
    let mut edits: BTreeMap<String, HistoryEdits> = BTreeMap::new();
    for history in target {
        let before = baseline_by_id[history.id.as_str()];
        if before.states().len() != history.states().len() {
            return Err(CodecError::NotImplemented(format!(
                "F3D history edit changes the state count: {}",
                history.id
            )));
        }
        let mut normalized = history.clone();
        normalized.preamble = before.preamble;
        for (state, before_state) in normalized.states_mut().iter_mut().zip(before.states()) {
            state.state_id = before_state.state_id;
            state.version_flag = before_state.version_flag;
            state.state_flag = before_state.state_flag;
            state.previous_ref = before_state.previous_ref;
            state.next_ref = before_state.next_ref;
            state.node_index = before_state.node_index;
            state.partner_ref = before_state.partner_ref;
            state.owner_ref = before_state.owner_ref;
            if state.bulletin_boards.len() != before_state.bulletin_boards.len() {
                return Err(CodecError::NotImplemented(format!(
                    "F3D history edit changes the bulletin-board count: {}",
                    history.id
                )));
            }
            for (board, before_board) in state
                .bulletin_boards
                .iter_mut()
                .zip(&before_state.bulletin_boards)
            {
                board.owner_ref = before_board.owner_ref;
                board.number = before_board.number;
                if board.changes.len() != before_board.changes.len() {
                    return Err(CodecError::NotImplemented(format!(
                        "F3D history edit changes the entity-change count: {}",
                        board.id
                    )));
                }
                for (change, before_change) in board.changes.iter_mut().zip(&before_board.changes) {
                    change.kind = before_change.kind;
                }
            }
        }
        if &normalized != before {
            return Err(CodecError::NotImplemented(format!(
                "F3D history edit changes fields outside the fixed delta-state header: {}",
                history.id
            )));
        }
        let stream = history
            .id
            .strip_prefix(crate::ids::SCHEME_PREFIX)
            .and_then(|id| id.rsplit_once(":asm-history#"))
            .map(|(stream, _)| stream.to_owned())
            .ok_or_else(|| {
                CodecError::malformed(format_args!("invalid ASM history id {}", history.id))
            })?;
        if let Some(preamble) = history.preamble {
            if history
                .states()
                .first()
                .is_none_or(|state| state.state_id != preamble.stream_size)
                || preamble.history_entry_count < 0
            {
                return Err(CodecError::malformed(format_args!(
                    "F3D history {} requires head state_id == stream_size and nonnegative history_entry_count",
                    history.id
                )));
            }
        }
        if history.preamble != before.preamble {
            let (Some(preamble), Some(_)) = (history.preamble, before.preamble) else {
                return Err(CodecError::NotImplemented(format!(
                    "cannot add or remove the F3D history preamble: {}",
                    history.id
                )));
            };
            if history.byte_offset == 0 || preamble.history_entry_count < 0 {
                return Err(CodecError::malformed(format_args!(
                    "F3D history {} requires head state_id == stream_size and nonnegative history_entry_count",
                    history.id
                )));
            }
            edits.entry(stream.clone()).or_default().preamble = Some(PreambleEdit {
                byte_offset: history.byte_offset,
                stream_size: preamble.stream_size,
                history_entry_count: preamble.history_entry_count,
            });
        }
        for (state, before_state) in history.states().iter().zip(before.states()) {
            if state != before_state {
                edits
                    .entry(stream.clone())
                    .or_default()
                    .states
                    .push(state.clone());
            }
            for (board, before_board) in state
                .bulletin_boards
                .iter()
                .zip(&before_state.bulletin_boards)
            {
                if board.owner_ref != before_board.owner_ref || board.number != before_board.number
                {
                    edits
                        .entry(stream.clone())
                        .or_default()
                        .boards
                        .push(board.clone());
                }
                for (change, before_change) in board.changes.iter().zip(&before_board.changes) {
                    if change != before_change {
                        edits
                            .entry(stream.clone())
                            .or_default()
                            .changes
                            .push(change.clone());
                    }
                }
            }
        }
    }
    Ok(edits)
}

pub(super) fn validate_sketch_point_edits(
    native: PatchNatives<'_>,
) -> Result<BTreeMap<String, Vec<SketchPointEdit>>, CodecError> {
    let baseline_native = native.baseline;
    let target_native = native.target;
    let baseline = baseline_native
        .as_ref()
        .map(|native| &native.sketch_points[..])
        .unwrap_or_default();
    let target = target_native
        .as_ref()
        .map(|native| &native.sketch_points[..])
        .unwrap_or_default();
    let by_id = baseline
        .iter()
        .map(|point| (point.id.as_str(), point))
        .collect::<BTreeMap<_, _>>();
    if by_id
        .keys()
        .copied()
        .ne(target.iter().map(|point| point.id.as_str()))
    {
        return Err(CodecError::NotImplemented(
            "F3D sketch-point regeneration requires the unchanged point-id set".into(),
        ));
    }
    let mut edits: BTreeMap<String, Vec<SketchPointEdit>> = BTreeMap::new();
    for point in target {
        point
            .validate_for_write(
                target_native
                    .as_ref()
                    .map(|native| native.sketch_curve_identities.as_slice())
                    .unwrap_or_default(),
            )
            .map_err(CodecError::Malformed)?;
        let before = by_id[point.id.as_str()];
        let mut normalized = point.clone();
        normalized
            .try_set_coordinates(before.coordinates())
            .map_err(CodecError::Malformed)?;
        if &normalized != before {
            return Err(CodecError::NotImplemented(format!(
                "F3D sketch-point edit changes fields other than coordinates: {}",
                point.id
            )));
        }
        if point.coordinates() == before.coordinates() {
            continue;
        }
        let stream = point
            .id
            .strip_prefix(crate::ids::SCHEME_PREFIX)
            .and_then(|id| id.rsplit_once(":sketch-point#"))
            .map(|(stream, _)| stream.to_owned())
            .ok_or_else(|| {
                CodecError::malformed(format_args!("invalid sketch-point id {}", point.id))
            })?;
        edits.entry(stream).or_default().push(SketchPointEdit {
            offset: point.byte_offset,
            coordinate_offset: point.coordinate_offset,
            coordinates: point.coordinates(),
        });
    }
    Ok(edits)
}

pub(super) fn validate_sketch_curve_edits(
    native: PatchNatives<'_>,
) -> Result<BTreeMap<String, Vec<SketchCurveEdit>>, CodecError> {
    let baseline_native = native.baseline;
    let target_native = native.target;
    let baseline = baseline_native
        .as_ref()
        .map(|native| &native.sketch_curve_identities[..])
        .unwrap_or_default();
    let target = target_native
        .as_ref()
        .map(|native| &native.sketch_curve_identities[..])
        .unwrap_or_default();
    let by_id = baseline
        .iter()
        .map(|curve| (curve.id.as_str(), curve))
        .collect::<BTreeMap<_, _>>();
    if by_id
        .keys()
        .copied()
        .ne(target.iter().map(|curve| curve.id.as_str()))
    {
        return Err(CodecError::NotImplemented(
            "F3D sketch-curve regeneration requires the unchanged curve-id set".into(),
        ));
    }
    let mut edits: BTreeMap<String, Vec<SketchCurveEdit>> = BTreeMap::new();
    for curve in target {
        let before = by_id[curve.id.as_str()];
        let mut normalized = curve.clone();
        normalized.geometry.clone_from(&before.geometry);
        if &normalized != before {
            return Err(CodecError::NotImplemented(format!(
                "F3D sketch-curve edit changes fields other than geometry: {}",
                curve.id
            )));
        }
        if curve.geometry == before.geometry {
            continue;
        }
        let geometry = curve.geometry.clone().ok_or_else(|| {
            CodecError::NotImplemented(format!("cannot remove sketch-curve geometry: {}", curve.id))
        })?;
        if !same_sketch_layout(before.geometry.as_ref(), &geometry) {
            return Err(CodecError::NotImplemented(format!(
                "F3D sketch-curve edit requires valid geometry with the original native layout: {}",
                curve.id
            )));
        }
        let stream = curve
            .id
            .strip_prefix(crate::ids::SCHEME_PREFIX)
            .and_then(|id| id.rsplit_once(":sketch-curve-identity#"))
            .map(|(stream, _)| stream.to_owned())
            .ok_or_else(|| {
                CodecError::malformed(format_args!("invalid sketch-curve id {}", curve.id))
            })?;
        edits.entry(stream).or_default().push(SketchCurveEdit {
            offset: curve.byte_offset,
            geometry_offset: curve.geometry_offset,
            geometry,
        });
    }
    Ok(edits)
}

fn same_sketch_layout(before: Option<&SketchCurveGeometry>, after: &SketchCurveGeometry) -> bool {
    match (before, after) {
        (Some(SketchCurveGeometry::Line { .. }), SketchCurveGeometry::Line { .. })
        | (Some(SketchCurveGeometry::Arc { .. }), SketchCurveGeometry::Arc { .. }) => true,
        (
            Some(SketchCurveGeometry::Nurbs {
                carrier_reference: old_carrier,
                subtype_class_tag: old_tag,
                subtype_record_index: old_index,
                geometry: old_geometry,
                ..
            }),
            SketchCurveGeometry::Nurbs {
                carrier_reference,
                subtype_class_tag,
                subtype_record_index,
                geometry,
                ..
            },
        ) => {
            old_carrier == carrier_reference
                && old_tag == subtype_class_tag
                && old_index == subtype_record_index
                && old_geometry.degree() == geometry.degree()
                && old_geometry.knot_count() == geometry.knot_count()
                && old_geometry.poles().weights().len() == geometry.poles().weights().len()
                && old_geometry.poles().point_count() == geometry.poles().point_count()
        }
        _ => false,
    }
}

pub(super) fn encode_sketch_relation_state(
    relation_id: &str,
    raw_bytes: &[u8],
    state: u64,
) -> Result<Vec<u8>, CodecError> {
    match crate::design::decode::sketch::relation_mask_width(raw_bytes).ok_or_else(|| {
        CodecError::malformed(format_args!(
            "F3D sketch relation {relation_id} has no valid mask-width discriminator"
        ))
    })? {
        crate::design::decode::sketch::SketchRelationMaskWidth::U64 => {
            Ok(state.to_le_bytes().to_vec())
        }
        crate::design::decode::sketch::SketchRelationMaskWidth::U32 => u32::try_from(state)
            .map(|state| state.to_le_bytes().to_vec())
            .map_err(|_| {
                CodecError::NotImplemented(format!(
                    "F3D sketch relation {relation_id} stores a u32 mask that cannot carry the requested 64-bit state"
                ))
            }),
    }
}

pub(super) fn validate_sketch_relation_edits(
    native: PatchNatives<'_>,
) -> Result<BTreeMap<String, Vec<Vec<ByteEdit>>>, CodecError> {
    let baseline_native = native.baseline;
    let target_native = native.target;
    let baseline = baseline_native
        .as_ref()
        .map(|native| &native.sketch_relations[..])
        .unwrap_or_default();
    let target = target_native
        .as_ref()
        .map(|native| &native.sketch_relations[..])
        .unwrap_or_default();
    let by_id = baseline
        .iter()
        .map(|relation| (relation.id.as_str(), relation))
        .collect::<BTreeMap<_, _>>();
    if by_id
        .keys()
        .copied()
        .ne(target.iter().map(|relation| relation.id.as_str()))
    {
        return Err(CodecError::NotImplemented(
            "F3D sketch-relation regeneration requires the unchanged relation-id set".into(),
        ));
    }
    let mut edits: BTreeMap<String, Vec<Vec<ByteEdit>>> = BTreeMap::new();
    for relation in target {
        let before = by_id[relation.id.as_str()];
        let mut normalized = relation.clone();
        normalized.owner_reference = before.owner_reference;
        normalized
            .try_edit(|draft| {
                draft.auxiliary_references = before.auxiliary_references().clone();
                draft.members = before.members().clone();
                draft.definition = before.definition.clone();
                draft.return_members = before.return_members().clone();
            })
            .map_err(|error| CodecError::NotImplemented(error.to_string()))?;
        if &normalized != before
            || relation
                .auxiliary_references()
                .offsets()
                .ne(before.auxiliary_references().offsets())
        {
            return Err(CodecError::NotImplemented(format!(
                "F3D sketch-relation edit changes fields outside its writable references and constraint mask: {}",
                relation.id
            )));
        }
        if relation.definition.state() == before.definition.state()
            && relation.owner_reference == before.owner_reference
            && relation.auxiliary_references() == before.auxiliary_references()
            && relation.members() == before.members()
            && relation.return_members() == before.return_members()
        {
            continue;
        }
        let stream = relation
            .id
            .strip_prefix(crate::ids::SCHEME_PREFIX)
            .and_then(|id| id.rsplit_once(":sketch-relation#"))
            .map(|(stream, _)| stream.to_owned())
            .ok_or_else(|| {
                CodecError::malformed(format_args!("invalid sketch-relation id {}", relation.id))
            })?;
        let mut values = Vec::new();
        collect_sketch_reference_edits(
            relation,
            before
                .members()
                .iter()
                .map(|row| row.reference.record_index()),
            relation
                .members()
                .iter()
                .map(|row| (row.reference.record_index(), row.offset)),
            &mut values,
        )?;
        match relation.auxiliary_references().located_rows() {
            Some(after) if before.auxiliary_references().len() == after.len() => {
                values.extend(
                    before
                        .auxiliary_references()
                        .values()
                        .zip(after)
                        .filter(|(before, after)| **before != after.value)
                        .map(|(_, after)| ByteEdit {
                            offset: relation.byte_offset + u64::from(after.offset),
                            value: after.value.to_le_bytes().to_vec(),
                        }),
                );
            }
            _ => {
                return Err(CodecError::NotImplemented(format!(
                    "F3D sketch relation {} must retain reference cardinality and offsets",
                    relation.id
                )))
            }
        }
        if relation.owner_reference != before.owner_reference {
            values.push(ByteEdit {
                offset: relation.byte_offset + u64::from(relation.owner_reference_offset()),
                value: relation.owner_reference.to_le_bytes().to_vec(),
            });
        }
        collect_sketch_reference_edits(
            relation,
            before
                .return_members()
                .iter()
                .map(|row| row.reference.record_index()),
            relation
                .return_members()
                .iter()
                .map(|row| (row.reference.record_index(), row.offset)),
            &mut values,
        )?;
        if relation.definition.state() != before.definition.state() {
            let encoded = encode_sketch_relation_state(
                &relation.id,
                before.raw_bytes(),
                relation.definition.state(),
            )?;
            values.push(ByteEdit {
                offset: relation.byte_offset + u64::from(relation.state_offset),
                value: encoded,
            });
        }
        edits.entry(stream).or_default().push(values);
    }
    Ok(edits)
}

fn collect_sketch_reference_edits(
    relation: &crate::records::sketch_relations::SketchRelation,
    before: impl ExactSizeIterator<Item = u32>,
    after: impl ExactSizeIterator<Item = (u32, u32)>,
    edits: &mut Vec<ByteEdit>,
) -> Result<(), CodecError> {
    if before.len() != after.len() {
        return Err(CodecError::NotImplemented(format!(
            "F3D sketch relation {} must retain reference cardinality and offsets",
            relation.id
        )));
    }
    edits.extend(
        before
            .zip(after)
            .filter(|(before, (after, _))| before != after)
            .map(|(_, (after, offset))| ByteEdit {
                offset: relation.byte_offset + u64::from(offset),
                value: after.to_le_bytes().to_vec(),
            }),
    );
    Ok(())
}

pub(super) fn validate_body_transform_edits(
    baseline: &[Body],
    target: &[Body],
) -> Result<BTreeMap<String, Transform>, CodecError> {
    let baseline = baseline
        .iter()
        .map(|body| (body.id.as_str(), body))
        .collect::<BTreeMap<_, _>>();
    let target = target
        .iter()
        .map(|body| (body.id.as_str(), body))
        .collect::<BTreeMap<_, _>>();
    if baseline.keys().ne(target.keys()) {
        return Err(CodecError::NotImplemented(
            "F3D body-transform regeneration requires the unchanged body-id set".into(),
        ));
    }
    let mut edits = BTreeMap::new();
    for (id, before) in baseline {
        let after = target[id];
        let mut normalized = after.clone();
        normalized.transform = before.transform;
        normalized.color = before.color;
        normalized.visible = before.visible;
        if &normalized != before {
            return Err(CodecError::NotImplemented(format!(
                "F3D body edit changes fields other than transform, color, or visibility: {id}"
            )));
        }
        if after.transform == before.transform {
            continue;
        }
        let transform = after.transform.ok_or_else(|| {
            CodecError::NotImplemented(format!("cannot remove F3D body transform: {id}"))
        })?;
        if before.transform.is_none() {
            return Err(CodecError::NotImplemented(format!(
                "F3D body transform {id} must replace an existing finite affine transform"
            )));
        }
        edits.insert(id.to_owned(), transform);
    }
    Ok(edits)
}

pub(super) fn validate_body_color_edits(
    baseline: &[Body],
    target: &[Body],
) -> Result<BTreeMap<String, Color>, CodecError> {
    let baseline = baseline
        .iter()
        .map(|body| (body.id.as_str(), body))
        .collect::<BTreeMap<_, _>>();
    let target = target
        .iter()
        .map(|body| (body.id.as_str(), body))
        .collect::<BTreeMap<_, _>>();
    if baseline.keys().ne(target.keys()) {
        return Err(CodecError::NotImplemented(
            "F3D body-color regeneration requires the unchanged body-id set".into(),
        ));
    }
    let mut edits = BTreeMap::new();
    for (id, before) in baseline {
        let after = target[id];
        let mut normalized = after.clone();
        normalized.color = before.color;
        normalized.transform = before.transform;
        normalized.visible = before.visible;
        if &normalized != before {
            return Err(CodecError::NotImplemented(format!(
                "F3D body edit changes fields other than transform, color, or visibility: {id}"
            )));
        }
        if after.color == before.color {
            continue;
        }
        let color = after.color.ok_or_else(|| {
            CodecError::NotImplemented(format!("cannot remove F3D body color: {id}"))
        })?;
        if before.color.is_none() || color.a() != 1.0 {
            return Err(CodecError::NotImplemented(format!(
                "F3D body color {id} must replace an existing opaque finite RGB color"
            )));
        }
        edits.insert(id.to_owned(), color);
    }
    Ok(edits)
}

pub(super) fn validate_edge_range_edits(
    baseline: &[Edge],
    target: &[Edge],
) -> Result<BTreeMap<String, [f64; 2]>, CodecError> {
    let baseline = baseline
        .iter()
        .map(|edge| (edge.id.as_str(), edge))
        .collect::<BTreeMap<_, _>>();
    let target = target
        .iter()
        .map(|edge| (edge.id.as_str(), edge))
        .collect::<BTreeMap<_, _>>();
    if baseline.keys().ne(target.keys()) {
        return Err(CodecError::NotImplemented(
            "F3D edge-range regeneration requires the unchanged edge-id set".into(),
        ));
    }
    let mut edits = BTreeMap::new();
    for (id, before) in baseline {
        let after = target[id];
        let mut normalized = after.clone();
        normalized.carrier = cadmpeg_ir::topology::EdgeCarrier::new(
            normalized.curve().cloned(),
            before
                .param_range()
                .map(cadmpeg_ir::units::FiniteVector::get),
        )
        .map_err(CodecError::malformed)?;
        normalized.tolerance = before.tolerance;
        if &normalized != before {
            return Err(CodecError::NotImplemented(format!(
                "F3D edge edit changes fields other than parameter range: {id}"
            )));
        }
        if after.param_range() == before.param_range() {
            continue;
        }
        let range = after.param_range().ok_or_else(|| {
            CodecError::NotImplemented(format!("cannot remove F3D edge range: {id}"))
        })?;
        if before.param_range().is_none() || range[0] == range[1] {
            return Err(CodecError::malformed(format_args!(
                "edited F3D edge range {id} must replace an existing finite non-degenerate range"
            )));
        }
        edits.insert(id.to_owned(), range.get());
    }
    Ok(edits)
}

pub(super) fn validate_face_sense_edits(
    native: PatchNatives<'_>,
    baseline_ir: &CadIr,
    target_ir: &CadIr,
) -> Result<BTreeMap<String, Sense>, CodecError> {
    let baseline = &baseline_ir.model.faces;
    let target = &target_ir.model.faces;
    let native_senses = native
        .baseline
        .map(|native| {
            native
                .face_sidedness
                .iter()
                .map(|metadata| (metadata.face.clone(), metadata.carrier_flipped))
                .collect::<BTreeMap<_, _>>()
        })
        .unwrap_or_default();
    let baseline = baseline
        .iter()
        .map(|face| (face.id.as_str(), face))
        .collect::<BTreeMap<_, _>>();
    let target = target
        .iter()
        .map(|face| (face.id.as_str(), face))
        .collect::<BTreeMap<_, _>>();
    if baseline.keys().ne(target.keys()) {
        return Err(CodecError::NotImplemented(
            "F3D face-sense regeneration requires the unchanged face-id set".into(),
        ));
    }
    let mut edits = BTreeMap::new();
    for (id, before) in baseline {
        let after = target[id];
        let mut normalized = after.clone();
        normalized.sense = before.sense;
        normalized.color = before.color;
        if &normalized != before {
            return Err(CodecError::NotImplemented(format!(
                "F3D face edit changes fields other than sense: {id}"
            )));
        }
        if after.sense != before.sense {
            let carrier_flipped = native_senses.get(&before.id).copied().unwrap_or(false);
            let native_after = normalized_face_sense_to_native(after.sense, carrier_flipped);
            edits.insert(id.to_owned(), native_after);
        }
    }
    Ok(edits)
}

pub(super) fn validate_face_color_edits(
    baseline: &[Face],
    target: &[Face],
) -> Result<BTreeMap<String, Color>, CodecError> {
    let baseline = baseline
        .iter()
        .map(|face| (face.id.as_str(), face))
        .collect::<BTreeMap<_, _>>();
    let target = target
        .iter()
        .map(|face| (face.id.as_str(), face))
        .collect::<BTreeMap<_, _>>();
    if baseline.keys().ne(target.keys()) {
        return Err(CodecError::NotImplemented(
            "F3D face-color regeneration requires the unchanged face-id set".into(),
        ));
    }
    let mut edits = BTreeMap::new();
    for (id, before) in baseline {
        let after = target[id];
        let mut normalized = after.clone();
        normalized.color = before.color;
        normalized.sense = before.sense;
        if &normalized != before {
            return Err(CodecError::NotImplemented(format!(
                "F3D face edit changes fields other than sense or color: {id}"
            )));
        }
        if after.color == before.color {
            continue;
        }
        let color = after.color.ok_or_else(|| {
            CodecError::NotImplemented(format!("cannot remove F3D face color: {id}"))
        })?;
        if before.color.is_none() || color.a() != 1.0 {
            return Err(CodecError::NotImplemented(format!(
                "F3D face color {id} must replace an existing opaque finite RGB color"
            )));
        }
        edits.insert(id.to_owned(), color);
    }
    Ok(edits)
}

pub(super) fn validate_coedge_sense_edits(
    baseline: &[Coedge],
    target: &[Coedge],
) -> Result<BTreeMap<String, Sense>, CodecError> {
    let baseline = baseline
        .iter()
        .map(|coedge| (coedge.id.as_str(), coedge))
        .collect::<BTreeMap<_, _>>();
    let target = target
        .iter()
        .map(|coedge| (coedge.id.as_str(), coedge))
        .collect::<BTreeMap<_, _>>();
    if baseline.keys().ne(target.keys()) {
        return Err(CodecError::NotImplemented(
            "F3D coedge-sense regeneration requires the unchanged coedge-id set".into(),
        ));
    }
    let mut edits = BTreeMap::new();
    for (id, before) in baseline {
        let after = target[id];
        let mut normalized = after.clone();
        normalized.sense = before.sense;
        if &normalized != before {
            return Err(CodecError::NotImplemented(format!(
                "F3D coedge edit changes fields other than sense: {id}"
            )));
        }
        if after.sense != before.sense {
            edits.insert(id.to_owned(), after.sense);
        }
    }
    Ok(edits)
}

pub(super) fn validate_curve_edits(
    baseline: &[Curve],
    target: &[Curve],
) -> Result<std::collections::BTreeSet<String>, CodecError> {
    let baseline = baseline
        .iter()
        .map(|curve| (curve.id.as_str(), curve.geometry.solved()))
        .collect::<BTreeMap<_, _>>();
    let target = target
        .iter()
        .map(|curve| (curve.id.as_str(), curve.geometry.solved()))
        .collect::<BTreeMap<_, _>>();
    if baseline.keys().ne(target.keys()) {
        return Err(CodecError::NotImplemented(
            "F3D curve regeneration requires the unchanged curve-id set".into(),
        ));
    }
    let mut edited = std::collections::BTreeSet::new();
    for (id, before) in baseline {
        let after = target[id];
        if before == after {
            continue;
        }
        edited.insert(id.to_owned());
        // A curve-kind change is refused here, once, so the arms below decide
        // the edit on their own bodies and carry no policy in their order.
        if before.map(std::mem::discriminant) != after.map(std::mem::discriminant) {
            return Err(CodecError::NotImplemented(
                if matches!(after, Some(SolvedCurveGeometry::Nurbs(_))) {
                    format!("F3D regeneration cannot change curve {id} into a NURBS carrier")
                } else {
                    format!("F3D regeneration does not support edits to curve {id}")
                },
            ));
        }
        match (before, after) {
            // A circle and an ellipse hold their two directions in one
            // `OrthonormalFrame3`, which admits them, so a same-kind edit to
            // either carries no condition this writer can still refuse.
            (
                _,
                Some(
                    SolvedCurveGeometry::Line(_)
                    | SolvedCurveGeometry::Degenerate(_)
                    | SolvedCurveGeometry::Circle(_)
                    | SolvedCurveGeometry::Ellipse(_),
                ),
            ) => {}
            (Some(SolvedCurveGeometry::Nurbs(before)), Some(SolvedCurveGeometry::Nurbs(after))) => {
                // `NurbsCurve` admission states finite poles and finite
                // nonzero weights, and the record states each weight as a
                // plain little-endian f64 payload, so the weight sign carries
                // no condition. A degree outside 1..=20 is malformed: no
                // spline layout reader admits such a record, so no f3d
                // document holds one. The rest are edits an f3d document can
                // state and this writer does not write: a carrier it does not
                // address, a changed distinct-knot count, and a changed
                // rational form or pole count.
                if !(id.starts_with("f3d:brep:entity#")
                    || id.starts_with("f3d:brep:tolerant-coedge-curve#")
                    || (id.starts_with("f3d:brep:procedural_surface#")
                        && (id.ends_with(":directrix") || id.ends_with(":spine"))))
                {
                    return Err(CodecError::NotImplemented(format!(
                        "edited F3D curve {id} is not a patchable NURBS carrier"
                    )));
                }
                if !writable_nurbs_degree(after.degree()) {
                    return Err(CodecError::malformed(format_args!(
                        "edited F3D curve {id} has a NURBS degree outside [1, 20]"
                    )));
                }
                if !unchanged_unique_knot_count(before.knots(), after.knots()) {
                    return Err(CodecError::NotImplemented(format!(
                        "edited F3D curve {id} changes the NURBS knot layout"
                    )));
                }
                if before.weights().is_some() != after.weights().is_some() {
                    return Err(CodecError::NotImplemented(format!(
                        "edited F3D curve {id} changes the NURBS rational form"
                    )));
                }
                if before.pole_count() != after.pole_count() {
                    return Err(CodecError::NotImplemented(format!(
                        "edited F3D curve {id} changes the NURBS control-point count"
                    )));
                }
            }
            _ => {
                return Err(CodecError::NotImplemented(format!(
                    "F3D regeneration does not support edits to curve {id}"
                )));
            }
        }
    }
    Ok(edited)
}

pub(super) fn validate_pcurve_edits(
    baseline_model: &Model,
    target_model: &Model,
) -> Result<BTreeMap<String, PcurveEdit>, CodecError> {
    let baseline = baseline_model
        .pcurves
        .iter()
        .map(|pcurve| (pcurve.id.as_str(), pcurve))
        .collect::<BTreeMap<_, _>>();
    let target = target_model
        .pcurves
        .iter()
        .map(|pcurve| (pcurve.id.as_str(), pcurve))
        .collect::<BTreeMap<_, _>>();
    if baseline.keys().ne(target.keys()) {
        return Err(CodecError::NotImplemented(
            "F3D pcurve regeneration requires the unchanged pcurve-id set".into(),
        ));
    }
    let mut edits = BTreeMap::new();
    for (id, before) in baseline {
        let after = target[id];
        let before_native = native_support_pcurve(
            pcurve_support_geometry(baseline_model, &before.id)?,
            &before.geometry,
        )?;
        let after_native = native_support_pcurve(
            pcurve_support_geometry(target_model, &after.id)?,
            &after.geometry,
        )?;
        if before == after && before_native == after_native {
            continue;
        }
        let (
            PcurveGeometry::Nurbs {
                nurbs: before_nurbs,
            },
            PcurveGeometry::Nurbs { nurbs: after_nurbs },
        ) = (&before.geometry, &after.geometry)
        else {
            return Err(CodecError::NotImplemented(format!(
                "F3D regeneration does not support this pcurve edit: {id}"
            )));
        };
        // `PcurveNurbs::new` and `PcurveNurbs::edit_control_points` both
        // require finite poles, so pole finiteness is not a condition this
        // chain can refuse. The `WeightedPole2::weight` field is a
        // `NonZeroReal`, and the patcher writes the weight as a plain
        // little-endian `f64` payload, so the weight value is not one either.
        let valid = id.starts_with("f3d:brep:entity#")
            && writable_nurbs_degree(after_nurbs.degree())
            && unchanged_unique_knot_count(before_nurbs.knots(), after_nurbs.knots())
            && before_nurbs.control_points().len() == after_nurbs.control_points().len()
            && before_nurbs.weights().is_some() == after_nurbs.weights().is_some();
        let contract_valid = before.wrapper_reversed().is_some()
            == after.wrapper_reversed().is_some()
            && before.native_tail_flags().is_some() == after.native_tail_flags().is_some()
            && before.uv_offset().is_some() == after.uv_offset().is_some()
            && before.parameter_range() == after.parameter_range()
            && before.fit_tolerance().is_some() == after.fit_tolerance().is_some();
        if !valid || !contract_valid {
            return Err(CodecError::NotImplemented(format!(
                "F3D pcurve edit changes fixed cache structure: {id}"
            )));
        }
        let periodic =
            (before_nurbs.periodic() != after_nurbs.periodic()).then_some(after_nurbs.periodic());
        let uv_offset = (before.uv_offset() != after.uv_offset())
            .then_some(after.uv_offset())
            .flatten();
        let support = pcurve_support_surface(target_model, &after.id)?
            .as_str()
            .to_owned();
        let edit = match &before.metadata {
            cadmpeg_ir::geometry::pcurve::PcurveMetadata::General { form: metadata }
                if metadata.wrapper_reversed.is_none() && metadata.fit_tolerance().is_none() =>
            {
                PcurveEdit::Ref {
                    support,
                    native_geometry: after_native,
                    periodic,
                    uv_offset: uv_offset.map(cadmpeg_ir::units::FiniteVector::get),
                    stored_uv_offset: after
                        .uv_offset()
                        .map_or([0.0, 0.0], cadmpeg_ir::units::FiniteVector::get),
                }
            }
            cadmpeg_ir::geometry::pcurve::PcurveMetadata::AsmInline { .. }
            | cadmpeg_ir::geometry::pcurve::PcurveMetadata::General { .. } => PcurveEdit::Inline {
                support,
                native_geometry: after_native,
                periodic,
                wrapper_reversed: (before.wrapper_reversed() != after.wrapper_reversed())
                    .then_some(after.wrapper_reversed())
                    .flatten(),
                native_tail_flags: (before.native_tail_flags() != after.native_tail_flags())
                    .then_some(after.native_tail_flags())
                    .flatten(),
                uv_offset: uv_offset.map(cadmpeg_ir::units::FiniteVector::get),
                stored_uv_offset: after
                    .uv_offset()
                    .map_or([0.0, 0.0], cadmpeg_ir::units::FiniteVector::get),
                fit_tolerance: (before.fit_tolerance() != after.fit_tolerance())
                    .then_some(after.fit_tolerance())
                    .flatten()
                    .map(cadmpeg_ir::geometry::FitTolerance::get),
            },
        };
        edits.insert(id.to_owned(), edit);
    }
    Ok(edits)
}

pub(super) fn validate_surface_edits(
    baseline: &[Surface],
    target: &[Surface],
) -> Result<std::collections::BTreeSet<String>, CodecError> {
    let baseline = baseline
        .iter()
        .map(|surface| (surface.id.as_str(), surface.geometry.solved()))
        .collect::<BTreeMap<_, _>>();
    let target = target
        .iter()
        .map(|surface| (surface.id.as_str(), surface.geometry.solved()))
        .collect::<BTreeMap<_, _>>();
    if baseline.keys().ne(target.keys()) {
        return Err(CodecError::NotImplemented(
            "F3D surface regeneration requires the unchanged surface-id set".into(),
        ));
    }
    let mut edited = std::collections::BTreeSet::new();
    for (id, before) in baseline {
        let after = target[id];
        if before == after {
            continue;
        }
        // A surface-kind change is refused here, once, so the arms below decide
        // the edit on their own bodies and carry no policy in their order.
        if before.map(std::mem::discriminant) != after.map(std::mem::discriminant) {
            return Err(CodecError::NotImplemented(
                if matches!(after, Some(SolvedSurfaceGeometry::Nurbs(_))) {
                    format!("F3D regeneration cannot change surface {id} into a NURBS carrier")
                } else {
                    format!("F3D regeneration does not support edits to surface {id}")
                },
            ));
        }
        match (before, after) {
            // A plane, sphere, torus and cylinder hold their two directions in
            // one `OrthonormalFrame3`, which admits them, so a same-kind edit
            // to any of them carries no condition this writer can still refuse.
            (
                _,
                Some(
                    SolvedSurfaceGeometry::Plane(_)
                    | SolvedSurfaceGeometry::Sphere(_)
                    | SolvedSurfaceGeometry::Torus(_)
                    | SolvedSurfaceGeometry::Cylinder(_),
                ),
            ) => {}
            (_, Some(SolvedSurfaceGeometry::Cone(cone_surface))) => {
                // `NonNegativeLength` admits zero and `Angle` admits every
                // finite angle, so the cone carrier states neither condition.
                // The record holds the reference direction scaled by the
                // radius, which a zero radius erases. The record states the
                // half angle as a sine and a cosine whose signs carry the axis
                // and normal senses, and the ASM surface reader rebuilds the
                // angle as `sine.abs().atan2(cosine.abs())`, whose range is
                // [0, pi/2]. The patcher writes the sine and the cosine of the
                // half angle, and `atan2(1.0, cos(pi/2))` is exactly `pi/2`, so
                // the closed endpoint survives the round trip. A negative half
                // angle and one above pi/2 are shapes no f3d document holds.
                if cone_surface.radius().get() == 0.0 {
                    return Err(CodecError::malformed(format_args!(
                        "edited F3D cone surface {id} has a zero cross-section radius"
                    )));
                }
                if !(0.0..=std::f64::consts::FRAC_PI_2).contains(&cone_surface.half_angle().get()) {
                    return Err(CodecError::malformed(format_args!(
                        "edited F3D cone surface {id} has a half angle outside [0, pi/2]"
                    )));
                }
            }
            (
                Some(SolvedSurfaceGeometry::Nurbs(before)),
                Some(SolvedSurfaceGeometry::Nurbs(after)),
            ) => {
                // `NurbsSurface` admission states finite poles and finite
                // nonzero weights, and the record states each weight as a
                // plain little-endian f64 payload, so the weight sign carries
                // no condition. A u or v degree outside 1..=20 is malformed:
                // no spline layout reader admits such a record, so no f3d
                // document holds one. The rest are edits an f3d document can
                // state and this writer does not write: a carrier it does not
                // address, a changed distinct-knot count, and a changed
                // rational form or pole count.
                if !(id.starts_with("f3d:brep:entity#")
                    || (id.starts_with("f3d:brep:procedural_surface#")
                        && (id.ends_with(":support0") || id.ends_with(":support1"))))
                {
                    return Err(CodecError::NotImplemented(format!(
                        "edited F3D surface {id} is not a patchable NURBS carrier"
                    )));
                }
                if !writable_nurbs_degree(after.u_degree()) {
                    return Err(CodecError::malformed(format_args!(
                        "edited F3D surface {id} has a NURBS u degree outside [1, 20]"
                    )));
                }
                if !unchanged_unique_knot_count(before.u_knots(), after.u_knots()) {
                    return Err(CodecError::NotImplemented(format!(
                        "edited F3D surface {id} changes the NURBS u knot layout"
                    )));
                }
                if !writable_nurbs_degree(after.v_degree()) {
                    return Err(CodecError::malformed(format_args!(
                        "edited F3D surface {id} has a NURBS v degree outside [1, 20]"
                    )));
                }
                if !unchanged_unique_knot_count(before.v_knots(), after.v_knots()) {
                    return Err(CodecError::NotImplemented(format!(
                        "edited F3D surface {id} changes the NURBS v knot layout"
                    )));
                }
                if before.u_count() != after.u_count() {
                    return Err(CodecError::NotImplemented(format!(
                        "edited F3D surface {id} changes the NURBS u control-point count"
                    )));
                }
                if before.v_count() != after.v_count() {
                    return Err(CodecError::NotImplemented(format!(
                        "edited F3D surface {id} changes the NURBS v control-point count"
                    )));
                }
                if before.weights().is_some() != after.weights().is_some() {
                    return Err(CodecError::NotImplemented(format!(
                        "edited F3D surface {id} changes the NURBS rational form"
                    )));
                }
            }
            _ => {
                return Err(CodecError::NotImplemented(format!(
                    "F3D regeneration does not support edits to surface {id}"
                )));
            }
        }
        edited.insert(id.to_owned());
    }
    Ok(edited)
}

pub(super) fn validate_procedural_surface_edits(
    baseline: &CadIr,
    target: &CadIr,
) -> Result<BTreeMap<String, ProceduralSurfaceEdit>, CodecError> {
    let baseline = baseline
        .model
        .procedural_surfaces
        .iter()
        .map(|surface| (surface.id.as_str(), surface))
        .collect::<BTreeMap<_, _>>();
    let target = target
        .model
        .procedural_surfaces
        .iter()
        .map(|surface| (surface.id.as_str(), surface))
        .collect::<BTreeMap<_, _>>();
    if baseline.keys().ne(target.keys()) {
        return Err(CodecError::NotImplemented(
            "F3D procedural-surface regeneration requires the unchanged construction-id set".into(),
        ));
    }
    let mut edits = BTreeMap::new();
    for (id, before) in baseline {
        let after = target[id];
        if after == before {
            continue;
        }
        let edit = match (before.definition(), after.definition()) {
            (
                ProceduralSurfaceDefinition::Extrusion(definition_payload_0),
                ProceduralSurfaceDefinition::Extrusion(definition_payload_1),
            ) if (definition_payload_0.directrix()) == (definition_payload_1.directrix())
                && definition_payload_0.revision_form().is_none()
                && definition_payload_1.revision_form().is_none() =>
            {
                let before_parameter_interval = definition_payload_0.parameter_interval();
                let before_direction = definition_payload_0.direction();
                let before_native_position = definition_payload_0.native_position();
                let after_parameter_interval = definition_payload_1.parameter_interval();
                let after_direction = definition_payload_1.direction();
                let after_native_position = definition_payload_1.native_position();
                {
                    let interval = after_parameter_interval.ok_or_else(|| {
                        CodecError::malformed(format_args!(
                            "F3D extrusion interval is missing: {id}"
                        ))
                    })?;
                    let position = after_native_position.ok_or_else(|| {
                        CodecError::malformed(format_args!(
                            "F3D extrusion native position is missing: {id}"
                        ))
                    })?;
                    if interval[0] >= interval[1] {
                        return Err(CodecError::malformed(format_args!(
                            "F3D extrusion interval must be strictly increasing: {id}"
                        )));
                    }
                    if after_direction.norm() == 0.0 {
                        return Err(CodecError::malformed(format_args!(
                            "F3D extrusion direction must be nonzero: {id}"
                        )));
                    }
                    (before_parameter_interval != after_parameter_interval
                        || before_direction != after_direction
                        || before_native_position != after_native_position)
                        .then_some(ProceduralSurfaceEdit::Extrusion {
                            parameter_interval: interval.get(),
                            direction: after_direction.get(),
                            native_position: position.get(),
                        })
                }
            }
            (
                ProceduralSurfaceDefinition::Blend(before_payload),
                ProceduralSurfaceDefinition::Blend(after_payload),
            ) if before_payload.supports() == after_payload.supports()
                && before_payload.spine() == after_payload.spine()
                && before_payload.cross_section() == after_payload.cross_section()
                && before_payload.native() == after_payload.native() =>
            {
                let values = match after_payload.radius() {
                    BlendRadiusLaw::Constant { signed_radius } => [signed_radius.get(); 2],
                    BlendRadiusLaw::Linear { start, end } => [start.get(), end.get()],
                    BlendRadiusLaw::Law { .. } => {
                        return Err(CodecError::NotImplemented(format!(
                            "F3D explicit blend-law regeneration is unsupported: {id}"
                        )));
                    }
                };
                if values.contains(&0.0) {
                    return Err(CodecError::malformed(format_args!(
                        "F3D rolling-ball radii must be nonzero: {id}"
                    )));
                }
                (before_payload.radius() != after_payload.radius())
                    .then_some(ProceduralSurfaceEdit::BlendRadii(values))
            }
            _ => {
                return Err(CodecError::NotImplemented(format!(
                    "F3D procedural-surface edit changes non-writable construction fields: {id}"
                )));
            }
        };
        if let Some(edit) = edit {
            edits.insert(id.to_owned(), edit);
        }
    }
    Ok(edits)
}

pub(super) fn validate_procedural_surface_fit_edits(
    baseline: &CadIr,
    target: &CadIr,
) -> Result<BTreeMap<String, f64>, CodecError> {
    let baseline = baseline
        .model
        .procedural_surfaces
        .iter()
        .map(|surface| (surface.id.as_str(), surface))
        .collect::<BTreeMap<_, _>>();
    let target = target
        .model
        .procedural_surfaces
        .iter()
        .map(|surface| (surface.id.as_str(), surface))
        .collect::<BTreeMap<_, _>>();
    let mut edits = BTreeMap::new();
    for (id, before) in baseline {
        let after = target[id];
        if after.cache_fit_tolerance() == before.cache_fit_tolerance() {
            continue;
        }
        let tolerance = after.cache_fit_tolerance().ok_or_else(|| {
            CodecError::NotImplemented(format!(
                "cannot remove F3D procedural-surface fit tolerance: {id}"
            ))
        })?;
        if before.cache_fit_tolerance().is_none() {
            return Err(CodecError::malformed(format_args!(
                "F3D procedural-surface fit tolerance must replace a finite nonnegative value: {id}"
            )));
        }
        edits.insert(id.to_owned(), tolerance.get());
    }
    Ok(edits)
}

pub(super) fn validate_procedural_curve_edits(
    baseline: &[ProceduralCurve],
    target: &[ProceduralCurve],
) -> Result<BTreeMap<String, ProceduralCurveEdit>, CodecError> {
    let baseline = baseline
        .iter()
        .map(|curve| (curve.id.as_str(), curve))
        .collect::<BTreeMap<_, _>>();
    let target = target
        .iter()
        .map(|curve| (curve.id.as_str(), curve))
        .collect::<BTreeMap<_, _>>();
    if baseline.keys().ne(target.keys()) {
        return Err(CodecError::NotImplemented(
            "F3D procedural-curve regeneration requires the unchanged construction-id set".into(),
        ));
    }
    let mut edits = BTreeMap::new();
    for (id, before) in baseline {
        let after = target[id];
        // The solved-cache contract travels inside the definition, and the
        // fit tolerance is written through its own edit, so the definition
        // comparison normalises that contract out of both sides.
        let before_definition = definition_without_legacy_cache(before.definition());
        let after_definition = definition_without_legacy_cache(after.definition());
        let definition = match (&before_definition, &after_definition) {
            (
                cadmpeg_ir::geometry::ProceduralCurveDefinition::Helix(_),
                cadmpeg_ir::geometry::ProceduralCurveDefinition::Helix(_),
            ) if before_definition != after_definition => Some(after_definition.clone()),
            (
                cadmpeg_ir::geometry::ProceduralCurveDefinition::VectorOffset(definition_payload_0),
                cadmpeg_ir::geometry::ProceduralCurveDefinition::VectorOffset(definition_payload_1),
            ) if (definition_payload_0.source()) == (definition_payload_1.source())
                && (definition_payload_0.roles()) == (definition_payload_1.roles())
                && before_definition != after_definition =>
            {
                Some(after_definition.clone())
            }
            (
                cadmpeg_ir::geometry::ProceduralCurveDefinition::Subset(definition_payload_0),
                cadmpeg_ir::geometry::ProceduralCurveDefinition::Subset(definition_payload_1),
            ) if (definition_payload_0.source()) == (definition_payload_1.source())
                && before_definition != after_definition =>
            {
                Some(after_definition.clone())
            }
            (
                cadmpeg_ir::geometry::ProceduralCurveDefinition::TwoSidedOffset(
                    definition_payload_0,
                ),
                cadmpeg_ir::geometry::ProceduralCurveDefinition::TwoSidedOffset(
                    definition_payload_1,
                ),
            ) if (definition_payload_0.context()).sides()
                == (definition_payload_1.context()).sides()
                && (definition_payload_0.context())
                    .discontinuities()
                    .iter()
                    .map(Vec::len)
                    .eq((definition_payload_1.context())
                        .discontinuities()
                        .iter()
                        .map(Vec::len))
                && before_definition != after_definition =>
            {
                Some(after_definition.clone())
            }
            (
                cadmpeg_ir::geometry::ProceduralCurveDefinition::SurfaceOffset(
                    definition_payload_0,
                ),
                cadmpeg_ir::geometry::ProceduralCurveDefinition::SurfaceOffset(
                    definition_payload_1,
                ),
            ) if (definition_payload_0.context()).sides()
                == (definition_payload_1.context()).sides()
                && (definition_payload_0.context())
                    .discontinuities()
                    .iter()
                    .map(Vec::len)
                    .eq((definition_payload_1.context())
                        .discontinuities()
                        .iter()
                        .map(Vec::len))
                && (definition_payload_0.base()) == (definition_payload_1.base())
                && before_definition != after_definition =>
            {
                Some(after_definition.clone())
            }
            (
                cadmpeg_ir::geometry::ProceduralCurveDefinition::Spring(before_payload),
                cadmpeg_ir::geometry::ProceduralCurveDefinition::Spring(after_payload),
            ) if spring_patch_shape_agrees(before_payload.layout(), after_payload.layout())
                && before_definition != after_definition =>
            {
                Some(after_definition.clone())
            }
            (
                cadmpeg_ir::geometry::ProceduralCurveDefinition::Projection(before_payload),
                cadmpeg_ir::geometry::ProceduralCurveDefinition::Projection(after_payload),
            ) if before_payload.context().sides() == after_payload.context().sides()
                && before_payload
                    .context()
                    .discontinuities()
                    .iter()
                    .map(Vec::len)
                    .eq(after_payload
                        .context()
                        .discontinuities()
                        .iter()
                        .map(Vec::len))
                && before_payload.source() == after_payload.source()
                && matches!(
                    (before_payload.tail(), after_payload.tail()),
                    (
                        cadmpeg_ir::geometry::ProjectionTail::EarlyClose { .. },
                        cadmpeg_ir::geometry::ProjectionTail::EarlyClose { .. },
                    ) | (
                        cadmpeg_ir::geometry::ProjectionTail::Ranged { .. },
                        cadmpeg_ir::geometry::ProjectionTail::Ranged { .. },
                    )
                )
                && before_definition != after_definition =>
            {
                Some(after_definition.clone())
            }
            (
                cadmpeg_ir::geometry::ProceduralCurveDefinition::Intersection {
                    context: before_context,
                    ..
                },
                cadmpeg_ir::geometry::ProceduralCurveDefinition::Intersection {
                    context: after_context,
                    ..
                },
            ) if before_context.sides() == after_context.sides()
                && before_context
                    .discontinuities()
                    .iter()
                    .map(Vec::len)
                    .eq(after_context.discontinuities().iter().map(Vec::len))
                && before_definition != after_definition =>
            {
                Some(after_definition.clone())
            }
            (
                cadmpeg_ir::geometry::ProceduralCurveDefinition::ThreeSurfaceIntersection(
                    before_payload,
                ),
                cadmpeg_ir::geometry::ProceduralCurveDefinition::ThreeSurfaceIntersection(
                    after_payload,
                ),
            ) if before_payload.context().sides() == after_payload.context().sides()
                && before_payload
                    .context()
                    .discontinuities()
                    .iter()
                    .map(Vec::len)
                    .eq(after_payload
                        .context()
                        .discontinuities()
                        .iter()
                        .map(Vec::len))
                && before_payload.third() == after_payload.third()
                && before_definition != after_definition =>
            {
                Some(after_definition.clone())
            }
            (
                cadmpeg_ir::geometry::ProceduralCurveDefinition::SurfaceCurve {
                    family: before_family,
                },
                cadmpeg_ir::geometry::ProceduralCurveDefinition::SurfaceCurve {
                    family: after_family,
                },
            ) if before_family.has_same_form(after_family)
                && before_family.context().sides() == after_family.context().sides()
                && before_family
                    .context()
                    .discontinuities()
                    .iter()
                    .map(Vec::len)
                    .eq(after_family
                        .context()
                        .discontinuities()
                        .iter()
                        .map(Vec::len))
                && before_definition != after_definition =>
            {
                Some(after_definition.clone())
            }
            (
                cadmpeg_ir::geometry::ProceduralCurveDefinition::Silhouette(before_payload),
                cadmpeg_ir::geometry::ProceduralCurveDefinition::Silhouette(after_payload),
            ) if before_payload.context() == after_payload.context()
                && std::mem::discriminant(before_payload.silhouette())
                    == std::mem::discriminant(after_payload.silhouette())
                && before_payload.cast_surface() == after_payload.cast_surface()
                && before_definition != after_definition =>
            {
                Some(after_definition.clone())
            }
            (
                cadmpeg_ir::geometry::ProceduralCurveDefinition::Compound(before_compound),
                cadmpeg_ir::geometry::ProceduralCurveDefinition::Compound(after_compound),
            ) if before_compound
                .components()
                .iter()
                .map(|item| &item.component)
                .eq(after_compound
                    .components()
                    .iter()
                    .map(|item| &item.component))
                && before_definition != after_definition =>
            {
                Some(after_definition.clone())
            }
            (before, after) if before == after => None,
            _ => {
                return Err(CodecError::NotImplemented(format!(
                    "F3D procedural-curve edit changes a non-writable definition: {id}"
                )))
            }
        };
        let fit_tolerance = if after.cache_fit_tolerance() == before.cache_fit_tolerance() {
            None
        } else {
            let tolerance = after.cache_fit_tolerance().ok_or_else(|| {
                CodecError::NotImplemented(format!(
                    "cannot remove F3D procedural-curve fit tolerance: {id}"
                ))
            })?;
            if before.cache_fit_tolerance().is_none() {
                return Err(CodecError::malformed(format_args!(
                    "F3D procedural-curve fit tolerance must replace a finite nonnegative value: {id}"
                )));
            }
            Some(tolerance)
        };
        if let Some(edit) = ProceduralCurveEdit::new(
            definition,
            fit_tolerance.map(cadmpeg_ir::geometry::FitTolerance::get),
        ) {
            edits.insert(id.to_owned(), edit);
        }
    }
    Ok(edits)
}

/// The definition with the solved-cache contract it states cleared, so two
/// definitions compare on their construction fields alone.
fn definition_without_legacy_cache(
    definition: &cadmpeg_ir::geometry::ProceduralCurveDefinition,
) -> cadmpeg_ir::geometry::ProceduralCurveDefinition {
    let mut cleared = definition.clone();
    cleared.clear_legacy_cache();
    cleared
}

fn spring_patch_shape_agrees(
    before: &cadmpeg_ir::geometry::SpringLayout<
        cadmpeg_ir::scalar::FiniteReal,
        cadmpeg_ir::topology::ParameterInterval,
    >,
    after: &cadmpeg_ir::geometry::SpringLayout<
        cadmpeg_ir::scalar::FiniteReal,
        cadmpeg_ir::topology::ParameterInterval,
    >,
) -> bool {
    match (before, after) {
        (
            cadmpeg_ir::geometry::SpringLayout::ContextFirst {
                supports: before_supports,
                first_pcurve: before_first,
                second_pcurve: before_second,
                discontinuities: before_discontinuities,
                ..
            },
            cadmpeg_ir::geometry::SpringLayout::ContextFirst {
                supports: after_supports,
                first_pcurve: after_first,
                second_pcurve: after_second,
                discontinuities: after_discontinuities,
                ..
            },
        ) => {
            before_supports == after_supports
                && before_first == after_first
                && before_second == after_second
                && before_discontinuities
                    .iter()
                    .map(Vec::len)
                    .eq(after_discontinuities.iter().map(Vec::len))
        }
        (
            cadmpeg_ir::geometry::SpringLayout::CacheFirst {
                context: before_context,
                ..
            },
            cadmpeg_ir::geometry::SpringLayout::CacheFirst {
                context: after_context,
                ..
            },
        ) => {
            before_context.sides() == after_context.sides()
                && before_context
                    .discontinuities()
                    .iter()
                    .map(Vec::len)
                    .eq(after_context.discontinuities().iter().map(Vec::len))
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    mod timestamps;
    mod zstd_members;

    use super::{
        validate_curve_edits, validate_material_assignment_edits, validate_surface_edits,
        PatchNatives,
    };
    use crate::native::F3dNative;
    use crate::records::references::DesignMaterialAssignment;
    use cadmpeg_ir::geometry::analytic::{CircleCurve, ConeSurface, LineCurve};
    use cadmpeg_ir::geometry::nurbs::{
        NurbsCurve, NurbsSurface, NurbsSurfaceAxis, NurbsSurfaceLanes,
    };
    use cadmpeg_ir::geometry::{
        Curve, CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, Surface, SurfaceGeometry,
    };
    use cadmpeg_ir::ids::{CurveId, SurfaceId};
    use cadmpeg_ir::math::{Point3, Vector3};

    fn curve(id: &str, geometry: SolvedCurveGeometry) -> Vec<Curve> {
        vec![Curve {
            parameter_range: None,
            id: CurveId::mint(id).expect("identity grammar"),
            geometry: CurveGeometry::Solved(geometry),
            source_object: None,
        }]
    }

    fn surface(id: &str, geometry: SolvedSurfaceGeometry) -> Vec<Surface> {
        vec![Surface {
            id: SurfaceId::mint(id).expect("identity grammar"),
            geometry: SurfaceGeometry::Solved(geometry),
            source_object: None,
        }]
    }

    fn cone_surface(radius: f64, half_angle: f64) -> Vec<Surface> {
        surface(
            "f3d:brep:entity#9",
            SolvedSurfaceGeometry::Cone(
                ConeSurface::try_new(
                    Point3::new(0.0, 0.0, 0.0),
                    Vector3::new(0.0, 0.0, 1.0),
                    Vector3::new(1.0, 0.0, 0.0),
                    radius,
                    1.0,
                    half_angle,
                )
                .expect("orthonormal frame, nonnegative radius and finite half angle"),
            ),
        )
    }

    /// The first `count` poles of a fixed degree-1 control polygon.
    fn poles(count: usize) -> Vec<Point3> {
        [
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(1.0, 0.0, 0.0),
            Point3::new(2.0, 0.0, 0.0),
        ]
        .get(..count)
        .expect("at most three poles")
        .to_vec()
    }

    fn nurbs_curve(
        knots: Vec<f64>,
        pole_count: usize,
        weights: Option<Vec<f64>>,
    ) -> SolvedCurveGeometry {
        SolvedCurveGeometry::Nurbs(
            NurbsCurve::from_lanes(1, knots, poles(pole_count), weights, false)
                .expect("degree, knots and control points agree"),
        )
    }

    fn nurbs_surface(
        u_knots: Vec<f64>,
        v_knots: Vec<f64>,
        u_count: usize,
        v_count: usize,
        weights: Option<Vec<Vec<f64>>>,
    ) -> SolvedSurfaceGeometry {
        SolvedSurfaceGeometry::Nurbs(
            NurbsSurface::from_lanes(
                NurbsSurfaceAxis::new(1, u_knots, false),
                NurbsSurfaceAxis::new(1, v_knots, false),
                NurbsSurfaceLanes::new((0..u_count).map(|_| poles(v_count)).collect(), weights),
                false,
            )
            .expect("degrees, knots and control grid agree"),
        )
    }

    /// An all-ones weight grid whose last row's first pole carries `last`.
    fn weight_grid(u_count: usize, v_count: usize, last: f64) -> Vec<Vec<f64>> {
        let mut rows = (0..u_count)
            .map(|_| (0..v_count).map(|_| 1.0).collect::<Vec<f64>>())
            .collect::<Vec<_>>();
        if let Some(row) = rows.last_mut() {
            if let Some(weight) = row.first_mut() {
                *weight = last;
            }
        }
        rows
    }

    #[test]
    fn a_curve_kind_change_is_refused_with_the_message_of_the_kind_it_becomes() {
        let baseline = curve(
            "f3d:brep:entity#7",
            SolvedCurveGeometry::Circle(
                CircleCurve::try_new(
                    Point3::new(0.0, 0.0, 0.0),
                    Vector3::new(0.0, 0.0, 1.0),
                    Vector3::new(1.0, 0.0, 0.0),
                    2.0,
                )
                .expect("orthonormal frame and positive radius"),
            ),
        );
        let line = curve(
            "f3d:brep:entity#7",
            SolvedCurveGeometry::Line(
                LineCurve::try_new(Point3::new(0.0, 0.0, 0.0), Vector3::new(1.0, 0.0, 0.0))
                    .expect("finite origin and nonzero direction"),
            ),
        );
        let error = validate_curve_edits(&baseline, &line)
            .expect_err("a circle carrier cannot become a line");
        assert!(
            error
                .to_string()
                .contains("does not support edits to curve f3d:brep:entity#7"),
            "{error}"
        );

        let nurbs = curve(
            "f3d:brep:entity#7",
            nurbs_curve(vec![0.0, 0.0, 1.0, 1.0], 2, None),
        );
        let error = validate_curve_edits(&baseline, &nurbs)
            .expect_err("a circle carrier cannot become a NURBS carrier");
        assert!(
            error
                .to_string()
                .contains("cannot change curve f3d:brep:entity#7 into a NURBS carrier"),
            "{error}"
        );
    }

    #[test]
    fn a_same_kind_nurbs_curve_edit_that_changes_the_knot_multiplicity_is_not_implemented() {
        let nurbs = |knots: Vec<f64>| curve("f3d:brep:entity#7", nurbs_curve(knots, 2, None));
        let error = validate_curve_edits(
            &nurbs(vec![0.0, 0.0, 1.0, 1.0]),
            &nurbs(vec![0.0, 0.5, 1.0, 1.0]),
        )
        .expect_err("a NURBS carrier keeps its knot multiplicities");
        assert!(
            error
                .to_string()
                .contains("edited F3D curve f3d:brep:entity#7 changes the NURBS knot layout"),
            "{error}"
        );
    }

    #[test]
    fn a_same_kind_nurbs_curve_edit_that_leaves_the_writable_degree_range_is_malformed() {
        let degree_zero = SolvedCurveGeometry::Nurbs(
            NurbsCurve::from_lanes(0, vec![0.0, 0.0, 1.0], poles(2), None, false)
                .expect("degree, knots and control points agree"),
        );
        let error = validate_curve_edits(
            &curve(
                "f3d:brep:entity#7",
                nurbs_curve(vec![0.0, 0.0, 1.0, 1.0], 2, None),
            ),
            &curve("f3d:brep:entity#7", degree_zero),
        )
        .expect_err("a spline record states a degree in 1..=20");
        assert!(
            error
                .to_string()
                .contains("edited F3D curve f3d:brep:entity#7 has a NURBS degree outside [1, 20]"),
            "{error}"
        );
    }

    #[test]
    fn a_nurbs_curve_edit_on_a_carrier_the_patcher_does_not_address_is_not_implemented() {
        let id = "f3d:brep:procedural_surface#7:profile";
        let error = validate_curve_edits(
            &curve(id, nurbs_curve(vec![0.0, 0.0, 1.0, 1.0], 2, None)),
            &curve(id, nurbs_curve(vec![0.0, 0.0, 2.0, 2.0], 2, None)),
        )
        .expect_err("only the addressed NURBS carriers are patchable");
        assert!(
            error
                .to_string()
                .contains("edited F3D curve f3d:brep:procedural_surface#7:profile is not a patchable NURBS carrier"),
            "{error}"
        );
    }

    #[test]
    fn a_same_kind_nurbs_curve_edit_that_changes_the_rational_form_is_not_implemented() {
        let error = validate_curve_edits(
            &curve(
                "f3d:brep:entity#7",
                nurbs_curve(vec![0.0, 0.0, 1.0, 1.0], 2, None),
            ),
            &curve(
                "f3d:brep:entity#7",
                nurbs_curve(vec![0.0, 0.0, 1.0, 1.0], 2, Some(vec![1.0, 1.0])),
            ),
        )
        .expect_err("a polynomial carrier cannot gain a weight lane");
        assert!(
            error
                .to_string()
                .contains("edited F3D curve f3d:brep:entity#7 changes the NURBS rational form"),
            "{error}"
        );
    }

    #[test]
    fn a_same_kind_nurbs_curve_edit_that_changes_the_pole_count_is_not_implemented() {
        let error = validate_curve_edits(
            &curve(
                "f3d:brep:entity#7",
                nurbs_curve(vec![0.0, 0.0, 1.0, 1.0], 2, None),
            ),
            &curve(
                "f3d:brep:entity#7",
                nurbs_curve(vec![0.0, 0.0, 1.0, 1.0, 1.0], 3, None),
            ),
        )
        .expect_err("a NURBS carrier keeps its control-point count");
        assert!(
            error.to_string().contains(
                "edited F3D curve f3d:brep:entity#7 changes the NURBS control-point count"
            ),
            "{error}"
        );
    }

    #[test]
    fn a_pole_edit_on_a_nurbs_curve_carrier_with_a_negative_baseline_weight_is_admitted() {
        let weights = Some(vec![1.0, -1.0]);
        let baseline = curve(
            "f3d:brep:entity#7",
            nurbs_curve(vec![0.0, 0.0, 1.0, 1.0], 2, weights.clone()),
        );
        let target = curve(
            "f3d:brep:entity#7",
            SolvedCurveGeometry::Nurbs(
                NurbsCurve::from_lanes(
                    1,
                    vec![0.0, 0.0, 1.0, 1.0],
                    vec![Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 5.0, 0.0)],
                    weights,
                    false,
                )
                .expect("degree, knots and control points agree"),
            ),
        );
        let edited = validate_curve_edits(&baseline, &target)
            .expect("a negative baseline weight refuses no pole edit");
        assert_eq!(
            edited,
            std::collections::BTreeSet::from(["f3d:brep:entity#7".to_owned()])
        );
    }

    #[test]
    fn a_same_kind_cone_edit_that_removes_the_radius_is_malformed() {
        let error = validate_surface_edits(
            &cone_surface(2.0, std::f64::consts::FRAC_PI_4),
            &cone_surface(0.0, std::f64::consts::FRAC_PI_4),
        )
        .expect_err("a cone carrier keeps a nonzero radius");
        assert!(
            error.to_string().contains(
                "edited F3D cone surface f3d:brep:entity#9 has a zero cross-section radius"
            ),
            "{error}"
        );
    }

    #[test]
    fn a_same_kind_cone_edit_that_opens_the_half_angle_past_a_right_angle_is_malformed() {
        let error = validate_surface_edits(
            &cone_surface(2.0, std::f64::consts::FRAC_PI_4),
            &cone_surface(2.0, std::f64::consts::PI),
        )
        .expect_err("a cone carrier keeps a half angle no wider than a right angle");
        assert!(
            error.to_string().contains(
                "edited F3D cone surface f3d:brep:entity#9 has a half angle outside [0, pi/2]"
            ),
            "{error}"
        );
    }

    #[test]
    fn a_radius_edit_on_a_cone_carrier_with_a_right_baseline_half_angle_is_admitted() {
        let edited = validate_surface_edits(
            &cone_surface(2.0, std::f64::consts::FRAC_PI_2),
            &cone_surface(3.0, std::f64::consts::FRAC_PI_2),
        )
        .expect("a right baseline half angle refuses no radius edit");
        assert_eq!(
            edited,
            std::collections::BTreeSet::from(["f3d:brep:entity#9".to_owned()])
        );
    }

    #[test]
    fn a_nurbs_surface_edit_on_a_carrier_the_patcher_does_not_address_is_not_implemented() {
        let id = "f3d:brep:procedural_surface#9:profile";
        let error = validate_surface_edits(
            &surface(
                id,
                nurbs_surface(
                    vec![0.0, 0.0, 1.0, 1.0],
                    vec![0.0, 0.0, 1.0, 1.0],
                    2,
                    2,
                    None,
                ),
            ),
            &surface(
                id,
                nurbs_surface(
                    vec![0.0, 0.0, 2.0, 2.0],
                    vec![0.0, 0.0, 1.0, 1.0],
                    2,
                    2,
                    None,
                ),
            ),
        )
        .expect_err("only the addressed NURBS carriers are patchable");
        assert!(
            error.to_string().contains(
                "edited F3D surface f3d:brep:procedural_surface#9:profile is not a patchable NURBS carrier"
            ),
            "{error}"
        );
    }

    #[test]
    fn a_same_kind_nurbs_surface_edit_that_changes_the_u_knot_multiplicity_is_not_implemented() {
        let error = validate_surface_edits(
            &surface(
                "f3d:brep:entity#9",
                nurbs_surface(
                    vec![0.0, 0.0, 1.0, 1.0],
                    vec![0.0, 0.0, 1.0, 1.0],
                    2,
                    2,
                    None,
                ),
            ),
            &surface(
                "f3d:brep:entity#9",
                nurbs_surface(
                    vec![0.0, 0.5, 1.0, 1.0],
                    vec![0.0, 0.0, 1.0, 1.0],
                    2,
                    2,
                    None,
                ),
            ),
        )
        .expect_err("a NURBS carrier keeps its u knot multiplicities");
        assert!(
            error
                .to_string()
                .contains("edited F3D surface f3d:brep:entity#9 changes the NURBS u knot layout"),
            "{error}"
        );
    }

    #[test]
    fn a_same_kind_nurbs_surface_edit_that_changes_the_v_knot_multiplicity_is_not_implemented() {
        let error = validate_surface_edits(
            &surface(
                "f3d:brep:entity#9",
                nurbs_surface(
                    vec![0.0, 0.0, 1.0, 1.0],
                    vec![0.0, 0.0, 1.0, 1.0],
                    2,
                    2,
                    None,
                ),
            ),
            &surface(
                "f3d:brep:entity#9",
                nurbs_surface(
                    vec![0.0, 0.0, 1.0, 1.0],
                    vec![0.0, 0.5, 1.0, 1.0],
                    2,
                    2,
                    None,
                ),
            ),
        )
        .expect_err("a NURBS carrier keeps its v knot multiplicities");
        assert!(
            error
                .to_string()
                .contains("edited F3D surface f3d:brep:entity#9 changes the NURBS v knot layout"),
            "{error}"
        );
    }

    #[test]
    fn a_same_kind_nurbs_surface_edit_that_leaves_the_writable_u_degree_range_is_malformed() {
        let u_degree_zero = SolvedSurfaceGeometry::Nurbs(
            NurbsSurface::from_lanes(
                NurbsSurfaceAxis::new(0, vec![0.0, 0.0, 1.0], false),
                NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
                NurbsSurfaceLanes::new((0..2).map(|_| poles(2)).collect(), None),
                false,
            )
            .expect("degrees, knots and control grid agree"),
        );
        let error = validate_surface_edits(
            &surface(
                "f3d:brep:entity#9",
                nurbs_surface(
                    vec![0.0, 0.0, 1.0, 1.0],
                    vec![0.0, 0.0, 1.0, 1.0],
                    2,
                    2,
                    None,
                ),
            ),
            &surface("f3d:brep:entity#9", u_degree_zero),
        )
        .expect_err("a spline record states a u degree in 1..=20");
        assert!(
            error.to_string().contains(
                "edited F3D surface f3d:brep:entity#9 has a NURBS u degree outside [1, 20]"
            ),
            "{error}"
        );
    }

    #[test]
    fn a_same_kind_nurbs_surface_edit_that_leaves_the_writable_v_degree_range_is_malformed() {
        let v_degree_zero = SolvedSurfaceGeometry::Nurbs(
            NurbsSurface::from_lanes(
                NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
                NurbsSurfaceAxis::new(0, vec![0.0, 0.0, 1.0], false),
                NurbsSurfaceLanes::new((0..2).map(|_| poles(2)).collect(), None),
                false,
            )
            .expect("degrees, knots and control grid agree"),
        );
        let error = validate_surface_edits(
            &surface(
                "f3d:brep:entity#9",
                nurbs_surface(
                    vec![0.0, 0.0, 1.0, 1.0],
                    vec![0.0, 0.0, 1.0, 1.0],
                    2,
                    2,
                    None,
                ),
            ),
            &surface("f3d:brep:entity#9", v_degree_zero),
        )
        .expect_err("a spline record states a v degree in 1..=20");
        assert!(
            error.to_string().contains(
                "edited F3D surface f3d:brep:entity#9 has a NURBS v degree outside [1, 20]"
            ),
            "{error}"
        );
    }

    #[test]
    fn a_same_kind_nurbs_surface_edit_that_changes_the_u_pole_count_is_not_implemented() {
        let error = validate_surface_edits(
            &surface(
                "f3d:brep:entity#9",
                nurbs_surface(
                    vec![0.0, 0.0, 1.0, 1.0],
                    vec![0.0, 0.0, 1.0, 1.0],
                    2,
                    2,
                    None,
                ),
            ),
            &surface(
                "f3d:brep:entity#9",
                nurbs_surface(
                    vec![0.0, 0.0, 1.0, 1.0, 1.0],
                    vec![0.0, 0.0, 1.0, 1.0],
                    3,
                    2,
                    None,
                ),
            ),
        )
        .expect_err("a NURBS carrier keeps its u control-point count");
        assert!(
            error.to_string().contains(
                "edited F3D surface f3d:brep:entity#9 changes the NURBS u control-point count"
            ),
            "{error}"
        );
    }

    #[test]
    fn a_same_kind_nurbs_surface_edit_that_changes_the_v_pole_count_is_not_implemented() {
        let error = validate_surface_edits(
            &surface(
                "f3d:brep:entity#9",
                nurbs_surface(
                    vec![0.0, 0.0, 1.0, 1.0],
                    vec![0.0, 0.0, 1.0, 1.0],
                    2,
                    2,
                    None,
                ),
            ),
            &surface(
                "f3d:brep:entity#9",
                nurbs_surface(
                    vec![0.0, 0.0, 1.0, 1.0],
                    vec![0.0, 0.0, 1.0, 1.0, 1.0],
                    2,
                    3,
                    None,
                ),
            ),
        )
        .expect_err("a NURBS carrier keeps its v control-point count");
        assert!(
            error.to_string().contains(
                "edited F3D surface f3d:brep:entity#9 changes the NURBS v control-point count"
            ),
            "{error}"
        );
    }

    #[test]
    fn a_same_kind_nurbs_surface_edit_that_changes_the_rational_form_is_not_implemented() {
        let error = validate_surface_edits(
            &surface(
                "f3d:brep:entity#9",
                nurbs_surface(
                    vec![0.0, 0.0, 1.0, 1.0],
                    vec![0.0, 0.0, 1.0, 1.0],
                    2,
                    2,
                    None,
                ),
            ),
            &surface(
                "f3d:brep:entity#9",
                nurbs_surface(
                    vec![0.0, 0.0, 1.0, 1.0],
                    vec![0.0, 0.0, 1.0, 1.0],
                    2,
                    2,
                    Some(weight_grid(2, 2, 1.0)),
                ),
            ),
        )
        .expect_err("a polynomial carrier cannot gain a weight grid");
        assert!(
            error
                .to_string()
                .contains("edited F3D surface f3d:brep:entity#9 changes the NURBS rational form"),
            "{error}"
        );
    }

    #[test]
    fn a_pole_edit_on_a_nurbs_surface_carrier_with_a_negative_baseline_weight_is_admitted() {
        let baseline = surface(
            "f3d:brep:entity#9",
            nurbs_surface(
                vec![0.0, 0.0, 1.0, 1.0],
                vec![0.0, 0.0, 1.0, 1.0],
                2,
                2,
                Some(weight_grid(2, 2, -1.0)),
            ),
        );
        let moved = vec![
            poles(2),
            vec![Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 5.0)],
        ];
        let target = surface(
            "f3d:brep:entity#9",
            SolvedSurfaceGeometry::Nurbs(
                NurbsSurface::from_lanes(
                    NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
                    NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
                    NurbsSurfaceLanes::new(moved, Some(weight_grid(2, 2, -1.0))),
                    false,
                )
                .expect("degrees, knots and control grid agree"),
            ),
        );
        let edited = validate_surface_edits(&baseline, &target)
            .expect("a negative baseline weight refuses no pole edit");
        assert_eq!(
            edited,
            std::collections::BTreeSet::from(["f3d:brep:entity#9".to_owned()])
        );
    }

    fn assignment(physical_token: bool) -> DesignMaterialAssignment {
        let mut document = serde_json::json!({
            "id": "material#0",
            "asm_body_key": 42,
            "asm_body_key_offset": 200,
            "entity_suffix_offset": 0,
            "entity_id": "0_985",
            "entity_id_offset": 8,
            "visual_guid": "11111111-2222-3333-4444-555555555555",
            "visual_guid_offset": 18
        });
        if physical_token {
            document["physical_token"] = "Prism-002".into();
            document["physical_token_offset"] = 90.into();
        }
        serde_json::from_value(document).expect("material assignment")
    }

    fn native(assignments: Vec<DesignMaterialAssignment>) -> F3dNative {
        F3dNative {
            design_material_assignments: assignments,
            ..Default::default()
        }
    }

    #[test]
    fn a_material_token_cannot_be_added_where_the_baseline_record_has_no_carrier() {
        let baseline = native(vec![assignment(false)]);
        let target = native(vec![assignment(true)]);
        let error = validate_material_assignment_edits(PatchNatives {
            baseline: Some(&baseline),
            target: Some(&target),
        })
        .expect_err("an added token has no carrier to write it into");
        assert!(
            error
                .to_string()
                .contains("changes fields outside writable strings"),
            "{error}"
        );

        let edits = validate_material_assignment_edits(PatchNatives {
            baseline: Some(&baseline),
            target: Some(&baseline),
        })
        .expect("an unchanged assignment set is editable");
        assert!(edits.is_empty());
    }
}
