// SPDX-License-Identifier: Apache-2.0
use super::project_dimension_constraints;
use super::project_spatial_dimension_constraints;
use crate::design::decode::parameters::parse_design_parameter_record;
use crate::design::dimensions::directional_point_dimension;
use crate::design::dimensions::exact_counted_dimension_relation;
use crate::design::dimensions::repeated_linear_dimension;
use crate::design::dimensions::two_locus_distance_dimension;
use crate::design::test_support::parameter_record;
use crate::ids::neutral_parameter_id_parts;
use crate::ids::neutral_sketch_id;
use crate::ids::neutral_spatial_sketch_id;
use crate::records::dimensions::DesignDimensionLocus;
use crate::records::dimensions::DesignDimensionLocusGroup;
use crate::records::dimensions::DesignDimensionLocusPair;
use crate::records::parameters::DesignParameterCompanion;
use crate::records::sketch_geometry::SketchCurveIdentity;
use crate::records::sketch_geometry::SketchPoint;
use crate::records::sketch_placement::DesignSketchPlacement;
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::sketches::SketchConstraintDefinitionInput;
use cadmpeg_ir::sketches::SketchEntity;
use cadmpeg_ir::sketches::SketchEntityId;
use cadmpeg_ir::sketches::SketchGeometry;
use cadmpeg_ir::sketches::SketchGeometryDefinition;
use cadmpeg_ir::sketches::SketchId;
use cadmpeg_ir::sketches::SpatialSketch;
use cadmpeg_ir::sketches::SpatialSketchConstraintDefinitionInput;

