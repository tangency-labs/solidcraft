//! A control request as the window gets it, and the design watch subscribed connections wait on.

use std::collections::VecDeque;
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
    /// Identity of the authenticated TCP connection, if any. The server holds the only strong
    /// reference. Compare with `ptr_eq` and check `strong_count` without upgrading it.
    pub connection: Option<std::sync::Weak<()>>,
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
        (
            Self {
                method: method.into(),
                params,
                reply: tx,
                started: Default::default(),
                chat_turn: None,
                connection: None,
                ticket: Default::default(),
                serial,
            },
            rx,
        )
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
    json!({"ok": false, "error": format!("{} the agent chat turn ended or its document tab was left", crate::NOT_RUN)})
}

/// The active design's id and revision for control connections that sent `control.subscribe`
/// (#6985): the window publishes them each frame while one is subscribed, and each such
/// connection waits for them to move, then sends the app's change event (`Protocol::change_event`).
/// It also keeps the one-off events the window posts (`post`), which such connections are sent
/// in order, ahead of the change line of the same moment.
#[derive(Default)]
pub struct DocumentWatch {
    subscribers: AtomicUsize,
    state: Mutex<WatchState>,
    moved: Condvar,
}

/// The most one-off events a watch keeps. A connection further behind is told how many it missed
/// (`events.dropped`), so that it reads the document again.
pub const EVENT_LIMIT: usize = 256;

/// What a [`DocumentWatch`] holds, under its lock.
#[derive(Default)]
struct WatchState {
    /// The design as last published.
    design: Option<(String, u64)>,
    /// The kept events with their sequence numbers, oldest first.
    events: VecDeque<(u64, Value)>,
    /// The sequence number the next posted event gets.
    next: u64,
}

/// What a subscribed connection has not been sent yet: see [`DocumentWatch::next`].
pub struct Next {
    /// The design now (`None` before anything was published).
    pub design: Option<(String, u64)>,
    /// The kept events at or after the cursor, oldest first.
    pub events: Vec<Value>,
    /// How many events after the cursor were dropped before the connection could be sent them.
    pub dropped: u64,
    /// Where the connection's cursor is now: after these events.
    pub cursor: u64,
}

/// The app's [`DocumentWatch`].
pub static DOCUMENT_WATCH: DocumentWatch = DocumentWatch::new();

impl DocumentWatch {
    pub const fn new() -> Self {
        DocumentWatch {
            subscribers: AtomicUsize::new(0),
            state: Mutex::new(WatchState { design: None, events: VecDeque::new(), next: 0 }),
            moved: Condvar::new(),
        }
    }

    fn state(&self) -> MutexGuard<'_, WatchState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The design as last published (`None` before anything was).
    pub fn current(&self) -> Option<(String, u64)> {
        self.state().design.clone()
    }

    /// The design as it is now; waiters wake only when it moved. Only the latest is kept, so
    /// changes made faster than a connection sends them go out as one.
    pub fn publish(&self, document_id: &str, revision: u64) {
        let mut state = self.state();
        if state.design.as_ref().is_some_and(|(id, r)| id == document_id && *r == revision) {
            return;
        }
        state.design = Some((document_id.to_string(), revision));
        self.moved.notify_all();
    }

    /// Post a one-off event, `{"event": event, "params": params}`, to the subscribed connections.
    /// Nothing is kept while no connection is subscribed. Events are never combined: each is sent
    /// whole, in the order posted, up to [`EVENT_LIMIT`] kept at a time.
    pub fn post(&self, event: &str, params: Value) {
        if !self.subscribed() {
            return;
        }
        let mut state = self.state();
        let sequence = state.next;
        state.events.push_back((sequence, json!({"event": event, "params": params})));
        state.next += 1;
        while state.events.len() > EVENT_LIMIT {
            state.events.pop_front();
        }
        self.moved.notify_all();
    }

    /// Whether a connection is subscribed: the window publishes only then.
    pub fn subscribed(&self) -> bool {
        self.subscribers.load(Ordering::Relaxed) > 0
    }

    /// Count a subscribed connection until the returned guard is dropped. Its events start at
    /// the first one posted after this call.
    pub fn join(&'static self) -> Subscriber {
        self.subscribers.fetch_add(1, Ordering::Relaxed);
        let from = self.state().next;
        Subscriber { watch: self, from }
    }

    /// Wait up to `wait` for the design to differ from `seen`, or for `stop`; the design then
    /// (`None` before anything was published).
    pub fn wait(&self, seen: &(String, u64), stop: &AtomicBool, wait: Duration) -> Option<(String, u64)> {
        let unmoved = |state: &mut WatchState| state.design.as_ref() == Some(seen) && !stop.load(Ordering::Relaxed);
        let (state, _) = self.moved.wait_timeout_while(self.state(), wait, unmoved).unwrap_or_else(PoisonError::into_inner);
        state.design.clone()
    }

