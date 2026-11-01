//! Current effect data -> the one generic details grammar; edits use normal commands.
use crate::{
    SolveApp,
    ui_kit::{DetailEvent, DetailFinding, DetailKey, DetailSetting, DetailValue, Details},
};
use serde_json::{Value, json};
use solidcraft_engine::{
    status_details::ContinuityDetail,
    status_items::{StatusItem, StatusSetting},
};

fn tr(id: &str) -> String {
    crate::i18n::tr(id).to_owned()
}
fn field(id: &str, label: &str, value: DetailValue) -> DetailSetting {
    DetailSetting { id: id.into(), label: tr(label), value, enabled: true }
}
fn length(app: &SolveApp, value: f64) -> DetailValue {
    DetailValue::Length { value, units: app.session.doc.units.clone() }
}
fn number(value: f64, min: f64, max: f64, suffix: &str) -> DetailValue {
    DetailValue::Number { value, min, max, suffix: suffix.into() }
}
fn axis(value: [f64; 3]) -> DetailValue {
    let axes = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    let current = if axes.iter().any(|axis| *axis == value || axis.map(|v| -v) == value) {
        crate::viewport_status::direction(value)
    } else {
        let [x, y, z] = value;
        format!("({x}, {y}, {z})")
    };
    DetailValue::Axis { value, current }
}
fn record_id(row: &ContinuityDetail) -> String {
    json!([row.body, row.edge]).to_string()
}
fn record_label(row: &ContinuityDetail) -> String {
    crate::i18n::format("ui.details_seam", &[("body", &row.body), ("edge", &row.edge.to_string())])
}
fn selected<'a>(app: &SolveApp, rows: &'a [ContinuityDetail]) -> Option<&'a ContinuityDetail> {
    app.viewport_status.selected_record.as_ref().and_then(|id| rows.iter().find(|row| record_id(row) == *id)).or_else(|| rows.first())
}

pub(crate) fn primary(effect: &StatusItem) -> &'static str {
    match effect.setting {
        StatusSetting::Draft { .. } => "angle",
        StatusSetting::Zebra { .. } => "stripes",
        StatusSetting::Accessibility { .. } => "direction",
        StatusSetting::Curvature { .. } => "radius",
        StatusSetting::Section { .. } => "offset",
        StatusSetting::Continuity { .. } => "record",
        StatusSetting::Environment => "",
    }
}

pub(crate) fn description(app: &SolveApp, effect: &StatusItem) -> Details {
    let mut details = Details::default();
    match effect.setting {
        StatusSetting::Draft { pull, angle_deg } => {
            details.caption = Some(tr("ui.details_draft_caption"));
            details.settings = vec![field("pull", "ui.details_pull", axis(pull)), field("angle", "ui.details_angle", DetailValue::Angle(angle_deg))];
        }
        StatusSetting::Zebra { stripes } => {
            details.caption = Some(tr("ui.details_zebra_caption"));
            details.settings.push(field("stripes", "ui.details_stripes", DetailValue::Slider { value: stripes, min: 1.0, max: 200.0 }));
        }
        StatusSetting::Accessibility { direction } => {
            details.caption = Some(tr("ui.details_access_caption"));
            details.settings.push(field("direction", "ui.details_direction", axis(direction)));
        }
        StatusSetting::Curvature { radius_mm } => {
            details.caption = Some(tr("ui.details_curvature_caption"));
            details.settings.push(field("radius", "ui.details_radius", length(app, radius_mm)));
        }
        StatusSetting::Section { origin_mm, normal, hatch } => {
            if let Some((_, offset, n)) = crate::viewport_status::section_coordinates(origin_mm, normal) {
                let flip = normal.iter().zip(n).map(|(a, b)| a * b).sum::<f64>() < 0.0;
                details.settings = vec![
                    field("plane", "ui.details_plane", axis(n)),
                    field("offset", "ui.details_offset", length(app, offset)),
                    field("flip", "ui.details_flip", DetailValue::Check(flip)),
                    field("hatch", "ui.details_hatch", DetailValue::Check(hatch)),
                ];
            }
        }
        StatusSetting::Continuity { .. } => continuity_description(app, &mut details),
        StatusSetting::Environment => {}
    }
    if effect.command != "inspect.continuity"
        && effect.has_key
        && let Some((_, _, rows)) = crate::viewport::analysis_legend_data(app)
    {
        details.key = rows
            .into_iter()
            .enumerate()
            .map(|(index, (colour, label, pattern))| DetailKey {
                colour,
                numeric: label.chars().any(|c| c.is_ascii_digit()),
                label,
                pattern: pattern || (matches!(effect.setting, StatusSetting::Draft { .. }) && index == 2),
                value: String::new(),
                pattern_colour: None,
            })
            .collect();
    }
    details
}

