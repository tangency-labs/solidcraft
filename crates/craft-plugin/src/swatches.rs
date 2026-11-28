//! Optional package artwork. Bad swatches produce warnings, never manifest errors.
use std::collections::{BTreeMap, BTreeSet};
use std::io::Read as _;
use std::path::Path;

use serde_json::Value;

pub const KEY: &str = "x-solidcraft-swatch";
pub const MAX_BYTES: usize = 64 * 1024;
const MAX_IMAGES: usize = 256;

/// The annotation for a larger picture of an option (a part, say) that the choice list shows as a
/// thumbnail and bigger for the highlighted option. Same package rules as a swatch, bigger limits.
pub const IMAGE_KEY: &str = "x-solidcraft-image";
/// The largest picture file, in bytes.
pub const IMAGE_MAX_BYTES: usize = 512 * 1024;
/// The largest picture, in pixels on each side, as decoded.
pub const IMAGE_MAX_SIDE: u32 = 1024;
/// Pictures are kept at most this many pixels on a side once decoded, to bound memory.
const IMAGE_KEPT_SIDE: u32 = 512;
/// The most decoded picture bytes one package keeps in total.
const IMAGE_TOTAL_BYTES: usize = 128 * 1024 * 1024;

/// What a kind of artwork may be.
struct Limits {
    bytes: usize,
    side: u32,
    kept_side: u32,
    alloc: u64,
}

const SWATCH_LIMITS: Limits = Limits { bytes: MAX_BYTES, side: 256, kept_side: 256, alloc: 4 * 1024 * 1024 };
const IMAGE_LIMITS: Limits = Limits { bytes: IMAGE_MAX_BYTES, side: IMAGE_MAX_SIDE, kept_side: IMAGE_KEPT_SIDE, alloc: 32 * 1024 * 1024 };

/// Decoded once at package load, independent of any UI or theme.
#[derive(Clone, Debug)]
pub struct Swatch {
    pub size: [usize; 2],
    pub rgba: Vec<u8>,
}

#[derive(Debug, Default)]
pub struct Report {
    /// Accepted `x-solidcraft-swatch` artwork, by package path.
    pub images: BTreeMap<String, Swatch>,
    /// Accepted `x-solidcraft-image` pictures, by package path. Kept apart from the swatches: one
    /// file may pass the picture limits and fail the swatch ones.
    pub pictures: BTreeMap<String, Swatch>,
    pub warnings: Vec<String>,
}

/// Only `oneOf` options of entry properties carry swatches. The reader receives checked paths;
/// archive callers look up bytes in their already checked package, folder callers use `read_file`.
pub fn check(entries: &[crate::Entry], mut read: impl FnMut(&str) -> Result<Vec<u8>, String>) -> Report {
    let mut report = Report::default();
    let mut seen = BTreeSet::new();
    let mut spent = 0usize;
    for entry in entries {
        for property in entry.input_schema.get("properties").and_then(Value::as_object).into_iter().flat_map(|p| p.values()) {
            for option in property.get("oneOf").and_then(Value::as_array).into_iter().flatten() {
                for (key, limits, what) in [(KEY, &SWATCH_LIMITS, "swatch"), (IMAGE_KEY, &IMAGE_LIMITS, "image")] {
                    let Some(annotation) = option.get(key) else { continue };
                    let Some(path) = annotation.as_str() else {
                        report.warnings.push(format!("{key}: expected a package PNG or JPEG path; {what} omitted"));
                        continue;
                    };
                    if !seen.insert((key, path)) {
                        continue;
                    }
                    if seen.len() > MAX_IMAGES {
                        report.warnings.push(format!("{key}: at most {MAX_IMAGES} distinct {what}s; remaining {what}s omitted"));
                        return report;
                    }
                    let image = format_for(path).and_then(|_| read(path)).and_then(|bytes| decode_limited(path, &bytes, limits));
                    match image {
                        Ok(image) => {
                            spent = spent.saturating_add(image.rgba.len());
                            if spent > IMAGE_TOTAL_BYTES {
                                report.warnings.push(format!("{key}: the {what}s are too large together; remaining {what}s omitted"));
                                return report;
                            }
                            let accepted = if key == IMAGE_KEY { &mut report.pictures } else { &mut report.images };
                            accepted.entry(path.to_owned()).or_insert(image);
                        }
                        Err(why) => report.warnings.push(format!("{key} {path}: {why}; {what} omitted")),
                    }
                }
            }
        }
    }
    report
}

