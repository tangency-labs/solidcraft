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

impl Published {
    /// Only a confirmed captured parent may redirect an offline child. Revision identities
    /// stay immutable; this mapping belongs to publication, not to the staged document.
    pub fn target(save: &PendingSave, parent: Option<&Self>) -> Result<Destination> {
        let Some(parent) = parent else { return Ok(save.destination.clone()) };
        parent.destination.validate()?;
        if save.expected_revision.as_deref() != Some(parent.revision.as_str())
            || parent.destination.connection != save.destination.connection
            || parent.destination.project != save.destination.project
            || !matches!(parent.commit.len(), 40 | 64)
            || !parent.commit.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            return Err(Error::Invalid("publication parent mismatch".into()));
        }
        Ok(parent.destination.clone())
    }

    pub fn validate(&self, save: &PendingSave, parent: Option<&Self>) -> Result<()> {
        let target = Self::target(save, parent)?;
        let redirected = target != save.destination;
        let valid_destination = if let Some(change) = save.manifest.change()? {
            !self.conflict && !redirected && self.destination == change.destination(&target)
        } else if self.conflict {
            (redirected && self.destination == target) || self.destination == conflict_destination(&target, &save.id)?
        } else {
            !redirected && self.destination == target
        };
        if !valid_destination
            || self.revision != save.id
            || !matches!(self.commit.len(), 40 | 64)
            || !self.commit.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            return Err(Error::Invalid("publication acknowledgement mismatch".into()));
        }
        Ok(())
    }
}

fn conflict_destination(destination: &Destination, revision: &str) -> Result<Destination> {
    let mut destination = destination.clone();
    let suffix = revision.get(..12).ok_or_else(|| Error::Invalid("short revision".into()))?;
    let original = destination.file.clone();
    destination.file = format!("conflicts/{original}-{suffix}");
    if destination.validate().is_err() {
        destination.file = format!("conflicts/{suffix}/{original}");
    }
    destination.validate()?;
    Ok(destination)
}

impl Publication {
    /// Check the captured payload mapping before acknowledging a retry. Transports
    /// additionally verify the committed document and object bytes (or Git identities).
    pub fn verify_payload(save: &PendingSave, manifest: &Manifest) -> Result<()> {
        manifest.write()?;
        let mut manifest = manifest.clone();
        // Management normalizes lifecycle requirements; optional metadata may grow.
        manifest.required.clone_from(&save.manifest.required);
        if crate::manifest_digest(&manifest)? != crate::manifest_digest(&save.manifest)? {
            return Err(Error::Invalid("published payload differs from captured save; local save retained".into()));
        }
        Ok(())
    }

    /// Managed storage bytes must survive ordinary Git checkouts without conversion.
    /// Ordinary project documents retain the repository's own attributes.
    pub const GIT_ATTRIBUTES: &[u8] = b".gitattributes -text -filter -ident -working-tree-encoding\nsolidcraft-project.json -text -filter -ident -working-tree-encoding\n**/document.json -text -filter -ident -working-tree-encoding\n**/manifest.json -text -filter -ident -working-tree-encoding\nobjects/** -text -filter -ident -working-tree-encoding\n";

    /// `existing` is the manifest at the requested destination. Hosts must additionally check
    /// its document schema before writing. Reading this plan never modifies local storage.
    pub fn prepare(save: &PendingSave, snapshot: Snapshot, project_marker: Option<&[u8]>, existing: Option<&[u8]>) -> Result<Self> {
        Self::prepare_with_parent(save, snapshot, project_marker, existing, None)
    }

    pub fn prepare_with_parent(
        save: &PendingSave,
        snapshot: Snapshot,
        project_marker: Option<&[u8]>,
        existing: Option<&[u8]>,
        parent: Option<&Published>,
    ) -> Result<Self> {
        Self::prepare_with_target(save, snapshot, project_marker, existing, parent, None)
    }

