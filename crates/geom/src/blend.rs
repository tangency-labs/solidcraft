//! Blend curves: one Bézier that joins two curves with a requested continuity order, by Hermite
//! interpolation of each reference curve's position and derivatives at the joints. This is the
//! geometry `sketch.blend_curve` builds; it knows nothing about sketches or the document.

use crate::{MAX_DERIVATIVE_ORDER, profile::binom};
use solidcore_record::Vec2;

/// Control points of the Bézier of degree `2k + 1`, `k = d0.len()`, that starts at `p0` with
/// derivatives `d0` and ends at `p1` with derivatives `d1`, all with respect to its own parameter
/// on [0, 1]. The derivative vectors are the reference curves' at the joints, measured in their
/// own parameters and oriented the way the blend runs through the joint, so the blend agrees with
/// each reference through its k-th derivative (G1, G2, G3, G4 for k = 1…4).
///
/// The two ends fix exactly half of the controls each: a Bézier's j-th forward difference is
/// `P⁽ʲ⁾(0)·(n − j)!/n!`, so the first `k + 1` controls follow from the start data and the last
/// `k + 1` from the end data, which meet exactly at the middle control. `None` when the ends
/// disagree on the order, the order is above [`MAX_DERIVATIVE_ORDER`], or a value is not finite.
pub fn hermite_bezier(p0: Vec2, p1: Vec2, d0: &[Vec2], d1: &[Vec2]) -> Option<Vec<Vec2>> {
    let k = d0.len();
    if k != d1.len() || k > MAX_DERIVATIVE_ORDER || !p0.is_finite() || !p1.is_finite() || d0.iter().chain(d1).any(|d| !d.is_finite()) {
        return None;
    }
    let n = 2 * k + 1;
    let mut c = vec![Vec2::ZERO; n + 1];
    c[0] = p0;
    for j in 1..=k {
        // `Δʲp₀ = P⁽ʲ⁾(0)·(n − j)!/n!`, and solving it for `pⱼ` puts `(−1)^(j − i + 1)` on every
        // control already known.
        let scale = 1.0 / (0..j).fold(1.0, |acc, i| acc * (n - i) as f64);
        let mut value = d0[j - 1] * scale;
        for (i, p) in c.iter().take(j).enumerate() {
            value += *p * (odd_sign(j - i) * binom(j, i));
        }
        c[j] = value;
    }
    c[n] = p1;
    for j in 1..=k {
        // The same at the far end, reading the controls backwards: `Σᵢ(−1)ⁱ·C(j,i)·pₙ₋ᵢ` is
        // `P⁽ʲ⁾(1)·(n − j)!/n!`, and solving that for `pₙ₋ⱼ` puts `(−1)^(j + i + 1)` on every control
        // already known.
        let scale = 1.0 / (0..j).fold(1.0, |acc, i| acc * (n - i) as f64);
        let mut value = d1[j - 1] * (scale * odd_sign(j + 1));
        for (i, p) in c[n - j + 1..].iter().rev().enumerate() {
            value += *p * (odd_sign(j + i) * binom(j, i));
        }
        c[n - j] = value;
    }
    c.iter().all(|p| p.is_finite()).then_some(c)
}

/// `(−1)^(m + 1)`: the sign an m-th difference expansion carries in a Bézier's endpoint identity.
fn odd_sign(m: usize) -> f64 {
    if m % 2 == 1 { 1.0 } else { -1.0 }
}
