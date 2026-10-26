//! Plastic dialogs: Boss, Lip / Groove, Snap Fit and Rest (placed by a pick on a face), Manage
//! Plastic Rules and Assign Plastic Rule. Values left empty take the body's plastic rule.

use crate::dialogs::put;
use crate::ui_kit::{choice_row, expr_row, hint_row};
use serde_json::{Map, Value, json};
use solidcraft_engine::Sel;
use solidcraft_engine::doc::FeatureKind;
use solidcraft_engine::doc::expr::Kind as ValueKind;
use solidcraft_engine::doc::plastic::PlasticRule;
use solidcraft_engine::geom::Vec3;

use crate::SolveApp;
use crate::dialogs::{Kind, combo, face_sel, field, form_row, shown_expr};
use crate::selection::{BODIES, FACES, PLANAR_FACES, SelInput, SelectionExt as _, point_value as pt};

crate::dialog_descriptor::dialog_descriptors! {
    #[derive(Clone, Debug)]
    pub enum Pl {
        Boss("plastic.boss", "BOSS", [("Position", PLANAR_FACES, false)],
            FeatureKind::Boss { diameter, height, hole_diameter, hole_depth, draft, fillet, ribs, rib_thickness, rib_length, rib_offset, .. }) {
            diameter: String = "6 mm".into() => shown_expr(diameter, ValueKind::Length),
            height: String = "10 mm".into() => shown_expr(height, ValueKind::Length),
            hole_diameter: String = "3 mm".into() => shown_expr(&s(hole_diameter), ValueKind::Length),
            hole_depth: String = String::new() => shown_expr(&s(hole_depth), ValueKind::Length),
            draft: String = String::new() => shown_expr(&s(draft), ValueKind::Angle),
            fillet: String = String::new() => shown_expr(&s(fillet), ValueKind::Length),
            ribs: String = "0".into() => ribs.as_deref().map(str::to_string).unwrap_or_else(|| "0".into()),
            rib_thickness: String = "1 mm".into() => rib_thickness.as_deref().map(|v| shown_expr(v, ValueKind::Length)).unwrap_or_else(|| "1 mm".into()),
            rib_length: String = "3 mm".into() => rib_length.as_deref().map(|v| shown_expr(v, ValueKind::Length)).unwrap_or_else(|| "3 mm".into()),
            rib_offset: String = "1 mm".into() => rib_offset.as_deref().map(|v| shown_expr(v, ValueKind::Length)).unwrap_or_else(|| "1 mm".into()),
        } => json!({"diameter": diameter, "height": height});
        Lip("plastic.lip", "LIP", [("Rim face", FACES, false)],
            FeatureKind::Lip { groove, width, height, gap, outside, .. }) {
            groove: bool = false => *groove,
            width: String = "1.5 mm".into() => shown_expr(width, ValueKind::Length),
            height: String = "2 mm".into() => shown_expr(height, ValueKind::Length),
            gap: String = String::new() => shown_expr(&s(gap), ValueKind::Length),
            outside: bool = false => *outside,
        } => json!({"width": width, "height": height, "type": if *groove { "groove" } else { "lip" }, "side": if *outside { "outside" } else { "inside" }});
        Rest("plastic.rest", "REST", [("Position", PLANAR_FACES, false)],
            FeatureKind::Rest { width, length, height, draft, thickness, .. }) {
            round: bool = true => length.is_none(),
            width: String = "10 mm".into() => shown_expr(width, ValueKind::Length),
            length: String = "15 mm".into() => length.as_deref().map(|l| shown_expr(l, ValueKind::Length)).unwrap_or_else(|| "15 mm".into()),
            height: String = "2 mm".into() => shown_expr(height, ValueKind::Length),
            draft: String = String::new() => shown_expr(&s(draft), ValueKind::Angle),
            thickness: String = String::new() => shown_expr(&s(thickness), ValueKind::Length),
        } => json!({"width": width, "height": height});
        ; custom {
            SnapFit {
                hook: usize,
                length: String,
                thickness: String,
                width: String,
                catch_depth: String,
                catch_length: String,
                base_height: String,
                new_body: bool,
            },
            /// The rule manager: the rule shown (index into the rules, past the end for a new one).
            Rules {
                pick: usize,
                name: String,
                values: Vec<String>,
                active: bool,
                loaded: Option<usize>,
            },
            Assign {
                rule: usize,
            },
        }
    }
}

