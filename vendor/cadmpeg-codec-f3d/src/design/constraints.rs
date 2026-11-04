// SPDX-License-Identifier: Apache-2.0
#![cfg_attr(test, allow(clippy::needless_range_loop))]
//! Project sketch constraint relations.

use cadmpeg_core::convert::f64_from_index;

use crate::design::dimensions::{
    exact_atomic_constraint, exact_coincident_loci, exact_offset_constraint, relation_kind_name,
};
use crate::design::face_resolve::design_angle;
use crate::design::feature_project::design_length;
use crate::ids::native_stream;
use crate::records::{
    parameters::DesignParameter,
    sketch_geometry::{SketchCurveIdentity, SketchPoint, SketchText},
    sketch_placement::DesignSketchPlacement,
    sketch_relations::{SketchConstraintKind, SketchRelation},
};
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::geometry::pcurve::{PcurveNurbs, PcurveNurbsPoles};
use cadmpeg_ir::math::Point2;
use std::collections::{HashMap, HashSet};

const EPS_CONSTRAINTS_EXACT_RECTANGULAR_PATTERN_E9: f64 = 1.0e-9;
const EPS_CONSTRAINTS_SCALAR_CLOSE_E9: f64 = 1.0e-9;

/// Project each native relation as an exact atomic constraint or an explicitly
/// native aggregate when its semantic members do not prove neutral loci.
pub(crate) fn project_sketch_constraints(
    ctx: &DecodeContext<'_>,
    placements: &[DesignSketchPlacement],
    parameters: &[DesignParameter],
    (points, curves, texts): (&[SketchPoint], &[SketchCurveIdentity], &[SketchText]),
    relations: &[SketchRelation],
    entities: &[cadmpeg_ir::sketches::SketchEntity],
) -> Result<Vec<cadmpeg_ir::sketches::SketchConstraint>, CodecError> {
    use cadmpeg_ir::sketches::{
        NativeOperandField, SketchConstraint, SketchConstraintDefinitionInput as Definition,
        SketchNativeOperand,
    };

    let mut sketches = HashMap::new();
    for placement in placements {
        let id = crate::ids::neutral_sketch_id(placement);
        {}
        if !entities.iter().any(|entity| entity.sketch == id) {
            continue;
        }
        let (Some(scope), Ok(entity_id)) = (
            native_stream(&placement.id),
            u32::try_from(placement.entity_id.suffix()),
        ) else {
            continue;
        };
        sketches.insert((scope, entity_id), id);
    }
    let mut record_keys_by_native_ref = HashMap::new();
    for point in points {
        if let Some(scope) = native_stream(&point.id) {
            record_keys_by_native_ref.insert(point.id.as_str(), (scope, point.record_index));
        }
    }
    for curve in curves {
        if let Some(scope) = native_stream(&curve.id) {
            record_keys_by_native_ref.insert(curve.id.as_str(), (scope, curve.record_index));
        }
    }
    for text in texts {
        if let Some(scope) = native_stream(&text.id) {
            record_keys_by_native_ref.insert(text.id.as_str(), (scope, text.record_index));
        }
    }
    let mut projected = HashMap::new();
    for entity in entities {
        if let Some(key) = entity
            .native_ref
            .as_deref()
            .and_then(|native_ref| record_keys_by_native_ref.get(native_ref).copied())
        {
            projected.insert(key, entity);
        }
    }
    let mut point_native_refs = HashMap::new();
    for point in points {
        if let Some(scope) = native_stream(&point.id) {
            point_native_refs.insert((scope, point.record_index), point.id.as_str());
        }
    }
    let mut curve_native_refs = HashMap::new();
    for curve in curves {
        if let Some(scope) = native_stream(&curve.id) {
            curve_native_refs.insert((scope, curve.record_index), curve.id.as_str());
        }
    }
    let mut text_native_refs = HashMap::new();
    for text in texts {
        if let Some(scope) = native_stream(&text.id) {
            text_native_refs.insert((scope, text.record_index), text.id.as_str());
        }
    }
    let native_operand = |scope: &str,
                          field: cadmpeg_core::text::NonBlankString,
                          record_index: u32| {
        let (family, native_ref) = if let Some(native_ref) =
            point_native_refs.get(&(scope, record_index)).copied()
        {
            (cadmpeg_core::nonblank_literal!("point"), Some(native_ref))
        } else if let Some(native_ref) = curve_native_refs.get(&(scope, record_index)).copied() {
            (cadmpeg_core::nonblank_literal!("curve"), Some(native_ref))
        } else if let Some(native_ref) = text_native_refs.get(&(scope, record_index)).copied() {
            (cadmpeg_core::nonblank_literal!("text"), Some(native_ref))
        } else {
            (cadmpeg_core::nonblank_literal!("record"), None)
        };
        SketchNativeOperand {
            native_kind: family,
            field: Some(NativeOperandField {
                name: field,
                role: None,
            }),
            object_index: Some(record_index),
            native_ref: native_ref
                .filter(|_| !projected.contains_key(&(scope, record_index)))
                .map(std::borrow::ToOwned::to_owned),
        }
    };

    let project_relation =
        |relation: &SketchRelation| -> Result<Option<SketchConstraint>, CodecError> {
            let Some(scope) = native_stream(&relation.id) else {
                return Ok(None);
            };
            let Some(sketch) = sketches.get(&(scope, relation.owner_reference)) else {
                return Ok(None);
            };
            let sketch = sketch.clone();
            let mut input_entities = Vec::new();
            for member in relation.members().iter() {
                if let Some(entity) = projected.get(&(scope, member.reference.record_index())) {
                    input_entities.push(*entity);
                }
            }
            // The second reference run is the relation's semantic member order.
            // The interleaved first run is retained separately because
            // circular-pattern decoding verifies both reference sets before using
            // the semantic order.
            let mut semantic_entities = Vec::new();
            for member in relation.return_members().iter() {
                if let Some(entity) = projected.get(&(scope, member.reference.record_index())) {
                    semantic_entities.push(*entity);
                }
            }
            let sole_kind = crate::design::relation_kinds::sole_constraint_kind(relation)
                .filter(|_| semantic_entities.len() == relation.return_members().len());
            let mut definition = if let Some(kind) = sole_kind {
                let loci = if kind == SketchConstraintKind::Coincident {
                    exact_coincident_loci(&semantic_entities, ctx)?
                } else {
                    None
                };
                match loci {
                    Some(loci) => Some(loci),
                    None => exact_atomic_constraint(kind, &semantic_entities),
                }
            } else {
                None
            };
            if definition.is_none() {
                definition =
                    exact_rectangular_pattern(relation, scope, parameters, &semantic_entities)?;
            }
            if definition.is_none() {
                definition = exact_circular_pattern(
                    relation,
                    scope,
                    parameters,
                    &input_entities,
                    &semantic_entities,
                );
            }
            let definition = if definition.is_some() {
                definition
            } else {
                exact_offset_constraint(relation, scope, &projected)
            };
            let definition = match definition {
                Some(definition) => Some(definition),
                None => exact_text_relation(relation, scope, &projected)?,
            };
            let definition = if let Some(definition) = definition {
                definition
            } else {
                let Some(native_kind) =
                    cadmpeg_core::text::NonBlankString::new(relation_kind_name(relation))
                else {
                    return Ok(None);
                };
                let member_indices = relation
                    .members()
                    .iter()
                    .map(|member| member.reference.record_index());
                let auxiliary_indices = relation.auxiliary_references().values().copied();
                let return_indices = relation
                    .return_members()
                    .iter()
                    .map(|member| member.reference.record_index());
                let mut native_entities = Vec::new();
                for record_index in member_indices
                    .chain(auxiliary_indices)
                    .chain(return_indices)
                {
                    if let Some(entity) = projected.get(&(scope, record_index)) {
                        native_entities.push(entity.id().clone());
                    }
                }
                let mut operands = Vec::new();
                for member in relation.members().iter() {
                    let operand = native_operand(
                        scope,
                        cadmpeg_core::nonblank_literal!("member"),
                        member.reference.record_index(),
                    );
                    operands.push(operand);
                }
                for record_index in relation.auxiliary_references().values() {
                    let operand = native_operand(
                        scope,
                        cadmpeg_core::nonblank_literal!("auxiliary"),
                        *record_index,
                    );
                    operands.push(operand);
                }
                for member in relation.return_members().iter() {
                    let operand = native_operand(
                        scope,
                        cadmpeg_core::nonblank_literal!("return"),
                        member.reference.record_index(),
                    );
                    operands.push(operand);
                }
                Definition::Native {
                    native_kind,
                    native_state: Some(relation.definition.state()),
                    native_flags: None,
                    native_properties: std::collections::BTreeMap::new(),
                    entities: native_entities,
                    parameter: None,
                    operands,
                }
            };
            let Some(definition) =
                cadmpeg_ir::sketches::SketchConstraintDefinition::try_from(definition).ok()
            else {
                return Ok(None);
            };
            Ok(Some(SketchConstraint {
                id: crate::ids::neutral_sketch_constraint_id(&relation.id, relation.record_index),
                sketch,
                definition,
                name: None,
                driving: None,
                active: None,
                virtual_space: None,
                visible: None,
                orientation: None,
                label_distance: None,
                label_position: None,
                metadata: None,
                native_ref: Some(relation.id.clone()),
            }))
        };
    let mut constraints = Vec::new();
    for relation in relations {
        if let Some(constraint) = project_relation(relation)? {
            constraints.push(constraint);
        }
    }
    constraints.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(constraints)
}

