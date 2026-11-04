// SPDX-License-Identifier: Apache-2.0
//! Project dimension constraint relations.

use crate::design::constraints::scalar_close;
use crate::design::feature_project::{design_dimension_unit, design_length};
use crate::design::geometry::{angle_in_sweep, sketch_entity_endpoints};
use crate::ids::native_stream;
use crate::records::{
    dimensions::{
        DesignDimensionAnnotationFrame, DesignDimensionLocusGroup, DesignDimensionLocusPair,
        DesignDimensionRecipeRecord,
    },
    parameters::{
        DesignParameter, DesignParameterCompanion, DesignParameterKind, DesignParameterOwner,
    },
    sketch_geometry::{SketchCurveIdentity, SketchPoint},
    sketch_placement::DesignSketchPlacement,
    sketch_relations::{SketchConstraintKind, SketchRelation, SketchRelationOperand},
};
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::geometry::pcurve::evaluator::PcurveEvaluatorLanes;
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::sketches::NativeOperandField;
use std::collections::{BTreeMap, HashMap, HashSet};

/// A sketch constraint governed by a dimension parameter.
fn dimension_constraint(
    id: cadmpeg_ir::sketches::SketchConstraintId,
    sketch: cadmpeg_ir::sketches::SketchId,
    definition: cadmpeg_ir::sketches::SketchConstraintDefinition,
    parameter: &DesignParameter,
    native_ref: String,
) -> cadmpeg_ir::sketches::SketchConstraint {
    cadmpeg_ir::sketches::SketchConstraint {
        id,
        sketch,
        definition,
        name: None,
        driving: parameter.driving(),
        active: None,
        virtual_space: None,
        visible: None,
        orientation: None,
        label_distance: None,
        label_position: None,
        metadata: None,
        native_ref: Some(native_ref),
    }
}

fn dimension_entity_ids_distinct(entities: &[&cadmpeg_ir::sketches::SketchEntity]) -> bool {
    let mut unique = HashSet::new();
    for entity in entities {
        if unique.contains(entity.id()) {
            return false;
        }
        unique.insert(entity.id());
    }
    true
}

const EPS_DIMENSIONS_OWNER_SCOPED_PARALLEL_LINE_SET_DIMENSION_DEFINITION_E9: f64 = 1.0e-9;
const EPS_DIMENSIONS_OWNER_SCOPED_LINE_LENGTH_DIMENSION_DEFINITION_E9: f64 = 1.0e-9;
const EPS_DIMENSIONS_UNIQUE_POINT_CLASS_DIMENSION_DEFINITION_E9: f64 = 1.0e-9;
const EPS_DIMENSIONS_OWNER_SCOPED_SPATIAL_LINE_LENGTH_DIMENSION_DEFINITION_E9: f64 = 1.0e-9;
const EPS_DIMENSIONS_OWNER_SCOPED_SPATIAL_PARALLEL_LINE_SET_DIMENSION_DEFINITION_E9: f64 = 1.0e-9;
const EPS_DIMENSIONS_SPATIAL_COUNTED_OFFSET_DIMENSION_DEFINITION_E9: f64 = 1.0e-9;
const EPS_DIMENSIONS_SPATIAL_POINT_DISTANCE_MATCHES_E9: f64 = 1.0e-9;
const EPS_DIMENSIONS_SPATIAL_PARALLEL_LINE_DISTANCE_MATCHES_E9: f64 = 1.0e-9;
const EPS_DIMENSIONS_SPATIAL_PARALLEL_LINE_DISTANCE_E12: f64 = 1.0e-12;
const EPS_DIMENSIONS_SPATIAL_PARALLEL_LINE_DISTANCE_E9: f64 = 1.0e-9;
const EPS_DIMENSIONS_RADIAL_DIMENSION_DEFINITION_AT_TOLERANCE_E9: f64 = 1.0e-9;
const EPS_DIMENSIONS_ANNOTATION_OFFSET_DIMENSION_DEFINITION_E9: f64 = 1.0e-9;
const EPS_DIMENSIONS_RADIAL_LOCUS_DIMENSION_DEFINITION_E9: f64 = 1.0e-9;
const EPS_DIMENSIONS_RADIAL_EXTENSION_ANNOTATION_GROUP_E12: f64 = 1.0e-12;
const EPS_DIMENSIONS_RADIAL_EXTENSION_ANNOTATION_GROUP_E9: f64 = 1.0e-9;
const EPS_DIMENSIONS_EXACT_COINCIDENT_LOCI_E9: f64 = 1.0e-9;
const EPS_DIMENSIONS_MIDPOINT_CONSTRAINT_E9: f64 = 1.0e-9;
const EPS_DIMENSIONS_RECIPE_LINEAR_DIMENSION_CANDIDATES_E9: f64 = 1.0e-9;
const EPS_DIMENSIONS_RECIPE_EXTENSION_POINT_DIMENSION_E9: f64 = 1.0e-9;
const EPS_DIMENSIONS_PARALLEL_LINE_DISTANCE_E12: f64 = 1.0e-12;
const EPS_DIMENSIONS_PARALLEL_LINE_DISTANCE_E9: f64 = 1.0e-9;
const EPS_DIMENSIONS_POINT_LINE_SEPARATION_E12: f64 = 1.0e-12;
const EPS_DIMENSIONS_LINEAR_MEASUREMENT_MATCHES_E9: f64 = 1.0e-9;
const EPS_DIMENSIONS_COUNTED_ROLE_RELATION_AT_TOLERANCE_E12: f64 = 1.0e-12;
const EPS_DIMENSIONS_COUNTED_ROLE_RELATION_AT_TOLERANCE_E9: f64 = 1.0e-9;
const EPS_DIMENSIONS_EXACT_LINE_ARC_TANGENCY_E12: f64 = 1.0e-12;
const EPS_DIMENSIONS_EXACT_LINE_ARC_TANGENCY_E9: f64 = 1.0e-9;
const EPS_DIMENSIONS_EXACT_EQUAL_SIZE_E9: f64 = 1.0e-9;
const EPS_DIMENSIONS_EXACT_EQUAL_SIZE_E12: f64 = 1.0e-12;
const EPS_DIMENSIONS_EXACT_COUNTED_DIMENSION_RELATION_E9: f64 = 1.0e-9;
const EPS_DIMENSIONS_POINT_LIES_ON_SKETCH_GEOMETRY_E9: f64 = 1.0e-9;
const EPS_DIMENSIONS_EXACT_COUNTED_OFFSET_E9: f64 = 1.0e-9;
const EPS_DIMENSIONS_OFFSET_PARAMETER_FACTOR_E9: f64 = 1.0e-9;
const EPS_DIMENSIONS_LINE_ANGLE_MATCHES_E9: f64 = 1.0e-9;
const EPS_DIMENSIONS_EXACT_OFFSET_CONSTRAINT_E9: f64 = 1.0e-9;
const EPS_DIMENSIONS_OFFSET_SOURCE_REVERSED_E9: f64 = 1.0e-9;
const EPS_DIMENSIONS_SKETCH_CURVE_OFFSET_E9: f64 = 1.0e-9;
const EPS_DIMENSIONS_PARALLEL_LINE_OFFSET_E12: f64 = 1.0e-12;
const EPS_DIMENSIONS_PARALLEL_LINE_OFFSET_E9: f64 = 1.0e-9;
const EPS_DIMENSIONS_REFLECTED_GEOMETRY_MATCHES_E9: f64 = 1.0e-9;
const EPS_DIMENSIONS_SKETCH_POINTS_CLOSE_E9: f64 = 1.0e-9;
const EPS_DIMENSIONS_PLANAR_POINT_E9: f64 = 1.0e-9;
const EPS_DIMENSIONS_SKETCH_NORMAL_SIGN_E9: f64 = 1.0e-9;

const PRESENTATION_MIN_LINE_LENGTH: f64 = 1.0e-12;

/// Record slices shared by every dimension-constraint projection: the sketch
/// placements, parameter and companion tables, the locus/group/annotation
/// dimension records, and the sketch geometry the loci reference.
pub(crate) struct DimensionConstraintInputs<'a> {
    pub(crate) placements: &'a [DesignSketchPlacement],
    pub(crate) parameters: &'a [DesignParameter],
    pub(crate) owners: &'a [DesignParameterOwner],
    pub(crate) pairs: &'a [DesignDimensionLocusPair],
    pub(crate) groups: &'a [DesignDimensionLocusGroup],
    pub(crate) annotation_frames: &'a [DesignDimensionAnnotationFrame],
    pub(crate) null_pairs: &'a [DesignDimensionLocusPair],
    pub(crate) companions: &'a [DesignParameterCompanion],
    pub(crate) recipe_records: &'a [DesignDimensionRecipeRecord],
    pub(crate) points: &'a [SketchPoint],
    pub(crate) curves: &'a [SketchCurveIdentity],
    pub(crate) entities: &'a [cadmpeg_ir::sketches::SketchEntity],
}

pub(crate) fn container_only_dimension_companions<'a>(
    pairs: &'a [DesignDimensionLocusPair],
    null_pairs: &'a [DesignDimensionLocusPair],
    annotation_frames: &'a [DesignDimensionAnnotationFrame],
    groups: &'a [DesignDimensionLocusGroup],
    recipe_records: &'a [DesignDimensionRecipeRecord],
) -> HashSet<(&'a str, u32)> {
    let physical_entries =
        pairs
            .iter()
            .filter_map(|pair| Some((native_stream(&pair.id)?, pair.companion_record_index)))
            .chain(
                null_pairs.iter().filter_map(|pair| {
                    Some((native_stream(&pair.id)?, pair.companion_record_index))
                }),
            )
            .chain(annotation_frames.iter().filter_map(|frame| {
                Some((native_stream(&frame.id)?, frame.companion_record_index?))
            }))
            .chain(groups.iter().filter_map(|group| {
                Some((native_stream(&group.id)?, group.companion_record_index))
            }))
            .chain(recipe_records.iter().filter_map(|record| {
                Some((native_stream(&record.id)?, record.companion_record_index))
            }));
    let mut physical = HashSet::new();
    for entry in physical_entries {
        physical.insert(entry);
    }
    let governed_entries =
        pairs
            .iter()
            .filter_map(|pair| {
                Some((
                    native_stream(&pair.id)?,
                    pair.governing_companion_record_index,
                ))
            })
            .chain(null_pairs.iter().filter_map(|pair| {
                Some((
                    native_stream(&pair.id)?,
                    pair.governing_companion_record_index,
                ))
            }))
            .chain(annotation_frames.iter().filter_map(|frame| {
                Some((
                    native_stream(&frame.id)?,
                    frame.governing_companion_record_index,
                ))
            }))
            .chain(groups.iter().filter_map(|group| {
                Some((native_stream(&group.id)?, group.companion_record_index))
            }))
            .chain(recipe_records.iter().filter_map(|record| {
                Some((native_stream(&record.id)?, record.companion_record_index))
            }));
    let mut governed = HashSet::new();
    for entry in governed_entries {
        governed.insert(entry);
    }
    let mut container_only = HashSet::new();
    for entry in physical.difference(&governed) {
        container_only.insert(*entry);
    }
    container_only
}

/// Project dimensional parameter companions into parameter-backed sketch
/// constraints. Solved linear measurements use the source kernel's absolute
/// resolution. Two-locus dimensions have neutral semantics; aggregate and
/// role-dependent forms remain explicit native constraints.
pub(crate) fn project_dimension_constraints(
    inputs: &DimensionConstraintInputs<'_>,
    spatial_sketches: &[cadmpeg_ir::sketches::SpatialSketch],
    linear_tolerance: f64,
) -> Vec<cadmpeg_ir::sketches::SketchConstraint> {
    let constraints = project_all_dimension_constraints(inputs, &[], linear_tolerance);
    retain_planar_dimension_constraints(inputs.placements, spatial_sketches, constraints)
}

/// Project planar dimensions with direct Fusion presentation frames. The
/// presentation carrier is kept separate from the established locus inputs so
/// callers that construct dimension fixtures do not need to synthesize an
/// unrelated native arena.
pub(crate) fn project_dimension_constraints_with_presentations(
    inputs: &DimensionConstraintInputs<'_>,
    presentation_frames: &[crate::records::dimensions::DesignDimensionPresentationFrame],
    spatial_sketches: &[cadmpeg_ir::sketches::SpatialSketch],
    linear_tolerance: f64,
) -> Vec<cadmpeg_ir::sketches::SketchConstraint> {
    let constraints =
        project_all_dimension_constraints(inputs, presentation_frames, linear_tolerance);
    retain_planar_dimension_constraints(inputs.placements, spatial_sketches, constraints)
}

fn retain_planar_dimension_constraints(
    placements: &[DesignSketchPlacement],
    spatial_sketches: &[cadmpeg_ir::sketches::SpatialSketch],
    constraints: Vec<cadmpeg_ir::sketches::SketchConstraint>,
) -> Vec<cadmpeg_ir::sketches::SketchConstraint> {
    let mut spatial_sketch_ids = HashSet::new();
    for sketch in spatial_sketches {
        spatial_sketch_ids.insert(&sketch.id);
    }
    let mut output = Vec::new();
    for constraint in constraints {
        let mut keep = true;
        for placement in placements {
            if crate::ids::neutral_sketch_id(placement) == constraint.sketch {
                keep =
                    !spatial_sketch_ids.contains(&crate::ids::neutral_spatial_sketch_id(placement));
                break;
            }
        }
        if keep {
            output.push(constraint);
        }
    }
    output
}

