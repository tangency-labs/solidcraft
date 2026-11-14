use crate::context_menu::{self, Target};
use crate::scenario::Harness;
use serde_json::json;
use solidcraft_engine::Sel;
use solidcraft_engine::geom::MeshGeom;

/// A new component lists its own expandable Origin in the Browser: the design's Origin stays
/// closed when the component's opens, and all seven items carry their rows and handles (#7313).
#[test]
fn new_component_has_its_own_origin_folder() {
    let mut h = Harness::new();
    let created = h.app.run("component.create", json!({"name":"Part"})).unwrap();
    let occurrence = created["occurrence"].as_u64().unwrap();
    h.frames(3);
    h.step(&json!({"click":{"handle":format!("fold:occurrence:{occurrence}")}})).unwrap();
    assert_eq!(super::drawn_rows().iter().filter(|row| row.0 == "Origin").count(), 2, "the design and the component each have an Origin");
    h.step(&json!({"click":{"handle":format!("fold:o:{occurrence}/origin")}})).unwrap();
    for item in ["O", "X", "Y", "Z", "XY", "XZ", "YZ"] {
        assert!(crate::scenario::handle_at(&format!("row:o:{occurrence}/origin/{item}")).is_some(), "missing component origin item {item}");
        assert!(crate::scenario::handle_at(&format!("indent:o:{occurrence}/origin/{item}")).is_some(), "missing component origin indent {item}");
        assert!(crate::scenario::rect_of(&format!("row_label:o:{occurrence}/origin/{item}")).is_some(), "missing component origin label {item}");
    }
    assert!(!h.app.tree.expanded.contains("origin"), "opening the component origin must not open the design origin");
}

/// #15658: a part's Origin is off by default, yet hovering its rows in the Browser shows each
/// item where the part is placed, and only while hovered: the folder's eye, the saved visibility
/// and the pick lists stay as they were. With the preference off, an axis or point row sets no
/// hover.
#[test]
fn hovering_a_parts_hidden_origin_rows_shows_them_where_the_part_is() {
    use crate::viewport::HoveredOrigin;
    use solidcore_record::Vec3;
    use solidcraft_engine::geom::prelude::*;
    let mut h = Harness::new();
    let created = h.app.run("component.create", json!({"name":"Part","grounded":false})).unwrap();
    let occurrence = created["occurrence"].as_u64().unwrap();
    h.app.run("occurrence.move", json!({"occurrence":occurrence,"translate":[60,40,0]})).unwrap();
    h.frames(3);
    h.step(&json!({"click":{"handle":format!("fold:occurrence:{occurrence}")}})).unwrap();
    h.step(&json!({"click":{"handle":format!("fold:o:{occurrence}/origin")}})).unwrap();
    let shown = h.app.session.visibility.shown_component_origins.clone();
    assert!(crate::viewport::construction_points(&h.app).is_empty(), "the part's Origin is off");
    let at = Vec3::new(60., 40., 0.);
    for item in ["O", "X", "XY"] {
        let key = solidcraft_engine::origins::key(&[occurrence], item);
        let row = crate::scenario::handle_at(&format!("row:{key}")).expect("the origin row");
        h.call("ui.move", json!({"x": row.x, "y": row.y}));
        assert_eq!(h.app.viewport.hover_plane.as_deref(), Some(key.as_str()), "{item}: its row is hovered");
        let (name, shown_item) = crate::viewport::hovered_hidden_origin(&h.app).expect("drawn for the hover");
        assert_eq!(name, key);
        match (item, shown_item) {
            ("O", HoveredOrigin::Point(p)) => assert!(p.dist(at) < 1e-9, "{p:?}"),
            ("X", HoveredOrigin::Axis { origin, dir, .. }) => assert!(origin.dist(at) < 1e-9 && dir.dot(Vec3::X) > 0.999, "{origin:?} {dir:?}"),
            ("XY", HoveredOrigin::Plane(q)) => assert!(q[0].dist(at) < 1e-9 && q.iter().all(|c| c.z.abs() < 1e-9), "{q:?}"),
            (item, other) => panic!("{item}: {other:?}"),
        }
        assert_eq!(h.app.session.visibility.shown_component_origins, shown, "{item}: the folder stays off");
        assert!(crate::viewport::construction_quads(&h.app).iter().all(|(n, _, _)| !n.contains("/origin/")), "{item}: not pickable");
    }
    // The pointer off the Browser: nothing is drawn for the part's Origin.
    h.call("ui.move", json!({"x": 900.0, "y": 400.0}));
    assert_eq!(crate::viewport::hovered_hidden_origin(&h.app), None);
    // Preference off: an axis row under the pointer sets no hover, as before.
    h.app.preferences.browser_hover_shows_origin = false;
    let key = solidcraft_engine::origins::key(&[occurrence], "X");
    let row = crate::scenario::handle_at(&format!("row:{key}")).expect("the X row");
    h.call("ui.move", json!({"x": row.x, "y": row.y}));
    assert_eq!(h.app.viewport.hover_plane, None);
}

/// A sketch started on a shown component origin plane follows the component: after an
/// occurrence.move the sketch's plane sits where the placement moved, with the axes it was
/// drawn with (#7313).
#[test]
fn component_origin_plane_can_start_a_sketch_after_the_component_moves() {
    use solidcore_record::Vec3;
    use solidcraft_engine::geom::prelude::*;
    let mut h = Harness::new();
    let created = h.app.run("component.create", json!({"name":"Part","grounded":false})).unwrap();
    let occurrence = created["occurrence"].as_u64().unwrap();
    h.app.run("occurrence.move", json!({"occurrence":occurrence,"translate":[20,30,40],"axis":[1,0,0],"angle":"90 deg"})).unwrap();
    h.frames(3);
    h.step(&json!({"click":{"handle":format!("fold:occurrence:{occurrence}")}})).unwrap();
    h.step(&json!({"click":{"handle":format!("fold:o:{occurrence}/origin")}})).unwrap();
    h.step(&json!({"click":{"handle":format!("eye:o:{occurrence}/origin")}})).unwrap();
    assert!(h.app.origin_shown(), "the component eye must not toggle the design origin");
    assert_eq!(crate::viewport::construction_points(&h.app).len(), 1);
    assert_eq!(crate::viewport::construction_axis_segments(&h.app).len(), 3);
    assert_eq!(crate::viewport::construction_quads(&h.app).len(), 3);
    h.app.start("sketch.create");
    h.frames(2);
    h.step(&json!({"click":{"handle":format!("row:o:{occurrence}/origin/XY")}})).unwrap();
    let sketch = h.app.session.active_sketch.expect("the component plane starts the sketch");
    let state = h.app.session.world_state().unwrap();
    let plane = state.sketch(sketch).unwrap().plane;
    assert!(plane.origin.dist(Vec3::new(20., 30., 40.)) < 1e-8);
    assert!(plane.normal().dot(Vec3::Y).abs() > 1. - 1e-8);
}

/// Opening a sketch must not bring a disabled component's origin back: the pick lists follow the
/// browser's eyes (the design Origin's #2581 pick-time reveal never extends to component
/// origins), and the sketch-plane preview refuses a disabled folder's plane while taking a
/// shown one's. Starting the sketch on the hidden plane leaves the folder's eye alone.
#[test]
fn opening_a_sketch_keeps_a_disabled_component_origin_hidden() {
    let mut h = Harness::new();
    let first = h.app.run("component.create", json!({"name":"Leaf A","grounded":false})).unwrap()["occurrence"].as_u64().unwrap();
    // Two sibling leaves: `component.create` places under the active component and activates
    // what it made, so back to the root first. A Leaf B inside Leaf A would make its origin key
    // name a placement no root-first walk finds (`Document::placement_frame`).
    h.app.run("component.activate", json!({"component": 0})).unwrap();
    let second = h.app.run("component.create", json!({"name":"Leaf B","grounded":false})).unwrap()["occurrence"].as_u64().unwrap();
    h.app.run("occurrence.move", json!({"occurrence":second,"translate":[40,0,0]})).unwrap();
    h.frames(2);
    let shown_plane = solidcraft_engine::origins::key(&[first], "XY");
    let hidden_plane = solidcraft_engine::origins::key(&[second], "XY");
    // Leaf A's origin folder shown, Leaf B's left disabled.
    h.app.run("browser.visibility", json!({"tree_folders":[solidcraft_engine::origins::folder(&[first])],"visible":true})).unwrap();
    let shown_prefix = format!("o:{first}/origin/");
    let datum_quads = |app: &crate::SolveApp| -> Vec<String> {
        crate::viewport::construction_quads(app).into_iter().map(|(n, _, _)| n).filter(|n| n.contains("/origin/")).collect()
    };
    let quads = datum_quads(&h.app);
    assert_eq!(quads.len(), 3, "{quads:?}");
    assert!(quads.iter().all(|n| n.starts_with(&shown_prefix)), "{quads:?}");
    // Create Sketch open: the disabled folder's datums stay out of the pick lists.
    h.app.start("sketch.create");
    h.frames(1);
    let quads = datum_quads(&h.app);
    assert_eq!(quads.len(), 3, "{quads:?}");
    assert!(quads.iter().all(|n| n.starts_with(&shown_prefix)), "{quads:?}");
    let hidden_prefix = format!("o:{second}/origin/");
    assert!(
        !crate::viewport::construction_axis_segments(&h.app).iter().any(|(n, _, _)| n.starts_with(&hidden_prefix)),
        "the disabled folder's axes stay out"
    );
    assert!(!crate::viewport::construction_points(&h.app).iter().any(|(n, _)| n.starts_with(&hidden_prefix)));
    // The preview lights the shown plane, and refuses the disabled one (selected, or hovered as
    // its browser row).
    h.app.session.selection = vec![Sel::Plane { name: hidden_plane.clone() }];
    assert!(crate::viewport::previewed_sketch_plane(&h.app).is_none(), "a disabled origin plane does not preview");
    h.app.session.selection = vec![Sel::Plane { name: shown_plane.clone() }];
    assert!(crate::viewport::previewed_sketch_plane(&h.app).is_some(), "the shown origin plane previews");
    h.app.session.selection.clear();
    // Starting the sketch on the hidden plane (its browser row stays a way in) does not turn the
    // folder on: visibility is design state, and the sketch flow does not write it.
    let before = h.app.session.visibility.shown_component_origins.clone();
    h.app.run("sketch.create", json!({"plane": hidden_plane})).unwrap();
    assert_eq!(h.app.session.visibility.shown_component_origins, before, "the sketch flow leaves the folder's eye alone");
}

