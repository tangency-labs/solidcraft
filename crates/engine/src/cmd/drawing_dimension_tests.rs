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
