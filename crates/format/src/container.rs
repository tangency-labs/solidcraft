//! The file container: a ZIP package of named parts (the file's JSON, binary payloads, optional
//! caches), never extracted to filesystem paths. Payloads are content-addressed blobs,
//! `blobs/<sha256>.<extension>`, stored once however often they are referred to. Every size is
//! bounded by the app's [`Limits`]: the file, each part, all parts expanded, the payloads resolved,
//! and the number of entries; a part is read no further than its declared size (a decompression
//! bomb stops there). What a part holds, and how a payload is encoded, is the app's.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{Cursor, Read, Seek, SeekFrom, Write};
use std::sync::Arc;

use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::FormatError;

/// How hard the payloads are compressed: Fast favours interactive file operations; Compact trades
/// CPU time for space.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Compression {
    #[default]
    Fast,
    Compact,
}

/// An app's budgets for its containers. The functions are read each time they are needed, so a
/// limit the app lets the user raise takes effect at once.
#[derive(Clone, Copy)]
pub struct Limits {
    /// The most entries a container holds.
    pub max_entries: usize,
    /// The most bytes all parts may expand to, and all payloads may resolve to.
    pub max_expanded: fn() -> usize,
    /// The most bytes a part of this name may expand to.
    pub part_limit: fn(&str) -> usize,
    /// The Deflate level of a compressed part of this name and size.
    pub level: fn(&str, usize) -> i64,
    /// Whether a container of this many bytes may be read, or why not.
    pub check_size: fn(u64) -> Result<(), String>,
    /// Whether the work is cancelled (a save the person stopped): a long part is written a piece
    /// at a time and stops before the next piece when this says so. `None`: it cannot be
    /// cancelled here.
    pub cancelled: Option<fn() -> bool>,
}

fn invalid(message: impl std::fmt::Display) -> FormatError {
    FormatError::Invalid(message.to_string())
}

/// Whether the bytes start as a container does, whatever the file is called.
pub fn is_container(bytes: &[u8]) -> bool {
    bytes.starts_with(b"PK\x03\x04")
}

/// The SHA-256 of the bytes.
pub fn sha256(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

fn hex(digest: &[u8; 32]) -> String {
    use std::fmt::Write as _;
    let mut out = String::with_capacity(64);
    for byte in digest {
        // Writing to a `String` cannot fail.
        let _ = write!(&mut out, "{byte:02x}");
    }
    out
}

/// The SHA-256 of the bytes, in lowercase hexadecimal: a blob's name, and how a cache is keyed to
/// the file it was written for.
pub fn digest(bytes: &[u8]) -> String {
    hex(&sha256(bytes))
}

/// One part a [`Writer`] wrote, with the SHA-256 of its uncompressed content. A caller that signs
/// a container uses these to bind the parts without reading the file back.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PartDigest {
    pub name: String,
    pub len: u64,
    pub digest: [u8; 32],
}

/// A container being written.
pub struct Writer {
    zip: zip::ZipWriter<Cursor<Vec<u8>>>,
    names: BTreeSet<String>,
    expanded: usize,
    resolved: usize,
    limits: Limits,
    /// Which written parts to record for a signature, and the records so far.
    tracked: Option<(fn(&str) -> bool, Vec<PartDigest>)>,
}

impl Writer {
    pub fn new(limits: Limits) -> Writer {
        Writer { zip: zip::ZipWriter::new(Cursor::new(Vec::new())), names: BTreeSet::new(), expanded: 0, resolved: 0, limits, tracked: None }
    }

    /// Record the parts written from now on whose name `include` accepts, with their SHA-256, for
    /// [`Writer::take_parts`]. Used by a caller that signs the container.
    pub fn track_parts(&mut self, include: fn(&str) -> bool) {
        self.tracked = Some((include, Vec::new()));
    }

    /// The tracked parts written so far, oldest first, and stop tracking.
    pub fn take_parts(&mut self) -> Option<Vec<PartDigest>> {
        self.tracked.take().map(|(_, parts)| parts)
    }

    /// The container's bytes.
    pub fn finish(self) -> Result<Vec<u8>, FormatError> {
        Ok(self.zip.finish().map_err(invalid)?.into_inner())
    }

    /// Add a part, compressed or stored. A name already written is written once.
    pub fn part(&mut self, name: &str, bytes: &[u8], compressed: bool) -> Result<(), FormatError> {
        self.part_with_digest(name, bytes, compressed, None)
    }

