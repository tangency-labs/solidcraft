//! The existing length display policy, shared by windowed and headless consumers.
//!
//! Values are stored in millimetres and displayed in the five design units. Unknown unit
//! names retain the millimetre fallback; expression units have a separate, broader vocabulary.
//! These functions format values, not validate them: non-finite inputs retain Rust's `NaN`,
//! `inf` and `-inf` text. Callers that require finite measurements must validate them first.

use serde::{Deserialize, Serialize};

use crate::{DocError, Result, expr};

/// Units per millimetre and the unit's name. Unknown names use millimetres.
pub fn unit_scale(units: &str) -> (f64, &'static str) {
    match units {
        "cm" => (0.1, "cm"),
        "m" => (0.001, "m"),
        "in" => (1.0 / 25.4, "in"),
        "ft" => (1.0 / 304.8, "ft"),
        _ => (1.0, "mm"),
    }
}

/// Inch labels retain a meaningful fourth digit without promoting floating-point noise.
/// `value` is already converted to the requested display unit, not a millimetre value.
pub fn length_precision(value: f64, units: &str, decimals: u8) -> usize {
    let base = usize::from(decimals.min(8));
    if units != "in" {
        return base;
    }
    let needs_four = ((value * 10_000.0).round() - (value * 1000.0).round() * 10.0).abs() >= 0.5;
    base.max(if needs_four { 4 } else { 3 })
}

/// A millimetre length in the requested display unit, without a unit suffix.
pub fn length_number(mm: f64, units: &str, decimals: u8) -> String {
    let value = mm * unit_scale(units).0;
    format!("{:.*}", length_precision(value, units, decimals), value)
}

/// A length in millimetres in `units`, with `decimals` (for display).
pub fn format_length(mm: f64, units: &str, decimals: u8) -> String {
    let (_, u) = unit_scale(units);
    format!("{} {u}", length_number(mm, units, decimals))
}

/// The decimal separator of a number, independent of UI language.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DecimalMarker {
    Point,
    Comma,
}

/// How an exact tie in the prepared decimal digits is rounded.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Ties {
    HalfEven,
    HalfAwayFromZero,
}

/// Number presentation for non-UI consumers. This does not change the legacy length helpers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct NumberFormat {
    pub marker: DecimalMarker,
    pub groups: bool,
    pub leading_zero: bool,
    pub trailing_zeros: bool,
    pub ties: Ties,
}

impl Default for NumberFormat {
    fn default() -> Self {
        Self { marker: DecimalMarker::Point, groups: false, leading_zero: true, trailing_zeros: true, ties: Ties::HalfEven }
    }
}

/// Formatted text without a unit suffix, and the rounded number in the display unit.
#[derive(Clone, Debug, PartialEq)]
pub struct Shown {
    pub text: String,
    pub value: f64,
}

/// Format an internal value (millimetres or radians) in a strictly validated display unit.
pub fn format_value(value: f64, kind: expr::Kind, unit: &str, decimals: u8, f: &NumberFormat) -> Result<Shown> {
    let (unit_kind, scale) = expr::unit_info(unit).ok_or_else(|| DocError::Invalid("unknown number display unit".into()))?;
    if unit_kind != kind {
        return Err(DocError::Invalid("number display unit has a different kind".into()));
    }
    format_display(value / scale, scale, decimals, f)
}

/// Format a unit-less number with the same validation and decimal rounding policy.
pub fn format_number(x: f64, decimals: u8, f: &NumberFormat) -> Result<Shown> {
    format_display(x, 1.0, decimals, f)
}

