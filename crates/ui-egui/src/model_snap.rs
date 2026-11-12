//! Snapping while the cursor hovers the model: the place a click takes, locked onto the
//! conventional CAD snap points of whatever lies under the cursor — a corner, the middle of an
//! edge, the centre of a hole or of a planar face, a quarter turn round a circle — and sliding
//! along an edge or over a face when none of them is in reach. A glyph marks every place the
//! hovered edge or face offers, and the one the cursor has taken carries its label.
//!
//! The candidates are [`solidcraft_geom::snap`], the same ones the `model.snap` command answers
//! with, so a script and a hover agree on what is there. The ranking is in screen pixels: what
//! the cursor is near on screen is what it locks onto, whatever the zoom or the size of the part.
//!
//! The suppress-snap chord turns it off while held, the same chord that suppresses sketch
//! snapping (`prefs::suppress_sketch_snap`, read for both in [`crate::drag_snap::suppressed`]).
//! Under Plasticity's controls the chord is what asks for it, and without it nothing locks on.

use std::cell::RefCell;

use egui::{Align2, FontId, Pos2, Shape, Stroke};
use solidcore_record::Vec3;
use solidcraft_engine::geom::prelude::*;
use solidcraft_engine::geom::snap::{self, Circle, SnapKind};

use crate::SolveApp;
use crate::theme::Tokens;
use crate::viewport::{Hit, Proj};

/// How near the cursor an exact place has to be on screen to be taken (screen points).
const SNAP_PX: f32 = 10.0;
/// Half the width of a snap glyph (screen points).
const GLYPH_PX: f32 = 4.5;
/// Most hits one hover takes candidates from.
const MAX_HITS: usize = 8;
/// Most bounding edges of a hovered face that offer their places too.
const MAX_FACE_EDGES: usize = 24;
/// Most glyphs one hover draws, so a face with a long boundary does not litter the view.
const MAX_GLYPHS: usize = 16;

/// A place the cursor has locked onto, and where it is on screen.
#[derive(Clone, Debug, PartialEq)]
pub struct Snap {
    pub kind: SnapKind,
    pub at: Vec3,
    pub body: String,
    /// The mesh edge it came from.
    pub edge: Option<usize>,
    /// The B-rep face it came from.
    pub face: Option<u32>,
    /// The circle behind a centre or a quadrant.
    pub circle: Option<Circle>,
    pub screen: Pos2,
    /// The body and face index this place may stand in for as a place on a face: set only when
    /// the point really lies on that face, so a centre taken from the rim of a hole counts and
    /// the middle of an edge on the far side of the part does not.
    pub place_on: Option<(String, usize)>,
}

impl Snap {
    pub fn label(&self) -> &'static str {
        self.kind.label()
    }

    /// Whether this is an exact place rather than one that only slides along the geometry.
    pub fn exact(&self) -> bool {
        self.kind.tier() == 0
    }
}