struct RectangularPatternSourceDirection {
    direction: [f64; 2],
    count: u32,
    distance: cadmpeg_ir::scalar::Length,
    distance_parameter: Option<cadmpeg_ir::features::ParameterId>,
    count_parameter: Option<cadmpeg_ir::features::ParameterId>,
}

#[derive(Clone, Copy)]
enum RectangularPatternDistanceForm {
    AdjacentSpacing,
    SeedToFinalSpan,
}

fn exact_rectangular_pattern(
    relation: &SketchRelation,
    scope: &str,
    parameters: &[DesignParameter],
    entities: &[&cadmpeg_ir::sketches::SketchEntity],
) -> Result<Option<cadmpeg_ir::sketches::SketchConstraintDefinitionInput>, CodecError> {
    use crate::records::sketch_relations::SketchPatternDefinition;
    use cadmpeg_ir::sketches::SketchConstraintDefinitionInput as Definition;

    if crate::design::relation_kinds::sole_constraint_kind(relation).is_none()
        || crate::design::decode::sketch::sketch_pattern_neutral_state(relation) != Some(true)
        || entities.len() != relation.return_members().len()
    {
        return Ok(None);
    }
    let distance_form =
        match crate::design::decode::sketch::rectangular_pattern_flags(relation.raw_bytes()) {
            Some([0, 0, 0]) => RectangularPatternDistanceForm::AdjacentSpacing,
            Some([1, 0, 0]) => RectangularPatternDistanceForm::SeedToFinalSpan,
            _ => return Ok(None),
        };
    let pattern = relation.definition.pattern();
    let Some(SketchPatternDefinition::Rectangular { directions }) = pattern else {
        return Ok(None);
    };
    let project_direction =
        |direction: &crate::records::sketch_relations::SketchPatternDirection| -> Option<_> {
            let source_direction = direction.direction.get();
            if source_direction[2].abs() > EPS_CONSTRAINTS_EXACT_RECTANGULAR_PATTERN_E9 {
                return None;
            }
            let count_parameter = parameters.iter().find(|parameter| {
                native_stream(&parameter.id) == Some(scope)
                    && direction
                        .count_parameter
                        .is_some_and(|record| parameter.owner_record_index() == Some(record))
            });
            let distance_parameter = parameters.iter().find(|parameter| {
                native_stream(&parameter.id) == Some(scope)
                    && direction
                        .distance_parameter
                        .is_some_and(|record| parameter.owner_record_index() == Some(record))
            });
            let count = direction.evaluated_count.get();
            if count_parameter.is_some_and(|parameter| {
                !scalar_close(parameter.evaluated_value().get(), f64::from(count))
            }) {
                return None;
            }
            let distance =
                cadmpeg_ir::scalar::Length::new(direction.evaluated_distance.get() * 10.0);
            let distance = distance?;
            if distance_parameter.is_some_and(|parameter| {
                design_length(parameter)
                    .is_none_or(|value| !scalar_close(value.get(), distance.get()))
            }) {
                return None;
            }
            Some(RectangularPatternSourceDirection {
                direction: [source_direction[0], source_direction[1]],
                count,
                distance,
                distance_parameter: distance_parameter.map(crate::ids::neutral_parameter_id),
                count_parameter: count_parameter.map(crate::ids::neutral_parameter_id),
            })
        };
    let Some(first) = project_direction(&directions[0]) else {
        return Ok(None);
    };
    let Some(second) = project_direction(&directions[1]) else {
        return Ok(None);
    };
    let source = [first, second];
    if source
        .iter()
        .any(|direction| !scalar_close(direction.direction[0].hypot(direction.direction[1]), 1.0))
    {
        return Ok(None);
    }
    let dot = source[0].direction[0] * source[1].direction[0]
        + source[0].direction[1] * source[1].direction[1];
    if dot.abs() > EPS_CONSTRAINTS_EXACT_RECTANGULAR_PATTERN_E9 {
        return Ok(None);
    }
    let Some(directions) = rectangular_pattern_directions(&source, distance_form)? else {
        return Ok(None);
    };
    let counts = source.each_ref().map(|direction| direction.count);
    let Some(rows) = exact_rectangular_pattern_instances(&directions, counts, entities) else {
        return Ok(None);
    };
    Ok(
        cadmpeg_ir::sketches::SketchRectangularPattern::new(directions, rows)
            .map(|pattern| Definition::RectangularPattern { pattern }),
    )
}

