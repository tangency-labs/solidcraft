//! Nondestructive display intent shared by transport output and retained-master
//! regrading. Raw scene-linear samples are immutable; this is not denoising.
use super::finishing::{Backdrop, Finishing};
use super::{CancelToken, ErrorCode, JobError, JobId, Result, ToneMap};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Presentation {
    pub exposure_ev: f64,
    pub tone_map: ToneMap,
    pub finishing: Finishing,
    pub backdrop: Option<Backdrop>,
    pub background: [u8; 3],
    pub transparent_background: bool,
}
impl Default for Presentation {
    fn default() -> Self {
        Self {
            exposure_ev: 0.0,
            tone_map: ToneMap::default(),
            finishing: Finishing::default(),
            backdrop: None,
            background: [240, 243, 248],
            transparent_background: false,
        }
    }
}
impl Presentation {
    pub fn validate(self) -> Result<()> {
        self.finishing.validate()?;
        if let Some(backdrop) = self.backdrop {
            backdrop.validate()?;
        }
        if !self.exposure_ev.is_finite() || !(-32.0..=32.0).contains(&self.exposure_ev) {
            return Err(JobError::new(ErrorCode::InvalidRequest, "presentation exposure must be -32..32 EV"));
        }
        if self.transparent_background
            && (self.backdrop.is_some() || self.finishing.bloom.is_some_and(|b| b.active()) || self.finishing.photographic.active())
        {
            return Err(JobError::new(ErrorCode::InvalidRequest, "choose opaque presentation for bloom or a camera backdrop"));
        }
        Ok(())
    }
    pub fn display(self, width: u32, height: u32) -> Result<Display> {
        self.validate()?;
        super::linear_exr::encoded_limit(width, height)?;
        Ok(Display {
            width,
            height,
            exposure: 2.0f64.powf(self.exposure_ev),
            tone_map: self.tone_map,
            finishing: self.finishing,
            backdrop: self.backdrop,
            background: self.background.map(crate::surface::srgb8_to_linear),
            transparent: self.transparent_background,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct MasterPresentation {
    pub source_job: JobId,
    pub master_job: JobId,
    pub master_sha256: String,
    pub presentation: Presentation,
    pub denoise_applied: bool,
}

#[derive(Clone, Copy)]
pub struct Display {
    pub finishing: super::finishing::Finishing,
    pub backdrop: Option<super::finishing::Backdrop>,
    pub width: u32,
    pub height: u32,
    pub exposure: f64,
    pub tone_map: ToneMap,
    pub background: [f64; 3],
    pub transparent: bool,
}
impl Display {
    pub fn encode(self, input: &[[f32; 4]], output: &mut [u8], cancel: &CancelToken) -> Result<()> {
        self.encode_at(input, output, cancel, 0)
    }
    pub fn encode_at(self, input: &[[f32; 4]], output: &mut [u8], cancel: &CancelToken, offset: usize) -> Result<()> {
        self.encode_bloom_at(input, output, cancel, offset, None)
    }
    pub fn encode_bloom_at(
        self,
        input: &[[f32; 4]],
        output: &mut [u8],
        cancel: &CancelToken,
        offset: usize,
        bloom: Option<&[[f32; 3]]>,
    ) -> Result<()> {
        if bloom.is_some_and(|pixels| pixels.len() != input.len()) {
            return Err(JobError::new(ErrorCode::InvalidRequest, "bloom/display length mismatch"));
        }
        super::linear_exr::encoded_limit(self.width, self.height)?;
        self.finishing.validate()?;
        if let Some(backdrop) = self.backdrop {
            backdrop.validate()?;
        }
        let count = self.width as usize * self.height as usize;
        if input.len().checked_mul(4) != Some(output.len())
            || offset.checked_add(input.len()).is_none_or(|n| n > count)
            || !self.exposure.is_finite()
            || !(2.0_f64.powi(-32)..=2.0_f64.powi(32)).contains(&self.exposure)
            || self.background.iter().any(|c| !c.is_finite() || !(0.0..=1.0).contains(c))
            || bloom.is_some_and(|g| g.len() != input.len())
        {
            return Err(JobError::new(ErrorCode::InvalidRequest, "invalid presentation dimensions, exposure or storage"));
        }
        if self.transparent && (self.backdrop.is_some() || self.finishing.bloom.is_some_and(|b| b.active()) || self.finishing.photographic.active()) {
            return Err(JobError::new(ErrorCode::InvalidRequest, "bloom and camera backdrops require opaque presentation"));
        }
        for (index, (pixel, rgba)) in input.iter().zip(output.as_chunks_mut::<4>().0).enumerate() {
            if index % 4096 == 0 {
                cancel.check()?;
            }
            let alpha = f64::from(pixel[3]).clamp(0.0, 1.0);
            let absolute = offset + index;
            let position = [
                ((absolute % self.width as usize) as f64 + 0.5) / f64::from(self.width),
                ((absolute / self.width as usize) as f64 + 0.5) / f64::from(self.height),
            ];
            let background = self.backdrop.map_or(self.background, |backdrop| backdrop.color(position));
            let mapped = std::array::from_fn(|channel| {
                let c = if alpha > 0. { f64::from(pixel[channel]) * self.exposure / alpha * self.finishing.rgb_gain[channel] } else { 0. };
                self.tone_map.map(c)
            });
            let mapped = self.finishing.display(mapped, position);
            for (channel, ((c, b), out)) in mapped.into_iter().zip(background).zip(rgba.iter_mut()).enumerate() {
                let mut value = if self.transparent { c } else { c * alpha + b * (1. - alpha) };
                if let Some(glow) = bloom.and_then(|pixels| pixels.get(index)) {
                    let strength = self.finishing.bloom.map_or(0.0, |b| b.strength);
                    value += strength * self.tone_map.map(f64::from(glow[channel]) * self.exposure * self.finishing.rgb_gain[channel]);
                }
                let encoded = if value <= 0.0031308 { value * 12.92 } else { 1.055 * value.powf(1. / 2.4) - 0.055 };
                *out = (encoded.clamp(0., 1.) * 255.).round() as u8;
            }
            rgba[3] = if self.transparent { (alpha * 255.0).round() as u8 } else { 255 };
        }
        Ok(())
    }
}
