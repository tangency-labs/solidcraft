//! Exact stored-binary64 phase arithmetic within a fixed, checked integer envelope.

use super::Invalid;
use super::budget::Budget;

type Result<T> = std::result::Result<T, Invalid>;

const MIN_EXPONENT: i32 = -1074;
const MAX_EXPONENT: i32 = 1023;
const ENVELOPE: Invalid = Invalid("hatch phase exceeds exact arithmetic envelope");
const PARAMETER: Invalid = Invalid("hatch phase cannot form a finite parameter enclosure");

/// Exact coefficient * 2^exponent. Nonzero coefficients are odd and never MIN.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Dyadic {
    coefficient: i128,
    exponent: i32,
}

/// Tight finite neighbours and one nearest-even conversion of an exact parameter.
/// Only Dyadic::param constructs this certificate; callers cannot supply a triple.
#[derive(Clone, Copy, Debug)]
pub(super) struct Param {
    lo: f64,
    hi: f64,
    nearest: f64,
}

impl Param {
    pub fn lo(self) -> f64 {
        self.lo
    }

    pub fn hi(self) -> f64 {
        self.hi
    }

    pub fn nearest(self) -> f64 {
        self.nearest
    }
}

impl Dyadic {
    pub const ZERO: Self = Self { coefficient: 0, exponent: 0 };

    pub fn from_parts(coefficient: i128, exponent: i32, budget: &mut Budget) -> Result<Self> {
        budget.work(2)?;
        normalize(coefficient, exponent)
    }

    pub fn from_f64(value: f64, budget: &mut Budget) -> Result<Self> {
        budget.work(2)?;
        if !value.is_finite() {
            return Err(Invalid("nonfinite hatch phase input"));
        }
        let bits = value.to_bits();
        let field = i32::try_from((bits >> 52) & 0x7ff).map_err(|_| ENVELOPE)?;
        let fraction = bits & ((1u64 << 52) - 1);
        let (significand, exponent) = if field == 0 { (fraction, MIN_EXPONENT) } else { (fraction | (1u64 << 52), field - 1075) };
        let magnitude = i128::from(significand);
        let coefficient = if bits >> 63 == 0 { magnitude } else { magnitude.checked_neg().ok_or(ENVELOPE)? };
        normalize(coefficient, exponent)
    }

    pub fn add(self, rhs: Self, budget: &mut Budget) -> Result<Self> {
        budget.work(4)?;
        if self.coefficient == 0 {
            return Ok(rhs);
        }
        if rhs.coefficient == 0 {
            return Ok(self);
        }
        let (a, b, exponent) = align(self, rhs)?;
        normalize(a.checked_add(b).ok_or(ENVELOPE)?, exponent)
    }

    pub fn sub(self, rhs: Self, budget: &mut Budget) -> Result<Self> {
        budget.work(4)?;
        if rhs.coefficient == 0 {
            return Ok(self);
        }
        if self == rhs {
            return Ok(Self::ZERO);
        }
        if self.coefficient == 0 {
            return normalize(rhs.coefficient.checked_neg().ok_or(ENVELOPE)?, rhs.exponent);
        }
        let (a, b, exponent) = align(self, rhs)?;
        normalize(a.checked_sub(b).ok_or(ENVELOPE)?, exponent)
    }

    pub fn mul_index(self, index: i64, budget: &mut Budget) -> Result<Self> {
        budget.work(4)?;
        if index == 0 || self.coefficient == 0 {
            return Ok(Self::ZERO);
        }
        // Widen before manipulating the sign, including i64::MIN. Extract powers
        // of two before multiplying so an exact scale need not grow the coefficient.
        let factor = i128::from(index);
        let shift = factor.trailing_zeros();
        let odd = factor.checked_shr(shift).ok_or(ENVELOPE)?;
        let exponent = self.exponent.checked_add(i32::try_from(shift).map_err(|_| ENVELOPE)?).ok_or(ENVELOPE)?;
        normalize(self.coefficient.checked_mul(odd).ok_or(ENVELOPE)?, exponent)
    }

    pub fn floor_ratio(self, positive: Self, budget: &mut Budget) -> Result<i128> {
        budget.work(4)?;
        if !positive.is_positive() {
            return Err(Invalid("hatch phase divisor must be positive"));
        }
        if self.coefficient == 0 {
            return Ok(0);
        }
        let (numerator, denominator, _) = align(self, positive)?;
        // Positive denominator makes Euclidean division floor toward -infinity,
        // with 0 <= remainder < denominator, also for negative phase indices.
        numerator.checked_div_euclid(denominator).ok_or(ENVELOPE)
    }

    pub fn is_positive(self) -> bool {
        self.coefficient > 0
    }

