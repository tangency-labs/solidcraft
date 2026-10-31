//! Select Other: press and hold, the viewport menu's entry, click cycling and dialog inputs; and
//! which of overlapping sketch curves a click picks.

use serde_json::{Value, json};
use solidcraft_engine::Sel;

use crate::scenario::Harness;

/// A sketch with two lines drawn on top of each other (l1, then l2), off the origin axes.
fn two_lines() -> (Harness, Value) {
    let mut h = Harness::new();
    h.app.set_origin_shown(false);
    h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    for _ in 0..2 {
        h.app.run("sketch.line", json!({"points":[[0,10],[20,10]],"infer":false,"connect":false})).unwrap();
    }
    h.app.cam.distance = 60.0;
    h.settle();
    // Curve tests pick away from MID; the centre is a distinct point target.
    let at = h.call("ui.at", json!({"sketch":[5,10]}))["result"].clone();
    (h, at)
}

/// Two short lines of the sketch's own (`a` on x 5–15, `b` on x 25–35) drawn along a projected
/// line (x 0–40, y 0, a box edge below the sketch plane): the ids of `a`, `b` and the projected
/// one, and screen points on `a`, on `b` and on the projected line alone.
struct OnProjected {
    h: Harness,
    a: String,
    b: String,
    projected: String,
    at_a: Value,
    at_b: Value,
    at_projected: Value,
}

fn own_lines_on_a_projected_line() -> OnProjected {
    let mut h = Harness::new();
    h.app.set_origin_shown(false);
    h.app.run("solid.box", json!({"length":40,"width":10,"height":10,"corner":[0,0,-20]})).unwrap();
    h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    h.app.run("sketch.project", json!({"refs":[{"edge":[20,0,-10]}]})).unwrap();
    for (x0, x1) in [(5, 15), (25, 35)] {
        h.app.run("sketch.line", json!({"points":[[x0,0],[x1,0]],"infer":false,"connect":false})).unwrap();
    }
    h.app.cam.distance = 100.0;
    h.settle();
    let sk = h.app.session.doc.sketch(h.app.session.active_sketch.unwrap()).unwrap();
    let projected: Vec<String> = sk.curves.iter().filter(|c| c.link.is_some()).map(|c| c.id.clone()).collect();
    let own: Vec<String> = sk.curves.iter().filter(|c| c.link.is_none()).map(|c| c.id.clone()).collect();
    assert_eq!((projected.len(), own.len()), (1, 2), "{:?}", sk.curves);
    let at = |h: &mut Harness, x: f64| h.call("ui.at", json!({"sketch":[x,0]}))["result"].clone();
    // Away from the midpoints (10, 30 and 20), which are picks of their own.
    let (at_a, at_b, at_projected) = (at(&mut h, 7.0), at(&mut h, 27.0), at(&mut h, 2.0));
    OnProjected { h, a: own[0].clone(), b: own[1].clone(), projected: projected[0].clone(), at_a, at_b, at_projected }
}

fn hovered_curve(h: &Harness) -> Option<String> {
    match &h.app.viewport.hover {
        Some(crate::viewport::Hit::SketchCurve { id, .. }) => Some(id.clone()),
        _ => None,
    }
}

fn labels(h: &Harness) -> Vec<String> {
    crate::select_other::describe(&h.app)["rows"].as_array().into_iter().flatten().filter_map(|r| r["label"].as_str().map(String::from)).collect()
}

fn curves(h: &Harness) -> Vec<String> {
    h.app.session.selection.iter().filter_map(|s| if let Sel::SketchCurve { id, .. } = s { Some(id.clone()) } else { None }).collect()
}

/// Two lines of the sketch's own apart from each other (`l1` on x 0–15, `l2` on x 25–40, at
/// y 10): screen points on each.
fn two_lines_apart() -> (Harness, Value, Value) {
    let mut h = Harness::new();
    h.app.set_origin_shown(false);
    h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    for (x0, x1) in [(0, 15), (25, 40)] {
        h.app.run("sketch.line", json!({"points":[[x0,10],[x1,10]],"infer":false,"connect":false})).unwrap();
    }
    h.app.cam.distance = 100.0;
    h.settle();
    let at = |h: &mut Harness, x: f64| h.call("ui.at", json!({"sketch":[x,10]}))["result"].clone();
    // Away from the midpoints (7.5 and 32.5), which are picks of their own.
    let (at_a, at_b) = (at(&mut h, 3.0), at(&mut h, 37.0));
    (h, at_a, at_b)
}

/// The open list's row of sketch curve `id`.
fn row_of(h: &Harness, id: &str) -> usize {
    let rows = crate::select_other::describe(&h.app)["rows"].clone();
    let sels: Vec<Sel> = rows.as_array().into_iter().flatten().filter_map(|r| serde_json::from_value(r["sel"].clone()).ok()).collect();
    sels.iter().position(|s| matches!(s, Sel::SketchCurve { id: x, .. } if x == id)).unwrap_or_else(|| panic!("no row for {id}: {rows}"))
}

fn sorted(mut v: Vec<String>) -> Vec<String> {
    v.sort();
    v
}

#[test]
fn holding_still_lists_both_coincident_lines_and_picks_the_one_underneath() {
    let (mut h, at) = two_lines();
    let before = h.app.session.doc.to_json();
    h.step(&json!({"hold": at})).unwrap();
    assert!(crate::select_other::is_open(&h.app), "a held press opens Select Other");
    let rows = labels(&h);
    assert!(rows.len() >= 2, "{rows:?}");
    assert_eq!(&rows[..2], ["Line 1", "Line 2"], "the sketch's own curves come first");
    // Letting go of the hold neither selected nor dragged anything.
    assert!(h.app.session.selection.is_empty(), "{:?}", h.app.session.selection);
    assert_eq!(h.app.session.doc.to_json(), before);
    // Hovering a row lights it up in the viewport.
    h.step(&json!({"move": {"handle": "select_other:1"}})).unwrap();
    assert!(matches!(&h.app.viewport.hover, Some(crate::viewport::Hit::SketchCurve { id, .. }) if id == "l2"), "{:?}", h.app.viewport.hover);
    h.step(&json!({"click": {"handle": "select_other:1"}})).unwrap();
    assert!(!crate::select_other::is_open(&h.app));
    assert_eq!(curves(&h), ["l2"]);
    // Shift-picking from the list adds, like a Shift-click.
    h.step(&json!({"hold": at})).unwrap();
    h.step(&json!({"click": {"handle": "select_other:0"}, "shift": true})).unwrap();
    let mut got = curves(&h);
    got.sort();
    assert_eq!(got, ["l1", "l2"]);
}

