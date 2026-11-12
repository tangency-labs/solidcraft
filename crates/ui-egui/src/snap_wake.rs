//! Snap points (#5912): while a point input that takes them is active (Move's From and To,
//! Measure), hovering an edge, a face or a sketch curve wakes the points it offers (`solidcraft_engine::snap`):
//! a hole's centre, an edge's middle and ends, a face's centre. Woken points are drawn as small
//! glyphs and picked like vertices from further away, so a centre in empty space can be clicked.
//! Sketch tools that place points take them too (#11229): a 2D tool's point lands on the
//! sketch plane where the snap point projects to, a 3D sketch tool's on the point itself.
//! On by default; the `snap_points` preference turns them off.
//! They stay awake across empty space and faces, and sleep when another edge or curve is hovered
//! (a face only replaces a face's points), or when the input stops taking them.

use egui::{Painter, Pos2, Stroke};
use solidcore_record::Vec3;
use solidcraft_engine::Sel;
use solidcraft_engine::snap::{SnapKind, SnapPoint, snap_points};

use crate::SolveApp;
use crate::viewport::{Hit, Proj};
use solidcraft_theme::Tokens;

/// How far from the pointer a woken point is picked (screen points): twice a vertex's reach.
pub const REACH: f32 = 16.0;
/// Within this of the pointer a woken point wins over the edge or curve it lies on.
pub const ON: f32 = 4.0;

/// The woken points and the item that woke them.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Woken {
    pub item: Option<Sel>,
    /// A face of the rim's own body crossed on the way to its centre: its points are woken too.
    pub face: Option<Sel>,
    pub points: Vec<SnapPoint>,
    /// The design's revision the points were found in: an edit, undo or redo sleeps them.
    pub revision: u64,
}

impl Woken {
    /// Whether `sel` is the item, or the crossed face, whose points are awake.
    pub fn has(&self, sel: &Sel) -> bool {
        [&self.item, &self.face].into_iter().flatten().any(|w| same_item(w, sel))
    }
}

/// Whether snap points are on and the active dialog input or sketch tool takes them.
pub fn wanted(app: &SolveApp) -> bool {
    app.preferences.snap_points
        && (app.dialog.as_ref().and_then(|d| d.active_input()).is_some_and(|i| i.accept & crate::selection::SNAP_POINTS != 0) || sketch_drawing(app))
}

/// Whether a sketch tool that places points is active (2D or 3D).
fn sketch_drawing(app: &SolveApp) -> bool {
    app.session.active_sketch.is_some() && app.tool.as_ref().is_some_and(|t| crate::sketch_tools::places_points(t) || crate::sketch3d::drawing(app))
}

/// Whether woken points may be picked or drawn: as [`wanted`], or in a sketch whose tool a click
/// has taken out of the app while it runs (the woken points are the last hover's; `update`
/// puts them to sleep once nothing takes them).
fn usable(app: &SolveApp) -> bool {
    wanted(app) || (app.preferences.snap_points && app.tool.is_none() && app.session.active_sketch.is_some())
}

/// The item a hit wakes points of: an edge or a face of a body, or a sketch curve (a line's
/// middle, a circle's centre). Not a curve of the sketch being edited: its tools snap to its own
/// points and middles already.
fn item_of(app: &SolveApp, h: &Hit) -> Option<Sel> {
    match h {
        Hit::SketchCurve { sketch, .. } if app.tool.is_some() && app.session.active_sketch == Some(*sketch) => None,
        Hit::Edge { body, index, mid, .. } => Some(Sel::Edge { body: body.clone(), index: *index, point: *mid }),
        Hit::Face { body, index, point } => Some(Sel::Face { body: body.clone(), index: *index, point: *point }),
        Hit::SketchCurve { sketch, id, .. } => Some(Sel::ExternalSketchCurve { sketch: *sketch, id: id.clone() }),
        _ => None,
    }
}

/// Whether two picks are the same edge or face (wherever on it).
fn same_item(a: &Sel, b: &Sel) -> bool {
    match (a, b) {
        (Sel::Edge { body: x, index: i, .. }, Sel::Edge { body: y, index: j, .. })
        | (Sel::Face { body: x, index: i, .. }, Sel::Face { body: y, index: j, .. }) => x == y && i == j,
        (Sel::ExternalSketchCurve { sketch: s, id: i }, Sel::ExternalSketchCurve { sketch: t, id: j }) => s == t && i == j,
        _ => false,
    }
}

