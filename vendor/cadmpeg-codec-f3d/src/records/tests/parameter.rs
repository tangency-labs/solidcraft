use super::super::{
    identity::{Located, RecordedValue},
    parameters::{
        DesignParameter, DesignParameterDiscriminator, DesignParameterDraft, DesignParameterSource,
    },
    references::DesignClassTag,
};
use serde_json::json;

fn draft() -> DesignParameterDraft {
    DesignParameterDraft {
        id: "parameter".into(),
        byte_offset: 100,
        class_tag: DesignClassTag::try_from("305".to_owned()).unwrap(),
        record_index: 1,
        source_ordinal: 0,
        source: DesignParameterSource::User {
            source_kind: cadmpeg_core::nonblank_literal!("User Parameter"),
            family_discriminator: Some(Located {
                value: DesignParameterDiscriminator::Code0,
                offset: 122,
            }),
        },
        expression: "-1 mm".into(),
        expression_offset: 140,
        source_kind_offset: 160,
        unit: Some(RecordedValue {
            value: "mm".into(),
            offset: 170,
        }),
        name: "Width".into(),
        name_offset: 180,
        evaluated_value: -1.0,
        evaluated_value_offset: 190,
    }
}

#[test]
fn parameter_rejects_empty_text_and_unlocated_units() {
    let wire = serde_json::to_value(DesignParameter::try_from(draft()).unwrap()).unwrap();
    for field in ["expression", "name", "unit"] {
        let mut invalid = wire.clone();
        invalid[field] = json!("");
        assert!(serde_json::from_value::<DesignParameter>(invalid)
            .unwrap_err()
            .to_string()
            .contains(field));
    }
    let mut invalid = wire;
    invalid.as_object_mut().unwrap().remove("unit_offset");
    assert!(serde_json::from_value::<DesignParameter>(invalid)
        .unwrap_err()
        .to_string()
        .contains("unit_offset"));
}

#[test]
fn parameter_rejects_unordered_offsets_on_the_wire() {
    let wire = serde_json::to_value(DesignParameter::try_from(draft()).unwrap()).unwrap();
    for (field, value) in [
        ("byte_offset", 140),
        ("expression_offset", 100),
        ("expression_offset", 160),
        ("source_kind_offset", 180),
        ("unit_offset", 160),
        ("unit_offset", 180),
        ("name_offset", 190),
        ("evaluated_value_offset", 180),
        ("family_discriminator_offset", 121),
    ] {
        let mut invalid = wire.clone();
        invalid[field] = json!(value);
        assert!(
            serde_json::from_value::<DesignParameter>(invalid).is_err(),
            "{field}"
        );
    }
    let mut no_unit = wire;
    no_unit.as_object_mut().unwrap().remove("unit");
    no_unit.as_object_mut().unwrap().remove("unit_offset");
    no_unit["name_offset"] = json!(160);
    assert!(serde_json::from_value::<DesignParameter>(no_unit).is_err());
}

#[test]
fn parameter_numeric_and_source_edits_keep_the_old_value_on_failure() {
    let parameter = DesignParameter::try_from(draft()).unwrap();
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let mut invalid = draft();
        invalid.evaluated_value = value;
        assert!(DesignParameter::try_from(invalid).is_err());
        let mut edited = parameter.clone();
        assert!(edited.try_set_evaluated_value(value).is_err());
        assert_eq!(edited, parameter);
    }
    let mut edited = parameter.clone();
    assert!(edited
        .try_set_source(DesignParameterSource::User {
            source_kind: cadmpeg_core::nonblank_literal!("User Parameter"),
            family_discriminator: Some(Located {
                value: DesignParameterDiscriminator::Code0,
                offset: 121
            })
        })
        .is_err());
    assert_eq!(edited, parameter);
    assert!(edited.try_set_unit_value(String::new()).is_err());
    assert_eq!(edited, parameter);
    for value in [-1.0, -0.0, 0.0, f64::MAX] {
        edited.try_set_evaluated_value(value).unwrap();
        assert_eq!(edited.evaluated_value().get().to_bits(), value.to_bits());
    }
}
