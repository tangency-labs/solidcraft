//! Problem reports filed on the ticket server (tickets.solidcraft.io) from the app: sign-in with a
//! code shown in the app, the report and its attachments sent in resumable chunks, a queue for
//! reports made offline. The network is used only when the user asks (Report, My tickets) or to
//! send a report already waiting in the queue; nothing runs in the background otherwise.
//!
//! The token lives in the system keyring (the browser's storage on the web) through
//! [`crate::agent_chat::backend::Secrets`]; it is never written anywhere else.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, TryRecvError};

use crate::SolveApp;
use crate::agent_chat::backend::HttpRequest;

pub mod agent;
pub mod api;
pub mod mine;
pub mod payload;
pub mod queue;

/// An HTTP answer: status and body, or why there is none.
pub type Reply = Result<(u16, Vec<u8>), String>;
/// Sends a request and answers on the channel (a worker thread on the desktop, `fetch` on the
/// web): the window never waits on the network.
pub type Http = Arc<dyn Fn(HttpRequest, Sender<Reply>) + Send + Sync>;

/// The token's name in the keyring.
pub const SECRET_ID: &str = "tickets.solidcraft.io";
/// How often a waiting report is tried again while the app runs, in ms.
const RETRY_MS: f64 = 5.0 * 60_000.0;

/// A request on its way.
pub struct Call(Receiver<Reply>);

impl Call {
    fn start(http: &Http, request: HttpRequest) -> Self {
        let (tx, rx) = std::sync::mpsc::channel();
        http(request, tx);
        Self(rx)
    }
    /// The answer once it came.
    fn poll(&self) -> Option<Reply> {
        match self.0.try_recv() {
            Ok(r) => Some(r),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => Some(Err("the request ended without an answer".into())),
        }
    }
}

/// Signing in with a code (device authorization).
#[derive(Default)]
pub enum SignIn {
    #[default]
    Idle,
    /// Asking the server for a code.
    Asking(Call),
    /// The code is shown and the browser opened; polling until the user approves.
    Waiting {
        code: api::DeviceCode,
        next: f64,
        until: f64,
        call: Option<Call>,
    },
    Failed(String),
}

/// Where sending a report is.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Step {
    /// Open a report draft (no account needed); its files go with the draft's upload token.
    Draft,
    Ticket,
    Open(usize),
    Resume(usize),
    Chunk(usize),
    Finish(usize),
}

pub struct Sending {
    pub report: payload::Outgoing,
    /// Its queue folder, when it came from (or went to) the queue.
    folder: Option<PathBuf>,
    step: Step,
    call: Option<Call>,
    /// The current file's part size and the parts it still needs (numbered from 1).
    part_size: usize,
    missing: std::collections::VecDeque<usize>,
    pub sent: usize,
    pub total: usize,
    /// Through a draft the person files in the browser (no sign-in in the app), rather than
    /// filed directly with a remembered token.
    via_draft: bool,
    /// The draft, once the server opened it.
    draft: Option<api::Draft>,
}

/// How the last report went.
#[derive(Clone, Debug, PartialEq)]
pub enum Outcome {
    /// The draft is on the server with its files, and its page opened in the browser.
    Opened,
    Filed(u64),
    Queued,
    Failed(String),
    Cancelled(Option<u64>),
}

#[derive(Default)]
pub struct Tickets {
    token: Option<String>,
    token_read: bool,
    /// The keyring could not keep the token: it lasts for this run only.
    pub token_unsaved: bool,
    pub sign_in: SignIn,
    pub apps: Vec<api::App>,
    apps_call: Option<Call>,
    pub sending: Option<Sending>,
    /// A report waiting for sign-in to finish.
    pending: Option<payload::Outgoing>,
    pub outcome: Option<Outcome>,
    /// The queue folder; None: no queue (reports made offline wait in memory for this run).
    pub queue_dir: Option<PathBuf>,
    memory_queue: Vec<payload::Outgoing>,
    /// Help › My tickets.
    pub mine: mine::Mine,
    retry_at: f64,
    /// A draft's page to open in the browser (on the next frame, which has the context).
    open_url: Option<String>,
    /// The agent report being sent or waiting in the dialog (`agent`), and the agents' reports
    /// waiting their turn.
    pub agent: Option<String>,
    pub agent_waiting: Vec<solidcraft_engine::report_issue::Request>,
    /// The last draft's page opened in the browser (for an agent's report status).
    pub opened_url: Option<String>,
    /// A draft opened in the browser, watched until it is filed: (draft, next poll, give up at,
    /// the poll on its way).
    watch: Option<(api::Draft, f64, f64, Option<Call>)>,
}

