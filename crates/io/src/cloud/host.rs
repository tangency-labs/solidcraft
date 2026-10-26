//! Native asynchronous adapter. Saves reach disk before any Git or network operation.
//! Each connection has an independent queue and local synchronization lock.
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use solidcraft_cloud::{
    Connection, Error, PendingSave, Result,
    jobs::{Host, Job, Operation, Phase},
};

use super::{GitRepository, LocalOutbox};
mod management;

pub struct NativeHost {
    root: PathBuf,
    access: Option<std::sync::Arc<dyn super::NativeAccess>>,
}

fn storage(error: impl std::fmt::Display) -> Error {
    Error::Storage(error.to_string())
}

fn check_cancelled(job: &Job) -> Result<()> {
    if job.cancelled()? { Err(storage("operation cancelled; durable saves retained")) } else { Ok(()) }
}

impl NativeHost {
    pub fn new(root: PathBuf) -> Result<Self> {
        Ok(Self { root: LocalOutbox::new(root)?.root().to_path_buf(), access: None })
    }

    pub fn with_access(mut self, access: std::sync::Arc<dyn super::NativeAccess>) -> Self {
        self.access = Some(access);
        self
    }

    fn run(
        operation: Operation,
        executable: &str,
        outbox: &LocalOutbox,
        job: &Job,
        access: Option<&dyn super::NativeAccess>,
        root: &std::path::Path,
    ) -> Result<()> {
        check_cancelled(job)?;
        match operation {
            Operation::Discard { connection, destination, revision } => {
                management::discard(outbox, &connection, &destination, &revision, executable, job, access)?;
                job.update(|s| {
                    s.result = Some(serde_json::json!({"discarded":revision}));
                    s.phase = Phase::Synced;
                })?;
            }
            Operation::Manage { connection, destination, expected_revision, change } => {
                let save = management::stage(outbox, &connection, &destination, expected_revision, &change, executable, job)?;
                Self::run(Operation::Save { connection, save: Box::new(save), synchronize: true }, executable, outbox, job, access, root)?;
            }
            Operation::Trash(connection) => {
                let entries = management::trash(outbox, &connection, executable, job)?;
                job.update(|s| {
                    s.result = Some(serde_json::json!(entries));
                    s.phase = Phase::Ready;
                })?;
            }
            Operation::History { connection, destination } => {
                let mut history =
                    match GitRepository::open_cancellable(connection.clone(), executable, job.clone()).and_then(|r| r.history(&destination)) {
                        Ok(result) => result,
                        Err(error @ Error::Storage(_)) => {
                            let Some(cached) = outbox.cached(&connection, &destination)? else { return Err(error) };
                            job.update(|s| s.message = Some(error.to_string()))?;
                            solidcraft_cloud::management::History {
                                revision: cached.revision,
                                revisions: Vec::new(),
                                queued: false,
                                deleted: cached.save.manifest.deleted(),
                            }
                        }
                        Err(error) => return Err(error),
                    };
                if let Some(pending) = pending_tip(outbox, &connection, &destination)? {
                    history.queued = pending.manifest.change()?.is_some();
                    history.revision = Some(pending.id);
                }
                job.update(|s| {
                    s.result = Some(serde_json::json!(history));
                    s.phase = Phase::Ready;
                })?;
            }
            Operation::Thumbnail { connection, destination } => {
                let png = super::previews::read(connection, destination, executable, outbox, job)?;
                check_cancelled(job)?;
                job.update(|status| {
                    status.result = Some(solidcraft_cloud::Thumbnail::response(png));
                    status.phase = Phase::Ready;
                })?;
            }
            Operation::GenerateSshKey(input) => {
                let key = super::ssh::generate(outbox.root(), input, job, access)?;
                job.update(|status| {
                    status.result = Some(serde_json::json!(key));
                    status.phase = Phase::Ready;
                })?;
            }
            Operation::LoadSettings => {
                let settings = outbox.load_settings()?.settings()?;
                job.update(|status| {
                    status.result = Some(serde_json::json!(settings));
                    status.phase = Phase::Ready;
                })?;
            }
            Operation::Configure(settings) => {
                outbox.configure(&settings)?;
                // Like publication, a committed settings change cannot be undone by a
                // cancellation racing its acknowledgement. Cancel in Preferences discards
                // the draft before this operation is submitted.
                job.update(|status| {
                    status.result = Some(serde_json::json!(settings));
                    status.phase = Phase::Synced;
                })?;
            }
            Operation::Setup(connection) => {
                let access = access.ok_or_else(|| storage("provider setup is unavailable"))?;
                let connection = access.prepare(connection, job)?;
                connection.validate()?;
                check_cancelled(job)?;
                // Account discovery resolves a provisional setup id. Use the final
                // repository's private directory for cloning and reconnecting.
                let outbox = LocalOutbox::new(root.join(solidcraft_cloud::digest(connection.id.as_bytes())))?;
                let lock = outbox.synchronization_lock()?;
                let token = access.credential(&connection.credential)?;
                let connection = super::provision::checkout(connection, executable, outbox.root(), token, job)?;
                check_cancelled(job)?;
                drop(lock);
                job.update(|status| {
                    status.result = Some(serde_json::json!(connection));
                    status.phase = Phase::Ready;
                })?;
            }
            Operation::Save { connection, save, synchronize } => {
                save.record.check_connection(&connection)?;
                let document = save
                    .objects
                    .get(&format!("documents/{}", save.record.manifest.document_hash))
                    .ok_or_else(|| storage("captured document is missing"))?;
                super::check_document(document)?;
                let change = save.record.manifest.change()?;
                job.update(|status| {
                    status.destination = Some(save.record.destination.clone());
                    status.revision = Some(save.record.id.clone());
                    status.change = change.clone();
                })?;
                outbox.stage(&save)?;
                job.update(|status| {
                    status.change = change;
                    status.saved_locally = true;
                    status.phase = if synchronize { Phase::Syncing } else { Phase::SavedLocally };
                    status.destination = Some(save.record.destination.clone());
                    status.revision = Some(save.record.id.clone());
                })?;
                if synchronize {
                    Self::synchronize(connection, executable, outbox, job, Some(save.record), access)?;
                }
            }
            Operation::Synchronize(connection) => Self::synchronize(connection, executable, outbox, job, None, access)?,
            Operation::Test(connection) => {
                let connection = open_repository(connection, executable, job, access)?.verified_connection()?;
                job.update(|status| {
                    status.result = Some(serde_json::json!(connection));
                    status.phase = Phase::Ready;
                })?;
            }
            Operation::List(connection) => {
                let cached = outbox.cached_entries(&connection)?;
                let mut pending = Vec::new();
                for save in outbox.pending()? {
                    save.check_connection(&connection)?;
                    if outbox.receipt(&save)?.is_none() {
                        pending.push(save);
                    }
                }
                let has_local = !cached.is_empty() || !pending.is_empty();
                let hidden: Vec<_> = pending.iter().filter(|s| s.manifest.deleted()).map(|s| s.destination.clone()).collect();
                let mut catalog = solidcraft_cloud::stored_designs(cached, pending)?;
                let mut destinations = BTreeMap::new();
                for entry in &catalog {
                    destinations.insert(entry.destination.design_path()?, entry.destination.clone());
                }
                match GitRepository::open_cancellable(connection.clone(), executable, job.clone()).and_then(|repository| {
                    let entries = repository.list()?;
                    for entry in catalog.iter_mut().filter(|e| matches!(e.phase, Phase::Ready | Phase::Synced) && !e.outdated) {
                        if !repository.matches_stored(entry).unwrap_or(false) {
                            entry.phase = Phase::Ready;
                            entry.outdated = true;
                            outbox.mark_outdated(&connection, entry)?;
                        }
                    }
                    Ok(entries)
                }) {
                    Ok(entries) => {
                        for destination in entries {
                            destinations.insert(destination.design_path()?, destination);
                        }
                    }
                    Err(error @ Error::Storage(_)) if has_local => job.update(|status| status.message = Some(error.to_string()))?,
                    Err(error) => return Err(error),
                }
                check_cancelled(job)?;
                for destination in hidden {
                    destinations.remove(&destination.design_path()?);
                }
                let result = serde_json::to_value(destinations.into_values().collect::<Vec<_>>()).map_err(storage)?;
                job.update(|status| {
                    status.result = Some(result);
                    status.catalog = catalog;
                    status.phase = Phase::Ready;
                })?;
            }
            Operation::Open { connection, destination, local_revision } => {
                destination.validate()?;
                if destination.connection != connection.id {
                    return Err(storage("repository connection mismatch"));
                }
                let mut snapshot = if let Some(save) = pending_revision(outbox, &connection, &destination, local_revision.as_deref())? {
                    let snapshot = outbox.recover_document(&save)?;
                    job.update(|status| status.revision = Some(save.id.clone()))?;
                    snapshot
                } else {
                    // Reading an ordinary checkout needs no connectivity and never fetches.
                    match GitRepository::open_cancellable(connection.clone(), executable, job.clone()).and_then(|repo| {
                        let lock = repo.lock()?;
                        Ok((repo.snapshot(&destination)?, lock))
                    }) {
                        Ok((snapshot, _lock)) => {
                            // Cache failures must be reported, never mistaken for an unavailable
                            // checkout and silently replaced with an older cached snapshot.
                            outbox.cache_checkout(&connection, &destination, &snapshot)?;
                            job.update(|status| status.revision = solidcraft_cloud::revision(&snapshot.manifest).map(str::to_owned))?;
                            snapshot
                        }
                        Err(error @ Error::Storage(_)) => {
                            let Some(cached) = outbox.cached(&connection, &destination)? else { return Err(error) };
                            let snapshot = outbox.recover_document(&cached.save)?;
                            job.update(|status| {
                                status.revision = cached.revision;
                                status.message = Some(error.to_string());
                            })?;
                            snapshot
                        }
                        Err(error) => return Err(error),
                    }
                };
                // A durable save can arrive while the checkout is read. As in the browser,
                // select that local head before exposing the opening result.
                if let Some(save) = pending_revision(outbox, &connection, &destination, local_revision.as_deref())?
                    && job.status()?.revision.as_deref() != Some(save.id.as_str())
                {
                    snapshot = outbox.recover_document(&save)?;
                    job.update(|status| status.revision = Some(save.id))?;
                }
                check_cancelled(job)?;
                job.update(|status| status.destination = Some(destination))?;
                job.opened(snapshot)?;
            }
        }
        Ok(())
    }

