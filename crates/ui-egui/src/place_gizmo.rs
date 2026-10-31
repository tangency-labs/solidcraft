//! Handles that place a feature on a face and turn it in the face.
//!
//! A dashed crosshair at the feature's place drags it across the model, near-white so the blue
//! preview it sits on does not hide it, cased in a darker edge so it also carries on a light body.
//! Under the pointer its lines grow, it shows the move cursor and a tooltip. While it drags, the
//! feature's footprint is outlined where the marker is: the solid preview is recomputed off the
//! window thread and trails the pointer by its boolean, the outline does not. A feature that can
//! turn in its face also gets a ring lying on the face about the same centre, a fixed size on
//! screen like the move triad's arrows; dragging its grip turns the feature about the face normal.
//! Nothing runs until OK: the handles edit the open dialog, which the live preview follows, so a
//! new feature and an Edit Feature behave alike.
//!
//! The mechanics live here; each tool that uses them is one file under `place_gizmo/` that gives a
//! [`Spec`]: where the place is, how a pointer re-places it, the footprint, and the turn if any.
//! [`spec`] picks the file by the dialog kind. The handles are published as `<name>:position` and
//! `<name>:rotate` for `ui.handles`.
//!
//! The handles are behind the `placement_handles` preference (Preferences > Design > Placement), off by
//! default: off, a feature is placed by its click only, as before, and its Position input takes faces
//! only. A Single hole's markers (#6818) stay behind Create Hole › Move holes by clicking or dragging
//! instead, as the hole's own handle was; each tool's [`Spec::serves`] says what turns it on.
//!
//! A tool has one place (a Rest) or one per item of its first input (a Single hole, a marker on each
//! hole placed): [`Spec::place`] says where they come from, and a drag or a click names the place by
//! its key. Most tools keep their place as the face item of one input ([`PlaceSource::Input`]): the
//! marker reads it from there and moves it by replacing the item, as a click on the input does.

mod boss;
mod hole;
mod look;
mod rest;
mod snap_fit;

use egui::{Pos2, vec2};
use look::{
    CARDINALS, CrossStyle, DIM, DIM_S, MARKER_ACTIVE, MARKER_HOT, RING_ACTIVE, RING_OPEN, RING_PX, RingStyle, angle_label, compass, crosshair,
    footprint, ring,
};
use solidcraft_engine::doc::expr::Kind as ValueKind;
use solidcraft_engine::geom::Vec3;
use solidcraft_engine::{Sel, Session};

use crate::SolveApp;
use crate::dialogs::Dialog;
pub(crate) use crate::dialogs::vec3_of;
use crate::motion;
pub(crate) use crate::selection::point_value as pt;
use crate::theme::Tokens;
use crate::viewport::{Hit, Proj};

/// One tool's use of the handles.
///
/// While `places`, `footprint` and the turn's `reach` and `angle` run, the dialog is in the app;
/// inside `move_to`, `click` and [`Turn::set`] it is taken out (`app.dialog` is `None`): read the
/// session and `d` there, never `app.dialog`.
pub(crate) struct Spec {
    /// Handle names (`<name>:position`, `<name>:rotate`; `<name>:<n>` with `many`) and widget ids.
    pub name: &'static str,
    /// Is this dialog one of the tool's, with the tool's handles turned on?
    pub serves: fn(&SolveApp, &Dialog) -> bool,
    /// The tool's commands whose face place also snaps to sketch points on the face while the
    /// handles are on (the place input then takes points, as a Single hole's does).
    pub snap_commands: &'static [&'static str],
    /// The move marker's tooltip.
    pub move_tip: &'static str,
    /// A marker on each of the tool's places, named `<name>:<n>` by their order, with no ring
    /// (a Single hole's; `turn` is then ignored). Off: one place, `<name>:position`, and the ring
    /// if any.
    pub many: bool,
    /// Where the places the handles stand at come from, and how a drag moves one.
    pub place: PlaceSource,
    /// What a click (a press without a drag) on the marker at `key` does: a Single hole is taken
    /// out. `None`: a click on the marker does nothing.
    pub click: Option<fn(&SolveApp, &mut Dialog, usize)>,
    /// The footprint outlined while dragging, in world points (empty: no outline).
    pub footprint: fn(&SolveApp, &Dialog, Vec3, Vec3) -> Vec<Vec3>,
    /// How freely the place at `key` moves on its face: anywhere, or along a line (a hole held at
    /// a distance from one reference edge). The marker shows it as its arms, and does not take a
    /// drag when the line cannot be dragged as the view shows it.
    pub freedom: fn(&SolveApp, &Dialog, usize) -> Freedom,
    pub turn: Option<Turn>,
}

