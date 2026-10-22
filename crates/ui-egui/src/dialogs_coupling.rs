//! The Gear, Rack and Pinion and Screw dialogs: two joints coupled by radii or a pitch
//! (`joint.gear`, `joint.rack_pinion`, `joint.screw`); the Cam dialog is `dialogs_cam.rs`. A radius is typed, or comes from the round
//! face picked on the part, and then follows that face when the design changes. Picking a gear's
//! face also picks the joint it turns on. Play turns the driving joint so the coupling shows its
//! motion before OK.

use crate::ui_kit::{choice_row, expr_row};
use egui::RichText;
use serde_json::{Value, json};
use solidcraft_engine::Sel;
use solidcraft_engine::Session;
use solidcraft_engine::doc::expr::Kind as ValueKind;

use crate::SolveApp;
use crate::dialogs::{field, form_row};
use crate::dialogs_assembly::Asm;
use crate::selection::{EDGES, FACES, SelInput};
use crate::theme::Tokens;

/// What a round-face input takes: a face, or an edge of one.
const ROUND: u16 = FACES | EDGES;

#[derive(Clone, Debug)]
pub struct CouplingForm {
    /// `joint.gear`, `joint.rack_pinion` or `joint.screw`.
    pub command: &'static str,
    /// The driving joint and the driven one (ids; the same cylindrical joint for a screw on one).
    pub a: u64,
    pub b: u64,
    /// Typed sizes, used where no round face is picked: gear 1's and gear 2's radii, the pinion's
    /// radius, or the screw's pitch.
    pub sizes: [String; 2],
    pub internal: bool,
    pub reverse: bool,
    /// The bodies of the picks the joints were last chosen from, so a pick chooses once.
    pub picked: [Option<String>; 2],
    pub animate: bool,
    /// Animation phase, 0…1.
    pub phase: f64,
}

/// Joints with a value that turns (`turn`) or slides: (id, label).
fn joints_that(s: &Session, turn: bool) -> Vec<(u64, String)> {
    s.doc
        .assembly
        .joints
        .iter()
        .filter(|j| (0..j.kind.dofs().len()).any(|i| j.kind.is_angle(i) == turn))
        .map(|j| (j.id, format!("{} ({:?})", j.name, j.kind)))
        .collect()
}

