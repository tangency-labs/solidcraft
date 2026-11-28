//! Frame Member's hot points: while the dialog is open, the member's section is outlined at the
//! start of the first chosen line, placed as the member will be (angle and mirror applied), with a
//! dot at each anchor (the origin, the centre, the centroid and the 3 × 3 grid of the section's
//! bounding box). A click on a dot makes that point the one on the line, as the dialog's Anchor
//! does, and empties Offset A and B (the anchor wins). The line's marker drags over the section's
//! plane: let go near a dot it takes that anchor, anywhere else (inside the section or past it, so
//! a member can sit off its line) it puts that point on the line, written as Offset A and B from
//! the anchor (rounded to 0.1 mm unless Alt or Ctrl is held) and as `locate` (mm from the
//! profile's own origin, as the engine takes it, within `frame::MAX_PROFILE_SIZE`). Two arrows,
//! **A** and **B**, drag the section along its own axes (Offset A or B, rounded to 1 mm, 0.1 mm
//! with Alt or Ctrl; the other offset is kept), and an arc turns it about the point on the line in
//! 15° steps (1° with Alt or Ctrl), writing the dialog's Angle. Only the first line gets the
//! section; the anchor is one for every line.
//!
//! The section comes from the engine (`member_section_view`), laid out by the code that makes
//! the member, so what is drawn is where the member goes; one cache ([`section_view`]) serves the
//! view and the dialog's offsets. Unfinished or refused dialog values show nothing; a half-typed
//! offset leaves the last section. The hot points, arrows and arc are always drawn while the Frame
//! Member dialog is open: placing the section by dragging it is what the dialog is for, so there is
//! no option. The handles are published as `frame_anchor:<anchor>`, `frame_anchor:position`,
//! `frame_anchor:arrow_a`, `frame_anchor:arrow_b` and `frame_anchor:rotate` for `ui.handles`.

use std::cell::RefCell;
use std::sync::{Arc, Weak};

use egui::{Pos2, vec2};
use serde_json::Value;
use solidcore_record::{Vec2, Vec3};
use solidcraft_engine::doc::expr::Kind as ValueKind;
use solidcraft_engine::doc::frame::{Anchor, SectionView};
use solidcraft_engine::doc::{Document, ModelState};
use solidcraft_engine::geom::prelude::*;
use solidcraft_theme::Tokens;

use super::look::{
    CARDINALS, CrossStyle, MARKER_ACTIVE, MARKER_HOT, RING_ACTIVE, RING_OPEN, RingStyle, angle_label, core, crosshair, offset_arrow, ring,
};
use super::{on_plane, with_dialog};
use crate::SolveApp;
use crate::dialogs::{Dialog, Kind};
use crate::dialogs_frame::{ANCHORS, anchor_index, locate_for};
use crate::motion;
use crate::ui_kit::BackingTooltipExt as _;
use crate::viewport::Proj;

/// Reach of a hot point or the marker around its centre (pixels), and how near a dot the dragged
/// marker must come to take its anchor.
const REACH: f32 = 11.0;
/// Under this many pixels across, the section shows no dots, arrows or arc: they would sit on one
/// another.
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
/// An offset arrow starts this far out from the middle of the section's side, and is this long
/// (pixels); its press area runs along it, this far either side.
const ARROW_GAP: f32 = REACH + 2.0;
const ARROW_LEN: f32 = 45.0;
const ARROW_REACH: f32 = 9.0;
/// Around the drawn section, the arrows and the arc, presses belong to the hot points this far
/// out (pixels): one that misses a handle there does nothing, rather than box selecting and
/// emptying the dialog's Lines.
const OWN_MARGIN: f32 = 24.0;
/// An offset under this (mm) shows as its letter alone: it would read 0.0.
const SHOWN_ZERO: f64 = 0.05;
/// How far outside the section's farthest corner from the line the arc runs (pixels), clear of
/// that corner's dot.
const ARC_GAP: f32 = 2.0 * REACH + 4.0;
/// Steps a millimetre of a drag: an arrow's offset (1 mm, with Alt or Ctrl 0.1 mm) and the
/// marker's (0.1 mm, with Alt or Ctrl free).
const ARROW_STEPS: f64 = 1.0;
const FINE_STEPS: f64 = 10.0;
/// The arc's step (degrees), and with Alt or Ctrl.
const TURN_STEP_DEG: f64 = 15.0;
const FINE_TURN_STEP_DEG: f64 = 1.0;
/// Sections kept laid out: the view's (with `locate`) and the dialog's (without), and a few more.
const CACHED: usize = 4;
/// The tooltips.
const DOT_TIP: &str = "ui.frame_hot_point_tip";
const MARKER_TIP: &str = "ui.frame_locate_tip";
const ARROW_TIPS: [&str; 2] = ["ui.frame_arrow_a_tip", "ui.frame_arrow_b_tip"];
const ROTATE_TIP: &str = "ui.frame_rotate_tip";
/// The arrows' letters, as in the dialog's Offset A and Offset B, and their handles' names.
const ARROW_LETTERS: [&str; 2] = ["A", "B"];
const ARROW_HANDLES: [&str; 2] = ["frame_anchor:arrow_a", "frame_anchor:arrow_b"];

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
    /// The section's bounding box: its bottom left and top right corners.
    lo: Vec2,
    hi: Vec2,
}

