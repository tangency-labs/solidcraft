// SPDX-License-Identifier: Apache-2.0
//! Project exact local component operations into neutral product structure.

pub(crate) mod owners;
pub(crate) mod paths;

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureOperation};
use cadmpeg_ir::ids::{BodyId, OccurrenceId, ProductDefinitionId};
use cadmpeg_ir::products::{
    Occurrence, OccurrenceParent, ProductDefinition, ProductDefinitionKind, PrototypeReference,
};
use cadmpeg_ir::report::loss::LossNote;

use crate::design::decode::components::{ComponentDefinition, StreamComponents};
use crate::loss::F3dLossCode;
use crate::records::bodies::DesignBodyBinding;
use crate::records::component_path::DesignComponentPath;
use crate::records::entity_header::DesignFeatureTimeline;
use crate::records::feature::{
    assembly::DesignAssemblyOperandQualifier, assembly_features::DesignComponentOccurrence,
    scope::DesignParameterScope,
};
use crate::records::mesh::DesignRelaxedGuidText;

const IDENTITY: [[f64; 4]; 4] = [
    [1.0, 0.0, 0.0, 0.0],
    [0.0, 1.0, 0.0, 0.0],
    [0.0, 0.0, 1.0, 0.0],
    [0.0, 0.0, 0.0, 1.0],
];

/// Decoded records that define and place local component occurrences.
pub(crate) struct LocalComponentSources<'a> {
    /// Design parameter scopes, including component operations.
    pub(crate) scopes: &'a [DesignParameterScope],
    /// Local component-occurrence carriers.
    pub(crate) occurrences: &'a [DesignComponentOccurrence],
    /// Component-path carriers that give world poses to nested occurrences.
    pub(crate) paths: &'a [DesignComponentPath],
    /// Component definitions and root component of each Design stream.
    pub(crate) components: &'a BTreeMap<String, StreamComponents>,
    /// Design body-map pairs.
    pub(crate) body_bindings: &'a [DesignBodyBinding],
    /// Solved model bodies, or `None` when bodies are projected after product structure.
    pub(crate) bodies: Option<&'a BTreeSet<BodyId>>,
    pub(crate) hidden_bodies: Option<&'a BTreeSet<BodyId>>,
    /// First free ordinal among root occurrences already in the model.
    pub(crate) root_ordinal_start: usize,
    pub(crate) limits: cadmpeg_core::decode::ResourceLimits,
}

/// Local product structure projected from component occurrences.
#[derive(Debug)]
pub(crate) struct LocalComponentProjection {
    pub(crate) definitions: Vec<ProductDefinition>,
    pub(crate) occurrences: Vec<Occurrence>,
    pub(crate) losses: Vec<LossNote>,
}

/// Project local component definitions and their placed occurrences.
///
/// Component operations supply the occurrences and placements they prove.
/// Local child trees repeat beneath each containing instance. A carrier whose
/// container or definition cannot resolve stays unplaced and is reported.
#[cfg(test)]
pub(crate) fn project_local_components(
    sources: &LocalComponentSources<'_>,
) -> Result<LocalComponentProjection, cadmpeg_core::CodecError> {
    project_local_components_with_types(sources, &[])
}

pub(crate) fn project_local_components_with_types(
    sources: &LocalComponentSources<'_>,
    types: &[crate::records::entity_header::SegmentType],
) -> Result<LocalComponentProjection, cadmpeg_core::CodecError> {
    let mut projector = Projector::new(sources);
    let mut types_by_stream = std::collections::HashMap::new();
    for scope in sources.scopes {
        let stream = crate::ids::native_stream(&scope.id).unwrap_or_default();
        let relation_only = scope.assembly_alignment().is_some_and(|alignment| {
            alignment.frame_sense.is_some()
                && super::decode::scopes::assembly_v3::relation_only(
                    scope,
                    alignment,
                    types_by_stream.entry(stream).or_insert_with(|| {
                        stream
                            .strip_prefix("f3d:")
                            .and_then(crate::ids::decode_identity_key_component)
                            .map(|entry| super::decode::meta::stream_types_by_entity(types, &entry))
                            .unwrap_or_default()
                    }),
                )
        });
        if let Some(qualifiers) = scope
            .assembly_alignment()
            .filter(|_| !relation_only)
            .and_then(
            super::super::records::feature::assembly::DesignAssemblyAlignment::operand_qualifiers,
        ) {
            for qualifier in &qualifiers {
                let DesignAssemblyOperandQualifier::OccurrencePath { path } = qualifier else {
                    continue;
                };
                let Some(guid) = path.occurrence_guids().first() else {
                    continue;
                };
                let key = guid.value.as_str().to_ascii_lowercase();
                let root = projector
                    .native_by_guid
                    .get(&(stream.to_owned(), key))
                    .copied()
                    .flatten();
                let Some(root) = root else {
                    continue;
                };
                projector.place(
                    stream,
                    &root.component_guid,
                    &root.occurrence_guid,
                    root.transform()
                        .map_or(IDENTITY, |frame| frame.value.into()),
                )?;
            }
        }
        if let Some(operation) = scope.copy_paste_component_operation() {
            projector.place(
                stream,
                &operation.component_guid,
                &operation.source_occurrence_guid,
                operation.source_transform,
            )?;
            projector.place(
                stream,
                &operation.component_guid,
                &operation.copied_occurrence_guid,
                operation.copied_transform,
            )?;
        }
        if let Some(construction) = scope.derived_instance_construction() {
            projector.place(
                stream,
                &construction.component_guid,
                &construction.occurrence_guid,
                construction.transform,
            )?;
        }
        let Some(
            crate::records::feature::patterns::DesignRectangularPatternInstances::Components {
                component_guid,
                seed,
                generated,
            },
        ) = scope
            .rectangular_pattern_construction()
            .and_then(|construction| construction.instances())
        else {
            continue;
        };
        for occurrence in std::iter::once(seed).chain(generated) {
            projector.place(
                stream,
                component_guid,
                &occurrence.occurrence_guid,
                occurrence.instance.transform.value,
            )?;
        }
    }
    projector.place_root_contained_carriers()?;
    projector.attach_root_occurrences();
    projector.expand_nested_carriers()?;
    projector.apply_path_poses()?;
    projector.finish()
}

/// Reason a local occurrence carrier stays unplaced.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Unplaced {
    RepeatedGuid,
    NoRootComponent,
    OwnerUnbound,
    OwnerCycle,
    ExpansionLimit,
    DefinitionUnbound,
    OwnerUninstanced,
    BodyOutsideComponents,
    BodiesPending,
}

impl Unplaced {
    fn reason(self) -> &'static str {
        match self {
            Self::RepeatedGuid => "the occurrence GUID repeats in its Design stream",
            Self::NoRootComponent => {
                "the Design stream has no unique root instance bound to a component naming space"
            }
            Self::OwnerUnbound => "the owner entity joins no unique component definition",
            Self::OwnerCycle => "the component owner graph has a cycle",
            Self::ExpansionLimit => "the occurrence expansion exceeds the decode limit",
            Self::OwnerUninstanced => "the containing component has no resolved instance path",
            Self::DefinitionUnbound => {
                "the component definition record stores no naming-space binding"
            }
            Self::BodyOutsideComponents => {
                "a model body has no component definition, so a product structure would omit it"
            }
            Self::BodiesPending => {
                "the document projects its bodies after product structure, so component body ownership is not known"
            }
        }
    }
}

struct Projector<'a> {
    sources: &'a LocalComponentSources<'a>,
    native_by_guid: BTreeMap<(String, String), Option<&'a DesignComponentOccurrence>>,
    guid_stream_counts: BTreeMap<String, usize>,
    definitions: BTreeMap<String, ProductDefinition>,
    occurrences: BTreeMap<String, (Occurrence, Option<&'a DesignComponentOccurrence>)>,
    unplaced: BTreeMap<Unplaced, usize>,
    rejected: BTreeSet<&'a str>,
    /// Root-contained carriers placed at their component-path pose.
    current_poses: usize,
    /// Nested occurrences placed at their component-path pose.
    path_poses: PathPoseOutcome,
}

/// Same-pose tolerance between two component-path carriers, in the
/// centimetres and unit rotation entries of the stored matrices.
const EPS_PATH_POSE_AGREE: f64 = 1.0e-6;
/// A resolved local placement that moves less than this many millimetres, with
/// a linear block within [`EPS_PATH_POSE_LINEAR`], equals the stored one.
const EPS_PATH_POSE_SHIFT_MM: f64 = 1.0e-4;
const EPS_PATH_POSE_LINEAR: f64 = 1.0e-6;

/// Relative tolerance between two definition-local poses: rotation entries
/// absolutely, translation scaled to its magnitude (at least 1 mm).
const EPS_PATH_POSE_LOCAL_REL: f64 = 1.0e-6;
/// Search steps allowed when assigning carriers to instances.
const MAX_PATH_ASSIGNMENT_STEPS: usize = 100_000;

/// One unit of work for the component-path pose pass.
enum PathJob {
    Single([[f64; 4]; 4]),
    Group(usize),
}

/// A candidate instance ordinal with the definition-local pose it gives.
type LocalChoice = (usize, [[f64; 4]; 3]);

/// Exhaustive search for the full assignments of carriers to distinct
/// instances under which every definition-local pose agrees.
struct AssignmentSearch<'s> {
    options: &'s [Vec<LocalChoice>],
    order: &'s [usize],
    chosen: Vec<LocalChoice>,
    steps: usize,
    /// Instance ordinals per carrier, in `order`.
    found: Vec<Vec<usize>>,
}

impl AssignmentSearch<'_> {
    fn descend(&mut self) {
        let depth = self.chosen.len();
        if depth == self.order.len() {
            self.found
                .push(self.chosen.iter().map(|(o, _)| *o).collect());
            return;
        }
        for &(ordinal, pose) in &self.options[self.order[depth]] {
            self.steps += 1;
            if self.steps > MAX_PATH_ASSIGNMENT_STEPS || self.found.len() > 1 {
                return;
            }
            let fresh = self.chosen.iter().all(|(used, _)| *used != ordinal);
            let agrees = self
                .chosen
                .first()
                .is_none_or(|(_, reference)| local_poses_agree(reference, &pose));
            if fresh && agrees {
                self.chosen.push((ordinal, pose));
                self.descend();
                self.chosen.pop();
            }
        }
    }
}

fn local_poses_agree(left: &[[f64; 4]; 3], right: &[[f64; 4]; 3]) -> bool {
    let scale = (0..3)
        .map(|row| left[row][3].powi(2))
        .sum::<f64>()
        .sqrt()
        .max(1.0);
    (0..3).all(|row| {
        (0..4).all(|column| {
            let limit = if column == 3 {
                EPS_PATH_POSE_LOCAL_REL * scale
            } else {
                EPS_PATH_POSE_LOCAL_REL
            };
            (left[row][column] - right[row][column]).abs() <= limit
        })
    })
}

/// What the component-path pose pass did.
#[derive(Default)]
struct PathPoseOutcome {
    /// Carriers that name no unique expanded occurrence.
    unresolved: usize,
    /// Occurrences that two carriers pose differently.
    disagreeing: usize,
    /// Occurrences whose resolved placement differs from the stored one.
    changed: usize,
    /// Largest translation change, in millimetres.
    largest_shift_mm: f64,
    /// Carriers, occurrences and targets placed by the unique consistent
    /// assignment.
    inferred_carriers: usize,
    inferred_instances: usize,
    inferred_groups: usize,
    /// Carriers of the occurrences that took a path pose.
    applied_carriers: BTreeSet<String>,
}

/// One expanded nested occurrence, for path resolution.
struct PathInstance {
    key: String,
    stream: String,
    record: u64,
    /// Records of the containing instances' carriers, innermost first.
    ancestors: Vec<u64>,
}

struct ClosingOccurrencePath<'a> {
    references: cadmpeg_core::decode::View<'a>,
    count: usize,
}

fn local_path_reference(view: &mut cadmpeg_core::decode::View<'_>) -> Option<u64> {
    if view.u8()? != 1 {
        return None;
    }
    let record = view.u64_le()?;
    (view.u16_le()? == 0 && record != 0).then_some(record)
}

fn closing_occurrence_path(path: &DesignComponentPath) -> Option<ClosingOccurrencePath<'_>> {
    use crate::layout::indexed_design_record_header as header;
    use crate::layout::local_component_occurrence_prefix as occurrence;
    use cadmpeg_core::decode::View;
    use std::mem::{size_of, size_of_val};

    const LOCAL_REFERENCE_BYTES: usize =
        occurrence::PATH_DISCRIMINATOR - occurrence::COMPONENT_REFERENCE;
    let mut frame = View::over_retained(path.frame_bytes.as_ref());
    let length = frame.u32_le()?;
    let length = frame.counted(u64::from(length), 1)?.get();
    if length != header::RECORD_INDEX - header::CLASS_TAG
        || !frame.take(length)?.iter().all(u8::is_ascii_digit)
        || frame.u64_le()? != path.carrier_record_index
    {
        return None;
    }
    let length = frame.u32_le()?;
    let length = frame.counted(u64::from(length), 1)?.get();
    frame.skip(length)?;
    if frame.u8()? != 1 {
        return None;
    }
    let count = frame.u32_le()?;
    let count = frame
        .counted(u64::from(count), LOCAL_REFERENCE_BYTES + size_of::<u32>())?
        .get();
    if count != path.elements.len() {
        return None;
    }
    for element in &path.elements {
        if local_path_reference(&mut frame)? != element.record_index
            || frame.u32_le()? != element.discriminator
        {
            return None;
        }
    }
    if frame.u8()? != 0 {
        return None;
    }
    match (frame.u8()?, path.transform) {
        (0, Some(_)) => frame.skip(size_of_val(&IDENTITY))?,
        (1, None) => {}
        _ => return None,
    }
    let count = frame.u32_le()?;
    let count = frame
        .counted(u64::from(count), LOCAL_REFERENCE_BYTES)?
        .get();
    let end = frame
        .position()
        .checked_add(count.checked_mul(LOCAL_REFERENCE_BYTES)?)?;
    let references = frame.child(frame.position(), end)?;
    let mut check = references;
    for _ in 0..count {
        local_path_reference(&mut check)?;
    }
    Some(ClosingOccurrencePath { references, count })
}

impl ClosingOccurrencePath<'_> {
    fn names(&self, instance: &PathInstance) -> bool {
        if instance.ancestors.len().checked_add(1) != Some(self.count) {
            return false;
        }
        let mut references = self.references;
        instance
            .ancestors
            .iter()
            .rev()
            .copied()
            .chain(std::iter::once(instance.record))
            .all(|record| local_path_reference(&mut references) == Some(record))
    }
}

impl<'a> Projector<'a> {
    fn new(sources: &'a LocalComponentSources<'a>) -> Self {
        let mut native_by_guid = BTreeMap::new();
        for occurrence in sources.occurrences {
            match native_by_guid.entry((
                crate::ids::native_stream(&occurrence.id)
                    .unwrap_or_default()
                    .to_owned(),
                occurrence.occurrence_guid.as_str().to_ascii_lowercase(),
            )) {
                std::collections::btree_map::Entry::Vacant(entry) => {
                    entry.insert(Some(occurrence));
                }
                std::collections::btree_map::Entry::Occupied(mut entry) => {
                    *entry.get_mut() = None;
                }
            }
        }
        let mut guid_stream_counts = BTreeMap::new();
        for (_, key) in native_by_guid.keys() {
            *guid_stream_counts.entry(key.clone()).or_default() += 1;
        }
        Self {
            sources,
            native_by_guid,
            guid_stream_counts,
            definitions: BTreeMap::new(),
            occurrences: BTreeMap::new(),
            unplaced: BTreeMap::new(),
            rejected: BTreeSet::new(),
            current_poses: 0,
            path_poses: PathPoseOutcome::default(),
        }
    }

