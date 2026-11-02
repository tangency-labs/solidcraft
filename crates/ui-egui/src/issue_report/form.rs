//! The reviewed report form. User answers stay separate from collected diagnostics.

use egui::{Id, Ui, vec2};

use super::{Draft, MAX_ANSWER, MAX_BODY, draft_url};
use crate::{SolveApp, i18n::tr_label, ui_kit};

const ALL_GUIDANCE: [&str; 4] = [
    "Report text copied. Paste it into GitHub and submit your report there.",
    "Review your report on GitHub and click Submit new issue to publish it.",
    "Opens your browser and copies the report text for you to paste into GitHub.",
    "Opens your browser and copies the report text. Review and submit your report on GitHub.",
];

fn answer(ui: &mut Ui, label: &str, hint: &str, text: &mut String, id: &str, rows: usize) -> egui::Response {
    ui_kit::text(ui, tr_label(label), ui_kit::Text::Heading(label));
    ui_kit::text(ui, tr_label(hint), ui_kit::Text::Note(hint));
    let response = ui_kit::text_area(ui, tr_label(label), Id::new(id), text, rows, MAX_ANSWER);
    crate::scenario::publish_handle(id, response.rect.center());
    super::truncate(text, MAX_ANSWER);
    response
}

fn screenshot_actions(app: &mut SolveApp, draft: &mut Draft, ui: &mut Ui) -> bool {
    let mut retake = false;
    ui.horizontal_wrapped(|ui| {
        retake = ui_kit::button(ui, tr_label("Retake screenshot"), ui_kit::Button::Normal).clicked();
        if ui_kit::button(ui, tr_label("Remove screenshot"), ui_kit::Button::Enabled(draft.image.is_some())).clicked() {
            draft.image = None;
            draft.texture = None;
            draft.note = "Screenshot removed from this draft.".into();
        }
    });
    if draft.image.is_some() {
        ui.horizontal_wrapped(|ui| {
            if ui_kit::button(ui, tr_label("Copy screenshot"), ui_kit::Button::Normal).clicked()
                && let Some(image) = draft.image.as_ref()
            {
                ui.ctx().copy_image((**image).clone());
                draft.opened_github = false;
                draft.note = "Paste the image into the GitHub draft, or save the PNG if image copying is unavailable.".into();
            }
            if ui_kit::button(ui, tr_label("Save screenshot…"), ui_kit::Button::Enabled(app.services.pick_save.is_some())).clicked()
                && let Some(image) = draft.image.clone()
            {
                app.pick_save(&format!("solidcraft-issue-{}.png", app.shot_token), &["png"], move |app, path| {
                    let Some(path) = path else { return };
                    let result = crate::control::write_screenshot(&image, &path);
                    if let Some(draft) = app.issue_report.draft.as_mut() {
                        draft.note = match result {
                            Ok(()) => "Screenshot saved. Attach the PNG in the GitHub draft.".into(),
                            Err(e) => format!("Could not save screenshot: {}", super::limited(&e, 1000)),
                        };
                    }
                });
            }
        });
    }
    if !draft.note.is_empty() {
        ui_kit::text(ui, tr_label(&draft.note), ui_kit::Text::Note("note"));
    }
    retake
}

fn screenshot(app: &mut SolveApp, draft: &mut Draft, ui: &mut Ui) -> bool {
    ui_kit::text(ui, tr_label("Screenshot (optional)"), ui_kit::Text::Heading("Screenshot (optional)"));
    let preview = draft.image.as_ref().map(|image| {
        draft.texture.get_or_insert_with(|| ui.ctx().load_texture("issue-report-frame", image.clone(), egui::TextureOptions::LINEAR)).clone()
    });
    let mut retake = false;
    let guidance = |ui: &mut Ui| {
        ui_kit::text(
            ui,
            tr_label("This image is not attached to GitHub yet. Copy or save it, then attach it on GitHub."),
            ui_kit::Text::Note("screenshot"),
        );
    };
    if let Some(texture) = preview {
        ui.add(egui::Image::new(&texture).max_size(vec2(ui.available_width(), 130.0)).maintain_aspect_ratio(true));
        guidance(ui);
    }
    retake |= screenshot_actions(app, draft, ui);
    retake
}

/// The design saved as one `.json` file to attach to the report (#6878): maintainers otherwise ask
/// for it in a comment. A copy: the open design keeps its own file.
fn design(app: &mut SolveApp, draft: &mut Draft, ui: &mut Ui) {
    ui_kit::text(ui, tr_label("Design (optional)"), ui_kit::Text::Heading("Design (optional)"));
    ui_kit::text(
        ui,
        tr_label("Reports are public: attach the design only if it can be shared. Drag the saved file into the GitHub draft."),
        ui_kit::Text::Note("design"),
    );
    if ui_kit::button(ui, tr_label("Save design…"), ui_kit::Button::Enabled(app.services.pick_save.is_some())).clicked() {
        draft.design_note.clear();
        app.pick_save(&format!("{}.json", design_file_stem(&app.session.doc.name)), &["json"], |app, path| {
            let Some(path) = path else { return };
            let result = app.run("file.export", serde_json::json!({"path": path, "format": "json"}));
            if let Some(draft) = app.issue_report.draft.as_mut() {
                draft.design_note = match result {
                    Ok(_) => "Design saved. Drag the .json file into the GitHub draft to attach it.".into(),
                    Err(e) => format!("Could not save the design: {e}"),
                };
            }
        });
    }
    if !draft.design_note.is_empty() {
        ui_kit::text(ui, tr_label(&draft.design_note), ui_kit::Text::Note("design_note"));
    }
}

