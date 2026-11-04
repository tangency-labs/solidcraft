// SPDX-License-Identifier: Apache-2.0

use super::super::{
    find_dimension_locus_groups, find_dimension_locus_pair, find_dimension_null_locus_pair,
    parse_dimension_locus_group, parse_dimension_locus_pair, parse_dimension_null_locus_pair,
};
use super::TEST_LINEAR_TOLERANCE;
use crate::design::decode::parameters::parse_design_parameter_record;
use crate::design::dimensions::{
    null_locus_dimension_definition, remove_dimension_frame_relations,
};
use crate::design::test_support::parameter_record;
use crate::records::{parameters::DesignParameterOwner, sketch_relations::SketchRelation};

use cadmpeg_ir::math::Point2;
use cadmpeg_ir::sketches::{
    SketchAxis, SketchConstraintDefinitionInput, SketchEntity, SketchEntityId, SketchGeometry,
    SketchGeometryDefinition, SketchId,
};
use std::collections::HashSet;

#[test]
fn dimension_locus_pair_resolves_two_typed_geometry_records() {
    let mut bytes = vec![0; 80];
    bytes[0..4].copy_from_slice(&3u32.to_le_bytes());
    bytes[4..7].copy_from_slice(b"277");
    bytes[7..11].copy_from_slice(&233u32.to_le_bytes());
    bytes[19] = 1;
    bytes[20..24].copy_from_slice(&3u32.to_le_bytes());
    bytes[24] = 1;
    bytes[35..39].copy_from_slice(&4u32.to_le_bytes());
    bytes[39] = 1;
    bytes[40..44].copy_from_slice(&192u32.to_le_bytes());
    bytes[50..54].copy_from_slice(&0u32.to_le_bytes());
    bytes[54] = 1;
    bytes[55..59].copy_from_slice(&194u32.to_le_bytes());
    bytes[65..69].copy_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"273");
    bytes.extend_from_slice(&233u32.to_le_bytes());

    let mut pair = parse_dimension_locus_pair(&bytes, 0, 228, &HashSet::from([192, 194]))
        .expect("paired dimension locus frame");
    pair.id = "f3d:Design/BulkStream.dat:design-dimension-locus-pair#0".into();
    assert_eq!(pair.companion_record_index, 228);
    assert_eq!(pair.record_index, 233);
    assert_eq!(pair.frame_length(), 80);
    assert_eq!(pair.loci()[0].geometry_index(), 192);
    assert_eq!(pair.loci()[0].role, 0);
    assert_eq!(pair.loci()[1].geometry_index(), 194);
    assert_eq!(pair.loci()[1].role, 1);
    assert_eq!(pair.paired_class_tag.as_str(), "273");
    let mut parameter = parse_design_parameter_record(&parameter_record(
        Some(300),
        "40 mm",
        "Linear Dimension-3",
        Some("mm"),
        "d3",
        4.0,
    ))
    .unwrap();
    parameter.id = "f3d:Design/BulkStream.dat:design-parameter#301".into();
    parameter.record_index = 301;
    let owner =
        DesignParameterOwner::try_from(crate::records::parameters::DesignParameterOwnerWire {
            id: "f3d:Design/BulkStream.dat:design-parameter-owner#300".into(),
            byte_offset: pair.paired_byte_offset() + 59,
            frame_length: 104,
            class_tag: crate::records::references::DesignClassTag::try_from("292".to_owned())
                .unwrap(),
            record_index: 300,
            scope_record_index: 10,
            local_ordinal: 0,
            evaluated_value: 4.0,
            evaluated_value_offset: pair.paired_byte_offset() + 99,
            parameter_record_index: 301,
            owned_ordinal: 3,
            variant: Some(0),
            companion_record_index: 302,
        })
        .unwrap();
    assert_eq!(
        crate::design::decode::dimension_frames::following_dimension_companion_record_index(
            &pair.id,
            pair.paired_byte_offset(),
            std::slice::from_ref(&owner),
            std::slice::from_ref(&parameter),
        ),
        Some(302)
    );
    assert_eq!(
        crate::design::decode::dimension_frames::following_dimension_companion_record_index(
            &pair.id,
            pair.paired_byte_offset(),
            &[owner.clone(), owner],
            std::slice::from_ref(&parameter),
        ),
        None
    );

    let mut nested = Vec::new();
    nested.extend_from_slice(&3u32.to_le_bytes());
    nested.extend_from_slice(b"341");
    nested.extend_from_slice(&229u32.to_le_bytes());
    nested.extend_from_slice(&bytes);
    let nested_end = nested.len();
    let nested = find_dimension_locus_pair(&nested, 0, nested_end, 228, &HashSet::from([192, 194]))
        .expect("nested paired dimension locus frame");
    assert_eq!(nested.byte_offset(), 11);
    assert_eq!(nested.paired_byte_offset(), 91);

    let mut competing = bytes.clone();
    competing.extend_from_slice(&bytes);
    assert!(find_dimension_locus_pair(
        &competing,
        0,
        competing.len(),
        228,
        &HashSet::from([192, 194]),
    )
    .is_none());
}

