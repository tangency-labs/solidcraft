//! Bounded first-interface groups and refractive roulette arithmetic.
//!
//! A group is one camera observation. It owns no rays or heap allocation; the
//! existing continuation slot is reused for each lane before the film is updated.
use crate::{
    job::{ErrorCode, JobError, Result},
    radiance::{CameraPath, LightPath, RadianceComponents},
};
use solidcore_record::Vec3;

fn invalid(message: &str) -> JobError {
    JobError::new(ErrorCode::InvalidRequest, message)
}

#[derive(Clone, Copy, Default)]
pub(super) struct FresnelGroup {
    pub lane: u32,
    /// The first-interface choice has forked subsequent random domains.
    pub stratified: bool,
    radiance: Vec3,
    components: RadianceComponents,
    coverage: Option<u32>,
}

#[derive(Clone, Copy)]
pub(super) struct SmoothSplit {
    pub transmitted: bool,
    /// Physical branch mass times the two-lane representation weight.
    pub weight: f64,
}

impl FresnelGroup {
    /// Force one child at the first eligible smooth boundary. A zero-mass child
    /// still completes its lane with its prefix, without tracing a suffix.
    pub fn split(&mut self, reflectance: f64, transmission_exists: bool) -> Result<SmoothSplit> {
        if self.stratified || self.lane >= 2 || !reflectance.is_finite() || !(0.0..=1.0).contains(&reflectance) {
            return Err(invalid("invalid CPU smooth first-interface split"));
        }
        self.stratified = true;
        let reflected = if transmission_exists { reflectance } else { 1.0 };
        let transmitted = self.lane == 1;
        Ok(SmoothSplit { transmitted, weight: 2.0 * if transmitted { 1.0 - reflected } else { reflected } })
    }

    /// Only the first Fresnel variate is stratified. All earlier choices share
    /// the same prefix; callers separate later domains using lane + 1.
    pub fn branch(&mut self, uniform: f64, strata: u32) -> Result<f64> {
        if !matches!(strata, 1 | 2 | 4) || self.lane >= strata || !uniform.is_finite() || !(0.0..1.0).contains(&uniform) {
            return Err(invalid("invalid CPU Fresnel stratum or branch variate"));
        }
        if strata == 1 || self.stratified {
            return Ok(uniform);
        }
        self.stratified = true;
        let upper = f64::from(self.lane + 1) / f64::from(strata);
        // Even a valid 53-bit input can round onto a later stratum's upper
        // boundary during addition. Keep the sample in its half-open bin.
        Ok(((f64::from(self.lane) + uniform) / f64::from(strata)).min(upper.next_down()))
    }

    pub fn complete(
        &mut self,
        radiance: Vec3,
        components: Option<RadianceComponents>,
        coverage: u32,
        strata: u32,
    ) -> Result<Option<(Vec3, Option<RadianceComponents>, u32)>> {
        if !matches!(strata, 1 | 2 | 4) || self.lane >= strata || coverage > 1 || self.coverage.is_some_and(|prior| prior != coverage) {
            return Err(invalid("CPU Fresnel lanes disagree with their shared camera coverage"));
        }
        let weight = 1.0 / f64::from(strata);
        let sum = self.radiance + radiance * weight;
        crate::raytrace::lighting::validate_radiance(sum).map_err(|error| invalid(&error.to_string()))?;
        let mut combined = self.components;
        if let Some(components) = components {
            for camera in [CameraPath::Other, CameraPath::Reflected, CameraPath::Transmitted] {
                for light in [LightPath::NonCaustic, LightPath::Caustic] {
                    combined.add(camera, light, components.component(camera, light).map(|value| value * weight))?;
                }
            }
        }
        self.coverage = Some(coverage);
        self.radiance = sum;
        self.components = combined;
        self.lane += 1;
        self.stratified = false;
        Ok((self.lane == strata).then_some((sum, components.map(|_| combined), coverage)))
    }
}

