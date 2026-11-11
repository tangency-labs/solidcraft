//! Sequential reconstruction of six exclusive radiance components.
//! One component owns the existing filter scratch; recombination stays linear.
use super::{CameraPath, DenoiseSettings, DenoiseTask, Guide, MAX_STEP_PIXELS, UncertaintyPixel, checked_color, invalid};
use crate::job::{CancelToken, ErrorCode, JobError, Result};
use crate::radiance::COMPONENT_COUNT;
use crate::radiance_film::ComponentFrame;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Initialize,
    Load,
    Filter,
    Add,
    Done,
}

pub(crate) struct ComponentDenoiseTask {
    width: u32,
    height: u32,
    frame: ComponentFrame,
    sum: Vec<[f32; 4]>,
    colors: Vec<[f32; 4]>,
    uncertainty: Vec<UncertaintyPixel>,
    filter: Option<DenoiseTask>,
    settings: DenoiseSettings,
    component: usize,
    pixel: usize,
    phase: Phase,
}

impl ComponentDenoiseTask {
    /// The existing color ping-pong and admitted uncertainty plane remain in use.
    /// Added storage is the compact snapshot and one scene-linear sum plane.
    pub(crate) const EXTRA_BYTES_PER_PIXEL: u64 = ComponentFrame::BYTES_PER_PIXEL + 16;

    pub(crate) fn new(width: u32, height: u32, beauty: Vec<[f32; 4]>, frame: ComponentFrame, settings: DenoiseSettings) -> Result<Self> {
        settings.validate()?;
        let count = usize::try_from(u64::from(width) * u64::from(height)).map_err(|_| invalid("component filter dimensions overflow"))?;
        if count == 0 || frame.len() != count || beauty.len() != count || beauty.capacity() != count {
            return Err(invalid("component reconstruction needs a complete admitted frame"));
        }
        Ok(Self {
            width,
            height,
            frame,
            sum: beauty,
            colors: Vec::new(),
            uncertainty: Vec::new(),
            filter: None,
            settings,
            component: 0,
            pixel: 0,
            phase: Phase::Initialize,
        })
    }

    pub(crate) fn next_pixels(&self) -> usize {
        if let Some(filter) = &self.filter { filter.next_pixels() } else { self.sum.len().saturating_sub(self.pixel).min(MAX_STEP_PIXELS) }
    }

    pub(crate) fn step(&mut self, guides: &[Guide], cancel: &CancelToken) -> Result<bool> {
        cancel.check()?;
        if self.phase == Phase::Done || guides.len() != self.sum.len() {
            return Err(invalid("component filter guide count or phase is invalid"));
        }
        if self.phase == Phase::Filter {
            let filter = self.filter.as_mut().ok_or_else(|| invalid("component filter disappeared"))?;
            if filter.step(guides, cancel)? {
                self.colors = self.filter.take().ok_or_else(|| invalid("completed component filter disappeared"))?.finish()?;
                self.pixel = 0;
                self.phase = Phase::Add;
            }
            return Ok(false);
        }
        if self.phase == Phase::Load && self.pixel == 0 {
            self.colors.clear();
            reserve(&mut self.colors, self.sum.len())?;
            reserve(&mut self.uncertainty, self.sum.len())?;
        }
        let end = self.pixel + self.next_pixels();
        for index in self.pixel..end {
            cancel.check()?;
            match self.phase {
                Phase::Initialize => {
                    let (components, count) = self.frame.beauty(index)?;
                    let sum = self.sum.get_mut(index).ok_or_else(|| invalid("component sum pixel disappeared"))?;
                    checked_color(*sum)?;
                    if count != 0 {
                        for (expected, raw) in components.into_iter().zip(sum.iter().take(3)) {
                            if (expected - f64::from(*raw)).abs() > 2e-6 * f64::from(*raw).max(1.0) {
                                return Err(invalid("component display snapshot does not reproduce raw beauty"));
                            }
                        }
                        for value in sum.iter_mut().take(3) {
                            *value = 0.0;
                        }
                    }
                    // Independent adaptive pilots have no production components.
                    // Keep their original beauty until real observations exist.
                }
                Phase::Load => {
                    let (mean, uncertainty) = self.frame.pixel(index, self.component)?;
                    let alpha = self.sum.get(index).ok_or_else(|| invalid("component alpha disappeared"))?[3];
                    self.colors.push(checked_color([mean[0], mean[1], mean[2], alpha])?);
                    uncertainty.validate()?;
                    self.uncertainty.push(uncertainty);
                }
                Phase::Add => {
                    if self.frame.pixel(index, self.component)?.1.count != 0 {
                        let value = self.colors.get(index).copied().ok_or_else(|| invalid("filtered component pixel disappeared"))?;
                        let sum = self.sum.get_mut(index).ok_or_else(|| invalid("component sum pixel disappeared"))?;
                        for (sum, value) in sum.iter_mut().take(3).zip(value) {
                            *sum += value;
                        }
                        checked_color(*sum)?;
                    }
                }
                _ => return Err(invalid("component filter cursor bypassed its phase")),
            }
        }
        self.pixel = end;
        if self.pixel == self.sum.len() {
            self.pixel = 0;
            match self.phase {
                Phase::Initialize => self.phase = Phase::Load,
                Phase::Load => {
                    let settings = DenoiseSettings { variance_guided: true, component_filtering: false, ..self.settings };
                    let mut filter = DenoiseTask::new_with_uncertainty(
                        self.width,
                        self.height,
                        std::mem::take(&mut self.colors),
                        settings,
                        Some(std::mem::take(&mut self.uncertainty)),
                    )?;
                    filter.component = Some(match self.component / 2 {
                        0 => CameraPath::Other,
                        1 => CameraPath::Reflected,
                        2 => CameraPath::Transmitted,
                        _ => return Err(invalid("component filter partition index is invalid")),
                    });
                    self.filter = Some(filter);
                    self.phase = Phase::Filter;
                }
                Phase::Add => {
                    self.component += 1;
                    self.phase = if self.component == COMPONENT_COUNT { Phase::Done } else { Phase::Load };
                }
                _ => return Err(invalid("component filter completed an invalid phase")),
            }
        }
        cancel.check()?;
        Ok(self.phase == Phase::Done)
    }

    pub(crate) fn finish(self) -> Result<Vec<[f32; 4]>> {
        if self.phase != Phase::Done {
            return Err(invalid("component reconstruction is incomplete"));
        }
        Ok(self.sum)
    }
}

fn reserve<T>(values: &mut Vec<T>, count: usize) -> Result<()> {
    if values.capacity() == 0 {
        values.try_reserve_exact(count).map_err(|_| JobError::new(ErrorCode::AllocationFailed, "component reconstruction allocation failed"))?;
    }
    if values.capacity() != count || !values.is_empty() {
        return Err(invalid("component reconstruction capacity exceeds admission"));
    }
    Ok(())
}
