//! Bounded linear area-filtered mip preparation for opaque CPU reference sampling.
//! Call from the existing preparation host; row cancellation does not yield a browser event loop.
use super::{SamplingError, TextureView};

/// Derived pixel storage only, excluding the borrowed base and small level metadata.
pub const MAX_MIP_TAIL_BYTES: usize = 32 * 1024 * 1024;
/// Include this recipe in preparation/cache identity, alongside source/role/policies.
/// V1 means floor-halved linear area filtering, opaque RGB and generated alpha=1.
pub const MIP_RECIPE_ID: &str = "solidcraft.opaque-linear-area-mips/1";
const MAX_LEVELS: usize = 12;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MipError {
    Budget,
    Allocation,
    Cancelled,
    Lod,
    Sampling(SamplingError),
}
impl std::fmt::Display for MipError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Budget => f.write_str("mip tail storage exceeds the admitted budget"),
            Self::Allocation => f.write_str("cannot allocate mip tail storage"),
            Self::Cancelled => f.write_str("mip preparation cancelled"),
            Self::Lod => f.write_str("LOD must be finite and within the prepared level range"),
            Self::Sampling(e) => e.fmt(f),
        }
    }
}
impl std::error::Error for MipError {}
impl From<SamplingError> for MipError {
    fn from(value: SamplingError) -> Self {
        Self::Sampling(value)
    }
}

struct Level {
    rgba: Vec<f32>,
    width: usize,
    height: usize,
}
impl Level {
    fn view(&self) -> TextureView<'_> {
        // Private levels are created only from admitted pixels and convex averages.
        // Reuse validation rather than rescan every stored pixel on each sample.
        TextureView { rgba: &self.rgba, width: self.width, height: self.height }
    }
}

/// Immutable derived levels; level zero borrows the admitted base without copying it.
/// Generated levels have alpha=1, matching the explicitly opaque RGB sampler.
pub struct MipChain<'a> {
    base: TextureView<'a>,
    tail: Vec<Level>,
    tail_bytes: usize,
    retained_bytes: usize,
}
impl<'a> MipChain<'a> {
    /// Generate all floor-halved levels through 1x1, or return no chain on failure.
    /// `max_tail_bytes` bounds derived pixel capacity, not total process memory.
    /// Preparation is synchronous and must run in the caller-owned preparation host.
    pub fn prepare(base: TextureView<'a>, max_tail_bytes: usize, cancelled: impl Fn() -> bool) -> Result<Self, MipError> {
        if max_tail_bytes > MAX_MIP_TAIL_BYTES {
            return Err(MipError::Budget);
        }
        check_cancel(&cancelled)?;
        let required = required_bytes(base.width, base.height)?;
        if required > max_tail_bytes {
            return Err(MipError::Budget);
        }
        let mut chain = Self { base, tail: Vec::new(), tail_bytes: 0, retained_bytes: size_of::<Self>() };
        chain.tail.try_reserve_exact(MAX_LEVELS - 1).map_err(|_| MipError::Allocation)?;
        let metadata_bytes =
            chain.tail.capacity().checked_mul(size_of::<Level>()).and_then(|n| n.checked_add(size_of::<Self>())).ok_or(MipError::Budget)?;
        chain.retained_bytes = metadata_bytes;
        while chain.level_count() < MAX_LEVELS {
            check_cancel(&cancelled)?;
            let source = chain.level(chain.level_count() - 1).ok_or(MipError::Budget)?;
            if source.width == 1 && source.height == 1 {
                break;
            }
            let width = (source.width / 2).max(1);
            let height = (source.height / 2).max(1);
            let length = width.checked_mul(height).and_then(|n| n.checked_mul(4)).ok_or(MipError::Budget)?;
            let mut rgba = Vec::new();
            rgba.try_reserve_exact(length).map_err(|_| MipError::Allocation)?;
            let bytes = rgba.capacity().checked_mul(size_of::<f32>()).ok_or(MipError::Budget)?;
            let next_bytes = chain.tail_bytes.checked_add(bytes).ok_or(MipError::Budget)?;
            if next_bytes > max_tail_bytes {
                return Err(MipError::Budget);
            }
            let retained_bytes = metadata_bytes.checked_add(next_bytes).ok_or(MipError::Budget)?;
            for y in 0..height {
                check_cancel(&cancelled)?;
                for x in 0..width {
                    let rgb = area_average(source, x, y, width, height)?;
                    rgba.extend(rgb.map(|v| v as f32));
                    rgba.push(1.0);
                }
            }
            check_cancel(&cancelled)?;
            chain.tail.push(Level { rgba, width, height });
            chain.tail_bytes = next_bytes;
            chain.retained_bytes = retained_bytes;
        }
        check_cancel(&cancelled)?;
        Ok(chain)
    }

