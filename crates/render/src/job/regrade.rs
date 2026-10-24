//! Cancellable, geometry-free presentation of an immutable native linear master.
//! A derivative retains the original frame lease instead of copying EXR bytes.
use super::{CancelToken, ErrorCode, ImageRequest, ImageWork, JobError, Phase, PngTask, Progress, RenderFrame, Result, TaskPoll};
use super::{bloom::BloomTask, linear_exr_decode::Decoder, presentation::Presentation};
use std::sync::Arc;

pub(super) fn required_bytes(source: &RenderFrame, settings: Presentation) -> Result<u64> {
    settings.validate()?;
    super::linear_exr::encoded_limit(source.info.width, source.info.height)?;
    if source.linear_exr().is_none() {
        return Err(JobError::new(ErrorCode::InvalidRequest, "this completed frame has no retained raw linear master"));
    }
    // Includes decoded float4, bloom ping/pong float3 or display bytes, bounded
    // PNG output and codec/header/task metadata. The source keeps its own lease.
    Ok(u64::from(source.info.width) * u64::from(source.info.height) * 48
        + 1_048_576
        + settings.finishing.photographic.scratch_bytes(source.info.width, source.info.height)?)
}

pub(super) struct RegradeTask {
    source: Arc<RenderFrame>,
    settings: Presentation,
    cancel: CancelToken,
    decoder: Option<Decoder>,
    bloom: Option<BloomTask>,
    linear: Option<Vec<[f32; 4]>>,
    glow: Option<Vec<[f32; 3]>>,
    rgba: Vec<u8>,
    encoded: usize,
    png: Option<PngTask>,
    photographic: Option<super::photographic::PhotographicTask>,
}
impl RegradeTask {
    pub(super) fn new(source: Arc<RenderFrame>, settings: Presentation, cancel: CancelToken) -> Result<Self> {
        required_bytes(&source, settings)?;
        let raw = source.linear_exr().ok_or_else(|| JobError::new(ErrorCode::InvalidRequest, "linear master disappeared"))?;
        let decoder = Decoder::new(raw, source.info.width, source.info.height, &cancel)?;
        Ok(Self {
            source,
            settings,
            cancel,
            decoder: Some(decoder),
            bloom: None,
            linear: None,
            glow: None,
            rgba: Vec::new(),
            encoded: 0,
            png: None,
            photographic: None,
        })
    }
    pub(super) fn source(&self) -> Arc<RenderFrame> {
        self.source.clone()
    }
    fn pending(&self, done: u64, total: u64) -> Result<TaskPoll> {
        Ok(TaskPoll::Pending(Progress { phase: Phase::Encoding, done, total }))
    }
}
impl ImageWork for RegradeTask {
    fn step(&mut self, units: usize) -> Result<TaskPoll> {
        self.cancel.check()?;
        if let Some(png) = &mut self.png {
            return png.step(units);
        }
        let (width, height) = (self.source.info.width, self.source.info.height);
        if let Some(mut task) = self.photographic.take() {
            if !task.step(units)? {
                let (done, total) = task.progress();
                self.photographic = Some(task);
                return self.pending(done, total);
            }
            let request = ImageRequest {
                width,
                height,
                camera: Default::default(),
                integrator: Default::default(),
                raytrace: None,
                environment: None,
                textures: None,
                background: self.settings.background,
                transparent_background: self.settings.transparent_background,
            };
            self.png = Some(PngTask::new(&request, task.finish()?, self.cancel.clone())?);
            return self.pending(0, u64::from(height));
        }
        if let Some(mut decoder) = self.decoder.take() {
            let raw = self.source.linear_exr().ok_or_else(|| JobError::new(ErrorCode::Internal, "missing retained master"))?;
            if !decoder.step(raw, &self.cancel)? {
                let done = decoder.rows();
                self.decoder = Some(decoder);
                return self.pending(done, u64::from(height));
            }
            let pixels = decoder.finish()?;
            if let Some(bloom) = self.settings.finishing.bloom.filter(|b| b.active()) {
                self.bloom = Some(BloomTask::new(width, height, pixels, bloom, self.cancel.clone())?);
                return self.pending(0, u64::from(height) * 7);
            }
            self.linear = Some(pixels);
        }
        if let Some(mut bloom) = self.bloom.take() {
            if !bloom.step(4)? {
                let (done, total) = (bloom.completed_lines(), bloom.total_lines());
                self.bloom = Some(bloom);
                return self.pending(done, total);
            }
            let (pixels, glow) = bloom.finish()?;
            self.linear = Some(pixels);
            self.glow = Some(glow);
        }
        let count = width as usize * height as usize;
        if self.rgba.is_empty() {
            self.rgba.try_reserve_exact(count * 4).map_err(|_| JobError::new(ErrorCode::AllocationFailed, "regrade display allocation failed"))?;
            if self.rgba.capacity() != count * 4 {
                return Err(JobError::new(ErrorCode::LimitExceeded, "regrade display exceeds admitted capacity"));
            }
            self.rgba.resize(count * 4, 0);
        }
        let end = self.encoded.saturating_add(units.clamp(1, 4096)).min(count);
        let pixels = self
            .linear
            .as_deref()
            .and_then(|p| p.get(self.encoded..end))
            .ok_or_else(|| JobError::new(ErrorCode::Internal, "missing regrade pixels"))?;
        let output = self.rgba.get_mut(self.encoded * 4..end * 4).ok_or_else(|| JobError::new(ErrorCode::Internal, "missing regrade output"))?;
        let glow = self.glow.as_deref().and_then(|g| g.get(self.encoded..end));
        self.settings.display(width, height)?.encode_bloom_at(pixels, output, &self.cancel, self.encoded, glow)?;
        self.encoded = end;
        if end < count {
            return self.pending(end as u64, count as u64);
        }
        self.glow = None;
        if self.settings.finishing.photographic.active() {
            let pixels = self.linear.take().ok_or_else(|| JobError::new(ErrorCode::Internal, "missing photographic master pixels"))?;
            self.photographic = Some(super::photographic::PhotographicTask::new(
                width,
                height,
                std::mem::take(&mut self.rgba),
                pixels,
                self.settings.finishing.photographic,
                self.cancel.clone(),
            )?);
            return self.pending(0, u64::from(width) * u64::from(height));
        }
        self.linear = None;
        let request = ImageRequest {
            width,
            height,
            camera: Default::default(),
            integrator: Default::default(),
            raytrace: None,
            environment: None,
            textures: None,
            background: self.settings.background,
            transparent_background: self.settings.transparent_background,
        };
        self.png = Some(PngTask::new(&request, std::mem::take(&mut self.rgba), self.cancel.clone())?);
        self.pending(0, u64::from(height))
    }
}
