//! Management captures use the existing outbox, publication receipts and retry ordering.
use super::*;
use solidcraft_cloud::{Destination, StagedSave, management::Change};

pub(super) fn current(
    outbox: &LocalOutbox,
    connection: &Connection,
    destination: &Destination,
    executable: &str,
    job: &Job,
) -> Result<solidcraft_cloud::Snapshot> {
    if let Some(save) = pending_tip(outbox, connection, destination)? {
        if save.manifest.change()?.is_some() {
            return Err(storage("synchronize this design's pending management action first"));
        }
        let mut snapshot = outbox.recover_document(&save)?;
        snapshot.manifest.extra.insert("revision".into(), serde_json::json!(save.id));
        return Ok(snapshot);
    }
    match GitRepository::open_cancellable(connection.clone(), executable, job.clone()).and_then(|repo| repo.snapshot(destination)) {
        Ok(snapshot) => Ok(snapshot),
        Err(error @ Error::Storage(_)) => {
            let Some(entry) = outbox.cached(connection, destination)? else { return Err(error) };
            let mut snapshot = outbox.recover_document(&entry.save)?;
            if let Some(revision) = entry.revision {
                snapshot.manifest.extra.insert("revision".into(), serde_json::json!(revision));
            }
            Ok(snapshot)
        }
        Err(error) => Err(error),
    }
}

pub(super) fn stage(
    outbox: &LocalOutbox,
    connection: &Connection,
    destination: &Destination,
    expected: Option<String>,
    change: &Change,
    executable: &str,
    job: &Job,
) -> Result<StagedSave> {
    change.validate(destination)?;
    let current = current(outbox, connection, destination, executable, job)?;
    let history = current.manifest.history_paths(destination)?;
    if solidcraft_cloud::revision(&current.manifest) != expected.as_deref() {
        return Err(Error::Conflict);
    }
    let mut snapshot = if let Change::Restore { commit, file } = change {
        let historical = Destination { file: file.clone(), ..destination.clone() };
        GitRepository::open_cancellable(connection.clone(), executable, job.clone())?.snapshot_at(&historical, commit)?
    } else {
        current
    };
    if matches!(change, Change::Restore { .. }) {
        snapshot
            .manifest
            .extra
            .insert("previous_paths".into(), serde_json::json!(history.into_iter().filter(|p| p != &destination.file).take(16).collect::<Vec<_>>()));
    }
    let snapshot = super::super::prepare_change(snapshot, destination, change)?;
    PendingSave::stage(connection, &destination.project, &destination.file, expected, snapshot)
}

pub(super) fn discard(
    outbox: &LocalOutbox,
    connection: &Connection,
    destination: &Destination,
    revision: &str,
    executable: &str,
    job: &Job,
    access: Option<&dyn super::super::NativeAccess>,
) -> Result<()> {
    let _sync = outbox.wait_for_synchronization(job)?;
    let pending = outbox.pending()?;
    let save = pending
        .iter()
        .find(|s| s.id == revision && &s.destination == destination && s.manifest.change().ok().flatten().is_some())
        .ok_or_else(|| storage("this management action is no longer queued; refresh"))?;
    if pending.iter().any(|s| s.expected_revision.as_deref() == Some(revision)) {
        return Err(storage("later saves depend on this action; synchronize before discarding it"));
    }
    let repository = super::open_repository(connection.clone(), executable, job, access)?;
    repository.refresh()?;
    let _checkout = repository.lock()?;
    let snapshot = repository.snapshot(destination)?;
    if solidcraft_cloud::revision(&snapshot.manifest) == Some(revision) {
        return Err(storage("this action reached the repository; synchronize to confirm it"));
    }
    check_cancelled(job)?;
    outbox.discard_management(save)?;
    outbox.cache_checkout(connection, destination, &snapshot)
}

pub(super) fn trash(outbox: &LocalOutbox, connection: &Connection, executable: &str, job: &Job) -> Result<Vec<Destination>> {
    let mut entries = BTreeMap::new();
    for entry in outbox.cached_entries(connection)? {
        if entry.save.manifest.deleted() && !entry.save.manifest.moved() {
            entries.insert(entry.destination.design_path()?, entry.destination);
        }
    }
    for save in outbox.pending()? {
        save.check_connection(connection)?;
        if save.manifest.deleted() {
            entries.insert(save.destination.design_path()?, save.destination);
        }
    }
    match GitRepository::open_cancellable(connection.clone(), executable, job.clone()).and_then(|repo| repo.list_deleted()) {
        Ok(remote) => {
            for destination in remote {
                entries.insert(destination.design_path()?, destination);
            }
        }
        Err(Error::Storage(_)) if !entries.is_empty() => {}
        Err(error) => return Err(error),
    }
    Ok(entries.into_values().collect())
}
