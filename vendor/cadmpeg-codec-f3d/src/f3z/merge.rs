// SPDX-License-Identifier: Apache-2.0
//! Shared product definitions and instance trees for decoded F3Z members.

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::annotations::{AnnotationBuilder, StreamHandle};
use cadmpeg_ir::document::{EntityRewrite, Model};
use cadmpeg_ir::ids::UnknownId;
use cadmpeg_ir::SourceFidelity;
use cadmpeg_ir::{Native, NativeRecord};
use serde::de::DeserializeOwned;
use serde_json::{Map, Value};

use super::archive::ClassifiedMember;
use super::{F3zDecodedMember, F3zSession};
use crate::container::ContainerScan;
use crate::loss::F3dLossCode;
use crate::records::xref::XrefReference;
use crate::xref::{self, XrefTable};

/// Merges the root document's recursively reachable members in archive scope.
pub(super) fn merge_archive(
    ctx: &DecodeContext<'_>,
    session: &F3zSession<'_>,
    decoded: &mut std::collections::BTreeMap<String, F3zDecodedMember>,
    operation_poses: super::session::OperationPoses,
    ir: &mut cadmpeg_ir::CadIr,
    report: &mut cadmpeg_ir::codec::DecodeBody,
    fidelity: &mut cadmpeg_ir::SourceFidelity,
) -> Result<usize, CodecError> {
    let scan = &session.scan;
    let table = xref_table_from_ir(ctx, ir)?;

    let stack = vec![session.model_root.clone()];
    MergeSession {
        ctx,
        scan,
        archive: &session.archive,
        session,
        decoded,
        stack,
        members: std::collections::BTreeMap::new(),
        failed: std::collections::BTreeMap::new(),
        operation_poses,
        operation_protected: std::collections::BTreeSet::new(),
        component_path_tables: Vec::new(),
    }
    .merge(ir, report, fidelity, &table)
}

/// Reassigns only repeated sibling ordinals after independent document graphs
/// have been combined.
pub(super) fn make_sibling_ordinals_unique(
    occurrences: &mut [cadmpeg_ir::products::Occurrence],
) -> Result<(), CodecError> {
    use std::collections::{HashMap, HashSet};

    // Every ordinal below a parent's floor is in use, so the search for the
    // smallest free ordinal resumes there and each parent's search is linear
    // over the whole pass.
    let mut used = HashMap::<Option<&cadmpeg_ir::ids::OccurrenceId>, (HashSet<u32>, u64)>::new();
    for occurrence in occurrences {
        let parent = match &occurrence.parent {
            cadmpeg_ir::products::OccurrenceParent::Root {} => None,
            cadmpeg_ir::products::OccurrenceParent::Occurrence { occurrence } => Some(occurrence),
        };
        let (siblings, floor) = used.entry(parent).or_default();
        let ordinal = if siblings.contains(&occurrence.ordinal) {
            let free = (*floor..=u64::from(u32::MAX))
                .filter_map(|candidate| u32::try_from(candidate).ok())
                .find(|candidate| !siblings.contains(candidate))
                .ok_or_else(|| {
                    CodecError::malformed(
                        "F3Z sibling occurrence population exhausts the u32 ordinal space",
                    )
                })?;
            *floor = u64::from(free) + 1;
            free
        } else {
            occurrence.ordinal
        };
        siblings.insert(ordinal);
        occurrence.ordinal = ordinal;
    }
    Ok(())
}

fn xref_table_from_ir(
    ctx: &DecodeContext<'_>,
    ir: &cadmpeg_ir::CadIr,
) -> Result<XrefTable, CodecError> {
    fn load_arena<T: DeserializeOwned>(
        ctx: &DecodeContext<'_>,
        namespace: &cadmpeg_ir::NativeNamespace,
        name: &str,
    ) -> Result<Vec<T>, CodecError> {
        match namespace.arena_as_for_decode(ctx, name) {
            Ok(records) => Ok(records),
            Err(error) => match CodecError::from(error) {
                error @ CodecError::ResourceLimit(_) => Err(error),
                CodecError::Malformed(message) => Err(CodecError::malformed(format_args!(
                    "invalid F3D native data: {message}"
                ))),
                error => Err(error),
            },
        }
    }
    let Some(namespace) = ir.native.namespace("f3d") else {
        return Ok(XrefTable::default());
    };
    let references: Vec<XrefReference> = load_arena(ctx, namespace, "xref_references")?;
    let unplaced_references = references
        .iter()
        .filter(|reference| !xref::has_projected_placement(reference, &ir.model.occurrences))
        .map(|reference| reference.id.clone())
        .collect();
    Ok(XrefTable {
        designs: load_arena(ctx, namespace, "xref_designs")?,
        references,
        unplaced_references,
        ..Default::default()
    })
}

struct MemberPlacement<'a> {
    reference: &'a XrefReference,
    parent: cadmpeg_ir::ids::OccurrenceId,
    scope: String,
}

fn member_placements<'a>(
    model: &Model,
    table: &'a XrefTable,
    containing_scope: Option<&str>,
) -> Vec<MemberPlacement<'a>> {
    use cadmpeg_ir::products::PrototypeReference;

    let mut placements = Vec::new();
    for reference in &table.references {
        for occurrence in model.occurrences.iter().filter(|occurrence| {
            matches!(occurrence.prototype, PrototypeReference::External { .. })
                && occurrence.native_ref.as_deref() == Some(reference.id.as_str())
        }) {
            let key = occurrence_key(reference);
            let mut scope = containing_scope
                .map_or_else(|| key.clone(), |parent| format!("{parent}/xref/{key}"));
            let neutral = crate::ids::neutral_xref_occurrence_id(
                reference.ordinal,
                reference.occurrence_ordinal,
            );
            let expected = containing_scope
                .and_then(|scope| rescope(neutral.as_str(), scope))
                .unwrap_or_else(|| neutral.as_str().to_owned());
            if occurrence.id.as_str() != expected {
                scope = format!(
                    "{scope}/instance-{}",
                    EscapedOccurrenceComponent(occurrence.id.as_str())
                );
            }
            placements.push(MemberPlacement {
                reference,
                parent: occurrence.id.clone(),
                scope,
            });
        }
    }
    placements
}

#[derive(Clone)]
struct MemberDefinition {
    definition: cadmpeg_ir::ids::ProductDefinitionId,
    scope: String,
    occurrences: Vec<cadmpeg_ir::products::Occurrence>,
    joints: Vec<cadmpeg_ir::products::AssemblyJoint>,
    table: XrefTable,
}

/// State shared by recursive F3Z reference traversal.
struct MergeSession<'r, 'a> {
    ctx: &'r DecodeContext<'a>,
    scan: &'r ContainerScan<'a>,
    archive: &'r super::archive::ArchiveSession,
    session: &'r F3zSession<'a>,
    decoded: &'r mut std::collections::BTreeMap<String, F3zDecodedMember>,
    stack: Vec<String>,
    members: std::collections::BTreeMap<String, MemberDefinition>,
    /// Members whose decode failed, by path, with the failure text. A failed
    /// member is decoded once; every later reference reports the same loss.
    failed: std::collections::BTreeMap<String, String>,
    operation_poses: std::collections::BTreeMap<String, crate::xref::operation_pose::OperationPose>,
    operation_protected: std::collections::BTreeSet<cadmpeg_ir::ids::OccurrenceId>,
    component_path_tables: Vec<(cadmpeg_ir::ids::OccurrenceId, XrefTable)>,
}