fn project_all_dimension_constraints(
    inputs: &DimensionConstraintInputs<'_>,
    presentation_frames: &[crate::records::dimensions::DesignDimensionPresentationFrame],
    linear_tolerance: f64,
) -> Vec<cadmpeg_ir::sketches::SketchConstraint> {
    use cadmpeg_ir::sketches::{
        SketchConstraintDefinitionInput as Definition, SketchGeometryDefinition,
        SketchNativeOperand,
    };

    let &DimensionConstraintInputs {
        placements,
        parameters,
        owners,
        pairs,
        groups,
        annotation_frames,
        null_pairs,
        companions,
        recipe_records,
        points,
        curves,
        entities,
    } = inputs;

    let mut sketches = HashMap::new();
    let mut sketches_by_scope = HashMap::new();
    for placement in placements {
        let Some(scope) = native_stream(&placement.id) else {
            continue;
        };
        if let Ok(suffix) = u32::try_from(placement.entity_id.suffix()) {
            sketches.insert((scope, suffix), crate::ids::neutral_sketch_id(placement));
        }
        if let Some(scope_record_index) = placement.scope_record_index {
            sketches_by_scope.insert(
                (scope, scope_record_index),
                crate::ids::neutral_sketch_id(placement),
            );
        }
    }
    let mut parameters_by_record = HashMap::new();
    for parameter in parameters {
        if let Some(scope) = native_stream(&parameter.id) {
            parameters_by_record.insert((scope, parameter.record_index), parameter);
        }
    }
    let parameters = parameters_by_record;
    let mut parameter_by_companion = HashMap::new();
    for owner in owners {
        if let Some(scope) = native_stream(owner.id()) {
            parameter_by_companion.insert(
                (scope, owner.companion_record_index()),
                owner.parameter_record_index(),
            );
        }
    }
    let mut native_geometry = HashMap::new();
    for point in points {
        if let Some(scope) = native_stream(&point.id) {
            native_geometry.insert(
                (scope, point.record_index),
                (
                    cadmpeg_core::nonblank_literal!("point"),
                    point.owner_reference,
                    point.id.as_str(),
                ),
            );
        }
    }
    for curve in curves {
        if let Some(scope) = native_stream(&curve.id) {
            native_geometry.insert(
                (scope, curve.record_index),
                (
                    cadmpeg_core::nonblank_literal!("curve"),
                    curve.owner_reference,
                    curve.id.as_str(),
                ),
            );
        }
    }
    let mut record_indices_by_native_ref = HashMap::new();
    for (key, (_, _, native_ref)) in &native_geometry {
        record_indices_by_native_ref.insert(*native_ref, *key);
    }
    let mut projected = HashMap::new();
    for entity in entities {
        if let Some(key) = entity
            .native_ref
            .as_deref()
            .and_then(|native_ref| record_indices_by_native_ref.get(native_ref).copied())
        {
            projected.insert(key, entity);
        }
    }
    let mut curve_secondary_ids = HashMap::new();
    for curve in curves {
        if let Some(scope) = native_stream(&curve.id) {
            curve_secondary_ids.insert((scope, curve.record_index), curve.secondary_id);
        }
    }

    let parameter_for = |scope: &str, companion_record_index: u32| {
        let record_index = *parameter_by_companion.get(&(scope, companion_record_index))?;
        let parameter = *parameters.get(&(scope, record_index))?;
        Some((parameter, crate::ids::neutral_parameter_id(parameter)))
    };
    let owner_presentation_definition =
        |scope: &str,
         owner_record_index: u32,
         parameter: &DesignParameter,
         parameter_id: &cadmpeg_ir::features::ParameterId| {
            governing_presentation_definition(
                presentation_frames.iter().filter(|frame| {
                    native_stream(&frame.id) == Some(scope)
                        && frame.governing_owner_record_index == owner_record_index
                }),
                scope,
                &projected,
                parameter,
                parameter_id,
                linear_tolerance,
            )
        };
    let sketch_for_geometry =
        |scope: &str, indices: &[u32]| -> Option<cadmpeg_ir::sketches::SketchId> {
            let projected_sketch = indices
                .iter()
                .filter_map(|record_index| projected.get(&(scope, *record_index)))
                .map(|entity| &entity.sketch)
                .next();
            if let Some(sketch) = projected_sketch.filter(|sketch| {
                indices.iter().all(|record_index| {
                    projected
                        .get(&(scope, *record_index))
                        .is_some_and(|entity| &entity.sketch == *sketch)
                })
            }) {
                return Some(sketch.clone());
            }
            let owner = indices
                .iter()
                .find_map(|record_index| native_geometry.get(&(scope, *record_index))?.1)?;
            if indices
                .iter()
                .filter_map(|record_index| native_geometry.get(&(scope, *record_index))?.1)
                .all(|candidate| candidate == owner)
            {
                sketches.get(&(scope, owner)).cloned()
            } else {
                None
            }
        };
    let sketch_for_owner_or_geometry =
        |scope: &str, owner: u32, indices: &[u32]| -> Option<cadmpeg_ir::sketches::SketchId> {
            if let Some(sketch) = sketches.get(&(scope, owner)) {
                return Some(sketch.clone());
            }
            sketch_for_geometry(scope, indices)
        };
    let native_operand = |scope: &str,
                          field: cadmpeg_core::text::NonBlankString,
                          role: Option<u32>,
                          record_index: u32|
     -> SketchNativeOperand {
        let geometry = native_geometry.get(&(scope, record_index));
        SketchNativeOperand {
            native_kind: match geometry {
                Some((kind, _, _)) => kind.clone(),
                None => cadmpeg_core::nonblank_literal!("record"),
            },
            field: Some(NativeOperandField { name: field, role }),
            object_index: Some(record_index),
            native_ref: geometry
                .filter(|_| !projected.contains_key(&(scope, record_index)))
                .map(|(_, _, native_ref)| (*native_ref).to_owned()),
        }
    };
    let native_definition =
        |scope: &str,
         source_kind: cadmpeg_core::text::NonBlankString,
         state: Option<u64>,
         operands: &[(cadmpeg_core::text::NonBlankString, Option<u32>, u32)],
         parameter|
         -> Definition {
            let mut entity_ids = Vec::new();
            let mut native_operands = Vec::new();
            for (field, role, record_index) in operands {
                if let Some(entity) = projected.get(&(scope, *record_index)) {
                    entity_ids.push(entity.id().clone());
                }
                let operand = native_operand(scope, field.clone(), *role, *record_index);
                native_operands.push(operand);
            }
            Definition::Native {
                native_kind: source_kind,
                native_state: state,
                native_flags: None,
                native_properties: std::collections::BTreeMap::new(),
                entities: entity_ids,
                parameter: Some(parameter),
                operands: native_operands,
            }
        };
    let exact_definition = |scope: &str,
                            source_parameter: &DesignParameter,
                            indices: &[u32],
                            parameter: cadmpeg_ir::features::ParameterId|
     -> Option<Definition> {
        if !design_dimension_unit(source_parameter) {
            return None;
        }
        let source_kind = source_parameter.source_kind_str();
        let evaluated_value = source_parameter.evaluated_value().get();
        let mut entities = Vec::new();
        for record_index in indices {
            let entity = projected.get(&(scope, *record_index)).copied()?;
            entities.push(entity);
        }
        if let [entity] = entities.as_slice() {
            let copied = parameter.clone();
            if let Some(definition) =
                radial_dimension_definition(entity, source_kind, evaluated_value, copied)
            {
                return Some(definition);
            }
        }
        if let [first, second] = entities.as_slice() {
            if first.id() == second.id() {
                return None;
            }
        }
        if source_kind.starts_with("Linear Dimension") && entities.len() == 2 {
            let evaluated_mm = evaluated_value * 10.0;
            let copied = parameter.clone();
            if let Some(definition) =
                directional_point_dimension(&entities, evaluated_mm, copied, linear_tolerance)
            {
                return Some(definition);
            }
            if point_line_separation(entities[0], entities[1], evaluated_mm, linear_tolerance)
                || parallel_line_separation(
                    entities[0],
                    entities[1],
                    evaluated_mm,
                    linear_tolerance,
                )
                || concentric_circle_separation(
                    entities[0],
                    entities[1],
                    evaluated_mm,
                    linear_tolerance,
                )
            {
                let ids: Vec<_> = entities.iter().map(|entity| entity.id().clone()).collect();
                return Some(Definition::Distance {
                    entities: ids,
                    parameter,
                });
            }
            let (
                SketchGeometryDefinition::Point {
                    position: first_position,
                },
                SketchGeometryDefinition::Point {
                    position: second_position,
                },
            ) = (
                entities[0].geometry.definition(),
                entities[1].geometry.definition(),
            )
            else {
                return None;
            };
            let measured =
                (first_position.u - second_position.u).hypot(first_position.v - second_position.v);
            if linear_measurement_matches(measured, evaluated_mm, linear_tolerance) {
                return Some(Definition::DistanceLoci {
                    first: cadmpeg_ir::sketches::SketchLocus::Entity(entities[0].id().clone()),
                    second: cadmpeg_ir::sketches::SketchLocus::Entity(entities[1].id().clone()),
                    parameter,
                });
            }
            return None;
        }
        if source_kind.starts_with("Angular Dimension")
            && entities.len() == 2
            && entities.iter().all(|entity| {
                matches!(
                    *entity.geometry.definition(),
                    SketchGeometryDefinition::Line { .. }
                )
            })
            && line_angle_matches(
                &entities[0].geometry,
                &entities[1].geometry,
                evaluated_value,
            )
        {
            return Some(Definition::Angle {
                first: entities[0].id().clone(),
                second: entities[1].id().clone(),
                parameter,
            });
        }
        if source_kind.starts_with("Angular Dimension") && entities.len() == 2 {
            let (first, second) =
                indirect_angular_lines(scope, &entities, evaluated_value, &projected)?;
            return Some(Definition::Angle {
                first,
                second,
                parameter,
            });
        }
        None
    };
    let exact_group_definition = |scope: &str,
                                  group: &DesignDimensionLocusGroup,
                                  parameter: &DesignParameter,
                                  parameter_id: cadmpeg_ir::features::ParameterId|
     -> Option<Definition> {
        if !design_dimension_unit(parameter) {
            return None;
        }
        let mut locus_entities = Vec::new();
        for locus in &group.loci {
            let entity = projected
                .get(&(scope, locus.geometry_record_index))
                .copied()?;
            locus_entities.push(entity);
        }
        if parameter.source_kind_str().starts_with("Angular Dimension") {
            let mut indices = Vec::new();
            for locus in &group.loci {
                indices.push(locus.geometry_record_index);
            }
            let copied = parameter_id.clone();
            if let Some(definition) = exact_definition(scope, parameter, &indices, copied) {
                return Some(definition);
            }
        }
        if group.state == 0 {
            let counted_definition = counted_role_relation_at_tolerance(
                &locus_entities,
                &group.owner_kinds(),
                linear_tolerance,
            );
            if let Some(definition) = counted_definition {
                return Some(definition);
            }
            if let Some(definition) = exact_counted_dimension_relation(&locus_entities) {
                return Some(definition);
            }
        }
        if let Some(definition) = radial_locus_dimension_definition(
            &locus_entities,
            entities,
            parameter.source_kind_str(),
            parameter.evaluated_value().get(),
            &parameter_id,
        ) {
            return Some(definition);
        }
        if parameter.source_kind_str().starts_with("Linear Dimension") {
            if group.state == 0x20 {
                let mut entities_by_record = HashMap::new();
                let mut secondary_ids = HashMap::new();
                for (locus, entity) in group.loci.iter().zip(&locus_entities) {
                    entities_by_record.insert(locus.geometry_record_index, *entity);
                    if let Some(secondary_id) = curve_secondary_ids
                        .get(&(scope, locus.geometry_record_index))
                        .copied()
                    {
                        secondary_ids.insert(locus.geometry_record_index, secondary_id);
                    }
                }
                let CountedOffset { pairs, distance } = exact_counted_offset(
                    &group.loci,
                    &entities_by_record,
                    &secondary_ids,
                    linear_tolerance,
                )?;
                let parameter = offset_parameter_factor(
                    distance.get(),
                    parameter.evaluated_value().get() * 10.0,
                )
                .map(|factor| cadmpeg_ir::sketches::OffsetParameter {
                    id: parameter_id,
                    negated: factor.is_sign_negative(),
                });
                return Some(Definition::Offset {
                    pairs,
                    distance,
                    parameter,
                });
            }
            let copied = parameter_id.clone();
            if let Some(definition) = directional_point_dimension(
                &locus_entities,
                parameter.evaluated_value().get() * 10.0,
                copied,
                linear_tolerance,
            ) {
                return Some(definition);
            }
            if group.state == 0 {
                return two_locus_distance_dimension(&locus_entities, parameter_id);
            }
        }
        None
    };
    let mut radial_extension_annotation_groups = HashSet::new();
    for group in groups {
        let candidate = (|| {
            let scope = native_stream(&group.id)?;
            if group.state != 0 {
                return None;
            }
            let (parameter, parameter_id) = parameter_for(scope, group.companion_record_index)?;
            let copied = parameter_id.clone();
            if exact_group_definition(scope, group, parameter, copied).is_some() {
                return None;
            }
            let mut locus_entities = Vec::new();
            for locus in &group.loci {
                let entity = projected
                    .get(&(scope, locus.geometry_record_index))
                    .copied()?;
                locus_entities.push(entity);
            }
            if !radial_extension_annotation_group(&locus_entities, parameter) {
                return None;
            }
            let mut locus_indices = Vec::new();
            for locus in &group.loci {
                locus_indices.push(locus.geometry_record_index);
            }
            let sketch =
                sketch_for_owner_or_geometry(scope, group.owner_reference, &locus_indices)?;
            owner_scoped_radial_dimension_definition(
                entities,
                &sketch,
                parameter,
                &parameter_id,
                linear_tolerance,
            )?;
            Some((scope, group.record_index))
        })();
        if let Some(result) = candidate {
            radial_extension_annotation_groups.insert(result);
        }
    }
    let mut exact_pair_companions = HashSet::new();
    for pair in pairs {
        let Some(scope) = native_stream(&pair.id) else {
            continue;
        };
        let Some((parameter, parameter_id)) =
            parameter_for(scope, pair.governing_companion_record_index)
        else {
            continue;
        };
        let indices = [
            pair.loci()[0].geometry_index(),
            pair.loci()[1].geometry_index(),
        ];
        if exact_definition(scope, parameter, &indices, parameter_id).is_some() {
            exact_pair_companions.insert((scope, pair.governing_companion_record_index));
        }
    }
    let mut parameterized_offset_companions = HashSet::new();
    for group in groups {
        let Some(scope) = native_stream(&group.id) else {
            continue;
        };
        let Some((parameter, parameter_id)) = parameter_for(scope, group.companion_record_index)
        else {
            continue;
        };
        if let Some(Definition::Offset {
            parameter: Some(_), ..
        }) = exact_group_definition(scope, group, parameter, parameter_id)
        {
            parameterized_offset_companions.insert((scope, group.companion_record_index));
        }
    }
    let mut projected_dimension_companions = HashSet::new();
    for pair in pairs {
        if let Some(scope) = native_stream(&pair.id) {
            projected_dimension_companions.insert((scope, pair.governing_companion_record_index));
        }
    }
    for frame in annotation_frames {
        if let Some(scope) = native_stream(&frame.id) {
            projected_dimension_companions.insert((scope, frame.governing_companion_record_index));
        }
    }
    for pair in null_pairs {
        if let Some(scope) = native_stream(&pair.id) {
            projected_dimension_companions.insert((scope, pair.governing_companion_record_index));
        }
    }
    for group in groups {
        let Some(scope) = native_stream(&group.id) else {
            continue;
        };
        if radial_extension_annotation_groups.contains(&(scope, group.record_index)) {
            continue;
        }
        let Some((parameter, parameter_id)) = parameter_for(scope, group.companion_record_index)
        else {
            continue;
        };
        let copied = parameter_id.clone();
        let definition = exact_group_definition(scope, group, parameter, copied);
        if definition.as_ref().is_none_or(|definition| {
            constraint_parameters(definition).any(|id| id == &parameter_id)
        }) {
            projected_dimension_companions.insert((scope, group.companion_record_index));
        }
    }

    let mut group_constraints = Vec::new();
    for group in groups {
        let projected = (|| {
            let scope = native_stream(&group.id)?;
            if radial_extension_annotation_groups.contains(&(scope, group.record_index)) {
                return None;
            }
            if exact_pair_companions.contains(&(scope, group.companion_record_index)) {
                return None;
            }
            let (parameter, parameter_id) = parameter_for(scope, group.companion_record_index)?;
            let mut locus_indices = Vec::new();
            for locus in &group.loci {
                locus_indices.push(locus.geometry_record_index);
            }
            let sketch =
                sketch_for_owner_or_geometry(scope, group.owner_reference, &locus_indices)?;
            let copied = parameter_id.clone();
            let exact = exact_group_definition(scope, group, parameter, copied);
            let definition = if let Some(definition) = exact {
                definition
            } else {
                let mut operands = Vec::new();
                for locus in &group.loci {
                    operands.push((
                        cadmpeg_core::nonblank_literal!("locus"),
                        Some(locus.role),
                        locus.geometry_record_index,
                    ));
                }
                operands.push((
                    cadmpeg_core::nonblank_literal!("owner"),
                    Some(group.owner_role),
                    group.owner_reference,
                ));
                for locus in &group.loci {
                    operands.push((
                        cadmpeg_core::nonblank_literal!("return"),
                        None,
                        locus.returned.value,
                    ));
                }
                native_definition(
                    scope,
                    parameter.source_kind().clone(),
                    Some(u64::from(group.state)),
                    &operands,
                    parameter_id,
                )
            };
            let definition =
                cadmpeg_ir::sketches::SketchConstraintDefinition::try_from(definition).ok()?;
            let native_ref = group.id.clone();
            Some(dimension_constraint(
                crate::ids::neutral_sketch_constraint_id(&group.id, group.record_index),
                sketch,
                definition,
                parameter,
                native_ref,
            ))
        })();
        if let Some(result) = projected {
            group_constraints.push(result);
        }
    }
    let mut pair_constraints = Vec::new();
    for pair in pairs {
        let Some(scope) = native_stream(&pair.id) else {
            continue;
        };
        let Some((parameter, parameter_id)) =
            parameter_for(scope, pair.governing_companion_record_index)
        else {
            continue;
        };
        let indices = [
            pair.loci()[0].geometry_index(),
            pair.loci()[1].geometry_index(),
        ];
        let Some(sketch) = sketch_for_geometry(scope, &indices) else {
            continue;
        };
        let constraint_id = crate::ids::neutral_dimension_constraint_id(&parameter_id, "pair");
        let copied = parameter_id.clone();
        let definition = exact_definition(scope, parameter, &indices, copied).or_else(|| {
            let [first_index, second_index] = indices;
            let first = projected.get(&(scope, first_index))?;
            let second = projected.get(&(scope, second_index))?;
            symmetric_parallel_line_dimension_definition(
                first,
                second,
                (pair.loci()[0].role, pair.loci()[1].role),
                parameter,
                parameter_id.clone(),
                linear_tolerance,
            )
        });
        let definition = if let Some(definition) = definition {
            definition
        } else {
            native_definition(
                scope,
                parameter.source_kind().clone(),
                None,
                &[
                    (
                        cadmpeg_core::nonblank_literal!("first_locus"),
                        Some(pair.loci()[0].role),
                        indices[0],
                    ),
                    (
                        cadmpeg_core::nonblank_literal!("second_locus"),
                        Some(pair.loci()[1].role),
                        indices[1],
                    ),
                ],
                parameter_id,
            )
        };
        let Ok(definition) = cadmpeg_ir::sketches::SketchConstraintDefinition::try_from(definition)
        else {
            continue;
        };
        pair_constraints.push(dimension_constraint(
            constraint_id,
            sketch,
            definition,
            parameter,
            pair.id.clone(),
        ));
    }
    let combined = pair_constraints
        .into_iter()
        .chain(group_constraints)
        .chain(annotation_frames.iter().filter_map(|frame| {
            let scope = native_stream(&frame.id)?;
            let (parameter, parameter_id) =
                parameter_for(scope, frame.governing_companion_record_index)?;
            let mut indices = Vec::new();
            for operand in frame.operands() {
                if let Some(index) = operand.geometry_record_index {
                    indices.push(index.get());
                }
            }
            let sketch = sketches.get(&(scope, frame.owner_reference))?.clone();
            let constraint_id =
                crate::ids::neutral_dimension_constraint_id(&parameter_id, "annotation");
            let copied = parameter_id.clone();
            let exact = exact_definition(scope, parameter, &indices, copied).or_else(|| {
                annotation_offset_dimension_definition(
                    frame,
                    (parameter, &parameter_id),
                    scope,
                    curves,
                    &projected,
                    linear_tolerance,
                )
            });
            let definition = if let Some(definition) = exact {
                definition
            } else {
                let mut operands = Vec::new();
                for operand in frame.operands() {
                    let native = match operand.geometry_record_index {
                        None => SketchNativeOperand {
                            native_kind: cadmpeg_core::nonblank_literal!("null_locus"),
                            field: Some(NativeOperandField {
                                name: cadmpeg_core::nonblank_literal!("locus"),
                                role: Some(operand.role),
                            }),
                            object_index: None,
                            native_ref: None,
                        },
                        Some(index) => native_operand(
                            scope,
                            cadmpeg_core::nonblank_literal!("locus"),
                            Some(operand.role),
                            index.get(),
                        ),
                    };
                    operands.push(native);
                }
                let mut entity_ids = Vec::new();
                for record_index in &indices {
                    if let Some(entity) = projected.get(&(scope, *record_index)) {
                        entity_ids.push(entity.id().clone());
                    }
                }
                Definition::Native {
                    native_kind: parameter.source_kind().clone(),
                    native_state: None,
                    native_flags: None,
                    native_properties: std::collections::BTreeMap::new(),
                    entities: entity_ids,
                    parameter: Some(parameter_id),
                    operands,
                }
            };
            let Ok(definition) =
                cadmpeg_ir::sketches::SketchConstraintDefinition::try_from(definition)
            else {
                return None;
            };
            let native_ref = frame.id.clone();
            Some(dimension_constraint(
                constraint_id,
                sketch,
                definition,
                parameter,
                native_ref,
            ))
        }))
        .chain(null_pairs.iter().filter_map(|pair| {
            let scope = native_stream(&pair.id)?;
            if parameterized_offset_companions
                .contains(&(scope, pair.governing_companion_record_index))
            {
                return None;
            }
            let (parameter, parameter_id) =
                parameter_for(scope, pair.governing_companion_record_index)?;
            let indices = [pair.loci()[1].geometry_index()];
            let sketch = sketch_for_geometry(scope, &indices)?;
            let constraint_id =
                crate::ids::neutral_dimension_constraint_id(&parameter_id, "null-pair");
            if design_dimension_unit(parameter) {
                if let Some(entity) = projected.get(&(scope, pair.loci()[1].geometry_index())) {
                    let copied = parameter_id.clone();
                    if let Some(definition) = null_locus_dimension_definition(
                        pair,
                        entity,
                        parameter.source_kind_str(),
                        parameter.evaluated_value().get(),
                        copied,
                        linear_tolerance,
                    ) {
                        let definition =
                            cadmpeg_ir::sketches::SketchConstraintDefinition::try_from(definition)
                                .ok()?;
                        let native_ref = pair.id.clone();
                        return Some(dimension_constraint(
                            constraint_id,
                            sketch,
                            definition,
                            parameter,
                            native_ref,
                        ));
                    }
                }
            }
            let mut operands = Vec::new();
            operands.push(SketchNativeOperand {
                native_kind: cadmpeg_core::nonblank_literal!("null_locus"),
                field: Some(NativeOperandField {
                    name: cadmpeg_core::nonblank_literal!("locus"),
                    role: Some(pair.loci()[0].role),
                }),
                object_index: None,
                native_ref: None,
            });
            let native = native_operand(
                scope,
                cadmpeg_core::nonblank_literal!("locus"),
                Some(pair.loci()[1].role),
                pair.loci()[1].geometry_index(),
            );
            operands.push(native);
            let mut entity_ids = Vec::new();
            for record_index in &indices {
                if let Some(entity) = projected.get(&(scope, *record_index)) {
                    entity_ids.push(entity.id().clone());
                }
            }
            let Ok(definition) =
                cadmpeg_ir::sketches::SketchConstraintDefinition::try_from(Definition::Native {
                    native_kind: parameter.source_kind().clone(),
                    native_state: None,
                    native_flags: None,
                    native_properties: std::collections::BTreeMap::new(),
                    entities: entity_ids,
                    parameter: Some(parameter_id),
                    operands,
                })
            else {
                return None;
            };
            Some(dimension_constraint(
                constraint_id,
                sketch,
                definition,
                parameter,
                pair.id.clone(),
            ))
        }));
    let mut constraints = Vec::new();
    for projected in combined {
        constraints.push(projected);
    }
    let mut companions_by_key = HashMap::new();
    for companion in companions {
        if let Some(scope) = native_stream(companion.id()) {
            companions_by_key.insert((scope, companion.record_index()), companion);
        }
    }
    let mut owners_by_companion = HashMap::new();
    for owner in owners {
        if let Some(scope) = native_stream(owner.id()) {
            owners_by_companion.insert((scope, owner.companion_record_index()), owner);
        }
    }
    let mut recipes_by_companion = BTreeMap::<(&str, u32), Vec<_>>::new();
    for record in recipe_records {
        let Some(scope) = native_stream(&record.id) else {
            continue;
        };
        if !projected_dimension_companions.contains(&(scope, record.companion_record_index)) {
            let key = (scope, record.companion_record_index);
            recipes_by_companion.entry(key).or_default().push(record);
        }
    }
    for records in recipes_by_companion.values_mut() {
        records.sort_by_key(|left| left.recipe_ordinal);
    }
    for ((scope, companion_record_index), records) in recipes_by_companion {
        let Some(companion) = companions_by_key.get(&(scope, companion_record_index)) else {
            continue;
        };
        let Some(owner) = owners_by_companion.get(&(scope, companion_record_index)) else {
            continue;
        };
        let Some((parameter, parameter_id)) = parameter_for(scope, companion_record_index) else {
            continue;
        };
        let constraint_id =
            crate::ids::neutral_dimension_constraint_id(&parameter_id, "recipe-group");
        let Some(sketch) = sketches_by_scope.get(&(scope, owner.scope_record_index())) else {
            continue;
        };
        let sketch = sketch.clone();
        let linear_candidates = if parameter.source_kind_str().starts_with("Linear Dimension")
            && design_dimension_unit(parameter)
        {
            recipe_linear_dimension_candidates(
                entities,
                &sketch,
                parameter.evaluated_value().get() * 10.0,
                &parameter_id,
                linear_tolerance,
            )
        } else {
            Vec::default()
        };
        let copied = parameter_id.clone();
        let repeated = repeated_linear_dimension(&linear_candidates, copied);
        let extension = recipe_extension_point_dimension(&linear_candidates, entities, &sketch);
        let radial = owner_scoped_radial_dimension_definition(
            entities,
            &sketch,
            parameter,
            &parameter_id,
            linear_tolerance,
        );
        let concentric = concentric_circle_dimension_definition(
            entities,
            &sketch,
            parameter,
            &parameter_id,
            linear_tolerance,
        );
        let presentation =
            owner_presentation_definition(scope, owner.record_index(), parameter, &parameter_id);
        let definition = if let Some(presentation) = presentation {
            presentation
        } else if let Some(radial) = radial {
            radial
        } else if linear_candidates.len() == 1 {
            let Some(definition) = linear_candidates.into_iter().next() else {
                continue;
            };
            definition
        } else if let Some(repeated) = repeated {
            repeated
        } else if let Some(extension) = extension {
            extension
        } else if let Some(concentric) = concentric {
            concentric
        } else {
            let mut operands = Vec::new();
            for record in records {
                let native_ref = record.id.clone();
                operands.push(SketchNativeOperand {
                    native_kind: cadmpeg_core::nonblank_literal!("construction_recipe"),
                    field: Some(NativeOperandField {
                        name: cadmpeg_core::nonblank_literal!("recipe"),
                        role: None,
                    }),
                    object_index: Some(record.record_index),
                    native_ref: Some(native_ref),
                });
            }
            Definition::Native {
                native_kind: parameter.source_kind().clone(),
                native_state: None,
                native_flags: None,
                native_properties: std::collections::BTreeMap::new(),
                entities: recipe_dimension_candidate_entities(&linear_candidates),
                parameter: Some(parameter_id),
                operands,
            }
        };
        let Ok(definition) = cadmpeg_ir::sketches::SketchConstraintDefinition::try_from(definition)
        else {
            continue;
        };
        let native_ref = companion.id().to_owned();
        constraints.push(dimension_constraint(
            constraint_id,
            sketch,
            definition,
            parameter,
            native_ref,
        ));
    }
    let mut projected_parameters = HashSet::new();
    for constraint in &constraints {
        for parameter in constraint_parameters(constraint.definition.kind()) {
            if !projected_parameters.contains(parameter) {
                let id = parameter.clone();
                projected_parameters.insert(id);
            }
        }
    }
    let container_only_payload_companions = container_only_dimension_companions(
        pairs,
        null_pairs,
        annotation_frames,
        groups,
        recipe_records,
    );
    for companion in companions {
        let projected = (|| {
            let scope = native_stream(companion.id())?;
            let owner = owners_by_companion.get(&(scope, companion.record_index()))?;
            let (parameter, parameter_id) = parameter_for(scope, companion.record_index())?;
            if parameter.kind() != DesignParameterKind::Dimension
                || projected_parameters.contains(&parameter_id)
                || container_only_payload_companions.contains(&(scope, companion.record_index()))
            {
                return None;
            }
            let sketch = sketches_by_scope
                .get(&(scope, owner.scope_record_index()))?
                .clone();
            let mut parallel_axis_angle = None;
            let mut saw_parallel_axis_angle = false;
            let mut multiple_parallel_axis_angles = false;
            for group in groups.iter().filter(|group| {
                native_stream(&group.id) == Some(scope)
                    && group.companion_record_index == companion.record_index()
            }) {
                let copied = parameter_id.clone();
                let Some(Definition::Parallel { first, second }) =
                    exact_group_definition(scope, group, parameter, copied)
                else {
                    continue;
                };
                let members = [
                    entities.iter().find(|entity| entity.id() == &first),
                    entities.iter().find(|entity| entity.id() == &second),
                ];
                let [Some(first), Some(second)] = members else {
                    continue;
                };
                let Some(definition) = parallel_group_axis_angle_definition(
                    &[first, second],
                    parameter,
                    &parameter_id,
                ) else {
                    continue;
                };
                if saw_parallel_axis_angle {
                    multiple_parallel_axis_angles = true;
                    parallel_axis_angle = None;
                } else {
                    saw_parallel_axis_angle = true;
                    parallel_axis_angle = Some(definition);
                }
            }
            if multiple_parallel_axis_angles {
                parallel_axis_angle = None;
            }
            let owner_scoped_definition = owner_scoped_radial_dimension_definition(
                entities,
                &sketch,
                parameter,
                &parameter_id,
                linear_tolerance,
            )
            .or_else(|| {
                preceding_incident_angular_dimension_definition(
                    scope,
                    points,
                    curves,
                    &projected,
                    &sketch,
                    (parameter, &parameter_id),
                )
            })
            .or_else(|| {
                owner_scoped_angular_dimension_definition(
                    entities,
                    &sketch,
                    parameter,
                    &parameter_id,
                )
            })
            .or_else(|| {
                owner_scoped_line_length_dimension_definition(
                    entities,
                    &sketch,
                    parameter,
                    &parameter_id,
                    linear_tolerance,
                )
            })
            .or_else(|| {
                unique_parallel_line_dimension_definition(
                    entities,
                    &sketch,
                    parameter,
                    &parameter_id,
                    linear_tolerance,
                )
            })
            .or_else(|| {
                owner_scoped_parallel_line_set_dimension_definition(
                    entities,
                    &sketch,
                    parameter,
                    &parameter_id,
                    linear_tolerance,
                )
            })
            .or_else(|| {
                unique_point_line_dimension_definition(
                    entities,
                    &sketch,
                    parameter,
                    &parameter_id,
                    linear_tolerance,
                )
            })
            .or_else(|| {
                unique_point_class_dimension_definition(
                    entities,
                    &sketch,
                    parameter,
                    &parameter_id,
                    linear_tolerance,
                )
            })
            .or_else(|| {
                concentric_circle_dimension_definition(
                    entities,
                    &sketch,
                    parameter,
                    &parameter_id,
                    linear_tolerance,
                )
            });
            let presentation_definition = owner_presentation_definition(
                scope,
                owner.record_index(),
                parameter,
                &parameter_id,
            );
            let exact_definition = presentation_definition
                .or(parallel_axis_angle)
                .or(owner_scoped_definition);
            if exact_definition.is_none()
                && companion
                    .payload()
                    .is_none_or(|payload| payload.byte_length() == 0)
            {
                return None;
            }
            let definition = if let Some(definition) = exact_definition {
                definition
            } else {
                let copied = parameter_id.clone();
                let native_ref = companion.id().to_owned();
                Definition::Native {
                    native_kind: parameter.source_kind().clone(),
                    native_state: None,
                    native_flags: None,
                    native_properties: std::collections::BTreeMap::new(),
                    entities: Vec::new(),
                    parameter: Some(copied),
                    operands: vec![SketchNativeOperand {
                        native_kind: cadmpeg_core::nonblank_literal!("dimension_companion"),
                        field: Some(NativeOperandField {
                            name: cadmpeg_core::nonblank_literal!("companion_payload"),
                            role: None,
                        }),
                        object_index: Some(companion.record_index()),
                        native_ref: Some(native_ref),
                    }],
                }
            };
            let definition =
                cadmpeg_ir::sketches::SketchConstraintDefinition::try_from(definition).ok()?;
            let native_ref = companion.id().to_owned();
            Some(dimension_constraint(
                crate::ids::neutral_dimension_constraint_id(&parameter_id, "companion-payload"),
                sketch,
                definition,
                parameter,
                native_ref,
            ))
        })();
        if let Some(result) = projected {
            constraints.push(result);
        }
    }
    constraints.sort_by(|a, b| a.id.cmp(&b.id));
    constraints
}

/// Resolve one direct presentation carrier only when its selected geometry
/// measures the parameter value. Presentation operands are authoritative for
/// selection, while the geometric check prevents a presentation record from
/// assigning a nearby entity with the same source sketch.
/// The definition of the one presentation frame among `frames` whose operand
/// measurement equals the parameter.
fn governing_presentation_definition<'a>(
    frames: impl Iterator<Item = &'a crate::records::dimensions::DesignDimensionPresentationFrame>,
    scope: &str,
    projected: &HashMap<(&str, u32), &cadmpeg_ir::sketches::SketchEntity>,
    parameter: &DesignParameter,
    parameter_id: &cadmpeg_ir::features::ParameterId,
    linear_tolerance: f64,
) -> Option<cadmpeg_ir::sketches::SketchConstraintDefinitionInput> {
    let mut definitions = frames.filter_map(|frame| {
        presentation_dimension_definition(
            scope,
            frame,
            projected,
            parameter,
            parameter_id,
            linear_tolerance,
        )
    });
    let definition = definitions.next()?;
    definitions.next().is_none().then_some(definition)
}

fn presentation_dimension_definition(
    scope: &str,
    frame: &crate::records::dimensions::DesignDimensionPresentationFrame,
    projected: &HashMap<(&str, u32), &cadmpeg_ir::sketches::SketchEntity>,
    parameter: &DesignParameter,
    parameter_id: &cadmpeg_ir::features::ParameterId,
    linear_tolerance: f64,
) -> Option<cadmpeg_ir::sketches::SketchConstraintDefinitionInput> {
    use cadmpeg_ir::sketches::{
        SketchConstraintDefinitionInput as Definition, SketchGeometryDefinition,
    };

    if !design_dimension_unit(parameter) {
        return None;
    }
    let mut entities = Vec::new();
    for operand in &frame.operands {
        let entity = projected
            .get(&(scope, operand.geometry_record_index.get()))
            .copied()?;
        entities.push(entity);
    }
    if entities.is_empty()
        || entities
            .iter()
            .any(|entity| entity.sketch != entities[0].sketch)
    {
        return None;
    }
    match entities.as_slice() {
        [entity] if parameter.source_kind_str().starts_with("Tangent Dimension") => {
            tangent_radius_dimension_definition(entity, parameter, parameter_id, linear_tolerance)
        }
        [entity] => radial_dimension_definition_at_tolerance(
            entity,
            parameter.source_kind_str(),
            parameter.evaluated_value().get(),
            parameter_id.clone(),
            linear_tolerance,
        )
        .or_else(|| {
            if !parameter.source_kind_str().starts_with("Linear Dimension") {
                return None;
            }
            let SketchGeometryDefinition::Line { start, end } = entity.geometry.definition() else {
                return None;
            };
            let measured = (end.u - start.u).hypot(end.v - start.v);
            linear_measurement_matches(
                measured,
                parameter.evaluated_value().get() * 10.0,
                linear_tolerance,
            )
            .then(|| Definition::DistanceLoci {
                first: cadmpeg_ir::sketches::SketchLocus::Start(entity.id().clone()),
                second: cadmpeg_ir::sketches::SketchLocus::End(entity.id().clone()),
                parameter: parameter_id.clone(),
            })
        }),
        [first, second] if parameter.source_kind_str().starts_with("Tangent Dimension") => {
            tangent_entity_distance_definition(
                first,
                second,
                parameter,
                parameter_id,
                linear_tolerance,
            )
        }
        [first, second] if parameter.source_kind_str().starts_with("Linear Dimension") => {
            explicit_linear_dimension_definition(
                first,
                second,
                parameter,
                parameter_id,
                linear_tolerance,
            )
        }
        [first, second] if parameter.source_kind_str().starts_with("Angular Dimension") => {
            line_angle_matches(
                &first.geometry,
                &second.geometry,
                parameter.evaluated_value().get(),
            )
            .then(|| Definition::Angle {
                first: first.id().clone(),
                second: second.id().clone(),
                parameter: parameter_id.clone(),
            })
        }
        _ => None,
    }
}

fn tangent_radius_dimension_definition(
    entity: &cadmpeg_ir::sketches::SketchEntity,
    parameter: &DesignParameter,
    parameter_id: &cadmpeg_ir::features::ParameterId,
    linear_tolerance: f64,
) -> Option<cadmpeg_ir::sketches::SketchConstraintDefinitionInput> {
    use cadmpeg_ir::sketches::{
        SketchConstraintDefinitionInput as Definition, SketchGeometryDefinition,
    };

    let radius = match entity.geometry.definition() {
        SketchGeometryDefinition::Circle { radius, .. }
        | SketchGeometryDefinition::Arc { radius, .. } => radius.get(),
        _ => return None,
    };
    linear_measurement_matches(
        radius,
        parameter.evaluated_value().get() * 10.0,
        linear_tolerance,
    )
    .then(|| Definition::Radius {
        entity: entity.id().clone(),
        parameter: parameter_id.clone(),
    })
}

fn tangent_entity_distance_definition(
    first: &cadmpeg_ir::sketches::SketchEntity,
    second: &cadmpeg_ir::sketches::SketchEntity,
    parameter: &DesignParameter,
    parameter_id: &cadmpeg_ir::features::ParameterId,
    linear_tolerance: f64,
) -> Option<cadmpeg_ir::sketches::SketchConstraintDefinitionInput> {
    use cadmpeg_ir::sketches::{
        SketchConstraintDefinitionInput as Definition, SketchGeometryDefinition,
    };

    let circle_geometry =
        |entity: &cadmpeg_ir::sketches::SketchEntity| match entity.geometry.definition() {
            SketchGeometryDefinition::Circle { center, radius }
            | SketchGeometryDefinition::Arc { center, radius, .. } => Some((*center, radius.get())),
            _ => None,
        };
    let candidates = match (first.geometry.definition(), second.geometry.definition()) {
        (
            SketchGeometryDefinition::Line { start, end },
            SketchGeometryDefinition::Circle { center, radius }
            | SketchGeometryDefinition::Arc { center, radius, .. },
        ) => {
            let line = (*start, *end);
            let circle = (*center, radius.get());
            let direction = Point2::new(line.1.u - line.0.u, line.1.v - line.0.v);
            let length = direction.u.hypot(direction.v);
            if length <= PRESENTATION_MIN_LINE_LENGTH {
                return None;
            }
            let offset = Point2::new(circle.0.u - line.0.u, circle.0.v - line.0.v);
            let center_distance = (offset.u * direction.v - offset.v * direction.u).abs() / length;
            vec![
                center_distance + circle.1,
                (center_distance - circle.1).abs(),
            ]
        }
        (
            SketchGeometryDefinition::Circle { center, radius }
            | SketchGeometryDefinition::Arc { center, radius, .. },
            SketchGeometryDefinition::Line { start, end },
        ) => {
            let line = (*start, *end);
            let circle = (*center, radius.get());
            let direction = Point2::new(line.1.u - line.0.u, line.1.v - line.0.v);
            let length = direction.u.hypot(direction.v);
            if length <= PRESENTATION_MIN_LINE_LENGTH {
                return None;
            }
            let offset = Point2::new(circle.0.u - line.0.u, circle.0.v - line.0.v);
            let center_distance = (offset.u * direction.v - offset.v * direction.u).abs() / length;
            vec![
                center_distance + circle.1,
                (center_distance - circle.1).abs(),
            ]
        }
        _ if circle_geometry(first).is_some() && circle_geometry(second).is_some() => {
            let (first_center, first_radius) = circle_geometry(first)?;
            let (second_center, second_radius) = circle_geometry(second)?;
            let center_distance =
                (first_center.u - second_center.u).hypot(first_center.v - second_center.v);
            vec![
                center_distance + first_radius + second_radius,
                (center_distance - first_radius - second_radius).abs(),
                (center_distance + first_radius - second_radius).abs(),
                (center_distance - first_radius + second_radius).abs(),
            ]
        }
        _ => return None,
    };
    let matches = candidates
        .into_iter()
        .filter(|candidate| {
            linear_measurement_matches(
                *candidate,
                parameter.evaluated_value().get() * 10.0,
                linear_tolerance,
            )
        })
        .count();
    if matches != 1 {
        return None;
    }
    Some(Definition::Distance {
        entities: vec![first.id().clone(), second.id().clone()],
        parameter: parameter_id.clone(),
    })
}

