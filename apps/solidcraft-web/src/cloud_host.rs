//! Browser tasks implement the same asynchronous command boundary as native Git workers.
use std::collections::{BTreeMap, BTreeSet};

use solidcraft_cloud::{
    Connection, Destination, Error, PendingSave, Result, Snapshot,
    github::GitHub,
    jobs::{Host, Job, Operation, Phase},
};

use crate::{
    cloud_settings::LocalSettings,
    cloud_storage::{Store, with_lock},
};

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
    let remote = async { repository(connection, job).await?.open(destination, solidcraft_engine::io::cloud::check_document).await }.await;
    match remote {
        Ok(snapshot) => {
            check_cancelled(job)?;
            store.cache_remote(destination.clone(), snapshot).await?;
        }
        Err(error @ Error::Storage(_)) => {
            if store.cached(destination).await?.is_none() {
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
        check_cancelled(&job)?;
        let mut pending = store.pending().await?;
        if let Some(target) = &target
            && !pending.iter().any(|save| save.id == target.id)
        {
            if let Some(published) = store.receipt(&target.id).await? {
                job.update(|status| {
                    status.destination = Some(published.destination.clone());
                    status.result = Some(serde_json::json!(published));
                    status.publications.push(published);
                    status.phase = Phase::Synced;
                })?;
                return Ok(());
            }
            pending.push(target.clone());
            pending = PendingSave::ordered(pending)?;
        }
        if pending.is_empty() {
            receive::refresh(&connection, &store, &job).await?;
            return job.update(|status| {
                status.result = Some(serde_json::json!([]));
                status.phase = Phase::Synced;
            });
        }
        let mut remote = None;
        job.update(|status| status.phase = Phase::Syncing)?;
        let mut blocked = BTreeSet::new();
        let mut first_error = None;
        let mut results = Vec::new();
        for save in pending {
            check_cancelled(&job)?;
            if save.expected_revision.as_ref().is_some_and(|parent| blocked.contains(parent)) {
                blocked.insert(save.id);
                continue;
            }
            let result = async {
                save.check_connection(&connection)?;
                let parent = match &save.expected_revision {
                    Some(id) => store.receipt(id).await?,
                    None => None,
                };
                let published = if let Some(published) = store.receipt(&save.id).await? {
                    published
                } else {
                    if remote.is_none() {
                        remote = Some(repository(&connection, &job).await?);
                    }
                    remote
                        .as_ref()
                        .ok_or_else(|| storage("repository unavailable"))?
                        .publish_with_parent(
                            &save,
                            || recovered(&store, &save),
                            |bytes| {
                                check_cancelled(&job)?;
                                solidcraft_engine::io::cloud::check_document(bytes)
                            },
                            parent.as_ref(),
                        )
                        .await?
                };
                // Finish this acknowledgement even if Cancel raced a successful ref update.
                store.confirm(save.clone(), published.clone()).await?;
                Ok(published)
            }
            .await;
            match result {
                Ok(published) => {
                    let value = serde_json::json!(published);
                    job.update(|status| status.publications.push(published.clone()))?;
                    if target.as_ref().is_some_and(|target| target.id == save.id) {
                        job.update(|status| {
                            status.destination = Some(published.destination);
                            status.revision = Some(published.revision);
                            status.result = Some(value);
                            status.phase = Phase::Synced;
                        })?;
                        return Ok(());
                    }
                    results.push(value);
                }
                Err(error) => {
                    blocked.insert(save.id);
                    if first_error.is_none() {
                        first_error = Some(error);
                    }
                }
            }
        }
        job.update(|status| status.result = Some(serde_json::json!(results)))?;
        if let Some(error) = first_error {
            return Err(error);
        }
        receive::refresh(&connection, &store, &job).await?;
        job.update(|status| status.phase = Phase::Synced)
    })
    .await
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
        | Operation::Synchronize(connection)
        | Operation::Thumbnail { connection, .. }
        | Operation::Open { connection, .. }
        | Operation::Save { connection, .. } => connection.clone(),
        _ => return Err(storage("invalid repository operation")),
    };
    let store = Store::open(connection.clone()).await?;
    job.update(|status| status.storage_persistent = Some(store.persistent))?;
    check_cancelled(job)?;
    match operation {
        Operation::Save { connection, save, synchronize: publish } => {
            save.record.check_connection(&connection)?;
            let document =
                save.objects.get(&format!("documents/{}", save.record.manifest.document_hash)).ok_or_else(|| storage("captured document missing"))?;
            solidcraft_engine::io::cloud::check_document(document)?;
            let record = store.stage(*save).await?;
            job.update(|status| {
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
            let mut catalog = solidcraft_cloud::stored_designs(cached, pending)?;
            let mut destinations = BTreeMap::new();
            for entry in &catalog {
                destinations.insert(entry.destination.design_path()?, entry.destination.clone());
            }
            match async { repository(&connection, job).await?.list_with_status(&mut catalog).await }.await {
                Ok(entries) => {
                    for destination in entries {
                        destinations.insert(destination.design_path()?, destination);
                    }
                }
                Err(error @ Error::Storage(_)) if !destinations.is_empty() => job.update(|status| status.message = Some(error.to_string()))?,
                Err(error) => return Err(error),
            }
            for entry in catalog.iter().filter(|entry| entry.outdated) {
                store.mark_outdated(entry).await?;
            }
            check_cancelled(job)?;
            job.update(|status| {
                status.result = Some(serde_json::json!(destinations.into_values().collect::<Vec<_>>()));
                status.catalog = catalog;
                status.phase = Phase::Ready;
            })?;
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
    fn submit(&self, operation: Operation, _: String) -> Result<Job> {
        let connection = match &operation {
            Operation::LoadSettings | Operation::Configure(_) | Operation::GenerateSshKey(_) => None,
            Operation::Setup(connection)
            | Operation::Test(connection)
            | Operation::List(connection)
            | Operation::Synchronize(connection)
            | Operation::Thumbnail { connection, .. }
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
