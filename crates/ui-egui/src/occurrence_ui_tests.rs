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
#[test]
fn a_folded_body_sits_one_level_under_its_nested_placement() {
    use crate::prefs::BodiesFolders;
    let mut h = Harness::new();
    let parent = h.app.run("component.create", json!({"name":"Assembly"})).unwrap()["occurrence"].as_u64().unwrap();
    let child = h.app.run("component.create", json!({"name":"Child"})).unwrap()["occurrence"].as_u64().unwrap();
    h.app.run("solid.box", json!({"length":10,"width":10,"height":10,"body_name":"Pin"})).unwrap();
    h.app.run("component.activate", json!({"component":0})).unwrap();
    h.app.tree.collapsed.insert(format!("{}/bodies", super::occurrence_key(&[parent, child])));
    // How many levels the body's row sits under its placement's (each eye sits at the same offset
    // in its row).
    let levels = || -> Option<i32> {
        let placement = crate::scenario::handle_at(&format!("eye:occurrence:{parent}/{child}"))?;
        let body = crate::scenario::handle_at("eye:Pin")?;
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
    assert!(!h.app.tree.order.contains(&format!("b:{body}")), "the body's row is closed away");
    h.app.session.selection = vec![Sel::Body { name: body.clone() }];
    let find = context_menu::items(&h.app, &Target::Viewport).into_iter().find(|i| i.id == "ui.findBrowser").unwrap();
    context_menu::run_item(&mut h.app, &find, egui::Pos2::ZERO);
    assert_eq!(h.app.tree.reveal.as_deref(), Some(body.as_str()));
    h.frames(2);
    assert!(h.app.tree.reveal.is_none(), "revealed");
    assert!(!h.app.tree.collapsed.contains(&scope));
    assert!(h.app.tree.order.contains(&format!("b:{body}")), "the row is drawn");
    assert!(crate::scenario::handle_at("fold:Bodies").is_none(), "with no folder row");
}

/// #4217: while a placement's Bodies folder is folded away, the placement's menu takes the
/// folder's items it lacks: Create Components from Bodies (the folder's bodies) and, with several
/// of them selected, Group Selected. With the folder drawn they stay on the folder's own menu.
#[test]
fn a_folded_folders_menu_items_move_to_the_placements_menu() {
    use crate::prefs::BodiesFolders;
    let (mut h, component, first, _) = fixture();
    h.app.run("component.activate", json!({"component":component})).unwrap();
    h.app.run("solid.box", json!({"length":5,"width":5,"height":5,"corner":[20,0,0],"body_name":"Second"})).unwrap();
    h.app.run("component.activate", json!({"component":0})).unwrap();
    let target = Target::Occurrence { path: vec![first] };
    let has = |h: &Harness, id: &str| context_menu::items(&h.app, &target).iter().any(|i| i.id == id);
    assert!(!has(&h, "component.from_bodies"), "Several bodies only: two bodies keep their folder");
    h.app.preferences.browser_bodies_folders = BodiesFolders::Never;
    let items = context_menu::items(&h.app, &target);
    let from = items.iter().find(|i| i.id == "component.from_bodies").unwrap();
    assert_eq!(from.params.as_ref().unwrap()["bodies"], json!(["Body1", "Second"]));
    assert!(has(&h, "ui.newGroup") && has(&h, "ui.showAll"), "on every component's menu");
    assert!(!has(&h, "ui.groupSelected"), "nothing selected");
    h.app.session.selection = vec![Sel::Body { name: "Body1".into() }, Sel::Body { name: "Second".into() }];
    let group = context_menu::items(&h.app, &target).into_iter().find(|i| i.id == "ui.groupSelected").unwrap();
    context_menu::run_item(&mut h.app, &group, egui::Pos2::ZERO);
    let made = h.app.session.doc.browser_groups.iter().find(|g| g.component == component).unwrap();
    assert_eq!(made.items, ["Body1", "Second"]);
    h.app.preferences.browser_bodies_folders = BodiesFolders::Always;
    assert!(!has(&h, "component.from_bodies"), "Always: on the folder's own menu");
}
