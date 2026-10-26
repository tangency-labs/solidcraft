//! Machine-local readable heads, separate from pending uploads and publication receipts.
//! Shared by browser storage and desktop recovery; never stored as Git objects.
use crate::{Destination, Error, Manifest, PendingSave, Result, jobs::Phase};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

const FORMAT: &str = "solidcraft-cache/1";

#[derive(Clone, Serialize, Deserialize)]
pub struct CachedEntry {
    format: String,
    pub save: PendingSave,
    pub destination: Destination,
    pub revision: Option<String>,
    /// Read from a remote, or publication confirmed. Reading a local checkout alone
    /// does not establish publication.
    #[serde(default)]
    pub confirmed: bool,
    /// A newer repository head was observed; retain this copy until it is pulled.
    #[serde(default)]
    pub outdated: bool,
    #[serde(default)]
    pub conflict: bool,
    #[serde(flatten)]
    extra: BTreeMap<String, serde_json::Value>,
}

impl CachedEntry {
    pub fn new(save: PendingSave, destination: Destination, revision: Option<String>, confirmed: bool) -> Result<Self> {
        let entry = Self { format: FORMAT.into(), save, destination, revision, confirmed, outdated: false, conflict: false, extra: BTreeMap::new() };
        entry.write()?;
        Ok(entry)
    }
    /// Mark only the head that was actually compared, preserving a concurrent save/pull.
    pub fn mark_outdated(&mut self, observed: &StoredDesign) -> Result<bool> {
        if self.destination != observed.destination
            || self.revision != observed.revision
            || self.save.manifest.document_hash != observed.document_hash
            || manifest_digest(&self.save.manifest)? != observed.manifest_hash
        {
            return Ok(false);
        }
        self.outdated = true;
        Ok(true)
    }
    pub fn read(text: &str) -> Result<Self> {
        if text.len() > crate::MAX_MANIFEST_BYTES {
            return Err(invalid("cached entry too large"));
        }
        let value: serde_json::Value = serde_json::from_str(text).map_err(invalid)?;
        if value.get("format").and_then(serde_json::Value::as_str) != Some(FORMAT) {
            return Err(Error::UpgradeRequired("cached entry format".into()));
        }
        let save = value.get("save").ok_or_else(|| invalid("cached capture missing"))?;
        PendingSave::read(&serde_json::to_vec(save).map_err(invalid)?)?;
        let entry: Self = serde_json::from_value(value).map_err(invalid)?;
        entry.destination.validate()?;
        if entry.destination.connection != entry.save.destination.connection
            || entry.destination.project != entry.save.destination.project
            || invalid_revision(&entry.revision)
        {
            return Err(invalid("cached destination or revision mismatch"));
        }
        Ok(entry)
    }
    pub fn write(&self) -> Result<String> {
        let text = serde_json::to_string(self).map_err(invalid)?;
        Self::read(&text)?;
        Ok(text)
    }
}

fn invalid(error: impl std::fmt::Display) -> Error {
    Error::Invalid(error.to_string())
}
fn invalid_revision(revision: &Option<String>) -> bool {
    revision.as_ref().is_some_and(|r| r.is_empty() || r.len() > 128 || r.chars().any(char::is_control))
}
fn hash(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// A queued tip offered for explicit offline inspection.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LocalRevision {
    pub revision: String,
    pub title: String,
}

/// Durable listing details, carried separately from the destination array for older callers.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StoredDesign {
    pub destination: Destination,
    pub revision: Option<String>,
    pub document_hash: String,
    pub manifest_hash: String,
    pub phase: Phase,
    pub conflict: bool,
    #[serde(default)]
    pub outdated: bool,
    /// Competing queued tips. The caller may open one explicitly without discarding any.
    #[serde(default)]
    pub local_revisions: Vec<LocalRevision>,
}
impl StoredDesign {
    pub fn matches_manifest(&self, manifest: &Manifest) -> Result<bool> {
        Ok(self.document_hash == manifest.document_hash
            && self.manifest_hash == manifest_digest(manifest)?
            && self.revision.as_deref() == crate::revision(manifest))
    }

