//! What kind of design a file holds (experimental, #6892): a single design, a part, a sheet metal
//! part or an assembly. The kind is fixed in the container's header (`container.json`'s
//! `content`), so renaming the file never changes it; each kind has its own extension.
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
    SheetMetal,
    Assembly,
}

impl DesignKind {
    pub const ALL: [DesignKind; 4] = [DesignKind::Single, DesignKind::Part, DesignKind::SheetMetal, DesignKind::Assembly];

    /// Its `content` in the container's header; a single design writes none.
    pub fn content(self) -> Option<&'static str> {
        match self {
            DesignKind::Single => None,
            DesignKind::Part => Some("part"),
            DesignKind::SheetMetal => Some("sheet_metal"),
            DesignKind::Assembly => Some("assembly"),
        }
    }

    /// The kind a header's `content` names, if it names one.
    pub fn from_content(content: &str) -> Option<DesignKind> {
        DesignKind::ALL.into_iter().find(|kind| kind.content() == Some(content))
    }

    /// The extension of its files, without the dot.
    pub fn extension(self) -> &'static str {
        match self {
            DesignKind::Single => crate::DESIGN_EXT,
            DesignKind::Part => "scprt",
            DesignKind::SheetMetal => "scsht",
            DesignKind::Assembly => "scasm",
        }
    }

    /// Its name in replies and messages.
    pub fn name(self) -> &'static str {
        match self {
            DesignKind::Single => "single design",
            DesignKind::Part => "part",
            DesignKind::SheetMetal => "sheet metal part",
            DesignKind::Assembly => "assembly",
        }
    }

    /// The kind `path`'s extension stands for (any case), if it is one of the four.
    pub fn for_path(path: &str) -> Option<DesignKind> {
        let extension = std::path::Path::new(path).extension()?.to_str()?;
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