fn format_for(path: &str) -> Result<image::ImageFormat, String> {
    if !(5..=128).contains(&path.len())
        || !path.split('/').all(|part| {
            !part.is_empty() && part != "." && part != ".." && part.bytes().all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'_' | b'-'))
        })
    {
        return Err("not a relative PNG or JPEG path inside the package".into());
    }
    match Path::new(path).extension().and_then(|e| e.to_str()) {
        Some("png") => Ok(image::ImageFormat::Png),
        Some("jpg" | "jpeg") => Ok(image::ImageFormat::Jpeg),
        _ => Err("only .png, .jpg and .jpeg files are allowed".into()),
    }
}

/// Bounded decoding of a swatch: neither a misleading suffix nor dimensions from hostile image
/// headers can enable another decoder or an unbounded allocation.
pub fn decode(path: &str, bytes: &[u8]) -> Result<Swatch, String> {
    decode_limited(path, bytes, &SWATCH_LIMITS)
}

/// The same bounded decoding for an `x-solidcraft-image` picture: up to 512 KiB and
/// 1024 × 1024 pixels, kept at most 512 pixels on a side.
pub fn decode_image(path: &str, bytes: &[u8]) -> Result<Swatch, String> {
    decode_limited(path, bytes, &IMAGE_LIMITS)
}

fn decode_limited(path: &str, bytes: &[u8], limits: &Limits) -> Result<Swatch, String> {
    let format = format_for(path)?;
    if bytes.len() > limits.bytes {
        return Err(format!("file is larger than {} KiB", limits.bytes / 1024));
    }
    if image::guess_format(bytes).map_err(|e| e.to_string())? != format {
        return Err("contents do not match the PNG or JPEG file type".into());
    }
    let side = limits.side;
    let mut reader = image::ImageReader::with_format(std::io::Cursor::new(bytes), format);
    let mut config = image::Limits::default();
    config.max_image_width = Some(side);
    config.max_image_height = Some(side);
    config.max_alloc = Some(limits.alloc);
    reader.limits(config);
    let mut pixels = reader.decode().map_err(|e| format!("invalid image (maximum {side} × {side}): {e}"))?.into_rgba8();
    let (width, height) = (pixels.width(), pixels.height());
    if width == 0 || height == 0 || width > side || height > side {
        return Err(format!("dimensions must be 1 to {side} pixels on each side"));
    }
    let longest = width.max(height);
    if longest > limits.kept_side {
        let scaled = |n: u32| ((u64::from(n) * u64::from(limits.kept_side)) / u64::from(longest)).max(1) as u32;
        pixels = image::imageops::resize(&pixels, scaled(width), scaled(height), image::imageops::FilterType::Triangle);
    }
    Ok(Swatch { size: [pixels.width() as usize, pixels.height() as usize], rgba: pixels.into_raw() })
}

/// No links or traversal, resolved inside the package, and a bounded read even if a file grows.
/// At most the swatch size.
pub fn read_file(dir: &Path, relative: &str) -> Result<Vec<u8>, String> {
    read_file_up_to(dir, relative, MAX_BYTES)
}

/// The same read for any package artwork: at most the picture size. [`check`] then applies the
/// limit of the annotation that named the file.
pub fn read_artwork(dir: &Path, relative: &str) -> Result<Vec<u8>, String> {
    read_file_up_to(dir, relative, IMAGE_MAX_BYTES)
}

fn read_file_up_to(dir: &Path, relative: &str, max: usize) -> Result<Vec<u8>, String> {
    format_for(relative)?;
    let root = dir.canonicalize().map_err(|e| e.to_string())?;
    let mut path = root.clone();
    for part in relative.split('/') {
        path.push(part);
        if std::fs::symlink_metadata(&path).map_err(|e| e.to_string())?.file_type().is_symlink() {
            return Err("a link is not a package picture".into());
        }
    }
    let resolved = path.canonicalize().map_err(|e| e.to_string())?;
    if !resolved.starts_with(&root) {
        return Err("path leaves the package".into());
    }
    let meta = std::fs::metadata(&resolved).map_err(|e| e.to_string())?;
    if !meta.is_file() || meta.len() > max as u64 {
        return Err(format!("not a file of at most {} KiB", max / 1024));
    }
    let file = std::fs::File::open(resolved).map_err(|e| e.to_string())?;
    let mut bytes = Vec::new();
    file.take(max as u64 + 1).read_to_end(&mut bytes).map_err(|e| e.to_string())?;
    if bytes.len() > max {
        return Err(format!("file is larger than {} KiB", max / 1024));
    }
    Ok(bytes)
}