/// Where a tool keeps its place, so where the marker reads it and how a drag moves it.
pub(crate) enum PlaceSource {
    /// The face item of the dialog's input `n` (a Rest's, Boss's or Snap Fit's Position input):
    /// the picked point, growing along the face's normal, else the stored `direction` when the
    /// face is gone. A drag replaces the item where a click on that input would put it, snapped
    /// as the click is, carrying `direction` and the in-face `along` onto the new face.
    Input(usize),
    /// The tool's own: `places` lists them; `move_to` re-places the one at `key` where a click
    /// with these hits (the pick under the pointer, taken while the dialog is open, so it snaps
    /// as a click does) would put it, false when nothing there can take it.
    Own { places: fn(&SolveApp, &Dialog) -> Vec<Place>, move_to: fn(&SolveApp, &mut Dialog, &[Hit], usize) -> bool },
}

impl Spec {
    /// The places the handles stand at.
    fn places(&self, app: &SolveApp, d: &Dialog) -> Vec<Place> {
        match self.place {
            PlaceSource::Input(i) => one(input_frame(app, d, i)),
            PlaceSource::Own { places, .. } => places(app, d),
        }
    }

    /// Re-place the feature at `key` where a click with these hits would put it.
    fn move_to(&self, app: &SolveApp, d: &mut Dialog, hits: &[Hit], key: usize) -> bool {
        match self.place {
            PlaceSource::Input(i) => input_move(app, d, hits, i),
            PlaceSource::Own { move_to, .. } => move_to(app, d, hits, key),
        }
    }

    /// The input whose rules the marker's pick follows: the place's, else the first (a Single
    /// hole's places are the items of its first input).
    fn place_input(&self) -> usize {
        match self.place {
            PlaceSource::Input(i) => i,
            PlaceSource::Own { .. } => 0,
        }
    }
}

/// The place of a tool kept in its input `i`: the face item's point and the direction the
/// feature grows, the face's normal, else the stored `direction`.
fn input_frame(app: &SolveApp, d: &Dialog, i: usize) -> Option<(Vec3, Vec3)> {
    let Sel::Face { body, index, point } = d.inputs.get(i)?.items.first()? else { return None };
    let n = crate::dialogs::planar_face(&app.session, body, *index)
        .map(|(_, n)| n)
        .or_else(|| d.extra.get("direction").and_then(vec3_of))
        .and_then(|n| n.normalized())?;
    Some((*point, n))
}

/// The move of a tool kept in its input `i`: the item replaced where a click with `hits` would
/// put it (snapped to a sketch point as a click is when the input takes points), with the stored
/// `direction` and in-face `along` carried onto the new face. False when nothing there can take it.
fn input_move(app: &SolveApp, d: &mut Dialog, hits: &[Hit], i: usize) -> bool {
    let s = &app.session;
    let turned = input_frame(app, d, i).and_then(|(_, n)| d.extra.get("along").and_then(vec3_of).map(|a| angle_of(a, n)));
    let Some(to) = face_pick(d, s, hits) else { return false };
    if let Sel::Face { body, index, .. } = &to
        && let Some((_, nn)) = crate::dialogs::planar_face(s, body, *index)
    {
        if d.extra.contains_key("direction") {
            d.extra.insert("direction".into(), pt(nn));
        }
        if let Some(a) = turned {
            d.extra.insert("along".into(), pt(dir_at(nn, a)));
        }
    }
    if let Some(inp) = d.inputs.get_mut(i) {
        inp.items = vec![to];
    }
    d.error = None;
    true
}

/// The ways a place can go on its face.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Freedom {
    /// Anywhere on the face.
    Plane,
    /// Along this world direction only (unit), in the face.
    Line(Vec3),
}

/// Every place of a tool moves anywhere on its face.
pub(crate) fn anywhere(_: &SolveApp, _: &Dialog, _: usize) -> Freedom {
    Freedom::Plane
}

