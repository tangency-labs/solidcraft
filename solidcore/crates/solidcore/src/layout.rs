//! Places for labels along a line: the least-squares spacing of ordered places to a pitch, as a
//! drawing spaces the numbers of an ordinate dimension set so that none overlap.
//!
//! Not upstream (SolidCraft's dimension layout, moved into the kernel so the layers above compute
//! no geometry, docs/layers.md §1). Not a certified query, so it sits outside `prism`, whose
//! queries each carry a proved bound: the order of the places is kept exactly, and consecutive
//! places are `pitch` apart or more, less at most `GAP_ROUNDING` of their size; places too large
//! for that to leave the pitch are refused.
//!
//! References: Best and Chakravarti, "Active set algorithms for isotonic regression; a unifying
//! framework", *Mathematical Programming* 47 (1990), 425–439 (pool adjacent violators, and its
//! invariance under a fixed shift of each target).
//!
//! Deviations: none (no upstream counterpart). No recursion.

use crate::error::{Result, SolidError};

const OP: &str = "layout::spread_along";

/// The most places one call spaces: the profile's segment budget.
pub const MAX_ITEMS: usize = crate::prism::profile::MAX_SEGMENTS;

/// The most rounding may take from a gap between two places, as a share of their size (the larger
/// of `|mean| + pitch·k` for the two). A place is its block's mean plus `pitch·k`, a product and a
/// sum, so it is within one `EPSILON` of its size from its exact value (two roundings of half an
/// ulp); a gap, read as their difference, is within three (two places and the subtraction).
/// `4·EPSILON = 2^-50` is the power of two above that, so the size at which a pitch stops fitting
/// is exact: 2^50 pitches. One `EPSILON` would refuse correct results; eight would pass gaps short by
/// more than rounding can explain.
const GAP_ROUNDING: f64 = 4.0 * f64::EPSILON;

