// SPDX-License-Identifier: Apache-2.0
//! Extrude selection groups: member runs, scalars and derived offsets.

fn selection_group_wire() -> super::DesignExtrudeSelectionGroupWire {
    let wire = r#"{"id":"group","scope_record_index":7,"scope_reference_ordinal":0,"record_index":9,"byte_offset":0,"class_tag":"277","member_count_offset":32,"members":[10,11],"member_offsets":[37,48],"opaque_index":1,"opaque_index_offset":58,"opaque_scalar":0.0,"opaque_scalar_offset":62,"variant":false,"paired_class_tag":"259","paired_byte_offset":111}"#;
    serde_json::from_str(wire).unwrap()
}

fn selection_group() -> super::DesignExtrudeSelectionGroup {
    selection_group_wire().try_into().unwrap()
}

fn selection_member() -> super::DesignExtrudeSelectionMember {
    let wire = serde_json::json!({
        "id": "member", "group_record_index": 9, "group_member_ordinal": 0,
        "record_index": 10, "byte_offset": 0, "class_tag": "277",
        "local_id": 12, "local_id_offset": 21,
        "asset_id": "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d", "asset_id_offset": 33,
        "context_id": "1b2c3d4e-5f6a-4b7c-8d9e-0f1a2b3c4d5e", "context_id_offset": 110,
        "next_record_index": 11, "next_byte_offset": 190
    });
    serde_json::from_value(wire).unwrap()
}

#[test]
fn extrude_selection_group_borrowed_wire_matches_owned_wire_bytes() {
    let group = selection_group();
    let owned = super::DesignExtrudeSelectionGroupWire::from(group.clone());
    assert_eq!(
        serde_json::to_vec(&group).unwrap(),
        serde_json::to_vec(&owned).unwrap()
    );
}

#[test]
fn extrude_selection_group_native_retained_limit_refuses_before_clone() {
    #[derive(serde::Serialize)]
    struct NestedRecord<'a> {
        id: &'static str,
        value: &'a super::DesignExtrudeSelectionGroup,
    }
    let group = selection_group();
    let record = NestedRecord {
        id: "f3d:native:extrude-group#0",
        value: &group,
    };
    crate::test_support::native_test::assert_borrowed_native_retained_limit(
        &record,
        "design_parameter_scopes",
        || super::EXTRUDE_SELECTION_GROUP_CLONE_COUNT.with(|count| count.set(0)),
        || super::EXTRUDE_SELECTION_GROUP_CLONE_COUNT.with(std::cell::Cell::get),
    );
}

#[test]
fn extrude_selection_member_borrowed_wire_matches_owned_wire_bytes() {
    let mut member = selection_member();
    for identities in [Vec::new(), vec!["identity#1".into(), "identity#2".into()]] {
        member.operand_identity_ids = identities;
        let owned = super::DesignExtrudeSelectionMemberDraft::from(member.clone());
        assert_eq!(
            serde_json::to_vec(&member).unwrap(),
            serde_json::to_vec(&owned).unwrap()
        );
    }
}

#[test]
fn extrude_selection_member_native_retained_limit_refuses_before_clone() {
    #[derive(serde::Serialize)]
    struct NestedRecord<'a> {
        id: &'static str,
        value: &'a super::DesignExtrudeSelectionMember,
    }
    let member = selection_member();
    let record = NestedRecord {
        id: "f3d:native:extrude-member#0",
        value: &member,
    };
    crate::test_support::native_test::assert_borrowed_native_retained_limit(
        &record,
        "design_parameter_scopes",
        || super::EXTRUDE_SELECTION_MEMBER_CLONE_COUNT.with(|count| count.set(0)),
        || super::EXTRUDE_SELECTION_MEMBER_CLONE_COUNT.with(std::cell::Cell::get),
    );
}

