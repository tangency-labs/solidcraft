//! Contracts and result types for Copy Design with Links (`cmd/file_copy_links.rs`): `file.copy_with_links.list`
//! lists a saved design, the files it links (all the way down) and the designs whose drawings draw
//! any of them; `file.copy_with_links` copies the ones picked into a folder under new names, their links
//! pointing at each other's copies and every file left out still linked where it is.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// List what Copy Design with Links would copy for a saved design. Nothing is written.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CopyLinksListParams {
    /// The saved design to start from. Left out: the design that is open, which must be saved with
    /// no unsaved changes.
    #[serde(default)]
    pub path: Option<String>,
    /// Also list the designs in the same folders whose drawings draw a listed design (default true).
    #[serde(default)]
    pub drawings: Option<bool>,
}

/// One file Copy Design with Links found.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CopyLinksItem {
    /// The file, as a whole path.
    pub path: String,
    /// `design` (the design copied, or one it links), `drawing` (a design whose drawings draw a
    /// listed design) or `file` (a linked STEP or other source file, copied byte for byte).
    pub kind: String,
    /// The listed files that link or draw it; empty for the design copied.
    pub used_by: Vec<String>,
    /// Why it can't be copied (missing, unreadable, saved by a newer SolidCraft…): it stays where it
    /// is and every copy keeps linking it there. Absent: it can be copied.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub problem: Option<String>,
}

/// What `file.copy_with_links.list` found: the design copied first, then each file in the order found.
#[derive(Debug, Serialize, JsonSchema)]
pub struct CopyLinksListResult {
    /// The design copied, as a whole path.
    pub design: String,
    pub items: Vec<CopyLinksItem>,
    /// Design files in the searched folders that could not be read while looking for drawings.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skipped: Vec<String>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// One file to copy, and the file name its copy gets.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CopyLinksFile {
    /// A file `file.copy_with_links.list` lists, as it lists it.
    pub path: String,
    /// The copy's file name in `folder`, with its extension: a name only, no folder.
    pub name: String,
}

/// Copy a saved design and the files picked from its list into a folder, under new names. A copied
/// design's links and drawings point at the copies; a file not picked keeps being linked where it
/// is. Nothing is replaced: an existing file at any new name refuses the whole call, and a call that
/// fails part way removes the copies it made. The design that is open is not changed.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CopyLinksParams {
    /// The saved design to start from, as `file.copy_with_links.list` takes it.
    #[serde(default)]
    pub path: Option<String>,
    /// Look for drawings as `file.copy_with_links.list` does (default true).
    #[serde(default)]
    pub drawings: Option<bool>,
    /// The folder the copies go in. It must exist.
    pub folder: String,
    /// The files to copy and their new names. It must hold the design copied; every other listed
    /// file is left where it is.
    pub copy: Vec<CopyLinksFile>,
}

/// One copy made.
#[derive(Debug, Serialize, JsonSchema)]
pub struct CopyLinksCopied {
    /// The file copied.
    pub from: String,
    /// Its copy.
    pub to: String,
}

/// What `file.copy_with_links` wrote.
#[derive(Debug, Serialize, JsonSchema)]
pub struct CopyLinksResult {
    /// The copy of the design copied.
    pub design: String,
    pub copied: Vec<CopyLinksCopied>,
    /// The listed files left where they are, which the copies still link.
    pub kept: Vec<String>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}