impl MergeSession<'_, '_> {
    fn merge(
        &mut self,
        parent_ir: &mut cadmpeg_ir::CadIr,
        parent_report: &mut cadmpeg_ir::codec::DecodeBody,
        parent_fidelity: &mut cadmpeg_ir::SourceFidelity,
        table: &XrefTable,
    ) -> Result<usize, CodecError> {
        let placements = member_placements(&parent_ir.model, table, None);
        let mut member_roots = std::collections::BTreeMap::new();
        let merged = self.merge_placements(
            parent_ir,
            parent_report,
            parent_fidelity,
            table,
            placements,
            &mut member_roots,
        )?;
        self.members.clear();
        self.decoded.clear();
        rebind_joint_descent(
            self.ctx,
            parent_ir,
            &member_roots,
            &mut parent_report.losses,
        )?;
        let unplaced_members = self
            .component_path_tables
            .iter()
            .filter(|(_, table)| !table.unplaced_references.is_empty())
            .map(|(boundary, _)| boundary.clone())
            .collect();
        omit_unbound_member_joints(
            &mut parent_ir.model,
            &unplaced_members,
            &mut parent_report.losses,
        );
        let (legacy, qualified): (Vec<_>, Vec<_>) =
            table.references.iter().cloned().partition(|reference| {
                reference
                    .contained_poses
                    .iter()
                    .all(|pose| !xref::component_path_has_inline_local_type(&pose.frame_bytes))
                    && reference
                        .owned_poses
                        .iter()
                        .all(|pose| !xref::component_path_has_inline_local_type(&pose.frame_bytes))
            });
        let legacy_table = XrefTable {
            references: legacy,
            ..Default::default()
        };
        let qualified_table = XrefTable {
            references: qualified,
            ..Default::default()
        };
        let mut left_identity = place_contained_poses_with_protected(
            self.ctx,
            parent_ir,
            &legacy_table,
            &mut parent_report.losses,
            &mut self.operation_protected,
            ContainedPass::Ordered,
            None,
        )?;
        crate::xref::operation_pose::project(
            self.ctx,
            parent_ir,
            &self.operation_poses,
            &self.operation_protected,
            &mut parent_report.losses,
        )?;
        for (boundary, member_table) in &self.component_path_tables {
            left_identity += place_contained_poses_with_protected(
                self.ctx,
                parent_ir,
                member_table,
                &mut parent_report.losses,
                &mut self.operation_protected,
                ContainedPass::Ordered,
                Some(boundary),
            )?;
        }
        left_identity += place_contained_poses_with_protected(
            self.ctx,
            parent_ir,
            &qualified_table,
            &mut parent_report.losses,
            &mut self.operation_protected,
            ContainedPass::Ordered,
            None,
        )?;
        crate::xref::operation_pose::project(
            self.ctx,
            parent_ir,
            &self.operation_poses,
            &self.operation_protected,
            &mut parent_report.losses,
        )?;
        left_identity += place_contained_poses_with_protected(
            self.ctx,
            parent_ir,
            table,
            &mut parent_report.losses,
            &mut std::collections::BTreeSet::new(),
            ContainedPass::Nested,
            None,
        )?;
        crate::design::assembly::release_joint_placed(&mut parent_report.losses, left_identity);
        parent_report
            .losses
            .retain(|loss| loss.code != F3dLossCode::SavedFreeEdgeBodyUnowned.kind());
        parent_report
            .losses
            .extend(crate::decode::saved_free_edge_bodies_loss(&parent_ir.model));
        Ok(merged)
    }

    fn merge_placements(
        &mut self,
        parent_ir: &mut cadmpeg_ir::CadIr,
        parent_report: &mut cadmpeg_ir::codec::DecodeBody,
        parent_fidelity: &mut cadmpeg_ir::SourceFidelity,
        table: &XrefTable,
        placements: Vec<MemberPlacement<'_>>,
        member_roots: &mut std::collections::BTreeMap<
            cadmpeg_ir::ids::OccurrenceId,
            cadmpeg_ir::ids::OccurrenceId,
        >,
    ) -> Result<usize, CodecError> {
        use cadmpeg_ir::products::PrototypeReference;

        let mut merged = 0usize;
        for placement in placements {
            let reference = placement.reference;
            let occurrence = placement.scope;
            let label = xref::design_for(table, reference)
                .map_or(reference.relative_path.as_str(), |design| {
                    design.display_name.as_str()
                });
            if self
                .stack
                .iter()
                .any(|path| path == reference.relative_path.as_str())
            {
                super::push_loss(
                    &mut parent_report.losses,
                    F3dLossCode::XrefCycle,
                    format_args!(
                        "xref {label}: reference cycle through {}; the occurrence was not resolved",
                        reference.relative_path
                    ),
                );
                continue;
            }
            let ctx = self.ctx;
            let _depth = ctx.enter_nested("f3z member reference")?;
            if let Some(error) = self.failed.get(reference.relative_path.as_str()) {
                super::push_loss(&mut parent_report.losses, F3dLossCode::XrefMemberUndecoded, format_args!(
                    "xref {label}: member {} failed to decode ({error}); the occurrence was not resolved",
                    reference.relative_path
                ));
                continue;
            }
            let member = if let Some(member) = self.members.get(reference.relative_path.as_str()) {
                self.ctx.charge_entities(
                    cadmpeg_core::decode::u64_from_index(
                        member
                            .occurrences
                            .len()
                            .checked_add(member.joints.len())
                            .ok_or_else(|| {
                                CodecError::malformed("F3Z repeated member entity count overflows")
                            })?,
                    ),
                    "F3Z repeated member occurrences and joints",
                )?;
                member.clone()
            } else {
                let Some(member) = self.archive.members.get(reference.relative_path.as_str())
                else {
                    if self.scan.has_entry(&reference.relative_path) {
                        super::push_loss(&mut parent_report.losses, F3dLossCode::XrefMemberUndecoded, format_args!(
                            "xref {label}: member {} is not an F3D document member; the occurrence was not resolved",
                            reference.relative_path
                        ));
                    } else {
                        super::push_loss(&mut parent_report.losses, F3dLossCode::XrefMemberMissing, format_args!(
                            "xref {label}: member {} is not present in the archive; the occurrence was not resolved",
                            reference.relative_path
                        ));
                    }
                    continue;
                };
                match member {
                    ClassifiedMember::Scanned { .. } => {}
                    ClassifiedMember::Unreadable(_) => continue,
                }
                let component = match self.session.take_member(
                    self.ctx,
                    reference.relative_path.as_str(),
                    self.decoded,
                ) {
                    Ok((component, component_poses)) => {
                        for (key, pose) in &component_poses {
                            let key = rescope(key, &occurrence).ok_or_else(|| {
                                CodecError::malformed("invalid F3Z operation-pose identity")
                            })?;
                            self.operation_poses.insert(key, pose.clone());
                        }
                        component
                    }
                    Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
                    Err(error) => {
                        super::push_loss(&mut parent_report.losses, F3dLossCode::XrefMemberUndecoded, format_args!(
                            "xref {label}: member {} failed to decode ({error}); the occurrence was not resolved",
                            reference.relative_path
                        ));
                        self.failed.insert(
                            reference.relative_path.as_str().to_owned(),
                            error.to_string(),
                        );
                        continue;
                    }
                };
                let mut child_table = xref_table_from_ir(self.ctx, &component.ir)?;
                let cadmpeg_ir::codec::Decoded {
                    ir: mut component_ir,
                    body: mut component_report,
                    source_fidelity: component_fidelity,
                } = component;
                scope_document_parameters(self.ctx, &mut component_ir.model)?;
                let definition = define_member(self.ctx, &mut component_ir.model, label)?;
                append_feature_history(&parent_ir.model, &mut component_ir.model)?;
                let mut scope = OccurrenceScope {
                    ctx: self.ctx,
                    occurrence: &occurrence,
                };
                let occurrence_start = parent_ir.model.occurrences.len();
                let joint_start = parent_ir.model.assembly_joints.len();
                parent_ir
                    .model
                    .extend_rewritten(component_ir.model, &mut scope)?;
                let member = MemberDefinition {
                    definition: scope.rewrite(definition)?,
                    scope: occurrence.clone(),
                    occurrences: parent_ir
                        .model
                        .occurrences
                        .drain(occurrence_start..)
                        .collect(),
                    joints: parent_ir
                        .model
                        .assembly_joints
                        .drain(joint_start..)
                        .collect(),
                    table: {
                        for reference in &mut child_table.references {
                            reference.id =
                                rescope(&reference.id, &occurrence).ok_or_else(|| {
                                    CodecError::malformed("invalid F3Z reference identity")
                                })?;
                        }
                        child_table
                    },
                };
                extend_native(
                    self.ctx,
                    &mut parent_ir.native,
                    component_ir.native,
                    &occurrence,
                )?;
                parent_fidelity.append(rescope_fidelity(component_fidelity, &occurrence)?)?;
                if component_report.transfer.geometry_transferred() {
                    parent_report.transfer = cadmpeg_ir::report::decode::DecodeTransfer::at(
                        parent_report.transfer.depth(),
                        true,
                    );
                }
                for loss in &mut component_report.losses {
                    loss.message = format!("xref {label}: {}", loss.message);
                }
                parent_report.losses.append(&mut component_report.losses);
                self.members
                    .insert(reference.relative_path.as_str().to_owned(), member.clone());
                member
            };
            let mut instance = member.instantiate(self.ctx, &occurrence, &placement.parent)?;
            let child_placements = member_placements(&instance, &member.table, Some(&occurrence));
            for root in &instance.occurrences {
                if matches!(root.parent, cadmpeg_ir::products::OccurrenceParent::Root {})
                    && matches!(root.prototype, PrototypeReference::Local { .. })
                    && root.native_ref.is_none()
                {
                    member_roots.insert(root.id.clone(), placement.parent.clone());
                }
            }
            reparent_component_roots(&mut instance.occurrences, &placement.parent)?;
            parent_ir.model.occurrences.extend(instance.occurrences);
            parent_ir
                .model
                .assembly_joints
                .extend(instance.assembly_joints);
            let placing = parent_ir
                .model
                .occurrences
                .iter_mut()
                .find(|item| item.id == placement.parent)
                .ok_or_else(|| CodecError::malformed("missing F3Z placing occurrence"))?;
            placing.prototype = PrototypeReference::Local {
                definition: member.definition,
            };
            self.stack.push(reference.relative_path.as_str().to_owned());
            let descendants = self.merge_placements(
                parent_ir,
                parent_report,
                parent_fidelity,
                &member.table,
                child_placements,
                member_roots,
            )?;
            self.stack.pop();
            self.component_path_tables
                .push((placement.parent.clone(), member.table.clone()));
            merged += descendants + 1;
            parent_report.notes.push(format!(
                "xref {label}: merged {} as occurrence {occurrence} (shared definition; {descendants} nested occurrence(s))",
                reference.relative_path
            ));
        }
        Ok(merged)
    }
}