    pub fn validate(&self, connection: &str) -> Result<()> {
        self.destination.validate()?;
        if self.destination.connection != connection
            || !hash(&self.document_hash)
            || !hash(&self.manifest_hash)
            || !matches!(self.phase, Phase::Ready | Phase::SavedLocally | Phase::Synced | Phase::Failed)
            || (self.outdated && self.phase != Phase::Ready)
            || invalid_revision(&self.revision)
            || self.local_revisions.len() > 10_000
            || self
                .local_revisions
                .iter()
                .any(|revision| !hash(&revision.revision) || revision.title.len() > 512 || revision.title.chars().any(char::is_control))
            || self.local_revisions.iter().map(|r| &r.revision).collect::<BTreeSet<_>>().len() != self.local_revisions.len()
        {
            return Err(invalid("invalid stored design status"));
        }
        Ok(())
    }
}

/// Pending tips override cached acknowledgements. Competing tips require attention;
/// no arbitrary local capture becomes the selected head.
pub fn stored_designs(cached: Vec<CachedEntry>, pending: Vec<PendingSave>) -> Result<Vec<StoredDesign>> {
    if cached.len() > 10_000 {
        return Err(invalid("too many cached designs"));
    }
    let key = |d: &Destination| (d.connection.clone(), d.project.clone(), d.file.clone());
    let mut entries = BTreeMap::new();
    for entry in cached {
        if entry.save.manifest.deleted() {
            continue;
        }
        let manifest_hash = manifest_digest(&entry.save.manifest)?;
        entries.insert(
            key(&entry.destination),
            StoredDesign {
                destination: entry.destination,
                revision: entry.revision,
                document_hash: entry.save.manifest.document_hash,
                manifest_hash,
                phase: if entry.confirmed && !entry.outdated { Phase::Synced } else { Phase::Ready },
                outdated: entry.outdated,
                conflict: entry.conflict,
                local_revisions: Vec::new(),
            },
        );
    }
    let pending = PendingSave::ordered(pending)?;
    let parents: BTreeSet<_> = pending.iter().filter_map(|p| p.expected_revision.as_ref()).collect();
    let mut counts = BTreeMap::new();
    for save in pending.iter().filter(|p| !parents.contains(&p.id)) {
        if save.manifest.deleted() {
            entries.remove(&key(&save.destination));
            continue;
        }
        let key = key(&save.destination);
        let count = counts.entry(key.clone()).or_insert(0usize);
        *count += 1;
        let mut local_revisions = entries.remove(&key).map(|entry| entry.local_revisions).unwrap_or_default();
        let title = save
            .manifest
            .extra
            .get("title")
            .and_then(serde_json::Value::as_str)
            .filter(|title| title.len() <= 512 && !title.chars().any(char::is_control))
            .unwrap_or_default()
            .to_owned();
        local_revisions.push(LocalRevision { revision: save.id.clone(), title });
        entries.insert(
            key,
            StoredDesign {
                destination: save.destination.clone(),
                revision: if *count == 1 { Some(save.id.clone()) } else { None },
                document_hash: save.manifest.document_hash.clone(),
                manifest_hash: manifest_digest(&save.manifest)?,
                phase: if *count == 1 { Phase::SavedLocally } else { Phase::Failed },
                conflict: false,
                outdated: false,
                local_revisions,
            },
        );
    }
    if entries.len() > 10_000 {
        return Err(invalid("too many stored designs"));
    }
    Ok(entries.into_values().collect())
}

