//! The Face type of the pattern dialog (`solid.pattern.face`): copies of bodies standing on a
//! face, on a grid across it. Its attractors, which change each copy with its distance to
//! points, sit behind the Advanced section: switching them on adds the Attractors pick and
//! their near and far values.

use serde_json::{Value, json};
use solidcraft_engine::Sel;
use solidcraft_engine::doc::{Attract, Falloff, PatternKind};

use crate::SolveApp;
use crate::dialogs::{Dialog, Kind, form_row, set_object_accepting};
use crate::selection::{self, BODIES, FACES, SelInput, SelectionExt as _, point_value as pt};
use crate::ui_kit::{self, choice_row, expr_row, hint_row};

/// The pick input the attractor points go into, present while attractors are on.
pub(crate) const ATTRACTORS: &str = "Attractors";

/// What an attractor point may be: a vertex, a sketch point or a construction point.
pub(crate) const ATTRACTOR_PICKS: selection::Accept = selection::VERTICES | selection::POINTS | selection::CONSTRUCTION_POINTS;

/// The dialog's values; the picks live in the dialog's inputs.
#[derive(Clone, Debug, PartialEq)]
pub struct Form {
    pub count1: String,
    pub count2: String,
    /// 0: rows; 1: every other column half a cell along.
    pub layout: usize,
    pub angle: String,
    pub margin: String,
    pub flip: bool,
    /// The Advanced section's switch.
    pub attract: bool,
    pub near: String,
    pub far: String,
    pub scale_near: String,
    pub scale_far: String,
    pub offset_near: String,
    pub offset_far: String,
    pub turn_near: String,
    pub turn_far: String,
    pub cutoff: String,
    /// 0: linear; 1: smooth.
    pub falloff: usize,
    /// When editing: the saved face and attractor references, sent while nothing is picked
    /// in their place.
    pub saved_face: Option<Value>,
    pub saved_points: Vec<Value>,
}

impl Default for Form {
    fn default() -> Self {
        Form {
            count1: "5".into(),
            count2: "5".into(),
            layout: 0,
            angle: String::new(),
            margin: String::new(),
            flip: false,
            attract: false,
            near: String::new(),
            far: String::new(),
            scale_near: "1".into(),
            scale_far: "1".into(),
            offset_near: String::new(),
            offset_far: String::new(),
            turn_near: String::new(),
            turn_far: String::new(),
            cutoff: String::new(),
            falloff: 0,
            saved_face: None,
            saved_points: Vec::new(),
        }
    }
}

pub(crate) fn inputs() -> Vec<SelInput> {
    vec![SelInput::new("Objects", BODIES, true), SelInput::new("Face", FACES, false)]
}

/// The dialog for a new face pattern.
pub(crate) fn kind() -> Kind {
    Kind::FacePattern(Box::default())
}

/// The fields under the picks. Returns whether Enter was pressed in one.
pub(crate) fn draw(app: &SolveApp, d: &mut Dialog, ui: &mut egui::Ui) -> bool {
    let Kind::FacePattern(f) = &mut d.kind else { return false };
    let doc = &app.session.doc;
    let mut enter = false;
    enter |= crate::dialogs::quantity_row(ui, doc, "Across", &mut f.count1);
    enter |= crate::dialogs::quantity_row(ui, doc, "Along", &mut f.count2);
    choice_row(ui, "Layout", "fp_layout", &mut f.layout, &["Grid", "Staggered"]);
    enter |= hint_row(ui, "Grid angle", &mut f.angle, "none");
    enter |= hint_row(ui, "Margin", &mut f.margin, "none");
    form_row(ui, "", |ui| {
        ui_kit::checkbox(ui, "Flip side", &mut f.flip);
    });
    let mut attract = f.attract;
    ui_kit::section(ui, "face_pattern_advanced", "Advanced", f.attract, |ui| {
        form_row(ui, "", |ui| {
            ui_kit::checkbox(ui, "Attractors", &mut attract);
        });
        if attract {
            ui_kit::status_line(ui, "Each copy changes with its distance to the nearest attractor point", ui_kit::StatusTone::Dim);
            enter |= hint_row(ui, "Near distance", &mut f.near, "nearest copy");
            enter |= hint_row(ui, "Far distance", &mut f.far, "farthest copy");
            enter |= expr_row(ui, "Scale near", &mut f.scale_near);
            enter |= expr_row(ui, "Scale far", &mut f.scale_far);
            enter |= hint_row(ui, "Offset near", &mut f.offset_near, "none");
            enter |= hint_row(ui, "Offset far", &mut f.offset_far, "none");
            enter |= hint_row(ui, "Turn near", &mut f.turn_near, "none");
            enter |= hint_row(ui, "Turn far", &mut f.turn_far, "none");
            enter |= hint_row(ui, "Cutoff", &mut f.cutoff, "none");
            choice_row(ui, "Falloff", "fp_falloff", &mut f.falloff, &["Linear", "Smooth"]);
        }
    });
    if attract != f.attract {
        f.attract = attract;
        set_object_accepting(&mut d.inputs, &mut d.active, ATTRACTORS, ATTRACTOR_PICKS, attract);
        if let Some(i) = d.inputs.iter_mut().find(|i| i.label == ATTRACTORS) {
            i.multi = true;
        }
    }
    enter
}

