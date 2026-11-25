//! The data a plugin keeps in a design (capability `storage@0.1`, RFC 0015 section 5), in the
//! layout [`solidcraft_doc::plugin_data`] describes. Only the plugin that owns the data changes it,
//! from inside its own run; anyone reads it. Data whose plugin is missing, off or older is kept and
//! saved back byte for byte.
use std::collections::BTreeMap;

use serde_json::Value;
use solidcraft_doc::Document;
use solidcraft_doc::blob::Blob;
use solidcraft_doc::plugin_data::{self, BLOBS, CACHE, DATA, META, Meta};

use crate::schema::plugin_storage::{BlobInfo, DataStatus, StorageData, StoredPlugin};
use crate::{EngineError, Result, Session};
use craft_plugin::HostError;

/// The largest main document a plugin keeps in one design, written out.
pub const MAX_DOCUMENT_BYTES: usize = 16 << 20;
/// The most bytes of files one plugin keeps in one design.
pub const MAX_BLOB_BYTES: u64 = 256 << 20;
/// The most bytes of plugin data, all plugins together, in one design: the design file's own limit.
pub const MAX_TOTAL_BYTES: u64 = 256 << 20;
/// The most bytes one read of a file or of the main document returns (`get-blob`, `get-document`).
pub const MAX_READ_BYTES: u32 = 1 << 20;

/// The plugin whose run is under way, set by the host for that run alone (never from a design or a
/// caller's parameters). Only it changes data, and only its own.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StorageCaller {
    pub id: String,
    pub name: String,
    pub version: String,
    /// The format the plugin writes (its manifest's `runtime.data_format`).
    pub data_format: u32,
}

fn denied(permission: impl Into<String>) -> EngineError {
    HostError::PermissionDenied { permission: permission.into() }.into()
}
fn limit(what: impl Into<String>) -> EngineError {
    HostError::LimitExceeded { limit: what.into() }.into()
}

/// The plugin a read is for: the caller's own inside a plugin's run, else the one named.
pub(crate) fn reader(s: &Session, cmd: &str, named: Option<&str>) -> Result<String> {
    match (&s.plugin_state.storage_caller, named) {
        (Some(caller), Some(id)) if id != caller.id => Err(denied(format!("a plugin reads only its own data, not {id}'s"))),
        (Some(caller), _) => Ok(caller.id.clone()),
        (None, Some(id)) if craft_plugin::valid_id(id) => Ok(id.to_string()),
        (None, Some(_)) => Err(crate::params::bad(cmd, "`plugin` must be a plugin id")),
        (None, None) => Err(crate::params::bad(cmd, "`plugin` is required: the id of the plugin whose data to read")),
    }
}

/// The plugin a write is for: only a plugin's own run writes, and only that plugin's data, and not
/// data a newer version of the plugin wrote.
pub(crate) fn writer(s: &Session) -> Result<StorageCaller> {
    let caller = s
        .plugin_state
        .storage_caller
        .clone()
        .ok_or_else(|| denied("only a plugin changes its data, from its own run (plugin.run); read it with storage.get"))?;
    let saved = plugin_data::meta(&s.doc.plugin_data, &caller.id).format;
    if saved > caller.data_format {
        let name = if caller.name.is_empty() { &caller.id } else { &caller.name };
        return Err(denied(format!(
            "{name} data in this design was saved by a newer version of the plugin (data format {saved}; this version writes {}); update the plugin to change it",
            caller.data_format
        )));
    }
    Ok(caller)
}

/// A plugin's files, by name, with whether each is a cache file and its size.
fn blobs(data: &BTreeMap<String, Blob>, id: &str) -> Vec<BlobInfo> {
    let prefix = format!("{id}/");
    data.iter()
        .filter(|(key, _)| key.starts_with(&prefix))
        .filter_map(|(key, bytes)| {
            let path = &key[prefix.len()..];
            let (name, cache) = match (path.strip_prefix(BLOBS), path.strip_prefix(CACHE)) {
                (Some(name), _) => (name, false),
                (_, Some(name)) => (name, true),
                _ => return None,
            };
            Some(BlobInfo { name: name.to_string(), cache, bytes: bytes.len() as u64 })
        })
        .collect()
}

/// Every byte a plugin keeps in the design.
fn plugin_bytes(data: &BTreeMap<String, Blob>, id: &str) -> u64 {
    let prefix = format!("{id}/");
    data.iter().filter(|(key, _)| key.starts_with(&prefix)).map(|(_, bytes)| bytes.len() as u64).sum()
}

/// A plugin's data in `doc`, for a reader. `caller` is the running plugin, when there is one.
/// An exporter that declares it reads its plugin's data gets it here too.
pub fn read(doc: &Document, id: &str, caller: Option<&StorageCaller>) -> StorageData {
    let data = &doc.plugin_data;
    let stored = data.contains_key(&format!("{id}/{META}")) || data.contains_key(&format!("{id}/{DATA}")) || !blobs(data, id).is_empty();
    let meta = plugin_data::meta(data, id);
    let document = plugin_data::file(data, id, DATA).and_then(|bytes| serde_json::from_slice(bytes).ok()).unwrap_or(Value::Null);
    let plugin_format = caller.filter(|caller| caller.id == id).map(|caller| caller.data_format);
    StorageData {
        plugin: id.to_string(),
        name: meta.name,
        plugin_version: meta.plugin_version,
        format: stored.then_some(meta.format),
        plugin_format,
        read_only: plugin_format.is_some_and(|writes| meta.format > writes),
        document,
        blobs: blobs(data, id),
    }
}

