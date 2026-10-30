//! Move/Copy: the move types (Free Move, Translate, Rotate, Point to Point, Point to Position),
//! their rows and inputs, the triad on the canvas and the solid.move they make. Nothing
//! moves until a value is typed or the triad is dragged. Switching the type keeps the move made
//! so far, and the next type moves the bodies on from where it left them: one solid.move holds
//! the whole chain.

use crate::selection::SelectionExt as _;
use crate::ui_kit::{choice_row, expr_row};
use serde_json::{Value, json};
use solidcraft_engine::Sel;
use solidcraft_engine::doc::expr::Kind as ValueKind;
use solidcraft_engine::doc::{Mat, apply_point, apply_vector, is_identity, mat_inverse, mat_mul, rigid};
use solidcraft_engine::geom::Vec3;

use crate::SolveApp;
use crate::dialogs::{Dialog, Kind};
use crate::gizmo::{self, Change, Drag};
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
        FREE => v.push(SelInput::new("Pivot", POINTS, false)),
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
        && let Ok(made) = transform(app, &p)
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
    d.extra.remove("pivot");
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
    app.dialog.as_ref().is_some_and(|d| matches!(d.kind, Kind::Move { .. }) && d.extra.contains_key("drag0"))
}

/// The current type's own move, when its picks and values make one.
fn made(app: &SolveApp, d: &Dialog) -> Option<Mat> {
    match step(app, d) {
        Ok(Step::Made(p)) => transform(app, &p).ok(),
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

/// The centre of the picked bodies where they are before this move (after the earlier types').
fn bodies_center(app: &SolveApp, d: &Dialog) -> Option<Vec3> {
    let st = match d.editing {
        Some((id, _)) => app.session.model.state_before(id),
        None => app.session.model.state(),
    };
    let mut bb: Option<(Vec3, Vec3)> = None;
    for n in items(d, 0).body_names() {
        let Some(b) = st.body(&n) else { continue };
        let x = b.mesh().bounds();
        bb = Some(match bb {
            None => (x.min, x.max),
            Some((lo, hi)) => (
                Vec3::new(lo.x.min(x.min.x), lo.y.min(x.min.y), lo.z.min(x.min.z)),
                Vec3::new(hi.x.max(x.max.x), hi.y.max(x.max.y), hi.z.max(x.max.z)),
            ),
        });
    }
    bb.map(|(lo, hi)| carried(d, true, (lo + hi) * 0.5))
}

/// What a Free Move turns about: the picked pivot, else the kept one (editing), else the
/// bodies' centre.
fn pivot(app: &SolveApp, d: &Dialog) -> Option<Vec3> {
    if let Some(p) = sel_point(app, d, items(d, 1).first()) {
        return Some(p);
    }
    if let Some(a) = d.extra.get("pivot").and_then(|v| serde_json::from_value::<[f64; 3]>(v.clone()).ok()) {
        return Some(Vec3::new(a[0], a[1], a[2]));
    }
    bodies_center(app, d)
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

fn mm(v: f64) -> String {
    let s = format!("{v:.6}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    format!("{} mm", if s == "-0" || s.is_empty() { "0" } else { s })
}

fn deg(v: f64) -> String {
    let s = format!("{:.4}", v.to_degrees());
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
            let made = transform(app, &p)?;
            with_transform(p, &mat_mul(&made, &c))
        }
        (Step::Needs(_), Some(c)) => with_transform(json!({ "bodies": items(d, 0).body_names() }), &c),
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

/// The rigid transform of solid.move parameters: the turn about the origin, then the translation.
fn transform(app: &SolveApp, p: &Value) -> Result<Mat, String> {
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
    Ok(rigid(t, Vec3::ZERO, axis, value(angle, ang)?))
}

/// `p` with its translation and turn replaced by `m`'s.
fn with_transform(mut p: Value, m: &Mat) -> Value {
    let rows = [0, 1, 2].map(|r| [m[0][r], m[1][r], m[2][r]]);
    let (axis, th) = gizmo::axis_angle(&rows);
    p["translate"] = json!([mm(m[3][0]), mm(m[3][1]), mm(m[3][2])]);
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
        FREE => {
            let a = [ang(app, rx)?, ang(app, ry)?, ang(app, rz)?];
            let turned: Vec<usize> = (0..3).filter(|i| a[*i].abs() > 1e-12).collect();
            if turned.is_empty() {
                p["translate"] = json!([x, y, z]);
                // A rotation kept from an edited move about another axis.
                for k in ["axis", "angle"] {
                    if let Some(v) = d.extra.get(k) {
                        p[k] = v.clone();
                    }
                }
            } else {
                let piv = pivot(app, d).ok_or("select bodies first")?;
                let r = gizmo::mul(&gizmo::mul(&gizmo::rotation(Vec3::Z, a[2]), &gizmo::rotation(Vec3::Y, a[1])), &gizmo::rotation(Vec3::X, a[0]));
                let (axis, th) = gizmo::axis_angle(&r);
                match turned.as_slice() {
                    // One turn about the origin: the typed values stay as they are.
                    [i] if piv.len() < 1e-9 => {
                        p["translate"] = json!([x, y, z]);
                        p["axis"] = json!([[1, 0, 0], [0, 1, 0], [0, 0, 1]][*i]);
                        p["angle"] = json!([rx, ry, rz][*i]);
                    }
                    _ => {
                        let t = Vec3::new(len(app, x)?, len(app, y)?, len(app, z)?);
                        p["translate"] = num(piv - gizmo::apply(&r, piv) + t);
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
            p["translate"] = num(o - gizmo::apply(&r, o));
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

/// Where the triad sits: the pivot carried by the move so far (Free Move and Translate only).
pub fn triad(app: &SolveApp, d: &Dialog) -> Option<gizmo::Triad> {
    let Kind::Move { mode, x, y, z, .. } = &d.kind else { return None };
    if !matches!(*mode, FREE | TRANSLATE) || bodies_empty(d) {
        return None;
    }
    let t = Vec3::new(len(app, x).unwrap_or(0.0), len(app, y).unwrap_or(0.0), len(app, z).unwrap_or(0.0));
    Some(gizmo::Triad { center: pivot(app, d)? + t, translate: true, rotate: *mode == FREE })
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
    let Kind::Move { x, y, z, rx, ry, rz, .. } = &d.kind else { return };
    if g.started || !d.extra.contains_key("drag0") {
        let v = [len(app, x), len(app, y), len(app, z), ang(app, rx), ang(app, ry), ang(app, rz)].map(|r| r.unwrap_or(0.0));
        d.extra.insert("drag0".into(), json!(v));
    }
    let v0: [f64; 6] = d.extra.get("drag0").and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_default();
    let Kind::Move { x, y, z, rx, ry, rz, .. } = &mut d.kind else { return };
    match g.change {
        Change::Translate(t) => {
            *x = crate::length_input::model(&app.session.doc, v0[0] + t.x);
            *y = crate::length_input::model(&app.session.doc, v0[1] + t.y);
            *z = crate::length_input::model(&app.session.doc, v0[2] + t.z);
        }
        Change::Rotate { axis, angle } => {
            let slot = match axis {
                0 => rx,
                1 => ry,
                _ => rz,
            };
            *slot = deg(v0.get(3 + axis.min(2)).copied().unwrap_or(0.0) + angle);
        }
    }
    if g.done {
        d.extra.remove("drag0");
    }
}

/// The dialog for editing a move: Free Move with its translation and, about a world axis
/// through the origin, its angle (another axis is kept as it is).
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
                None => {
                    d.extra.insert("axis".into(), json!([dir.x, dir.y, dir.z]));
                    d.extra.insert("angle".into(), json!(a));
                }
            }
            // The stored move turns about the origin.
            d.extra.insert("pivot".into(), json!([0.0, 0.0, 0.0]));
        }
    }
    d.kind = k;
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