impl Tickets {
    pub fn new() -> Self {
        Self { queue_dir: if cfg!(test) { None } else { queue::default_dir() }, ..Default::default() }
    }
}

fn secrets(app: &SolveApp) -> Option<&Arc<dyn crate::agent_chat::backend::Secrets>> {
    app.services.ticket_secrets.as_ref()
}

/// The token, read from the keyring the first time it is needed.
pub fn token(app: &mut SolveApp) -> Option<String> {
    if !app.tickets.token_read {
        app.tickets.token_read = true;
        app.tickets.token = secrets(app).and_then(|s| s.get(SECRET_ID).ok().flatten()).filter(|t| !t.is_empty());
    }
    app.tickets.token.clone()
}

pub fn signed_in(app: &mut SolveApp) -> bool {
    token(app).is_some()
}

/// Forget the token, here and in the keyring.
pub fn sign_out(app: &mut SolveApp) {
    if let Some(s) = secrets(app) {
        let _ = s.delete(SECRET_ID);
    }
    app.tickets.token = None;
    app.tickets.token_read = true;
    app.tickets.token_unsaved = false;
    app.tickets.sign_in = SignIn::Idle;
    app.tickets.apps.clear();
}

fn keep_token(app: &mut SolveApp, token: String) {
    app.tickets.token_unsaved = secrets(app).is_none_or(|s| s.set(SECRET_ID, &token).is_err());
    app.tickets.token = Some(token);
    app.tickets.token_read = true;
}

/// Start signing in: the server gives a code, the browser opens its page.
pub fn begin_sign_in(app: &mut SolveApp) {
    let Some(http) = app.services.tickets_http.clone() else {
        app.tickets.sign_in = SignIn::Failed("This build of SolidCraft cannot reach the ticket server.".into());
        return;
    };
    app.tickets.sign_in = SignIn::Asking(Call::start(&http, api::device_code()));
}

/// The apps tickets can be filed under, asked for when the report opens (signed in only).
pub fn load_apps(app: &mut SolveApp) {
    if app.tickets.apps_call.is_some() || !app.tickets.apps.is_empty() {
        return;
    }
    let (Some(http), Some(token)) = (app.services.tickets_http.clone(), token(app)) else { return };
    app.tickets.apps_call = Some(Call::start(&http, api::apps(&token)));
}

/// Send `report`: as a draft whose page opens in the browser, where the person signs in if
/// needed and files it; or filed directly when this computer has a remembered token.
pub fn send(app: &mut SolveApp, report: payload::Outgoing) {
    app.tickets.outcome = None;
    let via_draft = token(app).is_none();
    start(app, report, None, via_draft);
}

/// Send the oldest report waiting in the queue now (the dialog's Send waiting reports).
pub fn send_waiting(app: &mut SolveApp) {
    if app.tickets.sending.is_some() {
        return;
    }
    let via_draft = token(app).is_none();
    app.tickets.outcome = None;
    if !app.tickets.memory_queue.is_empty() {
        let report = app.tickets.memory_queue.remove(0);
        start(app, report, None, via_draft);
        return;
    }
    let Some(dir) = app.tickets.queue_dir.clone() else { return };
    if let Some(folder) = queue::list(&dir).into_iter().next() {
        match queue::load(&folder) {
            Ok(report) => start(app, report, Some(folder), via_draft),
            Err(_) => queue::remove(&folder),
        }
    }
}

