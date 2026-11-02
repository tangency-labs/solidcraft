//! The browser's search (#1413): the field, the filtered tree, the kind list and the actions.

use serde_json::{Value, json};
use solidcraft_engine::Sel;
use solidcraft_engine::browser_find::{Kind, Pattern, find};
use solidcraft_engine::doc::FeatureKind;

use crate::scenario::{Harness, handle_at};

/// A base plate with a sketch and a plane; two columns with two M20 bolts each (a bolt has a
/// thread sketch); two more bolts and a washer on the plate, the washer held by a joint.
fn assembly() -> Harness {
    let mut h = Harness::new();
    h.app.home.open = false;
    // Tall enough for the whole tree below the search bar: rows out of view are not drawn.
    h.call("ui.resize", json!({"width": 1600, "height": 1300}));
    let script = json!({"commands": [
        {"command": "solid.box", "params": {"length": 420, "width": 120, "height": 10, "corner": [-60, -40, -10], "body_name": "Base plate"}},
        {"command": "sketch.create", "params": {"plane": "XY", "name": "Bolt holes"}},
        {"command": "sketch.circle.center", "params": {"center": [40, 20], "diameter": 22}},
        {"command": "sketch.finish", "params": {}},
        {"command": "construct.plane.offset", "params": {"base": "XY", "offset": 150, "name": "Bolt line"}},
        {"command": "component.create", "params": {"name": "Column"}},
        {"command": "solid.box", "params": {"length": 40, "width": 40, "height": 300, "corner": [0, 0, 0], "body_name": "Column web"}},
        {"command": "component.create", "params": {"name": "Bolt M20", "parent": "Column"}},
        {"command": "solid.cylinder", "params": {"radius": 10, "height": 80, "base": [20, -20, 80], "axis": "Y", "body_name": "Bolt shank"}},
        {"command": "sketch.create", "params": {"plane": "XZ", "name": "Thread profile"}},
        {"command": "sketch.circle.center", "params": {"center": [20, 80], "diameter": 20}},
        {"command": "sketch.finish", "params": {}},
        {"command": "component.activate", "params": {"component": "Column"}},
        {"command": "occurrence.copy", "params": {"component": "Bolt M20", "translate": [0, 0, 120]}},
        {"command": "component.activate", "params": {"component": "root"}},
        {"command": "occurrence.copy", "params": {"component": "Column", "translate": [300, 0, 0]}},
        {"command": "occurrence.copy", "params": {"component": "Bolt M20", "parent": "root", "translate": [150, 0, -60]}},
        {"command": "occurrence.copy", "params": {"component": "Bolt M20", "parent": "root", "translate": [190, 0, -60]}},
        {"command": "component.create", "params": {"name": "Washer M20"}},
        {"command": "solid.cylinder", "params": {"radius": 18, "height": 3, "base": [20, -23, 80], "axis": "Y", "body_name": "Washer"}},
        {"command": "component.activate", "params": {"component": "root"}}
    ]});
    h.app.session.run_script(&script).unwrap();
    let (bolt, washer) = (occurrence(&h, 0, "Bolt M20:3"), occurrence(&h, 0, "Washer M20:1"));
    h.app.run("joint.as_built", json!({"type": "rigid", "a": bolt, "b": washer, "name": "Washer seat"})).unwrap();
    h.frames(3);
    h
}

/// The same with the bar waiting for the header's action.
fn magnifier() -> Harness {
    let mut h = assembly();
    h.app.preferences.browser_search_bar = crate::prefs::BrowserSearchBar::Magnifier;
    h.frames(2);
    h
}

fn occurrence(h: &Harness, parent: u64, name: &str) -> u64 {
    h.app.session.doc.occurrences.iter().find(|o| o.parent == parent && o.name == name).map(|o| o.id).unwrap()
}

fn rows() -> Vec<String> {
    crate::browser::drawn_rows().into_iter().map(|r| r.0).collect()
}

fn count(name: &str) -> usize {
    rows().iter().filter(|r| *r == name).count()
}

fn placements() -> Vec<Vec<u64>> {
    crate::browser::drawn_occurrences().into_iter().map(|o| o.0).collect()
}

fn search(h: &mut Harness, text: &str) {
    h.app.tree.search.show();
    h.app.tree.search.text = text.to_string();
    h.frames(2);
}

fn click(h: &mut Harness, handle: &str) {
    let p = handle_at(handle).unwrap_or_else(|| panic!("no {handle}"));
    let r = h.call("ui.click", json!({"x": p.x, "y": p.y}));
    assert_eq!(r["ok"], true, "{r}");
    h.frames(2);
}

fn total(h: &Harness) -> usize {
    h.app.tree.search.hits().map_or(0, |hits| hits.total())
}

