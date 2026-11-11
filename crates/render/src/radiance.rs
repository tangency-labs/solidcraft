//! Exclusive scene-linear path components and bounded observation statistics.
//!
//! A component describes the physical path, not the estimator which sampled it.
//! Classify the first camera-side optical event once; later interfaces retain it.
//! A caustic may therefore belong to a reflected or transmitted component.
//! All input radiance is already weighted. This module never reapplies a BSDF,
//! sampling probability, exposure, reconstruction kernel or display transform.
use crate::job::{ErrorCode, JobError, Result};

/// First camera-side optical event; ordinary paths retain `Other`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CameraPath {
    #[default]
    Other,
    Reflected,
    Transmitted,
}

/// Physical light-side path class, independent of the estimating technique.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum LightPath {
    #[default]
    NonCaustic,
    Caustic,
}

/// Number of disjoint camera-class by caustic-class components.
pub const COMPONENT_COUNT: usize = 6;

impl CameraPath {
    /// An established optical classification survives subsequent interfaces.
    pub fn first(self, event: Self) -> Self {
        if self == Self::Other { event } else { self }
    }
    fn index(self) -> usize {
        match self {
            Self::Other => 0,
            Self::Reflected => 1,
            Self::Transmitted => 2,
        }
    }
}
impl LightPath {
    fn index(self) -> usize {
        match self {
            Self::NonCaustic => 0,
            Self::Caustic => 1,
        }
    }
}
fn invalid(message: &str) -> JobError {
    JobError::new(ErrorCode::InvalidRequest, message)
}
fn valid_radiance(rgb: [f64; 3]) -> bool {
    rgb.into_iter().all(|value| value.is_finite() && (0.0..=1e12).contains(&value))
}

/// Six exclusive additive contributions for one camera observation.
///
/// Missing events remain zero. Record the complete sample, including those zeros,
/// in `ComponentMoments`; recording only nonzero events changes the estimator.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RadianceComponents {
    values: [[f64; 3]; COMPONENT_COUNT],
}
impl RadianceComponents {
    pub fn add(&mut self, camera: CameraPath, light: LightPath, radiance: [f64; 3]) -> Result<()> {
        if !valid_radiance(radiance) {
            return Err(invalid("invalid scene-linear component radiance"));
        }
        let index = camera.index() * 2 + light.index();
        let previous = self.values.get(index).copied().ok_or_else(|| invalid("radiance component index exceeds its partition"))?;
        let sum = std::array::from_fn(|channel| previous[channel] + radiance[channel]);
        let beauty = self.beauty();
        if !valid_radiance(sum) || !valid_radiance(std::array::from_fn(|channel| beauty[channel] + radiance[channel])) {
            return Err(invalid("scene-linear component sum exceeds finite radiance range"));
        }
        let destination = self.values.get_mut(index).ok_or_else(|| invalid("radiance component disappeared"))?;
        *destination = sum;
        Ok(())
    }
    pub fn component(&self, camera: CameraPath, light: LightPath) -> [f64; 3] {
        self.values.get(camera.index() * 2 + light.index()).copied().unwrap_or_default()
    }
    pub fn values(&self) -> &[[f64; 3]; COMPONENT_COUNT] {
        &self.values
    }
    pub fn beauty(&self) -> [f64; 3] {
        let mut sum = [0.0; 3];
        for component in self.values {
            for (target, value) in sum.iter_mut().zip(component) {
                *target += value;
            }
        }
        sum
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Moments {
    mean: [f64; 3],
    m2: [f64; 3],
}
impl Moments {
    fn observe(self, value: [f64; 3], count: u64) -> Result<Self> {
        if count == 0 || !valid_radiance(value) {
            return Err(invalid("invalid radiance moment observation"));
        }
        let mut next = self;
        for ((mean, m2), value) in next.mean.iter_mut().zip(next.m2.iter_mut()).zip(value) {
            let delta = value - *mean;
            *mean += delta / count as f64;
            *m2 += delta * (value - *mean);
            // Invalid moments are an error, never clamped into apparent
            // convergence. The radiance range bounds each finite Welford update.
            if !mean.is_finite() || !m2.is_finite() || *m2 < 0.0 {
                return Err(invalid("radiance moments exceed finite range"));
            }
        }
        Ok(next)
    }
    fn uncertainty(self, count: u64) -> UncertaintyPixel {
        if count < 2 {
            return UncertaintyPixel { variance_of_mean: [0.0; 3], count: count as u32 };
        }
        let denominator = count as f64 * (count - 1) as f64;
        UncertaintyPixel { variance_of_mean: self.m2.map(|value| (value / denominator) as f32), count: count as u32 }
    }
}

/// Per-pixel scatter of camera observations, conditional on any shared photon map.
///
/// This is not a convergence certificate. A reused photon map, spatially shared
/// photons, a changing reconstruction radius and samples within one randomized
/// low-discrepancy sequence require separate uncertainty assessment.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RadianceMoments {
    moments: Moments,
    count: u64,
}
impl RadianceMoments {
    pub const BYTES: u64 = std::mem::size_of::<Self>() as u64;
    pub fn observe(&mut self, radiance: [f64; 3]) -> Result<()> {
        let count =
            self.count.checked_add(1).filter(|count| *count <= u64::from(u32::MAX)).ok_or_else(|| invalid("radiance observation count overflow"))?;
        let moments = self.moments.observe(radiance, count)?;
        self.moments = moments;
        self.count = count;
        Ok(())
    }
    pub fn count(&self) -> u64 {
        self.count
    }
    pub fn mean(&self) -> [f64; 3] {
        self.moments.mean
    }
    pub fn uncertainty(&self) -> UncertaintyPixel {
        self.moments.uncertainty(self.count)
    }
}

/// Component moments retain a separate beauty observation to include covariance.
/// Adding component variances would be wrong when exclusive events are correlated.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ComponentMoments {
    components: [Moments; COMPONENT_COUNT],
    beauty: Moments,
    count: u64,
}
impl ComponentMoments {
    pub const BYTES: u64 = std::mem::size_of::<Self>() as u64;
    pub fn observe(&mut self, sample: &RadianceComponents) -> Result<()> {
        let count =
            self.count.checked_add(1).filter(|count| *count <= u64::from(u32::MAX)).ok_or_else(|| invalid("component observation count overflow"))?;
        let mut next = *self;
        for (moments, value) in next.components.iter_mut().zip(sample.values) {
            *moments = moments.observe(value, count)?;
        }
        next.beauty = next.beauty.observe(sample.beauty(), count)?;
        next.count = count;
        *self = next;
        Ok(())
    }
    pub fn count(&self) -> u64 {
        self.count
    }
    pub fn component_mean(&self, camera: CameraPath, light: LightPath) -> [f64; 3] {
        self.components.get(camera.index() * 2 + light.index()).map_or([0.0; 3], |moments| moments.mean)
    }
    pub fn component_uncertainty(&self, camera: CameraPath, light: LightPath) -> UncertaintyPixel {
        self.components.get(camera.index() * 2 + light.index()).copied().unwrap_or_default().uncertainty(self.count)
    }
    pub fn beauty_mean(&self) -> [f64; 3] {
        self.beauty.mean
    }
    pub fn beauty_uncertainty(&self) -> UncertaintyPixel {
        self.beauty.uncertainty(self.count)
    }
}

