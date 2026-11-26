//! Row-aligned GPU storage pages and filter windows. Page boundaries change
//! resource residency only; camera/sample identities remain full-image indices.
use super::{FAILURE_BYTES, JobError, JobOptions, Result};
use solidcraft_render::job::{ErrorCode, MAX_TRACE_DIMENSION, MAX_TRACE_PIXELS};

// A working-set target, not an image-size limit: larger images use more pages.
// Actual adapter binding/allocation limits can make pages smaller.
pub(super) const TARGET_BUFFER_BYTES: u64 = 1024 * 1024 * 1024;

#[derive(Clone, Copy, Debug)]
pub(super) struct Layout {
    pub width: u32,
    pub height: u32,
    pub rows: u32,
    pub halo: u32,
    pub planes: u32,
    pub optical_bytes: u64,
    filter: bool,
    adaptive: bool,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct Window {
    pub start: u32,
    pub pixels: u32,
    pub core_start: u32,
    pub core_pixels: u32,
}

fn limit(message: &str) -> JobError {
    JobError::new(ErrorCode::LimitExceeded, message)
}

impl Layout {
    pub(super) fn new(width: u32, height: u32, options: JobOptions, optical_bytes: u64, limits: &wgpu::Limits) -> Result<Self> {
        if width == 0
            || height == 0
            || width > MAX_TRACE_DIMENSION
            || height > MAX_TRACE_DIMENSION
            || u64::from(width) * u64::from(height) > MAX_TRACE_PIXELS
        {
            return Err(JobError::new(ErrorCode::InvalidRequest, "GPU image pages require a bounded nonempty trace grid"));
        }
        options.denoise_settings.validate()?;
        let adaptive = options.adaptive.is_some();
        let planes = if adaptive { 3 } else { 1 };
        let filter = options.denoise || options.denoise_diagnostics;
        let halo = if options.denoise { 2 * ((1 << options.denoise_settings.passes) - 1) } else { 0 };
        let storage = limits.max_storage_buffer_binding_size.min(limits.max_buffer_size).min(TARGET_BUFFER_BYTES);
        let film_row = u64::from(width) * 16 * u64::from(planes);
        let storage_rows = storage.checked_sub(optical_bytes).ok_or_else(|| limit("GPU storage cannot fit optical dispatch scratch"))? / film_row;
        let readback_rows = limits.max_buffer_size.checked_sub(FAILURE_BYTES).ok_or_else(|| limit("GPU readback cannot fit diagnostics"))?
            / (u64::from(width) * 16 * if adaptive { 2 } else { 1 });
        let mut rows = u64::from(height).min(storage_rows).min(readback_rows);
        if filter {
            let window_rows = storage / (u64::from(width) * super::denoise::GUIDE_BYTES_PER_PIXEL);
            if window_rows < u64::from(height) {
                let core_rows = window_rows
                    .checked_sub(u64::from(halo) * 2)
                    .ok_or_else(|| limit("GPU storage cannot fit one denoising row with its complete filter halo"))?;
                rows = rows.min(core_rows);
            }
        }
        // Each diagnostic core owns complete supersampling cells.
        if rows < u64::from(height) {
            let scale = u64::from(options.supersampling);
            if scale == 0 {
                return Err(JobError::new(ErrorCode::InvalidRequest, "GPU image page supersampling must be nonzero"));
            }
            rows -= rows % scale;
        }
        let rows = u32::try_from(rows).map_err(|_| limit("GPU image page row count exceeds its address space"))?;
        if rows == 0 {
            return Err(limit("GPU storage cannot fit one complete image row and its required scratch"));
        }
        let layout = Self { width, height, rows, halo, planes, optical_bytes, filter, adaptive };
        if layout.buffer_bytes(layout.max_core_pixels())? > storage || layout.readback_bytes()? > limits.max_buffer_size {
            return Err(limit("GPU image page allocation exceeds the selected adapter limits"));
        }
        Ok(layout)
    }

    #[cfg(test)]
    pub(super) fn with_rows(self, rows: u32, scale: u32) -> Result<Self> {
        let rows = rows.min(self.rows);
        if rows == 0 || scale == 0 || (rows < self.height && !rows.is_multiple_of(scale)) {
            return Err(limit("test GPU page rows must contain complete supersampling cells"));
        }
        Ok(Self { rows, ..self })
    }

    pub(super) fn is_paged(self) -> bool {
        self.rows < self.height
    }

    pub(super) fn page_count(self) -> u32 {
        self.height.div_ceil(self.rows)
    }

    pub(super) fn max_core_pixels(self) -> u32 {
        self.width * self.rows
    }

    pub(super) fn max_window_pixels(self) -> u32 {
        self.width * self.height.min(self.rows + self.halo * 2)
    }

    pub(super) fn buffer_bytes(self, pixels: u32) -> Result<u64> {
        u64::from(pixels)
            .checked_mul(16 * u64::from(self.planes))
            .and_then(|bytes| bytes.checked_add(self.optical_bytes))
            .ok_or_else(|| limit("GPU image page byte count overflow"))
    }

    pub(super) fn optical_scratch_bytes(self) -> Result<u64> {
        self.optical_bytes.checked_mul(u64::from(self.page_count())).ok_or_else(|| limit("GPU optical page scratch overflow"))
    }

    pub(super) fn readback_bytes(self) -> Result<u64> {
        let film = u64::from(self.max_core_pixels()) * 16 * if self.adaptive { 2 } else { 1 };
        let filter = if self.filter { u64::from(self.max_window_pixels()) * 16 } else { 0 };
        film.max(filter).checked_add(FAILURE_BYTES).ok_or_else(|| limit("GPU page readback byte count overflow"))
    }

    pub(super) fn filter_scratch_bytes(self) -> Result<u64> {
        if !self.filter {
            return Ok(0);
        }
        let normalized = if self.adaptive { 16 } else { 0 };
        let gather = if self.is_paged() { 16 * u64::from(self.planes) } else { 0 };
        u64::from(self.max_window_pixels())
            .checked_mul(super::denoise::BYTES_PER_PIXEL + normalized + gather)
            .ok_or_else(|| limit("GPU filter window byte count overflow"))
    }

    pub(super) fn windows(self) -> impl Iterator<Item = Window> {
        (0..self.height).step_by(self.rows as usize).map(move |row| {
            let end = self.height.min(row + self.rows);
            let first = row.saturating_sub(self.halo);
            let last = self.height.min(end + self.halo);
            Window {
                start: first * self.width,
                pixels: (last - first) * self.width,
                core_start: row * self.width,
                core_pixels: (end - row) * self.width,
            }
        })
    }
}

pub(super) struct FilmPage {
    pub start: u32,
    pub pixels: u32,
    pub accumulation: wgpu::Buffer,
    pub bind: wgpu::BindGroup,
}
