use crate::display_units::{DecimalMarker, DimensionStyle, DimensionValue, NumberFormat, Ties, format_dimension};
use crate::expr::Kind;
use crate::tolerance::{DimensionWarning, Frame, Indicator, Repeat, Role, Tolerance, TolerancePresentation, ToleranceText};
use crate::{DocError, Result};

fn value(nominal: f64, tolerance: Tolerance) -> DimensionValue {
    DimensionValue {
        nominal,
        kind: Kind::Length,
        tolerance,
        presentation: TolerancePresentation::Deviations,
        role: Role::Normal,
        indicator: None,
        count: 1,
        not_to_scale: false,
    }
}

fn style() -> DimensionStyle {
    DimensionStyle {
        unit: "mm".into(),
        number: NumberFormat { marker: DecimalMarker::Comma, ..NumberFormat::default() },
        decimals: 0,
        tolerance_decimals: 2,
        repeat: Repeat::Times,
    }
}

fn refused<T: std::fmt::Debug>(result: Result<T>, fragment: &str) {
    assert!(matches!(&result, Err(DocError::Invalid(_))), "expected a descriptive Invalid error for {fragment:?}, got {result:?}");
    if let Err(DocError::Invalid(message)) = result {
        assert!(message.contains(fragment), "{message:?} does not name {fragment:?}");
    }
}

#[test]
fn comma_deviations_symmetric_and_unsigned_zero() {
    let s = style();
    let mut v = value(40.0, Tolerance::Deviations { upper: 0.1, lower: -0.05 });
    let text = format_dimension(&v, &s).unwrap();
    assert_eq!(text.nominal, "40");
    assert_eq!(text.tolerance, ToleranceText::Stacked { upper: "+0,10".into(), lower: "−0,05".into() });
    assert_eq!(text.plain(), "40 +0,10/−0,05");
    assert!(!text.plain().contains('-'));
    assert_eq!(text.frame, Frame::None);
    assert!(!text.underline);
    assert!(text.warnings.is_empty());
    for zero in [0.0, -0.0, -0.0001, 0.0001] {
        v.tolerance = Tolerance::Deviations { upper: 0.1, lower: zero };
        assert_eq!(format_dimension(&v, &s).unwrap().plain(), "40 +0,10/0");
        v.tolerance = Tolerance::Deviations { upper: zero, lower: -0.05 };
        assert_eq!(format_dimension(&v, &s).unwrap().plain(), "40 0/−0,05");
    }
    v.tolerance = Tolerance::Symmetric { deviation: 0.05 };
    assert_eq!(format_dimension(&v, &s).unwrap().tolerance, ToleranceText::Symmetric("±0,05".into()));
    assert_eq!(format_dimension(&v, &s).unwrap().plain(), "40 ±0,05");
    v.tolerance = Tolerance::Symmetric { deviation: 0.0001 };
    assert_eq!(format_dimension(&v, &s).unwrap().plain(), "40 ±0");
}

#[test]
fn limits_are_sums_of_the_unrounded_nominal() {
    let s = style();
    let mut v = value(40.0, Tolerance::Deviations { upper: 0.1, lower: -0.05 });
    v.presentation = TolerancePresentation::Limits;
    let text = format_dimension(&v, &s).unwrap();
    assert_eq!(text.nominal, "40");
    assert_eq!(text.tolerance, ToleranceText::Limits { upper: "40,10".into(), lower: "39,95".into() });
    assert_eq!(text.plain(), "40,10/39,95");
    v.nominal = 40.4;
    let text = format_dimension(&v, &s).unwrap();
    assert_eq!(text.nominal, "40");
    assert_eq!(text.tolerance, ToleranceText::Limits { upper: "40,50".into(), lower: "40,35".into() });
    v.tolerance = Tolerance::Symmetric { deviation: 0.1 };
    assert_eq!(format_dimension(&v, &s).unwrap().tolerance, ToleranceText::Limits { upper: "40,50".into(), lower: "40,30".into() });
    v.nominal = -0.05;
    v.tolerance = Tolerance::Deviations { upper: 0.05, lower: -0.1 };
    assert_eq!(
        format_dimension(&v, &s).unwrap().tolerance,
        ToleranceText::Limits { upper: "0,00".into(), lower: "−0,15".into() },
        "limits are absolute numbers; a zero limit keeps its requested decimals"
    );
}

