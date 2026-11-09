// SPDX-License-Identifier: Apache-2.0
//! Bind a new-body Pipe whose result topology fits a circular section.

use std::collections::{HashMap, HashSet};

use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureOperation};
use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, Surface};
use cadmpeg_ir::ids::SurfaceId;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::topology::BodyKind;

use crate::ids::native_stream;
use crate::loss::F3dLossCode;
use crate::records::feature::extrude::DesignExtrudeOperation;
use crate::records::feature::path_features::DesignPipeConstruction;
use crate::records::feature::scope::{DesignParameterScope, DesignScopePayload};
use crate::records::parameters::DesignParameterOwner;
use crate::records::topology::construction::DesignConstructionOperandGroup;
use crate::records::topology::extrude_selection::DesignOperandRole;

/// Native records and saved topology that bind a Pipe's section.
pub(crate) struct PipeSectionInputs<'a> {
    pub(crate) scopes: &'a [DesignParameterScope],
    pub(crate) groups: &'a [DesignConstructionOperandGroup],
    pub(crate) owners: &'a [DesignParameterOwner],
    pub(crate) bodies: &'a [cadmpeg_ir::topology::Body],
    pub(crate) regions: &'a [cadmpeg_ir::topology::Region],
    pub(crate) faces: &'a [cadmpeg_ir::topology::Face],
    pub(crate) surfaces: &'a [Surface],
    pub(crate) linear_tolerance: f64,
}

/// Project each native new-body Pipe whose unchanged result faces fit a
/// circular section, and whose fractions both equal one, as a Sweep of that
/// circular region along its whole path group.
#[must_use]
pub(crate) fn bind_circular_pipe_sections(
    features: &mut [Feature],
    inputs: &PipeSectionInputs<'_>,
) -> Vec<LossNote> {
    let mut surfaces = HashMap::<&SurfaceId, Option<&Surface>>::new();
    for surface in inputs.surfaces {
        surfaces
            .entry(&surface.id)
            .and_modify(|slot| *slot = None)
            .or_insert(Some(surface));
    }
    let definitions = features
        .iter()
        .map(|feature| circular_pipe_sweep(feature, features, inputs, &surfaces))
        .collect::<Vec<_>>();
    let mut losses = Vec::new();
    for (feature, definition) in features.iter_mut().zip(definitions) {
        if let Some(definition) = definition {
            feature.evaluation.set_definition(definition);
            losses.push(F3dLossCode::FeaturePipeCircularSectionInferred.note(format!(
                "feature {}: the Pipe section is inferred as a circle from the planes, cylinders and tori of its result bodies",
                feature.id
            )));
            losses.push(F3dLossCode::FeaturePipeWholePathExtentAssumed.note(format!(
                "feature {}: the Pipe forward and reverse fractions are 1, and the sweep is assumed to cover its whole path group",
                feature.id
            )));
        }
    }
    losses
}

fn only<T>(mut matches: impl Iterator<Item = T>) -> Option<T> {
    let first = matches.next()?;
    matches.next().is_none().then_some(first)
}

fn circular_pipe_sweep(
    feature: &Feature,
    features: &[Feature],
    inputs: &PipeSectionInputs<'_>,
    surfaces: &HashMap<&SurfaceId, Option<&Surface>>,
) -> Option<FeatureDefinition> {
    use cadmpeg_ir::features::{
        GeneratedSweepSection, PathRef, SolidSweepOperation, SweepCircularRegion, SweepPathExtent,
        SweepSection, SweepShape,
    };
    use cadmpeg_ir::scalar::{Fraction, PositiveLength};

    let FeatureDefinition::Operation(FeatureOperation::Native { kind, .. }) =
        feature.evaluation.definition()
    else {
        return None;
    };
    if kind.as_str() != "Pipe" {
        return None;
    }
    let native_ref = feature.native_ref.as_deref()?;
    let scope = only(inputs.scopes.iter().filter(|scope| scope.id == native_ref))?;
    let DesignScopePayload::Pipe(Some(construction)) = scope.payload() else {
        return None;
    };
    if construction.operation != DesignExtrudeOperation::NewBody {
        return None;
    }
    let [forward, reverse, size, thickness] = pipe_values(scope, construction, inputs.owners)?;
    if forward != 1.0 || reverse != 1.0 {
        return None;
    }
    let stream = native_stream(&scope.id)?;
    let path = only(inputs.groups.iter().filter(|group| {
        native_stream(&group.id) == Some(stream)
            && group.scope_record_index == scope.record_index
            && group.role() == DesignOperandRole::ROLE_0X5
            && !group.members().is_empty()
    }))?;
    let outer_radius = size * 10.0 / 2.0;
    let wall = construction.is_hollow.then_some(thickness * 10.0);
    let inner_radius = wall.map(|wall| outer_radius - wall);
    if !result_bodies_unchanged(feature, features)
        || !result_faces_fit_circle(feature, inputs, surfaces, outer_radius, inner_radius)
    {
        return None;
    }
    let region = SweepCircularRegion::new(
        PositiveLength::new(outer_radius)?,
        match wall {
            Some(wall) => Some(PositiveLength::new(wall)?),
            None => None,
        },
    )
    .ok()?;
    Some(FeatureDefinition::Operation(FeatureOperation::Sweep {
        shape: SweepShape::Solid {
            op: SolidSweepOperation::NewBody,
            section: SweepSection::Generated(GeneratedSweepSection::CircularRegion { region }),
            sections: Vec::new(),
        },
        path: Some(PathRef::Native(path.id.clone())),
        orientation: None,
        transition: None,
        transformation: None,
        path_tangent: false,
        linearize: false,
        twist: None,
        path_extent: Some(SweepPathExtent {
            along_fraction: Fraction::new(forward)?,
            against_fraction: Fraction::new(reverse)?,
        }),
        guide_rail: None,
        taper: None,
        scale: None,
        allow_multi_profile_faces: None,
    }))
}

