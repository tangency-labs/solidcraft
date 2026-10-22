#![allow(clippy::unwrap_used, clippy::panic)]
use serde_json::{Value, json};
use solidcraft_doc::{Document, FeatureKind, ProfileSel};
use solidcraft_geom::Vec2;
use solidcraft_sketch::{CurveKind, Sketch};

use crate::{Session, Snapshot};

fn run(s: &mut Session, command: &str, params: Value) -> Value {
    s.execute(command, &params).unwrap_or_else(|error| panic!("{command}: {error}"))
}
fn rounded() -> (Session, u64, String) {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane":"XY","name":"Rounded"}));
    let sketch = s.active_sketch.unwrap();
    run(&mut s, "sketch.rectangle.two_point", json!({"p0":[-38,-33],"p1":[38,33]}));
    let fillet = run(&mut s, "sketch.fillet", json!({"points":["l1.start","l1.end","l2.end","l3.end"],"radius":"1 mm"}));
    (s, sketch, fillet["param"].as_str().unwrap().into())
}
fn offset(s: &mut Session, curve: &str, distance: f64) -> (String, Vec<String>) {
    let result = run(s, "sketch.offset", json!({"curves":[curve],"distance":distance,"side":[0,0]}));
    (result["association"].as_str().unwrap().into(), result["curves"].as_array().unwrap().iter().map(|v| v.as_str().unwrap().into()).collect())
}
fn solved(s: &Session, id: u64) -> Sketch {
    s.model.state().sketch(id).unwrap().sketch.clone()
}
fn arc_centres(sk: &Sketch, curves: &[String]) -> Vec<(String, usize, Vec2)> {
    curves
        .iter()
        .filter_map(|id| {
            let CurveKind::Arc { c, .. } = sk.curves.get(sk.curve_index(id)?)?.kind else { return None };
            Some((id.clone(), c, sk.point(c)?))
        })
        .collect()
}
fn history(stack: &[Snapshot]) -> Vec<(String, String, Option<u64>, String)> {
    stack.iter().map(|entry| (entry.label.clone(), entry.doc.to_json(), entry.active_sketch, format!("{:?}", entry.pending_moves))).collect()
}
fn refuses_atomically(s: &mut Session, command: &str, params: Value) -> String {
    let document = s.doc.to_json();
    let undo = history(&s.undo);
    let redo = history(&s.redo);
    let active = s.active_sketch;
    let error = s.execute(command, &params).unwrap_err().to_string();
    assert_eq!(s.doc.to_json(), document);
    assert_eq!(history(&s.undo), undo);
    assert_eq!(history(&s.redo), redo);
    assert_eq!(s.active_sketch, active);
    error
}

#[test]
fn offset_guard_retired_parent_refuses_new_child_without_destroying_redo() {
    let (mut s, sketch, _) = rounded();
    let (_, outputs) = offset(&mut s, "l1", 1.4);
    assert_eq!(outputs.len(), 4);
    assert_eq!(solved(&s, sketch).offsets[0].topology.as_ref().unwrap().roles.iter().filter(|r| r.retired).count(), 4);
    run(&mut s, "sketch.point", json!({"point":[100,100]}));
    run(&mut s, "edit.undo", json!({}));
    assert!(!s.redo.is_empty());
    let error = refuses_atomically(&mut s, "sketch.offset", json!({"curves":[outputs[0]],"distance":0.2,"side":[0,0]}));
    assert!(error.contains("retired corner") && error.contains("break its offset association"), "{error}");
}

