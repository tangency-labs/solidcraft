// SPDX-License-Identifier: Apache-2.0
use cadmpeg_test_support::edit;

use crate::design::decode::parameters::parse_design_parameter_record;
use crate::design::dimensions::owner_scoped_angular_dimension_definition;
use crate::design::dimensions::owner_scoped_line_length_dimension_definition;
use crate::design::dimensions::owner_scoped_radial_dimension_definition;
use crate::design::dimensions::preceding_incident_angular_dimension_definition;
use crate::design::dimensions::radial_dimension_definition;
use crate::design::dimensions::radial_extension_annotation_group;
use crate::design::dimensions::radial_locus_dimension_definition;
use crate::design::dimensions::spatial_parallel_line_distance_matches;
use crate::design::dimensions::spatial_point_distance_matches;
use crate::design::dimensions::unique_point_class_dimension_definition;
use crate::design::test_support::parameter_record;
use crate::records::sketch_geometry::SketchCurveGeometry;
use crate::records::sketch_geometry::SketchCurveIdentity;
use crate::records::sketch_geometry::SketchPoint;
use cadmpeg_ir::features::ParameterId;
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::math::Vector3;
use cadmpeg_ir::scalar::Length;
use cadmpeg_ir::sketches::SketchConstraintDefinitionInput;
use cadmpeg_ir::sketches::SketchEntity;
use cadmpeg_ir::sketches::SketchEntityId;
use cadmpeg_ir::sketches::SketchGeometry;
use cadmpeg_ir::sketches::SketchGeometryDefinition;
use cadmpeg_ir::sketches::SketchId;
use cadmpeg_ir::sketches::SketchLocus;
use std::collections::HashMap;

#[test]
fn spatial_dimension_matchers_refuse_infinite_measured_distances() {
    use cadmpeg_ir::sketches::{
        SpatialSketchEntity, SpatialSketchEntityId, SpatialSketchGeometry,
        SpatialSketchGeometryDefinition, SpatialSketchId,
    };

    let sketch = SpatialSketchId::mint("synthetic:test:spatial-sketch#large-lines")
        .expect("valid sketch identity");
    let parameter = parse_design_parameter_record(&parameter_record(
        Some(1),
        "4 mm",
        "Linear Dimension-2",
        Some("mm"),
        "d1",
        0.4,
    ))
    .expect("linear parameter");
    let parameter_id = ParameterId::mint("synthetic:test:parameter#large-lines")
        .expect("valid parameter identity");
    let line = |name: &str, start: Point3, end: Point3| {
        SpatialSketchEntity::new(
            SpatialSketchEntityId::mint(format!("synthetic:test:spatial-line#{name}"))
                .expect("valid line identity"),
            sketch.clone(),
            SpatialSketchGeometry::try_from(SpatialSketchGeometryDefinition::Line { start, end })
                .expect("finite separated endpoints"),
        )
    };
    let diagonal = line(
        "diagonal",
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(1.0e308, 1.0e308, 0.0),
    );
    assert!(
        crate::design::dimensions::owner_scoped_spatial_line_length_dimension_definition(
            std::slice::from_ref(&diagonal),
            &sketch,
            &parameter,
            &parameter_id,
            0.0
        )
        .is_none()
    );

    let first_start = Point3::new(0.0, 0.0, 0.0);
    let first_end = Point3::new(1.0e308, 0.0, 0.0);
    let second_start = Point3::new(0.0, 1.0e308, 0.0);
    let second_end = Point3::new(1.0e308, 1.0e308, 0.0);
    let entities = [
        line("first-a", first_start, first_end),
        line("first-b", first_start, first_end),
        line("second-a", second_start, second_end),
        line("second-b", second_start, second_end),
    ];
    assert_eq!(
        crate::design::dimensions::spatial_parallel_line_span_distance(
            &entities[0].geometry,
            &entities[2].geometry,
            0.0,
        ),
        Some(f64::INFINITY)
    );
    assert!(
        crate::design::dimensions::owner_scoped_spatial_parallel_line_set_dimension_definition(
            &entities,
            &sketch,
            &parameter,
            &parameter_id,
            0.0
        )
        .is_none()
    );
}

