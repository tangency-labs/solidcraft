//! Feedback: status lines and notes, progress and checks, toasts, confirmations, the viewport's
//! status items and the attention mark.

use super::{Group, Page};
use crate::state;
use egui::vec2;
use solidcraft_ui_kit as kit;

pub const STATUS_LINE: Page = Page {
    id: "status-line",
    title: "Status line and notes",
    group: Group::Feedback,
    items: &["status_line", "StatusTone", "caption", "warning_caption", "Text"],
    what: "A full-width line that says why something cannot happen yet or what went wrong, in the dim, warning or error tone, placed in the form where the problem is.",
    usage: r#"
ui_kit::status_line(ui, "Needs a joint that turns and one that slides", ui_kit::StatusTone::Dim);
ui_kit::status_line(ui, "The interference check could not finish.", ui_kit::StatusTone::Error);
ui_kit::text(ui, "Counts include the original.", ui_kit::Text::Note("pattern"));
"#,
    dos: &["Say what to do, not only what is wrong.", "Put it next to the input it is about, outside a `row`."],
    donts: &[
        "Don't open a dialog for a problem the form can show in place.",
        "Don't use red text for something that is only not ready yet: that is the dim tone.",
    ],
    demo: |ui, _| {
        state(ui, "Dim (not ready yet)", |ui| {
            kit::status_line(ui, "Needs a joint that turns and one that slides.", kit::StatusTone::Dim);
        });
        state(ui, "Warning", |ui| {
            kit::status_line(ui, "Two faces are thinner than the wall thickness.", kit::StatusTone::Warning);
        });
        state(ui, "Error", |ui| {
            kit::status_line(ui, "The interference check could not finish. Check the selected bodies.", kit::StatusTone::Error);
        });
        state(ui, "Note", |ui| {
            kit::text(
                ui,
                "Counts include the original. Centre balances copies; Flip chooses the extra side for even totals.",
                kit::Text::Note("gallery_note"),
            );
        });
    },
};

pub const PROGRESS: Page = Page {
    id: "progress",
    title: "Progress and checks",
    group: Group::Feedback,
    items: &[
        "progress",
        "progress_action",
        "steps",
        "check_line",
        "check_status_line",
        "CheckStatus",
        "status_header",
        "StatusHeader",
        "completion_text",
    ],
    what: "Long work shown as it goes: a bar, a bar with Cancel, the steps of a job, and a list of checks with their result.",
    usage: r#"
ui_kit::progress(ui, "import", 0.4);
ui_kit::progress_action(ui, "render", "Rendering", "12 of 30 samples", 0.4, "Cancel");
ui_kit::steps(ui, "import", &["Read", "Mesh", "Show"], 1, Some("40%"));
ui_kit::check_status_line(ui, "watertight", "Watertight", ui_kit::CheckStatus::Ok);
"#,
    dos: &["Offer Cancel for anything that can take more than a few seconds.", "Keep the window responsive: do slow work off the window thread."],
    donts: &["Don't show a bar that cannot move: use the steps or a spinner."],
    demo: |ui, d| {
        state(ui, "Progress (drag the fraction)", |ui| {
            kit::progress(ui, "gallery_progress", d.fraction.clamp(0.0, 1.0));
            kit::row(ui, "Fraction", |ui| kit::slider(ui, &mut d.fraction, 0.0..=1.0, kit::Slider::default()));
        });
        state(ui, "With Cancel", |ui| {
            if kit::progress_action(ui, "gallery_render", "Rendering", "12 of 30 samples", 0.4, "Cancel").clicked() {
                d.said("Cancel");
            }
        });
        state(ui, "Steps", |ui| {
            kit::steps(ui, "gallery_steps", &["Read", "Mesh", "Show"], 1, Some("40%"));
        });
        state(ui, "Checks", |ui| {
            kit::check_status_line(ui, "gallery_ok", "Watertight", kit::CheckStatus::Ok);
            kit::check_status_line(ui, "gallery_warn", "Two thin walls", kit::CheckStatus::Warning);
            kit::check_status_line(ui, "gallery_err", "Self-intersecting", kit::CheckStatus::Error);
            kit::check_line(ui, "gallery_line", "Ready to print", false);
        });
        state(ui, "Header", |ui| {
            kit::status_header(
                ui,
                "gallery_header",
                "Sketch 1",
                kit::StatusHeader { icon: "sketch", text: "2 degrees of freedom", complete: false, fraction: 0.5 },
            );
        });
    },
};

pub const TOAST: Page = Page {
    id: "toast",
    title: "Toast",
    group: Group::Feedback,
    items: &["toast::show", "toast::Place", "toast::Lead", "toast::Action", "toast::Extras", "toast::Shown"],
    what: "A short message in the corner after something happened (saved, exported, failed), with up to a couple of actions and a close. One that goes by itself shows a ring counting down.",
    usage: r#"
use ui_kit::toast;
let shown = toast::show(ui, free, toast::Place::Corner, "Exported Bracket.step".into(), toast::Lead::Dot(false),
    &[("Show in folder", toast::Action::Primary)],
    toast::Extras { close: Some("export"), countdown: Some((left, 6.0)), ..Default::default() });
"#,
    dos: &[
        "Use a toast for news about something done, with the action that follows (Show in folder, Undo).",
        "Give errors a red dot and keep them until closed.",
    ],
    donts: &["Don't use a toast for a question: that is a `Confirm`.", "Don't stack many toasts: replace the older one."],
    demo: |ui, d| {
        let width = ui.available_width().min(520.0);
        for (name, text, lead, countdown) in [
            ("Done, going by itself", "Exported Bracket.step", kit::toast::Lead::Dot(false), Some((0.6, 6.0))),
            ("Working", "Exporting Bracket.step…", kit::toast::Lead::Spinner, None),
            ("Error", "Bracket.step could not be written: the folder is read-only.", kit::toast::Lead::Dot(true), None),
        ] {
            state(ui, name, |ui| {
                let (free, _) = ui.allocate_exact_size(vec2(width, 120.0), egui::Sense::hover());
                let shown = kit::toast::show(
                    ui,
                    free,
                    kit::toast::Place::Corner,
                    text.into(),
                    lead,
                    &[("Show in folder", kit::toast::Action::Primary), ("Undo", kit::toast::Action::Dismiss)],
                    kit::toast::Extras { close: Some("gallery"), countdown, ..Default::default() },
                );
                if let Some(shown) = shown {
                    if let Some(i) = shown.clicked {
                        d.said(format!("button {i}"));
                    }
                    if shown.closed {
                        d.said("closed");
                    }
                }
            });
        }
    },
};

