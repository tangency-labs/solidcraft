//! Interactive construction-line family: reference picks, shared analytic previews and commits.

use egui::Stroke;
use serde_json::{Value, json};
use solidcore_record::Vec2;
use solidcraft_engine::cmd::{construction_line_choice, construction_line_mode, construction_line_solutions, construction_segment};
use solidcraft_engine::geom::prelude::*;

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

/// The one-click methods (#10258): an infinite line takes one click; a segment takes a second,
/// which picks the side it runs to. Before the first click they show the pointer's coordinates,
/// as the Line tool does, and the Offset box where they have one (#15618).
pub(crate) fn length_box_at_start(mode: &str) -> bool {
    matches!(mode, "horizontal" | "vertical" | "parallel" | "perpendicular")
}

/// The typed length, or None while the box is empty: an empty box keeps the infinite line.
pub(crate) fn typed_length(t: &Tool) -> Option<&str> {
    crate::sketch_dims::locked_expression(t, "Length").map(str::trim).filter(|text| !text.is_empty())
}

/// A digit typed before a one-click tool's first click gives its first value box the keyboard, so
/// the digit is its first character: a coordinate of a horizontal or vertical line, the distance
/// of a parallel one (#15618). The length is typed after the first click, as the Line tool's is.
/// A letter still reaches the shortcuts.
pub fn claim_typed_length(app: &SolveApp, ctx: &egui::Context) {
    let Some(t) = app.tool.as_ref() else { return };
    if !construction_line_mode(t.cmd).is_some_and(length_box_at_start) || !t.pts.is_empty() || t.hover.is_none() {
        return;
    }
    if ctx.memory(|m| m.focused().is_some()) {
        return;
    }
    // The first box: X, Y order for a horizontal or vertical line, the distance for a parallel one.
    // The classic tools (`construction_line_classic`) give it to the Length box, as before.
    let index = if t.classic { t.dims.iter().position(|b| b.label == "Length") } else { (!t.dims.is_empty()).then_some(0) };
    let Some(index) = index else { return };
    let digit = ctx.input(|i| {
        !i.modifiers.command
            && !i.modifiers.alt
            && i.events.iter().any(|e| matches!(e, egui::Event::Text(text) if text.chars().next().is_some_and(|c| c.is_ascii_digit())))
    });
    if digit {
        ctx.memory_mut(|m| m.request_focus(crate::sketch_dims::box_id(index)));
    }
}

/// The construction line methods that take a length, and so can draw a Normal segment (#10258).
pub(crate) fn takes_length(mode: &str) -> bool {
    matches!(mode, "horizontal" | "vertical" | "angle" | "parallel" | "perpendicular")
}

/// Whether the tool draws a segment rather than an infinite line: a line type other than
/// Infinite was chosen, or a length was typed. With the Construction/Normal switch
/// (`construction_line_classic`), only a typed length makes one, as before #15066.
pub(crate) fn segment(t: &Tool) -> bool {
    typed_length(t).is_some() || (t.line_type != 3 && !t.classic)
}

/// The Construction/Normal switch under a construction line tool's Length box (#10258), shown
/// with the `construction_line_classic` preference in place of the Sketch Palette's Line Type
/// icons (#15066). Normal makes the segment an ordinary sketch line, ready to close a profile,
/// without converting it afterwards. It only works with a length: an infinite line stays
/// construction, so the switch is greyed out and says why until a length is typed. It belongs to
/// the tool alone and leaves the Sketch Palette's drawing line type (X) as it is.
pub fn line_type_switch(ui: &mut egui::Ui, t: &mut Tool) {
    if construction_line_mode(t.cmd).is_none() {
        return;
    }
    let Some(i) = t.dims.iter().position(|b| b.label == "Length") else { return };
    let Some(length_box) = ui.ctx().memory(|m| m.area_rect(egui::Id::unique(("sc_dim_area", i)))) else { return };
    let tokens = solidcraft_theme::Tokens::get();
    let has_length = typed_length(t).is_some();
    let reason = solidcraft_ui_text::tr("ui.construction.normal_needs_length");
    let refused = [Some(reason), Some(reason)];
    // The Sketch Palette's own line type names, which `segmented` translates.
    let options = ["Construction", "Normal"];
    let mut selected = usize::from(has_length && t.line_type == 0);
    let frame = crate::ui_kit::Container::popup_frame(ui.style()).inner_margin(egui::Margin::same(1)).fill(tokens.panel);
    crate::ui_kit::Container::new(ui.ctx(), crate::ui_kit::Surface::Canvas)
        .area(egui::Id::unique("sc_construction_switch"))
        .fixed_pos(length_box.left_bottom() + egui::vec2(0.0, tokens.space_2))
        .order(egui::Order::Background)
        .show(ui.ctx(), |ui| {
            crate::ui_kit::Container::show_backing(ui, frame, |ui| {
                let segments = crate::ui_kit::Segments {
                    disabled_reasons: if has_length { &[] } else { &refused },
                    targets: &["construction_switch:construction", "construction_switch:normal"],
                    small: true,
                    intrinsic: true,
                    compact: true,
                    ..Default::default()
                };
                if crate::ui_kit::segmented(ui, &mut selected, &options, segments) {
                    // Normal, or Infinite: a construction segment while a length is typed.
                    t.line_type = if selected == 1 { 0 } else { 3 };
                }
            });
        });
}

