//! Bounded presentation of an owned scene-linear CPU pass. Transport and film remain separate.
//! The caller supplies the cumulative work debit; every mutating stage checks it first.
use super::denoise::{DenoiseTask, Guide};
use super::{CancelToken, DenoiseDiagnostics, DenoiseGuideClass, DenoiseSettings, ErrorCode, JobError, Phase, Progress, Result, TaskPoll};
use super::{bloom::BloomTask, encoding::PngEncoding, linear_exr::ExrTask, photographic::PhotographicTask, presentation::Presentation};
use crate::film::adaptive::CountsPlane;

fn invalid(message: &str) -> JobError {
    JobError::new(ErrorCode::InvalidRequest, message)
}

fn coverage_alpha(coverage: f32, samples: u32) -> Result<u8> {
    if samples == 0 || samples > super::MAX_SAMPLES_PER_PIXEL || !coverage.is_finite() || !(0.0..=1.0).contains(&coverage) {
        return Err(invalid("invalid CPU display coverage count"));
    }
    // Every CPU pass has one known denominator. At the admitted 65536 samples, float32
    // quantisation is far below half a hit, so the original count is unique.
    // Keep the raw FLOAT32 plane unchanged and round only the final byte from
    // the integer ratio: 5/6 rounds to 213; float32 alpha alone gives 212.
    let covered = (f64::from(coverage) * f64::from(samples)).round() as u32;
    if covered > samples || (f64::from(covered) / f64::from(samples)) as f32 != coverage {
        return Err(invalid("CPU display coverage disagrees with its sample count"));
    }
    let denominator = samples;
    u8::try_from((covered * 510 + denominator) / (denominator * 2)).map_err(|_| invalid("CPU display alpha exceeds one byte"))
}

struct Resolve {
    source: Vec<[f32; 4]>,
    pixels: Vec<[f32; 4]>,
    raw: bool,
}

enum Stage {
    Diagnostics(usize),
    MaskPng,
    Resolve(Resolve),
    Exr(ExrTask),
    Denoise(DenoiseTask),
    Bloom(BloomTask),
    Display(usize),
    Photographic(PhotographicTask),
    Png,
    Done,
}

pub(crate) struct OutputTask {
    width: u32,
    height: u32,
    trace_width: u32,
    trace_height: u32,
    supersampling: u32,
    denoise_settings: DenoiseSettings,
    retain_exr: bool,
    original: Option<Vec<[f32; 4]>>,
    resolved_alpha: Vec<u8>,
    diagnostics: Option<DenoiseDiagnostics>,
    mask_rgba: Vec<u8>,
    mask_png: Option<Vec<u8>>,
    encoded_limit: u64,
    presentation: Presentation,
    cancel: CancelToken,
    stage: Option<Stage>,
    png: Option<PngEncoding>,
    linear: Option<Vec<[f32; 4]>>,
    glow: Option<Vec<[f32; 3]>>,
    rgba: Vec<u8>,
    preserve_display: bool,
    denoise: bool,
    coverage_samples: u32,
    exr: Option<Vec<u8>>,
}
impl OutputTask {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new_with_quality(
        width: u32,
        height: u32,
        encoded_limit: u64,
        linear: Vec<[f32; 4]>,
        rgba: Vec<u8>,
        presentation: Presentation,
        preserve_display: bool,
        retain_exr: bool,
        denoise: bool,
        denoise_settings: DenoiseSettings,
        supersampling: u32,
        diagnostics: bool,
        coverage_samples: u32,
        cancel: CancelToken,
        charge: &mut impl FnMut(u64) -> Result<()>,
    ) -> Result<Self> {
        presentation.validate()?;
        cancel.check()?;
        denoise_settings.validate()?;
        if (denoise || supersampling != 1) && preserve_display {
            return Err(invalid("CPU denoising cannot preserve an unfiltered display buffer"));
        }
        if coverage_samples == 0 {
            return Err(invalid("CPU output requires a complete coverage sample count"));
        }
        super::linear_exr::encoded_limit(width, height)?;
        let (trace_width, trace_height) = super::supersampled_dimensions(width, height, supersampling)?;
        let count = u64::from(trace_width) * u64::from(trace_height);
        if linear.len() as u64 != count || (preserve_display && rgba.len() as u64 != count * 4) || (!preserve_display && !rgba.is_empty()) {
            return Err(invalid("CPU output storage does not match its complete pass"));
        }
        let mut task = Self {
            width,
            height,
            trace_width,
            trace_height,
            supersampling,
            denoise_settings,
            retain_exr,
            original: None,
            resolved_alpha: Vec::new(),
            diagnostics: diagnostics.then(|| DenoiseDiagnostics::new(trace_width, trace_height)),
            mask_rgba: Vec::new(),
            mask_png: None,
            encoded_limit,
            presentation,
            cancel,
            stage: None,
            png: None,
            linear: None,
            glow: None,
            rgba,
            preserve_display,
            denoise,
            coverage_samples,
            exr: None,
        };
        if diagnostics {
            task.linear = Some(linear);
            task.stage = Some(Stage::Diagnostics(0));
        } else {
            task.prepare_raw(linear, charge)?;
        }
        Ok(task)
    }
    pub(crate) fn denoise_diagnostics(&self) -> Option<DenoiseDiagnostics> {
        self.diagnostics
    }
    pub(crate) fn take_denoise_mask(&mut self) -> Option<Vec<u8>> {
        self.mask_png.take()
    }

