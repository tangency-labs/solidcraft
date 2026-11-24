//! Frame Member's hot points: while the dialog is open, the member's section is outlined at the
//! start of the first chosen line, placed as the member will be (angle and mirror applied), with a
//! dot at each anchor (the origin, the centre, the centroid and the 3 × 3 grid of the section's
//! bounding box). A click on a dot makes that point the one on the line, as the dialog's Anchor
//! does, and drops a `locate` the feature had (it would hide the anchor). The line's marker drags
//! over the section's plane: let go near a dot it takes that anchor, anywhere else (inside the
//! section or past it, so a member can sit off its line) it puts that point on the line
//! (`locate`, mm from the profile's own origin, rounded to 0.1 mm unless Alt or Ctrl is held;
//! within `frame::MAX_PROFILE_SIZE`, as the engine takes it). Only the first line gets the section; the anchor is one for every line.
//!
//! The section comes from the engine (`member_section_view`), laid out by the code that makes
//! the member, so what is drawn is where the member goes. Unfinished or refused dialog values show
//! nothing. The hot points are always on while the Frame Member dialog is open: placing the
//! section by dragging it is what the dialog is for, so there is no option. The handles
//! are published as `frame_anchor:<anchor>` and `frame_anchor:position` for `ui.handles`.

use std::cell::RefCell;
use std::sync::{Arc, Weak};

use egui::{Pos2, vec2};
use serde_json::Value;
use solidcore_record::{Vec2, Vec3};
use solidcraft_engine::doc::frame::{Anchor, SectionView};
use solidcraft_engine::doc::{Document, ModelState};
use solidcraft_engine::geom::prelude::*;
use solidcraft_theme::Tokens;

use super::look::{CARDINALS, CrossStyle, MARKER_ACTIVE, MARKER_HOT, core, crosshair};
use super::{on_plane, with_dialog};
use crate::SolveApp;
use crate::dialogs::{Dialog, Kind};
use crate::dialogs_frame::ANCHORS;
use crate::motion;
use crate::ui_kit::BackingTooltipExt as _;
use crate::viewport::Proj;

/// Reach of a hot point or the marker around its centre (pixels), and how near a dot the dragged
/// marker must come to take its anchor.
const REACH: f32 = 11.0;
/// Under this many pixels across, the section shows no dots: they would sit on one another.
const MIN_ACROSS: f32 = 30.0;
/// A hot point's radius at rest and under the pointer (pixels).
const DOT_R: f32 = 3.2;
const DOT_HOT_R: f32 = 5.5;
/// How far the ring round the anchor in use stands off its dot (pixels).
const ACTIVE_RING: f32 = 3.0;
/// Hot points closer than this (mm) are one: the first in `Anchor::ALL` order keeps it.
const SAME: f64 = 1e-6;
/// Caps on what a hostile section can make the overlay draw.
const MAX_LOOPS: usize = 64;
const MAX_LOOP_POINTS: usize = 2048;
/// The outline's line width.
const OUTLINE: f32 = 1.4;
/// The tooltips.
const DOT_TIP: &str = "ui.frame_hot_point_tip";
const MARKER_TIP: &str = "ui.frame_locate_tip";

/// Is this a Frame Member dialog (whose hot points are drawn)?
fn serves(_app: &SolveApp, d: &Dialog) -> bool {
    matches!(d.kind, Kind::Frame(_))
}

/// A hot point: its index in the dialog's `ANCHORS` (= `Anchor::ALL`), its name, label and point.
#[derive(Clone, Debug)]
struct Dot {
    index: usize,
    name: &'static str,
    label: &'static str,
    at: Vec2,
}

/// The section as the overlay draws it, worked out once per set of dialog values.
#[derive(Clone, Debug)]
struct Shown {
    view: SectionView,
    /// Each loop of the section as section points.
    loops: Vec<Vec<Vec2>>,
    /// The hot points, coinciding ones dropped.
    dots: Vec<Dot>,
    /// Every anchor's point by its index in `ANCHORS`, coinciding or not.
    points: Vec<Option<Vec2>>,
}

