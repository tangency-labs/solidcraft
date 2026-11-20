//! Problem reports on the ticket server (tickets.solidcraft.io), always through the browser:
//! Send creates a report draft (no account needed), uploads the ticked attachments in resumable
//! parts and opens the draft's page in the browser, where the person signs in and files it. The
//! app keeps no account details: no sign-in, no token. A report that cannot reach the server is
//! kept in a queue folder until the person sends it again.
//!
//! The network is used only when the person sends a report (or an agent asks for one,
//! `agent`); nothing runs in the background.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, TryRecvError};

use crate::SolveApp;
use crate::agent_chat::backend::HttpRequest;

pub mod agent;
pub mod api;
pub mod payload;
pub mod queue;

/// An HTTP answer: status and body, or why there is none.
pub type Reply = Result<(u16, Vec<u8>), String>;
/// Sends a request and answers on the channel (a worker thread on the desktop, `fetch` on the
/// web): the window never waits on the network.
pub type Http = Arc<dyn Fn(HttpRequest, Sender<Reply>) + Send + Sync>;

/// The keyring entry test builds kept a sign-in in; the app removes it once (it keeps none).
pub const SECRET_ID: &str = "tickets.solidcraft.io";

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

/// Where sending a report is.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Step {
    /// Open the report's draft; its files go with the draft's upload token.
    Draft,
    Open(usize),
    Resume(usize),
    Chunk(usize),
    Finish(usize),
}

/// A failed request's phase and attachment. Include the file as well as the HTTP path:
/// /api/uploads has no file name, and a timed-out request otherwise gives no clue which
/// attachment the app was sending (#17932).
fn step_context(step: Step, files: &[payload::File], missing: &std::collections::VecDeque<usize>) -> String {
    let file =
        |i: usize| files.get(i).map_or("attachment", |file| file.name.as_str()).chars().filter(|c| !c.is_control()).take(100).collect::<String>();
    match step {
        Step::Draft => "create report draft".into(),
        Step::Open(i) => format!("start upload of {}", file(i)),
        Step::Resume(i) => format!("resume upload of {}", file(i)),
        Step::Chunk(i) => format!("upload part {} of {}", missing.front().copied().unwrap_or(1), file(i)),
        Step::Finish(i) => format!("complete upload of {}", file(i)),
    }
}

pub struct Sending {
    pub report: payload::Outgoing,
    /// Its queue folder, when it came from the queue.
    folder: Option<PathBuf>,
    step: Step,
    call: Option<Call>,
    /// The current file's part size and the parts it still needs (numbered from 1).
    part_size: usize,
    missing: std::collections::VecDeque<usize>,
    pub sent: usize,
    pub total: usize,
    /// The draft, once the server opened it.
    draft: Option<api::Draft>,
    /// Times an upload was resumed or started again (parts missing, a hash mismatch): a few, then
    /// the report fails rather than going round for ever.
    redo: u8,
    /// The current step went unanswered once and is being sent again.
    again: bool,
    /// The current request's method and path (`PUT /api/uploads/up1/parts/1`), for the log.
    asked: String,
}

/// How the last report went.
#[derive(Clone, Debug, PartialEq)]
pub enum Outcome {
    /// The draft is on the server with its files, and its page opened in the browser (its
    /// address, to open again).
    Opened(String),
    /// The server could not be reached: the report waits in the queue (why).
    Queued(String),
    /// The server refused it, or something else went wrong (why).
    Failed(String),
    Cancelled,
}

#[derive(Default)]
pub struct Tickets {
    pub sending: Option<Sending>,
    pub outcome: Option<Outcome>,
    /// The report `sending` and `outcome` belong to (`ticket::TicketDraft::id`; 0: an agent's or
    /// a queued one), and the last such id given out.
    pub owner: u64,
    next_owner: u64,
    /// The last report sent, for Retry after a failure.
    pub last: Option<payload::Outgoing>,
    /// The queue folder; None: no queue (reports made offline wait in memory for this run).
    pub queue_dir: Option<PathBuf>,
    memory_queue: Vec<payload::Outgoing>,
    /// A draft's page to open in the browser (on the next frame, which has the context).
    open_url: Option<String>,
    /// The agent report being sent or waiting in the dialog (`agent`), and the agents' reports
    /// waiting their turn.
    pub agent: Option<String>,
    pub agent_waiting: Vec<solidcraft_engine::report_issue::Request>,
    /// An old keyring entry has been looked for (once a run).
    old_token_cleared: bool,
}

impl Tickets {
    pub fn new() -> Self {
        Self { queue_dir: if cfg!(test) { None } else { queue::default_dir() }, ..Default::default() }
    }
}

/// Test builds kept a sign-in in the keyring; the app keeps none, so it goes.
fn clear_old_token(app: &mut SolveApp) {
    if app.tickets.old_token_cleared {
        return;
    }
    app.tickets.old_token_cleared = true;
    if let Some(s) = app.services.ticket_secrets.as_ref()
        && s.get(SECRET_ID).ok().flatten().is_some()
    {
        let _ = s.delete(SECRET_ID);
    }
}

