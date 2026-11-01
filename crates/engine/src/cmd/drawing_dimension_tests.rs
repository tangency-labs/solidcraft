use std::collections::BTreeSet;

use serde_json::json;
use solidcraft_doc::drawing::view::{View, ViewKind, ViewPlace};
use solidcraft_doc::drawing_view::{ViewDirection, ViewOverrides};
use solidcraft_geom::Vec3;
use solidcraft_geom::drawing_view::NamedView;

use super::*;
use crate::schema::tests::conforms;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    let reply = s.execute(id, &p).unwrap_or_else(|e| panic!("{id}: {e}"));
    let schema = output_schema_for::<DimensionResult>();
    assert!(conforms(schema.as_value(), schema.as_value(), &reply) || !id.starts_with("drawing.dimension"), "{id}: {reply}");
    reply
}

fn refused(s: &mut Session, p: Value) -> String {
    s.execute(ADD, &p).err().map(|e| e.to_string()).unwrap_or_else(|| panic!("accepted: {p}"))
}

/// A box 80 x 40 x 30 mm along X, Y and Z from the origin, called Box, and a drawing of it, open,
/// with its front view at 1:1: the session and the view's id.
fn front_view() -> (Session, u64) {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 80, "width": 40, "height": 30, "body_name": "Box"}));
    drawn_from_the_front(s)
}

/// A drawing of `s`, open, with its front view at 1:1: the session and the view's id.
fn drawn_from_the_front(mut s: Session) -> (Session, u64) {
    run(&mut s, "drawing.create", json!({}));
    let mut d = (**s.drawing("test").unwrap()).clone();
    let id = d.allocate_id(&mut ids::IdSource::seeded(4896)).unwrap();
    let (sheet, source) = (d.sheets[0].id, d.sources[0].id);
    let kind = ViewKind::Base { source, view: ViewDirection::Named(NamedView::Front) };
    let settings = ViewOverrides { scale: Some("1:1".parse().unwrap()), ..ViewOverrides::default() };
    d.add_views([Arc::new(View { id, sheet, kind, place: ViewPlace::At([200.0, 150.0]), settings, reference_margin_mm: 0.0 })]);
    d.validate().unwrap();
    s.doc_mut().put_drawing(Arc::new(d));
    (s, id)
}

/// The index of the box's edge from `a` to `b`, as `{body, index}` names it.
fn edge_index(s: &Session, a: [f64; 3], b: [f64; 3]) -> usize {
    let (a, b) = (Vec3::from(a), Vec3::from(b));
    let state = s.model.state();
    let mesh = state.body("Box").unwrap().mesh();
    let ends = |line: &Vec<Vec3>| (line[0], line[line.len() - 1]);
    let joins = |(p, q): (Vec3, Vec3)| (p.dist(a) < 1e-9 && q.dist(b) < 1e-9) || (p.dist(b) < 1e-9 && q.dist(a) < 1e-9);
    mesh.edges.iter().position(|line| joins(ends(line))).unwrap()
}

/// The index of `body`'s edge whose every point is `radius` from `centre`, as `{body, index}`
/// names it: one of a circle's or an arc's.
pub(crate) fn round_index(s: &Session, body: &str, centre: [f64; 3], radius: f64) -> usize {
    let centre = Vec3::from(centre);
    let state = s.model.state();
    let mesh = state.body(body).unwrap().mesh();
    mesh.edges.iter().position(|line| line.iter().all(|p| (p.dist(centre) - radius).abs() < 1e-6)).unwrap()
}

fn edit(s: &mut Session, dimension: &Value, set: Value) -> Value {
    s.execute(EDIT, &json!({"dimension": dimension, "set": set})).unwrap_or_else(|e| panic!("{EDIT}: {e}"))
}

fn add(s: &mut Session, view: u64, kind: Value) -> Value {
    run(s, ADD, json!({"view": ids::encode(view), "kind": kind}))
}

fn reads(reply: &Value, value: f64, text: &str) {
    assert_eq!((reply["status"].as_str(), reply["text"].as_str()), (Some("ok"), Some(text)), "{reply}");
    assert!((reply["value"].as_f64().unwrap() - value).abs() < 1e-9, "{reply}");
}

/// A dimension added by points or by edges is saved by the edges' names, and reads as the front
/// view shows the box: its bottom front corners 80 apart, its top front edge 80 long, and that
/// edge square to the left front edge.
#[test]
fn a_dimension_is_added_by_points_or_edges_and_saved_by_name() {
    let (mut s, view) = front_view();
    let steps = s.undo.len();
    let by_points = add(&mut s, view, json!({"linear": {"a": [0, 0, 0], "b": [80, 0, 0], "direction": "horizontal"}}));
    reads(&by_points, 80.0, "80.00");
    assert_eq!((by_points["exact"].clone(), by_points["foreshortened"].clone()), (json!(true), json!(false)));
    assert_eq!(s.undo.len(), steps + 1, "one undo step");
    let record = &by_points["record"];
    assert_eq!((&record["offset_mm"], &record["text"], &record["decimals"]), (&json!(10.0), &json!("centred"), &json!(2)), "the defaults");
    let corner = &record["kind"]["linear"]["a"];
    assert_eq!((corner["body"].as_str(), corner["edges"].as_array().map(Vec::len)), (Some("Box"), Some(3)), "a box corner: three edges");
    assert!(corner.get("point").is_none() && corner.to_string().contains('|'), "names, not the point: {corner}");
    // The same corners by two of their edges each.
    let bottom = edge_index(&s, [0.0, 0.0, 0.0], [80.0, 0.0, 0.0]);
    let (left, right) = (edge_index(&s, [0.0, 0.0, 0.0], [0.0, 0.0, 30.0]), edge_index(&s, [80.0, 0.0, 0.0], [80.0, 0.0, 30.0]));
    let edge = |index: usize| json!({"body": "Box", "index": index});
    let by_edges = add(&mut s, view, json!({"linear": {"a": {"edges": [edge(bottom), edge(left)]}, "b": {"edges": [edge(right), edge(bottom)]}}}));
    reads(&by_edges, 80.0, "80.00");
    // The top front edge's length, by a point on it and by its index, saves the same name.
    let top = edge_index(&s, [0.0, 0.0, 30.0], [80.0, 0.0, 30.0]);
    let at = add(&mut s, view, json!({"length": {"edge": [40, 0, 30]}}));
    let indexed = add(&mut s, view, json!({"length": {"edge": edge(top)}}));
    reads(&at, 80.0, "80.00");
    assert_eq!(at["record"]["kind"], indexed["record"]["kind"]);
    let square = run(&mut s, ADD, json!({"view": ids::encode(view), "kind": {"angle": {"a": [40, 0, 30], "b": [0, 0, 15]}}, "decimals": 1}));
    reads(&square, 90.0, "90.0°");
    // Inspect reads each one back as add answered it.
    let id = by_points["dimension"].clone();
    assert_eq!(run(&mut s, INSPECT, json!({"dimension": id})), by_points);
    // The view lists them, in id order.
    let mut added: Vec<u64> =
        [&by_points, &by_edges, &at, &indexed, &square].iter().filter_map(|r| r["dimension"].as_str().and_then(ids::decode)).collect();
    added.sort_unstable();
    let listed = run(&mut s, "drawing.view.inspect", json!({"view": ids::encode(view)}))["dimensions"].clone();
    assert_eq!(listed, json!(added.into_iter().map(ids::encode).collect::<Vec<_>>()));
}

