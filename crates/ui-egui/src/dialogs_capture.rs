//! Edit dialog for captured occurrence positions and joints (`FeatureKind::CapturePosition`).
use crate::{
    SolveApp,
    dialogs::{Kind, shown_expr},
    selection::SelInput,
    ui_kit::{self, choice_row, expr_row},
};
use serde_json::{Map, Value, json};
use solidcraft_engine::doc::{CapturedJoint, CapturedOccurrenceMove, Document, Mat, MoveSpace, expr::Kind as ValueKind, joints::JointKind};
use solidcraft_ui_text::tr;
use std::collections::BTreeMap;

/// Editable draft for one [`CapturedOccurrenceMove`] inside a `CapturePosition` timeline feature.
#[derive(Clone, Debug)]
pub struct CaptureMoveDraft {
    pub occurrence: u64,
    pub label: String,
    /// `0` = Local, `1` = World.
    pub space: usize,
    pub x: String,
    pub y: String,
    pub z: String,
    pub has_origin: bool,
    pub ox: String,
    pub oy: String,
    pub oz: String,
    pub rotate_axis: Option<[f64; 3]>,
    pub angle: Option<String>,
    /// Preserved verbatim for moves captured from `matrix`, `to`, `to_matrix`, `align`, or `drag`.
    pub rotation: Option<Mat>,
    /// Preserved verbatim for nested occurrences moved in `MoveSpace::World`.
    pub path: Option<Vec<u64>>,
}

/// Editable draft for one [`CapturedJoint`] inside a `CapturePosition` timeline feature.
#[derive(Clone, Debug)]
pub struct CaptureJointDraft {
    pub joint: u64,
    pub name: String,
    pub kind: JointKind,
    pub label: String,
    pub is_angle: Vec<bool>,
    pub dof_labels: Vec<&'static str>,
    pub dofs: Vec<String>,
    pub initial_dofs: Vec<String>,
    pub raw_values: Vec<f64>,
    pub raw_exprs: Vec<Option<String>>,
}

/// Dialog state for editing a `FeatureKind::CapturePosition` feature (`Kind::CapturePosition`).
#[derive(Clone, Debug)]
pub struct CapturePosition {
    pub moves: Vec<CaptureMoveDraft>,
    pub joints: Vec<CaptureJointDraft>,
    pub selected_move: usize,
    pub selected_joint: usize,
}

fn dof_label(name: &str) -> &'static str {
    match name {
        "angle" => "ui.angle",
        "distance" => "ui.distance",
        "slide" => "ui.capture_position.slide",
        "x" => "X",
        "y" => "Y",
        "z" => "Z",
        "yaw" => "ui.view_angles_yaw",
        "pitch" => "ui.view_angles_pitch",
        "roll" => "ui.view_angles_roll",
        "turn" => "ui.capture_position.turn",
        "spin" => "ui.capture_position.spin",
        _ => "Value",
    }
}

/// Build the edit dialog state for a `CapturePosition` feature, or `None` if nothing is editable.
pub fn for_feature(doc: &Document, moves: &[CapturedOccurrenceMove], joints: &[CapturedJoint]) -> Option<(Kind, Vec<SelInput>, Map<String, Value>)> {
    let base_names: Vec<String> = moves
        .iter()
        .map(|m| doc.occurrences.iter().find(|o| o.id == m.occurrence).map_or_else(|| format!("Occurrence {}", m.occurrence), |o| o.name.clone()))
        .collect();
    let mut name_counts: BTreeMap<&str, usize> = BTreeMap::new();
    for name in &base_names {
        *name_counts.entry(name.as_str()).or_insert(0) += 1;
    }
    let mut name_seen: BTreeMap<&str, usize> = BTreeMap::new();
    let mut move_drafts = Vec::with_capacity(moves.len());
    for (m, base_name) in moves.iter().zip(&base_names) {
        let label = if name_counts.get(base_name.as_str()).copied().unwrap_or(1) > 1 {
            let idx = name_seen.entry(base_name.as_str()).or_insert(0);
            *idx += 1;
            format!("{base_name} #{idx}")
        } else {
            base_name.clone()
        };
        let x = shown_expr(&m.translate[0], ValueKind::Length);
        let y = shown_expr(&m.translate[1], ValueKind::Length);
        let z = shown_expr(&m.translate[2], ValueKind::Length);
        let (has_origin, ox, oy, oz, rotate_axis, angle, rotation) = if m.rotation.is_some() {
            (false, String::new(), String::new(), String::new(), None, None, m.rotation)
        } else {
            let (has_origin, ox, oy, oz) = match &m.origin {
                Some(o) => (true, shown_expr(&o[0], ValueKind::Length), shown_expr(&o[1], ValueKind::Length), shown_expr(&o[2], ValueKind::Length)),
                None => (false, "0 mm".into(), "0 mm".into(), "0 mm".into()),
            };
            (has_origin, ox, oy, oz, m.rotate_axis, m.angle.as_ref().map(|a| shown_expr(a, ValueKind::Angle)), None)
        };
        move_drafts.push(CaptureMoveDraft {
            occurrence: m.occurrence,
            label,
            space: usize::from(m.space == MoveSpace::World),
            x,
            y,
            z,
            has_origin,
            ox,
            oy,
            oz,
            rotate_axis,
            angle,
            rotation,
            path: m.path.clone(),
        });
    }

    let mut joint_drafts = Vec::with_capacity(joints.len());
    for cj in joints {
        let Some(dj) = doc.assembly.joints.iter().find(|j| j.id == cj.joint && j.kind != JointKind::Rigid) else {
            continue;
        };
        let dof_names = dj.kind.dofs();
        let dof_count = dof_names.len().max(cj.values.len()).max(cj.exprs.len()).min(64);
        let mut is_angle = Vec::with_capacity(dof_count);
        let mut dof_labels = Vec::with_capacity(dof_count);
        let mut dofs = Vec::with_capacity(dof_count);
        for i in 0..dof_count {
            let angle = dj.kind.is_angle(i);
            is_angle.push(angle);
            dof_labels.push(dof_label(dof_names.get(i).copied().unwrap_or("value")));
            let shown = if let Some(expr) = cj.exprs.get(i).and_then(|e| e.as_deref()) {
                shown_expr(expr, if angle { ValueKind::Angle } else { ValueKind::Length })
            } else {
                let v = cj.values.get(i).copied().unwrap_or(0.0);
                if angle { crate::dialogs_assembly::num(v.to_degrees(), "deg") } else { crate::length_input::model(doc, v) }
            };
            dofs.push(shown);
        }
        joint_drafts.push(CaptureJointDraft {
            joint: cj.joint,
            name: dj.name.clone(),
            kind: dj.kind,
            label: crate::dialogs_assembly::joint_choice(&dj.name, dj.kind),
            is_angle,
            dof_labels,
            initial_dofs: dofs.clone(),
            dofs,
            raw_values: cj.values.clone(),
            raw_exprs: cj.exprs.clone(),
        });
    }

    if move_drafts.is_empty() && joint_drafts.is_empty() {
        return None;
    }
    Some((
        Kind::CapturePosition(CapturePosition { moves: move_drafts, joints: joint_drafts, selected_move: 0, selected_joint: 0 }),
        Vec::new(),
        Map::new(),
    ))
}

