//! Browser tasks implement the same asynchronous command boundary as native Git workers.
use std::collections::BTreeMap;

use solidcraft_cloud::{
    Connection, Destination, Error, PendingSave, Published, Result, Snapshot,
    github::GitHub,
    jobs::{Host, Job, Operation, Phase},
};

use crate::{
    cloud_settings::LocalSettings,
    cloud_storage::{Store, with_lock},
};

#[path = "cloud_host/management.rs"]
mod management;
#[path = "cloud_host/receive.rs"]
mod receive;

pub struct BrowserHost;

fn storage(error: impl std::fmt::Display) -> Error {
    Error::Storage(error.to_string())
}

fn check_cancelled(job: &Job) -> Result<()> {
    if job.cancelled()? { Err(storage("operation cancelled; local captures retained")) } else { Ok(()) }
}

async fn repository(connection: &Connection, job: &Job) -> Result<GitHub> {
    if connection.provider != "github" {
        return Err(storage("this browser supports GitHub repositories; use the desktop app for other Git remotes"));
    }
    let token = if connection.credential.is_empty() { None } else { LocalSettings::open().await?.credential(&connection.credential).await? };
    let client = crate::cloud_client::client_for_job(token.as_deref(), Some(job.clone()))
        .map_err(|_| storage("GitHub client unavailable; reconnect this repository"))?;
    GitHub::new(client, connection)
}

async fn download(remote: &GitHub, store: &Store, destination: &Destination, job: &Job) -> Result<Snapshot> {
    remote
        .open_cached(destination, solidcraft_engine::io::cloud::check_document, store, |done, total| {
            check_cancelled(job)?;
            job.update(|status| status.download_progress = Some((done, total)))
        })
        .await
}

async fn recovered(store: &Store, save: &PendingSave) -> Result<Snapshot> {
    store.recover_checked(save, solidcraft_engine::io::cloud::check_document).await
}

async fn pending_tip(store: &Store, destination: &Destination) -> Result<Option<PendingSave>> {
    PendingSave::local_head(store.pending().await?, destination, None)
}

async fn open(store: &Store, connection: &Connection, destination: &Destination, local_revision: Option<&str>, job: &Job) -> Result<()> {
    destination.validate()?;
    if destination.connection != connection.id {
        return Err(storage("repository connection mismatch"));
    }
    if let Some(save) = PendingSave::local_head(store.pending().await?, destination, local_revision)? {
        let snapshot = recovered(store, &save).await?;
        check_cancelled(job)?;
        job.update(|status| {
            status.destination = Some(destination.clone());
            status.revision = Some(save.id);
        })?;
        return job.opened(snapshot);
    }
    let remote = async {
        let remote = repository(connection, job).await?;
        if let Some(cached) = store.cached(destination).await? {
            let stored = solidcraft_cloud::stored_designs(vec![cached.clone()], Vec::new())?;
            if cached.confirmed
                && !cached.outdated
                && let Some(stored) = stored.first()
            {
                if remote.is_current_cached(stored, store).await? {
                    return Ok(None);
                }
                let mut observed = stored.clone();
                observed.outdated = true;
                observed.phase = Phase::Ready;
                store.mark_outdated(&observed).await?;
            }
        }
        download(&remote, store, destination, job).await.map(Some)
    }
    .await;
    match remote {
        Ok(snapshot) => {
            check_cancelled(job)?;
            if let Some(snapshot) = snapshot {
                store.cache_remote(destination.clone(), snapshot).await?;
            }
        }
        Err(error @ Error::Storage(_)) => {
            if store.cached(destination).await?.is_none_or(|entry| entry.outdated) {
                return Err(error);
            }
            job.update(|status| status.message = Some(error.to_string()))?;
        }
        Err(error) => return Err(error),
    }
    // A local save can have arrived during the download. cache_remote preserves that head;
    // read the selected cache instead of replacing it with our older in-flight response.
    let cached = store.cached(destination).await?.ok_or_else(|| storage("cached design missing"))?;
    let pending = pending_tip(store, destination).await?;
    let snapshot = recovered(store, pending.as_ref().unwrap_or(&cached.save)).await?;
    check_cancelled(job)?;
    job.update(|status| {
        status.destination = Some(cached.destination);
        status.revision = pending.map(|save| save.id).or(cached.revision);
    })?;
    job.opened(snapshot)
}

