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

/// Process coordination ends when the guard drops, including if a concurrent spawn
/// still owns a duplicated descriptor. Closing the parent descriptor alone is insufficient.
pub(super) struct ProcessLock(fs::File);

impl ProcessLock {
    pub(super) fn acquire(path: &Path, busy: &str) -> Result<Self> {
        let file = fs::OpenOptions::new().create(true).truncate(false).read(true).write(true).open(path).map_err(storage)?;
        file.try_lock().map_err(|_| storage(busy))?;
        Ok(Self(file))
    }

    fn wait(path: &Path, job: &solidcraft_cloud::jobs::Job, timeout: std::time::Duration) -> Result<Self> {
        let file = fs::OpenOptions::new().create(true).truncate(false).read(true).write(true).open(path).map_err(storage)?;
        let started = std::time::Instant::now();
        loop {
            if job.cancelled()? {
                return Err(storage("operation cancelled; durable saves retained"));
            }
            match file.try_lock() {
                Ok(()) => return Ok(Self(file)),
                Err(fs::TryLockError::Error(error)) => return Err(storage(error)),
                Err(fs::TryLockError::WouldBlock) => {
                    if started.elapsed() >= timeout {
                        return Err(storage("waiting for the active synchronization timed out; saved copies are retained for retry"));
                    }
                    std::thread::sleep(std::time::Duration::from_millis(20));
                }
            }
        }
    }
}