    fn synchronize(
        connection: Connection,
        executable: &str,
        outbox: &LocalOutbox,
        job: &Job,
        target: Option<PendingSave>,
        access: Option<&dyn super::NativeAccess>,
    ) -> Result<()> {
        check_cancelled(job)?;
        let lock = outbox.wait_for_synchronization(job)?;
        let mut pending = outbox.pending()?;
        // Another completed worker may have removed this record after its caller staged the
        // same capture. Publishing it again is idempotent and confirms its own result.
        if let Some(target) = &target
            && !pending.iter().any(|save| save.id == target.id)
        {
            pending.push(target.clone());
            pending = PendingSave::ordered(pending)?;
        }
        if pending.is_empty() {
            super::receive::refresh(&connection, executable, outbox, job, access)?;
            drop(lock);
            job.update(|status| {
                status.phase = Phase::Synced;
                status.result = Some(serde_json::json!([]));
            })?;
            return Ok(());
        }
        // Completed receipts can acknowledge an interrupted queue removal without Git,
        // credentials, a checkout, or a second network publication.
        let mut repository = None;
        job.update(|status| status.phase = Phase::Syncing)?;
        let mut blocked = BTreeSet::new();
        let mut results = Vec::new();
        let mut first_error = None;
        for save in pending {
            check_cancelled(job)?;
            if save.expected_revision.as_ref().is_some_and(|parent| blocked.contains(parent)) {
                blocked.insert(save.id);
                continue;
            }
            let result = save
                .check_connection(&connection)
                .and_then(|()| {
                    if let Some(published) = outbox.receipt(&save)? {
                        return Ok(published);
                    }
                    let snapshot = outbox.recover_document(&save)?;
                    let parent = outbox.published_parent(&save)?;
                    if repository.is_none() {
                        repository = Some(open_repository(connection.clone(), executable, job, access)?);
                    }
                    repository.as_ref().ok_or_else(|| storage("repository unavailable"))?.publish_with_parent(&save, snapshot, parent.as_ref())
                })
                .and_then(|published| {
                    outbox.published(&save, &published)?;
                    Ok(published)
                });
            match result {
                Ok(published) => {
                    let managed = solidcraft_cloud::management::Managed::confirmed(&save, &published)?;
                    let result = serde_json::to_value(&published).map_err(storage)?;
                    job.update(|status| {
                        status.publications.push(published.clone());
                        status.managed.extend(managed);
                    })?;
                    if target.as_ref().is_some_and(|target| target.id == save.id) {
                        // A Save job belongs to this capture alone, never whichever other
                        // queued design happened to finish last during synchronization.
                        drop(lock);
                        job.update(|status| {
                            status.destination = Some(published.destination);
                            status.revision = Some(published.revision);
                            status.result = Some(result);
                            status.phase = Phase::Synced;
                        })?;
                        return Ok(());
                    }
                    results.push(result);
                }
                Err(error) => {
                    blocked.insert(save.id);
                    if first_error.is_none() {
                        first_error = Some(error);
                    }
                }
            }
        }
        job.update(|status| status.result = Some(serde_json::Value::Array(results)))?;
        if let Some(error) = first_error {
            return Err(error);
        }
        if target.is_none() {
            super::receive::refresh(&connection, executable, outbox, job, access)?;
        }
        // A caller can immediately submit its next save or setup after this acknowledgement.
        drop(lock);
        job.update(|status| status.phase = Phase::Synced)
    }
}

