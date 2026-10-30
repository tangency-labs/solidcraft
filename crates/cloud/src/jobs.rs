//! Application-owned asynchronous operations. Native workers and browser tasks expose the
//! same durable-save boundary; polling never substitutes the editor's current document.
use std::collections::BTreeMap;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU64, Ordering},
};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{Connection, Destination, Error, Published, Result, Settings, Snapshot, StagedSave};

static NEXT_JOB: AtomicU64 = AtomicU64::new(1);

/// One-shot secret input. Never serialize or include it in command parameters or logs.
#[derive(Default)]
pub struct SshKeyInput {
    pub passphrase: zeroize::Zeroizing<String>,
    pub remember: bool,
}

impl SshKeyInput {
    pub fn validate(&self) -> Result<()> {
        if self.passphrase.len() > 1024 || self.passphrase.chars().any(char::is_control) {
            return Err(Error::Invalid("SSH passphrase must be at most 1024 bytes without control characters".into()));
        }
        if self.remember && self.passphrase.is_empty() {
            return Err(Error::Invalid("enter a passphrase before remembering it".into()));
        }
        Ok(())
    }
}

/// Public result of local key generation. Contains no private key or passphrase.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SshKey {
    pub path: String,
    pub public_key: String,
    pub encrypted: bool,
    pub remembered: bool,
}

/// Stable local keychain identity; never written into a shared repository.
pub fn ssh_credential(path: &str) -> String {
    format!("ssh-key:{}", crate::digest(path.as_bytes()))
}

pub enum Operation {
    GenerateSshKey(SshKeyInput),
    LoadSettings,
    Configure(Settings),
    /// Verify provider access and resolve its repository; settings apply separately.
    Setup(Connection),
    Test(Connection),
    List(Connection),
    Trash(Connection),
    Discard {
        connection: Connection,
        destination: Destination,
        revision: String,
    },
    History {
        connection: Connection,
        destination: Destination,
    },
    Manage {
        connection: Connection,
        destination: Destination,
        expected_revision: Option<String>,
        change: crate::management::Change,
    },
    Thumbnail {
        connection: Connection,
        destination: Destination,
    },
    Open {
        connection: Connection,
        destination: Destination,
        local_revision: Option<String>,
    },
    Save {
        connection: Connection,
        save: Box<StagedSave>,
        synchronize: bool,
    },
    Synchronize(Connection),
}

/// Immutable document capture prepared by a platform worker, outside serialized commands.
pub type PrepareSave = Box<dyn FnOnce(&Job) -> Result<StagedSave> + Send>;

pub trait Host: Send + Sync {
    fn submit(&self, operation: Operation, git_executable: String) -> Result<Job>;

    /// Platform hosts schedule expensive serialization, compression and previews before
    /// staging. Small embedding/test hosts may execute preparation synchronously.
    fn prepare_save(&self, connection: Connection, prepare: PrepareSave, synchronize: bool, git_executable: String) -> Result<Job> {
        let preparation = Job::new(connection.id.clone())?;
        self.submit(Operation::Save { connection, save: Box::new(prepare(&preparation)?), synchronize }, git_executable)
    }

    /// Read-only setup check. Unsaved credentials stay outside JSON commands and storage.
    fn check_repository(&self, _connection: Connection, _input: RepositoryCheckInput) -> Result<Job> {
        Err(Error::Storage("repository checking is unavailable in this host".into()))
    }

    /// Secret input is deliberately separate from serializable engine commands and logs.
    fn store_credential(&self, _id: String, _token: String) -> Result<Job> {
        Err(Error::Storage("credential storage is unavailable in this host".into()))
    }
}

#[derive(Default)]
pub struct RepositoryCheckInput {
    pub token: Option<zeroize::Zeroizing<String>>,
}

