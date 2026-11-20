//! Optional color-only HDR finishing on the CPU. Native objects and their buffers stay
//! on an owned worker; polling never runs inference. Drop cancels and joins that worker
//! before the enclosing host can release its memory lease. Cooperative hosts refuse OIDN.
//!
//! The pinned wrapper has no safe progress/cancel or workspace controls. Cancellation
//! discards the result but retirement must wait for native execution. Workspace admission
//! is an estimate, not a native allocation cap; production qualification remains required.

use super::{CancelToken, DenoiseSettings, ErrorCode, JobError, Result};

/// Extra image storage: one packed RGB slice and the wrapper's in-place RGB buffer.
const IMAGE_BYTES_PER_PIXEL: u64 = 24;
/// Provisional allowance for model/workspace/runtime state, not an enforced OIDN cap.
/// The wrapper cannot set maxMemoryMB; large-image qualification is still required.
const NATIVE_WORKSPACE_BYTES: u64 = 512 * 1024 * 1024;

fn invalid(message: &str) -> JobError {
    JobError::new(ErrorCode::InvalidRequest, message)
}

/// Compile-time capability only. A feature-enabled build must also ship a working CPU
/// runtime; device creation failures are reported rather than falling back to Spatial.
pub fn available() -> bool {
    cfg!(all(feature = "denoise-oidn", not(target_arch = "wasm32")))
}

/// Execution-time checks, separate from portable saved settings. Transparent exports
/// need a qualified RGB/coverage policy before OIDN can be offered for them.
pub fn validate_request(width: u32, height: u32, transparent_background: bool, diagnostics: bool) -> Result<()> {
    if !available() {
        return Err(JobError::new(ErrorCode::HostUnavailable, "Open Image Denoise is not built into this SolidCraft; use the spatial filter"));
    }
    if diagnostics {
        return Err(invalid("Open Image Denoise has no Spatial guide diagnostics; disable denoise_diagnostics or use the spatial filter"));
    }
    required_bytes(width, height)?;
    if transparent_background {
        return Err(invalid("Open Image Denoise currently requires opaque output; disable transparent background or use the spatial filter"));
    }
    Ok(())
}

/// Extra bytes beyond the caller-owned raw RGBA image. Both CPU and GPU admission use
/// the same estimate before any native work starts, including the internal X2 grid.
pub fn required_bytes(width: u32, height: u32) -> Result<u64> {
    let pixels = u64::from(width) * u64::from(height);
    if width == 0 || height == 0 || width > super::MAX_TRACE_DIMENSION || height > super::MAX_TRACE_DIMENSION || pixels > super::MAX_TRACE_PIXELS {
        return Err(invalid("Open Image Denoise requires a bounded nonempty trace grid"));
    }
    pixels
        .checked_mul(IMAGE_BYTES_PER_PIXEL)
        .and_then(|bytes| bytes.checked_add(NATIVE_WORKSPACE_BYTES))
        .ok_or_else(|| JobError::new(ErrorCode::LimitExceeded, "Open Image Denoise memory estimate overflow"))
}

/// Only failure to execute the native finishing pass can preserve an already encoded
/// raw master. Invalid input, admission and cancellation must remain ordinary failures.
#[derive(Debug)]
pub struct OidnFailure {
    error: JobError,
    native: bool,
}
impl From<JobError> for OidnFailure {
    fn from(error: JobError) -> Self {
        Self { error, native: false }
    }
}
impl From<OidnFailure> for JobError {
    fn from(failure: OidnFailure) -> Self {
        failure.error
    }
}
impl OidnFailure {
    #[cfg(all(feature = "denoise-oidn", not(target_arch = "wasm32")))]
    fn native(error: JobError) -> Self {
        Self { native: error.code != ErrorCode::Cancelled, error }
    }
    /// Attribute the failure at the producer boundary without changing the job's error.
    pub fn record(self, native_failed: &mut bool) -> JobError {
        *native_failed = self.native;
        self.error
    }
}
type PassResult<T> = std::result::Result<T, OidnFailure>;

/// A single native pass. Callers retain this task (and their admission lease) until
/// completion. Dropping it waits for the worker: only native background hosts may own it.
pub struct OidnTask {
    #[cfg(all(feature = "denoise-oidn", not(target_arch = "wasm32")))]
    worker: Option<std::thread::JoinHandle<PassResult<Vec<[f32; 4]>>>>,
    cancel: CancelToken,
}

impl OidnTask {
    pub fn new(width: u32, height: u32, source: Vec<[f32; 4]>, settings: DenoiseSettings, cancel: CancelToken) -> PassResult<Self> {
        validate_request(width, height, false, false)?;
        cancel.check()?;
        settings.validate()?;
        if u64::from(width) * u64::from(height) != source.len() as u64 {
            return Err(invalid("Open Image Denoise needs the trace grid's own pixels").into());
        }
        #[cfg(all(feature = "denoise-oidn", not(target_arch = "wasm32")))]
        {
            let worker_cancel = cancel.clone();
            let worker = std::thread::Builder::new()
                .name("solidcraft-oidn".into())
                .spawn(move || filter(width, height, source, settings, &worker_cancel))
                .map_err(|error| JobError::new(ErrorCode::HostUnavailable, format!("could not start Open Image Denoise worker: {error}")))?;
            Ok(Self { worker: Some(worker), cancel })
        }
        #[cfg(not(all(feature = "denoise-oidn", not(target_arch = "wasm32"))))]
        {
            let _ = source;
            Err(JobError::new(ErrorCode::HostUnavailable, "Open Image Denoise is unavailable on this build target").into())
        }
    }