#[test]
fn spatial_line_distance_requires_parallel_geometry_and_exact_value() {
    use cadmpeg_ir::sketches::SpatialSketchGeometry;
    use cadmpeg_ir::sketches::SpatialSketchGeometryDefinition::Line;

    let first = SpatialSketchGeometry::try_from(Line {
        start: Point3::new(0.0, 0.0, 0.0),
        end: Point3::new(0.0, 10.0, 0.0),
    })
    .unwrap();
    let second = SpatialSketchGeometry::try_from(Line {
        start: Point3::new(3.0, 0.0, 4.0),
        end: Point3::new(3.0, -5.0, 4.0),
    })
    .unwrap();
    let crossing = SpatialSketchGeometry::try_from(Line {
        start: Point3::new(0.0, 0.0, 0.0),
        end: Point3::new(1.0, 0.0, 0.0),
    })
    .unwrap();

    assert!(spatial_parallel_line_distance_matches(&first, &second, 5.0));
    assert!(!spatial_parallel_line_distance_matches(
        &first, &second, 4.0
    ));
    assert!(!spatial_parallel_line_distance_matches(
        &first, &crossing, 0.0
    ));
}

#[test]
fn spatial_point_distance_requires_point_geometry_and_exact_value() {
    use cadmpeg_ir::sketches::SpatialSketchGeometry;
    use cadmpeg_ir::sketches::SpatialSketchGeometryDefinition::{Line, Point};

    let first = SpatialSketchGeometry::try_from(Point {
        position: Point3::new(1.0, 2.0, 3.0),
    })
    .unwrap();
    let second = SpatialSketchGeometry::try_from(Point {
        position: Point3::new(4.0, 6.0, 3.0),
    })
    .unwrap();
    let line = SpatialSketchGeometry::try_from(Line {
        start: Point3::new(1.0, 2.0, 3.0),
        end: Point3::new(4.0, 6.0, 3.0),
    })
    .unwrap();

    assert!(spatial_point_distance_matches(&first, &second, 5.0));
    assert!(!spatial_point_distance_matches(&first, &second, 4.0));
    assert!(!spatial_point_distance_matches(&first, &line, 5.0));
}

