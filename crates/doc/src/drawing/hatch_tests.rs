use std::f64::consts::FRAC_PI_4;

use serde_json::json;
use solidcore_record::Vec2;
use solidcore_record::profile::line_family::LineFamily;

use super::Pattern;

fn continuous() -> LineFamily {
    LineFamily { angle: 0.0, origin: Vec2::ZERO, delta_x: 0.0, delta_y: 5.0, dashes: vec![] }
}

fn pattern(family: LineFamily) -> Pattern {
    Pattern { name: "R45-a_$9".into(), description: "  Original pattern  ".into(), families: vec![family] }
}

#[test]
fn original_builtins_have_unit_paper_spacing_and_perpendicular_cross_families() {
    let diagonal = LineFamily { angle: FRAC_PI_4, delta_y: 1.0, ..continuous() };
    let cross = LineFamily { angle: 3.0 * FRAC_PI_4, delta_y: 1.0, ..continuous() };
    let lines = Pattern::lines_45();
    let crossed = Pattern::cross_hatch();
    assert_eq!(lines.name, "LINES_45");
    assert_eq!(crossed.name, "CROSS_HATCH");
    assert_eq!(lines.families, vec![diagonal.clone()]);
    assert_eq!(crossed.families, vec![diagonal, cross]);
    assert!(lines.validate().is_ok());
    assert!(crossed.validate().is_ok());
}

#[test]
fn signed_family_has_an_explicit_json_shape_without_normalization() {
    // An original record, independent of the serializer: origin is in the global
    // pattern frame, while both deltas retain their signed family-frame meaning.
    let value = json!({
        "name": "R45-a_$9",
        "description": "  Original pattern  ",
        "families": [{
            "angle": 0.25,
            "origin": [12.5, -7.25],
            "delta_x": -2.0,
            "delta_y": -5.0,
            "dashes": [-3.0, 0.0, 1.5]
        }]
    });
    let expected = pattern(LineFamily { angle: 0.25, origin: Vec2::new(12.5, -7.25), delta_x: -2.0, delta_y: -5.0, dashes: vec![-3.0, 0.0, 1.5] });
    let restored: Pattern = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(restored, expected);
    assert!(restored.validate().is_ok());
    assert_eq!(restored, expected);
    assert_eq!(serde_json::to_value(&restored).unwrap(), value);
}

#[test]
fn a_pattern_refuses_each_invalid_family_value() {
    for dashes in [vec![-3.0], vec![-1.0, -2.0, -3.0], vec![-100_000.0; 6]] {
        assert!(pattern(LineFamily { dashes, ..continuous() }).validate().is_ok());
    }
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(pattern(LineFamily { angle: value, ..continuous() }).validate().is_err());
        assert!(pattern(LineFamily { origin: Vec2::new(value, 0.0), ..continuous() }).validate().is_err());
        assert!(pattern(LineFamily { delta_x: value, ..continuous() }).validate().is_err());
        assert!(pattern(LineFamily { dashes: vec![2.0, value], ..continuous() }).validate().is_err());
    }
}

#[test]
fn family_counts_are_bounded_without_silent_truncation() {
    let mut p = pattern(continuous());
    p.families.clear();
    assert!(p.validate().is_err());
    p.families = vec![continuous(); 64];
    assert!(p.validate().is_ok());
    p.families.push(continuous());
    let before = p.clone();
    assert!(p.validate().is_err());
    assert_eq!(p, before);
}

#[test]
fn pattern_checks_the_last_family_and_the_last_dash() {
    let mut p = pattern(continuous());
    p.families = vec![continuous(); 64];
    p.families.last_mut().unwrap().delta_y = 0.0;
    let before = p.clone();
    assert!(p.validate().is_err());
    assert_eq!(p, before);

    let family = LineFamily { dashes: vec![1.0, -2.0, 0.0, 3.0, -4.0, f64::INFINITY], ..continuous() };
    assert!(pattern(family).validate().is_err());
}

#[test]
fn names_use_the_bounded_ascii_alphanumeric_first_identifier_grammar() {
    for name in ["A".to_owned(), "9_a-Z$0".to_owned(), "x".repeat(31)] {
        let p = Pattern { name: name.clone(), ..pattern(continuous()) };
        assert!(p.validate().is_ok(), "{name}");
        assert_eq!(p.name, name);
    }
    for name in ["", "_A", "-A", "$A", " A", "A ", "A B", "A,B", "A/B", "A.B", "Aé", "Ａ", "A\n", "A\0"] {
        assert!(Pattern { name: name.into(), ..pattern(continuous()) }.validate().is_err(), "{name:?}");
    }
    let p = Pattern { name: "x".repeat(32), ..pattern(continuous()) };
    assert!(p.validate().is_err());
    assert_eq!(p.name.len(), 32);
}

#[test]
fn description_limit_counts_unicode_scalars_not_utf8_bytes_and_keeps_spaces() {
    for description in [String::new(), "  Trama α  ".into(), "a".repeat(80), "é".repeat(80), "🟧".repeat(80), "e\u{301}".repeat(40)] {
        let p = Pattern { description: description.clone(), ..pattern(continuous()) };
        assert!(p.validate().is_ok());
        assert_eq!(p.description, description);
    }
    for description in ["a".repeat(81), "é".repeat(81), "🟧".repeat(81), "e\u{301}".repeat(41)] {
        let p = Pattern { description: description.clone(), ..pattern(continuous()) };
        assert!(p.validate().is_err());
        assert_eq!(p.description, description);
    }
}

#[test]
fn descriptions_refuse_ascii_and_unicode_control_characters() {
    for control in ['\0', '\t', '\n', '\r', '\u{7f}', '\u{85}', '\u{9f}'] {
        let p = Pattern { description: format!("before{control}after"), ..pattern(continuous()) };
        let before = p.clone();
        assert!(p.validate().is_err(), "{control:?}");
        assert_eq!(p, before);
    }
}

#[test]
fn unknown_fields_are_refused_at_both_pattern_and_family_boundaries() {
    let family = json!({"angle": 0.0, "origin": [0.0, 0.0], "delta_x": 0.0, "delta_y": 5.0, "dashes": []});
    let p = json!({"name": "A", "description": "", "families": [family.clone()]});
    assert!(serde_json::from_value::<Pattern>(p.clone()).is_ok());
    let mut extra_pattern = p.clone();
    extra_pattern["view_scale"] = json!(2.0);
    assert!(serde_json::from_value::<Pattern>(extra_pattern).is_err());
    let mut extra_family = family;
    extra_family["delta_z"] = json!(1.0);
    let mut nested = p;
    nested["families"] = json!([extra_family]);
    assert!(serde_json::from_value::<Pattern>(nested).is_err());
}

#[test]
fn decoding_is_not_numeric_validation_and_failed_validation_preserves_the_record() {
    let value = json!({
        "name": "A", "description": "Unchanged", "families": [{
            "angle": 0.0, "origin": [0.0, 0.0], "delta_x": 2.0, "delta_y": 0.0, "dashes": [1.0, -1.0]
        }]
    });
    let restored: Pattern = serde_json::from_value(value.clone()).unwrap();
    assert!(restored.validate().is_err());
    assert_eq!(serde_json::to_value(&restored).unwrap(), value);

    let mut edited = pattern(continuous());
    assert!(edited.validate().is_ok());
    edited.families.last_mut().unwrap().delta_y = 0.0;
    assert!(edited.validate().is_err());
}
