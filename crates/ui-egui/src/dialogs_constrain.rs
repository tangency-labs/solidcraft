//! The Constrain dialog (`assembly.constraint.create`, `assembly.constraint.edit`): a reference is
//! picked on each part first (a planar face, a round edge or face, a point), then the rules those
//! fit are offered (two faces: mate, flush, distance, parallel…; two round ones: concentric,
//! insert…). Apply adds the rule and keeps the dialog on the same two parts, for the next rule
//! between them. The rules already between the two parts are listed: each can be loaded to change
//! it (through `assembly.constraint.edit`), suppressed or deleted. A line says how free the moving
//! part is with the rule the preview shows.

use serde_json::{Value, json};
use solidcraft_engine::Sel;
use solidcraft_engine::doc::Document;
use solidcraft_engine::doc::joints::{self, Constraint, RefShape};

use crate::SolveApp;
use crate::dialogs_assembly::{Asm, Hover, SNAPS, SnapKind, SnapPt, picked_snap, side_pick};
use crate::selection::SelInput;
use crate::ui_kit::{StatusTone, choice_row, expr_row, status_line};

#[derive(Clone, Debug, Default)]
pub struct ConstrainForm {
    /// The rule chosen, among those that fit the picks.
    pub rule: Option<Constraint>,
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
    pub editing: Option<(u64, [Option<Sel>; 2])>,
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
fn label(c: Constraint) -> &'static str {
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
    })
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

/// Load constraint `id` into the form: its references as the picks, its rule, gap and angle.
fn load(app: &SolveApp, f: &mut ConstrainForm, inputs: &mut [SelInput], active: &mut usize, id: u64) {
    let Some(j) = app.session.doc.assembly.joints.iter().find(|j| j.id == id) else { return };
    let sides = [side_pick(app, &j.a), side_pick(app, &j.b)];
    for (input, side) in inputs.iter_mut().zip(&sides) {
        input.items = side.iter().cloned().collect();
    }
    *active = 0;
    f.rule = j.constraint;
    // A tangent's gap is its offset's expression; any other rule's, its offset.
    f.offset = j.exprs.offset.clone().unwrap_or_else(|| format!("{} mm", crate::dialogs_coupling::trim(j.offset)));
    f.angle = j.exprs.angle.clone().unwrap_or_else(|| format!("{} deg", crate::dialogs_coupling::trim(j.angle.to_degrees())));
    f.flip = false;
    f.error = None;
    f.editing = Some((id, sides));
}

/// The constraint being changed, while the picks are still the ones it was loaded with.
fn editing(f: &ConstrainForm, inputs: &[SelInput]) -> Option<u64> {
    let (id, sides) = f.editing.as_ref()?;
    let now: Vec<Option<&Sel>> = inputs.iter().take(2).map(|i| i.items.first()).collect();
    let was: Vec<Option<&Sel>> = sides.iter().map(Option::as_ref).collect();
    (now == was).then_some(*id)
}

/// The dialog's rows; true when Enter was pressed in a value.
pub fn rows(app: &mut SolveApp, ui: &mut egui::Ui, f: &mut ConstrainForm, inputs: &mut [SelInput], active: &mut usize) -> bool {
    let mut enter = false;
    let now = refs(app, f, inputs);
    let fit = now.as_ref().map(|r| Constraint::fitting(r.shapes[0], r.shapes[1])).unwrap_or_default();
    if fit.is_empty() {
        status_line(ui, solidcraft_ui_text::tr("constrain.pick_hint"), StatusTone::Dim);
    } else {
        // The rule chosen, or the likeliest when the picks changed.
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
        crate::ui_kit::options(ui, "constrain_options", &mut [(solidcraft_ui_text::tr("constrain.flip"), &mut f.flip)]);
        let apply_label = solidcraft_ui_text::tr(if editing(f, inputs).is_some() { "constrain.change" } else { "constrain.apply" });
        let r = crate::ui_kit::button(ui, apply_label, crate::ui_kit::Button::Normal);
        crate::scenario::publish_handle("dialog:Apply", r.rect.center());
        if r.clicked()
            && let Ok(cmds) = commands(app, f, inputs)
        {
            let parts = now.as_ref().map(|r| held_and_moving(r, f.rule));
            f.error = cmds.into_iter().find_map(|(id, p)| app.run(&id, p).err());
            // Done: keep the two parts, and pick the next rule on them again, from the first
            // reference and the defaults (a gap typed for one rule is not the next one's). Refused:
            // keep the picks and values, to correct the one that was wrong.
            if f.error.is_none() {
                for i in inputs.iter_mut() {
                    i.items.clear();
                }
                *active = 0;
                *f = ConstrainForm { pair: parts.or(f.pair), ..start_form() };
            }
        }
    }
    // The rules already between the two parts, and how free the moving one is.
    let parts = refs(app, f, inputs).map(|r| held_and_moving(&r, f.rule)).or(f.pair);
    if let Some((a, b)) = parts {
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
        // With a rule previewed, the preview's own reply; else the saved design.
        let (text, warn) = match app.preview.placed.as_ref().filter(|_| refs(app, f, inputs).is_some()) {
            Some(reply) => freedom(&app.session.doc, reply, b),
            None => saved_freedom(app, f, b),
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
    let r = refs(app, f, inputs).ok_or_else(|| solidcraft_ui_text::tr("constrain.pick_hint").to_string())?;
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
    Ok(vec![(id.into(), p)])
}

#[cfg(test)]
#[path = "dialogs_constrain_tests.rs"]
mod tests;
