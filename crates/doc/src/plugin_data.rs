//! Per-design plugin data: the files a plugin keeps with one design, under `plugins/<plugin-id>/`
//! in the design container (base64 in a plain JSON design). The design keeps them and saves them
//! back byte for byte, whether or not the plugin is installed: a missing plugin never stops a
//! design opening, and never loses its data.
//!
//! A plugin with the `storage` capability keeps its data in this layout (RFC 0015, section 5):
//! - `<plugin-id>/meta.json`: [`Meta`], who wrote the data and in which of the plugin's formats;
//! - `<plugin-id>/data.json`: the plugin's main document, any JSON;
//! - `<plugin-id>/blobs/<name>`: files of its own;
//! - `<plugin-id>/cache/<name>`: files it can make again (a cached toolpath), which may be dropped.
//!
//! Only the plugin itself changes them, through the app's `storage.*` commands; anything else in
//! the folder is kept as it came.
use std::collections::BTreeMap;
use std::sync::Arc;

use serde::{Deserialize, Deserializer, Serialize};

/// The container folder plugin data lives in.
pub const PREFIX: &str = "plugins/";
/// At most this many files of plugin data in one design; past it the rest is left out.
pub const MAX_FILES: usize = 1024;

/// A plugin data key, `<plugin-id>/<path>`: a plugin id (1–64 lowercase letters, digits and
/// inner hyphens), then a relative path of plain names (letters, digits, `.`, `_`, `-`; none
/// starting with `.`), 255 bytes at most. Keys are never paths on disk.
pub fn valid_key(key: &str) -> bool {
    let Some((id, path)) = key.split_once('/') else { return false };
    let id_ok = (1..=64).contains(&id.len())
        && id.bytes().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
        && !id.starts_with('-')
        && !id.ends_with('-');
    id_ok
        && key.len() <= 255
        && path.split('/').all(|name| {
            !name.is_empty() && !name.starts_with('.') && name.bytes().all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'_' | b'-'))
        })
}

/// The plugin's record of its data, `<plugin-id>/meta.json`.
pub const META: &str = "meta.json";
/// The plugin's main document, `<plugin-id>/data.json`.
pub const DATA: &str = "data.json";
/// The folder of the plugin's own files.
pub const BLOBS: &str = "blobs/";
/// The folder of files the plugin can make again.
pub const CACHE: &str = "cache/";

/// Who wrote a plugin's data and in which of its formats (`<plugin-id>/meta.json`). Read leniently:
/// a field that is missing or of the wrong kind reads as empty, so a hand-edited file never stops
/// the design opening.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Meta {
    pub plugin: String,
    /// The plugin's name, for the user ("Contains Manufacture data").
    pub name: String,
    pub plugin_version: String,
    /// The data's own format number; 0 when it is not known.
    pub format: u32,
}

impl Meta {
    /// Reads `bytes` as a meta record; `None` when they are not a JSON object.
    pub fn parse(bytes: &[u8]) -> Option<Self> {
        let value: serde_json::Value = serde_json::from_slice(bytes).ok()?;
        let object = value.as_object()?;
        let text = |key: &str, max: usize| -> String {
            object
                .get(key)
                .and_then(serde_json::Value::as_str)
                .filter(|t| !t.chars().any(char::is_control))
                .map(|t| t.chars().take(max).collect())
                .unwrap_or_default()
        };
        let format = object.get("format").and_then(serde_json::Value::as_u64).and_then(|f| u32::try_from(f).ok()).unwrap_or(0);
        Some(Self { plugin: text("plugin", 64), name: text("name", 80), plugin_version: text("plugin_version", 32), format })
    }
}

/// The ids of the plugins that have data in `data`, in order.
pub fn plugins(data: &BTreeMap<String, Arc<str>>) -> Vec<String> {
    let mut ids: Vec<String> = data.keys().filter_map(|key| key.split_once('/').map(|(id, _)| id.to_string())).collect();
    ids.dedup();
    ids
}

/// The bytes of one plugin data file, `None` when there is no such file or it is not base64.
pub fn file(data: &BTreeMap<String, Arc<str>>, id: &str, path: &str) -> Option<Vec<u8>> {
    data.get(&format!("{id}/{path}")).and_then(|text| crate::canvas::base64_decode(text))
}

/// A plugin's meta record: its `meta.json`, or, when that is missing or unreadable, one naming
/// only the plugin id, so data written by hand still says which plugin it belongs to.
pub fn meta(data: &BTreeMap<String, Arc<str>>, id: &str) -> Meta {
    let mut meta = file(data, id, META).and_then(|bytes| Meta::parse(&bytes)).unwrap_or_default();
    meta.plugin = id.to_string();
    meta
}

/// Reads a design's `plugin_data`, leaving out keys that are not [`valid_key`] and anything past
/// [`MAX_FILES`], so a hand-edited design still opens.
pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<BTreeMap<String, Arc<str>>, D::Error> {
    let all = BTreeMap::<String, Arc<str>>::deserialize(deserializer)?;
    Ok(all.into_iter().filter(|(key, _)| valid_key(key)).take(MAX_FILES).collect())
}