/// A section the engine laid out for one set of `frame.member` parameters, with what it was
/// worked out from. The design and model are held weakly, for identity only: they are freed once
/// the dialog or the design closes.
struct Cached {
    params: String,
    doc: Weak<Document>,
    state: Weak<ModelState>,
    component: u64,
    view: Result<SectionView, String>,
    /// The overlay's drawing of it, worked out the first time it is drawn.
    shown: Option<Option<Shown>>,
}

thread_local! {
    /// The sections asked for lately, oldest first, so they are not laid out again every frame
    /// (the view's hot points and the dialog's offsets share them).
    static CACHE: RefCell<Vec<Cached>> = const { RefCell::new(Vec::new()) };
    /// The section as it was when a drag (the marker, an arrow, the arc) began: the drag reads the
    /// pointer against it, not against the section that moves as the dialog's values change.
    static FROZEN: RefCell<Option<Shown>> = const { RefCell::new(None) };
    /// Where the hot points, the marker, the arrows and the arc's grip were drawn last frame
    /// (screen), for [`contains_pointer`]: navigation and Select Other run before the overlay is
    /// drawn.
    static DRAWN: RefCell<Vec<Pos2>> = const { RefCell::new(Vec::new()) };
    /// The area round the section, the arrows and the arc whose presses the hot points took last
    /// frame (screen), for [`contains_pointer`].
    static OWNED: RefCell<Vec<egui::Rect>> = const { RefCell::new(Vec::new()) };
}

/// Is `p` on a handle of the open Frame Member dialog's section, or in the area round the section,
/// its arrows and its arc that the hot points own (as drawn last frame)?
pub(crate) fn contains_pointer(app: &SolveApp, p: Pos2) -> bool {
    app.applying.is_none()
        && app.dialog.as_ref().is_some_and(|d| serves(app, d))
        && (DRAWN.with(|h| h.borrow().iter().any(|c| press_area(*c).contains(p))) || OWNED.with(|o| o.borrow().iter().any(|r| r.contains(p))))
}

/// The press area of a hot point, the marker or the rotation handle centred at `c` (screen): the
/// square [`contains_pointer`] tests, corners included.
fn press_area(c: Pos2) -> egui::Rect {
    egui::Rect::from_center_size(c, vec2(2.0 * REACH, 2.0 * REACH))
}

/// `f` on the cached section of `frame.member` parameters `p`, laid out now when no section of
/// these parameters, design, model and component is kept.
fn cached<T>(app: &SolveApp, p: &Value, f: impl FnOnce(&mut Cached) -> T) -> Option<T> {
    let params = p.to_string();
    let s = &app.session;
    let state = s.model.state();
    CACHE.with(|c| {
        let mut c = c.borrow_mut();
        c.retain(|entry| {
            entry.component == s.active_component
                && std::ptr::eq(entry.doc.as_ptr(), Arc::as_ptr(&s.doc))
                && std::ptr::eq(entry.state.as_ptr(), Arc::as_ptr(&state))
        });
        match c.iter().position(|entry| entry.params == params) {
            Some(i) => {
                let hit = c.remove(i);
                c.push(hit);
            }
            None => {
                let view = solidcraft_engine::cmd::member_section_view(s, p).map_err(|err| err.to_string());
                if c.len() >= CACHED {
                    c.remove(0);
                }
                c.push(Cached {
                    params,
                    doc: Arc::downgrade(&s.doc),
                    state: Arc::downgrade(&state),
                    component: s.active_component,
                    view,
                    shown: None,
                });
            }
        }
        c.last_mut().map(f)
    })
}

/// The section `frame.member` parameters `p` put on the first line, or why there is none: the
/// one cache the dialog's offsets and the view's hot points share.
pub(crate) fn section_view(app: &SolveApp, p: &Value) -> Result<SectionView, String> {
    cached(app, p, |entry| entry.view.clone()).unwrap_or_else(|| Err("no section".into()))
}

