//! Optional bounded camera statistics. Transport supplies complete observations;
//! this owner performs accumulation and exposes display uncertainty and reports.
use crate::job::{CancelToken, ErrorCode, JobError, Result};
use crate::radiance::{COMPONENT_COUNT, CameraPath, ComponentMoments, LightPath, RadianceComponents, RadianceMoments, UncertaintyPixel};
use serde::Serialize;

fn invalid(message: &str) -> JobError {
    JobError::new(ErrorCode::InvalidRequest, message)
}
fn reserve<T>(count: usize) -> Result<Vec<T>> {
    let mut values = Vec::new();
    values.try_reserve_exact(count).map_err(|_| JobError::new(ErrorCode::AllocationFailed, "radiance statistics allocation failed"))?;
    if values.capacity() != count {
        return Err(JobError::new(ErrorCode::LimitExceeded, "radiance statistics capacity exceeds admission"));
    }
    Ok(values)
}

/// A small report, retained after the per-pixel films are released.
#[derive(Clone, Debug, Serialize)]
pub struct RadianceReport {
    pub pixels: u64,
    pub observations: u64,
    pub minimum_samples: u64,
    pub maximum_samples: u64,
    pub mean_radiance_sum: [f64; 3],
    #[serde(skip_serializing_if = "Option::is_none")]
    pub component_radiance_sum: Option<[[f64; 3]; COMPONENT_COUNT]>,
    pub maximum_recombination_error: f64,
    /// This label deliberately makes no calibrated-convergence claim.
    pub uncertainty: &'static str,
}

