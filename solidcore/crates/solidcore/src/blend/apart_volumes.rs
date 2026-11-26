//! The volumes that check a body built apart (not upstream, #2784). Blends whose bands meet
//! across a face, on edges in groups that share no vertex, each taking material off, leave the
//! intersection of the bodies each group leaves on its own (`solidcore-ops`' `build_apart`).
//! That body's volume is bounded by those of its parts, which tells a sound intersection from
//! a boolean that lost or kept a piece.

/// Whether volumes fit a body built apart: every group takes material off (each body `alone`
/// below the `original` by more than the slack), and the `result` is at most each body's and at
/// least what is left once each group takes off its own, `original - sum(original - alone)`.
/// The slack is a thousandth of the least volume a group takes off, and never below a
/// millionth of the original (the volumes' own accuracy is a billionth): a small blend on a
/// large body is judged by what it takes off, not by the body's size.
#[must_use]
pub fn apart_volumes_fit(original: f64, alone: &[f64], result: f64) -> bool {
    let least_taken = alone
        .iter()
        .map(|&v| original - v)
        .fold(f64::INFINITY, f64::min);
    let slack = (1e-3 * least_taken).max(1e-6 * original);
    if alone.is_empty() || least_taken <= slack {
        return false;
    }
    let most = alone.iter().copied().fold(f64::INFINITY, f64::min);
    let least = original - alone.iter().map(|&v| original - v).sum::<f64>();
    result <= most + slack && result >= least - slack
}

#[cfg(test)]
mod tests {
    use super::apart_volumes_fit;

    /// The disc of #2784 (25 pi, 78.5398): each rim chamfered 1 alone leaves 63.8791 (the
    /// rim's triangle, 1/2, swept at 5 - 1/3), both together 56.8105 (the closed form): inside
    /// the bounds; a result that kept a piece (the disc whole) or lost one is not, nor a group
    /// that took nothing off or added material.
    #[test]
    fn a_disc_s_rims_fit_and_a_wrong_intersection_does_not() {
        let pi = core::f64::consts::PI;
        let original = 25.0 * pi;
        let one = original - pi * (5.0 - 1.0 / 3.0);
        let both = pi * 16.0 + 2.0 * pi * (1.0 + 1.0 / 24.0);
        assert!(apart_volumes_fit(original, &[one, one], both));
        assert!(!apart_volumes_fit(original, &[one, one], original));
        assert!(!apart_volumes_fit(original, &[one, one], 1.0));
        assert!(!apart_volumes_fit(original, &[original + 1.0, one], both));
        assert!(!apart_volumes_fit(original, &[original, one], one));
        assert!(!apart_volumes_fit(original, &[], original));
    }

    /// Two small rims on a large plate (100 000): each takes 1 off, together 2. A result that
    /// lost one group's cut (99 999) or kept the plate whole is refused, though both are within
    /// a thousandth of the plate.
    #[test]
    fn small_blends_on_a_large_body_are_judged_by_what_they_take_off() {
        let original = 100_000.0;
        let alone = [original - 1.0, original - 1.0];
        assert!(apart_volumes_fit(original, &alone, original - 2.0));
        assert!(!apart_volumes_fit(original, &alone, original - 1.0 + 0.01));
        assert!(!apart_volumes_fit(original, &alone, original));
    }
}
