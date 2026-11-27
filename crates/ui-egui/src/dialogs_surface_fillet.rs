//! The Surface Fillet and Surface Chamfer dialogs (#769): a point on each of two surface bodies
//! (which also picks the side of the other surface that is kept), the radius, or the chamfer's
//! distance with a second distance or an angle, what is left of the two surfaces (Trim) and
//! Extend. The dialog only gathers the values: `surface.fillet` and `surface.chamfer` build the
//! round or the bevel, and what they refuse shows in the dialog as they word it. Editing either
//! feature reopens its dialog with the stored values and picks.

use serde_json::{Map, Value, json};
use solidcraft_engine::Sel;
use solidcraft_engine::doc::{FaceAt, FeatureKind, FilletTrim};

use crate::SolveApp;
use crate::dialogs_surface::Surf;
use crate::selection::{FACES, SelInput, items, point_value as pt};
use crate::ui_kit::{self, BackingTooltipExt as _, choice_row, expr_row, expr_row_positive};

/// The two pick inputs: a point on each surface body.
const FIRST: &str = "ui.surface_round_first";
const SECOND: &str = "ui.surface_round_second";

/// What is left of the two surfaces (labels, and the `trim` parameter).
pub(crate) const TRIMS: [(&str, &str); 3] =
    [("ui.surface_round_trim_both", "both"), ("ui.surface_round_trim_first", "first"), ("ui.surface_round_trim_none", "none")];

/// The Trim choice of a stored feature.
pub(crate) fn trim_index(trim: FilletTrim) -> usize {
    match trim {
        FilletTrim::Both => 0,
        FilletTrim::First => 1,
        FilletTrim::None => 2,
    }
}

/// The chamfer types the dialog offers, as Chamfer's: an equal distance, two distances, or a
/// distance and an angle (indices into [`crate::dialogs::CHAMFER_TYPES`]).
const CHAMFER_EQUAL: usize = 0;
const CHAMFER_TWO: usize = 1;
const CHAMFER_ANGLE: usize = 2;

/// The chamfer type of a stored chamfer: a second distance, an angle, or neither.
pub(crate) fn chamfer_type(distance2: bool, angle: bool) -> usize {
    if distance2 {
        CHAMFER_TWO
    } else if angle {
        CHAMFER_ANGLE
    } else {
        CHAMFER_EQUAL
    }
}

/// The pick inputs of both dialogs: a face of each surface body, at the point that picks its side.
pub(crate) fn inputs() -> Vec<SelInput> {
    vec![SelInput::new(FIRST, FACES, false).tip("ui.surface_round_first_tip"), SelInput::new(SECOND, FACES, false).tip("ui.surface_round_second_tip")]
}

/// The Trim row and the options both dialogs share.
fn trim_row(ui: &mut egui::Ui, id: &str, trim: &mut usize) {
    choice_row(ui, "ui.trim", id, trim, &TRIMS.map(|(label, _)| label)).backing_on_hover_text(solidcraft_ui_text::tr("ui.surface_round_trim_tip"));
}

/// The Surface Fillet rows; true when Enter was pressed in a field.
pub(crate) fn fillet_rows(ui: &mut egui::Ui, radius: &mut String, trim: &mut usize, extend: &mut bool) -> bool {
    let enter = expr_row_positive(ui, "Radius", radius);
    trim_row(ui, "sf_trim", trim);
    ui_kit::options(ui, "surface_fillet_options", &mut [("Extend to meet", extend)]);
    enter
}

/// The Surface Chamfer rows; true when Enter was pressed in a field. The second side is a
/// distance or an angle, never both: the type chooses which row shows.
#[allow(clippy::too_many_arguments)]
pub(crate) fn chamfer_rows(
    ui: &mut egui::Ui,
    distance: &mut String,
    ctype: &mut usize,
    distance2: &mut String,
    angle: &mut String,
    flip: &mut bool,
    trim: &mut usize,
    extend: &mut bool,
) -> bool {
    choice_row(ui, "Chamfer Type", "sc_type", ctype, &crate::dialogs::CHAMFER_TYPES);
    let mut enter = expr_row_positive(ui, if *ctype == CHAMFER_EQUAL { "Distance" } else { "Distance 1" }, distance);
    match *ctype {
        CHAMFER_TWO => enter |= expr_row_positive(ui, "Distance 2", distance2),
        CHAMFER_ANGLE => enter |= expr_row(ui, "Angle", angle),
        _ => {}
    }
    trim_row(ui, "sc_trim", trim);
    // Flip swaps which surface is the first (the one Distance 1 and the angle are measured on).
    ui_kit::options(ui, "surface_chamfer_options", &mut [("Flip", flip), ("Extend to meet", extend)]);
    enter
}

