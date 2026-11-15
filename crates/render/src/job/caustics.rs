//! Opt-in finite-radius density estimation for optical caustics.
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
    /// Include rectangle emitters for smooth closed glass paths to opaque receivers.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub area_lights: bool,
    /// Include environment illumination for the smooth closed glass path family.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub environment: bool,
    /// Concentrate emission toward the captured optics with matched proposal densities.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub guided_emission: bool,
    /// Build a fresh photon map for each complete camera sampling pass.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub refresh_per_pass: bool,
    /// Allocate photons by emitted power with a uniform full-support mixture.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub power_weighted_sources: bool,
}
impl Default for CausticOptions {
    fn default() -> Self {
        Self {
            photons: 16384,
            radius_mm: 1.0,
            max_depth: 16,
            seed: 81,
            area_lights: false,
            environment: false,
            guided_emission: false,
            refresh_per_pass: false,
            power_weighted_sources: false,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct CausticWork {
    pub maps: u32,
    pub emitted_paths: u64,
    /// Light-path events only. Traversal and gathering use their own work units.
    pub max_light_path_events: u64,
}

impl CausticOptions {
    pub fn validate(self) -> Result<()> {
        if !(1..=super::MAX_CAUSTIC_PHOTONS).contains(&self.photons)
            || !self.radius_mm.is_finite()
            || !(1e-6..=1e6).contains(&self.radius_mm)
            || !(1..=super::MAX_PATH_DEPTH).contains(&self.max_depth)
        {
            return Err(JobError::new(
                ErrorCode::InvalidRequest,
                "caustics require 1..1048576 photons, finite radius 0.000001..1000000 mm and 1..64 light-path events",
            ));
        }
        Ok(())
    }

    pub fn refresh_work(self, camera_passes: u32) -> Result<Option<CausticWork>> {
        if !self.refresh_per_pass {
            return Ok(None);
        }
        if camera_passes == 0 {
            return Err(JobError::new(ErrorCode::InvalidRequest, "photon refresh requires at least one camera pass"));
        }
        let overflow = || JobError::new(ErrorCode::LimitExceeded, "photon refresh work estimate overflow");
        let emitted_paths = u64::from(camera_passes).checked_mul(u64::from(self.photons)).ok_or_else(overflow)?;
        let max_light_path_events = emitted_paths.checked_mul(u64::from(self.max_depth)).ok_or_else(overflow)?;
        Ok(Some(CausticWork { maps: camera_passes, emitted_paths, max_light_path_events }))
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
        assert!(serde_json::to_value(valid).unwrap().get("area_lights").is_none());
        assert!(serde_json::to_value(valid).unwrap().get("environment").is_none());
        assert!(serde_json::to_value(valid).unwrap().get("guided_emission").is_none());
        assert!(serde_json::to_value(valid).unwrap().get("refresh_per_pass").is_none());
        let area = CausticOptions { area_lights: true, ..valid };
        assert_eq!(serde_json::from_value::<CausticOptions>(serde_json::to_value(area).unwrap()).unwrap(), area);
        let environment = CausticOptions { environment: true, ..valid };
        assert_eq!(serde_json::from_value::<CausticOptions>(serde_json::to_value(environment).unwrap()).unwrap(), environment);
        let guided = CausticOptions { guided_emission: true, ..valid };
        assert_eq!(serde_json::from_value::<CausticOptions>(serde_json::to_value(guided).unwrap()).unwrap(), guided);
        let refresh = CausticOptions { refresh_per_pass: true, ..guided };
        assert_eq!(serde_json::from_value::<CausticOptions>(serde_json::to_value(refresh).unwrap()).unwrap(), refresh);
        assert_eq!(guided.refresh_work(20).unwrap(), None);
        assert_eq!(refresh.refresh_work(20).unwrap(), Some(CausticWork { maps: 20, emitted_paths: 327_680, max_light_path_events: 5_242_880 }));
        assert!(refresh.refresh_work(0).is_err());
        assert!(CausticOptions { photons: u32::MAX, ..refresh }.refresh_work(u32::MAX).is_err());
        for photons in [0, super::super::MAX_CAUSTIC_PHOTONS + 1, u32::MAX] {
            assert!(CausticOptions { photons, ..valid }.validate().is_err());
        }
        for radius_mm in [f64::NAN, f64::INFINITY, 0.0, -1.0, 1e-7, 1e6 + 1.0] {
            assert!(CausticOptions { radius_mm, ..valid }.validate().is_err());
        }
        for max_depth in [0, super::super::MAX_PATH_DEPTH + 1, u8::MAX] {
            assert!(CausticOptions { max_depth, ..valid }.validate().is_err());
        }
    }
}

#[cfg(test)]
mod source_proposal_tests {
    use super::super::photon_sources::*;
    use super::*;
    #[test]
    fn photon_source_bins_preserve_support_normalization_and_analytic_power() {
        for powers in [vec![0.0; 21], vec![f64::MAX, f64::MIN_POSITIVE, 0.0], vec![1.0], vec![1.0, 9.0, 90.0]] {
            let table = PhotonSourceDistribution::new(&powers).unwrap();
            assert_eq!(*table.cumulative().last().unwrap(), SOURCE_BINS);
            let mut previous = 0;
            let mut integral = 0.0;
            for (source, end) in table.cumulative().iter().copied().enumerate() {
                let width = end - previous;
                assert!(f64::from(width) / f64::from(SOURCE_BINS) >= 0.05 / powers.len() as f64 - 1.0 / f64::from(SOURCE_BINS));
                for bin in [previous, end - 1] {
                    let (selected, pdf) = table.select(f64::from(bin) / f64::from(SOURCE_BINS)).unwrap();
                    assert_eq!(selected, source);
                    assert_eq!(pdf, f64::from(width) / f64::from(SOURCE_BINS));
                }
                let pdf = f64::from(width) / f64::from(SOURCE_BINS);
                // Independent integral of source power under the represented proposal.
                if powers[source] < 1e300 {
                    integral += pdf * (powers[source] / pdf);
                }
                previous = end;
            }
            if powers.iter().all(|p| *p < 1e300) {
                assert!((integral - powers.iter().sum::<f64>()).abs() < 1e-12);
            }
        }
        let table = PhotonSourceDistribution::new(&[1.0, 9.0, 90.0]).unwrap();
        let mut counts = [0; 3];
        for draw in 0..100_000 {
            counts[table.select((f64::from(draw) + 0.5) / 100_000.0).unwrap().0] += 1;
        }
        for (index, power) in [1.0, 9.0, 90.0].into_iter().enumerate() {
            let expected = 0.95 * power / 100.0 + 0.05 / 3.0;
            assert!((f64::from(counts[index]) / 100_000.0 - expected).abs() < 0.00002);
        }
        for invalid in [vec![], vec![1.0; 22], vec![-1.0], vec![f64::NAN], vec![f64::INFINITY]] {
            assert!(PhotonSourceDistribution::new(&invalid).is_err());
        }
        for invalid in [-1.0, 1.0, f64::NAN, f64::INFINITY] {
            assert!(table.select(invalid).is_err());
        }
        assert!(serde_json::to_value(CausticOptions::default()).unwrap().get("power_weighted_sources").is_none());
        let enabled = CausticOptions { power_weighted_sources: true, ..Default::default() };
        assert_eq!(serde_json::from_value::<CausticOptions>(serde_json::to_value(enabled).unwrap()).unwrap(), enabled);
    }
    #[test]
    fn photon_source_power_matches_independent_spot_integral_and_rectangle_disk_units() {
        use super::super::{PunctualLightOptions, SpotConeOptions};
        let light = PunctualLightOptions {
            position_mm: [0.0; 3],
            color: [1.0; 3],
            intensity: 2.0,
            reference_distance_mm: 3.0,
            minimum_distance_mm: 1.0,
            cone: None,
        };
        assert!((punctual_power(&light).unwrap() - 72.0 * std::f64::consts::PI).abs() < 1e-12);
        let spot = PunctualLightOptions {
            cone: Some(SpotConeOptions { emitted_axis: [0.0, 0.0, 1.0], inner_half_angle_radians: 0.2, outer_half_angle_radians: 0.5 }),
            ..light
        };
        let mut integral = 0.0;
        for index in 0..100_000 {
            let z = -1.0 + 2.0 * (f64::from(index) + 0.5) / 100_000.0;
            let t = ((z - 0.5_f64.cos()) / (0.2_f64.cos() - 0.5_f64.cos())).clamp(0.0, 1.0);
            integral += 18.0 * t * t * (3.0 - 2.0 * t) * 4.0 * std::f64::consts::PI / 100_000.0;
        }
        assert!((punctual_power(&spot).unwrap() - integral).abs() < 1e-7);
        assert!((rectangle_power(6.0, [2.0; 3]).unwrap() - 12.0 * std::f64::consts::PI).abs() < 1e-12);
        assert!((disk_power(2.0, 3.0).unwrap() - 12.0 * std::f64::consts::PI).abs() < 1e-12);
        assert!(disk_power(f64::MAX, 1.0).is_err());
        assert!(rectangle_power(-1.0, [1.0; 3]).is_err());
    }
}
