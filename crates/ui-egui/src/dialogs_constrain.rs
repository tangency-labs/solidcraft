//! The Constrain dialog (`assembly.constraint.create`, `assembly.constraint.edit`): a reference is
//! picked on each part first (a planar face, a round edge or face, a point), then the rules those
//! fit are offered (two faces: mate, flush, distance, parallel…; two round ones: concentric,
//! insert…). Apply adds the rule and keeps the dialog on the same two parts, for the next rule
//! between them. The rules already between the two parts are listed: each can be loaded to change
//! it (through `assembly.constraint.edit`), suppressed or deleted. Center adds a face pair
//! in place of a single reference. A line says how free the moving part is with the previewed rule.

use serde_json::{Value, json};
use solidcraft_engine::Sel;
use solidcraft_engine::doc::Document;
use solidcraft_engine::doc::joints::{self, Constraint, RefShape};

use crate::SolveApp;
use crate::dialogs::{Dialog, Kind};
use crate::dialogs_assembly::{Asm, Hover, SNAPS, SnapKind, SnapPt, between_face_picks, picked_snap, side_pick, snap_in};
use crate::selection::{PLANAR_FACES, SelInput, same_item};
use crate::ui_kit::{StatusTone, choice_row, expr_row, status_line};

#[derive(Clone, Debug, Default)]
pub struct ConstrainForm {
    /// The rule chosen, among those that fit the picks.
    pub rule: Option<Constraint>,
    /// The Center mode: the first two references are the walls of one part, the third the
    /// part to center between them (`assembly.constraint.create`'s center rule).
    pub center: bool,
    /// Pick the moving reference from two faces, rather than a plane or axis.
    center_pair: bool,
    /// A mate's or flush's gap, a distance (a length or an expression).
    pub offset: String,
    /// An angle rule's angle.
    pub angle: String,
    /// Turn the moving part over.
    pub flip: bool,
    /// Why the last Apply, Suppress, Delete or Edit was refused.
    pub error: Option<String>,
    /// The two parts (a's occurrence, b's) the last rule was applied to: their rules stay listed
    /// while the next references are picked.
    pub pair: Option<(u64, u64)>,
    /// The constraint being changed (loaded from the list), and the picks it was loaded with: picking
    /// something else makes a new rule instead.
    pub editing: Option<(u64, Vec<Option<Sel>>)>,
    /// The snap under the cursor, as the Joint dialog keeps it for its glyphs.
    pub hover: Hover,
    /// How free the moving part is in the saved design, worked out once per design (the line shown
    /// while nothing is previewed): the design it is for, the occurrence, the text, a warning.
    pub saved: Option<(std::sync::Weak<Document>, u64, String, bool)>,
}

fn start_form() -> ConstrainForm {
    ConstrainForm { offset: "0 mm".into(), angle: "90 deg".into(), ..Default::default() }
}

pub fn start(_app: &SolveApp) -> Option<(Asm, Vec<SelInput>)> {
    Some((Asm::Constrain(Box::new(start_form())), vec![SelInput::new("Reference 1", SNAPS, false), SelInput::new("Reference 2", SNAPS, false)]))
}

/// The shape of a pick, as the engine sees the joint origin it becomes: a face centre or a datum
/// plane is a plane, a round edge or face its axis, a datum axis an axis (it is drawn as an edge's
/// middle, and its origin names the `axis`), any other edge (its middle, or a body's corner
/// between two edges) an edge, and a sketch or construction point a point.
fn shape(s: &SnapPt) -> RefShape {
    match s.kind {
        SnapKind::FaceCenter => RefShape::Plane,
        SnapKind::CircleCenter => RefShape::Round,
        SnapKind::EdgeMid | SnapKind::Vertex if s.param.get("axis").is_some() => RefShape::Axis,
        SnapKind::EdgeMid | SnapKind::Vertex if s.param.get("edge").is_some() => RefShape::Edge,
        SnapKind::EdgeMid | SnapKind::Vertex => RefShape::Point,
    }
}

