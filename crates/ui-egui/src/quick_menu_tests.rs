use serde_json::json;
use solidcraft_engine::Sel;

use super::*;
use crate::context_menu::{Target, items as menu_items};

fn curve(id: &str) -> Sel {
    Sel::SketchCurve { id: id.into(), sketch: None }
}

/// A harness in a sketch with two lines (`l1`, `l2`) and two circles (`c1`, `c2`).
fn sketch() -> crate::scenario::Harness {
    let mut h = crate::scenario::Harness::new();
    h.app.home.open = false;
    h.app.run("sketch.create", json!({"plane": "XY"})).unwrap();
    h.app.run("sketch.line", json!({"points": [[0, 0], [30, 0]], "ids": ["l1"], "connect": false, "infer": false})).unwrap();
    h.app.run("sketch.line", json!({"points": [[0, 10], [30, 14]], "ids": ["l2"], "connect": false, "infer": false})).unwrap();
    h.app.run("sketch.circle.center", json!({"center": [60, 0], "radius": 5, "id": "c1"})).unwrap();
    h.app.run("sketch.circle.center", json!({"center": [90, 0], "radius": 8, "id": "c2"})).unwrap();
    h
}

fn ids(items: &[Item]) -> Vec<&str> {
    items.iter().filter(|i| i.id != "-" && !i.id.starts_with('#')).map(|i| i.id.as_str()).collect()
}

/// The commands the selection fits come from their own specs, with only the constraints whose
/// kinds suit it: two lines, a line and a circle, two circles.
#[test]
fn the_quick_menu_lists_what_the_selection_fits() {
    let mut h = sketch();
    h.app.session.selection = vec![curve("l2"), curve("l1")];
    let two_lines = fitting(&h.app);
    for id in ["sketch.constraint.parallel", "sketch.constraint.perpendicular", "sketch.constraint.collinear", "sketch.constraint.equal"] {
        let item = two_lines.iter().find(|i| i.id == id).unwrap_or_else(|| panic!("{id}: {:?}", ids(&two_lines)));
        assert_eq!(item.params, Some(json!({"a": "l2", "b": "l1"})), "{id} runs on the picks in order");
    }
    for id in ["sketch.constraint.tangent", "sketch.constraint.concentric", "solid.fillet"] {
        assert!(!ids(&two_lines).contains(&id), "{id} doesn't take two lines: {:?}", ids(&two_lines));
    }
    h.app.session.selection = vec![curve("l1"), curve("c1")];
    let line_circle = ids(&fitting(&h.app)).iter().map(|s| s.to_string()).collect::<Vec<_>>();
    assert!(line_circle.contains(&"sketch.constraint.tangent".to_string()), "{line_circle:?}");
    assert!(!line_circle.contains(&"sketch.constraint.parallel".to_string()), "{line_circle:?}");
    h.app.session.selection = vec![curve("c1"), curve("c2")];
    let circles = fitting(&h.app);
    for id in ["sketch.constraint.concentric", "sketch.constraint.equal", "sketch.constraint.tangent"] {
        assert!(ids(&circles).contains(&id), "{id}: {:?}", ids(&circles));
    }
    // Nothing selected: the sketch's tools, as on the gesture ring.
    h.app.session.selection.clear();
    assert!(fitting(&h.app).is_empty());
    assert!(ids(&items(&h.app)).contains(&"sketch.line"), "{:?}", ids(&items(&h.app)));
}

/// A model edge offers Fillet and Chamfer, started as from the toolbar so their dialog takes the
/// edge; faces offer Shell; two edges of different bodies offer no blend.
#[test]
fn model_picks_offer_their_commands_to_start() {
    let mut h = crate::scenario::Harness::new();
    h.app.home.open = false;
    h.app.run("solid.box", json!({"length": 10, "width": 20, "height": 30})).unwrap();
    let body = h.app.session.world_state().unwrap().bodies.first().map(|b| b.name.clone()).unwrap();
    let edge = |x: f64| Sel::Edge { body: body.clone(), index: 0, point: solidcore_record::Vec3::new(x, 0.0, 0.0) };
    h.app.session.selection = vec![edge(5.0)];
    let fit = fitting(&h.app);
    for id in ["solid.fillet", "solid.chamfer"] {
        let item = fit.iter().find(|i| i.id == id).unwrap_or_else(|| panic!("{id}: {:?}", ids(&fit)));
        assert!(item.params.is_none(), "{id} starts its dialog, which takes the edge");
    }
    assert!(!ids(&fit).iter().any(|id| id.starts_with("sketch.constraint.")), "{:?}", ids(&fit));
    h.app.session.selection = vec![Sel::Face { body: body.clone(), index: 0, point: solidcore_record::Vec3::new(5.0, 10.0, 30.0) }];
    assert!(ids(&fitting(&h.app)).contains(&"solid.shell"), "{:?}", ids(&fitting(&h.app)));
    h.app.session.selection = vec![edge(5.0), Sel::Edge { body: "Other".into(), index: 1, point: solidcore_record::Vec3::new(0.0, 0.0, 0.0) }];
    assert!(!ids(&fitting(&h.app)).contains(&"solid.fillet"));
}