fn format_display(x: f64, scale: f64, decimals: u8, f: &NumberFormat) -> Result<Shown> {
    // The unit table supplies a positive finite scale. Its decimal step must be at most
    // 1e-9 internal units; the precision floor can make the effective tie window narrower.
    let unit_resolution = (scale.log10() + 9.0).ceil().max(0.0) as usize;
    let resolution = unit_resolution.max(usize::from(decimals) + 3);
    let (rounded, value) = round_decimal(x, decimals, resolution, f.ties)?;
    let (integer, fraction) = rounded.split_once('.').unwrap_or((&rounded, ""));
    let fraction = if f.trailing_zeros { fraction } else { fraction.trim_end_matches('0') };
    let integer = if !f.leading_zero && integer == "0" && !fraction.is_empty() { "" } else { integer };
    let mut text = String::new();
    if value.is_sign_negative() {
        text.push('−');
    }
    grouped_digits(&mut text, integer, true, f.groups);
    if !fraction.is_empty() {
        text.push(match f.marker {
            DecimalMarker::Point => '.',
            DecimalMarker::Comma => ',',
        });
        grouped_digits(&mut text, fraction, false, f.groups);
    }
    Ok(Shown { text, value })
}

/// Prepare a bounded magnitude, then round decimal digits, never a binary tie comparison.
fn round_decimal(x: f64, decimals: u8, resolution: usize, ties: Ties) -> Result<(String, f64)> {
    if decimals > 8 {
        return Err(DocError::Invalid("number precision must be at most 8 decimal places".into()));
    }
    if !x.is_finite() || x.abs() > 1e12 {
        return Err(DocError::Invalid("display value must be finite with magnitude at most 1e12".into()));
    }
    let prepared = format!("{:.*}", resolution, x.abs());
    let (integer, fraction) = prepared.split_once('.').unwrap_or((&prepared, ""));
    let places = usize::from(decimals);
    let mut digits: Vec<u8> = integer.bytes().chain(fraction.bytes().take(places)).collect();
    let discarded = fraction.as_bytes().get(places..).unwrap_or_default();
    let up = match discarded.first().copied() {
        Some(b'6'..=b'9') => true,
        Some(b'5') => {
            discarded.get(1..).unwrap_or_default().iter().any(|d| *d != b'0')
                || match ties {
                    Ties::HalfEven => digits.last().is_some_and(|d| !(d - b'0').is_multiple_of(2)),
                    Ties::HalfAwayFromZero => true,
                }
        }
        _ => false,
    };
    if up {
        let mut carry = true;
        for d in digits.iter_mut().rev() {
            if *d == b'9' {
                *d = b'0';
            } else {
                *d += 1;
                carry = false;
                break;
            }
        }
        if carry {
            digits.insert(0, b'1');
        }
    }
    if places > 0 {
        let point = digits.len().checked_sub(places).ok_or_else(|| DocError::Invalid("invalid prepared number".into()))?;
        digits.insert(point, b'.');
    }
    let rounded = String::from_utf8(digits).map_err(|_| DocError::Invalid("invalid prepared digits".into()))?;
    let magnitude = rounded.parse::<f64>().map_err(|_| DocError::Invalid("invalid rounded number".into()))?;
    let negative = x.is_sign_negative() && magnitude != 0.0;
    Ok((rounded, if negative { -magnitude } else { magnitude }))
}

fn grouped_digits(text: &mut String, digits: &str, integer: bool, groups: bool) {
    for (i, digit) in digits.chars().enumerate() {
        if groups && digits.len() >= 5 && i > 0 && if integer { (digits.len() - i).is_multiple_of(3) } else { i.is_multiple_of(3) } {
            text.push('\u{202f}');
        }
        text.push(digit);
    }
}

#[cfg(test)]
mod tests {
    use super::{format_length, length_number, length_precision, unit_scale};

    #[test]
    fn all_design_units_keep_bare_and_suffixed_labels() {
        for (mm, units, bare, suffixed) in [
            (12.5, "mm", "12.50", "12.50 mm"),
            (125.0, "cm", "12.50", "12.50 cm"),
            (12_500.0, "m", "12.50", "12.50 m"),
            (317.5, "in", "12.500", "12.500 in"),
            (3810.0, "ft", "12.50", "12.50 ft"),
        ] {
            assert_eq!(length_number(mm, units, 2), bare, "{units}");
            assert_eq!(format_length(mm, units, 2), suffixed, "{units}");
        }
    }

