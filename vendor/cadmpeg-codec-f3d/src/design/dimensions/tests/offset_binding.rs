// SPDX-License-Identifier: Apache-2.0

use crate::design::dimensions::bind_offset_dimension_parameters;
use crate::ids::neutral_parameter_id;
use crate::records::parameters::DesignParameter;
use cadmpeg_ir::sketches::{
    SketchConstraint, SketchConstraintDefinition, SketchConstraintDefinitionInput,
    SketchNativeOperand, SketchOffsetPair,
};

fn offset_binding_fixture() -> (Vec<SketchConstraint>, Vec<DesignParameter>) {
    let parameter = DesignParameter::try_from(crate::records::parameters::DesignParameterDraft {
        id: "f3d:test:parameter#1".into(),
        byte_offset: 0,
        class_tag: "277".to_owned().try_into().unwrap(),
        record_index: 1,
        source_ordinal: 1,
        source: crate::records::parameters::DesignParameterSource::new(
            "Linear Dimension-4".into(),
            Some(0),
            None,
        )
        .unwrap(),
        expression: "0.5".into(),
        expression_offset: 40,
        source_kind_offset: 60,
        unit: Some(crate::records::identity::RecordedValue {
            value: "cm".into(),
            offset: 70,
        }),
        name: "Distance".into(),
        name_offset: 80,
        evaluated_value: 0.5,
        evaluated_value_offset: 90,
    })
    .expect("generated parameter");
    let parameter_id = neutral_parameter_id(&parameter);
    let sketch = cadmpeg_ir::sketches::SketchId::mint("test:model:sketch#1").unwrap();
    let source = cadmpeg_ir::sketches::SketchEntityId::mint("test:model:entity#source").unwrap();
    let result = cadmpeg_ir::sketches::SketchEntityId::mint("test:model:entity#result").unwrap();
    let operand = |kind| SketchNativeOperand {
        native_kind: cadmpeg_core::text::NonBlankString::new(kind).unwrap(),
        field: None,
        object_index: None,
        native_ref: None,
    };
    let native = SketchConstraintDefinitionInput::Native {
        native_kind: cadmpeg_core::text::NonBlankString::new("Linear Dimension-4").unwrap(),
        native_state: None,
        native_flags: None,
        native_properties: std::collections::BTreeMap::new(),
        entities: vec![source.clone()],
        parameter: Some(parameter_id),
        operands: vec![operand("null_locus"), operand("curve")],
    };
    let offset = SketchConstraintDefinitionInput::Offset {
        pairs: vec![SketchOffsetPair {
            source,
            result,
            source_reversed: false,
        }],
        distance: cadmpeg_ir::scalar::Length::new(5.0).unwrap(),
        parameter: None,
    };
    let constraint = |ordinal, definition| SketchConstraint {
        id: cadmpeg_ir::sketches::SketchConstraintId::mint(format!(
            "test:model:constraint#{ordinal}"
        ))
        .unwrap(),
        sketch: sketch.clone(),
        definition: SketchConstraintDefinition::try_from(definition).unwrap(),
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
    (
        vec![constraint(1, native), constraint(2, offset)],
        vec![parameter],
    )
}

#[test]
fn offset_dimension_binding_preserves_parameter_and_removes_annotation() {
    let (mut constraints, parameters) = offset_binding_fixture();
    bind_offset_dimension_parameters(&mut constraints, &parameters);
    assert_eq!(constraints.len(), 1);
    assert!(matches!(constraints[0].definition.kind(),
        SketchConstraintDefinitionInput::Offset { parameter: Some(driving), .. }
            if driving.id == neutral_parameter_id(&parameters[0]) && !driving.negated));
}
