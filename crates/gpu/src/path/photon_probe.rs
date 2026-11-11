//! Fixed-size GPU photon-probe upload and readback. No full map readback is needed.
use super::*;
use solidcraft_render::job::photon_diagnostics::{
    EmissionMeasure, PhotonCameraAccounting, PhotonContribution, PhotonDiagnostics, PhotonEstimatorPartition, PhotonSampleAccounting,
};
use solidcraft_render::{job::photon_support::PhotonContributionSupport, radiance::CameraPath};

pub(super) const WORDS: u32 = 1024;
pub(super) const BYTES: u64 = WORDS as u64 * 4;
const RECORD_BASE: usize = 128;
const RECORD_WORDS: usize = 40;
const PIXEL_BASE: usize = 768;
const PIXEL_WORDS: usize = 64;

pub(super) fn upload(context: &Context, buffer: &wgpu::Buffer, options: CausticOptions) {
    let Some([x, y]) = options.probe_pixel else { return };
    let mut bytes = [0u8; BYTES as usize];
    let fields = [
        (0, 1),
        (1, x),
        (2, y),
        (3, options.probe_sample),
        (6, options.seed),
        (7, (options.radius_mm as f32).to_bits()),
        (8, (options.radius_mm as f32).to_bits()),
        (17, 1),
        (18, 1),
    ];
    for (index, word) in fields {
        bytes[index * 4..index * 4 + 4].copy_from_slice(&word.to_le_bytes());
    }
    context.queue.write_buffer(buffer, buffer.size() - (BYTES + u64::from(BOUNDS_WORDS) * 4), &bytes);
}

pub(super) fn configure(context: &Context, buffer: &wgpu::Buffer, options: CausticOptions, scale: u32, samples: u32) -> Result<()> {
    if options.probe_pixel.is_none() {
        return Ok(());
    }
    if !matches!(scale, 1 | 2) || samples == 0 {
        return Err(gpu("photon camera probe has an invalid output layout or production budget"));
    }
    let mut bytes = [0u8; 8];
    bytes[..4].copy_from_slice(&scale.to_le_bytes());
    bytes[4..].copy_from_slice(&samples.to_le_bytes());
    let offset = buffer.size() - (BYTES + u64::from(BOUNDS_WORDS) * 4) + 17 * 4;
    context.queue.write_buffer(buffer, offset, &bytes);
    Ok(())
}

pub(super) fn read(context: &Context, buffer: &wgpu::Buffer, options: CausticOptions, cancel: &CancelToken) -> Result<Option<PhotonDiagnostics>> {
    if options.probe_pixel.is_none() {
        return Ok(None);
    }
    cancel.check()?;
    context.ensure_available()?;
    let staging = checked_creation(&context.device, || {
        Ok(context.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("bounded photon probe readback"),
            size: BYTES,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        }))
    })?;
    let submission = checked_creation(&context.device, || {
        let mut encoder = context.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("bounded photon probe") });
        encoder.copy_buffer_to_buffer(buffer, buffer.size() - (BYTES + u64::from(BOUNDS_WORDS) * 4), &staging, 0, BYTES);
        Ok(context.queue.submit([encoder.finish()]))
    })?;
    let slice = staging.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |result| {
        let _ = tx.send(result);
    });
    context.wait(submission, TIMEOUT)?;
    rx.try_recv().map_err(gpu)?.map_err(gpu)?;
    let result = match slice.get_mapped_range() {
        Ok(bytes) => decode(&bytes),
        Err(error) => Err(gpu(error)),
    };
    staging.unmap();
    cancel.check()?;
    let report = result?.ok_or_else(|| gpu("configured photon probe is missing from readback"))?;
    if Some(report.pixel) != options.probe_pixel || report.sample != options.probe_sample || report.seed != options.seed {
        return Err(gpu("photon probe readback does not match the admitted selection"));
    }
    Ok(Some(report))
}

