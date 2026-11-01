//! Browser management stages the same immutable captures used by native Git.
use super::*;
use solidcraft_cloud::{Destination, StagedSave, management::Change};

pub(super) async fn current(store: &Store, connection: &Connection, destination: &Destination, job: &Job) -> Result<solidcraft_cloud::Snapshot> {
    if let Some(save) = pending_tip(store, destination).await? {
        if save.manifest.change()?.is_some() {
            return Err(storage("synchronize this design's pending management action first"));
        }
        let mut snapshot = recovered(store, &save).await?;
        snapshot.manifest.extra.insert("revision".into(), serde_json::json!(save.id));
        return Ok(snapshot);
    }
    match async { download(&repository(connection, job).await?, store, destination, job).await }.await {
        Ok(snapshot) => Ok(snapshot),
        Err(error @ Error::Storage(_)) => {
            let Some(entry) = store.cached(destination).await? else { return Err(error) };
            let mut snapshot = recovered(store, &entry.save).await?;
            if let Some(revision) = entry.revision {
                snapshot.manifest.extra.insert("revision".into(), serde_json::json!(revision));
            }
            Ok(snapshot)
        }
        Err(error) => Err(error),
    }
}

pub(super) async fn stage(
    store: &Store,
    connection: &Connection,
    destination: &Destination,
    expected: Option<String>,
    change: &Change,
    job: &Job,
) -> Result<StagedSave> {
    change.validate(destination)?;
    let current = current(store, connection, destination, job).await?;
    let history = current.manifest.history_paths(destination)?;
    if solidcraft_cloud::revision(&current.manifest) != expected.as_deref() {
        return Err(Error::Conflict);
    }
    let mut snapshot = if let Change::Restore { commit, file } = change {
        let historical = Destination { file: file.clone(), ..destination.clone() };
        repository(connection, job).await?.open_at(&historical, commit, solidcraft_engine::io::cloud::check_document).await?
    } else {
        current
    };
    if matches!(change, Change::Restore { .. }) {
        snapshot
            .manifest
            .extra
            .insert("previous_paths".into(), serde_json::json!(history.into_iter().filter(|p| p != &destination.file).take(16).collect::<Vec<_>>()));
    }
    let snapshot = solidcraft_engine::io::cloud::prepare_change(snapshot, destination, change)?;
    PendingSave::stage(connection, &destination.project, &destination.file, expected, snapshot)
}

pub(super) async fn discard(store: Store, connection: Connection, destination: Destination, revision: String, job: Job) -> Result<()> {
    let lock = format!("solidcraft-cloud-sync-{}", solidcraft_cloud::digest(connection.id.as_bytes()));
    with_lock(&lock, async move {
        let pending = store.pending().await?;
        let save = pending
            .iter()
            .find(|s| s.id == revision && s.destination == destination && s.manifest.change().ok().flatten().is_some())
            .ok_or_else(|| storage("this management action is no longer queued; refresh"))?
            .clone();
        if pending.iter().any(|s| s.expected_revision.as_ref() == Some(&revision)) {
            return Err(storage("later saves depend on this action; synchronize before discarding it"));
        }
        let snapshot = download(&repository(&connection, &job).await?, &store, &destination, &job).await?;
        if solidcraft_cloud::revision(&snapshot.manifest) == Some(&revision) {
            return Err(storage("this action reached the repository; synchronize to confirm it"));
        }
        check_cancelled(&job)?;
        store.discard_management(save).await?;
        store.cache_remote(destination, snapshot).await
    })
    .await
}

pub(super) async fn trash(store: &Store, connection: &Connection, job: &Job) -> Result<Vec<Destination>> {
    let mut entries = BTreeMap::new();
    for entry in store.cached_entries().await? {
        if entry.save.manifest.deleted() && !entry.save.manifest.moved() {
            entries.insert(entry.destination.design_path()?, entry.destination);
        }
    }
    for save in store.pending().await? {
        if save.manifest.deleted() {
            entries.insert(save.destination.design_path()?, save.destination);
        }
    }
    match async { repository(connection, job).await?.list_deleted().await }.await {
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
