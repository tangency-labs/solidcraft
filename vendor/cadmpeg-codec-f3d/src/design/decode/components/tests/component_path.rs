// SPDX-License-Identifier: Apache-2.0
//! Component-path carriers of any length.

use crate::records::component_path::{
    ComponentPathTransform, DesignComponentPath, DesignComponentPathElement,
    DesignComponentPathElementKind as Kind,
};
use crate::test_support::{cross_document_reference, local_reference};

/// A `CommonData` component-path carrier with the given local path elements.
fn path_carrier(entity_id: u64, elements: &[(u64, u32)], transform: [[f64; 4]; 4]) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&3_u32.to_le_bytes());
    bytes.extend_from_slice(b"297");
    bytes.extend_from_slice(&entity_id.to_le_bytes());
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    bytes.push(1);
    bytes.extend_from_slice(&u32::try_from(elements.len()).unwrap().to_le_bytes());
    for (record, discriminator) in elements {
        bytes.extend(local_reference(*record));
        bytes.extend_from_slice(&discriminator.to_le_bytes());
    }
    bytes.push(0);
    bytes.push(0);
    for value in transform.into_iter().flatten() {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    bytes.extend(local_reference(8));
    bytes.push(2);
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    bytes.extend_from_slice(&42_u32.to_le_bytes());
    bytes.extend(local_reference(8));
    bytes.extend(local_reference(3));
    bytes.extend(local_reference(6));
    bytes
}

fn translation(x: f64) -> [[f64; 4]; 4] {
    [
        [1.0, 0.0, 0.0, x],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ]
}

#[test]
fn a_path_of_any_length_keeps_each_element_and_its_discriminator() {
    let magic = Some(crate::metastream::MODERN_SERIALIZER_MAGIC);
    for elements in [
        vec![(159, 61), (200, 0)],
        vec![(159, 61), (300, 0), (400, 1), (500, 2), (600, 0)],
        vec![
            (1, 0),
            (2, 0),
            (3, 0),
            (4, 0),
            (5, 0),
            (6, 0),
            (7, 0),
            (8, 3),
        ],
    ] {
        let bytes = path_carrier(11, &elements, translation(0.5));
        let carrier = crate::xref::component_path_carrier(&bytes, magic).expect("path");
        assert_eq!(carrier.carrier_record_index, 11);
        assert_eq!(carrier.elements, elements);
        assert_eq!(carrier.transform, Some(translation(0.5)));
    }
}

#[test]
fn a_mirrored_matrix_is_a_valid_placement_and_a_skewed_one_is_not() {
    let magic = Some(crate::metastream::MODERN_SERIALIZER_MAGIC);
    let mut mirrored = translation(0.5);
    mirrored[0][0] = -1.0;
    let bytes = path_carrier(11, &[(159, 0), (200, 0)], mirrored);
    let carrier = crate::xref::component_path_carrier(&bytes, magic).expect("mirrored");
    assert_eq!(carrier.transform, Some(mirrored));
    // The strict two-element parser keeps its proper-rigid admission.
    assert!(crate::xref::local_component_path_pose(&bytes, magic).is_none());
    let mut skewed = translation(0.5);
    skewed[0][1] = 0.5;
    let bytes = path_carrier(11, &[(159, 0), (200, 0)], skewed);
    assert!(crate::xref::component_path_carrier(&bytes, magic).is_none());
}

#[test]
fn a_path_with_a_cross_document_element_is_not_a_local_path() {
    let magic = Some(crate::metastream::MODERN_SERIALIZER_MAGIC);
    let mut bytes = path_carrier(11, &[(159, 0), (200, 0), (300, 0)], translation(0.5));
    let first = local_reference(159);
    let at = bytes
        .windows(first.len())
        .position(|window| window == first.as_slice())
        .unwrap();
    bytes.splice(
        at..at + first.len(),
        cross_document_reference(100, "aaaabbbb-cccc-dddd-eeee-ffff00001111"),
    );
    assert!(crate::xref::component_path_carrier(&bytes, magic).is_none());
}

#[test]
fn a_retained_path_is_checked_against_its_record() {
    let magic = Some(crate::metastream::MODERN_SERIALIZER_MAGIC);
    let bytes = path_carrier(11, &[(159, 4), (200, 1)], translation(0.5));
    let carrier = crate::xref::component_path_carrier(&bytes, magic).unwrap();
    let path = DesignComponentPath {
        id: "f3d:Design/BulkStream.dat:design-component-path#11".to_owned(),
        carrier_record_index: carrier.carrier_record_index,
        elements: vec![
            DesignComponentPathElement {
                record_index: 159,
                kind: Kind::Component,
                discriminator: 4,
            },
            DesignComponentPathElement {
                record_index: 200,
                kind: Kind::Occurrence,
                discriminator: 1,
            },
        ],
        transform: Some(ComponentPathTransform::try_from(translation(0.5)).unwrap()),
        frame_bytes: bytes.into(),
    };
    assert!(crate::xref::component_path_matches(&path));
    let mut changed = path.clone();
    changed.elements[1].discriminator = 2;
    assert!(!crate::xref::component_path_matches(&changed));
    let mut moved = path;
    moved.transform = Some(ComponentPathTransform::try_from(translation(0.7)).unwrap());
    assert!(!crate::xref::component_path_matches(&moved));
}