pub const CONFIRM: Page = Page {
    id: "confirm",
    title: "Confirm",
    group: Group::Feedback,
    items: &["Confirm", "Tone", "dialog_mnemonics"],
    what: "The dialog that asks before something that cannot be undone or loses work: its tone (info, warning, danger), what will happen, and the actions named for what they do.",
    usage: r#"
let (closed, action) = ui_kit::Confirm::new(egui::Id::unique("save?"), "status_warning", "Save changes?")
    .tone(ui_kit::Tone::Warning)
    .actions("Save", "Cancel", Some("Don't Save"))
    .show(ctx, true, |ui| { ui_kit::text(ui, "Your changes will be lost.", ui_kit::Text::Report("msg", false)); });
"#,
    dos: &["Name the buttons for what they do (Delete, Don't Save), not Yes and No.", "List what will be deleted with `detail`."],
    donts: &["Don't confirm what Undo can take back.", "Don't make the destructive action the default for Enter."],
    demo: |ui, d| {
        ui.set_max_width(460.0);
        state(ui, "Warning", |ui| {
            let (_, action) = kit::Confirm::new(ui.scope_id().with("warn"), "status_warning", "Save changes?")
                .tone(kit::Tone::Warning)
                .actions("Save", "Cancel", Some("Don't Save"))
                .content(ui, true, |ui| {
                    kit::text(ui, "Your changes to Bracket will be lost.", kit::Text::Report("warn", false));
                });
            if action != kit::Action::None {
                d.said(format!("{action:?}"));
            }
        });
        state(ui, "Danger", |ui| {
            kit::Confirm::new(ui.scope_id().with("danger"), "delete", "Delete")
                .tone(kit::Tone::Danger)
                .detail("Fillet2 · Chamfer1")
                .actions("Delete", "Cancel", None)
                .content(ui, true, |ui| {
                    kit::text(ui, "These features will be deleted:", kit::Text::Report("danger", false));
                });
        });
    },
};

pub const VIEWPORT_STATUS: Page = Page {
    id: "viewport-status",
    title: "Viewport status",
    group: Group::Feedback,
    items: &["viewport_status_item", "StatusItem", "StatusKind", "StatusPresentation", "StatusAction", "viewport_status_row"],
    what: "The chips along the 3D view's edge that say a mode or analysis is on (Draft, Section, Zebra), open its settings and clear it.",
    usage: r#"
let item = ui_kit::StatusItem { id: "draft".into(), kind: ui_kit::StatusKind::Mode, name: "Draft".into(),
    setting: "Z · 1°".into(), has_key: true, editable: false };
let (rect, action) = ui_kit::viewport_status_item(ui, &item, &mut presentation);
"#,
    dos: &["Show every mode that changes what the view draws, so it is never on unnoticed."],
    donts: &["Don't keep the presentation state per frame: it remembers which card is open."],
    demo: |ui, d| {
        for (id, kind, name, setting) in [
            ("draft", kit::StatusKind::Mode, "Draft", "Z · 1°"),
            ("interference", kit::StatusKind::Result, "Interference", "2 found"),
            ("thin", kit::StatusKind::Attention, "Thin walls", "3 faces"),
        ] {
            let item = kit::StatusItem { id: id.into(), kind, name: name.into(), setting: setting.into(), has_key: true, editable: false };
            let (_, action) = kit::viewport_status_item(ui, &item, &mut d.status);
            if let Some(action) = action {
                d.said(format!("{action:?}"));
            }
            ui.add_space(4.0);
        }
    },
};

pub const ATTENTION: Page = Page {
    id: "attention",
    title: "Attention mark",
    group: Group::Feedback,
    items: &["attention_mark", "attention_at"],
    what: "The small badge on a control that has something new or wrong behind it, with a count when there is more than one.",
    usage: r#"
let response = ui_kit::button(ui, "Reports", ui_kit::Button::Normal);
ui_kit::attention_mark(ui.painter(), response.rect, Some(3));
"#,
    dos: &["Clear it as soon as the user has seen what it points to."],
    donts: &["Don't use it for decoration or to advertise features."],
    demo: |ui, _| {
        ui.horizontal(|ui| {
            let r = kit::button(ui, "Reports", kit::Button::Normal).rect;
            kit::attention_mark(ui.painter(), r, Some(3));
            ui.add_space(24.0);
            let r = kit::button(ui, "Updates", kit::Button::Normal).rect;
            kit::attention_mark(ui.painter(), r, None);
        });
    },
};
