//! Product-image work and its device-independent, session-scoped protocol.
//! Hosts own execution; documents and history never contain these values.

use solidcraft_geom::prelude::*;
use std::fmt;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use serde::{Deserialize, Serialize};

use crate::{BackendId, Camera, ExecutorId};

pub mod adaptive;
pub mod aging;
pub mod bloom;
pub mod caustics;
mod cpu_budget;
mod cpu_path;
mod cpu_sky;
#[cfg(feature = "raytrace-cpu")]
pub(crate) mod denoise;
pub(crate) mod encoding;
pub mod finishing;
mod host;
pub(crate) mod image;
pub mod led_strip;
mod lens;
#[cfg(test)]
mod light_union_tests;
mod lighting;
pub mod linear_exr;
mod linear_exr_decode;
pub mod looks;
#[cfg(feature = "raytrace-cpu")]
pub(crate) mod output;
pub mod photographic;
mod png;
pub mod presentation;
mod punctual;
mod regrade;
#[cfg(test)]
mod rt2_tests;
pub mod textures;
pub mod volume;

pub use caustics::CausticOptions;
pub use cpu_budget::{CpuBudget, CpuBudgetLimits, MAX_CPU_BODIES, MAX_CPU_SAMPLES_PER_PIXEL, MAX_CPU_TRIANGLES, MAX_CPU_WORK, NATIVE_CPU_POOL_BYTES};
pub use cpu_path::CpuPathOptions;
pub use cpu_sky::CpuSky;
#[cfg(feature = "raytrace-cpu")]
pub(crate) use host::FRAME_METADATA_BYTES;
pub(crate) use host::Reservation;
pub use host::{
    CaptureLease, CaptureProfile, CpuHost, HostKind, ImageHost, JobEnvironmentInfo, JobHandle, JobStatus, LinearAlphaConvention,
    LinearColourEncoding, LinearMasterInfo, PreparedSubmission, PreviewInfo, RenderFrame, RenderPreview, RetainedMetadata,
};
pub use image::{
    Admission, BodyRole, ImageTask, MAX_TILES, OrthoFrame, PicturePoll, PictureRequest, PictureTask, PixelWindow, Projection, Supersample, TaskPoll,
    admit, admit_geometry, admit_geometry_with_limits, admit_picture,
};
pub use lens::ThinLens;
pub use lighting::{AreaLight, AreaLights, DirectionalLights, MAX_AREA_LIGHTS, MAX_DIRECTIONAL_LIGHTS, StudioLighting, Sun, ToneMap};
pub use png::PngTask;
pub use presentation::OutputOptions;
pub use punctual::{MAX_PUNCTUAL_LIGHTS, MIN_GPU_SPOT_COSINE_GAP, PunctualDiagnostics, PunctualLightOptions, PunctualLights, SpotConeOptions};

/// Native path-tracing ceilings, further restricted by the actual device.
/// Kept in the device-independent layer so saved intent and UI agree with execution.
pub const GPU_PATH_LIMITS: crate::RenderLimits =
    crate::RenderLimits { max_dimension: 8192, max_pixels: 16_777_216, max_scratch_bytes: 8 * 1024 * 1024 * 1024 };
pub const MAX_GPU_PATH_TRIANGLES: u64 = 8_000_000;
pub const MAX_GPU_PATH_BODIES: usize = 65_536;
pub const MAX_GPU_DIELECTRICS: usize = 32;

/// Runtime sampling intent, separate from saved documents and device resources.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct JobOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub caustics: Option<CausticOptions>,
    #[serde(skip_serializing_if = "aging::AgingStack::is_empty")]
    pub aging: aging::AgingStack,
    pub nested_media: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub volume: Option<volume::HomogeneousVolume>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub adaptive: Option<adaptive::AdaptiveSampling>,
    /// Conditional Fresnel strata per primary sample; one keeps the old stream.
    pub transmission_samples: u32,

    #[serde(default, skip_serializing_if = "finishing::Finishing::is_identity")]
    pub finishing: finishing::Finishing,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backdrop: Option<finishing::Backdrop>,
    #[serde(default)]
    pub linear_exr: bool,
    pub samples: u32,
    pub max_depth: u32,
    pub seed: u32,
    pub exposure_ev: f64,
    pub lens: Option<ThinLens>,
    pub lighting: Option<StudioLighting>,
    pub tone_map: ToneMap,
    /// Optional final spatial filter. Raw progressive accumulation remains unchanged.
    pub denoise: bool,
}

impl Default for JobOptions {
    fn default() -> Self {
        Self {
            caustics: None,
            aging: Default::default(),
            nested_media: false,
            volume: None,
            adaptive: None,
            transmission_samples: 1,
            finishing: Default::default(),
            backdrop: None,
            linear_exr: false,
            samples: 64,
            max_depth: 8,
            seed: 0,
            exposure_ev: 0.0,
            lens: None,
            lighting: None,
            tone_map: ToneMap::default(),
            denoise: false,
        }
    }
}

impl JobOptions {
    /// Equality alone equates -0 and +0. Captured native option bits must survive
    /// work release, so the allocation-free sentinel also checks default-zero fields.
    pub fn is_structural_default(&self) -> bool {
        let photo = self.finishing.photographic;
        *self == Self::default()
            && [
                self.exposure_ev,
                self.finishing.vignette,
                photo.flare.strength,
                photo.chromatic_pixels,
                photo.radial_distortion,
                photo.soft_focus,
                photo.sharpening,
                photo.grain,
            ]
            .iter()
            .all(|value| value.to_bits() == 0)
    }

    /// Fixed native options survive work release through job/status handles.
    /// Only structural defaults use the allocation-free representation: authored
    /// Some(identity) lighting/effects remain captured and independently prepaid.
    pub fn retained_bytes(&self) -> Result<u64> {
        self.validate()?;
        if self.is_structural_default() {
            return Ok(0);
        }
        // The type contains only bounded inline values. The caller adds this
        // exact allocation to the existing retained-request/job/host pool caps.
        Ok(std::mem::size_of::<Self>() as u64)
    }

    /// Absolute complete-pass target, including independent pilots when adaptive.
    /// This is not a claim that every adaptive production pixel takes every pass.
    pub fn preview_target(self) -> Result<u32> {
        self.validate()?;
        self.samples
            .checked_add(self.adaptive.map_or(0, |adaptive| adaptive.pilot_samples))
            .ok_or_else(|| JobError::new(ErrorCode::LimitExceeded, "sampling pass target overflow"))
    }
    pub fn validate(&self) -> Result<()> {
        if let Some(caustics) = self.caustics {
            caustics.validate()?;
        }
        self.aging.validate()?;
        if let Some(volume) = self.volume {
            volume.validate()?;
            if volume.active() && self.transmission_samples != 1 {
                return Err(JobError::new(
                    ErrorCode::InvalidRequest,
                    "volume transport requires one transmission stratum; glass combinations are not supported",
                ));
            }
            if volume.active() && self.denoise {
                return Err(JobError::new(
                    ErrorCode::InvalidRequest,
                    "volume scattering has no surface-denoiser guides; disable denoising explicitly",
                ));
            }
        }
        if let Some(adaptive) = self.adaptive {
            adaptive.validate(self.samples)?;
        }
        if !matches!(self.transmission_samples, 1 | 2 | 4) {
            return Err(JobError::new(ErrorCode::InvalidRequest, "transmission_samples must be 1, 2 or 4"));
        }
        self.finishing.validate()?;
        if let Some(backdrop) = self.backdrop {
            backdrop.validate()?;
        }
        if let Some(lighting) = self.lighting {
            lighting.validate()?;
        }
        if let Some(lens) = self.lens {
            lens.validate()?;
        }
        if !(1..=4096).contains(&self.samples)
            || !(1..=16).contains(&self.max_depth)
            || !self.exposure_ev.is_finite()
            || !(-32.0..=32.0).contains(&self.exposure_ev)
        {
            return Err(JobError::new(ErrorCode::InvalidRequest, "samples must be 1..4096, depth 1..16 and exposure -32..32 EV"));
        }
        Ok(())
    }
}

