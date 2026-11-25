//! `diagnostics.report_issue`: an agent driving SolidCraft files a problem report. The engine
//! only checks the request and hands it to the app, which gathers what the report's dialog would
//! (system info, the failed operation, the log, a screenshot), sends it as a draft and opens the
//! draft in the person's browser, where they review and file it. The app answers with the report's
//! status, which `diagnostics.report_issue_status` reads.
//!
//! The person's design and crash dumps are theirs to share: an agent's request for them shows the
//! report dialog with those boxes for the person to tick, and never sends them on its own.

use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use serde_json::Value;

/// A report an agent asked for, waiting for the app.
#[derive(Clone, Debug, PartialEq)]
pub struct Request {
    pub id: String,
    /// The session (design) it was asked in ([`crate::Session::uid`]): the app that shows it
    /// takes it.
    pub session: u64,
    /// The command's parameters, checked against its contract.
    pub params: Value,
}

#[derive(Default)]
struct Mailbox {
    requests: Vec<Request>,
    statuses: HashMap<String, Value>,
}

static MAILBOX: Mutex<Option<Mailbox>> = Mutex::new(None);
static NEXT: AtomicU64 = AtomicU64::new(1);
static HOST: AtomicBool = AtomicBool::new(false);

fn with<R>(f: impl FnOnce(&mut Mailbox) -> R) -> Option<R> {
    let mut guard = MAILBOX.lock().ok()?;
    Some(f(guard.get_or_insert_with(Mailbox::default)))
}

/// The app takes reports (it calls this when it starts). Without it the command refuses.
pub fn attach_host() {
    HOST.store(true, Ordering::SeqCst);
}

pub fn host_attached() -> bool {
    HOST.load(Ordering::SeqCst)
}

/// Queue a request; its id.
pub(crate) fn submit(session: u64, params: Value) -> String {
    let id = format!("r{}", NEXT.fetch_add(1, Ordering::SeqCst));
    let status = serde_json::json!({"report": id, "status": "pending", "message": "Waiting for SolidCraft to gather the report."});
    with(|m| {
        m.requests.push(Request { id: id.clone(), session, params });
        m.statuses.insert(id.clone(), status);
    });
    id
}

/// The requests asked in session `session`, oldest first (taken: the app owns them now).
pub fn take_requests(session: u64) -> Vec<Request> {
    with(|m| {
        let (mine, rest) = std::mem::take(&mut m.requests).into_iter().partition(|r| r.session == session);
        m.requests = rest;
        mine
    })
    .unwrap_or_default()
}

/// The app's word on a report: `status` is its whole reply.
pub fn set_status(id: &str, status: Value) {
    with(|m| {
        if m.statuses.len() > 200 {
            m.statuses.clear();
        }
        m.statuses.insert(id.to_string(), status);
    });
}

/// What the app last said about report `id`.
pub fn status(id: &str) -> Option<Value> {
    with(|m| m.statuses.get(id).cloned()).flatten()
}
