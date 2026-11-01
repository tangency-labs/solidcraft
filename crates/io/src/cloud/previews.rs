//! Preview reads do not hydrate imports, rebuild geometry, or contact Git remotes.
use super::{GitRepository, LocalOutbox};
use solidcraft_cloud::{Connection, Destination, Error, PendingSave, Result, jobs::Job};

pub(super) fn captured(outbox: &LocalOutbox, save: &PendingSave) -> Result<Option<Vec<u8>>> {
    let document = outbox.read(&format!("documents/{}", save.manifest.document_hash))?;
    super::check_document(&document)?;
    if solidcraft_cloud::digest(&document) != save.manifest.document_hash {
        return Err(Error::Invalid("preview document hash mismatch".into()));
    }
    super::read_thumbnail(&save.manifest, |path| outbox.read(path))
}

pub(super) fn read(connection: Connection, destination: Destination, executable: &str, outbox: &LocalOutbox, job: &Job) -> Result<Option<Vec<u8>>> {
    destination.validate()?;
    if destination.connection != connection.id {
        return Err(Error::Invalid("repository connection mismatch".into()));
    }
    if let Some(save) = super::host::pending_tip(outbox, &connection, &destination)? {
        return captured(outbox, &save);
    }
    match GitRepository::open_cancellable(connection.clone(), executable, job.clone()).and_then(|repo| repo.thumbnail(&destination)) {
        Ok(png) => Ok(png),
        Err(error @ Error::Storage(_)) => match outbox.cached(&connection, &destination)? {
            Some(entry) => captured(outbox, &entry.save),
            None => Err(error),
        },
        Err(error) => Err(error),
    }
}