/// The joint roles and whether the driven one turns (a gear) or slides.
fn roles(command: &str) -> ([&'static str; 2], bool) {
    match command {
        "joint.gear" => (["Gear 1", "Gear 2"], true),
        "joint.rack_pinion" => (["Pinion", "Rack"], false),
        _ => (["Screw", "Nut"], false),
    }
}

pub fn start(app: &SolveApp, id: &str) -> Option<(Asm, Vec<SelInput>)> {
    let s = &app.session;
    let (_, turn_b) = roles(id);
    let (turns, b_list) = (joints_that(s, true), joints_that(s, turn_b));
    let first = |l: &[(u64, String)]| l.first().map(|j| j.0).unwrap_or(0);
    let (command, a, b, sizes, inputs) = match id {
        "joint.gear" => (
            "joint.gear",
            first(&turns),
            turns.get(1).map(|j| j.0).unwrap_or(0),
            [crate::dialogs::length_default(&app.session, "10"), crate::dialogs::length_default(&app.session, "20")],
            vec![SelInput::new("Gear 1 (optional)", ROUND, false), SelInput::new("Gear 2 (optional)", ROUND, false)],
        ),
        "joint.rack_pinion" => (
            "joint.rack_pinion",
            first(&turns),
            first(&b_list),
            [crate::dialogs::length_default(&app.session, "10"), String::new()],
            vec![SelInput::new("Pinion (optional)", ROUND, false)],
        ),
        "joint.screw" => {
            // A joint that turns and slides is a screw on its own.
            let both = turns.iter().find(|t| b_list.iter().any(|l| l.0 == t.0)).map(|t| t.0);
            let a = both.unwrap_or(first(&turns));
            ("joint.screw", a, both.unwrap_or(first(&b_list)), [crate::dialogs::length_default(&app.session, "1.5"), String::new()], vec![])
        }
        _ => return None,
    };
    let f = CouplingForm { command, a, b, sizes, internal: false, reverse: false, picked: [None, None], animate: false, phase: 0.0 };
    Some((Asm::Coupling(Box::new(f)), inputs))
}

/// The occurrence placing a picked body (0: the root), by the body's placement: each instance of
/// a copied gear is an occurrence of its own.
fn occurrence(s: &Session, body: &str) -> Option<u64> {
    s.doc.body_frames(&s.model.state()).into_iter().find(|f| f.0 == body).map(|(_, _, path, _)| path.last().copied().unwrap_or(0))
}

fn pick(inputs: &[SelInput], i: usize) -> Option<(&str, solidcraft_engine::geom::Vec3)> {
    match inputs.get(i)?.items.first()? {
        Sel::Face { body, point, .. } | Sel::Edge { body, point, .. } => Some((body.as_str(), *point)),
        _ => None,
    }
}

/// The radius of the round face picked in input `i`.
fn picked_radius(s: &Session, inputs: &[SelInput], i: usize) -> Option<f64> {
    let (body, point) = pick(inputs, i)?;
    let st = crate::open_meshing::session_world(s);
    solidcraft_engine::kernel::cylinder_face_at(&st.body(body)?.body, point).map(|c| c.radius)
}

/// Size `i` in mm: the picked face's radius, or the typed one.
fn size(s: &Session, f: &CouplingForm, inputs: &[SelInput], i: usize) -> Option<f64> {
    picked_radius(s, inputs, i)
        .or_else(|| crate::length_input::eval(&s.doc, f.sizes.get(i)?, ValueKind::Length).ok())
        .filter(|v| v.is_finite() && *v > 0.0)
}

/// A joint row: the joints `list` offers, `sel` the chosen id.
fn joint_row(ui: &mut egui::Ui, label: &str, salt: &str, list: &[(u64, String)], sel: &mut u64) {
    let labels: Vec<&str> = list.iter().map(|j| j.1.as_str()).collect();
    let mut pos = list.iter().position(|j| j.0 == *sel).unwrap_or(0);
    choice_row(ui, label, salt, &mut pos, &labels);
    *sel = list.get(pos).map(|j| j.0).unwrap_or(0);
}

/// The dialog's rows; true when Enter was pressed in a value.
pub fn rows(app: &mut SolveApp, ui: &mut egui::Ui, f: &mut CouplingForm, inputs: &[SelInput]) -> bool {
    let t = Tokens::get();
    let s = &app.session;
    let ([role_a, role_b], turn_b) = roles(f.command);
    let (la, lb) = (joints_that(s, true), joints_that(s, turn_b));
    if la.is_empty() || lb.is_empty() {
        form_row(ui, "", |ui| {
            let need = if turn_b { "needs two joints that turn" } else { "needs a joint that turns and one that slides" };
            ui.label(RichText::new(crate::i18n::tr_label(need)).color(t.text_dim));
        });
        return false;
    }
    // A face picked on a part chooses the joint that part turns on, once.
    for (i, slot) in [&mut f.a, &mut f.b].into_iter().enumerate() {
        let body = pick(inputs, i).map(|(b, _)| b.to_string());
        if body.is_some() && body != f.picked.get(i).cloned().flatten() {
            let turning = s.doc.assembly.joints.iter().filter(|j| la.iter().any(|l| l.0 == j.id));
            if let Some(j) = body
                .as_deref()
                .and_then(|b| occurrence(s, b))
                .and_then(|o| turning.clone().find(|j| j.b.occurrence == o).or(turning.clone().find(|j| j.a.occurrence == o)))
            {
                *slot = j.id;
            }
        }
        if let Some(p) = f.picked.get_mut(i) {
            *p = body;
        }
    }
    let mut enter = false;
    let size_row = |ui: &mut egui::Ui, label: &str, i: usize, text: &mut String, enter: &mut bool| {
        form_row(ui, label, |ui| {
            match (pick(inputs, i), picked_radius(s, inputs, i)) {
                (_, Some(r)) => {
                    ui.label(format!("{} mm  ({})", trim(r), crate::i18n::tr_label("picked")));
                }
                // A pick on a flat face or a straight edge has no radius: say so rather than fail on OK.
                (Some(_), None) => {
                    ui.label(RichText::new(crate::i18n::tr_label("not a round face")).color(t.warning));
                }
                (None, None) => *enter |= field(ui, text),
            }
        });
    };
    joint_row(ui, role_a, "cp_a", &la, &mut f.a);
    let [s0, s1] = &mut f.sizes;
    match f.command {
        "joint.gear" => {
            size_row(ui, "Radius 1", 0, s0, &mut enter);
            joint_row(ui, role_b, "cp_b", &lb, &mut f.b);
            size_row(ui, "Radius 2", 1, s1, &mut enter);
        }
        "joint.rack_pinion" => {
            size_row(ui, "Radius", 0, s0, &mut enter);
            joint_row(ui, role_b, "cp_b", &lb, &mut f.b);
        }
        _ => {
            joint_row(ui, role_b, "cp_b", &lb, &mut f.b);
            enter |= expr_row(ui, "Pitch", s0);
        }
    }
    if f.command == "joint.gear" {
        crate::ui_kit::options(ui, "coupling_options", &mut [("Internal gear", &mut f.internal), ("Reverse", &mut f.reverse)]);
    } else {
        crate::ui_kit::options(ui, "coupling_options", &mut [("Reverse", &mut f.reverse)]);
    }
    // What the coupling does, as joint.list gives a link's ratio.
    let sign = |neg: bool| if neg { -1.0 } else { 1.0 };
    let shown = match f.command {
        "joint.gear" => size(s, f, inputs, 0)
            .zip(size(s, f, inputs, 1))
            .map(|(ra, rb)| format!("{} deg / deg", trim(ra / rb * sign(!f.internal) * sign(f.reverse)))),
        "joint.rack_pinion" => size(s, f, inputs, 0).map(|r| format!("{} mm / deg", trim(r.to_radians() * sign(f.reverse)))),
        _ => size(s, f, inputs, 0).map(|p| format!("{} mm / deg", trim(p / 360.0 * sign(f.reverse)))),
    };
    form_row(ui, "Ratio", |ui| {
        ui.label(RichText::new(shown.unwrap_or_else(|| "-".into())).color(t.text_dim));
    });
    form_row(ui, "Animate", |ui| {
        let label = if f.animate { "■ Stop" } else { "▶ Play" };
        let r = crate::ui_kit::button(ui, label, crate::ui_kit::Button::Normal)
            .on_hover_text(crate::i18n::tr_label("Play the joint's motion in the view"));
        crate::scenario::publish_handle("dialog:Animate", r.rect.center());
        if r.clicked() {
            f.animate = !f.animate;
        }
    });
    if f.animate {
        let now = ui.input(|i| i.time);
        // 48 steps over 4 s: each one a preview.
        f.phase = ((now / 4.0).fract() * 48.0).floor() / 48.0;
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(30));
    }
    enter
}

