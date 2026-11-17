//! Manufacturing grain form and its display-only preview.

use serde_json::{Value, json};
use solidcraft_engine::Sel;

use crate::SolveApp;
use crate::dialogs::Kind;
use crate::selection::{BODIES, EDGES, PLANAR_FACES, SelInput};

#[derive(Clone, Debug)]
pub struct Grain {
    pub mode: usize,
    pub vector: [String; 3],
}

pub fn start(_: &SolveApp, id: &str) -> Option<(Kind, Vec<SelInput>)> {
    (id == "wood.grain").then(|| {
        (
            Kind::WoodGrain(Grain { mode: 0, vector: ["1".into(), "0".into(), "0".into()] }),
            vec![
                SelInput::new("ui.grain_body", BODIES, false),
                SelInput::new("ui.grain_edge", EDGES, false),
                SelInput::new("ui.grain_face", PLANAR_FACES, false),
            ],
        )
    })
}

pub fn rows(app: &SolveApp, ui: &mut egui::Ui, grain: &mut Grain) -> bool {
    let labels: Vec<&str> = ["ui.grain_by_edge", "ui.grain_length", "ui.grain_width", "ui.grain_thickness", "ui.grain_vector", "ui.grain_none"]
        .iter()
        .map(|id| app.tr(id))
        .collect();
    crate::ui_kit::choice_row(ui, app.tr("ui.grain_direction"), "wood_grain_direction", &mut grain.mode, &labels);
    let mut enter = false;
    if grain.mode == 4 {
        for (label, value) in ["X", "Y", "Z"].into_iter().zip(&mut grain.vector) {
            enter |= crate::ui_kit::text_row(ui, label, value, "").0;
        }
    }
    ui.label(app.tr("ui.grain_preview"));
    ui.label(app.tr("ui.wood_finish_preview"));
    enter
}

pub fn commands(grain: &Grain, inputs: &[SelInput]) -> Result<Vec<(String, Value)>, String> {
    let tr = |id| solidcraft_ui_text::tr(id).to_string();
    let Some(Sel::Body { name }) = inputs.first().and_then(|i| i.items.first()) else { return Err(tr("ui.grain_pick_body")) };
    let direction = match grain.mode {
        0 => {
            let Some(Sel::Edge { body, index, .. }) = inputs.get(1).and_then(|i| i.items.first()) else { return Err(tr("ui.grain_pick_edge")) };
            if body != name {
                return Err(tr("ui.grain_same_body"));
            }
            let mut pick = json!({"edge": index, "body": body});
            if let Some(Sel::Face { body, index, .. }) = inputs.get(2).and_then(|i| i.items.first()) {
                if body != name {
                    return Err(tr("ui.grain_same_body"));
                }
                pick["face"] = json!(index);
            }
            pick
        }
        1 => json!("length"),
        2 => json!("width"),
        3 => json!("thickness"),
        4 => {
            let mut vector = Vec::new();
            for value in &grain.vector {
                let v = value.parse::<f64>().ok().filter(|v| v.is_finite()).ok_or_else(|| tr("ui.grain_valid_vector"))?;
                vector.push(v);
            }
            json!(vector)
        }
        5 => json!("none"),
        _ => return Err(tr("ui.grain_direction")),
    };
    Ok(vec![("wood.grain".into(), json!({"body": name, "direction": direction}))])
}

/// Current form parameters, for both the preview and the scene cache's identity.
pub fn preview(app: &SolveApp) -> Option<Value> {
    let dialog = app.dialog.as_ref()?;
    let Kind::WoodGrain(grain) = &dialog.kind else { return None };
    commands(grain, &dialog.inputs).ok()?.into_iter().next().map(|(_, p)| p)
}

#[cfg(test)]
#[path = "wood_grain_tests.rs"]
mod tests;
