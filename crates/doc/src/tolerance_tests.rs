use crate::display_units::{DecimalMarker, DimensionStyle, DimensionValue, NumberFormat, Ties, format_dimension};
use crate::expr::Kind;
use crate::tolerance::{
    DecimalsRow, DimensionWarning, Frame, GeneralTolerance, Indicator, RangeRow, Repeat, Role, Tolerance, TolerancePresentation, ToleranceText,
};
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
    assert_eq!(format_dimension(&v, &s).unwrap().plain(), "4× Ø40,50/Ø40,35");
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

#[test]
fn legacy_limits_repeat_dimension_indicators() {
    let mut s = style();
    s.tolerance_decimals = 0;
    let cases = [
        (Some(Indicator::Diameter), 1, 11.0, "Ø12/Ø10"),
        (Some(Indicator::Diameter), 4, 11.0, "4× Ø12/Ø10"),
        (Some(Indicator::Diameter), 1, 1.0, "Ø2/Ø0"),
        (Some(Indicator::SphericalDiameter), 1, 11.0, "SØ12/SØ10"),
        (None, 1, 11.0, "12/10"),
    ];
    let mut actual = Vec::new();
    for (indicator, count, nominal, expected) in cases {
        let mut v = value(nominal, Tolerance::Symmetric { deviation: 1.0 });
        v.presentation = TolerancePresentation::Limits;
        v.indicator = indicator;
        v.count = count;
        let text = format_dimension(&v, &s).unwrap();
        eprintln!("limits input={nominal}±1 indicator={indicator:?} count={count}; expected={expected}; actual={}", text.plain());
        assert_eq!(text.nominal, nominal.to_string());
        assert!(matches!(&text.tolerance, ToleranceText::Limits { upper, lower } if !upper.contains('Ø') && !lower.contains('Ø')));
        actual.push(text.plain());
    }
    assert_eq!(actual, cases.map(|(_, _, _, expected)| expected));
    s.unit = "in".into();
    s.number.marker = DecimalMarker::Point;
    s.decimals = 3;
    s.tolerance_decimals = 3;
    let mut v = value(25.4, Tolerance::Symmetric { deviation: 0.127 });
    v.indicator = Some(Indicator::Diameter);
    v.presentation = TolerancePresentation::Limits;
    assert_eq!(format_dimension(&v, &s).unwrap().plain(), "Ø1.005/Ø0.995");
    v.presentation = TolerancePresentation::Deviations;
    assert_eq!(format_dimension(&v, &s).unwrap().plain(), "Ø1.000 ±0.005");
}

#[test]
fn canonical_formatter_keeps_the_existing_public_paths() {
    let mut v = value(11.0, Tolerance::Symmetric { deviation: 1.0 });
    v.presentation = TolerancePresentation::Limits;
    v.indicator = Some(Indicator::Diameter);
    let s = style();
    let canonical = crate::tolerance::format_dimension(&v, &s).unwrap();
    let original = crate::display_units::format_dimension(&v, &s).unwrap();
    assert_eq!(canonical, original);
    let _: &crate::tolerance::DimensionValue = &v;
    let _: &crate::tolerance::DimensionStyle = &s;
    let _: crate::tolerance::DimensionText = original;
}

const DEG: f64 = std::f64::consts::PI / 180.0;

fn decimals_block(unit: &str, rows: &[(u8, f64)], angle: Option<f64>) -> GeneralTolerance {
    let linear = rows.iter().map(|&(decimals, deviation)| DecimalsRow { decimals, deviation }).collect();
    GeneralTolerance::ByDecimals { unit: unit.into(), linear, angle }
}

fn range_block(unit: &str, rows: &[(Option<f64>, f64)], angle: Option<f64>) -> GeneralTolerance {
    let ranges = rows.iter().map(|&(up_to, deviation)| RangeRow { up_to, deviation }).collect();
    GeneralTolerance::ByRange { unit: unit.into(), ranges, angle }
}

/// The decimal-places block: 0 dp ±0.5, 1 dp ±0.1, 2 dp ±0.05, angles ±0.3°.
fn by_decimals() -> GeneralTolerance {
    decimals_block("mm", &[(0, 0.5), (1, 0.1), (2, 0.05)], Some(0.3 * DEG))
}

/// The range block: up to 100 ±1, over 100 up to 1000 ±2, over 1000 ±3.
fn by_range() -> GeneralTolerance {
    range_block("mm", &[(Some(100.0), 1.0), (Some(1000.0), 2.0), (None, 3.0)], None)
}

