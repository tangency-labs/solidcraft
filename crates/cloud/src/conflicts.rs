//! Explicit conflict choices, published atomically through the ordinary durable save queue.
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{CachedEntry, Destination, Error, Manifest, PendingSave, Publication, Published, Result, Snapshot, management::Change};

pub const CAPABILITY: &str = "conflict-resolution/1";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Keep {
    Local,
    Remote,
}

/// Conflict paths are reserved and deterministic, including copies made by older clients.
pub fn original(copy: &Destination) -> Result<Option<Destination>> {
    copy.validate()?;
    let Some(path) = copy.file.strip_prefix("conflicts/") else { return Ok(None) };
    if let Some((revision, file)) = path.split_once('/')
        && revision.len() == 12
        && revision.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        let original = Destination { file: file.into(), ..copy.clone() };
        original.validate()?;
        return Ok(Some(original));
    }
    let Some((file, suffix)) = path.rsplit_once('-') else { return Ok(None) };
    if suffix.len() != 12 || !suffix.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) {
        return Ok(None);
    }
    let original = Destination { file: file.into(), ..copy.clone() };
    original.validate()?;
    Ok(Some(original))
}

/// The original head to compare. No immutable assets need downloading to make this choice.
pub fn target(save: &PendingSave) -> Result<Option<Destination>> {
    let Some(change @ Change::Resolve { .. }) = save.manifest.change()? else { return Ok(None) };
    change.validate(&save.destination)?;
    original(&save.destination)
}

fn manifests(save: &PendingSave) -> Result<Option<(Manifest, Option<(Destination, Manifest)>)>> {
    let Some(Change::Resolve { file, revision, keep }) = save.manifest.change()? else { return Ok(None) };
    let mut tombstone = save.manifest.clone();
    tombstone.required.retain(|c| c != CAPABILITY);
    tombstone.set_change(&Change::Delete)?;
    tombstone.extra.insert("revision".into(), save.id.clone().into());
    tombstone.extra.insert("parent_revision".into(), save.expected_revision.clone().map(Value::String).unwrap_or(Value::Null));
    let replacement = if keep == Keep::Local {
        let mut manifest = tombstone.clone();
        manifest.extra.remove("lifecycle");
        manifest.extra.remove("moved_to");
        manifest.extra.insert("parent_revision".into(), revision.map(Value::String).unwrap_or(Value::Null));
        Some((Destination { file, ..save.destination.clone() }, manifest))
    } else {
        None
    };
    Ok(Some((tombstone, replacement)))
}

pub(crate) fn prepare(save: &PendingSave, snapshot: Snapshot, target: Option<&[u8]>, already_published: bool) -> Result<Publication> {
    let change = save.manifest.change()?.ok_or(Error::Conflict)?;
    change.validate(&save.destination)?;
    let Change::Resolve { revision, .. } = &change else { return Err(Error::Conflict) };
    let target = target.map(Manifest::read).transpose()?;
    // A successful retry is recognized by the conflict copy's tombstone, even when the
    // original has since changed again. It must never republish an obsolete replacement.
    if !already_published && (target.as_ref().and_then(crate::revision) != revision.as_deref() || (target.is_some() && revision.is_none())) {
        return Err(Error::Conflict);
    }
    let destination = change.destination(&save.destination);
    let mut files = std::collections::BTreeMap::new();
    if !already_published {
        let (tombstone, replacement) = manifests(save)?.ok_or(Error::Conflict)?;
        let source = save.destination.design_path()?;
        files.insert(format!("{source}/manifest.json"), tombstone.write()?);
        files.insert(format!("{source}/document.json"), snapshot.document.clone());
        if let Some((destination, mut manifest)) = replacement {
            if let Some(previous) = target {
                for (key, value) in previous.extra {
                    if !matches!(key.as_str(), "lifecycle" | "moved_to") {
                        manifest.extra.entry(key).or_insert(value);
                    }
                }
            }
            let prefix = destination.design_path()?;
            files.insert(format!("{prefix}/manifest.json"), manifest.write()?);
            files.insert(format!("{prefix}/document.json"), snapshot.document);
        }
        for (path, bytes) in snapshot.objects {
            crate::check_path(&path)?;
            if !path.starts_with("objects/") {
                return Err(Error::Invalid("unexpected snapshot object path".into()));
            }
            files.insert(format!("{}/{path}", destination.project), bytes);
        }
    }
    Ok(Publication { destination, files, conflict: false, already_published })
}

/// Normalize confirmed cache heads without copying, decoding or re-encoding any payload.
/// The source remains recoverable in Trash; keeping local also updates the original head.
pub fn confirmed(save: &PendingSave, published: &Published) -> Result<Option<Vec<CachedEntry>>> {
    let Some((tombstone, replacement)) = manifests(save)? else { return Ok(None) };
    published.validate(save, None)?;
    let mut entries = Vec::new();
    for (destination, manifest) in std::iter::once((save.destination.clone(), tombstone)).chain(replacement) {
        let captured = PendingSave::describe(&save.connection, &destination.project, &destination.file, None, manifest)?;
        entries.push(CachedEntry::new(captured, destination, Some(published.revision.clone()), true)?);
    }
    Ok(Some(entries))
}

/// A late acknowledgement must not hide another locally saved head.
pub fn may_replace(save: &PendingSave, current: &CachedEntry) -> Result<bool> {
    let expected = if current.destination == save.destination {
        &save.expected_revision
    } else if let Some(Change::Resolve { revision, .. }) = save.manifest.change()?.as_ref() {
        return Ok(current.revision == *revision || current.revision.as_deref() == Some(save.id.as_str()));
    } else {
        return Err(Error::Conflict);
    };
    Ok(current.revision == *expected || current.save.id == save.id || current.revision.as_deref() == Some(save.id.as_str()))
}
