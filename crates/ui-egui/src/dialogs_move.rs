//! Move/Copy: the move types (Free Move, Translate, Rotate, Point to Point, Point to Position),
//! their rows and inputs, the triad on the canvas and the solid.move they make. Nothing
//! moves until a value is typed or the triad is dragged. Switching the type keeps the move made
//! so far, and the next type moves the bodies on from where it left them: one solid.move holds
//! the whole chain.
//!
//! A Free Move's values are measured along its triad. Local, the triad starts as the bodies
//! stand and turns with them; Fixed, it lies as the picked target does, or as the world, and
//! stays. A placed pivot gives it a point, and each of the two its own axes to go on from. The
//! distances lie along the triad's axes as they are now (`along`)
//! and the angles apply in the order they were dragged (`order`), so a drag on a handle changes
//! its own value alone and the value reads what was dragged. Where no sum of angles can make a
//! turn, the turns so far go into the move made so far and the angle starts over (`bake`).
//!
//! Placing a pivot is a Free Move of the triad alone: the same values and handles measure the
//! pivot from where it started, and move no body, until the tick accepts it.

use crate::selection::SelectionExt as _;
use crate::ui_kit::{choice_row, expr_row};
use serde_json::{Value, json};
use solidcraft_engine::Sel;
use solidcraft_engine::doc::expr::Kind as ValueKind;
use solidcraft_engine::doc::{FeatureKind, Mat, apply_point, apply_vector, is_identity, mat_inverse, mat_mul, rigid};
use solidcraft_engine::geom::{Aabb3, Vec3};

use crate::SolveApp;
use crate::dialogs::{Dialog, Kind};
use crate::gizmo::{self, Axes, Change, Drag, WORLD};
use crate::selection::{AXIS_PICKS, BODIES, EDGES, FACES, ORIGIN, SelInput, VERTICES};

pub const TYPES: [&str; 5] = ["Free Move", "Translate", "Rotate", "Point to Point", "Point to Position"];
pub const FREE: usize = 0;
pub const TRANSLATE: usize = 1;
pub const ROTATE: usize = 2;
pub const POINT_TO_POINT: usize = 3;
pub const POINT_TO_POSITION: usize = 4;
const POINTS: u16 = VERTICES | FACES | EDGES | ORIGIN;

/// A fresh Move dialog of a type (all values zero).
pub fn kind(mode: usize, doc: &solidcraft_engine::doc::Document) -> Kind {
    let z = || crate::length_input::default(doc, "0");
    let a = || "0 deg".to_string();
    Kind::Move { mode, x: z(), y: z(), z: z(), rx: a(), ry: a(), rz: a(), angle: a() }
}

/// The selection inputs of a move type (the bodies first).
pub fn inputs(mode: usize) -> Vec<SelInput> {
    let mut v = vec![SelInput::new("Bodies", BODIES, true)];
    match mode {
        FREE => v.push(SelInput::new("Pivot", POINTS | AXIS_PICKS, false)),
        ROTATE => v.push(SelInput::new("Axis", AXIS_PICKS, false)),
        POINT_TO_POINT => {
            v.push(SelInput::new("From", POINTS, false));
            v.push(SelInput::new("To", POINTS, false));
        }
        POINT_TO_POSITION => v.push(SelInput::new("Point", POINTS, false)),
        _ => {}
    }
    v
}

/// The Move Type row (above the inputs); switching keeps the bodies and the move made so far
/// (`chain`), and starts the new type's values at zero.
pub fn type_row(app: &SolveApp, d: &mut Dialog, ui: &mut egui::Ui) {
    let Kind::Move { mode, .. } = d.kind else { return };
    let mut m = mode;
    choice_row(ui, "Move Type", "move_type", &mut m, &TYPES);
    if m != mode {
        switch_type(app, d, m);
    }
}

/// Change the Move Type to `m`, keeping the move made so far.
fn switch_type(app: &SolveApp, d: &mut Dialog, m: usize) {
    // A type whose picks are missing or whose values don't evaluate adds nothing.
    if let Ok(Step::Made(p)) = step(app, d)
        && let Ok(made) = transform(app, d, &p)
    {
        let total = chain(d).map_or(made, |c| mat_mul(&made, &c));
        d.extra.insert("chain".into(), json!(total));
    }
    let bodies = d.inputs.first().map(|i| i.items.clone()).unwrap_or_default();
    d.kind = kind(m, &app.session.doc);
    d.inputs = inputs(m);
    if let Some(i) = d.inputs.first_mut() {
        i.items = bodies;
    }
    for key in ["pivot", "pivot_axes", "pivot_own", "along", "order"] {
        d.extra.remove(key);
    }
    forget_placing(d);
    d.active = if bodies_empty(d) { 0 } else { 1.min(d.inputs.len() - 1) };
}

fn bodies_empty(d: &Dialog) -> bool {
    d.inputs.first().is_none_or(|i| i.items.is_empty())
}

/// The move made under earlier types, which the current type moves the bodies on from.
fn chain(d: &Dialog) -> Option<Mat> {
    let m: Mat = serde_json::from_value(d.extra.get("chain")?.clone()).ok()?;
    (!is_identity(&m)).then_some(m)
}

/// The moved bodies and where the move so far puts them (the earlier types' move, then the
/// current type's as far as it is made), while a Move dialog moves any: the picker finds them,
/// and their highlights and centres show, where the preview draws them, also mid-drag.
pub(crate) fn chained(app: &SolveApp) -> Option<(Vec<String>, Mat)> {
    let d = app.dialog.as_ref().filter(|d| matches!(d.kind, Kind::Move { .. }))?;
    let m = match (made(app, d), chain(d)) {
        (Some(s), Some(c)) => mat_mul(&s, &c),
        (Some(s), None) => s,
        (None, c) => c?,
    };
    (!is_identity(&m)).then(|| (items(d, 0).body_names(), m))
}

/// Whether the triad is being dragged in a Move dialog (`drag` keeps the values it began from).
pub(crate) fn dragging(app: &SolveApp) -> bool {
    app.dialog.as_ref().is_some_and(|d| matches!(d.kind, Kind::Move { .. }) && d.extra.contains_key("drag0") && !placing_pivot(d))
}

/// The current type's own move, when its picks and values make one.
fn made(app: &SolveApp, d: &Dialog) -> Option<Mat> {
    match step(app, d) {
        Ok(Step::Made(p)) => transform(app, d, &p).ok(),
        _ => None,
    }
}

/// Where a point a pick keeps on moved body `body` (see [`from_shown`]) shows: moved on by the
/// current type's own move.
pub(crate) fn shown_point(app: &SolveApp, body: &str, p: Vec3) -> Vec3 {
    let Some(d) = app.dialog.as_ref().filter(|d| matches!(d.kind, Kind::Move { .. }) && items(d, 0).body_names().iter().any(|n| n == body)) else {
        return p;
    };
    made(app, d).map_or(p, |m| apply_point(&m, p))
}