/// Provisional PNG from a complete pass, moved to the host for bounded publication.
/// It never enters the final-image or linear-master slot.
pub struct ImageWorkPreview {
    pub png: Vec<u8>,
    /// Uniform SPP, or the absolute pilot+production iteration for adaptive work.
    /// The host publishes phase metadata so display clients never label it uniform SPP.
    pub samples_per_pixel: u32,
}

/// One bounded quantum of image work. Implementations check cancellation between quanta.
pub trait ImageWork: Send {
    /// Cumulative omissions seen through the most recent complete image pass.
    /// Adaptive jobs include pilot paths and every production Fresnel stratum.
    fn punctual_diagnostics(&self) -> Option<PunctualDiagnostics> {
        None
    }

    /// Called after each step. The host hashes and publishes this separately from final output.
    fn take_preview(&mut self) -> Option<ImageWorkPreview> {
        None
    }
    fn sampling_report(&self) -> Option<adaptive::SamplingReport> {
        None
    }
    /// Optional raw linear companion, moved exactly once after PNG completion.
    /// The host validates intent, bounds, hashes and retains its memory lease.
    fn take_linear_exr(&mut self) -> Option<Vec<u8>> {
        None
    }
    fn step(&mut self, units: usize) -> Result<TaskPoll>;
}
impl ImageWork for ImageTask {
    fn step(&mut self, units: usize) -> Result<TaskPoll> {
        self.step(units)
    }
}

/// Device-independent executor boundary. The host owns admission leases and publication;
/// an executor owns computation only. GPU implementations live above the render crate.
pub trait ImageExecutor: Send + Sync {
    fn supports_glass_models(&self) -> bool {
        false
    }
    fn supports_caustics(&self) -> bool {
        false
    }
    /// Pure metadata: complete passes this request can publish (including adaptive pilots).
    /// The host prepays at most two immutable PNG snapshots in addition to executor work.
    fn preview_samples(&self, _options: JobOptions) -> Option<u32> {
        None
    }
    fn preview_unavailable_reason(&self, _options: JobOptions) -> Option<&'static str> {
        None
    }
    fn supports_aging(&self) -> bool {
        false
    }
    fn supports_photographic(&self) -> bool {
        false
    }
    fn supports_subsurface(&self) -> bool {
        false
    }
    fn supports_nested_media(&self) -> bool {
        false
    }
    fn supports_volume(&self) -> bool {
        false
    }
    fn supports_adaptive_sampling(&self) -> bool {
        false
    }
    fn supports_transmission_sampling(&self) -> bool {
        false
    }
    fn supports_bloom(&self) -> bool {
        false
    }
    fn supports_finishing(&self) -> bool {
        false
    }
    fn supports_linear_exr(&self) -> bool {
        false
    }
    /// Host-owned logical allocation pool. This is a ceiling, not eager allocation.
    /// Registration snapshots and validates it; unknown executors retain 512 MiB.
    fn memory_limit_bytes(&self) -> u64 {
        512 * 1024 * 1024
    }
    fn descriptor(&self) -> Result<crate::ExecutorDescriptor>;
    fn admit(&self, scene: &crate::ProductScene, request: &ImageRequest, options: JobOptions) -> Result<Admission>;
    fn begin(&self, scene: crate::ProductScene, request: ImageRequest, options: JobOptions, cancel: CancelToken) -> Result<Box<dyn ImageWork>>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JobId {
    pub session: u64,
    pub serial: u64,
    pub generation: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Queued,
    Preparing,
    Running,
    Encoding,
    Cancelling,
    Cancelled,
    Completed,
    Failed,
}

impl Phase {
    pub fn terminal(self) -> bool {
        matches!(self, Self::Cancelled | Self::Completed | Self::Failed)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct Progress {
    pub phase: Phase,
    pub done: u64,
    pub total: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    InvalidRequest,
    LimitExceeded,
    AllocationFailed,
    HostUnavailable,
    QueueFull,
    TimedOut,
    Cancelled,
    InvalidScene,
    EncodingFailed,
    WrongOwner,
    UnknownJob,
    NotCompleted,
    Internal,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct JobError {
    pub code: ErrorCode,
    pub message: String,
}

impl JobError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self { code, message: message.into() }
    }
}

impl fmt::Display for JobError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}: {}", self.code, self.message)
    }
}
impl std::error::Error for JobError {}

pub type Result<T> = std::result::Result<T, JobError>;

#[derive(Clone, Default)]
pub struct CancelToken(Arc<AtomicBool>);

impl CancelToken {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
    pub fn check(&self) -> Result<()> {
        if self.is_cancelled() { Err(JobError::new(ErrorCode::Cancelled, "render cancelled")) } else { Ok(()) }
    }
}

/// Per-request execution choice, independent of viewport graphics presets.
/// Keep both variants parseable in builds without optional raytracing, so those
/// builds return an explicit capability error before capturing model geometry.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Integrator {
    #[default]
    Raster,
    Raytrace,
}

/// Explicit scene-reflection transport, separate from opaque roughness bounds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReflectionMode {
    Mirror,
    RoughSpecular,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReflectionBody {
    pub body: String,
    pub mode: ReflectionMode,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ReflectionOptions {
    #[serde(deserialize_with = "bounded_reflection_bodies")]
    pub bodies: Vec<ReflectionBody>,
    pub max_bounces: u8,
    pub samples: u8,
}
impl Default for ReflectionOptions {
    fn default() -> Self {
        Self { bodies: Vec::new(), max_bounces: 2, samples: 4 }
    }
}
fn bounded_reflection_bodies<'de, D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Vec<ReflectionBody>, D::Error> {
    struct Bodies;
    impl<'de> serde::de::Visitor<'de> for Bodies {
        type Value = Vec<ReflectionBody>;
        fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("at most64 reflection body overrides")
        }
        fn visit_seq<A: serde::de::SeqAccess<'de>>(self, mut seq: A) -> std::result::Result<Self::Value, A::Error> {
            let mut values = Vec::new();
            while let Some(value) = seq.next_element()? {
                if values.len() == 64 {
                    return Err(serde::de::Error::custom("at most64 reflection bodies"));
                }
                values.push(value);
            }
            Ok(values)
        }
    }
    d.deserialize_seq(Bodies)
}
impl ReflectionOptions {
    pub fn validate(&self) -> Result<()> {
        if !(1..=8).contains(&self.max_bounces) || !(1..=64).contains(&self.samples) || self.bodies.len() > 64 || self.bodies.capacity() > 64 {
            return Err(JobError::new(ErrorCode::InvalidRequest, "reflection bounds: 1..8 bounces, 1..64 samples, at most64 body slots"));
        }
        for (index, body) in self.bodies.iter().enumerate() {
            if body.body.is_empty()
                || body.body.len() > 128
                || body.body.capacity() > 128
                || self.bodies.iter().take(index).any(|other| other.body == body.body)
            {
                return Err(JobError::new(ErrorCode::InvalidRequest, "reflection body names must be unique, nonempty and bounded to128 bytes"));
            }
        }
        Ok(())
    }
}

/// Linear-light runtime inputs. These are never design settings or GPU presets.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DirectionalLightOptions {
    pub direction: [f64; 3],
    pub color: [f64; 3],
    pub intensity: f64,
}

