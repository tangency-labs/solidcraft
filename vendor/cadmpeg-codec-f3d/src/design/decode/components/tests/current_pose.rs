// SPDX-License-Identifier: Apache-2.0
//! Component-path poses of local occurrences.

use crate::test_support::{cross_document_reference, local_reference};

/// A `CommonData` component-path carrier whose path is two local elements: a
/// component record, then a local occurrence carrier of that component.
fn local_component_path_carrier(
    entity_id: u64,
    component_record: u64,
    occurrence_record: u64,
    transform: [[f64; 4]; 4],
) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&3_u32.to_le_bytes());
    bytes.extend_from_slice(b"257");
    bytes.extend_from_slice(&entity_id.to_le_bytes());
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    bytes.push(1);
    bytes.extend_from_slice(&2_u32.to_le_bytes());
    bytes.extend(local_reference(component_record));
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    bytes.extend(local_reference(occurrence_record));
    bytes.extend_from_slice(&2_u32.to_le_bytes());
    bytes.push(0);
    bytes.push(0);
    for value in transform.into_iter().flatten() {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    bytes.extend(local_reference(occurrence_record));
    bytes.push(2);
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    bytes.extend_from_slice(&42_u32.to_le_bytes());
    bytes.extend(local_reference(8));
    bytes.extend(local_reference(3));
    bytes.extend(local_reference(6));
    bytes
}

#[test]
fn a_local_component_path_carrier_supplies_a_local_occurrence_pose() {
    use crate::records::feature::assembly_features::{
        DesignComponentOccurrence, DesignComponentOccurrenceDraft,
        DesignComponentOccurrencePlacement,
    };
    let magic = Some(crate::metastream::MODERN_SERIALIZER_MAGIC);
    let translation = |x: f64| {
        [
            [1.0, 0.0, 0.0, x],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ]
    };
    let carrier = local_component_path_carrier(11, 159, 10, translation(0.5));
    let pose = crate::xref::local_component_path_pose(&carrier, magic).expect("local pose");
    assert_eq!(
        (
            pose.carrier_record_index,
            pose.component_record_index,
            pose.occurrence_record_index
        ),
        (11, 159, 10)
    );
    assert_eq!(pose.transform, Some(translation(0.5)));
    // Neither parser admits the other's path.
    assert!(crate::xref::component_path_pose(&carrier, magic).is_none());
    let mut external = carrier.clone();
    let document = cross_document_reference(100, "aaaabbbb-cccc-dddd-eeee-ffff00001111");
    let first = local_reference(159);
    let at = external
        .windows(first.len())
        .position(|window| window == first.as_slice())
        .unwrap();
    external.splice(at..at + first.len(), document);
    assert!(crate::xref::local_component_path_pose(&external, magic).is_none());

    let occurrence = |record_index: u32| {
        DesignComponentOccurrence::try_new(DesignComponentOccurrenceDraft {
            id: format!("f3d:Design/BulkStream.dat:design-component-occurrence#{record_index}"),
            class_tag: crate::records::references::DesignClassTag::try_from("290".to_owned())
                .unwrap(),
            record_index,
            byte_offset: u64::from(record_index),
            component_record_index: 159,
            component_guid: "2ad641b3-9ea3-4e5f-a4da-926c3faeccae"
                .to_owned()
                .try_into()
                .unwrap(),
            occurrence_guid: crate::records::mesh::DesignRelaxedGuidText::try_from(
                "9d257e32-3a1f-45da-bbec-cfc658b575dc".to_owned(),
            )
            .unwrap()
            .into(),
            placement: DesignComponentOccurrencePlacement::Base,
            owner_entity: 1,
        })
        .unwrap()
    };
    // The carrier that names the occurrence's component and record binds; one
    // naming another record does not, and two leave the occurrence unposed.
    let mut occurrences = [occurrence(10), occurrence(20)];
    let other = crate::xref::local_component_path_pose(
        &local_component_path_carrier(12, 159, 30, translation(1.0)),
        magic,
    )
    .unwrap();
    let ambiguous = super::super::bind_current_poses(&mut occurrences, &[pose.clone(), other]);
    assert_eq!(ambiguous, 0);
    let current = occurrences[0].current_pose.as_ref().expect("current pose");
    assert_eq!(current.carrier_record_index, 11);
    assert!(occurrences[1].current_pose.is_none());
    assert!(crate::xref::local_current_pose_matches(&occurrences[0]));

    let mut changed = occurrences[0].clone();
    let mut tampered = changed.current_pose.clone().unwrap();
    tampered.transform = Some(translation(0.7).try_into().unwrap());
    changed.current_pose = Some(tampered);
    assert!(!crate::xref::local_current_pose_matches(&changed));
    let mut moved = occurrences[0].clone();
    moved.record_index = 20;
    assert!(!crate::xref::local_current_pose_matches(&moved));

    let mut twice = [occurrence(10)];
    let second = crate::xref::local_component_path_pose(
        &local_component_path_carrier(13, 159, 10, translation(2.0)),
        magic,
    )
    .unwrap();
    assert_eq!(
        super::super::bind_current_poses(&mut twice, &[pose, second]),
        1
    );
    assert!(twice[0].current_pose.is_none());
}