/// A pick on a moved body is made where the move so far shows it; the current type measures
/// from where the earlier types leave the body, so the pick is taken back by the current type's
/// own move. Kept there, a pivot or From point picked mid-move doesn't move with the move it
/// sets.
pub(crate) fn from_shown(app: &SolveApp, sel: Sel) -> Sel {
    let Some(d) = app.dialog.as_ref().filter(|d| matches!(d.kind, Kind::Move { .. }) && on_moved(d, &sel)) else { return sel };
    let Some(back) = made(app, d).and_then(|m| mat_inverse(&m)) else { return sel };
    match sel {
        Sel::Edge { body, index, point } => Sel::Edge { body, index, point: apply_point(&back, point) },
        Sel::Face { body, index, point } => Sel::Face { body, index, point: apply_point(&back, point) },
        Sel::Vertex { body, point } => Sel::Vertex { body, point: apply_point(&back, point) },
        other => other,
    }
}

/// Whether a pick is on one of the bodies being moved.
fn on_moved(d: &Dialog, s: &Sel) -> bool {
    match s {
        Sel::Edge { body, .. } | Sel::Face { body, .. } | Sel::Vertex { body, .. } => items(d, 0).body_names().contains(body),
        _ => false,
    }
}

/// A point of the model before this move where the earlier types' move puts it: on a moved body
/// (`moved`) or, for the bodies' centre, always.
fn carried(d: &Dialog, moved: bool, p: Vec3) -> Vec3 {
    match chain(d) {
        Some(c) if moved => apply_point(&c, p),
        _ => p,
    }
}

/// The value rows after the inputs; true when Enter was pressed in one.
pub fn rows(ui: &mut egui::Ui, k: &mut Kind) -> bool {
    let Kind::Move { mode, x, y, z, rx, ry, rz, angle } = k else { return false };
    let mut enter = false;
    match *mode {
        FREE | TRANSLATE => {
            enter |= expr_row(ui, "X Distance", x);
            enter |= expr_row(ui, "Y Distance", y);
            enter |= expr_row(ui, "Z Distance", z);
            if *mode == FREE {
                enter |= expr_row(ui, "X Angle", rx);
                enter |= expr_row(ui, "Y Angle", ry);
                enter |= expr_row(ui, "Z Angle", rz);
            }
        }
        ROTATE => enter |= expr_row(ui, "Angle", angle),
        POINT_TO_POSITION => {
            enter |= expr_row(ui, "X", x);
            enter |= expr_row(ui, "Y", y);
            enter |= expr_row(ui, "Z", z);
        }
        _ => {}
    }
    enter
}

/// [`rows`] while they place the pivot: framed together with the Pivot row they belong to.
pub(crate) fn pivot_rows(ui: &mut egui::Ui, k: &mut Kind) -> bool {
    let t = crate::theme::Tokens::get();
    let inner = ui.vertical(|ui| rows(ui, k));
    let band: Option<(egui::layers::ShapeIdx, egui::Pos2)> = ui.ctx().data(|data| data.get_temp(egui::Id::new("sc_pivot_band")));
    if let Some((slot, top)) = band {
        let rect = egui::Rect::from_min_max(top, inner.response.rect.max).expand(t.space_4);
        let frame = egui::epaint::RectShape::new(
            rect,
            t.control_radius,
            t.warning.gamma_multiply(0.12),
            egui::Stroke::new(1.0, t.warning),
            egui::StrokeKind::Inside,
        );
        ui.painter().set(slot, frame);
    }
    inner.inner
}

/// Where a point pick is: a round edge stands for its centre, where the model is before this
/// move, in the world (a body of a placed component where its occurrence puts it, as it was
/// picked); any other pick is where it was picked.
fn sel_point(app: &SolveApp, d: &Dialog, s: Option<&Sel>) -> Option<Vec3> {
    let s = s?;
    match s {
        Sel::Edge { body, index, point } => {
            let st = match d.editing {
                Some((id, _)) => std::sync::Arc::new(solidcraft_engine::doc::world_state(&app.session.doc, &app.session.model.state_before(id))),
                None => app.session.world_state(),
            };
            // The centre is found where the body was modelled; the picked point is where it stands.
            Some(crate::dialogs::edge_circle_in(&st, body, *index).map_or(*point, |(c, ..)| carried(d, on_moved(d, s), c)))
        }
        Sel::Vertex { point, .. } | Sel::Face { point, .. } => Some(*point),
        _ => None,
    }
}

fn items(d: &Dialog, i: usize) -> &[Sel] {
    crate::selection::items(&d.inputs, i)
}

/// The box around the picked bodies in the world, where they are before this move.
fn bodies_bounds(app: &SolveApp, d: &Dialog) -> Aabb3 {
    let names = items(d, 0).body_names();
    let mut bb = Aabb3::EMPTY;
    let Some((id, _)) = d.editing else {
        let st = app.session.world_state();
        for b in names.iter().filter_map(|n| st.body(n)) {
            bb = bb.union(&b.mesh().bounds());
        }
        return bb;
    };
    // This runs every frame: place the box rather than every body of the earlier model.
    let st = app.session.model.state_before(id);
    for (name, source, _, _, m) in app.session.doc.body_frames_with_source(&st) {
        if !names.contains(&name) {
            continue;
        }
        let Some(x) = st.body(&source).map(|b| b.mesh().bounds()) else { continue };
        for corner in 0..8 {
            let pick = |bit: u8, lo: f64, hi: f64| if corner >> bit & 1 == 0 { lo } else { hi };
            bb.add(apply_point(&m, Vec3::new(pick(0, x.min.x, x.max.x), pick(1, x.min.y, x.max.y), pick(2, x.min.z, x.max.z))));
        }
    }
    bb
}

/// The centre of the picked bodies where they are before this move (after the earlier types').
fn bodies_center(app: &SolveApp, d: &Dialog) -> Option<Vec3> {
    let bb = bodies_bounds(app, d);
    (!bb.is_empty()).then(|| carried(d, true, bb.center()))
}

/// A body as the model names it, and how its component places it in the world.
fn frame_of(app: &SolveApp, d: &Dialog, name: String) -> Option<(String, Mat)> {
    if app.session.doc.occurrences.is_empty() {
        return Some((name, solidcraft_engine::doc::IDENTITY));
    }
    let st = match d.editing {
        Some((id, _)) => app.session.model.state_before(id),
        None => app.session.model.state(),
    };
    let frames = app.session.placed_document().body_frames_with_source(&st);
    frames.into_iter().find(|f| f.0 == name).map(|f| (f.1, f.4))
}