#[test]
fn dimension_null_locus_pair_preserves_null_and_typed_roles() {
    let mut bytes = vec![0; 74];
    bytes[0..4].copy_from_slice(&3u32.to_le_bytes());
    bytes[4..7].copy_from_slice(b"277");
    bytes[7..11].copy_from_slice(&1394u32.to_le_bytes());
    bytes[19] = 1;
    bytes[20..24].copy_from_slice(&2u32.to_le_bytes());
    bytes[24] = 1;
    bytes[35..39].copy_from_slice(&10u32.to_le_bytes());
    bytes[39] = 1;
    bytes[40..44].copy_from_slice(&1109u32.to_le_bytes());
    bytes[50..54].copy_from_slice(&7u32.to_le_bytes());
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"273");
    bytes.extend_from_slice(&1394u32.to_le_bytes());

    let pair = parse_dimension_null_locus_pair(&bytes, 0, 1290, &HashSet::from([1109]))
        .expect("null-locus dimension frame");
    assert_eq!(pair.companion_record_index, 1290);
    assert_eq!(pair.governing_companion_record_index, 1290);
    assert_eq!(pair.record_index, 1394);
    assert_eq!(pair.frame_length(), 74);
    assert_eq!(pair.loci()[0].role, 10);
    assert_eq!(pair.loci()[1].geometry_index(), 1109);
    assert_eq!(pair.loci()[1].role, 7);
    assert_eq!(pair.paired_class_tag.as_str(), "273");

    assert!(parse_dimension_null_locus_pair(&bytes, 0, 1290, &HashSet::from([1110]),).is_none());

    let mut nested = Vec::new();
    nested.extend_from_slice(&3u32.to_le_bytes());
    nested.extend_from_slice(b"341");
    nested.extend_from_slice(&229u32.to_le_bytes());
    nested.extend_from_slice(&bytes);
    let nested_end = nested.len();
    let nested =
        find_dimension_null_locus_pair(&nested, 0, nested_end, 1290, &HashSet::from([1109]))
            .expect("null-locus frame following another indexed frame");
    assert_eq!(nested.byte_offset(), 11);
    assert_eq!(nested.paired_byte_offset(), 85);

    let mut axis_pair = pair.clone().into_draft();
    axis_pair.loci[0].role = 14;
    axis_pair.loci[1].role = 3;
    let mut axis_pair =
        crate::records::dimensions::DesignDimensionLocusPair::try_new(axis_pair).unwrap();
    let entity = SketchEntity::new(
        SketchEntityId::mint("f3d:model:sketch-entity#line").unwrap(),
        SketchId::mint("f3d:model:sketch#axis-angle").unwrap(),
        SketchGeometry::try_from(SketchGeometryDefinition::Line {
            start: Point2::new(0.0, 0.0),
            end: Point2::new(1.0, 1.0),
        })
        .unwrap(),
    );
    let parameter = cadmpeg_ir::features::ParameterId::mint("f3d:model:parameter#angle")
        .expect("identity grammar");
    assert!(matches!(
        null_locus_dimension_definition(&axis_pair, &entity, "Angular Dimension-2", std::f64::consts::FRAC_PI_4, parameter.clone(), TEST_LINEAR_TOLERANCE),
        Some(SketchConstraintDefinitionInput::AngleToAxis {
            entity: ref actual_entity,
            axis: SketchAxis::Horizontal,
            parameter: ref actual_parameter,
        }) if actual_entity == entity.id() && actual_parameter == &parameter
    ));
    assert!(null_locus_dimension_definition(
        &axis_pair,
        &entity,
        "Angular Dimension-2",
        0.5,
        parameter.clone(),
        TEST_LINEAR_TOLERANCE
    )
    .is_none());
    let mut draft = axis_pair.into_draft();
    draft.loci[0].role = 13;
    axis_pair = crate::records::dimensions::DesignDimensionLocusPair::try_new(draft).unwrap();
    assert!(null_locus_dimension_definition(
        &axis_pair,
        &entity,
        "Angular Dimension-2",
        std::f64::consts::FRAC_PI_4,
        parameter.clone(),
        TEST_LINEAR_TOLERANCE
    )
    .is_none());

    let radial_entity = SketchEntity::new(
        SketchEntityId::mint("synthetic:test:id#f3d:model:sketch-entity:circle").unwrap(),
        SketchId::mint("f3d:model:sketch#radial").unwrap(),
        SketchGeometry::try_from(SketchGeometryDefinition::Circle {
            center: Point2::new(0.0, 0.0),
            radius: cadmpeg_ir::scalar::Length::new(1.000_000_014_901_161_2).unwrap(),
        })
        .unwrap(),
    );
    assert!(matches!(
        null_locus_dimension_definition(&pair, &radial_entity, "Diameter Dimension-2", 0.2, parameter.clone(), TEST_LINEAR_TOLERANCE),
        Some(SketchConstraintDefinitionInput::Diameter {
            entity: ref actual_entity,
            parameter: ref actual_parameter,
        }) if actual_entity == radial_entity.id() && actual_parameter == &parameter
    ));
    assert!(null_locus_dimension_definition(
        &pair,
        &radial_entity,
        "Diameter Dimension-2",
        0.2,
        parameter,
        0.0
    )
    .is_none());
}

