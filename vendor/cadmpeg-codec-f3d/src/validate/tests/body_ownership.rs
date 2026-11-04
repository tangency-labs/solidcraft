// SPDX-License-Identifier: Apache-2.0

use crate::records::body_ownership::{COMPONENT_GUID, CONTAINER_BASE_GUID, CONTAINER_GUID};
use crate::records::entity_header::{BaseTypeGuid, SegmentType, SegmentTypeData};

fn registration(entity: u64, guid: &str, module: &str, version: u32, offset: u64) -> SegmentType {
    SegmentType::try_new(
        format!("f3d:Design/MetaStream.dat:design-type#{offset}"),
        SegmentTypeData {
            byte_offset: offset,
            type_guid: guid.to_owned().try_into().unwrap(),
            type_guid_offset: offset + 4,
            base_type_guid: BaseTypeGuid::Guid {
                value: CONTAINER_BASE_GUID.to_owned().try_into().unwrap(),
                offset: offset + 44,
            },
            version,
            version_offset: offset + 80,
            module: module.into(),
            entities: crate::records::identity::ReferenceRun::unlocated(vec![entity]),
        },
    )
    .unwrap()
}

fn layout(bodies: &[u64], form: u8) -> Vec<u8> {
    if form != 0 {
        let mut bytes = u32::try_from(bodies.len()).unwrap().to_le_bytes().to_vec();
        for body in bodies {
            bytes.push(1);
            bytes.extend_from_slice(&body.to_le_bytes());
            bytes.extend_from_slice(&[0, 0]);
        }
        if form == 1 {
            bytes.extend_from_slice(&u32::try_from(bodies.len()).unwrap().to_le_bytes());
            for body in bodies.iter().rev() {
                bytes.push(1);
                bytes.extend_from_slice(&body.to_le_bytes());
                bytes.extend_from_slice(&[0, 0]);
                bytes.extend_from_slice(&1_u32.to_le_bytes());
            }
        }
        bytes.push(1);
        bytes.extend_from_slice(&40_u64.to_le_bytes());
        bytes.extend_from_slice(&[0, 0]);
        return bytes;
    }
    let mut bytes = u32::try_from(bodies.len()).unwrap().to_le_bytes().to_vec();
    for (target, guid) in bodies
        .iter()
        .map(|body| {
            (
                *body,
                crate::design::presentation::BODY_PRESENTATION_TYPE_GUID,
            )
        })
        .chain([(40, COMPONENT_GUID)])
    {
        bytes.push(1);
        bytes.extend_from_slice(&target.to_le_bytes());
        bytes.extend_from_slice(&36_u32.to_le_bytes());
        bytes.extend_from_slice(guid.as_bytes());
        bytes.extend_from_slice(&[0, 0]);
    }
    bytes
}

#[test]
fn validator_and_decoder_admit_the_same_component_membership_tail() {
    let decode = cadmpeg_test_support::service_decode_context();
    let mut native = crate::native::F3dNative {
        design_types: vec![
            registration(60, CONTAINER_GUID, "Body", 4, 0),
            registration(40, COMPONENT_GUID, "Component", 12, 100),
            registration(
                900,
                crate::design::presentation::BODY_PRESENTATION_TYPE_GUID,
                "Body",
                15,
                200,
            ),
            registration(
                901,
                crate::design::presentation::BODY_PRESENTATION_TYPE_GUID,
                "Body",
                15,
                300,
            ),
        ],
        ..Default::default()
    };
    for (form, bodies) in (0..=2).flat_map(|form| {
        [&[][..], &[900][..], &[900, 901][..]]
            .into_iter()
            .map(move |bodies| (form, bodies))
    }) {
        let mut binding: crate::records::bodies::DesignBodyBinding = serde_json::from_value(serde_json::json!({
                "id": "f3d:Design/BulkStream.dat:design-body-binding#1000", "stream": "Design/BulkStream.dat",
                "pair_count": 1, "pair_ordinal": 0, "asm_body_key": 0, "asm_body_key_offset": 1000,
                "entity_suffix": 900, "entity_suffix_offset": 1008, "blob_name": "BREP.synthetic.smb", "blob_name_offset": 1050,
                "component_owner": crate::records::body_ownership::membership(&layout(if bodies.is_empty() { &[900] } else { bodies }, form)).unwrap().proof(60, 500, 0).unwrap()
            })).unwrap();
        if bodies.is_empty() {
            binding.component_owner.as_mut().unwrap().body_count = 0;
        }
        let ownership_types =
            super::super::body_ownership::OwnerTypes::new(&native, &decode).unwrap();
        assert_eq!(ownership_types.matches(&binding), !bodies.is_empty());
        let owner = binding.component_owner.as_ref().unwrap();
        let lookup = |entity| {
            native
                .design_types
                .iter()
                .find(|data| data.entities.values().any(|value| *value == entity))
                .map(|data| &**data)
        };
        assert_eq!(
            crate::design::decode::component_bodies::valid_owner(owner, 900, lookup),
            !bodies.is_empty()
        );
        let mut corrupted = binding.clone();
        match &mut corrupted.component_owner.as_mut().unwrap().references {
            crate::records::body_ownership::BodyOwnershipReferences::InlineTyped {
                owner, ..
            } => owner[0] = 0,
            crate::records::body_ownership::BodyOwnershipReferences::LocalSingle {
                owner, ..
            }
            | crate::records::body_ownership::BodyOwnershipReferences::Local { owner, .. } => {
                owner[0] = 0
            }
        }
        assert!(!ownership_types.matches(&corrupted));
        if let crate::records::body_ownership::BodyOwnershipReferences::Local {
            body_word, ..
        } = &mut binding.component_owner.as_mut().unwrap().references
        {
            *body_word = 2;
            assert!(!ownership_types.matches(&binding));
        }
    }
    let proof = crate::records::body_ownership::membership(&layout(&[900], 0))
        .unwrap()
        .proof(60, 500, 0)
        .unwrap();
    native.design_types[0].set_version(5);
    let lookup = |entity| {
        native
            .design_types
            .iter()
            .find(|data| data.entities.values().any(|value| *value == entity))
            .map(|data| &**data)
    };
    assert!(!crate::design::decode::component_bodies::valid_owner(
        &proof, 900, lookup
    ));
}
