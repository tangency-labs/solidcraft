//! Reports an agent asks for (`diagnostics.report_issue`, `solidcraft_engine::report_issue`): the
//! app gathers what the report dialog would and sends it as a draft that opens in the person's
//! browser. An agent cannot agree to send the person's design or crash dump: asking for them opens
//! the report dialog, filled in, for the person to tick them and send it themselves.

use serde_json::{Value, json};
use solidcraft_engine::report_issue::{self as mailbox, Request};

use super::{Outcome, api, payload};
use crate::SolveApp;

/// What an agent asked for, carried by the report dialog it opened.
#[derive(Clone, Debug, PartialEq)]
pub struct Ask {
    pub id: String,
    pub ticks: payload::Ticks,
    pub kind: payload::Kind,
    pub app: String,
}

fn flag(p: &Value, key: &str, default: bool) -> bool {
    p.get(key).and_then(Value::as_bool).unwrap_or(default)
}

fn ask_of(r: &Request) -> Ask {
    let p = &r.params;
    let kind = match p.get("type").and_then(Value::as_str) {
        Some("feature") => payload::Kind::Feature,
        Some("question") => payload::Kind::Question,
        _ => payload::Kind::Bug,
    };
    Ask {
        id: r.id.clone(),
        ticks: payload::Ticks {
            system: flag(p, "system", true),
            repro: flag(p, "repro", true),
            log: flag(p, "log", true),
            screenshot: flag(p, "screenshot", true),
            design: flag(p, "design", false),
            crash: flag(p, "crash", false),
        },
        kind,
        app: p.get("app").and_then(Value::as_str).filter(|a| !a.is_empty()).unwrap_or(api::DEFAULT_APP).chars().take(60).collect(),
    }
}

fn status(id: &str, status: &str, message: &str, extra: Value) {
    let mut v = json!({ "report": id, "status": status, "message": message });
    if let (Some(v), Some(extra)) = (v.as_object_mut(), extra.as_object()) {
        v.extend(extra.clone());
    }
    mailbox::set_status(id, v);
}

/// Take the agents' requests and keep their statuses up to date. Called every frame.
pub fn tick(app: &mut SolveApp, ctx: &egui::Context) {
    if app.services.tickets_http.is_none() {
        return;
    }
    mailbox::attach_host();
    app.tickets.agent_waiting.extend(mailbox::take_requests(app.session.uid));
    // One report at a time: the dialog's or an agent's.
    if app.tickets.sending.is_none() && app.issue_report.draft.is_none() && !app.tickets.agent_waiting.is_empty() {
        let r = app.tickets.agent_waiting.remove(0);
        start(app, ctx, &r);
    }
    report_progress(app);
}

/// The failure an agent's report is about: the open dialog's error, else the preview's.
fn failure(app: &SolveApp) -> Option<String> {
    app.dialog.as_ref().and_then(|d| d.error.clone()).or_else(|| app.preview.error.clone())
}

fn start(app: &mut SolveApp, ctx: &egui::Context, r: &Request) {
    let ask = ask_of(r);
    if !app.policy.allows("problem_reports") {
        status(&ask.id, "failed", "The organization's policy turns problem reports off on this computer: nothing was sent.", json!({}));
        return;
    }
    let title: String = r.params.get("title").and_then(Value::as_str).unwrap_or_default().trim().chars().take(payload::MAX_TITLE).collect();
    let description: String =
        r.params.get("description").and_then(Value::as_str).unwrap_or_default().chars().take(payload::MAX_DESCRIPTION).collect();
    let dialog = app.dialog.clone();
    let error = failure(app);
    if ask.ticks.design || ask.ticks.crash {
        // The person decides: the report dialog opens filled in, the design and crash boxes
        // unticked, for them to tick and send.
        crate::issue_report::begin(app, ctx, dialog.as_ref(), error.as_deref());
        if let Some(d) = app.issue_report.draft.as_mut() {
            d.title.clone_from(&title);
            d.description.clone_from(&description);
            d.agent = Some(ask.clone());
        }
        app.tickets.agent = Some(ask.id.clone());
        app.tickets.outcome = None;
        status(
            &ask.id,
            "awaiting_user",
            "The report dialog is open in SolidCraft. Only the person can include their model or crash report: ask them to tick it there and press Send report.",
            json!({}),
        );
        return;
    }
    let draft = crate::issue_report::draft_for(app, dialog.as_ref(), error.as_deref());
    let mut ticket = crate::issue_report::ticket::new_ticket(app, &draft, Some(ctx));
    ticket.form.title = if title.is_empty() { ticket.form.title } else { title };
    if !description.is_empty() {
        ticket.form.description = if ticket.from_failure { format!("{description}\n\n{}", ticket.form.description) } else { description };
    }
    ticket.form.kind = ask.kind;
    ticket.form.app.clone_from(&ask.app);
    let ticks = payload::Ticks { design: false, crash: false, ..ask.ticks };
    if ticks.screenshot {
        ticket.evidence.screenshot = crate::control::view_png(app, 1280, 720).ok();
    }
    let report = payload::build_via(
        &ticket.form,
        ticks,
        &ticket.evidence,
        &format!("solidcraft {}", crate::help::VERSION),
        &payload::Via::Agent { confirmed_by: None },
    );
    app.tickets.agent = Some(ask.id.clone());
    super::send(app, report);
    status(&ask.id, "sending", "Sending the report.", json!({}));
}

/// The current agent report's status, from how sending goes.
fn report_progress(app: &mut SolveApp) {
    let Some(id) = app.tickets.agent.clone() else { return };
    if app.tickets.sending.is_some() {
        status(&id, "sending", "Sending the report.", json!({}));
        return;
    }
    match app.tickets.outcome.clone() {
        Some(Outcome::Opened(url)) => {
            status(&id, "opened", "The report is open in the person's browser: they sign in there and click File ticket.", json!({ "url": url }));
            app.tickets.agent = None;
        }
        Some(Outcome::AlreadySent(url)) => {
            status(
                &id,
                "opened",
                "The same report was sent in the last 30 minutes, so nothing new was sent: its page is offered to the person again.",
                json!({ "url": url, "already_sent": true }),
            );
            app.tickets.agent = None;
        }
        Some(Outcome::Queued(why)) => {
            status(
                &id,
                "queued",
                &format!("Could not reach the ticket server ({why}); the report is kept in SolidCraft until the person sends it again."),
                json!({}),
            );
            app.tickets.agent = None;
        }
        Some(Outcome::Failed(e)) => {
            status(&id, "failed", &e, json!({}));
            app.tickets.agent = None;
        }
        Some(Outcome::Cancelled) => {
            status(&id, "failed", "The person cancelled the report.", json!({}));
            app.tickets.agent = None;
        }
        // The dialog an agent opened was closed without sending.
        None if app.issue_report.draft.is_none()
            && mailbox::status(&id).and_then(|s| s.get("status").and_then(Value::as_str).map(str::to_string)).as_deref() == Some("awaiting_user") =>
        {
            status(&id, "failed", "The person closed the report without sending it.", json!({}));
            app.tickets.agent = None;
        }
        None => {}
    }
}