    fn occurrence_id(
        &self,
        stream: &str,
        key: &impl crate::records::mesh::OccurrenceIdentity,
    ) -> OccurrenceId {
        if self
            .guid_stream_counts
            .get(&key.identity_text().to_ascii_lowercase())
            .is_some_and(|count| *count > 1)
        {
            crate::ids::stream_component_occurrence_id(stream, key)
        } else {
            crate::ids::neutral_component_occurrence_id(key)
        }
    }

    fn root_occurrence_id(&self, stream: &str, root: &ComponentDefinition) -> OccurrenceId {
        let repeated = self
            .sources
            .components
            .values()
            .filter_map(|components| {
                components
                    .root_component
                    .and_then(|record| components.roots.get(&record))
            })
            .filter(|other| other.context_uuid == root.context_uuid)
            .take(2)
            .count()
            > 1;
        if repeated {
            crate::ids::stream_root_occurrence_id(stream, &root.context_uuid)
        } else {
            crate::ids::neutral_root_occurrence_id(&root.context_uuid)
        }
    }

    fn stream_components(
        &self,
        carrier: &DesignComponentOccurrence,
    ) -> Option<(&'a str, &'a StreamComponents)> {
        let (stream, components) = self
            .sources
            .components
            .get_key_value(crate::ids::native_stream(&carrier.id)?)?;
        Some((stream.as_str(), components))
    }

    fn carrier_definition(
        &self,
        carrier: &DesignComponentOccurrence,
    ) -> Option<(&'a str, &'a ComponentDefinition)> {
        let (stream, components) = self.stream_components(carrier)?;
        Some((
            stream,
            components.roots.get(&carrier.component_record_index)?,
        ))
    }

    /// Place one occurrence once. The first placement of an occurrence wins.
    fn place(
        &mut self,
        stream: &str,
        component_guid: &DesignRelaxedGuidText,
        occurrence_guid: &impl crate::records::mesh::OccurrenceIdentity,
        transform: impl Into<[[f64; 4]; 4]>,
    ) -> Result<(), cadmpeg_core::CodecError> {
        let occurrence_id = self.occurrence_id(stream, occurrence_guid);
        if self.occurrences.contains_key(occurrence_id.as_str()) {
            return Ok(());
        }
        let transform = neutral_transform(transform)?;
        let carrier = self
            .native_by_guid
            .get(&(
                stream.to_owned(),
                occurrence_guid.identity_text().to_ascii_lowercase(),
            ))
            .copied()
            .flatten();
        let definition = match carrier.and_then(|carrier| self.carrier_definition(carrier)) {
            Some((stream, root)) => self.naming_space_definition(stream, root),
            None => self.guid_definition(component_guid),
        };
        let key = occurrence_id.as_str().to_owned();
        self.occurrences.insert(
            key,
            (
                Occurrence {
                    id: occurrence_id,
                    prototype: PrototypeReference::Local { definition },
                    parent: OccurrenceParent::Root {},
                    ordinal: 0,
                    transform,
                    linked_prototype: None,
                    scale: [cadmpeg_ir::scalar::FiniteReal::ONE; 3],
                    name: None,
                    visible: None,
                    link: None,
                    native_ref: carrier.map(|carrier| carrier.id.clone()),
                },
                carrier,
            ),
        );
        Ok(())
    }

    /// Place every carrier that the root component contains and that no
    /// component operation placed, at the carrier's own transform.
    fn place_root_contained_carriers(&mut self) -> Result<(), cadmpeg_core::CodecError> {
        let bodies_owned = match self.sources.bodies {
            None => Err(Unplaced::BodiesPending),
            Some(_) if !self.every_body_has_a_definition() => Err(Unplaced::BodyOutsideComponents),
            Some(_) => Ok(()),
        };
        for carrier in self.sources.occurrences {
            if self
                .stream_components(carrier)
                .is_some_and(|(_, components)| {
                    components
                        .root_component
                        .and_then(|root| components.roots.get(&root))
                        .is_some_and(|root| root.root_entity != carrier.owner_entity)
                })
            {
                continue;
            }
            let occurrence_id = self.occurrence_id(
                crate::ids::native_stream(&carrier.id).unwrap_or_default(),
                &carrier.occurrence_guid,
            );
            if self.occurrences.contains_key(occurrence_id.as_str()) {
                continue;
            }
            let unplaced = if self
                .native_by_guid
                .get(&(
                    crate::ids::native_stream(&carrier.id)
                        .unwrap_or_default()
                        .to_owned(),
                    carrier.occurrence_guid.as_str().to_ascii_lowercase(),
                ))
                .copied()
                .flatten()
                .is_none()
            {
                Some(Unplaced::RepeatedGuid)
            } else {
                match self.stream_components(carrier).and_then(|(_, components)| {
                    components
                        .root_component
                        .and_then(|root| components.roots.get(&root))
                }) {
                    None => Some(Unplaced::NoRootComponent),
                    Some(_) if self.carrier_definition(carrier).is_none() => {
                        Some(Unplaced::DefinitionUnbound)
                    }
                    Some(_) => bodies_owned.err(),
                }
            };
            if let Some(reason) = unplaced {
                *self.unplaced.entry(reason).or_default() += 1;
                self.rejected.insert(carrier.id.as_str());
                continue;
            }
            // The root occurrence is the identity, so a root-contained
            // occurrence takes its component-path pose as its own placement.
            let transform = match &carrier.current_pose {
                Some(pose) => {
                    self.current_poses += 1;
                    pose.transform
                        .map_or(IDENTITY, crate::records::xref::XrefPlacementTransform::rows)
                }
                None => carrier
                    .transform()
                    .map_or(IDENTITY, |frame| frame.value.into()),
            };
            self.place(
                crate::ids::native_stream(&carrier.id).unwrap_or_default(),
                &carrier.component_guid,
                &carrier.occurrence_guid,
                transform,
            )?;
        }
        Ok(())
    }

    /// Place the occurrences contained by each stream's root component inside
    /// one identity root occurrence of that component.
    fn attach_root_occurrences(&mut self) {
        for (stream, components) in self.sources.components {
            let Some(root) = components
                .root_component
                .and_then(|root| components.roots.get(&root))
            else {
                continue;
            };
            let root_occurrence = self.root_occurrence_id(stream, root);
            let mut contained = false;
            for (occurrence, carrier) in self.occurrences.values_mut() {
                if carrier.is_some_and(|carrier| {
                    crate::ids::native_stream(&carrier.id) == Some(stream.as_str())
                        && carrier.owner_entity == root.root_entity
                }) {
                    occurrence.parent = OccurrenceParent::Occurrence {
                        occurrence: root_occurrence.clone(),
                    };
                    contained = true;
                }
            }
            if !contained {
                continue;
            }
            let definition = self.naming_space_definition(stream, root);
            let key = root_occurrence.as_str().to_owned();
            self.occurrences.insert(
                key,
                (
                    Occurrence {
                        id: root_occurrence,
                        prototype: PrototypeReference::Local { definition },
                        parent: OccurrenceParent::Root {},
                        ordinal: 0,
                        transform: cadmpeg_ir::transform::Transform::identity(),
                        linked_prototype: None,
                        scale: [cadmpeg_ir::scalar::FiniteReal::ONE; 3],
                        name: None,
                        visible: None,
                        link: None,
                        native_ref: None,
                    },
                    None,
                ),
            );
        }
    }

    fn expand_nested_carriers(&mut self) -> Result<(), cadmpeg_core::CodecError> {
        let mut operation_placements = BTreeMap::new();
        for carrier in self.sources.occurrences {
            let nested = self
                .stream_components(carrier)
                .is_some_and(|(_, components)| {
                    components
                        .root_component
                        .and_then(|record| components.roots.get(&record))
                        .is_some_and(|root| root.root_entity != carrier.owner_entity)
                });
            if nested {
                let id = self.occurrence_id(
                    crate::ids::native_stream(&carrier.id).unwrap_or_default(),
                    &carrier.occurrence_guid,
                );
                if let Some((occurrence, _)) = self.occurrences.remove(id.as_str()) {
                    operation_placements.insert(carrier.id.as_str(), occurrence.transform);
                }
            }
        }
        for (stream, components) in self.sources.components {
            let Some(root) = components
                .root_component
                .and_then(|key| components.roots.get(&key))
            else {
                continue;
            };
            let mut owners = BTreeMap::new();
            for definition in components.roots.values() {
                owners
                    .entry(definition.root_entity)
                    .and_modify(|unique| *unique = false)
                    .or_insert(true);
            }
            let mut children = BTreeMap::<u64, Vec<&DesignComponentOccurrence>>::new();
            for carrier in
                self.sources.occurrences.iter().filter(|carrier| {
                    crate::ids::native_stream(&carrier.id) == Some(stream.as_str())
                })
            {
                let key = (
                    stream.clone(),
                    carrier.occurrence_guid.as_str().to_ascii_lowercase(),
                );
                let rejection = if self.native_by_guid.get(&key).copied().flatten().is_none() {
                    Some(Unplaced::RepeatedGuid)
                } else if owners.get(&carrier.owner_entity) != Some(&true) {
                    Some(Unplaced::OwnerUnbound)
                } else {
                    None
                };
                if let Some(reason) = rejection {
                    if self.rejected.insert(carrier.id.as_str()) {
                        *self.unplaced.entry(reason).or_default() += 1;
                    }
                    self.occurrences.remove(
                        self.occurrence_id(
                            crate::ids::native_stream(&carrier.id).unwrap_or_default(),
                            &carrier.occurrence_guid,
                        )
                        .as_str(),
                    );
                    continue;
                }
                children
                    .entry(carrier.owner_entity)
                    .or_default()
                    .push(carrier);
            }
            let mut colors = BTreeMap::<u64, u8>::new();
            let mut cyclic = BTreeSet::new();
            for definition in components.roots.values() {
                let mut stack = vec![(definition.root_entity, false)];
                while let Some((entity, exit)) = stack.pop() {
                    if exit {
                        colors.insert(entity, 2);
                        continue;
                    }
                    if colors.contains_key(&entity) {
                        continue;
                    }
                    colors.insert(entity, 1);
                    stack.push((entity, true));
                    for carrier in children.get(&entity).into_iter().flatten().rev() {
                        let Some(target) = components.roots.get(&carrier.component_record_index)
                        else {
                            continue;
                        };
                        if colors.get(&target.root_entity) == Some(&1) {
                            cyclic.insert(carrier.id.as_str());
                        } else if !colors.contains_key(&target.root_entity) {
                            stack.push((target.root_entity, false));
                        }
                    }
                }
            }
            *self.unplaced.entry(Unplaced::OwnerCycle).or_default() += cyclic.len();
            for carrier in children
                .values()
                .flatten()
                .filter(|carrier| cyclic.contains(carrier.id.as_str()))
            {
                self.occurrences.remove(
                    self.occurrence_id(
                        crate::ids::native_stream(&carrier.id).unwrap_or_default(),
                        &carrier.occurrence_guid,
                    )
                    .as_str(),
                );
            }
            let root_id = self.root_occurrence_id(stream, root);
            let mut stack = Vec::new();
            if self.occurrences.contains_key(root_id.as_str()) {
                stack.push((root_id, root.root_entity, 1_u64));
            }
            let mut reached = BTreeSet::new();
            let mut limited = BTreeSet::new();
            while let Some((parent, entity, depth)) = stack.pop() {
                for carrier in children.get(&entity).into_iter().flatten() {
                    if cyclic.contains(carrier.id.as_str()) {
                        continue;
                    }
                    let Some(target) = components.roots.get(&carrier.component_record_index) else {
                        if reached.insert(carrier.id.as_str()) {
                            *self
                                .unplaced
                                .entry(Unplaced::DefinitionUnbound)
                                .or_default() += 1;
                        }
                        continue;
                    };
                    let native_id = self.occurrence_id(
                        crate::ids::native_stream(&carrier.id).unwrap_or_default(),
                        &carrier.occurrence_guid,
                    );
                    let nested = entity != root.root_entity;
                    if !nested && !self.occurrences.contains_key(native_id.as_str()) {
                        continue;
                    }
                    reached.insert(carrier.id.as_str());
                    if depth >= self.sources.limits.max_recursion_depth
                        || cadmpeg_core::decode::u64_from_index(self.occurrences.len())
                            >= self.sources.limits.max_entities
                    {
                        limited.insert(target.root_entity);
                        *self.unplaced.entry(Unplaced::ExpansionLimit).or_default() += 1;
                        continue;
                    }
                    let id = if nested {
                        crate::ids::nested_component_occurrence_id(
                            &parent,
                            &carrier.occurrence_guid,
                        )
                    } else {
                        native_id.clone()
                    };
                    if nested {
                        let transform = operation_placements
                            .get(carrier.id.as_str())
                            .copied()
                            .map_or_else(
                                || {
                                    neutral_transform(
                                        carrier
                                            .transform()
                                            .map_or(IDENTITY, |frame| frame.value.into()),
                                    )
                                },
                                Ok,
                            )?;
                        let definition = self.naming_space_definition(stream, target);
                        self.occurrences.insert(
                            id.as_str().to_owned(),
                            (
                                Occurrence {
                                    id: id.clone(),
                                    prototype: PrototypeReference::Local { definition },
                                    parent: OccurrenceParent::Occurrence {
                                        occurrence: parent.clone(),
                                    },
                                    ordinal: 0,
                                    transform,
                                    linked_prototype: None,
                                    scale: [cadmpeg_ir::scalar::FiniteReal::ONE; 3],
                                    name: None,
                                    visible: None,
                                    link: None,
                                    native_ref: Some(carrier.id.clone()),
                                },
                                Some(carrier),
                            ),
                        );
                    }
                    reached.insert(carrier.id.as_str());
                    stack.push((id, target.root_entity, depth + 1));
                }
            }
            let mut pending = limited.iter().copied().collect::<Vec<_>>();
            while let Some(entity) = pending.pop() {
                for carrier in children.get(&entity).into_iter().flatten() {
                    if let Some(target) = components.roots.get(&carrier.component_record_index) {
                        if limited.insert(target.root_entity) {
                            pending.push(target.root_entity);
                        }
                    }
                }
            }
            for carrier in children.values().flatten() {
                if carrier.owner_entity == root.root_entity {
                    continue;
                }
                let native_id = self.occurrence_id(
                    crate::ids::native_stream(&carrier.id).unwrap_or_default(),
                    &carrier.occurrence_guid,
                );
                self.occurrences.remove(native_id.as_str());
                if !reached.contains(carrier.id.as_str())
                    && !cyclic.contains(carrier.id.as_str())
                    && !limited.contains(&carrier.owner_entity)
                {
                    *self.unplaced.entry(Unplaced::OwnerUninstanced).or_default() += 1;
                }
            }
        }
        self.unplaced.retain(|_, count| *count != 0);
        Ok(())
    }

    /// The product of the placements from the root occurrence down to and
    /// including `key`.
    fn world_of(&self, key: &str) -> Option<cadmpeg_ir::transform::Transform> {
        let mut chain = Vec::new();
        let mut current = key;
        for _ in 0..=self.occurrences.len() {
            let (occurrence, _) = self.occurrences.get(current)?;
            chain.push(occurrence.transform);
            match &occurrence.parent {
                OccurrenceParent::Occurrence { occurrence } => current = occurrence.as_str(),
                OccurrenceParent::Root {} => break,
            }
        }
        chain.into_iter().rev().try_fold(
            cadmpeg_ir::transform::Transform::identity(),
            |world, local| world.compose(local).ok(),
        )
    }

