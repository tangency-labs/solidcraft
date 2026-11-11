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
    /// Average independent maps with dimension-specific, decreasing kernel radii.
    /// Requires map refresh; this is progressive density estimation, not SPPM.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub progressive_radius: bool,
    /// At opaque receivers at or below this mapped and aged base roughness, let
    /// camera continuation own the smooth-glass area/environment path family.
    /// The existing full base/coat BSDF is retained; this threshold does not
    /// describe the coat lobe. Hard-source, rough and volume photons are retained.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub glossy_continuation_max_roughness: Option<f64>,
    /// One global trace-grid pixel whose selected production sample is diagnosed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub probe_pixel: Option<[u32; 2]>,
    /// Production sample ordinal, independent of adaptive pilot samples.
    #[serde(skip_serializing_if = "is_zero")]
    pub probe_sample: u32,
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
            progressive_radius: false,
            glossy_continuation_max_roughness: None,
            probe_pixel: None,
            probe_sample: 0,
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

fn is_zero(value: &u32) -> bool {
    *value == 0
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
        if self.glossy_continuation_max_roughness.is_some_and(|roughness| !roughness.is_finite() || !(0.0..=1.0).contains(&roughness)) {
            return Err(JobError::new(ErrorCode::InvalidRequest, "glossy caustic continuation requires a finite roughness threshold in 0..1"));
        }
        if self.progressive_radius && !self.refresh_per_pass {
            return Err(JobError::new(ErrorCode::InvalidRequest, "progressive photon radii require independent map refresh per pass"));
        }
        if self.probe_pixel.is_none() && self.probe_sample != 0 {
            return Err(JobError::new(ErrorCode::InvalidRequest, "a photon probe sample requires a probe pixel"));
        }
        Ok(())
    }

    /// Surface and volume radii for the zero-based global map epoch.
    /// Each map is normalized at its own radius before the film averages it.
    /// The schedule is independent of tile order, worker count and final budget.
    pub fn radii_mm(self, epoch: u32) -> Result<[f64; 2]> {
        self.validate()?;
        if !self.progressive_radius {
            return Ok([self.radius_mm; 2]);
        }
        let maps = f64::from(epoch) + 1.0;
        Ok([self.radius_mm * maps.powf(-1.0 / 6.0), self.radius_mm * maps.powf(-1.0 / 7.0)])
    }

    pub fn validate_probe(self, width: u32, height: u32, production_samples: u32) -> Result<()> {
        self.validate()?;
        if let Some([x, y]) = self.probe_pixel
            && (x >= width || y >= height || self.probe_sample >= production_samples)
        {
            return Err(JobError::new(ErrorCode::InvalidRequest, "photon probe pixel or sample exceeds the global trace grid or production budget"));
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

#[cfg(test)]
mod convergence_tests {
    use super::super::photon_diagnostics::{EmissionMeasure, PhotonContribution, PhotonDiagnostics, PhotonProbe};
    use super::*;

    #[test]
    fn progressive_photon_radii_use_global_dimension_specific_epochs_and_preserve_defaults() {
        let fixed = CausticOptions::default();
        for epoch in [0, 1, 63, 127, u32::MAX] {
            assert_eq!(fixed.radii_mm(epoch).unwrap(), [fixed.radius_mm; 2]);
        }
        assert!(CausticOptions { progressive_radius: true, ..fixed }.validate().is_err());
        let progressive = CausticOptions { refresh_per_pass: true, progressive_radius: true, radius_mm: 8.0, ..fixed };
        assert_eq!(progressive.radii_mm(0).unwrap(), [8.0; 2]);
        assert!((progressive.radii_mm(63).unwrap()[0] - 4.0).abs() < 1e-12);
        assert!((progressive.radii_mm(127).unwrap()[1] - 4.0).abs() < 1e-12);
        let mut previous = [f64::INFINITY; 2];
        for epoch in [0, 1, 16, 128, 16384, u32::MAX] {
            let radii = progressive.radii_mm(epoch).unwrap();
            assert!(radii[0] <= radii[1]);
            for index in 0..2 {
                assert!(radii[index].is_finite() && radii[index] > 0.0 && radii[index] <= previous[index]);
            }
            previous = radii;
        }
        let json = serde_json::to_value(fixed).unwrap();
        assert!(json.get("progressive_radius").is_none() && json.get("probe_pixel").is_none() && json.get("probe_sample").is_none());
        assert_eq!(serde_json::from_value::<CausticOptions>(serde_json::to_value(progressive).unwrap()).unwrap(), progressive);
    }

    #[test]
    fn progressive_photon_bandwidth_reduces_analytic_bias_and_independent_batch_variance() {
        let options = CausticOptions { progressive_radius: true, refresh_per_pass: true, ..Default::default() };
        // Uniform disk/ball kernels integrate to one. For f(x)=1+|x|^2,
        // convolution at zero is 1 + d*r^2/(d+2). With independent maps,
        // variance of their mean scales as sum(r^-d)/K^2 for fixed batch size.
        let mut previous_bias = [f64::INFINITY; 2];
        let mut previous_variance = [f64::INFINITY; 2];
        for maps in [16, 256, 4096] {
            let mut bias = [0.0; 2];
            let mut variance = [0.0; 2];
            for epoch in 0..maps {
                for (index, radius) in options.radii_mm(epoch).unwrap().into_iter().enumerate() {
                    let dimension = (index + 2) as f64;
                    bias[index] += dimension * radius.powi(2) / (dimension + 2.0);
                    variance[index] += radius.powf(-dimension);
                }
            }
            for index in 0..2 {
                bias[index] /= f64::from(maps);
                variance[index] /= f64::from(maps).powi(2);
                assert!(bias[index] < previous_bias[index] && variance[index] < previous_variance[index]);
                previous_bias[index] = bias[index];
                previous_variance[index] = variance[index];
            }
        }
    }

    #[test]
    fn photon_probes_bound_records_preserve_quantiles_and_merge_without_double_counting() {
        let contribution = |index: u32| PhotonContribution {
            emitted_path: index,
            source_probability: 1.0,
            emission_density: 0.25,
            emission_measure: EmissionMeasure::SolidAngle,
            reservoir_visits: 1,
            receiver_kind: 1,
            photon_flux: [1.0; 3],
            response: [1.0; 3],
            normal_measure: 1.0,
            radius_mm: 2.0,
            radiance: [2.0_f64.powi(index as i32 - 8); 3],
            camera_prefix: [1.0; 3],
            camera_radiance: [2.0_f64.powi(index as i32 - 8); 3],
            fresnel_strata: 1,
            ..Default::default()
        };
        let make = || PhotonDiagnostics::new([7, 9], 4, 36, 81, [2.0, 3.0]);
        let mut full = make();
        let mut left = make();
        let mut right = make();
        full.begin_gather().unwrap();
        left.begin_gather().unwrap();
        right.begin_gather().unwrap();
        for index in 0..16 {
            if index == 8 {
                full.begin_gather().unwrap();
            }
            full.visit().unwrap();
            let mut event = contribution(index);
            event.gather = index / 8;
            full.observe(event).unwrap();
            let partial = if index < 8 { &mut left } else { &mut right };
            partial.visit().unwrap();
            partial.observe(contribution(index)).unwrap();
        }
        left.merge(&right).unwrap();
        assert_eq!(left, full);
        assert_eq!((full.visited, full.eligible, full.contributing, full.top_count), (16, 16, 16, 8));
        assert_eq!(full.largest[0].emitted_path, 15);
        assert_eq!(full.largest[7].emitted_path, 8);
        assert_eq!(full.histogram.iter().sum::<u32>(), 16);
        let median = full.quantile_bounds(0.5).unwrap().unwrap();
        assert!(median[0].unwrap() <= 0.5 && median[1].unwrap() >= 0.5);
        assert!(full.quantile_bounds(f64::NAN).is_err());
        let mut invalid = contribution(0);
        invalid.normal_measure = f64::INFINITY;
        assert!(full.observe(invalid).is_err());
        let mut wrong_epoch = make();
        wrong_epoch.epoch += 1;
        assert!(full.merge(&wrong_epoch).is_err());
        let options = CausticOptions { probe_pixel: Some([7, 9]), probe_sample: 4, ..Default::default() };
        assert!(options.validate_probe(8, 10, 5).is_ok());
        assert!(options.validate_probe(7, 10, 5).is_err());
        assert!(options.validate_probe(8, 10, 4).is_err());
        let mut owner = PhotonProbe::new(Some(options)).unwrap();
        assert!(!owner.report().unwrap().selected);
        let mut wrong_pixel = full;
        wrong_pixel.pixel[0] += 1;
        assert!(owner.merge(&wrong_pixel).is_err(), "even the first report must match the selected sample");
        assert!(!owner.report().unwrap().selected);
        owner.merge(&full).unwrap();
        assert_eq!(
            owner.report(),
            Some(PhotonDiagnostics { camera: super::super::photon_diagnostics::PhotonCameraAccounting::new([7, 9], 1).unwrap(), ..full })
        );
        owner.reset();
        assert!(!owner.report().unwrap().selected);
        assert_eq!(owner.report().unwrap().contributing, 0);
        assert!(PhotonProbe::new(None).unwrap().report().is_none());
    }

    #[test]
    fn photon_probe_camera_ranking_uses_the_prefix_before_discarding_receiver_candidates() {
        let prefix = [0.001, 100.0, 1.0];
        let mut report = PhotonDiagnostics::new([0, 0], 0, 0, 81, [1.0; 2]);
        report.begin_gather().unwrap();
        for index in 0..9 {
            let local = if index == 8 { [0.0, 1.0, 0.0] } else { [100.0 + f64::from(index), 0.0, 0.0] };
            report.visit().unwrap();
            report
                .observe(PhotonContribution {
                    emitted_path: index,
                    source_probability: 1.0,
                    emission_density: 1.0,
                    radius_mm: 1.0,
                    radiance: local,
                    camera_prefix: prefix,
                    camera_radiance: std::array::from_fn(|axis| local[axis] * prefix[axis]),
                    fresnel_strata: 4,
                    ..Default::default()
                })
                .unwrap();
        }
        assert_eq!(report.top_count, 8);
        assert_eq!(report.camera_top_count, 8);
        assert!(report.largest.iter().all(|entry| entry.emitted_path != 8), "local ranking discards the dim green event");
        assert_eq!(report.largest_camera[0].emitted_path, 8, "the same event dominates after the nonuniform spectral prefix");
        assert_eq!(report.largest_camera[0].camera_radiance, [0.0, 100.0, 0.0]);
        assert_eq!(report.histogram.iter().sum::<u32>(), 9);
        assert_eq!(report.camera_histogram.iter().sum::<u32>(), 9);
        let mut invalid = report.largest_camera[0];
        invalid.camera_prefix[0] = f64::INFINITY;
        assert!(report.observe(invalid).is_err());
    }

    #[test]
    fn photon_probe_complete_lanes_and_unequal_siblings_reconstruct_dim_raw_output() {
        use super::super::photon_diagnostics::{PhotonCameraSample, PhotonEstimatorPartition};
        // Four siblings have distinct production denominators. One is uncovered;
        // each covered lane has two photon queries and a separate direct term.
        for scale in [1.0, 1e-7] {
            for selected in [1, 7] {
                let options = CausticOptions { probe_pixel: Some([1, 1]), probe_sample: selected, seed: 81, ..Default::default() };
                options.validate_probe(2, 2, 8).unwrap();
                let mut probe = PhotonProbe::with_supersampling(Some(options), 2).unwrap();
                for slot in 0..4u32 {
                    let pixel = [slot % 2, slot / 2];
                    for sample in 0..slot + 1 {
                        for lane in 0..4u32 {
                            let identity = PhotonCameraSample { pixel, sample, epoch: 0, stratum: lane, strata: 4 };
                            let covered = slot != 0;
                            let photon = if covered {
                                [f64::from((slot + 1) * (sample + 1) * (lane + 1)), f64::from(2 * (slot + 1)), f64::from(lane)].map(|v| v * scale)
                            } else {
                                [0.0; 3]
                            };
                            let other = if covered {
                                [0.5 * f64::from(lane + 1), f64::from(sample + 1), f64::from(slot + 1)].map(|v| v * scale)
                            } else {
                                [0.0; 3]
                            };
                            for fraction in [0.25, 0.75] {
                                let value = photon.map(|v| v * fraction);
                                probe.record_radiance(identity, true, value).unwrap();
                                if pixel == [1, 1] && sample == selected {
                                    let prefix = [2.0, 0.5, 4.0];
                                    let local = std::array::from_fn(|axis| value[axis] / prefix[axis]);
                                    let mut support = super::super::photon_support::PhotonContributionSupport::default();
                                    assert!(support.observe(local[0] * 0.2126 + local[1] * 0.7152 + local[2] * 0.0722));
                                    let mut query = PhotonDiagnostics::new(pixel, sample, 0, 81, [1.0; 2]);
                                    query.begin_gather().unwrap();
                                    query.visit().unwrap();
                                    query
                                        .observe(PhotonContribution {
                                            source_probability: 1.0,
                                            emission_density: 1.0,
                                            radius_mm: 1.0,
                                            radiance: local,
                                            camera_prefix: prefix,
                                            camera_radiance: value,
                                            stratum: lane,
                                            fresnel_strata: 4,
                                            query_eligible: 1,
                                            query_contributing: 1,
                                            support,
                                            ..Default::default()
                                        })
                                        .unwrap();
                                    probe.merge(&query).unwrap();
                                }
                            }
                            probe.record_radiance(identity, false, other).unwrap();
                            let beauty = std::array::from_fn(|axis| photon[axis] + other[axis]);
                            probe.complete_lane(identity, beauty, covered, [1.0; 2]).unwrap();
                        }
                    }
                }
                let raw = [(12.8125 * scale) as f32, (6.0 * scale) as f32, (3.375 * scale) as f32, 0.75];
                probe.finalize(raw, [1, 2, 3, 4], 2e-6).unwrap();
                let report = probe.report().unwrap();
                let output = report.camera.output.unwrap();
                assert_eq!(report.camera.trace_pixels.map(|pixel| pixel.samples), [1, 2, 3, 4]);
                assert_eq!(report.camera.trace_pixels.map(|pixel| pixel.covered_samples), [0, 2, 3, 4]);
                assert_eq!(output.raw_rgba, raw.map(f64::from));
                let expected = PhotonEstimatorPartition {
                    photon: [11.875, 4.5, 1.125].map(|v| v * scale),
                    other_estimators: [0.9375, 1.5, 2.25].map(|v| v * scale),
                    beauty: [12.8125, 6.0, 3.375].map(|v| v * scale),
                };
                // Relative checks at tiny energy prevent the production guard's
                // absolute floor from concealing a missing estimator term.
                for (actual, expected) in
                    output.photon.into_iter().chain(output.other_estimators).zip(expected.photon.into_iter().chain(expected.other_estimators))
                {
                    assert!((actual - expected).abs() <= 2e-14 * expected.abs(), "{actual} != {expected}");
                }
                for (residual, value) in output.recombination_residual.into_iter().zip(expected.beauty) {
                    assert!(residual.abs() <= 2e-7 * value.abs());
                }
                assert_eq!(report.selected, selected == 1);
                assert_eq!(report.camera.selected_sample.is_some(), selected == 1);
                if selected == 1 {
                    assert_eq!(report.gathers, 8);
                    let sample = report.camera.selected_sample.unwrap();
                    assert_eq!(sample.strata, 4);
                    for (actual, expected) in sample.radiance.photon.into_iter().zip([20.0, 8.0, 1.5].map(|v| v * scale)) {
                        assert!((actual - expected).abs() <= 2e-14 * expected.abs());
                    }
                    for event in report.largest_camera.iter().take(report.camera_top_count as usize) {
                        assert_eq!(event.final_output_radiance.unwrap(), event.camera_radiance.map(|v| v / 64.0));
                    }
                } else {
                    assert_eq!(report.contributing, 0, "an unreached detailed sample does not suppress complete output accounting");
                }
            }
        }
    }

    #[test]
    fn photon_probe_refuses_incomplete_lanes_duplicate_samples_and_wrong_final_counts() {
        use super::super::photon_diagnostics::PhotonCameraSample;
        let options = CausticOptions { probe_pixel: Some([0, 0]), ..Default::default() };
        let identity = PhotonCameraSample { pixel: [0, 0], sample: 0, epoch: 0, stratum: 0, strata: 2 };
        let mut probe = PhotonProbe::new(Some(options)).unwrap();
        probe.record_radiance(identity, false, [1.0; 3]).unwrap();
        probe.complete_lane(identity, [1.0; 3], true, [1.0; 2]).unwrap();
        assert!(probe.finalize([1.0; 4], [1, 0, 0, 0], 1e-6).is_err());
        assert!(probe.complete_lane(identity, [1.0; 3], true, [1.0; 2]).is_err(), "a repeated lane cannot form another observation");
        let second = PhotonCameraSample { stratum: 1, ..identity };
        probe.record_radiance(second, false, [1.0; 3]).unwrap();
        probe.complete_lane(second, [1.0; 3], true, [1.0; 2]).unwrap();
        assert!(probe.finalize([1.0; 4], [2, 0, 0, 0], 1e-6).is_err());
        probe.finalize([1.0; 4], [1, 0, 0, 0], 1e-6).unwrap();
        let before = probe.report().unwrap();
        assert!(before.camera.output.is_some());
        probe.reset();
        assert_eq!(probe.report().unwrap().camera.trace_pixels[0].samples, 0);
        assert!(probe.report().unwrap().camera.output.is_none());
        assert!(!probe.report().unwrap().selected);
        assert_eq!(PhotonProbe::required_bytes(None), 0);
        assert!(PhotonProbe::required_bytes(Some(options)) >= std::mem::size_of::<PhotonDiagnostics>() as u64 * 4);
    }

    #[test]
    fn glossy_receiver_policy_is_explicit_validated_and_inclusive() {
        use super::super::photon_receiver::continues_smooth_caustics;
        let legacy = CausticOptions::default();
        assert!(serde_json::to_value(legacy).unwrap().get("glossy_continuation_max_roughness").is_none());
        assert_eq!(serde_json::from_value::<CausticOptions>(serde_json::json!({})).unwrap(), legacy);
        for maximum in [0.0, 0.35, 1.0] {
            let selected = CausticOptions { glossy_continuation_max_roughness: Some(maximum), ..legacy };
            selected.validate().unwrap(); // A hard-source-only map may retain a no-effect threshold.
            assert_eq!(serde_json::from_value::<CausticOptions>(serde_json::to_value(selected).unwrap()).unwrap(), selected);
            assert!(continues_smooth_caustics(Some(maximum), maximum));
            assert!(continues_smooth_caustics(Some(maximum), 0.0));
        }
        for maximum in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -0.01, 1.01] {
            assert!(CausticOptions { glossy_continuation_max_roughness: Some(maximum), ..legacy }.validate().is_err());
            assert!(!continues_smooth_caustics(Some(maximum), 0.0));
        }
        assert!(!continues_smooth_caustics(None, 0.0));
        assert!(!continues_smooth_caustics(Some(0.35), 0.350_001));
        for roughness in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -0.01] {
            assert!(!continues_smooth_caustics(Some(1.0), roughness));
        }
    }
}
