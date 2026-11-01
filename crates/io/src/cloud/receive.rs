//! Update cached designs after an explicit repository synchronization.
//! Queued captures keep priority; only repository-confirmed, clean content becomes current.
use super::{LocalOutbox, NativeAccess, host::open_repository};
use solidcraft_cloud::{
    Connection, Result,
    jobs::{Job, Phase},
};

pub(super) fn refresh(connection: &Connection, executable: &str, outbox: &LocalOutbox, job: &Job, access: Option<&dyn NativeAccess>) -> Result<()> {
    let cached = outbox.cached_entries(connection)?;
    if connection.remote.is_empty()
        && (cached.is_empty()
            || (!std::path::Path::new(&connection.checkout).exists() && cached.iter().all(|entry| entry.confirmed && !entry.outdated)))
    {
        return Ok(());
    }
    job.update(|status| status.phase = Phase::Syncing)?;
    let repository = open_repository(connection.clone(), executable, job, access)?;
    repository.refresh()?;
    let _checkout = repository.lock()?;
    for observed in solidcraft_cloud::stored_designs(cached, Vec::new())? {
        if job.cancelled()? {
            return Err(solidcraft_cloud::Error::Storage("operation cancelled; local captures retained".into()));
        }
        if super::host::pending_tip(outbox, connection, &observed.destination)?.is_some() {
            continue;
        }
        if observed.phase == Phase::Synced && repository.matches_stored(&observed)? {
            continue;
        }
        outbox.mark_outdated(connection, &observed)?;
        let snapshot = repository.snapshot(&observed.destination)?;
        repository.confirm_synchronized(&observed.destination, &snapshot)?;
        outbox.cache_synchronized_checkout(connection, &observed.destination, &snapshot)?;
    }
    Ok(())
}
