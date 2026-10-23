//! Transport-independent publication planning. A host publishes this batch as one revision,
//! preserving the base tree and updating its ref without force.
use std::collections::BTreeMap;

use serde_json::Value;

use crate::{Destination, Error, Manifest, PROJECT_MARKER, PendingSave, Project, Result, Snapshot};

pub struct Publication {
    pub destination: Destination,
    pub files: BTreeMap<String, Vec<u8>>,
    pub conflict: bool,
    pub already_published: bool,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Published {
    pub destination: Destination,
    pub revision: String,
    pub commit: String,
    pub conflict: bool,
}

pub fn revision(manifest: &Manifest) -> Option<&str> {
    manifest.extra.get("revision").and_then(Value::as_str)
}

impl Publication {
    /// `existing` is the manifest at the requested destination. Hosts must additionally check
    /// its document schema before writing. Reading this plan never modifies local storage.
    pub fn prepare(save: &PendingSave, snapshot: Snapshot, project_marker: Option<&[u8]>, existing: Option<&[u8]>) -> Result<Self> {
        PendingSave::read(&serde_json::to_vec(save).map_err(|e| Error::Invalid(e.to_string()))?)?;
        if snapshot.manifest.write()? != save.manifest.write()? {
            return Err(Error::Invalid("snapshot does not match queued save".into()));
        }
        let project = project_marker.map(Project::read).transpose()?;
        let previous = existing.map(Manifest::read).transpose()?;
        let mut destination = save.destination.clone();
        let already_published = previous.as_ref().is_some_and(|manifest| revision(manifest) == Some(save.id.as_str()));
        let actual = previous.as_ref().and_then(revision);
        let conflict = !already_published && actual != save.expected_revision.as_deref();
        // An existing pre-revision snapshot is also a competing design, never an empty slot.
        let conflict = conflict || (!already_published && previous.is_some() && actual.is_none());
        if conflict {
            destination.file =
                format!("conflicts/{}-{}", destination.file, save.id.get(..12).ok_or_else(|| Error::Invalid("short revision".into()))?);
            destination.validate()?;
        }
        let mut files = BTreeMap::new();
        if !already_published {
            let mut manifest = snapshot.manifest;
            // Compatible optional data belongs to the design and must survive an ordinary save.
            if !conflict && let Some(previous) = previous {
                for (key, value) in previous.extra {
                    manifest.extra.entry(key).or_insert(value);
                }
                for (name, asset) in &mut manifest.assets {
                    if let Some(old) = previous.assets.get(name) {
                        for (key, value) in &old.extra {
                            asset.extra.entry(key.clone()).or_insert_with(|| value.clone());
                        }
                        for chunk in &mut asset.chunks {
                            if let Some(old_chunk) = old.chunks.iter().find(|old_chunk| old_chunk.hash == chunk.hash) {
                                for (key, value) in &old_chunk.extra {
                                    chunk.extra.entry(key.clone()).or_insert_with(|| value.clone());
                                }
                            }
                        }
                    }
                }
            }
            manifest.extra.insert("revision".into(), Value::String(save.id.clone()));
            manifest.extra.insert("parent_revision".into(), save.expected_revision.clone().map(Value::String).unwrap_or(Value::Null));
            let prefix = destination.design_path()?;
            files.insert(format!("{prefix}/document.json"), snapshot.document);
            files.insert(format!("{prefix}/manifest.json"), manifest.write()?);
            for (path, bytes) in snapshot.objects {
                crate::check_path(&path)?;
                if !path.starts_with("objects/") {
                    return Err(Error::Invalid("unexpected snapshot object path".into()));
                }
                files.insert(format!("{}/{path}", destination.project), bytes);
            }
            if project.is_none() {
                let name = destination.project.rsplit('/').next().unwrap_or("Project");
                files.insert(format!("{}/{PROJECT_MARKER}", destination.project), Project::new(name.into())?.write()?);
            }
        }
        Ok(Self { destination, files, conflict, already_published })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Connection;

    #[test]
    fn publication_is_idempotent_and_competing_designs_get_separate_files() {
        let stage = || {
            PendingSave::stage(&Connection::default(), "Robot", "parts/bracket", None, Snapshot::prepare(b"{}".to_vec(), BTreeMap::new()).unwrap())
                .unwrap()
        };
        let first = stage();
        let publish = Publication::prepare(&first.record, first.record.recover(|p| Ok(first.objects[p].clone())).unwrap(), None, None).unwrap();
        assert!(!publish.conflict);
        let marker = &publish.files["Robot/solidcraft-project.json"];
        let manifest = &publish.files["Robot/parts/bracket/manifest.json"];
        let retried = stage();
        let retry =
            Publication::prepare(&retried.record, retried.record.recover(|p| Ok(retried.objects[p].clone())).unwrap(), Some(marker), Some(manifest))
                .unwrap();
        assert!(retry.already_published);
        assert!(retry.files.is_empty());
        let other = PendingSave::stage(
            &Connection::default(),
            "Robot",
            "parts/bracket",
            None,
            Snapshot::prepare(b"{\"new\":true}".to_vec(), BTreeMap::new()).unwrap(),
        )
        .unwrap();
        let conflict =
            Publication::prepare(&other.record, other.record.recover(|p| Ok(other.objects[p].clone())).unwrap(), Some(marker), Some(manifest))
                .unwrap();
        assert!(conflict.conflict);
        assert!(conflict.files.keys().all(|p| !p.starts_with("Robot/parts/bracket/")));
        assert!(conflict.destination.file.starts_with("conflicts/parts/bracket-"));
    }

    #[test]
    fn unknown_file_kinds_block_publication_instead_of_becoming_conflict_copies() {
        let snapshot = Snapshot::prepare(b"{}".to_vec(), BTreeMap::new()).unwrap();
        let staged = PendingSave::stage(&Connection::default(), "Robot", "drawings/bracket", None, snapshot).unwrap();
        let mut existing = staged.record.manifest.clone();
        // Illustrative future capability, not a drawing schema definition. Unknown required
        // capabilities must be checked before revision comparison or any write batch exists.
        existing.required.push("drawing/1".into());
        let bytes = serde_json::to_vec(&existing).unwrap();
        let snapshot = staged.record.recover(|p| Ok(staged.objects[p].clone())).unwrap();
        assert!(matches!(Publication::prepare(&staged.record, snapshot, None, Some(&bytes)), Err(Error::UpgradeRequired(_))));
        assert!(matches!(existing.write(), Err(Error::UpgradeRequired(_))));
    }
}
