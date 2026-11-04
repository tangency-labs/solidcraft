// SPDX-License-Identifier: Apache-2.0

use crate::design::decode::scopes::thread::exact_thread_construction;
use crate::records::feature::scope::DesignParameterScope;
use crate::records::feature::thread::{
    DesignThreadConstruction, DesignThreadExtended, DesignThreadForm,
};
use crate::test_support::lp_utf16;
use cadmpeg_core::decode::u64_from_index;

const SCOPE: u32 = 100;
const OFFSET_OWNER: u32 = 120;
const LENGTH_OWNER: u32 = 123;

#[derive(Clone)]
struct Fixture {
    external: u8,
    entries: u32,
    class: &'static str,
    copy: &'static str,
    full_length: u8,
    modeled_flag: u8,
    faces: u32,
    swapped_owners: bool,
}

impl Fixture {
    fn cosmetic() -> Self {
        Self {
            external: 1,
            entries: 0,
            class: "6g",
            copy: "M10x1.5",
            full_length: 1,
            modeled_flag: 0,
            faces: 1,
            swapped_owners: false,
        }
    }

    fn modeled() -> Self {
        Self {
            entries: 1,
            modeled_flag: 1,
            ..Self::cosmetic()
        }
    }

    fn references(&self) -> Vec<u32> {
        let mut references = (0..self.faces)
            .flat_map(|face| [101 + 4 * face, 102 + 4 * face])
            .collect::<Vec<_>>();
        references.extend([OFFSET_OWNER, LENGTH_OWNER]);
        references
    }

    fn bytes(&self) -> Vec<u8> {
        fn marked(bytes: &mut Vec<u8>, record: u32) {
            bytes.push(1);
            bytes.extend_from_slice(&record.to_le_bytes());
            bytes.extend_from_slice(&[0; 6]);
        }
        let mut bytes = vec![0; 30];
        bytes[21..29].copy_from_slice(&60.0f64.to_le_bytes());
        bytes[29] = self.external;
        bytes.extend_from_slice(&self.entries.to_le_bytes());
        for _ in 0..self.entries {
            bytes.extend_from_slice(&1u64.to_le_bytes());
            bytes.extend_from_slice(&0x130u64.to_le_bytes());
        }
        lp_utf16(&mut bytes, self.class);
        lp_utf16(&mut bytes, self.copy);
        lp_utf16(&mut bytes, "M10x1.5");
        lp_utf16(&mut bytes, "10.0");
        lp_utf16(&mut bytes, "ISO Metric profile");
        bytes.extend_from_slice(&[self.full_length, 1, 0, 0, 0]);
        bytes.extend_from_slice(&0.985f64.to_le_bytes());
        bytes.extend_from_slice(&0.814f64.to_le_bytes());
        bytes.push(self.modeled_flag);
        bytes.extend_from_slice(&0.15f64.to_le_bytes());
        bytes.extend_from_slice(&0.893f64.to_le_bytes());
        bytes.push(0);
        let (first, second) = if self.swapped_owners {
            (OFFSET_OWNER, LENGTH_OWNER)
        } else {
            (LENGTH_OWNER, OFFSET_OWNER)
        };
        marked(&mut bytes, first);
        marked(&mut bytes, second);
        bytes.push(1);
        bytes
    }

    fn decode(&self) -> Option<DesignThreadConstruction> {
        let bytes = self.bytes();
        let references = self.references();
        let mut scope = DesignParameterScope::empty(
            "f3d:test:extended-thread#100",
            crate::records::feature::scope::DesignFeatureKind::Thread,
            SCOPE,
        );
        scope
            .try_edit(|draft| {
                draft.frame_length = 600;
                draft.paired_byte_offset = 600;
                draft.reference_members =
                    crate::records::identity::ReferenceRun::unlocated(references);
                draft.layout_fixture_references();
                draft.layout_fixture_tail();
            })
            .unwrap();
        let owners = [(OFFSET_OWNER, 0u32, 0.0), (LENGTH_OWNER, 1, 2.0)]
            .into_iter()
            .map(|(index, ordinal, value)| {
                let owner = crate::design::decode::parameters::parse_parameter_owner(
                    &crate::design::test_support::parameter_owner_frame(),
                )
                .unwrap()
                .into_record("test", 1000 + u64::from(ordinal) * 200)
                .unwrap();
                let mut wire = crate::records::parameters::DesignParameterOwnerWire::from(owner);
                wire.record_index = index;
                wire.scope_record_index = SCOPE;
                wire.local_ordinal = ordinal;
                wire.evaluated_value = value;
                wire.parameter_record_index = index + 1;
                wire.companion_record_index = index + 2;
                crate::records::parameters::DesignParameterOwner::try_from(wire).unwrap()
            })
            .collect::<Vec<_>>();
        exact_thread_construction(&bytes, &scope, &owners)
    }
}