/// The Sketch Palette's Line Type icon that a construction line tool taking a length lights
/// (#15066): the type chosen, or with Infinite left on, Construction once a length is typed, as
/// that makes a construction segment.
pub(crate) fn shown_line_type(t: &Tool) -> u8 {
    if t.line_type == 3 && typed_length(t).is_some() { 1 } else { t.line_type }
}

thread_local! {
    /// The line type last chosen in the Sketch Palette for a construction line tool: the next one
    /// started, another method included, begins at it (#15618).
    static LAST_LINE_TYPE: std::cell::Cell<u8> = const { std::cell::Cell::new(3) };
}

/// The line type a construction line tool starts at: the one last chosen, else Infinite.
pub(crate) fn last_line_type() -> u8 {
    LAST_LINE_TYPE.with(std::cell::Cell::get)
}

/// A click on the Sketch Palette's Line Type icon `kind` (0 normal, 1 construction,
/// 2 centerline, 3 infinite) while a construction line tool taking a length runs (#15066). The
/// icons stay in the palette, so they hold still while the pointer moves; beside the Length box
/// a switch followed the pointer away from the click. Infinite clears the typed length, so one
/// click draws an infinite line. The others draw a segment of that type, as the Line tool does:
/// a first click starts it and a second ends it, at the typed length or where the pointer is.
/// The palette's drawing line type (X) is left as it is.
pub(crate) fn choose_line_type(ctx: &egui::Context, t: &mut Tool, kind: u8) {
    t.line_type = kind;
    LAST_LINE_TYPE.with(|last| last.set(kind));
    if kind == 3 {
        // A segment's start is no part of an infinite line: the next click places that line.
        // The angle method's first click is where its line passes, so it keeps it.
        if construction_line_mode(t.cmd).is_some_and(length_box_at_start) {
            t.pts.clear();
        }
        t.dim_history.retain(|b| b.label != "Length");
        if let Some(i) = t.dims.iter().position(|b| b.label == "Length") {
            t.dims[i].locked = false;
            t.dims[i].value.clear();
            ctx.memory_mut(|m| m.surrender_focus(crate::sketch_dims::box_id(i)));
        }
    }
}

/// The length of the segment ending at `pointer` that the tool would draw now, when it draws one
/// with no typed length: what its Length box shows. Measured to where a click there puts the end,
/// as a typed angle moves it off the pointer.
pub(crate) fn pointer_segment_length(app: &SolveApp, t: &Tool, pointer: &(Vec2, Option<String>)) -> Option<f64> {
    if typed_length(t).is_some() {
        return None;
    }
    params(app, t, &crate::sketch_dims::placed(app, t, pointer), true)?.get("length")?.as_f64()
}

/// Where the segment being drawn ends with the pointer at `pointer`: at the typed length on the
/// pointer's side, or where the pointer is.
pub(crate) fn segment_end(app: &SolveApp, t: &Tool, pointer: Vec2) -> Option<Vec2> {
    let mode = construction_line_mode(t.cmd)?;
    let p = params(app, t, &(pointer, None), true)?;
    let length = p.get("length")?.as_f64()?;
    let state = crate::open_meshing::session_world(&app.session);
    let ss = app.session.active_sketch.and_then(|sid| state.sketch(sid))?;
    let lines = construction_line_solutions(&ss.sketch, &p, mode).ok()?;
    let line = *lines.get(construction_line_choice(&lines, &p).ok()?)?;
    Some(construction_segment(line, length, Some(pointer)).1)
}