fn start(app: &mut SolveApp, mut report: payload::Outgoing, folder: Option<PathBuf>, via_draft: bool) {
    if via_draft {
        // A draft is new each time: every file goes with it.
        for f in &mut report.files {
            f.upload = None;
            f.done = f.done && f.bytes.is_empty();
        }
        report.files.retain(|f| !f.done);
        report.filed = None;
    }
    let total = 1 + report.files.iter().filter(|f| !f.done).map(|f| f.bytes.len()).sum::<usize>();
    let step = if via_draft {
        Some(Step::Draft)
    } else if report.filed.is_some() {
        next_file(&report, 0)
    } else {
        Some(Step::Ticket)
    };
    let mut sending = Sending {
        report,
        folder,
        step: step.unwrap_or(Step::Ticket),
        call: None,
        part_size: api::MAX_PART,
        missing: Default::default(),
        sent: 0,
        total,
        via_draft,
        draft: None,
    };
    if step.is_none() {
        // Filed and uploaded already: only the queue entry is left.
        finish(app, sending);
        return;
    }
    sending.sent = usize::from(sending.report.filed.is_some());
    app.tickets.sending = Some(sending);
}

/// Stop sending. What was filed stays filed.
pub fn cancel(app: &mut SolveApp) {
    if let Some(s) = app.tickets.sending.take() {
        // A queued report stays queued, with how far it got.
        if let Some(folder) = &s.folder {
            let _ = queue::save(folder, &s.report);
        }
        app.tickets.outcome = Some(Outcome::Cancelled(s.report.filed));
    }
    app.tickets.pending = None;
    if matches!(app.tickets.sign_in, SignIn::Asking(_) | SignIn::Waiting { .. }) {
        app.tickets.sign_in = SignIn::Idle;
    }
}

fn next_file(report: &payload::Outgoing, from: usize) -> Option<Step> {
    let i = (from..report.files.len()).find(|i| report.files.get(*i).is_some_and(|f| !f.done))?;
    Some(if report.files.get(i).is_some_and(|f| f.upload.is_some()) { Step::Resume(i) } else { Step::Open(i) })
}

/// How many reports wait in the queue.
pub fn queued(app: &SolveApp) -> usize {
    app.tickets.queue_dir.as_deref().map_or(0, |d| queue::list(d).len()) + app.tickets.memory_queue.len()
}

fn enqueue(app: &mut SolveApp, s: Sending) {
    let Sending { report, folder, .. } = s;
    keep_for_later(app, report, folder);
}

/// Keep `report` to send when the server can be reached (its queue folder, if it has one).
fn keep_for_later(app: &mut SolveApp, report: payload::Outgoing, folder: Option<PathBuf>) {
    match (folder, app.tickets.queue_dir.clone()) {
        (Some(folder), _) => {
            let _ = queue::save(&folder, &report);
        }
        (None, Some(dir)) => {
            if let Err(e) = queue::put(&dir, &report) {
                app.tickets.outcome = Some(Outcome::Failed(format!("Could not keep the report to send later: {e}")));
                return;
            }
        }
        (None, None) => app.tickets.memory_queue.push(report),
    }
    app.tickets.outcome = Some(Outcome::Queued);
    app.tickets.retry_at = crate::now_ms() + RETRY_MS;
}

/// How long a draft opened in the browser is watched for being filed, in ms.
const WATCH_MS: f64 = 30.0 * 60_000.0;
/// Between two looks at it.
const WATCH_EVERY_MS: f64 = 5_000.0;

fn finish(app: &mut SolveApp, s: Sending) {
    if let Some(folder) = &s.folder {
        queue::remove(folder);
        // The next waiting report goes now, not in five minutes.
        app.tickets.retry_at = 0.0;
    }
    match s.draft {
        Some(draft) => {
            app.tickets.open_url = Some(draft.url.clone());
            app.tickets.opened_url = Some(draft.url.clone());
            app.tickets.outcome = Some(Outcome::Opened);
            let now = crate::now_ms();
            app.tickets.watch = Some((draft, now + WATCH_EVERY_MS, now + WATCH_MS, None));
        }
        None => app.tickets.outcome = s.report.filed.map(Outcome::Filed),
    }
}

