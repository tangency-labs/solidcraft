// Exact occurrence workflows through the same commands used by the Browser.
use crate::Session;
use serde_json::{Value, json};

fn run(s: &mut Session, command: &str, p: Value) -> Value {
    s.execute(command, &p).unwrap()
}
fn created(s: &mut Session, name: &str) -> (u64, u64) {
    let c = run(s, "component.create", json!({"name": name, "grounded": false}));
    (c["component"].as_u64().unwrap(), c["occurrence"].as_u64().unwrap())
}
fn near(value: &Value, expected: f64) {
    assert!((value.as_f64().unwrap() - expected).abs() < 1e-6, "{value} != {expected}");
}

#[test]
fn component_origins_keep_nested_copies_and_visibility_separate() {
    use crate::origins;
    use solidcraft_geom::Vec3;
    let mut s = Session::default();
    let (parent, first) = created(&mut s, "Assembly");
    run(&mut s, "occurrence.move", json!({"occurrence":first,"translate":[100,0,0]}));
    let (_, child) = created(&mut s, "Child");
    run(&mut s, "occurrence.move", json!({"occurrence":child,"translate":[20,0,0]}));
    run(&mut s, "component.activate", json!({"component":0}));
    let second = run(&mut s, "occurrence.copy", json!({"component":parent,"translate":[50,0,0]}))["occurrence"].as_u64().unwrap();
    let (one, two) = (vec![first, child], vec![second, child]);
    assert!(s.origin_plane(&origins::key(&one, "XY")).unwrap().origin.dist(Vec3::new(120., 0., 0.)) < 1e-8);
    assert!(s.origin_plane(&origins::key(&two, "XY")).unwrap().origin.dist(Vec3::new(170., 0., 0.)) < 1e-8);
    assert!(s.shown_component_origins().is_empty());
    run(&mut s, "browser.visibility", json!({"tree_folders":[origins::folder(&two)],"visible":true}));
    assert!(s.folder_hidden(&origins::folder(&one)));
    assert!(!s.folder_hidden(&origins::folder(&two)));
    assert_eq!(s.visibility.origin_hidden, None);
    assert_eq!(s.shown_component_origins().len(), 7);
    run(&mut s, "browser.visibility", json!({"items":[origins::key(&two,"X")],"visible":false}));
    assert_eq!(s.shown_component_origins().len(), 6);
    assert!(s.construction_visible("X"));
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(s.shown_component_origins().len(), 7);
    run(&mut s, "edit.redo", json!({}));
    let mut saved = (*s.doc).clone();
    saved.visibility = s.visibility.clone();
    let reopened = Session::new(serde_json::from_str(&serde_json::to_string(&saved).unwrap()).unwrap());
    assert_eq!(reopened.visibility, s.visibility);
    assert_eq!(reopened.shown_component_origins().len(), 6);
    run(&mut s, "browser.visibility", json!({"paths":[[second]],"visible":false}));
    assert!(s.shown_component_origins().is_empty(), "a hidden parent hides its origin too");
    let before = s.visibility.clone();
    assert!(s.execute("browser.visibility", &json!({"tree_folders":[origins::folder(&one),"o:999999/origin"],"visible":true})).is_err());
    assert_eq!(s.visibility, before, "invalid batches must be atomic");
    for invalid in ["o:/origin", "o:0/origin", "o:01/origin", "c0/origin"] {
        assert!(s.execute("browser.visibility", &json!({"tree_folders":[invalid],"visible":true})).is_err(), "{invalid}");
    }
    let folded = run(&mut s, "browser.expand_all", json!({"path":one}));
    assert!(folded["closed_by_default"].as_array().unwrap().contains(&json!(origins::folder(&one))));
    run(&mut s, "occurrence.move", json!({"occurrence":child,"translate":[10,0,0],"capture":false}));
    assert!(s.origin_plane(&origins::key(&one, "XY")).unwrap().origin.dist(Vec3::new(130., 0., 0.)) < 1e-8);
}

#[test]
fn component_origin_picks_use_world_geometry_then_the_authoring_frame_once() {
    use crate::origins;
    use solidcraft_geom::Vec3;
    let mut s = Session { param_check: crate::schema::ParamCheck::Strict, ..Session::default() };
    let (component, occurrence) = created(&mut s, "Part");
    run(&mut s, "occurrence.move", json!({"occurrence":occurrence,"translate":[20,30,40],"axis":[1,0,0],"angle":"90 deg"}));
    let plane = origins::key(&[occurrence], "XY");
    let expected = Vec3::new(20., 30., 40.);
    assert!(s.origin_plane(&plane).unwrap().origin.dist(expected) < 1e-8);
    assert!(s.origin_plane(&plane).unwrap().normal().dist(-Vec3::Y) < 1e-8);
    s.selection = vec![crate::Sel::Plane { name: plane.clone() }];
    run(&mut s, "view.normal_to", json!({}));
    let camera = s.camera.unwrap();
    assert!(camera.target.dist(expected) < 1e-8);
    assert!(camera.back().dot(Vec3::Y).abs() > 1. - 1e-8);
    for author in [0, component] {
        run(&mut s, "component.activate", json!({"component":author}));
        let sketch = run(&mut s, "sketch.create", json!({"plane":plane}))["sketch"].as_u64().unwrap();
        let state = s.world_state();
        let actual = state.sketch(sketch).unwrap().plane;
        assert!(actual.origin.dist(expected) < 1e-8, "author {author}: {actual:?}");
        assert!(actual.normal().dist(-Vec3::Y) < 1e-8);
        run(&mut s, "sketch.finish", json!({}));
    }
    run(&mut s, "component.activate", json!({"component":0}));
    run(&mut s, "construct.point.vertex", json!({"point":{"construct_point":origins::key(&[occurrence],"O")},"name":"Origin copy"}));
    let point = crate::view::construction_points(&s).into_iter().find(|(_, n, _)| n == "Origin copy").unwrap().2;
    assert!(point.dist(expected) < 1e-8);
    run(&mut s, "construct.plane.at_angle", json!({"base":plane,"axis":origins::key(&[occurrence],"X"),"angle":"90 deg","name":"Turned"}));
    let turned = crate::view::construction_planes(&s).into_iter().find(|(_, n, _)| n == "Turned").unwrap().2;
    assert!(turned.origin.dist(expected) < 1e-8);
    assert!(turned.normal().dot(Vec3::Z).abs() > 1. - 1e-8);
    run(&mut s, "component.activate", json!({"component":component}));
    run(&mut s, "construct.plane.perpendicular", json!({"plane":plane,"line":{"axis":origins::key(&[occurrence],"X")},"name":"Perpendicular"}));
    let perpendicular = crate::view::construction_planes(&s).into_iter().find(|(_, n, _)| n.ends_with("Perpendicular")).unwrap().2;
    assert!(perpendicular.origin.dist(expected) < 1e-8);
    assert!(perpendicular.normal().dot(Vec3::Z).abs() > 1. - 1e-8);
}

#[test]
fn component_create_rejects_bodies_without_changing_design() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length":10,"width":10,"height":10,"body_name":"Body1"}));
    let doc = s.doc.clone();
    let active_path = s.active_occurrence_path.clone();
    let error = s.execute("component.create", &json!({"name":"Part","bodies":["Body1"]})).unwrap_err();
    assert!(matches!(&error, crate::EngineError::BadParams { cmd, .. } if cmd == "component.create"));
    let msg = error.to_string();
    for expected in ["bodies", "name, parent, parent_path, activate", "component.move_bodies", "component.from_bodies"] {
        assert!(msg.contains(expected), "{msg}");
    }
    assert_eq!(s.doc, doc);
    assert_eq!(s.active_component, 0);
    assert_eq!(s.active_occurrence_path, active_path);
    assert_eq!(s.doc.model_body_component(s.model.state().body("Body1").unwrap()), 0);
}

#[test]
fn component_create_rejects_unknown_keys_and_accepts_documented_keys() {
    let mut s = Session::default();
    let (parent, occurrence) = created(&mut s, "Parent");
    let params = json!({"name":"Child","parent":parent,"parent_path":[occurrence],"activate":false});
    let mut invalid = params.clone();
    invalid["activte"] = json!(true);
    let doc = s.doc.clone();
    let error = s.execute("component.create", &invalid).unwrap_err().to_string();
    assert!(error.contains("activte"), "{error}");
    assert!(error.contains("name, parent, parent_path, activate"), "{error}");
    assert_eq!(s.doc, doc);
    assert_eq!(s.active_component, parent);
    assert_eq!(s.active_occurrence_path, Some(vec![occurrence]));
    let child = run(&mut s, "component.create", params);
    assert_eq!(child["name"], "Child");
    assert_eq!(child["parent"], parent);
    assert_eq!(s.active_component, parent);
    assert_eq!(s.active_occurrence_path, Some(vec![occurrence]));
}

#[test]
fn body_and_component_renames_keep_hidden_copies_through_history() {
    let mut s = Session::default();
    let (component, first) = created(&mut s, "Knob");
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "body_name": "Knob"}));
    let second = run(&mut s, "occurrence.copy", json!({"component": component, "translate": [20, 0, 0]}))["occurrence"].as_u64().unwrap();
    let name = |s: &Session, path: &[u64]| s.occurrence_placement(path).unwrap().bodies[0].world.clone();
    let hidden = name(&s, &[second]);
    run(&mut s, "browser.visibility", json!({"items": [hidden], "visible": false}));
    for (command, params) in [
        ("body.rename", json!({"body": "Knob", "name": "Grip"})),
        ("component.rename", json!({"component": component, "name": "Dial"})),
        ("edit.undo", json!({})),
        ("edit.undo", json!({})),
        ("edit.redo", json!({})),
        ("edit.redo", json!({})),
    ] {
        run(&mut s, command, params);
        assert!(s.body_visible(&name(&s, &[first])), "{command}: the first copy stays visible");
        assert!(!s.body_visible(&name(&s, &[second])), "{command}: the hidden copy stays hidden");
        assert_eq!(s.visibility.hidden_bodies, [name(&s, &[second])], "no stale names after {command}");
    }
}

#[test]
fn renaming_a_parent_preserves_hidden_nested_copies_with_ambiguous_labels() {
    let mut s = Session::default();
    let (parent, first_parent) = created(&mut s, "Assembly");
    let (_, child) = created(&mut s, "Knob");
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "body_name": "Knob"}));
    let second_parent = run(&mut s, "occurrence.copy", json!({"component": parent, "translate": [20, 0, 0]}))["occurrence"].as_u64().unwrap();
    let name = |s: &Session, path: &[u64]| s.occurrence_placement(path).unwrap().bodies[0].world.clone();
    let hidden = name(&s, &[second_parent, child]);
    run(&mut s, "browser.visibility", json!({"items": [hidden], "visible": false}));
    for (command, params) in [
        ("component.rename", json!({"component": parent, "name": "Assembly/Other"})),
        ("body.rename", json!({"body": "Knob", "name": "Grip (@occ[custom])"})),
        ("edit.undo", json!({})),
        ("edit.undo", json!({})),
        ("edit.redo", json!({})),
        ("edit.redo", json!({})),
    ] {
        run(&mut s, command, params);
        assert!(s.body_visible(&name(&s, &[first_parent, child])), "{command}");
        assert!(!s.body_visible(&name(&s, &[second_parent, child])), "{command}");
        assert_eq!(s.visibility.hidden_bodies, [name(&s, &[second_parent, child])]);
    }
}

#[test]
fn body_rename_reusing_hidden_names_preserves_each_copy() {
    let mut s = Session::default();
    let (component, first) = created(&mut s, "Part");
    let retired = run(&mut s, "solid.box", json!({"length": 5, "width": 5, "height": 5, "body_name": "Retired"}))["feature"].as_u64().unwrap();
    run(&mut s, "solid.box", json!({"length": 5, "width": 5, "height": 5, "corner": [10, 0, 0], "body_name": "Visible"}));
    let second = run(&mut s, "occurrence.copy", json!({"component": component, "translate": [20, 0, 0]}))["occurrence"].as_u64().unwrap();
    let name = |s: &Session, source: &str, path: &[u64]| {
        s.occurrence_placement(path).unwrap().bodies.iter().find(|b| b.source == source).unwrap().world.clone()
    };
    let retired_names = [name(&s, "Retired", &[first]), name(&s, "Retired", &[second])];
    run(&mut s, "browser.visibility", json!({"items": retired_names, "visible": false}));
    run(&mut s, "timeline.delete", json!({"feature": retired}));
    let hidden = name(&s, "Visible", &[second]);
    run(&mut s, "browser.visibility", json!({"items": [hidden], "visible": false}));
    let before = s.visibility.clone();
    for (command, params, source) in [
        ("body.rename", json!({"body": "Visible", "name": "Retired"}), "Retired"),
        ("edit.undo", json!({}), "Visible"),
        ("edit.redo", json!({}), "Retired"),
    ] {
        run(&mut s, command, params);
        assert!(s.body_visible(&name(&s, source, &[first])), "{command}: the first copy stays visible");
        assert!(!s.body_visible(&name(&s, source, &[second])), "{command}: the second copy stays hidden");
        if command == "edit.undo" {
            assert_eq!(s.visibility, before);
        } else {
            assert_eq!(s.visibility.hidden_bodies, [name(&s, source, &[second])]);
        }
    }
}