#[test]
fn dimension_proofs_require_the_evaluated_measurement() {
    const DOCUMENT_LINEAR_TOLERANCE: f64 = 1.0e-6;

    let dimension = |source_kind: &str, unit: &str| {
        parse_design_parameter_record(&parameter_record(
            Some(44),
            "value",
            source_kind,
            Some(unit),
            "d1",
            1.0,
        ))
        .expect("generated dimension parameter is canonical")
    };
    assert!(crate::design::feature_project::design_dimension_unit(
        &dimension("Linear Dimension-2", "mm")
    ));
    assert!(crate::design::feature_project::design_dimension_unit(
        &dimension("Radial Dimension-3", "mm")
    ));
    assert!(!crate::design::feature_project::design_dimension_unit(
        &dimension("Linear Dimension-2", "deg")
    ));
    assert!(crate::design::feature_project::design_dimension_unit(
        &dimension("Angular Dimension-2", "rad")
    ));
    assert!(crate::design::feature_project::design_dimension_unit(
        &dimension("Tangent Dimension-2", "mm")
    ));
    assert!(!crate::design::feature_project::design_dimension_unit(
        &dimension("Tangent Dimension-2", "deg")
    ));
    assert!(!crate::design::feature_project::design_dimension_unit(
        &dimension("Angular Dimension-2", "mm")
    ));
    assert!(!crate::design::feature_project::design_dimension_unit(
        &dimension("Radius Dimension-2", "native-unit")
    ));

    let entity = |id: &str, geometry: SketchGeometry| {
        cadmpeg_ir::sketches::SketchEntity::new(
            SketchEntityId::mint(id).unwrap(),
            SketchId::mint("generated:test:sketch#0").unwrap(),
            geometry,
        )
    };
    let first = entity(
        "generated:test:point#0",
        SketchGeometry::try_from(SketchGeometryDefinition::Point {
            position: Point2::new(0.0, 0.0),
        })
        .unwrap(),
    );
    let second = entity(
        "generated:test:point#1",
        SketchGeometry::try_from(SketchGeometryDefinition::Point {
            position: Point2::new(40.0, 0.0),
        })
        .unwrap(),
    );
    let parameter = cadmpeg_ir::features::ParameterId::mint("generated:test:parameter#0")
        .expect("identity grammar");
    assert!(crate::design::dimensions::directional_point_dimension(
        &[&first, &second],
        10.0,
        parameter.clone(),
        0.0
    )
    .is_none());
    assert!(matches!(
        crate::design::dimensions::directional_point_dimension(
            &[&first, &second],
            40.0,
            parameter.clone(),
            0.0
        ),
        Some(SketchConstraintDefinitionInput::HorizontalDistance { .. })
    ));
    let rounded = entity(
        "generated:test:point#rounded",
        SketchGeometry::try_from(SketchGeometryDefinition::Point {
            position: Point2::new(40.000_000_5, 0.0),
        })
        .unwrap(),
    );
    assert!(matches!(
        crate::design::dimensions::directional_point_dimension(
            &[&first, &rounded],
            40.0,
            parameter,
            1.0e-6
        ),
        Some(SketchConstraintDefinitionInput::HorizontalDistance { .. })
    ));

    let horizontal = entity(
        "generated:test:line#horizontal",
        SketchGeometry::try_from(SketchGeometryDefinition::Line {
            start: Point2::new(0.0, 0.0),
            end: Point2::new(10.0, 0.0),
        })
        .unwrap(),
    );
    let diagonal = entity(
        "generated:test:line#diagonal",
        SketchGeometry::try_from(SketchGeometryDefinition::Line {
            start: Point2::new(0.0, 0.0),
            end: Point2::new(10.0, 10.0),
        })
        .unwrap(),
    );
    assert!(!crate::design::dimensions::parallel_line_separation(
        &horizontal,
        &diagonal,
        2.0,
        0.0,
    ));
    assert!(!crate::design::dimensions::line_angle_matches(
        &horizontal.geometry,
        &diagonal.geometry,
        std::f64::consts::FRAC_PI_6,
    ));
    assert!(crate::design::dimensions::line_angle_matches(
        &horizontal.geometry,
        &diagonal.geometry,
        std::f64::consts::FRAC_PI_4,
    ));
    let vertical = entity(
        "generated:test:line#vertical",
        SketchGeometry::try_from(SketchGeometryDefinition::Line {
            start: Point2::new(0.0, -10.0),
            end: Point2::new(0.0, 10.0),
        })
        .unwrap(),
    );
    let offset_point = entity(
        "generated:test:point#offset",
        SketchGeometry::try_from(SketchGeometryDefinition::Point {
            position: Point2::new(2.0, 4.0),
        })
        .unwrap(),
    );
    assert!(crate::design::dimensions::point_line_separation(
        &offset_point,
        &vertical,
        2.0,
        0.0,
    ));
    assert!(crate::design::dimensions::point_line_separation(
        &offset_point,
        &vertical,
        2.000_000_5,
        1.0e-6,
    ));
    assert!(!crate::design::dimensions::point_line_separation(
        &vertical,
        &offset_point,
        3.0,
        1.0e-6,
    ));

    let inner_circle = entity(
        "generated:test:circle#inner",
        SketchGeometry::try_from(SketchGeometryDefinition::Circle {
            center: Point2::new(3.0, -2.0),
            radius: cadmpeg_ir::scalar::Length::new(4.0).unwrap(),
        })
        .unwrap(),
    );
    let outer_circle = entity(
        "generated:test:circle#outer",
        SketchGeometry::try_from(SketchGeometryDefinition::Circle {
            center: Point2::new(3.0, -2.0),
            radius: cadmpeg_ir::scalar::Length::new(4.25).unwrap(),
        })
        .unwrap(),
    );
    assert!(crate::design::dimensions::concentric_circle_separation(
        &inner_circle,
        &outer_circle,
        0.25,
        0.0,
    ));
    assert!(!crate::design::dimensions::concentric_circle_separation(
        &inner_circle,
        &outer_circle,
        0.5,
        0.0,
    ));
    let displaced_circle = entity(
        "generated:test:circle#displaced",
        SketchGeometry::try_from(SketchGeometryDefinition::Circle {
            center: Point2::new(3.001, -2.0),
            radius: cadmpeg_ir::scalar::Length::new(4.25).unwrap(),
        })
        .unwrap(),
    );
    assert!(!crate::design::dimensions::concentric_circle_separation(
        &inner_circle,
        &displaced_circle,
        0.25,
        0.0,
    ));

    let tolerant_center_circle = entity(
        "generated:test:circle#tolerant-center",
        SketchGeometry::try_from(SketchGeometryDefinition::Circle {
            center: Point2::new(3.000_000_5, -2.0),
            radius: cadmpeg_ir::scalar::Length::new(4.25).unwrap(),
        })
        .unwrap(),
    );
    assert!(crate::design::dimensions::concentric_circle_separation(
        &inner_circle,
        &tolerant_center_circle,
        0.25,
        DOCUMENT_LINEAR_TOLERANCE,
    ));
    assert!(!crate::design::dimensions::concentric_circle_separation(
        &inner_circle,
        &tolerant_center_circle,
        0.25,
        0.0,
    ));
    let tolerant_parallel = entity(
        "generated:test:line#tolerant-parallel",
        SketchGeometry::try_from(SketchGeometryDefinition::Line {
            start: Point2::new(0.0, 2.000_000_5),
            end: Point2::new(10.0, 2.000_000_5),
        })
        .unwrap(),
    );
    assert!(crate::design::dimensions::parallel_line_separation(
        &horizontal,
        &tolerant_parallel,
        2.0,
        DOCUMENT_LINEAR_TOLERANCE,
    ));
    assert!(!crate::design::dimensions::parallel_line_separation(
        &horizontal,
        &tolerant_parallel,
        2.0,
        0.0,
    ));
    let tolerant_outer_circle = entity(
        "generated:test:circle#tolerant-outer",
        SketchGeometry::try_from(SketchGeometryDefinition::Circle {
            center: Point2::new(3.0, -2.0),
            radius: cadmpeg_ir::scalar::Length::new(4.250_000_5).unwrap(),
        })
        .unwrap(),
    );
    assert!(crate::design::dimensions::concentric_circle_separation(
        &inner_circle,
        &tolerant_outer_circle,
        0.25,
        DOCUMENT_LINEAR_TOLERANCE,
    ));
    assert!(!crate::design::dimensions::concentric_circle_separation(
        &inner_circle,
        &tolerant_outer_circle,
        0.25,
        0.0,
    ));
}

