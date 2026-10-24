//! Durable-first save staging, shared by OPFS and native filesystem adapters.
//! Hosts persist every object, then atomically persist `record_path` last. Only after that
//! boundary may the UI report Saved locally. Publication never reads the current editor.
use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{Connection, Destination, Error, Manifest, Result, Snapshot, digest};

const QUEUE_FORMAT: &str = "solidcraft-outbox/1";
const PINNED_FORMAT: &str = "solidcraft-outbox/2";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PendingSave {
    pub format: String,
    pub id: String,
    pub destination: Destination,
    /// Endpoint recovery data contains credential identifiers, never credential values.
    pub connection: Connection,
    pub expected_revision: Option<String>,
    pub manifest: Manifest,
}

/// A write batch whose record is its commit point. No credentials are serialized here.
pub struct StagedSave {
    pub record: PendingSave,
    pub objects: BTreeMap<String, Vec<u8>>,
    pub record_path: String,
    pub record_bytes: Vec<u8>,
}

impl StagedSave {
    pub fn validate(&self) -> Result<()> {
        let record = PendingSave::read(&self.record_bytes)?;
        let expected_bytes = serde_json::to_vec_pretty(&self.record).map_err(|e| Error::Invalid(e.to_string()))?;
        if expected_bytes != self.record_bytes || self.record_path != format!("pending/{}.json", record.id) {
            return Err(Error::Invalid("staged save record mismatch".into()));
        }
        let recovered = record.recover(|path| self.objects.get(path).cloned().ok_or_else(|| Error::Storage("incomplete staged save".into())))?;
        let document_path = format!("documents/{}", record.manifest.document_hash);
        if self.objects.len() != recovered.objects.len() + 1
            || self.objects.keys().any(|path| path != &document_path && !recovered.objects.contains_key(path))
        {
            return Err(Error::Invalid("unexpected staged save object".into()));
        }
        Ok(())
    }
}

impl PendingSave {
    /// Pending children depend on captured local revisions, never on the latest editor state.
    /// Separate repositories and projects have no dependency on one another.
    pub fn ordered(records: Vec<Self>) -> Result<Vec<Self>> {
        if records.len() > 10_000 {
            return Err(Error::Invalid("too many pending saves".into()));
        }
        let mut remaining = BTreeMap::new();
        for record in records {
            if remaining.insert(record.id.clone(), record).is_some() {
                return Err(Error::Invalid("duplicate pending revision".into()));
            }
        }
        let mut children: BTreeMap<String, Vec<String>> = BTreeMap::new();
        let mut ready = std::collections::BTreeSet::new();
        for record in remaining.values() {
            if let Some(parent) = record.expected_revision.as_ref().and_then(|id| remaining.get(id)) {
                if parent.destination != record.destination {
                    return Err(Error::Invalid("pending revision belongs to another destination".into()));
                }
                children.entry(parent.id.clone()).or_default().push(record.id.clone());
            } else {
                ready.insert(record.id.clone());
            }
        }
        let mut ordered = Vec::new();
        while let Some(id) = ready.pop_first() {
            let record = remaining.remove(&id).ok_or_else(|| Error::Invalid("missing pending revision".into()))?;
            ordered.push(record);
            ready.extend(children.remove(&id).unwrap_or_default());
        }
        if !remaining.is_empty() {
            return Err(Error::Invalid("cyclic pending revisions".into()));
        }
        Ok(ordered)
    }
    pub fn stage(connection: &Connection, project: &str, file: &str, expected_revision: Option<String>, snapshot: Snapshot) -> Result<StagedSave> {
        connection.validate()?;
        let destination = Destination { connection: connection.id.clone(), project: project.into(), file: file.into() };
        destination.validate()?;
        let manifest_bytes = snapshot.manifest.write()?;
        let identity = serde_json::to_vec(&(&destination, connection, &expected_revision, digest(&manifest_bytes)))
            .map_err(|e| Error::Invalid(e.to_string()))?;
        let id = digest(&identity);
        let record = Self {
            format: if connection.git_remote.is_some() { PINNED_FORMAT } else { QUEUE_FORMAT }.into(),
            id: id.clone(),
            destination,
            connection: connection.clone(),
            expected_revision,
            manifest: snapshot.manifest,
        };
        let mut objects = snapshot.objects;
        objects.insert(format!("documents/{}", record.manifest.document_hash), snapshot.document);
        let record_bytes = serde_json::to_vec_pretty(&record).map_err(|e| Error::Invalid(e.to_string()))?;
        Self::read(&record_bytes)?;
        Ok(StagedSave { record, objects, record_path: format!("pending/{id}.json"), record_bytes })
    }