#[test]
fn point_and_independent_precisions() {
    let mut s = style();
    s.number.marker = DecimalMarker::Point;
    s.decimals = 1;
    s.tolerance_decimals = 3;
    let v = value(40.0, Tolerance::Symmetric { deviation: 0.05 });
    assert_eq!(format_dimension(&v, &s).unwrap().plain(), "40.0 ±0.050");
}

#[test]
fn roles_frame_text_and_refuse_every_kind_of_tolerance() {
    let s = style();
    for (role, frame, plain) in [(Role::Normal, Frame::None, "40"), (Role::Auxiliary, Frame::Parentheses, "(40)"), (Role::Exact, Frame::Box, "40")] {
        let mut v = value(40.0, Tolerance::None);
        v.role = role;
        let text = format_dimension(&v, &s).unwrap();
        assert_eq!(text.frame, frame);
        assert_eq!(text.plain(), plain);
        if role != Role::Normal {
            for tolerance in
                [Tolerance::Symmetric { deviation: 0.05 }, Tolerance::Deviations { upper: 0.1, lower: -0.05 }, Tolerance::Fit { class: "H7".into() }]
            {
                v.tolerance = tolerance;
                refused(format_dimension(&v, &s), "roles");
            }
        }
    }
}

#[test]
fn counts_indicators_and_metadata() {
    let mut s = style();
    for (indicator, prefix) in [
        (Indicator::Diameter, "Ø"),
        (Indicator::Radius, "R"),
        (Indicator::Square, "□"),
        (Indicator::SphericalDiameter, "SØ"),
        (Indicator::SphericalRadius, "SR"),
    ] {
        let mut v = value(40.0, Tolerance::None);
        v.indicator = Some(indicator);
        let text = format_dimension(&v, &s).unwrap();
        assert_eq!(text.count, None);
        assert_eq!(text.indicator.as_deref(), Some(prefix));
        assert_eq!(text.plain(), format!("{prefix}40"));
        v.count = 4;
        assert_eq!(format_dimension(&v, &s).unwrap().plain(), format!("4× {prefix}40"));
        s.repeat = Repeat::X;
        assert_eq!(format_dimension(&v, &s).unwrap().count.as_deref(), Some("4X "));
        assert_eq!(format_dimension(&v, &s).unwrap().plain(), format!("4X {prefix}40"));
        s.repeat = Repeat::Times;
        v.not_to_scale = true;
        let text = format_dimension(&v, &s).unwrap();
        assert!(text.underline);
        assert_eq!(text.plain(), format!("4× {prefix}40"));
        v.role = Role::Auxiliary;
        assert_eq!(format_dimension(&v, &s).unwrap().plain(), format!("(4× {prefix}40)"));
    }
    let mut v = value(40.0, Tolerance::None);
    v.count = 10_000;
    assert_eq!(format_dimension(&v, &s).unwrap().plain(), "10000× 40");
}

#[test]
fn fits_accept_exact_letter_codes_and_grade_forms() {
    let s = style();
    let letters = [
        "A", "B", "C", "CD", "D", "E", "EF", "F", "FG", "G", "H", "J", "JS", "K", "M", "N", "P", "R", "S", "T", "U", "V", "X", "Y", "Z", "ZA", "ZB",
        "ZC",
    ];
    for letter in letters {
        for code in [letter.to_string(), letter.to_ascii_lowercase()] {
            for grade in ["01", "0", "1", "6", "7", "18"] {
                let class = format!("{code}{grade}");
                let mut v = value(25.0, Tolerance::Fit { class: class.clone() });
                v.indicator = Some(Indicator::Diameter);
                let text = format_dimension(&v, &s).unwrap();
                assert_eq!(text.tolerance, ToleranceText::Code(class.clone()));
                assert_eq!(text.plain(), format!("Ø25 {class}"));
                assert!(text.warnings.is_empty());
                v.presentation = TolerancePresentation::Limits;
                assert_eq!(format_dimension(&v, &s).unwrap(), text);
            }
        }
    }
    for class in [
        "H19", "Hx", "jS6", "Js7", "7H", "", "I7", "L7", "O7", "Q7", "W7", "H00", "H02", "H001", "H-1", "H7/g6", " H7", "H7 ", "H7\n", "H７", "Η7",
        "\0",
    ] {
        refused(format_dimension(&value(25.0, Tolerance::Fit { class: class.into() }), &s), "fit class");
    }
    let v = value(25.0, Tolerance::Fit { class: "H".repeat(1_000_000) });
    let error = format_dimension(&v, &s).unwrap_err();
    assert!(error.to_string().len() < 160, "a hostile fit must not be copied into the error");
}

