//! Move/Copy: the move types (Free Move, Translate, Rotate, Point to Point, Point to Position),
//! their rows and inputs, the triad on the canvas and the solid.move they make. Nothing
//! moves until a value is typed or the triad is dragged.

use crate::selection::SelectionExt as _;
use crate::ui_kit::{choice_row, expr_row, field, row};
use serde_json::{Value, json};
use solidcraft_engine::Sel;
use solidcraft_engine::doc::expr::Kind as ValueKind;
use solidcraft_engine::geom::Vec3;

use crate::SolveApp;
use crate::dialogs::{Dialog, Kind};
use crate::gizmo::{self, Change, Drag};
use crate::selection::{AXES, BODIES, EDGES, FACES, ORIGIN, SelInput, VERTICES};

pub const TYPES: [&str; 5] = ["Free Move", "Translate", "Rotate", "Point to Point", "Point to Position"];
pub const FREE: usize = 0;
pub const TRANSLATE: usize = 1;
pub const ROTATE: usize = 2;
pub const POINT_TO_POINT: usize = 3;
pub const POINT_TO_POSITION: usize = 4;
/// The per-axis lock options (label, `Dialog::extra` key): a locked axis ignores triad drags.
/// A viewport constraint only; the solid.move parameters are unchanged.
pub const LOCKS: [(&str, &str); 3] = [("Lock X", "lock_x"), ("Lock Y", "lock_y"), ("Lock Z", "lock_z")];
const POINTS: u16 = VERTICES | FACES | EDGES | ORIGIN;

/// A fresh Move dialog of a type (all values zero).
pub fn kind(mode: usize) -> Kind {
    let z = || "0 mm".to_string();
    let a = || "0 deg".to_string();
    Kind::Move { mode, x: z(), y: z(), z: z(), rx: a(), ry: a(), rz: a(), angle: a() }
}

/// The selection inputs of a move type (the bodies first).
pub fn inputs(mode: usize) -> Vec<SelInput> {
    let mut v = vec![SelInput::new("Bodies", BODIES, true)];
    match mode {
        FREE => v.push(SelInput::new("Pivot", POINTS, false)),
        ROTATE => v.push(SelInput::new("Axis", AXES | EDGES, false)),
        POINT_TO_POINT => {
            v.push(SelInput::new("From", POINTS, false));
            v.push(SelInput::new("To", POINTS, false));
        }
        POINT_TO_POSITION => v.push(SelInput::new("Point", POINTS, false)),
        _ => {}
    }
    v
}

/// The Move Type row (above the inputs); switching keeps the bodies and resets the values.
pub fn type_row(d: &mut Dialog, ui: &mut egui::Ui) {
    let Kind::Move { mode, .. } = d.kind else { return };
    let mut m = mode;
    choice_row(ui, "Move Type", "move_type", &mut m, &TYPES);
    if m != mode {
        let bodies = d.inputs.first().map(|i| i.items.clone()).unwrap_or_default();
        d.kind = kind(m);
        d.inputs = inputs(m);
        if let Some(i) = d.inputs.first_mut() {
            i.items = bodies;
        }
        d.extra.remove("pivot");
        d.active = if bodies_empty(d) { 0 } else { 1.min(d.inputs.len() - 1) };
    }
}

fn bodies_empty(d: &Dialog) -> bool {
    d.inputs.first().is_none_or(|i| i.items.is_empty())
}