/// `{body, point}` of a face pick, or the stored one under `key` when nothing is picked (a face
/// an edit could not find again), else the message asking for it.
fn side(app: &SolveApp, picks: &[Sel], extra: &Map<String, Value>, key: &str, ask: &str) -> Result<Value, String> {
    match picks.first() {
        Some(Sel::Face { body, point, .. }) => Ok(json!({"body": body, "point": pt(*point)})),
        Some(_) => Err(app.tr(ask).to_string()),
        None => extra.get(key).filter(|v| !v.is_null()).cloned().ok_or_else(|| app.tr(ask).to_string()),
    }
}

/// The `trim` parameter: left out for the default (both).
fn put_trim(p: &mut Map<String, Value>, trim: usize) {
    if let Some((_, word)) = TRIMS.get(trim).filter(|_| trim != 0) {
        p.insert("trim".into(), json!(word));
    }
}

/// The command a Surface Fillet or Surface Chamfer dialog makes: its two picks and values, as
/// the command takes them (defaults left out, as the command stores them).
pub(crate) fn command(app: &SolveApp, k: &Surf, inputs: &[SelInput], extra: &Map<String, Value>) -> Result<(&'static str, Value), String> {
    let a = side(app, items(inputs, 0), extra, "a", "ui.surface_round_select_first")?;
    let b = side(app, items(inputs, 1), extra, "b", "ui.surface_round_select_second")?;
    let mut p = Map::from_iter([("a".into(), a), ("b".into(), b)]);
    let cmd = match k {
        Surf::Fillet { radius, trim, extend } => {
            p.insert("radius".into(), json!(radius));
            put_trim(&mut p, *trim);
            if *extend {
                p.insert("extend".into(), json!(true));
            }
            "surface.fillet"
        }
        Surf::Chamfer { distance, ctype, distance2, angle, flip, trim, extend } => {
            p.insert("distance".into(), json!(distance));
            match *ctype {
                CHAMFER_TWO => {
                    p.insert("distance2".into(), json!(distance2));
                }
                CHAMFER_ANGLE => {
                    p.insert("angle".into(), json!(angle));
                }
                _ => {}
            }
            if *flip {
                p.insert("flip".into(), json!(true));
            }
            put_trim(&mut p, *trim);
            if *extend {
                p.insert("extend".into(), json!(true));
            }
            "surface.chamfer"
        }
        _ => return Err("not a surface fillet or chamfer".into()),
    };
    Ok((cmd, Value::Object(p)))
}

/// The picks of a stored Surface Fillet or Chamfer: each face found again on its body at the
/// stored point, else kept as stored (under `a` or `b`) so OK sends it back unchanged.
pub(crate) fn for_feature(app: &SolveApp, kind: &FeatureKind) -> Option<(Vec<Vec<Sel>>, Map<String, Value>)> {
    let (a, b) = match kind {
        FeatureKind::SurfaceFillet { a, b, .. } | FeatureKind::SurfaceChamfer { a, b, .. } => (a, b),
        _ => return None,
    };
    let mut kept = Map::new();
    let mut pick = |key: &str, at: &FaceAt| -> Vec<Sel> {
        match crate::dialogs::face_sel_on(&app.session, Some(at.body.as_ref()), at.point.0) {
            Some(sel) => vec![sel],
            None => {
                kept.insert(key.into(), json!({"body": at.body, "point": pt(at.point.0)}));
                Vec::new()
            }
        }
    };
    let fill = vec![pick("a", a), pick("b", b)];
    Some((fill, kept))
}
