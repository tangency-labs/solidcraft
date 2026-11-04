// SPDX-License-Identifier: Apache-2.0

use crate::records::act::{
    ActChannelGroup, ActComponentLayout, ActComponentLink, ActEntity, ActGuid, ActRegistryChannel,
    ActRegistryFlag, ActTableReference,
};
use crate::records::identity::Located;
use crate::test_support::native_test::reject_changed_id;
use std::collections::BTreeMap;

const GUID: &str = "01234567-89ab-cdef-0123-456789abcdef";

#[test]
fn act_guid_borrowed_wire_matches_owned_wire_bytes() {
    let guid = ActGuid::new("f3d:native:act-guid#20".into(), 20, 3, GUID.into()).unwrap();
    let owned = super::ActGuidWire::from(guid.clone());
    assert_eq!(
        serde_json::to_vec(&guid).unwrap(),
        serde_json::to_vec(&owned).unwrap()
    );
}

#[test]
fn act_guid_native_retained_limit_refuses_before_guid_clone() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let guid = ActGuid::new("f3d:native:act-guid#20".into(), 20, 3, GUID.into()).unwrap();
    let needed = serde_json::to_vec(&guid).unwrap().len();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = u64::try_from(needed).unwrap() - 1;
    let (limited, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    super::ACT_GUID_CLONE_COUNT.with(|count| count.set(0));
    let error = namespace
        .set_arena(&limited, "act_guids", std::slice::from_ref(&guid))
        .unwrap_err();
    super::ACT_GUID_CLONE_COUNT.with(|count| assert_eq!(count.get(), 0));
    assert!(matches!(
        cadmpeg_core::CodecError::from(error),
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "serialize native record"
    ));

    let (service, _) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    namespace
        .set_arena(&service, "act_guids", std::slice::from_ref(&guid))
        .unwrap();
    assert_eq!(
        serde_json::to_value(&namespace.arenas()["act_guids"][0]).unwrap(),
        serde_json::to_value(&guid).unwrap()
    );
}

#[test]
fn act_table_reference_borrowed_wire_matches_owned_wire_bytes() {
    let reference =
        ActTableReference::new("f3d:native:act-table-reference#20".into(), 3, 20, 7).unwrap();
    let owned = super::ActTableReferenceWire::from(reference.clone());
    assert_eq!(
        serde_json::to_vec(&reference).unwrap(),
        serde_json::to_vec(&owned).unwrap()
    );
}

#[test]
fn act_table_reference_native_retained_limit_refuses_before_id_clone() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let reference =
        ActTableReference::new("f3d:native:act-table-reference#20".into(), 3, 20, 7).unwrap();
    let needed = serde_json::to_vec(&reference).unwrap().len();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = u64::try_from(needed).unwrap() - 1;
    let (limited, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    super::ACT_TABLE_REFERENCE_CLONE_COUNT.with(|count| count.set(0));
    let error = namespace
        .set_arena(
            &limited,
            "act_table_references",
            std::slice::from_ref(&reference),
        )
        .unwrap_err();
    super::ACT_TABLE_REFERENCE_CLONE_COUNT.with(|count| assert_eq!(count.get(), 0));
    assert!(matches!(
        cadmpeg_core::CodecError::from(error),
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "serialize native record"
    ));

    let (service, _) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    namespace
        .set_arena(
            &service,
            "act_table_references",
            std::slice::from_ref(&reference),
        )
        .unwrap();
    assert_eq!(
        serde_json::to_value(&namespace.arenas()["act_table_references"][0]).unwrap(),
        serde_json::to_value(&reference).unwrap()
    );
}

#[test]
fn act_registry_channel_borrowed_wire_matches_owned_wire_bytes() {
    let channel = ActRegistryChannel::new(
        "f3d:native:act-registry-channel#20".into(),
        3,
        20,
        "Appearance".into(),
        GUID.into(),
    )
    .unwrap();
    let owned = super::ActRegistryChannelWire::from(channel.clone());
    assert_eq!(
        serde_json::to_vec(&channel).unwrap(),
        serde_json::to_vec(&owned).unwrap()
    );
}