/// The section of the last values asked for, with what it was worked out from. The design and
/// model are held weakly: for identity only, they stay freed once the dialog or the design closes.
struct Cached {
    key: String,
    doc: Weak<Document>,
    state: Weak<ModelState>,
    component: u64,
    shown: Option<Shown>,
}

thread_local! {
    /// The section of the dialog's values last frame, so it is not laid out again every frame.
    static CACHE: RefCell<Option<Cached>> = const { RefCell::new(None) };
    /// The section as it was when the marker's drag began: the drag reads the pointer against it,
    /// not against the section that moves as `locate` changes.
    static FROZEN: RefCell<Option<Shown>> = const { RefCell::new(None) };
    /// Where the hot points and the marker were drawn last frame (screen), for
    /// [`contains_pointer`]: navigation and Select Other run before the overlay is drawn.
    static DRAWN: RefCell<Vec<Pos2>> = const { RefCell::new(Vec::new()) };
}

/// Is `p` on a hot point or the marker of the open Frame Member dialog (as drawn last frame)? The
/// same square as the press area they register (`press_area`), corners included.
pub(crate) fn contains_pointer(app: &SolveApp, p: Pos2) -> bool {
    app.applying.is_none()
        && app.dialog.as_ref().is_some_and(|d| serves(app, d))
        && DRAWN.with(|h| h.borrow().iter().any(|c| press_area(*c).contains(p)))
}

/// The press area of a hot point or the marker centred at `c` (screen).
fn press_area(c: Pos2) -> egui::Rect {
    egui::Rect::from_center_size(c, vec2(2.0 * REACH, 2.0 * REACH))
}

/// The `frame.member` parameters of the group the dialog shows, as one member; none while it is
/// unfinished.
fn params(app: &SolveApp, d: &Dialog) -> Option<Value> {
    let Kind::Frame(m) = &d.kind else { return None };
    crate::dialogs_frame::shown_params(app, m, &d.inputs, &d.extra).ok()
}

/// The section the dialog's values put on the first line, from the cache when neither the values
/// nor the model changed since it was worked out.
fn section(app: &SolveApp, d: &Dialog) -> Option<Shown> {
    let p = params(app, d)?;
    let key = p.to_string();
    let s = &app.session;
    let state = s.model.state();
    let hit = CACHE.with(|c| {
        c.borrow()
            .as_ref()
            .filter(|c| {
                c.key == key
                    && c.component == s.active_component
                    && std::ptr::eq(c.doc.as_ptr(), Arc::as_ptr(&s.doc))
                    && std::ptr::eq(c.state.as_ptr(), Arc::as_ptr(&state))
            })
            .map(|c| c.shown.clone())
    });
    if let Some(shown) = hit {
        return shown;
    }
    let shown = solidcraft_engine::cmd::frame::member_section_view(s, &p).ok().and_then(laid_out);
    CACHE.with(|c| {
        *c.borrow_mut() =
            Some(Cached { key, doc: Arc::downgrade(&s.doc), state: Arc::downgrade(&state), component: s.active_component, shown: shown.clone() })
    });
    shown
}

fn finite2(p: Vec2) -> bool {
    p.x.is_finite() && p.y.is_finite()
}

fn finite3(p: Vec3) -> bool {
    p.x.is_finite() && p.y.is_finite() && p.z.is_finite()
}

/// How far apart two section points are (mm), as the kernel measures it; none for a coordinate
/// that is not finite or a distance that overflows.
fn apart(a: Vec2, b: Vec2) -> Option<f64> {
    solidcore_ops::point_distance([a.x, a.y, 0.0], [b.x, b.y, 0.0]).ok().map(|d| d.value)
}

/// How far apart two screen points are (pixels), as the kernel measures it; infinite where it
/// refuses them, so such a point is never the nearest nor within reach.
fn px_apart(a: Pos2, b: Pos2) -> f64 {
    let lift = |p: Pos2| [f64::from(p.x), f64::from(p.y), 0.0];
    solidcore_ops::point_distance(lift(a), lift(b)).map_or(f64::INFINITY, |d| d.value)
}