/// #6396: a regular line drawn on part of a construction line is the one hovered and picked
/// first, whichever was drawn first; clicking again (or Select Other) still reaches the
/// construction line. A construction line clearly nearer the cursor still wins.
#[test]
fn a_regular_line_on_a_construction_line_is_picked_before_it() {
    for construction_first in [true, false] {
        let mut h = Harness::new();
        h.app.set_origin_shown(false);
        h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
        let cons = json!({"points":[[0,10],[20,10]],"infer":false,"connect":false,"construction":true,"ids":["c"]});
        let line = json!({"points":[[5,10],[15,10]],"infer":false,"connect":false,"ids":["r"]});
        for p in if construction_first { [cons, line] } else { [line, cons] } {
            h.app.run("sketch.line", p).unwrap();
        }
        h.app.run("sketch.line", json!({"points":[[0,14],[20,14]],"infer":false,"connect":false,"construction":true,"ids":["near"]})).unwrap();
        h.app.cam.distance = 60.0;
        h.settle();
        // Away from the shared midpoint (10, 10), which is a pick of its own.
        let at = h.call("ui.at", json!({"sketch":[7,10]}))["result"].clone();
        h.step(&json!({"move": at})).unwrap();
        assert!(
            matches!(&h.app.viewport.hover, Some(crate::viewport::Hit::SketchCurve { id, .. }) if id == "r"),
            "construction first: {construction_first}: {:?}",
            h.app.viewport.hover
        );
        h.step(&json!({"click": at})).unwrap();
        assert_eq!(curves(&h), ["r"], "construction first: {construction_first}");
        h.frames(30);
        h.step(&json!({"click": at})).unwrap();
        assert_eq!(curves(&h), ["c"], "the next click reaches the construction line");
        // On the construction line alone, it is still the one picked.
        h.app.session.selection.clear();
        let on_near = h.call("ui.at", json!({"sketch":[7,14]}))["result"].clone();
        h.frames(30);
        h.step(&json!({"click": on_near})).unwrap();
        assert_eq!(curves(&h), ["near"]);
    }
}

#[test]
fn esc_and_a_click_elsewhere_close_the_list_without_selecting() {
    let (mut h, at) = two_lines();
    h.step(&json!({"hold": at})).unwrap();
    assert!(crate::select_other::is_open(&h.app));
    h.step(&json!({"key": "Escape"})).unwrap();
    assert!(!crate::select_other::is_open(&h.app));
    assert!(h.app.session.active_sketch.is_some(), "Esc only closed the list");
    h.step(&json!({"hold": at})).unwrap();
    h.step(&json!({"click": [at[0].as_f64().unwrap() + 200.0, at[1].as_f64().unwrap() + 150.0]})).unwrap();
    assert!(!crate::select_other::is_open(&h.app));
    assert!(h.app.session.selection.is_empty(), "{:?}", h.app.session.selection);
}

#[test]
fn clicking_again_on_the_same_spot_selects_the_next_thing_under_it() {
    let (mut h, at) = two_lines();
    h.step(&json!({"click": at})).unwrap();
    assert_eq!(curves(&h), ["l1"]);
    // A deliberate second click (not part of a double-click).
    h.frames(30);
    h.step(&json!({"click": at})).unwrap();
    assert_eq!(curves(&h), ["l2"]);
    h.frames(30);
    h.step(&json!({"click": at})).unwrap();
    assert_eq!(curves(&h), ["l1"], "it wraps around");
    // A click somewhere else starts over.
    let x = at[0].as_f64().unwrap();
    let y = at[1].as_f64().unwrap();
    h.frames(30);
    h.step(&json!({"click": [x + 40.0, y]})).unwrap();
    h.frames(30);
    h.step(&json!({"click": at})).unwrap();
    assert_eq!(curves(&h), ["l1"]);
}

#[test]
fn a_double_click_does_not_cycle() {
    let (mut h, at) = two_lines();
    h.step(&json!({"click": at, "double": true})).unwrap();
    // The double-click's chain starts from the line under the cursor.
    assert!(curves(&h).contains(&"l1".to_string()), "{:?}", h.app.session.selection);
}

#[test]
fn the_viewport_menu_offers_select_other() {
    let (mut h, at) = two_lines();
    h.step(&json!({"click": at, "button": "right"})).unwrap();
    let menu = h.call("ui.menu", json!({}))["result"].clone();
    let items: Vec<&str> = menu["items"].as_array().into_iter().flatten().filter_map(|i| i["id"].as_str()).collect();
    assert!(items.contains(&"ui.selectOther"), "{items:?}");
    let r = h.call("ui.menuPick", json!({"item": "ui.selectOther"}));
    assert_eq!(r["ok"], true, "{r}");
    assert!(labels(&h).starts_with(&["Line 1".to_string(), "Line 2".to_string()]), "{:?}", labels(&h));
    // The control channel picks rows too.
    let r = h.call("ui.selectOther", json!({"pick": 1}));
    assert_eq!(r["ok"], true, "{r}");
    assert_eq!(curves(&h), ["l2"]);
    // Over empty space there is nothing to offer.
    h.step(&json!({"click": [40, 400], "button": "right"})).unwrap();
    let menu = h.call("ui.menu", json!({}))["result"].clone();
    assert!(!menu["items"].as_array().into_iter().flatten().any(|i| i["id"] == "ui.selectOther"), "{menu}");
}

#[test]
fn overlapping_bodies_list_both_and_the_hidden_one_can_be_picked() {
    let mut h = Harness::new();
    h.app.set_origin_shown(false);
    h.app.run("solid.box", json!({"length":10,"width":10,"height":10,"body_name":"Inner"})).unwrap();
    h.app.run("solid.box", json!({"length":10,"width":10,"height":10,"corner":[5,5,5],"body_name":"Outer"})).unwrap();
    h.call("ui.view", json!({"view":"top"}));
    h.call("ui.view", json!({"view":"fit"}));
    h.step(&json!({"hold": {"world": [7, 7, 15]}})).unwrap();
    let rows = labels(&h);
    assert!(rows.iter().any(|r| r == "Body · Outer") && rows.iter().any(|r| r == "Body · Inner"), "{rows:?}");
    let faces = rows.iter().filter(|r| r.starts_with("Face ")).count();
    assert_eq!(faces, 2, "the nearest face of each body: {rows:?}");
    let i = rows.iter().position(|r| r == "Body · Inner").unwrap();
    h.call("ui.selectOther", json!({"pick": i}));
    assert_eq!(h.app.session.selection, [Sel::Body { name: "Inner".into() }]);
}