#[test]
fn warnings_use_the_shown_nominal_in_the_same_units() {
    let s = style();
    for presentation in [TolerancePresentation::Deviations, TolerancePresentation::Limits] {
        for (nominal, expected) in [(10.4, true), (10.04, false), (-10.4, true), (-10.04, false)] {
            let mut v = value(nominal, Tolerance::Symmetric { deviation: 0.1 });
            v.presentation = presentation;
            let text = format_dimension(&v, &s).unwrap();
            assert_eq!(text.warnings, if expected { vec![DimensionWarning::RoundingOutsideTolerance] } else { vec![] });
        }
    }
    // These asymmetric cases distinguish which nominal centres the tolerance interval.
    for (upper, lower, warns) in [(0.5, -0.1, false), (0.1, -0.5, true)] {
        let v = value(10.4, Tolerance::Deviations { upper, lower });
        assert_eq!(!format_dimension(&v, &s).unwrap().warnings.is_empty(), warns);
    }
    // Inclusive bounds, with binary-exact fractions so the oracle has no decimal ambiguity.
    for (nominal, warns) in [(10.125, false), (10.125_f64.next_up(), true), (9.875, false), (9.875_f64.next_down(), true)] {
        let v = value(nominal, Tolerance::Symmetric { deviation: 0.125 });
        assert_eq!(!format_dimension(&v, &s).unwrap().warnings.is_empty(), warns);
    }
    // Subtraction would make 1.1 - 1 slightly larger than 0.1, falsely excluding
    // the exact endpoint. The next representable value beyond it must still warn.
    for (nominal, warns) in [(1.1, false), (1.1_f64.next_up(), true), (-1.1, false), ((-1.1_f64).next_down(), true)] {
        let v = value(nominal, Tolerance::Symmetric { deviation: 0.1 });
        assert_eq!(!format_dimension(&v, &s).unwrap().warnings.is_empty(), warns, "decimal boundary {nominal:?}");
    }
    let mut inches = s.clone();
    inches.unit = "in".into();
    for (nominal, deviation, warns) in [(35.56, 2.54, true), (26.416, 2.54, false)] {
        let v = value(nominal, Tolerance::Symmetric { deviation });
        assert_eq!(!format_dimension(&v, &inches).unwrap().warnings.is_empty(), warns);
    }
    let v = value(10.4, Tolerance::None);
    assert!(format_dimension(&v, &s).unwrap().warnings.is_empty());
}

#[test]
fn shared_number_policy_converts_nominals_deviations_and_limits() {
    let mut s = style();
    s.number.marker = DecimalMarker::Point;
    s.unit = "in".into();
    s.decimals = 3;
    s.tolerance_decimals = 3;
    let mut v = value(25.4, Tolerance::Symmetric { deviation: 0.254 });
    assert_eq!(format_dimension(&v, &s).unwrap().plain(), "1.000 ±0.010");
    v.presentation = TolerancePresentation::Limits;
    assert_eq!(format_dimension(&v, &s).unwrap().plain(), "1.010/0.990");
    s.unit = "deg".into();
    s.decimals = 1;
    v.kind = Kind::Angle;
    v.nominal = std::f64::consts::PI / 6.0;
    v.tolerance = Tolerance::Symmetric { deviation: std::f64::consts::PI / 180.0 };
    v.presentation = TolerancePresentation::Deviations;
    assert_eq!(format_dimension(&v, &s).unwrap().plain(), "30.0 ±1.000");
    v.kind = Kind::Unitless;
    v.nominal = 2.665;
    v.tolerance = Tolerance::Symmetric { deviation: 0.125 };
    s.unit = "".into();
    s.decimals = 2;
    s.tolerance_decimals = 2;
    assert_eq!(format_dimension(&v, &s).unwrap().plain(), "2.66 ±0.12");
    s.number.ties = Ties::HalfAwayFromZero;
    assert_eq!(format_dimension(&v, &s).unwrap().plain(), "2.67 ±0.13");
    s.number = NumberFormat { marker: DecimalMarker::Comma, groups: true, leading_zero: false, trailing_zeros: false, ties: Ties::HalfEven };
    s.tolerance_decimals = 5;
    v.nominal = 12_345.0;
    v.tolerance = Tolerance::Deviations { upper: 1.23456, lower: -0.50000 };
    assert_eq!(format_dimension(&v, &s).unwrap().plain(), "12\u{202f}345 +1,234\u{202f}56/−,500\u{202f}00");
}

