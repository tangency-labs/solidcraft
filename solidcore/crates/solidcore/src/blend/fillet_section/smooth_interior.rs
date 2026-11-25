//! Compact correction kernels for option A of issue #2356.
//!
//! Upstream: none; the approved solidcore #2356 construction, Q = 2.
//!
//! | Rust | Design |
//! |------|--------|
//! | [`correction_kernels`] | `G(d, eps)`, `G2(d, eps)` and their sided d-jets |
//!
//! Deviations: factored polynomials avoid endpoint cancellation. These primitives
//! are not wired into a surface: the correction still needs a matching interval
//! certificate, station data and rail compatibility checks. No recursion.

use crate::{Result, SolidError};

/// Value and first two derivatives with respect to the station displacement d.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CorrectionJet {
    /// Function value.
    pub value: f64,
    /// First d derivative.
    pub first: f64,
    /// Second d derivative.
    pub second: f64,
}

/// Evaluates the compact kernels `(G, G2)` of the approved Q = 2 construction.
///
/// The unique endpoint Hermite polynomials are
/// `H(u) = 3/16 + u/2 + 3u²/8 - u⁴/16` and
/// `H2(u) = 3u/32 + u²/4 + 3u³/16 - u⁵/32`.
/// Inside the window their compact residuals factor as
/// `G = -eps (1 - |u|)³ (3 + |u|) / 16`, `G2 = d G / 2`.
/// Both join zero to order two at `|d| = eps`. At d = 0, `before`
/// chooses the left jet: G's first derivative and G2's second derivative
/// have unit jumps (right minus left). The other derivatives are continuous.
///
/// Width must be strictly positive. At a rail the caller must use the exact
/// boundary extension of the complete correction, not divide by zero here.
/// A positive width does not itself prove regularity or rail compatibility.
///
/// # Errors
///
/// Returns invalid input for nonfinite d, nonpositive/nonfinite width, or
/// derivatives that cannot be represented as finite doubles.
pub fn correction_kernels(
    d: f64,
    eps: f64,
    before: bool,
) -> Result<(CorrectionJet, CorrectionJet)> {
    const OP: &str = "blend::fillet_section::smooth_interior::correction_kernels";
    if !d.is_finite() || !eps.is_finite() || eps <= 0.0 {
        return Err(SolidError::invalid_input(
            OP,
            "finite displacement and positive finite width required",
        ));
    }
    if d.abs() >= eps {
        return Ok((CorrectionJet::default(), CorrectionJet::default()));
    }
    let v = d.abs() / eps;
    let sign = if d < 0.0 || (d == 0.0 && before) {
        -1.0
    } else {
        1.0
    };
    let q = 1.0 - v;
    let g = CorrectionJet {
        value: (-q * q * q * ((3.0 + v) / 16.0)) * eps,
        first: sign * q * q * (2.0 + v) / 4.0,
        second: (-0.75 * (1.0 - v * v)) / eps,
    };
    let g2 = CorrectionJet {
        value: (d / 2.0) * g.value,
        first: (g.value + d * g.first) / 2.0,
        second: g.first + (d / 2.0) * g.second,
    };
    if [g.value, g.first, g.second, g2.value, g2.first, g2.second]
        .iter()
        .any(|v| !v.is_finite())
    {
        return Err(SolidError::invalid_input(
            OP,
            "correction derivatives exceed finite double range",
        ));
    }
    Ok((g, g2))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sided_jumps_cancel_first_and_second_derivative_jumps() {
        for eps in [1e-5, 0.15, 2.0] {
            let (a, a2) = correction_kernels(0.0, eps, true).unwrap();
            let (b, b2) = correction_kernels(0.0, eps, false).unwrap();
            assert_eq!(b.value, a.value);
            assert_eq!(b.first - a.first, 1.0);
            assert_eq!(b.second, a.second);
            assert_eq!(b2.value, a2.value);
            assert_eq!(b2.first, a2.first);
            assert_eq!(b2.second - a2.second, 1.0);
            // G2 changes the shared first derivative; averaging alone omits it.
            assert_eq!(a2.first, -3.0 * eps / 32.0);
        }
    }

    #[test]
    fn endpoint_jets_join_zero_and_match_endpoint_hermite_polynomials() {
        let eps = 0.25;
        for i in -16..=16 {
            let u = f64::from(i) / 16.0;
            let d = eps * u;
            let (g, g2) = correction_kernels(d, eps, false).unwrap();
            let h = 3.0 / 16.0 + u / 2.0 + 3.0 * u * u / 8.0 - u.powi(4) / 16.0;
            let h2 = 3.0 * u / 32.0 + u * u / 4.0 + 3.0 * u.powi(3) / 16.0 - u.powi(5) / 32.0;
            assert!((g.value - (d.max(0.0) - eps * h)).abs() < 1e-16);
            assert!((g2.value - (d.max(0.0).powi(2) / 2.0 - eps * eps * h2)).abs() < 1e-16);
            let step = if u < 0.0 { 0.0 } else { 1.0 };
            assert!((g.first - (step - (0.5 + 0.75 * u - 0.25 * u.powi(3)))).abs() < 1e-15);
            assert!((g.second + (0.75 - 0.75 * u * u) / eps).abs() < 1e-15);
            assert!(
                (g2.first
                    - (d.max(0.0)
                        - eps
                            * (3.0 / 32.0 + u / 2.0 + 9.0 * u * u / 16.0
                                - 5.0 * u.powi(4) / 32.0)))
                    .abs()
                    < 1e-15
            );
            assert!(
                (g2.second - (step - (0.5 + 9.0 * u / 8.0 - 5.0 * u.powi(3) / 8.0))).abs() < 1e-15
            );
        }
        for d in [-1.0, -eps, eps, 1.0] {
            assert_eq!(
                correction_kernels(d, eps, false).unwrap(),
                (CorrectionJet::default(), CorrectionJet::default())
            );
        }
        for d in [-eps * (1.0 - 1e-6), eps * (1.0 - 1e-6)] {
            let (g, g2) = correction_kernels(d, eps, false).unwrap();
            assert!(g.value.abs() < 1e-17 && g.first.abs() < 1e-12 && g.second.abs() < 1e-5);
            assert!(g2.value.abs() < 1e-17 && g2.first.abs() < 1e-12 && g2.second.abs() < 1e-5);
        }
    }

    #[test]
    fn shrinking_window_has_the_designs_normal_and_tangential_rail_orders() {
        for t in [1e-3, 1e-4, 1e-5] {
            let lambda = 0.15;
            let eps = lambda * 4.0 * t * (1.0 - t);
            let (g, g2) = correction_kernels(eps / 3.0, eps, false).unwrap();
            // Synthetic jumps with the design's orders: Delta_N=t²,
            // Delta2_N=t, Delta_T=t, Delta2_T=1. This checks kernel scaling,
            // not that a particular face satisfies these compatibility orders.
            assert!((t * t * g.value + t * g2.value).abs() <= 0.2 * t.powi(3));
            assert!((t * g.value + g2.value).abs() <= 0.2 * t.powi(2));
            assert!((t * t * g.second).abs() <= 2.0 * t);
            assert!(g2.second.abs() <= 0.5);
        }
    }

    #[test]
    fn invalid_widths_and_unrepresentable_jets_are_errors() {
        for eps in [0.0, -1.0, f64::INFINITY, f64::NAN, f64::MIN_POSITIVE / 16.0] {
            assert!(correction_kernels(0.0, eps, false).is_err());
        }
        assert!(correction_kernels(f64::NAN, 1.0, false).is_err());
    }

    #[test]
    fn large_width_does_not_overflow_a_representable_station_jet() {
        let eps = f64::MAX / 2.0;
        let (g, g2) = correction_kernels(0.0, eps, false).unwrap();
        assert_eq!(g.value, -eps * (3.0 / 16.0));
        assert_eq!(g.second, -0.75 / eps);
        assert!(g.second < 0.0);
        assert_eq!(g2.first, -eps * (3.0 / 32.0));
    }
}
