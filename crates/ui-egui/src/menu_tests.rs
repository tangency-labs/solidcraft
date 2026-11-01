//! Context menu tests: what the menus offer for each target, and that their items act.

use crate::test_support::sample_app;
use egui::pos2;
use serde_json::json;
use solidcraft_engine::Session;
use solidcraft_engine::geom::Vec3;

use crate::context_menu::{self, Item, Target};
use crate::viewport::Hit;
use crate::{Services, SolveApp};

#[test]
fn properties_leave_with_the_document_but_survive_a_failed_open() {
    let mut app = sample_app();
    app.menu.props = Some(("Prior body".into(), json!({"volume_mm3":42})));
    app.start("config.table");
    assert!(app.dialog.is_some());
    assert!(app.run("doc.open", json!({"path":"__missing_properties_regression__.solidcraft"})).is_err());
    assert!(app.menu.props.is_some());
    assert!(app.dialog.is_some());
    app.run("file.new", json!({"name":"New design"})).unwrap();
    assert!(app.menu.props.is_none());
    assert!(app.dialog.is_none());
    app.menu.props = Some(("New design".into(), json!({})));
    crate::documents::open_tab(&mut app, Session::default()).unwrap();
    assert!(app.menu.props.is_none());
    app.menu.props = Some(("Blank design".into(), json!({})));
    crate::documents::adopt(&mut app, Session::default());
    assert!(app.menu.props.is_none());
}

fn find(items: &[Item], label: &str) -> Item {
    items
        .iter()
        .find(|i| i.label == label)
        .cloned()
        .unwrap_or_else(|| panic!("no `{label}` in {:?}", items.iter().map(|i| &i.label).collect::<Vec<_>>()))
}

/// Run the item labelled `label` of the target's menu.
fn act(app: &mut SolveApp, t: &Target, label: &str) {
    let item = find(&context_menu::items(app, t), label);
    context_menu::run_item(app, &item, pos2(0.0, 0.0));
}

#[test]
fn linked_component_opens_its_part_in_a_tab_and_reports_a_missing_source() {
    let mut h = crate::scenario::Harness::new();
    let part = h.dir.join("part.solidcraft");
    let assembly = h.dir.join("assembly.solidcraft");
    let mut source = Session::default();
    source.execute("solid.box", &json!({"length": 10, "width": 20, "height": 30})).unwrap();
    source.execute("file.save", &json!({"path": part})).unwrap();
    let inserted = h.app.run("component.insert_design", json!({"path": part})).unwrap();
    let component = inserted["component"].as_u64().unwrap();
    let occurrence = inserted["occurrence"].as_u64().unwrap();
    let other_part = h.dir.join("other-part.solidcraft");
    let mut other_source = Session::default();
    other_source.execute("solid.box", &json!({"length": 5, "width": 5, "height": 5})).unwrap();
    other_source.execute("file.save", &json!({"path": other_part})).unwrap();
    let other = h.app.run("component.insert_design", json!({"path": other_part})).unwrap();
    let other_component = other["component"].as_u64().unwrap();
    h.app.run("file.save", json!({"path": assembly})).unwrap();

    h.app.tree.picked_components = vec![component, other_component];
    let linked = Target::Component { id: component };
    let row = context_menu::items(&h.app, &linked);
    assert!(row.iter().any(|item| item.label == "Open Linked Design"));
    let placed = context_menu::items(&h.app, &Target::Occurrence { path: vec![occurrence] });
    assert!(placed.iter().any(|item| item.label == "Open Linked Design"));
    assert!(!context_menu::items(&h.app, &Target::Component { id: 0 }).iter().any(|item| item.label == "Open Linked Design"));

    act(&mut h.app, &linked, "Open Linked Design");
    // The part opens in the background. A count of frames is no bound for that: frames are quick
    // while a loaded machine is slow to read the file. Wait as `Harness::settle` waits for a load.
    let began = std::time::Instant::now();
    while h.app.session.path.as_deref() != part.to_str() && began.elapsed() < std::time::Duration::from_secs(120) {
        h.frame();
    }
    assert_eq!(h.app.session.path.as_deref(), part.to_str());
    assert_eq!(h.app.docs.count(), 2);
    crate::documents::switch(&mut h.app, 0);
    assert_eq!(h.app.session.path.as_deref(), assembly.to_str());

    std::fs::remove_file(&part).unwrap();
    act(&mut h.app, &linked, "Open Linked Design");
    assert_eq!(h.app.docs.count(), 2);
    assert_eq!(h.app.session.path.as_deref(), assembly.to_str());
    assert!(h.app.status.as_ref().is_some_and(|(message, _, error)| *error && message.contains("Linked design not found")));
}

fn body(app: &SolveApp) -> String {
    app.session.model.state().bodies[0].name.clone()
}

#[test]
fn empty_space_offers_view_items_and_greys_out_undo() {
    let mut app = sample_app();
    app.session.undo.clear();
    let items = context_menu::items(&app, &Target::Viewport);
    assert!(!find(&items, "Undo").enabled, "nothing to undo");
    assert!(find(&items, "Fit").enabled);
    assert_eq!(context_menu::radial_items(&app).len(), 8);
    assert!(!context_menu::radial_items(&app)[0].enabled, "no command to repeat yet");
    // Hide Origin toggles.
    context_menu::run_item(&mut app, &find(&items, "Hide Origin"), pos2(0.0, 0.0));
    assert!(!app.origin_shown());
}

#[test]
fn appearance_browser_action_targets_the_clicked_body() {
    let mut app = sample_app();
    let b = body(&app);
    app.run("select.clear", json!({})).unwrap();
    act(&mut app, &Target::Body { name: b.clone() }, "Appearance");
    let dialog = app.dialog.as_ref().unwrap();
    assert!(matches!(dialog.kind, crate::dialogs::Kind::Appearance(_)));
    assert_eq!(dialog.inputs[0].items, vec![solidcraft_engine::Sel::Body { name: b }]);
}

#[test]
fn appearance_component_action_assigns_an_inherited_look() {
    let mut app = sample_app();
    let created = app.run("component.create", json!({"name": "Painted part"})).unwrap();
    let id = created["component"].as_u64().unwrap();
    act(&mut app, &Target::Component { id }, "Appearance");
    let d = app.dialog.clone().unwrap();
    let commands = crate::dialogs::apply_commands(&app, &d).unwrap();
    assert_eq!(commands[0].1["components"], json!(["Painted part"]));
    for (id, p) in commands {
        app.run(&id, p).unwrap();
    }
    assert!(app.session.doc.appearances.components.contains_key(&id));
}