#[test]
fn act_registry_channel_native_retained_limit_refuses_before_name_clone() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let channel = ActRegistryChannel::new(
        "f3d:native:act-registry-channel#20".into(),
        3,
        20,
        "Appearance".into(),
        GUID.into(),
    )
    .unwrap();
    let needed = serde_json::to_vec(&channel).unwrap().len();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = u64::try_from(needed).unwrap() - 1;
    let (limited, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    super::ACT_REGISTRY_CHANNEL_CLONE_COUNT.with(|count| count.set(0));
    let error = namespace
        .set_arena(
            &limited,
            "act_registry_channels",
            std::slice::from_ref(&channel),
        )
        .unwrap_err();
    super::ACT_REGISTRY_CHANNEL_CLONE_COUNT.with(|count| assert_eq!(count.get(), 0));
    assert!(matches!(
        cadmpeg_core::CodecError::from(error),
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "serialize native record"
    ));

    let (service, _) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    namespace
        .set_arena(
            &service,
            "act_registry_channels",
            std::slice::from_ref(&channel),
        )
        .unwrap();
    assert_eq!(
        serde_json::to_value(&namespace.arenas()["act_registry_channels"][0]).unwrap(),
        serde_json::to_value(&channel).unwrap()
    );
}

fn root_component_fixture() -> ActComponentLink {
    ActComponentLink::try_new(
        "f3d:native:act-component-link#20".into(),
        1,
        "261".to_owned().try_into().unwrap(),
        super::ActComponentReferences {
            instance_root: 2,
            tracked_entity: 3,
            components_root: 4,
        },
        ActRegistryFlag::Off,
        ActComponentLayout::new(20, "0_3".into(), "Root".into(), 1).unwrap(),
    )
    .unwrap()
}

#[test]
fn act_root_component_borrowed_wire_matches_owned_wire_bytes() {
    let root = root_component_fixture();
    let owned = super::ActComponentLinkWire::from(root.clone());
    assert_eq!(
        serde_json::to_vec(&root).unwrap(),
        serde_json::to_vec(&owned).unwrap()
    );
}

#[test]
fn act_root_component_native_retained_limit_refuses_before_layout_clone() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let root = root_component_fixture();
    let needed = serde_json::to_vec(&root).unwrap().len();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = u64::try_from(needed).unwrap() - 1;
    let (limited, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    super::ACT_COMPONENT_LINK_CLONE_COUNT.with(|count| count.set(0));
    let error = namespace
        .set_arena(&limited, "act_component_links", std::slice::from_ref(&root))
        .unwrap_err();
    super::ACT_COMPONENT_LINK_CLONE_COUNT.with(|count| assert_eq!(count.get(), 0));
    assert!(matches!(
        cadmpeg_core::CodecError::from(error),
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "serialize native record"
    ));

    let (service, _) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    namespace
        .set_arena(&service, "act_component_links", std::slice::from_ref(&root))
        .unwrap();
    assert_eq!(
        serde_json::to_value(&namespace.arenas()["act_component_links"][0]).unwrap(),
        serde_json::to_value(&root).unwrap()
    );
}

fn group() -> ActChannelGroup {
    ActChannelGroup::try_new(
        100,
        Some(200),
        "261".to_owned().try_into().unwrap(),
        BTreeMap::from([(
            "Appearance".into(),
            Located {
                value: GUID.to_owned().try_into().unwrap(),
                offset: 120,
            },
        )]),
        None,
    )
    .unwrap()
}

fn entity_fixture() -> ActEntity {
    ActEntity::try_new(
        "f3d:native:act-entity#7".into(),
        7,
        "0_1".into(),
        None,
        group(),
    )
    .unwrap()
}

#[test]
fn act_entity_borrowed_wire_matches_owned_wire_bytes() {
    let entity = entity_fixture();
    let owned = super::ActEntitySerde::from(entity.clone());
    assert_eq!(
        serde_json::to_vec(&entity).unwrap(),
        serde_json::to_vec(&owned).unwrap()
    );
}