impl DirectionalLightOptions {
    /// Direction points towards the source. Intensity is a relative incident-light
    /// scale, as in the CPU direct-light contract; this is not a finite Sun disk.
    pub fn validate(&self) -> Result<()> {
        let direction_valid = self.direction.iter().all(|n| n.is_finite() && n.abs() <= 1e9) && self.direction.iter().any(|n| *n != 0.0);
        if !direction_valid
            || !self.color.iter().all(|n| n.is_finite() && (0.0..=1.0).contains(n))
            || !self.intensity.is_finite()
            || !(0.0..=64.0).contains(&self.intensity)
        {
            return Err(JobError::new(ErrorCode::InvalidRequest, "invalid bounded directional light"));
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LightingPreset {
    Neutral,
    WarmCool,
    KeyRim,
}

impl LightingPreset {
    /// Portable source values shared by CPU transport and light inspection.
    pub fn directional_lights(self) -> Vec<DirectionalLightOptions> {
        let light = |direction, color, intensity| DirectionalLightOptions { direction, color, intensity };
        match self {
            Self::Neutral => crate::studio::LIGHTS.into_iter().map(|(d, intensity)| light([d.x, d.y, d.z], [1.0; 3], intensity)).collect(),
            Self::WarmCool => vec![
                light([0.6, 0.0, 0.8], [1.0, 0.72, 0.48], 3.0),
                light([-0.8, 0.0, 0.6], [0.45, 0.65, 1.0], 1.0),
                light([0.0, -0.6, 0.8], [1.0; 3], 1.5),
            ],
            Self::KeyRim => vec![light([0.6, -0.4, 0.8], [1.0; 3], 3.0), light([-0.6, 0.4, 0.8], [0.6, 0.8, 1.0], 2.0)],
        }
    }
}

/// Full world-space side vectors; U cross V is the emitting side.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RectangleLightOptions {
    pub center_mm: [f64; 3],
    pub edge_u_mm: [f64; 3],
    pub edge_v_mm: [f64; 3],
    pub color: [f64; 3],
    pub intensity: f64,
}

/// Explicit visual rotation about a certified planar face's outward normal.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnisotropicOrientation {
    #[default]
    Zero,
    QuarterTurn,
}

/// Runtime-only captured planar face selector; no persistent naming is invented.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnisotropicFaceOptions {
    pub body_index: usize,
    #[serde(deserialize_with = "bounded_anisotropic_body_name")]
    pub body_name: String,
    pub face: u32,
    pub tangent: [f64; 3],
    pub orientation: AnisotropicOrientation,
    pub roughness_x: f64,
    pub roughness_y: f64,
}

fn bounded_anisotropic_body_name<'de, D: serde::Deserializer<'de>>(d: D) -> std::result::Result<String, D::Error> {
    struct Name;
    impl serde::de::Visitor<'_> for Name {
        type Value = String;
        fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("captured body name of 1..1024 bytes")
        }
        fn visit_str<E: serde::de::Error>(self, value: &str) -> std::result::Result<String, E> {
            if value.is_empty() || value.len() > 1024 {
                return Err(E::custom("captured body name of 1..1024 bytes required"));
            }
            let mut name = String::new();
            name.try_reserve_exact(value.len()).map_err(E::custom)?;
            name.push_str(value);
            Ok(name)
        }
    }
    d.deserialize_str(Name)
}

/// Emission is relative linear-sRGB radiance, independent of reflected color.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EmissionSides {
    #[default]
    Front,
    Both,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SurfaceEmission {
    pub body_index: usize,
    #[serde(deserialize_with = "bounded_emission_body")]
    pub body: String,
    pub color: [f64; 3],
    pub strength: f64,
    #[serde(default)]
    pub sides: EmissionSides,
}

fn bounded_emission_body<'de, D: serde::Deserializer<'de>>(d: D) -> std::result::Result<String, D::Error> {
    struct Name;
    impl<'de> serde::de::Visitor<'de> for Name {
        type Value = String;
        fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("a nonempty captured body name of at most 256 bytes")
        }
        fn visit_str<E: serde::de::Error>(self, value: &str) -> std::result::Result<String, E> {
            if value.is_empty() || value.len() > 256 || value.chars().any(char::is_control) {
                return Err(E::custom("invalid surface emission body name"));
            }
            let mut name = String::new();
            name.try_reserve_exact(value.len()).map_err(E::custom)?;
            name.push_str(value);
            Ok(name)
        }
    }
    d.deserialize_str(Name)
}

impl SurfaceEmission {
    pub fn validate(&self) -> Result<()> {
        if self.body.is_empty()
            || self.body.len() > 256
            || self.body.capacity() > 256
            || self.body.chars().any(char::is_control)
            || !self.color.iter().all(|v| v.is_finite() && (0.0..=1.0).contains(v))
            || !self.strength.is_finite()
            || !(0.0..=64.0).contains(&self.strength)
        {
            return Err(JobError::new(ErrorCode::InvalidRequest, "invalid bounded surface emission"));
        }
        Ok(())
    }
}

fn bounded_emissions<'de, D: serde::Deserializer<'de>>(deserializer: D) -> std::result::Result<Option<Vec<SurfaceEmission>>, D::Error> {
    struct Emitters;
    impl<'de> serde::de::Visitor<'de> for Emitters {
        type Value = Vec<SurfaceEmission>;
        fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("at most 32 surface emitters")
        }
        fn visit_seq<A: serde::de::SeqAccess<'de>>(self, mut seq: A) -> std::result::Result<Self::Value, A::Error> {
            let mut values = Vec::new();
            while let Some(value) = seq.next_element::<SurfaceEmission>()? {
                if values.len() == 32 {
                    return Err(serde::de::Error::custom("at most 32 surface emitters"));
                }
                value.validate().map_err(serde::de::Error::custom)?;
                values.try_reserve_exact(1).map_err(serde::de::Error::custom)?;
                values.push(value);
            }
            Ok(values)
        }
    }
    deserializer.deserialize_seq(Emitters).map(Some)
}

/// Explicit runtime optics; never inferred from opacity or saved into a design.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DielectricBoundary {
    ThinSheet,
    ClosedBox,
    /// Exact captured closed-mesh certification is available only in CPU Path.
    ClosedMesh,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DielectricOptions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<crate::glass_model::GlassModel>,
    /// Homogeneous elastic scattering inside a certified closed Path medium.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subsurface: Option<crate::transmission::subsurface::Subsurface>,
    /// Straight thin-sheet light visibility. Closed refractive connections refuse.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub shadow_transmission: bool,
    pub body_index: usize,
    #[serde(deserialize_with = "bounded_dielectric_body_name")]
    pub body_name: String,
    pub boundary: DielectricBoundary,
    pub ior: f64,
    /// Per-channel Beer-Lambert extinction, inverse millimetres.
    pub absorption_per_mm: [f64; 3],
    /// Virtual normal thickness for ThinSheet only; ClosedBox requires zero.
    pub thickness_mm: f64,
}

/// Explicit constant secondary-ray environment; independent of display background.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RaytraceEnvironmentOptions {
    pub color: [f64; 3],
    pub intensity: f64,
}

fn bounded_dielectric_body_name<'de, D: serde::Deserializer<'de>>(deserializer: D) -> std::result::Result<String, D::Error> {
    struct Name;
    impl serde::de::Visitor<'_> for Name {
        type Value = String;
        fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("captured body name of 1..1024 bytes")
        }
        fn visit_str<E: serde::de::Error>(self, value: &str) -> std::result::Result<String, E> {
            if value.is_empty() || value.len() > 1024 {
                return Err(E::custom("captured body name of 1..1024 bytes required"));
            }
            let mut name = String::new();
            name.try_reserve_exact(value.len()).map_err(E::custom)?;
            name.push_str(value);
            Ok(name)
        }
    }
    deserializer.deserialize_str(Name)
}

