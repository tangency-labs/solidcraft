//! Explicit UI specimen source for scenarios and live component review. Never engine state.
use crate::ui_kit::{StatusAction, StatusItem, StatusKind, StatusPresentation};
use serde_json::{Value, json};

#[derive(Default)]
pub(crate) struct Fixture {
    items: Vec<StatusItem>,
    presentation: StatusPresentation,
}

fn item(id: &str, kind: StatusKind, label: &str, setting: String, has_key: bool) -> StatusItem {
    StatusItem { id: id.into(), kind, name: crate::i18n::tr(label).into(), setting, has_key, editable: false }
}

/// Presets are bounded, named specimens rather than arbitrary user-provided widget state.
pub(crate) fn control(app: &mut crate::SolveApp, params: &Value) -> Result<Value, String> {
    let preset = params.get("preset").and_then(Value::as_str).ok_or("missing `preset`")?;
    let zebra = || item("zebra", StatusKind::Mode, "Zebra", crate::i18n::format("ui.viewport_status_stripes", &[("count", "12")]), false);
    let section = || item("section", StatusKind::Mode, "Section", "Z 20 mm".into(), false);
    let result = || item("radius", StatusKind::Result, "ui.viewport_status_minimum_radius", "0.8 mm".into(), false);
    let attention = || {
        item(
            "interference",
            StatusKind::Attention,
            "cmd.inspect_interference",
            crate::i18n::format("ui.viewport_status_pairs", &[("count", "3")]),
            false,
        )
    };
    let items = match preset {
        "off" => Vec::new(),
        "mode" => vec![zebra()],
        "result" => vec![result()],
        "attention" => vec![attention()],
        "row" => vec![zebra(), section()],
        "kinds" => vec![zebra(), result(), attention()],
        "key" => vec![item("draft", StatusKind::Mode, "ui.draft", "Z · 1°".into(), true)],
        _ => return Err("preset must be off, mode, result, attention, row, kinds or key".into()),
    };
    app.viewport_status_fixture = Fixture { items, presentation: StatusPresentation::default() };
    Ok(json!({"preset": preset, "count": app.viewport_status_fixture.items.len()}))
}

pub(crate) fn show(app: &mut crate::SolveApp, ctx: &egui::Context, viewport: egui::Rect) {
    if app.viewport_status_fixture.items.is_empty() {
        return;
    }
    let content = crate::shell::content_rect(app, viewport);
    if !content.is_finite() || !content.is_positive() {
        return;
    }
    let mut free = crate::shell::workspace_content(app, content);
    for card in &app.shell.cards {
        free.min.y = free.top().max(card.bottom()).min(free.bottom());
    }
    if let Some(layout) = &app.viewport.layout {
        free.max.x = free.right().min(layout.axis_view.left()).max(free.left());
    }
    if !free.is_finite() || !free.is_positive() {
        return;
    }
    let fixture = &mut app.viewport_status_fixture;
    let actions = crate::ui_kit::viewport_status_row(ctx, free, &fixture.items, &mut fixture.presentation, |ui, _| {
        let t = crate::theme::Tokens::get();
        ui.label(crate::i18n::tr("cmd.inspect_draft"));
        ui.label(egui::RichText::new(format!("{} (0.00, 0.00, 1.00)", crate::i18n::tr("ui.pull_direction"))).monospace().color(t.text_dim));
        for (index, (colour, label)) in t.analysis.draft.into_iter().zip(["≥ 1.0°", "−1.0° … 1.0°", "≤ −1.0°"]).enumerate() {
            ui.horizontal(|ui| {
                let (rect, _) = ui.allocate_exact_size(egui::vec2(t.space_12, t.space_12), egui::Sense::hover());
                ui.painter().rect_filled(rect, t.radius_sm, crate::theme::srgb(colour));
                if index == 2 && t.analysis.hatch {
                    let painter = ui.painter().with_clip_rect(rect);
                    for offset in [-t.space_8, -t.space_4, 0.0, t.space_4, t.space_8] {
                        let from = rect.left_bottom() + egui::vec2(offset, 0.0);
                        painter.line_segment([from, from + egui::vec2(rect.height(), -rect.height())], ui.visuals().selection.stroke);
                    }
                }
                ui.label(label);
            });
        }
    });
    for action in actions {
        if let StatusAction::Clear(id) = action {
            fixture.items.retain(|item| item.id != id);
        }
    }
}