thread_local! {
    /// The place the cursor has taken, and every place the hovered geometry offers.
    static CURRENT: RefCell<Option<Snap>> = const { RefCell::new(None) };
    static OFFERED: RefCell<Vec<Snap>> = const { RefCell::new(Vec::new()) };
    /// Set per frame by the viewport: what is under the cursor is only the thing being dragged,
    /// so the place it took stands (see [`set_hold`]).
    static HOLD: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Hold the place taken for this frame rather than losing it.
///
/// A snapped drag lands the thing it moves **on** the place, where it then covers it: the next
/// frame has nothing under the cursor but the mover itself, which offers no places of its own, so
/// the place would come and go frame by frame and shake the body between it and the free drag.
/// The viewport sets this where everything under the cursor is on the move, and only there: over
/// empty space there is nothing to hold and the drag is free again, which is what it should be.
pub fn set_hold(on: bool) {
    HOLD.with(|h| h.set(on));
}

pub fn current() -> Option<Snap> {
    CURRENT.with(|c| c.borrow().clone())
}

/// Every exact place the hovered geometry offers, for the glyphs.
pub fn offered() -> Vec<Snap> {
    OFFERED.with(|o| o.borrow().clone())
}

pub fn clear() {
    CURRENT.with(|c| *c.borrow_mut() = None);
    OFFERED.with(|o| o.borrow_mut().clear());
}

fn keep(best: Option<Snap>, offered: Vec<Snap>) -> Option<Snap> {
    if best.is_none() && HOLD.with(std::cell::Cell::get) {
        return current();
    }
    CURRENT.with(|c| *c.borrow_mut() = best.clone());
    OFFERED.with(|o| *o.borrow_mut() = offered);
    best
}

/// The place the cursor at `pos` locks onto, from what the pick found under it.
///
/// Ranked in screen points, so the tolerance is the same however far away the part is: an exact
/// place within [`SNAP_PX`] wins, and failing that the cursor slides along the edge or over the
/// face it is on.
pub fn find(app: &SolveApp, proj: &Proj, pos: Option<Pos2>, hits: &[Hit]) -> Option<Snap> {
    let Some(pos) = pos.filter(|_| !crate::drag_snap::suppressed()) else {
        return keep(None, Vec::new());
    };
    // An input that snaps face places to the points its own sketch shows (the Single hole's)
    // owns the cursor: its rule is narrower than this one on purpose (#657, #2110), and two
    // snaps competing for one pick would take it somewhere neither promised.
    if app.dialog.as_ref().and_then(|d| d.active_input()).is_some_and(|i| crate::dialogs::snaps_to_points(i.accept)) {
        return keep(None, Vec::new());
    }
    // Which places the preferences leave on (Preferences › Selection › Snap).
    let kinds = crate::prefs::snap_kinds(&app.preferences);
    if kinds == 0 {
        return keep(None, Vec::new());
    }
    let st = crate::open_meshing::shown_world(app);
    let (o, d) = proj.ray(pos);
    let mut found: Vec<Snap> = Vec::new();
    // The origin belongs to no body, so no hit ever names it; it is offered wherever it is shown.
    if kinds & snap::ORIGIN != 0 && crate::viewport::origin_point_shown(app) {
        found.extend(place(proj, "", &snap_at(SnapKind::Origin, Vec3::ZERO), None));
    }
    for h in hits.iter().take(MAX_HITS) {
        // `on_face` is the face a place from this hit may stand in for a pick on. It is known
        // here, from where the place came, and must not be worked out again from the point
        // afterwards: searching the hovered face for it reads every triangle, which on an
        // imported mesh is tens of milliseconds of every hover.
        let (body, cands, on_face) = match h {
            Hit::Vertex { body, point } if !body.is_empty() && kinds & snap::VERTEX != 0 => {
                found.extend(place(proj, body, &snap_at(SnapKind::Vertex, *point), None));
                continue;
            }
            Hit::Edge { body, index, .. } => {
                let Some(b) = st.body(body) else { continue };
                let m = b.mesh();
                // Along the edge, the place under the cursor is the point of it nearest the ray.
                let near = m.edges.get(*index).and_then(|pts| nearest_to_ray(pts, o, d)).unwrap_or(o);
                (body, snap::edge_candidates(&m, *index, near, kinds), None)
            }
            Hit::Face { body, index, point } => {
                let Some(b) = st.body(body) else { continue };
                let m = b.mesh();
                let Ok(face) = u32::try_from(*index) else { continue };
                // Only a planar face holds the places of its own boundary in its plane, so only
                // it can stand in for them: the centre of a hole is in the plane of the face the
                // hole goes through, and nowhere on the curved wall the hole is bored out of.
                let centre = snap::planar_face_center(&m, face);
                let mut cands = Vec::new();
                if kinds & snap::FACE_CENTER != 0
                    && let Some((c, _)) = centre
                {
                    cands.push(snap::SnapCandidate { face: Some(face), ..snap_at(SnapKind::FaceCenter, c) });
                }
                if kinds & snap::ON_FACE != 0 {
                    cands.push(snap::SnapCandidate { face: Some(face), ..snap_at(SnapKind::OnFace, *point) });
                }
                // A face also offers the places of the edges that bound it, so that the cursor
                // can be taken to them: the centre of a hole stays on offer as the cursor leaves
                // the rim for it, and hovering the wall of the hole offers the same centre.
                for e in m.face_edges(face).into_iter().take(MAX_FACE_EDGES) {
                    let near = m.edges.get(e).and_then(|pts| nearest_to_ray(pts, o, d)).unwrap_or(*point);
                    cands.extend(snap::edge_candidates(&m, e, near, kinds & snap::EXACT));
                }
                (body, cands, centre.map(|_| *index))
            }
            _ => continue,
        };
        for c in &cands {
            found.extend(place(proj, body, c, on_face));
        }
    }
    let at = Vec3::new(f64::from(pos.x), f64::from(pos.y), 0.0);
    let key = |s: &Snap| (s.kind, Vec3::new(f64::from(s.screen.x), f64::from(s.screen.y), 0.0));
    // Every exact place the hovered geometry offers gets a glyph, in reach of the cursor or not:
    // that is how the centre of a hole shows itself while the cursor is still out on the rim.
    let mut offered: Vec<Snap> = found.iter().filter(|s| s.exact()).cloned().collect();
    snap::rank_by(&mut offered, at, 1.0, key);
    offered.truncate(MAX_GLYPHS);
    // Only a place within reach on screen is taken. Failing that the cursor slides along the
    // edge or over the face it is on, which is always at no distance at all.
    found.retain(|s| !s.exact() || s.screen.distance(pos) <= SNAP_PX);
    snap::rank_by(&mut found, at, 1.0, key);
    keep(found.into_iter().next(), offered)
}

/// The hits that are not on one of `bodies`.
///
/// What is being dragged sits under the cursor the whole way, so a drag that snapped to it would
/// snap to itself and never move. The places it offers are kept out of the hover while it is the
/// thing on the move.
pub fn without(hits: &[Hit], bodies: &[String]) -> Vec<Hit> {
    if bodies.is_empty() {
        return hits.to_vec();
    }
    let moving = |b: &String| bodies.iter().any(|n| n == b);
    hits.iter()
        .filter(|h| match h {
            Hit::Vertex { body, .. } | Hit::Edge { body, .. } | Hit::Face { body, .. } => !moving(body),
            _ => true,
        })
        .cloned()
        .collect()
}

/// A bare candidate of a kind at a place, for a hit that already is one.
fn snap_at(kind: SnapKind, at: Vec3) -> snap::SnapCandidate {
    snap::SnapCandidate { kind, at, edge: None, face: None, circle: None }
}

/// A candidate with where it lands on screen, dropped when it lands nowhere.
fn place(proj: &Proj, body: &str, c: &snap::SnapCandidate, on_face: Option<usize>) -> Option<Snap> {
    Some(Snap {
        kind: c.kind,
        at: c.at,
        body: body.to_string(),
        edge: c.edge,
        face: c.face,
        circle: c.circle,
        screen: proj.to_screen(c.at)?,
        place_on: on_face.map(|i| (body.to_string(), i)),
    })
}

/// The point of a polyline nearest the ray `o` + `d`.
fn nearest_to_ray(pts: &[Vec3], o: Vec3, d: Vec3) -> Option<Vec3> {
    pts.windows(2)
        .map(|w| crate::viewport::ray_segment_closest(o, d, w[0], w[1]))
        .min_by(|a, b| ray_dist(o, d, *a).total_cmp(&ray_dist(o, d, *b)))
        .or_else(|| pts.first().copied())
}

fn ray_dist(o: Vec3, d: Vec3, p: Vec3) -> f64 {
    let t = (p - o).dot(d);
    p.dist(o + d * t)
}

/// The hits with a face place moved to the snapped point, so what a click takes is what the
/// hover showed.
///
/// Only a face place moves: the point of an edge or a vertex hit is how that edge or vertex is
/// found again later, not a place on it, and a snap must not rewrite an identity.
pub fn apply(hits: &[Hit], snap: Option<&Snap>) -> Vec<Hit> {
    let Some((on_body, on_index)) = snap.filter(|s| s.exact()).and_then(|s| s.place_on.clone()) else {
        return hits.to_vec();
    };
    let at = snap.map(|s| s.at);
    hits.iter()
        .map(|h| match (h, at) {
            (Hit::Face { body, index, .. }, Some(at)) if *body == on_body && *index == on_index => {
                Hit::Face { body: body.clone(), index: *index, point: at }
            }
            _ => h.clone(),
        })
        .collect()
}

/// The glyph for a kind: a square for an end, a triangle for a middle, a circle for a centre, a
/// diamond for a quarter turn, a circle with a dot for the centre of a face.
fn glyph(kind: SnapKind, at: Pos2, half: f32, stroke: Stroke) -> Vec<Shape> {
    let poly = |pts: Vec<Pos2>| vec![Shape::closed_line(pts, stroke)];
    match kind {
        // A cross through a square, so the origin is not taken for a corner.
        SnapKind::Origin => {
            let mut sh =
                poly(vec![at + egui::vec2(-half, -half), at + egui::vec2(half, -half), at + egui::vec2(half, half), at + egui::vec2(-half, half)]);
            sh.push(Shape::line_segment([at + egui::vec2(-half, 0.0), at + egui::vec2(half, 0.0)], stroke));
            sh.push(Shape::line_segment([at + egui::vec2(0.0, -half), at + egui::vec2(0.0, half)], stroke));
            sh
        }
        SnapKind::Vertex => {
            poly(vec![at + egui::vec2(-half, -half), at + egui::vec2(half, -half), at + egui::vec2(half, half), at + egui::vec2(-half, half)])
        }
        SnapKind::Midpoint => poly(vec![at + egui::vec2(0.0, -half), at + egui::vec2(half, half), at + egui::vec2(-half, half)]),
        SnapKind::Quadrant => {
            poly(vec![at + egui::vec2(0.0, -half), at + egui::vec2(half, 0.0), at + egui::vec2(0.0, half), at + egui::vec2(-half, 0.0)])
        }
        SnapKind::Center => vec![Shape::circle_stroke(at, half, stroke)],
        SnapKind::FaceCenter => vec![Shape::circle_stroke(at, half, stroke), Shape::circle_filled(at, (half * 0.3).max(1.0), stroke.color)],
        // A place that only slides needs no glyph of its own.
        SnapKind::OnEdge => vec![
            Shape::line_segment([at + egui::vec2(-half, -half), at + egui::vec2(half, half)], stroke),
            Shape::line_segment([at + egui::vec2(-half, half), at + egui::vec2(half, -half)], stroke),
        ],
        SnapKind::OnFace => Vec::new(),
    }
}

/// Draw the places the hovered geometry offers, and the label of the one the cursor has taken.
pub fn show(app: &SolveApp, painter: &egui::Painter) {
    // While a sketch point is being dragged the sketch snap owns the cursor.
    if app.viewport.point_drag.is_some() {
        return;
    }
    let tk = Tokens::get();
    let taken = current();
    for s in offered() {
        if taken.as_ref().is_some_and(|t| t.at.dist(s.at) < 1e-9) {
            continue;
        }
        for sh in glyph(s.kind, s.screen, GLYPH_PX, Stroke::new(1.0, tk.accent_soft)) {
            painter.add(sh);
        }
    }
    let Some(s) = taken else { return };
    for sh in glyph(s.kind, s.screen, GLYPH_PX + 1.5, Stroke::new(1.8, tk.accent)) {
        painter.add(sh);
    }
    let label = s.label();
    if !label.is_empty() {
        let w = 9.0 + 6.0 * label.len() as f32;
        let r = egui::Rect::from_min_size(s.screen + egui::vec2(12.0, -24.0), egui::vec2(w, 14.0));
        painter.rect_filled(r, 3.0, tk.accent_soft);
        painter.text(r.center(), Align2::CENTER_CENTER, label, FontId::proportional(tk.font_micro), tk.text);
    }
    crate::scenario::publish_count("model_snap_offered", offered().len() as f64);
}

#[cfg(test)]
#[path = "model_snap_tests.rs"]
mod tests;
