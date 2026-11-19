//! The Drawing workspace's Dimension tool, as the sketch's: pick one edge of a view for its length,
//! or a circle's or an arc's radius or diameter; pick a second for the distance between the two, or
//! the angle between two lines that aren't parallel. Once the picks say what it measures, the
//! dimension follows the pointer until a click on the sheet places it, horizontal, vertical or
//! aligned by where the pointer is ([`linear_axis`], the sketch tool's rule) and
//! an angle in the sector the pointer is in ([`in_sector`]). A click on another
//! edge of the same view while one is picked takes it as the second pick.
//!
//! A drawing's dimensions are reference dimensions: each measures the view as it shows the model
//! and never drives it. The preview is `drawing.dimension.add` itself, dry run with the pointer as
//! its place (`line_through_mm`, or a radius's `text_to_mm`), so it is laid out as the placed one will be; the click adds the
//! same thing. Esc drops the picks, then leaves the tool, through the workspace's own Esc.
use serde_json::{Value, json};
use solidcore_record::{Seg2, Vec2, Vec3};
use solidcraft_engine::sketch::{LinearAxis, in_sector, linear_axis};
use solidcraft_engine::{
    doc::{drawing::dimension::EdgePick, ids},
    drawing_scene,
    geom::{Seg2Geom, Vec2Geom, hlr::Source},
};
use solidcraft_theme::Tokens;

use super::close_panel;
use crate::{
    SolveApp,
    drawing_sheet_page::OnScreen,
    ui_kit::{self, Button},
};

const COMMAND: &str = "drawing.dimension.add";
/// How far the pointer moves on paper, in screen points, before the preview is laid out again.
const RELAYOUT_PX: f64 = 1.0;
/// Two lines whose directions' cross product is under this are parallel.
const PARALLEL: f64 = 1e-6;

/// The tool's picks and its preview, on one sheet as of one revision of the design.
pub(super) struct Draft {
    sheet: u64,
    revision: u64,
    view: u64,
    picks: Vec<Picked>,
    preview: Option<Preview>,
}

/// An edge picked on a view: how `drawing.dimension.add` names it, and its shape on paper.
#[derive(Clone, Debug)]
pub(crate) struct Picked {
    body: String,
    index: usize,
    path: Vec<u64>,
    shape: Shape,
    /// Its stroke on paper, as the view drew it when it was picked: the draft ends with the
    /// revision, so it never outlives a moved view.
    stroke: Vec<Seg2>,
}

/// What a picked edge is on paper, read from the stroke the view draws for it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Shape {
    /// A line from one end to the other.
    Straight([Vec2; 2]),
    /// A circle or an arc about its centre.
    Round(Vec2),
    /// Neither: a spline, or an ellipse a tilted circle makes.
    Other,
}

/// The dimension the picks make with the pointer at `at`, as `drawing.dimension.add` dry run answered it.
struct Preview {
    at: Vec2,
    answer: Result<Value, String>,
}

/// What a stroke drawn on paper is: one line (its pieces on one line, as hidden-line removal
/// splits it), a circle or an arc, or something else.
pub(crate) fn shape_of(path: &[Seg2]) -> Shape {
    let mut ends: Vec<Vec2> = Vec::new();
    let mut centre = None;
    for seg in path {
        match *seg {
            Seg2::Line { a, b } => ends.extend([a, b]),
            Seg2::Arc { center, .. } => centre = Some(center),
            _ => return Shape::Other,
        }
    }
    if let Some(centre) = centre {
        return if ends.is_empty() { Shape::Round(centre) } else { Shape::Other };
    }
    let (Some(first), Some(last)) = (ends.first().copied(), ends.last().copied()) else { return Shape::Other };
    let Some(dir) = (last - first).normalized() else { return Shape::Other };
    let len = first.dist(last);
    if ends.iter().any(|p| (*p - first).cross(dir).abs() > 1e-6 * len.max(1.0)) {
        return Shape::Other;
    }
    let along = |p: &Vec2| (*p - first).dot(dir);
    let lo = ends.iter().copied().min_by(|p, q| along(p).total_cmp(&along(q))).unwrap_or(first);
    let hi = ends.iter().copied().max_by(|p, q| along(p).total_cmp(&along(q))).unwrap_or(last);
    Shape::Straight([lo, hi])
}