fn rectangular_pattern_directions(
    source: &[RectangularPatternSourceDirection; 2],
    distance_form: RectangularPatternDistanceForm,
) -> Result<Option<[cadmpeg_ir::sketches::SketchPatternDirection; 2]>, CodecError> {
    let make = |source: &RectangularPatternSourceDirection| -> Result<Option<_>, CodecError> {
        let spacing = match distance_form {
            RectangularPatternDistanceForm::AdjacentSpacing => source.distance.get(),
            RectangularPatternDistanceForm::SeedToFinalSpan => {
                if source.count > 1 {
                    source.distance.get() / f64::from(source.count - 1)
                } else {
                    0.0
                }
            }
        };
        if !spacing.is_finite() {
            return Ok(None);
        }
        let distance = source
            .distance_parameter
            .clone()
            .map(|parameter| match distance_form {
                RectangularPatternDistanceForm::AdjacentSpacing => {
                    cadmpeg_ir::sketches::SketchPatternDistance::Spacing { parameter }
                }
                RectangularPatternDistanceForm::SeedToFinalSpan => {
                    cadmpeg_ir::sketches::SketchPatternDistance::Span { parameter }
                }
            });
        let count_parameter = source.count_parameter.clone();
        Ok(cadmpeg_ir::sketches::SketchPatternDirection::new(
            source.direction,
            match cadmpeg_ir::scalar::Length::new(spacing) {
                Some(value) => value,
                None => return Ok(None),
            },
            distance,
            count_parameter,
        ))
    };
    let Some(first) = make(&source[0])? else {
        return Ok(None);
    };
    let Some(second) = make(&source[1])? else {
        return Ok(None);
    };
    Ok(Some([first, second]))
}

