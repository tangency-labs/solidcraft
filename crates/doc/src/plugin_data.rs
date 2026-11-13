//! Per-design plugin data: the files a plugin keeps with one design, under `plugins/<plugin-id>/`
//! in the design container (base64 in a plain JSON design). The design keeps them and saves them
//! back byte for byte, whether or not the plugin is installed: a missing plugin never stops a
//! design opening, and never loses its data. Plugins read and write them in a later capability
//! (`storage`); until then nothing changes them.
use std::collections::BTreeMap;

use serde::{Deserialize, Deserializer};

/// The container folder plugin data lives in.
pub const PREFIX: &str = "plugins/";
/// At most this many files of plugin data in one design; larger namespaces are refused.
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

/// Reads a design's `plugin_data`, leaving out keys that are not [`valid_key`]. More than
/// [`MAX_FILES`] valid files is an error, so opening cannot silently lose plugin data.
pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<BTreeMap<String, String>, D::Error> {
    let all = BTreeMap::<String, String>::deserialize(deserializer)?;
    let files: BTreeMap<_, _> = all.into_iter().filter(|(key, _)| valid_key(key)).take(MAX_FILES + 1).collect();
    if files.len() > MAX_FILES {
        return Err(serde::de::Error::custom(format!("plugin data exceeds {MAX_FILES} files")));
    }
    Ok(files)
}
