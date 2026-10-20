//! Compiled pipelines kept between runs, as games keep their shader caches (#3433).
//!
//! A cold Vulkan driver needs minutes for the path tracer's shader. Where the backend keeps a
//! pipeline cache (Vulkan; wgpu has none for D3D12 or Metal), the cache is seeded from disk
//! before the pipelines are built and written back after. Each file holds wgpu's own data
//! (versioned and keyed by adapter and driver, so a driver update starts empty) behind a
//! SHA-256 of it; a file that fails the check is ignored, never handed to the driver. Writes
//! go to a temporary file renamed into place, so concurrent processes never see half a file.
//! Every failure here costs speed only: the pipelines are built without the cache.

use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

/// The file format: this tag, the SHA-256 of the data, the data.
const MAGIC: &[u8; 8] = b"SCPIPE01";
const HEADER: usize = MAGIC.len() + 32;
/// Larger files are not read (a path tracer cache is a few megabytes).
const MAX_BYTES: u64 = 512 << 20;
/// Where the cache lives instead of the default folder, or `off`.
pub const ENV: &str = "SOLIDCRAFT_SHADER_CACHE";

/// A pipeline cache for one set of pipelines on one adapter, and the file it persists to.
pub(crate) struct ShaderCache {
    cache: wgpu::PipelineCache,
    path: PathBuf,
}

impl ShaderCache {
    /// The cache for the pipelines `name` on this device, seeded from the cache folder; `None`
    /// where the backend keeps no pipeline cache or caching is off.
    pub(crate) fn open(device: &wgpu::Device, info: &wgpu::AdapterInfo, name: &str) -> Option<Self> {
        Self::open_in(&dir()?, device, info, name)
    }

    pub(crate) fn open_in(dir: &Path, device: &wgpu::Device, info: &wgpu::AdapterInfo, name: &str) -> Option<Self> {
        let key = wgpu::util::pipeline_cache_key(info)?;
        let path = dir.join(format!("{key}_{name}.bin"));
        let data = read(&path);
        log::info!("shader cache {}: {}", path.display(), data.as_ref().map_or("empty".into(), |d| format!("{} KiB", d.len() / 1024)));
        let cache = solidcraft_gpu_native::pipeline_cache(device, name, data.as_deref())?;
        Some(Self { cache, path })
    }

    pub(crate) fn get(&self) -> &wgpu::PipelineCache {
        &self.cache
    }

    /// Write the cache back once its pipelines are built. A failure is logged, never fatal.
    pub(crate) fn save(&self) {
        let Some(data) = self.cache.get_data() else { return };
        match write(&self.path, &data) {
            Ok(()) => log::info!("shader cache {}: saved {} KiB", self.path.display(), data.len() / 1024),
            Err(e) => log::warn!("shader cache {}: not saved: {e}", self.path.display()),
        }
    }
}

/// The cache folder: `SOLIDCRAFT_SHADER_CACHE` (a folder, or `off`), else the user's cache
/// folder: `%LOCALAPPDATA%\SolidCraft\cache\shaders`, `~/Library/Caches/SolidCraft/shaders` or
/// `$XDG_CACHE_HOME/solidcraft/shaders` (`~/.cache` without it).
pub fn dir() -> Option<PathBuf> {
    let env = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty()).map(PathBuf::from);
    if let Some(chosen) = env(ENV) {
        return (chosen.as_os_str() != "off").then_some(chosen);
    }
    if cfg!(target_os = "windows") {
        Some(env("LOCALAPPDATA")?.join("SolidCraft").join("cache").join("shaders"))
    } else if cfg!(target_os = "macos") {
        Some(env("HOME")?.join("Library/Caches/SolidCraft/shaders"))
    } else {
        Some(env("XDG_CACHE_HOME").or_else(|| env("HOME").map(|h| h.join(".cache")))?.join("solidcraft/shaders"))
    }
}