fn exact_rectangular_pattern_instances(
    directions: &[cadmpeg_ir::sketches::SketchPatternDirection; 2],
    counts: [u32; 2],
    entities: &[&cadmpeg_ir::sketches::SketchEntity],
) -> Option<Vec<Vec<cadmpeg_ir::sketches::SketchPatternInstance>>> {
    let instance_count = usize::try_from(counts[0]).ok().and_then(|first| {
        usize::try_from(counts[1])
            .ok()
            .and_then(|second| first.checked_mul(second))
    });
    let instance_count = instance_count?;
    if instance_count == 0 || !entities.len().is_multiple_of(instance_count) {
        return None;
    }
    let entity_count = entities.len() / instance_count;
    let seed = entities.get(..entity_count)?;
    if seed.is_empty() {
        return None;
    }
    let Ok(first_count) = usize::try_from(counts[0]) else {
        return None;
    };
    let mut instances = Vec::new();
    for (position, instance) in entities.chunks_exact(entity_count).enumerate() {
        let expected = [
            match u32::try_from(position % first_count) {
                Ok(value) => value,
                Err(_) => return None,
            },
            match u32::try_from(position / first_count) {
                Ok(value) => value,
                Err(_) => return None,
            },
        ];
        let mut matched = false;
        for first in 0..counts[0] {
            for second in 0..counts[1] {
                let indices = [first, second];
                let translation = Point2::new(
                    f64::from(indices[0])
                        * directions[0].spacing().get()
                        * directions[0].direction().get()[0]
                        + f64::from(indices[1])
                            * directions[1].spacing().get()
                            * directions[1].direction().get()[0],
                    f64::from(indices[0])
                        * directions[0].spacing().get()
                        * directions[0].direction().get()[1]
                        + f64::from(indices[1])
                            * directions[1].spacing().get()
                            * directions[1].direction().get()[1],
                );
                if seed.iter().zip(instance).all(|(source, result)| {
                    translated_sketch_geometry_matches(
                        &source.geometry,
                        &result.geometry,
                        translation,
                    )
                }) {
                    if matched || indices != expected {
                        return None;
                    }
                    matched = true;
                }
            }
        }
        if !matched {
            return None;
        }
        instances.push(cadmpeg_ir::sketches::SketchPatternInstance {
            entities: instance.iter().map(|entity| entity.id().clone()).collect(),
        });
    }
    Some(
        (0..first_count)
            .map(|first| {
                instances
                    .iter()
                    .skip(first)
                    .step_by(first_count)
                    .cloned()
                    .collect()
            })
            .collect(),
    )
}