async fn synchronize(connection: Connection, store: Store, job: Job, target: Option<PendingSave>) -> Result<()> {
    let lock = format!("solidcraft-cloud-sync-{}", solidcraft_cloud::digest(connection.id.as_bytes()));
    with_lock(&lock, async move {
        let mut backend = BrowserSync { connection: &connection, store: &store, job: &job, remote: None };
        solidcraft_cloud::sync::synchronize(&mut backend, &connection, &job, target).await
    })
    .await
}

struct BrowserSync<'a> {
    connection: &'a Connection,
    store: &'a Store,
    job: &'a Job,
    remote: Option<GitHub>,
}
impl solidcraft_cloud::sync::Backend for BrowserSync<'_> {
    async fn pending(&mut self) -> Result<Vec<PendingSave>> {
        self.store.pending().await
    }
    async fn receipt(&mut self, save: &PendingSave) -> Result<Option<Published>> {
        self.store.receipt(&save.id).await
    }
    async fn parent(&mut self, save: &PendingSave) -> Result<Option<Published>> {
        match &save.expected_revision {
            Some(id) => self.store.receipt(id).await,
            None => Ok(None),
        }
    }
    async fn publish(&mut self, save: &PendingSave, parent: Option<&Published>) -> Result<Published> {
        if self.remote.is_none() {
            self.remote = Some(repository(self.connection, self.job).await?);
        }
        self.remote
            .as_ref()
            .ok_or_else(|| storage("repository unavailable"))?
            .publish_with_parent(
                save,
                || recovered(self.store, save),
                |bytes| {
                    check_cancelled(self.job)?;
                    solidcraft_engine::io::cloud::check_document(bytes)
                },
                parent,
            )
            .await
    }
    async fn confirm(&mut self, save: &PendingSave, published: &Published) -> Result<()> {
        self.store.confirm(save.clone(), published.clone()).await
    }
    async fn refresh(&mut self) -> Result<()> {
        receive::refresh(self.connection, self.store, self.job).await
    }
}

