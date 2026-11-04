// SPDX-License-Identifier: Apache-2.0

use crate::records::act::{ActChannelGroup, ActEntity};
use std::collections::BTreeMap;

fn group_wire() -> serde_json::Value {
    serde_json::json!({
        "id":"stream:act-entity#7", "record_index":7, "entity_id":"0_1", "in_table":false,
        "channel_class_tag":"261", "channel_record_index_offset":100, "channel_entity_id_offset":200,
        "channels":{"Appearance":"11111111-2222-3333-4444-555555555555"},
        "channel_guid_offsets":{"Appearance":120}
    })
}

#[test]
fn act_group_admission_rejects_invalid_names_counts_and_locations() {
    let channel = crate::records::identity::Located {
        value: "11111111-2222-3333-4444-555555555555"
            .to_owned()
            .try_into()
            .unwrap(),
        offset: 120,
    };
    for name in [String::new(), "x".repeat(129), "é".into()] {
        assert!(ActChannelGroup::try_new(
            100,
            Some(200),
            "261".to_owned().try_into().unwrap(),
            BTreeMap::from([(name, channel.clone())]),
            None
        )
        .is_err());
    }
    for count in [0, 1, 8, 9] {
        let channels = (0..count)
            .map(|index| {
                (
                    format!("Channel{index}"),
                    crate::records::identity::Located {
                        value: channel.value.clone(),
                        offset: 120 + 80 * index,
                    },
                )
            })
            .collect();
        assert_eq!(
            ActChannelGroup::try_new(
                100,
                Some(1000),
                "261".to_owned().try_into().unwrap(),
                channels,
                None
            )
            .is_ok(),
            (1..=8).contains(&count)
        );
    }
    for (field, value) in [
        ("channel_record_index_offset", 120),
        ("channel_entity_id_offset", 191),
    ] {
        let mut wire = group_wire();
        wire[field] = serde_json::json!(value);
        assert!(serde_json::from_value::<ActEntity>(wire).is_err());
    }
    for offset in [100, 129, u64::MAX] {
        let mut wire = group_wire();
        wire["channel_guid_offsets"]["Appearance"] = serde_json::json!(offset);
        assert!(serde_json::from_value::<ActEntity>(wire).is_err());
    }
    for tail_offset in [100, 191, 200] {
        let mut wire = group_wire();
        wire["channel_class_tail"] = serde_json::json!("01");
        wire["channel_class_tail_offset"] = serde_json::json!(tail_offset);
        assert!(serde_json::from_value::<ActEntity>(wire).is_err());
    }
}

#[test]
fn act_entity_requires_a_group_and_a_table_or_inline_key() {
    let mut wire = group_wire();
    wire.as_object_mut()
        .unwrap()
        .remove("channel_entity_id_offset");
    assert!(serde_json::from_value::<ActEntity>(wire.clone()).is_err());
    wire["in_table"] = serde_json::json!(true);
    wire["table_record_index_offset"] = serde_json::json!(20);
    wire["table_entity_id_offset"] = serde_json::json!(34);
    let entity: ActEntity = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(serde_json::to_value(entity).unwrap(), wire);
    for field in ["channel_class_tag", "channel_record_index_offset"] {
        wire.as_object_mut().unwrap().remove(field);
    }
    wire["channels"] = serde_json::json!({});
    wire["channel_guid_offsets"] = serde_json::json!({});
    assert!(serde_json::from_value::<ActEntity>(wire).is_err());
}

#[test]
fn act_entity_edits_keep_keys_and_channel_locations_valid() {
    let mut entity: ActEntity = serde_json::from_value(group_wire()).unwrap();
    for key in ["", "_1", "1_", "a_1", "1_2_3", "1_+2"] {
        let before = entity.clone();
        assert!(entity.try_set_entity_id(key.into()).is_err());
        assert_eq!(entity, before);
        let mut wire = group_wire();
        wire["entity_id"] = serde_json::json!(key);
        assert!(serde_json::from_value::<ActEntity>(wire).is_err());
    }
    entity.try_set_entity_id("000_001".into()).unwrap();
    assert_eq!(entity.entity_id(), "000_001");
    let offset = entity.channel_group().channels()["Appearance"].offset;
    entity
        .set_channel_guid(
            "Appearance",
            "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee"
                .to_owned()
                .try_into()
                .unwrap(),
        )
        .unwrap();
    assert_eq!(
        entity.channel_group().channels()["Appearance"].offset,
        offset
    );
    assert!(entity
        .set_channel_guid(
            "Other",
            "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee"
                .to_owned()
                .try_into()
                .unwrap()
        )
        .is_err());
}
