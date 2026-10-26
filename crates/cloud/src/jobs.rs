//! Application-owned asynchronous operations. Native workers and browser tasks expose the
//! same durable-save boundary; polling never substitutes the editor's current document.
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
    /// Verify provider access and prepare a checkout; settings apply separately.
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

pub trait Host: Send + Sync {
    fn submit(&self, operation: Operation, git_executable: String) -> Result<Job>;

    /// Secret input is deliberately separate from serializable engine commands and logs.
    fn store_credential(&self, _id: String, _token: String) -> Result<Job> {
        Err(Error::Storage("credential storage is unavailable in this host".into()))
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
    cancel_handler: Option<Box<dyn FnOnce() + Send>>,
}

#[derive(Clone)]
pub struct Job(Arc<Mutex<State>>);

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
                result: None,
                publications: Vec::new(),
                managed: Vec::new(),
                change: None,
                storage_persistent: None,
                catalog: Vec::new(),
            },
            cancelled: false,
            opened: None,
            cancel_handler: None,
        }))))
    }

    pub fn status(&self) -> Result<JobStatus> {
        self.0.lock().map(|state| state.status.clone()).map_err(|_| Error::Storage("cloud operation state unavailable".into()))
    }

    pub fn update(&self, change: impl FnOnce(&mut JobStatus)) -> Result<()> {
        let mut state = self.0.lock().map_err(|_| Error::Storage("cloud operation state unavailable".into()))?;
        if state.status.phase == Phase::Synced {
            return Ok(());
        }
        change(&mut state.status);
        if matches!(state.status.phase, Phase::Synced | Phase::Ready | Phase::Failed) {
            state.cancel_handler = None;
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
        let handler = state.cancel_handler.take();
        drop(state);
        if let Some(handler) = handler {
            handler();
        }
        Ok(())
    }

    /// Install the current worker's cancellation hook. Browser requests are sequential;
    /// replace the completed request's hook before starting the next. Never run it under
    /// the state mutex, since aborting can wake a future which reads this job immediately.
    pub fn set_cancel_handler(&self, handler: impl FnOnce() + Send + 'static) -> Result<()> {
        let mut state = self.0.lock().map_err(|_| Error::Storage("cloud operation state unavailable".into()))?;
        if state.status.phase == Phase::Synced {
            return Ok(());
        }
        if state.cancelled {
            drop(state);
            handler();
        } else {
            state.cancel_handler = Some(Box::new(handler));
        }
        Ok(())
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
            state.cancel_handler = None;
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
    fn cancellation_wakes_only_the_current_request_outside_the_job_lock() {
        let job = Job::new("connection".into()).unwrap();
        let called = Arc::new(AtomicUsize::new(0));
        let first = called.clone();
        job.set_cancel_handler(move || {
            first.fetch_add(100, Ordering::SeqCst);
        })
        .unwrap();
        let current = called.clone();
        let inspect = job.clone();
        job.set_cancel_handler(move || {
            assert_eq!(inspect.status().unwrap().phase, Phase::Cancelled);
            current.fetch_add(1, Ordering::SeqCst);
        })
        .unwrap();
        job.cancel().unwrap();
        assert_eq!(called.load(Ordering::SeqCst), 1);
        job.fail(Error::Storage("worker observed cancellation".into())).unwrap();
        assert_eq!(job.status().unwrap().phase, Phase::Cancelled);
        assert!(job.status().unwrap().message.is_none(), "a superseded preview is not a storage failure");
        job.cancel().unwrap();
        assert_eq!(called.load(Ordering::SeqCst), 1);
        let late = called.clone();
        job.set_cancel_handler(move || {
            late.fetch_add(1, Ordering::SeqCst);
        })
        .unwrap();
        assert_eq!(called.load(Ordering::SeqCst), 2);
        let synced = Job::new("other".into()).unwrap();
        let completed = called.clone();
        synced
            .set_cancel_handler(move || {
                completed.fetch_add(100, Ordering::SeqCst);
            })
            .unwrap();
        synced.update(|status| status.phase = Phase::Synced).unwrap();
        synced.cancel().unwrap();
        assert_eq!(called.load(Ordering::SeqCst), 2, "confirmed publication cannot be cancelled");
        let queued = Job::new("durable".into()).unwrap();
        queued.update(|status| status.saved_locally = true).unwrap();
        queued.cancel().unwrap();
        queued.fail(Error::Storage("upload stopped".into())).unwrap();
        assert_eq!(queued.status().unwrap().phase, Phase::SavedLocally);
        assert!(queued.status().unwrap().message.is_none());
    }
}