async fn run(operation: Operation, job: &Job) -> Result<()> {
    check_cancelled(job)?;
    let operation = match operation {
        Operation::GenerateSshKey(_) => return Err(Error::Storage("SSH key generation requires the desktop app".into())),
        Operation::LoadSettings => {
            let settings = LocalSettings::open().await?.settings().await?;
            return job.update(|status| {
                status.result = Some(serde_json::json!(settings));
                status.phase = Phase::Ready;
            });
        }
        Operation::Setup(connection) => {
            if connection.provider != "github" {
                return Err(storage("unsupported setup provider"));
            }
            let token = LocalSettings::open()
                .await?
                .credential(&connection.credential)
                .await?
                .ok_or_else(|| storage("paste a repository token to connect"))?;
            let client = crate::cloud_client::client_for_job(Some(&token), Some(job.clone())).map_err(|_| storage("GitHub client unavailable"))?;
            let mut connection = solidcraft_cloud::github::prepare(client, connection).await?;
            connection.checkout.clear();
            check_cancelled(job)?;
            return job.update(|status| {
                status.result = Some(serde_json::json!(connection));
                status.phase = Phase::Ready;
            });
        }
        Operation::Configure(settings) => {
            LocalSettings::open().await?.configure(settings.clone()).await?;
            return job.update(|status| {
                status.result = Some(serde_json::json!(settings));
                status.phase = Phase::Synced;
            });
        }
        operation => operation,
    };
    let connection = match &operation {
        Operation::Test(connection)
        | Operation::List(connection)
        | Operation::Trash(connection)
        | Operation::Synchronize(connection)
        | Operation::Thumbnail { connection, .. }
        | Operation::History { connection, .. }
        | Operation::Manage { connection, .. }
        | Operation::Discard { connection, .. }
        | Operation::Open { connection, .. }
        | Operation::Save { connection, .. } => connection.clone(),
        _ => return Err(storage("invalid repository operation")),
    };
    let store = Store::open(connection.clone()).await?;
    job.update(|status| status.storage_persistent = Some(store.persistent))?;
    check_cancelled(job)?;
    match operation {
        Operation::Discard { connection, destination, revision } => {
            management::discard(store, connection, destination, revision.clone(), job.clone()).await?;
            job.update(|s| {
                s.result = Some(serde_json::json!({"discarded":revision}));
                s.phase = Phase::Synced;
            })?;
        }
        Operation::Manage { connection, destination, expected_revision, change } => {
            let save = management::stage(&store, &connection, &destination, expected_revision, &change, job).await?;
            job.update(|status| {
                status.destination = Some(save.record.destination.clone());
                status.revision = Some(save.record.id.clone());
                status.change = Some(change);
            })?;
            let record = store.stage(save).await?;
            let change = record.manifest.change()?;
            job.update(|status| {
                status.change = change;
                status.saved_locally = true;
                status.phase = Phase::Syncing;
                status.destination = Some(record.destination.clone());
                status.revision = Some(record.id.clone());
            })?;
            synchronize(connection, store, job.clone(), Some(record)).await?;
        }
        Operation::Trash(connection) => {
            let entries = management::trash(&store, &connection, job).await?;
            job.update(|s| {
                s.result = Some(serde_json::json!(entries));
                s.phase = Phase::Ready;
            })?;
        }
        Operation::History { connection, destination } => {
            let mut history =
                match async { repository(&connection, job).await?.history(&destination, solidcraft_engine::io::cloud::check_document).await }.await {
                    Ok(result) => result,
                    Err(error @ Error::Storage(_)) => {
                        let Some(cached) = store.cached(&destination).await? else { return Err(error) };
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
            if let Some(pending) = pending_tip(&store, &destination).await? {
                history.queued = pending.manifest.change()?.is_some();
                history.revision = Some(pending.id);
            }
            job.update(|s| {
                s.result = Some(serde_json::json!(history));
                s.phase = Phase::Ready;
            })?;
        }
        Operation::Save { connection, save, synchronize: publish } => {
            save.record.check_connection(&connection)?;
            let document =
                save.objects.get(&format!("documents/{}", save.record.manifest.document_hash)).ok_or_else(|| storage("captured document missing"))?;
            solidcraft_engine::io::cloud::check_document(document)?;
            job.update(|status| {
                status.destination = Some(save.record.destination.clone());
                status.revision = Some(save.record.id.clone());
            })?;
            let record = store.stage(*save).await?;
            let change = record.manifest.change()?;
            job.update(|status| {
                status.change = change;
                status.saved_locally = true;
                status.phase = if publish { Phase::Syncing } else { Phase::SavedLocally };
                status.destination = Some(record.destination.clone());
                status.revision = Some(record.id.clone());
            })?;
            if publish {
                synchronize(connection, store, job.clone(), Some(record)).await?;
            }
        }
        Operation::Synchronize(connection) => synchronize(connection, store, job.clone(), None).await?,
        Operation::Test(connection) => {
            repository(&connection, job).await?.test_connection().await?;
            check_cancelled(job)?;
            job.update(|status| status.phase = Phase::Ready)?;
        }
        Operation::List(connection) => {
            let cached = store.cached_entries().await?;
            let mut pending = Vec::new();
            for save in store.pending().await? {
                if store.receipt(&save.id).await?.is_none() {
                    pending.push(save);
                }
            }
            let mut listing = solidcraft_cloud::Listing::new(cached, pending, job)?;
            let remote = async { repository(&connection, job).await?.list_with_status_cached(&mut listing.catalog, &store).await }.await;
            listing.merge(remote, job)?;
            for entry in listing.catalog.iter().filter(|entry| entry.outdated) {
                store.mark_outdated(entry).await?;
            }
            listing.finish(job)?;
        }
        Operation::Thumbnail { connection, destination } => {
            let png = if let Some(save) = pending_tip(&store, &destination).await? {
                store.thumbnail(&save).await?
            } else if let Some(entry) = store.cached(&destination).await? {
                store.thumbnail(&entry.save).await?
            } else {
                repository(&connection, job).await?.thumbnail(&destination, solidcraft_engine::io::cloud::check_document).await?
            };
            check_cancelled(job)?;
            job.update(|status| {
                status.result = Some(solidcraft_cloud::Thumbnail::response(png));
                status.phase = Phase::Ready;
            })?;
        }
        Operation::Open { connection, destination, local_revision } => {
            open(&store, &connection, &destination, local_revision.as_deref(), job).await?
        }
        _ => return Err(storage("invalid repository operation")),
    }
    Ok(())
}

impl Host for BrowserHost {
    fn check_repository(&self, connection: Connection, input: solidcraft_cloud::jobs::RepositoryCheckInput) -> Result<Job> {
        connection.validate()?;
        if connection.provider != "github" {
            return Err(storage("unsupported setup provider"));
        }
        let job = Job::new(connection.id.clone())?;
        let worker = job.clone();
        wasm_bindgen_futures::spawn_local(async move {
            let result = async {
                let token = match input.token {
                    Some(token) => Some(token),
                    None => {
                        solidcraft_cloud::jobs::RepositoryCheckInput::new(LocalSettings::open().await?.credential(&connection.credential).await?)?
                            .token
                    }
                };
                let client = crate::cloud_client::client_for_job(token.as_deref().map(|s| s.as_str()), Some(worker.clone()))
                    .map_err(|_| storage("GitHub client unavailable"))?;
                let exists = solidcraft_cloud::github::exists(client, &connection.repository).await?;
                worker.update(|s| {
                    s.result = Some(serde_json::json!({"exists":exists}));
                    s.phase = Phase::Ready;
                })
            }
            .await;
            if let Err(error) = result {
                let _ = worker.fail(error);
            }
        });
        Ok(job)
    }

    fn store_credential(&self, id: String, token: String) -> Result<Job> {
        solidcraft_cloud::setup::check_token(&token)?;
        let job = Job::new(String::new())?;
        let worker = job.clone();
        wasm_bindgen_futures::spawn_local(async move {
            let result = async {
                LocalSettings::open().await?.store_credential(id, Some(token)).await?;
                worker.update(|status| status.phase = Phase::Ready)
            }
            .await;
            if let Err(error) = result {
                let _ = worker.fail(error);
            }
        });
        Ok(job)
    }
    fn prepare_save(&self, connection: Connection, prepare: solidcraft_cloud::jobs::PrepareSave, synchronize: bool, _: String) -> Result<Job> {
        connection.validate()?;
        let job = Job::new(connection.id.clone())?;
        let worker = job.clone();
        wasm_bindgen_futures::spawn_local(async move {
            let result = async {
                check_cancelled(&worker)?;
                let save = prepare(&worker)?;
                run(Operation::Save { connection, save: Box::new(save), synchronize }, &worker).await
            }
            .await;
            if let Err(error) = result {
                let _ = worker.fail(error);
            }
        });
        Ok(job)
    }

    fn submit(&self, operation: Operation, _: String) -> Result<Job> {
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
        let worker = job.clone();
        wasm_bindgen_futures::spawn_local(async move {
            if let Err(error) = run(operation, &worker).await {
                let _ = worker.fail(error);
            }
        });
        Ok(job)
    }
}