/// Writes one file of the caller's data and stamps its `meta.json` with the caller, keeping every
/// other plugin's data and the plugin's other files as they are. `None` deletes the file.
pub(crate) fn put(s: &mut Session, caller: &StorageCaller, path: &str, bytes: Option<&[u8]>) -> Result<()> {
    put_replacing(s, caller, path, bytes, None)
}

/// Replaces a file and optionally removes its old category, validating the final map atomically.
pub(crate) fn put_replacing(s: &mut Session, caller: &StorageCaller, path: &str, bytes: Option<&[u8]>, remove: Option<&str>) -> Result<()> {
    let key = format!("{}/{path}", caller.id);
    if !plugin_data::valid_key(&key) {
        return Err(crate::params::bad("storage", format!("`{path}` is not a plain relative name")));
    }
    let meta_key = format!("{}/{META}", caller.id);
    let meta = Meta { plugin: caller.id.clone(), name: caller.name.clone(), plugin_version: caller.version.clone(), format: caller.data_format };
    let meta = serde_json::to_vec(&meta).map_err(|e| EngineError::Other(e.to_string()))?;
    // Values are shared Arcs, so staging the final map does not copy existing file contents.
    let mut data = s.doc.plugin_data.clone();
    if let Some(remove) = remove {
        data.remove(&format!("{}/{remove}", caller.id));
    }
    match bytes {
        Some(bytes) => {
            data.insert(key, bytes.into());
        }
        None => {
            data.remove(&key);
        }
    }
    data.insert(meta_key, meta.into());
    if data.len() > plugin_data::MAX_FILES {
        return Err(limit(format!("a design keeps at most {} files of plugin data", plugin_data::MAX_FILES)));
    }
    let total: u64 = data.values().map(|bytes| bytes.len() as u64).sum();
    if total > MAX_TOTAL_BYTES {
        return Err(limit("plugin data in one design is at most 256 MiB, all plugins together"));
    }
    let own: u64 = blobs(&data, &caller.id).iter().map(|blob| blob.bytes).sum();
    if own > MAX_BLOB_BYTES {
        return Err(limit("a plugin keeps at most 256 MiB of files in one design"));
    }
    s.doc_mut().plugin_data = data;
    Ok(())
}

/// Writes the caller's main document, after checking its size.
pub(crate) fn put_document(s: &mut Session, caller: &StorageCaller, document: &Value) -> Result<u64> {
    let bytes = serde_json::to_vec(document).map_err(|e| crate::params::bad("storage.set", e.to_string()))?;
    if bytes.len() > MAX_DOCUMENT_BYTES {
        return Err(limit("a plugin's main document is at most 16 MiB"));
    }
    put(s, caller, DATA, Some(&bytes))?;
    Ok(bytes.len() as u64)
}

/// RFC 7396: merges `patch` into `target`.
pub(crate) fn merge_patch(target: &mut Value, patch: &Value) {
    let Value::Object(patch) = patch else {
        *target = patch.clone();
        return;
    };
    if !target.is_object() {
        *target = Value::Object(Default::default());
    }
    if let Value::Object(target) = target {
        for (key, value) in patch {
            if value.is_null() {
                target.remove(key);
            } else {
                merge_patch(target.entry(key.clone()).or_insert(Value::Null), value);
            }
        }
    }
}

/// The full name of a file: `blobs/<name>` or `cache/<name>`, whichever holds it.
pub fn blob_path(doc: &Document, id: &str, name: &str) -> Option<(String, bool)> {
    [(BLOBS, false), (CACHE, true)]
        .into_iter()
        .map(|(folder, cache)| (format!("{folder}{name}"), cache))
        .find(|(path, _)| doc.plugin_data.contains_key(&format!("{id}/{path}")))
}

/// Part of a plugin's file: the bytes read, whether it is a cache file, and the whole file's size.
#[derive(Debug, PartialEq, Eq)]
pub struct BlobBytes {
    pub data: Vec<u8>,
    pub cache: bool,
    pub bytes: u64,
}

/// `len` bytes of `bytes` from `offset`; none past the end.
fn part_of(bytes: &[u8], offset: u64, len: u32) -> Vec<u8> {
    let start = usize::try_from(offset).unwrap_or(usize::MAX).min(bytes.len());
    let end = start.saturating_add(len as usize).min(bytes.len());
    bytes.get(start..end).unwrap_or_default().to_vec()
}

fn check_len(cmd: &str, len: u32) -> Result<()> {
    if (1..=MAX_READ_BYTES).contains(&len) { Ok(()) } else { Err(crate::params::bad(cmd, format!("`len` must be 1 to {MAX_READ_BYTES}"))) }
}