/// A rule's name in the user's language.
pub(crate) fn label(c: Constraint) -> &'static str {
    solidcraft_ui_text::tr(match c {
        Constraint::Mate => "constrain.rule_mate",
        Constraint::Flush => "constrain.rule_flush",
        Constraint::Distance => "constrain.rule_distance",
        Constraint::Concentric => "constrain.rule_concentric",
        Constraint::Insert => "constrain.rule_insert",
        Constraint::Parallel => "constrain.rule_parallel",
        Constraint::Perpendicular => "constrain.rule_perpendicular",
        Constraint::Angle => "constrain.rule_angle",
        Constraint::Tangent => "constrain.rule_tangent",
        Constraint::PointOnAxis => "constrain.rule_point_on_axis",
        Constraint::Center => "constrain.rule_center",
    })
}

/// A rule's icon, where a constraint is listed (the browser's Joints folders).
pub(crate) fn rule_icon(c: Constraint) -> &'static str {
    match c {
        Constraint::Mate => "constraint_mate",
        Constraint::Flush => "constraint_flush",
        Constraint::Distance => "constraint_distance",
        Constraint::Concentric => "constraint_concentric",
        Constraint::Insert => "constraint_insert",
        Constraint::Parallel => "constraint_parallel",
        Constraint::Perpendicular => "constraint_perpendicular",
        Constraint::Angle => "constraint_angle",
        Constraint::Tangent => "constraint_tangent",
        Constraint::PointOnAxis => "constraint_point_on_axis",
        Constraint::Center => "plane_midplane",
    }
}

/// Constrain, open on constraint `id` to change it (a double-click on its row, its menu's Edit):
/// its references picked, its rule, gap and angle loaded, as the dialog's own list loads it.
/// `None` for a joint made as a joint.
pub fn edit_constraint(app: &SolveApp, id: u64) -> Option<Dialog> {
    app.session.doc.assembly.joints.iter().find(|j| j.id == id)?.constraint?;
    let (Asm::Constrain(mut f), mut inputs) = start(app)? else { return None };
    let mut active = 0;
    load(app, &mut f, &mut inputs, &mut active, id);
    let mut d = Dialog::new(Kind::Assembly(Asm::Constrain(f)), inputs);
    d.active = active;
    Some(d)
}

/// The two picks and their snaps.
fn picks(app: &SolveApp, inputs: &[SelInput]) -> Option<[(Sel, SnapPt); 2]> {
    let pick = |i: usize| -> Option<(Sel, SnapPt)> {
        let sel = inputs.get(i)?.items.first()?.clone();
        let snap = picked_snap(app, &sel)?;
        Some((sel, snap))
    };
    Some([pick(0)?, pick(1)?])
}

/// The two references as the rules see them: their shapes, the parts they are on, and the origins a
/// new rule is made with (`None` for a loaded constraint, which keeps its own).
struct Refs {
    shapes: [RefShape; 2],
    parts: [u64; 2],
    params: Option<[Value; 2]>,
}

/// The references: the picks, or while a loaded constraint's picks are unchanged, its own saved
/// origins (as the Joint dialog edits a joint: a point side shows its part, which no snap can be
/// read back from, and keeps its origin).
fn refs(app: &SolveApp, f: &ConstrainForm, inputs: &[SelInput]) -> Option<Refs> {
    if let Some(id) = editing(f, inputs) {
        let j = app.session.doc.assembly.joints.iter().find(|j| j.id == id)?;
        return Some(Refs { shapes: [RefShape::of(&j.a.snap), RefShape::of(&j.b.snap)], parts: [j.a.part(), j.b.part()], params: None });
    }
    let [(_, a), (_, b)] = picks(app, inputs)?;
    Some(Refs { shapes: [shape(&a), shape(&b)], parts: [part_of(&a), part_of(&b)], params: Some([a.param, b.param]) })
}