struct Readback<'a>(&'a [u8]);
impl Readback<'_> {
    fn word(&self, index: usize) -> Result<u32> {
        let source = self.0.get(index * 4..index * 4 + 4).and_then(|v| v.try_into().ok()).ok_or_else(|| gpu("photon probe readback word missing"))?;
        Ok(u32::from_le_bytes(source))
    }
    fn float(&self, index: usize) -> Result<f64> {
        Ok(f64::from(f32::from_bits(self.word(index)?)))
    }
    fn rgb(&self, index: usize) -> Result<[f64; 3]> {
        Ok([self.float(index)?, self.float(index + 1)?, self.float(index + 2)?])
    }
    fn partition(&self, index: usize) -> Result<PhotonEstimatorPartition> {
        let value = PhotonEstimatorPartition { photon: self.rgb(index)?, other_estimators: self.rgb(index + 3)?, beauty: self.rgb(index + 6)? };
        value.validate(2e-4)?;
        Ok(value)
    }
    fn contribution(&self, at: usize) -> Result<PhotonContribution> {
        let roughness = self.float(at + 31)?;
        let mut value = PhotonContribution {
            emitted_path: self.word(at)?,
            source: self.word(at + 1)?,
            source_probability: self.float(at + 2)?,
            emission_density: self.float(at + 3)?,
            emission_measure: match self.word(at + 4)? {
                1 => EmissionMeasure::DiskArea,
                2 => EmissionMeasure::SolidAngle,
                3 => EmissionMeasure::AreaSolidAngle,
                4 => EmissionMeasure::DiskAreaSolidAngle,
                _ => return Err(gpu("photon probe emission measure is invalid")),
            },
            light_depth: self.word(at + 5)?,
            reservoir_visits: self.word(at + 6)?,
            receiver_kind: self.word(at + 7)?,
            receiver: self.word(at + 8)?,
            stratum: self.word(at + 9)?,
            photon_flux: self.rgb(at + 10)?,
            response: self.rgb(at + 13)?,
            normal_measure: self.float(at + 16)?,
            radius_mm: self.float(at + 17)?,
            radiance: self.rgb(at + 18)?,
            camera_prefix: self.rgb(at + 22)?,
            camera_radiance: self.rgb(at + 25)?,
            fresnel_strata: self.word(at + 39)?,
            camera_depth: self.word(at + 29)?,
            camera_interfaces: self.word(at + 30)?,
            camera_path: match self.word(at + 33)? {
                0 => CameraPath::Other,
                1 => CameraPath::Reflected,
                2 => CameraPath::Transmitted,
                _ => return Err(gpu("photon probe camera class is invalid")),
            },
            receiver_roughness: if roughness == -1.0 { None } else { Some(roughness) },
            gather: self.word(at + 32)?,
            query_eligible: self.word(at + 34)?,
            query_contributing: self.word(at + 35)?,
            support: PhotonContributionSupport {
                accepted: self.word(at + 35)?,
                maximum_luminance: self.float(at + 36)?,
                normalized_sum: self.float(at + 37)?,
                normalized_squared_sum: self.float(at + 38)?,
            },
            query_effective_event_support: None,
            final_output_radiance: None,
        };
        value.query_effective_event_support = value.support.effective_event_support();
        value.validate()?;
        if value.query_contributing > value.query_eligible || value.support.effective_event_support().is_none() {
            return Err(gpu("photon query support is invalid"));
        }
        Ok(value)
    }
}