    pub fn param(self, budget: &mut Budget) -> Result<Param> {
        budget.work(4)?;
        if self.coefficient == 0 {
            return Ok(Param { lo: 0.0, hi: 0.0, nearest: 0.0 });
        }
        let magnitude = u128::try_from(self.coefficient.checked_abs().ok_or(PARAMETER)?).map_err(|_| PARAMETER)?;
        let width = u128::BITS - magnitude.leading_zeros();
        let leading = self.exponent.checked_add(i32::try_from(width).map_err(|_| PARAMETER)?).and_then(|n| n.checked_sub(1)).ok_or(PARAMETER)?;
        let (lower, upper, rounded) = if leading < -1022 {
            // Every admitted dyadic is a multiple of the least subnormal. Thus a
            // subnormal result is exact, never rounded down from a smaller quantum.
            let shift = u32::try_from(self.exponent.checked_add(1074).ok_or(PARAMETER)?).map_err(|_| PARAMETER)?;
            let limit = ((1u128 << 52) - 1).checked_shr(shift).ok_or(PARAMETER)?;
            if magnitude > limit {
                return Err(PARAMETER);
            }
            let fraction = magnitude.checked_shl(shift).ok_or(PARAMETER)?;
            let value = f64::from_bits(u64::try_from(fraction).map_err(|_| PARAMETER)?);
            (value, value, value)
        } else {
            let (significand, remainder, round_up) = if width > 53 {
                let shift = width - 53;
                let unit = 1u128.checked_shl(shift).ok_or(PARAMETER)?;
                let significand = magnitude.checked_shr(shift).ok_or(PARAMETER)?;
                let remainder = magnitude & (unit - 1);
                let half = unit >> 1;
                (significand, remainder, remainder > half || (remainder == half && significand & 1 != 0))
            } else {
                // width + shift is exactly 53, so this shift cannot discard bits.
                (magnitude.checked_shl(53 - width).ok_or(PARAMETER)?, 0, false)
            };
            let lower = normal_value(significand, leading)?;
            let upper = if remainder == 0 { lower } else { lower.next_up() };
            // Selecting the adjacent value also handles carry into the next binade.
            // No intermediate conversion to f64 or floating scale loses the tail.
            (lower, upper, if round_up { upper } else { lower })
        };
        let param =
            if self.coefficient < 0 { Param { lo: -upper, hi: -lower, nearest: -rounded } } else { Param { lo: lower, hi: upper, nearest: rounded } };
        if !param.lo.is_finite() || !param.hi.is_finite() || !param.nearest.is_finite() || param.lo > param.nearest || param.nearest > param.hi {
            return Err(PARAMETER);
        }
        Ok(param)
    }
}

// Public operations charge fixed bounded work up front, including failure. These
// helpers have no allocation or iteration and do not charge that work a second time.
fn normalize(coefficient: i128, exponent: i32) -> Result<Dyadic> {
    if coefficient == i128::MIN || !(MIN_EXPONENT..=MAX_EXPONENT).contains(&exponent) {
        return Err(ENVELOPE);
    }
    if coefficient == 0 {
        return Ok(Dyadic::ZERO);
    }
    let shift = coefficient.trailing_zeros();
    let exponent = exponent.checked_add(i32::try_from(shift).map_err(|_| ENVELOPE)?).ok_or(ENVELOPE)?;
    if !(MIN_EXPONENT..=MAX_EXPONENT).contains(&exponent) {
        return Err(ENVELOPE);
    }
    Ok(Dyadic { coefficient: coefficient.checked_shr(shift).ok_or(ENVELOPE)?, exponent })
}

fn align(a: Dyadic, b: Dyadic) -> Result<(i128, i128, i32)> {
    let exponent = a.exponent.min(b.exponent);
    Ok((aligned_coefficient(a, exponent)?, aligned_coefficient(b, exponent)?, exponent))
}

fn aligned_coefficient(value: Dyadic, exponent: i32) -> Result<i128> {
    let shift = u32::try_from(value.exponent.checked_sub(exponent).ok_or(ENVELOPE)?).map_err(|_| ENVELOPE)?;
    let limit = i128::MAX.checked_shr(shift).ok_or(ENVELOPE)?;
    if value.coefficient.checked_abs().ok_or(ENVELOPE)? > limit {
        return Err(ENVELOPE);
    }
    // checked_shl checks only the count. The magnitude comparison above also
    // proves that neither signed overflow nor discarded value bits can occur.
    value.coefficient.checked_shl(shift).ok_or(ENVELOPE)
}

fn normal_value(significand: u128, leading: i32) -> Result<f64> {
    if !(-1022..=MAX_EXPONENT).contains(&leading) || !((1u128 << 52)..(1u128 << 53)).contains(&significand) {
        return Err(PARAMETER);
    }
    let exponent = u64::try_from(leading.checked_add(1023).ok_or(PARAMETER)?).map_err(|_| PARAMETER)?;
    let fraction = u64::try_from(significand - (1u128 << 52)).map_err(|_| PARAMETER)?;
    // The checked exponent uses at most 11 bits; fraction uses at most 52.
    Ok(f64::from_bits((exponent << 52) | fraction))
}