    /// [`Writer::part`] with a digest the caller already worked out (a blob's name is its digest),
    /// so a signed save does not hash the same bytes twice.
    fn part_with_digest(&mut self, name: &str, bytes: &[u8], compressed: bool, digest: Option<[u8; 32]>) -> Result<(), FormatError> {
        if bytes.len() > (self.limits.part_limit)(name) {
            return Err(invalid("part too large"));
        }
        if !self.names.insert(name.to_owned()) {
            return Ok(());
        }
        self.expanded = self.expanded.checked_add(bytes.len()).ok_or_else(|| invalid("expanded size overflow"))?;
        if self.expanded > (self.limits.max_expanded)() || self.names.len() > self.limits.max_entries {
            return Err(invalid("package too large"));
        }
        if let Some((include, parts)) = &mut self.tracked
            && include(name)
        {
            let digest = digest.unwrap_or_else(|| sha256(bytes));
            parts.push(PartDigest { name: name.to_owned(), len: bytes.len() as u64, digest });
        }
        let method = if compressed { zip::CompressionMethod::Deflated } else { zip::CompressionMethod::Stored };
        let level = (self.limits.level)(name, bytes.len());
        // A part of 4 GiB or more (only with a raised file limit) needs a ZIP64 entry.
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(method)
            .compression_level(compressed.then_some(level))
            .large_file(bytes.len() as u64 >= u64::from(u32::MAX));
        self.zip.start_file(name, options).map_err(invalid)?;
        // A piece at a time: a long compression (a large mesh or model cache) stops on Cancel
        // within a piece instead of after the whole part.
        for piece in bytes.chunks(1 << 20) {
            if self.limits.cancelled.is_some_and(|cancelled| cancelled()) {
                return Err(FormatError::Cancelled);
            }
            self.zip.write_all(piece).map_err(invalid)?;
        }
        Ok(())
    }

    /// Has a part of this name been written?
    pub fn has_part(&self, name: &str) -> bool {
        self.names.contains(name)
    }

    /// Would `entries` more optional parts of `extra` bytes, stored uncompressed, still keep the
    /// file within `budget` and its expanded size within the reader's budget?
    pub fn room(&self, entries: usize, extra: usize, budget: usize) -> bool {
        self.names.len().saturating_add(entries) <= self.limits.max_entries && extra <= self.space(entries, budget)
    }

    /// The bytes `entries` more optional parts may hold together ([`Writer::room`]).
    pub fn space(&self, entries: usize, budget: usize) -> usize {
        if self.names.len().saturating_add(entries) > self.limits.max_entries {
            return 0;
        }
        // The bytes written so far, plus what the open compressed part may still hold back.
        let written = self.zip.get_ref().map_or(usize::MAX, |c| c.get_ref().len()).saturating_add(1 << 20);
        let directory = self.names.len().saturating_add(entries).saturating_mul(256).saturating_add(65_536);
        budget.saturating_sub(written.saturating_add(directory)).min((self.limits.max_expanded)().saturating_sub(self.expanded))
    }

    /// Add a payload as a blob, `blobs/<sha256>.<extension>`, and say its name, for the JSON that
    /// refers to it.
    pub fn blob(&mut self, bytes: &[u8], extension: &str, compressed: bool) -> Result<Value, FormatError> {
        self.resolved = self
            .resolved
            .checked_add(bytes.len())
            .filter(|&n| n <= (self.limits.max_expanded)())
            .ok_or_else(|| invalid("resolved payloads too large"))?;
        let digest = sha256(bytes);
        let name = format!("blobs/{}.{extension}", hex(&digest));
        self.part_with_digest(&name, bytes, compressed, Some(digest))?;
        Ok(Value::String(name))
    }
}

/// A container being read, over a seekable source. Opening checks the directory (the entry count,
/// unique and plain names, every part's declared size), not the parts.
pub struct Reader<R: Read + Seek> {
    zip: zip::ZipArchive<R>,
    blobs: BTreeMap<String, Vec<u8>>,
    /// The bytes of payloads resolved so far, repeats counted: bounded by the expanded budget.
    pub hydrated: usize,
    document: Option<Arc<[u8]>>,
    limits: Limits,
}