fn bounded_dielectrics<'de, D: serde::Deserializer<'de>>(deserializer: D) -> std::result::Result<Vec<DielectricOptions>, D::Error> {
    struct Inputs;
    impl<'de> serde::de::Visitor<'de> for Inputs {
        type Value = Vec<DielectricOptions>;
        fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("at most eight dielectric overrides")
        }
        fn visit_seq<A: serde::de::SeqAccess<'de>>(self, mut sequence: A) -> std::result::Result<Self::Value, A::Error> {
            let mut inputs = Vec::new();
            inputs.try_reserve_exact(8).map_err(serde::de::Error::custom)?;
            while let Some(input) = sequence.next_element()? {
                if inputs.len() == 8 {
                    return Err(serde::de::Error::custom("at most eight dielectric overrides"));
                }
                inputs.push(input);
            }
            Ok(inputs)
        }
    }
    deserializer.deserialize_seq(Inputs)
}

/// CPU camera sampling, independent of the registered GPU's `progressive` wire flag.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CpuProgressiveOptions {
    pub samples_per_pixel: u32,
    pub seed: u32,
}

impl Default for CpuProgressiveOptions {
    fn default() -> Self {
        Self { samples_per_pixel: 16, seed: 0 }
    }
}

impl CpuProgressiveOptions {
    pub const MAX_PIXELS: u64 = 1_048_576;
    pub const MAX_CAMERA_SAMPLES: u64 = 16_777_216;

    pub fn validate(&self) -> Result<()> {
        if !(1..=64).contains(&self.samples_per_pixel) {
            return Err(JobError::new(ErrorCode::InvalidRequest, "CPU progressive samples_per_pixel must be 1..64"));
        }
        Ok(())
    }

    /// Compatibility entry point: a caller that supplies no profile retains Compact.
    pub fn validate_image(&self, width: u32, height: u32) -> Result<()> {
        self.validate_image_with_budget(width, height, CpuBudget::Compact)
    }

    pub fn validate_image_with_budget(&self, width: u32, height: u32, budget: CpuBudget) -> Result<()> {
        budget.validate_image(width, height, Some(*self))
    }

    /// At most two immutable nonfinal snapshots are admitted, independent of SPP.
    #[cfg(feature = "raytrace-cpu")]
    pub(crate) fn preview_slots(&self) -> u32 {
        self.samples_per_pixel.saturating_sub(1).min(2)
    }
}

/// One portable CPU camera envelope, shared by runtime, saved jobs and UI checks.
/// Pilots and maximum production both consume the existing profile sample caps.
pub fn cpu_camera_sampling(
    progressive: Option<CpuProgressiveOptions>,
    adaptive: Option<adaptive::AdaptiveSampling>,
    budget: CpuBudget,
) -> Result<Option<CpuProgressiveOptions>> {
    let Some(adaptive) = adaptive else { return Ok(progressive) };
    let progressive =
        progressive.ok_or_else(|| JobError::new(ErrorCode::InvalidRequest, "CPU adaptive sampling requires progressive camera sampling"))?;
    adaptive.validate(progressive.samples_per_pixel)?;
    let samples_per_pixel = progressive
        .samples_per_pixel
        .checked_add(adaptive.pilot_samples)
        .ok_or_else(|| JobError::new(ErrorCode::LimitExceeded, "CPU adaptive pass target overflow"))?;
    budget.validate_samples(samples_per_pixel)?;
    Ok(Some(CpuProgressiveOptions { samples_per_pixel, ..progressive }))
}

/// The CPU filter shares the bounded float4 presentation envelope; it never
/// enlarges a profile or implies that a linear EXR companion is requested.
pub fn validate_cpu_denoise_image(width: u32, height: u32) -> Result<()> {
    linear_exr::encoded_limit(width, height)
        .map(|_| ())
        .map_err(|_| JobError::new(ErrorCode::InvalidRequest, "CPU denoising requires 1..8192 sides and at most 16777216 pixels"))
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct RaytraceOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub caustics: Option<CausticOptions>,
    /// One bounded world-space participating medium, available with explicit Path.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub volume: Option<volume::HomogeneousVolume>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sun: Option<Box<Sun>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sky: Option<Box<CpuSky>>,
    /// Captured bounded aging parameters; saved stable targets are resolved by the engine.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aging: Option<Box<aging::AgingStack>>,
    /// Independent pilot allocation; the progressive sample count is the production cap.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub adaptive: Option<Box<adaptive::AdaptiveSampling>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<CpuPathOptions>,
    /// Optional CPU spatial output filtering; never changes the raw film or sampling.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub denoise: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lens: Option<ThinLens>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<Box<OutputOptions>>,
    #[serde(skip_serializing_if = "CpuBudget::is_compact")]
    pub budget: CpuBudget,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub progressive: Option<CpuProgressiveOptions>,
    #[serde(deserialize_with = "bounded_dielectrics", skip_serializing_if = "Vec::is_empty")]
    pub dielectrics: Vec<DielectricOptions>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub environment: Option<RaytraceEnvironmentOptions>,
    #[serde(deserialize_with = "crate::surface_patterns::deserialize_effects", skip_serializing_if = "Option::is_none")]
    pub surface_effects: Option<Vec<crate::surface_patterns::SurfaceEffect>>,
    #[serde(deserialize_with = "bounded_emissions", skip_serializing_if = "Option::is_none")]
    pub emissions: Option<Vec<SurfaceEmission>>,
    /// Transient spatial pixel-box samples; independent of reflection attempts.
    #[serde(skip_serializing_if = "single_primary_sample")]
    pub primary_samples: u8,
    pub reflections: Option<ReflectionOptions>,
    pub anisotropic_faces: [Option<AnisotropicFaceOptions>; 2],
    #[serde(deserialize_with = "punctual::bounded_punctual", skip_serializing_if = "Option::is_none")]
    pub punctual_lights: Option<Vec<PunctualLightOptions>>,
    #[serde(deserialize_with = "bounded_area_lights", skip_serializing_if = "Option::is_none")]
    pub area_lights: Option<Vec<RectangleLightOptions>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub area_samples: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seed: Option<u32>,
    pub preset: Option<LightingPreset>,
    #[serde(deserialize_with = "bounded_lights", skip_serializing_if = "Option::is_none")]
    pub lights: Option<Vec<DirectionalLightOptions>>,
    pub exposure_ev: f64,
    pub shadows: bool,
}

impl Default for RaytraceOptions {
    fn default() -> Self {
        Self {
            caustics: None,
            volume: None,
            sun: None,
            sky: None,
            aging: None,
            adaptive: None,
            denoise: false,
            path: None,
            lens: None,
            output: None,
            budget: CpuBudget::Compact,
            progressive: None,
            punctual_lights: None,
            area_lights: None,
            area_samples: None,
            seed: None,
            primary_samples: 1,
            emissions: None,
            surface_effects: None,
            dielectrics: Vec::new(),
            environment: None,
            anisotropic_faces: [None, None],
            reflections: None,
            preset: None,
            lights: None,
            exposure_ev: 0.0,
            shadows: true,
        }
    }
}

fn single_primary_sample(samples: &u8) -> bool {
    *samples == 1
}

fn bounded_lights<'de, D: serde::Deserializer<'de>>(deserializer: D) -> std::result::Result<Option<Vec<DirectionalLightOptions>>, D::Error> {
    struct Lights;
    impl<'de> serde::de::Visitor<'de> for Lights {
        type Value = Vec<DirectionalLightOptions>;
        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("at most eight directional lights")
        }
        fn visit_seq<A: serde::de::SeqAccess<'de>>(self, mut sequence: A) -> std::result::Result<Self::Value, A::Error> {
            let mut lights = Vec::new();
            while let Some(light) = sequence.next_element()? {
                if lights.len() == 8 {
                    return Err(serde::de::Error::custom("at most eight directional lights"));
                }
                lights.push(light);
            }
            Ok(lights)
        }
    }
    deserializer.deserialize_seq(Lights).map(Some)
}