    /// Wait up to `wait` until the design differs from `seen`, an event at or after `cursor` is
    /// posted, or `stop` is set; then what the connection has not been sent yet. Events that fell
    /// out of the kept ones are counted in `dropped`.
    pub fn next(&self, seen: &(String, u64), cursor: u64, stop: &AtomicBool, wait: Duration) -> Next {
        let quiet = |state: &mut WatchState| state.design.as_ref() == Some(seen) && state.next <= cursor && !stop.load(Ordering::Relaxed);
        let (state, _) = self.moved.wait_timeout_while(self.state(), wait, quiet).unwrap_or_else(PoisonError::into_inner);
        let kept = u64::try_from(state.events.len()).unwrap_or(u64::MAX);
        let oldest = state.next.saturating_sub(kept);
        let dropped = oldest.saturating_sub(cursor);
        let events: Vec<Value> = state.events.iter().filter(|(sequence, _)| *sequence >= cursor).map(|(_, event)| event.clone()).collect();
        Next { design: state.design.clone(), events, dropped, cursor: state.next }
    }
}

/// A subscribed connection, counted by its [`DocumentWatch`] while this lives.
pub struct Subscriber {
    watch: &'static DocumentWatch,
    from: u64,
}

impl Subscriber {
    /// The sequence number of the first event this subscriber is sent: the first posted after it
    /// joined.
    pub fn from(&self) -> u64 {
        self.from
    }
}

impl Drop for Subscriber {
    fn drop(&mut self) {
        self.watch.subscribers.fetch_sub(1, Ordering::Relaxed);
        // Under the lock, so a connection about to wait sees its `stop` set before it does.
        let _state = self.watch.state();
        self.watch.moved.notify_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The event `{"event": "step", "params": n}`, as `post("step", n)` queues it.
    fn step(n: usize) -> Value {
        json!({"event": "step", "params": n})
    }

    #[test]
    fn post_while_nobody_is_subscribed_keeps_nothing() {
        static WATCH: DocumentWatch = DocumentWatch::new();
        WATCH.post("step", json!(0));
        let subscriber = WATCH.join();
        assert_eq!(subscriber.from(), 0, "nothing was kept before the join");
    }

    #[test]
    fn posted_events_come_back_in_order_from_the_join() {
        static WATCH: DocumentWatch = DocumentWatch::new();
        WATCH.publish("d-1", 1);
        let subscriber = WATCH.join();
        let from = subscriber.from();
        let stop = AtomicBool::new(false);
        let seen = ("d-1".to_string(), 1);
        WATCH.post("step", json!(1));
        WATCH.post("step", json!(2));
        let first = WATCH.next(&seen, from, &stop, Duration::from_secs(5));
        assert_eq!(first.events, vec![step(1), step(2)]);
        assert_eq!((first.dropped, first.cursor, first.design.as_ref()), (0, from + 2, Some(&seen)));

        WATCH.post("step", json!(3));
        let second = WATCH.next(&seen, first.cursor, &stop, Duration::from_secs(5));
        assert_eq!((second.events, second.dropped, second.cursor), (vec![step(3)], 0, from + 3));
    }

    #[test]
    fn overflow_reports_what_was_dropped_and_keeps_the_newest() {
        static WATCH: DocumentWatch = DocumentWatch::new();
        WATCH.publish("d-1", 1);
        let subscriber = WATCH.join();
        let from = subscriber.from();
        let stop = AtomicBool::new(false);
        let seen = ("d-1".to_string(), 1);
        for n in 0..EVENT_LIMIT + 10 {
            WATCH.post("step", json!(n));
        }
        let got = WATCH.next(&seen, from, &stop, Duration::from_secs(5));
        assert_eq!(got.dropped, 10);
        assert_eq!(got.events.len(), EVENT_LIMIT);
        assert_eq!(got.events.first(), Some(&step(10)));
        assert_eq!(got.events.last(), Some(&step(EVENT_LIMIT + 9)));
        assert_eq!(got.cursor, from + EVENT_LIMIT as u64 + 10);
    }

    #[test]
    fn next_wakes_on_a_post() {
        static WATCH: DocumentWatch = DocumentWatch::new();
        WATCH.publish("d-1", 1);
        let subscriber = WATCH.join();
        let from = subscriber.from();
        let stop = AtomicBool::new(false);
        let seen = ("d-1".to_string(), 1);
        let started = std::time::Instant::now();
        let got = std::thread::scope(|scope| {
            scope.spawn(|| {
                std::thread::sleep(Duration::from_millis(50));
                WATCH.post("step", json!(7));
            });
            WATCH.next(&seen, from, &stop, Duration::from_secs(10))
        });
        assert_eq!(got.events, vec![step(7)]);
        assert!(started.elapsed() < Duration::from_secs(5), "woke at the post, not at the timeout");
    }
}