/// The part a snap is on: the placement it names (a datum plane's or axis's as well as a face's;
/// a part inside a copy of a sub-assembly is that copy's), 0 for the root design.
fn part_of(s: &SnapPt) -> u64 {
    s.param.get("occurrence").and_then(Value::as_u64).unwrap_or(0)
}

/// The two parts of `r` with rule `rule`: (the one held, the one that moves). The engine moves b,
/// which is the second reference unless the rule wants the plane or axis first (`Fit::swap`).
fn held_and_moving(r: &Refs, rule: Option<Constraint>) -> (u64, u64) {
    let swap = rule.and_then(|c| c.fit(r.shapes[0], r.shapes[1])).is_some_and(|fit| fit.swap);
    if swap { (r.parts[1], r.parts[0]) } else { (r.parts[0], r.parts[1]) }
}

/// The rules already between two parts, either way round: (id, name, rule, suppressed). A side's part
/// is the one its geometry is on (`JointOrigin::part`), as a pick names it.
fn between(doc: &Document, (a, b): (u64, u64)) -> Vec<(u64, String, Constraint, bool)> {
    doc.assembly
        .joints
        .iter()
        .filter(|j| (j.a.part(), j.b.part()) == (a, b) || (j.a.part(), j.b.part()) == (b, a))
        .filter_map(|j| Some((j.id, j.name.clone(), j.constraint?, j.suppressed)))
        .collect()
}

/// How free occurrence `occ` is by a solve's `dof` and `conflicts` (a joint command's reply), in the
/// sketch palette's words; true when it is a warning.
fn freedom(doc: &Document, reply: &Value, occ: u64) -> (String, bool) {
    let part = doc.occurrences.iter().find(|o| o.id == occ).map(|o| o.name.clone()).unwrap_or_default();
    let mine = |id: u64| doc.assembly.joints.iter().any(|j| j.id == id && (j.a.occurrence == occ || j.b.occurrence == occ));
    let conflict = reply["conflicts"].as_array().into_iter().flatten().find(|c| c["joint"].as_u64().is_some_and(mine));
    if let Some(c) = conflict {
        let other = c["name"].as_str().unwrap_or_default();
        return (solidcraft_ui_text::format("constrain.conflicts", &[("part", &part), ("other", other)]), true);
    }
    let dof = reply["dof"].as_array().into_iter().flatten().find(|d| d["occurrence"].as_u64() == Some(occ)).and_then(|d| d["dof"].as_u64());
    let text = match dof.unwrap_or(6) {
        0 => solidcraft_ui_text::format("constrain.fixed", &[("part", &part)]),
        1 => solidcraft_ui_text::format("constrain.dof_one", &[("part", &part)]),
        n => solidcraft_ui_text::format("constrain.dof", &[("part", &part), ("count", &n.to_string())]),
    };
    (text, false)
}

/// The saved design's line for `occ`, solved once per design (not every frame).
fn saved_freedom(app: &SolveApp, f: &mut ConstrainForm, occ: u64) -> (String, bool) {
    let s = &app.session;
    if let Some((doc, at, text, warn)) = &f.saved
        && *at == occ
        && doc.upgrade().is_some_and(|d| std::sync::Arc::ptr_eq(&d, &s.doc))
    {
        return (text.clone(), *warn);
    }
    let sol = joints::solve_holding_with_analysis(&s.doc, &s.model.state(), &joints::Held::new(), joints::Analysis::DegreesOfFreedom);
    let conflicts: Vec<Value> = sol.conflicts.iter().map(|(id, name, _)| json!({"joint": id, "name": name})).collect();
    let dof: Vec<Value> = sol.dof.iter().map(|(o, d)| json!({"occurrence": o, "dof": d})).collect();
    let (text, warn) = freedom(&s.doc, &json!({"dof": dof, "conflicts": conflicts}), occ);
    f.saved = Some((std::sync::Arc::downgrade(&s.doc), occ, text.clone(), warn));
    (text, warn)
}