#[test]
fn typing_keeps_the_matches_with_their_parents() {
    let mut h = assembly();
    assert!(rows().iter().any(|r| r == "Document Settings"));
    assert!(handle_at("browser:search:show").is_none(), "the bar is there, with no action to open it");
    click(&mut h, "browser:search");
    h.call("ui.text", json!({"text": "m20"}));
    h.frames(2);
    assert_eq!(h.app.tree.search.text, "m20");
    assert_eq!(total(&h), 7);
    let (c1, c2) = (occurrence(&h, 0, "Column:1"), occurrence(&h, 0, "Column:2"));
    let column = h.app.session.doc.find_component("Column").unwrap();
    let (b1, b2) = (occurrence(&h, column, "Bolt M20:1"), occurrence(&h, column, "Bolt M20:2"));
    let (b3, b4, w) = (occurrence(&h, 0, "Bolt M20:3"), occurrence(&h, 0, "Bolt M20:4"), occurrence(&h, 0, "Washer M20:1"));
    // The seven placements, and the two columns that hold four of them.
    assert_eq!(placements(), [vec![c1], vec![c1, b1], vec![c1, b2], vec![c2], vec![c2, b1], vec![c2, b2], vec![b3], vec![b4], vec![w]]);
    for gone in ["Document Settings", "Origin", "Named Views", "Base plate", "Bolt holes", "Bolt line", "Bolt shank", "Joints", "Bodies"] {
        assert_eq!(count(gone), 0, "{gone} doesn't match `m20`");
    }
    assert!(handle_at("browser:search:select-all").is_some());
}

/// The count is the rows that match: the browser and `browser.find` agree.
#[test]
fn the_count_is_the_matching_rows() {
    let mut h = assembly();
    search(&mut h, "shank");
    assert_eq!((total(&h), count("Bolt shank")), (6, 6));
    search(&mut h, "bolt");
    let bolts = placements().len() - 2; // less the two columns
    assert_eq!(total(&h), count("Bolt shank") + bolts + count("Bolt holes") + count("Bolt line"));
    assert_eq!(total(&h), 6 + 6 + 2);
    search(&mut h, "*:1");
    assert_eq!(total(&h), 4);
    search(&mut h, "thread");
    assert_eq!((total(&h), count("Thread profile")), (6, 6));
    search(&mut h, "seat");
    assert_eq!((total(&h), count("Washer seat"), count("Joints")), (1, 1, 1));
    for text in ["bolt", "m20", "*", "?olumn*", "washer", "e"] {
        search(&mut h, text);
        assert_eq!(total(&h), find(&h.app.session, Pattern::new(text).as_ref(), &[]).total, "{text}");
    }
}

#[test]
fn no_match_says_so_and_lists_nothing() {
    let mut h = assembly();
    search(&mut h, "zzz");
    assert_eq!(total(&h), 0);
    assert!(h.app.tree.search.on());
    assert!(placements().is_empty());
    assert_eq!(count(&h.app.session.doc.name.clone()), 0);
    assert!(handle_at("browser:search:select-all").is_none());
}

/// Picks a kind from the list beside the field.
fn pick_kind(h: &mut Harness, kind: &str) {
    click(h, "browser:search:kinds");
    click(h, &format!("browser:search:kind:{kind}"));
}

#[test]
fn the_kind_list_narrows_a_search_and_alone_lists_a_kind() {
    let mut h = assembly();
    // The list holds every kind the design has, and All, and closes when one is picked.
    click(&mut h, "browser:search:kinds");
    for kind in ["all", "component", "body", "sketch", "construction", "joint"] {
        assert!(handle_at(&format!("browser:search:kind:{kind}")).is_some(), "{kind}");
    }
    click(&mut h, "browser:search:kind:body");
    assert!(handle_at("browser:search:kind:all").is_none());
    assert_eq!(h.app.tree.search.kind, Some(Kind::Body));
    assert_eq!(total(&h), 10);
    assert_eq!((count("Base plate"), count("Column web"), count("Bolt shank"), count("Washer")), (1, 2, 6, 1));
    assert_eq!(count("Thread profile") + count("Bolt holes"), 0);
    search(&mut h, "bolt");
    assert_eq!((total(&h), count("Bolt shank"), count("Bolt holes")), (6, 6, 0));
    pick_kind(&mut h, "sketch");
    assert_eq!((total(&h), count("Bolt holes"), count("Bolt shank")), (1, 1, 0));
    // All, with nothing typed, is no search: the tree is back.
    search(&mut h, "");
    pick_kind(&mut h, "all");
    assert!(!h.app.tree.search.on());
    assert!(rows().iter().any(|r| r == "Document Settings"));
}

#[test]
fn select_all_selects_what_was_found() {
    let mut h = assembly();
    search(&mut h, "m20:1");
    click(&mut h, "browser:search:select-all");
    assert_eq!(h.app.session.selection.len(), 3);
    assert!(h.app.session.selection.iter().all(|s| matches!(s, Sel::Occurrence { .. })));
    // A component's sketch is listed in each of its six placements, and is one item.
    search(&mut h, "thread");
    click(&mut h, "browser:search:select-all");
    let thread = h.app.session.doc.find_feature("Thread profile").unwrap().id;
    assert_eq!(h.app.session.selection, [Sel::Feature { id: thread }]);
    // Joints are picked in the tree.
    search(&mut h, "seat");
    click(&mut h, "browser:search:select-all");
    assert!(h.app.session.selection.is_empty());
    assert_eq!(h.app.tree.picked_joints, [h.app.session.doc.assembly.joints[0].id]);
}