/// One place a tool's handles stand at.
pub(crate) struct Place {
    /// Which of the tool's places: the item's index in the first input for a tool with `many`,
    /// 0 otherwise.
    pub key: usize,
    /// The place and the direction the feature grows out of its face.
    pub base: Vec3,
    pub n: Vec3,
}

/// A one-place tool's places from its frame: one, or none.
pub(crate) fn one(frame: Option<(Vec3, Vec3)>) -> Vec<Place> {
    frame.map(|(base, n)| Place { key: 0, base, n }).into_iter().collect()
}

/// Are the shared handles on (Preferences > Design > Placement handles)? The gate of every
/// tool but the hole, whose markers keep the hole's own setting.
pub(crate) fn on(app: &SolveApp) -> bool {
    app.preferences.placement_handles
}

/// The face place a click with `hits` would add to the dialog's first input: snapped to a sketch
/// point as a click is when the input takes points; none when nothing there is a face.
pub(crate) fn face_pick(d: &Dialog, s: &Session, hits: &[Hit]) -> Option<Sel> {
    let snap = d.face_snap(s, hits);
    hits.iter().find_map(|h| d.candidate_on(s, h, snap.as_ref()).filter(|c| matches!(c, Sel::Face { .. })))
}

/// A ring whose grip turns the feature about its face normal.
pub(crate) struct Turn {
    pub tip: &'static str,
    /// How far the feature reaches from its place along its turn (world); `None` hides the ring
    /// (a round Rest has nothing to turn). The ring's size does not follow it.
    pub reach: fn(&SolveApp, &Dialog) -> Option<f64>,
    /// The current turn about `n`, from [`axes`]'s x towards its y.
    pub angle: fn(&SolveApp, &Dialog, Vec3) -> f64,
    /// Turn to the pointer's angle (`free` when Alt or Ctrl is held); returns the angle taken.
    pub set: fn(&SolveApp, &mut Dialog, Vec3, f64, bool) -> f64,
}

/// Every tool with handles, one file each under `place_gizmo/`, one per line in name order so
/// tools added on separate branches merge without touching each other's line.
#[rustfmt::skip]
static TOOLS: &[&Spec] = &[
    &boss::SPEC,
    &hole::SPEC,
    &rest::SPEC,
    &snap_fit::SPEC,
];

thread_local! {
    /// Where the handles were drawn last frame (screen), and whether each takes a press, for
    /// [`contains_pointer`]: navigation and Select Other's press-and-hold run before the handles
    /// are drawn. A place held still is drawn (the value box keeps off it) but takes no press.
    static DRAWN: std::cell::RefCell<Vec<(Pos2, bool)>> = const { std::cell::RefCell::new(Vec::new()) };
    /// The ring's screen centre last frame, for [`yield_box`].
    static RING_ABOUT: std::cell::Cell<Option<Pos2>> = const { std::cell::Cell::new(None) };
}

/// Is `p` on a placement handle of the open dialog (as drawn last frame)?
pub(crate) fn contains_pointer(app: &SolveApp, p: Pos2) -> bool {
    app.dialog.as_ref().is_some_and(|d| spec(app, d).is_some()) && drawn_at(p)
}

/// Is `p` within reach of a handle drawn last frame? For the dialog's own manipulators, which run
/// before the handles and would otherwise take a press meant for a handle their stem crosses.
pub(crate) fn drawn_at(p: Pos2) -> bool {
    DRAWN.with(|h| h.borrow().iter().any(|(c, live)| *live && c.distance(p) <= REACH))
}

/// The tool whose dialog this is, if it has handles and they are on.
fn spec(app: &SolveApp, d: &Dialog) -> Option<&'static Spec> {
    TOOLS.iter().copied().find(|s| (s.serves)(app, d))
}

/// How much of the screen around the base the dialog's drag arrow leaves to the move marker
/// (pixels): the marker's full reach and a gap, so the arrow stands above it. Zero without handles.
pub(crate) fn arrow_clearance(app: &SolveApp, d: &Dialog) -> f32 {
    if spec(app, d).is_some() { look::MARKER_CLEAR } else { 0.0 }
}

/// Does this command's face place also snap to sketch points on the face, so its marker can be
/// dragged onto one?
pub(crate) fn snaps_to_points(app: &SolveApp, id: &str) -> bool {
    app.preferences.placement_handles && TOOLS.iter().any(|s| s.snap_commands.contains(&id))
}