const WALL_LABELS: [&str; 2] = ["constrain.center_wall_1", "constrain.center_wall_2"];
const PART_FACE_1: &str = "constrain.center_part_face_1";
const PART_FACE_2: &str = "constrain.center_part_face_2";
const PART: &str = "constrain.center_part";

/// Center uses two planar wall inputs and either one joint snap or two planar part inputs.
fn sync_inputs(app: &SolveApp, f: &ConstrainForm, inputs: &mut Vec<SelInput>, active: &mut usize) {
    let planar = |sel: &Sel| match sel {
        Sel::Face { body, index, .. } => crate::dialogs::planar_face(&app.session, body, *index).is_some(),
        _ => false,
    };
    let refs = if f.center { WALL_LABELS } else { ["Reference 1", "Reference 2"] };
    for (input, label) in inputs.iter_mut().zip(refs) {
        input.label = label;
        input.accept = if f.center { PLANAR_FACES } else { SNAPS };
        if f.center {
            input.items.retain(planar);
            input.from_face.clear();
        }
    }
    // Adding an input normally focuses it. Here the first unfilled pick is the next step.
    let old_active = *active;
    // The single part pick can survive a switch to two faces if it was itself a planar face.
    if let Some(input) = inputs.get_mut(2) {
        input.label = PART;
    }
    crate::dialogs::set_object_accepting(inputs, active, PART, SNAPS, f.center);
    crate::dialogs::set_object_accepting(inputs, active, PART_FACE_2, PLANAR_FACES, f.center && f.center_pair);
    if let Some(input) = inputs.get_mut(2) {
        input.label = if f.center_pair { PART_FACE_1 } else { PART };
        input.accept = if f.center_pair { PLANAR_FACES } else { SNAPS };
        if f.center_pair {
            input.items.retain(planar);
            input.from_face.clear();
        }
    }
    let next = inputs.iter().position(|i| i.items.is_empty()).unwrap_or(old_active);
    *active = next.min(inputs.len().saturating_sub(1));
}

/// Load the stored origins as picks for highlighting; keep the saved references when unchanged.
fn load(app: &SolveApp, f: &mut ConstrainForm, inputs: &mut Vec<SelInput>, active: &mut usize, id: u64) {
    let Some(j) = app.session.doc.assembly.joints.iter().find(|j| j.id == id) else { return };
    f.rule = j.constraint;
    f.center = j.constraint == Some(Constraint::Center);
    f.center_pair = false;
    let sides = if f.center {
        let (first, second) = if matches!(j.a.snap, joints::Snap::BetweenFaces { .. }) { (&j.a, &j.b) } else { (&j.b, &j.a) };
        let mut sides = between_face_picks(app, first).into_iter().flatten().collect::<Vec<_>>();
        if let Some(pair) = between_face_picks(app, second) {
            f.center_pair = true;
            sides.extend(pair);
        } else {
            sides.push(side_pick(app, second));
        }
        sides
    } else {
        vec![side_pick(app, &j.a), side_pick(app, &j.b)]
    };
    sync_inputs(app, f, inputs, active);
    for (input, side) in inputs.iter_mut().zip(&sides) {
        input.items = side.iter().cloned().collect();
        input.from_face.clear();
    }
    *active = 0;
    f.offset = j.exprs.offset.clone().unwrap_or_else(|| format!("{} mm", crate::dialogs_coupling::trim(j.offset)));
    f.angle = j.exprs.angle.clone().unwrap_or_else(|| format!("{} deg", crate::dialogs_coupling::trim(j.angle.to_degrees())));
    f.flip = false;
    f.error = None;
    f.editing = Some((id, sides));
}

/// Changing the mode or picks starts a new rule, as it does for ordinary constraints.
fn editing(f: &ConstrainForm, inputs: &[SelInput]) -> Option<u64> {
    let (id, sides) = f.editing.as_ref()?;
    inputs.iter().map(|i| i.items.first()).eq(sides.iter().map(Option::as_ref)).then_some(*id)
}