    fn prepare_raw(&mut self, linear: Vec<[f32; 4]>, charge: &mut impl FnMut(u64) -> Result<()>) -> Result<()> {
        if self.retain_exr {
            if self.supersampling == 2 {
                self.stage = Some(Stage::Resolve(Resolve { source: linear, pixels: Vec::new(), raw: true }));
            } else {
                self.start_exr(linear, charge)?;
            }
        } else {
            self.prepare_display(linear, charge)?;
        }
        Ok(())
    }
    fn start_exr(&mut self, linear: Vec<[f32; 4]>, charge: &mut impl FnMut(u64) -> Result<()>) -> Result<()> {
        charge(u64::from(self.height))?;
        self.stage = Some(Stage::Exr(ExrTask::new_cpu(self.width, self.height, linear, self.cancel.clone())?));
        Ok(())
    }
    fn pending(&self) -> TaskPoll {
        TaskPoll::Pending(Progress { phase: Phase::Encoding, done: 0, total: u64::from(self.width) * u64::from(self.height) })
    }
    fn start_png(&mut self, charge: &mut impl FnMut(u64) -> Result<()>) -> Result<()> {
        self.cancel.check()?;
        charge(1)?;
        let size = usize::try_from(self.encoded_limit).map_err(|_| invalid("CPU PNG output exceeds address space"))?;
        let mut output = Vec::new();
        output.try_reserve_exact(size).map_err(|_| JobError::new(ErrorCode::AllocationFailed, "CPU presentation PNG allocation failed"))?;
        if output.capacity() != size {
            return Err(invalid("CPU presentation PNG capacity exceeds admission"));
        }
        self.png = Some(PngEncoding::with_output(self.width, self.height, self.encoded_limit, self.cancel.clone(), output)?);
        self.stage = Some(Stage::Png);
        Ok(())
    }
    fn prepare_display(&mut self, linear: Vec<[f32; 4]>, charge: &mut impl FnMut(u64) -> Result<()>) -> Result<()> {
        if self.denoise {
            charge(1)?; // one fallible, prepaid colour scratch reservation
            let task = if self.denoise_settings.is_default() {
                DenoiseTask::new(self.trace_width, self.trace_height, linear)?
            } else {
                DenoiseTask::new_with_settings(self.trace_width, self.trace_height, linear, self.denoise_settings)?
            };
            self.stage = Some(Stage::Denoise(task));
            Ok(())
        } else {
            self.prepare_resolve(linear, charge)
        }
    }
    fn prepare_resolve(&mut self, linear: Vec<[f32; 4]>, charge: &mut impl FnMut(u64) -> Result<()>) -> Result<()> {
        if self.supersampling == 2 {
            self.stage = Some(Stage::Resolve(Resolve { source: linear, pixels: Vec::new(), raw: false }));
            Ok(())
        } else {
            self.prepare_finishing(linear, charge)
        }
    }
    fn prepare_finishing(&mut self, linear: Vec<[f32; 4]>, charge: &mut impl FnMut(u64) -> Result<()>) -> Result<()> {
        if self.preserve_display {
            drop(linear);
            return self.start_png(charge);
        }
        if let Some(bloom) = self.presentation.finishing.bloom.filter(|b| b.active()) {
            charge(u64::from(self.width) * u64::from(self.height) * 2)?;
            self.stage = Some(Stage::Bloom(BloomTask::new(self.width, self.height, linear, bloom, self.cancel.clone())?));
        } else {
            self.linear = Some(linear);
            self.stage = Some(Stage::Display(0));
        }
        Ok(())
    }
    /// One bounded row/column or at most 256 pixels. The CPU caller limits calls per quantum.
    pub(crate) fn step(
        &mut self,
        guides: Option<&[Guide]>,
        counts: Option<&CountsPlane>,
        charge: &mut impl FnMut(u64) -> Result<()>,
    ) -> Result<TaskPoll> {
        self.step_with_diagnostics(guides, None, counts, charge)
    }
    pub(crate) fn step_with_diagnostics(
        &mut self,
        guides: Option<&[Guide]>,
        classes: Option<&[DenoiseGuideClass]>,
        counts: Option<&CountsPlane>,
        charge: &mut impl FnMut(u64) -> Result<()>,
    ) -> Result<TaskPoll> {
        self.cancel.check()?;
        let stage = self.stage.take().ok_or_else(|| invalid("CPU output stage disappeared"))?;
        match stage {
            Stage::Diagnostics(at) => {
                let count = self.width as usize * self.height as usize;
                let end = at.saturating_add(256).min(count);
                charge(((end - at) as u64) * u64::from(self.supersampling).pow(2))?;
                reserve_exact(&mut self.mask_rgba, count * 4)?;
                let classes = classes.ok_or_else(|| invalid("CPU diagnostic guide classes disappeared"))?;
                let linear = self.linear.as_deref().ok_or_else(|| invalid("CPU diagnostic radiance disappeared"))?;
                let report = self.diagnostics.as_mut().ok_or_else(|| invalid("CPU diagnostic report disappeared"))?;
                for pixel in at..end {
                    let (indices, samples) = super::quality::supersample_indices(self.width, self.supersampling, pixel)?;
                    let mut resolved = [DenoiseGuideClass::Unavailable; 4];
                    for (slot, index) in resolved.iter_mut().zip(&indices[..samples]) {
                        let mut class = *classes.get(*index).ok_or_else(|| invalid("CPU diagnostic guide index outside grid"))?;
                        let alpha = linear.get(*index).ok_or_else(|| invalid("CPU diagnostic coverage outside grid"))?[3];
                        if class.eligible() && alpha < 0.999 {
                            class = DenoiseGuideClass::PartialCoverage;
                        }
                        report.record(class);
                        *slot = class;
                    }
                    let class = super::resolve_denoise_class(resolved[..samples].iter().copied());
                    self.mask_rgba.extend_from_slice(&class.color());
                }
                if end == count {
                    self.png = Some(PngEncoding::new(self.width, self.height, self.encoded_limit, self.cancel.clone())?);
                    self.stage = Some(Stage::MaskPng);
                } else {
                    self.stage = Some(Stage::Diagnostics(end));
                }
            }
            Stage::MaskPng => {
                let task = self.png.as_mut().ok_or_else(|| invalid("CPU diagnostic PNG encoder disappeared"))?;
                charge(if task.row() < self.height as usize { u64::from(self.width) } else { 1 })?;
                if let Some(png) = task.step(&self.mask_rgba)? {
                    charge(png.len() as u64)?;
                    self.mask_png = Some(png);
                    self.png = None;
                    self.mask_rgba = Vec::new();
                    let linear = self.linear.take().ok_or_else(|| invalid("CPU diagnostic raw pass disappeared"))?;
                    self.prepare_raw(linear, charge)?;
                } else {
                    self.stage = Some(Stage::MaskPng);
                }
            }
            Stage::Resolve(mut task) => {
                let count = self.width as usize * self.height as usize;
                let at = task.pixels.len();
                let end = at.saturating_add(256).min(count);
                charge(((end - at) * 4) as u64)?;
                reserve_exact(&mut task.pixels, count)?;
                if !task.raw && self.presentation.transparent_background {
                    reserve_exact(&mut self.resolved_alpha, count)?;
                }
                for pixel in at..end {
                    task.pixels.push(super::quality::resolve_pixel(self.width, self.supersampling, &task.source, pixel)?);
                    if !task.raw && self.presentation.transparent_background {
                        self.resolved_alpha.push(resolved_coverage_alpha(
                            self.width,
                            self.supersampling,
                            pixel,
                            &task.source,
                            counts,
                            self.coverage_samples,
                        )?);
                    }
                }
                if end == count {
                    if task.raw {
                        self.original = Some(task.source);
                        self.start_exr(task.pixels, charge)?;
                    } else {
                        drop(task.source);
                        self.prepare_finishing(task.pixels, charge)?;
                    }
                } else {
                    self.stage = Some(Stage::Resolve(task));
                }
            }
            Stage::Exr(mut task) => {
                charge(u64::from(self.width))?;
                if task.step(1)? {
                    let (bytes, pixels) = task.finish()?;
                    charge(bytes.len() as u64)?; // host's later digest scan
                    self.exr = Some(bytes);
                    let pixels = match self.original.take() {
                        Some(original) => original,
                        None => pixels,
                    };
                    self.prepare_display(pixels, charge)?;
                } else {
                    self.stage = Some(Stage::Exr(task));
                }
            }
            Stage::Denoise(mut task) => {
                charge(task.next_pixels() as u64)?;
                let guides = guides.ok_or_else(|| invalid("CPU output is missing its admitted geometry guides"))?;
                if task.step(guides, &self.cancel)? {
                    self.prepare_resolve(task.finish()?, charge)?;
                } else {
                    self.stage = Some(Stage::Denoise(task));
                }
            }
            Stage::Bloom(mut task) => {
                charge(task.next_line_pixels())?;
                if task.step(1)? {
                    let (pixels, glow) = task.finish()?;
                    self.linear = Some(pixels);
                    self.glow = Some(glow);
                    self.stage = Some(Stage::Display(0));
                } else {
                    self.stage = Some(Stage::Bloom(task));
                }
            }
            Stage::Display(at) => {
                let count = self.width as usize * self.height as usize;
                let end = at.saturating_add(256).min(count);
                charge((end - at) as u64)?;
                let length = count.checked_mul(4).ok_or_else(|| invalid("CPU display length overflow"))?;
                if self.rgba.capacity() != length {
                    self.rgba
                        .try_reserve_exact(length.saturating_sub(self.rgba.len()))
                        .map_err(|_| JobError::new(ErrorCode::AllocationFailed, "CPU display allocation failed"))?;
                    if self.rgba.capacity() != length {
                        return Err(invalid("CPU display capacity exceeds admission"));
                    }
                }
                self.rgba.resize(end * 4, 0);
                let input = self.linear.as_deref().and_then(|v| v.get(at..end)).ok_or_else(|| invalid("CPU linear display pixels disappeared"))?;
                let output = self.rgba.get_mut(at * 4..end * 4).ok_or_else(|| invalid("CPU display pixels disappeared"))?;
                let glow = self.glow.as_deref().and_then(|v| v.get(at..end));
                self.presentation.display(self.width, self.height)?.encode_bloom_at(input, output, &self.cancel, at, glow)?;
                if self.presentation.transparent_background {
                    // Canonical fully clear pixels cannot carry lifted-black RGB into
                    // edge filtering. Raw linear coverage/radiance remains unchanged.
                    for (offset, (pixel, rgba)) in input.iter().zip(output.as_chunks_mut::<4>().0).enumerate() {
                        rgba[3] = if self.supersampling == 2 {
                            *self.resolved_alpha.get(at + offset).ok_or_else(|| invalid("CPU resolved coverage disappeared"))?
                        } else {
                            match counts {
                                Some(counts) => {
                                    let exact = counts.pixel(at + offset)?;
                                    if exact.samples() == 0 || (f64::from(exact.covered()) / f64::from(exact.samples())) as f32 != pixel[3] {
                                        return Err(invalid("CPU adaptive coverage disagrees with its complete count plane"));
                                    }
                                    exact.alpha8()?
                                }
                                None => coverage_alpha(pixel[3], self.coverage_samples)?,
                            }
                        };
                        if rgba[3] == 0 {
                            *rgba = [0; 4];
                        }
                    }
                }
                if end == count {
                    self.glow = None;
                    self.resolved_alpha = Vec::new();
                    if self.presentation.finishing.photographic.active() {
                        let pixels = self.linear.take().ok_or_else(|| invalid("CPU photographic source disappeared"))?;
                        self.stage = Some(Stage::Photographic(PhotographicTask::new(
                            self.width,
                            self.height,
                            std::mem::take(&mut self.rgba),
                            pixels,
                            self.presentation.finishing.photographic,
                            self.cancel.clone(),
                        )?));
                    } else {
                        self.linear = None;
                        self.start_png(charge)?;
                    }
                } else {
                    self.stage = Some(Stage::Display(end));
                }
            }
            Stage::Photographic(mut task) => {
                charge(task.next_step_pixels(256)?)?;
                if task.step(256)? {
                    self.rgba = task.finish()?;
                    self.start_png(charge)?;
                } else {
                    self.stage = Some(Stage::Photographic(task));
                }
            }
            Stage::Png => {
                let task = self.png.as_mut().ok_or_else(|| invalid("CPU PNG encoder disappeared"))?;
                charge(if task.row() < self.height as usize { u64::from(self.width) } else { 1 })?;
                if let Some(png) = task.step(&self.rgba)? {
                    charge(png.len() as u64)?; // host's later digest scan
                    self.png = None;
                    self.stage = Some(Stage::Done);
                    return Ok(TaskPoll::Complete(png));
                }
                self.stage = Some(Stage::Png);
            }
            Stage::Done => return Err(invalid("CPU output already completed")),
        }
        self.cancel.check()?;
        Ok(self.pending())
    }
    #[cfg(test)]
    pub(crate) fn stage_name(&self) -> &'static str {
        match &self.stage {
            Some(Stage::Diagnostics(_)) => "diagnostics",
            Some(Stage::MaskPng) => "denoise_mask",
            Some(Stage::Resolve(_)) => "resolve",
            Some(Stage::Exr(_)) => "exr",
            Some(Stage::Denoise(_)) => "denoise",
            Some(Stage::Bloom(_)) => "bloom",
            Some(Stage::Display(_)) => "display",
            Some(Stage::Photographic(_)) => "photographic",
            Some(Stage::Png) => "png",
            Some(Stage::Done) => "done",
            None => "empty",
        }
    }
    pub(crate) fn finish(self) -> Result<(Vec<u8>, Option<Vec<u8>>)> {
        self.cancel.check()?;
        if !matches!(self.stage, Some(Stage::Done)) {
            return Err(invalid("CPU output is incomplete"));
        }
        Ok((self.rgba, self.exr))
    }
}

