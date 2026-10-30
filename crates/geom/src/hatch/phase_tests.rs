//! Original integer and binary64 oracles for exact hatch phase arithmetic.

use super::Invalid;
use super::budget::{Budget, Limits};
use super::phase::Dyadic;

fn dyadic(coefficient: i128, exponent: i32) -> Dyadic {
    Dyadic::from_parts(coefficient, exponent, &mut Budget::default()).unwrap()
}

fn parameter_bits(value: Dyadic) -> (u64, u64, u64) {
    let parameter = value.param(&mut Budget::default()).unwrap();
    assert!(parameter.lo().is_finite() && parameter.nearest().is_finite() && parameter.hi().is_finite());
    assert!(parameter.lo() <= parameter.nearest() && parameter.nearest() <= parameter.hi());
    (parameter.nearest().to_bits(), parameter.lo().to_bits(), parameter.hi().to_bits())
}

#[test]
fn normalization_preserves_value_and_canonicalizes_signed_zero() {
    assert_eq!(dyadic(12, -4), dyadic(3, -2));
    assert_eq!(dyadic(-12, -4), dyadic(-3, -2));
    for exponent in [-1074, 0, 1023] {
        assert_eq!(dyadic(0, exponent), Dyadic::ZERO);
    }
    for value in [0.0, -0.0] {
        let zero = Dyadic::from_f64(value, &mut Budget::default()).unwrap();
        assert_eq!(zero, Dyadic::ZERO);
        assert_eq!(parameter_bits(zero), (0, 0, 0));
    }
    assert!(!Dyadic::ZERO.is_positive());
    assert!(!dyadic(-1, -1074).is_positive());
    assert!(dyadic(1, -1074).is_positive());
}

#[test]
fn coefficient_and_exponent_domains_are_checked_before_normalization() {
    for (coefficient, exponent) in
        [(i128::MIN, 0), (i128::MIN, -1074), (0, -1075), (2, -1075), (0, 1024), (0, i32::MIN), (0, i32::MAX), (2, 1023), (-2, 1023)]
    {
        assert!(Dyadic::from_parts(coefficient, exponent, &mut Budget::default()).is_err());
    }
    assert_eq!(dyadic(2, -1074), dyadic(1, -1073));
    assert!(dyadic(i128::MAX, -1074).is_positive());
    assert!(!dyadic(-i128::MAX, -1074).is_positive());
    assert!(dyadic(1, 1023).is_positive());
}

#[test]
fn finite_binary64_inputs_decode_to_their_exact_stored_values() {
    // Decimal expectations are the stored binary rationals, not 1/10 or 1/5.
    for (value, coefficient, exponent) in [
        (0.1, 3_602_879_701_896_397, -55),
        (0.2, 3_602_879_701_896_397, -54),
        (0.30000000000000004, 1_351_079_888_211_149, -52),
        (f64::from_bits(1), 1, -1074),
        (f64::MIN_POSITIVE, 1, -1022),
        (f64::MAX, (1_i128 << 53) - 1, 971),
    ] {
        assert_eq!(Dyadic::from_f64(value, &mut Budget::default()).unwrap(), dyadic(coefficient, exponent));
        assert_eq!(Dyadic::from_f64(-value, &mut Budget::default()).unwrap(), dyadic(-coefficient, exponent));
    }
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(Dyadic::from_f64(value, &mut Budget::default()).is_err());
    }
}

#[test]
fn decimal_addition_and_subtraction_keep_the_unrounded_period() {
    let a = Dyadic::from_f64(0.1, &mut Budget::default()).unwrap();
    let b = Dyadic::from_f64(0.2, &mut Budget::default()).unwrap();
    let mut budget = Budget::default();
    let sum = a.add(b, &mut budget).unwrap();
    assert_eq!(sum, dyadic(10_808_639_105_689_191, -55));
    assert_eq!(sum.sub(a, &mut budget).unwrap(), b);
    assert_eq!(sum.sub(b, &mut budget).unwrap(), a);
    assert_eq!(sum.sub(sum, &mut budget).unwrap(), Dyadic::ZERO);
    let rounded = Dyadic::from_f64(0.30000000000000004, &mut budget).unwrap();
    assert_eq!(rounded.sub(sum, &mut budget).unwrap(), dyadic(1, -55));
}