#[test]
fn presentation_dimensions_use_direct_operands_with_measurement_proofs() {
    let sketch = SketchId::mint("generated:test:sketch#presentation").unwrap();
    let entity = |record_index: u32, geometry: SketchGeometry| {
        SketchEntity::new(
            SketchEntityId::mint(format!("generated:test:entity#{record_index}")).unwrap(),
            sketch.clone(),
            geometry,
        )
        .with_native_ref(Some(format!("stream:geometry#{record_index}")))
    };
    let line = entity(
        306,
        SketchGeometry::try_from(SketchGeometryDefinition::Line {
            start: Point2::new(90.4875, -17.78),
            end: Point2::new(90.4875, 17.78),
        })
        .unwrap(),
    );
    let circle = entity(
        331,
        SketchGeometry::try_from(SketchGeometryDefinition::Circle {
            center: Point2::new(0.0, 0.0),
            radius: cadmpeg_ir::scalar::Length::new(11.1125).unwrap(),
        })
        .unwrap(),
    );
    let arc = entity(
        796,
        SketchGeometry::try_from(SketchGeometryDefinition::Arc {
            center: Point2::new(60.344_057_626_1, -19.05),
            radius: cadmpeg_ir::scalar::Length::new(12.7).unwrap(),
            start_angle: cadmpeg_ir::scalar::Angle::new(0.0).unwrap(),
            end_angle: cadmpeg_ir::scalar::Angle::new(0.975_682_713_4).unwrap(),
        })
        .unwrap(),
    );
    let outer_arc = entity(
        782,
        SketchGeometry::try_from(SketchGeometryDefinition::Arc {
            center: Point2::new(60.344_057_626_1, 19.05),
            radius: cadmpeg_ir::scalar::Length::new(12.7).unwrap(),
            start_angle: cadmpeg_ir::scalar::Angle::new(0.0).unwrap(),
            end_angle: cadmpeg_ir::scalar::Angle::new(0.975_682_713_4).unwrap(),
        })
        .unwrap(),
    );
    let first_point = entity(
        1061,
        SketchGeometry::try_from(SketchGeometryDefinition::Point {
            position: Point2::new(11.1125, -11.1125),
        })
        .unwrap(),
    );
    let second_point = entity(
        1075,
        SketchGeometry::try_from(SketchGeometryDefinition::Point {
            position: Point2::new(11.1125, -7.3025),
        })
        .unwrap(),
    );
    let entities = [line, circle, arc, outer_arc, first_point, second_point];
    let projected = entities
        .iter()
        .map(|entity| {
            let record_index = entity
                .native_ref
                .as_deref()
                .and_then(|native_ref| native_ref.rsplit_once('#'))
                .and_then(|(_, index)| index.parse::<u32>().ok())
                .expect("synthetic native geometry record");
            (("stream", record_index), entity)
        })
        .collect::<std::collections::HashMap<_, _>>();
    let frame = |operands| crate::records::dimensions::DesignDimensionPresentationFrame {
        id: "stream:presentation#0".into(),
        byte_offset: 0,
        class_tag: crate::records::references::DesignClassTag::try_from("314".to_owned()).unwrap(),
        record_index: 0,
        frame_length: 0,
        operands,
        presentation_bytes: Vec::new().into(),
        presentation_byte_offset: 0,
        paired_class_tag: crate::records::references::DesignClassTag::try_from("281".to_owned())
            .unwrap(),
        paired_byte_offset: 0,
        owner_reference: 0,
        owner_reference_offset: 0,
        governing_owner_record_index: 0,
        governing_parameter_record_index: 0,
        governing_companion_record_index: 0,
    };
    let operand = |record_index| crate::records::dimensions::DesignDimensionPresentationOperand {
        geometry_record_index: std::num::NonZeroU32::new(record_index).unwrap(),
        geometry_reference_offset: 0,
        role: 0,
        role_offset: 0,
    };
    let tangent_span = parse_design_parameter_record(&parameter_record(
        Some(44),
        "10.16",
        "Tangent Dimension-2",
        Some("cm"),
        "d4",
        10.16,
    ))
    .expect("synthetic tangent dimension");
    assert!(matches!(
        crate::design::dimensions::presentation_dimension_definition("stream", &frame(vec![operand(306), operand(331)]), &projected, &tangent_span, &cadmpeg_ir::features::ParameterId::mint("synthetic:test:id#parameter:d4").expect("identity grammar"), 1.0e-6),
        Some(SketchConstraintDefinitionInput::Distance { entities, .. })
            if entities.len() == 2
    ));

    let tangent_radius = parse_design_parameter_record(&parameter_record(
        Some(45),
        "1.27",
        "Tangent Dimension-2",
        Some("cm"),
        "d16",
        1.27,
    ))
    .expect("synthetic tangent radius dimension");
    assert!(matches!(
        crate::design::dimensions::presentation_dimension_definition("stream", &frame(vec![operand(796)]), &projected, &tangent_radius, &cadmpeg_ir::features::ParameterId::mint("synthetic:test:id#parameter:d16").expect("identity grammar"), 1.0e-6),
        Some(SketchConstraintDefinitionInput::Radius { entity, .. })
            if entity.as_str() == "generated:test:entity#796"
    ));
    assert!(matches!(
        crate::design::dimensions::presentation_dimension_definition("stream", &frame(vec![operand(782), operand(796)]), &projected, &tangent_radius, &cadmpeg_ir::features::ParameterId::mint("synthetic:test:id#parameter:d16").expect("identity grammar"), 1.0e-6),
        Some(SketchConstraintDefinitionInput::Distance { entities, .. })
            if entities.len() == 2
    ));
    let ambiguous_tangent = parse_design_parameter_record(&parameter_record(
        Some(47),
        "3.81",
        "Tangent Dimension-2",
        Some("cm"),
        "d16_ambiguous",
        3.81,
    ))
    .expect("synthetic ambiguous tangent dimension");
    assert!(
        crate::design::dimensions::presentation_dimension_definition(
            "stream",
            &frame(vec![operand(782), operand(796)]),
            &projected,
            &ambiguous_tangent,
            &cadmpeg_ir::features::ParameterId::mint("synthetic:test:id#parameter:d16_ambiguous")
                .expect("identity grammar"),
            1.0e-6
        )
        .is_none()
    );

    let point_distance = parse_design_parameter_record(&parameter_record(
        Some(46),
        "0.381",
        "Linear Dimension-2",
        Some("cm"),
        "d32",
        0.381,
    ))
    .expect("synthetic point distance dimension");
    let point_definition = crate::design::dimensions::presentation_dimension_definition(
        "stream",
        &frame(vec![operand(1061), operand(1075)]),
        &projected,
        &point_distance,
        &cadmpeg_ir::features::ParameterId::mint("synthetic:test:id#parameter:d32")
            .expect("identity grammar"),
        1.0e-6,
    );
    assert!(matches!(
        point_definition,
        Some(SketchConstraintDefinitionInput::VerticalDistance { .. })
    ));
}