/// The forward fraction, reverse fraction, Section Size and Section
/// Thickness of a Pipe, from its four unique scalar owners.
fn pipe_values(
    scope: &DesignParameterScope,
    construction: &DesignPipeConstruction,
    owners: &[DesignParameterOwner],
) -> Option<[f64; 4]> {
    let stream = native_stream(&scope.id)?;
    let mut values = [0.0; 4];
    for (value, record_index) in values.iter_mut().zip(construction.record_indexes) {
        let owner = only(owners.iter().filter(|owner| {
            native_stream(owner.id()) == Some(stream) && owner.record_index() == record_index
        }))?;
        *value = owner.evaluated_value().get();
    }
    Some(values)
}

/// Whether no later feature names one of the feature's result bodies, and
/// every later feature that can change bodies names its own results.
fn result_bodies_unchanged(feature: &Feature, features: &[Feature]) -> bool {
    let outputs = feature.evaluation.outputs();
    features
        .iter()
        .filter(|later| later.ordinal > feature.ordinal)
        .all(|later| {
            let later_outputs = later.evaluation.outputs();
            let changes_bodies = later.evaluation.definition().body_output_family().is_some()
                || matches!(
                    later.evaluation.definition(),
                    FeatureDefinition::Operation(FeatureOperation::Native { .. })
                );
            let shares_body = later_outputs.iter().any(|body| outputs.contains(body));
            !(shares_body || changes_bodies && later_outputs.is_empty())
        })
}

/// Whether the feature's result bodies are solids whose every face lies on a
/// plane, or on a cylinder or torus whose radius is the outer or, when hollow,
/// the inner section radius, with at least one curved face of each radius.
fn result_faces_fit_circle(
    feature: &Feature,
    inputs: &PipeSectionInputs<'_>,
    surfaces: &HashMap<&SurfaceId, Option<&Surface>>,
    outer_radius: f64,
    inner_radius: Option<f64>,
) -> bool {
    let outputs = feature.evaluation.outputs();
    if outputs.is_empty() {
        return false;
    }
    let mut shells = HashSet::new();
    for output in outputs {
        let Some(body) = only(inputs.bodies.iter().filter(|body| body.id == *output)) else {
            return false;
        };
        if body.kind != BodyKind::Solid {
            return false;
        }
        for region_id in &body.regions {
            let Some(region) = only(
                inputs
                    .regions
                    .iter()
                    .filter(|region| region.id == *region_id),
            ) else {
                return false;
            };
            shells.extend(region.shells.iter());
        }
    }
    let matches = |radius: f64, target: f64| (radius - target).abs() <= inputs.linear_tolerance;
    let (mut outer_seen, mut inner_seen) = (false, false);
    for face in inputs
        .faces
        .iter()
        .filter(|face| shells.contains(&face.shell))
    {
        let Some(Some(surface)) = surfaces.get(&face.surface) else {
            return false;
        };
        let radius = match surface.geometry.solved() {
            Some(SolvedSurfaceGeometry::Plane(_)) => continue,
            Some(SolvedSurfaceGeometry::Cylinder(cylinder)) => cylinder.radius().get(),
            Some(SolvedSurfaceGeometry::Torus(torus)) => torus.minor_radius().get().abs(),
            _ => return false,
        };
        if matches(radius, outer_radius) {
            outer_seen = true;
        } else if inner_radius.is_some_and(|inner| matches(radius, inner)) {
            inner_seen = true;
        } else {
            return false;
        }
    }
    outer_seen && (inner_radius.is_none() || inner_seen)
}

#[cfg(test)]
mod tests;
