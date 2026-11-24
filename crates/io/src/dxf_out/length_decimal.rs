//! The spelling of a DXF length: the stored binary64 value rounded exactly onto a 1e-9 grid of the
//! file's unit, an exact half tick away from zero, with at least one decimal digit.
//!
//! Lengths only (coordinates, radii, heights, extents, dash lengths). Dimensionless values (angles,
//! ratios, bulges, weights, knots, parameters, normals) keep twelve decimals.
//! Constant work, 24 inline bytes, no heap allocation, never an exponent or a negative zero.
//! Agreed on #5321; written by @0xlillith.

use std::fmt;

/// The largest length written, in the file's unit.
pub(super) const MAX_LENGTH: f64 = 9_000_000.0;
const MAX_TICKS: u128 = 9_000_000_000_000_000;
const TICKS_PER_UNIT: u64 = 1_000_000_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum LengthFormatError {
    NonFinite,
    OutsideLengthRange,
    ArithmeticBounds,
    BufferInvariant,
}

impl LengthFormatError {
    pub(super) fn message(self) -> &'static str {
        match self {
            Self::NonFinite => "DXF length must be finite",
            Self::OutsideLengthRange => "DXF length magnitude exceeds 9000000 in the file's unit",
            Self::ArithmeticBounds => "DXF length integer-grid arithmetic bounds failed",
            Self::BufferInvariant => "DXF length decimal buffer bounds failed",
        }
    }
}

impl fmt::Display for LengthFormatError {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        output.write_str(self.message())
    }
}

/// A length's spelling, held inline.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct LengthDecimal {
    bytes: [u8; 24],
    len: usize,
}

impl LengthDecimal {
    fn push(&mut self, byte: u8) -> Result<(), LengthFormatError> {
        let slot = self.bytes.get_mut(self.len).ok_or(LengthFormatError::BufferInvariant)?;
        *slot = byte;
        self.len += 1;
        Ok(())
    }

    pub(super) fn as_str(&self) -> Result<&str, LengthFormatError> {
        let bytes = self.bytes.get(..self.len).ok_or(LengthFormatError::BufferInvariant)?;
        std::str::from_utf8(bytes).map_err(|_| LengthFormatError::BufferInvariant)
    }
}

/// The exact grid magnitude of the binary64 input, without first rounding a floating-point
/// multiplication (which would send `8000000 + 1/1024` to the even tick below).
fn grid_magnitude(value: f64) -> Result<u64, LengthFormatError> {
    if !value.is_finite() {
        return Err(LengthFormatError::NonFinite);
    }
    if value.abs() > MAX_LENGTH {
        return Err(LengthFormatError::OutsideLengthRange);
    }
    let bits = value.to_bits();
    let exponent = ((bits >> 52) & 0x7ff) as u32;
    let fraction = bits & ((1u64 << 52) - 1);
    let (significand, shift) = if exponent == 0 {
        (fraction, 1074)
    } else {
        // value = significand * 2^(exponent - 1075). Admitted magnitudes keep exponent <= 1046,
        // hence 29 <= shift <= 1074.
        (fraction | (1u64 << 52), 1075u32.checked_sub(exponent).ok_or(LengthFormatError::ArithmeticBounds)?)
    };
    // At most 53 + 30 bits: exact in u128, subnormal significands included.
    let scaled = u128::from(significand).checked_mul(u128::from(TICKS_PER_UNIT)).ok_or(LengthFormatError::ArithmeticBounds)?;
    let rounded = if shift >= 128 {
        // scaled < 2^83, so such a value is strictly below half a tick.
        0
    } else {
        if shift == 0 {
            return Err(LengthFormatError::ArithmeticBounds);
        }
        let quotient = scaled >> shift;
        let remainder = scaled & ((1u128 << shift) - 1);
        let half = 1u128 << (shift - 1);
        quotient.checked_add(u128::from(remainder >= half)).ok_or(LengthFormatError::ArithmeticBounds)?
    };
    if rounded > MAX_TICKS {
        return Err(LengthFormatError::ArithmeticBounds);
    }
    u64::try_from(rounded).map_err(|_| LengthFormatError::ArithmeticBounds)
}

