// SPDX-License-Identifier: Apache-2.0

#[test]
fn work_plane_projects_solved_frame_in_millimetres() {
    use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};
    use cadmpeg_ir::math::{Point3, Vector3};

    let mut plane =
        DesignParameterScope::empty("f3d:test:scope#1", DesignFeatureKind::WorkPlane, 1);
    let matrix = [
        [0.0, 0.0, 1.0, 3.0],
        [1.0, 0.0, 0.0, -4.0],
        [0.0, 1.0, 0.0, 7.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    plane.with_work_plane_transform(matrix.try_into().unwrap());
    let FeatureDefinition::Operation(FeatureOperation::DatumPlane { frame }) =
        super::super::project_work_plane(&plane, matrix)
    else {
        panic!("expected solved datum plane");
    };
    let expected = cadmpeg_ir::features::FeatureDatumPlaneFrame::new(
        Point3::new(30.0, -40.0, 70.0),
        Vector3::new(1.0, 0.0, 0.0),
        Vector3::new(0.0, 1.0, 0.0),
    )
    .unwrap();
    assert_eq!(frame, expected);
}

#[test]
fn work_plane_projects_three_point_vertex_recipes() {
    use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
    use crate::records::feature::work_geometry::{DesignVertexRecipe, DesignWorkPlaneConstruction};

    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};

    let recipe = |record_index, vertex| {
        DesignVertexRecipe::try_new(
            crate::records::feature::work_geometry::DesignVertexRecipeDraft {
                record_index,
                byte_offset: u64::from(record_index),
                class_tag: crate::records::references::DesignClassTag::try_from("306".to_owned())
                    .unwrap(),
                paired_byte_offset: u64::from(record_index) + 16,
                paired_class_tag: crate::records::references::DesignClassTag::try_from(
                    "261".to_owned(),
                )
                .unwrap(),
                recipe_record_index: record_index + 3,
                recipe_record_byte_offset: u64::from(record_index) + 32,
                recipe_id: format!("f3d:test:recipe#{record_index}"),
                recipe_prefix_offset: u64::from(record_index) + 43,
                recipe_prefix_bytes: Vec::new(),
                recipe_references: Vec::new(),
                recipe_program_offset: 4,
                recipe_program: vec![0],
                resolution: Some(
                    crate::records::feature::work_geometry::DesignVertexResolution::new(4, vertex)
                        .unwrap(),
                ),
                next_record_index: record_index + 5,
                next_byte_offset: u64::from(record_index) + 200,
            },
        )
        .unwrap()
    };
    let mut plane =
        DesignParameterScope::empty("f3d:test:scope#20", DesignFeatureKind::WorkPlane, 20);
    plane.with_work_plane_transform(
        crate::records::sketch_placement::SketchPlacementMatrix::IDENTITY,
    );
    if let Some(frame) = plane.work_plane_frame_mut() {
        frame.work_plane_construction = Some(
            DesignWorkPlaneConstruction::try_new(
                21,
                Box::new([recipe(22, 43), recipe(27, 64), recipe(32, 84)]),
            )
            .unwrap(),
        );
    }
    let transform = crate::records::sketch_placement::SketchPlacementMatrix::IDENTITY.into();
    let FeatureDefinition::Operation(FeatureOperation::DatumThreePointPlane { points, .. }) =
        super::super::project_work_plane(&plane, transform)
    else {
        panic!("expected a three-point datum plane");
    };
    // Each recipe resolves vertex `slot` of input state 4 and keeps its recipe ID.
    let feature = crate::ids::neutral_feature_id(&plane);
    let prefix = crate::ids::feature_input_prefix(&feature, 4);
    let expected = [(22, 43), (27, 64), (32, 84)].map(|(record_index, slot)| {
        cadmpeg_ir::features::VertexSelection::historical(
            crate::ids::feature_input_topology_id(&feature, 4),
            crate::ids::history_input_vertex_id(&prefix, slot),
            format!("f3d:test:recipe#{record_index}"),
        )
        .unwrap()
    });
    assert_eq!(*points, expected);
}

#[test]
fn work_plane_modern_solved_frames_project_without_a_construction_inference() {
    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};
    use cadmpeg_ir::math::{Point3, Vector3};
    for placement in crate::test_support::placement_construction::modern_placements() {
        let mut scope = crate::test_support::work_plane::scope(&placement);
        assert!(
            crate::design::decode::placement_construction::work_planes::bind(
                std::slice::from_mut(&mut scope),
                std::slice::from_ref(&placement)
            )
            .is_empty()
        );
        let FeatureDefinition::Operation(FeatureOperation::DatumPlane { frame }) =
            super::super::project_work_plane(&scope, scope.work_plane_transform().unwrap().rows())
        else {
            panic!("expected solved datum plane");
        };
        let origin = if placement.fields.identity.as_ref().unwrap().value {
            Point3::new(0.0, 0.0, 0.0)
        } else {
            Point3::new(20.0, 30.0, 40.0)
        };
        let expected = cadmpeg_ir::features::FeatureDatumPlaneFrame::new(
            origin,
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .unwrap();
        assert_eq!(frame, expected);
    }
}