#[test]
fn large_member_and_cycle_products_cancel_as_integers() {
    let mut budget = Budget::default();
    let phase = dyadic(99_999, 0).mul_index(1_000_000_000_001, &mut budget).unwrap();
    assert_eq!(phase, dyadic(99_999_000_000_099_999, 0));
    let cycle = dyadic(100, 0).mul_index(-999_990_000_001_000, &mut budget).unwrap();
    let start = phase.add(cycle, &mut budget).unwrap();
    assert_eq!(start, dyadic(-1, 0));
    assert_eq!(start.add(dyadic(1, 0), &mut budget).unwrap(), Dyadic::ZERO);
}

#[test]
fn integer_multiplication_keeps_full_signed_index_precision() {
    let mut budget = Budget::default();
    assert_eq!(dyadic(3, -3).mul_index(-4, &mut budget).unwrap(), dyadic(-3, -1));
    assert_eq!(dyadic(1, -63).mul_index(i64::MIN, &mut budget).unwrap(), dyadic(-1, 0));
    assert_eq!(dyadic(1, 0).mul_index(i64::MAX, &mut budget).unwrap(), dyadic(i128::from(i64::MAX), 0));
    assert_eq!(Dyadic::ZERO.mul_index(i64::MIN, &mut budget).unwrap(), Dyadic::ZERO);
    assert_eq!(dyadic(i128::MAX, 1023).mul_index(0, &mut budget).unwrap(), Dyadic::ZERO);
    assert!(dyadic(1, 1023).mul_index(2, &mut budget).is_err());
}

#[test]
fn signed_floor_returns_the_mathematical_quotient_and_remainder() {
    for (n, e, p, pe, quotient, remainder, re) in
        [(-9, -3, 3, -3, -3, 0, 0), (-5, -2, 3, -3, -4, 1, -2), (-1, 0, 3, 0, -1, 2, 0), (5, -2, 3, -3, 3, 1, -3), (0, 0, 3, -3, 0, 0, 0)]
    {
        let mut budget = Budget::default();
        let numerator = dyadic(n, e);
        let denominator = dyadic(p, pe);
        assert_eq!(numerator.floor_ratio(denominator, &mut budget).unwrap(), i128::from(quotient));
        let multiple = denominator.mul_index(quotient, &mut budget).unwrap();
        assert_eq!(numerator.sub(multiple, &mut budget).unwrap(), dyadic(remainder, re));
    }
}

#[test]
fn signed_floor_distinguishes_values_that_all_round_to_minus_three() {
    let scale = 1_i128 << 54;
    for (coefficient, quotient, remainder) in [(-(3 * scale + 1), -4, scale - 1), (-3 * scale, -3, 0), (-(3 * scale - 1), -3, 1)] {
        let mut budget = Budget::default();
        let numerator = dyadic(coefficient, -54);
        assert_eq!(parameter_bits(numerator).0, 0xc008_0000_0000_0000);
        assert_eq!(numerator.floor_ratio(dyadic(1, 0), &mut budget).unwrap(), i128::from(quotient));
        assert_eq!(numerator.sub(dyadic(i128::from(quotient), 0), &mut budget).unwrap(), dyadic(remainder, -54));
    }
}

#[test]
fn exact_floor_is_separate_from_generator_cycle_index_admission() {
    let bound = 1_i128 << 52;
    let mut budget = Budget::default();
    assert_eq!(dyadic(-(3 * bound - 2), 0).floor_ratio(dyadic(3, 0), &mut budget).unwrap(), -bound);
    assert_eq!(dyadic(-(3 * bound + 1), 0).floor_ratio(dyadic(3, 0), &mut budget).unwrap(), -bound - 1);
}

#[test]
fn floor_rejects_nonpositive_divisors_even_for_a_zero_numerator() {
    for numerator in [Dyadic::ZERO, dyadic(-1, 0), dyadic(1, 0)] {
        for divisor in [Dyadic::ZERO, dyadic(-1, 0), dyadic(-1, -1074)] {
            assert!(numerator.floor_ratio(divisor, &mut Budget::default()).is_err());
        }
    }
    assert_eq!(Dyadic::ZERO.floor_ratio(dyadic(1, -1074), &mut Budget::default()).unwrap(), 0);
}