/// The first picked body's [`frame_of`].
fn body_frame(app: &SolveApp, d: &Dialog) -> Option<(String, Mat)> {
    frame_of(app, d, items(d, 0).body_names().into_iter().next()?)
}

/// The world point a solid.move turns the picked bodies about. The feature turns a body where it
/// was modelled, so that is the origin of the body's frame: where its component is placed, the
/// world origin outside one. Its one translation can only answer for the first body's frame.
fn turn_origin(app: &SolveApp, d: &Dialog) -> Vec3 {
    body_frame(app, d).map_or(Vec3::ZERO, |(_, m)| apply_point(&m, Vec3::ZERO))
}

/// What a Free Move turns about: the placed pivot or the kept one (editing), both kept from the
/// origin of the bodies' frame, else the bodies' centre. A pivot being placed measures from
/// where it started.
fn pivot(app: &SolveApp, d: &Dialog) -> Option<Vec3> {
    if let Some((at, _)) = placing(d) {
        return Some(at);
    }
    if let Some(a) = d.extra.get("pivot").and_then(|v| serde_json::from_value::<[f64; 3]>(v.clone()).ok()) {
        return Some(turn_origin(app, d) + Vec3::new(a[0], a[1], a[2]));
    }
    bodies_center(app, d)
}

/// Whether the move turns about the point an edited move keeps, the origin of the bodies' frame:
/// its stored values, expressions included, mean that. The triad still sits on the bodies.
fn kept_turn(d: &Dialog) -> bool {
    d.extra.contains_key("pivot") && !d.extra.contains_key("pivot_axes") && !placing_pivot(d)
}

/// Where the bodies' centre is after the move as it stands.
fn moved_centre(app: &SolveApp, d: &Dialog) -> Option<Vec3> {
    Some(apply_point(&made(app, d)?, bodies_center(app, d)?))
}

/// Turn the same move about the bodies' centre instead of the kept point: the angles stay, the
/// distances become how far that centre goes. A ring needs this, as it turns about the triad.
fn turn_on_centre(app: &SolveApp, d: &mut Dialog) {
    // A kept turn that does not evaluate lives in `axis` and `angle`, about the kept point.
    if !kept_turn(d) || d.extra.contains_key("angle") {
        return;
    }
    let (Some(at), Some(centre)) = (moved_centre(app, d), bodies_center(app, d)) else { return };
    let t = at - centre;
    d.extra.remove("pivot");
    if let Kind::Move { x, y, z, .. } = &mut d.kind {
        (*x, *y, *z) = (length(app, t.x), length(app, t.y), length(app, t.z));
    }
}

pub(crate) fn rows_of(m: &Mat) -> Axes {
    [0, 1, 2].map(|r| [m[0][r], m[1][r], m[2][r]])
}

fn same_axes(a: &Axes, b: &Axes) -> bool {
    a.iter().flatten().zip(b.iter().flatten()).all(|(x, y)| (x - y).abs() < 1e-9)
}

fn axes_in(d: &Dialog, key: &str) -> Option<Axes> {
    serde_json::from_value(d.extra.get(key)?.clone()).ok()
}

/// Whether the triad's axes stay as they are, rather than turn with the bodies.
fn fixed(d: &Dialog) -> bool {
    d.extra.get("fixed").and_then(Value::as_bool).unwrap_or(false)
}

/// Whether the triad turns as its rings are dragged: always, for a pivot being placed.
fn turns(d: &Dialog) -> bool {
    !fixed(d) || placing_pivot(d)
}

/// The order the angles apply in (see [`gizmo::turn_of`]).
fn order(d: &Dialog) -> [usize; 3] {
    d.extra.get("order").and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or(gizmo::ANGLE_ORDER)
}

/// How a body stands: as its component places it, turned by the moves made of it so far.
fn axes_of(app: &SolveApp, d: &Dialog, frame: Option<(String, Mat)>) -> Axes {
    let Some((name, frame)) = frame else { return WORLD };
    let doc = &app.session.doc;
    let mut turned = WORLD;
    for f in doc.features.iter().take(doc.marker.unwrap_or(doc.features.len())) {
        if d.editing.is_some_and(|(id, _)| id == f.id) {
            break;
        }
        if let (false, FeatureKind::Move { bodies, rotate_axis: Some(axis), angle: Some(angle), copy: false, .. }) = (f.suppressed, &f.kind)
            && bodies.iter().any(|b| **b == *name)
            && let Ok(th) = doc.eval(angle, ValueKind::Angle)
        {
            turned = gizmo::mul(&gizmo::rotation(Vec3::from(*axis), th), &turned);
        }
    }
    gizmo::mul(&rows_of(&frame), &turned)
}

/// The first picked body's axes where the move so far has turned it.
fn own_axes(app: &SolveApp, d: &Dialog) -> Axes {
    let own = axes_of(app, d, body_frame(app, d));
    chain(d).map_or(own, |c| gizmo::mul(&rows_of(&c), &own))
}

/// The Local reference: the bodies' own axes, as an accepted pivot was turned from them.
fn local_axes(app: &SolveApp, d: &Dialog) -> Axes {
    let own = own_axes(app, d);
    axes_in(d, "pivot_own").map_or(own, |turn| gizmo::mul(&own, &turn))
}

/// The Fixed reference: the accepted pivot's target, else the world.
fn fixed_axes(d: &Dialog) -> Axes {
    axes_in(d, "pivot_axes").unwrap_or(WORLD)
}

/// The axes a Free Move's angles start from: where the pivot being placed started, else the
/// reference of the frame in use.
fn base_axes(app: &SolveApp, d: &Dialog) -> Axes {
    if let Some((_, axes)) = placing(d) {
        return axes;
    }
    if fixed(d) { fixed_axes(d) } else { local_axes(app, d) }
}

/// The three angles of a Free Move.
fn angles(app: &SolveApp, d: &Dialog) -> Result<[f64; 3], String> {
    let Kind::Move { rx, ry, rz, .. } = &d.kind else { return Err("not a move".into()) };
    Ok([ang(app, rx)?, ang(app, ry)?, ang(app, rz)?])
}

/// The triad's axes now: they turn with the bodies unless fixed.
fn triad_axes(app: &SolveApp, d: &Dialog) -> Result<Axes, String> {
    let base = base_axes(app, d);
    Ok(if turns(d) { gizmo::mul(&base, &gizmo::turn_of(angles(app, d)?, order(d))) } else { base })
}

/// The axes the distances lie along.
fn along_axes(app: &SolveApp, d: &Dialog) -> Axes {
    if !matches!(d.kind, Kind::Move { mode: FREE, .. }) {
        return WORLD;
    }
    axes_in(d, "along").unwrap_or_else(|| base_axes(app, d))
}

