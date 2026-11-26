//! The Offset Curve dialog (#22414): `surface.offset_curve` from the SURFACE tab while a sketch is
//! being edited. Picks: the face (where it is clicked also picks the side, for a curve inside the
//! face) and the edge of that face, or curve of another sketch lying on it, to offset from. Values:
//! the distance along the face, Flip (a curve's other side) and the fit tolerance (empty: the
//! command's default). The curve is previewed live; the command's refusals show in the dialog and
//! leave the sketch as it was. After OK the status line gives the deviation the command reports.
//!
//! An offset curve is a link of its sketch, not a feature of its own: Edit Offset Curve (the
//! context menu of its 3D curve) reopens the dialog with the link's stored values, and OK changes
//! that link in place (`surface.offset_curve`'s `link`). The dialog computes nothing: the lengths
//! are the design's expressions evaluated to mm, the rest is the command's.

use serde_json::{Value, json};
use solidcraft_engine::Sel;
use solidcraft_engine::doc::expr::Kind as ValueKind;
use solidcraft_engine::sketch::LinkSource;

use crate::SolveApp;
use crate::dialogs::{Dialog, Kind};
use crate::dialogs_surface::Surf;
use crate::selection::{EDGES, FACES, SKETCH_LINES, SelInput, items};
use crate::ui_kit;
use crate::ui_kit::{expr_row, hint_row};

/// The command the dialog runs.
pub const COMMAND: &str = "surface.offset_curve";

/// The second pick's label (and its row's handle): what the curve is offset from.
pub const FROM: &str = "ui.offset_curve_from";

/// The dialog's values.
#[derive(Clone, Debug, PartialEq)]
pub struct OffsetCurveOpts {
    /// How far along the face, as typed (an expression of the design).
    pub distance: String,
    /// From a curve on the face: to the other side of it than the picked face point.
    pub flip: bool,
    /// How far the fitted curve may be from the true offset; empty for the command's default.
    pub tolerance: String,
    /// The link changed on OK when an existing offset curve is edited; `None` adds one.
    pub link: Option<String>,
}

impl OffsetCurveOpts {
    /// A new offset curve's values: `distance` (the design unit's default) and the rest unset.
    pub fn new(distance: String) -> OffsetCurveOpts {
        OffsetCurveOpts { distance, flip: false, tolerance: String::new(), link: None }
    }
}

/// The picks: the face, and the edge or curve to offset from.
pub fn inputs() -> Vec<SelInput> {
    vec![SelInput::new("Face", FACES, false), SelInput::new(FROM, EDGES | SKETCH_LINES, false).tip("ui.offset_curve_from_tip")]
}

/// The rows after the picks: Distance, Tolerance and Flip. True when Enter was pressed in a field.
pub fn rows(ui: &mut egui::Ui, o: &mut OffsetCurveOpts) -> bool {
    let mut enter = expr_row(ui, "Distance", &mut o.distance);
    enter |= hint_row(ui, "Tolerance", &mut o.tolerance, solidcraft_ui_text::tr("ui.offset_curve_tolerance_hint"));
    ui_kit::options(ui, "offset_curve_options", &mut [("Flip", &mut o.flip)]);
    enter
}

/// A length field's value in mm (the command takes numbers), or why not.
fn length(app: &SolveApp, label: &str, text: &str) -> Result<f64, String> {
    crate::length_input::no_definition(text, label)?;
    crate::length_input::eval(&app.session.doc, text.trim(), ValueKind::Length).map_err(|e| format!("{}: {e}", solidcraft_ui_text::tr_label(label)))
}