/// The unit normal of the section's plane: the plane is square to the member's line, so its
/// direction, made unit by the kernel's length of it. It runs along the line either way (x × y
/// runs back along it for a member Auto lays against its line, or in a mirrored component); both
/// of its uses ([`on_plane`] and the edge-on test) take either sign alike. None for a line with no
/// length.
fn plane_normal(view: &SectionView) -> Option<Vec3> {
    let len = solidcore_ops::point_distance([view.start.x, view.start.y, view.start.z], [view.end.x, view.end.y, view.end.z]).ok()?.value;
    (len.is_finite() && len > 0.0).then(|| (view.end - view.start) * (1.0 / len)).filter(|n| finite3(*n))
}

/// The overlay's section from the engine's: none when anything in it is not a finite number or
/// the section has no size.
fn laid_out(view: SectionView) -> Option<Shown> {
    if ![view.start, view.end, view.x, view.y].into_iter().all(finite3) || !finite2(view.at) || !finite2(view.origin) {
        return None;
    }
    let anchors = view.anchors();
    let corner = |a: Anchor| anchors.iter().find(|(b, _)| *b == a).map(|(_, p)| *p).filter(|p| finite2(*p));
    let (lo, hi) = (corner(Anchor::BottomLeft)?, corner(Anchor::TopRight)?);
    let size = apart(lo, hi)?;
    if !(size.is_finite() && size > SAME) {
        return None;
    }
    let tol = size * 1e-3;
    let mut loops = Vec::new();
    for l in view.regions.iter().flat_map(|r| std::iter::once(&r.outer).chain(&r.holes)).take(MAX_LOOPS) {
        let pts = l.polyline(tol).into_vec();
        let step = pts.len() / MAX_LOOP_POINTS + 1;
        let pts: Vec<Vec2> = pts.into_iter().step_by(step).collect();
        if pts.len() >= 2 && pts.iter().all(|p| finite2(*p)) {
            loops.push(pts);
        }
    }
    let mut points = vec![None; ANCHORS.len()];
    let mut dots: Vec<Dot> = Vec::new();
    for (a, at) in anchors {
        let Some(name) = serde_json::to_value(a).ok().and_then(|v| v.as_str().map(str::to_string)) else { continue };
        let Some((index, (name, label))) = ANCHORS.iter().enumerate().find(|(_, (n, _))| *n == name) else { continue };
        if !finite2(at) {
            continue;
        }
        if let Some(slot) = points.get_mut(index) {
            *slot = Some(at);
        }
        if dots.iter().all(|d| apart(d.at, at).is_none_or(|gap| gap > SAME)) {
            dots.push(Dot { index, name, label, at });
        }
    }
    Some(Shown { view, loops, dots, points })
}

/// `v` rounded to 0.1 mm, without a negative zero.
fn tenth(v: f64) -> f64 {
    (v * 10.0).round() / 10.0 + 0.0
}

/// Make the anchor at `index` the point on the line, dropping a located point.
fn set_anchor(d: &mut Dialog, index: usize) {
    if let Kind::Frame(m) = &mut d.kind {
        m.anchor = index;
        m.locate = None;
    }
    d.error = None;
}

/// Put section point `p` on the line: `locate`, measured as the member reads it.
fn set_locate(d: &mut Dialog, view: &SectionView, p: Vec2, free: bool) {
    let [x, y] = view.locate_of(p);
    let (x, y) = if free { (x, y) } else { (tenth(x), tenth(y)) };
    let limit = solidcraft_engine::doc::frame::MAX_PROFILE_SIZE;
    if x.is_finite()
        && y.is_finite()
        && x.abs() <= limit
        && y.abs() <= limit
        && let Kind::Frame(m) = &mut d.kind
    {
        m.locate = Some([x, y]);
        d.error = None;
    }
}

/// What can take a press this frame.
enum Target {
    Dot(usize),
    Marker,
}