/// Origin plane names (#6491): the list names an origin plane as the preferences do, and picking
/// it still selects the plane by its id.
#[test]
fn origin_planes_are_listed_by_the_names_the_preferences_give_them() {
    let mut h = Harness::new();
    h.call("ui.view", json!({"view":"top"}));
    h.settle();
    h.step(&json!({"hold": {"plane": "XY"}})).unwrap();
    assert!(labels(&h).iter().any(|r| r == "XY Plane"), "{:?}", labels(&h));
    h.step(&json!({"key": "Escape"})).unwrap();
    h.call("ui.prefs", json!({"origin_plane_names": "top_front_right"}));
    h.step(&json!({"hold": {"plane": "XY"}})).unwrap();
    let rows = labels(&h);
    let row = rows.iter().position(|r| r == "Top Plane").unwrap_or_else(|| panic!("{rows:?}"));
    assert!(!rows.iter().any(|r| r == "XY Plane"), "{rows:?}");
    h.step(&json!({"click": {"handle": format!("select_other:{row}")}})).unwrap();
    assert_eq!(h.app.session.selection, vec![Sel::Plane { name: "XY".into() }]);
}

#[test]
fn long_names_stay_inside_the_popup_and_remain_available_on_hover() {
    let mut h = Harness::new();
    h.app.set_origin_shown(false);
    h.call("ui.resize", json!({"width":1000,"height":700}));
    let name = "Long assembly component ".repeat(12);
    h.app.run("solid.box", json!({"length":10,"width":10,"height":10,"body_name":name})).unwrap();
    h.call("ui.view", json!({"view":"top"}));
    h.call("ui.view", json!({"view":"fit"}));
    h.step(&json!({"hold": {"world": [5, 5, 10]}})).unwrap();
    let label = format!("Body · {name}");
    let row = labels(&h).iter().position(|r| r == &label).unwrap();
    let mut output = h.ctx.run_ui(egui::RawInput { screen_rect: Some(h.ctx.content_rect()), focused: true, ..Default::default() }, |ui| {
        h.app.logic(ui.ctx());
        h.app.ui(ui);
    });
    output.textures_delta.clear();
    let text = output
        .shapes
        .iter()
        .find_map(|s| match &s.shape {
            egui::Shape::Text(t) if t.galley.job.text == label => Some(t),
            _ => None,
        })
        .expect("the body label is painted");
    let popup = h.ctx.memory(|m| m.area_rect(egui::Id::new("sc_select_other"))).unwrap();
    let text_rect = egui::Rect::from_min_size(text.pos, text.galley.size());
    assert!(popup.contains_rect(text_rect), "{text_rect:?} spills outside {popup:?}");
    assert!(h.ctx.content_rect().contains_rect(popup));
    assert!(text.galley.elided, "long names end in an ellipsis");

    h.step(&json!({"move": {"handle": format!("select_other:{row}")}})).unwrap();
    h.frames(90);
    assert!(h.widget(&label).is_some(), "the tooltip exposes the full label");
    h.step(&json!({"click": {"handle": format!("select_other:{row}")}})).unwrap();
    assert_eq!(h.app.session.selection, [Sel::Body { name }], "the shortened label still selects the original body");
}

#[test]
fn a_dialog_selection_input_takes_its_pick_from_the_list() {
    let mut h = Harness::new();
    h.app.set_origin_shown(false);
    h.app.run("solid.box", json!({"length":10,"width":10,"height":10,"body_name":"Block"})).unwrap();
    h.call("ui.view", json!({"view":"home"}));
    h.call("ui.view", json!({"view":"fit"}));
    h.step(&json!({"start": "solid.fillet"})).unwrap();
    h.frames(2);
    h.step(&json!({"hold": {"world": [5, 0, 10]}})).unwrap();
    let rows = crate::select_other::describe(&h.app)["rows"].clone();
    let sels: Vec<&Value> = rows.as_array().into_iter().flatten().map(|r| &r["sel"]).collect();
    assert!(!sels.is_empty(), "{rows}");
    // The edge in front, then the face behind it; no bodies, which the fillet doesn't take.
    assert_eq!(sels[0]["type"], "edge", "{rows}");
    assert!(sels.iter().all(|s| s["type"] == "edge" || s["type"] == "face"), "only what the fillet's input accepts: {rows}");
    h.call("ui.selectOther", json!({"pick": 0}));
    let d = h.app.dialog.as_ref().expect("the fillet dialog stays open");
    assert!(d.inputs.iter().any(|i| i.items.iter().any(|x| matches!(x, Sel::Edge { .. }))), "{:?}", d.inputs);
    assert!(h.app.session.selection.is_empty(), "the pick went to the dialog, not the selection");
}

#[test]
fn rows_name_points_by_their_curves() {
    let mut h = Harness::new();
    h.app.set_origin_shown(false);
    h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    h.app.run("sketch.circle.center", json!({"center":[10,10],"radius":3})).unwrap();
    h.app.cam.distance = 60.0;
    h.settle();
    let at = h.call("ui.at", json!({"sketch":[10,10]}))["result"].clone();
    h.call("ui.selectOther", json!({"x": at[0], "y": at[1]}));
    let rows = labels(&h);
    assert!(rows.iter().any(|r| r.starts_with("Point (") && r.contains("centre")), "{rows:?}");
}

#[test]
fn a_dense_spot_is_capped_per_kind_and_reports_the_total() {
    let mut h = Harness::new();
    h.app.set_origin_shown(false);
    h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    // Thirty overlapping lines and their MID targets each exceed the per-kind cap.
    for _ in 0..30 {
        h.app.run("sketch.line", json!({"points":[[0,10],[20,10]],"infer":false,"connect":false})).unwrap();
    }
    h.app.cam.distance = 60.0;
    h.settle();
    let at = h.call("ui.at", json!({"sketch":[10,10]}))["result"].clone();
    h.step(&json!({"hold": at})).unwrap();
    assert!(crate::select_other::is_open(&h.app));
    let d = crate::select_other::describe(&h.app);
    let rows = d["rows"].as_array().expect("rows");
    let lines = rows.iter().filter(|r| r["label"].as_str().is_some_and(|l| l.starts_with("Line "))).count();
    assert_eq!(lines, 20, "at most 20 rows of one kind, nearest first: {rows:?}");
    let mids = rows.iter().filter(|r| r["sel"]["type"] == "sketch_midpoint").count();
    assert_eq!(mids, 20, "MID targets have the same per-kind bound");
    assert_eq!(rows[0]["label"], json!("MID · Line 1"), "the nearest point target leads");
    let total = d["total"].as_u64().expect("total");
    assert!(total >= 60, "the total counts every line and MID candidate, not just the rows: {total}");
    assert!((total as usize) < rows.len() + 30, "the list is truncated: {total} total, {} rows", rows.len());
}