fn omit_unbound_member_joints(
    model: &mut Model,
    unplaced_members: &std::collections::BTreeSet<cadmpeg_ir::ids::OccurrenceId>,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
) {
    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};
    use cadmpeg_ir::products::{OccurrenceParent, OperandContainer};
    if unplaced_members.is_empty() {
        return;
    }
    let parents = model
        .occurrences
        .iter()
        .filter_map(|occurrence| {
            let OccurrenceParent::Occurrence { occurrence: parent } = &occurrence.parent else {
                return None;
            };
            Some((occurrence.id.clone(), parent.clone()))
        })
        .collect::<std::collections::BTreeMap<_, _>>();
    let mut ancestors = unplaced_members.clone();
    for member in unplaced_members {
        let mut current = member;
        for _ in 0..parents.len() {
            let Some(parent) = parents.get(current) else {
                break;
            };
            ancestors.insert(parent.clone());
            current = parent;
        }
    }
    let mut omitted = std::collections::BTreeSet::new();
    model.assembly_joints.retain(|joint| {
        let unresolved = joint.connectors().any(|connector| {
            let OperandContainer::Occurrence { occurrence, occurrence_path } = &connector.operand.container else {
                return matches!(connector.operand.container, OperandContainer::External { .. });
            };
            if occurrence_path.is_empty() && connector.operand.subelements.is_empty() {
                return false;
            }
            let mut current = occurrence;
            for _ in 0..=parents.len() {
                if ancestors.contains(current) { return true; }
                let Some(parent) = parents.get(current) else { break };
                current = parent;
            }
            false
        });
        if unresolved {
            omitted.insert(joint.id.clone());
            losses.push(F3dLossCode::AssemblyJointUnprojected.note(format!(
                "joint {} stays native because its operand path crosses a member with an omitted external placement",
                joint.id
            )));
        }
        !unresolved
    });
    for feature in &mut model.features {
        if matches!(feature.evaluation.definition(), FeatureDefinition::Operation(FeatureOperation::AssemblyJoint { joint }) if omitted.contains(joint))
        {
            feature
                .evaluation
                .set_definition(FeatureDefinition::Operation(FeatureOperation::Native {
                    kind: feature
                        .source_tag
                        .clone()
                        .unwrap_or_else(|| "Assemble".to_owned())
                        .into(),
                    parameters: std::collections::BTreeMap::new(),
                }));
        }
    }
}

fn rebind_joint_descent(
    ctx: &DecodeContext<'_>,
    ir: &mut cadmpeg_ir::CadIr,
    member_roots: &std::collections::BTreeMap<
        cadmpeg_ir::ids::OccurrenceId,
        cadmpeg_ir::ids::OccurrenceId,
    >,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
) -> Result<(), CodecError> {
    use crate::records::feature::assembly_features::DesignComponentOccurrence;
    use cadmpeg_ir::products::{OccurrenceParent, OperandContainer};

    if ir.model.assembly_joints.is_empty() {
        return Ok(());
    }
    let Some(namespace) = ir.native.namespace("f3d") else {
        return Ok(());
    };
    let carriers = namespace
        .arena_iter_as_for_decode::<DesignComponentOccurrence>(ctx, "design_component_occurrences")
        .map(|carrier| carrier.map(|carrier| (carrier.id, carrier.occurrence_guid)))
        .collect::<Result<std::collections::BTreeMap<_, _>, _>>()?;
    let mut children = std::collections::BTreeMap::new();
    for occurrence in &ir.model.occurrences {
        let Some(key) = occurrence
            .native_ref
            .as_deref()
            .and_then(|id| carriers.get(id))
        else {
            continue;
        };
        let OccurrenceParent::Occurrence { occurrence: parent } = &occurrence.parent else {
            continue;
        };
        let mut parent = parent;
        for _ in 0..=member_roots.len() {
            children
                .entry((parent.clone(), key.as_str().to_ascii_lowercase()))
                .and_modify(|selected| *selected = None)
                .or_insert(Some(&occurrence.id));
            let Some(outer) = member_roots.get(parent) else {
                break;
            };
            parent = outer;
        }
    }
    let external = namespace
        .arena_iter_as_for_decode::<XrefReference>(ctx, "xref_references")
        .filter_map(|reference| match reference {
            Ok(reference) => reference
                .placement_source
                .and_then(|source| source.occurrence_guid.map(|guid| Ok((reference.id, guid)))),
            Err(error) => Some(Err(error)),
        })
        .collect::<Result<std::collections::BTreeMap<_, _>, _>>()?;
    let mut complete_children = children.clone();
    for occurrence in &ir.model.occurrences {
        let Some(key) = occurrence
            .native_ref
            .as_deref()
            .and_then(|id| external.get(id))
        else {
            continue;
        };
        let OccurrenceParent::Occurrence { occurrence: parent } = &occurrence.parent else {
            continue;
        };
        let mut parent = parent;
        for _ in 0..=member_roots.len() {
            complete_children
                .entry((parent.clone(), key.as_str().to_ascii_lowercase()))
                .and_modify(|selected| *selected = None)
                .or_insert(Some(&occurrence.id));
            let Some(outer) = member_roots.get(parent) else {
                break;
            };
            parent = outer;
        }
    }
    let types = namespace
        .arena_iter_as_for_decode::<crate::records::entity_header::SegmentType>(ctx, "design_types")
        .collect::<Result<Vec<_>, _>>()?;
    let mut types_by_stream = std::collections::HashMap::new();
    let mut complete_scopes = std::collections::BTreeSet::new();
    for scope in namespace
        .arena_iter_as_for_decode::<crate::records::feature::scope::DesignParameterScope>(
            ctx,
            "design_parameter_scopes",
        )
    {
        let scope = scope?;
        let Some(stream) = crate::ids::native_stream(&scope.id) else {
            continue;
        };
        let stream_types = types_by_stream.entry(stream.to_owned()).or_insert_with(|| {
            stream
                .strip_prefix("f3d:")
                .and_then(crate::ids::decode_identity_key_component)
                .map(|entry| crate::design::decode::meta::stream_types_by_entity(&types, &entry))
                .unwrap_or_default()
        });
        if crate::design::decode::scopes::assembly_v3::joint_binds_after_merge(&scope, stream_types)
        {
            complete_scopes.insert(scope.id);
        }
    }
    let instances = ir
        .model
        .occurrences
        .iter()
        .map(|occurrence| &occurrence.id)
        .collect::<std::collections::BTreeSet<_>>();
    let mut refused = std::collections::BTreeSet::new();
    for joint in &mut ir.model.assembly_joints {
        let joint_id = joint.id.clone();
        if joint
            .native_ref
            .as_ref()
            .is_some_and(|id| complete_scopes.contains(id))
        {
            if !retain_complete_joint(joint, &complete_children, &instances, losses) {
                refused.insert(joint_id.clone());
            }
            continue;
        }
        for connector in joint.connectors_mut() {
            let operand = &mut connector.operand;
            // An operand that already names a nested document path is not
            // rebound: its path is relative to the current container.
            let OperandContainer::Occurrence {
                occurrence,
                occurrence_path,
            } = &operand.container
            else {
                continue;
            };
            if !occurrence_path.is_empty() {
                continue;
            }
            let mut selected = occurrence;
            let mut consumed = 0;
            let mut ambiguous = false;
            for key in &operand.subelements {
                match children.get(&(selected.clone(), key.to_ascii_lowercase())) {
                    Some(Some(child)) => {
                        selected = child;
                        consumed += 1;
                    }
                    Some(None) => {
                        ambiguous = true;
                        losses.push(F3dLossCode::AssemblyOperandDescentAmbiguous.note(format!(
                            "joint {joint_id}: operand descent key {key} selects multiple merged child occurrences; the operand is unchanged"
                        )));
                        break;
                    }
                    None => break,
                }
            }
            if consumed != 0 && !ambiguous {
                operand.container = OperandContainer::Occurrence {
                    occurrence: selected.clone(),
                    occurrence_path: Vec::new(),
                };
                operand.subelements.drain(..consumed);
            }
        }
    }
    ir.model
        .assembly_joints
        .retain(|joint| !refused.contains(&joint.id));
    bind_retained_joint_features(
        &mut ir.model.features,
        &ir.model.assembly_joints,
        &complete_scopes,
    );
    Ok(())
}

/// Bind the features of joints that survived complete descent. A refused joint
/// has no row, so its feature keeps its native definition and parameters.
fn bind_retained_joint_features(
    features: &mut [cadmpeg_ir::features::Feature],
    joints: &[cadmpeg_ir::products::AssemblyJoint],
    complete_scopes: &std::collections::BTreeSet<String>,
) {
    let retained = joints
        .iter()
        .filter(|joint| {
            joint
                .native_ref
                .as_ref()
                .is_some_and(|scope| complete_scopes.contains(scope.as_str()))
        })
        .cloned()
        .collect::<Vec<_>>();
    crate::design::assembly::bind_assembly_joint_features(features, &retained);
}

type JointChildren<'a> = std::collections::BTreeMap<
    (cadmpeg_ir::ids::OccurrenceId, String),
    Option<&'a cadmpeg_ir::ids::OccurrenceId>,
