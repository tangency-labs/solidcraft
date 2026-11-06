// SPDX-License-Identifier: Apache-2.0
//! Source-less pre-write validators for the neutral `CadIr` and its F3D native
//! extension.

use cadmpeg_core::decode::index_from_u32;

use std::collections::{BTreeMap, BTreeSet, HashMap};

use crate::native::F3dNative;
use crate::records::{
    entity_header::SegmentType,
    sketch_geometry::SketchCurveGeometry,
    sketch_links::{PersistentDesignLink, PersistentSubentityTag},
};
use cadmpeg_asm::brep::records::EvaluatedToleranceSlot;
use cadmpeg_core::CodecError;
use cadmpeg_ir::attributes::AttributeTarget;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::{CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry};

use super::attributes::source_less_body_key;
pub(super) fn validate_source_less_procedural_carriers(target: &CadIr) -> Result<(), CodecError> {
    let mut surface_owners = BTreeSet::new();
    for procedural in &target.model.procedural_surfaces {
        let mut owners =
            target.model.surfaces.iter().filter(|surface| {
                surface.geometry.procedural_construction() == Some(&procedural.id)
            });
        let (Some(surface), None) = (owners.next(), owners.next()) else {
            return Err(CodecError::InvalidInput(format!(
                "procedural surface {} has no unique carrier",
                procedural.id
            )));
        };
        let owner = &surface.id;
        if !surface_owners.insert(owner) {
            return Err(CodecError::InvalidInput(format!(
                "surface {owner} has multiple procedural constructions"
            )));
        }
        if surface.geometry.solved_cache().is_some_and(|geometry| {
            !matches!(
                geometry,
                SolvedSurfaceGeometry::Nurbs(_) | SolvedSurfaceGeometry::Unknown { .. }
            )
        }) {
            return Err(CodecError::NotImplemented(format!(
                "source-less F3D procedural surface {} cannot retain its construction on analytic carrier {}",
                procedural.id, surface.id
            )));
        }
    }

    let mut curve_owners = BTreeSet::new();
    for procedural in &target.model.procedural_curves {
        let mut owners = target
            .model
            .curves
            .iter()
            .filter(|curve| curve.geometry.procedural_construction() == Some(&procedural.id));
        let (Some(curve), None) = (owners.next(), owners.next()) else {
            return Err(CodecError::InvalidInput(format!(
                "procedural curve {} has no unique carrier",
                procedural.id
            )));
        };
        let owner = &curve.id;
        if !curve_owners.insert(owner) {
            return Err(CodecError::InvalidInput(format!(
                "curve {owner} has multiple procedural constructions"
            )));
        }
        match curve.geometry.solved_cache() {
            Some(SolvedCurveGeometry::Nurbs(_)) => {}
            None if procedural.cache_fit_tolerance().is_none() => {}
            None => {
                return Err(CodecError::InvalidInput(format!(
                    "cacheless procedural curve {} carries a cache-fit tolerance",
                    procedural.id
                )))
            }
            Some(_) => {
                return Err(CodecError::NotImplemented(format!(
                "source-less F3D procedural curve {} cannot retain its construction on carrier {}",
                procedural.id, curve.id
            )))
            }
        }
    }
    Ok(())
}

pub(super) fn validate_source_less_topology_tolerances(
    target: &CadIr,
    native: &F3dNative,
) -> Result<(), CodecError> {
    if let Some(face) = target
        .model
        .faces
        .iter()
        .find(|face| face.tolerance.is_some())
    {
        return Err(CodecError::NotImplemented(format!(
            "source-less F3D cannot serialize face {} tolerance losslessly",
            face.id
        )));
    }
    let tolerant = native
        .tolerant_coedge_parameters
        .iter()
        .filter(|parameters| {
            matches!(
                parameters.extension,
                cadmpeg_asm::brep::records::TolerantCoedgeExtension::EmbeddedCurve {
                    target: None,
                    ..
                }
            )
        })
        .map(|parameters| parameters.coedge.as_str())
        .collect::<std::collections::HashSet<_>>();
    if let Some(coedge) = target
        .model
        .coedges
        .iter()
        .find(|coedge| coedge.use_curve.is_some() && !tolerant.contains(coedge.id.as_str()))
    {
        return Err(CodecError::NotImplemented(format!(
            "source-less F3D coedge {} use curve lacks a cache-local tolerant extension",
            coedge.id
        )));
    }
    Ok(())
}

pub(super) fn validate_source_less_auxiliary_geometry(target: &CadIr) -> Result<(), CodecError> {
    if let Some(tessellation) = target.model.tessellations.first() {
        return Err(CodecError::NotImplemented(format!(
            "source-less F3D cannot serialize neutral tessellation {} losslessly",
            tessellation.id
        )));
    }
    if let Some(surface) = target
        .model
        .surfaces
        .iter()
        .find(|surface| surface.source_object.is_some())
    {
        return Err(CodecError::NotImplemented(format!(
            "source-less F3D cannot serialize source-object association on surface {} losslessly",
            surface.id
        )));
    }
    if let Some(curve) = target
        .model
        .curves
        .iter()
        .find(|curve| curve.source_object.is_some())
    {
        return Err(CodecError::NotImplemented(format!(
            "source-less F3D cannot serialize source-object association on curve {} losslessly",
            curve.id
        )));
    }
    Ok(())
}