/// A linear dimension's direction as `drawing.dimension.add` takes it, square to a line running
/// along `dir`: horizontal or vertical when that is one, else along it.
fn across(dir: Vec2) -> Value {
    let n = dir.perp();
    if n.y.abs() <= PARALLEL * n.len() {
        json!("horizontal")
    } else if n.x.abs() <= PARALLEL * n.len() {
        json!("vertical")
    } else {
        json!({"along": [n.x, n.y]})
    }
}

fn axis_name(axis: LinearAxis) -> &'static str {
    match axis {
        LinearAxis::Aligned => "aligned",
        LinearAxis::Horizontal => "horizontal",
        LinearAxis::Vertical => "vertical",
    }
}

/// Where two lines meet on paper, and the smaller angle between them there as the layout draws it
/// (the first ray and the sweep): the frame [`in_sector`] tests the pointer against.
fn smaller_angle(a: [Vec2; 2], b: [Vec2; 2]) -> Option<(Vec2, Vec2, f64)> {
    let (da, db) = ((a[1] - a[0]).normalized()?, (b[1] - b[0]).normalized()?);
    let turn = da.perp().dot(db);
    if turn.abs() < PARALLEL {
        return None;
    }
    let vertex = a[0] + da * ((b[0] - a[0]).perp().dot(db) / turn);
    let ray = |p: Vec2, q: Vec2| (if p.dist(vertex) >= q.dist(vertex) { p } else { q } - vertex).normalized();
    let (from_a, from_b) = (ray(a[0], a[1]).unwrap_or(da), ray(b[0], b[1]).unwrap_or(db));
    let from_b = if from_a.dot(from_b) < 0.0 { -from_b } else { from_b };
    let cross = from_a.perp().dot(from_b);
    let d0 = if cross >= 0.0 { from_a } else { from_b };
    Some((vertex, d0, cross.abs().atan2(from_a.dot(from_b))))
}

impl Picked {
    fn edge(&self) -> Value {
        json!({"body": self.body, "index": self.index, "path": self.path.iter().copied().map(ids::encode).collect::<Vec<_>>()})
    }

    fn centre(&self) -> Value {
        json!({"centre": {"body": self.body, "index": self.index}, "path": self.path.iter().copied().map(ids::encode).collect::<Vec<_>>()})
    }

    /// One end of a straight edge as a corner, by its point in the design's frame.
    fn end(&self, app: &SolveApp) -> Option<Value> {
        let pick = EdgePick::Index { body: self.body.clone(), index: self.index };
        let [start, _]: [Vec3; 2] = app.session.doc.edge_line(&app.session.model, &pick, &self.path).ok()?;
        Some(json!({"point": [start.x, start.y, start.z], "path": self.path.iter().copied().map(ids::encode).collect::<Vec<_>>()}))
    }
}

/// The `kind` of `drawing.dimension.add` the picks make with the pointer at `at`, as the sketch's
/// Dimension tool reads the same picks: one line its length, one circle or arc its size, two lines
/// the angle between them or, parallel, the distance across them, a line and a circle from the line
/// to the centre, two circles between their centres.
pub(crate) fn kind_for(app: &SolveApp, picks: &[Picked], at: Vec2) -> Option<Value> {
    Some(match picks {
        [one] => match one.shape {
            Shape::Straight([a, b]) => json!({"length": {"edge": one.edge(), "direction": axis_name(linear_axis(a, b, at))}}),
            Shape::Round(_) => json!({"radial": {"edge": one.edge()}}),
            Shape::Other => json!({"length": {"edge": one.edge()}}),
        },
        [first, second] => match (first.shape, second.shape) {
            (Shape::Straight(a), Shape::Straight(b)) => match smaller_angle(a, b) {
                Some((vertex, d0, sweep)) => {
                    let t = at - vertex;
                    let other_side = !(in_sector(d0, sweep, t) || in_sector(-d0, sweep, t));
                    json!({"angle": {"a": first.edge(), "b": second.edge(), "other_side": other_side}})
                }
                None => {
                    let dir = (a[1] - a[0]).normalized()?;
                    json!({"linear": {"a": first.end(app)?, "b": second.end(app)?, "direction": across(dir)}})
                }
            },
            (Shape::Straight([p, q]), Shape::Round(_)) => {
                json!({"linear": {"a": second.centre(), "b": first.end(app)?, "direction": across((q - p).normalized()?)}})
            }
            (Shape::Round(_), Shape::Straight([p, q])) => {
                json!({"linear": {"a": first.centre(), "b": second.end(app)?, "direction": across((q - p).normalized()?)}})
            }
            (Shape::Round(c), Shape::Round(d)) => {
                json!({"linear": {"a": first.centre(), "b": second.centre(), "direction": axis_name(linear_axis(c, d, at))}})
            }
            _ => return None,
        },
        _ => return None,
    })
}