/// Watch the draft opened in the browser: once it is filed, say so (with the ticket's number),
/// and keep the token the person asked this computer to remember.
fn tick_watch(app: &mut SolveApp, now: f64) {
    let Some((draft, next, until, call)) = app.tickets.watch.take() else { return };
    let Some(http) = app.services.tickets_http.clone() else { return };
    if now > until {
        return;
    }
    let call = match call {
        Some(call) => match call.poll() {
            None => Some(call),
            Some(reply) => match api::answer(reply).map(|v| api::parse_draft_status(&v)) {
                Ok(api::DraftStatus::Claimed { number, token }) => {
                    if let Some(token) = token {
                        keep_token(app, token);
                    }
                    if matches!(app.tickets.outcome, Some(Outcome::Opened)) && number > 0 {
                        app.tickets.outcome = Some(Outcome::Filed(number));
                    }
                    return;
                }
                Ok(api::DraftStatus::Expired) | Err(api::Failure::SignedOut | api::Failure::Refused(_)) => return,
                Ok(api::DraftStatus::Open) | Err(api::Failure::Offline(_)) => {
                    app.tickets.watch = Some((draft, now + WATCH_EVERY_MS, until, None));
                    return;
                }
            },
        },
        None if now >= next => Some(Call::start(&http, api::draft_status(&draft.id, &draft.upload_token))),
        None => None,
    };
    app.tickets.watch = Some((draft, next, until, call));
}

/// Advance sign-in, the apps list and a report being sent; try waiting reports again now and
/// then. Called every frame; does nothing without work.
pub fn tick(app: &mut SolveApp, ctx: &egui::Context) {
    let now = crate::now_ms();
    tick_sign_in(app, ctx, now);
    if let Some(reply) = app.tickets.apps_call.as_ref().and_then(Call::poll) {
        app.tickets.apps_call = None;
        match api::answer(reply) {
            Ok(v) => app.tickets.apps = api::parse_apps(&v),
            Err(api::Failure::SignedOut) => sign_out(app),
            Err(_) => {}
        }
    }
    tick_send(app);
    tick_watch(app, now);
    agent::tick(app, ctx);
    mine::tick(app, now);
    if let Some(url) = app.tickets.open_url.take() {
        ctx.open_url(egui::OpenUrl::new_tab(url));
    }
    retry_queue(app, now);
    if app.tickets.watch.is_some() {
        ctx.request_repaint_after(std::time::Duration::from_millis(1000));
    }
    if app.tickets.sending.is_some()
        || mine::busy(app)
        || app.tickets.apps_call.is_some()
        || matches!(app.tickets.sign_in, SignIn::Asking(_) | SignIn::Waiting { .. })
    {
        ctx.request_repaint_after(std::time::Duration::from_millis(100));
    }
}

fn tick_sign_in(app: &mut SolveApp, ctx: &egui::Context, now: f64) {
    let Some(http) = app.services.tickets_http.clone() else { return };
    match std::mem::take(&mut app.tickets.sign_in) {
        SignIn::Asking(call) => match call.poll() {
            None => app.tickets.sign_in = SignIn::Asking(call),
            Some(reply) => match api::answer(reply).and_then(|v| api::parse_device_code(&v)) {
                // Offline before signing in: the report waits, and goes after the next sign-in.
                Err(api::Failure::Offline(_)) if app.tickets.pending.is_some() => {
                    if let Some(report) = app.tickets.pending.take() {
                        keep_for_later(app, report, None);
                    }
                    app.tickets.sign_in = SignIn::Idle;
                }
                Ok(code) => {
                    ctx.open_url(egui::OpenUrl::new_tab(&code.verification));
                    let (next, until) = (now + code.interval_s * 1000.0, now + code.expires_in_s * 1000.0);
                    app.tickets.sign_in = SignIn::Waiting { code, next, until, call: None };
                }
                Err(e) => app.tickets.sign_in = SignIn::Failed(e.text()),
            },
        },
        SignIn::Waiting { mut code, mut next, until, call } => {
            if now > until {
                app.tickets.sign_in = SignIn::Failed("The sign-in code expired. Try again.".into());
                return;
            }
            let call = match call {
                Some(call) => match call.poll() {
                    None => Some(call),
                    Some(reply) => match api::parse_poll(reply) {
                        api::Poll::Token(token) => {
                            keep_token(app, token);
                            app.tickets.sign_in = SignIn::Idle;
                            load_apps(app);
                            if let Some(report) = app.tickets.pending.take() {
                                start(app, report, None, false);
                            }
                            // Reports kept while signed out go next.
                            retry_now(app);
                            if app.tickets.mine.open {
                                mine::refresh(app);
                            }
                            return;
                        }
                        api::Poll::Pending => {
                            next = now + code.interval_s * 1000.0;
                            None
                        }
                        api::Poll::SlowDown => {
                            code.interval_s += 5.0;
                            next = now + code.interval_s * 1000.0;
                            None
                        }
                        api::Poll::Expired => {
                            app.tickets.sign_in = SignIn::Failed("The sign-in code expired. Try again.".into());
                            return;
                        }
                        api::Poll::Denied => {
                            app.tickets.sign_in = SignIn::Failed("Sign-in was declined.".into());
                            app.tickets.pending = None;
                            return;
                        }
                        api::Poll::Failed(api::Failure::Offline(_)) => {
                            next = now + code.interval_s * 1000.0;
                            None
                        }
                        api::Poll::Failed(e) => {
                            app.tickets.sign_in = SignIn::Failed(e.text());
                            return;
                        }
                    },
                },
                None if now >= next => Some(Call::start(&http, api::device_token(&code.device_code))),
                None => None,
            };
            app.tickets.sign_in = SignIn::Waiting { code, next, until, call };
        }
        other => app.tickets.sign_in = other,
    }
}