/// An undone add takes the dimension out, and a redo puts it back.
#[test]
fn an_added_dimension_is_one_undo_step() {
    let (mut s, view) = front_view();
    let before = s.drawing("test").unwrap().to_json().unwrap();
    let p = json!({"view": ids::encode(view), "kind": {"length": {"edge": [40, 0, 30]}}, "offset_mm": -12.5, "text": {"at": [0, 40]}});
    let added = run(&mut s, ADD, p);
    let after = s.drawing("test").unwrap().to_json().unwrap();
    assert_eq!((added["record"]["offset_mm"].clone(), added["record"]["text"].clone()), (json!(-12.5), json!({"at": [0.0, 40.0]})));
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(s.drawing("test").unwrap().to_json().unwrap(), before);
    run(&mut s, "edit.redo", json!({}));
    assert_eq!(s.drawing("test").unwrap().to_json().unwrap(), after);
}

/// A copied view's dimensions are copies with ids of their own, measuring the same, and one undo
/// step takes the copy and them out.
#[test]
fn a_copied_view_takes_copies_of_its_dimensions() {
    let (mut s, view) = front_view();
    let original = run(&mut s, ADD, json!({"view": ids::encode(view), "kind": {"length": {"edge": [40, 0, 30]}}, "text": {"at": [0, 25]}}));
    let before = s.drawing("test").unwrap().to_json().unwrap();
    let reply = run(&mut s, "drawing.view.copy_to_sheet", json!({"view": ids::encode(view), "sheet": "Sheet 1", "position_mm": [100, 100]}));
    let copy = reply["view"].clone();
    let listed = run(&mut s, "drawing.view.inspect", json!({"view": copy}))["dimensions"].clone();
    let [copied] = listed.as_array().map(Vec::as_slice).unwrap_or_default() else { panic!("one copied dimension: {listed}") };
    assert_ne!(copied, &original["dimension"]);
    let read = run(&mut s, INSPECT, json!({"dimension": copied}));
    reads(&read, 80.0, "80.00");
    assert_eq!(read["view"], copy);
    let (mine, theirs) = (&read["record"], &original["record"]);
    assert_eq!((&mine["kind"], &mine["text"], &mine["offset_mm"]), (&theirs["kind"], &theirs["text"], &theirs["offset_mm"]));
    // The original keeps its own.
    assert_eq!(run(&mut s, "drawing.view.inspect", json!({"view": ids::encode(view)}))["dimensions"], json!([original["dimension"]]));
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(s.drawing("test").unwrap().to_json().unwrap(), before);
}

/// An edit changes only what it is given and names only what changed; the same values again
/// change nothing and add no undo step, and `null` shows the number again.
#[test]
fn an_edit_names_what_it_changed_and_the_same_again_changes_nothing() {
    let (mut s, view) = front_view();
    let added = add(&mut s, view, json!({"length": {"edge": [40, 0, 30]}}));
    let id = added["dimension"].clone();
    let steps = s.undo.len();
    let set = json!({"offset_mm": -15, "text": {"at": [5, 30]}, "decimals": 1, "override_text": "SEE NOTE", "reading": "true"});
    assert_eq!(edit(&mut s, &id, set.clone())["changed"], json!(["offset_mm", "text", "decimals", "override_text", "reading"]));
    assert_eq!(s.undo.len(), steps + 1, "one undo step");
    let read = run(&mut s, INSPECT, json!({"dimension": id}));
    assert_eq!((read["status"].as_str(), read["text"].as_str()), (Some("overridden"), Some("SEE NOTE")), "{read}");
    assert_eq!(read["record"]["kind"]["length"]["edge"], added["record"]["kind"]["length"]["edge"], "what it measures stays");
    let bytes = s.drawing("test").unwrap().to_json().unwrap();
    assert_eq!(edit(&mut s, &id, set)["changed"], json!([]));
    assert_eq!((s.undo.len(), s.drawing("test").unwrap().to_json().unwrap()), (steps + 1, bytes), "nothing changed: no undo step");
    assert_eq!(edit(&mut s, &id, json!({"override_text": null}))["changed"], json!(["override_text"]));
    reads(&run(&mut s, INSPECT, json!({"dimension": id})), 80.0, "80.0");
}

/// A radial dimension on a hole through the box is a diameter by default (its rim is two halves,
/// each half a turn), and on a fillet's quarter arc a radius; as asked, the hole reads its radius,
/// and an edit turns one into the other and back. It takes no direction, reading or side.
#[test]
fn a_radial_dimension_reads_a_hole_and_an_edit_makes_it_a_diameter() {
    let (mut s, view) = front_view();
    let drawing = s.drawing("test").unwrap().id;
    run(&mut s, "drawing.close", json!({}));
    run(&mut s, "solid.cylinder", json!({"radius": 6, "height": 42, "base": [40, -1, 15], "axis": "Y", "operation": "cut", "targets": ["Box"]}));
    run(&mut s, "solid.fillet", json!({"edges": [[80, 20, 30]], "radius": 5, "body": "Box"}));
    run(&mut s, "drawing.open", json!({"drawing": ids::encode(drawing)}));
    let hole = json!({"body": "Box", "index": round_index(&s, "Box", [40.0, 0.0, 15.0], 6.0)});
    let across = add(&mut s, view, json!({"radial": {"edge": hole}}));
    reads(&across, 12.0, "Ø12.00");
    assert_eq!(across["record"]["kind"]["radial"]["diameter"], json!(true), "{across}");
    let fillet = json!({"body": "Box", "index": round_index(&s, "Box", [75.0, 0.0, 25.0], 5.0)});
    let arc = add(&mut s, view, json!({"radial": {"edge": fillet}}));
    reads(&arc, 5.0, "R5.00");
    assert_eq!(arc["record"]["kind"]["radial"].get("diameter"), None, "{arc}");
    reads(&add(&mut s, view, json!({"radial": {"edge": fillet, "diameter": true}})), 10.0, "Ø10.00");
    let radius = add(&mut s, view, json!({"radial": {"edge": hole, "diameter": false}}));
    reads(&radius, 6.0, "R6.00");
    let id = radius["dimension"].clone();
    assert_eq!(edit(&mut s, &id, json!({"diameter": true}))["changed"], json!(["diameter"]));
    reads(&run(&mut s, INSPECT, json!({"dimension": id})), 12.0, "Ø12.00");
    assert_eq!(edit(&mut s, &id, json!({"diameter": false}))["changed"], json!(["diameter"]));
    reads(&run(&mut s, INSPECT, json!({"dimension": id})), 6.0, "R6.00");
    let before = s.drawing("test").unwrap().to_json().unwrap();
    for set in [json!({"direction": "horizontal"}), json!({"reading": "true"}), json!({"other_side": true})] {
        let p = json!({"dimension": id, "set": set});
        let e = s.execute(EDIT, &p).err().map(|e| e.to_string()).unwrap_or_else(|| panic!("accepted: {p}"));
        assert!(e.contains("a radial dimension has only `diameter`"), "{p}: {e}");
    }
    assert_eq!(s.drawing("test").unwrap().to_json().unwrap(), before);
}

/// A point on a curved edge names it wherever it falls between the edge's chords, a little way off it
/// names nothing, and a point on a straight edge is that edge however near a curved one is.
#[test]
fn a_point_on_a_curved_edge_finds_it_between_its_chords() {
    let (mut s, view) = front_view();
    let drawing = s.drawing("test").unwrap().id;
    run(&mut s, "drawing.close", json!({}));
    run(&mut s, "solid.cylinder", json!({"radius": 6, "height": 42, "base": [40, -1, 15], "axis": "Y", "operation": "cut", "targets": ["Box"]}));
    run(&mut s, "solid.fillet", json!({"edges": [[80, 20, 30]], "radius": 5, "body": "Box"}));
    run(&mut s, "drawing.open", json!({"drawing": ids::encode(drawing)}));
    let rim = |radius: f64, degrees: f64| {
        let (sin, cos) = degrees.to_radians().sin_cos();
        json!([40.0 + radius * cos, 0.0, 15.0 + radius * sin])
    };
    // The hole's rim is two half circles, and each point names one of them. Whole steps never land on
    // where they meet, which names both and is refused.
    let mut named = BTreeSet::new();
    for step in 0..55 {
        let at = add(&mut s, view, json!({"radial": {"edge": rim(6.0, 3.3 + 6.5 * f64::from(step))}}));
        reads(&at, 12.0, "Ø12.00");
        named.insert(at["record"]["kind"]["radial"]["edge"]["name"].as_str().unwrap().to_string());
    }
    assert_eq!(named.len(), 2, "{named:?}");
    assert!(refused(&mut s, json!({"view": ids::encode(view), "kind": {"radial": {"edge": rim(7.0, 20.0)}}})).contains("no edge"));
    let (sin, cos) = 45f64.to_radians().sin_cos();
    reads(&add(&mut s, view, json!({"radial": {"edge": [75.0 + 5.0 * cos, 0.0, 25.0 + 5.0 * sin]}})), 5.0, "R5.00");
    reads(&add(&mut s, view, json!({"length": {"edge": [74.99, 0, 30]}})), 75.0, "75.00");
}

