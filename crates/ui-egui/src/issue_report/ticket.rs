//! Report a problem: the in-app report that files a private ticket on tickets.solidcraft.io.
//! Each attachment is a box with what it holds said in plain words and shown before it goes;
//! the design is never sent unless its box is ticked.

use std::sync::Arc;

use egui::{ColorImage, Id, Ui, vec2};
use solidcraft_ui_text::{format, tr};

use super::Draft;
use crate::tickets::{self, Outcome, SignIn, api, payload};
use crate::{SolveApp, ui_kit};

/// How many lines of the log go with a report.
pub const LOG_LINES: usize = 300;

/// What the report form holds besides the shared draft.
pub struct TicketDraft {
    pub form: payload::Form,
    pub ticks: payload::Ticks,
    pub evidence: payload::Evidence,
    /// The title was found empty on Send.
    missing_title: bool,
}

/// Whether Report a bug files a ticket here (rather than opening a GitHub draft).
pub fn enabled(app: &SolveApp) -> bool {
    !app.preferences.report_on_github && app.services.tickets_http.is_some()
}

/// The text "System info" sends, exactly as shown: nothing about the design.
pub fn system_text(app: &SolveApp, ctx: Option<&egui::Context>) -> String {
    use crate::help::{BUILD_DATE, COMMIT, VERSION};
    let mut s = format!("SolidCraft {VERSION} ({COMMIT}, built {BUILD_DATE})\nOS: {} {}\n", std::env::consts::OS, std::env::consts::ARCH);
    let renderer = if app.viewport.gpu.is_some() { "GPU" } else { "CPU" };
    s.push_str(&format!("Renderer: {renderer}{}\n", app.help.gpu.as_ref().map(|g| format!(", {g}")).unwrap_or_default()));
    if let Some(api) = app.services.graphics_backend {
        s.push_str(&format!("Graphics API: {api}\n"));
    }
    for (name, label) in app.services.graphics_adapters.iter().take(4) {
        s.push_str(&format!("Adapter: {name} ({label})\n"));
    }
    if let Some(ws) = app.services.window_system {
        s.push_str(&format!("Window system: {ws}\n"));
    }
    if let Some(ctx) = ctx {
        let r = ctx.content_rect();
        s.push_str(&format!("Window: {:.0}×{:.0} points at {:.2} pixels per point\n", r.width(), r.height(), ctx.pixels_per_point()));
    }
    s
}

/// The last lines of the log (the design's command log, then the Messages), home folder as `~`.
pub fn log_lines(app: &SolveApp) -> Vec<String> {
    let home = super::home_text();
    let clean = |s: &str| {
        let s = super::limited(s, 2000);
        match home.as_deref() {
            Some(h) => s.replace(h, "~"),
            None => s,
        }
    };
    let mut lines: Vec<String> = app.session.log.iter().flat_map(|l| l.lines().map(clean).collect::<Vec<_>>()).collect();
    for m in app.messages.iter() {
        let level = if m.error { "error" } else { "info" };
        lines.push(format!("[{level}] {}", clean(&m.text)));
    }
    let n = lines.len();
    lines.drain(..n.saturating_sub(LOG_LINES));
    lines
}

