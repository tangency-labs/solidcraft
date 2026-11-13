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
use solidcraft_engine::geom::prelude::*;
use solidcraft_engine::snap::{SnapKind, SnapPoint};

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
    /// The last item hovered that offered nothing the woken points lack: it is not asked again
    /// while it stays under the pointer.
    pub kept_over: Option<Sel>,
    /// The woken holes' centres, each with a point on its rim, where the design keeps them: inside
    /// a hole's circle the pointer is on its way to the centre, whatever shows through the hole
    /// (#11476).
    pub rims: Vec<(Vec3, Vec3)>,
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
    app.preferences.snap_points && (input_takes(app) || sketch_drawing(app))
}

/// Whether the active dialog input takes snap points (Move, Align, Measure, joints).
fn input_takes(app: &SolveApp) -> bool {
    app.dialog.as_ref().and_then(|d| d.active_input()).is_some_and(|i| i.accept & crate::selection::SNAP_POINTS != 0)
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
/// when the input doesn't take them. So that the pointer can travel to a point it was shown:
/// - nothing changes them while `keep` is held in a dialog's point input (the Keep Snap Points
///   binding, Shift by default; in a sketch, Shift targets a midpoint instead), or while the
///   pointer is within reach of one of them (`near`: within [`REACH`] of a point, or inside a
///   woken rim's or a woken face's hole's circle on its way to the centre);
/// - an item that offers nothing the woken points lack keeps them (a hole's rim after its wall, a
///   woken face's own edge on the way to its corners).
///
/// A vertex of another body puts them to sleep.
///
/// A sketch curve under the pointer wakes its points even where the input doesn't take the
/// curve itself (Move's points), so it is looked for among all the hits (`hits`), not only the
/// hover.
///
/// In a joint origin input snap points belong to a face (#11476): an edge, or a body vertex that
/// isn't a woken point, stands for the flat face beside it ([`face_context`]; `hits` again),
/// whose normal a pick there takes and which is lit ([`lit_face`]).
pub fn update(app: &mut SolveApp, hits: &[Hit], keep: bool, near: Option<(&Proj, Pos2)>) {
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
    if keep && input_takes(app) && app.viewport.woken.item.is_some() {
        return;
    }
    if let Some(Hit::Vertex { body, .. }) = app.viewport.hover.as_ref()
        && app.viewport.woken.points.first().is_some_and(|p| !p.body.is_empty() && p.body != *body)
    {
        app.viewport.woken = Woken { revision: app.session.revision, ..Woken::default() };
        return;
    }
    let joint = joint_input(app);
    let is_woken = |p: Vec3| app.viewport.woken.points.iter().any(|w| w.at.dist(p) <= 1e-6 * (1.0 + p.len()));
    let hovered = match app.viewport.hover.as_ref() {
        Some(h @ Hit::Edge { .. }) if joint => face_context(app, h, hits, near).or_else(|| item_of(app, h)),
        Some(h @ Hit::Vertex { point, .. }) if joint && !is_woken(*point) => face_context(app, h, hits, near),
        Some(h) => item_of(app, h),
        None => None,
    };
    let curve = hits.iter().find_map(|h| item_of(app, h)).filter(|i| matches!(i, Sel::ExternalSketchCurve { .. }));
    let Some(item) = curve.or(hovered) else { return };
    if app.viewport.woken.has(&item) || app.viewport.woken.kept_over.as_ref().is_some_and(|k| same_item(k, &item)) {
        return;
    }
    // On the way to a woken point (within reach of one), nothing else takes over: not a rim seen
    // through a hole at its centre, nor the next face at a corner they share.
    if near.is_some_and(|(proj, pos)| in_reach(app, proj, pos)) {
        return;
    }
    // Crossing a face of the rim's own body on the way to its centre keeps the rim's points and
    // adds the face's; a face of another body sleeps them and wakes its own.
    let (points, rims) = offered(app, &item);
    if let Sel::Face { body: face_body, .. } = &item
        && let Some(rim) = app.viewport.woken.item.clone().filter(|w| matches!(w, Sel::Edge { body, .. } if body == face_body))
    {
        // The rim's own points, without any an earlier crossed face added; then this face's.
        let (rim_points, rim_rims) = offered(app, &rim);
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
        w.rims = rim_rims.into_iter().chain(rims).collect();
        w.face = Some(item.clone());
        remember(app, &item, &added);
        return;
    }
    // An item that offers nothing the woken points lack (a hole's rim after its wall, a woken
    // face's own edge) keeps them; it is remembered, so it is not asked again while it stays
    // under the pointer.
    let within = |a: &[SnapPoint], b: &[SnapPoint]| a.iter().all(|x| b.iter().any(|y| y.at.dist(x.at) <= 1e-6 * (1.0 + x.at.len())));
    if !app.viewport.woken.points.is_empty() && within(&points, &app.viewport.woken.points) {
        app.viewport.woken.kept_over = Some(item);
        return;
    }
    remember(app, &item, &points);
    app.viewport.woken = Woken { item: Some(item), face: None, points, kept_over: None, rims, revision: app.session.revision };
}

/// The points an item offers, found where the design keeps it ([`in_model`]), and the holes'
/// centres among them, each with a point on its rim (#11476).
fn offered(app: &SolveApp, item: &Sel) -> (Vec<SnapPoint>, Vec<(Vec3, Vec3)>) {
    let from = solidcraft_engine::snap::snap_points_from(&app.session, &in_model(app, item));
    let rims = from.iter().filter(|(p, _)| p.kind == SnapKind::Centre).filter_map(|(p, source)| source.edge.map(|e| (p.at, e))).collect();
    (from.into_iter().map(|(p, _)| p).collect(), rims)
}

/// Whether the active input takes joint origins (Joint, Joint Origin, Motion Trace).
fn joint_input(app: &SolveApp) -> bool {
    app.dialog.as_ref().and_then(|d| d.active_input()).is_some_and(|i| i.accept == crate::dialogs_assembly::SNAPS)
}

/// The face an edge or a body vertex under the pointer stands for in a joint origin input: of the
/// flat faces beside the edge (or around the vertex), the one under the pointer, else the one
/// facing the view most. `None` when no flat face is beside it (an edge between curved faces
/// keeps its own points).
fn face_context(app: &SolveApp, hover: &Hit, hits: &[Hit], near: Option<(&Proj, Pos2)>) -> Option<Sel> {
    let (body, at) = match hover {
        Hit::Edge { body, mid, .. } => (body, *mid),
        Hit::Vertex { body, point } => (body, *point),
        _ => return None,
    };
    let st = crate::open_meshing::session_world(&app.session);
    let b = st.body(body)?;
    let m = b.mesh();
    let beside: Vec<u32> = match hover {
        Hit::Edge { index, .. } => m.edge_faces.get(*index)?.clone(),
        _ => {
            // The faces of the edges that end at the vertex.
            let tol = 1e-6 * (1.0 + at.len());
            let ends_here = |e: &Vec<Vec3>| [e.first(), e.last()].into_iter().flatten().any(|p| p.dist(at) <= tol);
            let mut fs: Vec<u32> = m
                .edges
                .iter()
                .enumerate()
                .filter(|(_, e)| ends_here(e))
                .flat_map(|(i, _)| m.edge_faces.get(i).cloned().unwrap_or_default())
                .collect();
            fs.sort_unstable();
            fs.dedup();
            fs
        }
    };
    let flat = |f: u32| b.body.planar_face(f as usize).and_then(|p| p.normal().normalized());
    let under = hits.iter().find_map(|h| match h {
        Hit::Face { body: hb, index, point } if hb == body && beside.contains(&(*index as u32)) && flat(*index as u32).is_some() => {
            Some((*index, *point))
        }
        _ => None,
    });
    let (index, point) = match under {
        Some(x) => x,
        None => {
            let back = near.map(|(proj, _)| proj.cam.back())?;
            let (f, _) = beside.iter().filter_map(|&f| flat(f).map(|n| (f, n.dot(back)))).max_by(|x, y| x.1.total_cmp(&y.1))?;
            // A point inside the face: the middle of its triangle nearest the edge or vertex.
            let inside = m
                .triangles
                .iter()
                .zip(&m.tri_face)
                .filter(|(_, tf)| **tf == f)
                .filter_map(|(t, _)| m.tri(t))
                .map(|[a, b, c]| (a + b + c) / 3.0)
                .min_by(|x, y| x.dist(at).total_cmp(&y.dist(at)))?;
            (f as usize, inside)
        }
    };
    Some(Sel::Face { body: body.clone(), index, point })
}

/// The face to light in a joint origin input while the pointer is on one of its woken points or
/// edges: the face a pick there lies on, and whose normal it takes (#11476). A face under the
/// pointer lights itself.
pub(crate) fn lit_face(app: &SolveApp) -> Option<Hit> {
    let Some(Sel::Face { body, index, point }) = app.viewport.woken.item.as_ref().filter(|_| wanted(app) && joint_input(app)) else {
        return None;
    };
    match app.viewport.hover.as_ref()? {
        Hit::Vertex { .. } | Hit::Edge { .. } => Some(Hit::Face { body: body.clone(), index: *index, point: *point }),
        _ => None,
    }
}

/// Whether the pointer is still where the woken points lead it: within reach of one of them, or
/// inside the circle a woken rim, or a hole of a woken face, spans about its centre on screen (a
/// hole's opening), the points where they show (see [`shown`]).
fn in_reach(app: &SolveApp, proj: &Proj, pos: Pos2) -> bool {
    let shown = shown(app);
    if shown.iter().filter_map(|(at, _)| proj.to_screen(*at)).any(|p| p.distance(pos) <= REACH) {
        return true;
    }
    let inside = |c: Vec3, rim: Vec3| match (proj.to_screen(c), proj.to_screen(rim)) {
        (Some(c), Some(e)) => pos.distance(c) <= c.distance(e) + ON,
        _ => false,
    };
    if shown_rims(app).into_iter().any(|(c, rim)| inside(c, rim)) {
        return true;
    }
    let centre = shown.iter().find(|(_, p)| p.kind == SnapKind::Centre).and_then(|(at, _)| proj.to_screen(*at));
    let on_edge = match &app.viewport.woken.item {
        Some(Sel::Edge { point, .. }) => proj.to_screen(*point),
        _ => None,
    };
    match (centre, on_edge) {
        (Some(c), Some(e)) => pos.distance(c) <= c.distance(e) + ON,
        _ => false,
    }
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

/// The woken holes' rims (centre, point on the rim) where they show, as [`shown`] places the
/// points: on the body a Move dialog moves, where the move so far draws it.
fn shown_rims(app: &SolveApp) -> Vec<(Vec3, Vec3)> {
    let w = &app.viewport.woken;
    let body = match &w.item {
        Some(Sel::Edge { body, .. } | Sel::Face { body, .. }) => body.as_str(),
        _ => "",
    };
    match crate::dialogs_move::chained(app) {
        Some((bodies, m)) if bodies.iter().any(|b| b == body) => {
            let at = |p: Vec3| solidcraft_engine::doc::apply_point(&m, p);
            w.rims.iter().map(|(c, e)| (at(*c), at(*e))).collect()
        }
        _ => w.rims.clone(),
    }
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