/// A number without trailing zeros.
pub(crate) fn trim(v: f64) -> String {
    let s = format!("{v:.4}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" { "0".into() } else { s.into() }
}

/// The command OK runs.
pub fn commands(app: &SolveApp, f: &CouplingForm, inputs: &[SelInput]) -> Result<Vec<(String, Value)>, String> {
    let s = &app.session;
    let joint = |id: u64| s.doc.assembly.joints.iter().find(|j| j.id == id).map(|j| j.id).ok_or_else(|| "choose the joints to couple".to_string());
    let (a, b) = (joint(f.a)?, joint(f.b)?);
    // A picked face, or the typed size.
    let size = |i: usize| -> Value {
        match pick(inputs, i) {
            Some((body, p)) => {
                let mut v = json!({"circle": [p.x, p.y, p.z]});
                if let Some(o) = occurrence(s, body) {
                    v["occurrence"] = json!(o);
                }
                v
            }
            None => json!(f.sizes.get(i).map(|t| t.trim()).unwrap_or("")),
        }
    };
    let mut p = match f.command {
        "joint.gear" => json!({"a": a, "b": b, "ra": size(0), "rb": size(1), "internal": f.internal}),
        "joint.rack_pinion" => json!({"a": a, "b": b, "radius": size(0)}),
        _ if a == b => json!({"joint": a, "pitch": f.sizes[0].trim()}),
        _ => json!({"a": a, "b": b, "pitch": f.sizes[0].trim()}),
    };
    p["reverse"] = json!(f.reverse);
    Ok(vec![(f.command.to_string(), p)])
}

/// While animating, the preview turns the driving joint a full turn.
pub fn animate(s: &Session, f: &CouplingForm, cmds: &mut Vec<(String, Value)>) {
    if f.animate {
        turn(s, f.a, f.phase, cmds);
    }
}

/// Drive `joint` to `phase` of a full turn: its first value that turns (a planar joint's is its
/// third), the values before it as they are.
fn turn(s: &Session, joint: u64, phase: f64, cmds: &mut Vec<(String, Value)>) {
    let Some(j) = s.doc.assembly.joints.iter().find(|j| j.id == joint) else { return };
    let Some(turn) = (0..j.kind.dofs().len()).find(|i| j.kind.is_angle(*i)) else { return };
    let values: Vec<String> = (0..=turn)
        .map(|i| match j.values.get(i).copied().unwrap_or(0.0) {
            _ if i == turn => format!("{} deg", phase * 360.0),
            v if j.kind.is_angle(i) => format!("{} deg", v.to_degrees()),
            v => format!("{v} mm"),
        })
        .collect();
    cmds.push(("joint.drive".into(), json!({"joint": joint, "values": values})));
}

#[path = "dialogs_cam.rs"]
pub mod cam;

#[cfg(test)]
#[path = "dialogs_coupling_tests.rs"]
mod tests;
