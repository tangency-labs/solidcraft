//! Per-user plugin settings, outside every design. One bounded JSON map per plugin id.
//! The host stages a run's changes and publishes them only on a successful transaction.

use super::{HostError, valid_id};
use craft_plugin::{MAX_USER_STORAGE_BYTES, Result, check_user_storage};
use serde_json::Value;
use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};

pub type Data = BTreeMap<String, Value>;

/// Same native settings directory as the desktop's preferences, including its isolation override.
/// The CLI and headless MCP use this too; a plugins-folder override does not move preferences.
pub fn settings_dir() -> Option<PathBuf> {
    if cfg!(target_arch = "wasm32") {
        return None;
    }
    if let Some(path) = std::env::var_os("SOLIDCRAFT_SETTINGS_DIR") {
        let path = PathBuf::from(path);
        return path.is_absolute().then_some(path);
    }
    if cfg!(windows) {
        std::env::var_os("APPDATA").map(|p| PathBuf::from(p).join("SolidCraft/data"))
    } else if cfg!(target_os = "macos") {
        std::env::home_dir().map(|p| p.join("Library/Application Support/SolidCraft"))
    } else {
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .or_else(|| std::env::home_dir().map(|p| p.join(".local/share")))
            .map(|p| p.join("solidcraft"))
    }
}

fn invalid(reason: impl std::fmt::Display) -> HostError {
    HostError::InvalidInput { field: "user-storage".into(), reason: reason.to_string() }
}
fn path(settings: &Path, id: &str) -> Result<PathBuf> {
    if !valid_id(id) || !settings.is_absolute() {
        return Err(invalid("invalid plugin id or settings directory"));
    }
    let folder = settings.join("plugin-user-data");
    match std::fs::symlink_metadata(&folder) {
        Ok(meta) if !meta.is_dir() || meta.file_type().is_symlink() => return Err(invalid("user data folder must be a directory, not a link")),
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(invalid(e)),
        _ => {}
    }
    Ok(folder.join(format!("{id}.json")))
}

/// Load only the host-selected plugin's map. Never read unbounded data or follow a record link.
pub fn load(settings: &Path, id: &str) -> Result<Data> {
    let path = path(settings, id)?;
    let meta = match std::fs::symlink_metadata(&path) {
        Ok(meta) => meta,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Data::new()),
        Err(e) => return Err(invalid(e)),
    };
    if !meta.is_file() || meta.file_type().is_symlink() {
        return Err(invalid("user data must be a regular file, not a link"));
    }
    let file = std::fs::File::open(path).map_err(invalid)?;
    if !file.metadata().map_err(invalid)?.is_file() {
        return Err(invalid("user data must be a regular file"));
    }
    let mut bytes = Vec::new();
    file.take(MAX_USER_STORAGE_BYTES as u64 + 1).read_to_end(&mut bytes).map_err(invalid)?;
    if bytes.len() > MAX_USER_STORAGE_BYTES {
        return Err(HostError::LimitExceeded { limit: "user-storage total bytes (65536)".into() });
    }
    let data: Data = serde_json::from_slice(&bytes).map_err(invalid)?;
    check_user_storage(&data)?;
    Ok(data)
}

/// Publish a whole map in one atomic rename; no design or undo history is involved.
pub fn save(settings: &Path, id: &str, data: &Data) -> Result<()> {
    check_user_storage(data)?;
    let path = path(settings, id)?;
    let folder = path.parent().ok_or_else(|| invalid("missing user data folder"))?;
    std::fs::create_dir_all(folder).map_err(invalid)?;
    let bytes = serde_json::to_vec(data).map_err(invalid)?;
    solidcraft_io::safe_write::write_atomic_no_follow(&path, &bytes).map_err(invalid)
}

/// Explicit uninstall choice. An absent record is already removed; other failures are reported.
pub fn remove(settings: &Path, id: &str) -> Result<()> {
    let path = path(settings, id)?;
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(invalid(e)),
    }
}

/// Lazy, per-call staging kept only in the host's scratch session.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Pending {
    data: Data,
    dirty: bool,
}
impl Pending {
    pub fn load(settings: &Path, id: &str) -> Result<Self> {
        Ok(Self { data: load(settings, id)?, dirty: false })
    }
    pub fn get(&self, key: &str) -> Option<Value> {
        self.data.get(key).cloned()
    }
    pub fn list(&self) -> Vec<String> {
        self.data.keys().cloned().collect()
    }
    pub fn set(&mut self, key: &str, value: &Value) -> Result<()> {
        let mut data = self.data.clone();
        data.insert(key.into(), value.clone());
        check_user_storage(&data)?;
        self.dirty |= data != self.data;
        self.data = data;
        Ok(())
    }
    pub fn remove(&mut self, key: &str) -> bool {
        let removed = self.data.remove(key).is_some();
        self.dirty |= removed;
        removed
    }
    pub fn publish(&self, settings: &Path, id: &str) -> Result<()> {
        if self.dirty { save(settings, id, &self.data) } else { Ok(()) }
    }
}
