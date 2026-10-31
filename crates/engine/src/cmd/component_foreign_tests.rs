#![allow(clippy::unwrap_used)]
use crate::{ForeignCopy, Session};
use serde_json::{Value, json};
use std::sync::Arc;

fn run(s: &mut Session, cmd: &str, p: Value) -> Value {
    s.execute(cmd, &p).unwrap()
}
fn created(s: &mut Session, name: &str) -> (u64, u64) {
    let c = run(s, "component.create", json!({"name": name}));
    (c["component"].as_u64().unwrap(), c["occurrence"].as_u64().unwrap())
}
fn volume(s: &mut Session, id: u64) -> f64 {
    run(s, "component.properties", json!({"component": id}))["total"]["volume_mm3"].as_f64().unwrap()
}
fn design(name: &str) -> Session {
    let mut s = Session::default();
    s.doc_mut().name = name.into();
    s
}
/// `s` copies its placement `path` for another design.
fn copied(s: &Session, path: Vec<u64>) -> Option<Arc<ForeignCopy>> {
    Some(Arc::new(ForeignCopy::of(s, Vec::new(), vec![path])))
}

/// A component copied in another design pastes as an independent copy of it and what it holds:
/// its history, its appearances, a part placed twice in it still one component placed twice,
/// where it is in the world, named "(2)" where the name is taken, as one step to undo.
#[test]
fn a_component_from_another_design_pastes_with_its_history() {
    let mut source = design("Bracket");
    let (assembly, at) = created(&mut source, "Assembly");
    let (child, _) = created(&mut source, "Child");
    run(&mut source, "solid.box", json!({"length": 2, "width": 2, "height": 2, "body_name": "Cube"}));
    run(&mut source, "appearance.assign", json!({"bodies": ["Cube"], "color": "#abcdef"}));
    run(&mut source, "occurrence.copy", json!({"component": child}));
    run(&mut source, "component.activate", json!({"component": 0}));
    run(&mut source, "occurrence.ground", json!({"occurrence": at, "grounded": false}));
    run(&mut source, "occurrence.move", json!({"occurrence": at, "translate": [0, 30, 0]}));
    let was = source.world_state().body("Cube").unwrap().mesh().bounds();
    let box_features = source.doc.features.len();

    let mut s = design("Frame");
    created(&mut s, "Assembly");
    run(&mut s, "component.activate", json!({"component": 0}));
    s.foreign_copy = copied(&source, vec![at]);
    let undo = s.history.len();
    let out = run(&mut s, "component.paste_from_design", json!({}));
    assert_eq!(s.history.len(), undo + 1);
    let new = s.doc.components.iter().find(|c| c.name == "Assembly (2)").expect("named (2)").id;
    assert_eq!(out["paths"], json!([[s.doc.occurrence_of(new).unwrap().id]]));
    let kids: Vec<_> = s.doc.occurrences.iter().filter(|o| o.parent == new).collect();
    assert_eq!(kids.len(), 2);
    assert_eq!(kids[0].component, kids[1].component, "still one component placed twice");
    assert_eq!(s.doc.features.len(), box_features, "the box's history came along");
    assert!((volume(&mut s, new) - 16.0).abs() < 1e-6);
    assert_eq!(s.doc.appearances.bodies["Cube"].color, [0xab, 0xcd, 0xef]);
    let is = s.world_state().body("Cube").unwrap().mesh().bounds();
    assert!(was.min.dist(is.min) < 1e-6 && was.max.dist(is.max) < 1e-6, "{was:?} vs {is:?}");
    // Its history is its own: changing it here leaves the original as it was.
    let copy = s.doc.features.iter().find(|f| matches!(f.kind, solidcraft_doc::FeatureKind::Box { .. })).unwrap();
    let parameter = copy.param_names[0].clone();
    run(&mut s, "parameters.change", json!({"name": parameter, "expression": "3 mm"}));
    assert!((volume(&mut s, new) - 24.0).abs() < 1e-6);
    assert!((volume(&mut source, assembly) - 16.0).abs() < 1e-6);
    let reopened = solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap();
    assert_eq!(reopened.occurrences.iter().filter(|o| o.parent == new).count(), 2);
    run(&mut s, "edit.undo", json!({}));
    run(&mut s, "edit.undo", json!({}));
    assert!(s.doc.components.iter().all(|c| c.name != "Assembly (2)"));
}