/// The distances as a move in the world.
fn distance(app: &SolveApp, d: &Dialog) -> Result<Vec3, String> {
    let Kind::Move { x, y, z, .. } = &d.kind else { return Err("not a move".into()) };
    Ok(gizmo::apply(&along_axes(app, d), Vec3::new(len(app, x)?, len(app, y)?, len(app, z)?)))
}

/// Put the current type's turn (and with `whole` its distances too) into the move made so far,
/// so the values start from zero where the bodies stand. Nothing moves.
fn bake(app: &SolveApp, d: &mut Dialog, whole: bool) {
    // A pivot being placed has no move to keep: it starts its angles over where it stands.
    if let (Some((at, _)), Ok(axes)) = (placing(d), triad_axes(app, d)) {
        set_placing(d, at, &axes);
        d.extra.remove("order");
        zero_values(app, d, false);
        return;
    }
    turn_on_centre(app, d);
    let Some(m) = made(app, d) else { return };
    let t = distance(app, d).unwrap_or(Vec3::ZERO);
    let axes = triad_axes(app, d).ok();
    let at = pivot(app, d);
    let keep = if whole { m } else { mat_mul(&rigid(Vec3::ZERO - t, Vec3::ZERO, Vec3::Z, 0.0), &m) };
    let total = chain(d).map_or(keep, |c| mat_mul(&keep, &c));
    d.extra.insert("chain".into(), json!(total));
    if let (Some(at), true, true) = (at, whole, d.extra.contains_key("pivot")) {
        let kept = at + t - turn_origin(app, d);
        d.extra.insert("pivot".into(), json!([kept.x, kept.y, kept.z]));
    }
    if let Some(axes) = axes {
        d.extra.insert("along".into(), json!(axes));
    }
    for key in ["order", "axis", "angle"] {
        d.extra.remove(key);
    }
    zero_values(app, d, whole);
}

/// The angles back at zero, and with `whole` the distances too.
fn zero_values(app: &SolveApp, d: &mut Dialog, whole: bool) {
    let fresh = kind(FREE, &app.session.doc);
    if let (Kind::Move { x, y, z, rx, ry, rz, .. }, Kind::Move { x: x0, y: y0, z: z0, rx: rx0, ry: ry0, rz: rz0, .. }) = (&mut d.kind, fresh) {
        (*rx, *ry, *rz) = (rx0, ry0, rz0);
        if whole {
            (*x, *y, *z) = (x0, y0, z0);
        }
    }
}

/// Keep what the triad and the values mean as they change: a pivot pick places the pivot, and
/// the distances stay along the triad's axes when they turn, the bodies staying where they are.
pub fn sync(app: &SolveApp, d: &mut Dialog) {
    if !matches!(d.kind, Kind::Move { mode: FREE, .. }) {
        return;
    }
    take_pivot_pick(app, d);
    if let Ok(t) = distance(app, d) {
        keep_distance(app, d, t);
    }
}

/// Lay the distances along the triad's axes as they are now, so together they still make `t`.
fn keep_distance(app: &SolveApp, d: &mut Dialog, t: Vec3) {
    let Ok(now) = triad_axes(app, d) else { return };
    if same_axes(&now, &along_axes(app, d)) {
        return;
    }
    d.extra.insert("along".into(), json!(now));
    if t.len() < 1e-9 {
        return;
    }
    let v = gizmo::apply(&gizmo::inverse(&now), t);
    if let Kind::Move { x, y, z, .. } = &mut d.kind {
        *x = length(app, v.x);
        *y = length(app, v.y);
        *z = length(app, v.z);
    }
}

/// Where the pivot being placed started: what its values measure from.
fn placing(d: &Dialog) -> Option<(Vec3, Axes)> {
    let (at, axes): ([f64; 3], Axes) = serde_json::from_value(d.extra.get("placing")?.clone()).ok()?;
    Some((Vec3::new(at[0], at[1], at[2]), axes))
}

fn set_placing(d: &mut Dialog, at: Vec3, axes: &Axes) {
    d.extra.insert("placing".into(), json!(([at.x, at.y, at.z], axes)));
}

/// Whether a pivot is being placed: the triad and the values then move it, not the bodies.
pub(crate) fn placing_pivot(d: &Dialog) -> bool {
    placing(d).is_some()
}

/// How far the pivot being placed has gone: from where it started to where it is.
pub(crate) fn pivot_trail(app: &SolveApp, d: &Dialog) -> Option<(Vec3, Vec3)> {
    Some((placing(d)?.0, triad(app, d)?.center))
}

/// Start placing a pivot from where the triad is, with both references to go to. The move so
/// far is kept and the values go to the pivot, from zero.
pub(crate) fn begin_pivot(app: &SolveApp, d: &mut Dialog) {
    bake(app, d, true);
    let Some(t) = triad(app, d) else { return };
    let (local, fix) = (local_axes(app, d), fixed_axes(d));
    d.extra.insert("placing_local".into(), json!(local));
    d.extra.insert("placing_fixed".into(), json!(fix));
    zero_values(app, d, true);
    d.extra.remove("along");
    d.extra.remove("order");
    set_placing(d, t.center, &t.axes);
    d.active = 1;
}

fn forget_placing(d: &mut Dialog) {
    for key in ["placing", "placing_local", "placing_fixed", "placing_target"] {
        d.extra.remove(key);
    }
}

/// Stop placing the pivot, keeping the one there was: the values go back to the move, the next
/// pick to the bodies.
pub(crate) fn end_pivot(app: &SolveApp, d: &mut Dialog) {
    forget_placing(d);
    d.extra.remove("along");
    d.extra.remove("order");
    if let Some(i) = d.inputs.get_mut(1) {
        i.items.clear();
    }
    zero_values(app, d, true);
    d.active = 0;
}

/// The tick: the triad's place becomes the pivot, and each frame keeps the axes it reached.
pub(crate) fn accept_pivot(app: &SolveApp, d: &mut Dialog) {
    let Some(t) = triad(app, d) else { return };
    let (local, fix) = if fixed(d) { (axes_in(d, "placing_local"), Some(t.axes)) } else { (Some(t.axes), axes_in(d, "placing_fixed")) };
    let kept = t.center - turn_origin(app, d);
    d.extra.insert("pivot".into(), json!([kept.x, kept.y, kept.z]));
    d.extra.insert("pivot_axes".into(), json!(fix.unwrap_or(WORLD)));
    let own = gizmo::inverse(&own_axes(app, d));
    d.extra.insert("pivot_own".into(), json!(gizmo::mul(&own, &local.unwrap_or(t.axes))));
    end_pivot(app, d);
}

/// Back to the bodies' centre and the references without a pivot.
pub(crate) fn clear_pivot(app: &SolveApp, d: &mut Dialog) {
    bake(app, d, true);
    for key in ["pivot", "pivot_axes", "pivot_own", "along"] {
        d.extra.remove(key);
    }
}