>;

fn retain_complete_joint(
    joint: &mut cadmpeg_ir::products::AssemblyJoint,
    children: &JointChildren<'_>,
    instances: &std::collections::BTreeSet<&cadmpeg_ir::ids::OccurrenceId>,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
) -> bool {
    if complete_joint_descent(joint, children, instances) {
        return true;
    }
    losses.push(F3dLossCode::AssemblyOperandDescentUnresolved.note(format!(
        "joint {}: the complete occurrence path has no unique merged insertion instance; the joint stays native", joint.id
    )));
    false
}

fn complete_joint_descent(
    joint: &mut cadmpeg_ir::products::AssemblyJoint,
    children: &JointChildren<'_>,
    instances: &std::collections::BTreeSet<&cadmpeg_ir::ids::OccurrenceId>,
) -> bool {
    use cadmpeg_ir::products::OperandContainer;
    for connector in joint.connectors_mut() {
        let operand = &mut connector.operand;
        let OperandContainer::Occurrence {
            occurrence,
            occurrence_path,
        } = &operand.container
        else {
            if !matches!(operand.container, OperandContainer::Root {})
                || !operand.subelements.is_empty()
            {
                return false;
            }
            continue;
        };
        if !instances.contains(occurrence) || !occurrence_path.is_empty() {
            return false;
        }
        let mut selected = occurrence;
        for key in &operand.subelements {
            let Some(Some(child)) = children.get(&(selected.clone(), key.to_ascii_lowercase()))
            else {
                return false;
            };
            selected = child;
        }
        operand.container = OperandContainer::Occurrence {
            occurrence: selected.clone(),
            occurrence_path: Vec::new(),
        };
        operand.subelements.clear();
    }
    true
}

/// Contained poses of one instance agree to a tenth of a micrometre in
/// translation; the file stores them exactly, so this admits only rounding.
const CONTAINED_POSE_TRANSLATION_MM: f64 = 1.0e-4;
/// Contained poses of one instance agree to a nanoradian in rotation.
const CONTAINED_POSE_ROTATION: f64 = 1.0e-9;

/// Places each external occurrence at the pose that the component-path
/// carriers of the occurrences inside it store
/// ([spec §1.4](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#14-external-references)).
///
/// A carrier names an occurrence inside the referenced design, the external
/// occurrence's placement record and some of its local owners, innermost
/// first, and stores the inner occurrence's world pose. The inner occurrence's
/// merged placement below the external occurrence then gives the external
/// occurrence's world pose. An instance takes it only when its owners begin with the
/// carrier's and every carrier that reaches it through the most owners gives the
/// same pose.
#[cfg(test)]
fn place_contained_poses(
    ctx: &DecodeContext<'_>,
    ir: &mut cadmpeg_ir::CadIr,
    table: &XrefTable,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
) -> Result<usize, CodecError> {
    let ordered = place_contained_poses_with_protected(
        ctx,
        ir,
        table,
        losses,
        &mut std::collections::BTreeSet::new(),
        ContainedPass::Ordered,
        None,
    )?;
    let nested = place_contained_poses_with_protected(
        ctx,
        ir,
        table,
        losses,
        &mut std::collections::BTreeSet::new(),
        ContainedPass::Nested,
        None,
    )?;
    Ok(ordered + nested)
}

/// The carrier forms that one placement pass reads. Carriers of a nested
/// design state the nested occurrence's pose against its final placement below
/// the external occurrence, so they place after the operation tables have
/// posed the occurrences inside it; all other carriers place before.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ContainedPass {
    Ordered,
    Nested,
}

impl ContainedPass {
    /// Whether this pass reads one contained carrier of a reference.
    fn reads(
        self,
        reference: &XrefReference,
        pose: &crate::records::xref::XrefContainedPose,
    ) -> bool {
        match self {
            Self::Ordered => !xref::contained_pose_is_nested(pose),
            Self::Nested => {
                pose.owner_record_indices.is_empty()
                    && xref::contained_pose_is_nested(pose)
                    && (nested_contained_only(reference)
                        || (reference.owned_poses.is_empty()
                            && reference
                                .placement_source
                                .as_ref()
                                .is_some_and(|placement| {
                                    xref::component_path_names_contained(
                                        &pose.frame_bytes,
                                        placement.carrier_record_index,
                                        pose.contained_record_index,
                                    )
                                })))
            }
        }
    }

    /// Whether this pass answers for one contained carrier of a reference,
    /// which stays native when the pass does not bind it.
    fn owns(self, reference: &XrefReference) -> bool {
        match self {
            Self::Ordered => !nested_contained_only(reference) || !reference.owned_poses.is_empty(),
            Self::Nested => {
                reference.owned_poses.is_empty()
                    && reference
                        .contained_poses
                        .iter()
                        .any(|pose| self.reads(reference, pose))
            }
        }
    }

    /// Whether this pass places the reference's instances.
    fn places(self, reference: &XrefReference) -> bool {
        match self {
            Self::Ordered => {
                !reference.owned_poses.is_empty()
                    || reference
                        .contained_poses
                        .iter()
                        .any(|pose| self.reads(reference, pose))
            }
            Self::Nested => {
                reference.owned_poses.is_empty()
                    && reference
                        .contained_poses
                        .iter()
                        .any(|pose| self.reads(reference, pose))
            }
        }
    }
}

/// Whether every contained carrier of the reference is of a nested design.
fn nested_contained_only(reference: &XrefReference) -> bool {
    !reference.contained_poses.is_empty()
        && reference
            .contained_poses
            .iter()
            .all(xref::contained_pose_is_nested)
}

fn contained_frames_agree(
    left: cadmpeg_ir::transform::Transform,
    right: cadmpeg_ir::transform::Transform,
) -> bool {
    left.rows()
        .iter()
        .flatten()
        .zip(right.rows().iter().flatten())
        .enumerate()
        .all(|(at, (a, b))| {
            let tolerance = if at % 4 == 3 {
                CONTAINED_POSE_TRANSLATION_MM
            } else {
                CONTAINED_POSE_ROTATION
            };
            (a - b).abs() <= tolerance
        })
}