/// The `frame.member` parameters of the group the dialog shows, as one member; none while they
/// are unfinished. A half-typed offset leaves the group's last applied `locate` in them, so the
/// section stays in view.
fn params(app: &SolveApp, d: &Dialog) -> Option<Value> {
    let Kind::Frame(m) = &d.kind else { return None };
    crate::dialogs_frame::shown_params(app, m, &d.inputs, &d.extra).ok()
}

/// The section the dialog's values put on the first line, as the overlay draws it.
fn section(app: &SolveApp, d: &Dialog) -> Option<Shown> {
    let p = params(app, d)?;
    cached(app, &p, |entry| {
        let view = &entry.view;
        entry.shown.get_or_insert_with(|| view.clone().ok().and_then(laid_out)).clone()
    })
    .flatten()
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
    solidcraft_kernel::points::point_distance([a.x, a.y, 0.0], [b.x, b.y, 0.0]).ok().map(|d| d.value)
}

/// How far apart two screen points are (pixels), as the kernel measures it; infinite where it
/// refuses them, so such a point is never the nearest nor within reach.
fn px_apart(a: Pos2, b: Pos2) -> f64 {
    let lift = |p: Pos2| [f64::from(p.x), f64::from(p.y), 0.0];
    solidcraft_kernel::points::point_distance(lift(a), lift(b)).map_or(f64::INFINITY, |d| d.value)
}

/// The angle (radians, in (−π, π], counter-clockwise from the section's x axis) of section
/// point `p` about `at`: the kernel's turn from that axis to `p - at`, negative below the axis.
/// None where `p` is on `at` or not finite.
fn angle_about(at: Vec2, p: Vec2) -> Option<f64> {
    let (dx, dy) = (p.x - at.x, p.y - at.y);
    let turn = solidcraft_kernel::planar::joint_turn([1.0, 0.0], [dx, dy]).ok()?;
    Some(if dy < 0.0 { -turn } else { turn })
}

/// The unit normal of the section's plane: the plane is square to the member's line, so its
/// direction, made unit by the kernel's length of it. It runs along the line either way (x × y
/// runs back along it for a member Auto lays against its line, or in a mirrored component); both
/// of its uses ([`on_plane`] and the edge-on test) take either sign alike. None for a line with no
/// length.
fn plane_normal(view: &SectionView) -> Option<Vec3> {
    let len = solidcraft_kernel::points::point_distance([view.start.x, view.start.y, view.start.z], [view.end.x, view.end.y, view.end.z]).ok()?.value;
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
        let Some(index) = anchor_index(a) else { continue };
        let Some((name, label)) = ANCHORS.get(index).copied() else { continue };
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
    Some(Shown { view, loops, dots, points, lo, hi })
}

/// `v` rounded to `per_mm` steps a millimetre (none: as it is), without a negative zero.
fn rounded(v: f64, per_mm: Option<f64>) -> f64 {
    match per_mm {
        Some(n) => (v * n).round() / n + 0.0,
        None => v,
    }
}

/// How far the drawn section stands from its anchor in use: Offset A and B as they are applied.
fn drawn_offset(sh: &Shown, anchor: usize) -> Option<[f64; 2]> {
    let a = sh.points.get(anchor).copied().flatten()?;
    let off = [a.x - sh.view.at.x, a.y - sh.view.at.y];
    off.iter().all(|c| c.is_finite()).then_some(off)
}

/// Make the anchor at `index` the point on the line: Offset A and B emptied and a located point
/// dropped (the anchor wins).
fn set_anchor(d: &mut Dialog, index: usize) {
    if let Kind::Frame(m) = &mut d.kind {
        m.anchor = index;
        m.offset = Default::default();
        m.locate = None;
    }
    d.error = None;
}

/// Move the section `off` (Offset A and B, mm) from its anchor: the offsets marked in `write`
/// written as lengths (the others kept as typed) and `locate` with them, so the section is drawn
/// there at once. Nothing past the largest profile; true when written.
fn set_offset(app: &SolveApp, d: &mut Dialog, view: &SectionView, off: [f64; 2], write: [bool; 2]) -> bool {
    let Kind::Frame(m) = &mut d.kind else { return false };
    let Some(at) = locate_for(view, m.anchor, off) else { return false };
    for ((text, v), w) in m.offset.iter_mut().zip(off).zip(write) {
        if w {
            *text = crate::length_input::model(&app.session.doc, v);
        }
    }
    m.locate = Some(at);
    d.error = None;
    true
}

/// An offset's text in mm (empty is 0), none while it is not a length within the largest profile.
fn offset_value(app: &SolveApp, text: &str) -> Option<f64> {
    let text = text.trim();
    if text.is_empty() {
        return Some(0.0);
    }
    crate::length_input::eval(&app.session.doc, text, ValueKind::Length).ok().filter(|v| solidcraft_engine::doc::frame::within_profile_size(*v))
}

