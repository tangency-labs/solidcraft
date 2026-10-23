//! Delete for everything that can be selected, in one undo step: sketch entities (while
//! sketching), timeline features and sketches (with the features that depend on them),
//! construction planes, bodies (a Remove feature from here on in the timeline), occurrences and
//! components, canvases and joints. `dry_run` reports what would go and what would fail without
//! changing anything, so the UI can ask first.

use serde_json::{Value, json};
use solidcraft_doc::FeatureKind;
use solidcraft_geom::Vec3;

use super::CommandSpec;
use crate::params::{bad, bool_};
use crate::{EngineError, Result, Sel, Session};

pub static COMMANDS: &[CommandSpec] = &[CommandSpec::new("selection.delete", "Delete", delete).icon("delete").params(
    "items?: [selection items]; components?, occurrences?, canvases?, joints?: [ids or names]; dry_run?: bool → deleted, deleted_joints, would_fail, skipped (one undo step)",
)];

/// Run another command's body inside this one (so the whole delete is one undo step).
fn sub(s: &mut Session, id: &str, p: Value) -> Result<Value> {
    let spec = super::find_command(id).ok_or_else(|| EngineError::UnknownCommand(id.to_string()))?;
    let r = spec.run_nested(s, &p)?;
    s.refresh();
    Ok(r)
}

fn keys(p: &Value, k: &str) -> Vec<String> {
    match p.get(k) {
        Some(Value::Array(a)) => {
            a.iter().take(10_000).filter_map(|v| v.as_str().map(str::to_string).or_else(|| v.as_u64().map(|n| n.to_string()))).collect()
        }
        _ => Vec::new(),
    }
}

fn delete(s: &mut Session, p: &Value) -> Result<Value> {
    if bool_(p, "dry_run").unwrap_or(false) {
        let mut t = s.scratch();
        return apply(&mut t, p);
    }
    let r = apply(s, p)?;
    s.selection.clear();
    Ok(r)
}

