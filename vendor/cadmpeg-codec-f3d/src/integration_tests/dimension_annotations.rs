// SPDX-License-Identifier: Apache-2.0

use cadmpeg_ir::codec::CodecBackend;
use cadmpeg_ir::sketches::{SketchConstraintDefinitionInput, SketchLocus};

use super::{assert_valid, decode};
use crate::loss::F3dLossCode;
use crate::test_support::native_test::f3d_native;
use crate::test_support::zip_test::{
    f3d_with_generated_sketch_annotation, f3d_with_generated_sketch_presentation,
    SketchAnnotationCase,
};
use crate::F3dCodec;

#[test]
fn dimension_annotation_empty_metadata_projects_the_governed_parameter_and_entities() {
    let result = decode(f3d_with_generated_sketch_annotation(
        SketchAnnotationCase::Valid,
    ));
    assert_valid(&result);
    let native = f3d_native(result.ir());
    assert_eq!(native.design_dimension_annotation_frames.len(), 1);
    let frame = &native.design_dimension_annotation_frames[0];
    assert_eq!(frame.entity_genesis(), None);
    assert_eq!(frame.companion_record_index, Some(1202));
    assert_eq!(frame.governing_companion_record_index, 1205);
    let source_parameter = native
        .design_parameters
        .iter()
        .find(|parameter| parameter.record_index == 1204)
        .unwrap();
    let expected_parameter = crate::ids::neutral_parameter_id(source_parameter);
    let constraints = result
        .ir()
        .model
        .sketch_constraints
        .iter()
        .filter(|constraint| constraint.native_ref.as_deref() == Some(frame.id.as_str()))
        .collect::<Vec<_>>();
    assert_eq!(constraints.len(), 1);
    let SketchConstraintDefinitionInput::HorizontalDistance {
        first: SketchLocus::Entity(first),
        second: SketchLocus::Entity(second),
        parameter,
    } = constraints[0].definition.kind()
    else {
        panic!("horizontal dimension")
    };
    assert_eq!(parameter, &expected_parameter);
    for (index, entity) in [(1300, first), (1302, second)] {
        let source = native
            .sketch_points
            .iter()
            .find(|point| point.record_index == index)
            .unwrap();
        let expected = result
            .ir()
            .model
            .sketch_entities
            .iter()
            .find(|entity| entity.native_ref.as_deref() == Some(source.id.as_str()))
            .unwrap();
        assert_eq!(entity, expected.id());
    }
    assert!(<F3dCodec as CodecBackend>::validate_native(
        &cadmpeg_test_support::service_decode_context(),
        result.ir(),
        None
    )
    .unwrap()
    .iter()
    .all(|finding| finding.entity.as_deref() != Some(frame.id.as_str())));
    assert!(!result.report().losses.iter().any(|loss| loss.code
        == F3dLossCode::SketchDimensionNativeRetained
            .note(String::new())
            .code));
}

#[test]
fn dimension_annotation_empty_metadata_keeps_ambiguous_and_refused_dimensions_with_losses() {
    for case in [
        SketchAnnotationCase::Ambiguous,
        SketchAnnotationCase::UnknownOperand,
        SketchAnnotationCase::MetadataCount,
    ] {
        let result = decode(f3d_with_generated_sketch_annotation(case));
        let native = f3d_native(result.ir());
        assert_eq!(
            native.design_dimension_annotation_frames.len(),
            usize::from(case == SketchAnnotationCase::Ambiguous)
        );
        let source = native
            .design_parameters
            .iter()
            .find(|parameter| parameter.record_index == 1204)
            .unwrap();
        let expected_parameter = crate::ids::neutral_parameter_id(source);
        assert!(result.ir().model.sketch_constraints.iter().any(|constraint| matches!(constraint.definition.kind(), SketchConstraintDefinitionInput::Native { parameter: Some(parameter), .. } if parameter == &expected_parameter)));
        assert!(result.report().losses.iter().any(|loss| loss.code
            == F3dLossCode::SketchDimensionNativeRetained
                .note(String::new())
                .code));
    }
}

const PRESENTATION_V1: &str = "6CCF41D5-40BE-48ED-A834-18F3EAED6C57";
const PRESENTATION_V3: &str = "8C780195-72C0-4a56-A911-E43AB14357F2";
const PAIR_ENTITY_TRACKING: &str = "90055C05-546C-4EE7-B3C9-3DD922AD0C9C";
const PAIR_DIMENSION: &str = "855F0A64-6286-4FFC-B918-BC394F600107";

#[test]
fn presentation_frame_classes_resolve_through_type_table_ordinals() {
    for (primary, paired) in [
        (PRESENTATION_V3, PAIR_DIMENSION),
        (PRESENTATION_V3, PAIR_ENTITY_TRACKING),
        (PRESENTATION_V1, PAIR_DIMENSION),
    ] {
        let result = decode(f3d_with_generated_sketch_presentation(primary, paired));
        assert_valid(&result);
        let native = f3d_native(result.ir());
        let [frame] = native.design_dimension_presentation_frames.as_slice() else {
            panic!("one presentation frame for {primary} and {paired}")
        };
        assert_eq!(frame.class_tag.as_str(), "273");
        assert_eq!(frame.paired_class_tag.as_str(), "274");
        assert_eq!(frame.owner_reference, 277);
        assert_eq!(frame.governing_parameter_record_index, 1204);
        assert_eq!(frame.governing_companion_record_index, 1205);
        assert_eq!(
            frame
                .operands
                .iter()
                .map(|operand| (operand.geometry_record_index.get(), operand.role))
                .collect::<Vec<_>>(),
            [(1300, 0), (1302, 1)]
        );
        let source = native
            .design_parameters
            .iter()
            .find(|parameter| parameter.record_index == 1204)
            .unwrap();
        let expected_parameter = crate::ids::neutral_parameter_id(source);
        let expected_entity = |index| {
            let point = native
                .sketch_points
                .iter()
                .find(|point| point.record_index == index)
                .unwrap();
            result
                .ir()
                .model
                .sketch_entities
                .iter()
                .find(|entity| entity.native_ref.as_deref() == Some(point.id.as_str()))
                .unwrap()
                .id()
                .clone()
        };
        assert!(result
            .ir()
            .model
            .sketch_constraints
            .iter()
            .any(|constraint| matches!(
                constraint.definition.kind(),
                SketchConstraintDefinitionInput::HorizontalDistance {
                    first: SketchLocus::Entity(first),
                    second: SketchLocus::Entity(second),
                    parameter,
                } if parameter == &expected_parameter
                    && first == &expected_entity(1300)
                    && second == &expected_entity(1302)
            )));
    }
}

#[test]
fn presentation_frame_requires_a_registered_primary_and_paired_type() {
    let unrelated = "00000000-0000-0000-0000-00000000ABCD";
    for (primary, paired) in [
        (PRESENTATION_V3, unrelated),
        (unrelated, PAIR_DIMENSION),
        (PAIR_DIMENSION, PRESENTATION_V3),
    ] {
        let result = decode(f3d_with_generated_sketch_presentation(primary, paired));
        assert!(f3d_native(result.ir())
            .design_dimension_presentation_frames
            .is_empty());
    }
}