#[test]
fn nearest_even_ties_and_binade_carries_have_tight_signed_enclosures() {
    let two53 = 1_i128 << 53;
    let two54 = 1_i128 << 54;
    // Expected columns: nearest, lower, upper. The negative rows reverse
    // neighbour order; carrying to 2 crosses a binary exponent boundary.
    for (n, expected) in [
        (two53 + 1, (0x3ff0_0000_0000_0000, 0x3ff0_0000_0000_0000, 0x3ff0_0000_0000_0001)),
        (two53 + 3, (0x3ff0_0000_0000_0002, 0x3ff0_0000_0000_0001, 0x3ff0_0000_0000_0002)),
        (-(two53 + 1), (0xbff0_0000_0000_0000, 0xbff0_0000_0000_0001, 0xbff0_0000_0000_0000)),
        (-(two53 + 3), (0xbff0_0000_0000_0002, 0xbff0_0000_0000_0002, 0xbff0_0000_0000_0001)),
        (two54 - 1, (0x4000_0000_0000_0000, 0x3fff_ffff_ffff_ffff, 0x4000_0000_0000_0000)),
        (-(two54 - 1), (0xc000_0000_0000_0000, 0xc000_0000_0000_0000, 0xbfff_ffff_ffff_ffff)),
    ] {
        assert_eq!(parameter_bits(dyadic(n, -53)), expected, "coefficient {n}");
    }
}

#[test]
fn values_on_both_sides_of_a_tie_choose_the_nearest_neighbour() {
    let two54 = 1_i128 << 54;
    for (n, expected) in [
        (two54 + 1, (0x3ff0_0000_0000_0000, 0x3ff0_0000_0000_0000, 0x3ff0_0000_0000_0001)),
        (two54 + 3, (0x3ff0_0000_0000_0001, 0x3ff0_0000_0000_0000, 0x3ff0_0000_0000_0001)),
        (-(two54 + 1), (0xbff0_0000_0000_0000, 0xbff0_0000_0000_0001, 0xbff0_0000_0000_0000)),
        (-(two54 + 3), (0xbff0_0000_0000_0001, 0xbff0_0000_0000_0001, 0xbff0_0000_0000_0000)),
    ] {
        assert_eq!(parameter_bits(dyadic(n, -54)), expected, "coefficient {n}");
    }
}

#[test]
fn subnormal_and_normal_boundary_values_are_exact_points() {
    for (n, e, positive, negative) in [
        (1, -1074, 0x0000_0000_0000_0001, 0x8000_0000_0000_0001),
        ((1_i128 << 52) - 1, -1074, 0x000f_ffff_ffff_ffff, 0x800f_ffff_ffff_ffff),
        (1, -1022, 0x0010_0000_0000_0000, 0x8010_0000_0000_0000),
        ((1_i128 << 52) + 1, -1074, 0x0010_0000_0000_0001, 0x8010_0000_0000_0001),
        ((1_i128 << 53) - 1, 971, 0x7fef_ffff_ffff_ffff, 0xffef_ffff_ffff_ffff),
    ] {
        assert_eq!(parameter_bits(dyadic(n, e)), (positive, positive, positive));
        assert_eq!(parameter_bits(dyadic(-n, e)), (negative, negative, negative));
    }
}

#[test]
fn finite_output_requires_both_bounds_even_when_nearest_would_be_finite() {
    // MAX=(2^55-4)*2^969. The first value is one quarter of an ulp above
    // MAX and rounds to MAX, but its required upper neighbour is infinity.
    // The second is the overflow tie; the third exceeds the finite binade.
    for (n, e) in [((1_i128 << 55) - 3, 969), ((1_i128 << 54) - 1, 970), (3, 1023)] {
        assert!(dyadic(n, e).param(&mut Budget::default()).is_err());
        assert!(dyadic(-n, e).param(&mut Budget::default()).is_err());
    }
}

#[test]
fn alignment_capacity_accepts_126_bits_and_rejects_127_bits() {
    let mut budget = Budget::default();
    let one = dyadic(1, 0);
    let small = dyadic(1, -126);
    let sum = one.add(small, &mut budget).unwrap();
    assert_eq!(sum, dyadic((1_i128 << 126) + 1, -126));
    assert_eq!(sum.sub(one, &mut budget).unwrap(), small);
    assert!(one.add(dyadic(1, -127), &mut budget).is_err());
    assert!(one.add(dyadic(1, -1074), &mut budget).is_err());
    // The arithmetic envelope is bounded even if the final floor is small.
    assert!(dyadic(1, -1074).floor_ratio(one, &mut budget).is_err());
}

