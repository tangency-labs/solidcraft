// SPDX-License-Identifier: Apache-2.0
//! Project parameter-design features and dispatch per feature family.

use cadmpeg_core::decode::u64_from_index;

use crate::design::decode::operands::entity_selection_matches_curve;
use crate::design::dimensions::expression_identifiers;
use crate::design::edge_resolve::{
    project_fixed_fillet_with_corners, resolved_edge_flange_group, resolved_edge_group,
    resolved_edge_treatment_group_with_corners, resolved_surface_patch_edge_group,
    EdgeGroupTransition,
};
use crate::design::face_resolve::{
    design_angle, extrude_omits_zero_side_one_offset, extrude_profile_group_roots,
    resolved_body_recipe_selection, resolved_body_recipe_shape, resolved_direct_face_selection,
    resolved_extrude_profile_face_group, resolved_face_group, resolved_historical_face_group,
    resolved_historical_face_operand,
    resolved_historical_split_face_target_group_with_updated_faces,
    resolved_loft_edge_profile_group, resolved_profile_face_group,
};

use crate::design::{design_feature_family, DesignFeatureFamily};
use crate::ids::{self, native_stream};
use crate::layout::coil_long_scope_fixed_prologue as coil_long;

use crate::records::{
    bodies::DesignBodyBinding,
    entity_header::DesignFeatureTimeline,
    feature::{
        coil::{DesignCoilExtent, DesignCoilSection, DesignCoilSectionPlacement},
        extrude::{
            DesignExtrudeExtent, DesignExtrudeOperation, DesignExtrudePrologue, DesignExtrudeStart,
        },
        fixed_parameters::DesignFixedExtrudeDistance,
        scope::DesignParameterScope,
        surface_ops::{
            DesignSurfaceOffsetOperation, DesignSurfaceOffsetSupport, DesignSurfaceTrimOperation,
        },
        work_geometry::DesignEdgeTreatmentVertexOperand,
    },
    parameters::{DesignParameter, DesignParameterKind, DesignParameterOwner},
    recipes::ConstructionRecipeKind,
    sketch_geometry::{SketchCurveGeometry, SketchCurveIdentity},
    sketch_placement::DesignSketchPlacement,
    topology::{
        body_recipe::DesignBodyRecipeOperand, construction::DesignConstructionOperandGroup,
        edge_identity::DesignEdgeIdentityOperand, edge_identity::DesignEdgeOperand,
        entity_selection::DesignLoftLegacyBodyCarrier, extrude_selection::DesignExtrudeFaceRole,
        extrude_selection::DesignExtrudeOperandRole, extrude_selection::DesignOperandRole,
        face::DesignFaceOperand, fillet::DesignFilletRadiusGroup, fillet::DesignFilletRadiusLaw,
    },
};
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::math::{Point3, Vector3};
use std::collections::{HashMap, HashSet};
use std::hash::Hash;

pub(crate) mod form_cages;

/// Millimetres per Design length unit (the Design stream stores centimetres).
const DESIGN_LENGTH_TO_MILLIMETRE: f64 = 10.0;
const EPS_FEATURE_PROJECT_PROJECT_OFFSET_FACES_E9: f64 = 1.0e-9;
const EPS_FEATURE_PROJECT_MATRIX_AXIS_ANGLE_E12: f64 = 1.0e-12;
const EPS_FEATURE_PROJECT_MATRIX_AXIS_ANGLE_E8: f64 = 1.0e-8;
const EPS_FEATURE_PROJECT_PROJECT_EXTRUDE_E9: f64 = 1.0e-9;
const EPS_FEATURE_PROJECT_PROJECT_EXTRUDE_E12: f64 = 1.0e-12;
/// Largest `1 - cos` between a rectangular-pattern direction record and the
/// instance-run direction that still names the same direction.
const EPS_RECTANGULAR_PATTERN_DIRECTION_AGREEMENT: f64 = 1.0e-9;

macro_rules! or_none {
    ($value:expr) => {
        match $value {
            Some(value) => value,
            None => return Ok(None),
        }
    };
}

fn unique_feature_match<T>(mut matches: impl Iterator<Item = T>) -> Option<T> {
    let first = matches.next()?;
    matches.next().is_none().then_some(first)
}

/// Design record slices projected together into the neutral construction
/// history: the parameter, owner, and scope tables plus the construction
/// operand, fillet-radius, edge, edge-identity, face, and whole-body recipe
/// operand records and the sketch placements and body bindings each feature
/// scope resolves against.
#[cfg_attr(test, derive(Default))]
pub(crate) struct ProjectInputs<'a> {
    pub(crate) native: &'a [DesignParameter],
    pub(crate) owners: &'a [DesignParameterOwner],
    pub(crate) scopes: &'a [DesignParameterScope],
    pub(crate) timelines: &'a [DesignFeatureTimeline],
    pub(crate) construction_groups: &'a [DesignConstructionOperandGroup],
    pub(crate) fillet_radius_groups: &'a [DesignFilletRadiusGroup],
    pub(crate) edge_operands: &'a [DesignEdgeOperand],
    pub(crate) edge_identity_operands: &'a [DesignEdgeIdentityOperand],
    pub(crate) edge_treatment_vertex_operands: &'a [DesignEdgeTreatmentVertexOperand],
    pub(crate) entity_selection_operands:
        &'a [crate::records::topology::entity_selection::DesignEntitySelectionOperand],
    pub(crate) curve_identities: &'a [SketchCurveIdentity],
    pub(crate) face_operands: &'a [DesignFaceOperand],
    pub(crate) body_recipe_operands: &'a [DesignBodyRecipeOperand],
    pub(crate) legacy_loft_body_carriers: &'a [DesignLoftLegacyBodyCarrier],
    pub(crate) placements: &'a [DesignSketchPlacement],
    pub(crate) body_bindings: &'a [DesignBodyBinding],
    pub(crate) component_naming_spaces: &'a [crate::records::recipes::DesignComponentNamingSpace],
    pub(crate) histories: &'a [crate::history_records::AsmHistory],
    pub(crate) face_boundaries:
        &'a [crate::records::topology::historical_context::AsmFaceBoundary],
    pub(crate) sheet_metal_settings:
        &'a [crate::records::sheet_metal_settings::DesignSheetMetalSettings],
}

/// Authored construction ordinal of every parameter scope represented by a
/// neutral top-level feature. All input scopes must share one Design stream.
fn authored_scope_ordinals<'a>(
    scopes: &'a [DesignParameterScope],
    timelines: &[DesignFeatureTimeline],
) -> Result<HashMap<(&'a str, u32), u64>, CodecError> {
    let Some(first_scope) = scopes.first() else {
        return Ok(HashMap::new());
    };
    let stream = native_stream(&first_scope.id).unwrap_or(ids::DEFAULT_STREAM);
    if scopes
        .iter()
        .any(|scope| native_stream(&scope.id).unwrap_or(ids::DEFAULT_STREAM) != stream)
    {
        return Err(CodecError::NotImplemented(
            "independent Design scope streams have no shared authored timeline order".into(),
        ));
    }
    let mut stream_scopes = Vec::new();
    for scope in scopes {
        stream_scopes.push(scope);
    }
    authored_scope_ordinals_for_stream(&stream_scopes, timelines)
}

/// Authored scope ordinals evaluated independently for every Design stream.
pub(crate) fn authored_scope_ordinals_per_stream<'a>(
    scopes: &'a [DesignParameterScope],
    timelines: &[DesignFeatureTimeline],
) -> Result<HashMap<(&'a str, u32), u64>, CodecError> {
    let mut streams = HashMap::<&str, Vec<&DesignParameterScope>>::new();
    for scope in scopes {
        let stream = native_stream(&scope.id).unwrap_or(ids::DEFAULT_STREAM);
        if !streams.contains_key(stream) {
            streams.insert(stream, Vec::new());
        }
        streams
            .get_mut(stream)
            .ok_or_else(|| CodecError::malformed("authored stream index lost its key"))?
            .push(scope);
    }
    let mut out = HashMap::new();
    for stream_scopes in streams.into_values() {
        for (key, ordinal) in authored_scope_ordinals_for_stream(&stream_scopes, timelines)? {
            if out.insert(key, ordinal).is_some() {
                return Err(CodecError::Malformed(
                    "Design scope record identity is not unique".into(),
                ));
            }
        }
    }
    Ok(out)
}

fn authored_scope_ordinals_for_stream<'a>(
    scopes: &[&'a DesignParameterScope],
    timelines: &[DesignFeatureTimeline],
) -> Result<HashMap<(&'a str, u32), u64>, CodecError> {
    let mut out = HashMap::new();
    let Some(first_scope) = scopes.first().copied() else {
        return Ok(out);
    };
    let stream = native_stream(&first_scope.id).unwrap_or(ids::DEFAULT_STREAM);
    let mut scopes_by_record = HashMap::<u32, &DesignParameterScope>::new();
    for scope in scopes {
        if scopes_by_record
            .insert(scope.record_index, *scope)
            .is_some()
        {
            return Err(CodecError::Malformed(
                "Design scope record identity is not unique".into(),
            ));
        }
    }
    for scope in scopes {
        let Some(target_record_index) = scope
            .assembly_alignment()
            .and_then(super::super::records::feature::assembly::DesignAssemblyAlignment::joint_origin_scope_record_index)
        else {
            continue;
        };
        let Some(target) = scopes_by_record.get(&target_record_index) else {
            return Err(CodecError::Malformed(
                "Design assembly datum envelope has no JointOrigin target".into(),
            ));
        };
        if scope.kind() != crate::records::feature::scope::DesignFeatureKind::Assemble
            || target.kind() != crate::records::feature::scope::DesignFeatureKind::JointOrigin
            || target.joint_origin_transform().is_none()
        {
            return Err(CodecError::Malformed(
                "Design assembly datum envelope has an invalid JointOrigin target".into(),
            ));
        }
    }

    let mut stream_timelines = Vec::new();
    for timeline in timelines
        .iter()
        .filter(|timeline| native_stream(timeline.id()).unwrap_or(ids::DEFAULT_STREAM) == stream)
    {
        stream_timelines.push(timeline);
    }
    stream_timelines.sort_by_key(|left| left.source_ordinal);
    if stream_timelines.is_empty() {
        let first_family = design_feature_family(&first_scope.kind());
        let homogeneous = scopes.iter().all(|scope| {
            first_family.map_or_else(
                || scope.kind() == first_scope.kind(),
                |family| design_feature_family(&scope.kind()) == Some(family),
            )
        });
        let mut ordered = Vec::new();
        for &scope in scopes {
            ordered.push(scope);
        }
        ordered.sort_by_key(|left| left.feature_ordinal);
        let complete_ordinals = ordered.iter().enumerate().all(|(ordinal, scope)| {
            u32::try_from(ordinal)
                .ok()
                .and_then(|ordinal| ordinal.checked_add(1))
                == Some(scope.feature_ordinal.get())
        });
        if !homogeneous || !complete_ordinals {
            return Err(CodecError::NotImplemented(
                "Design scopes have no complete authored timeline order".into(),
            ));
        }
        for (ordinal, scope) in ordered.into_iter().enumerate() {
            let ordinal = u64::try_from(ordinal)
                .map_err(|_| CodecError::Malformed("Design feature ordinal exceeds u64".into()))?;
            if out
                .insert(
                    (
                        native_stream(&scope.id).unwrap_or(ids::DEFAULT_STREAM),
                        scope.record_index,
                    ),
                    ordinal,
                )
                .is_some()
            {
                return Err(CodecError::Malformed(
                    "Design scope record identity is not unique".into(),
                ));
            }
        }
        return Ok(out);
    }

    if !stream_timelines
        .iter()
        .enumerate()
        .all(|(ordinal, timeline)| u32::try_from(ordinal).ok() == Some(timeline.source_ordinal))
    {
        return Err(CodecError::Malformed(
            "Design timeline-record ordinals are not contiguous".into(),
        ));
    }
    if stream_timelines
        .iter()
        .filter(|timeline| !timeline.frame().items().is_empty())
        .count()
        > 1
    {
        return Err(CodecError::NotImplemented(
            "multiple nonempty Design timelines have no shared authored order".into(),
        ));
    }
    let mut item_ordinals = HashMap::<u64, u64>::new();
    let mut next_ordinal = 0_u64;
    for timeline in stream_timelines {
        for item in timeline.frame().items().iter().map(|item| item.value) {
            if item_ordinals.insert(item, next_ordinal).is_some() {
                return Err(CodecError::Malformed(
                    "Design timeline item identity is not unique".into(),
                ));
            }
            next_ordinal = next_ordinal.checked_add(1).ok_or_else(|| {
                CodecError::Malformed("Design feature ordinal exceeds u64".into())
            })?;
        }
    }
    for scope in scopes {
        if let Some(ordinal) = item_ordinals.get(&u64::from(scope.record_index)).copied() {
            // discarded-value: each scope is visited once in this stream.
            let _ = out.insert((stream, scope.record_index), ordinal);
        }
    }
    for scope in scopes {
        let Some(owner) = scope.base_feature_construction().and_then(
            crate::records::feature::base_feature::DesignBaseFeatureConstruction::timeline_owner,
        ) else {
            continue;
        };
        if scopes_by_record.contains_key(&owner) {
            continue;
        }
        if let Some(ordinal) = item_ordinals.get(&u64::from(owner)).copied() {
            // discarded-value: each owned scope is visited once in this stream.
            let _ = out.insert((stream, scope.record_index), ordinal);
        }
    }
    for scope in scopes {
        let Some(target_record_index) = scope
            .assembly_alignment()
            .and_then(super::super::records::feature::assembly::DesignAssemblyAlignment::joint_origin_scope_record_index)
        else {
            continue;
        };
        let target = scopes_by_record[&target_record_index];
        let source_key = (stream, scope.record_index);
        let Some(source_ordinal) = out.remove(&source_key) else {
            continue;
        };
        let target_key = (stream, target.record_index);
        if item_ordinals.contains_key(&u64::from(target.record_index)) {
            continue;
        }
        if out.insert(target_key, source_ordinal).is_some() {
            return Err(CodecError::Malformed(
                "Design JointOrigin target has multiple authored timeline positions".into(),
            ));
        }
    }
    Ok(out)
}

/// The millimetre coordinate frame that a `JointOrigin` scope solves.
pub(crate) fn joint_origin_frame(
    scope: &DesignParameterScope,
) -> Option<cadmpeg_ir::features::FeatureCoordinateFrame> {
    let transform = scope.joint_origin_transform()?;
    cadmpeg_ir::features::FeatureCoordinateFrame::new(
        Point3::new(
            transform[0][3] * 10.0,
            transform[1][3] * 10.0,
            transform[2][3] * 10.0,
        ),
        Vector3::new(transform[0][0], transform[1][0], transform[2][0]),
        Vector3::new(transform[0][1], transform[1][1], transform[2][1]),
        Vector3::new(transform[0][2], transform[1][2], transform[2][2]),
    )
}

/// Result of following one scope's preceding state through internal scopes.
pub(crate) enum ScopeHistoryPredecessor<'a> {
    /// The state chain reaches no projected parameter scope.
    None,
    /// The chain reaches one projected parameter scope.
    Scope(&'a DesignParameterScope),
    /// The state or history identity does not select one scope.
    Ambiguous,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) enum ComponentHistoryNamespace {
    Aggregate,
    Component(u64),
}

enum ScopeHistoryBinding {
    Absent,
    Bound(HashMap<String, String>),
}

/// History-state index qualified by component-local Design and ASM history.
pub(crate) struct ScopeHistoryGraph<'a> {
    binding: ScopeHistoryBinding,
    component_namespaces: HashMap<String, ComponentHistoryNamespace>,
    scopes_by_state:
        HashMap<(String, ComponentHistoryNamespace, String, i64), Vec<&'a DesignParameterScope>>,
}

impl<'a> ScopeHistoryGraph<'a> {
    pub(crate) fn new(
        scopes: &'a [DesignParameterScope],
        body_bindings: &[DesignBodyBinding],
        body_recipe_operands: &[DesignBodyRecipeOperand],
        component_naming_spaces: &[crate::records::recipes::DesignComponentNamingSpace],
        histories: &[crate::history_records::AsmHistory],
    ) -> Self {
        let binding = if histories.is_empty() {
            ScopeHistoryBinding::Absent
        } else {
            ScopeHistoryBinding::Bound(crate::history::bind_scope_histories(
                scopes,
                body_bindings,
                body_recipe_operands,
                histories,
            ))
        };
        let mut component_namespaces = HashMap::new();
        for scope in scopes {
            if let Some(namespace) = Self::component_namespace(scope, component_naming_spaces) {
                // discarded-value: each scope identity has one namespace.
                let _ = component_namespaces.insert(scope.id.clone(), namespace);
            }
        }
        let mut scopes_by_state: HashMap<_, Vec<_>> = HashMap::new();
        for scope in scopes {
            let (Some(stream), Some(state_id)) =
                (native_stream(&scope.id), scope.history_state_id())
            else {
                continue;
            };
            let history_id = match &binding {
                ScopeHistoryBinding::Absent => String::new(),
                ScopeHistoryBinding::Bound(bound) => {
                    let Some(history_id) = bound.get(&scope.id) else {
                        continue;
                    };
                    history_id.to_owned()
                }
            };
            let Some(component_namespace) = component_namespaces.get(&scope.id) else {
                continue;
            };
            let key = (
                stream.to_owned(),
                *component_namespace,
                history_id,
                state_id,
            );
            scopes_by_state.entry(key).or_default().push(scope);
        }
        Self {
            binding,
            component_namespaces,
            scopes_by_state,
        }
    }

    pub(crate) fn component_namespace(
        scope: &DesignParameterScope,
        component_naming_spaces: &[crate::records::recipes::DesignComponentNamingSpace],
    ) -> Option<ComponentHistoryNamespace> {
        Self::component_namespace_at(&scope.id, scope.record_index, component_naming_spaces)
    }

    pub(crate) fn component_namespace_at(
        id: &str,
        record_index: u32,
        component_naming_spaces: &[crate::records::recipes::DesignComponentNamingSpace],
    ) -> Option<ComponentHistoryNamespace> {
        let stream = native_stream(id)?;
        let mut stream_spaces = component_naming_spaces
            .iter()
            .filter(|space| native_stream(&space.id) == Some(stream))
            .peekable();
        if stream_spaces.peek().is_none() {
            return Some(ComponentHistoryNamespace::Aggregate);
        }
        stream_spaces
            .filter(|space| space.component_record_index <= u64::from(record_index))
            .max_by_key(|space| space.component_record_index)
            .map(|space| ComponentHistoryNamespace::Component(space.component_record_index))
    }

    fn history_id(&self, scope: &DesignParameterScope) -> Option<&str> {
        match &self.binding {
            ScopeHistoryBinding::Absent => Some(""),
            ScopeHistoryBinding::Bound(bound) => bound.get(&scope.id).map(String::as_str),
        }
    }

    fn state_key(
        &self,
        scope: &DesignParameterScope,
        state_id: i64,
    ) -> Option<(String, ComponentHistoryNamespace, String, i64)> {
        let (Some(stream), Some(namespace), Some(history_id)) = (
            native_stream(&scope.id),
            self.component_namespaces.get(&scope.id),
            self.history_id(scope),
        ) else {
            return None;
        };
        Some((
            stream.to_owned(),
            *namespace,
            history_id.to_owned(),
            state_id,
        ))
    }

    /// Follow `scope.previous_history_state_id()` until a scope accepted by
    /// `projected` is reached. Internal scopes preserve state continuity but
    /// are not themselves authored top-level features.
    pub(crate) fn predecessor<F>(
        &self,
        scope: &DesignParameterScope,
        projected: F,
    ) -> Result<ScopeHistoryPredecessor<'a>, CodecError>
    where
        F: Fn(&DesignParameterScope) -> bool,
    {
        let Some(mut state_id) = scope.previous_history_state_id() else {
            return Ok(ScopeHistoryPredecessor::None);
        };
        let Some(stream) = native_stream(&scope.id) else {
            return Ok(ScopeHistoryPredecessor::Ambiguous);
        };
        let Some(history_id) = self.history_id(scope) else {
            return Ok(ScopeHistoryPredecessor::Ambiguous);
        };
        let mut visited = HashSet::new();
        loop {
            let Some(component_namespace) = self.component_namespaces.get(&scope.id) else {
                return Ok(ScopeHistoryPredecessor::Ambiguous);
            };
            let Some(candidates) = self.scopes_by_state.get(&(
                stream.to_owned(),
                *component_namespace,
                history_id.to_owned(),
                state_id,
            )) else {
                return Ok(ScopeHistoryPredecessor::None);
            };
            let [candidate] = candidates.as_slice() else {
                return Ok(ScopeHistoryPredecessor::Ambiguous);
            };
            if candidate.id == scope.id {
                if visited.is_empty() {
                    return Ok(ScopeHistoryPredecessor::None);
                }
                return Err(CodecError::Malformed(
                    "Design scope history-state dependency is cyclic".into(),
                ));
            }
            if projected(candidate) {
                return Ok(ScopeHistoryPredecessor::Scope(candidate));
            }
            if !visited.insert(candidate.id.as_str()) {
                return Err(CodecError::Malformed(
                    "Design scope history-state dependency is cyclic".into(),
                ));
            }
            let Some(previous_state_id) = candidate.previous_history_state_id() else {
                return Ok(ScopeHistoryPredecessor::None);
            };
            if candidate.history_state_id() == Some(previous_state_id) {
                return Ok(ScopeHistoryPredecessor::None);
            }
            state_id = previous_state_id;
        }
    }
}

fn ensure_feature_dependencies_precede(
    features: &[cadmpeg_ir::features::Feature],
) -> Result<(), CodecError> {
    let mut ordinals = HashMap::new();
    for feature in features {
        ordinals.insert(&feature.id, feature.ordinal);
    }
    if ordinals.len() != features.len() {
        return Err(CodecError::Malformed(
            "projected Design feature identity is not unique".into(),
        ));
    }
    let mut unique_ordinals = HashSet::new();
    for feature in features {
        if !unique_ordinals.insert(feature.ordinal) {
            return Err(CodecError::Malformed(
                "projected Design feature ordinal is not unique".into(),
            ));
        }
        if let Some((dependency, dependency_ordinal)) =
            feature.dependencies.iter().find_map(|dependency| {
                ordinals
                    .get(dependency)
                    .filter(|ordinal| **ordinal >= feature.ordinal)
                    .map(|ordinal| (dependency, ordinal))
            })
        {
            return Err(crate::design::text::malformed_design(format_args!(
                    "Design feature dependency does not precede its authored timeline position: {dependency} at ordinal {dependency_ordinal} -> {} at ordinal {}",
                    feature.id, feature.ordinal,
                )));
        }
    }
    Ok(())
}

/// Neutral features and parameters projected from the Design records.
#[derive(Debug)]
pub(crate) struct ProjectedParameterDesign {
    pub(crate) features: Vec<cadmpeg_ir::features::Feature>,
    pub(crate) parameters: Vec<cadmpeg_ir::features::DesignParameter>,
    /// Owner scopes whose parameter ordering exceeded [`PARAMETER_ORDER_WORK`].
    /// Each keeps its source ordinals and loses its same-scope dependency edges.
    pub(crate) unordered_parameter_scopes: usize,
    /// `BaseFlange` scopes projected with their stored thickness copy because
    /// their sheet-metal rule states no thickness.
    pub(crate) unruled_flange_thicknesses: usize,
    pub(crate) hole_losses: Vec<cadmpeg_ir::report::loss::LossNote>,
}

impl ProjectedParameterDesign {
    /// The loss note for base flanges projected with their stored thickness
    /// copy, when there are any.
    pub(crate) fn unruled_flange_thickness_loss(
        &self,
    ) -> Option<cadmpeg_ir::report::loss::LossNote> {
        (self.unruled_flange_thicknesses != 0).then(|| {
            crate::loss::F3dLossCode::SheetThicknessUnruled.note(format!(
                "{} sheet-metal base flange(s) use the thickness copy their scope stores: their sheet-metal rule states no readable thickness, and the copy can differ from it.",
                self.unruled_flange_thicknesses,
            ))
        })
    }
}

/// Project Design parameters and feature scopes, including fixed edge identities.
pub(crate) fn project_parameter_design_with_edge_identities(
    ctx: &DecodeContext<'_>,
    inputs: &ProjectInputs<'_>,
) -> Result<ProjectedParameterDesign, CodecError> {
    use cadmpeg_ir::features::{
        patterns::PatternKind, DesignParameter as NeutralParameter, DimensionDisplay, Feature,
        FeatureDefinition, FeatureOperation, ParameterId, ParameterValue, UnresolvedFamily,
    };
    use cadmpeg_ir::scalar::{Angle, Length};
    use std::collections::BTreeMap;

    let &ProjectInputs {
        native,
        owners,
        scopes,
        timelines,
        construction_groups,
        edge_operands,
        edge_identity_operands,
        edge_treatment_vertex_operands,
        entity_selection_operands,
        curve_identities,
        face_operands,
        body_recipe_operands,
        placements,
        body_bindings,
        component_naming_spaces,
        histories,
        face_boundaries,
        sheet_metal_settings,
        ..
    } = inputs;
    let mut hole_losses = Vec::new();
    let unruled_flange_thicknesses = std::cell::Cell::new(0_usize);

    let source_ordinals = authored_scope_ordinals(scopes, timelines)?;

    let mut scope_ids = HashMap::new();
    {
        for scope in scopes {
            let Some(stream) = native_stream(&scope.id) else {
                continue;
            };
            if source_ordinals.contains_key(&(stream, scope.record_index)) {
                // discarded-value: duplicate scope keys retain the last feature ID.
                let _ = scope_ids.insert(
                    (stream, scope.record_index),
                    crate::ids::neutral_feature_id(scope),
                );
            }
        }
    }
    let mut owners_by_index = HashMap::new();
    {
        for owner in owners {
            let Some(stream) = native_stream(owner.id()) else {
                continue;
            };
            // discarded-value: duplicate owner keys retain the last source record.
            let _ = owners_by_index.insert((stream, owner.record_index()), owner);
        }
    }
    let mut features = scopes
        .iter()
        .filter(|scope| {
            let stream = native_stream(&scope.id).unwrap_or(ids::DEFAULT_STREAM);
            source_ordinals.contains_key(&(stream, scope.record_index))
        })
        .map(|scope| -> Result<Feature, CodecError> {
            let native_scope = native_stream(&scope.id).unwrap_or(ids::DEFAULT_STREAM);
            let mut parameters = Vec::new();
            for owner in owners.iter().filter(|owner| {
                native_stream(owner.id()) == Some(native_scope)
                    && owner.scope_record_index() == scope.record_index
            }) {
                if let Some(parameter) = native.iter().find(|parameter| {
                    native_stream(&parameter.id) == Some(native_scope)
                        && parameter.record_index == owner.parameter_record_index()
                }) {
                    parameters.push((owner.local_ordinal(), parameter));
                }
            }
            let family = design_feature_family(&scope.kind());
            let mut inserted_bodies: Vec<cadmpeg_ir::ids::BodyId> = Vec::new();
            let definition = match family {
                Some(DesignFeatureFamily::Sketch) => {
                    FeatureDefinition::Operation(FeatureOperation::Sketch {
                        sketch: cadmpeg_ir::features::SketchFeatureBinding::Unresolved,
                    })
                }
                // The joint is product structure; the feature names it only
                // after product projection has emitted it.
                Some(DesignFeatureFamily::Assemble) => native_scope_definition(scope, &parameters)?,
                Some(DesignFeatureFamily::Extrude) => {
                    let mut definition = project_extrude(
                        ctx,
                        scope,
                        &parameters,
                        construction_groups,
                        face_operands,
                        placements,
                        body_recipe_operands,
                    )?
                    .map_or_else(|| native_scope_definition(scope, &parameters), Ok)?;
                    bind_extrude_work_plane_targets(
                        &mut definition,
                        scope,
                        scopes,
                        construction_groups,
                        entity_selection_operands,
                    );
                    definition
                }
                Some(DesignFeatureFamily::Fillet) => {
                    project_fillet_arm(ctx, inputs, scope, parameters.as_slice(), native_scope)?
                }
                Some(DesignFeatureFamily::Chamfer) => {
                    let mut chamfer = if parameters.is_empty() {
                        project_fixed_chamfer(
                            scope,
                            construction_groups,
                            edge_operands,
                            edge_identity_operands,
                            edge_treatment_vertex_operands,
                            histories,
                            ctx,
                        )?
                    } else {
                        None
                    };
                    if chamfer.is_none() {
                        chamfer = project_chamfer(scope, &parameters, inputs, ctx)?;
                    }
                    chamfer.map_or_else(|| native_scope_definition(scope, &parameters), Ok)?
                }
                Some(DesignFeatureFamily::Combine) => project_combine(scope, native_scope)
                    .unwrap_or_else(|| {
                        FeatureDefinition::Operation(FeatureOperation::Native {
                            kind: scope.kind_name().into(),
                            parameters: BTreeMap::new(),
                        })
                    }),
                Some(DesignFeatureFamily::Draft) => project_draft(
                    scope,
                    scopes,
                    construction_groups,
                    entity_selection_operands,
                    face_operands,
                    histories,
                )?
                .unwrap_or_else(|| {
                    FeatureDefinition::Operation(FeatureOperation::Native {
                        kind: scope.kind_name().into(),
                        parameters: BTreeMap::new(),
                    })
                }),
                Some(DesignFeatureFamily::ReplaceFace) => project_replace_face(
                    scope,
                    construction_groups,
                    face_operands,
                    body_recipe_operands,
                )
                .map_or_else(|| native_scope_definition(scope, &parameters), Ok)?,
                Some(DesignFeatureFamily::Revolve) => project_fixed_revolve_with_entities(
                    scope,
                    construction_groups,
                    edge_operands,
                    entity_selection_operands,
                    face_operands,
                    (placements, curve_identities),
                )
                .unwrap_or(FeatureDefinition::Operation(FeatureOperation::Unresolved {
                    family: UnresolvedFamily::Revolve,
                })),
                Some(DesignFeatureFamily::Loft) => project_fixed_loft(scope, inputs, ctx)?
                    .unwrap_or_else(|| {
                        FeatureDefinition::Operation(FeatureOperation::Native {
                            kind: scope.kind_name().into(),
                            parameters: BTreeMap::new(),
                        })
                    }),
                Some(DesignFeatureFamily::Sweep) => project_fixed_sweep(
                    scope,
                    construction_groups,
                    edge_operands,
                    edge_identity_operands,
                    entity_selection_operands,
                    face_operands,
                    ctx,
                )?
                .unwrap_or_else(|| {
                    FeatureDefinition::Operation(FeatureOperation::Native {
                        kind: scope.kind_name().into(),
                        parameters: BTreeMap::new(),
                    })
                }),
                Some(DesignFeatureFamily::Pipe) => native_scope_definition(scope, &parameters)?,
                Some(DesignFeatureFamily::SurfacePatch) => project_surface_patch(
                    ctx,
                    scope,
                    construction_groups,
                    edge_operands,
                    edge_identity_operands,
                    face_boundaries,
                )?
                .unwrap_or_else(|| {
                    FeatureDefinition::Operation(FeatureOperation::Native {
                        kind: scope.kind_name().into(),
                        parameters: BTreeMap::new(),
                    })
                }),
                Some(DesignFeatureFamily::SurfaceExtend) => {
                    if let Some((operation, distance)) =
                        scope.surface_extend_operation().and_then(|operation| {
                            cadmpeg_ir::scalar::PositiveLength::new(operation.distance.get() * 10.0)
                                .map(|distance| (operation, distance))
                        })
                    {
                        use crate::records::feature::surface_ops::DesignSurfaceExtendMethod;
                        use cadmpeg_ir::features::{FaceSelection, SurfaceExtension};

                        let method = match operation.method {
                            DesignSurfaceExtendMethod::Natural => SurfaceExtension::Natural,
                            DesignSurfaceExtendMethod::Tangent => SurfaceExtension::Linear,
                            DesignSurfaceExtendMethod::Perpendicular => {
                                SurfaceExtension::Perpendicular
                            }
                        };
                        FeatureDefinition::Operation(FeatureOperation::ExtendSurface {
                            faces: FaceSelection::Native(format!(
                                "{}{}{}",
                                native_scope,
                                ":design-record#",
                                u64::from(operation.boundary_record_index)
                            )),
                            distance: Some(distance),
                            method,
                        })
                    } else {
                        FeatureDefinition::Operation(FeatureOperation::Native {
                            kind: scope.kind_name().into(),
                            parameters: BTreeMap::new(),
                        })
                    }
                }
                Some(DesignFeatureFamily::SurfaceOffset) => scope
                    .surface_offset_operation()
                    .map(|operation| {
                        project_surface_offset(scope, operation, construction_groups, face_operands)
                    })
                    .transpose()?
                    .flatten()
                    .unwrap_or_else(|| {
                        FeatureDefinition::Operation(FeatureOperation::Native {
                            kind: scope.kind_name().into(),
                            parameters: BTreeMap::new(),
                        })
                    }),
                Some(DesignFeatureFamily::SurfaceRuled) => project_ruled_surface(
                    scope,
                    owners,
                    native,
                    construction_groups,
                    edge_operands,
                    edge_identity_operands,
                    ctx,
                )?
                .map_or_else(|| native_scope_definition(scope, &parameters), Ok)?,
                Some(DesignFeatureFamily::SurfaceTrim) => {
                    project_surface_trim(scope, construction_groups, body_recipe_operands)
                        .unwrap_or_else(|| {
                            FeatureDefinition::Operation(FeatureOperation::Native {
                                kind: scope.kind_name().into(),
                                parameters: BTreeMap::new(),
                            })
                        })
                }
                Some(DesignFeatureFamily::BoundaryFill) => {
                    project_boundary_fill(scope, construction_groups).unwrap_or_else(|| {
                        FeatureDefinition::Operation(FeatureOperation::Native {
                            kind: scope.kind_name().into(),
                            parameters: BTreeMap::new(),
                        })
                    })
                }
                Some(DesignFeatureFamily::Hole) => project_hole(
                    scope,
                    &parameters,
                    construction_groups,
                    face_operands,
                    &mut hole_losses,
                )?
                .map_or_else(|| native_scope_definition(scope, &parameters), Ok)?,
                Some(DesignFeatureFamily::Split) => project_split(
                    scope,
                    scopes,
                    construction_groups,
                    entity_selection_operands,
                    face_operands,
                )?
                .unwrap_or_else(|| {
                    FeatureDefinition::Operation(FeatureOperation::Native {
                        kind: scope.kind_name().into(),
                        parameters: BTreeMap::new(),
                    })
                }),
                Some(DesignFeatureFamily::CircularPattern) => project_circular_pattern(
                    scope,
                    construction_groups,
                    face_operands,
                    entity_selection_operands,
                    scopes,
                )
                .unwrap_or_else(|| {
                    FeatureDefinition::Operation(FeatureOperation::Pattern {
                        seeds: Vec::new(),
                        pattern: PatternKind::UNRESOLVED_CIRCULAR,
                    })
                }),
                Some(DesignFeatureFamily::RectangularPattern) => {
                    project_rectangular_pattern_scalars(
                        scope,
                        construction_groups,
                        face_operands,
                        entity_selection_operands,
                        scopes,
                    )
                    .unwrap_or_else(|| {
                        FeatureDefinition::Operation(FeatureOperation::Pattern {
                            seeds: Vec::new(),
                            pattern: PatternKind::UNRESOLVED_LINEAR,
                        })
                    })
                }
                Some(DesignFeatureFamily::Mirror) => {
                    project_mirror(scope, construction_groups, face_operands, scopes)
                        .unwrap_or_else(|| {
                            FeatureDefinition::Operation(FeatureOperation::Pattern {
                                seeds: Vec::new(),
                                pattern: PatternKind::UNRESOLVED_MIRROR,
                            })
                        })
                }
                Some(DesignFeatureFamily::OffsetFaces) => {
                    project_offset_faces(scope, &parameters, face_operands, construction_groups)?
                        .map_or_else(|| native_scope_definition(scope, &parameters), Ok)?
                }
                Some(DesignFeatureFamily::Move) => project_move(scope, construction_groups)
                    .map_or_else(|| native_scope_definition(scope, &parameters), Ok)?,
                Some(DesignFeatureFamily::Shell) => {
                    project_shell(scope, face_operands, construction_groups)?
                        .map_or_else(|| native_scope_definition(scope, &parameters), Ok)?
                }
                Some(DesignFeatureFamily::Thicken) => {
                    project_thicken(scope, face_operands, construction_groups)?
                        .map_or_else(|| native_scope_definition(scope, &parameters), Ok)?
                }
                Some(DesignFeatureFamily::Coil) => {
                    project_coil(scope, &parameters, construction_groups)?
                        .map_or_else(|| native_scope_definition(scope, &parameters), Ok)?
                }
                Some(DesignFeatureFamily::Scale) => {
                    if let Some(operation) = scope.scale_operation() {
                        let factor =
                            cadmpeg_ir::scalar::NonZeroReal::from(operation.uniform_factor);
                        let body_group = construction_groups.iter().find(|group| {
                            native_stream(&group.id) == Some(native_scope)
                                && group.scope_record_index == scope.record_index
                                && group.record_index == operation.body_group_record_index
                        });
                        let bodies = match body_group {
                            Some(group) => {
                                cadmpeg_ir::features::BodySelection::Native(group.id.clone())
                            }
                            None => cadmpeg_ir::features::BodySelection::Unresolved,
                        };
                        let center = match operation.center_position.and_then(|center| {
                            cadmpeg_ir::features::FinitePoint3::new(Point3::new(
                                center.value[0].get() * 10.0,
                                center.value[1].get() * 10.0,
                                center.value[2].get() * 10.0,
                            ))
                        }) {
                            Some(center) => cadmpeg_ir::features::ScaleCenter::Point(center),
                            None => cadmpeg_ir::features::ScaleCenter::Native(format!(
                                "{}{}{}",
                                native_scope,
                                ":design-record#",
                                u64::from(operation.center_record_index)
                            )),
                        };
                        FeatureDefinition::Operation(FeatureOperation::Scale {
                            bodies,
                            center: Some(center),
                            factors: cadmpeg_ir::features::ScaleFactors::Uniform { factor },
                        })
                    } else {
                        FeatureDefinition::Operation(FeatureOperation::Native {
                            kind: scope.kind_name().into(),
                            parameters: BTreeMap::new(),
                        })
                    }
                }
                Some(DesignFeatureFamily::Thread) => scope
                    .thread_construction()
                    .and_then(|construction| {
                        construction
                            .nominal_size
                            .value()
                            .ok()
                            .and_then(cadmpeg_ir::scalar::PositiveLength::new)
                            .map(|size| (construction, size))
                    })
                    .map_or_else(
                        || {
                            Ok::<_, CodecError>(FeatureDefinition::Operation(
                                FeatureOperation::Unresolved {
                                    family: UnresolvedFamily::Thread,
                                },
                            ))
                        },
                        |(construction, nominal_size)| {
                            let face = project_thread_face_selection(
                                scope,
                                &construction.face_group_record_indices,
                                construction_groups,
                                face_operands,
                            );
                            let has_parameter_owners = owners.iter().any(|owner| {
                                native_stream(owner.id()) == Some(native_scope)
                                    && owner.scope_record_index() == scope.record_index
                            });
                            let face_reference_count =
                                construction.face_group_record_indices.len().checked_mul(2);
                            let full_face_extent = !has_parameter_owners
                                && Some(scope.reference_members().len()) == face_reference_count
                                && construction
                                    .face_group_record_indices
                                    .iter()
                                    .enumerate()
                                    .all(|(group_ordinal, group_record_index)| {
                                        let Some(pair_at) = group_ordinal.checked_mul(2) else {
                                            return false;
                                        };
                                        let Some(member_at) = pair_at.checked_add(1) else {
                                            return false;
                                        };
                                        let Some(member_record_index) = scope
                                            .reference_members()
                                            .values()
                                            .nth(member_at)
                                            .copied()
                                        else {
                                            return false;
                                        };
                                        scope.reference_members().values().nth(pair_at)
                                            == Some(group_record_index)
                                            && construction_groups.iter().any(|group| {
                                                native_stream(&group.id) == Some(native_scope)
                                                    && group.scope_record_index
                                                        == scope.record_index
                                                    && group.record_index == *group_record_index
                                                    && group.role() == DesignOperandRole::ROLE_0X10
                                                    && group
                                                        .members()
                                                        .iter()
                                                        .map(|member| member.value)
                                                        .eq([member_record_index])
                                            })
                                    });
                            let extent = cosmetic_thread_extent(
                                construction
                                    .extended
                                    .as_ref()
                                    .and_then(|extended| extended.full_length),
                                parameters
                                    .iter()
                                    .find(|(ordinal, _)| *ordinal == 1)
                                    .and_then(|(_, parameter)| design_positive_length(parameter)),
                                full_face_extent,
                            );
                            Ok(FeatureDefinition::Operation(
                                FeatureOperation::CosmeticThread {
                                    face,
                                    standard: Some(construction.profile.clone()),
                                    designation: Some(construction.designation.clone()),
                                    diameter: Some(nominal_size),
                                    pitch: cadmpeg_ir::scalar::PositiveLength::new(
                                        construction.pitch.get() * DESIGN_LENGTH_TO_MILLIMETRE,
                                    ),
                                    modeled: construction
                                        .extended
                                        .as_ref()
                                        .map(|extended| extended.modeled),
                                    extent,
                                },
                            ))
                        },
                    )?,
                Some(DesignFeatureFamily::SheetMetalEdgeFlange) => {
                    project_edge_flange(scope, inputs, ctx)?
                        .map_or_else(|| native_scope_definition(scope, &parameters), Ok)?
                }
                Some(DesignFeatureFamily::SheetMetalHem) => project_hem(scope, inputs, ctx)?
                    .map_or_else(|| native_scope_definition(scope, &parameters), Ok)?,
                None => {
                    if let Some(primitive) = project_solid_primitive(scope) {
                        primitive
                    } else if scope.kind()
                        == crate::records::feature::scope::DesignFeatureKind::JointOrigin
                    {
                        joint_origin_frame(scope).map_or_else(
                            || native_scope_definition(scope, &parameters),
                            |frame| {
                                Ok(FeatureDefinition::Operation(
                                    FeatureOperation::DatumCoordinateSystem { frame },
                                ))
                            },
                        )?
                    } else if matches!(
                        scope.kind(),
                        crate::records::feature::scope::DesignFeatureKind::WorkPlane
                            | crate::records::feature::scope::DesignFeatureKind::PlanoDeObra
                    ) {
                        scope.work_plane_transform().map_or_else(
                            || native_scope_definition(scope, &parameters),
                            |transform| Ok(project_work_plane(scope, transform.into())),
                        )?
                    } else if scope.kind()
                        == crate::records::feature::scope::DesignFeatureKind::WorkAxis
                    {
                        scope
                            .work_axis_construction()
                            .and_then(|construction| {
                                let displacement = Vector3::new(
                                    construction.displacement[0].get(),
                                    construction.displacement[1].get(),
                                    construction.displacement[2].get(),
                                );
                                Some((
                                    cadmpeg_ir::features::FinitePoint3::new(Point3::new(
                                        construction.origin[0].get() * 10.0,
                                        construction.origin[1].get() * 10.0,
                                        construction.origin[2].get() * 10.0,
                                    ))?,
                                    cadmpeg_ir::features::FeatureDirection3::from(
                                        cadmpeg_ir::units::UnitVector3::normalized(displacement)?,
                                    ),
                                ))
                            })
                            .map_or_else(
                                || native_scope_definition(scope, &parameters),
                                |(origin, direction)| {
                                    Ok(FeatureDefinition::Operation(FeatureOperation::DatumAxis {
                                        origin,
                                        direction,
                                    }))
                                },
                            )?
                    } else if scope.kind()
                        == crate::records::feature::scope::DesignFeatureKind::WorkPoint
                    {
                        scope
                            .work_point_construction()
                            .and_then(|construction| {
                                Some((
                                    construction,
                                    cadmpeg_ir::features::FinitePoint3::new(Point3::new(
                                        construction.position[0].get() * 10.0,
                                        construction.position[1].get() * 10.0,
                                        construction.position[2].get() * 10.0,
                                    ))?,
                                ))
                            })
                            .map_or_else(
                                || native_scope_definition(scope, &parameters),
                                |(construction, position)| {
                                    Ok(FeatureDefinition::Operation(FeatureOperation::DatumPoint {
                                        position,
                                        construction: project_work_point_construction(
                                            scope,
                                            construction,
                                            &parameters,
                                            edge_operands,
                                            &scope_ids,
                                        )
                                        .map(Box::new),
                                    }))
                                },
                            )?
                    } else if scope.kind()
                        == crate::records::feature::scope::DesignFeatureKind::BaseFlange
                    {
                        project_base_flange(
                            scope,
                            construction_groups,
                            placements,
                            sheet_metal_settings,
                        )
                        .map_or_else(
                            || {
                                FeatureDefinition::Operation(FeatureOperation::Native {
                                    kind: scope.kind_name().into(),
                                    parameters: BTreeMap::new(),
                                })
                            },
                            |(definition, ruled)| {
                                if !ruled {
                                    unruled_flange_thicknesses
                                        .set(unruled_flange_thicknesses.get() + 1);
                                }
                                definition
                            },
                        )
                    } else if scope.kind()
                        == crate::records::feature::scope::DesignFeatureKind::RemoveBody
                    {
                        project_remove_body(scope, construction_groups).unwrap_or_else(|| {
                            FeatureDefinition::Operation(FeatureOperation::Native {
                                kind: scope.kind_name().into(),
                                parameters: BTreeMap::new(),
                            })
                        })
                    } else if scope.kind()
                        == crate::records::feature::scope::DesignFeatureKind::SurfaceStitch
                    {
                        project_surface_stitch(scope, construction_groups).unwrap_or_else(|| {
                            FeatureDefinition::Operation(FeatureOperation::Native {
                                kind: scope.kind_name().into(),
                                parameters: BTreeMap::new(),
                            })
                        })
                    } else if scope.kind()
                        == crate::records::feature::scope::DesignFeatureKind::SplitFace
                    {
                        project_split_face(
                            scope,
                            scopes,
                            construction_groups,
                            entity_selection_operands,
                            face_operands,
                            histories,
                        )?
                        .unwrap_or_else(|| {
                            FeatureDefinition::Operation(FeatureOperation::Native {
                                kind: scope.kind_name().into(),
                                parameters: BTreeMap::new(),
                            })
                        })
                    } else if matches!(
                        scope.kind(),
                        crate::records::feature::scope::DesignFeatureKind::MoveFace
                            | crate::records::feature::scope::DesignFeatureKind::DeplacerFace
                    ) {
                        project_move_face(scope, construction_groups)?.unwrap_or_else(|| {
                            FeatureDefinition::Operation(FeatureOperation::Native {
                                kind: scope.kind_name().into(),
                                parameters: BTreeMap::new(),
                            })
                        })
                    } else if matches!(
                        scope.kind(),
                        crate::records::feature::scope::DesignFeatureKind::DeleteFace
                            | crate::records::feature::scope::DesignFeatureKind::SurfaceDeleteFace
                    ) {
                        project_delete_face(scope, construction_groups, face_operands)?
                            .unwrap_or_else(|| {
                                FeatureDefinition::Operation(FeatureOperation::Native {
                                    kind: scope.kind_name().into(),
                                    parameters: BTreeMap::new(),
                                })
                            })
                    } else if scope.kind()
                        == crate::records::feature::scope::DesignFeatureKind::CopyPasteBodies
                    {
                        if let Some(operation) = scope.copy_paste_bodies_operation() {
                            let selection = design_body_selection(
                                scope,
                                operation
                                    .bodies()
                                    .iter()
                                    .map(|body| u64::from(body.copied.value)),
                                body_bindings,
                            );
                            let bodies = match selection {
                                cadmpeg_ir::features::BodySelection::Resolved {
                                    bodies,
                                    native,
                                } => {
                                    inserted_bodies.extend_from_slice(bodies.as_slice());
                                    cadmpeg_ir::features::InsertedBodies::Resolved { native }
                                }
                                _ => cadmpeg_ir::features::InsertedBodies::Native(scope.id.clone()),
                            };
                            FeatureDefinition::Operation(FeatureOperation::InsertBodies { bodies })
                        } else {
                            FeatureDefinition::Operation(FeatureOperation::Native {
                                kind: scope.kind_name().into(),
                                parameters: BTreeMap::new(),
                            })
                        }
                    } else if scope.kind()
                        == crate::records::feature::scope::DesignFeatureKind::CopyPaste
                    {
                        scope.copy_paste_component_operation().map_or_else(
                            || {
                                FeatureDefinition::Operation(FeatureOperation::Native {
                                    kind: scope.kind_name().into(),
                                    parameters: BTreeMap::new(),
                                })
                            },
                            |operation| {
                                FeatureDefinition::Operation(FeatureOperation::InsertComponent {
                                    occurrence: crate::ids::neutral_component_occurrence_id(
                                        &operation.copied_occurrence_guid,
                                    ),
                                })
                            },
                        )
                    } else if scope.kind()
                        == crate::records::feature::scope::DesignFeatureKind::BaseFeature
                    {
                        if let Some(construction) = scope.base_feature_construction() {
                            FeatureDefinition::Operation(FeatureOperation::BaseFeature {
                                bodies: design_body_selection(
                                    scope,
                                    construction.body_entity_suffixes(),
                                    body_bindings,
                                ),
                            })
                        } else {
                            FeatureDefinition::Operation(FeatureOperation::Native {
                                kind: scope.kind_name().into(),
                                parameters: BTreeMap::new(),
                            })
                        }
                    } else {
                        native_scope_definition(scope, &parameters)?
                    }
                }
            };
            let outputs = inserted_bodies;
            Ok(Feature {
                product_definition: None,
                id: scope_ids[&(native_scope, scope.record_index)].clone(),
                ordinal: source_ordinals[&(native_scope, scope.record_index)],
                name: Some(format!(
                    "{} {}",
                    scope.source_kind_name(),
                    scope.feature_ordinal
                )),
                suppressed: Some(
                    matches!(
                        family,
                        Some(
                            DesignFeatureFamily::Extrude
                                | DesignFeatureFamily::Fillet
                                | DesignFeatureFamily::Chamfer
                        )
                    ) && scope.history_state_id().is_none()
                        && scope.previous_history_state_id().is_none(),
                ),
                dependencies: cadmpeg_ir::features::DistinctMembers::default(),
                source_properties: if matches!(
                    &definition,
                    FeatureDefinition::Operation(FeatureOperation::Native { .. })
                ) {
                    scope_properties(scope, native_scope, placements)?
                } else {
                    BTreeMap::new()
                },
                source_tag: Some(scope.source_kind_name().to_owned()),
                source_text: None,
                source_content: cadmpeg_ir::features::FeatureContent::default(),

                evaluation: cadmpeg_ir::features::FeatureEvaluation::new(
                    definition,
                    cadmpeg_ir::features::DistinctMembers::try_from(outputs)
                        .map_err(cadmpeg_core::CodecError::from)?,
                ),
                native_ref: Some(scope.id.clone()),
            })
        })
        .try_fold(Vec::new(), |mut features, feature| {
            features.push(feature?);
            Ok::<_, CodecError>(features)
        })?;
    let scope_history = ScopeHistoryGraph::new(
        scopes,
        body_bindings,
        body_recipe_operands,
        component_naming_spaces,
        histories,
    );
    for feature in &mut features {
        let Some(scope) = feature
            .native_ref
            .as_deref()
            .and_then(|native_ref| scopes.iter().find(|scope| scope.id == native_ref))
        else {
            continue;
        };
        let ScopeHistoryPredecessor::Scope(predecessor_scope) =
            scope_history.predecessor(scope, |candidate| {
                let stream = native_stream(&candidate.id).unwrap_or(ids::DEFAULT_STREAM);
                scope_ids.contains_key(&(stream, candidate.record_index))
            })?
        else {
            continue;
        };
        let stream = native_stream(&predecessor_scope.id).unwrap_or(ids::DEFAULT_STREAM);
        let Some(predecessor) = scope_ids.get(&(stream, predecessor_scope.record_index)) else {
            continue;
        };
        if predecessor != &feature.id {
            feature.dependencies.insert(predecessor.clone());
        }
    }
    for feature in &mut features {
        let FeatureDefinition::Operation(FeatureOperation::Pattern { seeds, .. }) =
            feature.evaluation.definition()
        else {
            continue;
        };
        for dependency in seeds.iter().filter_map(|seed| match seed {
            cadmpeg_ir::features::patterns::PatternSeed::Feature(feature) => Some(feature),
            _ => None,
        }) {
            if dependency != &feature.id {
                feature.dependencies.insert(dependency.clone());
            }
        }
    }
    for feature in &mut features {
        let feature_id = &feature.id;
        let dependencies = &mut feature.dependencies;
        let mut add_dependency = |dependency: &cadmpeg_ir::features::FeatureId| {
            if dependency != feature_id {
                dependencies.insert(dependency.clone());
            }
            Ok::<_, CodecError>(())
        };
        match feature.evaluation.definition() {
            FeatureDefinition::Operation(FeatureOperation::Draft { anchor, .. }) => {
                if let Some(plane) = anchor.pull().and_then(|pull| pull.plane.as_ref()) {
                    add_dependency(plane)?;
                }
            }
            FeatureDefinition::Operation(
                FeatureOperation::SplitFace {
                    tool: cadmpeg_ir::features::SplitFaceTool::Plane { plane },
                    ..
                }
                | FeatureOperation::SplitBody {
                    tool: cadmpeg_ir::features::SplitBodyTool::Plane { plane },
                    ..
                },
            ) => add_dependency(plane)?,
            FeatureDefinition::Operation(FeatureOperation::SplitFace {
                tool: cadmpeg_ir::features::SplitFaceTool::Planes { planes },
                ..
            }) => {
                for plane in planes {
                    add_dependency(plane)?;
                }
            }
            FeatureDefinition::Operation(FeatureOperation::DatumPoint {
                construction: Some(construction),
                ..
            }) => {
                use cadmpeg_ir::features::{
                    DatumPlaneReference, DatumPointConstruction, VertexSelection,
                };
                match construction.as_ref() {
                    DatumPointConstruction::ThreePlaneIntersection { planes } => {
                        for plane in planes.iter() {
                            if let DatumPlaneReference::Feature { feature } = plane {
                                add_dependency(feature)?;
                            }
                        }
                    }
                    DatumPointConstruction::EdgePlaneIntersection {
                        plane: DatumPlaneReference::Feature { feature },
                        ..
                    } => add_dependency(feature)?,
                    DatumPointConstruction::Vertex {
                        vertex: VertexSelection::Generated { vertex, .. },
                    } => add_dependency(&vertex.feature)?,
                    _ => {}
                }
            }
            FeatureDefinition::Operation(FeatureOperation::DatumThreePointPlane {
                points, ..
            }) => {
                for point in points.iter() {
                    if let cadmpeg_ir::features::VertexSelection::Generated { vertex, .. } = point {
                        add_dependency(&vertex.feature)?;
                    }
                }
            }
            _ => {}
        }
    }
    let mut history_state_features = HashMap::<
        (String, ComponentHistoryNamespace, String, i64),
        Option<cadmpeg_ir::features::FeatureId>,
    >::new();
    {
        for scope in scopes {
            let Some(state_id) = scope.history_state_id() else {
                continue;
            };
            let Some(key) = scope_history.state_key(scope, state_id) else {
                continue;
            };
            let stream = native_stream(&scope.id).unwrap_or(ids::DEFAULT_STREAM);
            let Some(feature_id) = scope_ids.get(&(stream, scope.record_index)) else {
                continue;
            };
            if let Some(candidate) = history_state_features.get_mut(&key) {
                *candidate = None;
            } else {
                let id = feature_id.clone();
                // discarded-value: the vacant-key check admits this state.
                let _ = history_state_features.insert(key, Some(id));
            }
        }
    }
    for feature in &mut features {
        let Some(scope) = feature
            .native_ref
            .as_deref()
            .and_then(|native_ref| scopes.iter().find(|scope| scope.id == native_ref))
        else {
            continue;
        };
        let Some(construction) = scope.work_point_construction() else {
            continue;
        };
        for state_id in construction
            .rule
            .inputs()
            .iter()
            .filter_map(|input| work_point_input_history_state_id(scope, input, edge_operands))
        {
            let Some(key) = scope_history.state_key(scope, state_id) else {
                continue;
            };
            let Some(Some(dependency)) = history_state_features.get(&key) else {
                continue;
            };
            if dependency != &feature.id {
                feature.dependencies.insert(dependency.clone());
            }
        }
    }
    features.sort_by(|a, b| a.id.cmp(&b.id));

    let mut parameters = native
        .iter()
        .map(|parameter| -> NeutralParameter {
            let stream = native_stream(&parameter.id).unwrap_or(ids::DEFAULT_STREAM);
            let native_owner = parameter
                .owner_record_index()
                .and_then(|record_index| owners_by_index.get(&(stream, record_index)));
            let owner =
                native_owner.and_then(|owner| scope_ids.get(&(stream, owner.scope_record_index())));
            let mut properties = BTreeMap::new();
            if parameter.kind() != DesignParameterKind::User {
                properties.insert(
                    cadmpeg_core::nonblank_literal!("source_kind"),
                    parameter.source_kind_str().to_owned(),
                );
            }
            if let (Some(owner_record_index), None) = (parameter.owner_record_index(), owner) {
                properties.insert(
                    cadmpeg_core::nonblank_literal!("owner_record_index"),
                    format!("{owner_record_index}"),
                );
            }
            let value = match parameter.unit().map(|field| field.value.as_str()) {
                Some(unit) if design_length_unit(unit) => {
                    Length::new(parameter.evaluated_value().get() * 10.0)
                        .map(ParameterValue::Length)
                }
                Some(unit) if design_angle_unit(unit) => {
                    Angle::new(parameter.evaluated_value().get()).map(ParameterValue::Angle)
                }
                None => Some(ParameterValue::Real(parameter.evaluated_value())),
                Some(unit) => {
                    properties.insert(cadmpeg_core::nonblank_literal!("unit"), unit.to_owned());
                    properties.insert(
                        cadmpeg_core::nonblank_literal!("evaluated_scalar"),
                        format!("{}", parameter.evaluated_value().get()),
                    );
                    None
                }
            };
            NeutralParameter {
                product_definition: None,
                id: crate::ids::neutral_parameter_id(parameter),
                owner: owner.cloned(),
                ordinal: owner
                    .zip(native_owner)
                    .map_or(parameter.source_ordinal, |(_, owner)| owner.local_ordinal()),
                name: parameter.name().to_owned(),
                expression: parameter.expression().to_owned(),
                display: if parameter.source_kind_str().contains("Diameter Dimension") {
                    Some(DimensionDisplay::Diameter)
                } else if parameter.source_kind_str().contains("Radius Dimension") {
                    Some(DimensionDisplay::Radius)
                } else {
                    None
                },
                value,
                dependencies: cadmpeg_ir::features::DistinctMembers::default(),
                properties,
                pmi: None,
                native_ref: Some(parameter.id.clone()),
            }
        })
        .collect::<Vec<_>>();
    let (
        parameter_scopes,
        document_aliases,
        feature_aliases,
        owned_aliases,
        parameter_owners,
        feature_order,
    ) = {
        let mut parameter_scopes = HashMap::new();
        for (source, parameter) in native.iter().zip(&parameters) {
            if let Some(stream) = native_stream(&source.id) {
                let id = parameter.id.clone();
                // discarded-value: duplicate parameter IDs keep the last source stream.
                let _ = parameter_scopes.insert(id, stream);
            }
        }
        let mut document_aliases = HashMap::<(&str, String), Option<ParameterId>>::new();
        let mut feature_aliases =
            HashMap::<(&str, cadmpeg_ir::features::FeatureId, String), Option<ParameterId>>::new();
        let mut owned_aliases = HashMap::<(&str, String), Vec<ParameterId>>::new();
        for parameter in &parameters {
            let scope = parameter_scopes[&parameter.id];
            if let Some(owner) = &parameter.owner {
                let key = (scope, owner.clone(), parameter.name.clone());
                if let Some(candidate) = feature_aliases.get_mut(&key) {
                    *candidate = None;
                } else {
                    let value = parameter.id.clone();
                    // discarded-value: the vacant-key check admits this alias.
                    let _ = feature_aliases.insert(key, Some(value));
                }
                let key = (scope, parameter.name.clone());
                owned_aliases
                    .entry(key)
                    .or_default()
                    .push(parameter.id.clone());
            } else {
                let key = (scope, parameter.name.clone());
                if let Some(candidate) = document_aliases.get_mut(&key) {
                    *candidate = None;
                } else {
                    let value = parameter.id.clone();
                    // discarded-value: the vacant-key check admits this alias.
                    let _ = document_aliases.insert(key, Some(value));
                }
            }
        }
        let mut parameter_owners = HashMap::new();
        for parameter in &parameters {
            let key = parameter.id.clone();
            let owner = parameter.owner.clone();
            // discarded-value: duplicate parameter IDs keep the last owner.
            let _ = parameter_owners.insert(key, owner);
        }
        let mut feature_order = HashMap::new();
        for feature in &features {
            let id = feature.id.clone();
            // discarded-value: duplicate feature IDs keep the last authored ordinal.
            let _ = feature_order.insert(id, feature.ordinal);
        }
        (
            parameter_scopes,
            document_aliases,
            feature_aliases,
            owned_aliases,
            parameter_owners,
            feature_order,
        )
    };
    for parameter in &mut parameters {
        let scope = parameter_scopes[&parameter.id];
        if parameter.properties.contains_key("owner_record_index") {
            continue;
        }
        parameter.dependencies.clear();
        for identifier in expression_identifiers(&parameter.expression) {
            let identifier = identifier.to_string();
            let alias_key = (scope, identifier);
            let preceding_owned = || {
                let consumer = parameter.owner.as_ref()?;
                let consumer_order = feature_order.get(consumer)?;
                let mut candidates = owned_aliases.get(&alias_key)?.iter().filter(|candidate| {
                    parameter_owners
                        .get(*candidate)
                        .and_then(Option::as_ref)
                        .and_then(|owner| feature_order.get(owner))
                        .is_some_and(|order| order < consumer_order)
                });
                let candidate = candidates.next()?;
                candidates.next().is_none().then_some(candidate)
            };
            let candidate = if let Some(owner) = &parameter.owner {
                let owner_key = owner.clone();
                let feature_identifier = alias_key.1.clone();
                match feature_aliases.get(&(scope, owner_key, feature_identifier)) {
                    Some(None) => None,
                    Some(Some(local)) => Some(local),
                    None => match document_aliases.get(&alias_key) {
                        Some(Some(document)) => Some(document),
                        Some(None) => None,
                        None => preceding_owned(),
                    },
                }
            } else {
                document_aliases.get(&alias_key).and_then(Option::as_ref)
            };
            let Some(candidate) = candidate else {
                continue;
            };
            let dependency_owner = parameter_owners.get(candidate);
            let allowed = match (dependency_owner, &parameter.owner) {
                (Some(Some(dependency_owner)), Some(consumer_owner))
                    if dependency_owner != consumer_owner =>
                {
                    feature_order
                        .get(dependency_owner)
                        .zip(feature_order.get(consumer_owner))
                        .is_some_and(|(dependency, consumer)| dependency < consumer)
                }
                (Some(Some(_)), None) => false,
                (Some(_), _) => true,
                (None, _) => false,
            };
            if !allowed || candidate == &parameter.id || parameter.dependencies.contains(candidate)
            {
                continue;
            }
            let dependency = candidate.clone();
            parameter.dependencies.insert(dependency);
        }
    }
    let unordered_parameter_scopes =
        normalize_parameter_ordinals(&mut parameters, &parameter_owners)?;
    for feature in &mut features {
        for parameter in parameters
            .iter()
            .filter(|parameter| parameter.owner.as_ref() == Some(&feature.id))
        {
            for dependency in &parameter.dependencies {
                if let Some(Some(owner)) = parameter_owners.get(dependency) {
                    if owner != &feature.id {
                        feature.dependencies.insert(owner.clone());
                    }
                }
            }
        }
    }
    ensure_feature_dependencies_precede(&features)?;
    parameters.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(ProjectedParameterDesign {
        features,
        parameters,
        unordered_parameter_scopes,
        unruled_flange_thicknesses: unruled_flange_thicknesses.get(),
        hole_losses,
    })
}

fn project_solid_primitive(
    scope: &DesignParameterScope,
) -> Option<cadmpeg_ir::features::FeatureDefinition> {
    use cadmpeg_ir::features::{
        FeatureDefinition, FeatureOperation, PrimitiveSolid, PrimitiveSolidKind,
    };
    use cadmpeg_ir::math::{Point3, Vector3};
    use cadmpeg_ir::scalar::{Angle, Length};
    let operation = |operation| match operation {
        DesignExtrudeOperation::Join => cadmpeg_ir::features::BooleanOp::Join,
        DesignExtrudeOperation::Cut => cadmpeg_ir::features::BooleanOp::Cut,
        DesignExtrudeOperation::Intersect => cadmpeg_ir::features::BooleanOp::Intersect,
        DesignExtrudeOperation::NewBody => cadmpeg_ir::features::BooleanOp::NewBody,
    };
    Some(match &scope.payload() {
        crate::records::feature::scope::DesignScopePayload::BoxPrimitive(Some(
            crate::records::feature::primitives::DesignBoxPrimitive {
                length,
                width,
                height,
                offset_x,
                offset_y,
                transform,
                operation: result,
                ..
            },
        )) => {
            // The typed-reference form stores the complete local frame; the
            // named form places the box by its two planar offsets.
            let placement = match transform {
                Some(frame) => {
                    let rows = frame.rows();
                    let row = |index: usize| {
                        let [x, y, z, translation] = rows[index];
                        [x, y, z, translation * 10.0]
                    };
                    cadmpeg_ir::transform::Transform::affine([row(0), row(1), row(2)])?
                }
                None => cadmpeg_ir::transform::Transform::affine([
                    [1.0, 0.0, 0.0, offset_x.get() * 10.0],
                    [0.0, 1.0, 0.0, offset_y.get() * 10.0],
                    [0.0, 0.0, 1.0, 0.0],
                ])?,
            };
            FeatureDefinition::Operation(FeatureOperation::Block {
                dimensions: Some([
                    cadmpeg_ir::scalar::PositiveLength::new(length.get() * 10.0)?,
                    cadmpeg_ir::scalar::PositiveLength::new(width.get() * 10.0)?,
                    cadmpeg_ir::scalar::PositiveLength::new(height.get() * 10.0)?,
                ]),
                placement: Some(cadmpeg_ir::features::FeatureRigidPlacement::new(placement)?),
                op: operation(*result),
            })
        }
        crate::records::feature::scope::DesignScopePayload::CylinderPrimitive(Some(
            crate::records::feature::primitives::DesignCylinderPrimitive {
                height,
                diameter,
                transform,
                operation: result,
                ..
            },
        )) => FeatureDefinition::Operation(FeatureOperation::Primitive {
            solid: PrimitiveSolid::new(PrimitiveSolidKind::Cylinder {
                radius: Length::new(diameter.get() * 5.0)?,
                height: Length::new(height.get() * 10.0)?,
                angle: Angle::FULL_TURN,
            })
            .ok()?,
            placement: match transform {
                Some(frame) => {
                    let rows = frame.value.rows();
                    let row = |index: usize| {
                        let [x, y, z, translation] = rows[index];
                        [x, y, z, translation * 10.0]
                    };
                    Some(cadmpeg_ir::features::FeatureRigidPlacement::new(
                        cadmpeg_ir::transform::Transform::affine([row(0), row(1), row(2)])?,
                    )?)
                }
                None => None,
            },
            op: operation(*result),
        }),
        crate::records::feature::scope::DesignScopePayload::SpherePrimitive(Some(
            crate::records::feature::primitives::DesignSpherePrimitive {
                transform,
                diameter,
                operation: result,
                ..
            },
        ))
        | crate::records::feature::scope::DesignScopePayload::EsferaPrimitiva(Some(
            crate::records::feature::primitives::DesignSpherePrimitive {
                transform,
                diameter,
                operation: result,
                ..
            },
        )) => FeatureDefinition::Operation(FeatureOperation::Sphere {
            center: cadmpeg_ir::features::FinitePoint3::new(Point3::new(
                transform[0][3] * 10.0,
                transform[1][3] * 10.0,
                transform[2][3] * 10.0,
            ))?,
            radius: cadmpeg_ir::scalar::PositiveLength::new(diameter.get() * 5.0)?,
            op: operation(*result),
        }),
        crate::records::feature::scope::DesignScopePayload::TorusPrimitive(Some(
            crate::records::feature::primitives::DesignTorusPrimitive {
                transform,
                major_diameter,
                minor_diameter,
                operation: result,
                ..
            },
        )) => FeatureDefinition::Operation(FeatureOperation::Torus {
            center: cadmpeg_ir::features::FinitePoint3::new(Point3::new(
                transform[0][3] * 10.0,
                transform[1][3] * 10.0,
                transform[2][3] * 10.0,
            ))?,
            axis: cadmpeg_ir::units::UnitVector3::new(Vector3::new(
                transform[0][2],
                transform[1][2],
                transform[2][2],
            ))?,
            major_radius: cadmpeg_ir::scalar::PositiveLength::new(major_diameter.get() * 5.0)?,
            minor_radius: cadmpeg_ir::scalar::PositiveLength::new(minor_diameter.get() * 5.0)?,
            op: operation(*result),
        }),
        _ => return None,
    })
}

fn work_point_edge_operand<'a>(
    scope: &DesignParameterScope,
    input: &crate::records::feature::work_geometry::DesignWorkPointInput,
    edge_operands: &'a [DesignEdgeOperand],
) -> Option<&'a DesignEdgeOperand> {
    let crate::records::feature::work_geometry::DesignWorkPointInputCarrier::EdgeRecipe {
        operand_id,
    } = input.carrier()?
    else {
        return None;
    };
    let stream = native_stream(&scope.id)?;
    let mut matching = edge_operands.iter().filter(|operand| {
        operand.id == *operand_id
            && native_stream(&operand.id) == Some(stream)
            && operand.scope_record_index == scope.record_index
            && operand.record_index() == input.record_index()
    });
    let operand = matching.next()?;
    matching.next().is_none().then_some(operand)
}

fn work_point_input_history_state_id(
    scope: &DesignParameterScope,
    input: &crate::records::feature::work_geometry::DesignWorkPointInput,
    edge_operands: &[DesignEdgeOperand],
) -> Option<i64> {
    match input.carrier()? {
        crate::records::feature::work_geometry::DesignWorkPointInputCarrier::EdgeRecipe {
            ..
        } => work_point_edge_operand(scope, input, edge_operands)?.recipe_state_id,
        crate::records::feature::work_geometry::DesignWorkPointInputCarrier::VertexRecipe {
            recipe,
        } => recipe.resolution.map(|resolution| resolution.state_id),
        crate::records::feature::work_geometry::DesignWorkPointInputCarrier::WorkPlane {
            ..
        }
        | crate::records::feature::work_geometry::DesignWorkPointInputCarrier::SketchPoint {
            ..
        } => None,
    }
}

pub(crate) fn work_point_recipe_state_id(
    scope: &DesignParameterScope,
    edge_operands: &[DesignEdgeOperand],
) -> Option<i64> {
    let construction = scope.work_point_construction()?;
    let mut states = construction
        .rule
        .inputs()
        .iter()
        .filter_map(|input| work_point_input_history_state_id(scope, input, edge_operands));
    let state = states.next()?;
    states.all(|candidate| candidate == state).then_some(state)
}

pub(crate) fn work_plane_recipe_state_id(scope: &DesignParameterScope) -> Option<i64> {
    scope.work_plane_construction()?.inputs()[0]
        .resolution
        .map(|resolution| resolution.state_id)
}

fn project_work_point_construction(
    scope: &DesignParameterScope,
    construction: &crate::records::feature::work_geometry::DesignWorkPointConstruction,
    parameters: &[(u32, &DesignParameter)],
    edge_operands: &[DesignEdgeOperand],
    scope_ids: &HashMap<(&str, u32), cadmpeg_ir::features::FeatureId>,
) -> Option<cadmpeg_ir::features::DatumPointConstruction> {
    use crate::records::feature::work_geometry::{
        DesignWorkPointInput, DesignWorkPointInputCarrier, DesignWorkPointRuleForm,
    };
    use cadmpeg_ir::features::{
        DatumPlaneReference, DatumPointConstruction, EdgeSelection, VertexSelection,
    };

    let stream = native_stream(&scope.id)?;
    let edge = |input: &DesignWorkPointInput| -> Option<EdgeSelection> {
        let operand = work_point_edge_operand(scope, input, edge_operands)?;
        let Some((state_id, edge_slot)) = operand
            .recipe_state_id
            .zip(crate::design::edge_resolve::resolved_edge_operand(operand))
        else {
            return Some(EdgeSelection::Native(operand.id.clone()));
        };
        let feature_id = crate::ids::neutral_feature_id(scope);
        let prefix = crate::ids::feature_input_prefix(&feature_id, state_id);
        Some(
            match cadmpeg_ir::features::EdgeSelection::historical(
                crate::ids::feature_input_topology_id(&feature_id, state_id),
                vec![crate::ids::history_input_edge_id(&prefix, edge_slot)],
                operand.id.clone(),
            ) {
                Ok(selection) => selection,
                Err(_) => EdgeSelection::Native(operand.id.clone()),
            },
        )
    };
    let plane = |input: &DesignWorkPointInput| -> Option<DatumPlaneReference> {
        let Some(DesignWorkPointInputCarrier::WorkPlane { selection }) = input.carrier() else {
            return None;
        };
        scope_ids
            .get(&(stream, selection.work_plane_scope_record_index))
            .map(|feature| DatumPlaneReference::Feature {
                feature: feature.clone(),
            })
    };

    let vertex = |input: &DesignWorkPointInput| -> Option<VertexSelection> {
        let Some(DesignWorkPointInputCarrier::VertexRecipe { recipe }) = input.carrier() else {
            return None;
        };
        if recipe.record_index() != input.record_index() {
            return None;
        }
        let vertex = match recipe.resolution {
            None => VertexSelection::native(recipe.recipe_id.clone())
                .unwrap_or(VertexSelection::Unresolved),
            Some(resolution) => {
                let state_id = resolution.state_id;
                let vertex_slot = resolution.vertex_slot();
                let feature_id = crate::ids::neutral_feature_id(scope);
                let prefix = crate::ids::feature_input_prefix(&feature_id, state_id);
                VertexSelection::historical(
                    crate::ids::feature_input_topology_id(&feature_id, state_id),
                    crate::ids::history_input_vertex_id(&prefix, vertex_slot),
                    recipe.recipe_id.clone(),
                )
                .unwrap_or(VertexSelection::Unresolved)
            }
        };
        Some(vertex)
    };

    Some(match construction.rule.form() {
        DesignWorkPointRuleForm::CircleCenter { input } => {
            DatumPointConstruction::CircleCenter { edge: edge(input)? }
        }
        DesignWorkPointRuleForm::TwoEdgeIntersection { inputs } => {
            DatumPointConstruction::TwoEdgeIntersection {
                edges: [edge(&inputs[0])?, edge(&inputs[1])?],
            }
        }
        DesignWorkPointRuleForm::ThreePlaneIntersection { inputs } => {
            DatumPointConstruction::ThreePlaneIntersection {
                planes: Box::new([plane(&inputs[0])?, plane(&inputs[1])?, plane(&inputs[2])?]),
            }
        }
        DesignWorkPointRuleForm::Vertex { input } => DatumPointConstruction::Vertex {
            vertex: vertex(input)?,
        },
        DesignWorkPointRuleForm::Native {
            reference_type: 1,
            inputs,
        } => {
            let [input] = inputs.as_slice() else {
                return None;
            };
            DatumPointConstruction::Vertex {
                vertex: vertex(input)?,
            }
        }
        DesignWorkPointRuleForm::EdgePlaneIntersection { inputs } => {
            DatumPointConstruction::EdgePlaneIntersection {
                edge: edge(&inputs[0])?,
                plane: plane(&inputs[1])?,
            }
        }
        DesignWorkPointRuleForm::DistanceOnEdge { input } => {
            let mut distances = parameters
                .iter()
                .map(|(_, parameter)| *parameter)
                .filter(|parameter| parameter.source_kind() == "PathDistance");
            let distance = distances.next()?;
            if distances.next().is_some()
                || !(0.0..=1.0).contains(&distance.evaluated_value().get())
            {
                return None;
            }
            DatumPointConstruction::DistanceOnEdge {
                edge: edge(input)?,
                fraction: cadmpeg_ir::scalar::Fraction::new(distance.evaluated_value().get())?,
            }
        }
        DesignWorkPointRuleForm::Native { .. } => return None,
    })
}

/// Millimetres per Design length unit (centimetre).
const DESIGN_LENGTH_TO_MM: f64 = 10.0;

/// The millimetre datum frame of a solved `WorkPlane` matrix: column 0 is the
/// u-axis, column 2 the normal, and column 3 the origin in centimetres.
pub(crate) fn work_plane_datum_frame(
    transform: [[f64; 4]; 4],
) -> Option<cadmpeg_ir::features::FeatureDatumPlaneFrame> {
    let (origin, normal, u_axis) = work_plane_frame_columns(transform);
    cadmpeg_ir::features::FeatureDatumPlaneFrame::new(origin, normal, u_axis)
}

/// The same `WorkPlane` frame as a feature support plane.
fn work_plane_support_frame(
    transform: [[f64; 4]; 4],
) -> Option<cadmpeg_ir::features::FeatureSupportPlaneFrame> {
    let (origin, normal, u_axis) = work_plane_frame_columns(transform);
    cadmpeg_ir::features::FeatureSupportPlaneFrame::new(origin, normal, u_axis)
}

fn work_plane_frame_columns(transform: [[f64; 4]; 4]) -> (Point3, Vector3, Vector3) {
    (
        Point3::new(
            transform[0][3] * DESIGN_LENGTH_TO_MM,
            transform[1][3] * DESIGN_LENGTH_TO_MM,
            transform[2][3] * DESIGN_LENGTH_TO_MM,
        ),
        Vector3::new(transform[0][2], transform[1][2], transform[2][2]),
        Vector3::new(transform[0][0], transform[1][0], transform[2][0]),
    )
}

/// Replace each zero-offset native start or termination face group of a
/// projected `Extrude` that selects one `WorkPlane` with that plane.
fn bind_extrude_work_plane_targets(
    definition: &mut cadmpeg_ir::features::FeatureDefinition,
    scope: &DesignParameterScope,
    scopes: &[DesignParameterScope],
    construction_groups: &[DesignConstructionOperandGroup],
    entity_selection_operands: &[crate::records::topology::entity_selection::DesignEntitySelectionOperand],
) {
    use cadmpeg_ir::features::{
        ExtrudeExtent, ExtrudeStart, FaceSelection, FeatureDefinition, FeatureOperation,
        LinearTermination, PlaneTargetSource,
    };

    let FeatureDefinition::Operation(FeatureOperation::Extrude { start, extent, .. }) = definition
    else {
        return;
    };
    let stream = native_stream(&scope.id);
    let work_plane_target = |native: &str| {
        let group = unique_feature_match(construction_groups.iter().filter(|group| {
            group.id == native
                && group.scope_record_index == scope.record_index
                && native_stream(&group.id) == stream
        }))?;
        let planes = selected_work_planes(scope, group, entity_selection_operands, scopes)?;
        let [plane] = planes.as_slice() else {
            return None;
        };
        let frame = work_plane_support_frame(plane.work_plane_transform()?.into())?;
        Some((
            frame,
            cadmpeg_core::text::NonBlankString::new(native.to_owned())?,
        ))
    };
    if let ExtrudeStart::FromFace {
        face: FaceSelection::Native(native),
        offset: None,
    } = start
    {
        if let Some((plane, native)) = work_plane_target(native) {
            *start = ExtrudeStart::FromPlane {
                plane,
                native,
                source: Some(PlaneTargetSource::WorkPlane),
            };
        }
    }
    let sides = match extent {
        ExtrudeExtent::OneSided { side } | ExtrudeExtent::Symmetric { side } => vec![side],
        ExtrudeExtent::TwoSided { first, second } => vec![first, second],
    };
    for side in sides {
        if let LinearTermination::ToFace {
            face: FaceSelection::Native(native),
            offset: None,
        } = &side.termination
        {
            if let Some((plane, native)) = work_plane_target(native) {
                side.termination = LinearTermination::ToPlane {
                    plane,
                    native,
                    source: Some(PlaneTargetSource::WorkPlane),
                };
            }
        }
    }
}

fn project_work_plane(
    scope: &DesignParameterScope,
    transform: [[f64; 4]; 4],
) -> cadmpeg_ir::features::FeatureDefinition {
    use cadmpeg_ir::features::{
        FeatureDefinition, FeatureOperation, UnresolvedFamily, VertexSelection,
    };

    let Some(frame) = work_plane_datum_frame(transform) else {
        return FeatureDefinition::Operation(FeatureOperation::Unresolved {
            family: UnresolvedFamily::DatumPlane,
        });
    };
    let Some(construction) = scope.work_plane_construction() else {
        return FeatureDefinition::Operation(FeatureOperation::DatumPlane { frame });
    };
    let Some(state_id) = work_plane_recipe_state_id(scope) else {
        return FeatureDefinition::Operation(FeatureOperation::Unresolved {
            family: UnresolvedFamily::DatumPlane,
        });
    };
    let feature_id = crate::ids::neutral_feature_id(scope);
    let prefix = crate::ids::feature_input_prefix(&feature_id, state_id);
    let vertex = |recipe: &crate::records::feature::work_geometry::DesignVertexRecipe|
        -> Option<VertexSelection> {
            let resolution = recipe.resolution?;
            let native = recipe.recipe_id.clone();
            let selection = VertexSelection::historical(
                crate::ids::feature_input_topology_id(&feature_id, state_id),
                crate::ids::history_input_vertex_id(&prefix, resolution.vertex_slot()),
                native,
            )
            .unwrap_or(VertexSelection::Unresolved);
            Some(selection)
    };
    let [first, second, third] = construction.inputs();
    let (Some(first), Some(second), Some(third)) = (vertex(first), vertex(second), vertex(third))
    else {
        return FeatureDefinition::Operation(FeatureOperation::Unresolved {
            family: UnresolvedFamily::DatumPlane,
        });
    };
    let Ok(points) =
        cadmpeg_ir::features::ThreePointSelection::try_from(Box::new([first, second, third]))
    else {
        return FeatureDefinition::Operation(FeatureOperation::Unresolved {
            family: UnresolvedFamily::DatumPlane,
        });
    };
    FeatureDefinition::Operation(FeatureOperation::DatumThreePointPlane { frame, points })
}

pub(super) fn project_combine(
    scope: &DesignParameterScope,
    native_scope: &str,
) -> Option<cadmpeg_ir::features::FeatureDefinition> {
    use cadmpeg_ir::features::{BodySelection, FeatureDefinition, FeatureOperation};

    let operation = scope.combine_operation()?;
    let selection = |record_index| {
        format!(
            "{}{}{}",
            native_scope,
            ":design-record#",
            u64::from(record_index)
        )
    };
    let target = BodySelection::Native(selection(operation.target_record_index));
    let tools = if operation.tools.additional.is_empty() {
        BodySelection::Native(selection(operation.tools.first.record_index))
    } else {
        let mut selected = Vec::new();
        for tool in operation.tools.iter() {
            selected.push(selection(tool.record_index));
        }
        BodySelection::NativeSet(cadmpeg_ir::features::NativeSelections::try_from(selected).ok()?)
    };
    let operands = cadmpeg_ir::features::CombineOperands::new(target, tools).ok()?;
    Some(FeatureDefinition::Operation(FeatureOperation::Combine {
        operands,
        op: operation.operation,
        keep_tools: operation.keep_tools,
    }))
}

fn scope_properties(
    scope: &DesignParameterScope,
    native_scope: &str,
    placements: &[DesignSketchPlacement],
) -> Result<std::collections::BTreeMap<cadmpeg_core::text::NonBlankString, String>, CodecError> {
    let mut properties = std::collections::BTreeMap::new();
    for (ordinal, record_index) in scope.reference_members().values().enumerate() {
        properties.insert(
            cadmpeg_core::text::NonBlankString::new(format!("reference:{ordinal}"))
                .ok_or_else(|| CodecError::malformed("reference property key is blank"))?,
            format!("{record_index}"),
        );
    }
    if let Some(profile) = scope
        .extrude_profile()
        .or(scope.base_flange_profile())
        .or_else(|| {
            scope
                .emboss_construction()
                .and_then(|construction| construction.sketch_profile.as_ref())
        })
    {
        if let Some(placement) = placements.iter().find(|placement| {
            native_stream(&placement.id) == Some(native_scope)
                && placement.entity_id == profile.entity_id
        }) {
            let id = crate::ids::neutral_sketch_id(placement);
            properties.insert(cadmpeg_core::nonblank_literal!("profile"), id.into_string());
        }
    }
    Ok(properties)
}

/// The native definition for a scope this codec does not type.
///
/// It states the scope's own kind name and every parameter expression the
/// scope carries, so a feature the codec will not type still names what the
/// source states about it.
///
/// # Errors
///
/// Names the scope whose parameter set states a blank parameter name.
fn native_scope_definition(
    scope: &DesignParameterScope,
    parameters: &[(u32, &DesignParameter)],
) -> Result<cadmpeg_ir::features::FeatureDefinition, CodecError> {
    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};

    let mut properties = std::collections::BTreeMap::new();
    for (_, parameter) in parameters {
        let name = parameter.name().to_owned();
        let Some(name) = cadmpeg_core::text::NonBlankString::new(name) else {
            return Err(cadmpeg_core::text::NamedEntryError::Blank {
                record: scope.id.clone(),
            }
            .into());
        };
        if properties.contains_key(&name) {
            return Err(cadmpeg_core::text::NamedEntryError::Restated {
                record: scope.id.clone(),
                key: name,
            }
            .into());
        }
        let expression = parameter.expression().to_owned();
        properties.insert(name, expression);
    }

    Ok(FeatureDefinition::Operation(FeatureOperation::Native {
        kind: scope.kind_name().to_owned().into(),
        parameters: properties,
    }))
}

fn project_fillet_arm(
    ctx: &DecodeContext<'_>,
    inputs: &ProjectInputs<'_>,
    scope: &DesignParameterScope,
    parameters: &[(u32, &DesignParameter)],
    native_scope: &str,
) -> Result<cadmpeg_ir::features::FeatureDefinition, CodecError> {
    use cadmpeg_ir::features::{
        edge_treatments::{FilletGroup, RadiusSpec},
        EdgeSelection, FeatureDefinition, FeatureOperation,
    };

    if parameters.is_empty()
        && crate::design::decode::operands::fixed_fillet_assignment_is_unresolved(
            scope,
            inputs.construction_groups,
        )
    {
        return native_scope_definition(scope, parameters);
    }

    if parameters.is_empty() {
        if let Some(definition) = project_full_round_fillet(
            scope,
            inputs.construction_groups,
            inputs.face_operands,
            inputs.owners,
            inputs.fillet_radius_groups,
            inputs.histories,
        )? {
            return Ok(definition);
        }
    }

    let mut assignments = Vec::new();
    for assignment in inputs.fillet_radius_groups {
        if native_stream(&assignment.id) == Some(native_scope)
            && assignment.scope_record_index == scope.record_index
        {
            assignments.push(assignment);
        }
    }
    assignments.sort_by_key(|left| left.group_ordinal);
    // A face group that no assignment covers selects faces whose radius set is
    // not stored, so the feature keeps its native form.
    if !assignments.is_empty()
        && inputs.construction_groups.iter().any(|group| {
            native_stream(&group.id) == Some(native_scope)
                && group.scope_record_index == scope.record_index
                && group.role() == DesignOperandRole::ROLE_0X10
                && !group.members().is_empty()
                && assignments
                    .iter()
                    .all(|assignment| assignment.group_record_index != group.record_index)
        })
    {
        return native_scope_definition(scope, parameters);
    }
    if !assignments.is_empty() {
        let Some(assignments) = resolved_fillet_assignments(&assignments, parameters) else {
            return native_scope_definition(scope, parameters);
        };
        let mut groups = Vec::new();
        for resolved in assignments {
            let edge_radius = match &resolved.radius {
                RadiusSpec::Constant { radius } => Some(radius.get()),
                _ => None,
            };
            let edges = if let Some(group) = inputs.construction_groups.iter().find(|group| {
                native_stream(&group.id) == Some(native_scope)
                    && group.record_index == resolved.assignment.group_record_index
            }) {
                resolved_edge_treatment_group_with_corners(
                    group,
                    crate::design::edge_resolve::EdgeTreatmentInputs {
                        groups: inputs.construction_groups,
                        operands: inputs.edge_operands,
                        identity_operands: inputs.edge_identity_operands,
                        vertex_operands: inputs.edge_treatment_vertex_operands,
                        histories: inputs.histories,
                        previous_state_id: scope.previous_history_state_id(),
                        feature_id: &crate::ids::neutral_feature_id(scope),
                        treatment_radius: edge_radius,
                    },
                    ctx,
                )?
            } else {
                EdgeSelection::Native(resolved.assignment.id.clone())
            };
            groups.push(FilletGroup {
                edges,
                radius: resolved.radius,
                tangency_weight: resolved.tangency_weight,
            });
        }
        let groups = groups.try_into().ok();
        return groups.map_or_else(
            || native_scope_definition(scope, parameters),
            |groups| {
                Ok(FeatureDefinition::Operation(FeatureOperation::Fillet {
                    groups,
                }))
            },
        );
    }
    if let Some(definition) = project_variable_fillet(scope, parameters, inputs, ctx)? {
        return Ok(definition);
    }
    if parameters.is_empty() {
        if let Some(definition) = project_fixed_fillet_with_corners(
            scope,
            inputs.construction_groups,
            inputs.edge_operands,
            inputs.edge_identity_operands,
            inputs.edge_treatment_vertex_operands,
            inputs.histories,
            ctx,
        )? {
            return Ok(definition);
        }
    }
    let [(_, parameter)] = parameters else {
        return native_scope_definition(scope, parameters);
    };
    let Some(radius) = (parameter.source_kind() == "Radius")
        .then(|| design_positive_length(parameter))
        .flatten()
    else {
        return native_scope_definition(scope, parameters);
    };
    Ok(FeatureDefinition::Operation(FeatureOperation::Fillet {
        groups: cadmpeg_ir::features::NonEmptyMembers::one(FilletGroup {
            edges: EdgeSelection::Native(scope.id.clone()),
            radius: RadiusSpec::Constant { radius },
            tangency_weight: None,
        }),
    }))
}

struct ResolvedFilletAssignment<'a> {
    assignment: &'a DesignFilletRadiusGroup,
    radius: cadmpeg_ir::features::edge_treatments::RadiusSpec,
    tangency_weight: Option<cadmpeg_ir::scalar::FiniteReal>,
}

fn resolved_fillet_assignments<'a>(
    assignments: &[&'a DesignFilletRadiusGroup],
    parameters: &[(u32, &DesignParameter)],
) -> Option<Vec<ResolvedFilletAssignment<'a>>> {
    use cadmpeg_ir::features::edge_treatments::RadiusSpec;

    let mut by_record = std::collections::BTreeMap::new();
    for (_, parameter) in parameters {
        // discarded-value: duplicate record indices keep the last parameter.
        let _ = by_record.insert(parameter.record_index, *parameter);
    }
    if by_record.len() != parameters.len() {
        return None;
    }
    let mut assigned = Vec::new();
    for assignment in assignments {
        for record in fillet_law_parameter_records(&assignment.law)
            .chain(assignment.tangency_weight_parameter_record_index)
        {
            assigned.push(record);
        }
    }
    let one_shared_set = assignments.windows(2).all(|pair| {
        pair[0].law == pair[1].law
            && pair[0].tangency_weight_parameter_record_index
                == pair[1].tangency_weight_parameter_record_index
    });
    assigned.sort_unstable_by(Ord::cmp);
    if one_shared_set {
        assigned.dedup();
    }
    if !assigned.iter().copied().eq(by_record.keys().copied()) {
        return None;
    }
    let parameter = |record, kind| {
        by_record
            .get(&record)
            .copied()
            .filter(|parameter| parameter.source_kind() == kind)
    };
    let length = |record, kind| design_positive_length(parameter(record, kind)?);
    let mut resolved = Vec::new();
    for &assignment in assignments {
        let tangency_weight = assignment
            .tangency_weight_parameter_record_index
            .map(|record| {
                parameter(record, "TangencyWeight")
                    .filter(|parameter| parameter.unit().is_none())
                    .map(crate::records::parameters::DesignParameter::evaluated_value)
            })
            .map_or(Some(None), |value| value.map(Some))?;
        let radius = match &assignment.law {
            DesignFilletRadiusLaw::Constant {
                radius_parameter_record_index,
            } => RadiusSpec::Constant {
                radius: length(*radius_parameter_record_index, "Radius")?,
            },
            DesignFilletRadiusLaw::Chordal {
                chord_length_parameter_record_index,
            } => RadiusSpec::Chordal {
                chord_length: length(*chord_length_parameter_record_index, "ChordLen")?,
            },
            DesignFilletRadiusLaw::Asymmetric {
                offset_one_parameter_record_index,
                offset_two_parameter_record_index,
            } => RadiusSpec::Asymmetric {
                offset_one: length(*offset_one_parameter_record_index, "EdgeOffset1")?,
                offset_two: length(*offset_two_parameter_record_index, "EdgeOffset2")?,
            },
            DesignFilletRadiusLaw::Variable {
                start_radius_parameter_record_index,
                end_radius_parameter_record_index,
                middle,
            } => {
                let start = parameter(*start_radius_parameter_record_index, "StartRadius")?;
                let end = parameter(*end_radius_parameter_record_index, "EndRadius")?;
                let mut controls = Vec::new();
                controls.push((0, start));
                controls.push((1, end));
                for (ordinal, row) in middle.iter().enumerate() {
                    let ordinal = u32::try_from(ordinal).ok()?;
                    let radius = parameter(row.radius_parameter_record_index, "MidRadius")?;
                    let position = parameter(row.parameter_record_index, "MidParams")?;
                    controls.push((ordinal, radius));
                    controls.push((ordinal, position));
                }
                let (points, _) = variable_fillet_law(&controls)?;
                RadiusSpec::Variable { points }
            }
        };
        resolved.push(ResolvedFilletAssignment {
            assignment,
            radius,
            tangency_weight,
        });
    }
    Some(resolved)
}

fn project_thread_face_selection(
    scope: &DesignParameterScope,
    face_group_record_indices: &[u32],
    groups: &[DesignConstructionOperandGroup],
    face_operands: &[DesignFaceOperand],
) -> cadmpeg_ir::features::FaceSelection {
    use cadmpeg_ir::features::FaceSelection;

    let Some(stream) = native_stream(&scope.id) else {
        return FaceSelection::Unresolved;
    };
    if face_group_record_indices.is_empty() {
        return FaceSelection::Unresolved;
    }
    let mut ordered_groups = Vec::new();
    for record_index in face_group_record_indices {
        let mut matching = groups.iter().filter(|group| {
            native_stream(&group.id) == Some(stream)
                && group.scope_record_index == scope.record_index
                && group.record_index == *record_index
                && group.role() == DesignOperandRole::ROLE_0X10
        });
        let Some(group) = matching.next() else {
            return FaceSelection::Unresolved;
        };
        if matching.next().is_some() {
            return FaceSelection::Unresolved;
        }
        ordered_groups.push(group);
    }
    let native_source = if let [group] = ordered_groups.as_slice() {
        group.id.as_str()
    } else {
        scope.id.as_str()
    };
    let native = native_source.to_owned();
    let mut state = None;
    let mut faces = Vec::new();
    for group in ordered_groups {
        let Some(FaceSelection::Historical {
            state: group_state,
            faces: group_faces,
            ..
        }) = resolved_historical_face_group(
            scope,
            scope.previous_history_state_id(),
            group,
            face_operands,
        )
        else {
            return FaceSelection::Native(native);
        };
        if state
            .as_ref()
            .is_some_and(|expected| expected != &group_state)
        {
            return FaceSelection::Native(native);
        }
        state.get_or_insert(group_state);
        for face in group_faces.as_slice() {
            if !faces.contains(face) {
                faces.push(face.clone());
            }
        }
    }
    let Some(state) = state else {
        return FaceSelection::Native(native);
    };

    let historical_native = native.clone();

    cadmpeg_ir::features::FaceSelection::historical(state, faces, historical_native)
        .unwrap_or(FaceSelection::Native(native))
}

/// Project Fusion's role-`0x4` full-round face construction.
///
/// This form has no radius parameter. Its one compact member identifies the
/// center face; the trailing true flag requests automatic inference of both
/// side-face sets. A role-`0x4` group with any other shape remains available to
/// the regular feature projectors.
fn project_full_round_fillet(
    scope: &DesignParameterScope,
    construction_groups: &[DesignConstructionOperandGroup],
    face_operands: &[DesignFaceOperand],
    owners: &[DesignParameterOwner],
    fillet_radius_groups: &[DesignFilletRadiusGroup],
    histories: &[crate::history_records::AsmHistory],
) -> Result<Option<cadmpeg_ir::features::FeatureDefinition>, CodecError> {
    let stream = or_none!(native_stream(&scope.id));
    if owners.iter().any(|owner| {
        native_stream(owner.id()) == Some(stream)
            && owner.scope_record_index() == scope.record_index
    }) || fillet_radius_groups.iter().any(|assignment| {
        native_stream(&assignment.id) == Some(stream)
            && assignment.scope_record_index == scope.record_index
    }) {
        return Ok(None);
    }
    let mut groups = construction_groups.iter().filter(|group| {
        native_stream(&group.id) == Some(stream) && group.scope_record_index == scope.record_index
    });
    let group = or_none!(groups.next());
    if groups.next().is_some()
        || group.role() != DesignOperandRole::BODIES_A
        || group.members().len() != 1
        || group.frame.trailing_records().len() != 1
        || group.frame.trailing_flags().len() != 1
        || group.frame.trailing_records()[0].value != group.frame.trailing_flags()[0].record_index
        || !group.frame.trailing_flags()[0].value
        || group.frame.variant
    {
        return Ok(None);
    }
    let [crate::records::identity::Located { value: member, .. }] = group.members() else {
        return Ok(None);
    };
    let mut operands = face_operands.iter().filter(|operand| {
        native_stream(&operand.id) == Some(stream)
            && operand.scope_record_index == scope.record_index
            && operand.group_record_index() == Some(group.record_index)
            && operand.group_member_ordinal() == Some(0)
            && operand.record_index() == *member
    });
    let operand = or_none!(operands.next());
    if operands.next().is_some() {
        return Ok(None);
    }
    if operand.recipe_kind != ConstructionRecipeKind::BoundedFace {
        return Ok(None);
    }
    if operand.resolved_face_slots.is_empty() {
        return Ok(None);
    }
    let center_faces = project_face_selection(scope, group, face_operands, histories)?;
    if matches!(center_faces, cadmpeg_ir::features::FaceSelection::Native(_)) {
        return Ok(None);
    }
    Ok(Some(cadmpeg_ir::features::FeatureDefinition::Operation(
        cadmpeg_ir::features::FeatureOperation::FullRoundFillet {
            groups: cadmpeg_ir::features::NonEmptyMembers::one(or_none!(
                cadmpeg_ir::features::edge_treatments::FullRoundFilletGroup::new(
                    center_faces,
                    cadmpeg_ir::features::edge_treatments::FullRoundSideSelection::Automatic,
                    cadmpeg_ir::features::edge_treatments::FullRoundSideSelection::Automatic,
                )
                .ok()
            )),
        },
    )))
}

fn design_body_selection(
    scope: &DesignParameterScope,
    entity_suffixes: impl ExactSizeIterator<Item = u64>,
    body_bindings: &[DesignBodyBinding],
) -> cadmpeg_ir::features::BodySelection {
    use cadmpeg_ir::features::BodySelection;

    let stream = native_stream(&scope.id).unwrap_or(ids::DEFAULT_STREAM);
    let expected_count = entity_suffixes.len();
    let mut bodies = Vec::new();
    for suffix in entity_suffixes {
        let mut matches = body_bindings.iter().filter(|binding| {
            native_stream(binding.id()) == Some(stream) && binding.entity_suffix == suffix
        });
        let Some(body) = matches.next().and_then(|binding| binding.body.as_ref()) else {
            return BodySelection::Native(scope.id.clone());
        };
        if matches.any(|candidate| candidate.body.as_ref() != Some(body)) {
            return BodySelection::Native(scope.id.clone());
        }
        bodies.push(body.clone());
    }
    if bodies.len() == expected_count {
        if let Ok(bodies) = cadmpeg_ir::features::DistinctMembers::try_from(bodies) {
            return BodySelection::Resolved {
                bodies,
                native: scope.id.clone(),
            };
        }
    }
    BodySelection::Native(scope.id.clone())
}

/// Bind each Sketch history node to geometry in exactly one neutral sketch arena.
pub(crate) fn bind_sketch_feature_geometry(
    features: &mut [cadmpeg_ir::features::Feature],
    scopes: &[DesignParameterScope],
    placements: &[DesignSketchPlacement],
    sketches: &[cadmpeg_ir::sketches::Sketch],
    spatial_sketches: &[cadmpeg_ir::sketches::SpatialSketch],
) -> Result<(), CodecError> {
    use cadmpeg_ir::features::{
        DatumPointConstruction, FeatureDefinition, FeatureOperation, LoftSection, PathRef,
        PlanarProfileRef, ProfileRef, SketchPointSelection,
    };

    for feature in features.iter_mut() {
        feature.evaluation.edit(|definition, _| {
            if !matches!(
                definition,
                FeatureDefinition::Operation(
                    FeatureOperation::Sketch { .. } | FeatureOperation::SpatialSketch { .. }
                )
            ) {
                return;
            }
            let Some(scope) = feature
                .native_ref
                .as_deref()
                .and_then(|native_ref| scopes.iter().find(|scope| scope.id == native_ref))
            else {
                return;
            };
            let stream = native_stream(&scope.id);
            let mut matching = placements.iter().filter(|placement| {
                native_stream(&placement.id) == stream
                    && placement.scope_record_index == Some(scope.record_index)
            });
            let Some(placement) = matching.next() else {
                return;
            };
            if matching.next().is_some() {
                return;
            }
            let planar = crate::ids::neutral_sketch_id(placement);
            let spatial = crate::ids::neutral_spatial_sketch_id(placement);
            let has_planar = sketches.iter().any(|sketch| sketch.id == planar);
            let has_spatial = spatial_sketches.iter().any(|sketch| sketch.id == spatial);
            *definition = match (has_planar, has_spatial) {
                (true, false) => FeatureDefinition::Operation(FeatureOperation::Sketch {
                    sketch: cadmpeg_ir::features::SketchFeatureBinding::Planar(Some(planar)),
                }),
                (false, true) => FeatureDefinition::Operation(FeatureOperation::SpatialSketch {
                    sketch: Some(spatial),
                }),
                _ => FeatureDefinition::Operation(FeatureOperation::Sketch {
                    sketch: cadmpeg_ir::features::SketchFeatureBinding::Unresolved,
                }),
            };
        });
    }
    for feature in features.iter_mut() {
        feature.evaluation.edit(|definition, _| {
            'feature_edit: {
                let Some(scope) = feature
                    .native_ref
                    .as_deref()
                    .and_then(|native_ref| scopes.iter().find(|scope| scope.id == native_ref))
                else {
                    break 'feature_edit;
                };
                let FeatureDefinition::Operation(FeatureOperation::Extrude { profile, .. }) =
                    definition
                else {
                    break 'feature_edit;
                };
                let ProfileRef::Planar(PlanarProfileRef::Sketch(sketch)) = profile else {
                    break 'feature_edit;
                };
                if sketches.iter().any(|candidate| candidate.id == *sketch) {
                    break 'feature_edit;
                }
                let mut spatial = None;
                for placement in placements {
                    if crate::ids::neutral_sketch_id(placement) != *sketch {
                        continue;
                    }
                    let spatial_id = crate::ids::neutral_spatial_sketch_id(placement);
                    if let Some(candidate) = spatial_sketches
                        .iter()
                        .find(|candidate| candidate.id == spatial_id)
                    {
                        if spatial.replace(candidate).is_some() {
                            break 'feature_edit;
                        }
                    }
                }
                let Some(spatial) = spatial else {
                    break 'feature_edit;
                };
                if spatial.profiles.is_empty() {
                    let Some(profile_operand) = scope.extrude_profile() else {
                        break 'feature_edit;
                    };
                    let Some(stream) = native_stream(&scope.id) else {
                        break 'feature_edit;
                    };
                    // The spatial carrier has no closed loop that can be represented
                    // by a profile index. Keep the exact profile frame as a native
                    // selection instead of retaining the provisional planar ID.
                    let sketch_id = spatial.id.clone();
                    let selection = format!(
                        "{}{}{}",
                        stream,
                        ":design-record-header#",
                        profile_operand.byte_offset()
                    );
                    *profile =
                        match ProfileRef::spatial_sketch_selection(sketch_id, vec![selection]) {
                            Ok(profile) => profile,
                            Err(_) => {
                                ProfileRef::Planar(PlanarProfileRef::Native(scope.id.clone()))
                            }
                        };
                    break 'feature_edit;
                }
                let Ok(profile_count) = u32::try_from(spatial.profiles.len()) else {
                    break 'feature_edit;
                };
                let mut profiles = Vec::new();
                for profile_index in 0..profile_count {
                    profiles.push(profile_index);
                }
                *profile = match ProfileRef::spatial_sketch_profiles(spatial.id.clone(), profiles) {
                    Ok(profile) => profile,
                    Err(_) => ProfileRef::Planar(PlanarProfileRef::Native(scope.id.clone())),
                };
            }
        });
    }
    let mut sketch_features = HashMap::new();
    let mut spatial_sketch_features = HashMap::new();
    {
        for feature in features.iter() {
            let (index, sketch) = match feature.evaluation.definition() {
                FeatureDefinition::Operation(FeatureOperation::Sketch {
                    sketch: cadmpeg_ir::features::SketchFeatureBinding::Planar(Some(sketch)),
                }) => (&mut sketch_features, sketch.as_str()),
                FeatureDefinition::Operation(FeatureOperation::SpatialSketch {
                    sketch: Some(sketch),
                }) => (&mut spatial_sketch_features, sketch.as_str()),
                _ => continue,
            };
            let key = sketch.to_owned();
            let value = feature.id.clone();
            // discarded-value: duplicate sketch bindings keep the last feature.
            let _ = index.insert(key, value);
        }
    };
    let planar_profile_dependency = |profile: &PlanarProfileRef| match profile {
        PlanarProfileRef::Sketch(sketch)
        | PlanarProfileRef::SketchProfiles { sketch, .. }
        | PlanarProfileRef::SketchRegions { sketch, .. }
        | PlanarProfileRef::SketchEntities { sketch, .. }
        | PlanarProfileRef::SketchSelection { sketch, .. } => sketch_features.get(sketch.as_str()),
        _ => None,
    };
    let profile_dependency = |profile: &ProfileRef| match profile {
        ProfileRef::Planar(
            PlanarProfileRef::Sketch(sketch)
            | PlanarProfileRef::SketchProfiles { sketch, .. }
            | PlanarProfileRef::SketchRegions { sketch, .. }
            | PlanarProfileRef::SketchEntities { sketch, .. }
            | PlanarProfileRef::SketchSelection { sketch, .. },
        ) => sketch_features.get(sketch.as_str()),
        ProfileRef::SpatialSketchProfiles { sketch, .. }
        | ProfileRef::SpatialSketchSelection { sketch, .. } => {
            spatial_sketch_features.get(sketch.as_str())
        }
        ProfileRef::Planar(_) => None,
    };
    let path_dependency = |path: &PathRef| match path {
        PathRef::Sketch(sketch) | PathRef::SketchCurves { sketch, .. } => {
            sketch_features.get(sketch.as_str())
        }
        PathRef::SpatialSketchSelection { sketch, .. }
        | PathRef::SpatialSketchCurves { sketch, .. } => {
            spatial_sketch_features.get(sketch.as_str())
        }
        _ => None,
    };
    let sketch_point_dependency = |point: &SketchPointSelection| match point {
        SketchPointSelection::Planar { sketch, .. } => sketch_features.get(sketch.as_str()),
        SketchPointSelection::Spatial { sketch, .. } => {
            spatial_sketch_features.get(sketch.as_str())
        }
        SketchPointSelection::Unresolved | SketchPointSelection::Native(_) => None,
    };
    for feature in features.iter_mut() {
        let mut add_dependency = |dependency: Option<&cadmpeg_ir::features::FeatureId>| {
            if let Some(dependency) = dependency {
                if dependency != &feature.id {
                    feature.dependencies.insert(dependency.clone());
                }
            }
            Ok::<(), CodecError>(())
        };
        match feature.evaluation.definition() {
            FeatureDefinition::Operation(FeatureOperation::Extrude { profile, .. }) => {
                add_dependency(profile_dependency(profile))?;
            }
            FeatureDefinition::Operation(FeatureOperation::SheetMetalBaseFlange {
                profile,
                ..
            }) => {
                add_dependency(planar_profile_dependency(profile))?;
            }
            FeatureDefinition::Operation(FeatureOperation::Revolve { construction, .. }) => {
                add_dependency(construction.profile().and_then(planar_profile_dependency))?;
                add_dependency(
                    construction
                        .axis()
                        .and_then(|axis| axis.reference.as_ref())
                        .and_then(path_dependency),
                )?;
            }
            FeatureDefinition::Operation(FeatureOperation::Sweep {
                shape,
                path,
                guide_rail,
                ..
            }) => {
                add_dependency(
                    shape
                        .referenced_profile()
                        .and_then(planar_profile_dependency),
                )?;
                match shape {
                    cadmpeg_ir::features::SweepShape::Unresolved { sections, .. }
                    | cadmpeg_ir::features::SweepShape::Surface { sections, .. } => {
                        for section in sections {
                            add_dependency(
                                section
                                    .referenced_profile()
                                    .and_then(planar_profile_dependency),
                            )?;
                        }
                    }
                    cadmpeg_ir::features::SweepShape::Solid { sections, .. } => {
                        for section in sections {
                            add_dependency(
                                section
                                    .referenced_profile()
                                    .and_then(planar_profile_dependency),
                            )?;
                        }
                    }
                }
                add_dependency(path.as_ref().and_then(path_dependency))?;
                add_dependency(
                    guide_rail
                        .as_ref()
                        .and_then(|guide| path_dependency(&guide.path)),
                )?;
            }
            FeatureDefinition::Operation(FeatureOperation::Loft {
                sections, guidance, ..
            }) => {
                for section in sections {
                    if let LoftSection::Profile(profile) = section {
                        add_dependency(profile_dependency(profile))?;
                    }
                }
                match guidance {
                    cadmpeg_ir::features::LoftGuidance::Guides(paths) => {
                        for path in paths {
                            add_dependency(path_dependency(path))?;
                        }
                    }
                    cadmpeg_ir::features::LoftGuidance::Centerline(path) => {
                        add_dependency(path_dependency(path))?;
                    }
                }
            }
            FeatureDefinition::Operation(FeatureOperation::DatumPoint {
                construction: Some(construction),
                ..
            }) => {
                if let DatumPointConstruction::SketchPoint { point } = construction.as_ref() {
                    add_dependency(sketch_point_dependency(point))?;
                }
            }
            FeatureDefinition::Operation(FeatureOperation::Hole {
                profile: Some(profile),
                ..
            }) => {
                add_dependency(planar_profile_dependency(profile))?;
            }
            _ => {}
        }
    }
    Ok(())
}

/// Bind `WorkPoint` inputs that select a sketch point after the sketch arenas
/// have been projected. The Design selection identifies a native point record;
/// the neutral point identity depends on whether that record belongs to a
/// planar or model-space sketch.
pub(crate) fn bind_work_point_sketch_point_constructions(
    features: &mut [cadmpeg_ir::features::Feature],
    scopes: &[DesignParameterScope],
    sketch_entities: &[cadmpeg_ir::sketches::SketchEntity],
    spatial_sketch_entities: &[cadmpeg_ir::sketches::SpatialSketchEntity],
) {
    use cadmpeg_ir::features::{
        DatumPointConstruction, FeatureDefinition, FeatureOperation, SketchPointSelection,
    };

    for feature in features.iter_mut() {
        feature.evaluation.edit(|definition, _| 'feature_edit: {
            let Some(scope) = feature
                .native_ref
                .as_deref()
                .and_then(|native_ref| scopes.iter().find(|scope| scope.id == native_ref))
            else {
                break 'feature_edit;
            };
            let FeatureDefinition::Operation(FeatureOperation::DatumPoint { construction, .. }) =
                definition
            else {
                break 'feature_edit;
            };
            if construction.is_some() {
                break 'feature_edit;
            }
            let Some(point) = scope.work_point_construction() else {
                break 'feature_edit;
            };
            let crate::records::feature::work_geometry::DesignWorkPointRuleForm::Vertex { input } =
                point.rule.form()
            else {
                break 'feature_edit;
            };
            let Some(carrier) = input.carrier() else {
                break 'feature_edit;
            };
            let record_index = input.record_index();
            let crate::records::feature::work_geometry::DesignWorkPointInputCarrier::SketchPoint {
                selection,
            } = carrier
            else {
                break 'feature_edit;
            };
            let stream = native_stream(&scope.id).unwrap_or(ids::DEFAULT_STREAM);
            if let Some(entity) = sketch_entities.iter().find(|entity| {
                entity.native_ref.as_deref() == Some(selection.point_native_id.as_str())
            }) {
                let native = format!("{}{}{}", stream, ":design-record#", u64::from(record_index));
                *construction = Some(Box::new(DatumPointConstruction::SketchPoint {
                    point: SketchPointSelection::Planar {
                        sketch: entity.sketch.clone(),
                        point: entity.id().clone(),
                        native,
                    },
                }));
            } else if let Some(entity) = spatial_sketch_entities.iter().find(|entity| {
                entity.native_ref.as_deref() == Some(selection.point_native_id.as_str())
            }) {
                let native = format!("{}{}{}", stream, ":design-record#", u64::from(record_index));
                *construction = Some(Box::new(DatumPointConstruction::SketchPoint {
                    point: SketchPointSelection::Spatial {
                        sketch: entity.sketch.clone(),
                        point: entity.id().clone(),
                        native,
                    },
                }));
            }
        });
    }
}

/// Bind the sketch points that place a sketch-point `Hole` as its profile
/// after the sketch arenas have been projected. Every location names one
/// planar sketch point, and all of them belong to one sketch.
pub(crate) fn bind_hole_sketch_point_profiles(
    features: &mut [cadmpeg_ir::features::Feature],
    scopes: &[DesignParameterScope],
    sketch_entities: &[cadmpeg_ir::sketches::SketchEntity],
) {
    use crate::records::feature::work_geometry::DesignWorkPointInputCarrier;
    use cadmpeg_ir::features::{
        holes::HoleProfileFilter, FeatureDefinition, FeatureOperation, PlanarProfileRef,
    };

    for feature in features.iter_mut() {
        feature.evaluation.edit(|definition, _| {
            let FeatureDefinition::Operation(FeatureOperation::Hole {
                profile: profile @ None,
                profile_filter,
                ..
            }) = definition
            else {
                return;
            };
            let Some(construction) = feature
                .native_ref
                .as_deref()
                .and_then(|native_ref| scopes.iter().find(|scope| scope.id == native_ref))
                .and_then(DesignParameterScope::hole_construction)
            else {
                return;
            };
            if !construction.placed_by_sketch_points() || construction.entry_points().is_none() {
                return;
            }
            let Some(points) = construction
                .locations
                .iter()
                .map(|location| {
                    let DesignWorkPointInputCarrier::SketchPoint { selection } =
                        location.vertex_input.as_ref()?.carrier()?
                    else {
                        return None;
                    };
                    let mut matches = sketch_entities.iter().filter(|entity| {
                        entity.native_ref.as_deref() == Some(selection.point_native_id.as_str())
                    });
                    match (matches.next(), matches.next()) {
                        (Some(entity), None) => Some(entity),
                        _ => None,
                    }
                })
                .collect::<Option<Vec<_>>>()
            else {
                return;
            };
            let Some(sketch) = points.first().map(|entity| &entity.sketch) else {
                return;
            };
            if points.iter().any(|entity| &entity.sketch != sketch) {
                return;
            }
            let Ok(selected) = PlanarProfileRef::sketch_entities(
                sketch.clone(),
                points.iter().map(|entity| entity.id().clone()).collect(),
            ) else {
                return;
            };
            *profile = Some(selected);
            *profile_filter = Some(HoleProfileFilter::Points);
        });
    }
}

fn project_surface_offset(
    scope: &DesignParameterScope,
    operation: &DesignSurfaceOffsetOperation,
    groups: &[DesignConstructionOperandGroup],
    face_operands: &[DesignFaceOperand],
) -> Result<Option<cadmpeg_ir::features::FeatureDefinition>, CodecError> {
    use cadmpeg_ir::features::{FaceSelection, FeatureDefinition, FeatureOperation};

    let Some(stream) = native_stream(&scope.id) else {
        return Ok(None);
    };
    let Some(distance) = cadmpeg_ir::scalar::Length::new(operation.distance.get() * 10.0) else {
        return Ok(None);
    };
    let DesignSurfaceOffsetSupport::FaceGroups {
        group_record_indices,
    } = &operation.support
    else {
        let DesignSurfaceOffsetSupport::BoundaryCarrier {
            boundary_record_index,
            ..
        } = &operation.support
        else {
            return Ok(None);
        };
        return Ok(Some(FeatureDefinition::Operation(
            FeatureOperation::OffsetSurface {
                faces: FaceSelection::Native(format!(
                    "{}{}{}",
                    stream,
                    ":design-record#",
                    u64::from(*boundary_record_index)
                )),
                distance: Some(distance),
            },
        )));
    };
    let mut faces = Vec::new();
    for group_record_index in group_record_indices {
        let mut matching_groups = groups.iter().filter(|group| {
            native_stream(&group.id) == Some(stream)
                && group.scope_record_index == scope.record_index
                && group.record_index == *group_record_index
                && group.role() == DesignOperandRole::PROFILE
                && !group.members().is_empty()
        });
        let Some(group) = matching_groups.next() else {
            return Ok(None);
        };
        if matching_groups.next().is_some() {
            return Ok(None);
        }
        let Some(FaceSelection::Resolved {
            faces: group_faces, ..
        }) = resolved_face_group(group, face_operands)?
        else {
            return Ok(None);
        };
        for face in &group_faces {
            if !faces.contains(face) {
                faces.push(face.clone());
            }
        }
    }
    if faces.is_empty() {
        return Ok(None);
    }
    let native = scope.id.clone();
    Ok(Some(FeatureDefinition::Operation(
        FeatureOperation::OffsetSurface {
            faces: FaceSelection::Resolved { faces, native },
            distance: Some(distance),
        },
    )))
}

/// Derive the neutral material-side flag from F3D's signed Draft angle.
///
/// F3D does not store a second outward bit. Keeping this rule in one helper
/// makes every Draft projection branch use the same convention.
const fn draft_outward(angle: f64) -> bool {
    angle < 0.0
}

/// The neutral extent of a Thread: the full-length flag of an extended
/// standard thread, else its length owner, else a selection of whole faces.
fn cosmetic_thread_extent(
    full_length: Option<bool>,
    length: Option<cadmpeg_ir::scalar::PositiveLength>,
    full_face: bool,
) -> Option<cadmpeg_ir::features::CosmeticThreadExtent> {
    use cadmpeg_ir::features::CosmeticThreadExtent;

    if full_length == Some(true) {
        return Some(CosmeticThreadExtent::Through {});
    }
    length
        .map(|length| CosmeticThreadExtent::Blind { length })
        .or_else(|| full_face.then_some(CosmeticThreadExtent::Through {}))
}

fn draft_pull_vector(normal: Vector3, flipped: bool) -> Vector3 {
    if flipped {
        Vector3::new(0.0 - normal.x, 0.0 - normal.y, 0.0 - normal.z)
    } else {
        normal
    }
}

/// Normal of the Design Geometry origin plane that a neutral-plane member names.
///
/// An origin-plane identity shares its number with an ASM entity, so its
/// historical candidates describe unrelated topology.
fn selected_origin_plane_normal(
    scope: &DesignParameterScope,
    group: &DesignConstructionOperandGroup,
    entity_selection_operands: &[crate::records::topology::entity_selection::DesignEntitySelectionOperand],
) -> Option<Vector3> {
    let stream = native_stream(&scope.id)?;
    let [crate::records::identity::Located { value: member, .. }] = group.members() else {
        return None;
    };
    let selection = unique_feature_match(entity_selection_operands.iter().filter(|operand| {
        native_stream(&operand.id) == Some(stream)
            && operand.scope_record_index == scope.record_index
            && operand.group_record_index == group.record_index
            && operand.group_member_ordinal == 0
            && operand.record_index() == *member
    }))?;
    if selection.secondary().is_some() {
        return None;
    }
    crate::history::selection::design_geometry_mirror_plane(selection.primary_identity)
        .map(|plane| plane.normal)
}

fn project_draft(
    scope: &DesignParameterScope,
    scopes: &[DesignParameterScope],
    groups: &[DesignConstructionOperandGroup],
    entity_selection_operands: &[crate::records::topology::entity_selection::DesignEntitySelectionOperand],
    face_operands: &[DesignFaceOperand],
    histories: &[crate::history_records::AsmHistory],
) -> Result<Option<cadmpeg_ir::features::FeatureDefinition>, CodecError> {
    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};

    let construction = or_none!(scope.draft_operation());
    let flipped = construction.flags.is_some_and(|flags| flags.flipped);
    let angle_outward = draft_outward(construction.angle.get());
    let faces = or_none!(single_operand_group(
        groups,
        scope,
        DesignOperandRole::ROLE_0X10
    ));
    let mut role_groups = Vec::new();
    for group in groups.iter().filter(|group| {
        native_stream(&group.id) == native_stream(&scope.id)
            && group.scope_record_index == scope.record_index
            && group.role() == DesignOperandRole::ROLE_0X21
            && !group.members().is_empty()
    }) {
        role_groups.push(group);
    }
    let member_of_scope = |group: &DesignConstructionOperandGroup| {
        group
            .members()
            .iter()
            .map(|member| &member.value)
            .all(|member| {
                scope
                    .reference_members()
                    .values()
                    .any(|value| value == member)
            })
    };
    if !scope
        .reference_members()
        .values()
        .any(|value| value == &faces.record_index)
        || !member_of_scope(faces)
    {
        return Ok(None);
    }
    Ok(match role_groups.as_slice() {
        [neutral_plane]
            if member_of_scope(neutral_plane)
                && group_has_entity_selection(scope, neutral_plane, entity_selection_operands) =>
        {
            if let Some(neutral_plane) =
                selected_work_plane(scope, neutral_plane, entity_selection_operands, scopes)
            {
                let transform = or_none!(neutral_plane.work_plane_transform());
                let pull_direction = or_none!(cadmpeg_ir::units::UnitVector3::normalized(
                    draft_pull_vector(
                        Vector3::new(transform[0][2], transform[1][2], transform[2][2]),
                        flipped,
                    )
                ));
                return Ok(Some(FeatureDefinition::Operation(
                    FeatureOperation::Draft {
                        faces: project_draft_face_selection(
                            scope,
                            faces,
                            face_operands,
                            histories,
                        )?,
                        anchor: cadmpeg_ir::features::DraftAnchor::NeutralPlane {
                            plane: cadmpeg_ir::features::FaceSelection::Native(
                                crate::ids::neutral_feature_id(neutral_plane).into_string(),
                            ),
                            pull: Some(cadmpeg_ir::features::DraftPull {
                                direction: cadmpeg_ir::features::FeatureDirection3::from(
                                    pull_direction,
                                ),
                                plane: Some(crate::ids::neutral_feature_id(neutral_plane)),
                            }),
                        },
                        angle: Some(or_none!(cadmpeg_ir::scalar::SlopeAngle::new(
                            construction.angle.get(),
                        ))),
                        outward: Some(angle_outward),
                    },
                )));
            }
            if let Some(normal) =
                selected_origin_plane_normal(scope, neutral_plane, entity_selection_operands)
            {
                let direction = or_none!(cadmpeg_ir::units::UnitVector3::normalized(
                    draft_pull_vector(normal, flipped)
                ));
                return Ok(Some(FeatureDefinition::Operation(
                    FeatureOperation::Draft {
                        faces: project_draft_face_selection(
                            scope,
                            faces,
                            face_operands,
                            histories,
                        )?,
                        anchor: cadmpeg_ir::features::DraftAnchor::NeutralPlane {
                            plane: cadmpeg_ir::features::FaceSelection::Native(
                                neutral_plane.id.clone(),
                            ),
                            pull: Some(cadmpeg_ir::features::DraftPull {
                                direction: cadmpeg_ir::features::FeatureDirection3::from(direction),
                                plane: None,
                            }),
                        },
                        angle: Some(or_none!(cadmpeg_ir::scalar::SlopeAngle::new(
                            construction.angle.get(),
                        ))),
                        outward: Some(angle_outward),
                    },
                )));
            }
            let neutral_plane = selected_historical_face_selection(
                scope,
                neutral_plane,
                entity_selection_operands,
                histories,
            );
            let neutral_plane = or_none!(neutral_plane);
            Some(FeatureDefinition::Operation(FeatureOperation::Draft {
                faces: project_draft_face_selection(scope, faces, face_operands, histories)?,
                anchor: cadmpeg_ir::features::DraftAnchor::NeutralPlane {
                    plane: neutral_plane,
                    pull: None,
                },
                angle: Some(or_none!(cadmpeg_ir::scalar::SlopeAngle::new(
                    construction.angle.get(),
                ))),
                outward: Some(angle_outward != flipped),
            }))
        }
        [neutral_plane] if member_of_scope(neutral_plane) => {
            Some(FeatureDefinition::Operation(FeatureOperation::Draft {
                faces: project_draft_face_selection(scope, faces, face_operands, histories)?,
                anchor: cadmpeg_ir::features::DraftAnchor::NeutralPlane {
                    plane: project_draft_face_selection(
                        scope,
                        neutral_plane,
                        face_operands,
                        histories,
                    )?,
                    pull: None,
                },
                angle: Some(or_none!(cadmpeg_ir::scalar::SlopeAngle::new(
                    construction.angle.get(),
                ))),
                outward: Some(angle_outward != flipped),
            }))
        }
        [first, second] if member_of_scope(first) && member_of_scope(second) => {
            let first_plane = selected_work_plane(scope, first, entity_selection_operands, scopes);
            let second_plane =
                selected_work_plane(scope, second, entity_selection_operands, scopes);
            let (parting_tool, pull_plane) = match (first_plane, second_plane) {
                (Some(plane), None)
                    if !group_has_entity_selection(scope, second, entity_selection_operands) =>
                {
                    (second, plane)
                }
                (None, Some(plane))
                    if !group_has_entity_selection(scope, first, entity_selection_operands) =>
                {
                    (first, plane)
                }
                _ => return Ok(None),
            };
            let transform = or_none!(pull_plane.work_plane_transform());
            let pull_direction = or_none!(cadmpeg_ir::units::UnitVector3::normalized(
                draft_pull_vector(
                    Vector3::new(transform[0][2], transform[1][2], transform[2][2]),
                    flipped,
                )
            ));
            Some(FeatureDefinition::Operation(FeatureOperation::Draft {
                faces: project_draft_face_selection(scope, faces, face_operands, histories)?,
                anchor: cadmpeg_ir::features::DraftAnchor::PartingLine {
                    tool: project_draft_face_selection(
                        scope,
                        parting_tool,
                        face_operands,
                        histories,
                    )?,
                    pull: cadmpeg_ir::features::DraftPull {
                        direction: cadmpeg_ir::features::FeatureDirection3::from(pull_direction),
                        plane: Some(crate::ids::neutral_feature_id(pull_plane)),
                    },
                },
                angle: Some(or_none!(cadmpeg_ir::scalar::SlopeAngle::new(
                    construction.angle.get(),
                ))),
                outward: Some(angle_outward),
            }))
        }
        _ => None,
    })
}

fn selected_historical_face_selection(
    scope: &DesignParameterScope,
    group: &DesignConstructionOperandGroup,
    entity_selection_operands: &[crate::records::topology::entity_selection::DesignEntitySelectionOperand],
    histories: &[crate::history_records::AsmHistory],
) -> Option<cadmpeg_ir::features::FaceSelection> {
    let previous_state_id =
        crate::history::effective_scope_previous_history_state_id(scope, histories)?;
    let stream = native_stream(&scope.id)?;
    let [crate::records::identity::Located { value: member, .. }] = group.members() else {
        return None;
    };
    let mut selections = entity_selection_operands.iter().filter(|operand| {
        native_stream(&operand.id) == Some(stream)
            && operand.scope_record_index == scope.record_index
            && operand.group_record_index == group.record_index
            && operand.group_member_ordinal == 0
            && operand.record_index() == *member
    });
    let selection = selections.next()?;
    if selections.next().is_some() || selection.secondary().is_some() {
        return None;
    }
    let mut face_slots = selection
        .historical_face_candidates
        .iter()
        .filter(|candidate| candidate.historical.state_ids.contains(&previous_state_id))
        .map(|candidate| candidate.face_slot);
    let face_slot = face_slots.next()?;
    if face_slots.any(|candidate| candidate != face_slot) {
        return None;
    }
    let feature = crate::ids::neutral_feature_id(scope);
    let prefix = crate::ids::feature_input_prefix(&feature, previous_state_id);
    Some(
        match cadmpeg_ir::features::FaceSelection::historical(
            crate::ids::feature_input_topology_id(&feature, previous_state_id),
            vec![crate::ids::history_input_face_id(&prefix, face_slot)],
            group.id.clone(),
        ) {
            Ok(selection) => selection,
            Err(_) => cadmpeg_ir::features::FaceSelection::Native(group.id.clone()),
        },
    )
}

fn project_face_selection(
    scope: &DesignParameterScope,
    group: &DesignConstructionOperandGroup,
    face_operands: &[DesignFaceOperand],
    histories: &[crate::history_records::AsmHistory],
) -> Result<cadmpeg_ir::features::FaceSelection, CodecError> {
    let historical = if let Some(previous_state_id) =
        crate::history::effective_scope_previous_history_state_id(scope, histories)
    {
        let updated_face_slots = scope
            .history_state_id()
            .and_then(|state_id| {
                crate::history::unique_history_state_pair(histories, state_id, previous_state_id)
            })
            .and_then(|(_, state, _)| state.transition.as_ref())
            .map_or(&[][..], |transition| {
                transition.topology.faces.updated.as_slice()
            });
        if let Some(selection) =
            resolved_historical_face_group(scope, Some(previous_state_id), group, face_operands)
        {
            Some(selection)
        } else {
            resolved_historical_split_face_target_group_with_updated_faces(
                scope,
                Some(previous_state_id),
                group,
                face_operands,
                updated_face_slots,
            )
        }
    } else {
        None
    };
    if let Some(selection) = historical {
        return Ok(selection);
    }
    match resolved_face_group(group, face_operands)? {
        Some(selection) => Ok(selection),
        None => Ok(cadmpeg_ir::features::FaceSelection::Native(
            group.id.clone(),
        )),
    }
}

fn project_draft_face_selection(
    scope: &DesignParameterScope,
    group: &DesignConstructionOperandGroup,
    face_operands: &[DesignFaceOperand],
    histories: &[crate::history_records::AsmHistory],
) -> Result<cadmpeg_ir::features::FaceSelection, CodecError> {
    let selection = project_face_selection(scope, group, face_operands, histories)?;
    if matches!(&selection, cadmpeg_ir::features::FaceSelection::Native(_)) {
        Ok(
            crate::design::face_resolve::resolved_explicit_bounded_face_group(
                group,
                face_operands,
            )?
            .unwrap_or(selection),
        )
    } else {
        Ok(selection)
    }
}

fn group_has_entity_selection(
    scope: &DesignParameterScope,
    group: &DesignConstructionOperandGroup,
    entity_selection_operands: &[crate::records::topology::entity_selection::DesignEntitySelectionOperand],
) -> bool {
    let Some(stream) = native_stream(&scope.id) else {
        return false;
    };
    group
        .members()
        .iter()
        .map(|member| &member.value)
        .enumerate()
        .any(|(ordinal, record_index)| {
            let Ok(ordinal) = u32::try_from(ordinal) else {
                return false;
            };
            entity_selection_operands.iter().any(|operand| {
                native_stream(&operand.id) == Some(stream)
                    && operand.scope_record_index == scope.record_index
                    && operand.group_record_index == group.record_index
                    && operand.group_member_ordinal == ordinal
                    && operand.record_index() == *record_index
            })
        })
}

fn selected_work_plane<'a>(
    scope: &DesignParameterScope,
    group: &DesignConstructionOperandGroup,
    entity_selection_operands: &[crate::records::topology::entity_selection::DesignEntitySelectionOperand],
    scopes: &'a [DesignParameterScope],
) -> Option<&'a DesignParameterScope> {
    let planes = selected_work_planes(scope, group, entity_selection_operands, scopes);
    let planes = planes?;
    let [plane] = planes.as_slice() else {
        return None;
    };
    Some(*plane)
}

fn selected_work_planes<'a>(
    scope: &DesignParameterScope,
    group: &DesignConstructionOperandGroup,
    entity_selection_operands: &[crate::records::topology::entity_selection::DesignEntitySelectionOperand],
    scopes: &'a [DesignParameterScope],
) -> Option<Vec<&'a DesignParameterScope>> {
    let stream = native_stream(&scope.id)?;
    if group.members().is_empty() {
        return None;
    }
    let mut planes = Vec::new();
    let mut target_record_indices = HashSet::new();
    for (ordinal, member) in group
        .members()
        .iter()
        .map(|member| &member.value)
        .enumerate()
    {
        let ordinal = u32::try_from(ordinal).ok()?;
        let selection = unique_feature_match(entity_selection_operands.iter().filter(|operand| {
            native_stream(&operand.id) == Some(stream)
                && operand.scope_record_index == scope.record_index
                && operand.group_record_index == group.record_index
                && operand.group_member_ordinal == ordinal
                && operand.record_index() == *member
        }));
        let selection = selection?;
        if selection.secondary().is_some() {
            return None;
        }
        let target_record_index = u32::try_from(selection.primary_identity)
            .ok()?
            .checked_add(1)?;
        if !target_record_indices.insert(target_record_index) {
            return None;
        }
        let target = unique_feature_match(scopes.iter().filter(|candidate| {
            native_stream(&candidate.id) == Some(stream)
                && candidate.record_index == target_record_index
                && matches!(
                    candidate.kind(),
                    crate::records::feature::scope::DesignFeatureKind::WorkPlane
                        | crate::records::feature::scope::DesignFeatureKind::PlanoDeObra
                )
                && candidate.work_plane_transform().is_some()
        }));
        let target = target?;
        planes.push(target);
    }
    Some(planes)
}

fn resolved_split_face_path(
    scope: &DesignParameterScope,
    group: &DesignConstructionOperandGroup,
    entity_selection_operands: &[crate::records::topology::entity_selection::DesignEntitySelectionOperand],
    histories: &[crate::history_records::AsmHistory],
) -> Option<cadmpeg_ir::features::PathRef> {
    use cadmpeg_ir::features::PathRef;

    let previous_state_id =
        crate::history::effective_scope_previous_history_state_id(scope, histories)?;
    let stream = native_stream(&scope.id)?;
    let feature = crate::ids::neutral_feature_id(scope);
    let prefix = crate::ids::feature_input_prefix(&feature, previous_state_id);
    let mut edge_slots = Vec::new();
    for (ordinal, member) in group
        .members()
        .iter()
        .map(|member| &member.value)
        .enumerate()
    {
        let ordinal = u32::try_from(ordinal).ok()?;
        let mut selections = entity_selection_operands.iter().filter(|selection| {
            native_stream(&selection.id) == Some(stream)
                && selection.scope_record_index == scope.record_index
                && selection.group_record_index == group.record_index
                && selection.group_member_ordinal == ordinal
                && selection.record_index() == *member
        });
        let selection = selections.next()?;
        if selections.next().is_some() || selection.secondary().is_some() {
            return None;
        }
        let edge_slot = selection.resolved_edge_slot?;
        if edge_slots.contains(&edge_slot) {
            return None;
        }
        edge_slots.push(edge_slot);
    }
    let mut edges = Vec::new();
    for edge_slot in edge_slots {
        let edge = crate::ids::history_input_edge_id(&prefix, edge_slot);
        edges.push(edge);
    }
    PathRef::historical_edges(
        crate::ids::feature_input_topology_id(&feature, previous_state_id),
        edges,
        group.id.clone(),
    )
    .ok()
}

/// Return the unique non-empty construction operand group in `scope` carrying
/// `role`. Yields `None` unless exactly one such group exists.
fn single_operand_group<'a>(
    groups: &'a [DesignConstructionOperandGroup],
    scope: &DesignParameterScope,
    role: DesignOperandRole,
) -> Option<&'a DesignConstructionOperandGroup> {
    let mut matching = groups.iter().filter(|group| {
        native_stream(&group.id) == native_stream(&scope.id)
            && group.scope_record_index == scope.record_index
            && group.role() == role
            && !group.members().is_empty()
    });
    let group = matching.next()?;
    matching.next().is_none().then_some(group)
}

pub(super) fn project_offset_faces(
    scope: &DesignParameterScope,
    parameters: &[(u32, &DesignParameter)],
    operands: &[DesignFaceOperand],
    groups: &[DesignConstructionOperandGroup],
) -> Result<Option<cadmpeg_ir::features::FeatureDefinition>, CodecError> {
    use cadmpeg_ir::features::{FaceMotion, FeatureDefinition, FeatureOperation};
    use cadmpeg_ir::scalar::Length;

    let parameter_distance = match parameters {
        [] => None,
        [(_, distance)] if distance.source_kind() == "distance" => {
            Some(or_none!(design_length(distance)))
        }
        _ => return Ok(None),
    };
    let fixed_distance = match &scope.payload() {
        crate::records::feature::scope::DesignScopePayload::OffsetFaces(value)
        | crate::records::feature::scope::DesignScopePayload::DecalerLesFaces(value) => {
            match value.as_ref() {
                Some(value) => Some(or_none!(Length::new(value.distance.get() * 10.0))),
                None => None,
            }
        }
        _ => return Ok(None),
    };
    let distance = match (parameter_distance, fixed_distance) {
        (Some(parameter), Some(fixed))
            if (parameter.get() - fixed.get()).abs()
                <= EPS_FEATURE_PROJECT_PROJECT_OFFSET_FACES_E9 =>
        {
            parameter
        }
        (Some(distance), None) | (None, Some(distance)) => distance,
        _ => return Ok(None),
    };
    let faces =
        if let Some(group) = single_operand_group(groups, scope, DesignOperandRole::ROLE_0X10) {
            cadmpeg_ir::features::FaceSelection::Native(group.id.clone())
        } else {
            or_none!(direct_face_selection(scope, operands))
        };
    Ok(Some(FeatureDefinition::Operation(
        FeatureOperation::MoveFace {
            faces,
            motion: FaceMotion::Offset { distance },
        },
    )))
}

pub(super) fn project_thicken(
    scope: &DesignParameterScope,
    operands: &[DesignFaceOperand],
    groups: &[DesignConstructionOperandGroup],
) -> Result<Option<cadmpeg_ir::features::FeatureDefinition>, CodecError> {
    use cadmpeg_ir::features::{FaceSelection, FeatureDefinition, FeatureOperation, ThickenSide};

    let crate::records::feature::scope::DesignScopePayload::Thicken(Some(
        crate::records::feature::direct_face::DesignThickenOperation {
            signed_thickness, ..
        },
    )) = &scope.payload()
    else {
        return Ok(None);
    };
    let faces = if let Some(faces) = direct_face_selection(scope, operands) {
        faces
    } else {
        let mut candidates = groups.iter().filter(|group| {
            native_stream(&group.id) == native_stream(&scope.id)
                && group.scope_record_index == scope.record_index
                && matches!(
                    group.role(),
                    DesignOperandRole::ROLE_0X5 | DesignOperandRole::ROLE_0X12
                )
                && !group.members().is_empty()
        });
        let group = or_none!(candidates.next());
        if candidates.next().is_some() {
            return Ok(None);
        }
        FaceSelection::Native(group.id.clone())
    };
    Ok(Some(FeatureDefinition::Operation(
        FeatureOperation::Thicken {
            faces,
            thickness: Some(or_none!(cadmpeg_ir::scalar::PositiveLength::new(
                signed_thickness.get().abs() * 10.0,
            ))),
            side: Some(if signed_thickness.get() > 0.0 {
                ThickenSide::Forward
            } else {
                ThickenSide::Reverse
            }),
        },
    )))
}

pub(super) fn project_shell(
    scope: &DesignParameterScope,
    operands: &[DesignFaceOperand],
    groups: &[DesignConstructionOperandGroup],
) -> Result<Option<cadmpeg_ir::features::FeatureDefinition>, CodecError> {
    use cadmpeg_ir::features::{BodySelection, FaceSelection, FeatureDefinition, FeatureOperation};

    let (crate::records::feature::scope::DesignScopePayload::Shell(Some(operation))
    | crate::records::feature::scope::DesignScopePayload::Schale(Some(operation))
    | crate::records::feature::scope::DesignScopePayload::Vaciado(Some(operation))) =
        &scope.payload()
    else {
        return Ok(None);
    };
    let inside = operation.thickness.get();
    let slots = operation.wall_slots.as_ref();
    let (wall, outward, mode) = match slots.and_then(|slots| slots.outside.as_ref()) {
        None => (inside, Some(operation.outward), None),
        Some(outside) if outside.thickness.get().to_bits() == inside.to_bits() => (
            inside + outside.thickness.get(),
            None,
            Some(cadmpeg_ir::features::ShellMode::BothSides),
        ),
        Some(_) => return Ok(None),
    };
    let join = slots.map(|slots| {
        if slots.rounded_corners {
            cadmpeg_ir::features::ShellJoin::Arc
        } else {
            cadmpeg_ir::features::ShellJoin::Intersection
        }
    });
    let bodies = single_operand_group(groups, scope, DesignOperandRole::BODIES_A)
        .map(|group| BodySelection::Native(group.id.clone()));
    let removed_faces =
        if let Some(group) = single_operand_group(groups, scope, DesignOperandRole::ROLE_0X10) {
            FaceSelection::Native(group.id.clone())
        } else if bodies.is_some() {
            FaceSelection::Faces(Vec::new())
        } else {
            or_none!(direct_face_selection(scope, operands))
        };
    Ok(Some(FeatureDefinition::Operation(
        FeatureOperation::Shell {
            bodies,
            removed_faces,
            thickness: Some(or_none!(cadmpeg_ir::scalar::PositiveLength::new(
                wall * 10.0,
            ))),
            outward,
            mode,
            join,
            resolve_intersections: None,
            allow_self_intersections: None,
        },
    )))
}

fn project_move(
    scope: &DesignParameterScope,
    groups: &[DesignConstructionOperandGroup],
) -> Option<cadmpeg_ir::features::FeatureDefinition> {
    use cadmpeg_ir::features::{BodySelection, FeatureDefinition, FeatureOperation};

    let operation = scope.move_operation()?;
    if !operation.preserves_orientation() {
        return None;
    }
    let group = single_operand_group(groups, scope, DesignOperandRole::BODIES_A)?;
    Some(FeatureDefinition::Operation(FeatureOperation::MoveBody {
        bodies: BodySelection::Native(group.id.clone()),
        translation: cadmpeg_ir::features::FiniteVector3::new(Vector3::new(
            operation.transform[0][3] * 10.0,
            operation.transform[1][3] * 10.0,
            operation.transform[2][3] * 10.0,
        ))?,
        rotation: matrix_axis_angle(operation.transform.as_ref()),
        copies: 0,
    }))
}

pub(super) fn project_remove_body(
    scope: &DesignParameterScope,
    groups: &[DesignConstructionOperandGroup],
) -> Option<cadmpeg_ir::features::FeatureDefinition> {
    use cadmpeg_ir::features::{
        BodyRetentionMode, BodySelection, FeatureDefinition, FeatureOperation,
    };

    let group = single_operand_group(groups, scope, DesignOperandRole::BODIES_A)?;
    Some(FeatureDefinition::Operation(FeatureOperation::DeleteBody {
        bodies: BodySelection::Native(group.id.clone()),
        mode: BodyRetentionMode::DeleteSelected,
    }))
}

/// The neutral base flange and whether its thickness is its sheet-metal
/// rule's; without a rule thickness the scope's stored copy is used.
fn project_base_flange(
    scope: &DesignParameterScope,
    groups: &[DesignConstructionOperandGroup],
    placements: &[DesignSketchPlacement],
    settings: &[crate::records::sheet_metal_settings::DesignSheetMetalSettings],
) -> Option<(cadmpeg_ir::features::FeatureDefinition, bool)> {
    use cadmpeg_ir::features::{
        FeatureDefinition, FeatureOperation, PlanarProfileRef, SheetMetalThicknessSide,
    };

    let operation = scope.base_flange_operation()?;
    let mut matching = groups.iter().filter(|group| {
        native_stream(&group.id) == native_stream(&scope.id)
            && group.scope_record_index == scope.record_index
    });
    let profile_group = matching.next()?;
    if matching.next().is_some() {
        return None;
    }
    if profile_group.scope_reference_ordinal != 0
        || profile_group.record_index != operation.profile_group_record_index
        || profile_group.role() != DesignOperandRole::PROFILE
        || !profile_group
            .members()
            .iter()
            .map(|member| member.value)
            .eq([operation.profile_record_index])
    {
        return None;
    }
    let profile = scope.base_flange_profile()?;
    if profile.scope_reference_ordinal != 1
        || profile.record_index != operation.profile_record_index
    {
        return None;
    }
    let placement = placements.iter().find(|placement| {
        native_stream(&placement.id) == native_stream(&scope.id)
            && placement.entity_id == profile.entity_id
    })?;
    let rule_thickness = settings
        .iter()
        .find(|settings| {
            native_stream(settings.id()) == native_stream(&scope.id)
                && settings.record_index == u64::from(operation.settings_record_index)
        })
        .and_then(|settings| settings.rule_thickness);
    let thickness = rule_thickness.unwrap_or(operation.thickness);
    Some((
        FeatureDefinition::Operation(FeatureOperation::SheetMetalBaseFlange {
            profile: PlanarProfileRef::Sketch(crate::ids::neutral_sketch_id(placement)),
            thickness: cadmpeg_ir::scalar::PositiveLength::new(thickness.get() * 10.0)?,
            side: SheetMetalThicknessSide::Forward,
        }),
        rule_thickness.is_some(),
    ))
}

/// Project a sheet-metal `EdgeFlange` scope onto its neutral operation.
///
/// Owner parameters supply the height, angle, and width. A to-object
/// height resolves its target entity to a known neutral construction feature;
/// otherwise the source selection remains explicit in the neutral height law.
fn project_edge_flange(
    scope: &DesignParameterScope,
    inputs: &ProjectInputs<'_>,
    ctx: &DecodeContext<'_>,
) -> Result<Option<cadmpeg_ir::features::FeatureDefinition>, CodecError> {
    use crate::records::feature::sheet_metal::{
        DesignBendPosition, DesignEdgeFlangeHeightExtent, DesignEdgeFlangeWidthParameterSource,
        DesignSheetMetalHeightDatum,
    };
    use cadmpeg_ir::features::{
        FeatureDefinition, FeatureOperation, SheetMetalBendPosition, SheetMetalFlangeHeight,
        SheetMetalFlangeHeightTarget, SheetMetalFlangeTwoSidedWidth, SheetMetalFlangeWidth,
        SheetMetalHeightDatum,
    };
    use cadmpeg_ir::scalar::PositiveLength;

    let ProjectInputs {
        native: parameters,
        owners,
        scopes,
        construction_groups: groups,
        edge_operands,
        edge_identity_operands,
        entity_selection_operands,
        ..
    } = inputs;
    let Some((operation, stream, height, angle, width, height_datum, bend_position)) =
        (|| -> Result<Option<_>, CodecError> {
            let operation = or_none!(scope.edge_flange_operation());
            let stream = or_none!(native_stream(&scope.id));
            let parameter = |owner_record_index, source_kind: &str| {
                let mut matching = owners.iter().filter(|owner| {
                    native_stream(owner.id()) == Some(stream)
                        && owner.scope_record_index() == scope.record_index
                        && owner.record_index() == owner_record_index
                });
                let owner = matching.next()?;
                if matching.next().is_some() {
                    return None;
                }
                parameters.iter().find(|parameter| {
                    native_stream(&parameter.id) == Some(stream)
                        && parameter.record_index == owner.parameter_record_index()
                        && parameter.source_kind() == source_kind
                })
            };

            let height = match &operation.selection.shape().height() {
                DesignEdgeFlangeHeightExtent::Distance => {
                    SheetMetalFlangeHeight::Distance(or_none!(design_positive_length(or_none!(
                        parameter(operation.height_owner_record_index, "FlangeHeight",)
                    ))))
                }
                DesignEdgeFlangeHeightExtent::ToObject {
                    target_group_record_index,
                    target_operand_record_index,
                    offset_owner_record_index,
                    ..
                } => {
                    let mut target_groups = groups.iter().filter(|group| {
                        native_stream(&group.id) == Some(stream)
                            && group.scope_record_index == scope.record_index
                            && group.record_index == *target_group_record_index
                            && group.role() == DesignOperandRole::ROLE_0X21
                            && group
                                .members()
                                .iter()
                                .map(|member| member.value)
                                .eq([*target_operand_record_index])
                    });
                    let target_group = or_none!(target_groups.next());
                    if target_groups.next().is_some() {
                        return Ok(None);
                    }
                    let mut target_selections =
                        entity_selection_operands.iter().filter(|operand| {
                            native_stream(&operand.id) == Some(stream)
                                && operand.scope_record_index == scope.record_index
                                && operand.group_record_index == target_group.record_index
                                && operand.group_member_ordinal == 0
                                && operand.record_index() == *target_operand_record_index
                        });
                    let target_selection = or_none!(target_selections.next());
                    if target_selections.next().is_some() {
                        return Ok(None);
                    }
                    let target_record_index =
                        or_none!(u32::try_from(target_selection.primary_identity)
                            .ok()
                            .and_then(|index| index.checked_add(1)));
                    let mut target_scopes = scopes.iter().filter(|candidate| {
                        native_stream(&candidate.id) == Some(stream)
                            && candidate.record_index == target_record_index
                            && matches!(
                                candidate.kind(),
                                crate::records::feature::scope::DesignFeatureKind::WorkPlane
                                    | crate::records::feature::scope::DesignFeatureKind::PlanoDeObra
                                    | crate::records::feature::scope::DesignFeatureKind::WorkPoint
                            )
                    });
                    let target = match target_scopes.next() {
                        Some(target_scope) if target_scopes.next().is_none() => {
                            SheetMetalFlangeHeightTarget::Feature(crate::ids::neutral_feature_id(
                                target_scope,
                            ))
                        }
                        None => SheetMetalFlangeHeightTarget::Native(target_selection.id.clone()),
                        Some(_) => return Ok(None),
                    };
                    let offset = or_none!(design_length(or_none!(parameter(
                        *offset_owner_record_index,
                        "ToObjectOffset"
                    ))));
                    SheetMetalFlangeHeight::ToObject { target, offset }
                }
            };
            let angle = or_none!(design_angle(or_none!(parameter(
                operation.angle_owner_record_index,
                "FlangeAngle",
            ))));

            let width = match &operation.selection.shape() {
                crate::records::feature::sheet_metal::DesignEdgeFlangeShape::FullEdge {
                    ..
                } => SheetMetalFlangeWidth::FullEdge,
                crate::records::feature::sheet_metal::DesignEdgeFlangeShape::Symmetric {
                    owner,
                    ..
                } => SheetMetalFlangeWidth::Symmetric {
                    width: or_none!(design_positive_length(or_none!(parameter(
                        *owner,
                        "EdgeWidth"
                    )))),
                },
                crate::records::feature::sheet_metal::DesignEdgeFlangeShape::SymmetricPerEdge(
                    edges,
                ) => {
                    let mut widths = edges.iter().map(|row| {
                        parameter(row.owners, "EdgeWidth").and_then(design_positive_length)
                    });
                    let first = or_none!(or_none!(widths.next()));
                    if widths.any(|width| width != Some(first)) {
                        return Ok(None);
                    }
                    SheetMetalFlangeWidth::Symmetric { width: first }
                }
                crate::records::feature::sheet_metal::DesignEdgeFlangeShape::TwoSidesPerEdge {
                    edges,
                    source,
                } => {
                    let (first_kind, second_kind) = match source {
                        DesignEdgeFlangeWidthParameterSource::EdgeWidth => {
                            ("EdgeWidth_1", "EdgeWidth_2")
                        }
                        DesignEdgeFlangeWidthParameterSource::EdgeOffset => {
                            ("EdgeOffset_1", "EdgeOffset_2")
                        }
                    };
                    let width_length = |owner, kind| {
                        let length = design_length(parameter(owner, kind)?)?;
                        PositiveLength::new(match source {
                            DesignEdgeFlangeWidthParameterSource::EdgeWidth => length.get(),
                            DesignEdgeFlangeWidthParameterSource::EdgeOffset => length.get().abs(),
                        })
                    };
                    let mut widths = Vec::new();
                    for row in edges {
                        let width = SheetMetalFlangeTwoSidedWidth {
                            first: or_none!(width_length(row.owners[0], first_kind)),
                            second: or_none!(width_length(row.owners[1], second_kind)),
                        };
                        widths.push(width);
                    }
                    SheetMetalFlangeWidth::TwoSidesPerEdge {
                        widths: or_none!(cadmpeg_ir::features::SheetMetalFlangeEdgeWidths::new(
                            widths
                        )
                        .ok()),
                    }
                }
                crate::records::feature::sheet_metal::DesignEdgeFlangeShape::TwoSides {
                    owners: [first, second],
                    ..
                } => SheetMetalFlangeWidth::TwoSides {
                    first: or_none!(design_positive_length(or_none!(parameter(
                        *first,
                        "EdgeWidth_1"
                    )))),
                    second: or_none!(design_positive_length(or_none!(parameter(
                        *second,
                        "EdgeWidth_2"
                    )))),
                },
            };

            let height_datum = match operation.height_datum {
                DesignSheetMetalHeightDatum::InnerFaces => SheetMetalHeightDatum::InnerFaces,
                DesignSheetMetalHeightDatum::OuterFaces => SheetMetalHeightDatum::OuterFaces,
                DesignSheetMetalHeightDatum::Unknown(_) => return Ok(None),
            };
            let bend_position = match operation.bend_position {
                DesignBendPosition::Outside => SheetMetalBendPosition::Outside,
                DesignBendPosition::Inside => SheetMetalBendPosition::Inside,
                DesignBendPosition::Adjacent => SheetMetalBendPosition::Adjacent,
                DesignBendPosition::TangentToSide => SheetMetalBendPosition::TangentToSide,
                DesignBendPosition::Unknown(_) => return Ok(None),
            };
            Ok(Some((
                operation,
                stream,
                height,
                angle,
                width,
                height_datum,
                bend_position,
            )))
        })()?
    else {
        return Ok(None);
    };

    // Each role-`0x08` group carries one selected edge. The aggregate role-`0x43`
    // group repeats them, so it contributes no separate selection.
    if operation.selection.shape().edges().next().is_none() {
        return Ok(None);
    }
    let bend_radius = if matches!(scope.class_tag.as_str(), "361" | "362")
        && operation.selection.shape().edges().count() > 1
    {
        let mut aggregates = groups.iter().filter(|group| {
            native_stream(&group.id) == Some(stream)
                && group.scope_record_index == scope.record_index
                && group.record_index == operation.selection.aggregate_group_record_index()
        });
        let aggregate = or_none!(aggregates.next());
        if aggregates.next().is_some()
            || aggregate.role() != DesignOperandRole::ROLE_0X43
            || !aggregate
                .members()
                .iter()
                .map(|member| member.value)
                .eq(operation
                    .selection
                    .shape()
                    .edges()
                    .map(|edge| edge.aggregate_operand_record_index))
        {
            return Ok(None);
        }
        let mut matching = inputs.sheet_metal_settings.iter().filter(|settings| {
            native_stream(settings.id()) == Some(stream)
                && settings.record_index == u64::from(operation.settings_record_index)
                && settings.owner_record_index == u64::from(scope.record_index)
        });
        let settings = or_none!(matching.next());
        if matching.next().is_some() || settings.mask != 0 {
            return Ok(None);
        }
        let thickness = or_none!(settings.rule_thickness).get() * 10.0;
        let mut selected = operation.selection.shape().edges();
        let first = or_none!(selected.next());
        let second = or_none!(selected.next());
        if selected.next().is_some() {
            return Ok(None);
        }
        let slot = |edge: &crate::records::feature::sheet_metal::DesignEdgeFlangeEdge| {
            unique_feature_match(edge_operands.iter().filter(|operand| {
                native_stream(&operand.id) == Some(stream)
                    && operand.scope_record_index == scope.record_index
                    && operand.record_index() == edge.operand_record_index()
            }))?
            .resolved_edge_slot
        };
        or_none!(crate::history::edge_flange::inside_radius_for_scope(
            scope,
            [or_none!(slot(first)), or_none!(slot(second))],
            thickness,
            inputs.histories,
        ))
    } else {
        or_none!(cadmpeg_ir::scalar::PositiveLength::new(
            operation.bend_radius.get() * 10.0
        ))
    };
    let mut selections = Vec::new();
    for edge in operation.selection.shape().edges() {
        let mut matching = groups.iter().filter(|group| {
            native_stream(&group.id) == Some(stream)
                && group.scope_record_index == scope.record_index
                && group.record_index == edge.group_record_index.get()
        });
        let Some(edge_group) = matching.next() else {
            return Ok(None);
        };
        if matching.next().is_some()
            || edge_group.role() != DesignOperandRole::BODIES_B
            || !edge_group
                .members()
                .iter()
                .map(|member| member.value)
                .eq([edge.operand_record_index()])
        {
            return Ok(None);
        }
        let selection = resolved_edge_flange_group(
            edge_group,
            groups,
            edge_operands,
            edge_identity_operands,
            scope.previous_history_state_id(),
            &crate::ids::neutral_feature_id(scope),
            ctx,
        )?;
        selections.push(selection);
    }
    let edges = if selections.len() == 1 {
        or_none!(selections.into_iter().next())
    } else {
        merge_edge_selections(scope, &selections)
    };

    Ok(Some(FeatureDefinition::Operation(
        FeatureOperation::SheetMetalEdgeFlange {
            edges,
            height,
            angle,
            height_datum,
            bend_position,
            width,
            bend_radius,
        },
    )))
}

/// Project a sheet-metal `Hem` scope onto its neutral operation.
///
/// The owner layout distinguishes the rolled and teardrop forms from the
/// shared gap-and-length layout. The native Flip control supplies fold direction;
/// inserted coaxial bend carriers distinguish Flat from Open gap-and-length forms.
fn project_hem(
    scope: &DesignParameterScope,
    inputs: &ProjectInputs<'_>,
    ctx: &DecodeContext<'_>,
) -> Result<Option<cadmpeg_ir::features::FeatureDefinition>, CodecError> {
    use crate::records::feature::sheet_metal::DesignHemParameterOwners;
    use cadmpeg_ir::features::{
        FeatureDefinition, FeatureOperation, SheetMetalHemDirection, SheetMetalHemForm,
    };

    let ProjectInputs {
        native: parameters,
        owners,
        construction_groups: groups,
        edge_operands,
        edge_identity_operands,
        histories,
        ..
    } = inputs;
    let Some((operation, form, edge_group)) = (|| {
        let operation = scope.hem_operation()?;
        let stream = native_stream(&scope.id)?;
        let parameter = |owner_record_index: u32, source_kind: &str| {
            let mut matching_owners = owners.iter().filter(|owner| {
                native_stream(owner.id()) == Some(stream)
                    && owner.scope_record_index() == scope.record_index
                    && owner.record_index() == owner_record_index
            });
            let owner = matching_owners.next()?;
            if matching_owners.next().is_some() {
                return None;
            }
            let mut matching_parameters = parameters.iter().filter(|parameter| {
                native_stream(&parameter.id) == Some(stream)
                    && parameter.record_index == owner.parameter_record_index()
                    && parameter.source_kind() == source_kind
            });
            let parameter = matching_parameters.next()?;
            matching_parameters.next().is_none().then_some(parameter)
        };

        let form =
            match &operation.parameter_owners {
                DesignHemParameterOwners::GapLength {
                    gap_owner_record_index,
                    length_owner_record_index,
                } => SheetMetalHemForm::GapLength {
                    gap: cadmpeg_ir::scalar::NonNegativeLength::try_from(design_length(
                        parameter(*gap_owner_record_index, "HemGap")?,
                    )?)
                    .ok()?,
                    length: design_positive_length(parameter(
                        *length_owner_record_index,
                        "HemLength",
                    )?)?,
                },
                DesignHemParameterOwners::RadiusAngle {
                    radius_owner_record_index,
                    angle_owner_record_index,
                } => SheetMetalHemForm::Rolled {
                    radius: design_positive_length(parameter(
                        *radius_owner_record_index,
                        "HemRadius",
                    )?)?,
                    angle: design_angle(parameter(*angle_owner_record_index, "HemAngle")?)?,
                },
                DesignHemParameterOwners::GapLengthRadius {
                    gap_owner_record_index,
                    length_owner_record_index,
                    radius_owner_record_index,
                } => SheetMetalHemForm::Teardrop {
                    gap: cadmpeg_ir::scalar::NonNegativeLength::try_from(design_length(
                        parameter(*gap_owner_record_index, "HemGap")?,
                    )?)
                    .ok()?,
                    length: design_positive_length(parameter(
                        *length_owner_record_index,
                        "HemLength",
                    )?)?,
                    radius: design_positive_length(parameter(
                        *radius_owner_record_index,
                        "HemRadius",
                    )?)?,
                },
            };

        let mut edge_groups = groups.iter().filter(|group| {
            native_stream(&group.id) == Some(stream)
                && group.scope_record_index == scope.record_index
                && group.record_index == operation.edge_group_record_index.get()
        });
        let edge_group = edge_groups.next()?;
        let edge_has_extra = edge_groups.next().is_some();
        let edge_role_ok = edge_group.role() == DesignOperandRole::BODIES_B;
        let edge_members_ok = edge_group
            .members()
            .iter()
            .map(|member| member.value)
            .eq([operation.edge_operand_record_index()]);
        if edge_has_extra || !edge_role_ok || !edge_members_ok {
            return None;
        }

        let mut aggregate_groups = groups.iter().filter(|group| {
            native_stream(&group.id) == Some(stream)
                && group.scope_record_index == scope.record_index
                && group.record_index == operation.aggregate_group_record_index.get()
        });
        let aggregate_group = aggregate_groups.next()?;
        let aggregate_has_extra = aggregate_groups.next().is_some();
        let aggregate_role_ok = aggregate_group.role() == DesignOperandRole::ROLE_0X43;
        let aggregate_members_ok = aggregate_group
            .members()
            .iter()
            .map(|member| member.value)
            .eq([operation.aggregate_operand_record_index()]);
        if aggregate_has_extra || !aggregate_role_ok || !aggregate_members_ok {
            return None;
        }
        Some((operation, form, edge_group))
    })() else {
        return Ok(None);
    };

    let edges = crate::design::edge_resolve::resolved_hem_edge_group(
        edge_group,
        groups,
        edge_operands,
        edge_identity_operands,
        crate::history::effective_scope_previous_history_state_id(scope, histories),
        &crate::ids::neutral_feature_id(scope),
        ctx,
    )?;

    let edge_slot = match unique_feature_match(edge_operands.iter().filter(|operand| {
        native_stream(&operand.id) == native_stream(&edge_group.id)
            && operand.scope_record_index == edge_group.scope_record_index
            && operand.record_index() == operation.edge_operand_record_index()
    })) {
        Some(operand) => crate::design::edge_resolve::resolved_hem_edge_slot(
            operand,
            crate::history::effective_scope_previous_history_state_id(scope, histories),
        ),
        None => None,
    };
    let gap_length_form = edge_slot.and_then(|edge_slot| {
        crate::history::hem_gap_length_form_for_scope(scope, edge_slot, histories)
    });
    let form = match (form, gap_length_form) {
        (
            SheetMetalHemForm::GapLength { gap: _, length },
            Some(crate::history::HemGapLengthForm::Flat),
        ) => SheetMetalHemForm::Flat { length },
        (
            SheetMetalHemForm::GapLength { gap, length },
            Some(crate::history::HemGapLengthForm::Open),
        ) => SheetMetalHemForm::Open { gap, length },
        (form, _) => form,
    };
    let direction = if operation.flipped {
        SheetMetalHemDirection::Reverse
    } else {
        SheetMetalHemDirection::Forward
    };

    Ok(Some(FeatureDefinition::Operation(
        FeatureOperation::SheetMetalHem {
            edges,
            bend_position: operation.bend_position,
            form,
            direction,
            bend_radius: or_none!(cadmpeg_ir::scalar::PositiveLength::new(
                operation.bend_radius.get() * 10.0,
            )),
        },
    )))
}

pub(super) fn project_surface_stitch(
    scope: &DesignParameterScope,
    groups: &[DesignConstructionOperandGroup],
) -> Option<cadmpeg_ir::features::FeatureDefinition> {
    use cadmpeg_ir::features::{FaceSelection, FeatureDefinition, FeatureOperation};

    let operation = scope.surface_stitch_operation()?;
    let input_end = scope.reference_members().len().checked_sub(2)?;
    let mut matching = Vec::new();
    for group in groups.iter().filter(|group| {
        native_stream(&group.id) == native_stream(&scope.id)
            && group.scope_record_index == scope.record_index
    }) {
        matching.push(group);
    }
    matching.sort_by(|left, right| {
        left.scope_reference_ordinal
            .cmp(&right.scope_reference_ordinal)
    });
    if matching.len().checked_mul(2)? != input_end
        || matching
            .iter()
            .enumerate()
            .zip(
                scope
                    .reference_members()
                    .values()
                    .step_by(2)
                    .zip(scope.reference_members().values().skip(1).step_by(2)),
            )
            .any(|((ordinal, group), (group_reference, member_reference))| {
                u32::try_from(ordinal * 2) != Ok(group.scope_reference_ordinal)
                    || group.record_index != *group_reference
                    || !group
                        .members()
                        .iter()
                        .map(|member| member.value)
                        .eq([*member_reference])
                    || group.role() != DesignOperandRole::ROLE_0X5
            })
    {
        return None;
    }
    Some(FeatureDefinition::Operation(
        FeatureOperation::KnitSurface {
            faces: FaceSelection::Native(scope.id.clone()),
            merge_entities: Some(true),
            create_solid: Some(true),
            gap_tolerance: Some(cadmpeg_ir::scalar::NonNegativeLength::new(
                operation.gap_tolerance.get() * 10.0,
            )?),
        },
    ))
}

fn project_ruled_surface(
    scope: &DesignParameterScope,
    owners: &[crate::records::parameters::DesignParameterOwner],
    parameters: &[DesignParameter],
    groups: &[DesignConstructionOperandGroup],
    edge_operands: &[DesignEdgeOperand],
    edge_identity_operands: &[DesignEdgeIdentityOperand],
    ctx: &DecodeContext<'_>,
) -> Result<Option<cadmpeg_ir::features::FeatureDefinition>, CodecError> {
    use crate::records::feature::surface_ops::{
        DesignRuledSurfaceCorner, DesignRuledSurfaceMethod,
    };
    use cadmpeg_ir::features::{
        FaceSelection, FeatureDefinition, FeatureOperation, RuledSurfaceCorner, RuledSurfaceMode,
    };

    let operation = or_none!(scope.ruled_surface_operation());
    let stream = or_none!(native_stream(&scope.id));
    let parameter = |owner_record_index, source_kind: &str| {
        let mut matching = owners.iter().filter(|owner| {
            native_stream(owner.id()) == Some(stream)
                && owner.scope_record_index() == scope.record_index
                && owner.record_index() == owner_record_index
        });
        let owner = matching.next()?;
        if matching.next().is_some() {
            return None;
        }
        parameters.iter().find(|parameter| {
            native_stream(&parameter.id) == Some(stream)
                && parameter.record_index == owner.parameter_record_index()
                && parameter.source_kind() == source_kind
        })
    };
    let distance = or_none!(design_positive_length(or_none!(parameter(
        operation.distance_owner_record_index,
        "ruledDistance",
    ))));
    let angle = or_none!(design_angle(or_none!(parameter(
        operation.angle_owner_record_index,
        "ruledAngle"
    ))));
    let mode = match operation.method {
        DesignRuledSurfaceMethod::Tangent => RuledSurfaceMode::Tangent { distance },
        DesignRuledSurfaceMethod::Normal => RuledSurfaceMode::Normal { distance },
        DesignRuledSurfaceMethod::Direction => return Ok(None),
    };
    let mut ordered_groups = Vec::new();
    for record_index in &operation.edge_group_record_indices {
        let mut matching = groups.iter().filter(|group| {
            native_stream(&group.id) == Some(stream)
                && group.scope_record_index == scope.record_index
                && group.record_index == *record_index
        });
        let group = or_none!(matching.next());
        if matching.next().is_some()
            || group.role() != DesignOperandRole::BODIES_B
            || group.members().len() != 1
        {
            return Ok(None);
        }
        let reference_ordinal = or_none!(usize::try_from(group.scope_reference_ordinal).ok());
        if scope.reference_members().values().nth(reference_ordinal) != Some(record_index)
            || scope
                .reference_members()
                .values()
                .nth(reference_ordinal + 1)
                != group.members().first().map(|member| &member.value)
        {
            return Ok(None);
        }
        ordered_groups.push(group);
    }
    let mut selections = Vec::new();
    for group in &ordered_groups {
        let selection = resolved_edge_group(
            group,
            groups,
            edge_operands,
            edge_identity_operands,
            scope.previous_history_state_id(),
            &crate::ids::neutral_feature_id(scope),
            ctx,
        )?;
        selections.push(selection);
    }
    let edges = merge_edge_selections(scope, &selections);
    let support_native = scope.id.clone();
    Ok(Some(FeatureDefinition::Operation(
        FeatureOperation::RuledSurface {
            edges,
            support_faces: FaceSelection::Native(support_native),
            mode,
            angle: Some(angle),
            alternate_face: Some(operation.alternate_face),
            corner: Some(match operation.corner {
                DesignRuledSurfaceCorner::Rounded => RuledSurfaceCorner::Rounded,
                DesignRuledSurfaceCorner::Mitered => RuledSurfaceCorner::Mitered,
            }),
        },
    )))
}

fn merge_edge_selections(
    scope: &DesignParameterScope,
    selections: &[cadmpeg_ir::features::EdgeSelection],
) -> cadmpeg_ir::features::EdgeSelection {
    use cadmpeg_ir::features::EdgeSelection;

    let native = || EdgeSelection::Native(scope.id.clone());
    if selections.iter().all(|selection| {
        matches!(
            selection,
            EdgeSelection::Edges(_) | EdgeSelection::Resolved { .. }
        )
    }) {
        let mut seen = HashSet::new();
        let mut resolved = Vec::new();
        for selection in selections {
            let (EdgeSelection::Edges(edges) | EdgeSelection::Resolved { edges, .. }) = selection
            else {
                return native();
            };
            for edge in edges {
                if !seen.insert(edge) {
                    return native();
                }
                resolved.push(edge.clone());
            }
        }
        return EdgeSelection::Resolved {
            edges: resolved,
            native: scope.id.clone(),
        };
    }
    if let Some(EdgeSelection::Historical { state, .. }) = selections.first() {
        if selections.iter().all(|selection| {
            matches!(selection,
            EdgeSelection::Historical { state: candidate, .. } if candidate == state)
        }) {
            let state = state.clone();
            let mut seen = HashSet::new();
            let mut resolved = Vec::new();
            for selection in selections {
                let EdgeSelection::Historical { edges, .. } = selection else {
                    return native();
                };
                for edge in edges {
                    if !seen.insert(edge) {
                        return native();
                    }
                    resolved.push(edge.clone());
                }
            }
            let selected_native = scope.id.clone();
            return cadmpeg_ir::features::EdgeSelection::historical(
                state,
                resolved,
                selected_native,
            )
            .unwrap_or_else(|_| native());
        }
    }
    native()
}

fn matrix_axis_angle(transform: &[[f64; 4]; 4]) -> Option<cadmpeg_ir::features::AxisAngle> {
    use cadmpeg_ir::features::AxisAngle;
    use cadmpeg_ir::scalar::Angle;

    let trace = transform[0][0] + transform[1][1] + transform[2][2];
    let sine = 0.5
        * (transform[2][1] - transform[1][2])
            .hypot(transform[0][2] - transform[2][0])
            .hypot(transform[1][0] - transform[0][1]);
    let angle = sine.atan2(((trace - 1.0) * 0.5).clamp(-1.0, 1.0));
    if angle.abs() <= EPS_FEATURE_PROJECT_MATRIX_AXIS_ANGLE_E12 {
        return None;
    }
    let (x, y, z) =
        if (std::f64::consts::PI - angle).abs() <= EPS_FEATURE_PROJECT_MATRIX_AXIS_ANGLE_E8 {
            // The largest diagonal supplies a nonzero axis component at a half-turn.
            // Recover the other signs from its row; the X component may be zero.
            let pivot = (0..3).max_by(|&a, &b| transform[a][a].total_cmp(&transform[b][b]))?;
            let mut axis = [0.0; 3];
            axis[pivot] = ((transform[pivot][pivot] + 1.0) * 0.5).max(0.0).sqrt();
            if axis[pivot] == 0.0 {
                return None;
            }
            for other in 0..3 {
                if other != pivot {
                    axis[other] =
                        (transform[pivot][other] + transform[other][pivot]) / (4.0 * axis[pivot]);
                }
            }
            // Away from an exact half-turn the skew terms still state the
            // orientation. The diagonal alone cannot distinguish opposite axes.
            let skew = [
                transform[2][1] - transform[1][2],
                transform[0][2] - transform[2][0],
                transform[1][0] - transform[0][1],
            ];
            if axis
                .iter()
                .zip(skew)
                .map(|(axis, skew)| axis * skew)
                .sum::<f64>()
                < 0.0
            {
                axis = axis.map(|value| -value);
            }
            (axis[0], axis[1], axis[2])
        } else {
            let scale = 2.0 * angle.sin();
            (
                (transform[2][1] - transform[1][2]) / scale,
                (transform[0][2] - transform[2][0]) / scale,
                (transform[1][0] - transform[0][1]) / scale,
            )
        };
    let norm = x.hypot(y).hypot(z);
    (norm > EPS_FEATURE_PROJECT_MATRIX_AXIS_ANGLE_E12).then_some(AxisAngle {
        origin: cadmpeg_ir::features::FinitePoint3::ZERO,
        direction: cadmpeg_ir::features::FeatureDirection3::new(Vector3::new(
            x / norm,
            y / norm,
            z / norm,
        ))?,
        angle: Angle::new(angle)?,
    })
}

pub(crate) fn direct_face_selection(
    scope: &DesignParameterScope,
    operands: &[DesignFaceOperand],
) -> Option<cadmpeg_ir::features::FaceSelection> {
    use cadmpeg_ir::features::FaceSelection;

    let mut matching = Vec::new();
    for operand in operands.iter().filter(|operand| {
        native_stream(&operand.id) == native_stream(&scope.id)
            && operand.scope_record_index == scope.record_index
    }) {
        matching.push(operand);
    }
    matching.sort_by(|left, right| {
        left.scope_reference_ordinal
            .cmp(&right.scope_reference_ordinal)
    });
    if matching.is_empty() {
        return None;
    }
    let mut members = Vec::new();
    for operand in &matching {
        members.push((operand.id.as_str(), operand.resolved_face_slots.as_slice()));
    }
    let feature_id = crate::ids::neutral_feature_id(scope);
    let historical_face = |previous_state_id, slot| {
        crate::ids::history_input_face_id(
            &crate::ids::feature_input_prefix(&feature_id, previous_state_id),
            slot,
        )
    };
    let faces = match scope.previous_history_state_id() {
        Some(previous_state_id) if members.iter().all(|(_, faces)| !faces.is_empty()) => {
            let mut resolved = Vec::new();
            for slot in members.iter().flat_map(|(_, faces)| faces.iter().copied()) {
                let face = historical_face(previous_state_id, slot);
                if !resolved.contains(&face) {
                    resolved.push(face);
                }
            }
            match cadmpeg_ir::features::FaceSelection::historical(
                crate::ids::feature_input_topology_id(&feature_id, previous_state_id),
                resolved,
                scope.id.clone(),
            ) {
                Ok(selection) => selection,
                Err(_) => FaceSelection::Native(scope.id.clone()),
            }
        }
        Some(previous_state_id) if members.iter().any(|(_, faces)| !faces.is_empty()) => {
            let mut faces = Vec::new();
            let mut unresolved = Vec::new();
            for (identity, slots) in &members {
                if slots.is_empty() {
                    unresolved.push((*identity).to_owned());
                } else {
                    for slot in *slots {
                        let face = historical_face(previous_state_id, *slot);
                        if !faces.contains(&face) {
                            faces.push(face);
                        }
                    }
                }
            }
            match cadmpeg_ir::features::FaceSelection::historical_partial(
                crate::ids::feature_input_topology_id(&feature_id, previous_state_id),
                faces,
                unresolved,
                scope.id.clone(),
            ) {
                Ok(selection) => selection,
                Err(_) => FaceSelection::Native(scope.id.clone()),
            }
        }
        _ => FaceSelection::Native(scope.id.clone()),
    };
    Some(faces)
}

/// Local work cap for ordering the parameters of one owner scope.
///
/// Each ordering round scans every unresolved parameter and its dependencies,
/// so a dependency chain makes the ordering quadratic in the scope's
/// parameter count.
pub(crate) const PARAMETER_ORDER_WORK: usize = 1 << 22;

/// Order every owner scope's parameters and return how many scopes exceeded
/// [`PARAMETER_ORDER_WORK`].
fn normalize_parameter_ordinals(
    parameters: &mut [cadmpeg_ir::features::DesignParameter],
    owners: &HashMap<cadmpeg_ir::features::ParameterId, Option<cadmpeg_ir::features::FeatureId>>,
) -> Result<usize, CodecError> {
    let mut groups = HashMap::<Option<cadmpeg_ir::features::FeatureId>, Vec<usize>>::new();
    for (index, parameter) in parameters.iter().enumerate() {
        if let Some(indices) = groups.get_mut(&parameter.owner) {
            indices.push(index);
        } else {
            let owner = parameter.owner.clone();
            let indices = vec![index];
            // discarded-value: a new owner group has no previous member list.
            let _ = groups.insert(owner, indices);
        }
    }
    let mut unordered_scopes = 0;
    for (owner, indices) in groups {
        if !order_parameter_scope(
            parameters,
            owners,
            owner.as_ref(),
            &indices,
            &cadmpeg_core::decode::WorkBudget::new(PARAMETER_ORDER_WORK),
        )? {
            unordered_scopes += 1;
        }
    }
    Ok(unordered_scopes)
}

/// Reassign the ordinals of one owner scope so each parameter follows its
/// same-scope dependencies, breaking cycles at their lowest-ordinal member.
///
/// Returns `false` when the ordering exhausts `budget`. The scope then keeps
/// its unique ordinal slots and loses its same-scope dependency edges, so the
/// projection stays valid. Decode reports the skipped scope as
/// `parameter.order-budget-exceeded` and the dropped edges as unbound
/// expression symbols.
fn order_parameter_scope(
    parameters: &mut [cadmpeg_ir::features::DesignParameter],
    owners: &HashMap<cadmpeg_ir::features::ParameterId, Option<cadmpeg_ir::features::FeatureId>>,
    owner: Option<&cadmpeg_ir::features::FeatureId>,
    indices: &[usize],
    budget: &cadmpeg_core::decode::WorkBudget,
) -> Result<bool, CodecError> {
    let mut source_order = indices.to_vec();
    source_order.sort_unstable_by(|a, b| {
        let a = &parameters[*a];
        let b = &parameters[*b];
        (a.ordinal, &a.id).cmp(&(b.ordinal, &b.id))
    });
    let mut ordinals = Vec::new();
    let mut previous: Option<u32> = None;
    for index in source_order {
        let parameter = &mut parameters[index];
        if let Some(previous) = previous {
            if parameter.ordinal <= previous {
                parameter.ordinal = previous
                    .checked_add(1)
                    .ok_or_else(|| CodecError::malformed("Design parameter ordinal exceeds u32"))?;
            }
        }
        ordinals.push(parameter.ordinal);
        previous = Some(parameter.ordinal);
    }
    let mut unresolved = HashSet::new();
    for index in indices {
        // discarded-value: each parameter index occurs once in its owner group.
        let _ = unresolved.insert(*index);
    }
    let mut resolved = HashSet::<cadmpeg_ir::features::ParameterId>::new();
    let mut order = Vec::new();
    while !unresolved.is_empty() {
        let mut ready = Vec::new();
        for index in &unresolved {
            let dependencies = &parameters[*index].dependencies;
            // The round charge also covers the component search of a
            // cycle-breaking round, which walks the same graph.
            if !budget.charge_by(1 + dependencies.len()) {
                break;
            }
            if dependencies.iter().all(|dependency| {
                owners.get(dependency).map(Option::as_ref) != Some(owner)
                    || resolved.contains(dependency)
            }) {
                ready.push(*index);
            }
        }
        if budget.exhausted() {
            for index in indices {
                parameters[*index]
                    .dependencies
                    .retain(|dependency| owners.get(dependency).map(Option::as_ref) != Some(owner));
            }
            return Ok(false);
        }
        ready.sort_by(|a, b| {
            let pa = &parameters[*a];
            let pb = &parameters[*b];
            pa.ordinal.cmp(&pb.ordinal).then_with(|| pa.id.cmp(&pb.id))
        });
        if ready.is_empty() {
            // Every blocked parameter has an unresolved dependency, so the
            // remaining graph contains a cycle. Break its lowest-ordinal
            // member, then resume ordering before selecting another cycle.
            let cycle = cyclic_parameter_components(parameters, &unresolved)
                .into_iter()
                .map(|(first, remaining)| {
                    let breaker = remaining.iter().copied().fold(first, |best, index| {
                        let candidate = &parameters[index];
                        let current = &parameters[best];
                        if (candidate.ordinal, &candidate.id) < (current.ordinal, &current.id) {
                            index
                        } else {
                            best
                        }
                    });
                    (breaker, first, remaining)
                })
                .min_by(|(a, _, _), (b, _, _)| {
                    let a = &parameters[*a];
                    let b = &parameters[*b];
                    (a.ordinal, &a.id).cmp(&(b.ordinal, &b.id))
                });
            if let Some((breaker, first, remaining)) = cycle {
                let mut members = HashSet::new();
                for index in std::iter::once(first).chain(remaining) {
                    let id = parameters[index].id.clone();
                    // discarded-value: component indices are distinct.
                    let _ = members.insert(id);
                }
                parameters[breaker]
                    .dependencies
                    .retain(|dependency| !members.contains(dependency));
            }
            continue;
        }
        for index in ready {
            unresolved.remove(&index);
            let id = parameters[index].id.clone();
            // discarded-value: a parameter is resolved once.
            let _ = resolved.insert(id);
            order.push(index);
        }
    }
    for (index, ordinal) in order.into_iter().zip(ordinals) {
        parameters[index].ordinal = ordinal;
    }
    Ok(true)
}

/// Strongly connected components of the unresolved parameter graph that contain a cycle.
/// Each component retains one member separately from its remaining members.
fn cyclic_parameter_components(
    parameters: &[cadmpeg_ir::features::DesignParameter],
    unresolved: &HashSet<usize>,
) -> Vec<(usize, Vec<usize>)> {
    enum Visit {
        Enter(usize),
        Leave(usize),
    }

    let mut indices = Vec::new();
    for index in unresolved {
        indices.push(*index);
    }
    let mut local_by_id = HashMap::new();
    for (local, index) in indices.iter().enumerate() {
        // discarded-value: distinct parameter IDs retain one local index.
        let _ = local_by_id.insert(&parameters[*index].id, local);
    }
    let mut edges = Vec::new();
    for index in &indices {
        let mut dependencies = Vec::new();
        for dependency in &parameters[*index].dependencies {
            if let Some(local) = local_by_id.get(dependency) {
                dependencies.push(*local);
            }
        }
        edges.push(dependencies);
    }
    let mut incoming = Vec::new();
    for _ in &indices {
        incoming.push(Vec::new());
    }
    for (source, dependencies) in edges.iter().enumerate() {
        for &target in dependencies {
            incoming[target].push(source);
        }
    }

    // Iterative graph walks keep long source dependency chains off the call stack.
    let mut visited = HashSet::new();
    let mut finished = Vec::new();
    for root in 0..indices.len() {
        if visited.contains(&root) {
            continue;
        }
        let mut pending = Vec::new();
        pending.push(Visit::Enter(root));
        while let Some(visit) = pending.pop() {
            match visit {
                Visit::Enter(node) => {
                    if !visited.insert(node) {
                        continue;
                    }
                    pending.push(Visit::Leave(node));
                    for dependency in &edges[node] {
                        pending.push(Visit::Enter(*dependency));
                    }
                }
                Visit::Leave(node) => {
                    finished.push(node);
                }
            }
        }
    }

    let mut assigned = HashSet::new();
    let mut cycles = Vec::new();
    for root in finished.into_iter().rev() {
        if !assigned.insert(root) {
            continue;
        }
        let mut remaining_members = Vec::new();
        let mut pending = Vec::new();
        pending.push(root);
        while let Some(node) = pending.pop() {
            for &source in &incoming[node] {
                if assigned.insert(source) {
                    remaining_members.push(indices[source]);
                    pending.push(source);
                }
            }
        }
        if !remaining_members.is_empty() || edges[root].contains(&root) {
            cycles.push((indices[root], remaining_members));
        }
    }
    cycles
}

fn design_positive_length(
    parameter: &DesignParameter,
) -> Option<cadmpeg_ir::scalar::PositiveLength> {
    cadmpeg_ir::scalar::PositiveLength::try_from(design_length(parameter)?).ok()
}

pub(super) fn design_length(parameter: &DesignParameter) -> Option<cadmpeg_ir::scalar::Length> {
    let value = parameter.evaluated_value().get() * 10.0;
    (parameter
        .unit()
        .map(|field| field.value.as_str())
        .is_some_and(design_length_unit))
    .then_some(cadmpeg_ir::scalar::Length::new(value)?)
}

pub(crate) fn design_length_unit(unit: &str) -> bool {
    matches!(unit, "mm" | "cm" | "m" | "in" | "ft")
}

pub(super) fn design_angle_unit(unit: &str) -> bool {
    matches!(unit, "deg" | "rad")
}

pub(super) fn design_dimension_unit(parameter: &DesignParameter) -> bool {
    let unit = parameter.unit().map(|field| field.value.as_str());
    if parameter.source_kind_str().starts_with("Linear Dimension")
        || parameter.source_kind_str().starts_with("Radius Dimension")
        || parameter.source_kind_str().starts_with("Radial Dimension")
        || parameter
            .source_kind_str()
            .starts_with("Diameter Dimension")
    {
        return unit.is_some_and(design_length_unit);
    }
    if parameter.source_kind_str().starts_with("Angular Dimension") {
        return unit.is_some_and(design_angle_unit);
    }
    if parameter.source_kind_str().starts_with("Tangent Dimension") {
        return unit.is_some_and(design_length_unit);
    }
    false
}

fn project_variable_fillet(
    scope: &DesignParameterScope,
    parameters: &[(u32, &DesignParameter)],
    inputs: &ProjectInputs<'_>,
    ctx: &DecodeContext<'_>,
) -> Result<Option<cadmpeg_ir::features::FeatureDefinition>, CodecError> {
    use cadmpeg_ir::features::{
        edge_treatments::{FilletGroup, RadiusSpec},
        FeatureDefinition, FeatureOperation,
    };

    let construction_groups = inputs.construction_groups;
    let edge_operands = inputs.edge_operands;
    let edge_identity_operands = inputs.edge_identity_operands;
    let edge_treatment_vertex_operands = inputs.edge_treatment_vertex_operands;
    let histories = inputs.histories;
    let stream = or_none!(native_stream(&scope.id));
    let mut groups = construction_groups.iter().filter(|group| {
        native_stream(&group.id) == Some(stream) && group.scope_record_index == scope.record_index
    });
    let Some(group) = groups.next() else {
        return Ok(None);
    };
    if groups.next().is_some() {
        return Ok(None);
    }
    let (points, tangency_weight) = or_none!(variable_fillet_law(parameters));
    Ok(Some(FeatureDefinition::Operation(
        FeatureOperation::Fillet {
            groups: cadmpeg_ir::features::NonEmptyMembers::one(FilletGroup {
                edges: resolved_edge_treatment_group_with_corners(
                    group,
                    crate::design::edge_resolve::EdgeTreatmentInputs {
                        groups: construction_groups,
                        operands: edge_operands,
                        identity_operands: edge_identity_operands,
                        vertex_operands: edge_treatment_vertex_operands,
                        histories,
                        previous_state_id: scope.previous_history_state_id(),
                        feature_id: &crate::ids::neutral_feature_id(scope),
                        treatment_radius: None,
                    },
                    ctx,
                )?,
                radius: RadiusSpec::Variable { points },
                tangency_weight,
            }),
        },
    )))
}

fn variable_fillet_law(
    parameters: &[(u32, &DesignParameter)],
) -> Option<(
    cadmpeg_ir::features::edge_treatments::VariableRadii,
    Option<cadmpeg_ir::scalar::FiniteReal>,
)> {
    use cadmpeg_ir::features::edge_treatments::VariableRadius;

    let unique_parameter = |kind: &str| {
        let mut matches = parameters
            .iter()
            .filter_map(|(_, parameter)| (parameter.source_kind() == kind).then_some(*parameter));
        let parameter = matches.next()?;
        matches.next().is_none().then_some(parameter)
    };
    let start = unique_parameter("StartRadius").and_then(design_length)?;
    let end = unique_parameter("EndRadius").and_then(design_length)?;
    let tangency_weight = {
        let mut matches = parameters.iter().filter_map(|(_, parameter)| {
            (parameter.source_kind() == "TangencyWeight").then_some(*parameter)
        });
        match (matches.next(), matches.next()) {
            (None, None) => None,
            (Some(parameter), None) => Some(parameter.evaluated_value()),
            (None, Some(_)) => return None,
            (Some(_), Some(_)) => return None,
        }
    };
    let mut middle_radii = Vec::new();
    let mut middle_parameters = Vec::new();
    for (ordinal, parameter) in parameters {
        match parameter.source_kind_str() {
            "MidRadius" => middle_radii.push((*ordinal, *parameter)),
            "MidParams" => middle_parameters.push((*ordinal, *parameter)),
            _ => {}
        }
    }
    middle_radii.sort_by_key(|left| left.0);
    middle_parameters.sort_by_key(|left| left.0);
    if middle_radii.len() != middle_parameters.len()
        || parameters.iter().any(|(_, parameter)| {
            !matches!(
                parameter.source_kind_str(),
                "StartRadius" | "EndRadius" | "MidRadius" | "MidParams" | "TangencyWeight"
            )
        })
    {
        return None;
    }
    let mut points = Vec::new();
    points.push(VariableRadius {
        parameter: 0.0,
        radius: start,
    });
    for ((_, radius), (_, parameter)) in middle_radii.into_iter().zip(middle_parameters) {
        let radius = design_length(radius)?;
        let parameter = parameter.evaluated_value().get();
        points.push(VariableRadius { parameter, radius });
    }
    points.push(VariableRadius {
        parameter: 1.0,
        radius: end,
    });
    cadmpeg_ir::features::edge_treatments::VariableRadii::new(points)
        .ok()
        .map(|points| (points, tangency_weight))
}

fn fillet_law_parameter_records(law: &DesignFilletRadiusLaw) -> impl Iterator<Item = u32> + '_ {
    let (first, second, middle) = match law {
        DesignFilletRadiusLaw::Constant {
            radius_parameter_record_index,
        } => (Some(*radius_parameter_record_index), None, None),
        DesignFilletRadiusLaw::Chordal {
            chord_length_parameter_record_index,
        } => (Some(*chord_length_parameter_record_index), None, None),
        DesignFilletRadiusLaw::Asymmetric {
            offset_one_parameter_record_index,
            offset_two_parameter_record_index,
        } => (
            Some(*offset_one_parameter_record_index),
            Some(*offset_two_parameter_record_index),
            None,
        ),
        DesignFilletRadiusLaw::Variable {
            start_radius_parameter_record_index,
            end_radius_parameter_record_index,
            middle,
        } => (
            Some(*start_radius_parameter_record_index),
            Some(*end_radius_parameter_record_index),
            Some(middle.as_slice()),
        ),
    };
    [first, second]
        .into_iter()
        .flatten()
        .chain(
            middle
                .into_iter()
                .flatten()
                .map(|row| row.radius_parameter_record_index),
        )
        .chain(
            middle
                .into_iter()
                .flatten()
                .map(|row| row.parameter_record_index),
        )
}

/// Count parameters whose unit token has no settled neutral quantity kind.
pub(crate) fn untyped_parameter_unit_count(parameters: &[DesignParameter]) -> usize {
    parameters
        .iter()
        .filter(|parameter| {
            parameter
                .unit()
                .map(|field| field.value.as_str())
                .is_some_and(|unit| !design_length_unit(unit) && !design_angle_unit(unit))
        })
        .count()
}

fn project_chamfer(
    scope: &DesignParameterScope,
    parameters: &[(u32, &DesignParameter)],
    inputs: &ProjectInputs<'_>,
    ctx: &DecodeContext<'_>,
) -> Result<Option<cadmpeg_ir::features::FeatureDefinition>, CodecError> {
    enum Lanes<'a> {
        Distance(&'a [&'a DesignParameter]),
        TwoDistances(&'a [&'a DesignParameter], &'a [&'a DesignParameter]),
        DistanceAngle(&'a [&'a DesignParameter], &'a [&'a DesignParameter]),
    }
    use cadmpeg_ir::features::{
        edge_treatments::{ChamferGroup, ChamferSpec},
        FeatureDefinition, FeatureOperation,
    };

    let construction_groups = inputs.construction_groups;
    let edge_operands = inputs.edge_operands;
    let edge_identity_operands = inputs.edge_identity_operands;
    let edge_treatment_vertex_operands = inputs.edge_treatment_vertex_operands;
    let histories = inputs.histories;
    let native_scope = native_stream(&scope.id);
    let mut edge_groups = Vec::new();
    for group in construction_groups.iter().filter(|group| {
        native_stream(&group.id) == native_scope
            && group.scope_record_index == scope.record_index
            && group.extrude_role().is_none()
    }) {
        edge_groups.push(group);
    }
    edge_groups.sort_by(|left, right| {
        left.scope_reference_ordinal
            .cmp(&right.scope_reference_ordinal)
    });
    let group_count = edge_groups.len();
    let ordered_parameters =
        |matches_kind: &dyn Fn(&str) -> bool| -> Result<Vec<&DesignParameter>, CodecError> {
            {
                let mut matches = Vec::new();
                for parameter in parameters
                    .iter()
                    .filter(|(_, parameter)| matches_kind(parameter.source_kind_str()))
                    .copied()
                {
                    matches.push(parameter);
                }
                matches.sort_by_key(|left| left.0);
                let mut out = Vec::new();
                for (_, parameter) in matches {
                    out.push(parameter);
                }
                Ok(out)
            }
        };
    let distances = ordered_parameters(&|kind| kind == "Distance")?;
    let first_distances = ordered_parameters(&|kind| kind == "Distance 1")?;
    let second_distances = ordered_parameters(&|kind| kind == "Distance 2")?;
    let left_distances = ordered_parameters(&|kind| kind == "leftDistance")?;
    let right_distances = ordered_parameters(&|kind| kind == "rightDistance")?;
    let angles =
        ordered_parameters(&|kind| matches!(kind, "Angle" | "Rotate Angle" | "rotateAngle"))?;
    if !parameters.iter().all(|(_, parameter)| {
        matches!(
            parameter.source_kind_str(),
            "Distance"
                | "Distance 1"
                | "Distance 2"
                | "leftDistance"
                | "rightDistance"
                | "Angle"
                | "Rotate Angle"
                | "rotateAngle"
        )
    }) {
        return Ok(None);
    }

    let lanes = if !left_distances.is_empty() || !right_distances.is_empty() {
        if !distances.is_empty() || !first_distances.is_empty() || !second_distances.is_empty() {
            return Ok(None);
        }
        if right_distances.is_empty() && !angles.is_empty() {
            if left_distances.len() != group_count || angles.len() != group_count {
                return Ok(None);
            }
            Lanes::DistanceAngle(&left_distances, &angles)
        } else if right_distances.is_empty() {
            if left_distances.len() != group_count {
                return Ok(None);
            }
            Lanes::Distance(&left_distances)
        } else {
            if !angles.is_empty()
                || left_distances.len() != group_count
                || right_distances.len() != group_count
            {
                return Ok(None);
            }
            Lanes::TwoDistances(&left_distances, &right_distances)
        }
    } else if !first_distances.is_empty() || !second_distances.is_empty() {
        if !distances.is_empty()
            || !angles.is_empty()
            || first_distances.len() != group_count
            || second_distances.len() != group_count
        {
            return Ok(None);
        }
        Lanes::TwoDistances(&first_distances, &second_distances)
    } else if !angles.is_empty() {
        if distances.len() != group_count || angles.len() != group_count {
            return Ok(None);
        }
        Lanes::DistanceAngle(&distances, &angles)
    } else if !distances.is_empty() {
        if distances.len() != group_count {
            return Ok(None);
        }
        Lanes::Distance(&distances)
    } else {
        return Ok(None);
    };
    let mut candidates = Vec::new();
    for ordinal in 0..group_count {
        let spec = match lanes {
            Lanes::Distance(distance) => design_positive_length(distance[ordinal])
                .map(|distance| ChamferSpec::Distance { distance }),
            Lanes::TwoDistances(first, second) => design_positive_length(first[ordinal])
                .zip(design_positive_length(second[ordinal]))
                .map(|(first, second)| ChamferSpec::TwoDistances { first, second }),
            Lanes::DistanceAngle(distance, angle) => design_positive_length(distance[ordinal])
                .zip(
                    design_angle(angle[ordinal])
                        .and_then(|value| cadmpeg_ir::scalar::InteriorAngle::try_from(value).ok()),
                )
                .map(|(distance, angle)| ChamferSpec::DistanceAngle { distance, angle }),
        };
        let spec = or_none!(spec);
        candidates.push(spec);
    }
    let mut groups = Vec::new();
    for (spec, group) in candidates.into_iter().zip(edge_groups) {
        let group = ChamferGroup {
            edges: resolved_edge_treatment_group_with_corners(
                group,
                crate::design::edge_resolve::EdgeTreatmentInputs {
                    groups: construction_groups,
                    operands: edge_operands,
                    identity_operands: edge_identity_operands,
                    vertex_operands: edge_treatment_vertex_operands,
                    histories,
                    previous_state_id: scope.previous_history_state_id(),
                    feature_id: &crate::ids::neutral_feature_id(scope),
                    treatment_radius: None,
                },
                ctx,
            )?,
            spec,
        };
        groups.push(group);
    }
    Ok(Some(FeatureDefinition::Operation(
        FeatureOperation::Chamfer {
            groups: or_none!(groups.try_into().ok()),
            flip_direction: false,
        },
    )))
}

fn project_fixed_chamfer(
    scope: &DesignParameterScope,
    construction_groups: &[DesignConstructionOperandGroup],
    edge_operands: &[DesignEdgeOperand],
    edge_identity_operands: &[DesignEdgeIdentityOperand],
    edge_treatment_vertex_operands: &[DesignEdgeTreatmentVertexOperand],
    histories: &[crate::history_records::AsmHistory],
    ctx: &DecodeContext<'_>,
) -> Result<Option<cadmpeg_ir::features::FeatureDefinition>, CodecError> {
    use cadmpeg_ir::features::{
        edge_treatments::{ChamferGroup, ChamferSpec},
        FeatureDefinition, FeatureOperation,
    };

    let fixed = or_none!(scope.fixed_chamfer_parameters());
    let stream = or_none!(native_stream(&scope.id));
    let group = or_none!(unique_feature_match(construction_groups.iter().filter(
        |group| {
            native_stream(&group.id) == Some(stream)
                && group.scope_record_index == scope.record_index
        }
    )));
    let spec = match fixed {
        crate::records::feature::fixed_parameters::DesignFixedChamferParameters::EqualDistance { distance } => {
            ChamferSpec::Distance {
                distance: or_none!(cadmpeg_ir::scalar::PositiveLength::new(distance.value.get() * 10.0)),
            }
        }
        crate::records::feature::fixed_parameters::DesignFixedChamferParameters::TwoDistances { first, second } => {
            ChamferSpec::TwoDistances {
                first: or_none!(cadmpeg_ir::scalar::PositiveLength::new(first.value.get() * 10.0)),
                second: or_none!(cadmpeg_ir::scalar::PositiveLength::new(second.value.get() * 10.0)),
            }
        }
    };
    Ok(Some(FeatureDefinition::Operation(
        FeatureOperation::Chamfer {
            groups: cadmpeg_ir::features::NonEmptyMembers::one(ChamferGroup {
                edges: resolved_edge_treatment_group_with_corners(
                    group,
                    crate::design::edge_resolve::EdgeTreatmentInputs {
                        groups: construction_groups,
                        operands: edge_operands,
                        identity_operands: edge_identity_operands,
                        vertex_operands: edge_treatment_vertex_operands,
                        histories,
                        previous_state_id: scope.previous_history_state_id(),
                        feature_id: &crate::ids::neutral_feature_id(scope),
                        treatment_radius: None,
                    },
                    ctx,
                )?,
                spec,
            }),
            flip_direction: false,
        },
    )))
}

fn fixed_boolean_operation(operation: DesignExtrudeOperation) -> cadmpeg_ir::features::BooleanOp {
    match operation {
        DesignExtrudeOperation::Join => cadmpeg_ir::features::BooleanOp::Join,
        DesignExtrudeOperation::Cut => cadmpeg_ir::features::BooleanOp::Cut,
        DesignExtrudeOperation::Intersect => cadmpeg_ir::features::BooleanOp::Intersect,
        DesignExtrudeOperation::NewBody => cadmpeg_ir::features::BooleanOp::NewBody,
    }
}

pub(super) fn project_fixed_revolve_with_entities(
    scope: &DesignParameterScope,
    construction_groups: &[DesignConstructionOperandGroup],
    edge_operands: &[DesignEdgeOperand],
    entity_selection_operands: &[crate::records::topology::entity_selection::DesignEntitySelectionOperand],
    face_operands: &[crate::records::topology::face::DesignFaceOperand],
    (placements, curve_identities): (&[DesignSketchPlacement], &[SketchCurveIdentity]),
) -> Option<cadmpeg_ir::features::FeatureDefinition> {
    use cadmpeg_ir::features::{
        AngularTermination, FeatureDefinition, FeatureOperation, PlanarProfileRef, RevolutionAxis,
        RevolveConstruction, RevolveExtent,
    };

    let crate::records::feature::scope::DesignScopePayload::Revolve(Some(
        crate::records::feature::path_features::DesignRevolveConstruction {
            operation,
            travel,
            second_side,
            solid,
            ..
        },
    )) = &scope.payload()
    else {
        return None;
    };
    let stream = native_stream(&scope.id)?;
    let mut profile = None;
    let mut axis_group = None;
    let mut targets = Vec::new();
    let mut body_count = 0;
    let mut group_count = 0;
    for group in construction_groups.iter().filter(|group| {
        native_stream(&group.id) == Some(stream) && group.scope_record_index == scope.record_index
    }) {
        group_count += 1;
        match group.role() {
            DesignOperandRole::PROFILE => {
                if profile.replace(group).is_some() {
                    return None;
                }
            }
            DesignOperandRole::ROLE_0X21 => {
                if axis_group.replace(group).is_some() {
                    return None;
                }
            }
            DesignOperandRole::BODIES_A | DesignOperandRole::BODIES_B => body_count += 1,
            DesignOperandRole::ROLE_0X5 => targets.push(group),
            _ => {}
        }
    }
    let profile = profile?;
    let axis_group = axis_group?;
    let expected_body_groups = usize::from(*operation != DesignExtrudeOperation::NewBody);
    let target_group = |record: &crate::records::identity::Located<u32>| {
        targets
            .iter()
            .find(|group| group.record_index == record.value && group.members().len() == 1)
            .copied()
    };
    let (target_extent, expected_target_groups) = match travel {
        crate::records::feature::path_features::DesignRevolveTravel::Angle(_) => (None, 0),
        crate::records::feature::path_features::DesignRevolveTravel::ToTargets(sides) => {
            let side_one = target_group(&sides.side_one)?;
            match &sides.side_two {
                None => (
                    Some(RevolveExtent::OneSided {
                        termination: AngularTermination::ToShape {
                            target: cadmpeg_ir::features::FaceSelection::Native(
                                side_one.id.clone(),
                            ),
                        },
                    }),
                    1,
                ),
                // The frame stores no side for either target.
                Some(side_two) => {
                    target_group(side_two)?;
                    (
                        Some(RevolveExtent::TwoSided {
                            first: AngularTermination::Unresolved {},
                            second: AngularTermination::Unresolved {},
                        }),
                        2,
                    )
                }
            }
        }
    };
    if body_count != expected_body_groups
        || group_count != 2 + expected_body_groups + expected_target_groups
    {
        return None;
    }
    let [_] = profile.members() else {
        return None;
    };
    let [crate::records::identity::Located {
        value: axis_member, ..
    }] = axis_group.members()
    else {
        return None;
    };
    let mut matching = edge_operands.iter().filter(|operand| {
        native_stream(&operand.id) == Some(stream)
            && operand.scope_record_index == scope.record_index
            && operand.record_index() == *axis_member
    });
    let first = matching.next();
    let second = matching.next();
    let axis = if let (Some(axis_operand), None) = (first, second) {
        Some(RevolutionAxis {
            origin: axis_operand.resolved_axis?.origin,
            direction: cadmpeg_ir::features::FeatureDirection3::from(
                axis_operand.resolved_axis?.direction,
            ),
            reference: None,
        })
    } else if first.is_none() {
        let resolved = resolve_sketch_axis_selection(
            scope,
            axis_group,
            *axis_member,
            entity_selection_operands,
            placements,
            curve_identities,
        )
        .or_else(|| {
            design_geometry_axis_selection(
                scope,
                axis_group,
                *axis_member,
                entity_selection_operands,
                placements,
            )
        });
        if resolved.is_none()
            && !unresolved_historical_face_axis_selection(
                scope,
                axis_group,
                *axis_member,
                entity_selection_operands,
            )
            && revolve_face_axis_operand(scope, axis_group, *axis_member, face_operands).is_none()
        {
            return None;
        }
        resolved
    } else {
        return None;
    };
    let revolve_profile: cadmpeg_ir::features::PlanarProfileRef =
        PlanarProfileRef::Native(profile.id.clone());
    let extent = match (travel, second_side) {
        (crate::records::feature::path_features::DesignRevolveTravel::ToTargets(_), _) => {
            target_extent?
        }
        (crate::records::feature::path_features::DesignRevolveTravel::Angle(angle), None) => {
            RevolveExtent::OneSided {
                termination: AngularTermination::Angle { angle: *angle },
            }
        }
        // Fusion's symmetric angle applies to each side; the neutral extent
        // states the total travel.
        (crate::records::feature::path_features::DesignRevolveTravel::Angle(angle), Some(side))
            if side.symmetric =>
        {
            RevolveExtent::Symmetric {
                termination: AngularTermination::Angle {
                    angle: cadmpeg_ir::scalar::PositiveAngle::new(2.0 * angle.get())?,
                },
            }
        }
        (crate::records::feature::path_features::DesignRevolveTravel::Angle(angle), Some(side)) => {
            RevolveExtent::TwoSided {
                first: AngularTermination::Angle { angle: *angle },
                second: AngularTermination::Angle { angle: side.angle },
            }
        }
    };
    Some(FeatureDefinition::Operation(FeatureOperation::Revolve {
        construction: match axis {
            Some(axis) => RevolveConstruction::Resolved {
                profile: revolve_profile,
                axis,
                extent,
                solid: *solid,
                face_maker: None,
                fuse_order: None,
                allow_multi_profile_faces: None,
            },
            None => RevolveConstruction::Unresolved(
                cadmpeg_ir::features::PartialRevolveConstruction::Axis {
                    profile: revolve_profile,
                    extent: Some(extent),
                    solid: *solid,
                    face_maker: None,
                    fuse_order: None,
                    allow_multi_profile_faces: None,
                },
            ),
        },
        op: fixed_boolean_operation(*operation),
    }))
}

fn unresolved_historical_face_axis_selection(
    scope: &DesignParameterScope,
    axis_group: &DesignConstructionOperandGroup,
    axis_member: u32,
    entity_selection_operands: &[crate::records::topology::entity_selection::DesignEntitySelectionOperand],
) -> bool {
    let stream = native_stream(&scope.id);
    let selection = unique_feature_match(entity_selection_operands.iter().filter(|operand| {
        native_stream(&operand.id) == stream
            && operand.scope_record_index == scope.record_index
            && operand.group_record_index == axis_group.record_index
            && operand.group_member_ordinal == 0
            && operand.record_index() == axis_member
    }));
    selection.is_some_and(|selection| !selection.historical_face_candidates.is_empty())
}

fn revolve_face_axis_operand<'a>(
    scope: &DesignParameterScope,
    axis_group: &DesignConstructionOperandGroup,
    axis_member: u32,
    face_operands: &'a [crate::records::topology::face::DesignFaceOperand],
) -> Option<&'a crate::records::topology::face::DesignFaceOperand> {
    let stream = native_stream(&scope.id);
    let operand = unique_feature_match(face_operands.iter().filter(|operand| {
        native_stream(&operand.id) == stream
            && operand.scope_record_index == scope.record_index
            && operand.group_record_index() == Some(axis_group.record_index)
            && operand.group_member_ordinal() == Some(0)
            && operand.record_index() == axis_member
    }))?;
    (crate::design::face_resolve::historical_face_operand_candidate_iter(operand)
        .next()
        .is_some())
    .then_some(operand)
}

/// Orient partial-turn axes using the source sweep convention.
pub(crate) fn orient_revolve_axes(
    features: &mut [cadmpeg_ir::features::Feature],
    scopes: &[DesignParameterScope],
    sketches: &[cadmpeg_ir::sketches::Sketch],
    sketch_entities: &[cadmpeg_ir::sketches::SketchEntity],
    (linear_tolerance, angular_tolerance): (f64, f64),
) {
    use cadmpeg_ir::features::{
        AngularTermination, FeatureDefinition, FeatureOperation, PlanarProfileRef, RevolveExtent,
    };

    for feature in features {
        let reversed = scopes.iter().any(|scope| {
            feature.native_ref.as_deref() == Some(scope.id.as_str())
                && matches!(
                    scope.payload(),
                    crate::records::feature::scope::DesignScopePayload::Revolve(Some(
                        crate::records::feature::path_features::DesignRevolveConstruction {
                            reversed: true,
                            ..
                        }
                    ))
                )
        });
        feature.evaluation.edit(|definition, _| {
            let FeatureDefinition::Operation(FeatureOperation::Revolve { construction, .. }) =
                definition
            else {
                return;
            };
            let Some(axis) = construction.axis().cloned() else {
                return;
            };
            if matches!(
                construction.extent(),
                Some(RevolveExtent::OneSided {
                    termination: AngularTermination::Angle { angle },
                }) if *angle == cadmpeg_ir::scalar::PositiveAngle::FULL_TURN
            ) {
                return;
            }
            let oriented = match construction.profile() {
                Some(PlanarProfileRef::Sketch(sketch)) => sketches
                    .iter()
                    .find(|candidate| &candidate.id == sketch)
                    .and_then(|sketch| {
                        oriented_revolve_axis(
                            &axis,
                            sketch,
                            sketch_entities,
                            reversed,
                            (linear_tolerance, angular_tolerance),
                        )
                    }),
                _ => None,
            };
            construction.set_axis(oriented);
        });
    }
}

/// The axis directed so that a right-handed turn carries the profile of
/// `sketch` toward the sketch normal, or toward its reverse when `reversed`, or
/// `None` when the axis is not parallel to the sketch plane or the profile does
/// not lie on one side of the axis.
fn oriented_revolve_axis(
    axis: &cadmpeg_ir::features::RevolutionAxis,
    sketch: &cadmpeg_ir::sketches::Sketch,
    sketch_entities: &[cadmpeg_ir::sketches::SketchEntity],
    reversed: bool,
    (linear_tolerance, angular_tolerance): (f64, f64),
) -> Option<cadmpeg_ir::features::RevolutionAxis> {
    let (origin, normal, u_axis) = sketch.resolved_placement()?;
    let normal = normal.get().unit()?;
    let u_axis = u_axis.get().unit()?;
    let v_axis = normal.cross(u_axis);
    let direction = axis.direction.get().unit()?;
    if direction.dot(normal).abs() > angular_tolerance {
        return None;
    }
    // In-plane and perpendicular to the axis: its sign names the profile side.
    let across = normal.cross(direction);
    let mut positive_side = None;
    for entity_use in sketch.profiles.iter().flatten() {
        let entity = sketch_entities
            .iter()
            .find(|entity| entity.id() == &entity_use.entity)?;
        for point in revolve_profile_sample_points(&entity.geometry) {
            let offset = origin
                .get()
                .translated(u_axis, point.u)
                .translated(v_axis, point.v)
                .vector_from(axis.origin.get())
                .dot(across);
            // A profile can touch its axis; only points off the axis name a side.
            if offset.abs() <= linear_tolerance {
                continue;
            }
            if *positive_side.get_or_insert(offset > 0.0) != (offset > 0.0) {
                return None;
            }
        }
    }
    Some(cadmpeg_ir::features::RevolutionAxis {
        direction: if positive_side? == reversed {
            // Reversing a zero component gives -0.0; keep it a plain zero.
            cadmpeg_ir::features::FeatureDirection3::from_unit_without_small_components(
                cadmpeg_ir::units::UnitVector3::normalized(axis.direction.reversed().get())?,
            )
        } else {
            axis.direction
        },
        ..axis.clone()
    })
}

/// Sketch points that lie on a profile curve or, for a closed conic, inside
/// it. A revolve profile does not cross its axis, so each of them names the
/// profile side. Other curve kinds contribute no point.
fn revolve_profile_sample_points(
    geometry: &cadmpeg_ir::sketches::SketchGeometry,
) -> Vec<cadmpeg_ir::math::Point2> {
    use cadmpeg_ir::math::Point2;
    use cadmpeg_ir::sketches::SketchGeometryDefinition as Definition;

    match geometry.definition().to_raw() {
        Definition::Line { start, end } => vec![start, end],
        Definition::Arc {
            center,
            radius,
            start_angle,
            end_angle,
        } => [start_angle, end_angle]
            .into_iter()
            .map(|angle| {
                Point2::new(
                    center.u + radius.get() * angle.get().cos(),
                    center.v + radius.get() * angle.get().sin(),
                )
            })
            .collect(),
        Definition::Circle { center, .. } => vec![center],
        Definition::Ellipse { center, bounds, .. } if bounds.is_none() => vec![center],
        _ => Vec::new(),
    }
}

/// Resolve Revolve axes selected through history-qualified analytic faces.
pub(crate) fn bind_revolve_face_axes(
    features: &mut [cadmpeg_ir::features::Feature],
    scopes: &[DesignParameterScope],
    construction_groups: &[DesignConstructionOperandGroup],
    entity_selection_operands: &[crate::records::topology::entity_selection::DesignEntitySelectionOperand],
    face_operands: &[crate::records::topology::face::DesignFaceOperand],
    faces: &[cadmpeg_ir::topology::Face],
    surfaces: &[cadmpeg_ir::geometry::Surface],
) {
    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};

    for feature in features {
        let native_ref = feature.native_ref.as_deref();
        feature.evaluation.edit(|definition, _| 'feature_edit: {
            let FeatureDefinition::Operation(FeatureOperation::Revolve { construction, .. }) =
                definition
            else {
                break 'feature_edit;
            };
            if construction.axis().is_some() {
                break 'feature_edit;
            }
            let Some(scope) = native_ref
                .and_then(|native_ref| scopes.iter().find(|scope| scope.id == native_ref))
            else {
                break 'feature_edit;
            };
            let Some(stream) = native_stream(&scope.id) else {
                break 'feature_edit;
            };
            let Some(group) = unique_feature_match(construction_groups.iter().filter(|group| {
                native_stream(&group.id) == Some(stream)
                    && group.scope_record_index == scope.record_index
                    && group.role() == DesignOperandRole::ROLE_0X21
            })) else {
                break 'feature_edit;
            };
            let [crate::records::identity::Located { value: member, .. }] = group.members() else {
                break 'feature_edit;
            };
            let selection =
                unique_feature_match(entity_selection_operands.iter().filter(|operand| {
                    native_stream(&operand.id) == Some(stream)
                        && operand.scope_record_index == scope.record_index
                        && operand.group_record_index == group.record_index
                        && operand.group_member_ordinal == 0
                        && operand.record_index() == *member
                }));
            let entity_face_slot = match selection {
                Some(selection) => selection
                    .historical_face_candidates
                    .first()
                    .map(|candidate| candidate.face_slot)
                    .filter(|slot| {
                        selection
                            .historical_face_candidates
                            .iter()
                            .all(|candidate| candidate.face_slot == *slot)
                    }),
                _ => None,
            };
            let entity_axis = entity_face_slot.and_then(|face_slot| {
                analytic_axis_for_face(&ids::brep_face_id(face_slot), faces, surfaces)
            });
            let recipe_axis = revolve_face_axis_operand(scope, group, *member, face_operands)
                .and_then(|operand| {
                    let first_face =
                        crate::design::face_resolve::historical_face_operand_candidate_iter(
                            operand,
                        )
                        .min_by(|left, right| left.as_str().cmp(right.as_str()))?;
                    let first = analytic_axis_for_face(first_face, faces, surfaces)?;
                    crate::design::face_resolve::historical_face_operand_candidate_iter(operand)
                        .map(|face_id| analytic_axis_for_face(face_id, faces, surfaces))
                        .all(|axis| {
                            axis.is_some_and(|axis| {
                                crate::history::same_axis_line(
                                    (first.origin.get(), first.direction.get()),
                                    (axis.origin.get(), axis.direction.get()),
                                )
                            })
                        })
                        .then_some(first)
                });
            construction.set_axis(match (entity_axis, recipe_axis) {
                (Some(entity), Some(recipe))
                    if crate::history::same_axis_line(
                        (entity.origin.get(), entity.direction.get()),
                        (recipe.origin.get(), recipe.direction.get()),
                    ) =>
                {
                    Some(entity)
                }
                (Some(axis), None) | (None, Some(axis)) => Some(axis),
                _ => None,
            });
        });
    }
}

fn analytic_axis_for_face(
    face_id: &cadmpeg_ir::ids::FaceId,
    faces: &[cadmpeg_ir::topology::Face],
    surfaces: &[cadmpeg_ir::geometry::Surface],
) -> Option<cadmpeg_ir::features::RevolutionAxis> {
    let face = unique_feature_match(faces.iter().filter(|face| &face.id == face_id))?;
    let surface =
        unique_feature_match(surfaces.iter().filter(|surface| surface.id == face.surface))?;
    analytic_surface_axis(&surface.geometry)
}

fn analytic_surface_axis(
    geometry: &cadmpeg_ir::geometry::SurfaceGeometry,
) -> Option<cadmpeg_ir::features::RevolutionAxis> {
    use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};

    let (origin, direction) = match geometry {
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(plane_surface)) => {
            (plane_surface.origin(), *plane_surface.frame().axis())
        }
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(cylinder_surface)) => {
            (cylinder_surface.origin(), *cylinder_surface.frame().axis())
        }
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(cone_surface)) => {
            (cone_surface.origin(), *cone_surface.frame().axis())
        }
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Torus(torus_surface)) => {
            (torus_surface.center(), *torus_surface.frame().axis())
        }
        _ => return None,
    };
    Some(cadmpeg_ir::features::RevolutionAxis {
        origin,
        direction: cadmpeg_ir::features::FeatureDirection3::from(direction),
        reference: None,
    })
}

/// Resolve a Revolve axis selection that names a Design Geometry origin axis.
///
/// The identity is admitted when no Sketch placement in the stream owns it.
/// A body edge or face axis is a recipe operand, so an ASM topology identity
/// with the same value is a different entity.
fn design_geometry_axis_selection(
    scope: &DesignParameterScope,
    axis_group: &DesignConstructionOperandGroup,
    axis_member: u32,
    entity_selection_operands: &[crate::records::topology::entity_selection::DesignEntitySelectionOperand],
    placements: &[DesignSketchPlacement],
) -> Option<cadmpeg_ir::features::RevolutionAxis> {
    let stream = native_stream(&scope.id)?;
    let selection = unique_feature_match(entity_selection_operands.iter().filter(|operand| {
        native_stream(&operand.id) == Some(stream)
            && operand.scope_record_index == scope.record_index
            && operand.group_record_index == axis_group.record_index
            && operand.group_member_ordinal == 0
            && operand.record_index() == axis_member
    }))?;
    if placements.iter().any(|placement| {
        native_stream(&placement.id) == Some(stream)
            && placement.entity_id.suffix() == selection.primary_identity
    }) {
        return None;
    }
    let direction = match selection.primary_identity {
        39 => cadmpeg_ir::units::UnitVector3::X_AXIS,
        40 => cadmpeg_ir::units::UnitVector3::Y_AXIS,
        41 => cadmpeg_ir::units::UnitVector3::Z_AXIS,
        _ => return None,
    };
    Some(cadmpeg_ir::features::RevolutionAxis {
        origin: cadmpeg_ir::features::FinitePoint3::ZERO,
        direction: cadmpeg_ir::features::FeatureDirection3::from(direction),
        reference: None,
    })
}

fn resolve_sketch_axis_selection(
    scope: &DesignParameterScope,
    axis_group: &DesignConstructionOperandGroup,
    axis_member: u32,
    entity_selection_operands: &[crate::records::topology::entity_selection::DesignEntitySelectionOperand],
    placements: &[DesignSketchPlacement],
    curve_identities: &[SketchCurveIdentity],
) -> Option<cadmpeg_ir::features::RevolutionAxis> {
    let stream = native_stream(&scope.id)?;
    let selection = unique_feature_match(entity_selection_operands.iter().filter(|operand| {
        native_stream(&operand.id) == Some(stream)
            && operand.scope_record_index == scope.record_index
            && operand.group_record_index == axis_group.record_index
            && operand.group_member_ordinal == 0
            && operand.record_index() == axis_member
    }))?;
    let owner_reference = u32::try_from(selection.primary_identity).ok()?;
    let placement = unique_feature_match(placements.iter().filter(|placement| {
        native_stream(&placement.id) == Some(stream)
            && placement.entity_id.suffix() == selection.primary_identity
    }))?;
    let curve = unique_feature_match(curve_identities.iter().filter(|curve| {
        native_stream(&curve.id) == Some(stream)
            && curve.owner_reference == Some(owner_reference)
            && entity_selection_matches_curve(selection, curve)
    }))?;
    let SketchCurveGeometry::Line {
        start, direction, ..
    } = curve.geometry.as_ref()?
    else {
        return None;
    };
    let start = start.as_raw();
    let direction = direction.as_raw();
    let origin_scale = crate::design::face_resolve::placement_origin_scale(placement);
    let origin = Point3::new(
        placement.transform()[0][0] * start.x
            + placement.transform()[0][1] * start.y
            + placement.transform()[0][2] * start.z
            + placement.transform()[0][3] * origin_scale,
        placement.transform()[1][0] * start.x
            + placement.transform()[1][1] * start.y
            + placement.transform()[1][2] * start.z
            + placement.transform()[1][3] * origin_scale,
        placement.transform()[2][0] * start.x
            + placement.transform()[2][1] * start.y
            + placement.transform()[2][2] * start.z
            + placement.transform()[2][3] * origin_scale,
    );
    let direction = Vector3::new(
        placement.transform()[0][0] * direction.x
            + placement.transform()[0][1] * direction.y
            + placement.transform()[0][2] * direction.z,
        placement.transform()[1][0] * direction.x
            + placement.transform()[1][1] * direction.y
            + placement.transform()[1][2] * direction.z,
        placement.transform()[2][0] * direction.x
            + placement.transform()[2][1] * direction.y
            + placement.transform()[2][2] * direction.z,
    );
    Some(cadmpeg_ir::features::RevolutionAxis {
        origin: cadmpeg_ir::features::FinitePoint3::new(origin)?,
        direction: cadmpeg_ir::features::FeatureDirection3::from(
            cadmpeg_ir::units::UnitVector3::normalized_by_reciprocal(direction)?,
        ),
        reference: None,
    })
}

pub(super) fn project_fixed_loft(
    scope: &DesignParameterScope,
    inputs: &ProjectInputs<'_>,
    ctx: &DecodeContext<'_>,
) -> Result<Option<cadmpeg_ir::features::FeatureDefinition>, CodecError> {
    use cadmpeg_ir::features::{
        FeatureDefinition, FeatureOperation, LoftPointSection, LoftSection, PlanarProfileRef,
        ProfileRef,
    };
    let &ProjectInputs {
        construction_groups,
        legacy_loft_body_carriers: legacy_body_carriers,
        edge_operands,
        edge_identity_operands,
        face_operands,
        face_boundaries,
        ..
    } = inputs;

    let (crate::records::feature::scope::DesignScopePayload::Loft(loft_payload)
    | crate::records::feature::scope::DesignScopePayload::Solevacion(loft_payload)) =
        &scope.payload()
    else {
        return Ok(None);
    };
    let Some(crate::records::feature::path_features::DesignLoftConstruction { operation, .. }) =
        loft_payload
    else {
        return Ok(None);
    };
    let stream = or_none!(native_stream(&scope.id));
    let mut groups = Vec::new();
    for group in construction_groups.iter().filter(|group| {
        native_stream(&group.id) == Some(stream) && group.scope_record_index == scope.record_index
    }) {
        groups.push(group);
    }
    groups.sort_by(|left, right| {
        left.scope_reference_ordinal
            .cmp(&right.scope_reference_ordinal)
    });
    let mut matching_legacy_carriers = legacy_body_carriers.iter().filter(|carrier| {
        native_stream(&carrier.id) == Some(stream)
            && carrier.scope_record_index == scope.record_index
    });
    let legacy_body_group_identity = match (
        matching_legacy_carriers.next(),
        matching_legacy_carriers.next(),
    ) {
        (Some(_), None) => {
            if groups
                .iter()
                .any(|group| group.role() == DesignOperandRole::BODIES_A)
            {
                return Ok(None);
            }
            let mut body_groups = groups.iter().filter(|group| {
                group.role() == DesignOperandRole::BODIES_B && group.scope_reference_ordinal == 1
            });
            let body_group = or_none!(body_groups.next());
            if body_groups.next().is_some() {
                return Ok(None);
            }
            Some((body_group.record_index, body_group.scope_reference_ordinal))
        }
        (None, _) => {
            // A carrier-free Boolean Loft scope with exactly six ordered reference
            // members, at least two sections, and no BODIES_A group admits a single
            // BODIES_B target body selection at scope-reference ordinal one.
            if scope.reference_members().len() == 6
                && !groups
                    .iter()
                    .any(|group| group.role() == DesignOperandRole::BODIES_A)
                && groups
                    .iter()
                    .filter(|group| {
                        matches!(
                            group.role(),
                            DesignOperandRole::PROFILE | DesignOperandRole::ROLE_0X43
                        )
                    })
                    .count()
                    >= 2
            {
                let mut body_groups = groups.iter().filter(|group| {
                    group.role() == DesignOperandRole::BODIES_B
                        && group.scope_reference_ordinal == 1
                });
                match (body_groups.next(), body_groups.next()) {
                    (Some(body_group), None) => {
                        Some((body_group.record_index, body_group.scope_reference_ordinal))
                    }
                    _ => None,
                }
            } else {
                None
            }
        }
        _ => return Ok(None),
    };
    let is_body_group = |group: &DesignConstructionOperandGroup| {
        group.role() == DesignOperandRole::BODIES_A
            || legacy_body_group_identity
                == Some((group.record_index, group.scope_reference_ordinal))
    };
    let body_count = groups.iter().filter(|group| is_body_group(group)).count();
    let expected_body_count = usize::from(*operation != DesignExtrudeOperation::NewBody);
    if body_count != expected_body_count {
        return Ok(None);
    }
    let mut operands = Vec::new();
    for group in groups.iter().filter(|group| !is_body_group(group)) {
        operands.push(*group);
    }
    let mut profile_groups = Vec::new();
    for group in operands.iter().filter(|group| {
        matches!(
            group.role(),
            DesignOperandRole::PROFILE | DesignOperandRole::ROLE_0X43
        )
    }) {
        profile_groups.push(*group);
    }
    let (sections, guides, centerline) = if profile_groups.len() >= 2 {
        if operands.iter().any(|group| {
            !matches!(
                group.role(),
                DesignOperandRole::PROFILE
                    | DesignOperandRole::ROLE_0X43
                    | DesignOperandRole::ROLE_0X5
                    | DesignOperandRole::ROLE_0X7
            )
        }) {
            return Ok(None);
        }
        let mut sections = Vec::new();
        for group in &profile_groups {
            let edge_profile =
                resolved_loft_edge_profile_group(scope, group, edge_operands, face_boundaries)?;
            let face_profile = if edge_profile.is_none() {
                resolved_profile_face_group(scope, group, face_operands)?
            } else {
                None
            };
            let profile = match edge_profile.or(face_profile) {
                Some(profile) => profile,
                None => ProfileRef::Planar(PlanarProfileRef::Native(group.id.clone())),
            };
            sections.push(LoftSection::Profile(profile));
        }
        let mut guides = Vec::new();
        for group in operands
            .iter()
            .filter(|group| group.role() == DesignOperandRole::ROLE_0X5)
        {
            let path = resolved_loft_path(
                group,
                construction_groups,
                edge_operands,
                edge_identity_operands,
                scope,
                ctx,
            )?;
            guides.push(path);
        }
        let mut centerlines = Vec::new();
        for group in operands
            .iter()
            .filter(|group| group.role() == DesignOperandRole::ROLE_0X7)
        {
            let path = resolved_loft_path(
                group,
                construction_groups,
                edge_operands,
                edge_identity_operands,
                scope,
                ctx,
            )?;
            centerlines.push(path);
        }
        let centerline = match centerlines.len() {
            0 => None,
            1 if guides.is_empty() => centerlines.pop(),
            _ => return Ok(None),
        };
        (sections, guides, centerline)
    } else if *operation == DesignExtrudeOperation::NewBody {
        if profile_groups.len() == 1
            && operands.iter().all(|group| {
                matches!(
                    group.role(),
                    DesignOperandRole::ROLE_0X43 | DesignOperandRole::ROLE_0X5
                )
            })
        {
            let point_ordinal = operands.iter().position(|group| {
                group.role() == DesignOperandRole::ROLE_0X5 && group.members().len() == 1
            });
            let point_ordinal = or_none!(point_ordinal);
            if !matches!(point_ordinal, 0) && point_ordinal + 1 != operands.len() {
                return Ok(None);
            }
            if operands.iter().enumerate().any(|(ordinal, group)| {
                ordinal != point_ordinal
                    && group.role() == DesignOperandRole::ROLE_0X5
                    && group.members().len() == 1
            }) {
                return Ok(None);
            }
            let mut sections = Vec::new();
            for (ordinal, group) in operands.iter().enumerate() {
                let id = group.id.clone();
                let section = if ordinal == point_ordinal {
                    LoftSection::Point(LoftPointSection::Native(or_none!(
                        cadmpeg_core::text::NonBlankString::new(id)
                    )))
                } else {
                    LoftSection::Profile(ProfileRef::Planar(PlanarProfileRef::Native(id)))
                };
                sections.push(section);
            }
            (sections, Vec::new(), None)
        } else if profile_groups.is_empty() {
            let role = if operands
                .iter()
                .all(|group| group.role() == DesignOperandRole::PROFILE)
            {
                DesignOperandRole::PROFILE
            } else if operands
                .iter()
                .all(|group| group.role() == DesignOperandRole::ROLE_0X5)
            {
                DesignOperandRole::ROLE_0X5
            } else {
                return Ok(None);
            };
            let mut sections = Vec::new();
            for group in operands.iter().filter(|group| group.role() == role) {
                let id = group.id.clone();
                sections.push(LoftSection::Profile(ProfileRef::Planar(
                    PlanarProfileRef::Native(id),
                )));
            }
            (sections, Vec::new(), None)
        } else {
            return Ok(None);
        }
    } else {
        return Ok(None);
    };
    if sections.len() < 2
        || sections.len() + guides.len() + usize::from(centerline.is_some()) + body_count
            != groups.len()
    {
        return Ok(None);
    }
    Ok(Some(FeatureDefinition::Operation(FeatureOperation::Loft {
        sections,
        guidance: if let Some(centerline) = centerline {
            cadmpeg_ir::features::LoftGuidance::Centerline(centerline)
        } else {
            cadmpeg_ir::features::LoftGuidance::Guides(guides)
        },
        op: fixed_boolean_operation(*operation),
        closed: false,
        solid: true,
        ruled: false,
        linearize: false,
        max_degree: None,
        allow_multi_profile_faces: None,
    })))
}

fn resolved_loft_path(
    group: &DesignConstructionOperandGroup,
    groups: &[DesignConstructionOperandGroup],
    operands: &[DesignEdgeOperand],
    identity_operands: &[DesignEdgeIdentityOperand],
    scope: &DesignParameterScope,
    ctx: &DecodeContext<'_>,
) -> Result<cadmpeg_ir::features::PathRef, CodecError> {
    let selection = resolved_edge_group(
        group,
        groups,
        operands,
        identity_operands,
        scope.previous_history_state_id(),
        &crate::ids::neutral_feature_id(scope),
        ctx,
    )?;
    Ok(loft_path_from_edge_selection(&group.id, selection))
}

#[derive(Clone, Copy)]
enum SurfacePatchRecipe<'a> {
    Grouped(&'a [crate::records::topology::historical_context::AsmFaceBoundary]),
    Direct,
}

fn resolved_surface_patch_path(
    groups: &[&DesignConstructionOperandGroup],
    all_groups: &[DesignConstructionOperandGroup],
    operands: &[DesignEdgeOperand],
    identity_operands: &[DesignEdgeIdentityOperand],
    scope: &DesignParameterScope,
    recipe: SurfacePatchRecipe<'_>,
    ctx: &DecodeContext<'_>,
) -> Result<cadmpeg_ir::features::PathRef, CodecError> {
    use cadmpeg_ir::features::PathRef;

    let mut paths = Vec::new();
    for group in groups {
        let selection = if let SurfacePatchRecipe::Grouped(face_boundaries) = recipe {
            resolved_surface_patch_edge_group(
                group,
                all_groups,
                operands,
                identity_operands,
                face_boundaries,
                EdgeGroupTransition {
                    previous_state_id: scope.previous_history_state_id(),
                    feature_id: &crate::ids::neutral_feature_id(scope),
                },
                ctx,
            )
        } else {
            resolved_edge_group(
                group,
                all_groups,
                operands,
                identity_operands,
                scope.previous_history_state_id(),
                &crate::ids::neutral_feature_id(scope),
                ctx,
            )
        }?;
        let path = loft_path_from_edge_selection(&group.id, selection);
        paths.push(path);
    }
    if matches!(recipe, SurfacePatchRecipe::Grouped(_)) && paths.len() == 1 {
        if let Some(path) = paths.pop() {
            return Ok(path);
        }
    }
    if paths.is_empty() {
        return Ok(PathRef::Native(scope.id.clone()));
    }
    if let Some(state) = paths.iter().find_map(|path| {
        let PathRef::HistoricalEdges { state, .. } = path else {
            return None;
        };
        Some(state)
    }) {
        if paths.iter().all(|path| {
            matches!(path,
            PathRef::HistoricalEdges { state: candidate, .. } if candidate == state)
        }) {
            let state = state.clone();
            let mut edges = Vec::new();
            for path in &paths {
                if let PathRef::HistoricalEdges {
                    edges: group_edges, ..
                } = path
                {
                    edges.extend_from_slice(group_edges);
                }
            }
            let members = cadmpeg_ir::features::SelectionMembers::try_from(edges);
            let native = scope.id.clone();
            return Ok(match members {
                Ok(edges) => PathRef::HistoricalEdges {
                    state,
                    edges,
                    native: cadmpeg_core::text::NonBlankString::new(native)
                        .ok_or_else(|| CodecError::malformed("surface patch path is blank"))?,
                },
                Err(_) => PathRef::Native(native),
            });
        }
    }
    if paths.iter().all(|path| matches!(path, PathRef::Edges(_))) {
        let mut edges = Vec::new();
        for path in &paths {
            if let PathRef::Edges(group_edges) = path {
                edges.extend_from_slice(group_edges);
            }
        }
        return Ok(PathRef::Edges(edges));
    }
    Ok(PathRef::Native(scope.id.clone()))
}

fn loft_path_from_edge_selection(
    native: &str,
    selection: cadmpeg_ir::features::EdgeSelection,
) -> cadmpeg_ir::features::PathRef {
    use cadmpeg_ir::features::{EdgeSelection, PathRef};

    match selection {
        EdgeSelection::Edges(edges) | EdgeSelection::Resolved { edges, .. } => {
            PathRef::Edges(edges)
        }
        EdgeSelection::Historical {
            state,
            edges,
            native,
        } => PathRef::HistoricalEdges {
            state,
            edges,
            native,
        },
        EdgeSelection::All
        | EdgeSelection::Unresolved
        | EdgeSelection::Native(_)
        | EdgeSelection::Generated { .. }
        | EdgeSelection::HistoricalPartial { .. } => PathRef::Native(native.to_owned()),
    }
}

fn project_circular_pattern(
    scope: &DesignParameterScope,
    groups: &[DesignConstructionOperandGroup],
    face_operands: &[DesignFaceOperand],
    entity_selection_operands: &[crate::records::topology::entity_selection::DesignEntitySelectionOperand],
    scopes: &[DesignParameterScope],
) -> Option<cadmpeg_ir::features::FeatureDefinition> {
    use cadmpeg_ir::features::{
        patterns::{PatternKind, PatternTransform},
        FeatureDefinition, FeatureOperation,
    };
    let construction = scope.circular_pattern_construction()?;
    let (axis_origin, axis_dir) = circular_pattern_axis(&construction.axis)?;
    let stream = native_stream(&scope.id)?;
    let group = unique_feature_match(groups.iter().filter(|group| {
        native_stream(&group.id) == Some(stream)
            && group.scope_record_index == scope.record_index
            && matches!(
                group.role(),
                DesignOperandRole::BODIES_A | DesignOperandRole::BODIES_B
            )
            && !group.members().is_empty()
    }))?;
    let seeds = pattern_group_seeds(
        scope,
        group,
        face_operands,
        entity_selection_operands,
        scopes,
    );
    let axis_origin = cadmpeg_ir::features::FinitePoint3::new(axis_origin)?;
    let axis_dir = cadmpeg_ir::features::FeatureDirection3::new(axis_dir)?;
    let pattern = PatternKind::new(PatternTransform::Circular {
        axis_origin,
        axis_dir,
        angle: construction.angle,
        count: construction.count.get(),
    })
    .ok()?;
    Some(FeatureDefinition::Operation(FeatureOperation::Pattern {
        seeds,
        pattern,
    }))
}

/// Seeds of a pattern's seed group. A body-role group selects its bodies. A
/// face-role group selects its historical faces in the scope's preceding
/// state; otherwise, when every member is a compact entity selection that
/// names a feature scope, it selects those features in member order.
fn pattern_group_seeds(
    scope: &DesignParameterScope,
    group: &DesignConstructionOperandGroup,
    face_operands: &[DesignFaceOperand],
    entity_selection_operands: &[crate::records::topology::entity_selection::DesignEntitySelectionOperand],
    scopes: &[DesignParameterScope],
) -> Vec<cadmpeg_ir::features::patterns::PatternSeed> {
    use cadmpeg_ir::features::patterns::PatternSeed;

    if group.role() != DesignOperandRole::BODIES_A {
        return vec![PatternSeed::Bodies(
            cadmpeg_ir::features::BodySelection::Native(group.id.clone()),
        )];
    }
    if let Some(selection) = resolved_historical_face_group(
        scope,
        scope.previous_history_state_id(),
        group,
        face_operands,
    ) {
        return vec![PatternSeed::Faces(selection)];
    }
    entity_selection_feature_seeds(scope, group, entity_selection_operands, scopes).unwrap_or_else(
        || {
            vec![PatternSeed::Faces(
                cadmpeg_ir::features::FaceSelection::Native(group.id.clone()),
            )]
        },
    )
}

/// Feature seeds named by a face-role group whose members are all compact
/// entity selections. Each compact identity is the record index of a
/// modelling feature scope in the same stream other than `scope`.
fn entity_selection_feature_seeds(
    scope: &DesignParameterScope,
    group: &DesignConstructionOperandGroup,
    entity_selection_operands: &[crate::records::topology::entity_selection::DesignEntitySelectionOperand],
    scopes: &[DesignParameterScope],
) -> Option<Vec<cadmpeg_ir::features::patterns::PatternSeed>> {
    let stream = native_stream(&scope.id)?;
    if group.members().is_empty() {
        return None;
    }
    group
        .members()
        .iter()
        .enumerate()
        .map(|(ordinal, member)| {
            let ordinal = u32::try_from(ordinal).ok()?;
            let selection =
                unique_feature_match(entity_selection_operands.iter().filter(|operand| {
                    native_stream(&operand.id) == Some(stream)
                        && operand.scope_record_index == scope.record_index
                        && operand.group_record_index == group.record_index
                        && operand.group_member_ordinal == ordinal
                        && operand.record_index() == member.value
                }))?;
            let record_index = u32::try_from(selection.compact_identity()?).ok()?;
            let seed_scope = unique_feature_match(scopes.iter().filter(|candidate| {
                native_stream(&candidate.id) == Some(stream)
                    && candidate.record_index == record_index
                    && candidate.record_index != scope.record_index
                    && candidate.history_state_id().is_some()
            }))?;
            Some(cadmpeg_ir::features::patterns::PatternSeed::Feature(
                crate::ids::neutral_feature_id(seed_scope),
            ))
        })
        .collect()
}

fn circular_pattern_axis(
    axis: &crate::records::feature::patterns::DesignCircularPatternAxis,
) -> Option<(Point3, Vector3)> {
    use crate::records::feature::patterns::DesignCircularPatternAxis;

    match axis {
        DesignCircularPatternAxis::Inline {
            origin, direction, ..
        } => Some((
            Point3::new(
                origin[0].get() * 10.0,
                origin[1].get() * 10.0,
                origin[2].get() * 10.0,
            ),
            *direction.as_raw(),
        )),
        DesignCircularPatternAxis::HistoricalEdge {
            resolved: Some(axis),
            ..
        } => Some((axis.origin.get(), *axis.direction.as_raw())),
        DesignCircularPatternAxis::HistoricalEdge { .. } => None,
    }
}

/// The gap count of an active rectangular pattern axis: one less than the
/// instance count. An axis is active only when it states two or more
/// instances, so the gap count is never zero and always divides.
#[derive(Clone, Copy)]
struct PatternIntervals(std::num::NonZeroU32);

impl PatternIntervals {
    /// Admit an active axis. An axis with fewer than two instances states no
    /// gap and is refused here.
    fn new(count: u32) -> Option<Self> {
        std::num::NonZeroU32::new(count.checked_sub(1)?).map(Self)
    }

    /// The gap count.
    const fn get(self) -> u32 {
        self.0.get()
    }
}

fn project_rectangular_pattern_scalars(
    scope: &DesignParameterScope,
    groups: &[DesignConstructionOperandGroup],
    face_operands: &[DesignFaceOperand],
    entity_selection_operands: &[crate::records::topology::entity_selection::DesignEntitySelectionOperand],
    scopes: &[DesignParameterScope],
) -> Option<cadmpeg_ir::features::FeatureDefinition> {
    use cadmpeg_ir::features::{
        patterns::{PatternKind, PatternSeed, PatternTransform},
        FeatureDefinition, FeatureOperation,
    };
    use cadmpeg_ir::scalar::PositiveLength;

    let construction = scope.rectangular_pattern_construction()?;
    let mut direction_records = construction.directions().iter();
    let mut active = Vec::new();
    let adjacent = construction
        .directions()
        .first()
        .is_some_and(|record| record.distance_type == 7);
    for (axis, (count, extent)) in [
        (construction.u_count(), construction.u_extent()),
        (construction.v_count(), construction.v_extent()),
    ]
    .into_iter()
    .enumerate()
    {
        let Some(intervals) = PatternIntervals::new(count) else {
            continue;
        };
        let stored_direction = if axis == 1 && construction.secondary_direction().is_some() {
            construction.secondary_direction().and_then(|record| {
                cadmpeg_ir::units::UnitVector3::normalized(Vector3::from(
                    record.value.map(cadmpeg_ir::scalar::FiniteReal::get),
                ))
            })
        } else {
            direction_records.next().and_then(
                crate::records::feature::patterns::DesignRectangularPatternDirection::direction,
            )
        };
        let direction = stored_direction.map(|direction| {
            if extent < 0.0 {
                direction.negated()
            } else {
                direction
            }
        });
        let divisor = if adjacent {
            1.0
        } else {
            f64::from(intervals.get())
        };
        let spacing = PositiveLength::new(extent.abs() * 10.0 / divisor)?;
        active.push((count, spacing, direction));
    }
    let instance_direction = construction.instances().and_then(|instances| {
        let first = &instances.frames().next()?.transform.value;
        let last = &instances.frames().next_back()?.transform.value;
        let delta = Vector3::new(
            last[0][3] - first[0][3],
            last[1][3] - first[1][3],
            last[2][3] - first[2][3],
        );
        cadmpeg_ir::units::UnitVector3::normalized_by_reciprocal(delta)
    });
    let component_seed = construction
        .instances()
        .and_then(|instances| match instances {
            crate::records::feature::patterns::DesignRectangularPatternInstances::Bodies(_) => None,
            crate::records::feature::patterns::DesignRectangularPatternInstances::Components {
                seed,
                ..
            } => Some(PatternSeed::Occurrences(
                (vec![crate::ids::neutral_component_occurrence_id(
                    &seed.occurrence_guid,
                )])
                .try_into()
                .ok()?,
            )),
        });
    let group_seed = if let Some(stream) = native_stream(&scope.id) {
        let mut matching_groups = groups.iter().filter(|group| {
            native_stream(&group.id) == Some(stream)
                && group.scope_record_index == scope.record_index
                && matches!(
                    group.role(),
                    DesignOperandRole::BODIES_A | DesignOperandRole::BODIES_B
                )
                && !group.members().is_empty()
        });
        let first = matching_groups.next();
        let second = matching_groups.next();
        match (first, second) {
            (Some(group), None) => Some(pattern_group_seeds(
                scope,
                group,
                face_operands,
                entity_selection_operands,
                scopes,
            )),
            _ => None,
        }
    } else {
        None
    };
    let seeds = component_seed
        .map(|seed| vec![seed])
        .or(group_seed)
        .unwrap_or_default();
    let (count, spacing, direction, second) = match active.as_slice() {
        // The instance run is the established direction of a single-direction
        // pattern; a direction record only fills it in when the run is absent
        // and must agree with it otherwise.
        [(count, spacing, direction)] => {
            let direction = match (instance_direction, *direction) {
                (Some(run), Some(record))
                    if 1.0 - run.as_raw().dot(*record.as_raw())
                        > EPS_RECTANGULAR_PATTERN_DIRECTION_AGREEMENT =>
                {
                    None
                }
                (run, record) => run.or(record),
            };
            (*count, *spacing, direction, None)
        }
        [(count, spacing, direction), (second_count, second_spacing, Some(second_direction))] => (
            *count,
            *spacing,
            *direction,
            Some(cadmpeg_ir::features::patterns::LinearPatternDirection {
                direction: cadmpeg_ir::features::FeatureDirection3::from(*second_direction),
                spacing: *second_spacing,
                count: *second_count,
            }),
        ),
        _ => return None,
    };
    let pattern = PatternKind::new(PatternTransform::Linear {
        direction: direction.map(cadmpeg_ir::features::FeatureDirection3::from),
        spacing,
        count,
        second,
        omitted_instances: Vec::new(),
    })
    .ok()?;
    Some(FeatureDefinition::Operation(FeatureOperation::Pattern {
        seeds,
        pattern,
    }))
}

fn project_mirror(
    scope: &DesignParameterScope,
    groups: &[DesignConstructionOperandGroup],
    face_operands: &[DesignFaceOperand],
    scopes: &[DesignParameterScope],
) -> Option<cadmpeg_ir::features::FeatureDefinition> {
    use cadmpeg_ir::features::{
        patterns::{PatternKind, PatternSeed, PatternTransform},
        FeatureDefinition, FeatureOperation,
    };

    let construction = scope.mirror_construction()?;
    let stream = native_stream(&scope.id)?;
    let matching_groups = || {
        groups.iter().filter(|group| {
            native_stream(&group.id) == Some(stream)
                && group.scope_record_index == scope.record_index
        })
    };
    let seed_group = unique_feature_match(matching_groups().filter(|group| {
        group.record_index == construction.seed_group_record_index
            && matches!(
                group.role(),
                DesignOperandRole::BODIES_A | DesignOperandRole::BODIES_B
            )
            && !group.members().is_empty()
    }));
    let plane_group = unique_feature_match(matching_groups().filter(|group| {
        group.record_index == construction.plane_group_record_index
            && group.role() == DesignOperandRole::ROLE_0X5
            && group.members().len() == 1
    }));
    let (Some(seed_group), Some(_plane_group)) = (seed_group, plane_group) else {
        return None;
    };
    let seed = if let Some(record_index) = construction
        .seed_feature_scope_record_index
        .map(|reference| reference.value)
    {
        let seed_scope = unique_feature_match(scopes.iter().filter(|candidate| {
            native_stream(&candidate.id) == Some(stream) && candidate.record_index == record_index
        }));
        let seed_scope = seed_scope?;
        PatternSeed::Feature(crate::ids::neutral_feature_id(seed_scope))
    } else if seed_group.role() == DesignOperandRole::BODIES_B {
        PatternSeed::Bodies(cadmpeg_ir::features::BodySelection::Native(
            seed_group.id.clone(),
        ))
    } else {
        PatternSeed::Faces(
            match resolved_historical_face_group(
                scope,
                scope.previous_history_state_id(),
                seed_group,
                face_operands,
            ) {
                Some(selection) => selection,
                None => cadmpeg_ir::features::FaceSelection::Native(seed_group.id.clone()),
            },
        )
    };
    let (plane_origin, plane_normal, scale_origin) = match construction.plane {
        Some(plane) => (plane.origin().get(), *plane.normal().as_raw(), false),
        None => {
            let plane_scope_record_index = construction
                .plane_scope_record_index
                .map(|value| value.value)?;
            let plane = unique_feature_match(scopes.iter().filter(|candidate| {
                native_stream(&candidate.id) == Some(stream)
                    && candidate.record_index == plane_scope_record_index
                    && matches!(
                        candidate.kind(),
                        crate::records::feature::scope::DesignFeatureKind::WorkPlane
                            | crate::records::feature::scope::DesignFeatureKind::PlanoDeObra
                    )
                    && candidate.work_plane_transform().is_some()
            }));
            let plane = plane?;
            let transform = plane.work_plane_transform()?;
            (
                Point3::new(transform[0][3], transform[1][3], transform[2][3]),
                Vector3::new(transform[0][2], transform[1][2], transform[2][2]),
                true,
            )
        }
    };
    let origin_scale = if scale_origin { 10.0 } else { 1.0 };
    let plane_origin = cadmpeg_ir::features::FinitePoint3::new(Point3::new(
        plane_origin.x * origin_scale,
        plane_origin.y * origin_scale,
        plane_origin.z * origin_scale,
    ))?;
    let plane_normal = cadmpeg_ir::features::FeatureDirection3::new(plane_normal)?;
    let pattern = PatternKind::new(PatternTransform::Mirror {
        plane_origin,
        plane_normal,
    })
    .ok()?;
    Some(FeatureDefinition::Operation(FeatureOperation::Pattern {
        seeds: vec![seed],
        pattern,
    }))
}

pub(super) fn project_fixed_sweep(
    scope: &DesignParameterScope,
    construction_groups: &[DesignConstructionOperandGroup],
    edge_operands: &[DesignEdgeOperand],
    edge_identity_operands: &[DesignEdgeIdentityOperand],
    entity_selection_operands: &[crate::records::topology::entity_selection::DesignEntitySelectionOperand],
    face_operands: &[DesignFaceOperand],
    ctx: &DecodeContext<'_>,
) -> Result<Option<cadmpeg_ir::features::FeatureDefinition>, CodecError> {
    use cadmpeg_ir::features::{
        FaceSelection, FeatureDefinition, FeatureOperation, PlanarProfileRef, SweepGuideRail,
        SweepOrientation, SweepPathExtent,
    };
    use cadmpeg_ir::scalar::Angle;

    let Some((operation, values, profile, path, paths, guide_surfaces)) = (|| -> Option<_> {
        let crate::records::feature::scope::DesignScopePayload::Sweep(Some(
            crate::records::feature::scope::DesignSweepScope {
                construction:
                    Some(crate::records::feature::path_features::DesignSweepConstruction {
                        operation,
                        values,
                        ..
                    }),
                ..
            },
        )) = &scope.payload()
        else {
            return None;
        };
        let values: Vec<f64> = values.iter().map(|value| value.get()).collect();
        let stream = native_stream(&scope.id)?;
        let mut groups = Vec::new();
        for group in construction_groups.iter().filter(|group| {
            native_stream(&group.id) == Some(stream)
                && group.scope_record_index == scope.record_index
        }) {
            groups.push(group);
        }
        let mut profiles = Vec::new();
        let mut paths = Vec::new();
        let mut bodies = Vec::new();
        let mut guide_surfaces = Vec::new();
        for group in &groups {
            match group.role() {
                DesignOperandRole::PROFILE => {
                    profiles.push(*group);
                }
                DesignOperandRole::ROLE_0X5 => {
                    paths.push(*group);
                }
                DesignOperandRole::BODIES_A => {
                    bodies.push(*group);
                }
                DesignOperandRole::FACES => {
                    guide_surfaces.push(*group);
                }
                _ => {}
            }
        }
        paths.sort_by(|left, right| {
            left.scope_reference_ordinal
                .cmp(&right.scope_reference_ordinal)
        });
        let guide_surface_form = match guide_surfaces.as_slice() {
            [] => false,
            [_] => true,
            _ => return None,
        };
        let profile = if guide_surface_form {
            let sweep_profile = scope.sweep_profile()?;
            let mut carriers = profiles.iter().filter(|group| {
                group
                    .members()
                    .iter()
                    .map(|member| member.value)
                    .eq([sweep_profile.record_index])
            });
            let mut selections = profiles.iter().filter(|group| {
                !group
                    .members()
                    .iter()
                    .map(|member| member.value)
                    .eq([sweep_profile.record_index])
            });
            let (Some(_carrier), None, Some(selection), None) = (
                carriers.next(),
                carriers.next(),
                selections.next(),
                selections.next(),
            ) else {
                return None;
            };
            if selection.members().is_empty()
                || !selection
                    .members()
                    .iter()
                    .map(|member| &member.value)
                    .all(|member| {
                        entity_selection_operands.iter().any(|operand| {
                            native_stream(&operand.id) == Some(stream)
                                && operand.scope_record_index == scope.record_index
                                && operand.group_record_index == selection.record_index
                                && operand.record_index() == *member
                        })
                    })
            {
                return None;
            }
            *selection
        } else {
            let [profile] = profiles.as_slice() else {
                return None;
            };
            *profile
        };
        let ([path] | [path, _]) = paths.as_slice() else {
            return None;
        };
        let expected_group_count =
            profiles.len() + paths.len() + bodies.len() + guide_surfaces.len();
        if bodies.len() > 1
            || groups.len() != expected_group_count
            || (*operation == DesignExtrudeOperation::NewBody && !bodies.is_empty())
            || (guide_surface_form && paths.len() != 1)
        {
            return None;
        }
        let valid_lanes = match values.as_slice() {
            [along, against] => {
                paths.len() == 1 && (0.0..=1.0).contains(along) && (0.0..=1.0).contains(against)
            }
            [along, against, along_rail, against_rail, _twist, _taper] => {
                [along, against, along_rail, against_rail]
                    .iter()
                    .all(|value| (0.0..=1.0).contains(*value))
                    && (paths.len() != 1 || (*along_rail == 1.0 && *against_rail == 1.0))
            }
            _ => false,
        };
        if !valid_lanes {
            return None;
        }
        Some((operation, values, profile, *path, paths, guide_surfaces))
    })() else {
        return Ok(None);
    };
    let path = resolved_loft_path(
        path,
        construction_groups,
        edge_operands,
        edge_identity_operands,
        scope,
        ctx,
    )?;
    let (along, against, guide_rail, twist, taper) = match values.as_slice() {
        [along, against] => (*along, *against, None, None, None),
        [along, against, along_rail, against_rail, twist, taper] => {
            let guide_extent = SweepPathExtent {
                along_fraction: or_none!(cadmpeg_ir::scalar::Fraction::new(*along_rail)),
                against_fraction: or_none!(cadmpeg_ir::scalar::Fraction::new(*against_rail)),
            };
            let guide_rail = if let Some(rail) = paths.get(1) {
                Some(SweepGuideRail {
                    path: resolved_loft_path(
                        rail,
                        construction_groups,
                        edge_operands,
                        edge_identity_operands,
                        scope,
                        ctx,
                    )?,
                    extent: guide_extent,
                })
            } else {
                None
            };
            let twist_angle = or_none!(Angle::new(*twist));
            let taper_angle = or_none!(Angle::new(*taper));
            (
                *along,
                *against,
                guide_rail,
                (*twist != 0.0).then_some(twist_angle),
                (*taper != 0.0).then_some(taper_angle),
            )
        }
        _ => return Ok(None),
    };
    let orientation = guide_surfaces
        .first()
        .map(|group| SweepOrientation::GuideSurface {
            faces: match resolved_historical_face_group(
                scope,
                scope.previous_history_state_id(),
                group,
                face_operands,
            ) {
                Some(selection) => selection,
                None => FaceSelection::Native(group.id.clone()),
            },
        });
    Ok(Some(FeatureDefinition::Operation(
        FeatureOperation::Sweep {
            shape: cadmpeg_ir::features::SweepShape::Solid {
                op: if *operation == DesignExtrudeOperation::NewBody {
                    cadmpeg_ir::features::SolidSweepOperation::NewBody
                } else {
                    or_none!(fixed_boolean_operation(*operation).try_into().ok())
                },
                section: cadmpeg_ir::features::SweepSection::Profile(PlanarProfileRef::Native(
                    profile.id.clone(),
                )),
                sections: Vec::new(),
            },

            path: Some(path),

            orientation,
            transition: None,
            transformation: None,
            path_tangent: false,
            linearize: false,
            twist,
            path_extent: Some(SweepPathExtent {
                along_fraction: or_none!(cadmpeg_ir::scalar::Fraction::new(along)),
                against_fraction: or_none!(cadmpeg_ir::scalar::Fraction::new(against)),
            }),
            guide_rail,
            taper,
            scale: None,
            allow_multi_profile_faces: None,
        },
    )))
}

/// Project each boundary condition and its resolved continuity weight.
///
/// An unknown continuity or unsupported active weight leaves the complete
/// condition vector unresolved; native settings retain their source values.
fn surface_patch_boundary_conditions(
    scope: &DesignParameterScope,
) -> Vec<cadmpeg_ir::features::FilledSurfaceBoundaryCondition> {
    use crate::records::feature::surface_ops::DesignPatchContinuity;
    use cadmpeg_ir::features::FilledSurfaceBoundaryCondition;

    const DEFAULT_WEIGHT: f64 = 0.5;
    const DEFAULT_WEIGHT_SENTINEL: f64 = -1.0;

    let mut conditions = Vec::new();
    for boundary in scope.surface_patch_boundaries() {
        let condition = match boundary.continuity {
            DesignPatchContinuity::Connected => FilledSurfaceBoundaryCondition::Contact {},
            DesignPatchContinuity::Tangent | DesignPatchContinuity::Curvature => {
                let raw = boundary.scale.get();
                let effective = if raw == DEFAULT_WEIGHT_SENTINEL {
                    DEFAULT_WEIGHT
                } else {
                    raw
                };
                let Some(weight) = cadmpeg_ir::scalar::PositiveReal::new(effective) else {
                    return Vec::new();
                };
                if boundary.continuity == DesignPatchContinuity::Tangent {
                    FilledSurfaceBoundaryCondition::Tangent {
                        weight: Some(weight),
                    }
                } else {
                    FilledSurfaceBoundaryCondition::Curvature {
                        weight: Some(weight),
                    }
                }
            }
            DesignPatchContinuity::Unknown(_) => return Vec::new(),
        };
        conditions.push(condition);
    }
    conditions
}

fn project_surface_patch(
    ctx: &DecodeContext<'_>,
    scope: &DesignParameterScope,
    construction_groups: &[DesignConstructionOperandGroup],
    edge_operands: &[DesignEdgeOperand],
    edge_identity_operands: &[DesignEdgeIdentityOperand],
    face_boundaries: &[crate::records::topology::historical_context::AsmFaceBoundary],
) -> Result<Option<cadmpeg_ir::features::FeatureDefinition>, CodecError> {
    use cadmpeg_ir::features::{
        FaceSelection, FeatureDefinition, FeatureOperation, SurfaceBoundary,
    };

    if scope.kind() != crate::records::feature::scope::DesignFeatureKind::SurfacePatch {
        return Ok(None);
    }
    if scope.surface_patch_normal().is_some_and(|normal| {
        !matches!(
            normal,
            crate::records::feature::surface_ops::DesignSurfacePatchNormalState::Explicit {
                reversed: crate::records::identity::Located { value: false, .. }
            }
        )
    }) {
        return Ok(None);
    }
    let Some(stream) = native_stream(&scope.id) else {
        return Ok(None);
    };
    let mut groups = Vec::new();
    for group in construction_groups.iter().filter(|group| {
        native_stream(&group.id) == Some(stream) && group.scope_record_index == scope.record_index
    }) {
        groups.push(group);
    }
    groups.sort_by(|left, right| {
        left.scope_reference_ordinal
            .cmp(&right.scope_reference_ordinal)
    });

    // The single-group path form stores the group, all of its ordered edge
    // members, and the tool body. It has no per-component settings records.
    let Some(grouped_path_frame_length) = u64::try_from(scope.reference_members().len())
        .ok()
        .and_then(|count| count.checked_mul(11))
        .and_then(|count| count.checked_add(277))
    else {
        return Ok(None);
    };
    if scope.frame_length() == grouped_path_frame_length {
        let [group] = groups.as_slice() else {
            return Ok(None);
        };
        if scope.reference_members().len() < 3 {
            return Ok(None);
        }
        let Some(first_reference) = scope.reference_members().values().next() else {
            return Ok(None);
        };
        let Some(edge_references) = scope
            .reference_members()
            .values_in(1..scope.reference_members().len() - 1)
        else {
            return Ok(None);
        };
        if group.scope_reference_ordinal != 0
            || group.record_index != *first_reference
            || group.role() != DesignOperandRole::BODIES_A
            || group.members().is_empty()
            || !group
                .members()
                .iter()
                .map(|member| member.value)
                .eq(edge_references.copied())
        {
            return Ok(None);
        }
        return Ok(Some(FeatureDefinition::Operation(
            FeatureOperation::FilledSurface {
                boundary: SurfaceBoundary::Path(resolved_surface_patch_path(
                    std::slice::from_ref(group),
                    construction_groups,
                    edge_operands,
                    edge_identity_operands,
                    scope,
                    SurfacePatchRecipe::Grouped(face_boundaries),
                    ctx,
                )?),
                support_faces: FaceSelection::Faces(Vec::new()),
                continuity: cadmpeg_ir::features::FilledSurfaceContinuityState::uniform(
                    cadmpeg_ir::features::SurfaceContinuity::Contact,
                ),
                merge_result: Some(false),
            },
        )));
    }

    // The reference count separates the two settings-bearing forms: the
    // fixed-path form has `3n + 1` references and the sketch-profile form has
    // three. Frame length does not, because the Design scope envelope has two
    // generations and the later one adds fourteen bytes to both forms.
    let (boundary_count, boundary_role) = if scope.reference_members().len() == 3 {
        (1, DesignOperandRole::PROFILE)
    } else {
        let Some(reference_count) = scope.reference_members().len().checked_sub(1) else {
            return Ok(None);
        };
        let boundary_count = reference_count / 3;
        if boundary_count == 0 || scope.reference_members().len() != boundary_count * 3 + 1 {
            return Ok(None);
        }
        (boundary_count, DesignOperandRole::BODIES_A)
    };
    if groups.len() != boundary_count || scope.surface_patch_boundaries().len() != boundary_count {
        return Ok(None);
    }
    let mut occupied = ctx.alloc_filled(
        scope.reference_members().len(),
        false,
        "f3d surface-patch reference occupancy",
    )?;
    for boundary in &groups {
        let Ok(group_ordinal) = usize::try_from(boundary.scope_reference_ordinal) else {
            return Ok(None);
        };
        let Some(member_ordinal) = group_ordinal.checked_add(1) else {
            return Ok(None);
        };
        let Some(settings_ordinal) = group_ordinal.checked_add(2) else {
            return Ok(None);
        };
        let Some(group_reference) = scope.reference_members().values().nth(group_ordinal) else {
            return Ok(None);
        };
        if settings_ordinal >= scope.reference_members().len()
            || boundary.record_index != *group_reference
            || boundary.role() != boundary_role
            || !boundary
                .members()
                .iter()
                .map(|member| member.value)
                .eq(scope
                    .reference_members()
                    .values()
                    .nth(member_ordinal)
                    .into_iter()
                    .copied())
            || occupied[group_ordinal]
            || occupied[member_ordinal]
            || occupied[settings_ordinal]
        {
            return Ok(None);
        }
        let Some(settings) = scope.surface_patch_boundaries().iter().find(|settings| {
            usize::try_from(settings.scope_reference_ordinal).ok() == Some(settings_ordinal)
        }) else {
            return Ok(None);
        };
        let Some(settings_reference) = scope.reference_members().values().nth(settings_ordinal)
        else {
            return Ok(None);
        };
        if settings.record_index != *settings_reference
            || settings.model_reference != boundary.record_index
        {
            return Ok(None);
        }
        occupied[group_ordinal] = true;
        occupied[member_ordinal] = true;
        occupied[settings_ordinal] = true;
    }
    let mut unoccupied = Vec::new();
    for (ordinal, occupied) in occupied.iter().enumerate() {
        if !occupied {
            unoccupied.push(ordinal);
        }
    }
    let endpoint_unoccupied = unoccupied.as_slice() == [0]
        || scope
            .reference_members()
            .len()
            .checked_sub(1)
            .is_some_and(|last| unoccupied.as_slice() == [last]);
    if (scope.reference_members().len() == 3 && !unoccupied.is_empty())
        || (scope.reference_members().len() != 3 && !endpoint_unoccupied)
    {
        return Ok(None);
    }
    let boundary = if let [boundary] = groups.as_slice() {
        resolved_loft_path(
            boundary,
            construction_groups,
            edge_operands,
            edge_identity_operands,
            scope,
            ctx,
        )?
    } else {
        resolved_surface_patch_path(
            &groups,
            construction_groups,
            edge_operands,
            edge_identity_operands,
            scope,
            SurfacePatchRecipe::Direct,
            ctx,
        )?
    };
    Ok(Some(FeatureDefinition::Operation(
        FeatureOperation::FilledSurface {
            boundary: SurfaceBoundary::Path(boundary),
            support_faces: FaceSelection::Faces(Vec::new()),
            continuity: cadmpeg_ir::features::NonEmptyMembers::try_from(
                surface_patch_boundary_conditions(scope),
            )
            .map_or_else(
                |_| cadmpeg_ir::features::FilledSurfaceContinuityState::unresolved(),
                cadmpeg_ir::features::FilledSurfaceContinuityState::per_boundary,
            ),
            merge_result: Some(false),
        },
    )))
}

fn project_boundary_fill(
    scope: &DesignParameterScope,
    construction_groups: &[DesignConstructionOperandGroup],
) -> Option<cadmpeg_ir::features::FeatureDefinition> {
    use cadmpeg_ir::features::{BodySelection, FeatureDefinition, FeatureOperation};

    if scope.kind() != crate::records::feature::scope::DesignFeatureKind::BoundaryFill
        || scope.reference_members().len() < 5
    {
        return None;
    }
    let stream = native_stream(&scope.id)?;
    let mut groups = Vec::new();
    for group in construction_groups.iter().filter(|group| {
        native_stream(&group.id) == Some(stream) && group.scope_record_index == scope.record_index
    }) {
        groups.push(group);
    }
    groups.sort_by(|left, right| {
        left.scope_reference_ordinal
            .cmp(&right.scope_reference_ordinal)
    });
    let (tools, cells) = groups.split_first()?;
    if tools.scope_reference_ordinal != 0
        || tools.record_index != *scope.reference_members().values().next()?
        || tools.role() != DesignOperandRole::BODIES_A
        || cells.is_empty()
    {
        return None;
    }
    for (index, group) in groups.iter().enumerate() {
        let start = usize::try_from(group.scope_reference_ordinal).ok()?;
        let end = groups
            .get(index + 1)
            .and_then(|next| usize::try_from(next.scope_reference_ordinal).ok())
            .unwrap_or(scope.reference_members().len() - 1);
        if start >= end
            || group.record_index != *scope.reference_members().values().nth(start)?
            || !group.members().iter().map(|member| member.value).eq(scope
                .reference_members()
                .values_in(start + 1..end)?
                .copied())
            || (index > 0 && group.role() != DesignOperandRole::ROLE_0X5)
        {
            return None;
        }
    }
    let selected_cells: Vec<_> = cells
        .iter()
        .map(|cell| BodySelection::Native(cell.id.clone()))
        .collect();
    let selected_cells = selected_cells.try_into().ok()?;
    Some(FeatureDefinition::Operation(
        FeatureOperation::BoundaryFill {
            tools: BodySelection::Native(tools.id.clone()),
            cells: selected_cells,
        },
    ))
}

fn hole_extent_loss(scope: &DesignParameterScope) -> cadmpeg_ir::report::loss::LossNote {
    crate::loss::F3dLossCode::HoleExtentUnproven.note(format!(
        "Hole {} retains its native extent because its controls, depth or face operands do not define an admitted extent.", scope.id
    ))
}

/// The face that ends an Extents To Hole: the native identity of its sole
/// role-`0x0000000500000000` group, which has one member.
fn hole_extent_face(
    scope: &DesignParameterScope,
    groups: &[DesignConstructionOperandGroup],
) -> Option<cadmpeg_ir::features::FaceSelection> {
    let group = single_operand_group(groups, scope, DesignOperandRole::ROLE_0X5)?;
    (group.members().len() == 1)
        .then(|| cadmpeg_ir::features::FaceSelection::Native(group.id.clone()))
}

fn hole_termination(
    scope: &DesignParameterScope,
    parameters: &[(u32, &DesignParameter)],
    groups: &[DesignConstructionOperandGroup],
    face_operands: &[DesignFaceOperand],
) -> Option<cadmpeg_ir::features::LinearTermination> {
    use cadmpeg_ir::features::LinearTermination;
    let extent = scope.hole_extent()?;
    let mut depths = parameters
        .iter()
        .filter(|(_, p)| p.source_kind() == "HoleDepth");
    let depth = depths.next();
    if depths.next().is_some() {
        return None;
    }
    match extent.extent_control {
        Some(0) if extent.saved_non_distance_depth.is_some() && depth.is_none() => {
            Some(LinearTermination::ToFace {
                face: hole_extent_face(scope, groups)?,
                offset: None,
            })
        }
        Some(0) | None => Some(LinearTermination::Blind {
            length: design_positive_length(depth?.1)?.into(),
        }),
        Some(1)
            if extent.saved_non_distance_depth.is_some()
                && depth.is_none()
                && hole_start_faces_only(scope, face_operands)? =>
        {
            Some(LinearTermination::ThroughAll {})
        }
        _ => None,
    }
}

/// Whether the face operands of a Hole scope name at most its start face: no
/// operand (a sketch-point Hole), or a direct `face` or `bounded_face` recipe
/// followed by a `bounded_face` recipe, each bound to its scope reference entry.
/// A further face operand, or a direct persistent face selection on a carrier
/// that sketch points do not place, can name an extent face, so it refuses the
/// form.
fn hole_start_faces_only(
    scope: &DesignParameterScope,
    face_operands: &[DesignFaceOperand],
) -> Option<bool> {
    use crate::records::recipes::ConstructionRecipeKind;
    let stream = native_stream(&scope.id)?;
    if scope.hole_construction().is_some_and(|construction| {
        construction.face_selection.is_some() && !construction.placed_by_sketch_points()
    }) {
        return Some(false);
    }
    let mut faces: Vec<_> = face_operands
        .iter()
        .filter(|operand| {
            native_stream(&operand.id) == Some(stream)
                && operand.scope_record_index == scope.record_index
        })
        .collect();
    faces.sort_by_key(|operand| operand.scope_reference_ordinal);
    let [first, second] = faces.as_slice() else {
        return Some(faces.is_empty());
    };
    let bound = |operand: &DesignFaceOperand| {
        operand.group_record_index().is_none()
            && usize::try_from(operand.scope_reference_ordinal)
                .ok()
                .and_then(|ordinal| scope.reference_members().values().nth(ordinal))
                == Some(&operand.record_index())
    };
    Some(
        first.scope_reference_ordinal < second.scope_reference_ordinal
            && matches!(
                first.recipe_kind,
                ConstructionRecipeKind::Face | ConstructionRecipeKind::BoundedFace
            )
            && second.recipe_kind == ConstructionRecipeKind::BoundedFace
            && bound(first)
            && bound(second),
    )
}

/// One directed placement per Hole entry point, in model millimetres.
fn hole_placements(
    scope: &DesignParameterScope,
) -> Option<Vec<cadmpeg_ir::features::holes::HolePlacement>> {
    let construction = scope.hole_construction()?;
    let control = scope.hole_extent()?.direction_control.as_ref()?.value;
    let [x, y, z] = construction.drilling_direction(control)?;
    let direction = cadmpeg_ir::features::FeatureDirection3::new(Vector3::new(x, y, z))?;
    construction
        .entry_points()?
        .into_iter()
        .map(|point| {
            Some(cadmpeg_ir::features::holes::HolePlacement::Directed {
                position: cadmpeg_ir::features::FinitePoint3::new(Point3::new(
                    point[0].get() * 10.0,
                    point[1].get() * 10.0,
                    point[2].get() * 10.0,
                ))?,
                direction,
            })
        })
        .collect()
}

/// The entry treatment that a Hole's parameters state.
enum HoleEntry {
    Counterbore(
        cadmpeg_ir::scalar::PositiveLength,
        cadmpeg_ir::scalar::PositiveLength,
    ),
    Countersink(
        cadmpeg_ir::scalar::PositiveLength,
        cadmpeg_ir::scalar::InteriorAngle,
    ),
}

fn project_hole(
    scope: &DesignParameterScope,
    parameters: &[(u32, &DesignParameter)],
    groups: &[DesignConstructionOperandGroup],
    face_operands: &[DesignFaceOperand],
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
) -> Result<Option<cadmpeg_ir::features::FeatureDefinition>, CodecError> {
    use cadmpeg_ir::features::{
        holes::{HoleBottom, HoleKind},
        FaceSelection, FeatureDefinition, FeatureOperation, LinearTermination,
    };

    if scope.kind() != crate::records::feature::scope::DesignFeatureKind::Hole {
        return Ok(None);
    }
    let Some(extent) = hole_termination(scope, parameters, groups, face_operands) else {
        losses.push(hole_extent_loss(scope));
        return Ok(None);
    };
    if !matches!(parameters.len(), 2..=5) {
        return Ok(None);
    }
    let parameter = |source_kind: &str| {
        let mut matches = parameters
            .iter()
            .filter(|(_, parameter)| parameter.source_kind() == source_kind)
            .map(|(_, parameter)| *parameter);
        let parameter = matches.next()?;
        matches.next().is_none().then_some(parameter)
    };
    let depth = match &extent {
        LinearTermination::Blind { length } => Some(length.get()),
        _ => None,
    };
    let base_count = if depth.is_some() { 3 } else { 2 };
    let diameter = or_none!(design_positive_length(or_none!(parameter("HoleDiameter"))));
    let tip_angle = or_none!(design_angle(or_none!(parameter("TipAngle"))));
    if tip_angle.get() <= 0.0 || tip_angle.get() > std::f64::consts::PI {
        return Ok(None);
    }
    let entry = match parameters.len().checked_sub(base_count) {
        Some(0) => None,
        Some(2) => match (
            (parameter("CBDepth"), parameter("CBDiameter")),
            (parameter("CSDiameter"), parameter("CSAngle")),
        ) {
            ((Some(counterbore_depth), Some(counterbore_diameter)), _) => {
                let counterbore_depth = or_none!(design_positive_length(counterbore_depth));
                let counterbore_diameter = or_none!(design_positive_length(counterbore_diameter));
                if depth.is_some_and(|depth| counterbore_depth.get() > depth)
                    || counterbore_diameter.get() <= diameter.get()
                {
                    return Ok(None);
                }
                Some(HoleEntry::Counterbore(
                    counterbore_diameter,
                    counterbore_depth,
                ))
            }
            (_, (Some(countersink_diameter), Some(countersink_angle))) => {
                let countersink_diameter = or_none!(design_positive_length(countersink_diameter));
                let countersink_angle = or_none!(cadmpeg_ir::scalar::InteriorAngle::try_from(
                    or_none!(design_angle(countersink_angle))
                )
                .ok());
                Some(HoleEntry::Countersink(
                    countersink_diameter,
                    countersink_angle,
                ))
            }
            _ => return Ok(None),
        },
        _ => return Ok(None),
    };
    // The tip angle lies in (0, pi]. Pi states a flat bottom; every smaller
    // tip angle is an interior drill-point angle.
    let drill_point_angle = cadmpeg_ir::scalar::InteriorAngle::try_from(tip_angle).ok();
    let (kind, bottom) = match (entry, drill_point_angle) {
        (Some(HoleEntry::Countersink(diameter, angle)), None) => (
            HoleKind::Countersink { diameter, angle },
            Some(HoleBottom::Flat),
        ),
        (Some(HoleEntry::Countersink(diameter, angle)), Some(included_angle)) => (
            HoleKind::Countersink { diameter, angle },
            Some(HoleBottom::Angled {
                included_angle,
                depth_to_tip: false,
            }),
        ),
        (None, None) => (HoleKind::Simple, Some(HoleBottom::Flat)),
        (None, Some(drill_point_angle)) => (
            HoleKind::SimpleDrilled { drill_point_angle },
            Some(HoleBottom::Angled {
                included_angle: drill_point_angle,
                depth_to_tip: false,
            }),
        ),
        (Some(HoleEntry::Counterbore(diameter, depth)), None) => (
            HoleKind::Counterbore { diameter, depth },
            Some(HoleBottom::Flat),
        ),
        (Some(HoleEntry::Counterbore(diameter, depth)), Some(drill_point_angle)) => (
            HoleKind::CounterboreDrilled {
                diameter,
                depth,
                drill_point_angle,
            },
            Some(HoleBottom::Angled {
                included_angle: drill_point_angle,
                depth_to_tip: false,
            }),
        ),
    };
    let face = match resolved_direct_face_selection(scope, face_operands)? {
        Some(face) => face,
        None => FaceSelection::Native(scope.id.clone()),
    };
    let placements = hole_placements(scope);
    if placements.is_none() {
        losses.push(crate::loss::F3dLossCode::HolePlacementUnproven.note(format!(
            "Hole {} retains its native placement because its carrier and point-data records do not define an admitted placement.", scope.id
        )));
    }
    Ok(Some(FeatureDefinition::Operation(FeatureOperation::Hole {
        profile: None,
        profile_filter: None,
        face: Some(face),
        direction: None,
        placements,
        shape: or_none!(cadmpeg_ir::features::holes::HoleShape::new(
            cadmpeg_ir::features::holes::HoleConstruction::Form {
                kind,
                specification: None,
            },
            None,
            Some(diameter),
        )
        .ok()),

        extent: Some(extent),
        bottom,
        taper_angle: None,
        allow_multi_profile_faces: None,
    })))
}

fn project_replace_face(
    scope: &DesignParameterScope,
    construction_groups: &[DesignConstructionOperandGroup],
    face_operands: &[DesignFaceOperand],
    body_recipe_operands: &[DesignBodyRecipeOperand],
) -> Option<cadmpeg_ir::features::FeatureDefinition> {
    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};

    if scope.kind() != crate::records::feature::scope::DesignFeatureKind::ReplaceFace
        || scope.class_tag.as_str() != "301"
        || scope.paired_class_tag.as_str() != "258"
        || scope.frame_length() != 290
        || scope.reference_members().len() != 4
    {
        return None;
    }
    let stream = native_stream(&scope.id)?;
    let mut groups = construction_groups.iter().filter(|group| {
        native_stream(&group.id) == Some(stream) && group.scope_record_index == scope.record_index
    });
    let (Some(first), Some(second), None) = (groups.next(), groups.next(), groups.next()) else {
        return None;
    };
    let (replacement_group, target_group) =
        if first.scope_reference_ordinal <= second.scope_reference_ordinal {
            (first, second)
        } else {
            (second, first)
        };
    let references = scope.reference_members().values_array::<4>();
    let references = references?.map(|value| *value);
    if replacement_group.scope_reference_ordinal != 0
        || replacement_group.record_index != references[0]
        || replacement_group.role() != DesignOperandRole::ROLE_0X9
        || !replacement_group
            .members()
            .iter()
            .map(|member| member.value)
            .eq(references[1..2].iter().copied())
        || target_group.scope_reference_ordinal != 2
        || target_group.record_index != references[2]
        || target_group.role() != DesignOperandRole::ROLE_0X10
        || !target_group
            .members()
            .iter()
            .map(|member| member.value)
            .eq(references[3..4].iter().copied())
    {
        return None;
    }
    let replacements =
        resolved_body_recipe_selection(scope, replacement_group, body_recipe_operands)?;
    let targets = resolved_historical_face_group(
        scope,
        scope.previous_history_state_id(),
        target_group,
        face_operands,
    )?;
    Some(FeatureDefinition::Operation(
        FeatureOperation::ReplaceFace {
            operands: cadmpeg_ir::features::ReplaceFaceOperands::new(targets, replacements).ok()?,
        },
    ))
}

/// Project the source selections of a `SurfaceTrim` scope.
///
/// Fusion stores the surface target as a role-`0x04` body-recipe group and
/// the trimming path as a role-`0x21` entity-selection group. The cell table
/// that selects the cells to remove is decoded separately and bound to the
/// projected operation after the source selections have been resolved.
fn project_surface_trim(
    scope: &DesignParameterScope,
    construction_groups: &[DesignConstructionOperandGroup],
    body_recipe_operands: &[DesignBodyRecipeOperand],
) -> Option<cadmpeg_ir::features::FeatureDefinition> {
    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation, PathRef, TrimRegion};

    if scope.kind() != crate::records::feature::scope::DesignFeatureKind::SurfaceTrim
        || scope.reference_members().len() != 4
    {
        return None;
    }
    let stream = native_stream(&scope.id)?;
    let references = scope.reference_members().values_array::<4>();
    let references = references?.map(|value| *value);
    let mut groups = construction_groups.iter().filter(|group| {
        native_stream(&group.id) == Some(stream) && group.scope_record_index == scope.record_index
    });
    let (Some(first), Some(second), None) = (groups.next(), groups.next(), groups.next()) else {
        return None;
    };
    let (target_group, tool_group) =
        if first.scope_reference_ordinal <= second.scope_reference_ordinal {
            (first, second)
        } else {
            (second, first)
        };
    if target_group.scope_reference_ordinal != 0
        || target_group.record_index != references[0]
        || target_group.role() != DesignOperandRole::BODIES_A
        || !target_group
            .members()
            .iter()
            .map(|member| member.value)
            .eq(references[1..2].iter().copied())
        || tool_group.scope_reference_ordinal != 2
        || tool_group.record_index != references[2]
        || tool_group.role() != DesignOperandRole::ROLE_0X21
        || !tool_group
            .members()
            .iter()
            .map(|member| member.value)
            .eq(references[3..4].iter().copied())
    {
        return None;
    }
    let faces = resolved_body_recipe_selection(scope, target_group, body_recipe_operands)?;
    Some(FeatureDefinition::Operation(
        FeatureOperation::TrimSurface {
            faces,
            tool: PathRef::Native(tool_group.id.clone()),
            keep: TrimRegion::Unresolved,
        },
    ))
}

/// Bind the exact removed-cell set retained by a decoded `SurfaceTrim`.
///
/// A source trim selects cells for removal. That set is independent of the
/// canonical inside/outside shorthand and must remain explicit when it is not
/// reducible to one of those two regions.
pub(crate) fn bind_surface_trim_cell_selections(
    features: &mut [cadmpeg_ir::features::Feature],
    scopes: &[DesignParameterScope],
    operations: &[DesignSurfaceTrimOperation],
) {
    for feature in features {
        if !matches!(
            feature.evaluation.definition(),
            cadmpeg_ir::features::FeatureDefinition::Operation(
                cadmpeg_ir::features::FeatureOperation::TrimSurface {
                    keep: cadmpeg_ir::features::TrimRegion::Unresolved,
                    ..
                }
            )
        ) {
            continue;
        }
        let Some(native_ref) = feature.native_ref.as_deref() else {
            continue;
        };
        let Some(scope) = scopes.iter().find(|scope| scope.id == native_ref) else {
            continue;
        };
        let Some(operation) = operations.iter().find(|operation| {
            operation.scope_record_index == scope.record_index
                && native_stream(&operation.id) == native_stream(&scope.id)
        }) else {
            continue;
        };
        let mut removed = Vec::new();
        for entry in operation.cell_entries() {
            removed.push(entry.ordinal);
        }
        let Some(selection) = cadmpeg_ir::features::TrimCellSelection::new(
            removed,
            u64::from(operation.trailing_value),
        ) else {
            continue;
        };
        feature.evaluation.edit(|definition, _| {
            if let cadmpeg_ir::features::FeatureDefinition::Operation(
                cadmpeg_ir::features::FeatureOperation::TrimSurface { keep, .. },
            ) = definition
            {
                *keep = cadmpeg_ir::features::TrimRegion::Cells(selection);
            }
        });
    }
}

pub(super) fn project_split(
    scope: &DesignParameterScope,
    scopes: &[DesignParameterScope],
    construction_groups: &[DesignConstructionOperandGroup],
    entity_selection_operands: &[crate::records::topology::entity_selection::DesignEntitySelectionOperand],
    face_operands: &[DesignFaceOperand],
) -> Result<Option<cadmpeg_ir::features::FeatureDefinition>, CodecError> {
    use cadmpeg_ir::features::{
        BodySelection, FaceSelection, FeatureDefinition, FeatureOperation, SplitBodyTool,
    };

    let parsed = (|| {
        if scope.kind() != crate::records::feature::scope::DesignFeatureKind::Split
            || scope.reference_members().len() < 4
        {
            return None;
        }
        let stream = native_stream(&scope.id)?;
        let mut groups = construction_groups.iter().filter(|group| {
            native_stream(&group.id) == Some(stream)
                && group.scope_record_index == scope.record_index
        });
        let (Some(first), Some(second), None) = (groups.next(), groups.next(), groups.next())
        else {
            return None;
        };
        let (tool_group, targets) =
            if first.scope_reference_ordinal <= second.scope_reference_ordinal {
                (first, second)
            } else {
                (second, first)
            };
        let target_ordinal = tool_group.members().len().checked_add(1)?;
        let tool_members = scope.reference_members().values_in(1..target_ordinal)?;
        let target_record_index = *scope.reference_members().values().nth(target_ordinal)?;
        let target_members = scope
            .reference_members()
            .values_in(target_ordinal.checked_add(1)?..scope.reference_members().len())?;
        if tool_group.scope_reference_ordinal != 0
            || tool_group.record_index != *scope.reference_members().values().next()?
            || tool_group.members().is_empty()
            || !tool_group
                .members()
                .iter()
                .map(|member| member.value)
                .eq(tool_members.copied())
            || usize::try_from(targets.scope_reference_ordinal).ok()? != target_ordinal
            || targets.record_index != target_record_index
            || targets.role() != DesignOperandRole::BODIES_A
            || targets.members().is_empty()
            || !targets
                .members()
                .iter()
                .map(|member| member.value)
                .eq(target_members.copied())
        {
            return None;
        }
        Some((stream, tool_group, targets))
    })();
    let Some((stream, tool_group, targets)) = parsed else {
        return Ok(None);
    };
    let tool = match tool_group.role() {
        DesignOperandRole::ROLE_0X9 => {
            let [crate::records::identity::Located {
                value: tool_record_index,
                ..
            }] = tool_group.members()
            else {
                return Ok(None);
            };
            let tool = unique_feature_match(face_operands.iter().filter(|operand| {
                native_stream(&operand.id) == Some(stream)
                    && operand.scope_record_index == scope.record_index
                    && operand.scope_reference_ordinal == 1
                    && operand.record_index() == *tool_record_index
                    && operand.recipe_kind == ConstructionRecipeKind::Face
                    && operand.recipe_program.as_slice() == [0, -1]
                    && operand.recipe_nodes.is_empty()
            }));
            let Some(tool) = tool else {
                return Ok(None);
            };
            let mut tools = if let Some(selection) = resolved_historical_face_operand(scope, tool) {
                selection
            } else {
                direct_face_selection(scope, face_operands)
                    .unwrap_or_else(|| FaceSelection::Native(String::new()))
            };
            match &mut tools {
                FaceSelection::Resolved { native, .. } | FaceSelection::Native(native) => {
                    native.clone_from(&tool.id);
                }
                FaceSelection::Historical { native, .. }
                | FaceSelection::HistoricalPartial { native, .. } => {
                    *native = or_none!(cadmpeg_core::text::NonBlankString::new(tool.id.clone()));
                }
                _ => {}
            }
            SplitBodyTool::Faces(tools)
        }
        DesignOperandRole::ROLE_0X21 => {
            match selected_work_planes(scope, tool_group, entity_selection_operands, scopes)
                .as_deref()
            {
                Some([plane]) => SplitBodyTool::Plane {
                    plane: crate::ids::neutral_feature_id(plane),
                },
                _ => SplitBodyTool::Faces(FaceSelection::Native(tool_group.id.clone())),
            }
        }
        _ => return Ok(None),
    };
    Ok(Some(FeatureDefinition::Operation(
        FeatureOperation::SplitBody {
            targets: BodySelection::Native(targets.id.clone()),
            tool,
        },
    )))
}

fn project_split_face(
    scope: &DesignParameterScope,
    scopes: &[DesignParameterScope],
    construction_groups: &[DesignConstructionOperandGroup],
    entity_selection_operands: &[crate::records::topology::entity_selection::DesignEntitySelectionOperand],
    face_operands: &[DesignFaceOperand],
    histories: &[crate::history_records::AsmHistory],
) -> Result<Option<cadmpeg_ir::features::FeatureDefinition>, CodecError> {
    use cadmpeg_ir::features::{
        FaceSelection, FeatureDefinition, FeatureOperation, PathRef, SplitFaceTool,
    };

    let reference_count = scope.reference_members().len();
    if scope.kind() != crate::records::feature::scope::DesignFeatureKind::SplitFace
        || reference_count < 4
    {
        return Ok(None);
    }
    let reference_tail_length = or_none!(11_u64.checked_mul(or_none!(u64::try_from(or_none!(
        reference_count.checked_sub(1)
    ))
    .ok())));
    let frame_base = or_none!(scope.frame_length().checked_sub(reference_tail_length));
    let compact = matches!(
        (scope.class_tag.as_str(), scope.paired_class_tag.as_str()),
        ("418", "266") | ("277", "258")
    );
    if !(matches!(frame_base, 290 | 291) || compact && frame_base == 286) {
        return Ok(None);
    }
    let stream = or_none!(native_stream(&scope.id));
    let mut groups = construction_groups.iter().filter(|group| {
        native_stream(&group.id) == Some(stream) && group.scope_record_index == scope.record_index
    });
    let (Some(first), Some(second), None) = (groups.next(), groups.next(), groups.next()) else {
        return Ok(None);
    };
    let (tool, targets) = if first.scope_reference_ordinal <= second.scope_reference_ordinal {
        (first, second)
    } else {
        (second, first)
    };
    let target_ordinal = or_none!(tool.members().len().checked_add(1));
    if tool.scope_reference_ordinal != 0
        || tool.record_index != *or_none!(scope.reference_members().values().next())
        || tool.role() != DesignOperandRole::ROLE_0X21
        || tool.members().is_empty()
        || !tool
            .members()
            .iter()
            .map(|member| member.value)
            .eq(or_none!(scope.reference_members().values_in(1..target_ordinal)).copied())
        || or_none!(usize::try_from(targets.scope_reference_ordinal).ok()) != target_ordinal
        || targets.record_index != *or_none!(scope.reference_members().values().nth(target_ordinal))
        || targets.role() != DesignOperandRole::ROLE_0X10
        || targets.members().is_empty()
        || !targets.members().iter().map(|member| member.value).eq(scope
            .reference_members()
            .values()
            .skip(target_ordinal + 1)
            .copied())
    {
        return Ok(None);
    }
    let target_selection = project_face_selection(scope, targets, face_operands, histories)?;
    let tool = if let Some(path) =
        resolved_split_face_path(scope, tool, entity_selection_operands, histories)
    {
        SplitFaceTool::Path(path)
    } else if let Some(planes) =
        selected_work_planes(scope, tool, entity_selection_operands, scopes)
    {
        let mut selected = Vec::new();
        for plane in planes {
            let id = crate::ids::neutral_feature_id(plane);
            selected.push(id);
        }
        if selected.len() == 1 {
            SplitFaceTool::Plane {
                plane: or_none!(selected.pop()),
            }
        } else {
            SplitFaceTool::Planes {
                planes: or_none!(selected.try_into().ok()),
            }
        }
    } else {
        SplitFaceTool::Path(PathRef::Native(tool.id.clone()))
    };
    Ok(Some(FeatureDefinition::Operation(
        FeatureOperation::SplitFace {
            targets: if matches!(target_selection, FaceSelection::Native(_)) {
                FaceSelection::Native(targets.id.clone())
            } else {
                target_selection
            },
            tool,
        },
    )))
}

/// A `MoveFace` scope projects when its transform is a pure translation.
fn project_move_face(
    scope: &DesignParameterScope,
    construction_groups: &[DesignConstructionOperandGroup],
) -> Result<Option<cadmpeg_ir::features::FeatureDefinition>, CodecError> {
    use cadmpeg_ir::features::{
        FaceMotion, FaceSelection, FeatureDefinition, FeatureDirection3, FeatureOperation,
    };

    let operation = or_none!(scope.move_operation());
    if !operation.preserves_orientation()
        || matrix_axis_angle(operation.transform.as_ref()).is_some()
    {
        return Ok(None);
    }
    let group = or_none!(single_operand_group(
        construction_groups,
        scope,
        DesignOperandRole::ROLE_0X10
    ));
    let member_count = or_none!(scope.reference_members().len().checked_sub(2));
    if group.scope_reference_ordinal != 0
        || group.record_index != *or_none!(scope.reference_members().values().next())
        || !group.members().iter().map(|member| member.value).eq(scope
            .reference_members()
            .values()
            .skip(1)
            .take(member_count)
            .copied())
    {
        return Ok(None);
    }
    let translation = Vector3::new(
        operation.transform[0][3] * 10.0,
        operation.transform[1][3] * 10.0,
        operation.transform[2][3] * 10.0,
    );
    let direction = or_none!(cadmpeg_ir::units::UnitVector3::normalized(translation));
    let distance = or_none!(cadmpeg_ir::scalar::Length::new(translation.norm()));
    Ok(Some(FeatureDefinition::Operation(
        FeatureOperation::MoveFace {
            faces: FaceSelection::Native(group.id.clone()),
            motion: FaceMotion::Translate {
                direction: FeatureDirection3::from(direction),
                distance,
            },
        },
    )))
}

fn project_delete_face(
    scope: &DesignParameterScope,
    construction_groups: &[DesignConstructionOperandGroup],
    face_operands: &[DesignFaceOperand],
) -> Result<Option<cadmpeg_ir::features::FeatureDefinition>, CodecError> {
    use cadmpeg_ir::features::{FaceSelection, FeatureDefinition, FeatureOperation};

    let reference_count = scope.reference_members().len();
    let reference_bytes =
        or_none!(11_u64.checked_mul(or_none!(u64::try_from(reference_count).ok())));
    let base_frame_length = or_none!(scope.frame_length().checked_sub(reference_bytes));
    let base_kind_offset = or_none!(scope
        .kind_offset()
        .checked_sub(scope.byte_offset())
        .and_then(|offset| offset.checked_sub(reference_bytes)));
    let heal = match scope.kind_name() {
        // The compact frame's fixed scope tail is 77 or 78 bytes. The legacy
        // frame is the compact frame without its four-byte marker-one field;
        // its class tags vary between files and do not select it.
        "DeleteFace" => match (base_frame_length, base_kind_offset) {
            (236 | 237, 139) | (241, 143) | (232, 135) => true,
            _ => return Ok(None),
        },
        "SurfaceDeleteFace" => {
            let common_layout = matches!(
                (base_frame_length, base_kind_offset),
                (250, 140) | (251, 139)
            );
            let class_layout = matches!(
                (
                    scope.class_tag.as_str(),
                    scope.paired_class_tag.as_str(),
                    base_frame_length,
                    base_kind_offset,
                ),
                ("287", "270", 245, 135)
                    | ("287", "270", 256, 146)
                    | ("327" | "545", "257", 250, 139)
                    | ("414", "263", 250, 140)
                    | ("497", "259", 257, 146)
                    | ("545", "257", 246, 135)
                    | ("545", "257", 257, 146)
            );
            if common_layout || class_layout {
                false
            } else {
                return Ok(None);
            }
        }
        _ => return Ok(None),
    };
    if reference_count < 2 {
        return Ok(None);
    }
    let stream = or_none!(native_stream(&scope.id));
    let matching = construction_groups.iter().filter(|group| {
        native_stream(&group.id) == Some(stream) && group.scope_record_index == scope.record_index
    });
    let Some(group) = unique_feature_match(matching) else {
        return Ok(None);
    };
    if group.scope_reference_ordinal != 0
        || group.record_index != *or_none!(scope.reference_members().values().next())
        || group.role() != DesignOperandRole::ROLE_0X10
        || !group.members().iter().map(|member| member.value).eq(scope
            .reference_members()
            .values()
            .skip(1)
            .copied())
    {
        return Ok(None);
    }
    let faces = resolved_historical_face_group(
        scope,
        scope.previous_history_state_id(),
        group,
        face_operands,
    )
    .map_or_else(
        || {
            if heal {
                Ok(None)
            } else {
                resolved_face_group(group, face_operands)
            }
        },
        |face| Ok(Some(face)),
    )?;
    let faces = match faces {
        Some(faces) => faces,
        None => FaceSelection::Native(group.id.clone()),
    };
    Ok(Some(FeatureDefinition::Operation(
        FeatureOperation::DeleteFace { faces, heal },
    )))
}

fn project_extrude(
    ctx: &DecodeContext<'_>,
    scope: &DesignParameterScope,
    parameters: &[(u32, &DesignParameter)],
    construction_groups: &[DesignConstructionOperandGroup],
    face_operands: &[DesignFaceOperand],
    placements: &[DesignSketchPlacement],
    body_recipe_operands: &[DesignBodyRecipeOperand],
) -> Result<Option<cadmpeg_ir::features::FeatureDefinition>, CodecError> {
    use cadmpeg_ir::features::{
        BooleanOp, ExtrudeDirection, ExtrudeExtent, ExtrudeSide, ExtrudeStart, FaceSelection,
        FeatureDefinition, FeatureOperation, LinearTermination, PlanarProfileRef, ProfileRef,
    };
    use cadmpeg_ir::scalar::{Angle, Length, NonZeroLength};
    // Per-side terminations without side-local modifiers; drafts and offsets
    // are attached below once they are resolved.
    enum ExtentShape {
        OneSided(LinearTermination),
        Symmetric(LinearTermination),
        TwoSided {
            first: LinearTermination,
            second: LinearTermination,
        },
    }
    #[derive(Clone, Copy)]
    enum AlongDirection {
        SignedDistance,
        PrologueReversal,
    }

    if scope.extrude_native_profiles().is_some() {
        return Ok(None);
    }
    let supported_parameter = |source_kind: &str| {
        matches!(
            source_kind,
            "AlongDistance"
                | "AgainstDistance"
                | "ProfileOffset"
                | "Side1Offset"
                | "Side2Offset"
                | "TaperAngle"
                | "Side2TaperAngle"
        )
    };
    if parameters
        .iter()
        .any(|(_, parameter)| !supported_parameter(parameter.source_kind_str()))
    {
        return Ok(None);
    }
    let mut scope_groups = Vec::new();
    for group in construction_groups.iter().filter(|group| {
        native_stream(&group.id) == native_stream(&scope.id)
            && group.scope_record_index == scope.record_index
    }) {
        scope_groups.push(group);
    }
    let profile_groups = or_none!(extrude_profile_group_roots(
        ctx,
        scope,
        construction_groups
    )?);
    let prologue = or_none!(scope.extrude_prologue());
    let profile_ref = match scope.extrude_profile() {
        Some(profile) => {
            let placement = or_none!(placements.iter().find(|placement| {
                native_stream(&placement.id) == native_stream(&scope.id)
                    && placement.entity_id == profile.entity_id
            }));
            ProfileRef::Planar(PlanarProfileRef::Sketch(crate::ids::neutral_sketch_id(
                placement,
            )))
        }
        None => {
            let [first, rest @ ..] = profile_groups.as_slice() else {
                return Ok(None);
            };
            if rest.is_empty() {
                match resolved_extrude_profile_face_group(
                    ctx,
                    scope,
                    first,
                    construction_groups,
                    face_operands,
                )? {
                    Some(profile) => profile,
                    None => ProfileRef::Planar(PlanarProfileRef::Native(first.id.clone())),
                }
            } else {
                let mut state = None;
                let mut faces = Vec::new();
                let mut native = Vec::new();
                let mut complete = true;
                for group in &profile_groups {
                    let Some(selection) = resolved_extrude_profile_face_group(
                        ctx,
                        scope,
                        group,
                        construction_groups,
                        face_operands,
                    )?
                    else {
                        complete = false;
                        break;
                    };
                    let ProfileRef::Planar(PlanarProfileRef::HistoricalFaces {
                        state: selected_state,
                        faces: selected_faces,
                        native: selected_native,
                    }) = selection
                    else {
                        complete = false;
                        break;
                    };
                    if state.as_ref().is_some_and(|state| state != &selected_state) {
                        complete = false;
                        break;
                    }
                    state = Some(selected_state);
                    for face in selected_faces {
                        if !faces.contains(&face) {
                            faces.push(face);
                        }
                    }
                    for id in selected_native {
                        native.push(id);
                    }
                }
                ProfileRef::Planar(match (complete, state) {
                    (true, Some(state)) if !faces.is_empty() => {
                        match PlanarProfileRef::historical_faces(state, faces, native) {
                            Ok(profile) => profile,
                            Err(_) => PlanarProfileRef::Native(scope.id.clone()),
                        }
                    }
                    _ => PlanarProfileRef::Native(scope.id.clone()),
                })
            }
        }
    };
    let mut face_groups = Vec::new();
    for group in scope_groups
        .iter()
        .filter(|group| {
            group
                .extrude_role()
                .is_some_and(|role| matches!(role, DesignExtrudeOperandRole::Faces(_)))
        })
        .copied()
    {
        face_groups.push(group);
    }
    let unique = |source_kind: &str| {
        let mut matches = parameters
            .iter()
            .map(|(_, parameter)| *parameter)
            .filter(|parameter| parameter.source_kind() == source_kind);
        let first = matches.next();
        matches.next().is_none().then_some(first)
    };
    let parameter_along = match or_none!(unique("AlongDistance")) {
        Some(parameter) => Some(or_none!(design_length(parameter))),
        None => None,
    };
    let fixed_along = scope
        .fixed_extrude_parameters()
        .as_ref()
        .and_then(|fixed| fixed.along_distance.as_ref())
        .map(|fixed| match fixed {
            DesignFixedExtrudeDistance::FixedScalar(scalar) => {
                Length::new(scalar.value.get() * 10.0)
                    .map(|length| (length, AlongDirection::SignedDistance))
                    .ok_or(())
            }
            DesignFixedExtrudeDistance::DistanceConstruction(scalar) => {
                Length::new(scalar.value.get() * 10.0)
                    .map(|length| (length, AlongDirection::PrologueReversal))
                    .ok_or(())
            }
        })
        .transpose()
        .ok();
    let fixed_along = or_none!(fixed_along);
    // A fixed symmetric distance lane does not state whether it is the half
    // or the whole travel (open item DR-78), so it defines no neutral length.
    if parameter_along.is_none()
        && fixed_along.is_some()
        && prologue.extent() == Some(DesignExtrudeExtent::SymmetricDistance)
    {
        return Ok(None);
    }
    let along = match (parameter_along, fixed_along) {
        (Some(parameter), Some((fixed, AlongDirection::SignedDistance)))
            if (parameter.get() - fixed.get()).abs() <= EPS_FEATURE_PROJECT_PROJECT_EXTRUDE_E9 =>
        {
            Some((parameter, AlongDirection::SignedDistance))
        }
        (Some(distance), None) => Some((distance, AlongDirection::SignedDistance)),
        (None, Some(distance)) => Some(distance),
        (None, None) => None,
        _ => return Ok(None),
    };
    let parameter_against = match or_none!(unique("AgainstDistance")) {
        Some(parameter) => Some(or_none!(design_length(parameter))),
        None => None,
    };
    let fixed_against = scope
        .fixed_extrude_parameters()
        .as_ref()
        .and_then(|fixed| fixed.against_distance.as_ref())
        .map(|fixed| Length::new(fixed.value.get() * 10.0).ok_or(()))
        .transpose()
        .ok();
    let fixed_against = or_none!(fixed_against);
    let against = match (parameter_against, fixed_against) {
        (Some(parameter), Some(fixed))
            if (parameter.get() - fixed.get()).abs() <= EPS_FEATURE_PROJECT_PROJECT_EXTRUDE_E9 =>
        {
            Some(parameter)
        }
        (Some(distance), None) | (None, Some(distance)) => Some(distance),
        (None, None) => None,
        _ => return Ok(None),
    };
    let fixed_length =
        |lane: Option<&crate::records::feature::fixed_parameters::DesignFixedExtrudeScalar>| {
            lane.map(|lane| Length::new(lane.value.get() * 10.0).ok_or(()))
                .transpose()
                .ok()
        };
    let fixed_lanes = scope.fixed_extrude_parameters();
    // When a parameter and a fixed lane both carry an offset, they agree.
    let offset = |parameter: Option<Length>, lane: Option<Length>| match (parameter, lane) {
        (Some(parameter), Some(lane))
            if (parameter.get() - lane.get()).abs() > EPS_FEATURE_PROJECT_PROJECT_EXTRUDE_E9 =>
        {
            Err(())
        }
        (Some(value), _) | (None, Some(value)) => Ok(Some(value)),
        (None, None) => Ok(None),
    };
    let parameter_profile_offset = match or_none!(unique("ProfileOffset")) {
        Some(parameter) => Some(or_none!(design_length(parameter))),
        None => None,
    };
    let profile_offset = or_none!(offset(
        parameter_profile_offset,
        or_none!(fixed_length(
            fixed_lanes.and_then(|fixed| fixed.profile_offset.as_ref())
        )),
    )
    .ok());
    let parameter_side_one_offset = match or_none!(unique("Side1Offset")) {
        Some(parameter) => Some(or_none!(design_length(parameter))),
        None => None,
    };
    let side_one_offset = or_none!(offset(
        parameter_side_one_offset,
        or_none!(fixed_length(
            fixed_lanes.and_then(|fixed| fixed.side_one_offset.as_ref())
        )),
    )
    .ok());
    let side_one_offset_count = parameters
        .iter()
        .filter(|(_, parameter)| parameter.source_kind() == "Side1Offset")
        .count();
    let omitted_zero_side_one_offset = side_one_offset.is_none()
        && extrude_omits_zero_side_one_offset(scope, &prologue, side_one_offset_count);
    let face_side_one_offset =
        side_one_offset.or_else(|| omitted_zero_side_one_offset.then_some(Length::ZERO));
    let effective_side_one_offset = side_one_offset.filter(|offset| offset.get() != 0.0);
    let parameter_side_two_offset = match or_none!(unique("Side2Offset")) {
        Some(parameter) => Some(or_none!(design_length(parameter))),
        None => None,
    };
    let side_two_offset = or_none!(offset(
        parameter_side_two_offset,
        or_none!(fixed_length(
            fixed_lanes.and_then(|fixed| fixed.side_two_offset.as_ref())
        )),
    )
    .ok());
    let effective_side_two_offset = side_two_offset.filter(|offset| offset.get() != 0.0);
    if effective_side_two_offset.is_some()
        && !matches!(
            scope
                .extrude_prologue()
                .and_then(DesignExtrudePrologue::extent),
            Some(
                DesignExtrudeExtent::TwoSidedToFaces | DesignExtrudeExtent::TwoSidedDistanceToFace,
            )
        )
    {
        return Ok(None);
    }
    let parameter_side_two_draft = match or_none!(unique("Side2TaperAngle")) {
        Some(parameter) => Some(or_none!(design_angle(parameter))),
        None => None,
    };
    let fixed_side_two_draft = scope
        .fixed_extrude_parameters()
        .as_ref()
        .and_then(|fixed| fixed.side_two_taper_angle.as_ref())
        .map(|fixed| Angle::from_assigned_real(fixed.value));
    let side_two_draft = match (parameter_side_two_draft, fixed_side_two_draft) {
        (Some(parameter), Some(fixed))
            if (parameter.get() - fixed.get()).abs() <= EPS_FEATURE_PROJECT_PROJECT_EXTRUDE_E12 =>
        {
            Some(parameter)
        }
        (Some(angle), None) | (None, Some(angle)) => Some(angle),
        (None, None) => None,
        _ => return Ok(None),
    };
    let mut start_groups = Vec::new();
    for group in face_groups
        .iter()
        .filter(|group| group.extrude_face_role() == Some(DesignExtrudeFaceRole::Start))
        .copied()
    {
        start_groups.push(group);
    }
    let mut termination_groups = Vec::new();
    for group in face_groups
        .iter()
        .filter(|group| group.extrude_face_role() == Some(DesignExtrudeFaceRole::Termination))
        .copied()
    {
        termination_groups.push(group);
    }
    let first_side_target_ordinal = match prologue {
        DesignExtrudePrologue::ReferenceAware {
            first_side_target_ordinal,
            ..
        } => first_side_target_ordinal.map(|target| target.scope_reference_ordinal),
        DesignExtrudePrologue::LegacyDistance { .. }
        | DesignExtrudePrologue::ShiftedReferenceAware { .. }
        | DesignExtrudePrologue::LegacyShifted { .. } => None,
    };
    let mut target_shape_groups = Vec::new();
    for group in scope_groups
        .iter()
        .filter(|group| {
            group.role() == DesignOperandRole::ROLE_0X5
                && group.extrude_role().is_none()
                && group.extrude_face_role().is_none()
                && first_side_target_ordinal
                    .is_none_or(|ordinal| group.scope_reference_ordinal == ordinal)
        })
        .copied()
    {
        target_shape_groups.push(group);
    }
    if start_groups.len() + termination_groups.len() != face_groups.len() {
        return Ok(None);
    }
    let selected_face =
        |group: &DesignConstructionOperandGroup| -> Result<FaceSelection, CodecError> {
            if let Some(selection) = resolved_historical_face_group(
                scope,
                scope.previous_history_state_id(),
                group,
                face_operands,
            ) {
                return Ok(selection);
            }
            match resolved_face_group(group, face_operands)? {
                Some(selection) => Ok(selection),
                None => Ok(FaceSelection::Native(group.id.clone())),
            }
        };
    let start = match prologue.start() {
        DesignExtrudeStart::ProfilePlane if start_groups.is_empty() => {
            if profile_offset.is_some() {
                return Ok(None);
            }
            ExtrudeStart::ProfilePlane {}
        }
        DesignExtrudeStart::OffsetProfilePlane if start_groups.is_empty() => {
            ExtrudeStart::OffsetProfilePlane {
                offset: or_none!(profile_offset),
            }
        }
        DesignExtrudeStart::FromFace => {
            let [start] = start_groups.as_slice() else {
                return Ok(None);
            };
            let offset = or_none!(profile_offset);
            ExtrudeStart::FromFace {
                face: selected_face(start)?,
                offset: (offset.get() != 0.0).then_some(offset),
            }
        }
        _ => return Ok(None),
    };
    // A zero distance admits no nonzero length. Its `Some(None)` matches no
    // distance arm and no arm that requires an absent distance, so the final
    // arm refuses it.
    let along = along.map(|(along, direction)| (NonZeroLength::try_from(along).ok(), direction));
    let against = against.map(|against| NonZeroLength::try_from(against).ok());
    let (shape, reverse_direction) = match (or_none!(prologue.extent()), along, against) {
        (DesignExtrudeExtent::OneSidedDistance, Some((Some(along), along_direction)), None)
            if (matches!(along_direction, AlongDirection::PrologueReversal)
                || !prologue.direction_reversed())
                && termination_groups.is_empty()
                && effective_side_one_offset.is_none() =>
        {
            (
                ExtentShape::OneSided(LinearTermination::Blind {
                    length: NonZeroLength::from(along.abs()),
                }),
                match along_direction {
                    AlongDirection::SignedDistance => along.get() < 0.0,
                    AlongDirection::PrologueReversal => prologue.direction_reversed(),
                },
            )
        }
        (
            DesignExtrudeExtent::TwoSidedDistance,
            Some((Some(along), AlongDirection::SignedDistance)),
            Some(Some(against)),
        ) if !prologue.direction_reversed()
            && termination_groups.is_empty()
            && effective_side_one_offset.is_none() =>
        {
            (
                ExtentShape::TwoSided {
                    first: LinearTermination::Blind {
                        length: NonZeroLength::from(along.abs()),
                    },
                    second: LinearTermination::Blind {
                        length: NonZeroLength::from(against.abs()),
                    },
                },
                along.get() < 0.0,
            )
        }
        (
            DesignExtrudeExtent::TwoSidedDistanceToFace,
            Some((Some(along), AlongDirection::SignedDistance)),
            None,
        ) if !prologue.direction_reversed()
            && termination_groups.len() == 1
            && target_shape_groups.is_empty()
            && effective_side_one_offset.is_none()
            && side_two_offset.is_some() =>
        {
            let [termination] = termination_groups.as_slice() else {
                return Ok(None);
            };
            (
                ExtentShape::TwoSided {
                    first: LinearTermination::Blind {
                        length: NonZeroLength::from(along.abs()),
                    },
                    second: LinearTermination::ToFace {
                        face: selected_face(termination)?,
                        offset: side_two_offset.filter(|offset| offset.get() != 0.0),
                    },
                },
                along.get() < 0.0,
            )
        }
        (DesignExtrudeExtent::TwoSidedToFaces, None, None)
            if termination_groups.len() == 2
                && target_shape_groups.is_empty()
                && side_one_offset.is_some()
                && side_two_offset.is_some() =>
        {
            let [first, second] = termination_groups.as_slice() else {
                return Ok(None);
            };
            (
                ExtentShape::TwoSided {
                    first: LinearTermination::ToFace {
                        face: selected_face(first)?,
                        offset: side_one_offset.filter(|offset| offset.get() != 0.0),
                    },
                    second: LinearTermination::ToFace {
                        face: selected_face(second)?,
                        offset: side_two_offset.filter(|offset| offset.get() != 0.0),
                    },
                },
                prologue.direction_reversed(),
            )
        }
        (
            DesignExtrudeExtent::SymmetricDistance,
            Some((Some(along), AlongDirection::SignedDistance)),
            None,
        ) if !prologue.direction_reversed()
            && termination_groups.is_empty()
            && effective_side_one_offset.is_none() =>
        {
            // Dual distance lanes share one per-side distance: each side
            // travels it, so the neutral whole travel is twice the input.
            // A single lane keeps the stored whole-travel reading.
            let length = if prologue.side_extent_discriminators() == Some([1, 1]) {
                or_none!(NonZeroLength::new(2.0 * along.get()))
            } else {
                along
            };
            (
                ExtentShape::Symmetric(LinearTermination::Blind {
                    length: NonZeroLength::from(length.abs()),
                }),
                along.get() < 0.0,
            )
        }
        (DesignExtrudeExtent::SymmetricThroughAll, None, None)
            if !prologue.direction_reversed()
                && termination_groups.is_empty()
                && effective_side_one_offset.is_none() =>
        {
            (
                ExtentShape::Symmetric(LinearTermination::ThroughAll {}),
                false,
            )
        }
        (DesignExtrudeExtent::OneSidedToFace, None, None) => {
            match (
                termination_groups.as_slice(),
                target_shape_groups.as_slice(),
            ) {
                ([termination], []) => {
                    let offset = or_none!(face_side_one_offset);
                    (
                        ExtentShape::OneSided(LinearTermination::ToFace {
                            face: selected_face(termination)?,
                            offset: (offset.get() != 0.0).then_some(offset),
                        }),
                        prologue.direction_reversed(),
                    )
                }
                ([], [target]) if effective_side_one_offset.is_none() => {
                    let target =
                        match resolved_body_recipe_shape(scope, target, body_recipe_operands) {
                            Some(selection) => selection,
                            None => FaceSelection::Native(target.id.clone()),
                        };
                    (
                        ExtentShape::OneSided(LinearTermination::ToShape { target }),
                        prologue.direction_reversed(),
                    )
                }
                _ => return Ok(None),
            }
        }
        (DesignExtrudeExtent::OneSidedThroughNext, None, None)
            if termination_groups.is_empty() && effective_side_one_offset.is_none() =>
        {
            (
                ExtentShape::OneSided(LinearTermination::ThroughNext {}),
                prologue.direction_reversed(),
            )
        }
        (DesignExtrudeExtent::OneSidedThroughAll, None, None)
            if termination_groups.is_empty() && effective_side_one_offset.is_none() =>
        {
            (
                ExtentShape::OneSided(LinearTermination::ThroughAll {}),
                prologue.direction_reversed(),
            )
        }
        _ => return Ok(None),
    };
    let direction = if reverse_direction {
        ExtrudeDirection::ReversedProfileNormal {}
    } else {
        ExtrudeDirection::ProfileNormal {}
    };
    let parameter_draft = match or_none!(unique("TaperAngle")) {
        Some(parameter) => {
            let angle = or_none!(design_angle(parameter));
            Some(angle)
        }
        None => None,
    };
    let fixed_draft = scope
        .fixed_extrude_parameters()
        .as_ref()
        .and_then(|fixed| fixed.taper_angle.as_ref())
        .map(|fixed| Angle::from_assigned_real(fixed.value));
    let draft = match (parameter_draft, fixed_draft) {
        (Some(parameter), Some(fixed))
            if (parameter.get() - fixed.get()).abs() <= EPS_FEATURE_PROJECT_PROJECT_EXTRUDE_E12 =>
        {
            Some(parameter)
        }
        (Some(angle), None) | (None, Some(angle)) => Some(angle),
        (None, None) => None,
        _ => return Ok(None),
    }
    .filter(|angle| angle.get() != 0.0)
    .map(cadmpeg_ir::scalar::SlopeAngle::try_from)
    .transpose()
    .ok();
    let draft = or_none!(draft);
    let second_draft = side_two_draft
        .filter(|angle| angle.get() != 0.0)
        .map(cadmpeg_ir::scalar::SlopeAngle::try_from)
        .transpose()
        .ok();
    let second_draft = or_none!(second_draft);
    // A side-two draft requires a two-sided extent. Other extents have no neutral
    // field for it, so return None.
    if second_draft.is_some() && !matches!(shape, ExtentShape::TwoSided { .. }) {
        return Ok(None);
    }
    let extent = match shape {
        ExtentShape::OneSided(termination) => ExtrudeExtent::OneSided {
            side: ExtrudeSide { termination, draft },
        },
        ExtentShape::Symmetric(termination) => ExtrudeExtent::Symmetric {
            side: ExtrudeSide { termination, draft },
        },
        ExtentShape::TwoSided { first, second } => ExtrudeExtent::TwoSided {
            first: ExtrudeSide {
                termination: first,
                draft,
            },
            second: ExtrudeSide {
                termination: second,
                draft: second_draft,
            },
        },
    };
    let has_body_operands = scope_groups
        .iter()
        .any(|group| group.extrude_role() == Some(DesignExtrudeOperandRole::Bodies));
    let op = match (prologue.operation(), has_body_operands) {
        (DesignExtrudeOperation::Join, true) => BooleanOp::Join,
        (DesignExtrudeOperation::Cut, true) => BooleanOp::Cut,
        (DesignExtrudeOperation::Intersect, true) => BooleanOp::Intersect,
        (DesignExtrudeOperation::NewBody, false) => BooleanOp::NewBody,
        _ => return Ok(None),
    };
    Ok(Some(FeatureDefinition::Operation(
        FeatureOperation::Extrude {
            profile: profile_ref,
            direction,
            start,
            extent,
            op,
            solid: Some(prologue.solid_operation()),
            face_maker: None,
            inner_wire_taper: None,
            length_along_profile_normal: None,
            allow_multi_profile_faces: None,
        },
    )))
}

fn spatial_sketch_entity_endpoints(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    entity: &cadmpeg_ir::sketches::SpatialSketchEntity,
) -> Result<Option<[Point3; 2]>, CodecError> {
    use cadmpeg_ir::sketches::SpatialSketchGeometryDefinition;

    match entity.geometry.definition() {
        SpatialSketchGeometryDefinition::Line { start, end } => Ok(Some([start.get(), end.get()])),
        SpatialSketchGeometryDefinition::Arc {
            center,
            normal,
            reference_direction,
            radius,
            start_angle,
            end_angle,
        } => {
            let normal = normal.as_raw();
            let reference_direction = reference_direction.as_raw();
            let transverse = normal.cross(*reference_direction);
            let at = |angle: f64| {
                center.translated(
                    reference_direction.scale(angle.cos()) + transverse.scale(angle.sin()),
                    radius.get(),
                )
            };
            Ok(Some([at(start_angle.get()), at(end_angle.get())]))
        }
        SpatialSketchGeometryDefinition::Nurbs { curve } if !curve.periodic() => {
            let Ok(degree) = usize::try_from(curve.degree()) else {
                return Ok(None);
            };
            let start = curve.knots()[degree];
            let end = curve.knots()[curve.pole_count()];
            let Some(first) =
                cadmpeg_ir::eval::finite_or_refusal(cadmpeg_ir::eval::decode::outer_refusal(
                    cadmpeg_ir::eval::decode::nurbs_curve_point_at(ctx, curve, start),
                )?)?
            else {
                return Ok(None);
            };
            let Some(last) =
                cadmpeg_ir::eval::finite_or_refusal(cadmpeg_ir::eval::decode::outer_refusal(
                    cadmpeg_ir::eval::decode::nurbs_curve_point_at(ctx, curve, end),
                )?)?
            else {
                return Ok(None);
            };
            Ok(Some([first.get(), last.get()]))
        }
        _ => Ok(None),
    }
}

/// Local work cap for the closed-profile chain search of one spatial sketch.
///
/// Each chain step scans every unused open edge, so the search is quadratic
/// in the sketch's open edges. An exhausted cap leaves the sketch with no
/// derived profiles.
pub(super) const SPATIAL_PROFILE_SEARCH_WORK: usize = 1 << 24;

/// Closed profiles of one spatial sketch, or `None` when the chain search
/// exhausts `budget`.
pub(super) fn closed_spatial_sketch_profiles(
    ctx: &DecodeContext<'_>,
    entities: &[&cadmpeg_ir::sketches::SpatialSketchEntity],
    tolerance: f64,
    budget: &cadmpeg_core::decode::WorkBudget,
) -> Result<Option<Vec<cadmpeg_ir::sketches::SpatialSketchProfile>>, CodecError> {
    use cadmpeg_ir::sketches::{
        SpatialSketchEntityUse, SpatialSketchGeometryDefinition, SpatialSketchProfile,
    };

    if !tolerance.is_finite() || tolerance <= 0.0 {
        return Ok(Some(Vec::new()));
    }
    let mut profiles = Vec::new();
    let mut edges = Vec::new();
    for entity in entities
        .iter()
        .copied()
        .filter(|entity| !entity.construction)
    {
        match entity.geometry.definition() {
            SpatialSketchGeometryDefinition::Circle {
                center,
                normal,
                reference_direction,
                ..
            } => {
                let boundary = vec![SpatialSketchEntityUse {
                    entity: entity.id().clone(),
                    reversed: false,
                }];
                if let Ok(profile) = SpatialSketchProfile::try_new(
                    center.get(),
                    *normal.as_raw(),
                    *reference_direction.as_raw(),
                    boundary,
                ) {
                    profiles.push(profile);
                }
            }
            _ => {
                if let Some(ends) = spatial_sketch_entity_endpoints(ctx, entity)? {
                    edges.push((entity, ends));
                }
            }
        }
    }
    let close = |a: Point3, b: Point3| (a.x - b.x).hypot(a.y - b.y).hypot(a.z - b.z) <= tolerance;
    let mut unused = HashSet::new();
    for index in 0..edges.len() {
        unused.insert(index);
    }
    loop {
        if !budget.charge_by(unused.len()) {
            return Ok(None);
        }
        let Some(&first) = unused
            .iter()
            .min_by(|a, b| edges[**a].0.id().cmp(edges[**b].0.id()))
        else {
            break;
        };
        unused.remove(&first);
        let mut uses = Vec::new();
        uses.push((first, false));
        let start = edges[first].1[0];
        let mut end = edges[first].1[1];
        while !close(end, start) {
            if !budget.charge_by(unused.len()) {
                return Ok(None);
            }
            let mut candidate = None;
            let mut ambiguous = false;
            for &index in &unused {
                let [candidate_start, candidate_end] = edges[index].1;
                let match_ = if close(end, candidate_start) {
                    Some((index, false))
                } else if close(end, candidate_end) {
                    Some((index, true))
                } else {
                    None
                };
                if let Some(match_) = match_ {
                    if candidate.replace(match_).is_some() {
                        ambiguous = true;
                        break;
                    }
                }
            }
            if ambiguous {
                break;
            }
            let Some(next) = candidate else {
                break;
            };
            unused.remove(&next.0);
            uses.push(next);
            end = if next.1 {
                edges[next.0].1[0]
            } else {
                edges[next.0].1[1]
            };
        }
        if !budget.charge_by(edges.len()) {
            return Ok(None);
        }
        let start_degree = edges
            .iter()
            .filter(|(_, [edge_start, edge_end])| {
                close(*edge_start, start) || close(*edge_end, start)
            })
            .count();
        if !close(end, start) || uses.len() < 3 || start_degree != 2 {
            continue;
        }
        let point_for = |(index, reversed): &(usize, bool)| edges[*index].1[usize::from(*reversed)];
        let origin = point_for(&uses[0]);
        let mut normal = Vector3::new(0.0, 0.0, 0.0);
        for pair in uses[1..].windows(2) {
            let a = point_for(&pair[0]).vector_from(origin);
            let b = point_for(&pair[1]).vector_from(origin);
            normal = normal + a.cross(b);
        }
        let normal_length = normal.norm();
        let first_end = edges[uses[0].0].1[1];
        let u = first_end.vector_from(origin);
        let u_length = u.norm();
        if normal_length <= tolerance || u_length <= tolerance {
            continue;
        }
        normal = normal.scale(1.0 / normal_length);
        let u_axis = u.scale(1.0 / u_length);
        if uses
            .iter()
            .any(|use_| point_for(use_).vector_from(origin).dot(normal).abs() > tolerance)
        {
            continue;
        }
        let mut boundary = Vec::new();
        for (index, reversed) in uses {
            boundary.push(SpatialSketchEntityUse {
                entity: edges[index].0.id().clone(),
                reversed,
            });
        }
        if let Ok(profile) = SpatialSketchProfile::try_new(origin, normal, u_axis, boundary) {
            profiles.push(profile);
        }
    }
    profiles.sort_by(|a, b| a.boundary()[0].entity.cmp(&b.boundary()[0].entity));
    Ok(Some(profiles))
}

fn project_coil(
    scope: &DesignParameterScope,
    parameters: &[(u32, &DesignParameter)],
    construction_groups: &[DesignConstructionOperandGroup],
) -> Result<Option<cadmpeg_ir::features::FeatureDefinition>, CodecError> {
    use cadmpeg_ir::features::{
        BodySelection, CoilConstruction, CoilExtent, CoilPlacement, CoilResult, CoilSection,
        CoilSectionPlacement, FeatureDefinition, FeatureOperation,
    };

    let parsed = (|| {
        let unique = |kind: &str| {
            let mut matches = parameters.iter().filter_map(|(_, parameter)| {
                (parameter.source_kind() == kind).then_some(*parameter)
            });
            let parameter = matches.next()?;
            matches.next().is_none().then_some(parameter)
        };
        let diameter = design_positive_length(unique("Diameter")?)?;
        let section_size = design_positive_length(unique("SectionSize")?)?;
        let dimensionless = |kind: &str| {
            let parameter = unique(kind)?;
            parameter.unit().is_none().then_some(())?;
            cadmpeg_ir::scalar::PositiveReal::new(parameter.evaluated_value().get())
        };
        let (extent, taper, expected_parameter_kinds): (_, _, &[&str]) =
            match scope.coil_extent()? {
                DesignCoilExtent::RevolutionsHeight => (
                    CoilExtent::RevolutionsHeight {
                        revolutions: dimensionless("Revolutions")?,
                        height: design_length(unique("Height")?)?,
                    },
                    design_angle(unique("TaperAngle")?)?,
                    &[
                        "Diameter",
                        "SectionSize",
                        "TaperAngle",
                        "Revolutions",
                        "Height",
                    ],
                ),
                DesignCoilExtent::RevolutionsPitch => (
                    CoilExtent::RevolutionsPitch {
                        revolutions: dimensionless("Revolutions")?,
                        pitch: cadmpeg_ir::scalar::NonZeroLength::try_from(design_length(unique(
                            "Pitch",
                        )?)?)
                        .ok()?,
                    },
                    design_angle(unique("TaperAngle")?)?,
                    &[
                        "Diameter",
                        "SectionSize",
                        "TaperAngle",
                        "Revolutions",
                        "Pitch",
                    ],
                ),
                DesignCoilExtent::HeightPitch => (
                    CoilExtent::HeightPitch {
                        height: cadmpeg_ir::scalar::NonZeroLength::try_from(design_length(
                            unique("Height")?,
                        )?)
                        .ok()?,
                        pitch: cadmpeg_ir::scalar::NonZeroLength::try_from(design_length(unique(
                            "Pitch",
                        )?)?)
                        .ok()?,
                    },
                    design_angle(unique("TaperAngle")?)?,
                    &["Diameter", "SectionSize", "TaperAngle", "Height", "Pitch"],
                ),
                DesignCoilExtent::Spiral => (
                    CoilExtent::Spiral {
                        revolutions: dimensionless("Revolutions")?,
                        radial_pitch: cadmpeg_ir::scalar::NonZeroLength::try_from(design_length(
                            unique("Pitch")?,
                        )?)
                        .ok()?,
                    },
                    cadmpeg_ir::scalar::Angle::ZERO,
                    &["Diameter", "SectionSize", "Revolutions", "Pitch"],
                ),
            };
        if parameters.len() != expected_parameter_kinds.len()
            || parameters.iter().any(|(_, parameter)| {
                !expected_parameter_kinds.contains(&parameter.source_kind_str())
            })
        {
            return None;
        }
        let section = match scope.coil_section()? {
            DesignCoilSection::Circular => CoilSection::Circular {
                diameter: section_size,
            },
            DesignCoilSection::Square => CoilSection::Square { size: section_size },
            DesignCoilSection::ExternalTriangle => {
                CoilSection::ExternalTriangle { size: section_size }
            }
            DesignCoilSection::InternalTriangle => {
                CoilSection::InternalTriangle { size: section_size }
            }
        };
        let section_placement = match scope.coil_section_placement()? {
            DesignCoilSectionPlacement::Inside => CoilSectionPlacement::Inside,
            DesignCoilSectionPlacement::Center => CoilSectionPlacement::Center,
            DesignCoilSectionPlacement::Outside => CoilSectionPlacement::Outside,
        };
        let operation = scope.coil_operation()?;
        let stream = native_stream(&scope.id)?;
        let first_body_group = if operation == DesignExtrudeOperation::NewBody {
            // The long Coil form carries one role-4 construction group for its
            // generated body even when the result is a new body. It is not a
            // Boolean target and must not suppress the typed result.
            None
        } else {
            let expected_role = if scope.coil_operation_offset()
                == scope
                    .byte_offset()
                    .checked_add(u64_from_index(coil_long::OPERATION))
            {
                DesignOperandRole::BODIES_A
            } else {
                DesignOperandRole::BODIES_B
            };
            let mut body_groups = construction_groups.iter().filter(|group| {
                native_stream(&group.id) == Some(stream)
                    && group.scope_record_index == scope.record_index
                    && group.role() == expected_role
            });
            let first_body_group = body_groups.next();
            if body_groups.next().is_some() {
                return None;
            }
            first_body_group
        };
        Some((
            diameter,
            extent,
            taper,
            section,
            section_placement,
            operation,
            first_body_group,
        ))
    })();
    let Some((diameter, extent, taper, section, section_placement, operation, first_body_group)) =
        parsed
    else {
        return Ok(None);
    };
    let result = match (operation, first_body_group) {
        (DesignExtrudeOperation::NewBody, None) => CoilResult::NewBody {},
        (operation, Some(group)) => CoilResult::Boolean {
            operation: match operation {
                DesignExtrudeOperation::Join => cadmpeg_ir::features::BooleanKind::Join,
                DesignExtrudeOperation::Cut => cadmpeg_ir::features::BooleanKind::Cut,
                DesignExtrudeOperation::Intersect => cadmpeg_ir::features::BooleanKind::Intersect,
                DesignExtrudeOperation::NewBody => return Ok(None),
            },
            targets: BodySelection::Native(group.id.clone()),
        },
        _ => return Ok(None),
    };
    let placement = scope
        .coil_placement()
        .as_ref()
        .map(|placement| placement.transform())
        .or_else(|| {
            scope
                .coil_transform()
                .as_ref()
                .map(|transform| &transform.transform)
        })
        .and_then(|transform| {
            cadmpeg_ir::features::FeatureUnitPlaneFrame::new(
                Point3::new(
                    transform[0][3] * 10.0,
                    transform[1][3] * 10.0,
                    transform[2][3] * 10.0,
                ),
                Vector3::new(transform[0][2], transform[1][2], transform[2][2]),
                Vector3::new(transform[0][0], transform[1][0], transform[2][0]),
            )
        });
    let placement = match placement {
        Some(frame) => CoilPlacement::Explicit { frame },
        None => CoilPlacement::Native {
            native_ref: match cadmpeg_ir::features::SelectionReference::try_from(scope.id.clone()) {
                Ok(reference) => reference,
                Err(_) => return Ok(None),
            },
        },
    };
    let clockwise = or_none!(scope.coil_clockwise());
    Ok(Some(FeatureDefinition::Operation(FeatureOperation::Coil {
        construction: CoilConstruction {
            placement,
            diameter,
            extent,
            section,
            section_placement,
            clockwise,
            taper,
        },
        result,
    })))
}

#[cfg(test)]
mod tests;
