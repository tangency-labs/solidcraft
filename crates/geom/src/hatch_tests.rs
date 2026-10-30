use std::f64::consts::{FRAC_PI_4, TAU};

use serde_json::json;

use super::{Family, Pattern};
use crate::Vec2;

fn continuous() -> Family {
    Family { angle: 0.0, origin: Vec2::ZERO, delta_x: 0.0, delta_y: 5.0, dashes: vec![] }
}

fn pattern(family: Family) -> Pattern {
    Pattern { name: "R45-a_$9".into(), description: "  Original pattern  ".into(), families: vec![family] }
}

type SetLength = fn(&mut Family, f64);

fn length_fields() -> [(&'static str, SetLength); 5] {
    [
        ("origin.x", |f, v| f.origin.x = v),
        ("origin.y", |f, v| f.origin.y = v),
        ("delta_x", |f, v| f.delta_x = v),
        ("delta_y", |f, v| f.delta_y = v),
        ("dash", |f, v| f.dashes = vec![2.0, v]),
    ]
}

#[test]
fn original_builtins_have_unit_paper_spacing_and_perpendicular_cross_families() {
    let diagonal = Family { angle: FRAC_PI_4, delta_y: 1.0, ..continuous() };
    let cross = Family { angle: 3.0 * FRAC_PI_4, delta_y: 1.0, ..continuous() };
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
    let expected = pattern(Family { angle: 0.25, origin: Vec2::new(12.5, -7.25), delta_x: -2.0, delta_y: -5.0, dashes: vec![-3.0, 0.0, 1.5] });
    let restored: Pattern = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(restored, expected);
    assert!(restored.validate().is_ok());
    assert_eq!(restored, expected);
    assert_eq!(serde_json::to_value(&restored).unwrap(), value);
}

#[test]
fn validation_keeps_unwrapped_angles_signed_deltas_and_dot_entries() {
    let family =
        Family { angle: 2.0 * TAU + FRAC_PI_4, origin: Vec2::new(4.0, -9.0), delta_x: -2.0, delta_y: -5.0, dashes: vec![0.0, -3.0, -0.0, 2.0] };
    let before = family.clone();
    assert!(family.validate().is_ok());
    assert_eq!(family, before);
    let bits: Vec<u64> = family.dashes.iter().map(|v| v.to_bits()).collect();
    assert_eq!(bits, vec![0.0_f64.to_bits(), (-3.0_f64).to_bits(), (-0.0_f64).to_bits(), 2.0_f64.to_bits()]);
}

#[test]
fn negative_only_sequences_are_valid_records_even_though_they_draw_no_ink() {
    // D1 admits the record; no generator or clipping result is asserted here.
    for dashes in [vec![-3.0], vec![-1.0, -2.0, -3.0], vec![-100_000.0; 6]] {
        let family = Family { dashes, ..continuous() };
        let before = family.clone();
        assert!(family.validate().is_ok());
        assert!(pattern(family.clone()).validate().is_ok());
        assert_eq!(family, before);
    }
}

#[test]
fn empty_dashes_are_continuous_but_nonempty_zero_periods_are_invalid() {
    assert!(continuous().validate().is_ok());
    for dashes in [vec![0.0], vec![-0.0], vec![0.0, -0.0], vec![0.0; 6]] {
        let family = Family { dashes, ..continuous() };
        assert!(family.validate().is_err());
    }
    // A signed sum of zero is not a zero period: 2 mm ink, then 2 mm gap.
    assert!(Family { dashes: vec![2.0, -2.0], ..continuous() }.validate().is_ok());
    assert!(Family { dashes: vec![0.0, -2.0], ..continuous() }.validate().is_ok());
}

#[test]
fn zero_perpendicular_spacing_is_invalid_but_subnormal_spacing_has_no_model_cutoff() {
    for delta_y in [0.0, -0.0] {
        assert!(Family { delta_y, ..continuous() }.validate().is_err());
    }
    let smallest = f64::from_bits(1);
    for delta_y in [smallest, -smallest] {
        let family = Family { delta_y, dashes: vec![0.0, -smallest], ..continuous() };
        assert!(family.validate().is_ok());
        assert_eq!(family.delta_y.to_bits(), delta_y.to_bits());
    }
    // Eventual generation must cap the work independently of record admission.
}

#[test]
fn every_stored_length_accepts_both_inclusive_limits_and_refuses_the_next_float() {
    // Literal policy bounds keep this oracle independent of implementation constants.
    let outside = f64::from_bits(100_000.0_f64.to_bits() + 1);
    for (field, set) in length_fields() {
        for value in [-100_000.0, 100_000.0] {
            let mut family = continuous();
            set(&mut family, value);
            let before = family.clone();
            assert!(family.validate().is_ok(), "{field}: {value}");
            assert_eq!(family, before);
        }
        for value in [-outside, outside] {
            let mut family = continuous();
            set(&mut family, value);
            let before = family.clone();
            assert!(family.validate().is_err(), "{field}: {value}");
            assert_eq!(family, before);
        }
    }
}

#[test]
fn angle_bounds_are_inclusive_and_do_not_wrap_an_out_of_range_angle() {
    for angle in [-1_000_000.0, 1_000_000.0] {
        let family = Family { angle, ..continuous() };
        assert!(family.validate().is_ok());
        assert_eq!(family.angle, angle);
    }
    let outside = f64::from_bits(1_000_000.0_f64.to_bits() + 1);
    for angle in [-outside, outside] {
        let family = Family { angle, ..continuous() };
        assert!(family.validate().is_err());
        assert_eq!(family.angle, angle);
    }
}

#[test]
fn each_numeric_field_refuses_nan_and_both_infinities() {
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(Family { angle: value, ..continuous() }.validate().is_err());
        for (field, set) in length_fields() {
            let mut family = continuous();
            set(&mut family, value);
            assert!(family.validate().is_err(), "{field}: {value}");
            assert!(pattern(family).validate().is_err(), "nested {field}: {value}");
        }
    }
}

#[test]
fn family_and_dash_counts_are_bounded_without_silent_truncation() {
    let mut p = pattern(continuous());
    p.families.clear();
    assert!(p.validate().is_err());
    p.families = vec![continuous(); 64];
    assert!(p.validate().is_ok());
    p.families.push(continuous());
    let before = p.clone();
    assert!(p.validate().is_err());
    assert_eq!(p, before);

    let mut family = Family { dashes: vec![1.0, -2.0, 0.0, 3.0, -4.0, 0.0], ..continuous() };
    assert!(family.validate().is_ok());
    family.dashes.push(1.0);
    let before = family.clone();
    assert!(family.validate().is_err());
    assert_eq!(family, before);
}

#[test]
fn pattern_checks_the_last_family_and_the_last_dash() {
    let mut p = pattern(continuous());
    p.families = vec![continuous(); 64];
    p.families.last_mut().unwrap().delta_y = 0.0;
    let before = p.clone();
    assert!(p.validate().is_err());
    assert_eq!(p, before);

    let family = Family { dashes: vec![1.0, -2.0, 0.0, 3.0, -4.0, f64::INFINITY], ..continuous() };
    assert!(family.validate().is_err());
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
    assert!(serde_json::from_value::<Family>(extra_family.clone()).is_err());
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
