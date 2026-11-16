use serde_json::json;
use solidcraft_doc::drawing::view::{View, ViewKind, ViewPlace};
use solidcraft_doc::drawing_view::{ViewDirection, ViewOverrides};
use solidcraft_geom::drawing_view::NamedView;
use solidcraft_geom::table::BALLOONED;
use solidcraft_sketch::PaperFonts;

use super::*;
use crate::schema::tests::conforms;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    let reply = s.execute(id, &p).unwrap_or_else(|e| panic!("{id}: {e}"));
    let schema = output_schema_for::<BalloonResult>();
    assert!(conforms(schema.as_value(), schema.as_value(), &reply) || !matches!(id, ADD | INSPECT | REPAIR), "{id}: {reply}");
    reply
}

fn refused(s: &mut Session, cmd: &str, p: Value) -> String {
    s.execute(cmd, &p).err().map(|e| e.to_string()).unwrap_or_else(|| panic!("accepted: {p}"))
}

fn saved(s: &Session) -> String {
    s.drawing("test").unwrap().to_json().unwrap()
}

/// A component, Bracket, holding a box 80 x 40 x 30 mm along X, Y and Z from the origin, called
/// Box, placed once where it was made; a 10 mm cube of the design's own, Base, at x = 200; and a
/// drawing of them, open, with a front view at 1:1. The session, the view's id and the Bracket's
/// component and occurrence.
fn placed() -> (Session, u64, [u64; 2]) {
    let mut s = Session::default();
    let made = run(&mut s, "component.create", json!({"name": "Bracket"}));
    let bracket = [made["component"].as_u64().unwrap(), made["occurrence"].as_u64().unwrap()];
    run(&mut s, "solid.box", json!({"length": 80, "width": 40, "height": 30, "body_name": "Box"}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [200, 0, 0], "operation": "new", "body_name": "Base"}));
    run(&mut s, "drawing.create", json!({}));
    let mut d = (**s.drawing("test").unwrap()).clone();
    let id = d.allocate_id(&mut ids::IdSource::seeded(3874)).unwrap();
    let (sheet, source) = (d.sheets[0].id, d.sources[0].id);
    let kind = ViewKind::Base { source, view: ViewDirection::Named(NamedView::Front) };
    let settings = ViewOverrides { scale: Some("1:1".parse().unwrap()), ..ViewOverrides::default() };
    d.add_views([Arc::new(View { id, sheet, kind, place: ViewPlace::At([200.0, 150.0]), settings, reference_shown_percent: 0.0, label: None })]);
    d.validate().unwrap();
    s.doc_mut().put_drawing(Arc::new(d));
    (s, id, bracket)
}

/// A balloon on the Box's bottom front edge, through the Bracket, at (60, 40) on paper.
fn add(s: &mut Session, view: u64, occurrence: u64) -> Value {
    run(s, ADD, json!({"view": ids::encode(view), "edge": {"point": [40, 0, 0], "path": [occurrence]}, "at": [60, 40]}))
}

fn ends_at(reply: &Value, p: [f64; 3]) {
    let point: Vec<f64> = reply["point"].as_array().unwrap().iter().map(|x| x.as_f64().unwrap()).collect();
    assert!(point.iter().zip(p).all(|(a, b)| (a - b).abs() < 1e-9), "{reply}");
}

/// A balloon numbers the part its edge is placed through, saves the edge by name (any point on it
/// picks the same record), and its arrow ends on the edge's middle, whatever point was picked. An
/// add is one undo step.
#[test]
fn a_balloon_points_at_a_placed_part_and_is_saved_by_name() {
    let (mut s, view, [_, bracket]) = placed();
    let before = saved(&s);
    let reply = add(&mut s, view, bracket);
    assert_eq!((reply["status"].as_str(), reply["exact"].as_bool()), (Some("ok"), Some(true)), "{reply}");
    assert_eq!(reply["occurrence"], json!([bracket]));
    assert_eq!(reply["view"], json!(ids::encode(view)));
    assert_eq!(reply["record"]["edge"]["path"], json!([bracket]));
    assert_eq!(reply["record"]["at"], json!([60.0, 40.0]));
    ends_at(&reply, [40.0, 0.0, 0.0]);
    let elsewhere = run(&mut s, ADD, json!({"view": ids::encode(view), "edge": {"point": [7.5, 0, 0], "path": [bracket]}, "at": [0, 0]}));
    assert_eq!(elsewhere["record"]["edge"], reply["record"]["edge"], "saved by name, not by the point");
    ends_at(&elsewhere, [40.0, 0.0, 0.0]);
    run(&mut s, "edit.undo", json!({}));
    let one = saved(&s);
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(saved(&s), before);
    run(&mut s, "edit.redo", json!({}));
    assert_eq!(saved(&s), one);
}

/// What a balloon can't point at is refused with its reason, and the drawing is left as it was: a
/// view the drawing doesn't have, a body of the design itself (no part), an occurrence that isn't
/// there, a point on no edge, and a place off paper.
#[test]
fn a_balloon_is_refused_what_it_cant_point_at() {
    let (mut s, view, [_, bracket]) = placed();
    let before = saved(&s);
    let v = json!(ids::encode(view));
    let nowhere = ids::encode(99);
    let cases = [
        (json!({"view": nowhere, "edge": {"point": [40, 0, 0], "path": [bracket]}, "at": [0, 0]}), format!("the drawing has no view {nowhere}")),
        (json!({"view": v, "edge": [205, 0, 0], "at": [0, 0]}), "it points at a placed part: its edge's body is placed through an occurrence".into()),
        (json!({"view": v, "edge": {"point": [40, 0, 0], "path": [999]}, "at": [0, 0]}), "no edge at [40, 0, 0]".into()),
        (json!({"view": v, "edge": {"point": [40, 20, 0], "path": [bracket]}, "at": [0, 0]}), "no edge at [40, 20, 0]".into()),
        (json!({"view": v, "edge": {"point": [40, 0, 0], "path": [bracket]}, "at": [5000.001, 0]}), "its place is at most 5000 mm on paper".into()),
    ];
    for (p, why) in cases {
        let e = refused(&mut s, ADD, p.clone());
        assert!(e.contains(&why), "{p}: {e}");
    }
    assert_eq!(saved(&s), before);
}

/// A balloon attaches only to a part its view draws. One on a part the view's hide leaves out, or
/// that its source doesn't choose, is refused in words that name the view and the setting, and the
/// drawing is left as it was. A view projected from it follows its parent's hide and names the parent.
#[test]
fn a_balloon_is_refused_on_a_part_its_view_leaves_out() {
    let (mut s, view, [_, bracket]) = placed();
    let v = ids::encode(view);
    let on = |view: &str| json!({"view": view, "edge": {"point": [40, 0, 0], "path": [bracket]}, "at": [60, 40]});
    run(&mut s, "drawing.view.edit", json!({"view": v, "set": {"hide": [[bracket]]}}));
    let projected = run(&mut s, "drawing.view.projected", json!({"parent": v, "side": "right"}))["view"].as_str().unwrap().to_string();
    let before = saved(&s);
    let e = refused(&mut s, ADD, on(&v));
    assert!(e.contains(&format!("view {v} doesn't draw that part: its hide leaves out occurrence path [{bracket}]")), "{e}");
    let e = refused(&mut s, ADD, on(&projected));
    assert!(e.contains(&format!("view {projected} doesn't draw that part")) && e.contains(&format!("(set on view {v})")), "{e}");
    assert_eq!(saved(&s), before);
    run(&mut s, "drawing.view.edit", json!({"view": v, "set": {"hide": null, "source": {"occurrences": [[]]}}}));
    let e = refused(&mut s, ADD, on(&v));
    assert!(e.contains(&format!("view {v} doesn't draw that part: its source draws only the occurrences it chose")), "{e}");
    run(&mut s, "drawing.view.edit", json!({"view": v, "set": {"source": null}}));
    assert_eq!(add(&mut s, view, bracket)["status"], "ok");
}

