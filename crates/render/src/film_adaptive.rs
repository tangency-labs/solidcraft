//! CPU camera film for the explicit independent-pilot image continuation.
//! Independent pilots allocate fixed production counts. Pilot radiance is discarded,
//! and complete passes borrow exact integer counts without retaining a new snapshot.
use super::{invalid, limit};
use crate::job::{
    CancelToken, CpuBudget, CpuProgressiveOptions, ErrorCode, JobError, Result,
    adaptive::{AdaptivePreview, AdaptiveSampling, PreviewStage, SamplingReport},
};

#[derive(Clone, Copy, Default)]
struct Moments {
    // Pilot: Welford RGBA mean/M2. Production: sum.rgb, with M2 unused.
    value: [f64; 4],
    m2: [f64; 4],
}
/// Exact counts have a separate, borrowed plane so image finishing need not
/// reconstruct integer coverage from FLOAT32 alpha or allocate another image.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PixelCounts {
    covered: u16,
    samples: u16,
    planned: u16,
}
impl PixelCounts {
    pub fn covered(self) -> u16 {
        self.covered
    }
    pub fn samples(self) -> u16 {
        self.samples
    }
    pub fn alpha8(self) -> Result<u8> {
        let samples = u32::from(self.samples);
        let covered = u32::from(self.covered);
        if samples == 0 || covered > samples {
            return Err(invalid("adaptive coverage has no valid sample denominator"));
        }
        u8::try_from((covered * 510 + samples) / (samples * 2)).map_err(|_| invalid("adaptive coverage exceeds one byte"))
    }
}
pub struct CountsPlane {
    values: Vec<PixelCounts>,
}
impl CountsPlane {
    pub fn len(&self) -> usize {
        self.values.len()
    }
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }
    pub fn pixel(&self, index: usize) -> Result<PixelCounts> {
        self.values.get(index).copied().ok_or_else(|| invalid("adaptive count index is outside the complete plane"))
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stage {
    Pilot,
    Reset,
    Production,
    Done,
}
/// A stream identity for an existing counter sampler. Production keeps the
/// original seed and zero-based ordinals. Pilots use a distinct fixed seed tag.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SamplingIdentity {
    pub seed: u32,
    pub ordinal: u32,
    pub stage: PreviewStage,
}
/// Two prepaid arrays, one ordered update/skip per pixel per complete sweep.
/// ImageTask includes required_bytes in the SAME existing job lease. The counts
/// remain borrowed by finishing, never copied into an unbilled snapshot plane.
pub struct AdaptiveCameraFilm {
    width: u32,
    height: u32,
    maximum: u32,
    seed: u32,
    options: AdaptiveSampling,
    moments: Vec<Moments>,
    counts: CountsPlane,
    count: usize,
    completed: u32,
    next_pixel: usize,
    stage: Stage,
    report: SamplingReport,
    production_evaluations: u64,
}
/// Borrowing this view prevents reset/add/skip until finishing releases it.
/// It is neither an encoded image nor a transferable retained snapshot/lease.
pub struct AdaptivePass<'a> {
    film: &'a AdaptiveCameraFilm,
}
impl AdaptivePass<'_> {
    pub fn metadata(&self) -> AdaptivePreview {
        let pilot = self.film.stage == Stage::Pilot;
        AdaptivePreview {
            stage: if pilot { PreviewStage::Pilot } else { PreviewStage::Production },
            completed: self.film.completed,
            target: if pilot { self.film.options.pilot_samples } else { self.film.maximum },
            pilot_samples: self.film.options.pilot_samples,
            minimum_production_samples: self.film.options.min_samples,
            maximum_production_samples: self.film.maximum,
        }
    }
    pub fn counts(&self) -> &CountsPlane {
        &self.film.counts
    }
    /// Premultiplied linear radiance and coverage, before any float32 conversion,
    /// denoising, exposure or display transform. The denominator is pixel-specific.
    pub fn pixel(&self, index: usize) -> Result<([f64; 3], f64)> {
        let counts = self.counts().pixel(index)?;
        if counts.samples == 0 || counts.covered > counts.samples {
            return Err(invalid("adaptive pass has invalid exact coverage"));
        }
        let value = self.film.moments.get(index).ok_or_else(|| invalid("adaptive resolve index exceeds admission"))?.value;
        let divisor = if self.film.stage == Stage::Pilot { 1.0 } else { f64::from(counts.samples) };
        Ok(([value[0] / divisor, value[1] / divisor, value[2] / divisor], f64::from(counts.covered) / f64::from(counts.samples)))
    }
}
impl AdaptiveCameraFilm {
    /// The existing CPU profile includes all pilots plus the production cap.
    /// GPU AdaptiveSampling chooses the statistical policy; no CPU cap is raised.
    pub fn required_bytes(width: u32, height: u32, production: CpuProgressiveOptions, options: AdaptiveSampling, budget: CpuBudget) -> Result<u64> {
        options.validate(production.samples_per_pixel)?;
        let total = options.pilot_samples.checked_add(production.samples_per_pixel).ok_or_else(|| limit("adaptive camera target overflow"))?;
        budget.validate_image(width, height, Some(CpuProgressiveOptions { samples_per_pixel: total, ..production }))?;
        (u64::from(width) * u64::from(height))
            .checked_mul((std::mem::size_of::<Moments>() + std::mem::size_of::<PixelCounts>()) as u64)
            .and_then(|bytes| bytes.checked_add(std::mem::size_of::<Self>() as u64))
            .filter(|bytes| *bytes <= budget.limits().max_job_bytes)
            .ok_or_else(|| limit("adaptive film storage exceeds the unchanged CPU profile"))
    }
    pub fn new(
        width: u32,
        height: u32,
        production: CpuProgressiveOptions,
        options: AdaptiveSampling,
        budget: CpuBudget,
        admitted_bytes: u64,
        cancel: &CancelToken,
    ) -> Result<Self> {
        cancel.check()?;
        let required = Self::required_bytes(width, height, production, options, budget)?;
        if admitted_bytes < required {
            return Err(limit("adaptive film is not prepaid by the owning image job"));
        }
        let count = usize::try_from(u64::from(width) * u64::from(height)).map_err(|_| limit("adaptive film exceeds the address space"))?;
        let mut moments = Vec::new();
        let mut counts = Vec::new();
        moments.try_reserve_exact(count).map_err(|_| JobError::new(ErrorCode::AllocationFailed, "could not reserve adaptive moments"))?;
        cancel.check()?;
        counts.try_reserve_exact(count).map_err(|_| JobError::new(ErrorCode::AllocationFailed, "could not reserve exact adaptive counts"))?;
        if moments.capacity() != count || counts.capacity() != count {
            return Err(limit("actual adaptive film capacity exceeds admission"));
        }
        cancel.check()?;
        Ok(Self {
            width,
            height,
            maximum: production.samples_per_pixel,
            seed: production.seed,
            options,
            moments,
            counts: CountsPlane { values: counts },
            count,
            completed: 0,
            next_pixel: 0,
            stage: Stage::Pilot,
            report: SamplingReport {
                pilot_samples_per_pixel: options.pilot_samples,
                minimum_production_samples: production.samples_per_pixel,
                maximum_production_samples: 0,
                production_samples: 0,
                primary_path_evaluations: 0,
                pixels_at_cap: 0,
                transmission_strata: 1,
            },
            production_evaluations: 0,
        })
    }
    pub fn dimensions(&self) -> [u32; 2] {
        [self.width, self.height]
    }
    pub fn initialized(&self) -> usize {
        self.counts.values.len()
    }
    pub fn ready(&self) -> bool {
        self.initialized() == self.count && self.moments.len() == self.count
    }
    pub fn initialize_one(&mut self, cancel: &CancelToken, charge: &mut impl FnMut() -> Result<()>) -> Result<()> {
        cancel.check()?;
        if self.ready() || self.moments.len() != self.counts.values.len() {
            return Err(invalid("adaptive film initialization is complete or inconsistent"));
        }
        charge()?;
        cancel.check()?;
        self.moments.push(Moments::default());
        self.counts.values.push(PixelCounts::default());
        Ok(())
    }
    pub fn next_pixel(&self) -> usize {
        self.next_pixel
    }
    pub fn completed_passes(&self) -> u32 {
        if self.stage == Stage::Pilot { self.completed } else { self.options.pilot_samples + self.completed }
    }
    pub fn final_pass_target(&self) -> u32 {
        self.options.pilot_samples + self.maximum
    }
    pub fn is_final(&self) -> bool {
        self.stage == Stage::Done
    }
    fn ordered(&self, index: usize) -> Result<PixelCounts> {
        if !self.ready() || self.stage == Stage::Reset || self.stage == Stage::Done || index != self.next_pixel {
            return Err(invalid("adaptive film needs the next pixel in a sampling sweep"));
        }
        self.counts.pixel(index)
    }
    /// None means this production pixel has already reached its pilot allocation;
    /// the caller must perform skip_one WITHOUT tracing or consuming a variate.
    pub fn sampling_identity(&self, index: usize) -> Result<Option<SamplingIdentity>> {
        let counts = self.ordered(index)?;
        if self.stage == Stage::Pilot {
            if self.completed >= self.options.pilot_samples {
                return Err(invalid("complete pilots require an explicit production reset"));
            }
            return Ok(Some(SamplingIdentity { seed: self.seed ^ 0xa511_e9b3, ordinal: self.completed, stage: PreviewStage::Pilot }));
        }
        if u32::from(counts.samples) >= u32::from(counts.planned) {
            return Ok(None);
        }
        Ok(Some(SamplingIdentity { seed: self.seed, ordinal: self.completed, stage: PreviewStage::Production }))
    }
    fn advance(&mut self) {
        self.next_pixel += 1;
        if self.next_pixel == self.count {
            self.next_pixel = 0;
            self.completed += 1;
            if self.stage == Stage::Production && self.completed == self.maximum {
                self.stage = Stage::Done;
            }
        }
    }
    /// Pilot moments/production sums and exact counts form ONE transaction. Every
    /// fallible check, work debit and cancellation check precedes both writes.
    pub fn add(
        &mut self,
        index: usize,
        identity: SamplingIdentity,
        rgb: [f64; 3],
        covered: bool,
        cancel: &CancelToken,
        charge: &mut impl FnMut() -> Result<()>,
    ) -> Result<()> {
        cancel.check()?;
        let expected = self.sampling_identity(index)?.ok_or_else(|| invalid("stopped adaptive pixel cannot accept another sample"))?;
        if identity != expected {
            return Err(invalid("adaptive stream/ordinal does not match the captured sweep"));
        }
        if rgb.iter().any(|value| !value.is_finite() || !(0.0..=1e12).contains(value)) || (!covered && rgb != [0.0; 3]) {
            return Err(invalid("invalid adaptive premultiplied camera radiance or coverage"));
        }
        let mut moments = self.moments.get(index).copied().ok_or_else(|| invalid("adaptive moments disappeared"))?;
        let mut counts = self.counts.pixel(index)?;
        let samples = counts.samples.checked_add(1).ok_or_else(|| limit("adaptive sample count overflow"))?;
        if u32::from(samples) != self.completed + 1 {
            return Err(invalid("adaptive pixel count disagrees with its active ordinal"));
        }
        let hits =
            counts.covered.checked_add(u16::from(covered)).filter(|hits| *hits <= samples).ok_or_else(|| limit("adaptive coverage overflow"))?;
        if self.stage == Stage::Pilot {
            let value = [rgb[0], rgb[1], rgb[2], f64::from(u8::from(covered))];
            for ((mean, m2), value) in moments.value.iter_mut().zip(moments.m2.iter_mut()).zip(value) {
                let delta = value - *mean;
                *mean += delta / f64::from(samples);
                *m2 += delta * (value - *mean);
            }
            if u32::from(samples) == self.options.pilot_samples {
                counts.planned = u16::try_from(self.options.target(moments.value, moments.m2, self.maximum)?)
                    .map_err(|_| limit("adaptive target exceeds exact count representation"))?;
            }
        } else {
            for (sum, value) in moments.value.iter_mut().take(3).zip(rgb) {
                *sum += value;
                if !sum.is_finite() || *sum < 0.0 || *sum > f64::from(self.maximum) * 1e12 {
                    return Err(invalid("adaptive production sum exceeds its finite bound"));
                }
            }
        }
        counts.samples = samples;
        counts.covered = hits;
        // Accumulate bounded report metadata with the existing pixel transaction;
        // final publication never needs a synchronous image-sized report scan.
        let mut report = self.report.clone();
        if self.stage == Stage::Pilot && u32::from(samples) == self.options.pilot_samples {
            let target = u32::from(counts.planned);
            report.minimum_production_samples = report.minimum_production_samples.min(target);
            report.maximum_production_samples = report.maximum_production_samples.max(target);
            report.production_samples =
                report.production_samples.checked_add(u64::from(target)).ok_or_else(|| limit("adaptive report allocation overflow"))?;
            report.pixels_at_cap += u64::from(target == self.maximum);
        }
        let evaluations = self
            .production_evaluations
            .checked_add(u64::from(self.stage == Stage::Production))
            .ok_or_else(|| limit("adaptive evaluation report overflow"))?;
        let target_moments = self.moments.get_mut(index).ok_or_else(|| invalid("adaptive destination moments disappeared"))?;
        let target_counts = self.counts.values.get_mut(index).ok_or_else(|| invalid("adaptive destination counts disappeared"))?;
        charge()?;
        cancel.check()?;
        *target_moments = moments;
        *target_counts = counts;
        self.report = report;
        self.production_evaluations = evaluations;
        self.advance();
        Ok(())
    }
    pub fn skip_one(&mut self, index: usize, cancel: &CancelToken, charge: &mut impl FnMut() -> Result<()>) -> Result<()> {
        cancel.check()?;
        if self.stage != Stage::Production || self.sampling_identity(index)?.is_some() {
            return Err(invalid("only a fully sampled production pixel may be skipped"));
        }
        charge()?;
        cancel.check()?;
        self.advance();
        Ok(())
    }
    /// Borrow exact counts only at a complete, frozen pass. The caller may pass
    /// this borrow into one finishing step without keeping a self-reference.
    pub fn counts_plane(&self) -> Result<&CountsPlane> {
        self.complete_pass()?;
        Ok(&self.counts)
    }

