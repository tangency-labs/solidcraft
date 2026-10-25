//! Pull changed cached designs without downloading every unopened design in a repository.
use super::*;

pub(super) async fn refresh(connection: &Connection, store: &Store, job: &Job) -> Result<()> {
    job.update(|status| status.phase = Phase::Syncing)?;
    let remote = repository(connection, job).await?;
    let mut observed = solidcraft_cloud::stored_designs(store.cached_entries().await?, Vec::new())?;
    remote.list_with_status(&mut observed).await?;
    for entry in observed {
        check_cancelled(job)?;
        if !entry.outdated || pending_tip(store, &entry.destination).await?.is_some() {
            continue;
        }
        store.mark_outdated(&entry).await?;
        let snapshot = remote.open(&entry.destination, solidcraft_engine::io::cloud::check_document).await?;
        check_cancelled(job)?;
        store.cache_remote(entry.destination, snapshot).await?;
    }
    Ok(())
}