/// Reach of a handle around its centre (pixels): wide, so a finger finds it.
const REACH: f32 = 18.0;
/// Points on a round footprint's outline.
const FOOTPRINT_SEGS: usize = 48;

/// A length expression's value (mm), zero when it does not evaluate.
pub(crate) fn len(app: &SolveApp, e: &str) -> f64 {
    app.session.doc.eval(e, ValueKind::Length).ok().filter(|v| v.is_finite()).unwrap_or(0.0).abs()
}

/// In-plane axes about `n`: x is the engine's default in-face direction (its `any_perp`), y
/// completes them, so angle 0 is an untouched feature.
pub(crate) fn axes(n: Vec3) -> (Vec3, Vec3) {
    let x = n.any_perp();
    (x, n.cross(x))
}

/// The direction in the face at `angle` about `n` from [`axes`]'s x.
pub(crate) fn dir_at(n: Vec3, angle: f64) -> Vec3 {
    let (x, y) = axes(n);
    x * angle.cos() + y * angle.sin()
}

/// The angle of `v` about `n` from [`axes`]'s x.
pub(crate) fn angle_of(v: Vec3, n: Vec3) -> f64 {
    let (x, y) = axes(n);
    v.dot(y).atan2(v.dot(x))
}

/// `deg` rounded to `step` degrees unless `free`.
pub(crate) fn stepped(deg: f64, step: f64, free: bool) -> f64 {
    if free { deg } else { (deg / step).round() * step }
}

/// The face's four compass points: the in-face directions along the world axes, as the Snap Fit
/// combo names them (`+X`, `−Y`, …), each with its angle about `n` from [`axes`]'s x.
pub(crate) fn cardinals(n: Vec3) -> Vec<(String, f64)> {
    crate::dialogs_plastic::hook_dirs(n).into_iter().map(|(name, d)| (name, angle_of(d, n))).collect()
}

/// Within this many degrees of a compass point the turn is pulled onto it; turning freely (Alt or
/// Ctrl held) keeps only a hair of it, so an axis can still be hit exactly.
const PULL_DEG: f64 = 8.0;
const PULL_FREE_DEG: f64 = 0.5;

/// `deg` pulled onto a compass point of the face when within [`PULL_DEG`] of one, otherwise
/// rounded to `step`; `free` leaves it alone but for the hair.
pub(crate) fn pulled(deg: f64, n: Vec3, step: f64, free: bool) -> f64 {
    let off = |a: f64| ((a - deg + 180.0).rem_euclid(360.0) - 180.0).abs();
    let pull = if free { PULL_FREE_DEG } else { PULL_DEG };
    match cardinals(n).into_iter().map(|(_, a)| a.to_degrees()).min_by(|a, b| off(*a).total_cmp(&off(*b))) {
        Some(a) if off(a) <= pull => a,
        _ => stepped(deg, step, free),
    }
}

/// The colour of the world axis a compass point's name ends in (the one set of axis colours).
fn axis_colour(name: &str, t: &Tokens) -> egui::Color32 {
    match name.chars().last() {
        Some('X') => t.axis_x,
        Some('Y') => t.axis_y,
        _ => t.axis_z,
    }
}

/// The compass point `angle` sits on, if any.
fn at_cardinal(n: Vec3, angle: f64) -> Option<String> {
    cardinals(n)
        .into_iter()
        .find(|(_, a)| ((a - angle + std::f64::consts::PI).rem_euclid(std::f64::consts::TAU) - std::f64::consts::PI).abs() < 1e-6)
        .map(|(name, _)| name)
}

/// A circle of radius `r` about `base` across `n`.
pub(crate) fn circle(base: Vec3, n: Vec3, r: f64) -> Vec<Vec3> {
    let (x, y) = axes(n);
    (0..FOOTPRINT_SEGS)
        .map(|i| {
            let a = std::f64::consts::TAU * i as f64 / FOOTPRINT_SEGS as f64;
            base + (x * a.cos() + y * a.sin()) * r
        })
        .collect()
}

/// A rectangle about `base` across `n`: `hl` either way along `a`, `hw` either way across it.
pub(crate) fn rect(base: Vec3, n: Vec3, a: Vec3, hl: f64, hw: f64) -> Vec<Vec3> {
    let b = n.cross(a);
    [(1.0, 1.0), (-1.0, 1.0), (-1.0, -1.0), (1.0, -1.0)].into_iter().map(|(sa, sb)| base + a * (sa * hl) + b * (sb * hw)).collect()
}