impl Pl {
    pub fn title(&self) -> &'static str {
        match self {
            Pl::Lip { groove: true, .. } => "GROOVE",
            Pl::SnapFit { .. } => "SNAP FIT",
            Pl::Rules { .. } => "MANAGE PLASTIC RULES",
            Pl::Assign { .. } => "ASSIGN PLASTIC RULE",
            _ => self.simple_metadata().map_or("", |(_, title)| title),
        }
    }

    pub fn previews(&self) -> bool {
        !matches!(self, Pl::Rules { .. } | Pl::Assign { .. })
    }

    pub fn primary(&mut self) -> Option<(&'static str, ValueKind, &mut String)> {
        Some(match self {
            Pl::Boss { height, .. } | Pl::Lip { height, .. } | Pl::Rest { height, .. } => ("Height", ValueKind::Length, height),
            Pl::SnapFit { length, .. } => ("Length", ValueKind::Length, length),
            _ => return None,
        })
    }
}

fn snap_fit(app: &SolveApp) -> Pl {
    Pl::SnapFit {
        hook: 0,
        length: crate::dialogs::length_default(&app.session, "10"),
        thickness: crate::dialogs::length_default(&app.session, "1.5"),
        width: crate::dialogs::length_default(&app.session, "5"),
        catch_depth: crate::dialogs::length_default(&app.session, "1"),
        catch_length: crate::dialogs::length_default(&app.session, "2"),
        base_height: String::new(),
        new_body: false,
    }
}

/// The dialog for a plastic command.
pub fn start(app: &SolveApp, id: &str) -> Option<(Kind, Vec<SelInput>)> {
    let (pl, inputs) = match id {
        "plastic.snap_fit" => (snap_fit(app), vec![SelInput::new("Position", PLANAR_FACES, false)]),
        "plastic.manage_rules" => {
            let rules = app.session.doc.plastic_rules();
            let pick = app.session.doc.plastic.active.as_ref().and_then(|a| rules.iter().position(|r| &r.name == a)).unwrap_or(0);
            (Pl::Rules { pick, name: String::new(), values: Vec::new(), active: false, loaded: None }, vec![])
        }
        "plastic.assign_rule" => (Pl::Assign { rule: 1 }, vec![SelInput::new("Bodies", BODIES, true)]),
        _ => Pl::simple_start(id)?,
    };
    Some((crate::length_input::defaults(&app.session.doc, Kind::Plastic(pl)), inputs))
}

/// A value row whose empty value means "from the rule" (or another default).
fn opt_field(ui: &mut egui::Ui, label: &str, v: &mut String, unit: &str, hint: &str) -> bool {
    let hint = format!("{unit}, {}", crate::i18n::tr_label(hint));
    hint_row(ui, label, v, &hint)
}

/// Directions across a face (with normal `n`) a snap fit's catch can point: two axes and their
/// opposites, named after the world axis they run along.
pub fn hook_dirs(n: Vec3) -> Vec<(String, Vec3)> {
    let u = [Vec3::X, Vec3::Y, Vec3::Z].into_iter().filter_map(|a| (a - n * a.dot(n)).normalized()).next().unwrap_or_else(|| n.any_perp());
    let v = n.cross(u);
    let name = |d: Vec3| -> String {
        let (axis, c) = [("X", d.x), ("Y", d.y), ("Z", d.z)].into_iter().max_by(|a, b| a.1.abs().total_cmp(&b.1.abs())).unwrap_or(("X", 0.0));
        format!("{}{axis}", if c < 0.0 { "−" } else { "+" })
    };
    [u, -u, v, -v].into_iter().map(|d| (name(d), d)).collect()
}

fn face_normal(app: &SolveApp, inputs: &[SelInput]) -> Option<Vec3> {
    match inputs.first()?.items.first()? {
        Sel::Face { body, index, .. } => crate::dialogs::planar_face(&app.session, body, *index).map(|(_, n)| n),
        _ => None,
    }
}