    /// No fraction is fabricated while the safe wrapper cannot report native progress.
    /// An active worker remains pending even after cancellation: an early error could
    /// drop this task and join native work inside the caller's bounded step. The host
    /// retires cancelled work outside that step; completed output is never published.
    pub fn poll(&mut self) -> PassResult<Option<Vec<[f32; 4]>>> {
        #[cfg(all(feature = "denoise-oidn", not(target_arch = "wasm32")))]
        if self.worker.as_ref().is_some_and(|worker| !worker.is_finished()) {
            // The native host immediately steps pending jobs again; yielding alone can
            // burn a core throughout inference. Back off without waiting for the pass.
            std::thread::sleep(std::time::Duration::from_millis(1));
            return Ok(None);
        }
        self.wait().map(Some)
    }

    /// Wait for retirement and take the result once. Bounded render steps must use `poll`.
    pub fn wait(&mut self) -> PassResult<Vec<[f32; 4]>> {
        #[cfg(all(feature = "denoise-oidn", not(target_arch = "wasm32")))]
        {
            let worker = self.worker.take().ok_or_else(|| invalid("Open Image Denoise task already completed"))?;
            let result = worker.join();
            self.cancel.check()?;
            // A worker panic has no trustworthy native execution result.
            result.map_err(|_| JobError::new(ErrorCode::Internal, "Open Image Denoise worker failed"))?
        }
        #[cfg(not(all(feature = "denoise-oidn", not(target_arch = "wasm32"))))]
        {
            self.cancel.check()?;
            Err(JobError::new(ErrorCode::HostUnavailable, "Open Image Denoise is unavailable on this build target").into())
        }
    }

    #[cfg(all(test, feature = "denoise-oidn", not(target_arch = "wasm32")))]
    pub(super) fn from_worker_for_test(worker: std::thread::JoinHandle<PassResult<Vec<[f32; 4]>>>, cancel: CancelToken) -> Self {
        Self { worker: Some(worker), cancel }
    }
}

impl Drop for OidnTask {
    fn drop(&mut self) {
        #[cfg(all(feature = "denoise-oidn", not(target_arch = "wasm32")))]
        if let Some(worker) = self.worker.take() {
            self.cancel.cancel();
            // Never detach native buffers from their caller's admission lease. Host work
            // drops on its native worker, not the window thread; cooperative hosts refuse it.
            let _ = worker.join();
        }
    }
}

#[cfg(all(feature = "denoise-oidn", not(target_arch = "wasm32")))]
fn filter(width: u32, height: u32, mut source: Vec<[f32; 4]>, settings: DenoiseSettings, cancel: &CancelToken) -> PassResult<Vec<[f32; 4]>> {
    use super::OidnQuality;
    cancel.check()?;
    for row in source.chunks(width as usize) {
        cancel.check()?;
        if row.iter().any(|p| p.iter().any(|c| !c.is_finite() || *c < 0.0) || p[3] > 1.0 || (p[3] == 0.0 && p[..3] != [0.0; 3])) {
            return Err(invalid("Open Image Denoise refuses invalid radiance or coverage; diagnose the raw render first").into());
        }
    }
    let native_error = |error: oidn::Error| {
        let code = match error.kind() {
            oidn::ErrorKind::OutOfMemory => ErrorCode::AllocationFailed,
            oidn::ErrorKind::UnsupportedHardware => ErrorCode::HostUnavailable,
            oidn::ErrorKind::Canceled => ErrorCode::Cancelled,
            _ => ErrorCode::Internal,
        };
        OidnFailure::native(JobError::new(code, format!("Open Image Denoise: {error}")))
    };
    let device = oidn::Device::cpu().map_err(native_error)?;
    let quality = match settings.oidn_quality {
        OidnQuality::Fast => oidn::Quality::Fast,
        OidnQuality::Balanced => oidn::Quality::Balanced,
        OidnQuality::High => oidn::Quality::High,
    };
    let mut filter = oidn::RayTracing::try_new(&device).map_err(native_error)?;
    filter.filter_quality(quality).hdr(true).srgb(false).image_dimensions(width as usize, height as usize);
    let count = source.len().checked_mul(3).ok_or_else(|| invalid("Open Image Denoise RGB length overflow"))?;
    let mut colour = Vec::new();
    colour.try_reserve_exact(count).map_err(|_| JobError::new(ErrorCode::AllocationFailed, "Open Image Denoise colour allocation failed"))?;
    if colour.capacity() != count {
        return Err(JobError::new(ErrorCode::LimitExceeded, "Open Image Denoise colour capacity exceeds admission").into());
    }
    for row in source.chunks(width as usize) {
        cancel.check()?;
        for pixel in row {
            colour.extend_from_slice(&pixel[..3]);
        }
    }
    // In-place filtering needs only one caller RGB slice and one native RGB buffer,
    // rather than the four RGB planes in the wrapper's out-of-place slice path.
    filter.filter_in_place(&mut colour).map_err(native_error)?;
    cancel.check()?;
    for (pixel, rgb) in source.iter_mut().zip(colour.as_chunks::<3>().0) {
        if rgb.iter().any(|v| !v.is_finite() || *v < 0.0) {
            return Err(OidnFailure::native(JobError::new(ErrorCode::Internal, "Open Image Denoise returned invalid radiance")));
        }
        // Keep exact raw coverage, and canonical zero RGB for a completely clear pixel.
        if pixel[3] != 0.0 {
            pixel[..3].copy_from_slice(rgb);
        }
    }
    cancel.check()?;
    Ok(source)
}