/// Where the pointer's ray meets the plane through `base` across `n`, none near edge-on, where a
/// plane intersection magnifies subpixel pointer error.
fn on_plane(proj: &Proj, pos: Pos2, base: Vec3, n: Vec3) -> Option<Vec3> {
    if proj.ray(pos).1.dot(n).abs() < 0.05 {
        return None;
    }
    crate::gizmo::on_plane(proj, pos, base, n).filter(|p| p.is_finite())
}

/// The dialog taken out of `app` for a change (a drag's move, a click, a turn), and put back.
fn with_dialog(app: &mut SolveApp, f: impl FnOnce(&SolveApp, &mut Dialog)) {
    if let Some(mut d) = app.dialog.take() {
        f(app, &mut d);
        app.dialog = Some(d);
    }
}

/// The marker's arms for a place's freedom, as unit screen directions: the four cardinals, the
/// two ways along a line as the view shows it, or none. A line seen nearly end-on (shorter on
/// screen than [`LINE_MIN_PX`] per millimetre, or the face itself seen nearly edge-on, where
/// [`on_plane`] refuses a pick) gets no arms and no press area: a drag there would move the
/// feature by centimetres per pixel, so the marker is the dot alone until the view turns.
fn arms(proj: &Proj, base: Vec3, n: Vec3, bs: Pos2, freedom: Freedom) -> Vec<egui::Vec2> {
    match freedom {
        Freedom::Plane => CARDINALS.to_vec(),
        Freedom::Line(dir) => {
            if proj.ray(bs).1.dot(n).abs() < 0.05 {
                return Vec::new();
            }
            match proj.to_screen(base + dir) {
                Some(p) if (p - bs).length() >= LINE_MIN_PX => {
                    let d = (p - bs).normalized();
                    vec![d, -d]
                }
                _ => Vec::new(),
            }
        }
    }
}

/// How long a millimetre along a line of motion must be on screen for the line to be dragged.
const LINE_MIN_PX: f32 = 0.5;

/// An id for each of the tool's `n` places by their order, the same from frame to frame: a place
/// keeps its id while it moves (its key, an index, can change, as a round face's two halves come
/// and go), a new place gets a new one. A marker's press, hover and look go by it. With a place
/// gone, every place gets a new id, so the one after it does not take over the press, hover or
/// look of the one taken out.
fn marker_ids(ctx: &egui::Context, store: egui::Id, n: usize) -> Vec<u64> {
    let (mut ids, mut next) = ctx.data(|d| d.get_temp::<(Vec<u64>, u64)>(store)).unwrap_or_default();
    if ids.len() > n {
        ids.clear();
    }
    while ids.len() < n {
        ids.push(next);
        next += 1;
    }
    ctx.data_mut(|d| d.insert_temp(store, (ids.clone(), next)));
    ids
}