fn tick_send(app: &mut SolveApp) {
    let Some(mut s) = app.tickets.sending.take() else { return };
    let Some(http) = app.services.tickets_http.clone() else {
        enqueue(app, s);
        return;
    };
    // A draft's files go with its upload token; a direct report with this computer's token.
    let token = if s.via_draft {
        s.draft.as_ref().map(|d| d.upload_token.clone()).unwrap_or_default()
    } else {
        match token(app) {
            Some(t) => t,
            None => {
                start_as_draft(app, s);
                return;
            }
        }
    };
    let Some(call) = s.call.take() else {
        s.call = Some(Call::start(&http, request(&s, &token)));
        app.tickets.sending = Some(s);
        return;
    };
    let Some(reply) = call.poll() else {
        s.call = Some(call);
        app.tickets.sending = Some(s);
        return;
    };
    let value = match api::answer(reply) {
        Ok(v) => v,
        Err(api::Failure::Offline(_)) => return enqueue(app, s),
        Err(api::Failure::SignedOut) if s.via_draft => {
            app.tickets.outcome = Some(Outcome::Failed("The report's draft expired on the server. Send it again.".into()));
            return;
        }
        Err(api::Failure::SignedOut) => {
            // The remembered token was revoked: forget it and go through the browser instead.
            sign_out(app);
            start_as_draft(app, s);
            return;
        }
        Err(api::Failure::Refused(e)) => {
            app.tickets.outcome = Some(Outcome::Failed(e));
            // The server will not take it as it is: it stops waiting (its files stay on disk).
            if let Some(folder) = &s.folder {
                queue::refused(folder);
            }
            return;
        }
    };
    let step = match s.step {
        Step::Draft => match api::parse_draft(&value) {
            Ok(d) => {
                s.draft = Some(d);
                s.sent += 1;
                next_file(&s.report, 0)
            }
            Err(e) => {
                app.tickets.outcome = Some(Outcome::Failed(e.text()));
                return;
            }
        },
        Step::Ticket => match api::parse_ticket(&value) {
            Ok(n) => {
                s.report.filed = Some(n);
                s.sent += 1;
                next_file(&s.report, 0)
            }
            Err(e) => {
                app.tickets.outcome = Some(Outcome::Failed(e.text()));
                return;
            }
        },
        Step::Open(i) => match api::parse_upload(&value) {
            Ok(u) => {
                if let Some(f) = s.report.files.get_mut(i) {
                    f.upload = Some(u.id);
                }
                s.part_size = u.part_size;
                s.missing = u.missing.into();
                Some(chunk_or_finish(&s, i))
            }
            Err(e) => {
                app.tickets.outcome = Some(Outcome::Failed(e.text()));
                return;
            }
        },
        Step::Resume(i) => match api::parse_upload(&value) {
            Ok(u) => {
                s.part_size = u.part_size;
                s.missing = u.missing.into();
                Some(chunk_or_finish(&s, i))
            }
            // The server no longer has it (expired): open it again.
            Err(_) => {
                if let Some(f) = s.report.files.get_mut(i) {
                    f.upload = None;
                }
                Some(Step::Open(i))
            }
        },
        Step::Chunk(i) => {
            if let Some(n) = s.missing.pop_front() {
                s.sent += part(&s, i, n).len();
            }
            Some(chunk_or_finish(&s, i))
        }
        Step::Finish(i) => {
            if let Some(f) = s.report.files.get_mut(i) {
                f.done = true;
                // A draft that goes again later needs its files again; a filed ticket doesn't.
                if !s.via_draft {
                    f.bytes = Vec::new();
                }
            }
            next_file(&s.report, i + 1)
        }
    };
    if let Some(folder) = &s.folder
        && !matches!(step, Some(Step::Chunk(_)))
    {
        let _ = queue::save(folder, &s.report);
    }
    match step {
        Some(step) => {
            s.step = step;
            app.tickets.sending = Some(s);
        }
        None => finish(app, s),
    }
}

