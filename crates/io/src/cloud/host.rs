//! Native asynchronous adapter. Saves reach disk before any Git or network operation.
//! Each connection has an independent queue and local synchronization lock.
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use solidcraft_cloud::{
    Connection, Error, PendingSave, Result,
    jobs::{Host, Job, Operation, Phase},
};

use super::{GitRepository, LocalOutbox};

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

    fn run(operation: Operation, executable: &str, outbox: &LocalOutbox, job: &Job, access: Option<&dyn super::NativeAccess>) -> Result<()> {
        check_cancelled(job)?;
        match operation {
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
                let _lock = outbox.synchronization_lock()?;
                let access = access.ok_or_else(|| storage("provider setup is unavailable"))?;
                let connection = access.prepare(connection, job)?;
                check_cancelled(job)?;
                let token = access.credential(&connection.credential)?;
                let connection = super::provision::checkout(connection, executable, outbox.root(), token, job)?;
                check_cancelled(job)?;
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
                outbox.stage(&save)?;
                job.update(|status| {
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
                let mut destinations = BTreeMap::new();
                for destination in outbox.cached_destinations(&connection)? {
                    destinations.insert(destination.design_path()?, destination);
                }
                for save in outbox.pending()? {
                    save.check_connection(&connection)?;
                    if outbox.receipt(&save)?.is_none() {
                        destinations.insert(save.destination.design_path()?, save.destination);
                    }
                }
                match GitRepository::open_cancellable(connection, executable, job.clone()).and_then(|repository| repository.list()) {
                    Ok(entries) => {
                        for destination in entries {
                            destinations.insert(destination.design_path()?, destination);
                        }
                    }
                    Err(error @ Error::Storage(_)) if !destinations.is_empty() => job.update(|status| status.message = Some(error.to_string()))?,
                    Err(error) => return Err(error),
                }
                check_cancelled(job)?;
                let result = serde_json::to_value(destinations.into_values().collect::<Vec<_>>()).map_err(storage)?;
                job.update(|status| {
                    status.result = Some(result);
                    status.phase = Phase::Ready;
                })?;
            }
            Operation::Open { connection, destination } => {
                destination.validate()?;
                if destination.connection != connection.id {
                    return Err(storage("repository connection mismatch"));
                }
                let mut pending = Vec::new();
                for save in outbox.pending()?.into_iter().filter(|save| save.destination == destination) {
                    save.check_connection(&connection)?;
                    if outbox.receipt(&save)?.is_none() {
                        pending.push(save);
                    }
                }
                // Two local sessions can have made competing offline saves. Neither snapshot
                // may silently win; retain both and leave selection to conflict resolution.
                let parents: BTreeSet<_> = pending.iter().filter_map(|save| save.expected_revision.as_deref()).collect();
                let tips: Vec<_> = pending.iter().filter(|save| !parents.contains(save.id.as_str())).collect();
                if tips.len() > 1 {
                    return Err(Error::Conflict);
                }
                let snapshot = if let Some(save) = tips.first() {
                    save.check_connection(&connection)?;
                    let snapshot = outbox.recover_document(save)?;
                    job.update(|status| status.revision = Some(save.id.clone()))?;
                    snapshot
                } else {
                    // Reading an ordinary checkout needs no connectivity and never fetches.
                    match GitRepository::open_cancellable(connection.clone(), executable, job.clone()).and_then(|repo| repo.snapshot(&destination)) {
                        Ok(snapshot) => {
                            job.update(|status| status.revision = solidcraft_cloud::revision(&snapshot.manifest).map(str::to_owned))?;
                            snapshot
                        }
                        Err(error @ Error::Storage(_)) => {
                            let Some(cached) = outbox.cached(&connection, &destination)? else { return Err(error) };
                            let snapshot = outbox.recover_document(&cached.save)?;
                            job.update(|status| {
                                status.revision = Some(cached.published.revision);
                                status.message = Some(error.to_string());
                            })?;
                            snapshot
                        }
                        Err(error) => return Err(error),
                    }
                };
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
        let _lock = outbox.synchronization_lock()?;
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
                    if repository.is_none() {
                        repository = Some(open_repository(connection.clone(), executable, job, access)?);
                    }
                    repository.as_ref().ok_or_else(|| storage("repository unavailable"))?.publish(&save, snapshot)
                })
                .and_then(|published| {
                    outbox.published(&save, &published)?;
                    Ok(published)
                });
            match result {
                Ok(published) => {
                    let result = serde_json::to_value(&published).map_err(storage)?;
                    job.update(|status| status.publications.push(published.clone()))?;
                    if target.as_ref().is_some_and(|target| target.id == save.id) {
                        // A Save job belongs to this capture alone, never whichever other
                        // queued design happened to finish last during synchronization.
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
        job.update(|status| status.phase = Phase::Synced)
    }
}

fn open_repository(connection: Connection, executable: &str, job: &Job, access: Option<&dyn super::NativeAccess>) -> Result<GitRepository> {
    let token = if connection.provider == "github"
        && connection.authentication == solidcraft_cloud::Authentication::Https
        && !connection.credential.is_empty()
    {
        access.map(|access| access.credential(&connection.credential)).transpose()?.flatten()
    } else {
        None
    };
    GitRepository::authenticated(connection, executable, job.clone(), token)
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
            Operation::LoadSettings | Operation::Configure(_) => None,
            Operation::Setup(connection)
            | Operation::Test(connection)
            | Operation::List(connection)
            | Operation::Synchronize(connection)
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
        let worker = job.clone();
        let scope = crate::vfs::current_scope();
        std::thread::Builder::new()
            .name("solidcraft-cloud".into())
            .spawn(move || {
                crate::vfs::scoped(scope, || {
                    if let Err(error) = Self::run(operation, &git_executable, &outbox, &worker, access.as_deref()) {
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
