//! One durable publication workflow for native Git, provider APIs and browser storage.
//! Adapters own locking and IO; ordering, retries and acknowledgement policy live here.
use crate::{
    Connection, Error, PendingSave, Published, Result,
    jobs::{Job, Phase},
};
use std::{collections::BTreeSet, future::Future};

pub trait Backend {
    fn pending(&mut self) -> impl Future<Output = Result<Vec<PendingSave>>>;
    fn receipt(&mut self, save: &PendingSave) -> impl Future<Output = Result<Option<Published>>>;
    fn parent(&mut self, save: &PendingSave) -> impl Future<Output = Result<Option<Published>>>;
    fn publish(&mut self, save: &PendingSave, parent: Option<&Published>) -> impl Future<Output = Result<Published>>;
    fn confirm(&mut self, save: &PendingSave, published: &Published) -> impl Future<Output = Result<()>>;
    fn release(&mut self) {}
    fn refresh(&mut self) -> impl Future<Output = Result<()>>;
}

pub fn check_cancelled(job: &Job) -> Result<()> {
    if job.cancelled()? { Err(Error::Storage("operation cancelled; local captures retained".into())) } else { Ok(()) }
}

/// Call under the platform's per-repository synchronization lock. Never hold a storage
/// transaction across publication. Once publication succeeds, finish its durable receipt
/// even if cancellation races it; independent successes survive later failures.
pub async fn synchronize(backend: &mut impl Backend, connection: &Connection, job: &Job, target: Option<PendingSave>) -> Result<()> {
    check_cancelled(job)?;
    let mut pending = backend.pending().await?;
    if let Some(target) = &target
        && !pending.iter().any(|save| save.id == target.id)
    {
        pending.push(target.clone());
    }
    let pending = PendingSave::ordered(pending)?;
    job.update(|s| s.phase = Phase::Syncing)?;
    let mut blocked = BTreeSet::new();
    let mut results = Vec::new();
    let mut first_error = None;
    for save in pending {
        check_cancelled(job)?;
        if save.expected_revision.as_ref().is_some_and(|parent| blocked.contains(parent)) {
            blocked.insert(save.id);
            continue;
        }
        let result = async {
            save.check_connection(connection)?;
            let parent = backend.parent(&save).await?;
            let published = match backend.receipt(&save).await? {
                Some(published) => published,
                None => backend.publish(&save, parent.as_ref()).await?,
            };
            published.validate(&save, parent.as_ref())?;
            backend.confirm(&save, &published).await?;
            Ok(published)
        }
        .await;
        match result {
            Ok(published) => {
                let managed = crate::management::Managed::confirmed(&save, &published)?;
                job.update(|s| {
                    s.publications.push(published.clone());
                    s.managed.extend(managed);
                })?;
                if target.as_ref().is_some_and(|target| target.id == save.id) {
                    backend.release();
                    return job.update(|s| {
                        s.destination = Some(published.destination.clone());
                        s.revision = Some(published.revision.clone());
                        s.result = Some(serde_json::json!(published));
                        s.phase = Phase::Synced;
                    });
                }
                results.push(published);
            }
            Err(error) => {
                blocked.insert(save.id);
                if first_error.is_none() {
                    first_error = Some(error);
                }
            }
        }
    }
    job.update(|s| s.result = Some(serde_json::json!(results)))?;
    if let Some(error) = first_error {
        return Err(error);
    }
    if target.is_none() {
        backend.refresh().await?;
    }
    backend.release();
    job.update(|s| s.phase = Phase::Synced)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Snapshot;
    use std::collections::BTreeMap;

    #[derive(Default)]
    struct Store {
        saves: Vec<PendingSave>,
        receipts: BTreeMap<String, Published>,
        failed: Option<String>,
        attempted: Vec<String>,
        confirmed: Vec<String>,
        refreshed: bool,
        released: bool,
        cancel: Option<Job>,
    }
    impl Backend for Store {
        async fn pending(&mut self) -> Result<Vec<PendingSave>> {
            Ok(self.saves.clone())
        }
        async fn receipt(&mut self, save: &PendingSave) -> Result<Option<Published>> {
            Ok(self.receipts.get(&save.id).cloned())
        }
        async fn parent(&mut self, save: &PendingSave) -> Result<Option<Published>> {
            Ok(save.expected_revision.as_ref().and_then(|id| self.receipts.get(id)).cloned())
        }
        async fn publish(&mut self, save: &PendingSave, _: Option<&Published>) -> Result<Published> {
            self.attempted.push(save.id.clone());
            if self.failed.as_ref() == Some(&save.id) {
                return Err(Error::Storage("offline".into()));
            }
            if let Some(job) = &self.cancel {
                job.cancel()?;
            }
            Ok(Published { destination: save.destination.clone(), revision: save.id.clone(), commit: "a".repeat(40), conflict: false })
        }
        async fn confirm(&mut self, save: &PendingSave, published: &Published) -> Result<()> {
            self.receipts.insert(save.id.clone(), published.clone());
            self.confirmed.push(save.id.clone());
            Ok(())
        }
        fn release(&mut self) {
            self.released = true;
        }
        async fn refresh(&mut self) -> Result<()> {
            self.refreshed = true;
            Ok(())
        }
    }
    fn save(file: &str, text: &str, parent: Option<String>) -> PendingSave {
        PendingSave::stage(&Connection::default(), "Project", file, parent, Snapshot::prepare(text.as_bytes().to_vec(), BTreeMap::new()).unwrap())
            .unwrap()
            .record
    }
    #[test]
    fn partial_success_blocks_only_descendants_and_receipts_resume_without_republication() {
        let first = save("parts/a", "first", None);
        let child = save("parts/a", "second", Some(first.id.clone()));
        let other = save("parts/b", "other", None);
        let mut store = Store { saves: vec![child.clone(), other.clone(), first.clone()], failed: Some(first.id.clone()), ..Default::default() };
        let job = Job::new("github".into()).unwrap();
        let runtime = tokio::runtime::Builder::new_current_thread().build().unwrap();
        assert!(runtime.block_on(synchronize(&mut store, &Connection::default(), &job, None)).is_err());
        assert!(!store.attempted.contains(&child.id));
        assert_eq!(store.confirmed, vec![other.id.clone()]);
        assert_eq!(job.status().unwrap().publications.len(), 1);
        assert!(!store.refreshed);
        store.failed = None;
        store.attempted.clear();
        let retry = Job::new("github".into()).unwrap();
        runtime.block_on(synchronize(&mut store, &Connection::default(), &retry, None)).unwrap();
        assert!(!store.attempted.contains(&other.id));
        assert_eq!(store.attempted, vec![first.id, child.id]);
        assert!(store.refreshed && store.released);
        assert_eq!(retry.status().unwrap().phase, Phase::Synced);
    }
    #[test]
    fn cancellation_after_publication_finishes_receipt_and_target_acknowledgement() {
        let save = save("parts/a", "capture", None);
        let job = Job::new("github".into()).unwrap();
        let mut store = Store { saves: vec![save.clone()], cancel: Some(job.clone()), ..Default::default() };
        let runtime = tokio::runtime::Builder::new_current_thread().build().unwrap();
        runtime.block_on(synchronize(&mut store, &Connection::default(), &job, Some(save.clone()))).unwrap();
        assert_eq!(store.confirmed, vec![save.id]);
        assert!(store.released && !store.refreshed);
        assert_eq!(job.status().unwrap().phase, Phase::Synced);
    }
}