fn apply(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "selection.delete";
    let items: Vec<Sel> = match p.get("items") {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Array(a)) if a.len() <= 10_000 => {
            a.iter().map(|v| serde_json::from_value(v.clone()).map_err(|e| bad(cmd, format!("bad item {v}: {e}")))).collect::<Result<_>>()?
        }
        Some(_) => return Err(bad(cmd, "`items` must be a list of selection items")),
    };
    if let Some(value) = p.get("joints").filter(|v| !v.is_null()) {
        let Some(joints) = value.as_array() else {
            return Err(bad(cmd, "`joints` must be a list of ids or names"));
        };
        if joints.len() > 10_000 || joints.iter().any(|j| !j.is_string() && j.as_u64().is_none()) {
            return Err(bad(cmd, "`joints` must contain at most 10000 ids or names"));
        }
    }
    let before = s.model.clone();
    let mut joints = p
        .get("joints")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|value| super::joints::joint_id(s, Some(value), cmd))
        .collect::<Result<Vec<_>>>()?;
    joints.sort_unstable();
    joints.dedup();
    let features_before: Vec<(u64, String)> = s.doc.features.iter().map(|f| (f.id, f.name.clone())).collect();
    let mut skipped: Vec<String> = Vec::new();
    let mut skip = |why: String| {
        if !skipped.contains(&why) {
            skipped.push(why);
        }
    };
    let mut entities = Vec::new();
    let mut features: Vec<u64> = Vec::new();
    let mut bodies: Vec<String> = Vec::new();
    let mut faces: Vec<(String, Vec<Vec3>)> = Vec::new();
    let mut selected_occurrences = Vec::new();
    for x in &items {
        match x {
            Sel::Occurrence { path } => {
                selected_occurrences.push(s.occurrence_placement(path)?.occurrence.to_string());
            }
            Sel::SketchCurve { id } | Sel::SketchPoint { id } | Sel::SketchConstraint { id } => entities.push(id.clone()),
            Sel::Feature { id } => {
                if s.doc.feature(*id).is_none() {
                    return Err(bad(cmd, format!("no feature {id}")));
                }
                features.push(*id);
            }
            Sel::Plane { name } => match s.doc.find_feature(name) {
                Some(f) if matches!(f.kind, FeatureKind::ConstructionPlane { .. }) => features.push(f.id),
                _ => skip("the origin planes can't be deleted".into()),
            },
            Sel::Axis { name } => match s.doc.find_feature(name) {
                Some(f) if matches!(f.kind, FeatureKind::ConstructionAxis { .. }) => features.push(f.id),
                _ => skip("the origin axes can't be deleted".into()),
            },
            Sel::ConstructionPoint { name } => match s.doc.find_feature(name) {
                Some(f) if matches!(f.kind, FeatureKind::ConstructionPoint { .. }) => features.push(f.id),
                _ => skip(format!("no construction point named \"{name}\"")),
            },
            Sel::Body { name } => bodies.push(name.clone()),
            Sel::Face { body, point, .. } => match faces.iter_mut().find(|(b, _)| b == body) {
                Some((_, ps)) => ps.push(*point),
                None => faces.push((body.clone(), vec![*point])),
            },
            Sel::Edge { .. } | Sel::Vertex { .. } => skip("edges and vertices are deleted with their body or face".into()),
            Sel::Profile { .. } => skip("a profile goes with its sketch".into()),
            Sel::ExternalSketchCurve { .. } | Sel::ExternalSketchPoint { .. } => skip("edit the source sketch to delete its geometry".into()),
        }
    }
    features.dedup();
    let mut done = false;
    for id in &joints {
        sub(s, "joint.delete", json!({"joint": id}))?;
        done = true;
    }
    if !entities.is_empty() {
        if s.active_sketch.is_none() {
            skip("sketch entities are deleted while editing their sketch".into());
        } else {
            sub(s, "sketch.delete", json!({ "entities": entities }))?;
            done = true;
        }
    }
    // Faces of a body that is going anyway are left to it; the rest become Delete Face features.
    for (body, points) in faces.iter().filter(|(b, _)| !bodies.contains(b)) {
        sub(s, "face.delete", json!({ "faces": points, "body": body }))?;
        done = true;
    }
    if !features.is_empty() {
        let ids: Vec<String> = features.iter().map(u64::to_string).collect();
        sub(s, "timeline.delete", json!({ "features": ids }))?;
        done = true;
    }
    // Bodies still there (not made by a feature deleted above) get a Remove feature.
    let st = s.model.state();
    bodies.retain(|b| st.body(b).is_some());
    bodies.dedup();
    if !bodies.is_empty() {
        sub(s, "solid.remove", json!({ "bodies": bodies }))?;
        done = true;
    }
    // An occurrence goes alone while its component has others; the last one takes the component.
    let mut components: Vec<u64> = Vec::new();
    selected_occurrences.extend(keys(p, "occurrences"));
    selected_occurrences.sort();
    selected_occurrences.dedup();
    for k in selected_occurrences {
        let Some(o) = s.doc.occurrences.iter().find(|o| o.id.to_string() == k || o.name == k).cloned() else {
            return Err(bad(cmd, format!("no occurrence `{k}`")));
        };
        if s.doc.occurrences.iter().filter(|x| x.component == o.component).count() > 1 {
            s.doc_mut().occurrences.retain(|x| x.id != o.id);
            s.pending_moves.remove(&o.id);
            done = true;
        } else {
            components.push(o.component);
        }
    }
    for k in keys(p, "components") {
        let id = s.doc.find_component(&k).ok_or_else(|| bad(cmd, format!("no component `{k}`")))?;
        if id == 0 {
            return Err(bad(cmd, "the design itself can't be deleted"));
        }
        components.push(id);
    }
    components.dedup();
    for c in components {
        // A parent deleted first takes its subcomponents with it.
        if s.doc.components.iter().any(|x| x.id == c) {
            sub(s, "component.delete", json!({ "component": c }))?;
            done = true;
        }
    }
    for k in keys(p, "canvases") {
        sub(s, "canvas.delete", json!({ "canvas": k }))?;
        done = true;
    }
    if !done {
        return Err(bad(cmd, skipped.first().cloned().unwrap_or_else(|| "nothing to delete".into())));
    }
    s.refresh();
    let deleted: Vec<&String> = features_before.iter().filter(|(id, _)| s.doc.feature(*id).is_none()).map(|(_, n)| n).collect();
    let would_fail: Vec<String> = s
        .model
        .results
        .iter()
        .filter(|r| r.error.is_some() && before.result(r.id).is_some_and(|b| b.error.is_none() && !b.skipped))
        .map(|r| r.name.clone())
        .collect();
    Ok(json!({ "deleted": deleted, "deleted_joints": joints, "bodies": bodies, "would_fail": would_fail, "skipped": skipped }))
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use crate::Session;

    fn run(s: &mut Session, id: &str, p: Value) -> Value {
        match s.execute(id, &p) {
            Ok(v) => v,
            Err(e) => panic!("{id} {p}: {e}"),
        }
    }

    fn bodies(s: &Session) -> usize {
        s.model.state().bodies.len()
    }

    /// A sketch with a rectangle extruded into a body, a second box, and an offset plane.
    fn part() -> (Session, u64) {
        let mut s = Session::default();
        let sk = run(&mut s, "sketch.create", json!({"plane": "XY"}))["sketch"].as_u64().unwrap();
        run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [10, 20]}));
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "solid.extrude", json!({"distance": 5}));
        run(&mut s, "solid.box", json!({"length": 5, "width": 5, "height": 5, "corner": [50, 0, 0]}));
        run(&mut s, "construct.plane.offset", json!({"base": "XY", "offset": 10}));
        (s, sk)
    }

    #[test]
    fn a_sketch_goes_with_its_dependents_after_a_dry_run() {
        let (mut s, sk) = part();
        let n = s.doc.features.len();
        let dry = run(&mut s, "selection.delete", json!({"items": [{"type": "feature", "id": sk}], "dry_run": true}));
        assert_eq!(s.doc.features.len(), n, "a dry run changes nothing");
        assert!(dry["deleted"].as_array().unwrap().len() >= 2, "the extrude goes with its sketch: {dry}");
        run(&mut s, "selection.delete", json!({"items": [{"type": "feature", "id": sk}]}));
        assert_eq!(bodies(&s), 1);
        run(&mut s, "edit.undo", json!({}));
        assert_eq!(s.doc.features.len(), n);
        assert_eq!(bodies(&s), 2);
    }

    #[test]
    fn mixed_selection_is_one_undo_step() {
        let (mut s, _) = part();
        let body = s.model.state().bodies[1].name.clone();
        let depth = s.undo.len();
        let n = s.doc.features.len();
        let r = run(
            &mut s,
            "selection.delete",
            json!({"items": [{"type": "body", "name": body}, {"type": "plane", "name": "Plane1"}, {"type": "plane", "name": "XY"}]}),
        );
        assert_eq!(bodies(&s), 1, "{r}");
        assert!(s.doc.find_feature("Plane1").is_none());
        assert_eq!(r["skipped"][0], "the origin planes can't be deleted");
        assert_eq!(s.undo.len(), depth + 1);
        run(&mut s, "edit.undo", json!({}));
        assert_eq!((bodies(&s), s.doc.features.len()), (2, n));
    }

    #[test]
    fn faces_alone_are_refused_and_hostile_input_too() {
        let (mut s, _) = part();
        let b = s.model.state().bodies[0].name.clone();
        let e = s.execute("selection.delete", &json!({"items": [{"type": "face", "body": b, "index": 0, "point": [0, 0, 0]}]})).unwrap_err();
        // A side of the part its neighbours can't close: Delete Face refuses it.
        assert!(e.to_string().contains("Delete Face"), "{e}");
        for p in [
            json!({}),
            json!({"items": "x"}),
            json!({"items": [{"type": "nope"}]}),
            json!({"items": [{"type": "feature", "id": 999}]}),
            json!({"components": ["0"]}),
        ] {
            assert!(s.execute("selection.delete", &p).is_err(), "{p}");
        }
    }

    fn joint_assembly() -> Session {
        let mut s = Session::default();
        for (name, x) in [("A", 0), ("B", 30), ("C", 60)] {
            run(&mut s, "component.activate", json!({"component": "root"}));
            run(&mut s, "component.create", json!({"name": name}));
            run(&mut s, "solid.box", json!({"length": 20, "width": 20, "height": 10, "corner": [x, 0, 0]}));
        }
        run(&mut s, "component.activate", json!({"component": "root"}));
        run(&mut s, "occurrence.ground", json!({"occurrence": "A:1", "grounded": true}));
        for (name, other, face) in [("Hinge", "B:1", [40, 10, 0]), ("Glue", "C:1", [70, 10, 0])] {
            run(
                &mut s,
                "joint.create",
                json!({"type": "revolute", "name": name,
                "a": {"occurrence": "A:1", "face": [10, 10, 10]}, "b": {"occurrence": other, "face": face}}),
            );
        }
        run(&mut s, "joint.motion_link", json!({"a": "Hinge", "b": "Glue", "ratio": 1}));
        s
    }

    #[test]
    fn selected_joints_and_their_link_are_deleted_in_one_undo_step_with_a_read_only_dry_run() {
        let mut s = joint_assembly();
        let features = serde_json::to_value(&s.doc.features).unwrap();
        let hinge = s.doc.assembly.joints[0].id;
        let dry = run(&mut s, "selection.delete", json!({"joints": [hinge, "Hinge", "Glue"], "dry_run": true}));
        assert_eq!(dry["deleted_joints"].as_array().unwrap().len(), 2);
        assert_eq!(s.doc.assembly.joints.len(), 2);
        assert_eq!(s.doc.assembly.links.len(), 1);
        assert_eq!(serde_json::to_value(&s.doc.features).unwrap(), features);
        run(&mut s, "selection.delete", json!({"joints": [hinge, "Hinge", "Glue"]}));
        assert!(s.doc.assembly.joints.is_empty() && s.doc.assembly.links.is_empty());
        assert_eq!(serde_json::to_value(&s.doc.features).unwrap(), features);
        run(&mut s, "edit.undo", json!({}));
        assert_eq!(s.doc.assembly.joints.len(), 2);
        assert_eq!(s.doc.assembly.links.len(), 1);
        assert_eq!(serde_json::to_value(&s.doc.features).unwrap(), features);
        run(&mut s, "edit.redo", json!({}));
        assert!(s.doc.assembly.joints.is_empty() && s.doc.assembly.links.is_empty());
    }

    #[test]
    fn an_unknown_joint_refuses_the_whole_delete_without_losing_features_or_other_joints() {
        let mut s = joint_assembly();
        let before = serde_json::to_value(&s.doc.features).unwrap();
        let feature = s.doc.features[0].id;
        assert!(s.execute("selection.delete", &json!({"items": [{"type": "feature", "id": feature}], "joints": ["Hinge", "missing"]})).is_err());
        for joints in [json!("Hinge"), json!([null]), json!([-1]), json!([1.5]), json!(vec!["Hinge"; 10_001])] {
            assert!(s.execute("selection.delete", &json!({"joints": joints})).is_err());
        }
        assert_eq!(serde_json::to_value(&s.doc.features).unwrap(), before);
        assert_eq!(s.doc.assembly.joints.len(), 2);
        assert_eq!(s.doc.assembly.links.len(), 1);
    }

    #[test]
    fn numeric_joint_ids_do_not_delete_a_different_joint_with_that_numeric_name() {
        for batch in [false, true] {
            let mut s = joint_assembly();
            let hinge = s.doc.assembly.joints[0].id;
            let glue = s.doc.assembly.joints[1].id;
            run(&mut s, "joint.edit", json!({"joint": hinge, "name": glue.to_string()}));
            let features = serde_json::to_value(&s.doc.features).unwrap();
            let (command, params) = if batch { ("selection.delete", json!({"joints": [glue]})) } else { ("joint.delete", json!({"joint": glue})) };
            run(&mut s, command, params);
            assert_eq!(s.doc.assembly.joints.len(), 1);
            assert_eq!(s.doc.assembly.joints[0].id, hinge, "a numeric id must not match another joint's name");
            assert_eq!(serde_json::to_value(&s.doc.features).unwrap(), features);
            run(&mut s, "edit.undo", json!({}));
            assert_eq!(s.doc.assembly.joints.len(), 2);
        }
    }

    #[test]
    fn occurrences_components_and_sketch_entities() {
        let (mut s, _) = part();
        let b = s.model.state().bodies[1].name.clone();
        run(&mut s, "component.from_bodies", json!({"bodies": [b]}));
        let c = s.doc.components[0].id;
        let o1 = s.doc.occurrence_of(c).unwrap().id;
        run(&mut s, "occurrence.copy", json!({"component": c, "translate": [0, 30, 0]}));
        assert_eq!(s.doc.occurrences.len(), 2);
        // One instance goes alone; the last one takes its component.
        run(&mut s, "selection.delete", json!({"occurrences": [o1]}));
        assert_eq!((s.doc.occurrences.len(), s.doc.components.len()), (1, 1));
        let o2 = s.doc.occurrences[0].id;
        run(&mut s, "selection.delete", json!({"occurrences": [o2]}));
        assert!(s.doc.components.is_empty());
        // Sketch entities while sketching.
        run(&mut s, "sketch.create", json!({"plane": "XY"}));
        run(&mut s, "sketch.line", json!({"points": [[0, 0], [10, 0]]}));
        let sk = s.active_sketch.unwrap();
        let id = s.model.state().sketch(sk).unwrap().sketch.curves[0].id.clone();
        run(&mut s, "selection.delete", json!({"items": [{"type": "sketch_curve", "id": id}]}));
        assert!(s.model.state().sketch(sk).unwrap().sketch.curves.is_empty());
    }
}
