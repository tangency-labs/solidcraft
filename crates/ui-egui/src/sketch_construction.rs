//! Interactive construction-line family: reference picks, shared analytic previews and commits.

use egui::Stroke;
use serde_json::{Value, json};
use solidcraft_engine::cmd::{construction_line_choice, construction_line_mode, construction_line_solutions};
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

pub fn hint(id: &str) -> String {
    let key = match construction_line_mode(id).unwrap_or("free") {
        "free" => "ui.construction.free_hint",
        "horizontal" | "vertical" => "ui.construction.point_hint",
        "angle" => "ui.construction.angle_hint",
        "parallel" | "perpendicular" => "ui.construction.reference_hint",
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
        "tangent_external" => first.unwrap_or(pointer),
        _ => pointer,
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
            let state = crate::open_meshing::session_world(&app.session);
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
    if matches!(mode, "free" | "angle" | "tangent_external") && t.pts.is_empty() {
        t.pts.push(at);
        return;
    }
    let Some(p) = params(app, t, &at, false) else { return };
    if app.run(t.cmd, p).is_ok() {
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
    let state = crate::open_meshing::session_world(&app.session);
    let Some(ss) = app.session.active_sketch.and_then(|sid| state.sketch(sid)) else { return };
    let Ok(lines) = construction_line_solutions(&ss.sketch, &p, mode) else { return };
    let chosen = construction_line_choice(&lines, &p).ok();
    let tokens = crate::theme::Tokens::get();
    let center = ss.plane.to_local(app.cam.target);
    let extent = app.cam.half_height().max(1.0) * 20.0;
    for (i, line) in lines.iter().enumerate() {
        let origin = line.start + line.direction * (center - line.start).dot(line.direction);
        let points = [ss.plane.to_world(origin - line.direction * extent), ss.plane.to_world(origin + line.direction * extent)];
        let selected = chosen == Some(i);
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