/// The dialog's own rows (after the selection inputs); true when Enter was pressed in a value.
pub fn rows(app: &mut SolveApp, ui: &mut egui::Ui, k: &mut Pl, inputs: &[SelInput]) -> bool {
    let mut enter = false;
    match k {
        Pl::Boss { diameter, height, hole_diameter, hole_depth, draft, fillet, ribs, rib_thickness, rib_length, rib_offset } => {
            enter |= expr_row(ui, "Diameter", diameter);
            enter |= expr_row(ui, "Height", height);
            crate::ui_kit::section(ui, "boss_options", "Options", true, |ui| {
                enter |= opt_field(ui, "Hole Diameter", hole_diameter, crate::length_input::unit(&app.session.doc), "no hole");
                enter |= opt_field(ui, "Hole Depth", hole_depth, crate::length_input::unit(&app.session.doc), "to the face");
            });
            crate::ui_kit::section(ui, "boss_advanced", "Advanced", false, |ui| {
                enter |= opt_field(ui, "Draft Angle", draft, "deg", "from the rule");
                enter |= opt_field(ui, "Root Fillet", fillet, crate::length_input::unit(&app.session.doc), "none");
                enter |= expr_row(ui, "Ribs", ribs);
                if app.session.doc.eval(ribs, ValueKind::Unitless).is_ok_and(|n| n >= 1.0) {
                    enter |= expr_row(ui, "  Rib Thickness", rib_thickness);
                    enter |= expr_row(ui, "  Rib Length", rib_length);
                    enter |= expr_row(ui, "  Rib Offset", rib_offset);
                }
            });
        }
        Pl::Lip { groove, width, height, gap, outside } => {
            form_row(ui, "Type", |ui| {
                let mut ty = usize::from(*groove);
                combo(ui, "pl_lip", &["Lip", "Groove"], &mut ty);
                *groove = ty == 1;
            });
            form_row(ui, "Rim Edge", |ui| {
                let mut side = usize::from(*outside);
                combo(ui, "pl_side", &["Inside", "Outside"], &mut side);
                *outside = side == 1;
            });
            enter |= expr_row(ui, "Width", width);
            enter |= expr_row(ui, "Height", height);
            if *groove {
                enter |= opt_field(ui, "Clearance", gap, crate::length_input::unit(&app.session.doc), "from the rule");
            }
        }
        Pl::SnapFit { hook, length, thickness, width, catch_depth, catch_length, base_height, new_body } => {
            let dirs = face_normal(app, inputs).map(hook_dirs).unwrap_or_default();
            if !dirs.is_empty() {
                let names: Vec<&str> = dirs.iter().map(|(n, _)| n.as_str()).collect();
                choice_row(ui, "Catch Direction", "pl_hook", hook, &names);
            }
            enter |= expr_row(ui, "Length", length);
            enter |= expr_row(ui, "Thickness", thickness);
            enter |= expr_row(ui, "Width", width);
            enter |= expr_row(ui, "Catch Depth", catch_depth);
            enter |= expr_row(ui, "Catch Length", catch_length);
            enter |= opt_field(ui, "Base Height", base_height, crate::length_input::unit(&app.session.doc), "at the face");
            form_row(ui, "Body", |ui| {
                let mut op = usize::from(*new_body);
                combo(ui, "pl_snap_body", &["Join", "New Body"], &mut op);
                *new_body = op == 1;
            });
        }
        Pl::Rest { round, width, length, height, draft, thickness } => {
            form_row(ui, "Shape", |ui| {
                let mut sh = usize::from(!*round);
                combo(ui, "pl_rest", &["Round", "Rectangular"], &mut sh);
                *round = sh == 0;
            });
            enter |= expr_row(ui, if *round { "Diameter" } else { "Width" }, width);
            if !*round {
                enter |= expr_row(ui, "Length", length);
            }
            enter |= expr_row(ui, "Height", height);
            crate::ui_kit::section(ui, "rest_advanced", "Advanced", false, |ui| {
                enter |= opt_field(ui, "Draft Angle", draft, "deg", "from the rule");
                enter |= opt_field(ui, "Wall Thickness", thickness, crate::length_input::unit(&app.session.doc), "solid");
            });
        }
        Pl::Rules { pick, name, values, active, loaded } => {
            let rules = app.session.doc.plastic_rules();
            crate::rule_editor::rule_editor! {
                ui, "pl_rules", rules, pick, name, active, loaded;
                new_name: format!("Plastic Rule{}", app.session.doc.plastic.rules.len() + 1),
                load: {
                    let base = rules.get(*pick).or(rules.first()).cloned();
                    if let Some(b) = base {
                        *values = std::iter::once(b.material.clone())
                            .chain(b.fields().iter().map(|(_, e, kind)| {
                                if *kind == ValueKind::Length {
                                    crate::length_input::stored(&app.session.doc, e)
                                } else {
                                    crate::dialogs::shown_expr(e, *kind)
                                }
                            }))
                            .collect();
                        *active = app.session.doc.plastic.active.as_deref() == Some(b.name.as_str()) && *pick < rules.len();
                    }
                },
                fields: {
                    let keys = rule_keys();
                    for (i, v) in values.iter_mut().enumerate() {
                        let label = if i == 0 { "Material".to_string() } else { keys.get(i - 1).map(|k| label_of(k)).unwrap_or_default() };
                        form_row(ui, &label, |ui| {
                            enter |= if i == 0 { crate::ui_kit::text_field(ui, v, "Material").0 } else { field(ui, v) };
                        });
                    }
                }
            }
        }
        Pl::Assign { rule } => {
            let mut names: Vec<String> = vec!["(no rule)".into()];
            names.extend(app.session.doc.plastic_rules().into_iter().map(|r| r.name));
            let refs: Vec<&str> = names.iter().map(String::as_str).collect();
            choice_row(ui, "Rule", "pl_assign", rule, &refs);
        }
    }
    enter
}

