// SPDX-License-Identifier: Apache-2.0
//! Resolve extrude profile selections against sketch regions.

use cadmpeg_core::container::ContainerRole;

use crate::container::ContainerScan;
use crate::design::decode::operands::{entity_selection_matches_curve, parse_sketch_profile};
use crate::design::geometry::{
    arrangement_region_containing_points, historical_member_points_in_state, point_in_polygon,
    point_on_sketch_entity, point_segment_distance, profile_loops_are_independent,
    project_to_sketch, region_containing_points, ProfileGeometryWork,
};
use crate::ids::native_stream;
use crate::records::{
    decal::DesignRecordHeader,
    entity_header::DesignEntityHeader,
    feature::scope::DesignParameterScope,
    sketch_geometry::SketchCurveIdentity,
    sketch_placement::DesignSketchPlacement,
    sketch_relations::SketchRelationOperand,
    topology::{
        construction::DesignConstructionOperandGroup,
        entity_selection::DesignEntitySelectionOperand,
        extrude_selection::DesignExtrudeSelectionGroup,
        extrude_selection::DesignExtrudeSelectionMember, extrude_selection::DesignOperandRole,
        sketch_profile::DesignSketchProfileOperand,
        sketch_profile::DesignSketchProfileRegionMember,
    },
};
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use std::collections::{HashMap, HashSet};

mod region_members;

/// Bind each Extrude's counted sketch selection to exact neutral profile loops
/// when every member identifies one unambiguous loop. Otherwise retain the
/// native selection together with the known sketch.
#[derive(Clone, Copy)]
pub(crate) struct ExtrudeProfileResolution<'a> {
    pub(crate) entities: &'a [cadmpeg_ir::sketches::SketchEntity],
    pub(crate) spatial_sketches: &'a [cadmpeg_ir::sketches::SpatialSketch],
    pub(crate) spatial_entities: &'a [cadmpeg_ir::sketches::SpatialSketchEntity],
    pub(crate) histories: &'a [crate::history_records::AsmHistory],
    pub(crate) scope_histories: &'a HashMap<String, String>,
    pub(crate) linear_tolerance: f64,
    pub(crate) angular_tolerance: f64,
    pub(crate) profile_work: &'a ProfileGeometryWork,
    pub(crate) ctx: &'a DecodeContext<'a>,
}

#[derive(Clone, Copy)]
pub(in crate::design) struct ScopedExtrudeProfileResolution<'a> {
    entities: &'a [cadmpeg_ir::sketches::SketchEntity],
    spatial_entities: &'a [cadmpeg_ir::sketches::SpatialSketchEntity],
    histories: &'a [crate::history_records::AsmHistory],
    linear_tolerance: f64,
    angular_tolerance: f64,
    profile_work: &'a ProfileGeometryWork,
    ctx: &'a DecodeContext<'a>,
}

impl<'a> ExtrudeProfileResolution<'a> {
    pub(super) fn scoped(
        self,
        histories: &'a [crate::history_records::AsmHistory],
    ) -> ScopedExtrudeProfileResolution<'a> {
        ScopedExtrudeProfileResolution {
            entities: self.entities,
            spatial_entities: self.spatial_entities,
            histories,
            linear_tolerance: self.linear_tolerance,
            angular_tolerance: self.angular_tolerance,
            profile_work: self.profile_work,
            ctx: self.ctx,
        }
    }
}

fn histories_for_scope<'a>(
    scope_id: &str,
    scope_histories: &HashMap<String, String>,
    histories: &'a [crate::history_records::AsmHistory],
) -> &'a [crate::history_records::AsmHistory] {
    if scope_histories.contains_key(scope_id) {
        crate::history::bound_scope_history(scope_id, scope_histories, histories)
            .map_or(&[], std::slice::from_ref)
    } else {
        histories
    }
}

/// Native and neutral arenas required to resolve curve selections in sketches.
pub(crate) struct SketchCurveSelectionResolution<'a> {
    /// Decoded Design feature scopes.
    pub(crate) scopes: &'a [DesignParameterScope],
    /// Counted construction-operand groups.
    pub(crate) groups: &'a [DesignConstructionOperandGroup],
    /// Nested entity-selection operands.
    pub(crate) operands: &'a [DesignEntitySelectionOperand],
    /// Typed-reference profile selections listing Sketch curve loops.
    pub(crate) profile_curves:
        &'a [crate::records::topology::entity_selection::DesignProfileCurveSelection],
    /// Projected Sketch placement carriers.
    pub(crate) placements: &'a [DesignSketchPlacement],
    /// Persistent Sketch curve identities.
    pub(crate) curve_identities: &'a [SketchCurveIdentity],
    /// Neutral planar Sketches.
    pub(crate) sketches: &'a [cadmpeg_ir::sketches::Sketch],
    /// Neutral planar Sketch entities.
    pub(crate) sketch_entities: &'a [cadmpeg_ir::sketches::SketchEntity],
    /// Neutral model-space Sketches for non-planar sketch carriers.
    pub(crate) spatial_sketches: &'a [cadmpeg_ir::sketches::SpatialSketch],
    /// Neutral model-space entities for non-planar sketch carriers.
    pub(crate) spatial_sketch_entities: &'a [cadmpeg_ir::sketches::SpatialSketchEntity],
}

#[derive(Clone, Copy)]
struct EntitySelectionPathResolution<'a> {
    operands: &'a [DesignEntitySelectionOperand],
    profile_curves: &'a [crate::records::topology::entity_selection::DesignProfileCurveSelection],
    placements: &'a [DesignSketchPlacement],
    curve_identities: &'a [SketchCurveIdentity],
    sketches: &'a [cadmpeg_ir::sketches::Sketch],
    sketch_entities: &'a [cadmpeg_ir::sketches::SketchEntity],
    spatial_sketches: &'a [cadmpeg_ir::sketches::SpatialSketch],
    spatial_sketch_entities: &'a [cadmpeg_ir::sketches::SpatialSketchEntity],
}

impl<'a> SketchCurveSelectionResolution<'a> {
    fn path_resolution(&self) -> EntitySelectionPathResolution<'a> {
        EntitySelectionPathResolution {
            operands: self.operands,
            profile_curves: self.profile_curves,
            placements: self.placements,
            curve_identities: self.curve_identities,
            sketches: self.sketches,
            sketch_entities: self.sketch_entities,
            spatial_sketches: self.spatial_sketches,
            spatial_sketch_entities: self.spatial_sketch_entities,
        }
    }
}

/// Bind exact Sweep sketch-profile and sole-curve path carriers.
pub(crate) fn bind_sweep_sketch_selections(
    features: &mut [cadmpeg_ir::features::Feature],
    resolution: &SketchCurveSelectionResolution<'_>,
) -> Result<(), CodecError> {
    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation, PathRef, PlanarProfileRef};
    let SketchCurveSelectionResolution {
        scopes,
        groups,
        operands,
        placements,
        curve_identities,
        sketches,
        sketch_entities,
        ..
    } = resolution;
    let path_resolution = resolution.path_resolution();
    for feature in features {
        let mut edit_result = Ok(());
        feature.evaluation.edit(|definition, _| {
            edit_result = (|| -> Result<(), CodecError> {
                'feature_edit: {
                    let Some(native_ref) = feature.native_ref.as_deref() else {
                        break 'feature_edit;
                    };
                    let mut matching_scopes = scopes.iter().filter(|scope| scope.id == native_ref);
                    let Some(scope) = matching_scopes.next() else {
                        break 'feature_edit;
                    };
                    if matching_scopes.next().is_some() {
                        break 'feature_edit;
                    }
                    let Some(stream) = native_stream(&scope.id) else {
                        break 'feature_edit;
                    };
                    let FeatureDefinition::Operation(FeatureOperation::Sweep {
                        shape,
                        path,
                        guide_rail,
                        ..
                    }) = definition
                    else {
                        break 'feature_edit;
                    };
                    {
                        let section = &mut *shape;
                        if let (Some(PlanarProfileRef::Native(group_id)), Some(profile_operand)) =
                            (section.referenced_profile(), scope.sweep_profile())
                        {
                            let group_id = group_id.as_str();
                            let group_matches = {
                                let mut matching_groups = groups.iter().filter(|group| {
                                    group.id == group_id
                                        && group.scope_record_index == scope.record_index
                                        && group.role() == DesignOperandRole::PROFILE
                                        && group
                                            .members()
                                            .iter()
                                            .map(|member| member.value)
                                            .eq([profile_operand.record_index])
                                        && native_stream(&group.id) == Some(stream)
                                });
                                matches!(
                                    (matching_groups.next(), matching_groups.next()),
                                    (Some(_), None)
                                )
                            };
                            if group_matches {
                                let mut candidates = placements.iter().filter(|placement| {
                                    native_stream(&placement.id) == Some(stream)
                                        && placement.entity_id.suffix()
                                            == profile_operand.entity_id.suffix()
                                });
                                if let (Some(placement), None) =
                                    (candidates.next(), candidates.next())
                                {
                                    let sketch = crate::ids::neutral_sketch_id(placement);
                                    if sketches.iter().any(|candidate| candidate.id == sketch) {
                                        section.set_referenced_profile((sketch).into());
                                    }
                                }
                            }
                        }
                        if let Some(PlanarProfileRef::Native(group_id)) =
                            section.referenced_profile()
                        {
                            let group_id = group_id.as_str();
                            let resolved = (|| -> Option<_> {
                                let mut matching_groups = groups.iter().filter(|group| {
                                    group.id == group_id
                                        && group.scope_record_index == scope.record_index
                                        && group.role() == DesignOperandRole::PROFILE
                                        && group.members().len() == 1
                                        && native_stream(&group.id) == Some(stream)
                                });
                                let group = matching_groups.next()?;
                                if matching_groups.next().is_some() {
                                    return None;
                                }
                                let mut matching_operands = operands.iter().filter(|operand| {
                                    operand.scope_record_index == scope.record_index
                                        && operand.group_record_index == group.record_index
                                        && operand.group_member_ordinal == 0
                                        && operand.record_index() == group.members()[0].value
                                        && native_stream(&operand.id) == Some(stream)
                                });
                                let operand = matching_operands.next()?;
                                if matching_operands.next().is_some() {
                                    return None;
                                }
                                let mut matching_placements =
                                    placements.iter().filter(|placement| {
                                        native_stream(&placement.id) == Some(stream)
                                            && placement.entity_id.suffix()
                                                == operand.primary_identity
                                    });
                                let placement = matching_placements.next()?;
                                if matching_placements.next().is_some() {
                                    return None;
                                }
                                let sketch = crate::ids::neutral_sketch_id(placement);
                                if !sketches.iter().any(|candidate| candidate.id == sketch) {
                                    return None;
                                }
                                let owner_reference =
                                    u32::try_from(operand.primary_identity).ok()?;
                                let mut matching_curves = curve_identities.iter().filter(|curve| {
                                    native_stream(&curve.id) == Some(stream)
                                        && curve.owner_reference == Some(owner_reference)
                                        && entity_selection_matches_curve(operand, curve)
                                });
                                let curve = matching_curves.next()?;
                                if matching_curves.next().is_some() {
                                    return None;
                                }
                                let selected = crate::ids::neutral_sketch_curve_id(
                                    &sketch,
                                    curve.primary_id.get(),
                                    curve.secondary_id,
                                );
                                sketch_entities
                                    .iter()
                                    .any(|entity| {
                                        entity.sketch == sketch && entity.id() == &selected
                                    })
                                    .then_some((sketch, selected))
                            })();
                            if let Some((sketch, selected)) = resolved {
                                let profile =
                                    PlanarProfileRef::sketch_entities(sketch, vec![selected])
                                        .map_err(CodecError::malformed)?;
                                section.set_referenced_profile(profile);
                            }
                        }
                        let resolve_path = |path: &PathRef| -> Option<PathRef> {
                            let PathRef::Native(group_id) = path else {
                                return None;
                            };
                            let mut matching_groups = groups.iter().filter(|group| {
                                group.id == *group_id
                                    && group.scope_record_index == scope.record_index
                                    && group.role() == DesignOperandRole::ROLE_0X5
                                    && native_stream(&group.id) == Some(stream)
                            });
                            let group = matching_groups.next()?;
                            if matching_groups.next().is_some() || group.members().len() != 1 {
                                return None;
                            }
                            resolve_entity_selection_path(group, &path_resolution)
                        };
                        if let Some(path) = path {
                            if let Some(resolved) = resolve_path(path) {
                                *path = resolved;
                            }
                        }
                        if let Some(guide_rail) = guide_rail {
                            if let Some(resolved) = resolve_path(&guide_rail.path) {
                                guide_rail.path = resolved;
                            }
                        }
                    }
                }
                Ok(())
            })();
        });
        edit_result?;
    }
    Ok(())
}

/// Resolve `SplitFace` curve-tool groups to ordered curves in one sketch.
pub(crate) fn bind_split_face_sketch_selections(
    features: &mut [cadmpeg_ir::features::Feature],
    resolution: &SketchCurveSelectionResolution<'_>,
) {
    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation, PathRef, SplitFaceTool};

    let path_resolution = resolution.path_resolution();
    for feature in features {
        feature.evaluation.edit(|definition, _| 'feature_edit: {
            let FeatureDefinition::Operation(FeatureOperation::SplitFace { tool, .. }) = definition
            else {
                break 'feature_edit;
            };
            let SplitFaceTool::Path(PathRef::Native(group_id)) = tool else {
                break 'feature_edit;
            };
            let mut matching_groups = resolution.groups.iter().filter(|group| {
                group.id == *group_id
                    && group.role() == DesignOperandRole::ROLE_0X21
                    && !group.members().is_empty()
            });
            let Some(group) = matching_groups.next() else {
                break 'feature_edit;
            };
            if matching_groups.next().is_some() {
                break 'feature_edit;
            }
            if let Some(path) = resolve_entity_selection_path(group, &path_resolution) {
                *tool = SplitFaceTool::Path(path);
            }
        });
    }
}

/// Resolve `SurfaceTrim` curve-tool groups to ordered curves in one sketch.
pub(crate) fn bind_surface_trim_sketch_selections(
    features: &mut [cadmpeg_ir::features::Feature],
    resolution: &SketchCurveSelectionResolution<'_>,
) {
    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation, PathRef};

    let path_resolution = resolution.path_resolution();
    for feature in features {
        feature.evaluation.edit(|definition, _| 'feature_edit: {
            let FeatureDefinition::Operation(FeatureOperation::TrimSurface { tool, .. }) =
                definition
            else {
                break 'feature_edit;
            };
            let PathRef::Native(group_id) = tool else {
                break 'feature_edit;
            };
            let mut matching_groups = resolution.groups.iter().filter(|group| {
                group.id == *group_id
                    && group.role() == DesignOperandRole::ROLE_0X21
                    && !group.members().is_empty()
            });
            let Some(group) = matching_groups.next() else {
                break 'feature_edit;
            };
            if matching_groups.next().is_some() {
                break 'feature_edit;
            }
            if let Some(path) = resolve_entity_selection_path(group, &path_resolution) {
                *tool = path;
            }
        });
    }
}

pub(crate) fn has_to_termination(extent: &cadmpeg_ir::features::ExtrudeExtent) -> bool {
    let check_side = |side: &cadmpeg_ir::features::ExtrudeSide| {
        matches!(
            side.termination,
            cadmpeg_ir::features::LinearTermination::ToFace { .. }
                | cadmpeg_ir::features::LinearTermination::ToPlane { .. }
                | cadmpeg_ir::features::LinearTermination::ToShape { .. }
                | cadmpeg_ir::features::LinearTermination::ThroughNext { .. }
        )
    };
    match extent {
        cadmpeg_ir::features::ExtrudeExtent::OneSided { side } => check_side(side),
        cadmpeg_ir::features::ExtrudeExtent::TwoSided { first, second } => {
            check_side(first) || check_side(second)
        }
        cadmpeg_ir::features::ExtrudeExtent::Symmetric { side } => check_side(side),
    }
}

