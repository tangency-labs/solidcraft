//! Rotate Member (`frame.rotate_member`): one frame member turned about its own line. The Angle
//! starts at the picked member's current turn; picking Parallel To works it out from the line with
//! a dry run of the command, and it stays editable. The view's dial drags it (`canvas`).
use crate::{
    SolveApp,
    dialogs::{Kind, shown_expr},
    selection::{AXES, CURVES, EDGES, PLANAR_FACES, SelInput, items},
    ui_kit::expr_row,
};
use serde_json::{Map, Value, json};
use solidcore_record::Vec3;
use solidcraft_engine::{
    Sel, Session,
    doc::{
        FeatureKind,
        end_cap::{Member, member_axis},
        expr::Kind as ValueKind,
        frame::{member_line_ids, member_turns},
    },
    geom::prelude::*,
};

const COMMAND: &str = "frame.rotate_member";

#[derive(Clone, Debug)]
pub struct RotateMember {
    pub angle: String,
    /// The member the Angle was filled in for: picking another shows that one's current turn.
    pub member: Option<Member>,
    /// The last Parallel To dry run: the command's face and line, and the Angle it gave (None:
    /// refused, and OK sends the line so the command says why).
    pub parallel: Option<(Value, Option<String>)>,
    /// Editing: the step's id, which the dry run and the member's side leave out as
    /// `timeline.redefine` does.
    pub step: Option<u64>,
}

impl Default for RotateMember {
    fn default() -> Self {
        Self { angle: "0 deg".into(), member: None, parallel: None, step: None }
    }
}

pub fn start(_: &SolveApp, id: &str) -> Option<(Kind, Vec<SelInput>)> {
    (id == COMMAND).then(|| {
        let inputs = vec![
            SelInput::new("ui.rotate_member.member", PLANAR_FACES, false),
            // What Plane Through Axis and Point takes as its axis.
            SelInput::new("ui.rotate_member.parallel_to", EDGES | AXES | CURVES, false),
        ];
        (Kind::RotateMember(RotateMember::default()), inputs)
    })
}

pub fn rows(app: &SolveApp, ui: &mut egui::Ui, c: &mut RotateMember, inputs: &[SelInput]) -> bool {
    sync(app, c, inputs);
    expr_row(ui, app.tr("ui.rotate_member.angle"), &mut c.angle)
}

/// Brings the Angle up to date with the picks: a newly picked member's current turn, and a newly
/// picked Parallel To line's angle.
pub fn sync(app: &SolveApp, c: &mut RotateMember, inputs: &[SelInput]) {
    let member = picked_body(inputs).and_then(|body| member_of(app, body));
    if member != c.member {
        if let Some(m) = &member {
            c.angle = current(app, m).map_or_else(|| "0 deg".into(), |(e, _)| e);
        }
        c.member = member;
        c.parallel = None;
    }
    if let Some(key) = parallel_args(app, inputs)
        && c.parallel.as_ref().is_none_or(|(k, _)| *k != key)
    {
        let angle = worked_out(app, c.step, &key);
        if let Some(a) = &angle {
            c.angle = a.clone();
        }
        c.parallel = Some((key, angle));
    }
}

pub fn commands(
    app: &SolveApp,
    c: &RotateMember,
    inputs: &[SelInput],
    extra: &Map<String, Value>,
    editing: bool,
) -> Result<Vec<(String, Value)>, String> {
    let face = face_arg(inputs).ok_or_else(|| app.tr("Select a face of a frame member").to_string())?;
    let mut args = json!({"face": face, "angle": c.angle});
    // The line goes with the angle it gave; an Angle edited since wins. Not worked out yet (or
    // refused): the command works it out, or says why not.
    let mut worked = true;
    if let Some(key) = parallel_args(app, inputs) {
        let given = c.parallel.as_ref().filter(|(k, _)| *k == key).and_then(|(_, a)| a.as_ref());
        worked = given.is_some();
        if given.is_none_or(|a| *a == c.angle) {
            args["parallel_to"] = key["parallel_to"].clone();
        }
    }
    // A new step to the member's current turn changes nothing: OK just closes. An edit always
    // runs, so an edit back to the current turn sticks.
    if !editing && worked && unchanged(app, c, inputs) {
        return Ok(Vec::new());
    }
    if let Value::Object(args) = &mut args {
        for (k, v) in extra {
            args.entry(k.clone()).or_insert_with(|| v.clone());
        }
    }
    Ok(vec![(COMMAND.into(), args)])
}

pub fn for_feature(app: &SolveApp, kind: &FeatureKind) -> Option<(Kind, Vec<SelInput>, Map<String, Value>)> {
    let FeatureKind::FrameMemberTurn { member, angle } = kind else { return None };
    let (_, mut inputs) = start(app, COMMAND)?;
    let step = app.session.doc.features.iter().find(|f| f.kind == *kind).map(|f| f.id);
    if let Some(input) = inputs.first_mut() {
        input.items = side_pick(app, step, member).into_iter().collect();
    }
    let c = RotateMember { angle: shown_expr(angle, ValueKind::Angle), member: Some(member.clone()), parallel: None, step };
    Some((Kind::RotateMember(c), inputs, Map::new()))
}