/// A component's origin datums read like the design Origin's, not like construction geometry:
/// its planes are quarter squares that start at the placement's origin and span one origin size
/// along its own axes, its axes are one origin-axis arm long from the placement's origin, and
/// its point stays pickable while the scene draws it as the design's dot.
#[test]
fn component_origin_datums_are_drawn_like_the_design_origin() {
    use solidcore_record::Vec3;
    use solidcraft_engine::geom::prelude::*;
    let mut h = Harness::new();
    let created = h.app.run("component.create", json!({"name":"Part","grounded":false})).unwrap();
    let occurrence = created["occurrence"].as_u64().unwrap();
    h.app.run("occurrence.move", json!({"occurrence":occurrence,"translate":[20,30,40],"axis":[1,0,0],"angle":"90 deg"})).unwrap();
    h.frames(3);
    h.step(&json!({"click":{"handle":format!("fold:occurrence:{occurrence}")}})).unwrap();
    h.step(&json!({"click":{"handle":format!("fold:o:{occurrence}/origin")}})).unwrap();
    h.step(&json!({"click":{"handle":format!("eye:o:{occurrence}/origin")}})).unwrap();
    let size = crate::selection::origin_size(h.app.cam.half_height());
    let at = Vec3::new(20., 30., 40.);
    for (name, _, q) in crate::viewport::construction_quads(&h.app) {
        assert!(q[0].dist(at) < 1e-8, "{name} starts at the placement's origin, not a square around it");
        assert!((q[1].dist(at) - size).abs() < 1e-8, "{name} spans one origin size along its own axis");
    }
    for (name, a, b) in crate::viewport::construction_axis_segments(&h.app) {
        assert!(a.dist(at) < 1e-8, "{name} starts at the placement's origin");
        assert!((b.dist(a) - size * crate::viewport::AXIS_LEN).abs() < 1e-8, "{name} is one origin-axis arm long");
    }
    assert_eq!(crate::viewport::construction_points(&h.app).len(), 1, "the origin point stays pickable");
}

/// A pick's chip names the placement it came from ("<placement> · <item>"), in the interface's
/// language, while the design's own origin keeps its bare name; a name that is not a datum key
/// labels itself, as before.
#[test]
fn component_origin_picks_label_the_placement() {
    let mut h = Harness::new();
    h.app.run("component.create", json!({"name":"Bracket","grounded":false})).unwrap();
    let names = h.app.preferences.origin_plane_names;
    let (occurrence, placement) = {
        let o = h.app.session.doc.occurrences.iter().find(|o| o.component != 0).expect("the placed component");
        (o.id, o.name.clone())
    };
    let label = |name: &str| crate::dialogs::component_origin_label(&h.app.session, names, name);
    assert_eq!(label(&solidcraft_engine::origins::key(&[occurrence], "XY")).unwrap(), format!("{placement} · {}", names.label("XY")));
    assert_eq!(label(&solidcraft_engine::origins::key(&[occurrence], "O")).unwrap(), format!("{placement} · O"));
    // The design's own origin items are not datums: they keep their own labels.
    assert_eq!(label("XY"), None);
    assert_eq!(label("X"), None);
    // Anything else labels itself (`select_other::label` falls back to the name).
    assert_eq!(label("no/such/key"), None);
    assert_eq!(
        crate::select_other::label(&h.app, &Sel::Axis { name: solidcraft_engine::origins::key(&[occurrence], "Y") }),
        format!("{placement} · {}", names.label("Y"))
    );
}

/// A shown placement's XZ square spans the placement's own +Z, the way the design Origin's XZ
/// square does at the design origin: the sketch plane `XZ` spans -Z (normal +Y), and the draw
/// kept that side, so the plane read flipped next to the design's (#7313).
#[test]
fn component_origin_xz_square_spans_the_placements_own_z() {
    use solidcore_record::Vec3;
    use solidcraft_engine::geom::Vec3Geom;
    let mut h = Harness::new();
    let occurrence = h.app.run("component.create", json!({"name": "Part", "grounded": false})).unwrap()["occurrence"].as_u64().unwrap();
    h.app.run("browser.visibility", json!({"tree_folders": [solidcraft_engine::origins::folder(&[occurrence])], "visible": true})).unwrap();
    h.frames(2);
    let key = solidcraft_engine::origins::key(&[occurrence], "XZ");
    let (_, n, q) = crate::viewport::construction_quads(&h.app).into_iter().find(|(name, _, _)| *name == key).expect("the shown XZ plane draws");
    // The placement sits at the world origin unrotated, so its own +Z is the world's.
    assert!(q.iter().all(|c| c.z > -1e-9), "the square must span +Z like the design's, got {q:?}");
    assert!(q.iter().any(|c| c.z > 1e-6), "the square must reach +Z, got {q:?}");
    assert!(n.dist(Vec3::new(0.0, -1.0, 0.0)) < 1e-9, "the normal reads the design's, got {n:?}");
}

/// A body a feature makes carries its evaluated owner's name in the Browser rows and the
/// component actions: the rename and the move-sketches targets name the component the
/// body belongs to now, not the one the feature was authored in (#7313).
#[test]
fn replaced_body_name_uses_evaluated_owner_in_browser_and_component_actions() {
    let mut h = Harness::new();
    // These rows are read with every placement open, as placements started before #9274.
    h.app.preferences.browser_components_open = true;
    h.app.run("solid.box", json!({"length":10,"width":10,"height":10,"body_name":"Block"})).unwrap();
    let transferred = h.app.run("component.from_bodies", json!({"bodies":["Block"]})).unwrap();
    let owner = transferred["components"][0]["component"].as_u64().unwrap();
    assert_eq!(context_menu::component_bodies(&h.app, owner), ["Block"]);
    h.app.run("solid.remove", json!({"bodies":["Block"]})).unwrap();
    h.app.run("solid.box", json!({"corner":[30,0,0],"length":5,"width":5,"height":5,"body_name":"Block"})).unwrap();
    h.frames(2);
    let state = h.app.session.model.state();
    let body = state.body("Block").unwrap();
    assert_eq!(h.app.session.doc.model_body_component(body), 0);
    assert_eq!(context_menu::component_bodies(&h.app, 0), ["Block"]);
    assert!(context_menu::component_bodies(&h.app, owner).is_empty());
    let cache = h.app.tree.cache.as_ref().unwrap();
    assert_eq!(cache.root.folders[0].as_ref().unwrap().entries().map(|entry| entry.key.as_str()).collect::<Vec<_>>(), ["Block"]);
    let occurrence = h.app.session.doc.occurrence_of(owner).unwrap().id;
    assert!(cache.occurrences.get([occurrence].as_slice()).unwrap().folders[0].as_ref().unwrap().entries().next().is_none());
    let bounds = h.app.session.world_state().unwrap().body("Block").unwrap().mesh().bounds();
    assert!((bounds.min.x - 30.0).abs() < 1e-9);
    assert!((bounds.max.x - 35.0).abs() < 1e-9);
}

fn fixture() -> (Harness, u64, u64, u64) {
    let (mut h, component, first) = crate::test_support::component_box();
    let second = h.app.run("occurrence.copy", json!({"occurrence":first,"translate":[50,0,0]})).unwrap()["occurrence"].as_u64().unwrap();
    // These rows are read with every placement open, as placements started before #9274.
    h.app.preferences.browser_components_open = true;
    h.frames(3);
    (h, component, first, second)
}