fn explicit_linear_dimension_definition(
    first: &cadmpeg_ir::sketches::SketchEntity,
    second: &cadmpeg_ir::sketches::SketchEntity,
    parameter: &DesignParameter,
    parameter_id: &cadmpeg_ir::features::ParameterId,
    linear_tolerance: f64,
) -> Option<cadmpeg_ir::sketches::SketchConstraintDefinitionInput> {
    use cadmpeg_ir::sketches::{
        SketchConstraintDefinitionInput as Definition, SketchGeometryDefinition, SketchLocus,
    };

    let expected = parameter.evaluated_value().get() * 10.0;
    if let Some(definition) = directional_point_dimension(
        &[first, second],
        expected,
        parameter_id.clone(),
        linear_tolerance,
    ) {
        return Some(definition);
    }
    if point_line_separation(first, second, expected, linear_tolerance)
        || parallel_line_separation(first, second, expected, linear_tolerance)
        || concentric_circle_separation(first, second, expected, linear_tolerance)
    {
        return Some(Definition::Distance {
            entities: vec![first.id().clone(), second.id().clone()],
            parameter: parameter_id.clone(),
        });
    }
    let (
        SketchGeometryDefinition::Point {
            position: first_position,
        },
        SketchGeometryDefinition::Point {
            position: second_position,
        },
    ) = (first.geometry.definition(), second.geometry.definition())
    else {
        return None;
    };
    let measured =
        (first_position.u - second_position.u).hypot(first_position.v - second_position.v);
    linear_measurement_matches(measured, expected, linear_tolerance).then(|| {
        Definition::DistanceLoci {
            first: SketchLocus::Entity(first.id().clone()),
            second: SketchLocus::Entity(second.id().clone()),
            parameter: parameter_id.clone(),
        }
    })
}

/// Resolve an angular parameter from the unique two-line point incidence
/// serialized before the parameter in its owning sketch.
fn preceding_incident_angular_dimension_definition(
    scope: &str,
    points: &[SketchPoint],
    curves: &[SketchCurveIdentity],
    projected: &HashMap<(&str, u32), &cadmpeg_ir::sketches::SketchEntity>,
    sketch: &cadmpeg_ir::sketches::SketchId,
    parameter: (&DesignParameter, &cadmpeg_ir::features::ParameterId),
) -> Option<cadmpeg_ir::sketches::SketchConstraintDefinitionInput> {
    use cadmpeg_ir::sketches::{
        SketchConstraintDefinitionInput as Definition, SketchGeometryDefinition,
    };

    let (parameter, parameter_id) = parameter;

    if !parameter.source_kind_str().starts_with("Angular Dimension")
        || !design_dimension_unit(parameter)
    {
        return None;
    }
    let preceding_curve = |record| {
        curves.iter().any(|curve| {
            native_stream(&curve.id) == Some(scope)
                && curve.byte_offset < parameter.byte_offset()
                && curve.record_index == record
        })
    };
    let mut matched = None::<(
        u32,
        u32,
        &cadmpeg_ir::sketches::SketchEntity,
        &cadmpeg_ir::sketches::SketchEntity,
    )>;
    for point in points.iter().filter(|point| {
        native_stream(&point.id) == Some(scope) && point.byte_offset < parameter.byte_offset()
    }) {
        let companion = point.companion();
        let [first_record, second_record] = companion.incident_curves else {
            continue;
        };
        if !preceding_curve(*first_record) || !preceding_curve(*second_record) {
            continue;
        }
        let (Some(first), Some(second)) = (
            projected.get(&(scope, *first_record)).copied(),
            projected.get(&(scope, *second_record)).copied(),
        ) else {
            continue;
        };
        if &first.sketch != sketch
            || &second.sketch != sketch
            || !matches!(
                *first.geometry.definition(),
                SketchGeometryDefinition::Line { .. }
            )
            || !matches!(
                *second.geometry.definition(),
                SketchGeometryDefinition::Line { .. }
            )
            || !line_angle_matches(
                &first.geometry,
                &second.geometry,
                parameter.evaluated_value().get(),
            )
        {
            continue;
        }
        let key = if first_record < second_record {
            (*first_record, *second_record)
        } else {
            (*second_record, *first_record)
        };
        if matched
            .as_ref()
            .is_some_and(|(left, right, _, _)| (*left, *right) != key)
        {
            return None;
        }
        matched = Some((key.0, key.1, first, second));
    }
    let (_, _, first, second) = matched?;
    Some(Definition::Angle {
        first: first.id().clone(),
        second: second.id().clone(),
        parameter: parameter_id.clone(),
    })
}

/// Resolve an angular parameter when exactly one unordered line pair in its
/// owning sketch has the evaluated supporting-line angle.
fn owner_scoped_angular_dimension_definition(
    entities: &[cadmpeg_ir::sketches::SketchEntity],
    sketch: &cadmpeg_ir::sketches::SketchId,
    parameter: &DesignParameter,
    parameter_id: &cadmpeg_ir::features::ParameterId,
) -> Option<cadmpeg_ir::sketches::SketchConstraintDefinitionInput> {
    use cadmpeg_ir::sketches::{
        SketchConstraintDefinitionInput as Definition, SketchGeometryDefinition,
    };

    if !parameter.source_kind_str().starts_with("Angular Dimension")
        || !design_dimension_unit(parameter)
    {
        return None;
    }
    let mut matched = None;
    for (first_index, first) in entities.iter().enumerate() {
        if &first.sketch != sketch
            || !matches!(
                *first.geometry.definition(),
                SketchGeometryDefinition::Line { .. }
            )
        {
            continue;
        }
        for second in entities.iter().skip(first_index + 1) {
            if &second.sketch != sketch
                || !matches!(
                    *second.geometry.definition(),
                    SketchGeometryDefinition::Line { .. }
                )
            {
                continue;
            }
            if !line_angle_matches(
                &first.geometry,
                &second.geometry,
                parameter.evaluated_value().get(),
            ) {
                continue;
            }
            if matched.is_some() {
                return None;
            }
            matched = Some((first, second));
        }
    }
    let (first, second) = matched?;
    Some(Definition::Angle {
        first: first.id().clone(),
        second: second.id().clone(),
        parameter: parameter_id.clone(),
    })
}

/// Bind an angular parameter to the common direction of one exact parallel
/// relation carried by the same dimension companion.
fn parallel_group_axis_angle_definition(
    entities: &[&cadmpeg_ir::sketches::SketchEntity],
    parameter: &DesignParameter,
    parameter_id: &cadmpeg_ir::features::ParameterId,
) -> Option<cadmpeg_ir::sketches::SketchConstraintDefinitionInput> {
    use cadmpeg_ir::sketches::{
        SketchAxis, SketchConstraintDefinitionInput as Definition, SketchGeometry,
        SketchGeometryDefinition,
    };

    let [first, second] = entities else {
        return None;
    };
    if parameter.source_kind() != "Angular Dimension-2"
        || !design_dimension_unit(parameter)
        || parallel_line_distance(first, second).is_none()
    {
        return None;
    }
    let horizontal_axis = SketchGeometry::try_from(SketchGeometryDefinition::Line {
        start: Point2::new(0.0, 0.0),
        end: Point2::new(1.0, 0.0),
    })
    .ok()?;
    (line_angle_matches(
        &first.geometry,
        &horizontal_axis,
        parameter.evaluated_value().get(),
    ) && line_angle_matches(
        &second.geometry,
        &horizontal_axis,
        parameter.evaluated_value().get(),
    ))
    .then(|| Definition::AngleToAxis {
        entity: first.id().clone(),
        axis: SketchAxis::Horizontal,
        parameter: parameter_id.clone(),
    })
}

/// Resolve one or more disjoint owner-scoped concentric-circle separations
/// controlled by one linear parameter.
fn concentric_circle_dimension_definition(
    entities: &[cadmpeg_ir::sketches::SketchEntity],
    sketch: &cadmpeg_ir::sketches::SketchId,
    parameter: &DesignParameter,
    parameter_id: &cadmpeg_ir::features::ParameterId,
    linear_tolerance: f64,
) -> Option<cadmpeg_ir::sketches::SketchConstraintDefinitionInput> {
    use cadmpeg_ir::sketches::{
        SketchConstraintDefinitionInput as Definition, SketchDistanceMeasurement as Measurement,
        SketchGeometryDefinition, SketchLocus,
    };

    if !parameter.source_kind_str().starts_with("Linear Dimension")
        || !design_dimension_unit(parameter)
    {
        return None;
    }
    let evaluated_mm = parameter.evaluated_value().get() * 10.0;
    if !evaluated_mm.is_finite() {
        return None;
    }
    let circles = entities
        .iter()
        .filter(|entity| {
            &entity.sketch == sketch
                && matches!(
                    *entity.geometry.definition(),
                    SketchGeometryDefinition::Circle { .. }
                )
        })
        .collect::<Vec<_>>();
    let mut pairs = Vec::new();
    let mut paired_entities = HashSet::new();
    for first in 0..circles.len() {
        for second in first + 1..circles.len() {
            if !concentric_circle_separation(
                circles[first],
                circles[second],
                evaluated_mm,
                linear_tolerance,
            ) {
                continue;
            }
            let first_id = circles[first].id();
            let second_id = circles[second].id();
            if paired_entities.contains(first_id) || paired_entities.contains(second_id) {
                return None;
            }
            paired_entities.insert(first_id);
            paired_entities.insert(second_id);
            pairs.push((circles[first], circles[second]));
        }
    }
    match pairs.as_slice() {
        [] => None,
        [(first, second)] => Some(Definition::Distance {
            entities: vec![first.id().clone(), second.id().clone()],
            parameter: parameter_id.clone(),
        }),
        _ => {
            let mut measurements = Vec::new();
            for (first, second) in pairs {
                let measurement = Measurement::Distance {
                    first: SketchLocus::Entity(first.id().clone()),
                    second: SketchLocus::Entity(second.id().clone()),
                };
                measurements.push(measurement);
            }
            Some(Definition::RepeatedDistance {
                measurements,
                parameter: parameter_id.clone(),
            })
        }
    }
}

/// Resolve an owner-scoped linear dimension when exactly one point-line pair
/// has the evaluated perpendicular separation.
fn unique_point_line_dimension_definition(
    entities: &[cadmpeg_ir::sketches::SketchEntity],
    sketch: &cadmpeg_ir::sketches::SketchId,
    parameter: &DesignParameter,
    parameter_id: &cadmpeg_ir::features::ParameterId,
    linear_tolerance: f64,
) -> Option<cadmpeg_ir::sketches::SketchConstraintDefinitionInput> {
    use cadmpeg_ir::sketches::{
        SketchConstraintDefinitionInput as Definition, SketchGeometryDefinition,
    };

    if !parameter.source_kind_str().starts_with("Linear Dimension")
        || !design_dimension_unit(parameter)
    {
        return None;
    }
    let evaluated_mm = parameter.evaluated_value().get() * 10.0;
    if !evaluated_mm.is_finite() {
        return None;
    }
    let points = entities.iter().filter(|entity| {
        &entity.sketch == sketch
            && matches!(
                *entity.geometry.definition(),
                SketchGeometryDefinition::Point { .. }
            )
    });
    let lines = || {
        entities.iter().filter(|entity| {
            &entity.sketch == sketch
                && matches!(
                    *entity.geometry.definition(),
                    SketchGeometryDefinition::Line { .. }
                )
        })
    };
    let mut matched = None;
    for point in points {
        for line in lines() {
            if point_line_separation(point, line, evaluated_mm, linear_tolerance) {
                if matched.is_some() {
                    return None;
                }
                matched = Some((point, line));
            }
        }
    }
    let (point, line) = matched?;
    Some(Definition::Distance {
        entities: vec![point.id().clone(), line.id().clone()],
        parameter: parameter_id.clone(),
    })
}

/// Resolve an owner-scoped linear dimension when exactly one parallel-line
/// pair has the evaluated supporting-line separation.
fn unique_parallel_line_dimension_definition(
    entities: &[cadmpeg_ir::sketches::SketchEntity],
    sketch: &cadmpeg_ir::sketches::SketchId,
    parameter: &DesignParameter,
    parameter_id: &cadmpeg_ir::features::ParameterId,
    linear_tolerance: f64,
) -> Option<cadmpeg_ir::sketches::SketchConstraintDefinitionInput> {
    use cadmpeg_ir::sketches::{
        SketchConstraintDefinitionInput as Definition, SketchGeometryDefinition,
    };

    if !parameter.source_kind_str().starts_with("Linear Dimension")
        || !design_dimension_unit(parameter)
    {
        return None;
    }
    let evaluated_mm = parameter.evaluated_value().get() * 10.0;
    if !evaluated_mm.is_finite() {
        return None;
    }
    let lines = || {
        entities.iter().filter(|entity| {
            &entity.sketch == sketch
                && matches!(
                    *entity.geometry.definition(),
                    SketchGeometryDefinition::Line { .. }
                )
        })
    };
    let mut matched = None;
    for (first_ordinal, first) in lines().enumerate() {
        for second in lines().skip(first_ordinal + 1) {
            if parallel_line_separation(first, second, evaluated_mm, linear_tolerance) {
                if matched.is_some() {
                    return None;
                }
                matched = Some((first, second));
            }
        }
    }
    let (first, second) = matched?;
    Some(Definition::Distance {
        entities: vec![first.id().clone(), second.id().clone()],
        parameter: parameter_id.clone(),
    })
}

/// Resolve an owner-scoped distance between fragmented parallel line carriers.
fn owner_scoped_parallel_line_set_dimension_definition(
    entities: &[cadmpeg_ir::sketches::SketchEntity],
    sketch: &cadmpeg_ir::sketches::SketchId,
    parameter: &DesignParameter,
    parameter_id: &cadmpeg_ir::features::ParameterId,
    linear_tolerance: f64,
) -> Option<cadmpeg_ir::sketches::SketchConstraintDefinitionInput> {
    use cadmpeg_ir::sketches::{
        SketchConstraintDefinitionInput as Definition, SketchGeometryDefinition,
    };

    if !parameter.source_kind_str().starts_with("Linear Dimension")
        || !design_dimension_unit(parameter)
        || !linear_tolerance.is_finite()
        || linear_tolerance < 0.0
    {
        return None;
    }
    let evaluated_mm = parameter.evaluated_value().get() * 10.0;
    if !evaluated_mm.is_finite() {
        return None;
    }
    let lines = entities
        .iter()
        .filter(|entity| {
            &entity.sketch == sketch
                && matches!(
                    *entity.geometry.definition(),
                    SketchGeometryDefinition::Line { .. }
                )
        })
        .collect::<Vec<_>>();
    let collinear = |first: &cadmpeg_ir::sketches::SketchEntity,
                     second: &cadmpeg_ir::sketches::SketchEntity| {
        parallel_line_distance(first, second).is_some_and(|distance| distance <= linear_tolerance)
    };
    let mut carriers = Vec::<Vec<&cadmpeg_ir::sketches::SketchEntity>>::new();
    for line in lines {
        let matches = carriers
            .iter()
            .enumerate()
            .filter_map(|(index, carrier)| {
                carrier
                    .iter()
                    .all(|member| collinear(member, line))
                    .then_some(index)
            })
            .collect::<Vec<_>>();
        match matches.as_slice() {
            [] => {
                let carrier = vec![line];
                carriers.push(carrier);
            }
            [index] => carriers[*index].push(line),
            _ => return None,
        }
    }
    let mut matched = None;
    for first in 0..carriers.len() {
        for second in first + 1..carriers.len() {
            if carriers[first].len() == 1 && carriers[second].len() == 1 {
                continue;
            }
            let measured = carriers[first].iter().find_map(|first| {
                carriers[second]
                    .iter()
                    .find_map(|second| parallel_line_span_distance(first, second, linear_tolerance))
            });
            let Some(measured) = measured else {
                continue;
            };
            let expected = evaluated_mm.abs();
            let tolerance = linear_tolerance.max(
                EPS_DIMENSIONS_OWNER_SCOPED_PARALLEL_LINE_SET_DIMENSION_DEFINITION_E9
                    * (1.0 + measured.abs().max(expected.abs())),
            );
            if (measured - expected).abs() > tolerance {
                continue;
            }
            if matched.is_some() {
                return None;
            }
            matched = Some((first, second));
        }
    }
    let (first, second) = matched?;
    Some(Definition::ParallelLineSetDistance {
        first: carriers[first]
            .iter()
            .map(|entity| entity.id().clone())
            .collect(),
        second: carriers[second]
            .iter()
            .map(|entity| entity.id().clone())
            .collect(),
        parameter: parameter_id.clone(),
    })
}

/// Resolve the owner-scoped line lengths governed by one linear parameter.
fn owner_scoped_line_length_dimension_definition(
    entities: &[cadmpeg_ir::sketches::SketchEntity],
    sketch: &cadmpeg_ir::sketches::SketchId,
    parameter: &DesignParameter,
    parameter_id: &cadmpeg_ir::features::ParameterId,
    linear_tolerance: f64,
) -> Option<cadmpeg_ir::sketches::SketchConstraintDefinitionInput> {
    use cadmpeg_ir::sketches::{
        SketchConstraintDefinitionInput as Definition, SketchGeometryDefinition, SketchLocus,
    };

    if !parameter.source_kind_str().starts_with("Linear Dimension")
        || !design_dimension_unit(parameter)
        || !linear_tolerance.is_finite()
        || linear_tolerance < 0.0
    {
        return None;
    }
    let expected = parameter.evaluated_value().get() * 10.0;
    if !expected.is_finite() {
        return None;
    }
    let matches = entities
        .iter()
        .filter(|entity| {
            if &entity.sketch != sketch {
                return false;
            }
            let SketchGeometryDefinition::Line { start, end } = entity.geometry.definition() else {
                return false;
            };
            let measured = (end.u - start.u).hypot(end.v - start.v);
            let tolerance = linear_tolerance.max(
                EPS_DIMENSIONS_OWNER_SCOPED_LINE_LENGTH_DIMENSION_DEFINITION_E9
                    * (1.0 + measured.abs().max(expected.abs())),
            );
            (measured - expected.abs()).abs() <= tolerance
        })
        .collect::<Vec<_>>();
    match matches.as_slice() {
        [] => None,
        [entity] => Some(Definition::DistanceLoci {
            first: SketchLocus::Start(entity.id().clone()),
            second: SketchLocus::End(entity.id().clone()),
            parameter: parameter_id.clone(),
        }),
        _ => Some(Definition::RepeatedLength {
            entities: matches.iter().map(|entity| entity.id().clone()).collect(),
            parameter: parameter_id.clone(),
        }),
    }
}

/// Resolve the unique owner-scoped distance between solved point loci.
fn unique_point_class_dimension_definition(
    entities: &[cadmpeg_ir::sketches::SketchEntity],
    sketch: &cadmpeg_ir::sketches::SketchId,
    parameter: &DesignParameter,
    parameter_id: &cadmpeg_ir::features::ParameterId,
    linear_tolerance: f64,
) -> Option<cadmpeg_ir::sketches::SketchConstraintDefinitionInput> {
    use cadmpeg_ir::sketches::{
        SketchConstraintDefinitionInput as Definition, SketchGeometryDefinition, SketchLocus,
    };

    if !parameter.source_kind_str().starts_with("Linear Dimension")
        || !design_dimension_unit(parameter)
        || !linear_tolerance.is_finite()
        || linear_tolerance < 0.0
    {
        return None;
    }
    let expected = (parameter.evaluated_value().get() * 10.0).abs();
    if !expected.is_finite() {
        return None;
    }
    let points = entities
        .iter()
        .filter(|entity| &entity.sketch == sketch)
        .filter_map(|entity| match *entity.geometry.definition() {
            SketchGeometryDefinition::Point { position } => Some((entity, position.get())),
            _ => None,
        })
        .collect::<Vec<_>>();
    let coincident = |first: Point2, second: Point2| {
        let scale = 1.0
            + first
                .u
                .abs()
                .max(first.v.abs())
                .max(second.u.abs().max(second.v.abs()));
        (second.u - first.u).hypot(second.v - first.v)
            <= linear_tolerance
                .max(EPS_DIMENSIONS_UNIQUE_POINT_CLASS_DIMENSION_DEFINITION_E9 * scale)
    };
    let mut classes = Vec::<Vec<(&cadmpeg_ir::sketches::SketchEntity, Point2)>>::new();
    for point in points {
        let matches = classes
            .iter()
            .enumerate()
            .filter_map(|(index, class)| {
                class
                    .iter()
                    .any(|member| coincident(member.1, point.1))
                    .then_some(index)
            })
            .collect::<Vec<_>>();
        let Some((&first, rest)) = matches.split_first() else {
            let class = vec![point];
            classes.push(class);
            continue;
        };
        classes[first].push(point);
        for &index in rest.iter().rev() {
            let merged = classes.remove(index);
            for member in merged {
                classes[first].push(member);
            }
        }
    }
    let mut matched = None;
    for first in 0..classes.len() {
        for second in first + 1..classes.len() {
            let first_position = classes[first][0].1;
            let second_position = classes[second][0].1;
            let du = second_position.u - first_position.u;
            let dv = second_position.v - first_position.v;
            let measured = du.hypot(dv);
            let tolerance = linear_tolerance.max(
                EPS_DIMENSIONS_UNIQUE_POINT_CLASS_DIMENSION_DEFINITION_E9
                    * (1.0 + measured.abs().max(expected.abs())),
            );
            if (measured - expected).abs() > tolerance {
                continue;
            }
            if matched.is_some() {
                return None;
            }
            matched = Some((classes[first][0].0, classes[second][0].0, du, dv, tolerance));
        }
    }
    let (first, second, du, dv, tolerance) = matched?;
    let first = SketchLocus::Entity(first.id().clone());
    let second = SketchLocus::Entity(second.id().clone());
    Some(if du.abs() <= tolerance {
        Definition::VerticalDistance {
            first,
            second,
            parameter: parameter_id.clone(),
        }
    } else if dv.abs() <= tolerance {
        Definition::HorizontalDistance {
            first,
            second,
            parameter: parameter_id.clone(),
        }
    } else {
        Definition::DistanceLoci {
            first,
            second,
            parameter: parameter_id.clone(),
        }
    })
}

/// Resolve the owner-scoped circular measurements governed by one radial
/// parameter.
fn owner_scoped_radial_dimension_definition(
    entities: &[cadmpeg_ir::sketches::SketchEntity],
    sketch: &cadmpeg_ir::sketches::SketchId,
    parameter: &DesignParameter,
    parameter_id: &cadmpeg_ir::features::ParameterId,
    linear_tolerance: f64,
) -> Option<cadmpeg_ir::sketches::SketchConstraintDefinitionInput> {
    use cadmpeg_ir::sketches::SketchConstraintDefinitionInput as Definition;

    if !design_dimension_unit(parameter) {
        return None;
    }

    let mut definitions = Vec::new();
    for entity in entities.iter().filter(|entity| &entity.sketch == sketch) {
        if let Some(result) = radial_dimension_definition_at_tolerance(
            entity,
            parameter.source_kind_str(),
            parameter.evaluated_value().get(),
            parameter_id.clone(),
            linear_tolerance,
        ) {
            definitions.push(result);
        }
    }
    if definitions.len() < 2 {
        return definitions.pop();
    }
    let radius = definitions
        .iter()
        .all(|definition| matches!(definition, Definition::Radius { .. }));
    let diameter = definitions
        .iter()
        .all(|definition| matches!(definition, Definition::Diameter { .. }));
    if !radius && !diameter {
        return None;
    }
    let mut members = Vec::new();
    for definition in definitions {
        let (Definition::Radius { entity, .. } | Definition::Diameter { entity, .. }) = definition
        else {
            return None;
        };
        members.push(entity);
    }
    let parameter = parameter_id.clone();
    Some(if radius {
        Definition::RepeatedRadius {
            entities: members,
            parameter,
        }
    } else {
        Definition::RepeatedDiameter {
            entities: members,
            parameter,
        }
    })
}

pub(crate) fn constraint_parameters(
    definition: &cadmpeg_ir::sketches::SketchConstraintDefinitionInput,
) -> impl Iterator<Item = &cadmpeg_ir::features::ParameterId> {
    use cadmpeg_ir::sketches::SketchConstraintDefinitionInput as Definition;

    let parameters = match definition {
        Definition::Offset { parameter, .. } => [
            parameter.as_ref().map(|parameter| &parameter.id),
            None,
            None,
            None,
        ],
        Definition::Native { parameter, .. } => [parameter.as_ref(), None, None, None],
        Definition::PolarDistance {
            distance_parameter, ..
        } => [distance_parameter.as_ref(), None, None, None],
        Definition::DistanceLociValue { parameter, .. } => [parameter.as_ref(), None, None, None],
        Definition::Distance { parameter, .. }
        | Definition::DistanceLoci { parameter, .. }
        | Definition::HorizontalDistance { parameter, .. }
        | Definition::VerticalDistance { parameter, .. }
        | Definition::RepeatedDistance { parameter, .. }
        | Definition::RepeatedLength { parameter, .. }
        | Definition::ParallelLineSetDistance { parameter, .. }
        | Definition::Angle { parameter, .. }
        | Definition::AngleToAxis { parameter, .. }
        | Definition::Radius { parameter, .. }
        | Definition::RepeatedRadius { parameter, .. }
        | Definition::Diameter { parameter, .. }
        | Definition::RepeatedDiameter { parameter, .. }
        | Definition::SnellsLaw { parameter, .. }
        | Definition::Weight { parameter, .. } => [Some(parameter), None, None, None],
        Definition::RectangularPattern { pattern } => {
            let [first, second] = pattern.directions();
            [
                first
                    .distance
                    .as_ref()
                    .map(cadmpeg_ir::sketches::SketchPatternDistance::parameter),
                first.count_parameter.as_ref(),
                second
                    .distance
                    .as_ref()
                    .map(cadmpeg_ir::sketches::SketchPatternDistance::parameter),
                second.count_parameter.as_ref(),
            ]
        }
        Definition::CircularPattern { pattern } => [
            pattern.angle_parameter(),
            pattern.count_parameter(),
            None,
            None,
        ],
        Definition::Disabled {}
        | Definition::Coincident { .. }
        | Definition::ProjectedCopy { .. }
        | Definition::Polygon { .. }
        | Definition::SplineGroup { .. }
        | Definition::TextFrame { .. }
        | Definition::TextPath { .. }
        | Definition::CoincidentLoci { .. }
        | Definition::SameCoordinate { .. }
        | Definition::PointSymmetric { .. }
        | Definition::TangentLoci { .. }
        | Definition::AtIntersection { .. }
        | Definition::Midpoint { .. }
        | Definition::PointCoordinateValues { .. }
        | Definition::MidpointCoordinate { .. }
        | Definition::AngleDifference { .. }
        | Definition::ScalarEquality { .. }
        | Definition::Concentric { .. }
        | Definition::Coradial { .. }
        | Definition::Collinear { .. }
        | Definition::Symmetric { .. }
        | Definition::Horizontal { .. }
        | Definition::Vertical { .. }
        | Definition::Parallel { .. }
        | Definition::Perpendicular { .. }
        | Definition::Tangent { .. }
        | Definition::Curvature { .. }
        | Definition::Equal { .. }
        | Definition::EqualDistance { .. }
        | Definition::ArcAngle { .. }
        | Definition::EllipseAngle { .. }
        | Definition::Fixed { .. }
        | Definition::PointOnObject { .. }
        | Definition::InternalAlignment { .. }
        | Definition::Group { .. }
        | Definition::Text { .. } => [None; 4],
    };
    parameters.into_iter().flatten()
}