/// A balloon added before its view's settings left its part out stays where it is, and inspect
/// warns, until the edit is undone.
#[test]
fn a_balloon_warns_when_its_view_leaves_its_part_out_later() {
    let (mut s, view, [_, bracket]) = placed();
    let v = ids::encode(view);
    let id = add(&mut s, view, bracket)["balloon"].clone();
    assert!(run(&mut s, INSPECT, json!({"balloon": id}))["warnings"].is_null());
    run(&mut s, "drawing.view.edit", json!({"view": v, "set": {"hide": [[bracket]]}}));
    let warned = run(&mut s, INSPECT, json!({"balloon": id}));
    let words = format!("view {v} doesn't draw this part now: its hide leaves out occurrence path [{bracket}]");
    assert_eq!(warned["warnings"], json!([words]), "{warned}");
    assert_eq!(warned["status"], "ok", "it still points at its part");
    run(&mut s, "edit.undo", json!({}));
    assert!(run(&mut s, INSPECT, json!({"balloon": id}))["warnings"].is_null());
}

/// An edit moves a balloon and names what changed, as one undo step; the same place again changes
/// nothing and adds no step. What it points at isn't one of its settings.
#[test]
fn an_edit_moves_a_balloon_and_the_same_place_again_changes_nothing() {
    let (mut s, view, [_, bracket]) = placed();
    let id = add(&mut s, view, bracket)["balloon"].clone();
    let before = saved(&s);
    let moved = run(&mut s, EDIT, json!({"balloon": id, "set": {"at": [-12.5, 80]}}));
    assert_eq!(moved, json!({"balloon": id, "changed": ["at"]}));
    assert_eq!(run(&mut s, INSPECT, json!({"balloon": id}))["record"]["at"], json!([-12.5, 80.0]));
    let after = saved(&s);
    for same in [json!({"at": [-12.5, 80]}), json!({})] {
        assert_eq!(run(&mut s, EDIT, json!({"balloon": id, "set": same}))["changed"], json!([]));
    }
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(saved(&s), before, "one step for the move, none for the same place");
    run(&mut s, "edit.redo", json!({}));
    assert_eq!(saved(&s), after);
    let e = refused(&mut s, EDIT, json!({"balloon": id, "set": {"edge": [40, 0, 0]}}));
    assert!(e.contains("edge"), "{e}");
    let e = refused(&mut s, EDIT, json!({"balloon": ids::encode(99), "set": {"at": [0, 0]}}));
    assert!(e.contains("the drawing has no balloon"), "{e}");
    assert_eq!(saved(&s), after);
}