/// Places for items whose ideal places along a line are `ideal` (least first), at least `pitch`
/// apart and moved as little as possible in all (least squares under their order): pool adjacent
/// violators on `ideal[k] - k·pitch`. Items that would overlap pool into blocks, each centred on
/// its members' ideal places; every other item stays where it is.
///
/// # Errors
///
/// `InvalidInput` for a pitch not finite or below zero, more than [`MAX_ITEMS`] places, a place
/// not finite, `ideal` not in order, a result not finite, or places 2^50 pitches in size or more,
/// where rounding could take a whole pitch from a gap.
pub fn spread_along(ideal: &[f64], pitch: f64) -> Result<Vec<f64>> {
    let refuse = |reason| SolidError::InvalidInput {
        operation: OP,
        reason,
    };
    if !(pitch.is_finite() && pitch >= 0.0) {
        return Err(refuse("a pitch not finite or below zero"));
    }
    if ideal.len() > MAX_ITEMS {
        return Err(refuse("more than 8192 places: at most 8192 fit"));
    }
    if !ideal.iter().all(|x| x.is_finite()) {
        return Err(refuse("a place not finite"));
    }
    if ideal.windows(2).any(|w| matches!(w, [a, b] if b < a)) {
        return Err(refuse("places not in order"));
    }
    // Each block: the sum of its members' targets (places less their spacing), and their count.
    let mut blocks: Vec<(f64, usize)> = Vec::new();
    for (i, x) in ideal.iter().enumerate() {
        blocks.push((x - pitch * i as f64, 1));
        // Pool the last two blocks while the earlier one's mean is above the later one's.
        while let [.., (a, m), (b, k)] = *blocks.as_slice() {
            if a / m as f64 <= b / k as f64 {
                break;
            }
            blocks.truncate(blocks.len().saturating_sub(2));
            blocks.push((a + b, m + k));
        }
    }
    let means: Vec<f64> = blocks
        .into_iter()
        .flat_map(|(sum, k)| std::iter::repeat_n(sum / k as f64, k))
        .collect();
    let places: Vec<f64> = means
        .iter()
        .enumerate()
        .map(|(i, mean)| mean + pitch * i as f64)
        .collect();
    if !places.iter().all(|x| x.is_finite()) {
        return Err(refuse("a place not finite"));
    }
    // Rounding takes at most `GAP_ROUNDING` of the places' size from a gap. Where that could be
    // the whole pitch, a gap may vanish (at `f64::MAX`, adding 1 changes nothing), so it is refused.
    let size = means
        .iter()
        .enumerate()
        .map(|(i, mean)| mean.abs() + pitch * i as f64)
        .fold(0.0, f64::max);
    if places.len() > 1 && pitch > 0.0 && GAP_ROUNDING * size >= pitch {
        return Err(refuse(
            "places too large to keep a pitch apart: a pitch fits where the places are under 2^50 times it",
        ));
    }
    Ok(places)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::indexing_slicing, clippy::unwrap_used, clippy::panic)]

    use super::{spread_along, GAP_ROUNDING, MAX_ITEMS};

    fn close(a: &[f64], b: &[f64]) -> bool {
        a.len() == b.len()
            && a.iter()
                .zip(b)
                .all(|(x, y)| (x - y).abs() <= 1e-12 * (1.0 + x.abs()))
    }

    /// Places already `pitch` apart or more stay where they are; a pitch of 0 moves nothing.
    #[test]
    fn places_apart_stay() {
        assert_eq!(
            spread_along(&[0.0, 10.0, 20.0], 5.0).unwrap(),
            [0.0, 10.0, 20.0]
        );
        assert_eq!(
            spread_along(&[1.0, 1.0, 2.0], 0.0).unwrap(),
            [1.0, 1.0, 2.0]
        );
        assert_eq!(spread_along(&[], 3.0).unwrap(), Vec::<f64>::new());
        assert_eq!(spread_along(&[7.5], 3.0).unwrap(), [7.5]);
    }

    /// Overlapping places pool into a block centred on them, one pitch apart; the others stay.
    #[test]
    fn overlapping_places_pool_round_their_middle() {
        assert!(close(&spread_along(&[3.0, 3.0], 2.0).unwrap(), &[2.0, 4.0]));
        assert!(close(
            &spread_along(&[0.0, 0.0, 0.0], 1.0).unwrap(),
            &[-1.0, 0.0, 1.0]
        ));
        // The first two pool (targets 0 and -1, mean -0.5); the third, at 10, is clear.
        assert!(close(
            &spread_along(&[0.0, 1.0, 10.0], 2.0).unwrap(),
            &[-0.5, 1.5, 10.0]
        ));
        // A block that meets the next pools again: targets 0, -1, -1.5 pool to -5/6.
        assert!(close(
            &spread_along(&[0.0, 0.0, 0.5, 9.0], 1.0).unwrap(),
            &[-5.0 / 6.0, 1.0 / 6.0, 7.0 / 6.0, 9.0]
        ));
    }

    /// What can't be spaced is refused: a pitch not finite or below zero, a place not finite,
    /// places out of order, more than `MAX_ITEMS`, places whose sums overflow, and places too large
    /// to keep a pitch apart.
    #[test]
    fn what_cannot_be_spaced_is_refused() {
        assert!(spread_along(&[0.0, 1.0], -1.0).is_err());
        assert!(spread_along(&[0.0, 1.0], f64::NAN).is_err());
        assert!(spread_along(&[0.0, 1.0], f64::INFINITY).is_err());
        assert!(spread_along(&[0.0, f64::NAN], 1.0).is_err());
        assert!(spread_along(&[f64::NEG_INFINITY, 0.0], 1.0).is_err());
        assert!(spread_along(&[1.0, 0.0], 1.0).is_err());
        assert!(spread_along(&vec![0.0; MAX_ITEMS + 1], 1.0)
            .is_err_and(|e| e.to_string().contains(&format!("at most {MAX_ITEMS}"))));
        assert!(spread_along(&vec![0.0; MAX_ITEMS], 1.0).is_ok());
        // Finite places whose pooled sum overflows: targets MAX and MAX - 1e300 pool.
        assert!(spread_along(&[f64::MAX, f64::MAX], 1e300)
            .is_err_and(|e| e.to_string().contains("not finite")));
        // Places too large for their pitch: rounding there could take the whole pitch (at MAX,
        // MAX + 1 is MAX), so the gap is refused, naming the size at which a pitch fits.
        assert_eq!(GAP_ROUNDING, 2f64.powi(-50));
        let too_large = |r: crate::error::Result<Vec<f64>>| {
            r.is_err_and(|e| e.to_string().contains("under 2^50 times it"))
        };
        assert!(too_large(spread_along(&[f64::MAX, f64::MAX], 1.0)));
        assert!(too_large(spread_along(&[1e17, 1e17], 1.0)));
        // Under 2^50 pitches the pitch is kept.
        let kept = spread_along(&[1e15, 1e15], 1.0).unwrap();
        assert!(kept[1] - kept[0] >= 1.0);
    }

    /// For random ordered places, the result is the least-squares fit under the order: in the
    /// shifted targets `y_k = x_k - k·pitch`, the fit is constant on blocks, rising from block to
    /// block, each block's value the mean of its targets, and within a block every prefix of
    /// `y - value` sums to zero or more (the optimality conditions of isotonic regression). Its
    /// places keep their order and are `pitch` apart or more.
    #[test]
    fn random_places_get_the_least_squares_fit() {
        let mut seed = 0x0001_3052_u64;
        let mut next = move || {
            seed = seed
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            (seed >> 11) as f64 / (1u64 << 53) as f64
        };
        for _ in 0..500 {
            let n = 1 + (next() * 40.0) as usize;
            let pitch = next() * 5.0;
            let mut ideal: Vec<f64> = (0..n).map(|_| next() * 60.0 - 30.0).collect();
            ideal.sort_by(f64::total_cmp);
            let got = spread_along(&ideal, pitch).unwrap();
            let tol = 1e-9 * (1.0 + 30.0 + pitch * n as f64);
            for k in 1..n {
                assert!(
                    got[k] - got[k - 1] >= pitch - tol,
                    "{ideal:?} {pitch} {got:?}"
                );
            }
            let y: Vec<f64> = (0..n).map(|k| ideal[k] - pitch * k as f64).collect();
            let z: Vec<f64> = (0..n).map(|k| got[k] - pitch * k as f64).collect();
            let mut start = 0;
            while start < n {
                let mut end = start + 1;
                while end < n && (z[end] - z[start]).abs() <= tol {
                    end += 1;
                }
                let value = z[start];
                let mean = y[start..end].iter().sum::<f64>() / (end - start) as f64;
                assert!((value - mean).abs() <= tol, "{ideal:?} {pitch} {got:?}");
                let mut prefix = 0.0;
                for yk in &y[start..end] {
                    prefix += yk - value;
                    assert!(prefix >= -tol, "{ideal:?} {pitch} {got:?}");
                }
                if end < n {
                    assert!(z[end] > value, "{ideal:?} {pitch} {got:?}");
                }
                start = end;
            }
        }
    }
}