pub(super) fn validate_source_less_recipes(native: &F3dNative) -> Result<(), CodecError> {
    if native.construction_recipes.windows(2).any(|pair| {
        pair[0].record_index.map(|index| index.value)
            > pair[1].record_index.map(|index| index.value)
    }) {
        return Err(CodecError::InvalidInput(
            "F3D construction recipes must be ordered by record index".into(),
        ));
    }
    let mut group_counts = HashMap::new();
    for recipe in &native.construction_recipes {
        let expected = group_counts
            .entry((
                recipe.kind,
                recipe
                    .design
                    .as_ref()
                    .map(|design| design.id.value.as_str()),
            ))
            .or_insert(0_u64);
        if u64::from(recipe.recipe_index) != *expected {
            return Err(CodecError::InvalidInput(format!(
                "F3D construction recipe {} has noncontiguous group index {}",
                recipe.id, recipe.recipe_index
            )));
        }
        *expected += 1;
    }
    Ok(())
}

fn source_less_design_record_type<'a>(
    native: &'a F3dNative,
    class_tag: &crate::records::references::DesignClassTag,
    record_index: u32,
    record_kind: &str,
) -> Result<&'a SegmentType, CodecError> {
    let type_ordinal = class_tag.dynamic_ordinal().ok_or_else(|| {
        CodecError::InvalidInput(format!(
            "F3D {record_kind} class tag {} is below the dynamic type range",
            class_tag.as_str()
        ))
    })?;
    let design_type = native.design_types.get(type_ordinal).ok_or_else(|| {
        CodecError::InvalidInput(format!(
            "F3D {record_kind} class tag {} is outside the Design type table",
            class_tag.as_str()
        ))
    })?;
    if !design_type
        .entities
        .values()
        .any(|registered| *registered == u64::from(record_index))
    {
        return Err(CodecError::InvalidInput(format!(
            "F3D {record_kind} {record_index} is not registered by class tag {}",
            class_tag.as_str()
        )));
    }
    Ok(design_type)
}

fn design_type_matches(design_type: &SegmentType, expected: (&str, u32, &str)) -> bool {
    design_type
        .type_guid
        .as_str()
        .eq_ignore_ascii_case(expected.0)
        && design_type.version == expected.1
        && design_type.module == expected.2
}

