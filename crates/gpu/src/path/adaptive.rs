//! Read back only bounded allocation metadata; radiance remains in the existing
//! film and source leases. The report counts work, not certified image error.
use super::*;
use solidcraft_render::job::adaptive::{PilotEvidenceLayout, PilotEvidenceReport, PilotPixelEvidence, SamplingReport};

pub(super) fn evidence_planes(options: JobOptions) -> u32 {
    options.adaptive.map_or(0, |adaptive| PilotEvidenceLayout::new(adaptive).gpu_planes())
}

fn internal(message: &str) -> JobError {
    JobError::new(ErrorCode::Internal, message)
}

fn production_count(options: &JobOptions, bytes: Option<&[u8]>) -> Result<u32> {
    let adaptive = options.adaptive.ok_or_else(|| internal("missing adaptive preview options"))?;
    let bytes: [u8; 4] = bytes.and_then(|bytes| bytes.try_into().ok()).ok_or_else(|| internal("missing adaptive preview budget"))?;
    let value = f32::from_le_bytes(bytes);
    if !value.is_finite() || value.fract() != 0.0 || value < adaptive.min_samples as f32 || value > options.samples as f32 {
        return Err(internal("invalid adaptive preview production count"));
    }
    Ok(value as u32)
}
pub(super) fn preview_divisor(options: &JobOptions, completed: u32, budget: Option<&[u8]>) -> Result<f32> {
    let adaptive = options.adaptive.ok_or_else(|| internal("missing adaptive preview options"))?;
    let total = options.samples.checked_add(adaptive.pilot_samples).ok_or_else(|| internal("adaptive preview target overflow"))?;
    if completed == 0 || completed > total {
        return Err(internal("adaptive preview iteration exceeds its target"));
    }
    if completed <= adaptive.pilot_samples {
        return Ok(1.0);
    }
    let production = completed - adaptive.pilot_samples;
    let target = production_count(options, budget)?;
    // The shader divides a finished pixel exactly once, then leaves that mean alone.
    Ok(if production < target { production as f32 } else { 1.0 })
}
pub(super) fn preview_paths(task: &Task, pixels: u64, budgets: Option<&[u8]>) -> Result<u64> {
    let options = task.options.adaptive.ok_or_else(|| internal("missing adaptive preview options"))?;
    if task.sample <= options.pilot_samples {
        return Ok(pixels * u64::from(task.sample));
    }
    let production = task.sample - options.pilot_samples;
    let mut paths = pixels * u64::from(options.pilot_samples);
    let bytes = budgets.ok_or_else(|| internal("missing adaptive preview budgets"))?;
    for (index, pixel) in bytes.as_chunks::<16>().0.iter().enumerate() {
        if index % 4096 == 0 {
            task.cancel.check()?;
        }
        paths += u64::from(production.min(production_count(&task.options, pixel.get(..4))?)) * u64::from(task.options.production_lanes());
    }
    Ok(paths)
}