/// The world name of a placed body as `Document::world_body_name` worked it out before
/// `WorldNames` (each name scanning the whole design), kept to prove the two agree.
fn world_name_by_scanning(doc: &crate::doc::Document, st: &crate::doc::ModelState, source: &str, component: u64, path: &[u64]) -> String {
    if path == doc.first_path(component) {
        return source.to_string();
    }
    let names: Vec<&str> = path.iter().filter_map(|id| doc.occurrences.iter().find(|o| o.id == *id)).map(|o| o.name.as_str()).collect();
    let candidate = format!("{source} ({})", names.join("/"));
    let mut counts = std::collections::BTreeMap::from([(0, 1usize)]);
    for name in &names {
        let mut next = std::collections::BTreeMap::<u64, usize>::new();
        for o in doc.occurrences.iter().filter(|o| o.name == *name) {
            if let Some(count) = counts.get(&o.parent) {
                let n = next.entry(o.component).or_default();
                *n = (*n + count).min(2);
            }
        }
        counts = next;
    }
    let ambiguous = counts.get(&component).copied().unwrap_or(0) > 1;
    let reserved =
        doc.occurrences.iter().any(|o| o.name.contains('/') || o.name.contains("@occ[")) || st.bodies.iter().any(|b| b.name.contains("@occ["));
    if !ambiguous && !reserved && st.body(&candidate).is_none() {
        return candidate;
    }
    let ids = path.iter().map(u64::to_string).collect::<Vec<_>>().join("/");
    let mut key = format!("{}@occ[{ids}]", serde_json::to_string(source).unwrap());
    while st.body(&key).is_some() {
        key.push('#');
    }
    key
}

/// Naming every placed body at once (`WorldNames`) gives each the name naming it alone did:
/// nested copies, copies whose labels collide, a body named like a label, and names with a
/// slash or the fallback key's marker.
#[test]
fn placed_body_names_match_naming_each_body_alone() {
    let mut s = Session::default();
    let (parent, _) = created(&mut s, "Assembly");
    let (knob, _) = created(&mut s, "Knob");
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "body_name": "Knob"}));
    run(&mut s, "occurrence.copy", json!({"component": parent, "translate": [20, 0, 0]}));
    run(&mut s, "occurrence.copy", json!({"component": knob, "translate": [0, 20, 0]}));
    run(&mut s, "component.activate", json!({"component": 0}));
    run(&mut s, "solid.box", json!({"length": 1, "width": 1, "height": 1, "body_name": "Knob (Assembly/Knob)"}));
    let check = |s: &Session, step: &str| {
        let st = s.model.state();
        let names = crate::doc::WorldNames::new(&s.doc, &st);
        let mut seen = 0;
        for (component, path, _) in s.doc.placements() {
            for b in st.bodies.iter().filter(|b| s.doc.model_body_component(b) == component) {
                assert_eq!(names.name(&b.name, component, &path), world_name_by_scanning(&s.doc, &st, &b.name, component, &path), "{step}: {path:?}");
                seen += 1;
            }
        }
        assert!(seen >= 5, "{step}: every placement is named ({seen})");
    };
    check(&s, "copies");
    run(&mut s, "component.rename", json!({"component": parent, "name": "Knob"}));
    check(&s, "a parent named like its child");
    run(&mut s, "component.rename", json!({"component": parent, "name": "Assembly/Other"}));
    check(&s, "a slash");
    run(&mut s, "body.rename", json!({"body": "Knob", "name": "Grip (@occ[custom])"}));
    check(&s, "the fallback marker");
}

#[test]
fn instance_copy_uses_selected_source_transform() {
    let mut s = Session::default();
    let (component, first) = created(&mut s, "Block");
    let second = run(&mut s, "occurrence.copy", json!({"component": component, "translate": [20,0,0]}))["occurrence"].as_u64().unwrap();
    run(&mut s, "occurrence.move", json!({"occurrence": second, "translate": [30,0,0]}));
    let third = run(&mut s, "occurrence.copy", json!({"occurrence": second, "translate": [5,0,0]}))["occurrence"].as_u64().unwrap();
    let placement = s.doc.occurrences.iter().find(|o| o.id == third).unwrap();
    assert!((placement.transform[3][0] - 55.0).abs() < 1e-6);
    assert_eq!(s.doc.occurrences.iter().find(|o| o.id == first).unwrap().transform[3][0], 0.0);
}

#[test]
fn instance_world_move_under_rotated_parent_uses_selected_path() {
    let mut s = Session::default();
    let (_, parent) = created(&mut s, "Parent");
    run(&mut s, "occurrence.move", json!({"occurrence": parent, "axis": [0,0,1], "angle": "90 deg"}));
    let (_, child) = created(&mut s, "Child");
    run(&mut s, "solid.box", json!({"length": 1, "width": 1, "height": 1}));
    run(&mut s, "occurrence.move", json!({"occurrence": child, "path": [parent, child], "space": "world", "translate": [10,0,0]}));
    let result = run(&mut s, "occurrence.properties", json!({"occurrence": child, "path": [parent,child]}));
    near(&result["total"]["center_of_mass"][0], 9.5);
    near(&result["total"]["center_of_mass"][1], 0.5);
}

#[test]
fn instance_authoring_at_second_placement_keeps_definition_shared() {
    let mut s = Session::default();
    let (component, first) = created(&mut s, "Block");
    let second = run(&mut s, "occurrence.copy", json!({"component": component, "translate": [20,0,0]}))["occurrence"].as_u64().unwrap();
    run(&mut s, "component.activate", json!({"component": component, "occurrence": second, "path": [second]}));
    run(&mut s, "solid.box", json!({"length": 2, "width": 2, "height": 2, "corner": [20,0,0], "operation": "new"}));
    for (occurrence, x) in [(first, 1.0), (second, 21.0)] {
        let result = run(&mut s, "occurrence.properties", json!({"occurrence": occurrence, "path": [occurrence]}));
        near(&result["total"]["center_of_mass"][0], x);
    }
}

#[test]
fn instance_selection_survives_rename_move_capture_and_serde() {
    let mut s = Session::default();
    let (component, _) = created(&mut s, "Block");
    let second = run(&mut s, "occurrence.copy", json!({"component": component, "translate": [20,0,0]}))["occurrence"].as_u64().unwrap();
    let selected = json!({"type": "occurrence", "path": [second]});
    run(&mut s, "select.set", json!({"items": [selected]}));
    run(&mut s, "component.rename", json!({"component": component, "name": "Renamed"}));
    run(&mut s, "occurrence.move", json!({"occurrence": second, "translate": [5,0,0], "capture": false}));
    run(&mut s, "component.capture_position", json!({}));
    assert_eq!(serde_json::to_value(&s.selection).unwrap(), json!([selected]));
    let roundtrip: Vec<crate::Sel> = serde_json::from_value(json!([selected])).unwrap();
    assert_eq!(roundtrip, s.selection);
}

#[test]
fn component_rename_carries_into_placement_names() {
    // #1665: the browser row shows the placement's name, so a rename has to reach it.
    let mut s = Session::default();
    let (component, first) = created(&mut s, "Block");
    let second = run(&mut s, "occurrence.copy", json!({"component": component, "translate": [20,0,0]}))["occurrence"].as_u64().unwrap();
    let third = run(&mut s, "occurrence.copy", json!({"component": component, "translate": [40,0,0]}))["occurrence"].as_u64().unwrap();
    let name = |s: &Session, id: u64| s.doc.occurrences.iter().find(|o| o.id == id).unwrap().name.clone();
    assert_eq!(name(&s, second), "Block:2");
    // A placement named otherwise (an import's own name) keeps it.
    s.doc_mut().occurrences.iter_mut().find(|o| o.id == third).unwrap().name = "Blockade:3".into();
    run(&mut s, "component.rename", json!({"component": component, "name": "box"}));
    assert_eq!([name(&s, first), name(&s, second), name(&s, third)], ["box:1", "box:2", "Blockade:3"]);
    assert_eq!(run(&mut s, "component.list", json!({}))["components"][0]["children"][0]["name"], "box:1");
}

#[test]
fn instance_selection_delete_removes_only_selected_instance() {
    let mut s = Session::default();
    let (component, first) = created(&mut s, "Block");
    run(&mut s, "solid.box", json!({"length": 2, "width": 2, "height": 2}));
    let second = run(&mut s, "occurrence.copy", json!({"component": component, "translate": [20,0,0]}))["occurrence"].as_u64().unwrap();
    run(&mut s, "select.set", json!({"items": [{"type": "occurrence", "path": [second]}]}));
    let items = s.selection.clone();
    run(&mut s, "selection.delete", json!({"items": items}));
    assert!(s.doc.occurrences.iter().any(|o| o.id == first));
    assert!(!s.doc.occurrences.iter().any(|o| o.id == second));
    assert_eq!(s.world_state().bodies.len(), 1);
    assert!(s.doc.components.iter().any(|c| c.id == component));
}

#[test]
fn world_move_includes_pending_parent_and_leaf_and_shared_scope() {
    let mut s = Session::default();
    let (parent_component, parent) = created(&mut s, "Parent");
    let (_, child) = created(&mut s, "Child");
    run(&mut s, "occurrence.move", json!({"occurrence":parent,"axis":[0,0,1],"angle":"90 deg","translate":[3,0,0],"capture":false}));
    run(&mut s, "occurrence.move", json!({"occurrence":child,"translate":[2,0,0],"capture":false}));
    let before = s.occurrence_placement(&[parent, child]).unwrap().world;
    run(&mut s, "occurrence.move", json!({"occurrence":child,"path":[parent,child],"space":"world","translate":[10,0,0],"capture":false}));
    let after = s.occurrence_placement(&[parent, child]).unwrap().world;
    assert!((after[3][0] - before[3][0] - 10.0).abs() < 1e-8);
    assert!((after[3][1] - before[3][1]).abs() < 1e-8);
    run(
        &mut s,
        "occurrence.move",
        json!({"occurrence":child,"path":[parent,child],"space":"world","axis":[0,0,1],"angle":"90 deg","origin":[0,0,0],"capture":false}),
    );
    let rotated = s.occurrence_placement(&[parent, child]).unwrap().world;
    assert!((rotated[3][0] + after[3][1]).abs() < 1e-8);
    assert!((rotated[3][1] - after[3][0]).abs() < 1e-8);
    run(&mut s, "component.activate", json!({"component":parent_component}));
    let second_parent = run(&mut s, "occurrence.copy", json!({"occurrence":parent,"translate":[30,0,0]}))["occurrence"].as_u64().unwrap();
    assert_eq!(s.occurrence_scope_count(&[second_parent, child]).unwrap(), 2);
    assert!(s.execute("occurrence.move", &json!({"occurrence":child,"space":"world","translate":[1,0,0]})).is_err());
}

#[test]
fn invalid_paths_are_atomic_for_session_state() {
    let mut s = Session::default();
    let (component, first) = created(&mut s, "Part");
    let second = run(&mut s, "occurrence.copy", json!({"occurrence":first}))["occurrence"].as_u64().unwrap();
    run(&mut s, "component.activate", json!({"component":component,"occurrence":second,"path":[second]}));
    run(&mut s, "occurrence.move", json!({"occurrence":second,"translate":[5,0,0],"capture":false}));
    run(&mut s, "browser.visibility", json!({"visible":false,"paths":[[second]]}));
    let pending = s.pending_moves.clone();
    let active = s.active_occurrence_path.clone();
    let vis = s.visibility.clone();
    let doc = s.doc.clone();
    for (cmd, p) in [
        ("occurrence.move", json!({"occurrence":second,"path":[first],"space":"world","translate":[10,0,0]})),
        ("occurrence.move", json!({"occurrence":second,"space":17,"translate":[10,0,0]})),
        ("component.activate", json!({"component":component,"occurrence":second,"path":[first,second]})),
        ("browser.visibility", json!({"visible":true,"paths":[[second],[9999]]})),
        ("select.set", json!({"items":[{"type":"occurrence","path":[first,second]}]})),
        ("occurrence.copy", json!({"occurrence":second,"path":[first]})),
    ] {
        assert!(s.execute(cmd, &p).is_err(), "{cmd}");
        assert_eq!(s.pending_moves, pending);
        assert_eq!(s.active_occurrence_path, active);
        assert_eq!(s.visibility, vis);
        assert_eq!(s.doc, doc);
    }
}

#[test]
fn exact_visibility_isolate_rename_and_async_accept() {
    let mut s = Session::default();
    let (component, first) = created(&mut s, "Part");
    run(&mut s, "solid.box", json!({"length":2,"width":2,"height":2}));
    let second = run(&mut s, "occurrence.copy", json!({"occurrence":first,"translate":[20,0,0]}))["occurrence"].as_u64().unwrap();
    let first_body = s.occurrence_bodies(&[first], true).unwrap()[0].clone();
    let second_body = s.occurrence_bodies(&[second], true).unwrap()[0].clone();
    run(&mut s, "browser.visibility", json!({"visible":false,"paths":[[second]]}));
    assert!(s.body_visible(&first_body));
    assert!(!s.body_visible(&second_body));
    let mut scratch = s.scratch();
    assert_eq!(scratch.visibility, s.visibility);
    run(&mut scratch, "component.rename", json!({"component":component,"name":"Changed"}));
    s.accept_computed(scratch);
    assert!(!s.occurrence_visible(&[second]));
    run(&mut s, "browser.visibility", json!({"visible":true,"paths":[[second]],"isolate":true}));
    assert!(!s.body_visible(&first_body));
    assert!(s.body_visible(&second_body));
    run(&mut s, "browser.visibility", json!({"visible":true,"folder":"bodies"}));
    assert!(s.body_visible(&first_body));
    assert!(s.body_visible(&second_body));
}

#[test]
fn synthesized_name_collision_keeps_source_and_instance_separate() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length":1,"width":1,"height":1}));
    let root = s.model.state().bodies[0].name.clone();
    let (_, first) = created(&mut s, "Part");
    run(&mut s, "solid.box", json!({"length":2,"width":2,"height":2}));
    let second = run(&mut s, "occurrence.copy", json!({"occurrence":first,"translate":[20,0,0]}))["occurrence"].as_u64().unwrap();
    let candidate = s.occurrence_bodies(&[second], false).unwrap()[0].clone();
    run(&mut s, "body.rename", json!({"body":root,"name":candidate}));
    let instance = s.occurrence_bodies(&[second], false).unwrap()[0].clone();
    assert_ne!(instance, candidate);
    let frames = s.doc.body_frames(&s.model.state());
    let world = s.world_state();
    assert!(frames.iter().all(|(name, _, _, _)| world.body(name).is_some()));
    assert_eq!(world.bodies.len(), world.bodies.iter().map(|b| &b.name).collect::<std::collections::BTreeSet<_>>().len());
    run(&mut s, "browser.visibility", json!({"visible":false,"paths":[[second]]}));
    assert!(s.body_visible(&candidate));
    assert!(!s.body_visible(&instance));
    let source = s.occurrence_placement(&[second]).unwrap().bodies[0].source.clone();
    run(&mut s, "material.assign", json!({"bodies":[instance],"material":"Brass"}));
    assert_eq!(s.doc.materials.get(&source).map(String::as_str), Some("Brass"));
    assert!(!s.doc.materials.contains_key(&candidate));
}