    pub fn complete_pass(&self) -> Result<AdaptivePass<'_>> {
        if !self.ready() || self.next_pixel != 0 || self.completed == 0 || self.stage == Stage::Reset {
            return Err(invalid("adaptive film has no complete sampling pass"));
        }
        Ok(AdaptivePass { film: self })
    }
    /// Called only after last pilot finishing/publication is no longer borrowing
    /// this film. Reset is cooperative and never exposes partially discarded pilots.
    pub fn begin_production(&mut self, cancel: &CancelToken, charge: &mut impl FnMut() -> Result<()>) -> Result<()> {
        cancel.check()?;
        if self.stage != Stage::Pilot || self.next_pixel != 0 || self.completed != self.options.pilot_samples {
            return Err(invalid("production requires all complete independent pilots"));
        }
        charge()?;
        cancel.check()?;
        self.stage = Stage::Reset;
        self.completed = 0;
        Ok(())
    }
    pub fn resetting(&self) -> bool {
        self.stage == Stage::Reset
    }
    pub fn reset_one(&mut self, cancel: &CancelToken, charge: &mut impl FnMut() -> Result<()>) -> Result<()> {
        cancel.check()?;
        if self.stage != Stage::Reset {
            return Err(invalid("adaptive film is not resetting pilots"));
        }
        let index = self.next_pixel;
        let moments = self.moments.get_mut(index).ok_or_else(|| invalid("adaptive reset index exceeds admission"))?;
        let counts = self.counts.values.get_mut(index).ok_or_else(|| invalid("adaptive reset count disappeared"))?;
        if !(self.options.min_samples..=self.maximum).contains(&u32::from(counts.planned)) {
            return Err(invalid("adaptive reset has no admitted production allocation"));
        }
        charge()?;
        cancel.check()?;
        *moments = Moments::default();
        counts.covered = 0;
        counts.samples = 0;
        self.next_pixel += 1;
        if self.next_pixel == self.count {
            self.next_pixel = 0;
            self.stage = Stage::Production;
        }
        Ok(())
    }
    pub fn production_target(&self, index: usize) -> Result<u32> {
        if self.stage == Stage::Pilot && (self.next_pixel != 0 || self.completed != self.options.pilot_samples) {
            return Err(invalid("adaptive allocations require the complete pilot pass"));
        }
        Ok(u32::from(self.counts.pixel(index)?.planned))
    }
    pub fn report(&self) -> Result<SamplingReport> {
        if !self.is_final() {
            return Err(invalid("adaptive sampling report requires complete production"));
        }
        if self.production_evaluations != self.report.production_samples {
            return Err(invalid("adaptive final evaluation count disagrees with its pilot allocation"));
        }
        let mut report = self.report.clone();
        report.primary_path_evaluations = u64::from(self.options.pilot_samples) * self.count as u64 + self.production_evaluations;
        Ok(report)
    }
}
