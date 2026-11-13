//! Bounded CPU row preparation with complete linear area-filtered mip chains;
//! callers prepay required_bytes and schedule these continuations on their worker.
use super::{BaseColorMode, TextureBinding, TextureMap, normal_direction_variance, unit_normal_texel};
use crate::{
    job::{CancelToken, ErrorCode, JobError, Result},
    surface_patterns::ShadingSurface,
    texture_input::{MapRole, png_metadata::PngMetadata},
    texture_sampling::{TextureView, mips},
};
use solidcore_record::Vec3;
use solidcraft_geom::prelude::*;
use solidcraft_viewport::surface::{MIN_ROUGHNESS, srgb8_to_linear};
use std::{
    io::{self, BufRead, Read, Seek, SeekFrom},
    sync::Arc,
};

/// The existing whole-image texture decoder's bounded workspace, not a raised host cap.
const CODEC_BYTES: usize = crate::texture_input::MAX_CODEC_BYTES as usize;
const READ_CHUNK: usize = 4096;
const MAX_EDGE: u32 = 1024;

fn invalid(message: impl Into<String>) -> JobError {
    JobError::new(ErrorCode::InvalidRequest, message)
}
fn allocation() -> JobError {
    JobError::new(ErrorCode::AllocationFailed, "CPU map preparation allocation failed")
}
fn codec_error(cancel: &CancelToken, message: impl Into<String>) -> JobError {
    if cancel.is_cancelled() { JobError::new(ErrorCode::Cancelled, "CPU map preparation cancelled") } else { invalid(message) }
}
fn guarded<T>(cancel: &CancelToken, work: impl FnOnce() -> Result<T>) -> Result<T> {
    cancel.check()?;
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(work)).map_err(|_| invalid("CPU PNG decoder failed"))?;
    cancel.check()?;
    result
}