/// After the hover was found this frame: a newly hovered edge or curve wakes its points (and the
/// earlier ones sleep); a face does too unless an edge's or curve's points are awake, so the
/// pointer can cross the faces between a rim and its centre. Anything else keeps them. All sleep
/// when the input doesn't take them.
///
/// A sketch curve under the pointer wakes its points even where the input doesn't take the
/// curve itself (Move's points), so it is looked for among all the hits (`hits`), not only the
/// hover.
pub fn update(app: &mut SolveApp, hits: &[Hit]) {
    if app.dialog.is_none() {
        app.viewport.snapped.clear();
    }
    if !wanted(app) {
        app.viewport.woken = Woken::default();
        return;
    }
    // An edit, undo or redo sleeps the woken points, whatever is under the pointer now.
    if app.viewport.woken.revision != app.session.revision {
        app.viewport.woken = Woken { revision: app.session.revision, ..Woken::default() };
    }
    let curve = hits.iter().find_map(|h| item_of(app, h)).filter(|i| matches!(i, Sel::ExternalSketchCurve { .. }));
    let Some(item) = curve.or_else(|| app.viewport.hover.as_ref().and_then(|h| item_of(app, h))) else { return };
    if app.viewport.woken.has(&item) {
        return;
    }
    // Crossing a face of the rim's own body on the way to its centre keeps the rim's points and
    // adds the face's; a face of another body sleeps them and wakes its own.
    let points = snap_points(&app.session, &in_model(app, &item));
    if let Sel::Face { body: face_body, .. } = &item
        && let Some(rim) = app.viewport.woken.item.clone().filter(|w| matches!(w, Sel::Edge { body, .. } if body == face_body))
    {
        // The rim's own points, without any an earlier crossed face added; then this face's.
        let rim_points = snap_points(&app.session, &in_model(app, &rim));
        let w = &mut app.viewport.woken;
        w.points = rim_points;
        // Only the face's own points are remembered as the face's: a point the rim already
        // shows there stays the rim's, as it is drawn and picked.
        let mut added = Vec::new();
        for p in points {
            let tol = 1e-6 * (1.0 + p.at.x.abs().max(p.at.y.abs()).max(p.at.z.abs()));
            if !w.points.iter().any(|q| (q.at.x - p.at.x).abs() <= tol && (q.at.y - p.at.y).abs() <= tol && (q.at.z - p.at.z).abs() <= tol) {
                added.push(p);
            }
        }
        w.points.extend(added.iter().cloned());
        w.face = Some(item.clone());
        remember(app, &item, &added);
        return;
    }
    remember(app, &item, &points);
    app.viewport.woken = Woken { item: Some(item), face: None, points, revision: app.session.revision };
}

/// A hovered item taken back to where the design keeps it: on a body a Move dialog moves, the
/// item is picked where the move so far draws the body (see [`shown`]), and its points are
/// found on the design.
fn in_model(app: &SolveApp, item: &Sel) -> Sel {
    let Some((bodies, back)) = crate::dialogs_move::chained(app).and_then(|(b, m)| Some((b, solidcraft_engine::doc::mat_inverse(&m)?))) else {
        return item.clone();
    };
    let undo = |p: &Vec3| solidcraft_engine::doc::apply_point(&back, *p);
    match item {
        Sel::Edge { body, index, point } if bodies.contains(body) => Sel::Edge { body: body.clone(), index: *index, point: undo(point) },
        Sel::Face { body, index, point } if bodies.contains(body) => Sel::Face { body: body.clone(), index: *index, point: undo(point) },
        other => other.clone(),
    }
}

/// The woken points where they show: a point of a body a Move dialog moves is where the move
/// so far draws that body, not where the design keeps it.
pub fn shown(app: &SolveApp) -> Vec<(Vec3, &SnapPoint)> {
    let moved = crate::dialogs_move::chained(app);
    let at = |p: &SnapPoint| match &moved {
        Some((bodies, m)) if bodies.contains(&p.body) => solidcraft_engine::doc::apply_point(m, p.at),
        _ => p.at,
    };
    // A point of a body or sketch hidden since it woke is not shown or picked.
    if let Some(Sel::ExternalSketchCurve { sketch, .. }) = &app.viewport.woken.item
        && !crate::browser::sketch_visible(app, *sketch)
    {
        return Vec::new();
    }
    app.viewport.woken.points.iter().filter(|p| p.body.is_empty() || app.body_visible(&p.body)).map(|p| (at(p), p)).collect()
}