#[test]
fn fitting_shift_counts_do_not_allow_value_bits_to_be_discarded() {
    let mut budget = Budget::default();
    // Shifting by 126 is a legal i128 shift count, but 3*2^126 exceeds MAX.
    assert!(dyadic(3, 0).add(dyadic(1, -126), &mut budget).is_err());
    assert!(dyadic(-3, 0).sub(dyadic(1, -126), &mut budget).is_err());
    assert!(dyadic(3, 0).floor_ratio(dyadic(1, -126), &mut budget).is_err());
}

#[test]
fn overflowing_sums_and_raw_minimum_are_not_rescued_by_normalization() {
    let mut budget = Budget::default();
    assert!(dyadic(i128::MAX, -126).add(dyadic(1, -125), &mut budget).is_err());
    assert!(dyadic(-i128::MAX, -126).sub(dyadic(1, -125), &mut budget).is_err());
    assert!(dyadic(-i128::MAX, 0).sub(dyadic(1, 0), &mut budget).is_err());
    assert!(dyadic(i128::MAX, 0).add(dyadic(1, 0), &mut budget).is_err());
}

#[test]
fn multiplication_refuses_overflow_but_accepts_a_fitting_product() {
    let mut budget = Budget::default();
    assert!(dyadic((1_i128 << 126) + 1, -126).mul_index(3, &mut budget).is_err());
    assert!(dyadic(-((1_i128 << 126) + 1), -126).mul_index(3, &mut budget).is_err());
    let coefficient = (1_i128 << 125) - 1;
    assert_eq!(dyadic(coefficient, -125).mul_index(3, &mut budget).unwrap(), dyadic(3 * coefficient, -125));
}

fn assert_work_cost(cost: usize, operation: impl Fn(&mut Budget) -> Result<(), Invalid>) {
    let mut short = Budget::new(Limits { work: cost - 1, ..Limits::default() });
    assert_eq!(operation(&mut short), Err(Invalid("geometry work budget exhausted")));
    let mut exact = Budget::new(Limits { work: cost, ..Limits::default() });
    assert_eq!(operation(&mut exact), Ok(()));
    assert_eq!(exact.work(1), Err(Invalid("geometry work budget exhausted")));
}

#[test]
fn every_numeric_operation_obeys_its_fixed_work_charge() {
    let one = dyadic(1, 0);
    assert_work_cost(2, |budget| Dyadic::from_parts(1, 0, budget).map(|_| ()));
    assert_work_cost(2, |budget| Dyadic::from_f64(0.1, budget).map(|_| ()));
    assert_work_cost(4, |budget| one.add(one, budget).map(|_| ()));
    assert_work_cost(4, |budget| one.sub(one, budget).map(|_| ()));
    assert_work_cost(4, |budget| one.mul_index(0, budget).map(|_| ()));
    assert_work_cost(4, |budget| Dyadic::ZERO.floor_ratio(one, budget).map(|_| ()));
    assert_work_cost(4, |budget| Dyadic::ZERO.param(budget).map(|_| ()));
}

#[test]
fn failed_numeric_operations_still_consume_the_shared_work_budget() {
    let mut budget = Budget::new(Limits { work: 16, ..Limits::default() });
    let envelope = Invalid("hatch phase exceeds exact arithmetic envelope");
    assert_eq!(Dyadic::from_parts(i128::MIN, 0, &mut budget), Err(envelope.clone()));
    assert_eq!(Dyadic::from_f64(f64::INFINITY, &mut budget), Err(Invalid("nonfinite hatch phase input")));
    assert_eq!(dyadic(1, 0).add(dyadic(1, -127), &mut budget), Err(envelope));
    assert_eq!(Dyadic::ZERO.floor_ratio(Dyadic::ZERO, &mut budget), Err(Invalid("hatch phase divisor must be positive")));
    assert_eq!(dyadic(3, 1023).param(&mut budget).map(|_| ()), Err(Invalid("hatch phase cannot form a finite parameter enclosure")));
    assert_eq!(budget.work(1), Err(Invalid("geometry work budget exhausted")));
}