#[test]
fn hostile_numbers_orders_counts_and_precisions_are_refused() {
    let s = style();
    for nominal in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        refused(format_dimension(&value(nominal, Tolerance::None), &s), "nominal");
    }
    for deviation in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 1e6_f64.next_up(), -1e6_f64.next_up()] {
        refused(format_dimension(&value(40.0, Tolerance::Symmetric { deviation }), &s), "symmetric");
        refused(format_dimension(&value(40.0, Tolerance::Deviations { upper: deviation, lower: -0.1 }), &s), "upper");
        refused(format_dimension(&value(40.0, Tolerance::Deviations { upper: 0.1, lower: deviation }), &s), "lower");
    }
    for deviation in [0.0, -0.0, -0.1] {
        refused(format_dimension(&value(40.0, Tolerance::Symmetric { deviation }), &s), "symmetric");
    }
    for (upper, lower) in [(0.0, 0.0), (-0.0, 0.0), (-0.1, 0.1), (0.1, 0.1)] {
        refused(format_dimension(&value(40.0, Tolerance::Deviations { upper, lower }), &s), "upper");
    }
    for count in [0, 10_001, u32::MAX] {
        let mut v = value(40.0, Tolerance::None);
        v.count = count;
        refused(format_dimension(&v, &s), "count");
    }
    for precision in [9, u8::MAX] {
        let mut s = s.clone();
        s.decimals = precision;
        refused(format_dimension(&value(40.0, Tolerance::None), &s), "decimals");
        s.decimals = 0;
        s.tolerance_decimals = precision;
        refused(format_dimension(&value(40.0, Tolerance::None), &s), "tolerance_decimals");
    }
    for unit in ["parsec", "deg", "", "IN", "\0", "../../mm"] {
        let mut s = s.clone();
        s.unit = unit.into();
        refused(format_dimension(&value(40.0, Tolerance::None), &s), "unit");
    }
    // The magnitude cap is in internal units, not the selected display unit.
    let mut s = s;
    s.unit = "m".into();
    refused(format_dimension(&value(0.0, Tolerance::Symmetric { deviation: 1_000_001.0 }), &s), "1e6");
    assert!(format_dimension(&value(0.0, Tolerance::Symmetric { deviation: 1e6 }), &s).is_ok());
    s.unit = "mm".into();
    s.decimals = 8;
    s.tolerance_decimals = 8;
    assert!(format_dimension(&value(1e12, Tolerance::None), &s).is_ok());
    refused(format_dimension(&value(1e12_f64.next_up(), Tolerance::None), &s), "display value");
    let mut v = value(1e12, Tolerance::Symmetric { deviation: 1.0 });
    v.presentation = TolerancePresentation::Limits;
    refused(format_dimension(&v, &s), "display value");
}

fn random(state: &mut u64) -> u64 {
    *state ^= *state << 13;
    *state ^= *state >> 7;
    *state ^= *state << 17;
    *state
}

