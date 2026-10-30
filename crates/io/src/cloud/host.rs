//! Native asynchronous adapter. Saves reach disk before any Git or network operation.
//! Each connection has an independent queue and local synchronization lock.
use std::collections::BTreeMap;
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

pub(super) fn check_cancelled(job: &Job) -> Result<()> {
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

    fn spawn(
        &self,
        connection: Option<&Connection>,
        executable: String,
        prepare: impl FnOnce(&Job) -> Result<Operation> + Send + 'static,
    ) -> Result<Job> {
        let job = Job::new(connection.map(|c| c.id.clone()).unwrap_or_default())?;
        let outbox_root = connection.map(|c| self.root.join(solidcraft_cloud::digest(c.id.as_bytes()))).unwrap_or_else(|| self.root.clone());
        let access = self.access.clone();
        let worker = job.clone();
        let scope = crate::vfs::current_scope();
        std::thread::Builder::new()
            .name("solidcraft-cloud".into())
            .spawn(move || {
                crate::vfs::scoped(scope, || {
                    let result = (|| {
                        check_cancelled(&worker)?;
                        let operation = prepare(&worker)?;
                        let outbox = LocalOutbox::new(outbox_root)?;
                        Self::run(operation, &executable, &outbox, &worker, access.as_deref())
                    })();
                    if let Err(error) = result {
                        let _ = worker.fail(error);
                    }
                });
            })
            .map_err(storage)?;
        Ok(job)
    }

    fn run(operation: Operation, executable: &str, outbox: &LocalOutbox, job: &Job, access: Option<&dyn super::NativeAccess>) -> Result<()> {
        check_cancelled(job)?;
        if let Some(access) = access
            && super::provider::connection(&operation).is_some_and(|connection| access.uses_provider_api(connection))
        {
            return super::provider::run(operation, outbox, job, access);
        }
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
                Self::run(Operation::Save { connection, save: Box::new(save), synchronize: true }, executable, outbox, job, access)?;
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
                let mut listing = solidcraft_cloud::Listing::new(cached, pending, job)?;
                let remote = GitRepository::open_cancellable(connection.clone(), executable, job.clone()).and_then(|repository| {
                    let entries = repository.list()?;
                    for entry in listing.catalog.iter_mut().filter(|e| matches!(e.phase, Phase::Ready | Phase::Synced) && !e.outdated) {
                        if !repository.matches_stored(entry).unwrap_or(false) {
                            entry.phase = Phase::Ready;
                            entry.outdated = true;
                            outbox.mark_outdated(&connection, entry)?;
                        }
                    }
                    Ok(entries)
                });
                listing.merge(remote, job)?;
                listing.finish(job)?;
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
        super::sync::synchronize(outbox, &connection, executable, job, access, target)
    }
}

pub(super) fn pending_tip(outbox: &LocalOutbox, connection: &Connection, destination: &solidcraft_cloud::Destination) -> Result<Option<PendingSave>> {
    pending_revision(outbox, connection, destination, None)
}

pub(super) fn pending_revision(
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
    let token = if connection.authentication == solidcraft_cloud::Authentication::Https
        && (connection.provider == "github" || !connection.https_username.is_empty())
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
    fn check_repository(&self, connection: Connection, input: solidcraft_cloud::jobs::RepositoryCheckInput) -> Result<Job> {
        connection.validate()?;
        let access = self.access.clone().ok_or_else(|| storage("repository checking is unavailable"))?;
        let job = Job::new(connection.id.clone())?;
        let worker = job.clone();
        std::thread::Builder::new()
            .name("solidcraft-cloud-check".into())
            .spawn(move || {
                let result = access.check_repository(&connection, input, &worker).and_then(|exists| {
                    worker.update(|s| {
                        s.result = Some(serde_json::json!({"exists":exists}));
                        s.phase = Phase::Ready;
                    })
                });
                if let Err(error) = result {
                    let _ = worker.fail(error);
                }
            })
            .map_err(storage)?;
        Ok(job)
    }

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
    fn prepare_save(
        &self,
        connection: Connection,
        prepare: solidcraft_cloud::jobs::PrepareSave,
        synchronize: bool,
        executable: String,
    ) -> Result<Job> {
        connection.validate()?;
        self.spawn(Some(&connection.clone()), executable, move |job| Ok(Operation::Save { connection, save: Box::new(prepare(job)?), synchronize }))
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
        let connection = connection.cloned();
        self.spawn(connection.as_ref(), git_executable, |_| Ok(operation))
    }
}

#[cfg(test)]
mod tests;
