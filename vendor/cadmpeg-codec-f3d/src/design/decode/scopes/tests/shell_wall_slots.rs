// SPDX-License-Identifier: Apache-2.0

use crate::design::decode::scopes::direct_face::exact_direct_face_operation;
use crate::design::decode::scopes::parameter_scope::parse_parameter_scope_with_type_guid;
use crate::records::feature::direct_face::{DesignDirectFaceOperation, DesignShellOperation};
use crate::records::references::DesignClassTag;
use crate::test_support::{indexed_header, lp_utf16};

const SHELL_TYPE_GUID: &str = "E8063467-BC05-4688-8196-AECB90F0EFEE";
const GROUP: u32 = 91;
const MEMBER: u32 = 92;
const INSIDE: u32 = 93;
const OUTSIDE: u32 = 94;

#[derive(Clone)]
struct Fixture {
    class_tag: [u8; 3],
    tangent_chain: u8,
    rounded_corners: u8,
    inside: Option<u32>,
    outside: Option<u32>,
    table: Vec<u32>,
    group_reference: u32,
    gap_before_table: usize,
    scalars: Vec<(u32, f64)>,
    corrupt: Option<(usize, u8)>,
}

impl Fixture {
    fn new(inside: Option<u32>, outside: Option<u32>) -> Self {
        let mut table = vec![GROUP, MEMBER];
        table.extend(inside);
        table.extend(outside);
        Self {
            class_tag: *b"412",
            tangent_chain: 0,
            rounded_corners: 0,
            inside,
            outside,
            table,
            group_reference: GROUP,
            gap_before_table: 0,
            scalars: vec![(INSIDE, 0.25), (OUTSIDE, 0.5)],
            corrupt: None,
        }
    }

    fn bytes(&self) -> Vec<u8> {
        fn marked(bytes: &mut Vec<u8>, record: u32) {
            bytes.push(1);
            bytes.extend_from_slice(&record.to_le_bytes());
            bytes.extend_from_slice(&[0; 6]);
        }
        let mut bytes = Vec::new();
        indexed_header(&mut bytes, self.class_tag, 90);
        bytes.extend_from_slice(&[0; 10]);
        bytes.push(self.tangent_chain);
        bytes.push(self.rounded_corners);
        for slot in [self.inside, self.outside] {
            match slot {
                Some(record) => marked(&mut bytes, record),
                None => bytes.push(0),
            }
        }
        bytes.extend_from_slice(&[0; 12]);
        bytes.extend_from_slice(&1_u32.to_le_bytes());
        marked(&mut bytes, self.group_reference);
        lp_utf16(&mut bytes, "00000000-0000-0000-0000-000000000000");
        bytes.resize(bytes.len() + 3 + self.gap_before_table, 0);
        bytes.extend_from_slice(&u32::try_from(self.table.len()).unwrap().to_le_bytes());
        for record in &self.table {
            marked(&mut bytes, *record);
        }
        bytes.extend_from_slice(&17_u32.to_le_bytes());
        lp_utf16(&mut bytes, "Coque");
        let mut tail = [0; 78];
        tail[0..4].copy_from_slice(&2_u32.to_le_bytes());
        tail[31..35].copy_from_slice(&16_u32.to_le_bytes());
        bytes.extend_from_slice(&tail);
        indexed_header(&mut bytes, *b"259", 90);
        for (record, value) in &self.scalars {
            let mut scalar = vec![0; 105];
            scalar[0..4].copy_from_slice(&3_u32.to_le_bytes());
            scalar[4..7].copy_from_slice(b"321");
            scalar[7..11].copy_from_slice(&record.to_le_bytes());
            scalar[24] = 1;
            scalar[25..29].copy_from_slice(&42_u32.to_le_bytes());
            scalar[40..48].copy_from_slice(&value.to_le_bytes());
            bytes.extend_from_slice(&scalar);
            indexed_header(&mut bytes, *b"265", *record);
        }
        if let Some((offset, value)) = self.corrupt {
            bytes[offset] = value;
        }
        bytes
    }

    fn decode(&self) -> Option<DesignShellOperation> {
        let bytes = self.bytes();
        let records = crate::design::test_support::indexed_record_offsets_for_test(&bytes);
        let class_tag =
            DesignClassTag::try_from(String::from_utf8(self.class_tag.to_vec()).unwrap()).unwrap();
        let scope = parse_parameter_scope_with_type_guid(
            &bytes,
            &records,
            90,
            &class_tag,
            0,
            Some(SHELL_TYPE_GUID),
        )
        .unwrap()
        .expect("Shell scope");
        match exact_direct_face_operation(&bytes, &records, &scope) {
            Some(DesignDirectFaceOperation::Shell(operation)) => Some(operation),
            Some(_) => panic!("expected a Shell operation"),
            None => None,
        }
    }
}