#[test]
fn symmetric_parallel_line_dimension_uses_twice_the_carrier_gap() {
    let entity = |id: &str, geometry| {
        cadmpeg_ir::sketches::SketchEntity::new(
            SketchEntityId::mint(id).unwrap(),
            SketchId::mint("generated:test:sketch#symmetric-distance").unwrap(),
            geometry,
        )
    };
    let first = entity(
        "generated:test:line#first",
        SketchGeometry::try_from(SketchGeometryDefinition::Line {
            start: Point2::new(0.0, 0.0),
            end: Point2::new(0.0, 10.0),
        })
        .unwrap(),
    );
    let second = entity(
        "generated:test:line#second",
        SketchGeometry::try_from(SketchGeometryDefinition::Line {
            start: Point2::new(5.0, 2.0),
            end: Point2::new(5.0, 8.0),
        })
        .unwrap(),
    );
    let parameter = parse_design_parameter_record(&parameter_record(
        Some(44),
        "value",
        "Linear Dimension-3",
        Some("mm"),
        "d1",
        1.0,
    ))
    .expect("symmetric line-width parameter");
    let parameter_id =
        cadmpeg_ir::features::ParameterId::mint("generated:test:parameter#symmetric")
            .expect("identity grammar");

    assert!(matches!(
        crate::design::dimensions::symmetric_parallel_line_dimension_definition(&first, &second, (1, 1), &parameter, parameter_id.clone(), 1.0e-6),
        Some(SketchConstraintDefinitionInput::Distance { entities, parameter: actual })
            if entities == vec![first.id().clone(), second.id().clone()] && actual == parameter_id
    ));

    let mut direct_parameter = parameter.clone();
    direct_parameter.try_set_evaluated_value(0.5).unwrap();
    assert!(
        crate::design::dimensions::symmetric_parallel_line_dimension_definition(
            &first,
            &second,
            (1, 1),
            &direct_parameter,
            parameter_id.clone(),
            1.0e-6
        )
        .is_none()
    );
    assert!(
        crate::design::dimensions::symmetric_parallel_line_dimension_definition(
            &first,
            &second,
            (0, 1),
            &parameter,
            parameter_id,
            1.0e-6
        )
        .is_none()
    );
}