/// The rule's value fields, in the order `PlasticRule::fields` gives them.
fn rule_keys() -> Vec<&'static str> {
    PlasticRule::library().first().map(|r| r.fields().iter().map(|(k, _, _)| *k).collect()).unwrap_or_default()
}

fn label_of(key: &str) -> String {
    key.split('_')
        .map(|w| {
            let mut c = w.chars();
            c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// The commands the dialog's OK runs (`extra`: parameters kept from the feature being edited).
pub fn commands(app: &SolveApp, k: &Pl, inputs: &[SelInput], extra: &Map<String, Value>) -> Result<Vec<(String, Value)>, String> {
    let face = inputs
        .first()
        .and_then(|i| i.items.first())
        .and_then(|x| if let Sel::Face { body, point, .. } = x { Some((body.clone(), *point)) } else { None });
    let placed = |what: &str| -> Result<Value, String> {
        let (body, at) = face.clone().ok_or_else(|| format!("select {what} first"))?;
        Ok(json!({"position": pt(at), "body": body}))
    };
    let (cmd, mut p): (&str, Value) = match k {
        Pl::Boss { hole_diameter, hole_depth, draft, fillet, ribs, rib_thickness, rib_length, rib_offset, .. } => {
            let mut p = placed("a point on a face")?;
            put(&mut p, &[("hole_diameter", hole_diameter), ("hole_depth", hole_depth), ("draft", draft), ("fillet", fillet)]);
            if app.session.doc.eval(ribs, ValueKind::Unitless).is_ok_and(|n| n >= 1.0) {
                put(&mut p, &[("ribs", ribs), ("rib_thickness", rib_thickness), ("rib_length", rib_length), ("rib_offset", rib_offset)]);
            }
            ("plastic.boss", p)
        }
        Pl::Lip { groove, gap, .. } => {
            let (body, at) = face.clone().ok_or("select the rim face first")?;
            let mut p = json!({"face": pt(at), "body": body});
            if *groove {
                put(&mut p, &[("gap", gap)]);
            }
            ("plastic.lip", p)
        }
        Pl::SnapFit { hook, length, thickness, width, catch_depth, catch_length, base_height, new_body } => {
            let mut p = placed("a point on a face")?;
            let n = extra
                .get("direction")
                .and_then(|v| Some(Vec3::new(v.get(0)?.as_f64()?, v.get(1)?.as_f64()?, v.get(2)?.as_f64()?)))
                .or(face_normal(app, inputs));
            let dir = n.map(hook_dirs).and_then(|d| d.get(*hook).map(|x| x.1)).ok_or("pick a planar face")?;
            p["hook"] = pt(dir);
            for (key, v) in
                [("length", length), ("thickness", thickness), ("width", width), ("catch_depth", catch_depth), ("catch_length", catch_length)]
            {
                p[key] = json!(v);
            }
            put(&mut p, &[("base_height", base_height)]);
            if *new_body {
                p["new_body"] = json!(true);
            }
            ("plastic.snap_fit", p)
        }
        Pl::Rest { round, length, draft, thickness, .. } => {
            let mut p = placed("a point on a face")?;
            if !*round {
                p["length"] = json!(length);
            }
            put(&mut p, &[("draft", draft), ("thickness", thickness)]);
            ("plastic.rest", p)
        }
        Pl::Rules { pick, name, values, active, .. } => {
            let rules = app.session.doc.plastic_rules();
            let fields = std::iter::once("material").chain(rule_keys()).zip(values);
            let p = crate::rule_editor::params(rules.get(*pick).map(|r| r.name.as_str()), name, *active, fields)?;
            ("plastic.manage_rules", p)
        }
        Pl::Assign { rule } => {
            let bodies = crate::selection::items(inputs, 0).body_names();
            if bodies.is_empty() {
                return Err("select bodies first".into());
            }
            let name = rule.checked_sub(1).and_then(|i| app.session.doc.plastic_rules().get(i).map(|r| r.name.clone())).unwrap_or_default();
            ("plastic.assign_rule", json!({"bodies": bodies, "rule": name}))
        }
    };
    k.fill_simple_params(&mut p);
    if let Value::Object(m) = &mut p {
        for (key, v) in extra {
            m.entry(key.clone()).or_insert_with(|| v.clone());
        }
    }
    Ok(vec![(cmd.to_string(), p)])
}

fn s(o: &Option<solidcraft_engine::doc::Expr>) -> String {
    o.as_deref().map(str::to_string).unwrap_or_default()
}

/// The dialog editing a plastic feature, filled from it.
pub fn for_feature(app: &SolveApp, kind: &FeatureKind) -> Option<(Kind, Vec<SelInput>, Map<String, Value>)> {
    let ss = &app.session;
    let mut extra = Map::new();
    let (pl, id, at): (Pl, &str, Vec3) = match kind {
        FeatureKind::Boss { position, direction, body, .. } => {
            extra.insert("direction".into(), pt(direction.0));
            if let Some(b) = body {
                extra.insert("body".into(), json!(b));
            }
            (Pl::simple_restore(kind)?, "plastic.boss", position.0)
        }
        FeatureKind::Lip { face, body, .. } => {
            if let Some(b) = body {
                extra.insert("body".into(), json!(b));
            }
            (Pl::simple_restore(kind)?, "plastic.lip", face.0)
        }
        FeatureKind::SnapFit { position, direction, hook, length, thickness, width, catch_depth, catch_length, base_height, new_body, body } => {
            extra.insert("direction".into(), pt(direction.0));
            if let Some(b) = body {
                extra.insert("body".into(), json!(b));
            }
            let h = hook_dirs(direction.0).iter().position(|(_, d)| d.dist(hook.0) < 1e-6);
            if h.is_none() {
                extra.insert("hook".into(), pt(hook.0));
            }
            let f = Pl::SnapFit {
                hook: h.unwrap_or(0),
                length: shown_expr(length, ValueKind::Length),
                thickness: shown_expr(thickness, ValueKind::Length),
                width: shown_expr(width, ValueKind::Length),
                catch_depth: shown_expr(catch_depth, ValueKind::Length),
                catch_length: shown_expr(catch_length, ValueKind::Length),
                base_height: base_height.as_deref().map(|v| shown_expr(v, ValueKind::Length)).unwrap_or_default(),
                new_body: *new_body,
            };
            (f, "plastic.snap_fit", position.0)
        }
        FeatureKind::Rest { position, direction, along, body, .. } => {
            extra.insert("direction".into(), pt(direction.0));
            if let Some(a) = along {
                extra.insert("along".into(), pt(a.0));
            }
            if let Some(b) = body {
                extra.insert("body".into(), json!(b));
            }
            let r = Pl::simple_restore(kind)?;
            (r, "plastic.rest", position.0)
        }
        _ => return None,
    };
    let (_, mut inputs) = start(app, id)?;
    if let Some(i) = inputs.first_mut() {
        i.items = face_sel(ss, at).into_iter().collect();
    }
    // A pick that no longer finds its face keeps the stored point.
    if let Some(i) = inputs.first_mut().filter(|i| i.items.is_empty()) {
        i.items = vec![Sel::Face { body: extra.get("body").and_then(Value::as_str).unwrap_or_default().to_string(), index: 0, point: at }];
    }
    Some((Kind::Plastic(pl), inputs, extra))
}

#[cfg(test)]
#[path = "dialogs_plastic_tests.rs"]
mod tests;