/// Owns the existing source Arc; no borrowed/self-referential decoder or encoded copy.
struct EncodedReader {
    bytes: Arc<Vec<u8>>,
    position: usize,
    cancel: CancelToken,
}
impl Read for EncodedReader {
    fn read(&mut self, destination: &mut [u8]) -> io::Result<usize> {
        let source = self.fill_buf()?;
        let count = source.len().min(destination.len());
        destination
            .get_mut(..count)
            .ok_or_else(|| io::Error::other("CPU map read range"))?
            .copy_from_slice(source.get(..count).ok_or_else(|| io::Error::other("CPU map source range"))?);
        self.consume(count);
        Ok(count)
    }
}
impl BufRead for EncodedReader {
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
        if self.cancel.is_cancelled() {
            return Err(io::Error::other("CPU map cancelled"));
        }
        let end = self.position.saturating_add(READ_CHUNK).min(self.bytes.len());
        self.bytes.get(self.position..end).ok_or_else(|| io::Error::other("CPU map cursor outside source"))
    }
    fn consume(&mut self, count: usize) {
        self.position = self.position.saturating_add(count).min(self.bytes.len());
    }
}
impl Seek for EncodedReader {
    fn seek(&mut self, from: SeekFrom) -> io::Result<u64> {
        if self.cancel.is_cancelled() {
            return Err(io::Error::other("CPU map cancelled"));
        }
        let position = match from {
            SeekFrom::Start(value) => i128::from(value),
            SeekFrom::Current(value) => self.position as i128 + i128::from(value),
            SeekFrom::End(value) => self.bytes.len() as i128 + i128::from(value),
        };
        if position < 0 || position > self.bytes.len() as i128 {
            return Err(io::Error::other("CPU map seek outside source"));
        }
        self.position = usize::try_from(position).map_err(|_| io::Error::other("CPU map seek overflow"))?;
        Ok(self.position as u64)
    }
}
struct Level {
    pixels: Vec<[f32; 4]>,
    width: u32,
    height: u32,
}
impl Level {
    fn view(&self) -> Result<TextureView<'_>> {
        // The builder validated each encoded/averaged pixel before insertion, and
        // this private complete level is immutable. Never rescan a map per ray.
        TextureView::from_admitted(self.pixels.as_flattened(), [self.width, self.height]).map_err(|error| invalid(error.to_string()))
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stage {
    Metadata,
    Begin,
    Row,
    Pixel(usize),
    FinishPng,
    NewLevel,
    MipPixel(usize),
    Done,
}

pub struct MapTask {
    map: TextureMap,
    cancel: CancelToken,
    readers: Vec<png::Reader<EncodedReader>>,
    metadata: PngMetadata,
    row: Vec<u8>,
    channels: usize,
    rows: u32,
    levels: Vec<Level>,
    stage: Stage,
    working_bytes: u64,
    full_mips: bool,
    normal_variance: bool,
}
impl MapTask {
    /// Source capacity + existing codec allowance + exact base/tail capacities,
    /// one encoded row, exact level/reader metadata and this continuation.
    pub fn required_bytes(map: &TextureMap) -> Result<u64> {
        Self::required(map, true)
    }
    pub(crate) fn row_workspace_bytes(width: u32) -> u64 {
        CODEC_BYTES as u64 + u64::from(width) * 4 + size_of::<png::Reader<EncodedReader>>() as u64 + size_of::<Self>() as u64
    }
    fn required(map: &TextureMap, full_mips: bool) -> Result<u64> {
        let [width, height] = map.dimensions();
        if width == 0 || height == 0 || width > MAX_EDGE || height > MAX_EDGE {
            return Err(invalid("CPU map dimensions exceed 1024x1024"));
        }
        map.source().png_encoded().map_err(|error| invalid(error.to_string()))?;
        let mut pixels = 0_u64;
        let mut levels = 0_u64;
        let (mut w, mut h) = (width, height);
        loop {
            pixels = pixels.checked_add(u64::from(w) * u64::from(h) * 16).ok_or_else(|| invalid("CPU map pixel accounting overflow"))?;
            levels += 1;
            if !full_mips || (w == 1 && h == 1) {
                break;
            }
            w = (w / 2).max(1);
            h = (h / 2).max(1);
        }
        (map.retained_bytes() as u64)
            // One original Arc<Vec<u8>> allocation, not one charge per clone.
            .checked_add((size_of::<Vec<u8>>() + 2 * size_of::<usize>()) as u64)
            .and_then(|bytes| bytes.checked_add(CODEC_BYTES as u64))
            .and_then(|bytes| bytes.checked_add(pixels))
            .and_then(|bytes| bytes.checked_add(u64::from(width) * 4))
            .and_then(|bytes| bytes.checked_add(levels * size_of::<Level>() as u64))
            .and_then(|bytes| bytes.checked_add(size_of::<png::Reader<EncodedReader>>() as u64))
            .and_then(|bytes| bytes.checked_add(size_of::<Self>() as u64))
            .ok_or_else(|| invalid("CPU map preparation accounting overflow"))
    }
    pub fn new(map: TextureMap, max_bytes: u64, cancel: CancelToken) -> Result<Self> {
        Self::new_with_quality(map, max_bytes, cancel, false)
    }
    pub(crate) fn new_with_quality(map: TextureMap, max_bytes: u64, cancel: CancelToken, quality: bool) -> Result<Self> {
        Self::begin(map, max_bytes, cancel, true, quality)
    }
    fn begin(map: TextureMap, max_bytes: u64, cancel: CancelToken, full_mips: bool, quality: bool) -> Result<Self> {
        cancel.check()?;
        let working_bytes = Self::required(&map, full_mips)?;
        if max_bytes > crate::job::CpuBudget::Compact.limits().max_job_bytes || working_bytes > max_bytes {
            return Err(JobError::new(ErrorCode::LimitExceeded, "CPU map preparation exceeds admitted bytes"));
        }
        let [width, height] = map.dimensions();
        let (mut w, mut h) = (width, height);
        let mut count = 1;
        while full_mips && (w > 1 || h > 1) {
            count += 1;
            w = (w / 2).max(1);
            h = (h / 2).max(1);
        }
        let mut levels = Vec::new();
        levels.try_reserve_exact(count).map_err(|_| allocation())?;
        let mut pixels = Vec::new();
        let length = width as usize * height as usize;
        pixels.try_reserve_exact(length).map_err(|_| allocation())?;
        let mut row = Vec::new();
        row.try_reserve_exact(width as usize * 4).map_err(|_| allocation())?;
        let mut readers = Vec::new();
        readers.try_reserve_exact(1).map_err(|_| allocation())?;
        if levels.capacity() != count || pixels.capacity() != length || row.capacity() != width as usize * 4 || readers.capacity() != 1 {
            return Err(invalid("CPU map capacity exceeds admission"));
        }
        levels.push(Level { pixels, width, height });
        let role = map.role();
        Ok(Self {
            map,
            cancel,
            readers,
            metadata: PngMetadata::new(true, true).with_role(role),
            row,
            channels: 0,
            rows: 0,
            levels,
            stage: Stage::Metadata,
            working_bytes,
            full_mips,
            normal_variance: quality && role == MapRole::NormalOpenGl,
        })
    }
    pub fn working_bytes(&self) -> u64 {
        self.working_bytes
    }
    pub fn completed_rows(&self) -> u32 {
        self.rows
    }
    #[cfg(test)]
    pub(super) fn phase_name(&self) -> &'static str {
        match self.stage {
            Stage::Metadata => self.metadata.phase_name(),
            Stage::Begin => "begin",
            Stage::Row => "row",
            Stage::Pixel(_) => "linear",
            Stage::FinishPng => "finish_png",
            Stage::NewLevel => "new_level",
            Stage::MipPixel(_) => "mip",
            Stage::Done => "done",
        }
    }
    /// Each call prepays one metadata header/4096-byte CRC block/EXIF entry,
    /// PNG setup/one at-most-1024-wide
    /// decoded row, one linear pixel, one mip pixel (at most3x3), or a transition.
    /// Codec reads observe cancellation in at-most4096-byte source slices too.
    /// PNG codec/header work is bounded by its existing input/workspace limits;
    /// this is not a wall-time guarantee and it is not an installed image worker.
    pub fn step(&mut self, charge: &mut impl FnMut(u64) -> Result<()>) -> Result<bool> {
        self.cancel.check()?;
        if self.stage == Stage::Done {
            return Ok(true);
        }
        charge(1)?;
        self.cancel.check()?;
        match self.stage {
            Stage::Metadata => {
                let bytes = self.map.source().png_encoded().map_err(|error| invalid(error.to_string()))?;
                if self.metadata.step(&bytes).map_err(|error| invalid(error.to_string()))? {
                    self.stage = Stage::Begin;
                }
            }
            Stage::Begin => {
                let bytes = self.map.source().png_encoded().map_err(|error| invalid(error.to_string()))?;
                let cancel = self.cancel.clone();
                let mut decoder =
                    png::Decoder::new_with_limits(EncodedReader { bytes, position: 0, cancel: cancel.clone() }, png::Limits { bytes: CODEC_BYTES });
                decoder.set_transformations(png::Transformations::EXPAND);
                decoder.set_ignore_text_chunk(true);
                // Presence was already refused by the shared/cooperative metadata policy.
                decoder.set_ignore_iccp_chunk(true);
                let reader = guarded(&cancel, || decoder.read_info().map_err(|error| codec_error(&cancel, error.to_string())))?;
                let info = reader.info();
                if info.interlaced
                    || info.animation_control.is_some()
                    || info.frame_control.is_some()
                    || [info.width, info.height] != self.map.dimensions()
                {
                    return Err(invalid("CPU row preparation requires a static noninterlaced PNG with unchanged dimensions"));
                }
                let (color, depth) = reader.output_color_type();
                if depth != png::BitDepth::Eight {
                    return Err(invalid("CPU row preparation requires 8-bit PNG channels"));
                }
                self.channels = match color {
                    png::ColorType::Grayscale => 1,
                    png::ColorType::GrayscaleAlpha => 2,
                    png::ColorType::Rgb => 3,
                    png::ColorType::Rgba => 4,
                    _ => return Err(invalid("CPU row preparation requires gray, gray-alpha, RGB or RGBA PNG")),
                };
                self.readers.push(reader);
                self.stage = Stage::Row;
            }
            Stage::Row => {
                let reader = self.readers.first_mut().ok_or_else(|| invalid("CPU PNG reader missing"))?;
                let row = &mut self.row;
                row.clear();
                let width = self.map.dimensions()[0] as usize;
                let cancel = self.cancel.clone();
                let expected = width * self.channels;
                let found = guarded(&cancel, || {
                    let next = reader.next_row().map_err(|error| codec_error(&cancel, error.to_string()))?;
                    if let Some(next) = next {
                        if next.data().len() != expected || expected > row.capacity() {
                            return Err(invalid("CPU PNG row exceeds admission"));
                        }
                        row.extend_from_slice(next.data());
                        Ok(true)
                    } else {
                        Ok(false)
                    }
                })?;
                if found {
                    if self.rows >= self.map.dimensions()[1] {
                        return Err(invalid("CPU PNG has excessive rows"));
                    }
                    self.stage = Stage::Pixel(0);
                } else {
                    if self.rows != self.map.dimensions()[1] {
                        return Err(invalid("CPU PNG ended before a complete map"));
                    }
                    self.stage = Stage::FinishPng;
                }
            }
            Stage::Pixel(x) => {
                let start = x * self.channels;
                let bytes = self.row.get(start..start + self.channels).ok_or_else(|| invalid("CPU PNG row pixel missing"))?;
                let channel = |index| bytes.get(index).copied().ok_or_else(|| invalid("CPU PNG channel missing"));
                let rgba = match self.channels {
                    1 => {
                        let g = channel(0)?;
                        [g, g, g, 255]
                    }
                    2 => {
                        let g = channel(0)?;
                        [g, g, g, channel(1)?]
                    }
                    3 => [channel(0)?, channel(1)?, channel(2)?, 255],
                    4 => [channel(0)?, channel(1)?, channel(2)?, channel(3)?],
                    _ => return Err(invalid("CPU PNG channels invalid")),
                };
                let mut pixel = [0_f32; 4];
                for (index, value) in pixel.iter_mut().enumerate() {
                    *value = if index < 3 && self.map.role() == MapRole::BaseColor {
                        srgb8_to_linear(rgba[index]) as f32
                    } else {
                        f32::from(rgba[index]) / 255.0
                    };
                }
                if self.normal_variance {
                    let normal = unit_normal_texel([pixel[0], pixel[1], pixel[2]]);
                    pixel[..3].copy_from_slice(&normal);
                }
                let base = self.levels.first_mut().ok_or_else(|| invalid("CPU map base missing"))?;
                push_pixel(base, pixel)?;
                if x + 1 == base.width as usize {
                    self.rows += 1;
                    self.stage = Stage::Row;
                } else {
                    self.stage = Stage::Pixel(x + 1);
                }
            }
            Stage::FinishPng => {
                // next_row(None) has consumed/flushed the full IDAT sequence. The
                // cooperative prepass already verified every original tail chunk,
                // including CRC/order/IEND and metadata policy, before this codec.
                // Do not parse unbounded tail metadata again in Reader::finish().
                self.readers = Vec::new();
                self.row = Vec::new();
                self.stage = Stage::NewLevel;
            }
            Stage::NewLevel => {
                let prior = self.levels.last().ok_or_else(|| invalid("CPU mip source missing"))?;
                if !self.full_mips || (prior.width == 1 && prior.height == 1) {
                    self.stage = Stage::Done;
                    return Ok(true);
                }
                let width = (prior.width / 2).max(1);
                let height = (prior.height / 2).max(1);
                let length = width as usize * height as usize;
                if self.levels.len() == self.levels.capacity() {
                    return Err(invalid("CPU mip metadata exceeds admission"));
                }
                let mut pixels = Vec::new();
                pixels.try_reserve_exact(length).map_err(|_| allocation())?;
                if pixels.capacity() != length {
                    return Err(invalid("CPU mip pixels exceed admission"));
                }
                self.levels.push(Level { pixels, width, height });
                self.stage = Stage::MipPixel(0);
            }
            Stage::MipPixel(pixel) => {
                let prior_index = self.levels.len().checked_sub(2).ok_or_else(|| invalid("CPU mip predecessor missing"))?;
                let source = self.levels.get(prior_index).ok_or_else(|| invalid("CPU mip source missing"))?;
                let target = self.levels.last().ok_or_else(|| invalid("CPU mip destination missing"))?;
                let (width, height) = (target.width, target.height);
                let rgb = mips::area_average(source.view()?, pixel % width as usize, pixel / width as usize, width as usize, height as usize)
                    .map_err(|error| invalid(error.to_string()))?;
                let target = self.levels.last_mut().ok_or_else(|| invalid("CPU mip destination missing"))?;
                push_pixel(target, [rgb[0] as f32, rgb[1] as f32, rgb[2] as f32, 1.0])?;
                self.stage = if pixel + 1 == width as usize * height as usize { Stage::NewLevel } else { Stage::MipPixel(pixel + 1) };
            }
            Stage::Done => return Ok(true),
        }
        self.cancel.check()?;
        Ok(false)
    }
    pub fn finish(self) -> Result<PreparedMap> {
        self.cancel.check()?;
        if self.stage != Stage::Done || self.levels.iter().any(|level| level.pixels.len() != level.width as usize * level.height as usize) {
            return Err(invalid("CPU map preparation is incomplete"));
        }
        Ok(PreparedMap { identity: self.map.identity(), role: self.map.role(), levels: self.levels, normal_variance: self.normal_variance })
    }
}
fn push_pixel(level: &mut Level, pixel: [f32; 4]) -> Result<()> {
    if pixel.into_iter().any(|value| !value.is_finite() || !(0.0..=1.0).contains(&value)) || level.pixels.len() >= level.pixels.capacity() {
        return Err(invalid("CPU map pixel exceeds admitted values or capacity"));
    }
    level.pixels.push(pixel);
    Ok(())
}