/// Draw the handles of the open dialog and take their drags (after the dialog's own arrow, so the
/// marker wins the pointer near the base). The dialog stays in `app` while the pointer is picked
/// under, so a drag snaps as a click does (its input's points, the hole's face and rim snaps);
/// the pick and the move go by the place input (the first), whichever input is active, so a
/// reference edge input waiting for its edge does not stop the marker.
pub(crate) fn show(app: &mut SolveApp, ui: &mut egui::Ui, painter: &egui::Painter, proj: &Proj) {
    DRAWN.with(|h| h.borrow_mut().clear());
    RING_ABOUT.with(|c| c.set(None));
    let Some(sp) = app.dialog.as_ref().and_then(|d| spec(app, d)) else { return };
    let places = |app: &SolveApp| app.dialog.as_ref().map(|d| sp.places(app, d)).unwrap_or_default();
    let t = Tokens::get();
    let area = |c: Pos2| egui::Rect::from_center_size(c, vec2(2.0 * REACH, 2.0 * REACH));
    let id = |what: &str| egui::Id::new(("sc_place", sp.name, what));
    let at = |marker: u64, what: &str| egui::Id::new(("sc_place", sp.name, marker, what));
    let store = id("markers");
    let sense = if sp.click.is_some() { egui::Sense::click_and_drag() } else { egui::Sense::drag() };
    let freedom_of = |app: &SolveApp, key: usize| app.dialog.as_ref().map_or(Freedom::Plane, |d| (sp.freedom)(app, d, key));

    // The crosshairs that move the feature, one per place. The press lands where the finger is,
    // not always the centre: the drag keeps that gap. A place held still has no press area.
    let mut markers: Vec<(u64, egui::Response)> = Vec::new();
    let before = places(app);
    let ids = marker_ids(ui.ctx(), store, before.len());
    for (pl, marker) in before.into_iter().zip(ids) {
        let Some(bs) = proj.to_screen(pl.base) else { continue };
        if arms(proj, pl.base, pl.n, bs, freedom_of(app, pl.key)).is_empty() {
            continue;
        }
        let r = ui.interact(area(bs), at(marker, "position"), sense);
        let off = motion::grab(ui, at(marker, "grab"), &r, |p| Some(p - bs));
        if r.dragged() {
            crate::tool_modifiers::set_manipulator_dragging(true);
            if let Some(p) = r.interact_pointer_pos() {
                // The pick follows the place input's rules, whichever input is active.
                let was = app.dialog.as_mut().map(|d| std::mem::replace(&mut d.active, sp.place_input()));
                let hits = crate::viewport::pick(app, proj, p - off.unwrap_or_default());
                with_dialog(app, |app, d| {
                    sp.move_to(app, d, &hits, pl.key);
                });
                if let (Some(d), Some(was)) = (app.dialog.as_mut(), was) {
                    d.active = was;
                }
            }
        } else if r.clicked()
            && let Some(click) = sp.click
        {
            with_dialog(app, |app, d| click(app, d, pl.key));
        }
        markers.push((marker, r));
    }
    // Read the places again: a drag moved one this frame, a click may have taken one out (and
    // the markers' ids with it, so this frame's press and hover reach none of the others).
    let now = places(app);
    let ids = marker_ids(ui.ctx(), store, now.len());
    let ctx = ui.ctx().clone();
    // The marker steps back while the ring works (the ring's drag is last frame's).
    let turning = ui.data(|m| m.get_temp::<bool>(id("turning"))).unwrap_or(false);
    let mut first: Option<(Place, Pos2, bool)> = None;
    for (n, (pl, marker)) in now.into_iter().zip(ids).enumerate() {
        let Some(bs) = proj.to_screen(pl.base) else { continue };
        let name = if sp.many { format!("{}:{n}", sp.name) } else { format!("{}:position", sp.name) };
        crate::scenario::publish_handle(&name, bs);
        let dirs = arms(proj, pl.base, pl.n, bs, freedom_of(app, pl.key));
        let held = dirs.is_empty();
        DRAWN.with(|h| h.borrow_mut().push((bs, !held)));
        let r = markers.iter().filter(|_| !held).find(|(m, _)| *m == marker).map(|(_, r)| r);
        let (hovered, dragging) = r.map_or((false, false), |r| (r.hovered(), r.dragged()));
        // The tip while pointing, not while dragging: the hand is busy and the footprint says it all.
        if hovered
            && !dragging
            && let Some(r) = r
        {
            r.clone().on_hover_text(crate::i18n::tr_label(sp.move_tip));
        }
        // A held place steps back as a handle not in use does: there, but offering no move.
        let style = CrossStyle {
            hot: MARKER_HOT.run(&ctx, at(marker, "hot"), hovered || dragging).t,
            active: MARKER_ACTIVE.run(&ctx, at(marker, "active"), dragging).t,
            alpha: motion::step_back(&ctx, at(marker, "marker_dim"), turning || held, DIM, DIM_S),
        };
        crate::scenario::publish_count(&format!("{name}:hot"), f64::from(style.hot));
        if dragging && let Some(d) = app.dialog.as_ref() {
            let pts: Option<Vec<Pos2>> = (sp.footprint)(app, d, pl.base, pl.n).into_iter().map(|p| proj.to_screen(p)).collect();
            if let Some(pts) = pts.filter(|p| p.len() >= 3) {
                footprint(painter, &pts, &t);
            }
        }
        crosshair(painter, bs, &style, &dirs, &t);
        if hovered || dragging {
            ui.ctx().set_cursor_icon(if dragging { egui::CursorIcon::Grabbing } else { egui::CursorIcon::Move });
        }
        if n == 0 {
            first = Some((pl, bs, dragging));
        }
    }

    // The ring that turns a one-place feature: on the face about the same centre, a fixed size
    // on screen.
    let Some(turn) = &sp.turn else { return };
    let Some((Place { base, n, .. }, bs, moving)) = first.filter(|_| !sp.many) else { return };
    if app.dialog.as_ref().and_then(|d| (turn.reach)(app, d)).is_none() {
        return;
    }
    let Some(mut angle) = app.dialog.as_ref().map(|d| (turn.angle)(app, d, n)) else { return };
    let (x, y) = axes(n);
    // The world radius that makes the ring RING_PX across its least foreshortened face direction
    // (the larger singular value of the projected x, y pair), so it keeps its size as the view
    // turns and does not change while the feature turns.
    let Some(radius) = ring_radius(proj, base, x, y) else { return };
    // A point of the ring's plane by its angle and its offsets from the circle in screen points
    // (outwards along the radius, along the tangent), so the grip and the compass lie on the
    // face like the ring and foreshorten with it.
    let per_px = radius / f64::from(RING_PX);
    let on = |a: f64, r_px: f32, t_px: f32| {
        let (rd, td) = (x * a.cos() + y * a.sin(), y * a.cos() - x * a.sin());
        proj.to_screen(base + rd * (radius + f64::from(r_px) * per_px) + td * (f64::from(t_px) * per_px))
    };
    let at = |a: f64| on(a, 0.0, 0.0);
    let Some(mut gs) = at(angle) else { return };
    let r = ui.interact(area(gs), id("rotate"), egui::Sense::drag());
    let pointer_angle = |p: Pos2| match on_plane(proj, p, base, n) {
        Some(q) => Some((q - base).dot(y).atan2((q - base).dot(x))),
        None => crate::canvas::projected_rotation_angle(proj, p, base, x, y, radius, angle, (-std::f64::consts::PI, std::f64::consts::PI)),
    };
    // As for the marker, the press lands somewhere on the grip, not on its exact angle: the drag
    // keeps that angular gap.
    let grab = motion::grab(ui, id("rotate_grab"), &r, |p| pointer_angle(p).filter(|a| a.is_finite()).map(|a| a - angle));
    if r.dragged()
        && let Some(p) = r.interact_pointer_pos()
    {
        crate::tool_modifiers::set_manipulator_dragging(true);
        if let Some(raw) = pointer_angle(p).filter(|a| a.is_finite()).map(|a| a - grab.unwrap_or(0.0)) {
            let free = crate::tool_modifiers::manipulator_free();
            with_dialog(app, |app, d| {
                angle = (turn.set)(app, d, n, raw, free);
                d.error = None;
            });
            if let Some(g) = at(angle) {
                gs = g;
            }
        }
    }
    crate::scenario::publish_handle(&format!("{}:rotate", sp.name), gs);
    DRAWN.with(|h| h.borrow_mut().push((gs, true)));
    let (hovered, dragging) = (r.hovered(), r.dragged());
    ui.data_mut(|m| m.insert_temp(id("turning"), dragging));
    RING_ABOUT.with(|c| c.set(Some(bs)));
    let open = RING_OPEN.run(&ctx, id("ring_hot"), hovered || dragging);
    let on_point = at_cardinal(n, angle);
    // Aligned with a world axis, the grip takes that axis's colour.
    let axis = on_point.as_deref().map(|name| axis_colour(name, &t));
    let style = RingStyle {
        hot: open.t,
        active: RING_ACTIVE.run(&ctx, id("ring_active"), dragging).t,
        fading: open.fading(),
        alpha: motion::step_back(&ctx, id("ring_dim"), moving, DIM, DIM_S),
        tint: motion::tint(&ctx, id("ring_tint"), axis, look::core(true), DIM_S),
    };
    ring(painter, &on, angle, &style);
    // The compass stays put while the feature turns, so you can see how far you have gone.
    if style.active > 0.01 {
        // The tick of the point the grip sits on is left out: the grip, in that axis's colour, is
        // the tick then, and a tick drawn under it would show its casing through.
        let near = |a: f64| ((a - angle + std::f64::consts::PI).rem_euclid(std::f64::consts::TAU) - std::f64::consts::PI).abs() < 1e-6;
        let dirs: Vec<(f64, egui::Color32)> =
            cardinals(n).into_iter().filter(|(_, a)| !near(*a)).map(|(name, a)| (a, axis_colour(&name, &t))).collect();
        compass(painter, &on, &dirs, style.active);
    }
    if hovered && !dragging {
        r.clone().on_hover_text(crate::i18n::tr_label(turn.tip));
    }
    if hovered || dragging {
        ui.ctx().set_cursor_icon(if dragging { egui::CursorIcon::Grabbing } else { egui::CursorIcon::Grab });
    }
    if dragging {
        // The readout wraps to a half turn either way; the grab offset can carry the raw angle past it.
        let mut shown = (angle + std::f64::consts::PI).rem_euclid(std::f64::consts::TAU) - std::f64::consts::PI;
        if shown <= -std::f64::consts::PI + 1e-9 {
            shown = std::f64::consts::PI;
        }
        angle_label(painter, gs, bs, shown, on_point.as_deref().map(|name| (name, axis_colour(name, &t))), &t);
    }
}