pub(super) fn pending_tip(outbox: &LocalOutbox, connection: &Connection, destination: &solidcraft_cloud::Destination) -> Result<Option<PendingSave>> {
    pending_revision(outbox, connection, destination, None)
}

fn pending_revision(
    outbox: &LocalOutbox,
    connection: &Connection,
    destination: &solidcraft_cloud::Destination,
    revision: Option<&str>,
) -> Result<Option<PendingSave>> {
    let mut pending = Vec::new();
    for save in outbox.pending()?.into_iter().filter(|save| &save.destination == destination) {
        save.check_connection(connection)?;
        if outbox.receipt(&save)?.is_none() {
            pending.push(save);
        }
    }
    PendingSave::local_head(pending, destination, revision)
}

pub(super) fn open_repository(
    connection: Connection,
    executable: &str,
    job: &Job,
    access: Option<&dyn super::NativeAccess>,
) -> Result<GitRepository> {
    let token = if connection.provider == "github"
        && connection.authentication == solidcraft_cloud::Authentication::Https
        && !connection.credential.is_empty()
    {
        access.map(|access| access.credential(&connection.credential)).transpose()?.flatten()
    } else {
        None
    };
    let askpass = match (&connection.ssh_identity, access) {
        (solidcraft_cloud::SshIdentity::File(path), Some(access)) => access.ssh_askpass(path)?,
        _ => None,
    };
    GitRepository::authenticated(connection, executable, job.clone(), token).map(|repository| repository.with_ssh_askpass(askpass))
}

