//! Durable native orchestration for hosting APIs. Credentials are requested only for network work.
use super::*;
use crate::cloud::{
    LocalOutbox, NativeAccess,
    host::{check_cancelled, pending_revision, pending_tip},
    prepare_change,
};
use solidcraft_cloud::{
    Error,
    jobs::{Job, Operation, Phase},
    management::Change,
};
use std::collections::BTreeMap;

fn storage(message: impl std::fmt::Display) -> Error {
    Error::Storage(message.to_string())
}

fn cached(outbox: &LocalOutbox, connection: &Connection, destination: &Destination) -> Result<Option<Snapshot>> {
    let Some(entry) = outbox.cached(connection, destination)? else { return Ok(None) };
    if entry.outdated {
        return Ok(None);
    }
    let mut snapshot = outbox.recover_document(&entry.save)?;
    if let Some(revision) = entry.revision {
        snapshot.manifest.extra.insert("revision".into(), revision.into());
    }
    Ok(Some(snapshot))
}

fn current(outbox: &LocalOutbox, connection: &Connection, destination: &Destination, job: &Job, access: &dyn NativeAccess) -> Result<Snapshot> {
    if let Some(save) = pending_tip(outbox, connection, destination)? {
        let mut snapshot = outbox.recover_document(&save)?;
        snapshot.manifest.extra.insert("revision".into(), save.id.into());
        return Ok(snapshot);
    }
    match access.provider_repository(connection, job).and_then(|mut repository| repository.open(destination, outbox)) {
        Ok(snapshot) => Ok(snapshot),
        Err(error @ Error::Storage(_)) => cached(outbox, connection, destination)?.ok_or(error),
        Err(error) => Err(error),
    }
}

