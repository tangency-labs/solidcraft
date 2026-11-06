// SPDX-License-Identifier: Apache-2.0
use super::{
    neutral_sketch_id, ConstructionRecipeKind, DesignDimensionRecipeRecord,
    DesignParameterCompanion, DesignSketchPlacement, Point2, SketchConstraintDefinitionInput,
    SketchCurveIdentity, SketchEntity, SketchEntityId, SketchGeometry, SketchGeometryDefinition,
};
use crate::design::dimensions::project_dimension_constraints_with_presentations;
use crate::records::dimensions::{
    DesignDimensionPresentationFrame, DesignDimensionPresentationOperand,
};

const STREAM: &str = "f3d:A";

fn class(tag: &str) -> crate::records::references::DesignClassTag {
    crate::records::references::DesignClassTag::try_from(tag.to_owned()).unwrap()
}

fn curve(record_index: u32) -> SketchCurveIdentity {
    SketchCurveIdentity {
        id: format!("{STREAM}:sketch-curve#{record_index}"),
        record_index,
        owner_reference: Some(100),
        class_tag: class("300"),
        byte_offset: 0,
        geometry_offset: 0,
        entity_genesis: None,
        primary_id: std::num::NonZeroU64::new(u64::from(record_index)).unwrap(),
        secondary_id: 0,
        geometry: None,
        planar_z_noise: None,
    }
}

fn frame(operands: [u32; 2]) -> DesignDimensionPresentationFrame {
    DesignDimensionPresentationFrame {
        id: format!("{STREAM}:design-dimension-presentation-frame#90"),
        byte_offset: 0,
        class_tag: class("289"),
        record_index: 90,
        frame_length: 0,
        operands: operands
            .into_iter()
            .map(|record_index| DesignDimensionPresentationOperand {
                geometry_record_index: std::num::NonZeroU32::new(record_index).unwrap(),
                geometry_reference_offset: 0,
                role: 0,
                role_offset: 0,
            })
            .collect(),
        presentation_bytes: Vec::new().into(),
        presentation_byte_offset: 0,
        paired_class_tag: class("291"),
        paired_byte_offset: 0,
        owner_reference: 100,
        owner_reference_offset: 0,
        governing_owner_record_index: 21,
        governing_parameter_record_index: 20,
        governing_companion_record_index: 22,
    }
}