/// A right-click on a sketch curve in the viewport reaches the sketch's own menu, the one the
/// browser row opens, Edit Sketch among it.
#[test]
fn a_sketch_curve_in_the_viewport_offers_the_sketch_menu() {
    let mut app = SolveApp::new(Session::default(), Services::default());
    app.run("sketch.create", json!({"plane": "XY", "name": "Base"})).unwrap();
    app.run("sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [40, 30]})).unwrap();
    app.run("sketch.finish", json!({})).unwrap();
    let (sketch, curve) = {
        let st = app.session.world_state();
        let ss = st.sketches.first().expect("the design has one sketch");
        (ss.feature, ss.sketch.curves.first().expect("the rectangle drew curves").id.clone())
    };
    app.run("select.set", json!({"items": [{"type": "external_sketch_curve", "sketch": sketch, "id": curve}]})).unwrap();
    let items = context_menu::items(&app, &Target::Viewport);
    // Item for item the browser row's menu, with the viewport's Clear Selection after it.
    let row = context_menu::items(&app, &Target::Sketch { id: sketch });
    let labels = |v: &[Item]| v.iter().map(|i| i.label.clone()).collect::<Vec<_>>();
    assert_eq!(labels(&items), [labels(&row), vec![String::new(), "Clear Selection".to_string()]].concat());
    let edit = find(&items, "Edit Sketch");
    assert_eq!(edit.id, "ui.editSketch");
    assert_eq!(edit.params, Some(json!({"sketch": sketch})));
    assert!(edit.enabled);
    context_menu::run_item(&mut app, &edit, pos2(0.0, 0.0));
    assert_eq!(app.session.active_sketch, Some(sketch), "Edit Sketch opens the sketch that was right-clicked");
    // The sketch is open now: the viewport offers the sketch-editing menu, not a second Edit Sketch.
    let editing = context_menu::items(&app, &Target::Viewport);
    assert!(find(&editing, "Finish Sketch").enabled);
    assert!(!editing.iter().any(|i| i.label == "Edit Sketch"), "{:?}", labels(&editing));
}

/// Solid geometry in the selection keeps the body menu: a right-click on a face offers the
/// face's commands even when a sketch curve is selected as well.
#[test]
fn a_selected_face_keeps_the_body_menu_beside_a_sketch_curve() {
    let mut app = sample_app();
    let b = body(&app);
    let sketch = app.session.world_state().sketches.first().expect("the sample design has sketches").feature;
    let items = json!([
        {"type": "face", "body": b, "index": 0, "point": [0, 0, 0]},
        {"type": "external_sketch_curve", "sketch": sketch, "id": "l1"},
    ]);
    app.run("select.set", json!({"items": items})).unwrap();
    let items = context_menu::items(&app, &Target::Viewport);
    assert!(find(&items, "Press Pull").enabled);
    assert!(find(&items, "Edit Profile Sketch").enabled);
    assert!(!items.iter().any(|i| i.label == "Redefine Sketch Plane"), "the face menu, not the sketch menu");
}

/// A right-click on a face or edge that is part of a multi-selection keeps the whole selection:
/// the pick keeps a point of its own, so the face is compared by body and index (#3733).
#[test]
fn right_click_on_a_selected_face_keeps_the_multi_selection() {
    let mut app = sample_app();
    let b = body(&app);
    app.run(
        "select.set",
        json!({"items": [
            {"type": "face", "body": b, "index": 0, "point": [1, 2, 3]},
            {"type": "face", "body": b, "index": 1, "point": [4, 5, 6]},
        ]}),
    )
    .unwrap();
    app.viewport.hover = Some(Hit::Face { body: b.clone(), index: 0, point: Vec3::new(9.0, 9.0, 9.0) });
    context_menu::open(&mut app, pos2(400.0, 300.0), Default::default());
    assert_eq!(app.session.selection.len(), 2, "the right-click keeps the faces it is about: {:?}", app.session.selection);
    // A face outside the selection is still picked, so its menu is about the face under the cursor.
    app.viewport.hover = Some(Hit::Face { body: b, index: 2, point: Vec3::new(9.0, 9.0, 9.0) });
    context_menu::open(&mut app, pos2(400.0, 300.0), Default::default());
    assert_eq!(app.session.selection.len(), 1, "a face outside the selection is picked: {:?}", app.session.selection);
}

/// A right-click on a face of a selected body keeps the whole selection: the body is what is
/// selected, so the menu is about the bodies and the face under the cursor does not replace them
/// (#5849).
#[test]
fn right_click_on_a_face_of_a_selected_body_keeps_the_bodies() {
    let mut app = sample_app();
    let first = body(&app);
    app.run("solid.box", json!({"length": 5, "width": 5, "height": 5, "corner": [200, 0, 0]})).unwrap();
    let second = app.session.model.state().bodies.iter().map(|b| b.name.clone()).find(|n| *n != first).expect("a second body");
    let both = vec![solidcraft_engine::Sel::Body { name: first.clone() }, solidcraft_engine::Sel::Body { name: second.clone() }];
    app.run("select.set", json!({"items": [{"type": "body", "name": first}, {"type": "body", "name": second}]})).unwrap();
    // A face of the first body: both bodies stay selected, and the menu stays the bodies' menu.
    app.viewport.hover = Some(Hit::Face { body: first.clone(), index: 0, point: Vec3::new(9.0, 9.0, 9.0) });
    context_menu::open(&mut app, pos2(400.0, 300.0), Default::default());
    assert_eq!(app.session.selection, both, "the right-click keeps the bodies it is about");
    assert!(!context_menu::items(&app, &Target::Viewport).iter().any(|i| i.label == "Create Sketch"), "the bodies' menu, not the face menu");
    // An edge and a vertex of a selected body count the same way.
    app.viewport.hover = Some(Hit::Edge { body: first.clone(), index: 0, mid: Vec3::new(9.0, 9.0, 9.0), point: Vec3::new(9.0, 9.0, 9.0) });
    context_menu::open(&mut app, pos2(400.0, 300.0), Default::default());
    assert_eq!(app.session.selection, both, "an edge of a selected body keeps the bodies");
    // A face of a body outside the selection is still picked, so the menu is about that face.
    app.run("select.set", json!({"items": [{"type": "body", "name": first}]})).unwrap();
    app.viewport.hover = Some(Hit::Face { body: second, index: 0, point: Vec3::new(9.0, 9.0, 9.0) });
    context_menu::open(&mut app, pos2(400.0, 300.0), Default::default());
    assert!(
        matches!(app.session.selection.as_slice(), [solidcraft_engine::Sel::Face { .. }]),
        "a face of an unselected body is picked: {:?}",
        app.session.selection
    );
}

#[test]
fn face_menu_hides_and_finds_the_body() {
    let mut app = sample_app();
    let b = body(&app);
    app.run("select.set", json!({"items": [{"type": "face", "body": b, "index": 0, "point": [0, 0, 0]}]})).unwrap();
    let items = context_menu::items(&app, &Target::Viewport);
    for l in ["Create Sketch", "Offset Plane", "Press Pull", "Extrude", "Measure", "Find in Browser", "Find in Timeline"] {
        assert!(find(&items, l).enabled, "{l}");
    }
    assert!(find(&items, "Appearance").enabled);
    context_menu::run_item(&mut app, &find(&items, "Find in Timeline"), pos2(0.0, 0.0));
    assert!(matches!(app.session.selection.as_slice(), [solidcraft_engine::Sel::Feature { .. }]));
    context_menu::run_item(&mut app, &find(&items, "Show/Hide"), pos2(0.0, 0.0));
    assert_eq!(app.session.visibility.hidden_bodies, vec![b]);
}

#[test]
fn body_menu_isolates_renames_locks_and_deletes() {
    let mut app = sample_app();
    app.run("solid.box", json!({"length": 5, "width": 5, "height": 5, "corner": [200, 0, 0]})).unwrap();
    let b = body(&app);
    let t = Target::Body { name: b.clone() };
    let items = context_menu::items(&app, &t);
    context_menu::run_item(&mut app, &find(&items, "Isolate"), pos2(0.0, 0.0));
    assert_eq!(app.session.visibility.hidden_bodies.len(), 1, "the other body is hidden");
    assert!(!app.session.visibility.hidden_bodies.contains(&b));
    // Rename through the rename box.
    context_menu::run_item(&mut app, &find(&items, "Rename"), pos2(10.0, 10.0));
    assert!(context_menu::finish_rename(&mut app, Some("Base"), true));
    assert!(app.session.model.state().body("Base").is_some());
    // Locked bodies can't be moved or deleted.
    let t = Target::Body { name: "Base".into() };
    act(&mut app, &t, "Lock");
    let items = context_menu::items(&app, &t);
    assert!(!find(&items, "Delete").enabled && !find(&items, "Move/Copy").enabled);
    context_menu::run_item(&mut app, &find(&items, "Unlock"), pos2(0.0, 0.0));
    let n = app.session.model.state().bodies.len();
    act(&mut app, &t, "Delete");
    assert_eq!(app.session.model.state().bodies.len(), n - 1);
}

#[test]
fn properties_measure_the_body() {
    let mut app = sample_app();
    let b = body(&app);
    let items = context_menu::items(&app, &Target::Body { name: b });
    context_menu::run_item(&mut app, &find(&items, "Properties"), pos2(0.0, 0.0));
    let (_, v) = app.menu.props.clone().unwrap();
    assert!(v["total"]["volume_mm3"].as_f64().unwrap() > 1000.0);
    assert!(v["total"]["mass_g"].as_f64().unwrap() > 0.0);
}

#[test]
fn component_properties_measure_one_assembly_placement() {
    let mut session = Session::default();
    let component = session.execute("component.create", &json!({"name": "Subassembly"})).unwrap()["component"].as_u64().unwrap();
    session.execute("component.create", &json!({"name": "Block"})).unwrap();
    session.execute("solid.box", &json!({"length": 10, "width": 10, "height": 10})).unwrap();
    session.execute("occurrence.copy", &json!({"component": component, "translate": [100, 0, 0]})).unwrap();
    let mut app = SolveApp::new(session, Services::default());
    act(&mut app, &Target::Component { id: component }, "Properties");
    let (title, properties) = app.menu.props.as_ref().unwrap();
    assert_eq!(title, "Subassembly");
    assert_eq!(properties["body_count"], 1);
    assert!((properties["total"]["volume_mm3"].as_f64().unwrap() - 1000.0).abs() < 1e-6);
    assert!((properties["total"]["center_of_mass"][0].as_f64().unwrap() - 5.0).abs() < 1e-6);
    act(&mut app, &Target::Component { id: 0 }, "Properties");
    assert_eq!(app.menu.props.as_ref().unwrap().1["body_count"], 2);
}

#[test]
fn sketch_menu_hides_and_moves_to_another_plane() {
    let mut app = sample_app();
    let id = app.session.doc.features.iter().find(|f| f.name == "Holes").unwrap().id;
    let t = Target::Sketch { id };
    let items = context_menu::items(&app, &t);
    let vis = crate::browser::sketch_visible(&app, id);
    context_menu::run_item(&mut app, &find(&items, "Show/Hide"), pos2(0.0, 0.0));
    assert_eq!(crate::browser::sketch_visible(&app, id), !vis);
    context_menu::run_item(&mut app, &find(&items, "Show Dimension"), pos2(0.0, 0.0));
    assert_eq!(app.session.visibility.shown_dimensions, vec![id]);
    assert!(find(&context_menu::items(&app, &t), "Hide Dimension").enabled);
    context_menu::run_item(&mut app, &find(&items, "Edit Sketch"), pos2(0.0, 0.0));
    assert_eq!(app.session.active_sketch, Some(id));
    app.finish_sketch();
    context_menu::run_item(&mut app, &find(&items, "Redefine Sketch Plane"), pos2(0.0, 0.0));
    assert_eq!(app.tree.redefine, Some(id));
}

#[test]
fn a_selected_arc_offers_an_arc_length_dimension() {
    // #6039: the arc length dimension existed as a command, but nothing in the UI led to it.
    let mut app = sample_app();
    app.run("sketch.create", json!({"plane": "XY"})).unwrap();
    app.run("sketch.arc.center_point", json!({"center": [0, 0], "start": [10, 0], "end": [0, 10]})).unwrap();
    app.run("sketch.circle.center", json!({"center": [40, 0], "radius": 5})).unwrap();
    let ids: Vec<String> = {
        let st = app.session.model.state();
        let ss = st.sketch(app.session.active_sketch.unwrap()).unwrap();
        ss.sketch.curves.iter().map(|c| c.id.clone()).collect()
    };
    let select = |app: &mut SolveApp, id: &str| app.run("select.set", json!({"items": [{"type": "sketch_curve", "id": id}]})).unwrap();
    select(&mut app, &ids[1]);
    assert!(!context_menu::items(&app, &Target::Viewport).iter().any(|i| i.label == "Arc Length Dimension"), "a circle has no arc length");
    select(&mut app, &ids[0]);
    let item = find(&context_menu::items(&app, &Target::Viewport), "Arc Length Dimension");
    assert!(item.enabled);
    context_menu::run_item(&mut app, &item, pos2(0.0, 0.0));
    let st = app.session.model.state();
    let ss = st.sketch(app.session.active_sketch.unwrap()).unwrap();
    let len = ss.sketch.constraints.iter().find_map(|c| match c.kind {
        solidcraft_engine::sketch::ConstraintKind::ArcLength { value, .. } => Some(value),
        _ => None,
    });
    // A quarter of a circle of radius 10.
    assert!(len.is_some_and(|l| (l - 5.0 * std::f64::consts::PI).abs() < 1e-5), "{:?}", ss.sketch.constraints);
    assert!(ss.report.ok());
}

#[test]
fn sketch_entities_offer_the_constraints_that_fit() {
    let mut app = sample_app();
    app.run("sketch.create", json!({"plane": "XY"})).unwrap();
    app.run("sketch.line", json!({"points": [[0, 0], [10, 1]]})).unwrap();
    app.run("sketch.line", json!({"points": [[0, 5], [10, 7]]})).unwrap();
    let ids: Vec<String> = {
        let st = app.session.model.state();
        let ss = st.sketch(app.session.active_sketch.unwrap()).unwrap();
        ss.sketch.curves.iter().map(|c| c.id.clone()).collect()
    };
    app.run("select.set", json!({"items": ids.iter().map(|i| json!({"type": "sketch_curve", "id": i})).collect::<Vec<_>>()})).unwrap();
    let items = context_menu::items(&app, &Target::Viewport);
    let par = find(&items, "Parallel");
    assert!(find(&items, "Finish Sketch").enabled);
    context_menu::run_item(&mut app, &par, pos2(0.0, 0.0));
    let st = app.session.model.state();
    let ss = st.sketch(app.session.active_sketch.unwrap()).unwrap();
    assert!(ss.sketch.constraints.iter().any(|c| format!("{:?}", c.kind).contains("Parallel")), "{:?}", ss.sketch.constraints);
}

#[test]
fn sketch_dimension_menu_deletes_only_the_dimension_and_supports_undo() {
    for (driven, preselected) in [(false, false), (false, true), (true, false), (true, true)] {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
        let line = h.app.run("sketch.line", json!({"points":[[0,0],[30,0]],"connect":false,"infer":false})).unwrap()["curves"][0]
            .as_str()
            .unwrap()
            .to_owned();
        let horizontal = h.app.run("sketch.constrainer", json!({"entities":[line]})).unwrap()["constraint"].as_str().unwrap().to_owned();
        let dimension = h.app.run("sketch.dimension", json!({"entities":[line],"value":30})).unwrap();
        let id = dimension["created"]["dimensions"][0].as_str().unwrap();
        if driven {
            h.app.run("sketch.toggle_driven", json!({"constraint":id})).unwrap();
        }
        let saved = h.app.session.doc.to_json();
        let undo = h.app.session.undo.len();
        h.call("ui.view", json!({"view":"top"}));
        h.call("ui.view", json!({"view":"fit"}));
        if preselected {
            h.step(&json!({"click":{"dimension":id}})).unwrap();
        }
        h.step(&json!({"click":{"dimension":id},"button":"right"})).unwrap();
        assert_eq!(h.app.session.selection, vec![solidcraft_engine::Sel::SketchConstraint { id: id.to_owned() }]);
        let menu = h.call("ui.menu", json!({}));
        assert!(menu["result"]["items"].as_array().unwrap().iter().any(|i| i["id"] == "ui.delete" && i["enabled"] == true), "{menu}");
        assert_eq!(h.call("ui.menuPick", json!({"item":"ui.delete"}))["ok"], true);
        let after = h.app.run("sketch.inspect", json!({})).unwrap();
        assert_eq!(after["curves"].as_array().unwrap().len(), 1);
        assert_eq!(after["curves"][0]["id"], line);
        assert_eq!(after["constraints"].as_array().unwrap().len(), 1);
        assert_eq!(after["constraints"][0]["id"], horizontal);
        assert_eq!(h.app.session.undo.len(), undo + 1);
        h.app.run("edit.undo", json!({})).unwrap();
        assert_eq!(h.app.session.doc.to_json(), saved);
        h.app.run("edit.redo", json!({})).unwrap();
        assert_eq!(h.app.run("sketch.inspect", json!({})).unwrap(), after);
    }
}

#[test]
fn sketch_constraint_menu_keeps_the_selection_and_respects_pick_filters() {
    let mut h = crate::scenario::Harness::new();
    h.app.home.open = false;
    h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    let line =
        h.app.run("sketch.line", json!({"points":[[0,0],[30,0]],"connect":false,"infer":false})).unwrap()["curves"][0].as_str().unwrap().to_owned();
    let horizontal = h.app.run("sketch.constrainer", json!({"entities":[line]})).unwrap()["constraint"].as_str().unwrap().to_owned();
    let dimension = h.app.run("sketch.dimension", json!({"entities":[line],"value":30})).unwrap();
    let id = dimension["created"]["dimensions"][0].as_str().unwrap();
    h.call("ui.view", json!({"view":"top"}));
    h.call("ui.view", json!({"view":"fit"}));

    // An empty selection, including a dimension whose picking is disabled, has no Delete.
    assert!(!context_menu::items(&h.app, &Target::Viewport).iter().any(|i| i.id == "ui.delete"));
    h.app.ui.pick_off.push("dimensions".into());
    h.step(&json!({"click":{"dimension":id},"button":"right"})).unwrap();
    assert!(h.app.session.selection.is_empty());
    assert!(!context_menu::items(&h.app, &Target::Viewport).iter().any(|i| i.id == "ui.delete"));
    h.call("ui.menu", json!({"close":true}));
    h.app.ui.pick_off.clear();

    // A non-dimensional constraint can be deleted too. Right-clicking a dimension already
    // in a multi-selection retains all the selected constraints for the same Delete action.
    for ids in [vec![horizontal.clone()], vec![horizontal.clone(), id.to_owned()]] {
        let selection: Vec<_> = ids.iter().map(|id| solidcraft_engine::Sel::SketchConstraint { id: id.clone() }).collect();
        h.app.session.selection = selection.clone();
        if ids.len() > 1 {
            h.step(&json!({"click":{"dimension":id},"button":"right"})).unwrap();
            assert_eq!(h.app.session.selection, selection);
        }
        let item = find(&context_menu::items(&h.app, &Target::Viewport), "Delete");
        assert!(item.enabled);
        context_menu::run_item(&mut h.app, &item, pos2(0.0, 0.0));
        let after = h.app.run("sketch.inspect", json!({})).unwrap();
        assert_eq!(after["curves"].as_array().unwrap().len(), 1);
        assert_eq!(after["constraints"].as_array().unwrap().len(), 2 - ids.len());
        h.app.run("edit.undo", json!({})).unwrap();
    }
}

#[test]
fn component_menu_activates_grounds_and_pastes() {
    let mut app = sample_app();
    let b = body(&app);
    app.run("component.from_bodies", json!({"bodies": [b]})).unwrap();
    let c = app.session.doc.components[0].id;
    let t = Target::Component { id: c };
    act(&mut app, &t, "Activate");
    assert_eq!(app.session.active_component, c);
    // The design's first part comes in grounded (#2804); the menu frees it and grounds it again.
    assert!(app.session.doc.occurrence_of(c).unwrap().grounded);
    act(&mut app, &t, "Unground");
    assert!(!app.session.doc.occurrence_of(c).unwrap().grounded);
    assert!(find(&context_menu::items(&app, &t), "Move/Copy").enabled, "free");
    act(&mut app, &t, "Ground");
    assert!(app.session.doc.occurrence_of(c).unwrap().grounded);
    assert!(!find(&context_menu::items(&app, &t), "Move/Copy").enabled, "grounded");
    // Paste New pastes an independent copy of the copied component into another one (here the
    // design): it is off until a component is copied, and never into itself.
    let placement = Target::Occurrence { path: app.session.doc.first_path(c) };
    app.session.selection = vec![solidcraft_engine::Sel::Occurrence { path: app.session.doc.first_path(c) }];
    act(&mut app, &placement, "Copy");
    assert_eq!(app.copied_placements.len(), 1, "{:?}", app.status);
    let design = Target::Component { id: 0 };
    assert!(find(&context_menu::items(&app, &design), "Paste New").enabled);
    act(&mut app, &design, "Paste New");
    assert_eq!(app.session.doc.components.len(), 2);
}

#[test]
fn move_shortcut_uses_the_selected_nested_instance_and_pending_position() {
    let mut app = SolveApp::new(Session::default(), Services::default());
    let parent = app.run("component.create", json!({"name":"Assembly"})).unwrap()["occurrence"].as_u64().unwrap();
    let child = app.run("component.create", json!({"name":"Carriage"})).unwrap()["occurrence"].as_u64().unwrap();
    app.run("solid.box", json!({"length":30,"width":20,"height":10})).unwrap();
    let copy = app.run("occurrence.copy", json!({"occurrence":parent,"translate":[100,0,0]})).unwrap()["occurrence"].as_u64().unwrap();
    let path = vec![copy, child];
    app.run("component.activate", json!({"component":"root"})).unwrap();
    app.run("occurrence.move", json!({"occurrence":child,"path":path,"translate":[5,0,0],"capture":false})).unwrap();
    let baseline = app.session.pending_moves[&child];
    app.run("select.set", json!({"items":[{"type":"occurrence","path":path}]})).unwrap();
    crate::keymap::bind(&mut app, "solid.move", "Shift+M", false).unwrap();
    crate::keymap::press(&mut app, "M", false);
    assert!(app.tree.occurrence_move.is_none(), "the old shortcut was cleared");
    app.ui.tab = "INSPECT".into();
    crate::keymap::press(&mut app, "Shift+M", false);
    assert_eq!(app.ui.tab, "MODEL", "starting component Move still selects the command's workspace tab");
    let movement = app.tree.occurrence_move.as_ref().expect("component Move opens from its rebound shortcut");
    assert_eq!(movement.occurrence, child);
    assert_eq!(movement.path, path, "use the selected copy's path, not the component's first placement");
    assert_eq!(movement.baseline, Some(baseline));
    assert!(movement.had_pending);
    assert!(app.dialog.is_none(), "the body Move dialog must not open too");
    assert!(app.tree.capture_prompt.is_none(), "continuing a component move does not ask to capture it first");
}

#[test]
fn move_shortcut_respects_grounding_and_keeps_body_selection_as_a_body_move() {
    let mut app = SolveApp::new(Session::default(), Services::default());
    let occurrence = app.run("component.create", json!({"name":"Carriage"})).unwrap()["occurrence"].as_u64().unwrap();
    app.run("solid.box", json!({"length":30,"width":20,"height":10})).unwrap();
    app.run("occurrence.ground", json!({"occurrence":occurrence,"grounded":true})).unwrap();
    app.run("select.set", json!({"items":[{"type":"occurrence","path":[occurrence]}]})).unwrap();
    crate::keymap::press(&mut app, "M", false);
    assert!(app.tree.occurrence_move.is_none(), "a grounded component cannot move");
    assert!(app.dialog.is_none(), "do not fall back to moving the grounded component's bodies");

    app.run("occurrence.ground", json!({"occurrence":occurrence,"grounded":false})).unwrap();
    app.run("select.set", json!({"items":[{"type":"body","name":"Body1"}]})).unwrap();
    crate::keymap::press(&mut app, "M", false);
    assert!(app.tree.occurrence_move.is_none(), "a body selection still edits geometry");
    let dialog = app.dialog.as_ref().expect("body Move opens");
    assert!(matches!(dialog.kind, crate::dialogs::Kind::Move { .. }));
    assert_eq!(dialog.inputs[0].items, vec![solidcraft_engine::Sel::Body { name: "Body1".into() }]);
}

#[test]
fn component_and_occurrence_menus_cycle_shared_identification_color() {
    let mut app = SolveApp::new(Session::default(), Services::default());
    let id = app.run("component.create", json!({"name":"Part"})).unwrap()["component"].as_u64().unwrap();
    let original = app.session.doc.component_identification_color(id);
    assert!(app.session.colored_components.is_none());
    act(&mut app, &Target::Component { id }, "Cycle Component Color");
    let next = app.session.doc.component_identification_color(id);
    assert_ne!(original, next);
    assert!(app.session.colored_components.is_some());
    assert!(app.dialog.is_none());
    let occurrence = app.session.doc.occurrence_of(id).unwrap().id;
    act(&mut app, &Target::Occurrence { path: vec![occurrence] }, "Cycle Component Color");
    assert_ne!(app.session.doc.component_identification_color(id), next);
    app.run("edit.undo", json!({})).unwrap();
    assert_eq!(app.session.doc.component_identification_color(id), next);
}

#[test]
fn pick_runs_an_item_of_the_open_menu() {
    let mut app = sample_app();
    assert!(context_menu::pick(&mut app, "Fit").is_err(), "no menu open");
    context_menu::open_for(&mut app, pos2(500.0, 400.0), Target::Viewport);
    assert!(context_menu::pick(&mut app, "Nope").is_err());
    app.session.undo.clear();
    assert!(context_menu::pick(&mut app, "Undo").is_err(), "disabled");
    assert!(context_menu::pick(&mut app, "ui.origin").is_ok());
    assert!(app.viewport.context_menu.is_none(), "a pick closes the menu");
    assert!(!app.origin_shown());
}

#[test]
fn folder_and_group_menus_make_rename_and_drop_groups() {
    let mut app = sample_app();
    app.run("solid.box", json!({"length": 5, "width": 5, "height": 5, "corner": [200, 0, 0]})).unwrap();
    let names: Vec<String> = app.session.model.state().bodies.iter().map(|b| b.name.clone()).collect();
    let items: Vec<_> = names.iter().map(|n| json!({"type": "body", "name": n})).collect();
    app.run("select.set", json!({ "items": items })).unwrap();
    let folder = Target::Folder { component: 0, folder: "bodies".into(), scope: "c0".into() };
    act(&mut app, &folder, "Group Selected");
    let g = app.session.doc.browser_groups[0].clone();
    assert_eq!(g.items.len(), 2);
    // A body's menu offers grouping too while several are selected.
    assert!(context_menu::items(&app, &Target::Body { name: names[0].clone() }).iter().any(|i| i.label == "Group Selected"));
    let t = Target::Group { id: g.id, scope: "c0".into() };
    act(&mut app, &t, "Rename");
    assert!(context_menu::finish_rename(&mut app, Some("Parts"), true));
    assert_eq!(app.session.doc.browser_groups[0].name, "Parts");
    // Show/Hide hides the group itself: its bodies keep their own state but aren't drawn.
    act(&mut app, &t, "Show/Hide");
    assert!(app.session.visibility.hidden_bodies.is_empty());
    assert!(app.session.folder_hidden(&format!("c0/g{}", g.id)));
    assert!(g.item_keys().all(|b| !app.session.body_visible(b)));
    act(&mut app, &t, "Show/Hide");
    assert!(g.item_keys().all(|b| app.session.body_visible(b)));
    act(&mut app, &t, "Ungroup");
    assert!(app.session.doc.browser_groups.is_empty());
    act(&mut app, &folder, "New Group");
    assert_eq!(app.session.doc.browser_groups[0].items.len(), 0);
}

#[test]
fn browser_folds_are_kept_in_the_preferences() {
    let mut app = sample_app();
    app.tree.collapsed.insert("c0/sketches".into());
    app.tree.expanded.insert("origin".into());
    let prefs = app.prefs();
    let mut other = sample_app();
    other.load_prefs(&prefs);
    assert!(other.tree.collapsed.contains("c0/sketches") && other.tree.expanded.contains("origin"));
}

#[test]
fn expand_and_collapse_all_open_the_browser_tree_from_the_design_menu() {
    let mut app = sample_app();
    let n = app.session.undo.len();
    let root = Target::Component { id: 0 };
    assert!(context_menu::items(&app, &root).iter().any(|i| i.label == "Expand All" && i.id == "browser.expand_all"));
    act(&mut app, &root, "Collapse All");
    assert!(app.tree.collapsed.contains("c0/bodies"), "{:?}", app.tree.collapsed);
    assert!(!app.tree.collapsed.contains("c0"), "the design row stays open: {:?}", app.tree.collapsed);
    assert_eq!(app.session.undo.len(), n);
    act(&mut app, &root, "Expand All");
    assert!(!app.tree.collapsed.contains("c0/bodies"), "{:?}", app.tree.collapsed);
    assert!(app.tree.expanded.contains("origin") && app.tree.expanded.contains("c0/features"), "{:?}", app.tree.expanded);
    assert_eq!(app.session.undo.len(), n);

    let created = app.run("component.create", json!({"name": "Part"})).unwrap();
    let occ = created["occurrence"].as_u64().unwrap();
    let child = app.run("component.create", json!({"name": "Inner", "parent_path": [occ]})).unwrap()["occurrence"].as_u64().unwrap();
    let path = vec![occ];
    act(&mut app, &Target::Occurrence { path: path.clone() }, "Collapse All");
    assert!(app.tree.collapsed.contains(&format!("o:{occ}/{child}")), "its children fold: {:?}", app.tree.collapsed);
    assert!(!app.tree.collapsed.contains(&format!("o:{occ}")), "the clicked row stays open: {:?}", app.tree.collapsed);
    assert!(!app.tree.collapsed.contains("c0/bodies"), "a placement's collapse leaves the rest of the tree: {:?}", app.tree.collapsed);
    act(&mut app, &Target::Occurrence { path }, "Expand All");
    assert!(!app.tree.collapsed.contains(&format!("o:{occ}/{child}")), "{:?}", app.tree.collapsed);
}

#[test]
fn delete_key_handles_every_kind_and_asks_about_dependents() {
    use solidcraft_engine::Sel;
    let mut app = sample_app();
    app.run("construct.plane.offset", json!({"base": "XY", "offset": 10})).unwrap();
    app.run("solid.box", json!({"length": 5, "width": 5, "height": 5, "corner": [200, 0, 0]})).unwrap();
    let n = app.session.doc.features.len();
    // A body and a construction plane together: one undo step, no question.
    let b = app.session.model.state().bodies.last().unwrap().name.clone();
    app.run("select.set", json!({"items": [{"type": "body", "name": b}, {"type": "plane", "name": "Plane1"}]})).unwrap();
    crate::delete::delete_selection(&mut app);
    assert!(app.menu.confirm.is_none());
    assert!(app.session.model.state().body(&b).is_none());
    assert!(app.session.doc.find_feature("Plane1").is_none());
    app.run("edit.undo", json!({})).unwrap();
    assert_eq!(app.session.doc.features.len(), n);
    // A sketch with features built on it: the confirmation lists them first.
    let sk = app.session.doc.features.iter().find(|f| f.name == "Base").unwrap().id;
    app.run("select.set", json!({"items": [Sel::Feature { id: sk }]})).unwrap();
    crate::delete::delete_selection(&mut app);
    let pending = app.menu.confirm.clone().expect("asks first");
    assert!(pending.report["deleted"].as_array().unwrap().len() > 1);
    assert!(crate::delete::confirm(&mut app, false));
    assert_eq!(app.session.doc.features.len(), n, "cancel keeps everything");
    crate::delete::delete_selection(&mut app);
    crate::delete::confirm(&mut app, true);
    assert!(app.session.doc.features.len() < n);
    app.run("edit.undo", json!({})).unwrap();
    assert_eq!(app.session.doc.features.len(), n);
    // A face its neighbours can't close (a side of the plate): refused, nothing changes.
    let body = &app.session.model.state().bodies[0];
    let face = body.body.faces(0.01).unwrap().into_iter().find(|f| f.plane_normal.is_some_and(|n| n.x < -0.99)).unwrap();
    let b0 = body.name.clone();
    app.run("select.set", json!({"items": [{"type": "face", "body": b0, "index": face.index, "point": face.centroid}]})).unwrap();
    crate::delete::delete_selection(&mut app);
    assert!(app.status.as_ref().is_some_and(|s| s.0.contains("Delete Face")), "{:?}", app.status);
    assert_eq!(app.session.doc.features.len(), n);
}

/// #9079: Modify ▸ Delete with nothing selected says what to pick, not the command's parameter
/// error, and changes nothing; with something selected it deletes it, as the Delete key does.
#[test]
fn modify_delete_with_nothing_selected_asks_for_a_pick() {
    let mut app = SolveApp::new(Session::default(), Services::default());
    app.run("solid.box", json!({"length": 10, "width": 10, "height": 10, "body_name": "Box"})).unwrap();
    app.run("construct.plane.offset", json!({"base": "XY", "offset": 10})).unwrap();
    let n = app.session.doc.features.len();
    app.run("select.clear", json!({})).unwrap();
    app.start("timeline.delete");
    let (message, _, error) = app.status.clone().expect("a hint");
    assert!(!error && message == app.tr("ui.delete_select_first"), "{message:?} error: {error}");
    assert!(!message.contains('`'), "no parameter names: {message}");
    assert_eq!(app.session.doc.features.len(), n, "nothing deleted");
    app.run("select.set", json!({"items": [{"type": "plane", "name": "Plane1"}]})).unwrap();
    app.start("timeline.delete");
    assert!(app.session.doc.find_feature("Plane1").is_none(), "the selection goes");
    assert_eq!(app.session.doc.features.len(), n - 1);
}

#[test]
fn browser_feature_menu_deletes_its_target_and_supports_undo() {
    let mut app = SolveApp::new(Session::default(), Services::default());
    for (name, x) in [("Keep", 0), ("Delete", 30)] {
        app.run("solid.box", json!({"length":10,"width":10,"height":10,"corner":[x,0,0],"body_name":name})).unwrap();
    }
    let keep = app.session.doc.features[0].id;
    let target = app.session.doc.features[1].id;
    // A Browser context menu acts on its row, even when another feature is selected.
    app.session.selection = vec![solidcraft_engine::Sel::Feature { id: keep }];
    let saved = app.session.doc.to_json();
    let undo = app.session.undo.len();
    act(&mut app, &Target::Feature { id: target }, "Delete");
    assert!(app.menu.confirm.is_none());
    assert!(app.session.doc.feature(keep).is_some());
    assert!(app.session.doc.feature(target).is_none());
    assert_eq!(app.session.undo.len(), undo + 1);
    app.run("edit.undo", json!({})).unwrap();
    assert_eq!(app.session.doc.to_json(), saved);
    app.run("edit.redo", json!({})).unwrap();
    assert!(app.session.doc.feature(keep).is_some());
    assert!(app.session.doc.feature(target).is_none());
    assert!(!find(&context_menu::items(&app, &Target::Feature { id: target }), "Delete").enabled);
    assert!(!find(&context_menu::items(&app, &Target::Feature { id: target }), "Rename").enabled);
}

#[test]
fn browser_feature_menu_renames_its_target_and_supports_undo() {
    let mut app = SolveApp::new(Session::default(), Services::default());
    for x in [0, 30] {
        app.run("solid.box", json!({"length":10,"width":10,"height":10,"corner":[x,0,0]})).unwrap();
    }
    let keep = app.session.doc.features[0].id;
    let target = app.session.doc.features[1].id;
    let keep_name = app.session.doc.feature(keep).unwrap().name.clone();
    let target_name = app.session.doc.feature(target).unwrap().name.clone();
    app.session.selection = vec![solidcraft_engine::Sel::Feature { id: keep }];
    let saved = app.session.doc.to_json();
    let undo = app.session.undo.len();
    act(&mut app, &Target::Feature { id: target }, "Rename");
    assert_eq!(app.menu.rename.as_ref().unwrap().text, target_name);
    assert!(context_menu::finish_rename(&mut app, Some("Cancelled"), false));
    assert_eq!(app.session.doc.to_json(), saved);
    assert_eq!(app.session.undo.len(), undo);
    act(&mut app, &Target::Feature { id: target }, "Rename");
    assert!(context_menu::finish_rename(&mut app, Some("Renamed feature"), true));
    assert!(app.menu.rename.is_none());
    assert_eq!(app.session.doc.feature(target).unwrap().name, "Renamed feature");
    assert_eq!(app.session.doc.feature(keep).unwrap().name, keep_name);
    assert_eq!(app.session.undo.len(), undo + 1);
    app.run("edit.undo", json!({})).unwrap();
    assert_eq!(app.session.doc.to_json(), saved);
    app.run("edit.redo", json!({})).unwrap();
    assert_eq!(app.session.doc.feature(target).unwrap().name, "Renamed feature");
}

#[test]
fn browser_feature_menu_confirms_dependent_changes_before_deleting() {
    let mut app = sample_app();
    let target = app.session.doc.features[1].id;
    let saved = app.session.doc.to_json();
    let undo = app.session.undo.len();
    act(&mut app, &Target::Feature { id: target }, "Delete");
    assert!(app.menu.confirm.is_some(), "later features depend on the plate's extrusion");
    assert_eq!(app.session.doc.to_json(), saved);
    assert_eq!(app.session.undo.len(), undo);
    assert!(crate::delete::confirm(&mut app, false));
    assert_eq!(app.session.doc.to_json(), saved);
    act(&mut app, &Target::Feature { id: target }, "Delete");
    assert!(crate::delete::confirm(&mut app, true));
    assert!(app.session.doc.feature(target).is_none());
    assert_eq!(app.session.undo.len(), undo + 1);
    app.run("edit.undo", json!({})).unwrap();
    assert_eq!(app.session.doc.to_json(), saved);
}

#[test]
fn menu_delete_items_use_the_same_path() {
    let mut app = sample_app();
    let b = body(&app);
    let original_volume = solidcraft_engine::kernel::measure(&app.session.world_state().body(&b).unwrap().body).unwrap().volume;
    let created = app.run("component.from_bodies", json!({"bodies": [b]})).unwrap();
    let c = app.session.doc.components[0].id;
    let transfer = created["components"][0]["feature"].as_u64().unwrap();
    let transfer_name = app.session.doc.feature(transfer).unwrap().name.clone();
    let saved = app.session.doc.to_json();
    act(&mut app, &Target::Component { id: c }, "Delete");
    let confirmation = app.menu.confirm.as_ref().expect("recorded transfer history needs confirmation");
    assert!(confirmation.report["deleted"].as_array().unwrap().iter().any(|name| name.as_str() == Some(transfer_name.as_str())));
    assert!(crate::delete::confirm(&mut app, false));
    assert_eq!(app.session.doc.to_json(), saved, "cancel keeps component and transfer history");
    act(&mut app, &Target::Component { id: c }, "Delete");
    assert!(crate::delete::confirm(&mut app, true));
    assert!(app.session.doc.components.is_empty());
    assert!(app.session.doc.feature(transfer).is_none());
    let restored = app.session.world_state();
    assert_eq!(restored.bodies.len(), 1, "the original joined plate stays intact");
    let plate = restored.body(&b).unwrap();
    assert_eq!(plate.component, 0);
    assert!((solidcraft_engine::kernel::measure(&plate.body).unwrap().volume - original_volume).abs() < 1e-6);
    assert!(plate.mesh().contains(solidcraft_engine::geom::Vec3::new(40.0, 35.0, 12.0)));
    let mut reopened = solidcraft_engine::Session::new(solidcraft_engine::doc::Document::from_json(&app.session.doc.to_json()).unwrap());
    reopened.refresh();
    let reopened_state = reopened.world_state();
    assert_eq!(reopened_state.bodies.len(), 1);
    assert_eq!(reopened_state.body(&b).unwrap().component, 0);
    assert!((solidcraft_engine::kernel::measure(&reopened_state.body(&b).unwrap().body).unwrap().volume - original_volume).abs() < 1e-6);
    // A sketch from the browser: asks, since its features go too.
    let id = app.session.doc.features.iter().find(|f| f.name == "Base").unwrap().id;
    act(&mut app, &Target::Sketch { id }, "Delete");
    assert!(app.menu.confirm.is_some());
}

#[test]
fn canvas_menu_hides_renames_calibrates_and_deletes() {
    let mut app = sample_app();
    let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
    png.extend(400u32.to_be_bytes());
    png.extend(200u32.to_be_bytes());
    png.extend([8, 6, 0, 0, 0, 0, 0, 0, 0]);
    let data = solidcraft_engine::doc::canvas::base64_encode(&png);
    let id = app.run("canvas.insert", json!({"data": data, "plane": "XY", "width": 80})).unwrap()["canvas"].as_u64().unwrap();
    let t = Target::Canvas { id };
    act(&mut app, &t, "Show/Hide");
    assert!(!app.session.doc.canvases[0].visible);
    act(&mut app, &t, "Rename");
    assert!(context_menu::finish_rename(&mut app, Some("Photo"), true));
    assert_eq!(app.session.doc.canvases[0].name, "Photo");
    act(&mut app, &t, "Calibrate");
    let p = app.tree.canvas_panel.clone().unwrap();
    assert!(p.calibrate && (p.distance - 80.0).abs() < 1e-9, "starts from the image's width");
    act(&mut app, &t, "Delete");
    assert!(app.session.doc.canvases.is_empty());
    app.run("edit.undo", json!({})).unwrap();
    assert_eq!(app.session.doc.canvases.len(), 1);
    // A canvas picked in the browser goes with the Delete key.
    app.tree.picked_canvases = vec![id];
    crate::delete::delete_selection(&mut app);
    assert!(app.session.doc.canvases.is_empty());
}

/// The canvas and browser menus keep their items in a fixed order.
#[test]
fn menus_follow_fusion_order() {
    let mut app = sample_app();
    let b = body(&app);
    let order = |items: &[Item], want: &[&str]| {
        let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
        let pos: Vec<usize> = want.iter().map(|w| labels.iter().position(|l| l == w).unwrap_or_else(|| panic!("no `{w}` in {labels:?}"))).collect();
        assert!(pos.windows(2).all(|p| p[0] < p[1]), "{want:?} out of order in {labels:?}");
    };
    app.run("select.set", json!({"items": [{"type": "face", "body": b, "index": 0, "point": [0, 0, 0]}]})).unwrap();
    let face = context_menu::items(&app, &Target::Viewport);
    order(
        &face,
        &[
            "Create Sketch",
            "Extrude",
            "Offset Plane",
            "Shell",
            "Edit Feature",
            "Edit Profile Sketch",
            "Appearance",
            "Properties",
            "Delete",
            "Show/Hide",
            "Find in Browser",
        ],
    );
    assert_eq!(find(&face, "Extrude").shortcut, "E");
    assert!(find(&face, "Edit Profile Sketch").enabled, "the plate's extrude has a sketch");
    let body_menu = context_menu::items(&app, &Target::Body { name: b.clone() });
    order(
        &body_menu,
        &[
            "Move/Copy",
            "Create Components from Bodies",
            "Physical Material",
            "Appearance",
            "Properties",
            "Save As Mesh",
            "Delete",
            "Remove",
            "Rename",
            "Show/Hide",
            "Isolate",
        ],
    );
    let id = app.session.doc.features.iter().find(|f| f.name == "Base").unwrap().id;
    let sketch = context_menu::items(&app, &Target::Sketch { id });
    order(
        &sketch,
        &[
            "Extrude",
            "Edit Sketch",
            "Redefine Sketch Plane",
            "Export DXF…",
            "Delete",
            "Rename",
            "Look At",
            "Hide Profile",
            "Show Dimension",
            "Show/Hide",
            "Find in Timeline",
        ],
    );
    let folder = context_menu::items(&app, &Target::Folder { component: 0, folder: "bodies".into(), scope: "c0".into() });
    order(&folder, &["Create Components from Bodies", "New Group", "Show/Hide", "Show All"]);
    let origin = context_menu::items(&app, &Target::Origin);
    order(&origin, &["Show/Hide", "Show All", "Hide Planes", "Hide Axes"]);
    context_menu::run_item(&mut app, &find(&origin, "Hide Planes"), pos2(0.0, 0.0));
    assert!(["XY", "XZ", "YZ"].iter().all(|p| app.session.visibility.hidden_origin.iter().any(|h| h == p)));
    app.run("select.clear", json!({})).unwrap();
    let empty = context_menu::items(&app, &Target::Viewport);
    order(&empty, &["Pan", "Zoom", "Orbit", "Show All", "Unisolate", "Extrude", "Fillet"]);
    context_menu::run_item(&mut app, &find(&empty, "Pan"), pos2(0.0, 0.0));
    assert!(matches!(app.viewport.nav, Some(crate::viewport::NavMode::Pan)));
}

#[test]
fn v_toggles_what_is_selected() {
    let mut app = sample_app();
    let b = body(&app);
    let sk = app.session.doc.features.iter().find(|f| f.name == "Base").unwrap().id;
    app.run("select.set", json!({"items": [{"type": "body", "name": b}, {"type": "plane", "name": "XY"}, {"type": "feature", "id": sk}]})).unwrap();
    crate::browser::set_sketch_visible(&mut app, sk, true);
    context_menu::toggle_visibility(&mut app);
    assert!(app.session.visibility.hidden_bodies.contains(&b) && app.session.visibility.hidden_origin.iter().any(|h| h == "XY"));
    assert!(!crate::browser::sketch_visible(&app, sk));
    context_menu::toggle_visibility(&mut app);
    assert!(app.session.visibility.hidden_bodies.is_empty() && app.session.visibility.hidden_origin.is_empty());
    assert!(crate::browser::sketch_visible(&app, sk));
    // Components picked in the browser hide their bodies.
    app.run("component.from_bodies", json!({"bodies": [b.clone()]})).unwrap();
    app.run("select.clear", json!({})).unwrap();
    app.tree.picked_components = vec![app.session.doc.components[0].id];
    context_menu::toggle_visibility(&mut app);
    assert_eq!(app.session.visibility.hidden_bodies, vec![b]);
    assert_eq!(find(&context_menu::items(&app, &Target::Body { name: body(&app) }), "Show/Hide").shortcut, "V");
}

/// V pressed with a browser menu open runs that menu's Show/Hide, as its row says, on the row
/// the menu was opened for: a folder, the Origin folder or an origin plane is not selected by
/// the right-click, so V used to act on the selection instead (nothing, or another body).
#[test]
fn v_with_a_menu_open_shows_or_hides_what_the_menu_is_for() {
    let mut h = crate::scenario::Harness::new();
    h.app.run("solid.box", json!({"length": 10, "width": 20, "height": 30})).unwrap();
    let b = body(&h.app);
    let press_v = |h: &mut crate::scenario::Harness| {
        for pressed in [true, false] {
            h.app.synthetic.push(egui::Event::Key { key: egui::Key::V, physical_key: None, pressed, repeat: false, modifiers: Default::default() });
        }
        while !h.app.synthetic.is_empty() {
            h.frame();
        }
        h.frames(2);
    };
    let open = |h: &mut crate::scenario::Harness, target: Target| {
        context_menu::open_for(&mut h.app, pos2(300.0, 200.0), target);
        h.frames(2);
    };
    // A body selected elsewhere is not what the folder's menu is for.
    h.app.run("select.set", json!({"items": [{"type": "body", "name": b}]})).unwrap();
    let bodies = Target::Folder { component: 0, folder: "bodies".into(), scope: "c0".into() };
    for hidden in [true, false] {
        open(&mut h, bodies.clone());
        press_v(&mut h);
        assert_eq!(h.app.session.folder_hidden("c0/bodies"), hidden, "V on the Bodies folder's menu");
        assert!(h.app.viewport.context_menu.is_none(), "the menu closes as after a click");
        assert!(!h.app.session.visibility.hidden_bodies.contains(&b), "the selected body is left alone");
    }
    let shown = h.app.origin_shown();
    open(&mut h, Target::Origin);
    press_v(&mut h);
    assert_eq!(h.app.origin_shown(), !shown, "V on the Origin folder's menu");
    open(&mut h, Target::OriginPlane { name: "XY".into() });
    press_v(&mut h);
    assert!(h.app.session.visibility.hidden_origin.iter().any(|o| o == "XY"), "V on an origin plane's menu");
    open(&mut h, Target::OriginAxis { name: "Z".into() });
    press_v(&mut h);
    assert!(h.app.session.visibility.hidden_origin.iter().any(|o| o == "Z"), "V on an origin axis's menu (#8382)");
    assert!(!h.app.session.visibility.hidden_bodies.contains(&b));
    // With no menu open, V still flips the selection.
    press_v(&mut h);
    assert!(h.app.session.visibility.hidden_bodies.contains(&b), "V without a menu acts on the selection");
}

#[test]
fn named_views_save_restore_rename_and_delete() {
    let mut app = sample_app();
    act(&mut app, &Target::NamedViews, "New Named View");
    assert_eq!(app.session.doc.named_views.len(), 1);
    let saved = app.cam;
    app.cam.set_view(solidcraft_engine::render::StandardView::Top);
    let t = Target::NamedView { name: "Named View1".into() };
    act(&mut app, &t, "Restore");
    assert_eq!(app.cam_anim.map(|a| a.to), Some(saved), "animates back to the saved camera");
    act(&mut app, &t, "Rename");
    assert!(context_menu::finish_rename(&mut app, Some("Front detail"), true));
    let t = Target::NamedView { name: "Front detail".into() };
    act(&mut app, &t, "Delete");
    assert!(app.session.doc.named_views.is_empty());
    // Standard views restore but can't be renamed or deleted.
    let top = context_menu::items(&app, &Target::NamedView { name: "Top".into() });
    assert!(!find(&top, "Delete").enabled);
    context_menu::run_item(&mut app, &find(&top, "Restore"), pos2(0.0, 0.0));
    assert!(app.cam_anim.is_some());
}

#[test]
fn units_menu_changes_the_design_units_and_new_designs_take_the_preference() {
    let mut app = sample_app();
    let items = context_menu::items(&app, &Target::Units);
    assert_eq!(items.len(), 5);
    act(&mut app, &Target::Units, "in");
    assert_eq!(app.session.doc.units, "in");
    assert!(crate::prefs::show_mm(&app, 25.4, 1).starts_with("1.000 in"));
    app.preferences.default_units = "cm".into();
    crate::documents::new_design(&mut app);
    assert_eq!(app.session.doc.units, "cm");
    assert!(!app.session.is_dirty(), "a fresh design stays unmodified");
}

#[test]
fn an_uncaptured_move_asks_before_the_next_command() {
    let mut app = sample_app();
    let b = body(&app);
    app.run("component.from_bodies", json!({"bodies": [b]})).unwrap();
    let occ = app.session.doc.occurrences[0].id;
    app.run("occurrence.ground", json!({"occurrence": occ, "grounded": false})).unwrap();
    app.run("occurrence.move", json!({"occurrence": occ, "translate": [10, 0, 0], "capture": false})).unwrap();
    app.start("sketch.create");
    assert_eq!(app.tree.capture_prompt.as_deref(), Some("sketch.create"), "asks first");
    assert!(app.dialog.is_none());
    crate::browser::answer_capture(&mut app, "cancel").unwrap();
    assert!(app.tree.capture_prompt.is_none() && !app.session.pending_moves.is_empty());
    // Revert, then the command goes ahead.
    app.start("sketch.create");
    crate::browser::answer_capture(&mut app, "revert").unwrap();
    assert!(app.session.pending_moves.is_empty());
    assert!(app.dialog.is_some(), "Create Sketch started");
    assert_eq!(app.session.doc.occurrences[0].transform, solidcraft_engine::doc::IDENTITY);
    // Capture keeps the move.
    app.dialog = None;
    app.run("occurrence.move", json!({"occurrence": occ, "translate": [10, 0, 0], "capture": false})).unwrap();
    app.start("solid.extrude");
    crate::browser::answer_capture(&mut app, "capture").unwrap();
    assert_eq!(app.session.doc.occurrences[0].transform[3][0], 10.0);
    assert!(crate::browser::answer_capture(&mut app, "capture").is_err(), "nothing waits now");
}

/// #9736: a move not captured yet is a change to save. Closing the tab asks "save changes?";
/// Save asks the Capture Position question before it writes, and the close waits for it.
/// Cancel keeps the tab and the move; Capture saves the move and the tab closes. Save As asks
/// the same question first.
#[test]
fn closing_with_an_uncaptured_move_asks_to_save_and_to_capture() {
    let dir = std::env::temp_dir().join(format!("solidcraft-close-pending-move-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("two.sc");
    let mut app = sample_app();
    let b = body(&app);
    app.run("component.from_bodies", json!({"bodies": [b]})).unwrap();
    let occ = app.session.doc.occurrences[0].id;
    app.run("occurrence.ground", json!({"occurrence": occ, "grounded": false})).unwrap();
    app.run("file.save_as", json!({"path": path.to_string_lossy()})).unwrap();
    assert!(!crate::documents::any_dirty(&app));
    app.run("occurrence.move", json!({"occurrence": occ, "translate": [10, 0, 0], "capture": false})).unwrap();
    assert!(crate::documents::any_dirty(&app), "the move is a change to save");

    crate::toolbar::save_as(&mut app);
    assert!(app.tree.capture_prompt.as_deref().is_some_and(|q| q.starts_with("__save_as:")), "{:?}", app.tree.capture_prompt);
    assert!(!app.file_picker_busy(), "no file is picked before the answer");
    crate::browser::answer_capture(&mut app, "cancel").unwrap();

    crate::documents::close(&mut app, 0, false);
    assert_eq!(app.docs.closing, Some(0), "asks to save");
    crate::documents::answer_prompt(&mut app, crate::documents::Answer::Save);
    assert_eq!(app.tree.capture_prompt.as_deref(), Some(crate::browser::SAVE));
    assert_eq!(app.docs.closing, Some(0), "the close waits for the answer");
    crate::documents::answer_prompt(&mut app, crate::documents::Answer::DontSave);
    assert_eq!(app.docs.closing, Some(0), "the Capture Position question is the one to answer now");
    crate::browser::answer_capture(&mut app, "cancel").unwrap();
    assert!(app.docs.closing.is_none() && !app.session.pending_moves.is_empty(), "still open, with the move");

    crate::documents::close(&mut app, 0, false);
    crate::documents::answer_prompt(&mut app, crate::documents::Answer::Save);
    crate::browser::answer_capture(&mut app, "capture").unwrap();
    crate::save_job::tick(&mut app, true);
    assert!(app.docs.closing.is_none() && app.session.path.is_none(), "saved and closed: a blank design is left");
    let saved = solidcraft_engine::io::read_design(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(saved.occurrences[0].transform[3][0], 10.0, "the file holds the move");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn f2_renames_the_selected_item() {
    let mut app = sample_app();
    let b = body(&app);
    app.run("select.set", json!({"items": [{"type": "body", "name": b}]})).unwrap();
    assert!(context_menu::rename_selection(&mut app, pos2(0.0, 0.0)));
    assert!(context_menu::finish_rename(&mut app, Some("Base"), true));
    assert!(app.session.model.state().body("Base").is_some());
    app.run("select.clear", json!({})).unwrap();
    assert!(!context_menu::rename_selection(&mut app, pos2(0.0, 0.0)), "nothing selected");
}

/// Rename shows F2 wherever F2 renames (bodies, sketches and features, construction geometry,
/// components and placements, groups, canvases), as Cut, Copy, Delete and Show/Hide show their
/// keys; joints and saved views, which F2 does not rename, show none. F2 pressed with such a
/// menu open renames what the menu is for, as its row says: a group or canvas is not selected by
/// the right-click that opens its menu, so F2 would rename the selection instead.
#[test]
fn rename_shows_f2_and_f2_with_a_menu_open_renames_what_the_menu_is_for() {
    use crate::context_menu::RenameWhat;
    let mut h = crate::scenario::Harness::new();
    h.app.run("solid.box", json!({"length": 10, "width": 20, "height": 30})).unwrap();
    let b = body(&h.app);
    let feature = h.app.session.doc.features[0].id;
    h.app.run("construct.plane.offset", json!({"base": "XY", "offset": 5})).unwrap();
    let plane = h.app.session.doc.features.last().unwrap().id;
    let part = h.app.run("component.create", json!({"name": "Menu part"})).unwrap()["component"].as_u64().unwrap();
    let occurrence = h.app.session.doc.occurrence_of(part).unwrap().id;
    let group = h.app.run("browser.group", json!({"component": 0, "folder": "bodies", "items": [b]})).unwrap()["group"].as_u64().unwrap();
    let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
    png.extend(400u32.to_be_bytes());
    png.extend(200u32.to_be_bytes());
    png.extend([8, 6, 0, 0, 0, 0, 0, 0, 0]);
    let data = solidcraft_engine::doc::canvas::base64_encode(&png);
    let canvas = h.app.run("canvas.insert", json!({"data": data, "plane": "XY", "width": 80})).unwrap()["canvas"].as_u64().unwrap();
    let rename_key = |app: &SolveApp, target: &Target| {
        context_menu::items(app, target)
            .into_iter()
            .find(|i| i.id == "ui.rename")
            .map(|i| i.shortcut)
            .unwrap_or_else(|| panic!("no Rename row: {target:?}"))
    };
    for target in [
        Target::Body { name: b.clone() },
        Target::Sketch { id: feature },
        Target::Feature { id: feature },
        Target::Plane { id: plane },
        Target::Component { id: part },
        Target::Occurrence { path: vec![occurrence] },
        Target::Group { id: group, scope: "c0".into() },
        Target::Canvas { id: canvas },
    ] {
        assert_eq!(rename_key(&h.app, &target), "F2", "Rename shows F2: {target:?}");
    }
    assert_eq!(rename_key(&h.app, &Target::NamedView { name: "Top".into() }), "", "F2 does not rename a saved view");
    let sample = solidcraft_engine::sample::samples().into_iter().find(|s| s.name == "Hinge Assembly").unwrap();
    let mut assembly = SolveApp::new(Session::default(), Services::default());
    assembly.session.run_script(&(sample.script)()).unwrap();
    let joint = assembly.session.doc.assembly.joints[0].id;
    assert_eq!(rename_key(&assembly, &Target::Joint { id: joint }), "", "F2 does not rename a joint");

    let press_f2 = |h: &mut crate::scenario::Harness| {
        for pressed in [true, false] {
            h.app.synthetic.push(egui::Event::Key { key: egui::Key::F2, physical_key: None, pressed, repeat: false, modifiers: Default::default() });
        }
        while !h.app.synthetic.is_empty() {
            h.frame();
        }
    };
    // A body selected elsewhere is not what the group's or the canvas's menu is for.
    h.app.run("select.set", json!({"items": [{"type": "body", "name": b}]})).unwrap();
    for (target, what) in
        [(Target::Group { id: group, scope: "c0".into() }, RenameWhat::Group(group)), (Target::Canvas { id: canvas }, RenameWhat::Canvas(canvas))]
    {
        context_menu::open_for(&mut h.app, pos2(300.0, 200.0), target.clone());
        h.frames(2);
        press_f2(&mut h);
        let renaming = h.app.menu.rename.as_ref().map(|r| r.what.clone());
        assert_eq!(renaming, Some(what), "F2 on the menu of {target:?}");
        assert!(h.app.viewport.context_menu.is_none(), "the menu closes as after a click");
        h.step(&json!({"key": "Escape"})).unwrap();
        h.frames(2);
        assert!(h.app.menu.rename.is_none());
    }
    // With no menu open, F2 still renames the selection.
    h.app.run("select.set", json!({"items": [{"type": "body", "name": b}]})).unwrap();
    h.frames(2);
    press_f2(&mut h);
    assert_eq!(h.app.menu.rename.as_ref().map(|r| r.what.clone()), Some(RenameWhat::Body(b)), "F2 without a menu renames the selection");
    h.step(&json!({"key": "Escape"})).unwrap();
    h.frames(2);
    assert!(h.app.menu.rename.is_none());
}

/// A double-click on a body's browser row edits its name in the row, like F2 (no floating box);
/// Escape cancels, and a second double-click then commits a new name.
#[test]
fn double_click_on_a_body_row_renames_it_in_the_row() {
    let mut h = crate::scenario::Harness::new();
    h.app.run("solid.box", json!({"length": 40, "width": 40, "height": 10, "body_name": "Block"})).unwrap();
    h.app.run("select.clear", json!({})).unwrap();
    h.frames(3);
    let at = crate::scenario::handle_at("row:Block").expect("the body has a browser row");
    h.call("ui.click", json!({"x": at.x, "y": at.y, "double": true}));
    h.frames(3);
    let rename = h.app.menu.rename.as_ref().expect("a double-click starts the rename");
    assert!(rename.inline, "the name is edited in its row");
    assert!(matches!(&rename.what, context_menu::RenameWhat::Body(b) if b == "Block"));
    assert!(crate::scenario::handle_at("rename:Block").is_some(), "the row shows its name editor");
    // Escape leaves the name alone.
    h.step(&json!({"key": "Escape"})).unwrap();
    h.frames(2);
    assert!(h.app.menu.rename.is_none());
    assert!(h.app.session.model.state().body("Block").is_some());
    // Past egui's double-click window, so this is a new double-click and not a triple.
    h.frames(40);
    let at = crate::scenario::handle_at("row:Block").expect("the row is still there");
    h.call("ui.click", json!({"x": at.x, "y": at.y, "double": true}));
    h.frames(3);
    assert!(context_menu::finish_rename(&mut h.app, Some("Base"), true));
    assert!(h.app.session.model.state().body("Base").is_some());
}

#[test]
fn occurrence_moves_turn_about_their_origin_then_move() {
    let mut app = sample_app();
    let b = body(&app);
    app.run("component.from_bodies", json!({"bodies": [b]})).unwrap();
    let occ = app.session.doc.occurrences[0].id;
    app.run("occurrence.ground", json!({"occurrence": occ, "grounded": false})).unwrap();
    let mut m = crate::browser::OccMove::new(occ);
    m.pivot = Some(solidcraft_engine::geom::Vec3::new(5.0, 0.0, 0.0));
    m.translate = [10.0, 0.0, 0.0];
    m.angles = [0.0, 0.0, 90.0];
    crate::browser::apply_occurrence_move(&mut app, &m);
    let t = *app.session.pending_moves.get(&occ).unwrap();
    // The pivot lands 10 mm along X; X turns into Y.
    let p = solidcraft_engine::doc::apply_point(&t, solidcraft_engine::geom::Vec3::new(5.0, 0.0, 0.0));
    let x = solidcraft_engine::doc::apply_vector(&t, solidcraft_engine::geom::Vec3::X);
    assert!((p.x - 15.0).abs() < 1e-9 && p.y.abs() < 1e-9, "{p:?}");
    assert!((x.y - 1.0).abs() < 1e-9, "{x:?}");
    // Changing the values again starts from where it was, not from the last move.
    m.translate = [0.0, 0.0, 0.0];
    m.angles = [0.0; 3];
    crate::browser::apply_occurrence_move(&mut app, &m);
    assert_eq!(*app.session.pending_moves.get(&occ).unwrap(), solidcraft_engine::doc::IDENTITY);
}

#[test]
fn toolbar_pins_removes_reorders_and_keeps_its_layout() {
    use crate::toolbar_custom as tc;
    let mut app = sample_app();
    app.ui.tab = "MODEL".into();
    let ids = |app: &SolveApp, panel: &str| -> Vec<&'static str> {
        let lists = tc::panel_lists("MODEL");
        let (_, cmds, n) = lists.iter().find(|(p, ..)| *p == panel).unwrap();
        tc::buttons(app, "MODEL", panel, cmds, *n).iter().map(|c| c.id).collect()
    };
    let create = ids(&app, "CREATE");
    // Pin a command that isn't a button yet: it lands at the end of its panel.
    let extra =
        tc::panel_lists("MODEL").iter().find(|(p, ..)| *p == "CREATE").unwrap().1.iter().map(|c| c.id).find(|id| !create.contains(id)).unwrap();
    act(&mut app, &Target::ToolbarCommand { id: extra.into() }, "Pin to Toolbar");
    assert_eq!(ids(&app, "CREATE").last(), Some(&extra));
    // Remove a default button.
    act(&mut app, &Target::ToolbarCommand { id: create[0].into() }, "Remove from Toolbar");
    assert!(!ids(&app, "CREATE").contains(&create[0]));
    // Reorder: the pinned one before the second default; then into another panel.
    tc::reorder(&mut app, "MODEL", extra, "CREATE", Some(create[1]));
    assert_eq!(ids(&app, "CREATE")[0], extra);
    tc::reorder(&mut app, "MODEL", extra, "MODIFY", None);
    assert!(!ids(&app, "CREATE").contains(&extra) && ids(&app, "MODIFY").last() == Some(&extra));
    // Pin to Shortcuts goes to the S box's favourites.
    act(&mut app, &Target::ToolbarCommand { id: "solid.extrude".into() }, "Pin to Shortcuts");
    assert!(app.sbox.pinned.iter().any(|p| p == "solid.extrude"));
    // Kept between runs, and exported and imported with the shortcuts.
    let mut other = sample_app();
    other.load_prefs(&app.prefs());
    assert_eq!(other.toolbar_custom, app.toolbar_custom);
    let file = tc::export(&app);
    let mut third = sample_app();
    tc::import(&mut third, &file).unwrap();
    assert_eq!(third.toolbar_custom, app.toolbar_custom);
    assert!(tc::import(&mut third, &json!({"format": "nope"})).is_err());
    // Reset Toolbar puts the tab back.
    act(&mut app, &Target::ToolbarCommand { id: extra.into() }, "Reset Toolbar");
    assert_eq!(ids(&app, "CREATE"), create);
}

#[test]
fn the_workspace_menu_lists_design_and_render_and_greys_future_workspaces() {
    let app = sample_app();
    let items = context_menu::items(&app, &Target::Workspace);
    assert!(find(&items, "Design").enabled);
    assert!(find(&items, "Render").enabled);
    assert!(!find(&items, "Animation").enabled);
    assert!(!items.iter().any(|i| i.label.contains("Generative")));
}

/// A right-click on a construction plane's browser row opens its menu, like a body's or a
/// sketch's row, and the menu's items act on that plane.
#[test]
fn construction_plane_row_opens_its_menu() {
    let mut h = crate::scenario::Harness::new();
    h.app.run("solid.box", json!({"length": 40, "width": 40, "height": 10, "body_name": "Block"})).unwrap();
    h.app.run("construct.plane.offset", json!({"base": "XY", "offset": 25, "name": "Cut"})).unwrap();
    let id = h.app.session.doc.find_feature("Cut").map(|f| f.id).unwrap();
    h.app.run("select.clear", json!({})).unwrap();
    h.frames(3);
    let at = crate::scenario::handle_at("row:Cut").expect("the plane has a browser row");
    h.call("ui.click", json!({"x": at.x, "y": at.y, "button": "right"}));
    h.frames(2);
    let menu = h.call("ui.menu", json!({}));
    let menu = &menu["result"];
    assert_eq!(menu["open"], true, "{menu}");
    assert_eq!(menu["target"], json!({"type": "plane", "id": id}), "{menu}");
    let labels: Vec<&str> = menu["items"].as_array().unwrap().iter().filter_map(|i| i["label"].as_str()).filter(|l| !l.is_empty()).collect();
    assert_eq!(labels, ["Create Sketch", "Edit Feature", "Delete", "Rename", "Show/Hide", "Find in Timeline"]);
    assert_eq!(h.app.session.selection, vec![solidcraft_engine::Sel::Plane { name: "Cut".into() }], "the right-click picks the plane");
    context_menu::close(&mut h.app);

    let t = Target::Plane { id };
    act(&mut h.app, &t, "Show/Hide");
    assert!(h.app.session.visibility.hidden_origin.contains(&"Cut".to_string()));
    act(&mut h.app, &t, "Show/Hide");
    assert!(!h.app.session.visibility.hidden_origin.contains(&"Cut".to_string()));
    act(&mut h.app, &t, "Rename");
    assert!(context_menu::finish_rename(&mut h.app, Some("Section"), true));
    assert!(h.app.session.doc.find_feature("Section").is_some_and(|f| f.id == id));
    act(&mut h.app, &t, "Find in Timeline");
    assert_eq!(h.app.session.selection, vec![solidcraft_engine::Sel::Feature { id }]);
    act(&mut h.app, &t, "Edit Feature");
    assert!(h.app.dialog.is_some(), "Edit Feature opens the plane's dialog");
    h.app.dialog = None;
    act(&mut h.app, &t, "Delete");
    assert!(h.app.session.doc.feature(id).is_none(), "nothing depends on the plane: it goes at once");
}

/// A right-click on a plane offers a sketch on it (#1354): an origin plane in the viewport (it
/// stays unselected), an origin plane's browser row and a construction plane's row. Picking it
/// starts the sketch on that plane.
#[test]
fn right_click_on_a_plane_offers_a_sketch_on_it() {
    use solidcraft_engine::geom::Vec3;
    let normal = |h: &crate::scenario::Harness| h.app.session.world_state().sketches.first().map(|s| s.plane.normal()).unwrap_or_default();
    for (plane, axis) in [("XY", Vec3::Z), ("XZ", Vec3::Y), ("YZ", Vec3::X)] {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.settle();
        let p = h.at(&json!({"plane": plane})).unwrap();
        h.call("ui.menu", json!({"x": p[0], "y": p[1]}));
        let menu = h.call("ui.menu", json!({}))["result"].clone();
        let items = menu["items"].as_array().unwrap();
        let labels: Vec<&str> = items.iter().filter_map(|i| i["label"].as_str()).filter(|l| !l.is_empty()).collect();
        assert_eq!(labels[..4], ["Select Other…", "Create Sketch", "Look At", "Pan"], "{plane}: {labels:?}");
        let sketch = items.iter().find(|i| i["label"] == "Create Sketch").unwrap();
        assert_eq!(sketch["params"], json!({"plane": plane}));
        assert!(h.app.session.selection.is_empty(), "the right-click leaves the plane unselected");
        assert_eq!(h.call("ui.menuPick", json!({"item": "Create Sketch"}))["ok"], true);
        assert!(h.app.session.active_sketch.is_some(), "{plane}: picking it starts a sketch");
        assert!(normal(&h).dot(axis).abs() > 0.999, "{plane}: sketched on {:?}", normal(&h));
    }

    let mut h = crate::scenario::Harness::new();
    h.app.home.open = false;
    crate::browser::toggle(&mut h.app, "origin", false);
    h.frames(3);
    let at = crate::scenario::handle_at("row:XZ").expect("the Origin folder lists XZ");
    h.call("ui.click", json!({"x": at.x, "y": at.y, "button": "right"}));
    h.frames(2);
    let menu = h.call("ui.menu", json!({}))["result"].clone();
    assert_eq!(menu["target"], json!({"type": "origin_plane", "name": "XZ"}), "{menu}");
    let t = Target::OriginPlane { name: "XZ".into() };
    act(&mut h.app, &t, "Show/Hide");
    assert!(h.app.session.visibility.hidden_origin.contains(&"XZ".to_string()));
    act(&mut h.app, &t, "Show/Hide");
    assert!(!h.app.session.visibility.hidden_origin.contains(&"XZ".to_string()));
    context_menu::close(&mut h.app);
    // The toolbar's Create Sketch is already waiting for a plane: the menu's replaces it.
    h.app.start("sketch.create");
    assert!(h.app.dialog.is_some());
    act(&mut h.app, &t, "Create Sketch");
    h.frames(3);
    assert!(h.app.dialog.is_none(), "no Create Sketch dialog is left waiting for a plane");
    assert!(normal(&h).dot(Vec3::Y).abs() > 0.999);

    let mut h = crate::scenario::Harness::new();
    h.app.run("construct.plane.offset", json!({"base": "XY", "offset": 25, "name": "Cut"})).unwrap();
    let id = h.app.session.doc.find_feature("Cut").map(|f| f.id).unwrap();
    act(&mut h.app, &Target::Plane { id }, "Create Sketch");
    h.frames(3);
    let sketch = h.app.session.active_sketch.and_then(|s| h.app.session.world_state().sketch(s).map(|s| s.plane.origin.z));
    assert_eq!(sketch, Some(25.0), "the sketch is on the construction plane");
}

/// A right-click on an origin axis row opens its own menu, with the planes' Show/Hide (#8382).
#[test]
fn right_click_on_an_origin_axis_offers_show_hide() {
    let mut h = crate::scenario::Harness::new();
    h.app.home.open = false;
    crate::browser::toggle(&mut h.app, "origin", false);
    h.frames(3);
    let at = crate::scenario::handle_at("row:Y").expect("the Origin folder lists the Y axis");
    h.call("ui.click", json!({"x": at.x, "y": at.y, "button": "right"}));
    h.frames(2);
    let menu = h.call("ui.menu", json!({}))["result"].clone();
    assert_eq!(menu["target"], json!({"type": "origin_axis", "name": "Y"}), "{menu}");
    let t = Target::OriginAxis { name: "Y".into() };
    let labels: Vec<String> = context_menu::items(&h.app, &t).iter().map(|i| i.label.clone()).collect();
    assert_eq!(labels, ["Show/Hide"], "an axis offers no Create Sketch");
    act(&mut h.app, &t, "Show/Hide");
    assert!(h.app.session.visibility.hidden_origin.contains(&"Y".to_string()), "hidden through the menu");
    act(&mut h.app, &t, "Show/Hide");
    assert!(!h.app.session.visibility.hidden_origin.contains(&"Y".to_string()), "shown again");
}

/// A right-click on empty space leads with Create Sketch too (#3032), in the same place as on a
/// plane: the toolbar's, which asks for the plane or face. Either way the menu offers it once.
#[test]
fn empty_space_leads_with_create_sketch() {
    let mut app = sample_app();
    let items = context_menu::items(&app, &Target::Viewport);
    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).filter(|l| !l.is_empty()).collect();
    assert_eq!(labels[..2], ["Create Sketch", "Pan"], "{labels:?}");
    assert_eq!(items.first().map(|i| i.label.as_str()), Some("Create Sketch"), "first, as on a plane");
    assert!(items.get(1).is_some_and(|i| i.id == "-"), "then a separator, as on a plane");
    assert_eq!(labels.iter().filter(|l| **l == "Create Sketch").count(), 1, "{labels:?}");
    let sketch = find(&items, "Create Sketch");
    assert!(sketch.enabled && sketch.params.is_none(), "{sketch:?}");
    // A plane under the cursor: still one Create Sketch, first, on that plane.
    app.menu.plane = Some("XY".into());
    let on_plane = context_menu::items(&app, &Target::Viewport);
    let sketches: Vec<&Item> = on_plane.iter().filter(|i| i.label == "Create Sketch").collect();
    assert_eq!(sketches.len(), 1);
    assert_eq!(sketches[0].params, Some(json!({"plane": "XY"})));
    assert_eq!(on_plane.first().map(|i| i.label.as_str()), Some("Create Sketch"));
    // Picking the empty-space one starts the toolbar's Create Sketch, waiting for a plane.
    app.menu.plane = None;
    context_menu::run_item(&mut app, &sketch, pos2(0.0, 0.0));
    assert!(matches!(app.dialog.as_ref().map(|d| &d.kind), Some(crate::dialogs::Kind::Sketch)), "Create Sketch started");
}

/// Create Sketch from a right-click on empty space sketches on the next plane clicked (#3032).
#[test]
fn create_sketch_from_empty_space_sketches_on_the_plane_clicked_next() {
    use solidcraft_engine::geom::Vec3;
    let mut h = crate::scenario::Harness::new();
    h.app.home.open = false;
    h.settle();
    let view = h.call("ui.inspect", json!({}))["result"]["viewport"].clone();
    let corner = [view[0].as_f64().unwrap() + 40.0, view[3].as_f64().unwrap() - 40.0];
    h.call("ui.menu", json!({"x": corner[0], "y": corner[1]}));
    let menu = h.call("ui.menu", json!({}))["result"].clone();
    let first = menu["items"].as_array().unwrap().iter().find_map(|i| i["label"].as_str().filter(|l| !l.is_empty()));
    assert_eq!(first, Some("Create Sketch"), "{menu}");
    assert_eq!(h.call("ui.menuPick", json!({"item": "Create Sketch"}))["ok"], true);
    assert!(h.app.session.active_sketch.is_none(), "it waits for a plane or face");
    let p = h.at(&json!({"plane": "XY"})).unwrap();
    h.call("ui.click", json!({"x": p[0], "y": p[1]}));
    h.frames(3);
    let normal = h.app.session.active_sketch.and_then(|s| h.app.session.world_state().sketch(s).map(|s| s.plane.normal()));
    assert!(normal.is_some_and(|n| n.dot(Vec3::Z).abs() > 0.999), "sketching on XY: {normal:?}");
}

/// Menus opened near the window's bottom right corner, in the viewport or the browser, open up
/// or to the left where they would not fit, ending at the cursor and staying in the window, at
/// any UI scale (#1354).
#[test]
fn menus_near_the_window_corner_open_inward() {
    for scale in [0.7, 1.0, 1.5] {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.call("ui.prefs", json!({"ui_scale": scale}));
        let screen = h.ctx.content_rect();
        let vp = h.app.viewport.rect.unwrap();
        let at = vp.right_bottom() - egui::vec2(40.0, 40.0);
        for target in [None, Some(json!({"type": "origin"}))] {
            let mut params = json!({"x": at.x, "y": at.y});
            if let Some(t) = &target {
                params["target"] = t.clone();
            }
            h.call("ui.menu", params);
            assert_eq!(h.call("ui.menu", json!({}))["result"]["open"], true, "{scale} {target:?}");
            // An auto-sized menu takes a few frames to settle its size.
            h.frames(3);
            let r = h.ctx.memory(|m| m.area_rect(egui::Id::new("sc_context_menu"))).unwrap();
            assert!(screen.contains_rect(r), "{scale} {target:?}: {r:?} outside {screen:?}");
            // Below and right of the cursor where it fits, else ending at it.
            let x = if at.x + r.width() <= screen.right() { at.x } else { at.x - r.width() };
            let y = if at.y + r.height() <= screen.bottom() { at.y } else { at.y - r.height() };
            assert!((r.left() - x).abs() <= 2.0 && (r.top() - y).abs() <= 2.0, "{scale} {target:?}: {r:?} for {at:?} in {screen:?}");
            if target.is_none() {
                assert!((r.bottom() - at.y).abs() <= 2.0, "{scale}: the empty-space menu is too tall for below and opens above");
            }
            context_menu::close(&mut h.app);
            h.frames(2);
        }
    }
}

/// #3313: renaming a body by typing in the Rename box and pressing Enter (or Tab) renames it.
#[test]
fn typing_a_new_body_name_and_enter_renames_it() {
    for confirm in ["Enter", "Tab"] {
        let mut h = crate::scenario::Harness::new();
        h.app.run("solid.box", json!({"length": 10, "width": 10, "height": 10})).unwrap();
        let b = h.app.session.model.state().bodies[0].name.clone();
        h.app.run("select.set", json!({"items": [{"type": "body", "name": b}]})).unwrap();
        h.frames(2);
        h.step(&json!({"key": "F2"})).unwrap();
        h.frames(2);
        assert!(h.app.menu.rename.is_some(), "F2 opens the Rename box");
        h.step(&json!({"text": "Lid"})).unwrap();
        h.frames(1);
        h.step(&json!({"key": confirm})).unwrap();
        h.frames(2);
        let names: Vec<String> = h.app.session.model.state().bodies.iter().map(|b| b.name.clone()).collect();
        assert_eq!(names, ["Lid"], "{confirm} renames the body; status {:?}", h.app.status);
    }
    // Escape still cancels.
    let mut h = crate::scenario::Harness::new();
    h.app.run("solid.box", json!({"length": 10, "width": 10, "height": 10})).unwrap();
    let b = h.app.session.model.state().bodies[0].name.clone();
    h.app.run("select.set", json!({"items": [{"type": "body", "name": b.clone()}]})).unwrap();
    h.frames(2);
    h.step(&json!({"key": "F2"})).unwrap();
    h.frames(2);
    h.step(&json!({"text": "Lid"})).unwrap();
    h.frames(1);
    h.step(&json!({"key": "Escape"})).unwrap();
    h.frames(2);
    assert!(h.app.menu.rename.is_none());
    assert_eq!(h.app.session.model.state().bodies[0].name, b, "Escape keeps the old name");
}

#[test]
fn contribution_picked_faces_open_their_own_feature_after_later_modifiers() {
    use solidcraft_engine::{Sel, geom::Vec3};
    for expected in [1, 2, 3] {
        let mut app = SolveApp::new(Session::default(), Services::default());
        app.run("solid.box", json!({"length":40,"width":30,"height":20,"body_name":"Plate"})).unwrap();
        app.run("solid.hole", json!({"position":[20,15,20],"diameter":8})).unwrap();
        app.run("solid.fillet", json!({"edges":[[0,0,10]],"radius":2})).unwrap();
        app.run("solid.hole", json!({"position":[32,20,20],"diameter":4})).unwrap();
        let state = app.session.world_state();
        let b = state.body("Plate").unwrap();
        let names = solidcraft_engine::doc::naming::face_names(b);
        let index = names.iter().position(|n| n.starts_with(&format!("F{expected}:"))).unwrap();
        let mesh = b.mesh();
        let point = mesh
            .triangles
            .iter()
            .enumerate()
            .find_map(|(i, t)| (mesh.tri_face.get(i).copied() == Some(index as u32)).then(|| mesh.tri(t)).flatten())
            .map(|t| (t[0] + t[1] + t[2]) / 3.0)
            .unwrap_or(Vec3::ZERO);
        let pick = Sel::Face { body: "Plate".into(), index, point };
        app.session.selection = vec![pick.clone()];
        let items = context_menu::items(&app, &Target::Viewport);
        for label in ["Edit Feature", "Find in Timeline"] {
            let item = find(&items, label);
            assert!(item.enabled, "{label}, {expected}: {names:?}");
            assert_eq!(item.params.as_ref().unwrap()["feature"], json!(expected));
        }
        act(&mut app, &Target::Viewport, "Find in Timeline");
        assert_eq!(app.session.selection, vec![Sel::Feature { id: expected }]);
        // The browser lists bodies: a picked face finds its body there, whichever feature made the face.
        app.session.selection = vec![pick.clone()];
        act(&mut app, &Target::Viewport, "Find in Browser");
        assert_eq!(app.session.selection, vec![Sel::Body { name: "Plate".into() }]);
        assert_eq!(app.tree.reveal.as_deref(), Some("Plate"));
        app.session.selection = vec![pick];
        act(&mut app, &Target::Viewport, "Edit Feature");
        assert_eq!(app.dialog.as_ref().unwrap().editing.unwrap().0, expected);
    }
}

#[test]
fn contribution_unresolvable_face_does_not_edit_the_body_creator() {
    let mut app = SolveApp::new(Session::default(), Services::default());
    app.run("solid.box", json!({"length":10,"width":10,"height":10,"body_name":"Plate"})).unwrap();
    app.session.selection =
        vec![solidcraft_engine::Sel::Face { body: "Plate".into(), index: usize::MAX, point: solidcraft_engine::geom::Vec3::ZERO }];
    let items = context_menu::items(&app, &Target::Viewport);
    assert!(!find(&items, "Edit Feature").enabled);
    assert!(!find(&items, "Find in Timeline").enabled);
    assert!(find(&items, "Find in Browser").enabled, "the body is still known");
}

#[test]
fn every_context_target_has_identified_icon_rows_including_submenus() {
    fn check(rows: &[Item]) {
        for row in rows {
            assert!(!row.id.is_empty(), "missing menu identity: {row:?}");
            let structural = row.id == "-" || row.id.starts_with('#');
            let planned = row.id == "ui.workspace" && row.shortcut == "coming later";
            assert!(structural || planned || !row.icon.is_empty(), "missing menu icon: {row:?}");
            if !row.icon.is_empty() {
                assert!(!crate::icon_svg::resolve(&row.icon).placeholder, "placeholder: {}", row.icon);
            }
            check(&row.children);
        }
    }
    let mut app = sample_app();
    let feature = app.session.doc.features[0].id;
    app.run("construct.plane.offset", json!({"base":"XY","offset":5})).unwrap();
    let plane = app.session.doc.features.last().unwrap().id;
    let part = app.run("component.create", json!({"name":"Menu part"})).unwrap()["component"].as_u64().unwrap();
    let occurrence = app.session.doc.occurrence_of(part).unwrap().id;
    let group_id = app.run("browser.group", json!({"component":0,"folder":"bodies","items":[body(&app)]})).unwrap()["group"].as_u64().unwrap();
    for target in [
        Target::Viewport,
        Target::Body { name: body(&app) },
        Target::Sketch { id: feature },
        Target::Plane { id: plane },
        Target::Feature { id: feature },
        Target::OriginPlane { name: "XY".into() },
        Target::OriginAxis { name: "Z".into() },
        Target::Component { id: part },
        Target::Occurrence { path: vec![occurrence] },
        Target::Group { id: group_id, scope: "c0".into() },
        Target::Origin,
        Target::Units,
        Target::VisualStyle,
        Target::ViewTool,
        Target::ToolbarCommand { id: "solid.extrude".into() },
        Target::Toolbar,
        Target::HistoryControls,
        Target::Workspace,
        Target::NamedViews,
        Target::NamedView { name: "Top".into() },
        Target::Canvas { id: 1 },
        Target::Section,
        Target::Folder { component: 0, folder: "bodies".into(), scope: "c0".into() },
    ] {
        let rows = context_menu::items(&app, &target);
        assert!(!rows.is_empty(), "unexercised target: {target:?}");
        check(&rows);
    }
    let sample = solidcraft_engine::sample::samples().into_iter().find(|s| s.name == "Hinge Assembly").unwrap();
    let mut assembly = SolveApp::new(Session::default(), Services::default());
    assembly.session.run_script(&(sample.script)()).unwrap();
    assert!(!assembly.session.doc.assembly.joints.is_empty());
    for joint in &assembly.session.doc.assembly.joints {
        let rows = context_menu::items(&assembly, &Target::Joint { id: joint.id });
        assert!(!rows.is_empty());
        check(&rows);
    }
    check(&context_menu::radial_items(&app));
}

#[test]
fn appearance_submenu_swatches_apply_to_the_clicked_body_and_keep_library_action() {
    let mut app = sample_app();
    let name = body(&app);
    let rows = context_menu::items(&app, &Target::Body { name: name.clone() });
    let appearance = find(&rows, "Appearance");
    assert_eq!(appearance.children[0].id, "ui.appearance");
    assert_eq!(appearance.children[0].params, appearance.params);
    let swatch = appearance.children.iter().find(|row| row.swatch.is_some()).unwrap();
    context_menu::run_item(&mut app, swatch, pos2(0.0, 0.0));
    assert_eq!(app.session.doc.appearances.bodies[&name].color, swatch.swatch.unwrap());
}

fn appearance_selection_app() -> SolveApp {
    let mut app = SolveApp::new(Session::default(), Services::default());
    for (name, x) in [("One", 0), ("Two", 20), ("Other", 40)] {
        app.run("solid.box", json!({"body_name":name,"length":10,"width":10,"height":10,"corner":[x,0,0],"operation":"new"})).unwrap();
    }
    app.run("appearance.assign", json!({"bodies":["One","Two","Other"],"color":"#245cb4"})).unwrap();
    app.run("select.set", json!({"items":[{"type":"body","name":"One"},{"type":"body","name":"Two"}]})).unwrap();
    app
}

/// #7935: a selected browser row's swatches and Remove override act on the selected bodies;
/// a row outside the selection still acts only on that row. Each action is one undo step.
#[test]
fn appearance_browser_selection_assigns_and_removes_overrides_together() {
    for (clicked, expected) in [("One", vec!["One", "Two"]), ("Two", vec!["One", "Two"]), ("Other", vec!["Other"])] {
        let mut app = appearance_selection_app();
        let target = Target::Body { name: clicked.into() };
        let appearance = find(&context_menu::items(&app, &target), "Appearance");
        let swatch = find(&appearance.children, "Paint - Red");
        let original = app.session.doc.clone();
        let undo = app.session.undo.len();
        context_menu::run_item(&mut app, &swatch, pos2(0.0, 0.0));
        assert_eq!(app.session.undo.len(), undo + 1);
        for name in ["One", "Two", "Other"] {
            let color = if expected.contains(&name) { [196, 40, 36] } else { [36, 92, 180] };
            assert_eq!(app.session.doc.appearances.bodies[name].color, color, "clicked {clicked}, body {name}");
        }
        let assigned = app.session.doc.clone();
        app.run("edit.undo", json!({})).unwrap();
        assert_eq!(app.session.doc, original);
        app.run("edit.redo", json!({})).unwrap();
        assert_eq!(app.session.doc, assigned);

        let remove = find(&appearance.children, "Remove override");
        context_menu::run_item(&mut app, &remove, pos2(0.0, 0.0));
        assert_eq!(app.session.undo.len(), undo + 2);
        for name in ["One", "Two", "Other"] {
            assert_eq!(app.session.doc.appearances.bodies.contains_key(name), !expected.contains(&name), "clicked {clicked}, body {name}");
        }
        app.run("edit.undo", json!({})).unwrap();
        assert_eq!(app.session.doc, assigned);
    }
}

#[test]
fn appearance_browser_selection_reaches_the_editor_without_other_bodies() {
    for (clicked, expected) in [("Two", vec!["One", "Two"]), ("Other", vec!["Other"])] {
        let mut app = appearance_selection_app();
        // A face in the selection must not turn its whole body into another appearance target.
        app.session.selection.push(solidcraft_engine::Sel::Face { body: "Other".into(), index: 0, point: Vec3::ZERO });
        let appearance = find(&context_menu::items(&app, &Target::Body { name: clicked.into() }), "Appearance");
        assert_eq!(appearance.children[0].params, appearance.params, "the library entry and parent use the same targets");
        context_menu::run_item(&mut app, &appearance, pos2(0.0, 0.0));
        let mut dialog = app.dialog.clone().expect("the Appearance editor opens");
        let crate::dialogs::Kind::Appearance(state) = &mut dialog.kind else { panic!("Appearance editor expected") };
        state.color = [196, 40, 36];
        let targets: Vec<_> = expected.iter().map(|name| solidcraft_engine::Sel::Body { name: (*name).into() }).collect();
        assert_eq!(dialog.inputs[0].items, targets);
        for (command, params) in crate::dialogs::apply_commands(&app, &dialog).unwrap() {
            app.run(&command, params).unwrap();
        }
        for name in ["One", "Two", "Other"] {
            let color = if expected.contains(&name) { [196, 40, 36] } else { [36, 92, 180] };
            assert_eq!(app.session.doc.appearances.bodies[name].color, color, "clicked {clicked}, body {name}");
        }
    }
}

#[test]
fn appearance_parent_click_opens_editor_after_hovering_quick_choices() {
    use crate::scenario::{Harness, rect_of};
    let mut h = Harness::new();
    h.app = sample_app();
    let name = body(&h.app);
    h.frames(3);
    context_menu::open_for(&mut h.app, pos2(400.0, 180.0), Target::Body { name: name.clone() });
    h.frames(2);
    h.step(&json!({"move":[10, 10]})).unwrap();
    h.step(&json!({"move":{"handle":"menu:ui.appearance"}})).unwrap();
    h.frames(2);
    assert!(rect_of("menu:appearance.assign:Aluminum - Satin").is_some(), "hover opens quick choices");
    assert!(h.app.dialog.is_none(), "hover must not open the editor");
    h.step(&json!({"click":{"handle":"menu:ui.appearance"}})).unwrap();
    assert!(h.app.viewport.context_menu.is_none(), "activation closes the context menu");
    let dialog = h.app.dialog.as_ref().expect("parent click opens the full editor");
    assert!(matches!(dialog.kind, crate::dialogs::Kind::Appearance(_)));
    assert_eq!(dialog.inputs[0].items, vec![solidcraft_engine::Sel::Body { name }]);
}

#[test]
fn appearance_hovered_quick_choice_applies_without_opening_editor() {
    use crate::scenario::Harness;
    let mut h = Harness::new();
    h.app = sample_app();
    let name = body(&h.app);
    let rows = context_menu::items(&h.app, &Target::Body { name: name.clone() });
    let appearance = find(&rows, "Appearance");
    let swatch = appearance.children.iter().find(|row| row.swatch.is_some()).unwrap();
    h.frames(3);
    context_menu::open_for(&mut h.app, pos2(400.0, 180.0), Target::Body { name: name.clone() });
    h.frames(2);
    h.step(&json!({"move":[10, 10]})).unwrap();
    h.step(&json!({"move":{"handle":"menu:ui.appearance"}})).unwrap();
    h.frames(2);
    h.step(&json!({"click":{"handle":format!("menu:{}:{}", swatch.id, swatch.label)}})).unwrap();
    assert_eq!(h.app.session.doc.appearances.bodies[&name].color, swatch.swatch.unwrap());
    assert!(h.app.dialog.is_none(), "quick choice applies directly");
    assert!(h.app.viewport.context_menu.is_none());
}

#[test]
fn appearance_submenu_keyboard_opens_beside_parent_and_left_returns_to_it() {
    use crate::scenario::{Harness, rect_of};
    let mut h = Harness::new();
    h.app = sample_app();
    h.frames(3);
    let target = Target::Body { name: body(&h.app) };
    let rows = context_menu::items(&h.app, &target);
    context_menu::open_for(&mut h.app, pos2(400.0, 180.0), target);
    let index = rows.iter().position(|i| i.id == "ui.appearance").unwrap();
    h.app.menu.key_row = Some(index);
    h.frames(2);
    h.step(&json!({"key":"ArrowRight"})).unwrap();
    let parent = rect_of("menu:ui.appearance").unwrap();
    let child = rect_of("menu:appearance.assign:Aluminum - Satin").unwrap();
    assert!(child.left() >= parent.right() || child.right() <= parent.left(), "submenu overlaps parent: {parent:?}, {child:?}");
    assert!(h.app.dialog.is_none(), "opening submenu must not execute its first child");
    h.step(&json!({"key":"ArrowLeft"})).unwrap();
    h.frames(2);
    assert!(rect_of("menu:appearance.assign:Aluminum - Satin").is_none(), "Left closes the submenu");
    h.step(&json!({"key":"Escape"})).unwrap();
    assert!(h.app.viewport.context_menu.is_none());
}

/// The Appearance flyout opened by hovering its row closes once the pointer moves to another
/// row of the same menu (#6657).
#[test]
fn appearance_submenu_closes_when_the_pointer_moves_to_another_row() {
    use crate::scenario::{Harness, rect_of};
    let mut h = Harness::new();
    h.app = sample_app();
    h.frames(3);
    let target = Target::Body { name: body(&h.app) };
    let rows = context_menu::items(&h.app, &target);
    context_menu::open_for(&mut h.app, pos2(400.0, 180.0), target);
    h.frames(2);
    let flyout = "menu:appearance.assign:Aluminum - Satin";
    h.step(&json!({"move":[10, 10]})).unwrap();
    h.step(&json!({"move":{"handle":"menu:ui.appearance"}})).unwrap();
    h.frames(2);
    assert!(rect_of(flyout).is_some(), "hovering Appearance opens its flyout");
    let other = rows.iter().find(|i| i.enabled && i.children.is_empty()).unwrap();
    h.step(&json!({"move":{"handle":format!("menu:{}", other.id)}})).unwrap();
    h.frames(2);
    assert!(rect_of(flyout).is_some(), "passing over {} on the way keeps the flyout for a moment", other.label);
    h.frames(30);
    assert!(rect_of(flyout).is_none(), "resting on {} closes the Appearance flyout", other.label);
    assert!(h.app.viewport.context_menu.is_some(), "the menu itself stays open");
    h.step(&json!({"move":{"handle":"menu:ui.appearance"}})).unwrap();
    h.frames(2);
    assert!(rect_of(flyout).is_some(), "hovering Appearance again reopens it");
    h.step(&json!({"key":"Escape"})).unwrap();
    assert!(h.app.viewport.context_menu.is_none());
    assert!(h.app.dialog.is_none(), "closing the flyout runs nothing");
}

#[test]
fn only_one_row_is_lit_while_a_flyout_is_closing() {
    use crate::scenario::{Harness, rect_of};
    fn lit_rows(h: &Harness, rows: &[Item]) -> Vec<String> {
        fn filled(shape: &egui::Shape, r: egui::Rect, found: &mut bool) {
            match shape {
                egui::Shape::Rect(rect) => *found |= rect.rect == r && rect.fill == crate::theme::Tokens::get().accent_soft,
                egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| filled(s, r, found)),
                _ => {}
            }
        }
        rows.iter()
            .filter(|i| {
                rect_of(&format!("menu:{}", i.id)).is_some_and(|r| {
                    let mut found = false;
                    h.shapes.iter().for_each(|s| filled(&s.shape, r, &mut found));
                    found
                })
            })
            .map(|i| i.label.clone())
            .collect()
    }
    let mut h = Harness::new();
    h.app = sample_app();
    h.frames(3);
    let target = Target::Body { name: body(&h.app) };
    let rows = context_menu::items(&h.app, &target);
    context_menu::open_for(&mut h.app, pos2(400.0, 180.0), target);
    h.frames(2);
    h.step(&json!({"move":[10, 10]})).unwrap();
    h.step(&json!({"move":{"handle":"menu:ui.appearance"}})).unwrap();
    h.frames(2);
    assert_eq!(lit_rows(&h, &rows), vec!["Appearance".to_string()], "the hovered parent of the open flyout is lit");
    let other = rows.iter().find(|i| i.enabled && i.children.is_empty()).unwrap();
    h.step(&json!({"move":{"handle":format!("menu:{}", other.id)}})).unwrap();
    h.frames(2);
    assert!(rect_of("menu:appearance.assign:Aluminum - Satin").is_some(), "the flyout is still closing");
    assert_eq!(lit_rows(&h, &rows), vec![other.label.clone()], "the parent gives up its highlight as soon as the pointer leaves it");
}

#[test]
fn narrow_overflow_submenu_keeps_opening_click_and_accepts_the_next_click() {
    use crate::scenario::Harness;
    let mut h = Harness::new();
    h.call("ui.resize", json!({"width":200,"height":800}));
    h.step(&json!({"click":{"handle":"toolbar:overflow"}})).unwrap();
    h.step(&json!({"widget":"CREATE"})).unwrap();
    assert!(h.app.dialog.is_none(), "opening a submenu must not run Extrude beneath its opener");
    assert!(crate::scenario::handle_at("toolbar:solid.extrude").is_some());
    h.step(&json!({"click":{"handle":"toolbar:solid.extrude"}})).unwrap();
    assert!(matches!(h.app.dialog.as_ref().map(|d| &d.kind), Some(crate::dialogs::Kind::Extrude { .. })));
}

/// A right-click on a browser row opens its menu, also when the press and the release reach the
/// same frame (a trackpad tap, or a click during a slow frame).
mod browser_rows {
    use egui::{Event, Modifiers, PointerButton, Pos2, vec2};

    use crate::context_menu::Target;
    use crate::scenario::Harness;

    fn harness() -> Harness {
        let mut h = Harness::new();
        h.app.home.open = false;
        h.app.ui.show_browser = true;
        h.app.session.run_script(&solidcraft_engine::sample::script()).expect("sample");
        h.frames(3);
        h
    }

    /// What a menu is about, without the browser's scope of a folder.
    fn kind(t: &Target) -> String {
        match t {
            Target::Folder { folder, .. } => format!("folder:{folder}"),
            Target::Body { name } => format!("body:{name}"),
            other => format!("{other:?}"),
        }
    }

    fn right(at: Pos2, pressed: bool) -> Event {
        Event::PointerButton { pos: at, button: PointerButton::Secondary, pressed, modifiers: Modifiers::NONE }
    }

    fn right_click(h: &mut Harness, at: Pos2, one_frame: bool) {
        h.frame_with(vec![Event::PointerMoved(at)]);
        if one_frame {
            h.frame_with(vec![right(at, true), right(at, false)]);
        } else {
            h.frame_with(vec![right(at, true)]);
            h.frame_with(vec![right(at, false)]);
        }
        h.frames(4);
    }

    #[test]
    fn a_right_click_on_a_row_opens_its_menu_however_its_events_arrive() {
        let mut h = harness();
        let rows = [("row:Origin", "Origin"), ("row:Bodies", "folder:bodies"), ("row:Plate", "body:Plate"), ("row:Sketches", "folder:sketches")];
        for (row, target) in rows {
            let Some(centre) = crate::scenario::handle_at(row) else { panic!("{row} is not drawn: {:?}", crate::scenario::handles("row:")) };
            // Pointers sit between whole points (trackpads, HiDPI screens).
            for offset in [vec2(0.0, 0.0), vec2(0.25, 0.75), vec2(0.5, 0.5), vec2(0.75, 0.25)] {
                for one_frame in [false, true] {
                    crate::context_menu::close(&mut h.app);
                    h.frames(2);
                    right_click(&mut h, centre + offset, one_frame);
                    let how = if one_frame { "in one frame" } else { "in two frames" };
                    assert!(h.app.viewport.context_menu.is_some(), "{row} at {offset:?}, press and release {how}: the menu did not stay open");
                    assert_eq!(kind(&h.app.menu.target), target, "{row}");
                }
            }
        }
    }

    #[test]
    fn a_click_elsewhere_moves_or_closes_an_open_menu() {
        let mut h = harness();
        let half = vec2(0.5, 0.5);
        let plate = crate::scenario::handle_at("row:Plate").expect("Plate row");
        let origin = crate::scenario::handle_at("row:Origin").expect("Origin row");
        right_click(&mut h, plate + half, true);
        assert!(h.app.viewport.context_menu.is_some());
        assert_eq!(h.app.menu.target, Target::Body { name: "Plate".into() });
        right_click(&mut h, origin + half, true);
        assert!(h.app.viewport.context_menu.is_some(), "a right-click on another row opens that row's menu");
        assert_eq!(h.app.menu.target, Target::Origin);

        let empty = h.app.viewport.rect.expect("viewport").right_bottom() - vec2(80.5, 80.5);
        right_click(&mut h, empty, true);
        assert!(h.app.viewport.context_menu.is_some(), "the viewport's menu opens the same way");
        assert_eq!(h.app.menu.target, Target::Viewport);

        let primary = |pressed| Event::PointerButton { pos: origin, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
        h.frame_with(vec![Event::PointerMoved(origin)]);
        h.frame_with(vec![primary(true), primary(false)]);
        h.frames(2);
        assert!(h.app.viewport.context_menu.is_none(), "a click outside the menu closes it");
    }
}

/// #8570: Insert Design lists the designs open in other tabs. One click inserts a saved one; one
/// with unsaved changes inserts its saved file and says so; one never saved is greyed out, as there
/// is no file to link. Browse… opens the file picker. Alone, or with the preference off, the
/// button goes straight to the picker, as before.
#[test]
fn insert_design_offers_the_other_open_designs_then_browse() {
    let mut h = crate::scenario::Harness::new();
    let open = |dir: &std::path::Path, file: &str, size: f64| {
        let path = dir.join(file);
        let mut s = Session::default();
        s.execute("solid.box", &json!({"length": size, "width": size, "height": size})).unwrap();
        s.execute("file.save", &json!({"path": path})).unwrap();
        let mut opened = Session::default();
        opened.execute("doc.open", &json!({"path": path})).unwrap();
        opened
    };
    let linked = |app: &SolveApp, file: &str| app.session.doc.links.values().filter(|l| l.path.ends_with(file)).count();
    let menu_open = |app: &SolveApp| app.viewport.context_menu.is_some() && app.menu.target == Target::InsertDesign;

    // Alone: the picker, as before.
    h.step(&json!({"choose_file": "none.solidcraft"})).unwrap();
    h.app.start("component.insert_design");
    assert!(!menu_open(&h.app));
    h.frames(3);
    context_menu::close(&mut h.app);

    let dir = h.dir.clone();
    crate::documents::open_tab(&mut h.app, open(&dir, "bracket.solidcraft", 10.0)).unwrap();
    crate::documents::open_tab(&mut h.app, open(&dir, "plate.solidcraft", 20.0)).unwrap();
    h.app.run("solid.box", json!({"length": 5, "width": 5, "height": 5, "corner": [40, 0, 0]})).unwrap();
    crate::documents::open_tab(&mut h.app, Session::default()).unwrap();
    h.app.run("solid.box", json!({"length": 5, "width": 5, "height": 5})).unwrap();
    crate::documents::open_tab(&mut h.app, Session::default()).unwrap();
    crate::documents::switch(&mut h.app, 0);
    h.app.run("solid.box", json!({"length": 1, "width": 1, "height": 1, "body_name": "Base"})).unwrap();

    h.app.start("component.insert_design");
    assert!(menu_open(&h.app));
    let items = context_menu::items(&h.app, &Target::InsertDesign);
    let rows: Vec<(&str, &str, bool)> = items.iter().map(|i| (i.label.as_str(), i.shortcut.as_str(), i.enabled)).collect();
    let names: Vec<String> = h.app.docs.background().map(|s| s.doc.name.clone()).collect();
    assert_eq!(
        rows,
        [
            ("Open designs", "", false),
            (names[0].as_str(), "bracket.solidcraft", true),
            (names[1].as_str(), "Unsaved changes", true),
            (names[2].as_str(), "Not saved yet", false),
            ("", "", false),
            ("Browse…", "", true),
        ],
        "a saved design, one with unsaved changes, one never saved; not the blank new tab"
    );

    act(&mut h.app, &Target::InsertDesign, &names[0]);
    assert_eq!(linked(&h.app, "bracket.solidcraft"), 1);
    assert!(h.app.status.as_ref().is_none_or(|(s, _, _)| !s.contains("unsaved")), "{:?}", h.app.status);
    act(&mut h.app, &Target::InsertDesign, &names[1]);
    assert_eq!(linked(&h.app, "plate.solidcraft"), 1);
    assert!(
        h.app.status.as_ref().is_some_and(|(s, _, error)| !*error && s.contains("unsaved changes") && s.contains("Update Links")),
        "{:?}",
        h.app.status
    );

    // Browse…: the picker, for any file on disk.
    h.step(&json!({"choose_file": "bracket.solidcraft"})).unwrap();
    act(&mut h.app, &Target::InsertDesign, "Browse…");
    h.frames(3);
    assert_eq!(linked(&h.app, "bracket.solidcraft"), 2);

    // The preference off: straight to the picker.
    h.app.preferences.insert_lists_open_designs = false;
    context_menu::close(&mut h.app);
    h.step(&json!({"choose_file": "plate.solidcraft"})).unwrap();
    h.app.start("component.insert_design");
    assert!(!menu_open(&h.app));
    h.frames(3);
    assert_eq!(linked(&h.app, "plate.solidcraft"), 2);
}