/// `v` rounded to a millionth, so a value at the press plus whole steps reads as typed plus the
/// steps (`21.3 mm`, `22 deg`), not as the float noise of the sum.
fn tidy(v: f64) -> f64 {
    (v * 1e6).round() / 1e6 + 0.0
}

/// An arrow's label: its letter alone while its offset is 0 or empty, else the letter and the
/// offset in the design's unit, 0.1 mm shown (`A 5.0 mm`).
fn arrow_label(app: &SolveApp, letter: &str, mm: f64) -> String {
    if !mm.is_finite() || mm.abs() < SHOWN_ZERO {
        return letter.to_string();
    }
    format!("{letter} {}", crate::length_input::model_drag(&app.session.doc, mm, 0.1, 1))
}

/// The dialog's Angle (radians; empty is 0), none when it is not an angle.
fn angle_of(app: &SolveApp, text: &str) -> Option<f64> {
    let text = text.trim();
    if text.is_empty() {
        return Some(0.0);
    }
    crate::length_input::eval(&app.session.doc, text, ValueKind::Angle).ok().filter(|a| a.is_finite())
}

/// `a` (radians) brought into (−π, π].
fn wrapped(a: f64) -> f64 {
    use std::f64::consts::{PI, TAU};
    PI - (PI - a).rem_euclid(TAU)
}

/// An offset arrow on screen: its tail, its tip, and the point that takes a press (its middle).
#[derive(Clone, Copy)]
struct Arrow {
    tail: Pos2,
    tip: Pos2,
    grip: Pos2,
}

/// The arc that turns the section: a circle about the point on the line in the section's plane,
/// past the section's farthest corner from it.
#[derive(Clone, Copy)]
struct Turn {
    /// The circle's radius (mm).
    radius: f64,
    /// Millimetres a screen point there.
    per_px: f64,
    /// Where its grip is (radians, in the section's axes as drawn).
    grip: f64,
}

/// The section moved as a drag under way draws it: turned by `turn` (radians) about the point on
/// the line, then moved `shift` (mm) along its axes.
#[derive(Clone, Copy)]
struct Moved {
    turn: f64,
    shift: [f64; 2],
}

impl Moved {
    const NONE: Moved = Moved { turn: 0.0, shift: [0.0, 0.0] };

    fn apply(&self, at: Vec2, p: Vec2) -> Vec2 {
        let (c, s) = (self.turn.cos(), self.turn.sin());
        let (dx, dy) = (p.x - at.x, p.y - at.y);
        Vec2::new(at.x + dx * c - dy * s + self.shift[0], at.y + dx * s + dy * c + self.shift[1])
    }

    fn any(&self) -> bool {
        self.turn != 0.0 || self.shift != [0.0, 0.0]
    }
}

/// The section on screen.
struct Screen {
    loops: Vec<Vec<Pos2>>,
    dots: Vec<(Dot, Pos2)>,
    arrows: [Option<Arrow>; 2],
    /// Its size across (pixels).
    across: f32,
}

/// Where the section, its dots and its arrows are on screen, moved by `moved`.
fn on_screen(sh: &Shown, proj: &Proj, moved: Moved) -> Screen {
    let at = sh.view.at;
    let to_screen = |p: Vec2| proj.to_screen(sh.view.world(moved.apply(at, p)));
    let loops: Vec<Vec<Pos2>> = sh.loops.iter().filter_map(|l| l.iter().map(|p| to_screen(*p)).collect::<Option<Vec<Pos2>>>()).collect();
    let bounds = loops.iter().flatten().fold(egui::Rect::NOTHING, |r, p| r.union(egui::Rect::from_min_max(*p, *p)));
    let across = if bounds.is_positive() { bounds.width().max(bounds.height()) } else { 0.0 };
    let dots = sh.dots.iter().filter_map(|dot| to_screen(dot.at).map(|s| (dot.clone(), s))).collect();
    // Out from the middle of the section's +A side and of its +B side, the ways they move it.
    let mid = Vec2::new((sh.lo.x + sh.hi.x) / 2.0, (sh.lo.y + sh.hi.y) / 2.0);
    let arrow = |base: Vec2, ahead: Vec2| -> Option<Arrow> {
        let (b, a) = (to_screen(base)?, to_screen(ahead)?);
        let dir = a - b;
        let len = dir.length();
        if !(len.is_finite() && len > 1e-4) {
            return None;
        }
        let dir = dir / len;
        let tail = b + dir * ARROW_GAP;
        Some(Arrow { tail, tip: tail + dir * ARROW_LEN, grip: tail + dir * (ARROW_LEN / 2.0) })
    };
    let arrows =
        [arrow(Vec2::new(sh.hi.x, mid.y), Vec2::new(sh.hi.x + 1.0, mid.y)), arrow(Vec2::new(mid.x, sh.hi.y), Vec2::new(mid.x, sh.hi.y + 1.0))];
    Screen { loops, dots, arrows, across }
}