#[test]
fn duplicate_nested_labels_use_full_path_and_reopen_undo_preserve_placements() {
    let mut s = Session::default();
    let (parent_component, parent) = created(&mut s, "Parent");
    let (_, child) = created(&mut s, "Child");
    run(&mut s, "solid.box", json!({"length":1,"width":1,"height":1}));
    let parent2 = s.doc_mut().add_occurrence(parent_component, 0, solidcraft_doc::IDENTITY).unwrap();
    let parent3 = s.doc_mut().add_occurrence(parent_component, 0, solidcraft_doc::IDENTITY).unwrap();
    for o in s.doc_mut().occurrences.iter_mut().filter(|o| o.id == parent2 || o.id == parent3) {
        o.name = "Same".into();
    }
    s.refresh();
    let a = s.occurrence_bodies(&[parent2, child], false).unwrap()[0].clone();
    let b = s.occurrence_bodies(&[parent3, child], false).unwrap()[0].clone();
    assert_ne!(a, b);
    let mut reopened = Session::new(solidcraft_doc::format::read(&s.doc.to_json()).unwrap());
    assert_eq!(reopened.occurrence_bodies(&[parent2, child], false).unwrap()[0], a);
    assert_eq!(reopened.occurrence_bodies(&[parent3, child], false).unwrap()[0], b);
    run(&mut reopened, "selection.delete", json!({"occurrences":[parent3]}));
    assert!(reopened.occurrence_placement(&[parent3, child]).is_err());
    run(&mut reopened, "edit.undo", json!({}));
    assert!(reopened.occurrence_placement(&[parent3, child]).is_ok());
    assert!(reopened.occurrence_placement(&[parent, child]).is_ok());
}

#[test]
fn compact_duplicated_assembly_exact_resolution_and_expansion_limit() {
    let mut s = Session::default();
    let mut parent = 0;
    let mut path = Vec::new();
    for i in 0..18 {
        let component = s.doc_mut().add_component(Some(&format!("Level{i}")), parent).unwrap();
        let occurrence = s.doc.occurrence_of(component).unwrap().id;
        path.push(occurrence);
        s.doc_mut().add_occurrence(component, parent, solidcraft_doc::IDENTITY).unwrap();
        parent = component;
    }
    assert!(s.occurrence_placement(&path).is_ok());
    let top = [path[0]];
    assert!(s.occurrence_bodies(&top, true).is_err());
    assert!(s.execute("select.set", &json!({"items":[{"type":"occurrence","path":top}]})).is_err());
    assert!(s.selection.is_empty());
}

#[test]
fn identity_instances_map_world_keys_to_shared_sources() {
    let mut s = Session::default();
    let (_, first) = created(&mut s, "Part");
    run(&mut s, "solid.box", json!({"length":2,"width":2,"height":2}));
    let second = run(&mut s, "occurrence.copy", json!({"occurrence":first}))["occurrence"].as_u64().unwrap();
    let placement = s.occurrence_placement(&[second]).unwrap();
    let body = &placement.bodies[0];
    assert_ne!(body.source, body.world);
    run(&mut s, "material.assign", json!({"bodies":[body.world],"material":"Brass"}));
    assert_eq!(s.doc.materials.get(&body.source).map(String::as_str), Some("Brass"));
}

#[test]
fn dialog_baseline_move_preserves_other_pending_positions_without_drift() {
    let mut s = Session::default();
    let (_, first) = created(&mut s, "Part");
    let second = run(&mut s, "occurrence.copy", json!({"occurrence":first,"translate":[20,0,0]}))["occurrence"].as_u64().unwrap();
    run(&mut s, "occurrence.move", json!({"occurrence":first,"translate":[5,0,0],"capture":false}));
    run(&mut s, "occurrence.move", json!({"occurrence":second,"translate":[3,0,0],"capture":false}));
    let baseline = s.occurrence_placement(&[second]).unwrap().local;
    for offset in [10.0, 15.0, 0.0] {
        run(
            &mut s,
            "occurrence.move",
            json!({"occurrence":second,"path":[second],"space":"world","base_transform":baseline,"translate":[offset,0,0],"capture":false}),
        );
        assert_eq!(s.pending_moves.get(&first).unwrap()[3][0], 5.0);
        assert_eq!(s.pending_moves.get(&second).unwrap()[3][0], 23.0 + offset);
    }
    let pending = s.pending_moves.clone();
    assert!(s.execute("occurrence.move", &json!({"occurrence":second,"base_transform":[[0,0,0,0],[0,0,0,0],[0,0,0,0],[0,0,0,1]]})).is_err());
    assert_eq!(s.pending_moves, pending);
    for base in [
        [[2.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]],
        [[-1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]],
    ] {
        assert!(s.execute("occurrence.move", &json!({"occurrence":second,"base_transform":base})).is_err());
        assert_eq!(s.pending_moves, pending);
    }
    assert!(s.execute("component.revert_position", &json!({"occurrences":[second,9999]})).is_err());
    assert_eq!(s.pending_moves, pending);
    run(&mut s, "component.revert_position", json!({"occurrences":[second]}));
    assert!(!s.pending_moves.contains_key(&second));
    assert_eq!(s.pending_moves.get(&first).unwrap()[3][0], 5.0);
}

#[test]
fn hostile_moves_copies_and_reused_ids_do_not_leak_state() {
    let mut s = Session::default();
    let (_, first) = created(&mut s, "Part");
    run(&mut s, "occurrence.move", json!({"occurrence":first,"translate":[10,0,0],"capture":false}));
    let pending = s.pending_moves.clone();
    let doc = s.doc.clone();
    for (cmd, p) in [
        ("occurrence.move", json!({"occurrence":first,"origin":[1e308,0,0],"angle":"180 deg"})),
        ("occurrence.move", json!({"occurrence":first,"translate":[1e9,0,0],"capture":false})),
        ("occurrence.move", json!({"occurrence":first,"translate":[1,2]})),
        ("occurrence.move", json!({"occurrence":first,"angle":false})),
        ("occurrence.move", json!({"occurrence":first,"axis":[1e308,0,0],"angle":"90 deg"})),
        ("occurrence.move", json!({"occurrence":first,"axis":[0,0,0],"angle":"90 deg"})),
        ("occurrence.copy", json!({"occurrence":first,"translate":[1e9,0,0]})),
        ("occurrence.copy", json!({"occurrence":first,"translate":"oops"})),
    ] {
        assert!(s.execute(cmd, &p).is_err(), "{cmd}");
        assert_eq!(s.pending_moves, pending);
        assert_eq!(s.doc, doc);
    }
    run(&mut s, "edit.undo", json!({}));
    assert!(s.pending_moves.is_empty());
    let (_, other) = created(&mut s, "Other");
    assert_eq!(first, other);
    assert_eq!(s.occurrence_placement(&[other]).unwrap().world[3][0], 0.0);
    run(&mut s, "occurrence.move", json!({"occurrence":other,"translate":[20,0,0],"capture":false}));
    run(&mut s, "component.capture_position", json!({}));
    assert!(s.pending_moves.is_empty());
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(s.pending_moves.get(&other).unwrap()[3][0], 20.0);
    assert_eq!(s.doc.occurrences.iter().find(|occurrence| occurrence.id == other).unwrap().transform[3][0], 0.0);
    assert_eq!(s.occurrence_placement(&[other]).unwrap().world[3][0], 20.0);
    run(&mut s, "edit.redo", json!({}));
    assert!(s.pending_moves.is_empty());
    assert_eq!(s.doc.occurrences.iter().find(|occurrence| occurrence.id == other).unwrap().transform[3][0], 20.0);
}

#[test]
fn isolate_nested_child_hides_ancestor_material_and_sibling() {
    let mut s = Session::default();
    let (_, parent) = created(&mut s, "Parent");
    run(&mut s, "solid.box", json!({"length":10,"width":10,"height":10}));
    let (child_component, child) = created(&mut s, "Child");
    run(&mut s, "solid.box", json!({"length":1,"width":1,"height":1}));
    let parent_component = s.doc.occurrences.iter().find(|o| o.id == child).unwrap().parent;
    let sibling = s
        .doc_mut()
        .add_occurrence(
            child_component,
            parent_component,
            solidcraft_doc::rigid(crate::geom::Vec3::new(20.0, 0.0, 0.0), crate::geom::Vec3::ZERO, crate::geom::Vec3::Z, 0.0),
        )
        .unwrap();
    s.refresh();
    run(&mut s, "browser.visibility", json!({"visible":true,"paths":[[parent,child]],"isolate":true}));
    for (path, expected) in [(vec![parent], false), (vec![parent, child], true), (vec![parent, sibling], false)] {
        let body = s.occurrence_bodies(&path, false).unwrap()[0].clone();
        assert_eq!(s.body_visible(&body), expected);
    }
}

#[test]
fn context_new_child_uses_inactive_parent_placement() {
    let mut s = Session::default();
    let (parent_component, parent) = created(&mut s, "Parent");
    let second = run(&mut s, "occurrence.copy", json!({"occurrence":parent,"translate":[20,0,0]}))["occurrence"].as_u64().unwrap();
    run(&mut s, "component.activate", json!({"component":0}));
    let child =
        run(&mut s, "component.create", json!({"parent":parent_component,"parent_path":[second],"name":"Child"}))["occurrence"].as_u64().unwrap();
    assert_eq!(s.active_occurrence_path, Some(vec![second, child]));
    assert_eq!(s.active_component_frame()[3][0], 20.0);
    let doc = s.doc.clone();
    let active = s.active_occurrence_path.clone();
    assert!(s.execute("component.create", &json!({"parent":parent_component,"parent_path":[999],"name":"Invalid"})).is_err());
    assert_eq!(s.doc, doc);
    assert_eq!(s.active_occurrence_path, active);
}

#[test]
fn paste_new_uses_selected_pending_placement() {
    let mut s = Session::default();
    let (component, first) = created(&mut s, "Part");
    run(&mut s, "solid.box", json!({"length":1,"width":1,"height":1}));
    let second = run(&mut s, "occurrence.copy", json!({"occurrence":first,"translate":[20,0,0]}))["occurrence"].as_u64().unwrap();
    run(&mut s, "occurrence.move", json!({"occurrence":second,"translate":[30,0,0],"capture":false}));
    let copied =
        run(&mut s, "component.paste_new", json!({"component":component,"occurrence":second,"path":[second],"translate":[5,0,0]}))["component"]
            .as_u64()
            .unwrap();
    assert_eq!(s.doc.occurrence_of(copied).unwrap().transform[3][0], 55.0);
    let result = run(&mut s, "component.properties", json!({"component":copied}));
    near(&result["total"]["center_of_mass"][0], 55.5);
    let result = run(&mut s, "occurrence.properties", json!({"occurrence":first,"path":[first]}));
    near(&result["total"]["center_of_mass"][0], 0.5);
}

#[test]
fn instance_styles_follow_source_identity_after_collision_rename_and_pending_move() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length":1,"width":1,"height":1}));
    let root = s.model.state().bodies[0].name.clone();
    let (component, first) = created(&mut s, "Part");
    run(&mut s, "solid.box", json!({"length":2,"width":2,"height":2}));
    let second = run(&mut s, "occurrence.copy", json!({"occurrence":first,"translate":[20,0,0]}))["occurrence"].as_u64().unwrap();
    let candidate = s.occurrence_bodies(&[second], false).unwrap()[0].clone();
    run(&mut s, "body.rename", json!({"body":root,"name":candidate}));
    let instance = s.occurrence_bodies(&[second], false).unwrap()[0].clone();
    let source = s.occurrence_placement(&[second]).unwrap().bodies[0].source.clone();
    run(&mut s, "material.assign", json!({"bodies":[instance],"material":"Brass"}));
    let world = s.world_state();
    assert_eq!(s.doc.body_color(world.body(&instance).unwrap()), s.doc.body_color(world.body(&source).unwrap()));
    run(&mut s, "appearance.assign", json!({"components":[component.to_string()],"color":"#0000ff","opacity":0.7}));
    assert_eq!(s.doc.body_color(s.world_state().body(&instance).unwrap()), Some([0.0, 0.0, 1.0]));
    run(&mut s, "appearance.assign", json!({"bodies":[instance],"color":"#ff0000","opacity":0.5}));
    run(&mut s, "appearance.assign", json!({"faces":[{"body":instance,"point":[21,1,2]}],"color":"#00ff00","opacity":0.4}));
    // Exercise the point-based face fallback as well as persistent face-name matching.
    for face in &mut s.doc_mut().appearances.faces {
        face.name = None;
    }
    run(&mut s, "body.rename", json!({"body":source,"name":"Shared Renamed"}));
    run(&mut s, "occurrence.move", json!({"occurrence":second,"translate":[5,0,0],"capture":false}));
    let world = s.world_state();
    for id in [first, second] {
        let bodyname = s.occurrence_bodies(&[id], false).unwrap()[0].clone();
        let body = world.body(&bodyname).unwrap();
        assert_eq!(body.definition_name(), "Shared Renamed");
        assert_eq!(body.component, component);
        let look = s.doc.model_body_look(body).unwrap();
        assert_eq!(look.color, [255, 0, 0]);
        assert_eq!(look.opacity, 0.5);
        assert!(s.doc.face_colors(body).iter().any(|(_, l)| l.color == [0, 255, 0] && l.opacity == 0.4));
        let measured = run(&mut s, "inspect.measure", json!({"bodies":[bodyname]}));
        assert_eq!(measured["bodies"][0]["material"], "Brass");
        assert!((measured["bodies"][0]["mass_g"].as_f64().unwrap() - 0.068).abs() < 1e-8);
    }
    let definition = s.model.state();
    assert_eq!(solidcraft_kernel::measure(&definition.body("Shared Renamed").unwrap().body).unwrap().bbox.min, crate::geom::Vec3::ZERO);
    let painted = s.doc.painted(&world);
    let secondname = s.occurrence_bodies(&[second], false).unwrap()[0].clone();
    let body = painted.body(&secondname).unwrap();
    assert_eq!(body.body.color(), Some([1.0, 0.0, 0.0]));
    assert!(body.body.paint().unwrap().faces.iter().any(|f| f.color == [0.0, 1.0, 0.0]));
    let bytes = solidcraft_io::export(&painted, &[secondname], solidcraft_io::Format::Step, "InstanceStyles").unwrap();
    let imported = solidcraft_kernel::step_import(&String::from_utf8(bytes).unwrap()).unwrap();
    assert_eq!(imported.bodies.len(), 1);
    assert_eq!(imported.bodies[0].body.color(), Some([1.0, 0.0, 0.0]));
    assert!(imported.bodies[0].body.paint().unwrap().faces.iter().any(|f| f.color == [0.0, 1.0, 0.0]));
}