/// Attach single-locus offset dimensions to uniquely matching typed offset
/// relations and remove their redundant native annotation constraints.
pub(crate) fn bind_offset_dimension_parameters(
    constraints: &mut Vec<cadmpeg_ir::sketches::SketchConstraint>,
    parameters: &[DesignParameter],
) {
    use cadmpeg_ir::sketches::SketchConstraintDefinitionInput as Definition;

    let mut parameter_values = HashMap::new();
    for parameter in parameters {
        if let Some(value) = design_length(parameter) {
            parameter_values.insert(crate::ids::neutral_parameter_id(parameter), value.get());
        }
    }
    let mut bindings = Vec::new();
    for (dimension_index, dimension) in constraints.iter().enumerate() {
        let Definition::Native {
            native_kind,
            entities,
            parameter: Some(parameter),
            operands,
            ..
        } = dimension.definition.kind()
        else {
            continue;
        };
        let [entity] = entities.as_slice() else {
            continue;
        };
        if !native_kind.as_str().starts_with("Linear Dimension")
            || operands.len() != 2
            || operands[0].native_kind != "null_locus"
            || operands[1].native_kind != "curve"
        {
            continue;
        }
        let Some(parameter_value) = parameter_values.get(parameter).copied() else {
            continue;
        };
        let mut candidate = None;
        let mut ambiguous = false;
        for (offset_index, constraint) in constraints.iter().enumerate() {
            if constraint.sketch != dimension.sketch {
                continue;
            }
            let Definition::Offset {
                pairs,
                distance,
                parameter: None,
            } = constraint.definition.kind()
            else {
                continue;
            };
            if pairs.iter().any(|pair| &pair.source == entity)
                && scalar_close(distance.get(), parameter_value.abs())
                && candidate.replace(offset_index).is_some()
            {
                ambiguous = true;
                break;
            }
        }
        if let Some(offset_index) = candidate.filter(|_| !ambiguous) {
            let parameter = parameter.clone();
            bindings.push((dimension_index, offset_index, parameter, parameter_value));
        }
    }
    let mut offset_counts = HashMap::new();
    for binding in &bindings {
        if let Some(count) = offset_counts.get_mut(&binding.1) {
            *count += 1;
        } else {
            offset_counts.insert(binding.1, 1usize);
        }
    }
    bindings.retain(|binding| offset_counts.get(&binding.1) == Some(&1));
    let mut removed = HashSet::new();
    for (dimension_index, offset_index, parameter, parameter_value) in bindings {
        let applied = constraints[offset_index].definition.set_offset_parameter(
            cadmpeg_ir::sketches::OffsetParameter {
                id: parameter,
                negated: parameter_value.is_sign_negative(),
            },
        );
        if applied {
            removed.insert(dimension_index);
        }
    }
    let mut index = 0usize;
    constraints.retain(|_| {
        let keep = !removed.contains(&index);
        index += 1;
        keep
    });
}

/// Project dimensions owned by model-space sketches without assigning them
/// planar relation semantics.
pub(crate) fn project_spatial_dimension_constraints(
    inputs: &DimensionConstraintInputs<'_>,
    spatial_sketches: &[cadmpeg_ir::sketches::SpatialSketch],
    spatial_entities: &[cadmpeg_ir::sketches::SpatialSketchEntity],
    linear_tolerance: f64,
) -> Vec<cadmpeg_ir::sketches::SpatialSketchConstraint> {
    use cadmpeg_ir::sketches::{
        SketchConstraintDefinitionInput, SketchNativeOperand, SpatialSketchConstraint,
        SpatialSketchConstraintDefinitionInput,
    };

    let &DimensionConstraintInputs {
        placements,
        parameters,
        owners,
        companions,
        points,
        curves,
        ..
    } = inputs;

    let mut spatial_by_planar_id = HashMap::new();
    for placement in placements {
        let spatial_id = crate::ids::neutral_spatial_sketch_id(placement);
        if spatial_sketches
            .iter()
            .any(|sketch| sketch.id == spatial_id)
        {
            spatial_by_planar_id.insert(crate::ids::neutral_sketch_id(placement), spatial_id);
        }
    }
    let mut spatial_by_scope = HashMap::new();
    for placement in placements {
        let (Some(scope), Some(scope_record_index)) =
            (native_stream(&placement.id), placement.scope_record_index)
        else {
            continue;
        };
        if let Some(sketch) = spatial_by_planar_id.get(&crate::ids::neutral_sketch_id(placement)) {
            let sketch = sketch.clone();
            spatial_by_scope.insert((scope, scope_record_index), sketch);
        }
    }
    let native_records = points
        .iter()
        .filter_map(|point| {
            Some((
                point.id.as_str(),
                (native_stream(&point.id)?, point.record_index),
            ))
        })
        .chain(curves.iter().filter_map(|curve| {
            Some((
                curve.id.as_str(),
                (native_stream(&curve.id)?, curve.record_index),
            ))
        }));
    let mut native_record_indices = HashMap::new();
    for (native_ref, record) in native_records {
        native_record_indices.insert(native_ref, record);
    }
    let mut spatial_by_record = HashMap::new();
    for entity in spatial_entities {
        let Some(native_ref) = entity.native_ref.as_deref() else {
            continue;
        };
        if let Some(key) = native_record_indices.get(native_ref) {
            spatial_by_record.insert(*key, entity);
        }
    }
    let mut parameter_lengths = HashMap::new();
    let mut parameters_by_id = HashMap::new();
    for parameter in parameters {
        if let Some(length) = design_length(parameter) {
            parameter_lengths.insert(
                crate::ids::neutral_parameter_id(parameter),
                length.get().abs(),
            );
        }
        parameters_by_id.insert(crate::ids::neutral_parameter_id(parameter), parameter);
    }
    let source_constraints = project_all_dimension_constraints(inputs, &[], linear_tolerance);
    let mut parameter_constraint_counts = HashMap::new();
    for parameter in source_constraints
        .iter()
        .flat_map(|constraint| constraint_parameters(constraint.definition.kind()))
    {
        if let Some(count) = parameter_constraint_counts.get_mut(parameter) {
            *count += 1;
        } else {
            let id = parameter.clone();
            parameter_constraint_counts.insert(id, 1usize);
        }
    }
    let mut source_parameters = HashSet::new();
    let mut projected = Vec::new();
    for constraint in source_constraints {
        let projected_constraint = (|| -> Option<SpatialSketchConstraint> {
            let sketch = spatial_by_planar_id.get(&constraint.sketch)?;
            let sketch = sketch.clone();
            for parameter in constraint_parameters(constraint.definition.kind()) {
                let id = parameter.clone();
                source_parameters.insert(id);
            }
            let definition = match constraint.definition.into_kind() {
                SketchConstraintDefinitionInput::Native {
                    native_kind,
                    native_state,
                    parameter,
                    operands,
                    ..
                } => {
                    let symmetry = spatial_reflection_symmetry(
                        native_kind.as_str(),
                        native_state,
                        &operands,
                        constraint.native_ref.as_deref(),
                        &sketch,
                        &spatial_by_record,
                    );
                    let offset = parameter.as_ref().and_then(|parameter_id| {
                        let expected = *parameter_lengths.get(parameter_id)?;
                        let parameter = parameters_by_id.get(parameter_id)?;
                        let signed = design_length(parameter)?.get();
                        spatial_counted_offset_dimension_definition(
                            (native_kind.as_str(), native_state, &operands),
                            (parameter_id, expected, signed),
                            &sketch,
                            spatial_sketches,
                            &spatial_by_record,
                        )
                    });
                    let distance = parameter.as_ref().and_then(|parameter| {
                        let expected = *parameter_lengths.get(parameter)?;
                        let scope = native_stream(constraint.native_ref.as_deref()?)?;
                        let mut measured = Vec::new();
                        for operand in operands.iter().filter(|operand| {
                            operand_field(operand)
                                .is_some_and(|field| field == "locus" || field.ends_with("_locus"))
                                && operand.object_index.is_some()
                        }) {
                            let entity = spatial_by_record
                                .get(&(scope, operand.object_index?))
                                .copied()?;
                            measured.push(entity);
                        }
                        let [first, second] = measured.as_slice() else {
                            return None;
                        };
                        if !native_kind.as_str().starts_with("Linear Dimension")
                            || first.sketch != sketch
                            || second.sketch != sketch
                            || first.id() == second.id()
                        {
                            return None;
                        }
                        if spatial_point_distance_matches(
                            &first.geometry,
                            &second.geometry,
                            expected,
                        ) {
                            Some(SpatialSketchConstraintDefinitionInput::PointDistance {
                                first: first.id().clone(),
                                second: second.id().clone(),
                                parameter: parameter.clone(),
                            })
                        } else if spatial_parallel_line_distance_matches(
                            &first.geometry,
                            &second.geometry,
                            expected,
                        ) {
                            Some(
                                SpatialSketchConstraintDefinitionInput::ParallelLineDistance {
                                    first: first.id().clone(),
                                    second: second.id().clone(),
                                    parameter: parameter.clone(),
                                },
                            )
                        } else {
                            None
                        }
                    });
                    let owner_scoped = (|| -> Option<SpatialSketchConstraintDefinitionInput> {
                        if operands.len() != 1
                            || operands[0].native_kind != "dimension_companion"
                            || !operand_field(&operands[0]).is_some_and(|field| {
                                field == "companion" || field == "companion_payload"
                            })
                        {
                            return None;
                        }
                        let parameter_id = parameter.as_ref()?;
                        if parameter_constraint_counts.get(parameter_id) != Some(&1) {
                            return None;
                        }
                        let parameter = parameters_by_id.get(parameter_id)?;
                        let line_length = owner_scoped_spatial_line_length_dimension_definition(
                            spatial_entities,
                            &sketch,
                            parameter,
                            parameter_id,
                            linear_tolerance,
                        );
                        line_length
                            .or_else(|| {
                                unique_spatial_parallel_line_dimension_definition(
                                    spatial_entities,
                                    &sketch,
                                    parameter,
                                    parameter_id,
                                )
                            })
                            .or_else(|| {
                                owner_scoped_spatial_repeated_profile_line_distance_definition(
                                    spatial_entities,
                                    spatial_sketches,
                                    &sketch,
                                    parameter,
                                    parameter_id,
                                )
                            })
                            .or_else(|| {
                                owner_scoped_spatial_parallel_line_set_dimension_definition(
                                    spatial_entities,
                                    &sketch,
                                    parameter,
                                    parameter_id,
                                    linear_tolerance,
                                )
                            })
                    })();
                    symmetry.or(offset).or(distance).or(owner_scoped).unwrap_or(
                        SpatialSketchConstraintDefinitionInput::Native {
                            native_kind,
                            native_state,
                            parameter,
                            operands,
                        },
                    )
                }
                _ => return None,
            };
            let definition =
                cadmpeg_ir::sketches::SpatialSketchConstraintDefinition::try_from(definition)
                    .ok()?;
            Some(SpatialSketchConstraint {
                id: constraint.id,
                sketch,
                definition,
                native_ref: constraint.native_ref,
            })
        })();
        if let Some(projected_constraint) = projected_constraint {
            projected.push(projected_constraint);
        }
    }

    let retained_parameter_ids =
        projected
            .iter()
            .filter_map(|constraint| match constraint.definition.kind() {
                SpatialSketchConstraintDefinitionInput::Native {
                    parameter: Some(parameter),
                    ..
                }
                | SpatialSketchConstraintDefinitionInput::PointDistance { parameter, .. }
                | SpatialSketchConstraintDefinitionInput::PointLineDistance { parameter, .. }
                | SpatialSketchConstraintDefinitionInput::LineLength { parameter, .. }
                | SpatialSketchConstraintDefinitionInput::RepeatedLineLength {
                    parameter, ..
                }
                | SpatialSketchConstraintDefinitionInput::ParallelLineDistance {
                    parameter, ..
                }
                | SpatialSketchConstraintDefinitionInput::RepeatedParallelLineDistance {
                    parameter,
                    ..
                }
                | SpatialSketchConstraintDefinitionInput::ParallelLineSetDistance {
                    parameter,
                    ..
                } => Some(parameter),
                SpatialSketchConstraintDefinitionInput::Offset {
                    parameter: Some(parameter),
                    ..
                } => Some(&parameter.id),
                _ => None,
            });
    let mut retained_parameters = HashSet::new();
    for parameter in retained_parameter_ids {
        let id = parameter.clone();
        retained_parameters.insert(id);
    }
    let mut owners_by_record = HashMap::new();
    for owner in owners {
        if let Some(scope) = native_stream(owner.id()) {
            owners_by_record.insert((scope, owner.record_index()), owner);
        }
    }
    let mut companions_by_record = HashMap::new();
    for companion in companions {
        if let Some(scope) = native_stream(companion.id()) {
            companions_by_record.insert((scope, companion.record_index()), companion);
        }
    }
    let mut missing = Vec::new();
    for parameter_id in source_parameters.difference(&retained_parameters) {
        missing.push(parameter_id);
    }
    missing.sort_by(|first, second| first.as_str().cmp(second.as_str()));
    for parameter_id in missing {
        let Some(parameter) = parameters_by_id.get(parameter_id) else {
            continue;
        };
        let Some(scope) = native_stream(&parameter.id) else {
            continue;
        };
        let Some(owner_record_index) = parameter.owner_record_index() else {
            continue;
        };
        let Some(owner) = owners_by_record.get(&(scope, owner_record_index)) else {
            continue;
        };
        let Some(companion) = companions_by_record.get(&(scope, owner.companion_record_index()))
        else {
            continue;
        };
        let Some(sketch) = spatial_by_scope.get(&(scope, owner.scope_record_index())) else {
            continue;
        };
        let sketch = (*sketch).clone();
        let native_ref = companion.id().to_owned();
        let constraint_native_ref = companion.id().to_owned();
        let definition = cadmpeg_ir::sketches::SpatialSketchConstraintDefinition::try_from(
            SpatialSketchConstraintDefinitionInput::Native {
                native_kind: parameter.source_kind().clone(),
                native_state: None,
                parameter: Some(parameter_id.clone()),
                operands: vec![SketchNativeOperand {
                    native_kind: cadmpeg_core::nonblank_literal!("dimension_companion"),
                    field: Some(NativeOperandField {
                        name: if companion
                            .payload()
                            .is_none_or(|payload| payload.byte_length() == 0)
                        {
                            cadmpeg_core::nonblank_literal!("companion")
                        } else {
                            cadmpeg_core::nonblank_literal!("companion_payload")
                        },
                        role: None,
                    }),
                    object_index: Some(companion.record_index()),
                    native_ref: Some(native_ref),
                }],
            },
        )
        .ok();
        let Some(definition) = definition else {
            continue;
        };
        projected.push(SpatialSketchConstraint {
            id: crate::ids::neutral_dimension_constraint_id(parameter_id, "companion-payload"),
            sketch,
            definition,
            native_ref: Some(constraint_native_ref),
        });
    }
    projected
}

fn owner_scoped_spatial_line_length_dimension_definition(
    entities: &[cadmpeg_ir::sketches::SpatialSketchEntity],
    sketch: &cadmpeg_ir::sketches::SpatialSketchId,
    parameter: &DesignParameter,
    parameter_id: &cadmpeg_ir::features::ParameterId,
    linear_tolerance: f64,
) -> Option<cadmpeg_ir::sketches::SpatialSketchConstraintDefinitionInput> {
    use cadmpeg_ir::sketches::{
        SpatialSketchConstraintDefinitionInput as Definition, SpatialSketchGeometryDefinition,
    };

    if !parameter.source_kind_str().starts_with("Linear Dimension")
        || !design_dimension_unit(parameter)
        || !linear_tolerance.is_finite()
        || linear_tolerance < 0.0
    {
        return None;
    }
    let expected = (parameter.evaluated_value().get() * 10.0).abs();
    if !expected.is_finite() {
        return None;
    }
    let mut matches = Vec::new();
    for entity in entities
        .iter()
        .filter(|entity| &entity.sketch == sketch)
        .filter(|entity| {
            let SpatialSketchGeometryDefinition::Line { start, end } =
                *entity.geometry.definition()
            else {
                return false;
            };
            let measured = (end.x - start.x).hypot((end.y - start.y).hypot(end.z - start.z));
            if !measured.is_finite() {
                return false;
            }
            let tolerance = linear_tolerance.max(
                EPS_DIMENSIONS_OWNER_SCOPED_SPATIAL_LINE_LENGTH_DIMENSION_DEFINITION_E9
                    * (1.0 + measured.abs().max(expected.abs())),
            );
            (measured - expected).abs() <= tolerance
        })
    {
        matches.push(entity.id().clone());
    }
    match matches.len() {
        0 => None,
        1 => Some(Definition::LineLength {
            entity: matches.remove(0),
            parameter: parameter_id.clone(),
        }),
        _ => Some(Definition::RepeatedLineLength {
            entities: matches,
            parameter: parameter_id.clone(),
        }),
    }
}

fn unique_spatial_parallel_line_dimension_definition(
    entities: &[cadmpeg_ir::sketches::SpatialSketchEntity],
    sketch: &cadmpeg_ir::sketches::SpatialSketchId,
    parameter: &DesignParameter,
    parameter_id: &cadmpeg_ir::features::ParameterId,
) -> Option<cadmpeg_ir::sketches::SpatialSketchConstraintDefinitionInput> {
    use cadmpeg_ir::sketches::{
        SpatialSketchConstraintDefinitionInput as Definition, SpatialSketchGeometryDefinition,
    };

    if !parameter.source_kind_str().starts_with("Linear Dimension")
        || !design_dimension_unit(parameter)
    {
        return None;
    }
    let expected = (parameter.evaluated_value().get() * 10.0).abs();
    if !expected.is_finite() {
        return None;
    }
    let lines = entities
        .iter()
        .filter(|entity| {
            &entity.sketch == sketch
                && matches!(
                    *entity.geometry.definition(),
                    SpatialSketchGeometryDefinition::Line { .. }
                )
        })
        .collect::<Vec<_>>();
    let mut matched = None;
    for first in 0..lines.len() {
        for second in first + 1..lines.len() {
            if spatial_parallel_line_distance_matches(
                &lines[first].geometry,
                &lines[second].geometry,
                expected,
            ) {
                if matched.is_some() {
                    return None;
                }
                matched = Some((lines[first], lines[second]));
            }
        }
    }
    let (first, second) = matched?;
    Some(Definition::ParallelLineDistance {
        first: first.id().clone(),
        second: second.id().clone(),
        parameter: parameter_id.clone(),
    })
}

fn owner_scoped_spatial_repeated_profile_line_distance_definition(
    entities: &[cadmpeg_ir::sketches::SpatialSketchEntity],
    sketches: &[cadmpeg_ir::sketches::SpatialSketch],
    sketch: &cadmpeg_ir::sketches::SpatialSketchId,
    parameter: &DesignParameter,
    parameter_id: &cadmpeg_ir::features::ParameterId,
) -> Option<cadmpeg_ir::sketches::SpatialSketchConstraintDefinitionInput> {
    use cadmpeg_ir::sketches::{
        SpatialSketchConstraintDefinitionInput as Definition, SpatialSketchEntityPair,
    };

    if !parameter.source_kind_str().starts_with("Linear Dimension")
        || !design_dimension_unit(parameter)
    {
        return None;
    }
    let expected = (parameter.evaluated_value().get() * 10.0).abs();
    if !expected.is_finite() {
        return None;
    }
    let mut entities_by_id = HashMap::new();
    for entity in entities.iter().filter(|entity| &entity.sketch == sketch) {
        entities_by_id.insert(entity.id(), entity);
    }
    let sketch = sketches.iter().find(|candidate| &candidate.id == sketch)?;
    let mut seen_pairs = HashSet::new();
    let mut used_entities = HashSet::new();
    let mut pairs = Vec::new();
    for profile in &sketch.profiles {
        if profile.boundary().len() < 4 {
            continue;
        }
        for index in 0..profile.boundary().len() {
            let first_id = &profile.boundary()[index].entity;
            let second_id = &profile.boundary()[(index + 2) % profile.boundary().len()].entity;
            let key = if first_id < second_id {
                (first_id, second_id)
            } else {
                (second_id, first_id)
            };
            if seen_pairs.contains(&key) {
                continue;
            }
            seen_pairs.insert(key);
            let first = entities_by_id.get(first_id)?;
            let second = entities_by_id.get(second_id)?;
            if !spatial_parallel_line_distance_matches(&first.geometry, &second.geometry, expected)
            {
                continue;
            }
            if used_entities.contains(first_id) || used_entities.contains(second_id) {
                return None;
            }
            used_entities.insert(first_id);
            used_entities.insert(second_id);
            pairs.push(SpatialSketchEntityPair {
                first: first_id.clone(),
                second: second_id.clone(),
            });
        }
    }
    (pairs.len() >= 2).then(|| Definition::RepeatedParallelLineDistance {
        pairs,
        parameter: parameter_id.clone(),
    })
}

fn owner_scoped_spatial_parallel_line_set_dimension_definition(
    entities: &[cadmpeg_ir::sketches::SpatialSketchEntity],
    sketch: &cadmpeg_ir::sketches::SpatialSketchId,
    parameter: &DesignParameter,
    parameter_id: &cadmpeg_ir::features::ParameterId,
    linear_tolerance: f64,
) -> Option<cadmpeg_ir::sketches::SpatialSketchConstraintDefinitionInput> {
    use cadmpeg_ir::sketches::{
        SpatialSketchConstraintDefinitionInput as Definition, SpatialSketchGeometryDefinition,
    };

    if !parameter.source_kind_str().starts_with("Linear Dimension")
        || !design_dimension_unit(parameter)
        || !linear_tolerance.is_finite()
        || linear_tolerance < 0.0
    {
        return None;
    }
    let expected = (parameter.evaluated_value().get() * 10.0).abs();
    if !expected.is_finite() {
        return None;
    }
    let lines = entities
        .iter()
        .filter(|entity| {
            &entity.sketch == sketch
                && matches!(
                    *entity.geometry.definition(),
                    SpatialSketchGeometryDefinition::Line { .. }
                )
        })
        .collect::<Vec<_>>();
    let collinear = |first: &cadmpeg_ir::sketches::SpatialSketchEntity,
                     second: &cadmpeg_ir::sketches::SpatialSketchEntity| {
        spatial_parallel_line_distance(&first.geometry, &second.geometry)
            .is_some_and(|distance| distance <= linear_tolerance)
    };
    let mut carriers = Vec::<Vec<&cadmpeg_ir::sketches::SpatialSketchEntity>>::new();
    for line in lines {
        let matches = carriers
            .iter()
            .enumerate()
            .filter_map(|(index, carrier)| {
                carrier
                    .iter()
                    .all(|member| collinear(member, line))
                    .then_some(index)
            })
            .collect::<Vec<_>>();
        match matches.as_slice() {
            [] => {
                let carrier = vec![line];
                carriers.push(carrier);
            }
            [index] => carriers[*index].push(line),
            _ => return None,
        }
    }
    let mut matched = None;
    for first in 0..carriers.len() {
        for second in first + 1..carriers.len() {
            if carriers[first].len() == 1 && carriers[second].len() == 1 {
                continue;
            }
            let measured = carriers[first].iter().find_map(|first| {
                carriers[second].iter().find_map(|second| {
                    spatial_parallel_line_span_distance(
                        &first.geometry,
                        &second.geometry,
                        linear_tolerance,
                    )
                })
            });
            let Some(measured) = measured else {
                continue;
            };
            if !measured.is_finite() {
                continue;
            }
            let tolerance = linear_tolerance.max(
                EPS_DIMENSIONS_OWNER_SCOPED_SPATIAL_PARALLEL_LINE_SET_DIMENSION_DEFINITION_E9
                    * (1.0 + measured.abs().max(expected.abs())),
            );
            if (measured - expected).abs() > tolerance {
                continue;
            }
            if matched.is_some() {
                return None;
            }
            matched = Some((first, second));
        }
    }
    let (first, second) = matched?;
    Some(Definition::ParallelLineSetDistance {
        first: carriers[first]
            .iter()
            .map(|entity| entity.id().clone())
            .collect(),
        second: carriers[second]
            .iter()
            .map(|entity| entity.id().clone())
            .collect(),
        parameter: parameter_id.clone(),
    })
}

fn operand_field(operand: &cadmpeg_ir::sketches::SketchNativeOperand) -> Option<&str> {
    operand.field.as_ref().map(|field| field.name.as_str())
}

fn operand_role(operand: &cadmpeg_ir::sketches::SketchNativeOperand) -> Option<u32> {
    operand.field.as_ref().and_then(|field| field.role)
}

fn spatial_reflection_symmetry(
    native_kind: &str,
    native_state: Option<u64>,
    operands: &[cadmpeg_ir::sketches::SketchNativeOperand],
    native_ref: Option<&str>,
    sketch: &cadmpeg_ir::sketches::SpatialSketchId,
    spatial_by_record: &HashMap<(&str, u32), &cadmpeg_ir::sketches::SpatialSketchEntity>,
) -> Option<cadmpeg_ir::sketches::SpatialSketchConstraintDefinitionInput> {
    use cadmpeg_ir::sketches::{
        SpatialSketchConstraintDefinitionInput as Definition, SpatialSketchGeometryDefinition,
    };

    if !native_kind.starts_with("Linear Dimension") || native_state != Some(0) {
        return None;
    }
    let mut owners = operands
        .iter()
        .filter(|operand| operand_field(operand) == Some("owner"));
    let owner = owners.next()?;
    if owners.next().is_some() {
        return None;
    }
    if !operand_role(owner).is_some_and(|role| {
        crate::records::sketch_relations::constraint_kinds_iter(u64::from(role))
            .any(|kind| kind == SketchConstraintKind::Symmetry)
    }) {
        return None;
    }
    let scope = native_stream(native_ref?)?;
    let mut loci = operands
        .iter()
        .filter(|operand| operand_field(operand) == Some("locus"));
    let mut entity_for = || {
        let operand = loci.next()?;
        spatial_by_record
            .get(&(scope, operand.object_index?))
            .copied()
    };
    let entities = [entity_for()?, entity_for()?, entity_for()?];
    if loci.next().is_some() {
        return None;
    }
    let [first, second, third] = entities;
    if entities.iter().any(|entity| &entity.sketch != sketch)
        || first.id() == second.id()
        || first.id() == third.id()
        || second.id() == third.id()
    {
        return None;
    }
    let mut points = [None; 2];
    let mut point_count = 0;
    let mut axis = None;
    for entity in entities {
        match *entity.geometry.definition() {
            SpatialSketchGeometryDefinition::Point { position } => {
                let slot = points.get_mut(point_count)?;
                *slot = Some((entity, position.get()));
                point_count += 1;
            }
            SpatialSketchGeometryDefinition::Line { start, end } if axis.is_none() => {
                axis = Some((entity, start.get(), end.get()));
            }
            _ => return None,
        }
    }
    let [Some((first, first_position)), Some((second, second_position))] = points else {
        return None;
    };
    let (axis, axis_start, axis_end) = axis?;
    cadmpeg_ir::eval::spatial_points_are_reflections(
        first_position,
        second_position,
        axis_start,
        axis_end,
    )
    .then(|| Definition::Symmetric {
        first: first.id().clone(),
        second: second.id().clone(),
        axis: axis.id().clone(),
    })
}