/// The data in a cache file, when the file is whole and its checksum matches.
fn read(path: &Path) -> Option<Vec<u8>> {
    if std::fs::metadata(path).ok()?.len() > MAX_BYTES {
        return None;
    }
    let bytes = std::fs::read(path).ok()?;
    let (head, data) = bytes.split_at_checked(HEADER)?;
    let (magic, sum) = head.split_at(MAGIC.len());
    if magic != MAGIC || sum != Sha256::digest(data).as_slice() {
        log::warn!("shader cache {}: ignored (damaged or from another version)", path.display());
        return None;
    }
    Some(data.to_vec())
}

fn write(path: &Path, data: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut bytes = Vec::with_capacity(HEADER + data.len());
    bytes.extend_from_slice(MAGIC);
    bytes.extend_from_slice(&Sha256::digest(data));
    bytes.extend_from_slice(data);
    let temporary = path.with_extension(format!("{}.tmp", std::process::id()));
    std::fs::write(&temporary, &bytes)?;
    std::fs::rename(&temporary, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&temporary);
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("solidcraft-shader-cache-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn files_round_trip_and_damage_is_ignored() {
        let dir = scratch("round-trip");
        let path = dir.join("nested").join("cache.bin");
        let data: Vec<u8> = (0..10_000u32).map(|i| (i * 7) as u8).collect();
        write(&path, &data).unwrap();
        assert_eq!(read(&path).as_deref(), Some(&data[..]));

        let mut bytes = std::fs::read(&path).unwrap();
        let last = bytes.len() - 1;
        bytes[last] ^= 1;
        std::fs::write(&path, &bytes).unwrap();
        assert_eq!(read(&path), None, "a flipped bit");
        std::fs::write(&path, &bytes[..HEADER - 1]).unwrap();
        assert_eq!(read(&path), None, "truncated inside the header");
        let mut other = bytes.clone();
        other[0] = b'X';
        std::fs::write(&path, &other).unwrap();
        assert_eq!(read(&path), None, "another format");
        assert_eq!(read(&dir.join("missing.bin")), None);
        // Nothing but the file itself is left behind.
        write(&path, &data).unwrap();
        assert_eq!(std::fs::read_dir(path.parent().unwrap()).unwrap().count(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// On a device with a pipeline cache, building a pipeline and saving leaves a checked file
    /// that the next open reads.
    #[test]
    fn a_built_pipeline_is_saved_and_read_back() {
        let Some((adapter, _)) = crate::select::pick(crate::select::Policy::Auto) else { return };
        let info = adapter.get_info();
        let Ok((device, _queue)) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            required_features: adapter.features() & wgpu::Features::PIPELINE_CACHE,
            ..Default::default()
        })) else {
            return;
        };
        let dir = scratch("device");
        let Some(cache) = ShaderCache::open_in(&dir, &device, &info, "test") else {
            assert!(wgpu::util::pipeline_cache_key(&info).is_none() || !device.features().contains(wgpu::Features::PIPELINE_CACHE));
            return;
        };
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: None,
            source: wgpu::ShaderSource::Wgsl("@group(0) @binding(0) var<storage, read_write> v: array<u32>; @compute @workgroup_size(64) fn main(@builtin(global_invocation_id) i: vec3<u32>) { v[i.x] = i.x * 3u; }".into()),
        });
        let _pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: None,
            layout: None,
            module: &module,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: Some(cache.get()),
        });
        cache.save();
        let saved = read(&cache.path).expect("a saved cache with a matching checksum");
        assert!(!saved.is_empty());
        assert!(ShaderCache::open_in(&dir, &device, &info, "test").is_some(), "the next run seeds from it");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn only_vulkan_keeps_a_pipeline_cache() {
        let info = |backend| wgpu::AdapterInfo::new(wgpu::DeviceType::DiscreteGpu, backend);
        assert!(wgpu::util::pipeline_cache_key(&info(wgpu::Backend::Vulkan)).is_some());
        for backend in [wgpu::Backend::Dx12, wgpu::Backend::Metal, wgpu::Backend::Gl] {
            assert!(wgpu::util::pipeline_cache_key(&info(backend)).is_none(), "{backend:?}");
        }
    }
}