    /// Resolution compares both design heads in the same repository tree before publishing.
    pub fn prepare_with_target(
        save: &PendingSave,
        snapshot: Snapshot,
        project_marker: Option<&[u8]>,
        existing: Option<&[u8]>,
        parent: Option<&Published>,
        resolution_target: Option<&[u8]>,
    ) -> Result<Self> {
        PendingSave::read(&serde_json::to_vec(save).map_err(|e| Error::Invalid(e.to_string()))?)?;
        if snapshot.manifest.write()? != save.manifest.write()? {
            return Err(Error::Invalid("snapshot does not match queued save".into()));
        }
        let project = project_marker.map(Project::read).transpose()?;
        let previous = existing.map(Manifest::read).transpose()?;
        let mut destination = Published::target(save, parent)?;
        let source = destination.clone();
        let change = save.manifest.change()?;
        let redirected = destination != save.destination;
        let already_published = previous.as_ref().is_some_and(|manifest| revision(manifest) == Some(save.id.as_str()));
        let actual = previous.as_ref().and_then(revision);
        let conflict = !already_published && actual != save.expected_revision.as_deref();
        // An existing pre-revision snapshot is also a competing design, never an empty slot.
        let conflict = conflict || (!already_published && previous.is_some() && actual.is_none());
        if change.is_some() && (conflict || redirected || previous.is_none()) {
            return Err(Error::Conflict);
        }
        if matches!(change, Some(crate::management::Change::Resolve { .. })) {
            return crate::conflicts::prepare(save, snapshot, resolution_target, already_published);
        }
        if conflict {
            destination = conflict_destination(&destination, &save.id)?;
        }
        if let Some(change) = &change {
            destination = change.destination(&destination);
        }
        let mut files = BTreeMap::new();
        if !already_published {
            files.insert(format!("{}/.gitattributes", destination.project), Self::GIT_ATTRIBUTES.to_vec());
            let mut manifest = snapshot.manifest;
            // Compatible optional data belongs to the design and must survive an ordinary save.
            if !conflict && let Some(previous) = previous {
                for (key, value) in previous.extra {
                    if !matches!(key.as_str(), "lifecycle" | "moved_to") {
                        manifest.extra.entry(key).or_insert(value);
                    }
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
            if matches!(change, Some(crate::management::Change::Rename { .. })) {
                let mut tombstone = manifest.clone();
                tombstone.set_change(&crate::management::Change::Delete)?;
                tombstone.extra.insert("moved_to".into(), Value::String(destination.file.clone()));
                let old = source.design_path()?;
                files.insert(format!("{old}/manifest.json"), tombstone.write()?);
                files.insert(format!("{old}/document.json"), snapshot.document.clone());
            }
            if matches!(change, Some(crate::management::Change::Rename { .. } | crate::management::Change::Restore { .. })) {
                manifest.extra.remove("lifecycle");
                manifest.extra.remove("moved_to");
            }
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
        Ok(Self { destination, files, conflict: conflict || redirected, already_published })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Connection;

    #[test]
    fn conflict_paths_preserve_maximum_names_unicode_and_full_paths() {
        for file in ["a".repeat(128), "é".repeat(64), format!("{}/{}/{}/{}", "x".repeat(127), "y".repeat(127), "z".repeat(127), "w".repeat(128))] {
            let connection = Connection::default();
            let destination = Destination { connection: connection.id.clone(), project: "Project".into(), file };
            destination.validate().unwrap();
            let copy = conflict_destination(&destination, &"a".repeat(64)).unwrap();
            assert!(copy.file.split('/').all(|part| part.len() <= 128));
            assert_eq!(crate::conflicts::original(&copy).unwrap().unwrap(), destination);
            let child = conflict_destination(&copy, &"b".repeat(64)).unwrap();
            assert_eq!(crate::conflicts::original(&child).unwrap().unwrap(), copy);
            let change = crate::management::Change::Resolve {
                file: destination.file.clone(),
                revision: Some("a".repeat(64)),
                keep: crate::conflicts::Keep::Local,
            };
            change.validate(&copy).unwrap();
        }
    }

    #[test]
    fn conflict_resolution_checks_both_heads_is_atomic_and_retries_without_replacing_newer_edits() {
        use crate::{
            conflicts::{CAPABILITY, Keep},
            management::Change,
        };
        let connection = Connection::default();
        let first = PendingSave::stage(
            &connection,
            "Project",
            "parts/design",
            None,
            Snapshot::prepare(b"{\"remote\":true}".to_vec(), BTreeMap::new()).unwrap(),
        )
        .unwrap();
        let remote = Publication::prepare(&first.record, first.record.recover(|p| Ok(first.objects[p].clone())).unwrap(), None, None).unwrap();
        let marker = &remote.files["Project/solidcraft-project.json"];
        let original = &remote.files["Project/parts/design/manifest.json"];
        let local = PendingSave::stage(
            &connection,
            "Project",
            "parts/design",
            None,
            Snapshot::prepare(b"{\"local\":true}".to_vec(), BTreeMap::new()).unwrap(),
        )
        .unwrap();
        let copy = Publication::prepare(&local.record, local.record.recover(|p| Ok(local.objects[p].clone())).unwrap(), Some(marker), Some(original))
            .unwrap();
        assert_eq!(crate::conflicts::original(&copy.destination).unwrap().unwrap(), first.record.destination);
        let source_path = format!("{}/manifest.json", copy.destination.design_path().unwrap());
        let source = &copy.files[&source_path];
        for keep in [Keep::Local, Keep::Remote] {
            let mut snapshot = local.record.recover(|p| Ok(local.objects[p].clone())).unwrap();
            snapshot
                .manifest
                .set_change(&Change::Resolve { file: first.record.destination.file.clone(), revision: Some(first.record.id.clone()), keep })
                .unwrap();
            let save = PendingSave::stage(&connection, "Project", &copy.destination.file, Some(local.record.id.clone()), snapshot).unwrap();
            let recover = || save.record.recover(|p| Ok(save.objects[p].clone())).unwrap();
            let plan = Publication::prepare_with_target(&save.record, recover(), Some(marker), Some(source), None, Some(original)).unwrap();
            assert!(Manifest::read(&plan.files[&source_path]).unwrap().deleted());
            assert!(!plan.conflict);
            assert!(plan.files.values().all(|bytes| !String::from_utf8_lossy(bytes).contains(CAPABILITY)));
            assert_eq!(plan.files.contains_key("Project/parts/design/document.json"), keep == Keep::Local);
            if keep == Keep::Local {
                assert_eq!(plan.files["Project/parts/design/document.json"], b"{\"local\":true}");
            }
            let mut newer = Manifest::read(original).unwrap();
            newer.extra.insert("revision".into(), "f".repeat(64).into());
            assert!(matches!(
                Publication::prepare_with_target(&save.record, recover(), Some(marker), Some(source), None, Some(&newer.write().unwrap())),
                Err(Error::Conflict)
            ));
            let retry = Publication::prepare_with_target(
                &save.record,
                recover(),
                Some(marker),
                Some(&plan.files[&source_path]),
                None,
                Some(&newer.write().unwrap()),
            )
            .unwrap();
            assert!(retry.already_published && retry.files.is_empty());
            let published = Published { destination: plan.destination, revision: save.record.id.clone(), commit: "a".repeat(40), conflict: false };
            let cached = crate::conflicts::confirmed(&save.record, &published).unwrap().unwrap();
            assert!(cached[0].save.manifest.deleted());
            assert_eq!(cached.len(), if keep == Keep::Local { 2 } else { 1 });
            let mut future: Value = serde_json::from_slice(&save.record.manifest.write().unwrap()).unwrap();
            future["required"].as_array_mut().unwrap().push("conflict-resolution/2".into());
            assert!(matches!(Manifest::read(&serde_json::to_vec(&future).unwrap()), Err(Error::UpgradeRequired(_))));
        }
    }

    #[test]
    fn management_is_atomic_idempotent_and_refuses_a_stale_parent() {
        use crate::management::Change;
        let connection = Connection::default();
        let initial =
            PendingSave::stage(&connection, "Project", "parts/old", None, Snapshot::prepare(b"{}".to_vec(), BTreeMap::new()).unwrap()).unwrap();
        let first = Publication::prepare(&initial.record, initial.record.recover(|p| Ok(initial.objects[p].clone())).unwrap(), None, None).unwrap();
        let source = &first.files["Project/parts/old/manifest.json"];
        let marker = &first.files["Project/solidcraft-project.json"];
        for change in
            [Change::Rename { file: "parts/new".into() }, Change::Delete, Change::Restore { commit: "a".repeat(40), file: "parts/old".into() }]
        {
            let mut snapshot = Snapshot::prepare(b"{}".to_vec(), BTreeMap::new()).unwrap();
            snapshot.manifest.set_change(&change).unwrap();
            let save = PendingSave::stage(&connection, "Project", "parts/old", Some(initial.record.id.clone()), snapshot).unwrap();
            let plan = Publication::prepare(&save.record, save.record.recover(|p| Ok(save.objects[p].clone())).unwrap(), Some(marker), Some(source))
                .unwrap();
            assert!(!plan.conflict);
            let source_after = plan.files.get("Project/parts/old/manifest.json").unwrap();
            let published =
                Published { destination: plan.destination.clone(), revision: save.record.id.clone(), commit: "a".repeat(40), conflict: false };
            published.validate(&save.record, None).unwrap();
            if matches!(change, Change::Rename { .. }) {
                assert!(Manifest::read(source_after).unwrap().moved());
                assert!(!Manifest::read(&plan.files["Project/parts/new/manifest.json"]).unwrap().deleted());
            }
            let retry =
                Publication::prepare(&save.record, save.record.recover(|p| Ok(save.objects[p].clone())).unwrap(), Some(marker), Some(source_after))
                    .unwrap();
            assert!(retry.already_published);
            assert!(retry.files.is_empty());
            assert_eq!(retry.destination, plan.destination);
            let mut competitor = Manifest::read(source).unwrap();
            competitor.extra.insert("revision".into(), Value::String("b".repeat(64)));
            assert!(matches!(
                Publication::prepare(
                    &save.record,
                    save.record.recover(|p| Ok(save.objects[p].clone())).unwrap(),
                    Some(marker),
                    Some(&competitor.write().unwrap())
                ),
                Err(Error::Conflict)
            ));
        }
    }

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