fn spatial_counted_offset_dimension_definition(
    native: (
        &str,
        Option<u64>,
        &[cadmpeg_ir::sketches::SketchNativeOperand],
    ),
    measurement: (&cadmpeg_ir::features::ParameterId, f64, f64),
    sketch: &cadmpeg_ir::sketches::SpatialSketchId,
    spatial_sketches: &[cadmpeg_ir::sketches::SpatialSketch],
    spatial_by_record: &HashMap<(&str, u32), &cadmpeg_ir::sketches::SpatialSketchEntity>,
) -> Option<cadmpeg_ir::sketches::SpatialSketchConstraintDefinitionInput> {
    use cadmpeg_ir::scalar::Length;

    use cadmpeg_ir::sketches::SpatialSketchConstraintDefinitionInput as Definition;

    let (native_kind, native_state, operands) = native;
    let (parameter, distance, signed_parameter) = measurement;

    if !native_kind.starts_with("Linear Dimension")
        || native_state != Some(0x20)
        || !distance.is_finite()
        || distance <= EPS_DIMENSIONS_SPATIAL_COUNTED_OFFSET_DIMENSION_DEFINITION_E9
    {
        return None;
    }
    let parameter_factor = offset_parameter_factor(distance, signed_parameter)?;
    let scope = operands
        .iter()
        .find_map(|operand| native_stream(operand.native_ref.as_deref()?))?;
    let owner_position = operands
        .iter()
        .take_while(|operand| operand_field(operand) == Some("locus"))
        .count();
    let loci = &operands[..owner_position];
    let owner = operands.get(owner_position)?;
    let returns = operands.get(owner_position + 1..)?;
    if operand_field(owner) != Some("owner")
        || owner.native_kind != "record"
        || operand_role(owner) != Some(0)
        || loci.iter().any(|operand| operand.native_kind != "curve")
        || returns.iter().any(|operand| {
            operand.native_kind != "curve"
                || operand_field(operand) != Some("return")
                || operand_role(operand).is_some()
        })
        || loci.len() < 4
        || !loci.len().is_multiple_of(2)
        || returns.len() != loci.len()
    {
        return None;
    }
    let source_count = loci
        .iter()
        .position(|operand| operand_role(operand) == Some(0))?;
    if source_count == 0
        || source_count * 2 != loci.len()
        || loci[..source_count]
            .iter()
            .any(|operand| operand_role(operand) == Some(0))
        || loci[source_count..]
            .iter()
            .any(|operand| operand_role(operand) != Some(0))
    {
        return None;
    }
    let mut roles = HashMap::new();
    for operand in loci {
        roles.insert(operand.object_index?, operand_role(operand)?);
    }
    if roles.len() != loci.len() {
        return None;
    }
    let mut result_records = HashSet::new();
    for pair in returns.chunks_exact(2) {
        result_records.insert(pair[1].object_index?);
    }
    let mut result_ids = HashSet::new();
    for record in &result_records {
        if let Some(entity) = spatial_by_record.get(&(scope, *record)) {
            result_ids.insert(entity.id());
        }
    }
    if result_ids.len() != result_records.len() {
        return None;
    }
    let spatial = spatial_sketches
        .iter()
        .find(|candidate| &candidate.id == sketch)?;
    let mut matching_profiles = spatial.profiles.iter().filter(|profile| {
        profile
            .boundary()
            .iter()
            .all(|use_| result_ids.contains(&use_.entity))
            && result_ids
                .iter()
                .all(|id| profile.boundary().iter().any(|use_| &use_.entity == *id))
    });
    let normal = matching_profiles.next()?.normal();
    if matching_profiles.next().is_some() {
        return None;
    }
    let mut sources = Vec::new();
    let mut results = Vec::new();
    let mut used = HashSet::new();
    for operands in returns.chunks_exact(2) {
        let source_record = operands[0].object_index?;
        let result_record = operands[1].object_index?;
        if !matches!(roles.get(&source_record), Some(role) if *role != 0)
            || roles.get(&result_record) != Some(&0)
            || used.contains(&source_record)
            || used.contains(&result_record)
        {
            return None;
        }
        used.insert(source_record);
        used.insert(result_record);
        let source = *spatial_by_record.get(&(scope, source_record))?;
        let result = *spatial_by_record.get(&(scope, result_record))?;
        if source.sketch != *sketch
            || result.sketch != *sketch
            || !spatial_curve(&source.geometry)
            || !spatial_curve(&result.geometry)
        {
            return None;
        }
        sources.push(source.id().clone());
        results.push(result.id().clone());
    }
    if used.len() != loci.len() {
        return None;
    }
    Some(Definition::Offset {
        sources,
        results,
        normal: normal.into(),
        distance: Length::new(distance)?,
        parameter: Some(cadmpeg_ir::sketches::OffsetParameter {
            id: parameter.clone(),
            negated: parameter_factor.is_sign_negative(),
        }),
    })
}

fn spatial_curve(geometry: &cadmpeg_ir::sketches::SpatialSketchGeometry) -> bool {
    use cadmpeg_ir::sketches::SpatialSketchGeometryDefinition;
    matches!(
        (geometry).definition(),
        SpatialSketchGeometryDefinition::Line { .. }
            | SpatialSketchGeometryDefinition::Circle { .. }
            | SpatialSketchGeometryDefinition::Arc { .. }
            | SpatialSketchGeometryDefinition::Nurbs { .. }
    )
}

fn spatial_point_distance_matches(
    first: &cadmpeg_ir::sketches::SpatialSketchGeometry,
    second: &cadmpeg_ir::sketches::SpatialSketchGeometry,
    expected: f64,
) -> bool {
    use cadmpeg_ir::sketches::SpatialSketchGeometryDefinition;

    let (
        SpatialSketchGeometryDefinition::Point { position: first },
        SpatialSketchGeometryDefinition::Point { position: second },
    ) = (first.definition(), second.definition())
    else {
        return false;
    };
    let measured = first.distance(second.get());
    let scale = 1.0 + measured.max(expected.abs());
    expected.is_finite()
        && measured.is_finite()
        && (measured - expected.abs()).abs()
            <= EPS_DIMENSIONS_SPATIAL_POINT_DISTANCE_MATCHES_E9 * scale
}

fn spatial_parallel_line_distance_matches(
    first: &cadmpeg_ir::sketches::SpatialSketchGeometry,
    second: &cadmpeg_ir::sketches::SpatialSketchGeometry,
    expected: f64,
) -> bool {
    let Some(measured) = spatial_parallel_line_distance(first, second) else {
        return false;
    };
    let scale = 1.0 + measured.max(expected.abs());
    expected.is_finite()
        && measured.is_finite()
        && (measured - expected.abs()).abs()
            <= EPS_DIMENSIONS_SPATIAL_PARALLEL_LINE_DISTANCE_MATCHES_E9 * scale
}

fn spatial_line_segment(
    geometry: &cadmpeg_ir::sketches::SpatialSketchGeometry,
) -> Option<[Point3; 2]> {
    match geometry.definition() {
        cadmpeg_ir::sketches::SpatialSketchGeometryDefinition::Line { start, end } => {
            Some([start.get(), end.get()])
        }
        _ => None,
    }
}

fn spatial_parallel_line_distance(
    first: &cadmpeg_ir::sketches::SpatialSketchGeometry,
    second: &cadmpeg_ir::sketches::SpatialSketchGeometry,
) -> Option<f64> {
    spatial_parallel_segment_distance(spatial_line_segment(first)?, spatial_line_segment(second)?)
}

fn spatial_parallel_segment_distance(
    [first_start, first_end]: [Point3; 2],
    [second_start, second_end]: [Point3; 2],
) -> Option<f64> {
    let first_direction = first_end.vector_from(first_start);
    let second_direction = second_end.vector_from(second_start);
    let first_length = first_direction.norm();
    let second_length = second_direction.norm();
    let cross = first_direction.cross(second_direction);
    if first_length <= EPS_DIMENSIONS_SPATIAL_PARALLEL_LINE_DISTANCE_E12
        || second_length <= EPS_DIMENSIONS_SPATIAL_PARALLEL_LINE_DISTANCE_E12
        || cross.norm()
            > EPS_DIMENSIONS_SPATIAL_PARALLEL_LINE_DISTANCE_E9 * first_length * second_length
    {
        return None;
    }
    let offset = second_start.vector_from(first_start);
    let area = offset.cross(first_direction).norm();
    Some(area / first_length)
}

fn spatial_parallel_line_span_distance(
    first: &cadmpeg_ir::sketches::SpatialSketchGeometry,
    second: &cadmpeg_ir::sketches::SpatialSketchGeometry,
    linear_tolerance: f64,
) -> Option<f64> {
    let first_segment = spatial_line_segment(first)?;
    let second_segment = spatial_line_segment(second)?;
    let distance = spatial_parallel_segment_distance(first_segment, second_segment)?;
    let [first_start, first_end] = first_segment;
    let [second_start, second_end] = second_segment;
    let direction = first_end.vector_from(first_start);
    let length = direction.norm();
    let project = |point: Point3| Vector3::new(point.x, point.y, point.z).dot(direction) / length;
    let first_interval = [project(first_start), project(first_end)];
    let second_interval = [project(second_start), project(second_end)];
    let first_min = first_interval[0].min(first_interval[1]);
    let first_max = first_interval[0].max(first_interval[1]);
    let second_min = second_interval[0].min(second_interval[1]);
    let second_max = second_interval[0].max(second_interval[1]);
    (first_min.max(second_min) <= first_max.min(second_max) + linear_tolerance).then_some(distance)
}

fn repeated_linear_dimension(
    candidates: &[cadmpeg_ir::sketches::SketchConstraintDefinitionInput],
    parameter: cadmpeg_ir::features::ParameterId,
) -> Option<cadmpeg_ir::sketches::SketchConstraintDefinitionInput> {
    use cadmpeg_ir::sketches::{
        SketchConstraintDefinitionInput as Definition, SketchDistanceMeasurement as Measurement,
        SketchLocus,
    };

    if candidates.len() < 2 {
        return None;
    }
    let mut entities = HashSet::new();
    let mut measurements = Vec::new();
    for candidate in candidates {
        let (first, second, measurement) = match candidate {
            Definition::Distance { entities: pair, .. } => {
                let [first, second] = pair.as_slice() else {
                    return None;
                };
                (
                    first,
                    second,
                    Measurement::Distance {
                        first: SketchLocus::Entity(first.clone()),
                        second: SketchLocus::Entity(second.clone()),
                    },
                )
            }
            Definition::HorizontalDistance { first, second, .. } => (
                locus_entity_id(first),
                locus_entity_id(second),
                Measurement::Horizontal {
                    first: first.clone(),
                    second: second.clone(),
                },
            ),
            Definition::VerticalDistance { first, second, .. } => (
                locus_entity_id(first),
                locus_entity_id(second),
                Measurement::Vertical {
                    first: first.clone(),
                    second: second.clone(),
                },
            ),
            _ => return None,
        };
        if first == second || entities.contains(first) || entities.contains(second) {
            return None;
        }
        entities.insert(first);
        entities.insert(second);
        measurements.push(measurement);
    }
    Some(Definition::RepeatedDistance {
        measurements,
        parameter,
    })
}

fn locus_entity_id(
    locus: &cadmpeg_ir::sketches::SketchLocus,
) -> &cadmpeg_ir::sketches::SketchEntityId {
    use cadmpeg_ir::sketches::SketchLocus;
    match locus {
        SketchLocus::Entity(entity)
        | SketchLocus::Start(entity)
        | SketchLocus::End(entity)
        | SketchLocus::Center(entity) => entity,
    }
}

pub(super) fn null_locus_dimension_definition(
    pair: &DesignDimensionLocusPair,
    entity: &cadmpeg_ir::sketches::SketchEntity,
    source_kind: &str,
    evaluated_value: f64,
    parameter: cadmpeg_ir::features::ParameterId,
    linear_tolerance: f64,
) -> Option<cadmpeg_ir::sketches::SketchConstraintDefinitionInput> {
    use cadmpeg_ir::sketches::{
        SketchAxis, SketchConstraintDefinitionInput as Definition, SketchGeometry,
        SketchGeometryDefinition,
    };

    if let Some(definition) = radial_dimension_definition_at_tolerance(
        entity,
        source_kind,
        evaluated_value,
        parameter.clone(),
        linear_tolerance,
    ) {
        return Some(definition);
    }
    if source_kind != "Angular Dimension-2"
        || pair.loci()[0].role != 14
        || pair.loci()[1].role != 3
        || !matches!(
            *entity.geometry.definition(),
            SketchGeometryDefinition::Line { .. }
        )
    {
        return None;
    }
    let horizontal_axis = SketchGeometry::try_from(SketchGeometryDefinition::Line {
        start: Point2::new(0.0, 0.0),
        end: Point2::new(1.0, 0.0),
    })
    .ok()?;
    line_angle_matches(&entity.geometry, &horizontal_axis, evaluated_value).then(|| {
        Definition::AngleToAxis {
            entity: entity.id().clone(),
            axis: SketchAxis::Horizontal,
            parameter,
        }
    })
}

fn radial_dimension_definition(
    entity: &cadmpeg_ir::sketches::SketchEntity,
    source_kind: &str,
    evaluated_value: f64,
    parameter: cadmpeg_ir::features::ParameterId,
) -> Option<cadmpeg_ir::sketches::SketchConstraintDefinitionInput> {
    radial_dimension_definition_at_tolerance(entity, source_kind, evaluated_value, parameter, 0.0)
}

fn radial_dimension_definition_at_tolerance(
    entity: &cadmpeg_ir::sketches::SketchEntity,
    source_kind: &str,
    evaluated_value: f64,
    parameter: cadmpeg_ir::features::ParameterId,
    linear_tolerance: f64,
) -> Option<cadmpeg_ir::sketches::SketchConstraintDefinitionInput> {
    use cadmpeg_ir::sketches::{
        SketchConstraintDefinitionInput as Definition, SketchGeometryDefinition as Geometry,
    };

    let radius = match entity.geometry.definition() {
        Geometry::Circle { radius, .. } | Geometry::Arc { radius, .. } => radius.get(),
        _ => return None,
    };
    let is_radius =
        source_kind.starts_with("Radius Dimension") || source_kind.starts_with("Radial Dimension");
    let measured = if is_radius {
        radius
    } else if source_kind.starts_with("Diameter Dimension") {
        2.0 * radius
    } else {
        return None;
    };
    let evaluated = evaluated_value * 10.0;
    let scale = 1.0 + measured.abs().max(evaluated.abs());
    let tolerance =
        linear_tolerance.max(EPS_DIMENSIONS_RADIAL_DIMENSION_DEFINITION_AT_TOLERANCE_E9 * scale);
    if !evaluated.is_finite()
        || !tolerance.is_finite()
        || tolerance < 0.0
        || (measured - evaluated).abs() > tolerance
    {
        return None;
    }
    Some(if is_radius {
        Definition::Radius {
            entity: entity.id().clone(),
            parameter,
        }
    } else {
        Definition::Diameter {
            entity: entity.id().clone(),
            parameter,
        }
    })
}

/// Resolve a parameterized linear annotation that governs an offset pair.
///
/// Fusion stores this form without a generic sketch-relation record. The
/// explicit form returns both curves; the single-source form returns the
/// source and requires a unique generated result. In both forms, the source
/// curve has a null secondary identity, the generated result has a non-null
/// secondary identity, and the annotation's evaluated parameter selects the
/// parallel or concentric offset. Requiring all three facts avoids assigning
/// an arbitrary offset when the sketch contains several generated curves.
fn annotation_offset_dimension_definition(
    frame: &DesignDimensionAnnotationFrame,
    parameter: (&DesignParameter, &cadmpeg_ir::features::ParameterId),
    scope: &str,
    curves: &[SketchCurveIdentity],
    projected: &HashMap<(&str, u32), &cadmpeg_ir::sketches::SketchEntity>,
    linear_tolerance: f64,
) -> Option<cadmpeg_ir::sketches::SketchConstraintDefinitionInput> {
    use cadmpeg_ir::scalar::Length;

    use cadmpeg_ir::sketches::{SketchConstraintDefinitionInput as Definition, SketchOffsetPair};

    let (parameter, parameter_id) = parameter;

    if !parameter.source_kind_str().starts_with("Linear Dimension")
        || !design_dimension_unit(parameter)
        || !linear_tolerance.is_finite()
        || linear_tolerance < 0.0
    {
        return None;
    }

    let curve_for_index = |record_index| {
        let mut matches = curves.iter().filter(|curve| {
            native_stream(&curve.id) == Some(scope)
                && curve.record_index == record_index
                && curve.owner_reference == Some(frame.owner_reference)
        });
        let curve = matches.next()?;
        matches.next().is_none().then_some(curve)
    };
    let non_null_indices = frame
        .operands()
        .iter()
        .filter_map(|operand| operand.geometry_record_index.map(std::num::NonZeroU32::get))
        .collect::<Vec<_>>();
    let null_locus_count = frame
        .operands()
        .iter()
        .filter(|operand| operand.geometry_record_index.is_none())
        .count();

    let explicit_pair = match non_null_indices.as_slice() {
        [first_index, second_index]
            if frame.operands().len() == 3
                && null_locus_count == 1
                && first_index != second_index =>
        {
            let first_curve = curve_for_index(*first_index)?;
            let second_curve = curve_for_index(*second_index)?;
            let (source_curve, result_curve) = match (
                first_curve.secondary_id == 0,
                second_curve.secondary_id == 0,
            ) {
                (true, false) => (first_curve, second_curve),
                (false, true) => (second_curve, first_curve),
                _ => return None,
            };
            Some((source_curve.record_index, Some(result_curve.record_index)))
        }
        _ => None,
    };
    let (source_record_index, explicit_result_record_index) = if let Some(pair) = explicit_pair {
        pair
    } else {
        match non_null_indices.as_slice() {
            [source_record_index] if frame.operands().len() == 2 && null_locus_count == 1 => {
                let source_curve = curve_for_index(*source_record_index)?;
                (source_curve.secondary_id == 0).then_some((*source_record_index, None))?
            }
            _ => return None,
        }
    };

    let source = projected.get(&(scope, source_record_index))?;
    let expected = parameter.evaluated_value().get() * 10.0;
    if !expected.is_finite() {
        return None;
    }

    let (result, distance) = if let Some(result_record_index) = explicit_result_record_index {
        let result_curve = curve_for_index(result_record_index)?;
        if result_curve.secondary_id == 0 {
            return None;
        }
        let result = projected.get(&(scope, result_record_index))?;
        let distance = sketch_curve_offset(&source.geometry, &result.geometry)?;
        let tolerance = linear_tolerance.max(
            EPS_DIMENSIONS_ANNOTATION_OFFSET_DIMENSION_DEFINITION_E9
                * (1.0 + distance.abs().max(expected.abs())),
        );
        (distance.abs() > EPS_DIMENSIONS_ANNOTATION_OFFSET_DIMENSION_DEFINITION_E9
            && (distance.abs() - expected.abs()).abs() <= tolerance)
            .then_some((result, distance))?
    } else {
        let mut matches = curves
            .iter()
            .filter(|curve| {
                native_stream(&curve.id) == Some(scope)
                    && curve.record_index != source_record_index
                    && curve.owner_reference == Some(frame.owner_reference)
                    && curve.secondary_id != 0
            })
            .filter_map(|curve| {
                let result = projected.get(&(scope, curve.record_index))?;
                let distance = sketch_curve_offset(&source.geometry, &result.geometry)?;
                let tolerance = linear_tolerance.max(
                    EPS_DIMENSIONS_ANNOTATION_OFFSET_DIMENSION_DEFINITION_E9
                        * (1.0 + distance.abs().max(expected.abs())),
                );
                (distance.abs() > EPS_DIMENSIONS_ANNOTATION_OFFSET_DIMENSION_DEFINITION_E9
                    && (distance.abs() - expected.abs()).abs() <= tolerance)
                    .then_some((result, distance))
            });
        let result = matches.next()?;
        if matches.next().is_some() {
            return None;
        }
        result
    };
    let parameter_factor = offset_parameter_factor(distance.abs(), expected)?;
    Some(Definition::Offset {
        pairs: vec![SketchOffsetPair {
            source: source.id().clone(),
            result: result.id().clone(),
            source_reversed: distance.is_sign_negative(),
        }],
        distance: Length::new(distance.abs())?,
        parameter: Some(cadmpeg_ir::sketches::OffsetParameter {
            id: parameter_id.clone(),
            negated: parameter_factor.is_sign_negative(),
        }),
    })
}

/// Resolve a radial locus group from its selected circular entity or from a
/// selected center point that uniquely identifies a measured circle or arc.
fn radial_locus_dimension_definition(
    loci: &[&cadmpeg_ir::sketches::SketchEntity],
    all_entities: &[cadmpeg_ir::sketches::SketchEntity],
    source_kind: &str,
    evaluated_value: f64,
    parameter: &cadmpeg_ir::features::ParameterId,
) -> Option<cadmpeg_ir::sketches::SketchConstraintDefinitionInput> {
    use cadmpeg_ir::sketches::{
        SketchConstraintDefinitionInput as Definition, SketchGeometryDefinition,
    };

    let mut direct = Vec::new();
    for entity in loci {
        if let Some(result) =
            radial_dimension_definition(entity, source_kind, evaluated_value, parameter.clone())
        {
            direct.push(result);
        }
    }
    if direct.len() == 1 {
        return direct.pop();
    }
    if !direct.is_empty() {
        let radius = direct
            .iter()
            .all(|definition| matches!(definition, Definition::Radius { .. }));
        let diameter = direct
            .iter()
            .all(|definition| matches!(definition, Definition::Diameter { .. }));
        if !radius && !diameter {
            return None;
        }
        let mut ids = Vec::new();
        for definition in direct {
            let (Definition::Radius { entity, .. } | Definition::Diameter { entity, .. }) =
                definition
            else {
                return None;
            };
            ids.push(entity);
        }
        let mut unique = HashSet::new();
        for id in &ids {
            if unique.contains(id) {
                return None;
            }
            unique.insert(id);
        }
        let parameter = parameter.clone();
        return Some(if radius {
            Definition::RepeatedRadius {
                entities: ids,
                parameter,
            }
        } else {
            Definition::RepeatedDiameter {
                entities: ids,
                parameter,
            }
        });
    }

    let sketch = &loci.first()?.sketch;
    if loci.iter().any(|entity| &entity.sketch != sketch) {
        return None;
    }
    let centers = || {
        loci.iter()
            .filter_map(|entity| match entity.geometry.definition() {
                SketchGeometryDefinition::Point { position } => Some(*position),
                _ => None,
            })
    };
    centers().next()?;
    let candidates = all_entities
        .iter()
        .filter(|entity| &entity.sketch == sketch)
        .filter(|entity| {
            let center = match entity.geometry.definition() {
                SketchGeometryDefinition::Circle { center, .. }
                | SketchGeometryDefinition::Arc { center, .. } => *center,
                _ => return false,
            };
            centers().any(|witness| {
                let scale = 1.0
                    + center
                        .u
                        .abs()
                        .max(center.v.abs())
                        .max(witness.u.abs())
                        .max(witness.v.abs());
                (center.u - witness.u).hypot(center.v - witness.v)
                    <= EPS_DIMENSIONS_RADIAL_LOCUS_DIMENSION_DEFINITION_E9 * scale
            })
        });
    let mut matched = None;
    let mut multiple = false;
    for entity in candidates {
        if let Some(result) =
            radial_dimension_definition(entity, source_kind, evaluated_value, parameter.clone())
        {
            let definition = result;
            if matched.is_some() {
                multiple = true;
            }
            matched = Some(definition);
        }
    }
    if multiple {
        None
    } else {
        matched
    }
}

/// Identify a point-and-line radial annotation whose point lies on the
/// line's infinite carrier. The pair locates a virtual sharp corner and does
/// not itself identify the governed circular entity.
fn radial_extension_annotation_group(
    loci: &[&cadmpeg_ir::sketches::SketchEntity],
    parameter: &DesignParameter,
) -> bool {
    use cadmpeg_ir::sketches::SketchGeometryDefinition;

    if !design_dimension_unit(parameter)
        || !(parameter.source_kind_str().starts_with("Radius Dimension")
            || parameter.source_kind_str().starts_with("Radial Dimension")
            || parameter
                .source_kind_str()
                .starts_with("Diameter Dimension"))
    {
        return false;
    }
    let [first, second] = loci else {
        return false;
    };
    let (point, start, end) = match (first.geometry.definition(), second.geometry.definition()) {
        (
            SketchGeometryDefinition::Point { position },
            SketchGeometryDefinition::Line { start, end },
        )
        | (
            SketchGeometryDefinition::Line { start, end },
            SketchGeometryDefinition::Point { position },
        ) => (*position, *start, *end),
        _ => return false,
    };
    let du = end.u - start.u;
    let dv = end.v - start.v;
    let length = du.hypot(dv);
    if length <= EPS_DIMENSIONS_RADIAL_EXTENSION_ANNOTATION_GROUP_E12 {
        return false;
    }
    let relative_u = point.u - start.u;
    let relative_v = point.v - start.v;
    relative_u.mul_add(dv, -relative_v * du).abs()
        <= EPS_DIMENSIONS_RADIAL_EXTENSION_ANNOTATION_GROUP_E9
            * (1.0 + length + relative_u.abs().max(relative_v.abs()))
}

/// Remove generic relation parses whose exact stream position is owned by a
/// typed dimension frame.
pub(crate) fn remove_dimension_frame_relations(
    relations: &mut Vec<SketchRelation>,
    pairs: &[DesignDimensionLocusPair],
    groups: &[DesignDimensionLocusGroup],
    null_pairs: &[DesignDimensionLocusPair],
) {
    let mut dimension_frames = HashSet::new();
    let frames = pairs
        .iter()
        .filter_map(|pair| Some((native_stream(&pair.id)?, pair.byte_offset())))
        .chain(
            groups
                .iter()
                .filter_map(|group| Some((native_stream(&group.id)?, group.byte_offset))),
        )
        .chain(
            null_pairs
                .iter()
                .filter_map(|pair| Some((native_stream(&pair.id)?, pair.byte_offset()))),
        );
    for frame in frames {
        dimension_frames.insert(frame);
    }
    relations.retain(|relation| {
        native_stream(&relation.id)
            .is_none_or(|scope| !dimension_frames.contains(&(scope, relation.byte_offset)))
    });
}

#[derive(Clone, Copy)]
pub(crate) struct DimensionLocusInputs<'a> {
    pub(crate) placements: &'a [DesignSketchPlacement],
    pub(crate) owners: &'a [DesignParameterOwner],
    pub(crate) pairs: &'a [DesignDimensionLocusPair],
    pub(crate) groups: &'a [DesignDimensionLocusGroup],
    pub(crate) annotation_frames: &'a [DesignDimensionAnnotationFrame],
    pub(crate) null_pairs: &'a [DesignDimensionLocusPair],
}

/// Bind geometry referenced only by dimensional companions to the sketch
/// reached through the parameter scope or the counted frame's explicit owner.
pub(crate) fn bind_dimension_loci(
    input: DimensionLocusInputs<'_>,
    points: &mut [SketchPoint],
    curves: &mut [SketchCurveIdentity],
) -> Result<(), CodecError> {
    let DimensionLocusInputs {
        placements,
        owners,
        pairs,
        groups,
        annotation_frames,
        null_pairs,
    } = input;
    let mut placements_by_scope = HashMap::new();
    for placement in placements {
        if let (Some(scope), Some(record_index), Ok(owner)) = (
            native_stream(&placement.id),
            placement.scope_record_index,
            u32::try_from(placement.entity_id.suffix()),
        ) {
            placements_by_scope.insert((scope, record_index), owner);
        }
    }
    let mut scopes_by_companion = HashMap::new();
    for owner in owners {
        if let Some(scope) = native_stream(owner.id()) {
            scopes_by_companion.insert(
                (scope, owner.companion_record_index()),
                owner.scope_record_index(),
            );
        }
    }
    let mut bindings = HashMap::<&str, HashMap<u32, u32>>::new();
    for pair in pairs {
        let Some(scope) = native_stream(&pair.id) else {
            continue;
        };
        let Some(parameter_scope) = scopes_by_companion
            .get(&(scope, pair.governing_companion_record_index))
            .copied()
        else {
            continue;
        };
        let Some(owner) = placements_by_scope.get(&(scope, parameter_scope)).copied() else {
            continue;
        };
        insert_dimension_binding(&mut bindings, scope, pair.loci()[0].geometry_index(), owner)?;
        insert_dimension_binding(&mut bindings, scope, pair.loci()[1].geometry_index(), owner)?;
    }
    for group in groups {
        let Some(scope) = native_stream(&group.id) else {
            continue;
        };
        for locus in &group.loci {
            insert_dimension_binding(
                &mut bindings,
                scope,
                locus.geometry_record_index,
                group.owner_reference,
            )?;
        }
    }
    for frame in annotation_frames {
        let Some(scope) = native_stream(&frame.id) else {
            continue;
        };
        for record_index in frame
            .operands()
            .iter()
            .filter_map(|operand| operand.geometry_record_index.map(std::num::NonZeroU32::get))
        {
            insert_dimension_binding(&mut bindings, scope, record_index, frame.owner_reference)?;
        }
    }
    for pair in null_pairs {
        let Some(scope) = native_stream(&pair.id) else {
            continue;
        };
        let Some(parameter_scope) = scopes_by_companion
            .get(&(scope, pair.governing_companion_record_index))
            .copied()
        else {
            continue;
        };
        let Some(owner) = placements_by_scope.get(&(scope, parameter_scope)).copied() else {
            continue;
        };
        insert_dimension_binding(&mut bindings, scope, pair.loci()[1].geometry_index(), owner)?;
    }
    for point in points {
        let Some(scope) = native_stream(&point.id) else {
            continue;
        };
        let Some(owner) = bindings
            .get(scope)
            .and_then(|records| records.get(&point.record_index))
            .copied()
        else {
            continue;
        };
        if point
            .owner_reference
            .replace(owner)
            .is_some_and(|existing| existing != owner)
        {
            return Err(crate::design::text::malformed_design(format_args!(
                "Fusion sketch point {} has conflicting relation and dimension owners",
                point.record_index
            )));
        }
    }
    for curve in curves {
        let Some(scope) = native_stream(&curve.id) else {
            continue;
        };
        let Some(owner) = bindings
            .get(scope)
            .and_then(|records| records.get(&curve.record_index))
            .copied()
        else {
            continue;
        };
        if curve
            .owner_reference
            .replace(owner)
            .is_some_and(|existing| existing != owner)
        {
            return Err(crate::design::text::malformed_design(format_args!(
                "Fusion sketch curve {} has conflicting relation and dimension owners",
                curve.record_index
            )));
        }
    }
    Ok(())
}