/// The user parameters a pasted component uses come along: as they were where the name is
/// free, else renamed `w_2`, its features following; this design's own `w` stays as it is.
#[test]
fn user_parameters_come_along_renamed_where_taken() {
    let mut source = design("Bracket");
    run(&mut source, "parameters.add", json!({"name": "w", "expression": "4 mm"}));
    let (_, at) = created(&mut source, "Part");
    let feature = run(&mut source, "solid.box", json!({"length": 2, "width": 2, "height": 2, "body_name": "Cube"}))["feature"].as_u64().unwrap();
    let length = source.doc.feature(feature).unwrap().param_names[0].clone();
    run(&mut source, "parameters.change", json!({"name": length, "expression": "w"}));

    let mut s = design("Frame");
    run(&mut s, "parameters.add", json!({"name": "w", "expression": "10 mm"}));
    s.foreign_copy = copied(&source, vec![at]);
    run(&mut s, "component.paste_from_design", json!({}));
    let w2 = s.doc.params.iter().find(|p| p.name == "w_2").expect("w came along as w_2");
    assert_eq!(w2.expr, "4 mm");
    assert_eq!(s.doc.params.iter().find(|p| p.name == "w").unwrap().expr, "10 mm");
    let part = s.doc.components.iter().find(|c| c.name == "Part").unwrap().id;
    assert!((volume(&mut s, part) - 16.0).abs() < 1e-6);
    run(&mut s, "parameters.change", json!({"name": "w_2", "expression": "1 mm"}));
    assert!((volume(&mut s, part) - 4.0).abs() < 1e-6, "the copy follows w_2");
}

/// A component whose features use what isn't in it (here a body outside it) isn't pasted into
/// another design: it is refused, saying what, and the design is left as it was.
#[test]
fn a_component_using_what_is_outside_it_is_refused_saying_what() {
    let mut source = design("Bracket");
    run(&mut source, "solid.box", json!({"length": 2, "width": 2, "height": 2, "body_name": "Cube"}));
    let (_, at) = created(&mut source, "Part");
    run(&mut source, "solid.box", json!({"length": 1, "width": 1, "height": 1, "body_name": "Pin"}));
    run(&mut source, "solid.move", json!({"bodies": ["Cube"], "translate": [10, 0, 0]}));

    let mut s = design("Frame");
    s.foreign_copy = copied(&source, vec![at]);
    let saved = s.doc.to_json();
    let error = s.execute("component.paste_from_design", &json!({})).unwrap_err().to_string();
    assert!(error.contains("uses body Cube"), "{error}");
    assert_eq!(s.doc.to_json(), saved);
}

/// A component that bodies were moved into (a body transfer, as Create Components from Bodies
/// makes) pastes into another design, which has none of the components the transfer names.
#[test]
fn a_component_bodies_were_moved_into_pastes_into_another_design() {
    let mut source = design("Bracket");
    run(&mut source, "solid.box", json!({"length": 2, "width": 2, "height": 2, "body_name": "Cube"}));
    let (part, at) = created(&mut source, "Part");
    run(&mut source, "component.move_bodies", json!({"bodies": ["Cube"], "component": part}));
    run(&mut source, "solid.box", json!({"length": 1, "width": 1, "height": 1, "body_name": "Pin"}));

    let mut s = design("Frame");
    s.foreign_copy = copied(&source, vec![at]);
    run(&mut s, "component.paste_from_design", json!({}));
    let new = s.doc.components.iter().find(|c| c.name == "Part").unwrap().id;
    let state = s.model.state();
    assert_eq!(s.doc.model_body_component(state.body("Cube").unwrap()), new);
    assert_eq!(s.doc.model_body_component(state.body("Pin").unwrap()), new);
    // The body moved in from outside comes as an imported body.
    assert!(s.doc.find_feature("BodyImport1").is_some_and(|f| f.component == new));
}