#[test]
fn overlapping_midpoints_can_be_selected_individually_without_creating_points() {
    let (mut h, _) = two_lines();
    let at = h.call("ui.at", json!({"sketch":[10,10]}))["result"].clone();
    let before = h.app.session.doc.to_json();
    let undo = h.app.session.history.len();
    h.step(&json!({"hold":at})).unwrap();
    assert!(labels(&h).starts_with(&["MID · Line 1".into(), "MID · Line 2".into(), "Line 1".into(), "Line 2".into()]));
    h.step(&json!({"click":{"handle":"select_other:1"}})).unwrap();
    assert_eq!(h.app.session.selection, vec![Sel::SketchMidpoint { id: "l2".into() }]);
    h.step(&json!({"hold":at})).unwrap();
    h.step(&json!({"click":{"handle":"select_other:0"},"shift":true})).unwrap();
    assert_eq!(h.app.session.selection, vec![Sel::SketchMidpoint { id: "l2".into() }, Sel::SketchMidpoint { id: "l1".into() }]);
    assert_eq!(h.app.session.doc.to_json(), before);
    assert_eq!(h.app.session.history.len(), undo);
}

#[test]
fn select_other_opened_with_shift_adds_its_choice_to_the_selection() {
    // From the viewport menu, opened by a Shift right-click: the right-click's own pick doesn't
    // stay, and the choice adds without Shift still held.
    let (mut h, at_a, at_b) = two_lines_apart();
    h.step(&json!({"click": at_a})).unwrap();
    assert_eq!(curves(&h), ["l1"]);
    h.frames(30);
    h.step(&json!({"click": at_b, "button": "right", "shift": true})).unwrap();
    let r = h.call("ui.menuPick", json!({"item": "ui.selectOther"}));
    assert_eq!(r["ok"], true, "{r}");
    assert_eq!(curves(&h), ["l1"], "opening the list put the selection back");
    let i = row_of(&h, "l2");
    h.step(&json!({"click": {"handle": format!("select_other:{i}")}})).unwrap();
    assert_eq!(sorted(curves(&h)), ["l1", "l2"]);
    // By press and hold with Shift.
    h.frames(30);
    h.step(&json!({"click": at_a})).unwrap();
    assert_eq!(curves(&h), ["l1"]);
    h.frames(30);
    h.step(&json!({"hold": at_b, "shift": true})).unwrap();
    assert!(crate::select_other::is_open(&h.app));
    let i = row_of(&h, "l2");
    h.step(&json!({"click": {"handle": format!("select_other:{i}")}})).unwrap();
    assert_eq!(sorted(curves(&h)), ["l1", "l2"]);
}

#[test]
fn a_click_picks_the_sketchs_own_curve_over_a_projected_one_under_it() {
    let OnProjected { mut h, a, b, projected, at_a, at_b, at_projected } = own_lines_on_a_projected_line();
    h.step(&json!({"move": at_a})).unwrap();
    assert_eq!(hovered_curve(&h).as_ref(), Some(&a), "the hover shows what a click picks");
    h.step(&json!({"click": at_a})).unwrap();
    assert_eq!(curves(&h), std::slice::from_ref(&a));
    h.frames(30);
    h.step(&json!({"click": at_b, "shift": true})).unwrap();
    assert_eq!(sorted(curves(&h)), sorted(vec![a.clone(), b.clone()]));
    // Where nothing of the sketch's own is, the projected line is picked.
    h.frames(30);
    h.step(&json!({"move": at_projected})).unwrap();
    assert_eq!(hovered_curve(&h).as_ref(), Some(&projected));
    h.step(&json!({"click": at_projected})).unwrap();
    assert_eq!(curves(&h), std::slice::from_ref(&projected));
    // Select Other lists the sketch's own line first, and the projected one under it.
    h.frames(30);
    h.step(&json!({"hold": at_a})).unwrap();
    assert!(row_of(&h, &a) < row_of(&h, &projected), "{:?}", labels(&h));
    h.step(&json!({"click": {"handle": format!("select_other:{}", row_of(&h, &projected))}})).unwrap();
    assert_eq!(curves(&h), [projected]);
}

/// Three plates stacked along Z (made bottom, top, middle, so model order isn't depth order),
/// the top one smallest, held over the top plate's edge from above.
fn stacked_plates() -> Harness {
    let mut h = Harness::new();
    h.app.set_origin_shown(false);
    h.app.run("solid.box", json!({"length":30,"width":30,"height":2,"corner":[-15,-15,0],"body_name":"Bottom"})).unwrap();
    h.app.run("solid.box", json!({"length":10,"width":10,"height":2,"corner":[-5,-5,20],"body_name":"Top"})).unwrap();
    h.app.run("solid.box", json!({"length":20,"width":20,"height":2,"corner":[-10,-10,10],"body_name":"Middle"})).unwrap();
    h.call("ui.view", json!({"view":"top"}));
    h.call("ui.view", json!({"view":"fit"}));
    h
}

#[test]
fn stacked_bodies_list_front_to_back_one_body_at_a_time() {
    let mut h = stacked_plates();
    h.step(&json!({"hold": {"world": [4.9, 0, 22]}})).unwrap();
    let rows = labels(&h);
    // Each row's body, in list order: one body at a time, the nearest first (#5345).
    let bodies: Vec<&str> = rows.iter().filter_map(|r| r.rsplit(" · ").next()).collect();
    let mut runs = bodies.clone();
    runs.dedup();
    assert_eq!(runs, ["Top", "Middle", "Bottom"], "front to back, each body's rows together: {rows:?}");
    // Within a body: the edge under the pointer, then its face, then the body itself.
    assert!(rows[0].starts_with("Edge ") && rows[0].ends_with("· Top"), "{rows:?}");
    for body in ["Top", "Middle", "Bottom"] {
        let mine: Vec<&String> = rows.iter().filter(|r| r.ends_with(&format!("· {body}"))).collect();
        assert!(mine.iter().any(|r| r.starts_with("Face ")), "{body}: {rows:?}");
        assert_eq!(mine.last().map(|r| r.as_str()), Some(format!("Body · {body}").as_str()), "{rows:?}");
    }
}

