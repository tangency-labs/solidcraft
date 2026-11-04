// SPDX-License-Identifier: Apache-2.0

use super::{
    DesignClassTag, DesignCompanionPayload, DesignParameter, DesignParameterCompanion,
    DesignParameterCompanionWire, DesignParameterDiscriminator, DesignParameterDraft,
    DesignParameterFlags, DesignParameterOwner, DesignParameterOwnerWire, DesignParameterSerde,
    DesignParameterSource, PARAMETER_CLONE_COUNT, PARAMETER_COMPANION_CLONE_COUNT,
    PARAMETER_OWNER_CLONE_COUNT,
};
use crate::records::identity::{Located, RecordedValue};

fn parameter(owned: bool) -> DesignParameter {
    let source = if owned {
        DesignParameterSource::new("Feature Dimension".into(), Some(4), None).unwrap()
    } else {
        DesignParameterSource::User {
            source_kind: cadmpeg_core::nonblank_literal!("User Parameter"),
            family_discriminator: Some(Located {
                value: DesignParameterDiscriminator::Code0,
                offset: 122,
            }),
        }
    };
    DesignParameter::try_from(DesignParameterDraft {
        id: "f3d:native:parameter#0".into(),
        byte_offset: 100,
        class_tag: DesignClassTag::try_from("305".to_owned()).unwrap(),
        record_index: 1,
        source_ordinal: 0,
        source,
        expression: "-1 mm".into(),
        expression_offset: 140,
        source_kind_offset: 160,
        unit: (!owned).then(|| RecordedValue {
            value: "mm".into(),
            offset: 170,
        }),
        name: "Width".into(),
        name_offset: 180,
        evaluated_value: -1.0,
        evaluated_value_offset: 190,
    })
    .unwrap()
}

#[test]
fn parameter_borrowed_wire_matches_owned_wire_bytes() {
    for record in [parameter(false), parameter(true)] {
        let owned = DesignParameterSerde::from(record.clone());
        assert_eq!(
            serde_json::to_vec(&record).unwrap(),
            serde_json::to_vec(&owned).unwrap()
        );
    }
}

#[test]
fn parameter_flags_round_trip_and_a_document_parameter_has_no_driven_flag() {
    let flags = DesignParameterFlags {
        driven: true,
        favorite: true,
    };
    let record = parameter(true).with_flags(flags).unwrap();
    assert_eq!(record.driving(), Some(false));
    let wire = serde_json::to_value(&record).unwrap();
    assert_eq!(
        (wire["driven"].clone(), wire["favorite"].clone()),
        (true.into(), true.into())
    );
    let read: DesignParameter = serde_json::from_value(wire).unwrap();
    assert_eq!(read.flags(), flags);

    let plain = serde_json::to_value(parameter(true)).unwrap();
    assert!(plain.get("driven").is_none() && plain.get("favorite").is_none());
    assert_eq!(parameter(true).driving(), None);

    assert!(parameter(false).with_flags(flags).is_err());
    let favorite = DesignParameterFlags {
        driven: false,
        favorite: true,
    };
    assert!(parameter(false).with_flags(favorite).is_ok());
    let mut document = serde_json::to_value(parameter(false)).unwrap();
    document["driven"] = true.into();
    assert!(serde_json::from_value::<DesignParameter>(document).is_err());
}

#[test]
fn parameter_native_retained_limit_refuses_before_record_clone() {
    let record = parameter(false);
    crate::test_support::native_test::assert_borrowed_native_retained_limit(
        &record,
        "design_parameters",
        || PARAMETER_CLONE_COUNT.with(|count| count.set(0)),
        || PARAMETER_CLONE_COUNT.with(std::cell::Cell::get),
    );
}

fn owner(variant: Option<u8>) -> DesignParameterOwner {
    DesignParameterOwner::try_from(DesignParameterOwnerWire {
        id: "f3d:native:parameter-owner#0".into(),
        byte_offset: 100,
        frame_length: if variant.is_some() { 104 } else { 103 },
        class_tag: DesignClassTag::try_from("292".to_owned()).unwrap(),
        record_index: 10,
        scope_record_index: 3,
        local_ordinal: 0,
        evaluated_value: 2.0,
        evaluated_value_offset: 140,
        parameter_record_index: 11,
        owned_ordinal: 0,
        variant,
        companion_record_index: 12,
    })
    .unwrap()
}

fn companion(bound: bool) -> DesignParameterCompanion {
    let record = DesignParameterCompanion::unbound(
        "f3d:native:parameter-companion#0".into(),
        201,
        DesignClassTag::try_from("293".to_owned()).unwrap(),
        12,
        10,
        std::num::NonZeroU64::new(1).unwrap(),
        230,
    );
    if bound {
        record.bound(DesignCompanionPayload::new(
            240,
            50,
            vec!["recipe-a".into(), "recipe-b".into()],
        ))
    } else {
        record
    }
}

#[test]
fn parameter_owner_borrowed_wire_matches_owned_wire_bytes() {
    for record in [owner(None), owner(Some(0))] {
        let owned = DesignParameterOwnerWire::from(record.clone());
        assert_eq!(
            serde_json::to_vec(&record).unwrap(),
            serde_json::to_vec(&owned).unwrap()
        );
    }
}

#[test]
fn parameter_owner_native_retained_limit_refuses_before_record_clone() {
    let record = owner(Some(0));
    crate::test_support::native_test::assert_borrowed_native_retained_limit(
        &record,
        "design_parameter_owners",
        || PARAMETER_OWNER_CLONE_COUNT.with(|count| count.set(0)),
        || PARAMETER_OWNER_CLONE_COUNT.with(std::cell::Cell::get),
    );
}

#[test]
fn parameter_companion_borrowed_wire_matches_owned_wire_bytes() {
    for record in [companion(false), companion(true)] {
        let owned = DesignParameterCompanionWire::from(record.clone());
        assert_eq!(
            serde_json::to_vec(&record).unwrap(),
            serde_json::to_vec(&owned).unwrap()
        );
    }
}

#[test]
fn parameter_companion_native_retained_limit_refuses_before_record_clone() {
    let record = companion(true);
    crate::test_support::native_test::assert_borrowed_native_retained_limit(
        &record,
        "design_parameter_companions",
        || PARAMETER_COMPANION_CLONE_COUNT.with(|count| count.set(0)),
        || PARAMETER_COMPANION_CLONE_COUNT.with(std::cell::Cell::get),
    );
}
