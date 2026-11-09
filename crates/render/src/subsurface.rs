//! Homogeneous random-walk scattering inside a certified closed dielectric.
//! Absorption remains Transmission::extinction; scattering is a scalar collision
//! rate in inverse millimetres, not an invented density or surface-opacity hack.
use super::TransmissionError;
use serde::{Deserialize, Serialize};
use solidcore_record::Vec3;
use solidcraft_geom::prelude::*;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Subsurface {
    pub scattering_per_mm: f64,
    #[serde(default)]
    pub anisotropy: f64,
}
impl Subsurface {
    pub fn validate(self) -> Result<(), TransmissionError> {
        if !self.scattering_per_mm.is_finite()
            || !(0.0..=100.0).contains(&self.scattering_per_mm)
            || !self.anisotropy.is_finite()
            || !(-0.95..=0.95).contains(&self.anisotropy)
        {
            return Err(TransmissionError::Scattering);
        }
        Ok(())
    }
    pub fn active(self) -> bool {
        self.scattering_per_mm > 0.0
    }
    /// Analog collision distance in millimetres. A positive infinity means the
    /// collision is beyond every finite admitted query, not an unbounded loop.
    /// Counter zero uses the centre of its lowest53-bit cell; no other sample,
    /// radiance, optical number or geometric distance is clamped.
    pub fn free_flight(self, uniform: f64) -> Result<f64, TransmissionError> {
        self.validate()?;
        if !self.active() || !uniform.is_finite() || !(0.0..1.0).contains(&uniform) {
            return Err(TransmissionError::Scattering);
        }
        let open = if uniform == 0.0 { 1.0 / 18_014_398_509_481_984.0 } else { uniform };
        let distance = -(-open).ln_1p() / self.scattering_per_mm;
        if distance.is_nan() || distance <= 0.0 {
            return Err(TransmissionError::Scattering);
        }
        Ok(distance)
    }
    /// Normalized HG density over propagation directions, per steradian.
    pub fn phase(self, cosine: f64) -> Result<f64, TransmissionError> {
        self.validate()?;
        if !cosine.is_finite() || !(-1.0..=1.0).contains(&cosine) {
            return Err(TransmissionError::Angle);
        }
        let g = self.anisotropy;
        let denominator = 1.0 + g * g - 2.0 * g * cosine;
        Ok((1.0 - g * g) / (4.0 * std::f64::consts::PI * denominator * denominator.sqrt()))
    }
    /// Inverse HG distribution continuous at g=0. The small-g expression is an
    /// algebraic rearrangement of the same inverse, never an isotropic shortcut.
    pub fn sample_cosine(self, uniform: f64) -> Result<f64, TransmissionError> {
        self.validate()?;
        if !uniform.is_finite() || !(0.0..1.0).contains(&uniform) {
            return Err(TransmissionError::Scattering);
        }
        let g = self.anisotropy;
        let cosine = if g.abs() < 0.1 {
            let t = 2.0 * uniform - 1.0;
            let numerator = t + 0.5 * g * (3.0 + t * t) + g * g * t + 0.5 * g * g * g * (t * t - 1.0);
            numerator / (1.0 + g * t).powi(2)
        } else {
            let ratio = (1.0 - g * g) / (1.0 - g + 2.0 * g * uniform);
            (1.0 + g * g - ratio * ratio) / (2.0 * g)
        };
        // Floating roundoff at the two geometric poles cannot change a unit
        // direction's cosine domain. This is not a radiance or density clamp.
        Ok(cosine.clamp(-1.0, 1.0))
    }
    pub fn sample_direction(self, incoming: Vec3, uniform: [f64; 2]) -> Result<Vec3, TransmissionError> {
        if !uniform[1].is_finite() || !(0.0..1.0).contains(&uniform[1]) {
            return Err(TransmissionError::Scattering);
        }
        let direction = crate::surface::unit_direction(incoming).map_err(|_| TransmissionError::Direction)?;
        let cosine = self.sample_cosine(uniform[0])?;
        let radius = ((1.0 - cosine) * (1.0 + cosine)).sqrt();
        let angle = std::f64::consts::TAU * uniform[1];
        let axis = if direction.z.abs() > 0.9 { Vec3::Y } else { Vec3::Z };
        let x = crate::surface::unit_direction(axis.cross(direction)).map_err(|_| TransmissionError::Direction)?;
        let y = direction.cross(x);
        crate::surface::unit_direction(x * (radius * angle.cos()) + y * (radius * angle.sin()) + direction * cosine)
            .map_err(|_| TransmissionError::Direction)
    }
    pub fn survival(self, distance_mm: f64) -> Result<f64, TransmissionError> {
        self.validate()?;
        if !distance_mm.is_finite() || distance_mm < 0.0 {
            return Err(TransmissionError::Distance);
        }
        Ok((-self.scattering_per_mm * distance_mm).exp())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn scattering_rate_is_a_length_quantity_and_absorption_is_independent() {
        let s = Subsurface { scattering_per_mm: 0.4, anisotropy: 0.3 };
        assert_eq!(s.survival(2.5).unwrap(), (-1.0_f64).exp());
        assert!((s.survival(1.0).unwrap() * s.survival(1.5).unwrap() - s.survival(2.5).unwrap()).abs() < 1e-15);
        for invalid in [f64::NAN, f64::INFINITY, -1.0, 100.01] {
            assert!(Subsurface { scattering_per_mm: invalid, ..s }.validate().is_err());
        }
        assert!(Subsurface { anisotropy: 1.0, ..s }.validate().is_err());
        assert!(s.survival(-1.0).is_err());
        assert!(!Subsurface { scattering_per_mm: 0.0, ..s }.active());
    }
}

#[cfg(test)]
mod transport_math_tests {
    use super::*;
    #[test]
    fn free_flight_matches_independent_survival_inverse_and_keeps_tiny_rate_queries_bounded() {
        for rate in [0.001, 0.4, 100.0] {
            let s = Subsurface { scattering_per_mm: rate, anisotropy: 0.0 };
            for u in [0.0, 1e-8, 0.1, 0.5, 1.0_f64.next_down()] {
                let distance = s.free_flight(u).unwrap();
                assert!(distance > 0.0 && distance.is_finite());
                let open = if u == 0.0 { 2.0_f64.powi(-54) } else { u };
                assert!((s.survival(distance).unwrap() - (1.0 - open)).abs() < 3e-15);
            }
        }
        let tiny = Subsurface { scattering_per_mm: f64::MIN_POSITIVE * f64::EPSILON, anisotropy: 0.0 };
        assert_eq!(tiny.free_flight(0.5).unwrap(), f64::INFINITY, "an infinite flight only means no collision on a finite query");
        assert_eq!(tiny.survival(1e12).unwrap(), 1.0);
        let inactive = Subsurface { scattering_per_mm: 0.0, anisotropy: 0.0 };
        assert!(inactive.free_flight(0.5).is_err());
        for value in [f64::NAN, f64::INFINITY, -0.1, 1.0] {
            assert!(Subsurface { scattering_per_mm: 1.0, anisotropy: 0.0 }.free_flight(value).is_err());
        }
    }
    #[test]
    fn hg_inverse_matches_an_independent_cdf_for_zero_small_and_extreme_anisotropy() {
        for g in [-0.95, -0.1, -1e-10, 0.0, 1e-10, 0.1, 0.95] {
            let s = Subsurface { scattering_per_mm: 1.0, anisotropy: g };
            for u in [0.0, 1e-8, 0.001, 0.1, 0.5, 0.9, 0.999, 1.0_f64.next_down()] {
                let c = s.sample_cosine(u).unwrap();
                // Integral of2pi*p(c), rationalized independently of the inverse.
                let root = (1.0 + g * g - 2.0 * g * c).sqrt();
                let cdf = (1.0 - g) * (1.0 + c) / ((1.0 + g + root) * root);
                assert!((cdf - u).abs() < 2e-12, "g{g},u{u}: CDF{cdf}");
                for incoming in [Vec3::Z, -Vec3::Z, Vec3::X, Vec3::new(0.3, 0.4, 0.5)] {
                    let direction = s.sample_direction(incoming, [u, 0.37]).unwrap();
                    let incoming = crate::surface::unit_direction(incoming).unwrap();
                    assert!((direction.len() - 1.0).abs() < 1e-15);
                    assert!((direction.dot(incoming) - c).abs() < 1e-15);
                }
                if g == 0.0 {
                    assert_eq!(c, 2.0 * u - 1.0);
                }
            }
        }
    }
    fn integrate(f: &impl Fn(f64) -> f64, a: f64, b: f64, eps: f64, depth: u8) -> f64 {
        let m = (a + b) * 0.5;
        let q1 = (a + m) * 0.5;
        let q3 = (m + b) * 0.5;
        let coarse = (b - a) * (f(a) + 4.0 * f(m) + f(b)) / 6.0;
        let fine = (b - a) * (f(a) + 4.0 * f(q1) + 2.0 * f(m) + 4.0 * f(q3) + f(b)) / 12.0;
        if depth == 0 || (fine - coarse).abs() < 15.0 * eps {
            fine + (fine - coarse) / 15.0
        } else {
            integrate(f, a, m, eps * 0.5, depth - 1) + integrate(f, m, b, eps * 0.5, depth - 1)
        }
    }
    #[test]
    fn hg_density_has_unit_mass_and_its_authored_first_moment_by_independent_quadrature() {
        for g in [-0.95, -0.1, -1e-10, 0.0, 1e-10, 0.1, 0.95] {
            let s = Subsurface { scattering_per_mm: 1.0, anisotropy: g };
            let density = |c| std::f64::consts::TAU * s.phase(c).unwrap();
            assert!((integrate(&density, -1.0, 1.0, 1e-13, 24) - 1.0).abs() < 2e-11);
            assert!((integrate(&|c| c * density(c), -1.0, 1.0, 1e-13, 24) - g).abs() < 2e-11);
        }
    }
}