pub(super) fn validate_source_less_sketch_graph(native: &F3dNative) -> Result<(), CodecError> {
    let sketch_owners = native
        .design_entity_headers
        .iter()
        .filter(|header| header.in_sketch_module())
        .map(|header| header.entity_id.suffix())
        .collect::<BTreeSet<_>>();
    let root_indices = native
        .design_entity_headers
        .iter()
        .filter(|header| header.in_sketch_module())
        .flat_map(|header| header.reference_values().copied())
        .collect::<BTreeSet<_>>();
    let mut typed_indices = BTreeMap::<u32, &str>::new();
    let mut typed_records = Vec::new();
    for (record_index, id, class_tag) in native
        .sketch_points
        .iter()
        .map(|record| (record.record_index, record.id.as_str(), &record.class_tag))
        .chain(
            native
                .sketch_curve_identities
                .iter()
                .map(|record| (record.record_index, record.id.as_str(), &record.class_tag)),
        )
        .chain(
            native
                .sketch_relations
                .iter()
                .map(|record| (record.record_index, record.id.as_str(), &record.class_tag)),
        )
        .chain(
            native
                .sketch_texts
                .iter()
                .map(|record| (record.record_index, record.id.as_str(), &record.class_tag)),
        )
    {
        if let Some(before) = typed_indices.insert(record_index, id) {
            return Err(CodecError::InvalidInput(format!(
                "F3D sketch records {before} and {id} share record index {record_index}"
            )));
        }
        typed_records.push((record_index, class_tag));
    }
    for (record_index, class_tag) in typed_records {
        source_less_design_record_type(native, class_tag, record_index, "sketch record")?;
    }
    let mut geometry_owners = BTreeMap::new();
    let mut point_companions = BTreeSet::new();
    let curve_indices = native
        .sketch_curve_identities
        .iter()
        .map(|curve| curve.record_index)
        .collect::<BTreeSet<_>>();
    for point in &native.sketch_points {
        let point_type = source_less_design_record_type(
            native,
            &point.class_tag,
            point.record_index,
            "sketch point",
        )?;
        if !design_type_matches(
            point_type,
            crate::design::decode::sketch::CURRENT_SKETCH_POINT_TYPE,
        ) {
            return Err(CodecError::NotImplemented(format!(
                "source-less F3D sketch point {} requires the current point record type",
                point.id
            )));
        }
        if point.record_form().class_version() != point_type.version
            || !matches!(
                point.record_form(),
                crate::records::sketch_geometry::SketchPointRecordForm::Version11 {
                    derived_type: false,
                    ..
                }
            )
        {
            return Err(CodecError::NotImplemented(format!(
                "source-less F3D sketch point {} requires the version-11 member sequence",
                point.id
            )));
        }
        if point
            .persistent_id()
            .is_none_or(|persistent_id| persistent_id == 0)
        {
            return Err(CodecError::InvalidInput(format!(
                "source-less F3D sketch point {} has no persistent identity",
                point.id
            )));
        }
        let owner_reference = point.owner_reference.ok_or_else(|| {
            CodecError::InvalidInput(format!(
                "F3D sketch point {} has no direct sketch owner",
                point.id
            ))
        })?;
        if !sketch_owners.contains(&u64::from(owner_reference)) {
            return Err(CodecError::InvalidInput(format!(
                "F3D sketch point {} references missing sketch owner {owner_reference}",
                point.id
            )));
        }
        let owner_type_count = native
            .design_types
            .iter()
            .filter(|design_type| {
                design_type
                    .entities
                    .values()
                    .any(|registered| *registered == u64::from(owner_reference))
                    && design_type.type_guid.as_str().eq_ignore_ascii_case(
                        crate::design::decode::sketch::SKETCH_CONTAINER_TYPE_GUID,
                    )
            })
            .count();
        if owner_type_count != 1 {
            return Err(CodecError::InvalidInput(format!(
                "F3D sketch point {} owner {owner_reference} does not have one sketch-container type registration",
                point.id
            )));
        }
        if point.paired_reference == point.record_index
            || typed_indices.contains_key(&point.paired_reference)
            || !point_companions.insert(point.paired_reference)
        {
            return Err(CodecError::InvalidInput(format!(
                "F3D sketch point {} has a conflicting companion record {}",
                point.id, point.paired_reference
            )));
        }
        let companion_type = native
            .design_types
            .iter()
            .filter(|design_type| {
                design_type
                    .entities
                    .values()
                    .any(|registered| *registered == u64::from(point.paired_reference))
                    && design_type_matches(
                        design_type,
                        crate::design::decode::sketch::SKETCH_POINT_COMPANION_TYPE,
                    )
            })
            .count();
        if companion_type != 1 {
            return Err(CodecError::InvalidInput(format!(
                "F3D sketch point {} companion {} does not have one current companion type registration",
                point.id, point.paired_reference
            )));
        }
        let companion = point.companion();
        for curve in companion.incident_curves {
            if !curve_indices.contains(curve) {
                return Err(CodecError::InvalidInput(format!(
                    "F3D sketch point {} companion references missing curve {curve}",
                    point.id
                )));
            }
        }
        geometry_owners.insert(point.record_index, owner_reference);
    }
    for curve in &native.sketch_curve_identities {
        let curve_type = source_less_design_record_type(
            native,
            &curve.class_tag,
            curve.record_index,
            "sketch curve",
        )?;
        let expected_type = match curve.geometry.as_ref() {
            Some(SketchCurveGeometry::Line { .. }) => {
                crate::design::decode::sketch::CURRENT_SKETCH_LINE_TYPE
            }
            Some(SketchCurveGeometry::Arc { .. }) => {
                crate::design::decode::sketch::CURRENT_SKETCH_CIRCULAR_TYPE
            }
            Some(SketchCurveGeometry::Nurbs { .. }) => {
                crate::design::decode::sketch::CURRENT_SKETCH_NURBS_TYPE
            }
            None => {
                return Err(CodecError::NotImplemented(format!(
                    "source-less F3D sketch curve {} has no writable geometry",
                    curve.id
                )))
            }
        };
        if !design_type_matches(curve_type, expected_type) {
            return Err(CodecError::NotImplemented(format!(
                "source-less F3D sketch curve {} requires its current geometry record type",
                curve.id
            )));
        }
        let owner_reference = curve.owner_reference.ok_or_else(|| {
            CodecError::InvalidInput(format!(
                "F3D sketch curve {} has no direct sketch owner",
                curve.id
            ))
        })?;
        if !sketch_owners.contains(&u64::from(owner_reference)) {
            return Err(CodecError::InvalidInput(format!(
                "F3D sketch curve {} references missing sketch owner {owner_reference}",
                curve.id
            )));
        }
        geometry_owners.insert(curve.record_index, owner_reference);
    }
    for relation in &native.sketch_relations {
        if !root_indices.contains(&relation.record_index) {
            return Err(CodecError::InvalidInput(format!(
                "F3D sketch relation {} is not reachable from a sketch header",
                relation.id
            )));
        }
        if !sketch_owners.contains(&u64::from(relation.owner_reference)) {
            return Err(CodecError::InvalidInput(format!(
                "F3D sketch relation {} references missing sketch owner {}",
                relation.id, relation.owner_reference
            )));
        }
        for member in relation.all_member_indices() {
            if geometry_owners
                .get(&member)
                .is_some_and(|owner| *owner != relation.owner_reference)
            {
                return Err(CodecError::InvalidInput(format!(
                    "F3D sketch relation {} owner disagrees with geometry record {member}",
                    relation.id
                )));
            }
        }
    }
    for text in &native.sketch_texts {
        if !root_indices.contains(&text.record_index) {
            return Err(CodecError::InvalidInput(format!(
                "F3D sketch text {} is not reachable from a sketch header",
                text.id
            )));
        }
        if !sketch_owners.contains(&u64::from(text.owner_reference)) {
            return Err(CodecError::InvalidInput(format!(
                "F3D sketch text {} references missing sketch owner {}",
                text.id, text.owner_reference
            )));
        }
    }
    let mut reachable_headers = root_indices;
    for relation in &native.sketch_relations {
        reachable_headers.extend(relation.all_member_indices());
    }
    let mut explicit_headers = BTreeSet::new();
    for header in &native.design_record_headers {
        if !explicit_headers.insert(header.record_index) {
            return Err(CodecError::InvalidInput(format!(
                "multiple F3D Design record headers use index {}",
                header.record_index
            )));
        }
        if typed_indices.contains_key(&header.record_index) {
            return Err(CodecError::InvalidInput(format!(
                "F3D Design record header {} shadows a typed sketch record",
                header.id
            )));
        }
        if !reachable_headers.contains(&header.record_index) {
            return Err(CodecError::InvalidInput(format!(
                "F3D Design record header {} is unreachable from the sketch graph",
                header.id
            )));
        }
    }
    Ok(())
}

