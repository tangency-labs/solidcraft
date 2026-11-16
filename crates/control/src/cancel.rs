//! Where a control request is, so the connection that sent it can cancel it (`control.cancel`,
//! #1989): shared between the window, which moves it along, and the connection. Whoever changes
//! it first wins.

use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Mutex, PoisonError};

/// A request's place in the app, and how to stop the work computing it.
#[derive(Default)]
pub struct Ticket {
    state: AtomicU8,
    /// It is an export computing on a copy, whose worker writes its file.
    writes_file: AtomicBool,
    /// Tells its worker to stop: set by the window when it hands the request to one.
    stopper: Mutex<Option<Box<dyn Fn() + Send + Sync>>>,
}

impl std::fmt::Debug for Ticket {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Ticket").field("state", &self.state).field("writes_file", &self.writes_file).finish_non_exhaustive()
    }
}

/// Waiting its turn (sent, or held behind another request).
const WAITING: u8 = 0;
/// Running on the window thread (or handed to something that can't stop it, such as the file
/// loader).
const RUNNING: u8 = 1;
/// Computing off the window thread.
const COMPUTING: u8 = 2;
const CANCELLED: u8 = 3;

/// What [`Ticket::cancel`] did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cancel {
    /// It was waiting its turn: it will not run.
    Removed,
    /// It was computing off the window thread: it is answered as stopped and nothing of it is
    /// applied. An export's worker may still write its file.
    Stopped { writes_file: bool },
    /// It runs on the window thread: it will finish.
    Running,
    /// It was cancelled or stopped before.
    Already,
}

impl Ticket {
    /// Cancel the request, unless it already runs on the window thread.
    pub fn cancel(&self) -> Cancel {
        let mut state = self.state.load(Ordering::Acquire);
        loop {
            let done = match state {
                WAITING => Cancel::Removed,
                COMPUTING => Cancel::Stopped { writes_file: self.writes_file.load(Ordering::Acquire) },
                CANCELLED => return Cancel::Already,
                _ => return Cancel::Running,
            };
            match self.state.compare_exchange(state, CANCELLED, Ordering::AcqRel, Ordering::Acquire) {
                Ok(_) => {
                    self.stop_worker();
                    return done;
                }
                Err(now) => state = now,
            }
        }
    }

    pub fn is_cancelled(&self) -> bool {
        self.state.load(Ordering::Acquire) == CANCELLED
    }

    /// The window takes the request to run it: false when it was cancelled.
    pub fn take(&self) -> bool {
        self.moves(WAITING, RUNNING)
    }

    /// The work computing it stops on `stop` (a worker's own stop flag). Cancelled already, it is
    /// stopped at once.
    pub fn on_stop(&self, stop: impl Fn() + Send + Sync + 'static) {
        *self.stopper.lock().unwrap_or_else(PoisonError::into_inner) = Some(Box::new(stop));
        if self.is_cancelled() {
            self.stop_worker();
        }
    }

    /// The window hands it to a worker (`writes_file`: an export on a copy).
    pub fn computing(&self, writes_file: bool) {
        self.writes_file.store(writes_file, Ordering::Release);
        let _ = self.moves(RUNNING, COMPUTING) || self.moves(WAITING, COMPUTING);
    }

    /// Its worker is done and it runs on the window thread now (or it never left): false when it
    /// was cancelled meanwhile. Only the window moves it on from running, so running stays running.
    pub fn back(&self) -> bool {
        self.moves(COMPUTING, RUNNING) || self.state.load(Ordering::Acquire) == RUNNING
    }

    /// Computed, but it waits its turn again (a file opening meanwhile): false when cancelled.
    pub fn wait_again(&self) -> bool {
        self.moves(COMPUTING, WAITING)
    }

    /// The person at the app stopped it (Stop): its worker stops too.
    pub fn stop(&self) {
        self.state.store(CANCELLED, Ordering::Release);
        self.stop_worker();
    }

    fn stop_worker(&self) {
        if let Some(stop) = self.stopper.lock().unwrap_or_else(PoisonError::into_inner).as_ref() {
            stop();
        }
    }

    fn moves(&self, from: u8, to: u8) -> bool {
        self.state.compare_exchange(from, to, Ordering::AcqRel, Ordering::Acquire).is_ok()
    }
}