/// Inspect says where a balloon stands with the model now: a notch across its edge splits it, so it
/// lists both pieces and has no middle; undone, it is whole again; an occurrence that is gone is
/// lost, said so, and never moved to another part. The model is changed with the drawing closed.
#[test]
fn inspect_says_where_a_balloon_stands_with_the_model() {
    let (mut s, view, [component, bracket]) = placed();
    let drawing = json!({"drawing": ids::encode(s.drawing("test").unwrap().id)});
    let id = add(&mut s, view, bracket)["balloon"].clone();
    run(&mut s, "drawing.close", json!({}));
    run(&mut s, "component.activate", json!({"component": component}));
    run(&mut s, "solid.box", json!({"length": 10, "width": 5, "height": 5, "corner": [35, -1, -1], "operation": "cut", "targets": ["Box"]}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    run(&mut s, "drawing.open", drawing.clone());
    let split = run(&mut s, INSPECT, json!({"balloon": id}));
    assert_eq!(split["status"], "ambiguous", "{split}");
    assert_eq!(split["candidates"].as_array().map(Vec::len), Some(2), "{split}");
    assert!(split.get("point").is_none(), "no one middle: {split}");
    run(&mut s, "drawing.close", json!({}));
    run(&mut s, "edit.undo", json!({}));
    run(&mut s, "drawing.open", drawing);
    let whole = run(&mut s, INSPECT, json!({"balloon": id}));
    assert_eq!(whole["status"], "ok", "{whole}");
    ends_at(&whole, [40.0, 0.0, 0.0]);
    let mut d = (**s.drawing("test").unwrap()).clone();
    let b = d.balloons.first_mut().map(Arc::make_mut).unwrap();
    b.edge.path = vec![999];
    s.doc_mut().put_drawing(Arc::new(d));
    let gone = run(&mut s, INSPECT, json!({"balloon": id}));
    assert_eq!((gone["status"].as_str(), gone["occurrence"].clone()), (Some("lost"), json!([999])), "{gone}");
    assert!(gone["why"].as_str().is_some_and(|why| !why.is_empty()) && gone.get("point").is_none(), "{gone}");
    let e = refused(&mut s, INSPECT, json!({"balloon": ids::encode(99)}));
    assert!(e.contains("the drawing has no balloon"), "{e}");
}

/// A balloon put on a piece of a cut edge is found through the piece's numbered name, so not exact.
/// Once the cut is suppressed, the two faces the name names meet in one whole edge again, found
/// exactly, and the arrow moves to its middle.
#[test]
fn a_balloon_on_a_piece_is_not_exact_until_the_edge_is_whole_again() {
    let (mut s, view, [component, bracket]) = placed();
    let drawing = json!({"drawing": ids::encode(s.drawing("test").unwrap().id)});
    run(&mut s, "drawing.close", json!({}));
    run(&mut s, "component.activate", json!({"component": component}));
    let cut = run(
        &mut s,
        "solid.box",
        json!({"length": 10, "width": 5, "height": 5, "corner": [35, -1, -1], "operation": "cut", "targets": ["Box"]}),
    )["feature"]
        .clone();
    run(&mut s, "component.activate", json!({"component": "root"}));
    run(&mut s, "drawing.open", drawing.clone());
    let piece = run(&mut s, ADD, json!({"view": ids::encode(view), "edge": {"point": [10, 0, 0], "path": [bracket]}, "at": [60, 40]}));
    assert_eq!((piece["status"].as_str(), piece["exact"].as_bool()), (Some("ok"), Some(false)), "{piece}");
    ends_at(&piece, [17.5, 0.0, 0.0]);
    run(&mut s, "drawing.close", json!({}));
    run(&mut s, "component.activate", json!({"component": component}));
    run(&mut s, "timeline.suppress", json!({"feature": cut, "suppressed": true}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    run(&mut s, "drawing.open", drawing);
    let whole = run(&mut s, INSPECT, json!({"balloon": piece["balloon"]}));
    assert_eq!((whole["status"].as_str(), whole["exact"].as_bool()), (Some("ok"), Some(true)), "{whole}");
    ends_at(&whole, [40.0, 0.0, 0.0]);
}

/// A fillet inside the Bracket takes the Box's bottom front edge a balloon points at: inspect says it
/// is lost and lists leads. The one where the fillet meets the front, at z = 5, passed back as it
/// stands, repairs it on its part: it reads ok with its arrow on that edge's middle, its id, place
/// and part kept, as one undo step, and once it reads it isn't repaired again. Undone, it is lost.
/// A corner, or an edge with a path of its own, is refused and changes nothing.
#[test]
fn a_lost_balloon_is_repaired_on_its_part() {
    let (mut s, view, [component, bracket]) = placed();
    let drawing = json!({"drawing": ids::encode(s.drawing("test").unwrap().id)});
    let id = add(&mut s, view, bracket)["balloon"].clone();
    run(&mut s, "drawing.close", json!({}));
    run(&mut s, "component.activate", json!({"component": component}));
    run(&mut s, "solid.fillet", json!({"edges": [[40, 0, 0]], "radius": 5, "body": "Box"}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    run(&mut s, "drawing.open", drawing);
    let lost = run(&mut s, INSPECT, json!({"balloon": id}));
    assert_eq!(lost["status"], "lost", "{lost}");
    let front = {
        let state = s.model.state();
        let mesh = state.body("Box").unwrap().mesh();
        mesh.edges.iter().position(|line| line.iter().all(|p| p.y.abs() < 1e-6 && (p.z - 5.0).abs() < 1e-6)).unwrap()
    };
    let front = json!({"edge": {"body": "Box", "index": front}});
    let pick = lost["candidates"].as_array().and_then(|c| c.iter().find(|c| **c == front)).cloned();
    let pick = pick.unwrap_or_else(|| panic!("the fillet's front edge is a lead: {lost}"));
    let (before, steps) = (saved(&s), s.undo.len());
    let p = json!({"balloon": id, "pick": pick});
    let repaired = run(&mut s, REPAIR, p.clone());
    assert_eq!(repaired["status"], "ok", "{repaired}");
    ends_at(&repaired, [40.0, 0.0, 5.0]);
    assert_eq!((&repaired["balloon"], &repaired["record"]["at"], &repaired["occurrence"]), (&id, &json!([60.0, 40.0]), &json!([bracket])));
    assert_eq!(s.undo.len(), steps + 1, "one undo step");
    assert!(refused(&mut s, REPAIR, p).contains("it reads ok"));
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(saved(&s), before);
    assert_eq!(run(&mut s, INSPECT, json!({"balloon": id}))["status"], "lost");
    for (pick, why) in [
        (json!({"corner": [40, 0, 5]}), "this reference is an edge"),
        (json!({"edge": {"point": [40, 0, 5], "path": [bracket]}}), "a repair's pick has no `path`"),
    ] {
        let e = refused(&mut s, REPAIR, json!({"balloon": id, "pick": pick}));
        assert!(e.contains(why), "{e}");
    }
    assert_eq!((saved(&s), s.undo.len()), (before, steps));
}

/// Every place a value can go in `v`, as JSON pointers: `v` itself, then each member and item.
fn places(v: &Value, at: String, out: &mut Vec<String>) {
    out.push(at.clone());
    match v {
        Value::Object(o) => o.iter().for_each(|(k, x)| places(x, format!("{at}/{k}"), out)),
        Value::Array(l) => l.iter().enumerate().for_each(|(i, x)| places(x, format!("{at}/{i}"), out)),
        _ => {}
    }
}

/// A hostile value in each place of each command's parameters, at every depth (the edge's pick and
/// path, the place, the edit's settings): never a panic, and the drawing keeps its rules.
#[test]
fn hostile_balloon_params_never_panic() {
    let (mut s, view, [_, bracket]) = placed();
    let id = add(&mut s, view, bracket)["balloon"].clone();
    let v = json!(ids::encode(view));
    let calls = [
        (ADD, json!({"view": v, "edge": {"point": [40, 0, 0], "path": [bracket]}, "at": [1, 2]})),
        (ADD, json!({"view": v, "edge": {"body": "Box", "index": 0, "path": [bracket]}, "at": [1, 2]})),
        (EDIT, json!({"balloon": id, "set": {"at": [3, 4]}})),
        (INSPECT, json!({"balloon": id})),
        (REPAIR, json!({"balloon": id, "pick": {"edge": {"body": "Box", "index": 0}}})),
    ];
    let mut values = crate::tests::hostile_values();
    values.extend([json!({"body": "Box", "index": u64::MAX}), Value::Array(vec![json!(1); 1000]), json!([1e308, 0]), json!("x".repeat(600))]);
    let mut internal = Vec::new();
    for (cmd, template) in calls {
        let mut at = Vec::new();
        places(&template, String::new(), &mut at);
        for (place, value) in at.iter().flat_map(|place| values.iter().map(move |value| (place, value))) {
            let mut p = template.clone();
            if let Some(slot) = p.pointer_mut(place) {
                *slot = value.clone();
            }
            let mut copy = s.scratch();
            if let Err(crate::EngineError::Internal(_, msg)) = copy.execute(cmd, &p) {
                internal.push(format!("{cmd} {p}: {msg}"));
            }
            copy.doc.drawings.iter().for_each(|d| d.validate().unwrap());
        }
    }
    assert!(internal.is_empty(), "balloon commands panicked:\n{}", internal.join("\n"));
}

/// A view lists its balloons. A copied view's balloons are copies with ids of their own, pointing
/// at the same part from the same place, and one undo step takes the copy and them out. A sheet
/// with them on it is refused, naming them, until its contents go with it.
#[test]
fn balloons_go_with_their_view_when_it_is_copied_and_with_its_sheet() {
    let (mut s, view, [_, bracket]) = placed();
    let original = add(&mut s, view, bracket);
    let listed = |s: &mut Session, view: &Value| run(s, "drawing.view.inspect", json!({"view": view}))["balloons"].clone();
    assert_eq!(listed(&mut s, &json!(ids::encode(view))), json!([original["balloon"]]));
    let before = saved(&s);
    let sheet = run(&mut s, "drawing.sheet.add", json!({"name": "Sheet 2"}))["sheet"].clone();
    let reply = run(&mut s, "drawing.view.copy_to_sheet", json!({"view": ids::encode(view), "sheet": sheet, "position_mm": [100, 100]}));
    let copy = reply["view"].clone();
    let copied = listed(&mut s, &copy);
    let [balloon] = copied.as_array().map(Vec::as_slice).unwrap_or_default() else { panic!("one copied balloon: {copied}") };
    assert_ne!(balloon, &original["balloon"]);
    let read = run(&mut s, INSPECT, json!({"balloon": balloon}));
    assert_eq!((read["status"].as_str(), &read["view"]), (Some("ok"), &copy), "{read}");
    assert_eq!((&read["record"]["edge"], &read["record"]["at"]), (&original["record"]["edge"], &original["record"]["at"]));
    assert_eq!(listed(&mut s, &json!(ids::encode(view))), json!([original["balloon"]]), "the original keeps its own");
    let e = refused(&mut s, "drawing.sheet.delete", json!({"sheet": sheet}));
    let named = format!("1 view (`{}`) and 1 balloon (`{}`)", copy.as_str().unwrap(), balloon.as_str().unwrap());
    assert!(e.contains(&named), "{e}");
    run(&mut s, "drawing.sheet.delete", json!({"sheet": sheet, "contents": "delete"}));
    let e = refused(&mut s, INSPECT, json!({"balloon": balloon}));
    assert!(e.contains("the drawing has no balloon"), "{e}");
    for _ in 0..3 {
        run(&mut s, "edit.undo", json!({}));
    }
    assert_eq!(saved(&s), before);
}

/// A parts list on the drawing's first sheet: its id.
fn list(s: &mut Session, set: Value) -> String {
    let sheet = ids::encode(s.drawing("test").unwrap().sheets[0].id);
    let reply = s.execute("drawing.table.add", &json!({"sheet": sheet, "kind": "parts_list", "set": set})).unwrap();
    reply["table"].as_str().unwrap().to_string()
}

fn read(s: &mut Session, balloon: &str) -> Value {
    run(s, INSPECT, json!({"balloon": balloon}))
}

/// A parts list of one sheet shows what that sheet's balloons read, each row numbered and counted as in
/// the whole list; with none, its heading alone. One of the drawing shows what any sheet's balloons read.
/// A balloon on a view moved to another sheet goes with it, and the window draws each change as a build.
#[test]
fn a_list_can_show_only_what_its_sheet_balloons() {
    let (mut s, view, [_, bracket]) = placed();
    let drawing = ids::encode(s.drawing("test").unwrap().id);
    run(&mut s, "drawing.close", json!({}));
    let made = |s: &mut Session, name: &str, x: f64| {
        let occurrence = run(s, "component.create", json!({"name": name}))["occurrence"].as_u64().unwrap();
        run(s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [x, 0, 0], "body_name": name}));
        run(s, "component.activate", json!({"component": "root"}));
        occurrence
    };
    let pin = made(&mut s, "Pin", 300.0);
    let nut = made(&mut s, "Nut", 400.0);
    run(&mut s, "occurrence.copy", json!({"component": "Nut", "translate": [0, 100, 0]}));
    run(&mut s, "drawing.open", json!({"drawing": drawing}));

    let second = ids::decode(run(&mut s, "drawing.sheet.add", json!({"name": "Sheet 2"}))["sheet"].as_str().unwrap()).unwrap();
    let mut d = (**s.drawing("test").unwrap()).clone();
    let other = d.allocate_id(&mut ids::IdSource::seeded(3875)).unwrap();
    let mut moved = (**d.views.iter().find(|v| v.id == view).unwrap()).clone();
    moved.id = other;
    moved.sheet = second;
    d.add_views([Arc::new(moved)]);
    s.doc_mut().put_drawing(Arc::new(d));

    let table = s.execute("drawing.table.add", &json!({"sheet": ids::encode(second), "kind": "parts_list"})).unwrap();
    let table = ids::decode(table["table"].as_str().unwrap()).unwrap();
    // Each row the table shows: its name, item, quantity and ballooned count, from the rows it is laid out from.
    let names = |s: &Session| -> Vec<[String; 4]> {
        let d = s.drawing("test").unwrap().clone();
        let record = d.tables.by_id(table).unwrap();
        let sheet = d.sheets.iter().find(|sheet| sheet.id == second).unwrap();
        let lettering = PaperFonts::new(&d.font, &d.language, true);
        let space = crate::drawing_table_paper::drawing_space(sheet);
        let placed = crate::drawing_table_paper::place(s, &d, record, space, &lettering).ok().unwrap();
        let by_key = |key: &str| placed.rows.iter().find(|row| row.key == key).unwrap();
        let field = |key: &str, name: &str| by_key(key).fields.get(name).cloned().unwrap_or_default();
        let shown = placed.layout.rows.iter().filter_map(|row| row.key.as_deref());
        shown.map(|key| ["name", "item", "quantity", BALLOONED].map(|name| field(key, name))).collect()
    };
    let whole = names(&s);
    assert_eq!(whole.len(), 3, "{whole:?}");
    let of = |name: &str, count: &str| {
        let [name, item, quantity, _] = whole.iter().find(|row| row[0] == name).unwrap().clone();
        [name, item, quantity, count.to_string()]
    };
    let sheet = second;
    let settled = |s: &mut Session| {
        crate::drawing_scene::extract(s, sheet).unwrap();
        while crate::drawing_scene::step(s) {}
        let view = crate::drawing_scene::extract(s, sheet).unwrap();
        let d = s.drawing("test").unwrap().clone();
        assert_eq!(*view.scene, crate::drawing_scene::build(s, &d, sheet).unwrap().scene, "as a build draws it");
    };
    let filter = |s: &mut Session, scope: &str| {
        let set = json!({"filter": {"ballooned": scope}});
        s.execute("drawing.table.edit", &json!({"table": ids::encode(table), "set": set})).unwrap();
    };

    filter(&mut s, "sheet");
    assert!(names(&s).is_empty(), "no balloons: its heading alone");
    settled(&mut s);
    let on = |s: &mut Session, view: u64, occurrence: u64, x: f64| {
        let edge = json!({"point": [x, 0, 0], "path": [occurrence]});
        run(s, ADD, json!({"view": ids::encode(view), "edge": edge, "at": [60, 40]}))["balloon"].clone()
    };
    on(&mut s, other, nut, 405.0);
    assert_eq!(names(&s), [of("Nut", "1")], "numbered and counted as in the whole list");
    assert_eq!(of("Nut", "1")[2], "2");
    settled(&mut s);
    on(&mut s, other, pin, 305.0);
    assert_eq!(names(&s), [of("Nut", "1"), of("Pin", "1")]);
    on(&mut s, view, bracket, 40.0);
    assert_eq!(names(&s).len(), 2, "the balloon on the other sheet isn't this sheet's");
    settled(&mut s);

    filter(&mut s, "drawing");
    assert_eq!(names(&s).len(), 3, "the drawing's balloons");
    settled(&mut s);
    // What drawing.inspect counts is what the list shows.
    let inspected = s.execute("drawing.inspect", &json!({})).unwrap();
    let list = inspected["parts_lists"].as_array().unwrap().iter().find(|one| one["table"] == ids::encode(table)).unwrap().clone();
    let counted: Vec<u64> = list["ballooned"].as_array().unwrap().iter().map(|row| row["balloons"].as_u64().unwrap()).collect();
    assert_eq!(counted.iter().filter(|n| **n > 0).count(), 3, "{list}");

    filter(&mut s, "sheet");
    assert_eq!(names(&s).len(), 2);
    let inspected = s.execute("drawing.inspect", &json!({})).unwrap();
    let list = inspected["parts_lists"].as_array().unwrap().iter().find(|one| one["table"] == ids::encode(table)).unwrap().clone();
    let counted: Vec<u64> = list["ballooned"].as_array().unwrap().iter().map(|row| row["balloons"].as_u64().unwrap()).collect();
    assert_eq!(counted.iter().filter(|n| **n > 0).count(), 2, "its sheet's count, as the list shows: {list}");
    let mut d = (**s.drawing("test").unwrap()).clone();
    let at = d.views.iter().position(|v| v.id == view).unwrap();
    Arc::make_mut(d.views.get_mut(at).unwrap()).sheet = second;
    s.doc_mut().put_drawing(Arc::new(d));
    assert_eq!(names(&s).len(), 3, "its view moved here");
    settled(&mut s);
}

/// A revision table has no rows a balloon reads, so it can't be filtered by them.
#[test]
fn a_revision_table_cant_show_only_what_is_ballooned() {
    let (mut s, ..) = placed();
    let sheet = ids::encode(s.drawing("test").unwrap().sheets[0].id);
    let table = s.execute("drawing.table.add", &json!({"sheet": sheet, "kind": "revision"})).unwrap()["table"].clone();
    let e = refused(&mut s, "drawing.table.edit", json!({"table": table, "set": {"filter": {"ballooned": "sheet"}}}));
    assert!(e.contains("parts list"), "{e}");
}

/// A balloon with no list says so, and reads the only one when there is one: the row its part is
/// counted in. Two lists leave it to choose, with the choices listed, until it names one. A list's
/// delete reports the balloon that named it and leaves the balloon. One undo step each.
#[test]
fn a_balloon_reads_its_row_in_a_parts_list() {
    let (mut s, view, [_, occurrence]) = placed();
    let balloon = add(&mut s, view, occurrence)["balloon"].as_str().unwrap().to_string();
    let alone = read(&mut s, &balloon);
    assert_eq!((&alone["row_status"], alone.get("item"), alone.get("parts_list")), (&json!("no_list"), None, None), "{alone}");

    let first = list(&mut s, json!({}));
    let read1 = read(&mut s, &balloon);
    assert_eq!((&read1["row_status"], &read1["item"], &read1["parts_list"]), (&json!("ok"), &json!("1"), &json!(first)), "{read1}");
    assert!(s.drawing("test").unwrap().balloons.by_id(ids::decode(&balloon).unwrap()).unwrap().parts_list.is_none(), "worked out, not saved");

    let second = list(&mut s, json!({"list": {"levels": "top"}}));
    let both = read(&mut s, &balloon);
    assert_eq!(both["row_status"], json!("no_list"), "{both}");
    assert_eq!(both["choices"].as_array().map(Vec::len), Some(2));

    let steps = s.undo.len();
    let named = run(&mut s, EDIT, json!({"balloon": balloon, "set": {"parts_list": second}}));
    assert_eq!((&named["changed"], s.undo.len()), (&json!(["parts_list"]), steps + 1), "{named}");
    assert_eq!(read(&mut s, &balloon)["parts_list"], json!(second));
    run(&mut s, EDIT, json!({"balloon": balloon, "set": {"parts_list": second}}));
    assert_eq!(s.undo.len(), steps + 1, "nothing changed, no step");
    assert!(refused(&mut s, EDIT, json!({"balloon": balloon, "set": {"parts_list": ids::encode(99)}})).contains("parts lists"));

    let gone = s.execute("drawing.table.delete", &json!({"table": second})).unwrap();
    assert_eq!(gone["balloons"], json!([balloon]), "{gone}");
    assert_eq!(read(&mut s, &balloon)["parts_list"], json!(first), "the only one left");
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(read(&mut s, &balloon)["parts_list"], json!(second), "the undo brings its link back");
    run(&mut s, EDIT, json!({"balloon": balloon, "set": {"parts_list": null}}));
    assert_eq!(read(&mut s, &balloon)["row_status"], json!("no_list"));

    let on_add = run(
        &mut s,
        ADD,
        json!({"view": ids::encode(view), "edge": {"point": [40, 0, 0], "path": [occurrence]}, "at": [60, 40], "parts_list": first}),
    );
    assert_eq!(on_add["parts_list"], json!(first), "{on_add}");
    refused(&mut s, ADD, json!({"view": ids::encode(view), "edge": {"point": [40, 0, 0], "path": [occurrence]}, "at": [60, 40], "parts_list": "x"}));
}

/// A balloon follows its row: renumbered, it shows the new number. What the model does to the row
/// says what became of it, and never removes the balloon; undoing brings the number back.
#[test]
fn a_balloon_follows_its_row_through_the_model() {
    let (mut s, view, [component, occurrence]) = placed();
    let balloon = add(&mut s, view, occurrence)["balloon"].as_str().unwrap().to_string();
    list(&mut s, json!({}));
    assert_eq!(read(&mut s, &balloon)["item"], json!("1"));
    let drawing = ids::encode(s.open_drawing.unwrap());
    let with_drawing_closed = |s: &mut Session, id: &str, p: Value| {
        s.execute("drawing.close", &json!({})).unwrap();
        s.execute(id, &p).unwrap();
        s.execute("drawing.open", &json!({"drawing": drawing})).unwrap();
    };

    with_drawing_closed(&mut s, "assembly.bom.renumber", json!({"levels": "flat", "start": 7}));
    assert_eq!(read(&mut s, &balloon)["item"], json!("7"), "renumbered");

    with_drawing_closed(&mut s, "occurrence.bom_type", json!({"occurrence": occurrence, "bom_type": "reference"}));
    let reference = read(&mut s, &balloon);
    assert_eq!((&reference["row_status"], reference.get("item")), (&json!("reference"), None), "{reference}");
    assert!(reference["warnings"][0].as_str().is_some_and(|w| w.contains("reference")), "{reference}");
    with_drawing_closed(&mut s, "edit.undo", json!({}));
    assert_eq!(read(&mut s, &balloon)["item"], json!("7"), "undone");

    with_drawing_closed(&mut s, "occurrence.bom_type", json!({"occurrence": occurrence, "bom_type": "phantom"}));
    assert_eq!(read(&mut s, &balloon)["row_status"], json!("ok"), "a phantom's own bodies are a row of their own");
    with_drawing_closed(&mut s, "edit.undo", json!({}));

    with_drawing_closed(&mut s, "component.delete", json!({"component": component}));
    let deleted = read(&mut s, &balloon);
    assert_eq!(deleted["row_status"], json!("not_found"), "{deleted}");
    assert_eq!(s.drawing("test").unwrap().balloons.len(), 1, "the balloon stays");
    with_drawing_closed(&mut s, "edit.undo", json!({}));
    assert_eq!(read(&mut s, &balloon)["item"], json!("7"), "the part back, its number back");
}

/// How many balloons point at each row of each parts list, as `drawing.inspect` gives them, by the
/// list's id.
fn ballooned(s: &mut Session) -> Vec<(String, Vec<(String, u64)>)> {
    let inspected = s.execute("drawing.inspect", &json!({})).unwrap();
    let rows = |list: &Value| -> Vec<(String, u64)> {
        list["ballooned"].as_array().unwrap().iter().map(|r| (r["item"].as_str().unwrap().into(), r["balloons"].as_u64().unwrap())).collect()
    };
    let lists = inspected["parts_lists"].as_array().unwrap();
    let mut by_id: Vec<_> = lists.iter().map(|list| (list["table"].as_str().unwrap().into(), rows(list))).collect();
    by_id.sort();
    by_id
}

/// Each row of a parts list counts the balloons that read it, 0 included, in the list's order: a
/// balloon reading another list, or none while two are on the sheet, counts for none, as does one
/// whose part is gone. Worked out on each read and never saved, and the same in the rows the sheet
/// draws.
#[test]
fn a_parts_list_counts_the_balloons_on_each_row() {
    let (mut s, view, [component, occurrence]) = placed();
    let drawing = ids::encode(s.open_drawing.unwrap());
    let with_drawing_closed = |s: &mut Session, id: &str, p: Value| {
        s.execute("drawing.close", &json!({})).unwrap();
        let reply = s.execute(id, &p).unwrap();
        s.execute("drawing.open", &json!({"drawing": drawing})).unwrap();
        reply
    };
    // A 10 mm pin of its own at x = 100, below the Bracket in the list.
    let pin = with_drawing_closed(&mut s, "component.create", json!({"name": "Pin"}));
    with_drawing_closed(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [100, 0, 0], "body_name": "Pin"}));
    let pin = pin["occurrence"].as_u64().unwrap();
    let first = list(&mut s, json!({}));
    let rows = |counts: [u64; 2]| vec![(first.clone(), vec![("1".to_string(), counts[0]), ("2".to_string(), counts[1])])];
    assert_eq!(ballooned(&mut s), rows([0, 0]), "every row, none ballooned");

    let on_bracket = add(&mut s, view, occurrence)["balloon"].as_str().unwrap().to_string();
    assert_eq!(ballooned(&mut s), rows([1, 0]));
    add(&mut s, view, occurrence);
    assert_eq!(ballooned(&mut s), rows([2, 0]), "two on one row");
    run(&mut s, ADD, json!({"view": ids::encode(view), "edge": {"point": [105, 0, 0], "path": [pin]}, "at": [-60, 40]}));
    assert_eq!(ballooned(&mut s), rows([2, 1]));
    // The rows the sheet draws hold the same counts, in the editor's column.
    let shown = s.execute("drawing.table.inspect", &json!({"table": first})).unwrap();
    let counts: Vec<u64> = shown["rows"].as_array().unwrap().iter().map(|row| row["balloons"].as_u64().unwrap()).collect();
    assert_eq!(counts, [2, 1], "{shown}");

    let second = list(&mut s, json!({}));
    let two = ballooned(&mut s);
    assert_eq!(two.iter().map(|(_, r)| r.iter().map(|(_, n)| n).sum::<u64>()).collect::<Vec<_>>(), [0, 0], "none names a list: none counts");
    run(&mut s, EDIT, json!({"balloon": on_bracket, "set": {"parts_list": second}}));
    let mut named =
        vec![(first.clone(), vec![("1".to_string(), 0), ("2".to_string(), 0)]), (second.clone(), vec![("1".to_string(), 1), ("2".to_string(), 0)])];
    named.sort();
    assert_eq!(ballooned(&mut s), named, "it counts on the list it names alone");
    s.execute("drawing.table.delete", &json!({"table": first})).unwrap();
    assert_eq!(ballooned(&mut s), vec![(second.clone(), vec![("1".to_string(), 2), ("2".to_string(), 1)])], "the only list again");

    with_drawing_closed(&mut s, "component.delete", json!({"component": component}));
    assert_eq!(
        ballooned(&mut s),
        vec![(second.clone(), vec![("2".to_string(), 1)])],
        "the Bracket's balloons count for no row; the Pin keeps its number"
    );
    with_drawing_closed(&mut s, "assembly.bom.renumber", json!({"levels": "flat", "start": 1}));
    assert_eq!(ballooned(&mut s), vec![(second, vec![("1".to_string(), 1)])], "nor for the row that takes the Bracket's number");
}

/// A list of the component the balloon's part is in reads it below that component; a list of the
/// component a balloon's path doesn't run through, or one deleted since, finds nothing.
#[test]
fn a_balloon_reads_a_components_list_below_it() {
    let (mut s, view, [component, occurrence]) = placed();
    let balloon = add(&mut s, view, occurrence)["balloon"].as_str().unwrap().to_string();
    list(&mut s, json!({"list": {"component": component}}));
    assert_eq!(read(&mut s, &balloon)["row_status"], json!("no_row"), "the assembly listed has no row");
    let drawing = ids::encode(s.open_drawing.unwrap());
    s.execute("drawing.close", &json!({})).unwrap();
    s.execute("component.delete", &json!({"component": component})).unwrap();
    s.execute("drawing.open", &json!({"drawing": drawing})).unwrap();
    assert_eq!(read(&mut s, &balloon)["row_status"], json!("not_found"));
}

/// A balloon on a parts list that a file made, listing another design's file, reads no row and says
/// why: it is never numbered from the design the drawing is in (`Session::drawn`). Not on inspect, not
/// on the sheet, and the list counts no balloons against this design's rows.
#[test]
fn a_balloon_on_a_list_of_a_linked_design_is_never_numbered() {
    let (mut s, view, [_, occurrence]) = placed();
    let balloon = add(&mut s, view, occurrence)["balloon"].as_str().unwrap().to_string();
    let table = list(&mut s, json!({}));
    assert_eq!(read(&mut s, &balloon)["row_status"], json!("ok"), "read from this design while it lists it");
    let mut d = (**s.drawing("test").unwrap()).clone();
    let linked = d.sources.iter().map(|source| source.id).max().unwrap() + 1;
    d.sources.push(solidcraft_doc::drawing::Source { id: linked, path: Some("plate.solidcraft".into()) });
    let at = d.tables.index_of(ids::decode(&table).unwrap()).unwrap();
    if let solidcraft_doc::drawing::table::Kind::PartsList(list) = &mut Arc::make_mut(d.tables.get_mut(at).unwrap()).kind {
        list.of.source = linked;
    }
    s.doc_mut().put_drawing(Arc::new(d));
    s.drawing("test").unwrap().validate().unwrap();

    let reply = read(&mut s, &balloon);
    assert_eq!((&reply["row_status"], reply.get("item"), &reply["parts_list"]), (&json!("unread"), None, &json!(table)), "{reply}");
    let warned = reply["warnings"].as_array().unwrap().iter().any(|w| w.as_str().is_some_and(|w| w.contains("linked design")));
    assert!(warned, "{reply}");
    let (_, word) = on_sheet(&mut s, &balloon);
    assert!(word["why"].as_str().is_some_and(|why| why.contains("linked design")), "{word}");
    let inspected = s.execute("drawing.inspect", &json!({})).unwrap();
    let listed = inspected["parts_lists"].as_array().unwrap().iter().find(|l| l["table"] == json!(table)).cloned().unwrap();
    assert_eq!(listed["ballooned"], Value::Null, "no rows counted from this design: {listed}");
}

/// That `reply` reads ok with its arrow on `p`, to within what the joints' solve places parts to.
fn ends_near(reply: &Value, p: [f64; 3]) {
    let point: Vec<f64> = reply["point"].as_array().unwrap_or_else(|| panic!("no point: {reply}")).iter().map(|x| x.as_f64().unwrap()).collect();
    assert!(reply["status"] == "ok" && point.iter().zip(p).all(|(a, b)| (a - b).abs() < 1e-6), "{reply}");
}

/// A balloon goes on a part of a view in a position of its own, and its arrow ends where that view
/// draws the edge (#13048): on [`hinge_pair`](crate::drawing_scene::tests::hinge_pair)'s view with the
/// leaf turned up 90° about the hinge line (y 20, z 0), the leaf's free bottom edge, (0..40, 40, 0) as
/// designed, is at y 20 + 0, z 0 + 20: its middle at (20, 20, 20), not the design's (20, 40, 0).
#[test]
fn a_balloon_on_a_posed_view_ends_where_the_view_draws_its_edge() {
    let (mut s, view, _) = crate::drawing_scene::tests::hinge_pair();
    let leaf = s.doc.occurrences.iter().find(|o| o.name.starts_with("Leaf")).map(|o| o.id).unwrap();
    run(&mut s, "drawing.view.edit", json!({"view": ids::encode(view), "set": {"pose": [{"joint": "Hinge", "values": [90]}]}}));
    let added = run(&mut s, ADD, json!({"view": ids::encode(view), "edge": {"point": [20, 40, 0], "path": [leaf]}, "at": [60, 40]}));
    ends_near(&added, [20.0, 20.0, 20.0]);
    ends_near(&run(&mut s, INSPECT, json!({"balloon": added["balloon"]})), [20.0, 20.0, 20.0]);
}

/// A balloon goes on a part of a view that draws an exploded view, and its arrow ends where that view
/// draws the edge: on [`two_cubes`](crate::drawing_scene::tests::two_cubes) with the second moved 30 mm
/// further along x, the second's bottom front edge, (30..40, 0, 0) assembled, has its middle at
/// (35 + 30, 0, 0).
#[test]
fn a_balloon_on_an_exploded_view_ends_where_the_view_draws_its_edge() {
    let (mut s, [_, second]) = crate::drawing_scene::tests::two_cubes();
    run(&mut s, "explode.create", json!({"name": "Apart", "moves": [{"occurrence": second, "translate": [30, 0, 0]}]}));
    run(&mut s, "drawing.create", json!({}));
    let sheet = ids::encode(s.drawing("test").unwrap().sheets[0].id);
    let p = json!({"sheet": sheet, "view": "front", "scale": "1:1", "position_mm": [150, 150], "exploded": "Apart"});
    let view = run(&mut s, "drawing.view.base", p)["view"].clone();
    let added = run(&mut s, ADD, json!({"view": view, "edge": {"point": [35, 0, 0], "path": [second]}, "at": [60, 40]}));
    ends_near(&added, [65.0, 0.0, 0.0]);
    ends_near(&run(&mut s, INSPECT, json!({"balloon": added["balloon"]})), [65.0, 0.0, 0.0]);
}

/// The balloon's group in the reply of `drawing.scene` for the first sheet (`Null` when it isn't
/// drawn), and the reply's word on it (`Null` when it isn't listed).
fn on_sheet(s: &mut Session, balloon: &str) -> (Value, Value) {
    let sheet = ids::encode(s.drawing("test").unwrap().sheets[0].id);
    let reply = s.execute("drawing.scene", &json!({"sheet": sheet})).unwrap();
    let schema = output_schema_for::<crate::schema::drawing_scene::SheetSceneResult>();
    assert!(conforms(schema.as_value(), schema.as_value(), &reply), "{reply}");
    let wanted = ids::decode(balloon).unwrap();
    let group = reply["scene"]["groups"].as_array().unwrap().iter().find(|g| g["object"] == json!(wanted)).cloned();
    let word = reply["balloons"].as_array().into_iter().flatten().find(|b| b["balloon"] == json!(balloon)).cloned();
    (group.unwrap_or(Value::Null), word.unwrap_or(Value::Null))
}

fn items_of<'a>(group: &'a Value, kind: &str) -> Vec<&'a Value> {
    group["items"].as_array().into_iter().flatten().filter(|item| item["kind"] == json!(kind)).collect()
}