#[test]
fn owner_scoped_radial_dimensions_preserve_repeated_measurements() {
    let mut entity = SketchEntity::new(
        SketchEntityId::mint("f3d:model:sketch-entity#circle").unwrap(),
        SketchId::mint("f3d:model:sketch#radial").unwrap(),
        SketchGeometry::try_from(SketchGeometryDefinition::Circle {
            center: Point2::new(2.0, 3.0),
            radius: Length::new(5.0).unwrap(),
        })
        .unwrap(),
    );
    let radius_parameter =
        cadmpeg_ir::features::ParameterId::mint("synthetic:test:parameter#radius")
            .expect("identity grammar");
    assert!(matches!(
        radial_dimension_definition(&entity, "Radius Dimension-2", 0.5, radius_parameter.clone()),
        Some(SketchConstraintDefinitionInput::Radius { entity: ref actual, parameter: ref p })
            if actual == entity.id() && p == &radius_parameter
    ));
    assert!(matches!(
        radial_dimension_definition(&entity, "Radial Dimension-3", 0.5, radius_parameter.clone()),
        Some(SketchConstraintDefinitionInput::Radius { entity: ref actual, .. })
            if actual == entity.id()
    ));
    let diameter_parameter =
        cadmpeg_ir::features::ParameterId::mint("synthetic:test:parameter#diameter")
            .expect("identity grammar");
    assert!(matches!(
        radial_dimension_definition(&entity, "Diameter Dimension-2", 1.0, diameter_parameter.clone()),
        Some(SketchConstraintDefinitionInput::Diameter { entity: ref actual, parameter: ref p })
            if actual == entity.id() && p == &diameter_parameter
    ));
    assert!(radial_dimension_definition(
        &entity,
        "Diameter Dimension-2",
        0.5,
        diameter_parameter.clone()
    )
    .is_none());
    let parameter = parse_design_parameter_record(&parameter_record(
        Some(1),
        "10 mm",
        "Diameter Dimension-2",
        Some("mm"),
        "d1",
        1.0,
    ))
    .expect("diameter parameter");
    assert!(matches!(
        owner_scoped_radial_dimension_definition(std::slice::from_ref(&entity), &entity.sketch, &parameter, &diameter_parameter, 1.0e-6),
        Some(SketchConstraintDefinitionInput::Diameter {
            entity: ref actual,
            ..
        }) if actual == entity.id()
    ));
    let mut duplicate = SketchEntity::new(
        SketchEntityId::mint("f3d:model:sketch-entity#duplicate-circle").unwrap(),
        entity.sketch.clone(),
        entity.geometry.clone(),
    )
    .with_construction(entity.construction)
    .with_native_ref(entity.native_ref.clone())
    .with_geometry_ref(entity.geometry_ref.clone())
    .with_endpoint_refs(entity.endpoint_refs.clone());
    edit::replace(&mut duplicate.geometry, |previous| {
        let mut definition = previous.definition().to_raw();
        {
            const RADIUS_PERTURBATION: f64 = 5.0e-7;

            let definition: &mut cadmpeg_ir::sketches::SketchGeometryDefinition = &mut definition;

            let SketchGeometryDefinition::Circle { radius, .. } = definition else {
                unreachable!("test entity is circular")
            };
            *radius = cadmpeg_ir::scalar::Length::new(radius.get() + RADIUS_PERTURBATION).unwrap();
        };
        definition.try_into()
    })
    .unwrap();
    assert!(matches!(
        owner_scoped_radial_dimension_definition(&[entity.clone(), duplicate.clone()], &entity.sketch, &parameter, &diameter_parameter, 1.0e-6),
        Some(SketchConstraintDefinitionInput::RepeatedDiameter {
            entities,
            parameter,
        }) if entities == vec![entity.id().clone(), duplicate.id().clone()]
            && parameter == diameter_parameter
    ));

    let radial_parameter = parse_design_parameter_record(&parameter_record(
        Some(1),
        "5 mm",
        "Radial Dimension-2",
        Some("mm"),
        "d2",
        0.5,
    ))
    .expect("radial parameter");
    assert!(matches!(
        owner_scoped_radial_dimension_definition(&[entity.clone(), duplicate.clone()], &entity.sketch, &radial_parameter, &radius_parameter, 1.0e-6),
        Some(SketchConstraintDefinitionInput::RepeatedRadius {
            entities,
            parameter,
        }) if entities == vec![entity.id().clone(), duplicate.id().clone()]
            && parameter == radius_parameter
    ));

    entity.geometry = SketchGeometry::try_from(SketchGeometryDefinition::Arc {
        center: Point2::new(2.0, 3.0),
        radius: Length::new(5.0).unwrap(),
        start_angle: cadmpeg_ir::scalar::Angle::new(0.0).unwrap(),
        end_angle: cadmpeg_ir::scalar::Angle::new(1.0).unwrap(),
    })
    .unwrap();
    assert!(
        radial_dimension_definition(&entity, "Diameter Dimension", 1.0, diameter_parameter)
            .is_some()
    );
    entity.geometry = SketchGeometry::try_from(SketchGeometryDefinition::Ellipse {
        center: Point2::new(2.0, 3.0),
        major_angle: cadmpeg_ir::scalar::Angle::new(0.0).unwrap(),
        radii: cadmpeg_ir::sketches::EllipseRadii {
            major_radius: Length::new(5.0).unwrap(),
            minor_radius: Length::new(3.0).unwrap(),
        },
        bounds: None,
    })
    .unwrap();
    assert!(
        radial_dimension_definition(&entity, "Radius Dimension-2", 0.5, radius_parameter).is_none()
    );
}