/// Presentation-only content identity. Required capabilities and asset mappings count;
/// publication bookkeeping and compatible optional metadata do not change the payload.
pub fn manifest_digest(manifest: &Manifest) -> Result<String> {
    let mut manifest = manifest.clone();
    manifest.extra.clear();
    for asset in manifest.assets.values_mut() {
        asset.extra.clear();
        for chunk in &mut asset.chunks {
            chunk.extra.clear();
        }
    }
    Ok(crate::digest(&manifest.write()?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Connection, Snapshot, jobs::Phase};
    fn save(name: &str, parent: Option<String>) -> PendingSave {
        PendingSave::stage(
            &Connection::default(),
            "Robot",
            "parts/bracket",
            parent,
            Snapshot::prepare(name.as_bytes().to_vec(), BTreeMap::new()).unwrap(),
        )
        .unwrap()
        .record
    }
    #[test]
    fn cache_envelope_preserves_optional_metadata_and_rejects_future_formats_first() {
        let save = save("one", None);
        let cache = CachedEntry::new(save.clone(), save.destination.clone(), None, false).unwrap();
        let mut value: serde_json::Value = serde_json::from_str(&cache.write().unwrap()).unwrap();
        value.as_object_mut().unwrap().remove("confirmed");
        value.as_object_mut().unwrap().remove("conflict");
        value["optional"] = serde_json::json!({"tag":42});
        let old = CachedEntry::read(&value.to_string()).unwrap();
        assert!(!old.confirmed);
        assert_eq!(serde_json::from_str::<serde_json::Value>(&old.write().unwrap()).unwrap()["optional"]["tag"], 42);
        value["format"] = "solidcraft-cache/99".into();
        value["save"] = serde_json::Value::Null;
        assert!(matches!(CachedEntry::read(&value.to_string()), Err(Error::UpgradeRequired(_))));
    }
    #[test]
    fn outdated_heads_survive_restart_without_hiding_local_work_or_newer_pulls() {
        let first = save("one", None);
        let mut cache = CachedEntry::new(first.clone(), first.destination.clone(), Some(first.id.clone()), true).unwrap();
        let observed = stored_designs(vec![cache.clone()], vec![]).unwrap().remove(0);
        assert!(cache.mark_outdated(&observed).unwrap());
        let cache = CachedEntry::read(&cache.write().unwrap()).unwrap();
        let stale = stored_designs(vec![cache.clone()], vec![]).unwrap();
        assert!(stale[0].outdated);
        assert_eq!(stale[0].phase, Phase::Ready);
        stale[0].validate(&first.connection.id).unwrap();
        let child = save("local work", Some(first.id.clone()));
        let local = stored_designs(vec![cache], vec![child.clone()]).unwrap();
        assert!(!local[0].outdated);
        assert_eq!(local[0].phase, Phase::SavedLocally);
        let mut newer = CachedEntry::new(child.clone(), child.destination.clone(), Some(child.id), true).unwrap();
        assert!(!newer.mark_outdated(&observed).unwrap());
        assert!(!newer.outdated);
        let mut legacy = serde_json::to_value(newer).unwrap();
        legacy.as_object_mut().unwrap().remove("outdated");
        assert!(!CachedEntry::read(&legacy.to_string()).unwrap().outdated);
    }

    #[test]
    fn durable_heads_distinguish_read_only_confirmed_queued_and_competing_captures() {
        let first = save("one", None);
        let cache = CachedEntry::new(first.clone(), first.destination.clone(), Some(first.id.clone()), false).unwrap();
        let status = stored_designs(vec![cache.clone()], vec![]).unwrap();
        assert_eq!(status[0].phase, Phase::Ready);
        status[0].validate(&first.connection.id).unwrap();
        assert!(status[0].validate("other").is_err());
        let mut confirmed = cache.clone();
        confirmed.confirmed = true;
        assert_eq!(stored_designs(vec![confirmed], vec![]).unwrap()[0].phase, Phase::Synced);
        let child = save("two", Some(first.id.clone()));
        let status = stored_designs(vec![cache.clone()], vec![first.clone(), child.clone()]).unwrap();
        assert_eq!(status[0].phase, Phase::SavedLocally);
        assert_eq!(status[0].revision.as_deref(), Some(child.id.as_str()));
        let competitor = save("other", None);
        let records = vec![first, child.clone(), competitor.clone()];
        let status = stored_designs(vec![cache], records.clone()).unwrap();
        assert_eq!(status[0].phase, Phase::Failed);
        assert!(status[0].revision.is_none());
        assert_eq!(status[0].local_revisions.len(), 2);
        assert!(status[0].local_revisions.iter().any(|r| r.revision == child.id));
        assert!(status[0].local_revisions.iter().any(|r| r.revision == competitor.id));
        assert!(matches!(PendingSave::local_head(records.clone(), &child.destination, None), Err(Error::Conflict)));
        assert_eq!(PendingSave::local_head(records.clone(), &child.destination, Some(&child.id)).unwrap().unwrap().id, child.id);
        let mut other = child.destination.clone();
        other.project = "Other".into();
        assert!(PendingSave::local_head(records.clone(), &other, Some(&child.id)).is_err());
        assert!(PendingSave::local_head(records, &child.destination, Some(&"f".repeat(64))).is_err());
    }
}