/// The distances of the pivot being placed, to reach `at` along `axes`.
fn pivot_distance(app: &SolveApp, d: &mut Dialog, at: Vec3, axes: &Axes) {
    let Some((start, _)) = placing(d) else { return };
    let v = gizmo::apply(&gizmo::inverse(axes), at - start);
    if let Kind::Move { x, y, z, .. } = &mut d.kind {
        (*x, *y, *z) = (length(app, v.x), length(app, v.y), length(app, v.z));
    }
    d.extra.insert("along".into(), json!(axes));
}

/// The pivot being placed goes over to `axes`, at `at`. The values read that from where it
/// started: how far it went, and how far it turned.
fn pivot_reference(app: &SolveApp, d: &mut Dialog, at: Vec3, axes: &Axes) {
    let Some((_, start)) = placing(d) else { return };
    let turned = gizmo::rotation_angles(&gizmo::mul(&gizmo::inverse(&start), axes));
    if let Kind::Move { rx, ry, rz, .. } = &mut d.kind {
        // Finer than a typed move's: the axes come back from these, along a picked edge.
        [*rx, *ry, *rz] = turned.map(|th| degrees(th, 8));
    }
    d.extra.remove("order");
    pivot_distance(app, d, at, axes);
}

/// Local or Fixed. The move so far stays and the values start from zero on the new axes; a pivot
/// being placed goes to that frame's reference, each keeping the turns it was given.
pub(crate) fn set_fixed(app: &SolveApp, d: &mut Dialog, on: bool) {
    if !placing_pivot(d) {
        bake(app, d, true);
        d.extra.remove("along");
        d.extra.insert("fixed".into(), json!(on));
        return;
    }
    let Some(t) = triad(app, d) else { return };
    let (left, next) = if on { ("placing_local", "placing_fixed") } else { ("placing_fixed", "placing_local") };
    d.extra.insert(left.into(), json!(t.axes));
    d.extra.insert("fixed".into(), json!(on));
    let axes = axes_in(d, next).unwrap_or(WORLD);
    pivot_reference(app, d, t.center, &axes);
}

/// `axes` turned the shortest way until Z lies along `dir`.
fn laid_along(axes: &Axes, dir: Vec3) -> Axes {
    let Some(dir) = dir.normalized() else { return *axes };
    let z = gizmo::axis(axes, 2);
    let across = z.cross(dir);
    let turn = match across.normalized().filter(|_| across.len() > 1e-9) {
        Some(a) => gizmo::rotation(a, z.dot(dir).clamp(-1.0, 1.0).acos()),
        None if z.dot(dir) < 0.0 => gizmo::rotation(gizmo::axis(axes, 0), std::f64::consts::PI),
        None => WORLD,
    };
    gizmo::mul(&turn, axes)
}

/// A pick while placing the pivot. A point sets where it is, a line which way its Z lies, so the
/// two can be picked one after the other; a round edge, a planar face or a round face sets both.
/// What gives a direction is the Fixed reference and goes there; a point gives the frame of what
/// it is on, unless a direction has been picked.
fn take_pivot_pick(app: &SolveApp, d: &mut Dialog) {
    let (true, Some(s), Some(t)) = (placing_pivot(d), items(d, 1).first().cloned(), triad(app, d)) else { return };
    if let Some(i) = d.inputs.get_mut(1) {
        i.items.clear();
    }
    let target = axes_in(d, "placing_fixed").unwrap_or(WORLD);
    let from = if fixed(d) { t.axes } else { target };
    let line = axis_line(app, d, &s);
    let plane = match &s {
        Sel::Face { body, index, .. } => crate::dialogs::planar_face(&app.session, body, *index),
        _ => None,
    };
    let mut at = t.center;
    let mut laid = None;
    match (&s, plane, line) {
        (Sel::Face { .. }, Some((c, n)), _) => {
            let moved = on_moved(d, &s);
            at = carried(d, moved, c);
            laid = Some(laid_along(&from, chain(d).filter(|_| moved).map_or(n, |m| apply_vector(&m, n))));
        }
        (Sel::Face { point, .. }, None, Some((o, a))) => {
            at = o + a * (*point - o).dot(a);
            laid = Some(laid_along(&from, a));
        }
        (Sel::Edge { body, index, .. }, _, Some((_, a))) => {
            if crate::dialogs::edge_circle(&app.session, body, *index).is_some()
                && let Some(c) = sel_point(app, d, Some(&s))
            {
                at = c;
            }
            laid = Some(laid_along(&from, a));
        }
        (_, _, Some((_, a))) => laid = Some(laid_along(&from, a)),
        (Sel::Vertex { point, .. } | Sel::Edge { point, .. } | Sel::Face { point, .. }, ..) => at = *point,
        _ => {}
    }
    if let Some(axes) = laid {
        d.extra.insert("placing_fixed".into(), json!(axes));
        d.extra.insert("placing_target".into(), json!(true));
        d.extra.insert("fixed".into(), json!(true));
        pivot_reference(app, d, at, &axes);
        return;
    }
    let body = match &s {
        Sel::Vertex { body, .. } | Sel::Edge { body, .. } | Sel::Face { body, .. } => body.clone(),
        _ => String::new(),
    };
    if !d.extra.contains_key("placing_target") {
        let on = axes_of(app, d, frame_of(app, d, body));
        let frame = chain(d).filter(|_| on_moved(d, &s)).map_or(on, |c| gizmo::mul(&rows_of(&c), &on));
        d.extra.insert("placing_fixed".into(), json!(frame));
        if fixed(d) {
            pivot_reference(app, d, at, &frame);
            return;
        }
    }
    pivot_distance(app, d, at, &t.axes);
}