/// A design file path in a fresh temporary folder (removed by the caller).
fn temp_design(tag: &str, ext: &str) -> (std::path::PathBuf, String) {
    let dir = std::env::temp_dir().join(format!("solidcraft-visibility-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(format!("design.{ext}")).to_string_lossy().to_string();
    (dir, path)
}

/// #531: hiding a placement and a body is saved with the design and comes back when it opens.
/// Each hide is an undo step of its own (#2219, so undoing a later feature still leaves it
/// hidden), the design then has unsaved changes, and showing it again is saved the same way.
#[test]
fn hidden_placements_and_bodies_are_saved_with_the_design() {
    let (dir, path) = temp_design("saved", "solidcraft");
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length":4,"width":4,"height":4,"body_name":"Base"}));
    let (component, first) = created(&mut s, "Part");
    run(&mut s, "solid.box", json!({"length":2,"width":2,"height":2}));
    let second = run(&mut s, "occurrence.copy", json!({"occurrence":first,"translate":[20,0,0]}))["occurrence"].as_u64().unwrap();
    run(&mut s, "component.activate", json!({"component":0}));
    run(&mut s, "file.save", json!({"path": path}));
    assert!(!s.is_dirty());

    let undo = s.undo.len();
    run(&mut s, "browser.visibility", json!({"visible":false,"paths":[[second]]}));
    run(&mut s, "browser.visibility", json!({"visible":false,"items":["Base"]}));
    assert_eq!(s.undo.len(), undo + 2, "each hide is one undo step");
    assert!(s.is_dirty(), "hiding leaves the design with unsaved changes");
    run(&mut s, "solid.box", json!({"length":1,"width":1,"height":1,"corner":[50,0,0],"operation":"new","body_name":"Later"}));
    run(&mut s, "edit.undo", json!({}));
    assert!(!s.body_visible("Base") && !s.occurrence_visible(&[second]), "undo keeps the hiding");

    run(&mut s, "file.save", json!({}));
    assert!(!s.is_dirty());
    run(&mut s, "file.new", json!({}));
    run(&mut s, "doc.open", json!({"path": path}));
    assert!(!s.body_visible("Base"), "the hidden body stays hidden");
    assert!(!s.occurrence_visible(&[second]), "the hidden placement stays hidden");
    assert!(s.occurrence_visible(&[first]));
    assert!(s.doc.components.iter().any(|c| c.id == component));
    assert!(!s.is_dirty(), "an opened design has no unsaved changes");
    assert!(s.undo.is_empty());

    run(&mut s, "browser.visibility", json!({"visible":true,"folder":"bodies"}));
    assert!(s.is_dirty());
    run(&mut s, "file.save", json!({}));
    run(&mut s, "file.new", json!({}));
    run(&mut s, "doc.open", json!({"path": path}));
    assert!(s.body_visible("Base") && s.occurrence_visible(&[second]), "showing again is saved too");
    let _ = std::fs::remove_dir_all(&dir);
}

/// #531: designs saved before visibility was kept (no `visibility` in the file) open with
/// everything shown, and a design with nothing hidden saves exactly as before.
#[test]
fn designs_without_saved_visibility_open_unchanged() {
    let (dir, path) = temp_design("legacy", "json");
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length":4,"width":4,"height":4,"body_name":"Base"}));
    let mut legacy: Value = serde_json::from_slice(&solidcraft_io::write_design(&s.doc)).unwrap();
    assert!(legacy.get("visibility").is_none(), "nothing hidden: no visibility in the file");
    legacy["format"] = json!("solidcraft/15");
    std::fs::write(&path, serde_json::to_vec(&legacy).unwrap()).unwrap();

    run(&mut s, "file.new", json!({}));
    run(&mut s, "doc.open", json!({"path": path}));
    assert!(s.body_visible("Base"));
    assert_eq!(s.visibility, crate::Visibility::default());
    assert!(!s.is_dirty());
    run(&mut s, "file.save", json!({}));
    let saved: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert!(saved.get("visibility").is_none());

    run(&mut s, "browser.visibility", json!({"visible":false,"items":["Base"]}));
    run(&mut s, "file.save", json!({}));
    let saved: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(saved["visibility"]["hidden_bodies"], json!(["Base"]));
    let _ = std::fs::remove_dir_all(&dir);
}

/// Command names for linking another design are a proposal (#533); adjust them to the real API.
const INSERT_DESIGN: &str = "component.insert_design";
const UPDATE_LINKS: &str = "component.update_links";

fn saved_box_design(path: &std::path::Path, height: f64) {
    let mut part = Session::default();
    run(&mut part, "solid.box", json!({"length": 10, "width": 20, "height": height, "body_name": "Plate"}));
    run(&mut part, "file.save", json!({"path": path}));
}

#[test]
fn a_linked_design_brings_its_bodies_in_and_follows_edits_to_the_part_file() {
    let dir = std::env::temp_dir().join(format!("solidcraft-linked-design-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let part = dir.join("plate.solidcraft");
    // Given a part saved in its own design: a 10 x 20 x 30 plate.
    saved_box_design(&part, 30.0);
    // When an assembly inserts it as a linked component, the plate's body is in the assembly.
    let mut assembly = Session::default();
    let inserted = assembly.execute(INSERT_DESIGN, &json!({"path": part, "translate": [50, 0, 0]}));
    let inserted = inserted.unwrap_or_else(|e| panic!("{INSERT_DESIGN}: {e}"));
    let occurrence = inserted["occurrence"].as_u64().unwrap_or_else(|| panic!("no occurrence in {inserted}"));
    let measured = run(&mut assembly, "occurrence.properties", json!({"occurrence": occurrence, "path": [occurrence]}));
    near(&measured["total"]["volume_mm3"], 6000.0);
    near(&measured["total"]["center_of_mass"][0], 55.0);
    // When the part is edited (30 -> 45 tall) and saved again, updating the links follows it.
    saved_box_design(&part, 45.0);
    run(&mut assembly, UPDATE_LINKS, json!({}));
    let measured = run(&mut assembly, "occurrence.properties", json!({"occurrence": occurrence, "path": [occurrence]}));
    near(&measured["total"]["volume_mm3"], 9000.0);
    near(&measured["total"]["center_of_mass"][0], 55.0);
    let _ = std::fs::remove_dir_all(&dir);
}

/// #5679: a linked part's bodies come from its design file, so a cut or join from the top level
/// on a sketch on its face is refused with the reason, and the plate stays as the file has it.
#[test]
fn a_cut_or_join_from_the_top_level_into_a_linked_part_is_refused() {
    let dir = std::env::temp_dir().join(format!("solidcraft-linked-cut-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let part = dir.join("plate.solidcraft");
    saved_box_design(&part, 30.0);
    let mut s = Session::default();
    run(&mut s, INSERT_DESIGN, json!({"path": part, "translate": [50, 0, 0]}));
    let _ = std::fs::remove_dir_all(&dir);
    run(&mut s, "component.activate", json!({"component": "root"}));
    run(&mut s, "sketch.create", json!({"plane": {"face": [55, 10, 30]}, "project_edges": false}));
    let plane = s
        .doc
        .features
        .iter()
        .rev()
        .find_map(|f| match &f.kind {
            solidcraft_doc::FeatureKind::Sketch { plane: solidcraft_doc::PlaneRef::Face { plane, .. }, .. } => Some(*plane),
            _ => None,
        })
        .unwrap();
    let c = plane.to_local(solidcraft_geom::Vec3::new(55.0, 10.0, 30.0));
    run(&mut s, "sketch.circle.center", json!({"center": [c.x, c.y], "radius": 2}));
    run(&mut s, "sketch.finish", json!({}));
    let features = s.doc.features.len();
    for (operation, distance) in [("cut", -5), ("join", 5)] {
        let refused = s.execute("solid.extrude", &json!({"distance": distance, "operation": operation})).unwrap_err().to_string();
        assert!(refused.contains("`Plate`") && refused.contains("linked to a design file"), "{operation}: {refused}");
    }
    assert_eq!(s.doc.features.len(), features);
    let volume = run(&mut s, "inspect.measure", json!({"bodies": ["Plate"]}))["total"]["volume_mm3"].clone();
    near(&volume, 6000.0);
}

/// The compact list counts what a component holds and leaves out identity transforms; the full
/// list stays as it was.
#[test]
fn compact_component_list_counts_and_skips_identity_transforms() {
    let mut s = Session::default();
    let (component, first) = created(&mut s, "Block");
    run(&mut s, "solid.box", json!({"length": 1, "width": 1, "height": 1, "body_name": "Cube"}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    let second = run(&mut s, "occurrence.copy", json!({"component": component, "translate": [20, 0, 0]}))["occurrence"].as_u64().unwrap();
    let full = run(&mut s, "component.list", json!({}));
    let block = |l: &Value| l["components"].as_array().unwrap().iter().find(|c| c["id"] == component).unwrap().clone();
    assert_eq!(block(&full)["bodies"], json!(["Cube"]));
    assert!(block(&full)["occurrences"].as_array().unwrap().iter().all(|o| o["transform"].is_array()));
    let compact = run(&mut s, "component.list", json!({"compact": true}));
    let b = block(&compact);
    assert_eq!((&b["bodies"], &b["features"], &b["sketches"]), (&json!(1), &json!(1), &json!(0)), "{b}");
    let placed = |id: u64| b["occurrences"].as_array().unwrap().iter().find(|o| o["id"] == id).unwrap().clone();
    assert!(placed(first).get("transform").is_none(), "{b}");
    assert_eq!(placed(second)["transform"][3][0], 20.0, "{b}");
    let root = compact["components"].as_array().unwrap().iter().find(|c| c["id"] == 0).unwrap().clone();
    assert_eq!(root["children"], json!([first, second]), "{root}");
    // Body names on request.
    assert_eq!(block(&run(&mut s, "component.list", json!({"compact": true, "bodies": true})))["bodies"], json!(["Cube"]));
}

/// Each component lists only what it holds, in timeline order: its bodies, features and
/// sketches, its own occurrences and the occurrences inside it, through nested components.
#[test]
fn component_list_gives_each_component_only_its_own_contents() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 5, "width": 5, "height": 5, "name": "Base", "body_name": "Base"}));
    let (arm, arm_first) = created(&mut s, "Arm");
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Arm sketch"}));
    run(&mut s, "solid.box", json!({"corner": [0, 20, 0], "length": 2, "width": 2, "height": 2, "name": "Lug", "body_name": "Lug"}));
    let pin = run(&mut s, "component.create", json!({"name": "Pin", "parent": arm, "parent_path": [arm_first]}));
    let (pin, pin_first) = (pin["component"].as_u64().unwrap(), pin["occurrence"].as_u64().unwrap());
    run(&mut s, "solid.box", json!({"corner": [0, 40, 0], "length": 1, "width": 1, "height": 1, "name": "Shank", "body_name": "Shank"}));
    run(&mut s, "solid.box", json!({"corner": [0, 50, 0], "length": 1, "width": 1, "height": 1, "name": "Head", "body_name": "Head"}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    let arm_second = run(&mut s, "occurrence.copy", json!({"component": arm, "translate": [30, 0, 0]}))["occurrence"].as_u64().unwrap();
    let list = run(&mut s, "component.list", json!({}));
    let of = |id: u64| list["components"].as_array().unwrap().iter().find(|c| c["id"] == id).unwrap().clone();
    let ids = |v: &Value| v.as_array().unwrap().iter().map(|o| o["id"].as_u64().unwrap()).collect::<Vec<_>>();
    let (root, arm_node, pin_node) = (of(0), of(arm), of(pin));
    assert_eq!((&root["bodies"], &root["features"], &root["sketches"]), (&json!(["Base"]), &json!(["Base"]), &json!([])), "{root}");
    assert_eq!(ids(&root["children"]), vec![arm_first, arm_second], "{root}");
    assert_eq!(
        (&arm_node["bodies"], &arm_node["features"], &arm_node["sketches"]),
        (&json!(["Lug"]), &json!(["Lug"]), &json!(["Arm sketch"])),
        "{arm_node}"
    );
    assert_eq!((ids(&arm_node["occurrences"]), ids(&arm_node["children"])), (vec![arm_first, arm_second], vec![pin_first]), "{arm_node}");
    assert_eq!((&pin_node["bodies"], &pin_node["features"]), (&json!(["Shank", "Head"]), &json!(["Shank", "Head"])), "{pin_node}");
    assert_eq!((ids(&pin_node["occurrences"]), ids(&pin_node["children"])), (vec![pin_first], vec![]), "{pin_node}");
}

/// A script's 3x4 placement matrix (rotation rows, then the translation column) moves an
/// occurrence exactly as the same turn and move given as an axis, an angle and a translation.
#[test]
fn matrix_move_turns_a_quarter_and_moves() {
    let mut s = Session::default();
    let (_, occ) = created(&mut s, "Block");
    run(&mut s, "solid.box", json!({"length": 2, "width": 1, "height": 1}));
    // 90 degrees about Z, then (10, 20, 30): (x, y, z) goes to (10 - y, 20 + x, 30 + z).
    let r = run(&mut s, "occurrence.move", json!({"occurrence": occ, "matrix": [0, -1, 0, 10, 1, 0, 0, 20, 0, 0, 1, 30]}));
    let t = &r["transform"];
    for (col, expected) in [[0.0, 1.0, 0.0, 0.0], [-1.0, 0.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [10.0, 20.0, 30.0, 1.0]].iter().enumerate() {
        for (row, x) in expected.iter().enumerate() {
            near(&t[col][row], *x);
        }
    }
    // The box's centre (1, 0.5, 0.5) lands at (9.5, 21, 30.5).
    let props = run(&mut s, "occurrence.properties", json!({"occurrence": occ}));
    for (i, x) in [9.5, 21.0, 30.5].into_iter().enumerate() {
        near(&props["total"]["center_of_mass"][i], x);
    }
    // The same as axis/angle/translate, and the 4x4 form as rows says the same.
    let mut other = Session::default();
    let (_, o2) = created(&mut other, "Block");
    let r2 = run(&mut other, "occurrence.move", json!({"occurrence": o2, "axis": [0, 0, 1], "angle": "90 deg", "translate": [10, 20, 30]}));
    let mut rows = Session::default();
    let (_, o3) = created(&mut rows, "Block");
    let r3 = run(&mut rows, "occurrence.move", json!({"occurrence": o3, "matrix": [[0, -1, 0, 10], [1, 0, 0, 20], [0, 0, 1, 30], [0, 0, 0, 1]]}));
    for col in 0..4 {
        for row in 0..4 {
            near(&r2["transform"][col][row], t[col][row].as_f64().unwrap());
            near(&r3["transform"][col][row], t[col][row].as_f64().unwrap());
        }
    }
}

/// #3223, #2872: a STEP assembly whose part sits two levels down, each level placed, opened and
/// stood up. With the imported root active, as Open leaves it, or the top level, a circle sketched
/// on the part's top face cuts it through, reads as a Cut dragged in and a Join dragged out, and
/// joins it in one piece instead of starting a body of the active component's own.
#[test]
fn a_stood_up_nested_step_part_is_cut_and_joined_from_its_face() {
    let dir = std::env::temp_dir().join(format!("solidcraft-nested-stood-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("nested.step");
    let mut src = Session::default();
    let (_, carrier) = created(&mut src, "Carrier");
    let (part, nested) = created(&mut src, "Nested Part");
    run(&mut src, "solid.box", json!({"length": 40, "width": 24, "height": 10, "body_name": "NestedBlock"}));
    run(&mut src, "occurrence.move", json!({"occurrence": carrier, "translate": [12, 7, 5], "axis": [0, 0, 1], "angle": "30 deg"}));
    run(&mut src, "occurrence.move", json!({"occurrence": nested, "translate": [3, 11, 2], "axis": [0, 1, 0], "angle": "20 deg"}));
    // The middle of the top face, in the file's frame: the imported root's.
    let top = solidcraft_doc::apply_point(&src.doc.component_transform(part), solidcraft_geom::Vec3::new(20.0, 12.0, 10.0));
    run(&mut src, "file.export", json!({"path": path, "format": "step"}));

    // #5679: the same from the top level, where the part is two moved and turned levels down
    // (#5678 was this cut silently doing nothing; `auto` read it as a new body).
    for top_level in [false, true] {
        let mut s = Session::default();
        run(&mut s, "doc.open", json!({"path": path, "up": "y"}));
        let at = solidcraft_doc::apply_point(&s.doc.component_transform(s.active_component), top);
        if top_level {
            run(&mut s, "component.activate", json!({"component": "root"}));
        }
        // The sketch is in the active component's frame: the imported root's, or the world's.
        let local = if top_level { at } else { top };
        run(&mut s, "sketch.create", json!({"plane": {"face": [at.x, at.y, at.z]}, "project_edges": false}));
        let centre = s
            .doc
            .features
            .iter()
            .rev()
            .find_map(|f| match &f.kind {
                solidcraft_doc::FeatureKind::Sketch { plane: solidcraft_doc::PlaneRef::Face { plane, .. }, .. } => Some(plane.to_local(local)),
                _ => None,
            })
            .unwrap();
        run(&mut s, "sketch.circle.center", json!({"center": [centre.x, centre.y], "radius": 3}));
        run(&mut s, "sketch.finish", json!({}));
        assert_eq!(crate::auto_operation(&s, &json!({"distance": -5})), Some("cut"), "top level {top_level}");
        assert_eq!(crate::auto_operation(&s, &json!({"distance": 5})), Some("join"), "top level {top_level}");
        let volume = |s: &mut Session| run(s, "inspect.measure", json!({"bodies": ["NestedBlock"]}))["total"]["volume_mm3"].as_f64().unwrap();
        let block = 40.0 * 24.0 * 10.0;
        let hole = std::f64::consts::PI * 3.0 * 3.0;

        // The circle is measured as faceted: within a thousandth of what it removes or adds.
        for operation in ["cut", "auto"] {
            run(&mut s, "solid.extrude", json!({"distance": -20, "operation": operation}));
            assert!((block - volume(&mut s) - hole * 10.0).abs() < 1e-3 * hole * 10.0, "{top_level} {operation}: {}", volume(&mut s));
            run(&mut s, "edit.undo", json!({}));
        }
        for operation in ["join", "auto"] {
            run(&mut s, "solid.extrude", json!({"distance": 5, "operation": operation}));
            let st = s.model.state();
            assert_eq!(st.bodies.iter().map(|b| b.name.as_str()).collect::<Vec<_>>(), ["NestedBlock"], "{top_level} {operation}");
            assert_eq!(st.body("NestedBlock").map(|b| b.body.shell_count()), Some(1), "{top_level} {operation}");
            assert!((volume(&mut s) - block - hole * 5.0).abs() < 1e-3 * hole * 5.0, "{top_level} {operation}: {}", volume(&mut s));
            run(&mut s, "edit.undo", json!({}));
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// A rotation that is one only up to rounding is orthonormalised; a scale, a shear or a mirror
/// is refused with the reason, and the occurrence stays where it was.
#[test]
fn matrix_move_orthonormalises_rounding_and_refuses_what_is_not_rigid() {
    let mut s = Session::default();
    let (_, occ) = created(&mut s, "Block");
    let (sin, cos) = 30f64.to_radians().sin_cos();
    let e = 3e-10;
    let nearly = json!([cos + e, -sin, 0, 5, sin, cos - e, e, 0, 0, -e, 1.0 + e, 0]);
    let r = run(&mut s, "occurrence.move", json!({"occurrence": occ, "matrix": nearly, "space": "world"}));
    let m: solidcraft_doc::Mat = serde_json::from_value(r["transform"].clone()).unwrap();
    for i in 0..3 {
        for j in 0..3 {
            let dot: f64 = (0..3).map(|k| m[i][k] * m[j][k]).sum();
            assert!((dot - f64::from(u8::from(i == j))).abs() < 1e-15, "column {i}·{j} = {dot}");
        }
    }
    assert!((m[0][0] - cos).abs() < 1e-9 && (m[0][1] - sin).abs() < 1e-9 && (m[3][0] - 5.0).abs() < 1e-12, "{m:?}");

    let before = s.doc.occurrences.iter().find(|o| o.id == occ).unwrap().transform;
    let refused = |s: &mut Session, p: Value| s.execute("occurrence.move", &p).unwrap_err().to_string();
    let scaled = refused(&mut s, json!({"occurrence": occ, "matrix": [1.001, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0]}));
    assert!(scaled.contains("not rigid") && scaled.contains("2.00"), "{scaled}");
    let sheared = refused(&mut s, json!({"occurrence": occ, "matrix": [1, 0.01, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0]}));
    assert!(sheared.contains("not rigid"), "{sheared}");
    let mirrored = refused(&mut s, json!({"occurrence": occ, "matrix": [-1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0]}));
    assert!(mirrored.contains("solid.mirror"), "{mirrored}");
    let projective = refused(&mut s, json!({"occurrence": occ, "matrix": [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0.5, 0, 0, 1]}));
    assert!(projective.contains("fourth row"), "{projective}");
    // A looser tolerance is the caller's to ask for, up to 1e-3.
    let rounded = json!([0.866, -0.5, 0, 0, 0.5, 0.866, 0, 0, 0, 0, 1, 0]);
    assert!(refused(&mut s, json!({"occurrence": occ, "matrix": rounded})).contains("not rigid"));
    for p in [
        json!({"occurrence": occ, "matrix": rounded, "tolerance": 1}),
        json!({"occurrence": occ, "matrix": [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1], "translate": [1, 0, 0]}),
        json!({"occurrence": occ, "matrix": [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0], "translate": [1, 0, 0]}),
        json!({"occurrence": occ, "matrix": [1, 0, 0, 1e12, 0, 1, 0, 0, 0, 0, 1, 0]}),
        json!({"occurrence": occ, "matrix": [[1, 0, 0, 0], [0, 1, 0, 0]]}),
        json!({"occurrence": occ, "matrix": "identity"}),
        json!({"occurrence": occ, "matrix": [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]}),
    ] {
        assert!(s.execute("occurrence.move", &p).is_err(), "{p}");
    }
    assert_eq!(s.doc.occurrences.iter().find(|o| o.id == occ).unwrap().transform, before);
    run(&mut s, "occurrence.move", json!({"occurrence": occ, "matrix": rounded, "tolerance": 1e-3}));
}

/// #3383: a combine across components meets the tool where it is shown, not where its numbers
/// would be in the target's own frame. The plate in a component turned a quarter about X sits at
/// y -10..0, z -20..20; the boss in the root overlaps it by 10 x 5 x 5. The reverse (a peg in the
/// turned component cut from a root plate) removes 10 x 10 x 5 of it, not 10 x 10 x 10.
#[test]
fn combine_across_a_turned_occurrence_meets_the_tool_where_it_is_shown() {
    let volume = |s: &Session, name: &str| s.model.state().body(name).map(|b| solidcraft_kernel::measure(&b.body).unwrap().volume).unwrap_or(0.0);
    let turned = |s: &mut Session, body: Value| {
        created(s, "Part");
        run(s, "solid.box", body);
        run(s, "component.activate", json!({"component": "root"}));
        run(s, "occurrence.move", json!({"occurrence": "Part:1", "axis": [1, 0, 0], "angle": "90 deg"}));
        run(s, "component.capture_position", json!({}));
    };
    let mut s = Session::default();
    turned(&mut s, json!({"length": 40, "width": 40, "height": 10, "corner": [-20, -20, 0], "body_name": "Plate"}));
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [-5, -5, -25], "operation": "new", "body_name": "Boss"}));
    run(&mut s, "solid.combine", json!({"target": "Plate", "tools": ["Boss"]}));
    assert_eq!(s.model.state().body("Plate").map(|b| b.body.shell_count()), Some(1));
    let want = 40.0 * 40.0 * 10.0 + 10.0 * 10.0 * 10.0 - 10.0 * 5.0 * 5.0;
    assert!((volume(&s, "Plate") - want).abs() < 1e-6 * want, "{} vs {want}", volume(&s, "Plate"));

    let mut s = Session::default();
    turned(&mut s, json!({"length": 10, "width": 10, "height": 10, "corner": [-5, -5, 0], "body_name": "Peg"}));
    run(&mut s, "solid.box", json!({"length": 40, "width": 40, "height": 10, "corner": [-20, -20, 0], "operation": "new", "body_name": "Base"}));
    run(&mut s, "solid.combine", json!({"target": "Base", "tools": ["Peg"], "operation": "cut"}));
    let want = 40.0 * 40.0 * 10.0 - 10.0 * 10.0 * 5.0;
    assert!((volume(&s, "Base") - want).abs() < 1e-6 * want, "{} vs {want}", volume(&s, "Base"));
}

/// #3383: a feature joining into a body of another, placed component draws its tool where the
/// tool is shown, as Combine does. Part is moved 100 mm along x, so its 10 mm cubes sit at x 100..110
/// and 120..130; a box and an extrude drawn from the root over each add 4 x 4 x 5 to it in one
/// piece, not a separate lump 100 mm further on. A through-all cut is as long as the target is
/// far where it is shown.
#[test]
fn joins_into_a_moved_occurrence_meet_the_tool_where_it_is_shown() {
    let volume = |s: &Session, name: &str| s.model.state().body(name).map(|b| solidcraft_kernel::measure(&b.body).unwrap().volume).unwrap_or(0.0);
    let mut s = Session::default();
    created(&mut s, "Part");
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "body_name": "A"}));
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [20, 0, 0], "operation": "new", "body_name": "B"}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    run(&mut s, "occurrence.move", json!({"occurrence": "Part:1", "translate": [100, 0, 0]}));
    run(&mut s, "component.capture_position", json!({}));
    run(&mut s, "solid.box", json!({"length": 4, "width": 4, "height": 15, "corner": [103, 3, 0], "operation": "join", "targets": ["A"]}));
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [123, 3], "p1": [127, 7]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"distance": 15, "operation": "join", "targets": ["B"]}));
    let want = 10.0 * 10.0 * 10.0 + 4.0 * 4.0 * 5.0;
    for b in ["A", "B"] {
        assert_eq!(s.model.state().body(b).map(|b| b.body.shell_count()), Some(1), "{b}");
        assert!((volume(&s, b) - want).abs() < 1e-6 * want, "{b}: {} vs {want}", volume(&s, b));
    }

    // Through all reaches the target where it is shown: Part moved 100 mm along x and up, a cut
    // drawn on the root's XY plane goes 100 mm up through its cube, not 10 mm.
    let mut s = Session::default();
    created(&mut s, "Part");
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "body_name": "C"}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    run(&mut s, "occurrence.move", json!({"occurrence": "Part:1", "translate": [100, 0, 100]}));
    run(&mut s, "component.capture_position", json!({}));
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [103, 3], "p1": [107, 7]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"through_all": true, "operation": "cut", "targets": ["C"]}));
    let want = 10.0 * 10.0 * 10.0 - 4.0 * 4.0 * 10.0;
    assert!((volume(&s, "C") - want).abs() < 1e-6 * want, "C: {} vs {want}", volume(&s, "C"));
}