#[test]
fn owner_scoped_line_lengths_preserve_repeated_entities() {
    let sketch = SketchId::mint("f3d:model:sketch#line-length").unwrap();
    let line = |name: &str, v: f64, length: f64| {
        SketchEntity::new(
            SketchEntityId::mint(format!("f3d:model:sketch-entity#{name}")).unwrap(),
            sketch.clone(),
            SketchGeometry::try_from(SketchGeometryDefinition::Line {
                start: Point2::new(0.0, v),
                end: Point2::new(length, v),
            })
            .unwrap(),
        )
    };
    let first = line("first", 0.0, 4.0);
    let second = line("second", 2.0, 4.0 + 5.0e-7);
    let parameter = parse_design_parameter_record(&parameter_record(
        Some(1),
        "4 mm",
        "Linear Dimension-2",
        Some("mm"),
        "d1",
        0.4,
    ))
    .expect("linear parameter");
    let parameter_id =
        cadmpeg_ir::features::ParameterId::mint("synthetic:test:parameter#line-length")
            .expect("identity grammar");

    assert!(matches!(
        owner_scoped_line_length_dimension_definition(std::slice::from_ref(&first), &sketch, &parameter, &parameter_id, 1.0e-6),
        Some(SketchConstraintDefinitionInput::DistanceLoci {
            first: SketchLocus::Start(ref entity),
            second: SketchLocus::End(ref other),
            parameter: ref actual_parameter,
        }) if entity == first.id() && other == first.id() && actual_parameter == &parameter_id
    ));
    assert!(matches!(
        owner_scoped_line_length_dimension_definition(&[first.clone(), second.clone()], &sketch, &parameter, &parameter_id, 1.0e-6),
        Some(SketchConstraintDefinitionInput::RepeatedLength {
            entities,
            parameter,
        }) if entities == vec![first.id().clone(), second.id().clone()]
            && parameter == parameter_id
    ));
}

#[test]
fn owner_scoped_angular_dimension_requires_one_matching_line_pair() {
    let sketch = SketchId::mint("f3d:model:sketch#angular").unwrap();
    let line = |name: &str, angle: f64| {
        SketchEntity::new(
            SketchEntityId::mint(format!("f3d:model:sketch-entity#{name}")).unwrap(),
            sketch.clone(),
            SketchGeometry::try_from(SketchGeometryDefinition::Line {
                start: Point2::new(0.0, 0.0),
                end: Point2::new(angle.cos(), angle.sin()),
            })
            .unwrap(),
        )
    };
    let horizontal = line("horizontal", 0.0);
    let sloped = line("sloped", std::f64::consts::FRAC_PI_6);
    let vertical = line("vertical", std::f64::consts::FRAC_PI_2);
    let parameter = parse_design_parameter_record(&parameter_record(
        Some(1),
        "30 deg",
        "Angular Dimension-2",
        Some("deg"),
        "d1",
        std::f64::consts::FRAC_PI_6,
    ))
    .expect("angular parameter");
    let parameter_id = cadmpeg_ir::features::ParameterId::mint("synthetic:test:parameter#angle")
        .expect("identity grammar");

    assert!(matches!(
        owner_scoped_angular_dimension_definition(&[horizontal.clone(), sloped.clone(), vertical.clone()], &sketch, &parameter, &parameter_id),
        Some(SketchConstraintDefinitionInput::Angle {
            first,
            second,
            parameter,
        }) if first == horizontal.id().clone() && second == sloped.id().clone() && parameter == parameter_id
    ));

    let other_sloped = line("other-sloped", -std::f64::consts::FRAC_PI_6);
    assert!(owner_scoped_angular_dimension_definition(
        &[horizontal, sloped, vertical, other_sloped],
        &sketch,
        &parameter,
        &parameter_id
    )
    .is_none());
}

