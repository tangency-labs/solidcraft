//! Delete for everything that can be selected, in one undo step: sketch entities (while
//! sketching), timeline features and sketches (with the features that depend on them),
//! construction planes and axes, bodies (a Remove feature from here on in the timeline), occurrences and
//! components, canvases, point clouds and joints. `dry_run` reports what would go and what would fail without
//! changing anything, so the UI can ask first.

use serde_json::{Value, json};
use solidcraft_doc::FeatureKind;
use solidcraft_geom::Vec3;

use super::{CommandSpec, sketch_store};
use crate::params::{bad, bool_};
use crate::{EngineError, Result, Sel, Session};

pub static COMMANDS: &[CommandSpec] = &[CommandSpec::new("selection.delete", "Delete", delete).icon("delete").params(
    "items?: [selection items]; components?, occurrences?, canvases?, point_clouds?, joints?: [ids or names]; dry_run?: bool → deleted, deleted_joints, would_fail, skipped (one undo step)",
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
        // What the check computes is kept where this session keeps it (a check run ahead, off a
        // front end's window thread), so the delete itself finds it there (#9917).
        t.model.keep(s.model.kept());
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
    let before_doc = s.doc.clone();
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
            Sel::SketchCurve { id, .. } | Sel::SketchPoint { id } | Sel::SketchConstraint { id } => entities.push(id.clone()),
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
            Sel::SketchMidpoint { .. } => skip("a midpoint belongs to its sketch edge".into()),
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
            // In the sketch being edited, a profile stands for the curves around it (#7107);
            // its holes are shapes of their own and stay. A letter's outline stands for its
            // whole text, so no counters are left behind.
            Sel::Profile { sketch, index } if s.active_sketch == Some(*sketch) => {
                let st = s.model.state();
                let (Some(ss), Ok(sk)) = (st.sketch(*sketch), sketch_store::read(&s.doc, *sketch)) else {
                    skip("nothing around that profile can be deleted".into());
                    continue;
                };
                let outer = ss.profiles.get(*index).map(|pr| solidcraft_sketch::distinct_curves(&pr.outer_curves)).unwrap_or_default();
                let before = entities.len();
                for c in outer.iter().filter_map(|id| sk.curves.iter().find(|c| &c.id == id)) {
                    let text = c.link.as_ref().filter(|l| sk.link(l).is_some_and(|l| l.kind == solidcraft_sketch::LinkKind::Text));
                    let id = text.unwrap_or(&c.id);
                    if !entities.contains(id) {
                        entities.push(id.clone());
                    }
                }
                if entities.len() == before {
                    skip("nothing around that profile can be deleted".into());
                }
            }
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
        sub(s, "timeline.delete", json!({ "features": features }))?;
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
            // The joints on it and the other assembly records naming it go with it: left behind,
            // a joint stays in the Browser joined to nothing (#7749).
            let gone = s.doc_mut().remove_occurrences(&std::collections::BTreeSet::from([o.id]));
            s.visibility.hidden_joints.retain(|h| !gone.contains(h));
            joints.extend(gone);
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
    for k in keys(p, "point_clouds") {
        sub(s, "pointcloud.delete", json!({ "cloud": k }))?;
        done = true;
    }
    if !done {
        return Err(bad(cmd, skipped.first().cloned().unwrap_or_else(|| "nothing to delete".into())));
    }
    s.refresh();
    let deleted: Vec<&String> = features_before.iter().filter(|(id, _)| s.doc.feature(*id).is_none()).map(|(_, n)| n).collect();
    // Features after a rolled-back marker count too (#3471).
    let would_fail: Vec<String> =
        super::edit::newly_broken_in_history(&before_doc, &before, &s.doc, &s.model).into_iter().map(|(name, _)| name).collect();
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

    /// With the history rolled back, the checks before a delete look past the marker too (#3471):
    /// deleting the plate (or its extrude) there breaks the hole cut after the marker, and the
    /// confirmation must say so instead of the hole's error mark turning up only at the end.
    #[test]
    fn the_checks_before_a_delete_in_a_rolled_back_history_see_the_features_after_the_marker() {
        let mut s = Session::default();
        run(&mut s, "sketch.create", json!({"plane": "XY"}));
        run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [40, 20]}));
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "solid.extrude", json!({"distance": 5, "name": "Plate"}));
        run(&mut s, "solid.cylinder", json!({"base": [20, 10, -1], "radius": 2, "height": 10, "operation": "cut", "name": "Hole"}));
        let body = s.model.state().bodies[0].name.clone();
        let plate = s.doc.find_feature("Plate").unwrap().id;
        for marker in [None, Some(2)] {
            run(&mut s, "timeline.roll_to", marker.map_or(json!({}), |m| json!({"position": m})));
            let doc = s.doc.clone();
            let dry = run(&mut s, "selection.delete", json!({"items": [{"type": "body", "name": body}], "dry_run": true}));
            // At the end the plate's Remove goes after the hole; rolled back, before it.
            let breaks = if marker.is_some() { json!(["Hole"]) } else { json!([]) };
            assert_eq!(dry["would_fail"], breaks, "marker {marker:?}: {dry}");
            let deps = run(&mut s, "timeline.dependents", json!({"feature": plate}));
            assert_eq!(deps["would_fail"], json!(["Hole"]), "marker {marker:?}: {deps}");
            assert_eq!(*s.doc, *doc, "the checks change nothing");
            assert!(s.model.result(s.doc.find_feature("Hole").unwrap().id).is_some_and(|r| r.skipped == marker.is_some() && r.error.is_none()));
        }
        // Deleting anyway (rolled back) still reports what it broke.
        let done = run(&mut s, "selection.delete", json!({"items": [{"type": "body", "name": body}]}));
        assert_eq!(done["would_fail"], json!(["Hole"]), "{done}");
    }

    /// Deleting a dimension of a sketch with a rolled-back history after it (#7253): the sketch
    /// keeps its geometry, so the check before the delete does not build the features after the
    /// marker again (that took minutes on a long history). A deleted line still breaks them, and
    /// the check still says so.
    #[test]
    fn deleting_a_dimension_in_a_rolled_back_history_builds_nothing_after_the_marker() {
        let mut s = Session::default();
        let sk = run(&mut s, "sketch.create", json!({"plane": "XY"}))["sketch"].as_u64().unwrap();
        run(&mut s, "sketch.line", json!({"points": [[0, 0], [40, 0], [30, 20], [10, 20]], "closed": true}));
        let lines: Vec<String> = s.model.state().sketch(sk).unwrap().sketch.curves.iter().map(|c| c.id.clone()).collect();
        let param = run(&mut s, "sketch.dimension", json!({"entities": [lines[0], lines[1]], "type": "angle"}))["param"].as_str().map(str::to_string);
        let angle = s.model.state().sketch(sk).unwrap().sketch.constraints.iter().find(|c| param.is_some() && c.param == param).unwrap().id.clone();
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "solid.extrude", json!({"distance": 5, "name": "Plate"}));
        let later = 6;
        for i in 0..later {
            run(&mut s, "solid.cylinder", json!({"base": [12 + 3 * i, 8, -1], "radius": 1, "height": 10, "operation": "cut"}));
        }
        run(&mut s, "timeline.roll_to", json!({"position": 1}));
        run(&mut s, "sketch.edit", json!({"sketch": sk}));
        let cached = |s: &Session| s.model.cache.lock().map(|c| c.len()).unwrap_or(0);
        let before = cached(&s);
        let doc = s.doc.clone();
        let items = json!([{"type": "sketch_constraint", "id": angle}]);
        let dry = run(&mut s, "selection.delete", json!({"items": items, "dry_run": true}));
        assert_eq!((&dry["deleted"], &dry["would_fail"]), (&json!([]), &json!([])), "{dry}");
        assert_eq!(*s.doc, *doc, "the check changes nothing");
        // The sketch itself is solved again; Plate and the cuts are not built.
        let built = cached(&s).saturating_sub(before);
        assert!(built < later, "{built} results built for the check");
        let line = json!([{"type": "sketch_curve", "id": lines[1]}]);
        let dry = run(&mut s, "selection.delete", json!({"items": line, "dry_run": true}));
        assert!(dry["would_fail"].as_array().unwrap().contains(&json!("Plate")), "{dry}");
        run(&mut s, "selection.delete", json!({"items": items}));
        assert!(s.model.state().sketch(sk).unwrap().sketch.constraints.iter().all(|c| c.id != angle));
    }

    /// #9917: the check before a delete in a sketch with a rolled-back history after it builds the
    /// history before the delete only when a feature fails after it. Deleting a hole in the plate
    /// changes the plate and every cut after it and breaks none of them: the check reports
    /// nothing. Deleting a side of the plate's outline breaks the plate, and the check still says
    /// so, with the cache too small to keep a state (as on a large design).
    #[test]
    fn a_delete_in_a_rolled_back_history_reports_what_breaks_after_the_marker_and_nothing_else() {
        let mut s = Session::default();
        let sk = run(&mut s, "sketch.create", json!({"plane": "XY"}))["sketch"].as_u64().unwrap();
        let outline = run(&mut s, "sketch.line", json!({"points": [[0, 0], [40, 0], [30, 20], [10, 20]], "closed": true}))["curves"][0]
            .as_str()
            .unwrap()
            .to_string();
        let hole = run(&mut s, "sketch.circle.center", json!({"center": [30, 15], "radius": 2}))["curves"][0].as_str().unwrap().to_string();
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "solid.extrude", json!({"distance": 5, "name": "Plate"}));
        for i in 0..6 {
            run(&mut s, "solid.cylinder", json!({"base": [12 + 3 * i, 8, -1], "radius": 1, "height": 10, "operation": "cut"}));
        }
        run(&mut s, "timeline.roll_to", json!({"position": 1}));
        run(&mut s, "sketch.edit", json!({"sketch": sk}));
        *s.model.cache.lock().unwrap() = solidcraft_doc::EvalCache::with_budget(1);
        let doc = s.doc.clone();
        let dry = run(&mut s, "selection.delete", json!({"items": [{"type": "sketch_curve", "id": hole}], "dry_run": true}));
        assert_eq!(dry["would_fail"], json!([]), "{dry}");
        let dry = run(&mut s, "selection.delete", json!({"items": [{"type": "sketch_curve", "id": outline}], "dry_run": true}));
        assert!(dry["would_fail"].as_array().unwrap().contains(&json!("Plate")), "{dry}");
        assert_eq!(*s.doc, *doc, "the checks change nothing");
    }

    /// #9917: a delete checked ahead (off a front end's window thread) keeps what its dry run
    /// builds of the history after the marker, and the delete run with that finds all of it:
    /// nothing is built again on the window thread, where it took half a minute.
    #[test]
    fn a_delete_checked_ahead_keeps_the_history_it_builds_for_the_delete() {
        let mut s = Session::default();
        let sk = run(&mut s, "sketch.create", json!({"plane": "XY"}))["sketch"].as_u64().unwrap();
        run(&mut s, "sketch.line", json!({"points": [[0, 0], [40, 0], [30, 20], [10, 20]], "closed": true}));
        let hole = run(&mut s, "sketch.circle.center", json!({"center": [30, 15], "radius": 2}))["curves"][0].as_str().unwrap().to_string();
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "solid.extrude", json!({"distance": 5, "name": "Plate"}));
        let later = 6;
        for i in 0..later {
            run(&mut s, "solid.cylinder", json!({"base": [12 + 3 * i, 8, -1], "radius": 1, "height": 10, "operation": "cut"}));
        }
        run(&mut s, "timeline.roll_to", json!({"position": 1}));
        run(&mut s, "sketch.edit", json!({"sketch": sk}));
        // A cache too small to keep a state, as on a large design: only what is kept carries over.
        *s.model.cache.lock().unwrap() = solidcraft_doc::EvalCache::with_budget(1);
        let items = json!([{"type": "sketch_curve", "id": hole}]);
        let ahead = s.ahead();
        let kept = ahead.kept();
        let replies = ahead.replies(&[("selection.delete".to_string(), json!({"items": items, "dry_run": true}))]);
        assert!(replies.iter().all(Result::is_ok), "{replies:?}");
        let built = kept.len();
        assert!(built > later, "the check kept the history after the marker: {built} results");
        s.model.keep(Some(kept.clone()));
        run(&mut s, "selection.delete", json!({"items": items}));
        s.model.keep(None);
        assert_eq!(kept.len(), built, "the delete found everything it builds among what the check kept");
        assert!(s.model.state().sketch(sk).unwrap().sketch.curves.iter().all(|c| c.id != hole));
    }

    #[test]
    fn mixed_selection_is_one_undo_step() {
        let (mut s, _) = part();
        let body = s.model.state().bodies[1].name.clone();
        let depth = s.history.len();
        let n = s.doc.features.len();
        let r = run(
            &mut s,
            "selection.delete",
            json!({"items": [{"type": "body", "name": body}, {"type": "plane", "name": "Plane1"}, {"type": "plane", "name": "XY"}]}),
        );
        assert_eq!(bodies(&s), 1, "{r}");
        assert!(s.doc.find_feature("Plane1").is_none());
        assert_eq!(r["skipped"][0], "the origin planes can't be deleted");
        assert_eq!(s.history.len(), depth + 1);
        run(&mut s, "edit.undo", json!({}));
        assert_eq!((bodies(&s), s.doc.features.len()), (2, n));
    }

    #[test]
    fn construction_axis_deletes_with_dry_run_and_undo_but_origin_axes_are_refused() {
        let mut s = Session::default();
        run(&mut s, "construct.axis.two_points", json!({"a": {"point": [0, 0, 0]}, "b": {"point": [1, 2, 3]}, "name": "CustomAxis"}));
        let axis = s.doc.find_feature("CustomAxis").unwrap().id;
        let depth = s.history.len();
        let items = json!([{"type": "axis", "name": "CustomAxis"}, {"type": "axis", "name": "X"}]);
        let dry = run(&mut s, "selection.delete", json!({"items": items, "dry_run": true}));
        assert_eq!(dry["deleted"], json!(["CustomAxis"]));
        assert_eq!(dry["skipped"], json!(["the origin axes can't be deleted"]));
        assert!(s.doc.feature(axis).is_some());
        assert_eq!(s.history.len(), depth);
        let deleted = run(&mut s, "selection.delete", json!({"items": items}));
        assert_eq!(deleted, dry);
        assert!(s.doc.feature(axis).is_none());
        assert_eq!(s.history.len(), depth + 1);
        run(&mut s, "edit.undo", json!({}));
        assert!(s.doc.feature(axis).is_some());
        for name in ["X", "Y", "Z"] {
            let error = s.execute("selection.delete", &json!({"items": [{"type": "axis", "name": name}]})).unwrap_err();
            assert!(error.to_string().contains("the origin axes can't be deleted"), "{error}");
            assert!(s.doc.feature(axis).is_some());
        }
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

    /// An instance deleted while its component has others goes alone, and the joints on it go
    /// with it, with their motion links (#7749): left behind, a joint names an occurrence that is
    /// gone, and every solve reports it.
    #[test]
    fn an_instance_deleted_alone_takes_its_joints_with_it() {
        let mut s = joint_assembly();
        let b1 = s.doc.occurrences.iter().find(|o| o.name == "B:1").unwrap().id;
        let b2 = run(&mut s, "occurrence.copy", json!({"occurrence": "B:1", "translate": [0, 40, 0]}))["occurrence"].as_u64().unwrap();
        let dry = run(&mut s, "selection.delete", json!({"occurrences": [b1], "dry_run": true}));
        assert_eq!(s.doc.assembly.joints.len(), 2, "a dry run changes nothing");
        let deleted = run(&mut s, "selection.delete", json!({"occurrences": [b1]}));
        assert_eq!(deleted, dry);
        let left: Vec<&str> = s.doc.assembly.joints.iter().map(|j| j.name.as_str()).collect();
        assert_eq!(left, ["Glue"], "the joint on the deleted instance goes with it");
        assert!(s.doc.assembly.links.is_empty(), "and so does its motion link");
        assert_eq!(deleted["deleted_joints"].as_array().map(Vec::len), Some(1), "the reply names it: {deleted}");
        assert!(s.doc.occurrences.iter().any(|o| o.id == b2), "the other instance stays");
        let solved = run(&mut s, "joint.solve", json!({}));
        assert_eq!(solved["errors"], json!([]), "no joint is left pointing at the deleted instance");
        run(&mut s, "edit.undo", json!({}));
        assert_eq!(s.doc.assembly.joints.len(), 2);
        assert_eq!(s.doc.assembly.links.len(), 1);
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

    /// A rectangle picked by clicking inside it (a profile) goes with Delete while sketching
    /// (#7107): its four lines go, the circle inside it (its hole) stays, and undo brings them
    /// back. Outside the sketch a profile still goes only with its sketch.
    #[test]
    fn a_profile_deletes_the_curves_around_it_while_sketching() {
        let mut s = Session::default();
        let sk = run(&mut s, "sketch.create", json!({"plane": "XY"}))["sketch"].as_u64().unwrap();
        run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [40, 20]}));
        run(&mut s, "sketch.circle.center", json!({"center": [10, 10], "radius": 3, "id": "hole"}));
        let index = {
            let st = s.model.state();
            solidcraft_sketch::profile_at_point(&st.sketch(sk).unwrap().profiles, solidcraft_geom::Vec2::new(30.0, 10.0)).unwrap()
        };
        let before = curves(&s, sk);
        assert_eq!(before.len(), 5, "four lines and a circle: {before:?}");
        let item = json!({"items": [{"type": "profile", "sketch": sk, "index": index}]});
        run(&mut s, "selection.delete", item.clone());
        assert_eq!(curves(&s, sk), vec!["hole".to_string()], "only the rectangle goes");
        run(&mut s, "edit.undo", json!({}));
        assert_eq!(curves(&s, sk), before);
        run(&mut s, "sketch.finish", json!({}));
        let err = s.execute("selection.delete", &item).unwrap_err().to_string();
        assert!(err.contains("a profile goes with its sketch"), "{err}");
        assert_eq!(curves(&s, sk), before);
    }

    /// A picked letter goes with its whole text, counters too (an O's inside is a profile of
    /// its own), and the rectangle beside it stays.
    #[test]
    fn a_letter_profile_deletes_its_whole_text_while_sketching() {
        let mut s = Session::default();
        let sk = run(&mut s, "sketch.create", json!({"plane": "XY"}))["sketch"].as_u64().unwrap();
        run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [40, 20]}));
        run(&mut s, "sketch.text", json!({"text": "OB", "at": [60, 0], "height": 10}));
        let (index, rect) = {
            let st = s.model.state();
            let ss = st.sketch(sk).unwrap();
            let text = |id: &String| ss.sketch.curves.iter().any(|c| &c.id == id && c.link.is_some());
            let index = ss.profiles.iter().position(|p| p.outer_curves.iter().all(text)).unwrap();
            (index, ss.sketch.curves.iter().filter(|c| c.link.is_none()).map(|c| c.id.clone()).collect::<Vec<_>>())
        };
        assert_eq!(rect.len(), 4, "{rect:?}");
        assert!(curves(&s, sk).len() > 4, "the text has outlines");
        run(&mut s, "selection.delete", json!({"items": [{"type": "profile", "sketch": sk, "index": index}]}));
        assert_eq!(curves(&s, sk), rect, "the whole text goes, the rectangle stays");
    }

    fn curves(s: &Session, sk: u64) -> Vec<String> {
        s.model.state().sketch(sk).unwrap().sketch.curves.iter().map(|c| c.id.clone()).collect()
    }
}