#[test]
fn viewport_restores_isolated_occurrences_and_undo_restores_isolation() {
    for label in ["Show All", "Unisolate"] {
        let (mut h, _, first, second) = fixture();
        h.app.session.selection.clear();
        let menu_item =
            |h: &Harness, label: &str| context_menu::items(&h.app, &Target::Viewport).into_iter().find(|item| item.label == label).unwrap();
        assert!(!menu_item(&h, label).enabled, "nothing is hidden yet");
        let isolate =
            context_menu::items(&h.app, &Target::Occurrence { path: vec![first] }).into_iter().find(|item| item.label == "Isolate").unwrap();
        context_menu::run_item(&mut h.app, &isolate, egui::Pos2::ZERO);
        assert!(h.app.session.visibility.hidden_bodies.is_empty());
        assert_eq!(h.app.session.visibility.hidden_occurrences, vec![vec![second]]);
        let other_body = h.app.session.occurrence_bodies(&[second], false).unwrap()[0].clone();
        assert!(!h.app.session.body_visible(&other_body));

        let restore = menu_item(&h, label);
        assert!(restore.enabled, "{label} must restore a hidden component instance");
        let undo = h.app.session.undo.len();
        context_menu::run_item(&mut h.app, &restore, egui::Pos2::ZERO);
        assert!(h.app.session.visibility.hidden_occurrences.is_empty());
        assert!(h.app.session.body_visible(&other_body));
        assert_eq!(h.app.session.undo.len(), undo + 1, "restoring visibility is one undo step");
        assert!(!menu_item(&h, label).enabled, "all component instances are visible again");

        h.app.run("edit.undo", json!({})).unwrap();
        assert_eq!(h.app.session.visibility.hidden_occurrences, vec![vec![second]]);
        assert!(!h.app.session.body_visible(&other_body));
        assert!(menu_item(&h, label).enabled);
    }
}

#[test]
fn folder_visibility_shortcut_targets_only_the_clicked_placement() {
    let (mut h, component, first, second) = fixture();
    let group = h.app.run("browser.group", json!({"component":component,"folder":"bodies","items":["Body1"],"name":"Group"})).unwrap()["group"]
        .as_u64()
        .unwrap();
    h.app.preferences.browser_bodies_folders = crate::prefs::BodiesFolders::Always;
    h.app.tree.collapsed.insert(super::occurrence_key(&[first]));
    h.settle();
    let first_body = h.app.session.occurrence_bodies(&[first], false).unwrap()[0].clone();
    let second_body = h.app.session.occurrence_bodies(&[second], false).unwrap()[0].clone();
    for (label, key) in [("Bodies", format!("o:{second}/bodies")), ("Group (1)", format!("o:{second}/g{group}"))] {
        h.step(&json!({"click":{"handle":format!("row:{label}")}})).unwrap();
        h.step(&json!({"key":"V"})).unwrap();
        assert_eq!(h.app.session.visibility.hidden_folders, vec![key]);
        assert!(h.app.session.body_visible(&first_body));
        assert!(!h.app.session.body_visible(&second_body));
        h.step(&json!({"key":"V"})).unwrap();
        assert!(h.app.session.body_visible(&second_body));
        h.step(&json!({"click":{"handle":format!("row:{label}")}})).unwrap();
    }
}

#[test]
fn browser_reuses_unchanged_rows_and_refreshes_live_state_and_replaced_documents() {
    let (mut h, _, first, second) = fixture();
    let second_body = h.app.session.occurrence_bodies(&[second], false).unwrap().pop().unwrap();
    let first_body = h.app.session.occurrence_bodies(&[first], false).unwrap().pop().unwrap();
    let row = |h: &Harness| {
        let cache = h.app.tree.cache.as_ref().unwrap();
        let entry = cache.occurrences.get([second].as_slice()).unwrap().folders[0].as_ref().unwrap().entries().next().unwrap();
        (entry.row_key.as_ptr(), entry.row_key.len())
    };
    let before = row(&h);
    assert_ne!(first_body, second_body);
    h.frames(3);
    assert_eq!(row(&h), before, "unchanged frames replaced cached row storage");
    h.app.run("select.set", json!({"items":[Sel::Body { name: second_body.clone() }]})).unwrap();
    h.app.session.visibility.hidden_bodies.push(second_body.clone());
    h.frames(2);
    let entry =
        h.app.tree.cache.as_ref().unwrap().occurrences.get([second].as_slice()).unwrap().folders[0].as_ref().unwrap().entries().next().unwrap();
    assert_eq!(entry.row_key.strip_prefix("b:"), Some(second_body.as_str()));
    assert!(entry.is_selected(&h.app));
    assert!(!entry.is_visible(&h.app));
    assert!(h.app.session.body_visible(&first_body));
    assert_eq!(row(&h), before, "selection or visibility rebuilt stable row data");

    let revision = h.app.session.revision;
    h.app.session = solidcraft_engine::Session::new(solidcraft_engine::doc::Document::new("Replacement"));
    h.app.session.revision = revision;
    h.frames(2);
    assert!(super::drawn_rows().iter().any(|row| row.0 == "Replacement"));
    assert!(super::drawn_occurrences().is_empty());
}

fn click(h: &mut Harness, path: &[u64], control: &str) {
    let point = h.call("ui.at", json!({"occurrence":path,"control":control}));
    assert_eq!(point["ok"], true, "{point}");
    let p = &point["result"];
    let result = h.call("ui.click", json!({"x":p[0],"y":p[1]}));
    assert_eq!(result["ok"], true);
}

#[test]
fn save_mesh_browser_pick_uses_the_body_of_the_clicked_placement() {
    let (mut h, _, first, second) = fixture();
    h.app.tree.collapsed.insert(super::occurrence_key(&[first]));
    let expected = h.app.session.occurrence_bodies(&[second], true).unwrap();
    h.app.start("file.save_mesh");
    h.settle();
    h.step(&json!({"click":{"handle":"row:Body1"}})).unwrap();
    let d = h.app.dialog.as_ref().unwrap();
    let crate::dialogs::Kind::SaveMesh(k) = &d.kind else { panic!("Save as Mesh is open") };
    let commands = crate::dialogs_save_mesh::commands(k, &d.inputs).unwrap();
    assert_eq!(commands[0].1["bodies"], json!(expected));
    assert_ne!(expected, h.app.session.occurrence_bodies(&[first], true).unwrap());
}

#[test]
fn occurrence_second_row_selects_exact_path_and_menu_actions() {
    let (mut h, component, first, second) = fixture();
    click(&mut h, &[second], "row");
    assert_eq!(h.app.session.selection, vec![Sel::Occurrence { path: vec![second] }]);
    let bounds = crate::viewport::selection_bounds(&h.app).unwrap();
    assert!((bounds.center().x - 55.0).abs() < 1e-8);
    let target = Target::Occurrence { path: vec![second] };
    let items = context_menu::entries(&h.app, &target);
    // Copy takes this placement (the row right-clicked). Paste New pastes an independent copy of
    // it into the design, never into itself: off on its own row, on on the design's.
    let copy = items.iter().find(|i| i.id == "ui.copy").unwrap();
    assert!(copy.enabled);
    context_menu::run_item(&mut h.app, copy, egui::Pos2::ZERO);
    assert_eq!(h.app.copied_placements, vec![vec![second]]);
    let own = context_menu::entries(&h.app, &target);
    assert!(!own.iter().find(|i| i.id == "ui.pasteNew").unwrap().enabled, "not into itself");
    let design = context_menu::entries(&h.app, &Target::Component { id: 0 });
    let independent = design.iter().find(|i| i.id == "ui.pasteNew").unwrap();
    assert!(independent.enabled);
    assert_eq!(independent.params.as_ref().unwrap()["component"], 0);
    let properties = items.iter().find(|i| i.id == "ui.properties").unwrap();
    assert_eq!(properties.params.as_ref().unwrap()["occurrence"], second);
    context_menu::run_item(&mut h.app, properties, egui::Pos2::ZERO);
    assert!((h.app.menu.props.as_ref().unwrap().1["total"]["center_of_mass"][0].as_f64().unwrap() - 55.0).abs() < 1e-8);
    let movement = items.iter().find(|i| i.id == "ui.moveOccurrence").unwrap();
    context_menu::run_item(&mut h.app, movement, egui::Pos2::ZERO);
    assert_eq!(h.app.tree.occurrence_move.as_ref().unwrap().path, vec![second]);
    let ground = items.iter().find(|i| i.id == "occurrence.ground").unwrap();
    context_menu::run_item(&mut h.app, ground, egui::Pos2::ZERO);
    assert!(h.app.session.doc.occurrences.iter().find(|o| o.id == second).unwrap().grounded);
    assert!(!h.app.session.doc.occurrences.iter().find(|o| o.id == first).unwrap().grounded);
    h.app.tree.occurrence_move = None;
    click(&mut h, &[second], "activate");
    assert_eq!(h.app.session.active_component, component);
    assert_eq!(h.app.session.active_occurrence_path, Some(vec![second]));
}