/// `surface.offset_curve`'s parameters from the dialog: the face point (and its body), what to
/// offset from, the distance, and the tolerance, Flip and the edited link when they are set.
pub fn command(app: &SolveApp, o: &OffsetCurveOpts, inputs: &[SelInput]) -> Result<(&'static str, Value), String> {
    let Some(Sel::Face { body, point, .. }) = items(inputs, 0).first() else {
        return Err(solidcraft_ui_text::tr("ui.offset_curve_pick_face").to_string());
    };
    let from = match items(inputs, 1).first() {
        Some(Sel::Edge { body, point, .. }) => json!({"edge": [point.x, point.y, point.z], "body": body}),
        // A curve of the sketch being edited would offset into itself: the curve must be another's.
        Some(Sel::ExternalSketchCurve { sketch, .. } | Sel::SketchCurve { sketch: Some(sketch), .. })
            if Some(*sketch) == app.session.active_sketch =>
        {
            return Err(solidcraft_ui_text::tr("ui.offset_curve_other_sketch").to_string());
        }
        Some(Sel::ExternalSketchCurve { sketch, id } | Sel::SketchCurve { sketch: Some(sketch), id }) => json!({"sketch": sketch, "curve": id}),
        Some(Sel::SketchCurve { sketch: None, .. }) => return Err(solidcraft_ui_text::tr("ui.offset_curve_other_sketch").to_string()),
        _ => return Err(solidcraft_ui_text::tr("ui.offset_curve_pick_from").to_string()),
    };
    let distance = length(app, "Distance", &o.distance)?;
    let mut p = json!({"face": [point.x, point.y, point.z], "body": body, "from": from, "distance": distance});
    if !o.tolerance.trim().is_empty() {
        p["tolerance"] = json!(length(app, "Tolerance", &o.tolerance)?);
    }
    if o.flip {
        p["flip"] = json!(true);
    }
    if let Some(link) = &o.link {
        p["link"] = json!(link);
    }
    Ok((COMMAND, p))
}

/// The dialog's values and picks for the offset curve `link` whose stored source is `source`:
/// its face and what it is offset from picked again where they are stored, its numbers in the
/// design's unit. `None` when `source` is not an offset curve.
pub fn for_link(app: &SolveApp, link: &str, source: &LinkSource) -> Option<(OffsetCurveOpts, Vec<SelInput>)> {
    let LinkSource::FaceOffset { body, at, from, distance, tolerance, flip } = source else { return None };
    let s = &app.session;
    let only = |b: &str| (!b.is_empty()).then_some(b.to_string());
    let face = crate::dialogs::face_sel_on(s, only(body).as_deref(), *at);
    let from = match from.as_ref() {
        LinkSource::Edge { body: of, at } => crate::dialogs::edge_sel_on(s, only(of).as_deref(), *at),
        LinkSource::SketchCurve { sketch, curve } => Some(Sel::ExternalSketchCurve { sketch: *sketch, id: curve.clone() }),
        _ => None,
    };
    let o = OffsetCurveOpts {
        distance: crate::length_input::model(&s.doc, *distance),
        flip: *flip,
        tolerance: tolerance.map(|t| crate::length_input::model(&s.doc, t)).unwrap_or_default(),
        link: Some(link.to_string()),
    };
    let mut ins = inputs();
    for (input, pick) in ins.iter_mut().zip([face, from]) {
        input.items = pick.into_iter().collect();
    }
    Some((o, ins))
}

/// The offset curve's link the selection is, when it is one 3D curve of the sketch being edited
/// that an offset curve made.
pub(crate) fn selected(app: &SolveApp) -> Option<String> {
    let [Sel::SketchCurve { id, sketch: None }] = app.session.selection.as_slice() else { return None };
    let sid = app.session.active_sketch?;
    let state = crate::open_meshing::session_world(&app.session);
    let sk = &state.sketch(sid)?.sketch;
    let link = sk.wires.get(sk.wire_index(id)?)?.link.clone()?;
    matches!(sk.link(&link)?.source, LinkSource::FaceOffset { .. }).then_some(link)
}

/// Reopen the dialog on the offset curve `link` of the sketch being edited, with its stored
/// values; OK changes that link. False when there is no such offset curve.
pub fn edit(app: &mut SolveApp, link: &str) -> bool {
    let Some(sid) = app.session.active_sketch else { return false };
    let state = crate::open_meshing::session_world(&app.session);
    let Some(source) = state.sketch(sid).and_then(|s| s.sketch.link(link)).map(|l| l.source.clone()) else { return false };
    let Some((o, ins)) = for_link(app, link, &source) else { return false };
    app.tool = None;
    app.dialog = Some(Dialog::new(Kind::Surface(Surf::OffsetCurve(o)), ins));
    true
}

/// What the status line says after OK: the deviation the command reports, and whether the face's
/// edge cut the curve short. `None` for a reply without a deviation.
pub fn status(reply: &Value) -> Option<String> {
    let deviation = reply.get("deviation").and_then(Value::as_f64)?;
    let id = if reply.get("clipped").and_then(Value::as_bool).unwrap_or(false) { "ui.offset_curve_done_clipped" } else { "ui.offset_curve_done" };
    Some(solidcraft_ui_text::format(id, &[("deviation", &format!("{deviation:.6}"))]))
}

#[cfg(test)]
#[path = "dialogs_offset_curve_tests.rs"]
mod tests;