#[test]
fn an_edge_is_as_deep_as_its_point_under_the_pointer_not_its_middle() {
    let mut h = stacked_plates();
    h.call("ui.view", json!({"view":"iso"}));
    h.call("ui.view", json!({"view":"fit"}));
    h.settle();
    // On the bottom plate's near top edge along Y (x = 15, y from -15 to 15), well off its middle.
    let under = solidcraft_engine::geom::Vec3::new(15.0, 9.0, 2.0);
    let at = h.call("ui.at", json!({"world": [under.x, under.y, under.z]}))["result"].clone();
    let pos = egui::pos2(at[0].as_f64().unwrap() as f32, at[1].as_f64().unwrap() as f32);
    let proj = crate::viewport::projection(&h.app, h.app.viewport.rect.unwrap());
    let hits = crate::viewport::pick_all(&h.app, &proj, pos);
    let edge = hits.iter().find(|x| matches!(x, crate::viewport::Hit::Edge { body, .. } if body == "Bottom")).expect("the edge");
    let (o, d) = proj.ray(pos);
    let st = crate::open_meshing::shown_world(&h.app);
    let index = st.bodies.iter().enumerate().map(|(i, b)| (b.name.as_str(), i)).collect();
    let t = super::hit_depth(&st, &index, o, d, edge).expect("a depth");
    let want = (under - o).dot(d);
    let mid = solidcraft_engine::geom::Vec3::new(15.0, 0.0, 2.0);
    assert!((t - want).abs() < 0.5, "depth {t}, the point under the pointer {want}, its middle {}", (mid - o).dot(d));
}

/// The navigation bar's Orbit, Pan and Zoom: each one's slot and the mode it lights.
fn lit_modes() -> [(&'static str, crate::viewport::NavMode); 3] {
    use crate::viewport::NavMode;
    [("orbit", NavMode::Orbit), ("pan", NavMode::Pan), ("zoom", NavMode::Zoom)]
}

/// Screen point `at` moved by `d` points right and down.
fn offset(at: &Value, d: f64) -> Value {
    json!([at[0].as_f64().unwrap_or_default() + d, at[1].as_f64().unwrap_or_default() + d])
}

/// A mode lit in the navigation bar owns left drags, not a press held still: the hold opens
/// the list as it does with no mode lit, and a plain click still selects.
#[test]
fn holding_still_opens_the_list_while_a_navigation_mode_is_lit() {
    for (slot, mode) in lit_modes() {
        let (mut h, at) = two_lines();
        h.step(&json!({"click": {"handle": format!("nav:slot:{slot}")}})).unwrap();
        assert_eq!(h.app.viewport.nav, Some(mode), "{slot} is lit");
        let cam = h.app.cam;
        h.step(&json!({"hold": at})).unwrap();
        assert!(crate::select_other::is_open(&h.app), "a held press opens Select Other with {slot} lit");
        assert_eq!(h.app.cam, cam, "{slot}: holding still did not move the view");
        assert!(h.app.session.selection.is_empty(), "{slot}: {:?}", h.app.session.selection);
        h.step(&json!({"click": {"handle": "select_other:1"}})).unwrap();
        assert_eq!(curves(&h), ["l2"], "{slot}");
        h.app.session.selection.clear();
        h.step(&json!({"click": at})).unwrap();
        assert_eq!(curves(&h).len(), 1, "{slot}: a click still selects");
        assert_eq!(h.app.viewport.nav, Some(mode), "{slot} stays lit");
    }
}

/// A press held still on a line keeps the line lit until the list opens, with Orbit, Pan or
/// Zoom lit as with none: the press is not navigating yet (#8864).
#[test]
fn a_press_held_still_keeps_the_highlight_while_a_navigation_mode_is_lit() {
    for slot in [None, Some("orbit"), Some("pan"), Some("zoom")] {
        let (mut h, at) = two_lines();
        if let Some(slot) = slot {
            h.step(&json!({"click": {"handle": format!("nav:slot:{slot}")}})).unwrap();
        }
        h.step(&json!({"move": at})).unwrap();
        h.frames(2);
        assert!(hovered_curve(&h).is_some(), "{slot:?}: the line under the pointer is lit");
        let pos = egui::pos2(at[0].as_f64().unwrap_or_default() as f32, at[1].as_f64().unwrap_or_default() as f32);
        let button = |pressed| egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() };
        h.app.synthetic.push(button(true));
        h.frames(10);
        assert!(!crate::select_other::is_open(&h.app), "{slot:?}: not open yet");
        assert!(hovered_curve(&h).is_some(), "{slot:?}: still lit while the press is held");
        h.frames(40);
        assert!(crate::select_other::is_open(&h.app), "{slot:?}: the hold opened the list");
        h.app.synthetic.push(button(false));
        h.frames(2);
    }
}

/// A drag under a lit mode still moves the view and does not open the list, whether it starts
/// at once or after the press was held still for longer than egui waits to call it a drag.
#[test]
fn a_drag_under_a_lit_navigation_mode_moves_the_view_and_opens_nothing() {
    for (slot, _) in lit_modes() {
        let (mut h, at) = two_lines();
        h.step(&json!({"click": {"handle": format!("nav:slot:{slot}")}})).unwrap();
        let cam = h.app.cam;
        h.step(&json!({"drag": [at, offset(&at, 40.0)]})).unwrap();
        assert_ne!(h.app.cam, cam, "{slot}: the drag moved the view");
        assert!(!crate::select_other::is_open(&h.app), "{slot}: a drag does not open Select Other");
        assert!(h.app.session.selection.is_empty(), "{slot}: {:?}", h.app.session.selection);
    }
}

/// A preset's modifier chord is a gesture to navigate from the press on: Maya's Alt+left held
/// still on overlapping lines does not open the list, unlike a mode lit in the navigation bar.
#[test]
fn a_preset_chord_held_still_does_not_open_the_list() {
    let (mut h, at) = two_lines();
    h.app.preferences.nav = "maya".into();
    let (x, y) = (at[0].as_f64().unwrap_or_default(), at[1].as_f64().unwrap_or_default());
    h.call("ui.drag", json!({"x0": x, "y0": y, "x1": x, "y1": y, "steps": 1, "hold": 40, "alt": true}));
    h.settle();
    assert!(!crate::select_other::is_open(&h.app), "Maya Alt+left held still navigates, it does not open Select Other");
}

