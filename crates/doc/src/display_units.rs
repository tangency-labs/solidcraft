//! The existing length display policy, shared by windowed and headless consumers.
//!
//! Values are stored in millimetres and displayed in the five design units, by the
//! `solidcraft-display-units` crate's functions, re-exported here. Unknown unit names retain the
//! millimetre fallback; expression units have a separate, broader vocabulary.
//! These functions format values, not validate them: non-finite inputs retain Rust's `NaN`,
//! `inf` and `-inf` text. Callers that require finite measurements must validate them first.

use serde::{Deserialize, Serialize};

pub use crate::tolerance::{DimensionStyle, DimensionText, DimensionValue, format_dimension};
use crate::{DocError, Result, expr};

pub use solidcraft_display_units::{format_length, length_number, length_precision, unit_scale};

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
    format_display(value / scale, scale, decimals, f, None)
}

/// Format a unit-less number with the same validation and decimal rounding policy.
pub fn format_number(x: f64, decimals: u8, f: &NumberFormat) -> Result<Shown> {
    format_display(x, 1.0, decimals, f, None)
}

/// Format an internal value with a rounding step in the selected display unit.
/// The step is a shortest round-trip decimal, finite, positive and at most 1e6.
/// Input decimal preparation precedes step rounding; requested decimal precision follows it.
/// Both rounding operations use `f.ties`. The original value is never changed.
/// The exact snapped display magnitude must be at most 1e12. An out-of-range snap
/// returns [`DocError::Invalid`] before final decimal-place rounding, even if that
/// final rounding would bring the displayed value back into range.
pub fn format_value_with_step(value: f64, kind: expr::Kind, unit: &str, decimals: u8, step: f64, f: &NumberFormat) -> Result<Shown> {
    let (unit_kind, scale) = expr::unit_info(unit).ok_or_else(|| DocError::Invalid("unknown number display unit".into()))?;
    if unit_kind != kind {
        return Err(DocError::Invalid("number display unit has a different kind".into()));
    }
    format_display(value / scale, scale, decimals, f, Some(step))
}

/// Format a unit-less number with the step policy of [`format_value_with_step`].
/// Decimal precision can round a step multiple again; `Shown.value` always matches the text.
/// Out-of-range snapped magnitudes are rejected before final decimal-place rounding.
pub fn format_number_with_step(x: f64, decimals: u8, step: f64, f: &NumberFormat) -> Result<Shown> {
    format_display(x, 1.0, decimals, f, Some(step))
}