#[test]
fn counted_linear_graph_selects_one_parameter_backed_direction() {
    let entity = |id: &str, position| {
        cadmpeg_ir::sketches::SketchEntity::new(
            SketchEntityId::mint(id).unwrap(),
            SketchId::mint("generated:test:sketch#0").unwrap(),
            SketchGeometry::try_from(SketchGeometryDefinition::Point { position }).unwrap(),
        )
    };
    let first = entity("generated:test:point#first", Point2::new(4.0, 16.0));
    let second = entity("generated:test:point#second", Point2::new(4.0, 14.0));
    let parameter = cadmpeg_ir::features::ParameterId::mint("generated:test:parameter#distance")
        .expect("identity grammar");

    let definition =
        directional_point_dimension(&[&first, &second], 2.0, parameter.clone(), 0.0).unwrap();
    assert!(matches!(
        definition,
        SketchConstraintDefinitionInput::VerticalDistance {
            first: cadmpeg_ir::sketches::SketchLocus::Entity(ref first_id),
            second: cadmpeg_ir::sketches::SketchLocus::Entity(ref second_id),
            parameter: ref parameter_id,
        } if first_id == first.id() && second_id == second.id() && parameter_id == &parameter
    ));
    assert!(directional_point_dimension(&[&first, &second], 3.0, parameter, 0.0).is_none());

    let diagonal = entity("generated:test:point#diagonal", Point2::new(7.0, 14.0));
    assert!(matches!(
        directional_point_dimension(
            &[&first, &diagonal],
            3.0,
            cadmpeg_ir::features::ParameterId::mint("generated:test:parameter#horizontal")
                .expect("identity grammar"),
            0.0
        ),
        Some(SketchConstraintDefinitionInput::HorizontalDistance { .. })
    ));
    let square = entity("generated:test:point#square", Point2::new(6.0, 18.0));
    assert!(directional_point_dimension(
        &[&first, &square],
        2.0,
        cadmpeg_ir::features::ParameterId::mint("generated:test:parameter#ambiguous")
            .expect("identity grammar"),
        0.0
    )
    .is_none());
}

#[test]
fn unclassified_two_locus_linear_group_is_parameter_backed_distance() {
    let entity = |id: &str, geometry| {
        cadmpeg_ir::sketches::SketchEntity::new(
            SketchEntityId::mint(id).unwrap(),
            SketchId::mint("generated:test:sketch#0").unwrap(),
            geometry,
        )
    };
    let point = entity(
        "generated:test:point#dimension",
        SketchGeometry::try_from(SketchGeometryDefinition::Point {
            position: Point2::new(0.0, 0.0),
        })
        .unwrap(),
    );
    let line = entity(
        "generated:test:line#dimension",
        SketchGeometry::try_from(SketchGeometryDefinition::Line {
            start: Point2::new(-10.0, 0.0),
            end: Point2::new(-50.0, 0.0),
        })
        .unwrap(),
    );
    let parameter = cadmpeg_ir::features::ParameterId::mint("generated:test:parameter#distance")
        .expect("identity grammar");

    assert!(exact_counted_dimension_relation(&[&point, &line]).is_none());
    assert!(matches!(
        two_locus_distance_dimension(&[&point, &line], parameter.clone()),
        Some(SketchConstraintDefinitionInput::Distance {
            ref entities,
            parameter: ref actual_parameter,
        }) if entities == &[point.id().clone(), line.id().clone()] && actual_parameter == &parameter
    ));
}

#[test]
fn counted_linear_graph_projects_exact_auxiliary_relations() {
    let entity = |id: &str, geometry| {
        cadmpeg_ir::sketches::SketchEntity::new(
            SketchEntityId::mint(id).unwrap(),
            SketchId::mint("generated:test:sketch#0").unwrap(),
            geometry,
        )
    };
    let horizontal = entity(
        "generated:test:line#horizontal",
        SketchGeometry::try_from(SketchGeometryDefinition::Line {
            start: Point2::new(0.0, 0.0),
            end: Point2::new(10.0, 0.0),
        })
        .unwrap(),
    );
    let vertical = entity(
        "generated:test:line#vertical",
        SketchGeometry::try_from(SketchGeometryDefinition::Line {
            start: Point2::new(0.0, -2.0),
            end: Point2::new(0.0, 2.0),
        })
        .unwrap(),
    );
    let parallel = entity(
        "generated:test:line#parallel",
        SketchGeometry::try_from(SketchGeometryDefinition::Line {
            start: Point2::new(0.0, 2.0),
            end: Point2::new(10.0, 2.0),
        })
        .unwrap(),
    );
    let point = entity(
        "generated:test:point#on-line",
        SketchGeometry::try_from(SketchGeometryDefinition::Point {
            position: Point2::new(4.0, 0.0),
        })
        .unwrap(),
    );
    let duplicate_point = entity(
        "generated:test:point#duplicate",
        SketchGeometry::try_from(SketchGeometryDefinition::Point {
            position: Point2::new(4.0, 0.0),
        })
        .unwrap(),
    );
    let arc = entity(
        "generated:test:arc#bounded",
        SketchGeometry::try_from(SketchGeometryDefinition::Arc {
            center: Point2::new(3.0, 0.0),
            radius: cadmpeg_ir::scalar::Length::new(1.0).unwrap(),
            start_angle: cadmpeg_ir::scalar::Angle::new(0.0).unwrap(),
            end_angle: cadmpeg_ir::scalar::Angle::new(std::f64::consts::FRAC_PI_2).unwrap(),
        })
        .unwrap(),
    );
    let arc_start = entity(
        "generated:test:point#arc-start",
        SketchGeometry::try_from(SketchGeometryDefinition::Point {
            position: Point2::new(4.0, 0.0),
        })
        .unwrap(),
    );
    let outside_arc = entity(
        "generated:test:point#outside-arc",
        SketchGeometry::try_from(SketchGeometryDefinition::Point {
            position: Point2::new(2.0, 0.0),
        })
        .unwrap(),
    );

    assert!(matches!(
        exact_counted_dimension_relation(&[&horizontal, &vertical],),
        Some(SketchConstraintDefinitionInput::Perpendicular { .. })
    ));
    assert!(matches!(
        exact_counted_dimension_relation(&[&horizontal, &parallel],),
        Some(SketchConstraintDefinitionInput::Parallel { .. })
    ));
    assert!(matches!(
        exact_counted_dimension_relation(&[&horizontal, &point],),
        Some(SketchConstraintDefinitionInput::Coincident { .. })
    ));
    assert!(matches!(
        exact_counted_dimension_relation(&[&point, &duplicate_point],),
        Some(SketchConstraintDefinitionInput::Coincident { .. })
    ));
    assert!(matches!(
        exact_counted_dimension_relation(&[&arc_start, &arc],),
        Some(SketchConstraintDefinitionInput::Coincident { .. })
    ));
    assert!(exact_counted_dimension_relation(&[&outside_arc, &arc]).is_none());
}

