//! Read back only bounded allocation metadata; radiance remains in the existing
//! film and source leases. The report counts work, not certified image error.
use super::*;
use solidcraft_render::job::adaptive::SamplingReport;

pub(super) fn report(task: &Task) -> Result<SamplingReport> {
    task.cancel.check()?;
    task.context.ensure_available()?;
    let pixels = u64::from(task.request.width) * u64::from(task.request.height);
    let mut report = SamplingReport {
        pilot_samples_per_pixel: task.options.adaptive.map_or(0, |a| a.pilot_samples),
        minimum_production_samples: task.options.samples,
        maximum_production_samples: task.options.samples,
        production_samples: pixels * u64::from(task.options.samples),
        primary_path_evaluations: pixels * u64::from(task.options.samples) * u64::from(task.options.transmission_samples),
        pixels_at_cap: pixels,
        transmission_strata: task.options.transmission_samples,
    };
    let Some(options) = task.options.adaptive else { return Ok(report) };
    let bytes = pixels * 16;
    let context = &task.context;
    let scopes = scopes(&context.device);
    let mut encoder = context.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("adaptive budget report") });
    encoder.copy_buffer_to_buffer(&task.accumulation, bytes * 2, &task.readback, 0, bytes);
    let submission = context.queue.submit([encoder.finish()]);
    checked(scopes)?;
    let slice = task.readback.slice(0..bytes);
    let (tx, rx) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |result| {
        let _ = tx.send(result);
    });
    context.wait(submission, TIMEOUT)?;
    rx.try_recv().map_err(gpu)?.map_err(gpu)?;
    let result = (|| {
        let data = slice.get_mapped_range().map_err(gpu)?;
        report.maximum_production_samples = 0;
        report.production_samples = 0;
        report.pixels_at_cap = 0;
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
        }
        report.primary_path_evaluations =
            pixels * u64::from(report.pilot_samples_per_pixel) + report.production_samples * u64::from(report.transmission_strata);
        task.cancel.check()?;
        Ok(report)
    })();
    task.readback.unmap();
    result
}