/// Undo only refractive radiance scaling for the survival decision. Never apply
/// this factor to radiance already accumulated or to the physical throughput.
pub(super) fn refractive_scale(scale: f64, incident_ior: f64, transmitted_ior: f64) -> Result<f64> {
    if [scale, incident_ior, transmitted_ior].into_iter().any(|value| !value.is_finite() || value <= 0.0) {
        return Err(invalid("CPU roulette has invalid refractive scale"));
    }
    let next = scale * (transmitted_ior / incident_ior).powi(2);
    if !next.is_finite() || next <= 0.0 {
        return Err(invalid("CPU roulette refractive scale exceeds finite range"));
    }
    Ok(next)
}

pub(super) fn survival(throughput: Vec3, eta_scale: f64) -> Result<f64> {
    crate::raytrace::lighting::validate_radiance(throughput).map_err(|error| invalid(&error.to_string()))?;
    if !eta_scale.is_finite() || eta_scale <= 0.0 {
        return Err(invalid("CPU roulette has invalid refractive scale"));
    }
    let corrected = throughput.x.max(throughput.y).max(throughput.z) * eta_scale;
    if !corrected.is_finite() {
        return Err(invalid("CPU roulette corrected throughput exceeds finite range"));
    }
    Ok(corrected.clamp(0.05, 1.0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use solidcraft_geom::Vec3Geom;

    #[test]
    fn first_fresnel_groups_keep_prefix_coverage_and_average_once() {
        for strata in [1, 2, 4] {
            let mut group = FresnelGroup::default();
            for lane in 0..strata {
                assert_eq!(group.lane, lane);
                let first = group.branch(0.25, strata).unwrap();
                assert_eq!(first, (f64::from(lane) + 0.25) / f64::from(strata));
                assert_eq!(group.branch(0.75, strata).unwrap(), 0.75);
                let mut components = RadianceComponents::default();
                components.add(CameraPath::Reflected, LightPath::Caustic, [f64::from(lane + 1), 0.0, 0.0]).unwrap();
                let done = group.complete(Vec3::X * f64::from(lane + 1), Some(components), 1, strata).unwrap();
                if lane + 1 == strata {
                    let (radiance, components, coverage) = done.unwrap();
                    assert_eq!(radiance, Vec3::X * (f64::from(strata + 1) / 2.0));
                    assert_eq!(components.unwrap().beauty(), [radiance.x, 0.0, 0.0]);
                    assert_eq!(coverage, 1);
                } else {
                    assert!(done.is_none());
                }
            }
        }
    }

    #[test]
    fn fresnel_mapping_preserves_half_open_bins_at_float_endpoints() {
        for uniform in [0.0, 2.0_f64.powi(-24), 0.5, 1.0 - 2.0_f64.powi(-24), 1.0_f64.next_down()] {
            let mut single = FresnelGroup::default();
            assert_eq!(single.branch(uniform, 1).unwrap().to_bits(), uniform.to_bits());
            for strata in [2, 4] {
                for lane in 0..strata {
                    let mut group = FresnelGroup { lane, ..Default::default() };
                    let mapped = group.branch(uniform, strata).unwrap();
                    let lower = f64::from(lane) / f64::from(strata);
                    let upper = f64::from(lane + 1) / f64::from(strata);
                    assert!(mapped >= lower && mapped < upper, "lane={lane}/{strata}, uniform={uniform}");
                    if uniform == 1.0_f64.next_down() && lane > 0 {
                        assert_eq!(mapped.to_bits(), upper.next_down().to_bits());
                    }
                }
            }
        }
    }

    #[test]
    fn strata_fail_atomically_for_invalid_lanes_and_inconsistent_primary_coverage() {
        let mut group = FresnelGroup::default();
        assert!(group.branch(f64::NAN, 4).is_err());
        assert!(group.branch(1.0, 4).is_err());
        assert!(group.branch(0.5, 3).is_err());
        group.complete(Vec3::ZERO, None, 0, 2).unwrap();
        assert!(group.complete(Vec3::X, None, 1, 2).is_err());
        assert_eq!(group.lane, 1);
    }

    #[test]
    fn roulette_excludes_refractive_scaling_and_preserves_conditional_expectation() {
        let inside = refractive_scale(1.0, 1.0, 1.5).unwrap();
        assert_eq!(survival(Vec3::new(1.0, 1.0, 1.0) / 2.25, inside).unwrap(), 1.0);
        let outside = refractive_scale(inside, 1.5, 1.0).unwrap();
        assert!((outside - 1.0).abs() < 1e-14);
        for throughput in [0.0, 0.001, 0.1, 0.75, 1.0, 12.0] {
            let probability = survival(Vec3::X * throughput, 1.0).unwrap();
            assert!((probability * (throughput / probability) - throughput).abs() < 1e-14);
            assert!((0.05..=1.0).contains(&probability));
        }
        assert!(refractive_scale(1.0, 0.0, 1.5).is_err());
        assert!(survival(Vec3::X, f64::NAN).is_err());
    }

    #[test]
    fn smooth_split_weights_both_suffixes_without_scaling_shared_prefix() {
        let prefix = Vec3::new(3.0, 5.0, 7.0);
        let reflected = Vec3::new(11.0, 2.0, 1.0);
        let transmitted = Vec3::new(1.0, 13.0, 4.0);
        let eta_squared = 4.0 / 9.0;
        for fresnel in [0.0, 0.04, 0.25, 0.9, 1.0] {
            let mut group = FresnelGroup::default();
            for lane in 0..2 {
                let choice = group.split(fresnel, true).unwrap();
                assert_eq!(choice.transmitted, lane == 1);
                let suffix = if choice.transmitted { transmitted * eta_squared } else { reflected } * choice.weight;
                let mut components = RadianceComponents::default();
                components.add(CameraPath::Other, LightPath::NonCaustic, [prefix.x, prefix.y, prefix.z]).unwrap();
                let camera = if choice.transmitted { CameraPath::Transmitted } else { CameraPath::Reflected };
                components.add(camera, LightPath::NonCaustic, [suffix.x, suffix.y, suffix.z]).unwrap();
                let result = group.complete(prefix + suffix, Some(components), 1, 2).unwrap();
                if lane == 0 {
                    assert!(result.is_none());
                } else {
                    let (radiance, components, coverage) = result.unwrap();
                    let expected = prefix + reflected * fresnel + transmitted * ((1.0 - fresnel) * eta_squared);
                    assert!((radiance - expected).len() < 2e-14);
                    assert_eq!(coverage, 1);
                    let components = components.unwrap();
                    assert_eq!(components.component(CameraPath::Other, LightPath::NonCaustic), [3.0, 5.0, 7.0]);
                    for (a, b) in components.beauty().into_iter().zip([radiance.x, radiance.y, radiance.z]) {
                        assert!((a - b).abs() < 2e-14);
                    }
                }
            }
        }
    }

    #[test]
    fn smooth_split_total_internal_reflection_and_zero_mass_still_complete_two_lanes() {
        let mut group = FresnelGroup::default();
        let reflection = group.split(1.0, false).unwrap();
        assert_eq!((reflection.transmitted, reflection.weight), (false, 2.0));
        assert!(group.split(0.5, true).is_err(), "a selected boundary cannot recursively split");
        assert!(group.complete(Vec3::X * 6.0, None, 1, 2).unwrap().is_none());
        let transmission = group.split(1.0, false).unwrap();
        assert_eq!((transmission.transmitted, transmission.weight), (true, 0.0));
        let result = group.complete(Vec3::ZERO, None, 1, 2).unwrap().unwrap();
        assert_eq!(result.0, Vec3::X * 3.0);
        assert_eq!(result.2, 1);
        for invalid in [f64::NAN, f64::INFINITY, -0.1, 1.1] {
            assert!(FresnelGroup::default().split(invalid, true).is_err());
        }
        assert!(FresnelGroup { lane: 2, ..Default::default() }.split(0.5, true).is_err());
    }
}