/// Once the list is open, the press it came from does not turn into a drag of the view at
/// egui's 0.8 s mark: the camera stays put under the open list while the pointer moves to a row.
#[test]
fn a_press_that_opened_the_list_does_not_move_the_view_when_it_moves_on() {
    for (slot, _) in lit_modes() {
        let (mut h, at) = two_lines();
        h.step(&json!({"click": {"handle": format!("nav:slot:{slot}")}})).unwrap();
        let cam = h.app.cam;
        let (x, y) = (at[0].as_f64().unwrap_or_default(), at[1].as_f64().unwrap_or_default());
        h.call("ui.drag", json!({"x0": x, "y0": y, "x1": x + 40.0, "y1": y + 40.0, "steps": 8, "hold": 60}));
        h.settle();
        assert!(crate::select_other::is_open(&h.app), "{slot}: the list stays open");
        assert_eq!(h.app.cam, cam, "{slot}: the view did not move under the open list");
    }
}

/// Zooming out does not make a click reach through the body. The pixel of the box's hidden far
/// corner is on the near +X face, so every distance picks something standing on that face —
/// the face, or one of its edges or corners once the box is small on screen — never the far
/// side behind it. The occlusion slack used to be cut from the camera's distance, so far
/// enough out the whole body fitted inside it.
#[test]
fn a_zoomed_out_click_picks_the_near_face_not_an_edge_through_the_body() {
    let mut h = Harness::new();
    h.app.set_origin_shown(false);
    h.app.run("solid.box", json!({"length":10,"width":10,"height":10,"corner":[-5,-5,-5]})).unwrap();
    for distance in [40.0, 300.0, 1000.0, 3000.0] {
        h.app.cam.distance = distance;
        h.settle();
        let at = h.at(&json!({"world": [0, 5, -5]})).unwrap();
        let proj = crate::viewport::projection(&h.app, h.app.viewport.rect.unwrap());
        let hits = crate::viewport::pick(&h.app, &proj, egui::pos2(at[0] as f32, at[1] as f32));
        let first = hits.first().expect("the box is under the cursor");
        let point = match first {
            crate::viewport::Hit::Face { point, .. } | crate::viewport::Hit::Vertex { point, .. } => *point,
            crate::viewport::Hit::Edge { mid, .. } => *mid,
            other => panic!("at {distance} mm away the box is not picked: {other:?}"),
        };
        assert!(point.x > 4.9, "at {distance} mm away the pick is behind the near face: {first:?}");
    }
}

/// A small part on a large one is picked by its own size, not the assembly's: the 2 mm box's
/// far corner is 2 mm behind its near face, and a slack cut from the 200 mm plate (or from the
/// camera) reached straight through it — the kraken board's 2.54 mm connectors picked their far
/// side from 5 mm behind.
#[test]
fn a_small_body_on_a_large_one_is_not_picked_through() {
    let mut h = Harness::new();
    h.app.set_origin_shown(false);
    h.app.run("solid.box", json!({"length":200,"width":200,"height":10,"corner":[-100,-100,-10]})).unwrap();
    h.app.run("solid.box", json!({"length":2,"width":2,"height":2,"corner":[-1,-1,0]})).unwrap();
    for distance in [20.0, 100.0, 600.0] {
        h.app.cam.distance = distance;
        h.settle();
        // The small box's far bottom corner, whose pixel is on its near +X face.
        let at = h.at(&json!({"world": [0, 1, 0]})).unwrap();
        let proj = crate::viewport::projection(&h.app, h.app.viewport.rect.unwrap());
        let hits = crate::viewport::pick(&h.app, &proj, egui::pos2(at[0] as f32, at[1] as f32));
        let first = hits.first().expect("the small box is under the cursor");
        let point = match first {
            crate::viewport::Hit::Face { point, .. } | crate::viewport::Hit::Vertex { point, .. } => *point,
            crate::viewport::Hit::Edge { mid, .. } => *mid,
            other => panic!("at {distance} mm away the small box is not picked: {other:?}"),
        };
        assert!(point.x > 0.9, "at {distance} mm away the pick is behind the near face: {first:?}");
    }
}

