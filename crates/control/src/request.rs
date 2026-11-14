//! A control request as the window gets it, and the design watch subscribed connections wait on.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Condvar, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use serde_json::{Value, json};

pub type ControlResponse = Value;

pub struct ControlRequest {
    pub method: String,
    pub params: Value,
    pub reply: Sender<ControlResponse>,
    /// Set once the window starts on it; until then it waits its turn (`control.running`, #3654).
    pub started: std::sync::Arc<std::sync::atomic::AtomicBool>,
    /// Chat requests already admitted by the gate may outlive a tab switch. This turn's
    /// validity follows them through queues, computation and cursor animation.
    pub chat_turn: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    /// Where the request is, shared with the connection that sent it so it can cancel it
    /// (`control.cancel`, #1989).
    pub ticket: std::sync::Arc<crate::Ticket>,
    /// Told apart from every other request of this run: names its row while it waits.
    pub serial: u64,
}

impl ControlRequest {
    pub fn new(method: impl Into<String>, params: Value) -> (Self, std::sync::mpsc::Receiver<ControlResponse>) {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        let (tx, rx) = std::sync::mpsc::channel();
        let serial = NEXT.fetch_add(1, Ordering::Relaxed);
        (Self { method: method.into(), params, reply: tx, started: Default::default(), chat_turn: None, ticket: Default::default(), serial }, rx)
    }

    /// Answer a request whose agent chat turn has ended, and say so: such a request is not run.
    pub fn refuse_cancelled_chat(&self) -> bool {
        if chat_turn_cancelled(self.chat_turn.as_deref()) {
            let _ = self.reply.send(cancelled_chat_response());
            true
        } else {
            false
        }
    }
}

/// Whether the agent chat turn a request belongs to has ended (`None`: it belongs to none).
pub fn chat_turn_cancelled(turn: Option<&std::sync::atomic::AtomicBool>) -> bool {
    turn.is_some_and(|active| !active.load(std::sync::atomic::Ordering::Relaxed))
}

/// The reply to a request whose agent chat turn has ended.
pub fn cancelled_chat_response() -> Value {
    json!({"ok": false, "error": "not run: the agent chat turn ended or its document tab was left"})
}

/// The active design's id and revision for control connections that sent `control.subscribe`
/// (#6985): the window publishes them each frame while one is subscribed, and each such
/// connection waits for them to move, then sends the app's change event (`Protocol::change_event`).
#[derive(Default)]
pub struct DocumentWatch {
    subscribers: AtomicUsize,
    now: Mutex<Option<(String, u64)>>,
    moved: Condvar,
}

/// The app's [`DocumentWatch`].
pub static DOCUMENT_WATCH: DocumentWatch = DocumentWatch::new();

impl DocumentWatch {
    pub const fn new() -> Self {
        DocumentWatch { subscribers: AtomicUsize::new(0), now: Mutex::new(None), moved: Condvar::new() }
    }

    fn now(&self) -> MutexGuard<'_, Option<(String, u64)>> {
        self.now.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The design as last published (`None` before anything was).
    pub fn current(&self) -> Option<(String, u64)> {
        self.now().clone()
    }

    /// The design as it is now; waiters wake only when it moved. Only the latest is kept, so
    /// changes made faster than a connection sends them go out as one.
    pub fn publish(&self, document_id: &str, revision: u64) {
        let mut now = self.now();
        if now.as_ref().is_some_and(|(id, r)| id == document_id && *r == revision) {
            return;
        }
        *now = Some((document_id.to_string(), revision));
        self.moved.notify_all();
    }

    /// Whether a connection is subscribed: the window publishes only then.
    pub fn subscribed(&self) -> bool {
        self.subscribers.load(Ordering::Relaxed) > 0
    }

    /// Count a subscribed connection until the returned guard is dropped.
    pub fn join(&'static self) -> Subscriber {
        self.subscribers.fetch_add(1, Ordering::Relaxed);
        Subscriber(self)
    }

    /// Wait up to `wait` for the design to differ from `seen`, or for `stop`; the design then
    /// (`None` before anything was published).
    pub fn wait(&self, seen: &(String, u64), stop: &AtomicBool, wait: Duration) -> Option<(String, u64)> {
        let unmoved = |now: &mut Option<(String, u64)>| now.as_ref() == Some(seen) && !stop.load(Ordering::Relaxed);
        let (now, _) = self.moved.wait_timeout_while(self.now(), wait, unmoved).unwrap_or_else(PoisonError::into_inner);
        now.clone()
    }
}

/// A subscribed connection, counted by its [`DocumentWatch`] while this lives.
pub struct Subscriber(&'static DocumentWatch);

impl Drop for Subscriber {
    fn drop(&mut self) {
        self.0.subscribers.fetch_sub(1, Ordering::Relaxed);
        // Under the lock, so a connection about to wait sees its `stop` set before it does.
        let _now = self.0.now();
        self.0.moved.notify_all();
    }
}