fn bounded_area_lights<'de, D: serde::Deserializer<'de>>(deserializer: D) -> std::result::Result<Option<Vec<RectangleLightOptions>>, D::Error> {
    struct AreaLights;
    impl<'de> serde::de::Visitor<'de> for AreaLights {
        type Value = Vec<RectangleLightOptions>;
        fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("at most four rectangular lights")
        }
        fn visit_seq<A: serde::de::SeqAccess<'de>>(self, mut seq: A) -> std::result::Result<Self::Value, A::Error> {
            let mut lights = Vec::new();
            while let Some(light) = seq.next_element()? {
                if lights.len() == 4 {
                    return Err(serde::de::Error::custom("at most four rectangular lights"));
                }
                lights.push(light);
            }
            Ok(lights)
        }
    }
    struct Optional;
    impl<'de> serde::de::Visitor<'de> for Optional {
        type Value = Option<Vec<RectangleLightOptions>>;
        fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("an optional bounded rectangular-light list")
        }
        fn visit_none<E: serde::de::Error>(self) -> std::result::Result<Self::Value, E> {
            Ok(None)
        }
        fn visit_unit<E: serde::de::Error>(self) -> std::result::Result<Self::Value, E> {
            Ok(None)
        }
        fn visit_some<D: serde::Deserializer<'de>>(self, d: D) -> std::result::Result<Self::Value, D::Error> {
            d.deserialize_seq(AreaLights).map(Some)
        }
    }
    deserializer.deserialize_option(Optional)
}

impl RaytraceOptions {
    /// Dynamic dielectric capacities in one task or immutable request copy.
    pub(crate) fn retained_dielectric_bytes(&self) -> Result<u64> {
        let limit = || JobError::new(ErrorCode::LimitExceeded, "dielectric request capacity overflow");
        let mut bytes = (self.dielectrics.capacity() as u64).checked_mul(std::mem::size_of::<DielectricOptions>() as u64).ok_or_else(limit)?;
        for input in &self.dielectrics {
            bytes = bytes.checked_add(input.body_name.capacity() as u64).ok_or_else(limit)?;
        }
        Ok(bytes)
    }
    /// Dynamic surface-effect capacities in one task or retained request copy.
    pub(crate) fn retained_surface_effect_bytes(&self) -> Result<u64> {
        let limit = || JobError::new(ErrorCode::LimitExceeded, "surface effect capacity overflow");
        let slots = self.surface_effects.as_ref().map_or(0, Vec::capacity) as u64;
        let mut bytes = slots.checked_mul(std::mem::size_of::<crate::surface_patterns::SurfaceEffect>() as u64).ok_or_else(limit)?;
        if let Some(values) = &self.surface_effects {
            for value in values {
                bytes = bytes.checked_add(value.body.capacity() as u64).ok_or_else(limit)?;
            }
        }
        Ok(bytes)
    }
    /// Dynamic emission capacities in one options copy; included in both task and Record admission.
    pub(crate) fn retained_emission_bytes(&self) -> Result<u64> {
        let limit = || JobError::new(ErrorCode::LimitExceeded, "surface emission capacity overflow");
        let slots = self.emissions.as_ref().map_or(0, Vec::capacity) as u64;
        let mut bytes = slots.checked_mul(std::mem::size_of::<SurfaceEmission>() as u64).ok_or_else(limit)?;
        if let Some(values) = &self.emissions {
            for value in values {
                bytes = bytes.checked_add(value.body.capacity() as u64).ok_or_else(limit)?;
            }
        }
        Ok(bytes)
    }
    /// Complete camera-sample envelope: independent pilots plus production cap.
    /// The ordinary path returns the original value without changing its limits.
    pub fn camera_sampling(&self) -> Result<Option<CpuProgressiveOptions>> {
        cpu_camera_sampling(self.progressive, self.adaptive.as_deref().copied(), self.budget)
    }
    pub fn preview_target(&self) -> Result<u32> {
        Ok(self.camera_sampling()?.map_or(0, |options| options.samples_per_pixel))
    }
    pub fn retained_adaptive_bytes(&self) -> Result<u64> {
        if self.adaptive.is_some() {
            self.camera_sampling()?;
            Ok(std::mem::size_of::<adaptive::AdaptiveSampling>() as u64)
        } else {
            Ok(0)
        }
    }

    /// One capacity calculation shared by host and engine preflight.
    pub fn retained_aging_bytes(&self) -> Result<u64> {
        if let Some(stack) = &self.aging {
            stack.validate()?;
            Ok(std::mem::size_of::<aging::AgingStack>() as u64)
        } else {
            Ok(0)
        }
    }
    pub(crate) fn retained_volume_bytes(&self) -> Result<u64> {
        if let Some(volume) = self.volume {
            volume.validate()?;
        }
        // Inline intent has no extra heap owner, including authored zero.
        // Every retained request pays its fixed field through metadata_bytes.
        Ok(0)
    }
    pub(crate) fn retained_celestial_bytes(&self) -> Result<u64> {
        if let Some(sun) = self.sun.as_deref() {
            sun.validate()?;
        }
        if let Some(sky) = self.sky.as_deref() {
            sky.validate()?;
        }
        let sun = if self.sun.is_some() { std::mem::size_of::<Sun>() as u64 } else { 0 };
        let sky = if self.sky.is_some() { std::mem::size_of::<CpuSky>() as u64 } else { 0 };
        Ok(sun + sky)
    }

