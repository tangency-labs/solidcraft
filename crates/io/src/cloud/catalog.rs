//! Machine-local confirmed captures. Cache first, immutable receipt second, queue removal
//! last: every interrupted boundary can be retried without consulting a mutable editor.
use std::fs;

use serde::{Deserialize, Serialize};
use solidcraft_cloud::{Connection, Destination, Error, PendingSave, Published, Result, digest};

use super::LocalOutbox;

const FORMAT: &str = "solidcraft-native-publication/1";
const MAX_ENTRIES: usize = 10_000;
const MAX_CATALOG_BYTES: usize = 128 * 1024 * 1024;

fn storage(error: impl std::fmt::Display) -> Error {
    Error::Storage(error.to_string())
}

#[derive(Serialize, Deserialize)]
pub(super) struct CachedPublication {
    format: String,
    pub save: PendingSave,
    pub published: Published,
}

impl CachedPublication {
    fn read(bytes: &[u8]) -> Result<Self> {
        if bytes.len() > solidcraft_cloud::MAX_MANIFEST_BYTES {
            return Err(storage("publication record too large"));
        }
        let value: serde_json::Value = serde_json::from_slice(bytes).map_err(storage)?;
        if value.get("format").and_then(serde_json::Value::as_str) != Some(FORMAT) {
            return Err(Error::UpgradeRequired("native publication record format".into()));
        }
        let save = value.get("save").ok_or_else(|| storage("publication capture missing"))?;
        let save = PendingSave::read(&serde_json::to_vec(save).map_err(storage)?)?;
        let entry: Self = serde_json::from_value(value).map_err(storage)?;
        entry.published.destination.validate()?;
        let mut destination = save.destination;
        if entry.published.conflict {
            destination.file = format!("conflicts/{}-{}", destination.file, save.id.get(..12).ok_or_else(|| storage("invalid revision"))?);
        }
        if entry.published.revision != save.id
            || entry.published.destination != destination
            || !matches!(entry.published.commit.len(), 40 | 64)
            || !entry.published.commit.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            return Err(storage("publication acknowledgement mismatch"));
        }
        Ok(entry)
    }

    fn matches(&self, save: &PendingSave) -> Result<()> {
        if serde_json::to_value(&self.save).map_err(storage)? != serde_json::to_value(save).map_err(storage)? {
            return Err(storage("publication capture identifier mismatch"));
        }
        Ok(())
    }
}

fn cache_path(destination: &Destination) -> Result<String> {
    destination.validate()?;
    Ok(format!("cached/{}.json", digest(&serde_json::to_vec(destination).map_err(storage)?)))
}

impl LocalOutbox {
    fn publication(&self, path: &str) -> Result<Option<CachedPublication>> {
        if !self.path(path)?.exists() {
            return Ok(None);
        }
        CachedPublication::read(&self.read(path)?).map(Some)
    }

    pub(super) fn receipt(&self, save: &PendingSave) -> Result<Option<Published>> {
        PendingSave::read(&serde_json::to_vec(save).map_err(storage)?)?;
        let entry = self.publication(&format!("receipts/{}.json", save.id))?;
        if let Some(entry) = &entry {
            entry.matches(save)?;
        }
        Ok(entry.map(|entry| entry.published))
    }

    pub(super) fn cached(&self, connection: &Connection, destination: &Destination) -> Result<Option<CachedPublication>> {
        let entry = self.publication(&cache_path(destination)?)?;
        if let Some(entry) = &entry {
            entry.save.check_connection(connection)?;
            if &entry.published.destination != destination {
                return Err(storage("cached file identifier mismatch"));
            }
        }
        Ok(entry)
    }

    pub(super) fn cached_destinations(&self, connection: &Connection) -> Result<Vec<Destination>> {
        let path = self.path("cached")?;
        if !path.exists() {
            return Ok(Vec::new());
        }
        let mut destinations = Vec::new();
        let mut total = 0usize;
        for entry in fs::read_dir(path).map_err(storage)? {
            let entry = entry.map_err(storage)?;
            if entry.path().extension().is_none_or(|extension| extension != "json") {
                continue;
            }
            if destinations.len() >= MAX_ENTRIES {
                return Err(storage("too many cached files"));
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            let path = format!("cached/{name}");
            let bytes = self.read(&path)?;
            total = total.checked_add(bytes.len()).ok_or_else(|| storage("cached catalog too large"))?;
            if total > MAX_CATALOG_BYTES {
                return Err(storage("cached catalog too large"));
            }
            let entry = CachedPublication::read(&bytes)?;
            entry.save.check_connection(connection)?;
            if cache_path(&entry.published.destination)? != path {
                return Err(storage("cached file identifier mismatch"));
            }
            destinations.push(entry.published.destination);
        }
        Ok(destinations)
    }

    /// Call only with a repository-confirmed publication. An existing receipt recognizes
    /// retries even after a newer revision has replaced this destination's cached head.
    pub fn published(&self, save: &PendingSave, published: &Published) -> Result<()> {
        let bytes = serde_json::to_vec_pretty(&serde_json::json!({"format": FORMAT, "save": save, "published": published})).map_err(storage)?;
        CachedPublication::read(&bytes)?;
        let _lock = self.process_lock("catalog.lock", "this connection's catalog is being updated; saved copies are retained")?;
        let receipt = self.receipt(save)?;
        if let Some(receipt) = &receipt
            && serde_json::to_value(receipt).map_err(storage)? != serde_json::to_value(published).map_err(storage)?
        {
            return Err(Error::Conflict);
        }
        let cached = self.cached(&save.connection, &published.destination)?;
        // Preflight before replacing metadata: a newer cached document must never be hidden
        // by a supported envelope around an older schema, or by a corrupt/missing document.
        self.check_captured_document(save)?;
        if let Some(cached) = &cached {
            self.check_captured_document(&cached.save)?;
        }
        // Validate the queue record before changing either catalog file.
        let pending = format!("pending/{}.json", save.id);
        if self.path(&pending)?.exists() {
            let current = PendingSave::read(&self.read(&pending)?)?;
            if serde_json::to_value(&current).map_err(storage)? != serde_json::to_value(save).map_err(storage)? {
                return Err(Error::Conflict);
            }
        }
        if receipt.is_none() || cached.is_none() {
            let path = self.path(&cache_path(&published.destination)?)?;
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).map_err(storage)?;
            }
            crate::safe_write::write_atomic(&path, &bytes, false).map_err(storage)?;
        }
        if receipt.is_none() {
            self.store(&format!("receipts/{}.json", save.id), &bytes)?;
        }
        self.remove_pending(save)
    }


    fn check_captured_document(&self, save: &PendingSave) -> Result<()> {
        let bytes = self.read(&format!("documents/{}", save.manifest.document_hash))?;
        if digest(&bytes) != save.manifest.document_hash {
            return Err(storage("cached document hash mismatch"));
        }
        super::check_document(&bytes)
    }
}
