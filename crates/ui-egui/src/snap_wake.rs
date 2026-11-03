//! Snap points (#5912): while a point input that takes them is active (Move's From and To,
//! Measure), hovering an edge, a face or a sketch curve wakes the points it offers (`solidcraft_engine::snap`):
//! a hole's centre, an edge's middle and ends, a face's centre. Woken points are drawn as small
//! glyphs and picked like vertices from further away, so a centre in empty space can be clicked.
//! On by default; the `snap_points` preference turns them off.
//! They stay awake across empty space and faces, and sleep when another edge or curve is hovered
//! (a face only replaces a face's points), or when the input stops taking them.

use egui::{Painter, Pos2, Stroke};
use solidcore_record::Vec3;
use solidcraft_engine::Sel;
use solidcraft_engine::geom::prelude::*;
use solidcraft_engine::snap::{SnapKind, SnapPoint};

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
    /// The last item hovered that offered nothing the woken points lack: it is not asked again
    /// while it stays under the pointer.
    pub kept_over: Option<Sel>,
    /// The woken holes' centres, each with a point on its rim: inside a hole's circle the pointer
    /// is on its way to the centre, whatever shows through the hole (#11476).
    pub rims: Vec<(Vec3, Vec3)>,
}

/// Whether snap points are on and the active dialog input takes them.
pub fn wanted(app: &SolveApp) -> bool {
    app.preferences.snap_points && app.dialog.as_ref().and_then(|d| d.active_input()).is_some_and(|i| i.accept & crate::selection::SNAP_POINTS != 0)
}

/// The item a hit wakes points of: an edge or a face of a body, or a sketch curve (a line's
/// middle, a circle's centre).
fn item_of(h: &Hit) -> Option<Sel> {
    match h {
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

/// After the hover was found this frame: a newly hovered edge, face or curve wakes its points
/// and the earlier ones sleep, with these exceptions, so the pointer can travel to a point it
/// was shown:
/// - an item that offers nothing the woken points lack keeps them (a woken face's own edges, on
///   the way to its corners and centres);
/// - a face keeps an edge's or curve's points unless it offers them all too (a hole's rim on the
///   face: nothing is lost), so the pointer can cross the faces between a rim and its centre;
/// - nothing changes them while the pointer is within reach of one of them (`near`: within
///   [`REACH`] of a point, or inside a woken rim's or a woken face's hole's circle on its way to
///   the centre), or while `keep` is held (the Keep Snap Points binding, Shift by default).
///
/// In a joint origin input snap points belong to a face (#11476): an edge, or a body vertex that
/// isn't a woken point, stands for the flat face beside it ([`face_context`]; `hits`: everything
/// under the pointer), whose normal a pick there takes and which is lit ([`lit_face`]).
///
/// Empty space keeps them; a vertex of another body, or an input that doesn't take them, puts
/// them to sleep.
pub fn update(app: &mut SolveApp, keep: bool, near: Option<(&Proj, Pos2)>, hits: &[Hit]) {
    if !wanted(app) {
        app.viewport.woken = Woken::default();
        return;
    }
    if keep && app.viewport.woken.item.is_some() {
        return;
    }
    if let Some(Hit::Vertex { body, .. }) = app.viewport.hover.as_ref()
        && app.viewport.woken.points.first().is_some_and(|p| !p.body.is_empty() && p.body != *body)
    {
        app.viewport.woken = Woken::default();
        return;
    }
    let joint = joint_input(app);
    let is_woken = |p: Vec3| app.viewport.woken.points.iter().any(|w| w.at.dist(p) <= 1e-6 * (1.0 + p.len()));
    let item = match app.viewport.hover.as_ref() {
        Some(h @ Hit::Edge { .. }) if joint => face_context(app, h, hits, near).or_else(|| item_of(h)),
        Some(h @ Hit::Vertex { point, .. }) if joint && !is_woken(*point) => face_context(app, h, hits, near),
        Some(h) => item_of(h),
        None => None,
    };
    let Some(item) = item else { return };
    let woken = &app.viewport.woken;
    if [&woken.item, &woken.kept_over].into_iter().flatten().any(|w| same_item(w, &item)) {
        return;
    }
    // On the way to a woken point (within reach of one), nothing else takes over: not a rim seen
    // through a hole at its centre, nor the next face at a corner they share.
    if near.is_some_and(|(proj, pos)| in_reach(proj, pos, woken)) {
        return;
    }
    let from = solidcraft_engine::snap::snap_points_from(&app.session, &item);
    let rims: Vec<(Vec3, Vec3)> =
        from.iter().filter(|(p, _)| p.kind == SnapKind::Centre).filter_map(|(p, source)| source.edge.map(|e| (p.at, e))).collect();
    let points: Vec<SnapPoint> = from.into_iter().map(|(p, _)| p).collect();
    let within = |a: &[SnapPoint], b: &[SnapPoint]| a.iter().all(|x| b.iter().any(|y| y.at.dist(x.at) <= 1e-6 * (1.0 + x.at.len())));
    if matches!(item, Sel::Face { .. }) && woken.item.as_ref().is_some_and(|w| !matches!(w, Sel::Face { .. })) && !within(&woken.points, &points) {
        return;
    }
    // An item that offers nothing the woken points lack (a hole's rim after its wall, a woken
    // face's own edge) keeps them; it is remembered, so it is not asked again while it stays
    // under the pointer.
    if !woken.points.is_empty() && within(&points, &woken.points) {
        app.viewport.woken.kept_over = Some(item);
        return;
    }
    app.viewport.woken = Woken { item: Some(item), points, kept_over: None, rims };
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
/// inside the circle a woken rim spans about its centre on screen (a hole's opening).
fn in_reach(proj: &Proj, pos: Pos2, w: &Woken) -> bool {
    if w.points.iter().filter_map(|p| proj.to_screen(p.at)).any(|p| p.distance(pos) <= REACH) {
        return true;
    }
    let inside = |c: Vec3, rim: Vec3| match (proj.to_screen(c), proj.to_screen(rim)) {
        (Some(c), Some(e)) => pos.distance(c) <= c.distance(e) + ON,
        _ => false,
    };
    if w.rims.iter().any(|(c, rim)| inside(*c, *rim)) {
        return true;
    }
    let centre = w.points.iter().find(|p| p.kind == SnapKind::Centre).and_then(|p| proj.to_screen(p.at));
    let on_edge = match &w.item {
        Some(Sel::Edge { point, .. }) => proj.to_screen(*point),
        _ => None,
    };
    match (centre, on_edge) {
        (Some(c), Some(e)) => pos.distance(c) <= c.distance(e) + ON,
        _ => false,
    }
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
