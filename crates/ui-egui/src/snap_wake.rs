//! Hover pick points (#5912): while a point input that takes them is active (Move's From and To,
//! Measure), hovering an edge, a face or a sketch curve wakes the points it offers (`solidcraft_engine::snap`):
//! a hole's centre, an edge's middle and ends, a face's centre. Woken points are drawn as small
//! glyphs and picked like vertices from further away, so a centre in empty space can be clicked.
//! They stay awake across empty space and sleep when another edge or face is hovered, or when
//! the input stops taking them.

use egui::{Painter, Pos2, Stroke};
use solidcraft_engine::Sel;
use solidcraft_engine::geom::Vec3;
use solidcraft_engine::snap::{SnapKind, SnapPoint, snap_points};

use crate::SolveApp;
use crate::theme::Tokens;
use crate::viewport::{Hit, Proj};

/// How far from the pointer a woken point is picked (screen points): twice a vertex's reach.
pub const REACH: f32 = 16.0;
/// Within this of the pointer a woken point wins over the edge or curve it lies on.
pub const ON: f32 = 4.0;

/// The woken points and the item that woke them.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Woken {
    pub item: Option<Sel>,
    pub points: Vec<SnapPoint>,
}

/// Whether the active dialog input takes hover pick points.
pub fn wanted(app: &SolveApp) -> bool {
    app.dialog.as_ref().and_then(|d| d.active_input()).is_some_and(|i| i.accept & crate::selection::SNAP_POINTS != 0)
}

/// The item a hit wakes points of: an edge or a face of a body, or a sketch curve (a line's
/// middle, a circle's centre).
fn item_of(h: &Hit) -> Option<Sel> {
    match h {
        Hit::Edge { body, index, mid } => Some(Sel::Edge { body: body.clone(), index: *index, point: *mid }),
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

/// After the hover was found this frame: a newly hovered edge or face wakes its points (and the
/// earlier ones sleep); anything else keeps them. All sleep when the input doesn't take them.
pub fn update(app: &mut SolveApp) {
    if !wanted(app) {
        app.viewport.woken = Woken::default();
        return;
    }
    let Some(item) = app.viewport.hover.as_ref().and_then(item_of) else { return };
    if app.viewport.woken.item.as_ref().is_some_and(|w| same_item(w, &item)) {
        return;
    }
    let points = snap_points(&app.session, &item);
    app.viewport.woken = Woken { item: Some(item), points };
}

/// The woken points within reach of `pos`, nearest first, as vertex hits (with their distance).
pub fn hits(app: &SolveApp, proj: &Proj, pos: Pos2) -> Vec<(f32, Hit)> {
    if !wanted(app) {
        return Vec::new();
    }
    let mut v: Vec<(f32, Hit)> = app
        .viewport
        .woken
        .points
        .iter()
        .filter_map(|p| {
            let d = proj.to_screen(p.at)?.distance(pos);
            (d < REACH).then(|| (d, Hit::Vertex { body: p.body.clone(), point: p.at }))
        })
        .collect();
    v.sort_by(|a, b| a.0.total_cmp(&b.0));
    v
}

/// Draw the woken points: a ring with a cross for a centre, a ring with a dot for a face's
/// centre, a triangle for a middle, a square for an end. Vertices draw as vertices already.
pub fn draw(app: &SolveApp, painter: &Painter, proj: &Proj) {
    if !wanted(app) {
        return;
    }
    let t = Tokens::get();
    let line = Stroke::new(1.5, t.hover_edge);
    for p in &app.viewport.woken.points {
        let Some(c) = proj.to_screen(p.at) else { continue };
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

/// The woken points for `ui.inspect`: where each is and what it is.
pub fn inspect(app: &SolveApp) -> serde_json::Value {
    serde_json::to_value(&app.viewport.woken.points).unwrap_or_default()
}

/// A world point's label in an input ("(x, y, z) mm").
pub fn label(p: Vec3) -> String {
    format!("({:.2}, {:.2}, {:.2}) mm", p.x, p.y, p.z)
}
