//! The reviewed report form. User answers stay separate from collected diagnostics.

use egui::{Id, Ui, vec2};

use super::{Draft, MAX_ANSWER, MAX_BODY, draft_url};
use crate::{SolveApp, ui_kit};
use solidcraft_ui_text::tr_label;

fn copy_screenshot(draft: &mut Draft, ctx: &egui::Context) {
    if let Some(image) = &draft.image {
        ctx.copy_image((**image).clone());
        draft.opened_github = false;
    }
}

fn paste_instruction(os: egui::os::OperatingSystem) -> &'static str {
    if os == egui::os::OperatingSystem::Mac { "ui.report_screenshot_paste_mac" } else { "ui.report_screenshot_paste" }
}

/// Keep reviewed diagnostics intact; change only the app's attachment prompt in the export.
fn with_visual_context(body: &str, instruction: &str) -> String {
    let prompt = "## Visual context\nAttach the reviewed screenshot or a short example showing the problem.";
    let section = format!("## Visual context\n{instruction}");
    if body.contains(prompt) { body.replacen(prompt, &section, 1) } else { format!("{body}\n\n{section}\n") }
}

fn screenshot_path(token: u64) -> String {
    let name = format!("solidcraft-issue-{token}-{:.0}.png", crate::now_ms());
    #[cfg(not(target_arch = "wasm32"))]
    let name = std::env::temp_dir().join(name).to_string_lossy().into_owned();
    // The web host downloads only writes under this prefix; other VFS paths stay in memory.
    #[cfg(target_arch = "wasm32")]
    let name = format!("/downloads/{name}");
    name
}

/// The text and image share one clipboard. Decide with the final paste instruction included,
/// so a report near the URL limit takes the file path too. The web host downloads VFS writes.
pub(super) fn open_github(draft: &mut Draft, ctx: &egui::Context, path: &str) {
    let mut body = draft.report_body();
    let title = draft.report_title();
    let english = solidcraft_ui_text::Locale::for_language("en");
    if draft.image.is_some() {
        let key = paste_instruction(ctx.os());
        let paste_body = with_visual_context(&body, english.text(key));
        let (url, copy_needed) = draft_url(&title, &paste_body);
        if !copy_needed {
            copy_screenshot(draft, ctx);
            // The window's note names the GitHub form; the issue's own wording says "this box".
            draft.note = solidcraft_ui_text::tr("ui.report_screenshot_copied").to_string();
            ctx.open_url(egui::OpenUrl::new_tab(url));
            draft.opened_github = true;
            return;
        }
        if let Some(image) = &draft.image {
            match crate::control::write_screenshot(image, path) {
                Ok(()) => {
                    #[cfg(target_arch = "wasm32")]
                    let path = path.strip_prefix("/downloads/").unwrap_or(path);
                    let key = "ui.report_screenshot_drag";
                    // The issue is public: it names the file only, never the folder (which can hold the
                    // user name). The window's note keeps the whole path so the reporter can find it.
                    let file = std::path::Path::new(path).file_name().and_then(|n| n.to_str()).unwrap_or(path);
                    body = with_visual_context(&body, &english.format(key, &[("file", file)]));
                    draft.note = solidcraft_ui_text::format(key, &[("file", path)]);
                }
                Err(error) => {
                    draft.note = solidcraft_ui_text::format("ui.report_screenshot_save_failed", &[("error", &super::limited(&error, 1000))]);
                }
            }
        }
        // Always keep the full text on the clipboard once the image paste version exceeds
        // the URL limit, even if the fallback wording would fit.
        let url = format!("{}?title={}", super::ISSUE_URL, super::encode(&super::limited(&title, 160)));
        ctx.copy_text(body);
        ctx.open_url(egui::OpenUrl::new_tab(url));
    } else {
        let (url, _) = draft_url(&title, &body);
        ctx.copy_text(body);
        ctx.open_url(egui::OpenUrl::new_tab(url));
    }
    draft.opened_github = true;
}