/// Write a step of sending to the app's log.
fn log(app: &mut SolveApp, text: impl Into<String>) {
    app.session.echo(format!("Problem report: {}", text.into()));
}

/// Send `report`: as a draft whose page opens in the browser.
pub fn send(app: &mut SolveApp, report: payload::Outgoing) {
    send_for(app, report, 0);
}

/// A new report's id, for [`send_for`].
pub fn new_owner(app: &mut SolveApp) -> u64 {
    app.tickets.next_owner += 1;
    app.tickets.next_owner
}

/// [`send`] the report with id `owner` (its dialog's): how it goes is that dialog's.
pub fn send_for(app: &mut SolveApp, report: payload::Outgoing, owner: u64) {
    if app.tickets.sending.is_some() {
        return;
    }
    app.tickets.owner = owner;
    app.tickets.outcome = None;
    start(app, report, None);
}

/// Send the last report again (Retry), or the oldest waiting in the queue.
pub fn retry(app: &mut SolveApp, owner: u64) {
    if app.tickets.sending.is_some() {
        return;
    }
    app.tickets.owner = owner;
    app.tickets.outcome = None;
    if !app.tickets.memory_queue.is_empty() {
        let report = app.tickets.memory_queue.remove(0);
        start(app, report, None);
        return;
    }
    if let Some(dir) = app.tickets.queue_dir.clone()
        && let Some(folder) = queue::list(&dir).into_iter().next()
    {
        match queue::load(&folder) {
            Ok(report) => return start(app, report, Some(folder)),
            Err(_) => queue::remove(&folder),
        }
    }
    if let Some(report) = app.tickets.last.clone() {
        start(app, report, None);
    }
}

fn start(app: &mut SolveApp, mut report: payload::Outgoing, folder: Option<PathBuf>) {
    // A draft is new each time: every file goes with it.
    for f in &mut report.files {
        f.upload = None;
        f.done = false;
    }
    report.files.retain(|f| !f.bytes.is_empty());
    report.filed = None;
    app.tickets.last = Some(report.clone());
    let total = 1 + report.files.iter().map(|f| f.bytes.len()).sum::<usize>();
    log(app, format!("sending \"{}\" with {} attachment(s)", report.ticket["title"].as_str().unwrap_or_default(), report.files.len()));
    app.tickets.sending = Some(Sending {
        report,
        folder,
        step: Step::Draft,
        call: None,
        part_size: api::MAX_PART,
        missing: Default::default(),
        sent: 0,
        total,
        draft: None,
        redo: 0,
        again: false,
        asked: String::new(),
    });
}