pub(super) fn validate_source_less_design_ownership(native: &F3dNative) -> Result<(), CodecError> {
    let mut types_by_guid = BTreeMap::new();
    let mut entity_types = BTreeMap::new();
    let mut entity_modules = BTreeMap::new();
    for design_type in &native.design_types {
        if types_by_guid
            .insert(design_type.type_guid.as_str(), design_type)
            .is_some()
        {
            return Err(CodecError::InvalidInput(format!(
                "duplicate F3D Design type GUID: {}",
                design_type.type_guid.as_str()
            )));
        }
        for entity_id in design_type.entities.values() {
            if let Some(before) = entity_types.insert(*entity_id, design_type.type_guid.as_str()) {
                return Err(CodecError::InvalidInput(format!(
                    "F3D Design entity {entity_id} is registered by both type {before} and type {}",
                    design_type.type_guid.as_str()
                )));
            }
            entity_modules.insert(*entity_id, design_type.module.clone());
        }
    }
    // A base type need not be registered by the same segment, so an unresolved
    // base GUID is legal; a resolved chain must still terminate.
    for design_type in &native.design_types {
        if design_type
            .base_type_guid
            .value()
            .map(crate::records::mesh::DesignRelaxedGuidText::as_str)
            == Some(design_type.type_guid.as_str())
        {
            return Err(CodecError::InvalidInput(format!(
                "F3D Design type {} is its own base type",
                design_type.id()
            )));
        }
        let mut ancestors = BTreeSet::new();
        let mut cursor = design_type;
        while let Some(base) = cursor
            .base_type_guid
            .value()
            .map(crate::records::mesh::DesignRelaxedGuidText::as_str)
            .and_then(|base| types_by_guid.get(base))
        {
            if !ancestors.insert(base.type_guid.as_str()) {
                return Err(CodecError::InvalidInput(format!(
                    "F3D Design type hierarchy contains a cycle at {}",
                    base.type_guid.as_str()
                )));
            }
            cursor = base;
        }
    }
    for header in &native.design_entity_headers {
        let owned_module = entity_modules.get(&header.entity_id.suffix()).cloned();
        if header.module() != owned_module.as_deref() {
            return Err(CodecError::InvalidInput(format!(
                "F3D Design header {} module conflicts with MetaStream ownership",
                header.id
            )));
        }
    }
    Ok(())
}

/// Proof that [`validate_source_less_design_bindings`] ran against the borrowed
/// `F3dNative`. The private field keeps construction inside this module, so an
/// encoder that reads binding-validated fields (material physical tokens) cannot
/// be reached without the check having run on the very native it will read.
#[derive(Clone, Copy)]
pub(super) struct DesignBindingsValidated<'a> {
    native: &'a F3dNative,
}

impl<'a> DesignBindingsValidated<'a> {
    /// The native whose design bindings were validated.
    pub(super) fn native(self) -> &'a F3dNative {
        self.native
    }
}

pub(super) fn validate_source_less_design_bindings(
    native: &F3dNative,
) -> Result<DesignBindingsValidated<'_>, CodecError> {
    let mut by_key = BTreeMap::new();
    let mut by_suffix = BTreeMap::new();
    let mut insert = |key: u64, suffix: u64, id: &str| -> Result<(), CodecError> {
        if by_key
            .insert(key, suffix)
            .is_some_and(|before| before != suffix)
            || by_suffix
                .insert(suffix, key)
                .is_some_and(|before| before != key)
        {
            return Err(CodecError::InvalidInput(format!(
                "F3D Design body binding {id} conflicts with the body-map key/suffix bijection"
            )));
        }
        Ok(())
    };
    if let Some(assignment) = native.design_material_assignments.first() {
        return Err(CodecError::NotImplemented(format!(
            "source-less F3D material assignment {} requires a typed body-presentation B-rep and scene graph",
            assignment.id
        )));
    }
    for visibility in &native.body_visibilities {
        insert(
            visibility.asm_body_key(),
            visibility.entity_suffix,
            visibility.id(),
        )?;
    }
    Ok(DesignBindingsValidated { native })
}

pub(super) fn validate_source_less_history_graph(
    target: &CadIr,
    native: &F3dNative,
) -> Result<(), CodecError> {
    let Some(namespace) = target.native.namespace("f3d") else {
        return Ok(());
    };
    let stored_count = |arena: &str| namespace.arenas().get(arena).map_or(0, Vec::len);
    for arena in [
        "asm_histories",
        "asm_delta_states",
        "asm_bulletin_boards",
        "asm_entity_changes",
        "asm_history_records",
    ] {
        if let Some(records) = namespace.arenas().get(arena) {
            let unique = records
                .iter()
                .map(cadmpeg_ir::NativeRecord::id)
                .collect::<BTreeSet<_>>();
            if unique.len() != records.len() {
                return Err(CodecError::InvalidInput(format!(
                    "F3D {arena} contains duplicate record ids"
                )));
            }
        }
    }
    let states = native
        .asm_histories
        .iter()
        .flat_map(crate::history_records::AsmHistory::states)
        .collect::<Vec<_>>();
    let boards = states
        .iter()
        .flat_map(|state| &state.bulletin_boards)
        .collect::<Vec<_>>();
    let changes = boards.iter().flat_map(|board| &board.changes).count();
    let records = states.iter().flat_map(|state| &state.records).count();
    let reconstructed = [
        ("asm_histories", native.asm_histories.len()),
        ("asm_delta_states", states.len()),
        ("asm_bulletin_boards", boards.len()),
        ("asm_entity_changes", changes),
        ("asm_history_records", records),
    ];
    if reconstructed
        .iter()
        .any(|(arena, count)| stored_count(arena) != *count)
    {
        return Err(CodecError::InvalidInput(
            "F3D ASM history graph contains orphaned or ambiguously parented records".into(),
        ));
    }
    for state in states {
        for record in &state.records {
            if record.raw_bytes.is_empty() {
                return Err(CodecError::InvalidInput(format!(
                    "F3D ASM history record {} has an empty native payload",
                    record.id
                )));
            }
        }
    }
    if let Some(history) = native
        .asm_histories
        .iter()
        .find(|history| history.binding_budget_exceeded)
    {
        return Err(CodecError::NotImplemented(format!(
            "source-less F3D cannot serialize ASM history {} whose bulletin boards and records were not retained",
            history.id
        )));
    }
    for history in &native.asm_histories {
        match history.preamble {
            Some(preamble)
                if history
                    .states()
                    .first()
                    .is_some_and(|state| state.state_id == preamble.stream_size)
                    && preamble.history_entry_count >= 0 => {}
            Some(_) => {
                return Err(CodecError::InvalidInput(format!(
                    "F3D history {} requires head state_id == stream_size and nonnegative history_entry_count",
                    history.id
                )));
            }
            None => {}
        }
    }
    if native
        .asm_histories
        .iter()
        .any(|history| !crate::history::graph_is_coherent(history))
    {
        return Err(CodecError::InvalidInput(
            "F3D ASM history graph is not a coherent doubly linked state chain".into(),
        ));
    }
    Ok(())
}