const ALL_GUIDANCE: [&str; 6] = [
    "ui.report_screenshot_copied",
    "ui.report_screenshot_open",
    "Report text copied. Paste it into GitHub and submit your report there.",
    "Review your report on GitHub and click Submit new issue to publish it.",
    "Opens your browser and copies the report text for you to paste into GitHub.",
    "Opens your browser and copies the report text. Review and submit your report on GitHub.",
];

fn answer(ui: &mut Ui, label: &str, hint: &str, text: &mut String, id: &str, rows: usize) -> egui::Response {
    ui_kit::text(ui, tr_label(label), ui_kit::Text::Heading(label));
    ui_kit::text(ui, tr_label(hint), ui_kit::Text::Note(hint));
    let response = ui_kit::text_area(ui, tr_label(label), Id::unique(id), text, rows, MAX_ANSWER);
    crate::scenario::publish_handle(id, response.rect.center());
    super::truncate_chars(text, MAX_ANSWER);
    // What doesn't fit is not added: near the limit, say how much room is left (#7191).
    let left = MAX_ANSWER.saturating_sub(text.chars().count());
    if left < super::ANSWER_ROOM {
        let key = if left == 1 { "ui.report_characters_left_one" } else { "ui.report_characters_left_other" };
        ui_kit::text(ui, &solidcraft_ui_text::format(key, &[("count", &left.to_string())]), ui_kit::Text::Note(id));
    }
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
            if ui_kit::button(ui, tr_label("Copy screenshot"), ui_kit::Button::Normal).clicked() {
                copy_screenshot(draft, ui.ctx());
                draft.note = "Paste the image into the GitHub draft, or save the PNG if image copying is unavailable.".into();
            }
            if ui_kit::button(ui, tr_label("Save screenshot…"), ui_kit::Button::Enabled(app.services.pick_save.is_some())).clicked()
                && let Some(image) = draft.image.clone()
            {
                app.pick_save(&format!("solidcraft-issue-{}.png", app.shots.token), &["png"], move |app, path| {
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

/// Returns whether to retake the screenshot and whether the user closed the form.
pub(super) fn show(app: &mut SolveApp, draft: &mut Draft, ctx: &egui::Context) -> (bool, bool) {
    let mut retake = false;
    let t = solidcraft_theme::Tokens::get();
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
            ("Open GitHub issue form", crate::ui_kit::type_face::bold(t.font_label), t.space_16),
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
    let (closed, mut action) = ui_kit::Window::new(Id::unique("sc_issue_report"), "bug_report", tr_label("Report a bug"))
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
                        let mut body = draft.report_body();
                        if draft.image.is_some() {
                            body = with_visual_context(&body, solidcraft_ui_text::Locale::for_language("en").text(paste_instruction(ctx.os())));
                        }
                        let (_, copy_needed) = draft_url(&draft.report_title(), &body);
                        let guidance = if draft.opened_github {
                            if copy_needed {
                                "Report text copied. Paste it into GitHub and submit your report there."
                            } else if draft.image.is_some() {
                                "ui.report_screenshot_copied"
                            } else {
                                "Review your report on GitHub and click Submit new issue to publish it."
                            }
                        } else if copy_needed {
                            "Opens your browser and copies the report text for you to paste into GitHub."
                        } else if draft.image.is_some() {
                            "ui.report_screenshot_open"
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
                            Id::unique("issue_report_title"),
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
                        ui_kit::text(
                            ui,
                            tr_label("App/system information and command settings are included. Review private details before sharing."),
                            ui_kit::Text::Note("private"),
                        );
                        ui_kit::section(ui, "issue-report-technical", "Technical details", false, |ui| {
                            let response = ui_kit::text_area(
                                ui,
                                tr_label("Technical details"),
                                Id::unique("issue_report_technical"),
                                &mut draft.body,
                                8,
                                MAX_BODY,
                            );
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
                open_github(draft, ctx, &screenshot_path(app.shots.token));
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