    pub fn retained_bytes(&self) -> Result<u64> {
        self.validate()?;
        let limit = || JobError::new(ErrorCode::LimitExceeded, "raytrace request capacity overflow");
        let mut bytes = self
            .lights
            .as_ref()
            .map_or(Some(0), |l| (l.capacity() as u64).checked_mul(std::mem::size_of::<DirectionalLightOptions>() as u64))
            .ok_or_else(limit)?;
        for (capacity, size) in [
            (self.area_lights.as_ref().map_or(0, Vec::capacity), std::mem::size_of::<RectangleLightOptions>()),
            (self.punctual_lights.as_ref().map_or(0, Vec::capacity), std::mem::size_of::<PunctualLightOptions>()),
        ] {
            bytes = bytes.checked_add((capacity as u64).checked_mul(size as u64).ok_or_else(limit)?).ok_or_else(limit)?;
        }
        if let Some(reflections) = &self.reflections {
            let slots = (reflections.bodies.capacity() as u64).checked_mul(std::mem::size_of::<ReflectionBody>() as u64).ok_or_else(limit)?;
            bytes = bytes.checked_add(slots).ok_or_else(limit)?;
            for body in &reflections.bodies {
                bytes = bytes.checked_add(body.body.capacity() as u64).ok_or_else(limit)?;
            }
        }
        for target in self.anisotropic_faces.iter().flatten() {
            bytes = bytes.checked_add(target.body_name.capacity() as u64).ok_or_else(limit)?;
        }
        bytes = bytes.checked_add(if self.output.is_some() { std::mem::size_of::<OutputOptions>() as u64 } else { 0 }).ok_or_else(limit)?;
        bytes = bytes.checked_add(self.retained_aging_bytes()?).ok_or_else(limit)?;
        bytes = bytes.checked_add(self.retained_adaptive_bytes()?).ok_or_else(limit)?;
        bytes = bytes.checked_add(self.retained_emission_bytes()?).ok_or_else(limit)?;
        bytes = bytes.checked_add(self.retained_surface_effect_bytes()?).ok_or_else(limit)?;
        bytes = bytes.checked_add(self.retained_dielectric_bytes()?).ok_or_else(limit)?;
        bytes = bytes.checked_add(self.retained_volume_bytes()?).ok_or_else(limit)?;
        bytes = bytes.checked_add(self.retained_celestial_bytes()?).ok_or_else(limit)?;
        Ok(bytes)
    }
    /// No environment NEE on selected reflectors; rough transport must be purely metallic.
    /// Rejecting every nonmetallic effective face prevents silently dropping its diffuse HDR.
    pub fn validate_environment_reflections(&self, scene: &crate::ProductScene) -> Result<()> {
        self.validate()?;
        if self.environment.is_some() || self.sky.is_some() {
            return Err(JobError::new(ErrorCode::InvalidRequest, "choose either the HDR input or constant/Sky environment"));
        }
        if let Some(reflections) = &self.reflections {
            for selector in &reflections.bodies {
                let mut bodies = scene.meshes.iter().filter(|b| b.name == selector.body);
                let body = bodies.next().ok_or_else(|| JobError::new(ErrorCode::InvalidRequest, "environment reflection body missing"))?;
                if bodies.next().is_some() {
                    return Err(JobError::new(ErrorCode::InvalidRequest, "ambiguous environment reflection body"));
                }
                if selector.mode == ReflectionMode::RoughSpecular
                    && self.aging.as_deref().is_some_and(|stack| {
                        stack.iter().any(|layer| {
                            layer.active()
                                && layer.target.is_none_or(|target| target.body_name.as_str() == body.name)
                                && layer.surface_class != aging::SurfaceClass::Dielectric
                                && layer.metallic != 1.0
                        })
                    })
                {
                    return Err(JobError::new(
                        ErrorCode::InvalidRequest,
                        "HDR rough reflection cannot discard the diffuse lobe introduced by CPU aging; keep metallic aging or use opaque path transport explicitly",
                    ));
                }
                if selector.mode == ReflectionMode::RoughSpecular
                    && (body.surface.is_none_or(|s| s.metallic() != 1.0) || body.faces.iter().any(|f| f.surface.is_none_or(|s| s.metallic() != 1.0)))
                {
                    return Err(JobError::new(
                        ErrorCode::InvalidRequest,
                        "HDR rough reflection requires fully metallic body and face surfaces; diffuse/specular partition is not supported",
                    ));
                }
            }
        }
        for target in self.anisotropic_faces.iter().flatten() {
            let mode = self.reflections.as_ref().and_then(|r| r.bodies.iter().find(|b| b.body == target.body_name).map(|b| b.mode));
            if mode != Some(ReflectionMode::RoughSpecular) {
                return Err(JobError::new(
                    ErrorCode::InvalidRequest,
                    "HDR anisotropy requires a selected fully metallic rough reflector; anisotropic HDR direct lighting is not supported",
                ));
            }
        }
        Ok(())
    }
    pub fn validate(&self) -> Result<()> {
        let _ = self.retained_volume_bytes()?;
        if let Some(caustics) = self.caustics {
            caustics.validate()?;
            if self.path.is_none() {
                return Err(JobError::new(ErrorCode::InvalidRequest, "CPU caustics require explicit Indirect lighting"));
            }
        }
        if self.volume.as_ref().is_some_and(|v| v.active()) {
            if self.path.is_none() {
                return Err(JobError::new(ErrorCode::InvalidRequest, "CPU Atmosphere requires explicit Indirect lighting"));
            }
            if self.denoise || !self.dielectrics.is_empty() {
                return Err(JobError::new(
                    ErrorCode::InvalidRequest,
                    "CPU Atmosphere cannot combine with Glass, subsurface scattering or surface Denoise; turn off one explicitly",
                ));
            }
        }
        let _ = self.retained_celestial_bytes()?;
        if (self.sun.is_some() || self.sky.is_some()) && self.path.is_none() {
            return Err(JobError::new(ErrorCode::InvalidRequest, "finite Sun and gradient Sky require explicit CPU Indirect lighting"));
        }
        if self.sky.is_some() && self.environment.is_some() {
            return Err(JobError::new(ErrorCode::InvalidRequest, "choose either gradient Sky or constant CPU environment"));
        }
        if self.lights.as_ref().map_or(3, Vec::len) + usize::from(self.sun.is_some()) > MAX_DIRECTIONAL_LIGHTS {
            return Err(JobError::new(ErrorCode::InvalidRequest, "at most eight CPU directional lights including the finite Sun"));
        }
        if let Some(stack) = &self.aging {
            stack.validate()?;
            if stack.active() && self.anisotropic_faces.iter().any(Option::is_some) {
                return Err(JobError::new(
                    ErrorCode::InvalidRequest,
                    "CPU aging cannot replace the independently authored anisotropic roughness axes; preserve those values and choose an isotropic transport explicitly",
                ));
            }
        }

        if self.adaptive.is_some() {
            self.camera_sampling()?;
        }
        if let Some(path) = self.path {
            path.validate()?;
            cpu_path::validate_emitters(self.area_lights.as_deref().unwrap_or(&[]))?;
            if self.progressive.is_none() || self.primary_samples != 1 {
                return Err(JobError::new(
                    ErrorCode::InvalidRequest,
                    "CPU opaque path transport requires explicit progressive sampling with primary_samples: 1",
                ));
            }
            if !self.shadows {
                return Err(JobError::new(
                    ErrorCode::InvalidRequest,
                    "CPU opaque path transport requires traced shadows for matched light visibility",
                ));
            }
            if self.reflections.is_some() || self.anisotropic_faces.iter().any(Option::is_some) {
                return Err(JobError::new(
                    ErrorCode::InvalidRequest,
                    "CPU path transport cannot combine selected reflection or certified anisotropy overrides; preserve them and choose the legacy transport explicitly",
                ));
            }
        }

        if let Some(lens) = self.lens {
            lens.validate()?;
            if lens.aperture_radius_mm > 0.0 && self.progressive.is_none() {
                return Err(JobError::new(ErrorCode::InvalidRequest, "CPU depth of field requires progressive camera sampling"));
            }
        }
        if let Some(output) = &self.output {
            output.validate()?;
        }
        if let Some(progressive) = self.progressive {
            self.budget.validate_samples(progressive.samples_per_pixel)?;
            if self.primary_samples != 1 || self.reflections.as_ref().is_some_and(|r| !r.bodies.is_empty() && r.samples != 1) {
                return Err(JobError::new(
                    ErrorCode::InvalidRequest,
                    "CPU progressive requires primary_samples: 1 and one reflection attempt per camera sample",
                ));
            }
        }
        if self.environment.is_some_and(|environment| {
            !environment.color.into_iter().all(|v| v.is_finite() && (0.0..=1.0).contains(&v))
                || !environment.intensity.is_finite()
                || !(0.0..=64.0).contains(&environment.intensity)
        }) {
            return Err(JobError::new(ErrorCode::InvalidRequest, "invalid constant raytrace environment"));
        }
        if self.dielectrics.len() > 8 || self.dielectrics.capacity() > 8 {
            return Err(JobError::new(ErrorCode::InvalidRequest, "at most eight dielectric overrides"));
        }
        for (index, input) in self.dielectrics.iter().enumerate() {
            if let Some(model) = input.model {
                model.validate(input.ior).map_err(|e| JobError::new(ErrorCode::InvalidRequest, e.to_string()))?;
                if self.path.is_none() {
                    return Err(JobError::new(ErrorCode::InvalidRequest, "advanced Glass models require CPU Indirect lighting"));
                }
                if input.boundary == DielectricBoundary::ThinSheet && input.shadow_transmission && model.roughness > 0.0 {
                    return Err(JobError::new(ErrorCode::InvalidRequest, "rough thin-sheet Glass requires disabled straight transmitted shadows"));
                }
            }
            if let Some(scattering) = input.subsurface {
                scattering.validate().map_err(|error| JobError::new(ErrorCode::InvalidRequest, error.to_string()))?;
                if self.path.is_none() || input.boundary == DielectricBoundary::ThinSheet {
                    return Err(JobError::new(
                        ErrorCode::InvalidRequest,
                        "CPU subsurface scattering requires Indirect lighting and a certified closed medium",
                    ));
                }
                if scattering.active() && self.denoise {
                    return Err(JobError::new(
                        ErrorCode::InvalidRequest,
                        "CPU subsurface random walks have no surface-denoiser guides; disable denoising explicitly",
                    ));
                }
            }
            if self.path.is_none() && (input.boundary == DielectricBoundary::ClosedMesh || input.shadow_transmission) {
                return Err(JobError::new(
                    ErrorCode::InvalidRequest,
                    "CPU general dielectric boundaries and thin transmitted shadows require explicit Indirect lighting",
                ));
            }
            if input.boundary != DielectricBoundary::ThinSheet && input.shadow_transmission {
                return Err(JobError::new(
                    ErrorCode::InvalidRequest,
                    "CPU closed dielectric light connections are unsupported; preserve the setting or explicitly turn off transmitted shadows",
                ));
            }
            if input.body_name.is_empty()
                || input.body_name.len() > 1024
                || input.body_name.capacity() > 1024
                || !input.ior.is_finite()
                || !(1.0..=3.0).contains(&input.ior)
                || !input.absorption_per_mm.into_iter().all(|v| v.is_finite() && (0.0..=100.0).contains(&v))
                || !input.thickness_mm.is_finite()
                || !(0.0..=10000.0).contains(&input.thickness_mm)
                || (input.boundary != DielectricBoundary::ThinSheet && input.thickness_mm != 0.0)
                || self.dielectrics.iter().take(index).any(|previous| previous.body_index == input.body_index)
            {
                return Err(JobError::new(ErrorCode::InvalidRequest, "invalid or duplicate dielectric override"));
            }
        }
        if let Some(values) = &self.surface_effects {
            if values.capacity() > 32 {
                return Err(JobError::new(ErrorCode::InvalidRequest, "at most 32 surface-effect reserved slots"));
            }
            crate::surface_patterns::validate_effects(values)?;
        }
        if let Some(values) = &self.emissions {
            if values.len() > 32 || values.capacity() > 32 {
                return Err(JobError::new(ErrorCode::InvalidRequest, "at most 32 surface emitters"));
            }
            for (i, value) in values.iter().enumerate() {
                value.validate()?;
                if values.iter().take(i).any(|other| other.body_index == value.body_index) {
                    return Err(JobError::new(ErrorCode::InvalidRequest, "duplicate surface emission target"));
                }
            }
        }
        let area_count = self.area_lights.as_ref().map_or(0, Vec::len);
        if self.area_lights.as_ref().is_some_and(|v| v.len() > 4 || v.capacity() > 4) {
            return Err(JobError::new(ErrorCode::InvalidRequest, "at most four rectangular lights and reserved slots"));
        }
        if area_count == 0 && (self.area_samples.is_some() || self.seed.is_some()) {
            return Err(JobError::new(ErrorCode::InvalidRequest, "area_samples and seed require nonempty area_lights"));
        }
        if self.area_samples.is_some_and(|n| !matches!(n, 1 | 4 | 16 | 64)) {
            return Err(JobError::new(ErrorCode::InvalidRequest, "area_samples must be 1, 4, 16 or 64"));
        }
        let directional_count = self.lights.as_ref().map_or_else(
            || {
                if self.preset == Some(LightingPreset::KeyRim) { 2 } else { 3 }
            },
            Vec::len,
        );
        if directional_count
            .checked_add(area_count)
            .and_then(|n| n.checked_add(self.punctual_lights.as_ref().map_or(0, Vec::len)))
            .is_none_or(|n| n > 20)
        {
            return Err(JobError::new(ErrorCode::InvalidRequest, "at most twenty combined directional, punctual and rectangular lights"));
        }
        if let Some(lights) = &self.area_lights {
            for light in lights {
                use solidcore_record::Vec3;
                let vec = |v: [f64; 3]| Vec3::new(v[0], v[1], v[2]);
                let c = vec(light.center_mm);
                let u = vec(light.edge_u_mm);
                let v = vec(light.edge_v_mm);
                if !c.is_finite()
                    || !u.is_finite()
                    || !v.is_finite()
                    || !light.color.iter().all(|x| x.is_finite() && (0.0..=1.0).contains(x))
                    || !light.intensity.is_finite()
                    || !(0.0..=64.0).contains(&light.intensity)
                    || !(1e-6..=1e6).contains(&u.len())
                    || !(1e-6..=1e6).contains(&v.len())
                    || (u.dot(v) / (u.len() * v.len())).abs() > 1e-10
                {
                    return Err(JobError::new(ErrorCode::InvalidRequest, "invalid finite rectangular light"));
                }
                for a in [-0.5, 0.5] {
                    for b in [-0.5, 0.5] {
                        let p = c + u * a + v * b;
                        if !p.is_finite() || [p.x, p.y, p.z].into_iter().any(|x| x.abs() > 1e9) {
                            return Err(JobError::new(ErrorCode::InvalidRequest, "rectangular light corner exceeds world bounds"));
                        }
                    }
                }
            }
        }
        if let Some(lights) = &self.punctual_lights {
            if lights.len() > 8 || lights.capacity() > 8 {
                return Err(JobError::new(ErrorCode::InvalidRequest, "at most eight point/spot sources and reserved slots"));
            }
            for light in lights {
                light.validate()?;
            }
        }
        if !matches!(self.primary_samples, 1 | 4 | 16) {
            return Err(JobError::new(ErrorCode::InvalidRequest, "primary samples must be exactly 1, 4 or 16"));
        }

        if let Some(reflections) = &self.reflections {
            reflections.validate()?;
        }
        for (i, target) in self.anisotropic_faces.iter().enumerate().filter_map(|(i, target)| target.as_ref().map(|target| (i, target))) {
            if target.body_name.is_empty()
                || target.body_name.len() > 1024
                || target.body_name.capacity() > 1024
                || !target.tangent.iter().all(|v| v.is_finite())
                || !target.tangent.iter().any(|v| *v != 0.0)
                || ![target.roughness_x, target.roughness_y].into_iter().all(|v| v.is_finite() && (crate::surface::MIN_ROUGHNESS..=1.0).contains(&v))
            {
                return Err(JobError::new(ErrorCode::InvalidRequest, "invalid bounded anisotropic planar face options"));
            }
            if self.anisotropic_faces.iter().skip(i + 1).flatten().any(|other| target.body_index == other.body_index && target.face == other.face) {
                return Err(JobError::new(ErrorCode::InvalidRequest, "duplicate anisotropic captured face"));
            }
        }
        if self.reflections.as_ref().is_some_and(|r| {
            r.bodies.iter().any(|b| b.mode == ReflectionMode::Mirror && self.anisotropic_faces.iter().flatten().any(|t| t.body_name == b.body))
        }) {
            return Err(JobError::new(ErrorCode::InvalidRequest, "delta mirror cannot have anisotropic rough axes"));
        }
        if self.preset.is_some() && self.lights.is_some() {
            return Err(JobError::new(ErrorCode::InvalidRequest, "choose either a lighting preset or custom lights"));
        }
        if !self.exposure_ev.is_finite() || !(-16.0..=16.0).contains(&self.exposure_ev) {
            return Err(JobError::new(ErrorCode::InvalidRequest, "raytrace exposure_ev must be finite and within -16..16"));
        }
        if let Some(lights) = &self.lights {
            if lights.len() > 8 || lights.capacity() > 8 {
                return Err(JobError::new(ErrorCode::InvalidRequest, "at most eight directional lights"));
            }
            for light in lights {
                light.validate()?;
            }
        }
        Ok(())
    }
}

