//! Native GitHub API adapter. The shared crate owns Git objects, compatibility and conflicts.
use super::*;
use solidcraft_cloud::{Destination, PendingSave, Published, Snapshot, StoredDesign, github::GitHub, management::History};
use solidcraft_engine::io::cloud::{ProviderRepository, check_document};
use std::future::Future;

pub(super) struct Repository {
    pub connection: Connection,
    pub job: Job,
    pub token: zeroize::Zeroizing<String>,
}

impl Repository {
    fn call<T, F: Future<Output = Result<T>>>(&self, operation: impl FnOnce(GitHub) -> F) -> Result<T> {
        let runtime = tokio::runtime::Builder::new_current_thread().build().map_err(|_| Error::Storage("repository worker unavailable".into()))?;
        tokio::task::LocalSet::new().block_on(&runtime, async {
            let client = client(&self.token, self.job.clone()).map_err(|_| Error::Storage("repository client unavailable".into()))?;
            operation(GitHub::new(client, &self.connection)?).await
        })
    }
}

impl ProviderRepository for Repository {
    fn test(&mut self) -> Result<()> {
        self.call(|api| async move { api.test_connection().await })
    }
    fn list(&mut self, stored: &mut [StoredDesign]) -> Result<Vec<Destination>> {
        self.call(|api| async move { api.list_with_status(stored).await })
    }
    fn trash(&mut self) -> Result<Vec<Destination>> {
        self.call(|api| async move { api.list_deleted().await })
    }
    fn open(&mut self, destination: &Destination) -> Result<Snapshot> {
        self.call(|api| async move { api.open(destination, check_document).await })
    }
    fn thumbnail(&mut self, destination: &Destination) -> Result<Option<Vec<u8>>> {
        self.call(|api| async move { api.thumbnail(destination, check_document).await })
    }
    fn history(&mut self, destination: &Destination) -> Result<History> {
        self.call(|api| async move { api.history(destination, check_document).await })
    }
    fn open_at(&mut self, destination: &Destination, commit: &str) -> Result<Snapshot> {
        self.call(|api| async move { api.open_at(destination, commit, check_document).await })
    }
    fn publish(&mut self, save: &PendingSave, snapshot: Snapshot, parent: Option<&Published>) -> Result<Published> {
        self.call(|api| async move {
            api.publish_with_parent(
                save,
                || async {
                    Ok(Snapshot { manifest: snapshot.manifest.clone(), document: snapshot.document.clone(), objects: snapshot.objects.clone() })
                },
                check_document,
                parent,
            )
            .await
        })
    }
}