fn extended(construction: &DesignThreadConstruction) -> &DesignThreadExtended {
    construction.extended.as_ref().expect("extended fields")
}

#[test]
fn the_extended_prefix_reads_its_class_designation_copy_and_sense() {
    let construction = Fixture::cosmetic().decode().expect("cosmetic thread");
    assert_eq!(construction.form, DesignThreadForm::StandardExtended);
    assert_eq!(construction.designation.as_str(), "M10x1.5");
    assert_eq!(
        construction.designation_offset,
        u64_from_index(34 + (4 + 4) + (4 + 14))
    );
    assert_eq!(construction.face_group_record_indices, [101]);
    assert_eq!(
        extended(&construction),
        &DesignThreadExtended {
            class: "6g".into(),
            external: true,
            modeled: false,
            full_length: Some(true),
        }
    );

    let mut class = Fixture::cosmetic();
    class.class = "4g6g";
    let construction = class.decode().expect("four-unit class");
    assert_eq!(extended(&construction).class, "4g6g");
    assert_eq!(
        construction.designation_offset,
        u64_from_index(34 + (4 + 8) + (4 + 14))
    );

    let mut empty_copy = Fixture::cosmetic();
    empty_copy.copy = "";
    assert_eq!(
        empty_copy.decode().map(|c| c.designation_offset),
        Some(u64_from_index(34 + (4 + 4) + 4))
    );
    let mut other_copy = Fixture::cosmetic();
    other_copy.copy = "M10x1.25";
    assert!(other_copy.decode().is_none());

    let mut internal = Fixture::cosmetic();
    internal.external = 0;
    internal.class = "6H";
    assert!(!extended(&internal.decode().expect("internal thread")).external);
    let mut invalid = Fixture::cosmetic();
    invalid.external = 2;
    assert!(invalid.decode().is_none());
    let mut no_class = Fixture::cosmetic();
    no_class.class = "";
    assert!(no_class.decode().is_none());
}

#[test]
fn the_entry_count_and_the_tail_flag_state_a_modeled_thread() {
    let construction = Fixture::modeled().decode().expect("modeled thread");
    assert!(extended(&construction).modeled);
    assert_eq!(
        construction.designation_offset,
        u64_from_index(34 + 16 + (4 + 4) + (4 + 14))
    );
    let mut cases = Vec::new();
    let mut unflagged = Fixture::modeled();
    unflagged.modeled_flag = 0;
    cases.push(("entry without flag", unflagged));
    let mut flagged = Fixture::cosmetic();
    flagged.modeled_flag = 1;
    cases.push(("flag without entry", flagged));
    let mut two = Fixture::modeled();
    two.entries = 2;
    cases.push(("two entries", two));
    for (name, fixture) in cases {
        assert!(fixture.decode().is_none(), "{name}");
    }
}

#[test]
fn the_first_marker_byte_is_the_full_length_flag() {
    let mut blind = Fixture::cosmetic();
    blind.full_length = 0;
    assert_eq!(
        extended(&blind.decode().expect("thread of a set length")).full_length,
        Some(false)
    );
    let mut invalid = Fixture::cosmetic();
    invalid.full_length = 2;
    assert!(invalid.decode().is_none());
}

#[test]
fn the_owners_follow_every_face_selection_pair() {
    let mut three = Fixture::cosmetic();
    three.faces = 3;
    let construction = three.decode().expect("three-face thread");
    assert_eq!(construction.face_group_record_indices, [101, 105, 109]);
    let mut swapped = Fixture::cosmetic();
    swapped.swapped_owners = true;
    assert!(swapped.decode().is_none());
    let mut swapped_three = three.clone();
    swapped_three.swapped_owners = true;
    assert!(swapped_three.decode().is_none());
}