impl Host for NativeHost {
    fn store_credential(&self, id: String, token: String) -> Result<Job> {
        solidcraft_cloud::setup::check_token(&token)?;
        if id.is_empty() || id.len() > 4096 || id.chars().any(char::is_control) {
            return Err(storage("invalid credential identifier"));
        }
        let access = self.access.clone().ok_or_else(|| storage("credential storage is unavailable"))?;
        let job = Job::new(String::new())?;
        let worker = job.clone();
        std::thread::Builder::new()
            .name("solidcraft-cloud-credential".into())
            .spawn(move || {
                let result = access.store_credential(&id, &token).and_then(|()| worker.update(|s| s.phase = Phase::Ready));
                if let Err(error) = result {
                    let _ = worker.fail(error);
                }
            })
            .map_err(storage)?;
        Ok(job)
    }
    fn submit(&self, operation: Operation, git_executable: String) -> Result<Job> {
        let connection = match &operation {
            Operation::LoadSettings | Operation::Configure(_) | Operation::GenerateSshKey(_) => None,
            Operation::Setup(connection)
            | Operation::Test(connection)
            | Operation::List(connection)
            | Operation::Trash(connection)
            | Operation::Synchronize(connection)
            | Operation::Thumbnail { connection, .. }
            | Operation::History { connection, .. }
            | Operation::Manage { connection, .. }
            | Operation::Discard { connection, .. }
            | Operation::Open { connection, .. }
            | Operation::Save { connection, .. } => Some(connection),
        };
        if let Some(connection) = connection {
            connection.validate()?;
        }
        if let Operation::Configure(settings) = &operation {
            settings.validate()?;
        }
        let job = Job::new(connection.map(|c| c.id.clone()).unwrap_or_default())?;
        // Hash identifiers so case-insensitive filesystems and reserved device names cannot
        // alias two distinct connections' private queues.
        let root = connection.map(|c| self.root.join(solidcraft_cloud::digest(c.id.as_bytes()))).unwrap_or_else(|| self.root.clone());
        let outbox = LocalOutbox::new(root)?;
        let access = self.access.clone();
        let root = self.root.clone();
        let worker = job.clone();
        let scope = crate::vfs::current_scope();
        std::thread::Builder::new()
            .name("solidcraft-cloud".into())
            .spawn(move || {
                crate::vfs::scoped(scope, || {
                    if let Err(error) = Self::run(operation, &git_executable, &outbox, &worker, access.as_deref(), &root) {
                        let _ = worker.fail(error);
                    }
                });
            })
            .map_err(storage)?;
        Ok(job)
    }
}

#[cfg(test)]
mod tests;