pub(in crate::cloud) fn run(operation: Operation, outbox: &LocalOutbox, job: &Job, access: &dyn NativeAccess) -> Result<()> {
    let connection = super::connection(&operation).ok_or_else(|| storage("invalid provider operation"))?.clone();
    check_cancelled(job)?;
    if let Operation::Open { destination, .. }
    | Operation::Thumbnail { destination, .. }
    | Operation::History { destination, .. }
    | Operation::Manage { destination, .. }
    | Operation::Discard { destination, .. } = &operation
    {
        destination.validate()?;
        if destination.connection != connection.id {
            return Err(storage("repository connection mismatch"));
        }
    }
    match operation {
        Operation::Setup(connection) => {
            let connection = access.prepare(connection, job)?;
            connection.validate()?;
            check_cancelled(job)?;
            job.update(|s| {
                s.result = Some(serde_json::json!(connection));
                s.phase = Phase::Ready;
            })?;
        }
        Operation::Test(connection) => {
            access.provider_repository(&connection, job)?.test()?;
            check_cancelled(job)?;
            job.update(|s| {
                s.result = Some(serde_json::json!(connection));
                s.phase = Phase::Ready;
            })?;
        }
        Operation::Save { connection, save, synchronize: upload } => {
            save.record.check_connection(&connection)?;
            super::super::check_document(
                save.objects.get(&format!("documents/{}", save.record.manifest.document_hash)).ok_or_else(|| storage("captured document missing"))?,
            )?;
            let change = save.record.manifest.change()?;
            outbox.stage(&save)?;
            job.update(|s| {
                s.saved_locally = true;
                s.destination = Some(save.record.destination.clone());
                s.revision = Some(save.record.id.clone());
                s.change = change;
                s.phase = if upload { Phase::Syncing } else { Phase::SavedLocally };
            })?;
            if upload {
                synchronize(outbox, &connection, job, access, Some(save.record))?;
            }
        }
        Operation::Synchronize(connection) => synchronize(outbox, &connection, job, access, None)?,
        Operation::List(connection) => {
            let entries = outbox.cached_entries(&connection)?;
            let pending = pending(outbox, &connection)?;
            let mut listing = solidcraft_cloud::Listing::new(entries, pending, job)?;
            let remote = access.provider_repository(&connection, job).and_then(|mut repository| repository.list_cached(&mut listing.catalog, outbox));
            listing.merge(remote, job)?;
            for entry in listing.catalog.iter().filter(|e| e.outdated) {
                outbox.mark_outdated(&connection, entry)?;
            }
            listing.finish(job)?;
        }
        Operation::Open { connection, destination, local_revision } => {
            let snapshot = if let Some(save) = pending_revision(outbox, &connection, &destination, local_revision.as_deref())? {
                let mut snapshot = outbox.recover_document(&save)?;
                snapshot.manifest.extra.insert("revision".into(), save.id.into());
                snapshot
            } else {
                let remote = (|| {
                    let mut repository = access.provider_repository(&connection, job)?;
                    if let Some(entry) = outbox.cached(&connection, &destination)? {
                        let stored = solidcraft_cloud::stored_designs(vec![entry.clone()], Vec::new())?;
                        if entry.confirmed
                            && !entry.outdated
                            && let Some(stored) = stored.first()
                        {
                            if repository.is_current(stored, outbox)? {
                                return cached(outbox, &connection, &destination)?.ok_or_else(|| storage("cached design missing"));
                            }
                            let mut observed = stored.clone();
                            observed.outdated = true;
                            observed.phase = Phase::Ready;
                            outbox.mark_outdated(&connection, &observed)?;
                        }
                    }
                    let snapshot = repository.open(&destination, outbox)?;
                    outbox.cache_synchronized_checkout(&connection, &destination, &snapshot)?;
                    Ok(snapshot)
                })();
                match remote {
                    Ok(snapshot) => snapshot,
                    Err(error @ Error::Storage(_)) => {
                        job.update(|s| s.message = Some(error.to_string()))?;
                        cached(outbox, &connection, &destination)?.ok_or(error)?
                    }
                    Err(error) => return Err(error),
                }
            };
            let snapshot = if let Some(save) = pending_revision(outbox, &connection, &destination, local_revision.as_deref())? {
                let mut snapshot = outbox.recover_document(&save)?;
                snapshot.manifest.extra.insert("revision".into(), save.id.into());
                snapshot
            } else {
                snapshot
            };
            check_cancelled(job)?;
            job.update(|s| {
                s.destination = Some(destination);
                s.revision = solidcraft_cloud::revision(&snapshot.manifest).map(str::to_owned);
            })?;
            job.opened(snapshot)?;
        }
        Operation::Thumbnail { connection, destination } => {
            let local = pending_tip(outbox, &connection, &destination)?;
            let png = if let Some(save) = local {
                super::super::previews::captured(outbox, &save)?
            } else if let Some(entry) = outbox.cached(&connection, &destination)? {
                super::super::previews::captured(outbox, &entry.save)?
            } else {
                match access.provider_repository(&connection, job).and_then(|mut repository| repository.thumbnail(&destination)) {
                    Ok(png) => png,
                    Err(error @ Error::Storage(_)) => {
                        let entry = outbox.cached(&connection, &destination)?.ok_or(error)?;
                        super::super::previews::captured(outbox, &entry.save)?
                    }
                    Err(error) => return Err(error),
                }
            };
            check_cancelled(job)?;
            job.update(|s| {
                s.result = Some(solidcraft_cloud::Thumbnail::response(png));
                s.phase = Phase::Ready;
            })?;
        }
        Operation::History { connection, destination } => {
            let mut history = match access.provider_repository(&connection, job).and_then(|mut repository| repository.history(&destination)) {
                Ok(history) => history,
                Err(error @ Error::Storage(_)) => {
                    let entry = outbox.cached(&connection, &destination)?.ok_or_else(|| storage(&error))?;
                    job.update(|s| s.message = Some(error.to_string()))?;
                    History { revision: entry.revision, revisions: Vec::new(), deleted: entry.save.manifest.deleted(), queued: false }
                }
                Err(error) => return Err(error),
            };
            if let Some(save) = pending_tip(outbox, &connection, &destination)? {
                history.queued = save.manifest.change()?.is_some();
                history.revision = Some(save.id);
            }
            job.update(|s| {
                s.result = Some(serde_json::json!(history));
                s.phase = Phase::Ready;
            })?;
        }
        Operation::Trash(connection) => {
            let mut entries = BTreeMap::new();
            for entry in outbox.cached_entries(&connection)? {
                if entry.save.manifest.deleted() && !entry.save.manifest.moved() {
                    entries.insert(entry.destination.design_path()?, entry.destination);
                }
            }
            for save in pending(outbox, &connection)? {
                if save.manifest.deleted() {
                    entries.insert(save.destination.design_path()?, save.destination);
                }
            }
            match access.provider_repository(&connection, job).and_then(|mut repository| repository.trash()) {
                Ok(remote) => {
                    for destination in remote {
                        entries.insert(destination.design_path()?, destination);
                    }
                }
                Err(Error::Storage(_)) if !entries.is_empty() => {}
                Err(error) => return Err(error),
            }
            job.update(|s| {
                s.result = Some(serde_json::json!(entries.into_values().collect::<Vec<_>>()));
                s.phase = Phase::Ready;
            })?;
        }
        Operation::Manage { connection, destination, expected_revision, change } => {
            change.validate(&destination)?;
            if pending_tip(outbox, &connection, &destination)?.is_some_and(|s| s.manifest.change().ok().flatten().is_some()) {
                return Err(storage("synchronize this design's pending management action first"));
            }
            let current = current(outbox, &connection, &destination, job, access)?;
            if solidcraft_cloud::revision(&current.manifest) != expected_revision.as_deref() {
                return Err(Error::Conflict);
            }
            let paths = current.manifest.history_paths(&destination)?;
            let mut snapshot = if let Change::Restore { commit, file } = &change {
                access.provider_repository(&connection, job)?.open_at(&Destination { file: file.clone(), ..destination.clone() }, commit)?
            } else {
                current
            };
            if matches!(change, Change::Restore { .. }) {
                snapshot.manifest.extra.insert(
                    "previous_paths".into(),
                    serde_json::json!(paths.into_iter().filter(|p| p != &destination.file).take(16).collect::<Vec<_>>()),
                );
            }
            let snapshot = prepare_change(snapshot, &destination, &change)?;
            let save = PendingSave::stage(&connection, &destination.project, &destination.file, expected_revision, snapshot)?;
            run(Operation::Save { connection, save: Box::new(save), synchronize: true }, outbox, job, access)?;
        }
        Operation::Discard { connection, destination, revision } => {
            let _lock = outbox.wait_for_synchronization(job)?;
            let pending = pending(outbox, &connection)?;
            let save = pending
                .iter()
                .find(|s| s.id == revision && s.destination == destination && s.manifest.change().ok().flatten().is_some())
                .ok_or_else(|| storage("this management action is no longer queued; refresh"))?;
            if pending.iter().any(|s| s.expected_revision.as_deref() == Some(&revision)) {
                return Err(storage("later saves depend on this action; synchronize before discarding it"));
            }
            let snapshot = access.provider_repository(&connection, job)?.open(&destination, outbox)?;
            if solidcraft_cloud::revision(&snapshot.manifest) == Some(&revision) {
                return Err(storage("this action reached the repository; synchronize to confirm it"));
            }
            check_cancelled(job)?;
            outbox.discard_management(save)?;
            outbox.cache_synchronized_checkout(&connection, &destination, &snapshot)?;
            job.update(|s| {
                s.result = Some(serde_json::json!({"discarded":revision}));
                s.phase = Phase::Synced;
            })?;
        }
        _ => return Err(storage("unsupported provider operation")),
    }
    Ok(())
}

