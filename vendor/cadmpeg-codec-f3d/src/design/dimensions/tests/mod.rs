// SPDX-License-Identifier: Apache-2.0
#![allow(
    clippy::cloned_ref_to_slice_refs,
    clippy::default_trait_access,
    clippy::trivially_copy_pass_by_ref,
    clippy::uninlined_format_args
)]

mod linear;
mod offset;
mod offset_binding;
mod owner_scoped;
mod recipe;
mod relations;

const EPS_DIMENSION_PROJECTION_LINEAR: f64 = 1.0e-6;

fn project_dimension_constraints(
    inputs: &crate::design::dimensions::DimensionConstraintInputs<'_>,
    spatial_sketches: &[cadmpeg_ir::sketches::SpatialSketch],
) -> Vec<cadmpeg_ir::sketches::SketchConstraint> {
    crate::design::dimensions::project_dimension_constraints(
        inputs,
        spatial_sketches,
        EPS_DIMENSION_PROJECTION_LINEAR,
    )
}

fn project_spatial_dimension_constraints(
    inputs: &crate::design::dimensions::DimensionConstraintInputs<'_>,
    spatial_sketches: &[cadmpeg_ir::sketches::SpatialSketch],
    spatial_entities: &[cadmpeg_ir::sketches::SpatialSketchEntity],
) -> Vec<cadmpeg_ir::sketches::SpatialSketchConstraint> {
    crate::design::dimensions::project_spatial_dimension_constraints(
        inputs,
        spatial_sketches,
        spatial_entities,
        EPS_DIMENSION_PROJECTION_LINEAR,
    )
}

mod numerical_ranges;
mod parameter_iteration;

mod relation_kinds;

#[test]
fn planar_dimension_output_keeps_unplaced_constraints() {
    let constraint = cadmpeg_ir::sketches::SketchConstraint {
        id: cadmpeg_ir::sketches::SketchConstraintId::mint(
            "synthetic:test:id#dimension-limit-constraint",
        )
        .unwrap(),
        sketch: cadmpeg_ir::sketches::SketchId::mint("synthetic:test:id#dimension-limit-sketch")
            .unwrap(),
        definition: cadmpeg_ir::sketches::SketchConstraintDefinition::try_from(
            cadmpeg_ir::sketches::SketchConstraintDefinitionInput::Disabled {},
        )
        .unwrap(),
        name: None,
        driving: None,
        active: None,
        virtual_space: None,
        visible: None,
        orientation: None,
        label_distance: None,
        label_position: None,
        metadata: None,
        native_ref: None,
    };
    let output = crate::design::dimensions::retain_planar_dimension_constraints(
        &[],
        &[],
        vec![constraint.clone()],
    );
    assert_eq!(output, [constraint]);
}
