//! A saved design's picture and 3D display meshes, read for file browsers without the kernel.
//!
//! Finder icons and Quick Look (apps/solidcraft-quicklook) show a `.sc` design without opening it:
//! they read the picture a desktop save keeps at `cache/thumbnail.png` ([`picture`]) and the
//! display meshes a save may keep at `cache/preview.bin` ([`model`]). Nothing here evaluates or
//! tessellates the model, and nothing depends on the kernel, so an extension that links this
//! crate stays small and quick (docs/design-container.md, "Portable thumbnail" and "Provisional
//! saved display").
//!
//! Windows File Explorer's thumbnail handler (apps/solidcraft-explorer) reads the picture with
//! [`keyed_picture`], which also checks the picture's key against the design, as the app does.
//!
//! Input is hostile: a file browser hands over whatever file has the extension. Every read is
//! bounded, and any problem (not a container, a missing or oversized part, a malformed picture or
//! mesh) is `None`: the file browser then shows its generic icon or the picture alone.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use serde::Deserialize;

mod keyed;
mod model;

pub use keyed::keyed_picture;
pub use model::{Grab, Matrices, Model, Orbit, Pan, Vertex, model};

/// The container formats this reader knows (docs/design-container.md; the second is the
/// pre-rename draft, docs/compatibility.md).
const CONTAINERS: &[&str] = &["solidcraft-container/1", "solvecraft-container/1"];
/// The largest design file looked at, as the app's own cap on a design (`io::max_design`).
const MAX_FILE: u64 = 2 << 30;
/// More entries than any design the app writes (docs/design-container.md, "Bounds").
const MAX_ENTRIES: usize = 10_000;
/// The ZIP directory's bound: 10,000 entries with names far longer than a design's.
const MAX_DIRECTORY: u64 = 8 << 20;
const MAX_CONTAINER_JSON: u64 = 4 << 10;
/// The saved picture's own bound is 256 KiB; a later, larger picture still fits.
const MAX_PICTURE: u64 = 8 << 20;
/// A picture wider or taller than this is not one SolidCraft wrote.
const MAX_PICTURE_SIDE: u32 = 4096;

/// The saved picture: PNG bytes and their size in pixels.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Picture {
    pub png: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

/// The picture a desktop save keeps in the design at `path`, if it has a valid one.
pub fn picture(path: &Path) -> Option<Picture> {
    let mut archive = open(path)?;
    let png = part(&mut archive, "cache/thumbnail.png", MAX_PICTURE)?;
    let (width, height) = png_size(&png)?;
    Some(Picture { png, width, height })
}

/// The width and height a PNG's header gives, if the bytes start as a PNG SolidCraft could write.
pub fn png_size(png: &[u8]) -> Option<(u32, u32)> {
    if png.get(..8)? != b"\x89PNG\r\n\x1a\n" || png.get(12..16)? != b"IHDR" {
        return None;
    }
    let side = |at: usize| png.get(at..at + 4).and_then(|b| b.try_into().ok()).map(u32::from_be_bytes);
    let (width, height) = (side(16)?, side(20)?);
    let fits = |s: u32| (1..=MAX_PICTURE_SIDE).contains(&s);
    (fits(width) && fits(height)).then_some((width, height))
}

impl Picture {
    /// The largest size with the picture's proportions that fits in `width` x `height` (a file
    /// browser's icon or card, in any unit), or `None` when that box is empty.
    pub fn fit(&self, width: f64, height: f64) -> Option<(f64, f64)> {
        let usable = |side: f64| side.is_finite() && side > 0.0;
        if !usable(width) || !usable(height) {
            return None;
        }
        let (w, h) = (f64::from(self.width), f64::from(self.height));
        let scale = (width / w).min(height / h);
        Some((w * scale, h * scale))
    }
}

/// The ZIP container at `path`, after its size and what [`open_archive`] checks.
fn open(path: &Path) -> Option<zip::ZipArchive<File>> {
    let file = File::open(path).ok()?;
    let meta = file.metadata().ok()?;
    if !meta.is_file() || meta.len() > MAX_FILE {
        return None;
    }
    open_archive(file)
}

/// The ZIP container `source` holds, read from its start whatever its position, after its size,
/// its end-of-directory record, its entry count and `container.json` are checked.
fn open_archive<R: Read + Seek>(mut source: R) -> Option<zip::ZipArchive<R>> {
    let len = source.seek(SeekFrom::End(0)).ok()?;
    if len > MAX_FILE {
        return None;
    }
    directory_fits(&mut source, len)?;
    source.seek(SeekFrom::Start(0)).ok()?;
    let mut archive = zip::ZipArchive::new(source).ok()?;
    if archive.len() > MAX_ENTRIES {
        return None;
    }
    let container: Container = serde_json::from_slice(&part(&mut archive, "container.json", MAX_CONTAINER_JSON)?).ok()?;
    CONTAINERS.contains(&container.format.as_str()).then_some(archive)
}

/// `Some` when the end-of-central-directory record that ends `source` (`len` bytes long)
/// describes a directory the `zip` crate may parse: one disk, the same entry count on it and in
/// all, at most [`MAX_ENTRIES`] entries and [`MAX_DIRECTORY`] bytes, inside the file before the
/// record, and no ZIP64 (a container never uses it). Checked before the `zip` crate reads the
/// directory, so a hostile one cannot make it allocate or loop beyond these bounds.
fn directory_fits<R: Read + Seek>(source: &mut R, len: u64) -> Option<()> {
    let _ = (source, len);
    Some(())
}

/// One entry's bytes, read in full only when its declared and actual sizes are within `limit`.
fn part<R: Read + Seek>(archive: &mut zip::ZipArchive<R>, name: &str, limit: u64) -> Option<Vec<u8>> {
    let entry = archive.by_name(name).ok()?;
    if entry.size() > limit {
        return None;
    }
    let mut bytes = Vec::with_capacity(usize::try_from(entry.size()).ok()?);
    // One byte past the limit tells a lying header from an honest one.
    entry.take(limit + 1).read_to_end(&mut bytes).ok()?;
    (bytes.len() as u64 <= limit).then_some(bytes)
}

#[derive(Deserialize)]
struct Container {
    format: String,
}

#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;
