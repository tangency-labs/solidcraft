// SPDX-License-Identifier: Apache-2.0
use super::{
    counted_role_relation, Angle, Length, Point2, SketchConstraintDefinitionInput, SketchEntityId,
    SketchGeometry, SketchGeometryDefinition, SketchId,
};

#[test]
fn counted_role_relation_matches_roles_and_tangency() {
    let line = |id: &str, start, end| {
        cadmpeg_ir::sketches::SketchEntity::new(
            SketchEntityId::mint(id).unwrap(),
            SketchId::mint("generated:test:sketch#0").unwrap(),
            SketchGeometry::try_from(SketchGeometryDefinition::Line { start, end }).unwrap(),
        )
    };
    let horizontal = line(
        "generated:test:line#horizontal",
        Point2::new(-2.0, 3.0),
        Point2::new(5.0, 3.0),
    );
    let vertical = line(
        "generated:test:line#vertical",
        Point2::new(4.0, -1.0),
        Point2::new(4.0, 8.0),
    );

    assert!(matches!(
        counted_role_relation(&[&horizontal], 0x40),
        Some(SketchConstraintDefinitionInput::Horizontal { entity })
            if &entity == horizontal.id()
    ));
    assert!(matches!(
        counted_role_relation(&[&vertical], 0x80),
        Some(SketchConstraintDefinitionInput::Vertical { entity })
            if &entity == vertical.id()
    ));
    assert!(matches!(
        counted_role_relation(&[&horizontal], 0x40 | 0x800),
        Some(SketchConstraintDefinitionInput::Horizontal { entity }) if &entity == horizontal.id()
    ));
    assert!(counted_role_relation(&[&horizontal], 0x80).is_none());
    assert!(counted_role_relation(&[&horizontal, &vertical], 0x40).is_none());

    let arc = cadmpeg_ir::sketches::SketchEntity::new(
        SketchEntityId::mint("generated:test:arc#tangent").unwrap(),
        horizontal.sketch.clone(),
        SketchGeometry::try_from(SketchGeometryDefinition::Arc {
            center: Point2::new(-2.0, 2.0),
            radius: Length::new(1.0).unwrap(),
            start_angle: Angle::new(std::f64::consts::FRAC_PI_2).unwrap(),
            end_angle: Angle::new(std::f64::consts::PI).unwrap(),
        })
        .unwrap(),
    );
    assert!(matches!(
        counted_role_relation(&[&arc, &horizontal], 0x100),
        Some(SketchConstraintDefinitionInput::Tangent { first, second })
            if &first == arc.id() && &second == horizontal.id()
    ));

    let tangent_arc = cadmpeg_ir::sketches::SketchEntity::new(
        SketchEntityId::mint("generated:test:arc#arc-tangent").unwrap(),
        horizontal.sketch.clone(),
        SketchGeometry::try_from(SketchGeometryDefinition::Arc {
            center: Point2::new(-2.0, 5.0),
            radius: Length::new(2.0).unwrap(),
            start_angle: Angle::new(-std::f64::consts::FRAC_PI_2).unwrap(),
            end_angle: Angle::new(0.0).unwrap(),
        })
        .unwrap(),
    );
    assert!(matches!(
        counted_role_relation(&[&arc, &tangent_arc], 0x100),
        Some(SketchConstraintDefinitionInput::Tangent { first, second })
            if &first == arc.id() && &second == tangent_arc.id()
    ));

    let non_tangent_arc = cadmpeg_ir::sketches::SketchEntity::new(
        SketchEntityId::mint("generated:test:arc#arc-not-tangent").unwrap(),
        tangent_arc.sketch.clone(),
        SketchGeometry::try_from(SketchGeometryDefinition::Arc {
            center: Point2::new(-1.0, 3.0),
            radius: Length::new(1.0).unwrap(),
            start_angle: Angle::new(std::f64::consts::PI).unwrap(),
            end_angle: Angle::new(2.0 * std::f64::consts::PI).unwrap(),
        })
        .unwrap(),
    );
    assert!(counted_role_relation(&[&arc, &non_tangent_arc], 0x100).is_none());

    let interior_tangent_arc = cadmpeg_ir::sketches::SketchEntity::new(
        SketchEntityId::mint("generated:test:arc#arc-interior-tangent").unwrap(),
        tangent_arc.sketch.clone(),
        SketchGeometry::try_from(SketchGeometryDefinition::Arc {
            center: Point2::new(-2.0 - 2.0 / 2.0_f64.sqrt(), 2.0 + 2.0 / 2.0_f64.sqrt()),
            radius: Length::new(1.0).unwrap(),
            start_angle: Angle::new(-std::f64::consts::FRAC_PI_2).unwrap(),
            end_angle: Angle::new(0.0).unwrap(),
        })
        .unwrap(),
    );
    assert!(matches!(
        counted_role_relation(&[&arc, &interior_tangent_arc], 0x100),
        Some(SketchConstraintDefinitionInput::Tangent { first, second })
            if &first == arc.id() && &second == interior_tangent_arc.id()
    ));
}
