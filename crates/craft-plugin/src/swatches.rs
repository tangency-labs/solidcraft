//! Optional package artwork. Bad swatches produce warnings, never manifest errors.
use std::collections::{BTreeMap, BTreeSet};
use std::io::Read as _;
use std::path::Path;

use serde_json::Value;

pub const KEY: &str = "x-solidcraft-swatch";
pub const MAX_BYTES: usize = 64 * 1024;
const MAX_IMAGES: usize = 256;

/// Decoded once at package load, independent of any UI or theme.
#[derive(Clone, Debug)]
pub struct Swatch {
    pub size: [usize; 2],
    pub rgba: Vec<u8>,
}

#[derive(Debug, Default)]
pub struct Report {
    pub images: BTreeMap<String, Swatch>,
    pub warnings: Vec<String>,
}

/// Only `oneOf` options of entry properties carry swatches. The reader receives checked paths;
/// archive callers look up bytes in their already checked package, folder callers use `read_file`.
pub fn check(entries: &[crate::Entry], mut read: impl FnMut(&str) -> Result<Vec<u8>, String>) -> Report {
    let mut report = Report::default();
    let mut seen = BTreeSet::new();
    for entry in entries {
        for property in entry.input_schema.get("properties").and_then(Value::as_object).into_iter().flat_map(|p| p.values()) {
            for option in property.get("oneOf").and_then(Value::as_array).into_iter().flatten() {
                let Some(annotation) = option.get(KEY) else { continue };
                let Some(path) = annotation.as_str() else {
                    report.warnings.push(format!("{KEY}: expected a package PNG or JPEG path; swatch omitted"));
                    continue;
                };
                if !seen.insert(path) {
                    continue;
                }
                if seen.len() > MAX_IMAGES {
                    report.warnings.push(format!("{KEY}: at most {MAX_IMAGES} distinct swatches; remaining swatches omitted"));
                    return report;
                }
                let image = format_for(path).and_then(|_| read(path)).and_then(|bytes| decode(path, &bytes));
                match image {
                    Ok(image) => {
                        report.images.insert(path.to_owned(), image);
                    }
                    Err(why) => report.warnings.push(format!("{KEY} {path}: {why}; swatch omitted")),
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

/// Bounded decoding: neither a misleading suffix nor dimensions from hostile image headers
/// can enable another decoder or an unbounded allocation.
pub fn decode(path: &str, bytes: &[u8]) -> Result<Swatch, String> {
    let format = format_for(path)?;
    if bytes.len() > MAX_BYTES {
        return Err("file is larger than 64 KiB".into());
    }
    if image::guess_format(bytes).map_err(|e| e.to_string())? != format {
        return Err("contents do not match the PNG or JPEG file type".into());
    }
    let mut reader = image::ImageReader::with_format(std::io::Cursor::new(bytes), format);
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(256);
    limits.max_image_height = Some(256);
    limits.max_alloc = Some(4 * 1024 * 1024);
    reader.limits(limits);
    let pixels = reader.decode().map_err(|e| format!("invalid image (maximum 256 × 256): {e}"))?.into_rgba8();
    if pixels.width() == 0 || pixels.height() == 0 || pixels.width() > 256 || pixels.height() > 256 {
        return Err("dimensions must be 1 to 256 pixels on each side".into());
    }
    Ok(Swatch { size: [pixels.width() as usize, pixels.height() as usize], rgba: pixels.into_raw() })
}

/// No links or traversal, resolved inside the package, and a bounded read even if a file grows.
pub fn read_file(dir: &Path, relative: &str) -> Result<Vec<u8>, String> {
    format_for(relative)?;
    let root = dir.canonicalize().map_err(|e| e.to_string())?;
    let mut path = root.clone();
    for part in relative.split('/') {
        path.push(part);
        if std::fs::symlink_metadata(&path).map_err(|e| e.to_string())?.file_type().is_symlink() {
            return Err("a link is not a package swatch".into());
        }
    }
    let resolved = path.canonicalize().map_err(|e| e.to_string())?;
    if !resolved.starts_with(&root) {
        return Err("path leaves the package".into());
    }
    let meta = std::fs::metadata(&resolved).map_err(|e| e.to_string())?;
    if !meta.is_file() || meta.len() > MAX_BYTES as u64 {
        return Err("not a file of at most 64 KiB".into());
    }
    let file = std::fs::File::open(resolved).map_err(|e| e.to_string())?;
    let mut bytes = Vec::new();
    file.take(MAX_BYTES as u64 + 1).read_to_end(&mut bytes).map_err(|e| e.to_string())?;
    if bytes.len() > MAX_BYTES {
        return Err("file is larger than 64 KiB".into());
    }
    Ok(bytes)
}