#[test]
fn exact_pair_suppresses_counted_frames_in_its_containing_companion() {
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
    assert_eq!(constraints[0].driving, None);

    // The parameter of a driven dimension makes its constraint driven.
    let driven = parameter
        .clone()
        .with_flags(crate::records::parameters::DesignParameterFlags {
            driven: true,
            favorite: false,
        })
        .unwrap();
    let driven_constraints = project_dimension_constraints(
        &crate::design::dimensions::DimensionConstraintInputs {
            placements: std::slice::from_ref(&placement),
            parameters: std::slice::from_ref(&driven),
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
    assert_eq!(driven_constraints.len(), 1);
    assert_eq!(driven_constraints[0].driving, Some(false));

    let spatial_sketch = SpatialSketch {
        id: neutral_spatial_sketch_id(&placement),
        name: None,
        configuration: None,
        visible: None,
        profiles: Vec::new(),
        native_ref: Some(placement.id.clone()),
    };
    let spatial_entities = points
        .iter()
        .map(|point| {
            cadmpeg_ir::sketches::SpatialSketchEntity::new(
                cadmpeg_ir::sketches::SpatialSketchEntityId::mint(format!(
                    "synthetic:test:id#spatial-point-{}",
                    point.record_index
                ))
                .unwrap(),
                spatial_sketch.id.clone(),
                cadmpeg_ir::sketches::SpatialSketchGeometry::try_from(
                    cadmpeg_ir::sketches::SpatialSketchGeometryDefinition::Point {
                        position: Point3::new(0.0, point.coordinates().v, 0.0),
                    },
                )
                .unwrap(),
            )
            .with_native_ref(Some(point.id.clone()))
        })
        .collect::<Vec<_>>();
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
    let spatial_constraints = project_spatial_dimension_constraints(
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
            entities: &[],
        },
        std::slice::from_ref(&spatial_sketch),
        &spatial_entities,
    );
    assert_eq!(spatial_constraints.len(), 1, "{spatial_constraints:#?}");
    assert!(matches!(
        &spatial_constraints[0],
        cadmpeg_ir::sketches::SpatialSketchConstraint {
            sketch: actual_sketch,
            definition,
            ..
        } if matches!(definition.kind(), SpatialSketchConstraintDefinitionInput::PointDistance { first, second, parameter: actual_parameter } if actual_sketch == &spatial_sketch.id
            && actual_parameter == &neutral_parameter_id_parts(stream, 20)
            && first == spatial_entities[0].id()
            && second == spatial_entities[1].id())
    ));

    let axis_record = SketchCurveIdentity {
        id: format!("{stream}:sketch-curve#42"),
        record_index: 42,
        owner_reference: Some(100),
        class_tag: crate::records::references::DesignClassTag::try_from("300".to_owned()).unwrap(),
        byte_offset: 0,
        geometry_offset: 0,
        entity_genesis: None,
        primary_id: std::num::NonZeroU64::new(42).unwrap(),
        secondary_id: 0,
        geometry: None,
        planar_z_noise: None,
    };
    let axis_entity = cadmpeg_ir::sketches::SpatialSketchEntity::new(
        cadmpeg_ir::sketches::SpatialSketchEntityId::mint("synthetic:test:id#spatial-axis")
            .unwrap(),
        spatial_sketch.id.clone(),
        cadmpeg_ir::sketches::SpatialSketchGeometry::try_from(
            cadmpeg_ir::sketches::SpatialSketchGeometryDefinition::Line {
                start: Point3::new(-1.0, 1.0, 0.0),
                end: Point3::new(1.0, 1.0, 0.0),
            },
        )
        .unwrap(),
    )
    .with_construction(true)
    .with_native_ref(Some(axis_record.id.clone()));
    let symmetry_group = DesignDimensionLocusGroup {
        id: format!("{stream}:design-dimension-locus-group#31"),
        companion_record_index: 22,
        byte_offset: 0,
        class_tag: crate::records::references::DesignClassTag::try_from("277".to_owned()).unwrap(),
        record_index: 31,
        frame_length: 100,
        loci: vec![
            DesignDimensionLocus {
                returned: crate::records::identity::Located {
                    value: 40,
                    offset: 0,
                },
                geometry_record_index: 42,
                geometry_reference_offset: 0,
                role: 5,
                role_offset: 0,
            },
            DesignDimensionLocus {
                returned: crate::records::identity::Located {
                    value: 41,
                    offset: 0,
                },
                geometry_record_index: 40,
                geometry_reference_offset: 0,
                role: 0,
                role_offset: 0,
            },
            DesignDimensionLocus {
                returned: crate::records::identity::Located {
                    value: 42,
                    offset: 0,
                },
                geometry_record_index: 41,
                geometry_reference_offset: 0,
                role: 0,
                role_offset: 0,
            },
        ],
        owner_reference: 100,
        owner_reference_offset: 0,
        owner_role: 0x400,
        owner_role_offset: 0,
        state: 0,
        state_offset: 0,
        next_class_tag: crate::records::references::DesignClassTag::try_from("273".to_owned())
            .unwrap(),
        next_record_index: 32,
        next_byte_offset: 0,
    };
    let mut symmetry_parameter = parameter.clone();
    symmetry_parameter
        .try_set_source(
            crate::records::parameters::DesignParameterSource::new(
                "Linear Dimension-6".into(),
                symmetry_parameter.owner_record_index(),
                symmetry_parameter.family_discriminator(),
            )
            .unwrap(),
        )
        .unwrap();
    let mut symmetry_entities = spatial_entities.clone();
    symmetry_entities.push(axis_entity.clone());
    let symmetry_constraints = project_spatial_dimension_constraints(
        &crate::design::dimensions::DimensionConstraintInputs {
            placements: std::slice::from_ref(&placement),
            parameters: std::slice::from_ref(&symmetry_parameter),
            owners: std::slice::from_ref(&owner),
            pairs: &[],
            groups: std::slice::from_ref(&symmetry_group),
            annotation_frames: &[],
            null_pairs: &[],
            companions: std::slice::from_ref(&companion),
            recipe_records: &[],
            points: &points,
            curves: std::slice::from_ref(&axis_record),
            entities: &[],
        },
        std::slice::from_ref(&spatial_sketch),
        &symmetry_entities,
    );
    assert_eq!(symmetry_constraints.len(), 2, "{symmetry_constraints:#?}");
    assert!(symmetry_constraints.iter().any(|constraint| matches!(
        constraint.definition.kind(),
        SpatialSketchConstraintDefinitionInput::Symmetric {
            first,
            second,
            axis,
        } if first == spatial_entities[0].id()
            && second == spatial_entities[1].id()
            && axis == axis_entity.id()
    )));
    assert!(symmetry_constraints.iter().any(|constraint| matches!(
        constraint.definition.kind(),
        SpatialSketchConstraintDefinitionInput::Native {
            parameter: Some(_),
            ..
        }
    )));

    let mut zero_parameter = parameter;
    zero_parameter.try_set_evaluated_value(0.0).unwrap();
    let mut duplicate_pair = pair.clone().into_draft();
    duplicate_pair.loci[1].geometry_record_index = duplicate_pair.loci[0].geometry_record_index;
    let duplicate_pair = DesignDimensionLocusPair::try_new(duplicate_pair).unwrap();
    let duplicate = project_dimension_constraints(
        &crate::design::dimensions::DimensionConstraintInputs {
            placements: std::slice::from_ref(&placement),
            parameters: std::slice::from_ref(&zero_parameter),
            owners: std::slice::from_ref(&owner),
            pairs: std::slice::from_ref(&duplicate_pair),
            groups: &[],
            annotation_frames: &[],
            null_pairs: &[],
            companions: &[],
            recipe_records: &[],
            points: &points,
            curves: &[],
            entities: &entities,
        },
        &[],
    );
    assert_eq!(duplicate.len(), 1);
    assert!(matches!(
        duplicate[0].definition.kind(),
        SketchConstraintDefinitionInput::Native { ref operands, .. }
            if operands.iter().map(|operand| (operand.field.as_ref().map(|field| field.name.as_str()), operand.field.as_ref().and_then(|field| field.role), operand.object_index)).collect::<Vec<_>>()
                == [
                    (Some("first_locus"), Some(7), Some(40)),
                    (Some("second_locus"), Some(8), Some(40)),
                ]
    ));

    let mut group_owner = owner.clone();
    {
        let mut wire =
            crate::records::parameters::DesignParameterOwnerWire::from(group_owner.clone());
        wire.companion_record_index = group.companion_record_index;
        wire.record_index = group.companion_record_index - 1;
        wire.parameter_record_index = group.companion_record_index - 2;
        wire.id = format!("{stream}:design-parameter-owner#{}", wire.record_index);
        group_owner = crate::records::parameters::DesignParameterOwner::try_from(wire).unwrap();
    }
    let mut group_parameter = zero_parameter.clone();
    group_parameter.record_index = group_owner.parameter_record_index();
    group_parameter.id = format!("{stream}:design-parameter#{}", group_parameter.record_index);
    group_parameter
        .try_set_source(
            crate::records::parameters::DesignParameterSource::new(
                zero_parameter.source_kind_str().into(),
                Some(group_owner.record_index()),
                zero_parameter.family_discriminator(),
            )
            .unwrap(),
        )
        .unwrap();
    let combined = project_dimension_constraints(
        &crate::design::dimensions::DimensionConstraintInputs {
            placements: std::slice::from_ref(&placement),
            parameters: &[zero_parameter, group_parameter.clone()],
            owners: &[owner, group_owner.clone()],
            pairs: std::slice::from_ref(&pair),
            groups: std::slice::from_ref(&group),
            annotation_frames: &[],
            null_pairs: &[],
            companions: &[],
            recipe_records: &[],
            points: &points,
            curves: &[],
            entities: &entities,
        },
        &[],
    );
    assert_eq!(combined.len(), 2);

    let grouped = project_dimension_constraints(
        &crate::design::dimensions::DimensionConstraintInputs {
            placements: std::slice::from_ref(&placement),
            parameters: std::slice::from_ref(&group_parameter),
            owners: std::slice::from_ref(&group_owner),
            pairs: &[],
            groups: std::slice::from_ref(&group),
            annotation_frames: &[],
            null_pairs: &[],
            companions: &[],
            recipe_records: &[],
            points: &points,
            curves: &[],
            entities: &entities,
        },
        &[],
    );
    assert!(matches!(
        grouped.as_slice(),
        [cadmpeg_ir::sketches::SketchConstraint {
            definition,
            ..
        }] if matches!(definition.kind(), SketchConstraintDefinitionInput::Native {
                native_state: Some(0),
                native_flags: None,
                operands,
                ..
            } if operands.iter().map(|operand| (operand.field.as_ref().map(|field| field.name.as_str()), operand.field.as_ref().and_then(|field| field.role), operand.object_index)).collect::<Vec<_>>()
            == [
                (Some("locus"), Some(0), Some(40)),
                (Some("owner"), Some(0), Some(100)),
                (Some("return"), None, Some(40)),
            ]
    )));

    let mut indirect_group = group;
    indirect_group.owner_reference = 999;
    let indirect = project_dimension_constraints(
        &crate::design::dimensions::DimensionConstraintInputs {
            placements: std::slice::from_ref(&placement),
            parameters: std::slice::from_ref(&group_parameter),
            owners: std::slice::from_ref(&group_owner),
            pairs: &[],
            groups: std::slice::from_ref(&indirect_group),
            annotation_frames: &[],
            null_pairs: &[],
            companions: &[],
            recipe_records: &[],
            points: &points,
            curves: &[],
            entities: &entities,
        },
        &[],
    );
    assert_eq!(indirect.len(), 1);
    assert_eq!(indirect[0].sketch, sketch);
}

#[test]
fn repeated_linear_dimension_requires_disjoint_measurement_pairs() {
    use cadmpeg_ir::features::ParameterId;
    use cadmpeg_ir::sketches::{
        SketchConstraintDefinitionInput as Definition, SketchDistanceMeasurement as Measurement,
        SketchEntityId, SketchLocus,
    };

    let entity =
        |name: &str| SketchEntityId::mint(format!("generated:test:entity#{name}")).unwrap();
    let parameter =
        ParameterId::mint("synthetic:test:id#generated:distance").expect("identity grammar");
    let horizontal = |first: &str, second: &str| Definition::HorizontalDistance {
        first: SketchLocus::Entity(entity(first)),
        second: SketchLocus::Entity(entity(second)),
        parameter: parameter.clone(),
    };
    let candidates = vec![horizontal("a", "b"), horizontal("c", "d")];
    let Definition::RepeatedDistance {
        measurements,
        parameter: actual,
    } = repeated_linear_dimension(&candidates, parameter.clone()).unwrap()
    else {
        panic!("expected repeated distance")
    };
    assert_eq!(actual, parameter);
    assert!(matches!(
        measurements.as_slice(),
        [
            Measurement::Horizontal { .. },
            Measurement::Horizontal { .. }
        ]
    ));

    let ambiguous = vec![horizontal("a", "b"), horizontal("a", "c")];
    assert!(repeated_linear_dimension(&ambiguous, parameter).is_none());
}

mod spatial_distance;

mod presentation_frames;