/// Everything the boxes can attach, collected when the report opens.
pub fn evidence(
    app: &SolveApp,
    ctx: Option<&egui::Context>,
    error: Option<(Option<String>, String)>,
    crash: Option<(String, String)>,
) -> payload::Evidence {
    let steps = solidcraft_engine::recent_commands::of(app.session.uid)
        .into_iter()
        .map(|r| payload::Step { command: r.command, params: r.params, ok: r.ok, result: r.result })
        .collect::<Vec<_>>();
    // The failing command: the one the report was made for, else a feature that failed.
    let failing = error.or_else(|| {
        app.session.doc.features.iter().find_map(|f| {
            let e = app.session.model.result(f.id)?.error.as_deref()?;
            Some((Some(format!("{} ({})", f.name, f.kind.type_name())), super::limited(e, 2000)))
        })
    });
    let name = app
        .session
        .path
        .as_deref()
        .and_then(|p| std::path::Path::new(p).file_name().map(|n| n.to_string_lossy().into_owned()))
        .filter(|n| n.ends_with(".sc"))
        .unwrap_or_else(|| {
            let n: String =
                app.session.doc.name.chars().filter(|c| !matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|')).take(80).collect();
            format!("{}.sc", if n.trim().is_empty() { "design" } else { n.trim() })
        });
    payload::Evidence {
        system: system_text(app, ctx),
        app_version: crate::help::VERSION.to_string(),
        os: format!("{} {}", std::env::consts::OS, std::env::consts::ARCH),
        steps,
        failing,
        log: log_lines(app),
        screenshot: None,
        design: Some((name, solidcraft_engine::io::write_design(&app.session.doc))),
        crash,
    }
}

/// The 3D view's part of a window screenshot (`view` in points), or the whole of it.
pub fn crop(image: &Arc<ColorImage>, view: Option<egui::Rect>, pixels_per_point: f32) -> Arc<ColorImage> {
    let Some(view) = view.filter(|r| r.is_positive()) else { return image.clone() };
    let [w, h] = image.size;
    let px = |v: f32, max: usize| ((v * pixels_per_point).round().max(0.0) as usize).min(max);
    let (x0, y0, x1, y1) = (px(view.min.x, w), px(view.min.y, h), px(view.max.x, w), px(view.max.y, h));
    if x1 <= x0 + 8 || y1 <= y0 + 8 {
        return image.clone();
    }
    let mut pixels = Vec::with_capacity((x1 - x0) * (y1 - y0));
    for y in y0..y1 {
        pixels.extend_from_slice(image.pixels.get(y * w + x0..y * w + x1).unwrap_or_default());
    }
    if pixels.len() != (x1 - x0) * (y1 - y0) {
        return image.clone();
    }
    Arc::new(ColorImage::new([x1 - x0, y1 - y0], pixels))
}

fn png(image: &ColorImage) -> Option<Vec<u8>> {
    solidcraft_control_egui::screenshot::encode(image, "view.png").ok()
}

/// The report to send: the form and the ticked boxes only.
pub fn outgoing(draft: &Draft, ticket: &TicketDraft) -> payload::Outgoing {
    let mut evidence = ticket.evidence.clone();
    if ticket.ticks.screenshot {
        evidence.screenshot = draft.view.as_ref().or(draft.image.as_ref()).and_then(|i| png(i));
    }
    payload::build(&ticket.form, ticket.ticks, &evidence, &format!("solidcraft {}", crate::help::VERSION))
}

fn heading(ui: &mut Ui, id: &str) {
    ui_kit::text(ui, tr(id), ui_kit::Text::Heading(id));
}

fn note(ui: &mut Ui, id: &str, text: &str) {
    ui_kit::text(ui, text, ui_kit::Text::Note(id));
}

fn check(ui: &mut Ui, id: &str, value: &mut bool) {
    let r = ui_kit::checkbox(ui, tr(id), value);
    crate::scenario::publish_target(&format!("report:{id}"), r.rect);
}

fn shown(ui: &mut Ui, id: &str, title: &str, text: &str) {
    ui_kit::section(ui, id, title, false, |ui| {
        egui::ScrollArea::vertical().id_salt(id).max_height(160.0).show(ui, |ui| {
            ui_kit::text(ui, text, ui_kit::Text::MonoNote(id));
        });
    });
}

/// The attachment boxes, each with what it holds.
fn attachments(app: &SolveApp, draft: &mut Draft, ticket: &mut TicketDraft, ui: &mut Ui) -> bool {
    let mut retake = false;
    heading(ui, "report.attach");
    let ev = &ticket.evidence;
    check(ui, "report.system", &mut ticket.ticks.system);
    note(ui, "report.system_note", tr("report.system_note"));
    shown(ui, "report-system", tr("report.system_show"), &ev.system);

    check(ui, "report.repro", &mut ticket.ticks.repro);
    note(ui, "report.repro_note", &format("report.repro_note", &[("count", &ev.steps.len().to_string())]));
    shown(ui, "report-repro", tr("report.repro_show"), &payload::steps_text(&ev.steps, ev.failing.as_ref()));

    check(ui, "report.log", &mut ticket.ticks.log);
    if ev.log.is_empty() {
        note(ui, "report.log_note", tr("report.log_empty"));
    } else {
        note(ui, "report.log_note", &format("report.log_note", &[("count", &ev.log.len().to_string())]));
        shown(ui, "report-log", tr("report.log_show"), &ev.log.join("\n"));
    }

    check(ui, "report.screenshot", &mut ticket.ticks.screenshot);
    let image = draft.view.clone().or_else(|| draft.image.clone());
    if let Some(image) = image {
        let texture = draft.texture.get_or_insert_with(|| ui.ctx().load_texture("ticket-report-view", image, egui::TextureOptions::LINEAR)).clone();
        ui.add(egui::Image::new(&texture).max_size(vec2(220.0, 124.0)).maintain_aspect_ratio(true));
    } else if !draft.note.is_empty() {
        note(ui, "report.screenshot_state", &draft.note);
    }
    let r = ui_kit::button(ui, tr("report.retake"), ui_kit::Button::Enabled(app.tickets.sending.is_none()));
    crate::scenario::publish_target("report:retake", r.rect);
    retake |= r.clicked();

    if let Some((name, bytes)) = &ev.design {
        check(ui, "report.design", &mut ticket.ticks.design);
        note(ui, "report.design_note", tr("report.design_note"));
        note(ui, "report.design_file", &format("report.design_file", &[("name", name), ("size", &payload::size_text(bytes.len()))]));
    }
    if ev.crash.is_some() {
        check(ui, "report.crash", &mut ticket.ticks.crash);
        note(ui, "report.crash_note", tr("report.crash_note"));
    }
    retake
}

/// Sign-in, progress and how the last report went.
fn status(app: &mut SolveApp, ui: &mut Ui) {
    match &app.tickets.sign_in {
        SignIn::Asking(_) => note(ui, "report.signing_in", tr("report.signing_in")),
        SignIn::Waiting { code, .. } => {
            note(ui, "report.sign_in_needed", tr("report.sign_in_needed"));
            let r = ui_kit::text(ui, &code.user_code, ui_kit::Text::Heading("report.user_code"));
            crate::scenario::publish_target("report:user_code", r.rect);
            note(ui, "report.sign_in_waiting", tr("report.sign_in_waiting"));
            let url = code.verification.clone();
            if ui_kit::button(ui, tr("report.sign_in_open"), ui_kit::Button::Normal).clicked() {
                ui.ctx().open_url(egui::OpenUrl::new_tab(url));
            }
        }
        SignIn::Failed(e) => {
            ui_kit::text(ui, &format("report.failed", &[("error", e)]), ui_kit::Text::Error("report.sign_in_failed"));
        }
        SignIn::Idle => {}
    }
    if let Some(s) = &app.tickets.sending {
        let (done, total) = (s.sent, s.total.max(1));
        note(ui, "report.sending", &format("report.sending", &[("done", &payload::size_text(done)), ("total", &payload::size_text(total))]));
        ui_kit::progress(ui, "report-progress", done as f32 / total as f32);
        let r = ui_kit::button(ui, tr("report.cancel_send"), ui_kit::Button::Normal);
        crate::scenario::publish_target("report:cancel_send", r.rect);
        if r.clicked() {
            tickets::cancel(app);
        }
    }
    match app.tickets.outcome.clone() {
        Some(Outcome::Filed(n)) => {
            ui.horizontal_wrapped(|ui| {
                ui_kit::text(ui, &format("report.filed", &[("number", &n.to_string())]), ui_kit::Text::Heading("report.filed"));
                let r = ui_kit::button(ui, tr("report.open_ticket"), ui_kit::Button::Normal);
                crate::scenario::publish_target("report:open_ticket", r.rect);
                if r.clicked() {
                    ui.ctx().open_url(egui::OpenUrl::new_tab(api::ticket_url(n)));
                }
            });
        }
        Some(Outcome::Queued) => note(ui, "report.queued", tr("report.queued")),
        Some(Outcome::Failed(e)) => {
            ui_kit::text(ui, &format("report.failed", &[("error", &e)]), ui_kit::Text::Error("report.failed"));
        }
        Some(Outcome::Cancelled(Some(n))) => note(ui, "report.cancelled", &format("report.cancelled_filed", &[("number", &n.to_string())])),
        Some(Outcome::Cancelled(None)) => note(ui, "report.cancelled", tr("report.cancelled")),
        None => {}
    }
    if app.tickets.token_unsaved {
        note(ui, "report.token_unsaved", tr("report.token_unsaved"));
    }
}

/// Returns whether to retake the screenshot and whether the user closed the form.
pub(super) fn show(app: &mut SolveApp, draft: &mut Draft, ctx: &egui::Context) -> (bool, bool) {
    let mut ticket = match draft.ticket.take() {
        Some(t) => t,
        None => {
            let error = match &draft.origin {
                super::Origin::View { error: Some(e) } => {
                    let command = draft.title_prefix.trim_end_matches(": ").to_string();
                    Some((Some(command).filter(|c| !c.is_empty()), e.clone()))
                }
                _ => None,
            };
            let mut evidence = evidence(app, Some(ctx), error, draft.crash.clone());
            if draft.crash.is_some() {
                // Made from a crash report: the failing command is the crash's.
                evidence.failing = Some((None, super::limited(&draft.description, 2000)));
            }
            let form = payload::Form {
                app: api::DEFAULT_APP.into(),
                title: draft.title.clone(),
                description: draft.description.clone(),
                ..Default::default()
            };
            tickets::load_apps(app);
            if app.tickets.sending.is_none() {
                app.tickets.outcome = None;
            }
            Box::new(TicketDraft {
                form,
                ticks: payload::Ticks { crash: draft.crash.is_some(), ..Default::default() },
                evidence,
                missing_title: false,
            })
        }
    };
    let mut retake = false;
    let t = solidcraft_theme::Tokens::get();
    let screen = ctx.content_rect();
    let cards = ((screen.width() - 2.0 * t.space_16) / t.form_card_width).clamp(0.5, 2.0);
    // Filed or queued: Send again would file it twice; the report is done.
    let busy = app.tickets.sending.is_some()
        || matches!(app.tickets.sign_in, SignIn::Asking(_) | SignIn::Waiting { .. })
        || matches!(app.tickets.outcome, Some(Outcome::Filed(_) | Outcome::Queued));
    let (closed, action) = ui_kit::Window::new(Id::unique("sc_issue_report"), "bug_report", tr("report.title"))
        .cards(cards)
        .placed(egui::Align2::CENTER_TOP, egui::pos2(screen.center().x, screen.top() + t.space_16))
        .fields()
        .keep_open_on_escape()
        .actions(tr("report.send"), tr("report.close"), None)
        .show(ctx, !busy, |ui| {
            let chrome = ui_kit::header_height() + t.form_footer_height + 6.0 * t.space_8;
            let scroll = (screen.height() - 2.0 * t.space_16 - chrome).clamp(60.0, 640.0);
            ui.allocate_ui(vec2(ui.available_width(), scroll), |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("ticket-report")
                    .max_height(scroll)
                    .min_scrolled_height(scroll)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        note(ui, "report.private_note", tr("report.private_note"));
                        let app_name = |slug: &str, apps: &[api::App]| {
                            apps.iter().find(|a| a.slug == slug).map_or_else(
                                || if slug == api::DEFAULT_APP { "SolidCraft".to_string() } else { slug.to_string() },
                                |a| a.name.clone(),
                            )
                        };
                        ui_kit::row(ui, tr("report.app"), |ui| {
                            let current = app_name(&ticket.form.app, &app.tickets.apps);
                            let apps = app.tickets.apps.clone();
                            let r = ui_kit::dropdown_menu(ui, "report-app", &current, |ui| {
                                for a in &apps {
                                    if ui_kit::list_picker(ui, &a.slug, &a.name, a.slug == ticket.form.app).clicked() {
                                        ticket.form.app = a.slug.clone();
                                    }
                                }
                            });
                            crate::scenario::publish_target("report:app", r.rect);
                        });
                        ui_kit::row(ui, tr("report.type"), |ui| {
                            let mut kind = payload::Kind::ALL.iter().position(|k| *k == ticket.form.kind).unwrap_or(0);
                            let labels = [tr("report.type_bug"), tr("report.type_feature"), tr("report.type_question")];
                            if ui_kit::segmented(
                                ui,
                                &mut kind,
                                &labels,
                                ui_kit::Segments {
                                    targets: &["report:type:bug", "report:type:feature", "report:type:question"],
                                    ..Default::default()
                                },
                            ) {
                                ticket.form.kind = payload::Kind::ALL.get(kind).copied().unwrap_or_default();
                            }
                        });
                        heading(ui, "report.summary");
                        let r = ui_kit::bounded_cell_editor(
                            ui,
                            Id::unique("ticket_report_title"),
                            &mut ticket.form.title,
                            tr("report.summary_hint"),
                            ticket.missing_title,
                            payload::MAX_TITLE,
                        );
                        crate::scenario::publish_target("report:title", r.rect);
                        if !draft.focus_requested {
                            r.request_focus();
                            draft.focus_requested = true;
                        }
                        if ticket.missing_title {
                            if ticket.form.title.trim().is_empty() {
                                ui_kit::text(ui, tr("report.title_needed"), ui_kit::Text::Error("report.title_needed"));
                            } else {
                                ticket.missing_title = false;
                            }
                        }
                        heading(ui, "report.description");
                        let r = ui_kit::text_area(
                            ui,
                            tr("report.description"),
                            Id::unique("ticket_report_body"),
                            &mut ticket.form.description,
                            4,
                            payload::MAX_DESCRIPTION,
                        );
                        crate::scenario::publish_target("report:description", r.rect);
                        note(ui, "report.description_hint", tr("report.description_hint"));
                        ui_kit::divider(ui, "report-attach");
                        retake = attachments(app, draft, &mut ticket, ui);
                        ui_kit::divider(ui, "report-status");
                        status(app, ui);
                    });
            });
        });
    if action == ui_kit::Action::Ok && !busy {
        ticket.missing_title = ticket.form.title.trim().is_empty();
        if !ticket.missing_title {
            let report = outgoing(draft, &ticket);
            tickets::send(app, report);
        }
    }
    draft.ticket = Some(ticket);
    let close = closed || action == ui_kit::Action::Cancel;
    if close {
        // Closing keeps a report that is being sent going; it is the user's to cancel.
        draft.ticket = None;
    }
    (retake, close)
}