/// The dialog's rows; true when Enter was pressed in a value.
pub fn rows(app: &mut SolveApp, ui: &mut egui::Ui, f: &mut ConstrainForm, inputs: &mut Vec<SelInput>, active: &mut usize) -> bool {
    let mut enter = false;
    let was = (f.center, f.center_pair);
    let now = (!f.center).then(|| refs(app, f, inputs)).flatten();
    let fit = now.as_ref().map(|r| Constraint::fitting(r.shapes[0], r.shapes[1])).unwrap_or_default();
    if !fit.is_empty() {
        let mut pos = f.rule.and_then(|r| fit.iter().position(|c| *c == r)).unwrap_or(0);
        let labels: Vec<&str> = fit.iter().map(|c| label(*c)).collect();
        choice_row(ui, solidcraft_ui_text::tr("constrain.type"), "constrain_type", &mut pos, &labels);
        f.rule = fit.get(pos).copied();
        match f.rule {
            Some(Constraint::Angle) => enter |= expr_row(ui, solidcraft_ui_text::tr("constrain.angle"), &mut f.angle),
            Some(Constraint::Mate | Constraint::Flush | Constraint::Distance | Constraint::Tangent) => {
                enter |= expr_row(ui, solidcraft_ui_text::tr("constrain.offset"), &mut f.offset)
            }
            _ => {}
        }
    }
    crate::ui_kit::section(ui, "constrain_options", "Options", true, |ui| {
        let r = crate::ui_kit::checkbox(ui, solidcraft_ui_text::tr("constrain.center"), &mut f.center);
        crate::scenario::publish_handle("dialog:center_mode", r.rect.center());
        if f.center {
            let r = crate::ui_kit::checkbox(ui, solidcraft_ui_text::tr("constrain.center_pair"), &mut f.center_pair);
            crate::scenario::publish_handle("dialog:center_pair", r.rect.center());
        } else if !fit.is_empty() {
            crate::ui_kit::checkbox(ui, solidcraft_ui_text::tr("constrain.flip"), &mut f.flip);
        }
    });
    if was != (f.center, f.center_pair) {
        sync_inputs(app, f, inputs, active);
        f.editing = None;
        f.error = None;
    }
    let (cmds, parts) = command_and_parts(app, f, inputs);
    match cmds {
        Ok(cmds) => {
            let apply_label = solidcraft_ui_text::tr(if editing(f, inputs).is_some() { "constrain.change" } else { "constrain.apply" });
            let r = crate::ui_kit::button(ui, apply_label, crate::ui_kit::Button::Normal);
            crate::scenario::publish_handle("dialog:Apply", r.rect.center());
            if r.clicked() {
                let (id, params) = cmds;
                f.error = app.run(&id, params).err();
                if f.error.is_none() {
                    for input in inputs.iter_mut() {
                        input.items.clear();
                        input.from_face.clear();
                    }
                    *active = 0;
                    *f = ConstrainForm { pair: parts.or(f.pair), center: f.center, center_pair: f.center_pair, ..start_form() };
                    sync_inputs(app, f, inputs, active);
                }
            }
        }
        Err(message) => {
            let waiting = ["constrain.pick_hint", "constrain.center_hint", "constrain.center_part_hint"]
                .into_iter()
                .any(|id| message == solidcraft_ui_text::tr(id));
            status_line(ui, &message, if waiting { StatusTone::Dim } else { StatusTone::Error });
        }
    }
    // The rules already between the two parts, and how free the moving one is.
    if let Some((a, b)) = parts.or(f.pair) {
        let rules = between(&app.session.doc, (a, b));
        if !rules.is_empty() {
            ui.label(egui::RichText::new(solidcraft_ui_text::tr("constrain.between")).strong());
        }
        let mut load_id = None;
        for (id, name, rule, suppressed) in rules {
            // The name on its own line, its buttons under it: a long name keeps them in the panel.
            let text = format!("{name} ({})", label(rule));
            ui.label(if suppressed { egui::RichText::new(text).weak() } else { egui::RichText::new(text) });
            ui.horizontal(|ui| {
                let edit = crate::ui_kit::button(ui, solidcraft_ui_text::tr_label("Edit"), crate::ui_kit::Button::Normal);
                crate::scenario::publish_handle(&format!("dialog:edit:{name}"), edit.rect.center());
                if edit.clicked() {
                    load_id = Some(id);
                }
                let s = crate::ui_kit::button(
                    ui,
                    solidcraft_ui_text::tr_label(if suppressed { "Unsuppress" } else { "Suppress" }),
                    crate::ui_kit::Button::Normal,
                );
                crate::scenario::publish_handle(&format!("dialog:suppress:{name}"), s.rect.center());
                if s.clicked() {
                    f.error = app.run("assembly.constraint.edit", json!({"constraint": id, "suppressed": !suppressed})).err();
                }
                let d = crate::ui_kit::button(ui, solidcraft_ui_text::tr_label("Delete"), crate::ui_kit::Button::Normal);
                crate::scenario::publish_handle(&format!("dialog:delete:{name}"), d.rect.center());
                if d.clicked() {
                    f.error = app.run("joint.delete", json!({"joint": id})).err();
                }
            });
        }
        if let Some(id) = load_id {
            load(app, f, inputs, active, id);
        }
        // A root datum is held even when picked on the second side.
        let moving = if b == 0 { a } else { b };
        let (text, warn) = match app.preview.placed.as_ref().filter(|_| parts.is_some()) {
            Some(reply) => freedom(&app.session.doc, reply, moving),
            None => saved_freedom(app, f, moving),
        };
        let r = status_line(ui, &text, if warn { StatusTone::Warning } else { StatusTone::Dim });
        crate::scenario::publish_handle("dialog:constrain_status", r.rect.center());
    }
    if let Some(e) = &f.error {
        status_line(ui, e, StatusTone::Error);
    }
    enter
}