/// Optical sample coordinates only. The original winding/offset normal stays
/// owned by the image consumer; geometric_normal here is oriented toward view.
#[derive(Clone, Copy, Debug)]
pub struct MapPoint {
    pub position: Vec3,
    pub geometric_normal: Vec3,
    pub view_direction: Vec3,
}

pub struct PreparedMap {
    identity: ([u8; 32], u8),
    role: MapRole,
    levels: Vec<Level>,
    normal_variance: bool,
}
impl PreparedMap {
    pub(crate) fn level_metadata_bytes() -> u64 {
        size_of::<Level>() as u64
    }
    pub fn identity(&self) -> (([u8; 32], u8), &'static str) {
        let recipe = if self.normal_variance { "solidcraft.unit-normal-area-mips/1" } else { mips::MIP_RECIPE_ID };
        (self.identity, recipe)
    }
    pub fn level_count(&self) -> usize {
        self.levels.len()
    }
    pub fn dimensions(&self, level: usize) -> Option<[u32; 2]> {
        self.levels.get(level).map(|value| [value.width, value.height])
    }
    /// Exact retained capacities; caller owns the single existing lifetime lease.
    pub fn retained_bytes(&self) -> u64 {
        size_of::<Self>() as u64
            + self.levels.capacity() as u64 * size_of::<Level>() as u64
            + self.levels.iter().map(|level| level.pixels.capacity() as u64 * 16).sum::<u64>()
    }
    /// Trilinear repeating RGB in linear light, with explicit footprint-selected LOD.
    pub fn sample(&self, uv: [f64; 2], lod: f64) -> Result<[f64; 3]> {
        let last = self.levels.len().checked_sub(1).ok_or_else(|| invalid("CPU prepared map has no levels"))?;
        if !lod.is_finite() || lod < 0.0 || lod > last as f64 {
            return Err(invalid("CPU explicit mip LOD outside prepared levels"));
        }
        let low = lod.floor() as usize;
        let high = (low + 1).min(last);
        let fraction = lod - low as f64;
        let sample = |index: usize| {
            self.levels
                .get(index)
                .ok_or_else(|| invalid("CPU mip level missing"))?
                .view()?
                .sample_repeat_opaque(uv)
                .map_err(|error| invalid(error.to_string()))
        };
        let a = sample(low)?;
        if fraction == 0.0 || high == low {
            return Ok(a);
        }
        let b = sample(high)?;
        Ok(std::array::from_fn(|index| a[index] * (1.0 - fraction) + b[index] * fraction))
    }
    /// Filter along the major axis of an unwrapped UV footprint with at most
    /// eight taps. The minor axis sets the trilinear mip level.
    pub(crate) fn sample_anisotropic(&self, uv: [f64; 2], dx: [f64; 2], dy: [f64; 2]) -> Result<([f64; 3], f64)> {
        let [width, height] = self.dimensions(0).ok_or_else(|| invalid("CPU map base dimensions missing"))?;
        let size = [f64::from(width), f64::from(height)];
        let u = [dx[0] * size[0], dy[0] * size[0]];
        let v = [dx[1] * size[1], dy[1] * size[1]];
        let dot = |a: [f64; 2], b: [f64; 2]| a[0] * b[0] + a[1] * b[1];
        let (a, b, c) = (dot(u, u), dot(u, v), dot(v, v));
        let separation = (a - c).hypot(2.0 * b);
        let eigenvalue = (a + c + separation) * 0.5;
        let major = eigenvalue.max(0.0).sqrt();
        let minor = ((a + c - separation) * 0.5).max(0.0).sqrt();
        let filter_width = minor.max(major / 8.0).max(1.0);
        let last = (self.level_count() - 1) as f64;
        let lod = filter_width.log2().clamp(0.0, last);
        let minification = if last == 0.0 { 0.0 } else { lod.max(major - 1.0).clamp(0.0, 1.0) };
        if lod == last || major <= 1.0 {
            return Ok((self.sample(uv, lod)?, minification));
        }
        let eigen = [b, eigenvalue - a];
        let eigen_length = eigen[0].hypot(eigen[1]);
        let axis = if eigen_length > 1e-12 {
            [eigen[0] / eigen_length, eigen[1] / eigen_length]
        } else if a >= c {
            [1.0, 0.0]
        } else {
            [0.0, 1.0]
        };
        let taps = (major / filter_width).ceil().clamp(1.0, 8.0) as usize;
        let mut result = [0.0; 3];
        for tap in 0..taps {
            let offset = ((tap as f64 + 0.5) / taps as f64 - 0.5) * major;
            let sample = self.sample([uv[0] + axis[0] * offset / size[0], uv[1] + axis[1] * offset / size[1]], lod)?;
            for channel in 0..3 {
                result[channel] += sample[channel] / taps as f64;
            }
        }
        Ok((result, minification))
    }
    pub fn apply(&self, binding: &TextureBinding, point: MapPoint, original: ShadingSurface, lod: f64) -> Result<ShadingSurface> {
        self.apply_sample(binding, point, original, self.sample(binding.uv(point.position)?, lod)?, lod.clamp(0.0, 1.0))
    }
    pub(crate) fn apply_anisotropic(
        &self,
        binding: &TextureBinding,
        point: MapPoint,
        original: ShadingSurface,
        dx: [f64; 2],
        dy: [f64; 2],
    ) -> Result<ShadingSurface> {
        let (rgb, minification) = self.sample_anisotropic(binding.uv(point.position)?, dx, dy)?;
        self.apply_sample(binding, point, original, rgb, minification)
    }
    fn apply_sample(
        &self,
        binding: &TextureBinding,
        point: MapPoint,
        original: ShadingSurface,
        rgb: [f64; 3],
        minification: f64,
    ) -> Result<ShadingSurface> {
        let shading = solidcraft_viewport::surface::unit_direction(original.shading_normal).map_err(|error| invalid(error.to_string()))?;
        let geometric = solidcraft_viewport::surface::unit_direction(point.geometric_normal).map_err(|error| invalid(error.to_string()))?;
        let view = solidcraft_viewport::surface::unit_direction(point.view_direction).map_err(|error| invalid(error.to_string()))?;
        if geometric.dot(view) <= 0.0 {
            return Err(invalid("CPU mapped surface view crosses its oriented geometric surface"));
        }
        // Match the GPU's pre-map shading-normal and post-map view guards.
        let normal = if shading.dot(view) > 0.0 { shading } else { geometric };
        let source = match self.role {
            MapRole::BaseColor => &binding.base_color,
            MapRole::NormalOpenGl => &binding.normal,
            MapRole::Roughness => &binding.roughness,
            MapRole::Metallic => &binding.metallic,
            _ => return Err(invalid("CPU map role unsupported")),
        };
        if source.as_ref().is_none_or(|source| source.identity() != self.identity) {
            return Err(invalid("CPU prepared map does not match its binding source/role"));
        }
        if self.role == MapRole::NormalOpenGl {
            let mapped = binding.frame.mapped_normal(normal, geometric, rgb, binding.normal_strength)?;
            let mut surface = original.surface;
            if self.normal_variance && binding.normal_strength > 0.0 && minification > 0.0 {
                let variance = normal_direction_variance(rgb) * minification;
                // Constant directions can lose a few bits during float reconstruction.
                if variance > 1e-6 {
                    let alpha = surface.roughness() * surface.roughness();
                    let broadened = (alpha * alpha + variance * binding.normal_strength.powi(2)).min(1.0).sqrt().sqrt();
                    surface = surface
                        .with_authored_finish(surface.base_color(), surface.metallic(), broadened)
                        .map_err(|error| invalid(error.to_string()))?;
                }
            }
            return Ok(ShadingSurface { surface, shading_normal: if mapped.dot(view) > 0.0 { mapped } else { normal } });
        }
        let surface = original.surface;
        let base = if self.role == MapRole::BaseColor {
            match binding.base_color_mode {
                BaseColorMode::Replace => Vec3::new(rgb[0], rgb[1], rgb[2]),
                BaseColorMode::Modulate => {
                    Vec3::new(surface.base_color().x * rgb[0], surface.base_color().y * rgb[1], surface.base_color().z * rgb[2])
                }
            }
        } else {
            surface.base_color()
        };
        let roughness = if self.role == MapRole::Roughness { rgb[0].clamp(MIN_ROUGHNESS, 1.0) } else { surface.roughness() };
        let metallic = if self.role == MapRole::Metallic { rgb[0] } else { surface.metallic() };
        let changed = surface.with_authored_finish(base, metallic, roughness).map_err(|error| invalid(error.to_string()))?;
        Ok(ShadingSurface { surface: changed, shading_normal: normal })
    }
}
