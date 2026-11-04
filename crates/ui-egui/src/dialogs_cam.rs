//! The Cam dialog (`joint.cam`): a cam turning on a revolute joint drives a follower, a slider or
//! a revolute, along the cam's profile (a closed sketch profile, or edges of the cam's body). The
//! follower is a knife edge, a roller (its radius typed, or its round face picked, which is also
//! where it touches) or a flat face. Picking the profile on a part chooses the joint that part
//! turns on. Play turns the cam a full turn so the follower shows its motion before OK.

use crate::ui_kit::BackingTooltipExt as _;
use egui::RichText;
use serde_json::{Value, json};
use solidcraft_engine::Sel;
use solidcraft_engine::Session;
use solidcraft_engine::doc::joints::JointKind;

use super::{ROUND, joint_row, occurrence, pick, picked_radius, trim, turn};
use crate::SolveApp;
use crate::dialogs::{combo, field, form_row};
use crate::dialogs_assembly::Asm;
use crate::selection::{EDGES, FACES, PROFILES, SelInput, SelectionExt as _, VERTICES};
use crate::theme::Tokens;

/// The follower types, as `joint.cam` takes them, and their labels.
const FOLLOWERS: [&str; 3] = ["knife", "roller", "flat"];
const FOLLOWER_LABELS: [&str; 3] = ["Knife edge", "Roller", "Flat face"];

/// The inputs: the cam's profile, the roller's round face, the follower's tip.
const PROFILE: usize = 0;
const ROLLER: usize = 1;
const TIP: usize = 2;

#[derive(Clone, Debug)]
pub struct CamForm {
    /// The cam's joint and the follower's (ids).
    pub a: u64,
    pub b: u64,
    /// Index into `FOLLOWERS`.
    pub follower: usize,
    /// The roller's radius, used where no round face is picked.
    pub radius: String,
    /// Ride the cam from the other side.
    pub flip: bool,
    /// The body of the profile pick the cam's joint was last chosen from, so a pick chooses once.
    pub picked: Option<String>,
    pub animate: bool,
    /// Animation phase, 0…1.
    pub phase: f64,
}

/// Joints of a kind: (id, label).
fn joints_of(s: &Session, kinds: &[JointKind]) -> Vec<(u64, String)> {
    s.doc.assembly.joints.iter().filter(|j| kinds.contains(&j.kind)).map(|j| (j.id, crate::dialogs_assembly::joint_choice(&j.name, j.kind))).collect()
}

pub fn start(app: &SolveApp) -> (Asm, Vec<SelInput>) {
    let s = &app.session;
    let cams = joints_of(s, &[JointKind::Revolute]);
    let followers = joints_of(s, &[JointKind::Slider, JointKind::Revolute]);
    let a = cams.first().map(|j| j.0).unwrap_or(0);
    // A slider first, else another revolute than the cam's.
    let b = followers.iter().find(|j| j.0 != a).map(|j| j.0).unwrap_or(0);
    let f = CamForm {
        a,
        b,
        follower: 0,
        radius: crate::dialogs::length_default(&app.session, "5"),
        flip: false,
        picked: None,
        animate: false,
        phase: 0.0,
    };
    let inputs = vec![
        SelInput::new("Profile", PROFILES | EDGES, true),
        SelInput::new("Roller (optional)", ROUND, false),
        SelInput::new("Tip (optional)", FACES | EDGES | VERTICES, false),
    ];
    (Asm::Cam(Box::new(f)), inputs)
}

/// The occurrence the profile picked in `inputs` is on: an edge's body's, or a sketch's component's
/// first.
fn profile_occurrence(s: &Session, inputs: &[SelInput]) -> Option<(String, u64)> {
    match inputs.get(PROFILE)?.items.first()? {
        Sel::Edge { body, .. } => Some((body.clone(), occurrence(s, body)?)),
        Sel::Profile { sketch, .. } => {
            let comp = s.doc.feature(*sketch).map(|f| f.component)?;
            let occ = if comp == 0 { 0 } else { s.doc.occurrence_of(comp)?.id };
            Some((format!("sketch {sketch}"), occ))
        }
        _ => None,
    }
}

