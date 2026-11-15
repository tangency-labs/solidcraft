//! Help › My tickets: the user's own tickets with their status and the latest reply, and a dot on
//! a ticket (and on the Help menu's item) when something changed since it was last looked at. The
//! list is fetched when the window opens or Refresh is pressed, and otherwise at most every 15
//! minutes while signed in (the first check a minute after start), for the dot; never when
//! signed out.

use std::collections::BTreeMap;

use serde_json::Value;

use super::{Call, SignIn, api};
use crate::{SolveApp, ui_kit};
use solidcraft_ui_text::{format, tr};

/// The least time between two automatic checks, in ms.
pub const POLL_MS: f64 = 15.0 * 60_000.0;
/// The first automatic check comes this long after the app starts, in ms.
const FIRST_MS: f64 = 60_000.0;

/// The latest reply on a ticket.
#[derive(Clone, Debug, PartialEq)]
pub struct Reply {
    pub author: String,
    pub excerpt: String,
    pub at: String,
    /// From a triager or admin (not the reporter).
    pub staff: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MyTicket {
    pub number: u64,
    pub title: String,
    pub status: String,
    pub app: String,
    pub updated: String,
    /// The server has unread notices for it.
    pub unread: bool,
    pub reply: Option<Reply>,
}

#[derive(Default)]
pub struct Mine {
    pub open: bool,
    pub list: Vec<MyTicket>,
    call: Option<Call>,
    pub error: Option<String>,
    /// When the list was last asked for (ms); 0: never.
    pub asked_at: f64,
    /// The first tick (ms): when the app started, for the first automatic check.
    started: f64,
}

fn text(v: &Value, key: &str, n: usize) -> String {
    v.get(key).and_then(Value::as_str).unwrap_or_default().chars().take(n).collect()
}

/// The tickets in a list answer.
pub fn parse(v: &Value) -> Vec<MyTicket> {
    v.get("tickets")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|t| {
            Some(MyTicket {
                number: t.get("id")?.as_u64()?,
                title: text(t, "title", 200),
                status: text(t, "status", 30),
                app: t.get("app").map(|a| text(a, "name", 60)).unwrap_or_default(),
                updated: text(t, "updatedAt", 40),
                unread: t.get("unread").and_then(Value::as_u64).unwrap_or(0) > 0,
                reply: t.get("lastComment").filter(|c| c.is_object()).map(|c| Reply {
                    author: text(c, "author", 80),
                    excerpt: text(c, "excerpt", 300),
                    at: text(c, "at", 40),
                    staff: c.get("fromMaintainer").and_then(Value::as_bool).unwrap_or(false),
                }),
            })
        })
        .take(200)
        .collect()
}

/// Whether `t` changed since the user last saw the list (`seen`: number → its `updated` then).
pub fn is_new(t: &MyTicket, seen: &BTreeMap<u64, String>) -> bool {
    t.unread || seen.get(&t.number).is_some_and(|s| *s != t.updated)
}

/// Whether any ticket has something new: the Help menu's dot.
pub fn any_new(app: &SolveApp) -> bool {
    app.tickets.mine.list.iter().any(|t| is_new(t, &app.preferences.tickets_seen))
}

/// Ask for the list now (opening the window, Refresh). Signed in only.
pub fn refresh(app: &mut SolveApp) {
    ask(app, crate::now_ms());
}

fn ask(app: &mut SolveApp, now: f64) {
    if app.tickets.mine.call.is_some() {
        return;
    }
    let (Some(http), Some(token)) = (app.services.tickets_http.clone(), super::token(app)) else { return };
    app.tickets.mine.asked_at = now;
    app.tickets.mine.call = Some(Call::start(&http, api::mine(&token)));
}

/// Take up an answer; check again when 15 minutes have passed, signed in only.
pub fn tick(app: &mut SolveApp, now: f64) {
    if let Some(reply) = app.tickets.mine.call.as_ref().and_then(Call::poll) {
        app.tickets.mine.call = None;
        match api::answer(reply) {
            Ok(v) => {
                app.tickets.mine.list = parse(&v);
                app.tickets.mine.error = None;
                // A ticket seen for the first time is not new: it is the user's own report.
                for t in &app.tickets.mine.list {
                    app.preferences.tickets_seen.entry(t.number).or_insert_with(|| t.updated.clone());
                }
            }
            Err(api::Failure::SignedOut) => super::sign_out(app),
            Err(e) => app.tickets.mine.error = Some(e.text()),
        }
    }
    if app.tickets.mine.started == 0.0 {
        app.tickets.mine.started = now;
    }
    // Checks by itself only when signed in: the first a minute after start, then every 15
    // minutes. The keyring is read once for it; signed out, nothing is sent.
    let due = if app.tickets.mine.asked_at > 0.0 { now - app.tickets.mine.asked_at >= POLL_MS } else { now - app.tickets.mine.started >= FIRST_MS };
    if due && app.services.tickets_http.is_some() {
        if super::token(app).is_some() {
            ask(app, now);
        } else {
            // Not signed in: look again only after the user signs in (which asks at once).
            app.tickets.mine.asked_at = now;
        }
    }
}