#[test]
fn turn_up_axis_follows_the_placement_right_clicked() {
    // Whichever placement comes first, a grounded one can't turn and a free one can.
    let (mut h, _, first, second) = fixture();
    let turns = |h: &Harness, occurrence: u64| {
        let items = context_menu::entries(&h.app, &Target::Occurrence { path: vec![occurrence] });
        let turn = items.into_iter().find(|i| i.id == "ui.turnUpAxis").unwrap();
        let children: Vec<_> = turn.children.iter().map(|c| (c.enabled, c.params.as_ref().unwrap()["occurrence"].as_u64())).collect();
        (turn.enabled, children)
    };
    for (grounded, free) in [(first, second), (second, first)] {
        h.app.run("occurrence.ground", json!({"occurrence": grounded, "grounded": true})).unwrap();
        h.app.run("occurrence.ground", json!({"occurrence": free, "grounded": false})).unwrap();
        assert_eq!(turns(&h, free), (true, vec![(true, Some(free)); 2]), "free placement {free}");
        assert_eq!(turns(&h, grounded), (false, vec![(false, Some(grounded)); 2]), "grounded placement {grounded}");
    }
}

#[test]
fn occurrence_eye_and_fold_are_independent_and_survive_rename() {
    let (mut h, component, first, second) = fixture();
    click(&mut h, &[second], "eye");
    assert!(h.app.session.occurrence_hidden(&[second]));
    assert!(h.app.session.occurrence_visible(&[first]));
    let second_body = h.app.session.occurrence_bodies(&[second], true).unwrap()[0].clone();
    assert!(!h.app.session.body_visible(&second_body));
    click(&mut h, &[second], "fold");
    assert!(h.app.tree.collapsed.contains(&format!("o:{second}")));
    assert!(!h.app.tree.collapsed.contains(&format!("o:{first}")));
    h.app.run("component.rename", json!({"component":component,"name":"Renamed Block"})).unwrap();
    h.frames(3);
    assert!(h.app.session.occurrence_hidden(&[second]));
    click(&mut h, &[second], "eye");
    assert!(h.app.session.occurrence_visible(&[second]));
}

#[test]
fn occurrence_move_dialog_keeps_other_pending_moves_and_reuses_initial_baseline() {
    let (mut h, _, first, second) = fixture();
    h.app.run("occurrence.move", json!({"occurrence":first,"translate":[0,10,0],"capture":false})).unwrap();
    h.app.run("occurrence.move", json!({"occurrence":second,"translate":[5,0,0],"capture":false})).unwrap();
    let first_pending = h.app.session.pending_moves[&first];
    let items = context_menu::items(&h.app, &Target::Occurrence { path: vec![second] });
    let movement = items.iter().find(|i| i.id == "ui.moveOccurrence").unwrap();
    context_menu::run_item(&mut h.app, movement, egui::Pos2::ZERO);
    let mut m = h.app.tree.occurrence_move.clone().unwrap();
    m.pivot = Some(solidcraft_engine::doc::apply_point(&h.app.session.occurrence_placement(&[second]).unwrap().world, solidcore_record::Vec3::ZERO));
    m.translate = [10.0, 0.0, 0.0];
    crate::browser::apply_occurrence_move(&mut h.app, &m);
    let at = h.app.session.occurrence_placement(&[second]).unwrap();
    assert!((solidcraft_engine::doc::apply_point(&at.world, solidcore_record::Vec3::ZERO).x - 65.0).abs() < 1e-8);
    m.translate = [3.0, 0.0, 0.0];
    crate::browser::apply_occurrence_move(&mut h.app, &m);
    let at = h.app.session.occurrence_placement(&[second]).unwrap();
    assert!((solidcraft_engine::doc::apply_point(&at.world, solidcore_record::Vec3::ZERO).x - 58.0).abs() < 1e-8);
    assert_eq!(h.app.session.pending_moves[&first], first_pending);
    crate::browser::revert_occurrence_dialog(&mut h.app, &m);
    let restored = h.app.session.occurrence_placement(&[second]).unwrap();
    assert!((solidcraft_engine::doc::apply_point(&restored.world, solidcore_record::Vec3::ZERO).x - 55.0).abs() < 1e-8);
    assert_eq!(h.app.session.pending_moves[&first], first_pending);
    h.app.run("component.revert_position", json!({"occurrences":[second]})).unwrap();
    context_menu::run_item(&mut h.app, movement, egui::Pos2::ZERO);
    let mut fresh = h.app.tree.occurrence_move.clone().unwrap();
    assert!(!fresh.had_pending);
    fresh.translate = [2.0, 0.0, 0.0];
    crate::browser::apply_occurrence_move(&mut h.app, &fresh);
    crate::browser::revert_occurrence_dialog(&mut h.app, &fresh);
    assert!(!h.app.session.pending_moves.contains_key(&second));
    assert_eq!(h.app.session.pending_moves[&first], first_pending);
}

#[test]
fn occurrence_delete_targets_second_copy_even_if_first_was_selected() {
    let (mut h, _, first, second) = fixture();
    click(&mut h, &[first], "row");
    let items = context_menu::items(&h.app, &Target::Occurrence { path: vec![second] });
    let delete = items.iter().find(|i| i.id == "ui.delete").unwrap();
    context_menu::run_item(&mut h.app, delete, egui::Pos2::ZERO);
    crate::delete::confirm(&mut h.app, true);
    assert!(h.app.session.doc.occurrences.iter().any(|o| o.id == first));
    assert!(!h.app.session.doc.occurrences.iter().any(|o| o.id == second));
    assert_eq!(h.app.session.world_state().unwrap().bodies.len(), 1);
}

#[test]
fn occurrence_new_component_keeps_inactive_second_parent_authoring_context() {
    let (mut h, _, first, second) = fixture();
    assert_eq!(h.app.session.active_component, 0);
    let items = context_menu::entries(&h.app, &Target::Occurrence { path: vec![second] });
    let create = items.iter().find(|i| i.id == "component.create").unwrap();
    assert_eq!(create.params.as_ref().unwrap()["parent_path"], json!([second]));
    context_menu::run_item(&mut h.app, create, egui::Pos2::ZERO);
    let active = h.app.session.active_occurrence_path.as_ref().unwrap();
    assert_eq!(active.len(), 2);
    assert_eq!(active[0], second);
    assert_ne!(active[0], first);
    h.app.run("solid.box", json!({"corner":[51,1,1],"length":2,"width":2,"height":2})).unwrap();
    let feature = h.app.session.model.state().bodies.last().unwrap().feature;
    let mut centers: Vec<f64> =
        h.app.session.world_state().unwrap().bodies.iter().filter(|b| b.feature == feature).map(|b| b.mesh().bounds().center().x).collect();
    centers.sort_by(f64::total_cmp);
    assert_eq!(centers.len(), 2);
    assert!((centers[0] - 2.0).abs() < 1e-8 && (centers[1] - 52.0).abs() < 1e-8, "{centers:?}");
}

#[test]
fn occurrence_body_appearance_editor_inherits_definition_precision() {
    let (mut h, _, _, second) = fixture();
    h.app.run("appearance.assign", json!({"bodies":["Body1"],"color":"#123456","appearance":"Tinted source","opacity":0.123456})).unwrap();
    let name = h.app.session.occurrence_bodies(&[second], false).unwrap()[0].clone();
    h.app.session.selection = vec![Sel::Body { name }];
    let (kind, _) = crate::dialogs_appearance::start(&h.app, "appearance.assign").unwrap();
    let crate::dialogs::Kind::Appearance(look) = kind else { panic!("appearance editor") };
    assert_eq!(look.color, [0x12, 0x34, 0x56]);
    assert_eq!(look.opacity, 0.123456);
    assert_eq!(look.look.as_deref(), Some("Tinted source"));
}

#[test]
fn occurrence_nested_shared_child_has_distinct_rows_and_scope() {
    let mut h = Harness::new();
    // These rows are read with every placement open, as placements started before #9274.
    h.app.preferences.browser_components_open = true;
    let parent = h.app.run("component.create", json!({"name":"Assembly"})).unwrap();
    let parent_component = parent["component"].as_u64().unwrap();
    let parent_occurrence = parent["occurrence"].as_u64().unwrap();
    let child = h.app.run("component.create", json!({"name":"Child"})).unwrap()["occurrence"].as_u64().unwrap();
    h.app.run("solid.box", json!({"length":10,"width":10,"height":10})).unwrap();
    h.app.run("component.activate", json!({"component":0})).unwrap();
    let other = h.app.run("occurrence.copy", json!({"component":parent_component,"translate":[80,0,0]})).unwrap()["occurrence"].as_u64().unwrap();
    h.frames(3);
    click(&mut h, &[other, child], "row");
    assert_eq!(h.app.session.selection, vec![Sel::Occurrence { path: vec![other, child] }]);
    assert!((crate::viewport::selection_bounds(&h.app).unwrap().center().x - 85.0).abs() < 1e-8);
    let items = context_menu::items(&h.app, &Target::Occurrence { path: vec![other, child] });
    assert!(items.iter().any(|i| i.label.starts_with("Shared child:")));
    click(&mut h, &[other, child], "fold");
    assert!(h.app.tree.collapsed.contains(&format!("o:{other}/{child}")));
    assert!(!h.app.tree.collapsed.contains(&format!("o:{parent_occurrence}/{child}")));
    click(&mut h, &[other, child], "eye");
    assert!(h.app.session.occurrence_visible(&[parent_occurrence, child]));
    assert!(!h.app.session.occurrence_visible(&[other, child]));
    let delete = items.iter().find(|i| i.id == "ui.delete").unwrap();
    context_menu::run_item(&mut h.app, delete, egui::Pos2::ZERO);
    assert!(h.app.menu.confirm.is_some(), "shared child deletion must communicate scope before removing both renderings");
}