/// Lengths shown in `unit` with `decimals`, a point and trailing zeros.
fn shown(unit: &str, decimals: u8) -> DimensionStyle {
    DimensionStyle { unit: unit.into(), number: NumberFormat::default(), decimals, ..style() }
}

/// The general tolerance a plain length of `nominal` mm, shown in `unit` with `decimals`, takes.
fn general(block: &GeneralTolerance, nominal: f64, unit: &str, decimals: u8) -> Option<f64> {
    block.effective(&value(nominal, Tolerance::None), &shown(unit, decimals)).unwrap()
}

/// A decimal-places block goes by the decimals a length is shown with: `25` ±0.5, `25.0` ±0.1,
/// `25.00` ±0.05, and none at 3 dp (untoleranced); in inches the decimals mean something else, so
/// a millimetre block gives none. Angles take its one angle value: `30.0°` ±0.3°.
#[test]
fn a_decimal_places_block_goes_by_the_shown_decimals() {
    let block = by_decimals();
    assert_eq!([0, 1, 2, 3].map(|d| general(&block, 25.0, "mm", d)), [Some(0.5), Some(0.1), Some(0.05), None]);
    assert_eq!(general(&block, 25.4, "in", 1), None);
    let angle = DimensionValue { kind: Kind::Angle, ..value(30.0 * DEG, Tolerance::None) };
    assert_eq!(block.effective(&angle, &shown("deg", 1)).unwrap(), Some(0.3 * DEG));
}

/// A range block goes by the size a length is shown with, each row up to its end: 100 ±1, 1000
/// ±2, 1000.5 at 1 dp ±3, but 1000.4 at 0 dp is shown `1000`, ±2; 100.04 is ±2 at 2 dp and shown
/// `100` at 0 dp ±1; 4 in (101.6 mm) ±2; a size is a magnitude. A shown size within 1e-9 mm of a
/// row's end is at it; past a closed last row, none; no angle value, none for an angle.
#[test]
fn a_range_block_goes_by_the_shown_size() {
    let block = by_range();
    let cases =
        [(100.0, 0, 1.0), (1000.0, 0, 2.0), (1000.5, 1, 3.0), (1000.4, 0, 2.0), (100.04, 2, 2.0), (100.04, 0, 1.0), (0.0, 0, 1.0), (-50.0, 0, 1.0)];
    for (nominal, decimals, deviation) in cases {
        assert_eq!(general(&block, nominal, "mm", decimals), Some(deviation), "{nominal} at {decimals} dp");
    }
    assert_eq!(general(&block, 101.6, "in", 0), Some(2.0));
    let near = range_block("mm", &[(Some(100.0 - 5e-10), 1.0), (None, 2.0)], None);
    assert_eq!(general(&near, 100.0, "mm", 0), Some(1.0));
    let closed = range_block("mm", &[(Some(100.0), 1.0)], None);
    assert_eq!(general(&closed, 150.0, "mm", 0), None);
    let angle = DimensionValue { kind: Kind::Angle, ..value(30.0 * DEG, Tolerance::None) };
    assert_eq!(block.effective(&angle, &shown("deg", 1)).unwrap(), None);
}

/// A range block reads its ends in its own unit: in an inch block, up to 4 in ±0.254, then ±0.5,
/// 3.00 in takes 0.254, 4.00 in is at the end and takes 0.254 too, and 5.00 in takes 0.5. The same
/// lengths shown in millimetres take the same rows.
#[test]
fn a_range_block_reads_its_ends_in_its_unit() {
    let inch = range_block("in", &[(Some(4.0), 0.254), (None, 0.5)], None);
    for (inches, deviation) in [(3.0, 0.254), (4.0, 0.254), (5.0, 0.5)] {
        assert_eq!(general(&inch, inches * 25.4, "in", 2), Some(deviation), "{inches} in");
        assert_eq!(general(&inch, inches * 25.4, "mm", 1), Some(deviation), "{inches} in shown in mm");
    }
}

/// A block its check refuses is refused when it judges a dimension too, so a block read from a file
/// unchecked never gives a negative or endless ±.
#[test]
fn an_unchecked_block_is_refused_by_effective() {
    let negative = decimals_block("mm", &[(0, -5.0)], None);
    refused(negative.effective(&value(25.0, Tolerance::None), &shown("mm", 0)), "deviations");
    let endless = range_block("mm", &[(None, 1.0)], Some(f64::INFINITY));
    let angle = DimensionValue { kind: Kind::Angle, ..value(0.5, Tolerance::None) };
    refused(endless.effective(&angle, &shown("deg", 0)), "angle");
}