pub(super) fn validate_source_less_design_links(
    target: &CadIr,
    native: &F3dNative,
    attributes: &super::attributes::AttributeIndex<'_>,
) -> Result<(), CodecError> {
    if let Some(sentinel) = native.mesh_surface_sentinels.first() {
        return Err(CodecError::NotImplemented(format!(
            "source-less F3D cannot serialize mesh-surface sentinel {} without its retained ASM record",
            sentinel.id()
        )));
    }
    let coedges = target
        .model
        .coedges
        .iter()
        .map(|coedge| (&coedge.id, coedge))
        .collect::<HashMap<_, _>>();
    let mut linked_coedges = BTreeSet::new();
    for link in &native.sketch_curve_links {
        // Only a coedge-owned link is regenerated; a link on any other owner is
        // reported as a source-fidelity loss instead of refusing the write.
        let AttributeTarget::Coedge(coedge) = &link.target else {
            continue;
        };
        if !coedges.contains_key(coedge) {
            return Err(CodecError::InvalidInput(format!(
                "F3D sketch-curve link {} targets a missing coedge {}",
                link.id,
                coedge.as_str()
            )));
        }
        if !linked_coedges.insert(coedge) {
            return Err(CodecError::InvalidInput(format!(
                "source-less F3D generation supports one sketch-curve link per coedge: {}",
                coedge.as_str()
            )));
        }
    }

    let bodies = target
        .model
        .bodies
        .iter()
        .enumerate()
        .map(|(ordinal, body)| (&body.id, (ordinal, body)))
        .collect::<HashMap<_, _>>();
    let faces = target
        .model
        .faces
        .iter()
        .map(|face| (&face.id, face))
        .collect::<HashMap<_, _>>();
    let edges = target
        .model
        .edges
        .iter()
        .map(|edge| (&edge.id, edge))
        .collect::<HashMap<_, _>>();
    let mut groups: BTreeMap<AttributeTarget, Vec<&PersistentDesignLink>> = BTreeMap::new();
    for link in &native.persistent_design_links {
        let target_key = match &link.target {
            cadmpeg_ir::attributes::AttributeTarget::Body(id) if bodies.contains_key(id) => {
                Some(link.target.clone())
            }
            _ => None,
        };
        let Some(target_key) = target_key else {
            return Err(CodecError::InvalidInput(format!(
                "F3D persistent design link {} has an unsupported or missing target",
                link.id
            )));
        };

        groups.entry(target_key).or_default().push(link);
    }
    let mut subentity_groups: BTreeMap<(u8, String), Vec<&PersistentSubentityTag>> =
        BTreeMap::new();
    for tag in &native.persistent_subentity_tags {
        let target_key = match &tag.target {
            cadmpeg_ir::attributes::AttributeTarget::Face(id) if faces.contains_key(id) => {
                Some((2, id.as_str().to_owned()))
            }
            cadmpeg_ir::attributes::AttributeTarget::Edge(id) if edges.contains_key(id) => {
                Some((1, id.as_str().to_owned()))
            }
            _ => None,
        };
        let Some(target_key) = target_key else {
            return Err(CodecError::InvalidInput(format!(
                "F3D persistent subentity tag {} has an unsupported or missing target",
                tag.id
            )));
        };

        subentity_groups.entry(target_key).or_default().push(tag);
    }
    for (target, mut tags) in subentity_groups {
        tags.sort_by_key(|tag| tag.ordinal);
        for (ordinal, tag) in tags.iter().enumerate() {
            if index_from_u32(tag.ordinal) != ordinal {
                return Err(CodecError::InvalidInput(format!(
                    "F3D persistent subentity tags for {target:?} require contiguous ordinals"
                )));
            }
        }
    }
    for (target, mut links) in groups {
        links.sort_by_key(|link| link.ordinal);
        for (ordinal, link) in links.iter().enumerate() {
            if index_from_u32(link.ordinal) != ordinal {
                return Err(CodecError::InvalidInput(format!(
                    "F3D persistent design links for {target:?} require contiguous ordinals"
                )));
            }
        }
    }

    let curve_by_id = target
        .model
        .curves
        .iter()
        .map(|curve| (curve.id.as_str(), curve))
        .collect::<std::collections::HashMap<_, _>>();
    let mut tolerant_coedges = BTreeSet::new();
    for parameters in &native.tolerant_coedge_parameters {
        let coedge = coedges.get(&parameters.coedge).copied().ok_or_else(|| {
            CodecError::InvalidInput(format!(
                "F3D tolerant-coedge metadata {} targets missing coedge {}",
                parameters.id(),
                parameters.coedge
            ))
        })?;
        if !tolerant_coedges.insert(&parameters.coedge) {
            return Err(CodecError::InvalidInput(format!(
                "multiple F3D tolerant-coedge records target {}",
                parameters.coedge
            )));
        }
        match &parameters.extension {
            cadmpeg_asm::brep::records::TolerantCoedgeExtension::None {}
            | cadmpeg_asm::brep::records::TolerantCoedgeExtension::Empty { target: None } => {}
            cadmpeg_asm::brep::records::TolerantCoedgeExtension::EmbeddedCurve {
                target: None,
                parameter_range,
                ..
            } => {
                let use_curve = coedge.use_curve.as_ref().ok_or_else(|| {
                    CodecError::InvalidInput(format!(
                        "F3D tolerant-coedge extension {} has no use curve",
                        parameters.id()
                    ))
                })?;
                let curve = curve_by_id
                    .get(use_curve.curve.as_str())
                    .copied()
                    .ok_or_else(|| {
                        CodecError::InvalidInput(format!(
                            "F3D tolerant-coedge extension {} references missing use curve {}",
                            parameters.id(),
                            use_curve.curve
                        ))
                    })?;
                if !matches!(
                    curve.geometry,
                    CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(_))
                ) {
                    return Err(CodecError::NotImplemented(format!(
                        "source-less F3D tolerant-coedge extension {} requires a NURBS use curve",
                        parameters.id()
                    )));
                }
                let effective_range = parameter_range.unwrap_or(parameters.parameter_range);
                if use_curve.parameter_range.endpoints() != effective_range.get() {
                    return Err(CodecError::InvalidInput(format!(
                        "F3D tolerant-coedge extension {} has an inconsistent use-curve parameter range",
                        parameters.id()
                    )));
                }
            }
            cadmpeg_asm::brep::records::TolerantCoedgeExtension::Empty { target: Some(_) }
            | cadmpeg_asm::brep::records::TolerantCoedgeExtension::Reference { .. }
            | cadmpeg_asm::brep::records::TolerantCoedgeExtension::EmbeddedCurve {
                target: Some(_),
                ..
            } => {
                return Err(CodecError::NotImplemented(format!(
                    "source-less F3D cannot relocate tolerant-coedge extension {}",
                    parameters.id()
                )));
            }
        }
    }

    let vertices = target
        .model
        .vertices
        .iter()
        .map(|vertex| (&vertex.id, vertex))
        .collect::<HashMap<_, _>>();
    let shells = target
        .model
        .shells
        .iter()
        .map(|shell| (&shell.id, shell))
        .collect::<HashMap<_, _>>();
    macro_rules! validate_unique_targets {
        ($items:expr, $field:ident, $valid:expr, $label:literal) => {
            validate_unique_targets!($items, $field, $valid, $label, [id()], |item, _target| {});
        };
        ($items:expr, $field:ident, $valid:expr, $label:literal,
         [$($id:tt)+], |$item:ident, $target:pat_param| $validate:block) => {{
            let mut seen = BTreeSet::new();
            for $item in $items {
                let $target = $valid.get(&$item.$field).copied().ok_or_else(|| {
                    CodecError::InvalidInput(format!(
                        "F3D {} metadata {} targets missing entity {}",
                        $label, $item.$($id)+, $item.$field
                    ))
                })?;
                if !seen.insert(&$item.$field) {
                    return Err(CodecError::InvalidInput(format!(
                        "multiple F3D {} records target {}",
                        $label, $item.$field
                    )));
                }
                $validate
            }
        }};
    }
    validate_unique_targets!(&native.body_native_keys, body, bodies, "body-native-key");
    validate_unique_targets!(
        &native.body_visibilities,
        body,
        bodies,
        "body-visibility",
        [id()],
        |visibility, (ordinal, body)| {
            if body.visible != Some(visibility.visible) {
                return Err(CodecError::InvalidInput(format!(
                    "F3D body visibility {} conflicts with body {} visibility",
                    visibility.id(),
                    visibility.body
                )));
            }
            let emitted_key = source_less_body_key(attributes, body, ordinal)?;
            if u64::try_from(emitted_key).ok() != Some(visibility.asm_body_key()) {
                return Err(CodecError::InvalidInput(format!(
                    "F3D body visibility {} uses an ASM key different from body {}",
                    visibility.id(),
                    visibility.body
                )));
            }
        }
    );
    validate_unique_targets!(
        &native.transform_hints,
        body,
        bodies,
        "transform-hint",
        [id()],
        |hints, (_, body)| {
            if body.transform.is_none() {
                return Err(CodecError::InvalidInput(format!(
                    "F3D transform hints {} target a body without a transform",
                    hints.id()
                )));
            }
        }
    );
    validate_unique_targets!(&native.edge_continuities, edge, edges, "edge-continuity");
    validate_unique_targets!(&native.edge_ownerships, edge, edges, "edge-ownership");
    validate_unique_targets!(
        &native.vertex_ownerships,
        vertex,
        vertices,
        "vertex-ownership"
    );
    validate_unique_targets!(&native.face_sidedness, face, faces, "face-sidedness");
    validate_unique_targets!(
        &native.tolerant_edge_tails,
        edge,
        edges,
        "tolerant-edge",
        [id()],
        |tail, edge| {
            if edge.tolerance.is_none() {
                return Err(CodecError::InvalidInput(format!(
                    "F3D tolerant-edge metadata {} requires a tolerant edge",
                    tail.id()
                )));
            }
        }
    );
    validate_unique_targets!(
        &native.tolerant_vertex_tails,
        vertex,
        vertices,
        "tolerant-vertex",
        [id()],
        |tail, vertex| {
            if vertex.tolerance.is_some()
                != matches!(
                    tail.evaluated_slot,
                    EvaluatedToleranceSlot::Evaluated { .. }
                )
            {
                return Err(CodecError::InvalidInput(format!(
                    "F3D tolerant-vertex metadata {} requires finite fields and a tolerant vertex",
                    tail.id()
                )));
            }
        }
    );
    let mut wire_record_indices = BTreeSet::new();
    for wire in &native.wire_topologies {
        let shell = shells.get(&wire.shell).copied().ok_or_else(|| {
            CodecError::InvalidInput(format!(
                "F3D wire-topology metadata {} targets missing entity {}",
                wire.id(),
                wire.shell
            ))
        })?;
        if !wire_record_indices.insert(wire.record_index) {
            return Err(CodecError::InvalidInput(format!(
                "multiple F3D wire-topology records use native index {}",
                wire.record_index
            )));
        }
        let member_form_is_valid = match &wire.members {
            cadmpeg_asm::brep::records::WireMembers::Edges(edges) => {
                !edges.is_empty() && edges.iter().all(|edge| shell.wire_edges().contains(edge))
            }
            cadmpeg_asm::brep::records::WireMembers::Vertex(vertex) => {
                shell.free_vertices().contains(vertex)
            }
        };
        if !member_form_is_valid {
            return Err(CodecError::InvalidInput(format!(
                "F3D wire metadata {} has invalid edge-ring or isolated-vertex membership",
                wire.id()
            )));
        }
    }
    Ok(())
}