/// Fully resolved input; no mutable session, topology graph or GPU resources.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImageRequest {
    /// Transient visibility of the three implicit studio lights; never saved in a design.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub studio_weights: Option<[f64; 3]>,
    /// Captured GPU resources stay in admitted work, never in status or saved data.
    #[serde(skip)]
    pub environment: Option<Arc<crate::lighting::environment::EnvironmentSource>>,
    #[serde(skip)]
    pub textures: Option<Arc<textures::RuntimeTextures>>,
    #[serde(default)]
    pub integrator: Integrator,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raytrace: Option<RaytraceOptions>,
    pub camera: Camera,
    pub width: u32,
    pub height: u32,
    pub background: [u8; 3],
    pub transparent_background: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ColourEncoding {
    Srgb,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AlphaConvention {
    Straight,
}

impl ImageRequest {
    pub fn cpu_budget(&self) -> CpuBudget {
        if self.integrator == Integrator::Raytrace {
            self.raytrace.as_ref().map_or(CpuBudget::Compact, |options| options.budget)
        } else {
            CpuBudget::Compact
        }
    }

    /// Fixed inline request growth is prepaid even when raytrace is None.
    /// Queued/worker state cannot fund the retained Record after work release.
    pub const fn fixed_cpu_metadata_bytes() -> u64 {
        (std::mem::size_of::<Option<CausticOptions>>()
            + std::mem::size_of::<Option<volume::HomogeneousVolume>>()
            + std::mem::size_of::<Option<Box<Sun>>>()
            + std::mem::size_of::<Option<Box<CpuSky>>>()) as u64
    }
    /// Resource summaries and fixed CPU intent headers share one retained lease.
    pub fn retained_resource_metadata_bytes(&self) -> u64 {
        let environment = self
            .environment
            .as_ref()
            .map_or(0, |source| (std::mem::size_of::<crate::lighting::environment::EnvironmentInfo>() + source.info().source_sha256.len()) as u64);
        Self::fixed_cpu_metadata_bytes()
            + environment
            + if self.textures.is_some() { std::mem::size_of::<textures::TextureSummary>() as u64 } else { 0 }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct FrameInfo {
    pub job: JobId,
    pub scene_generation: u64,
    pub backend: BackendId,
    pub executor: ExecutorId,
    pub width: u32,
    pub height: u32,
    pub encoding: ColourEncoding,
    pub alpha: AlphaConvention,
    /// Exact encoded PNG length, not pixel storage or reserved Vec capacity.
    pub png_bytes: u64,
    /// Cached lowercase SHA-256 of those encoded bytes, computed before completion.
    pub png_sha256: String,
}
#[cfg(test)]
mod runtime_request_roundtrip_tests {
    use super::*;
    #[test]
    fn default_preset_and_empty_lights_requests_roundtrip_without_invalid_null_sequence() {
        for options in [
            RaytraceOptions::default(),
            RaytraceOptions { preset: Some(LightingPreset::Neutral), ..Default::default() },
            RaytraceOptions { lights: Some(Vec::new()), ..Default::default() },
        ] {
            let value = serde_json::to_value(&options).unwrap();
            if options.lights.is_none() {
                assert!(value.get("lights").is_none());
            } else {
                assert_eq!(value.get("lights").unwrap(), &serde_json::json!([]));
            }
            let decoded: RaytraceOptions = serde_json::from_value(value).unwrap();
            assert_eq!(decoded, options);
            assert!(decoded.validate().is_ok());
        }
        assert_eq!(serde_json::from_value::<RaytraceOptions>(serde_json::json!({})).unwrap(), RaytraceOptions::default());
    }

    #[test]
    fn cpu_progressive_options_are_optional_strict_and_bounded_before_capture() {
        use serde_json::json;
        assert!(serde_json::to_value(RaytraceOptions::default()).unwrap().get("progressive").is_none());
        assert_eq!(serde_json::from_value::<RaytraceOptions>(json!({"progressive":null})).unwrap(), RaytraceOptions::default());
        let options: RaytraceOptions = serde_json::from_value(json!({"progressive":{"samples_per_pixel":3,"seed":u32::MAX}})).unwrap();
        options.validate().unwrap();
        assert_eq!(serde_json::from_value::<RaytraceOptions>(serde_json::to_value(&options).unwrap()).unwrap(), options);
        for invalid in [json!({"seed":-1}), json!({"seed":4294967296_u64}), json!({"seed":0.5}), json!({"adaptive":true})] {
            assert!(serde_json::from_value::<CpuProgressiveOptions>(invalid).is_err());
        }
        for samples_per_pixel in [0, 65, u32::MAX] {
            assert!(CpuProgressiveOptions { samples_per_pixel, seed: 0 }.validate().is_err());
        }
        for samples_per_pixel in [1, 2, 3, 16, 64] {
            CpuProgressiveOptions { samples_per_pixel, seed: 0 }.validate_image(512, 512).unwrap();
        }
        CpuProgressiveOptions::default().validate_image(1024, 1024).unwrap();
        assert!(CpuProgressiveOptions { samples_per_pixel: 17, seed: 0 }.validate_image(1024, 1024).is_err());
        for (width, height) in [(0, 1), (1, 0), (1025, 1024), (4097, 1), (u32::MAX, u32::MAX)] {
            assert!(CpuProgressiveOptions { samples_per_pixel: 1, seed: 0 }.validate_image(width, height).is_err());
        }
    }

    #[test]
    fn progressive_rejects_incompatible_sampling_without_changing_fixed_aa() {
        let progressive = Some(CpuProgressiveOptions::default());
        for primary_samples in [4, 16] {
            let fixed = RaytraceOptions { primary_samples, ..Default::default() };
            fixed.validate().unwrap();
            assert!(RaytraceOptions { progressive, ..fixed }.validate().is_err());
        }
        let reflections = ReflectionOptions {
            bodies: vec![ReflectionBody { body: "Body".into(), mode: ReflectionMode::RoughSpecular }],
            samples: 4,
            max_bounces: 2,
        };
        assert!(RaytraceOptions { progressive, reflections: Some(reflections), ..Default::default() }.validate().is_err());
        let area = RectangleLightOptions {
            center_mm: [0.0, 0.0, 10.0],
            edge_u_mm: [1.0, 0.0, 0.0],
            edge_v_mm: [0.0, -1.0, 0.0],
            color: [1.0; 3],
            intensity: 1.0,
        };
        RaytraceOptions { progressive, area_lights: Some(vec![area]), ..Default::default() }.validate().unwrap();
        let dielectric = DielectricOptions {
            model: None,
            shadow_transmission: false,
            subsurface: None,
            body_index: 0,
            body_name: "Body".into(),
            boundary: DielectricBoundary::ThinSheet,
            ior: 1.5,
            absorption_per_mm: [0.0; 3],
            thickness_mm: 1.0,
        };
        RaytraceOptions { progressive, dielectrics: vec![dielectric], ..Default::default() }.validate().unwrap();
        let face = AnisotropicFaceOptions {
            body_index: 0,
            body_name: "Body".into(),
            face: 0,
            tangent: [1.0, 0.0, 0.0],
            orientation: AnisotropicOrientation::Zero,
            roughness_x: 0.2,
            roughness_y: 0.6,
        };
        RaytraceOptions { progressive, anisotropic_faces: [Some(face), None], ..Default::default() }.validate().unwrap();
        RaytraceOptions { progressive, area_lights: Some(Vec::new()), ..Default::default() }.validate().unwrap();
    }
}
