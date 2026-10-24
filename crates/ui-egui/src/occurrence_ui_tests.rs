use crate::context_menu::{self, Target};
use crate::scenario::Harness;
use serde_json::json;
use solidcraft_engine::Sel;

#[test]
fn replaced_body_name_uses_evaluated_owner_in_browser_and_component_actions() {
    let mut h = Harness::new();
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
    assert_eq!(super::entries(&h.app, 0, "bodies").iter().map(|entry| entry.key.as_str()).collect::<Vec<_>>(), ["Block"]);
    assert!(super::entries(&h.app, owner, "bodies").is_empty());
    let bounds = h.app.session.world_state().body("Block").unwrap().mesh().bounds();
    assert!((bounds.min.x - 30.0).abs() < 1e-9);
    assert!((bounds.max.x - 35.0).abs() < 1e-9);
}

fn fixture() -> (Harness, u64, u64, u64) {
    let (mut h, component, first) = crate::test_support::component_box();
    let second = h.app.run("occurrence.copy", json!({"occurrence":first,"translate":[50,0,0]})).unwrap()["occurrence"].as_u64().unwrap();
    h.frames(3);
    (h, component, first, second)
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
    let items = context_menu::items(&h.app, &target);
    let independent = items.iter().find(|i| i.id == "component.paste_new").unwrap();
    assert_eq!(independent.label, "Paste New (Own Features)");
    assert_eq!(independent.params.as_ref().unwrap()["occurrence"], second);
    assert_eq!(independent.params.as_ref().unwrap()["path"], json!([second]));
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
    m.pivot =
        Some(solidcraft_engine::doc::apply_point(&h.app.session.occurrence_placement(&[second]).unwrap().world, solidcraft_engine::geom::Vec3::ZERO));
    m.translate = [10.0, 0.0, 0.0];
    crate::browser::apply_occurrence_move(&mut h.app, &m);
    let at = h.app.session.occurrence_placement(&[second]).unwrap();
    assert!((solidcraft_engine::doc::apply_point(&at.world, solidcraft_engine::geom::Vec3::ZERO).x - 65.0).abs() < 1e-8);
    m.translate = [3.0, 0.0, 0.0];
    crate::browser::apply_occurrence_move(&mut h.app, &m);
    let at = h.app.session.occurrence_placement(&[second]).unwrap();
    assert!((solidcraft_engine::doc::apply_point(&at.world, solidcraft_engine::geom::Vec3::ZERO).x - 58.0).abs() < 1e-8);
    assert_eq!(h.app.session.pending_moves[&first], first_pending);
    crate::browser::revert_occurrence_dialog(&mut h.app, &m);
    let restored = h.app.session.occurrence_placement(&[second]).unwrap();
    assert!((solidcraft_engine::doc::apply_point(&restored.world, solidcraft_engine::geom::Vec3::ZERO).x - 55.0).abs() < 1e-8);
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
    assert_eq!(h.app.session.world_state().bodies.len(), 1);
}