#[test]
fn occurrence_distance_editor_reads_inches_and_keeps_four_digits_in_model_space() {
    let (mut h, _, first, _) = fixture();
    h.app.run("document.units", json!({"units":"in"})).unwrap();
    let target = Target::Occurrence { path: vec![first] };
    let items = context_menu::items(&h.app, &target);
    let movement = items.iter().find(|i| i.id == "ui.moveOccurrence").unwrap();
    context_menu::run_item(&mut h.app, movement, egui::Pos2::ZERO);
    h.settle();
    let original = h.app.session.doc.to_json();
    let point = h.call("ui.at", json!({"handle":"occurrence-move:distance:0"}));
    let at = &point["result"];
    h.call("ui.click", json!({"x":at[0],"y":at[1],"double":true}));
    h.settle();
    h.step(&json!({"key":"A","cmd":true})).unwrap();
    h.step(&json!({"text":"1.2345"})).unwrap();
    h.step(&json!({"key":"Enter"})).unwrap();
    let m = h.app.tree.occurrence_move.as_ref().unwrap();
    assert!((m.translate[0] - 31.3563).abs() < 1e-8, "{:?}", m.translate);
    assert_eq!(h.app.session.doc.to_json(), original, "pending movement does not alter the stored design");
    h.app.session.selection = vec![Sel::Occurrence { path: vec![first] }];
    let bounds = crate::viewport::selection_bounds(&h.app).unwrap();
    assert!((bounds.min.x - 31.3563).abs() < 1e-6);
    let movement = h.app.tree.occurrence_move.clone().unwrap();
    crate::browser::revert_occurrence_dialog(&mut h.app, &movement);
    assert!(h.app.session.pending_moves.is_empty());
}

/// #3270: a body dragged from a child component onto the parent's Bodies folder moves back into
/// the parent, as a drop on the parent's own row does.
#[test]
fn a_body_dropped_on_a_bodies_folder_moves_into_that_component() {
    let mut h = Harness::new();
    // These rows are read with every placement open, as placements started before #9274.
    h.app.preferences.browser_components_open = true;
    h.app.home.open = false;
    h.app.run("solid.box", json!({"length":10,"width":10,"height":10,"body_name":"Plate"})).unwrap();
    h.app.run("solid.box", json!({"corner":[20,0,0],"length":5,"width":5,"height":5,"body_name":"Lid"})).unwrap();
    let part = h.app.run("component.create", json!({"name":"Part"})).unwrap()["component"].as_u64().unwrap();
    h.app.run("solid.box", json!({"corner":[40,0,0],"length":5,"width":5,"height":5,"body_name":"Pin"})).unwrap();
    h.app.run("component.activate", json!({"component":0})).unwrap();
    h.frames(4);
    let owner = |h: &Harness, name: &str| {
        let state = h.app.session.model.state();
        h.app.session.doc.model_body_component(state.body(name).unwrap())
    };
    assert_eq!(owner(&h, "Pin"), part);
    let at = |h: &mut Harness, handle: &str| h.at(&json!({"handle": handle})).unwrap();
    let (plate, lid, pin) = (at(&mut h, "row:Plate"), at(&mut h, "row:Lid"), at(&mut h, "row:Pin"));
    // The root's Bodies folder row sits one row above its first body.
    let folder = [plate[0], plate[1] - (lid[1] - plate[1])];
    h.step(&json!({"drag": [pin, folder], "steps": 12})).unwrap();
    h.frames(3);
    assert_eq!(owner(&h, "Pin"), 0, "Pin moved back into the root component");
}

/// Paste on a component's row pastes inside it: never a component inside itself, so that Paste
/// is off on its own row, and a copy of it pastes into its parent (here the design) from there.
#[test]
fn a_copied_component_pastes_into_its_parent_not_into_itself() {
    let (mut h, component, _, second) = fixture();
    click(&mut h, &[second], "row");
    let own = Target::Occurrence { path: vec![second] };
    let design = Target::Component { id: 0 };
    let find = |h: &Harness, target: &Target, id: &str| context_menu::entries(&h.app, target).into_iter().find(|i| i.id == id).unwrap();
    assert!(!find(&h, &own, "ui.pasteNew").enabled, "nothing copied yet");
    let copy = find(&h, &own, "ui.copy");
    context_menu::run_item(&mut h.app, &copy, egui::Pos2::ZERO);
    assert!(!find(&h, &own, "ui.paste").enabled, "not inside itself");
    assert!(!find(&h, &own, "ui.pasteNew").enabled);
    let paste = find(&h, &design, "ui.paste");
    assert!(paste.enabled);
    let before = h.app.session.doc.occurrences.len();
    context_menu::run_item(&mut h.app, &paste, egui::Pos2::ZERO);
    assert_eq!(h.app.session.doc.occurrences.len(), before + 1);
    let pasted = h.app.session.doc.occurrences.last().unwrap().clone();
    assert_eq!((pasted.component, pasted.parent), (component, 0), "an instance of Block, in the design");
    assert_eq!(h.app.session.selection, vec![Sel::Occurrence { path: vec![pasted.id] }]);
    assert!((pasted.transform[3][0] - 50.0).abs() < 1e-9, "where the copied instance is");
    assert!(!crate::browser::is_open(&h.app, &format!("o:{}", pasted.id), true), "it arrives folded");
}

/// #4217: whether a component's Bodies folder folds away, under Always, Several bodies only and
/// Never. Several bodies only (the default) folds a lone body with no groups, Never every
/// component's folder, Always none. The design's own folder, an empty one and a hidden one never
/// fold, and hiding is per placement.
#[test]
fn bodies_folders_fold_by_the_preference_but_never_the_designs_or_a_hidden_one() {
    use crate::prefs::BodiesFolders;
    let (mut h, component, first, second) = fixture();
    assert_eq!(h.app.preferences.browser_bodies_folders, BodiesFolders::Several, "the default");
    let modes = |h: &mut Harness, comp: u64, scope: &str| {
        let bodies = super::entries(&h.app, comp, "bodies").len();
        BodiesFolders::ALL.map(|mode| {
            h.app.preferences.browser_bodies_folders = mode;
            super::bodies_folded(&h.app, comp, scope, bodies)
        })
    };
    let (one, two) = (super::occurrence_key(&[first]), super::occurrence_key(&[second]));
    assert_eq!(modes(&mut h, component, &one), [false, true, true], "one body");
    assert_eq!(modes(&mut h, component, &two), [false, true, true], "one body, the other placement");
    h.app.run("solid.box", json!({"length":5,"width":5,"height":5,"corner":[0,40,0]})).unwrap();
    assert_eq!(modes(&mut h, 0, "c0"), [false; 3], "the design's own folder");
    let empty = h.app.run("component.create", json!({"name":"Empty"})).unwrap();
    h.app.run("component.activate", json!({"component":0})).unwrap();
    let empty_scope = super::occurrence_key(&[empty["occurrence"].as_u64().unwrap()]);
    assert_eq!(modes(&mut h, empty["component"].as_u64().unwrap(), &empty_scope), [false; 3], "no bodies");
    let show = |h: &mut Harness, visible: bool| {
        h.app.run("browser.visibility", json!({"tree_folders":[format!("{one}/bodies")],"visible":visible})).unwrap()
    };
    show(&mut h, false);
    assert_eq!(modes(&mut h, component, &one), [false; 3], "a hidden folder stays: its eye is the way back");
    assert_eq!(modes(&mut h, component, &two), [false, true, true], "the other placement's folder has its own eye");
    show(&mut h, true);
    let group = h.app.run("browser.group", json!({"component":component,"folder":"bodies","items":["Body1"]})).unwrap()["group"].clone();
    assert_eq!(modes(&mut h, component, &one), [false, false, true], "a group");
    h.app.run("browser.ungroup", json!({"group":group})).unwrap();
    assert_eq!(modes(&mut h, component, &one), [false, true, true], "the group gone");
    h.app.run("component.activate", json!({"component":component})).unwrap();
    h.app.run("solid.box", json!({"length":5,"width":5,"height":5,"corner":[20,0,0]})).unwrap();
    h.app.run("component.activate", json!({"component":0})).unwrap();
    assert_eq!(modes(&mut h, component, &one), [false, false, true], "two bodies");
}