/// Where the balloon's arrowhead ends on the sheet.
fn tip_of(group: &Value) -> [f64; 2] {
    let at = &items_of(group, "fill")[0]["region"]["outer"]["segs"][0]["a"];
    [at[0].as_f64().unwrap(), at[1].as_f64().unwrap()]
}

/// The centre of the balloon's circle on the sheet, and its radius.
fn circle_of(group: &Value) -> (Value, Value) {
    let circle = items_of(group, "stroke").into_iter().find(|item| item["closed"] == json!(true)).unwrap();
    (circle["path"][0]["center"].clone(), circle["path"][0]["radius"].clone())
}

fn near(a: [f64; 2], b: [f64; 2]) -> bool {
    (a[0] - b[0]).abs() < 1e-9 && (a[1] - b[1]).abs() < 1e-9
}

/// A balloon draws on its sheet after its view: a circle at its place from the view's centre, and a
/// leader to its edge's middle, worked out by hand. The view is centred on what it shows, 0 to 210
/// along X and 0 to 30 up, so at (200, 150) the middle of the bottom edge, (40, 0, 0), is at
/// (135, 135), and the balloon at (60, 40) from the view's centre is at (260, 190). Its number comes
/// with a list, follows the list, and goes when the list does; moving the view moves it all; an
/// export writes it and says where it stands; a delete takes it off the sheet.
#[test]
fn a_balloon_draws_on_its_sheet_with_its_number_and_its_leader() {
    let (mut s, view, [_, occurrence]) = placed();
    let balloon = add(&mut s, view, occurrence)["balloon"].as_str().unwrap().to_string();

    let (group, word) = on_sheet(&mut s, &balloon);
    let said = (group["kind"].as_str(), &word["drawn"], &word["status"], &word["row_status"]);
    assert_eq!(said, (Some("balloon"), &json!(true), &json!("ok"), &json!("no_list")), "{word}");
    assert!(word["why"].as_str().is_some_and(|why| why.contains("no parts list")), "{word}");
    assert!(items_of(&group, "text").is_empty(), "no number yet: {group}");
    assert_eq!(circle_of(&group), (json!([260.0, 190.0]), json!(5.0)), "{group}");
    assert!(near(tip_of(&group), [135.0, 135.0]), "{group}");

    let table = list(&mut s, json!({}));
    let (group, word) = on_sheet(&mut s, &balloon);
    assert_eq!((&items_of(&group, "text")[0]["text"], &word["item"], &word["row_status"]), (&json!("1"), &json!("1"), &json!("ok")), "{word}");
    assert!(word.get("why").is_none(), "{word}");

    let mut d = (**s.drawing("test").unwrap()).clone();
    Arc::make_mut(d.views.first_mut().unwrap()).place = ViewPlace::At([220.0, 140.0]);
    s.doc_mut().put_drawing(Arc::new(d));
    let (moved, _) = on_sheet(&mut s, &balloon);
    assert!(near(tip_of(&moved), [155.0, 125.0]), "moved with its view: {moved}");
    assert_eq!(circle_of(&moved).0, json!([280.0, 180.0]), "{moved}");

    let exported = s.execute("drawing.export", &json!({"sheet": ids::encode(s.drawing("test").unwrap().sheets[0].id)})).unwrap();
    assert_eq!((&exported["balloons"][0]["drawn"], &exported["balloons"][0]["item"]), (&json!(true), &json!("1")), "{exported}");
    assert!(exported["svg"].as_str().is_some_and(|svg| svg.contains("data-kind=\"balloon\"")), "its kind in the file");

    s.execute("drawing.table.delete", &json!({"table": table})).unwrap();
    let (group, word) = on_sheet(&mut s, &balloon);
    assert!(items_of(&group, "text").is_empty() && word["row_status"] == json!("no_list"), "{word}");

    let drawing = ids::encode(s.open_drawing.unwrap());
    s.execute("selection.delete", &json!({"items": [{"type": "drawing", "drawing": drawing, "object": balloon}]})).unwrap();
    assert_eq!(on_sheet(&mut s, &balloon), (Value::Null, Value::Null), "gone from the sheet and its list");
}

