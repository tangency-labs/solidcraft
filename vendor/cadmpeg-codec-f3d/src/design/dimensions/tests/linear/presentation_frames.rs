// SPDX-License-Identifier: Apache-2.0
use super::{
    parameter_record, parse_design_parameter_record, Point2, SketchConstraintDefinitionInput,
    SketchEntity, SketchEntityId, SketchGeometry, SketchGeometryDefinition, SketchId,
};
use crate::design::dimensions::governing_presentation_definition;
use crate::records::dimensions::{
    DesignDimensionPresentationFrame, DesignDimensionPresentationOperand,
};

fn frame(id: &str, operands: [u32; 2]) -> DesignDimensionPresentationFrame {
    DesignDimensionPresentationFrame {
        id: format!("stream:presentation#{id}"),
        byte_offset: 0,
        class_tag: crate::records::references::DesignClassTag::try_from("289".to_owned()).unwrap(),
        record_index: 0,
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
        paired_class_tag: crate::records::references::DesignClassTag::try_from("291".to_owned())
            .unwrap(),
        paired_byte_offset: 0,
        owner_reference: 0,
        owner_reference_offset: 0,
        governing_owner_record_index: 7,
        governing_parameter_record_index: 0,
        governing_companion_record_index: 0,
    }
}

#[test]
fn owner_with_several_frames_takes_the_one_that_measures_its_parameter() {
    let sketch = SketchId::mint("generated:test:sketch#frames").unwrap();
    let point = |record_index: u32, u, v| {
        SketchEntity::new(
            SketchEntityId::mint(format!("generated:test:entity#{record_index}")).unwrap(),
            sketch.clone(),
            SketchGeometry::try_from(SketchGeometryDefinition::Point {
                position: Point2::new(u, v),
            })
            .unwrap(),
        )
    };
    let entities = [
        point(11, 0.0, 0.0),
        point(12, 0.0, 51.75),
        point(21, 0.0, 0.0),
        point(22, 916.75, 0.0),
    ];
    let projected = entities
        .iter()
        .map(|entity| {
            let record_index = entity
                .id()
                .as_str()
                .rsplit_once('#')
                .and_then(|(_, index)| index.parse::<u32>().ok())
                .unwrap();
            (("stream", record_index), entity)
        })
        .collect::<std::collections::HashMap<_, _>>();
    let parameter = parse_design_parameter_record(&parameter_record(
        Some(7),
        "5.175",
        "Linear Dimension-6",
        Some("cm"),
        "d55",
        5.175,
    ))
    .unwrap();
    let parameter_id = cadmpeg_ir::features::ParameterId::mint("synthetic:test:id#parameter:d55")
        .expect("identity grammar");
    let measuring = frame("measuring", [11, 12]);
    let stale = frame("stale", [21, 22]);
    let resolve = |frames: &[&DesignDimensionPresentationFrame]| {
        governing_presentation_definition(
            frames.iter().copied(),
            "stream",
            &projected,
            &parameter,
            &parameter_id,
            1.0e-6,
        )
    };
    for frames in [[&stale, &measuring], [&measuring, &stale]] {
        assert!(matches!(
            resolve(&frames),
            Some(SketchConstraintDefinitionInput::VerticalDistance { first, second, .. })
                if first == cadmpeg_ir::sketches::SketchLocus::Entity(entities[0].id().clone())
                    && second == cadmpeg_ir::sketches::SketchLocus::Entity(entities[1].id().clone())
        ));
    }
    assert!(resolve(&[&stale]).is_none());
    assert!(resolve(&[&measuring, &measuring]).is_none());
}