    #[test]
    fn inch_labels_keep_three_or_four_digits_without_promoting_noise() {
        for (mm, decimals, expected) in [
            (25.4, 0, "1.000"),
            (25.4 * 1.234, 2, "1.234"),
            (25.4 * 1.2345, 0, "1.2345"),
            (-25.4 * 1.2345, 3, "-1.2345"),
            (25.4 * 0.001, 2, "0.001"),
            (25.4 * (1.0 + 1e-12), 2, "1.000"),
            (25.4, 4, "1.0000"),
            (25.4, 5, "1.00000"),
            (0.0, 0, "0.000"),
        ] {
            assert_eq!(length_number(mm, "in", decimals), expected, "{mm} mm, {decimals} places");
        }
    }

    #[test]
    fn precision_uses_the_converted_value_and_caps_requested_places() {
        assert_eq!(length_precision(1.2345, "in", 2), 4);
        assert_eq!(length_precision(1.234, "in", 2), 3);
        assert_eq!(length_precision(1.2345, "mm", 2), 2);
        assert_eq!(length_precision(1.2345, "in", 6), 6);
        assert_eq!(length_precision(1.2345, "in", u8::MAX), 8);
        assert_eq!(length_precision(1.2345, "mm", u8::MAX), 8);
        assert_eq!(length_number(12.5, "mm", u8::MAX), "12.50000000");
        assert_eq!(length_number(25.4, "in", u8::MAX), "1.00000000");
    }

    #[test]
    fn rounding_and_signed_zero_keep_the_existing_display_policy() {
        for (mm, decimals, expected) in [
            (0.0, 2, "0.00"),
            (-0.0, 2, "-0.00"),
            (-0.0001, 2, "-0.00"),
            (-12.5, 2, "-12.50"),
            (12.5, 0, "12"),
            (13.5, 0, "14"),
            (-12.5, 0, "-12"),
            (-13.5, 0, "-14"),
            (1.25, 1, "1.2"),
            (1.75, 1, "1.8"),
        ] {
            assert_eq!(length_number(mm, "mm", decimals), expected, "{mm} mm, {decimals} places");
        }
        assert_eq!(format_length(-0.0, "in", 2), "-0.000 in");
    }

    #[test]
    fn unknown_display_units_keep_the_millimetre_fallback() {
        for units in ["", "inch", " in ", "IN", "unsupported"] {
            assert_eq!(unit_scale(units), (1.0, "mm"), "{units}");
            assert_eq!(length_number(25.4, units, 2), "25.40", "{units}");
            assert_eq!(format_length(25.4, units, 2), "25.40 mm", "{units}");
        }
    }

    #[test]
    fn non_finite_text_is_preserved_without_claiming_validation() {
        assert_eq!(length_number(f64::NAN, "mm", 2), "NaN");
        assert_eq!(format_length(f64::INFINITY, "in", u8::MAX), "inf in");
        assert_eq!(format_length(f64::NEG_INFINITY, "cm", 2), "-inf cm");
    }
}

#[cfg(test)]
mod number_tests {
    use super::*;
    use expr::Kind::{Angle, Length, Unitless};

    fn pair(x: f64, dp: u8, even: &str, away: &str) {
        for (ties, expected) in [(Ties::HalfEven, even), (Ties::HalfAwayFromZero, away)] {
            let shown = format_number(x, dp, &NumberFormat { ties, ..NumberFormat::default() }).unwrap();
            assert_eq!(shown.text, expected, "x={x:?}, dp={dp}, ties={ties:?}");
            let parsed: f64 = expected.replace('−', "-").parse().unwrap();
            assert_eq!(shown.value, parsed);
        }
    }

    #[test]
    fn number_exact_binary_ties() {
        for (x, dp, even, away) in [
            (0.125, 2, "0.12", "0.13"),
            (0.375, 2, "0.38", "0.38"),
            (12.5, 0, "12", "13"),
            (-12.5, 0, "−12", "−13"),
            (13.5, 0, "14", "14"),
            (-13.5, 0, "−14", "−14"),
        ] {
            pair(x, dp, even, away);
        }
    }

    #[test]
    fn number_decimal_ties() {
        for (x, even, away) in [(2.675, "2.68", "2.68"), (2.665, "2.66", "2.67"), (1.005, "1.00", "1.01"), (0.285, "0.28", "0.29")] {
            pair(x, 2, even, away);
        }
        // The existing UI policy deliberately remains different.
        assert_eq!(length_number(2.665, "mm", 2), "2.67");
        assert_eq!(length_number(2.675, "mm", 2), "2.67");
        assert_eq!(length_number(1.005, "mm", 2), "1.00");
        assert_eq!(length_number(0.285, "mm", 2), "0.28");
    }