#[test]
fn act_entity_native_retained_limit_refuses_before_channel_clone() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let entity = entity_fixture();
    let needed = serde_json::to_vec(&entity).unwrap().len();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = u64::try_from(needed).unwrap() - 1;
    let (limited, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    super::ACT_ENTITY_CLONE_COUNT.with(|count| count.set(0));
    let error = namespace
        .set_arena(&limited, "act_entities", std::slice::from_ref(&entity))
        .unwrap_err();
    super::ACT_ENTITY_CLONE_COUNT.with(|count| assert_eq!(count.get(), 0));
    assert!(matches!(
        cadmpeg_core::CodecError::from(error),
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "serialize native record"
    ));

    let (service, _) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    namespace
        .set_arena(&service, "act_entities", std::slice::from_ref(&entity))
        .unwrap();
    assert_eq!(
        serde_json::to_value(&namespace.arenas()["act_entities"][0]).unwrap(),
        serde_json::to_value(&entity).unwrap()
    );
}

#[test]
fn act_channels_reject_unpaired_keys_and_preserve_split_wire_maps() {
    let wire = serde_json::json!({
        "id": "stream:act-entity#7", "record_index": 7, "entity_id": "0_1",
        "in_table": false, "channel_class_tag": "261", "channel_record_index_offset": 100,
        "channel_entity_id_offset": 200,
        "channels": {"Appearance": "11111111-2222-3333-4444-555555555555"},
        "channel_guid_offsets": {"Appearance": 120}
    });
    let entity: crate::records::act::ActEntity = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(serde_json::to_value(entity).unwrap(), wire);
    for offsets in [serde_json::json!({}), serde_json::json!({"Material": 120})] {
        let mut invalid = wire.clone();
        invalid["channel_guid_offsets"] = offsets;
        let error = serde_json::from_value::<crate::records::act::ActEntity>(invalid).unwrap_err();
        assert!(error
            .to_string()
            .contains("channels and channel_guid_offsets"));
    }
    for guid in ["", "11111111-2222-3333-4444-55555555555z"] {
        let mut invalid = wire.clone();
        invalid["channels"]["Appearance"] = serde_json::json!(guid);
        let error = serde_json::from_value::<crate::records::act::ActEntity>(invalid).unwrap_err();
        assert!(error.to_string().contains("GUID"));
    }
    let mut invalid = wire;
    invalid["channels"] = serde_json::json!({});
    assert!(serde_json::from_value::<crate::records::act::ActEntity>(invalid).is_err());
}

#[test]
fn act_stream_residue_requires_nonpadding_bytes_and_a_bounded_offset() {
    let wire = serde_json::json!({
        "id": "stream:act-entity#7", "record_index": 7, "entity_id": "0_1",
        "in_table": false, "channel_class_tag": "261", "channel_record_index_offset": 100,
        "channel_entity_id_offset": 200,
        "channels": {"Appearance":"11111111-2222-3333-4444-555555555555"}, "channel_guid_offsets": {"Appearance":120},
        "channel_class_tail": "0001", "channel_class_tail_offset": 300
    });
    let entity: crate::records::act::ActEntity = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(serde_json::to_value(entity).unwrap(), wire);
    for (bytes, offset) in [
        (serde_json::json!(""), serde_json::json!(300)),
        (serde_json::json!("0000"), serde_json::json!(300)),
        (serde_json::json!("01"), serde_json::Value::Null),
        (serde_json::json!("01"), serde_json::json!(u64::MAX)),
    ] {
        let mut invalid = wire.clone();
        invalid["channel_class_tail"] = bytes;
        invalid["channel_class_tail_offset"] = offset;
        let error = serde_json::from_value::<crate::records::act::ActEntity>(invalid).unwrap_err();
        assert!(error.to_string().contains("channel_class_tail"));
    }
}