#[test]
fn component_delete_removes_joints_to_its_occurrences_and_undo_restores_them() {
    let mut s = Session::default();
    for (name, corner) in [("Arm", [20, -2, -2]), ("Kept", [-2, 20, -2])] {
        run(&mut s, "component.activate", json!({"component": 0}));
        created(&mut s, name);
        run(&mut s, "solid.box", json!({"length": 4, "width": 4, "height": 4, "corner": corner}));
    }
    run(
        &mut s,
        "joint.create",
        json!({"type": "revolute", "a": {"occurrence": 0, "point": [0,0,0]}, "b": {"occurrence": "Arm", "point": [0,0,0]}, "name": "Pivot"}),
    );
    run(
        &mut s,
        "joint.create",
        json!({"type": "rigid", "a": {"occurrence": 0, "point": [0,0,0]}, "b": {"occurrence": "Kept", "point": [0,0,0]}, "name": "Held"}),
    );
    let arm = s.doc.find_component("Arm").unwrap();
    let before = s.doc.clone();

    let reply = run(&mut s, "component.delete", json!({"component": "Arm"}));
    let features: Vec<u64> = before.features.iter().filter(|f| f.component == arm).map(|f| f.id).collect();
    assert_eq!(reply, json!({"deleted": [arm], "features": features}));
    let joints: Vec<&str> = s.doc.assembly.joints.iter().map(|j| j.name.as_str()).collect();
    assert_eq!(joints, ["Held"]);
    run(&mut s, "joint.list", json!({}));

    run(&mut s, "edit.undo", json!({}));
    assert_eq!(s.doc, before);
}

