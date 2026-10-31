//! A design and the designs it links, packed into one `.zip` to hand on (#3890 K12): the design
//! at the top level and each linked design under [`PACK_DESIGNS`]. The pack is built in memory,
//! one file added at a time, within [`MAX_PACK_BYTES`] and [`MAX_PACK_ENTRIES`].
//!
//! Entry names are made here from file names, never taken whole from a link: a control
//! character or one of `/ \ : < > " | ? *` becomes `_`, no name ends in `.` or a space, a name
//! Windows keeps for a device gets `_` after its stem, and a name another entry has, case aside,
//! is numbered (`plate.sc`, `plate-2.sc`).

use std::collections::BTreeSet;
use std::io::{Cursor, Write};

use super::is_design_container;
use crate::{IoError, Result};

/// A pack's file extension.
pub const PACK_EXT: &str = "zip";
/// The folder the linked designs go in.
pub const PACK_DESIGNS: &str = "designs";
/// The most a pack holds in all: it is built in memory, and read back the same way.
pub const MAX_PACK_BYTES: u64 = 1 << 30;
/// The most other designs a pack carries: the files a design's links name and, once drawings
/// name files of their own, its drawings' sources, counted together.
pub const MAX_PACK_DESIGNS: usize = 256;
/// The most files a pack holds: the design and the designs it names.
pub const MAX_PACK_ENTRIES: usize = 1 + MAX_PACK_DESIGNS;

/// Whether `path` names a pack (`.zip`, any case).
pub fn is_pack_path(path: &str) -> bool {
    std::path::Path::new(path).extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case(PACK_EXT))
}

/// Refuse files that come to more than a pack holds.
pub fn fits_pack(bytes: u64) -> Result<()> {
    if bytes > MAX_PACK_BYTES {
        return Err(IoError::Invalid(format!(
            "the files come to {} MiB; a pack holds at most {} MiB",
            bytes.div_ceil(1 << 20),
            MAX_PACK_BYTES >> 20
        )));
    }
    Ok(())
}

/// A pack being written. Each file goes in as it is added, so only the pack and one file are
/// held at once.
pub struct PackWriter {
    zip: zip::ZipWriter<Cursor<Vec<u8>>>,
    /// Each entry's name in lower case, to keep names apart on systems that ignore case.
    names: BTreeSet<String>,
    bytes: u64,
}

impl Default for PackWriter {
    fn default() -> PackWriter {
        PackWriter { zip: zip::ZipWriter::new(Cursor::new(Vec::new())), names: BTreeSet::new(), bytes: 0 }
    }
}

impl PackWriter {
    /// `bytes` as the file named `name` under `folder` (`None`: the top level): the entry's name.
    /// A design container is stored as it is (it is compressed already); anything else deflated.
    pub fn add(&mut self, folder: Option<&str>, name: &str, bytes: &[u8]) -> Result<String> {
        if self.names.len() >= MAX_PACK_ENTRIES {
            return Err(IoError::Invalid(format!("a pack holds at most {MAX_PACK_ENTRIES} files")));
        }
        self.bytes = self.bytes.saturating_add(bytes.len() as u64);
        fits_pack(self.bytes)?;
        let entry = self.unique(folder, &entry_name(name))?;
        let method = if is_design_container(bytes) { zip::CompressionMethod::Stored } else { zip::CompressionMethod::Deflated };
        let invalid = |e: zip::result::ZipError| IoError::Invalid(format!("pack: {e}"));
        self.zip.start_file(entry.as_str(), zip::write::SimpleFileOptions::default().compression_method(method)).map_err(invalid)?;
        self.zip.write_all(bytes).map_err(|e| IoError::Invalid(format!("pack: {e}")))?;
        Ok(entry)
    }

    /// The pack's bytes.
    pub fn finish(self) -> Result<Vec<u8>> {
        Ok(self.zip.finish().map_err(|e| IoError::Invalid(format!("pack: {e}")))?.into_inner())
    }

    /// `name` under `folder`, numbered when another entry has it, case aside.
    fn unique(&mut self, folder: Option<&str>, name: &str) -> Result<String> {
        let (stem, ext) = match name.rsplit_once('.') {
            Some((stem, ext)) if !stem.is_empty() => (stem, format!(".{ext}")),
            _ => (name, String::new()),
        };
        let candidates = std::iter::once(name.to_string()).chain((2..=MAX_PACK_ENTRIES).map(|n| format!("{stem}-{n}{ext}")));
        for candidate in candidates {
            let entry = folder.map_or_else(|| candidate.clone(), |folder| format!("{folder}/{candidate}"));
            if self.names.insert(entry.to_lowercase()) {
                return Ok(entry);
            }
        }
        Err(IoError::Invalid(format!("a pack holds at most {MAX_PACK_ENTRIES} files")))
    }
}

/// `name` as an entry may have it (the module's notes), before numbering.
fn entry_name(name: &str) -> String {
    let kept: String =
        name.chars().map(|c| if c.is_control() || matches!(c, '/' | '\\' | ':' | '<' | '>' | '"' | '|' | '?' | '*') { '_' } else { c }).collect();
    let kept = kept.trim_end_matches([' ', '.']);
    let kept = if kept.is_empty() { "file" } else { kept };
    if crate::vfs::remote(kept) != Some(crate::vfs::Remote::Device) {
        return kept.to_string();
    }
    match kept.split_once('.') {
        Some((stem, rest)) => format!("{stem}_.{rest}"),
        None => format!("{kept}_"),
    }
}

#[cfg(test)]
#[path = "pack_tests.rs"]
mod tests;