/// The dialog's rows; true when Enter was pressed in a value.
pub fn rows(app: &mut SolveApp, ui: &mut egui::Ui, f: &mut CamForm, inputs: &[SelInput]) -> bool {
    let t = Tokens::get();
    let s = &app.session;
    let (cams, followers) = (joints_of(s, &[JointKind::Revolute]), joints_of(s, &[JointKind::Slider, JointKind::Revolute]));
    if cams.is_empty() || followers.len() < 2 && followers.iter().all(|j| cams.iter().any(|c| c.0 == j.0)) {
        crate::ui_kit::status_line(ui, "needs a joint that turns and one that slides or swings", crate::ui_kit::StatusTone::Dim);
        return false;
    }
    // A profile picked on a part chooses the joint that part turns on, once.
    let at = profile_occurrence(s, inputs);
    if at.as_ref().map(|x| &x.0) != f.picked.as_ref() {
        if let Some(j) = at.as_ref().and_then(|(_, o)| {
            let turning = s.doc.assembly.joints.iter().filter(|j| j.kind == JointKind::Revolute);
            turning.clone().find(|j| j.b.occurrence == *o).or(turning.clone().find(|j| j.a.occurrence == *o))
        }) {
            f.a = j.id;
            if f.b == f.a {
                f.b = followers.iter().find(|x| x.0 != f.a).map(|x| x.0).unwrap_or(0);
            }
        }
        f.picked = at.map(|x| x.0);
    }
    let mut enter = false;
    joint_row(ui, "Cam", "cam_a", &cams, &mut f.a);
    joint_row(ui, "Follower", "cam_b", &followers, &mut f.b);
    form_row(ui, "Type", |ui| {
        let labels: Vec<&str> = FOLLOWER_LABELS.iter().map(|l| crate::i18n::tr_label(l)).collect();
        combo(ui, "cam_follower", &labels, &mut f.follower);
    });
    if FOLLOWERS.get(f.follower) == Some(&"roller") {
        form_row(ui, "Radius", |ui| match (pick(inputs, ROLLER), picked_radius(s, inputs, ROLLER)) {
            (_, Some(r)) => {
                ui.label(format!("{} mm  ({})", trim(r), crate::i18n::tr_label("picked")));
            }
            (Some(_), None) => {
                ui.label(RichText::new(crate::i18n::tr_label("not a round face")).color(t.warning));
            }
            (None, None) => enter |= field(ui, &mut f.radius),
        });
    }
    crate::ui_kit::options(ui, "cam_options", &mut [("Other side", &mut f.flip)]);
    form_row(ui, "Animate", |ui| {
        let label = crate::i18n::tr(if f.animate { "ui.joint_stop" } else { "ui.joint_play" });
        let r = crate::ui_kit::button(ui, label, crate::ui_kit::Button::Normal)
            .backing_on_hover_text(crate::i18n::tr_label("Play the joint's motion in the view"));
        crate::scenario::publish_handle("dialog:Animate", r.rect.center());
        if r.clicked() {
            f.animate = !f.animate;
        }
    });
    if f.animate {
        let now = ui.input(|i| i.time);
        // 72 steps over 6 s: each one a preview.
        f.phase = ((now / 6.0).fract() * 72.0).floor() / 72.0;
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(30));
    }
    enter
}

/// The command OK runs.
pub fn commands(app: &SolveApp, f: &CamForm, inputs: &[SelInput]) -> Result<Vec<(String, Value)>, String> {
    let s = &app.session;
    let joint = |id: u64| {
        s.doc.assembly.joints.iter().find(|j| j.id == id).map(|j| j.id).ok_or_else(|| "choose the cam's and the follower's joints".to_string())
    };
    let (a, b) = (joint(f.a)?, joint(f.b)?);
    let items = inputs.get(PROFILE).map(|i| i.items.as_slice()).unwrap_or_default();
    let profile = match items.first() {
        Some(Sel::Profile { sketch, index }) => json!({"sketch": sketch, "index": index}),
        Some(Sel::Edge { body, .. }) => {
            let pts: Vec<Value> = items.edge_points().into_iter().map(crate::selection::point_value).collect();
            let mut p = json!({ "edges": pts });
            if let Some(o) = occurrence(s, body) {
                p["occurrence"] = json!(o);
            }
            p
        }
        _ => return Err("select the cam's profile (a sketch profile or the cam's edges)".into()),
    };
    let follower = FOLLOWERS.get(f.follower).copied().unwrap_or("knife");
    let mut p = json!({"a": a, "b": b, "profile": profile, "follower": follower, "flip": f.flip});
    if follower == "roller" {
        p["radius"] = match pick(inputs, ROLLER) {
            Some((body, at)) => {
                let mut v = json!({"circle": [at.x, at.y, at.z]});
                if let Some(o) = occurrence(s, body) {
                    v["occurrence"] = json!(o);
                }
                v
            }
            None => json!(f.radius.trim()),
        };
    }
    if let Some(snap) = inputs.get(TIP).and_then(|i| i.items.first()).and_then(|x| crate::dialogs_assembly::snap_of(s, x)) {
        p["tip"] = snap.param;
    }
    Ok(vec![("joint.cam".to_string(), p)])
}

/// While animating, the preview turns the cam a full turn.
pub fn animate(s: &Session, f: &CamForm, cmds: &mut Vec<(String, Value)>) {
    if f.animate {
        turn(s, f.a, f.phase, cmds);
    }
}

#[cfg(test)]
#[path = "dialogs_cam_tests.rs"]
mod tests;