/// A linear dimension measures from the centre of a hole or an arc, each named by its edge: the
/// hole through the box about (40, 15) on the front, and the fillet's quarter arc about (75, 25).
/// A straight edge has no centre, and a centre's edge has no path of its own.
#[test]
fn a_linear_dimension_measures_from_a_holes_centre() {
    let (mut s, view) = front_view();
    let drawing = s.drawing("test").unwrap().id;
    run(&mut s, "drawing.close", json!({}));
    run(&mut s, "solid.cylinder", json!({"radius": 6, "height": 42, "base": [40, -1, 15], "axis": "Y", "operation": "cut", "targets": ["Box"]}));
    run(&mut s, "solid.fillet", json!({"edges": [[80, 20, 30]], "radius": 5, "body": "Box"}));
    run(&mut s, "drawing.open", json!({"drawing": ids::encode(drawing)}));
    let centre = |at: [f64; 3], radius: f64| json!({"centre": {"body": "Box", "index": round_index(&s, "Box", at, radius)}});
    let (hole, arc) = (centre([40.0, 0.0, 15.0], 6.0), centre([75.0, 0.0, 25.0], 5.0));
    let along = add(&mut s, view, json!({"linear": {"a": [0, 0, 0], "b": hole, "direction": "horizontal"}}));
    reads(&along, 40.0, "40.00");
    let saved = &along["record"]["kind"]["linear"]["b"];
    assert_eq!((saved["centre"].clone(), saved["edges"].as_array().map(Vec::len)), (json!(true), Some(1)), "{saved}");
    reads(&add(&mut s, view, json!({"linear": {"a": [0, 0, 0], "b": hole, "direction": "vertical"}})), 15.0, "15.00");
    reads(&add(&mut s, view, json!({"linear": {"a": hole, "b": arc, "direction": "horizontal"}})), 35.0, "35.00");
    reads(&add(&mut s, view, json!({"linear": {"a": hole, "b": arc, "direction": "vertical"}})), 10.0, "10.00");
    let before = s.drawing("test").unwrap().to_json().unwrap();
    let v = ids::encode(view);
    let linear = |a: Value| json!({"view": v, "kind": {"linear": {"a": a, "b": [80, 0, 0]}}});
    let cases = [
        (linear(json!({"centre": [40, 0, 30]})), "isn't a circle or an arc: a centre is a circle's or an arc's"),
        (linear(json!({"centre": {"point": [40, 0, 30], "path": [1]}})), "{centre: a circle or an arc}"),
        (linear(json!({"centre": [40, 0, 30], "edges": [[40, 0, 30], [0, 0, 15]]})), "{centre: a circle or an arc}"),
        (linear(json!({"centre": [40, 20, 15]})), "no edge at [40, 20, 15]"),
    ];
    for (p, why) in cases {
        let e = refused(&mut s, p.clone());
        assert!(e.contains(why), "{p}: {e}");
    }
    assert_eq!(s.drawing("test").unwrap().to_json().unwrap(), before);
}

/// A chain from the box's bottom front corner through the centres of holes at 10, 30 and 60 along
/// it reads 10, 20 and 30, adding up to its overall 60 within 1e-9, and shows its members' numbers,
/// not one of its own. Its direction is its one setting. It keeps its rules: its points in order
/// along its line, 3 to 64 of them (counted before any is looked up), and no text instead.
#[test]
fn a_chain_reads_holes_at_10_30_and_60_as_10_20_and_30() {
    let (mut s, view) = front_view();
    let drawing = s.drawing("test").unwrap().id;
    run(&mut s, "drawing.close", json!({}));
    for x in [10, 30, 60] {
        run(&mut s, "solid.cylinder", json!({"radius": 3, "height": 42, "base": [x, -1, 15], "axis": "Y", "operation": "cut", "targets": ["Box"]}));
    }
    run(&mut s, "drawing.open", json!({"drawing": ids::encode(drawing)}));
    let hole = |x: f64| json!({"centre": {"body": "Box", "index": round_index(&s, "Box", [x, 0.0, 15.0], 3.0)}});
    let (h10, h30, h60) = (hole(10.0), hole(30.0), hole(60.0));
    let steps = s.undo.len();
    let chain = add(&mut s, view, json!({"chain": {"points": [[0, 0, 0], h10, h30, h60], "direction": "horizontal"}}));
    assert_eq!(s.undo.len(), steps + 1, "one undo step");
    assert_eq!((chain["status"].as_str(), chain.get("text")), (Some("ok"), None), "{chain}");
    let members = |reply: &Value| {
        let listed = reply["members"].as_array().cloned().unwrap_or_default();
        listed.iter().map(|m| (m["value"].as_f64().unwrap_or(f64::NAN), m["text"].as_str().unwrap_or("").to_string())).collect::<Vec<_>>()
    };
    let read = members(&chain);
    assert_eq!(read.iter().map(|(_, text)| text.as_str()).collect::<Vec<_>>(), ["10.00", "20.00", "30.00"]);
    assert!(read.iter().zip([10.0, 20.0, 30.0]).all(|((value, _), part)| (value - part).abs() < 1e-9), "{read:?}");
    let overall = chain["value"].as_f64().unwrap();
    assert!((overall - 60.0).abs() < 1e-9 && (read.iter().map(|(value, _)| value).sum::<f64>() - overall).abs() < 1e-9, "{chain}");
    assert_eq!(chain["record"]["kind"]["chain"]["points"].as_array().map(Vec::len), Some(4));
    // A direction given against the chain's way reads the same.
    let id = chain["dimension"].clone();
    assert_eq!(edit(&mut s, &id, json!({"direction": {"along": [-1, 0]}}))["changed"], json!(["direction"]));
    assert_eq!(members(&run(&mut s, INSPECT, json!({"dimension": id}))), read);
    let before = s.drawing("test").unwrap().to_json().unwrap();
    for (set, why) in [
        (json!({"reading": "true"}), "a chain has only `direction`"),
        (json!({"other_side": true}), "a chain has only `direction`"),
        (json!({"diameter": true}), "`diameter` is a radial dimension's"),
        (json!({"override_text": "60"}), "a chain shows its members' numbers, never a text instead"),
    ] {
        let p = json!({"dimension": id, "set": set});
        let e = s.execute(EDIT, &p).err().map(|e| e.to_string()).unwrap_or_else(|| panic!("accepted: {p}"));
        assert!(e.contains(why), "{p}: {e}");
    }
    let v = ids::encode(view);
    let chained = |points: Value| json!({"view": v, "kind": {"chain": {"points": points, "direction": "horizontal"}}});
    let nowhere = json!([1e6, 1e6, 1e6]);
    let cases = [
        (chained(json!([h10, h60, h30])), "a chain's points run one way along its line"),
        (chained(json!([h10, h60])), "a chain has 3 to 64 points"),
        (chained(Value::Array(vec![nowhere.clone(); 65])), "a chain has 3 to 64 points"),
        (chained(json!([h10, nowhere, h60])), "no corner at"),
        (json!({"view": v, "kind": {"chain": {"points": [h10, h30, h60], "reading": "true"}}}), "unknown field `reading`"),
        (json!({"view": v, "kind": {"chain": {"points": [h10, h30, h60]}}, "override_text": "60"}), "never a text instead"),
    ];
    for (p, why) in cases {
        let e = refused(&mut s, p.clone());
        assert!(e.contains(why), "{p}: {e}");
    }
    assert_eq!(s.drawing("test").unwrap().to_json().unwrap(), before);
}