/// The command's parameters.
pub(crate) fn params(app: &SolveApp, d: &Dialog) -> Result<Value, String> {
    let Kind::FacePattern(f) = &d.kind else { return Err("not a face pattern".into()) };
    let picks = |label: &str| d.inputs.iter().find(|i| i.label == label).map_or(&[][..], |i| i.items.as_slice());
    let bodies = picks("Objects").body_names();
    if bodies.is_empty() {
        return Err("select the bodies to copy".into());
    }
    let face = match picks("Face").first() {
        Some(Sel::Face { point, .. }) => json!({"face": pt(*point)}),
        _ => f.saved_face.clone().ok_or("select the face to pattern on")?,
    };
    let mut p = json!({"bodies": bodies, "face": face, "count1": f.count1.trim(), "count2": f.count2.trim()});
    let mut put = |key: &str, v: &str| {
        if !v.trim().is_empty() {
            p[key] = json!(v.trim());
        }
    };
    put("angle", &f.angle);
    put("margin", &f.margin);
    if f.layout == 1 {
        p["staggered"] = json!(true);
    }
    if f.flip {
        p["flip"] = json!(true);
    }
    if f.attract {
        let mut points: Vec<Value> = picks(ATTRACTORS).iter().map(|sel| crate::dialogs::construct_ref(app, sel)).collect::<Result<_, _>>()?;
        if points.is_empty() {
            points.clone_from(&f.saved_points);
        }
        if points.is_empty() {
            return Err("select the attractor points, or switch Attractors off".into());
        }
        let mut a = json!({"points": points});
        for (key, v) in [
            ("near", &f.near),
            ("far", &f.far),
            ("scale_near", &f.scale_near),
            ("scale_far", &f.scale_far),
            ("offset_near", &f.offset_near),
            ("offset_far", &f.offset_far),
            ("turn_near", &f.turn_near),
            ("turn_far", &f.turn_far),
            ("cutoff", &f.cutoff),
        ] {
            if !v.trim().is_empty() {
                a[key] = json!(v.trim());
            }
        }
        if f.falloff == 1 {
            a["falloff"] = json!("smooth");
        }
        p["attract"] = a;
    }
    Ok(p)
}

/// The dialog's values for a saved face pattern.
pub(crate) fn form_of(pattern: &PatternKind) -> Option<Form> {
    let PatternKind::Face { face, count1, count2, staggered, angle, margin, flip, anchor: _, attract } = pattern else { return None };
    let s = |e: &Option<solidcraft_engine::doc::Expr>| e.as_deref().map(str::to_string).unwrap_or_default();
    let mut f = Form {
        count1: count1.to_string(),
        count2: count2.to_string(),
        layout: usize::from(*staggered),
        angle: s(angle),
        margin: s(margin),
        flip: *flip,
        saved_face: serde_json::to_value(face).ok(),
        ..Form::default()
    };
    if let Some(a) = attract.as_deref() {
        let Attract { points, near, far, scale_near, scale_far, offset_near, offset_far, turn_near, turn_far, cutoff, falloff } = a;
        f.attract = true;
        f.saved_points = points.iter().filter_map(|g| serde_json::to_value(g).ok()).collect();
        f.near = s(near);
        f.far = s(far);
        f.scale_near = scale_near.as_deref().unwrap_or("1").to_string();
        f.scale_far = scale_far.as_deref().unwrap_or("1").to_string();
        f.offset_near = s(offset_near);
        f.offset_far = s(offset_far);
        f.turn_near = s(turn_near);
        f.turn_far = s(turn_far);
        f.cutoff = s(cutoff);
        f.falloff = usize::from(*falloff == Falloff::Smooth);
    }
    Some(f)
}