    /// Give each nested occurrence that one component-path carrier names the
    /// carrier's world pose, outermost occurrence first, as
    /// `inverse(parent world) * pose`. A carrier names an occurrence by its
    /// component, its own record and its innermost containing records; it
    /// applies only when exactly one expanded occurrence matches. Two
    /// carriers that pose one occurrence differently apply to neither.
    fn apply_path_poses(&mut self) -> Result<(), cadmpeg_core::CodecError> {
        if self.sources.paths.is_empty() {
            return Ok(());
        }
        let mut carriers = BTreeMap::<(&str, u64), &DesignComponentOccurrence>::new();
        for carrier in self.sources.occurrences {
            if let Some(stream) = crate::ids::native_stream(&carrier.id) {
                carriers.insert((stream, u64::from(carrier.record_index)), carrier);
            }
        }
        let mut instances = Vec::new();
        for (key, (occurrence, carrier)) in &self.occurrences {
            let Some(carrier) = carrier else {
                continue;
            };
            let Some((stream, components)) = self.stream_components(carrier) else {
                continue;
            };
            let nested = components
                .root_component
                .and_then(|record| components.roots.get(&record))
                .is_some_and(|root| root.root_entity != carrier.owner_entity);
            if !nested {
                continue;
            }
            let mut ancestors = Vec::new();
            let mut parent = &occurrence.parent;
            for _ in 0..=self.occurrences.len() {
                let OccurrenceParent::Occurrence { occurrence } = parent else {
                    break;
                };
                let Some((above, above_carrier)) = self.occurrences.get(occurrence.as_str()) else {
                    break;
                };
                if let Some(above_carrier) = above_carrier {
                    ancestors.push(u64::from(above_carrier.record_index));
                }
                parent = &above.parent;
            }
            instances.push(PathInstance {
                key: key.clone(),
                stream: stream.to_owned(),
                record: u64::from(carrier.record_index),
                ancestors,
            });
        }
        let mut resolved = BTreeMap::<usize, Vec<&DesignComponentPath>>::new();
        let mut ambiguous = Vec::<(&DesignComponentPath, Vec<usize>)>::new();
        for path in self.sources.paths {
            use crate::records::component_path::DesignComponentPathElementKind as Kind;
            let stream = crate::ids::native_stream(&path.id).unwrap_or_default();
            let of_kind = |kind: Kind| {
                path.elements
                    .iter()
                    .filter(|element| element.kind == kind)
                    .map(|element| element.record_index)
                    .collect::<Vec<_>>()
            };
            let (components, occurrences) = (of_kind(Kind::Component), of_kind(Kind::Occurrence));
            let distinct = path
                .elements
                .iter()
                .map(|element| element.record_index)
                .collect::<BTreeSet<_>>()
                .len()
                == path.elements.len()
                && path
                    .elements
                    .iter()
                    .all(|element| element.kind != Kind::Other);
            let target = occurrences
                .iter()
                .copied()
                .filter(|record| {
                    carriers.get(&(stream, *record)).is_some_and(|carrier| {
                        components.first() == Some(&carrier.component_record_index)
                    })
                })
                .collect::<Vec<_>>();
            let (true, [_], [target]) = (distinct, components.as_slice(), target.as_slice()) else {
                self.path_poses.unresolved += 1;
                continue;
            };
            // A root-contained target takes its pose through the carrier's
            // current pose, not through an expanded instance.
            let root_contained = carriers.get(&(stream, *target)).is_some_and(|carrier| {
                self.stream_components(carrier)
                    .and_then(|(_, stream_components)| {
                        stream_components
                            .root_component
                            .and_then(|record| stream_components.roots.get(&record))
                    })
                    .is_some_and(|root| root.root_entity == carrier.owner_entity)
            });
            if root_contained {
                continue;
            }
            let named = occurrences
                .iter()
                .copied()
                .filter(|record| record != target)
                .collect::<BTreeSet<_>>();
            let closing = closing_occurrence_path(path).filter(|closing| {
                instances.iter().any(|instance| {
                    instance.stream == stream
                        && instance.record == *target
                        && closing.names(instance)
                })
            });
            let matching = instances
                .iter()
                .enumerate()
                .filter(|(_, instance)| {
                    instance.stream == stream
                        && instance.record == *target
                        && closing
                            .as_ref()
                            .is_none_or(|closing| closing.names(instance))
                        && instance.ancestors.len() >= named.len()
                        && instance.ancestors[..named.len()]
                            .iter()
                            .copied()
                            .collect::<BTreeSet<_>>()
                            == named
                })
                .map(|(ordinal, _)| ordinal)
                .collect::<Vec<_>>();
            match matching.as_slice() {
                [] => self.path_poses.unresolved += 1,
                [ordinal] => resolved.entry(*ordinal).or_default().push(path),
                _ => ambiguous.push((path, matching)),
            }
        }
        let claimed = resolved.keys().copied().collect::<BTreeSet<_>>();
        let rows_of = |path: &DesignComponentPath| {
            path.transform.map_or(
                IDENTITY,
                crate::records::component_path::ComponentPathTransform::rows,
            )
        };
        let mut jobs = Vec::new();
        for (ordinal, paths) in resolved {
            let first = rows_of(paths[0]);
            let agree = paths.iter().all(|path| {
                rows_of(path)
                    .iter()
                    .flatten()
                    .zip(first.iter().flatten())
                    .all(|(left, right)| (left - right).abs() <= EPS_PATH_POSE_AGREE)
            });
            if agree {
                jobs.push((
                    instances[ordinal].ancestors.len(),
                    ordinal,
                    PathJob::Single(first),
                ));
            } else {
                self.path_poses.disagreeing += 1;
            }
        }
        let mut groups = BTreeMap::<(&str, u64), Vec<(&DesignComponentPath, Vec<usize>)>>::new();
        for (path, candidates) in ambiguous {
            let Some(&first) = candidates.first() else {
                continue;
            };
            let target = &instances[first];
            groups
                .entry((target.stream.as_str(), target.record))
                .or_default()
                .push((path, candidates));
        }
        for (index, carriers) in groups.values().enumerate() {
            // A candidate's world pose depends on every ancestor above it,
            // so the group waits for the deepest candidate's ancestors.
            let depth = carriers
                .iter()
                .flat_map(|(_, candidates)| candidates)
                .map(|ordinal| instances[*ordinal].ancestors.len())
                .max()
                .unwrap_or_default();
            jobs.push((depth, usize::MAX - index, PathJob::Group(index)));
        }
        let groups = groups.into_values().collect::<Vec<_>>();
        jobs.sort_by_key(|(depth, ordinal, _)| (*depth, *ordinal));
        for (_, ordinal, job) in jobs {
            match job {
                PathJob::Single(rows) => {
                    let key = instances[ordinal].key.as_str();
                    if let Some(local) = self.path_local_pose(key, rows)? {
                        self.set_path_pose(key, local);
                    } else {
                        self.path_poses.unresolved += 1;
                    }
                }
                PathJob::Group(index) => {
                    self.infer_path_group(&instances, &groups[index], &claimed, &rows_of)?;
                }
            }
        }
        Ok(())
    }

    /// `inverse(parent world) * pose` for the occurrence `key`, in
    /// millimetres, with the parent's corrections already applied.
    fn path_local_pose(
        &self,
        key: &str,
        rows: [[f64; 4]; 4],
    ) -> Result<Option<cadmpeg_ir::transform::Transform>, cadmpeg_core::CodecError> {
        let Some((occurrence, _)) = self.occurrences.get(key) else {
            return Ok(None);
        };
        let parent_world = match &occurrence.parent {
            OccurrenceParent::Root {} => Some(cadmpeg_ir::transform::Transform::identity()),
            OccurrenceParent::Occurrence { occurrence } => self.world_of(occurrence.as_str()),
        };
        let pose = neutral_transform(rows)?;
        Ok(parent_world
            .and_then(|world| world.try_inverse_affine().ok())
            .and_then(|inverse| inverse.compose(pose).ok()))
    }

    /// Replace the placement of `key` by `local` and count the change.
    fn set_path_pose(&mut self, key: &str, local: cadmpeg_ir::transform::Transform) {
        let Some((occurrence, carrier)) = self.occurrences.get_mut(key) else {
            return;
        };
        let (stored, new) = (occurrence.transform.affine_rows(), local.affine_rows());
        let shift = (0..3)
            .map(|row| (new[row][3] - stored[row][3]).powi(2))
            .sum::<f64>()
            .sqrt();
        let linear = (0..3)
            .flat_map(|row| (0..3).map(move |column| (row, column)))
            .map(|(row, column)| (new[row][column] - stored[row][column]).abs())
            .fold(0.0, f64::max);
        if shift > EPS_PATH_POSE_SHIFT_MM || linear > EPS_PATH_POSE_LINEAR {
            self.path_poses.changed += 1;
            self.path_poses.largest_shift_mm = self.path_poses.largest_shift_mm.max(shift);
        }
        if let Some(carrier) = carrier {
            self.path_poses.applied_carriers.insert(carrier.id.clone());
        }
        occurrence.transform = local;
    }

    /// Assign the carriers of one target to distinct instances. The
    /// assignment applies only when exactly one full assignment gives every
    /// carrier the same definition-local pose; the search stops past
    /// [`MAX_PATH_ASSIGNMENT_STEPS`] steps.
    fn infer_path_group(
        &mut self,
        instances: &[PathInstance],
        carriers: &[(&DesignComponentPath, Vec<usize>)],
        claimed: &BTreeSet<usize>,
        rows_of: &dyn Fn(&DesignComponentPath) -> [[f64; 4]; 4],
    ) -> Result<(), cadmpeg_core::CodecError> {
        // Per carrier, the candidates with their definition-local poses.
        let mut options = Vec::new();
        for (path, candidates) in carriers {
            let mut row = Vec::new();
            for ordinal in candidates {
                if claimed.contains(ordinal) {
                    continue;
                }
                let key = instances[*ordinal].key.as_str();
                if let Some(local) = self.path_local_pose(key, rows_of(path))? {
                    row.push((*ordinal, local.affine_rows()));
                }
            }
            options.push(row);
        }
        let mut order = (0..carriers.len()).collect::<Vec<_>>();
        order.sort_by_key(|carrier| (options[*carrier].len(), *carrier));
        let mut search = AssignmentSearch {
            options: &options,
            order: &order,
            chosen: Vec::new(),
            steps: 0,
            found: Vec::new(),
        };
        search.descend();
        match (
            search.steps > MAX_PATH_ASSIGNMENT_STEPS,
            search.found.as_slice(),
        ) {
            (false, [assignment]) => {
                for (&carrier, &ordinal) in order.iter().zip(assignment) {
                    let key = instances[ordinal].key.as_str();
                    if let Some(local) = self.path_local_pose(key, rows_of(carriers[carrier].0))? {
                        self.set_path_pose(key, local);
                    }
                }
                self.path_poses.inferred_carriers += carriers.len();
                self.path_poses.inferred_instances += assignment.len();
                self.path_poses.inferred_groups += 1;
            }
            _ => self.path_poses.unresolved += carriers.len(),
        }
        Ok(())
    }

    fn finish(self) -> Result<LocalComponentProjection, cadmpeg_core::CodecError> {
        let overflow =
            || cadmpeg_core::CodecError::malformed("Fusion Design occurrence ordinal exceeds u32");
        let mut occurrences = self.occurrences.into_values().collect::<Vec<_>>();
        occurrences.sort_by(|(left, left_carrier), (right, right_carrier)| {
            (
                left_carrier.map(DesignComponentOccurrence::byte_offset),
                left.id.as_str(),
            )
                .cmp(&(
                    right_carrier.map(DesignComponentOccurrence::byte_offset),
                    right.id.as_str(),
                ))
        });
        let mut next_ordinal = BTreeMap::<Option<OccurrenceId>, u32>::new();
        let root_start = u32::try_from(self.sources.root_ordinal_start).map_err(|_| overflow())?;
        for (occurrence, _) in &mut occurrences {
            let parent = match &occurrence.parent {
                OccurrenceParent::Root {} => None,
                OccurrenceParent::Occurrence { occurrence } => Some(occurrence.clone()),
            };
            let start = if parent.is_none() { root_start } else { 0 };
            let ordinal = next_ordinal.entry(parent).or_insert(start);
            occurrence.ordinal = *ordinal;
            *ordinal = ordinal.checked_add(1).ok_or_else(overflow)?;
        }
        let mut occurrences = occurrences
            .into_iter()
            .map(|(occurrence, _)| occurrence)
            .collect::<Vec<_>>();
        occurrences.sort_by(|left, right| left.id.cmp(&right.id));
        let native_poses = self
            .sources
            .occurrences
            .iter()
            .filter(|carrier| carrier.current_pose.is_some())
            .count()
            .checked_sub(self.current_poses)
            .and_then(|count| {
                count.checked_sub(
                    self.sources
                        .occurrences
                        .iter()
                        .filter(|carrier| {
                            carrier.current_pose.is_some()
                                && self.path_poses.applied_carriers.contains(&carrier.id)
                        })
                        .count(),
                )
            })
            .ok_or_else(|| {
                cadmpeg_core::CodecError::malformed(
                    "Fusion Design placed more current poses than its carriers hold",
                )
            })?;
        let fallback_owners =
            self.sources
                .body_bindings
                .iter()
                .filter(|binding| {
                    binding.body.is_some()
                        && binding.component_owner.is_none()
                        && !binding.component_owner_unresolved
                        && self.sources.components.contains_key(
                            crate::ids::native_stream(binding.id()).unwrap_or_default(),
                        )
                })
                .count();
        let unresolved_owners = self
            .sources
            .body_bindings
            .iter()
            .filter(|binding| binding.body.is_some() && binding.component_owner_unresolved)
            .count();
        let mut losses: Vec<LossNote> = self
            .unplaced
            .into_iter()
            .map(|(reason, count)| {
                match reason {
                    Unplaced::OwnerUnbound => F3dLossCode::LocalOccurrenceOwnerUnbound,
                    Unplaced::OwnerCycle => F3dLossCode::LocalOccurrenceOwnerCycle,
                    Unplaced::ExpansionLimit => F3dLossCode::LocalOccurrenceExpansionLimit,
                    _ => F3dLossCode::LocalOccurrenceUnplaced,
                }
                .note(if reason == Unplaced::ExpansionLimit {
                    format!(
                        "{count} local instance subtree(s) are not expanded: {}",
                        reason.reason()
                    )
                } else {
                    format!(
                        "{count} local component occurrence(s) are not placed: {}",
                        reason.reason()
                    )
                })
            })
            .collect();
        if self.current_poses != 0 {
            losses.push(F3dLossCode::LocalOccurrenceCurrentPose.note(format!(
                "{} local occurrence(s) in the root component take their current pose from a component-path carrier; their stored placements stay in the native frames",
                self.current_poses
            )));
        }
        if self.path_poses.changed != 0 {
            losses.push(F3dLossCode::LocalOccurrencePathPose.note(format!(
                "{} nested local occurrence(s) take the world pose of their component-path carrier in place of their stored placement; the largest translation change is {:.3} mm",
                self.path_poses.changed, self.path_poses.largest_shift_mm
            )));
        }
        if self.path_poses.inferred_carriers != 0 {
            losses.push(F3dLossCode::LocalOccurrencePathPoseInferred.note(format!(
                "{} component-path carrier(s) name several expanded local occurrences; {} occurrence(s) in {} target(s) take them by the one assignment to distinct occurrences that gives every definition-local pose the same value",
                self.path_poses.inferred_carriers,
                self.path_poses.inferred_instances,
                self.path_poses.inferred_groups
            )));
        }
        if self.path_poses.unresolved != 0 {
            losses.push(F3dLossCode::LocalOccurrencePathPoseUnresolved.note(format!(
                "{} component-path carrier(s) name no unique expanded local occurrence; they are not applied",
                self.path_poses.unresolved
            )));
        }
        if self.path_poses.disagreeing != 0 {
            losses.push(F3dLossCode::LocalOccurrencePathPoseDisagrees.note(format!(
                "{} local occurrence(s) are named by component-path carriers with different poses; they keep their stored placement",
                self.path_poses.disagreeing
            )));
        }
        if unresolved_owners != 0 {
            losses.push(F3dLossCode::ComponentBodyOwnerUnresolved.note(format!(
                "{unresolved_owners} solved body binding(s) have no unique admitted Body-container component owner."
            )));
        }
        if fallback_owners != 0 {
            losses.push(F3dLossCode::ComponentBodyOwnerFallback.note(format!(
                "{fallback_owners} solved body binding(s) use entity-cluster component ownership because no admitted Body-container component membership names them."
            )));
        }
        if native_poses != 0 {
            losses.push(F3dLossCode::LocalOccurrenceCurrentPoseNative.note(format!(
                "{native_poses} local occurrence(s) keep their stored placement; the pose of their component-path carrier stays native"
            )));
        }
        Ok(LocalComponentProjection {
            definitions: self.definitions.into_values().collect(),
            occurrences,
            losses,
        })
    }