fn continuity_description(app: &SolveApp, details: &mut Details) {
    use solidcraft_engine::status_details::ContinuityVerdict as V;
    let rows = app.session.continuity_details();
    let Some(row) = selected(app, &rows) else { return };
    details.caption = Some(tr("ui.details_continuity_caption"));
    let index = rows.iter().position(|other| record_id(other) == record_id(row)).unwrap_or(0);
    let (scale, unit) = crate::prefs::unit_scale(&app.session.doc.units);
    details.settings = vec![
        field("record", "ui.details_edit_seam", DetailValue::Choice { selected: index, options: rows.iter().map(record_label).collect() }),
        field("gap", "ui.details_gap", length(app, row.gap_tolerance_mm)),
        field("angle", "ui.details_angle", DetailValue::Angle(row.angle_tolerance_deg)),
        field("curvature", "ui.details_curvature", number(row.curvature_tolerance_per_mm / scale, 0.0, 1e9 / scale, &format!(" /{unit}"))),
        field("samples", "ui.details_samples", number(row.samples as f64, 3.0, 257.0, "")),
    ];
    for setting in details.settings.iter_mut().skip(1) {
        setting.enabled = row.current;
    }
    if !row.current {
        details.caption = Some(tr("ui.details_continuity_stale"));
    }
    let t = crate::theme::Tokens::get();
    for (label, colour, count) in [
        ("ui.details_smooth", t.continuity[0], rows.iter().filter(|s| s.verdict == V::Smooth).count()),
        ("ui.details_jump", t.continuity[1], rows.iter().filter(|s| s.verdict == V::CurvatureJump).count()),
        ("ui.details_sharp_gap", t.continuity[2], rows.iter().filter(|s| matches!(s.verdict, V::Sharp | V::Gap)).count()),
    ] {
        details.key.push(DetailKey {
            colour,
            pattern: label == "ui.details_sharp_gap",
            label: tr(label),
            value: count.to_string(),
            numeric: false,
            pattern_colour: (label == "ui.details_sharp_gap").then_some(t.continuity[3]),
        });
    }
    details.findings_title = tr("ui.details_worst_seams");
    details.findings_count = Some(rows.len().to_string());
    let mut findings: Vec<_> = rows.iter().collect();
    findings.sort_by(|a, b| {
        b.verdict.cmp(&a.verdict).then_with(|| b.gap_mm.total_cmp(&a.gap_mm)).then_with(|| b.normal_angle_deg.total_cmp(&a.normal_angle_deg))
    });
    details.findings = findings
        .into_iter()
        .map(|row| {
            let value = match row.verdict {
                V::Gap => format!("G0 {}", crate::prefs::show_mm(app, row.gap_mm, 1)),
                V::Sharp => format!("G1 {}°", row.normal_angle_deg),
                V::CurvatureJump => format!("G2 {} /{unit}", row.curvature_jump_per_mm / scale),
                V::Smooth => format!("G2 {} /{unit}", row.curvature_jump_per_mm / scale),
            };
            DetailFinding { id: record_id(row), label: record_label(row), value, warning: row.verdict != V::Smooth, target: row.current }
        })
        .collect();
    details.actions.push(("copy".into(), tr("ui.details_copy_list")));
}