/// The stood-up nested STEP part of `a_stood_up_nested_step_part_is_cut_and_joined_from_its_face`,
/// opened with its imported root active, and a sketch on the part's top face. Returns the
/// session, the sketch, the middle of the top face in the root's frame, the face's outward
/// normal and that middle in the sketch's coordinates.
fn nested_part_face_sketch() -> (Session, u64, solidcraft_geom::Vec3, solidcraft_geom::Vec3, solidcraft_geom::Vec2) {
    let dir = std::env::temp_dir().join(format!("solidcraft-nested-scope-{}-{:?}", std::process::id(), std::thread::current().id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("nested.step");
    let mut src = Session::default();
    let (_, carrier) = created(&mut src, "Carrier");
    let (part, nested) = created(&mut src, "Nested Part");
    run(&mut src, "solid.box", json!({"length": 40, "width": 24, "height": 10, "body_name": "NestedBlock"}));
    run(&mut src, "occurrence.move", json!({"occurrence": carrier, "translate": [12, 7, 5], "axis": [0, 0, 1], "angle": "30 deg"}));
    run(&mut src, "occurrence.move", json!({"occurrence": nested, "translate": [3, 11, 2], "axis": [0, 1, 0], "angle": "20 deg"}));
    let top = solidcraft_doc::apply_point(&src.doc.component_transform(part), solidcraft_geom::Vec3::new(20.0, 12.0, 10.0));
    run(&mut src, "file.export", json!({"path": path, "format": "step"}));
    let mut s = Session::default();
    run(&mut s, "doc.open", json!({"path": path, "up": "y"}));
    let _ = std::fs::remove_dir_all(&dir);
    let at = solidcraft_doc::apply_point(&s.doc.component_transform(s.active_component), top);
    let sketch = run(&mut s, "sketch.create", json!({"plane": {"face": [at.x, at.y, at.z]}, "project_edges": false}))["sketch"].as_u64().unwrap();
    let Some(solidcraft_doc::FeatureKind::Sketch { plane: solidcraft_doc::PlaneRef::Face { plane, .. }, .. }) =
        s.doc.feature(sketch).map(|f| &f.kind)
    else {
        panic!("a sketch on the face")
    };
    let (normal, centre) = (plane.normal(), plane.to_local(top));
    (s, sketch, top, normal, centre)
}

/// After a feature from the nested part's face: still one body, in one piece, `change` mm³
/// bigger than the 40 × 24 × 10 block (within a thousandth: round profiles are faceted).
fn assert_block_changed_by(s: &mut Session, change: f64) {
    let st = s.model.state();
    assert_eq!(st.bodies.iter().map(|b| b.name.as_str()).collect::<Vec<_>>(), ["NestedBlock"]);
    assert_eq!(st.body("NestedBlock").map(|b| b.body.shell_count()), Some(1));
    let volume = run(s, "inspect.measure", json!({"bodies": ["NestedBlock"]}))["total"]["volume_mm3"].as_f64().unwrap();
    assert!((volume - 9600.0 - change).abs() < 1e-3 * change.abs(), "{volume}");
}

/// #3223 for Revolve: a ring revolved about a line on the nested part's face, with the imported
/// root active, joins the part (the half above the face) instead of starting a body of its own.
#[test]
fn a_revolve_from_a_nested_step_parts_face_joins_it() {
    let (mut s, _, _, _, c) = nested_part_face_sketch();
    let axis = run(&mut s, "sketch.line", json!({"points": [[c.x, c.y - 5.0], [c.x, c.y + 5.0]]}))["curves"][0].as_str().unwrap().to_string();
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [c.x + 1.0, c.y - 2.0], "p1": [c.x + 3.0, c.y + 2.0]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.revolve", json!({"axis": axis, "angle": "360 deg", "operation": "join"}));
    assert_block_changed_by(&mut s, std::f64::consts::PI * (9.0 - 1.0) * 4.0 / 2.0);
}

/// #3223 for Loft: a loft from a circle on the nested part's face to one 5 mm above joins it.
#[test]
fn a_loft_from_a_nested_step_parts_face_joins_it() {
    let (mut s, sketch, top, _, c) = nested_part_face_sketch();
    run(&mut s, "sketch.circle.center", json!({"center": [c.x, c.y], "radius": 3}));
    run(&mut s, "sketch.finish", json!({}));
    let at = solidcraft_doc::apply_point(&s.doc.component_transform(s.active_component), top);
    let high =
        run(&mut s, "sketch.create", json!({"plane": {"face": [at.x, at.y, at.z]}, "offset": 5, "project_edges": false}))["sketch"].as_u64().unwrap();
    run(&mut s, "sketch.circle.center", json!({"center": [c.x, c.y], "radius": 3}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.loft", json!({"sections": [{"sketch": sketch}, {"sketch": high}], "operation": "join"}));
    assert_block_changed_by(&mut s, std::f64::consts::PI * 9.0 * 5.0);
}

/// #3223 for Sweep: a circle on the nested part's face swept 5 mm out along its normal joins it.
#[test]
fn a_sweep_from_a_nested_step_parts_face_joins_it() {
    let (mut s, sketch, top, n, c) = nested_part_face_sketch();
    run(&mut s, "sketch.circle.center", json!({"center": [c.x, c.y], "radius": 3}));
    run(&mut s, "sketch.finish", json!({}));
    let root = s.doc.component_transform(s.active_component);
    let (a, b) = (solidcraft_doc::apply_point(&root, top), solidcraft_doc::apply_point(&root, top + n * 5.0));
    let path = run(&mut s, "sketch.create", json!({"plane": "XY"}))["sketch"].as_u64().unwrap();
    run(&mut s, "sketch.options", json!({"sketch_3d": true}));
    let wire = run(&mut s, "sketch.line3d", json!({"points": [[a.x, a.y, a.z], [b.x, b.y, b.z]]}))["curves"][0].as_str().unwrap().to_string();
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.sweep", json!({"sketch": sketch, "path_sketch": path, "path": [wire], "operation": "join"}));
    assert_block_changed_by(&mut s, std::f64::consts::PI * 9.0 * 5.0);
}

/// #3223 for Hole: a hole at a point sketched on the nested part's face, with the imported root
/// active, drills the part instead of finding no body to cut.
#[test]
fn a_hole_at_a_point_on_a_nested_step_parts_face_drills_it() {
    let (mut s, sketch, _, _, c) = nested_part_face_sketch();
    run(&mut s, "sketch.point", json!({"point": [c.x, c.y]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.hole", json!({"sketch": sketch, "diameter": 4, "depth": 5}));
    assert_block_changed_by(&mut s, -std::f64::consts::PI * 4.0 * 5.0);
}

/// Pasting a body into a component placed elsewhere puts the copy in that component, where the
/// body is in the world, as one step to undo.
#[test]
fn pasted_bodies_go_into_the_component_where_the_body_is() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 20, "width": 20, "height": 20, "body_name": "Cube"}));
    let (component, occurrence) = created(&mut s, "A");
    run(&mut s, "occurrence.move", json!({"occurrence": occurrence, "translate": [100,0,0]}));
    let undo = s.undo.len();
    let out = run(&mut s, "component.copy_bodies", json!({"bodies": ["Cube"]}));
    let copy = out["bodies"][0].as_str().unwrap().to_string();
    assert_eq!(s.undo.len(), undo + 1, "one step to undo");
    let state = s.model.state();
    assert_eq!(s.doc.model_body_component(state.body(&copy).unwrap()), component);
    let placed = run(&mut s, "occurrence.properties", json!({"occurrence": occurrence, "path": [occurrence]}));
    for i in 0..3 {
        near(&placed["total"]["center_of_mass"][i], 10.0);
    }
    assert_eq!(s.selection, vec![crate::Sel::Body { name: copy.clone() }]);
    run(&mut s, "edit.undo", json!({}));
    assert!(s.model.state().body(&copy).is_none());
}

/// A component pasted into another component, as an instance or as a new copy, sits where the
/// copied placement is in the world; a new copy can't go inside the component itself.
#[test]
fn components_paste_in_place_into_another_component() {
    let mut s = Session::default();
    let (part, part_occurrence) = created(&mut s, "Part");
    run(&mut s, "solid.box", json!({"length": 2, "width": 2, "height": 2}));
    run(&mut s, "occurrence.move", json!({"occurrence": part_occurrence, "translate": [50,0,0]}));
    run(&mut s, "component.activate", json!({"component": 0}));
    let (holder, holder_occurrence) = created(&mut s, "Holder");
    run(&mut s, "occurrence.move", json!({"occurrence": holder_occurrence, "translate": [0,100,0]}));
    let source = json!({"component": part, "occurrence": part_occurrence, "parent": holder, "in_place": true});
    let instance = run(&mut s, "occurrence.copy", source.clone())["occurrence"].as_u64().unwrap();
    let copy = run(&mut s, "component.paste_new", source)["occurrence"].as_u64().unwrap();
    for placed in [instance, copy] {
        let result = run(&mut s, "occurrence.properties", json!({"occurrence": placed, "path": [holder_occurrence, placed]}));
        for (i, expected) in [51.0, 1.0, 1.0].into_iter().enumerate() {
            near(&result["total"]["center_of_mass"][i], expected);
        }
    }
    let into_itself = json!({"component": holder, "occurrence": holder_occurrence, "parent": holder, "in_place": true});
    assert!(s.execute("component.paste_new", &into_itself).is_err());
}

/// Pasted into a component placed more than once, a component goes into every instance: placed
/// by the active instance where the copied one is, and in the same place in the other.
#[test]
fn a_component_pasted_into_an_instanced_component_goes_into_every_instance() {
    let mut s = Session::default();
    let (_, part_occurrence) = created(&mut s, "Part");
    run(&mut s, "solid.box", json!({"length": 2, "width": 2, "height": 2}));
    run(&mut s, "component.activate", json!({"component": 0}));
    let (plate, first) = created(&mut s, "Plate");
    let second = run(&mut s, "occurrence.copy", json!({"component": plate, "translate": [0,100,0]}))["occurrence"].as_u64().unwrap();
    run(&mut s, "component.activate", json!({"component": 0}));
    let source = json!({"occurrence": part_occurrence, "parent": plate, "in_place": true});
    assert!(s.execute("occurrence.copy", &source).is_err(), "no instance of Plate to place it by");
    run(&mut s, "component.activate", json!({"component": plate, "occurrence": second, "path": [second]}));
    let pasted = run(&mut s, "occurrence.copy", source)["occurrence"].as_u64().unwrap();
    for (instance, y) in [(second, 1.0), (first, -99.0)] {
        let result = run(&mut s, "occurrence.properties", json!({"occurrence": pasted, "path": [instance, pasted]}));
        near(&result["total"]["center_of_mass"][1], y);
    }
}

/// A body pasted into a component placed more than once goes into every instance: placed by the
/// active instance where the body is, and in the same place in the other.
#[test]
fn a_body_pasted_into_an_instanced_component_goes_into_every_instance() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 2, "width": 2, "height": 2, "body_name": "Cube"}));
    let (plate, first) = created(&mut s, "Plate");
    let second = run(&mut s, "occurrence.copy", json!({"component": plate, "translate": [0,100,0]}))["occurrence"].as_u64().unwrap();
    run(&mut s, "component.activate", json!({"component": plate, "occurrence": second, "path": [second]}));
    run(&mut s, "component.copy_bodies", json!({"bodies": ["Cube"]}));
    for (instance, y) in [(second, 1.0), (first, -99.0)] {
        let result = run(&mut s, "occurrence.properties", json!({"occurrence": instance, "path": [instance]}));
        near(&result["total"]["center_of_mass"][1], y);
    }
}