/// How many woken points a dialog remembers (the oldest are forgotten first).
const REMEMBERED: usize = 256;

/// Remember points woken by `item`, for [`snapped`].
fn remember(app: &mut SolveApp, item: &Sel, points: &[SnapPoint]) {
    let seen = &mut app.viewport.snapped;
    seen.retain(|(_, p)| !points.iter().any(|q| q.at == p.at));
    seen.extend(points.iter().map(|p| (item.clone(), p.clone())));
    let over = seen.len().saturating_sub(REMEMBERED);
    seen.drain(..over);
}

/// The snap point picked at `at` while this dialog was open, and the item that woke it.
pub fn snapped(app: &SolveApp, at: Vec3) -> Option<&(Sel, SnapPoint)> {
    app.viewport.snapped.iter().rev().find(|(_, p)| p.at == at)
}

/// The woken points within reach of `pos`, nearest first, as vertex hits (with their distance
/// and kind).
pub fn hits(app: &SolveApp, proj: &Proj, pos: Pos2) -> Vec<(f32, Hit, SnapKind)> {
    if !usable(app) {
        return Vec::new();
    }
    let mut v: Vec<(f32, Hit, SnapKind)> = shown(app)
        .into_iter()
        .filter_map(|(at, p)| {
            let d = proj.to_screen(at)?.distance(pos);
            (d < REACH).then(|| (d, Hit::Vertex { body: p.body.clone(), point: at }, p.kind))
        })
        .collect();
    v.sort_by(|a, b| a.0.total_cmp(&b.0));
    v
}

/// Draw the woken points: a ring with a cross for a centre, a ring with a dot for a face's
/// centre, a triangle for a middle, a square for an end. Vertices draw as vertices already.
pub fn draw(app: &SolveApp, painter: &Painter, proj: &Proj) {
    if !usable(app) {
        return;
    }
    let t = Tokens::get();
    let line = Stroke::new(1.5, t.hover_edge);
    for (at, p) in shown(app) {
        let Some(c) = proj.to_screen(at) else { continue };
        let r = 5.0;
        match p.kind {
            SnapKind::Centre => {
                painter.circle(c, r, t.hover_edge_halo, line);
                painter.line_segment([c - egui::vec2(r + 3.0, 0.0), c + egui::vec2(r + 3.0, 0.0)], line);
                painter.line_segment([c - egui::vec2(0.0, r + 3.0), c + egui::vec2(0.0, r + 3.0)], line);
            }
            SnapKind::FaceCentre => {
                painter.circle(c, r, t.hover_edge_halo, line);
                painter.circle_filled(c, 1.5, t.hover_edge);
            }
            SnapKind::Middle => {
                let pts = vec![c + egui::vec2(0.0, -r), c + egui::vec2(r, r * 0.8), c + egui::vec2(-r, r * 0.8)];
                painter.add(egui::Shape::convex_polygon(pts, t.hover_edge_halo, line));
            }
            SnapKind::End => {
                painter.rect(egui::Rect::from_center_size(c, egui::vec2(r * 1.6, r * 1.6)), 0.0, t.hover_edge_halo, line, egui::StrokeKind::Middle);
            }
            SnapKind::Vertex => {}
        }
    }
}

/// The woken points for `ui.inspect`: where each shows (as `shown` draws it) and what it is.
pub fn inspect(app: &SolveApp) -> serde_json::Value {
    let points: Vec<SnapPoint> = shown(app).into_iter().map(|(at, p)| SnapPoint { at, ..p.clone() }).collect();
    serde_json::to_value(&points).unwrap_or_default()
}

/// A world point's label in an input ("(x, y, z) mm").
pub fn label(p: Vec3) -> String {
    format!("({:.2}, {:.2}, {:.2}) mm", p.x, p.y, p.z)
}

/// The woken point at `at`, if `at` is one (a vertex hit made by [`hits`]).
pub fn woken_at(app: &SolveApp, at: Vec3) -> Option<&SnapPoint> {
    app.viewport.woken.points.iter().find(|p| p.at == at)
}
