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

/// The index of the box's edge whose every point is `radius` from `centre`, as `{body, index}`
/// names it: one of a circle's or an arc's.
fn round_index(s: &Session, centre: [f64; 3], radius: f64) -> usize {
    let centre = Vec3::from(centre);
    let state = s.model.state();
    let mesh = state.body("Box").unwrap().mesh();
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
    let hole = json!({"body": "Box", "index": round_index(&s, [40.0, 0.0, 15.0], 6.0)});
    let across = add(&mut s, view, json!({"radial": {"edge": hole}}));
    reads(&across, 12.0, "Ø12.00");
    assert_eq!(across["record"]["kind"]["radial"]["diameter"], json!(true), "{across}");
    let fillet = json!({"body": "Box", "index": round_index(&s, [75.0, 0.0, 25.0], 5.0)});
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
    // A session that never read it has none.
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
/// corners' edges, the edit's settings): never a panic, and the drawing keeps its rules.
#[test]
fn hostile_dimension_params_never_panic() {
    let (mut s, view) = front_view();
    let id = add(&mut s, view, json!({"length": {"edge": [40, 0, 30]}}))["dimension"].clone();
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