/// Portable 16-byte filter input. Count below two means unavailable statistics,
/// not zero uncertainty. `variance_of_mean` is camera-conditional scatter only.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct UncertaintyPixel {
    pub variance_of_mean: [f32; 3],
    pub count: u32,
}
impl UncertaintyPixel {
    pub const BYTES: u64 = std::mem::size_of::<Self>() as u64;
    pub fn validate(self) -> Result<()> {
        if self.variance_of_mean.into_iter().any(|value| !value.is_finite() || value < 0.0) {
            return Err(invalid("invalid radiance uncertainty for denoising"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn optical_and_caustic_components_are_exclusive_and_linear() {
        let mut sample = RadianceComponents::default();
        for camera in [CameraPath::Other, CameraPath::Reflected, CameraPath::Transmitted] {
            for light in [LightPath::NonCaustic, LightPath::Caustic] {
                sample.add(camera, light, [1.0, 2.0, 3.0]).unwrap();
            }
        }
        assert_eq!(sample.beauty(), [6.0, 12.0, 18.0]);
        assert_eq!(sample.component(CameraPath::Transmitted, LightPath::Caustic), [1.0, 2.0, 3.0]);
        assert_eq!(CameraPath::Reflected.first(CameraPath::Transmitted), CameraPath::Reflected);
        assert_eq!(CameraPath::Other.first(CameraPath::Transmitted), CameraPath::Transmitted);
    }

    #[test]
    fn absent_events_are_zero_observations_and_beauty_retains_covariance() {
        let mut a = RadianceComponents::default();
        a.add(CameraPath::Reflected, LightPath::Caustic, [2.0; 3]).unwrap();
        let mut b = RadianceComponents::default();
        b.add(CameraPath::Transmitted, LightPath::NonCaustic, [2.0; 3]).unwrap();
        let mut moments = ComponentMoments::default();
        moments.observe(&a).unwrap();
        moments.observe(&b).unwrap();
        assert_eq!(moments.count(), 2);
        assert_eq!(moments.component_mean(CameraPath::Reflected, LightPath::Caustic), [1.0; 3]);
        assert_eq!(moments.component_uncertainty(CameraPath::Reflected, LightPath::Caustic).variance_of_mean, [1.0; 3]);
        assert_eq!(moments.beauty_mean(), [2.0; 3]);
        assert_eq!(moments.beauty_uncertainty().variance_of_mean, [0.0; 3]);
    }

    #[test]
    fn uncertainty_uses_sample_variance_and_marks_missing_observations() {
        let mut moments = RadianceMoments::default();
        assert_eq!(moments.uncertainty().count, 0);
        moments.observe([1.0; 3]).unwrap();
        assert_eq!(moments.uncertainty().count, 1);
        moments.observe([3.0; 3]).unwrap();
        assert_eq!(moments.mean(), [2.0; 3]);
        assert_eq!(moments.uncertainty().variance_of_mean, [1.0; 3]);
        moments.observe([5.0; 3]).unwrap();
        assert_eq!(moments.uncertainty().variance_of_mean, [4.0 / 3.0; 3]);
        assert_eq!(UncertaintyPixel::BYTES, 16);
        assert_eq!(RadianceMoments::BYTES, 56);
        assert_eq!(ComponentMoments::BYTES, 344);
    }

    #[test]
    fn invalid_component_and_moment_inputs_leave_state_unchanged() {
        let mut sample = RadianceComponents::default();
        sample.add(CameraPath::Other, LightPath::NonCaustic, [1e12; 3]).unwrap();
        let original = sample;
        for value in [f64::NAN, f64::INFINITY, -1.0, 1e12 + 1.0] {
            assert!(sample.add(CameraPath::Other, LightPath::Caustic, [value; 3]).is_err());
            assert_eq!(sample, original);
        }
        let mut moments = RadianceMoments::default();
        moments.observe([1.0; 3]).unwrap();
        let original = moments;
        assert!(moments.observe([f64::NAN; 3]).is_err());
        assert_eq!(moments, original);
        assert!(UncertaintyPixel { variance_of_mean: [-1.0; 3], count: 2 }.validate().is_err());
    }
}
