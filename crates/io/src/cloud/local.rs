//! Durable local captures, publication receipts, and the offline catalog.
use std::fs;
use std::path::{Path, PathBuf};

use solidcraft_cloud::{Error, PendingSave, Result, Snapshot, StagedSave};

pub struct LocalOutbox {
    root: PathBuf,
}

fn storage(e: impl std::fmt::Display) -> Error {
    Error::Storage(e.to_string())
}

impl LocalOutbox {
    /// Settings live beside connection queues, never in a shared checkout.
    pub(super) fn load_settings(&self) -> Result<solidcraft_cloud::Configuration> {
        let path = self.path("connections.json")?;
        if !path.exists() {
            return Ok(Default::default());
        }
        solidcraft_cloud::Configuration::read(&crate::vfs::read(&path.to_string_lossy(), solidcraft_cloud::MAX_SETTINGS_BYTES).map_err(storage)?)
    }

    pub(super) fn configure(&self, settings: &solidcraft_cloud::Settings) -> Result<()> {
        let lock = fs::OpenOptions::new().create(true).truncate(false).read(true).write(true).open(self.path("settings.lock")?).map_err(storage)?;
        lock.try_lock().map_err(|_| storage("connection settings are being changed by another process"))?;
        let bytes = self.load_settings()?.update(settings)?;
        crate::safe_write::write_atomic(&self.path("connections.json")?, &bytes, false).map_err(storage)
    }

    pub fn new(root: PathBuf) -> Result<Self> {
        crate::vfs::check(&root.to_string_lossy()).map_err(storage)?;
        fs::create_dir_all(&root).map_err(storage)?;
        Ok(Self { root: fs::canonicalize(root).map_err(storage)? })
    }

    fn path(&self, name: &str) -> Result<PathBuf> {
        solidcraft_cloud::check_path(name)?;
        let mut path = self.root.clone();
        for part in name.split('/') {
            path.push(part);
            if fs::symlink_metadata(&path).is_ok_and(|m| m.file_type().is_symlink()) {
                return Err(storage("outbox paths must not be symbolic links"));
            }
        }
        crate::vfs::check(&path.to_string_lossy()).map_err(storage)?;
        Ok(path)
    }

    fn read(&self, name: &str) -> Result<Vec<u8>> {
        let max = if name.starts_with("objects/") { solidcraft_cloud::CHUNK_BYTES } else { solidcraft_cloud::MAX_MANIFEST_BYTES };
        crate::vfs::read(&self.path(name)?.to_string_lossy(), max).map_err(storage)
    }

    fn store(&self, name: &str, bytes: &[u8]) -> Result<()> {
        let path = self.path(name)?;
        if path.exists() {
            if self.read(name)? != bytes {
                return Err(storage("existing outbox object differs; local copy retained"));
            }
            return Ok(());
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(storage)?;
        }
        crate::safe_write::write_atomic(&path, bytes, false).map_err(storage)
    }

    pub fn stage(&self, save: &StagedSave) -> Result<()> {
        save.validate()?;
        // Verify completeness before the first write, then persist the commit record last.
        for (path, bytes) in &save.objects {
            self.store(path, bytes)?;
        }
        self.store(&save.record_path, &save.record_bytes)
    }

    pub fn pending(&self) -> Result<Vec<PendingSave>> {
        let path = self.path("pending")?;
        if !path.exists() {
            return Ok(Vec::new());
        }
        let mut records = Vec::new();
        for entry in fs::read_dir(path).map_err(storage)? {
            let entry = entry.map_err(storage)?;
            if entry.path().extension().is_none_or(|e| e != "json") {
                continue;
            }
            if records.len() >= 10_000 {
                return Err(storage("too many queued saves"));
            }
            let name = entry.file_name().to_string_lossy().to_string();
            let record = PendingSave::read(&self.read(&format!("pending/{name}"))?)?;
            if name != format!("{}.json", record.id) {
                return Err(storage("pending save identifier mismatch"));
            }
            records.push(record);
        }
        PendingSave::ordered(records)
    }

    pub fn recover(&self, save: &PendingSave) -> Result<Snapshot> {
        save.recover(|p| self.read(p))
    }

    pub(super) fn recover_document(&self, save: &PendingSave) -> Result<Snapshot> {
        save.recover_checked(|p| self.read(p), super::check_document)
    }

    /// Call only after the repository adapter confirms this captured save was published.
    pub fn published(&self, save: &PendingSave) -> Result<()> {
        PendingSave::read(&serde_json::to_vec(save).map_err(storage)?)?;
        let path = self.path(&format!("pending/{}.json", save.id))?;
        if !path.exists() {
            return Ok(());
        }
        let current = PendingSave::read(&crate::vfs::read(&path.to_string_lossy(), solidcraft_cloud::MAX_MANIFEST_BYTES).map_err(storage)?)?;
        if current.manifest.document_hash != save.manifest.document_hash {
            return Err(Error::Conflict);
        }
        fs::remove_file(path).map_err(storage)
    }

    /// Local process coordination only; this is not a collaborative file lock.
    pub(super) fn synchronization_lock(&self) -> Result<fs::File> {
        let lock = fs::OpenOptions::new().create(true).truncate(false).read(true).write(true).open(self.path("sync.lock")?).map_err(storage)?;
        lock.try_lock().map_err(|_| storage("this connection is already synchronizing; saved copies are retained"))?;
        Ok(lock)
    }

    pub fn root(&self) -> &Path {
        &self.root
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    #[test]
    fn local_save_survives_reopening_until_publication_is_confirmed() {
        let root = std::env::temp_dir().join(format!(
            "solidcraft-outbox-{}-{}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        let save = PendingSave::stage(
            &Default::default(),
            "project",
            "parts/design",
            None,
            Snapshot::prepare(b"{}".to_vec(), BTreeMap::from([("blob".into(), vec![42; 100])])).unwrap(),
        )
        .unwrap();
        LocalOutbox::new(root.clone()).unwrap().stage(&save).unwrap();
        let reopened = LocalOutbox::new(root.clone()).unwrap();
        let pending = reopened.pending().unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(reopened.recover(&pending[0]).unwrap().document, b"{}");
        // A failed or skipped upload leaves the queued record. Retrying stage is idempotent.
        reopened.stage(&save).unwrap();
        assert_eq!(reopened.pending().unwrap().len(), 1);
        reopened.published(&pending[0]).unwrap();
        assert!(reopened.pending().unwrap().is_empty());
        assert!(reopened.recover(&pending[0]).is_ok());
        fs::remove_dir_all(root).unwrap();
    }
}