#[test]
fn preceding_incident_angular_dimension_excludes_later_symmetric_geometry() {
    let stream = "f3d:A";
    let sketch = SketchId::mint("f3d:model:sketch#angular-incidence").unwrap();
    let curve = |record_index, byte_offset, angle: f64| SketchCurveIdentity {
        id: format!("{stream}:sketch-curve#{record_index}"),
        record_index,
        owner_reference: Some(100),
        class_tag: crate::records::references::DesignClassTag::try_from("301".to_owned()).unwrap(),
        byte_offset,
        geometry_offset: 0,
        entity_genesis: None,
        primary_id: std::num::NonZeroU64::new(u64::from(record_index)).unwrap(),
        secondary_id: 0,
        geometry: Some(
            SketchCurveGeometry::line(
                Point3::new(0.0, 0.0, 0.0),
                Point3::new(angle.cos(), angle.sin(), 0.0),
                Vector3::new(angle.cos(), angle.sin(), 0.0),
                Vector3::new(0.0, 0.0, 1.0),
            )
            .unwrap(),
        ),
        planar_z_noise: None,
    };
    let curves = vec![
        curve(10, 10, 0.0),
        curve(11, 20, 3.0 * std::f64::consts::FRAC_PI_4),
        curve(12, 110, std::f64::consts::FRAC_PI_2),
        curve(13, 120, -std::f64::consts::FRAC_PI_4),
    ];
    let point = |record_index, byte_offset, incident_curves| {
        SketchPoint::try_from(crate::records::sketch_geometry::SketchPointDraft {
            id: format!("{stream}:sketch-point#{record_index}"),
            record_index,
            owner_reference: Some(100),
            class_tag: crate::records::references::DesignClassTag::try_from("300".to_owned())
                .unwrap(),
            byte_offset,
            coordinate_offset: 0,
            companion: crate::records::sketch_geometry::SketchPointCompanion { incident_curves },
            record_form: crate::records::sketch_geometry::SketchPointRecordForm::version11(
                u64::from(record_index),
                crate::records::sketch_geometry::SketchPointClosure::Selector0State0,
                None,
                0.0,
            ),
            paired_reference: 0,
            coordinates: Point2::new(0.0, 0.0),
        })
        .unwrap()
    };
    let points = vec![point(20, 30, vec![10, 11]), point(21, 130, vec![12, 13])];
    let entity = |record_index, angle: f64| {
        SketchEntity::new(
            SketchEntityId::mint(format!("f3d:model:sketch-entity#line-{record_index}")).unwrap(),
            sketch.clone(),
            SketchGeometry::try_from(SketchGeometryDefinition::Line {
                start: Point2::new(0.0, 0.0),
                end: Point2::new(angle.cos(), angle.sin()),
            })
            .unwrap(),
        )
    };
    let entities = [
        entity(10, 0.0),
        entity(11, 3.0 * std::f64::consts::FRAC_PI_4),
        entity(12, std::f64::consts::FRAC_PI_2),
        entity(13, -std::f64::consts::FRAC_PI_4),
    ];
    let projected = HashMap::from([
        ((stream, 10), &entities[0]),
        ((stream, 11), &entities[1]),
        ((stream, 12), &entities[2]),
        ((stream, 13), &entities[3]),
    ]);
    let parameter = crate::design::decode::parameters::parse_design_parameter(&parameter_record(
        Some(1),
        "135 deg",
        "Angular Dimension-2",
        Some("deg"),
        "d1",
        3.0 * std::f64::consts::FRAC_PI_4,
    ))
    .unwrap()
    .expect("angular parameter")
    .into_record("Design/BulkStream.dat", 100)
    .expect("located parameter");
    let parameter_id =
        ParameterId::mint("synthetic:test:parameter#angle").expect("identity grammar");

    assert!(matches!(
        preceding_incident_angular_dimension_definition(stream, &points, &curves, &projected, &sketch, (&parameter, &parameter_id)),
        Some(SketchConstraintDefinitionInput::Angle {
            first,
            second,
            parameter,
        }) if first == entities[0].id().clone() && second == entities[1].id().clone() && parameter == parameter_id
    ));
}

#[test]
fn owner_scoped_point_dimensions_quotient_coincident_identities() {
    let sketch = SketchId::mint("f3d:model:sketch#point-classes").unwrap();
    let point = |name: &str, u: f64, v: f64| {
        SketchEntity::new(
            SketchEntityId::mint(format!("f3d:model:sketch-entity#{name}")).unwrap(),
            sketch.clone(),
            SketchGeometry::try_from(SketchGeometryDefinition::Point {
                position: Point2::new(u, v),
            })
            .unwrap(),
        )
    };
    let lower = point("lower", -53.0, -20.875);
    let lower_duplicate = point("lower-duplicate", -53.0, -20.875 + 5.0e-7);
    let upper = point("upper", -53.0, -7.875);
    let parameter = parse_design_parameter_record(&parameter_record(
        Some(1),
        "13 mm",
        "Linear Dimension-2",
        Some("mm"),
        "d19",
        1.3,
    ))
    .expect("linear parameter");
    let parameter_id =
        cadmpeg_ir::features::ParameterId::mint("synthetic:test:parameter#point-classes")
            .expect("identity grammar");

    assert!(matches!(
        unique_point_class_dimension_definition(&[lower.clone(), lower_duplicate, upper.clone()], &sketch, &parameter, &parameter_id, 1.0e-6),
        Some(SketchConstraintDefinitionInput::VerticalDistance {
            first: SketchLocus::Entity(first),
            second: SketchLocus::Entity(second),
            parameter,
        }) if first == lower.id().clone() && second == upper.id().clone() && parameter == parameter_id
    ));

    let another_upper = point("another-upper", -40.0, -7.875);
    assert!(unique_point_class_dimension_definition(
        &[lower, upper, another_upper],
        &sketch,
        &parameter,
        &parameter_id,
        1.0e-6
    )
    .is_none());
}