/// A film is absent unless the request explicitly collects statistics.
pub struct RadianceFilm {
    pixels: usize,
    beauty: Vec<RadianceMoments>,
    components: Vec<ComponentMoments>,
    maximum_observation_recombination_error: f64,
}
impl RadianceFilm {
    pub fn required_bytes(width: u32, height: u32, components: bool, enabled: bool) -> Result<u64> {
        if !enabled {
            return Ok(0);
        }
        let pixels = u64::from(width)
            .checked_mul(u64::from(height))
            .filter(|pixels| *pixels > 0)
            .ok_or_else(|| invalid("radiance statistics need a nonempty bounded image"))?;
        let moments = if components { ComponentMoments::BYTES } else { RadianceMoments::BYTES };
        pixels
            .checked_mul(moments + UncertaintyPixel::BYTES)
            .ok_or_else(|| JobError::new(ErrorCode::LimitExceeded, "radiance statistics byte count overflow"))
    }
    pub fn new(width: u32, height: u32, components: bool, enabled: bool, cancel: &CancelToken) -> Result<Self> {
        Self::required_bytes(width, height, components, enabled)?;
        if !enabled {
            return Ok(Self { pixels: 0, beauty: Vec::new(), components: Vec::new(), maximum_observation_recombination_error: 0.0 });
        }
        let pixels = usize::try_from(u64::from(width) * u64::from(height)).map_err(|_| invalid("radiance statistics exceed address space"))?;
        let mut film = Self { pixels, beauty: Vec::new(), components: Vec::new(), maximum_observation_recombination_error: 0.0 };
        if components {
            film.components = reserve(pixels)?;
        } else {
            film.beauty = reserve(pixels)?;
        }
        for index in 0..pixels {
            if index % 4096 == 0 {
                cancel.check()?;
            }
            if components {
                film.components.push(ComponentMoments::default());
            } else {
                film.beauty.push(RadianceMoments::default());
            }
        }
        Ok(film)
    }
    pub fn active(&self) -> bool {
        self.pixels > 0
    }
    pub fn observe(&mut self, pixel: usize, value: [f64; 3], components: Option<&RadianceComponents>) -> Result<()> {
        if !self.active() {
            return Ok(());
        }
        if !self.components.is_empty() {
            let sample = components.ok_or_else(|| invalid("camera observation is missing its radiance components"))?;
            let sum = sample.beauty();
            let mut residual = 0.0_f64;
            for (sum, value) in sum.into_iter().zip(value) {
                let difference = (sum - value).abs();
                if !value.is_finite() || difference > 1e-10 * value.abs().max(1.0) {
                    return Err(invalid("scene-linear radiance components do not reproduce the camera observation"));
                }
                residual = residual.max(difference);
            }
            self.components.get_mut(pixel).ok_or_else(|| invalid("component pixel outside the admitted film"))?.observe(sample)?;
            self.maximum_observation_recombination_error = self.maximum_observation_recombination_error.max(residual);
            Ok(())
        } else {
            self.beauty.get_mut(pixel).ok_or_else(|| invalid("statistics pixel outside the admitted film"))?.observe(value)
        }
    }
    pub fn uncertainty(&self, cancel: &CancelToken) -> Result<Vec<UncertaintyPixel>> {
        let mut output = reserve(self.pixels)?;
        for index in 0..self.pixels {
            if index % 4096 == 0 {
                cancel.check()?;
            }
            let value = if let Some(pixel) = self.components.get(index) {
                pixel.beauty_uncertainty()
            } else {
                self.beauty.get(index).ok_or_else(|| invalid("statistics pixel missing"))?.uncertainty()
            };
            value.validate()?;
            output.push(value);
        }
        Ok(output)
    }
    /// Mean linear component, including zero observations. Alpha is supplied by
    /// the beauty film; a component is radiance rather than independent coverage.
    pub fn component(&self, camera: CameraPath, light: LightPath, cancel: &CancelToken) -> Result<Vec<[f32; 3]>> {
        if self.components.len() != self.pixels || !self.active() {
            return Err(invalid("radiance components were not requested"));
        }
        let mut output = reserve(self.pixels)?;
        for (index, pixel) in self.components.iter().enumerate() {
            if index % 4096 == 0 {
                cancel.check()?;
            }
            output.push(pixel.component_mean(camera, light).map(|value| value as f32));
        }
        Ok(output)
    }
    pub fn report(&self, cancel: &CancelToken) -> Result<Option<RadianceReport>> {
        if !self.active() {
            return Ok(None);
        }
        let mut report = RadianceReport {
            pixels: self.pixels as u64,
            observations: 0,
            minimum_samples: u64::MAX,
            maximum_samples: 0,
            mean_radiance_sum: [0.0; 3],
            component_radiance_sum: (!self.components.is_empty()).then_some([[0.0; 3]; COMPONENT_COUNT]),
            maximum_recombination_error: self.maximum_observation_recombination_error,
            uncertainty: "camera-observation scatter; shared photons, changing radii and low-discrepancy dependence are not convergence bounds",
        };
        for index in 0..self.pixels {
            if index % 4096 == 0 {
                cancel.check()?;
            }
            let (count, mean) = if let Some(pixel) = self.components.get(index) {
                let mut sum = [0.0; 3];
                if let Some(sums) = &mut report.component_radiance_sum {
                    for (camera_index, camera) in [CameraPath::Other, CameraPath::Reflected, CameraPath::Transmitted].into_iter().enumerate() {
                        for (light_index, light) in [LightPath::NonCaustic, LightPath::Caustic].into_iter().enumerate() {
                            let mean = pixel.component_mean(camera, light);
                            let target = sums.get_mut(camera_index * 2 + light_index).ok_or_else(|| invalid("component report index"))?;
                            for ((target, sum), value) in target.iter_mut().zip(sum.iter_mut()).zip(mean) {
                                *target += value;
                                *sum += value;
                            }
                        }
                    }
                }
                for (sum, beauty) in sum.into_iter().zip(pixel.beauty_mean()) {
                    report.maximum_recombination_error = report.maximum_recombination_error.max((sum - beauty).abs());
                }
                (pixel.count(), pixel.beauty_mean())
            } else {
                let pixel = self.beauty.get(index).ok_or_else(|| invalid("statistics report pixel missing"))?;
                (pixel.count(), pixel.mean())
            };
            report.observations = report.observations.checked_add(count).ok_or_else(|| invalid("radiance report count overflow"))?;
            report.minimum_samples = report.minimum_samples.min(count);
            report.maximum_samples = report.maximum_samples.max(count);
            for (sum, value) in report.mean_radiance_sum.iter_mut().zip(mean) {
                *sum += value;
            }
        }
        Ok(Some(report))
    }
}

/// Compact display snapshot, independent of mutable camera accumulation.
/// Six scene-linear means and variances share one exact observation count.
#[cfg(feature = "raytrace-cpu")]
#[derive(Clone, Copy)]
struct ComponentPixel {
    means: [[f32; 3]; COMPONENT_COUNT],
    variances: [[f32; 3]; COMPONENT_COUNT],
    count: u32,
}

