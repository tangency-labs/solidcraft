//! Homogeneous random-walk scattering inside a certified closed dielectric.
//! Absorption remains Transmission::extinction; scattering is a scalar collision
//! rate in inverse millimetres, not an invented density or surface-opacity hack.
use super::TransmissionError;
use serde::{Deserialize, Serialize};

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
