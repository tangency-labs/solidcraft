// SPDX-License-Identifier: Apache-2.0

#[test]
fn typed_profile_validator_checks_region_offsets_and_exterior_cardinality() {
    use crate::records::topology::entity_selection::{
        DesignProfileCurveEntry, DesignProfileCurveLoop, DesignProfileCurveSelection,
    };
    let mut selection = DesignProfileCurveSelection {
        id: "synthetic:selection".into(),
        scope_record_index: 1,
        group_record_index: 2,
        group_member_ordinal: 0,
        record_index: 3,
        byte_offset: 10,
        owner_text: "42".into(),
        owner_text_offset: 30,
        sketch_scope_record_index: 7,
        sketch_scope_offset: 80,
        identity_record_offset: 100,
        identity_end_offset: 274,
        loop_lists: [
            vec![vec![
                DesignProfileCurveLoop {
                    outer: false,
                    entries: vec![DesignProfileCurveEntry {
                        primary_id: 11,
                        primary_id_offset: 188,
                        secondary_id: 0,
                        entry_fields: [1, 1, 1],
                    }],
                },
                DesignProfileCurveLoop {
                    outer: true,
                    entries: vec![DesignProfileCurveEntry {
                        primary_id: 12,
                        primary_id_offset: 233,
                        secondary_id: 0,
                        entry_fields: [1, 1, 1],
                    }],
                },
            ]],
            vec![],
        ],
    };
    let validate = |selection: &DesignProfileCurveSelection| {
        let mut findings = Vec::new();
        super::super::validate_profile_curve_selection(selection, &mut findings);
        findings
    };
    assert!(validate(&selection).is_empty());
    selection.loop_lists[0][0][0].outer = true;
    assert_eq!(validate(&selection).len(), 1);
    selection.loop_lists[0][0][0].outer = false;
    selection.loop_lists[0][0][1].entries[0].primary_id_offset += 1;
    assert_eq!(validate(&selection).len(), 1);
    selection.loop_lists[0][0][1].entries[0].primary_id_offset -= 1;
    selection.identity_end_offset += 1;
    assert_eq!(validate(&selection).len(), 1);
}