fn place_contained_poses_with_protected(
    ctx: &DecodeContext<'_>,
    ir: &mut cadmpeg_ir::CadIr,
    table: &XrefTable,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
    protected: &mut std::collections::BTreeSet<cadmpeg_ir::ids::OccurrenceId>,
    pass: ContainedPass,
    boundary: Option<&cadmpeg_ir::ids::OccurrenceId>,
) -> Result<usize, CodecError> {
    use crate::records::feature::assembly_features::DesignComponentOccurrence;
    use cadmpeg_ir::products::OccurrenceParent;
    use cadmpeg_ir::transform::Transform;
    use std::collections::{BTreeMap, BTreeSet};

    if !table
        .references
        .iter()
        .any(|reference| pass.places(reference))
    {
        return Ok(0);
    }
    let Some(namespace) = ir.native.namespace("f3d") else {
        return Ok(0);
    };
    // The record index of each local carrier, and of each placed external
    // reference, by native identity. Members' records are scoped below their
    // first instance, so a record index names one only within its scope.
    let mut records = namespace
        .arena_iter_as_for_decode::<DesignComponentOccurrence>(ctx, "design_component_occurrences")
        .map(|carrier| carrier.map(|carrier| (carrier.id, u64::from(carrier.record_index))))
        .collect::<Result<BTreeMap<_, _>, _>>()?;
    for reference in namespace.arena_iter_as_for_decode::<XrefReference>(ctx, "xref_references") {
        let reference = reference?;
        if let Some(source) = &reference.placement_source {
            records.insert(reference.id, source.carrier_record_index);
        }
    }
    let index = ir
        .model
        .occurrences
        .iter()
        .enumerate()
        .map(|(position, occurrence)| (occurrence.id.clone(), position))
        .collect::<BTreeMap<_, _>>();
    let parent_of = |position: usize| match ir.model.occurrences.get(position).map(|o| &o.parent) {
        Some(OccurrenceParent::Occurrence { occurrence }) => index.get(occurrence).copied(),
        _ => None,
    };
    let boundary_index = boundary
        .map(|id| {
            index.get(id).copied().ok_or_else(|| {
                CodecError::malformed("F3Z component-path source occurrence is absent")
            })
        })
        .transpose()?;
    let in_scope = |mut position: usize| {
        if let Some(boundary) = boundary_index {
            for _ in 0..=index.len() {
                let Some(parent) = parent_of(position) else {
                    return false;
                };
                if parent == boundary {
                    return true;
                }
                position = parent;
            }
            false
        } else {
            true
        }
    };
    let mut source_chain = Vec::new();
    let mut at = boundary_index;
    while let Some(position) = at {
        source_chain.push(position);
        if source_chain.len() > index.len() {
            return Err(CodecError::malformed(
                "F3Z component-path source owner cycle",
            ));
        }
        at = parent_of(position);
    }
    let source_world = source_chain
        .into_iter()
        .rev()
        .try_fold(Transform::identity(), |world, position| {
            world.compose(ir.model.occurrences[position].transform)
        })
        .map_err(|_| CodecError::malformed("F3Z component-path source frame overflow"))?;
    let source_scope = boundary_index.and_then(|boundary| {
        ir.model
            .occurrences
            .iter()
            .enumerate()
            .find(|(position, _)| parent_of(*position) == Some(boundary))
            .and_then(|(_, occurrence)| {
                occurrence
                    .id
                    .as_str()
                    .split_once("/model:")
                    .map(|(scope, _)| scope)
            })
    });
    let mut children = BTreeMap::<usize, Vec<usize>>::new();
    for position in 0..ir.model.occurrences.len() {
        if let Some(parent) = parent_of(position) {
            children.entry(parent).or_default().push(position);
        }
    }
    let local_root = |id: &str| !id.starts_with("f3d:xref/");
    let owners_of = |instance: usize| {
        let mut owners = Vec::new();
        let mut at = parent_of(instance);
        while let Some(position) = at {
            if Some(position) == boundary_index {
                break;
            }
            let Some(record) = ir.model.occurrences.get(position).and_then(|occurrence| {
                occurrence
                    .native_ref
                    .as_deref()
                    .filter(|id| {
                        source_scope.map_or_else(
                            || local_root(id),
                            |scope| {
                                ir.model.occurrences[position]
                                    .id
                                    .as_str()
                                    .split_once("/model:")
                                    .is_some_and(|(owner_scope, _)| owner_scope == scope)
                            },
                        )
                    })
                    .and_then(|id| records.get(id))
            }) else {
                break;
            };
            owners.push(*record);
            at = parent_of(position);
            if owners.len() > ir.model.occurrences.len() {
                break;
            }
        }
        owners
    };
    let mut placed = Vec::new();
    let mut disagreeing = 0usize;
    let mut owned_disagreeing = 0usize;
    let mut bound = BTreeSet::new();
    let mut bound_owned = BTreeSet::new();
    let mut owned_placed = 0usize;
    let mut qualified_constraints = false;
    let saved_state_native = table
        .references
        .iter()
        .flat_map(|reference| &reference.contained_poses)
        .any(|pose| !pose.owner_record_indices.is_empty() && xref::contained_pose_is_nested(pose));
    for reference in table
        .references
        .iter()
        .filter(|reference| pass.places(reference))
    {
        let instances = ir
            .model
            .occurrences
            .iter()
            .enumerate()
            .filter(|(position, occurrence)| {
                in_scope(*position)
                    && occurrence.native_ref.as_deref() == Some(reference.id.as_str())
            })
            .map(|(position, _)| position)
            .collect::<Vec<_>>();
        let instance_owners = instances
            .iter()
            .map(|&instance| owners_of(instance))
            .collect::<Vec<_>>();
        let unique_owners = |owners: &[u64]| {
            instance_owners
                .iter()
                .filter(|chain| chain.starts_with(owners))
                .count()
                == 1
        };
        for instance in instances {
            let owners = owners_of(instance);
            let independent = reference.contained_poses.iter().any(|pose| {
                !xref::contained_pose_is_nested(pose)
                    && owners.starts_with(&pose.owner_record_indices)
                    && xref::component_path_has_inline_local_type(&pose.frame_bytes)
            }) || reference.owned_poses.iter().any(|pose| {
                owners.starts_with(&pose.owner_record_indices)
                    && xref::component_path_has_inline_local_type(&pose.frame_bytes)
            }) || (pass == ContainedPass::Nested
                && !nested_contained_only(reference)
                && reference
                    .contained_poses
                    .iter()
                    .any(|pose| pass.reads(reference, pose)));
            qualified_constraints |= independent;
            let member_scopes = children
                .get(&instance)
                .into_iter()
                .flatten()
                .filter_map(|&child| {
                    ir.model
                        .occurrences
                        .get(child)?
                        .id
                        .as_str()
                        .split_once("/model:")
                        .map(|(scope, _)| scope)
                })
                .collect::<BTreeSet<_>>();
            let member_scope = if member_scopes.len() == 1 {
                member_scopes.first().copied()
            } else {
                None
            };
            // The merged occurrences inside this instance, by their record.
            let mut inside = BTreeMap::<u64, Vec<(usize, Transform)>>::new();
            let mut stack = vec![(instance, Transform::identity())];
            while let Some((position, below)) = stack.pop() {
                for &child in children.get(&position).map_or(&[][..], Vec::as_slice) {
                    let Some(occurrence) = ir.model.occurrences.get(child) else {
                        continue;
                    };
                    let local = below.compose(occurrence.transform).map_err(|_| {
                        CodecError::malformed("F3Z contained occurrence transform is not finite")
                    })?;
                    // Only the referenced design's own records: a member nested
                    // inside it has its own record indices.
                    let own = occurrence.native_ref.as_deref().filter(|id| {
                        if independent {
                            member_scope.is_some()
                                && occurrence
                                    .id
                                    .as_str()
                                    .split_once("/model:")
                                    .map(|(scope, _)| scope)
                                    == member_scope
                        } else {
                            !id.strip_prefix("f3d:xref/")
                                .is_some_and(|scoped| scoped.contains("/xref/"))
                        }
                    });
                    if let Some(record) = own.and_then(|id| records.get(id)) {
                        inside.entry(*record).or_default().push((child, local));
                    }
                    stack.push((child, local));
                }
            }
            let reach = reference
                .contained_poses
                .iter()
                .filter(|pose| owners.starts_with(&pose.owner_record_indices))
                .map(|pose| pose.owner_record_indices.len())
                .max()
                .unwrap_or(0);
            let mut target_poses = BTreeMap::<usize, Vec<(usize, Transform, bool)>>::new();
            for (ordinal, pose) in reference.contained_poses.iter().enumerate() {
                // A carrier lists the instance's owners from the innermost out, as
                // far as it goes.
                if !pass.reads(reference, pose)
                    || !owners.starts_with(&pose.owner_record_indices)
                    || (independent
                        && (!unique_owners(&pose.owner_record_indices)
                            || pose.owner_record_indices.len() < reach))
                {
                    continue;
                }
                let Some([(target, below)]) =
                    inside.get(&pose.contained_record_index).map(Vec::as_slice)
                else {
                    continue;
                };
                let Some(stored) =
                    crate::design::components::neutral_affine(pose.transform.map_or(
                        crate::xref::IDENTITY_ROWS,
                        crate::records::xref::XrefPlacementTransform::rows,
                    ))
                else {
                    continue;
                };
                let mut stored = source_world
                    .compose(stored)
                    .map_err(|_| CodecError::malformed("F3Z contained source pose overflow"))?;
                if independent
                    && owners.is_empty()
                    && reference.current_pose.is_some()
                    && reference.owned_poses.is_empty()
                    && reference
                        .placement_source
                        .as_ref()
                        .is_some_and(|placement| {
                            xref::component_path_names_contained(
                                &pose.frame_bytes,
                                placement.carrier_record_index,
                                pose.contained_record_index,
                            )
                        })
                {
                    if let Some(source) =
                        crate::design::components::neutral_affine(reference.transform.map_or(
                            crate::xref::IDENTITY_ROWS,
                            crate::records::xref::XrefPlacementTransform::rows,
                        ))
                    {
                        let source_pose = source_world
                            .compose(source)
                            .and_then(|source| source.compose(*below))
                            .map_err(|_| {
                                CodecError::malformed("F3Z contained source frame overflow")
                            })?;
                        if contained_frames_agree(source_pose, stored) {
                            stored = source_world
                                .compose(ir.model.occurrences[instance].transform)
                                .and_then(|world| world.compose(*below))
                                .map_err(|_| {
                                    CodecError::malformed("F3Z contained current frame overflow")
                                })?;
                        }
                    }
                }
                if let Some(occurrence) = ir.model.occurrences.get(*target) {
                    protected.insert(occurrence.id.clone());
                }
                let implied = below
                    .try_inverse_affine()
                    .and_then(|inverse| stored.compose(inverse))
                    .map_err(|_| CodecError::malformed("F3Z contained pose is not invertible"))?;
                bound.insert((reference.id.as_str(), ordinal));
                let selected = if independent { *target } else { instance };
                let world_pose = if independent { stored } else { implied };
                target_poses.entry(selected).or_default().push((
                    pose.owner_record_indices.len(),
                    world_pose,
                    false,
                ));
            }
            let own_pose = target_poses.is_empty() || independent;
            if own_pose {
                for (ordinal, pose) in reference.owned_poses.iter().enumerate() {
                    if !owners.starts_with(&pose.owner_record_indices)
                        || (independent && !unique_owners(&pose.owner_record_indices))
                    {
                        continue;
                    }
                    let Some(stored) =
                        crate::design::components::neutral_affine(pose.transform.map_or(
                            crate::xref::IDENTITY_ROWS,
                            crate::records::xref::XrefPlacementTransform::rows,
                        ))
                    else {
                        continue;
                    };
                    let stored = source_world
                        .compose(stored)
                        .map_err(|_| CodecError::malformed("F3Z own source pose overflow"))?;
                    bound_owned.insert((reference.id.as_str(), ordinal));
                    target_poses.entry(instance).or_default().push((
                        pose.owner_record_indices.len(),
                        stored,
                        true,
                    ));
                }
            }
            // A carrier that lists fewer owners can keep the pose from before an owner
            // it leaves out moved: the carriers that list the most owners give the
            // pose, and must agree.
            for (target, poses) in target_poses {
                let own_pose = poses.iter().all(|(_, _, own)| *own);
                let Some(reach) = poses.iter().map(|(reach, _, _)| *reach).max() else {
                    continue;
                };
                let poses = poses
                    .into_iter()
                    .filter(|(owners, _, _)| *owners == reach)
                    .map(|(_, pose, _)| pose)
                    .collect::<Vec<_>>();
                let Some(first) = poses.first().copied() else {
                    continue;
                };
                let agree = poses
                    .iter()
                    .all(|pose| contained_frames_agree(*pose, first));
                if !agree || !first.is_proper_rigid() {
                    if own_pose {
                        owned_disagreeing += 1;
                    } else {
                        disagreeing += 1;
                    }
                    continue;
                }
                if let Some(occurrence) = ir.model.occurrences.get(target) {
                    protected.insert(occurrence.id.clone());
                }
                placed.push((target, first));
                if own_pose {
                    owned_placed += 1;
                }
            }
        }
    }
    let unbound = table
        .references
        .iter()
        .flat_map(|reference| {
            reference
                .contained_poses
                .iter()
                .enumerate()
                .filter(move |(_, pose)| {
                    pass.owns(reference)
                        && match pass {
                            ContainedPass::Ordered => {
                                !(ContainedPass::Nested.places(reference)
                                    && ContainedPass::Nested.reads(reference, pose))
                            }
                            ContainedPass::Nested => pass.reads(reference, pose),
                        }
                })
                .map(move |(ordinal, _)| (reference.id.as_str(), ordinal))
        })
        .filter(|key| !bound.contains(key))
        .count();
    let owned_unbound = table
        .references
        .iter()
        .filter(|reference| pass == ContainedPass::Ordered && pass.places(reference))
        .flat_map(|reference| {
            (0..reference.owned_poses.len()).map(move |ordinal| (reference.id.as_str(), ordinal))
        })
        .filter(|key| !bound_owned.contains(key))
        .count();
    let count = placed.len() - owned_placed;
    let mut left_identity = 0usize;
    placed.sort_by_key(|(position, _)| {
        let mut depth = 0;
        let mut at = parent_of(*position);
        while let Some(parent) = at {
            depth += 1;
            if depth > index.len() {
                break;
            }
            at = parent_of(parent);
        }
        depth
    });
    for (position, pose) in placed {
        let mut chain = Vec::new();
        let mut at = match &ir.model.occurrences[position].parent {
            OccurrenceParent::Occurrence { occurrence } => index.get(occurrence).copied(),
            OccurrenceParent::Root { .. } => None,
        };
        while let Some(parent) = at {
            chain.push(parent);
            if chain.len() > index.len() {
                return Err(CodecError::malformed("F3Z occurrence owner cycle"));
            }
            at = match &ir.model.occurrences[parent].parent {
                OccurrenceParent::Occurrence { occurrence } => index.get(occurrence).copied(),
                OccurrenceParent::Root { .. } => None,
            };
        }
        let parent_world = chain
            .iter()
            .rev()
            .try_fold(Transform::identity(), |world, &parent| {
                world.compose(ir.model.occurrences[parent].transform)
            })
            .map_err(|_| CodecError::malformed("F3Z occurrence world transform is not finite"))?;
        let local = parent_world
            .try_inverse_affine()
            .and_then(|inverse| inverse.compose(pose))
            .map_err(|_| CodecError::malformed("F3Z contained pose is not invertible"))?;
        if let Some(occurrence) = ir.model.occurrences.get_mut(position) {
            if occurrence.transform.rows() == Transform::identity().rows()
                && local.rows() != Transform::identity().rows()
            {
                left_identity += 1;
            }
            occurrence.transform = local;
        }
    }
    if count != 0 {
        let message = if qualified_constraints {
            format!("{count} occurrence instance(s) take component-path pose constraints in their composed owner frames; their stored placement records stay native")
        } else {
            format!("{count} external occurrence instance(s) take the pose that the component-path carriers of the occurrences inside them store; their occurrence-placement matrices stay in the native frames")
        };
        losses.push(F3dLossCode::XrefContainedPose.note(message));
    }
    if disagreeing != 0 {
        let message = if qualified_constraints {
            format!("{disagreeing} occurrence instance(s) keep their local placement: equally specific component-path carriers give different poses")
        } else {
            format!("{disagreeing} external occurrence instance(s) keep their placement: the component-path carriers of the occurrences inside them give different poses")
        };
        losses.push(F3dLossCode::XrefContainedPoseDisagrees.note(message));
    }
    if owned_placed != 0 {
        losses.push(F3dLossCode::XrefOwnedPose.note(format!(
            "{owned_placed} external occurrence instance(s) below local owners take the pose their own component-path carriers store; their occurrence-placement matrices stay in the native frames"
        )));
    }
    if owned_disagreeing != 0 {
        losses.push(F3dLossCode::XrefOwnedPoseDisagrees.note(format!(
            "{owned_disagreeing} external occurrence instance(s) below local owners keep their placement: the instance's own component-path carriers give different poses"
        )));
    }
    if owned_unbound != 0 {
        losses.push(F3dLossCode::XrefOwnedPoseNative.note(format!(
            "{owned_unbound} own component-path carrier(s) of external occurrences below local owners stay native: a contained-occurrence carrier places their instance, or no merged instance has their owner chain"
        )));
    }
    if unbound != 0 {
        let message = if qualified_constraints || saved_state_native {
            format!("{unbound} contained component-path carrier(s) stay native: their member, target, owner scope or saved state is unresolved, or a carrier lists more of the instance's owners")
        } else {
            format!("{unbound} component-path carrier(s) of occurrences inside external occurrences name no merged instance with their owner chain; they stay native")
        };
        losses.push(F3dLossCode::XrefContainedPoseNative.note(message));
    }
    Ok(left_identity)
}

