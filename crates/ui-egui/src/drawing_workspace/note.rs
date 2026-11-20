//! Placement and editing of plain-text sheet notes.
use serde_json::json;
use solidcore_record::Vec2;
use solidcraft_engine::{
    doc::ids,
    geom::cell::{MAX_TEXT_BYTES, Part},
};
use solidcraft_theme::Tokens;

use super::close_panel;
use crate::{
    SolveApp,
    ui_kit::{self, Button},
};

#[derive(Default)]
pub(super) struct Draft {
    text: String,
}

fn tr(key: &'static str) -> &'static str {
    solidcraft_ui_text::tr(key)
}

fn ready(text: &str) -> bool {
    !text.trim().is_empty() && text.len() <= MAX_TEXT_BYTES
}

pub(super) fn editor(app: &mut SolveApp, ui: &mut egui::Ui, selected: Option<u64>) {
    if app.drawing_workspace.note.is_none() {
        let text = match selected {
            None => Some(String::new()),
            Some(id) => app.session.drawing("drawing.inspect").ok().and_then(|drawing| {
                let note = drawing.notes.by_id(id)?;
                note.content
                    .0
                    .iter()
                    .map(|part| match part {
                        Part::Text(text) => Some(text.as_str()),
                        Part::Field { .. } => None,
                    })
                    .collect::<Option<String>>()
            }),
        };
        app.drawing_workspace.note = text.map(|text| Draft { text });
    }
    let (cancel, save) = super::inset(ui, |ui| {
        let mut valid = false;
        if let Some(draft) = app.drawing_workspace.note.as_mut() {
            let field = ui_kit::multiline_text_field(ui, tr("ui.drawing_panel.text"), ui.scope_id().with("note_text"), &mut draft.text);
            crate::scenario::publish_rect("drawing:note:text", field.rect);
            valid = ready(&draft.text);
            if selected.is_none() {
                ui_kit::text(ui, tr("ui.drawing_note.place"), ui_kit::Text::Note("drawing_note_place"));
            }
        } else {
            ui_kit::text(ui, tr("ui.drawing_note.fields"), ui_kit::Text::Note("drawing_note_fields"));
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
        let params = json!({"note": ids::encode(id), "set": {"content": draft.text}});
        match app.run("drawing.note.edit", params) {
            Ok(_) => {
                close_panel(app);
            }
            Err(error) => app.set_status(error, true),
        }
    }
}

pub(crate) fn on_sheet(app: &mut SolveApp, painter: &egui::Painter, screen: egui::Pos2, point: Vec2, sheet: u64, clicked: bool) {
    let Some(draft) = app.drawing_workspace.note.as_ref().filter(|draft| ready(&draft.text)) else { return };
    let tokens = Tokens::get();
    painter.text(screen, egui::Align2::LEFT_TOP, &draft.text, egui::FontId::proportional(tokens.font_small), tokens.accent);
    if clicked {
        let params = json!({"anchor":{"sheet":ids::encode(sheet)}, "at":[point.x, point.y], "content":draft.text});
        match app.run("drawing.note.add", params) {
            Ok(_) => {
                close_panel(app);
            }
            Err(error) => app.set_status(error, true),
        }
    }
}