fn reserve_exact<T>(values: &mut Vec<T>, count: usize) -> Result<()> {
    if values.capacity() == 0 {
        values.try_reserve_exact(count).map_err(|_| JobError::new(ErrorCode::AllocationFailed, "CPU output scratch allocation failed"))?;
    }
    if values.capacity() != count {
        return Err(invalid("CPU output scratch capacity exceeds admission"));
    }
    Ok(())
}

/// Reconstruct alpha from exact subpixel coverage ratios, including adaptive
/// denominators. Four admitted u32 sample counts fit the u128 rational sum.
fn resolved_coverage_alpha(width: u32, scale: u32, pixel: usize, linear: &[[f32; 4]], counts: Option<&CountsPlane>, samples: u32) -> Result<u8> {
    let (indices, count) = super::quality::supersample_indices(width, scale, pixel)?;
    let (mut numerator, mut denominator) = (0_u128, 1_u128);
    for index in &indices[..count] {
        let alpha = linear.get(*index).ok_or_else(|| invalid("CPU resolved coverage source disappeared"))?[3];
        let (covered, samples) = match counts {
            Some(counts) => {
                let exact = counts.pixel(*index)?;
                (exact.covered(), exact.samples())
            }
            None => {
                if samples == 0 || samples > super::MAX_SAMPLES_PER_PIXEL || !alpha.is_finite() || !(0.0..=1.0).contains(&alpha) {
                    return Err(invalid("CPU resolved coverage sample count invalid"));
                }
                ((f64::from(alpha) * f64::from(samples)).round() as u32, samples)
            }
        };
        if samples == 0 || samples > super::MAX_SAMPLES_PER_PIXEL || covered > samples || (f64::from(covered) / f64::from(samples)) as f32 != alpha {
            return Err(invalid("CPU resolved coverage disagrees with its complete count plane"));
        }
        numerator = numerator * u128::from(samples) + u128::from(covered) * denominator;
        denominator *= u128::from(samples);
    }
    denominator *= count as u128;
    u8::try_from((numerator * 510 + denominator) / (denominator * 2)).map_err(|_| invalid("CPU resolved alpha exceeds one byte"))
}