#[test]
fn a_curve_from_intersect_is_labelled_intersected_and_one_from_project_projected() {
    use solidcraft_engine::sketch::LinkKind;
    let mut h = Harness::new();
    h.app.run("solid.box", json!({"length":40,"width":10,"height":10,"body_name":"Block"})).unwrap();
    let label = |h: &Harness, kind: LinkKind| {
        let sk = h.app.session.doc.sketch(h.app.session.active_sketch.unwrap()).unwrap();
        let c = sk.curves.iter().find(|c| c.link.as_deref().and_then(|l| sk.link(l)).is_some_and(|l| l.kind == kind)).unwrap();
        crate::select_other::curve_label(sk, &c.id)
    };
    h.app.run("sketch.create", json!({"plane":"XY","offset":5,"project_edges":false})).unwrap();
    h.app.run("sketch.intersect", json!({"refs":[{"body":"Block"}]})).unwrap();
    assert!(label(&h, LinkKind::Intersect).ends_with("(intersected)"));
    h.app.run("sketch.finish", json!({})).unwrap();
    h.app.run("sketch.create", json!({"plane":"XY","offset":20,"project_edges":false})).unwrap();
    h.app.run("sketch.project", json!({"refs":[{"body":"Block"}]})).unwrap();
    assert!(label(&h, LinkKind::Project).ends_with("(projected)"));
}
#[test]
fn long_press_picks_hidden_profiles_behind_a_body_without_revealing_them() {
    let mut h = Harness::new();
    h.app.set_origin_shown(false);
    h.app.run("sketch.create", json!({"plane":"XY","name":"Buried profile"})).unwrap();
    h.app.run("sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[10,10]})).unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
    let sketch = h.app.session.doc.find_feature("Buried profile").unwrap().id;
    h.app.run("solid.box", json!({"length":20,"width":20,"height":10,"corner":[-5,-5,1],"body_name":"Cover"})).unwrap();
    h.call("ui.view", json!({"view":"top"}));
    h.call("ui.view", json!({"view":"fit"}));
    h.app.session.visibility.hidden_sketches.push(sketch);
    h.app.start("solid.extrude");
    h.app.dialog.as_mut().unwrap().inputs[0].items.clear();
    h.settle();
    assert_eq!(h.call("ui.prefs", json!({}))["result"]["select_other_hidden"], true);
    assert_eq!(h.call("ui.prefs", json!({"select_other_hidden":false}))["ok"], true);
    h.step(&json!({"hold":{"world":[5,5,0]}})).unwrap();
    assert!(!h.app.viewport.select_other.popup.as_ref().unwrap().rows.iter().any(|r| matches!(r.sel, Sel::Profile { .. })));
    h.call("ui.selectOther", json!({"close":true}));
    assert_eq!(h.call("ui.prefs", json!({"select_other_hidden":true}))["ok"], true);
    let before = h.app.session.visibility.hidden_sketches.clone();
    h.step(&json!({"hold":{"world":[5,5,0]}})).unwrap();
    let popup = h.app.viewport.select_other.popup.as_ref().unwrap();
    let row = popup.rows.iter().position(|r| r.sel == Sel::Profile { sketch, index: 0 }).unwrap();
    h.call("ui.selectOther", json!({"pick":row}));
    assert_eq!(h.app.dialog.as_ref().unwrap().inputs[0].items, [Sel::Profile { sketch, index: 0 }]);
    assert_eq!(h.app.session.visibility.hidden_sketches, before);
    assert!(!crate::viewport::visible_sketches(&h.app).contains(&sketch));
    assert!(h.app.session.selection.is_empty());
}

#[test]
fn see_through_includes_hidden_bodies_and_occluded_edges_but_normal_picks_do_not() {
    let mut h = Harness::new();
    h.app.set_origin_shown(false);
    h.app.run("solid.box", json!({"length":10,"width":10,"height":10,"body_name":"Hidden"})).unwrap();
    h.app.run("solid.box", json!({"length":30,"width":30,"height":10,"corner":[-10,-10,20],"body_name":"Cover"})).unwrap();
    h.call("ui.view", json!({"view":"top"}));
    h.call("ui.view", json!({"view":"fit"}));
    let point = h.at(&json!({"world":[5,0,10]})).unwrap();
    let pos = egui::pos2(point[0] as f32, point[1] as f32);
    let proj = crate::viewport::projection(&h.app, h.app.viewport.rect.unwrap());
    let edge = |hit: &crate::viewport::Hit| matches!(hit, crate::viewport::Hit::Edge { body, .. } if body == "Hidden");
    assert!(crate::viewport::pick_through(&h.app, &proj, pos).iter().any(edge));
    assert!(!crate::viewport::pick_all(&h.app, &proj, pos).iter().any(edge));
    h.app.session.visibility.hidden_bodies.push("Hidden".into());
    let hidden = h.app.session.visibility.hidden_bodies.clone();
    h.call("ui.prefs", json!({"select_other_hidden":false}));
    h.call("ui.selectOther", json!({"x":pos.x,"y":pos.y}));
    assert!(!h.app.viewport.select_other.popup.as_ref().unwrap().rows.iter().any(|r| r.sel == Sel::Body { name: "Hidden".into() }));
    h.call("ui.selectOther", json!({"close":true}));
    h.call("ui.prefs", json!({"select_other_hidden":true}));
    h.call("ui.selectOther", json!({"x":pos.x,"y":pos.y}));
    let i = h.app.viewport.select_other.popup.as_ref().unwrap().rows.iter().position(|r| r.sel == Sel::Body { name: "Hidden".into() }).unwrap();
    h.call("ui.selectOther", json!({"pick":i}));
    assert_eq!(h.app.session.selection, [Sel::Body { name: "Hidden".into() }]);
    assert_eq!(h.app.session.visibility.hidden_bodies, hidden);
    assert!(
        !crate::viewport::pick_all(&h.app, &proj, pos).iter().any(|hit| matches!(hit, crate::viewport::Hit::Face { body, .. } if body == "Hidden"))
    );
}

#[test]
fn approximate_long_click_reaches_profiles_and_shows_all_eight_rows() {
    let mut h = Harness::new();
    h.app.set_origin_shown(false);
    let mut sketches = Vec::new();
    for i in 0..8 {
        h.app.run("sketch.create", json!({"plane":"XY","name":format!("Layer{i}")})).unwrap();
        h.app.run("sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[10,10]})).unwrap();
        h.app.run("sketch.finish", json!({})).unwrap();
        sketches.push(h.app.session.doc.find_feature(&format!("Layer{i}")).unwrap().id);
    }
    h.call("ui.view", json!({"view":"top"}));
    h.call("ui.view", json!({"view":"fit"}));
    h.app.session.visibility.hidden_sketches = sketches.clone();
    h.app.start("solid.extrude");
    h.app.dialog.as_mut().unwrap().inputs[0].items.clear();
    h.settle();
    let at = h.at(&json!({"world":[10,5,0]})).unwrap();
    let pos = egui::pos2(at[0] as f32 + 3.0, at[1] as f32);
    let proj = crate::viewport::projection(&h.app, h.app.viewport.rect.unwrap());
    assert!(crate::viewport::pick_all(&h.app, &proj, pos).iter().all(|x| !matches!(x, crate::viewport::Hit::Profile { .. })));
    h.step(&json!({"hold":[pos.x,pos.y]})).unwrap();
    assert_eq!(h.app.viewport.select_other.popup.as_ref().unwrap().rows.len(), 8);
    h.frames(3);
    if let Ok(dir) = std::env::var("SOLIDCRAFT_SELECT_OTHER_EVIDENCE") {
        let ctx = crate::fonts::test_context();
        let mut textures = std::collections::HashMap::new();
        let mut last = None;
        for _ in 0..4 {
            let output = ctx.run_ui(
                egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1600.0, 1000.0))), ..Default::default() },
                |ui| super::show(&mut h.app, ui.ctx()),
            );
            crate::ui_kit::test_textures_update(&mut textures, &output.textures_delta);
            last = Some(output);
        }
        let output = last.unwrap();
        let rect = crate::scenario::rect_of("select_other").unwrap();
        std::fs::create_dir_all(&dir).unwrap();
        crate::ui_kit::test_raster(&ctx, output.shapes, output.pixels_per_point, rect.expand(4.0), &textures)
            .save(std::path::Path::new(&dir).join("select-other.png"))
            .unwrap();
        h.frames(4);
    }
    let rect = crate::scenario::rect_of("select_other").unwrap();
    assert!(rect.height() > 8.0 * super::ROW_HEIGHT, "all eight rows plus header: {rect:?}");
    for i in 0..8 {
        assert!(rect.contains(crate::scenario::handle_at(&format!("select_other:{i}")).unwrap()), "row {i}: {rect:?}");
    }
    h.step(&json!({"click":{"handle":"select_other:7"}})).unwrap();
    assert_eq!(h.app.dialog.as_ref().unwrap().inputs[0].items, [Sel::Profile { sketch: sketches[7], index: 0 }]);
    h.app.session.visibility.hidden_sketches.clear();
    h.call("ui.prefs", json!({"select_other_hidden":false}));
    assert!(!super::rows(&h.app, &proj, pos).iter().any(|r| matches!(r.sel, Sel::Profile { .. })), "old list uses the exact pointer");
    h.call("ui.prefs", json!({"select_other_hidden":true}));
    assert_eq!(super::rows(&h.app, &proj, pos).iter().filter(|r| matches!(r.sel, Sel::Profile { .. })).count(), 8);
    h.app.session.visibility.hidden_sketches = sketches.clone();
}

