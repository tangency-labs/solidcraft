//! Captured existing HDR source, moved through a job under its ONE work lease.
#[cfg(feature = "raytrace-cpu")]
use super::environment_sampling::Environment;
use serde::Serialize;
use sha2::{Digest, Sha256};

#[derive(Clone, Debug, Serialize)]
pub struct EnvironmentInfo {
    pub source_sha256: [u8; 32],
    pub header: super::HdrInfo,
    pub rotation_radians: f64,
    pub intensity: f64,
    pub samples: u32,
    pub color_policy: EnvironmentColorPolicy,
}
#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EnvironmentColorPolicy {
    ExplicitLinearSrgbD65,
}

#[cfg(feature = "raytrace-cpu")]
use super::{DecodedHdr, HdrDecodeTask, HdrTaskPoll};
use super::{HdrError, HdrErrorCode, HdrSource, Result};

/// Transient captured input, never serialized into status or saved design data.
pub struct EnvironmentInput {
    source: HdrSource,
    info: EnvironmentInfo,
}
impl EnvironmentInput {
    pub fn new(source: HdrSource, rotation: f64, intensity: f64, samples: u32) -> Result<Self> {
        if !rotation.is_finite() || !intensity.is_finite() || !(0.0..=64.0).contains(&intensity) || !matches!(samples, 1 | 4 | 16 | 64) {
            return Err(HdrError::new(HdrErrorCode::InvalidInput, "invalid environment controls"));
        }
        if source.info().primaries != Some([0.64, 0.33, 0.30, 0.60, 0.15, 0.06, 0.3127, 0.3290]) {
            return Err(HdrError::new(HdrErrorCode::InvalidInput, "environment requires explicit sRGB/D65 primaries"));
        }
        let info = EnvironmentInfo {
            source_sha256: Sha256::digest(source.encoded_bytes()).into(),
            header: source.info().clone(),
            rotation_radians: rotation,
            intensity,
            samples,
            color_policy: EnvironmentColorPolicy::ExplicitLinearSrgbD65,
        };
        Ok(Self { source, info })
    }
    pub fn metadata(&self) -> EnvironmentInfo {
        self.info.clone()
    }

    /// Source capacity + exact decoded allocation + scanline + conservative task state.
    /// The host adds this BEFORE reserving; this is no separate memory pool.
    pub fn required_bytes(&self) -> Result<u64> {
        self.required_bytes_with_importance(false)
    }
    /// Include the optional table and its peak builder scratch in the same lease.
    pub fn required_bytes_with_importance(&self, importance: bool) -> Result<u64> {
        let table = if importance {
            super::environment::EnvironmentBuilder::admitted_bytes(self.source.info().width, self.source.info().height)
                .map_err(|e| HdrError::new(HdrErrorCode::LimitExceeded, e.to_string()))?
        } else {
            0
        };
        self.source
            .retained_encoded_bytes()
            .checked_add(self.source.info().decoded_rgb_bytes)
            .and_then(|v| v.checked_add(u64::from(self.source.info().width) * 4))
            .and_then(|v| v.checked_add(8192))
            .and_then(|v| v.checked_add(table))
            .ok_or_else(|| HdrError::new(HdrErrorCode::LimitExceeded, "environment admission overflow"))
    }
    #[cfg(feature = "raytrace-cpu")]
    pub(crate) fn begin(self, importance: bool, power: bool) -> Result<EnvironmentState> {
        if std::mem::size_of::<HdrDecodeTask>() > 4096 || std::mem::size_of::<HdrDecodeTask>() + std::mem::size_of::<EnvironmentState>() > 8192 {
            return Err(HdrError::new(HdrErrorCode::LimitExceeded, "HDR decoder state exceeds admission"));
        }
        let mut decoder = Vec::new();
        decoder.try_reserve_exact(1).map_err(|_| HdrError::new(HdrErrorCode::AllocationFailed, "HDR decoder state allocation failed"))?;
        if decoder.capacity() != 1 {
            return Err(HdrError::new(HdrErrorCode::LimitExceeded, "HDR decoder state capacity exceeds admission"));
        }
        decoder.push(HdrDecodeTask::new(self.source)?);
        Ok(EnvironmentState {
            decoder,
            decoded: None,
            importance_enabled: importance,
            energy_ready: !power,
            energy_cursor: 0,
            energy_sum: 0.0,
            importance_builder: None,
            importance: None,
            rotation: self.info.rotation_radians,
            intensity: self.info.intensity,
            samples: self.info.samples,
        })
    }
}