#[test]
fn act_table_row_derives_the_entity_offset_and_rejects_wire_drift() {
    let wire = serde_json::json!({
        "id": "stream:act-entity#7", "record_index": 7, "entity_id": "0_1",
        "in_table": true, "table_record_index_offset": 20, "table_entity_id_offset": 34,
        "channel_class_tag": "261", "channel_record_index_offset": 100,
        "channels": {"Appearance":"11111111-2222-3333-4444-555555555555"}, "channel_guid_offsets": {"Appearance":120}
    });
    let entity: crate::records::act::ActEntity = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(entity.table_entity_id_offset(), Some(34));
    assert_eq!(serde_json::to_value(entity).unwrap(), wire);
    for offset in [20, 33, 35, u64::MAX] {
        let mut invalid = wire.clone();
        invalid["table_entity_id_offset"] = serde_json::json!(offset);
        let error = serde_json::from_value::<crate::records::act::ActEntity>(invalid).unwrap_err();
        assert!(error.to_string().contains("table_entity_id_offset"));
    }
    assert!(crate::records::act::ActTableRow::new(u64::MAX - 13).is_err());
    assert!(crate::records::act::ActTableRow::new(u64::MAX - 14).is_ok());
}

#[test]
fn act_component_link_preserves_tracking_references_on_the_wire() {
    let wire = serde_json::json!({
        "id": "stream:act-component-link#0", "byte_offset": 0,
        "record_index": 1, "record_index_offset": 7, "class_tag": "261",
        "instance_root_record": 2, "instance_root_record_offset": 22,
        "tracked_entity_record": 3, "tracked_entity_record_offset": 43,
        "components_root_record": 4, "components_root_record_offset": 63,
        "registry_flag": 0, "registry_flag_offset": 53,
        "entity_id": "0_3", "entity_id_offset": 36,
        "display_name": "", "display_name_offset": 61
    });
    let root: crate::records::act::ActComponentLink = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(serde_json::to_value(root).unwrap(), wire);
    for record in [0, 1, 2, 4, u32::MAX] {
        let mut invalid = wire.clone();
        invalid["tracked_entity_record"] = serde_json::json!(record);
        let link = serde_json::from_value::<crate::records::act::ActComponentLink>(invalid.clone())
            .unwrap();
        assert_eq!(link.references.tracked_entity, record);
        assert_eq!(serde_json::to_value(link).unwrap(), invalid);
    }
    for field in [
        "record_index_offset",
        "instance_root_record_offset",
        "entity_id_offset",
        "tracked_entity_record_offset",
        "registry_flag_offset",
        "display_name_offset",
        "components_root_record_offset",
    ] {
        let mut invalid = wire.clone();
        invalid[field] = serde_json::json!(0);
        assert!(
            serde_json::from_value::<crate::records::act::ActComponentLink>(invalid).is_err(),
            "{field}"
        );
    }
}

#[test]
fn act_root_layout_derives_utf16_offsets_and_bounds_padding() {
    use crate::records::act::ActComponentLayout;
    let layout = ActComponentLayout::new(100, "0_3".into(), "😀".into(), 8).unwrap();
    assert_eq!(layout.record_index_offset(), 107);
    assert_eq!(layout.instance_root_record_offset(), 122);
    assert_eq!(layout.entity_id_offset(), 136);
    assert_eq!(layout.tracked_entity_record_offset(), 143);
    assert_eq!(layout.registry_flag_offset(), 153);
    assert_eq!(layout.display_name_offset(), 161);
    assert_eq!(layout.components_root_record_offset(), 174);
    assert!(ActComponentLayout::new(0, "0_3".into(), String::new(), 0).is_ok());
    for padding in [9, u64::MAX] {
        assert!(ActComponentLayout::new(0, "0_3".into(), String::new(), padding).is_err());
    }
    assert!(ActComponentLayout::new(u64::MAX - 63, "0_3".into(), String::new(), 1).is_ok());
    assert!(ActComponentLayout::new(u64::MAX - 62, "0_3".into(), String::new(), 1).is_err());
    assert!(ActComponentLayout::new(0, String::new(), String::new(), 1).is_err());
}