fn exact_text_relation(
    relation: &SketchRelation,
    scope: &str,
    projected: &HashMap<(&str, u32), &cadmpeg_ir::sketches::SketchEntity>,
) -> Result<Option<cadmpeg_ir::sketches::SketchConstraintDefinitionInput>, CodecError> {
    use crate::records::sketch_relations::SketchPatternDefinition;
    use cadmpeg_ir::sketches::{
        SketchConstraintDefinitionInput as Definition, SketchGeometryDefinition,
    };
    use cadmpeg_ir::transform::Transform;

    if crate::design::relation_kinds::sole_constraint_kind(relation).is_none() {
        return Ok(None);
    }
    let pattern = relation.definition.pattern();
    Ok(match pattern {
        Some(SketchPatternDefinition::TextFrame { text_reference })
            if relation
                .members()
                .first()
                .map(|member| member.reference.record_index())
                == Some(*text_reference)
                && relation
                    .auxiliary_references()
                    .values()
                    .copied()
                    .eq([*text_reference])
                && relation
                    .return_members()
                    .iter()
                    .map(|member| member.reference.record_index())
                    .eq(relation
                        .members()
                        .iter()
                        .skip(1)
                        .map(|member| member.reference.record_index())) =>
        {
            let Some(text) = projected.get(&(scope, *text_reference)) else {
                return Ok(None);
            };
            if !matches!(
                *text.geometry.definition(),
                SketchGeometryDefinition::Text { .. }
            ) {
                return Ok(None);
            }
            let mut frame = Vec::new();
            for member in relation.return_members().iter() {
                let Some(entity) = projected.get(&(scope, member.reference.record_index())) else {
                    return Ok(None);
                };
                if entity.id() == text.id()
                    || matches!(
                        *entity.geometry.definition(),
                        SketchGeometryDefinition::Text { .. }
                    )
                {
                    return Ok(None);
                }
                frame.push(entity.id().clone());
            }
            if frame.is_empty() {
                return Ok(None);
            }
            Some(Definition::TextFrame {
                text: text.id().clone(),
                frame,
            })
        }
        Some(SketchPatternDefinition::TextPath {
            text_reference,
            glyph_transforms,
        }) if relation.members().len() == 2
            && relation.members()[1].reference.record_index() == *text_reference
            && relation
                .auxiliary_references()
                .values()
                .copied()
                .eq([*text_reference])
            && relation
                .return_members()
                .iter()
                .map(|member| member.reference.record_index())
                .eq([relation.members()[0].reference.record_index()]) =>
        {
            let Some(path) =
                projected.get(&(scope, relation.members()[0].reference.record_index()))
            else {
                return Ok(None);
            };
            let Some(text) = projected.get(&(scope, *text_reference)) else {
                return Ok(None);
            };
            if path.id() == text.id()
                || matches!(
                    *path.geometry.definition(),
                    SketchGeometryDefinition::Point { .. } | SketchGeometryDefinition::Text { .. }
                )
                || !matches!(
                    *text.geometry.definition(),
                    SketchGeometryDefinition::Text { .. }
                )
                || glyph_transforms.is_empty()
            {
                return Ok(None);
            }
            let mut glyphs = Vec::new();
            for source in glyph_transforms {
                let source = source.rows();
                if source[3] != [0.0, 0.0, 0.0, 1.0] {
                    return Ok(None);
                }
                let mut rows = [source[0], source[1], source[2]];
                for row in &mut rows {
                    row[3] *= 10.0;
                }
                let Some(glyph) = Transform::affine(rows) else {
                    return Ok(None);
                };
                glyphs.push(glyph);
            }
            Some(Definition::TextPath {
                text: text.id().clone(),
                path: path.id().clone(),
                glyph_transforms: glyphs,
            })
        }
        _ => None,
    })
}