pub(crate) fn bind_extrude_profile_selections(
    features: &mut [cadmpeg_ir::features::Feature],
    scopes: &[DesignParameterScope],
    groups: &[DesignExtrudeSelectionGroup],
    members: &[DesignExtrudeSelectionMember],
    sketches: &[cadmpeg_ir::sketches::Sketch],
    curve_resolution: &SketchCurveSelectionResolution<'_>,
    resolution: ExtrudeProfileResolution<'_>,
) -> Result<(), CodecError> {
    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation, PlanarProfileRef, ProfileRef};

    for feature in features {
        let mut edit_result = Ok(());
        let mut to_distance_info = None;
        let mut historical_face_sketch_info: Option<(String, String)> = None;
        let mut historical_face_offset_info: Option<f64> = None;
        feature.evaluation.edit(|definition, _| {
            edit_result = (|| -> Result<(), CodecError> {
                'feature_edit: {
                    let Some(scope) = feature.native_ref.as_deref() else {
                        break 'feature_edit;
                    };
                    let Some(scope) = scopes.iter().find(|candidate| candidate.id == scope) else {
                        break 'feature_edit;
                    };
                    let scoped_histories = histories_for_scope(
                        &scope.id,
                        resolution.scope_histories,
                        resolution.histories,
                    );
                    let scoped_resolution = resolution.scoped(scoped_histories);
                    let effective_previous_history_state_id =
                        crate::history::effective_scope_previous_history_state_id(
                            scope,
                            scoped_histories,
                        );
                    let mut matching_groups = Vec::new();
                    for group in groups.iter().filter(|group| {
                        native_stream(&group.id) == native_stream(&scope.id)
                            && group.scope_record_index == scope.record_index
                    }) {
                        matching_groups.push(group);
                    }
                    matching_groups.sort_by(|left, right| {
                        left.scope_reference_ordinal
                            .cmp(&right.scope_reference_ordinal)
                    });
                    let FeatureDefinition::Operation(FeatureOperation::Extrude {
                        profile,
                        extent,
                        ..
                    }) = definition
                    else {
                        break 'feature_edit;
                    };
                    let is_to_termination = has_to_termination(extent);
                    if let ProfileRef::Planar(PlanarProfileRef::Native(native)) = profile {
                        let mut entity_groups = curve_resolution.groups.iter().filter(|group| {
                            group.id == *native
                                && native_stream(&group.id) == native_stream(&scope.id)
                                && group.scope_record_index == scope.record_index
                        });
                        if let (Some(group), None) = (entity_groups.next(), entity_groups.next()) {
                            if let Some(selection) = resolve_entity_selection_profile(
                                group,
                                &curve_resolution.path_resolution(),
                            ) {
                                *profile = selection;
                                if is_to_termination {
                                    if let (
                                        Some(state_id),
                                        Some(prev_state_id),
                                        ProfileRef::Planar(
                                            PlanarProfileRef::Sketch(sk_id)
                                            | PlanarProfileRef::SketchProfiles {
                                                sketch: sk_id, ..
                                            }
                                            | PlanarProfileRef::SketchRegions {
                                                sketch: sk_id, ..
                                            }
                                            | PlanarProfileRef::SketchSelection {
                                                sketch: sk_id, ..
                                            },
                                        ),
                                    ) = (
                                        scope.history_state_id(),
                                        effective_previous_history_state_id,
                                        &*profile,
                                    ) {
                                        if let Some(sk) = sketches.iter().find(|s| &s.id == sk_id) {
                                            to_distance_info = historical_extrude_to_distance(
                                                sk,
                                                0.0,
                                                scoped_histories,
                                                state_id,
                                                prev_state_id,
                                            );
                                        }
                                    }
                                }
                                break 'feature_edit;
                            }
                            if let Some(selection) = resolve_profile_curve_arrangement_regions(
                                group,
                                &curve_resolution.path_resolution(),
                                scoped_resolution,
                            )? {
                                *profile = selection;
                                break 'feature_edit;
                            }
                        }
                        if let Some(selection) = historical_face_profile_selection(
                            &matching_groups,
                            members,
                            effective_previous_history_state_id,
                            &feature.id,
                            scoped_histories,
                        )? {
                            *profile = ProfileRef::Planar(selection);
                        }
                    }
                    if let ProfileRef::Planar(PlanarProfileRef::HistoricalFaces { faces, .. }) =
                        profile
                    {
                        if let (Some(state_id), Some(prev_state_id)) = (
                            scope.history_state_id(),
                            effective_previous_history_state_id,
                        ) {
                            if let Some((sk_id, prof_indices, face_off, dist)) =
                                resolve_historical_faces_to_sketch(
                                    faces.as_slice(),
                                    sketches,
                                    scoped_resolution,
                                    scoped_histories,
                                    state_id,
                                    prev_state_id,
                                )
                            {
                                historical_face_sketch_info = Some((sk_id, prof_indices));
                                historical_face_offset_info = face_off;
                                if dist.is_some() {
                                    to_distance_info = dist;
                                }
                            }
                        }
                        break 'feature_edit;
                    }
                    let ProfileRef::Planar(PlanarProfileRef::Sketch(sketch_id)) = profile else {
                        break 'feature_edit;
                    };
                    let Some(sketch) = sketches.iter().find(|sketch| sketch.id == *sketch_id)
                    else {
                        if matching_groups.is_empty() {
                            break 'feature_edit;
                        }
                        let spatial_id_parts = sketch_id.as_str().split_once("f3d:model:sketch#");
                        if let Some(spatial_sketch) =
                            resolution.spatial_sketches.iter().find(|candidate| {
                                match spatial_id_parts {
                                    Some((before, after)) => {
                                        candidate.id.as_str().strip_prefix(before).and_then(
                                            |rest| rest.strip_prefix("f3d:model:spatial-sketch#"),
                                        ) == Some(after)
                                    }
                                    None => candidate.id.as_str() == sketch_id.as_str(),
                                }
                            })
                        {
                            let mut selections = Vec::new();
                            for group in &matching_groups {
                                let selection = resolved_spatial_extrude_profile_selection(
                                    group,
                                    members,
                                    spatial_sketch,
                                    scoped_resolution.spatial_entities,
                                    scoped_resolution,
                                    scope.history_state_id(),
                                    effective_previous_history_state_id,
                                )?;
                                selections.push(selection);
                            }
                            let mut indices = Vec::new();
                            let mut all_resolved = true;
                            for selection in &selections {
                                let Some(index) = selection else {
                                    all_resolved = false;
                                    break;
                                };
                                if !indices.contains(index) {
                                    indices.push(*index);
                                }
                            }
                            if all_resolved {
                                let sketch_id = spatial_sketch.id.clone();
                                *profile =
                                    match ProfileRef::spatial_sketch_profiles(sketch_id, indices) {
                                        Ok(profile) => profile,
                                        Err(_) => ProfileRef::Planar(PlanarProfileRef::Native(
                                            scope.id.clone(),
                                        )),
                                    };
                            } else {
                                let group_ids: Vec<_> = matching_groups
                                    .iter()
                                    .map(|group| group.id.clone())
                                    .collect();
                                let sketch_id = spatial_sketch.id.clone();
                                *profile = match ProfileRef::spatial_sketch_selection(
                                    sketch_id, group_ids,
                                ) {
                                    Ok(profile) => profile,
                                    Err(_) => ProfileRef::Planar(PlanarProfileRef::Native(
                                        scope.id.clone(),
                                    )),
                                };
                            }
                            break 'feature_edit;
                        }
                        let id = match matching_groups.as_slice() {
                            [group] => &group.id,
                            _ => &scope.id,
                        };
                        *profile = ProfileRef::Planar(PlanarProfileRef::Native(id.to_owned()));
                        break 'feature_edit;
                    };
                    if let (Some(state_id), Some(prev_state_id)) = (
                        scope.history_state_id(),
                        effective_previous_history_state_id,
                    ) {
                        let is_to_termination = has_to_termination(extent);
                        if is_to_termination {
                            to_distance_info = historical_extrude_to_distance(
                                sketch,
                                0.0,
                                scoped_histories,
                                state_id,
                                prev_state_id,
                            );
                        }
                    }
                    let explicit_profiles = scope
                        .extrude_profile()
                        .zip(native_stream(&scope.id))
                        .and_then(|(profile_operand, stream)| {
                            resolved_sketch_profile_regions(
                                stream,
                                profile_operand,
                                sketch,
                                curve_resolution.curve_identities,
                                curve_resolution.sketch_entities,
                            )
                        });
                    let mut region_selection_unmatched = false;
                    if explicit_profiles.is_none() {
                        if let Some((operand, stream)) = scope
                            .extrude_profile()
                            .zip(native_stream(&scope.id))
                            .filter(|(operand, _)| operand.region_selection.is_some())
                        {
                            if let Some(selected) = region_members::resolve(
                                stream,
                                operand,
                                sketch,
                                curve_resolution.curve_identities,
                                curve_resolution.sketch_entities,
                                scoped_resolution,
                            )? {
                                *profile = ProfileRef::Planar(selected);
                                break 'feature_edit;
                            }
                            region_selection_unmatched = true;
                        }
                    }
                    if !matching_groups.is_empty() {
                        let mut selections = Vec::new();
                        for group in &matching_groups {
                            let selection = resolved_extrude_profile_selection(
                                sketch_id,
                                group,
                                members,
                                sketch,
                                scoped_resolution,
                                scope.history_state_id(),
                                effective_previous_history_state_id,
                            )?;
                            selections.push(selection);
                        }
                        if let Some(merged) =
                            merge_resolved_profile_selections(sketch_id, &selections)
                        {
                            let use_merged = match (&explicit_profiles, &merged) {
                                (None, _) => true,
                                (
                                    Some(exp),
                                    ProfileRef::Planar(PlanarProfileRef::SketchRegions {
                                        regions,
                                        ..
                                    }),
                                ) => regions.as_slice().iter().all(|reg| match reg {
                                    cadmpeg_ir::features::SketchProfileRegion::Loops { loops } => {
                                        exp.contains(&loops.outer())
                                    }
                                    cadmpeg_ir::features::SketchProfileRegion::Trimmed {
                                        ..
                                    } => false,
                                }),
                                _ => false,
                            };
                            if use_merged {
                                *profile = merged;
                                break 'feature_edit;
                            }
                        }
                    }
                    if let Some(profiles) = explicit_profiles {
                        let sketch_id = sketch_id.clone();
                        *profile = ProfileRef::Planar(
                            match PlanarProfileRef::sketch_profiles(sketch_id, profiles) {
                                Ok(profile) => profile,
                                Err(_) => PlanarProfileRef::Native(scope.id.clone()),
                            },
                        );
                        break 'feature_edit;
                    }
                    if matching_groups.is_empty() {
                        if region_selection_unmatched {
                            if let (Some(state_id), Some(prev_state_id)) = (
                                scope.history_state_id(),
                                effective_previous_history_state_id,
                            ) {
                                if let Some(sel) = transition_profile_selection(
                                    sketch,
                                    scoped_resolution,
                                    state_id,
                                    prev_state_id,
                                )? {
                                    *profile = ProfileRef::Planar(match sel {
                                        ResolvedProfileSelection::Loops(profiles) => {
                                            PlanarProfileRef::sketch_profiles(
                                                sketch_id.clone(),
                                                profiles,
                                            )
                                            .unwrap_or_else(|_| {
                                                PlanarProfileRef::Native(scope.id.clone())
                                            })
                                        }
                                        ResolvedProfileSelection::Regions(regions) => {
                                            PlanarProfileRef::sketch_regions(
                                                sketch_id.clone(),
                                                regions,
                                            )
                                            .unwrap_or_else(|_| {
                                                PlanarProfileRef::Native(scope.id.clone())
                                            })
                                        }
                                    });
                                    break 'feature_edit;
                                }
                            }
                            *profile = ProfileRef::Planar(
                                PlanarProfileRef::sketch_selection(
                                    sketch_id.clone(),
                                    vec![scope.id.clone()],
                                )
                                .unwrap_or_else(|_| PlanarProfileRef::Native(scope.id.clone())),
                            );
                        }
                        break 'feature_edit;
                    }
                    let group_ids: Vec<_> = matching_groups
                        .iter()
                        .map(|group| group.id.clone())
                        .collect();
                    let sketch_id = sketch_id.clone();
                    let fallback = match PlanarProfileRef::sketch_selection(sketch_id, group_ids) {
                        Ok(profile) => profile,
                        Err(_) => PlanarProfileRef::Native(scope.id.clone()),
                    };
                    *profile = ProfileRef::Planar(fallback);
                }
                Ok(())
            })();
        });
        edit_result?;
        if let Some((sk_id, prof_indices)) = historical_face_sketch_info {
            feature
                .source_properties
                .entry(cadmpeg_core::nonblank_literal!("historical_face_sketch"))
                .or_insert(sk_id);
            feature
                .source_properties
                .entry(cadmpeg_core::nonblank_literal!("historical_face_profiles"))
                .or_insert(prof_indices);
        }
        if let Some(face_off) = historical_face_offset_info {
            if face_off.abs() > 1.0e-4 {
                feature
                    .source_properties
                    .entry(cadmpeg_core::nonblank_literal!("historical_face_offset"))
                    .or_insert_with(|| face_off.to_string());
            }
        }
        if let Some(dist) = to_distance_info {
            feature
                .source_properties
                .entry(cadmpeg_core::nonblank_literal!("extrude_to_distance"))
                .or_insert_with(|| dist.to_string());
        }
        resolution.profile_work.finish_selection();
    }
    Ok(())
}

fn resolve_entity_selection_profile(
    group: &DesignConstructionOperandGroup,
    resolution: &EntitySelectionPathResolution<'_>,
) -> Option<cadmpeg_ir::features::ProfileRef> {
    use cadmpeg_ir::features::{PathRef, PlanarProfileRef, ProfileRef};

    if group.role() != DesignOperandRole::PROFILE {
        return None;
    }
    if resolution.profile_curves.iter().any(|selection| {
        native_stream(&selection.id) == native_stream(&group.id)
            && selection.group_record_index == group.record_index
            && selection.scope_record_index == group.scope_record_index
    }) {
        return resolve_profile_curve_regions(group, resolution);
    }
    let path = resolve_entity_selection_path(group, resolution)?;
    match path {
        PathRef::SketchCurves { sketch, curves } => {
            let source = resolution
                .sketches
                .iter()
                .find(|source| source.id == sketch)?;
            let mut selected_profiles = Vec::new();
            let mut has_unprofiled_entity = false;
            for curve in &curves {
                let mut matches = source
                    .profiles
                    .iter()
                    .enumerate()
                    .filter(|(_, profile)| profile.iter().any(|use_| use_.entity == *curve));
                let Some((profile_index, _)) = matches.next() else {
                    has_unprofiled_entity = true;
                    continue;
                };
                if matches.next().is_some() {
                    return None;
                }
                let Ok(profile_index) = u32::try_from(profile_index) else {
                    return None;
                };
                if !selected_profiles.contains(&profile_index) {
                    selected_profiles.push(profile_index);
                }
            }
            if has_unprofiled_entity {
                Some(ProfileRef::Planar(PlanarProfileRef::SketchEntities {
                    sketch,
                    entities: curves,
                }))
            } else {
                PlanarProfileRef::sketch_profiles(sketch, selected_profiles)
                    .ok()
                    .map(ProfileRef::Planar)
            }
        }
        PathRef::SpatialSketchCurves { sketch, curves } => {
            let source = resolution
                .spatial_sketches
                .iter()
                .find(|source| source.id == sketch)?;
            let mut profiles = Vec::new();
            for curve in &curves {
                let mut matches = source.profiles.iter().enumerate().filter(|(_, profile)| {
                    profile.boundary().iter().any(|use_| use_.entity == *curve)
                });
                let (index, _) = matches.next()?;
                if matches.next().is_some() {
                    return None;
                }
                let Ok(index) = u32::try_from(index) else {
                    return None;
                };
                if !profiles.contains(&index) {
                    profiles.push(index);
                }
            }
            if profiles.is_empty() {
                return None;
            }
            ProfileRef::spatial_sketch_profiles(sketch, profiles).ok()
        }
        _ => None,
    }
}

fn historical_face_profile_selection(
    groups: &[&DesignExtrudeSelectionGroup],
    members: &[DesignExtrudeSelectionMember],
    previous_state_id: Option<i64>,
    feature_id: &cadmpeg_ir::features::FeatureId,
    scoped_histories: &[crate::history_records::AsmHistory],
) -> Result<Option<cadmpeg_ir::features::PlanarProfileRef>, CodecError> {
    use cadmpeg_ir::features::PlanarProfileRef;

    let Some(previous_state_id) = previous_state_id else {
        return Ok(None);
    };
    let mut states = scoped_histories
        .iter()
        .flat_map(|history| history.states().iter().map(move |state| (history, state)))
        .filter(|(_, state)| state.state_id == previous_state_id);
    let Some((history, state)) = states.next() else {
        return Ok(None);
    };
    let Some(topology) = history.topology(state) else {
        return Ok(None);
    };
    if states.next().is_some() {
        return Ok(None);
    }
    let Some(stream) = groups.first().and_then(|group| native_stream(&group.id)) else {
        return Ok(None);
    };
    let mut selected_faces = Vec::new();
    for group in groups {
        if native_stream(&group.id) != Some(stream) {
            return Ok(None);
        }
        let mut group_members = Vec::new();
        for member in members.iter().filter(|member| {
            native_stream(&member.id) == Some(stream)
                && member.group_record_index == group.record_index
        }) {
            group_members.push(member);
        }
        group_members.sort_by_key(|left| left.group_member_ordinal);
        if group_members.len() != group.members().len()
            || group_members
                .iter()
                .zip(group.members())
                .any(|(member, record_index)| member.record_index() != record_index.value)
        {
            return Ok(None);
        }
        let mut candidates = None::<HashSet<i64>>;
        for member in group_members {
            let (kind, entity_ref) = match &member.historical {
                Some(binding) => {
                    if !binding.state_ids.is_empty()
                        && !binding.state_ids.contains(&previous_state_id)
                    {
                        return Ok(None);
                    }
                    (Some(binding.kind), binding.entity_ref)
                }
                None => {
                    let Ok(entity_ref) = i64::try_from(member.local_id) else {
                        return Ok(None);
                    };
                    (None, entity_ref)
                }
            };
            let member_faces = historical_profile_face_candidates(kind, entity_ref, topology)?;
            if member_faces.is_empty() {
                return Ok(None);
            }
            candidates = Some(match candidates {
                None => member_faces,
                Some(mut candidates) => {
                    candidates.retain(|face| member_faces.contains(face));
                    candidates
                }
            });
        }
        let Some(candidates) = candidates else {
            return Ok(None);
        };
        let mut candidates = candidates.into_iter();
        let Some(face) = candidates.next() else {
            return Ok(None);
        };
        if candidates.next().is_some() {
            return Ok(None);
        }
        if !selected_faces.contains(&face) {
            selected_faces.push(face);
        }
    }
    if selected_faces.is_empty() {
        return Ok(None);
    }
    let mut face_ids = Vec::new();
    for face in selected_faces {
        let id = crate::ids::history_input_face_id(
            &crate::ids::feature_input_prefix(feature_id, previous_state_id),
            face,
        );
        face_ids.push(id);
    }
    let group_ids: Vec<_> = groups.iter().map(|group| group.id.clone()).collect();
    Ok(PlanarProfileRef::historical_faces(
        crate::ids::feature_input_topology_id(feature_id, previous_state_id),
        face_ids,
        group_ids,
    )
    .ok())
}