/// The arc for the section: [`ARC_GAP`] screen points past its corner farthest from the point on
/// the line, its grip that way; none when the view gives no scale there.
fn turn_of(sh: &Shown, proj: &Proj) -> Option<Turn> {
    let at = sh.view.at;
    let corners = [sh.hi, Vec2::new(sh.lo.x, sh.hi.y), Vec2::new(sh.hi.x, sh.lo.y), sh.lo];
    let mut far = (sh.hi, apart(sh.hi, at)?);
    for c in corners {
        let d = apart(c, at)?;
        if d > far.1 + SAME {
            far = (c, d);
        }
    }
    let screen = |p: Vec2| proj.to_screen(sh.view.world(p));
    let (s0, sx, sy) = (screen(at)?, screen(Vec2::new(at.x + 1.0, at.y))?, screen(Vec2::new(at.x, at.y + 1.0))?);
    let px_per_mm = px_apart(sx, s0).max(px_apart(sy, s0));
    if !(px_per_mm.is_finite() && px_per_mm > 1e-9) {
        return None;
    }
    let per_px = 1.0 / px_per_mm;
    let radius = far.1 + f64::from(ARC_GAP) * per_px;
    let grip = angle_about(at, far.0)?;
    [radius, per_px, grip].iter().all(|v| v.is_finite()).then_some(Turn { radius, per_px, grip })
}

/// The screen point of the arc's plane at angle `a` (radians) about the point on the line, `r_px`
/// out from the circle and `t_px` along it (screen points), as [`ring`] takes it.
fn arc_point(view: &SectionView, proj: &Proj, arc: Turn, a: f64, r_px: f32, t_px: f32) -> Option<Pos2> {
    let (r, t) = (arc.radius + f64::from(r_px) * arc.per_px, f64::from(t_px) * arc.per_px);
    let (c, s) = (a.cos(), a.sin());
    proj.to_screen(view.world(Vec2::new(view.at.x + r * c - t * s, view.at.y + r * s + t * c)))
}

/// What can take a press this frame.
enum Target {
    Dot(usize),
    Marker,
    Arrow(usize),
    Rotate,
}

