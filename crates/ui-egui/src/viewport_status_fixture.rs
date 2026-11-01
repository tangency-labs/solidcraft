//! Explicit UI specimen source for scenarios and live component review. Never engine state.
use crate::ui_kit::{StatusAction, StatusItem, StatusKind, StatusPresentation};
use serde_json::{Value, json};

#[derive(Default)]
pub(crate) struct Fixture {
    items: Vec<StatusItem>,
    presentation: StatusPresentation,
    details_readout: bool,
}

fn item(id: &str, kind: StatusKind, label: &str, setting: String, has_key: bool) -> StatusItem {
    StatusItem { id: id.into(), kind, name: crate::i18n::tr(label).into(), setting, has_key, editable: false }
}

fn specimen(id: &str, readout: bool) -> crate::ui_kit::Details {
    use crate::ui_kit::{DetailFinding, DetailKey, DetailSetting, DetailValue, Details};
    let t = crate::theme::Tokens::get();
    let tr = |id| crate::i18n::tr(id).to_owned();
    let mut description = Details { caption: Some(tr("ui.details_fixture_caption")), ..Default::default() };
    description.key = t
        .analysis
        .draft
        .into_iter()
        .zip(["≥ 1.0°", "−1.0° … 1.0°", "≤ −1.0°"])
        .enumerate()
        .map(|(index, (colour, label))| DetailKey {
            numeric: true,
            pattern_colour: None,
            colour: crate::theme::srgb(colour),
            pattern: index == 2,
            label: label.into(),
            value: String::new(),
        })
        .collect();
    if id != "details" {
        return description;
    }
    description.settings = vec![
        DetailSetting {
            id: "axis".into(),
            label: tr("ui.details_pull"),
            value: DetailValue::Axis { value: [0.0, 0.0, 1.0], current: "Z".into() },
            enabled: true,
        },
        DetailSetting { id: "angle".into(), label: tr("ui.details_angle"), value: DetailValue::Angle(1.0), enabled: true },
        DetailSetting {
            id: "slider".into(),
            label: tr("ui.details_stripes"),
            value: DetailValue::Slider { value: 14.5, min: 1.0, max: 200.0 },
            enabled: true,
        },
        DetailSetting { id: "check".into(), label: tr("ui.details_hatch"), value: DetailValue::Check(true), enabled: true },
    ];
    if readout {
        description.readout = (0..9).map(|index| (tr("ui.details_fixture_readout"), format!("{index}.00 mm"))).collect();
    } else {
        description.findings_title = tr("ui.details_fixture");
        description.findings = (0..9)
            .map(|index| DetailFinding {
                id: format!("specimen_{index}"),
                label: tr("ui.details_fixture_problem"),
                value: format!("{index}.00 mm"),
                warning: true,
                target: true,
            })
            .collect();
    }
    description.actions.push(("copy".into(), tr("ui.details_copy_list")));
    description
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
        "many" => vec![zebra(), section(), result(), attention()],
        "key" => vec![item("draft", StatusKind::Mode, "ui.draft", "Z · 1°".into(), true)],
        "details_findings" | "details_readout" => vec![item("details", StatusKind::Result, "ui.details_fixture", String::new(), true)],
        _ => return Err("unknown viewport status component specimen preset".into()),
    };
    app.viewport_status_fixture.items = items;
    app.viewport_status_fixture.details_readout = preset == "details_readout";
    Ok(json!({"preset": preset, "count": app.viewport_status_fixture.items.len()}))
}

pub(crate) fn show(app: &mut crate::SolveApp, ctx: &egui::Context, viewport: egui::Rect) {
    let fixture = &mut app.viewport_status_fixture;
    let folded = app.preferences.viewport_status_summary && fixture.items.len() >= 3;
    fixture.presentation.reconcile_summary(ctx, if folded { fixture.items.iter().map(|item| item.id.clone()).collect() } else { Vec::new() });
    if fixture.items.is_empty() {
        fixture.presentation.open_key = None;
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
    let readout = fixture.details_readout;
    let key_contents = |ui: &mut egui::Ui, id: &str| {
        let description = specimen(id, readout);
        crate::ui_kit::details(ui, id, &description, free.height() * 0.6, None);
    };
    // Deliberately use the oldest specimen as newest to exercise chronology/slot divergence.
    // This is explicit specimen metadata, never an engine activation claim.
    let newest = fixture.items.first().map(|item| item.id.as_str()).unwrap_or_default();
    let actions = if folded {
        crate::ui_kit::viewport_status_summary(ctx, free, &fixture.items, newest, &mut fixture.presentation, key_contents)
    } else {
        crate::ui_kit::viewport_status_row(ctx, free, &fixture.items, &mut fixture.presentation, key_contents)
    };
    for action in actions {
        match action {
            StatusAction::Clear(id) => fixture.items.retain(|item| item.id != id),
            StatusAction::ClearAll => fixture.items.clear(),
            StatusAction::Edit(_) => {}
        }
    }
}