#[test]
fn radial_locus_groups_use_direct_curves_then_unique_center_witnesses() {
    let sketch = SketchId::mint("f3d:model:sketch#radial-loci").unwrap();
    let point = |id: &str, u, v| {
        SketchEntity::new(
            SketchEntityId::mint(id).unwrap(),
            sketch.clone(),
            SketchGeometry::try_from(SketchGeometryDefinition::Point {
                position: Point2::new(u, v),
            })
            .unwrap(),
        )
    };
    let circle = |id: &str, u, v, radius| {
        SketchEntity::new(
            SketchEntityId::mint(id).unwrap(),
            sketch.clone(),
            SketchGeometry::try_from(SketchGeometryDefinition::Circle {
                center: Point2::new(u, v),
                radius: Length::new(radius).unwrap(),
            })
            .unwrap(),
        )
    };
    let center = point("synthetic:test:id#center", 2.0, 3.0);
    let annotation = point("synthetic:test:id#annotation", 7.0, 3.0);
    let measured = circle("synthetic:test:id#measured", 2.0, 3.0, 5.0);
    let other_center = circle("synthetic:test:id#other-center", 20.0, 30.0, 5.0);
    let other_radius = circle("synthetic:test:id#other-radius", 2.0, 3.0, 7.0);
    let all = [
        center.clone(),
        annotation.clone(),
        measured.clone(),
        other_center,
        other_radius,
    ];
    let parameter = cadmpeg_ir::features::ParameterId::mint("synthetic:test:parameter#radial-loci")
        .expect("identity grammar");

    assert!(matches!(
        radial_locus_dimension_definition(&[&measured, &annotation], &all, "Radial Dimension-2", 0.5, &parameter),
        Some(SketchConstraintDefinitionInput::Radius { entity, .. }) if entity == measured.id().clone()
    ));
    let repeated = circle("synthetic:test:id#repeated", 12.0, 3.0, 5.0);
    assert!(matches!(
        radial_locus_dimension_definition(&[&measured, &annotation, &repeated], &all, "Diameter Dimension-3", 1.0, &parameter),
        Some(SketchConstraintDefinitionInput::RepeatedDiameter { entities, parameter: actual })
            if entities == vec![measured.id().clone(), repeated.id().clone()] && actual == parameter
    ));
    assert!(matches!(
        radial_locus_dimension_definition(&[&center], &all, "Diameter Dimension-2", 1.0, &parameter),
        Some(SketchConstraintDefinitionInput::Diameter { entity, .. }) if entity == measured.id().clone()
    ));
}