fn pending(outbox: &LocalOutbox, connection: &Connection) -> Result<Vec<PendingSave>> {
    let mut saves = Vec::new();
    for save in outbox.pending()? {
        save.check_connection(connection)?;
        if outbox.receipt(&save)?.is_none() {
            saves.push(save);
        }
    }
    Ok(saves)
}

pub(in crate::cloud) fn refresh(outbox: &LocalOutbox, connection: &Connection, job: &Job, repository: &mut dyn ProviderRepository) -> Result<()> {
    let mut catalog = solidcraft_cloud::stored_designs(outbox.cached_entries(connection)?, Vec::new())?;
    repository.list_cached(&mut catalog, outbox)?;
    for observed in catalog {
        check_cancelled(job)?;
        if pending_tip(outbox, connection, &observed.destination)?.is_some() {
            continue;
        }
        if !observed.outdated && observed.phase == Phase::Synced {
            continue;
        }
        outbox.mark_outdated(connection, &observed)?;
        let snapshot = repository.open(&observed.destination, outbox)?;
        outbox.cache_synchronized_checkout(connection, &observed.destination, &snapshot)?;
    }
    Ok(())
}

fn synchronize(outbox: &LocalOutbox, connection: &Connection, job: &Job, access: &dyn NativeAccess, target: Option<PendingSave>) -> Result<()> {
    super::super::sync::synchronize(outbox, connection, "", job, Some(access), target)
}