/// A file name for the design: its name with characters file systems refuse replaced.
pub(super) fn design_file_stem(name: &str) -> String {
    let stem: String = name.trim().chars().map(|c| if c.is_control() || r#"/\:*?"<>|"#.contains(c) { '_' } else { c }).take(80).collect();
    let stem = stem.trim_matches(['.', ' ']);
    if stem.is_empty() { "solidcraft-design".into() } else { stem.into() }
}

/// Returns whether to retake the screenshot and whether the user closed the form.
pub(super) fn show(app: &mut SolveApp, draft: &mut Draft, ctx: &egui::Context) -> (bool, bool) {
    let mut retake = false;
    let t = crate::theme::Tokens::get();
    let screen = ctx.content_rect();
    let cards = ((screen.width() - 2.0 * t.space_16) / t.form_card_width).clamp(0.5, 2.6);
    // Ctrl+Enter (⌘Enter on a Mac) in any field continues on GitHub. Read before the fields see
    // it, so it adds no line break. Only the press counts: the repeats of a held key are dropped,
    // so holding it opens GitHub once.
    let submit = ctx.input_mut(|i| {
        let mut pressed_now = false;
        i.events.retain(|e| match e {
            egui::Event::Key { key: egui::Key::Enter, pressed, repeat, modifiers, .. } if modifiers.command => {
                pressed_now |= *pressed && !*repeat;
                false
            }
            _ => true,
        });
        pressed_now
    });
    // The shared footer is a single row. Keep both export actions reachable at high zoom;
    // the header's close button remains available when the redundant closing action is omitted.
    let actions_width = ctx.fonts_mut(|fonts| {
        [
            ("Open GitHub issue form", crate::fonts::bold(t.font_label), t.space_16),
            ("Copy report", egui::FontId::proportional(t.font_label), t.space_12),
            ("Close draft", egui::FontId::proportional(t.font_label), t.space_12),
        ]
        .into_iter()
        .map(|(label, font, padding)| fonts.layout_no_wrap(tr_label(label).to_string(), font, t.form_ink).size().x + 2.0 * padding)
        .sum::<f32>()
    });
    let compact = cards * t.form_card_width < actions_width + 2.0 * t.space_12 + 2.0 * t.space_6 + t.space_8;
    let open_label = if compact { "Open GitHub" } else { "Open GitHub issue form" };
    let close_label = if compact { "" } else { "Close draft" };
    let (closed, mut action) = ui_kit::Window::new(Id::new("sc_issue_report"), "bug_report", tr_label("Report a bug"))
        .cards(cards)
        // Opens at the top middle, and its title band drags it off what the report is about
        // (#9916).
        .placed(egui::Align2::CENTER_TOP, egui::pos2(screen.center().x, screen.top() + t.space_16))
        .fields()
        .keep_open_on_escape()
        .actions(tr_label(open_label), tr_label(close_label), Some(tr_label("Copy report")))
        .show(ctx, true, |ui| {
            // The card's own header, footer and margins come on top of the scrolling body.
            let chrome = ui_kit::header_height() + t.form_footer_height + 6.0 * t.space_8;
            let scroll = (screen.height() - 2.0 * t.space_16 - chrome).clamp(60.0, 640.0);
            // Sized outright, not from what is left: a card that fits its content grows frame by
            // frame otherwise, and the footer moves under a click.
            ui.allocate_ui(vec2(ui.available_width(), scroll), |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("issue-report-review")
                    .max_height(scroll)
                    .min_scrolled_height(scroll)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        let body = draft.report_body();
                        let (_, copy_needed) = draft_url(&draft.report_title(), &body);
                        let guidance = if draft.opened_github {
                            if copy_needed {
                                "Report text copied. Paste it into GitHub and submit your report there."
                            } else {
                                "Review your report on GitHub and click Submit new issue to publish it."
                            }
                        } else if copy_needed {
                            "Opens your browser and copies the report text for you to paste into GitHub."
                        } else {
                            "Opens your browser and copies the report text. Review and submit your report on GitHub."
                        };
                        // Reserve the tallest wording, so the card (and its footer) keeps its height as the
                        // guidance changes under a click.
                        let width = ui.available_width();
                        let font = egui::FontId::proportional(t.font_small);
                        let tallest = ALL_GUIDANCE
                            .iter()
                            .map(|g| ui.painter().layout(tr_label(g).to_string(), font.clone(), t.form_dim, width).size().y)
                            .fold(0.0_f32, f32::max);
                        ui.allocate_ui(vec2(width, tallest), |ui| {
                            ui_kit::text(ui, tr_label(guidance), ui_kit::Text::Note("guidance"));
                        });
                        ui_kit::text(ui, tr_label("Reports are public. A GitHub account is required."), ui_kit::Text::Note("public"));
                        ui_kit::text(
                            ui,
                            tr_label("SolidCraft cannot submit reports directly. Submit on GitHub; no token is needed in the app."),
                            ui_kit::Text::Note("submission"),
                        );
                        ui_kit::text(
                            ui,
                            tr_label("If you are offline or GitHub does not open, use Copy report and keep the text to submit later."),
                            ui_kit::Text::Note("offline"),
                        );
                        ui_kit::divider(ui, "report");
                        ui_kit::text(ui, tr_label("Summary"), ui_kit::Text::Heading("Summary"));
                        let response = ui_kit::bounded_cell_editor(
                            ui,
                            Id::new("issue_report_title"),
                            &mut draft.title,
                            "A short title for the problem (optional)",
                            false,
                            160,
                        );
                        if response.changed() {
                            draft.opened_github = false;
                        }
                        super::truncate(&mut draft.title, 160);
                        let response = answer(ui, "What happened?", "Describe the problem you saw", &mut draft.description, "issue_report_body", 3);
                        if response.changed() {
                            draft.opened_github = false;
                        }
                        crate::scenario::publish_rect("report:description", response.rect);
                        if !draft.focus_requested {
                            response.scroll_to_me(Some(egui::Align::Center));
                            response.request_focus();
                            draft.focus_requested = true;
                        }
                        if draft.missing_description {
                            if draft.description.trim().is_empty() {
                                ui_kit::text(ui, tr_label("Describe what happened before continuing."), ui_kit::Text::Error("description"));
                            } else {
                                draft.missing_description = false;
                            }
                        }
                        ui_kit::text(ui, tr_label(shortcut_hint(ui.ctx().os())), ui_kit::Text::Note("shortcut"));
                        let steps = answer(
                            ui,
                            "Steps to reproduce (optional)",
                            "What did you do just before the problem? A short list is enough",
                            &mut draft.steps,
                            "issue_report_steps",
                            2,
                        );
                        let expected =
                            answer(ui, "Expected result (optional)", "What did you expect to see?", &mut draft.expected, "issue_report_expected", 2);
                        if steps.changed() || expected.changed() {
                            draft.opened_github = false;
                        }
                        retake = screenshot(app, draft, ui);
                        design(app, draft, ui);
                        ui_kit::text(
                            ui,
                            tr_label("App/system information and command settings are included. Review private details before sharing."),
                            ui_kit::Text::Note("private"),
                        );
                        ui_kit::section(ui, "issue-report-technical", "Technical details", false, |ui| {
                            let response =
                                ui_kit::text_area(ui, tr_label("Technical details"), Id::new("issue_report_technical"), &mut draft.body, 8, MAX_BODY);
                            if response.changed() {
                                draft.opened_github = false;
                            }
                            super::truncate(&mut draft.body, MAX_BODY);
                        });
                    });
            });
        });
    if submit && action == ui_kit::Action::None {
        action = ui_kit::Action::Ok;
    }
    // TextEdit applies this frame's events while it is drawn. Export only after every editor
    // has processed its input, including diagnostic redactions.
    match action {
        ui_kit::Action::Ok => {
            draft.missing_description = draft.description.trim().is_empty();
            if !draft.missing_description {
                let body = draft.report_body();
                let (url, _) = draft_url(&draft.report_title(), &body);
                // Keep a local fallback even for short reports: opening the browser may fail,
                // be blocked by the web host, or lead to an offline page. No delivery is confirmed.
                ctx.copy_text(body);
                ctx.open_url(egui::OpenUrl::new_tab(url));
                draft.opened_github = true;
                ctx.request_repaint();
            }
        }
        ui_kit::Action::Apply => {
            ctx.copy_text(draft.report_body());
            draft.note = "Report copied. Keep this text to submit on GitHub when you are online.".into();
        }
        _ => {}
    }
    // Found empty, the description takes the focus once more, not every frame: the other
    // fields stay free to type in (#7190).
    if action == ui_kit::Action::Ok && draft.missing_description {
        draft.focus_requested = false;
    }
    (retake, closed || action == ui_kit::Action::Cancel)
}

/// The keys that continue on GitHub from any field (#7203), as this computer spells them.
pub(super) fn shortcut_hint(os: egui::os::OperatingSystem) -> &'static str {
    if os == egui::os::OperatingSystem::Mac { "⌘Enter continues on GitHub." } else { "Ctrl+Enter continues on GitHub." }
}
