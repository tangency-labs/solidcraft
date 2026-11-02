//! Bounded texture admission, independent of documents, files, schedulers and GPU handles.
//! This utility does not advertise texture/PBR support or define a persistent pack schema.
use image::{ImageDecoder, ImageFormat, ImageReader, Limits};
use serde::{Deserialize, Serialize};
use std::{io::Cursor, sync::Arc};

pub mod normalization;

pub const MAX_TEXTURE_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_TEXTURE_EDGE: u32 = 2048;
const MAX_CODEC_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MapRole {
    BaseColor,
    NormalOpenGl,
    Roughness,
    Metallic,
    AmbientOcclusion,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Interpretation {
    Srgb,
    Data,
}
impl MapRole {
    pub fn interpretation(self) -> Interpretation {
        if self == Self::BaseColor { Interpretation::Srgb } else { Interpretation::Data }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextureError(pub String);
impl std::fmt::Display for TextureError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for TextureError {}
type Result<T> = std::result::Result<T, TextureError>;
fn invalid(message: impl Into<String>) -> TextureError {
    TextureError(message.into())
}

#[derive(Clone, Debug, Serialize)]
pub struct TextureInfo {
    pub width: u32,
    pub height: u32,
    pub encoded_bytes: usize,
    pub rgba_bytes: u64,
    pub role: MapRole,
    pub interpretation: Interpretation,
}

/// Header-admitted source. Full decoding must succeed before publishing pixels.
#[derive(Clone)]
pub struct TextureSource {
    bytes: Arc<Vec<u8>>,
    format: ImageFormat,
    info: TextureInfo,
}
fn reader(bytes: &[u8], format: ImageFormat) -> ImageReader<Cursor<&[u8]>> {
    let mut reader = ImageReader::with_format(Cursor::new(bytes), format);
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_TEXTURE_EDGE);
    limits.max_image_height = Some(MAX_TEXTURE_EDGE);
    limits.max_alloc = Some(MAX_CODEC_BYTES);
    reader.limits(limits);
    reader
}
fn guarded<T>(work: impl FnOnce() -> Result<T>) -> Result<T> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(work)).map_err(|_| invalid("texture decoder failed"))?
}
fn admitted_format(bytes: &[u8], capacity: usize) -> Result<ImageFormat> {
    if bytes.is_empty() || bytes.len() > MAX_TEXTURE_BYTES || capacity > MAX_TEXTURE_BYTES {
        return Err(invalid("texture input must contain at most 16 MiB"));
    }
    let format = image::guess_format(bytes).map_err(|_| invalid("texture must be PNG or JPEG"))?;
    if !matches!(format, ImageFormat::Png | ImageFormat::Jpeg) {
        return Err(invalid("texture must be PNG or JPEG; HDR uses the Radiance input utility"));
    }
    if !is_whole(bytes, format) {
        return Err(invalid("texture is missing its final terminator"));
    }
    Ok(format)
}
/// Whether a PNG or JPEG ends with its final marker. Whole-image codecs may return pixels before
/// noticing a missing trailer; this rejects that truncation. It is not a general container check.
pub(crate) fn is_whole(bytes: &[u8], format: ImageFormat) -> bool {
    match format {
        ImageFormat::Png => bytes.ends_with(b"\x00\x00\x00\x00IEND\xae\x42\x60\x82"),
        ImageFormat::Jpeg => bytes.ends_with(b"\xff\xd9"),
        _ => false,
    }
}
impl TextureSource {
    #[cfg(test)]
    pub(crate) fn shares_encoded_storage(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.bytes, &other.bytes)
    }
    pub fn new(bytes: Vec<u8>, role: MapRole) -> Result<Self> {
        let format = admitted_format(&bytes, bytes.capacity())?;
        let (width, height) = guarded(|| {
            let mut decoder = reader(&bytes, format).into_decoder().map_err(|e| invalid(e.to_string()))?;
            if !matches!(decoder.color_type(), image::ColorType::L8 | image::ColorType::La8 | image::ColorType::Rgb8 | image::ColorType::Rgba8) {
                return Err(invalid("only 8-bit texture channels are supported"));
            }
            if decoder.icc_profile().map_err(|e| invalid(e.to_string()))?.is_some() {
                return Err(invalid("embedded ICC profiles need explicit color conversion before admission"));
            }
            if decoder.orientation().map_err(|e| invalid(e.to_string()))? != image::metadata::Orientation::NoTransforms {
                return Err(invalid("texture orientation must be normalized before admission"));
            }
            Ok(decoder.dimensions())
        })?;
        if width == 0 || height == 0 || width > MAX_TEXTURE_EDGE || height > MAX_TEXTURE_EDGE {
            return Err(invalid("texture dimensions exceed 2048x2048"));
        }
        let rgba_bytes = u64::from(width) * u64::from(height) * 4;
        let info = TextureInfo { width, height, encoded_bytes: bytes.len(), rgba_bytes, role, interpretation: role.interpretation() };
        Ok(Self { bytes: Arc::new(bytes), format, info })
    }
    pub fn info(&self) -> &TextureInfo {
        &self.info
    }
    /// Bounded whole-image codec work, intended for a caller-owned worker. Cancellation is
    /// checked at the codec boundary, not during PNG/JPEG decoding. No partial image escapes.
    pub fn decode(&self, cancelled: impl Fn() -> bool) -> Result<DecodedTexture> {
        if cancelled() {
            return Err(invalid("texture decoding cancelled"));
        }
        let image = guarded(|| reader(&self.bytes, self.format).decode().map_err(|e| invalid(e.to_string())))?;
        if cancelled() {
            return Err(invalid("texture decoding cancelled"));
        }
        let pixels = guarded(|| Ok(image.into_rgba8().into_raw()))?;
        if cancelled() {
            return Err(invalid("texture decoding cancelled"));
        }
        if pixels.len() as u64 != self.info.rgba_bytes {
            return Err(invalid("decoded texture dimensions changed"));
        }
        Ok(DecodedTexture { info: self.info.clone(), pixels })
    }
}
/// Completed pixels retain encoded channel values. sRGB conversion, normal basis handling
/// and sampling belong to the eventual renderer adapter, not this input utility.
pub struct DecodedTexture {
    info: TextureInfo,
    pixels: Vec<u8>,
}
impl DecodedTexture {
    pub fn info(&self) -> &TextureInfo {
        &self.info
    }
    pub fn rgba(&self) -> &[u8] {
        &self.pixels
    }
}

#[cfg(test)]
mod tests;