/// The value rows after the inputs; true when Enter was pressed in one.
pub fn rows(ui: &mut egui::Ui, k: &mut Kind, extra: &mut serde_json::Map<String, Value>) -> bool {
    let Kind::Move { mode, x, y, z, rx, ry, rz, angle } = k else { return false };
    let mut enter = false;
    match *mode {
        FREE | TRANSLATE => {
            enter |= lock_row(ui, "X Distance", x, extra, 0);
            enter |= lock_row(ui, "Y Distance", y, extra, 1);
            enter |= lock_row(ui, "Z Distance", z, extra, 2);
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

/// A distance row with a lock toggle at its end: a locked axis ignores triad drags. The toggle's
/// width is taken from the field, so it stays inside the row.
fn lock_row(ui: &mut egui::Ui, label: &str, value: &mut String, extra: &mut serde_json::Map<String, Value>, axis: usize) -> bool {
    let Some((name, key)) = LOCKS.get(axis).copied() else { return false };
    row(ui, label, |ui| {
        let t = crate::theme::Tokens::get();
        let size = t.form_close_size;
        // The field fills the width it is given, so give it the row less the toggle.
        let full = ui.max_rect();
        let field_rect = egui::Rect::from_min_max(full.min, egui::pos2(full.right() - size - 4.0, full.max.y));
        let enter = ui
            .scope_builder(egui::UiBuilder::new().max_rect(field_rect).layout(egui::Layout::left_to_right(egui::Align::Center)), |ui| {
                field(ui, value)
            })
            .inner;
        let on = extra.get(key).and_then(Value::as_bool).unwrap_or(false);
        let rect = egui::Rect::from_center_size(egui::pos2(full.right() - size / 2.0, full.center().y), egui::vec2(size, size));
        let r = ui.interact(rect, ui.make_persistent_id(("move_lock", key)), egui::Sense::click());
        if r.hovered() {
            ui.painter().rect_filled(rect, t.control_radius, t.hover);
        }
        let ink = if on { t.accent } else { t.form_dim };
        crate::icons::glyphs::badge(ui.painter(), egui::Rect::from_center_size(rect.center(), egui::vec2(12.0, 12.0)), "lock", ink);
        crate::scenario::publish_handle(&format!("move:lock:{}", &key["lock_".len()..]), rect.center());
        if r.clicked() {
            extra.insert(key.into(), Value::Bool(!on));
        }
        r.on_hover_text(crate::i18n::tr_label(name));
        enter
    })
}

fn sel_point(s: Option<&Sel>) -> Option<Vec3> {
    match s? {
        Sel::Vertex { point, .. } | Sel::Face { point, .. } | Sel::Edge { point, .. } => Some(*point),
        _ => None,
    }
}

fn items(d: &Dialog, i: usize) -> &[Sel] {
    crate::selection::items(&d.inputs, i)
}

/// The centre of the picked bodies where they are before this move.
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
    bb.map(|(lo, hi)| (lo + hi) * 0.5)
}

/// What a Free Move turns about: the picked pivot, else the kept one (editing), else the
/// bodies' centre.
fn pivot(app: &SolveApp, d: &Dialog) -> Option<Vec3> {
    if let Some(p) = sel_point(items(d, 1).first()) {
        return Some(p);
    }
    if let Some(a) = d.extra.get("pivot").and_then(|v| serde_json::from_value::<[f64; 3]>(v.clone()).ok()) {
        return Some(Vec3::new(a[0], a[1], a[2]));
    }
    bodies_center(app, d)
}

fn len(app: &SolveApp, s: &str) -> Result<f64, String> {
    app.session.doc.eval(s, ValueKind::Length).map_err(|e| e.to_string()).and_then(|v| if v.is_finite() { Ok(v) } else { Err("not a number".into()) })
}

fn ang(app: &SolveApp, s: &str) -> Result<f64, String> {
    app.session.doc.eval(s, ValueKind::Angle).map_err(|e| e.to_string()).and_then(|v| if v.is_finite() { Ok(v) } else { Err("not a number".into()) })
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

/// The solid.move parameters (bodies, translate, axis, angle).
pub fn params(app: &SolveApp, d: &Dialog) -> Result<Value, String> {
    let Kind::Move { mode, x, y, z, rx, ry, rz, angle } = &d.kind else { return Err("not a move".into()) };
    let bodies = items(d, 0).body_names();
    if bodies.is_empty() {
        return Err("select bodies first".into());
    }
    let need_point = |i: usize, what: &str| sel_point(items(d, i).first()).ok_or(format!("select {what} first"));
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
            let (o, a) = items(d, 1).first().and_then(|s| axis_line(app, s)).ok_or("select an axis or a straight edge first")?;
            let th = ang(app, angle)?;
            let r = gizmo::rotation(a, th);
            p["translate"] = num(o - gizmo::apply(&r, o));
            p["axis"] = json!([a.x, a.y, a.z]);
            p["angle"] = json!(angle);
        }
        POINT_TO_POINT => {
            let (from, to) = (need_point(1, "the point to move from")?, need_point(2, "the point to move to")?);
            p["translate"] = num(to - from);
        }
        _ => {
            let from = need_point(1, "the point to move")?;
            let at = Vec3::new(len(app, x)?, len(app, y)?, len(app, z)?);
            p["translate"] = num(at - from);
        }
    }
    if d.extra.get("copy").and_then(Value::as_bool).unwrap_or(false) {
        p["copy"] = json!(true);
    }
    Ok(p)
}

/// An axis line: origin axes and sketch lines, or a straight body edge.
fn axis_line(app: &SolveApp, s: &Sel) -> Option<(Vec3, Vec3)> {
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

/// Whether the move type has a dragged translation the axis locks can hold still.
pub fn has_locks(k: &Kind) -> bool {
    matches!(k, Kind::Move { mode, .. } if matches!(*mode, FREE | TRANSLATE))
}

fn locked(d: &Dialog, axis: usize) -> bool {
    LOCKS.get(axis).is_some_and(|(_, key)| d.extra.get(*key).and_then(Value::as_bool).unwrap_or(false))
}

/// Apply a triad drag to the dialog's values (added to the values when the drag began).
pub fn drag(app: &SolveApp, d: &mut Dialog, g: Drag) {
    let Kind::Move { x, y, z, rx, ry, rz, .. } = &d.kind else { return };
    if g.started || !d.extra.contains_key("drag0") {
        let v = [len(app, x), len(app, y), len(app, z), ang(app, rx), ang(app, ry), ang(app, rz)].map(|r| r.unwrap_or(0.0));
        d.extra.insert("drag0".into(), json!(v));
    }
    let v0: [f64; 6] = d.extra.get("drag0").and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_default();
    let keep = [locked(d, 0), locked(d, 1), locked(d, 2)];
    let Kind::Move { x, y, z, rx, ry, rz, .. } = &mut d.kind else { return };
    match g.change {
        Change::Translate(t) => {
            // A locked axis keeps the value it had when the drag began.
            let pick = |i: usize, dt: f64| if keep[i] { v0[i] } else { v0[i] + dt };
            *x = mm(pick(0, t.x));
            *y = mm(pick(1, t.y));
            *z = mm(pick(2, t.z));
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
    d: &mut Dialog,
    bodies: &[String],
    translate: &[solidcraft_engine::doc::Expr; 3],
    axis: Option<Vec3>,
    angle: Option<&solidcraft_engine::doc::Expr>,
    copy: bool,
) {
    let [x, y, z] = translate.clone().map(String::from);
    let mut k = kind(FREE);
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