impl Drop for ProcessLock {
    fn drop(&mut self) {
        let _ = self.0.unlock();
    }
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
        let _lock = self.process_lock("settings.lock", "connection settings are being changed by another process")?;
        let bytes = self.load_settings()?.update(settings)?;
        crate::safe_write::write_atomic(&self.path("connections.json")?, &bytes, false).map_err(storage)
    }

    pub fn new(root: PathBuf) -> Result<Self> {
        crate::vfs::check(&root.to_string_lossy()).map_err(storage)?;
        fs::create_dir_all(&root).map_err(storage)?;
        Ok(Self { root: fs::canonicalize(root).map_err(storage)? })
    }

    pub(super) fn path(&self, name: &str) -> Result<PathBuf> {
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

    pub(super) fn read(&self, name: &str) -> Result<Vec<u8>> {
        let max = if name.starts_with("objects/") { solidcraft_cloud::CHUNK_BYTES } else { solidcraft_cloud::MAX_MANIFEST_BYTES };
        crate::vfs::read(&self.path(name)?.to_string_lossy(), max).map_err(storage)
    }

    pub(super) fn store(&self, name: &str, bytes: &[u8]) -> Result<()> {
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
        self.pending_bounded(128 << 20)
    }

    fn pending_bounded(&self, limit: usize) -> Result<Vec<PendingSave>> {
        let path = self.path("pending")?;
        if !path.exists() {
            return Ok(Vec::new());
        }
        let mut records = Vec::new();
        let mut metadata_bytes = 0usize;
        for entry in fs::read_dir(path).map_err(storage)? {
            let entry = entry.map_err(storage)?;
            if entry.path().extension().is_none_or(|e| e != "json") {
                continue;
            }
            if records.len() >= 10_000 {
                return Err(storage("too many queued saves"));
            }
            let name = entry.file_name().to_string_lossy().to_string();
            let bytes = self.read(&format!("pending/{name}"))?;
            metadata_bytes = metadata_bytes.checked_add(bytes.len()).ok_or_else(|| storage("queued metadata too large"))?;
            if metadata_bytes > limit {
                return Err(storage("queued metadata too large"));
            }
            let record = PendingSave::read(&bytes)?;
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

    /// The catalog adapter calls this only after its cache and receipt reach disk.
    pub(super) fn remove_pending(&self, save: &PendingSave) -> Result<()> {
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
    pub(super) fn synchronization_lock(&self) -> Result<ProcessLock> {
        self.process_lock("sync.lock", "this connection is already synchronizing; saved copies are retained")
    }

    /// Publication waits off the UI thread. A Save has already persisted its snapshot.
    pub(super) fn wait_for_synchronization(&self, job: &solidcraft_cloud::jobs::Job) -> Result<ProcessLock> {
        ProcessLock::wait(&self.path("sync.lock")?, job, std::time::Duration::from_secs(300))
    }

    pub(super) fn process_lock(&self, name: &str, busy: &str) -> Result<ProcessLock> {
        ProcessLock::acquire(&self.path(name)?, busy)
    }

    pub fn root(&self) -> &Path {
        &self.root
    }
}

impl solidcraft_cloud::DownloadCache for LocalOutbox {
    async fn load(&self, path: &str) -> Result<Option<Vec<u8>>> {
        if !path.starts_with("objects/") && !path.starts_with("checks/") {
            return Err(storage("invalid download object path"));
        }
        if !self.path(path)?.exists() {
            return Ok(None);
        }
        self.read(path).map(Some)
    }
    async fn store(&self, path: &str, bytes: &[u8]) -> Result<()> {
        if !path.starts_with("objects/") && !path.starts_with("checks/") || bytes.len() > solidcraft_cloud::CHUNK_BYTES {
            return Err(storage("invalid download object"));
        }
        if path.starts_with("checks/") {
            let target = self.path(path)?;
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent).map_err(storage)?;
            }
            return crate::safe_write::write_atomic(&target, bytes, false).map_err(storage);
        }
        self.store(path, bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn publication_wait_is_bounded_and_cancellable() {
        let fixture = Fixture::new();
        let outbox = fixture.outbox();
        let held = outbox.synchronization_lock().unwrap();
        let job = solidcraft_cloud::jobs::Job::new("waiting".into()).unwrap();
        let path = outbox.path("sync.lock").unwrap();
        let timeout = ProcessLock::wait(&path, &job, std::time::Duration::ZERO).err().unwrap();
        assert!(timeout.to_string().contains("timed out"));
        job.cancel().unwrap();
        let cancelled = ProcessLock::wait(&path, &job, std::time::Duration::from_secs(300)).err().unwrap();
        assert!(cancelled.to_string().contains("cancelled"));
        drop(held);
        let ready = solidcraft_cloud::jobs::Job::new("ready".into()).unwrap();
        let _lock = ProcessLock::wait(&path, &ready, std::time::Duration::ZERO).unwrap();
    }

    #[test]
    fn synchronization_lock_releases_even_while_an_inherited_descriptor_is_still_open() {
        let fixture = Fixture::new();
        let outbox = fixture.outbox();
        let held = outbox.synchronization_lock().unwrap();
        // A concurrent Git spawn can inherit this descriptor until exec closes it.
        let inherited = held.0.try_clone().unwrap();
        assert!(outbox.synchronization_lock().is_err());
        drop(held);
        let next = outbox.synchronization_lock().unwrap();
        drop(next);
        drop(inherited);
    }

    #[test]
    fn metadata_locks_release_even_while_an_inherited_descriptor_is_still_open() {
        let fixture = Fixture::new();
        let outbox = fixture.outbox();
        for name in ["catalog.lock", "settings.lock"] {
            let held = outbox.process_lock(name, "busy").unwrap();
            let inherited = held.0.try_clone().unwrap();
            assert!(outbox.process_lock(name, "busy").is_err());
            drop(held);
            let next = outbox.process_lock(name, "busy").unwrap();
            drop(next);
            drop(inherited);
        }
    }

    struct Fixture(PathBuf);

    impl Fixture {
        fn new() -> Self {
            Self(std::env::temp_dir().join(format!(
                "solidcraft-outbox-{}-{}",
                std::process::id(),
                std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
            )))
        }

        fn outbox(&self) -> LocalOutbox {
            LocalOutbox::new(self.0.clone()).unwrap()
        }

        fn save(&self, name: &str, parent: Option<String>) -> StagedSave {
            let save = PendingSave::stage(
                &Default::default(),
                "project",
                "parts/design",
                parent,
                super::super::prepare_document(&solidcraft_doc::Document::new(name)).unwrap(),
            )
            .unwrap();
            self.outbox().stage(&save).unwrap();
            save
        }

        fn publication(save: &PendingSave) -> solidcraft_cloud::Published {
            solidcraft_cloud::Published { destination: save.destination.clone(), revision: save.id.clone(), commit: "a".repeat(40), conflict: false }
        }

        fn catalog_files(&self, folder: &str) -> Vec<PathBuf> {
            fs::read_dir(self.0.join(folder)).unwrap().map(|entry| entry.unwrap().path()).collect()
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn queued_metadata_limit_bounds_the_total_before_reading_all_records() {
        let fixture = Fixture::new();
        let first = fixture.save("one", None);
        let outbox = fixture.outbox();
        assert_eq!(outbox.pending_bounded(first.record_bytes.len() + 1).unwrap().len(), 1);
        let second = fixture.save("two", Some(first.record.id.clone()));
        let limit = first.record_bytes.len() + second.record_bytes.len() - 1;
        assert!(outbox.pending_bounded(limit).unwrap_err().to_string().contains("metadata too large"));
        assert_eq!(outbox.pending().unwrap().len(), 2);
    }

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
            super::super::prepare_document(&solidcraft_doc::Document::new("Local capture")).unwrap(),
        )
        .unwrap();
        LocalOutbox::new(root.clone()).unwrap().stage(&save).unwrap();
        let reopened = LocalOutbox::new(root.clone()).unwrap();
        let pending = reopened.pending().unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(reopened.recover(&pending[0]).unwrap().document, save.objects[&format!("documents/{}", pending[0].manifest.document_hash)]);
        // A failed or skipped upload leaves the queued record. Retrying stage is idempotent.
        reopened.stage(&save).unwrap();
        assert_eq!(reopened.pending().unwrap().len(), 1);
        reopened
            .published(
                &pending[0],
                &solidcraft_cloud::Published {
                    destination: pending[0].destination.clone(),
                    revision: pending[0].id.clone(),
                    commit: "a".repeat(40),
                    conflict: false,
                },
            )
            .unwrap();
        assert!(reopened.pending().unwrap().is_empty());
        assert!(reopened.recover(&pending[0]).is_ok());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn confirmation_recovers_each_interrupted_metadata_boundary_and_preserves_optional_receipts() {
        for boundary in ["cache", "receipt", "removed"] {
            let fixture = Fixture::new();
            let staged = fixture.save("Confirmed design", None);
            let publication = Fixture::publication(&staged.record);
            let outbox = fixture.outbox();
            outbox.published(&staged.record, &publication).unwrap();
            if boundary != "removed" {
                outbox.stage(&staged).unwrap();
            }
            let receipt = fixture.catalog_files("receipts").pop().unwrap();
            if boundary == "cache" {
                fs::remove_file(&receipt).unwrap();
            } else {
                let mut value: serde_json::Value = serde_json::from_slice(&fs::read(&receipt).unwrap()).unwrap();
                value["optional_annotation"] = serde_json::json!("keep on retry");
                fs::write(&receipt, serde_json::to_vec(&value).unwrap()).unwrap();
            }
            drop(outbox);
            let reopened = fixture.outbox();
            let cached = reopened.cached(&staged.record.connection, &staged.record.destination).unwrap().unwrap();
            assert_eq!(cached.revision, Some(staged.record.id.clone()));
            reopened.published(&staged.record, &publication).unwrap();
            assert!(reopened.pending().unwrap().is_empty());
            assert_eq!(reopened.receipt(&staged.record).unwrap().unwrap().revision, staged.record.id);
            assert_eq!(reopened.cached_destinations(&staged.record.connection).unwrap(), vec![staged.record.destination.clone()]);
            assert_eq!(
                reopened.recover_document(&cached.save).unwrap().document,
                staged.objects[&format!("documents/{}", staged.record.manifest.document_hash)]
            );
            if boundary != "cache" {
                let value: serde_json::Value = serde_json::from_slice(&fs::read(&receipt).unwrap()).unwrap();
                assert_eq!(value["optional_annotation"], "keep on retry");
            }
        }
    }

    #[test]
    fn future_catalog_and_receipt_formats_preserve_pending_captures_and_metadata() {
        for target in ["cached", "receipts"] {
            let fixture = Fixture::new();
            let first = fixture.save("First design", None);
            let outbox = fixture.outbox();
            outbox.published(&first.record, &Fixture::publication(&first.record)).unwrap();
            let next = fixture.save("New edit", Some(first.record.id.clone()));
            let file = if target == "cached" {
                fixture.catalog_files("cached").pop().unwrap()
            } else {
                fixture.0.join(format!("receipts/{}.json", next.record.id))
            };
            let future = br#"{"format":"solidcraft-native-publication/999"}"#;
            fs::write(&file, future).unwrap();
            let before_cache = fs::read(fixture.catalog_files("cached").pop().unwrap()).unwrap();
            let error = outbox.published(&next.record, &Fixture::publication(&next.record)).unwrap_err();
            assert!(matches!(error, Error::UpgradeRequired(_)));
            assert_eq!(fs::read(&file).unwrap(), future);
            assert_eq!(fs::read(fixture.catalog_files("cached").pop().unwrap()).unwrap(), before_cache);
            assert_eq!(outbox.pending().unwrap()[0].id, next.record.id);
        }
    }

    #[test]
    fn publication_metadata_rejects_wrong_revision_commit_and_destination_before_queue_removal() {
        let fixture = Fixture::new();
        let staged = fixture.save("Retained design", None);
        for setting in ["revision", "commit", "destination", "conflict"] {
            let mut publication = Fixture::publication(&staged.record);
            match setting {
                "revision" => publication.revision = "b".repeat(64),
                "commit" => publication.commit = "invalid commit".into(),
                "destination" => publication.destination.file = "parts/unrelated".into(),
                _ => publication.conflict = true,
            }
            assert!(fixture.outbox().published(&staged.record, &publication).is_err());
            assert_eq!(fixture.outbox().pending().unwrap().len(), 1);
            assert!(!fixture.0.join("cached").exists());
            assert!(!fixture.0.join("receipts").exists());
        }
    }

    #[test]
    fn cache_identity_and_document_integrity_are_checked_before_replacement() {
        let fixture = Fixture::new();
        let first = fixture.save("First design", None);
        let outbox = fixture.outbox();
        outbox.published(&first.record, &Fixture::publication(&first.record)).unwrap();
        let next = fixture.save("New edit", Some(first.record.id.clone()));
        let cached = fixture.catalog_files("cached").pop().unwrap();
        let before = fs::read(&cached).unwrap();
        let renamed = fixture.0.join(format!("cached/{}.json", "b".repeat(64)));
        fs::rename(&cached, &renamed).unwrap();
        assert!(outbox.cached_destinations(&first.record.connection).is_err());
        fs::rename(&renamed, &cached).unwrap();
        fs::write(fixture.0.join(format!("documents/{}", first.record.manifest.document_hash)), b"corrupt document").unwrap();
        assert!(outbox.published(&next.record, &Fixture::publication(&next.record)).is_err());
        assert_eq!(fs::read(&cached).unwrap(), before);
        assert!(outbox.receipt(&next.record).unwrap().is_none());
        assert_eq!(outbox.pending().unwrap().len(), 1);
    }

    #[test]
    fn future_cached_document_requires_upgrade_before_confirmation_replaces_metadata() {
        let fixture = Fixture::new();
        let staged = fixture.save("Current edit", None);
        let outbox = fixture.outbox();
        outbox.published(&staged.record, &Fixture::publication(&staged.record)).unwrap();
        let cached = fixture.catalog_files("cached").pop().unwrap();
        let future = PendingSave::stage(
            &staged.record.connection,
            "project",
            "parts/design",
            None,
            Snapshot::prepare(br#"{"format":"solidcraft/4294967295"}"#.to_vec(), Default::default()).unwrap(),
        )
        .unwrap();
        outbox.stage(&future).unwrap();
        let bytes = serde_json::to_vec(&serde_json::json!({
            "format": "solidcraft-native-publication/1",
            "save": future.record,
            "published": Fixture::publication(&future.record),
        }))
        .unwrap();
        fs::write(&cached, &bytes).unwrap();
        let next = fixture.save("New edit", Some(staged.record.id));
        let error = outbox.published(&next.record, &Fixture::publication(&next.record)).unwrap_err();
        assert!(matches!(error, Error::UpgradeRequired(_)));
        assert_eq!(fs::read(&cached).unwrap(), bytes);
        assert!(outbox.receipt(&next.record).unwrap().is_none());
        assert_eq!(outbox.pending().unwrap().len(), 2);
    }
}