/// The world radius of a circle about `base` across the face spanned by `x` and `y` that is
/// [`RING_PX`] on screen along the face direction the view foreshortens least.
fn ring_radius(proj: &Proj, base: Vec3, x: Vec3, y: Vec3) -> Option<f64> {
    let o = proj.to_screen(base)?;
    let (a, b) = (proj.to_screen(base + x)? - o, proj.to_screen(base + y)? - o);
    let (p, q, r) = (f64::from(a.length_sq()), f64::from(b.length_sq()), f64::from(a.dot(b)));
    let px_per_mm = ((p + q + ((p - q).powi(2) + 4.0 * r * r).sqrt()) / 2.0).sqrt();
    (px_per_mm > 1e-6).then(|| f64::from(RING_PX) / px_per_mm)
}

/// Where the floating value box goes so it does not cover a placement handle: `at` as the caller
/// wants it, or, with a handle's centre drawn under it last frame, the same box to the left of
/// `anchor` (the arrow's tip) or above it, whichever is clear first. Seen straight down the arrow
/// collapses onto the place and the box sits just right of it, where the ring's grip often is.
pub(crate) fn yield_box(ui: &egui::Ui, anchor: Pos2, at: Pos2) -> Pos2 {
    let Some(size) = ui.ctx().memory(|m| m.area_rect(egui::Id::new("sc_canvas_area"))).map(|r| r.size()) else { return at };
    let target = match RING_ABOUT.with(|c| c.get()) {
        // With a ring, the box keeps out of the whole circle the ring, its grip, the compass and
        // the readout sweep, whatever the turn: one place per view, nothing moves when a turn
        // starts, and the grip can never come under the box.
        Some(centre) => clear_of_circle(at, size, centre, RING_PX + RING_ROOM),
        None => {
            let covers = |p: Pos2| DRAWN.with(|h| h.borrow().iter().any(|(c, _)| egui::Rect::from_min_size(p, size).expand(4.0).contains(*c)));
            if covers(at) {
                let left = Pos2::new(anchor.x - (at.x - anchor.x) - size.x, at.y);
                let above = Pos2::new(at.x, anchor.y - 14.0 - size.y);
                [left, above].into_iter().find(|p| !covers(*p)).unwrap_or(at)
            } else {
                at
            }
        }
    };
    // The step aside slides, as an offset from where the box would be, so the box still follows
    // the arrow's tip at once when the view moves.
    at + motion::slide(ui.ctx(), egui::Id::new("sc_place_box"), target - at, DIM_S)
}

/// What the ring needs around it beyond its radius (pixels): the grip, the compass and the
/// readout beside the grip.
const RING_ROOM: f32 = 44.0;

/// `at`, or the nearest place above, below, left or right of the circle where a box of `size`
/// no longer meets it.
fn clear_of_circle(at: Pos2, size: egui::Vec2, centre: Pos2, r: f32) -> Pos2 {
    let rect = egui::Rect::from_min_size(at, size);
    if rect.clamp(centre).distance(centre) >= r {
        return at;
    }
    let candidates = [
        Pos2::new(at.x, centre.y - r - size.y),
        Pos2::new(at.x, centre.y + r),
        Pos2::new(centre.x + r, at.y),
        Pos2::new(centre.x - r - size.x, at.y),
    ];
    candidates.into_iter().min_by(|a, b| a.distance(at).total_cmp(&b.distance(at))).unwrap_or(at)
}