#[test]
fn radial_extension_annotations_require_a_point_on_the_line_carrier() {
    let sketch = SketchId::mint("f3d:model:sketch#radial-extension").unwrap();
    let entity = |id: &str, geometry| {
        SketchEntity::new(SketchEntityId::mint(id).unwrap(), sketch.clone(), geometry)
    };
    let line = entity(
        "synthetic:test:id#line",
        SketchGeometry::try_from(SketchGeometryDefinition::Line {
            start: Point2::new(0.0, 0.0),
            end: Point2::new(6.0, 0.0),
        })
        .unwrap(),
    );
    let extension_point = entity(
        "synthetic:test:id#extension-point",
        SketchGeometry::try_from(SketchGeometryDefinition::Point {
            position: Point2::new(6.5, 0.0),
        })
        .unwrap(),
    );
    let off_carrier = entity(
        "synthetic:test:id#off-carrier",
        SketchGeometry::try_from(SketchGeometryDefinition::Point {
            position: Point2::new(6.5, 0.25),
        })
        .unwrap(),
    );
    let parameter = parse_design_parameter_record(&parameter_record(
        Some(1),
        "5 mm",
        "Radial Dimension-2",
        Some("mm"),
        "d1",
        0.5,
    ))
    .expect("radial parameter");

    assert!(radial_extension_annotation_group(
        &[&extension_point, &line],
        &parameter,
    ));
    assert!(!radial_extension_annotation_group(
        &[&off_carrier, &line],
        &parameter,
    ));

    let mut linear = parameter;
    linear
        .try_set_source(
            crate::records::parameters::DesignParameterSource::new(
                "Linear Dimension-2".into(),
                linear.owner_record_index(),
                linear.family_discriminator(),
            )
            .unwrap(),
        )
        .unwrap();
    assert!(!radial_extension_annotation_group(
        &[&extension_point, &line],
        &linear,
    ));
}

#[test]
fn geometric_membership_rejects_large_residuals_and_short_line_false_positives() {
    use crate::design::dimensions::point_lies_on_sketch_geometry;
    use cadmpeg_ir::sketches::{SpatialSketchGeometry, SpatialSketchGeometryDefinition};
    let point = |x| {
        SpatialSketchGeometry::try_from(SpatialSketchGeometryDefinition::Point {
            position: Point3::new(x, 0.0, 0.0),
        })
        .unwrap()
    };
    assert!(!spatial_point_distance_matches(
        &point(0.0),
        &point(1e200),
        1.0
    ));
    assert!(spatial_point_distance_matches(
        &point(0.0),
        &point(1e200),
        1e200
    ));
    let short_length = 0.000_001;
    for definition in [
        SketchGeometryDefinition::Line {
            start: Point2::new(0.0, 0.0),
            end: Point2::new(short_length, 0.0),
        },
        SketchGeometryDefinition::ReferenceLine {
            origin: Point2::new(0.0, 0.0),
            direction: Point2::new(short_length, 0.0),
        },
    ] {
        let geometry = SketchGeometry::try_from(definition).unwrap();
        assert!(!point_lies_on_sketch_geometry(
            Point2::new(short_length / 2.0, 0.0001),
            &geometry,
        ));
        assert!(point_lies_on_sketch_geometry(
            Point2::new(short_length / 2.0, 0.0),
            &geometry,
        ));
    }
    let ellipse = SketchGeometry::try_from(SketchGeometryDefinition::Ellipse {
        center: Point2::new(0.0, 0.0),
        major_angle: cadmpeg_ir::scalar::Angle::new(0.0).unwrap(),
        radii: cadmpeg_ir::sketches::EllipseRadii {
            major_radius: Length::new(1.0).unwrap(),
            minor_radius: Length::new(0.5).unwrap(),
        },
        bounds: None,
    })
    .unwrap();
    assert!(!point_lies_on_sketch_geometry(
        Point2::new(1e200, 0.0),
        &ellipse,
    ));
    assert!(point_lies_on_sketch_geometry(
        Point2::new(1.0, 0.0),
        &ellipse,
    ));
}