pub(super) fn panel(app: &mut SolveApp, ui: &mut egui::Ui) {
    let picked = app.drawing_workspace.dimension.as_ref().is_some_and(|draft| !draft.picks.is_empty());
    let key = if picked { "ui.drawing_dimension_tool.place" } else { "ui.drawing_dimension_tool.pick" };
    let close = super::inset(ui, |ui| {
        ui_kit::text(ui, solidcraft_ui_text::tr(key), ui_kit::Text::Note(key));
        ui.separator();
        ui_kit::button(ui, solidcraft_ui_text::tr("ui.cancel"), Button::Normal).clicked()
    });
    if close {
        close_panel(app);
    }
}

/// Drops the tool's picks, keeping the tool: the first Esc, as in the sketch. Whether there were any.
pub(crate) fn clear_picks(app: &mut SolveApp) -> bool {
    app.drawing_workspace.dimension.take().is_some_and(|draft| !draft.picks.is_empty())
}

/// The edge under `point`, picked: on a drawn view, an edge of a body (not a silhouette).
fn pick(app: &mut SolveApp, sheet: u64, point: Vec2, band: f64) -> Result<Option<(u64, Picked)>, String> {
    let Some(hit) = drawing_scene::line_at(&mut app.session, sheet, point, band).map_err(|error| error.to_string())? else { return Ok(None) };
    let Source::Edge { edge, .. } = hit.source else { return Ok(None) };
    Ok(Some((hit.view, Picked { body: hit.body.body, index: edge, path: hit.body.path, shape: shape_of(&hit.path), stroke: hit.path })))
}

/// What `drawing.dimension.add` takes for the picks with the text at `at`, or `None` when they make
/// no dimension.
fn params_for(app: &SolveApp, draft: &Draft, at: Vec2) -> Option<Value> {
    let kind = kind_for(app, &draft.picks, at)?;
    // A radius's text goes where the pointer is, its leader turned to it; the others' line or arc
    // passes through the pointer, their text on it, as a sketch dimension's line does.
    let drag = if kind.get("radial").is_some() { "text_to_mm" } else { "line_through_mm" };
    let mut params = json!({"view": ids::encode(draft.view), "kind": kind});
    params[drag] = json!([at.x, at.y]);
    Some(params)
}

fn preview(app: &mut SolveApp, draft: &Draft, at: Vec2) -> Option<Preview> {
    let mut dry = params_for(app, draft, at)?;
    dry["dry_run"] = json!(true);
    Some(Preview { at, answer: app.run(COMMAND, dry) })
}

fn points(value: &Value) -> Vec<Vec2> {
    value.as_array().into_iter().flatten().filter_map(|p| serde_json::from_value::<[f64; 2]>(p.clone()).ok().map(Vec2::from)).collect()
}