#[cfg(feature = "raytrace-cpu")]
pub(crate) struct ComponentFrame {
    pixels: Vec<ComponentPixel>,
}
#[cfg(feature = "raytrace-cpu")]
impl ComponentFrame {
    pub(crate) const BYTES_PER_PIXEL: u64 = std::mem::size_of::<ComponentPixel>() as u64;
    pub(crate) fn len(&self) -> usize {
        self.pixels.len()
    }
    pub(crate) fn pixel(&self, index: usize, component: usize) -> Result<([f32; 3], UncertaintyPixel)> {
        let pixel = self.pixels.get(index).ok_or_else(|| invalid("component frame pixel exceeds admission"))?;
        let mean = pixel.means.get(component).copied().ok_or_else(|| invalid("component frame component outside partition"))?;
        let variance_of_mean = pixel.variances.get(component).copied().ok_or_else(|| invalid("component frame uncertainty disappeared"))?;
        Ok((mean, UncertaintyPixel { variance_of_mean, count: pixel.count }))
    }
    pub(crate) fn beauty(&self, index: usize) -> Result<([f64; 3], u32)> {
        let pixel = self.pixels.get(index).ok_or_else(|| invalid("component frame pixel exceeds admission"))?;
        let mut sum = [0.0; 3];
        for mean in pixel.means {
            for (sum, value) in sum.iter_mut().zip(mean) {
                *sum += f64::from(value);
            }
        }
        Ok((sum, pixel.count))
    }
}
#[cfg(feature = "raytrace-cpu")]
impl RadianceFilm {
    pub(crate) fn component_frame(&self, cancel: &CancelToken) -> Result<ComponentFrame> {
        if self.components.len() != self.pixels || !self.active() {
            return Err(invalid("component reconstruction requires captured component statistics"));
        }
        let mut pixels = reserve(self.pixels)?;
        for (index, source) in self.components.iter().enumerate() {
            if index % 4096 == 0 {
                cancel.check()?;
            }
            let count = u32::try_from(source.count()).map_err(|_| invalid("component display observation count overflow"))?;
            let mut pixel = ComponentPixel { means: [[0.0; 3]; COMPONENT_COUNT], variances: [[0.0; 3]; COMPONENT_COUNT], count };
            for (camera_index, camera) in [CameraPath::Other, CameraPath::Reflected, CameraPath::Transmitted].into_iter().enumerate() {
                for (light_index, light) in [LightPath::NonCaustic, LightPath::Caustic].into_iter().enumerate() {
                    let component = camera_index * 2 + light_index;
                    let uncertainty = source.component_uncertainty(camera, light);
                    uncertainty.validate()?;
                    *pixel.means.get_mut(component).ok_or_else(|| invalid("component display partition index"))? =
                        source.component_mean(camera, light).map(|value| value as f32);
                    *pixel.variances.get_mut(component).ok_or_else(|| invalid("component display moment index"))? = uncertainty.variance_of_mean;
                }
            }
            pixels.push(pixel);
        }
        cancel.check()?;
        Ok(ComponentFrame { pixels })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_retains_checked_observation_residual_when_mean_residual_cancels() {
        let cancel = CancelToken::default();
        let mut film = RadianceFilm::new(1, 1, true, true, &cancel).unwrap();
        let mut sample = RadianceComponents::default();
        sample.add(CameraPath::Reflected, LightPath::NonCaustic, [1.0; 3]).unwrap();
        let residual = 2.0_f64.powi(-40);
        for sign in [-1.0, 1.0] {
            film.observe(0, [1.0 + sign * residual, 1.0, 1.0], Some(&sample)).unwrap();
        }
        let report = film.report(&cancel).unwrap().unwrap();
        assert_eq!(report.observations, 2);
        assert_eq!(report.maximum_recombination_error, residual);
        assert!(film.observe(0, [1.001, 1.0, 1.0], Some(&sample)).is_err());
        let after = film.report(&cancel).unwrap().unwrap();
        assert_eq!(after.observations, report.observations);
        assert_eq!(after.maximum_recombination_error, residual, "failed observations cannot update the report");
    }
}
