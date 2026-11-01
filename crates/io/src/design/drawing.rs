//! Drawing files (#3890): a [`Drawing`] saved as a container or as plain JSON, like a design.
//!
//! A drawing container is a design container holding `drawing.json` (Deflate, as
//! `document.json`) in place of `document.json`, and saying so in `container.json`:
//! `{"content": "drawing", "format": "solidcraft-container/1"}`. A design's `container.json` has no
//! `content`, as before. The container's own checks (budgets, entries, the ZIP directory) are the
//! design container's. The plain JSON form is [`Drawing::to_json`]'s text, the same bytes each
//! save, so a drawing can live in version control as text.
//!
//! A design is never read as a drawing nor a drawing as a design: [`file_kind`] tells them apart,
//! and each reader refuses the other's file by name.

use std::io::{Cursor, Read, Seek};

use serde::Deserialize;
use serde_json::{Value, json};
use solidcraft_doc::drawing::{Drawing, MAX_DRAWING_BYTES};
use solidcraft_doc::format::DRAWING;

use super::{PackageReader, PackageWriter, VERSION, invalid, is_design_container};
use crate::{IoError, MAX_DESIGN_BYTES, Result};

/// The part a drawing container keeps its drawing in.
const DRAWING_PART: &str = "drawing.json";
/// What a drawing's `container.json` says it holds.
const DRAWING_CONTENT: &str = "drawing";

/// What a SolidCraft file holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileKind {
    Design,
    Drawing,
}

/// Which kind of file `bytes` are, from the container's `container.json` or the JSON's `format`.
/// JSON that isn't a drawing's counts as a design, which the design reader then checks.
pub fn file_kind(bytes: &[u8]) -> Result<FileKind> {
    if is_design_container(bytes) {
        return container_kind(&mut PackageReader::new(Cursor::new(bytes))?);
    }
    #[derive(Deserialize)]
    struct Format {
        format: Option<String>,
    }
    let format = serde_json::from_slice::<Format>(bytes).ok().and_then(|f| f.format);
    Ok(if format.is_some_and(|f| DRAWING.version_of(&f).is_some()) { FileKind::Drawing } else { FileKind::Design })
}

/// What a container holds, by its `container.json`: no `content` is a design.
pub(super) fn container_kind<R: Read + Seek>(reader: &mut PackageReader<R>) -> Result<FileKind> {
    let container: Value = serde_json::from_slice(&reader.limited_part("container.json", 4096)?).map_err(invalid)?;
    if !matches!(container.get("format").and_then(Value::as_str), Some(super::VERSION | super::LEGACY_VERSION)) {
        return Err(invalid("unsupported container version"));
    }
    match container.get("content") {
        None => Ok(FileKind::Design),
        Some(Value::String(content)) if content == DRAWING_CONTENT => Ok(FileKind::Drawing),
        Some(other) => Err(invalid(format!("this build doesn't know container content {other}"))),
    }
}

/// The drawing as a container: the same bytes for the same drawing.
pub fn write_drawing_container(drawing: &Drawing) -> Result<Vec<u8>> {
    let text = drawing.to_json()?;
    let mut writer = PackageWriter::new();
    writer.part(DRAWING_PART, text.as_bytes(), true)?;
    let container = json!({"format": VERSION, "content": DRAWING_CONTENT});
    writer.part("container.json", &serde_json::to_vec(&container).map_err(invalid)?, false)?;
    writer.finish()
}

/// Read a drawing file's bytes: a container or plain JSON.
pub fn read_drawing(bytes: &[u8]) -> Result<Drawing> {
    if bytes.len() > MAX_DESIGN_BYTES {
        return Err(IoError::Invalid("drawing file too large".into()));
    }
    if is_design_container(bytes) {
        return read_container(Cursor::new(bytes));
    }
    if file_kind(bytes)? == FileKind::Design {
        return Err(IoError::Invalid("this file is a SolidCraft design, not a drawing: open it as a design".into()));
    }
    let text = std::str::from_utf8(bytes).map_err(|_| IoError::Invalid("drawing file is not UTF-8 JSON".into()))?;
    Ok(Drawing::from_json(text)?)
}

/// Read a drawing from disk (or the browser's files), a container through one seekable handle.
pub fn read_drawing_file(path: &str) -> Result<Drawing> {
    let mut source = super::open_regular_file(path)?;
    if super::is_container_source(&mut source)? {
        return read_container(source);
    }
    let mut bytes = Vec::new();
    source.take(MAX_DRAWING_BYTES as u64 + 1).read_to_end(&mut bytes).map_err(invalid)?;
    read_drawing(&bytes)
}

fn read_container<R: Read + Seek>(source: R) -> Result<Drawing> {
    let mut reader = PackageReader::new(source)?;
    if container_kind(&mut reader)? == FileKind::Design {
        return Err(IoError::Invalid("this file is a SolidCraft design, not a drawing: open it as a design".into()));
    }
    if reader.has_part("document.json") {
        return Err(invalid("a drawing container also holds a design's document.json"));
    }
    let bytes = reader.limited_part(DRAWING_PART, MAX_DRAWING_BYTES)?;
    let text = std::str::from_utf8(&bytes).map_err(invalid)?;
    Ok(Drawing::from_json(text)?)
}

#[cfg(test)]
#[path = "drawing_tests.rs"]
mod tests;
