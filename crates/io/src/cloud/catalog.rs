//! Machine-local confirmed captures. Cache first, immutable receipt second, queue removal
//! last: every interrupted boundary can be retried without consulting a mutable editor.
use std::fs;

use serde::{Deserialize, Serialize};
use solidcraft_cloud::{CachedEntry, Connection, Destination, Error, PendingSave, Published, Result, Snapshot, digest};

use super::LocalOutbox;

const FORMAT: &str = "solidcraft-native-publication/1";
const ROUTED_FORMAT: &str = "solidcraft-native-publication/2";
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    parent: Option<Published>,
}

impl CachedPublication {
    fn read(bytes: &[u8]) -> Result<Self> {
        if bytes.len() > solidcraft_cloud::MAX_MANIFEST_BYTES {
            return Err(storage("publication record too large"));
        }
        let value: serde_json::Value = serde_json::from_slice(bytes).map_err(storage)?;
        if !matches!(value.get("format").and_then(serde_json::Value::as_str), Some(FORMAT | ROUTED_FORMAT)) {
            return Err(Error::UpgradeRequired("native publication record format".into()));
        }
        let save = value.get("save").ok_or_else(|| storage("publication capture missing"))?;
        let save = PendingSave::read(&serde_json::to_vec(save).map_err(storage)?)?;
        let entry: Self = serde_json::from_value(value).map_err(storage)?;
        if entry.format == FORMAT && entry.parent.is_some() {
            return Err(storage("publication acknowledgement mismatch"));
        }
        entry.published.validate(&save, entry.parent.as_ref())?;
        Ok(entry)
    }

    fn matches(&self, save: &PendingSave) -> Result<()> {
        if serde_json::to_value(&self.save).map_err(storage)? != serde_json::to_value(save).map_err(storage)? {
            return Err(storage("publication capture identifier mismatch"));
        }
        Ok(())
    }
}

fn read_cache(bytes: &[u8]) -> Result<CachedEntry> {
    let text = std::str::from_utf8(bytes).map_err(storage)?;
    // Existing publication caches remain readable. Receipts keep their strict original
    // envelope; new read-only caches use the shared browser/desktop cache envelope.
    let value: serde_json::Value = serde_json::from_str(text).map_err(storage)?;
    if matches!(value.get("format").and_then(serde_json::Value::as_str), Some(FORMAT | ROUTED_FORMAT)) {
        let entry = CachedPublication::read(bytes)?;
        let mut cached = CachedEntry::new(entry.save, entry.published.destination, Some(entry.published.revision), true)?;
        cached.conflict = entry.published.conflict;
        return Ok(cached);
    }
    CachedEntry::read(text)
}

fn cache_path(destination: &Destination) -> Result<String> {
    destination.validate()?;
    Ok(format!("cached/{}.json", digest(&serde_json::to_vec(destination).map_err(storage)?)))
}

impl LocalOutbox {
    pub(super) fn discard_management(&self, save: &PendingSave) -> Result<()> {
        if save.manifest.change()?.is_none() {
            return Err(storage("ordinary saved edits cannot be discarded as management"));
        }
        let _lock = self.process_lock("catalog.lock", "this connection's catalog is being updated")?;
        if self.receipt(save)?.is_some() {
            return Err(storage("this action is confirmed; synchronize before managing it again"));
        }
        let pending = format!("pending/{}.json", save.id);
        let current = PendingSave::read(&self.read(&pending)?)?;
        if serde_json::to_vec(&current).map_err(storage)? != serde_json::to_vec(save).map_err(storage)? {
            return Err(Error::Conflict);
        }
        if let Some(entry) = self.cached(&save.connection, &save.destination)? {
            self.check_captured_document(&entry.save)?;
            if entry.save.id == save.id {
                fs::remove_file(self.path(&cache_path(&save.destination)?)?).map_err(storage)?;
            }
        }
        self.remove_pending(save)
    }
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