/// Free Move's Pivot row, in place of a picking chip: the button that places a pivot, the tick
/// that accepts it, and Local or Fixed.
pub(crate) fn pivot_row(app: &SolveApp, d: &mut Dialog, ui: &mut egui::Ui) {
    let place = placing_pivot(d);
    let custom = d.extra.contains_key("pivot_axes");
    let enabled = !bodies_empty(d);
    // The frame round a pivot being placed starts at this row and closes in `pivot_rows`.
    if place {
        let band = (ui.painter().add(egui::Shape::Noop), ui.cursor().min);
        ui.ctx().data_mut(|data| data.insert_temp(egui::Id::new("sc_pivot_band"), band));
    }
    let mut frame = usize::from(fixed(d));
    let (mut press, mut tick, mut reset, mut framed) = (false, false, false, false);
    crate::ui_kit::row(ui, crate::i18n::tr_label("Pivot"), |ui| {
        ui.add_enabled_ui(enabled, |ui| {
            ui.horizontal(|ui| {
                let label = crate::i18n::tr(if place { "dialog.move_pivot_discard" } else { "dialog.move_pivot_set" });
                let mut on = place || custom;
                let button = crate::ui_kit::toggle_icon(ui, "pivot", label, &mut on, "origin");
                // The Pivot input's field, for what reads the form: a button with no text on it.
                button.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, ui.is_enabled(), on, "Pivot"));
                crate::scenario::publish_rect("dialog:field:Pivot", button.rect);
                crate::scenario::publish_handle("dialog:pivot:place", button.rect.center());
                press = button.clicked();
                if place {
                    tick = crate::ui_kit::check_button(ui, "dialog:pivot:accept", crate::i18n::tr("dialog.move_pivot_accept")).clicked();
                } else if custom {
                    let (rect, _) =
                        ui.allocate_exact_size(egui::vec2(ui.spacing().interact_size.y, ui.spacing().interact_size.y), egui::Sense::hover());
                    reset = crate::ui_kit::icon_button_at(ui, rect, "close", crate::i18n::tr("dialog.move_pivot_clear")).clicked();
                }
                let options = [crate::i18n::tr("dialog.move_frame_local"), crate::i18n::tr("dialog.move_frame_fixed")];
                let segments =
                    crate::ui_kit::Segments { compact: true, targets: &["dialog:pivot:local", "dialog:pivot:fixed"], ..Default::default() };
                framed = ui.scope(|ui| crate::ui_kit::segmented(ui, &mut frame, &options, segments)).inner;
            });
        })
        .response
        .on_hover_text(crate::i18n::tr(if fixed(d) { "dialog.move_frame_fixed_tip" } else { "dialog.move_frame_local_tip" }));
    });
    if press && place {
        end_pivot(app, d);
    } else if press {
        begin_pivot(app, d);
    }
    if tick {
        accept_pivot(app, d);
    }
    if reset {
        clear_pivot(app, d);
    }
    if framed {
        set_fixed(app, d, frame == 1);
    }
}

fn len(app: &SolveApp, s: &str) -> Result<f64, String> {
    crate::length_input::eval(&app.session.doc, s, ValueKind::Length)
        .map_err(|e| e.to_string())
        .and_then(|v| if v.is_finite() { Ok(v) } else { Err("not a number".into()) })
}

fn ang(app: &SolveApp, s: &str) -> Result<f64, String> {
    crate::length_input::eval(&app.session.doc, s, ValueKind::Angle)
        .map_err(|e| e.to_string())
        .and_then(|v| if v.is_finite() { Ok(v) } else { Err("not a number".into()) })
}

/// A distance worked out from turned axes, without the rounding noise of the turn.
fn length(app: &SolveApp, v: f64) -> String {
    crate::length_input::model(&app.session.doc, (v * 1e6).round() / 1e6 + 0.0)
}