/// The command OK (and Apply) runs: a new rule, or a change to the one loaded from the list.
pub fn commands(app: &SolveApp, f: &ConstrainForm, inputs: &[SelInput]) -> Result<Vec<(String, Value)>, String> {
    command_and_parts(app, f, inputs).0.map(|(id, params)| vec![(id, params)])
}

type RuleCommand = (String, Value);

fn command_and_parts(app: &SolveApp, f: &ConstrainForm, inputs: &[SelInput]) -> (Result<RuleCommand, String>, Option<(u64, u64)>) {
    if f.center {
        return match center_command(app, f, inputs) {
            Ok((cmd, parts)) => (Ok(cmd), Some(parts)),
            Err(e) => (Err(e), None),
        };
    }
    let now = refs(app, f, inputs);
    let parts = now.as_ref().map(|r| held_and_moving(r, f.rule));
    (ordinary_command(app, f, inputs, now), parts)
}

fn ordinary_command(app: &SolveApp, f: &ConstrainForm, inputs: &[SelInput], refs: Option<Refs>) -> Result<RuleCommand, String> {
    let r = refs.ok_or_else(|| solidcraft_ui_text::tr("constrain.pick_hint").to_string())?;
    // The rule chosen, or the likeliest the picks fit (as the Type row shows before it is drawn).
    let fit = Constraint::fitting(r.shapes[0], r.shapes[1]);
    let rule = f.rule.filter(|c| fit.contains(c)).or(fit.first().copied());
    let rule = rule.ok_or_else(|| solidcraft_ui_text::tr("constrain.none_fit").to_string())?;
    let mut p = match (editing(f, inputs), r.params) {
        // A changed rule names its type; an unchanged one doesn't, as a new type sets the facing
        // again from the rule (a flipped mate whose gap is edited stays flipped).
        (Some(id), _) => {
            let was = app.session.doc.assembly.joints.iter().find(|j| j.id == id).and_then(|j| j.constraint);
            if was == Some(rule) { json!({"constraint": id}) } else { json!({"constraint": id, "type": rule.name()}) }
        }
        (None, Some([a, b])) => json!({"type": rule.name(), "a": a, "b": b}),
        (None, None) => return Err(solidcraft_ui_text::tr("constrain.pick_hint").to_string()),
    };
    if f.flip {
        p["flip"] = json!(true);
    }
    match rule {
        Constraint::Angle => p["angle"] = json!(f.angle.trim()),
        Constraint::Mate | Constraint::Flush | Constraint::Distance | Constraint::Tangent => p["offset"] = json!(f.offset.trim()),
        _ => {}
    }
    let id = if editing(f, inputs).is_some() { "assembly.constraint.edit" } else { "assembly.constraint.create" };
    Ok((id.into(), p))
}