fn exact_circular_pattern(
    relation: &SketchRelation,
    scope: &str,
    parameters: &[DesignParameter],
    members: &[&cadmpeg_ir::sketches::SketchEntity],
    returned: &[&cadmpeg_ir::sketches::SketchEntity],
) -> Option<cadmpeg_ir::sketches::SketchConstraintDefinitionInput> {
    use crate::records::sketch_relations::SketchPatternDefinition;
    use cadmpeg_ir::sketches::{
        SketchCircularPattern, SketchCircularPatternInstance,
        SketchConstraintDefinitionInput as Definition, SketchGeometryDefinition,
    };

    if crate::design::relation_kinds::sole_constraint_kind(relation).is_none()
        || crate::design::decode::sketch::sketch_pattern_neutral_state(relation) != Some(true)
        || members.len() != relation.members().len()
        || returned.len() != relation.return_members().len()
    {
        return None;
    }
    let pattern = relation.definition.pattern();
    let Some(SketchPatternDefinition::Circular {
        angle_parameter,
        count_parameter,
        evaluated_angle,
        evaluated_count,
    }) = pattern
    else {
        return None;
    };
    let angle_parameter = parameters.iter().find(|parameter| {
        native_stream(&parameter.id) == Some(scope)
            && angle_parameter.is_some_and(|record| parameter.owner_record_index() == Some(record))
    });
    let count_parameter = parameters.iter().find(|parameter| {
        native_stream(&parameter.id) == Some(scope)
            && count_parameter.is_some_and(|record| parameter.owner_record_index() == Some(record))
    });
    let angle = cadmpeg_ir::scalar::Angle::from_assigned_real(*evaluated_angle);
    if angle_parameter.is_some_and(|parameter| {
        design_angle(parameter).is_none_or(|value| !scalar_close(value.get(), angle.get()))
    }) || count_parameter.is_some_and(|parameter| {
        !scalar_close(
            parameter.evaluated_value().get(),
            f64::from(evaluated_count.get()),
        )
    }) {
        return None;
    }
    // Relation ordinals do not classify center/seed/generated; partition by
    // geometry when the member and returned id sets match.
    let mut member_ids = HashSet::new();
    for entity in members {
        member_ids.insert(entity.id());
    }
    let mut returned_ids = HashSet::new();
    for entity in returned {
        returned_ids.insert(entity.id());
    }
    if member_ids.len() != members.len()
        || returned_ids.len() != returned.len()
        || member_ids != returned_ids
    {
        return None;
    }
    let mut candidates = Vec::new();
    for center in members.iter().copied() {
        let SketchGeometryDefinition::Point {
            position: center_position,
        } = *center.geometry.definition()
        else {
            continue;
        };
        let center_position = center_position.get();
        let mut patterned = Vec::new();
        for entity in returned
            .iter()
            .copied()
            .filter(|entity| entity.id() != center.id())
        {
            patterned.push(entity);
        }
        let Ok(count) = usize::try_from(evaluated_count.get()) else {
            return None;
        };
        if patterned.is_empty() || !patterned.len().is_multiple_of(count) {
            continue;
        }
        let arity = patterned.len() / count;
        let seed = &patterned[..arity];
        if seed.is_empty() {
            continue;
        }
        let first_divisor = f64::from(evaluated_count.get());
        let second_divisor = (evaluated_count.get() > 1)
            .then(|| f64::from(evaluated_count.get() - 1))
            .filter(|second| !scalar_close(first_divisor, *second));
        for divisor in [Some(first_divisor), second_divisor].into_iter().flatten() {
            // The seed is the first chunk; it is not an instance, and the
            // instances carry only their nonzero rotations.
            let mut instances = Vec::new();
            let mut valid = true;
            for (index, instance) in patterned.chunks_exact(arity).enumerate().skip(1) {
                let index = f64_from_index(index)?;
                let rotation = evaluated_angle.get() * index / divisor;
                if !seed.iter().zip(instance).all(|(source, result)| {
                    rotated_sketch_geometry_matches(
                        &source.geometry,
                        &result.geometry,
                        center_position,
                        rotation,
                    )
                }) {
                    valid = false;
                    break;
                }
                let Some(angle) = cadmpeg_ir::scalar::NonZeroAngle::new(rotation) else {
                    valid = false;
                    break;
                };
                let entity_ids: Vec<_> =
                    instance.iter().map(|entity| entity.id().clone()).collect();
                instances.push(SketchCircularPatternInstance {
                    angle,
                    entities: entity_ids,
                });
            }
            if valid {
                let seed_entities: Vec<_> = seed.iter().map(|entity| entity.id().clone()).collect();
                let center = center.id().clone();
                candidates.push((center, seed_entities, instances));
            }
        }
    }
    candidates.dedup();
    if candidates.len() != 1 {
        return None;
    }
    let (center, seed_entities, instances) = candidates.pop()?;
    let pattern = SketchCircularPattern::new(
        center,
        angle,
        angle_parameter.map(crate::ids::neutral_parameter_id),
        count_parameter.map(crate::ids::neutral_parameter_id),
        seed_entities,
        instances,
    );
    pattern.map(|pattern| Definition::CircularPattern { pattern })
}

