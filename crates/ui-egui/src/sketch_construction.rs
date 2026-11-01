//! Interactive construction-line family: reference picks, shared analytic previews and commits.

use egui::Stroke;
use serde_json::{Value, json};
use solidcraft_engine::cmd::{construction_line_choice, construction_line_mode, construction_line_solutions, construction_segment};
use solidcraft_engine::geom::Vec2;

use crate::SolveApp;
use crate::tools::Tool;
use crate::viewport::{Hit, Proj};

fn references(mode: &str) -> usize {
    match mode {
        "tangent_two" | "bisector" => 2,
        "parallel" | "perpendicular" | "tangent_external" | "tangent_at" => 1,
        _ => 0,
    }
}

/// The one-click methods whose Length box shows from the start (#10258). Without a length they
/// still draw an infinite line in one click; with one, a second click picks the side it runs to.
pub(crate) fn length_box_at_start(mode: &str) -> bool {
    matches!(mode, "horizontal" | "vertical" | "parallel" | "perpendicular")
}

/// The typed length, or None while the box is empty: an empty box keeps the infinite line.
pub(crate) fn typed_length(t: &Tool) -> Option<&str> {
    crate::sketch_dims::locked_expression(t, "Length").map(str::trim).filter(|text| !text.is_empty())
}

/// A digit typed while a one-click tool's Length box waits gives that box the keyboard, so the
/// digit is its first character. Anything else (a tool's shortcut letter) still reaches the
/// shortcuts: the box only takes the keyboard once a number is being typed.
pub fn claim_typed_length(app: &SolveApp, ctx: &egui::Context) {
    let Some(t) = app.tool.as_ref() else { return };
    if !construction_line_mode(t.cmd).is_some_and(length_box_at_start) || !t.pts.is_empty() || t.hover.is_none() {
        return;
    }
    if t.dims.first().is_none_or(|b| b.label != "Length") || ctx.memory(|m| m.focused().is_some()) {
        return;
    }
    let digit = ctx.input(|i| {
        !i.modifiers.command
            && !i.modifiers.alt
            && i.events.iter().any(|e| matches!(e, egui::Event::Text(text) if text.chars().next().is_some_and(|c| c.is_ascii_digit())))
    });
    if digit {
        ctx.memory_mut(|m| m.request_focus(crate::sketch_dims::box_id(0)));
    }
}

/// The Construction/Normal switch under a construction line tool's Length box (#10258). Normal
/// makes the segment an ordinary sketch line, ready to close a profile, without converting it
/// afterwards. It only works with a length: an infinite line stays construction, so the switch
/// is greyed out and says why until a length is typed. It belongs to the tool alone and leaves
/// the Sketch Palette's drawing line type (X) as it is.
pub fn line_type_switch(ui: &mut egui::Ui, t: &mut Tool) {
    if construction_line_mode(t.cmd).is_none() {
        return;
    }
    let Some(i) = t.dims.iter().position(|b| b.label == "Length") else { return };
    let Some(length_box) = ui.ctx().memory(|m| m.area_rect(egui::Id::new(("sc_dim_area", i)))) else { return };
    let tokens = crate::theme::Tokens::get();
    let has_length = typed_length(t).is_some();
    let reason = crate::i18n::tr("ui.construction.normal_needs_length");
    let refused = [Some(reason), Some(reason)];
    // The Sketch Palette's own line type names, which `segmented` translates.
    let options = ["Construction", "Normal"];
    let mut selected = usize::from(has_length && t.normal);
    let frame = crate::ui_kit::Container::popup_frame(ui.style()).inner_margin(egui::Margin::same(1)).fill(tokens.panel);
    crate::ui_kit::Container::new(ui.ctx(), crate::ui_kit::Surface::Canvas)
        .area(egui::Id::new("sc_construction_switch"))
        .fixed_pos(length_box.left_bottom() + egui::vec2(0.0, tokens.space_2))
        .order(egui::Order::Background)
        .show(ui.ctx(), |ui| {
            frame.show(ui, |ui| {
                let segments = crate::ui_kit::Segments {
                    disabled_reasons: if has_length { &[] } else { &refused },
                    targets: &["construction_switch:construction", "construction_switch:normal"],
                    small: true,
                    intrinsic: true,
                    compact: true,
                    ..Default::default()
                };
                if crate::ui_kit::segmented(ui, &mut selected, &options, segments) {
                    t.normal = selected == 1;
                }
            });
        });
}