/// A cylinder's diameter is read between its silhouettes: a boss of radius 5 along X on the box's
/// right face, seen side on from the front, reads Ø10.00, picked by a point on it or by its body
/// and index, and saved by its face's name; it takes no settings. A flat face, a fillet's quarter
/// round (one of its silhouettes isn't on it) and a hole the view sees end-on are refused, with
/// the drawing unchanged.
#[test]
fn a_cylinders_diameter_is_read_between_its_silhouettes() {
    let (mut s, view) = front_view();
    let drawing = s.drawing("test").unwrap().id;
    run(&mut s, "drawing.close", json!({}));
    run(&mut s, "solid.cylinder", json!({"radius": 5, "height": 20, "base": [80, 20, 15], "axis": "X", "operation": "join", "targets": ["Box"]}));
    run(&mut s, "solid.fillet", json!({"edges": [[40, 0, 30]], "radius": 4, "body": "Box"}));
    run(&mut s, "solid.cylinder", json!({"radius": 3, "height": 42, "base": [20, -1, 15], "axis": "Y", "operation": "cut", "targets": ["Box"]}));
    run(&mut s, "drawing.open", json!({"drawing": ids::encode(drawing)}));
    // 30° round from the boss's top, away from where its two half faces meet.
    let on_boss = [90.0, 22.5, 15.0 + 5.0 * 30f64.to_radians().cos()];
    let boss = add(&mut s, view, json!({"cylinder": {"face": on_boss}}));
    reads(&boss, 10.0, "Ø10.00");
    assert_eq!((boss["exact"].clone(), boss["foreshortened"].clone()), (json!(true), json!(false)), "{boss}");
    let saved = boss["record"]["kind"]["cylinder"]["face"].clone();
    assert!(saved["body"] == "Box" && saved["name"].as_str().is_some_and(|n| !n.is_empty()), "{saved}");
    let index = {
        let state = s.model.state();
        solidcraft_doc::appearance::face_index_at(state.body("Box").unwrap(), Vec3::from(on_boss)).unwrap()
    };
    let by_index = add(&mut s, view, json!({"cylinder": {"face": {"body": "Box", "index": index}}}));
    assert_eq!(by_index["record"]["kind"], boss["record"]["kind"]);
    let id = boss["dimension"].clone();
    let before = s.drawing("test").unwrap().to_json().unwrap();
    for (set, why) in [
        (json!({"direction": "horizontal"}), "a cylinder's diameter has no direction, reading or side"),
        (json!({"reading": "true"}), "a cylinder's diameter has no direction, reading or side"),
        (json!({"diameter": false}), "`diameter` is a radial dimension's"),
    ] {
        let p = json!({"dimension": id, "set": set});
        let e = s.execute(EDIT, &p).err().map(|e| e.to_string()).unwrap_or_else(|| panic!("accepted: {p}"));
        assert!(e.contains(why), "{p}: {e}");
    }
    let v = ids::encode(view);
    let cylinder = |face: Value| json!({"view": v, "kind": {"cylinder": {"face": face}}});
    let on_fillet = [40.0, 4.0 - 4.0 * 45f64.to_radians().cos(), 26.0 + 4.0 * 45f64.to_radians().sin()];
    let cases = [
        (cylinder(json!([40, 0, 10])), "isn't a cylinder"),
        (cylinder(json!(on_fillet)), "isn't a whole cylinder in this view"),
        (cylinder(json!([23, 20, 15])), "the cylinder is seen end-on in this view"),
        (cylinder(json!([200, 200, 200])), "no face at [200, 200, 200]"),
        (cylinder(json!({"body": "Box", "index": 9999})), "body `Box` has no face 9999"),
        (cylinder(json!({"body": "Box"})), "a face is a point on it"),
        (cylinder(json!({"body": 7, "index": 0})), "a face's `body` is its name"),
    ];
    for (p, why) in cases {
        let e = refused(&mut s, p.clone());
        assert!(e.contains(why), "{p}: {e}");
    }
    assert_eq!(s.drawing("test").unwrap().to_json().unwrap(), before);
}

/// Two bosses along X on the box's right face, radius 5 at z = 15 and radius 3 at z = 25, in the
/// front view: near to near 2, near to far 8, far to near 12 and far to far 18, saved by each
/// face's name and side; up the view the same. Two faces of one boss make its Ø10, which takes no
/// direction. Its edit takes a direction only. Refused, with the drawing unchanged: a flat face, a
/// fillet's quarter round on the side its silhouette isn't on, a hole seen end-on, and a side that
/// is neither near nor far.
#[test]
fn two_cylinders_read_between_the_silhouettes_given() {
    let (mut s, view) = front_view();
    let drawing = s.drawing("test").unwrap().id;
    run(&mut s, "drawing.close", json!({}));
    for (radius, z) in [(5, 15), (3, 25)] {
        let boss = json!({"radius": radius, "height": 20, "base": [80, 20, z], "axis": "X", "operation": "join", "targets": ["Box"]});
        run(&mut s, "solid.cylinder", boss);
    }
    run(&mut s, "solid.fillet", json!({"edges": [[40, 0, 30]], "radius": 4, "body": "Box"}));
    run(&mut s, "solid.cylinder", json!({"radius": 3, "height": 42, "base": [20, -1, 15], "axis": "Y", "operation": "cut", "targets": ["Box"]}));
    run(&mut s, "drawing.open", json!({"drawing": ids::encode(drawing)}));
    // Each 30° round from its boss's top, away from where its two half faces meet.
    let on = |r: f64, z: f64| json!([90.0, 20.0 + r * 30f64.to_radians().sin(), z + r * 30f64.to_radians().cos()]);
    let (big, small) = (on(5.0, 15.0), on(3.0, 25.0));
    let between = |a: &str, b: &str| json!({"silhouettes": {"a": {"face": big, "side": a}, "b": {"face": small, "side": b}}});
    for (a, b, value) in [("near", "near", 2.0), ("near", "far", 8.0), ("far", "near", 12.0), ("far", "far", 18.0)] {
        let added = add(&mut s, view, between(a, b));
        reads(&added, value, &format!("{value:.2}"));
        assert_eq!((added["exact"].clone(), added["foreshortened"].clone()), (json!(true), json!(false)), "{added}");
        let saved = &added["record"]["kind"]["silhouettes"];
        assert!(saved["a"]["side"] == a && saved["b"]["side"] == b && saved["direction"] == "aligned", "{saved}");
        assert_eq!(saved["a"]["face"]["body"], "Box", "{saved}");
        let up = edit(&mut s, &added["dimension"], json!({"direction": "vertical"}));
        assert_eq!(up["changed"], json!(["direction"]), "{up}");
        reads(&run(&mut s, INSPECT, json!({"dimension": added["dimension"]})), value, &format!("{value:.2}"));
    }
    let other_half = on(-5.0, 15.0);
    let one = add(&mut s, view, json!({"silhouettes": {"a": {"face": big, "side": "near"}, "b": {"face": other_half, "side": "far"}}}));
    reads(&one, 10.0, "Ø10.00");
    assert!(one["record"]["kind"]["cylinder"]["face"]["name"].is_string(), "{one}");
    let id = add(&mut s, view, between("near", "near"))["dimension"].clone();
    let before = s.drawing("test").unwrap().to_json().unwrap();
    let e = s.execute(EDIT, &json!({"dimension": id, "set": {"reading": "true"}})).err().map(|e| e.to_string()).unwrap_or_default();
    assert!(e.contains("only `direction`"), "{e}");
    let v = ids::encode(view);
    let with =
        |b: Value, side: &str| json!({"view": v, "kind": {"silhouettes": {"a": {"face": big, "side": "near"}, "b": {"face": b, "side": side}}}});
    let on_fillet = [40.0, 4.0 - 4.0 * 45f64.to_radians().cos(), 26.0 + 4.0 * 45f64.to_radians().sin()];
    let halves = json!({"view": v, "kind": {"silhouettes": {"a": {"face": big, "side": "near"}, "b": {"face": other_half, "side": "far"},
        "direction": "vertical"}}});
    let cases = [
        (halves, "takes no direction"),
        (with(json!([40, 0, 10]), "near"), "isn't a cylinder"),
        (with(json!(on_fillet), "near"), "near silhouette in this view isn't on it"),
        (with(json!([23, 20, 15]), "near"), "the cylinder is seen end-on in this view"),
        (with(small.clone(), "left"), "unknown variant `left`"),
    ];
    for (p, why) in cases {
        let e = refused(&mut s, p.clone());
        assert!(e.contains(why), "{p}: {e}");
    }
    assert_eq!(s.drawing("test").unwrap().to_json().unwrap(), before);
}