pub(super) fn report(task: &Task) -> Result<SamplingReport> {
    task.cancel.check()?;
    task.context.ensure_available()?;
    let pixels = u64::from(task.request.width) * u64::from(task.request.height);
    let mut report = SamplingReport {
        pilot_samples_per_pixel: task.options.adaptive.map_or(0, |a| a.pilot_samples),
        minimum_production_samples: task.options.samples,
        maximum_production_samples: task.options.samples,
        production_samples: pixels * u64::from(task.options.samples),
        primary_path_evaluations: pixels * u64::from(task.options.samples) * u64::from(task.options.production_lanes()),
        pixels_at_cap: pixels,
        budget_limited_pixels: 0,
        transmission_strata: task.options.transmission_samples,
        split_first_interface: task.options.split_first_interface,
        pilot_evidence: task
            .options
            .adaptive
            .filter(|a| a.evidence_enabled())
            .map(|a| PilotEvidenceReport::new(a, task.options.caustics))
            .transpose()?,
    };
    let Some(options) = task.options.adaptive else { return Ok(report) };
    report.maximum_production_samples = 0;
    report.production_samples = 0;
    report.pixels_at_cap = 0;
    for page in &task.pages {
        let bytes = u64::from(page.pixels) * 16;
        task.readback_data(
            bytes,
            |encoder| {
                encoder.copy_buffer_to_buffer(&page.accumulation, bytes * 2, &task.readback, 0, bytes);
                Ok(())
            },
            |data| {
                for (index, record) in data.as_chunks::<16>().0.iter().enumerate() {
                    if index % 4096 == 0 {
                        task.cancel.check()?;
                    }
                    let value = f32::from_le_bytes([record[0], record[1], record[2], record[3]]);
                    if !value.is_finite() || value.fract() != 0.0 || value < options.min_samples as f32 || value > task.options.samples as f32 {
                        return Err(JobError::new(ErrorCode::Internal, "invalid GPU adaptive production count"));
                    }
                    let count = value as u32;
                    report.minimum_production_samples = report.minimum_production_samples.min(count);
                    report.maximum_production_samples = report.maximum_production_samples.max(count);
                    report.production_samples += u64::from(count);
                    report.pixels_at_cap += u64::from(count == task.options.samples);
                    let limited = f32::from_le_bytes([record[4], record[5], record[6], record[7]]);
                    if limited != 0.0 && limited != 1.0 {
                        return Err(JobError::new(ErrorCode::Internal, "invalid GPU adaptive budget-limit flag"));
                    }
                    report.budget_limited_pixels += u64::from(limited == 1.0);
                }
                Ok(())
            },
        )?;
    }
    if let Some(evidence) = report.pilot_evidence.as_mut() {
        read_evidence(task, evidence)?;
    }
    report.primary_path_evaluations =
        pixels * u64::from(report.pilot_samples_per_pixel) + report.production_samples * u64::from(report.production_lanes());
    task.cancel.check()?;
    Ok(report)
}

fn read_evidence(task: &Task, report: &mut PilotEvidenceReport) -> Result<()> {
    let options = task.options.adaptive.ok_or_else(|| internal("pilot evidence requires adaptive options"))?;
    let layout = PilotEvidenceLayout::new(options);
    let base = super::radiance::base(task.options) + super::radiance::planes(task.options) + layout.moment_planes();
    for page in &task.pages {
        let bytes = u64::from(page.pixels) * 16;
        task.readback_data(
            bytes * 2,
            |encoder| {
                encoder.copy_buffer_to_buffer(&page.accumulation, u64::from(base) * bytes, &task.readback, 0, bytes);
                encoder.copy_buffer_to_buffer(&page.accumulation, u64::from(base + 2) * bytes, &task.readback, bytes, bytes);
                Ok(())
            },
            |data| {
                let (supports, diagnostics) = data.split_at(bytes as usize);
                for (index, (support, diagnostic)) in supports.as_chunks::<16>().0.iter().zip(diagnostics.as_chunks::<16>().0).enumerate() {
                    if index % 4096 == 0 {
                        task.cancel.check()?;
                    }
                    let decode = |record: &[u8; 16]| -> [f64; 4] {
                        std::array::from_fn(|channel| {
                            f64::from(f32::from_le_bytes([
                                record[channel * 4],
                                record[channel * 4 + 1],
                                record[channel * 4 + 2],
                                record[channel * 4 + 3],
                            ]))
                        })
                    };
                    let support = decode(support);
                    let diagnostic = decode(diagnostic);
                    if support.into_iter().chain(diagnostic).any(|value| !value.is_finite() || value < 0.0)
                        || support[0].fract() != 0.0
                        || support[0] > f64::from(options.pilot_samples)
                        || support[1].fract() != 0.0
                        || support[1] > support[0]
                        || !matches!(diagnostic[0], 0.0 | 1.0)
                        || !matches!(diagnostic[2], 0.0 | 1.0)
                    {
                        return Err(internal("invalid bounded GPU pilot evidence"));
                    }
                    report.observe_pixel(PilotPixelEvidence {
                        component_budget_limited: diagnostic[0] == 1.0,
                        map_scatter_to_tolerance: diagnostic[1],
                        invalid: diagnostic[2] == 1.0,
                        query_observations: support[0] as u32,
                        empty_observations: support[1] as u32,
                        minimum_query_effective_event_support: (support[2] > 0.0).then_some(support[2]),
                    });
                }
                Ok(())
            },
        )?;
    }
    Ok(())
}