/// The dial: the picked member's mid-point and its line's direction, start to end.
pub(crate) fn dial(app: &SolveApp, inputs: &[SelInput]) -> Option<(Vec3, Vec3)> {
    let body = picked_body(inputs)?;
    let member = member_of(app, body)?;
    let axis = member_axis(&app.session.doc, &app.session.model.state(), &member).ok()?;
    // ponytail: the body's box centre, on the line for profiles symmetric about their centre.
    let centre = crate::open_meshing::session_world(&app.session).body(body)?.mesh().bounds().center();
    Some((centre, axis))
}

fn picked_body(inputs: &[SelInput]) -> Option<&str> {
    match items(inputs, 0).first() {
        Some(Sel::Face { body, .. }) => Some(body),
        _ => None,
    }
}

fn face_arg(inputs: &[SelInput]) -> Option<Value> {
    match items(inputs, 0).first() {
        Some(Sel::Face { body, index, point }) => Some(json!({"face": point, "body": body, "index": index})),
        _ => None,
    }
}

/// The dry run's arguments: the picked face and Parallel To line.
fn parallel_args(app: &SolveApp, inputs: &[SelInput]) -> Option<Value> {
    let line = crate::dialogs::construct_ref(app, items(inputs, 1).first()?).ok()?;
    Some(json!({"face": face_arg(inputs)?, "parallel_to": line}))
}

/// The angle `frame.rotate_member` works out for `args`, run on a copy of the design.
fn worked_out(app: &SolveApp, step: Option<u64>, args: &Value) -> Option<String> {
    let mut s = design(app, step);
    let id = s.execute(COMMAND, args).ok()?.get("feature")?.as_u64()?;
    let FeatureKind::FrameMemberTurn { angle, .. } = &s.doc.feature(id)?.kind else { return None };
    Some(shown_expr(angle, ValueKind::Angle))
}

/// Whether the Angle is the picked member's current turn.
fn unchanged(app: &SolveApp, c: &RotateMember, inputs: &[SelInput]) -> bool {
    let Some(member) = picked_body(inputs).and_then(|body| member_of(app, body)) else { return false };
    let now = current(app, &member).map_or(0.0, |(_, radians)| radians);
    crate::length_input::eval(&app.session.doc, &c.angle, ValueKind::Angle).is_ok_and(|a| (a - now).abs() < 1e-9)
}

/// The member's current turn, as the Angle shows it and in radians; None: never turned.
fn current(app: &SolveApp, m: &Member) -> Option<(String, f64)> {
    let doc = &app.session.doc;
    let (e, radians) = member_turns(doc, &doc.evaluation_values(), m.feature).remove(&m.line)?;
    Some((shown_expr(&e, ValueKind::Angle), radians))
}

/// The member a body is: the Frame Member feature that made it, and its line. Its bodies are made
/// one per line, in line order.
fn member_of(app: &SolveApp, body: &str) -> Option<Member> {
    let feature = app.session.model.state().body(body)?.feature;
    let kind = &app.session.doc.feature(feature)?.kind;
    let made = &app.session.model.result(feature)?.state;
    let lines = member_line_ids(made, kind).ok()?;
    let i = made.bodies.iter().filter(|b| b.feature == feature).position(|b| b.name == body)?;
    Some(Member { feature: feature.into(), line: lines.get(i)?.clone() })
}

/// The design the command runs in: the session's or, for an edited step, the design as
/// `timeline.redefine` runs it, with the step turned off and the marker just past it.
fn design(app: &SolveApp, step: Option<u64>) -> Session {
    let mut s = app.session.scratch();
    if let Some(id) = step
        && let Some(at) = s.doc.feature_index(id)
    {
        if let Some(f) = s.doc_mut().feature_mut(id) {
            f.suppressed = true;
        }
        s.doc_mut().marker = Some(at.saturating_add(1));
        s.refresh();
    }
    s
}

/// A side of the member's body, picked as the view would (a planar face along its line), in the
/// design the edited `step` runs in.
fn side_pick(app: &SolveApp, step: Option<u64>, member: &Member) -> Option<Sel> {
    let s = design(app, step);
    let made = &s.model.result(member.feature.0)?.state;
    let lines = member_line_ids(made, &s.doc.feature(member.feature.0)?.kind).ok()?;
    let i = lines.iter().position(|l| *l == member.line)?;
    let name = made.bodies.iter().filter(|b| b.feature == member.feature.0).nth(i)?.name.clone();
    let state = s.model.state();
    let body = state.body(&name)?;
    let axis = member_axis(&s.doc, &state, member).ok()?;
    let mesh = body.mesh();
    mesh.triangles.iter().zip(&mesh.tri_face).find_map(|(tri, index)| {
        let side = body.body.planar_face(*index as usize).is_some_and(|pl| pl.normal().dot(axis).abs() < 1e-6);
        let [a, b, c] = mesh.tri(tri)?;
        side.then(|| Sel::Face { body: name.clone(), index: *index as usize, point: (a + b + c) / 3. })
    })
}