/// Shift+S opens the quick menu at the cursor; changing the selection closes it.
#[test]
fn the_key_opens_the_quick_menu_and_a_new_selection_closes_it() {
    let mut h = sketch();
    h.app.session.selection = vec![curve("l2"), curve("l1")];
    h.frames(3);
    h.step(&json!({"key": "S", "shift": true})).unwrap();
    h.frames(2);
    assert!(h.app.viewport.context_menu.is_some(), "Shift+S opens the menu");
    assert_eq!(h.app.menu.target, Target::Quick);
    let listed = h.call("ui.menu", json!({}));
    let labels: Vec<&str> = listed["result"]["items"].as_array().into_iter().flatten().filter_map(|i| i["label"].as_str()).collect();
    assert!(labels.contains(&"Parallel"), "{listed}");
    let _ = h.app.run("select.set", json!({"items": [{"type": "sketch_curve", "id": "c1"}]}));
    h.frames(2);
    assert!(h.app.viewport.context_menu.is_none(), "a new selection closes it");
}

/// The menu closes once the pointer is [`LEAVE`] points clear of it, not before.
#[test]
fn the_menu_closes_when_the_pointer_moves_away() {
    let rect = Rect::from_min_size(egui::pos2(100.0, 100.0), egui::vec2(250.0, 200.0));
    assert!(!left(rect, Some(egui::pos2(120.0, 120.0))));
    assert!(!left(rect, Some(egui::pos2(100.0 - LEAVE + 1.0, 150.0))));
    assert!(left(rect, Some(egui::pos2(100.0 - LEAVE - 1.0, 150.0))));
    assert!(!left(rect, None), "no pointer: it stays");
}

/// Off by default; with the preference off, selecting never opens the menu.
#[test]
fn opening_after_a_selection_is_off_by_default() {
    assert!(!crate::prefs::Prefs::default().quick_menu_after_selection);
    let mut h = sketch();
    h.app.menu.quick.click = Some((egui::pos2(200.0, 200.0), 0.0));
    h.app.session.selection = vec![curve("l2"), curve("l1")];
    h.frames(30);
    assert!(h.app.viewport.context_menu.is_none());
}

/// With the preference on, a click's selection that a command takes opens the menu where the
/// click was, after a short pause; a selection nothing takes opens nothing.
#[test]
fn with_the_preference_a_selecting_click_opens_the_menu() {
    let mut h = sketch();
    h.app.preferences.quick_menu_after_selection = true;
    h.frames(2);
    let at = egui::pos2(400.0, 300.0);
    h.app.synthetic.push(egui::Event::PointerMoved(at));
    h.frames(1);
    h.app.menu.quick.click = Some((at, h.ctx.input(|i| i.time)));
    h.app.session.selection = vec![curve("l2"), curve("l1")];
    for _ in 0..40 {
        h.frames(1);
        if h.app.viewport.context_menu.is_some() {
            break;
        }
    }
    assert_eq!(h.app.viewport.context_menu, Some(at), "it opens where the click was");
    assert_eq!(h.app.menu.target, Target::Quick);
    assert!(ids(&menu_items(&h.app, &Target::Quick)).contains(&"sketch.constraint.parallel"));
}

/// The right-click menu's constraints come from the same specs: what it offered before for two
/// lines, two circles, a line and a circle, still offered, first and in the order it had.
#[test]
fn the_right_click_constraints_come_from_the_command_specs() {
    let mut h = sketch();
    let cases: [(&[&str], &[&str]); 3] = [
        (
            &["l1", "l2"],
            &[
                "sketch.constraint.parallel",
                "sketch.constraint.perpendicular",
                "sketch.constraint.collinear",
                "sketch.constraint.equal",
                "sketch.constraint.midpoint",
            ],
        ),
        (&["c1", "c2"], &["sketch.constraint.concentric", "sketch.constraint.equal", "sketch.constraint.tangent"]),
        (&["l1", "c1"], &["sketch.constraint.tangent"]),
    ];
    for (picks, expected) in cases {
        h.app.session.selection = picks.iter().map(|id| curve(id)).collect();
        let menu = menu_items(&h.app, &Target::Viewport);
        let listed = ids(&menu);
        for id in expected {
            assert!(listed.contains(id), "{picks:?}: {id} missing from {listed:?}");
        }
        assert!(!listed.contains(&"sketch.constraint.fix"), "Fix/Unfix keeps its own row");
        let heading = menu.iter().position(|i| i.is_heading() && i.label == "Constraints").expect("a Constraints heading");
        let constraints: Vec<&str> = ids(&menu[heading + 1..]);
        assert_eq!(&constraints[..expected.len()], expected, "{picks:?}: the rows it had come first, in their old order");
    }
}

/// A box that takes two lines and a constraint icon still offers what the two lines take: the
/// rules read only the selection's sketch geometry.
#[test]
fn constraint_icons_in_the_selection_do_not_hide_the_lines_constraints() {
    let mut h = sketch();
    h.app.run("sketch.constraint.horizontal_vertical", json!({"line": "l1", "mode": "horizontal"})).unwrap();
    let st = h.app.session.world_state().unwrap();
    let icon = st.sketch(h.app.session.active_sketch.unwrap()).unwrap().sketch.constraints.last().map(|c| c.id.clone()).unwrap();
    h.app.session.selection = vec![curve("l1"), curve("l2"), Sel::SketchConstraint { id: icon }];
    let listed = ids(&menu_items(&h.app, &Target::Viewport)).iter().map(|s| s.to_string()).collect::<Vec<_>>();
    for id in ["sketch.constraint.parallel", "sketch.constraint.perpendicular", "sketch.constraint.equal"] {
        assert!(listed.contains(&id.to_string()), "{id} missing from {listed:?}");
    }
}