#[test]
fn hide_others_leaves_only_what_was_found_shown() {
    let mut h = assembly();
    let world: Vec<String> = h.app.session.world_state().bodies.iter().map(|b| b.name.clone()).collect();
    assert_eq!(world.len(), 10);
    search(&mut h, "washer m20");
    click(&mut h, "browser:search:hide-others");
    let washer = h.app.session.occurrence_bodies(&[occurrence(&h, 0, "Washer M20:1")], true).unwrap();
    assert_eq!(washer.len(), 1);
    let mut hidden = h.app.session.visibility.hidden_bodies.clone();
    hidden.sort();
    let mut others: Vec<String> = world.iter().filter(|b| !washer.contains(b)).cloned().collect();
    others.sort();
    assert_eq!(hidden, others);
    // A search that found no body or placement has nothing to keep: the button is off.
    h.app.session.visibility.hidden_bodies.clear();
    search(&mut h, "holes");
    click(&mut h, "browser:search:hide-others");
    assert!(h.app.session.visibility.hidden_bodies.is_empty());
}

#[test]
fn a_search_opens_closed_rows_and_ending_it_restores_the_tree() {
    let mut h = assembly();
    let c1 = occurrence(&h, 0, "Column:1");
    h.app.tree.collapsed.insert(format!("o:{c1}"));
    h.frames(2);
    assert!(!placements().iter().any(|p| p.len() == 2 && p[0] == c1), "Column:1 starts closed");
    let folds = h.app.tree.collapsed.clone();
    search(&mut h, "m20");
    assert_eq!(placements().iter().filter(|p| p.len() == 2 && p[0] == c1).count(), 2, "the search opens it");
    // Closing it in the search hides its bolts, and leaves the tree's own folds alone.
    click(&mut h, &format!("fold:occurrence:{c1}"));
    assert!(!placements().iter().any(|p| p.len() == 2 && p[0] == c1));
    assert_eq!(h.app.tree.collapsed, folds);
    // Another search opens it again.
    search(&mut h, "m20:");
    assert_eq!(placements().iter().filter(|p| p.len() == 2 && p[0] == c1).count(), 2);
    // Esc in the field ends it: the tree as it was, Column:1 still closed.
    click(&mut h, "browser:search");
    h.call("ui.key", json!({"key": "Escape"}));
    h.frames(2);
    assert!(!h.app.tree.search.on());
    assert!(h.app.tree.search.text.is_empty());
    assert_eq!(h.app.tree.collapsed, folds);
    assert!(rows().iter().any(|r| r == "Document Settings"));
    assert!(!placements().iter().any(|p| p.len() == 2 && p[0] == c1));
    assert!(handle_at("browser:search").is_some(), "the bar stays");
}

/// The bar opened from the magnifier goes when Esc ends the search, and comes back empty.
#[test]
fn esc_closes_a_bar_that_was_asked_for() {
    let mut h = magnifier();
    click(&mut h, "browser:search:show");
    h.call("ui.text", json!({"text": "web"}));
    h.frames(2);
    h.call("ui.key", json!({"key": "Escape"}));
    h.frames(2);
    assert!(handle_at("browser:search").is_none() && !h.app.tree.search.on());
    click(&mut h, "browser:search:show");
    assert!(h.app.tree.search.text.is_empty());
}

#[test]
fn the_clear_button_ends_a_search() {
    let mut h = assembly();
    search(&mut h, "bolt");
    h.app.tree.search.kind = Some(Kind::Body);
    h.frames(2);
    click(&mut h, "browser:search:clear");
    assert!(!h.app.tree.search.on());
    assert_eq!((h.app.tree.search.text.as_str(), h.app.tree.search.kind), ("", None));
    assert!(handle_at("browser:search:clear").is_none(), "nothing left to clear");
    assert!(handle_at("browser:search").is_some(), "the bar stays");
    // A kind alone is a search to clear too.
    h.app.tree.search.kind = Some(Kind::Body);
    h.frames(2);
    click(&mut h, "browser:search:clear");
    assert_eq!(h.app.tree.search.kind, None);
}

/// The × of a bar that was asked for is there from the start, and closes it.
#[test]
fn the_clear_button_closes_a_bar_that_was_asked_for() {
    let mut h = magnifier();
    click(&mut h, "browser:search:show");
    click(&mut h, "browser:search:clear");
    assert!(handle_at("browser:search").is_none());
    assert!(!crate::browser_search::bar_shown(&h.app));
}

/// Changing the row mid-search never strands a filtered tree without its bar.
#[test]
fn changing_the_preference_keeps_a_search_endable() {
    let mut h = assembly();
    click(&mut h, "browser:search");
    h.call("ui.text", json!({"text": "web"}));
    h.frames(2);
    h.app.preferences.browser_search_bar = crate::prefs::BrowserSearchBar::Magnifier;
    h.frames(2);
    assert!(handle_at("browser:search").is_some() && handle_at("browser:search:clear").is_some());
    click(&mut h, "browser:search:clear");
    assert!(handle_at("browser:search").is_none());
    assert_eq!(count("Origin"), 1);
    h.app.preferences.browser_search_bar = crate::prefs::BrowserSearchBar::Always;
    h.frames(2);
    assert!(handle_at("browser:search").is_some() && handle_at("browser:search:show").is_none());
}

/// A restart brings the preference back, not the last search.
#[test]
fn a_restart_keeps_the_mode_and_drops_the_search() {
    for mode in crate::prefs::BrowserSearchBar::ALL {
        let mut h = assembly();
        h.app.preferences.browser_search_bar = mode;
        search(&mut h, "web");
        let mut restored = crate::SolveApp::new(Default::default(), Default::default());
        restored.load_prefs(&h.app.prefs());
        assert_eq!(restored.preferences.browser_search_bar, mode);
        assert!(restored.tree.search.text.is_empty() && !restored.tree.search.on());
    }
}

