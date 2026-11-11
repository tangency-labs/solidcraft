//! GPU film statistics use the existing storage binding and row-page layout.
//! Readback stays bounded by one page, even for large exported images.
use super::{JobOptions, Result, Task, gpu};
use solidcraft_render::radiance::{COMPONENT_COUNT, UncertaintyPixel};
use solidcraft_render::radiance_film::RadianceReport;

pub(super) fn enabled(options: JobOptions) -> bool {
    options.radiance_components || options.denoise_settings.variance_guided
}
pub(super) fn planes(options: JobOptions) -> u32 {
    if !enabled(options) {
        0
    } else if options.radiance_components {
        27
    } else {
        3
    }
}
pub(super) fn base(options: JobOptions) -> u32 {
    if options.adaptive.is_some() { 3 } else { 1 }
}
pub(super) fn bytes(options: JobOptions, pixels: u64) -> Result<u64> {
    // Device moments, optional components and one host filter-uncertainty plane.
    pixels
        .checked_mul(u64::from(planes(options)) * 16 + if enabled(options) { if options.adaptive.is_some() { 16 } else { 32 } } else { 0 })
        .ok_or_else(|| gpu("GPU radiance statistics size overflow"))
}

pub(super) struct Snapshot {
    pub uncertainty: Vec<UncertaintyPixel>,
    pub report: RadianceReport,
}
fn vector(bytes: &[u8]) -> Result<[f32; 4]> {
    if bytes.len() != 16 {
        return Err(gpu("GPU radiance moment has invalid size"));
    }
    let mut values = [0.0; 4];
    for (value, chunk) in values.iter_mut().zip(bytes.as_chunks::<4>().0) {
        *value = f32::from_le_bytes(*chunk);
        if !value.is_finite() || *value < 0.0 {
            return Err(gpu("GPU radiance statistics are nonfinite or negative"));
        }
    }
    Ok(values)
}
pub(super) fn read(task: &Task) -> Result<Snapshot> {
    if !enabled(task.options) {
        return Err(gpu("GPU camera statistics were not requested"));
    }
    let count = task.request.width * task.request.height;
    let mut uncertainty = super::scene::reserved(count as usize)?;
    let mut report = RadianceReport {
        pixels: u64::from(count),
        observations: 0,
        minimum_samples: u64::MAX,
        maximum_samples: 0,
        mean_radiance_sum: [0.0; 3],
        component_radiance_sum: task.options.radiance_components.then_some([[0.0; 3]; COMPONENT_COUNT]),
        maximum_recombination_error: 0.0,
        uncertainty: "camera-observation scatter; shared photons, changing radii and low-discrepancy dependence are not convergence bounds",
    };
    let first = base(task.options);
    for page in &task.pages {
        let bytes = u64::from(page.pixels) * 16;
        task.readback_data(
            bytes * 2,
            |encoder| {
                encoder.copy_buffer_to_buffer(&page.accumulation, u64::from(first) * bytes, &task.readback, 0, bytes * 2);
                Ok(())
            },
            |data| {
                let (means, moments) = data.split_at(bytes as usize);
                for (index, (mean, m2)) in means.as_chunks::<16>().0.iter().zip(moments.as_chunks::<16>().0).enumerate() {
                    if index % 4096 == 0 {
                        task.cancel.check()?;
                    }
                    let mean = vector(mean)?;
                    let m2 = vector(m2)?;
                    if mean[3].fract() != 0.0 || mean[3] > task.options.samples as f32 {
                        return Err(gpu("GPU radiance observation count exceeds the production budget"));
                    }
                    let samples = mean[3] as u32;
                    if !task.encoding_preview {
                        let minimum =
                            task.options.adaptive.map_or(task.options.samples, |adaptive| adaptive.for_optical_scene(task.optical_scene).min_samples);
                        if samples < minimum || (task.options.adaptive.is_none() && samples != task.options.samples) {
                            return Err(gpu("GPU radiance statistics are missing complete production observations"));
                        }
                    }
                    let divisor = f64::from(samples) * f64::from(samples.saturating_sub(1));
                    let variance_of_mean =
                        if samples < 2 { [0.0; 3] } else { std::array::from_fn(|channel| (f64::from(m2[channel]) / divisor) as f32) };
                    let pixel = UncertaintyPixel { variance_of_mean, count: samples };
                    pixel.validate()?;
                    uncertainty.push(pixel);
                    report.observations =
                        report.observations.checked_add(u64::from(samples)).ok_or_else(|| gpu("GPU statistics sample count overflow"))?;
                    report.minimum_samples = report.minimum_samples.min(u64::from(samples));
                    report.maximum_samples = report.maximum_samples.max(u64::from(samples));
                    report.maximum_recombination_error = report.maximum_recombination_error.max(f64::from(m2[3]));
                    for (sum, value) in report.mean_radiance_sum.iter_mut().zip(mean) {
                        *sum += f64::from(value);
                    }
                }
                Ok(())
            },
        )?;
        if let Some(sums) = &mut report.component_radiance_sum {
            for (component, sum) in sums.iter_mut().enumerate() {
                // Component scatter can overflow independently of beauty.
                // Reuse the admitted two-plane staging buffer to validate each
                // mean/M2 pair before reporting or component reconstruction.
                task.readback_data(
                    bytes * 2,
                    |encoder| {
                        encoder.copy_buffer_to_buffer(&page.accumulation, u64::from(first + 3 + component as u32) * bytes, &task.readback, 0, bytes);
                        encoder.copy_buffer_to_buffer(
                            &page.accumulation,
                            u64::from(first + 9 + component as u32) * bytes,
                            &task.readback,
                            bytes,
                            bytes,
                        );
                        Ok(())
                    },
                    |data| {
                        let (means, moments) = data.split_at(bytes as usize);
                        for (index, (mean, m2)) in means.as_chunks::<16>().0.iter().zip(moments.as_chunks::<16>().0).enumerate() {
                            if index % 4096 == 0 {
                                task.cancel.check()?;
                            }
                            let _ = vector(m2)?;
                            for (sum, value) in sum.iter_mut().zip(vector(mean)?) {
                                *sum += f64::from(value);
                            }
                        }
                        Ok(())
                    },
                )?;
            }
        }
    }
    if uncertainty.len() != count as usize {
        return Err(gpu("GPU radiance statistics do not cover the image"));
    }
    Ok(Snapshot { uncertainty, report })
}