/// Go on as a draft (the remembered token is gone): the ticket wasn't filed, so nothing is lost.
fn start_as_draft(app: &mut SolveApp, s: Sending) {
    if s.report.filed.is_some() {
        app.tickets.outcome = Some(Outcome::Failed("Your sign-in on this computer is no longer valid. Sign in again to finish the upload.".into()));
        return;
    }
    let outcome = app.tickets.outcome.clone();
    start(app, s.report, s.folder, true);
    app.tickets.outcome = outcome;
}

fn chunk_or_finish(s: &Sending, i: usize) -> Step {
    if s.missing.is_empty() { Step::Finish(i) } else { Step::Chunk(i) }
}

/// Part `n` (from 1) of file `i`.
fn part(s: &Sending, i: usize, n: usize) -> &[u8] {
    let Some(f) = s.report.files.get(i) else { return &[] };
    let start = n.saturating_sub(1).saturating_mul(s.part_size);
    f.bytes.get(start.min(f.bytes.len())..start.saturating_add(s.part_size).min(f.bytes.len())).unwrap_or_default()
}

fn request(s: &Sending, token: &str) -> HttpRequest {
    let filed = if s.via_draft { None } else { s.report.filed };
    let file = |i: usize| s.report.files.get(i);
    let upload = |i: usize| file(i).and_then(|f| f.upload.clone()).unwrap_or_default();
    match s.step {
        Step::Draft => api::create_draft(&s.report.ticket),
        Step::Ticket => api::create_ticket(token, &s.report.ticket),
        Step::Open(i) => {
            let f = file(i);
            api::create_upload(
                token,
                filed,
                f.map_or("", |f| &f.name),
                f.map_or(&[][..], |f| &f.bytes),
                f.map_or("application/octet-stream", |f| &f.content_type),
                f.map_or("", |f| &f.kind),
            )
        }
        Step::Resume(i) => api::upload_status(token, &upload(i)),
        Step::Chunk(i) => {
            let n = s.missing.front().copied().unwrap_or(1);
            api::upload_part(token, &upload(i), n, part(s, i, n))
        }
        Step::Finish(i) => api::finish_upload(token, &upload(i)),
    }
}

/// Reports waiting in the queue go again: when the app starts and every few minutes, only when
/// signed in and nothing else is being sent.
fn retry_queue(app: &mut SolveApp, now: f64) {
    if now < app.tickets.retry_at || app.tickets.sending.is_some() || app.services.tickets_http.is_none() {
        return;
    }
    app.tickets.retry_at = now + RETRY_MS;
    if !app.tickets.memory_queue.is_empty() && token(app).is_some() {
        let report = app.tickets.memory_queue.remove(0);
        start(app, report, None, false);
        return;
    }
    let Some(dir) = app.tickets.queue_dir.clone() else { return };
    let Some(folder) = queue::list(&dir).into_iter().next() else { return };
    if token(app).is_none() {
        return;
    }
    match queue::load(&folder) {
        Ok(report) => {
            // A queued report goes quietly: its result is not the dialog's.
            let outcome = app.tickets.outcome.clone();
            start(app, report, Some(folder), false);
            app.tickets.outcome = outcome;
        }
        Err(_) => queue::remove(&folder),
    }
}

/// Send the queued reports now (after a report was queued, the dialog's Try again).
pub fn retry_now(app: &mut SolveApp) {
    app.tickets.retry_at = 0.0;
}
