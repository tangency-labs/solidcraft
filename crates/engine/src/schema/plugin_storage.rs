//! Typed contracts for the `storage.*` commands: the data a plugin keeps in a design.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Read a plugin's data in the open design. A plugin reads its own and leaves `plugin` out.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StorageGetParams {
    /// The plugin id. Required outside a plugin's run; inside one, only the plugin's own id.
    #[serde(default)]
    pub plugin: Option<String>,
    /// Read the serialized main document in parts: supplying `offset` or `len` returns a
    /// DocumentRead instead of StorageData. Byte offset; default 0.
    #[serde(default)]
    pub offset: Option<u64>,
    /// The most document bytes to return, 1 to 524288 (512 KiB). Default 524288.
    #[serde(default)]
    pub len: Option<u32>,
}

/// Either the complete plugin data or a bounded read of its serialized main document.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum StorageGetResult {
    Full(StorageData),
    Chunk(DocumentRead),
}

/// Part of a plugin's serialized main document, without the file list or metadata.
#[derive(Debug, Serialize, JsonSchema)]
pub struct DocumentRead {
    pub plugin: String,
    /// Raw JSON bytes from `offset`, base64; empty past the end or when there is no document.
    pub data: String,
    pub offset: u64,
    /// The whole serialized document's size; 0 when there is none.
    pub bytes: u64,
}

/// One of a plugin's files in the design.
#[derive(Debug, Serialize, JsonSchema)]
pub struct BlobInfo {
    pub name: String,
    /// A file the plugin can make again, which a save may drop.
    pub cache: bool,
    pub bytes: u64,
}

/// A plugin's data in the open design.
#[derive(Debug, Serialize, JsonSchema)]
pub struct StorageData {
    pub plugin: String,
    /// The plugin's name as its data records it ("" when unknown).
    pub name: String,
    /// The version of the plugin that last wrote the data ("" when unknown).
    pub plugin_version: String,
    /// The data's format number as saved; `null` when the design has none of this plugin's data.
    pub format: Option<u32>,
    /// The format the running plugin writes, inside a plugin's run. A saved `format` lower than
    /// this is data to migrate (the plugin's next `storage.set` saves it in this format); higher
    /// is data from a newer version of the plugin, which this one only reads.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plugin_format: Option<u32>,
    /// Whether the data can't be changed from here: written by a newer version of the plugin.
    pub read_only: bool,
    /// The main document; `null` when there is none.
    pub document: Value,
    pub blobs: Vec<BlobInfo>,
}

/// Replace the calling plugin's main document: one undoable edit.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StorageSetParams {
    /// Any JSON, at most 16 MiB written out.
    pub document: Value,
}

/// Change the calling plugin's main document with an RFC 7396 merge patch: one undoable edit.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StoragePatchParams {
    /// The merge patch: objects merge key by key, `null` removes a key, anything else replaces.
    pub patch: Value,
}

/// What a write left.
#[derive(Debug, Serialize, JsonSchema)]
pub struct StorageWritten {
    pub plugin: String,
    /// The format the data is now saved in: the running plugin's.
    pub format: u32,
    /// The main document's size, written out.
    pub bytes: u64,
}

/// Write one of the calling plugin's files: one undoable edit.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StoragePutBlobParams {
    /// A relative path of plain names (letters, digits, `.`, `_`, `-`; none starting with `.`).
    pub name: String,
    /// The bytes, base64.
    pub data: String,
    /// A file the plugin can make again (a cached toolpath): a save may drop it.
    #[serde(default)]
    pub cache: bool,
    /// Add the bytes to the end of the file instead of replacing it, to write a large file in parts.
    #[serde(default)]
    pub append: bool,
}

/// What `storage.put_blob` left.
#[derive(Debug, Serialize, JsonSchema)]
pub struct BlobWritten {
    pub name: String,
    pub cache: bool,
    /// The file's size now.
    pub bytes: u64,
}

/// Read part of a plugin's file.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StorageGetBlobParams {
    /// The plugin id. Required outside a plugin's run; inside one, only the plugin's own id.
    #[serde(default)]
    pub plugin: Option<String>,
    pub name: String,
    /// Where to start, in bytes. Default 0.
    #[serde(default)]
    pub offset: u64,
    /// The most bytes to return, 1 to 524288 (512 KiB). Default 524288.
    #[serde(default)]
    pub len: Option<u32>,
}

/// Part of a plugin's file.
#[derive(Debug, Serialize, JsonSchema)]
pub struct BlobRead {
    pub name: String,
    pub cache: bool,
    /// The bytes from `offset`, base64; empty past the end.
    pub data: String,
    pub offset: u64,
    /// The whole file's size.
    pub bytes: u64,
}

/// Delete one of the calling plugin's files: one undoable edit.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StorageDeleteBlobParams {
    pub name: String,
}

/// What `storage.delete_blob` did.
#[derive(Debug, Serialize, JsonSchema)]
pub struct BlobDeleted {
    pub name: String,
    /// False when there was no such file.
    pub deleted: bool,
}

/// List the plugins that keep data in the open design (takes nothing).
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StorageListParams {}

/// Whether the plugin that keeps some data can change it here.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DataStatus {
    /// The plugin is installed, on, and writes this format.
    Ready,
    /// The plugin is installed and on, and writes a newer format: it migrates the data when it next
    /// writes it.
    Migrate,
    /// The plugin isn't installed: the data is kept and saved back unchanged.
    Missing,
    /// The plugin is installed but turned off.
    Disabled,
    /// The installed plugin keeps no data in designs, or keeps an older format than this data's.
    Older,
}

/// One plugin's data in the open design, and whether that plugin can change it here.
#[derive(Debug, Serialize, JsonSchema)]
pub struct StoredPlugin {
    pub plugin: String,
    /// The plugin's name: the installed plugin's, else the one its data records, else its id.
    pub name: String,
    /// The version of the plugin that last wrote the data ("" when unknown).
    pub plugin_version: String,
    /// The data's format number as saved; 0 when unknown.
    pub format: u32,
    /// The installed plugin's version, when it is installed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub installed_version: Option<String>,
    pub status: DataStatus,
    /// Whether the data is only kept, not changed, here (every status but ready and migrate).
    pub read_only: bool,
    /// The bytes it keeps, its main document and files together.
    pub bytes: u64,
}

/// The plugins that keep data in the open design.
#[derive(Debug, Serialize, JsonSchema)]
pub struct StorageList {
    pub plugins: Vec<StoredPlugin>,
}
