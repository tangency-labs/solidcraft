// SPDX-License-Identifier: Apache-2.0

use crate::design::feature_project::project_coil;
use crate::records::{
    feature::{
        coil::{
            DesignCoilExtent, DesignCoilSection, DesignCoilSectionPlacement, DesignCoilTransform,
        },
        extrude::DesignExtrudeOperation,
        scope::DesignParameterScope,
    },
    parameters::DesignParameter,
};
use cadmpeg_ir::features::{CoilPlacement, CoilResult, FeatureDefinition, FeatureOperation};

fn parameter(
    record_index: u32,
    source_kind: &str,
    unit: Option<&str>,
    value: f64,
) -> DesignParameter {
    crate::records::parameters::DesignParameter::try_from(
        crate::records::parameters::DesignParameterDraft {
            id: format!("f3d:Design/BulkStream.dat:parameter#{record_index}"),
            byte_offset: 0,
            class_tag: crate::records::references::DesignClassTag::try_from("000".to_owned())
                .unwrap(),
            record_index,
            source_ordinal: 0,
            source: crate::records::parameters::DesignParameterSource::new(
                source_kind.into(),
                Some(0),
                None,
            )
            .unwrap(),
            expression: value.to_string(),
            expression_offset: 40,
            source_kind_offset: 60,

            unit: unit.map(|value| crate::records::identity::RecordedValue {
                value: value.to_owned(),
                offset: 70,
            }),
            name: source_kind.into(),
            name_offset: 80,
            evaluated_value: value,
            evaluated_value_offset: 90,
        },
    )
    .unwrap()
}

fn long_coil_fixture() -> (DesignParameterScope, [DesignParameter; 5]) {
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#40",
        crate::records::feature::scope::DesignFeatureKind::CoilPrimitive,
        40,
    );
    if let crate::records::feature::scope::DesignScopePayloadMut::SpirePrimitive(slot)
    | crate::records::feature::scope::DesignScopePayloadMut::CoilPrimitive(slot) =
        scope.payload_mut()
    {
        slot.get_or_insert_with(Default::default).operation =
            Some(crate::records::identity::RecordedValue {
                value: DesignExtrudeOperation::NewBody,
                offset: 62,
            });
    }
    if let crate::records::feature::scope::DesignScopePayloadMut::SpirePrimitive(slot)
    | crate::records::feature::scope::DesignScopePayloadMut::CoilPrimitive(slot) =
        scope.payload_mut()
    {
        slot.get_or_insert_with(Default::default).extent =
            Some(crate::records::identity::MaybeRecordedValue::Unlocated(
                DesignCoilExtent::RevolutionsHeight,
            ));
    }
    if let crate::records::feature::scope::DesignScopePayloadMut::SpirePrimitive(slot)
    | crate::records::feature::scope::DesignScopePayloadMut::CoilPrimitive(slot) =
        scope.payload_mut()
    {
        slot.get_or_insert_with(Default::default).section = Some(
            crate::records::identity::MaybeRecordedValue::Unlocated(DesignCoilSection::Circular),
        );
    }
    {
        let value = Some(DesignCoilSectionPlacement::Inside);
        if let crate::records::feature::scope::DesignScopePayloadMut::SpirePrimitive(slot)
        | crate::records::feature::scope::DesignScopePayloadMut::CoilPrimitive(slot) =
            scope.payload_mut()
        {
            slot.get_or_insert_with(Default::default).section_placement =
                value.map(crate::records::identity::MaybeRecordedValue::Unlocated);
        }
    }
    {
        let value = Some(false);
        if let crate::records::feature::scope::DesignScopePayloadMut::SpirePrimitive(slot)
        | crate::records::feature::scope::DesignScopePayloadMut::CoilPrimitive(slot) =
            scope.payload_mut()
        {
            slot.get_or_insert_with(Default::default).clockwise =
                value.map(crate::records::identity::MaybeRecordedValue::Unlocated);
        }
    }
    if let crate::records::feature::scope::DesignScopePayloadMut::SpirePrimitive(slot)
    | crate::records::feature::scope::DesignScopePayloadMut::CoilPrimitive(slot) =
        scope.payload_mut()
    {
        slot.get_or_insert_with(Default::default).transform = Some(DesignCoilTransform {
            transform: [
                [1.0, 0.0, 0.0, 1.25],
                [0.0, 1.0, 0.0, -2.5],
                [0.0, 0.0, 1.0, 3.75],
                [0.0, 0.0, 0.0, 1.0],
            ]
            .try_into()
            .unwrap(),
            transform_offset: 77,
        });
    }
    let parameters = [
        parameter(1, "Diameter", Some("cm"), 2.0),
        parameter(2, "SectionSize", Some("cm"), 0.2),
        parameter(3, "TaperAngle", Some("rad"), 0.0),
        parameter(4, "Revolutions", None, 3.0),
        parameter(5, "Height", Some("cm"), 1.5),
    ];
    (scope, parameters)
}

