// SPDX-License-Identifier: Apache-2.0
//! Sketch-profile operands and profile region members.

use serde_json::json;

fn sketch_profile_operand() -> super::DesignSketchProfileOperand {
    serde_json::from_value(json!({
        "scope_reference_ordinal": 0, "record_index": 7, "byte_offset": 0,
        "class_tag": "300", "paired_class_tag": "301", "paired_byte_offset": 160,
        "asset_id": "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d", "asset_id_offset": 32,
        "entity_id": "Sketch_1", "entity_suffix": 1, "entity_reference_offset": 80
    }))
    .unwrap()
}

#[test]
fn sketch_profile_operand_borrowed_wire_matches_owned_wire_bytes() {
    let mut operand = sketch_profile_operand();
    for selection in [
        None,
        Some(super::DesignSketchProfileRegionSelection {
            record_index: 8,
            byte_offset: 90,
            class_tag: "302".to_owned().try_into().unwrap(),
            region_count_offset: 100,
            regions: Vec::new(),
            companion_class_tag: "303".to_owned().try_into().unwrap(),
            companion_byte_offset: 140,
        }),
    ] {
        operand.region_selection = selection;
        let owned = super::DesignSketchProfileOperandWire::from(operand.clone());
        assert_eq!(
            serde_json::to_vec(&operand).unwrap(),
            serde_json::to_vec(&owned).unwrap()
        );
    }
}

#[test]
fn sketch_profile_operand_native_retained_limit_refuses_before_clone() {
    #[derive(serde::Serialize)]
    struct NestedRecord<'a> {
        id: &'static str,
        value: &'a super::DesignSketchProfileOperand,
    }
    let operand = sketch_profile_operand();
    let record = NestedRecord {
        id: "f3d:native:sketch-profile#0",
        value: &operand,
    };
    crate::test_support::native_test::assert_borrowed_native_retained_limit(
        &record,
        "design_parameter_scopes",
        || super::SKETCH_PROFILE_OPERAND_CLONE_COUNT.with(|count| count.set(0)),
        || super::SKETCH_PROFILE_OPERAND_CLONE_COUNT.with(std::cell::Cell::get),
    );
}

#[test]
fn profile_region_member_preserves_fixed_words_and_nonzero_incidence_values() {
    let wire = |kind: u32, identity: u64, words: [u32; 8]| {
        format!("{{\"kind\":{kind},\"kind_offset\":40,\"curve_primary_id\":{identity},\"curve_primary_id_offset\":44,\"incidence_words\":{},\"incidence_words_offset\":48}}", serde_json::to_string(&words).expect("incidence words"))
    };
    for identity in [1, u64::from(u32::MAX)] {
        for flag in [0, 1] {
            for first in [1, 2, 3, 11, u32::MAX] {
                for second in [1, 2, 3, 12, u32::MAX] {
                    let json = wire(3, identity, [0, 0, 0, flag, first, second, 0, 0]);
                    let member: super::DesignSketchProfileRegionMember =
                        serde_json::from_str(&json).expect("region member");
                    assert_eq!(
                        serde_json::to_string(&member).expect("region member wire"),
                        json
                    );
                }
            }
        }
    }
    for kind in [0, 1, 2, 4, u32::MAX] {
        let error = serde_json::from_str::<super::DesignSketchProfileRegionMember>(&wire(
            kind,
            1,
            [0, 0, 0, 0, 1, 1, 0, 0],
        ))
        .expect_err("fixed kind");
        assert!(error.to_string().contains("kind"));
    }
    for identity in [0, u64::from(u32::MAX) + 1, u64::MAX] {
        let error = serde_json::from_str::<super::DesignSketchProfileRegionMember>(&wire(
            3,
            identity,
            [0, 0, 0, 0, 1, 1, 0, 0],
        ))
        .expect_err("nonzero u32 identity");
        assert!(error.to_string().contains("curve_primary_id"));
    }
    for index in 0..8 {
        let mut words = [0, 0, 0, 0, 1, 1, 0, 0];
        words[index] = if matches!(index, 4 | 5) { 0 } else { 3 };
        let error =
            serde_json::from_str::<super::DesignSketchProfileRegionMember>(&wire(3, 1, words))
                .expect_err("invalid incidence word");
        assert!(error.to_string().contains("incidence_words"));
    }
}

#[test]
fn sketch_profile_offsets_must_follow_their_predecessors() {
    use super::DesignSketchProfileOperand;
    let wire = json!({
        "scope_reference_ordinal": 0, "record_index": 7, "byte_offset": 0,
        "class_tag": "300", "paired_class_tag": "301", "paired_byte_offset": 160,
        "asset_id": "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d", "asset_id_offset": 32,
        "entity_id": "Sketch_1", "entity_suffix": 1, "entity_reference_offset": 80
    });
    let admitted: DesignSketchProfileOperand = serde_json::from_value(wire.clone()).unwrap();
    let mut draft = admitted.into_draft();
    draft.byte_offset = draft.asset_id_offset;
    assert!(DesignSketchProfileOperand::try_new(draft).is_err());
    for (field, preceding) in [
        ("asset_id_offset", 0),
        ("entity_reference_offset", 32),
        ("paired_byte_offset", 80),
    ] {
        let mut invalid = wire.clone();
        invalid[field] = preceding.into();
        assert!(serde_json::from_value::<DesignSketchProfileOperand>(invalid).is_err());
    }
}