    pub fn read(bytes: &[u8]) -> Result<Self> {
        if bytes.len() > crate::MAX_MANIFEST_BYTES {
            return Err(Error::Invalid("pending save too large".into()));
        }
        let value: serde_json::Value = serde_json::from_slice(bytes).map_err(|e| Error::Invalid(e.to_string()))?;
        match value.get("format").and_then(serde_json::Value::as_str) {
            Some(QUEUE_FORMAT | PINNED_FORMAT) => {}
            Some(format) => {
                return Err(Error::UpgradeRequired(format!("unsupported pending save format {format}")));
            }
            None => return Err(Error::Invalid("pending save has no format".into())),
        }
        let manifest = value.get("manifest").ok_or_else(|| Error::Invalid("pending save has no manifest".into()))?;
        Manifest::read(&serde_json::to_vec(manifest).map_err(|e| Error::Invalid(e.to_string()))?)?;
        let record: Self = serde_json::from_value(value).map_err(|e| Error::Invalid(e.to_string()))?;
        // Validate every value used to select local paths after a restart.
        for id in [&record.id, &record.manifest.document_hash] {
            if id.len() != 64 || !id.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) {
                return Err(Error::Invalid("pending save has invalid hash".into()));
            }
        }
        record.destination.validate()?;
        record.connection.validate()?;
        if record.connection.id != record.destination.connection {
            return Err(Error::Invalid("pending save connection mismatch".into()));
        }
        if record.expected_revision.as_ref().is_some_and(|revision| revision.len() > 128 || revision.chars().any(char::is_control)) {
            return Err(Error::Invalid("invalid parent revision".into()));
        }
        let identity = serde_json::to_vec(&(&record.destination, &record.connection, &record.expected_revision, digest(&record.manifest.write()?)))
            .map_err(|e| Error::Invalid(e.to_string()))?;
        if record.id != digest(&identity) {
            return Err(Error::Invalid("pending save identity mismatch".into()));
        }
        Ok(record)
    }

    pub fn check_connection(&self, connection: &Connection) -> Result<()> {
        if !self.connection.same_repository(connection) {
            return Err(Error::Storage("queued save belongs to its original repository; reconnect it before synchronizing".into()));
        }
        Ok(())
    }

    /// Verify a recovered save without consulting mutable editor state.
    pub fn recover(&self, read: impl FnMut(&str) -> Result<Vec<u8>>) -> Result<Snapshot> {
        self.recover_checked(read, |_| Ok(()))
    }

    /// Schema checks precede reading or decoding any heavy payload on resumed operations.
    pub fn recover_checked(&self, mut read: impl FnMut(&str) -> Result<Vec<u8>>, check_document: impl Fn(&[u8]) -> Result<()>) -> Result<Snapshot> {
        Self::read(&serde_json::to_vec(self).map_err(|e| Error::Invalid(e.to_string()))?)?;
        let document = read(&format!("documents/{}", self.manifest.document_hash))?;
        if document.len() > crate::MAX_MANIFEST_BYTES || digest(&document) != self.manifest.document_hash {
            return Err(Error::Invalid("captured document hash or size mismatch".into()));
        }
        check_document(&document)?;
        let mut objects = BTreeMap::new();
        Snapshot::hydrate(&self.manifest, &document, |path| {
            let bytes = read(path)?;
            objects.insert(path.into(), bytes.clone());
            Ok(bytes)
        })?;
        Ok(Snapshot { manifest: self.manifest.clone(), document, objects })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn resumed_schema_preflight_happens_before_any_asset_read() {
        let snapshot = Snapshot::prepare(b"future document schema".to_vec(), BTreeMap::from([("blob".into(), vec![42; 100])])).unwrap();
        let save = PendingSave::stage(&Default::default(), "project", "part", None, snapshot).unwrap();
        let result = save.record.recover_checked(
            |path| {
                assert!(path.starts_with("documents/"), "future schemas must not read assets");
                Ok(save.objects[path].clone())
            },
            |_| Err(Error::UpgradeRequired("future document schema".into())),
        );
        assert!(matches!(result, Err(Error::UpgradeRequired(_))));
    }
    #[test]
    fn staged_save_rejects_unreferenced_files() {
        let snapshot = Snapshot::prepare(b"{}".to_vec(), BTreeMap::new()).unwrap();
        let mut staged = PendingSave::stage(&Connection::default(), "Robot", "parts/bracket", None, snapshot).unwrap();
        staged.validate().unwrap();
        staged.objects.insert("pending/another-save.json".into(), b"{}".to_vec());
        assert!(matches!(staged.validate(), Err(Error::Invalid(_))));
    }

    #[test]
    fn restart_reconstructs_captured_save_and_retries_have_stable_identity() {
        let make = || Snapshot::prepare(br#"{"value":10}"#.to_vec(), BTreeMap::from([("blob".into(), vec![9; 100])])).unwrap();
        let a = PendingSave::stage(&Connection::default(), "project", "parts/design", Some("parent".into()), make()).unwrap();
        let b = PendingSave::stage(&Connection::default(), "project", "parts/design", Some("parent".into()), make()).unwrap();
        assert_eq!(a.record.id, b.record.id);
        let restarted = PendingSave::read(&a.record_bytes).unwrap();
        let snapshot = restarted.recover(|p| a.objects.get(p).cloned().ok_or_else(|| Error::Storage("missing".into()))).unwrap();
        assert_eq!(snapshot.document, br#"{"value":10}"#);
        assert!(!String::from_utf8(a.record_bytes).unwrap().contains("token"));
    }
    #[test]
    fn incomplete_writes_and_future_queue_versions_do_not_discard_saved_records() {
        let snapshot = Snapshot::prepare(b"{}".to_vec(), BTreeMap::new()).unwrap();
        let staged = PendingSave::stage(&Connection::default(), "project", "parts/design", None, snapshot).unwrap();
        assert!(staged.record.recover(|_| Err(Error::Storage("interrupted".into()))).is_err());
        assert!(matches!(PendingSave::read(br#"{"format":"solidcraft-outbox/3"}"#), Err(Error::UpgradeRequired(_))));
        assert!(PendingSave::read(&staged.record_bytes).is_ok());
    }

    #[test]
    fn reconnecting_credentials_does_not_redirect_captured_saves() {
        let connection = Connection { repository: "owner/designs".into(), ..Default::default() };
        let save =
            PendingSave::stage(&connection, "project", "parts/design", None, Snapshot::prepare(b"{}".to_vec(), BTreeMap::new()).unwrap()).unwrap();
        let mut renewed = connection.clone();
        renewed.credential = "cloud:renewed".into();
        renewed.name = "New display name".into();
        assert!(save.record.check_connection(&renewed).is_ok());
        renewed.repository = "different/designs".into();
        assert!(save.record.check_connection(&renewed).is_err());
        let restarted = PendingSave::read(&save.record_bytes).unwrap();
        assert_eq!(restarted.connection.repository, "owner/designs");
        assert!(restarted.check_connection(&connection).is_ok());
    }

    #[test]
    fn pinned_queue_versions_keep_recovery_identity_and_reject_retargeting() {
        let remote = crate::GitRemote { fetch: "../read.git".into(), push: vec!["../write.git".into()] };
        let connection = Connection { git_remote: Some(remote), ..Default::default() };
        let staged = PendingSave::stage(&connection, "project", "part", None, Snapshot::prepare(b"{}".to_vec(), BTreeMap::new()).unwrap()).unwrap();
        assert_eq!(staged.record.format, "solidcraft-outbox/2");
        staged.validate().unwrap();
        let recovered = PendingSave::read(&staged.record_bytes).unwrap();
        recovered.check_connection(&connection).unwrap();
        let mut changed = connection.clone();
        changed.git_remote.as_mut().unwrap().push = vec!["../another.git".into()];
        assert!(recovered.check_connection(&changed).is_err());
        changed.git_remote = None;
        assert!(recovered.check_connection(&changed).is_err());
    }

    #[test]
    fn offline_saves_keep_project_and_repository_identity_and_revision_order() {
        let stage = |connection: &Connection, project: &str, parent, text: &str| {
            PendingSave::stage(connection, project, "parts/design", parent, Snapshot::prepare(text.as_bytes().to_vec(), BTreeMap::new()).unwrap())
                .unwrap()
        };
        let connection = Connection::default();
        let first = stage(&connection, "robot", None, "first");
        let second = stage(&connection, "robot", Some(first.record.id.clone()), "second");
        let other_project = stage(&connection, "enclosure", None, "first");
        let other_repo = stage(&Connection { id: "work".into(), credential: "cloud:work".into(), ..Default::default() }, "robot", None, "first");
        assert_ne!(first.record.id, other_project.record.id);
        assert_ne!(first.record.id, other_repo.record.id);
        let ordered = PendingSave::ordered(vec![second.record.clone(), other_repo.record, other_project.record, first.record.clone()]).unwrap();
        let index = |id: &str| ordered.iter().position(|save| save.id == id).unwrap();
        assert!(index(&first.record.id) < index(&second.record.id));
        assert_eq!(second.record.destination.design_path().unwrap(), "robot/parts/design");
    }
}
