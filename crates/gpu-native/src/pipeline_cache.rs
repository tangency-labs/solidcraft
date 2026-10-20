//! A pipeline cache seeded with data from an earlier run (#3433).
//!
//! `Device::create_pipeline_cache` is `unsafe` because wgpu cannot prove that seed data came from
//! `PipelineCache::get_data`. wgpu checks its own header (format version, backend, adapter and
//! driver) and, with `fallback`, starts empty when that doesn't match; it does not check the
//! data itself. The caller (`solidcraft_gpu::shader_cache`) only passes data it wrote itself from
//! `get_data`, behind a SHA-256 of the data that it checks first, so corrupted or truncated files
//! never reach the driver.
#![allow(unsafe_code)]

/// A pipeline cache for `device`, seeded with `data` that an earlier run's
/// [`wgpu::PipelineCache::get_data`] returned; `None` when the device lacks
/// [`wgpu::Features::PIPELINE_CACHE`].
pub fn pipeline_cache(device: &wgpu::Device, label: &str, data: Option<&[u8]>) -> Option<wgpu::PipelineCache> {
    if !device.features().contains(wgpu::Features::PIPELINE_CACHE) {
        return None;
    }
    // SAFETY: `data` is `get_data` output written by this program and verified against its
    // checksum by the caller (see the module documentation); `fallback: true` makes wgpu start
    // an empty cache instead of failing when the data is for another adapter, driver or format.
    Some(unsafe { device.create_pipeline_cache(&wgpu::PipelineCacheDescriptor { label: Some(label), data, fallback: true }) })
}