/// Stop sending. Nothing is filed: the draft, if made, expires on the server.
pub fn cancel(app: &mut SolveApp) {
    if app.tickets.sending.take().is_some() {
        log(app, "cancelled");
        app.tickets.outcome = Some(Outcome::Cancelled);
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

/// The server could not be reached: keep the report until the person sends it again.
fn keep_for_later(app: &mut SolveApp, s: Sending, why: String) {
    log(app, format!("could not reach the server ({why}); the report is kept"));
    let Sending { report, folder, .. } = s;
    match (folder, app.tickets.queue_dir.clone()) {
        // It came from the queue: it is still there.
        (Some(_), _) => {}
        (None, Some(dir)) => {
            if let Err(e) = queue::put(&dir, &report) {
                app.tickets.outcome = Some(Outcome::Failed(format!("{why}; and the report could not be kept to send later: {e}")));
                return;
            }
            // Retry sends it from the queue now, not twice.
            app.tickets.last = None;
        }
        (None, None) => {
            app.tickets.memory_queue.push(report);
            app.tickets.last = None;
        }
    }
    app.tickets.outcome = Some(Outcome::Queued(why));
}

fn fail(app: &mut SolveApp, s: &Sending, why: String) {
    log(app, format!("failed: {why}"));
    // The server will not take it as it is: a queued one stops waiting, and its files go.
    if let Some(folder) = &s.folder {
        queue::remove(folder);
    }
    app.tickets.outcome = Some(Outcome::Failed(why));
}

fn finish(app: &mut SolveApp, s: Sending) {
    if let Some(folder) = &s.folder {
        queue::remove(folder);
    }
    match s.draft {
        Some(draft) => {
            log(app, "opened in the browser");
            app.tickets.open_url = Some(draft.url.clone());
            app.tickets.outcome = Some(Outcome::Opened(draft.url));
        }
        None => app.tickets.outcome = Some(Outcome::Failed("The server did not open the report.".into())),
    }
}

/// Advance a report being sent and the agents' requests. Called every frame; does nothing
/// without work.
pub fn tick(app: &mut SolveApp, ctx: &egui::Context) {
    clear_old_token(app);
    tick_send(app);
    agent::tick(app, ctx);
    if let Some(url) = app.tickets.open_url.take() {
        ctx.open_url(egui::OpenUrl::new_tab(url));
    }
    if app.tickets.sending.is_some() {
        // The answers come from another thread: look for them, and draw the progress.
        ctx.request_repaint_after(std::time::Duration::from_millis(50));
    }
}

fn tick_send(app: &mut SolveApp) {
    let Some(mut s) = app.tickets.sending.take() else { return };
    let Some(http) = app.services.tickets_http.clone() else {
        fail(app, &s, "this build of SolidCraft cannot reach the ticket server".into());
        return;
    };
    let token = s.draft.as_ref().map(|d| d.upload_token.clone()).unwrap_or_default();
    let Some(call) = s.call.take() else {
        let r = request(&s, &token);
        s.asked = format!(
            "{} ({} {})",
            step_context(s.step, &s.report.files, &s.missing),
            r.method,
            r.url.strip_prefix(&api::server()).unwrap_or_default(),
        );
        s.call = Some(Call::start(&http, r));
        app.tickets.sending = Some(s);
        return;
    };
    let Some(reply) = call.poll() else {
        s.call = Some(call);
        app.tickets.sending = Some(s);
        return;
    };
    // Completing an upload the server is missing parts of resumes it; one whose hash didn't
    // match starts again. A few times at most.
    if let (Step::Finish(i), Ok((status @ (409 | 422), _))) = (s.step, &reply) {
        s.redo = s.redo.saturating_add(1);
        s.again = false;
        if s.redo > 3 {
            fail(app, &s, format!("the server did not take an attachment after several tries (HTTP {status})"));
            return;
        }
        s.step = if *status == 409 {
            Step::Resume(i)
        } else {
            if let Some(f) = s.report.files.get_mut(i) {
                f.upload = None;
            }
            Step::Open(i)
        };
        app.tickets.sending = Some(s);
        return;
    }
    let value = match api::answer(reply) {
        Ok(v) => v,
        // No answer: the same step once more on a new connection, then the report waits.
        Err(api::Failure::Offline(why)) if !s.again => {
            log(app, format!("no answer to {} ({why}); trying once more", s.asked));
            s.again = true;
            app.tickets.sending = Some(s);
            return;
        }
        Err(api::Failure::Offline(why)) => {
            let why = format!("{why} (at {})", s.asked);
            return keep_for_later(app, s, why);
        }
        // A draft's upload token gone stale: the draft expired.
        Err(api::Failure::SignedOut) => return fail(app, &s, "the report's draft expired on the server (HTTP 401)".into()),
        Err(api::Failure::Refused(e)) => return fail(app, &s, e),
    };
    let step = match s.step {
        Step::Draft => match api::parse_draft(&value) {
            Ok(d) => {
                log(app, format!("draft {} created", d.id));
                s.draft = Some(d);
                s.sent += 1;
                next_file(&s.report, 0)
            }
            Err(e) => return fail(app, &s, e.text()),
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
            Err(e) => return fail(app, &s, e.text()),
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
            let name = s.report.files.get(i).map(|f| f.name.clone()).unwrap_or_default();
            log(app, format!("uploaded {name}"));
            if let Some(f) = s.report.files.get_mut(i) {
                f.done = true;
            }
            next_file(&s.report, i + 1)
        }
    };
    s.again = false;
    match step {
        Some(step) => {
            s.step = step;
            app.tickets.sending = Some(s);
        }
        None => finish(app, s),
    }
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
    let file = |i: usize| s.report.files.get(i);
    let upload = |i: usize| file(i).and_then(|f| f.upload.clone()).unwrap_or_default();
    match s.step {
        Step::Draft => api::create_draft(&s.report.ticket),
        Step::Open(i) => {
            let f = file(i);
            api::create_upload(
                token,
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Each offline request reports the phase and the file it was working on. File names
    /// may contain control characters: these must not introduce extra log lines.
    #[test]
    fn timeout_step_context_names_the_phase_and_attachment() {
        let files = [
            payload::File {
                name: "log.txt".into(),
                content_type: "text/plain".into(),
                kind: "log".into(),
                bytes: vec![1],
                upload: None,
                done: false,
            },
            payload::File {
                name: "screenshot\n.png".into(),
                content_type: "image/png".into(),
                kind: "screenshot".into(),
                bytes: vec![2],
                upload: None,
                done: false,
            },
        ];
        let parts = std::collections::VecDeque::from([3]);
        assert_eq!(step_context(Step::Draft, &files, &parts), "create report draft");
        assert_eq!(step_context(Step::Open(0), &files, &parts), "start upload of log.txt");
        assert_eq!(step_context(Step::Resume(1), &files, &parts), "resume upload of screenshot.png");
        assert_eq!(step_context(Step::Chunk(1), &files, &parts), "upload part 3 of screenshot.png");
        assert_eq!(step_context(Step::Finish(1), &files, &parts), "complete upload of screenshot.png");
        assert_eq!(step_context(Step::Open(5), &files, &parts), "start upload of attachment");
    }
}
