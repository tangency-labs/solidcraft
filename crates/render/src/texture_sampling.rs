//! Allocation-free periodic reference sampling of already normalized, linear RGBA.
//! No decoding, transfer function, normal interpretation or renderer capability is implied.

use crate::texture_input::MAX_TEXTURE_EDGE;

pub mod mips;

/// CPU reference bound: beyond this, fractional repeat precision is deliberately refused.
/// This is not a GPU/f32 coordinate precision guarantee.
pub const MAX_REPEAT_COORDINATE: f64 = 1_048_576.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SamplingError {
    Dimensions,
    Length,
    Values,
    Coordinates,
}
impl std::fmt::Display for SamplingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Dimensions => "texture dimensions must be positive and at most 2048",
            Self::Length => "texture storage must be exactly width * height * 4 floats",
            Self::Values => "normalized texture channels must be finite and in [0, 1]",
            Self::Coordinates => "repeat coordinates must be finite and within the CPU reference bound",
        })
    }
}
impl std::error::Error for SamplingError {}

/// Validated borrowed normalized RGBA. Construction scans the bounded input once;
/// sampling does constant work, without allocations. The caller retains ownership.
#[derive(Clone, Copy, Debug)]
pub struct TextureView<'a> {
    rgba: &'a [f32],
    width: usize,
    height: usize,
}
impl<'a> TextureView<'a> {
    /// Borrow the admitted linear RGBA storage without a copy or another scan.
    pub fn rgba(&self) -> &'a [f32] {
        self.rgba
    }

    pub fn new(rgba: &'a [f32], dimensions: [u32; 2]) -> Result<Self, SamplingError> {
        let [width, height] = dimensions;
        if width == 0 || height == 0 || width > MAX_TEXTURE_EDGE || height > MAX_TEXTURE_EDGE {
            return Err(SamplingError::Dimensions);
        }
        let width = usize::try_from(width).map_err(|_| SamplingError::Dimensions)?;
        let height = usize::try_from(height).map_err(|_| SamplingError::Dimensions)?;
        let length = width.checked_mul(height).and_then(|n| n.checked_mul(4)).ok_or(SamplingError::Length)?;
        if rgba.len() != length {
            return Err(SamplingError::Length);
        }
        if rgba.iter().any(|v| !v.is_finite() || !(0.0..=1.0).contains(v)) {
            return Err(SamplingError::Values);
        }
        Ok(Self { rgba, width, height })
    }

    /// Shape-only borrow of private immutable storage whose builder already
    /// validated every sample incrementally. Never use on unvalidated external
    /// pixels: this avoids an image-wide scan on every future mapped ray.
    #[cfg(feature = "raytrace-cpu")]
    pub(crate) fn from_admitted(rgba: &'a [f32], dimensions: [u32; 2]) -> Result<Self, SamplingError> {
        let [width, height] = dimensions;
        if width == 0 || height == 0 || width > MAX_TEXTURE_EDGE || height > MAX_TEXTURE_EDGE {
            return Err(SamplingError::Dimensions);
        }
        if rgba.len() != width as usize * height as usize * 4 {
            return Err(SamplingError::Length);
        }
        Ok(Self { rgba, width: width as usize, height: height as usize })
    }

    /// Bilinear level-zero RGB for an explicitly opaque consumer. Alpha is validated
    /// but deliberately ignored (even zero-alpha RGB participates). This is NOT
    /// transparent compositing or premultiplied-alpha filtering.
    /// UV (0,0) is the top-left image boundary, and v increases downwards. Texel
    /// centers are ((x+.5)/width, (y+.5)/height). Both axes repeat, without a Y flip.
    /// Inputs must already be linear color or linear data; no transfer is applied.
    pub fn sample_repeat_opaque(&self, uv: [f64; 2]) -> Result<[f64; 3], SamplingError> {
        if uv.iter().any(|v| !v.is_finite() || v.abs() > MAX_REPEAT_COORDINATE) {
            return Err(SamplingError::Coordinates);
        }
        let [u, v] = uv;
        let (x0, x1, tx) = axis(u, self.width);
        let (y0, y1, ty) = axis(v, self.height);
        let mut rgb = [0.0; 3];
        for (channel, output) in rgb.iter_mut().enumerate() {
            let a = self.channel(x0, y0, channel)?;
            let b = self.channel(x1, y0, channel)?;
            let c = self.channel(x0, y1, channel)?;
            let d = self.channel(x1, y1, channel)?;
            *output = (a * (1.0 - tx) + b * tx) * (1.0 - ty) + (c * (1.0 - tx) + d * tx) * ty;
        }
        Ok(rgb)
    }

    fn channel(&self, x: usize, y: usize, channel: usize) -> Result<f64, SamplingError> {
        self.rgba.get((y * self.width + x) * 4 + channel).copied().map(f64::from).ok_or(SamplingError::Length)
    }
}

fn axis(coordinate: f64, size: usize) -> (usize, usize, f64) {
    // Reduce before scaling: no huge-coordinate multiplication or unbounded index.
    let position = coordinate.rem_euclid(1.0) * size as f64 - 0.5;
    let lower = position.floor();
    let first = (lower as i64).rem_euclid(size as i64) as usize;
    (first, (first + 1) % size, position - lower)
}

#[cfg(test)]
mod tests;
