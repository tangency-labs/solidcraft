//! The constants the functions reduce their arguments with, each held between two adjacent
//! numbers. `tests::constants_hold_their_values` computes each one exactly enough, from series
//! with bounded tails in whole-number arithmetic, to check both bounds.

use std::f64::consts::{LN_2 as LN_2_NEAREST, LN_10 as LN_10_NEAREST, PI as PI_NEAREST};

use super::Interval;

/// π.
pub(super) const PI: Interval = Interval { lo: PI_NEAREST, hi: PI_NEAREST.next_up() };
/// π/2, split for reducing an argument: [`HALF_PI_HEAD`] has 30 significant bits, so `k` times it
/// is exact for `|k| < 2²³`, and [`HALF_PI_TAIL`] holds the rest.
pub(super) const HALF_PI_HEAD: f64 = 1.570796325802803;
pub(super) const HALF_PI_TAIL: Interval = Interval { lo: 9.920935796805404e-10, hi: 9.920935796805406e-10 };
/// ln 2, split the same way; `k` times its head is exact for `|k| < 2²³`.
pub(super) const LN_2_HEAD: f64 = 0.6931471796706319;
pub(super) const LN_2_TAIL: Interval = Interval { lo: 8.893134238886676e-10, hi: 8.893134238886677e-10 };
/// ln 2 and ln 10. The standard library's constants are the nearest numbers, below π and ln 2
/// and above ln 10; the test checks which side each is on.
pub(super) const LN_2: Interval = Interval { lo: LN_2_NEAREST, hi: LN_2_NEAREST.next_up() };
pub(super) const LN_10: Interval = Interval { lo: LN_10_NEAREST.next_down(), hi: LN_10_NEAREST };

/// π/2.
pub(super) fn half_pi() -> Interval {
    Interval::point(HALF_PI_HEAD) + HALF_PI_TAIL
}

/// `x − k·c` for a constant split into `head` (30 bits) and `tail`, `k` whole: for `|k| < 2²³`,
/// `k·head` is exact and so is its difference from `x` where they are close, leaving only the
/// rounding of `k·tail`. Every step is an interval operation, so it holds for any `k`.
pub(super) fn reduce(x: f64, k: f64, head: f64, tail: Interval) -> Interval {
    let k = Interval::point(k);
    Interval::point(x) - k * Interval::point(head) - k * tail
}