pub fn hint(id: &str) -> String {
    let key = match construction_line_mode(id).unwrap_or("free") {
        "free" => "ui.construction.free_hint",
        "horizontal" | "vertical" => "ui.construction.axis_hint",
        "angle" => "ui.construction.angle_hint",
        "parallel" => "ui.construction.parallel_hint",
        "perpendicular" => "ui.construction.reference_hint",
        "tangent_external" => "ui.construction.external_hint",
        "tangent_at" => "ui.construction.contact_hint",
        "tangent_two" => "ui.construction.common_hint",
        "bisector" => "ui.construction.bisector_hint",
        _ => "ui.construction.point_hint",
    };
    crate::i18n::tr(key).to_owned()
}

fn arg(point: &(Vec2, Option<String>), preview: bool) -> Value {
    if !preview && let Some(id) = &point.1 {
        return json!(id);
    }
    json!([point.0.x, point.0.y])
}

/// Parameters for the line under the pointer, also used for the final click.
fn params(app: &SolveApp, t: &Tool, pointer: &(Vec2, Option<String>), preview: bool) -> Option<Value> {
    let mode = construction_line_mode(t.cmd)?;
    let refs = references(mode);
    if t.picks.len() < refs {
        return None;
    }
    let first = t.pts.first();
    let at = match mode {
        "free" | "angle" => first?,
        // A line with a length starts where the first click was; the pointer picks its side.
        _ => first.unwrap_or(pointer),
    };
    let mut p = json!({"mode": mode, "at": arg(at, preview), "near": [pointer.0.x, pointer.0.y]});
    if let Some(id) = t.picks.first() {
        p["curve"] = json!(id);
    }
    if let Some(id) = t.picks.get(1) {
        p["curve2"] = json!(id);
    }
    if mode == "free" {
        p["through"] = arg(pointer, preview);
    }
    if mode == "angle" {
        let angle = crate::sketch_dims::locked_value(app, t, "Angle").unwrap_or_else(|| (pointer.0 - at.0).angle());
        p["angle"] = json!(angle.to_degrees());
        if !preview && let Some(expression) = crate::sketch_dims::locked_expression(t, "Angle") {
            p["angle"] = json!(expression);
        }
    }
    if matches!(mode, "parallel" | "horizontal" | "vertical") {
        if let Some(offset) = crate::sketch_dims::locked_value(app, t, "Offset") {
            p["offset"] = json!(offset);
        }
        if !preview && let Some(expression) = crate::sketch_dims::locked_expression(t, "Offset") {
            p["offset"] = json!(expression);
        }
    }
    if let Some(expression) = typed_length(t) {
        p["length"] = if preview { json!(crate::sketch_dims::locked_value(app, t, "Length")?) } else { json!(expression) };
        // An infinite line is always construction, so the switch only counts with a length.
        if t.normal {
            p["construction"] = json!(false);
        }
    }
    Some(p)
}

pub fn on_click(app: &mut SolveApp, tool: &mut Tool, hits: &[Hit], at: Option<(Vec2, Option<String>)>) {
    let Some(mode) = construction_line_mode(tool.cmd) else { return };
    let refs = references(mode);
    if tool.picks.len() < refs {
        let active = app.session.active_sketch;
        let picked = hits.iter().find_map(|h| match h {
            Hit::SketchCurve { sketch, id, .. } if Some(*sketch) == active => Some(id.clone()),
            _ => None,
        });
        if let Some(id) = picked {
            let state = app.session.world_state();
            let valid = active.and_then(|sid| state.sketch(sid)).is_some_and(|ss| {
                ss.sketch.curve_index(&id).and_then(|i| ss.sketch.curves.get(i)).is_some_and(|c| {
                    use solidcraft_engine::sketch::CurveKind;
                    match mode {
                        "parallel" | "perpendicular" | "bisector" => matches!(c.kind, CurveKind::Line { .. }),
                        "tangent_external" | "tangent_two" => matches!(c.kind, CurveKind::Circle { .. } | CurveKind::Arc { .. }),
                        "tangent_at" => !matches!(c.kind, CurveKind::Line { .. } | CurveKind::Ellipse { .. }),
                        _ => false,
                    }
                })
            });
            if valid && !tool.picks.contains(&id) {
                tool.picks.push(id);
            }
        }
        return;
    }
    if let Some(at) = at {
        place_point(app, tool, at);
    }
}

