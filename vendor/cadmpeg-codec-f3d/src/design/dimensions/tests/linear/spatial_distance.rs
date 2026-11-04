// SPDX-License-Identifier: Apache-2.0
use super::{
    neutral_sketch_id, neutral_spatial_sketch_id, project_dimension_constraints,
    DesignDimensionLocus, DesignDimensionLocusGroup, DesignDimensionLocusPair,
    DesignParameterCompanion, DesignSketchPlacement, Point2, SketchConstraintDefinitionInput,
    SketchEntity, SketchEntityId, SketchGeometry, SketchGeometryDefinition, SketchPoint,
    SpatialSketch,
};
#[test]
fn spatial_distance_projects_vertical_distance_and_skips_spatial_sketches() {
    let stream = "f3d:A";
    let placement = DesignSketchPlacement {
        frame: crate::records::sketch_placement::DesignSketchFrame::new(
            0,
            crate::records::sketch_placement::DesignSketchFrameForm::ScopeCompact,
        )
        .unwrap(),

        id: format!("{stream}:design-sketch-placement#0"),
        scope_record_index: Some(10),
        entity_id: crate::records::identity::DesignEntityId::try_from("0_100".to_owned())
            .expect("valid entity ID"),

        visibility: None,

        class_tag: crate::records::references::DesignClassTag::try_from("356".to_owned()).unwrap(),
        record_index: 11,

        paired_class_tag: crate::records::references::DesignClassTag::try_from("259".to_owned())
            .unwrap(),
    };
    let parameter = crate::records::parameters::DesignParameter::try_from(
        crate::records::parameters::DesignParameterDraft {
            id: format!("{stream}:design-parameter#20"),
            byte_offset: 0,
            class_tag: crate::records::references::DesignClassTag::try_from("305".to_owned())
                .unwrap(),
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
            id: format!("{stream}:design-parameter-owner#21"),
            byte_offset: 0,
            frame_length: 104,
            class_tag: crate::records::references::DesignClassTag::try_from("292".to_owned())
                .unwrap(),
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
        format!("{stream}:design-parameter-companion#22"),
        0,
        crate::records::references::DesignClassTag::try_from("408".to_owned()).unwrap(),
        22,
        21,
        std::num::NonZeroU64::new(1).unwrap(),
        42,
    )
    .bound(crate::records::parameters::DesignCompanionPayload::new(
        58,
        0,
        Vec::new(),
    ));
    let pair = DesignDimensionLocusPair::try_new(
        crate::records::dimensions::DesignDimensionLocusPairDraft {
            id: format!("{stream}:design-dimension-locus-pair#30"),
            companion_record_index: 99,
            governing_companion_record_index: 22,
            byte_offset: 30,
            class_tag: crate::records::references::DesignClassTag::try_from("277".to_owned())
                .unwrap(),
            record_index: 30,
            frame_length: 100,
            opaque_index: Some(crate::records::identity::Located {
                value: 0,
                offset: 65,
            }),
            loci: [
                crate::records::dimensions::DesignDimensionAnnotationOperand {
                    geometry_record_index: std::num::NonZeroU32::new(40),
                    geometry_reference_offset: 70,
                    role: 7,
                    role_offset: 80,
                },
                crate::records::dimensions::DesignDimensionAnnotationOperand {
                    geometry_record_index: std::num::NonZeroU32::new(41),
                    geometry_reference_offset: 85,
                    role: 8,
                    role_offset: 95,
                },
            ],
            paired_class_tag: crate::records::references::DesignClassTag::try_from(
                "273".to_owned(),
            )
            .unwrap(),
            paired_byte_offset: 130,
        },
    )
    .unwrap();
    let group = DesignDimensionLocusGroup {
        id: format!("{stream}:design-dimension-locus-group#140"),
        companion_record_index: 99,
        byte_offset: 140,
        class_tag: crate::records::references::DesignClassTag::try_from("277".to_owned()).unwrap(),
        record_index: 31,
        frame_length: 100,
        loci: vec![DesignDimensionLocus {
            returned: crate::records::identity::Located {
                value: 40,
                offset: 210,
            },
            geometry_record_index: 40,
            geometry_reference_offset: 170,
            role: 0,
            role_offset: 180,
        }],
        owner_reference: 100,
        owner_reference_offset: 185,
        owner_role: 0,
        owner_role_offset: 195,
        state: 0,
        state_offset: 199,
        next_class_tag: crate::records::references::DesignClassTag::try_from("273".to_owned())
            .unwrap(),
        next_record_index: 32,
        next_byte_offset: 240,
    };
    let point = |record_index, y| {
        SketchPoint::try_from(crate::records::sketch_geometry::SketchPointDraft {
            id: format!("{stream}:sketch-point#{record_index}"),
            record_index,
            owner_reference: Some(100),
            class_tag: crate::records::references::DesignClassTag::try_from("300".to_owned())
                .unwrap(),
            byte_offset: 0,
            coordinate_offset: 0,
            companion: crate::records::sketch_geometry::SketchPointCompanion {
                incident_curves: Vec::new(),
            },
            record_form: crate::records::sketch_geometry::SketchPointRecordForm::version11(
                u64::from(record_index),
                crate::records::sketch_geometry::SketchPointClosure::Selector0State0,
                None,
                0.0,
            ),
            paired_reference: 0,
            coordinates: Point2::new(0.0, y),
        })
        .unwrap()
    };
    let points = [point(40, 0.0), point(41, 2.0)];
    let sketch = neutral_sketch_id(&placement);
    let entities = points
        .iter()
        .map(|point| {
            SketchEntity::new(
                SketchEntityId::mint(format!("synthetic:test:id#point-{}", point.record_index))
                    .unwrap(),
                sketch.clone(),
                SketchGeometry::try_from(SketchGeometryDefinition::Point {
                    position: point.coordinates(),
                })
                .unwrap(),
            )
            .with_native_ref(Some(point.id.clone()))
        })
        .collect::<Vec<_>>();

    let constraints = project_dimension_constraints(
        &crate::design::dimensions::DimensionConstraintInputs {
            placements: std::slice::from_ref(&placement),
            parameters: std::slice::from_ref(&parameter),
            owners: std::slice::from_ref(&owner),
            pairs: std::slice::from_ref(&pair),
            groups: std::slice::from_ref(&group),
            annotation_frames: &[],
            null_pairs: &[],
            companions: std::slice::from_ref(&companion),
            recipe_records: &[],
            points: &points,
            curves: &[],
            entities: &entities,
        },
        &[],
    );

    assert_eq!(constraints.len(), 1);
    assert!(matches!(
        constraints[0].definition.kind(),
        SketchConstraintDefinitionInput::VerticalDistance { .. }
    ));

    let spatial_sketch = SpatialSketch {
        id: neutral_spatial_sketch_id(&placement),
        name: None,
        configuration: None,
        visible: None,
        profiles: Vec::new(),
        native_ref: Some(placement.id.clone()),
    };
    assert!(project_dimension_constraints(
        &crate::design::dimensions::DimensionConstraintInputs {
            placements: std::slice::from_ref(&placement),
            parameters: std::slice::from_ref(&parameter),
            owners: std::slice::from_ref(&owner),
            pairs: std::slice::from_ref(&pair),
            groups: std::slice::from_ref(&group),
            annotation_frames: &[],
            null_pairs: &[],
            companions: std::slice::from_ref(&companion),
            recipe_records: &[],
            points: &points,
            curves: &[],
            entities: &entities,
        },
        std::slice::from_ref(&spatial_sketch),
    )
    .is_empty());
}