fn mm(v: f64) -> String {
    let s = format!("{v:.6}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    format!("{} mm", if s == "-0" || s.is_empty() { "0" } else { s })
}

fn deg(v: f64) -> String {
    degrees(v, 4)
}

fn degrees(v: f64, places: usize) -> String {
    let s = format!("{:.*}", places, v.to_degrees());
    let s = s.trim_end_matches('0').trim_end_matches('.');
    format!("{} deg", if s == "-0" || s.is_empty() { "0" } else { s })
}

/// The solid.move parameters (bodies, translate, axis, angle): the current type's move after
/// the earlier types' (`chain`), which alone is made while the current type's picks are missing.
pub fn params(app: &SolveApp, d: &Dialog) -> Result<Value, String> {
    let mut p = match (step(app, d)?, chain(d)) {
        (Step::Made(p), None) => p,
        (Step::Needs(e), None) => return Err(e),
        (Step::Made(p), Some(c)) => {
            let made = transform(app, d, &p)?;
            with_transform(app, d, p, &mat_mul(&made, &c))
        }
        (Step::Needs(_), Some(c)) => with_transform(app, d, json!({ "bodies": items(d, 0).body_names() }), &c),
    };
    if d.extra.get("copy").and_then(Value::as_bool).unwrap_or(false) {
        p["copy"] = json!(true);
    }
    Ok(p)
}

/// The current type's move, or what it still needs picked.
enum Step {
    Made(Value),
    Needs(String),
}

/// The rigid transform of solid.move parameters, in the world: the turn about [`turn_origin`],
/// then the translation.
fn transform(app: &SolveApp, d: &Dialog, p: &Value) -> Result<Mat, String> {
    let value = |v: &Value, f: fn(&SolveApp, &str) -> Result<f64, String>| match v {
        Value::String(e) => f(app, e),
        Value::Number(n) => n.as_f64().ok_or_else(|| "not a number".to_string()),
        _ => Err("not a number".into()),
    };
    let t = match p.get("translate").and_then(Value::as_array) {
        Some(a) if a.len() == 3 => Vec3::new(value(&a[0], len)?, value(&a[1], len)?, value(&a[2], len)?),
        _ => Vec3::ZERO,
    };
    let Some(angle) = p.get("angle") else { return Ok(rigid(t, Vec3::ZERO, Vec3::Z, 0.0)) };
    let axis = match p.get("axis").and_then(Value::as_array) {
        Some(a) if a.len() == 3 => Vec3::new(value(&a[0], len)?, value(&a[1], len)?, value(&a[2], len)?),
        _ => Vec3::Z,
    };
    Ok(rigid(t, turn_origin(app, d), axis, value(angle, ang)?))
}

/// `p` with its translation and turn replaced by `m`'s.
fn with_transform(app: &SolveApp, d: &Dialog, mut p: Value, m: &Mat) -> Value {
    let rows = rows_of(m);
    let (axis, th) = gizmo::axis_angle(&rows);
    // `m` turns about the world origin, solid.move about `turn_origin`.
    let o = if th.abs() > 1e-12 { turn_origin(app, d) } else { Vec3::ZERO };
    p["translate"] = {
        let t = Vec3::new(m[3][0], m[3][1], m[3][2]) + gizmo::apply(&rows, o) - o;
        json!([mm(t.x), mm(t.y), mm(t.z)])
    };
    if let Some(o) = p.as_object_mut() {
        o.remove("axis");
        o.remove("angle");
    }
    if th.abs() > 1e-12 {
        p["axis"] = json!([axis.x, axis.y, axis.z]);
        p["angle"] = json!(deg(th));
    }
    p
}

/// The current type's own move, from where the earlier types leave the bodies.
fn step(app: &SolveApp, d: &Dialog) -> Result<Step, String> {
    let Kind::Move { mode, x, y, z, rx, ry, rz, angle } = &d.kind else { return Err("not a move".into()) };
    let bodies = items(d, 0).body_names();
    if bodies.is_empty() {
        return Err("select bodies first".into());
    }
    let need_point = |i: usize, what: &str| sel_point(app, d, items(d, i).first()).ok_or(format!("select {what} first"));
    let num = |t: Vec3| json!([mm(t.x), mm(t.y), mm(t.z)]);
    let mut p = json!({ "bodies": bodies });
    match *mode {
        TRANSLATE => p["translate"] = json!([x, y, z]),
        // The values place the pivot for now: they move no body.
        FREE if placing_pivot(d) => p["translate"] = num(Vec3::ZERO),
        FREE => {
            let a = [ang(app, rx)?, ang(app, ry)?, ang(app, rz)?];
            let turned: Vec<usize> = (0..3).filter(|i| a[*i].abs() > 1e-12).collect();
            let base = base_axes(app, d);
            // Along the world's axes the typed distances stay as they are.
            let straight = same_axes(&along_axes(app, d), &WORLD);
            if turned.is_empty() {
                p["translate"] = if straight { json!([x, y, z]) } else { num(distance(app, d)?) };
                // A rotation kept from an edited move about another axis.
                for k in ["axis", "angle"] {
                    if let Some(v) = d.extra.get(k) {
                        p[k] = v.clone();
                    }
                }
            } else {
                let arm = pivot(app, d).ok_or("select bodies first")? - turn_origin(app, d);
                let r = gizmo::turn_in(&base, &gizmo::turn_of(a, order(d)));
                let (axis, th) = gizmo::axis_angle(&r);
                match turned.as_slice() {
                    // One turn about the origin: the typed values stay as they are.
                    [i] if arm.len() < 1e-9 && straight && same_axes(&base, &WORLD) => {
                        p["translate"] = json!([x, y, z]);
                        p["axis"] = json!([[1, 0, 0], [0, 1, 0], [0, 0, 1]][*i]);
                        p["angle"] = json!([rx, ry, rz][*i]);
                    }
                    _ => {
                        p["translate"] = num(arm - gizmo::apply(&r, arm) + distance(app, d)?);
                        p["axis"] = json!([axis.x, axis.y, axis.z]);
                        p["angle"] = json!(deg(th));
                    }
                }
            }
        }
        ROTATE => {
            let Some((o, a)) = items(d, 1).first().and_then(|s| axis_line(app, d, s)) else {
                return Ok(Step::Needs("select an axis, a straight or circular edge, or a cylinder, cone or torus face first".into()));
            };
            let th = ang(app, angle)?;
            let r = gizmo::rotation(a, th);
            let arm = o - turn_origin(app, d);
            p["translate"] = num(arm - gizmo::apply(&r, arm));
            p["axis"] = json!([a.x, a.y, a.z]);
            p["angle"] = json!(angle);
        }
        POINT_TO_POINT => {
            let (from, to) = match (need_point(1, "the point to move from"), need_point(2, "the point to move to")) {
                (Ok(from), Ok(to)) => (from, to),
                (Err(e), _) | (_, Err(e)) => return Ok(Step::Needs(e)),
            };
            p["translate"] = num(to - from);
        }
        _ => {
            let from = match need_point(1, "the point to move") {
                Ok(from) => from,
                Err(e) => return Ok(Step::Needs(e)),
            };
            let at = Vec3::new(len(app, x)?, len(app, y)?, len(app, z)?);
            p["translate"] = num(at - from);
        }
    }
    Ok(Step::Made(p))
}

/// An axis line: origin axes and sketch lines, a straight or circular body edge, or a cylinder,
/// cone or torus face (`dialogs::axis_of`), or a straight edge of a body where it was modelled;
/// on a moved body, where the earlier types' move puts it.
fn axis_line(app: &SolveApp, d: &Dialog, s: &Sel) -> Option<(Vec3, Vec3)> {
    let (o, a) = modelled_axis(app, s)?;
    match chain(d) {
        Some(c) if on_moved(d, s) => Some((apply_point(&c, o), apply_vector(&c, a))),
        _ => Some((o, a)),
    }
}

fn modelled_axis(app: &SolveApp, s: &Sel) -> Option<(Vec3, Vec3)> {
    if let Some(a) = crate::dialogs::axis_of(app, s) {
        return Some(a);
    }
    let Sel::Edge { body, index, .. } = s else { return None };
    let st = app.session.model.state();
    let m = st.body(body)?.mesh();
    let e = m.edges.get(*index)?;
    let (a, b) = (*e.first()?, *e.last()?);
    let dir = (b - a).normalized()?;
    // Straight: every point on the chord.
    e.iter().all(|p| (*p - a).cross(dir).len() < 1e-6 * (1.0 + (b - a).len())).then_some((a, dir))
}

/// Where the triad sits and how it lies: the pivot carried by the move so far, on the bodies
/// for an edited move's kept turn (Free Move and Translate only).
pub fn triad(app: &SolveApp, d: &Dialog) -> Option<gizmo::Triad> {
    let Kind::Move { mode, x, y, z, .. } = &d.kind else { return None };
    if !matches!(*mode, FREE | TRANSLATE) || bodies_empty(d) {
        return None;
    }
    let along = along_axes(app, d);
    let t = Vec3::new(len(app, x).unwrap_or(0.0), len(app, y).unwrap_or(0.0), len(app, z).unwrap_or(0.0));
    let axes = if *mode == FREE { triad_axes(app, d).unwrap_or(along) } else { WORLD };
    let on_body = if kept_turn(d) { moved_centre(app, d) } else { None };
    let center = match on_body {
        Some(at) => at,
        None => pivot(app, d)? + gizmo::apply(&along, t),
    };
    Some(gizmo::Triad { center, axes, translate: true, rotate: *mode == FREE })
}

/// Rotate's ring lies on the selected axis, level with the picked bodies' centre.
pub(crate) fn rotation_axis(app: &SolveApp, d: &Dialog) -> Option<(Vec3, Vec3)> {
    if !matches!(d.kind, Kind::Move { mode: ROTATE, .. }) {
        return None;
    }
    let centre = bodies_center(app, d)?;
    let (origin, axis) = axis_line(app, d, items(d, 1).first()?)?;
    let axis = axis.normalized()?;
    Some((origin + axis * (centre - origin).dot(axis), axis))
}

/// Apply a triad drag to the dialog's values (added to the values when the drag began).
/// Which value the canvas box shows and types into, for the types that translate: X, Y or Z
/// (0 to 2), or, in Free Move, the angle about X, Y or Z (3 to 5). Kept in `extra` as
/// `value_field`; X at first.
pub fn value_field(d: &Dialog) -> usize {
    let Kind::Move { mode, .. } = &d.kind else { return 0 };
    let n = fields(*mode);
    d.extra.get("value_field").and_then(Value::as_u64).map_or(0, |f| f as usize).min(n.saturating_sub(1))
}

/// How many values the box can show for a move type (see [`value_field`]).
fn fields(mode: usize) -> usize {
    match mode {
        FREE => 6,
        TRANSLATE | POINT_TO_POSITION => 3,
        _ => 1,
    }
}

/// Show the next (`step` 1) or previous (`step` -1) value in the box, round the type's values.
pub fn step_value_field(d: &mut Dialog, step: i64) {
    let Kind::Move { mode, .. } = &d.kind else { return };
    let n = fields(*mode) as i64;
    let next = (value_field(d) as i64 + step).rem_euclid(n.max(1));
    d.extra.insert("value_field".into(), json!(next));
}

/// Before a ring's first turn: let its angle add, or start it over on the turns so far.
fn settle_ring(app: &SolveApp, d: &mut Dialog, ring: usize) {
    let Ok(a) = angles(app, d) else { return };
    match gizmo::ring_order(a, order(d), ring, turns(d)) {
        Some(o) => {
            d.extra.insert("order".into(), json!(o));
        }
        None => bake(app, d, false),
    }
}

pub fn drag(app: &SolveApp, d: &mut Dialog, g: Drag) {
    // The box shows the value the handle drives: the distance along a dragged arrow, the angle
    // of a dragged ring (#4776). A plane square or the ball moves along two or three axes and
    // leaves it as it is.
    let field = match g.handle {
        crate::gizmo::Handle::Axis(i) => Some(i.min(2)),
        crate::gizmo::Handle::Ring(i) => Some(3 + i.min(2)),
        _ => None,
    };
    if let (Some(f), Kind::Move { mode, .. }) = (field, &d.kind)
        && f < fields(*mode)
    {
        d.extra.insert("value_field".into(), json!(f));
    }
    let snapshot = |app: &SolveApp, d: &mut Dialog| {
        let Kind::Move { x, y, z, rx, ry, rz, .. } = &d.kind else { return };
        let v = [len(app, x), len(app, y), len(app, z), ang(app, rx), ang(app, ry), ang(app, rz)].map(|r| r.unwrap_or(0.0));
        d.extra.insert("drag0".into(), json!(v));
        let t = distance(app, d).unwrap_or(Vec3::ZERO);
        d.extra.insert("drag_t".into(), json!([t.x, t.y, t.z]));
    };
    if g.started || !d.extra.contains_key("drag0") {
        snapshot(app, d);
        d.extra.remove("ring_set");
    }
    // Not on the press: a click on a ring changes nothing.
    if let Change::Rotate { axis, angle } = g.change
        && angle != 0.0
        && !d.extra.contains_key("ring_set")
    {
        turn_on_centre(app, d);
        settle_ring(app, d, axis);
        d.extra.insert("ring_set".into(), json!(true));
        snapshot(app, d);
    }
    let along = along_axes(app, d);
    let v0: [f64; 6] = d.extra.get("drag0").and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_default();
    let Kind::Move { x, y, z, rx, ry, rz, .. } = &mut d.kind else { return };
    match g.change {
        Change::Translate(t) => {
            let t = gizmo::apply(&gizmo::inverse(&along), t);
            *x = length(app, v0[0] + t.x);
            *y = length(app, v0[1] + t.y);
            *z = length(app, v0[2] + t.z);
        }
        Change::Rotate { axis, angle } => {
            let i = axis.min(2);
            if let Some(slot) = [rx, ry, rz].into_iter().nth(i)
                && angle != 0.0
            {
                *slot = deg(gizmo::within_turn(v0[3 + i] + angle));
            }
        }
    }
    // From the distance the drag began with: each frame's rounding would add up.
    let held: Option<[f64; 3]> = d.extra.get("drag_t").and_then(|v| serde_json::from_value(v.clone()).ok());
    match (g.change, held) {
        (Change::Rotate { .. }, Some(t)) => keep_distance(app, d, Vec3::new(t[0], t[1], t[2])),
        _ => sync(app, d),
    }
    if g.done {
        for key in ["drag0", "drag_t", "ring_set"] {
            d.extra.remove(key);
        }
    }
}

/// The Free Move angles (X, Y, Z) that make the same turn as a turn of `angle` about `axis`,
/// in the dialog's order (Z applied last, then Y, then X — the same composition `step` uses),
/// so an edited move shows angles that re-make the turn the feature stored.
pub(crate) fn euler_from_axis_angle(axis: Vec3, angle: f64) -> (f64, f64, f64) {
    let axis = axis.normalized().unwrap_or(Vec3::Z);
    let [rx, ry, rz] = gizmo::rotation_angles(&gizmo::rotation(axis, angle));
    (rx, ry, rz)
}

/// The dialog for editing a move: Free Move with its translation and, about a world axis
/// through the origin, its angle (another axis is shown as the Free Move angles that make
/// the same turn, or kept as it is when the angle does not evaluate to a number). The stored
/// values are in the world, so it opens Fixed.
pub fn for_feature(
    doc: &solidcraft_engine::doc::Document,
    d: &mut Dialog,
    bodies: &[String],
    translate: &[solidcraft_engine::doc::Expr; 3],
    axis: Option<Vec3>,
    angle: Option<&solidcraft_engine::doc::Expr>,
    copy: bool,
) {
    let [x, y, z] = translate.clone().map(String::from);
    let mut k = kind(FREE, doc);
    if let Kind::Move { x: kx, y: ky, z: kz, rx, ry, rz, .. } = &mut k {
        (*kx, *ky, *kz) = (x, y, z);
        if let Some(a) = angle {
            let dir = axis.unwrap_or(Vec3::Z);
            let world = [Vec3::X, Vec3::Y, Vec3::Z].iter().position(|w| dir.cross(*w).len() < 1e-9 && dir.dot(*w) > 0.0);
            match world {
                Some(0) => *rx = a.to_string(),
                Some(1) => *ry = a.to_string(),
                Some(_) => *rz = a.to_string(),
                None => match doc.eval(a, ValueKind::Angle) {
                    Ok(th) if th.is_finite() => {
                        let (ax, ay, az) = euler_from_axis_angle(dir, th);
                        (*rx, *ry, *rz) = (deg(ax), deg(ay), deg(az));
                    }
                    _ => {
                        d.extra.insert("axis".into(), json!([dir.x, dir.y, dir.z]));
                        d.extra.insert("angle".into(), json!(a));
                    }
                },
            }
            // The stored move turns about the origin.
            d.extra.insert("pivot".into(), json!([0.0, 0.0, 0.0]));
        }
    }
    d.kind = k;
    d.extra.insert("fixed".into(), json!(true));
    d.inputs = inputs(FREE);
    if let Some(inp) = d.inputs.first_mut() {
        inp.items = bodies.iter().map(|n| Sel::Body { name: n.clone() }).collect();
    }
    if copy {
        d.extra.insert("copy".into(), json!(true));
    }
}

#[cfg(test)]
#[path = "dialogs_move_tests.rs"]
mod tests;