/// Render the `CapturePosition` dialog form rows using `ui_kit`.
pub fn rows(_app: &SolveApp, ui: &mut egui::Ui, c: &mut CapturePosition) -> bool {
    let mut enter = false;
    let mut has_move_angle = false;
    if !c.moves.is_empty() {
        let comp_label = tr("render.pick_component");
        if c.moves.len() > 1 {
            c.selected_move = c.selected_move.min(c.moves.len().saturating_sub(1));
            let labels: Vec<&str> = c.moves.iter().map(|m| m.label.as_str()).collect();
            choice_row(ui, comp_label, "capture_move_occ", &mut c.selected_move, &labels);
        } else if let Some(m) = c.moves.first() {
            ui_kit::value_row(ui, comp_label, &m.label);
        }
        if let Some(m) = c.moves.get_mut(c.selected_move) {
            choice_row(
                ui,
                tr("ui.capture_position.space"),
                "capture_move_space",
                &mut m.space,
                &["ui.capture_position.local", "ui.capture_position.world"],
            );
            enter |= expr_row(ui, tr("ui.x_distance"), &mut m.x);
            enter |= expr_row(ui, tr("ui.y_distance"), &mut m.y);
            enter |= expr_row(ui, tr("Z Distance"), &mut m.z);
            if m.rotation.is_some() {
                ui_kit::text(ui, tr("ui.capture_position.rotation_preserved"), ui_kit::Text::Hint("capture_rot"));
            } else {
                if let Some(angle) = &mut m.angle {
                    has_move_angle = true;
                    enter |= expr_row(ui, tr("ui.angle"), angle);
                }
                if m.has_origin {
                    ui_kit::section(ui, "capture_origin", tr("ui.origin"), false, |ui| {
                        enter |= expr_row(ui, tr("ui.capture_position.origin_x"), &mut m.ox);
                        enter |= expr_row(ui, tr("ui.capture_position.origin_y"), &mut m.oy);
                        enter |= expr_row(ui, tr("ui.capture_position.origin_z"), &mut m.oz);
                    });
                }
                if let Some([ax, ay, az]) = m.rotate_axis {
                    ui_kit::value_row(ui, tr("ui.axis"), &format!("{ax:.4}, {ay:.4}, {az:.4}"));
                }
            }
        }
    }
    if !c.joints.is_empty() {
        let default_open = c.moves.is_empty();
        ui_kit::section(ui, "capture_joints", tr("ui.joints"), default_open, |ui| {
            let joint_label = tr("cmd.joint_create");
            if c.joints.len() > 1 {
                c.selected_joint = c.selected_joint.min(c.joints.len().saturating_sub(1));
                let labels: Vec<String> = c.joints.iter().map(|j| crate::dialogs_assembly::joint_choice(&j.name, j.kind)).collect();
                let refs: Vec<&str> = labels.iter().map(String::as_str).collect();
                choice_row(ui, joint_label, "capture_joint_sel", &mut c.selected_joint, &refs);
            } else if let Some(j) = c.joints.first() {
                ui_kit::value_row(ui, joint_label, &crate::dialogs_assembly::joint_choice(&j.name, j.kind));
            }
            if let Some(j) = c.joints.get_mut(c.selected_joint) {
                for (i, val) in j.dofs.iter_mut().enumerate() {
                    let key = j.dof_labels.get(i).copied().unwrap_or("Value");
                    let label = tr(key);
                    if key == "ui.angle" && has_move_angle {
                        enter |= ui_kit::expr_row_as(ui, label, "joint:Angle", val);
                    } else {
                        enter |= expr_row(ui, label, val);
                    }
                }
            }
        });
    }
    enter
}