fn rotated_sketch_geometry_matches(
    source: &cadmpeg_ir::sketches::SketchGeometry,
    result: &cadmpeg_ir::sketches::SketchGeometry,
    center: Point2,
    angle: f64,
) -> bool {
    use cadmpeg_ir::sketches::SketchGeometryDefinition;

    let rotate = |point: Point2| {
        let (sin, cos) = angle.sin_cos();
        let u = point.u - center.u;
        let v = point.v - center.v;
        Point2::new(center.u + cos * u - sin * v, center.v + sin * u + cos * v)
    };
    let point_matches = |first: Point2, second: Point2| {
        let first = rotate(first);
        scalar_close(first.u, second.u) && scalar_close(first.v, second.v)
    };
    let angle_matches = |first: f64, second: f64| {
        let delta = (first + angle - second).rem_euclid(std::f64::consts::TAU);
        scalar_close(delta, 0.0) || scalar_close(delta, std::f64::consts::TAU)
    };
    match (source.definition(), result.definition()) {
        (
            SketchGeometryDefinition::Point { position: first },
            SketchGeometryDefinition::Point { position: second },
        ) => point_matches(first.get(), second.get()),
        (
            SketchGeometryDefinition::Line { start: a, end: b },
            SketchGeometryDefinition::Line { start: c, end: d },
        ) => point_matches(a.get(), c.get()) && point_matches(b.get(), d.get()),
        (
            SketchGeometryDefinition::Circle {
                center: a,
                radius: ar,
            },
            SketchGeometryDefinition::Circle {
                center: b,
                radius: br,
            },
        ) => point_matches(a.get(), b.get()) && scalar_close(ar.get(), br.get()),
        (
            SketchGeometryDefinition::Arc {
                center: a,
                radius: ar,
                start_angle: as_,
                end_angle: ae,
            },
            SketchGeometryDefinition::Arc {
                center: b,
                radius: br,
                start_angle: bs,
                end_angle: be,
            },
        ) => {
            point_matches(a.get(), b.get())
                && scalar_close(ar.get(), br.get())
                && angle_matches(as_.get(), bs.get())
                && angle_matches(ae.get(), be.get())
        }
        (
            SketchGeometryDefinition::Ellipse {
                center: a,
                major_angle: aa,
                radii: ar,
                bounds: ab,
            },
            SketchGeometryDefinition::Ellipse {
                center: b,
                major_angle: ba,
                radii: br,
                bounds: bb,
            },
        ) => {
            point_matches(a.get(), b.get())
                && angle_matches(aa.get(), ba.get())
                && scalar_close(ar.major().get(), br.major().get())
                && scalar_close(ar.minor().get(), br.minor().get())
                && optional_angle_bounds_match(ab.as_ref(), bb.as_ref())
        }
        (
            SketchGeometryDefinition::Nurbs { curve: first },
            SketchGeometryDefinition::Nurbs { curve: second },
        ) => {
            first.degree() == second.degree()
                && first.periodic() == second.periodic()
                && equal_scalars(first.knots(), second.knots())
                && nurbs_poles_match(first, second, point_matches)
        }
        _ => false,
    }
}