impl MemberDefinition {
    fn instantiate(
        &self,
        ctx: &DecodeContext<'_>,
        scope: &str,
        parent: &cadmpeg_ir::ids::OccurrenceId,
    ) -> Result<Model, CodecError> {
        let prefix = format!("f3d:xref/{}/", self.scope);
        let ids = self
            .occurrences
            .iter()
            .map(|occurrence| occurrence.id.as_str())
            .chain(self.joints.iter().map(|joint| joint.id.as_str()))
            .map(|id| {
                let tail = id
                    .strip_prefix(&prefix)
                    .ok_or_else(|| CodecError::malformed("invalid F3Z member instance scope"))?;
                Ok((id, format!("f3d:xref/{scope}/{tail}")))
            })
            .collect::<Result<std::collections::BTreeMap<_, _>, CodecError>>()?;
        let rewrite =
            |id: &str| Ok::<_, CodecError>(ids.get(id).cloned().unwrap_or_else(|| id.to_owned()));
        let occurrences = self
            .occurrences
            .iter()
            .map(|occurrence| {
                cadmpeg_ir::schema::rewrite::identities(ctx, occurrence.clone(), rewrite)
            })
            .collect::<Result<Vec<_>, _>>()?;
        let assembly_joints = self
            .joints
            .iter()
            .map(|joint| {
                let mut joint =
                    cadmpeg_ir::schema::rewrite::identities(ctx, joint.clone(), rewrite)?;
                for connector in joint.connectors_mut() {
                    if matches!(
                        connector.operand.container,
                        cadmpeg_ir::products::OperandContainer::Root {}
                    ) {
                        connector.operand.container =
                            cadmpeg_ir::products::OperandContainer::Occurrence {
                                occurrence: parent.clone(),
                                occurrence_path: Vec::new(),
                            };
                    }
                }
                Ok(joint)
            })
            .collect::<Result<Vec<_>, CodecError>>()?;
        let mut instance = Model::default();
        instance.occurrences = occurrences;
        instance.assembly_joints = assembly_joints;
        Ok(instance)
    }
}

fn define_member(
    ctx: &DecodeContext<'_>,
    model: &mut Model,
    label: &str,
) -> Result<cadmpeg_ir::ids::ProductDefinitionId, CodecError> {
    use cadmpeg_ir::products::{ProductDefinition, ProductDefinitionKind};

    let owned = model
        .product_definitions
        .iter()
        .flat_map(|definition| &definition.bodies)
        .collect::<std::collections::BTreeSet<_>>();
    let bodies = model
        .bodies
        .iter()
        .filter(|body| !owned.contains(&body.id))
        .map(|body| body.id.clone())
        .collect();
    let owned_sketches = model
        .product_definitions
        .iter()
        .flat_map(|definition| &definition.sketches)
        .collect::<std::collections::BTreeSet<_>>();
    let sketches = model
        .sketches
        .iter()
        .filter(|sketch| !owned_sketches.contains(&sketch.id))
        .map(|sketch| sketch.id.clone())
        .collect();
    let id = crate::ids::archive_product_definition_id();
    if let Some(definition) = model
        .product_definitions
        .iter_mut()
        .find(|row| row.id == id)
    {
        definition.bodies.extend(bodies);
        definition.sketches.extend(sketches);
        definition.label = (!label.trim().is_empty()).then(|| label.to_owned());
        return Ok(id);
    }
    ctx.charge_entities(1, "F3Z member product definition")?;
    model.product_definitions.push(ProductDefinition {
        id: id.clone(),
        kind: ProductDefinitionKind::Part,
        source_name: None,
        label: (!label.trim().is_empty()).then(|| label.to_owned()),
        description: None,
        part_number: None,
        bom_properties: std::collections::BTreeMap::new(),
        bodies,
        sketches,
        native_ref: None,
    });
    Ok(id)
}