/// A balloon whose edge is gone stays where it is, with its circle and its number and no leader, and
/// says it is lost; put back, its leader returns.
#[test]
fn a_lost_balloon_keeps_its_circle_and_number() {
    let (mut s, view, [_, occurrence]) = placed();
    let balloon = add(&mut s, view, occurrence)["balloon"].as_str().unwrap().to_string();
    list(&mut s, json!({}));
    assert_eq!(items_of(&on_sheet(&mut s, &balloon).0, "fill").len(), 1, "its arrowhead");
    let mut d = (**s.drawing("test").unwrap()).clone();
    let was = d.clone();
    Arc::make_mut(d.balloons.first_mut().unwrap()).edge.name = "gone".into();
    s.doc_mut().put_drawing(Arc::new(d));
    let (group, word) = on_sheet(&mut s, &balloon);
    assert_eq!((&word["status"], &word["drawn"]), (&json!("lost"), &json!(true)), "{word}");
    assert!(items_of(&group, "fill").is_empty(), "no arrowhead: {group}");
    assert_eq!(items_of(&group, "stroke").len(), 1, "only its circle: {group}");
    assert_eq!(items_of(&group, "text")[0]["text"], json!("1"), "{group}");
    assert_eq!(circle_of(&group).0, json!([260.0, 190.0]), "where it was: {group}");
    s.doc_mut().put_drawing(Arc::new(was));
    assert_eq!(items_of(&on_sheet(&mut s, &balloon).0, "fill").len(), 1, "back");
}