/// Always there, the bar gives its field the ×'s room until there is something to clear.
#[test]
fn the_idle_bar_has_no_clear_button() {
    let mut h = assembly();
    assert!(handle_at("browser:search").is_some() && handle_at("browser:search:clear").is_none());
    assert!(handle_at("browser:search:select-all").is_none() && !h.app.tree.search.on());
    assert!(rows().iter().any(|r| r == "Document Settings"), "the whole tree under it");
    h.call("ui.key", json!({"key": "Escape"}));
    h.frames(2);
    let idle = handle_at("browser:search").unwrap();
    search(&mut h, "web");
    assert!(handle_at("browser:search").unwrap().x < idle.x, "the field gives way to the ×");
}

#[test]
fn the_header_action_shows_the_bar_and_ends_the_search() {
    let mut h = magnifier();
    h.ctx.enable_accesskit();
    h.frames(2);
    assert_eq!(header_action_on(&h), Some(false));
    assert!(handle_at("browser:search").is_none(), "the bar waits for the header's action");
    h.app.preferences.browser_collapsed = true;
    h.frames(2);
    assert!(!rows().iter().any(|r| r == "Document Settings"), "the card is folded");
    click(&mut h, "browser:search:show");
    assert!(!h.app.preferences.browser_collapsed, "the bar's card unfolds");
    assert!(handle_at("browser:search").is_some());
    assert_eq!(header_action_on(&h), Some(true));
    h.call("ui.text", json!({"text": "washer"}));
    h.frames(2);
    assert_eq!(count("Washer"), 1);
    assert_eq!(count("Origin"), 0);
    click(&mut h, "browser:search:show");
    assert!(handle_at("browser:search").is_none());
    assert!(!h.app.tree.search.on());
    assert!(h.app.tree.search.text.is_empty());
    assert_eq!(count("Origin"), 1);
}

/// Hide others is one undo step (the plan on #1413), also when what it keeps was hidden.
#[test]
fn hide_others_is_one_undo_step() {
    let mut h = assembly();
    let washer = h.app.session.occurrence_bodies(&[occurrence(&h, 0, "Washer M20:1")], true).unwrap();
    h.app.run("browser.visibility", json!({"items": washer, "visible": false})).unwrap();
    let (steps, hidden, bodies) =
        (h.app.session.undo.len(), h.app.session.visibility.hidden_bodies.clone(), h.app.session.world_state().bodies.len());
    search(&mut h, "washer m20");
    click(&mut h, "browser:search:hide-others");
    assert_eq!(h.app.session.undo.len(), steps + 1);
    assert_eq!(h.app.session.visibility.hidden_bodies.len(), bodies - washer.len());
    h.app.run("edit.undo", json!({})).unwrap();
    h.frames(2);
    assert_eq!(h.app.session.visibility.hidden_bodies, hidden, "one undo brings back what was hidden before");
    assert_eq!(h.app.session.world_state().bodies.len(), bodies, "and no modelling step goes with it");
}

/// F2 on a group renames it during a search (#10013), and the search stays.
#[test]
fn f2_renames_a_group_during_a_search() {
    let mut h = assembly();
    let group = h.app.run("browser.group", json!({"component": 0, "folder": "sketches", "items": []})).unwrap()["group"].as_u64().unwrap();
    let sketch = h.app.session.doc.features.iter().find(|f| f.name == "Bolt holes").map(|f| f.id).unwrap();
    h.app.run("browser.move", json!({"component": 0, "folder": "sketches", "group": group, "items": [sketch.to_string()]})).unwrap();
    h.app.run("browser.rename_group", json!({"group": group, "name": "Holes"})).unwrap();
    search(&mut h, "bolt holes");
    click(&mut h, "row:Holes (1)");
    h.step(&json!({"key": "F2"})).unwrap();
    h.frames(2);
    assert!(matches!(h.app.menu.rename.as_ref().map(|r| &r.what), Some(crate::context_menu::RenameWhat::Group(id)) if *id == group));
    h.step(&json!({"text": "Slots"})).unwrap();
    h.frames(1);
    h.step(&json!({"key": "Enter"})).unwrap();
    h.frames(2);
    assert_eq!(h.app.tree.search.text, "bolt holes");
    assert_eq!((count("Slots (1)"), total(&h)), (1, 1));
}