/// A dimension whose edge is filleted away has no value, and gives the one it last read this
/// session as its last good value.
#[test]
fn a_dimension_that_cant_be_measured_gives_its_last_good_value() {
    let (mut s, view) = front_view();
    let drawing = s.drawing("test").unwrap().id;
    let added = add(&mut s, view, json!({"length": {"edge": [40, 0, 30]}}));
    reads(&added, 80.0, "80.00");
    assert!(added.get("last_good").is_none(), "measured now: {added}");
    run(&mut s, "drawing.close", json!({}));
    run(&mut s, "solid.fillet", json!({"body": "Box", "radius": 5, "edges": [[40, 0, 30]]}));
    run(&mut s, "drawing.open", json!({"drawing": ids::encode(drawing)}));
    let read = run(&mut s, INSPECT, json!({"dimension": added["dimension"]}));
    assert!(read.get("value").is_none() && read["status"] != json!("ok"), "{read}");
    assert_eq!(read["last_good"], json!(80.0), "{read}");
    // A read isn't in the document: a session on it, with no file in between, has none.
    let mut fresh = Session::new((*s.doc).clone());
    run(&mut fresh, "drawing.open", json!({"drawing": ids::encode(drawing)}));
    assert!(run(&mut fresh, INSPECT, json!({"dimension": added["dimension"]})).get("last_good").is_none());
}

/// A setting its kind hasn't got, and a value the record would refuse, are refused with the
/// drawing unchanged; an angle takes its other side.
#[test]
fn an_edit_refuses_what_the_dimension_cant_take() {
    let (mut s, view) = front_view();
    let length = add(&mut s, view, json!({"length": {"edge": [40, 0, 30]}}))["dimension"].clone();
    let angle = add(&mut s, view, json!({"angle": {"a": [40, 0, 30], "b": [0, 0, 15]}}))["dimension"].clone();
    let before = s.drawing("test").unwrap().to_json().unwrap();
    let nothing = json!(ids::encode(99));
    let cases = [
        (&length, json!({"other_side": true}), "`other_side` is an angle's"),
        (&angle, json!({"direction": "horizontal"}), "an angle has no `direction`"),
        (&length, json!({"diameter": true}), "`diameter` is a radial dimension's"),
        (&length, json!({"decimals": 7}), "at most 6 decimals"),
        (&length, json!({"offset_mm": 1e9}), "on paper"),
        (&length, json!({"override_text": ""}), "instead of its number is 1 to"),
        (&length, json!({"offset_mm": null}), ""),
        (&length, json!({"edge": [0, 0, 0]}), "edge"),
        (&nothing, json!({"decimals": 1}), "the drawing has no dimension"),
    ];
    for (id, set, why) in cases {
        let p = json!({"dimension": id, "set": set});
        let e = s.execute(EDIT, &p).err().map(|e| e.to_string()).unwrap_or_else(|| panic!("accepted: {p}"));
        assert!(e.contains(why), "{p}: {e}");
    }
    assert_eq!(s.drawing("test").unwrap().to_json().unwrap(), before);
    assert_eq!(edit(&mut s, &angle, json!({"other_side": true}))["changed"], json!(["other_side"]));
    assert_eq!(run(&mut s, INSPECT, json!({"dimension": angle}))["record"]["kind"]["angle"]["other_side"], json!(true));
}

