//! Opt-in finite-radius density estimation for delta-light optical caustics.
//! Photon count and radius are explicit quality controls, not a convergence certificate.
use super::{ErrorCode, JobError, Result};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CausticOptions {
    pub photons: u32,
    pub radius_mm: f64,
    pub max_depth: u8,
    pub seed: u32,
}
impl Default for CausticOptions {
    fn default() -> Self {
        Self { photons: 16384, radius_mm: 1.0, max_depth: 16, seed: 81 }
    }
}
impl CausticOptions {
    pub fn validate(self) -> Result<()> {
        if !(1..=65536).contains(&self.photons)
            || !self.radius_mm.is_finite()
            || !(1e-6..=1e6).contains(&self.radius_mm)
            || !(1..=16).contains(&self.max_depth)
        {
            return Err(JobError::new(
                ErrorCode::InvalidRequest,
                "caustics require 1..65536 photons, finite radius 0.000001..1000000 mm and 1..16 light-path events",
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn photon_options_round_trip_and_reject_unbounded_work_and_radius() {
        let valid = CausticOptions::default();
        valid.validate().unwrap();
        assert_eq!(serde_json::from_value::<CausticOptions>(serde_json::to_value(valid).unwrap()).unwrap(), valid);
        for photons in [0, 65537, u32::MAX] {
            assert!(CausticOptions { photons, ..valid }.validate().is_err());
        }
        for radius_mm in [f64::NAN, f64::INFINITY, 0.0, -1.0, 1e-7, 1e6 + 1.0] {
            assert!(CausticOptions { radius_mm, ..valid }.validate().is_err());
        }
        for max_depth in [0, 17, u8::MAX] {
            assert!(CausticOptions { max_depth, ..valid }.validate().is_err());
        }
    }
}