/// Places every root-level occurrence from a merged member inside the
/// occurrence that owns that member. Child occurrence parents already carry
/// the member-local hierarchy and are left unchanged.
fn reparent_component_roots(
    occurrences: &mut [cadmpeg_ir::products::Occurrence],
    parent: &cadmpeg_ir::ids::OccurrenceId,
) -> Result<(), CodecError> {
    for occurrence in occurrences {
        if matches!(
            occurrence.parent,
            cadmpeg_ir::products::OccurrenceParent::Root {}
        ) {
            let parent_id = String::from_utf8(parent.as_str().as_bytes().to_vec())
                .map_err(CodecError::malformed)?;
            occurrence.parent = cadmpeg_ir::products::OccurrenceParent::Occurrence {
                occurrence: cadmpeg_ir::ids::OccurrenceId::mint(parent_id)
                    .map_err(CodecError::malformed)?,
            };
        }
    }
    Ok(())
}

/// Gives document parameters an equation container before occurrence rescoping.
/// The container is an entity the member decode did not admit, so it is
/// charged here.
fn scope_document_parameters(ctx: &DecodeContext<'_>, model: &mut Model) -> Result<(), CodecError> {
    use cadmpeg_ir::features::{
        DistinctMembers, Feature, FeatureContent, FeatureDefinition, FeatureEvaluation,
        FeatureOperation, FeatureTreeNodeRole, TreeChildren,
    };

    if model
        .parameters
        .iter()
        .all(|parameter| parameter.owner.is_some())
    {
        return Ok(());
    }
    ctx.charge_entities(1, "F3Z document parameter scope")?;
    let owner = crate::ids::document_parameter_scope_id();
    let first = model
        .features
        .iter()
        .map(|feature| feature.ordinal)
        .min()
        .unwrap_or(0);
    for feature in &mut model.features {
        feature.ordinal = feature
            .ordinal
            .checked_sub(first)
            .and_then(|ordinal| ordinal.checked_add(1))
            .ok_or_else(|| CodecError::malformed("merged F3Z feature ordinal exceeds u64::MAX"))?;
    }
    let mut product_owners = model
        .parameters
        .iter()
        .filter(|row| row.owner.is_none())
        .map(|row| row.product_definition.clone());
    let product_definition = product_owners
        .next()
        .flatten()
        .filter(|owner| product_owners.all(|other| other.as_ref() == Some(owner)));
    for parameter in &mut model.parameters {
        if parameter.owner.is_none() {
            parameter.owner = Some(owner.clone());
        }
    }
    model.features.push(Feature {
        product_definition,
        id: owner,
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: DistinctMembers::default(),
        source_properties: std::collections::BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: FeatureContent::default(),
        evaluation: FeatureEvaluation::from_definition(FeatureDefinition::Operation(
            FeatureOperation::TreeNode {
                role: FeatureTreeNodeRole::Equations,
                children: TreeChildren::default(),
            },
        )),
        native_ref: None,
    });
    Ok(())
}

/// Places one component's feature history after the histories already merged.
fn append_feature_history(parent: &Model, component: &mut Model) -> Result<(), CodecError> {
    let Some(component_minimum) = component
        .features
        .iter()
        .map(|feature| feature.ordinal)
        .min()
    else {
        return Ok(());
    };

    let next = parent
        .features
        .iter()
        .map(|feature| feature.ordinal)
        .max()
        .map_or(Ok(0), |ordinal| {
            ordinal.checked_add(1).ok_or_else(|| {
                CodecError::Malformed("merged F3Z feature ordinal exceeds u64::MAX".into())
            })
        })?;

    for feature in &mut component.features {
        feature.ordinal = feature
            .ordinal
            .checked_sub(component_minimum)
            .and_then(|ordinal| ordinal.checked_add(next))
            .ok_or_else(|| {
                CodecError::Malformed("merged F3Z feature ordinal exceeds u64::MAX".into())
            })?;
    }
    Ok(())
}

fn rescope_fidelity(
    source: SourceFidelity,
    occurrence: &str,
) -> Result<SourceFidelity, CodecError> {
    let (mut annotations, records) = source.into_parts();
    annotations
        .map_ids(|id| match rescope(id, occurrence) {
            Some(id) => Ok(id),
            None => Ok(id.to_string()),
        })?
        .map_err(CodecError::from)?;
    // The occurrence is one owner component. Escape its separators so two
    // different occurrences cannot share an owner by shifting a path boundary.
    let owner = cadmpeg_ir::StreamName::try_from(format!(
        "f3d:xref/{}/",
        EscapedOccurrenceComponent(occurrence)
    ))
    .map_err(CodecError::malformed)?;
    let provenance = std::mem::take(&mut annotations.provenance);
    let mut builder = AnnotationBuilder::resume(annotations);
    for (id, provenance) in provenance {
        let stream =
            cadmpeg_ir::StreamName::try_from(format!("{}{}", owner.as_str(), provenance.stream()))
                .map_err(CodecError::malformed)?;
        let stream = StreamHandle::new(stream);
        builder.note(&id, &stream, provenance.offset, provenance.tag.as_deref());
    }
    let mut rescoped = SourceFidelity::with_annotations(builder.build());
    for (id, record) in records {
        let id_text = match rescope(id.as_str(), occurrence) {
            Some(id) => id,
            None => {
                format!("{id}")
            }
        };
        let id = UnknownId::mint(id_text).map_err(|error| {
            CodecError::malformed(format_args!("F3Z retained record {id}: {error}"))
        })?;
        let stream =
            cadmpeg_ir::StreamName::try_from(format!("{}{}", owner.as_str(), record.stream()))
                .map_err(CodecError::malformed)?;

        rescoped.insert_retained_record(id, record.with_owner(stream))?;
    }
    Ok(rescoped)
}

fn occurrence_key(reference: &XrefReference) -> String {
    let role = EscapedOccurrenceComponent(&reference.neutron_role);
    // `occurrence_ordinal` restarts for each Redirections reference. Keep the
    // source reference ordinal in the scope so two admitted rows carrying the
    // same role cannot merge their model or fidelity identities.
    format!(
        "role-{role}/reference-{}/occurrence-{}",
        reference.ordinal, reference.occurrence_ordinal
    )
}

struct EscapedOccurrenceComponent<'a>(&'a str);

impl std::fmt::Display for EscapedOccurrenceComponent<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for character in self.0.chars() {
            if matches!(character, ':' | '#' | '%' | '/') || character.is_whitespace() {
                let mut bytes = [0; 4];
                for byte in character.encode_utf8(&mut bytes).as_bytes() {
                    write!(formatter, "%{byte:02X}")?;
                }
            } else {
                write!(formatter, "{character}")?;
            }
        }
        Ok(())
    }
}

fn rescope(text: &str, occurrence: &str) -> Option<String> {
    text.strip_prefix("f3d:")
        .map(|rest| format!("f3d:xref/{occurrence}/{rest}"))
}

/// Rewrites every `f3d:` identity in one model entity into occurrence scope.
struct OccurrenceScope<'r, 'a> {
    ctx: &'r DecodeContext<'a>,
    occurrence: &'r str,
}

impl EntityRewrite for OccurrenceScope<'_, '_> {
    type Error = CodecError;

    fn rewrite<T: cadmpeg_ir::schema::rewrite::typed::RewriteIdentities>(
        &mut self,
        entity: T,
    ) -> Result<T, CodecError> {
        cadmpeg_ir::schema::rewrite::identities(self.ctx, entity, |id: &str| {
            match rescope(id, self.occurrence) {
                Some(rewritten) => Ok(rewritten),
                None => Ok(id.to_owned()),
            }
        })
    }
}

/// Appends all known component-native arenas after occurrence-local rescoping.
fn extend_native(
    ctx: &DecodeContext<'_>,
    root: &mut Native,
    mut component: Native,
    occurrence: &str,
) -> Result<(), CodecError> {
    let Some(mut source) = component.0.remove("f3d") else {
        return Ok(());
    };
    let target = root.namespace_mut("f3d");
    for name in crate::native::F3D_ARENA_NAMES
        .iter()
        .copied()
        .chain(std::iter::once("unknowns"))
    {
        let Some(records) = source.arenas_mut().remove(name) else {
            continue;
        };
        if records.is_empty() {
            continue;
        }
        let arena = target.arenas_mut().entry(name.to_string()).or_default();

        for record in records {
            arena.push(rescope_record(ctx, &record, name, occurrence)?);
        }
    }
    Ok(())
}