#[test]
fn see_through_control_rejects_out_of_range_coordinates() {
    let mut h = Harness::new();
    h.settle();
    for x in [1e300, -1e300, 1e7] {
        let reply = h.call("ui.selectOther", json!({"x":x,"y":10}));
        assert_eq!(reply["ok"], false, "{reply}");
    }
}

#[test]
fn popup_uses_window_height_and_flips_above_the_pointer() {
    for (count, y, height) in [(20, 100.0, 1000), (20, 900.0, 1000), (20, 500.0, 1000), (60, 500.0, 1000), (20, 240.0, 480)] {
        let (mut h, at) = two_lines();
        h.step(&json!({"hold":at})).unwrap();
        let popup = h.app.viewport.select_other.popup.as_mut().unwrap();
        popup.rows = vec![popup.rows[0].clone(); count];
        popup.total = count;
        popup.at.y = y;
        h.call("ui.resize", json!({"width":1600,"height":height}));
        h.frames(4);
        let rect = crate::scenario::rect_of("select_other").unwrap();
        assert!(h.ctx.content_rect().contains_rect(rect), "{count} rows: {rect:?}");
        if count == 20 && height == 1000 {
            assert!(rect.height() > 360.0, "large window shows more than the old fixed cap: {rect:?}");
            assert!(rect.contains(crate::scenario::handle_at("select_other:19").unwrap()), "last row fits: {rect:?}");
            if y > 500.0 {
                assert!(rect.bottom() <= y, "opens above the pointer: {rect:?}");
            } else if y < 500.0 {
                assert!(rect.top() > y, "opens below the pointer: {rect:?}");
            }
        } else {
            assert!(!rect.contains(crate::scenario::handle_at(&format!("select_other:{}", count - 1)).unwrap()), "overflow needs scrolling");
            h.app.synthetic.push(egui::Event::PointerMoved(rect.center()));
            h.app.synthetic.push(egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: egui::vec2(0.0, -2000.0),
                phase: egui::TouchPhase::Move,
                modifiers: egui::Modifiers::NONE,
            });
            h.frames(40);
            assert!(rect.contains(crate::scenario::handle_at(&format!("select_other:{}", count - 1)).unwrap()), "can scroll to the last row");
        }
    }
}

#[test]
fn hidden_selection_preference_is_saved_and_older_settings_get_the_new_default() {
    let prefs: crate::prefs::Prefs = serde_json::from_value(json!({})).unwrap();
    assert_eq!(serde_json::to_value(&prefs).unwrap()["select_other_hidden"], true);
    let prefs: crate::prefs::Prefs = serde_json::from_value(json!({"select_other_hidden":false})).unwrap();
    assert_eq!(serde_json::to_value(&prefs).unwrap()["select_other_hidden"], false);
}

#[test]
fn hidden_selection_preference_can_be_changed_and_cancelled_in_preferences() {
    let mut h = Harness::new();
    h.call("ui.prefs", json!({"open":"Sketch & selection"}));
    crate::prefs::reveal_preference(&mut h, "prefs:select_other_hidden");
    h.step(&json!({"click":{"handle":"prefs:select_other_hidden"}})).unwrap();
    assert_eq!(h.call("ui.prefs", json!({}))["result"]["select_other_hidden"], false);
    h.call("ui.prefs", json!({"button":"cancel"}));
    assert_eq!(h.call("ui.prefs", json!({}))["result"]["select_other_hidden"], true);
}

#[test]
fn changing_hidden_selection_or_opening_an_empty_point_clears_the_old_list() {
    let mut h = Harness::new();
    h.app.set_origin_shown(false);
    h.app.run("solid.box", json!({"length":10,"width":10,"height":10,"body_name":"Hidden"})).unwrap();
    h.call("ui.view", json!({"view":"top"}));
    h.call("ui.view", json!({"view":"fit"}));
    let at = h.at(&json!({"world":[5,5,10]})).unwrap();
    let point = json!({"x":at[0],"y":at[1]});
    h.app.session.selection.clear();
    h.app.session.visibility.hidden_bodies.push("Hidden".into());
    h.call("ui.selectOther", point.clone());
    assert!(super::is_open(&h.app));
    h.call("ui.prefs", json!({"select_other_hidden":false}));
    assert!(!super::is_open(&h.app), "old hidden rows close when the preference changes");
    assert_eq!(h.call("ui.selectOther", point.clone())["result"]["open"], false);
    assert_eq!(h.call("ui.selectOther", json!({"pick":0}))["ok"], false);
    assert!(h.app.session.selection.is_empty());
    h.call("ui.prefs", json!({"select_other_hidden":true}));
    h.call("ui.selectOther", point.clone());
    h.app.preferences.select_other_hidden = false;
    h.frames(2);
    assert!(!super::is_open(&h.app), "direct preference changes also close stale rows");
    h.app.session.visibility.hidden_bodies.clear();
    h.call("ui.selectOther", point);
    assert!(super::is_open(&h.app));
    let rect = h.app.viewport.rect.unwrap();
    assert_eq!(h.call("ui.selectOther", json!({"x":rect.left()+20.0,"y":rect.top()+20.0}))["result"]["open"], false);
    assert!(!super::is_open(&h.app), "an empty point cannot keep the preceding list");
}