/// The list as it is now has been seen.
pub fn mark_seen(app: &mut SolveApp) {
    let seen: Vec<(u64, String)> = app.tickets.mine.list.iter().map(|t| (t.number, t.updated.clone())).collect();
    for (n, updated) in seen {
        app.preferences.tickets_seen.insert(n, updated);
    }
    // Only tickets still listed are remembered.
    let listed: std::collections::BTreeSet<u64> = app.tickets.mine.list.iter().map(|t| t.number).collect();
    if !listed.is_empty() {
        app.preferences.tickets_seen.retain(|n, _| listed.contains(n));
    }
}

/// Open the window (Help › My tickets): the list is fetched as it opens.
pub fn open(app: &mut SolveApp) {
    app.tickets.mine.open = true;
    refresh(app);
}

pub fn busy(app: &SolveApp) -> bool {
    app.tickets.mine.call.is_some()
}

fn status_label(status: &str) -> String {
    let id = format!("tickets.status.{}", status.replace('-', "_"));
    let text = tr(&id);
    if text == id { status.to_string() } else { text.to_string() }
}

/// The My tickets window.
pub fn show(app: &mut SolveApp, ctx: &egui::Context) {
    if !app.tickets.mine.open {
        return;
    }
    let signed_in = super::signed_in(app);
    let busy = busy(app);
    let mut refresh_now = false;
    let mut sign_in = false;
    let (closed, action) = ui_kit::Window::new(egui::Id::unique("sc_my_tickets"), "list", tr("tickets.mine_title"))
        .cards(2.0)
        .actions("", tr("tickets.close"), None)
        .show(ctx, true, |ui| {
            if !signed_in {
                match &app.tickets.sign_in {
                    SignIn::Waiting { code, .. } => {
                        ui_kit::text(ui, tr("tickets.sign_in_code"), ui_kit::Text::Note("tickets.sign_in_code"));
                        let r = ui_kit::text(ui, &code.user_code, ui_kit::Text::Heading("tickets.user_code"));
                        crate::scenario::publish_target("tickets:user_code", r.rect);
                    }
                    SignIn::Asking(_) => {
                        ui_kit::text(ui, tr("report.signing_in"), ui_kit::Text::Note("tickets.signing_in"));
                    }
                    SignIn::Failed(e) => {
                        ui_kit::text(ui, &format("report.failed", &[("error", e)]), ui_kit::Text::Error("tickets.sign_in_failed"));
                        sign_in = ui_kit::button(ui, tr("tickets.sign_in"), ui_kit::Button::Normal).clicked();
                    }
                    SignIn::Idle => {
                        ui_kit::text(ui, tr("tickets.mine_signed_out"), ui_kit::Text::Note("tickets.signed_out"));
                        let r = ui_kit::button(ui, tr("tickets.sign_in"), ui_kit::Button::Normal);
                        crate::scenario::publish_target("tickets:sign_in", r.rect);
                        sign_in = r.clicked();
                    }
                }
                return;
            }
            ui.horizontal(|ui| {
                let r = ui_kit::button(ui, tr("tickets.refresh"), ui_kit::Button::Enabled(!busy));
                crate::scenario::publish_target("tickets:refresh", r.rect);
                refresh_now = r.clicked();
                if busy {
                    ui_kit::text(ui, tr("tickets.loading"), ui_kit::Text::Note("tickets.loading"));
                }
            });
            if let Some(e) = &app.tickets.mine.error {
                ui_kit::text(ui, &format("tickets.failed", &[("error", e)]), ui_kit::Text::Error("tickets.failed"));
            }
            if app.tickets.mine.list.is_empty() && !busy && app.tickets.mine.error.is_none() {
                ui_kit::text(ui, tr("tickets.mine_empty"), ui_kit::Text::Note("tickets.empty"));
            }
            let seen = app.preferences.tickets_seen.clone();
            egui::ScrollArea::vertical().id_salt("my-tickets").max_height(420.0).show(ui, |ui| {
                for t in &app.tickets.mine.list {
                    ui_kit::divider(ui, "my-ticket");
                    ui.horizontal_wrapped(|ui| {
                        let dot = if is_new(t, &seen) { "● " } else { "" };
                        ui_kit::text(ui, &format!("{dot}#{} {}", t.number, t.title), ui_kit::Text::Heading("tickets.row"));
                    });
                    ui.horizontal_wrapped(|ui| {
                        ui_kit::text(ui, &status_label(&t.status), ui_kit::Text::Note("tickets.status"));
                        let r = ui_kit::button(ui, tr("tickets.open"), ui_kit::Button::Normal);
                        crate::scenario::publish_target(&format!("tickets:open:{}", t.number), r.rect);
                        if r.clicked() {
                            ui.ctx().open_url(egui::OpenUrl::new_tab(api::ticket_url(t.number)));
                        }
                    });
                    match &t.reply {
                        Some(r) => {
                            let text = format("tickets.reply_from", &[("author", &r.author), ("excerpt", &r.excerpt)]);
                            ui_kit::text(ui, &text, ui_kit::Text::Note("tickets.reply"));
                        }
                        None => {
                            ui_kit::text(ui, tr("tickets.no_reply"), ui_kit::Text::Note("tickets.no_reply"));
                        }
                    }
                }
            });
        });
    if sign_in {
        super::begin_sign_in(app);
    }
    if refresh_now {
        refresh(app);
    }
    if closed || action == ui_kit::Action::Cancel {
        // What was shown has been seen: the dots go.
        mark_seen(app);
        app.tickets.mine.open = false;
    }
}