/// Rebuild a complete command from current Session values for every single-field edit.
/// The value passed by the widget changes exactly one companion of that current command.
pub(crate) fn edit_params(app: &SolveApp, effect: &StatusItem, id: &str, value: &DetailValue) -> Option<Value> {
    match effect.setting {
        StatusSetting::Draft { pull, angle_deg } => {
            Some(json!({"pull":if id=="pull" { axis_value(value)? } else {pull},"angle":if id=="angle"{scalar(value)?}else{angle_deg}}))
        }
        StatusSetting::Zebra { stripes } => Some(json!({"stripes":if id=="stripes"{scalar(value)?}else{stripes}})),
        StatusSetting::Accessibility { direction } => Some(json!({"direction":if id=="direction"{axis_value(value)?}else{direction}})),
        StatusSetting::Curvature { radius_mm } => Some(json!({"radius":if id=="radius"{scalar(value)?}else{radius_mm}})),
        StatusSetting::Section { origin_mm, normal, hatch } => {
            let (_, old_offset, n) = crate::viewport_status::section_coordinates(origin_mm, normal)?;
            let mut base = origin_mm.into_iter().zip(n).map(|(o, n)| o - n * old_offset).collect::<Vec<_>>();
            let new_n = if id == "plane" { axis_value(value)? } else { n };
            if id == "plane" {
                // Project the base onto the new plane, preserving the signed offset companion.
                let projection = base.iter().zip(new_n).map(|(o, n)| o * n).sum::<f64>();
                base = base.into_iter().zip(new_n).map(|(o, n)| o - n * projection).collect();
            }
            let flip = normal.iter().zip(n).map(|(a, b)| a * b).sum::<f64>() < 0.0;
            // `replace`: the card edits the section shown in place; without it every field change
            // would add a new Section Analysis to the design (#3977).
            Some(json!({"plane":{"origin":base,"normal":new_n},"offset":format!("{} mm",if id=="offset"{scalar(value)?}else{old_offset}),
                "flip":if id=="flip"{check(value)?}else{flip},"hatch":if id=="hatch"{check(value)?}else{hatch},"replace":true}))
        }
        StatusSetting::Continuity { .. } => {
            let rows = app.session.continuity_details();
            let row = selected(app, &rows).filter(|row| row.current)?;
            let scale = crate::prefs::unit_scale(&app.session.doc.units).0;
            let samples = if id == "samples" { scalar(value)? } else { row.samples as f64 };
            Some(json!({"body":row.body,"edges":[{"index":row.edge}],"add":true,"samples":samples,
                "gap_tolerance_mm":if id=="gap"{scalar(value)?}else{row.gap_tolerance_mm},
                "angle_tolerance_deg":if id=="angle"{scalar(value)?}else{row.angle_tolerance_deg},
                "curvature_tolerance_per_mm":if id=="curvature"{scalar(value)?*scale}else{row.curvature_tolerance_per_mm}}))
        }
        StatusSetting::Environment => None,
    }
}
fn scalar(value: &DetailValue) -> Option<f64> {
    match value {
        DetailValue::Number { value, .. } | DetailValue::Length { value, .. } | DetailValue::Slider { value, .. } | DetailValue::Angle(value) => {
            Some(*value)
        }
        _ => None,
    }
}
fn check(value: &DetailValue) -> Option<bool> {
    if let DetailValue::Check(value) = value { Some(*value) } else { None }
}
fn axis_value(value: &DetailValue) -> Option<[f64; 3]> {
    if let DetailValue::Axis { value, .. } = value { Some(*value) } else { None }
}