fn owned_parameters(parameters: &[DesignParameter; 5]) -> Vec<(u32, &DesignParameter)> {
    parameters
        .iter()
        .enumerate()
        .map(|(ordinal, parameter)| {
            (
                u32::try_from(ordinal).expect("fixture value fits u32"),
                parameter,
            )
        })
        .collect()
}

#[test]
fn long_coil_matrix_projects_as_explicit_placement() {
    let (scope, parameters) = long_coil_fixture();
    let owned = owned_parameters(&parameters);

    let FeatureDefinition::Operation(FeatureOperation::Coil { construction, .. }) =
        project_coil(&scope, &owned, &[])
            .unwrap()
            .expect("typed long Coil")
    else {
        panic!("expected Coil definition")
    };
    assert_eq!(
        construction.placement,
        CoilPlacement::Explicit {
            frame: cadmpeg_ir::features::FeatureUnitPlaneFrame::new(
                cadmpeg_ir::math::Point3::new(12.5, -25.0, 37.5),
                cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
                cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0)
            )
            .unwrap()
        }
    );
}

#[test]
fn coil_without_transform_keeps_native_placement() {
    let (mut scope, parameters) = long_coil_fixture();
    if let crate::records::feature::scope::DesignScopePayloadMut::SpirePrimitive(slot)
    | crate::records::feature::scope::DesignScopePayloadMut::CoilPrimitive(slot) =
        scope.payload_mut()
    {
        slot.as_mut().unwrap().transform = None;
    }
    let owned = owned_parameters(&parameters);
    let definition = project_coil(&scope, &owned, &[]).unwrap().unwrap();
    assert!(matches!(
        definition,
        FeatureDefinition::Operation(FeatureOperation::Coil { construction, .. })
            if matches!(&construction.placement, CoilPlacement::Native { native_ref }
                if native_ref.as_str() == scope.id)
    ));
}

fn coil_body_group() -> crate::records::topology::construction::DesignConstructionOperandGroup {
    use crate::records::topology::construction::{
        DesignConstructionOperandGroup, DesignConstructionOperandGroupDraft,
        DesignConstructionOperandGroupFrame, DesignConstructionOperandGroupFrameDraft,
        DesignConstructionOperandRole,
    };
    DesignConstructionOperandGroup::try_from(DesignConstructionOperandGroupDraft {
        id: "f3d:Design/BulkStream.dat:group#60".into(),
        scope_record_index: 40,
        scope_reference_ordinal: 0,
        record_index: 60,
        byte_offset: 0,
        class_tag: "282".to_owned().try_into().unwrap(),
        members: vec![crate::records::identity::Located {
            value: 61,
            offset: 0,
        }],
        lost_edge_references: Vec::new(),
        frame: DesignConstructionOperandGroupFrame::try_from(
            DesignConstructionOperandGroupFrameDraft {
                member_count_offset: 0,
                auxiliary_records: Vec::new(),
                auxiliary_paths: Vec::new(),
                trailing_records: Vec::new(),
                trailing_transforms: Vec::new(),
                trailing_dual_transforms: Vec::new(),
                trailing_flags: Vec::new(),
                opaque_index: 1,
                opaque_index_offset: 18,
                opaque_scalar: 0.0,
                opaque_scalar_offset: 22,
                variant: false,
                closing_flag: false,
            },
        )
        .unwrap(),
        operand_role: DesignConstructionOperandRole::Other(
            crate::records::topology::extrude_selection::DesignOperandRole::BODIES_B,
        ),
        role_offset: 0,
        paired_class_tag: "261".to_owned().try_into().unwrap(),
        paired_byte_offset: 0,
    })
    .unwrap()
}

#[test]
fn coil_join_projects_boolean_result() {
    let (mut scope, parameters) = long_coil_fixture();
    if let crate::records::feature::scope::DesignScopePayloadMut::SpirePrimitive(slot)
    | crate::records::feature::scope::DesignScopePayloadMut::CoilPrimitive(slot) =
        scope.payload_mut()
    {
        slot.as_mut().unwrap().operation = Some(crate::records::identity::RecordedValue {
            value: DesignExtrudeOperation::Join,
            offset: 62,
        });
    }
    let group = coil_body_group();
    let owned = owned_parameters(&parameters);
    let definition = project_coil(&scope, &owned, std::slice::from_ref(&group))
        .unwrap()
        .unwrap();
    assert!(matches!(
        definition,
        FeatureDefinition::Operation(FeatureOperation::Coil {
            result: CoilResult::Boolean { .. },
            ..
        })
    ));
}