fn insert_dimension_binding<'a>(
    bindings: &mut HashMap<&'a str, HashMap<u32, u32>>,
    scope: &'a str,
    record_index: u32,
    owner: u32,
) -> Result<(), CodecError> {
    if !bindings.contains_key(scope) {
        bindings.insert(scope, HashMap::new());
    }
    let records = bindings
        .get_mut(scope)
        .ok_or_else(|| CodecError::malformed("dimension binding scope missing after insertion"))?;
    if records
        .insert(record_index, owner)
        .is_some_and(|existing| existing != owner)
    {
        return Err(crate::design::text::malformed_design(format_args!(
            "Fusion dimensional geometry record {record_index} belongs to multiple sketches"
        )));
    }
    Ok(())
}

pub(super) fn exact_atomic_constraint(
    kind: SketchConstraintKind,
    entities: &[&cadmpeg_ir::sketches::SketchEntity],
) -> Option<cadmpeg_ir::sketches::SketchConstraintDefinitionInput> {
    use cadmpeg_ir::sketches::{
        SketchConstraintDefinitionInput as Definition, SketchCoordinateAxis,
        SketchGeometryDefinition as Geometry, SketchLocus,
    };

    let lines = || -> Option<_> {
        if entities.len() == 2
            && entities[0].id() != entities[1].id()
            && entities
                .iter()
                .all(|entity| matches!(*entity.geometry.definition(), Geometry::Line { .. }))
        {
            Some((entities[0].id().clone(), entities[1].id().clone()))
        } else {
            None
        }
    };
    let curves = || -> Option<_> {
        if entities.len() == 2
            && entities[0].id() != entities[1].id()
            && entities.iter().all(|entity| {
                matches!(
                    *entity.geometry.definition(),
                    Geometry::Line { .. }
                        | Geometry::Circle { .. }
                        | Geometry::Arc { .. }
                        | Geometry::Ellipse { .. }
                        | Geometry::Nurbs { .. }
                )
            })
        {
            Some((entities[0].id().clone(), entities[1].id().clone()))
        } else {
            None
        }
    };
    let equal_size_entities = || -> Option<_> {
        let [first, second] = entities else {
            return None;
        };
        if first.id() != second.id()
            && matches!(
                (first.geometry.definition(), second.geometry.definition()),
                (Geometry::Line { .. }, Geometry::Line { .. })
                    | (
                        Geometry::Circle { .. } | Geometry::Arc { .. },
                        Geometry::Circle { .. } | Geometry::Arc { .. }
                    )
                    | (Geometry::Ellipse { .. }, Geometry::Ellipse { .. })
            )
        {
            Some((first.id().clone(), second.id().clone()))
        } else {
            None
        }
    };
    match kind {
        SketchConstraintKind::Coincident
            if entities.len() >= 2 && dimension_entity_ids_distinct(entities) =>
        {
            Some(Definition::Coincident {
                entities: entities.iter().map(|entity| entity.id().clone()).collect(),
            })
        }
        SketchConstraintKind::Colinear => {
            lines().map(|(first, second)| Definition::Collinear { first, second })
        }
        SketchConstraintKind::Concentric => {
            if entities.len() == 2
                && entities[0].id() != entities[1].id()
                && entities.iter().all(|entity| {
                    matches!(
                        *entity.geometry.definition(),
                        Geometry::Circle { .. } | Geometry::Arc { .. } | Geometry::Ellipse { .. }
                    )
                })
            {
                let (first, second) = (entities[0].id().clone(), entities[1].id().clone());
                return Some(Definition::Concentric { first, second });
            }
            let (first, second, axis) = reflected_symmetry(entities)?;
            Some(Definition::Symmetric {
                first: SketchLocus::Entity(first.id().clone()),
                second: SketchLocus::Entity(second.id().clone()),
                axis: axis.id().clone(),
            })
        }
        SketchConstraintKind::Symmetry => {
            let (first, second, axis) = reflected_symmetry(entities)?;
            Some(Definition::Symmetric {
                first: SketchLocus::Entity(first.id().clone()),
                second: SketchLocus::Entity(second.id().clone()),
                axis: axis.id().clone(),
            })
        }
        SketchConstraintKind::EqualLength => {
            lines().map(|(first, second)| Definition::Equal { first, second })
        }
        SketchConstraintKind::Parallel => match lines() {
            Some((first, second)) => Some(Definition::Parallel { first, second }),
            None => midpoint_constraint(entities, MidpointCarrier::Line),
        },
        SketchConstraintKind::Perpendicular => {
            lines().map(|(first, second)| Definition::Perpendicular { first, second })
        }
        SketchConstraintKind::Horizontal
            if entities.len() == 1
                && matches!(*entities[0].geometry.definition(), Geometry::Line { .. }) =>
        {
            Some(Definition::Horizontal {
                entity: entities[0].id().clone(),
            })
        }
        SketchConstraintKind::Horizontal
            if entities.len() == 2
                && entities[0].id() != entities[1].id()
                && entities.iter().all(|entity| {
                    matches!(*entity.geometry.definition(), Geometry::Point { .. })
                }) =>
        {
            let (first, second) = (entities[0].id().clone(), entities[1].id().clone());
            let Ok(relation) = cadmpeg_ir::sketches::SketchSameCoordinate::try_new(
                SketchLocus::Entity(first),
                SketchLocus::Entity(second),
                SketchCoordinateAxis::V,
            ) else {
                return None;
            };
            Some(Definition::SameCoordinate { relation })
        }
        SketchConstraintKind::Vertical
            if entities.len() == 1
                && matches!(*entities[0].geometry.definition(), Geometry::Line { .. }) =>
        {
            Some(Definition::Vertical {
                entity: entities[0].id().clone(),
            })
        }
        SketchConstraintKind::Vertical
            if entities.len() == 2
                && entities[0].id() != entities[1].id()
                && entities.iter().all(|entity| {
                    matches!(*entity.geometry.definition(), Geometry::Point { .. })
                }) =>
        {
            let (first, second) = (entities[0].id().clone(), entities[1].id().clone());
            let Ok(relation) = cadmpeg_ir::sketches::SketchSameCoordinate::try_new(
                SketchLocus::Entity(first),
                SketchLocus::Entity(second),
                SketchCoordinateAxis::U,
            ) else {
                return None;
            };
            Some(Definition::SameCoordinate { relation })
        }
        SketchConstraintKind::Tangent => {
            curves().map(|(first, second)| Definition::Tangent { first, second })
        }
        SketchConstraintKind::Curvature => {
            curves().map(|(first, second)| Definition::Curvature { first, second })
        }
        SketchConstraintKind::Midpoint => midpoint_constraint(entities, MidpointCarrier::LineOrArc),
        SketchConstraintKind::Equal => {
            equal_size_entities().map(|(first, second)| Definition::Equal { first, second })
        }
        SketchConstraintKind::Polygon
            if entities.len() >= 3 && dimension_entity_ids_distinct(entities) =>
        {
            let members = entities.iter().map(|entity| entity.id().clone()).collect();
            let Ok(polygon) = cadmpeg_ir::sketches::SketchPolygon::try_new(members) else {
                return None;
            };
            Some(Definition::Polygon { polygon })
        }
        SketchConstraintKind::SplineGroup
            if entities.len() >= 2 && dimension_entity_ids_distinct(entities) =>
        {
            Some(Definition::SplineGroup {
                entities: entities.iter().map(|entity| entity.id().clone()).collect(),
            })
        }
        _ => None,
    }
}

pub(super) fn exact_coincident_loci(
    entities: &[&cadmpeg_ir::sketches::SketchEntity],
    ctx: &DecodeContext<'_>,
) -> Result<Option<cadmpeg_ir::sketches::SketchConstraintDefinitionInput>, CodecError> {
    use cadmpeg_ir::sketches::{
        SketchConstraintDefinitionInput as Definition, SketchGeometryDefinition as Geometry,
        SketchLocus,
    };

    let loci = |entity: &cadmpeg_ir::sketches::SketchEntity| {
        let mut loci = Vec::new();
        if let Some([start, end]) = sketch_entity_endpoints(entity, ctx)? {
            loci.push((SketchLocus::Start(entity.id().clone()), start));
            loci.push((SketchLocus::End(entity.id().clone()), end));
        }
        match entity.geometry.definition() {
            Geometry::Point { position } => {
                loci.push((SketchLocus::Entity(entity.id().clone()), position.get()));
            }
            Geometry::Circle { center, .. }
            | Geometry::Arc { center, .. }
            | Geometry::Ellipse { center, .. }
            | Geometry::Hyperbola { center, .. } => {
                loci.push((SketchLocus::Center(entity.id().clone()), center.get()));
            }
            Geometry::Line { .. }
            | Geometry::ReferenceLine { .. }
            | Geometry::Parabola { .. }
            | Geometry::Nurbs { .. }
            | Geometry::Text { .. }
            | Geometry::ExternalReference { .. }
            | Geometry::Native { .. } => {}
        }
        Ok::<_, CodecError>(loci)
    };

    if entities.len() < 2 {
        return Ok(None);
    }
    let mut unique = HashSet::new();
    let mut loci_by_entity = Vec::new();
    for entity in entities {
        if unique.contains(entity.id()) {
            return Ok(None);
        }
        unique.insert(entity.id());
        loci_by_entity.push(loci(entity)?);
    }
    let mut unique_solution: Option<Vec<SketchLocus>> = None;
    for (first_locus, position) in &loci_by_entity[0] {
        let mut solution = Vec::new();
        solution.push(first_locus.clone());
        for member_loci in loci_by_entity.iter().skip(1) {
            {}
            let mut matches = member_loci.iter().filter(|(_, candidate)| {
                (candidate.u - position.u).hypot(candidate.v - position.v)
                    <= EPS_DIMENSIONS_EXACT_COINCIDENT_LOCI_E9
            });
            let Some(matched) = matches.next() else {
                solution.clear();
                break;
            };
            if matches.next().is_some() {
                solution.clear();
                break;
            }
            solution.push(matched.0.clone());
        }
        if solution.len() == entities.len() {
            if let Some(existing) = &unique_solution {
                if existing != &solution {
                    return Ok(None);
                }
            } else {
                unique_solution = Some(solution);
            }
        }
    }
    Ok(unique_solution.map(|loci| Definition::CoincidentLoci { loci }))
}

/// Bounded curves whose midpoint a relation bit can name.
#[derive(Clone, Copy, PartialEq, Eq)]
enum MidpointCarrier {
    Line,
    LineOrArc,
}

fn midpoint_constraint(
    entities: &[&cadmpeg_ir::sketches::SketchEntity],
    carrier: MidpointCarrier,
) -> Option<cadmpeg_ir::sketches::SketchConstraintDefinitionInput> {
    use cadmpeg_ir::sketches::{
        SketchConstraintDefinitionInput as Definition, SketchGeometryDefinition as Geometry,
        SketchLocus,
    };

    let [first, second] = entities else {
        return None;
    };
    let (curve, point, position) = match (first.geometry.definition(), second.geometry.definition())
    {
        (_, Geometry::Point { position }) => (*first, *second, position),
        (Geometry::Point { position }, _) => (*second, *first, position),
        _ => return None,
    };
    let midpoint = match curve.geometry.definition() {
        Geometry::Line { start, end } => {
            Point2::new(start.u.midpoint(end.u), start.v.midpoint(end.v))
        }
        Geometry::Arc {
            center,
            radius,
            start_angle,
            end_angle,
        } if carrier == MidpointCarrier::LineOrArc => {
            let angle = start_angle.get().midpoint(end_angle.get());
            Point2::new(
                center.u + radius.get() * angle.cos(),
                center.v + radius.get() * angle.sin(),
            )
        }
        _ => return None,
    };
    if (position.u - midpoint.u).abs() > EPS_DIMENSIONS_MIDPOINT_CONSTRAINT_E9
        || (position.v - midpoint.v).abs() > EPS_DIMENSIONS_MIDPOINT_CONSTRAINT_E9
    {
        return None;
    }
    Some(Definition::Midpoint {
        point: SketchLocus::Entity(point.id().clone()),
        entity: curve.id().clone(),
    })
}

fn indirect_angular_lines(
    scope: &str,
    operands: &[&cadmpeg_ir::sketches::SketchEntity],
    evaluated_value: f64,
    projected: &HashMap<(&str, u32), &cadmpeg_ir::sketches::SketchEntity>,
) -> Option<(
    cadmpeg_ir::sketches::SketchEntityId,
    cadmpeg_ir::sketches::SketchEntityId,
)> {
    let pair = (|| {
        use cadmpeg_ir::sketches::SketchGeometryDefinition;

        let [first, second] = operands else {
            return None;
        };
        let (point_ordinal, position, explicit_line) =
            match (first.geometry.definition(), second.geometry.definition()) {
                (
                    SketchGeometryDefinition::Point { position },
                    SketchGeometryDefinition::Line { .. },
                ) => (0, position, *second),
                (
                    SketchGeometryDefinition::Line { .. },
                    SketchGeometryDefinition::Point { position },
                ) => (1, position, *first),
                _ => return None,
            };
        if !evaluated_value.is_finite() || !(0.0..=std::f64::consts::PI).contains(&evaluated_value)
        {
            return None;
        }
        let candidates = projected
            .iter()
            .filter(|((candidate_scope, _), candidate)| {
                *candidate_scope == scope
                    && candidate.sketch == explicit_line.sketch
                    && candidate.id() != explicit_line.id()
            })
            .filter_map(|(_, candidate)| {
                let SketchGeometryDefinition::Line { start, end } = candidate.geometry.definition()
                else {
                    return None;
                };
                (sketch_points_close(position.get(), start.get())
                    || sketch_points_close(position.get(), end.get()))
                .then_some(*candidate)
            })
            .filter(|candidate| {
                line_angle_matches(
                    &explicit_line.geometry,
                    &candidate.geometry,
                    evaluated_value,
                )
            });
        let mut candidate = None;
        for next in candidates {
            match candidate {
                None => candidate = Some(next),
                Some(first) if first.id() == next.id() => {}
                Some(_) => return None,
            }
        }
        let candidate = candidate?;
        Some(if point_ordinal == 0 {
            (candidate.id(), explicit_line.id())
        } else {
            (explicit_line.id(), candidate.id())
        })
    })();
    let (first, second) = pair?;
    Some((first.clone(), second.clone()))
}

fn directional_point_dimension(
    entities: &[&cadmpeg_ir::sketches::SketchEntity],
    evaluated_mm: f64,
    parameter: cadmpeg_ir::features::ParameterId,
    linear_tolerance: f64,
) -> Option<cadmpeg_ir::sketches::SketchConstraintDefinitionInput> {
    use cadmpeg_ir::sketches::{
        SketchConstraintDefinitionInput as Definition, SketchGeometryDefinition, SketchLocus,
    };

    let [first, second] = entities else {
        return None;
    };
    let SketchGeometryDefinition::Point {
        position: first_position,
    } = first.geometry.definition()
    else {
        return None;
    };
    let SketchGeometryDefinition::Point {
        position: second_position,
    } = second.geometry.definition()
    else {
        return None;
    };
    let first_locus = SketchLocus::Entity(first.id().clone());
    let second_locus = SketchLocus::Entity(second.id().clone());
    let horizontal = linear_measurement_matches(
        first_position.u - second_position.u,
        evaluated_mm,
        linear_tolerance,
    );
    let vertical = linear_measurement_matches(
        first_position.v - second_position.v,
        evaluated_mm,
        linear_tolerance,
    );
    match (horizontal, vertical) {
        (true, false) => Some(Definition::HorizontalDistance {
            first: first_locus,
            second: second_locus,
            parameter,
        }),
        (false, true) => Some(Definition::VerticalDistance {
            first: first_locus,
            second: second_locus,
            parameter,
        }),
        (false, false) | (true, true) => None,
    }
}

fn recipe_linear_dimension_candidates(
    entities: &[cadmpeg_ir::sketches::SketchEntity],
    sketch: &cadmpeg_ir::sketches::SketchId,
    evaluated_mm: f64,
    parameter: &cadmpeg_ir::features::ParameterId,
    linear_tolerance: f64,
) -> Vec<cadmpeg_ir::sketches::SketchConstraintDefinitionInput> {
    use cadmpeg_ir::sketches::{
        SketchConstraintDefinitionInput as Definition, SketchGeometryDefinition,
    };

    let sketch_entities = entities
        .iter()
        .filter(|entity| &entity.sketch == sketch)
        .collect::<Vec<_>>();
    let points = sketch_entities
        .iter()
        .copied()
        .filter_map(|entity| match *entity.geometry.definition() {
            SketchGeometryDefinition::Point { position } => Some((entity, position.get())),
            _ => None,
        })
        .collect::<Vec<_>>();
    let lines = sketch_entities
        .iter()
        .copied()
        .filter(|entity| {
            matches!(
                *entity.geometry.definition(),
                cadmpeg_ir::sketches::SketchGeometryDefinition::Line { .. }
            )
        })
        .collect::<Vec<_>>();
    let mut line_pairs = Vec::new();
    for first in 0..lines.len() {
        for second in first + 1..lines.len() {
            if parallel_line_separation(lines[first], lines[second], evaluated_mm, linear_tolerance)
            {
                line_pairs.push((lines[first], lines[second]));
            }
        }
    }
    let same_point = |left: Point2, right: Point2| {
        let scale = 1.0
            + left
                .u
                .abs()
                .max(left.v.abs())
                .max(right.u.abs())
                .max(right.v.abs());
        (left.u - right.u).abs() <= EPS_DIMENSIONS_RECIPE_LINEAR_DIMENSION_CANDIDATES_E9 * scale
            && (left.v - right.v).abs()
                <= EPS_DIMENSIONS_RECIPE_LINEAR_DIMENSION_CANDIDATES_E9 * scale
    };
    let point_on_endpoint = |position: Point2, line: &cadmpeg_ir::sketches::SketchEntity| match line
        .geometry
        .definition()
    {
        SketchGeometryDefinition::Line { start, end } => [start.get(), end.get()]
            .into_iter()
            .any(|end| same_point(position, end)),
        _ => false,
    };
    let mut candidates = Vec::new();
    for first in 0..points.len() {
        for second in first + 1..points.len() {
            let subsumed_by_line_pair = line_pairs.iter().any(|(first_line, second_line)| {
                (point_on_endpoint(points[first].1, first_line)
                    && point_on_endpoint(points[second].1, second_line))
                    || (point_on_endpoint(points[first].1, second_line)
                        && point_on_endpoint(points[second].1, first_line))
            });
            if subsumed_by_line_pair {
                continue;
            }
            if let Some(definition) = directional_point_dimension(
                &[points[first].0, points[second].0],
                evaluated_mm,
                parameter.clone(),
                0.0,
            ) {
                candidates.push(definition);
            }
        }
    }
    for (first, second) in line_pairs {
        let definition = Definition::Distance {
            entities: vec![first.id().clone(), second.id().clone()],
            parameter: parameter.clone(),
        };
        candidates.push(definition);
    }
    candidates
}

fn recipe_dimension_candidate_entities(
    candidates: &[cadmpeg_ir::sketches::SketchConstraintDefinitionInput],
) -> Vec<cadmpeg_ir::sketches::SketchEntityId> {
    use cadmpeg_ir::sketches::SketchConstraintDefinitionInput as Definition;

    let mut entities = Vec::new();
    let mut add = |entity: &cadmpeg_ir::sketches::SketchEntityId| {
        if !entities.contains(entity) {
            entities.push(entity.clone());
        }
    };
    for candidate in candidates {
        match candidate {
            Definition::Distance {
                entities: candidate_entities,
                ..
            } => {
                for entity in candidate_entities {
                    add(entity);
                }
            }
            Definition::HorizontalDistance { first, second, .. }
            | Definition::VerticalDistance { first, second, .. } => {
                add(locus_entity_id(first));
                add(locus_entity_id(second));
            }
            _ => {}
        }
    }
    entities
}

/// Resolve an ambiguous recipe-backed directional distance through one
/// detached point on the extension of an axis-aligned bounded line. The other
/// measured point must be an endpoint of that same line.
fn recipe_extension_point_dimension(
    candidates: &[cadmpeg_ir::sketches::SketchConstraintDefinitionInput],
    entities: &[cadmpeg_ir::sketches::SketchEntity],
    sketch: &cadmpeg_ir::sketches::SketchId,
) -> Option<cadmpeg_ir::sketches::SketchConstraintDefinitionInput> {
    use cadmpeg_ir::sketches::{
        SketchConstraintDefinitionInput as Definition, SketchGeometryDefinition,
    };

    let sketch_entities = || entities.iter().filter(|entity| &entity.sketch == sketch);
    let lines = || sketch_entities().filter_map(|entity| Some((entity, line_segment(entity)?)));
    let point = |id: &cadmpeg_ir::sketches::SketchEntityId| {
        sketch_entities().find_map(|entity| match *entity.geometry.definition() {
            SketchGeometryDefinition::Point { position } if entity.id() == id => {
                Some(position.get())
            }
            _ => None,
        })
    };
    let is_any_line_endpoint = |position: Point2| {
        lines().any(|(_, [start, end])| {
            sketch_points_close(position, start) || sketch_points_close(position, end)
        })
    };
    let mut matched = None;
    for candidate in candidates {
        let (first_id, second_id, horizontal) = match candidate {
            Definition::HorizontalDistance { first, second, .. } => {
                (locus_entity_id(first), locus_entity_id(second), true)
            }
            Definition::VerticalDistance { first, second, .. } => {
                (locus_entity_id(first), locus_entity_id(second), false)
            }
            _ => continue,
        };
        let first_position = point(first_id)?;
        let second_position = point(second_id)?;
        let qualifies = [
            (first_position, second_position),
            (second_position, first_position),
        ]
        .into_iter()
        .any(|(detached, endpoint)| {
            !is_any_line_endpoint(detached)
                && lines().any(|(_, [start, end])| {
                    let du = end.u - start.u;
                    let dv = end.v - start.v;
                    let length = du.hypot(dv);
                    let axis_aligned = if horizontal {
                        dv.abs()
                            <= EPS_DIMENSIONS_RECIPE_EXTENSION_POINT_DIMENSION_E9 * (1.0 + du.abs())
                    } else {
                        du.abs()
                            <= EPS_DIMENSIONS_RECIPE_EXTENSION_POINT_DIMENSION_E9 * (1.0 + dv.abs())
                    };
                    if !length.is_finite()
                        || length <= EPS_DIMENSIONS_RECIPE_EXTENSION_POINT_DIMENSION_E9
                        || !axis_aligned
                    {
                        return false;
                    }
                    if !sketch_points_close(endpoint, start) && !sketch_points_close(endpoint, end)
                    {
                        return false;
                    }
                    let relative_u = detached.u - start.u;
                    let relative_v = detached.v - start.v;
                    let carrier_error = relative_u
                        .mul_add(dv / length, -relative_v * (du / length))
                        .abs();
                    let carrier_tolerance = EPS_DIMENSIONS_RECIPE_EXTENSION_POINT_DIMENSION_E9
                        * (1.0 + length + relative_u.abs().max(relative_v.abs()));
                    let projection =
                        (relative_u * (du / length) + relative_v * (dv / length)) / length;
                    carrier_error <= carrier_tolerance
                        && !(-EPS_DIMENSIONS_RECIPE_EXTENSION_POINT_DIMENSION_E9
                            ..=1.0 + EPS_DIMENSIONS_RECIPE_EXTENSION_POINT_DIMENSION_E9)
                            .contains(&projection)
                })
        });
        if qualifies {
            if matched.is_some() {
                return None;
            }
            matched = Some(candidate);
        }
    }
    let (first, second, parameter, horizontal) = match matched? {
        Definition::HorizontalDistance {
            first,
            second,
            parameter,
        } => (first, second, parameter, true),
        Definition::VerticalDistance {
            first,
            second,
            parameter,
        } => (first, second, parameter, false),
        _ => return None,
    };
    let (first, second, parameter) = (first.clone(), second.clone(), parameter.clone());
    Some(if horizontal {
        Definition::HorizontalDistance {
            first,
            second,
            parameter,
        }
    } else {
        Definition::VerticalDistance {
            first,
            second,
            parameter,
        }
    })
}

fn parallel_line_separation(
    first: &cadmpeg_ir::sketches::SketchEntity,
    second: &cadmpeg_ir::sketches::SketchEntity,
    evaluated_mm: f64,
    linear_tolerance: f64,
) -> bool {
    let Some(separation) = parallel_line_distance(first, second) else {
        return false;
    };
    linear_measurement_matches(separation, evaluated_mm, linear_tolerance)
}

/// Resolve the symmetric line-width form of a paired linear dimension.
///
/// A nonzero role on both parallel line loci selects a width dimension: the
/// stored value is twice the perpendicular carrier separation. Zero roles use
/// the ordinary direct separation rules in `exact_definition`.
fn symmetric_parallel_line_dimension_definition(
    first: &cadmpeg_ir::sketches::SketchEntity,
    second: &cadmpeg_ir::sketches::SketchEntity,
    roles: (u32, u32),
    parameter: &DesignParameter,
    parameter_id: cadmpeg_ir::features::ParameterId,
    linear_tolerance: f64,
) -> Option<cadmpeg_ir::sketches::SketchConstraintDefinitionInput> {
    use cadmpeg_ir::sketches::SketchConstraintDefinitionInput as Definition;

    let (first_role, second_role) = roles;

    if first_role == 0
        || second_role == 0
        || !parameter.source_kind_str().starts_with("Linear Dimension")
        || !design_dimension_unit(parameter)
    {
        return None;
    }
    let separation = parallel_line_distance(first, second)?;
    let expected = parameter.evaluated_value().get() * 10.0;
    linear_measurement_matches(2.0 * separation, expected, linear_tolerance).then(|| {
        Definition::Distance {
            entities: vec![first.id().clone(), second.id().clone()],
            parameter: parameter_id,
        }
    })
}

fn line_segment(geometry: &cadmpeg_ir::sketches::SketchEntity) -> Option<[Point2; 2]> {
    match geometry.geometry.definition() {
        cadmpeg_ir::sketches::SketchGeometryDefinition::Line { start, end } => {
            Some([start.get(), end.get()])
        }
        _ => None,
    }
}

fn parallel_line_distance(
    first: &cadmpeg_ir::sketches::SketchEntity,
    second: &cadmpeg_ir::sketches::SketchEntity,
) -> Option<f64> {
    parallel_segment_distance(line_segment(first)?, line_segment(second)?)
}