/// The sheet reads a balloon's row by the rule `drawing.balloon.inspect` reads it by: the same word
/// and number for no list, a list's row, a reference, a part deleted and a list with no row for it.
#[test]
fn the_sheet_and_inspect_read_a_balloons_row_alike() {
    let (mut s, view, [component, occurrence]) = placed();
    let balloon = add(&mut s, view, occurrence)["balloon"].as_str().unwrap().to_string();
    let drawing = ids::encode(s.open_drawing.unwrap());
    let with_drawing_closed = |s: &mut Session, id: &str, p: Value| {
        s.execute("drawing.close", &json!({})).unwrap();
        s.execute(id, &p).unwrap();
        s.execute("drawing.open", &json!({"drawing": drawing})).unwrap();
    };
    let alike = |s: &mut Session, want: &str| {
        let inspected = read(s, &balloon);
        let (_, word) = on_sheet(s, &balloon);
        assert_eq!((&word["row_status"], word.get("item")), (&inspected["row_status"], inspected.get("item")), "{word} {inspected}");
        assert_eq!(word["row_status"], json!(want), "{word}");
    };
    alike(&mut s, "no_list");
    let first = list(&mut s, json!({}));
    alike(&mut s, "ok");
    with_drawing_closed(&mut s, "occurrence.bom_type", json!({"occurrence": occurrence, "bom_type": "reference"}));
    alike(&mut s, "reference");
    with_drawing_closed(&mut s, "edit.undo", json!({}));
    with_drawing_closed(&mut s, "component.delete", json!({"component": component}));
    alike(&mut s, "not_found");
    with_drawing_closed(&mut s, "edit.undo", json!({}));
    s.execute("drawing.table.delete", &json!({"table": first})).unwrap();
    list(&mut s, json!({"list": {"component": component}}));
    alike(&mut s, "no_row");
}