impl RepositoryCheckInput {
    pub fn new(token: Option<String>) -> Result<Self> {
        let token = token.map(zeroize::Zeroizing::new);
        if let Some(token) = &token {
            crate::setup::check_token(token)?;
        }
        Ok(Self { token })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Working,
    SavedLocally,
    Syncing,
    Synced,
    Ready,
    Failed,
    Cancelled,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct JobStatus {
    pub operation: String,
    pub connection: String,
    pub phase: Phase,
    pub saved_locally: bool,
    pub destination: Option<Destination>,
    pub revision: Option<String>,
    pub message: Option<String>,
    /// Verified immutable chunks available during an Open (completed, total).
    #[serde(default)]
    pub download_progress: Option<(usize, usize)>,
    pub result: Option<Value>,
    /// Confirmed captures, including partial success before a later failure or cancellation.
    /// A connection synchronization job can publish several independent Save operations.
    #[serde(default)]
    pub publications: Vec<Published>,
    #[serde(default)]
    pub managed: Vec<crate::management::Managed>,
    #[serde(default)]
    pub change: Option<crate::management::Change>,
    /// Browser storage persistence was granted (false is a recoverability warning).
    #[serde(default)]
    pub storage_persistent: Option<bool>,
    /// Reconstructed from durable cache and outbox state on repository listing.
    #[serde(default)]
    pub catalog: Vec<crate::StoredDesign>,
}

struct State {
    status: JobStatus,
    cancelled: bool,
    opened: Option<Snapshot>,
    cancel_handlers: BTreeMap<u64, Box<dyn FnOnce() + Send>>,
    next_handler: u64,
}

#[derive(Clone)]
pub struct Job(Arc<Mutex<State>>);

pub struct Cancellation {
    job: Job,
    id: u64,
}
impl Drop for Cancellation {
    fn drop(&mut self) {
        if let Ok(mut state) = self.job.0.lock() {
            state.cancel_handlers.remove(&self.id);
        }
    }
}

impl Job {
    pub fn new(connection: String) -> Result<Self> {
        let mut id = NEXT_JOB.load(Ordering::Relaxed);
        loop {
            let next = id.checked_add(1).ok_or_else(|| Error::Storage("cloud operation identity exhausted".into()))?;
            match NEXT_JOB.compare_exchange_weak(id, next, Ordering::Relaxed, Ordering::Relaxed) {
                Ok(_) => break,
                Err(current) => id = current,
            }
        }
        Ok(Self(Arc::new(Mutex::new(State {
            status: JobStatus {
                operation: format!("cloud-{id}"),
                connection,
                phase: Phase::Working,
                saved_locally: false,
                destination: None,
                revision: None,
                message: None,
                download_progress: None,
                result: None,
                publications: Vec::new(),
                managed: Vec::new(),
                change: None,
                storage_persistent: None,
                catalog: Vec::new(),
            },
            cancelled: false,
            opened: None,
            cancel_handlers: BTreeMap::new(),
            next_handler: 0,
        }))))
    }

    pub fn status(&self) -> Result<JobStatus> {
        self.0.lock().map(|state| state.status.clone()).map_err(|_| Error::Storage("cloud operation state unavailable".into()))
    }

    /// Expose locally stored projects before repository access. The listing remains
    /// Working until its remote refresh completes, so callers can show both at once.
    pub fn local_catalog(&self, catalog: Vec<crate::StoredDesign>) -> Result<()> {
        let destinations: Vec<_> = catalog.iter().map(|entry| &entry.destination).collect();
        let result = serde_json::to_value(destinations).map_err(|e| Error::Storage(e.to_string()))?;
        self.update(|status| {
            status.result = Some(result);
            status.catalog = catalog;
        })
    }

    pub fn update(&self, change: impl FnOnce(&mut JobStatus)) -> Result<()> {
        let mut state = self.0.lock().map_err(|_| Error::Storage("cloud operation state unavailable".into()))?;
        if state.status.phase == Phase::Synced {
            return Ok(());
        }
        change(&mut state.status);
        if matches!(state.status.phase, Phase::Synced | Phase::Ready | Phase::Failed) {
            state.cancel_handlers.clear();
        }
        if state.cancelled && state.status.phase != Phase::Synced {
            state.status.phase = if state.status.saved_locally { Phase::SavedLocally } else { Phase::Cancelled };
        }
        Ok(())
    }

    pub fn cancel(&self) -> Result<()> {
        let mut state = self.0.lock().map_err(|_| Error::Storage("cloud operation state unavailable".into()))?;
        if state.status.phase == Phase::Synced {
            return Ok(());
        }
        state.cancelled = true;
        state.status.phase = if state.status.saved_locally { Phase::SavedLocally } else { Phase::Cancelled };
        // A durable queued save is retained even when the user cancels its upload.
        let handlers = std::mem::take(&mut state.cancel_handlers);
        drop(state);
        for handler in handlers.into_values() {
            handler();
        }
        Ok(())
    }

    /// Register one in-flight request. Concurrent requests are all aborted on Cancel;
    /// dropping its guard removes the hook as soon as that request finishes.
    pub fn on_cancel(&self, handler: impl FnOnce() + Send + 'static) -> Result<Cancellation> {
        let mut state = self.0.lock().map_err(|_| Error::Storage("cloud operation state unavailable".into()))?;
        if state.cancelled {
            drop(state);
            handler();
            return Ok(Cancellation { job: self.clone(), id: 0 });
        }
        let id = state.next_handler.checked_add(1).ok_or_else(|| Error::Storage("cloud cancellation identity exhausted".into()))?;
        state.next_handler = id;
        if state.status.phase != Phase::Synced {
            state.cancel_handlers.insert(id, Box::new(handler));
        }
        Ok(Cancellation { job: self.clone(), id })
    }

    pub fn cancelled(&self) -> Result<bool> {
        self.0.lock().map(|state| state.cancelled).map_err(|_| Error::Storage("cloud operation state unavailable".into()))
    }

    pub fn fail(&self, error: Error) -> Result<()> {
        let mut state = self.0.lock().map_err(|_| Error::Storage("cloud operation state unavailable".into()))?;
        if !state.cancelled {
            state.status.phase = if state.status.saved_locally { Phase::SavedLocally } else { Phase::Failed };
            state.status.message = Some(error.to_string());
        }
        Ok(())
    }

    pub fn opened(&self, snapshot: Snapshot) -> Result<()> {
        let mut state = self.0.lock().map_err(|_| Error::Storage("cloud operation state unavailable".into()))?;
        if !state.cancelled {
            state.opened = Some(snapshot);
            state.status.phase = Phase::Ready;
            state.cancel_handlers.clear();
        }
        Ok(())
    }

    /// Consume a completed transient response so image bytes do not remain in history.
    pub fn take_result(&self) -> Result<Value> {
        let mut state = self.0.lock().map_err(|_| Error::Storage("cloud operation state unavailable".into()))?;
        if state.cancelled || state.status.phase != Phase::Ready {
            return Err(Error::Storage("operation has not completed".into()));
        }
        state.status.result.take().ok_or_else(|| Error::Storage("operation result already consumed".into()))
    }

    pub fn take_opened(&self) -> Result<Snapshot> {
        let mut state = self.0.lock().map_err(|_| Error::Storage("cloud operation state unavailable".into()))?;
        if state.cancelled {
            return Err(Error::Storage("opening cancelled".into()));
        }
        state.opened.take().ok_or_else(|| Error::Storage("opening has not completed".into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    #[test]
    fn cancellation_wakes_concurrent_requests_outside_the_job_lock_and_drops_finished_hooks() {
        let job = Job::new("connection".into()).unwrap();
        let called = Arc::new(AtomicUsize::new(0));
        let add = |count| {
            let called = called.clone();
            let inspect = job.clone();
            job.on_cancel(move || {
                assert!(inspect.cancelled().unwrap());
                called.fetch_add(count, Ordering::SeqCst);
            })
            .unwrap()
        };
        let finished = add(100);
        let _first = add(1);
        let _second = add(2);
        drop(finished);
        job.cancel().unwrap();
        assert_eq!(called.load(Ordering::SeqCst), 3);
        job.fail(Error::Storage("cancelled".into())).unwrap();
        assert_eq!(job.status().unwrap().phase, Phase::Cancelled);
        assert!(job.status().unwrap().message.is_none());
        job.cancel().unwrap();
        let _late = add(4);
        assert_eq!(called.load(Ordering::SeqCst), 7);
        let synced = Job::new("other".into()).unwrap();
        let _hook = synced.on_cancel(|| panic!("confirmed publication cannot be cancelled")).unwrap();
        synced.update(|s| s.phase = Phase::Synced).unwrap();
        synced.cancel().unwrap();
        let queued = Job::new("durable".into()).unwrap();
        queued.update(|s| s.saved_locally = true).unwrap();
        queued.cancel().unwrap();
        queued.fail(Error::Storage("upload stopped".into())).unwrap();
        assert_eq!(queued.status().unwrap().phase, Phase::SavedLocally);
        assert!(queued.status().unwrap().message.is_none());
    }
}