#[test]
fn each_wall_slot_selects_its_direction_and_scalar() {
    let inside = Fixture::new(Some(INSIDE), None).decode().unwrap();
    assert_eq!(inside.thickness.get(), 0.25);
    assert_eq!(inside.thickness_record_index, INSIDE);
    assert!(!inside.outward);
    assert_eq!(inside.outward_offset, 23);
    assert_eq!(inside.wall_slots.unwrap().outside, None);

    let outside = Fixture::new(None, Some(OUTSIDE)).decode().unwrap();
    assert_eq!(outside.thickness.get(), 0.5);
    assert_eq!(outside.thickness_record_index, OUTSIDE);
    assert!(outside.outward);
    assert_eq!(outside.outward_offset, 24);
    assert_eq!(outside.wall_slots.unwrap().outside, None);

    let both = Fixture::new(Some(INSIDE), Some(OUTSIDE)).decode().unwrap();
    assert_eq!(both.thickness.get(), 0.25);
    assert_eq!(both.thickness_record_index, INSIDE);
    assert!(!both.outward);
    let wall = both.wall_slots.unwrap().outside.unwrap();
    assert_eq!(wall.thickness.get(), 0.5);
    assert_eq!(wall.thickness_record_index, OUTSIDE);
}

#[test]
fn the_form_has_no_class_requirement() {
    for class_tag in [*b"262", *b"369", *b"412"] {
        let mut fixture = Fixture::new(Some(INSIDE), None);
        fixture.class_tag = class_tag;
        assert_eq!(fixture.decode().unwrap().thickness_record_index, INSIDE);
    }
}

#[test]
fn the_table_length_follows_the_member_count() {
    let mut fixture = Fixture::new(Some(INSIDE), Some(OUTSIDE));
    fixture.table = vec![GROUP, MEMBER, 95, INSIDE, OUTSIDE];
    assert!(fixture
        .decode()
        .unwrap()
        .wall_slots
        .unwrap()
        .outside
        .is_some());
}

#[test]
fn flags_read_tangent_chain_and_rounded_corners_apart() {
    for (tangent_chain, rounded_corners) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
        let mut fixture = Fixture::new(Some(INSIDE), None);
        fixture.tangent_chain = tangent_chain;
        fixture.rounded_corners = rounded_corners;
        let slots = fixture.decode().unwrap().wall_slots.unwrap();
        assert_eq!(slots.tangent_chain, tangent_chain == 1);
        assert_eq!(slots.rounded_corners, rounded_corners == 1);
    }
    let mut invalid = Fixture::new(Some(INSIDE), None);
    invalid.tangent_chain = 2;
    assert_eq!(invalid.decode(), None);
    let mut invalid = Fixture::new(Some(INSIDE), None);
    invalid.rounded_corners = 2;
    assert_eq!(invalid.decode(), None);
}

#[test]
fn a_frame_outside_the_grammar_is_not_this_form() {
    let mut no_wall = Fixture::new(None, None);
    no_wall.table = vec![GROUP, MEMBER, INSIDE];
    assert_eq!(no_wall.decode(), None);

    let mut invalid_marker = Fixture::new(Some(INSIDE), None);
    invalid_marker.corrupt = Some((23, 2));
    assert_eq!(invalid_marker.decode(), None);

    for offset in [11, 20] {
        let mut nonzero = Fixture::new(Some(INSIDE), None);
        nonzero.corrupt = Some((offset, 1));
        assert_eq!(nonzero.decode(), None, "offset {offset}");
    }

    let mut nonzero_after_slots = Fixture::new(Some(INSIDE), None);
    nonzero_after_slots.corrupt = Some((35, 1));
    assert_eq!(nonzero_after_slots.decode(), None);

    let mut group_count = Fixture::new(Some(INSIDE), None);
    group_count.corrupt = Some((47, 2));
    assert_eq!(group_count.decode(), None);

    let mut other_group = Fixture::new(Some(INSIDE), None);
    other_group.group_reference = MEMBER;
    assert_eq!(other_group.decode(), None);

    let mut gap = Fixture::new(Some(INSIDE), None);
    gap.gap_before_table = 1;
    assert_eq!(gap.decode(), None);
}

#[test]
fn the_table_must_end_with_every_wall_in_slot_order() {
    let mut swapped = Fixture::new(Some(INSIDE), Some(OUTSIDE));
    swapped.table = vec![GROUP, MEMBER, OUTSIDE, INSIDE];
    assert_eq!(swapped.decode(), None);

    let mut one_listed = Fixture::new(Some(INSIDE), Some(OUTSIDE));
    one_listed.table = vec![GROUP, MEMBER, 95, INSIDE];
    assert_eq!(one_listed.decode(), None);

    let mut no_selection = Fixture::new(Some(INSIDE), None);
    no_selection.table = vec![GROUP, INSIDE];
    assert_eq!(no_selection.decode(), None);
}

#[test]
fn every_present_wall_needs_a_positive_scalar() {
    let mut missing_inside = Fixture::new(Some(INSIDE), Some(OUTSIDE));
    missing_inside.scalars = vec![(OUTSIDE, 0.5)];
    assert_eq!(missing_inside.decode(), None);

    let mut missing_outside = Fixture::new(Some(INSIDE), Some(OUTSIDE));
    missing_outside.scalars = vec![(INSIDE, 0.25)];
    assert_eq!(missing_outside.decode(), None);

    let mut zero_outside = Fixture::new(Some(INSIDE), Some(OUTSIDE));
    zero_outside.scalars = vec![(INSIDE, 0.25), (OUTSIDE, 0.0)];
    assert_eq!(zero_outside.decode(), None);

    let mut negative_inside = Fixture::new(Some(INSIDE), None);
    negative_inside.scalars = vec![(INSIDE, -0.25)];
    assert_eq!(negative_inside.decode(), None);
}
