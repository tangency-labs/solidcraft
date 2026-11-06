//! Deterministic counter sampling and owned camera-film continuation.
//! No target sample count, work quantum or preview cadence enters the sampler.
use crate::{
    film::{
        adaptive::{AdaptiveCameraFilm, CountsPlane, SamplingIdentity},
        primary::PrimaryFilm,
    },
    job::{
        CancelToken, CpuBudget, CpuProgressiveOptions, ErrorCode, JobError, Result,
        adaptive::{AdaptiveSampling, PreviewStage, SamplingReport},
    },
};

pub(super) enum CameraFilm {
    Uniform(PrimaryFilm),
    // One fixed, fallibly prepaid metadata slot keeps the ordinary task's layout
    // small. Its two pixel arrays are allocated only for explicit adaptive intent.
    Adaptive(Vec<AdaptiveCameraFilm>),
}
impl CameraFilm {
    pub(super) fn required_bytes(
        width: u32,
        height: u32,
        options: CpuProgressiveOptions,
        adaptive: Option<&AdaptiveSampling>,
        budget: CpuBudget,
    ) -> Result<u64> {
        match adaptive {
            None => PrimaryFilm::required_bytes(width, height, options, budget),
            Some(adaptive) => AdaptiveCameraFilm::required_bytes(width, height, options, *adaptive, budget),
        }
    }
    pub(super) fn new(
        width: u32,
        height: u32,
        options: CpuProgressiveOptions,
        adaptive: Option<&AdaptiveSampling>,
        budget: CpuBudget,
        bytes: u64,
        cancel: &CancelToken,
    ) -> Result<Self> {
        match adaptive {
            None => PrimaryFilm::new(width, height, options, bytes, cancel, budget).map(Self::Uniform),
            Some(adaptive) => {
                cancel.check()?;
                let mut slot = Vec::new();
                slot.try_reserve_exact(1).map_err(|_| JobError::new(ErrorCode::AllocationFailed, "could not reserve CPU adaptive film state"))?;
                if slot.capacity() != 1 {
                    return Err(JobError::new(ErrorCode::LimitExceeded, "CPU adaptive state capacity exceeds admission"));
                }
                slot.push(AdaptiveCameraFilm::new(width, height, options, *adaptive, budget, bytes, cancel)?);
                Ok(Self::Adaptive(slot))
            }
        }
    }
    fn state(&self) -> Result<&AdaptiveCameraFilm> {
        match self {
            Self::Adaptive(slot) => slot.first().ok_or_else(|| JobError::new(ErrorCode::Internal, "CPU adaptive film state is missing")),
            _ => Err(JobError::new(ErrorCode::Internal, "ordinary film has no adaptive state")),
        }
    }
    fn state_mut(&mut self) -> Result<&mut AdaptiveCameraFilm> {
        match self {
            Self::Adaptive(slot) => slot.first_mut().ok_or_else(|| JobError::new(ErrorCode::Internal, "CPU adaptive film state is missing")),
            _ => Err(JobError::new(ErrorCode::Internal, "ordinary film has no adaptive state")),
        }
    }
    pub(super) fn samples(&self) -> u32 {
        match self {
            Self::Uniform(film) => film.samples(),
            Self::Adaptive(slot) => slot.first().map_or(0, AdaptiveCameraFilm::completed_passes),
        }
    }
    pub(super) fn initialized(&self) -> usize {
        match self {
            Self::Uniform(film) => film.initialized(),
            Self::Adaptive(slot) => slot.first().map_or(0, AdaptiveCameraFilm::initialized),
        }
    }
    pub(super) fn ready(&self) -> bool {
        match self {
            Self::Uniform(film) => film.ready(),
            Self::Adaptive(slot) => slot.first().is_some_and(AdaptiveCameraFilm::ready),
        }
    }
    pub(super) fn initialize_one(&mut self, cancel: &CancelToken, charge: &mut impl FnMut() -> Result<()>) -> Result<()> {
        match self {
            Self::Uniform(film) => film.initialize_one(cancel, charge),
            Self::Adaptive(_) => self.state_mut()?.initialize_one(cancel, charge),
        }
    }
    pub(super) fn add(
        &mut self,
        index: usize,
        ordinal: u32,
        rgb: [f64; 3],
        covered: bool,
        cancel: &CancelToken,
        charge: &mut impl FnMut() -> Result<()>,
    ) -> Result<()> {
        match self {
            Self::Uniform(film) => film.add(index, ordinal, rgb, covered, cancel, charge),
            Self::Adaptive(_) => {
                let film = self.state_mut()?;
                if ordinal != film.completed_passes() {
                    return Err(JobError::new(ErrorCode::InvalidRequest, "CPU adaptive absolute pass disagrees with its film"));
                }
                let identity =
                    film.sampling_identity(index)?.ok_or_else(|| JobError::new(ErrorCode::Internal, "stopped CPU adaptive pixel was traced"))?;
                film.add(index, identity, rgb, covered, cancel, charge)
            }
        }
    }
    pub(super) fn pixel(&self, index: usize) -> Result<([f64; 3], u16, u16)> {
        match self {
            Self::Uniform(film) => film.pixel(index),
            Self::Adaptive(_) => {
                let pass = self.state()?.complete_pass()?;
                let (mean, _) = pass.pixel(index)?;
                let counts = pass.counts().pixel(index)?;
                Ok((mean, counts.covered(), counts.samples()))
            }
        }
    }
    pub(super) fn counts(&self) -> Result<Option<&CountsPlane>> {
        match self {
            Self::Uniform(_) => Ok(None),
            Self::Adaptive(_) => self.state()?.counts_plane().map(Some),
        }
    }
    pub(super) fn identity(&self, index: usize, seed: u32) -> Result<Option<SamplingIdentity>> {
        match self {
            Self::Uniform(film) => Ok(Some(SamplingIdentity { seed, ordinal: film.samples(), stage: PreviewStage::Production })),
            Self::Adaptive(_) => self.state()?.sampling_identity(index),
        }
    }
    pub(super) fn skip(&mut self, index: usize, cancel: &CancelToken, charge: &mut impl FnMut() -> Result<()>) -> Result<()> {
        self.state_mut()?.skip_one(index, cancel, charge)
    }
    pub(super) fn resetting(&self) -> bool {
        matches!(self, Self::Adaptive(slot) if slot.first().is_some_and(AdaptiveCameraFilm::resetting))
    }
    pub(super) fn reset_progress(&self) -> usize {
        match self {
            Self::Adaptive(slot) => slot.first().map_or(0, AdaptiveCameraFilm::next_pixel),
            _ => 0,
        }
    }
    pub(super) fn reset_one(&mut self, cancel: &CancelToken, charge: &mut impl FnMut() -> Result<()>) -> Result<()> {
        self.state_mut()?.reset_one(cancel, charge)
    }
    pub(super) fn pilot_complete(&self) -> bool {
        match self {
            Self::Uniform(_) => false,
            Self::Adaptive(slot) => slot.first().is_some_and(|film| {
                film.complete_pass().is_ok_and(|pass| {
                    let metadata = pass.metadata();
                    metadata.stage == PreviewStage::Pilot && metadata.completed == metadata.pilot_samples
                })
            }),
        }
    }
    pub(super) fn begin_production(&mut self, cancel: &CancelToken, charge: &mut impl FnMut() -> Result<()>) -> Result<()> {
        self.state_mut()?.begin_production(cancel, charge)
    }
    pub(super) fn report(&self) -> Result<Option<SamplingReport>> {
        match self {
            Self::Uniform(_) => Ok(None),
            Self::Adaptive(_) => self.state()?.report().map(Some),
        }
    }
}
pub(super) struct Progressive {
    pub film: CameraFilm,
    pub options: CpuProgressiveOptions,
    pub target: u32,
    pub resolving: bool,
    pub resolve_pixel: usize,
}
impl Progressive {
    pub(super) fn identity(&self, pixel: usize) -> Result<SamplingIdentity> {
        self.film.identity(pixel, self.options.seed)?.ok_or_else(|| JobError::new(ErrorCode::Internal, "stopped adaptive pixel requested variates"))
    }
    pub(super) fn variates(&self, pixel: usize, domain: u64) -> Result<[f64; 2]> {
        let identity = self.identity(pixel)?;
        Ok(variates(identity.seed, pixel as u64, identity.ordinal, domain))
    }
    pub(super) fn area_key(&self, pixel: usize, value: u64) -> Result<u64> {
        let identity = self.identity(pixel)?;
        Ok(value ^ (u64::from(identity.seed ^ self.options.seed) << 32))
    }
}

/// Integer-only mixing before conversion to exactly representable 53-bit unit
/// variates. Domains separate camera, BSDF and environment choices at each bounce.
pub(super) fn variates(seed: u32, pixel: u64, ordinal: u32, domain: u64) -> [f64; 2] {
    fn mix(mut value: u64) -> u64 {
        value ^= value >> 30;
        value = value.wrapping_mul(0xbf58_476d_1ce4_e5b9);
        value ^= value >> 27;
        value = value.wrapping_mul(0x94d0_49bb_1331_11eb);
        value ^ (value >> 31)
    }
    let key = mix(u64::from(seed) ^ 0x4350_5546_494c_4d00)
        ^ mix(pixel.wrapping_add(0x9e37_79b9_7f4a_7c15))
        ^ mix(u64::from(ordinal).wrapping_add(0xd1b5_4a32_d192_ed03))
        ^ mix(domain.wrapping_add(0x94d0_49bb_1331_11eb));
    [0_u64, 1].map(|axis| (mix(key ^ mix(axis.wrapping_add(0x632b_e59b_d9b4_e019))) >> 11) as f64 / 9_007_199_254_740_992.0)
}
