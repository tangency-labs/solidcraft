//! Bounded presentation of an owned scene-linear CPU pass. Transport and film remain separate.
//! The caller supplies the cumulative work debit; every mutating stage checks it first.
use super::denoise::{DenoiseTask, Guide};
use super::{CancelToken, ErrorCode, JobError, Phase, Progress, Result, TaskPoll};
use super::{bloom::BloomTask, encoding::PngEncoding, linear_exr::ExrTask, photographic::PhotographicTask, presentation::Presentation};
use crate::film::adaptive::CountsPlane;

fn invalid(message: &str) -> JobError {
    JobError::new(ErrorCode::InvalidRequest, message)
}

fn coverage_alpha(coverage: f32, samples: u16) -> Result<u8> {
    if samples == 0 || !coverage.is_finite() || !(0.0..=1.0).contains(&coverage) {
        return Err(invalid("invalid CPU display coverage count"));
    }
    // Every CPU pass has one known denominator. Even at u16::MAX, float32
    // quantisation is far below half a hit, so the original count is unique.
    // Keep the raw FLOAT32 plane unchanged and round only the final byte from
    // the integer ratio: 5/6 rounds to 213; float32 alpha alone gives 212.
    let covered = (f64::from(coverage) * f64::from(samples)).round() as u32;
    if covered > u32::from(samples) || (f64::from(covered) / f64::from(samples)) as f32 != coverage {
        return Err(invalid("CPU display coverage disagrees with its sample count"));
    }
    let denominator = u32::from(samples);
    u8::try_from((covered * 510 + denominator) / (denominator * 2)).map_err(|_| invalid("CPU display alpha exceeds one byte"))
}

enum Stage {
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
    coverage_samples: u16,
    exr: Option<Vec<u8>>,
}
impl OutputTask {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        width: u32,
        height: u32,
        encoded_limit: u64,
        linear: Vec<[f32; 4]>,
        rgba: Vec<u8>,
        presentation: Presentation,
        preserve_display: bool,
        retain_exr: bool,
        denoise: bool,
        coverage_samples: u16,
        cancel: CancelToken,
        charge: &mut impl FnMut(u64) -> Result<()>,
    ) -> Result<Self> {
        presentation.validate()?;
        cancel.check()?;
        if denoise && preserve_display {
            return Err(invalid("CPU denoising cannot preserve an unfiltered display buffer"));
        }
        if coverage_samples == 0 {
            return Err(invalid("CPU output requires a complete coverage sample count"));
        }
        let count = u64::from(width) * u64::from(height);
        super::linear_exr::encoded_limit(width, height)?;
        if linear.len() as u64 != count || (preserve_display && rgba.len() as u64 != count * 4) || (!preserve_display && !rgba.is_empty()) {
            return Err(invalid("CPU output storage does not match its complete pass"));
        }
        let mut task = Self {
            width,
            height,
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
        if retain_exr {
            charge(u64::from(height))?; // bounded offset-table construction
            task.stage = Some(Stage::Exr(ExrTask::new_cpu(width, height, linear, task.cancel.clone())?));
        } else {
            task.prepare_display(linear, charge)?;
        }
        Ok(task)
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
            self.stage = Some(Stage::Denoise(DenoiseTask::new(self.width, self.height, linear)?));
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
        self.cancel.check()?;
        let stage = self.stage.take().ok_or_else(|| invalid("CPU output stage disappeared"))?;
        match stage {
            Stage::Exr(mut task) => {
                charge(u64::from(self.width))?;
                if task.step(1)? {
                    let (bytes, pixels) = task.finish()?;
                    charge(bytes.len() as u64)?; // host's later digest scan
                    self.exr = Some(bytes);
                    self.prepare_display(pixels, charge)?;
                } else {
                    self.stage = Some(Stage::Exr(task));
                }
            }
            Stage::Denoise(mut task) => {
                charge(task.next_pixels() as u64)?;
                let guides = guides.ok_or_else(|| invalid("CPU output is missing its admitted geometry guides"))?;
                if task.step(guides, &self.cancel)? {
                    self.prepare_finishing(task.finish()?, charge)?;
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
                        rgba[3] = match counts {
                            Some(counts) => {
                                let exact = counts.pixel(at + offset)?;
                                if exact.samples() == 0 || (f64::from(exact.covered()) / f64::from(exact.samples())) as f32 != pixel[3] {
                                    return Err(invalid("CPU adaptive coverage disagrees with its complete count plane"));
                                }
                                exact.alpha8()?
                            }
                            None => coverage_alpha(pixel[3], self.coverage_samples)?,
                        };
                        if rgba[3] == 0 {
                            *rgba = [0; 4];
                        }
                    }
                }
                if end == count {
                    self.glow = None;
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