/// Paints an answer's layout: its lines, arrowheads and text, as it will sit once placed.
fn paint_layout(painter: &egui::Painter, at: OnScreen, answer: &Value) {
    let tokens = Tokens::get();
    let stroke = egui::Stroke::new(1.0, tokens.accent);
    let layout = &answer["layout"];
    for line in ["extension_lines", "dimension_lines"].iter().flat_map(|key| layout[*key].as_array().into_iter().flatten()) {
        let line: Vec<egui::Pos2> = points(line).into_iter().map(|p| at.screen(p)).collect();
        painter.add(egui::Shape::line(line, stroke));
    }
    for arrow in layout["arrows"].as_array().into_iter().flatten() {
        let (Some(tip), Some(dir)) = (points(&json!([arrow["tip"]])).first().copied(), points(&json!([arrow["points"]])).first().copied()) else {
            continue;
        };
        let back = dir * -3.0;
        let side = dir.perp() * 1.0;
        let shape = [tip, tip + back + side, tip + back - side].map(|p| at.screen(p));
        painter.add(egui::Shape::convex_polygon(shape.to_vec(), tokens.accent, egui::Stroke::NONE));
    }
    let text = answer["text"].as_str().unwrap_or_default();
    let centre = layout["text_boxes"].as_array().and_then(|boxes| boxes.first()).map(points).filter(|corners| corners.len() == 4);
    if let Some(corners) = centre {
        let middle = corners.iter().fold(Vec2::ZERO, |sum, p| sum + *p) * 0.25;
        painter.text(at.screen(middle), egui::Align2::CENTER_CENTER, text, egui::FontId::proportional(tokens.font_small), tokens.accent);
    }
}

/// Paints a picked edge's stroke, so the picks show.
fn paint_pick(painter: &egui::Painter, at: OnScreen, shape: Shape, path: &[Seg2]) {
    let stroke = egui::Stroke::new(2.5, Tokens::get().accent);
    if let Shape::Straight([a, b]) = shape {
        painter.line_segment([at.screen(a), at.screen(b)], stroke);
        return;
    }
    for seg in path {
        let line: Vec<egui::Pos2> = (0..=24).map(|i| at.screen(seg.point_at(f64::from(i) / 24.0))).collect();
        painter.add(egui::Shape::line(line, stroke));
    }
}

pub(crate) fn on_sheet(app: &mut SolveApp, painter: &egui::Painter, at: OnScreen, sheet: u64, point: Vec2, band: f64, clicked: bool) {
    let revision = app.session.revision;
    let mut draft = app.drawing_workspace.dimension.take().filter(|draft| draft.sheet == sheet && draft.revision == revision);
    if clicked {
        let more = draft.as_ref().is_none_or(|draft| draft.picks.len() < 2);
        let hit = if more {
            match pick(app, sheet, point, band) {
                Ok(hit) => hit,
                Err(error) => {
                    app.set_status(error, true);
                    None
                }
            }
        } else {
            None
        };
        match (&mut draft, hit) {
            // A first pick, or another edge of the same view as the first.
            (None, Some((view, picked))) => draft = Some(Draft { sheet, revision, view, picks: vec![picked], preview: None }),
            (Some(d), Some((view, picked)))
                if view == d.view && !d.picks.iter().any(|p| p.body == picked.body && p.index == picked.index && p.path == picked.path) =>
            {
                d.picks.push(picked);
                d.preview = None;
            }
            // Anywhere else: the dimension the picks make goes where the pointer is.
            (Some(d), _) => {
                if let Some(params) = params_for(app, d, point) {
                    match app.run(COMMAND, params) {
                        Ok(_) => draft = None,
                        Err(error) => app.set_status(error, true),
                    }
                }
            }
            (None, None) => {}
        }
    }
    if let Some(d) = &mut draft {
        let stale = d.preview.as_ref().is_none_or(|p| p.at.dist(point) * at_scale(at) > RELAYOUT_PX);
        if stale {
            d.preview = preview(app, d, point);
        }
        for picked in &d.picks {
            paint_pick(painter, at, picked.shape, &picked.stroke);
        }
        if let Some(Preview { answer: Ok(answer), .. }) = &d.preview {
            paint_layout(painter, at, answer);
        }
        painter.ctx().request_repaint();
    }
    app.drawing_workspace.dimension = draft;
}

/// Paper points per millimetre on screen.
fn at_scale(at: OnScreen) -> f64 {
    at.screen(Vec2::new(1.0, 0.0)).x as f64 - at.screen(Vec2::ZERO).x as f64
}

/// The picks so far, for tests: their shapes on paper.
#[cfg(test)]
pub(crate) fn picked_shapes(app: &SolveApp) -> Vec<Shape> {
    app.drawing_workspace.dimension.as_ref().map(|d| d.picks.iter().map(|p| p.shape).collect()).unwrap_or_default()
}