/// Draw the section and its hot points for the open Frame Member dialog and take their clicks and
/// the marker's, the arrows' and the arc's drags. Called from the tail of `canvas::show`, after
/// the placement handles.
pub(crate) fn show(app: &mut SolveApp, ui: &mut egui::Ui, painter: &egui::Painter, proj: &Proj) {
    // Owned only on frames the section is drawn: every early return below leaves nothing owned.
    DRAWN.with(|h| h.borrow_mut().clear());
    OWNED.with(|o| o.borrow_mut().clear());
    let marker_id = egui::Id::unique("sc_frame_anchor_position");
    let arrow_ids = [egui::Id::unique("sc_frame_anchor_arrow_a"), egui::Id::unique("sc_frame_anchor_arrow_b")];
    let rotate_id = egui::Id::unique("sc_frame_anchor_rotate");
    let Some(d) = app.dialog.as_ref().filter(|d| serves(app, d)) else {
        FROZEN.with(|f| *f.borrow_mut() = None);
        return;
    };
    let Kind::Frame(m) = &d.kind else { return };
    let (anchor, angle) = (m.anchor, angle_of(app, &m.angle));
    let ctx = ui.ctx().clone();
    let dragging = [marker_id, rotate_id].into_iter().chain(arrow_ids).any(|id| ctx.is_being_dragged(id));
    let frozen = if dragging { FROZEN.with(|f| f.borrow().clone()) } else { None };
    if !dragging {
        FROZEN.with(|f| *f.borrow_mut() = None);
    }
    let Some(sh) = frozen.or_else(|| section(app, d)) else { return };
    let Some(start) = proj.to_screen(sh.view.start) else { return };
    let Some(n) = plane_normal(&sh.view) else { return };
    let t = Tokens::get();
    let at_rest = on_screen(&sh, proj, Moved::NONE);
    // Seen edge-on, a pointer on the section's plane is centimetres a pixel: nothing takes a press.
    let ray = proj.ray(start).1;
    let live = solidcraft_kernel::affine::distance_along([0.0; 3], [n.x, n.y, n.z], [ray.x, ray.y, ray.z]).abs() >= 0.05;
    // Too small on screen, the dots, arrows and arc would sit on one another: none of them.
    let handles = live && at_rest.across >= MIN_ACROSS;
    let dots: Vec<(Dot, Pos2)> = if handles { at_rest.dots.clone() } else { Vec::new() };
    let arrows = if handles { at_rest.arrows } else { [None, None] };
    let arc = if handles && angle.is_some() { turn_of(&sh, proj) } else { None };

    // Press areas, farthest from the pointer first so the nearest takes the press; the marker
    // last, so it wins a tie with the dot it sits on (that dot only takes clicks, the marker drags).
    let mut order: Vec<(Target, Pos2)> = dots.iter().enumerate().map(|(i, (_, s))| (Target::Dot(i), *s)).collect();
    for (i, a) in arrows.iter().enumerate() {
        if let Some(a) = a {
            order.push((Target::Arrow(i), a.grip));
        }
    }
    if let Some(g) = arc.and_then(|a| arc_point(&sh.view, proj, a, a.grip, 0.0, 0.0)) {
        order.push((Target::Rotate, g));
    }
    if live {
        order.push((Target::Marker, start));
    }
    if let Some(p) = ctx.pointer_latest_pos() {
        order.sort_by(|a, b| px_apart(b.1, p).total_cmp(&px_apart(a.1, p)));
    }
    let arrow_area = |a: &Arrow| egui::Rect::from_two_pos(a.tail, a.tip).expand(ARROW_REACH);
    // The area the hot points own, under their handles: a press there that takes no handle does
    // nothing (no box selection, no pick).
    let mut owned: Vec<egui::Rect> = Vec::new();
    if handles {
        let section = at_rest
            .loops
            .iter()
            .flatten()
            .chain(dots.iter().map(|(_, s)| s))
            .fold(egui::Rect::NOTHING, |r, p| r.union(egui::Rect::from_min_max(*p, *p)));
        if section.is_positive() {
            owned.push(section);
        }
        owned.extend(arrows.iter().flatten().map(arrow_area));
        if let Some(a) = arc {
            let half = std::f64::consts::FRAC_PI_4 * 1.2;
            let pts: Vec<Pos2> =
                (0..=12).filter_map(|k| arc_point(&sh.view, proj, a, a.grip - half + 2.0 * half * f64::from(k) / 12.0, 0.0, 0.0)).collect();
            let bounds = pts.iter().fold(egui::Rect::NOTHING, |r, p| r.union(egui::Rect::from_min_max(*p, *p)));
            if bounds.is_finite() && bounds.width() >= 0.0 {
                owned.push(bounds);
            }
        }
    }
    let owned: Vec<egui::Rect> = owned.into_iter().map(|r| r.expand(OWN_MARGIN)).filter(|r| r.is_finite()).collect();
    for (k, r) in owned.iter().enumerate() {
        ui.interact(*r, egui::Id::unique(("sc_frame_anchor_owned", k)), egui::Sense::click_and_drag());
    }
    OWNED.with(|o| *o.borrow_mut() = owned);
    let mut dot_hits: Vec<Option<egui::Response>> = vec![None; dots.len()];
    let mut arrow_hits: [Option<egui::Response>; 2] = [None, None];
    let mut marker: Option<egui::Response> = None;
    let mut rotate: Option<egui::Response> = None;
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
            Target::Arrow(i) => {
                if let (Some(slot), Some(id), Some(Some(a))) = (arrow_hits.get_mut(i), arrow_ids.get(i), arrows.get(i)) {
                    *slot = Some(ui.interact(arrow_area(a), *id, egui::Sense::drag()));
                }
            }
            Target::Rotate => rotate = Some(ui.interact(press_area(at), rotate_id, egui::Sense::drag())),
            Target::Marker => marker = Some(ui.interact(press_area(at), marker_id, egui::Sense::drag())),
        }
    }
    if marker.iter().chain(arrow_hits.iter().flatten()).chain(rotate.iter()).any(egui::Response::dragged) {
        crate::tool_modifiers::set_manipulator_dragging(true);
        FROZEN.with(|f| {
            let mut f = f.borrow_mut();
            if f.is_none() {
                *f = Some(sh.clone());
            }
        });
    }
    let free = crate::tool_modifiers::manipulator_free();
    let section_at = |p: Pos2| on_plane(proj, p, sh.view.start, n).map(|q| sh.view.section_point(q)).filter(|p| finite2(*p));
    let to_screen = |p: Vec2| proj.to_screen(sh.view.world(p));

    // The marker's drag: the pointer on the section's plane as it was when the drag began, pulled
    // onto a dot within reach, else that point of the plane, inside the section or past it.
    let mut marker_at = start;
    let mut snapped: Option<usize> = None;
    // The offsets a drag under way gives, for the arrows' labels.
    let mut live_offset: Option<[f64; 2]> = None;
    if let Some(r) = marker.as_ref() {
        let off = motion::grab(ui, egui::Id::unique("sc_frame_anchor_grab"), r, |p| Some(p - start));
        if r.dragged()
            && let Some(p) = r.interact_pointer_pos()
        {
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
                live_offset = Some([0.0, 0.0]);
            } else if let Some(p) = section_at(target)
                && let Some(a) = sh.points.get(anchor).copied().flatten()
            {
                let steps = (!free).then_some(FINE_STEPS);
                let off = [rounded(a.x - p.x, steps), rounded(a.y - p.y, steps)];
                let view = sh.view.clone();
                let mut done = false;
                with_dialog(app, |app, d| done = set_offset(app, d, &view, off, [true, true]));
                if done {
                    live_offset = Some(off);
                }
                marker_at = to_screen(p).unwrap_or(marker_at);
            }
        }
    }

    // An arrow's drag: whole steps of how far the pointer has gone along the arrow's axis on the
    // section's plane since the press, added to the offset as it was at the press (as typed when
    // it is a length), so a press that moves no step changes nothing and a drag back to no step
    // puts back the text of the press. Only this offset's text is written; the other's stays as
    // typed.
    let mut moved = Moved::NONE;
    let offset_texts = match app.dialog.as_ref().map(|d| &d.kind) {
        Some(Kind::Frame(m)) => m.offset.clone(),
        _ => Default::default(),
    };
    let drawn = drawn_offset(&sh, anchor);
    let values: Vec<Option<f64>> =
        (0..2).map(|i| offset_texts.get(i).and_then(|t| offset_value(app, t)).or_else(|| drawn.and_then(|o| o.get(i).copied()))).collect();
    let at_press = |i: usize| values.get(i).copied().flatten();
    for (i, (r, id)) in arrow_hits.iter().zip(arrow_ids).enumerate() {
        let Some(r) = r else { continue };
        let from = motion::grab(ui, id.with("grab"), r, |p| {
            let q = section_at(p)?;
            Some((if i == 0 { q.x } else { q.y }, at_press(i)?, offset_texts.get(i).cloned().unwrap_or_default()))
        });
        if !r.dragged() {
            continue;
        }
        let (Some((pressed, was, typed)), Some(q), Some(other)) = (from, r.interact_pointer_pos().and_then(section_at), at_press(1 - i.min(1)))
        else {
            continue;
        };
        let now = if i == 0 { q.x } else { q.y };
        let per_mm = if free { FINE_STEPS } else { ARROW_STEPS };
        let steps = rounded(now - pressed, Some(per_mm));
        let v = tidy(was + steps);
        let off = if i == 0 { [v, other] } else { [other, v] };
        // No step: the text of the press, as it was typed.
        let text = if steps == 0.0 { typed } else { crate::length_input::model(&app.session.doc, v) };
        let view = sh.view.clone();
        let mut done = offset_texts.get(i) == Some(&text);
        if !done {
            with_dialog(app, |app, d| {
                done = set_offset(app, d, &view, off, [false, false]);
                if let Kind::Frame(m) = &mut d.kind
                    && let Some(slot) = m.offset.get_mut(i)
                    && done
                {
                    *slot = text;
                }
            });
        }
        if done && let (Some(slot), Some(d)) = (moved.shift.get_mut(i), drawn.and_then(|o| o.get(i).copied())) {
            *slot = v - d;
        }
        live_offset = Some(off);
    }

    // The arc's drag: whole steps of the pointer's turn about the point on the line since the
    // press, added to the Angle as it was at the press (kept with the press: the dialog's changes
    // as it turns), so a press that turns no step changes nothing and a turn back to no step puts
    // back the Angle's text of the press.
    let mut turned_to: Option<f64> = None;
    if let (Some(r), Some(angle)) = (rotate.as_ref(), angle) {
        let at = sh.view.at;
        let pointer_angle = |p: Pos2| section_at(p).and_then(|q| angle_about(at, q)).filter(|a| a.is_finite());
        let typed = match app.dialog.as_ref().map(|d| &d.kind) {
            Some(Kind::Frame(m)) => m.angle.clone(),
            _ => String::new(),
        };
        let from = motion::grab(ui, rotate_id.with("grab"), r, |p| pointer_angle(p).map(|a| (a, angle, typed)));
        if r.dragged()
            && let (Some((from, angle, typed)), Some(now)) = (from, r.interact_pointer_pos().and_then(pointer_angle))
        {
            let step = if free { FINE_TURN_STEP_DEG } else { TURN_STEP_DEG };
            let steps = (wrapped(now - from).to_degrees() / step).round() * step;
            let deg = tidy(wrapped((angle.to_degrees() + steps).to_radians()).to_degrees());
            if deg.is_finite() {
                let text = if steps == 0.0 { typed } else { format!("{deg} deg") };
                with_dialog(app, |_, d| {
                    if let Kind::Frame(m) = &mut d.kind
                        && m.angle != text
                    {
                        m.angle = text;
                        d.error = None;
                    }
                });
                // The frozen section is the one at the press: turned by the steps.
                moved.turn = steps.to_radians();
                turned_to = Some(deg.to_radians());
            }
        }
    }

    // While an arrow or the arc drags, the section is drawn where it is going.
    let shown = if moved.any() { on_screen(&sh, proj, moved) } else { at_rest };
    let dots: Vec<(Dot, Pos2)> = if handles { shown.dots } else { Vec::new() };
    let arrows = if handles { shown.arrows } else { [None, None] };

    // What the dialog holds now: the anchor in use is ringed, none while a point is located.
    let (anchor, located) = match app.dialog.as_ref().map(|d| &d.kind) {
        Some(Kind::Frame(m)) => (m.anchor, m.locate.is_some()),
        _ => (0, false),
    };
    let active_at = if located { None } else { sh.points.get(anchor).copied().flatten() };
    let is_active = |p: Vec2| active_at.and_then(|a| apart(a, p)).is_some_and(|gap| gap <= SAME);

    // The outline: a proposal, so the manipulator's line, not a handle's white.
    for l in &shown.loops {
        let mut closed = l.clone();
        if let Some(first) = l.first() {
            closed.push(*first);
        }
        painter.add(egui::Shape::line(closed, egui::Stroke::new(OUTLINE, t.manipulator)));
    }
    let (case, white) = (t.icons.marker_halo, core(true));
    for (i, (dot, s)) in dots.iter().enumerate() {
        let r = dot_hits.get(i).and_then(Option::as_ref);
        let hovered = r.is_some_and(egui::Response::hovered) || snapped == Some(i);
        let hot = MARKER_HOT.run(&ctx, egui::Id::unique(("sc_frame_anchor_hot", dot.name)), hovered).t;
        let radius = DOT_R + (DOT_HOT_R - DOT_R) * hot;
        if is_active(dot.at) {
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

    // The arrows, each with its letter.
    for (i, a) in arrows.iter().enumerate() {
        let (Some(a), Some(r), Some(letter), Some(handle), Some(tip)) =
            (a, arrow_hits.get(i).and_then(Option::as_ref), ARROW_LETTERS.get(i), ARROW_HANDLES.get(i), ARROW_TIPS.get(i))
        else {
            continue;
        };
        let (hovered, dragging) = (r.hovered(), r.dragged());
        let hot = MARKER_HOT.run(&ctx, egui::Id::unique(("sc_frame_anchor_arrow_hot", i)), hovered || dragging).t;
        let value = live_offset.or(drawn).and_then(|o| o.get(i).copied()).unwrap_or(0.0);
        let label = arrow_label(app, letter, value);
        offset_arrow(painter, a.tail, a.tip, hot, &label, &t);
        r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &label));
        crate::scenario::publish_handle(handle, a.grip);
        DRAWN.with(|h| h.borrow_mut().push(a.grip));
        if hovered && !dragging {
            r.clone().backing_on_hover_text(app.tr(tip));
        }
        if hovered || dragging {
            ctx.set_cursor_icon(if dragging { egui::CursorIcon::Grabbing } else { egui::CursorIcon::Move });
        }
    }

    // The arc: the placement ring's look, about the point on the line.
    if let (Some(arc), Some(r), Some(angle)) = (arc, rotate.as_ref(), angle) {
        let view = &sh.view;
        let on = |a: f64, r_px: f32, t_px: f32| arc_point(view, proj, arc, a, r_px, t_px);
        let grip = arc.grip + moved.turn;
        let (hovered, dragging) = (r.hovered(), r.dragged());
        let open = RING_OPEN.run(&ctx, egui::Id::unique("sc_frame_anchor_ring_hot"), hovered || dragging);
        let style = RingStyle {
            hot: open.t,
            active: RING_ACTIVE.run(&ctx, egui::Id::unique("sc_frame_anchor_ring_active"), dragging).t,
            fading: open.fading(),
            alpha: 1.0,
            tint: None,
        };
        ring(painter, &on, grip, &style);
        if let Some(g) = on(grip, 0.0, 0.0) {
            crate::scenario::publish_handle("frame_anchor:rotate", g);
            DRAWN.with(|h| h.borrow_mut().push(g));
            if dragging {
                angle_label(painter, g, start, turned_to.unwrap_or(angle), None, &t);
            }
        }
        if hovered && !dragging {
            r.clone().backing_on_hover_text(app.tr(ROTATE_TIP));
        }
        if hovered || dragging {
            ctx.set_cursor_icon(if dragging { egui::CursorIcon::Grabbing } else { egui::CursorIcon::Grab });
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
