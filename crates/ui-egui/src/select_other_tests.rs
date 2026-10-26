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
    assert_eq!(faces, 4, "the front and back faces of each body: {rows:?}");
    let i = rows.iter().position(|r| r == "Body · Inner").unwrap();
    h.call("ui.selectOther", json!({"pick": i}));
    assert_eq!(h.app.session.selection, [Sel::Body { name: "Inner".into() }]);
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
    // This extra paint pass must stay on the harness clock; egui's wall clock can
    // otherwise run a fraction ahead of the next synthetic pointer event.
    let mut output = h.ctx.run_ui(
        egui::RawInput { screen_rect: Some(h.ctx.content_rect()), time: Some(h.ctx.input(|i| i.time)), focused: true, ..Default::default() },
        |ui| {
            h.app.logic(ui.ctx());
            h.app.ui(ui);
        },
    );
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
    let undo = h.app.session.undo.len();
    h.step(&json!({"hold":at})).unwrap();
    assert!(labels(&h).starts_with(&["MID · Line 1".into(), "MID · Line 2".into(), "Line 1".into(), "Line 2".into()]));
    h.step(&json!({"click":{"handle":"select_other:1"}})).unwrap();
    assert_eq!(h.app.session.selection, vec![Sel::SketchMidpoint { id: "l2".into() }]);
    h.step(&json!({"hold":at})).unwrap();
    h.step(&json!({"click":{"handle":"select_other:0"},"shift":true})).unwrap();
    assert_eq!(h.app.session.selection, vec![Sel::SketchMidpoint { id: "l2".into() }, Sel::SketchMidpoint { id: "l1".into() }]);
    assert_eq!(h.app.session.doc.to_json(), before);
    assert_eq!(h.app.session.undo.len(), undo);
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
fn select_other_offers_obscured_edges_while_a_normal_click_keeps_the_front() {
    let mut h = stacked_plates();
    let at = h.call("ui.at", json!({"world":[4.9,0,22]}))["result"].clone();
    let pos = egui::pos2(at[0].as_f64().unwrap() as f32, at[1].as_f64().unwrap() as f32);
    let proj = crate::viewport::projection(&h.app, h.app.viewport.rect.unwrap());
    let lower_edge =
        |hit: &crate::viewport::Hit| matches!(hit, crate::viewport::Hit::Edge { body, mid, .. } if body == "Top" && (mid.z - 20.0).abs() < 1e-6);
    assert!(!crate::viewport::pick(&h.app, &proj, pos).iter().any(lower_edge), "a normal click cannot see through the top face");
    assert!(crate::select_other::rows(&h.app, &proj, pos).iter().any(|row| lower_edge(&row.hit)), "Select Other offers the obscured edge");
}

#[test]
fn hidden_geometry_preference_preserves_the_nearest_face_list() {
    let mut h = stacked_plates();
    let at = h.call("ui.at", json!({"world":[2,1,22]}))["result"].clone();
    let pos = egui::pos2(at[0].as_f64().unwrap() as f32, at[1].as_f64().unwrap() as f32);
    let proj = crate::viewport::projection(&h.app, h.app.viewport.rect.unwrap());
    assert!(serde_json::from_value::<crate::prefs::Prefs>(json!({})).unwrap().select_other_hidden_geometry);
    for measure in [false, true] {
        if measure {
            h.app.start("inspect.measure");
        }
        for enabled in [true, false] {
            h.call("ui.prefs", json!({"select_other_hidden_geometry":enabled}));
            let restored: crate::prefs::Prefs = serde_json::from_value(serde_json::to_value(&h.app.preferences).unwrap()).unwrap();
            assert_eq!(restored.select_other_hidden_geometry, enabled);
            let rows = crate::select_other::rows(&h.app, &proj, pos);
            let faces = rows.iter().filter(|r| matches!(&r.sel, Sel::Face { body, .. } if body == "Top")).count();
            assert_eq!(faces, if enabled { 2 } else { 1 }, "measure={measure}, enabled={enabled}: {rows:?}");
        }
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