fn historical_profile_face_candidates(
    kind: Option<crate::records::topology::body_recipe::AsmHistoricalEntityKind>,
    entity_ref: i64,
    topology: &crate::history_records::AsmHistoricalTopology,
) -> Result<HashSet<i64>, CodecError> {
    use crate::records::topology::body_recipe::AsmHistoricalEntityKind;

    let all_kinds = [
        AsmHistoricalEntityKind::Face,
        AsmHistoricalEntityKind::Loop,
        AsmHistoricalEntityKind::Coedge,
        AsmHistoricalEntityKind::Edge,
        AsmHistoricalEntityKind::Pcurve,
        AsmHistoricalEntityKind::Curve,
        AsmHistoricalEntityKind::Vertex,
        AsmHistoricalEntityKind::Point,
        AsmHistoricalEntityKind::Surface,
    ];
    let kinds = kind
        .iter()
        .chain(all_kinds.iter().filter(|_| kind.is_none()));
    let loop_faces = |loop_ref| {
        let mut faces = HashSet::new();
        for relation in topology
            .face_loops
            .iter()
            .filter(|relation| relation.member_refs.contains(&loop_ref))
        {
            faces.insert(relation.owner_ref);
        }
        faces
    };
    let coedge_faces = |coedge_ref| {
        let mut faces = HashSet::new();
        for coedge in topology
            .coedge_topology
            .iter()
            .filter(|coedge| coedge.coedge == coedge_ref)
        {
            for face in loop_faces(coedge.owner_loop) {
                faces.insert(face);
            }
        }
        Ok::<_, CodecError>(faces)
    };
    let edge_faces = |edge_ref| {
        let mut faces = HashSet::new();
        for coedge in topology
            .coedge_topology
            .iter()
            .filter(|coedge| coedge.edge == edge_ref)
        {
            for face in loop_faces(coedge.owner_loop) {
                faces.insert(face);
            }
        }
        Ok::<_, CodecError>(faces)
    };
    let mut faces = HashSet::new();
    for kind in kinds {
        match kind {
            AsmHistoricalEntityKind::Face => {
                if topology.faces.contains(&entity_ref) {
                    faces.insert(entity_ref);
                }
            }
            AsmHistoricalEntityKind::Loop => {
                for face in loop_faces(entity_ref) {
                    faces.insert(face);
                }
            }
            AsmHistoricalEntityKind::Coedge => {
                for face in coedge_faces(entity_ref)? {
                    faces.insert(face);
                }
            }
            AsmHistoricalEntityKind::Edge => {
                for face in edge_faces(entity_ref)? {
                    faces.insert(face);
                }
            }
            AsmHistoricalEntityKind::Pcurve => {
                for binding in topology
                    .coedge_pcurves
                    .iter()
                    .filter(|binding| binding.carrier == Some(entity_ref))
                {
                    for face in coedge_faces(binding.entity)? {
                        faces.insert(face);
                    }
                }
            }
            AsmHistoricalEntityKind::Curve => {
                for binding in topology
                    .edge_curves
                    .iter()
                    .filter(|binding| binding.carrier == Some(entity_ref))
                {
                    for face in edge_faces(binding.entity)? {
                        faces.insert(face);
                    }
                }
            }
            AsmHistoricalEntityKind::Vertex => {
                for edge in topology
                    .edge_vertices
                    .iter()
                    .filter(|edge| edge.start_vertex == entity_ref || edge.end_vertex == entity_ref)
                {
                    for face in edge_faces(edge.edge)? {
                        faces.insert(face);
                    }
                }
            }
            AsmHistoricalEntityKind::Point => {
                let mut vertices = HashSet::new();
                for binding in topology
                    .vertex_points
                    .iter()
                    .filter(|binding| binding.carrier == entity_ref)
                {
                    vertices.insert(binding.entity);
                }
                for edge in topology.edge_vertices.iter().filter(|edge| {
                    vertices.contains(&edge.start_vertex) || vertices.contains(&edge.end_vertex)
                }) {
                    for face in edge_faces(edge.edge)? {
                        faces.insert(face);
                    }
                }
            }
            AsmHistoricalEntityKind::Surface => {
                for binding in topology
                    .face_surfaces
                    .iter()
                    .filter(|binding| binding.carrier == entity_ref)
                {
                    faces.insert(binding.entity);
                }
            }
            AsmHistoricalEntityKind::Body
            | AsmHistoricalEntityKind::Region
            | AsmHistoricalEntityKind::Shell => {}
        }
    }
    Ok(faces)
}

#[derive(Debug, Clone, PartialEq)]
pub(in crate::design) enum ResolvedProfileSelection {
    Loops(Vec<u32>),
    Regions(Vec<cadmpeg_ir::features::SketchProfileRegion>),
}

fn merge_resolved_profile_selections(
    sketch: &cadmpeg_ir::sketches::SketchId,
    selections: &[cadmpeg_ir::features::ProfileRef],
) -> Option<cadmpeg_ir::features::ProfileRef> {
    use cadmpeg_ir::features::{PlanarProfileRef, ProfileRef};

    let mut profiles = Vec::new();
    let mut regions = Vec::new();
    for selection in selections {
        match selection {
            ProfileRef::Planar(PlanarProfileRef::SketchProfiles {
                sketch: selected,
                profiles: selected_profiles,
            }) if selected == sketch && regions.is_empty() => {
                for profile in selected_profiles.as_slice().iter().copied() {
                    if !profiles.contains(&profile) {
                        profiles.push(profile);
                    }
                }
            }
            ProfileRef::Planar(PlanarProfileRef::SketchRegions {
                sketch: selected,
                regions: selected_regions,
            }) if selected == sketch && profiles.is_empty() => {
                for region in selected_regions.as_slice() {
                    if !regions.contains(region) {
                        regions.push(region.clone());
                    }
                }
            }
            _ => return None,
        }
    }
    let selected = if !profiles.is_empty() {
        PlanarProfileRef::sketch_profiles(sketch.clone(), profiles).ok()
    } else if !regions.is_empty() {
        PlanarProfileRef::sketch_regions(sketch.clone(), regions).ok()
    } else {
        return None;
    };
    selected.map(ProfileRef::Planar)
}

fn copy_bound_profile(
    profile: &cadmpeg_ir::features::ProfileRef,
) -> Result<cadmpeg_ir::features::ProfileRef, CodecError> {
    use cadmpeg_ir::features::ProfileRef;

    match profile {
        ProfileRef::Planar(planar) => Ok(ProfileRef::Planar(copy_bound_planar_profile(planar)?)),
        ProfileRef::SpatialSketchProfiles { .. } | ProfileRef::SpatialSketchSelection { .. } => {
            Ok(profile.clone())
        }
    }
}

fn copy_bound_planar_profile(
    profile: &cadmpeg_ir::features::PlanarProfileRef,
) -> Result<cadmpeg_ir::features::PlanarProfileRef, CodecError> {
    use cadmpeg_ir::features::PlanarProfileRef;

    match profile {
        PlanarProfileRef::Sketch(_)
        | PlanarProfileRef::SketchProfiles { .. }
        | PlanarProfileRef::SketchRegions { .. }
        | PlanarProfileRef::Native(_) => Ok(profile.clone()),
        _ => Err(CodecError::malformed(
            "bound planar profile has unsupported resolved form",
        )),
    }
}

fn copy_bound_path(
    path: &cadmpeg_ir::features::PathRef,
) -> Result<cadmpeg_ir::features::PathRef, CodecError> {
    use cadmpeg_ir::features::PathRef;

    match path {
        PathRef::SketchCurves { .. } | PathRef::SpatialSketchCurves { .. } => Ok(path.clone()),
        _ => Err(CodecError::malformed(
            "bound path has unsupported resolved form",
        )),
    }
}

pub(super) fn resolved_extrude_profile_selection(
    sketch_id: &cadmpeg_ir::sketches::SketchId,
    group: &DesignExtrudeSelectionGroup,
    members: &[DesignExtrudeSelectionMember],
    sketch: &cadmpeg_ir::sketches::Sketch,
    resolution: ScopedExtrudeProfileResolution<'_>,
    history_state_id: Option<i64>,
    previous_history_state_id: Option<i64>,
) -> Result<cadmpeg_ir::features::ProfileRef, CodecError> {
    use cadmpeg_ir::features::{PlanarProfileRef, ProfileRef};

    let mut selection_members = Vec::new();
    for member in members.iter().filter(|member| {
        native_stream(&member.id) == native_stream(&group.id)
            && member.group_record_index == group.record_index
    }) {
        selection_members.push(member);
    }
    selection_members.sort_by_key(|left| left.group_member_ordinal);
    let exact_member_run = selection_members.len() == group.members().len()
        && selection_members
            .iter()
            .zip(group.members())
            .all(|(member, record_index)| member.record_index() == record_index.value);
    let mut resolved_profiles = None;
    if exact_member_run {
        let mut selected = Vec::new();
        let mut complete = true;
        for member in &selection_members {
            let Some(geometry) = member.resolved_geometry.as_ref() else {
                complete = false;
                break;
            };
            let SketchRelationOperand::Curve {
                primary_id,
                secondary_id,
                ..
            } = geometry
            else {
                complete = false;
                break;
            };
            let entity = crate::ids::neutral_sketch_curve_id(sketch_id, *primary_id, *secondary_id);
            let matches: Vec<_> = sketch
                .profiles
                .iter()
                .enumerate()
                .filter(|(_, profile)| profile.iter().any(|use_| use_.entity == entity))
                .collect();
            let index = match matches.as_slice() {
                [(idx, _)] => *idx,
                _ => {
                    complete = false;
                    break;
                }
            };
            let Ok(profile_index) = u32::try_from(index) else {
                complete = false;
                break;
            };
            if !selected.contains(&profile_index) {
                selected.push(profile_index);
            }
        }
        if complete && !selected.is_empty() {
            resolved_profiles = Some(ResolvedProfileSelection::Loops(selected));
        }
    }
    if resolved_profiles.is_none() && exact_member_run {
        resolved_profiles = historical_selection_regions(
            &selection_members,
            sketch,
            resolution.entities,
            resolution.histories,
            resolution.linear_tolerance,
            resolution.profile_work,
            resolution.ctx,
        )?;
    }
    if resolved_profiles.is_none() {
        if let (Some(state_id), Some(previous_state_id)) =
            (history_state_id, previous_history_state_id)
        {
            resolved_profiles =
                transition_profile_selection(sketch, resolution, state_id, previous_state_id)?;
        }
    }
    if resolved_profiles.is_none() && exact_member_run {
        let mut selected = Vec::new();
        let mut complete = true;
        for member in &selection_members {
            let Some(SketchRelationOperand::Curve {
                primary_id,
                secondary_id,
                ..
            }) = member.resolved_geometry.as_ref()
            else {
                complete = false;
                break;
            };
            let entity = crate::ids::neutral_sketch_curve_id(sketch_id, *primary_id, *secondary_id);
            let matches: Vec<_> = sketch
                .profiles
                .iter()
                .enumerate()
                .filter(|(_, profile)| profile.iter().any(|use_| use_.entity == entity))
                .collect();
            let Some((idx, _)) = matches.iter().min_by_key(|(_, profile)| profile.len()) else {
                complete = false;
                break;
            };
            let Ok(profile_index) = u32::try_from(*idx) else {
                complete = false;
                break;
            };
            if !selected.contains(&profile_index) {
                selected.push(profile_index);
            }
        }
        if complete && !selected.is_empty() {
            resolved_profiles = Some(ResolvedProfileSelection::Loops(selected));
        }
    }
    if resolved_profiles.is_none() && sketch.profiles.len() == 1 {
        resolved_profiles = Some(ResolvedProfileSelection::Loops(vec![0]));
    }
    let profile = match resolved_profiles {
        Some(ResolvedProfileSelection::Loops(profiles)) => {
            match PlanarProfileRef::sketch_profiles(sketch_id.clone(), profiles) {
                Ok(profile) => profile,
                Err(_) => PlanarProfileRef::Native(group.id.clone()),
            }
        }
        Some(ResolvedProfileSelection::Regions(regions)) => {
            match PlanarProfileRef::sketch_regions(sketch_id.clone(), regions) {
                Ok(profile) => profile,
                Err(_) => PlanarProfileRef::Native(group.id.clone()),
            }
        }
        None => {
            let id = group.id.clone();
            let ids = vec![id];
            match PlanarProfileRef::sketch_selection(sketch_id.clone(), ids) {
                Ok(profile) => profile,
                Err(_) => PlanarProfileRef::Native(group.id.clone()),
            }
        }
    };
    Ok(ProfileRef::Planar(profile))
}