fn parallel_segment_distance(
    [first_start, first_end]: [Point2; 2],
    [second_start, second_end]: [Point2; 2],
) -> Option<f64> {
    let first_direction = Point2::new(first_end.u - first_start.u, first_end.v - first_start.v);
    let second_direction =
        Point2::new(second_end.u - second_start.u, second_end.v - second_start.v);
    let first_length = first_direction.u.hypot(first_direction.v);
    let second_length = second_direction.u.hypot(second_direction.v);
    if first_length <= EPS_DIMENSIONS_PARALLEL_LINE_DISTANCE_E12
        || second_length <= EPS_DIMENSIONS_PARALLEL_LINE_DISTANCE_E12
    {
        return None;
    }
    let cross = first_direction.u * second_direction.v - first_direction.v * second_direction.u;
    if cross.abs() > EPS_DIMENSIONS_PARALLEL_LINE_DISTANCE_E9 * first_length * second_length {
        return None;
    }
    let offset = Point2::new(
        second_start.u - first_start.u,
        second_start.v - first_start.v,
    );
    Some((offset.u * first_direction.v - offset.v * first_direction.u).abs() / first_length)
}

fn parallel_line_span_distance(
    first: &cadmpeg_ir::sketches::SketchEntity,
    second: &cadmpeg_ir::sketches::SketchEntity,
    linear_tolerance: f64,
) -> Option<f64> {
    let first_segment = line_segment(first)?;
    let second_segment = line_segment(second)?;
    let distance = parallel_segment_distance(first_segment, second_segment)?;
    let [first_start, first_end] = first_segment;
    let [second_start, second_end] = second_segment;
    let direction = Point2::new(first_end.u - first_start.u, first_end.v - first_start.v);
    let length = direction.u.hypot(direction.v);
    let project = |point: Point2| (point.u * direction.u + point.v * direction.v) / length;
    let first_interval = [project(first_start), project(first_end)];
    let second_interval = [project(second_start), project(second_end)];
    let first_min = first_interval[0].min(first_interval[1]);
    let first_max = first_interval[0].max(first_interval[1]);
    let second_min = second_interval[0].min(second_interval[1]);
    let second_max = second_interval[0].max(second_interval[1]);
    (first_min.max(second_min) <= first_max.min(second_max) + linear_tolerance).then_some(distance)
}

fn concentric_circle_separation(
    first: &cadmpeg_ir::sketches::SketchEntity,
    second: &cadmpeg_ir::sketches::SketchEntity,
    evaluated_mm: f64,
    linear_tolerance: f64,
) -> bool {
    use cadmpeg_ir::sketches::SketchGeometryDefinition;

    let (
        SketchGeometryDefinition::Circle {
            center: first_center,
            radius: first_radius,
        },
        SketchGeometryDefinition::Circle {
            center: second_center,
            radius: second_radius,
        },
    ) = (first.geometry.definition(), second.geometry.definition())
    else {
        return false;
    };
    if !evaluated_mm.is_finite() {
        return false;
    }
    let center_separation =
        (first_center.u - second_center.u).hypot(first_center.v - second_center.v);
    if !linear_measurement_matches(center_separation, 0.0, linear_tolerance) {
        return false;
    }
    let measured = (first_radius.get() - second_radius.get()).abs();
    measured > 0.0 && linear_measurement_matches(measured, evaluated_mm, linear_tolerance)
}

fn point_line_separation(
    first: &cadmpeg_ir::sketches::SketchEntity,
    second: &cadmpeg_ir::sketches::SketchEntity,
    evaluated_mm: f64,
    linear_tolerance: f64,
) -> bool {
    use cadmpeg_ir::sketches::SketchGeometryDefinition;

    let (point, line) = match (first.geometry.definition(), second.geometry.definition()) {
        (
            SketchGeometryDefinition::Point { position },
            SketchGeometryDefinition::Line { start, end },
        )
        | (
            SketchGeometryDefinition::Line { start, end },
            SketchGeometryDefinition::Point { position },
        ) => (*position, (*start, *end)),
        _ => return false,
    };
    let direction = Point2::new(line.1.u - line.0.u, line.1.v - line.0.v);
    let length = direction.u.hypot(direction.v);
    if length <= EPS_DIMENSIONS_POINT_LINE_SEPARATION_E12 || !evaluated_mm.is_finite() {
        return false;
    }
    let offset = Point2::new(point.u - line.0.u, point.v - line.0.v);
    let measured = (offset.u * direction.v - offset.v * direction.u).abs() / length;
    linear_measurement_matches(measured, evaluated_mm, linear_tolerance)
}

fn linear_measurement_matches(measured: f64, expected: f64, linear_tolerance: f64) -> bool {
    if !measured.is_finite()
        || !expected.is_finite()
        || !linear_tolerance.is_finite()
        || linear_tolerance < 0.0
    {
        return false;
    }
    let expected = expected.abs();
    let scale = 1.0 + measured.abs().max(expected);
    (measured.abs() - expected).abs()
        <= linear_tolerance.max(EPS_DIMENSIONS_LINEAR_MEASUREMENT_MATCHES_E9 * scale)
}

fn two_locus_distance_dimension(
    entities: &[&cadmpeg_ir::sketches::SketchEntity],
    parameter: cadmpeg_ir::features::ParameterId,
) -> Option<cadmpeg_ir::sketches::SketchConstraintDefinitionInput> {
    use cadmpeg_ir::sketches::SketchConstraintDefinitionInput as Definition;

    if entities.len() != 2 || entities[0].id() == entities[1].id() {
        return None;
    }
    Some(Definition::Distance {
        entities: entities.iter().map(|entity| entity.id().clone()).collect(),
        parameter,
    })
}

#[cfg(test)]
fn counted_role_relation(
    entities: &[&cadmpeg_ir::sketches::SketchEntity],
    owner_role: u64,
) -> Option<cadmpeg_ir::sketches::SketchConstraintDefinitionInput> {
    counted_role_relation_at_tolerance(
        entities,
        &crate::records::sketch_relations::constraint_kinds_from_state(owner_role).0,
        0.0,
    )
}

fn counted_role_relation_at_tolerance(
    entities: &[&cadmpeg_ir::sketches::SketchEntity],
    owner_kinds: &[SketchConstraintKind],
    linear_tolerance: f64,
) -> Option<cadmpeg_ir::sketches::SketchConstraintDefinitionInput> {
    use cadmpeg_ir::sketches::{
        SketchConstraintDefinitionInput as Definition, SketchGeometryDefinition as Geometry,
    };
    owner_kinds.iter().find_map(|kind| match (kind, entities) {
        (SketchConstraintKind::Horizontal | SketchConstraintKind::Vertical, [entity]) => {
            let Geometry::Line { start, end } = entity.geometry.definition() else {
                return None;
            };
            let du = end.u - start.u;
            let dv = end.v - start.v;
            let length = du.hypot(dv);
            if length <= EPS_DIMENSIONS_COUNTED_ROLE_RELATION_AT_TOLERANCE_E12 {
                return None;
            }
            match kind {
                SketchConstraintKind::Horizontal
                    if dv.abs()
                        <= EPS_DIMENSIONS_COUNTED_ROLE_RELATION_AT_TOLERANCE_E9 * length =>
                {
                    Some(Definition::Horizontal {
                        entity: entity.id().clone(),
                    })
                }
                SketchConstraintKind::Vertical
                    if du.abs()
                        <= EPS_DIMENSIONS_COUNTED_ROLE_RELATION_AT_TOLERANCE_E9 * length =>
                {
                    Some(Definition::Vertical {
                        entity: entity.id().clone(),
                    })
                }
                _ => None,
            }
        }
        (SketchConstraintKind::Tangent, [first, second])
            if exact_line_arc_tangency(entities, linear_tolerance)
                || exact_circular_tangency(entities, linear_tolerance) =>
        {
            Some(Definition::Tangent {
                first: first.id().clone(),
                second: second.id().clone(),
            })
        }
        (SketchConstraintKind::Equal, [first, second]) if exact_equal_size(entities) => {
            Some(Definition::Equal {
                first: first.id().clone(),
                second: second.id().clone(),
            })
        }
        _ => None,
    })
}

fn exact_line_arc_tangency(
    entities: &[&cadmpeg_ir::sketches::SketchEntity],
    linear_tolerance: f64,
) -> bool {
    use cadmpeg_ir::sketches::SketchGeometryDefinition as Geometry;

    let [first, second] = entities else {
        return false;
    };
    let (line_start, line_end, center, radius, arc_start, arc_end) =
        match (first.geometry.definition(), second.geometry.definition()) {
            (
                Geometry::Line {
                    start: line_start,
                    end: line_end,
                },
                Geometry::Arc {
                    center,
                    radius,
                    start_angle,
                    end_angle,
                },
            )
            | (
                Geometry::Arc {
                    center,
                    radius,
                    start_angle,
                    end_angle,
                },
                Geometry::Line {
                    start: line_start,
                    end: line_end,
                },
            ) => (
                *line_start,
                *line_end,
                *center,
                radius.get(),
                Point2::new(
                    center.u + radius.get() * start_angle.get().cos(),
                    center.v + radius.get() * start_angle.get().sin(),
                ),
                Point2::new(
                    center.u + radius.get() * end_angle.get().cos(),
                    center.v + radius.get() * end_angle.get().sin(),
                ),
            ),
            _ => return false,
        };
    let line_direction = Point2::new(line_end.u - line_start.u, line_end.v - line_start.v);
    let line_length = line_direction.u.hypot(line_direction.v);
    if line_length <= EPS_DIMENSIONS_EXACT_LINE_ARC_TANGENCY_E12 {
        return false;
    }
    [line_start, line_end].into_iter().any(|line_point| {
        [arc_start, arc_end].into_iter().any(|arc_point| {
            let scale = 1.0
                + line_point
                    .u
                    .abs()
                    .max(line_point.v.abs())
                    .max(arc_point.u.abs())
                    .max(arc_point.v.abs())
                    .max(radius);
            let point_tolerance = linear_tolerance.max(EPS_CIRCULAR_TANGENCY * scale.max(1.0));
            let radius_direction = Point2::new(arc_point.u - center.u, arc_point.v - center.v);
            (line_point.u - arc_point.u).abs() <= point_tolerance
                && (line_point.v - arc_point.v).abs() <= point_tolerance
                && (line_direction.u * radius_direction.u + line_direction.v * radius_direction.v)
                    .abs()
                    <= EPS_DIMENSIONS_EXACT_LINE_ARC_TANGENCY_E9 * line_length * radius
        })
    })
}

fn exact_circular_tangency(
    entities: &[&cadmpeg_ir::sketches::SketchEntity],
    linear_tolerance: f64,
) -> bool {
    use cadmpeg_ir::sketches::SketchGeometryDefinition as Geometry;

    let [first, second] = entities else {
        return false;
    };
    if first.id() == second.id() {
        return false;
    }
    let circular = |geometry: &cadmpeg_ir::sketches::SketchGeometry| match geometry.definition() {
        Geometry::Circle { center, radius } | Geometry::Arc { center, radius, .. } => {
            Some((*center, radius.get()))
        }
        _ => None,
    };
    let Some((first_center, first_radius)) = circular(&first.geometry) else {
        return false;
    };
    let Some((second_center, second_radius)) = circular(&second.geometry) else {
        return false;
    };
    let center_delta = Point2::new(
        second_center.u - first_center.u,
        second_center.v - first_center.v,
    );
    let center_distance = center_delta.u.hypot(center_delta.v);
    if !center_distance.is_finite() || center_distance <= EPS_CIRCULAR_TANGENCY_LENGTH {
        return false;
    }
    let tangent_tolerance =
        linear_tolerance.max(EPS_CIRCULAR_TANGENCY * (1.0 + first_radius.max(second_radius)));
    let close = |left: f64, right: f64| (left - right).abs() <= tangent_tolerance;
    if !close(center_distance, first_radius + second_radius)
        && !close(center_distance, (first_radius - second_radius).abs())
    {
        return false;
    }
    let unit = Point2::new(
        center_delta.u / center_distance,
        center_delta.v / center_distance,
    );
    let candidate_points = [
        Point2::new(
            first_center.u + first_radius * unit.u,
            first_center.v + first_radius * unit.v,
        ),
        Point2::new(
            first_center.u - first_radius * unit.u,
            first_center.v - first_radius * unit.v,
        ),
    ];
    candidate_points.iter().any(|point| {
        let second_radius_error =
            (point.u - second_center.u).hypot(point.v - second_center.v) - second_radius;
        if second_radius_error.abs() > tangent_tolerance {
            return false;
        }
        circular_entity_contains_point(&first.geometry, *point, linear_tolerance)
            && circular_entity_contains_point(&second.geometry, *point, linear_tolerance)
    })
}

fn circular_entity_contains_point(
    geometry: &cadmpeg_ir::sketches::SketchGeometry,
    point: Point2,
    linear_tolerance: f64,
) -> bool {
    use cadmpeg_ir::sketches::SketchGeometryDefinition as Geometry;

    match geometry.definition() {
        Geometry::Circle { .. } => true,
        Geometry::Arc {
            center,
            radius,
            start_angle,
            end_angle,
        } => {
            let relative = Point2::new(point.u - center.u, point.v - center.v);
            let scale = 1.0 + radius.get().abs().max(point.u.abs().max(point.v.abs()));
            let point_tolerance = linear_tolerance.max(EPS_CIRCULAR_TANGENCY * scale.max(1.0));
            // Not a stated value: the floored quantity is the length-to-angle ratio
            // computed here, and the constant is the double-precision angular
            // resolution of that division, never read from the file.
            let angular_tolerance =
                (point_tolerance / radius.get().abs()).max(EPS_CIRCULAR_TANGENCY);
            let angle = relative.v.atan2(relative.u);
            let angle_close = |left: f64, right: f64| {
                let distance = (left - right).abs().rem_euclid(std::f64::consts::TAU);
                distance.min(std::f64::consts::TAU - distance) <= angular_tolerance
            };
            (relative.u.hypot(relative.v) - radius.get()).abs() <= point_tolerance
                && (angle_close(angle, start_angle.get())
                    || angle_close(angle, end_angle.get())
                    || angle_in_sweep(angle, start_angle.get(), end_angle.get(), angular_tolerance))
        }
        _ => false,
    }
}

const EPS_CIRCULAR_TANGENCY: f64 = 1.0e-9;
const EPS_CIRCULAR_TANGENCY_LENGTH: f64 = 1.0e-12;

fn exact_equal_size(entities: &[&cadmpeg_ir::sketches::SketchEntity]) -> bool {
    use cadmpeg_ir::sketches::SketchGeometryDefinition as Geometry;

    let [first, second] = entities else {
        return false;
    };
    if first.id() == second.id() {
        return false;
    }
    let close = |first: f64, second: f64| {
        (first - second).abs()
            <= EPS_DIMENSIONS_EXACT_EQUAL_SIZE_E9 * (1.0 + first.abs().max(second.abs()))
    };
    match (first.geometry.definition(), second.geometry.definition()) {
        (
            Geometry::Line {
                start: first_start,
                end: first_end,
            },
            Geometry::Line {
                start: second_start,
                end: second_end,
            },
        ) => {
            let first_length = (first_end.u - first_start.u).hypot(first_end.v - first_start.v);
            let second_length =
                (second_end.u - second_start.u).hypot(second_end.v - second_start.v);
            first_length > EPS_DIMENSIONS_EXACT_EQUAL_SIZE_E12
                && second_length > EPS_DIMENSIONS_EXACT_EQUAL_SIZE_E12
                && close(first_length, second_length)
        }
        (
            Geometry::Circle { radius: first, .. } | Geometry::Arc { radius: first, .. },
            Geometry::Circle { radius: second, .. } | Geometry::Arc { radius: second, .. },
        ) => close(first.get(), second.get()),
        (
            Geometry::Ellipse {
                radii: first_radii, ..
            },
            Geometry::Ellipse {
                radii: second_radii,
                ..
            },
        ) => {
            close(first_radii.major().get(), second_radii.major().get())
                && close(first_radii.minor().get(), second_radii.minor().get())
        }
        _ => false,
    }
}

const EPS_CENTERED_RELATION: f64 = 1.0e-9;
const EPS_OFFSET_SWEEP: f64 = 1.0e-12;

fn exact_centered_entity_relation(
    entities: &[&cadmpeg_ir::sketches::SketchEntity],
) -> Option<cadmpeg_ir::sketches::SketchConstraintDefinitionInput> {
    use cadmpeg_ir::sketches::{
        SketchConstraintDefinitionInput as Definition, SketchGeometryDefinition,
    };

    let [first, second] = entities else {
        return None;
    };
    if first.id() == second.id() {
        return None;
    }
    let centered_geometry =
        |entity: &cadmpeg_ir::sketches::SketchEntity| match entity.geometry.definition() {
            SketchGeometryDefinition::Circle { center, radius }
            | SketchGeometryDefinition::Arc { center, radius, .. } => {
                Some((center.get(), Some(radius.get())))
            }
            SketchGeometryDefinition::Ellipse { center, .. } => Some((center.get(), None)),
            _ => None,
        };
    let (first_center, first_radius) = centered_geometry(first)?;
    let (second_center, second_radius) = centered_geometry(second)?;
    if !sketch_points_close(first_center, second_center) {
        return None;
    }
    if let (Some(first_radius), Some(second_radius)) = (first_radius, second_radius) {
        let scale = 1.0 + first_radius.abs().max(second_radius.abs());
        if (first_radius - second_radius).abs() <= EPS_CENTERED_RELATION * scale {
            return Some(Definition::Coradial {
                first: first.id().clone(),
                second: second.id().clone(),
            });
        }
    }
    Some(Definition::Concentric {
        first: first.id().clone(),
        second: second.id().clone(),
    })
}

fn exact_counted_dimension_relation(
    entities: &[&cadmpeg_ir::sketches::SketchEntity],
) -> Option<cadmpeg_ir::sketches::SketchConstraintDefinitionInput> {
    use cadmpeg_ir::sketches::{
        SketchConstraintDefinitionInput as Definition, SketchGeometryDefinition, SketchLocus,
    };

    if let Some(definition) = exact_centered_entity_relation(entities) {
        return Some(definition);
    }
    if let Some((first, second, axis)) = reflected_symmetry(entities) {
        return Some(Definition::Symmetric {
            first: SketchLocus::Entity(first.id().clone()),
            second: SketchLocus::Entity(second.id().clone()),
            axis: axis.id().clone(),
        });
    }
    let [first, second] = entities else {
        return None;
    };
    if first.id() == second.id() {
        return None;
    }
    let point_on_geometry = |point: &cadmpeg_ir::sketches::SketchEntity,
                             geometry: &cadmpeg_ir::sketches::SketchEntity|
     -> bool {
        let SketchGeometryDefinition::Point { position } = *point.geometry.definition() else {
            return false;
        };
        point_lies_on_sketch_geometry(position.get(), &geometry.geometry)
    };
    if point_on_geometry(first, second) || point_on_geometry(second, first) {
        return Some(Definition::Coincident {
            entities: vec![first.id().clone(), second.id().clone()],
        });
    }
    let (
        SketchGeometryDefinition::Line {
            start: first_start,
            end: first_end,
        },
        SketchGeometryDefinition::Line {
            start: second_start,
            end: second_end,
        },
    ) = (first.geometry.definition(), second.geometry.definition())
    else {
        return None;
    };
    let first_direction = Point2::new(first_end.u - first_start.u, first_end.v - first_start.v);
    let second_direction =
        Point2::new(second_end.u - second_start.u, second_end.v - second_start.v);
    let first_length = first_direction.u.hypot(first_direction.v);
    let second_length = second_direction.u.hypot(second_direction.v);
    if !first_length.is_finite()
        || !second_length.is_finite()
        || first_length <= EPS_DIMENSIONS_EXACT_COUNTED_DIMENSION_RELATION_E9
        || second_length <= EPS_DIMENSIONS_EXACT_COUNTED_DIMENSION_RELATION_E9
    {
        return None;
    }
    let first_direction = Point2::new(
        first_direction.u / first_length,
        first_direction.v / first_length,
    );
    let second_direction = Point2::new(
        second_direction.u / second_length,
        second_direction.v / second_length,
    );
    let cross = first_direction
        .u
        .mul_add(second_direction.v, -first_direction.v * second_direction.u);
    if cross.abs() <= EPS_DIMENSIONS_EXACT_COUNTED_DIMENSION_RELATION_E9 {
        let signed_offset = parallel_line_offset(&first.geometry, &second.geometry)?;
        return Some(
            if signed_offset.abs()
                <= EPS_DIMENSIONS_EXACT_COUNTED_DIMENSION_RELATION_E9 * (1.0 + first_length)
            {
                Definition::Collinear {
                    first: first.id().clone(),
                    second: second.id().clone(),
                }
            } else {
                Definition::Parallel {
                    first: first.id().clone(),
                    second: second.id().clone(),
                }
            },
        );
    }
    let dot = first_direction
        .u
        .mul_add(second_direction.u, first_direction.v * second_direction.v);

    (dot.abs() <= EPS_DIMENSIONS_EXACT_COUNTED_DIMENSION_RELATION_E9).then(|| {
        Definition::Perpendicular {
            first: first.id().clone(),
            second: second.id().clone(),
        }
    })
}

pub(super) fn point_lies_on_sketch_geometry(
    point: Point2,
    geometry: &cadmpeg_ir::sketches::SketchGeometry,
) -> bool {
    use cadmpeg_ir::sketches::SketchGeometryDefinition;

    if let SketchGeometryDefinition::Nurbs { curve } = geometry.definition() {
        if curve.periodic() {
            return false;
        }
        let tolerance = EPS_DIMENSIONS_POINT_LIES_ON_SKETCH_GEOMETRY_E9
            * (1.0 + point.u.abs().max(point.v.abs()));
        let lanes = PcurveEvaluatorLanes::new(curve.pole_rows());
        return cadmpeg_ir::eval::nurbs_pcurve_contains_point(
            curve.degree(),
            curve.knots(),
            lanes.points(),
            lanes.weights(),
            point,
            tolerance,
        ) == Some(true);
    }

    let close = |left: f64, right: f64| {
        left.is_finite()
            && right.is_finite()
            && (left - right).abs()
                <= EPS_DIMENSIONS_POINT_LIES_ON_SKETCH_GEOMETRY_E9
                    * (1.0 + left.abs().max(right.abs()))
    };
    match geometry.definition() {
        SketchGeometryDefinition::Point { position } => sketch_points_close(point, position.get()),
        SketchGeometryDefinition::Line { start, end } => {
            let direction = Point2::new(end.u - start.u, end.v - start.v);
            let length = direction.u.hypot(direction.v);
            if !length.is_finite() || length <= EPS_DIMENSIONS_POINT_LIES_ON_SKETCH_GEOMETRY_E9 {
                return false;
            }
            let unit = Point2::new(direction.u / length, direction.v / length);
            let relative = Point2::new(point.u - start.u, point.v - start.v);
            let parameter = relative.u.mul_add(unit.u, relative.v * unit.v) / length;
            let perpendicular = relative.u.mul_add(unit.v, -relative.v * unit.u);
            (-EPS_DIMENSIONS_POINT_LIES_ON_SKETCH_GEOMETRY_E9
                ..=1.0 + EPS_DIMENSIONS_POINT_LIES_ON_SKETCH_GEOMETRY_E9)
                .contains(&parameter)
                && perpendicular.is_finite()
                && perpendicular.abs() <= EPS_DIMENSIONS_POINT_LIES_ON_SKETCH_GEOMETRY_E9
        }
        SketchGeometryDefinition::ReferenceLine { origin, direction } => {
            let length = direction.u.hypot(direction.v);
            if !length.is_finite() || length <= EPS_DIMENSIONS_POINT_LIES_ON_SKETCH_GEOMETRY_E9 {
                return false;
            }
            let relative = Point2::new(point.u - origin.u, point.v - origin.v);
            let perpendicular = relative
                .u
                .mul_add(direction.v / length, -relative.v * (direction.u / length));
            perpendicular.is_finite()
                && perpendicular.abs() <= EPS_DIMENSIONS_POINT_LIES_ON_SKETCH_GEOMETRY_E9
        }
        SketchGeometryDefinition::Circle { center, radius } => {
            close((point.u - center.u).hypot(point.v - center.v), radius.get())
        }
        SketchGeometryDefinition::Arc {
            center,
            radius,
            start_angle,
            end_angle,
        } => {
            let relative = Point2::new(point.u - center.u, point.v - center.v);
            close(relative.u.hypot(relative.v), radius.get())
                && angle_in_sweep(
                    relative.v.atan2(relative.u),
                    start_angle.get(),
                    end_angle.get(),
                    EPS_DIMENSIONS_POINT_LIES_ON_SKETCH_GEOMETRY_E9,
                )
        }
        SketchGeometryDefinition::Ellipse {
            center,
            major_angle,
            radii,
            bounds,
        } => {
            let relative = Point2::new(point.u - center.u, point.v - center.v);
            let (sin, cos) = major_angle.get().sin_cos();
            let x = relative.u.mul_add(cos, relative.v * sin) / radii.major().get();
            let y = (-relative.u).mul_add(sin, relative.v * cos) / radii.minor().get();
            close(x.mul_add(x, y * y), 1.0)
                && match bounds {
                    Some([start, end]) => angle_in_sweep(
                        y.atan2(x),
                        start.get(),
                        end.get(),
                        EPS_DIMENSIONS_POINT_LIES_ON_SKETCH_GEOMETRY_E9,
                    ),
                    None => true,
                }
        }
        SketchGeometryDefinition::Hyperbola {
            center,
            major_angle,
            major_radius,
            minor_radius,
            bounds,
        } => {
            let relative = Point2::new(point.u - center.u, point.v - center.v);
            let (sin, cos) = major_angle.get().sin_cos();
            let x = relative.u.mul_add(cos, relative.v * sin) / major_radius.get();
            let y = (-relative.u).mul_add(sin, relative.v * cos) / minor_radius.get();
            let parameter = y.asinh();
            close(x, parameter.cosh())
                && match bounds {
                    Some([start, end]) => {
                        parameter >= start.get() - EPS_DIMENSIONS_POINT_LIES_ON_SKETCH_GEOMETRY_E9
                            && parameter
                                <= end.get() + EPS_DIMENSIONS_POINT_LIES_ON_SKETCH_GEOMETRY_E9
                    }
                    None => true,
                }
        }
        SketchGeometryDefinition::Parabola {
            vertex,
            axis_angle,
            focal_length,
            bounds,
        } => {
            let relative = Point2::new(point.u - vertex.u, point.v - vertex.v);
            let (sin, cos) = axis_angle.get().sin_cos();
            let x = relative.u.mul_add(cos, relative.v * sin);
            let y = (-relative.u).mul_add(sin, relative.v * cos);
            let parameter = y;
            let Some(axial) = cadmpeg_ir::math::product_quotient(
                [parameter, parameter],
                [4.0, focal_length.get()],
            ) else {
                return false;
            };
            close(x, axial.get())
                && match bounds {
                    Some([start, end]) => {
                        parameter
                            >= start.get().min(end.get())
                                - EPS_DIMENSIONS_POINT_LIES_ON_SKETCH_GEOMETRY_E9
                            && parameter
                                <= start.get().max(end.get())
                                    + EPS_DIMENSIONS_POINT_LIES_ON_SKETCH_GEOMETRY_E9
                    }
                    None => true,
                }
        }
        SketchGeometryDefinition::Nurbs { .. }
        | SketchGeometryDefinition::Text { .. }
        | SketchGeometryDefinition::ExternalReference { .. }
        | SketchGeometryDefinition::Native { .. } => false,
    }
}

struct CountedOffset {
    pairs: Vec<cadmpeg_ir::sketches::SketchOffsetPair>,
    distance: cadmpeg_ir::scalar::Length,
}

