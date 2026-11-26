//! Not upstream (#2784): the volumes that check a body built apart. Blends whose bands meet
//! across a face, on edges in groups that share no vertex, each taking material off, leave the
//! intersection of the bodies each group leaves on its own (`solidcore-ops`' `build_apart`).
//! That body's volume is bounded by those of its parts, which tells a sound intersection from
//! a boolean that lost or kept a piece.

/// Whether volumes fit a body built apart, to a thousandth of the original's: no group adds
/// material (each body `alone` at most the `original`), and the `result` is at most each
/// body's and at least what is left once each group takes off its own,
/// `original - sum(original - alone)`.
#[must_use]
pub fn apart_volumes_fit(original: f64, alone: &[f64], result: f64) -> bool {
    let slack = 1e-3 * original;
    let adds = alone.iter().any(|&v| v > original + slack);
    let most = alone.iter().copied().fold(f64::INFINITY, f64::min);
    let least = original - alone.iter().map(|&v| original - v).sum::<f64>();
    !adds && result <= most + slack && result >= least - slack
}

#[cfg(test)]
mod tests {
    use super::apart_volumes_fit;

    /// The disc of #2784 (25 pi, 78.5398): each rim chamfered 1 alone leaves 63.8791 (the
    /// rim's triangle, 1/2, swept at 5 - 1/3), both together 56.8105 (the closed form): inside
    /// the bounds; a result that kept a piece (the disc whole) or lost one is not, nor a group
    /// that added material.
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
    }
}