impl<R: Read + Seek> Reader<R> {
    pub fn new(mut source: R, limits: Limits) -> Result<Self, FormatError> {
        let length = source.seek(SeekFrom::End(0)).map_err(invalid)?;
        (limits.check_size)(length).map_err(invalid)?;
        let tail_length = length.min(65_557) as usize;
        source.seek(SeekFrom::Start(length - tail_length as u64)).map_err(invalid)?;
        let mut tail = vec![0; tail_length];
        source.read_exact(&mut tail).map_err(invalid)?;
        // zip's name index can coalesce duplicate central-directory entries. The bounded format
        // does not need ZIP64/multiple disks: verify the declared entry count as well so
        // duplicate names cannot silently select a payload.
        let eocd = tail.windows(4).rposition(|v| v == b"PK\x05\x06").ok_or_else(|| invalid("missing ZIP directory"))?;
        let end = tail.get(eocd..).filter(|v| v.len() >= 22).ok_or_else(|| invalid("truncated ZIP directory"))?;
        let u16_at = |offset| -> Result<u16, FormatError> {
            let array: [u8; 2] = end.get(offset..offset + 2).ok_or_else(|| invalid("truncated ZIP directory"))?.try_into().map_err(invalid)?;
            Ok(u16::from_le_bytes(array))
        };
        let declared = u16_at(10)? as usize;
        if declared > limits.max_entries
            || u16_at(4)? != 0
            || u16_at(6)? != 0
            || u16_at(8)? as usize != declared
            || end.len() != 22 + u16_at(20)? as usize
        {
            return Err(invalid("too many entries or unsupported ZIP directory"));
        }
        // A container is a ZIP with nothing in front of it. Saying so skips zip's search for a
        // prefix, which reads back through the stored parts and would load a large payload
        // before a preview can be shown.
        let config = zip::read::Config { archive_offset: zip::read::ArchiveOffset::Known(0) };
        let mut zip = zip::ZipArchive::with_config(config, source).map_err(invalid)?;
        if u16_at(4)? != 0
            || u16_at(6)? != 0
            || u16_at(8)? as usize != zip.len()
            || u16_at(10)? as usize != zip.len()
            || end.len() != 22 + u16_at(20)? as usize
        {
            return Err(invalid("duplicate entries or unsupported ZIP directory"));
        }
        let mut names = BTreeSet::new();
        let mut total = 0u64;
        for i in 0..zip.len() {
            let part = zip.by_index_raw(i).map_err(invalid)?;
            let name = part.name().map_err(invalid)?.into_owned();
            if name.is_empty()
                || name.contains('\\')
                || name.split('/').any(|c| c.is_empty() || c == "." || c == "..")
                || !names.insert(name.to_owned())
            {
                return Err(invalid("duplicate or invalid archive entry"));
            }
            total = total.checked_add(part.size()).ok_or_else(|| invalid("expanded size overflow"))?;
            let limit = (limits.part_limit)(&name);
            if part.size() > limit as u64 || total > (limits.max_expanded)() as u64 {
                return Err(invalid("expanded package too large"));
            }
        }
        Ok(Self { zip, blobs: BTreeMap::new(), hydrated: 0, document: None, limits })
    }

    /// The file's own part, `document.json`, read once and kept: provisional caches and the
    /// authoritative reading are bound to the same bytes, even if another process overwrites the
    /// open file in place. Blobs remain hash-checked.
    pub fn document(&mut self, limit: usize) -> Result<Arc<[u8]>, FormatError> {
        if self.document.is_none() {
            self.document = Some(self.limited_part("document.json", limit)?.into());
        }
        self.document.as_ref().filter(|d| d.len() <= limit).cloned().ok_or_else(|| invalid("metadata budget"))
    }

    /// Whether the container has a part of this name.
    pub fn has_part(&self, name: &str) -> bool {
        self.zip.index_for_name(name).is_some()
    }

    /// The names of every part, in the container's order.
    pub fn part_names(&self) -> Vec<String> {
        self.zip.file_names().filter_map(|name| name.ok().map(|name| name.into_owned())).collect()
    }

    /// The part of this name, at most `limit` bytes.
    pub fn limited_part(&mut self, name: &str, limit: usize) -> Result<Vec<u8>, FormatError> {
        let part = self.zip.by_name(name).map_err(|e| invalid(format!("required part `{name}`: {e}")))?;
        let expected = part.size();
        if expected > limit as u64 {
            return Err(invalid("part too large"));
        }
        let mut bytes = Vec::new();
        bytes.try_reserve_exact(expected as usize).map_err(|_| invalid("could not reserve required part"))?;
        // A compressed part may expand beyond its declared size (a decompression bomb): stop
        // one byte past it instead of at the limit.
        part.take(expected + 1).read_to_end(&mut bytes).map_err(invalid)?;
        if bytes.len() > limit || bytes.len() as u64 != expected {
            return Err(invalid("part too large or length mismatch"));
        }
        Ok(bytes)
    }

    /// The blob `value`'s `field` names, of this extension, within its part limit.
    pub fn blob(&mut self, value: &Value, field: &str, extension: &str) -> Result<&[u8], FormatError> {
        let limit = (self.limits.part_limit)(&format!("blob.{extension}"));
        self.limited_blob(value, field, extension, limit)
    }