fn format_display(x: f64, scale: f64, decimals: u8, f: &NumberFormat, step: Option<f64>) -> Result<Shown> {
    // The unit table supplies a positive finite scale. Its decimal step must be at most
    // 1e-9 internal units; the precision floor can make the effective tie window narrower.
    let unit_resolution = (scale.log10() + 9.0).ceil().max(0.0) as usize;
    let resolution = unit_resolution.max(usize::from(decimals) + 3);
    let (rounded, value) = match step {
        Some(step) => round_decimal_step(x, step, decimals, resolution, f.ties)?,
        None => round_decimal(x, decimals, resolution, f.ties)?,
    };
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

/// Validate and prepare the magnitude once, identically for both rounding paths.
fn prepare_decimal(x: f64, decimals: u8, resolution: usize) -> Result<String> {
    if decimals > 8 {
        return Err(DocError::Invalid("number precision must be at most 8 decimal places".into()));
    }
    if !x.is_finite() || x.abs() > 1e12 {
        return Err(DocError::Invalid("display value must be finite with magnitude at most 1e12".into()));
    }
    Ok(format!("{:.*}", resolution, x.abs()))
}

/// Prepare a bounded magnitude, then round decimal digits, never a binary tie comparison.
fn round_decimal(x: f64, decimals: u8, resolution: usize, ties: Ties) -> Result<(String, f64)> {
    let prepared = prepare_decimal(x, decimals, resolution)?;
    round_prepared(&prepared, x.is_sign_negative(), decimals, ties)
}

fn round_prepared(prepared: &str, negative: bool, decimals: u8, ties: Ties) -> Result<(String, f64)> {
    let (integer, fraction) = prepared.split_once('.').unwrap_or((prepared, ""));
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
    let negative = negative && magnitude != 0.0;
    Ok((rounded, if negative { -magnitude } else { magnitude }))
}

/// Divide decimal digits by a small decimal coefficient, retaining the full quotient.
/// A shortest f64 coefficient has at most 17 significant digits. With the unit table's
/// resolution <= 15, a scaled divisor is <= 1e21, so remainder * 10 fits u128.
/// At most 324 appended zeros are needed, even for the smallest positive subnormal.
fn round_decimal_step(x: f64, step: f64, decimals: u8, resolution: usize, ties: Ties) -> Result<(String, f64)> {
    if !step.is_finite() || step <= 0.0 || step > 1e6 {
        return Err(DocError::Invalid("number rounding step must be finite, positive and at most 1e6".into()));
    }
    let prepared = prepare_decimal(x, decimals, resolution)?;
    let scientific = format!("{step:e}");
    let (mantissa, exponent) = scientific.split_once('e').ok_or_else(|| DocError::Invalid("invalid decimal step".into()))?;
    let fraction = mantissa.split_once('.').map_or(0, |(_, f)| f.len());
    let coefficient = mantissa.replace('.', "").parse::<u128>().map_err(|_| DocError::Invalid("invalid step coefficient".into()))?;
    let exponent = exponent.parse::<i32>().map_err(|_| DocError::Invalid("invalid step exponent".into()))? - fraction as i32;
    let shift = exponent + resolution as i32;
    let divisor = if shift > 0 { 10_u128.checked_pow(shift as u32).and_then(|n| coefficient.checked_mul(n)) } else { Some(coefficient) }
        .ok_or_else(|| DocError::Invalid("rounding step scale overflow".into()))?;
    let mut remainder = 0_u128;
    let mut quotient = Vec::new();
    for digit in prepared.bytes().filter(|d| *d != b'.').chain(std::iter::repeat_n(b'0', (-shift).max(0) as usize)) {
        remainder = remainder * 10 + u128::from(digit - b'0');
        quotient.push((remainder / divisor) as u8);
        remainder %= divisor;
    }
    let up = remainder * 2 > divisor
        || (remainder * 2 == divisor && (ties == Ties::HalfAwayFromZero || quotient.last().is_some_and(|d| !d.is_multiple_of(2))));
    // Exact quotient * coefficient, with the rounding increment folded into the carry.
    let mut carry = if up { coefficient } else { 0 };
    let mut product = Vec::new();
    for digit in quotient.iter().rev() {
        let n = u128::from(*digit) * coefficient + carry;
        product.push(b'0' + (n % 10) as u8);
        carry = n / 10;
    }
    while carry > 0 {
        product.push(b'0' + (carry % 10) as u8);
        carry /= 10;
    }
    product.reverse();
    let product = String::from_utf8(product).map_err(|_| DocError::Invalid("invalid step digits".into()))?;
    let mut magnitude = product.trim_start_matches('0').to_string();
    if magnitude.is_empty() {
        magnitude.push('0');
    }
    if exponent >= 0 {
        if magnitude != "0" {
            magnitude.extend(std::iter::repeat_n('0', exponent as usize));
        }
    } else {
        let places = (-exponent) as usize;
        if magnitude.len() <= places {
            magnitude = format!("{}{}", "0".repeat(places + 1 - magnitude.len()), magnitude);
        }
        magnitude.insert(magnitude.len() - places, '.');
    }
    let (integer, fraction) = magnitude.split_once('.').unwrap_or((&magnitude, ""));
    let whole = integer.parse::<u64>().map_err(|_| DocError::Invalid("rounded display magnitude overflow".into()))?;
    if whole > 1_000_000_000_000 || (whole == 1_000_000_000_000 && fraction.bytes().any(|d| d != b'0')) {
        return Err(DocError::Invalid("rounded display magnitude must be at most 1e12".into()));
    }
    // round_prepared needs at least the requested fraction digits, including for integer steps.
    let missing = usize::from(decimals).saturating_sub(fraction.len());
    if missing > 0 {
        if !magnitude.contains('.') {
            magnitude.push('.');
        }
        magnitude.extend(std::iter::repeat_n('0', missing));
    }
    round_prepared(&magnitude, x.is_sign_negative(), decimals, ties)
}

pub(crate) fn grouped_digits(text: &mut String, digits: &str, integer: bool, groups: bool) {
    for (i, digit) in digits.chars().enumerate() {
        if groups && digits.len() >= 5 && i > 0 && if integer { (digits.len() - i).is_multiple_of(3) } else { i.is_multiple_of(3) } {
            text.push('\u{202f}');
        }
        text.push(digit);
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

    #[test]
    fn step_ties_and_decimal_fractions() {
        for (x, step, dp, even, away) in [
            (1.25, 0.5, 1, "1.0", "1.5"),
            (1.75, 0.5, 1, "2.0", "2.0"),
            (12.5, 5.0, 0, "10", "15"),
            (25.0, 10.0, 0, "20", "30"),
            (0.125, 0.05, 2, "0.10", "0.15"),
            (0.375, 0.25, 2, "0.50", "0.50"),
            (0.15, 0.1, 1, "0.2", "0.2"),
            (0.25, 0.1, 1, "0.2", "0.3"),
            (1.25, 0.5, 0, "1", "2"),
        ] {
            for (ties, expected) in [(Ties::HalfEven, even), (Ties::HalfAwayFromZero, away)] {
                let f = NumberFormat { ties, ..NumberFormat::default() };
                for sign in [1.0, -1.0] {
                    let shown = format_number_with_step(sign * x, dp, step, &f).unwrap();
                    assert_eq!(shown.text, if sign < 0.0 { format!("−{expected}") } else { expected.into() });
                    assert_eq!(shown.value, sign * expected.parse::<f64>().unwrap());
                }
            }
        }
    }

    #[test]
    fn step_prepares_display_before_division_and_keeps_boundary_neighbors() {
        for ties in [Ties::HalfEven, Ties::HalfAwayFromZero] {
            let f = NumberFormat { ties, ..NumberFormat::default() };
            for x in [1.75_f64.next_down(), 1.75, 1.75_f64.next_up(), 1.7499999996] {
                assert_eq!(format_number_with_step(x, 1, 0.5, &f).unwrap().text, "2.0");
            }
            for x in [1.25_f64.next_down(), 1.25, 1.25_f64.next_up()] {
                let expected = if ties == Ties::HalfEven { "1.0" } else { "1.5" };
                assert_eq!(format_number_with_step(x, 1, 0.5, &f).unwrap().text, expected);
                assert_eq!(format_number_with_step(-x, 1, 0.5, &f).unwrap().text, format!("−{expected}"));
            }
            assert_eq!(format_number_with_step(1.7499999994, 1, 0.5, &f).unwrap().text, "1.5");
            assert_eq!(format_number_with_step(1.7500000006, 1, 0.5, &f).unwrap().text, "2.0");
        }
        let f = NumberFormat::default();
        // The precision floor narrows the preparation window at eight decimals.
        assert_eq!(format_number_with_step(1.7499999996, 8, 0.5, &f).unwrap().text, "1.50000000");
    }

    #[test]
    fn step_tiny_values_large_magnitudes_and_exact_final_digits() {
        let f = NumberFormat::default();
        for step in [f64::from_bits(1), f64::MIN_POSITIVE, 1e-200, 1e-20, 1e-8] {
            for x in [0.0, -0.0, f64::from_bits(1), -1e11, 1e11, 34947628153.461784] {
                let shown = format_number_with_step(x, 5, step, &f).unwrap();
                assert!(shown.value.is_finite());
                assert_eq!(shown.value, shown.text.replace('−', "-").parse::<f64>().unwrap());
            }
        }
        assert_eq!(format_number_with_step(1e12, 8, f64::from_bits(1), &f).unwrap().text, "1000000000000.00000000");
        assert_eq!(format_number_with_step(34947628153.461784, 5, 1e-8, &f).unwrap().text, "34947628153.46178");
        // A tiny step can move an exact decimal tie by less than any f64 ULP.
        // Keep those digits through the second rounding rather than parsing the snap to f64.
        assert_eq!(format_number_with_step(1.25, 1, 3e-323, &f).unwrap().text, "1.3");
        assert_eq!(format_number_with_step(-1.25, 1, 3e-323, &f).unwrap().text, "−1.3");
        for step in [0.5, 10.0, 1e6] {
            for x in [-0.0, -0.1] {
                let shown = format_number_with_step(x, 2, step, &f).unwrap();
                assert_eq!(shown.text, "0.00");
                assert_eq!(shown.value.to_bits(), 0.0_f64.to_bits());
            }
        }
    }

    #[test]
    fn step_extreme_coefficients_have_independent_expected_digits() {
        // Shortest decimals: largest subnormal = 2225073858507201 * 10^-323,
        // smallest normal = 22250738585072014 * 10^-324. For x=1.25, integer
        // division leaves remainders 2221948386734081 and 354296156594724.
        // Twice the first is above its divisor, twice the second below: the
        // snap is just above/below 1.25, so final rounding gives 1.3/1.2 in BOTH modes.
        // Other expected digits were calculated with independent rational arithmetic.
        for (step, quarter) in [(f64::MIN_POSITIVE.next_down(), "1.3"), (f64::MIN_POSITIVE, "1.2")] {
            for ties in [Ties::HalfEven, Ties::HalfAwayFromZero] {
                let f = NumberFormat { ties, ..NumberFormat::default() };
                for (x, dp, expected) in [
                    (1.25, 1, quarter),
                    (99999999999.75, 0, "100000000000"),
                    (99999999999.75, 1, "99999999999.7"),
                    (99999999999.75, 2, "99999999999.75"),
                    (34947628153.461784, 2, "34947628153.46"),
                    (34947628153.461784, 5, "34947628153.46178"),
                ] {
                    for sign in [1.0, -1.0] {
                        let shown = format_number_with_step(sign * x, dp, step, &f).unwrap();
                        let text = if sign < 0.0 { format!("−{expected}") } else { expected.into() };
                        assert_eq!(shown.text, text, "x={x:?}, step={step:e}, dp={dp}, ties={ties:?}");
                        assert_eq!(shown.value, sign * expected.parse::<f64>().unwrap());
                    }
                }
            }
        }
    }

    #[test]
    fn step_and_final_decimal_rounding_carry_independently() {
        // 9.975 / .05 = 199.5: both modes increment the step index to 200.
        // 9.96 / .03 = 332 exactly: only the final rounding carries to 10.0.
        // 1.26 / .1 = 12.6: the snap is 1.3, then zero decimals give 1 (not 1.26).
        // 1.49 / .5 = 2.98: the snap is 1.5, then zero decimals give 2 (not 1).
        for (x, step, dp, even, away) in [
            (9.975, 0.05, 2, "10.00", "10.00"),
            (9.96, 0.03, 1, "10.0", "10.0"),
            (1.26, 0.1, 0, "1", "1"),
            (1.49, 0.5, 0, "2", "2"),
            (100000000000.25, 0.1, 0, "100000000000", "100000000000"),
            (100000000000.25, 0.1, 1, "100000000000.2", "100000000000.3"),
            (100000000000.25, 0.1, 2, "100000000000.20", "100000000000.30"),
            (999999999999.75, 0.25, 0, "1000000000000", "1000000000000"),
        ] {
            for (ties, expected) in [(Ties::HalfEven, even), (Ties::HalfAwayFromZero, away)] {
                let f = NumberFormat { ties, ..NumberFormat::default() };
                for sign in [1.0, -1.0] {
                    let shown = format_number_with_step(sign * x, dp, step, &f).unwrap();
                    assert_eq!(shown.text, if sign < 0.0 { format!("−{expected}") } else { expected.into() });
                    assert_eq!(shown.value, sign * expected.parse::<f64>().unwrap());
                }
            }
        }
    }

    #[test]
    fn step_rejects_invalid_inputs_and_out_of_range_snap() {
        let f = NumberFormat::default();
        for step in [0.0, -0.0, -0.5, f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 1e6_f64.next_up()] {
            assert!(matches!(format_number_with_step(1.0, 2, step, &f), Err(DocError::Invalid(_))));
            assert!(matches!(format_value_with_step(1.0, Length, "mm", 2, step, &f), Err(DocError::Invalid(_))));
        }
        for x in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 1e12_f64.next_up(), -1e12_f64.next_up()] {
            assert!(matches!(format_number_with_step(x, 2, 0.5, &f), Err(DocError::Invalid(_))));
        }
        for dp in [9, 255] {
            assert!(matches!(format_number_with_step(1.0, dp, 0.5, &f), Err(DocError::Invalid(_))));
        }
        for x in [1e12, -1e12] {
            for ties in [Ties::HalfEven, Ties::HalfAwayFromZero] {
                let f = NumberFormat { ties, ..f };
                for step in [600_000.0, 0.6] {
                    // The .6 snap has magnitude 1e12 + .2: reject it even though
                    // zero decimal places would otherwise bring it back to 1e12.
                    assert!(matches!(format_number_with_step(x, 0, step, &f), Err(DocError::Invalid(_))));
                }
            }
            assert_eq!(format_number_with_step(x, 0, 1e6, &f).unwrap().value, x);
        }
        for (kind, unit) in [(Length, "deg"), (Angle, "mm"), (Length, "unknown")] {
            assert!(matches!(format_value_with_step(1.0, kind, unit, 2, 0.5, &f), Err(DocError::Invalid(_))));
        }
    }

    #[test]
    fn step_units_options_and_input_preservation() {
        let f = NumberFormat::default();
        let input: f64 = 25.4 * 1.25;
        let bits = input.to_bits();
        assert_eq!(format_value_with_step(input, Length, "in", 1, 0.5, &f).unwrap().text, "1.0");
        assert_eq!(input.to_bits(), bits);
        assert_eq!(format_value_with_step(std::f64::consts::PI / 6.0, Angle, "deg", 1, 5.0, &f).unwrap().text, "30.0");
        assert_eq!(format_value_with_step(1e15, Length, "m", 1, 10.0, &f).unwrap().value, 1e12);
        let styled = NumberFormat { marker: DecimalMarker::Comma, groups: true, ..f };
        assert_eq!(format_number_with_step(-12345.25, 2, 0.5, &styled).unwrap().text, "−12\u{202f}345,00");
        let trim = NumberFormat { leading_zero: false, trailing_zeros: false, ..styled };
        assert_eq!(format_number_with_step(-0.6, 3, 0.5, &trim).unwrap().text, "−,5");
        assert_eq!(format_number_with_step(1.23456789, 8, 1e-8, &styled).unwrap().text, "1,234\u{202f}567\u{202f}89");
        for (distance, expected) in [(0.4, "1.0"), (0.6, "0.5")] {
            assert_eq!(format_value_with_step((0.75 - distance * 1e-4) * 1e-6, Length, "nm", 1, 0.5, &f).unwrap().text, expected);
        }
        for step in [1e6_f64.next_down(), 0.12345678901234567, 2.2250738585072014e-308] {
            let shown = format_number_with_step(12345.6789, 8, step, &f).unwrap();
            assert_eq!(shown.value, shown.text.parse::<f64>().unwrap());
        }
        // Explicitly keep both historical policies, including their different decimal ties.
        assert_eq!(format_number(2.665, 2, &f).unwrap().text, "2.66");
        assert_eq!(length_number(2.665, "mm", 2), "2.67");
    }

    #[test]
    fn step_seeded_integer_reference_and_hostile_domain() {
        let mut seed = 0x4896_1138_0abc_def1;
        for _ in 0..20_000 {
            // Independent bounded integer oracle: x and step are thousandths; dp=3.
            let n = random(&mut seed) % 1_000_000;
            let s = random(&mut seed) % 100_000 + 1;
            for ties in [Ties::HalfEven, Ties::HalfAwayFromZero] {
                let f = NumberFormat { ties, ..NumberFormat::default() };
                let q = n / s;
                let r = n % s;
                let up = 2 * r > s || (2 * r == s && (ties == Ties::HalfAwayFromZero || q % 2 == 1));
                let expected = (q + u64::from(up)) * s;
                let text = format!("{}.{:03}", expected / 1000, expected % 1000);
                let shown = format_number_with_step(n as f64 / 1000.0, 3, s as f64 / 1000.0, &f).unwrap();
                assert_eq!(shown.text, text);
                // Exercise arbitrary positive subnormals and normal steps without quotient overflow.
                let step = f64::from_bits(random(&mut seed));
                if step.is_finite() && step > 0.0 && step <= 1e6 {
                    let shown = format_number_with_step(n as f64, 3, step, &f).unwrap();
                    assert!(shown.value.is_finite() && shown.value >= 0.0);
                    assert_eq!(shown.value, shown.text.parse::<f64>().unwrap());
                }
            }
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