/// Enter in the angle box follows the same path as a click.
pub fn place_point(app: &mut SolveApp, t: &mut Tool, at: (Vec2, Option<String>)) {
    let Some(mode) = construction_line_mode(t.cmd) else { return };
    if t.picks.len() < references(mode) {
        return;
    }
    if let Err(error) = crate::sketch_dims::validate(app, t) {
        app.set_status(error, true);
        return;
    }
    let at = crate::sketch_dims::placed(app, t, &at);
    let picks_side = length_box_at_start(mode) && typed_length(t).is_some();
    if (matches!(mode, "free" | "angle" | "tangent_external") || picks_side) && t.pts.is_empty() {
        t.pts.push(at);
        if picks_side {
            app.set_status(crate::i18n::tr("ui.construction.side_hint").to_owned(), false);
        }
        return;
    }
    // A click back on the start names no side: wait for one.
    if picks_side && t.pts.first().is_some_and(|start| start.0.dist(at.0) < 1e-9) {
        return;
    }
    let Some(p) = params(app, t, &at, false) else { return };
    if app.run(t.cmd, p).is_ok() {
        if !t.pts.is_empty() && length_box_at_start(mode) {
            app.set_status(hint(t.cmd), false);
        }
        t.pts.clear();
        // Retain references for repeated parallel lines/tangents. Right-click resets the picks.
        crate::sketch_dims::reset(t);
    }
}

/// Shared geometry, drawn far enough either way to cover the visible sketch plane.
pub fn preview(app: &SolveApp, t: &Tool, painter: &egui::Painter, proj: &Proj) {
    let Some(pointer) = &t.hover else { return };
    let Some(mode) = construction_line_mode(t.cmd) else { return };
    let Some(p) = params(app, t, pointer, true) else { return };
    let state = app.session.world_state();
    let Some(ss) = app.session.active_sketch.and_then(|sid| state.sketch(sid)) else { return };
    let Ok(lines) = construction_line_solutions(&ss.sketch, &p, mode) else { return };
    let chosen = construction_line_choice(&lines, &p).ok();
    let tokens = crate::theme::Tokens::get();
    let center = ss.plane.to_local(app.cam.target);
    let extent = app.cam.half_height().max(1.0) * 20.0;
    let length = p.get("length").and_then(Value::as_f64);
    for (i, line) in lines.iter().enumerate() {
        let selected = chosen == Some(i);
        // A typed length previews the segment the click will create, on the pointer's side.
        let points = match length {
            Some(length) if selected => {
                let (a, b) = construction_segment(*line, length, Some(pointer.0));
                [ss.plane.to_world(a), ss.plane.to_world(b)]
            }
            _ => {
                let origin = line.start + line.direction * (center - line.start).dot(line.direction);
                [ss.plane.to_world(origin - line.direction * extent), ss.plane.to_world(origin + line.direction * extent)]
            }
        };
        let stroke =
            Stroke::new(if selected { 2.0 } else { 1.0 }, if selected { tokens.preview_new } else { tokens.sketch_accent.gamma_multiply(0.35) });
        crate::tools::paint_sketch_path(&points, 3, painter, proj, stroke);
    }
    if let Some(first) = t.pts.first()
        && let Some(p) = proj.to_screen(ss.plane.to_world(first.0))
    {
        painter.circle_stroke(p, 4.0, Stroke::new(1.5, tokens.sketch_accent));
    }
}

#[cfg(test)]
#[path = "sketch_construction_tests.rs"]
mod tests;