    /// [`Reader::blob`] within `limit` bytes. Its hash is checked when it is first read, and every
    /// reference counts against the resolved budget.
    pub fn limited_blob(&mut self, value: &Value, field: &str, extension: &str, limit: usize) -> Result<&[u8], FormatError> {
        let name = value.get(field).and_then(Value::as_str).ok_or_else(|| invalid(format!("missing `{field}` reference")))?;
        let hash = name
            .strip_prefix("blobs/")
            .and_then(|n| n.strip_suffix(&format!(".{extension}")))
            .filter(|h| h.len() == 64 && h.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)))
            .ok_or_else(|| invalid("invalid blob reference"))?;
        if !self.blobs.contains_key(name) {
            let bytes = self.limited_part(name, limit)?;
            if digest(&bytes) != hash {
                return Err(invalid(format!("blob hash mismatch: {name}")));
            }
            self.blobs.insert(name.to_owned(), bytes);
        }
        let bytes = self.blobs.get(name).ok_or_else(|| invalid("missing blob"))?;
        if bytes.len() > limit {
            return Err(invalid("blob exceeds its resource budget"));
        }
        // Count repeated references too: a tiny archive must not expand into unbounded owned
        // strings or arrays when the same blob is referenced repeatedly.
        self.hydrated = self
            .hydrated
            .checked_add(bytes.len())
            .filter(|&n| n <= (self.limits.max_expanded)())
            .ok_or_else(|| invalid("resolved payloads too large"))?;
        Ok(bytes)
    }
}

/// A small PNG picture of the file, kept in the container as a disposable cache keyed to the
/// exact file it shows: `cache/thumbnail.json` (its key) and `cache/thumbnail.png`. Reading it
/// never resolves a payload; a picture that isn't one, or is stale, is a miss.
#[derive(Clone, Copy, Debug)]
pub struct Thumbnail {
    /// The key's format tag.
    pub format: &'static str,
    /// The build that wrote it: only the same build reads it back.
    pub app: &'static str,
    /// Its width and height in pixels.
    pub size: [usize; 2],
    /// The most bytes it may have.
    pub max_bytes: usize,
}

impl Thumbnail {
    /// Whether the bytes start as a PNG of this size within the byte budget (not a decode: a
    /// consumer decodes with its own dimension and allocation limits).
    pub fn valid(&self, bytes: &[u8]) -> bool {
        bytes.len() <= self.max_bytes
            && bytes.get(..8) == Some(b"\x89PNG\r\n\x1a\n")
            && bytes.get(8..16) == Some(b"\0\0\0\rIHDR")
            && bytes.get(16..20) == Some((self.size[0] as u32).to_be_bytes().as_slice())
            && bytes.get(20..24) == Some((self.size[1] as u32).to_be_bytes().as_slice())
    }

    /// Add the picture, keyed to `document` (the file's own part), when it is one and there is
    /// room for it within `budget`; otherwise leave it out, never failing the save.
    pub fn write(&self, writer: &mut Writer, png: &[u8], document: &[u8], budget: usize) -> Result<(), FormatError> {
        if !self.valid(png) {
            return Ok(());
        }
        let key =
            serde_json::to_vec(&serde_json::json!({"format": self.format, "app": self.app, "document": digest(document), "payload": digest(png)}))
                .map_err(invalid)?;
        if !writer.room(2, key.len().saturating_add(png.len()), budget) {
            return Ok(());
        }
        writer.part("cache/thumbnail.json", &key, false)?;
        writer.part("cache/thumbnail.png", png, false)
    }

    /// The picture, when this build wrote it for exactly the file's own part (`document.json`,
    /// read within `document_limit`).
    pub fn read<R: Read + Seek>(&self, reader: &mut Reader<R>, document_limit: usize) -> Result<Vec<u8>, FormatError> {
        let key: Value = serde_json::from_slice(&reader.limited_part("cache/thumbnail.json", 4096)?).map_err(invalid)?;
        let document = reader.document(document_limit)?;
        if key.get("format").and_then(Value::as_str) != Some(self.format)
            || key.get("app").and_then(Value::as_str) != Some(self.app)
            || key.get("document").and_then(Value::as_str) != Some(digest(&document).as_str())
        {
            return Err(invalid("thumbnail key"));
        }
        let png = reader.limited_part("cache/thumbnail.png", self.max_bytes)?;
        if !self.valid(&png) || key.get("payload").and_then(Value::as_str) != Some(digest(&png).as_str()) {
            return Err(invalid("thumbnail payload"));
        }
        Ok(png)
    }
}

#[cfg(test)]
#[path = "container_tests.rs"]
mod tests;