/// A sheet's scene once nothing is waiting, as the window asks for it.
fn settled(s: &mut Session, sheet: u64) -> crate::drawing_scene::SheetView {
    let mut answer = crate::drawing_scene::extract(s, sheet).unwrap();
    while crate::drawing_scene::step(s) || answer.waiting_for_fonts {
        answer = crate::drawing_scene::extract(s, sheet).unwrap();
    }
    crate::drawing_scene::extract(s, sheet).unwrap()
}

fn built(s: &Session, sheet: u64) -> solidcraft_geom::paper::Scene {
    crate::drawing_scene::build(s, &s.drawing("test").unwrap().clone(), sheet).unwrap().scene
}

/// The provider draws a balloon as a build does, and reads it again only when it or what it reads
/// changes: a new list gives it a number, an edit moves it, and a step with nothing changed reads
/// nothing.
#[test]
fn the_provider_keeps_a_balloon_until_what_it_reads_changes() {
    let (mut s, view, [_, occurrence]) = placed();
    let balloon = ids::decode(add(&mut s, view, occurrence)["balloon"].as_str().unwrap()).unwrap();
    let sheet = s.drawing("test").unwrap().sheets[0].id;
    let first = settled(&mut s, sheet);
    assert_eq!(*first.scene, built(&s, sheet), "as a build draws it");
    assert!(first.stale_balloons.is_empty() && first.balloons.iter().any(|b| b.balloon == balloon && b.drawn));
    let again = settled(&mut s, sheet);
    assert_eq!(again.generation, first.generation, "nothing read again");

    list(&mut s, json!({}));
    let numbered = crate::drawing_scene::extract(&mut s, sheet).unwrap();
    assert_eq!((numbered.stale_balloons.as_slice(), numbered.stale_dimensions.len()), ([balloon].as_slice(), 0), "a list to read");
    let numbered = settled(&mut s, sheet);
    assert_eq!(*numbered.scene, built(&s, sheet));
    assert_ne!(numbered.generation, first.generation);
    assert!(numbered.balloons.iter().any(|b| b.row.item() == Some("1")));

    run(&mut s, EDIT, json!({"balloon": ids::encode(balloon), "set": {"at": [70, 50]}}));
    let moved = settled(&mut s, sheet);
    assert_ne!(moved.generation, numbered.generation);
    assert_eq!(*moved.scene, built(&s, sheet));
}