    #[test]
    fn number_tie_zone_edges_and_precision_floor() {
        for (x, expected) in [(61.49999999999999, "62"), (61.4999999996, "62"), (61.4999999994, "61"), (61.4999997, "61"), (61.4996, "61")] {
            pair(x, 0, expected, expected);
        }
        pair(1.0000000049, 8, "1.00000000", "1.00000000");
        pair(2.4501, 1, "2.5", "2.5");
        pair(2.5499, 1, "2.5", "2.5");
    }

    #[test]
    fn number_carries() {
        pair(9.995, 2, "10.00", "10.00");
        pair(-9.995, 2, "−10.00", "−10.00");
        pair(99.5, 0, "100", "100");
        pair(0.999999995, 8, "1.00000000", "1.00000000");
    }

    #[test]
    fn number_units_and_aliases() {
        let f = NumberFormat::default();
        for (x, kind, unit, dp, expected) in [
            (25.4, Length, "in", 3, "1.000"),
            (25.4 * (1.0 + 1e-12), Length, "in", 3, "1.000"),
            (25.4, Length, "in", 0, "1"),
            (12.5, Length, "cm", 2, "1.25"),
            (3810.0, Length, "ft", 1, "12.5"),
            (std::f64::consts::PI / 6.0, Angle, "deg", 1, "30.0"),
            (1.0, Angle, "deg", 3, "57.296"),
            (1.0, Angle, "rad", 3, "1.000"),
            (0.001, Length, "um", 3, "1.000"),
            (0.001, Length, "µm", 3, "1.000"),
            (1e-6, Length, "nm", 3, "1.000"),
            (0.0254, Length, "mil", 3, "1.000"),
            (914.4, Length, "yd", 3, "1.000"),
            (1e6, Length, "km", 3, "1.000"),
            (25.4, Length, " inch ", 3, "1.000"),
            (std::f64::consts::PI, Angle, "°", 1, "180.0"),
            (std::f64::consts::PI, Angle, "degrees", 1, "180.0"),
            (1.25, Unitless, "", 2, "1.25"),
            (1.25, Unitless, "none", 2, "1.25"),
            (1.25, Unitless, "unitless", 2, "1.25"),
        ] {
            let shown = format_value(x, kind, unit, dp, &f).unwrap();
            assert_eq!(shown.text, expected, "{x:?} {kind:?} in {unit:?}");
            assert_eq!(shown.value, expected.parse::<f64>().unwrap());
        }
        for (kind, unit) in [(Length, "deg"), (Angle, "mm"), (Length, ""), (Length, "parsec"), (Unitless, "mm"), (Length, "IN")] {
            assert!(matches!(format_value(1.0, kind, unit, 2, &f), Err(DocError::Invalid(_))), "{kind:?} {unit:?}");
        }
    }

    #[test]
    fn number_unit_resolution_edges() {
        let f = NumberFormat { ties: Ties::HalfAwayFromZero, ..NumberFormat::default() };
        for (kind, unit, resolution) in [
            (Length, "mm", 9),
            (Length, "cm", 10),
            (Length, "in", 11),
            (Length, "m", 12),
            (Length, "ft", 12),
            (Angle, "deg", 8),
            (Unitless, "", 9),
            (Length, "nm", 3),
            (Length, "km", 15),
        ] {
            let scale = expr::unit_info(unit).unwrap().1;
            let quantum = 10.0_f64.powi(-resolution);
            for (distance, expected) in [(0.4, "1"), (0.6, "0")] {
                let internal = (0.5 - distance * quantum) * scale;
                assert_eq!(
                    format_value(internal, kind, unit, 0, &f).unwrap().text,
                    expected,
                    "{unit:?}, resolution={resolution}, distance={distance}"
                );
            }
        }
    }

