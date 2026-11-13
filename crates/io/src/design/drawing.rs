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
//!
//! A design's drawings sit in its container (#3890): each drawing in its own entry,
//! `drawings/<id>.json` (Deflate, as `document.json`), its id as [`ids::encode`] writes it and its
//! text [`Drawing::to_json`]'s. `document.json` holds nothing of them, so its bytes, and the model
//! cache keyed to them, don't change when only a drawing does. An entry under `drawings/` that
//! isn't a drawing's, or whose record has another id, is damage: the design is refused.

use std::io::{Cursor, Read, Seek};
use std::sync::Arc;

use serde::Deserialize;
use serde_json::{Value, json};
use solidcraft_doc::drawing::{self, Drawing, MAX_DRAWING_BYTES};
use solidcraft_doc::format::DRAWING;
use solidcraft_doc::{Document, ids};

use super::{PackageReader, PackageWriter, VERSION, invalid, is_design_container};
use crate::{IoError, MAX_DESIGN_BYTES, Result};

/// The part a drawing container keeps its drawing in.
const DRAWING_PART: &str = "drawing.json";
/// What a drawing's `container.json` says it holds.
const DRAWING_CONTENT: &str = "drawing";
/// The folder of a design container's drawing entries.
const FOLDER: &str = "drawings/";

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
    if !container.get("format").and_then(Value::as_str).is_some_and(super::is_container_version) {
        return Err(invalid("unsupported container version"));
    }
    match container.get("content") {
        None => Ok(FileKind::Design),
        Some(Value::String(content)) if content == DRAWING_CONTENT => Ok(FileKind::Drawing),
        // A later kind of content that still holds a design opens as one.
        Some(_) if reader.has_part("document.json") => Ok(FileKind::Design),
        Some(other) => Err(invalid(format!("it holds {other}, which this version doesn't know: update SolidCraft to open it"))),
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

/// Write each of the design's drawings to its entry.
pub(super) fn write(writer: &mut PackageWriter, doc: &Document) -> Result<()> {
    drawing::check_in_design(&doc.drawings)?;
    for d in &doc.drawings {
        let text = d.to_json().map_err(|e| invalid(format!("{} not saved: {e}", d.label())))?;
        writer.part(&format!("{FOLDER}{}.json", ids::encode(d.id)), text.as_bytes(), true)?;
    }
    Ok(())
}

/// Read the container's drawings, in their order.
pub(super) fn read<R: Read + Seek>(reader: &mut PackageReader<R>) -> Result<Vec<Arc<Drawing>>> {
    let names: Vec<String> = reader.part_names().into_iter().filter(|n| n.starts_with(FOLDER)).collect();
    drawing::check_count(names.len()).map_err(invalid)?;
    let mut drawings = Vec::with_capacity(names.len());
    for name in names {
        let id = name.strip_prefix(FOLDER).and_then(|n| n.strip_suffix(".json")).and_then(ids::decode);
        let id = id.ok_or_else(|| invalid(format!("`{name}` is not a drawing's entry")))?;
        let bytes = reader.limited_part(&name, MAX_DRAWING_BYTES)?;
        let d = Drawing::from_json(std::str::from_utf8(&bytes).map_err(invalid)?).map_err(|e| invalid(format!("`{name}`: {e}")))?;
        if d.id != id {
            return Err(invalid(format!("`{name}` holds the drawing {}", ids::encode(d.id))));
        }
        drawings.push(d);
    }
    drawing::in_design(drawings).map_err(invalid)
}

#[cfg(test)]
#[path = "drawing_tests.rs"]
mod tests;