pub(in crate::design) fn resolve_historical_faces_to_sketch(
    faces: &[cadmpeg_ir::ids::HistoricalFaceId],
    sketches: &[cadmpeg_ir::sketches::Sketch],
    resolution: ScopedExtrudeProfileResolution<'_>,
    histories: &[crate::history_records::AsmHistory],
    state_id: i64,
    previous_state_id: i64,
) -> Option<(String, String, Option<f64>, Option<f64>)> {
    let mut prev_states = histories
        .iter()
        .flat_map(|history| history.states().iter().map(move |state| (history, state)))
        .filter(|(_, state)| state.state_id == previous_state_id);
    let (prev_history, prev_state) = prev_states.next()?;
    if prev_states.next().is_some() {
        return None;
    }
    let prev_topology = prev_history.topology(prev_state)?;
    let face_ids: Vec<i64> = faces
        .iter()
        .filter_map(|f| f.as_str().rsplit(':').next()?.parse::<i64>().ok())
        .collect();
    if face_ids.is_empty() {
        return None;
    }
    let first_face = face_ids[0];
    let face_surface = prev_topology
        .face_surfaces
        .iter()
        .find(|fs| fs.entity == first_face)
        .map(|fs| fs.carrier)
        .or_else(|| {
            prev_history
                .states()
                .iter()
                .filter_map(|s| prev_history.topology(s))
                .find_map(|t| {
                    t.face_surfaces
                        .iter()
                        .find(|fs| fs.entity == first_face)
                        .map(|fs| fs.carrier)
                })
        });
    let face_plane = face_surface.and_then(|s| {
        prev_topology
            .surface_planes
            .iter()
            .find(|sp| sp.surface == s)
            .or_else(|| {
                prev_history
                    .states()
                    .iter()
                    .filter_map(|st| prev_history.topology(st))
                    .find_map(|t| t.surface_planes.iter().find(|sp| sp.surface == s))
            })
    });
    let mut face_verts = Vec::new();
    for &fid in &face_ids {
        for pt in historical_face_all_points(fid, prev_topology) {
            if !face_verts.contains(&pt) {
                face_verts.push(pt);
            }
        }
    }

    // Collect edges of selected faces and adjacent faces in prev_topology
    let mut face_edges = HashSet::new();
    for &fid in &face_ids {
        for fl in prev_topology
            .face_loops
            .iter()
            .filter(|fl| fl.owner_ref == fid)
        {
            for &loop_id in &fl.member_refs {
                for lc in prev_topology
                    .loop_coedges
                    .iter()
                    .filter(|lc| lc.owner_ref == loop_id)
                {
                    for &coedge_id in &lc.member_refs {
                        if let Some(ce) = prev_topology
                            .coedge_topology
                            .iter()
                            .find(|c| c.coedge == coedge_id)
                        {
                            face_edges.insert(ce.edge);
                        }
                    }
                }
            }
        }
    }
    let mut adjacent_faces = Vec::new();
    for ce in &prev_topology.coedge_topology {
        if face_edges.contains(&ce.edge) {
            for lc in prev_topology
                .loop_coedges
                .iter()
                .filter(|lc| lc.member_refs.contains(&ce.coedge))
            {
                for fl in prev_topology
                    .face_loops
                    .iter()
                    .filter(|fl| fl.member_refs.contains(&lc.owner_ref))
                {
                    if !face_ids.contains(&fl.owner_ref) && !adjacent_faces.contains(&fl.owner_ref)
                    {
                        adjacent_faces.push(fl.owner_ref);
                    }
                }
            }
        }
    }
    let mut circle_axes = Vec::new();
    for &fid in &face_ids {
        for fl in prev_topology
            .face_loops
            .iter()
            .filter(|fl| fl.owner_ref == fid)
        {
            for &loop_id in &fl.member_refs {
                for lc in prev_topology
                    .loop_coedges
                    .iter()
                    .filter(|lc| lc.owner_ref == loop_id)
                {
                    if lc.member_refs.len() == 1 {
                        let coedge_id = lc.member_refs[0];
                        if let Some(ce) = prev_topology
                            .coedge_topology
                            .iter()
                            .find(|c| c.coedge == coedge_id)
                        {
                            if let Some(ec) = prev_topology
                                .edge_curves
                                .iter()
                                .find(|ec| ec.entity == ce.edge)
                            {
                                if let Some(ca) = prev_topology
                                    .curve_axes
                                    .iter()
                                    .find(|ca| Some(ca.curve) == ec.carrier)
                                {
                                    if !circle_axes.contains(&(ca.origin, ca.direction)) {
                                        circle_axes.push((ca.origin, ca.direction));
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    let mut best_match: Option<(f64, Option<f64>, &cadmpeg_ir::sketches::Sketch, Vec<u32>)> = None;
    for candidate_sketch in sketches.iter().rev() {
        let Some((sk_origin, sk_normal, sk_u)) = candidate_sketch.resolved_placement() else {
            continue;
        };
        if let Some(fp) = face_plane {
            let align =
                fp.normal.x * sk_normal.x + fp.normal.y * sk_normal.y + fp.normal.z * sk_normal.z;
            if align.abs() < 0.98 {
                continue;
            }
        } else if let Some((_, axis_dir)) = circle_axes.first() {
            let align =
                axis_dir.x * sk_normal.x + axis_dir.y * sk_normal.y + axis_dir.z * sk_normal.z;
            if align.abs() < 0.98 {
                continue;
            }
        } else if let Some(first_pt) = face_verts.first() {
            let all_coplanar_with_sketch = face_verts.iter().all(|pt| {
                let d = pt.vector_from(*first_pt).dot(*sk_normal.as_raw());
                d.abs() <= resolution.linear_tolerance * 10.0
            });
            if !all_coplanar_with_sketch {
                continue;
            }
        }
        let signed_plane_offset = face_verts
            .first()
            .or_else(|| circle_axes.first().map(|(c, _)| c))
            .or_else(|| face_plane.map(|fp| &fp.origin))
            .map(|pt| pt.vector_from(*sk_origin).dot(*sk_normal.as_raw()));
        let Some(dist) = historical_extrude_to_distance(
            candidate_sketch,
            signed_plane_offset.unwrap_or(0.0),
            histories,
            state_id,
            previous_state_id,
        ) else {
            continue;
        };
        if dist.abs() <= resolution.linear_tolerance {
            continue;
        }
        let sk_v = sk_normal.cross(sk_u.get());
        let mut matched_loops = Vec::new();
        if !circle_axes.is_empty() {
            for &(c_origin, _) in &circle_axes {
                let offset = c_origin.vector_from(*sk_origin);
                let p2 = Point2::new(offset.dot(sk_u.get()), offset.dot(sk_v));
                for (idx, prof) in candidate_sketch.profiles.iter().enumerate() {
                    if prof.len() != circle_axes.len() {
                        continue;
                    }
                    let Ok(idx_u32) = u32::try_from(idx) else {
                        continue;
                    };
                    let on_circle_center = prof.iter().any(|use_| {
                        resolution
                            .entities
                            .iter()
                            .find(|e| e.sketch == candidate_sketch.id && e.id() == &use_.entity)
                            .is_some_and(|e| match e.geometry.definition() {
                                cadmpeg_ir::sketches::SketchGeometryDefinition::Circle {
                                    center,
                                    ..
                                } => {
                                    let du = p2.u - center.u;
                                    let dv = p2.v - center.v;
                                    du.hypot(dv) <= resolution.linear_tolerance * 10.0
                                }
                                _ => false,
                            })
                    });
                    if on_circle_center && !matched_loops.contains(&idx_u32) {
                        matched_loops.push(idx_u32);
                    }
                }
            }
        }
        if matched_loops.is_empty() && !face_verts.is_empty() {
            let mut all_verts_matched = true;
            for &pt in &face_verts {
                let offset = pt.vector_from(*sk_origin);
                let p2 = Point2::new(offset.dot(sk_u.get()), offset.dot(sk_v));
                let mut vert_loops = Vec::new();
                for (idx, prof) in candidate_sketch.profiles.iter().enumerate() {
                    let Ok(idx_u32) = u32::try_from(idx) else {
                        continue;
                    };
                    let on_prof = prof.iter().any(|use_| {
                        resolution
                            .entities
                            .iter()
                            .find(|e| e.sketch == candidate_sketch.id && e.id() == &use_.entity)
                            .is_some_and(|e| {
                                let is_circle_or_arc = matches!(
                                    e.geometry.definition(),
                                    cadmpeg_ir::sketches::SketchGeometryDefinition::Circle { .. }
                                        | cadmpeg_ir::sketches::SketchGeometryDefinition::Arc { .. }
                                );
                                (is_circle_or_arc || face_verts.len() >= 3)
                                    && point_on_sketch_entity(p2, e, resolution.linear_tolerance)
                            })
                    });
                    if on_prof {
                        vert_loops.push(idx_u32);
                    }
                }
                if vert_loops.is_empty() {
                    all_verts_matched = false;
                    break;
                }
                for l in vert_loops {
                    if !matched_loops.contains(&l) {
                        matched_loops.push(l);
                    }
                }
            }
            if !all_verts_matched {
                matched_loops.clear();
            }
        }
        if matched_loops.is_empty() {
            for &adj_face in &adjacent_faces {
                let cyl = inserted_cylindrical_profile_selection(
                    candidate_sketch,
                    resolution.entities,
                    prev_topology,
                    adj_face,
                    resolution.linear_tolerance,
                    resolution.angular_tolerance,
                );
                if let Some(ResolvedProfileSelection::Loops(loops)) = cyl {
                    for l in loops {
                        if !matched_loops.contains(&l) {
                            matched_loops.push(l);
                        }
                    }
                }
            }
        }
        if matched_loops.is_empty() {
            if let Ok(Some(ResolvedProfileSelection::Loops(loops))) = transition_profile_selection(
                candidate_sketch,
                resolution,
                state_id,
                previous_state_id,
            ) {
                matched_loops = loops;
            }
        }
        if !matched_loops.is_empty() {
            matched_loops.sort_unstable();
            let plane_dist = signed_plane_offset.map_or(f64::INFINITY, f64::abs);
            if best_match
                .as_ref()
                .is_none_or(|(best_d, _, _, _)| plane_dist < *best_d)
            {
                best_match = Some((
                    plane_dist,
                    signed_plane_offset,
                    candidate_sketch,
                    matched_loops,
                ));
            }
        }
    }
    if let Some((_, signed_plane_offset, candidate_sketch, matched_loops)) = best_match {
        let prof_str = matched_loops
            .iter()
            .map(u32::to_string)
            .collect::<Vec<_>>()
            .join(",");
        let dist = historical_extrude_to_distance(
            candidate_sketch,
            signed_plane_offset.unwrap_or(0.0),
            histories,
            state_id,
            previous_state_id,
        );
        return Some((
            candidate_sketch.id.as_str().to_string(),
            prof_str,
            signed_plane_offset,
            dist,
        ));
    }
    None
}

pub(crate) fn historical_extrude_to_distance(
    sketch: &cadmpeg_ir::sketches::Sketch,
    start_offset: f64,
    histories: &[crate::history_records::AsmHistory],
    state_id: i64,
    previous_state_id: i64,
) -> Option<f64> {
    let (sketch_origin, sketch_normal, _) = sketch.resolved_placement()?;
    let mut states = histories
        .iter()
        .flat_map(|history| history.states().iter().map(move |state| (history, state)))
        .filter(|(_, state)| state.state_id == state_id);
    let (history, end_state) = states.next()?;
    if states.next().is_some() {
        return None;
    }
    let end_topology = history.topology(end_state)?;
    let mut chain = Vec::new();
    let mut cur = end_state;
    while cur.state_id != previous_state_id && chain.len() < 8 {
        let prev_id = cur.transition.as_ref()?.previous_state_id?;
        chain.push(cur);
        if prev_id == previous_state_id {
            break;
        }
        cur = history.states().iter().find(|s| s.state_id == prev_id)?;
    }
    let signed_dist = |p: Point3| -> f64 {
        ((p.x - sketch_origin.x) * sketch_normal.x
            + (p.y - sketch_origin.y) * sketch_normal.y
            + (p.z - sketch_origin.z) * sketch_normal.z)
            - start_offset
    };
    let mut best: f64 = 0.0;
    let update_best = |best: &mut f64, d: f64| {
        if d.abs() > 0.1 && (*best == 0.0 || d.abs() < best.abs()) {
            *best = d;
        }
    };
    for step_state in &chain {
        let Some(transition) = step_state.transition.as_ref() else {
            continue;
        };
        let Some(topology) = history.topology(step_state) else {
            continue;
        };
        for &pt_id in &transition.topology.points.inserted {
            if !end_topology.points.contains(&pt_id) {
                continue;
            }
            if let Some(p) = topology
                .point_positions
                .iter()
                .find(|p| p.point == pt_id)
                .map(|p| p.position)
            {
                update_best(&mut best, signed_dist(p));
            }
        }
        for &curve_id in &transition.topology.curves.inserted {
            if !end_topology.curves.contains(&curve_id) {
                continue;
            }
            if let Some(ca) = topology.curve_axes.iter().find(|c| c.curve == curve_id) {
                let align = ca.direction.x * sketch_normal.x
                    + ca.direction.y * sketch_normal.y
                    + ca.direction.z * sketch_normal.z;
                if align.abs() > 0.98 {
                    update_best(&mut best, signed_dist(ca.origin));
                }
            }
        }
        for &surf_id in &transition.topology.surfaces.inserted {
            if !end_topology.surfaces.contains(&surf_id) {
                continue;
            }
            if let Some(sp) = topology
                .surface_planes
                .iter()
                .find(|s| s.surface == surf_id)
            {
                let align = sp.normal.x * sketch_normal.x
                    + sp.normal.y * sketch_normal.y
                    + sp.normal.z * sketch_normal.z;
                if align.abs() > 0.98 {
                    update_best(&mut best, signed_dist(sp.origin));
                }
            }
        }
    }
    if best.abs() <= 1.0e-4 {
        for step_state in &chain {
            let Some(transition) = step_state.transition.as_ref() else {
                continue;
            };
            let Some(topology) = history.topology(step_state) else {
                continue;
            };
            let max_cyl_r = transition
                .topology
                .surfaces
                .inserted
                .iter()
                .filter_map(|&sid| {
                    topology
                        .surface_cylinders
                        .iter()
                        .find(|c| c.surface == sid)
                        .map(|c| c.radius)
                })
                .fold(0.0_f64, f64::max);
            for &curve_id in &transition.topology.curves.inserted {
                if !end_topology.curves.contains(&curve_id) {
                    continue;
                }
                if let Some(ca) = topology.curve_axes.iter().find(|c| c.curve == curve_id) {
                    let align = ca.direction.x * sketch_normal.x
                        + ca.direction.y * sketch_normal.y
                        + ca.direction.z * sketch_normal.z;
                    if align.abs() > 0.35 {
                        let d = signed_dist(ca.origin);
                        if d.abs() > 0.1 {
                            let tan_theta = (1.0 - align * align).max(0.0).sqrt() / align.abs();
                            let d_adj = d + d.signum() * max_cyl_r * tan_theta;
                            update_best(&mut best, d_adj);
                        }
                    }
                }
            }
            for &surf_id in &transition.topology.surfaces.inserted {
                if !end_topology.surfaces.contains(&surf_id) {
                    continue;
                }
                if let Some(sp) = topology
                    .surface_planes
                    .iter()
                    .find(|s| s.surface == surf_id)
                {
                    let align = sp.normal.x * sketch_normal.x
                        + sp.normal.y * sketch_normal.y
                        + sp.normal.z * sketch_normal.z;
                    if align.abs() > 0.35 {
                        update_best(&mut best, signed_dist(sp.origin));
                    }
                }
            }
        }
    }
    if best.abs() > 1.0e-4 {
        return Some(best);
    }
    None
}

pub(in crate::design) fn transition_profile_selection(
    sketch: &cadmpeg_ir::sketches::Sketch,
    resolution: ScopedExtrudeProfileResolution<'_>,
    state_id: i64,
    previous_state_id: i64,
) -> Result<Option<ResolvedProfileSelection>, CodecError> {
    let mut states = resolution
        .histories
        .iter()
        .flat_map(|history| history.states().iter().map(move |state| (history, state)))
        .filter(|(_, state)| state.state_id == state_id);
    let Some((history, end_state)) = states.next() else {
        return Ok(None);
    };
    if states.next().is_some() {
        return Ok(None);
    }
    let mut chain = Vec::new();
    let mut cur = end_state;
    while cur.state_id != previous_state_id && chain.len() < 8 {
        let Some(prev_id) = cur.transition.as_ref().and_then(|t| t.previous_state_id) else {
            return Ok(None);
        };
        chain.push((cur, prev_id));
        if prev_id == previous_state_id {
            break;
        }
        let Some(prev_state) = history.states().iter().find(|s| s.state_id == prev_id) else {
            return Ok(None);
        };
        cur = prev_state;
    }
    if chain.last().map(|(_, p)| *p) != Some(previous_state_id) {
        return Ok(None);
    }
    for (step_state, step_prev_id) in &chain {
        if let Some(sel) = transition_profile_selection_step(
            sketch,
            resolution,
            history,
            step_state,
            *step_prev_id,
        )? {
            return Ok(Some(sel));
        }
    }
    Ok(None)
}

fn transition_profile_selection_step(
    sketch: &cadmpeg_ir::sketches::Sketch,
    resolution: ScopedExtrudeProfileResolution<'_>,
    history: &crate::history_records::AsmHistory,
    state: &crate::history_records::AsmDeltaState,
    previous_state_id: i64,
) -> Result<Option<ResolvedProfileSelection>, CodecError> {
    macro_rules! geometric {
        ($value:expr) => {
            match $value {
                Some(value) => value,
                None => return Ok(None),
            }
        };
    }
    let entities = resolution.entities;
    let profile_work = resolution.profile_work;
    let topology = geometric!(history.topology(state));
    let inserted_faces = &geometric!(state.transition.as_ref())
        .topology
        .faces
        .inserted;
    let prev_topology_opt = history
        .states()
        .iter()
        .find(|s| s.state_id == previous_state_id)
        .and_then(|s| history.topology(s));
    // The document linear tolerance is admitted at or above the analytic floor
    // when the kernel header is read, so it drives the comparisons unchanged.
    let tolerance = resolution.linear_tolerance;
    let mut cylindrical_selections = Vec::new();
    let mut cyl_counts: HashMap<u32, usize> = HashMap::new();
    for face in inserted_faces {
        let selection = inserted_cylindrical_profile_selection(
            sketch,
            entities,
            topology,
            *face,
            tolerance,
            resolution.angular_tolerance,
        );
        if let Some(ResolvedProfileSelection::Loops(ref loops)) = selection {
            for &l in loops {
                *cyl_counts.entry(l).or_insert(0) += 1;
            }
        }
        cylindrical_selections.push(selection);
    }
    let mut prev_cyl_counts: HashMap<u32, usize> = HashMap::new();
    if cyl_counts.len() > 1 {
        if let Some(prev_topo) = prev_topology_opt {
            for fl in &prev_topo.face_loops {
                if let Some(ResolvedProfileSelection::Loops(loops)) =
                    inserted_cylindrical_profile_selection(
                        sketch,
                        entities,
                        prev_topo,
                        fl.owner_ref,
                        tolerance,
                        resolution.angular_tolerance,
                    )
                {
                    for l in loops {
                        *prev_cyl_counts.entry(l).or_insert(0) += 1;
                    }
                }
            }
        }
    }
    let mut cyl_loops = Vec::new();
    for selection in cylindrical_selections.iter().flatten() {
        let ResolvedProfileSelection::Loops(selected) = selection else {
            continue;
        };
        for &profile in selected {
            let cur_count = cyl_counts.get(&profile).copied().unwrap_or(0);
            let prev_count = prev_cyl_counts.get(&profile).copied().unwrap_or(0);
            if cur_count > prev_count && !cyl_loops.contains(&profile) {
                cyl_loops.push(profile);
            }
        }
    }
    let placement_opt = sketch.resolved_placement();
    let face_has_off_plane_point = |face: i64| -> bool {
        let Some((sk_origin, sk_normal, _)) = placement_opt else {
            return true;
        };
        historical_face_all_points(face, topology)
            .into_iter()
            .any(|pt| pt.vector_from(*sk_origin).dot(*sk_normal.as_raw()).abs() > tolerance)
    };
    let has_prior_faces = prev_topology_opt.is_some_and(|pt| !pt.faces.is_empty());
    let any_off_plane_face = inserted_faces.iter().any(|&f| face_has_off_plane_point(f));
    let only_circular_faces = !cyl_loops.is_empty()
        && inserted_faces
            .iter()
            .zip(&cylindrical_selections)
            .all(|(face, cyl_sel)| {
                cyl_sel.is_some()
                    || (has_prior_faces && any_off_plane_face && !face_has_off_plane_point(*face))
                    || historical_face_points(*face, topology).is_none_or(|pts| pts.len() <= 2)
            });
    if only_circular_faces {
        return Ok(Some(ResolvedProfileSelection::Loops(cyl_loops)));
    }
    let mut inserted_selections = Vec::new();
    let mut cap_selections = Vec::new();
    for face in inserted_faces {
        if has_prior_faces && any_off_plane_face && !face_has_off_plane_point(*face) {
            continue;
        }
        let points = historical_face_points(*face, topology);
        let selection = match points.as_deref() {
            Some(points) => selection_containing_points(
                sketch,
                entities,
                points,
                tolerance,
                profile_work,
                resolution.ctx,
            )?,
            None => None,
        };
        if points
            .as_deref()
            .is_some_and(|points| points_share_sketch_offset(sketch, points, tolerance))
        {
            cap_selections.push(selection.clone());
        }
        inserted_selections.push(selection);
    }
    let inserted = transition_inserted_profile_selection(
        sketch,
        entities,
        tolerance,
        inserted_selections,
        profile_work,
        resolution.ctx,
    )?;
    if let Some(sel) = inserted {
        if let ResolvedProfileSelection::Loops(ref loops) = sel {
            if loops.len() == 1
                && !cyl_loops.is_empty()
                && !cyl_loops.contains(&loops[0])
                && usize::try_from(loops[0])
                    .ok()
                    .and_then(|idx| sketch.profiles.get(idx))
                    .is_some_and(|p| p.len() > 1)
            {
                if let Ok(region) =
                    cadmpeg_ir::features::SketchProfileRegion::loops(loops[0], cyl_loops.clone())
                {
                    return Ok(Some(ResolvedProfileSelection::Regions(vec![region])));
                }
            }
        }
        return Ok(Some(sel));
    }
    // A cap lies in one plane parallel to the Sketch and repeats the profile.
    // Side faces that a Boolean merges with existing faces can select other
    // loops, so agreeing caps prove the selection over differing side faces.
    if !cap_selections.is_empty() && cap_selections.iter().all(Option::is_some) {
        if let Some(selection) = unique_resolved_selection(cap_selections) {
            if let ResolvedProfileSelection::Loops(ref loops) = selection {
                if loops.len() == 1
                    && !cyl_loops.is_empty()
                    && !cyl_loops.contains(&loops[0])
                    && usize::try_from(loops[0])
                        .ok()
                        .and_then(|idx| sketch.profiles.get(idx))
                        .is_some_and(|p| p.len() > 1)
                {
                    if let Ok(region) = cadmpeg_ir::features::SketchProfileRegion::loops(
                        loops[0],
                        cyl_loops.clone(),
                    ) {
                        return Ok(Some(ResolvedProfileSelection::Regions(vec![region])));
                    }
                }
            }
            return Ok(Some(selection));
        }
    }
    if let Some(selection) = unique_resolved_selection(cylindrical_selections) {
        return Ok(Some(selection));
    }
    if !cyl_loops.is_empty() {
        return Ok(Some(ResolvedProfileSelection::Loops(cyl_loops)));
    }
    if let Some((sk_origin, sk_normal, _)) = sketch.resolved_placement() {
        let mut inserted_plane_pts = Vec::new();
        for face in inserted_faces {
            for pt in historical_face_all_points(*face, topology) {
                let d = pt.vector_from(*sk_origin).dot(*sk_normal.as_raw()).abs();
                if d <= tolerance * 10.0 {
                    if let Some(p2) = project_to_sketch(sketch, pt) {
                        inserted_plane_pts.push(p2);
                    }
                }
            }
        }
        if !inserted_plane_pts.is_empty() {
            let mut circle_loops = Vec::new();
            for (idx, prof) in sketch.profiles.iter().enumerate() {
                let Ok(idx_u32) = u32::try_from(idx) else {
                    continue;
                };
                let is_matched_circle = prof.len() == 1
                    && prof.iter().all(|use_| {
                        entities
                            .iter()
                            .find(|e| e.id() == &use_.entity)
                            .is_some_and(|e| {
                                matches!(
                                    e.geometry.definition(),
                                    cadmpeg_ir::sketches::SketchGeometryDefinition::Circle { .. }
                                ) && inserted_plane_pts
                                    .iter()
                                    .any(|&p2| point_on_sketch_entity(p2, e, tolerance))
                            })
                    });
                if is_matched_circle {
                    circle_loops.push(idx_u32);
                }
            }
            if circle_loops.len() == 1 {
                return Ok(Some(ResolvedProfileSelection::Loops(circle_loops)));
            }
        }
    }
    // Deleted faces exist in the state immediately before the transition.
    let immediate_previous_state_id = geometric!(state
        .transition
        .as_ref()
        .and_then(|transition| transition.previous_state_id));
    let mut previous_states = resolution
        .histories
        .iter()
        .flat_map(|history| history.states().iter().map(move |state| (history, state)))
        .filter(|(_, state)| state.state_id == immediate_previous_state_id);
    let (previous_history, previous) = geometric!(previous_states.next());
    if previous_states.next().is_some() {
        return Ok(None);
    }
    let previous_topology = geometric!(previous_history.topology(previous));
    let deleted = &geometric!(state.transition.as_ref()).topology.faces.deleted;
    let faces = geometric!(unique_multi_face_deleted_carrier_family(
        deleted,
        previous_topology
    ));
    let mut selections = Vec::new();
    let mut combined_deleted_points = Vec::new();
    for face in faces {
        let pts = historical_face_points(face, previous_topology);
        if let Some(ref points) = pts {
            for &p in points {
                if !combined_deleted_points.contains(&p) {
                    combined_deleted_points.push(p);
                }
            }
        }
        let selection = match pts {
            Some(points) => selection_containing_points(
                sketch,
                entities,
                &points,
                tolerance,
                profile_work,
                resolution.ctx,
            )?,
            None => None,
        };
        selections.push(selection);
    }
    if !combined_deleted_points.is_empty() {
        let mut proj_pts = Vec::new();
        for &pt in &combined_deleted_points {
            if let Some(p2) = project_to_sketch(sketch, pt) {
                proj_pts.push(p2);
            }
        }
        let mut circle_loops = Vec::new();
        for (idx, prof) in sketch.profiles.iter().enumerate() {
            let Ok(idx_u32) = u32::try_from(idx) else {
                continue;
            };
            let is_matched_circle = prof.len() == 1
                && prof.iter().all(|use_| {
                    entities
                        .iter()
                        .find(|e| e.id() == &use_.entity)
                        .is_some_and(|e| {
                            matches!(
                                e.geometry.definition(),
                                cadmpeg_ir::sketches::SketchGeometryDefinition::Circle { .. }
                            ) && proj_pts
                                .iter()
                                .filter(|&&p2| point_on_sketch_entity(p2, e, tolerance))
                                .count()
                                >= 2
                        })
                });
            if is_matched_circle {
                circle_loops.push(idx_u32);
            }
        }
        if !circle_loops.is_empty() {
            return Ok(Some(ResolvedProfileSelection::Loops(circle_loops)));
        }
    }
    Ok(ordered_unique_profile_selections(selections))
}

/// The unique state `state_id` whose transition links back to
/// `previous_state_id`. The link is direct, or it passes through unique
/// states whose transitions carry no history rows and neither insert nor
/// delete any topology entity.
fn linked_transition_state(
    histories: &[crate::history_records::AsmHistory],
    state_id: i64,
    previous_state_id: i64,
) -> Option<(
    &crate::history_records::AsmHistory,
    &crate::history_records::AsmDeltaState,
)> {
    let unique_state = |id: i64| {
        let mut states = histories
            .iter()
            .flat_map(|history| history.states().iter().map(move |state| (history, state)))
            .filter(move |(_, state)| state.state_id == id);
        let found = states.next()?;
        states.next().is_none().then_some(found)
    };
    let found = unique_state(state_id)?;
    let state_count: usize = histories.iter().map(|history| history.states().len()).sum();
    let mut linked = found.1.transition.as_ref()?.previous_state_id?;
    for _ in 0..state_count {
        if linked == previous_state_id {
            return Some(found);
        }
        let transition = unique_state(linked)?.1.transition.as_ref()?;
        if transition.rows != crate::history_records::AsmHistoricalRowsDelta::default()
            || !topology_delta_keeps_entities(&transition.topology)
        {
            return None;
        }
        linked = transition.previous_state_id?;
    }
    None
}

/// Whether a topology delta inserts and deletes no entity of any family.
fn topology_delta_keeps_entities(
    delta: &crate::history_records::AsmHistoricalTopologyDelta,
) -> bool {
    [
        &delta.bodies,
        &delta.regions,
        &delta.shells,
        &delta.faces,
        &delta.loops,
        &delta.coedges,
        &delta.edges,
        &delta.vertices,
        &delta.points,
        &delta.surfaces,
        &delta.curves,
        &delta.pcurves,
    ]
    .into_iter()
    .all(|family| family.inserted.is_empty() && family.deleted.is_empty())
}

/// Whether every point lies at one offset along the Sketch normal.
fn points_share_sketch_offset(
    sketch: &cadmpeg_ir::sketches::Sketch,
    points: &[Point3],
    tolerance: f64,
) -> bool {
    let Some((origin, normal, _)) = sketch.resolved_placement() else {
        return false;
    };
    let offset = |point: &Point3| {
        (point.x - origin.x) * normal.x
            + (point.y - origin.y) * normal.y
            + (point.z - origin.z) * normal.z
    };
    let Some(first) = points.first().map(offset) else {
        return false;
    };
    points
        .iter()
        .all(|point| (offset(point) - first).abs() <= tolerance)
}

fn inserted_cylindrical_profile_selection(
    sketch: &cadmpeg_ir::sketches::Sketch,
    entities: &[cadmpeg_ir::sketches::SketchEntity],
    topology: &crate::history_records::AsmHistoricalTopology,
    face: i64,
    linear_tolerance: f64,
    angular_tolerance: f64,
) -> Option<ResolvedProfileSelection> {
    use cadmpeg_ir::sketches::SketchGeometryDefinition;

    let mut carriers = topology
        .face_surfaces
        .iter()
        .filter(|binding| binding.entity == face);
    let carrier = carriers.next().map(|binding| binding.carrier)?;
    if carriers.next().is_some() {
        return None;
    }
    let mut cylinders = topology
        .surface_cylinders
        .iter()
        .filter(|cylinder| cylinder.surface == carrier);
    let cylinder = cylinders.next()?;
    if cylinders.next().is_some() || !cylinder.radius.is_finite() || cylinder.radius <= 0.0 {
        return None;
    }
    let (sketch_origin, sketch_normal, _) = sketch.resolved_placement()?;
    let alignment = cylinder.axis.x * sketch_normal.x
        + cylinder.axis.y * sketch_normal.y
        + cylinder.axis.z * sketch_normal.z;
    if alignment.abs() < angular_tolerance.cos() {
        return None;
    }
    let offset = (sketch_origin.x - cylinder.origin.x) * sketch_normal.x
        + (sketch_origin.y - cylinder.origin.y) * sketch_normal.y
        + (sketch_origin.z - cylinder.origin.z) * sketch_normal.z;
    let parameter = offset / alignment;
    let center = project_to_sketch(
        sketch,
        Point3::new(
            cylinder.origin.x + parameter * cylinder.axis.x,
            cylinder.origin.y + parameter * cylinder.axis.y,
            cylinder.origin.z + parameter * cylinder.axis.z,
        ),
    )?;
    let points = historical_face_points(face, topology)?;
    let mut projected = Vec::new();
    for point in &points {
        let point = project_to_sketch(sketch, *point)?;
        projected.push(point);
    }
    let mut matches = sketch
        .profiles
        .iter()
        .enumerate()
        .filter_map(|(index, profile)| {
            let [use_] = profile.as_slice() else {
                return None;
            };
            let entity = entities
                .iter()
                .find(|entity| entity.sketch == sketch.id && entity.id() == &use_.entity)?;
            let SketchGeometryDefinition::Circle {
                center: candidate_center,
                radius: candidate_radius,
            } = *entity.geometry.definition()
            else {
                return None;
            };
            ((candidate_center.u - center.u).hypot(candidate_center.v - center.v)
                <= linear_tolerance
                && (candidate_radius.get() - cylinder.radius).abs() <= linear_tolerance
                && projected.iter().all(|point| {
                    ((point.u - candidate_center.u).hypot(point.v - candidate_center.v)
                        - candidate_radius.get())
                    .abs()
                        <= linear_tolerance
                }))
            .then(|| u32::try_from(index).ok())?
        });
    let profile = matches.next()?;
    matches
        .next()
        .is_none()
        .then(|| ResolvedProfileSelection::Loops(vec![profile]))
}

fn resolved_spatial_extrude_profile_selection(
    group: &DesignExtrudeSelectionGroup,
    members: &[DesignExtrudeSelectionMember],
    sketch: &cadmpeg_ir::sketches::SpatialSketch,
    entities: &[cadmpeg_ir::sketches::SpatialSketchEntity],
    resolution: ScopedExtrudeProfileResolution<'_>,
    history_state_id: Option<i64>,
    previous_history_state_id: Option<i64>,
) -> Result<Option<u32>, CodecError> {
    enum ExactSelection {
        Resolved(u32),
        Unavailable,
        Contradictory,
    }

    let mut group_members = Vec::new();
    for member in members.iter().filter(|member| {
        native_stream(&member.id) == native_stream(&group.id)
            && member.group_record_index == group.record_index
    }) {
        group_members.push(member);
    }
    group_members.sort_by_key(|left| left.group_member_ordinal);
    let exact_member_run = group_members.len() == group.members().len()
        && group_members
            .iter()
            .zip(group.members())
            .all(|(member, record_index)| member.record_index() == record_index.value);
    let exact_selection = (|| -> ExactSelection {
        if !exact_member_run {
            return ExactSelection::Unavailable;
        }
        let mut selected = None;
        for member in group_members {
            let Some(SketchRelationOperand::Curve {
                primary_id,
                secondary_id,
                ..
            }) = member.resolved_geometry.as_ref()
            else {
                return ExactSelection::Unavailable;
            };
            let entity =
                crate::ids::neutral_spatial_sketch_curve_id(&sketch.id, *primary_id, *secondary_id);
            let mut matches =
                sketch.profiles.iter().enumerate().filter(|(_, profile)| {
                    profile.boundary().iter().any(|use_| use_.entity == entity)
                });
            let Some((index, _)) = matches.next() else {
                return ExactSelection::Unavailable;
            };
            if matches.next().is_some() {
                return ExactSelection::Unavailable;
            }
            let Ok(profile) = u32::try_from(index) else {
                return ExactSelection::Unavailable;
            };
            if selected
                .replace(profile)
                .is_some_and(|selected| selected != profile)
            {
                return ExactSelection::Contradictory;
            }
        }
        selected.map_or(ExactSelection::Unavailable, ExactSelection::Resolved)
    })();
    match exact_selection {
        ExactSelection::Resolved(selection) => Ok(Some(selection)),
        ExactSelection::Contradictory => Ok(None),
        ExactSelection::Unavailable => {
            let transition = if let Some((state_id, previous_state_id)) =
                history_state_id.zip(previous_history_state_id)
            {
                transition_spatial_profile_selection(
                    sketch,
                    entities,
                    resolution.histories,
                    state_id,
                    previous_state_id,
                    resolution.linear_tolerance,
                )?
            } else {
                None
            };
            Ok(transition.or_else(|| (sketch.profiles.len() == 1).then_some(0)))
        }
    }
}

fn transition_spatial_profile_selection(
    sketch: &cadmpeg_ir::sketches::SpatialSketch,
    entities: &[cadmpeg_ir::sketches::SpatialSketchEntity],
    histories: &[crate::history_records::AsmHistory],
    state_id: i64,
    previous_state_id: i64,
    linear_tolerance: f64,
) -> Result<Option<u32>, CodecError> {
    let Some((history, state)) = linked_transition_state(histories, state_id, previous_state_id)
    else {
        return Ok(None);
    };
    let Some(topology) = history.topology(state) else {
        return Ok(None);
    };
    // The document linear tolerance is admitted at or above the analytic floor
    // when the kernel header is read, so it drives the comparisons unchanged.
    let tolerance = linear_tolerance;
    let unique = |faces: &[i64],
                  topology: &crate::history_records::AsmHistoricalTopology|
     -> Result<Option<u32>, CodecError> {
        let mut indices = Vec::new();
        for face in faces {
            if let Some(points) = historical_face_points(*face, topology) {
                if let Some(index) =
                    spatial_polyline_profile_containing_points(sketch, entities, &points, tolerance)
                {
                    indices.push(index);
                }
            }
        }
        indices.sort_unstable_by(Ord::cmp);
        indices.dedup();
        Ok((indices.len() == 1).then(|| indices[0]))
    };
    let Some(transition) = state.transition.as_ref() else {
        return Ok(None);
    };
    if let Some(index) = unique(&transition.topology.faces.inserted, topology)? {
        return Ok(Some(index));
    }
    // Deleted faces exist in the state immediately before the transition.
    let Some(immediate_previous_state_id) = transition.previous_state_id else {
        return Ok(None);
    };
    let mut previous_states = histories
        .iter()
        .flat_map(|history| history.states().iter().map(move |state| (history, state)))
        .filter(|(_, state)| state.state_id == immediate_previous_state_id);
    let Some((previous_history, previous)) = previous_states.next() else {
        return Ok(None);
    };
    if previous_states.next().is_some() {
        return Ok(None);
    }
    let Some(previous_topology) = previous_history.topology(previous) else {
        return Ok(None);
    };
    unique(&transition.topology.faces.deleted, previous_topology)
}

fn spatial_polyline_profile_containing_points(
    sketch: &cadmpeg_ir::sketches::SpatialSketch,
    entities: &[cadmpeg_ir::sketches::SpatialSketchEntity],
    points: &[Point3],
    tolerance: f64,
) -> Option<u32> {
    let mut selected = None;
    for (index, profile) in sketch.profiles.iter().enumerate() {
        let mut offsets = points.iter().map(|point| {
            point
                .vector_from(profile.origin().get())
                .dot(profile.normal().into())
        });
        let Some(first) = offsets.next() else {
            continue;
        };
        if !offsets.all(|offset| (offset - first).abs() <= tolerance) {
            continue;
        }
        let normal = Vector3::from(profile.normal());
        let u_axis = Vector3::from(profile.u_axis());
        let v_axis = normal.cross(u_axis);
        let project = |point: Point3| {
            let offset = point.vector_from(profile.origin().get());
            Point2::new(offset.dot(u_axis), offset.dot(v_axis))
        };
        let mut polygon = Vec::new();
        for use_ in profile.boundary() {
            let entity = entities
                .iter()
                .find(|entity| entity.sketch == sketch.id && entity.id() == &use_.entity)?;
            let cadmpeg_ir::sketches::SpatialSketchGeometryDefinition::Line { start, end } =
                entity.geometry.definition()
            else {
                return None;
            };
            let point = project(if use_.reversed {
                end.get()
            } else {
                start.get()
            });
            polygon.push(point);
        }
        if polygon.len() >= 3
            && points.iter().all(|point| {
                let point = project(*point);
                point_in_polygon(point, &polygon)
                    || polygon.iter().enumerate().any(|(index, start)| {
                        let end = polygon[(index + 1) % polygon.len()];
                        point_segment_distance(point, (*start, end)) <= tolerance
                    })
            })
        {
            let Ok(index) = u32::try_from(index) else {
                return None;
            };
            if selected.replace(index).is_some() {
                return None;
            }
        }
    }
    selected
}

fn unique_multi_face_deleted_carrier_family(
    deleted_faces: &[i64],
    topology: &crate::history_records::AsmHistoricalTopology,
) -> Option<Vec<i64>> {
    let mut seen = HashSet::new();
    let mut families = HashMap::<i64, Vec<i64>>::new();
    for face in deleted_faces.iter().copied() {
        if seen.contains(&face) {
            return None;
        }
        seen.insert(face);
        let mut bindings = topology
            .face_surfaces
            .iter()
            .filter(|binding| binding.entity == face);
        let carrier = bindings.next().map(|binding| binding.carrier)?;
        if bindings.next().is_some() {
            return None;
        }
        families.entry(carrier).or_default().push(face);
    }
    let mut candidates = families.into_values().filter(|faces| faces.len() > 1);
    let mut faces = candidates.next()?;
    if candidates.next().is_some() {
        return None;
    }
    faces.sort_unstable_by(Ord::cmp);
    Some(faces)
}

fn unique_resolved_selection<T: PartialEq>(
    selections: impl IntoIterator<Item = Option<T>>,
) -> Option<T> {
    let mut selections = selections.into_iter().flatten();
    let first = selections.next()?;
    selections
        .all(|selection| selection == first)
        .then_some(first)
}

pub(in crate::design) fn transition_inserted_profile_selection(
    sketch: &cadmpeg_ir::sketches::Sketch,
    entities: &[cadmpeg_ir::sketches::SketchEntity],
    tolerance: f64,
    selections: Vec<Option<ResolvedProfileSelection>>,
    profile_work: &ProfileGeometryWork,
    ctx: &DecodeContext<'_>,
) -> Result<Option<ResolvedProfileSelection>, CodecError> {
    use cadmpeg_ir::features::SketchProfileRegion;

    let first_selection = selections.iter().flatten().next();
    if let Some(first) = first_selection {
        if selections
            .iter()
            .flatten()
            .all(|selection| selection == first)
        {
            return Ok(selections.into_iter().flatten().next());
        }
    }
    let mut loop_selections = selections
        .iter()
        .flatten()
        .filter_map(|selection| match selection {
            ResolvedProfileSelection::Loops(loops) if !loops.is_empty() => Some(loops.as_slice()),
            _ => None,
        });
    if let Some(first) = loop_selections.next() {
        if loop_selections.all(|candidate| candidate == first) {
            return Ok(selections.into_iter().flatten().find(|selection| {
                matches!(selection, ResolvedProfileSelection::Loops(loops) if !loops.is_empty())
            }));
        }
    }
    if selections.iter().flatten().all(|selection| {
        matches!(selection, ResolvedProfileSelection::Loops(loops) if !loops.is_empty())
    }) {
        let mut loops = Vec::new();
        let mut loop_counts: HashMap<u32, usize> = HashMap::new();
        for selection in selections.iter().flatten() {
            let ResolvedProfileSelection::Loops(selected) = selection else { continue; };
            for profile in selected.iter().copied() {
                *loop_counts.entry(profile).or_insert(0) += 1;
                if !loops.contains(&profile) {
                    loops.push(profile);
                }
            }
        }
        if !loops.is_empty() {
            if profile_loops_are_independent(
                sketch,
                entities,
                &loops,
                tolerance,
                profile_work,
                ctx,
            )? {
                return Ok(Some(ResolvedProfileSelection::Loops(loops)));
            }
            return Ok(None);
        }
    }
    let mut regions = selections
        .iter()
        .flatten()
        .filter_map(|selection| match selection {
            ResolvedProfileSelection::Regions(regions) => match regions.as_slice() {
                [SketchProfileRegion::Loops { loops }] if !loops.holes().is_empty() => {
                    Some((loops.outer(), loops.holes()))
                }
                _ => None,
            },
            ResolvedProfileSelection::Loops(_) => None,
        });
    let Some((outer, holes)) = regions.next() else {
        return Ok(None);
    };
    if regions.any(|candidate| candidate != (outer, holes)) {
        return Ok(None);
    }
    let mut has_outer_support = false;
    let mut supported_holes = Vec::new();
    for selection in selections.iter().flatten() {
        match selection {
            ResolvedProfileSelection::Regions(regions)
                if matches!(
                    regions.as_slice(),
                    [SketchProfileRegion::Loops { loops }] if loops.outer() == outer && loops.holes() == holes
                ) => {}
            ResolvedProfileSelection::Loops(loops)
                if !loops.is_empty()
                    && loops
                        .iter()
                        .all(|profile| *profile == outer || holes.contains(profile)) =>
            {
                for &profile in loops {
                    if profile == outer {
                        has_outer_support = true;
                    } else if holes.contains(&profile) && !supported_holes.contains(&profile) {
                        supported_holes.push(profile);
                    }
                }
            }
            _ => return Ok(None),
        }
    }
    if !has_outer_support {
        if !supported_holes.is_empty() {
            return Ok(Some(ResolvedProfileSelection::Loops(supported_holes)));
        }
        return Ok(None);
    }
    let mut owned_holes = Vec::new();
    for hole in holes.iter().copied() {
        owned_holes.push(hole);
    }
    let Some(region) = SketchProfileRegion::loops(outer, owned_holes).ok() else {
        return Ok(None);
    };
    let output = vec![region];
    Ok(Some(ResolvedProfileSelection::Regions(output)))
}

fn historical_face_all_points(
    face: i64,
    topology: &crate::history_records::AsmHistoricalTopology,
) -> Vec<Point3> {
    let Some(loops) = topology
        .face_loops
        .iter()
        .find(|relation| relation.owner_ref == face)
    else {
        return Vec::new();
    };
    let mut positions = Vec::new();
    for loop_ref in &loops.member_refs {
        let Some(coedges) = topology
            .loop_coedges
            .iter()
            .find(|relation| relation.owner_ref == *loop_ref)
        else {
            continue;
        };
        for coedge_ref in &coedges.member_refs {
            let Some(coedge) = topology
                .coedge_topology
                .iter()
                .find(|coedge| coedge.coedge == *coedge_ref)
            else {
                continue;
            };
            let Some(edge) = topology
                .edge_vertices
                .iter()
                .find(|edge| edge.edge == coedge.edge)
            else {
                continue;
            };
            for vertex_ref in [edge.start_vertex, edge.end_vertex] {
                let Some(point_ref) = topology
                    .vertex_points
                    .iter()
                    .find(|binding| binding.entity == vertex_ref)
                    .map(|binding| binding.carrier)
                else {
                    continue;
                };
                let Some(position) = topology
                    .point_positions
                    .iter()
                    .find(|point| point.point == point_ref)
                    .map(|point| point.position)
                else {
                    continue;
                };
                if !positions.contains(&position) {
                    positions.push(position);
                }
            }
        }
    }
    positions
}

pub(super) fn historical_face_points(
    face: i64,
    topology: &crate::history_records::AsmHistoricalTopology,
) -> Option<Vec<Point3>> {
    let loops = topology
        .face_loops
        .iter()
        .find(|relation| relation.owner_ref == face)?;
    let mut positions = Vec::new();
    let mut closed_edges_only = true;
    for loop_ref in &loops.member_refs {
        let coedges = topology
            .loop_coedges
            .iter()
            .find(|relation| relation.owner_ref == *loop_ref)?;
        for coedge_ref in &coedges.member_refs {
            let coedge = topology
                .coedge_topology
                .iter()
                .find(|coedge| coedge.coedge == *coedge_ref)?;
            let edge = topology
                .edge_vertices
                .iter()
                .find(|edge| edge.edge == coedge.edge)?;
            closed_edges_only &= edge.start_vertex == edge.end_vertex;
            for vertex_ref in [edge.start_vertex, edge.end_vertex] {
                let point_ref = topology
                    .vertex_points
                    .iter()
                    .find(|binding| binding.entity == vertex_ref)
                    .map(|binding| binding.carrier)?;
                let position = topology
                    .point_positions
                    .iter()
                    .find(|point| point.point == point_ref)
                    .map(|point| point.position)?;
                if !positions.contains(&position) {
                    positions.push(position);
                }
            }
        }
    }
    // Three distinct vertices span a polygonal boundary. A boundary made only
    // of closed edges, such as full circles, has one vertex per edge, and
    // those vertices are its complete point set.
    (positions.len() >= 3 || (closed_edges_only && !positions.is_empty())).then_some(positions)
}

fn historical_selection_regions(
    members: &[&DesignExtrudeSelectionMember],
    sketch: &cadmpeg_ir::sketches::Sketch,
    entities: &[cadmpeg_ir::sketches::SketchEntity],
    histories: &[crate::history_records::AsmHistory],
    linear_tolerance: f64,
    profile_work: &ProfileGeometryWork,
    ctx: &DecodeContext<'_>,
) -> Result<Option<ResolvedProfileSelection>, CodecError> {
    // The document linear tolerance is admitted at or above the analytic floor
    // when the kernel header is read, so it drives the comparisons unchanged.
    let tolerance = linear_tolerance;
    let mut states = HashMap::new();
    for (history, state) in histories
        .iter()
        .flat_map(|history| history.states().iter().map(move |state| (history, state)))
    {
        if let Some(previous) = states.get_mut(&state.state_id) {
            *previous = None;
        } else {
            states.insert(state.state_id, Some((history, state)));
        }
    }
    let Some(first_member) = members.first() else {
        return Ok(None);
    };
    let mut state_ids = Vec::new();
    if let Some(binding) = first_member.historical.as_ref() {
        for state_id in binding.state_ids.iter().copied() {
            if !state_ids.contains(&state_id) {
                state_ids.push(state_id);
            }
        }
    }
    for member in &members[1..] {
        state_ids.retain(|state_id| {
            member
                .historical
                .as_ref()
                .is_some_and(|binding| binding.state_ids.contains(state_id))
        });
    }
    state_ids.sort_unstable_by(Ord::cmp);
    let mut previous_member_points: Option<Vec<Vec<Point3>>> = None;
    let mut state_selection = None;
    let mut conflicting_state_selections = false;
    for state_id in state_ids {
        let Some(topology) = states
            .get(&state_id)
            .and_then(|state| state.as_ref())
            .and_then(|(history, state)| history.topology(state))
        else {
            continue;
        };
        let mut member_points = Vec::new();
        let mut complete = true;
        for member in members {
            let points = match historical_member_points_in_state(member, topology) {
                Some(points) => Some(points),
                None => {
                    if let Some(point) = resolved_selection_member_point(member, sketch, entities) {
                        let points = vec![point];
                        Some(points)
                    } else {
                        None
                    }
                }
            };
            let Some(points) = points else {
                complete = false;
                break;
            };
            member_points.push(points);
        }
        if !complete {
            continue;
        }
        if previous_member_points
            .as_ref()
            .is_some_and(|previous| same_member_point_bits(previous, &member_points))
        {
            continue;
        }
        let selection = selection_for_member_points(
            members,
            sketch,
            entities,
            &member_points,
            tolerance,
            profile_work,
            ctx,
        )?;
        previous_member_points = Some(member_points);
        if let Some(selection) = selection {
            if let Some(previous) = state_selection.as_ref() {
                conflicting_state_selections |= previous != &selection;
            } else {
                state_selection = Some(selection);
            }
        }
    }
    if state_selection.is_some() {
        return Ok(if conflicting_state_selections {
            None
        } else {
            state_selection
        });
    }
    let mut member_points = Vec::new();
    let mut complete = true;
    for member in members {
        let Some(point) = resolved_selection_member_point(member, sketch, entities) else {
            complete = false;
            break;
        };
        let points = vec![point];
        member_points.push(points);
    }
    if complete {
        if let Some(selection) = selection_for_member_points(
            members,
            sketch,
            entities,
            &member_points,
            tolerance,
            profile_work,
            ctx,
        )? {
            return Ok(Some(selection));
        }
    }
    let mut selections = Vec::new();
    for member in members {
        let selection =
            if let Some(point) = resolved_selection_member_point(member, sketch, entities) {
                selection_containing_points(
                    sketch,
                    entities,
                    std::slice::from_ref(&point),
                    tolerance,
                    profile_work,
                    ctx,
                )?
            } else {
                resolved_selection_member_profiles(member, sketch)
                    .filter(|profiles| profiles.len() == 1)
                    .map(ResolvedProfileSelection::Loops)
            };
        selections.push(selection);
    }
    if ordered_selection_has_value(&selections) {
        return Ok(ordered_unique_profile_selections(selections));
    }
    Ok(region_with_boundary_selection_members(
        members,
        sketch,
        &selections,
    ))
}

fn same_member_point_bits(left: &[Vec<Point3>], right: &[Vec<Point3>]) -> bool {
    left.len() == right.len()
        && left.iter().zip(right).all(|(left, right)| {
            left.len() == right.len()
                && left.iter().zip(right).all(|(left, right)| {
                    left.x.to_bits() == right.x.to_bits()
                        && left.y.to_bits() == right.y.to_bits()
                        && left.z.to_bits() == right.z.to_bits()
                })
        })
}

fn selection_for_member_points(
    members: &[&DesignExtrudeSelectionMember],
    sketch: &cadmpeg_ir::sketches::Sketch,
    entities: &[cadmpeg_ir::sketches::SketchEntity],
    member_points: &[Vec<Point3>],
    tolerance: f64,
    profile_work: &ProfileGeometryWork,
    ctx: &DecodeContext<'_>,
) -> Result<Option<ResolvedProfileSelection>, CodecError> {
    let mut all_points = Vec::new();
    for point in member_points.iter().flatten().copied() {
        all_points.push(point);
    }
    if let Some(selection) =
        selection_containing_points(sketch, entities, &all_points, tolerance, profile_work, ctx)?
    {
        return Ok(Some(selection));
    }
    let mut selections = Vec::new();
    for points in member_points {
        let selection =
            selection_containing_points(sketch, entities, points, tolerance, profile_work, ctx)?;
        selections.push(selection);
    }
    if ordered_selection_has_value(&selections) {
        return Ok(ordered_unique_profile_selections(selections));
    }
    Ok(region_with_boundary_selection_members(
        members,
        sketch,
        &selections,
    ))
}

fn region_with_boundary_selection_members(
    members: &[&DesignExtrudeSelectionMember],
    sketch: &cadmpeg_ir::sketches::Sketch,
    selections: &[Option<ResolvedProfileSelection>],
) -> Option<ResolvedProfileSelection> {
    use cadmpeg_ir::features::SketchProfileRegion;

    let mut regions = selections.iter().filter_map(|selection| match selection {
        Some(ResolvedProfileSelection::Regions(regions)) => Some(regions.as_slice()),
        _ => None,
    });
    let Some([region]) = regions.next() else {
        return None;
    };
    let SketchProfileRegion::Loops { loops } = region else {
        return None;
    };
    if regions.any(|candidate| candidate != std::slice::from_ref(region)) {
        return None;
    }
    for (member, selection) in members.iter().zip(selections) {
        let matches = match selection {
            Some(ResolvedProfileSelection::Regions(candidate)) => {
                candidate == std::slice::from_ref(region)
            }
            Some(ResolvedProfileSelection::Loops(selected)) => {
                !selected.is_empty()
                    && selected
                        .iter()
                        .all(|profile| *profile == loops.outer() || loops.holes().contains(profile))
            }
            None => resolved_selection_member_profiles(member, sketch).is_some_and(|profiles| {
                !profiles.is_empty()
                    && profiles
                        .iter()
                        .all(|profile| *profile == loops.outer() || loops.holes().contains(profile))
            }),
        };
        if !matches {
            return None;
        }
    }
    let mut owned_holes = Vec::new();
    for hole in loops.holes().iter().copied() {
        owned_holes.push(hole);
    }
    let region = SketchProfileRegion::loops(loops.outer(), owned_holes).ok()?;
    let selected = vec![region];
    Some(ResolvedProfileSelection::Regions(selected))
}

fn resolved_selection_member_profiles(
    member: &DesignExtrudeSelectionMember,
    sketch: &cadmpeg_ir::sketches::Sketch,
) -> Option<Vec<u32>> {
    let geometry = member.resolved_geometry.as_ref()?;
    let SketchRelationOperand::Curve {
        primary_id,
        secondary_id,
        ..
    } = geometry
    else {
        return None;
    };
    let entity = crate::ids::neutral_sketch_curve_id(&sketch.id, *primary_id, *secondary_id);
    let mut profiles = Vec::new();
    for (index, profile) in sketch.profiles.iter().enumerate() {
        if profile.iter().any(|use_| use_.entity == entity) {
            let Ok(index) = u32::try_from(index) else {
                return None;
            };
            profiles.push(index);
        }
    }
    Some(profiles)
}

fn resolved_selection_member_point(
    member: &DesignExtrudeSelectionMember,
    sketch: &cadmpeg_ir::sketches::Sketch,
    entities: &[cadmpeg_ir::sketches::SketchEntity],
) -> Option<Point3> {
    use cadmpeg_ir::sketches::SketchGeometryDefinition;

    let SketchRelationOperand::Point {
        record_index,
        persistent_id,
    } = member.resolved_geometry.as_ref()?
    else {
        return None;
    };
    let entity_id = persistent_id.map_or_else(
        || crate::ids::neutral_sketch_record_id(&sketch.id, *record_index),
        |persistent_id| crate::ids::neutral_sketch_point_id(&sketch.id, persistent_id),
    );
    let SketchGeometryDefinition::Point { position } = entities
        .iter()
        .find(|entity| entity.id() == &entity_id && entity.sketch == sketch.id)?
        .geometry
        .definition()
    else {
        return None;
    };
    let (origin, normal, u_axis) = sketch.resolved_placement()?;
    let v_axis = normal.cross(u_axis.get());
    Some(
        origin
            .translated(u_axis.get(), position.u)
            .translated(v_axis, position.v),
    )
}

fn ordered_selection_has_value(selections: &[Option<ResolvedProfileSelection>]) -> bool {
    let mut has_loops = false;
    let mut has_regions = false;
    for selection in selections {
        match selection {
            Some(ResolvedProfileSelection::Loops(selected)) if !has_regions => {
                has_loops |= !selected.is_empty();
            }
            Some(ResolvedProfileSelection::Regions(selected)) if !has_loops => {
                has_regions |= !selected.is_empty();
            }
            _ => return false,
        }
    }
    has_loops || has_regions
}

fn ordered_unique_profile_selections(
    matches: impl IntoIterator<Item = Option<ResolvedProfileSelection>>,
) -> Option<ResolvedProfileSelection> {
    let mut loops = Vec::new();
    let mut regions = Vec::new();
    for selection in matches {
        let selection = selection?;
        match selection {
            ResolvedProfileSelection::Loops(selected) if regions.is_empty() => {
                for loop_index in selected {
                    if !loops.contains(&loop_index) {
                        loops.push(loop_index);
                    }
                }
            }
            ResolvedProfileSelection::Regions(selected) if loops.is_empty() => {
                for region in selected {
                    if !regions.contains(&region) {
                        regions.push(region);
                    }
                }
            }
            _ => return None,
        }
    }
    if !loops.is_empty() {
        Some(ResolvedProfileSelection::Loops(loops))
    } else if !regions.is_empty() {
        Some(ResolvedProfileSelection::Regions(regions))
    } else {
        None
    }
}

pub(in crate::design) fn selection_containing_points(
    sketch: &cadmpeg_ir::sketches::Sketch,
    entities: &[cadmpeg_ir::sketches::SketchEntity],
    points: &[Point3],
    tolerance: f64,
    profile_work: &ProfileGeometryWork,
    ctx: &DecodeContext<'_>,
) -> Result<Option<ResolvedProfileSelection>, CodecError> {
    let mut projected = Vec::new();
    let mut unique_projected: Vec<Point2> = Vec::new();
    for point in points {
        let Some(point) = project_to_sketch(sketch, *point) else {
            return Ok(None);
        };
        projected.push(point);
        if !unique_projected
            .iter()
            .any(|p| (p.u - point.u).hypot(p.v - point.v) <= tolerance)
        {
            unique_projected.push(point);
        }
    }
    let mut boundaries = Vec::new();
    let mut had_partial_boundary_match = false;
    for (index, profile) in sketch.profiles.iter().enumerate() {
        let mut matches = true;
        for point in &projected {
            let mut on_boundary = false;
            for use_ in profile {
                let Some(entity) = entities.iter().find(|entity| entity.id() == &use_.entity)
                else {
                    continue;
                };
                if point_on_sketch_entity(*point, entity, tolerance) {
                    on_boundary = true;
                    break;
                }
            }
            if !on_boundary {
                matches = false;
                break;
            }
        }
        if matches && points.len() >= 2 && unique_projected.len() == 1 {
            let p0 = unique_projected[0];
            let on_curve = profile.iter().any(|use_| {
                entities
                    .iter()
                    .find(|entity| entity.id() == &use_.entity)
                    .is_some_and(|entity| {
                        matches!(
                            entity.geometry.definition(),
                            cadmpeg_ir::sketches::SketchGeometryDefinition::Circle { .. }
                                | cadmpeg_ir::sketches::SketchGeometryDefinition::Arc { .. }
                        ) && point_on_sketch_entity(p0, entity, tolerance)
                    })
            });
            if !on_curve {
                matches = false;
                had_partial_boundary_match = true;
            }
        } else if matches && unique_projected.len() == 2 {
            let p0 = unique_projected[0];
            let p1 = unique_projected[1];
            let on_single_entity = profile.iter().any(|use_| {
                entities
                    .iter()
                    .find(|entity| entity.id() == &use_.entity)
                    .is_some_and(|entity| {
                        let on_both = point_on_sketch_entity(p0, entity, tolerance)
                            && point_on_sketch_entity(p1, entity, tolerance);
                        if !on_both {
                            return false;
                        }
                        match entity.geometry.definition() {
                            cadmpeg_ir::sketches::SketchGeometryDefinition::Line { start, end } => {
                                let seg_len = (p0.u - p1.u).hypot(p0.v - p1.v);
                                let line_len = (start.u - end.u).hypot(start.v - end.v);
                                seg_len >= 0.5 * line_len
                            }
                            _ => true,
                        }
                    })
            });
            if !on_single_entity {
                matches = false;
                had_partial_boundary_match = true;
            }
        } else if matches && unique_projected.len() >= 3 && profile.len() >= 2 {
            let all_lines_covered = profile.iter().all(|use_| {
                entities
                    .iter()
                    .find(|entity| entity.id() == &use_.entity)
                    .is_some_and(|entity| {
                        if matches!(
                            entity.geometry.definition(),
                            cadmpeg_ir::sketches::SketchGeometryDefinition::Line { .. }
                        ) {
                            unique_projected
                                .iter()
                                .any(|&pt| point_on_sketch_entity(pt, entity, tolerance))
                        } else {
                            true
                        }
                    })
            });
            if !all_lines_covered {
                matches = false;
                had_partial_boundary_match = true;
            }
        }
        if matches {
            let Some(index) = u32::try_from(index).ok() else {
                return Ok(None);
            };
            boundaries.push(index);
        }
    }
    if boundaries.len() > 1 && unique_projected.len() == 2 {
        let circle_boundaries: Vec<u32> = boundaries
            .iter()
            .copied()
            .filter(|&idx| {
                usize::try_from(idx)
                    .ok()
                    .and_then(|i| sketch.profiles.get(i))
                    .is_some_and(|prof| {
                        prof.len() == 1
                            && prof.iter().all(|use_| {
                                entities
                                    .iter()
                                    .find(|e| e.id() == &use_.entity)
                                    .is_some_and(|e| {
                                        matches!(
                                            e.geometry.definition(),
                                            cadmpeg_ir::sketches::SketchGeometryDefinition::Circle { .. }
                                        )
                                    })
                            })
                    })
            })
            .collect();
        if !circle_boundaries.is_empty() {
            boundaries = circle_boundaries;
        }
    }
    if let [profile] = boundaries.as_slice() {
        let loops = vec![*profile];
        return Ok(Some(ResolvedProfileSelection::Loops(loops)));
    }
    if !boundaries.is_empty() && unique_projected.len() <= 2 {
        return Ok(None);
    }
    if boundaries.is_empty() && had_partial_boundary_match {
        return Ok(None);
    }
    if let Some(region) = arrangement_region_containing_points(
        sketch,
        entities,
        &projected,
        tolerance,
        profile_work,
        ctx,
    )? {
        let regions = vec![region];
        return Ok(Some(ResolvedProfileSelection::Regions(regions)));
    }
    if !boundaries.is_empty() {
        return Ok(None);
    }
    let Some(region) =
        region_containing_points(sketch, entities, points, tolerance, profile_work, ctx)?
    else {
        return Ok(None);
    };
    let regions = vec![region];
    Ok(Some(ResolvedProfileSelection::Regions(regions)))
}

/// Solved sketch records used to bind Loft and Revolve profile operands and
/// Loft guide selections.
pub(crate) struct SketchProfileResolution<'a> {
    pub(crate) entities: &'a [DesignEntityHeader],
    pub(crate) entity_selection_operands: &'a [DesignEntitySelectionOperand],
    pub(crate) placements: &'a [DesignSketchPlacement],
    pub(crate) curve_identities: &'a [SketchCurveIdentity],
    pub(crate) sketches: &'a [cadmpeg_ir::sketches::Sketch],
    pub(crate) sketch_entities: &'a [cadmpeg_ir::sketches::SketchEntity],
    pub(crate) spatial_sketches: &'a [cadmpeg_ir::sketches::SpatialSketch],
    pub(crate) spatial_sketch_entities: &'a [cadmpeg_ir::sketches::SpatialSketchEntity],
    pub(crate) linear_tolerance: f64,
    pub(crate) angular_tolerance: f64,
}

impl<'a> SketchProfileResolution<'a> {
    fn path_resolution(&self) -> EntitySelectionPathResolution<'a> {
        EntitySelectionPathResolution {
            operands: self.entity_selection_operands,
            profile_curves: &[],
            placements: self.placements,
            curve_identities: self.curve_identities,
            sketches: self.sketches,
            sketch_entities: self.sketch_entities,
            spatial_sketches: self.spatial_sketches,
            spatial_sketch_entities: self.spatial_sketch_entities,
        }
    }
}

fn resolve_profile_curve_regions(
    group: &DesignConstructionOperandGroup,
    resolution: &EntitySelectionPathResolution<'_>,
) -> Option<cadmpeg_ir::features::ProfileRef> {
    use cadmpeg_ir::features::{PlanarProfileRef, ProfileRef, SketchProfileRegion};
    let stream = native_stream(&group.id)?;
    let (sketch, curves) = profile_curve_selection_curves(group, stream, resolution)?;
    let source = resolution
        .sketches
        .iter()
        .find(|source| source.id == sketch)?;
    let mut regions = Vec::new();
    let mut curve_cursor = 0usize;
    for (ordinal, record_index) in group.members().iter().enumerate() {
        let selection = resolution.profile_curves.iter().find(|selection| {
            native_stream(&selection.id) == Some(stream)
                && selection.scope_record_index == group.scope_record_index
                && selection.group_record_index == group.record_index
                && usize::try_from(selection.group_member_ordinal).ok() == Some(ordinal)
                && selection.record_index == record_index.value
        })?;
        if !selection.valid_layout() {
            return None;
        }
        for loops in selection.loop_lists.iter().flatten() {
            let mut outer = None;
            let mut holes = Vec::new();
            let mut used = HashSet::new();
            for loop_ in loops {
                let end = curve_cursor.checked_add(loop_.entries.len())?;
                let selected = curves.get(curve_cursor..end)?;
                curve_cursor = end;
                let mut distinct = HashSet::new();
                if selected.iter().any(|curve| !distinct.insert(curve)) {
                    return None;
                }
                let mut matches = source.profiles.iter().enumerate().filter(|(_, profile)| {
                    profile.len() == selected.len()
                        && profile.iter().all(|use_| selected.contains(&use_.entity))
                });
                let (index, _) = matches.next()?;
                if matches.next().is_some() {
                    return None;
                }
                let index = u32::try_from(index).ok()?;
                if !used.insert(index) {
                    return None;
                }
                if loop_.outer {
                    if outer.replace(index).is_some() {
                        return None;
                    }
                } else {
                    holes.push(index);
                }
            }
            let region = SketchProfileRegion::loops(outer?, holes).ok()?;
            if !regions.contains(&region) {
                regions.push(region);
            }
        }
    }
    if curve_cursor != curves.len() {
        return None;
    }
    PlanarProfileRef::sketch_regions(sketch, regions)
        .ok()
        .map(ProfileRef::Planar)
}

fn resolve_profile_curve_arrangement_regions(
    group: &DesignConstructionOperandGroup,
    resolution: &EntitySelectionPathResolution<'_>,
    scoped: ScopedExtrudeProfileResolution<'_>,
) -> Result<Option<cadmpeg_ir::features::ProfileRef>, CodecError> {
    use cadmpeg_ir::features::{PlanarProfileRef, ProfileRef};
    let Some(stream) = native_stream(&group.id) else {
        return Ok(None);
    };
    let Some((sketch, curves)) = profile_curve_selection_curves(group, stream, resolution) else {
        return Ok(None);
    };
    let Some(source) = resolution
        .sketches
        .iter()
        .find(|source| source.id == sketch)
    else {
        return Ok(None);
    };
    let mut regions = Vec::new();
    let mut cursor = 0usize;
    for (ordinal, member) in group.members().iter().enumerate() {
        let Some(selection) = resolution.profile_curves.iter().find(|selection| {
            native_stream(&selection.id) == Some(stream)
                && selection.scope_record_index == group.scope_record_index
                && selection.group_record_index == group.record_index
                && usize::try_from(selection.group_member_ordinal).ok() == Some(ordinal)
                && selection.record_index == member.value
        }) else {
            return Ok(None);
        };
        if !selection.valid_layout() {
            return Ok(None);
        }
        for loops in selection.loop_lists.iter().flatten() {
            let [loop_] = loops.as_slice() else {
                return Ok(None);
            };
            if !loop_.outer {
                return Ok(None);
            }
            let Some(end) = cursor.checked_add(loop_.entries.len()) else {
                return Ok(None);
            };
            let Some(program) = curves.get(cursor..end) else {
                return Ok(None);
            };
            cursor = end;
            let Some(region) = crate::design::geometry::arrangement_region_matching_curve_program(
                source,
                resolution.sketch_entities,
                program,
                scoped.linear_tolerance,
                scoped.profile_work,
                scoped.ctx,
            )?
            else {
                return Ok(None);
            };
            if !regions.contains(&region) {
                regions.push(region);
            }
        }
    }
    if cursor != curves.len() {
        return Ok(None);
    }
    Ok(PlanarProfileRef::sketch_regions(sketch, regions)
        .ok()
        .map(ProfileRef::Planar))
}

fn resolve_profile_curve_path(
    group: &DesignConstructionOperandGroup,
    stream: &str,
    resolution: &EntitySelectionPathResolution<'_>,
) -> Option<cadmpeg_ir::features::PathRef> {
    if resolution
        .profile_curves
        .iter()
        .filter(|selection| {
            native_stream(&selection.id) == Some(stream)
                && selection.scope_record_index == group.scope_record_index
                && selection.group_record_index == group.record_index
        })
        .flat_map(|selection| selection.loop_lists.iter().flatten())
        .any(|loops| !matches!(loops.as_slice(), [loop_] if loop_.outer))
    {
        return None;
    }
    let (sketch, curves) = profile_curve_selection_curves(group, stream, resolution)?;
    let mut seen = HashSet::new();
    if curves.iter().any(|curve| !seen.insert(curve)) {
        return None;
    }
    cadmpeg_ir::features::PathRef::sketch_curves(sketch, curves).ok()
}

fn profile_curve_selection_curves(
    group: &DesignConstructionOperandGroup,
    stream: &str,
    resolution: &EntitySelectionPathResolution<'_>,
) -> Option<(
    cadmpeg_ir::sketches::SketchId,
    Vec<cadmpeg_ir::sketches::SketchEntityId>,
)> {
    use cadmpeg_ir::sketches::SketchGeometryDefinition;

    macro_rules! available {
        ($value:expr) => {
            match $value {
                Some(value) => value,
                None => return None,
            }
        };
    }
    let mut sketch_scope = None;
    let mut owner_text = None;
    let mut selections = Vec::new();
    for (ordinal, record_index) in group
        .members()
        .iter()
        .map(|member| member.value)
        .enumerate()
    {
        let ordinal = available!(u32::try_from(ordinal).ok());
        let mut matches = resolution.profile_curves.iter().filter(|selection| {
            native_stream(&selection.id) == Some(stream)
                && selection.scope_record_index == group.scope_record_index
                && selection.group_record_index == group.record_index
                && selection.group_member_ordinal == ordinal
                && selection.record_index == record_index
        });
        let selection = available!(matches.next());
        if !selection.valid_layout()
            || matches.next().is_some()
            || sketch_scope.is_some_and(|scope| scope != selection.sketch_scope_record_index)
            || owner_text.is_some_and(|text: &str| text != selection.owner_text)
        {
            return None;
        }
        sketch_scope = Some(selection.sketch_scope_record_index);
        owner_text = Some(selection.owner_text.as_str());
        selections.push(selection);
    }
    let sketch_scope = available!(sketch_scope);
    let mut matching_placements = resolution.placements.iter().filter(|placement| {
        native_stream(&placement.id) == Some(stream)
            && placement.scope_record_index == Some(sketch_scope)
    });
    let placement = available!(matching_placements.next());
    if matching_placements.next().is_some()
        || owner_text != Some(placement.entity_id.suffix().to_string().as_str())
    {
        return None;
    }
    let owner_reference = available!(u32::try_from(placement.entity_id.suffix()).ok());
    let sketch = crate::ids::neutral_sketch_id(placement);
    if !resolution
        .sketches
        .iter()
        .any(|candidate| candidate.id == sketch)
    {
        return None;
    }
    let mut curves = Vec::new();
    for entry in selections.iter().flat_map(|selection| {
        selection
            .loop_lists
            .iter()
            .flatten()
            .flatten()
            .flat_map(|loop_| &loop_.entries)
    }) {
        let mut matches = resolution.curve_identities.iter().filter(|curve| {
            native_stream(&curve.id) == Some(stream)
                && curve.owner_reference == Some(owner_reference)
                && curve.primary_id.get() == entry.primary_id
                && (entry.secondary_id == 0 || curve.secondary_id == entry.secondary_id)
        });
        let curve = available!(matches.next());
        if matches.next().is_some() {
            return None;
        }
        let identity = (curve.primary_id.get(), curve.secondary_id);
        curves.push(crate::ids::neutral_sketch_curve_id(
            &sketch, identity.0, identity.1,
        ));
    }
    if curves.iter().any(|curve| {
        !resolution.sketch_entities.iter().any(|entity| {
            entity.sketch == sketch
                && entity.id() == curve
                && !matches!(
                    *entity.geometry.definition(),
                    SketchGeometryDefinition::Point { .. }
                )
        })
    }) {
        return None;
    }
    Some((sketch, curves))
}

/// Resolve one complete ordered entity-selection group to a neutral Sketch
/// path. The source identity pair is only meaningful with its owning Sketch
/// identity. Require one-to-one record ownership, a shared selection
/// namespace, a unique placement, and a unique neutral curve for every member
/// before exposing the path as typed geometry.
fn resolve_entity_selection_path(
    group: &DesignConstructionOperandGroup,
    resolution: &EntitySelectionPathResolution<'_>,
) -> Option<cadmpeg_ir::features::PathRef> {
    use cadmpeg_ir::features::PathRef;
    use cadmpeg_ir::sketches::SketchGeometryDefinition;

    macro_rules! available {
        ($value:expr) => {
            match $value {
                Some(value) => value,
                None => return None,
            }
        };
    }
    if group.members().is_empty() {
        return None;
    }
    let stream = available!(native_stream(&group.id));
    if resolution.profile_curves.iter().any(|selection| {
        native_stream(&selection.id) == Some(stream)
            && selection.scope_record_index == group.scope_record_index
            && selection.group_record_index == group.record_index
    }) {
        return resolve_profile_curve_path(group, stream, resolution);
    }
    let mut selected_identities = Vec::new();
    let mut member_records = HashSet::new();
    let mut primary_identity = None;
    let mut asset_id = None;
    let mut context_id = None;
    for (ordinal, record_index) in group
        .members()
        .iter()
        .map(|member| member.value)
        .enumerate()
    {
        let ordinal = available!(u32::try_from(ordinal).ok());
        if !member_records.insert(record_index) {
            return None;
        }
        let mut matches = resolution.operands.iter().filter(|operand| {
            native_stream(&operand.id) == Some(stream)
                && operand.scope_record_index == group.scope_record_index
                && operand.group_record_index == group.record_index
                && operand.group_member_ordinal == ordinal
                && operand.record_index() == record_index
        });
        let operand = available!(matches.next());
        if matches.next().is_some() {
            return None;
        }
        let secondary = available!(operand.secondary());
        if let Some(expected) = primary_identity {
            if expected != operand.primary_identity {
                return None;
            }
        } else {
            primary_identity = Some(operand.primary_identity);
        }
        if let Some(expected) = asset_id {
            if expected != operand.asset_id.as_str() {
                return None;
            }
        } else {
            asset_id = Some(operand.asset_id.as_str());
        }
        if let Some(expected) = context_id {
            if expected != operand.context_id.as_str() {
                return None;
            }
        } else {
            context_id = Some(operand.context_id.as_str());
        }
        selected_identities.push(secondary);
    }
    let primary_identity = available!(primary_identity);
    let mut matching_placements = resolution.placements.iter().filter(|placement| {
        native_stream(&placement.id) == Some(stream)
            && placement.entity_id.suffix() == primary_identity
    });
    let placement = available!(matching_placements.next());
    if matching_placements.next().is_some() {
        return None;
    }

    let mut curve_ids = Vec::new();
    let mut selected_curve_identities = HashSet::new();
    let owner_reference = available!(u32::try_from(primary_identity).ok());
    for secondary in &selected_identities {
        let secondary_identity = secondary.identity.value;
        let mut curves = resolution.curve_identities.iter().filter(|curve| {
            native_stream(&curve.id) == Some(stream)
                && curve.owner_reference == Some(owner_reference)
                && curve.primary_id.get() == secondary_identity
                && secondary
                    .curve_identity
                    .is_none_or(|identity| curve.secondary_id == identity.value)
        });
        let curve = available!(curves.next());
        if curves.next().is_some() {
            return None;
        }
        let identity = (curve.primary_id.get(), curve.secondary_id);
        if !selected_curve_identities.insert(identity) {
            return None;
        }
        curve_ids.push(identity);
    }

    let spatial_sketch = crate::ids::neutral_spatial_sketch_id(placement);
    if resolution
        .spatial_sketches
        .iter()
        .any(|sketch| sketch.id == spatial_sketch)
    {
        let mut selections = HashSet::new();
        for (primary, secondary) in &curve_ids {
            let id =
                crate::ids::neutral_spatial_sketch_curve_id(&spatial_sketch, *primary, *secondary);
            selections.insert(id);
        }
        if selections.len() != curve_ids.len()
            || selections.iter().any(|curve| {
                !resolution
                    .spatial_sketch_entities
                    .iter()
                    .any(|entity| entity.sketch == spatial_sketch && entity.id() == curve)
            })
        {
            return None;
        }
        let mut curves = Vec::new();
        for (primary, secondary) in curve_ids {
            let id =
                crate::ids::neutral_spatial_sketch_curve_id(&spatial_sketch, primary, secondary);
            curves.push(id);
        }
        let sketch = spatial_sketch.clone();
        return PathRef::spatial_sketch_curves(sketch, curves).ok();
    }

    let sketch = crate::ids::neutral_sketch_id(placement);
    if !resolution
        .sketches
        .iter()
        .any(|candidate| candidate.id == sketch)
    {
        return None;
    }
    let mut curves = Vec::new();
    for (primary, secondary) in curve_ids {
        let id = crate::ids::neutral_sketch_curve_id(&sketch, primary, secondary);
        curves.push(id);
    }
    if curves.iter().any(|curve| {
        !resolution.sketch_entities.iter().any(|entity| {
            entity.sketch == sketch
                && entity.id() == curve
                && !matches!(
                    *entity.geometry.definition(),
                    SketchGeometryDefinition::Point { .. }
                )
        })
    }) {
        return None;
    }
    PathRef::sketch_curves(sketch, curves).ok()
}

/// Resolve one ordered Loft guide or centerline group whose members select
/// curves from a Sketch.
fn resolved_loft_entity_selection_path(
    group: &DesignConstructionOperandGroup,
    resolution: &SketchProfileResolution<'_>,
) -> Option<cadmpeg_ir::features::PathRef> {
    if !matches!(
        group.role(),
        DesignOperandRole::ROLE_0X5 | DesignOperandRole::ROLE_0X7
    ) {
        return None;
    }
    let path_resolution = resolution.path_resolution();
    resolve_entity_selection_path(group, &path_resolution)
}

fn spatial_profile_member_entity<'a>(
    stream: &str,
    owner_reference: u32,
    member: &DesignSketchProfileRegionMember,
    spatial_sketch: &cadmpeg_ir::sketches::SpatialSketch,
    curve_identities: &[SketchCurveIdentity],
    spatial_entities: &'a [cadmpeg_ir::sketches::SpatialSketchEntity],
) -> Option<&'a cadmpeg_ir::sketches::SpatialSketchEntity> {
    let mut curves = curve_identities.iter().filter(|curve| {
        native_stream(&curve.id) == Some(stream)
            && curve.owner_reference == Some(owner_reference)
            && curve.primary_id.get() == u64::from(member.curve_primary_id.get())
    });
    let curve = curves.next()?;
    if curves.next().is_some() {
        return None;
    }
    let entity_id = crate::ids::neutral_spatial_sketch_curve_id(
        &spatial_sketch.id,
        curve.primary_id.get(),
        curve.secondary_id,
    );
    let mut entities = spatial_entities
        .iter()
        .filter(|entity| entity.sketch == spatial_sketch.id && entity.id() == &entity_id);
    let entity = entities.next()?;
    entities.next().is_none().then_some(entity)
}

fn sketch_profile_member_entity<'a>(
    stream: &str,
    owner_reference: u32,
    member: &DesignSketchProfileRegionMember,
    sketch: &cadmpeg_ir::sketches::Sketch,
    curve_identities: &[SketchCurveIdentity],
    sketch_entities: &'a [cadmpeg_ir::sketches::SketchEntity],
) -> Option<&'a cadmpeg_ir::sketches::SketchEntityId> {
    let mut curves = curve_identities.iter().filter(|curve| {
        native_stream(&curve.id) == Some(stream)
            && curve.owner_reference == Some(owner_reference)
            && curve.primary_id.get() == u64::from(member.curve_primary_id.get())
    });
    let curve = curves.next()?;
    if curves.next().is_some() {
        return None;
    }
    let entity_id =
        crate::ids::neutral_sketch_curve_id(&sketch.id, curve.primary_id.get(), curve.secondary_id);
    let mut entities = sketch_entities
        .iter()
        .filter(|entity| entity.sketch == sketch.id && entity.id() == &entity_id);
    let entity = entities.next()?;
    if entities.next().is_some() {
        return None;
    }
    Some(entity.id())
}

fn resolved_sketch_profile_regions(
    stream: &str,
    profile: &DesignSketchProfileOperand,
    sketch: &cadmpeg_ir::sketches::Sketch,
    curve_identities: &[SketchCurveIdentity],
    sketch_entities: &[cadmpeg_ir::sketches::SketchEntity],
) -> Option<Vec<u32>> {
    let selection = profile.region_selection.as_ref()?;
    if selection.has_extended_incidence() {
        return None;
    }
    let Ok(owner_reference) = u32::try_from(profile.entity_id.suffix()) else {
        return None;
    };
    let mut resolved = Vec::new();
    for region in &selection.regions {
        let first_member = region.members.first()?;
        let first = sketch_profile_member_entity(
            stream,
            owner_reference,
            first_member,
            sketch,
            curve_identities,
            sketch_entities,
        )?;
        let mut matching_profiles = sketch
            .profiles
            .iter()
            .enumerate()
            .filter(|(_, profile)| profile.iter().any(|use_| &use_.entity == first));
        let (profile_index, selected_profile) = matching_profiles.next()?;
        if matching_profiles.next().is_some() {
            return None;
        }
        for member in &region.members[1..] {
            let entity = sketch_profile_member_entity(
                stream,
                owner_reference,
                member,
                sketch,
                curve_identities,
                sketch_entities,
            )?;
            if !selected_profile.iter().any(|use_| &use_.entity == entity) {
                return None;
            }
        }
        let Ok(profile_index) = u32::try_from(profile_index) else {
            return None;
        };
        if resolved.contains(&profile_index) {
            return None;
        }
        resolved.push(profile_index);
    }
    (!resolved.is_empty()).then_some(resolved)
}

fn coincident_spatial_profile_geometry(
    first: &cadmpeg_ir::sketches::SpatialSketchGeometry,
    second: &cadmpeg_ir::sketches::SpatialSketchGeometry,
    linear_tolerance: f64,
    angular_tolerance: f64,
) -> bool {
    use cadmpeg_ir::sketches::SpatialSketchGeometryDefinition;

    let (
        SpatialSketchGeometryDefinition::Circle {
            center: first_center,
            normal: first_normal,
            radius: first_radius,
            ..
        },
        SpatialSketchGeometryDefinition::Circle {
            center: second_center,
            normal: second_normal,
            radius: second_radius,
            ..
        },
    ) = (first.definition(), second.definition())
    else {
        return false;
    };
    if !linear_tolerance.is_finite()
        || linear_tolerance < 0.0
        || !angular_tolerance.is_finite()
        || angular_tolerance < 0.0
    {
        return false;
    }
    let center_delta = Vector3::new(
        first_center.x - second_center.x,
        first_center.y - second_center.y,
        first_center.z - second_center.z,
    );
    let first_normal = *first_normal.to_unit_length_charted().as_raw();
    let second_normal = *second_normal.to_unit_length_charted().as_raw();
    let normal_angle = first_normal
        .cross(second_normal)
        .norm()
        .atan2(first_normal.dot(second_normal).abs());
    center_delta.norm() <= linear_tolerance
        && (first_radius.get() - second_radius.get()).abs() <= linear_tolerance
        && normal_angle <= angular_tolerance
}

fn resolved_spatial_sketch_profile_regions(
    stream: &str,
    profile: &DesignSketchProfileOperand,
    spatial_sketch: &cadmpeg_ir::sketches::SpatialSketch,
    resolution: &SketchProfileResolution<'_>,
) -> Option<Vec<u32>> {
    let Some(selection) = profile.region_selection.as_ref() else {
        if spatial_sketch.profiles.is_empty() {
            return None;
        }
        let mut profiles = Vec::new();
        for index in 0..spatial_sketch.profiles.len() {
            let Ok(index) = u32::try_from(index) else {
                return None;
            };
            profiles.push(index);
        }
        return Some(profiles);
    };
    if selection.has_extended_incidence() {
        return None;
    }
    let Ok(owner_reference) = u32::try_from(profile.entity_id.suffix()) else {
        return None;
    };
    let mut resolved = Vec::new();
    for region in &selection.regions {
        let first_member = region.members.first()?;
        let first = spatial_profile_member_entity(
            stream,
            owner_reference,
            first_member,
            spatial_sketch,
            resolution.curve_identities,
            resolution.spatial_sketch_entities,
        )?;
        let mut matching_profiles =
            spatial_sketch
                .profiles
                .iter()
                .enumerate()
                .filter(|(_, candidate)| {
                    candidate
                        .boundary()
                        .iter()
                        .any(|use_| &use_.entity == first.id())
                });
        let (profile_index, selected_profile) = matching_profiles.next()?;
        if matching_profiles.next().is_some() {
            return None;
        }
        for member in &region.members[1..] {
            let entity = spatial_profile_member_entity(
                stream,
                owner_reference,
                member,
                spatial_sketch,
                resolution.curve_identities,
                resolution.spatial_sketch_entities,
            )?;
            if selected_profile
                .boundary()
                .iter()
                .any(|use_| &use_.entity == entity.id())
            {
                continue;
            }
            let coincident = selected_profile.boundary().iter().any(|use_| {
                resolution
                    .spatial_sketch_entities
                    .iter()
                    .find(|candidate| {
                        candidate.sketch == spatial_sketch.id && candidate.id() == &use_.entity
                    })
                    .is_some_and(|candidate| {
                        coincident_spatial_profile_geometry(
                            &candidate.geometry,
                            &entity.geometry,
                            resolution.linear_tolerance,
                            resolution.angular_tolerance,
                        )
                    })
            });
            if !coincident {
                return None;
            }
        }
        let Ok(profile_index) = u32::try_from(profile_index) else {
            return None;
        };
        if resolved.contains(&profile_index) {
            return None;
        }
        resolved.push(profile_index);
    }
    (!resolved.is_empty()).then_some(resolved)
}

fn spatial_profile_containing_entity(
    sketch: &cadmpeg_ir::sketches::SpatialSketch,
    entity: &cadmpeg_ir::sketches::SpatialSketchEntityId,
) -> Option<u32> {
    let mut profiles = sketch
        .profiles
        .iter()
        .enumerate()
        .filter(|(_, profile)| profile.boundary().iter().any(|use_| use_.entity == *entity));
    let (index, _) = profiles.next()?;
    if profiles.next().is_some() {
        return None;
    }
    u32::try_from(index).ok()
}

pub(crate) fn bind_loft_and_revolve_sketch_selections(
    scan: &ContainerScan,
    groups: &[DesignConstructionOperandGroup],
    headers: &[DesignRecordHeader],
    resolution: &SketchProfileResolution<'_>,
    features: &mut [cadmpeg_ir::features::Feature],
) -> Result<(), CodecError> {
    use cadmpeg_ir::features::{
        FeatureDefinition, FeatureOperation, LoftSection, PathRef, PlanarProfileRef, ProfileRef,
    };

    let mut header_index = HashMap::new();
    for header in headers {
        let Some(stream) = native_stream(&header.id) else {
            continue;
        };
        header_index.insert((stream, header.record_index), header);
    }
    let mut resolved_profiles = HashMap::new();
    for group in groups.iter().filter(|group| {
        matches!(
            group.role(),
            DesignOperandRole::PROFILE | DesignOperandRole::ROLE_0X43
        ) && group.members().len() == 1
    }) {
        let Some(stream) = native_stream(&group.id) else {
            continue;
        };
        let Some(entry) = scan.design_stream_entry_for_scope(ContainerRole::Bulkstream, stream)
        else {
            continue;
        };
        let bytes = scan.entry_bytes(&entry.name)?;
        let Some(header) = header_index.get(&(stream, group.members()[0].value)) else {
            continue;
        };
        let Some(profile) = parse_sketch_profile(
            bytes,
            stream,
            group.scope_reference_ordinal,
            header,
            resolution.entities,
        ) else {
            continue;
        };
        let mut matches = resolution.placements.iter().filter(|placement| {
            native_stream(&placement.id) == Some(stream) && placement.entity_id == profile.entity_id
        });
        let Some(placement) = matches.next() else {
            continue;
        };
        if matches.next().is_some() {
            continue;
        }
        let spatial_sketch_id = crate::ids::neutral_spatial_sketch_id(placement);
        let resolved = if let Some(spatial_sketch) = resolution
            .spatial_sketches
            .iter()
            .find(|sketch| sketch.id == spatial_sketch_id)
        {
            let id = spatial_sketch_id.clone();
            if let Some(profiles) = resolved_spatial_sketch_profile_regions(
                stream,
                &profile,
                spatial_sketch,
                resolution,
            ) {
                match ProfileRef::spatial_sketch_profiles(id, profiles) {
                    Ok(profile) => profile,
                    Err(_) => ProfileRef::Planar(PlanarProfileRef::Native(group.id.clone())),
                }
            } else {
                let group_id = group.id.clone();
                match ProfileRef::spatial_sketch_selection(id, vec![group_id]) {
                    Ok(profile) => profile,
                    Err(_) => ProfileRef::Planar(PlanarProfileRef::Native(group.id.clone())),
                }
            }
        } else {
            let sketch = crate::ids::neutral_sketch_id(placement);
            if !resolution
                .sketches
                .iter()
                .any(|candidate| candidate.id == sketch)
            {
                continue;
            }
            ProfileRef::Planar(PlanarProfileRef::Sketch(sketch))
        };
        resolved_profiles.insert(group.id.as_str(), resolved);
    }
    let mut resolved_entity_paths = HashMap::new();
    for group in groups.iter().filter(|group| {
        matches!(
            group.role(),
            DesignOperandRole::ROLE_0X5 | DesignOperandRole::ROLE_0X7
        ) && !group.members().is_empty()
    }) {
        if let Some(path) = resolved_loft_entity_selection_path(group, resolution) {
            resolved_entity_paths.insert(group.id.as_str(), path);
        }
    }
    for group in groups
        .iter()
        .filter(|group| group.role() == DesignOperandRole::ROLE_0X5 && group.members().len() == 1)
    {
        let Some(stream) = native_stream(&group.id) else {
            continue;
        };
        let mut operands = resolution
            .entity_selection_operands
            .iter()
            .filter(|operand| {
                native_stream(&operand.id) == Some(stream)
                    && operand.scope_record_index == group.scope_record_index
                    && operand.group_record_index == group.record_index
                    && operand.group_member_ordinal == 0
                    && operand.record_index() == group.members()[0].value
            });
        let Some(operand) = operands.next() else {
            continue;
        };
        if operands.next().is_some() {
            continue;
        }
        let mut matching_placements = resolution.placements.iter().filter(|placement| {
            native_stream(&placement.id) == Some(stream)
                && placement.entity_id.suffix() == operand.primary_identity
        });
        let Some(placement) = matching_placements.next() else {
            continue;
        };
        if matching_placements.next().is_some() {
            continue;
        }
        let spatial_sketch_id = crate::ids::neutral_spatial_sketch_id(placement);
        let Some(spatial_sketch) = resolution
            .spatial_sketches
            .iter()
            .find(|sketch| sketch.id == spatial_sketch_id)
        else {
            continue;
        };
        let Ok(owner_reference) = u32::try_from(operand.primary_identity) else {
            continue;
        };
        let mut geometry_matches = resolution.curve_identities.iter().filter(|curve| {
            native_stream(&curve.id) == Some(stream)
                && curve.owner_reference == Some(owner_reference)
                && entity_selection_matches_curve(operand, curve)
        });
        let Some(curve) = geometry_matches.next() else {
            continue;
        };
        if geometry_matches.next().is_some() {
            continue;
        }
        let entity = crate::ids::neutral_spatial_sketch_curve_id(
            &spatial_sketch_id,
            curve.primary_id.get(),
            curve.secondary_id,
        );
        let profile = spatial_profile_containing_entity(spatial_sketch, &entity);
        let id = spatial_sketch_id.clone();
        let resolved = if let Some(profile) = profile {
            ProfileRef::spatial_sketch_profiles(id, vec![profile])
        } else {
            let operand_id = operand.id.clone();
            ProfileRef::spatial_sketch_selection(id, vec![operand_id])
        };
        let resolved = match resolved {
            Ok(profile) => profile,
            Err(_) => ProfileRef::Planar(PlanarProfileRef::Native(group.id.clone())),
        };
        resolved_profiles.insert(group.id.as_str(), resolved);
    }
    for feature in features.iter_mut() {
        let mut edit_result = Ok(());
        feature.evaluation.edit(|definition, _| {
            edit_result = (|| -> Result<(), CodecError> {
                let FeatureDefinition::Operation(FeatureOperation::Loft {
                    sections, guidance, ..
                }) = definition
                else {
                    return Ok(());
                };
                for section in sections.iter_mut() {
                    let LoftSection::Profile(ProfileRef::Planar(PlanarProfileRef::Native(native))) =
                        section
                    else {
                        continue;
                    };
                    if let Some(profile) = resolved_profiles.get(native.as_str()) {
                        *section = LoftSection::Profile(copy_bound_profile(profile)?);
                    }
                }
                match guidance {
                    cadmpeg_ir::features::LoftGuidance::Guides(guides) => {
                        for guide in guides.iter_mut() {
                            let PathRef::Native(native) = guide else {
                                continue;
                            };
                            if let Some(path) = resolved_entity_paths.get(native.as_str()) {
                                *guide = copy_bound_path(path)?;
                            }
                        }
                    }
                    cadmpeg_ir::features::LoftGuidance::Centerline(centerline) => {
                        if let PathRef::Native(native) = centerline {
                            if let Some(path) = resolved_entity_paths.get(native.as_str()) {
                                *centerline = copy_bound_path(path)?;
                            }
                        }
                    }
                }
                Ok(())
            })();
        });
        edit_result?;
    }
    for feature in features.iter_mut() {
        let mut edit_result = Ok(());
        feature.evaluation.edit(|definition, _| {
            edit_result = (|| -> Result<(), CodecError> {
                let FeatureDefinition::Operation(FeatureOperation::Revolve {
                    construction, ..
                }) = definition
                else {
                    return Ok(());
                };
                let Some(PlanarProfileRef::Native(native)) = construction.profile() else {
                    return Ok(());
                };
                let Some(ProfileRef::Planar(profile)) = resolved_profiles.get(native.as_str())
                else {
                    return Ok(());
                };
                construction.set_profile(Some(copy_bound_planar_profile(profile)?));
                Ok(())
            })();
        });
        edit_result?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod numerical_range_tests;