    #[test]
    fn number_zero_sign_caps_and_shown_value() {
        for x in [-0.0, -0.004, f64::from_bits(1)] {
            let shown = format_number(x, 2, &NumberFormat::default()).unwrap();
            assert_eq!(shown.text, "0.00");
            assert_eq!(shown.value.to_bits(), 0.0_f64.to_bits());
        }
        pair(-1.5, 1, "−1.5", "−1.5");
        pair(12.5, 8, "12.50000000", "12.50000000");
        let f = NumberFormat::default();
        for dp in [9, 255] {
            assert!(matches!(format_number(12.5, dp, &f), Err(DocError::Invalid(_))));
            assert!(matches!(format_value(25.4, Length, "in", dp, &f), Err(DocError::Invalid(_))));
        }
        for x in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 1e13, -1e13, 1e12_f64.next_up(), -1e12_f64.next_up()] {
            assert!(matches!(format_number(x, 2, &f), Err(DocError::Invalid(_))), "{x:?}");
            assert!(matches!(format_value(x, Length, "mm", 2, &f), Err(DocError::Invalid(_))), "{x:?}");
        }
        for x in [-1e12, 1e12] {
            assert_eq!(format_number(x, 8, &f).unwrap().value, x);
        }
        assert_eq!(format_value(1e15, Length, "m", 8, &f).unwrap().value, 1e12, "cap is in display units");
        assert_eq!(format_number(2.665, 2, &f).unwrap().value, 2.66);
        assert_eq!(format_value(25.4, Length, "in", 3, &f).unwrap().value, 1.0);
    }

    #[test]
    fn number_format_options_and_group_boundaries() {
        let base = NumberFormat::default();
        let comma = NumberFormat { marker: DecimalMarker::Comma, ..base };
        assert_eq!(format_number(2.665, 2, &comma).unwrap().text, "2,66");
        let no_leading = NumberFormat { leading_zero: false, ..base };
        assert_eq!(format_number(0.5, 3, &no_leading).unwrap().text, ".500");
        let trim = NumberFormat { trailing_zeros: false, ..base };
        assert_eq!(format_number(25.0, 2, &trim).unwrap().text, "25");
        assert_eq!(format_number(25.5, 2, &trim).unwrap().text, "25.5");
        assert_eq!(format_number(0.5, 3, &NumberFormat { leading_zero: false, ..trim }).unwrap().text, ".5");
        let groups = NumberFormat { groups: true, ..base };
        for (x, dp, expected) in [
            (12345.6789, 4, "12\u{202f}345.6789"),
            (1.23456, 5, "1.234\u{202f}56"),
            (1234.5678, 4, "1234.5678"),
            (123456.0, 0, "123\u{202f}456"),
            (1234.0, 0, "1234"),
            (12345.0, 0, "12\u{202f}345"),
            (1.23456789, 8, "1.234\u{202f}567\u{202f}89"),
            (-12345.6789, 4, "−12\u{202f}345.6789"),
            (9999.9, 0, "10\u{202f}000"),
        ] {
            assert_eq!(format_number(x, dp, &groups).unwrap().text, expected, "{x:?}, dp={dp}");
        }
        assert_eq!(format_number(12345.6789, 4, &NumberFormat { groups: true, ..comma }).unwrap().text, "12\u{202f}345,6789");
        assert_eq!(format_number(1.23450, 5, &NumberFormat { groups: true, ..trim }).unwrap().text, "1.2345", "group printed digits after trimming");
    }

    #[test]
    fn number_provisional_leading_zero_for_rounded_zero() {
        // Provisional #5290 assumption: a fractional zero also omits the integer zero.
        let f = NumberFormat { leading_zero: false, ..NumberFormat::default() };
        for x in [0.0, -0.0, -0.004] {
            assert_eq!(format_number(x, 2, &f).unwrap().text, ".00");
            assert_eq!(format_number(x, 0, &f).unwrap().text, "0");
            assert_eq!(format_number(x, 2, &NumberFormat { trailing_zeros: false, ..f }).unwrap().text, "0");
        }
        assert_eq!(format_number(-0.5, 2, &f).unwrap().text, "−.50");
        assert_eq!(format_number(0.0, 2, &NumberFormat { marker: DecimalMarker::Comma, ..f }).unwrap().text, ",00");
    }

    #[test]
    fn number_provisional_serde_convention_and_strict_fields() {
        // Provisional #5290 assumption, matching expr::Kind and other doc enums.
        let f: NumberFormat = serde_json::from_str("{}").unwrap();
        assert_eq!(f, NumberFormat::default());
        assert_eq!(
            serde_json::to_value(f).unwrap(),
            serde_json::json!({
                "marker": "point", "groups": false, "leading_zero": true,
                "trailing_zeros": true, "ties": "half_even"
            })
        );
        let f: NumberFormat = serde_json::from_value(serde_json::json!({"marker":"comma", "ties":"half_away_from_zero"})).unwrap();
        assert_eq!(f.marker, DecimalMarker::Comma);
        assert_eq!(f.ties, Ties::HalfAwayFromZero);
        assert!(f.leading_zero && f.trailing_zeros && !f.groups);
        assert_eq!(serde_json::from_value::<NumberFormat>(serde_json::to_value(f).unwrap()).unwrap(), f);
        for bad in [serde_json::json!({"locale":"en"}), serde_json::json!({"marker":"Point"}), serde_json::json!({"ties":"unknown"})] {
            assert!(serde_json::from_value::<NumberFormat>(bad).is_err());
        }
    }

    fn random(state: &mut u64) -> u64 {
        *state ^= *state << 13;
        *state ^= *state >> 7;
        *state ^= *state << 17;
        *state
    }

    /// Compare the printed decimal exactly with the input's binary fraction. A second f64
    /// subtraction can exceed the bound solely because parsing the correct text rounds again.
    fn decimal_within_bound(x: f64, text: &str, dp: u8, resolution: usize) -> bool {
        let unsigned = text.trim_start_matches('-');
        let (whole, fraction) = unsigned.split_once('.').unwrap_or((unsigned, ""));
        let coefficient: u128 = format!("{whole}{fraction}").parse().unwrap();
        let coefficient = coefficient * 10_u128.pow(u32::from(dp) - fraction.len() as u32);
        let extra = 10_u128.pow((resolution - usize::from(dp)) as u32);
        // Multiply |text - input| <= (10^-dp + 10^-resolution)/2 by 2*10^resolution.
        let center = 2 * coefficient * extra;
        let radius = extra + 1;
        let bits = x.abs().to_bits();
        let exponent = ((bits >> 52) & 0x7ff) as u32;
        let mantissa = u128::from(bits & ((1_u64 << 52) - 1));
        let (mantissa, shift) = if exponent == 0 { (mantissa, 1074) } else { (mantissa | (1_u128 << 52), 1075 - exponent) };
        let input = 2 * mantissa * 10_u128.pow(resolution as u32);
        // Input is below 2^104 at the supported resolutions. A shifted bound above u128's
        // range is therefore certainly above input; saturation preserves the comparison.
        let shifted = |n: u128| {
            if n == 0 {
                0
            } else if shift >= 128 || n > (u128::MAX >> shift) {
                u128::MAX
            } else {
                n << shift
            }
        };
        shifted(center.saturating_sub(radius)) <= input && input <= shifted(center + radius)
    }

    #[test]
    fn number_large_magnitude_decimal_error_bound() {
        let x = 34947628153.461784;
        let shown = format_number(x, 5, &NumberFormat::default()).unwrap();
        assert_eq!(shown.text, "34947628153.46178");
        assert!(decimal_within_bound(x, &shown.text, 5, 9));
        assert!(!decimal_within_bound(x, "34947628153.46177", 5, 9));
        // Another exact binary tie whose rounded decimal has an inexact f64 representation.
        let x = 100000000000.25;
        let shown = format_number(x, 1, &NumberFormat::default()).unwrap();
        assert_eq!(shown.text, "100000000000.2");
        assert!(decimal_within_bound(x, &shown.text, 1, 9));
    }

    #[test]
    fn number_seeded_100k_properties() {
        // Reference resolutions derived by hand from the unit table, not from the formatter.
        let units = [
            ("mm", 9),
            ("cm", 10),
            ("m", 12),
            ("in", 11),
            ("ft", 12),
            ("yd", 12),
            ("km", 15),
            ("um", 6),
            ("µm", 6),
            ("nm", 3),
            ("mil", 8),
            ("inch", 11),
            ("deg", 8),
            ("rad", 9),
            ("°", 8),
            ("degree", 8),
            ("degrees", 8),
            ("", 9),
            ("none", 9),
            ("unitless", 9),
            (" in ", 11),
            ("parsec", 9),
            ("IN", 9),
            ("\0", 9),
            ("../../mm", 9),
            ("🦀", 9),
        ];
        let mut seed = 0x5290_a735_19d2_8bc1;
        let (mut accepted, mut rejected, mut ordered) = (0, 0, 0);
        for case in 0..100_000 {
            let x = f64::from_bits(random(&mut seed));
            let dp = (random(&mut seed) % 13) as u8;
            let bits = random(&mut seed);
            let f = NumberFormat {
                marker: if bits & 1 == 0 { DecimalMarker::Point } else { DecimalMarker::Comma },
                groups: bits & 2 != 0,
                leading_zero: bits & 4 != 0,
                trailing_zeros: bits & 8 != 0,
                ties: if bits & 16 == 0 { Ties::HalfEven } else { Ties::HalfAwayFromZero },
            };
            let kind = match random(&mut seed) % 3 {
                0 => Length,
                1 => Angle,
                _ => Unitless,
            };
            let (unit, base_resolution) = units[(random(&mut seed) % units.len() as u64) as usize];
            let bare = case % 2 == 0;
            let info = if bare { Some((Unitless, 1.0)) } else { expr::unit_info(unit).filter(|(k, _)| *k == kind) };
            let call = |v| if bare { format_number(v, dp, &f) } else { format_value(v, kind, unit, dp, &f) };
            let result = call(x);
            let display = info.map(|(_, scale)| x / scale);
            let valid = dp <= 8 && display.is_some_and(|v| v.is_finite() && v.abs() <= 1e12);
            if !valid {
                rejected += 1;
                assert!(matches!(result, Err(DocError::Invalid(_))), "case {case}: {x:?}, dp={dp}, {kind:?}, {unit:?}: {result:?}");
                continue;
            }
            accepted += 1;
            let shown = result.unwrap();
            let canonical = shown.text.replace(',', ".").replace('\u{202f}', "").replace('−', "-");
            let parsed: f64 = canonical.parse().unwrap();
            assert_eq!(shown.value.to_bits(), parsed.to_bits(), "case {case}: {shown:?}");
            assert!(!shown.text.contains('-'));
            if parsed == 0.0 {
                assert!(!shown.value.is_sign_negative() && !shown.text.starts_with('−'));
            }
            let step = 10.0_f64.powi(-i32::from(dp));
            let resolution = if bare { 9 } else { base_resolution }.max(usize::from(dp) + 3);
            assert!(
                decimal_within_bound(display.unwrap(), &canonical, dp, resolution),
                "case {case}: input={x:?}, dp={dp}, unit={unit:?}, {shown:?}"
            );
            if parsed != 0.0 {
                assert_eq!(parsed.is_sign_negative(), x.is_sign_negative());
            }
            // Shown.value already has display units; converting it again would be wrong.
            assert_eq!(format_number(shown.value, dp, &f).unwrap().text, shown.text, "idempotence case {case}");
            // Fixed settings/units; move by >=2 display steps and several input ULPs.
            // Unlike next_up alone, this requires actual output increases, not just equal zeros.
            let scale = info.unwrap().1;
            let delta = (2.0 * step * scale).max(x.abs() * f64::EPSILON * 4.0);
            let y = x + delta;
            if let Ok(next) = call(y) {
                ordered += 1;
                assert!(y > x && next.value > shown.value, "monotonicity case {case}: {x:?}->{y:?}, dp={dp}, unit={unit:?}: {shown:?}->{next:?}");
            }
        }
        assert!(accepted > 10_000 && rejected > 10_000 && ordered > 10_000, "coverage: accepted={accepted}, rejected={rejected}, ordered={ordered}");
        eprintln!("seed=0x5290a73519d28bc1 cases=100000 accepted={accepted} rejected={rejected} strictly_ordered={ordered}");
    }
}