    /// Definition identified by a component naming space.
    fn naming_space_definition(
        &mut self,
        stream: &str,
        root: &ComponentDefinition,
    ) -> ProductDefinitionId {
        let id = crate::ids::neutral_component_id(&root.context_uuid);
        if !self.definitions.contains_key(id.as_str()) {
            let bodies = self.definition_bodies(stream, root.root_entity);
            let mut definition = component_definition(id.clone(), bodies);
            definition.source_name.clone_from(&root.name);
            self.definitions.insert(id.as_str().to_owned(), definition);
        }
        id
    }

    /// Definition identified by a carrier GUID when no naming space binds the
    /// component record.
    fn guid_definition(&mut self, component_guid: &DesignRelaxedGuidText) -> ProductDefinitionId {
        let id = crate::ids::neutral_component_id(component_guid);
        if !self.definitions.contains_key(id.as_str()) {
            self.definitions.insert(
                id.as_str().to_owned(),
                component_definition(id.clone(), Vec::new()),
            );
        }
        id
    }

    fn definition_bodies(&self, stream: &str, root_entity: u64) -> Vec<BodyId> {
        let mut bodies = self
            .sources
            .body_bindings
            .iter()
            .filter(|binding| {
                crate::ids::native_stream(binding.id()) == Some(stream)
                    && match &binding.component_owner {
                        Some(owner) => owner
                            .component_for(binding.entity_suffix)
                            .and_then(|component| {
                                self.sources.components.get(stream)?.roots.get(&component)
                            })
                            .is_some_and(|definition| definition.root_entity == root_entity),
                        None => {
                            !binding.component_owner_unresolved
                                && self
                                    .sources
                                    .components
                                    .get(stream)
                                    .is_some_and(|components| {
                                        components
                                            .cluster_owner(binding.entity_suffix)
                                            .is_some_and(|owner| owner.root_entity == root_entity)
                                    })
                        }
                    }
            })
            .filter_map(|binding| binding.body.clone())
            .filter(|body| {
                self.sources
                    .bodies
                    .is_none_or(|bodies| bodies.contains(body))
            })
            .collect::<Vec<_>>();
        bodies.sort();
        bodies.dedup();
        bodies
    }

    fn every_body_has_a_definition(&self) -> bool {
        let mut owned = BTreeSet::new();
        for (stream, components) in self.sources.components {
            for root in components.roots.values() {
                owned.extend(self.definition_bodies(stream, root.root_entity));
            }
        }
        self.sources.bodies.is_some_and(|bodies| {
            bodies.iter().all(|body| {
                self.sources
                    .hidden_bodies
                    .is_some_and(|hidden| hidden.contains(body))
                    || owned.contains(body)
            })
        })
    }
}

fn component_definition(id: ProductDefinitionId, bodies: Vec<BodyId>) -> ProductDefinition {
    ProductDefinition {
        sketches: Vec::default(),
        id,
        kind: ProductDefinitionKind::Part,
        source_name: None,
        label: None,
        description: None,
        part_number: None,
        bom_properties: BTreeMap::new(),
        bodies,
        native_ref: None,
    }
}

/// Project a proven local occurrence into a `DerivedInstance` feature.
pub(crate) fn project_derived_instance_features(
    features: &mut [Feature],
    scopes: &[DesignParameterScope],
) {
    for scope in scopes {
        let Some(construction) = scope.derived_instance_construction() else {
            continue;
        };
        let Some(feature) = features
            .iter_mut()
            .find(|feature| feature.native_ref.as_deref() == Some(scope.id.as_str()))
        else {
            continue;
        };
        if !matches!(
            feature.evaluation.definition(),
            FeatureDefinition::Operation(FeatureOperation::Native { .. })
        ) {
            continue;
        }
        feature
            .evaluation
            .set_definition(FeatureDefinition::Operation(
                FeatureOperation::InsertComponent {
                    occurrence: crate::ids::neutral_component_occurrence_id(
                        &construction.occurrence_guid,
                    ),
                },
            ));
    }
}

/// The occurrence of every top-level `Component Insert`.
///
/// The timeline item run names the top-level inserts. An unlisted scope is an
/// internal operation operand and has no occurrence of its own. A container
/// reference whose role and transform select a top-level insert places it;
/// any other top-level insert projects an unresolved occurrence from its own
/// placement. The prototype of that occurrence stays unresolved because the
/// source names no target document or object this decoder can identify.
#[derive(Debug, Default)]
pub(crate) struct ComponentInserts {
    /// Occurrence of each placed top-level insert, keyed by scope id.
    occurrences: BTreeMap<String, OccurrenceId>,
    /// Inserts that no container reference places, in scope order.
    unresolved: Vec<UnresolvedComponentInsert>,
    /// Top-level inserts with no container reference whose placement has no
    /// finite millimetre transform.
    unplaced: usize,
    /// Inserts with no container reference that no authored order classifies.
    unordered: usize,
}

#[derive(Debug)]
struct UnresolvedComponentInsert {
    id: OccurrenceId,
    scope: String,
    name: String,
    transform: cadmpeg_ir::transform::Transform,
}

impl ComponentInserts {
    /// Decide every `Component Insert` occurrence from the scopes, the
    /// timelines, and the container references.
    ///
    /// Timelines that did not decode are passed as none. Native storage keeps
    /// no record of that failure, so the writer's re-projection would read
    /// the timelines as absent too. The timeline-less rule then decides, and
    /// it orders only a single-family stream.
    pub(crate) fn decide(
        scopes: &[DesignParameterScope],
        timelines: &[DesignFeatureTimeline],
        references: &[crate::records::xref::XrefReference],
    ) -> Self {
        let mut inserts = Self::default();
        if scopes
            .iter()
            .all(|scope| scope.component_insert_construction().is_none())
        {
            return inserts;
        }
        let authored =
            crate::design::feature_project::authored_scope_ordinals_per_stream(scopes, timelines)
                .ok();
        for scope in scopes {
            let Some(construction) = scope.component_insert_construction() else {
                continue;
            };
            let stream = crate::ids::native_stream(&scope.id).unwrap_or(crate::ids::DEFAULT_STREAM);
            let reference =
                crate::xref::component_insert_reference(references, stream, construction);
            let Some(authored) = &authored else {
                if reference.is_none() {
                    inserts.unordered += 1;
                }
                continue;
            };
            if !authored.contains_key(&(stream, scope.record_index)) {
                continue;
            }
            if let Some(reference) = reference {
                inserts.occurrences.insert(
                    scope.id.clone(),
                    crate::ids::neutral_xref_occurrence_id(
                        reference.ordinal,
                        reference.occurrence_ordinal,
                    ),
                );
                continue;
            }
            let Some(transform) = neutral_affine(*construction.transform()) else {
                inserts.unplaced += 1;
                continue;
            };
            let id = crate::ids::neutral_component_insert_occurrence_id(scope);
            inserts.occurrences.insert(scope.id.clone(), id.clone());
            inserts.unresolved.push(UnresolvedComponentInsert {
                id,
                scope: scope.id.clone(),
                name: construction.neutron_role.clone(),
                transform,
            });
        }
        inserts
    }

    /// Resolve native external placements to unique expanded instance paths.
    pub(crate) fn bind_external_paths(
        &mut self,
        references: &[crate::records::xref::XrefReference],
        projected: &[Occurrence],
    ) {
        self.occurrences.retain(|scope, id| {
            let stream = crate::ids::native_stream(scope).unwrap_or(crate::ids::DEFAULT_STREAM);
            let Some(reference) = references.iter().find(|reference| {
                reference
                    .placement_source
                    .as_ref()
                    .is_some_and(|source| source.stream == stream)
                    && *id
                        == crate::ids::neutral_xref_occurrence_id(
                            reference.ordinal,
                            reference.occurrence_ordinal,
                        )
            }) else {
                return true;
            };
            let Some(occurrence) = crate::xref::unique_occurrence(reference, projected) else {
                return false;
            };
            *id = occurrence.id.clone();
            true
        });
    }

    /// The occurrence that places the top-level insert `scope`.
    pub(crate) fn occurrence(&self, scope: &str) -> Option<&OccurrenceId> {
        self.occurrences.get(scope)
    }

    /// Root occurrences of the inserts that no container reference places,
    /// numbered from `ordinal_start`.
    pub(crate) fn unresolved_occurrences(
        &self,
        ordinal_start: usize,
    ) -> Result<Vec<Occurrence>, cadmpeg_core::CodecError> {
        let mut occurrences = Vec::new();
        for (offset, insert) in self.unresolved.iter().enumerate() {
            let ordinal = ordinal_start
                .checked_add(offset)
                .and_then(|ordinal| u32::try_from(ordinal).ok())
                .ok_or_else(|| {
                    cadmpeg_core::CodecError::malformed(
                        "Fusion Design occurrence ordinal exceeds u32",
                    )
                })?;
            occurrences.push(Occurrence {
                id: insert.id.clone(),
                prototype: PrototypeReference::Unresolved {},
                parent: OccurrenceParent::Root {},
                ordinal,
                transform: insert.transform,
                linked_prototype: None,
                scale: [cadmpeg_ir::scalar::FiniteReal::ONE; 3],
                name: Some(insert.name.clone()),
                visible: None,
                link: None,
                native_ref: Some(insert.scope.clone()),
            });
        }
        Ok(occurrences)
    }

    /// Losses for the inserts that project no occurrence.
    pub(crate) fn losses(&self) -> Vec<LossNote> {
        let mut losses = Vec::new();
        if self.unplaced != 0 {
            losses.push(F3dLossCode::ComponentInsertUnplaced.note(format!(
                "{} unresolved Component Insert occurrence(s) were not projected: the placement has no finite millimetre transform.",
                self.unplaced
            )));
        }
        if self.unordered != 0 {
            losses.push(F3dLossCode::ComponentInsertOrderUnknown.note(format!(
                "{} unresolved Component Insert occurrence(s) were not projected: no authored timeline order tells top-level inserts from internal operands.",
                self.unordered
            )));
        }
        losses
    }
}

/// Bind each `Component Insert` feature that no container reference resolved
/// to the unresolved occurrence that product projection emitted for its scope.
///
/// Run after `crate::xref::bind_component_insert_features`: a feature that
/// still has its native definition has no container reference.
pub(crate) fn bind_unresolved_component_insert_features(
    features: &mut [Feature],
    scopes: &[DesignParameterScope],
    occurrences: &[Occurrence],
) {
    let projected = occurrences
        .iter()
        .map(|occurrence| occurrence.id.as_str())
        .collect::<BTreeSet<_>>();
    for scope in scopes {
        if scope.component_insert_construction().is_none() {
            continue;
        }
        let occurrence = crate::ids::neutral_component_insert_occurrence_id(scope);
        if !projected.contains(occurrence.as_str()) {
            continue;
        }
        let Some(feature) = features
            .iter_mut()
            .find(|feature| feature.native_ref.as_deref() == Some(scope.id.as_str()))
        else {
            continue;
        };
        if !matches!(
            feature.evaluation.definition(),
            FeatureDefinition::Operation(FeatureOperation::Native { .. })
        ) {
            continue;
        }
        feature
            .evaluation
            .set_definition(FeatureDefinition::Operation(
                FeatureOperation::InsertComponent { occurrence },
            ));
    }
}

/// A millimetre placement projected from source centimetres, when it is a
/// finite affine transform.
pub(crate) fn neutral_affine(
    transform: impl Into<[[f64; 4]; 4]>,
) -> Option<cadmpeg_ir::transform::Transform> {
    let source = transform.into();
    if source[3] != [0.0, 0.0, 0.0, 1.0] {
        return None;
    }
    let mut rows = [source[0], source[1], source[2]];
    for row in &mut rows {
        row[3] *= 10.0;
    }
    cadmpeg_ir::transform::Transform::affine(rows)
}

/// A millimetre placement projected from source centimetres.
pub(crate) fn neutral_transform(
    transform: impl Into<[[f64; 4]; 4]>,
) -> Result<cadmpeg_ir::transform::Transform, cadmpeg_core::CodecError> {
    neutral_affine(transform).ok_or_else(|| {
        cadmpeg_core::CodecError::NotImplemented(
            "F3D placement must project to a finite affine transform".into(),
        )
    })
}

#[cfg(test)]
mod tests {
    mod owners;
    mod path_poses;

    use crate::design::test_support::identity_matrix;
    use crate::records::feature::{
        assembly_features::{
            DesignComponentOccurrence, DesignCopyPasteComponentOperation,
            DesignDerivedInstanceConstruction,
        },
        scope::DesignParameterScope,
    };

    use std::collections::{BTreeMap, BTreeSet};