/// Editing a sketch rolls the model back to it, and a body a later feature removes stays out of
/// the search (the engine's `a_body_removed_later_is_not_found_while_a_sketch_is_edited`): the
/// bar looks again when the sketch being edited changes, with no change of design.
#[test]
fn starting_to_edit_a_sketch_updates_the_search() {
    let mut h = Harness::new();
    h.app.home.open = false;
    for (name, x) in [("Kept block", 0), ("Gone block", 20)] {
        let sketch = format!("{name} sketch");
        h.app.run("sketch.create", json!({"plane": "XY", "name": sketch})).unwrap();
        h.app.run("sketch.rectangle.two_point", json!({"p0": [x, 0], "p1": [x + 10, 10]})).unwrap();
        h.app.run("sketch.finish", json!({})).unwrap();
        h.app.run("solid.extrude", json!({"sketch": sketch, "distance": 5, "body_name": name})).unwrap();
    }
    h.app.run("sketch.create", json!({"plane": "XY", "offset": "30 mm", "name": "Edited"})).unwrap();
    h.app.run("sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [5, 5]})).unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
    h.app.run("solid.remove", json!({"bodies": ["Gone block"]})).unwrap();
    h.app.run("timeline.roll_to", json!({"feature": "Edited"})).unwrap();
    search(&mut h, "block");
    h.app.tree.search.kind = Some(Kind::Body);
    h.frames(2);
    assert_eq!(total(&h), 2);
    h.app.run("sketch.edit", json!({"sketch": "Edited"})).unwrap();
    h.frames(2);
    assert_eq!(total(&h), 1);
}

/// The header's action as assistive technology reports it: on or off.
fn header_action_on(h: &Harness) -> Option<bool> {
    let label = crate::i18n::tr("ui.browser_search_hint");
    let nodes = &h.output.accesskit_update.as_ref()?.nodes;
    let (_, node) = nodes.iter().find(|(_, n)| n.role() == egui::accesskit::Role::Button && n.label() == Some(label))?;
    node.toggled().map(|on| on == egui::accesskit::Toggled::True)
}

/// A narrow card keeps the title and the action; the body count gives way, never overlaps.
#[test]
fn a_narrow_card_drops_the_body_count() {
    let mut h = magnifier();
    assert!(crate::scenario::rect_of("browser:body-count").is_some());
    h.app.preferences.browser_width = 170.0;
    h.frames(3);
    assert!(crate::scenario::rect_of("browser:body-count").is_none());
    assert!(handle_at("browser:search:show").is_some());
}

/// Render filters its tree with a search of its own: this one ends there and never shows.
#[test]
fn render_has_its_own_search() {
    let mut h = assembly();
    search(&mut h, "washer");
    assert!(h.app.tree.search.on());
    h.call("ui.workspace", json!({"workspace": "render"}));
    h.frames(3);
    assert!(crate::render_workspace::active(&h.app));
    assert!(!h.app.tree.search.on() && !crate::browser_search::bar_shown(&h.app));
    assert!(handle_at("browser:search").is_none() && handle_at("browser:search:show").is_none());
}

/// The vertical history's Bodies folder and Origin are the browser's rows, unfiltered.
#[test]
fn the_history_rows_stay_out_of_the_search() {
    let mut h = assembly();
    h.app.preferences.timeline_left = true;
    h.call("ui.prefs", json!({"timeline_origin_bodies": true}));
    search(&mut h, "washer");
    assert_eq!(h.app.tree.search.text, "washer", "the history gives the search back");
    assert!(h.app.tree.search.on());
    assert_eq!(count("Origin"), 0, "the browser's own rows stay filtered");
    assert!(handle_at("row:Origin").is_some(), "the history's Origin is drawn");
    assert_eq!(count("Base plate"), 0, "the browser's own rows don't list it");
    assert!(handle_at("row:Base plate").is_some(), "the history's Bodies are all there");
}

/// A group is listed when something inside it matches, at any depth, and only then.
#[test]
fn groups_list_only_what_holds_a_match() {
    let mut h = assembly();
    let outer = h.app.run("browser.group", json!({"component": 0, "folder": "sketches", "items": []})).unwrap()["group"].as_u64().unwrap();
    let inner =
        h.app.run("browser.group", json!({"component": 0, "folder": "sketches", "items": [], "parent": outer})).unwrap()["group"].as_u64().unwrap();
    let empty = h.app.run("browser.group", json!({"component": 0, "folder": "sketches", "items": []})).unwrap()["group"].as_u64().unwrap();
    let sketch = h.app.session.doc.features.iter().find(|f| f.name == "Bolt holes").map(|f| f.id).unwrap();
    h.app.run("browser.move", json!({"component": 0, "folder": "sketches", "group": inner, "items": [sketch.to_string()]})).unwrap();
    for (id, name) in [(outer, "Outer"), (inner, "Inner"), (empty, "Empty")] {
        h.app.run("browser.rename_group", json!({"group": id, "name": name})).unwrap();
    }
    h.frames(2);
    search(&mut h, "bolt holes");
    assert_eq!((count("Outer (1)"), count("Inner (1)"), count("Bolt holes"), count("Empty (0)")), (1, 1, 1, 0));
    // A row closed in the search hides what it holds; the tree's own folds stay as they were.
    let folds = h.app.tree.collapsed.clone();
    crate::browser::toggle(&mut h.app, &format!("c0/g{inner}"), true);
    h.frames(2);
    assert_eq!((count("Inner (1)"), count("Bolt holes")), (1, 0));
    h.app.tree.search.clear();
    h.frames(2);
    assert_eq!(h.app.tree.collapsed, folds);
    assert_eq!((count("Outer (1)"), count("Empty (0)")), (1, 1));
}

/// A placement's own Joints folder (#9130) stays out: the root folder lists the joints found.
#[test]
fn a_placement_lists_no_joints_during_a_search() {
    let mut h = assembly();
    let bolt = occurrence(&h, 0, "Bolt M20:3");
    assert!(handle_at("row:Joints").is_some());
    // The bolt holds a match, so its row is open.
    search(&mut h, "bolt");
    assert!(placements().contains(&vec![bolt]));
    assert_eq!(count("Joints"), 0);
    search(&mut h, "washer seat");
    assert_eq!((count("Joints"), count("Washer seat")), (1, 1));
}

#[test]
fn the_search_is_in_both_browser_layouts() {
    let mut h = assembly();
    h.app.preferences.timeline_left = true;
    h.frames(2);
    assert!(handle_at("left-browser-toggle").is_some(), "the browser is below the timeline");
    search(&mut h, "washer");
    assert_eq!((total(&h), count("Washer seat"), count("Washer")), (3, 1, 1));
    assert!(handle_at("browser:search:select-all").is_some());
}

#[test]
fn ctrl_f_over_the_browser_takes_the_keyboard() {
    assert!(
        crate::keymap::RESERVED.contains(&crate::keymap::key_name(egui::Key::F, egui::Modifiers::COMMAND).as_str()),
        "it can't be given to a command"
    );
    let mut h = assembly();
    let row = handle_at("row:Document Settings").unwrap();
    h.call("ui.move", json!({"x": row.x, "y": row.y}));
    h.frames(1);
    h.call("ui.key", json!({"key": "F", "cmd": true}));
    h.frames(2);
    h.call("ui.text", json!({"text": "web"}));
    h.frames(2);
    assert_eq!(h.app.tree.search.text, "web");
    assert_eq!(count("Column web"), 2);
}

/// In the magnifier's mode Ctrl+F opens the bar first, unfolding its card.
#[test]
fn ctrl_f_opens_a_bar_that_waits() {
    let mut h = magnifier();
    let row = handle_at("row:Document Settings").unwrap();
    h.call("ui.move", json!({"x": row.x, "y": row.y}));
    h.frames(1);
    h.call("ui.key", json!({"key": "F", "cmd": true}));
    h.frames(2);
    assert!(handle_at("browser:search").is_some());
    h.call("ui.text", json!({"text": "web"}));
    h.frames(2);
    assert_eq!(h.app.tree.search.text, "web");
}

/// Names in other scripts match, ignoring case where the script has one, and the field's limit
/// counts letters, not bytes.
#[test]
fn names_in_any_script_are_found() {
    let mut h = assembly();
    for (name, text) in [("בורג", "בורג"), ("ボルト", "ボルト"), ("螺栓上", "螺栓"), ("볼트", "볼트"), ("Болт", "болт"), ("Écrou", "écrou")]
    {
        h.app.run("solid.box", json!({"length": 5, "width": 5, "height": 5, "corner": [0, 200, 0], "body_name": name})).unwrap();
        search(&mut h, text);
        assert_eq!((total(&h), count(name)), (1, 1), "{name}");
    }
    click(&mut h, "browser:search");
    h.app.tree.search.text.clear();
    h.call("ui.text", json!({"text": "ב".repeat(300)}));
    h.frames(2);
    assert_eq!(h.app.tree.search.text.chars().count(), solidcraft_engine::browser_find::MAX_PATTERN_CHARS);
}

#[test]
fn ctrl_f_leaves_a_field_being_typed_in() {
    let mut h = magnifier();
    h.call("ui.palette", json!({}));
    h.frames(10);
    assert!(h.ctx.text_edit_focused(), "the toolbar's search is being typed in");
    let row = handle_at("row:Document Settings").unwrap();
    h.call("ui.move", json!({"x": row.x, "y": row.y}));
    h.frames(2);
    h.call("ui.key", json!({"key": "F", "cmd": true}));
    h.frames(2);
    assert!(!crate::browser_search::bar_shown(&h.app));
    assert!(h.ctx.text_edit_focused());
}

/// Find in Browser, the menu action, as a user runs it.
fn find_in_browser(h: &mut Harness, params: Value) {
    let item = crate::context_menu::act("ui.findBrowser", "Find in Browser", "list").with(params);
    crate::context_menu::run_item(&mut h.app, &item, egui::pos2(0.0, 0.0));
    h.frames(3);
}

/// A pick in the view, as the viewport does it: the picked body selected, then revealed.
fn pick(h: &mut Harness, world: &str) {
    h.app.session.selection = vec![Sel::Body { name: world.into() }];
    crate::browser::reveal_pick(&mut h.app);
    h.frames(3);
}

/// The bodies the search found in the placements under `first` (their names in the world).
fn found_under(h: &Harness, first: u64) -> Vec<String> {
    let found = &h.app.tree.search.hits().unwrap().search.found;
    found
        .iter()
        .filter(|f| f.path.first() == Some(&first))
        .filter_map(|f| match &f.item {
            Some(Sel::Body { name }) => Some(name.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn find_in_browser_ends_the_search() {
    let mut h = assembly();
    search(&mut h, "m20");
    find_in_browser(&mut h, json!({"body": "Base plate"}));
    assert!(!h.app.tree.search.on());
    assert_eq!(count("Base plate"), 1);
}

/// A pick in the view keeps the search (#1413 with #3081): the text and the results stay, and the
/// picked body's row opens under a placement closed in the search, scrolled to.
#[test]
fn a_pick_during_a_search_keeps_it_and_opens_the_picked_row() {
    let mut h = assembly();
    let c1 = occurrence(&h, 0, "Column:1");
    search(&mut h, "shank");
    let (text, found) = (h.app.tree.search.text.clone(), total(&h));
    let shank = found_under(&h, c1).first().cloned().expect("a shank under Column:1");
    click(&mut h, &format!("fold:occurrence:{c1}"));
    assert!(!placements().iter().any(|p| p.len() == 2 && p[0] == c1), "Column:1 closed in the search");
    pick(&mut h, &shank);
    assert!(h.app.tree.search.on(), "the search stays");
    assert_eq!((h.app.tree.search.text.clone(), total(&h)), (text, found), "with its text and results");
    assert!(placements().iter().any(|p| p.len() == 2 && p[0] == c1), "Column:1 opened for the pick");
    assert!(!h.app.tree.search.closed(&format!("o:{c1}")));
    assert!(h.app.tree.reveal.is_none(), "the row was drawn and scrolled to");
    assert!(!rows().iter().any(|r| r == "Document Settings"), "still the search's rows, not the whole tree");
}

/// A pick of a body the search didn't find changes no row, and the search stays: not even the
/// placement holding it, closed in the search though it lists other matches.
#[test]
fn a_pick_the_search_did_not_find_moves_no_row() {
    let mut h = assembly();
    let c1 = occurrence(&h, 0, "Column:1");
    // Column:1's web, by its name in the world: in the placement the search lists, not found by it.
    let webs = find(&h.app.session, Pattern::new("web").as_ref(), &[Kind::Body]);
    let web = webs.found.iter().find(|f| f.path.first() == Some(&c1)).and_then(|f| f.item.clone()).unwrap();
    let Sel::Body { name: web } = web else { panic!("{web:?}") };
    search(&mut h, "shank");
    click(&mut h, &format!("fold:occurrence:{c1}"));
    let (before, folds) = (rows(), h.app.tree.collapsed.clone());
    for body in [web.as_str(), "Base plate"] {
        pick(&mut h, body);
        assert!(h.app.tree.search.on(), "{body}");
        assert_eq!(rows(), before, "{body}");
        assert!(h.app.tree.search.closed(&format!("o:{c1}")), "{body}: a row closed in the search stays closed");
        assert_eq!(h.app.tree.collapsed, folds, "{body}");
    }
}

/// Find in Browser on a face opens the Features folder at the face's feature, so it ends the search too. During a
/// search that folder isn't listed: a search never looks for features.
#[test]
fn find_in_browser_on_a_face_ends_the_search_and_a_search_leaves_out_the_features_folder() {
    let mut h = assembly();
    assert!(count("Features") > 0);
    search(&mut h, "m20");
    assert_eq!(count("Features"), 0);
    let made_by = |f: &&solidcraft_engine::doc::Feature| {
        f.component == 0 && !matches!(f.kind, FeatureKind::Sketch { .. } | FeatureKind::ConstructionPlane { .. })
    };
    let feature = h.app.session.doc.features.iter().find(made_by).map(|f| f.id).unwrap();
    crate::browser::reveal_feature(&mut h.app, feature, "Base plate");
    h.frames(3);
    assert!(!h.app.tree.search.on());
    assert!(count("Features") > 0);
}

#[test]
fn the_design_changing_updates_the_search() {
    let mut h = assembly();
    search(&mut h, "washer");
    h.app.tree.search.kind = Some(Kind::Body);
    h.frames(2);
    assert_eq!((total(&h), count("Washer")), (1, 1));
    h.app.run("body.rename", json!({"body": "Washer", "name": "Spacer"})).unwrap();
    h.frames(2);
    assert_eq!((total(&h), count("Spacer")), (0, 0));
    h.app.run("edit.undo", json!({})).unwrap();
    h.frames(2);
    assert_eq!((total(&h), count("Washer")), (1, 1));
    // A new body that matches, and undoing it.
    h.app.run("solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [0, 0, 400], "body_name": "Washer blank"})).unwrap();
    h.frames(2);
    assert_eq!(total(&h), 2);
    h.app.run("edit.undo", json!({})).unwrap();
    h.frames(2);
    assert_eq!(total(&h), 1);
    // The evaluated bodies changing under the same document.
    let doc = h.app.session.doc.clone();
    assert!(h.app.session.model.rename_body(&doc, "Washer", "Spacer"));
    h.frames(2);
    assert_eq!(total(&h), 0);
    // A sketch's name, which the document holds.
    h.app.tree.search.kind = None;
    search(&mut h, "bolt holes");
    assert_eq!(total(&h), 1);
    h.app.run("timeline.rename", json!({"feature": "Bolt holes", "name": "Slots"})).unwrap();
    h.frames(2);
    assert_eq!(total(&h), 0);
    // A placement's name, and a joint's, which only the document holds.
    search(&mut h, "washer m20");
    assert_eq!(total(&h), 1);
    let state = h.app.session.model.state();
    h.app.run("component.rename", json!({"component": "Washer M20", "name": "Spacer M20"})).unwrap();
    h.frames(2);
    assert_eq!(total(&h), 0, "same bodies: {}", std::sync::Arc::ptr_eq(&state, &h.app.session.model.state()));
}

/// Whatever is typed, the browser draws and its count is the engine's.
#[test]
fn hostile_text_in_the_field_never_panics() {
    let mut h = assembly();
    let mut seed = 0x9e37_79b9_7f4a_7c15_u64;
    let alphabet: Vec<char> = "ab*?:M2 0\u{0}\u{FFFF}é\u{10FFFF}".chars().collect();
    for _ in 0..150 {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        let len = (seed % 12) as usize;
        let text: String = (0..len).map(|i| alphabet[((seed >> (i * 5)) as usize) % alphabet.len()]).collect();
        search(&mut h, &text);
        let want = Pattern::new(&text).map(|p| find(&h.app.session, Some(&p), &[]).total);
        assert_eq!(h.app.tree.search.hits().map(|hits| hits.total()), want, "{text:?}");
    }
    // Longer than the field takes: the field keeps the first 256 characters.
    click(&mut h, "browser:search");
    h.app.tree.search.text.clear();
    h.call("ui.text", json!({"text": "b".repeat(300)}));
    h.frames(2);
    assert!(h.app.tree.search.text.chars().count() <= solidcraft_engine::browser_find::MAX_PATTERN_CHARS);
    let _: Value = h.call("ui.key", json!({"key": "Escape"}));
}

/// A search shows only some of a folder: dragging there would reorder the folder from those
/// alone, so rows don't drag until it ends.
#[test]
fn rows_do_not_drag_during_a_search() {
    let mut h = Harness::new();
    h.app.home.open = false;
    h.app.session.run_script(&solidcraft_engine::sample::script()).unwrap();
    h.frames(3);
    search(&mut h, "bo");
    assert_eq!((count("Boss"), count("Bore"), count("Base")), (1, 1, 0));
    let order = h.app.session.doc.browser_order.clone();
    let (bore, boss) = (handle_at("row:Bore").unwrap(), handle_at("row:Boss").unwrap());
    h.call("ui.drag", json!({"x0": bore.x + 40.0, "y0": bore.y, "x1": boss.x + 40.0, "y1": boss.y - 8.0, "steps": 12}));
    h.frames(4);
    assert_eq!(h.app.session.doc.browser_order, order);
}

#[test]
fn groups_do_not_drag_during_a_search() {
    let mut h = assembly();
    let script = json!({"commands": [
        {"command": "sketch.create", "params": {"plane": "XY", "name": "Bolt outline"}},
        {"command": "sketch.circle.center", "params": {"center": [60, 20], "diameter": 30}},
        {"command": "sketch.finish", "params": {}}
    ]});
    h.app.session.run_script(&script).unwrap();
    let mut group = |sketch: &str, name: &str| {
        let id = h.app.run("browser.group", json!({"component": 0, "folder": "sketches", "items": []})).unwrap()["group"].as_u64().unwrap();
        let sketch = h.app.session.doc.features.iter().find(|f| f.name == sketch).map(|f| f.id).unwrap();
        h.app.run("browser.move", json!({"component": 0, "folder": "sketches", "group": id, "items": [sketch.to_string()]})).unwrap();
        h.app.run("browser.rename_group", json!({"group": id, "name": name})).unwrap();
        id
    };
    let holes = group("Bolt holes", "Holes");
    group("Bolt outline", "Outlines");
    h.frames(2);
    let drag = |h: &mut Harness| {
        let (from, to) = (handle_at("row:Holes (1)").unwrap(), handle_at("row:Outlines (1)").unwrap());
        h.call("ui.drag", json!({"x0": from.x + 40.0, "y0": from.y, "x1": to.x + 40.0, "y1": to.y + 4.0, "steps": 12}));
        h.frames(4);
    };
    let level = h.app.session.doc.group_level(holes);
    search(&mut h, "bolt");
    drag(&mut h);
    assert_eq!(h.app.session.doc.group_level(holes), level, "the search shows only part of the folder");
    h.app.tree.search.clear();
    h.frames(2);
    drag(&mut h);
    assert_eq!(h.app.session.doc.group_level(holes), level + 1, "without a search the same drag moves it");
}

/// During a search only the rows with something listed beneath them fold: a matching
/// placement with nothing under it is a leaf, not an open row with nothing in it.
#[test]
fn only_rows_with_something_beneath_fold_during_a_search() {
    let mut h = assembly();
    let folds = |name: &str| crate::browser::drawn_rows().into_iter().find(|r| r.0 == name).map(|r| r.3.is_some());
    search(&mut h, "m20");
    assert_eq!((folds("Column:1"), folds("Bolt M20:3"), folds("Washer M20:1")), (Some(true), Some(false), Some(false)));
    // The washer's body matches too: its placement folds to show it.
    search(&mut h, "washer");
    assert_eq!((folds("Washer M20:1"), folds("Washer")), (Some(true), Some(false)));
    // Without a search, every placement folds as before (the columns closed, so it is in view).
    for column in ["Column:1", "Column:2"] {
        h.app.tree.collapsed.insert(format!("o:{}", occurrence(&h, 0, column)));
    }
    search(&mut h, "");
    assert_eq!(folds("Bolt M20:3"), Some(true));
}

/// The kind list and the buttons fit the browser in every language: a long translation moves a
/// button to the next line rather than running past the panel's edge, where it would be cut off,
/// and a button's label never wraps inside it (the two buttons stay one line high).
#[test]
fn the_search_bar_fits_the_browser_in_every_language() {
    let mut h = assembly();
    search(&mut h, "m20");
    for catalog in crate::i18n::LOCALES {
        h.app.preferences.language = catalog.id.to_string();
        h.frames(2);
        let bar = crate::scenario::rect_of("browser:search:bar").unwrap();
        for name in ["browser:search:kinds", "browser:search:select-all", "browser:search:hide-others"] {
            let r = crate::scenario::rect_of(name).unwrap_or_else(|| panic!("{}: no {name}", catalog.id));
            assert!(r.max.x <= bar.max.x + 0.5, "{}: {name} ends at {} past the browser's {}", catalog.id, r.max.x, bar.max.x);
        }
        let height = |name| crate::scenario::rect_of(name).map(|r| r.height());
        assert_eq!(height("browser:search:select-all"), height("browser:search:hide-others"), "{}", catalog.id);
    }
}