/// How far along the line the pointer is from its start: the length of a segment with no typed
/// length, which ends where the pointer is.
fn pointer_length(app: &SolveApp, p: &Value, mode: &str, pointer: Vec2) -> Option<f64> {
    let state = crate::open_meshing::session_world(&app.session);
    let ss = app.session.active_sketch.and_then(|sid| state.sketch(sid))?;
    let lines = construction_line_solutions(&ss.sketch, p, mode).ok()?;
    let line = *lines.get(construction_line_choice(&lines, p).ok()?)?;
    let along = line.direction.len();
    let length = ((pointer - line.start).dot(line.direction) / along).abs();
    (along > 1e-12 && length > 1e-9).then_some(length)
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
    solidcraft_ui_text::tr(key).to_owned()
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
    // Before the first click the preview goes where typed values put the click.
    let typed = (preview && first.is_none()).then(|| (crate::sketch_dims::effective(app, t, pointer.0), None));
    let at = match mode {
        "free" | "angle" => first?,
        // A line with a length starts where the first click was; the pointer picks its side.
        _ => first.or(typed.as_ref()).unwrap_or(pointer),
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
    }
    // A horizontal or vertical line's typed coordinate can be negative or zero: the distance is
    // its size, and `at` (moved to the coordinate) gives the side. Zero is the axis itself.
    if matches!(mode, "parallel" | "horizontal" | "vertical")
        && let Some(offset) = crate::sketch_dims::locked_value(app, t, "Offset").filter(|offset| *offset != 0.0)
    {
        p["offset"] = json!(offset.abs());
    }
    if let Some(expression) = typed_length(t) {
        p["length"] = if preview { json!(crate::sketch_dims::locked_value(app, t, "Length")?) } else { json!(expression) };
    } else if segment(t) && takes_length(mode) && !t.pts.is_empty() {
        // Measured on the numbers: the line is found from them, not from the expressions below.
        p["length"] = json!(pointer_length(app, &p, mode, pointer.0)?);
    }
    // The command keeps a typed angle or offset as its expression. A locked box the user cleared
    // is no offset: an empty string is not a length expression.
    if !preview {
        if mode == "angle"
            && let Some(expression) = crate::sketch_dims::locked_expression(t, "Angle")
        {
            p["angle"] = json!(expression);
        }
        // Only a positive coordinate is its own distance: a negative one's distance is its size.
        if matches!(mode, "parallel" | "horizontal" | "vertical")
            && crate::sketch_dims::locked_value(app, t, "Offset").is_some_and(|offset| offset > 0.0)
            && let Some(expression) = crate::sketch_dims::locked_expression(t, "Offset").filter(|text| !text.trim().is_empty())
        {
            p["offset"] = json!(expression);
        }
    }
    // An infinite line is always construction, so the line type only counts for a segment. A
    // centerline is made from a normal segment once it exists (`place_point`).
    if p.get("length").is_some() && matches!(t.line_type, 0 | 2) {
        p["construction"] = json!(false);
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
    // Only a segment waits for a second click and pins this one as its start. A typed offset
    // places the infinite line on this click, on the cursor's side of the reference.
    let picks_side = length_box_at_start(mode) && segment(t);
    if (matches!(mode, "free" | "angle" | "tangent_external") || picks_side) && t.pts.is_empty() {
        t.pts.push(at);
        if picks_side {
            app.set_status(solidcraft_ui_text::tr("ui.construction.side_hint").to_owned(), false);
        }
        return;
    }
    // A click back on the start names no side: wait for one.
    if picks_side && t.pts.first().is_some_and(|start| start.0.dist(at.0) < 1e-9) {
        return;
    }
    let Some(p) = params(app, t, &at, false) else { return };
    let centerline = t.line_type == 2 && p.get("length").is_some();
    if let Ok(reply) = app.run(t.cmd, p) {
        if centerline {
            // Part of the line's own undo step, as the palette's drawing line type is.
            let depth = app.session.undo.len();
            let _ = app.session.execute("sketch.centerline", &json!({ "curves": reply["curves"], "value": true }));
            app.session.undo.truncate(depth);
        }
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
    // Where a click there puts the point: a typed angle moves the end off the pointer.
    let Some(pointer) = &t.hover.as_ref().map(|hover| crate::sketch_dims::placed(app, t, hover)) else { return };
    let Some(mode) = construction_line_mode(t.cmd) else { return };
    // Before the first click, the pointer's coordinates in grey beside it, as the Line tool shows
    // them (#15618); not while a typed value draws the sketch at it. Horizontal and vertical lines
    // show theirs with their value boxes (`sketch_dims`), as they can be typed.
    if t.pts.is_empty()
        && !t.classic
        && !matches!(mode, "horizontal" | "vertical")
        && crate::dim_view::preview_world(app).is_none()
        && let Some(sketch) =
            app.session.active_sketch.and_then(|sid| crate::open_meshing::session_world(&app.session).sketch(sid).map(|ss| ss.plane))
        && let Some(at) = proj.to_screen(sketch.to_world(pointer.0))
    {
        readout(app, painter, proj, at, pointer.0);
    }
    let Some(p) = params(app, t, pointer, true) else { return };
    let state = crate::open_meshing::session_world(&app.session);
    let Some(ss) = app.session.active_sketch.and_then(|sid| state.sketch(sid)) else { return };
    let Ok(lines) = construction_line_solutions(&ss.sketch, &p, mode) else { return };
    let chosen = construction_line_choice(&lines, &p).ok();
    let tokens = solidcraft_theme::Tokens::get();
    let center = ss.plane.to_local(app.cam.target);
    let extent = app.cam.half_height().max(1.0) * 20.0;
    let length = p.get("length").and_then(Value::as_f64);
    // A segment shows only itself, as the Line tool does: before its start there is no line yet,
    // just the point the click puts down (#15066). An infinite line shows itself whole.
    let waiting = segment(t) && takes_length(mode) && length.is_none();
    // Drawn in the line type it will get: a construction segment dashed, a normal one solid.
    let style = match (length, t.line_type) {
        (Some(_), 0 | 2) => t.line_type,
        (Some(_), _) => 1,
        (None, _) => 3,
    };
    let mut ends = Vec::new();
    for (i, line) in lines.iter().enumerate().filter(|_| !waiting) {
        let selected = chosen == Some(i);
        // A length previews the segment the click will create, on the pointer's side.
        let points = match length {
            Some(length) if selected => {
                let (a, b) = construction_segment(*line, length, Some(pointer.0));
                ends = vec![a, b];
                [ss.plane.to_world(a), ss.plane.to_world(b)]
            }
            _ => {
                let origin = line.start + line.direction * (center - line.start).dot(line.direction);
                [ss.plane.to_world(origin - line.direction * extent), ss.plane.to_world(origin + line.direction * extent)]
            }
        };
        let stroke =
            Stroke::new(if selected { 2.0 } else { 1.0 }, if selected { tokens.preview_new } else { tokens.sketch_accent.gamma_multiply(0.35) });
        crate::tools::paint_sketch_path(&points, if selected { style } else { 3 }, painter, proj, stroke);
    }
    // After a segment's first click, its end's coordinates in grey beside the pointer, as the Line
    // tool shows them (#15618).
    if !t.pts.is_empty()
        && !t.classic
        && let Some(&end) = ends.get(1)
        && crate::dim_view::preview_world(app).is_none()
        && let Some(at) = proj.to_screen(ss.plane.to_world(pointer.0))
    {
        readout(app, painter, proj, at, end);
    }
    // The segment's two ends, and before its first click the point that click puts down.
    for end in ends {
        if let Some(at) = proj.to_screen(ss.plane.to_world(end)) {
            painter.circle_filled(at, 3.0, tokens.preview_new);
        }
    }
    if waiting
        && t.pts.is_empty()
        && let Some(at) = proj.to_screen(ss.plane.to_world(pointer.0))
    {
        painter.circle_stroke(at, if pointer.1.is_some() { 6.0 } else { 3.0 }, Stroke::new(1.5, tokens.sketch_accent));
    }
    if let Some(first) = t.pts.first()
        && let Some(p) = proj.to_screen(ss.plane.to_world(first.0))
    {
        painter.circle_stroke(p, 4.0, Stroke::new(1.5, tokens.sketch_accent));
    }
}

/// `point`'s sketch coordinates in grey below and right of the pointer at `at`, where the Line
/// tool shows them.
fn readout(app: &SolveApp, painter: &egui::Painter, proj: &Proj, at: egui::Pos2, point: Vec2) {
    let shown = painter.text(
        crate::tool_cursor::readout_at(painter, proj.rect, at),
        egui::Align2::LEFT_TOP,
        crate::sketch_cursor_readout::text(app, point),
        egui::FontId::proportional(solidcraft_theme::Tokens::get().font_small),
        solidcraft_theme::Tokens::view().text_dim,
    );
    crate::scenario::publish_rect("viewport:readout", shown);
}

#[cfg(test)]
#[path = "sketch_construction_tests.rs"]
mod tests;