pub(super) fn validate_source_less_body_kinds(
    model: &cadmpeg_ir::document::Model,
) -> Result<(), CodecError> {
    let edges = model
        .edges
        .iter()
        .map(|edge| (&edge.id, edge))
        .collect::<HashMap<_, _>>();
    let curves = model
        .curves
        .iter()
        .map(|curve| (&curve.id, curve))
        .collect::<HashMap<_, _>>();
    // A curveless edge is written with a null curve reference, so a decoder
    // reads it back as collapsed when both endpoints name one vertex.
    let collapsed = |edge: &cadmpeg_ir::ids::EdgeId| {
        edges.get(edge).is_some_and(|edge| {
            edge.start == edge.end
                && edge.curve().is_none_or(|curve| {
                    curves.get(curve).is_some_and(|curve| {
                        matches!(
                            curve.geometry,
                            CurveGeometry::Solved(SolvedCurveGeometry::Degenerate(_))
                        )
                    })
                })
        })
    };
    for body in &model.bodies {
        let shell_ids = model
            .regions
            .iter()
            .filter(|region| region.body == body.id)
            .flat_map(|region| &region.shells)
            .collect::<BTreeSet<_>>();
        let face_ids = model
            .shells
            .iter()
            .filter(|shell| shell_ids.contains(&shell.id))
            .flat_map(cadmpeg_ir::topology::Shell::faces)
            .collect::<BTreeSet<_>>();
        let has_wires = model
            .shells
            .iter()
            .filter(|shell| shell_ids.contains(&shell.id))
            .any(|shell| !shell.wire_edges().is_empty() || !shell.free_vertices().is_empty());
        let loop_ids = model
            .faces
            .iter()
            .filter(|face| face_ids.contains(&face.id))
            .flat_map(|face| &face.loops)
            .collect::<BTreeSet<_>>();
        let coedge_ids = model
            .loops
            .iter()
            .filter(|loop_| loop_ids.contains(&loop_.id))
            .flat_map(cadmpeg_ir::topology::Loop::coedges)
            .collect::<BTreeSet<_>>();
        let derived = if face_ids.is_empty() {
            cadmpeg_ir::topology::BodyKind::Wire
        } else if has_wires {
            cadmpeg_ir::topology::BodyKind::General
        } else {
            cadmpeg_asm::brep::geometry::faced_body_kind(
                model
                    .coedges
                    .iter()
                    .filter(|coedge| coedge_ids.contains(&coedge.id))
                    .map(|coedge| (&coedge.edge, coedge.sense)),
                collapsed,
            )
        };
        if body.kind != derived {
            return Err(CodecError::InvalidInput(format!(
                "body {} declares {:?} but its incidence graph is {:?}",
                body.id, body.kind, derived
            )));
        }
    }
    Ok(())
}