#[test]
fn offset_guard_live_child_follows_radius_then_retirement_is_honest() {
    let (mut s, sketch, radius) = rounded();
    let (_, parent) = offset(&mut s, "l1", 0.2);
    let (child_association, child) = offset(&mut s, &parent[0], 0.2);
    let arc_radius = |s: &Session| {
        let sk = solved(s, sketch);
        child
            .iter()
            .filter_map(|id| {
                let CurveKind::Arc { c, a, .. } = sk.curves.get(sk.curve_index(id)?)?.kind else { return None };
                Some(sk.point(c)?.dist(sk.point(a)?))
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(arc_radius(&s).len(), 4);
    assert!(arc_radius(&s).iter().all(|r| (*r - 0.6).abs() < 1e-6));
    run(&mut s, "parameters.change", json!({"name":radius,"expression":"2 mm"}));
    assert!(s.model.results.iter().all(|r| r.error.is_none()));
    assert!(arc_radius(&s).iter().all(|r| (*r - 1.6).abs() < 1e-6));
    run(&mut s, "sketch.finish", json!({}));
    run(
        &mut s,
        "solid.extrude",
        json!({"sketch":sketch,"profiles":{"offset_boundaries":[child_association]},"distance":2,"operation":"new","body_name":"Child"}),
    );
    let before = s.doc.to_json();
    let undo = history(&s.undo);
    let redo = history(&s.redo);
    match s.execute("parameters.change", &json!({"name":radius,"expression":"0.1 mm"})) {
        Err(error) => {
            assert!(error.to_string().contains("offset"), "{error}");
            assert_eq!(s.doc.to_json(), before);
            assert_eq!(history(&s.undo), undo);
            assert_eq!(history(&s.redo), redo);
        }
        Ok(_) => {
            assert!(s.model.results.iter().any(|r| r.error.is_some()), "retired parent must not keep a stale successful child");
            assert!(s.world_state().unwrap().body("Child").is_none());
            let reopened = Session::new(Document::from_json(&s.doc.to_json()).unwrap());
            assert!(reopened.model.results.iter().any(|r| r.error.is_some()));
            assert!(reopened.world_state().unwrap().body("Child").is_none());
        }
    }
}

#[test]
fn offset_guard_partial_delete_requires_break_and_surviving_arc_centres_detach() {
    let (mut s, sketch, _) = rounded();
    let (association, outputs) = offset(&mut s, "l1", 0.2);
    let error = refuses_atomically(&mut s, "sketch.delete", json!({"entities":[outputs[0]]}));
    assert!(error.contains("break the offset association"), "{error}");
    let source = solved(&s, sketch);
    let source_ids: Vec<_> = source.curves.iter().filter(|c| !outputs.contains(&c.id)).map(|c| c.id.clone()).collect();
    let source_centres = arc_centres(&source, &source_ids);
    assert_eq!(source_centres.len(), 4);
    run(&mut s, "sketch.offset.break", json!({"curve":association}));
    let released = solved(&s, sketch);
    let independent = arc_centres(&released, &outputs);
    assert_eq!(independent.len(), 4);
    assert!(independent.iter().all(|(_, centre, _)| source_centres.iter().all(|(_, source, _)| centre != source)));
    // Released geometry is editable: anchor its new centres independently, then
    // move a source centre. Shared source nodes would block that actual move.
    let released_centres: Vec<_> = independent.iter().map(|(id, _, _)| format!("{id}.center")).collect();
    run(&mut s, "sketch.constraint.fix", json!({"entities":released_centres,"fixed":true}));
    run(&mut s, "sketch.delete", json!({"entities":[outputs[0]]}));
    let deleted = solved(&s, sketch);
    assert!(deleted.curve_index(&outputs[0]).is_none());
    let survivors = arc_centres(&deleted, &outputs);
    let (source_id, _, position) = source_centres.first().unwrap();
    let target = *position + Vec2::new(5.0, 5.0);
    run(&mut s, "sketch.move_point", json!({"point":format!("{source_id}.center"),"to":[target.x,target.y]}));
    assert!(s.model.results.iter().all(|r| r.error.is_none()));
    let changed = solved(&s, sketch);
    for (id, _, position) in independent {
        let after = arc_centres(&changed, std::slice::from_ref(&id));
        assert_eq!(after.len(), 1);
        assert!(
            after[0].2.dist(position) < 1e-8,
            "{id}: released {position:?}, after deletion {:?}, after source move {:?}; deleted curve {}",
            survivors.iter().find(|(curve, _, _)| curve == &id),
            after[0],
            outputs[0]
        );
    }
    let moved = arc_centres(&changed, std::slice::from_ref(source_id));
    assert_eq!(moved.len(), 1);
    assert!(moved[0].2.dist(target) < 1e-8);
    assert!(moved[0].2.dist(*position) > 5.0);
}

#[test]
fn offset_guard_complete_delete_keeps_dangling_profile_loadable_without_circle_retarget() {
    let (mut s, sketch, _) = rounded();
    let (association, outputs) = offset(&mut s, "l1", 0.2);
    run(&mut s, "sketch.circle.center", json!({"center":[100,0],"radius":3,"id":"Unrelated"}));
    run(&mut s, "sketch.finish", json!({}));
    run(
        &mut s,
        "solid.extrude",
        json!({"sketch":sketch,"profiles":{"offset_boundaries":[association]},"distance":2,"operation":"new","name":"OffsetBody","body_name":"Selected"}),
    );
    run(&mut s, "sketch.edit", json!({"sketch":sketch}));
    run(&mut s, "sketch.delete", json!({"entities":outputs}));
    let saved = s.doc.to_json();
    let FeatureKind::Extrude { profiles: ProfileSel::OffsetBoundaries { associations }, .. } = &s.doc.find_feature("OffsetBody").unwrap().kind else {
        panic!("explicit offset identity lost")
    };
    assert_eq!(associations, &vec![association]);
    let check = |s: &Session| {
        assert!(solved(s, sketch).curve_index("Unrelated").is_some());
        assert!(s.model.results.iter().find(|r| r.id == s.doc.find_feature("OffsetBody").unwrap().id).unwrap().error.is_some());
        assert!(s.world_state().unwrap().body("Selected").is_none(), "dangling selection must never adopt the unrelated circle");
    };
    check(&s);
    let reopened = Session::new(Document::from_json(&saved).unwrap());
    check(&reopened);
    assert_eq!(reopened.doc.to_json(), saved);
}
