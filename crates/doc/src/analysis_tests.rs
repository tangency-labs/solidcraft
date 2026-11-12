use serde_json::json;

use super::{MAX_SETUP_BYTES, MAX_SETUP_DEPTH, MAX_STUDIES, Studies, Study, StudyKind};

fn setup(v: serde_json::Value) -> serde_json::Map<String, serde_json::Value> {
    v.as_object().cloned().unwrap_or_default()
}

#[test]
fn a_study_is_saved_and_read_back_as_it_was() {
    let mut studies = Studies::default();
    studies.studies.insert(
        "Hot end".into(),
        Study { kind: StudyKind::Thermal, setup: setup(json!({"cells": 32, "material": "Aluminum", "conditions": [{"side": "-x", "fixed_c": 90}]})) },
    );
    let text = serde_json::to_string(&studies).unwrap();
    let back: Studies = serde_json::from_str(&text).unwrap();
    assert_eq!(back, studies, "a study survives the round trip the design file makes it take");
}

#[test]
fn an_empty_set_of_studies_is_not_written_into_the_design() {
    // The design file gains nothing from designs that were never analysed.
    assert!(Studies::default().is_empty());
}

#[test]
fn a_kind_names_the_command_it_runs() {
    assert_eq!(StudyKind::Thermal.command(), "sim.thermal");
    assert_eq!(StudyKind::Flow.command(), "sim.flow");
    assert_eq!(StudyKind::Stress.command(), "sim.stress");
    for kind in StudyKind::ALL {
        assert_eq!(StudyKind::from_id(kind.id()), Some(*kind), "{} reads back", kind.id());
    }
    assert_eq!(StudyKind::from_id("buckling"), None, "a kind we have no solver for is not a kind");
}

#[test]
fn a_name_that_is_not_a_label_is_refused() {
    assert!(Studies::validate_name("Hot end").is_ok());
    assert!(Studies::validate_name("").is_err(), "empty");
    assert!(Studies::validate_name("  ").is_err(), "all space");
    assert!(Studies::validate_name(" leading").is_err(), "untrimmed");
    assert!(Studies::validate_name("two\nlines").is_err(), "a control character");
    assert!(Studies::validate_name(&"x".repeat(129)).is_err(), "past the limit");
    assert!(Studies::validate_name(&"x".repeat(128)).is_ok(), "at the limit");
}

#[test]
fn a_design_with_too_many_studies_is_refused() {
    let mut studies = Studies::default();
    for i in 0..=MAX_STUDIES {
        studies.studies.insert(format!("Study {i}"), Study::new(StudyKind::Thermal));
    }
    assert!(studies.validate().is_err(), "{} studies is past the limit", studies.studies.len());
}

#[test]
fn a_setup_nested_past_the_limit_is_refused_rather_than_walked() {
    // A design file is input: a setup nested a thousand deep must be refused on the way in,
    // not discovered by a stack overflow on the way through.
    let mut deep = json!(1);
    for _ in 0..MAX_SETUP_DEPTH + 2 {
        deep = json!([deep]);
    }
    let mut studies = Studies::default();
    studies.studies.insert("Deep".into(), Study { kind: StudyKind::Flow, setup: setup(json!({"conditions": deep})) });
    let err = studies.validate().expect_err("refused");
    assert!(format!("{err}").contains("deep"), "{err}");
}

#[test]
fn a_setup_larger_than_the_limit_is_refused() {
    let mut studies = Studies::default();
    studies.studies.insert("Big".into(), Study { kind: StudyKind::Stress, setup: setup(json!({"note": "x".repeat(MAX_SETUP_BYTES + 1)})) });
    let err = studies.validate().expect_err("refused");
    assert!(format!("{err}").contains("bytes of setup"), "{err}");
}

#[test]
fn a_hostile_design_is_refused_as_it_is_read_rather_than_after() {
    // Validation hangs off deserialization, so a design file carrying a bad study never
    // becomes a Document at all.
    let text = format!(r#"{{"studies":{{"{}":{{"kind":"thermal"}}}}}}"#, "n".repeat(200));
    let err = serde_json::from_str::<Studies>(&text).expect_err("refused while reading");
    assert!(format!("{err}").contains("name"), "{err}");
}