#[test]
fn measuring_presentation_frame_precedes_recipe_candidates() {
    let placement = DesignSketchPlacement {
        frame: crate::records::sketch_placement::DesignSketchFrame::new(
            0,
            crate::records::sketch_placement::DesignSketchFrameForm::ScopeCompact,
        )
        .unwrap(),
        id: format!("{STREAM}:design-sketch-placement#0"),
        scope_record_index: Some(10),
        entity_id: crate::records::identity::DesignEntityId::try_from("0_100".to_owned())
            .expect("valid entity ID"),
        visibility: None,
        class_tag: class("356"),
        record_index: 11,
        paired_class_tag: class("259"),
    };
    let parameter = crate::records::parameters::DesignParameter::try_from(
        crate::records::parameters::DesignParameterDraft {
            id: format!("{STREAM}:design-parameter#20"),
            byte_offset: 0,
            class_tag: class("305"),
            record_index: 20,
            source_ordinal: 4,
            source: crate::records::parameters::DesignParameterSource::new(
                "Linear Dimension-4".into(),
                Some(21),
                Some(crate::records::identity::Located {
                    value: crate::records::parameters::DesignParameterDiscriminator::Code0,
                    offset: 22,
                }),
            )
            .unwrap(),
            expression: "2 mm".into(),
            expression_offset: 40,
            source_kind_offset: 60,
            unit: Some(crate::records::identity::RecordedValue {
                value: "mm".into(),
                offset: 70,
            }),
            name: "d4".into(),
            name_offset: 80,
            evaluated_value: 0.2,
            evaluated_value_offset: 90,
        },
    )
    .unwrap();
    let owner = crate::records::parameters::DesignParameterOwner::try_from(
        crate::records::parameters::DesignParameterOwnerWire {
            id: format!("{STREAM}:design-parameter-owner#21"),
            byte_offset: 0,
            frame_length: 104,
            class_tag: class("292"),
            record_index: 21,
            scope_record_index: 10,
            local_ordinal: 0,
            evaluated_value: 0.2,
            evaluated_value_offset: 40,
            parameter_record_index: 20,
            owned_ordinal: 0,
            variant: Some(0),
            companion_record_index: 22,
        },
    )
    .unwrap();
    let companion = DesignParameterCompanion::unbound(
        format!("{STREAM}:design-parameter-companion#22"),
        0,
        class("408"),
        22,
        21,
        std::num::NonZeroU64::new(1).unwrap(),
        0,
    )
    .bound(crate::records::parameters::DesignCompanionPayload::new(
        58,
        200,
        Vec::new(),
    ));
    let recipe = DesignDimensionRecipeRecord {
        id: format!("{STREAM}:design-dimension-recipe-record#30"),
        companion_record_index: 22,
        recipe_ordinal: 0,
        recipe_id: format!("{STREAM}:construction-recipe#30"),
        recipe_kind: ConstructionRecipeKind::Edge,
        byte_offset: 0,
        class_tag: class("423"),
        record_index: 30,
        frame_length: 10,
        prefix_offset: 0,
        prefix_bytes: Vec::new().into(),
        references: Vec::new(),
        program_offset: 0,
        program: vec![-1],
        matching_edge_operand_ids: Vec::new(),
    };
    let sketch = neutral_sketch_id(&placement);
    let line = |record_index: u32, start, end| {
        SketchEntity::new(
            SketchEntityId::mint(format!("synthetic:test:id#line-{record_index}")).unwrap(),
            sketch.clone(),
            SketchGeometry::try_from(SketchGeometryDefinition::Line { start, end }).unwrap(),
        )
        .with_native_ref(Some(curve(record_index).id))
    };
    let entities = [
        line(41, Point2::new(0.0, 0.0), Point2::new(4.0, 0.0)),
        line(42, Point2::new(0.0, 2.0), Point2::new(4.0, 2.0)),
        line(43, Point2::new(10.0, 0.0), Point2::new(10.0, 4.0)),
        line(44, Point2::new(12.0, 0.0), Point2::new(12.0, 4.0)),
    ];
    let curves = [41, 42, 43, 44].map(curve);
    let project = |frames: &[DesignDimensionPresentationFrame]| {
        project_dimension_constraints_with_presentations(
            &crate::design::dimensions::DimensionConstraintInputs {
                placements: std::slice::from_ref(&placement),
                parameters: std::slice::from_ref(&parameter),
                owners: std::slice::from_ref(&owner),
                pairs: &[],
                groups: &[],
                annotation_frames: &[],
                null_pairs: &[],
                companions: std::slice::from_ref(&companion),
                recipe_records: std::slice::from_ref(&recipe),
                points: &[],
                curves: &curves,
                entities: &entities,
            },
            frames,
            &[],
            1.0e-6,
        )
    };
    let definition = |frames: &[DesignDimensionPresentationFrame]| {
        let constraints = project(frames);
        let [constraint] = constraints.as_slice() else {
            panic!("expected one recipe-backed dimension")
        };
        constraint.definition.kind().clone()
    };

    assert!(matches!(
        definition(&[]),
        SketchConstraintDefinitionInput::RepeatedDistance { .. }
    ));
    assert!(matches!(
        definition(&[frame([43, 44])]),
        SketchConstraintDefinitionInput::Distance { entities: selected, .. }
            if selected == [entities[2].id().clone(), entities[3].id().clone()]
    ));
    assert!(matches!(
        definition(&[frame([41, 43])]),
        SketchConstraintDefinitionInput::RepeatedDistance { .. }
    ));
}
