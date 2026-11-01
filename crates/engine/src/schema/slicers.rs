//! Contracts and result types for the command in `cmd/file.rs` that lists the installed slicers:
//! `file.slicers` (#13306).
//!
//! The handler reads no parameter, so its contract has no key: a key in a call is reported in
//! warn and refused in strict, and a call without one runs as it always did.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// `file.slicers` takes no parameters.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct SlicersParams {}

/// What `file.slicers` returns: the network folders the user added, then the slicers found on
/// this machine, known ones in a fixed order (no slicer in the browser). Pass an entry's `id`,
/// its `name` or a slicer's `program` as `file.send_to_slicer`'s or `file.save_mesh`'s `slicer`.
#[derive(Debug, Serialize, JsonSchema)]
pub struct SlicersResult {
    /// The network folders, then the slicers installed on this machine.
    pub slicers: Vec<SlicerEntry>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// One entry of the list: a network folder or an installed slicer.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum SlicerEntry {
    /// A network folder the user added (`file.slicers.add_remote`).
    Folder(NetworkFolderInfo),
    /// An installed slicer, or a program the user added under Custom….
    Slicer(SlicerInfo),
}

/// One network folder the user added: a send copies the file there and starts no program.
#[derive(Debug, Serialize, JsonSchema)]
pub struct NetworkFolderInfo {
    /// `remote:<name>`.
    pub id: String,
    /// The name the user gave it.
    pub name: String,
    /// Always true: this entry is a network folder, not a slicer.
    pub remote: bool,
    /// `folder`: a folder on this computer, usually a mounted share.
    pub kind: String,
    /// The folder the files go to.
    pub folder: String,
}

/// One installed slicer.
#[derive(Debug, Serialize, JsonSchema)]
pub struct SlicerInfo {
    /// `prusa`, `bambu`, … for the known ones, `custom` for a program the user picked. When one
    /// slicer is installed more than once, each install has its own id (`prusa#<where it was
    /// found>`).
    pub id: String,
    /// The name to show, which says which install it is when there are several.
    pub name: String,
    /// The program that is started, as a path or a command name; null when there is none.
    pub program: Option<String>,
    /// The path of the application's icon file (a PNG, a macOS `.icns` file or a Windows
    /// program); null when none was found.
    pub icon: Option<String>,
}

#[cfg(test)]
#[path = "slicers_tests.rs"]
mod tests;