/// Resolved wire-ownership ordinals of one body.
#[derive(Clone, Copy)]
pub(super) struct WireBodyOwnership {
    /// Ordinal of the body's first region in `model.regions`.
    pub(super) first_region: usize,
    /// Ordinal of that region's first shell in `model.shells`.
    pub(super) first_shell: usize,
}

/// Resolved wire-ownership ordinals of one region.
#[derive(Clone, Copy)]
pub(super) struct WireRegionOwnership {
    /// Ordinal of the owning body in `model.bodies`.
    pub(super) body: usize,
    /// Ordinal of the next region of the same body in `model.regions`.
    pub(super) next_region: Option<usize>,
    /// Ordinal of the region's first shell in `model.shells`.
    pub(super) first_shell: usize,
}

/// Resolved wire-ownership ordinals of one shell.
#[derive(Clone, Copy)]
pub(super) struct WireShellOwnership {
    /// Ordinal of the owning region in `model.regions`.
    pub(super) region: usize,
    /// Ordinal of the next shell of the same region in `model.shells`.
    pub(super) next_shell: Option<usize>,
}

/// Proof that [`validate_source_less_wire_ownership`] ran against the borrowed
/// `CadIr`. The private fields keep construction inside this module, so the wire
/// encoder reads resolved ordinals instead of re-searching the model and
/// asserting that every lookup succeeds.
pub(in crate::writer::generate) struct WireOwnershipValidated {
    bodies: Vec<WireBodyOwnership>,
    regions: Vec<WireRegionOwnership>,
    shells: Vec<WireShellOwnership>,
}