/// No general tolerance for a dimension with its own tolerance or fit code, or an auxiliary or
/// exact one (ISO 129-1 4.1.4, 4.1.5), and none from a block of `None`.
#[test]
fn own_tolerances_auxiliary_and_exact_dimensions_take_none() {
    for block in [by_decimals(), by_range()] {
        assert!(general(&block, 25.0, "mm", 0).is_some());
        let own = [Tolerance::Symmetric { deviation: 0.2 }, Tolerance::Deviations { upper: 0.1, lower: 0.0 }, Tolerance::Fit { class: "H7".into() }];
        for tolerance in own {
            assert_eq!(block.effective(&value(25.0, tolerance.clone()), &shown("mm", 0)).unwrap(), None, "{tolerance:?}");
        }
        for role in [Role::Auxiliary, Role::Exact] {
            assert_eq!(block.effective(&DimensionValue { role, ..value(25.0, Tolerance::None) }, &shown("mm", 0)).unwrap(), None, "{role:?}");
        }
    }
    assert_eq!(general(&GeneralTolerance::None, 25.0, "mm", 0), None);
}

/// Every limit of a block, each refused with its reason; a dimension the block can't judge is
/// refused too.
#[test]
fn a_general_tolerance_out_of_its_limits_is_refused() {
    let point = NumberFormat::default();
    for ok in [by_decimals(), by_range(), GeneralTolerance::None, range_block("in", &[(Some(1e12), 1e6)], Some(std::f64::consts::PI))] {
        ok.validate(&point).unwrap();
    }
    let open = |d| range_block("mm", &[(None, d)], None);
    for unit in ["deg", "", "parsec"] {
        refused(range_block(unit, &[(None, 1.0)], None).validate(&point), "length unit");
    }
    refused(decimals_block("mm", &[], None).validate(&point), "1 to 64 rows");
    let rows: Vec<(Option<f64>, f64)> = (1..=65).map(|i| (Some(f64::from(i)), 1.0)).collect();
    refused(range_block("mm", &rows, None).validate(&point), "1 to 64 rows");
    range_block("mm", rows.get(..64).unwrap(), None).validate(&point).unwrap();
    for d in [0.0, -0.1, f64::NAN, f64::INFINITY, 1e6 * (1.0 + f64::EPSILON)] {
        refused(open(d).validate(&point), "deviations");
        refused(decimals_block("mm", &[(0, d)], None).validate(&point), "deviations");
    }
    for a in [0.0, -1.0, f64::NAN, 3.2] {
        refused(range_block("mm", &[(None, 1.0)], Some(a)).validate(&point), "angle");
    }
    refused(decimals_block("mm", &[(9, 0.1)], None).validate(&point), "decimals are 0 to 8");
    refused(decimals_block("mm", &[(1, 0.1), (1, 0.2)], None).validate(&point), "decimals are 0 to 8, each once");
    for ends in [[Some(100.0), Some(100.0)], [Some(100.0), Some(50.0)], [Some(0.0), Some(1.0)], [Some(f64::NAN), None], [Some(1.0), Some(1e13)]] {
        refused(range_block("mm", &[(ends[0], 1.0), (ends[1], 2.0)], None).validate(&point), "ranges go up");
    }
    refused(range_block("mm", &[(None, 1.0), (Some(100.0), 2.0)], None).validate(&point), "last range");
    let no_zeros = NumberFormat { trailing_zeros: false, ..point };
    refused(by_decimals().validate(&no_zeros), "trailing zeros");
    by_range().validate(&no_zeros).unwrap();
    let mut bare = shown("mm", 1);
    bare.number.trailing_zeros = false;
    refused(by_decimals().effective(&value(25.0, Tolerance::None), &bare), "trailing zeros");
    refused(by_range().effective(&value(f64::NAN, Tolerance::None), &shown("mm", 0)), "finite");
    assert!(by_range().effective(&value(25.0, Tolerance::None), &shown("parsec", 0)).is_err());
    assert!(by_range().effective(&DimensionValue { kind: Kind::Unitless, ..value(25.0, Tolerance::None) }, &shown("mm", 0)).is_err());
}

