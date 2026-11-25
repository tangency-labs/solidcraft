use std::f64::consts::{FRAC_PI_4, TAU};

use serde_json::json;

use super::LineFamily;
use crate::Vec2;

fn continuous() -> LineFamily {
    LineFamily {
        angle: 0.0,
        origin: Vec2::ZERO,
        delta_x: 0.0,
        delta_y: 5.0,
        dashes: vec![],
    }
}

type SetLength = fn(&mut LineFamily, f64);

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
fn validation_keeps_unwrapped_angles_signed_deltas_and_dot_entries() {
    let family = LineFamily {
        angle: 2.0 * TAU + FRAC_PI_4,
        origin: Vec2::new(4.0, -9.0),
        delta_x: -2.0,
        delta_y: -5.0,
        dashes: vec![0.0, -3.0, -0.0, 2.0],
    };
    let before = family.clone();
    assert!(family.validate().is_ok());
    assert_eq!(family, before);
    let bits: Vec<u64> = family.dashes.iter().map(|v| v.to_bits()).collect();
    assert_eq!(
        bits,
        vec![
            0.0_f64.to_bits(),
            (-3.0_f64).to_bits(),
            (-0.0_f64).to_bits(),
            2.0_f64.to_bits()
        ]
    );
}

#[test]
fn negative_only_sequences_are_valid_records_even_though_they_draw_no_ink() {
    // D1 admits the record; no generator or clipping result is asserted here.
    for dashes in [vec![-3.0], vec![-1.0, -2.0, -3.0], vec![-100_000.0; 6]] {
        let family = LineFamily {
            dashes,
            ..continuous()
        };
        let before = family.clone();
        assert!(family.validate().is_ok());
        assert_eq!(family, before);
    }
}

#[test]
fn empty_dashes_are_continuous_but_nonempty_zero_periods_are_invalid() {
    assert!(continuous().validate().is_ok());
    for dashes in [vec![0.0], vec![-0.0], vec![0.0, -0.0], vec![0.0; 6]] {
        let family = LineFamily {
            dashes,
            ..continuous()
        };
        assert!(family.validate().is_err());
    }
    // A signed sum of zero is not a zero period: 2 mm ink, then 2 mm gap.
    assert!(
        LineFamily {
            dashes: vec![2.0, -2.0],
            ..continuous()
        }
        .validate()
        .is_ok()
    );
    assert!(
        LineFamily {
            dashes: vec![0.0, -2.0],
            ..continuous()
        }
        .validate()
        .is_ok()
    );
}

#[test]
fn zero_perpendicular_spacing_is_invalid_but_subnormal_spacing_has_no_model_cutoff() {
    for delta_y in [0.0, -0.0] {
        assert!(
            LineFamily {
                delta_y,
                ..continuous()
            }
            .validate()
            .is_err()
        );
    }
    let smallest = f64::from_bits(1);
    for delta_y in [smallest, -smallest] {
        let family = LineFamily {
            delta_y,
            dashes: vec![0.0, -smallest],
            ..continuous()
        };
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
        let family = LineFamily {
            angle,
            ..continuous()
        };
        assert!(family.validate().is_ok());
        assert_eq!(family.angle, angle);
    }
    let outside = f64::from_bits(1_000_000.0_f64.to_bits() + 1);
    for angle in [-outside, outside] {
        let family = LineFamily {
            angle,
            ..continuous()
        };
        assert!(family.validate().is_err());
        assert_eq!(family.angle, angle);
    }
}

#[test]
fn each_numeric_field_refuses_nan_and_both_infinities() {
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(
            LineFamily {
                angle: value,
                ..continuous()
            }
            .validate()
            .is_err()
        );
        for (field, set) in length_fields() {
            let mut family = continuous();
            set(&mut family, value);
            assert!(family.validate().is_err(), "{field}: {value}");
        }
    }
}

#[test]
fn dash_counts_are_bounded_without_silent_truncation() {
    let mut family = LineFamily {
        dashes: vec![1.0, -2.0, 0.0, 3.0, -4.0, 0.0],
        ..continuous()
    };
    assert!(family.validate().is_ok());
    family.dashes.push(1.0);
    let before = family.clone();
    assert!(family.validate().is_err());
    assert_eq!(family, before);
}

#[test]
fn a_family_checks_its_last_dash() {
    let family = LineFamily {
        dashes: vec![1.0, -2.0, 0.0, 3.0, -4.0, f64::INFINITY],
        ..continuous()
    };
    assert!(family.validate().is_err());
}

#[test]
fn unknown_fields_are_refused() {
    let family =
        json!({"angle": 0.0, "origin": [0.0, 0.0], "delta_x": 0.0, "delta_y": 5.0, "dashes": []});
    assert!(serde_json::from_value::<LineFamily>(family.clone()).is_ok());
    let mut extra_family = family;
    extra_family["delta_z"] = json!(1.0);
    assert!(serde_json::from_value::<LineFamily>(extra_family).is_err());
}