    pub fn level_count(&self) -> usize {
        self.tail.len() + 1
    }
    pub fn tail_bytes(&self) -> usize {
        self.tail_bytes
    }
    /// Retained pixel/level Vec capacities plus the chain value. Charge to the
    /// existing host pool; borrowed base ownership and allocator overhead are separate.
    pub fn retained_bytes(&self) -> usize {
        self.retained_bytes
    }
    /// Borrow a prepared level without allocation or another full-image admission scan.
    pub fn level(&self, index: usize) -> Option<TextureView<'_>> {
        if index == 0 { Some(self.base) } else { self.tail.get(index - 1).map(Level::view) }
    }
    pub fn dimensions(&self, index: usize) -> Option<[u32; 2]> {
        self.level(index).map(|v| [v.width as u32, v.height as u32])
    }

    /// Explicit LOD only: bilinear within each level, linear between adjacent levels.
    /// This does not choose a footprint, provide anisotropy or certify GPU parity.
    /// Out-of-range LOD refuses rather than silently clamping to a different policy.
    pub fn sample_repeat_lod_opaque(&self, uv: [f64; 2], lod: f64) -> Result<[f64; 3], MipError> {
        let last = self.level_count() - 1;
        if !lod.is_finite() || lod < 0.0 || lod > last as f64 {
            return Err(MipError::Lod);
        }
        let low = lod.floor() as usize;
        let high = (low + 1).min(last);
        let t = lod - low as f64;
        let a = self.level(low).ok_or(MipError::Lod)?.sample_repeat_opaque(uv)?;
        let b = self.level(high).ok_or(MipError::Lod)?.sample_repeat_opaque(uv)?;
        let mut rgb = [0.0; 3];
        for ((output, a), b) in rgb.iter_mut().zip(a).zip(b) {
            *output = a * (1.0 - t) + b * t;
        }
        Ok(rgb)
    }
}

fn check_cancel(cancelled: &impl Fn() -> bool) -> Result<(), MipError> {
    if cancelled() { Err(MipError::Cancelled) } else { Ok(()) }
}

fn required_bytes(mut width: usize, mut height: usize) -> Result<usize, MipError> {
    let mut bytes = 0usize;
    for _ in 1..MAX_LEVELS {
        if width == 1 && height == 1 {
            return Ok(bytes);
        }
        width = (width / 2).max(1);
        height = (height / 2).max(1);
        let level_bytes = width.checked_mul(height).and_then(|n| n.checked_mul(4 * size_of::<f32>())).ok_or(MipError::Budget)?;
        bytes = bytes.checked_add(level_bytes).ok_or(MipError::Budget)?;
    }
    if width == 1 && height == 1 { Ok(bytes) } else { Err(MipError::Budget) }
}

pub(crate) fn area_average(source: TextureView<'_>, x: usize, y: usize, width: usize, height: usize) -> Result<[f64; 3], MipError> {
    // Integer overlap numerators avoid dropping the final row/column at odd sizes.
    // All factors are bounded by the validated 2048 edge; at most 3x3 source cells.
    let x_start = x * source.width;
    let x_end = (x + 1) * source.width;
    let y_start = y * source.height;
    let y_end = (y + 1) * source.height;
    let mut rgb = [0.0; 3];
    for sy in y_start / height..y_end.div_ceil(height) {
        let wy = ((sy + 1) * height).min(y_end) - (sy * height).max(y_start);
        for sx in x_start / width..x_end.div_ceil(width) {
            let wx = ((sx + 1) * width).min(x_end) - (sx * width).max(x_start);
            let weight = wx as f64 * wy as f64 / (source.width as f64 * source.height as f64);
            for (channel, output) in rgb.iter_mut().enumerate() {
                *output += source.channel(sx, sy, channel)? * weight;
            }
        }
    }
    Ok(rgb)
}

#[cfg(test)]
mod tests;