/// The Center mode builds two joint origins. The wall pair and optional part pair share
/// the same format and validation; a plane or axis on its own uses the ordinary snap.
fn center_command(app: &SolveApp, f: &ConstrainForm, inputs: &[SelInput]) -> Result<(RuleCommand, (u64, u64)), String> {
    let text = |id: &str| solidcraft_ui_text::tr(id).to_string();
    if let Some(id) = editing(f, inputs)
        && let Some(j) = app.session.doc.assembly.joints.iter().find(|j| j.id == id)
    {
        return Ok((("assembly.constraint.edit".into(), json!({"constraint": id})), (j.a.part(), j.b.part())));
    }
    let pick = |i: usize| inputs.get(i).and_then(|input| input.items.first().map(|sel| (input, sel)));
    let pair = |i: usize, hint: &str| -> Result<Value, String> {
        let (_, a) = pick(i).ok_or_else(|| text(hint))?;
        let (_, b) = pick(i + 1).ok_or_else(|| text(hint))?;
        let face = |sel: &Sel| {
            let snap = picked_snap(app, sel).ok_or_else(|| text("constrain.center_walls"))?;
            snap.param.get("face").is_some().then_some(snap.param).ok_or_else(|| text("constrain.center_walls"))
        };
        let (a_param, b_param) = (face(a)?, face(b)?);
        if a_param.get("path") != b_param.get("path") || a_param.get("occurrence") != b_param.get("occurrence") {
            return Err(text("constrain.center_two_walls"));
        }
        if same_item(a, b) {
            return Err(text("constrain.center_distinct"));
        }
        let mut side = json!({"faces": [a_param["face"], b_param["face"]]});
        for key in ["occurrence", "path"] {
            if let Some(value) = a_param.get(key) {
                side[key] = value.clone();
            }
        }
        Ok(side)
    };
    let a = pair(0, "constrain.center_hint")?;
    let (command, kind, b) = if f.center_pair {
        ("assembly.constraint.create", "center", pair(2, "constrain.center_part_hint")?)
    } else {
        let (input, sel) = pick(2).ok_or_else(|| text("constrain.center_part_hint"))?;
        let part = snap_in(app, input, sel).ok_or_else(|| text("constrain.center_reference"))?;
        match shape(&part) {
            RefShape::Plane => ("assembly.constraint.create", "center", part.param),
            RefShape::Round | RefShape::Axis => ("joint.create", "line_on_plane", part.param),
            RefShape::Point | RefShape::Edge => return Err(text("constrain.center_reference")),
        }
    };
    let occurrence = |p: &Value| p.get("occurrence").and_then(Value::as_u64).unwrap_or(0);
    let parts = (occurrence(&a), occurrence(&b));
    Ok(((command.into(), json!({"type": kind, "a": a, "b": b})), parts))
}

#[cfg(test)]
#[path = "dialogs_constrain_tests.rs"]
mod tests;
