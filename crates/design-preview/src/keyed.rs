//! The saved picture, only when its key says it is this design's (docs/design-container.md,
//! "Portable thumbnail"). Windows File Explorer's thumbnail handler reads it
//! (apps/solidcraft-explorer).
//!
//! A save keeps `cache/thumbnail.png` and its key `cache/thumbnail.json`: `format`, `app`,
//! `document` and `payload`, the last two the lowercase hexadecimal SHA-256 of the container's
//! `document.json` and of the PNG. The app's own reader (crates/io/src/design/thumbnail.rs) uses a
//! picture only when both match, and so does this one: a file browser keeps a thumbnail until the
//! file changes, so a stale picture would outlive the edit that made it stale. Unlike the app, it
//! also takes format 1, which every save before 0.51 holds: the app draws a missing picture again,
//! and a file browser cannot.

use std::io::{Read, Seek};

use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::{Picture, open_archive, part, png_size};

/// A picture format a key may name: its tag, its size in pixels and its byte bound.
struct Format {
    tag: &'static str,
    size: (u32, u32),
    max_bytes: u64,
}

/// The formats SolidCraft writes or has written (crates/io/src/design/thumbnail.rs; format 1
/// until 0.50).
const FORMATS: &[Format] = &[
    Format { tag: "solidcraft-thumbnail/1", size: (240, 150), max_bytes: 256 << 10 },
    Format { tag: "solidcraft-thumbnail/2", size: (512, 384), max_bytes: 512 << 10 },
];
/// The key's bound, as the app reads it.
const MAX_KEY: u64 = 4 << 10;
/// `document.json`'s bound for a standalone preview (docs/design-container.md, "Seekable reader").
const MAX_DOCUMENT: u64 = 16 << 20;

#[derive(Deserialize)]
struct Key {
    format: String,
    document: String,
    payload: String,
}

/// The picture a save keeps in the design `source` holds, only when its key matches: the key's
/// format is one this reader knows, the PNG has that format's size and byte bound, and the key's
/// digests are those of the container's `document.json` and of the PNG. Reads from the start of
/// `source` whatever its position. Anything else is `None`.
pub fn keyed_picture<R: Read + Seek>(source: R) -> Option<Picture> {
    let mut archive = open_archive(source)?;
    let key: Key = serde_json::from_slice(&part(&mut archive, "cache/thumbnail.json", MAX_KEY)?).ok()?;
    let format = FORMATS.iter().find(|f| f.tag == key.format)?;
    let document = part(&mut archive, "document.json", MAX_DOCUMENT)?;
    if sha256_hex(&document) != key.document {
        return None;
    }
    let png = part(&mut archive, "cache/thumbnail.png", format.max_bytes)?;
    let (width, height) = png_size(&png)?;
    ((width, height) == format.size && sha256_hex(&png) == key.payload).then_some(Picture { png, width, height })
}

/// Lowercase hexadecimal SHA-256, as the writer keys it.
fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
#[path = "keyed_tests.rs"]
mod tests;