#[test]
fn dimension_seeded_100k_never_panics_and_is_deterministic() {
    let mut seed = 0x6782_b14e_920d_5ac3;
    let units = [("mm", Kind::Length), ("in", Kind::Length), ("m", Kind::Length), ("deg", Kind::Angle), ("rad", Kind::Angle), ("", Kind::Unitless)];
    let fits = ["H7", "h6", "js6", "JS7", "H01", "ZC18", "H19", "jS6", "", "I7", " H7", "H７", "\0", "a/../../b"];
    let indicators = [
        None,
        Some(Indicator::Diameter),
        Some(Indicator::Radius),
        Some(Indicator::Square),
        Some(Indicator::SphericalDiameter),
        Some(Indicator::SphericalRadius),
    ];
    let mut accepted = [0; 4];
    let mut rejected = 0;
    for case in 0..100_000 {
        let nominal = match random(&mut seed) % 4 {
            0 => f64::from_bits(random(&mut seed)),
            1 => -0.0,
            _ => (random(&mut seed) % 2_000_001) as f64 / 100.0 - 10_000.0,
        };
        let deviation =
            if random(&mut seed).is_multiple_of(5) { f64::from_bits(random(&mut seed)) } else { (random(&mut seed) % 100_000 + 1) as f64 / 100.0 };
        let category = (random(&mut seed) % 4) as usize;
        let tolerance = match category {
            0 => Tolerance::None,
            1 => Tolerance::Symmetric { deviation },
            2 => Tolerance::Deviations { upper: deviation, lower: if random(&mut seed).is_multiple_of(3) { 0.0 } else { -deviation } },
            _ => {
                let class = if random(&mut seed).is_multiple_of(3) {
                    (0..random(&mut seed) % 9).map(|_| char::from_u32((random(&mut seed) % 0x11_0000) as u32).unwrap_or('\u{fffd}')).collect()
                } else {
                    fits[(random(&mut seed) % fits.len() as u64) as usize].into()
                };
                Tolerance::Fit { class }
            }
        };
        let (unit, kind) = units[(random(&mut seed) % units.len() as u64) as usize];
        let mut v = value(nominal, tolerance);
        v.kind = kind;
        v.presentation = if random(&mut seed).is_multiple_of(2) { TolerancePresentation::Deviations } else { TolerancePresentation::Limits };
        v.role = match random(&mut seed) % 5 {
            0 => Role::Exact,
            1 => Role::Auxiliary,
            _ => Role::Normal,
        };
        v.count = match random(&mut seed) % 10 {
            0 => 0,
            1 => 10_001,
            2 => u32::MAX,
            _ => (random(&mut seed) % 10_000 + 1) as u32,
        };
        v.indicator = indicators[(random(&mut seed) % indicators.len() as u64) as usize];
        v.not_to_scale = random(&mut seed).is_multiple_of(2);
        let bits = random(&mut seed);
        let s = DimensionStyle {
            unit: if bits.is_multiple_of(10) { "unknown".into() } else { unit.into() },
            number: NumberFormat {
                marker: if bits & 1 == 0 { DecimalMarker::Point } else { DecimalMarker::Comma },
                groups: bits & 2 != 0,
                leading_zero: bits & 4 != 0,
                trailing_zeros: bits & 8 != 0,
                ties: if bits & 16 == 0 { Ties::HalfEven } else { Ties::HalfAwayFromZero },
            },
            decimals: (random(&mut seed) % 11) as u8,
            tolerance_decimals: (random(&mut seed) % 11) as u8,
            repeat: if bits & 32 == 0 { Repeat::Times } else { Repeat::X },
        };
        let first = format_dimension(&v, &s);
        let second = format_dimension(&v, &s);
        assert_eq!(first, second, "determinism case {case}: {v:?}, {s:?}");
        match first {
            Ok(text) => {
                accepted[category] += 1;
                assert_eq!(text.nominal, crate::display_units::format_value(v.nominal, v.kind, &s.unit, s.decimals, &s.number).unwrap().text);
                if !matches!(&text.tolerance, ToleranceText::Limits { .. }) {
                    assert!(text.plain().contains(&text.nominal), "nominal lost in case {case}: {text:?}");
                }
                if let ToleranceText::Stacked { upper, lower } | ToleranceText::Limits { upper, lower } = &text.tolerance
                    && upper != "0"
                    && lower != "0"
                {
                    let places = |t: &str| t.split_once(['.', ',']).map_or(0, |(_, f)| f.chars().filter(char::is_ascii_digit).count());
                    assert_eq!(places(upper), places(lower), "stacked precision in case {case}: {text:?}");
                }
                assert!(!text.plain().contains('\n'), "plain output must stay on one line");
                assert_eq!(text.underline, v.not_to_scale);
            }
            Err(DocError::Invalid(_)) => rejected += 1,
            other => panic!("unexpected error in case {case}: {other:?}"),
        }
    }
    assert!(accepted.iter().all(|n| *n > 1_000), "must exercise every successful tolerance kind: {accepted:?}");
    assert!(rejected > 10_000, "must exercise hostile inputs");
    eprintln!("seed=0x6782b14e920d5ac3 cases=100000 accepted_by_kind={accepted:?} rejected={rejected}");
}

