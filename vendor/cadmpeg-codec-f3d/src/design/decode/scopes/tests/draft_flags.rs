// SPDX-License-Identifier: Apache-2.0

use crate::design::decode::scopes::draft::exact_draft_operation_with_owners;
use crate::design::decode::scopes::parameter_scope::parse_parameter_scope_with_type_guid;
use crate::design::DesignFeatureFamily;
use crate::records::feature::direct_face::{DesignDraftFlags, DesignDraftOperation};
use crate::records::references::DesignClassTag;
use crate::test_support::{indexed_header, lp_utf16};

const DRAFT_TYPE_GUID: &str = "F4163A07-9E71-4C46-BD92-ACE5B5B40BED";
const SCOPE: u32 = 90;
const ANGLE: u32 = 95;
const OPPOSITE: u32 = 96;

#[derive(Clone)]
struct Fixture {
    form: u32,
    constant: u32,
    flipped: u8,
    tangent_chain: u8,
    mode: u32,
    angle_reference: u32,
    opposite_reference: u32,
    zero_at_11: u8,
    opposite_value: f64,
}

impl Fixture {
    fn new() -> Self {
        Self {
            form: 3,
            constant: 1,
            flipped: 0,
            tangent_chain: 0,
            mode: 1,
            angle_reference: ANGLE,
            opposite_reference: OPPOSITE,
            zero_at_11: 0,
            opposite_value: 0.0,
        }
    }

    fn bytes(&self) -> Vec<u8> {
        fn marked(bytes: &mut Vec<u8>, record: u32) {
            bytes.push(1);
            bytes.extend_from_slice(&record.to_le_bytes());
            bytes.extend_from_slice(&[0; 6]);
        }
        let mut bytes = Vec::new();
        indexed_header(&mut bytes, *b"331", SCOPE);
        bytes.push(self.zero_at_11);
        bytes.extend_from_slice(&[0; 9]);
        bytes.extend_from_slice(&self.form.to_le_bytes());
        bytes.extend_from_slice(&self.constant.to_le_bytes());
        bytes.push(self.flipped);
        bytes.push(self.tangent_chain);
        bytes.extend_from_slice(&self.mode.to_le_bytes());
        marked(&mut bytes, self.angle_reference);
        marked(&mut bytes, self.opposite_reference);
        bytes.extend_from_slice(&[0; 20]);
        let table = [91_u32, 92, 93, 94, ANGLE, OPPOSITE];
        bytes.extend_from_slice(&u32::try_from(table.len()).unwrap().to_le_bytes());
        for record in table {
            marked(&mut bytes, record);
        }
        bytes.extend_from_slice(&17_u32.to_le_bytes());
        lp_utf16(&mut bytes, "Dépouille");
        let mut tail = [0; 78];
        tail[0..4].copy_from_slice(&2_u32.to_le_bytes());
        tail[31..35].copy_from_slice(&16_u32.to_le_bytes());
        bytes.extend_from_slice(&tail);
        indexed_header(&mut bytes, *b"258", SCOPE);
        for (record, ordinal, value) in [(ANGLE, 0_u8, 0.25), (OPPOSITE, 1, self.opposite_value)] {
            let mut scalar = vec![0; 104];
            scalar[0..4].copy_from_slice(&3_u32.to_le_bytes());
            scalar[4..7].copy_from_slice(b"277");
            scalar[7..11].copy_from_slice(&record.to_le_bytes());
            scalar[24] = 1;
            scalar[25..29].copy_from_slice(&SCOPE.to_le_bytes());
            scalar[35] = ordinal;
            scalar[40..48].copy_from_slice(&f64::to_le_bytes(value));
            bytes.extend_from_slice(&scalar);
            indexed_header(&mut bytes, *b"261", record);
        }
        bytes
    }

    fn decode(&self) -> Option<DesignDraftOperation> {
        let bytes = self.bytes();
        let records = crate::design::test_support::indexed_record_offsets_for_test(&bytes);
        let class_tag = DesignClassTag::try_from("331".to_owned()).unwrap();
        let scope = parse_parameter_scope_with_type_guid(
            &bytes,
            &records,
            SCOPE,
            &class_tag,
            0,
            Some(DRAFT_TYPE_GUID),
        )
        .unwrap()
        .expect("Draft scope");
        assert_eq!(
            crate::design::design_feature_family(&scope.kind()),
            Some(DesignFeatureFamily::Draft)
        );
        assert_eq!(scope.source_kind_name(), "Dépouille");
        exact_draft_operation_with_owners(&bytes, &records, &scope, &[])
    }
}

#[test]
fn the_type_guid_selects_draft_for_a_localized_name() {
    let operation = Fixture::new().decode().unwrap();
    assert_eq!(operation.angle.get(), 0.25);
    assert_eq!(operation.angle_record_index, ANGLE);
    assert_eq!(operation.opposite_angle_record_index, OPPOSITE);
}

#[test]
fn the_prologue_reads_flip_and_tangent_chain_apart() {
    for (flipped, tangent_chain) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
        let mut fixture = Fixture::new();
        fixture.flipped = flipped;
        fixture.tangent_chain = tangent_chain;
        assert_eq!(
            fixture.decode().unwrap().flags,
            Some(DesignDraftFlags {
                flipped: flipped == 1,
                tangent_chain: tangent_chain == 1,
            })
        );
    }
    let mut invalid = Fixture::new();
    invalid.flipped = 2;
    assert!(invalid.decode().is_none());
    let mut invalid = Fixture::new();
    invalid.tangent_chain = 2;
    assert!(invalid.decode().is_none());
}

#[test]
fn only_the_single_angle_mode_has_an_operation() {
    for mode in [0, 2, 3, 4] {
        let mut fixture = Fixture::new();
        fixture.mode = mode;
        assert!(fixture.decode().is_none(), "mode {mode}");
    }
    let mut two_with_angle = Fixture::new();
    two_with_angle.mode = 2;
    two_with_angle.opposite_value = 0.1;
    assert!(two_with_angle.decode().is_none());
    let mut single_with_second_angle = Fixture::new();
    single_with_second_angle.opposite_value = 0.1;
    assert!(single_with_second_angle.decode().is_none());
}

#[test]
fn a_frame_without_this_prologue_states_no_flags() {
    let mut other_form = Fixture::new();
    other_form.form = 4;
    other_form.mode = 2;
    let mut other_constant = Fixture::new();
    other_constant.constant = 2;
    other_constant.mode = 2;
    let mut nonzero = Fixture::new();
    nonzero.zero_at_11 = 1;
    nonzero.mode = 2;
    let mut other_angle = Fixture::new();
    other_angle.angle_reference = 91;
    other_angle.mode = 2;
    let mut other_opposite = Fixture::new();
    other_opposite.opposite_reference = 91;
    other_opposite.mode = 2;
    for fixture in [
        other_form,
        other_constant,
        nonzero,
        other_angle,
        other_opposite,
    ] {
        assert_eq!(fixture.decode().unwrap().flags, None);
    }
}