/// #4217: a folded folder's lone body is drawn one level under its placement, two placements
/// deep, with no Bodies row, even when that folder was closed while it was still drawn (nothing
/// could open it again). Always draws the folder, closed as it was left, then open with the body
/// two levels under the placement.
/// #8768: while the Browser's rows start closed (Preferences › Layout), a placement's contents
/// stay hidden until its arrow opens it, even with "Components start open" on (#9274), Find in
/// Browser opens the placements holding its body, and Collapse All and Expand All close and open
/// them.
#[test]
fn placements_start_closed_with_the_preference_and_find_in_browser_opens_the_path_to_its_body() {
    let mut h = Harness::new();
    assert_eq!(h.call("ui.prefs", json!({"browser_components_open": true, "browser_rows_start_closed": true}))["ok"], true);
    let outer = h.app.run("component.create", json!({"name":"Frame"})).unwrap()["occurrence"].as_u64().unwrap();
    let inner = h.app.run("component.create", json!({"name":"Part"})).unwrap()["occurrence"].as_u64().unwrap();
    h.app.run("solid.box", json!({"length":10,"width":10,"height":10,"body_name":"Deep"})).unwrap();
    h.app.run("component.activate", json!({"component":0})).unwrap();
    h.frames(3);
    let row = |path: &str| crate::scenario::handle_at(&format!("row:occurrence:{path}"));
    assert!(row(&outer.to_string()).is_some(), "the top-level placement shows");
    assert!(row(&format!("{outer}/{inner}")).is_none(), "a placement's contents stay hidden until it is opened");
    click(&mut h, &[outer], "fold");
    assert!(row(&format!("{outer}/{inner}")).is_some(), "opening a placement shows its children");
    click(&mut h, &[outer], "fold");
    assert!(row(&format!("{outer}/{inner}")).is_none(), "closing it hides them again");
    let deep = h.app.session.occurrence_bodies(&[outer, inner], false).unwrap()[0].clone();
    crate::browser::reveal_body(&mut h.app, &deep);
    h.frames(3);
    assert!(h.app.tree.expanded.contains(&format!("o:{outer}")) && h.app.tree.expanded.contains(&format!("o:{outer}/{inner}")));
    assert!(row(&format!("{outer}/{inner}")).is_some(), "Find in Browser opened the placements holding the body");
    h.app.run("browser.collapse_all", json!({})).unwrap();
    h.frames(3);
    assert!(row(&format!("{outer}/{inner}")).is_none(), "Collapse All closes the placements");
    h.app.run("browser.expand_all", json!({})).unwrap();
    h.frames(3);
    assert!(row(&format!("{outer}/{inner}")).is_some(), "Expand All opens them");
}

#[test]
fn a_folded_body_sits_one_level_under_its_nested_placement() {
    use crate::prefs::BodiesFolders;
    let mut h = Harness::new();
    // These rows are read with every placement open, as placements started before #9274.
    h.app.preferences.browser_components_open = true;
    let parent = h.app.run("component.create", json!({"name":"Assembly"})).unwrap()["occurrence"].as_u64().unwrap();
    let child = h.app.run("component.create", json!({"name":"Child"})).unwrap()["occurrence"].as_u64().unwrap();
    h.app.run("solid.box", json!({"length":10,"width":10,"height":10,"body_name":"Pin"})).unwrap();
    h.app.run("component.activate", json!({"component":0})).unwrap();
    h.app.tree.collapsed.insert(format!("{}/bodies", super::occurrence_key(&[parent, child])));
    // How many levels the body's row sits under its placement's.
    let levels = || -> Option<i32> {
        let placement = crate::scenario::handle_at("indent:Child:1")?;
        let body = crate::scenario::handle_at("indent:Pin")?;
        Some(((body.x - placement.x) / super::INDENT).round() as i32)
    };
    for (mode, want) in [(BodiesFolders::Several, Some(1)), (BodiesFolders::Never, Some(1)), (BodiesFolders::Always, None)] {
        h.app.preferences.browser_bodies_folders = mode;
        h.frames(2);
        assert_eq!(levels(), want, "{mode:?}");
        assert_eq!(crate::scenario::handle_at("fold:Bodies").is_some(), mode == BodiesFolders::Always, "{mode:?}: the Bodies row");
    }
    h.app.tree.collapsed.clear();
    h.frames(2);
    assert_eq!(levels(), Some(2), "Always, open: under the Bodies folder");
}

/// #4217: Find in Browser reaches a body whose folder is folded away: its closed placement opens
/// and the body's row is drawn and scrolled to.
#[test]
fn find_in_browser_reaches_a_folded_body() {
    let (mut h, _, first, second) = fixture();
    let body = h.app.session.occurrence_bodies(&[second], true).unwrap()[0].clone();
    let scope = super::occurrence_key(&[second]);
    h.app.tree.collapsed.extend([super::occurrence_key(&[first]), scope.clone(), format!("{scope}/bodies")]);
    h.frames(2);
    assert!(
        !h.app.tree.content.owner(crate::semantic_content::ContentOwner::BROWSER).unwrap().order.contains(&format!("b:{body}")),
        "the body's row is closed away"
    );
    h.app.session.selection = vec![Sel::Body { name: body.clone() }];
    let find = context_menu::items(&h.app, &Target::Viewport).into_iter().find(|i| i.id == "ui.findBrowser").unwrap();
    context_menu::run_item(&mut h.app, &find, egui::Pos2::ZERO);
    assert_eq!(h.app.tree.reveal.as_deref(), Some(body.as_str()));
    h.frames(2);
    assert!(h.app.tree.reveal.is_none(), "revealed");
    assert!(!h.app.tree.collapsed.contains(&scope));
    assert!(
        h.app.tree.content.owner(crate::semantic_content::ContentOwner::BROWSER).unwrap().order.contains(&format!("b:{body}")),
        "the row is drawn"
    );
    assert!(crate::scenario::handle_at("fold:Bodies").is_none(), "with no folder row");
}

/// #4217: while a placement's Bodies folder is folded away, the placement's menu takes the
/// folder's items it lacks: Create Components from Bodies (the folder's bodies) and, with several
/// of them selected, Group Selected. With the folder drawn they stay on the folder's own menu.
#[test]
fn a_folded_folders_menu_items_move_to_the_placements_menu() {
    use crate::prefs::BodiesFolders;
    let (mut h, component, first, _) = fixture();
    let target = Target::Occurrence { path: vec![first] };
    let has = |h: &Harness, id: &str| context_menu::entries(&h.app, &target).iter().any(|i| i.id == id);
    let items = context_menu::items(&h.app, &target);
    assert!(!has(&h, "component.from_bodies"), "a component's only body is not offered as a component of its own");
    assert!(items.last().is_some_and(|i| i.id != "-"), "and leaves no separator behind");
    h.app.run("component.activate", json!({"component":component})).unwrap();
    h.app.run("solid.box", json!({"length":5,"width":5,"height":5,"corner":[20,0,0],"body_name":"Second"})).unwrap();
    h.app.run("component.activate", json!({"component":0})).unwrap();
    assert!(!has(&h, "component.from_bodies"), "Several bodies only: two bodies keep their folder");
    h.app.preferences.browser_bodies_folders = BodiesFolders::Never;
    let items = context_menu::items(&h.app, &target);
    let from = items.iter().find(|i| i.id == "component.from_bodies").unwrap();
    assert_eq!(from.params.as_ref().unwrap()["bodies"], json!(["Body1", "Second"]));
    assert!(has(&h, "ui.newGroup"), "on every component's menu");
    assert!(!has(&h, "ui.showAll"), "Show All Bodies only while something is hidden");
    h.app.run("browser.visibility", json!({"items": ["Second"], "visible": false})).unwrap();
    assert!(has(&h, "ui.showAll"), "a hidden body anywhere brings it back");
    h.app.run("browser.visibility", json!({"items": ["Second"], "visible": true})).unwrap();
    assert!(!has(&h, "ui.groupSelected"), "nothing selected");
    h.app.session.selection = vec![Sel::Body { name: "Body1".into() }, Sel::Body { name: "Second".into() }];
    let group = context_menu::items(&h.app, &target).into_iter().find(|i| i.id == "ui.groupSelected").unwrap();
    context_menu::run_item(&mut h.app, &group, egui::Pos2::ZERO);
    let made = h.app.session.doc.browser_groups.iter().find(|g| g.component == component).unwrap();
    assert_eq!(made.item_keys().collect::<Vec<_>>(), ["Body1", "Second"]);
    h.app.preferences.browser_bodies_folders = BodiesFolders::Always;
    assert!(!has(&h, "component.from_bodies"), "Always: on the folder's own menu");
}

