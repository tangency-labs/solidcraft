//! What kind of design a file holds (experimental, #6892): a single design, a part or an assembly.
//! The kind is fixed in the container's header (`container.json`'s `content`), so renaming the file
//! never changes it; each kind has its own extension. A part may hold sheet metal: that is a mark
//! the model gives it (its sheet metal bodies), not a kind of its own.
//!
//! Earlier builds of this experiment also had a sheet metal part (`"sheet_metal"`, `.scsht`): such
//! a file reads as a part ([`LEGACY_SHEET_METAL`]), and saving it writes `"part"`.
//!
//! A single design (`.sc`) writes no `content`, so its files are exactly what they were. Older
//! releases (0.45.0-rc.1 and later) open a file of another kind as a design from a newer
//! SolidCraft: with a notice, and saving it keeps the header as it came. No design format change:
//! `document.json` is the same for every kind.

use std::io::{Read, Seek};

use serde_json::{Value, json};

use super::{PackageReader, VERSION, invalid, is_container_source, open_regular_file};
use crate::Result;

/// The kind of design a file holds.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DesignKind {
    /// Parts, sheet metal and assemblies together in one design: what every design was before.
    #[default]
    Single,
    Part,
    Assembly,
}

/// The header `content` and extension of the sheet metal part of this experiment's earlier builds,
/// read as a part's.
pub const LEGACY_SHEET_METAL: (&str, &str) = ("sheet_metal", "scsht");

impl DesignKind {
    pub const ALL: [DesignKind; 3] = [DesignKind::Single, DesignKind::Part, DesignKind::Assembly];

    /// Its `content` in the container's header; a single design writes none.
    pub fn content(self) -> Option<&'static str> {
        match self {
            DesignKind::Single => None,
            DesignKind::Part => Some("part"),
            DesignKind::Assembly => Some("assembly"),
        }
    }

    /// The kind a header's `content` names, if it names one; the earlier sheet metal part is a part.
    pub fn from_content(content: &str) -> Option<DesignKind> {
        if content == LEGACY_SHEET_METAL.0 {
            return Some(DesignKind::Part);
        }
        DesignKind::ALL.into_iter().find(|kind| kind.content() == Some(content))
    }

    /// The extension of its files, without the dot.
    pub fn extension(self) -> &'static str {
        match self {
            DesignKind::Single => crate::DESIGN_EXT,
            DesignKind::Part => "scprt",
            DesignKind::Assembly => "scasm",
        }
    }

    /// Its name in replies and messages.
    pub fn name(self) -> &'static str {
        match self {
            DesignKind::Single => "single design",
            DesignKind::Part => "part",
            DesignKind::Assembly => "assembly",
        }
    }

    /// The kind `path`'s extension stands for (any case), if it is one of the three; the earlier
    /// sheet metal part's `.scsht` is a part's.
    pub fn for_path(path: &str) -> Option<DesignKind> {
        let extension = std::path::Path::new(path).extension()?.to_str()?;
        if extension.eq_ignore_ascii_case(LEGACY_SHEET_METAL.1) {
            return Some(DesignKind::Part);
        }
        DesignKind::ALL.into_iter().find(|kind| kind.extension().eq_ignore_ascii_case(extension))
    }

    /// The container's header for a design of this kind.
    pub(super) fn header(self) -> Value {
        match self.content() {
            Some(content) => json!({"format": VERSION, "content": content}),
            None => json!({"format": VERSION}),
        }
    }
}

/// The kind a container's header says: none or a `content` that isn't a kind is a single design.
pub(super) fn header_kind(container: &Value) -> DesignKind {
    container.get("content").and_then(Value::as_str).and_then(DesignKind::from_content).unwrap_or_default()
}

/// The kind of design the container `reader` holds.
pub(super) fn container_design_kind<R: Read + Seek>(reader: &mut PackageReader<R>) -> Result<DesignKind> {
    let container: Value = serde_json::from_slice(&reader.limited_part("container.json", 4096)?).map_err(invalid)?;
    Ok(header_kind(&container))
}

/// The kind of design the file at `path` holds, by its header alone. A plain JSON design, a file
/// that can't be read, or one that isn't a container is a single design.
pub fn read_design_kind_file(path: &str) -> DesignKind {
    let read = || -> Result<DesignKind> {
        let mut source = open_regular_file(path)?;
        if !is_container_source(&mut source)? {
            return Ok(DesignKind::Single);
        }
        container_design_kind(&mut PackageReader::new(source)?)
    };
    read().unwrap_or_default()
}

/// The kind of design `bytes` hold ([`read_design_kind_file`] for bytes).
pub fn design_kind(bytes: &[u8]) -> DesignKind {
    if !super::is_design_container(bytes) {
        return DesignKind::Single;
    }
    PackageReader::new(std::io::Cursor::new(bytes)).and_then(|mut reader| container_design_kind(&mut reader)).unwrap_or_default()
}

/// Whether the file at `path` was saved as a sheet metal part by an earlier build of this
/// experiment (its header says `"sheet_metal"`); it reads as a part.
pub fn is_legacy_sheet_metal_file(path: &str) -> bool {
    let read = || -> Result<bool> {
        let mut source = open_regular_file(path)?;
        if !is_container_source(&mut source)? {
            return Ok(false);
        }
        let container: Value = serde_json::from_slice(&PackageReader::new(source)?.limited_part("container.json", 4096)?).map_err(invalid)?;
        Ok(container.get("content").and_then(Value::as_str) == Some(LEGACY_SHEET_METAL.0))
    };
    read().unwrap_or(false)
}
