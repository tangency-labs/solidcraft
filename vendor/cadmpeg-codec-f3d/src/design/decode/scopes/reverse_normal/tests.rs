// SPDX-License-Identifier: Apache-2.0

use crate::design::decode::scopes::parameter_scope::parse_parameter_scope_with_type_guid;
use crate::records::feature::body_ops::DesignReverseNormalOperation;
use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
use crate::records::identity::Located;
use crate::records::references::DesignClassTag;
use crate::test_support::{indexed_header, lp_utf16};

const REVERSE_NORMAL_TYPE_GUID: &str = "693D6578-4BCE-44F8-B15A-2CE5162A23EE";
const NULL_GUID: &str = "00000000-0000-0000-0000-000000000000";
const SCOPE: u32 = 700;
const GROUP: u32 = 704;
const BODIES: [u32; 2] = [707, 711];

#[derive(Clone)]
struct Fixture {
    shifted: bool,
    prologue_byte: u8,
    group_count: u32,
    group_reference: u32,
    guid: &'static str,
    kind: &'static str,
}

impl Fixture {
    fn compact() -> Self {
        Self {
            shifted: false,
            prologue_byte: 0,
            group_count: 1,
            group_reference: GROUP,
            guid: NULL_GUID,
            kind: "Normale umdrehen",
        }
    }

    fn shifted() -> Self {
        Self {
            shifted: true,
            kind: "Reverse Normal",
            ..Self::compact()
        }
    }

    fn bytes(&self) -> Vec<u8> {
        fn marked(bytes: &mut Vec<u8>, record: u32) {
            bytes.push(1);
            bytes.extend_from_slice(&record.to_le_bytes());
            bytes.extend_from_slice(&[0; 6]);
        }
        let mut bytes = Vec::new();
        indexed_header(&mut bytes, *b"422", SCOPE);
        if self.shifted {
            bytes.extend_from_slice(&[0; 9]);
            bytes.push(1);
            bytes.extend_from_slice(&[0; 4]);
        } else {
            bytes.extend_from_slice(&[0; 10]);
        }
        bytes[12] = self.prologue_byte;
        bytes.extend_from_slice(&self.group_count.to_le_bytes());
        marked(&mut bytes, self.group_reference);
        lp_utf16(&mut bytes, self.guid);
        bytes.extend_from_slice(&[0; 3]);
        let table = [GROUP, BODIES[0], BODIES[1]];
        bytes.extend_from_slice(&u32::try_from(table.len()).unwrap().to_le_bytes());
        for record in table {
            marked(&mut bytes, record);
        }
        bytes.extend_from_slice(&248_u32.to_le_bytes());
        lp_utf16(&mut bytes, self.kind);
        let mut tail = [0; 77];
        tail[0..4].copy_from_slice(&7_u32.to_le_bytes());
        tail[31..35].copy_from_slice(&247_u32.to_le_bytes());
        bytes.extend_from_slice(&tail);
        indexed_header(&mut bytes, *b"268", SCOPE);
        bytes
    }

    fn decode(&self, type_guid: Option<&str>) -> DesignParameterScope {
        let bytes = self.bytes();
        parse_parameter_scope_with_type_guid(
            &bytes,
            &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
            SCOPE,
            &DesignClassTag::try_from("422".to_owned()).unwrap(),
            0,
            type_guid,
        )
        .unwrap()
        .expect("Reverse Normal scope")
    }
}

#[test]
fn type_guid_selects_reverse_normal_for_a_localized_compact_scope() {
    let scope = Fixture::compact().decode(Some(REVERSE_NORMAL_TYPE_GUID));
    assert_eq!(scope.kind(), DesignFeatureKind::ReverseNormal);
    assert_eq!(scope.source_kind_name(), "Normale umdrehen");
    assert_eq!(
        scope.reverse_normal_operation(),
        Some(&DesignReverseNormalOperation {
            body_group: Located {
                value: GROUP,
                offset: 26,
            },
        })
    );
    assert_eq!(
        scope
            .reference_members()
            .values()
            .copied()
            .collect::<Vec<_>>(),
        [GROUP, BODIES[0], BODIES[1]]
    );
    let restored: DesignParameterScope =
        serde_json::from_str(&serde_json::to_string(&scope).unwrap()).unwrap();
    assert_eq!(restored, scope);
}

#[test]
fn shifted_prefix_moves_the_body_group_reference_four_bytes() {
    let scope = Fixture::shifted().decode(None);
    assert_eq!(scope.kind(), DesignFeatureKind::ReverseNormal);
    assert_eq!(
        scope.reverse_normal_operation(),
        Some(&DesignReverseNormalOperation {
            body_group: Located {
                value: GROUP,
                offset: 30,
            },
        })
    );
}

#[test]
fn prefix_outside_either_form_leaves_the_operation_unread() {
    for fixture in [
        Fixture {
            group_count: 2,
            ..Fixture::compact()
        },
        Fixture {
            group_reference: BODIES[0],
            ..Fixture::compact()
        },
        Fixture {
            guid: "ABCDEF12-3456-7890-ABCD-EF1234567890",
            ..Fixture::compact()
        },
        Fixture {
            prologue_byte: 1,
            ..Fixture::compact()
        },
        Fixture {
            prologue_byte: 1,
            ..Fixture::shifted()
        },
    ] {
        let scope = fixture.decode(Some(REVERSE_NORMAL_TYPE_GUID));
        assert_eq!(scope.kind(), DesignFeatureKind::ReverseNormal);
        assert_eq!(scope.reverse_normal_operation(), None);
        assert!(scope.payload().is_uninterpreted());
    }
}