#[test]
fn act_table_reference_derives_target_offset_and_rejects_wire_drift() {
    let wire = serde_json::json!({
        "id": "stream:act-table-reference#20", "ordinal": 0,
        "byte_offset": 20, "target_record": 3, "target_record_offset": 21
    });
    let reference: crate::records::act::ActTableReference =
        serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(reference.byte_offset(), 20);
    assert_eq!(serde_json::to_value(reference).unwrap(), wire);
    for offset in [0, 20, 22, u64::MAX] {
        let mut invalid = wire.clone();
        invalid["target_record_offset"] = serde_json::json!(offset);
        let error =
            serde_json::from_value::<crate::records::act::ActTableReference>(invalid).unwrap_err();
        assert!(error.to_string().contains("target_record_offset"));
    }
    assert!(crate::records::act::ActTableReference::new(
        format!("stream:act-table-reference#{}", u64::MAX),
        0,
        u64::MAX,
        3
    )
    .is_err());
    assert!(crate::records::act::ActTableReference::new(
        format!("stream:act-table-reference#{}", u64::MAX - 1),
        0,
        u64::MAX - 1,
        3
    )
    .is_ok());
}

#[test]
fn act_guid_derives_payload_offset_and_rejects_wire_drift() {
    let wire = serde_json::json!({
        "id": "stream:act-guid#20", "byte_offset": 20, "guid_offset": 24,
        "ordinal": 0, "guid": "01234567-89ab-cdef-0123-456789abcdef"
    });
    let guid: crate::records::act::ActGuid = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(guid.byte_offset(), 20);
    assert_eq!(guid.guid_offset(), 24);
    assert_eq!(serde_json::to_value(guid).unwrap(), wire);
    for offset in [0, 20, 23, 25, u64::MAX] {
        let mut invalid = wire.clone();
        invalid["guid_offset"] = serde_json::json!(offset);
        let error = serde_json::from_value::<crate::records::act::ActGuid>(invalid).unwrap_err();
        assert!(error.to_string().contains("guid_offset"));
    }
    for text in ["", "01234567-89ab-cdef-0123-456789abcdeg"] {
        let mut invalid = wire.clone();
        invalid["guid"] = serde_json::json!(text);
        let error = serde_json::from_value::<crate::records::act::ActGuid>(invalid).unwrap_err();
        assert!(error.to_string().contains("GUID"));
    }
    let text = "01234567-89ab-cdef-0123-456789abcdef";
    assert!(crate::records::act::ActGuid::new(
        format!("stream:act-guid#{}", u64::MAX - 3),
        u64::MAX - 3,
        0,
        text.into()
    )
    .is_err());
    assert!(crate::records::act::ActGuid::new(
        format!("stream:act-guid#{}", u64::MAX - 4),
        u64::MAX - 4,
        0,
        text.into()
    )
    .is_ok());
}

#[test]
fn act_registry_channel_derives_offsets_and_rejects_invalid_wire() {
    let wire = serde_json::json!({
        "id": "stream:act-registry-channel#20", "ordinal": 0, "byte_offset": 20,
        "name": "abc", "name_offset": 24,
        "guid": "01234567-89ab-cdef-0123-456789abcdef", "guid_offset": 31
    });
    let channel: crate::records::act::ActRegistryChannel =
        serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(channel.byte_offset(), 20);
    assert_eq!(channel.name(), "abc");
    assert_eq!(channel.name_offset(), 24);
    assert_eq!(channel.guid_offset(), 31);
    assert_eq!(serde_json::to_value(channel).unwrap(), wire);
    for field in ["name_offset", "guid_offset"] {
        let mut invalid = wire.clone();
        invalid[field] = serde_json::json!(0);
        assert!(
            serde_json::from_value::<crate::records::act::ActRegistryChannel>(invalid).is_err()
        );
    }
    for name in [String::new(), "x".repeat(129), "é".into()] {
        let mut invalid = wire.clone();
        invalid["name"] = serde_json::json!(name);
        assert!(
            serde_json::from_value::<crate::records::act::ActRegistryChannel>(invalid).is_err()
        );
    }
    let guid = "01234567-89ab-cdef-0123-456789abcdef";
    assert!(crate::records::act::ActRegistryChannel::new(
        format!("stream:act-registry-channel#{}", u64::MAX - 10),
        0,
        u64::MAX - 10,
        "abc".into(),
        guid.into()
    )
    .is_err());
    assert!(crate::records::act::ActRegistryChannel::new(
        format!("stream:act-registry-channel#{}", u64::MAX - 11),
        0,
        u64::MAX - 11,
        "abc".into(),
        guid.into()
    )
    .is_ok());
    assert!(crate::records::act::ActRegistryChannel::new(
        "stream:act-registry-channel#0".into(),
        0,
        0,
        "abc".into(),
        String::new()
    )
    .is_err());
}

