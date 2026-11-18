//! A projected length dimension picked from an edge on a drawing view.
use serde_json::{Value, json};
use solidcore_record::Vec2;
use solidcraft_engine::{
    doc::ids,
    drawing_scene,
    geom::{Vec2Geom, hlr::Source},
};
use solidcraft_theme::Tokens;

use super::close_panel;
use crate::{
    SolveApp,
    drawing_sheet_page::OnScreen,
    ui_kit::{self, Button},
};

const COMMAND: &str = "drawing.dimension.add";
const SAMPLE_OFFSET: f64 = 10.0;

pub(super) struct Draft {
    sheet: u64,
    revision: u64,
    params: Value,
    endpoints: [Vec2; 2],
    normal: Vec2,
    label: String,
}

pub(super) fn panel(app: &mut SolveApp, ui: &mut egui::Ui) {
    let key = if app.drawing_workspace.dimension.is_some() { "ui.drawing_dimension_tool.place" } else { "ui.drawing_dimension_tool.pick" };
    let close = super::inset(ui, |ui| {
        ui_kit::text(ui, solidcraft_ui_text::tr(key), ui_kit::Text::Note(key));
        ui.separator();
        ui_kit::button(ui, solidcraft_ui_text::tr("ui.cancel"), Button::Normal).clicked()
    });
    if close {
        close_panel(app);
    }
}

fn tip(answer: &Value, index: usize) -> Option<Vec2> {
    serde_json::from_value::<[f64; 2]>(answer["layout"]["arrows"].get(index)?.get("tip")?.clone()).ok().map(Vec2::from)
}

fn pick(app: &mut SolveApp, sheet: u64, point: Vec2, band: f64) -> Result<Option<Draft>, String> {
    let Some(hit) = drawing_scene::line_at(&mut app.session, sheet, point, band).map_err(|error| error.to_string())? else { return Ok(None) };
    let Source::Edge { edge, .. } = hit.source else { return Ok(None) };
    let mut params = json!({
        "view": ids::encode(hit.view),
        "kind": {"length": {"edge": {"body": hit.body.body, "index": edge, "path": hit.body.path.into_iter().map(ids::encode).collect::<Vec<_>>()}}},
        "offset_mm": SAMPLE_OFFSET,
        "dry_run": true,
    });
    let sample = app.run(COMMAND, params.clone())?;
    params["offset_mm"] = json!(SAMPLE_OFFSET + 1.0);
    let shifted = app.run(COMMAND, params.clone())?;
    let Some((a, b, c)) = tip(&sample, 0).zip(tip(&sample, 1)).zip(tip(&shifted, 0)).map(|((a, b), c)| (a, b, c)) else {
        return Ok(None);
    };
    let normal = c - a;
    if !(0.99..=1.01).contains(&normal.dot(normal)) {
        return Ok(None);
    }
    let endpoints = [a - normal * SAMPLE_OFFSET, b - normal * SAMPLE_OFFSET];
    params["dry_run"] = json!(false);
    Ok(Some(Draft {
        sheet,
        revision: app.session.revision,
        params,
        endpoints,
        normal,
        label: sample["text"].as_str().unwrap_or_default().to_owned(),
    }))
}

pub(crate) fn on_sheet(app: &mut SolveApp, painter: &egui::Painter, at: OnScreen, sheet: u64, point: Vec2, band: f64, clicked: bool) {
    let draft = app.drawing_workspace.dimension.take().filter(|draft| draft.sheet == sheet && draft.revision == app.session.revision);
    if let Some(mut draft) = draft {
        let [a, b] = draft.endpoints;
        let offset = (point - a).dot(draft.normal);
        let shift = draft.normal * offset;
        let tokens = Tokens::get();
        let stroke = egui::Stroke::new(1.0, tokens.accent);
        for [from, to] in [[a, a + shift], [b, b + shift], [a + shift, b + shift]] {
            painter.line_segment([at.screen(from), at.screen(to)], stroke);
        }
        painter.text(
            at.screen((a + b) * 0.5 + shift),
            egui::Align2::CENTER_BOTTOM,
            &draft.label,
            egui::FontId::proportional(tokens.font_small),
            tokens.accent,
        );
        if clicked {
            draft.params["offset_mm"] = json!(offset);
            match app.run(COMMAND, draft.params.clone()) {
                Ok(_) => {
                    close_panel(app);
                    return;
                }
                Err(error) => app.set_status(error, true),
            }
        }
        app.drawing_workspace.dimension = Some(draft);
    } else if clicked {
        match pick(app, sheet, point, band) {
            Ok(draft) => {
                app.drawing_workspace.dimension = draft;
                painter.ctx().request_repaint();
            }
            Err(error) => app.set_status(error, true),
        }
    }
}