/// Blocks and dimensions, three in four tidy (valid blocks, lengths in the block's unit with a
/// point and trailing zeros) and the rest hostile (any unit, no rows or 65, non-finite, zero and
/// negative numbers, ends out of order, decimals over 8, no trailing zeros), 20,000 seeded cases:
/// checking and judging never panic and give the same answer twice, a block that fails its check
/// is refused when it judges, and a block that passes it gives only its own values, to plain
/// dimensions only.
#[test]
fn general_tolerance_seeded_never_panics_and_is_deterministic() {
    let mut seed = 0x4896_e09e_5eed_7a11;
    let hostile = [0.0, -1.0, -0.0, 0.1, 1e13, f64::NAN, f64::INFINITY, f64::MIN_POSITIVE, 1e300];
    let sizes = [0.0, 1.0, 25.0, 25.4, 99.99, 100.0, 100.04, 999.6, 1000.0, 1000.5, 1e6];
    let units = ["mm", "in", "cm", "deg", "", "parsec"];
    let any = |seed: &mut u64, list: &[f64]| list.get((random(seed) % list.len() as u64) as usize).copied().unwrap_or(0.0);
    let (mut checked, mut judged) = (0, 0);
    for case in 0..20_000 {
        let tidy = !random(&mut seed).is_multiple_of(4);
        let unit = if tidy { units[(random(&mut seed) % 3) as usize] } else { units[(random(&mut seed) % 6) as usize] };
        let rows = if tidy {
            1 + random(&mut seed) % 4
        } else if random(&mut seed).is_multiple_of(10) {
            65
        } else {
            random(&mut seed) % 6
        };
        let angle = match (tidy, random(&mut seed) % 2) {
            (true, 0) => Some(0.3 * DEG),
            (false, 0) => Some(any(&mut seed, &hostile)),
            _ => None,
        };
        let block = if random(&mut seed).is_multiple_of(2) {
            let linear: Vec<(u8, f64)> = (0..rows)
                .map(|i| if tidy { (i as u8, 0.5 / 10f64.powi(i as i32)) } else { ((random(&mut seed) % 11) as u8, any(&mut seed, &hostile)) })
                .collect();
            decimals_block(unit, &linear, angle)
        } else {
            let ranges: Vec<(Option<f64>, f64)> = (0..rows)
                .map(|i| match tidy {
                    true => ((i + 1 < rows || random(&mut seed).is_multiple_of(2)).then(|| 100.0 * 10f64.powi(i as i32)), 1.0 + i as f64),
                    false => ((!random(&mut seed).is_multiple_of(4)).then(|| any(&mut seed, &hostile)), any(&mut seed, &hostile)),
                })
                .collect();
            range_block(unit, &ranges, angle)
        };
        let mut v = value(if tidy { any(&mut seed, &sizes) } else { f64::from_bits(random(&mut seed)) }, Tolerance::None);
        v.kind = [Kind::Length, Kind::Length, Kind::Length, Kind::Angle, Kind::Unitless][(random(&mut seed) % 5) as usize];
        v.role = [Role::Normal, Role::Normal, Role::Normal, Role::Auxiliary, Role::Exact][(random(&mut seed) % 5) as usize];
        if random(&mut seed).is_multiple_of(6) {
            v.tolerance = Tolerance::Symmetric { deviation: 0.1 };
        }
        let number = NumberFormat { trailing_zeros: tidy || random(&mut seed).is_multiple_of(2), ..NumberFormat::default() };
        let style_unit = match (tidy, v.kind) {
            (true, Kind::Angle) => "deg",
            (true, _) => unit,
            (false, _) => units[(random(&mut seed) % 6) as usize],
        };
        let decimals = if tidy { random(&mut seed) % 4 } else { random(&mut seed) % 11 };
        let style = DimensionStyle { number, ..shown(style_unit, decimals as u8) };
        let valid = block.validate(&number).is_ok();
        let first = block.effective(&v, &style);
        // The same answer twice, compared by bits: a block that fails `validate` may hold NaN, which `==` never equals.
        let bits = |r: &Result<Option<f64>>| r.as_ref().map(|d| d.map(f64::to_bits)).map_err(ToString::to_string);
        assert_eq!(bits(&first), bits(&block.effective(&v, &style)), "case {case}");
        assert!(valid || first.is_err(), "case {case}: {block:?} fails its check but judged {first:?}");
        checked += usize::from(valid);
        if let (true, Ok(Some(d))) = (valid, &first) {
            judged += 1;
            let own = match &block {
                GeneralTolerance::ByDecimals { linear, angle, .. } => linear.iter().any(|r| r.deviation == *d) || *angle == Some(*d),
                GeneralTolerance::ByRange { ranges, angle, .. } => ranges.iter().any(|r| r.deviation == *d) || *angle == Some(*d),
                GeneralTolerance::None => false,
            };
            assert!(own && v.role == Role::Normal && v.tolerance == Tolerance::None, "case {case}: {d} from {block:?} for {v:?}");
        }
    }
    assert!(checked > 10_000 && judged > 1_000, "checked {checked}, judged {judged}");
}
