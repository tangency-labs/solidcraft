//! Application-owned asynchronous operations. Native workers and browser tasks expose the
//! same durable-save boundary; polling never substitutes the editor's current document.
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU64, Ordering},
};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{Connection, Destination, Error, Result, Snapshot, StagedSave};

static NEXT_JOB: AtomicU64 = AtomicU64::new(1);

pub enum Operation {
    Test(Connection),
    List(Connection),
    Open { connection: Connection, destination: Destination },
    Save { connection: Connection, save: Box<StagedSave>, synchronize: bool },
    Synchronize(Connection),
}

pub trait Host: Send + Sync {
    fn submit(&self, operation: Operation, git_executable: String) -> Result<Job>;
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
}

struct State {
    status: JobStatus,
    cancelled: bool,
    opened: Option<Snapshot>,
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
            },
            cancelled: false,
            opened: None,
        }))))
    }

    pub fn status(&self) -> Result<JobStatus> {
        self.0.lock().map(|state| state.status.clone()).map_err(|_| Error::Storage("cloud operation state unavailable".into()))
    }

    pub fn update(&self, change: impl FnOnce(&mut JobStatus)) -> Result<()> {
        let mut state = self.0.lock().map_err(|_| Error::Storage("cloud operation state unavailable".into()))?;
        change(&mut state.status);
        Ok(())
    }

    pub fn cancel(&self) -> Result<()> {
        let mut state = self.0.lock().map_err(|_| Error::Storage("cloud operation state unavailable".into()))?;
        state.cancelled = true;
        if !state.status.saved_locally {
            state.status.phase = Phase::Cancelled;
        }
        // A durable queued save is retained even when the user cancels its upload.
        Ok(())
    }

    pub fn cancelled(&self) -> Result<bool> {
        self.0.lock().map(|state| state.cancelled).map_err(|_| Error::Storage("cloud operation state unavailable".into()))
    }

    pub fn fail(&self, error: Error) -> Result<()> {
        self.update(|status| {
            status.phase = if status.saved_locally { Phase::SavedLocally } else { Phase::Failed };
            status.message = Some(error.to_string());
        })
    }

    pub fn opened(&self, snapshot: Snapshot) -> Result<()> {
        let mut state = self.0.lock().map_err(|_| Error::Storage("cloud operation state unavailable".into()))?;
        if !state.cancelled {
            state.opened = Some(snapshot);
            state.status.phase = Phase::Ready;
        }
        Ok(())
    }

    pub fn take_opened(&self) -> Result<Snapshot> {
        let mut state = self.0.lock().map_err(|_| Error::Storage("cloud operation state unavailable".into()))?;
        if state.cancelled {
            return Err(Error::Storage("opening cancelled".into()));
        }
        state.opened.take().ok_or_else(|| Error::Storage("opening has not completed".into()))
    }
}