/// A nested placement moves out to the design and back in, staying where it is in the world, and
/// the session's paths to it (selection, hidden placements) follow it.
#[test]
fn reparent_keeps_the_world_position_and_follows_paths() {
    let mut s = Session::default();
    let (parent_component, parent) = created(&mut s, "Parent");
    run(&mut s, "occurrence.move", json!({"occurrence": parent, "translate": [100,0,0], "axis": [0,0,1], "angle": "90 deg"}));
    let (_, child) = created(&mut s, "Child");
    run(&mut s, "solid.box", json!({"length": 2, "width": 2, "height": 2}));
    let centre =
        |s: &mut Session, path: Value| run(s, "occurrence.properties", json!({"occurrence": child, "path": path}))["total"]["center_of_mass"].clone();
    let before = centre(&mut s, json!([parent, child]));
    run(&mut s, "select.set", json!({"items": [{"type": "occurrence", "path": [parent, child]}]}));
    s.visibility.hidden_occurrences.push(vec![parent, child]);

    let out = run(&mut s, "occurrence.reparent", json!({"occurrence": child, "parent": "root"}));
    assert_eq!(out["path"], json!([child]));
    assert_eq!(s.doc.occurrences.iter().find(|o| o.id == child).unwrap().parent, 0);
    let after = centre(&mut s, json!([child]));
    for i in 0..3 {
        near(&after[i], before[i].as_f64().unwrap());
    }
    assert_eq!(s.selection, vec![crate::Sel::Occurrence { path: vec![child] }]);
    assert_eq!(s.visibility.hidden_occurrences, vec![vec![child]]);

    run(&mut s, "occurrence.reparent", json!({"occurrence": child, "parent": parent_component}));
    let back = centre(&mut s, json!([parent, child]));
    for i in 0..3 {
        near(&back[i], before[i].as_f64().unwrap());
    }
    // Undo puts it where it was.
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(s.doc.occurrences.iter().find(|o| o.id == child).unwrap().parent, 0);
}

/// A component can't go inside itself or what it holds. Out of a parent placed more than once it
/// moves from the placement named, keeping that one's place in the world.
#[test]
fn reparent_refuses_cycles_and_moves_out_of_a_named_instance() {
    let mut s = Session::default();
    let (parent_component, parent) = created(&mut s, "Parent");
    let (child_component, child) = created(&mut s, "Child");
    run(&mut s, "solid.box", json!({"length": 2, "width": 2, "height": 2}));
    for into in [parent_component, child_component] {
        assert!(s.execute("occurrence.reparent", &json!({"occurrence": parent, "parent": into})).is_err(), "into {into}");
    }
    let second = run(&mut s, "occurrence.copy", json!({"component": parent_component, "translate": [50,0,0]}))["occurrence"].as_u64().unwrap();
    let err = s.execute("occurrence.reparent", &json!({"occurrence": child, "parent": 0})).unwrap_err().to_string();
    assert!(err.contains("path"), "which placement is ambiguous: {err}");
    run(&mut s, "occurrence.reparent", json!({"occurrence": child, "path": [second, child], "parent": 0}));
    let result = run(&mut s, "occurrence.properties", json!({"occurrence": child, "path": [child]}));
    near(&result["total"]["center_of_mass"][0], 51.0);
}

/// A component placed somewhere other than where it was made still can't take a placement of
/// what holds it: A placed in B means B can't then be placed in A, by copy or by a move.
#[test]
fn a_component_placed_in_another_cannot_then_hold_it() {
    let mut s = Session::default();
    let a = run(&mut s, "component.create", json!({"name": "A", "activate": false}))["component"].as_u64().unwrap();
    let b = run(&mut s, "component.create", json!({"name": "B", "activate": false}));
    let (b, b_occurrence) = (b["component"].as_u64().unwrap(), b["occurrence"].as_u64().unwrap());
    run(&mut s, "occurrence.copy", json!({"component": a, "parent": b}));
    assert!(s.execute("occurrence.copy", &json!({"component": b, "parent": a})).is_err(), "B inside A inside B");
    assert!(s.execute("occurrence.reparent", &json!({"occurrence": b_occurrence, "parent": a})).is_err(), "moved, too");
}

/// A component named after its body: each Paste New numbers the component and its body alike,
/// one after the other, with no number skipped (they are numbered among their own kind).
#[test]
fn paste_new_numbers_a_component_and_its_body_alike() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 2, "width": 2, "height": 2, "body_name": "Plate"}));
    let plate = run(&mut s, "component.from_bodies", json!({"bodies": ["Plate"]}))["components"][0]["component"].as_u64().unwrap();
    for _ in 0..2 {
        run(&mut s, "component.paste_new", json!({"component": plate}));
    }
    let components: Vec<&str> = s.doc.components.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(components, ["Plate", "Plate (2)", "Plate (3)"]);
    let state = s.model.state();
    let mut bodies: Vec<&str> = state.bodies.iter().map(|b| b.name.as_str()).collect();
    bodies.sort_unstable();
    assert_eq!(bodies, ["Plate", "Plate (2)", "Plate (3)"]);
}

/// A Paste New copy has features of its own: editing the copy leaves the original as it was.
#[test]
fn a_paste_new_copy_is_independent_of_its_original() {
    let mut s = Session::default();
    let (part, _) = created(&mut s, "Part");
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "S"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [10, 10]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"sketch": "S", "distance": 5, "body_name": "Block"}));
    run(&mut s, "component.activate", json!({"component": 0}));
    run(&mut s, "component.paste_new", json!({"component": part}));
    run(&mut s, "timeline.edit", json!({"feature": "Extrude1 (2)", "set": {"extent": {"distance": "30"}}}));
    let state = s.model.state();
    let volume = |name: &str| solidcraft_kernel::measure(&state.body(name).unwrap().body).unwrap().volume;
    assert!((volume("Block") - 500.0).abs() < 1e-6, "the original is as it was");
    assert!((volume("Block (2)") - 3000.0).abs() < 1e-6, "the copy changed");
}

/// What was pasted and moved keeps its place, name and component when the design is saved and
/// opened again.
#[test]
fn pasted_and_moved_things_survive_reopening() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 2, "width": 2, "height": 2, "body_name": "Cube"}));
    let (a, a_at) = created(&mut s, "A");
    run(&mut s, "occurrence.move", json!({"occurrence": a_at, "translate": [100,0,0]}));
    run(&mut s, "component.copy_bodies", json!({"bodies": ["Cube"]}));
    run(&mut s, "component.activate", json!({"component": 0}));
    let (_, b_at) = created(&mut s, "B");
    run(&mut s, "solid.box", json!({"length": 1, "width": 1, "height": 1, "corner": [50, 0, 0], "body_name": "Pin"}));
    run(&mut s, "component.activate", json!({"component": 0}));
    run(&mut s, "occurrence.reparent", json!({"occurrence": b_at, "parent": a}));
    let before = s.world_state();
    let mut reopened = Session { doc: solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap().into(), ..Session::default() };
    reopened.refresh();
    let after = reopened.world_state();
    for name in ["Cube", "Cube (2)", "Pin"] {
        let (was, is) = (before.body(name).unwrap().mesh().bounds(), after.body(name).unwrap().mesh().bounds());
        assert!(was.min.dist(is.min) < 1e-6 && was.max.dist(is.max) < 1e-6, "{name}: {was:?} → {is:?}");
    }
    let state = reopened.model.state();
    assert_eq!(reopened.doc.model_body_component(state.body("Cube (2)").unwrap()), a);
    assert_eq!(reopened.doc.occurrences.iter().find(|o| o.id == b_at).unwrap().parent, a);
}

/// A body seen in another placement of its component (`Cube (B:2)`) copies from there: the copy
/// is of that body, named after it, and sits where that placement shows it.
#[test]
fn a_body_seen_in_another_placement_copies_from_there() {
    let mut s = Session::default();
    let (b, _) = created(&mut s, "B");
    run(&mut s, "solid.box", json!({"length": 2, "width": 2, "height": 2, "body_name": "Cube"}));
    run(&mut s, "component.activate", json!({"component": 0}));
    let second = run(&mut s, "occurrence.copy", json!({"component": b, "translate": [0,100,0]}))["occurrence"].as_u64().unwrap();
    let shown = s.occurrence_placement(&[second]).unwrap().bodies[0].world.clone();
    assert_ne!(shown, "Cube");
    let out = run(&mut s, "component.copy_bodies", json!({"bodies": [shown.clone()], "component": 0}));
    assert_eq!(out["bodies"], json!(["Cube (2)"]));
    let world = s.world_state();
    let (seen, copy) = (world.body(&shown).unwrap().mesh().bounds(), world.body("Cube (2)").unwrap().mesh().bounds());
    assert!(seen.min.dist(copy.min) < 1e-6 && seen.max.dist(copy.max) < 1e-6, "{seen:?} vs {copy:?}");
    let state = s.model.state();
    assert_eq!(s.doc.model_body_component(state.body("Cube (2)").unwrap()), 0);
}

/// A nested assembly's body listing, and the indexed placement agreeing with the plain walk
/// (#7502: listing a subtree must cost the subtree, not the subtree times the design).
#[test]
fn occurrence_bodies_lists_a_nested_assembly() {
    let mut s = Session::default();
    let (_outer, outer) = created(&mut s, "Outer");
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "body_name": "Shell"}));
    let (_middle, middle) = created(&mut s, "Middle");
    run(&mut s, "solid.box", json!({"length": 2, "width": 2, "height": 2, "body_name": "Plate"}));
    let (_inner, inner) = created(&mut s, "Inner");
    run(&mut s, "solid.box", json!({"length": 1, "width": 1, "height": 1, "body_name": "Pin"}));
    let deep = [outer, middle, inner];
    let sorted = |v: Vec<String>| {
        let mut v = v;
        v.sort();
        v
    };
    assert_eq!(sorted(s.occurrence_bodies(&deep, true).unwrap()), ["Pin"]);
    assert_eq!(sorted(s.occurrence_bodies(&[outer, middle], true).unwrap()), ["Pin", "Plate"]);
    assert_eq!(sorted(s.occurrence_bodies(&[outer], true).unwrap()), ["Pin", "Plate", "Shell"]);
    assert_eq!(sorted(s.occurrence_bodies(&[outer, middle], false).unwrap()), ["Plate"]);
    // The indexed placement answers exactly what the plain walk does.
    for path in [&deep[..], &[outer, middle][..], &[outer][..]] {
        let (indexed, walked) = (s.occurrence_placement(path).unwrap(), s.occurrence_placement_walk(path).unwrap());
        assert_eq!(indexed.component, walked.component);
        assert_eq!(indexed.name, walked.name);
        assert_eq!(indexed.world, walked.world);
        assert_eq!(indexed.bodies.len(), walked.bodies.len());
        for (a, b) in indexed.bodies.iter().zip(&walked.bodies) {
            assert_eq!((a.source.as_str(), a.world.as_str()), (b.source.as_str(), b.world.as_str()));
        }
    }
}

/// A burst of other paths retires single remembered answers instead of clearing the whole
/// cache, so the active occurrence's listing survives and needn't re-expand (#7502).
#[test]
fn occurrence_bodies_cache_retires_one_answer_when_full() {
    let mut s = Session::default();
    let (_assembly, assembly) = created(&mut s, "Assembly");
    let (part, _) = created(&mut s, "Part");
    run(&mut s, "solid.box", json!({"length": 1, "width": 1, "height": 1, "body_name": "Part"}));
    let mut copies = Vec::new();
    for i in 0..130 {
        let o = run(&mut s, "occurrence.copy", json!({"component": part, "translate": [i as f64 * 10.0, 0.0, 0.0]}))["occurrence"].as_u64().unwrap();
        copies.push(o);
    }
    // The active listing (the whole assembly) is asked first and remembered.
    let root = vec![assembly];
    assert_eq!(s.occurrence_bodies(&root, true).unwrap().len(), 131);
    // Then a burst of per-copy queries: 260 more keys, over the cache's 256-entry limit.
    for o in &copies {
        let path = vec![assembly, *o];
        assert_eq!(s.occurrence_bodies(&path, false).unwrap().len(), 1);
        assert_eq!(s.occurrence_bodies(&path, true).unwrap().len(), 1);
    }
    let guard = s.occurrence_bodies_cache.lock().unwrap();
    let cache = guard.as_ref().unwrap();
    assert!(cache.bodies.contains_key(&(root.clone(), true)), "the active listing survives a burst of other paths");
    assert!(cache.bodies.len() <= 256, "the cache stays bounded at its limit");
    drop(guard);
    assert_eq!(s.occurrence_bodies(&root, true).unwrap().len(), 131);
}

/// A connector's boss extruded To Object up to the shell of another component joins the
/// connector only: the shell it runs to stays its own body in its own component, and the PCB
/// sketched on the shell afterwards is still made (a Join extrude to an object joined the body
/// it ran to across components, 0.24.1).
#[test]
fn join_extrude_to_a_body_of_another_component_leaves_that_body_alone() {
    let mut s = Session::default();
    let (shell, _) = created(&mut s, "Shell");
    run(&mut s, "solid.box", json!({"corner":[-20,-20,20],"length":40,"width":40,"height":5,"body_name":"Shell"}));
    let (connector, _) = created(&mut s, "Connector");
    run(&mut s, "solid.cylinder", json!({"radius":6,"height":10,"body_name":"Connector"}));
    let sketch = run(&mut s, "sketch.create", json!({"plane":{"face":[0,0,10]},"project_edges":false}))["sketch"].as_u64().unwrap();
    let plane = s.model.state().sketch(sketch).unwrap().plane;
    let centre = plane.to_local(solidcraft_geom::Vec3::new(0.0, 0.0, 10.0));
    run(&mut s, "sketch.circle.center", json!({"center":[centre.x, centre.y],"radius":4}));
    run(&mut s, "sketch.finish", json!({}));
    // The boss runs up to the shell's underside and joins the connector it was sketched on.
    run(&mut s, "solid.extrude", json!({"sketch":sketch,"to":[0,0,20],"operation":"join","targets":["Connector"]}));
    let (pcb, _) = created(&mut s, "PCB");
    run(&mut s, "solid.box", json!({"corner":[-15,-15,25],"length":30,"width":30,"height":1.6,"body_name":"PCB"}));
    assert!(s.model.results.iter().all(|r| r.error.is_none()), "{:?}", s.model.results);
    let st = s.model.state();
    let mut names: Vec<&str> = st.bodies.iter().map(|b| b.name.as_str()).collect();
    names.sort_unstable();
    assert_eq!(names, ["Connector", "PCB", "Shell"], "three bodies, none merged into another");
    for (name, component) in [("Shell", shell), ("Connector", connector), ("PCB", pcb)] {
        assert_eq!(s.doc.model_body_component(st.body(name).unwrap()), component, "{name} stays in its component");
    }
    let volume = |name: &str| solidcraft_kernel::measure(&st.body(name).unwrap().body).unwrap().volume;
    let pi = std::f64::consts::PI;
    assert!((volume("Shell") - 8000.0).abs() < 1e-3, "the shell is unchanged: {}", volume("Shell"));
    assert!((volume("Connector") - (pi * 36.0 * 10.0 + pi * 16.0 * 10.0)).abs() < 1.0, "the connector took the boss: {}", volume("Connector"));
}