/// Build the `component.capture_position` command parameters (wrapped in `timeline.redefine` by
/// `dialogs::apply_commands`).
pub fn commands(app: &SolveApp, c: &CapturePosition) -> Result<Vec<(String, Value)>, String> {
    let mut moves = Vec::with_capacity(c.moves.len());
    for m in &c.moves {
        let (x, y, z) = (m.x.trim(), m.y.trim(), m.z.trim());
        if x.is_empty() || y.is_empty() || z.is_empty() {
            return Err("enter X, Y and Z distances".into());
        }
        let mut obj = Map::new();
        obj.insert("occurrence".into(), json!(m.occurrence));
        obj.insert("space".into(), json!(if m.space == 1 { "world" } else { "local" }));
        obj.insert("translate".into(), json!([x, y, z]));
        if let Some(rot) = m.rotation {
            obj.insert("rotation".into(), json!(rot));
        } else {
            if m.has_origin {
                let (ox, oy, oz) = (m.ox.trim(), m.oy.trim(), m.oz.trim());
                if ox.is_empty() || oy.is_empty() || oz.is_empty() {
                    return Err("enter origin X, Y and Z".into());
                }
                obj.insert("origin".into(), json!([ox, oy, oz]));
            }
            if let Some(axis) = m.rotate_axis {
                obj.insert("rotate_axis".into(), json!(axis));
            }
            if let Some(angle) = &m.angle {
                let angle = angle.trim();
                if angle.is_empty() {
                    return Err("enter an angle".into());
                }
                obj.insert("angle".into(), json!(angle));
            }
        }
        if let Some(path) = &m.path {
            obj.insert("path".into(), json!(path));
        }
        moves.push(Value::Object(obj));
    }

    let mut joints = Vec::with_capacity(c.joints.len());
    for j in &c.joints {
        let mut values = Vec::with_capacity(j.dofs.len());
        let mut exprs: Vec<Option<String>> = Vec::with_capacity(j.dofs.len());
        for (i, dof) in j.dofs.iter().enumerate() {
            if j.initial_dofs.get(i).is_some_and(|init| init == dof) {
                values.push(j.raw_values.get(i).copied().unwrap_or(0.0));
                exprs.push(j.raw_exprs.get(i).cloned().flatten());
            } else {
                let text = dof.trim();
                if text.is_empty() {
                    return Err("enter a joint value".into());
                }
                let label = tr(j.dof_labels.get(i).copied().unwrap_or("Value"));
                crate::length_input::no_definition(text, label)?;
                let kind = if j.is_angle.get(i).copied().unwrap_or(false) { ValueKind::Angle } else { ValueKind::Length };
                let v = app.session.doc.eval(text, kind).map_err(|e| e.to_string())?;
                values.push(v);
                exprs.push(Some(text.to_string()));
            }
        }
        while exprs.last().is_some_and(Option::is_none) {
            exprs.pop();
        }
        joints.push(json!({
            "joint": j.joint,
            "values": values,
            "exprs": exprs,
        }));
    }

    if moves.is_empty() && joints.is_empty() {
        return Err("nothing to capture".into());
    }
    let mut params = Map::new();
    if !moves.is_empty() {
        params.insert("moves".into(), Value::Array(moves));
    }
    if !joints.is_empty() {
        params.insert("joints".into(), Value::Array(joints));
    }
    Ok(vec![("component.capture_position".into(), Value::Object(params))])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenario::Harness;
    use solidcraft_engine::doc::FeatureKind;
    use solidcraft_engine::geom::MeshGeom as _;

    fn two_part_harness() -> (Harness, u64, u64) {
        let mut h = Harness::new();
        h.app.run("parameters.add", json!({"name": "dx", "expression": "10 mm"})).unwrap();
        h.app.run("parameters.add", json!({"name": "dy", "expression": "5 mm"})).unwrap();
        let o1 = h.app.run("component.create", json!({"name": "PartA", "grounded": false})).unwrap()["occurrence"].as_u64().unwrap();
        h.app.run("solid.box", json!({"length": 10, "width": 10, "height": 10, "body_name": "BoxA"})).unwrap();
        h.app.run("component.activate", json!({"component": "root"})).unwrap();
        let o2 = h.app.run("component.create", json!({"name": "PartB", "grounded": false})).unwrap()["occurrence"].as_u64().unwrap();
        h.app.run("solid.box", json!({"length": 10, "width": 10, "height": 10, "body_name": "BoxB"})).unwrap();
        h.app.run("component.activate", json!({"component": "root"})).unwrap();
        (h, o1, o2)
    }

    fn apply(h: &mut Harness, d: &crate::dialogs::Dialog) {
        for (cmd, params) in crate::dialogs::apply_commands(&h.app, d).unwrap() {
            h.app.run(&cmd, params).unwrap();
        }
    }

    #[test]
    fn single_move_translation_expression_round_trip_preserves_dn_and_rebuilds() {
        let (mut h, o1, _) = two_part_harness();
        h.app.run("occurrence.move", json!({"occurrence": o1, "translate": ["dx", "0", "0"], "capture": false})).unwrap();
        let cp_id = h.app.run("component.capture_position", json!({})).unwrap()["feature"].as_u64().unwrap();
        let dn_before: Vec<_> = h.app.session.doc.params.iter().filter(|p| p.model).map(|p| p.name.clone()).collect();

        let mut d = crate::dialogs::for_feature(&h.app, cp_id, None).unwrap();
        assert!(format!("{d:?}").contains("CapturePosition"));
        if let Kind::CapturePosition(c) = &mut d.kind {
            assert_eq!(c.moves[0].x, "dx");
            c.moves[0].x = "dx * 2".into();
            c.moves[0].y = "dy".into();
        } else {
            panic!("expected Kind::CapturePosition");
        }
        apply(&mut h, &d);

        let FeatureKind::CapturePosition { moves, .. } = &h.app.session.doc.feature(cp_id).unwrap().kind else {
            panic!("expected CapturePosition");
        };
        assert_eq!(moves[0].translate, ["dx * 2", "dy", "0 mm"]);
        assert!(moves[0].angle.is_none(), "pure translation edit must not invent angle");
        let dn_after: Vec<_> = h.app.session.doc.params.iter().filter(|p| p.model).map(|p| p.name.clone()).collect();
        assert_eq!(dn_after, dn_before, "dN parameter names must stay stable");

        h.app.run("parameters.change", json!({"name": "dx", "expression": "15 mm"})).unwrap();
        let b = h.app.session.world_state().unwrap().body("BoxA").unwrap().mesh().bounds();
        assert!((b.min.x - 30.0).abs() < 1e-5 && (b.min.y - 5.0).abs() < 1e-5, "{b:?}");
    }

    #[test]
    fn multi_occurrence_edit_preserves_other_moves() {
        let (mut h, o1, o2) = two_part_harness();
        h.app.run("occurrence.move", json!({"occurrence": o1, "translate": ["dx", "0", "0"], "capture": false})).unwrap();
        h.app.run("occurrence.move", json!({"occurrence": o2, "translate": ["0", "dy", "0"], "capture": false})).unwrap();
        let cp_id = h.app.run("component.capture_position", json!({})).unwrap()["feature"].as_u64().unwrap();

        let mut d = crate::dialogs::for_feature(&h.app, cp_id, None).unwrap();
        if let Kind::CapturePosition(c) = &mut d.kind {
            assert_eq!(c.moves.len(), 2);
            c.moves[0].x = "25 mm".into();
        } else {
            panic!("expected Kind::CapturePosition");
        }
        apply(&mut h, &d);

        let FeatureKind::CapturePosition { moves, .. } = &h.app.session.doc.feature(cp_id).unwrap().kind else {
            panic!("expected CapturePosition");
        };
        assert_eq!(moves.len(), 2);
        assert_eq!(moves[0].translate[0], "25 mm");
        assert_eq!(moves[1].translate[1], "dy");
        let b2 = h.app.session.world_state().unwrap().body("BoxB").unwrap().mesh().bounds();
        assert!((b2.min.y - 5.0).abs() < 1e-5, "{b2:?}");
    }

    #[test]
    fn axis_angle_preserves_origin_and_rotate_axis() {
        let (mut h, o1, _) = two_part_harness();
        h.app.run("parameters.add", json!({"name": "ox", "expression": "5 mm"})).unwrap();
        h.app.run("parameters.add", json!({"name": "theta", "expression": "90 deg"})).unwrap();
        h.app
            .run(
                "occurrence.move",
                json!({"occurrence": o1, "origin": ["ox", "0 mm", "0 mm"], "axis": [0, 0, 1], "angle": "theta", "capture": false}),
            )
            .unwrap();
        let cp_id = h.app.run("component.capture_position", json!({})).unwrap()["feature"].as_u64().unwrap();

        let mut d = crate::dialogs::for_feature(&h.app, cp_id, None).unwrap();
        if let Kind::CapturePosition(c) = &mut d.kind {
            assert!(c.moves[0].has_origin);
            assert_eq!(c.moves[0].ox, "ox");
            assert_eq!(c.moves[0].angle.as_deref(), Some("theta"));
            assert_eq!(c.moves[0].rotate_axis, Some([0.0, 0.0, 1.0]));
            c.moves[0].angle = Some("theta * 2".into());
        } else {
            panic!("expected Kind::CapturePosition");
        }
        apply(&mut h, &d);

        let FeatureKind::CapturePosition { moves, .. } = &h.app.session.doc.feature(cp_id).unwrap().kind else {
            panic!("expected CapturePosition");
        };
        assert_eq!(moves[0].origin.as_ref().map(|o| o[0].as_str()), Some("ox"));
        assert_eq!(moves[0].rotate_axis, Some([0.0, 0.0, 1.0]));
        assert_eq!(moves[0].angle.as_deref(), Some("theta * 2"));
    }

    #[test]
    fn fallback_rotation_matrix_is_preserved_verbatim() {
        let (mut h, o1, _) = two_part_harness();
        // 90 deg rotation about Z with translation [10, 0, 0] via matrix -> sets rotation: Some(Mat).
        h.app
            .run(
                "occurrence.move",
                json!({"occurrence": o1, "matrix": [[0.0, -1.0, 0.0, 10.0], [1.0, 0.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0]], "capture": false}),
            )
            .unwrap();
        let cp_id = h.app.run("component.capture_position", json!({})).unwrap()["feature"].as_u64().unwrap();

        let mut d = crate::dialogs::for_feature(&h.app, cp_id, None).unwrap();
        let original_rot = match &mut d.kind {
            Kind::CapturePosition(c) => {
                assert!(c.moves[0].rotation.is_some());
                assert!(!c.moves[0].has_origin && c.moves[0].angle.is_none());
                c.moves[0].x = "20 mm".into();
                c.moves[0].rotation
            }
            _ => panic!("expected Kind::CapturePosition"),
        };
        apply(&mut h, &d);

        let FeatureKind::CapturePosition { moves, .. } = &h.app.session.doc.feature(cp_id).unwrap().kind else {
            panic!("expected CapturePosition");
        };
        assert_eq!(moves[0].rotation, original_rot);
        let b = h.app.session.world_state().unwrap().body("BoxA").unwrap().mesh().bounds();
        assert!((b.min.x - 10.0).abs() < 1e-5 && (b.max.x - 20.0).abs() < 1e-5 && (b.max.y - 10.0).abs() < 1e-5, "{b:?}");
    }

    #[test]
    fn captured_joints_preserve_untouched_and_edit_expressions() {
        let (mut h, o1, o2) = two_part_harness();
        h.app.run("parameters.add", json!({"name": "theta", "expression": "90 deg"})).unwrap();
        h.app.run("occurrence.ground", json!({"occurrence": o1, "grounded": true})).unwrap();
        h.app.run("occurrence.move", json!({"occurrence": o2, "translate": [20, 0, 0]})).unwrap();
        h.app
            .run(
                "joint.create",
                json!({
                    "type": "revolute",
                    "name": "Hinge",
                    "a": {"face": [10, 5, 5]},
                    "b": {"face": [20, 5, 5]}
                }),
            )
            .unwrap();
        h.app.run("joint.drive", json!({"joint": "Hinge", "value": 45.0})).unwrap();
        let cp_id = h.app.run("component.capture_position", json!({})).unwrap()["feature"].as_u64().unwrap();

        let mut d = crate::dialogs::for_feature(&h.app, cp_id, None).unwrap();
        if let Kind::CapturePosition(c) = &mut d.kind {
            assert_eq!(c.joints.len(), 1);
            assert_eq!(c.joints[0].dofs[0], "45 deg");
            // Edit joint DOF to parameter expression.
            c.joints[0].dofs[0] = "theta".into();
        } else {
            panic!("expected Kind::CapturePosition");
        }
        apply(&mut h, &d);

        let FeatureKind::CapturePosition { joints, .. } = &h.app.session.doc.feature(cp_id).unwrap().kind else {
            panic!("expected CapturePosition");
        };
        assert_eq!(joints[0].exprs, vec![Some("theta".into())]);
        assert!((joints[0].values[0] - 90_f64.to_radians()).abs() < 1e-6);

        // Changing theta updates joint pose.
        h.app.run("parameters.change", json!({"name": "theta", "expression": "180 deg"})).unwrap();
        let j = h.app.session.doc.assembly.joints.iter().find(|j| j.name == "Hinge").unwrap();
        assert!((j.values[0] - 180_f64.to_radians()).abs() < 1e-6);
    }

    #[test]
    fn inch_design_preserves_unchanged_mm_and_converts_edited_bare_number() {
        let (mut h, o1, _) = two_part_harness();
        h.app.run("document.units", json!({"units": "in"})).unwrap();
        h.app.run("occurrence.move", json!({"occurrence": o1, "translate": ["25.4", "0", "0"], "capture": false})).unwrap();
        let cp_id = h.app.run("component.capture_position", json!({})).unwrap()["feature"].as_u64().unwrap();

        let mut d = crate::dialogs::for_feature(&h.app, cp_id, None).unwrap();
        apply(&mut h, &d);
        let b1 = h.app.session.world_state().unwrap().body("BoxA").unwrap().mesh().bounds();
        assert!((b1.min.x - 25.4).abs() < 1e-4 && b1.min.y.abs() < 1e-4, "{b1:?}");

        if let Kind::CapturePosition(c) = &mut d.kind {
            c.moves[0].y = "2".into();
        }
        apply(&mut h, &d);
        let b2 = h.app.session.world_state().unwrap().body("BoxA").unwrap().mesh().bounds();
        assert!((b2.min.x - 25.4).abs() < 1e-4 && (b2.min.y - 50.8).abs() < 1e-4, "{b2:?}");
    }

    #[test]
    fn cancel_restores_marker_and_empty_capture_returns_none() {
        let (mut h, o1, _) = two_part_harness();
        assert!(for_feature(&h.app.session.doc, &[], &[]).is_none());

        h.app.run("occurrence.move", json!({"occurrence": o1, "translate": ["10", "0", "0"], "capture": false})).unwrap();
        let cp_id = h.app.run("component.capture_position", json!({})).unwrap()["feature"].as_u64().unwrap();
        h.app.run("sketch.create", json!({"plane": "XY"})).unwrap();
        h.app.run("sketch.finish", json!({})).unwrap();
        assert!(h.app.session.doc.marker.is_none());

        h.app.edit_feature(cp_id);
        assert!(matches!(h.app.dialog.as_ref().map(|d| &d.kind), Some(Kind::CapturePosition(_))));
        assert!(h.app.session.doc.marker.is_some());
        crate::dialogs::cancel(&mut h.app);
        assert!(h.app.dialog.is_none());
        assert!(h.app.session.doc.marker.is_none());
    }

    #[test]
    fn capture_position_renders_rows_via_ui_kit_without_panics() {
        let (mut h, o1, o2) = two_part_harness();
        h.app
            .run("occurrence.move", json!({"occurrence": o1, "origin": ["1", "2", "3"], "axis": [0, 0, 1], "angle": "15 deg", "capture": false}))
            .unwrap();
        h.app
            .run(
                "occurrence.move",
                json!({"occurrence": o2, "matrix": [[0.0, -1.0, 0.0, 5.0], [1.0, 0.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0]], "capture": false}),
            )
            .unwrap();
        let cp_id = h.app.run("component.capture_position", json!({})).unwrap()["feature"].as_u64().unwrap();

        h.app.edit_feature(cp_id);
        h.frames(3);
        assert!(crate::scenario::handle_at("dialog:X Distance").is_some());
        assert!(crate::scenario::handle_at("dialog:Y Distance").is_some());
        assert!(crate::scenario::handle_at("dialog:Z Distance").is_some());
        assert!(crate::scenario::handle_at("dialog:Angle").is_some());
    }

    #[test]
    fn duplicate_occurrence_names_across_distinct_ids_are_disambiguated() {
        let (mut h, o1, o2) = two_part_harness();
        for o in &mut h.app.session.doc_mut().occurrences {
            if o.id == o1 || o.id == o2 {
                o.name = "Bracket:1".into();
            }
        }
        let moves = vec![
            CapturedOccurrenceMove {
                occurrence: o1,
                space: MoveSpace::Local,
                translate: ["10 mm".into(), "0 mm".into(), "0 mm".into()],
                origin: None,
                rotate_axis: None,
                angle: None,
                rotation: None,
                path: None,
            },
            CapturedOccurrenceMove {
                occurrence: o2,
                space: MoveSpace::Local,
                translate: ["20 mm".into(), "0 mm".into(), "0 mm".into()],
                origin: None,
                rotate_axis: None,
                angle: None,
                rotation: None,
                path: None,
            },
        ];
        let (kind, _, _) = for_feature(&h.app.session.doc, &moves, &[]).unwrap();
        let Kind::CapturePosition(c) = kind else { panic!("expected Kind::CapturePosition") };
        assert_eq!(c.moves[0].label, "Bracket:1 #1");
        assert_eq!(c.moves[1].label, "Bracket:1 #2");
    }

    #[test]
    fn inline_param_definition_in_capture_move_creates_param_while_joint_dof_refuses() {
        let (mut h, o1, o2) = two_part_harness();
        h.app.run("occurrence.ground", json!({"occurrence": o1, "grounded": true})).unwrap();
        h.app.run("occurrence.move", json!({"occurrence": o2, "translate": [20, 0, 0]})).unwrap();
        h.app
            .run(
                "joint.create",
                json!({
                    "type": "revolute",
                    "name": "Hinge",
                    "a": {"face": [10, 5, 5]},
                    "b": {"face": [20, 5, 5]}
                }),
            )
            .unwrap();
        let o3 = h.app.run("component.create", json!({"name": "PartC", "grounded": false})).unwrap()["occurrence"].as_u64().unwrap();
        h.app.run("solid.box", json!({"length": 10, "width": 10, "height": 10, "body_name": "BoxC"})).unwrap();
        h.app.run("component.activate", json!({"component": "root"})).unwrap();

        h.app.run("document.units", json!({"units": "in"})).unwrap();
        h.app.run("occurrence.move", json!({"occurrence": o3, "translate": [25.4, 0, 0], "capture": false})).unwrap();
        h.app.run("joint.drive", json!({"joint": "Hinge", "value": 45.0})).unwrap();
        let cp_id = h.app.run("component.capture_position", json!({})).unwrap()["feature"].as_u64().unwrap();

        // 1. Move expression field supports inline definition via timeline.redefine -> Document::define_inline.
        let mut d = crate::dialogs::for_feature(&h.app, cp_id, None).unwrap();
        if let Kind::CapturePosition(c) = &mut d.kind {
            c.moves[0].x = "new_gap = 2".into();
        } else {
            panic!("expected Kind::CapturePosition");
        }
        apply(&mut h, &d);
        let param = h.app.session.doc.param("new_gap").expect("CapturePosition move inline definition creates user parameter");
        assert_eq!((param.expr.as_str(), param.unit.as_str()), ("2 in", "in"));
        let FeatureKind::CapturePosition { moves, .. } = &h.app.session.doc.feature(cp_id).unwrap().kind else {
            panic!("expected CapturePosition");
        };
        assert_eq!(moves[0].translate[0], "new_gap");
        let b1 = h.app.session.world_state().unwrap().body("BoxC").unwrap().mesh().bounds();
        assert!((b1.min.x - 50.8).abs() < 1e-4, "{b1:?}");
        h.app.run("parameters.change", json!({"name": "new_gap", "expression": "3 in"})).unwrap();
        let b2 = h.app.session.world_state().unwrap().body("BoxC").unwrap().mesh().bounds();
        assert!((b2.min.x - 76.2).abs() < 1e-4, "{b2:?}");

        // 2. Joint DOF refuses inline definition cleanly because CapturedJoint is not scanned by define_inline.
        let mut d2 = crate::dialogs::for_feature(&h.app, cp_id, None).unwrap();
        if let Kind::CapturePosition(c) = &mut d2.kind {
            c.joints[0].dofs[0] = "j_ang = 30 deg".into();
        }
        let err = crate::dialogs::apply_commands(&h.app, &d2).unwrap_err();
        assert!(err.contains("Parameters") && err.contains("j_ang"), "{err}");
        assert!(h.app.session.doc.param("j_ang").is_none());
    }

    #[test]
    fn real_ui_combos_sections_autocomplete_errors_inline_defs_and_ja_labels() {
        let mut h = Harness::new();
        h.app.home.open = false;
        h.app.run("parameters.add", json!({"name": "dx_param", "expression": "10 mm"})).unwrap();
        h.app.run("parameters.add", json!({"name": "ox", "expression": "4 mm"})).unwrap();
        h.app.run("parameters.add", json!({"name": "theta", "expression": "45 deg"})).unwrap();

        let (p_comp, p_occ1) = {
            let c = h.app.run("component.create", json!({"name": "Parent", "grounded": true})).unwrap();
            (c["component"].as_u64().unwrap(), c["occurrence"].as_u64().unwrap())
        };
        h.app.run("solid.box", json!({"length": 10, "width": 10, "height": 10, "body_name": "BaseBox"})).unwrap();
        h.app.run("occurrence.move", json!({"occurrence": p_occ1, "axis": [0, 0, 1], "angle": "90 deg", "move_grounded": true})).unwrap();
        let c_occ =
            h.app.run("component.create", json!({"name": "Child", "parent": p_comp, "grounded": false})).unwrap()["occurrence"].as_u64().unwrap();
        h.app.run("solid.box", json!({"length": 4, "width": 4, "height": 4, "body_name": "ChildBox"})).unwrap();
        h.app.run("component.activate", json!({"component": "root"})).unwrap();
        let p_occ2 = h.app.run("occurrence.copy", json!({"component": p_comp, "translate": [0, 40, 0]})).unwrap()["occurrence"].as_u64().unwrap();

        let o_free = h.app.run("component.create", json!({"name": "Free", "grounded": false})).unwrap()["occurrence"].as_u64().unwrap();
        h.app.run("solid.box", json!({"length": 4, "width": 4, "height": 4, "corner": [20, 0, 0], "body_name": "FreeBox"})).unwrap();
        h.app.run("component.activate", json!({"component": "root"})).unwrap();

        let o_cyl = h.app.run("component.create", json!({"name": "CylPart", "grounded": false})).unwrap()["occurrence"].as_u64().unwrap();
        h.app.run("solid.box", json!({"length": 4, "width": 4, "height": 4, "corner": [40, 0, 0], "body_name": "CylBox"})).unwrap();
        h.app.run("component.activate", json!({"component": "root"})).unwrap();
        h.app.run("joint.create", json!({"type": "cylindrical", "name": "CylJoint", "a": {"face": [10, 5, 5]}, "b": {"face": [40, 2, 2]}})).unwrap();
        let _ = (p_occ2, o_cyl);

        // Drive Cylindrical joint and move Child (via Move UI on rotated multi-instance parent) + Free (with origin & angle).
        crate::dialogs_move::open_on_occurrence(&mut h.app, vec![p_occ1, c_occ], false);
        h.frames(2);
        h.step(&json!({"click": {"handle": "dialog:Capture All Positions"}})).unwrap();
        h.step(&json!({"click": {"handle": "dialog:X Distance"}})).unwrap();
        h.step(&json!({"key": "A", "cmd": true})).unwrap();
        h.step(&json!({"text": "dx_param"})).unwrap();
        h.step(&json!({"click": {"handle": "dialog:OK"}})).unwrap();
        h.frames(2);

        h.app
            .run(
                "occurrence.move",
                json!({"occurrence": o_free, "origin": ["ox", 0, 0], "axis": [0, 0, 1], "angle": "theta", "translate": [0, 0, 0], "capture": false}),
            )
            .unwrap();
        h.app.run("joint.drive", json!({"joint": "CylJoint", "values": [30, 12]})).unwrap();
        let cp_id = h.app.run("component.capture_position", json!({})).unwrap()["feature"].as_u64().unwrap();

        // Open CapturePosition dialog and test handles, combos, sections, autocomplete, error banner, and space switch.
        h.app.edit_feature(cp_id);
        h.frames(3);
        assert!(crate::scenario::handle_at("dialog:X Distance").is_some());
        // Switch Component combo to Free:1 (which has Angle + Origin).
        h.step(&json!({"click": {"handle": "dialog:Component"}})).unwrap();
        h.frames(2);
        h.step(&json!({"click": {"handle": "dialog:Component:Free:1"}})).unwrap();
        h.frames(2);
        // Open Origin and Joints sections; verify disambiguated Angle vs joint:Angle handles and Cylindrical Distance handle.
        h.step(&json!({"click": {"handle": "dialog:section:Origin"}})).unwrap();
        h.step(&json!({"click": {"handle": "dialog:section:Joints"}})).unwrap();
        h.frames(2);
        assert!(crate::scenario::handle_at("dialog:Origin X").is_some());
        assert!(crate::scenario::handle_at("dialog:Origin Y").is_some());
        assert!(crate::scenario::handle_at("dialog:Origin Z").is_some());
        assert!(crate::scenario::handle_at("dialog:Angle").is_some());
        assert!(crate::scenario::handle_at("dialog:joint:Angle").is_some());
        assert!(crate::scenario::handle_at("dialog:Distance").is_some());

        // Edit Joint Angle via disambiguated handle without touching Move Angle.
        h.step(&json!({"click": {"handle": "dialog:joint:Angle"}})).unwrap();
        h.step(&json!({"key": "A", "cmd": true})).unwrap();
        h.step(&json!({"text": "60 deg"})).unwrap();
        // Edit Origin X with inline definition + Angle with inline definition.
        h.step(&json!({"click": {"handle": "dialog:Origin X"}})).unwrap();
        h.step(&json!({"key": "A", "cmd": true})).unwrap();
        h.step(&json!({"text": "ox_inline = 12"})).unwrap();
        h.step(&json!({"click": {"handle": "dialog:Angle"}})).unwrap();
        h.step(&json!({"key": "A", "cmd": true})).unwrap();
        h.step(&json!({"text": "ang_inline = 60 deg"})).unwrap();

        // Test error banner on invalid expression, then fix with autocomplete Tab on Child:1 X Distance.
        h.step(&json!({"click": {"handle": "dialog:Component"}})).unwrap();
        h.frames(2);
        h.step(&json!({"click": {"handle": "dialog:Component:Child:1"}})).unwrap();
        h.frames(2);
        h.step(&json!({"click": {"handle": "dialog:Space"}})).unwrap();
        h.frames(2);
        h.step(&json!({"click": {"handle": "dialog:Space:ui.capture_position.local"}})).unwrap();
        h.frames(2);

        h.step(&json!({"click": {"handle": "dialog:X Distance"}})).unwrap();
        h.step(&json!({"key": "A", "cmd": true})).unwrap();
        h.step(&json!({"text": "no_such_param"})).unwrap();
        h.step(&json!({"click": {"handle": "dialog:OK"}})).unwrap();
        h.frames(2);
        assert!(h.app.dialog.is_some(), "invalid expr keeps CapturePosition dialog open with error banner");

        h.step(&json!({"click": {"handle": "dialog:X Distance"}})).unwrap();
        h.step(&json!({"key": "A", "cmd": true})).unwrap();
        h.step(&json!({"text": "dx_"})).unwrap();
        h.frames(2);
        assert!(crate::scenario::rect_of("dialog:suggestions").is_some(), "autocomplete popup shown for dx_");
        h.step(&json!({"key": "Tab"})).unwrap();
        h.step(&json!({"click": {"handle": "dialog:OK"}})).unwrap();
        h.frames(2);
        assert!(h.app.dialog.is_none());
        let FeatureKind::CapturePosition { moves, .. } = &h.app.session.doc.feature(cp_id).unwrap().kind else {
            panic!("expected CapturePosition");
        };
        assert_eq!(moves[0].space, MoveSpace::Local, "switching Space in UI must persist Local on CapturePosition");
        assert!(h.app.session.doc.param("ox_inline").is_some());
        assert!(h.app.session.doc.param("ang_inline").is_some());

        // Rename parameters referenced by Origin and Angle, verify references update.
        let r1 = h.app.run("parameters.rename", json!({"name": "ox_inline", "new_name": "ox2"})).unwrap();
        let r2 = h.app.run("parameters.rename", json!({"name": "ang_inline", "new_name": "ang2"})).unwrap();
        assert!(r1["references_updated"].as_u64().unwrap() >= 1);
        assert!(r2["references_updated"].as_u64().unwrap() >= 1);

        // Inch-unit joint edit via UI dialog.
        h.app.run("document.units", json!({"units": "in"})).unwrap();
        let mut d_in = crate::dialogs::for_feature(&h.app, cp_id, None).unwrap();
        if let Kind::CapturePosition(c) = &mut d_in.kind {
            c.joints[0].dofs[1] = "2".into();
        }
        apply(&mut h, &d_in);
        let FeatureKind::CapturePosition { joints, .. } = &h.app.session.doc.feature(cp_id).unwrap().kind else {
            panic!("expected CapturePosition");
        };
        assert!((joints[0].values[1] - 50.8).abs() < 1e-4, "{:?}", joints[0].values);

        // Japanese UI label verification (Bug 5): verify translated Origin X/Y/Z, Origin, Axis, Joints, Joint, Angle, Distance, Slide, Turn.
        h.app.preferences.language = "ja".into();
        h.app.edit_feature(cp_id);
        h.frames(3);
        let ja_loc = solidcraft_ui_text::Locale::for_language("ja");
        let origin_section_ja = format!("dialog:section:{}", ja_loc.text("ui.origin"));
        let joints_section_ja = format!("dialog:section:{}", ja_loc.text("ui.joints"));
        let comp_ja = format!("dialog:{}", ja_loc.text("render.pick_component"));
        h.step(&json!({"click": {"handle": comp_ja}})).unwrap();
        h.frames(2);
        h.step(&json!({"click": {"handle": format!("dialog:{}:Free:1", ja_loc.text("render.pick_component"))}})).unwrap();
        h.frames(2);
        if crate::scenario::handle_at(&format!("dialog:{}", ja_loc.text("ui.capture_position.origin_x"))).is_none()
            && crate::scenario::handle_at(&origin_section_ja).is_some()
        {
            h.step(&json!({"click": {"handle": origin_section_ja}})).unwrap();
        }
        if crate::scenario::handle_at("dialog:joint:Angle").is_none() && crate::scenario::handle_at(&joints_section_ja).is_some() {
            h.step(&json!({"click": {"handle": joints_section_ja}})).unwrap();
        }
        h.frames(2);
        let output = egui::FullOutput { shapes: h.shapes.clone(), ..Default::default() };
        let texts: Vec<String> = crate::test_support::text_shapes(&output).map(|t| t.galley.text().to_string()).collect();
        for key in [
            "ui.capture_position.origin_x",
            "ui.capture_position.origin_y",
            "ui.capture_position.origin_z",
            "ui.origin",
            "ui.axis",
            "ui.joints",
            "cmd.joint_create",
            "ui.angle",
            "ui.distance",
        ] {
            let expected = ja_loc.text(key);
            assert!(texts.iter().any(|t| t.contains(expected)), "ja UI missing `{key}` ({expected}): {texts:?}");
        }
        for literal in ["Origin X", "Origin Y", "Origin Z", "Origin", "Axis", "Joints", "Joint", "Component"] {
            assert!(!texts.iter().any(|t| t == literal), "untranslated English `{literal}` rendered in ja UI: {texts:?}");
        }
        assert_eq!(ja_loc.text("ui.capture_position.slide"), "スライド");
        assert_eq!(ja_loc.text("ui.capture_position.turn"), "回転");
    }

    #[test]
    fn non_english_angle_ball_dof_and_joint_choice_labels() {
        let (mut h, o1, o2) = two_part_harness();
        h.app.home.open = false;
        h.app.run("occurrence.ground", json!({"occurrence": o1, "grounded": true})).unwrap();
        h.app.run("occurrence.move", json!({"occurrence": o2, "translate": [20, 0, 0]})).unwrap();
        let o3 = h.app.run("component.create", json!({"name": "PartC", "grounded": false})).unwrap()["occurrence"].as_u64().unwrap();
        h.app.run("solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [40, 0, 0], "body_name": "BoxC"})).unwrap();
        h.app.run("component.activate", json!({"component": "root"})).unwrap();
        let _ = o3;

        h.app.run("joint.create", json!({"type": "angle", "name": "AngJ", "a": {"face": [10, 5, 5]}, "b": {"face": [20, 5, 5]}})).unwrap();
        h.app.run("joint.create", json!({"type": "ball", "name": "BallJ", "a": {"face": [10, 5, 5]}, "b": {"face": [40, 5, 5]}})).unwrap();
        h.app.run("joint.drive", json!({"joint": "AngJ", "values": [1, 2, 3, 15, 30]})).unwrap();
        h.app.run("joint.drive", json!({"joint": "BallJ", "values": [10, 20, 30]})).unwrap();
        let cp_id = h.app.run("component.capture_position", json!({})).unwrap()["feature"].as_u64().unwrap();

        for lang in ["ja", "de", "zh"] {
            h.app.preferences.language = lang.into();
            h.frames(1);
            let loc = solidcraft_ui_text::Locale::for_language(lang);
            let joints_label = loc.text("cmd.joint_create");
            let ang_choice = format!("AngJ ({})", loc.label("Angle"));
            let ball_choice = format!("BallJ ({})", loc.label("Ball"));

            h.app.edit_feature(cp_id);
            h.frames(3);
            let output_ang = egui::FullOutput { shapes: h.shapes.clone(), ..Default::default() };
            let texts_ang: Vec<String> = crate::test_support::text_shapes(&output_ang).map(|t| t.galley.text().to_string()).collect();

            assert!(texts_ang.iter().any(|t| t == &ang_choice), "{lang} missing localized `{ang_choice}`: {texts_ang:?}");
            assert!(!texts_ang.iter().any(|t| t == "AngJ (Angle)"), "{lang} leaked untranslated `AngJ (Angle)`: {texts_ang:?}");
            assert!(texts_ang.iter().any(|t| t == loc.text("ui.capture_position.spin")), "{lang} missing ui.capture_position.spin: {texts_ang:?}");
            assert!(
                !texts_ang.iter().any(|t| t == loc.text("navbar.spin")),
                "{lang} leaked camera Continuous Orbit navbar.spin ({}) for joint spin: {texts_ang:?}",
                loc.text("navbar.spin")
            );

            // Switch joint combo to BallJ and check pitch & localized Ball choice.
            h.step(&json!({"click": {"handle": format!("dialog:{joints_label}")}})).unwrap();
            h.frames(2);
            h.step(&json!({"click": {"handle": format!("dialog:{joints_label}:{ball_choice}")}})).unwrap();
            h.frames(2);
            let output_ball = egui::FullOutput { shapes: h.shapes.clone(), ..Default::default() };
            let texts_ball: Vec<String> = crate::test_support::text_shapes(&output_ball).map(|t| t.galley.text().to_string()).collect();

            assert!(texts_ball.iter().any(|t| t == &ball_choice), "{lang} missing localized `{ball_choice}`: {texts_ball:?}");
            assert!(!texts_ball.iter().any(|t| t == "BallJ (Ball)"), "{lang} leaked untranslated `BallJ (Ball)`: {texts_ball:?}");
            assert!(texts_ball.iter().any(|t| t == loc.text("ui.view_angles_pitch")), "{lang} missing ui.view_angles_pitch: {texts_ball:?}");
            if loc.text("Pitch") != loc.text("ui.view_angles_pitch") {
                assert!(
                    !texts_ball.iter().any(|t| t == loc.text("Pitch")),
                    "{lang} leaked screw thread pitch ({}) for Ball pitch DOF: {texts_ball:?}",
                    loc.text("Pitch")
                );
            }

            crate::dialogs::cancel(&mut h.app);
            h.frames(1);
        }
    }
}