/// A pick that doesn't name one thing, and a dimension the record would refuse, are refused
/// with the drawing unchanged.
#[test]
fn a_pick_that_names_nothing_or_several_things_is_refused() {
    let (mut s, view) = front_view();
    let before = s.drawing("test").unwrap().to_json().unwrap();
    let v = ids::encode(view);
    let length = |edge: Value| json!({"view": v, "kind": {"length": {"edge": edge}}});
    let linear = |a: Value| json!({"view": v, "kind": {"linear": {"a": a, "b": [80, 0, 0]}}});
    let cases = [
        (length(json!([40, 20, 15])), "no edge at [40, 20, 15]"),
        (length(json!([0, 0, 0])), "[0, 0, 0] is on 3 edges"),
        (length(json!({"body": "Nope", "index": 0})), "no body `Nope`"),
        (length(json!({"body": "Box", "index": 999})), "body `Box` has no edge 999"),
        (length(json!({"point": [40, 0, 30], "index": 1})), "an edge is a point on it"),
        (linear(json!([40, 0, 0])), "no corner at [40, 0, 0]"),
        (linear(json!({"edges": [{"point": [40, 0, 0]}, {"point": [40, 40, 30]}]})), "the corner's edges don't meet"),
        (linear(json!({"edges": [{"point": [40, 0, 0], "path": [1]}]})), "each without a path of its own"),
        (json!({"view": v, "kind": {"length": {"edge": [40, 0, 30]}}, "decimals": 7}), "at most 6 decimals"),
        (json!({"view": v, "kind": {"length": {"edge": [40, 0, 30]}}, "offset_mm": 1e9}), "on paper"),
        (json!({"view": ids::encode(99), "kind": {"length": {"edge": [40, 0, 30]}}}), "the drawing has no view"),
        (json!({"view": v, "kind": {"radius": {"edge": [40, 0, 30]}}}), "radius"),
        (json!({"view": v, "kind": {"radial": {"edge": [40, 0, 30]}}}), "isn't a circle or an arc"),
        (json!({"view": v, "kind": {"radial": {"edge": [0, 20, 0]}}}), "isn't a circle in this view (it isn't parallel to it)"),
        (json!({"view": v, "kind": {"radial": {"edge": [40, 0, 30], "diameter": 1}}}), "must be true or false"),
    ];
    for (p, why) in cases {
        let e = refused(&mut s, p.clone());
        assert!(e.contains(why), "{p}: {e}");
    }
    assert_eq!(s.drawing("test").unwrap().to_json().unwrap(), before);
    let e = s.execute(INSPECT, &json!({"dimension": ids::encode(99)})).err().map(|e| e.to_string()).unwrap_or_default();
    assert!(e.contains("the drawing has no dimension"), "{e}");
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

/// A hostile value in each place of each command's parameters, at every depth (picks, paths,
/// corners' edges, a chain's points, the edit's settings): never a panic, and the drawing keeps its
/// rules.
#[test]
fn hostile_dimension_params_never_panic() {
    let (mut s, view) = front_view();
    let id = add(&mut s, view, json!({"length": {"edge": [40, 0, 30]}}))["dimension"].clone();
    // Aligned from the first corner to the last, each corner past the one before.
    let chain = add(&mut s, view, json!({"chain": {"points": [[0, 0, 0], [80, 0, 0], [80, 0, 30]]}}))["dimension"].clone();
    let v = json!(ids::encode(view));
    let corner = json!({"edges": [{"body": "Box", "index": 0}, {"point": [0, 0, 15]}], "path": [1]});
    let calls = [
        (
            ADD,
            json!({"view": v, "kind": {"linear": {"a": [0, 0, 0], "b": corner, "direction": {"along": [1, 0]}, "reading": "projected"}},
            "offset_mm": 5, "text": {"at": [1, 2]}, "decimals": 1, "override_text": "x"}),
        ),
        (ADD, json!({"view": v, "kind": {"angle": {"a": {"point": [40, 0, 30], "path": [1]}, "b": [0, 0, 15], "other_side": true}}})),
        (ADD, json!({"view": v, "kind": {"radial": {"edge": {"body": "Box", "index": 0, "path": [1]}, "diameter": true}}})),
        (
            ADD,
            json!({"view": v, "kind": {"chain": {"points": [[0, 0, 0], {"centre": {"body": "Box", "index": 0}, "path": [1]}, corner, [80, 0, 0]],
            "direction": "horizontal"}}}),
        ),
        (EDIT, json!({"dimension": chain, "set": {"direction": {"along": [1, 1]}, "decimals": 3}})),
        (ADD, json!({"view": v, "kind": {"cylinder": {"face": {"body": "Box", "index": 0, "path": [1]}}}})),
        (
            ADD,
            json!({"view": v, "kind": {"silhouettes": {"a": {"face": {"body": "Box", "index": 0, "path": [1]}, "side": "near"},
            "b": {"face": [0, 0, 15], "side": "far"}, "direction": {"along": [1, 1]}}}}),
        ),
        (
            EDIT,
            json!({"dimension": id, "set": {"offset_mm": 5, "text": "centred", "decimals": 1, "override_text": null, "direction": "vertical",
            "reading": "projected"}}),
        ),
        (INSPECT, json!({"dimension": id})),
    ];
    let many = |v: Value| Value::Array(vec![v; 1000]);
    let mut values = crate::tests::hostile_values();
    values.extend([
        json!({"body": "Box", "index": u64::MAX}),
        json!({"edges": []}),
        json!({"edges": many(json!([0, 0, 0]))}),
        many(json!(1)),
        json!([1e308, 0, 0]),
        json!("x".repeat(600)),
    ]);
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
    assert!(internal.is_empty(), "dimension commands panicked:\n{}", internal.join("\n"));
}

/// A dimension on a reference part's edge: added by its occurrence, reading its length, and the same
/// after the part stops being reference. The finder doesn't ask what a part's type is.
#[test]
fn a_dimension_can_measure_a_reference_parts_edge() {
    let mut s = Session::default();
    let block = run(&mut s, "component.create", json!({"name": "Block"}))["occurrence"].clone();
    run(&mut s, "solid.box", json!({"length": 20, "width": 30, "height": 10, "body_name": "Block", "operation": "new"}));
    run(&mut s, "component.activate", json!({"component": 0}));
    run(&mut s, "component.metadata.set", json!({"component": "Block", "bom_type": "reference"}));
    let (mut s, view) = drawn_from_the_front(s);
    let edge = json!({"point": [10, 0, 10], "path": [block]});
    let added = run(&mut s, ADD, json!({"view": ids::encode(view), "kind": {"length": {"edge": edge}}}));
    assert!((added["value"].as_f64().unwrap() - 20.0).abs() < 1e-9, "{added}");
    let drawing = ids::encode(s.drawing("test").unwrap().id);
    run(&mut s, "drawing.close", json!({}));
    run(&mut s, "component.metadata.set", json!({"component": "Block", "bom_type": "normal"}));
    run(&mut s, "drawing.open", json!({"drawing": drawing}));
    let read = run(&mut s, INSPECT, json!({"dimension": added["dimension"]}));
    assert_eq!((&read["status"], &read["value"]), (&json!("ok"), &added["value"]), "{read}");
}

/// A box with an 80 mm dimension on its top front edge, read once, and the drawing closed: the
/// session, the drawing's id and the dimension's.
fn read_once() -> (Session, u64, u64) {
    let (mut s, view) = front_view();
    let drawing = s.drawing("test").unwrap().id;
    let added = add(&mut s, view, json!({"length": {"edge": [40, 0, 30]}}));
    run(&mut s, "drawing.close", json!({}));
    (s, drawing, ids::decode(added["dimension"].as_str().unwrap()).unwrap())
}

/// Fillets away the edge the [`read_once`] dimension measures.
fn lose_the_edge(s: &mut Session) {
    run(s, "solid.fillet", json!({"body": "Box", "radius": 5, "edges": [[40, 0, 30]]}));
}

/// `s` saved to a file called `name` and opened from it in a new session, with `drawing` open.
fn reopened(s: &mut Session, drawing: u64, name: &str) -> Session {
    let dir = std::env::temp_dir().join(format!("solidcraft-last-good-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    run(s, "file.save_as", json!({"path": path}));
    let mut fresh = Session::default();
    run(&mut fresh, "doc.open", json!({"path": path}));
    run(&mut fresh, "drawing.open", json!({"drawing": ids::encode(drawing)}));
    let _ = std::fs::remove_dir_all(&dir);
    fresh
}

fn inspect_of(s: &mut Session, dimension: u64) -> Value {
    run(s, INSPECT, json!({"dimension": ids::encode(dimension)}))
}

/// A lost dimension still says what it last read after the design is saved and opened again, in
/// either form of the file. It stays lost, with no value and no text, and a read of it leaves
/// nothing to save.
#[test]
fn a_lost_dimension_keeps_its_last_good_value_after_reopening() {
    for name in [format!("part.{}", solidcraft_io::DESIGN_EXT), "part.json".to_string()] {
        let (mut s, drawing, dimension) = read_once();
        lose_the_edge(&mut s);
        let mut opened = reopened(&mut s, drawing, &name);
        let read = inspect_of(&mut opened, dimension);
        assert_eq!(read["status"], json!("lost"), "{name}: {read}");
        assert!(read.get("value").is_none() && read.get("text").is_none(), "{name}, never its value: {read}");
        assert_eq!(read["last_good"], json!(80.0), "{name}: {read}");
        assert!(!opened.is_dirty(), "{name}: a read leaves nothing to save");
    }
}

/// A read changes nothing in the document: no undo step, nothing to save, the same document. Only
/// the file's copy has the value, and a session that read nothing copies nothing.
#[test]
fn a_read_stays_out_of_the_document() {
    let (mut s, drawing, dimension) = read_once();
    run(&mut s, "drawing.open", json!({"drawing": ids::encode(drawing)}));
    let path = std::env::temp_dir().join(format!("solidcraft-last-good-read-{}.json", std::process::id()));
    run(&mut s, "file.save_as", json!({"path": path}));
    let (doc, steps) = (s.doc.clone(), s.undo.len());
    inspect_of(&mut s, dimension);
    assert!(Arc::ptr_eq(&s.doc, &doc) && s.undo.len() == steps && !s.is_dirty());
    assert_eq!(s.doc.drawing(drawing).unwrap().dimension(dimension).unwrap().last_good, None, "only in the file's copy");
    assert_eq!(s.design().drawing(drawing).unwrap().dimension(dimension).unwrap().last_good, Some(80.0));
    let opened = Session::new(solidcraft_io::read_design(&std::fs::read(&path).unwrap()).unwrap());
    let _ = std::fs::remove_file(&path);
    assert!(matches!(opened.design(), std::borrow::Cow::Borrowed(_)), "nothing read: nothing to copy");
    assert_eq!(opened.doc.drawing(drawing).unwrap().dimension(dimension).unwrap().last_good, Some(80.0));
    let bytes = solidcraft_io::write_design(&opened.design());
    assert_eq!(solidcraft_io::write_design(&solidcraft_io::read_design(&bytes).unwrap()), bytes, "save, open, save: the same bytes");
}

/// A saved value is only ever what it last read: measured now, a dimension reads the model and has
/// no last good value, whatever the file said. A newer read replaces the saved one, in the session
/// and through the next save.
#[test]
fn a_saved_last_good_value_never_reads_as_the_value() {
    let (mut s, drawing, dimension) = read_once();
    s.dimension_values.insert((drawing, dimension), 55.5);
    let mut opened = reopened(&mut s, drawing, "seeded.json");
    let read = inspect_of(&mut opened, dimension);
    reads(&read, 80.0, "80.00");
    assert_eq!(read["record"]["last_good"], json!(55.5), "as saved");
    assert!(read.get("last_good").is_none(), "measured now: {read}");
    run(&mut opened, "drawing.close", json!({}));
    lose_the_edge(&mut opened);
    run(&mut opened, "drawing.open", json!({"drawing": ids::encode(drawing)}));
    assert_eq!(inspect_of(&mut opened, dimension)["last_good"], json!(80.0), "this session's read comes first");
    run(&mut opened, "drawing.close", json!({}));
    let mut again = reopened(&mut opened, drawing, "again.json");
    assert_eq!(inspect_of(&mut again, dimension)["last_good"], json!(80.0), "and replaces the saved one");
}

/// An angle saves its last good value in radians and gives it in degrees, as it gives its value.
#[test]
fn a_lost_angle_gives_its_saved_value_in_degrees() {
    let (mut s, view) = front_view();
    let drawing = s.drawing("test").unwrap().id;
    let added = add(&mut s, view, json!({"angle": {"a": [40, 0, 30], "b": [0, 0, 15]}}));
    let dimension = ids::decode(added["dimension"].as_str().unwrap()).unwrap();
    run(&mut s, "drawing.close", json!({}));
    lose_the_edge(&mut s);
    let mut opened = reopened(&mut s, drawing, "angle.json");
    let read = inspect_of(&mut opened, dimension);
    assert_eq!(read["status"], json!("lost"), "{read}");
    assert!((read["record"]["last_good"].as_f64().unwrap() - std::f64::consts::FRAC_PI_2).abs() < 1e-9, "{read}");
    assert!((read["last_good"].as_f64().unwrap() - 90.0).abs() < 1e-9, "{read}");
}

/// A model can read past the largest number a drawing saves: that value isn't saved, so the file
/// still opens, and the dimension has none.
#[test]
fn a_value_past_what_a_drawing_saves_leaves_the_file_able_to_open() {
    let (mut s, drawing, dimension) = read_once();
    s.dimension_values.insert((drawing, dimension), 2e9);
    let mut opened = reopened(&mut s, drawing, "huge.json");
    assert!(inspect_of(&mut opened, dimension)["record"].get("last_good").is_none());
}

/// A background save runs on a scratch copy of the session, and an autosave on its own snapshot:
/// both keep what the dimensions last read, so a recovered design has it too.
#[test]
fn background_saves_and_autosaves_keep_the_last_good_value() {
    let (s, drawing, dimension) = read_once();
    let last_good = |doc: &solidcraft_doc::Document| doc.drawing(drawing).unwrap().dimension(dimension).unwrap().last_good;
    assert_eq!(last_good(&s.scratch().design()), Some(80.0));
    let dir = std::env::temp_dir().join(format!("solidcraft-last-good-autosave-{}", std::process::id()));
    let mut saver = crate::recovery::Autosaver::new(&dir, std::time::Duration::from_secs(3600)).unwrap();
    assert!(saver.save(&s).unwrap());
    drop(saver);
    let found = crate::recovery::orphans(&dir);
    let (doc, _) = crate::recovery::load(&dir, &found[0].id).unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(last_good(&doc), Some(80.0));
}

/// What this session read goes with this design: opening another one starts with none, so it can't
/// reach that design's file, even a copy with the same ids.
#[test]
fn opening_a_design_starts_with_no_last_good_values() {
    let (mut s, _, _) = read_once();
    let path = std::env::temp_dir().join(format!("solidcraft-last-good-open-{}.json", std::process::id()));
    std::fs::write(&path, solidcraft_io::write_design(&s.doc)).unwrap();
    run(&mut s, "doc.open", json!({"path": path}));
    let _ = std::fs::remove_file(&path);
    assert!(s.dimension_values.is_empty());
}

/// A view copied to a sheet brings its dimensions with what they saved: the same references, read
/// on the same view direction.
#[test]
fn a_copied_dimension_keeps_its_saved_value() {
    let (mut s, drawing, dimension) = read_once();
    let mut opened = reopened(&mut s, drawing, "copy.json");
    let view = opened.drawing("test").unwrap().dimension(dimension).unwrap().view;
    run(&mut opened, "drawing.view.copy_to_sheet", json!({"view": ids::encode(view), "sheet": "Sheet 1", "position_mm": [100, 100]}));
    let d = opened.drawing("test").unwrap();
    let copy = d.dimensions.iter().find(|x| x.id != dimension).unwrap();
    assert_eq!(copy.last_good, Some(80.0));
}

/// One command-script plate with two independently authored, positioned bore dimensions.
fn workshop_plate_dimensions() -> (Session, u64, Value, Value) {
    let mut s = Session::default();
    let script: Value = serde_json::from_str(include_str!("../../../../examples/acceptance/drawing_m2.json")).unwrap();
    let made = s.run_script(&script).unwrap();
    let top = made.last().unwrap()["view"].clone();
    let drawing = s.drawing("test").unwrap().id;
    let bore1 = json!({"body": "Plate", "index": round_index(&s, "Plate", [20.0, 20.0, 20.0], 5.0)});
    let bore2 = json!({"body": "Plate", "index": round_index(&s, "Plate", [60.0, 50.0, 20.0], 5.0)});
    let first = run(&mut s, "drawing.dimension.add", json!({"view": top, "kind": {"radial": {"edge": bore1}}, "text": {"at": [12.0, 24.0]}}));
    let second = run(&mut s, "drawing.dimension.add", json!({"view": top, "kind": {"radial": {"edge": bore2}}, "text": {"at": [-18.0, 15.0]}}));
    reads(&first, 10.0, "Ø10.00");
    reads(&second, 10.0, "Ø10.00");
    assert_ne!(first["dimension"], second["dimension"], "each bore needs its own dimension");
    (s, drawing, first, second)
}

/// A lost workshop bore retains its last Ø10 measurement without becoming a live value.
fn reads_lost_bore(reply: &Value) {
    assert_eq!(reply["status"], "lost", "{reply}");
    assert!(reply.get("value").is_none(), "{reply}");
    assert!(reply["why"].as_str().is_some_and(|why| !why.is_empty()), "{reply}");
    assert!(reply["last_good"].as_f64().is_some_and(|value| (value - 10.0).abs() < 1e-9), "last measured diameter must stay Ø10: {reply}");
}

/// Changing Bore1's diameter updates its measurement without rebinding the reference.
#[test]
fn changing_a_bores_diameter_keeps_its_dimension() {
    let (mut s, drawing, first, second) = workshop_plate_dimensions();
    run(&mut s, "drawing.close", json!({}));
    run(&mut s, "parameters.change", json!({"name": "bore1_d", "expression": "12 mm"}));
    run(&mut s, "drawing.open", json!({"drawing": ids::encode(drawing)}));

    let changed = run(&mut s, "drawing.dimension.inspect", json!({"dimension": first["dimension"]}));
    assert_eq!(changed["record"]["kind"], first["record"]["kind"], "Bore1 reference moved");
    reads(&changed, 12.0, "Ø12.00");
    let other = run(&mut s, "drawing.dimension.inspect", json!({"dimension": second["dimension"]}));
    assert_eq!(other["record"]["kind"], second["record"]["kind"], "Bore2 reference moved");
    reads(&other, 10.0, "Ø10.00");
}

/// Deleting Bore1 loses its own measured reference without rebinding it to Bore2.
#[test]
fn deleting_a_bore_loses_its_dimension_and_never_moves_it() {
    let (mut s, drawing, first, second) = workshop_plate_dimensions();
    run(&mut s, "drawing.close", json!({}));
    run(&mut s, "timeline.delete", json!({"feature": "Bore1"}));
    run(&mut s, "drawing.open", json!({"drawing": ids::encode(drawing)}));

    let lost = run(&mut s, "drawing.dimension.inspect", json!({"dimension": first["dimension"]}));
    assert_eq!(lost["record"]["kind"], first["record"]["kind"], "deleted Bore1 reference moved");
    reads_lost_bore(&lost);
    let surviving = run(&mut s, "drawing.dimension.inspect", json!({"dimension": second["dimension"]}));
    assert_eq!(surviving["record"]["kind"], second["record"]["kind"], "Bore2 reference moved");
    reads(&surviving, 10.0, "Ø10.00");
}

/// Undo and redo of a bore resize leave the authored drawing and labels unchanged.
#[test]
fn undo_and_redo_a_diameter_change_keep_its_dimension() {
    let (mut s, drawing, first, second) = workshop_plate_dimensions();
    let authored = s.drawing("test").unwrap().to_json().unwrap();
    run(&mut s, "drawing.close", json!({}));
    run(&mut s, "parameters.change", json!({"name": "bore1_d", "expression": "12 mm"}));
    run(&mut s, "drawing.open", json!({"drawing": ids::encode(drawing)}));
    let changed = run(&mut s, "drawing.dimension.inspect", json!({"dimension": first["dimension"]}));
    assert_eq!(changed["record"]["kind"], first["record"]["kind"]);
    reads(&changed, 12.0, "Ø12.00");

    run(&mut s, "drawing.close", json!({}));
    run(&mut s, "edit.undo", json!({}));
    run(&mut s, "drawing.open", json!({"drawing": ids::encode(drawing)}));
    assert_eq!(s.drawing("test").unwrap().to_json().unwrap(), authored, "model undo changed the authored drawing");
    let undone = run(&mut s, "drawing.dimension.inspect", json!({"dimension": first["dimension"]}));
    assert_eq!(undone["record"]["kind"], first["record"]["kind"]);
    reads(&undone, 10.0, "Ø10.00");
    let other = run(&mut s, "drawing.dimension.inspect", json!({"dimension": second["dimension"]}));
    assert_eq!(other["record"]["kind"], second["record"]["kind"]);
    reads(&other, 10.0, "Ø10.00");

    run(&mut s, "drawing.close", json!({}));
    run(&mut s, "edit.redo", json!({}));
    run(&mut s, "drawing.open", json!({"drawing": ids::encode(drawing)}));
    assert_eq!(s.drawing("test").unwrap().to_json().unwrap(), authored, "model redo changed the authored drawing");
    let redone = run(&mut s, "drawing.dimension.inspect", json!({"dimension": first["dimension"]}));
    assert_eq!(redone["record"]["kind"], first["record"]["kind"]);
    reads(&redone, 12.0, "Ø12.00");
    let other = run(&mut s, "drawing.dimension.inspect", json!({"dimension": second["dimension"]}));
    assert_eq!(other["record"]["kind"], second["record"]["kind"]);
    reads(&other, 10.0, "Ø10.00");
}

/// Undo and redo of deleting Bore1 preserve both authored references and their positions.
#[test]
fn undo_and_redo_a_deleted_bore_restore_its_dimension() {
    let (mut s, drawing, first, second) = workshop_plate_dimensions();
    let authored = s.drawing("test").unwrap().to_json().unwrap();
    run(&mut s, "drawing.close", json!({}));
    run(&mut s, "timeline.delete", json!({"feature": "Bore1"}));
    run(&mut s, "drawing.open", json!({"drawing": ids::encode(drawing)}));
    let deleted = run(&mut s, "drawing.dimension.inspect", json!({"dimension": first["dimension"]}));
    assert_eq!(deleted["record"]["kind"], first["record"]["kind"]);
    reads_lost_bore(&deleted);
    let other = run(&mut s, "drawing.dimension.inspect", json!({"dimension": second["dimension"]}));
    assert_eq!(other["record"]["kind"], second["record"]["kind"]);
    reads(&other, 10.0, "Ø10.00");

    run(&mut s, "drawing.close", json!({}));
    run(&mut s, "edit.undo", json!({}));
    run(&mut s, "drawing.open", json!({"drawing": ids::encode(drawing)}));
    assert_eq!(s.drawing("test").unwrap().to_json().unwrap(), authored, "undo changed authored drawing");
    let recovered = run(&mut s, "drawing.dimension.inspect", json!({"dimension": first["dimension"]}));
    assert_eq!(recovered["record"]["kind"], first["record"]["kind"]);
    reads(&recovered, 10.0, "Ø10.00");

    run(&mut s, "drawing.close", json!({}));
    run(&mut s, "edit.redo", json!({}));
    run(&mut s, "drawing.open", json!({"drawing": ids::encode(drawing)}));
    assert_eq!(s.drawing("test").unwrap().to_json().unwrap(), authored, "redo changed authored drawing");
    let lost_again = run(&mut s, "drawing.dimension.inspect", json!({"dimension": first["dimension"]}));
    assert_eq!(lost_again["record"]["kind"], first["record"]["kind"]);
    reads_lost_bore(&lost_again);
    let other = run(&mut s, "drawing.dimension.inspect", json!({"dimension": second["dimension"]}));
    assert_eq!(other["record"]["kind"], second["record"]["kind"]);
    reads(&other, 10.0, "Ø10.00");
}

/// Save/reopen keeps the workshop plate's references, authored positions and lost value.
#[test]
fn reopening_a_plate_keeps_dimensions_and_a_lost_bores_last_value() {
    for state in ["unchanged", "resized", "deleted"] {
        let (mut s, drawing, first, second) = workshop_plate_dimensions();
        run(&mut s, "drawing.close", json!({}));
        match state {
            "resized" => {
                run(&mut s, "parameters.change", json!({"name": "bore1_d", "expression": "12 mm"}));
            }
            "deleted" => {
                run(&mut s, "timeline.delete", json!({"feature": "Bore1"}));
            }
            _ => {}
        }
        run(&mut s, "drawing.open", json!({"drawing": ids::encode(drawing)}));
        if state == "deleted" {
            let lost = run(&mut s, "drawing.dimension.inspect", json!({"dimension": first["dimension"]}));
            assert_eq!(lost["record"]["kind"], first["record"]["kind"]);
            reads_lost_bore(&lost);
        } else {
            let expected = if state == "resized" { 12.0 } else { 10.0 };
            let text = if state == "resized" { "Ø12.00" } else { "Ø10.00" };
            reads(&run(&mut s, "drawing.dimension.inspect", json!({"dimension": first["dimension"]})), expected, text);
        }
        let mut opened = reopened(&mut s, drawing, &format!("w23a-e2e04-{state}.solidcraft"));
        let bore1 = run(&mut opened, "drawing.dimension.inspect", json!({"dimension": first["dimension"]}));
        let bore2 = run(&mut opened, "drawing.dimension.inspect", json!({"dimension": second["dimension"]}));
        assert_eq!(bore1["record"]["kind"], first["record"]["kind"], "{state}: Bore1 reference changed");
        assert_eq!(bore2["record"]["kind"], second["record"]["kind"], "{state}: Bore2 reference changed");
        assert_eq!(bore1["record"]["text"], first["record"]["text"], "{state}: Bore1 label moved");
        assert_eq!(bore2["record"]["text"], second["record"]["text"], "{state}: Bore2 label moved");
        reads(&bore2, 10.0, "Ø10.00");
        match state {
            "unchanged" => reads(&bore1, 10.0, "Ø10.00"),
            "resized" => reads(&bore1, 12.0, "Ø12.00"),
            "deleted" => reads_lost_bore(&bore1),
            _ => unreachable!(),
        }
    }
}