#[test]
fn unique_spatial_parallel_line_pair_projects_their_distance() {
    use cadmpeg_ir::sketches::{
        SpatialSketchConstraintDefinitionInput, SpatialSketchEntity, SpatialSketchEntityId,
        SpatialSketchGeometry, SpatialSketchGeometryDefinition, SpatialSketchId,
    };

    let sketch = SpatialSketchId::mint("synthetic:test:spatial-sketch#parallel").unwrap();
    // 0.2 cm is the 2 mm distance between the two parallel lines.
    let parameter = parse_design_parameter_record(&parameter_record(
        Some(1),
        "2 mm",
        "Linear Dimension-2",
        Some("mm"),
        "d1",
        0.2,
    ))
    .unwrap();
    let parameter_id = ParameterId::mint("synthetic:test:parameter#spatial-parallel").unwrap();
    let line = |index: u32, x0: f64, x1: f64, y: f64| {
        SpatialSketchEntity::new(
            SpatialSketchEntityId::mint(format!("synthetic:test:spatial-line#{index}")).unwrap(),
            sketch.clone(),
            SpatialSketchGeometry::try_from(SpatialSketchGeometryDefinition::Line {
                start: Point3::new(x0, y, 0.0),
                end: Point3::new(x1, y, 0.0),
            })
            .unwrap(),
        )
    };
    let entities = [line(1, 0.0, 4.0, 0.0), line(2, 0.0, 4.0, 2.0)];
    assert_eq!(
        crate::design::dimensions::unique_spatial_parallel_line_dimension_definition(
            &entities,
            &sketch,
            &parameter,
            &parameter_id,
        )
        .unwrap(),
        SpatialSketchConstraintDefinitionInput::ParallelLineDistance {
            first: entities[0].id().clone(),
            second: entities[1].id().clone(),
            parameter: parameter_id.clone(),
        }
    );
}

#[test]
fn repeated_spatial_profile_sides_project_a_repeated_distance() {
    use cadmpeg_ir::sketches::{
        SpatialSketch, SpatialSketchConstraintDefinitionInput, SpatialSketchEntity,
        SpatialSketchEntityId, SpatialSketchEntityPair, SpatialSketchEntityUse,
        SpatialSketchGeometry, SpatialSketchGeometryDefinition, SpatialSketchId,
        SpatialSketchProfile,
    };

    let sketch = SpatialSketchId::mint("synthetic:test:spatial-sketch#repeated").unwrap();
    // 0.2 cm is the 2 mm distance between opposite long sides of each profile.
    let parameter = parse_design_parameter_record(&parameter_record(
        Some(1),
        "2 mm",
        "Linear Dimension-2",
        Some("mm"),
        "d1",
        0.2,
    ))
    .unwrap();
    let parameter_id = ParameterId::mint("synthetic:test:parameter#spatial-repeated").unwrap();
    let mut entities = Vec::new();
    let mut profiles = Vec::new();
    // Two 4 mm by 2 mm rectangles; edges 0 and 2 of each are 2 mm apart and
    // edges 1 and 3 are 4 mm apart.
    for (first_edge, x) in [(0_u32, 0.0), (4, 10.0)] {
        let corners = [
            Point3::new(x, 0.0, 0.0),
            Point3::new(x + 4.0, 0.0, 0.0),
            Point3::new(x + 4.0, 2.0, 0.0),
            Point3::new(x, 2.0, 0.0),
        ];
        let mut boundary = Vec::new();
        for (edge, ordinal) in (first_edge..).zip(0..4) {
            let entity = SpatialSketchEntity::new(
                SpatialSketchEntityId::mint(format!("synthetic:test:spatial-edge#{edge}")).unwrap(),
                sketch.clone(),
                SpatialSketchGeometry::try_from(SpatialSketchGeometryDefinition::Line {
                    start: corners[ordinal],
                    end: corners[(ordinal + 1) % 4],
                })
                .unwrap(),
            );
            boundary.push(SpatialSketchEntityUse {
                entity: entity.id().clone(),
                reversed: false,
            });
            entities.push(entity);
        }
        profiles.push(
            SpatialSketchProfile::try_new(
                corners[0],
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
                boundary,
            )
            .unwrap(),
        );
    }
    let sketches = [SpatialSketch {
        id: sketch.clone(),
        name: None,
        configuration: None,
        visible: None,
        profiles,
        native_ref: None,
    }];
    let pair = |first: usize, second: usize| SpatialSketchEntityPair {
        first: entities[first].id().clone(),
        second: entities[second].id().clone(),
    };
    assert_eq!(
        crate::design::dimensions::owner_scoped_spatial_repeated_profile_line_distance_definition(
            &entities,
            &sketches,
            &sketch,
            &parameter,
            &parameter_id,
        )
        .unwrap(),
        SpatialSketchConstraintDefinitionInput::RepeatedParallelLineDistance {
            pairs: vec![pair(0, 2), pair(4, 6)],
            parameter: parameter_id.clone(),
        }
    );
}