impl WireOwnershipValidated {
    /// Per-body ordinals in `model.bodies` order.
    pub(super) fn bodies(&self) -> &[WireBodyOwnership] {
        &self.bodies
    }

    /// Per-region ordinals in `model.regions` order.
    pub(super) fn regions(&self) -> &[WireRegionOwnership] {
        &self.regions
    }

    /// Per-shell ordinals in `model.shells` order.
    pub(super) fn shells(&self) -> &[WireShellOwnership] {
        &self.shells
    }
}

pub(super) fn validate_source_less_wire_ownership(
    target: &CadIr,
) -> Result<WireOwnershipValidated, CodecError> {
    let model = &target.model;
    let inconsistent =
        || CodecError::Malformed("source-less F3D wire ownership is inconsistent".into());
    let region_ordinal =
        |id: &cadmpeg_ir::ids::RegionId| model.regions.iter().position(|region| region.id == *id);
    let shell_ordinal =
        |id: &cadmpeg_ir::ids::ShellId| model.shells.iter().position(|shell| shell.id == *id);

    let mut regions = Vec::new();
    for region in &model.regions {
        let (body, owner, position) = model
            .bodies
            .iter()
            .enumerate()
            .find_map(|(ordinal, body)| {
                if body.id != region.body {
                    return None;
                }
                body.regions
                    .iter()
                    .position(|id| *id == region.id)
                    .map(|position| (ordinal, body, position))
            })
            .ok_or_else(inconsistent)?;
        let next_region = owner
            .regions
            .get(position + 1)
            .map(|id| region_ordinal(id).ok_or_else(inconsistent))
            .transpose()?;
        let first_shell = region
            .shells
            .first()
            .and_then(&shell_ordinal)
            .ok_or_else(inconsistent)?;
        if region.shells.iter().any(|id| {
            !model
                .shells
                .iter()
                .any(|shell| shell.id == *id && shell.region == region.id)
        }) {
            return Err(inconsistent());
        }
        regions.push(WireRegionOwnership {
            body,
            next_region,
            first_shell,
        });
    }

    let mut bodies = Vec::new();
    for body in &model.bodies {
        let first_region = body
            .regions
            .first()
            .and_then(&region_ordinal)
            .ok_or_else(inconsistent)?;
        if body.regions.iter().any(|id| {
            !model
                .regions
                .iter()
                .any(|region| region.id == *id && region.body == body.id)
        }) {
            return Err(inconsistent());
        }
        bodies.push(WireBodyOwnership {
            first_region,
            first_shell: regions[first_region].first_shell,
        });
    }

    let mut shells = Vec::new();
    for shell in &model.shells {
        let (region, owner, position) = model
            .regions
            .iter()
            .enumerate()
            .find_map(|(ordinal, region)| {
                if region.id != shell.region {
                    return None;
                }
                region
                    .shells
                    .iter()
                    .position(|id| *id == shell.id)
                    .map(|position| (ordinal, region, position))
            })
            .ok_or_else(inconsistent)?;
        let next_shell = owner
            .shells
            .get(position + 1)
            .map(|id| shell_ordinal(id).ok_or_else(inconsistent))
            .transpose()?;
        shells.push(WireShellOwnership { region, next_shell });
    }

    Ok(WireOwnershipValidated {
        bodies,
        regions,
        shells,
    })
}

#[cfg(test)]
mod tests {
    use super::{validate_source_less_design_links, F3dNative};
    use crate::writer::generate::attributes::AttributeIndex;
    use cadmpeg_asm::brep::records::identity::NativeRecordNamespace;
    use cadmpeg_asm::brep::records::{EvaluatedToleranceSlot, TolerantVertexTail};
    use cadmpeg_core::CodecError;
    use cadmpeg_ir::scalar::PositiveReal;

    #[test]
    fn tolerant_vertex_precondition_rejects_unset_with_a_neutral_tolerance() {
        let mut target = cadmpeg_ir::examples::unit_cube().expect("unit cube fixture is admitted");
        target.model.vertices[0].tolerance = Some(PositiveReal::new(0.025).unwrap());
        let native = F3dNative {
            tolerant_vertex_tails: vec![TolerantVertexTail {
                source_namespace: NativeRecordNamespace::new(crate::ids::ID_FORMAT),
                record_index: 0,
                vertex: target.model.vertices[0].id.clone(),
                leading_tolerances: [
                    cadmpeg_ir::scalar::FiniteReal::new(-1.0).unwrap(),
                    cadmpeg_ir::scalar::FiniteReal::new(-1.0).unwrap(),
                ],
                evaluated_slot: EvaluatedToleranceSlot::Unset { trailing: Some(0) },
            }],
            ..F3dNative::default()
        };
        let attributes = AttributeIndex::new(&target, &native);
        let error = validate_source_less_design_links(&target, &native, &attributes)
            .expect_err("unset native evaluation must not coexist with a neutral tolerance");
        assert!(matches!(error, CodecError::InvalidInput(_)));
        assert!(error
            .to_string()
            .contains("requires finite fields and a tolerant vertex"));
    }
}