/// Writes the running plugin's file `name` from its bytes, as they are (the `storage` interface's
/// `put-blob`): a cache file when `cache`; `append` adds them to its end. One name is one file, so
/// writing it as the other kind moves it, checked with every limit before anything changes. The
/// file's size.
pub(crate) fn write_blob(s: &mut Session, name: &str, data: &[u8], cache: bool, append: bool) -> Result<u64> {
    let caller = writer(s)?;
    let path = format!("{}{name}", if cache { CACHE } else { BLOBS });
    let mut bytes = if append {
        blob_path(&s.doc, &caller.id, name).and_then(|(path, _)| blob(&s.doc, &caller.id, &path)).map(<[u8]>::to_vec).unwrap_or_default()
    } else {
        Vec::new()
    };
    bytes.extend_from_slice(data);
    let other = format!("{}{name}", if cache { BLOBS } else { CACHE });
    put_replacing(s, &caller, &path, Some(&bytes), Some(&other))?;
    Ok(bytes.len() as u64)
}

/// Up to `len` bytes of a plugin's file `name` from `offset` (`get-blob`).
pub(crate) fn read_blob(s: &Session, plugin: Option<&str>, name: &str, offset: u64, len: u32) -> Result<BlobBytes> {
    const CMD: &str = "storage.get-blob";
    let id = reader(s, CMD, plugin)?;
    check_len(CMD, len)?;
    let (path, cache) = blob_path(&s.doc, &id, name).ok_or_else(|| crate::params::bad(CMD, format!("{id} keeps no file named {name}")))?;
    let bytes = blob(&s.doc, &id, &path).unwrap_or_default();
    Ok(BlobBytes { data: part_of(bytes, offset, len), cache, bytes: bytes.len() as u64 })
}

/// Up to `len` bytes of a plugin's main document, its saved JSON, from `offset`, and its whole
/// size; none when it has none (`get-document`).
pub(crate) fn read_document(s: &Session, plugin: Option<&str>, offset: u64, len: u32) -> Result<(Vec<u8>, u64)> {
    const CMD: &str = "storage.get-document";
    let id = reader(s, CMD, plugin)?;
    check_len(CMD, len)?;
    let bytes = blob(&s.doc, &id, DATA).unwrap_or_default();
    Ok((part_of(bytes, offset, len), bytes.len() as u64))
}

/// The bytes of one of a plugin's files, by its full name (`blobs/<name>` or `cache/<name>`).
pub fn blob<'a>(doc: &'a Document, id: &str, path: &str) -> Option<&'a [u8]> {
    plugin_data::file(&doc.plugin_data, id, path)
}

/// The installed plugin with this id, when there is one: its manifest alone (not its component, so
/// opening a design reads a few KiB per plugin). Reading it runs no code.
#[cfg(not(target_arch = "wasm32"))]
fn installed(s: &Session, id: &str) -> Option<craft_plugin::Manifest> {
    let dir = s.plugin_state.packages.get(id).map(std::path::PathBuf::from).or_else(|| s.plugin_state.plugins_dir().map(|dir| dir.join(id)))?;
    let bytes = solidcraft_io::vfs::read(&dir.join("plugin.json").to_string_lossy(), super::MAX_MANIFEST_BYTES).ok()?;
    super::parse_manifest(&bytes, id).ok()
}
#[cfg(target_arch = "wasm32")]
fn installed(_s: &Session, _id: &str) -> Option<craft_plugin::Manifest> {
    None
}

/// The plugins that keep data in the open design, and whether each can change it here.
pub fn list(s: &Session) -> Vec<StoredPlugin> {
    let data = &s.doc.plugin_data;
    plugin_data::plugins(data)
        .into_iter()
        .map(|id| {
            let meta = plugin_data::meta(data, &id);
            let manifest = installed(s, &id);
            let writes = manifest.as_ref().and_then(|m| m.runtime.as_ref()).and_then(|runtime| runtime.data_format);
            let status = match (&manifest, writes) {
                (None, _) => DataStatus::Missing,
                (Some(_), _) if s.disabled_plugins.contains(&id) => DataStatus::Disabled,
                (Some(_), None) => DataStatus::Older,
                (Some(_), Some(writes)) if meta.format > writes => DataStatus::Older,
                (Some(_), Some(writes)) if meta.format < writes => DataStatus::Migrate,
                (Some(_), Some(_)) => DataStatus::Ready,
            };
            let name = manifest.as_ref().map(|m| m.name.clone()).filter(|n| !n.is_empty()).unwrap_or_else(|| meta.name.clone());
            StoredPlugin {
                name: if name.is_empty() { id.clone() } else { name },
                plugin_version: meta.plugin_version,
                format: meta.format,
                installed_version: manifest.map(|m| m.version),
                read_only: !matches!(status, DataStatus::Ready | DataStatus::Migrate),
                status,
                bytes: plugin_bytes(data, &id),
                plugin: id,
            }
        })
        .collect()
}