    pub(super) fn published_parent(&self, save: &PendingSave) -> Result<Option<Published>> {
        let Some(id) = &save.expected_revision else { return Ok(None) };
        // External commit identifiers need not be local captured revisions.
        if id.len() != 64 || !id.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Ok(None);
        }
        let Some(entry) = self.publication(&format!("receipts/{id}.json"))? else { return Ok(None) };
        entry.save.check_connection(&save.connection)?;
        if entry.save.id != *id || (entry.save.destination != save.destination && entry.published.destination != save.destination) {
            return Err(storage("publication parent belongs to another design"));
        }
        Published::target(save, Some(&entry.published))?;
        Ok(Some(entry.published))
    }

    pub(super) fn cached(&self, connection: &Connection, destination: &Destination) -> Result<Option<CachedEntry>> {
        let path = cache_path(destination)?;
        if !self.path(&path)?.exists() {
            return Ok(None);
        }
        let entry = Some(read_cache(&self.read(&path)?)?);
        if let Some(entry) = &entry {
            entry.save.check_connection(connection)?;
            if &entry.destination != destination {
                return Err(storage("cached file identifier mismatch"));
            }
        }
        Ok(entry)
    }

    pub(super) fn cached_entries(&self, connection: &Connection) -> Result<Vec<CachedEntry>> {
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
            let entry = read_cache(&bytes)?;
            entry.save.check_connection(connection)?;
            if cache_path(&entry.destination)? != path {
                return Err(storage("cached file identifier mismatch"));
            }
            destinations.push(entry);
        }
        Ok(destinations)
    }

    #[cfg(test)]
    pub(super) fn cached_destinations(&self, connection: &Connection) -> Result<Vec<Destination>> {
        Ok(self.cached_entries(connection)?.into_iter().map(|e| e.destination).collect())
    }

    /// Call only with a repository-confirmed publication. An existing receipt recognizes
    /// retries even after a newer revision has replaced this destination's cached head.
    pub fn published(&self, save: &PendingSave, published: &Published) -> Result<()> {
        let parent = self.published_parent(save)?;
        let redirected = Published::target(save, parent.as_ref())? != save.destination;
        let bytes = serde_json::to_vec_pretty(&serde_json::json!({
            "format": if redirected { ROUTED_FORMAT } else { FORMAT },
            "save": save, "published": published, "parent": if redirected { parent } else { None }
        }))
        .map_err(storage)?;
        CachedPublication::read(&bytes)?;
        let _lock = self.process_lock("catalog.lock", "this connection's catalog is being updated; saved copies are retained")?;
        let moved_source = if matches!(save.manifest.change()?, Some(solidcraft_cloud::management::Change::Rename { .. })) {
            let source = self.cached(&save.connection, &save.destination)?;
            if let Some(entry) = &source {
                self.check_captured_document(&entry.save)?;
            }
            source
                .filter(|entry| entry.revision == save.expected_revision || entry.save.id == save.id)
                .map(|_| cache_path(&save.destination))
                .transpose()?
        } else {
            None
        };
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
        if let Some(entries) = solidcraft_cloud::conflicts::confirmed(save, published)? {
            for entry in &entries {
                self.check_captured_document(&entry.save)?;
                if let Some(current) = self.cached(&save.connection, &entry.destination)? {
                    self.check_captured_document(&current.save)?;
                }
            }
            for entry in entries {
                let current = self.cached(&save.connection, &entry.destination)?;
                if current.as_ref().map(|c| solidcraft_cloud::conflicts::may_replace(save, c)).transpose()?.unwrap_or(true) {
                    let path = self.path(&cache_path(&entry.destination)?)?;
                    if let Some(parent) = path.parent() {
                        fs::create_dir_all(parent).map_err(storage)?;
                    }
                    crate::safe_write::write_atomic(&path, entry.write()?.as_bytes(), false).map_err(storage)?;
                }
            }
        } else if receipt.is_none() || cached.is_none() {
            let path = self.path(&cache_path(&published.destination)?)?;
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).map_err(storage)?;
            }
            let mut cached = CachedEntry::new(save.clone(), published.destination.clone(), Some(published.revision.clone()), true)?;
            cached.conflict = published.conflict;
            crate::safe_write::write_atomic(&path, cached.write()?.as_bytes(), false).map_err(storage)?;
        }
        if receipt.is_none() {
            self.store(&format!("receipts/{}.json", save.id), &bytes)?;
        }
        if let Some(path) = moved_source {
            let path = self.path(&path)?;
            if path.exists() {
                fs::remove_file(path).map_err(storage)?;
            }
        }
        self.remove_pending(save)
    }

    pub(super) fn mark_outdated(&self, connection: &Connection, observed: &solidcraft_cloud::StoredDesign) -> Result<()> {
        let _lock = self.process_lock("catalog.lock", "this connection's catalog is being updated; saved copies are retained")?;
        if let Some(mut current) = self.cached(connection, &observed.destination)?
            && current.mark_outdated(observed)?
        {
            crate::safe_write::write_atomic(&self.path(&cache_path(&observed.destination)?)?, current.write()?.as_bytes(), false).map_err(storage)?;
        }
        Ok(())
    }

    pub(super) fn cache_synchronized_checkout(&self, connection: &Connection, destination: &Destination, snapshot: &Snapshot) -> Result<()> {
        self.cache_snapshot(connection, destination, snapshot, true)
    }

    /// Retain a verified checkout snapshot without queuing it or inventing a publication.
    /// The caller holds the checkout lock across its read and this cache update.
    pub(super) fn cache_checkout(&self, connection: &Connection, destination: &Destination, snapshot: &Snapshot) -> Result<()> {
        self.cache_snapshot(connection, destination, snapshot, false)
    }

    fn cache_snapshot(&self, connection: &Connection, destination: &Destination, snapshot: &Snapshot, synchronized: bool) -> Result<()> {
        let revision = solidcraft_cloud::revision(&snapshot.manifest).map(str::to_owned);
        let save = PendingSave::describe(connection, &destination.project, &destination.file, revision.clone(), snapshot.manifest.clone())?;
        super::check_document(&snapshot.document)?;
        if digest(&snapshot.document) != save.manifest.document_hash {
            return Err(storage("document hash mismatch"));
        }
        solidcraft_cloud::Snapshot::hydrate(&snapshot.manifest, &snapshot.document, |path| {
            snapshot.objects.get(path).cloned().ok_or_else(|| storage("asset missing"))
        })?;
        let _lock = self.process_lock("catalog.lock", "this connection's catalog is being updated; saved copies are retained")?;
        let current = self.cached(connection, destination)?;
        if let Some(current) = &current {
            self.check_captured_document(&current.save)?;
        }
        // Reading cannot replace a recoverable local edit, including one captured by
        // another tab while the checkout snapshot was being read.
        for pending in self.pending()?.into_iter().filter(|p| &p.destination == destination) {
            pending.check_connection(connection)?;
            if self.receipt(&pending)?.is_none() {
                return Ok(());
            }
        }
        let confirmed = synchronized
            || match &current {
                Some(c) => {
                    c.confirmed
                        && !c.outdated
                        && c.revision == revision
                        && solidcraft_cloud::manifest_digest(&c.save.manifest)? == solidcraft_cloud::manifest_digest(&save.manifest)?
                }
                None => false,
            };
        for (path, bytes) in &snapshot.objects {
            self.store(path, bytes)?;
        }
        self.store(&format!("documents/{}", save.manifest.document_hash), &snapshot.document)?;
        let mut entry = CachedEntry::new(save, destination.clone(), revision, confirmed)?;
        entry.conflict = current.as_ref().is_some_and(|c| c.conflict);
        let path = self.path(&cache_path(destination)?)?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(storage)?;
        }
        crate::safe_write::write_atomic(&path, entry.write()?.as_bytes(), false).map_err(storage)
    }

    fn check_captured_document(&self, save: &PendingSave) -> Result<()> {
        let bytes = self.read(&format!("documents/{}", save.manifest.document_hash))?;
        if digest(&bytes) != save.manifest.document_hash {
            return Err(storage("cached document hash mismatch"));
        }
        super::check_document(&bytes)
    }
}