/// Draw the section and its hot points for the open Frame Member dialog and take their clicks and
/// the marker's drag. Called from the tail of `canvas::show`, after the placement handles.
pub(crate) fn show(app: &mut SolveApp, ui: &mut egui::Ui, painter: &egui::Painter, proj: &Proj) {
    DRAWN.with(|h| h.borrow_mut().clear());
    let marker_id = egui::Id::unique("sc_frame_anchor_position");
    let Some(d) = app.dialog.as_ref().filter(|d| serves(app, d)) else {
        FROZEN.with(|f| *f.borrow_mut() = None);
        return;
    };
    let dragging = ui.ctx().is_being_dragged(marker_id);
    let frozen = if dragging { FROZEN.with(|f| f.borrow().clone()) } else { None };
    if !dragging {
        FROZEN.with(|f| *f.borrow_mut() = None);
    }
    let Some(sh) = frozen.or_else(|| section(app, d)) else { return };
    let Some(start) = proj.to_screen(sh.view.start) else { return };
    let Some(n) = plane_normal(&sh.view) else { return };
    let t = Tokens::get();
    let to_screen = |p: Vec2| proj.to_screen(sh.view.world(p));
    let loops: Vec<Vec<Pos2>> = sh.loops.iter().filter_map(|l| l.iter().map(|p| to_screen(*p)).collect::<Option<Vec<Pos2>>>()).collect();
    let bounds = loops.iter().flatten().fold(egui::Rect::NOTHING, |r, p| r.union(egui::Rect::from_min_max(*p, *p)));
    let across = if bounds.is_positive() { bounds.width().max(bounds.height()) } else { 0.0 };
    // Seen edge-on, a pointer on the section's plane is centimetres a pixel: nothing takes a press.
    let ray = proj.ray(start).1;
    let live = solidcore_ops::affine::distance_along([0.0; 3], [n.x, n.y, n.z], [ray.x, ray.y, ray.z]).abs() >= 0.05;
    let dots: Vec<(Dot, Pos2)> = if live && across >= MIN_ACROSS {
        sh.dots.iter().filter_map(|dot| to_screen(dot.at).map(|s| (dot.clone(), s))).collect()
    } else {
        Vec::new()
    };

    // Press areas, farthest from the pointer first so the nearest takes the press; the marker
    // last, so it wins a tie with the dot it sits on (that dot only takes clicks, the marker drags).
    let mut order: Vec<(Target, Pos2)> = dots.iter().enumerate().map(|(i, (_, s))| (Target::Dot(i), *s)).collect();
    if live {
        order.push((Target::Marker, start));
    }
    if let Some(p) = ui.ctx().pointer_latest_pos() {
        order.sort_by(|a, b| px_apart(b.1, p).total_cmp(&px_apart(a.1, p)));
    }
    let mut dot_hits: Vec<Option<egui::Response>> = vec![None; dots.len()];
    let mut marker: Option<egui::Response> = None;
    for (target, at) in order {
        match target {
            Target::Dot(i) => {
                let Some((dot, _)) = dots.get(i) else { continue };
                let r = ui.interact(press_area(at), egui::Id::unique(("sc_frame_anchor_dot", dot.name)), egui::Sense::click());
                if r.clicked() {
                    let index = dot.index;
                    with_dialog(app, |_, d| set_anchor(d, index));
                }
                if let Some(slot) = dot_hits.get_mut(i) {
                    *slot = Some(r);
                }
            }
            Target::Marker => marker = Some(ui.interact(press_area(at), marker_id, egui::Sense::drag())),
        }
    }

    // The marker's drag: the pointer on the section's plane as it was when the drag began, pulled
    // onto a dot within reach, else that point of the plane, inside the section or past it.
    let mut marker_at = start;
    let mut snapped: Option<usize> = None;
    if let Some(r) = marker.as_ref() {
        let off = motion::grab(ui, egui::Id::unique("sc_frame_anchor_grab"), r, |p| Some(p - start));
        if r.dragged() {
            crate::tool_modifiers::set_manipulator_dragging(true);
            FROZEN.with(|f| {
                let mut f = f.borrow_mut();
                if f.is_none() {
                    *f = Some(sh.clone());
                }
            });
            if let Some(p) = r.interact_pointer_pos() {
                let target = p - off.unwrap_or_default();
                let near = dots
                    .iter()
                    .enumerate()
                    .filter(|(_, (_, s))| px_apart(*s, target) <= f64::from(REACH))
                    .min_by(|a, b| px_apart(a.1.1, target).total_cmp(&px_apart(b.1.1, target)));
                if let Some((i, (dot, s))) = near {
                    let index = dot.index;
                    with_dialog(app, |_, d| set_anchor(d, index));
                    marker_at = *s;
                    snapped = Some(i);
                } else if let Some(q) = on_plane(proj, target, sh.view.start, n) {
                    let p = sh.view.section_point(q);
                    if finite2(p) {
                        let free = crate::tool_modifiers::manipulator_free();
                        let view = sh.view.clone();
                        with_dialog(app, |_, d| set_locate(d, &view, p, free));
                        marker_at = to_screen(p).unwrap_or(marker_at);
                    }
                }
            }
        }
    }

    // What the dialog holds now: the anchor in use is ringed, none while a point is located.
    let (anchor, located) = match app.dialog.as_ref().map(|d| &d.kind) {
        Some(Kind::Frame(m)) => (m.anchor, m.locate.is_some()),
        _ => (0, false),
    };
    let active_at = if located { None } else { sh.points.get(anchor).copied().flatten() };

    // The outline: a proposal, so the manipulator's line, not a handle's white.
    for l in &loops {
        let mut closed = l.clone();
        if let Some(first) = l.first() {
            closed.push(*first);
        }
        painter.add(egui::Shape::line(closed, egui::Stroke::new(OUTLINE, t.manipulator)));
    }
    let ctx = ui.ctx().clone();
    let (case, white) = (t.icons.marker_halo, core(true));
    for (i, (dot, s)) in dots.iter().enumerate() {
        let r = dot_hits.get(i).and_then(Option::as_ref);
        let hovered = r.is_some_and(egui::Response::hovered) || snapped == Some(i);
        let hot = MARKER_HOT.run(&ctx, egui::Id::unique(("sc_frame_anchor_hot", dot.name)), hovered).t;
        let radius = DOT_R + (DOT_HOT_R - DOT_R) * hot;
        if active_at.and_then(|a| apart(a, dot.at)).is_some_and(|gap| gap <= SAME) {
            painter.circle_stroke(*s, radius + ACTIVE_RING, egui::Stroke::new(OUTLINE + 0.6, t.manipulator));
        }
        painter.circle_filled(*s, radius + 0.4, case);
        painter.circle_filled(*s, radius, white);
        crate::scenario::publish_handle(&format!("frame_anchor:{}", dot.name), *s);
        DRAWN.with(|h| h.borrow_mut().push(*s));
        if let Some(r) = r
            && r.hovered()
        {
            r.clone().backing_on_hover_text(format!("{}\n{}", app.tr_label(dot.label), app.tr(DOT_TIP)));
            ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
        }
    }
    let Some(r) = marker else { return };
    crate::scenario::publish_handle("frame_anchor:position", marker_at);
    DRAWN.with(|h| h.borrow_mut().push(start));
    let (hovered, dragging) = (r.hovered(), r.dragged());
    let style = CrossStyle {
        hot: MARKER_HOT.run(&ctx, egui::Id::unique("sc_frame_anchor_marker_hot"), hovered || dragging).t,
        active: MARKER_ACTIVE.run(&ctx, egui::Id::unique("sc_frame_anchor_marker_active"), dragging).t,
        alpha: 1.0,
    };
    crosshair(painter, marker_at, &style, &CARDINALS, &t);
    if hovered && !dragging {
        r.clone().backing_on_hover_text(app.tr(MARKER_TIP));
    }
    if hovered || dragging {
        ctx.set_cursor_icon(if dragging { egui::CursorIcon::Grabbing } else { egui::CursorIcon::Move });
    }
}

#[cfg(test)]
#[path = "frame_anchor_tests.rs"]
mod tests;