fn exact_counted_offset(
    loci: &[crate::records::dimensions::DesignDimensionLocus],
    entities: &HashMap<u32, &cadmpeg_ir::sketches::SketchEntity>,
    secondary_ids: &HashMap<u32, u64>,
    linear_tolerance: f64,
) -> Option<CountedOffset> {
    use cadmpeg_ir::scalar::Length;
    use cadmpeg_ir::sketches::SketchOffsetPair;

    if loci.len() != entities.len() || loci.len() < 2 || !loci.len().is_multiple_of(2) {
        return None;
    }
    let source_count = loci.len() / 2;
    let role_partition = loci[..source_count].iter().all(|locus| locus.role != 0)
        && loci[source_count..].iter().all(|locus| locus.role == 0);
    let identity_partition = loci.iter().all(|locus| locus.role != 0)
        && loci[..source_count]
            .iter()
            .all(|locus| secondary_ids.get(&locus.geometry_record_index).copied() == Some(0))
        && loci[source_count..].iter().all(|locus| {
            secondary_ids
                .get(&locus.geometry_record_index)
                .is_some_and(|secondary_id| *secondary_id != 0)
        });
    if !role_partition && !identity_partition {
        return None;
    }
    let mut source_records = HashSet::new();
    let mut result_records = HashSet::new();
    for locus in &loci[..source_count] {
        source_records.insert(locus.geometry_record_index);
    }
    for locus in &loci[source_count..] {
        result_records.insert(locus.geometry_record_index);
    }
    if source_records.len() != source_count || result_records.len() != source_count {
        return None;
    }
    let mut used_members = HashSet::new();
    let mut pairs = Vec::new();
    let mut canonical_distance: Option<f64> = None;
    for [source_locus, result_locus] in loci.as_chunks::<2>().0 {
        let source_record_index = source_locus.returned.value;
        let result_record_index = result_locus.returned.value;
        if !source_records.contains(&source_record_index)
            || !result_records.contains(&result_record_index)
            || used_members.contains(&source_record_index)
            || used_members.contains(&result_record_index)
        {
            return None;
        }
        used_members.insert(source_record_index);
        used_members.insert(result_record_index);
        let source = entities.get(&source_record_index)?;
        let result = entities.get(&result_record_index)?;
        let distance = match sketch_curve_offset(&source.geometry, &result.geometry) {
            Some(distance) => distance,
            None => cadmpeg_ir::eval::fitted_nurbs_offset_frame_distance(
                &source.geometry,
                &result.geometry,
                linear_tolerance,
            )
            .map(cadmpeg_ir::scalar::FiniteReal::get)?,
        };
        if distance.abs() <= EPS_DIMENSIONS_EXACT_COUNTED_OFFSET_E9 {
            return None;
        }
        let source_reversed = offset_source_reversed(distance, &mut canonical_distance)?;
        pairs.push(SketchOffsetPair {
            source: source.id().clone(),
            result: result.id().clone(),
            source_reversed,
        });
    }
    Some(CountedOffset {
        pairs,
        distance: Length::new(canonical_distance?)?,
    })
}

fn offset_parameter_factor(distance: f64, parameter_value: f64) -> Option<f64> {
    let scale = 1.0 + distance.abs().max(parameter_value.abs());
    (distance.is_finite()
        && parameter_value.is_finite()
        && (distance - parameter_value.abs()).abs()
            <= scale * EPS_DIMENSIONS_OFFSET_PARAMETER_FACTOR_E9)
        .then(|| {
            if parameter_value.is_sign_positive() {
                1.0
            } else {
                -1.0
            }
        })
}

fn line_angle_matches(
    first: &cadmpeg_ir::sketches::SketchGeometry,
    second: &cadmpeg_ir::sketches::SketchGeometry,
    expected: f64,
) -> bool {
    use cadmpeg_ir::sketches::SketchGeometryDefinition;

    let SketchGeometryDefinition::Line {
        start: first_start,
        end: first_end,
    } = first.definition()
    else {
        return false;
    };
    let SketchGeometryDefinition::Line {
        start: second_start,
        end: second_end,
    } = second.definition()
    else {
        return false;
    };
    let first_du = first_end.u - first_start.u;
    let first_dv = first_end.v - first_start.v;
    let second_du = second_end.u - second_start.u;
    let second_dv = second_end.v - second_start.v;
    let unit = |du, dv| {
        cadmpeg_ir::features::FiniteVector3::new(cadmpeg_ir::math::Vector3::new(du, dv, 0.0))
            .and_then(cadmpeg_ir::features::FiniteVector3::unit_nonzero)
    };
    let Some(first) = unit(first_du, first_dv) else {
        return false;
    };
    let Some(second) = unit(second_du, second_dv) else {
        return false;
    };
    let angle = first.cross(second).norm().atan2(first.dot(second));
    let supplementary = std::f64::consts::PI - angle;
    let scale = 1.0 + expected.abs();
    (angle - expected).abs() <= scale * EPS_DIMENSIONS_LINE_ANGLE_MATCHES_E9
        || (supplementary - expected).abs() <= scale * EPS_DIMENSIONS_LINE_ANGLE_MATCHES_E9
}

pub(super) fn exact_offset_constraint(
    relation: &SketchRelation,
    scope: &str,
    projected: &HashMap<(&str, u32), &cadmpeg_ir::sketches::SketchEntity>,
) -> Option<cadmpeg_ir::sketches::SketchConstraintDefinitionInput> {
    use cadmpeg_ir::scalar::Length;
    use cadmpeg_ir::sketches::{SketchConstraintDefinitionInput as Definition, SketchOffsetPair};

    // The second reference run carries the bijection in order. A stored offset
    // relation's sources can be another offset relation's results, so their
    // secondary identities are not null and only the run order separates the
    // two sides.
    let ordered_pairs = crate::design::relation_kinds::sole_constraint_kind(relation)
        == Some(SketchConstraintKind::Offset);
    let minimum_return_members = if ordered_pairs { 2 } else { 4 };
    if crate::design::relation_kinds::unknown_constraint_bits(relation.definition.state()) != 0
        || !matches!(
            crate::design::relation_kinds::sole_constraint_kind(relation),
            Some(SketchConstraintKind::Perpendicular | SketchConstraintKind::Offset)
        )
        || relation.return_members().len() < minimum_return_members
        || !relation.return_members().len().is_multiple_of(2)
        || relation.return_members().len() != relation.members().len()
        || relation
            .return_members()
            .iter()
            .any(|member| member.reference.resolved().is_none())
    {
        return None;
    }
    let mut pairs = Vec::new();
    let mut used_entities = HashSet::new();
    let mut canonical_distance: Option<f64> = None;
    for members in relation.return_members().chunks_exact(2) {
        let (first_record_index, first_secondary_id, second_record_index, second_secondary_id) =
            match (&members[0].reference, &members[1].reference) {
                (
                    crate::records::sketch_relations::SketchRelationReference::Resolved(
                        SketchRelationOperand::Curve {
                            record_index: first_record_index,
                            secondary_id: first_secondary_id,
                            ..
                        },
                    ),
                    crate::records::sketch_relations::SketchRelationReference::Resolved(
                        SketchRelationOperand::Curve {
                            record_index: second_record_index,
                            secondary_id: second_secondary_id,
                            ..
                        },
                    ),
                ) => (
                    *first_record_index,
                    *first_secondary_id,
                    *second_record_index,
                    *second_secondary_id,
                ),
                _ => return None,
            };
        let (source_record_index, result_record_index) =
            if ordered_pairs || (first_secondary_id == 0 && second_secondary_id != 0) {
                (first_record_index, second_record_index)
            } else {
                return None;
            };
        let source = projected.get(&(scope, source_record_index))?;
        let result = projected.get(&(scope, result_record_index))?;
        if used_entities.contains(source.id()) || used_entities.contains(result.id()) {
            return None;
        }
        used_entities.insert(source.id());
        used_entities.insert(result.id());
        let distance = if ordered_pairs {
            sketch_curve_offset(&source.geometry, &result.geometry)?
        } else {
            parallel_line_offset(&source.geometry, &result.geometry)?
        };
        if distance.abs() <= EPS_DIMENSIONS_EXACT_OFFSET_CONSTRAINT_E9 {
            return None;
        }
        let source_reversed = offset_source_reversed(distance, &mut canonical_distance)?;
        pairs.push(SketchOffsetPair {
            source: source.id().clone(),
            result: result.id().clone(),
            source_reversed,
        });
    }
    Some(Definition::Offset {
        pairs,
        distance: Length::new(canonical_distance?)?,
        parameter: None,
    })
}

fn offset_source_reversed(distance: f64, canonical: &mut Option<f64>) -> Option<bool> {
    let magnitude = distance.abs();
    let Some(expected) = *canonical else {
        *canonical = Some(magnitude);
        return Some(distance.is_sign_negative());
    };
    let scale = 1.0 + magnitude.max(expected);
    if (magnitude - expected).abs() > scale * EPS_DIMENSIONS_OFFSET_SOURCE_REVERSED_E9 {
        return None;
    }
    Some(distance.is_sign_negative())
}

fn sketch_curve_offset(
    source: &cadmpeg_ir::sketches::SketchGeometry,
    result: &cadmpeg_ir::sketches::SketchGeometry,
) -> Option<f64> {
    use cadmpeg_ir::sketches::SketchGeometryDefinition;

    match (source.definition(), result.definition()) {
        (
            SketchGeometryDefinition::Circle {
                center: source_center,
                radius: source_radius,
            },
            SketchGeometryDefinition::Circle {
                center: result_center,
                radius: result_radius,
            },
        ) => {
            let scale = 1.0
                + source_center
                    .u
                    .abs()
                    .max(source_center.v.abs())
                    .max(result_center.u.abs())
                    .max(result_center.v.abs())
                    .max(source_radius.get())
                    .max(result_radius.get());
            ((source_center.u - result_center.u).abs() <= EPS_CENTERED_RELATION * scale
                && (source_center.v - result_center.v).abs() <= EPS_CENTERED_RELATION * scale)
                .then_some(source_radius.get() - result_radius.get())
        }
        (
            SketchGeometryDefinition::Circle {
                center: source_center,
                radius: source_radius,
            },
            SketchGeometryDefinition::Arc {
                center: result_center,
                radius: result_radius,
                start_angle: result_start,
                end_angle: result_end,
            },
        ) => {
            let scale = 1.0
                + source_center
                    .u
                    .abs()
                    .max(source_center.v.abs())
                    .max(result_center.u.abs())
                    .max(result_center.v.abs())
                    .max(source_radius.get())
                    .max(result_radius.get());
            let result_sweep = result_end.get() - result_start.get();
            (result_sweep.abs() > EPS_OFFSET_SWEEP
                && (source_center.u - result_center.u).abs() <= EPS_CENTERED_RELATION * scale
                && (source_center.v - result_center.v).abs() <= EPS_CENTERED_RELATION * scale)
                .then_some(source_radius.get() - result_radius.get())
        }
        (
            SketchGeometryDefinition::Arc {
                center: source_center,
                radius: source_radius,
                start_angle: source_start,
                end_angle: source_end,
            },
            SketchGeometryDefinition::Circle {
                center: result_center,
                radius: result_radius,
            },
        ) => {
            let scale = 1.0
                + source_center
                    .u
                    .abs()
                    .max(source_center.v.abs())
                    .max(result_center.u.abs())
                    .max(result_center.v.abs())
                    .max(source_radius.get())
                    .max(result_radius.get());
            let source_sweep = source_end.get() - source_start.get();
            (source_sweep.abs() > EPS_OFFSET_SWEEP
                && (source_center.u - result_center.u).abs() <= EPS_CENTERED_RELATION * scale
                && (source_center.v - result_center.v).abs() <= EPS_CENTERED_RELATION * scale)
                .then_some(source_sweep.signum() * (source_radius.get() - result_radius.get()))
        }
        (
            SketchGeometryDefinition::Arc {
                center: source_center,
                radius: source_radius,
                start_angle: source_start,
                end_angle: source_end,
            },
            SketchGeometryDefinition::Arc {
                center: result_center,
                radius: result_radius,
                start_angle: result_start,
                end_angle: result_end,
            },
        ) => {
            let scale = 1.0
                + source_center
                    .u
                    .abs()
                    .max(source_center.v.abs())
                    .max(result_center.u.abs())
                    .max(result_center.v.abs())
                    .max(source_radius.get())
                    .max(result_radius.get());
            let source_sweep = source_end.get() - source_start.get();
            let result_sweep = result_end.get() - result_start.get();
            let angular_overlap = [source_start.get(), source_end.get()]
                .into_iter()
                .any(|angle| {
                    angle_in_sweep(
                        angle,
                        result_start.get(),
                        result_end.get(),
                        EPS_DIMENSIONS_SKETCH_CURVE_OFFSET_E9,
                    )
                })
                || [result_start.get(), result_end.get()]
                    .into_iter()
                    .any(|angle| {
                        angle_in_sweep(
                            angle,
                            source_start.get(),
                            source_end.get(),
                            EPS_DIMENSIONS_SKETCH_CURVE_OFFSET_E9,
                        )
                    });
            (source_sweep.abs() > EPS_OFFSET_SWEEP
                && result_sweep.abs() > EPS_OFFSET_SWEEP
                && source_sweep.signum() == result_sweep.signum()
                && angular_overlap
                && (source_center.u - result_center.u).abs()
                    <= EPS_DIMENSIONS_SKETCH_CURVE_OFFSET_E9 * scale
                && (source_center.v - result_center.v).abs()
                    <= EPS_DIMENSIONS_SKETCH_CURVE_OFFSET_E9 * scale)
                .then_some(source_sweep.signum() * (source_radius.get() - result_radius.get()))
        }
        _ => parallel_line_offset(source, result),
    }
}

fn parallel_line_offset(
    source: &cadmpeg_ir::sketches::SketchGeometry,
    result: &cadmpeg_ir::sketches::SketchGeometry,
) -> Option<f64> {
    use cadmpeg_ir::sketches::SketchGeometryDefinition;

    let SketchGeometryDefinition::Line {
        start: source_start,
        end: source_end,
    } = source.definition()
    else {
        return None;
    };
    let SketchGeometryDefinition::Line {
        start: result_start,
        end: result_end,
    } = result.definition()
    else {
        return None;
    };
    let source_du = source_end.u - source_start.u;
    let source_dv = source_end.v - source_start.v;
    let result_du = result_end.u - result_start.u;
    let result_dv = result_end.v - result_start.v;
    let source_length = source_du.hypot(source_dv);
    let result_length = result_du.hypot(result_dv);
    if source_length <= EPS_DIMENSIONS_PARALLEL_LINE_OFFSET_E12
        || result_length <= EPS_DIMENSIONS_PARALLEL_LINE_OFFSET_E12
    {
        return None;
    }
    let source_direction = cadmpeg_ir::features::FiniteVector3::new(
        cadmpeg_ir::math::Vector3::new(source_du, source_dv, 0.0),
    )?
    .unit_nonzero()?;
    let result_direction = cadmpeg_ir::features::FiniteVector3::new(
        cadmpeg_ir::math::Vector3::new(result_du, result_dv, 0.0),
    )?
    .unit_nonzero()?;
    let parallel_error = source_direction.cross(result_direction).norm();
    if parallel_error > EPS_DIMENSIONS_PARALLEL_LINE_OFFSET_E9 {
        return None;
    }
    let normal_u = -source_direction.y;
    let normal_v = source_direction.x;
    let distance_at = |point: &Point2| {
        (point.u - source_start.u) * normal_u + (point.v - source_start.v) * normal_v
    };
    let start_distance = distance_at(result_start);
    let end_distance = distance_at(result_end);
    let scale = 1.0 + start_distance.abs().max(end_distance.abs());
    ((start_distance - end_distance).abs() <= scale * EPS_DIMENSIONS_PARALLEL_LINE_OFFSET_E9)
        .then_some(start_distance)
}

fn reflected_symmetry<'a>(
    entities: &[&'a cadmpeg_ir::sketches::SketchEntity],
) -> Option<(
    &'a cadmpeg_ir::sketches::SketchEntity,
    &'a cadmpeg_ir::sketches::SketchEntity,
    &'a cadmpeg_ir::sketches::SketchEntity,
)> {
    use cadmpeg_ir::sketches::SketchGeometryDefinition;

    let [first, second, third] = entities else {
        return None;
    };
    if first.id() == second.id() || first.id() == third.id() || second.id() == third.id() {
        return None;
    }
    let mut candidate = None;
    for (axis_ordinal, axis) in entities.iter().enumerate() {
        let axis = *axis;
        let SketchGeometryDefinition::Line {
            start: axis_start,
            end: axis_end,
        } = axis.geometry.definition()
        else {
            continue;
        };
        let others = match axis_ordinal {
            0 => [*second, *third],
            1 => [*first, *third],
            _ => [*first, *second],
        };
        if reflected_geometry_matches(
            &others[0].geometry,
            &others[1].geometry,
            axis_start,
            axis_end,
        ) && candidate.replace((others[0], others[1], axis)).is_some()
        {
            return None;
        }
    }
    candidate
}

fn reflected_geometry_matches(
    first: &cadmpeg_ir::sketches::SketchGeometry,
    second: &cadmpeg_ir::sketches::SketchGeometry,
    axis_start: &Point2,
    axis_end: &Point2,
) -> bool {
    use cadmpeg_ir::sketches::SketchGeometryDefinition;

    match (first.definition(), second.definition()) {
        (
            SketchGeometryDefinition::Point {
                position: first_position,
            },
            SketchGeometryDefinition::Point {
                position: second_position,
            },
        ) => reflect_point(first_position.get(), *axis_start, *axis_end)
            .is_some_and(|reflected| sketch_points_close(reflected, second_position.get())),
        (
            SketchGeometryDefinition::Line {
                start: first_start,
                end: first_end,
            },
            SketchGeometryDefinition::Line {
                start: second_start,
                end: second_end,
            },
        ) => {
            let Some(reflected_start) = reflect_point(first_start.get(), *axis_start, *axis_end)
            else {
                return false;
            };
            let Some(reflected_end) = reflect_point(first_end.get(), *axis_start, *axis_end) else {
                return false;
            };
            sketch_points_close(reflected_start, second_start.get())
                && sketch_points_close(reflected_end, second_end.get())
                || sketch_points_close(reflected_start, second_end.get())
                    && sketch_points_close(reflected_end, second_start.get())
        }
        (
            SketchGeometryDefinition::Circle {
                center: first_center,
                radius: first_radius,
            },
            SketchGeometryDefinition::Circle {
                center: second_center,
                radius: second_radius,
            },
        ) => {
            let radius_scale = 1.0 + first_radius.get().abs().max(second_radius.get().abs());
            (first_radius.get() - second_radius.get()).abs()
                <= EPS_DIMENSIONS_REFLECTED_GEOMETRY_MATCHES_E9 * radius_scale
                && reflect_point(first_center.get(), *axis_start, *axis_end)
                    .is_some_and(|reflected| sketch_points_close(reflected, second_center.get()))
        }
        (
            SketchGeometryDefinition::Arc {
                center: first_center,
                radius: first_radius,
                start_angle: first_start,
                end_angle: first_end,
            },
            SketchGeometryDefinition::Arc {
                center: second_center,
                radius: second_radius,
                start_angle: second_start,
                end_angle: second_end,
            },
        ) => {
            let radius_scale = 1.0 + first_radius.get().abs().max(second_radius.get().abs());
            let first_sweep = (first_end.get() - first_start.get()).abs();
            let second_sweep = (second_end.get() - second_start.get()).abs();
            let sweep_scale = 1.0 + first_sweep.max(second_sweep);
            if (first_radius.get() - second_radius.get()).abs()
                > EPS_DIMENSIONS_REFLECTED_GEOMETRY_MATCHES_E9 * radius_scale
                || (first_sweep - second_sweep).abs()
                    > EPS_DIMENSIONS_REFLECTED_GEOMETRY_MATCHES_E9 * sweep_scale
                || !reflect_point(first_center.get(), *axis_start, *axis_end)
                    .is_some_and(|reflected| sketch_points_close(reflected, second_center.get()))
            {
                return false;
            }
            let arc_point = |center: Point2, radius: f64, angle: f64| {
                Point2::new(
                    center.u + radius * angle.cos(),
                    center.v + radius * angle.sin(),
                )
            };
            let Some(reflected_start) = reflect_point(
                arc_point(first_center.get(), first_radius.get(), first_start.get()),
                *axis_start,
                *axis_end,
            ) else {
                return false;
            };
            let Some(reflected_end) = reflect_point(
                arc_point(first_center.get(), first_radius.get(), first_end.get()),
                *axis_start,
                *axis_end,
            ) else {
                return false;
            };
            let second_start =
                arc_point(second_center.get(), second_radius.get(), second_start.get());
            let second_end = arc_point(second_center.get(), second_radius.get(), second_end.get());
            sketch_points_close(reflected_start, second_start)
                && sketch_points_close(reflected_end, second_end)
                || sketch_points_close(reflected_start, second_end)
                    && sketch_points_close(reflected_end, second_start)
        }
        _ => false,
    }
}

const EPS_REFLECTION_AXIS_LENGTH: f64 = 1.0e-9;

fn reflect_point(point: Point2, axis_start: Point2, axis_end: Point2) -> Option<Point2> {
    let direction =
        cadmpeg_ir::math::Vector3::new(axis_end.u - axis_start.u, axis_end.v - axis_start.v, 0.0);
    if direction.norm() <= EPS_REFLECTION_AXIS_LENGTH {
        return None;
    }
    let unit = cadmpeg_ir::features::FiniteVector3::new(direction)?.unit_nonzero()?;
    let normal = Point2::new(-unit.y, unit.x);
    let distance = normal
        .u
        .mul_add(point.u - axis_start.u, normal.v * (point.v - axis_start.v));
    if !distance.is_finite() {
        return None;
    }
    let reflected = Point2::new(
        (-2.0 * normal.u).mul_add(distance, point.u),
        (-2.0 * normal.v).mul_add(distance, point.v),
    );
    reflected.is_finite().then_some(reflected)
}

fn sketch_points_close(first: Point2, second: Point2) -> bool {
    let scale = 1.0
        + first
            .u
            .abs()
            .max(first.v.abs())
            .max(second.u.abs())
            .max(second.v.abs());
    (first.u - second.u).abs() <= scale * EPS_DIMENSIONS_SKETCH_POINTS_CLOSE_E9
        && (first.v - second.v).abs() <= scale * EPS_DIMENSIONS_SKETCH_POINTS_CLOSE_E9
}

pub(super) fn relation_kind_name(relation: &SketchRelation) -> String {
    struct Names([Option<&'static str>; 21]);
    impl std::fmt::Display for Names {
        fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            let mut separator = "";
            for name in self.0.iter().flatten() {
                formatter.write_str(separator)?;
                formatter.write_str(name)?;
                separator = "+";
            }
            Ok(())
        }
    }
    let mut names = [None; 21];
    for (slot, kind) in
        names
            .iter_mut()
            .zip(crate::records::sketch_relations::constraint_kinds_iter(
                relation.definition.state(),
            ))
    {
        *slot = Some(match kind {
            SketchConstraintKind::Coincident => "coincident",
            SketchConstraintKind::Colinear => "collinear",
            SketchConstraintKind::Concentric => "concentric",
            SketchConstraintKind::EqualLength => "equal_length",
            SketchConstraintKind::Parallel => "parallel",
            SketchConstraintKind::Perpendicular => "perpendicular",
            SketchConstraintKind::Horizontal => "horizontal",
            SketchConstraintKind::Vertical => "vertical",
            SketchConstraintKind::Tangent => "tangent",
            SketchConstraintKind::Curvature => "curvature",
            SketchConstraintKind::Symmetry => "symmetry",
            SketchConstraintKind::Equal => "equal",
            SketchConstraintKind::Midpoint => "midpoint",
            SketchConstraintKind::Polygon => "polygon",
            SketchConstraintKind::Offset => "offset",
            SketchConstraintKind::SplineGroup => "spline_group",
            SketchConstraintKind::CircularPattern => "circular_pattern",
            SketchConstraintKind::RectangularPattern => "rectangular_pattern",
            SketchConstraintKind::TextFrame => "text_frame",
            SketchConstraintKind::TextPath => "text_path",
        });
    }
    if crate::design::relation_kinds::unknown_constraint_bits(relation.definition.state()) != 0 {
        names[20] = Some("unknown_bits");
    }
    Names(names).to_string()
}

pub(super) fn planar_point(point: &Point3) -> bool {
    point.is_finite() && point.z.abs() <= EPS_DIMENSIONS_PLANAR_POINT_E9
}

pub(super) fn sketch_normal_sign(normal: &Vector3) -> Option<f64> {
    (normal.x.abs() <= EPS_DIMENSIONS_SKETCH_NORMAL_SIGN_E9
        && normal.y.abs() <= EPS_DIMENSIONS_SKETCH_NORMAL_SIGN_E9
        && (normal.z.abs() - 1.0).abs() <= EPS_DIMENSIONS_SKETCH_NORMAL_SIGN_E9)
        .then_some(normal.z.signum())
}

pub(super) fn expression_identifiers(expression: &str) -> impl Iterator<Item = &str> {
    let identifier_character = |character: char| {
        character.is_alphanumeric() || matches!(character, '_' | '"' | '$' | '°' | 'µ')
    };
    let mut start = None;
    let mut characters = expression
        .char_indices()
        .chain(std::iter::once((expression.len(), '\0')));
    std::iter::from_fn(move || {
        for (offset, character) in characters.by_ref() {
            if identifier_character(character) {
                start.get_or_insert(offset);
                continue;
            }
            let Some(token_start) = start.take() else {
                continue;
            };
            let token = &expression[token_start..offset];
            if !token
                .chars()
                .next()
                .is_some_and(|character| character.is_alphabetic() || character == '_')
            {
                continue;
            }
            let next = expression[offset..]
                .chars()
                .find(|character| !character.is_whitespace());
            if next == Some('(') {
                continue;
            }
            let previous = expression[..token_start]
                .chars()
                .rev()
                .find(|character| !character.is_whitespace());
            if matches!(token, "mm" | "cm" | "m" | "in" | "ft" | "deg" | "rad")
                && previous.is_some_and(|character| character.is_ascii_digit() || character == ')')
            {
                continue;
            }
            return Some(token);
        }
        None
    })
}

/// Count decoded same-stream parameter-name symbols that have no neutral
/// dependency edge.
pub(crate) fn unresolved_parameter_expression_dependency_count(
    native: &[DesignParameter],
    projected: &[cadmpeg_ir::features::DesignParameter],
) -> usize {
    let mut projected_by_native_ref = HashMap::new();
    let mut projected_by_id = HashMap::new();
    for parameter in projected {
        if let Some(native_ref) = parameter.native_ref.as_deref() {
            projected_by_native_ref.insert(native_ref, parameter);
        }
        projected_by_id.insert(&parameter.id, parameter);
    }
    let mut names_by_stream = HashMap::<&str, HashSet<&str>>::new();
    for parameter in native {
        let Some(stream) = native_stream(&parameter.id) else {
            continue;
        };
        if !names_by_stream.contains_key(stream) {
            names_by_stream.insert(stream, HashSet::new());
        }
        if let Some(names) = names_by_stream.get_mut(stream) {
            names.insert(parameter.name());
        }
    }

    let mut unresolved = 0usize;
    for parameter in native {
        let Some(stream) = native_stream(&parameter.id) else {
            continue;
        };
        let Some(names) = names_by_stream.get(stream) else {
            continue;
        };
        let Some(projected) = projected_by_native_ref.get(parameter.id.as_str()) else {
            continue;
        };
        let mut dependency_names = HashSet::new();
        for dependency in &projected.dependencies {
            if let Some(dependency) = projected_by_id.get(dependency) {
                dependency_names.insert(dependency.name.as_str());
            }
        }
        let mut identifiers = HashSet::new();
        for identifier in expression_identifiers(parameter.expression()) {
            if names.contains(identifier) {
                identifiers.insert(identifier);
            }
        }
        unresolved += identifiers
            .into_iter()
            .filter(|identifier| !dependency_names.contains(*identifier))
            .count();
    }
    unresolved
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod numerical_range_tests;
