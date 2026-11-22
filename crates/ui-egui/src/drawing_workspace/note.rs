//! Placement and editing of plain-text sheet notes.
use serde_json::json;
use solidcore_record::Vec2;
use solidcraft_engine::{
    doc::ids,
    geom::cell::{HAlign, MAX_TEXT_BYTES, Part},
};
use solidcraft_theme::Tokens;

use super::close_panel;
use crate::{
    SolveApp,
    ui_kit::{self, Button},
};

/// The choices of the alignment control, in the order it lists them: where the note's text stands
/// from its place.
/// with the word the commands take for each.
const ALIGNS: [(HAlign, &str, &str, &str); 3] = [
    (HAlign::Left, "left", "ui.drawing_note.align_left", "drawing:note:align:left"),
    (HAlign::Centre, "centre", "ui.drawing_note.align_centre", "drawing:note:align:centre"),
    (HAlign::Right, "right", "ui.drawing_note.align_right", "drawing:note:align:right"),
];

/// The word `drawing.note.add` and `.edit` take for `align`.
fn word(align: HAlign) -> &'static str {
    ALIGNS.iter().find(|(one, ..)| *one == align).map_or("centre", |(_, word, ..)| word)
}

#[derive(Default)]
pub(super) struct Draft {
    text: String,
    /// Which end of the text sits at its place: left for a note about to be placed, as the command
    /// has it, and the note's own when one is edited. None for an edited note saved before alignments,
    /// which stays as it is unless the choice is touched (the control shows it as centre).
    align: Option<HAlign>,
    /// The note being edited has linked fields: its text isn't edited here, its alignment is.
    fields: bool,
    /// The preview last drawn: for this text, with the pointer here, on this sheet, aligned so. Drawn again only
    /// when one of them changes.
    shown: Option<(String, Vec2, u64, Option<HAlign>, Option<crate::drawing_sheet_page::Marks>)>,
}

/// How finely a preview's letters are flattened, on paper.
const PREVIEW_TOLERANCE_MM: f64 = 0.01;

fn tr(key: &'static str) -> &'static str {
    solidcraft_ui_text::tr(key)
}

fn ready(text: &str) -> bool {
    !text.trim().is_empty() && text.len() <= MAX_TEXT_BYTES
}

pub(super) fn editor(app: &mut SolveApp, ui: &mut egui::Ui, selected: Option<u64>) {
    if app.drawing_workspace.note.is_none() {
        let draft = match selected {
            None => Some(Draft { align: Some(HAlign::Left), ..Draft::default() }),
            Some(id) => app.session.drawing("drawing.inspect").ok().and_then(|drawing| {
                let note = drawing.notes.by_id(id)?;
                let text = note.content.0.iter().map(|part| match part {
                    Part::Text(text) => Some(text.as_str()),
                    Part::Field { .. } => None,
                });
                let plain = text.collect::<Option<String>>();
                Some(Draft { fields: plain.is_none(), text: plain.unwrap_or_default(), align: note.align, shown: None })
            }),
        };
        app.drawing_workspace.note = draft;
    }
    let (cancel, save) = super::inset(ui, |ui| {
        let mut valid = false;
        if let Some(draft) = app.drawing_workspace.note.as_mut() {
            if draft.fields {
                ui_kit::text(ui, tr("ui.drawing_note.fields"), ui_kit::Text::Note("drawing_note_fields"));
                valid = true;
            } else {
                let field = ui_kit::multiline_text_field(ui, tr("ui.drawing_panel.text"), ui.scope_id().with("note_text"), &mut draft.text);
                crate::scenario::publish_rect("drawing:note:text", field.rect);
                valid = ready(&draft.text);
            }
            let labels = ALIGNS.map(|(_, _, key, _)| tr(key));
            let targets = ALIGNS.map(|(.., target)| target);
            let shown = draft.align.unwrap_or(HAlign::Centre);
            let mut index = ALIGNS.iter().position(|(align, ..)| *align == shown).unwrap_or(1);
            ui_kit::row(ui, tr("ui.drawing_note.align"), |ui| {
                ui_kit::segmented(ui, &mut index, &labels, ui_kit::Segments { targets: &targets, ..Default::default() })
            });
            if let Some((align, ..)) = ALIGNS.get(index).filter(|(align, ..)| *align != shown) {
                draft.align = Some(*align);
            }
            if selected.is_none() {
                ui_kit::text(ui, tr("ui.drawing_note.place"), ui_kit::Text::Note("drawing_note_place"));
            }
        }
        ui.separator();
        ui.horizontal(|ui| {
            let cancel = ui_kit::button(ui, tr("ui.cancel"), Button::Normal).clicked();
            let save = selected.is_some() && ui.add_enabled_ui(valid, |ui| ui_kit::button(ui, tr("ui.heading_ok"), Button::Normal)).inner.clicked();
            (cancel, save)
        })
        .inner
    });
    if cancel {
        close_panel(app);
    } else if save
        && let Some(id) = selected
        && let Some(draft) = app.drawing_workspace.note.as_ref()
    {
        let mut set = if draft.fields { json!({}) } else { json!({"content": draft.text}) };
        if let Some(align) = draft.align {
            set["align"] = json!(word(align));
        }
        let params = json!({"note": ids::encode(id), "set": set});
        match app.run("drawing.note.edit", params) {
            Ok(_) => {
                close_panel(app);
            }
            Err(error) => app.set_status(error, true),
        }
    }
}

pub(crate) fn on_sheet(app: &mut SolveApp, painter: &egui::Painter, at: crate::drawing_sheet_page::OnScreen, point: Vec2, sheet: u64, clicked: bool) {
    if !app.drawing_workspace.note.as_ref().is_some_and(|draft| ready(&draft.text)) {
        return;
    }
    let Some(mut draft) = app.drawing_workspace.note.take() else { return };
    let params = json!({"anchor":{"sheet":ids::encode(sheet)}, "at":[point.x, point.y], "content":draft.text, "align": draft.align.map(word)});
    // The note as the sheet will draw it once placed: its own group, in the drawing's lettering, at
    // its paper size, in the accent colour as every preview on the sheet is.
    let fresh =
        draft.shown.as_ref().is_some_and(|(text, at, on, align, _)| *text == draft.text && *at == point && *on == sheet && *align == draft.align);
    if !fresh {
        let marks = solidcraft_engine::drawing_scene::note_preview_scene(&app.session, &params)
            .ok()
            .map(|scene| crate::drawing_sheet_page::Marks::of(&scene, 0, PREVIEW_TOLERANCE_MM));
        draft.shown = Some((draft.text.clone(), point, sheet, draft.align, marks));
    }
    if let Some((.., Some(marks))) = &draft.shown {
        at.paint_preview(painter, marks, Vec2::ZERO, Tokens::get().accent);
    }
    app.drawing_workspace.note = Some(draft);
    if clicked {
        match app.run("drawing.note.add", params) {
            Ok(_) => {
                close_panel(app);
            }
            Err(error) => app.set_status(error, true),
        }
    }
}