#[cfg(feature = "raytrace-cpu")]
pub(crate) struct EnvironmentState {
    decoder: Vec<HdrDecodeTask>,
    decoded: Option<DecodedHdr>,
    importance_enabled: bool,
    energy_ready: bool,
    energy_cursor: usize,
    energy_sum: f64,
    importance_builder: Option<super::environment::EnvironmentBuilder>,
    importance: Option<super::environment::PreparedEnvironment>,
    rotation: f64,
    intensity: f64,
    pub samples: u32,
}
#[cfg(feature = "raytrace-cpu")]
impl EnvironmentState {
    pub fn ready(&self) -> bool {
        self.decoded.is_some() && self.energy_ready && (!self.importance_enabled || self.importance.is_some())
    }
    /// Caller debits one shared unit before each invocation; decoder also observes cancellation.
    pub fn prepare_one(&mut self, cancelled: impl Fn() -> bool) -> Result<()> {
        super::check_cancelled(&cancelled)?;
        if let Some(builder) = self.importance_builder.as_mut() {
            let decoded = self.decoded.as_ref().ok_or_else(|| HdrError::new(HdrErrorCode::Consumed, "importance source missing"))?;
            builder.step(decoded.pixels()).map_err(|e| HdrError::new(HdrErrorCode::InvalidInput, e.to_string()))?;
            if builder.ready() {
                self.importance = Some(
                    self.importance_builder
                        .take()
                        .ok_or_else(|| HdrError::new(HdrErrorCode::Consumed, "importance builder missing"))?
                        .finish()
                        .map_err(|e| HdrError::new(HdrErrorCode::InvalidInput, e.to_string()))?,
                );
            }
            if let Some(table) = &self.importance {
                self.energy_sum = table.luminance_integral();
                self.energy_ready = true;
            }
            return Ok(());
        }
        if let Some(decoded) = &self.decoded
            && !self.energy_ready
        {
            let pixel =
                *decoded.pixels().get(self.energy_cursor).ok_or_else(|| HdrError::new(HdrErrorCode::Consumed, "environment power pixel missing"))?;
            self.energy_sum += super::environment::texel_luminance_mass(decoded.info().width, decoded.info().height, self.energy_cursor, pixel);
            self.energy_cursor += 1;
            self.energy_ready = self.energy_cursor == decoded.pixels().len();
            return Ok(());
        }
        let decoder = self.decoder.first_mut().ok_or_else(|| HdrError::new(HdrErrorCode::Consumed, "environment decoder consumed"))?;
        if let HdrTaskPoll::Complete(decoded) = decoder.step(1, cancelled)? {
            let expected = decoded.info().decoded_rgb_bytes;
            if decoded.retained_pixel_bytes()? != expected {
                return Err(HdrError::new(HdrErrorCode::LimitExceeded, "environment decoded capacity exceeds admission"));
            }
            Environment::from_decoder_output(&decoded, self.rotation, self.intensity)?.require_linear_srgb()?;
            if self.importance_enabled {
                self.importance_builder = Some(
                    super::environment::EnvironmentBuilder::new(decoded.info().width, decoded.info().height)
                        .map_err(|e| HdrError::new(HdrErrorCode::AllocationFailed, e.to_string()))?,
                );
            }
            self.decoded = Some(decoded);
            self.decoder = Vec::new();
        }
        Ok(())
    }
    pub fn luminance_integral(&self) -> Result<f64> {
        if !self.ready() {
            return Err(HdrError::new(HdrErrorCode::Consumed, "environment power not ready"));
        }
        Ok(self.energy_sum * self.intensity)
    }
    pub fn view(&self) -> Result<Environment<'_>> {
        let decoded = self.decoded.as_ref().ok_or_else(|| HdrError::new(HdrErrorCode::Consumed, "environment not ready"))?;
        if !self.ready() {
            return Err(HdrError::new(HdrErrorCode::Consumed, "environment importance not ready"));
        }
        Environment::from_decoder_output(decoded, self.rotation, self.intensity)?.with_importance(self.importance.as_ref())
    }
}