/// Centre annotations, balloons and their table share the one-shot and retained sheet without
/// losing one another when either annotation changes.
#[test]
fn a_centre_line_and_balloon_keep_their_groups_and_table_in_the_cached_sheet() {
    let (mut s, view, [_, occurrence]) = placed();
    let balloon = add(&mut s, view, occurrence)["balloon"].as_str().unwrap().to_owned();
    let table = list(&mut s, json!({}));
    let line = run(
        &mut s,
        "drawing.center_line.add",
        json!({"view": ids::encode(view), "a": {"point": [0, 0, 0], "path": [occurrence]},
               "b": {"point": [80, 0, 0], "path": [occurrence]}, "overshoot_mm": 2.0}),
    )["line"]
        .as_str()
        .unwrap()
        .to_owned();
    let (balloon_id, line_id, table_id) = (ids::decode(&balloon).unwrap(), ids::decode(&line).unwrap(), ids::decode(&table).unwrap());
    let sheet = s.drawing("test").unwrap().sheets[0].id;
    let group = |scene: &solidcraft_geom::paper::Scene, id| scene.groups.iter().find(|group| group.object == id).unwrap().clone();
    let first = settled(&mut s, sheet);
    assert_eq!(*first.scene, built(&s, sheet));
    assert!(first.balloons.iter().any(|b| b.balloon == balloon_id && b.drawn && b.row.item() == Some("1")));
    assert!(first.tables.iter().any(|t| t.table == table_id && t.drawn));
    let centre = group(&first.scene, line_id);
    let _ = group(&first.scene, balloon_id);
    let _ = group(&first.scene, table_id);

    run(&mut s, EDIT, json!({"balloon": balloon, "set": {"at": [70, 50]}}));
    let moved = settled(&mut s, sheet);
    assert_eq!(*moved.scene, built(&s, sheet));
    assert_eq!(group(&moved.scene, line_id), centre);
    assert_ne!(group(&moved.scene, balloon_id), group(&first.scene, balloon_id));
    assert!(moved.tables.iter().any(|t| t.table == table_id && t.drawn));

    run(&mut s, "drawing.center_line.edit", json!({"line": line, "overshoot_mm": 4.0}));
    let edited = settled(&mut s, sheet);
    assert_eq!(*edited.scene, built(&s, sheet));
    assert_ne!(group(&edited.scene, line_id), centre);
    assert_eq!(group(&edited.scene, balloon_id), group(&moved.scene, balloon_id));
    let exported = s.execute("drawing.export", &json!({"sheet": ids::encode(sheet)})).unwrap();
    let svg = exported["svg"].as_str().unwrap();
    assert!(svg.contains("data-kind=\"balloon\"") && svg.contains("data-kind=\"centre_line\"") && svg.contains("data-kind=\"parts_list\""));
    assert!(exported["balloons"].as_array().unwrap().iter().any(|b| b["balloon"] == json!(balloon) && b["drawn"] == json!(true)));
    assert!(exported["tables"].as_array().unwrap().iter().any(|t| t["table"] == json!(table) && t["drawn"] == json!(true)));
}

/// A held deferred drawing keeps a balloon as it keeps its dimensions: closed during a model change
/// and opened again, it shows the balloon as it was and lists it out of date, and a background
/// export writes the same; updated, the balloon is read again.
#[test]
fn a_held_drawing_keeps_its_balloons_and_exports_them_from_a_scratch_copy() {
    let (mut s, view, [_, occurrence]) = placed();
    let id = ids::decode(add(&mut s, view, occurrence)["balloon"].as_str().unwrap()).unwrap();
    list(&mut s, json!({}));
    let sheet = s.drawing("test").unwrap().sheets[0].id;
    let drawing = json!({"drawing": ids::encode(s.open_drawing.unwrap())});
    run(&mut s, "drawing.set", json!({"updates": "deferred"}));
    run(&mut s, "drawing.update", json!({}));
    let before = settled(&mut s, sheet);
    run(&mut s, "drawing.close", json!({}));
    let more = json!({"length": 10, "width": 10, "height": 10, "corner": [300, 0, 0], "operation": "new", "body_name": "More"});
    run(&mut s, "solid.box", more);
    run(&mut s, "drawing.open", drawing);

    let group = |scene: &solidcraft_geom::paper::Scene| serde_json::to_string(scene.groups.iter().find(|g| g.object == id).unwrap()).unwrap();
    let held = settled(&mut s, sheet);
    assert_eq!(group(&held.scene), group(&before.scene), "as it was");
    let svg = json!({"sheet": ids::encode(sheet), "format": "svg"});
    let there = s.scratch_as_shown().execute("drawing.export", &svg).unwrap();
    let here = run(&mut s, "drawing.export", svg);
    assert!(here["out_of_date"].as_array().unwrap().contains(&json!(ids::encode(id))), "{here}");
    assert_eq!((&there["svg"], &there["out_of_date"], &there["balloons"]), (&here["svg"], &here["out_of_date"], &here["balloons"]));

    run(&mut s, "drawing.update", json!({}));
    let now = settled(&mut s, sheet);
    assert_eq!(*now.scene, built(&s, sheet));
    let listed = run(&mut s, "drawing.export", json!({"sheet": ids::encode(sheet)}));
    assert!(!listed["out_of_date"].as_array().into_iter().flatten().any(|v| *v == json!(ids::encode(id))), "{listed}");
}