pub(crate) fn dispatch(app: &mut SolveApp, ctx: &egui::Context, owner: &str, event: DetailEvent) {
    let Some(effect) = app.session.status_items().into_iter().find(|effect| crate::viewport_status::identity(effect) == owner) else { return };
    match event {
        DetailEvent::Change(id, DetailValue::Choice { selected, .. }) if id == "record" && effect.command == "inspect.continuity" => {
            app.viewport_status.selected_record = app.session.continuity_details().get(selected).map(record_id);
        }
        DetailEvent::Change(id, value) => {
            if let Some(params) = edit_params(app, &effect, &id, &value)
                && let Err(error) = app.run(&effect.command, params)
            {
                app.set_status(error, true);
            }
        }
        DetailEvent::Target(id) if effect.command == "inspect.continuity" => {
            let Some((body, edge)) = serde_json::from_str::<(String, usize)>(&id).ok() else { return };
            let Some(points) = app.session.continuity_target_points(&body, edge) else { return };
            let mut bounds = solidcraft_engine::geom::Aabb3::EMPTY;
            for point in points {
                bounds.add(*point);
            }
            if bounds.is_empty() {
                return;
            }
            let mut camera = app.cam;
            camera.fit(&bounds);
            let params = json!({"camera":{"target":camera.target,"yaw":camera.yaw,"pitch":camera.pitch,"distance":camera.distance,"fov":camera.fov,"roll":camera.roll}});
            if let Err(error) = app.run("view.apply", params) {
                app.set_status(error, true);
            }
        }
        DetailEvent::Action(id) if id == "copy" && effect.command == "inspect.continuity" => {
            let rows = app.session.continuity_details();
            if let Ok(text) = serde_json::to_string_pretty(&rows) {
                ctx.copy_text(text.clone());
                app.clipboard_text = Some(text);
            }
        }
        DetailEvent::Action(_) | DetailEvent::Target(_) => {}
    }
}

pub(crate) fn legacy_description(app: &SolveApp) -> Details {
    let mut details = Details::default();
    if let Some((title, along, rows)) = crate::viewport::analysis_legend_data(app) {
        details.caption = Some(along.map_or_else(|| title.to_owned(), |along| format!("{title}\n{along}")));
        details.key = rows
            .into_iter()
            .map(|(colour, label, pattern)| DetailKey {
                colour,
                numeric: label.chars().any(|c| c.is_ascii_digit()),
                label,
                pattern,
                value: String::new(),
                pattern_colour: None,
            })
            .collect();
    }
    details
}

/// Compatibility placement uses the same description and renderer, never a second painter.
pub(crate) fn legacy_legends(app: &SolveApp, ui: &mut egui::Ui, viewport: egui::Rect) {
    let mut descriptions = Vec::new();
    if !app.preferences.viewport_analysis_status {
        descriptions.push(("analysis:legend", legacy_description(app)));
    }
    if (!app.preferences.viewport_analysis_status || !app.preferences.viewport_status_details) && !app.session.continuity.is_empty() {
        let t = crate::theme::Tokens::get();
        let key = ["ui.continuity_g2", "ui.continuity_g1", "ui.continuity_g0", "ui.continuity_gap"]
            .into_iter()
            .zip(t.continuity)
            .enumerate()
            .map(|(index, (label, colour))| DetailKey {
                colour,
                label: tr(label),
                pattern: index == 3,
                value: String::new(),
                numeric: false,
                pattern_colour: None,
            })
            .collect();
        descriptions.push(("continuity_legend", Details { key, ..Default::default() }));
    }
    let mut free = crate::shell::workspace_content(app, viewport);
    for (id, description) in descriptions {
        if let Some(rect) = legacy_legend(app, ui, free, id, &description) {
            free.max.y = (rect.top() - crate::theme::Tokens::get().space_8).max(free.top());
        }
    }
}

fn legacy_legend(_app: &SolveApp, ui: &mut egui::Ui, free: egui::Rect, id: &str, description: &Details) -> Option<egui::Rect> {
    if description.key.is_empty() {
        return None;
    }
    let t = crate::theme::Tokens::get();
    let container = crate::ui_kit::Container::new(ui.ctx(), crate::ui_kit::Surface::ViewportStatus);
    let result = container
        .area(egui::Id::new(id))
        .order(egui::Order::Background)
        .movable(false)
        .pivot(egui::Align2::LEFT_BOTTOM)
        .fixed_pos(free.left_bottom() + egui::vec2(t.space_12, -t.space_12))
        .show(ui.ctx(), |ui| {
            let frame = container.frame(ui.ctx(), t.overlay, crate::frame::Side::Left).inner_margin(t.space_8);
            container.show_frame(ui, frame, free, |ui| {
                crate::ui_kit::details(ui, id, description, free.height() * 0.6, None);
            });
        });
    crate::scenario::publish_target(id, result.response.rect);
    Some(result.response.rect)
}