#[test]
fn dimension_locus_group_preserves_roles_owner_state_and_return_order() {
    let mut bytes = vec![0; 101];
    bytes[0..4].copy_from_slice(&3u32.to_le_bytes());
    bytes[4..7].copy_from_slice(b"286");
    bytes[7..11].copy_from_slice(&249u32.to_le_bytes());
    bytes[19] = 1;
    bytes[20..24].copy_from_slice(&2u32.to_le_bytes());
    bytes[24] = 1;
    bytes[25..29].copy_from_slice(&175u32.to_le_bytes());
    bytes[35..39].copy_from_slice(&2u32.to_le_bytes());
    bytes[39] = 1;
    bytes[40..44].copy_from_slice(&217u32.to_le_bytes());
    bytes[50..54].copy_from_slice(&1u32.to_le_bytes());
    bytes[55] = 1;
    bytes[56..60].copy_from_slice(&172u32.to_le_bytes());
    bytes[66..70].copy_from_slice(&1u32.to_le_bytes());
    bytes[74..78].copy_from_slice(&2u32.to_le_bytes());
    bytes[78] = 1;
    bytes[79..83].copy_from_slice(&217u32.to_le_bytes());
    bytes[89] = 1;
    bytes[90..94].copy_from_slice(&175u32.to_le_bytes());
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"314");
    bytes.extend_from_slice(&250u32.to_le_bytes());

    let group = parse_dimension_locus_group(
        &bytes,
        0,
        240,
        &HashSet::from([175, 217]),
        &HashSet::from([172]),
    )
    .expect("counted dimension locus frame");
    assert_eq!(group.companion_record_index, 240);
    assert_eq!(group.record_index, 249);
    assert_eq!(group.frame_length, 101);
    assert_eq!(group.owner_reference, 172);
    assert_eq!(group.owner_role, 1);
    assert_eq!(group.state, 0);
    assert_eq!(group.loci[0].geometry_record_index, 175);
    assert_eq!(group.loci[0].role, 2);
    assert_eq!(group.loci[1].geometry_record_index, 217);
    assert_eq!(group.loci[1].role, 1);
    assert_eq!(
        group
            .loci
            .iter()
            .map(|locus| locus.returned.value)
            .collect::<Vec<_>>(),
        [217, 175]
    );
    assert_eq!(group.next_class_tag.as_str(), "314");
    assert_eq!(group.next_record_index, 250);

    let relation_at = |stream: &str, byte_offset| {
        SketchRelation::try_new(crate::records::sketch_relations::SketchRelationDraft {
            id: format!("f3d:{stream}:sketch-relation#{byte_offset}"),
            record_index: 249,
            class_tag: crate::records::references::DesignClassTag::try_from("286".to_owned())
                .unwrap(),
            byte_offset,
            state_offset: 66,
            owner_reference: 172,
            owner_entity_id: Some(cadmpeg_core::text::NonBlankString::new("0_172").unwrap()),
            auxiliary_references: crate::records::identity::ReferenceRun::located(Vec::new()),
            rectangular_counted_reference_count: None,
            members: ([(175, 25), (217, 40)]
                .into_iter()
                .map(|(record_index, offset)| {
                    crate::records::sketch_relations::SketchRelationMember {
                        reference: crate::records::sketch_relations::SketchRelationReference::Index(
                            record_index,
                        ),
                        offset,
                        relation_ordinal: Some(0),
                    }
                })
                .collect::<Vec<_>>())
            .try_into()
            .expect("uniform member resolution"),
            owner_reference_offset: 56,
            definition: crate::records::sketch_relations::SketchRelationDefinition::new(0, None)
                .expect("valid relation definition"),
            entity_genesis: None,
            return_members: ([(217, 79), (175, 90)]
                .into_iter()
                .map(|(record_index, offset)| {
                    crate::records::sketch_relations::SketchRelationReturnMember {
                        reference: crate::records::sketch_relations::SketchRelationReference::Index(
                            record_index,
                        ),
                        offset,
                    }
                })
                .collect::<Vec<_>>())
            .try_into()
            .expect("uniform member resolution"),
            raw_bytes: bytes[..101].to_vec(),
        })
        .unwrap()
    };
    let mut relations = vec![relation_at("native", 0), relation_at("other", 0)];
    let mut group = group;
    group.id = "f3d:native:design-dimension-locus-group#0".into();
    remove_dimension_frame_relations(&mut relations, &[], &[group], &[]);
    assert_eq!(relations.len(), 1);
    assert!(relations[0].id.starts_with("f3d:other:"));

    let body = bytes[11..101].to_vec();
    bytes.extend_from_slice(&body);
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"315");
    bytes.extend_from_slice(&251u32.to_le_bytes());
    let groups = find_dimension_locus_groups(
        &bytes,
        0,
        bytes.len(),
        240,
        &HashSet::from([175, 217]),
        &HashSet::from([172]),
    );
    assert_eq!(
        groups
            .iter()
            .map(|group| group.record_index)
            .collect::<Vec<_>>(),
        [249, 250]
    );
}
