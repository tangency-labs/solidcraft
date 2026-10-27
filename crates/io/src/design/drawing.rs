//! A design's drawings in its container (#3890): each drawing in its own entry,
//! `drawings/<id>.json` (Deflate, as `document.json`), its id as [`ids::encode`] writes it and its
//! text [`Drawing::to_json`]'s. `document.json` holds nothing of them, so its bytes, and the model
//! cache keyed to them, don't change when only a drawing does. An entry under `drawings/` that
//! isn't a drawing's, or whose record has another id, is damage: the design is refused.

use std::io::{Read, Seek};
use std::sync::Arc;

use solidcraft_doc::drawing::{self, Drawing, MAX_DRAWING_BYTES};
use solidcraft_doc::{Document, ids};

use super::{PackageReader, PackageWriter, invalid};
use crate::Result;

/// The folder of a container's drawing entries.
const FOLDER: &str = "drawings/";

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
    let names: Vec<String> = reader.names()?.into_iter().filter(|n| n.starts_with(FOLDER)).collect();
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