#[test]
fn limits_replace_the_nominal_in_plain_text() {
    let s = style();
    let mut v = value(40.0, Tolerance::Deviations { upper: 0.1, lower: -0.05 });
    v.presentation = TolerancePresentation::Limits;
    let text = format_dimension(&v, &s).unwrap();
    eprintln!("limits: input=40 +0.1/-0.05 expected=40,10/39,95 actual={}", text.plain());
    assert_eq!(text.plain(), "40,10/39,95");
    assert_eq!(text.nominal, "40", "the nominal remains available as metadata");
    assert_eq!(text.tolerance, ToleranceText::Limits { upper: "40,10".into(), lower: "39,95".into() });
    v.nominal = 40.4;
    v.count = 4;
    v.indicator = Some(Indicator::Diameter);
    assert_eq!(format_dimension(&v, &s).unwrap().plain(), "4× Ø40,50/40,35");
}

#[test]
fn same_sign_deviations_warn_only_after_rounding_moves_the_nominal() {
    let s = style();
    let cases = [
        (40.0, -0.02, -0.05, "40 −0,02/−0,05", false),
        (40.0, 0.05, 0.02, "40 +0,05/+0,02", false),
        (39.96, -0.02, -0.05, "40 −0,02/−0,05", false),
        (40.04, 0.05, 0.02, "40 +0,05/+0,02", false),
        (39.6, -0.02, -0.05, "40 −0,02/−0,05", true),
        (40.4, 0.05, 0.02, "40 +0,05/+0,02", true),
    ];
    let mut observed = Vec::new();
    for (nominal, upper, lower, expected, warns) in cases {
        let text = format_dimension(&value(nominal, Tolerance::Deviations { upper, lower }), &s).unwrap();
        eprintln!(
            "same-sign: input={nominal} {upper}/{lower} expected={expected}, warning={warns}; actual={}, warnings={:?}",
            text.plain(),
            text.warnings
        );
        observed.push((text.plain(), !text.warnings.is_empty()));
    }
    assert_eq!(observed, cases.map(|(_, _, _, text, warns)| (text.to_owned(), warns)));
    // Compare before/after rounding in display units, avoiding a false change from
    // converting the shown value back into internal units.
    for (kind, unit, nominal, scale) in
        [(Kind::Length, "in", 1016.0, 25.4), (Kind::Angle, "deg", std::f64::consts::FRAC_PI_2, std::f64::consts::PI / 180.0)]
    {
        let mut s = s.clone();
        s.unit = unit.into();
        let mut v = value(nominal, Tolerance::Deviations { upper: -0.02 * scale, lower: -0.05 * scale });
        v.kind = kind;
        assert!(format_dimension(&v, &s).unwrap().warnings.is_empty(), "{unit}");
    }
}

#[test]
fn stacked_values_suppress_only_shared_trailing_zeros() {
    let mut s = style();
    s.number.trailing_zeros = false;
    let mut observed = Vec::new();
    let cases = [
        (0.1, -0.05, "40 +0,10/−0,05"),
        (0.1, -0.2, "40 +0,1/−0,2"),
        (0.0, -0.05, "40 0/−0,05"),
        (0.1, -0.0001, "40 +0,1/0"),
        (0.1, 0.05, "40 +0,10/+0,05"),
        (-0.05, -0.1, "40 −0,05/−0,10"),
    ];
    for (upper, lower, expected) in cases {
        let text = format_dimension(&value(40.0, Tolerance::Deviations { upper, lower }), &s).unwrap();
        eprintln!("stacked: input=40 {upper}/{lower}, trim zeros; expected={expected}; actual={}", text.plain());
        observed.push(text.plain());
    }
    assert_eq!(observed, cases.map(|(_, _, text)| text));
    for marker in [DecimalMarker::Comma, DecimalMarker::Point] {
        s.number.marker = marker;
        s.number.leading_zero = false;
        s.number.groups = true;
        s.tolerance_decimals = 5;
        let v = value(12_345.0, Tolerance::Deviations { upper: 1.23456, lower: -0.5 });
        let expected = if marker == DecimalMarker::Comma {
            "12\u{202f}345 +1,234\u{202f}56/−,500\u{202f}00"
        } else {
            "12\u{202f}345 +1.234\u{202f}56/−.500\u{202f}00"
        };
        assert_eq!(format_dimension(&v, &s).unwrap().plain(), expected);
    }
    s.number.marker = DecimalMarker::Comma;
    s.tolerance_decimals = 2;
    let mut v = value(-0.05, Tolerance::Deviations { upper: 0.05, lower: -0.1 });
    v.presentation = TolerancePresentation::Limits;
    assert_eq!(format_dimension(&v, &s).unwrap().plain(), ",00/−,15", "a zero limit shares the pair's precision and leading-zero policy");
}