#[test]
fn extrude_selection_group_members_preserve_wire_and_reject_unequal_offsets() {
    let wire = r#"{"id":"group","scope_record_index":7,"scope_reference_ordinal":0,"record_index":9,"byte_offset":0,"class_tag":"277","member_count_offset":32,"members":[10,11],"member_offsets":[37,48],"opaque_index":1,"opaque_index_offset":58,"opaque_scalar":0.0,"opaque_scalar_offset":62,"variant":false,"paired_class_tag":"259","paired_byte_offset":111}"#;
    let group: super::DesignExtrudeSelectionGroup =
        serde_json::from_str(wire).expect("selection group");
    assert_eq!(serde_json::to_string(&group).expect("selection wire"), wire);
    for offsets in ["[]", "[37]", "[37,48,59]"] {
        let invalid = wire.replace(
            "\"member_offsets\":[37,48]",
            &format!("\"member_offsets\":{offsets}"),
        );
        let error = serde_json::from_str::<super::DesignExtrudeSelectionGroup>(&invalid)
            .expect_err("unequal member arrays")
            .to_string();
        assert!(error.contains("members"));
        assert!(error.contains("member_offsets"));
    }
}

#[test]
fn extrude_group_rejects_invalid_run_and_scalar_admission() {
    let wire = serde_json::json!({"id": "group", "scope_record_index": 7, "scope_reference_ordinal": 0,
        "record_index": 9, "byte_offset": 0, "class_tag": "277", "member_count_offset": 32,
        "members": [10, 11], "member_offsets": [37, 48], "opaque_index": 1,
        "opaque_index_offset": 58, "opaque_scalar": -1.0, "opaque_scalar_offset": 62,
        "variant": false, "paired_class_tag": "259", "paired_byte_offset": 111});
    let group: super::DesignExtrudeSelectionGroup = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(serde_json::to_value(&group).unwrap(), wire);
    for (field, value) in [
        ("members", serde_json::json!([10, 10])),
        ("member_offsets", serde_json::json!([37, 49])),
        ("opaque_index", serde_json::json!(0)),
        ("member_count_offset", serde_json::json!(33)),
        ("opaque_index_offset", serde_json::json!(59)),
        ("opaque_scalar_offset", serde_json::json!(63)),
        ("paired_byte_offset", serde_json::json!(112)),
        ("byte_offset", serde_json::json!(u64::MAX)),
    ] {
        let mut invalid = wire.clone();
        invalid[field] = value;
        assert!(
            serde_json::from_value::<super::DesignExtrudeSelectionGroup>(invalid).is_err(),
            "{field}"
        );
    }
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let mut invalid = super::DesignExtrudeSelectionGroupWire::from(group.clone());
        invalid.opaque_scalar = value;
        assert!(super::DesignExtrudeSelectionGroup::try_from(invalid).is_err());
    }
    for members in [vec![], vec![10, 10]] {
        let mut changed = group.clone();
        assert!(changed.try_set_members(members).is_err());
        assert_eq!(changed, group);
    }
}

/// The typed-reference group widens the scope reference, every member, and
/// the three references after the opaque lane by forty bytes each, and it
/// stores no variant byte.
#[test]
fn typed_selection_group_offsets_use_the_wide_reference_stride() {
    let mut wire = selection_group_wire();
    wire.typed_references = true;
    wire.variant = None;
    wire.member_count_offset = 72;
    wire.member_offsets = vec![77, 128];
    wire.opaque_index_offset = 178;
    wire.opaque_scalar_offset = 182;
    wire.paired_byte_offset = 350;
    let group = super::DesignExtrudeSelectionGroup::try_from(wire.clone()).expect("typed group");
    assert!(group.typed_references);
    assert_eq!(group.variant, None);
    let round_trip = serde_json::to_value(&group).unwrap();
    assert_eq!(round_trip["typed_references"], true);
    assert!(round_trip.get("variant").is_none());

    // The plain stride or a variant byte does not fit the typed form.
    let mut plain_stride = wire.clone();
    plain_stride.member_offsets = vec![77, 88];
    assert!(super::DesignExtrudeSelectionGroup::try_from(plain_stride).is_err());
    let mut with_variant = wire;
    with_variant.variant = Some(false);
    assert!(super::DesignExtrudeSelectionGroup::try_from(with_variant).is_err());
}
