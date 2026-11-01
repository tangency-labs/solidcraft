//! Where a primitive goes, typed in its dialog (#5638). The Box is placed by its corner or its
//! centre, the Cylinder and Cone by the centre of their base, the Sphere and Torus by their
//! centre, as X, Y and Z fields that take expressions and units like the size fields. A point
//! picked in the view (a vertex, a point on an edge or face, the origin) fills the fields of the
//! Box, Sphere and Torus.
//!
//! The placement lives in the dialog's extra parameters under the command's own key (`corner`,
//! `center` or `base`). It is written there only once a field or the choice changes, so a new
//! dialog left alone sends no placement (the box stays at the origin, as before), and an edited
//! feature's stored point goes back unchanged.

use serde_json::{Map, Value, json};
use solidcraft_engine::Session;
use solidcraft_engine::doc::expr::Kind as ValueKind;
use solidcraft_engine::geom::Vec3;

use crate::i18n::tr;
use crate::selection::{EDGES, FACES, ORIGIN, SelInput, VERTICES};
use crate::ui_kit::{self, choice_row, expr_row};
use solidcraft_engine::Sel;

/// The placement keys a primitive's dialog offers, the choice's first option first.
fn keys(cmd: &str) -> &'static [&'static str] {
    match cmd {
        "solid.box" => &["corner", "center"],
        "solid.cylinder" | "solid.cone" => &["base"],
        "solid.sphere" | "solid.torus" => &["center"],
        _ => &[],
    }
}

/// The Box's choice of the point its fields place.
const BOX_PLACEMENTS: [&str; 2] = ["Corner", "Center"];

/// The selection inputs of a primitive's dialog: the Box takes a point for its corner or centre,
/// the Sphere and Torus for their centre.
pub(crate) fn inputs(cmd: &str) -> Vec<SelInput> {
    match cmd {
        "solid.box" | "solid.sphere" | "solid.torus" => {
            vec![SelInput::new("Point", VERTICES | EDGES | FACES | ORIGIN, false).tip("dialog.primitive_point_tip")]
        }
        _ => Vec::new(),
    }
}

/// The text a placement coordinate shows: typed text as it is, a stored number (millimetres) as
/// the edit fields show stored lengths.
fn shown(s: &Session, v: Option<&Value>) -> String {
    match v {
        Some(Value::String(t)) => t.clone(),
        // "10", not the JSON number's "10.0".
        Some(Value::Number(n)) => crate::dialogs::stored_length(s, &n.as_f64().map_or_else(|| n.to_string(), |v| v.to_string())),
        _ => crate::dialogs::length_default(s, "0"),
    }
}

/// The current placement: which key, and its three coordinates as the fields show them.
fn current(s: &Session, cmd: &str, extra: &Map<String, Value>) -> Option<(usize, [String; 3])> {
    let keys = keys(cmd);
    let first = keys.first()?;
    let index = keys.iter().position(|k| extra.contains_key(*k)).unwrap_or(0);
    let point = extra.get(keys.get(index).copied().unwrap_or(first)).and_then(Value::as_array);
    let coord = |i: usize| shown(s, point.and_then(|p| p.get(i)));
    Some((index, [coord(0), coord(1), coord(2)]))
}

/// Store a placement under `key`, replacing the other choice.
fn store(cmd: &str, extra: &mut Map<String, Value>, index: usize, coords: &[String; 3]) {
    let keys = keys(cmd);
    let Some(key) = keys.get(index) else { return };
    for k in keys {
        extra.remove(*k);
    }
    extra.insert((*key).into(), json!(coords));
}

/// The placement rows (shown while no pre-selected face or vertex places the primitive). True
/// when Enter was pressed in a field.
pub(crate) fn rows(ui: &mut egui::Ui, s: &Session, cmd: &str, extra: &mut Map<String, Value>) -> bool {
    let Some((index, mut coords)) = current(s, cmd, extra) else { return false };
    let mut choice = index;
    if keys(cmd).len() > 1 {
        choice_row(ui, "Placement", "pr_place", &mut choice, &BOX_PLACEMENTS);
    } else {
        let point = if keys(cmd) == ["center"] { tr("ui.center") } else { tr("Base Center") };
        ui_kit::row(ui, "Placement", |ui| {
            ui_kit::text(ui, point, ui_kit::Text::Cell);
        });
    }
    let before = coords.clone();
    let mut enter = false;
    for (label, value) in ["X", "Y", "Z"].into_iter().zip(coords.iter_mut()) {
        enter |= expr_row(ui, label, value);
    }
    if choice != index || coords != before {
        store(cmd, extra, choice, &coords);
    }
    enter
}

/// A coordinate of a picked point as a field shows it, in millimetres.
fn mm(v: f64) -> String {
    let v = (v * 1e6).round() / 1e6;
    // No "-0 mm".
    let v = if v == 0.0 { 0.0 } else { v };
    format!("{v} mm")
}

/// A point picked into the Box's input: it goes into the X, Y and Z of the current placement
/// (the corner or the centre), replacing a pre-selected face or vertex.
pub(crate) fn picked(s: &Session, cmd: &str, at: &mut Option<(Vec3, Option<Vec3>)>, extra: &mut Map<String, Value>, sel: Option<&Sel>) {
    let point = match sel {
        Some(Sel::Vertex { point, .. } | Sel::Face { point, .. } | Sel::Edge { point, .. }) => *point,
        _ => return,
    };
    if !point.is_finite() || inputs(cmd).is_empty() {
        return;
    }
    let Some((index, _)) = current(s, cmd, extra) else { return };
    store(cmd, extra, index, &[mm(point.x), mm(point.y), mm(point.z)]);
    *at = None;
}

/// The typed placement into the command's parameters: a bare number typed into a field is in the
/// design's units (as in the size fields), an empty field is 0, and a stored number is kept.
pub(crate) fn params(s: &Session, cmd: &str, extra: &Map<String, Value>, p: &mut Map<String, Value>) {
    for key in keys(cmd) {
        let Some(Value::Array(point)) = extra.get(*key) else { continue };
        let coords: Vec<Value> = point
            .iter()
            .map(|c| match c {
                Value::String(t) if t.trim().is_empty() => json!(0),
                Value::String(t) => json!(s.doc.with_design_unit(t, ValueKind::Length)),
                other => other.clone(),
            })
            .collect();
        p.insert((*key).into(), Value::Array(coords));
    }
}
