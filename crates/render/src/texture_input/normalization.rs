//! Explicit high-precision normalization. No sampler, persistent resource or capability.

mod icc;

mod decode;

use super::{MapRole, Result, admitted_format, guarded, invalid};
use image::{ImageFormat, metadata::Orientation};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::sync::Arc;

pub const MAX_WORKING_BYTES: usize = 256 * 1024 * 1024;
pub const MAX_NORMALIZED_BYTES: usize = 64 * 1024 * 1024;
const GAMUT_TOLERANCE: f64 = 3.0 / 65535.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UnprofiledColor {
    Refuse,
    /// Declare sRGB primaries; use stated PNG gamma, or sRGB transfer when absent.
    DeclareSrgbPrimaries,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OrientationPolicy {
    Refuse,
    Apply,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GamutPolicy {
    /// Refuse values beyond the documented finite-precision edge tolerance.
    Refuse,
    /// Explicit preview policy, with every clamped RGB channel counted in the receipt.
    Clip,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct NormalizationOptions {
    pub unprofiled_color: UnprofiledColor,
    pub orientation: OrientationPolicy,
    pub gamut: GamutPolicy,
    /// Conservative buffer/workspace admission estimate, not a process-RSS ceiling.
    pub max_working_bytes: usize,
}
impl Default for NormalizationOptions {
    fn default() -> Self {
        Self {
            unprofiled_color: UnprofiledColor::Refuse,
            orientation: OrientationPolicy::Refuse,
            gamut: GamutPolicy::Refuse,
            max_working_bytes: MAX_WORKING_BYTES,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ColorConversion {
    RawData,
    DeclaredSrgb,
    PngSrgb,
    PngGammaSrgbPrimaries { encoding_gamma: f64 },
    IccMatrixTrc { profile_sha256: String },
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct NormalizationReceipt {
    pub recipe_version: u32,
    pub source_sha256: String,
    pub normalized_sha256: String,
    pub format: String,
    pub encoded_bytes: usize,
    pub retained_encoded_capacity: usize,
    pub normalized_bytes: usize,
    pub role: MapRole,
    pub source_dimensions: [u32; 2],
    pub source_channel_bits: u8,
    pub output_dimensions: [u32; 2],
    pub output_encoding: String,
    pub output_precision: String,
    pub source_exif_orientation: u8,
    pub exif_metadata_present: bool,
    pub exif_orientation_tag_present: bool,
    pub orientation_applied: bool,
    pub conversion: ColorConversion,
    pub ignored_png_color_metadata: bool,
    pub gamut_policy: GamutPolicy,
    pub gamut_tolerance: f64,
    pub clamped_rgb_channels: u64,
    pub estimated_working_bytes: usize,
    pub options: NormalizationOptions,
}

/// Encoded source only. Decoding, metadata admission and normalization happen in `decode`.
/// The caller must schedule whole-image codec/CMS work away from an interactive UI thread.
#[derive(Clone)]
pub struct NormalizationSource {
    bytes: Arc<Vec<u8>>,
    format: ImageFormat,
    role: MapRole,
    options: NormalizationOptions,
}
impl NormalizationSource {
    pub fn new(bytes: Vec<u8>, role: MapRole, options: NormalizationOptions) -> Result<Self> {
        let format = admitted_format(&bytes, bytes.capacity())?;
        if options.max_working_bytes == 0 || options.max_working_bytes > MAX_WORKING_BYTES {
            return Err(invalid("normalization working budget must be positive and at most 256 MiB"));
        }
        if bytes.capacity().checked_add(decode::CODEC_BYTES).is_none_or(|minimum| minimum > options.max_working_bytes) {
            return Err(invalid("normalization working budget must cover encoded storage and the codec reservation"));
        }
        Ok(Self { bytes: Arc::new(bytes), format, role, options })
    }

    pub fn decode(&self, cancelled: impl Fn() -> bool) -> Result<NormalizedTexture> {
        check_cancel(&cancelled)?;
        let raw = guarded(|| decode::image(&self.bytes, self.format, self.options.max_working_bytes, self.bytes.capacity()))?;
        check_cancel(&cancelled)?;
        let orientation = raw.orientation;
        if orientation != Orientation::NoTransforms {
            if self.options.orientation == OrientationPolicy::Refuse {
                return Err(invalid("normalization requires explicit orientation consent"));
            }
            if self.role == MapRole::NormalOpenGl {
                return Err(invalid("oriented normal maps need an explicit vector/basis conversion policy"));
            }
        }
        let conversion = raw.conversion(self.role, self.options.unprofiled_color)?;
        let cms = match &conversion {
            ColorConversion::IccMatrixTrc { .. } => {
                let profile = raw.icc.as_ref().ok_or_else(|| invalid("ICC metadata disappeared"))?;
                Some(icc::prepare(profile)?)
            }
            _ => None,
        };
        check_cancel(&cancelled)?;
        let [width, height] = raw.dimensions;
        let dimensions = match orientation {
            Orientation::Rotate90 | Orientation::Rotate270 | Orientation::Rotate90FlipH | Orientation::Rotate270FlipH => [height, width],
            _ => [width, height],
        };
        let sample_count = usize::try_from(u64::from(width) * u64::from(height) * 4).map_err(|_| invalid("normalized image size overflow"))?;
        if sample_count.checked_mul(4).is_none_or(|n| n > MAX_NORMALIZED_BYTES) {
            return Err(invalid("normalized float pixels exceed 64 MiB"));
        }
        let mut pixels = zeroed::<f32>(sample_count)?;
        let row_channels = usize::try_from(u64::from(width) * 3).map_err(|_| invalid("normalization row overflow"))?;
        let mut input = zeroed::<f64>(row_channels)?;
        let mut converted = zeroed::<f64>(row_channels)?;
        let mut clamped_rgb_channels = 0;
        for y in 0..height {
            check_cancel(&cancelled)?;
            for (x, channels) in input.as_chunks_mut::<3>().0.iter_mut().enumerate() {
                let [r, g, b, _] = raw.pixel(u32::try_from(x).map_err(|_| invalid("pixel coordinate overflow"))?, y)?;
                channels.copy_from_slice(&[r, g, b]);
            }
            if let Some(cms) = &cms {
                guarded(|| cms.transform(&input, &mut converted).map_err(|e| invalid(format!("ICC conversion: {e}"))))?;
            } else {
                converted.copy_from_slice(&input);
            }
            for (x, rgb) in converted.as_chunks::<3>().0.iter().enumerate() {
                let x = u32::try_from(x).map_err(|_| invalid("pixel coordinate overflow"))?;
                let [_, _, _, alpha] = raw.pixel(x, y)?;
                let rgb = *rgb;
                let mut rgba = [0.0_f32; 4];
                for (destination, value) in rgba.iter_mut().take(3).zip(rgb) {
                    if !value.is_finite() {
                        return Err(invalid("color conversion produced nonfinite values"));
                    }
                    if self.options.gamut == GamutPolicy::Refuse && !(-GAMUT_TOLERANCE..=1.0 + GAMUT_TOLERANCE).contains(&value) {
                        return Err(invalid("color conversion is outside sRGB gamut; explicit preview clipping is required"));
                    }
                    let bounded = value.clamp(0.0, 1.0);
                    if bounded != value {
                        clamped_rgb_channels += 1;
                    }
                    let linear = match conversion {
                        ColorConversion::RawData => bounded,
                        ColorConversion::PngGammaSrgbPrimaries { encoding_gamma } => bounded.powf(1.0 / encoding_gamma),
                        _ => solidcraft_geom::srgb::decode(bounded),
                    };
                    if !linear.is_finite() || !(0.0..=1.0).contains(&linear) {
                        return Err(invalid("normalization produced invalid linear/data channels"));
                    }
                    *destination = linear as f32;
                }
                if !alpha.is_finite() || !(0.0..=1.0).contains(&alpha) {
                    return Err(invalid("normalization produced invalid alpha"));
                }
                if let Some(destination) = rgba.get_mut(3) {
                    *destination = alpha as f32;
                }
                let index = destination_index(raw.dimensions, dimensions, orientation, x, y)?;
                let end = index.checked_add(4).ok_or_else(|| invalid("oriented pixel offset overflow"))?;
                pixels.get_mut(index..end).ok_or_else(|| invalid("oriented pixel lies outside its output"))?.copy_from_slice(&rgba);
            }
        }
        let source_sha256 = digest_bytes(&self.bytes, &cancelled)?;
        let mut pixel_hash = Sha256::new();
        for chunk in pixels.chunks(4096) {
            check_cancel(&cancelled)?;
            for value in chunk {
                pixel_hash.update(value.to_le_bytes());
            }
        }
        check_cancel(&cancelled)?;
        let receipt = NormalizationReceipt {
            recipe_version: 1,
            source_sha256,
            normalized_sha256: format!("{:x}", pixel_hash.finalize()),
            format: if self.format == ImageFormat::Png { "png" } else { "jpeg" }.into(),
            encoded_bytes: self.bytes.len(),
            retained_encoded_capacity: self.bytes.capacity(),
            normalized_bytes: sample_count * 4,
            role: self.role,
            source_dimensions: raw.dimensions,
            source_channel_bits: raw.source_bits,
            output_dimensions: dimensions,
            output_encoding: if self.role == MapRole::BaseColor { "linear_srgb" } else { "data" }.into(),
            output_precision: "rgba_f32".into(),
            source_exif_orientation: orientation.to_exif(),
            exif_metadata_present: raw.exif_present,
            exif_orientation_tag_present: raw.orientation_tag_present,
            orientation_applied: orientation != Orientation::NoTransforms,
            ignored_png_color_metadata: self.role != MapRole::BaseColor && raw.has_png_color_metadata(),
            conversion,
            gamut_policy: self.options.gamut,
            gamut_tolerance: GAMUT_TOLERANCE,
            clamped_rgb_channels,
            estimated_working_bytes: raw.estimated_working_bytes,
            options: self.options,
        };
        Ok(NormalizedTexture { receipt, pixels })
    }
}

pub struct NormalizedTexture {
    receipt: NormalizationReceipt,
    pixels: Vec<f32>,
}
impl std::fmt::Debug for NormalizedTexture {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NormalizedTexture").field("receipt", &self.receipt).field("sample_count", &self.pixels.len()).finish()
    }
}
impl NormalizedTexture {
    pub fn receipt(&self) -> &NormalizationReceipt {
        &self.receipt
    }
    pub fn rgba(&self) -> &[f32] {
        &self.pixels
    }
}

fn zeroed<T: Default + Clone>(length: usize) -> Result<Vec<T>> {
    let mut values = Vec::new();
    values.try_reserve_exact(length).map_err(|_| invalid("cannot allocate normalization buffer"))?;
    values.resize(length, T::default());
    Ok(values)
}
fn check_cancel(cancelled: &impl Fn() -> bool) -> Result<()> {
    if cancelled() { Err(invalid("texture normalization cancelled")) } else { Ok(()) }
}
fn digest_bytes(bytes: &[u8], cancelled: &impl Fn() -> bool) -> Result<String> {
    let mut digest = Sha256::new();
    for chunk in bytes.chunks(64 * 1024) {
        check_cancel(cancelled)?;
        digest.update(chunk);
    }
    Ok(format!("{:x}", digest.finalize()))
}
fn destination_index(source: [u32; 2], output: [u32; 2], orientation: Orientation, x: u32, y: u32) -> Result<usize> {
    let [width, height] = source;
    if x >= width || y >= height || width == 0 || height == 0 {
        return Err(invalid("invalid orientation coordinate"));
    }
    let reverse_x = width - 1 - x;
    let reverse_y = height - 1 - y;
    let (dx, dy) = match orientation {
        Orientation::NoTransforms => (x, y),
        Orientation::Rotate90 => (reverse_y, x),
        Orientation::Rotate180 => (reverse_x, reverse_y),
        Orientation::Rotate270 => (y, reverse_x),
        Orientation::FlipHorizontal => (reverse_x, y),
        Orientation::FlipVertical => (x, reverse_y),
        Orientation::Rotate90FlipH => (y, x),
        Orientation::Rotate270FlipH => (reverse_y, reverse_x),
    };
    let [output_width, output_height] = output;
    if dx >= output_width || dy >= output_height {
        return Err(invalid("invalid oriented output dimensions"));
    }
    let index = u64::from(dy)
        .checked_mul(u64::from(output_width))
        .and_then(|value| value.checked_add(u64::from(dx)))
        .and_then(|value| value.checked_mul(4))
        .ok_or_else(|| invalid("orientation index overflow"))?;
    usize::try_from(index).map_err(|_| invalid("orientation index overflow"))
}

#[cfg(test)]
mod tests;