/// #5164: dragging a placement row onto another moves it into that component. The command
/// underneath is `occurrence.reparent`, which keeps the placement's world position. Dragging it
/// back onto the design row moves it out to the design.
#[test]
fn dragging_an_occurrence_row_onto_another_moves_it_into_that_component() {
    let mut h = Harness::new();
    // These rows are read with every placement open, as placements started before #9274.
    h.app.preferences.browser_components_open = true;
    // Component A with a body, at the design root.
    let a = h.app.run("component.create", json!({"name":"A"})).unwrap();
    let (_a_comp, a_occ) = (a["component"].as_u64().unwrap(), a["occurrence"].as_u64().unwrap());
    h.app.run("solid.box", json!({"length":10,"width":10,"height":10})).unwrap();
    // Back to the design root so B is a sibling of A, not a child of it.
    h.app.run("component.activate", json!({"component":0})).unwrap();
    // Component B with a body, at the design root.
    let b = h.app.run("component.create", json!({"name":"B"})).unwrap();
    let (b_comp, b_occ) = (b["component"].as_u64().unwrap(), b["occurrence"].as_u64().unwrap());
    h.app.run("solid.box", json!({"length":10,"width":10,"height":10,"corner":[30,0,0]})).unwrap();
    h.app.run("component.activate", json!({"component":0})).unwrap();
    h.frames(3);

    // Both rows sit at the root. Drag A's row onto B's row.
    let from = crate::scenario::handle_at(&format!("row:occurrence:{a_occ}")).unwrap();
    let to = crate::scenario::handle_at(&format!("row:occurrence:{b_occ}")).unwrap();
    let drag = h.call("ui.drag", json!({"x0":from.x,"y0":from.y,"x1":to.x,"y1":to.y,"steps":12}));
    assert_eq!(drag["ok"], true);
    h.frames(2);

    // A's occurrence is now a child of B's occurrence.
    let o = h.app.session.doc.occurrences.iter().find(|o| o.id == a_occ).unwrap();
    assert_eq!(o.parent, b_comp, "A's occurrence moved into component B");
    assert_eq!(h.app.session.doc.occurrences.iter().find(|o| o.id == b_occ).unwrap().parent, 0, "B's occurrence stays at the design");

    // Drag A's row, now under B, back onto the design row.
    let design = crate::scenario::handle_at("row:Untitled").unwrap();
    let from = crate::scenario::handle_at(&format!("row:occurrence:{b_occ}/{a_occ}")).unwrap();
    let drag = h.call("ui.drag", json!({"x0":from.x,"y0":from.y,"x1":design.x,"y1":design.y,"steps":12}));
    assert_eq!(drag["ok"], true);
    h.frames(2);
    let o = h.app.session.doc.occurrences.iter().find(|o| o.id == a_occ).unwrap();
    assert_eq!(o.parent, 0, "A's occurrence moved back out to the design");
}

/// Dropping a placement row on the top of a sibling's row puts it before that sibling, in the
/// order saved with the design; the rows follow. (The middle of a row moves it into that
/// component, as above.)
#[test]
fn dragging_an_occurrence_row_onto_the_top_of_a_sibling_reorders_them() {
    let mut h = Harness::new();
    let mut placed = Vec::new();
    for name in ["A", "B", "C"] {
        placed.push(h.app.run("component.create", json!({"name":name})).unwrap()["occurrence"].as_u64().unwrap());
        h.app.run("component.activate", json!({"component":0})).unwrap();
    }
    let [a, b, c] = [placed[0], placed[1], placed[2]];
    h.frames(3);
    let row = |id: u64| crate::scenario::handle_at(&format!("row:occurrence:{id}")).unwrap();
    let (from, to) = (row(c), row(a));
    assert_eq!(h.call("ui.drag", json!({"x0":from.x,"y0":from.y,"x1":to.x,"y1":to.y - 10.0,"steps":12}))["ok"], true);
    h.frames(3);
    let saved = &h.app.session.doc.browser_order["0/components"];
    assert_eq!(*saved, [c, a, b].map(|id| solidcraft_engine::doc::BrowserEntry::item(&id.to_string())));
    assert!(row(c).y < row(a).y && row(a).y < row(b).y, "the rows follow the saved order");
    assert!(h.app.session.doc.occurrences.iter().all(|o| o.parent == 0), "none moved into another");
}

/// #10218: the design's Sketches folder offers Move Sketches to the Part They're On for its
/// sketches; the one drawn on the part's face goes to the part, the one on XY stays, and the
/// status bar says so.
#[test]
fn sketches_folder_moves_its_sketches_to_the_part_they_are_on() {
    let (mut h, component, occurrence) = crate::test_support::component_box();
    h.app.session.auto_project = false;
    let world = h.app.session.world_state().unwrap();
    let body = world.bodies.iter().find(|b| h.app.session.doc.model_body_component(b) == component).unwrap().name.clone();
    let face = world.body(&body).unwrap().body.faces(0.01).unwrap().into_iter().max_by(|a, b| a.centroid.z.total_cmp(&b.centroid.z)).unwrap();
    let pick = json!({"face":[face.centroid.x,face.centroid.y,face.centroid.z],"body":body,"index":face.index,"path":[occurrence]});
    let on_face = h.app.run("sketch.create", json!({"name":"OnFace","plane":pick})).unwrap()["sketch"].as_u64().unwrap();
    h.app.finish_sketch();
    let on_xy = h.app.run("sketch.create", json!({"name":"OnXY","plane":"XY"})).unwrap()["sketch"].as_u64().unwrap();
    h.app.finish_sketch();
    let target = Target::Folder { component: 0, folder: "sketches".into(), scope: "c0".into() };
    let items = context_menu::items(&h.app, &target);
    let item = items.iter().find(|i| i.id == "component.move_sketches.to_parts").expect("on the Sketches folder's menu");
    // Ids, as numbers: a string would be a sketch's name first.
    assert_eq!(item.params.as_ref().unwrap()["sketches"], json!([on_face, on_xy]));
    context_menu::run_item(&mut h.app, item, egui::Pos2::ZERO);
    assert_eq!(h.app.session.doc.feature(on_face).unwrap().component, component);
    assert_eq!(h.app.session.doc.feature(on_xy).unwrap().component, 0);
    let (status, _, error) = h.app.status.clone().unwrap();
    assert!(!error && status.contains("Moved 1") && status.contains("1 stayed") && status.contains("not on a part's face"), "{status}");
    assert!(context_menu::items(&h.app, &Target::Sketch { id: on_xy }).iter().any(|i| i.id == "component.move_sketches.to_parts"));
}

/// Dragging a sketch onto a part moves that sketch when another one is named like its id: the
/// drag sends ids as numbers, and a string would be a sketch's name first (#9748).
#[test]
fn dragging_a_sketch_onto_a_part_moves_it_when_another_is_named_like_its_id() {
    let (mut h, component, _) = crate::test_support::component_box();
    let dragged = h.app.run("sketch.create", json!({"name":"Dragged","plane":"XY"})).unwrap()["sketch"].as_u64().unwrap();
    h.app.finish_sketch();
    let other = h.app.run("sketch.create", json!({"name":"Other","plane":"XY"})).unwrap()["sketch"].as_u64().unwrap();
    h.app.finish_sketch();
    h.app.run("timeline.rename", json!({"feature": other, "name": dragged.to_string()})).unwrap();
    super::apply_drop(&mut h.app, super::DropAction::Component { to: component, folder: "sketches", keys: vec![dragged.to_string()] });
    assert_eq!(h.app.session.doc.feature(dragged).unwrap().component, component);
    assert_eq!(h.app.session.doc.feature(other).unwrap().component, 0);
}