    use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureId, FeatureOperation};
    use cadmpeg_ir::ids::BodyId;
    use cadmpeg_ir::products::{
        Occurrence, OccurrenceParent, ProductDefinition, PrototypeReference,
    };

    use crate::design::decode::components::{ComponentDefinition, StreamComponents};
    use crate::records::bodies::DesignBodyBinding;
    use crate::records::feature::assembly_features::{
        DesignComponentOccurrenceDraft, DesignComponentOccurrencePlacement,
    };

    const STREAM: &str = "f3d:Design/BulkStream.dat";
    const COMPONENT_CONTEXT: &str = "3df3a877-70e6-45e7-bf55-7d4d5ce42a20";
    const ROOT_CONTEXT: &str = "103b35af-2d83-4986-88b5-95c7fdce0b17";
    const COMPONENT_ROOT: u64 = 160;
    const DOCUMENT_ROOT: u64 = 277;
    const COMPONENT_BODY: &str = "f3d:brep/component.smbh/brep:entity#1";
    const ROOT_BODY: &str = "f3d:brep/root.smbh/brep:entity#1";

    fn project_without_naming_spaces(
        scopes: &[DesignParameterScope],
        occurrences: &[DesignComponentOccurrence],
    ) -> (Vec<ProductDefinition>, Vec<Occurrence>) {
        let components = BTreeMap::new();
        let bodies = BTreeSet::new();
        let projection = super::project_local_components(&super::LocalComponentSources {
            hidden_bodies: None,
            scopes,
            occurrences,
            paths: &[],
            components: &components,
            body_bindings: &[],
            bodies: Some(&bodies),
            root_ordinal_start: 0,
            limits: cadmpeg_core::decode::ResourceLimits::default(),
        })
        .unwrap();
        (projection.definitions, projection.occurrences)
    }

    fn guid(text: &str) -> crate::records::mesh::DesignRelaxedGuidText {
        text.to_owned().try_into().expect("GUID")
    }

    /// One component definition record 159 rooted at entity 160, and the root
    /// component record 276 rooted at entity 277 and named by the root instance.
    fn two_component_stream(root_component: Option<u64>) -> BTreeMap<String, StreamComponents> {
        BTreeMap::from([(
            STREAM.to_owned(),
            StreamComponents {
                roots: BTreeMap::from([
                    (
                        159,
                        ComponentDefinition {
                            root_entity: COMPONENT_ROOT,
                            context_uuid: guid(COMPONENT_CONTEXT),
                            name: Some("Bracket".to_owned()),
                        },
                    ),
                    (
                        276,
                        ComponentDefinition {
                            root_entity: DOCUMENT_ROOT,
                            context_uuid: guid(ROOT_CONTEXT),
                            name: None,
                        },
                    ),
                ]),
                root_component,
                ..Default::default()
            },
        )])
    }

    fn ownership_layout(body: u64, component: u64) -> Vec<u8> {
        let mut bytes = 1_u32.to_le_bytes().to_vec();
        for (target, guid) in [
            (
                body,
                crate::design::presentation::BODY_PRESENTATION_TYPE_GUID,
            ),
            (component, crate::records::body_ownership::COMPONENT_GUID),
        ] {
            bytes.push(1);
            bytes.extend_from_slice(&target.to_le_bytes());
            bytes.extend_from_slice(&36_u32.to_le_bytes());
            bytes.extend_from_slice(guid.as_bytes());
            bytes.extend_from_slice(&[0, 0]);
        }
        bytes
    }

    fn binding(offset: u64, entity_suffix: u64, body: &str) -> DesignBodyBinding {
        serde_json::from_value(serde_json::json!({
            "id": format!("{STREAM}:design-body-binding#{offset}"),
            "stream": "Design/BulkStream.dat",
            "pair_count": 1,
            "pair_ordinal": 0,
            "asm_body_key": 11,
            "asm_body_key_offset": offset,
            "entity_suffix": entity_suffix,
            "entity_suffix_offset": offset + 8,
            "blob_name": "BREP.",
            "blob_name_offset": offset + 32,
            "body": body,
            "component_owner": crate::records::body_ownership::membership(&ownership_layout(entity_suffix, if entity_suffix < DOCUMENT_ROOT { 159 } else { 276 })).unwrap().proof(900, 0, 0).unwrap(),
        }))
        .unwrap()
    }

    fn carrier(
        byte_offset: u64,
        occurrence_guid: &str,
        placement: DesignComponentOccurrencePlacement,
        owner_entity: u64,
    ) -> DesignComponentOccurrence {
        DesignComponentOccurrence::try_new(DesignComponentOccurrenceDraft {
            id: format!("{STREAM}:design-component-occurrence#{byte_offset}"),
            class_tag: crate::records::references::DesignClassTag::try_from("339".to_owned())
                .unwrap(),
            record_index: u32::try_from(byte_offset).unwrap(),
            byte_offset,
            component_record_index: 159,
            component_guid: guid("2ad641b3-9ea3-4e5f-a4da-926c3faeccae"),
            occurrence_guid: guid(occurrence_guid).into(),
            placement,
            owner_entity,
        })
        .unwrap()
    }

    fn placed(ordinal: u32, transform: [[f64; 4]; 4]) -> DesignComponentOccurrencePlacement {
        DesignComponentOccurrencePlacement::Explicit {
            ordinal: std::num::NonZeroU32::new(ordinal).unwrap(),
            transform: transform.try_into().unwrap(),
        }
    }

    fn project_carriers(
        carriers: &[DesignComponentOccurrence],
        components: &BTreeMap<String, StreamComponents>,
        bodies: &[&str],
    ) -> super::LocalComponentProjection {
        let bindings = [
            binding(1000, 194, COMPONENT_BODY),
            binding(2000, 300, ROOT_BODY),
        ];
        let bodies = bodies
            .iter()
            .map(|body| BodyId::mint(*body).unwrap())
            .collect::<BTreeSet<_>>();
        super::project_local_components(&super::LocalComponentSources {
            hidden_bodies: None,
            scopes: &[],
            occurrences: carriers,
            paths: &[],
            components,
            body_bindings: &bindings,
            bodies: Some(&bodies),
            root_ordinal_start: 0,
            limits: cadmpeg_core::decode::ResourceLimits::default(),
        })
        .unwrap()
    }

    fn external_owner_table(owner: u64) -> crate::xref::XrefTable {
        let mut matrix = super::IDENTITY;
        matrix[0][3] = 2.0;
        crate::xref::XrefTable {
            references: vec![crate::records::xref::XrefReference {
                current_pose: None,
                operation_pose: None,
                contained_poses: Vec::new(),
                owned_poses: Vec::new(),
                id: "f3d:xref:reference#0".to_owned(),
                ordinal: 0,
                occurrence_ordinal: 0,
                placement_source: Some(crate::records::xref::XrefPlacementSource {
                    frame_bytes: Vec::new().into(),
                    occurrence_guid: None,
                    stream: STREAM.to_owned(),
                    carrier_record_index: 500,
                    owner: Some(crate::records::xref::XrefLocalOwner {
                        target: owner,
                        type_guid: None,
                    }),
                }),
                from: "root.f3d".to_owned().try_into().unwrap(),
                relative_path: "part.f3d".to_owned().try_into().unwrap(),
                neutron_role: "external-role".to_owned().try_into().unwrap(),
                neutron_data: String::new(),
                transform: Some(matrix.try_into().unwrap()),
            }],
            ..Default::default()
        }
    }

    #[test]
    fn external_owner_repeats_local_children_under_each_containing_instance() {
        let components = two_component_stream(Some(276));
        let mut first = super::IDENTITY;
        first[0] = [0.0, -1.0, 0.0, 3.0];
        first[1] = [1.0, 0.0, 0.0, 4.0];
        let mut second = super::IDENTITY;
        second[0][3] = 8.0;
        let carriers = [
            carrier(
                100,
                "00000000-0000-4000-8000-000000000001",
                placed(1, first),
                DOCUMENT_ROOT,
            ),
            carrier(
                200,
                "00000000-0000-4000-8000-000000000002",
                placed(2, second),
                DOCUMENT_ROOT,
            ),
        ];
        let bodies = BTreeSet::new();
        for count in [1, 2] {
            let mut sources = super::LocalComponentSources {
                hidden_bodies: None,
                scopes: &[],
                occurrences: &carriers[..count],
                paths: &[],
                components: &components,
                body_bindings: &[],
                bodies: Some(&bodies),
                root_ordinal_start: 1,
                limits: cadmpeg_core::decode::ResourceLimits::default(),
            };
            let local = super::project_local_components(&sources).unwrap();
            let mut table = external_owner_table(COMPONENT_ROOT);
            let external_key = "00000000-0000-4000-8000-000000000009";
            table.references[0]
                .placement_source
                .as_mut()
                .unwrap()
                .occurrence_guid = Some(external_key.to_owned().try_into().unwrap());
            let projection =
                crate::xref::project_occurrences(&table, &sources, &local.occurrences).unwrap();
            assert!(local.losses.is_empty());
            assert!(projection.losses.is_empty());
            assert_eq!(projection.occurrences.len(), count);
            let mut combined = local.occurrences.clone();
            combined.extend(projection.occurrences.iter().cloned());
            let bindings = super::paths::LocalOccurrencePaths::new(
                &carriers[..count],
                &table.references,
                &combined,
            );
            assert_eq!(
                bindings.ambiguous(STREAM, &[external_key.to_owned()]),
                count == 2
            );
            let mut paths = BTreeSet::new();
            for (index, carrier) in carriers[..count].iter().enumerate() {
                let parent = crate::ids::neutral_component_occurrence_id(&carrier.occurrence_guid);
                let child = projection
                    .occurrences
                    .iter()
                    .find(|child| {
                        child.parent
                            == OccurrenceParent::Occurrence {
                                occurrence: parent.clone(),
                            }
                    })
                    .unwrap();
                assert!(paths.insert(child.id.clone()));
                assert_eq!(
                    bindings.resolve(
                        STREAM,
                        &[
                            carrier.occurrence_guid.as_str().to_owned(),
                            external_key.to_owned(),
                        ]
                    ),
                    Some((child.id.clone(), 2))
                );
                assert_eq!(
                    child.id,
                    crate::ids::nested_xref_occurrence_id(&parent, 0, 0)
                );
                assert_eq!(child.transform.rows()[0][3], 20.0);
                let parent = local
                    .occurrences
                    .iter()
                    .find(|occurrence| occurrence.id == parent)
                    .unwrap();
                let world = parent.transform.compose(child.transform).unwrap().rows();
                assert_eq!(
                    [world[0][3], world[1][3]],
                    if index == 0 {
                        [30.0, 60.0]
                    } else {
                        [100.0, 0.0]
                    }
                );
            }
            if count == 2 {
                sources.limits.max_entities =
                    cadmpeg_core::decode::u64_from_index(local.occurrences.len()) + 1;
                assert!(matches!(
                    crate::xref::project_occurrences(&external_owner_table(COMPONENT_ROOT), &sources, &local.occurrences),
                    Err(cadmpeg_core::CodecError::ResourceLimit(limit)) if limit.operation == "expand F3D external occurrences"
                ));
            }
        }
    }

    #[test]
    fn external_root_owner_stays_root_and_unresolved_owners_report_a_loss() {
        let components = two_component_stream(Some(276));
        let bodies = BTreeSet::new();
        let sources = super::LocalComponentSources {
            hidden_bodies: None,
            scopes: &[],
            occurrences: &[],
            paths: &[],
            components: &components,
            body_bindings: &[],
            bodies: Some(&bodies),
            root_ordinal_start: 1,
            limits: cadmpeg_core::decode::ResourceLimits::default(),
        };
        for world_carrier in [false, true] {
            for (owner, stream, unresolved) in [
                (DOCUMENT_ROOT, STREAM, false),
                (999, STREAM, true),
                (COMPONENT_ROOT, STREAM, true),
                (DOCUMENT_ROOT, "f3d:OtherDesign/BulkStream.dat", true),
            ] {
                let mut table = external_owner_table(owner);
                table.references[0]
                    .placement_source
                    .as_mut()
                    .unwrap()
                    .stream = stream.to_owned();
                if world_carrier {
                    let mut rows = crate::xref::IDENTITY_ROWS;
                    rows[0][3] = 4.0;
                    table.references[0].current_pose =
                        Some(crate::records::xref::XrefCurrentPose {
                            carrier_record_index: 99,
                            frame_bytes: cadmpeg_ir::native::bytes::NativeBytes::default(),
                            transform: Some(rows.try_into().unwrap()),
                        });
                }
                let projection = crate::xref::project_occurrences(&table, &sources, &[]).unwrap();
                assert_eq!(projection.occurrences.len(), 1);
                assert_eq!(projection.occurrences[0].parent, OccurrenceParent::Root {});
                assert_eq!(
                    projection.occurrences[0].transform.rows()[0][3],
                    if world_carrier { 40.0 } else { 20.0 }
                );
                assert_eq!(
                    projection.losses.len(),
                    usize::from(unresolved) + usize::from(world_carrier)
                );
                if unresolved {
                    assert_eq!(
                        projection.losses[0].code,
                        crate::loss::F3dLossCode::XrefOwnerUnresolved.kind()
                    );
                }
            }
        }
    }

    #[test]
    fn explicit_membership_places_a_late_body_in_its_child_component() {
        let components = two_component_stream(Some(276));
        let body = BodyId::mint(COMPONENT_BODY).unwrap();
        let bodies = BTreeSet::from([body.clone()]);
        let mut binding = binding(1000, 900, COMPONENT_BODY);
        binding.component_owner = Some(
            crate::records::body_ownership::membership(&ownership_layout(900, 159))
                .unwrap()
                .proof(900, 0, 0)
                .unwrap(),
        );
        let bindings = [binding];
        let carriers = [carrier(
            100,
            "00000000-0000-4000-8000-000000000001",
            placed(
                1,
                [
                    [0.0, -1.0, 0.0, 2.0],
                    [1.0, 0.0, 0.0, 3.0],
                    [0.0, 0.0, 1.0, 4.0],
                    [0.0, 0.0, 0.0, 1.0],
                ],
            ),
            DOCUMENT_ROOT,
        )];
        let projection = super::project_local_components(&super::LocalComponentSources {
            hidden_bodies: None,
            scopes: &[],
            occurrences: &carriers,
            paths: &[],
            components: &components,
            body_bindings: &bindings,
            bodies: Some(&bodies),
            root_ordinal_start: 0,
            limits: cadmpeg_core::decode::ResourceLimits::default(),
        })
        .unwrap();
        assert!(projection.losses.is_empty());
        let occurrence = projection
            .occurrences
            .iter()
            .find(|occurrence| occurrence.native_ref.is_some())
            .unwrap();
        assert_eq!(
            occurrence.transform.rows(),
            [
                [0.0, -1.0, 0.0, 20.0],
                [1.0, 0.0, 0.0, 30.0],
                [0.0, 0.0, 1.0, 40.0],
                [0.0, 0.0, 0.0, 1.0]
            ]
        );

        let child = crate::ids::neutral_component_id(&guid(COMPONENT_CONTEXT));
        let root = crate::ids::neutral_component_id(&guid(ROOT_CONTEXT));
        assert_eq!(
            projection
                .definitions
                .iter()
                .find(|d| d.id == child)
                .unwrap()
                .bodies,
            [body]
        );
        assert!(projection
            .definitions
            .iter()
            .find(|d| d.id == root)
            .unwrap()
            .bodies
            .is_empty());
    }

    #[test]
    fn missing_component_membership_uses_entity_cluster_and_reports_it() {
        let components = two_component_stream(Some(276));
        let body = BodyId::mint(COMPONENT_BODY).unwrap();
        let bodies = BTreeSet::from([body]);
        let mut binding = binding(1000, 194, COMPONENT_BODY);
        binding.component_owner = None;
        let bindings = [binding];
        let carriers = [carrier(
            100,
            "00000000-0000-4000-8000-000000000001",
            DesignComponentOccurrencePlacement::Base,
            DOCUMENT_ROOT,
        )];
        let projection = super::project_local_components(&super::LocalComponentSources {
            hidden_bodies: None,
            scopes: &[],
            occurrences: &carriers,
            paths: &[],
            components: &components,
            body_bindings: &bindings,
            bodies: Some(&bodies),
            root_ordinal_start: 0,
            limits: cadmpeg_core::decode::ResourceLimits::default(),
        })
        .unwrap();
        assert_eq!(projection.occurrences.len(), 2);
        assert!(!projection
            .losses
            .iter()
            .any(|loss| loss.code == crate::loss::F3dLossCode::LocalOccurrenceUnplaced.kind()));
        assert_eq!(
            projection
                .definitions
                .iter()
                .flat_map(|definition| &definition.bodies)
                .count(),
            1
        );
        assert!(projection
            .losses
            .iter()
            .any(|loss| loss.code == crate::loss::F3dLossCode::ComponentBodyOwnerFallback.kind()));
    }

    #[test]
    fn a_hidden_body_without_membership_uses_its_cluster_and_keeps_placement() {
        let components = two_component_stream(Some(276));
        let body = BodyId::mint(COMPONENT_BODY).unwrap();
        let bodies = BTreeSet::from([body]);
        let mut binding = binding(1000, 194, COMPONENT_BODY);
        binding.component_owner = None;
        let bindings = [binding];
        let carriers = [carrier(
            100,
            "00000000-0000-4000-8000-000000000001",
            DesignComponentOccurrencePlacement::Base,
            DOCUMENT_ROOT,
        )];
        let projection = super::project_local_components(&super::LocalComponentSources {
            hidden_bodies: Some(&bodies),
            scopes: &[],
            occurrences: &carriers,
            paths: &[],
            components: &components,
            body_bindings: &bindings,
            bodies: Some(&bodies),
            root_ordinal_start: 0,
            limits: cadmpeg_core::decode::ResourceLimits::default(),
        })
        .unwrap();
        assert_eq!(projection.occurrences.len(), 2);
        assert_eq!(
            projection
                .definitions
                .iter()
                .flat_map(|definition| &definition.bodies)
                .count(),
            1
        );
        assert!(projection
            .losses
            .iter()
            .any(|loss| loss.code == crate::loss::F3dLossCode::ComponentBodyOwnerFallback.kind()));
        assert!(!projection
            .losses
            .iter()
            .any(|loss| loss.code == crate::loss::F3dLossCode::LocalOccurrenceUnplaced.kind()));
    }

    #[test]
    fn explicit_membership_and_cluster_fallback_share_one_stream() {
        let components = two_component_stream(Some(276));
        let mut explicit = binding(1000, 901, COMPONENT_BODY);
        explicit.component_owner = Some(
            crate::records::body_ownership::membership(&ownership_layout(901, 159))
                .unwrap()
                .proof(900, 0, 0)
                .unwrap(),
        );
        let mut fallback = binding(1100, 900, ROOT_BODY);
        fallback.component_owner = None;
        let bindings = [explicit, fallback];
        let sources = super::LocalComponentSources {
            hidden_bodies: None,
            scopes: &[],
            occurrences: &[],
            paths: &[],
            components: &components,
            body_bindings: &bindings,
            bodies: None,
            root_ordinal_start: 0,
            limits: cadmpeg_core::decode::ResourceLimits::default(),
        };
        let projector = super::Projector::new(&sources);
        assert_eq!(
            projector.definition_bodies(STREAM, COMPONENT_ROOT),
            [BodyId::mint(COMPONENT_BODY).unwrap()]
        );
        assert_eq!(
            projector.definition_bodies(STREAM, DOCUMENT_ROOT),
            [BodyId::mint(ROOT_BODY).unwrap()]
        );
    }

    #[test]
    fn every_body_has_a_definition_counts_an_ordinary_body_without_a_native_key() {
        let components = two_component_stream(Some(276));
        let id = BodyId::mint(COMPONENT_BODY).unwrap();
        let bodies = BTreeSet::from([id.clone()]);
        let bindings = [binding(1000, 194, COMPONENT_BODY)];
        let sources = super::LocalComponentSources {
            hidden_bodies: None,
            scopes: &[],
            occurrences: &[],
            paths: &[],
            components: &components,
            body_bindings: &bindings,
            bodies: Some(&bodies),
            root_ordinal_start: 0,
            limits: cadmpeg_core::decode::ResourceLimits::default(),
        };
        let projector = super::Projector::new(&sources);
        assert_eq!(
            projector.definition_bodies(STREAM, COMPONENT_ROOT),
            vec![id]
        );
        assert!(projector.every_body_has_a_definition());
        let sources = super::LocalComponentSources {
            hidden_bodies: None,
            body_bindings: &[],
            ..sources
        };
        assert!(!super::Projector::new(&sources).every_body_has_a_definition());
    }

    #[test]
    fn root_contained_carriers_are_placed_in_the_root_occurrence_at_their_transform() {
        const FIRST: &str = "9d257e32-3a1f-45da-bbec-cfc658b575dc";
        const SECOND: &str = "d8ea5386-bc3f-4eb1-93af-09024e6db29a";
        // A quarter turn about z followed by a centimetre translation.
        let transform = [
            [0.0, -1.0, 0.0, 0.42],
            [1.0, 0.0, 0.0, 0.98],
            [0.0, 0.0, 1.0, 2.3],
            [0.0, 0.0, 0.0, 1.0],
        ];
        let carriers = [
            carrier(
                100,
                FIRST,
                DesignComponentOccurrencePlacement::Base,
                DOCUMENT_ROOT,
            ),
            carrier(500, SECOND, placed(2, transform), DOCUMENT_ROOT),
        ];
        let projection = project_carriers(
            &carriers,
            &two_component_stream(Some(276)),
            &[COMPONENT_BODY, ROOT_BODY],
        );
        assert!(projection.losses.is_empty(), "{:?}", projection.losses);

        let component = crate::ids::neutral_component_id(&guid(COMPONENT_CONTEXT));
        let root = crate::ids::neutral_component_id(&guid(ROOT_CONTEXT));
        let definition = |id: &cadmpeg_ir::ids::ProductDefinitionId| {
            projection
                .definitions
                .iter()
                .find(|definition| &definition.id == id)
                .expect("definition")
        };
        assert_eq!(projection.definitions.len(), 2);
        assert_eq!(
            definition(&component).bodies,
            [BodyId::mint(COMPONENT_BODY).unwrap()]
        );
        assert_eq!(definition(&root).bodies, [BodyId::mint(ROOT_BODY).unwrap()]);
        // The component's stored name is its definition's source name.
        assert_eq!(
            definition(&component).source_name.as_deref(),
            Some("Bracket")
        );
        assert_eq!(definition(&root).source_name, None);

        let occurrence = |id: cadmpeg_ir::ids::OccurrenceId| {
            projection
                .occurrences
                .iter()
                .find(|occurrence| occurrence.id == id)
                .expect("occurrence")
                .clone()
        };
        let root_occurrence =
            occurrence(crate::ids::neutral_root_occurrence_id(&guid(ROOT_CONTEXT)));
        assert_eq!(projection.occurrences.len(), 3);
        assert_eq!(root_occurrence.parent, OccurrenceParent::Root {});
        assert_eq!(
            root_occurrence.prototype,
            PrototypeReference::Local { definition: root }
        );
        assert_eq!(
            root_occurrence.transform,
            cadmpeg_ir::transform::Transform::identity()
        );
        let in_root = OccurrenceParent::Occurrence {
            occurrence: root_occurrence.id.clone(),
        };
        let first = occurrence(crate::ids::neutral_component_occurrence_id(&guid(FIRST)));
        let second = occurrence(crate::ids::neutral_component_occurrence_id(&guid(SECOND)));
        for (child, ordinal) in [(&first, 0), (&second, 1)] {
            assert_eq!(child.parent, in_root);
            assert_eq!(child.ordinal, ordinal);
            assert_eq!(
                child.prototype,
                PrototypeReference::Local {
                    definition: component.clone()
                }
            );
        }
        assert_eq!(
            first.transform,
            cadmpeg_ir::transform::Transform::identity()
        );
        let rows = second.transform.rows();
        let expected = [
            [0.0, -1.0, 0.0, 4.2],
            [1.0, 0.0, 0.0, 9.8],
            [0.0, 0.0, 1.0, 23.0],
        ];
        for (row, expected) in rows.iter().zip(expected) {
            for (value, expected) in row.iter().zip(expected) {
                assert!((value - expected).abs() < 1e-12, "{rows:?}");
            }
        }
    }

    #[test]
    fn empty_component_cluster_keeps_its_occurrence_without_taking_root_bodies() {
        const EMPTY_CONTEXT: &str = "00000000-0000-4000-8000-000000000010";
        let mut components = two_component_stream(Some(276));
        let stream = components.get_mut(STREAM).unwrap();
        stream.roots.insert(
            219,
            ComponentDefinition {
                root_entity: 220,
                context_uuid: guid(EMPTY_CONTEXT),
                name: Some("Empty component".to_owned()),
            },
        );
        let mut empty = carrier(
            200,
            "00000000-0000-4000-8000-000000000002",
            placed(1, super::IDENTITY),
            DOCUMENT_ROOT,
        );
        empty.component_record_index = 219;
        let carriers = [
            carrier(
                100,
                "00000000-0000-4000-8000-000000000001",
                DesignComponentOccurrencePlacement::Base,
                DOCUMENT_ROOT,
            ),
            empty,
            carrier(
                300,
                "00000000-0000-4000-8000-000000000003",
                placed(2, super::IDENTITY),
                DOCUMENT_ROOT,
            ),
        ];
        let projection = project_carriers(&carriers, &components, &[COMPONENT_BODY, ROOT_BODY]);
        assert!(projection.losses.is_empty());
        assert_eq!(projection.definitions.len(), 3);
        assert_eq!(projection.occurrences.len(), 4);
        for (context, expected) in [
            (
                COMPONENT_CONTEXT,
                vec![BodyId::mint(COMPONENT_BODY).unwrap()],
            ),
            (EMPTY_CONTEXT, Vec::new()),
            (ROOT_CONTEXT, vec![BodyId::mint(ROOT_BODY).unwrap()]),
        ] {
            let id = crate::ids::neutral_component_id(&guid(context));
            let definition = projection
                .definitions
                .iter()
                .find(|definition| definition.id == id)
                .unwrap();
            assert_eq!(definition.bodies, expected);
        }
        let root = crate::ids::neutral_root_occurrence_id(&guid(ROOT_CONTEXT));
        for carrier in &carriers {
            let id = crate::ids::neutral_component_occurrence_id(&carrier.occurrence_guid);
            let occurrence = projection
                .occurrences
                .iter()
                .find(|occurrence| occurrence.id == id)
                .unwrap();
            assert_eq!(
                occurrence.parent,
                OccurrenceParent::Occurrence {
                    occurrence: root.clone()
                }
            );
            assert!(projection.occurrences.iter().all(|other| {
                other.parent
                    != OccurrenceParent::Occurrence {
                        occurrence: id.clone(),
                    }
            }));
        }
    }

    #[test]
    fn carriers_the_source_does_not_place_stay_unplaced_with_a_loss() {
        const OCCURRENCE: &str = "443e5aa6-d561-4b5b-9634-661e7da12db3";
        let identity = DesignComponentOccurrencePlacement::Base;
        for (carriers, components, bodies, reason) in [
            (
                [carrier(100, OCCURRENCE, identity, COMPONENT_ROOT)],
                two_component_stream(Some(276)),
                vec![COMPONENT_BODY, ROOT_BODY],
                "owner graph has a cycle",
            ),
            (
                [carrier(100, OCCURRENCE, identity, DOCUMENT_ROOT)],
                two_component_stream(None),
                vec![COMPONENT_BODY, ROOT_BODY],
                "no unique root instance",
            ),
            (
                [carrier(100, OCCURRENCE, identity, DOCUMENT_ROOT)],
                two_component_stream(Some(276)),
                vec![
                    COMPONENT_BODY,
                    ROOT_BODY,
                    "f3d:brep/other.smbh/brep:entity#1",
                ],
                "a model body has no component definition",
            ),
        ] {
            let projection = project_carriers(&carriers, &components, &bodies);
            assert!(projection.occurrences.is_empty(), "{reason}");
            assert!(projection.definitions.is_empty(), "{reason}");
            assert_eq!(projection.losses.len(), 1, "{reason}");
            assert_eq!(
                projection.losses[0].code,
                if reason == "owner graph has a cycle" {
                    crate::loss::F3dLossCode::LocalOccurrenceOwnerCycle.kind()
                } else {
                    crate::loss::F3dLossCode::LocalOccurrenceUnplaced.kind()
                }
            );
            assert!(projection.losses[0].message.contains(reason), "{reason}");
        }
    }

    #[test]
    fn carrier_naming_a_dropped_component_stays_unplaced() {
        const OCCURRENCE: &str = "443e5aa6-d561-4b5b-9634-661e7da12db3";
        let mut dropped = carrier(
            100,
            OCCURRENCE,
            DesignComponentOccurrencePlacement::Base,
            DOCUMENT_ROOT,
        );
        dropped.component_record_index = 999;
        let projection = project_carriers(
            &[dropped],
            &two_component_stream(Some(276)),
            &[COMPONENT_BODY, ROOT_BODY],
        );
        assert!(projection.occurrences.is_empty());
        assert!(projection.definitions.is_empty());
        assert_eq!(projection.losses.len(), 1);
        assert_eq!(
            projection.losses[0].code,
            crate::loss::F3dLossCode::LocalOccurrenceUnplaced.kind()
        );
        assert!(projection.losses[0]
            .message
            .contains("no naming-space binding"));
    }

    fn nested_fixture() -> (
        Vec<DesignComponentOccurrence>,
        BTreeMap<String, StreamComponents>,
    ) {
        let mut components = two_component_stream(Some(276));
        let stream = components.get_mut(STREAM).unwrap();
        stream.roots.insert(
            100,
            ComponentDefinition {
                root_entity: 101,
                context_uuid: guid("00000000-0000-4000-8000-000000000101"),
                name: Some("Module".to_owned()),
            },
        );
        let parent_transform = [
            [0.0, -1.0, 0.0, 3.0],
            [1.0, 0.0, 0.0, 4.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ];
        let mut parent = carrier(
            100,
            "00000000-0000-4000-8000-000000000001",
            placed(1, parent_transform),
            DOCUMENT_ROOT,
        );
        parent.component_record_index = 100;
        let mut translated = super::IDENTITY;
        translated[0][3] = 1.0;
        (
            vec![
                parent,
                carrier(
                    200,
                    "00000000-0000-4000-8000-000000000002",
                    DesignComponentOccurrencePlacement::Base,
                    101,
                ),
                carrier(
                    300,
                    "00000000-0000-4000-8000-000000000003",
                    placed(2, translated),
                    101,
                ),
            ],
            components,
        )
    }

    #[test]
    fn a_root_contained_carrier_takes_its_component_path_pose() {
        // The module's carrier stores a quarter turn at (3, 4) cm, and its
        // component-path carrier the identity turn at 0.5 cm along x: the
        // module is placed 5 mm along x. Its child's carrier pose stays native.
        let (mut carriers, components) = nested_fixture();
        let pose = |x: f64| {
            let mut rows = super::IDENTITY;
            rows[0][3] = x;
            Some(crate::records::xref::XrefCurrentPose {
                carrier_record_index: 900,
                frame_bytes: Vec::new().into(),
                transform: Some(rows.try_into().unwrap()),
            })
        };
        carriers[0].current_pose = pose(0.5);
        carriers[1].current_pose = pose(9.0);
        let empty = BTreeSet::new();
        let projection = super::project_local_components(&super::LocalComponentSources {
            hidden_bodies: None,
            scopes: &[],
            occurrences: &carriers,
            paths: &[],
            components: &components,
            body_bindings: &[],
            bodies: Some(&empty),
            root_ordinal_start: 0,
            limits: cadmpeg_core::decode::ResourceLimits::default(),
        })
        .unwrap();
        let parent = crate::ids::neutral_component_occurrence_id(&carriers[0].occurrence_guid);
        let transform = |id: &cadmpeg_ir::ids::OccurrenceId| {
            projection
                .occurrences
                .iter()
                .find(|occurrence| &occurrence.id == id)
                .unwrap()
                .transform
                .rows()
        };
        let rows = transform(&parent);
        let expected = [
            [1.0, 0.0, 0.0, 5.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
        ];
        for (row, expected) in rows.iter().zip(expected) {
            for (value, expected) in row.iter().zip(expected) {
                assert!((value - expected).abs() < 1e-12, "{rows:?}");
            }
        }
        let child =
            crate::ids::nested_component_occurrence_id(&parent, &carriers[1].occurrence_guid);
        assert_eq!(
            transform(&child),
            cadmpeg_ir::transform::Transform::identity().rows()
        );
        let codes = projection
            .losses
            .iter()
            .map(|loss| loss.code.clone())
            .collect::<Vec<_>>();
        assert_eq!(
            codes,
            [
                crate::loss::F3dLossCode::LocalOccurrenceCurrentPose.kind(),
                crate::loss::F3dLossCode::LocalOccurrenceCurrentPoseNative.kind(),
            ]
        );
    }

    #[test]
    fn empty_final_body_set_places_local_and_nested_groups_but_pending_bodies_defer() {
        let (carriers, components) = nested_fixture();
        let empty = BTreeSet::new();
        for bodies in [Some(&empty), None] {
            let projection = super::project_local_components(&super::LocalComponentSources {
                hidden_bodies: None,
                scopes: &[],
                occurrences: &carriers,
                paths: &[],
                components: &components,
                body_bindings: &[],
                bodies,
                root_ordinal_start: 0,
                limits: cadmpeg_core::decode::ResourceLimits::default(),
            })
            .unwrap();
            if bodies.is_some() {
                assert!(projection.losses.is_empty(), "{:?}", projection.losses);
                assert_eq!(projection.definitions.len(), 3);
                assert!(projection
                    .definitions
                    .iter()
                    .all(|definition| definition.bodies.is_empty()));
                assert_eq!(projection.occurrences.len(), 4);
                let parent =
                    crate::ids::neutral_component_occurrence_id(&carriers[0].occurrence_guid);
                for child in &carriers[1..] {
                    let id =
                        crate::ids::nested_component_occurrence_id(&parent, &child.occurrence_guid);
                    let occurrence = projection
                        .occurrences
                        .iter()
                        .find(|occurrence| occurrence.id == id)
                        .unwrap();
                    assert_eq!(
                        occurrence.parent,
                        OccurrenceParent::Occurrence {
                            occurrence: parent.clone()
                        }
                    );
                }
            } else {
                assert!(projection.occurrences.is_empty());
                assert!(projection.definitions.is_empty());
                assert!(projection
                    .losses
                    .iter()
                    .all(|loss| loss.code
                        == crate::loss::F3dLossCode::LocalOccurrenceUnplaced.kind()));
                assert!(projection
                    .losses
                    .iter()
                    .any(|loss| loss.message.contains("body ownership is not known")));
            }
        }
    }

    #[test]
    fn nested_instances_compose_local_frames_and_repeat_shared_prototypes() {
        const TOLERANCE: f64 = 1e-12;
        let (mut carriers, components) = nested_fixture();
        let mut second = carriers[0].clone();
        second.id = format!("{STREAM}:design-component-occurrence#400");
        second.occurrence_guid = guid("00000000-0000-4000-8000-000000000004").into();
        carriers.push(second);
        let projection = project_carriers(&carriers, &components, &[COMPONENT_BODY]);
        assert!(projection.losses.is_empty(), "{:?}", projection.losses);
        assert_eq!(projection.definitions.len(), 3);
        assert_eq!(projection.occurrences.len(), 7);
        let graph = cadmpeg_ir::products::AssemblyGraph::new(&projection.occurrences).unwrap();
        let prototype = PrototypeReference::Local {
            definition: crate::ids::neutral_component_id(&guid(COMPONENT_CONTEXT)),
        };
        let leaves = projection
            .occurrences
            .iter()
            .filter(|occurrence| occurrence.prototype == prototype)
            .collect::<Vec<_>>();
        assert_eq!(leaves.len(), 4);
        assert_eq!(
            leaves
                .iter()
                .map(|leaf| &leaf.id)
                .collect::<BTreeSet<_>>()
                .len(),
            4
        );
        assert_eq!(
            projection
                .definitions
                .iter()
                .flat_map(|definition| &definition.bodies)
                .count(),
            1
        );
        let paths =
            super::paths::LocalOccurrencePaths::new(&carriers, &[], &projection.occurrences);
        assert!(paths
            .resolve(STREAM, &[carriers[1].occurrence_guid.as_str().to_owned()])
            .is_none());
        for parent in [&carriers[0], &carriers[3]] {
            let parent_id = crate::ids::neutral_component_occurrence_id(&parent.occurrence_guid);
            for (child, y_min, face_x) in [(&carriers[1], 40.0, 10.0), (&carriers[2], 50.0, 0.0)] {
                let id =
                    crate::ids::nested_component_occurrence_id(&parent_id, &child.occurrence_guid);
                let pose = cadmpeg_test_support::assembly::resolved_transform(&graph, &id).unwrap();
                let leaf = graph.occurrence(&id).unwrap();
                assert_eq!(
                    leaf.parent,
                    OccurrenceParent::Occurrence {
                        occurrence: parent_id.clone()
                    }
                );
                assert_eq!(leaf.prototype, prototype);
                assert_eq!(
                    paths.resolve(
                        STREAM,
                        &[
                            parent.occurrence_guid.as_str().to_owned(),
                            child.occurrence_guid.as_str().to_owned()
                        ]
                    ),
                    Some((id, 2))
                );
                let mut min = [f64::INFINITY; 3];
                let mut max = [f64::NEG_INFINITY; 3];
                for x in [0.0, 10.0] {
                    for y in [0.0, 10.0] {
                        for z in [0.0, 10.0] {
                            let point = pose
                                .apply_point(cadmpeg_ir::math::Point3::new(x, y, z))
                                .unwrap()
                                .get();
                            for (axis, value) in [point.x, point.y, point.z].into_iter().enumerate()
                            {
                                min[axis] = min[axis].min(value);
                                max[axis] = max[axis].max(value);
                            }
                        }
                    }
                }
                for (actual, expected) in
                    min.into_iter()
                        .chain(max)
                        .zip([20.0, y_min, 0.0, 30.0, y_min + 10.0, 10.0])
                {
                    assert!((actual - expected).abs() < TOLERANCE);
                }
                for y in [0.0, 10.0] {
                    for z in [0.0, 10.0] {
                        let face = pose
                            .apply_point(cadmpeg_ir::math::Point3::new(face_x, y, z))
                            .unwrap()
                            .get();
                        assert!((face.y - 50.0).abs() < TOLERANCE);
                    }
                }
            }
        }
    }

    #[test]
    fn an_operation_placement_stays_local_beneath_its_containing_instance() {
        const TOLERANCE: f64 = 1e-12;
        let (carriers, components) = nested_fixture();
        let mut transform = super::IDENTITY;
        transform[0][3] = 2.0;
        let mut scope = DesignParameterScope::empty(
            &format!("{STREAM}:design-parameter-scope#600"),
            crate::records::feature::scope::DesignFeatureKind::DerivedInstance,
            600,
        );
        if let crate::records::feature::scope::DesignScopePayloadMut::DerivedInstance(slot) =
            scope.payload_mut()
        {
            *slot = Some(DesignDerivedInstanceConstruction {
                reference_record_index: 159,
                relation_record_index: 601,
                carrier_record_index: 300,
                component_guid: carriers[2].component_guid.clone(),
                occurrence_guid: carriers[2].occurrence_guid.clone(),
                transform: transform.try_into().unwrap(),
                transform_offset: 0,
            });
        }
        let bodies = BTreeSet::new();
        let projection = super::project_local_components(&super::LocalComponentSources {
            hidden_bodies: None,
            scopes: &[scope],
            occurrences: &carriers,
            paths: &[],
            components: &components,
            body_bindings: &[],
            bodies: Some(&bodies),
            root_ordinal_start: 0,
            limits: cadmpeg_core::decode::ResourceLimits {
                max_entities: 4,
                ..Default::default()
            },
        })
        .unwrap();
        assert!(projection.losses.is_empty());
        let graph = cadmpeg_ir::products::AssemblyGraph::new(&projection.occurrences).unwrap();
        assert!(graph
            .occurrence(&crate::ids::neutral_component_occurrence_id(
                &carriers[2].occurrence_guid
            ))
            .is_none());
        let parent = crate::ids::neutral_component_occurrence_id(&carriers[0].occurrence_guid);
        let child =
            crate::ids::nested_component_occurrence_id(&parent, &carriers[2].occurrence_guid);
        let rows = cadmpeg_test_support::assembly::resolved_transform(&graph, &child)
            .unwrap()
            .rows();
        assert!((rows[0][3] - 30.0).abs() < TOLERANCE);
        assert!((rows[1][3] - 60.0).abs() < TOLERANCE);
        assert!(
            (graph.occurrence(&child).unwrap().transform.rows()[0][3] - 20.0).abs() < TOLERANCE
        );
    }

    #[test]
    fn occurrence_keys_are_local_to_their_design_stream() {
        const OTHER_STREAM: &str = "f3d:DesignOther/BulkStream.dat";
        let mut components = two_component_stream(Some(276));
        components.insert(OTHER_STREAM.to_owned(), components[STREAM].clone());
        let first = carrier(
            100,
            "00000000-0000-4000-8000-000000000001",
            DesignComponentOccurrencePlacement::Base,
            DOCUMENT_ROOT,
        );
        let mut second = first.clone();
        second.id = format!("{OTHER_STREAM}:design-component-occurrence#100");
        let carriers = [first, second];
        let projection = project_carriers(&carriers, &components, &[]);
        assert!(projection.losses.is_empty());
        assert_eq!(projection.occurrences.len(), 4);
        let graph = cadmpeg_ir::products::AssemblyGraph::new(&projection.occurrences).unwrap();
        let paths =
            super::paths::LocalOccurrencePaths::new(&carriers, &[], &projection.occurrences);
        let key = vec![carriers[0].occurrence_guid.as_str().to_owned()];
        let first = paths.resolve(STREAM, &key).unwrap().0;
        let second = paths.resolve(OTHER_STREAM, &key).unwrap().0;
        assert_ne!(first, second);
        assert_ne!(
            graph.occurrence(&first).unwrap().parent,
            graph.occurrence(&second).unwrap().parent
        );
    }

    #[test]
    fn deep_identity_owner_chains_resolve_through_the_assembly_graph() {
        let mut components = two_component_stream(Some(276));
        let stream = components.get_mut(STREAM).unwrap();
        let mut carriers = Vec::new();
        let mut owner = DOCUMENT_ROOT;
        for index in 0..64 {
            let record = 1000 + index * 2;
            stream.roots.insert(
                record,
                ComponentDefinition {
                    root_entity: record + 1,
                    context_uuid: guid(&format!("00000000-0000-4000-8000-{record:012}")),
                    name: None,
                },
            );
            let mut child = carrier(
                record,
                &format!("00000000-0000-4000-8001-{record:012}"),
                DesignComponentOccurrencePlacement::Base,
                owner,
            );
            child.component_record_index = record;
            carriers.push(child);
            owner = record + 1;
        }
        let projection = project_carriers(&carriers, &components, &[]);
        assert!(projection.losses.is_empty());
        assert_eq!(projection.occurrences.len(), 65);
        let graph = cadmpeg_ir::products::AssemblyGraph::new(&projection.occurrences).unwrap();
        for occurrence in &projection.occurrences {
            assert_eq!(
                cadmpeg_test_support::assembly::resolved_transform(&graph, &occurrence.id).unwrap(),
                cadmpeg_ir::transform::Transform::identity()
            );
        }
    }

    #[test]
    fn malformed_owners_and_expansion_limits_have_distinct_losses() {
        let (mut carriers, components) = nested_fixture();
        carriers[1].owner_entity = 999;
        let projection = project_carriers(&carriers, &components, &[]);
        assert_eq!(projection.losses.len(), 1);
        assert_eq!(
            projection.losses[0].code,
            crate::loss::F3dLossCode::LocalOccurrenceOwnerUnbound.kind()
        );
        let (carriers, components) = nested_fixture();
        let bodies = BTreeSet::new();
        let projection = super::project_local_components(&super::LocalComponentSources {
            hidden_bodies: None,
            scopes: &[],
            occurrences: &carriers,
            paths: &[],
            components: &components,
            body_bindings: &[],
            bodies: Some(&bodies),
            root_ordinal_start: 0,
            limits: cadmpeg_core::decode::ResourceLimits {
                max_entities: 3,
                ..Default::default()
            },
        })
        .unwrap();
        assert_eq!(projection.occurrences.len(), 3);
        assert_eq!(projection.losses.len(), 1);
        assert_eq!(
            projection.losses[0].code,
            crate::loss::F3dLossCode::LocalOccurrenceExpansionLimit.kind()
        );
        cadmpeg_ir::products::AssemblyGraph::new(&projection.occurrences).unwrap();
    }

    #[test]
    fn local_component_placement_scales_only_translation_to_millimetres() {
        let transform = [
            [0.0, -1.0, 0.0, 1.25],
            [1.0, 0.0, 0.0, -2.5],
            [0.0, 0.0, 1.0, 3.75],
            [0.0, 0.0, 0.0, 1.0],
        ];
        assert_eq!(
            super::neutral_transform(transform).unwrap().rows(),
            [
                [0.0, -1.0, 0.0, 12.5],
                [1.0, 0.0, 0.0, -25.0],
                [0.0, 0.0, 1.0, 37.5],
                [0.0, 0.0, 0.0, 1.0],
            ]
        );
    }

    #[test]
    fn equal_component_guids_share_one_definition_across_carrier_references() {
        const COMPONENT: &str = "11111111-2222-4333-8444-555555555555";
        const SOURCE: &str = "aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee";
        const COPY: &str = "aaaaaaaa-bbbb-4ccc-8ddd-ffffffffffff";
        let occurrence = |record_index: u32, component_record_index: u64, occurrence_guid: &str| {
            DesignComponentOccurrence::try_new(
                crate::records::feature::assembly_features::DesignComponentOccurrenceDraft {
                    id: format!(
                        "f3d:Design/BulkStream.dat:design-component-occurrence#{record_index}"
                    ),
                    class_tag: crate::records::references::DesignClassTag::try_from(
                        "256".to_owned(),
                    )
                    .unwrap(),
                    record_index,
                    byte_offset: u64::from(record_index),
                    component_record_index,
                    component_guid: COMPONENT.to_owned().try_into().expect("GUID"),
                    occurrence_guid: occurrence_guid.to_owned().try_into().expect("GUID"),
                    placement: crate::records::feature::assembly_features::DesignComponentOccurrencePlacement::Base,
                    owner_entity: 0,
                },
            )
            .unwrap()
        };
        let native_occurrences = [occurrence(100, 700, SOURCE), occurrence(101, 701, COPY)];
        let mut scope = DesignParameterScope::empty(
            "f3d:Design/BulkStream.dat:design-parameter-scope#10",
            crate::records::feature::scope::DesignFeatureKind::CopyPaste,
            10,
        );
        if let crate::records::feature::scope::DesignScopePayloadMut::CopyPaste(slot) =
            scope.payload_mut()
        {
            *slot = Some(DesignCopyPasteComponentOperation {
                relation_record_index: 20,
                source_occurrence_record_index: 100,
                copied_occurrence_record_index: 101,
                component_guid: COMPONENT.to_owned().try_into().expect("GUID"),
                source_occurrence_guid: SOURCE.to_owned().try_into().expect("GUID"),
                copied_occurrence_guid: COPY.to_owned().try_into().expect("GUID"),
                source_transform: identity_matrix().try_into().unwrap(),
                source_transform_offset: 0,
                copied_transform: identity_matrix().try_into().unwrap(),
                copied_transform_offset: 0,
            });
        }

        let (definitions, occurrences) =
            project_without_naming_spaces(&[scope], &native_occurrences);

        assert_eq!(definitions.len(), 1);
        assert_eq!(occurrences.len(), 2);
        let definition = definitions[0].id.clone();
        assert!(occurrences.iter().all(|occurrence| matches!(
            &occurrence.prototype,
            PrototypeReference::Local { definition: actual } if actual == &definition
        )));
    }

    #[test]
    fn derived_instance_projects_its_joined_local_occurrence() {
        const COMPONENT: &str = "11111111-2222-4333-8444-555555555555";
        const OCCURRENCE: &str = "aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee";
        let mut scope = DesignParameterScope::empty(
            "f3d:Design/BulkStream.dat:design-parameter-scope#385",
            crate::records::feature::scope::DesignFeatureKind::DerivedInstance,
            385,
        );
        if let crate::records::feature::scope::DesignScopePayloadMut::DerivedInstance(slot) =
            scope.payload_mut()
        {
            *slot = Some(DesignDerivedInstanceConstruction {
                reference_record_index: 305,
                relation_record_index: 383,
                carrier_record_index: 382,
                component_guid: COMPONENT.to_owned().try_into().expect("GUID"),
                occurrence_guid: OCCURRENCE.to_owned().try_into().expect("GUID"),
                transform: identity_matrix().try_into().unwrap(),
                transform_offset: 473,
            });
        }
        let native_occurrence = DesignComponentOccurrence::try_new(
            crate::records::feature::assembly_features::DesignComponentOccurrenceDraft {
                id: "f3d:Design/BulkStream.dat:design-component-occurrence#382".into(),
                class_tag: crate::records::references::DesignClassTag::try_from("380".to_owned())
                    .unwrap(),
                record_index: 382,
                byte_offset: 0,
                component_record_index: 305,
                component_guid: COMPONENT.to_owned().try_into().expect("GUID"),
                occurrence_guid: OCCURRENCE.to_owned().try_into().expect("GUID"),
                placement: crate::records::feature::assembly_features::DesignComponentOccurrencePlacement::Explicit {
                    ordinal: std::num::NonZeroU32::MIN,
                    transform: identity_matrix().try_into().unwrap(),
                },
                owner_entity: 0,
            },
        )
        .unwrap();
        let (definitions, occurrences) = project_without_naming_spaces(
            std::slice::from_ref(&scope),
            std::slice::from_ref(&native_occurrence),
        );
        assert_eq!(definitions.len(), 1);
        assert_eq!(occurrences.len(), 1);
        assert_eq!(
            occurrences[0].id,
            crate::ids::neutral_component_occurrence_id(
                &crate::records::mesh::DesignOccurrenceKey::try_from(OCCURRENCE.to_owned())
                    .unwrap()
            )
        );
        assert!(matches!(
            &occurrences[0].prototype,
            PrototypeReference::Local { definition }
                if definition == &crate::ids::neutral_component_id(&COMPONENT.to_owned().try_into().unwrap())
        ));

        let mut feature = Feature {
            product_definition: None,
            id: FeatureId::mint("f3d:model:feature#derived").expect("identity grammar"),
            ordinal: 1,
            name: None,
            suppressed: None,
            dependencies: cadmpeg_ir::features::DistinctMembers::default(),
            source_properties: std::collections::BTreeMap::default(),
            source_tag: None,
            source_text: None,
            source_content: cadmpeg_ir::features::FeatureContent::default(),
            evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
                FeatureDefinition::Operation(FeatureOperation::Native {
                    kind: "DerivedInstance".into(),
                    parameters: std::collections::BTreeMap::new(),
                }),
            ),
            native_ref: None,
        };
        feature.native_ref = Some(scope.id.clone());
        super::project_derived_instance_features(
            std::slice::from_mut(&mut feature),
            std::slice::from_ref(&scope),
        );
        assert_eq!(
            *feature.evaluation.definition(),
            FeatureDefinition::Operation(FeatureOperation::InsertComponent {
                occurrence: crate::ids::neutral_component_occurrence_id(
                    &crate::records::mesh::DesignOccurrenceKey::try_from(OCCURRENCE.to_owned())
                        .unwrap()
                ),
            })
        );
        let inserted = feature.evaluation.definition().clone();
        feature
            .evaluation
            .set_definition(FeatureDefinition::Operation(FeatureOperation::Native {
                kind: "DerivedInstance".into(),
                parameters: BTreeMap::new(),
            }));
        let (_, operation_occurrences) =
            project_without_naming_spaces(std::slice::from_ref(&scope), &[]);
        assert_eq!(operation_occurrences.len(), 1);
        super::project_derived_instance_features(
            std::slice::from_mut(&mut feature),
            std::slice::from_ref(&scope),
        );
        let paths = super::paths::LocalOccurrencePaths::new(&[], &[], &operation_occurrences);
        assert!(
            super::paths::bind_feature_paths(std::slice::from_mut(&mut feature), &paths).is_empty()
        );
        assert_eq!(*feature.evaluation.definition(), inserted);
    }

    fn component_insert_scope(record_index: u32, role: &str) -> DesignParameterScope {
        let mut scope = DesignParameterScope::empty(
            &format!("{STREAM}:design-parameter-scope#{record_index}"),
            crate::records::feature::scope::DesignFeatureKind::ComponentInsert,
            record_index,
        );
        if let crate::records::feature::scope::DesignScopePayloadMut::ComponentInsert(slot) =
            scope.payload_mut()
        {
            *slot = Some(
                crate::records::feature::assembly_features::DesignComponentInsertConstruction {
                    relation_record_index: record_index + 1,
                    carrier_record_index: record_index + 2,
                    occurrence_identity: None,
                    neutron_role: role.into(),
                    neutron_role_offset: 0,
                    placement: None,
                },
            );
        }
        scope
    }

    fn placed_component_insert_scope(
        record_index: u32,
        role: &str,
        translation: [f64; 3],
    ) -> DesignParameterScope {
        let mut scope = component_insert_scope(record_index, role);
        let mut matrix = identity_matrix();
        for (row, value) in matrix.iter_mut().zip(translation) {
            row[3] = value;
        }
        if let crate::records::feature::scope::DesignScopePayloadMut::ComponentInsert(Some(
            construction,
        )) = scope.payload_mut()
        {
            construction.placement = Some(
                crate::records::feature::assembly_features::DesignComponentInsertMatrix {
                    scope: crate::records::identity::Located {
                        value: matrix.try_into().unwrap(),
                        offset: 0,
                    },
                    carrier_offset: None,
                },
            );
        }
        scope
    }

    fn resolved_role_reference() -> crate::records::xref::XrefReference {
        crate::records::xref::XrefReference {
            current_pose: None,
            operation_pose: None,
            contained_poses: Vec::new(),
            owned_poses: Vec::new(),
            id: "f3d:xref:reference#0-occurrence-0".into(),
            ordinal: 0,
            occurrence_ordinal: 0,
            placement_source: Some(crate::records::xref::XrefPlacementSource {
                frame_bytes: Vec::new().into(),
                occurrence_guid: None,
                stream: STREAM.into(),
                carrier_record_index: 52,
                owner: None,
            }),
            from: "root.f3d".to_owned().try_into().unwrap(),
            relative_path: "part.f3d".to_owned().try_into().unwrap(),
            neutron_role: "resolved-role".to_owned().try_into().unwrap(),
            neutron_data: "data".into(),
            transform: Some(identity_matrix().try_into().unwrap()),
        }
    }

    fn loss_codes(inserts: &super::ComponentInserts) -> Vec<cadmpeg_ir::report::loss::LossKind> {
        inserts.losses().into_iter().map(|loss| loss.code).collect()
    }

    #[test]
    fn external_component_features_and_joint_operands_use_unique_instance_paths() {
        let scope = component_insert_scope(50, "resolved-role");
        let scopes = [scope.clone()];
        let timelines = crate::design::test_support::synthetic_feature_timelines(&scopes);
        let table = crate::xref::XrefTable {
            references: vec![resolved_role_reference()],
            ..Default::default()
        };
        let components = BTreeMap::new();
        let bodies = BTreeSet::new();
        let sources = super::LocalComponentSources {
            hidden_bodies: None,
            scopes: &[],
            occurrences: &[],
            paths: &[],
            components: &components,
            body_bindings: &[],
            bodies: Some(&bodies),
            root_ordinal_start: 0,
            limits: cadmpeg_core::decode::ResourceLimits::default(),
        };
        let base = crate::xref::project_occurrences(&table, &sources, &[])
            .unwrap()
            .occurrences
            .remove(0);
        let mut foreign = table.references[0].clone();
        foreign.id = "f3d:xref/foreign/xref:reference#0-occurrence-0".to_owned();
        foreign.placement_source.as_mut().unwrap().stream =
            "f3d:OtherDesign/BulkStream.dat".to_owned();
        let references = [foreign, table.references[0].clone()];
        let mut projected = Vec::new();
        for count in 1..=2 {
            let mut occurrence = base.clone();
            let parent =
                cadmpeg_ir::ids::OccurrenceId::mint(format!("f3d:model:occurrence#group-{count}"))
                    .unwrap();
            occurrence.id = crate::ids::nested_xref_occurrence_id(&parent, 0, 0);
            occurrence.parent = OccurrenceParent::Occurrence { occurrence: parent };
            projected.push(occurrence);
            let mut inserts = super::ComponentInserts::decide(&scopes, &timelines, &references);
            inserts.bind_external_paths(&references, &projected);
            let mut feature = Feature {
                product_definition: None,
                id: FeatureId::mint("f3d:model:feature#external").unwrap(),
                ordinal: 0,
                name: None,
                suppressed: None,
                dependencies: cadmpeg_ir::features::DistinctMembers::default(),
                source_properties: BTreeMap::new(),
                source_tag: None,
                source_text: None,
                source_content: cadmpeg_ir::features::FeatureContent::default(),
                evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
                    FeatureDefinition::Operation(FeatureOperation::Native {
                        kind: "ComponentInsert".into(),
                        parameters: BTreeMap::new(),
                    }),
                ),
                native_ref: Some(scope.id.clone()),
            };
            let losses = crate::xref::bind_component_insert_features(
                std::slice::from_mut(&mut feature),
                &scopes,
                &table,
                &projected,
            );
            if count == 1 {
                assert_eq!(inserts.occurrence(&scope.id), Some(&projected[0].id));
                assert!(losses.is_empty());
                assert!(
                    matches!(feature.evaluation.definition(), FeatureDefinition::Operation(FeatureOperation::InsertComponent { occurrence }) if occurrence == &projected[0].id)
                );
            } else {
                assert!(inserts.occurrence(&scope.id).is_none());
                assert!(matches!(
                    feature.evaluation.definition(),
                    FeatureDefinition::Operation(FeatureOperation::Native { .. })
                ));
                assert_eq!(losses.len(), 1);
                assert_eq!(
                    losses[0].code,
                    crate::loss::F3dLossCode::FeatureOccurrencePathUnresolved.kind()
                );
            }
        }
    }

    #[test]
    fn unresolved_component_insert_occurrences_follow_scopes_without_features() {
        let unresolved = component_insert_scope(40, "unresolved-role");
        let resolved = component_insert_scope(50, "resolved-role");
        let scopes = [unresolved.clone(), resolved.clone()];
        let timelines = crate::design::test_support::synthetic_feature_timelines(&scopes);

        let inserts =
            super::ComponentInserts::decide(&scopes, &timelines, &[resolved_role_reference()]);
        let occurrences = inserts.unresolved_occurrences(3).unwrap();

        assert_eq!(occurrences.len(), 1);
        assert_eq!(
            occurrences[0].id,
            crate::ids::neutral_component_insert_occurrence_id(&unresolved)
        );
        assert_eq!(occurrences[0].ordinal, 3);
        assert_eq!(
            occurrences[0].native_ref.as_deref(),
            Some(unresolved.id.as_str())
        );
        assert!(matches!(
            occurrences[0].prototype,
            PrototypeReference::Unresolved {}
        ));
        assert_eq!(inserts.occurrence(&unresolved.id), Some(&occurrences[0].id));
        assert_eq!(
            inserts.occurrence(&resolved.id),
            Some(&crate::ids::neutral_xref_occurrence_id(0, 0))
        );
        assert!(inserts.losses().is_empty());
    }

    #[test]
    fn an_unlisted_component_insert_scope_is_an_operand_without_an_occurrence() {
        let listed = component_insert_scope(40, "listed-role");
        let operand = component_insert_scope(50, "operand-role");
        let timelines =
            crate::design::test_support::synthetic_feature_timelines(std::slice::from_ref(&listed));

        let inserts =
            super::ComponentInserts::decide(&[listed.clone(), operand.clone()], &timelines, &[]);

        let occurrences = inserts.unresolved_occurrences(0).unwrap();
        assert_eq!(occurrences.len(), 1);
        assert_eq!(
            occurrences[0].native_ref.as_deref(),
            Some(listed.id.as_str())
        );
        assert_eq!(inserts.occurrence(&operand.id), None);
        assert!(inserts.losses().is_empty());
    }

    #[test]
    fn component_inserts_without_an_authored_order_are_not_projected() {
        let scopes = [
            component_insert_scope(40, "first-role"),
            component_insert_scope(50, "second-role"),
        ];

        let inserts = super::ComponentInserts::decide(&scopes, &[], &[]);

        assert!(inserts.unresolved_occurrences(0).unwrap().is_empty());
        assert_eq!(inserts.occurrence(&scopes[0].id), None);
        assert_eq!(
            loss_codes(&inserts),
            [crate::loss::F3dLossCode::ComponentInsertOrderUnknown.kind()]
        );
    }

    #[test]
    fn a_component_insert_without_a_millimetre_placement_is_skipped_with_a_loss() {
        let unplaced = placed_component_insert_scope(40, "unplaced-role", [1.0e308, 0.0, 0.0]);
        let placed = placed_component_insert_scope(50, "placed-role", [1.0, 2.0, 3.0]);
        let scopes = [unplaced.clone(), placed.clone()];
        let timelines = crate::design::test_support::synthetic_feature_timelines(&scopes);

        let inserts = super::ComponentInserts::decide(&scopes, &timelines, &[]);

        let occurrences = inserts.unresolved_occurrences(0).unwrap();
        assert_eq!(occurrences.len(), 1);
        assert_eq!(
            occurrences[0].native_ref.as_deref(),
            Some(placed.id.as_str())
        );
        assert_eq!(
            occurrences[0].transform.rows()[0][3],
            10.0,
            "translation projects to millimetres"
        );
        assert_eq!(inserts.occurrence(&unplaced.id), None);
        assert_eq!(
            loss_codes(&inserts),
            [crate::loss::F3dLossCode::ComponentInsertUnplaced.kind()]
        );
    }

    #[test]
    fn unresolved_component_insert_features_bind_to_their_scope_occurrence() {
        let scope = component_insert_scope(40, "unresolved-role");
        let scopes = std::slice::from_ref(&scope);
        let inserts = super::ComponentInserts::decide(
            scopes,
            &crate::design::test_support::synthetic_feature_timelines(scopes),
            &[],
        );
        let occurrences = inserts.unresolved_occurrences(0).unwrap();
        let mut feature = Feature {
            product_definition: None,
            id: FeatureId::mint("f3d:model:feature#insert").expect("identity grammar"),
            ordinal: 1,
            name: None,
            suppressed: None,
            dependencies: cadmpeg_ir::features::DistinctMembers::default(),
            source_properties: std::collections::BTreeMap::default(),
            source_tag: None,
            source_text: None,
            source_content: cadmpeg_ir::features::FeatureContent::default(),
            evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
                FeatureDefinition::Operation(FeatureOperation::Native {
                    kind: "ComponentInsert".into(),
                    parameters: std::collections::BTreeMap::new(),
                }),
            ),
            native_ref: Some(scope.id.clone()),
        };
        let native = feature.evaluation.definition().clone();

        super::bind_unresolved_component_insert_features(
            std::slice::from_mut(&mut feature),
            scopes,
            &[],
        );
        assert_eq!(
            *feature.evaluation.definition(),
            native,
            "no projected occurrence leaves the feature native"
        );

        super::bind_unresolved_component_insert_features(
            std::slice::from_mut(&mut feature),
            scopes,
            &occurrences,
        );
        assert_eq!(
            *feature.evaluation.definition(),
            FeatureDefinition::Operation(FeatureOperation::InsertComponent {
                occurrence: crate::ids::neutral_component_insert_occurrence_id(&scope),
            })
        );
    }
}