pub(super) fn decode(bytes: &[u8]) -> Result<Option<PhotonDiagnostics>> {
    if bytes.len() != BYTES as usize {
        return Err(gpu("photon probe readback has an invalid length"));
    }
    let read = Readback(bytes);
    if read.word(0)? == 0 {
        return Ok(None);
    }
    if read.word(0)? != 1 || read.word(4)? > 1 {
        return Err(gpu("photon probe readback has an invalid selection flag"));
    }
    let mut result =
        PhotonDiagnostics::new([read.word(1)?, read.word(2)?], read.word(3)?, read.word(5)?, read.word(6)?, [read.float(7)?, read.float(8)?]);
    result.selected = read.word(4)? != 0;
    result.gathers = read.word(9)?;
    result.visited = read.word(10)?;
    result.eligible = read.word(11)?;
    result.contributing = read.word(12)?;
    result.top_count = read.word(13)?;
    result.local_radiance_sum = read.rgb(14)?;
    result.camera_top_count = read.word(19)?;
    result.camera_contributing = read.word(20)?;
    result.camera_radiance_sum = read.rgb(21)?;
    for (index, target) in result.histogram.iter_mut().enumerate() {
        *target = read.word(32 + index)?;
    }
    for (index, target) in result.camera_histogram.iter_mut().enumerate() {
        *target = read.word(64 + index)?;
    }
    if result.top_count != result.contributing.min(result.largest.len() as u32)
        || result.camera_top_count != result.camera_contributing.min(result.largest_camera.len() as u32)
        || result.contributing > result.eligible
        || result.eligible > result.visited
        || result.camera_contributing > result.contributing
        || result.histogram.iter().map(|value| u64::from(*value)).sum::<u64>() != u64::from(result.contributing)
        || result.camera_histogram.iter().map(|value| u64::from(*value)).sum::<u64>() != u64::from(result.camera_contributing)
        || result.local_radiance_sum.into_iter().chain(result.camera_radiance_sum).any(|v| !v.is_finite() || v < 0.0)
        || result.radii_mm.into_iter().any(|v| !v.is_finite() || v <= 0.0)
        || (!result.selected && (result.gathers != 0 || result.visited != 0 || result.contributing != 0))
    {
        return Err(gpu("photon probe readback violates its counter or numeric contract"));
    }
    for (rank, target) in result.largest.iter_mut().take(result.top_count as usize).enumerate() {
        *target = read.contribution(RECORD_BASE + rank * RECORD_WORDS)?;
        if target.gather >= result.gathers {
            return Err(gpu("photon query identity exceeds the selected sample"));
        }
    }
    for (rank, target) in result.largest_camera.iter_mut().take(result.camera_top_count as usize).enumerate() {
        *target = read.contribution(RECORD_BASE + (8 + rank) * RECORD_WORDS)?;
        if target.gather >= result.gathers {
            return Err(gpu("photon camera query identity exceeds the selected sample"));
        }
    }
    result.camera = PhotonCameraAccounting::new(result.pixel, read.word(17)?)?;
    let maximum = read.word(18)?;
    if maximum == 0 {
        return Err(gpu("photon camera probe has no production budget"));
    }
    for (slot, pixel) in result.camera.trace_pixels.iter_mut().enumerate().take(result.camera.trace_pixel_count as usize) {
        let at = PIXEL_BASE + slot * PIXEL_WORDS;
        pixel.samples = read.word(at)?;
        pixel.covered_samples = read.word(at + 1)?;
        pixel.planned_samples = (read.word(at + 2)? != 0).then_some(read.word(at + 2)?);
        pixel.radiance_sum = read.partition(at + 6)?;
        if pixel.covered_samples > pixel.samples
            || pixel.samples > pixel.planned_samples.unwrap_or(0)
            || pixel.planned_samples.is_some_and(|count| count > maximum)
            || read.word(at + 3)? != 0
            || read.word(at + 34)? != 0
            || read.word(at + 51)? > 1
        {
            return Err(gpu("photon camera readback has an incomplete or invalid observation count"));
        }
        if read.word(at + 51)? != 0 {
            if pixel.pixel != result.pixel || !result.selected || pixel.samples <= result.sample || result.camera.selected_sample.is_some() {
                return Err(gpu("selected photon camera sample has an invalid identity"));
            }
            let strata = read.word(at + 50)?;
            let coverage = read.float(at + 49)?;
            if !matches!(strata, 1 | 2 | 4) || (coverage != 0.0 && coverage != 1.0) {
                return Err(gpu("selected photon camera sample has invalid lane or coverage metadata"));
            }
            result.camera.selected_sample =
                Some(PhotonSampleAccounting { sample: result.sample, strata, coverage, radiance: read.partition(at + 40)? });
        }
    }
    if result.selected != result.camera.selected_sample.is_some() {
        return Err(gpu("photon selection and completed camera sample disagree"));
    }
    Ok(Some(result))
}