/// Rescopes one native record's identity and every identity it references.
fn rescope_record(
    ctx: &DecodeContext<'_>,
    record: &NativeRecord,
    arena: &str,
    occurrence: &str,
) -> Result<NativeRecord, CodecError> {
    let mut fields = typed_fields(ctx, record, arena, occurrence)?;
    rescope_native_reference_fields(ctx, arena, &mut fields, occurrence)?;
    let id = rescope(record.id(), occurrence).unwrap_or_else(|| record.id().to_string());
    NativeRecord::new(
        cadmpeg_ir::ids::Identity::new(id).map_err(CodecError::malformed)?,
        fields,
    )
    .map_err(CodecError::from)
}

/// Rewrite typed identity markers before JSON erases their ownership.
///
/// Most F3D native records carry source text and numeric stream facts. The
/// records listed here also carry an IR identity marker inside a structured
/// field. Going through the typed owner keeps that marker distinct from an
/// ordinary `String` while the field-specific pass below handles native text
/// references that have not yet gained a newtype.
fn typed_fields(
    ctx: &DecodeContext<'_>,
    record: &NativeRecord,
    arena: &str,
    occurrence: &str,
) -> Result<Map<String, Value>, CodecError> {
    macro_rules! typed {
        ($type:path) => {
            record.rewrite_fields::<$type, _>(ctx, |id| match rescope(id, occurrence) {
                Some(rewritten) => Ok(rewritten),
                None => Ok(id.to_owned()),
            })?
        };
    }

    Ok(match arena {
        "body_visibilities" => typed!(crate::records::bodies::BodyVisibility),
        "creation_timestamps" => typed!(crate::records::recipes::CreationTimestamp),
        "design_body_bindings" => typed!(crate::records::bodies::DesignBodyBinding),
        "design_body_recipe_operands" => {
            typed!(crate::records::topology::body_recipe::DesignBodyRecipeOperand)
        }
        "design_dimension_recipe_records" => {
            typed!(crate::records::dimensions::DesignDimensionRecipeRecord)
        }
        "design_edge_operands" => {
            typed!(crate::records::topology::edge_identity::DesignEdgeOperand)
        }
        "design_edge_treatment_vertex_operands" => {
            typed!(crate::records::feature::work_geometry::DesignEdgeTreatmentVertexOperand)
        }
        "design_face_operands" => typed!(crate::records::topology::face::DesignFaceOperand),
        "design_mesh_features" => typed!(crate::records::mesh::DesignMeshFeature),
        "design_parameter_scopes" => typed!(crate::records::feature::scope::DesignParameterScope),
        "persistent_design_links" => typed!(crate::records::sketch_links::PersistentDesignLink),
        "persistent_subentity_tags" => typed!(crate::records::sketch_links::PersistentSubentityTag),
        "sketch_curve_links" => typed!(crate::records::sketch_links::SketchCurveLink),
        _ => record.copy_fields(ctx)?,
    })
}

/// Rewrite only fields whose owners document an identity relationship.
///
/// Native records intentionally retain arbitrary source strings, including
/// configuration extensions and names that can happen to begin with `f3d:`.
/// Field names are therefore the admission boundary: this list is assembled
/// from the native record definitions and their readers, and no map key or
/// unowned string is traversed as an identity.
fn rescope_native_reference_fields(
    ctx: &DecodeContext<'_>,
    arena: &str,
    fields: &mut Map<String, Value>,
    occurrence: &str,
) -> Result<(), CodecError> {
    let direct_fields: &[&str] = match arena {
        "asm_bulletin_boards"
        | "asm_delta_states"
        | "asm_entity_changes"
        | "asm_face_boundaries"
        | "asm_history_records" => &["parent"],
        "body_native_keys" => &["body"],
        "edge_continuities" => &["edge"],
        "edge_ownerships" => &["edge", "owner_coedge"],
        "face_native_keys" | "face_sidedness" => &["face"],
        "design_body_bounds" => &["body_binding_ids"],
        "design_body_recipe_operands" | "design_edge_operands" | "design_face_operands" => {
            &["recipe_id"]
        }
        "design_dimension_recipe_records" => &["recipe_id", "matching_edge_operand_ids"],
        "design_construction_operand_groups" => &["lost_edge_references"],
        "design_extrude_selection_members" => &["operand_identity_ids"],
        "design_edge_identity_operands" => &["resolution_identity_id"],
        "design_parameter_companions" => &["owned_recipe_ids"],
        "mesh_surface_sentinels" => &["surface"],
        "tolerant_coedge_parameters" => &["coedge"],
        "tolerant_edge_tails" => &["edge"],
        "tolerant_vertex_tails" => &["vertex"],
        "transform_hints" => &["body"],
        "unknowns" => &["links"],
        "vertex_ownerships" => &["owning_edge", "vertex"],
        "wire_topologies" => &["edges", "free_vertex", "shell"],
        _ => &[],
    };
    for field in direct_fields {
        if let Some(value) = fields.get_mut(*field) {
            scope_identity_value(value, occurrence);
        }
    }

    // These records carry history-qualified selection proofs as nested
    // structs. Their `history_id` is a native record identity; the adjacent
    // historical slots are numeric source facts and remain unchanged.
    if matches!(
        arena,
        "design_entity_selection_operands"
            | "design_edge_identity_operands"
            | "design_extrude_selection_members"
    ) {
        scope_named_fields(ctx, fields, &["history_id"], occurrence)?;
    }

    // WorkPoint, mesh and edge-operand records contain native identities as ordinary strings
    // inside their nested envelopes. Their surrounding payloads also carry
    // source text, so the walker is restricted to the field names owned by the
    // corresponding native relations.
    match arena {
        "design_edge_operands" => {
            scope_named_fields(
                ctx,
                fields,
                &[
                    "result_face_boundary_ids",
                    "preceding_face_boundary_ids",
                    "preceding_support_face_boundary_ids",
                ],
                occurrence,
            )?;
        }
        "design_edge_treatment_vertex_operands" => {
            scope_named_fields(ctx, fields, &["recipe_id"], occurrence)?;
        }
        "design_mesh_features" => {
            scope_named_fields(ctx, fields, &["tessellation_id"], occurrence)?;
        }
        "xref_references" => {
            scope_named_fields(ctx, fields, &["stream"], occurrence)?;
        }
        "design_parameter_scopes" => {
            scope_named_fields(
                ctx,
                fields,
                &["history_id", "operand_id", "point_native_id", "recipe_id"],
                occurrence,
            )?;
        }
        _ => {}
    }
    Ok(())
}

fn scope_identity_value(value: &mut Value, occurrence: &str) {
    match value {
        Value::String(text) => {
            if let Some(rescoped) = rescope(text, occurrence) {
                *text = rescoped;
            }
        }
        Value::Array(items) => {
            for item in items {
                if let Value::String(text) = item {
                    if let Some(rescoped) = rescope(text, occurrence) {
                        *text = rescoped;
                    }
                }
            }
        }
        // `AttributeTarget` is the only selected object field. Its `id` is a
        // typed identity and all other members are its discriminator/value.
        Value::Object(fields) => {
            if let Some(Value::String(text)) = fields.get_mut("id") {
                if let Some(rescoped) = rescope(text, occurrence) {
                    *text = rescoped;
                }
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}

fn scope_named_fields(
    ctx: &DecodeContext<'_>,
    fields: &mut Map<String, Value>,
    names: &[&str],
    occurrence: &str,
) -> Result<(), CodecError> {
    let _depth = ctx.enter_nested("walk F3Z native identity fields")?;
    for (name, value) in fields {
        if names.contains(&name.as_str()) {
            scope_identity_value(value, occurrence);
        } else {
            scope_named_values(ctx, value, names, occurrence)?;
        }
    }
    Ok(())
}

fn scope_named_values(
    ctx: &DecodeContext<'_>,
    value: &mut Value,
    names: &[&str],
    occurrence: &str,
) -> Result<(), CodecError> {
    let _depth = ctx.enter_nested("walk F3Z native identity values")?;
    match value {
        Value::Object(fields) => scope_named_fields(ctx, fields, names, occurrence)?,
        Value::Array(items) => {
            for item in items {
                scope_named_values(ctx, item, names, occurrence)?;
            }
        }
        Value::String(_) | Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    mod contained;
    mod fidelity;
    mod occurrence;
    mod sketches;
    mod unplaced;

    use super::define_member;

    #[test]
    fn member_definition_keeps_a_finite_body_transform_without_preplacement() {
        let transform = cadmpeg_ir::transform::Transform::affine([
            [1.0, 0.0, 0.0, f64::MAX],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
        ])
        .unwrap();
        let mut model = cadmpeg_ir::examples::unit_cube().unwrap().model;
        model.product_definitions.clear();
        model.bodies[0].transform = Some(transform);
        let ctx = cadmpeg_test_support::service_decode_context();
        define_member(&ctx, &mut model, "member").unwrap();
        assert_eq!(model.bodies[0].transform, Some(transform));
        assert_eq!(
            model.product_definitions[0].bodies,
            [model.bodies[0].id.clone()]
        );
    }

    #[test]
    fn member_definition_obeys_the_entity_limit() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_entities = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut model = cadmpeg_ir::document::Model::default();
        let error = define_member(&ctx, &mut model, "member").unwrap_err();
        assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(_)));
        assert!(model.product_definitions.is_empty());
    }
}