pub(super) fn scalar_close(first: f64, second: f64) -> bool {
    first.is_finite()
        && second.is_finite()
        && (first - second).abs()
            <= EPS_CONSTRAINTS_SCALAR_CLOSE_E9 * (1.0 + first.abs().max(second.abs()))
}

fn translated_sketch_geometry_matches(
    source: &cadmpeg_ir::sketches::SketchGeometry,
    result: &cadmpeg_ir::sketches::SketchGeometry,
    translation: Point2,
) -> bool {
    use cadmpeg_ir::sketches::SketchGeometryDefinition;

    let point_matches = |first: Point2, second: Point2| {
        scalar_close(first.u + translation.u, second.u)
            && scalar_close(first.v + translation.v, second.v)
    };
    match (source.definition(), result.definition()) {
        (
            SketchGeometryDefinition::Point { position: first },
            SketchGeometryDefinition::Point { position: second },
        ) => point_matches(first.get(), second.get()),
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
            point_matches(first_start.get(), second_start.get())
                && point_matches(first_end.get(), second_end.get())
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
            point_matches(first_center.get(), second_center.get())
                && scalar_close(first_radius.get(), second_radius.get())
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
            point_matches(first_center.get(), second_center.get())
                && scalar_close(first_radius.get(), second_radius.get())
                && scalar_close(first_start.get(), second_start.get())
                && scalar_close(first_end.get(), second_end.get())
        }
        (
            SketchGeometryDefinition::Ellipse {
                center: first_center,
                major_angle: first_major_angle,
                radii: first_radii,
                bounds: first_bounds,
            },
            SketchGeometryDefinition::Ellipse {
                center: second_center,
                major_angle: second_major_angle,
                radii: second_radii,
                bounds: second_bounds,
            },
        ) => {
            point_matches(first_center.get(), second_center.get())
                && scalar_close(first_major_angle.get(), second_major_angle.get())
                && scalar_close(first_radii.major().get(), second_radii.major().get())
                && scalar_close(first_radii.minor().get(), second_radii.minor().get())
                && optional_angle_bounds_match(first_bounds.as_ref(), second_bounds.as_ref())
        }
        (
            SketchGeometryDefinition::Nurbs { curve: first },
            SketchGeometryDefinition::Nurbs { curve: second },
        ) => {
            first.degree() == second.degree()
                && first.periodic() == second.periodic()
                && equal_scalars(first.knots(), second.knots())
                && nurbs_poles_match(first, second, point_matches)
        }
        _ => false,
    }
}

fn optional_angle_bounds_match(
    first: Option<&[cadmpeg_ir::scalar::Angle; 2]>,
    second: Option<&[cadmpeg_ir::scalar::Angle; 2]>,
) -> bool {
    match (first, second) {
        (None, None) => true,
        (Some(first), Some(second)) => {
            scalar_close(first[0].get(), second[0].get())
                && scalar_close(first[1].get(), second[1].get())
        }
        _ => false,
    }
}

fn equal_scalars(first: &[f64], second: &[f64]) -> bool {
    first.len() == second.len()
        && first
            .iter()
            .zip(second)
            .all(|(first, second)| scalar_close(*first, *second))
}

fn nurbs_poles_match(
    first: &PcurveNurbs,
    second: &PcurveNurbs,
    point_matches: impl Fn(Point2, Point2) -> bool,
) -> bool {
    match (first.pole_rows(), second.pole_rows()) {
        (
            PcurveNurbsPoles::Polynomial { points: first },
            PcurveNurbsPoles::Polynomial { points: second },
        ) => {
            first.len() == second.len()
                && first
                    .iter()
                    .zip(second)
                    .all(|(a, b)| point_matches(a.get(), b.get()))
        }
        (
            PcurveNurbsPoles::Rational { points: first },
            PcurveNurbsPoles::Rational { points: second },
        ) => {
            first.len() == second.len()
                && first.iter().zip(second).all(|(a, b)| {
                    point_matches(a.point.get(), b.point.get())
                        && scalar_close(a.weight.get(), b.weight.get())
                })
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests;