/// A pattern of bodies made in a component copies them about its own component's axis, so
/// moving that component's occurrence moves the copies of a body that lives elsewhere. The model
/// kept between edits must show them where a fresh evaluation does: a root block patterned four
/// times about the Z axis of component Part, then Part moved.
#[test]
fn a_pattern_of_another_components_body_follows_its_own_component_when_moved() {
    let boxes = |st: &solidcraft_doc::ModelState| -> Vec<String> {
        st.bodies
            .iter()
            .map(|b| {
                let bb = b.body.outline_bounds();
                format!("{} [{:.6} {:.6} {:.6}]..[{:.6} {:.6} {:.6}]", b.name, bb.min.x, bb.min.y, bb.min.z, bb.max.x, bb.max.y, bb.max.z)
            })
            .collect()
    };
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"corner": [-4, -27, 0], "length": 20, "width": 13, "height": 4}));
    created(&mut s, "Part");
    run(&mut s, "solid.pattern.circular", json!({"axis": "Z", "bodies": ["Body1"], "count": 4}));
    run(&mut s, "occurrence.move", json!({"occurrence": "Part:1", "translate": [6, -14, -3]}));
    let mut fresh = solidcraft_doc::Model::new();
    fresh.no_cache = true;
    fresh.evaluate(&s.doc);
    assert_eq!(boxes(&s.model.state()), boxes(&fresh.state()), "the kept model against a fresh evaluation");
}

#[test]
fn occurrence_align_moves_only_selected_instance_including_90_degree_face_mate() {
    let mut s = Session::default();
    let c = run(&mut s, "component.create", json!({"name": "Extrusion"}));
    let (comp, first) = (c["component"].as_u64().unwrap(), c["occurrence"].as_u64().unwrap());
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 60, "body_name": "Beam"}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    let second = run(&mut s, "occurrence.copy", json!({"component": comp, "translate": [40, 0, 0]}))["occurrence"].as_u64().unwrap();

    // Activate the second instance (the user's exact workflow) and align its bottom face (-Z at
    // [45, 5, 0]) onto the first instance's +X side face ([10, 5, 30]): instance 2 turns 90 deg
    // to extend along +X, while instance 1 remains untouched.
    run(&mut s, "component.activate", json!({"component": comp, "occurrence": second, "path": [second]}));
    let features_before = s.doc.features.len();
    run(
        &mut s,
        "occurrence.align",
        json!({
            "occurrence": second,
            "path": [second],
            "from_face": [45, 5, 0],
            "to_face": [10, 5, 30]
        }),
    );
    assert_eq!(s.doc.features.len(), features_before, "aligning an occurrence must not add a body feature to the shared component");

    let st = s.world_state();
    let b1 = st.body("Beam").unwrap().mesh().bounds();
    assert!((b1.min.x - 0.0).abs() < 1e-5 && (b1.max.x - 10.0).abs() < 1e-5, "{b1:?}");
    assert!((b1.min.y - 0.0).abs() < 1e-5 && (b1.max.y - 10.0).abs() < 1e-5, "{b1:?}");
    assert!((b1.min.z - 0.0).abs() < 1e-5 && (b1.max.z - 60.0).abs() < 1e-5, "{b1:?}");

    let b2 = st.body("Beam (Extrusion:2)").unwrap().mesh().bounds();
    assert!((b2.min.x - 10.0).abs() < 1e-5 && (b2.max.x - 70.0).abs() < 1e-5, "{b2:?}");
    assert!((b2.min.y - 0.0).abs() < 1e-5 && (b2.max.y - 10.0).abs() < 1e-5, "{b2:?}");
    assert!((b2.min.z - 25.0).abs() < 1e-5 && (b2.max.z - 35.0).abs() < 1e-5, "{b2:?}");

    // Grounded first part refuses without move_grounded, leaving both placements untouched.
    let doc_before = s.doc.clone();
    let err = s.execute("occurrence.align", &json!({"occurrence": first, "from": [0, 0, 0], "to": [5, 0, 0]})).unwrap_err().to_string();
    assert!(err.contains("grounded"), "{err}");
    assert_eq!(s.doc, doc_before);
}

#[test]
fn occurrence_align_supports_snaps_planes_flip_nested_paths_and_pending_capture() {
    let mut s = Session::default();
    let (_, parent) = created(&mut s, "Frame");
    run(&mut s, "occurrence.move", json!({"occurrence": parent, "axis": [0, 0, 1], "angle": "90 deg"}));
    let (_, child) = created(&mut s, "Bracket");
    run(&mut s, "solid.box", json!({"length": 10, "width": 20, "height": 4, "corner": [5, 5, 10], "body_name": "Plate"}));
    run(&mut s, "component.activate", json!({"component": "root"}));

    // Align the child under a 90-deg rotated parent onto XY plane along plane normal only (capture: false).
    let before = s.world_state().body("Plate").unwrap().mesh().bounds();
    run(
        &mut s,
        "occurrence.align",
        json!({
            "paths": [[parent, child]],
            "from_face": [before.center().x, before.center().y, 10.0],
            "to_plane": "XY",
            "capture": false
        }),
    );
    let moved = s.world_state().body("Plate").unwrap().mesh().bounds();
    assert!((moved.min.z - 0.0).abs() < 1e-5 && (moved.max.z - 4.0).abs() < 1e-5, "{moved:?}");
    assert!((moved.center().x - before.center().x).abs() < 1e-5 && (moved.center().y - before.center().y).abs() < 1e-5, "{moved:?}");

    // Revert drops the pending alignment; re-align with capture: true and vertex snap to origin.
    run(&mut s, "component.revert_position", json!({}));
    assert!((s.world_state().body("Plate").unwrap().mesh().bounds().min.z - 10.0).abs() < 1e-5);

    run(
        &mut s,
        "occurrence.align",
        json!({
            "occurrence": child,
            "path": [parent, child],
            "from": {"snap": "vertex", "at": [before.min.x, before.min.y, before.min.z]},
            "to": "origin"
        }),
    );
    let at_origin = s.world_state().body("Plate").unwrap().mesh().bounds();
    assert!(at_origin.min.dist(solidcraft_geom::Vec3::ZERO) < 1e-5, "{at_origin:?}");

    // Hostile / invalid parameters never panic.
    for bad_params in [
        json!({}),
        json!({"occurrence": 999999, "from": [0, 0, 0], "to": [1, 0, 0]}),
        json!({"paths": [], "from": [0, 0, 0], "to": [1, 0, 0]}),
        json!({"occurrence": child, "path": [parent, child], "from": [1e12, 0, 0], "to": [0, 0, 0]}),
        json!({"occurrence": child, "path": [parent, child], "from": {"snap": "bogus", "at": [0, 0, 0]}, "to": [0, 0, 0]}),
        json!({"occurrence": child, "path": [parent, child], "from_plane": "XY", "from": [0, 0, 0], "to": [0, 0, 1]}),
    ] {
        assert!(s.execute("occurrence.align", &bad_params).is_err(), "{bad_params}");
    }
}

#[test]
fn occurrence_align_keeps_every_pick_and_reports_shared_definitions() {
    // Frame holds Bracket and is placed twice (A and B). Aligning [A, Bracket] together with B
    // must move both picks: B is another placement of Frame, not an ancestor of [A, Bracket].
    let mut s = Session::default();
    let (frame, a) = created(&mut s, "Frame");
    let (_, bracket) = created(&mut s, "Bracket");
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 4, "body_name": "Plate"}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    let b = run(&mut s, "occurrence.copy", json!({"component": frame, "translate": [0, 50, 0]}))["occurrence"].as_u64().unwrap();
    let origin = |s: &Session, path: &[u64]| solidcraft_doc::apply_point(&s.occurrence_placement(path).unwrap().world, solidcraft_geom::Vec3::ZERO);
    let (a0, ab0, b0, bb0) = (origin(&s, &[a]), origin(&s, &[a, bracket]), origin(&s, &[b]), origin(&s, &[b, bracket]));

    assert!(!s.occurrence_shared(&[a]) && !s.occurrence_shared(&[b]), "top-level placements are their own");
    assert!(s.occurrence_shared(&[a, bracket]) && s.occurrence_shared(&[b, bracket]), "Bracket's placement lives in Frame, placed twice");

    let r = run(&mut s, "occurrence.align", json!({"paths": [[a, bracket], [b]], "from": [0, 0, 0], "to": [5, 0, 0]}));
    assert_eq!(r["paths"], json!([[a, bracket], [b]]), "{r}");
    assert_eq!(r["shared"], json!([[a, bracket]]), "{r}");

    let shift = solidcraft_geom::Vec3::new(5.0, 0.0, 0.0);
    assert!(origin(&s, &[a]).dist(a0) < 1e-9, "A itself stays");
    assert!(origin(&s, &[a, bracket]).dist(ab0 + shift) < 1e-9, "the picked Bracket moves");
    assert!(origin(&s, &[b]).dist(b0 + shift) < 1e-9, "B moves");
    // B's Bracket moves with B and, sharing Frame's definition, with the other pick too.
    assert!(origin(&s, &[b, bracket]).dist(bb0 + shift + shift) < 1e-9);

    // A top-level-only alignment reports nothing shared.
    let r = run(&mut s, "occurrence.align", json!({"paths": [[b]], "from": [0, 0, 0], "to": [1, 0, 0]}));
    assert!(r.get("shared").is_none(), "{r}");
}

/// A body copied in another design pastes here as an imported body: at the world coordinates it
/// had there, into the component asked for (placed elsewhere), named as it was or numbered when
/// that name is taken, from one Import feature named as any import is, as one step to undo.
#[test]
fn a_body_copied_in_another_design_pastes_as_an_imported_body() {
    let mut source = Session::default();
    source.doc_mut().name = "Bracket".into();
    let (_, holder) = created(&mut source, "Holder");
    run(&mut source, "occurrence.move", json!({"occurrence": holder, "translate": [0,50,0]}));
    run(&mut source, "solid.box", json!({"length": 2, "width": 2, "height": 2, "body_name": "Cube"}));
    let was = source.world_state().body("Cube").unwrap().mesh().bounds();
    let copy = crate::ForeignCopy::of(&source, vec!["Cube".into()], Vec::new());

    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 1, "width": 1, "height": 1, "body_name": "Cube"}));
    let (a, a_at) = created(&mut s, "A");
    run(&mut s, "occurrence.move", json!({"occurrence": a_at, "translate": [100,0,0]}));
    s.foreign_copy = Some(std::sync::Arc::new(copy));
    let undo = s.undo.len();
    let out = run(&mut s, "component.paste_from_design", json!({"component": a, "component_path": [a_at]}));
    assert_eq!(out["bodies"], json!(["Cube (2)"]));
    assert_eq!(out["feature"], "Import1");
    assert_eq!(s.undo.len(), undo + 1);
    let is = s.world_state().body("Cube (2)").unwrap().mesh().bounds();
    assert!(was.min.dist(is.min) < 1e-6 && was.max.dist(is.max) < 1e-6, "{was:?} vs {is:?}");
    let state = s.model.state();
    assert_eq!(s.doc.model_body_component(state.body("Cube (2)").unwrap()), a);
    assert!(matches!(&s.doc.find_feature("Import1").unwrap().kind, solidcraft_doc::FeatureKind::Import { file, .. } if file == "Bracket (pasted)"));
    run(&mut s, "edit.undo", json!({}));
    assert!(s.model.state().body("Cube (2)").is_none());
}

/// A body seen in another placement of its component ("Cube (Holder:2)") pastes into another
/// design under its own name, at that placement's position.
#[test]
fn a_body_seen_in_another_placement_pastes_into_another_design_under_its_own_name() {
    let mut source = Session::default();
    source.doc_mut().name = "Bracket".into();
    let (holder, first) = created(&mut source, "Holder");
    run(&mut source, "solid.box", json!({"length": 2, "width": 2, "height": 2, "body_name": "Cube"}));
    run(&mut source, "component.activate", json!({"component": 0}));
    let second = run(&mut source, "occurrence.copy", json!({"occurrence": first}))["occurrence"].as_u64().unwrap();
    run(&mut source, "occurrence.move", json!({"occurrence": second, "translate": [0,40,0]}));
    let seen = source.placed_document().world_body_name(&source.model.state(), "Cube", holder, &[second]);
    assert_ne!(seen, "Cube");
    let was = source.world_state().body(&seen).unwrap().mesh().bounds();
    let copy = crate::ForeignCopy::of(&source, vec![seen], Vec::new());
    assert_eq!(copy.names, vec!["Cube".to_string()]);

    let mut s = Session { foreign_copy: Some(std::sync::Arc::new(copy)), ..Default::default() };
    let out = run(&mut s, "component.paste_from_design", json!({}));
    assert_eq!(out["bodies"], json!(["Cube"]));
    let is = s.world_state().body("Cube").unwrap().mesh().bounds();
    assert!(was.min.dist(is.min) < 1e-6 && was.max.dist(is.max) < 1e-6, "{was:?} vs {is:?}");
}
