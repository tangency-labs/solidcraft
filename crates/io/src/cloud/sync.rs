//! Worker-local adapters for the portable synchronization workflow.
use super::{GitRepository, LocalOutbox, NativeAccess, ProviderRepository};
use solidcraft_cloud::{Connection, PendingSave, Published, Result, Snapshot, jobs::Job, sync::Backend};

enum Repository {
    Git(Box<GitRepository>),
    Provider(Box<dyn ProviderRepository>),
}
struct Native<'a> {
    outbox: &'a LocalOutbox,
    connection: &'a Connection,
    executable: &'a str,
    job: &'a Job,
    access: Option<&'a dyn NativeAccess>,
    repository: Option<Repository>,
    lock: Option<super::local::ProcessLock>,
}
impl Native<'_> {
    fn repository(&mut self) -> Result<&mut Repository> {
        if self.repository.is_none() {
            self.repository = Some(if let Some(access) = self.access.filter(|a| a.uses_provider_api(self.connection)) {
                Repository::Provider(access.provider_repository(self.connection, self.job)?)
            } else {
                Repository::Git(Box::new(super::host::open_repository(self.connection.clone(), self.executable, self.job, self.access)?))
            });
        }
        self.repository.as_mut().ok_or_else(|| solidcraft_cloud::Error::Storage("repository unavailable".into()))
    }
}
impl Backend for Native<'_> {
    async fn pending(&mut self) -> Result<Vec<PendingSave>> {
        self.outbox.pending()
    }
    async fn receipt(&mut self, save: &PendingSave) -> Result<Option<Published>> {
        self.outbox.receipt(save)
    }
    async fn parent(&mut self, save: &PendingSave) -> Result<Option<Published>> {
        self.outbox.published_parent(save)
    }
    async fn publish(&mut self, save: &PendingSave, parent: Option<&Published>) -> Result<Published> {
        let snapshot: Snapshot = self.outbox.recover_document(save)?;
        match self.repository()? {
            Repository::Git(repository) => repository.publish_with_parent(save, snapshot, parent),
            Repository::Provider(repository) => repository.publish(save, snapshot, parent),
        }
    }
    async fn confirm(&mut self, save: &PendingSave, published: &Published) -> Result<()> {
        self.outbox.published(save, published)
    }
    fn release(&mut self) {
        self.lock.take();
    }
    async fn refresh(&mut self) -> Result<()> {
        if self.access.is_some_and(|a| a.uses_provider_api(self.connection)) {
            let outbox = self.outbox;
            let connection = self.connection;
            let job = self.job;
            let Repository::Provider(repository) = self.repository()? else {
                return Err(solidcraft_cloud::Error::Storage("provider unavailable".into()));
            };
            super::provider::refresh(outbox, connection, job, repository.as_mut())
        } else {
            super::receive::refresh(self.connection, self.executable, self.outbox, self.job, self.access)
        }
    }
}

pub(super) fn synchronize(
    outbox: &LocalOutbox,
    connection: &Connection,
    executable: &str,
    job: &Job,
    access: Option<&dyn NativeAccess>,
    target: Option<PendingSave>,
) -> Result<()> {
    let lock = outbox.wait_for_synchronization(job)?;
    futures_lite::future::block_on(solidcraft_cloud::sync::synchronize(
        &mut Native { outbox, connection, executable, job, access, repository: None, lock: Some(lock) },
        connection,
        job,
        target,
    ))
}