/// A length within ±9000000 as the shortest grid decimal with at least one decimal digit. The
/// sign is written only for a non-zero rounded magnitude.
pub(super) fn length_decimal(value: f64) -> Result<LengthDecimal, LengthFormatError> {
    let ticks = grid_magnitude(value)?;
    let mut output = LengthDecimal { bytes: [0; 24], len: 0 };
    if ticks != 0 && value.is_sign_negative() {
        output.push(b'-')?;
    }
    let mut whole = ticks / TICKS_PER_UNIT;
    let fraction = ticks % TICKS_PER_UNIT;
    let mut reverse = [0u8; 7];
    let mut count = 0usize;
    loop {
        let digit = u8::try_from(whole % 10).map_err(|_| LengthFormatError::ArithmeticBounds)?;
        *reverse.get_mut(count).ok_or(LengthFormatError::BufferInvariant)? = b'0' + digit;
        count += 1;
        whole /= 10;
        if whole == 0 {
            break;
        }
    }
    for byte in reverse.iter().take(count).rev() {
        output.push(*byte)?;
    }
    output.push(b'.')?;
    let decimal_start = output.len;
    let mut divisor = 100_000_000u64;
    for _ in 0..9 {
        let digit = u8::try_from((fraction / divisor) % 10).map_err(|_| LengthFormatError::ArithmeticBounds)?;
        output.push(b'0' + digit)?;
        divisor /= 10;
    }
    while output.len > decimal_start + 1 && output.bytes.get(output.len - 1) == Some(&b'0') {
        output.len -= 1;
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(value: f64) -> String {
        length_decimal(value).unwrap().as_str().unwrap().to_string()
    }

    #[test]
    fn zero_subnormals_signs_and_integral_values_have_explicit_spellings() {
        for value in [0.0, -0.0, f64::from_bits(1), -f64::from_bits(1), f64::MIN_POSITIVE, -f64::MIN_POSITIVE] {
            assert_eq!(text(value), "0.0");
        }
        for (value, expected) in [(1.0, "1.0"), (-1.0, "-1.0"), (12.5, "12.5"), (-12.5, "-12.5")] {
            assert_eq!(text(value), expected);
        }
    }

    #[test]
    fn rational_binary_half_grid_ties_and_neighbors_have_independent_expectations() {
        // 1/1024 * 1e9 = 976562.5 ticks exactly, a binary rational tie.
        let half: f64 = 1.0 / 1024.0;
        for (value, expected) in [
            (half, "0.000976563"),
            (half.next_down(), "0.000976562"),
            (half.next_up(), "0.000976563"),
            (-half, "-0.000976563"),
            ((-half).next_up(), "-0.000976562"),
            ((-half).next_down(), "-0.000976563"),
        ] {
            assert_eq!(text(value), expected);
        }
    }

    #[test]
    fn exact_grid_rounding_does_not_lose_large_half_tick_to_f64_multiplication() {
        // An exact binary value whose half tick is not representable after scaling: the floating
        // product has a spacing of one tick, and rounds the tie to the even tick below.
        let value: f64 = 8_000_000.0 + 1.0 / 1024.0;
        assert_eq!((value * 1e9).round() as u64, 8_000_000_000_976_562);
        assert_eq!(grid_magnitude(value).unwrap(), 8_000_000_000_976_563);
        assert_eq!(text(value), "8000000.000976563");
        assert_eq!(text(-value), "-8000000.000976563");
    }

    #[test]
    fn inclusive_length_bounds_refuse_outside_values_and_all_nonfinite_values() {
        assert_eq!(text(9_000_000.0), "9000000.0");
        assert_eq!(text(-9_000_000.0), "-9000000.0");
        for value in [9_000_000.0f64.next_up(), (-9_000_000.0f64).next_down(), f64::MAX, -f64::MAX] {
            assert_eq!(length_decimal(value), Err(LengthFormatError::OutsideLengthRange));
        }
        for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert_eq!(length_decimal(value), Err(LengthFormatError::NonFinite));
        }
    }

    #[test]
    fn grid_fraction_leading_and_trailing_zeroes_are_decimal_only() {
        for (value, expected) in
            [(1.0 / 512.0, "0.001953125"), (1.0 / 16.0, "0.0625"), (1e-9, "0.000000001"), (-1e-9, "-0.000000001"), (1e-12, "0.0")]
        {
            assert_eq!(text(value), expected);
        }
        for value in [-8_999_999.0, -0.001, 0.0, 0.001, 8_999_999.0] {
            let text = text(value);
            assert!(text.len() <= 18);
            assert!(!text.chars().any(|c| matches!(c, 'e' | 'E' | '+')));
            assert_ne!(text, "-0.0");
        }
    }
}