#[test]
fn native_act_ids_bind_kind_and_key_at_each_admission_route() {
    for id in [
        "",
        "act-entity#7",
        "stream:act-guid#7",
        "stream:act-entity#8",
        "stream:act-entity#07",
    ] {
        assert!(ActEntity::try_new(id.into(), 7, "0_1".into(), None, group()).is_err());
    }
    reject_changed_id(
        ActEntity::try_new("stream:act-entity#7".into(), 7, "0_1".into(), None, group()).unwrap(),
        &[
            "",
            "act-entity#7",
            "stream:act-guid#7",
            "stream:act-entity#8",
            "stream:act-entity#07",
        ],
    );
    for id in ["id", "stream:act-guid#21", "stream:act-entity#20"] {
        assert!(ActGuid::new(id.into(), 20, 0, GUID.into()).is_err());
    }
    reject_changed_id(
        ActGuid::new("stream:act-guid#20".into(), 20, 0, GUID.into()).unwrap(),
        &["id", "stream:act-guid#21", "stream:act-entity#20"],
    );
    for id in ["id", "stream:act-table-reference#21", "stream:act-guid#20"] {
        assert!(ActTableReference::new(id.into(), 0, 20, 7).is_err());
    }
    reject_changed_id(
        ActTableReference::new("stream:act-table-reference#20".into(), 0, 20, 7).unwrap(),
        &["id", "stream:act-table-reference#21", "stream:act-guid#20"],
    );
    for id in ["id", "stream:act-registry-channel#21", "stream:act-guid#20"] {
        assert!(
            ActRegistryChannel::new(id.into(), 0, 20, "Appearance".into(), GUID.into()).is_err()
        );
    }
    reject_changed_id(
        ActRegistryChannel::new(
            "stream:act-registry-channel#20".into(),
            0,
            20,
            "Appearance".into(),
            GUID.into(),
        )
        .unwrap(),
        &["id", "stream:act-registry-channel#21", "stream:act-guid#20"],
    );
    for id in ["id", "stream:act-component-link#21", "stream:act-guid#20"] {
        assert!(ActComponentLink::try_new(
            id.into(),
            1,
            "261".to_owned().try_into().unwrap(),
            super::ActComponentReferences {
                instance_root: 2,
                tracked_entity: 3,
                components_root: 4
            },
            ActRegistryFlag::Off,
            ActComponentLayout::new(20, "0_3".into(), String::new(), 1).unwrap()
        )
        .is_err());
    }
    let mut root = ActComponentLink::try_new(
        "stream:act-component-link#20".into(),
        1,
        "261".to_owned().try_into().unwrap(),
        super::ActComponentReferences {
            instance_root: 2,
            tracked_entity: 3,
            components_root: 4,
        },
        ActRegistryFlag::Off,
        ActComponentLayout::new(20, "0_3".into(), String::new(), 1).unwrap(),
    )
    .unwrap();
    root.try_set_strings("10_3".into(), "Renamed".into())
        .unwrap();
    assert_eq!(root.id(), "stream:act-component-link#20");
    assert_eq!(root.layout().byte_offset(), 20);
    let before = root.clone();
    assert!(root
        .try_set_strings("invalid".into(), "Name".into())
        .is_err());
    assert_eq!(root, before);
    reject_changed_id(
        root,
        &["id", "stream:act-component-link#21", "stream:act-guid#20"],
    );
}