#[test]
fn unchanged_nominals_do_not_warn_for_unit_conversion_roundoff() {
    let mut s = style();
    s.decimals = 3;
    let cases = [
        (Kind::Length, "in", 1.3 * 25.4, 25.4, "1,300"),
        (Kind::Length, "in", 33.02, 25.4, "1,300"),
        (Kind::Length, "in", 0.762, 25.4, "0,030"),
        (Kind::Length, "ft", 15.24, 304.8, "0,050"),
        (Kind::Length, "cm", 0.7, 10.0, "0,070"),
        (Kind::Angle, "deg", std::f64::consts::FRAC_PI_2, std::f64::consts::PI / 180.0, "90,000"),
    ];
    let mut warned = Vec::new();
    for (kind, unit, nominal, scale, expected) in cases {
        s.unit = unit.into();
        for (upper, lower) in [(-0.02, -0.05), (0.05, 0.02)] {
            let mut v = value(nominal, Tolerance::Deviations { upper: upper * scale, lower: lower * scale });
            v.kind = kind;
            let text = format_dimension(&v, &s).unwrap();
            assert_eq!(text.nominal, expected);
            if !text.warnings.is_empty() {
                warned.push((unit, nominal, text.plain()));
            }
        }
    }
    assert!(warned.is_empty(), "unit conversion alone must not warn: {warned:?}");
    // The roundoff guard is relative, not a fixed tolerance that hides small values.
    for unit in ["mm", "in"] {
        s.unit = unit.into();
        let v = value(1e-20, Tolerance::Symmetric { deviation: 1e-21 });
        assert_eq!(format_dimension(&v, &s).unwrap().warnings, [DimensionWarning::RoundingOutsideTolerance]);
    }
    // Even when conversion underflows, a nonzero internal value can be outside
    // the shown zero's tolerance. Do not mistake that for an unchanged nominal.
    s.unit = "in".into();
    let v = value(f64::from_bits(2), Tolerance::Symmetric { deviation: f64::from_bits(1) });
    assert_eq!(format_dimension(&v, &s).unwrap().warnings, [DimensionWarning::RoundingOutsideTolerance]);
    // Identity units need no roundoff allowance, even for a one-step change.
    s.unit = "mm".into();
    s.decimals = 0;
    let v = value(1.0_f64.next_up(), Tolerance::Deviations { upper: -0.02, lower: -0.05 });
    assert_eq!(format_dimension(&v, &s).unwrap().warnings, [DimensionWarning::RoundingOutsideTolerance]);
}

#[test]
fn repeat_counts_use_the_unitless_number_format() {
    let mut s = style();
    s.number.groups = true;
    let mut v = value(25.4, Tolerance::None);
    v.count = 10_000;
    s.unit = "in".into();
    s.decimals = 3;
    let text = format_dimension(&v, &s).unwrap();
    eprintln!("repeat: input=count 10000, 25.4 mm in inches, groups=true; expected=10\u{202f}000× 1,000; actual={}", text.plain());
    assert_eq!(text.plain(), "10\u{202f}000× 1,000");
    for (repeat, suffix) in [(Repeat::Times, "× "), (Repeat::X, "X ")] {
        s.repeat = repeat;
        for (groups, count, expected) in [(true, 9999, "9999"), (true, 10_000, "10\u{202f}000"), (false, 10_000, "10000")] {
            s.number.groups = groups;
            v.count = count;
            assert_eq!(format_dimension(&v, &s).unwrap().count, Some(format!("{expected}{suffix}")));
        }
        v.count = 1;
        assert_eq!(format_dimension(&v, &s).unwrap().count, None);
    }
}