/// The one resolution order at the dialog resolver sites: a construction feature by name first,
/// then a component origin item key (`plane_by_name`, `axis_of`; the viewport's `named_plane`
/// reads planes through `plane_by_name`). A feature named like a datum key stays reachable, and
/// without the feature the same key is the datum at the placement.
#[test]
fn resolver_order_is_feature_first_at_every_site() {
    use solidcore_record::Vec3;
    use solidcraft_engine::geom::prelude::*;
    let mut h = Harness::new();
    h.app.run("component.create", json!({"name":"Part","grounded":false})).unwrap();
    let occurrence = h.app.session.doc.occurrences.iter().find(|o| o.component != 0).unwrap().id;
    h.app.run("occurrence.move", json!({"occurrence":occurrence,"translate":[100,0,0]})).unwrap();
    // The features are created in the active component, and `component.create` made Part active:
    // back to the root, so the features sit at the root origin while the same keys as datum items
    // sit at the placement - that separation is what proves the resolver order below.
    h.app.run("component.activate", json!({"component": 0})).unwrap();
    // The datum half of the fixture reads the Browser's eyes (`plane_by_name`, `axis_of` offer a
    // component origin only when its Origin folder is shown), so the folder is shown for it.
    h.app.run("browser.visibility", json!({"tree_folders":[solidcraft_engine::origins::folder(&[occurrence])],"visible":true})).unwrap();
    let axis_key = solidcraft_engine::origins::key(&[occurrence], "X");
    // Through two points, so the fixture pins the order, not the two-planes direction convention.
    h.app.run("construct.axis.two_points", json!({"a":{"point":[0,0,0]},"b":{"point":[5,0,0]},"name":axis_key})).unwrap();
    let plane_key = solidcraft_engine::origins::key(&[occurrence], "XY");
    h.app.run("construct.plane.offset", json!({"base":"XY","offset":"2 mm","name":plane_key})).unwrap();
    // A feature first: the axis where the feature is now (the root origin), the plane the feature
    // makes - not the placement's datum of the same key.
    assert_eq!(crate::dialogs::axis_of(&h.app, &Sel::Axis { name: axis_key }), Some((Vec3::ZERO, Vec3::X)));
    let pl = crate::dialogs::plane_by_name(&h.app.session, &plane_key).unwrap();
    assert!(pl.origin.dist(Vec3::new(0., 0., 2.)) < 1e-8, "{pl:?}");
    // Without the feature, the same key is the component origin's, at the placement.
    let pl = crate::dialogs::plane_by_name(&h.app.session, &solidcraft_engine::origins::key(&[occurrence], "XZ")).unwrap();
    assert!(pl.origin.dist(Vec3::new(100., 0., 0.)) < 1e-8, "{pl:?}");
    assert_eq!(
        crate::dialogs::axis_of(&h.app, &Sel::Axis { name: solidcraft_engine::origins::key(&[occurrence], "Y") }),
        Some((Vec3::new(100., 0., 0.), Vec3::Y))
    );
}

/// Case-only name collisions (#7313 review, the defaults check): a construction feature named
/// like a datum key but spelled in another case still answers first — the lookup is the
/// case-insensitive one `find_feature` does — for the chip, the viewport styling and the
/// sketch-flow resolvers (`plane_by_name`, `axis_of`), with the placement's Origin folder shown
/// and with it hidden. A key no feature's name collides with is still the datum while the
/// folder is shown, and dark once it is hidden, under either spelling.
#[test]
fn case_only_name_collisions_keep_the_feature_first() {
    use solidcore_record::Vec3;
    use solidcraft_engine::geom::prelude::*;
    let mut h = Harness::new();
    h.app.run("component.create", json!({"name":"Part","grounded":false})).unwrap();
    let occurrence = h.app.session.doc.occurrences.iter().find(|o| o.component != 0).unwrap().id;
    h.app.run("occurrence.move", json!({"occurrence":occurrence,"translate":[100,0,0]})).unwrap();
    // The features are created in the active component, and `component.create` made Part active:
    // back to the root, so the features sit at the root origin while the same keys as datum items
    // sit at the placement.
    h.app.run("component.activate", json!({"component": 0})).unwrap();
    let plane_key = solidcraft_engine::origins::key(&[occurrence], "XZ");
    let axis_key = solidcraft_engine::origins::key(&[occurrence], "X");
    let free_key = solidcraft_engine::origins::key(&[occurrence], "YZ");
    // Two features whose names are the keys' letters in another case; none is named like
    // `free_key`.
    h.app.run("construct.plane.offset", json!({"base":"XY","offset":"2 mm","name":plane_key.to_ascii_lowercase()})).unwrap();
    h.app.run("construct.axis.two_points", json!({"a":{"point":[0,0,0]},"b":{"point":[5,0,0]},"name":axis_key.to_ascii_lowercase()})).unwrap();
    // Shown: a colliding key stays the feature's under either spelling. The datum branch may
    // not answer a case-variant spelling of a feature's name; the feature itself is reached by
    // its own stored spelling, as feature names always were.
    h.app.run("browser.visibility", json!({"tree_folders":[solidcraft_engine::origins::folder(&[occurrence])],"visible":true})).unwrap();
    let names = h.app.preferences.origin_plane_names;
    assert_eq!(crate::viewport::component_origin_item(&h.app, &axis_key), None, "{axis_key}: the case-variant feature keeps construction styling");
    assert_eq!(crate::dialogs::component_origin_label(&h.app.session, names, &plane_key), None, "{plane_key}: the chip keeps the feature's name");
    assert!(crate::dialogs::plane_by_name(&h.app.session, &plane_key).is_none(), "{plane_key}: the datum does not shadow the feature");
    assert_eq!(crate::dialogs::axis_of(&h.app, &Sel::Axis { name: axis_key.clone() }), None, "{axis_key}: the datum does not shadow the feature");
    let pl = crate::dialogs::plane_by_name(&h.app.session, &plane_key.to_ascii_lowercase()).unwrap();
    assert!(pl.origin.dist(Vec3::new(0., 0., 2.)) < 1e-8, "{pl:?}");
    assert_eq!(crate::dialogs::axis_of(&h.app, &Sel::Axis { name: axis_key.to_ascii_lowercase() }), Some((Vec3::ZERO, Vec3::X)));
    // A key no feature collides with is the datum at the placement, under either spelling.
    let pl = crate::dialogs::plane_by_name(&h.app.session, &free_key).unwrap();
    assert!(pl.origin.dist(Vec3::new(100., 0., 0.)) < 1e-8, "{pl:?}");
    let pl = crate::dialogs::plane_by_name(&h.app.session, &free_key.to_ascii_lowercase()).unwrap();
    assert!(pl.origin.dist(Vec3::new(100., 0., 0.)) < 1e-8, "{pl:?}");
    // Hidden: the same colliding keys stay the feature's, and the free key's datum goes dark
    // under both spellings — hiding the folder never changes what the features answer to.
    h.app.run("browser.visibility", json!({"tree_folders":[solidcraft_engine::origins::folder(&[occurrence])],"visible":false})).unwrap();
    assert_eq!(crate::viewport::component_origin_item(&h.app, &axis_key), None, "{axis_key}: the feature keeps its styling with the folder hidden");
    assert!(crate::dialogs::plane_by_name(&h.app.session, &plane_key).is_none(), "{plane_key}");
    assert!(crate::dialogs::plane_by_name(&h.app.session, &free_key).is_none(), "{free_key}: the hidden folder's plane is not offered");
    assert!(
        crate::dialogs::plane_by_name(&h.app.session, &free_key.to_ascii_lowercase()).is_none(),
        "{free_key}: neither is a case-variant spelling of it"
    );
    let pl = crate::dialogs::plane_by_name(&h.app.session, &plane_key.to_ascii_lowercase()).unwrap();
    assert!(pl.origin.dist(Vec3::new(0., 0., 2.)) < 1e-8, "{pl:?}");
    assert_eq!(crate::dialogs::axis_of(&h.app, &Sel::Axis { name: axis_key.to_ascii_lowercase() }), Some((Vec3::ZERO, Vec3::X)));
}

/// A construction feature keeps its own label and styling when its name shares a datum key's
/// shape (#7313 review): the chip says the feature's name and the viewport styles it as
/// construction geometry, so a construction point by such a name stays drawn. Without the
/// feature, the same key is the datum: the chip names the placement, and the item reads like
/// the design Origin's.
#[test]
fn construction_features_keep_their_labels_and_styling() {
    use solidcraft_engine::origins;
    let mut h = Harness::new();
    h.app.run("component.create", json!({"name":"Part","grounded":false})).unwrap();
    let occurrence = h.app.session.doc.occurrences.iter().find(|o| o.component != 0).unwrap().id;
    h.app.run("occurrence.move", json!({"occurrence":occurrence,"translate":[100,0,0]})).unwrap();
    h.app.run("component.activate", json!({"component": 0})).unwrap();
    let plane_key = origins::key(&[occurrence], "XY");
    h.app.run("construct.plane.offset", json!({"base":"XY","offset":"2 mm","name":plane_key.clone()})).unwrap();
    h.app
        .run("construct.point.vertex", json!({"point":{"construct_point":origins::key(&[occurrence],"O")},"name":origins::key(&[occurrence],"X")}))
        .unwrap();
    // The feature answers to its own name first, as the resolvers do: the chip keeps the
    // feature's name, and the viewport styles the plane and the point as construction geometry.
    let names = h.app.preferences.origin_plane_names;
    assert_eq!(crate::dialogs::component_origin_label(&h.app.session, names, &plane_key), None, "the feature's chip keeps its name");
    assert_eq!(
        crate::viewport::component_origin_item(&h.app, &origins::key(&[occurrence], "X")),
        None,
        "the feature keeps construction styling and stays drawn"
    );
    // Without a feature of the same key, the key is the datum: the chip names the placement,
    // and the item reads like the design Origin's.
    let xz = origins::key(&[occurrence], "XZ");
    assert!(crate::dialogs::component_origin_label(&h.app.session, names, &xz).is_some(), "{xz}");
    assert_eq!(crate::viewport::component_origin_item(&h.app, &xz), Some("XZ"));
    // The design's own origin keeps its bare name: not a placement's item, feature or not.
    assert_eq!(crate::viewport::component_origin_item(&h.app, "XY"), None);
}