#[test]
fn occurrence_new_component_keeps_inactive_second_parent_authoring_context() {
    let (mut h, _, first, second) = fixture();
    assert_eq!(h.app.session.active_component, 0);
    let items = context_menu::items(&h.app, &Target::Occurrence { path: vec![second] });
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
        h.app.session.world_state().bodies.iter().filter(|b| b.feature == feature).map(|b| b.mesh().bounds().center().x).collect();
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

/// Where a placement's first body sits (world space, min X).
fn placement_x(h: &Harness, path: &[u64]) -> f64 {
    let name = h.app.session.occurrence_bodies(path, true).unwrap()[0].clone();
    h.app.session.world_state().body(&name).unwrap().mesh().bounds().min.x
}

#[test]
fn move_of_a_picked_component_moves_only_that_placement() {
    let (mut h, _, first, second) = fixture();
    let (before_first, before_second) = (placement_x(&h, &[first]), placement_x(&h, &[second]));
    click(&mut h, &[second], "row");
    h.app.start("solid.move");
    let mut d = h.app.dialog.clone().unwrap();
    assert_eq!(d.inputs[0].items, vec![Sel::Occurrence { path: vec![second] }], "Move takes the picked component");
    if let crate::dialogs::Kind::Move { x, .. } = &mut d.kind {
        *x = "7 mm".into();
    }
    for (command, params) in crate::dialogs_move::commands(&h.app, &d).unwrap() {
        assert_eq!(command, "occurrence.move");
        h.app.run(&command, params).unwrap();
    }
    assert!((placement_x(&h, &[second]) - before_second - 7.0).abs() < 1e-6, "the picked placement moved 7 mm");
    assert!((placement_x(&h, &[first]) - before_first).abs() < 1e-6, "the other placement stayed");
}

#[test]
fn move_of_picked_bodies_still_runs_solid_move() {
    let (mut h, _, _, second) = fixture();
    let body = h.app.session.occurrence_bodies(&[second], true).unwrap()[0].clone();
    h.app.run("select.set", json!({"items":[Sel::Body{name:body.clone()}]})).unwrap();
    h.app.start("solid.move");
    let d = h.app.dialog.clone().unwrap();
    let commands = crate::dialogs_move::commands(&h.app, &d).unwrap();
    assert_eq!(commands.len(), 1);
    assert_eq!(commands[0].0, "solid.move");
    assert_eq!(commands[0].1["bodies"], json!([body]));
}

#[test]
fn a_picked_component_does_not_fill_other_dialogs_bodies() {
    let (mut h, _, _, second) = fixture();
    click(&mut h, &[second], "row");
    for id in ["solid.remove", "solid.combine"] {
        h.app.start(id);
        let d = h.app.dialog.as_ref().unwrap();
        assert!(d.inputs.iter().all(|i| i.items.is_empty()), "{id} takes no bodies from a picked component");
        h.app.dialog = None;
    }
}

fn move_dialog_for(h: &mut Harness, path: &[u64], set: impl FnOnce(&mut crate::dialogs::Kind)) -> crate::dialogs::Dialog {
    click(h, path, "row");
    h.app.start("solid.move");
    let mut d = h.app.dialog.clone().unwrap();
    set(&mut d.kind);
    d
}

#[test]
fn move_turns_a_picked_placement_about_its_own_centre() {
    use solidcraft_engine::{doc::apply_point, geom::Vec3};
    let (mut h, _, first, second) = fixture();
    let origin = |h: &Harness, id: u64| apply_point(&h.app.session.occurrence_placement(&[id]).unwrap().world, Vec3::ZERO);
    let (before_first, before_second) = (origin(&h, first), origin(&h, second));
    let d = move_dialog_for(&mut h, &[second], |k| {
        if let crate::dialogs::Kind::Move { rz, .. } = k {
            *rz = "90 deg".into();
        }
    });
    crate::dialogs::run_dialog(&mut h.app, &d).unwrap();
    // The 10 mm cube at x 50..60 turns about its centre (55, 5, 5): its origin corner goes to (60, 0, 0).
    assert!((origin(&h, second) - Vec3::new(60.0, 0.0, 0.0)).len() < 1e-6, "{before_second:?} -> {:?}", origin(&h, second));
    let world = h.app.session.occurrence_placement(&[second]).unwrap().world;
    assert!((apply_point(&world, Vec3::X) - apply_point(&world, Vec3::ZERO) - Vec3::Y).len() < 1e-6, "local X now points along world Y");
    assert!((origin(&h, first) - before_first).len() < 1e-9, "the other placement stayed");
}

#[test]
fn move_of_a_nested_picked_component_moves_the_shared_child_in_every_copy_of_its_parent() {
    let mut h = Harness::new();
    let parent = h.app.run("component.create", json!({"name":"Assembly"})).unwrap();
    let (parent_component, parent_occurrence) = (parent["component"].as_u64().unwrap(), parent["occurrence"].as_u64().unwrap());
    let child = h.app.run("component.create", json!({"name":"Child"})).unwrap()["occurrence"].as_u64().unwrap();
    h.app.run("solid.box", json!({"length":10,"width":10,"height":10})).unwrap();
    h.app.run("component.activate", json!({"component":0})).unwrap();
    let other = h.app.run("occurrence.copy", json!({"component":parent_component,"translate":[80,0,0]})).unwrap()["occurrence"].as_u64().unwrap();
    h.frames(3);
    let (inside, outside) = ([other, child], [parent_occurrence, child]);
    let (before_inside, before_outside) = (placement_x(&h, &inside), placement_x(&h, &outside));
    let d = move_dialog_for(&mut h, &inside, |k| {
        if let crate::dialogs::Kind::Move { x, .. } = k {
            *x = "7 mm".into();
        }
    });
    crate::dialogs::run_dialog(&mut h.app, &d).unwrap();
    // The child's position is part of the parent's definition, so both parent copies show it moved.
    assert!((placement_x(&h, &inside) - before_inside - 7.0).abs() < 1e-6, "the picked nested copy moved 7 mm");
    assert!((placement_x(&h, &outside) - before_outside - 7.0).abs() < 1e-6, "the same child inside the other assembly moved too");
}

#[test]
fn move_of_a_grounded_placement_says_why_it_cannot() {
    let (mut h, _, first, second) = fixture();
    h.app.run("occurrence.ground", json!({"occurrence":second,"grounded":true})).unwrap();
    let before = placement_x(&h, &[second]);
    let d = move_dialog_for(&mut h, &[second], |k| {
        if let crate::dialogs::Kind::Move { x, .. } = k {
            *x = "7 mm".into();
        }
    });
    let error = crate::dialogs::run_dialog(&mut h.app, &d).unwrap_err();
    assert!(error.contains("grounded"), "{error}");
    assert_eq!(placement_x(&h, &[second]), before);
    let _ = first;
}
