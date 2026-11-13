use std::collections::BTreeSet;

use serde_json::json;
use solidcore_record::{Vec2, Vec3};
use solidcraft_doc::drawing::view::{View, ViewKind, ViewPlace};
use solidcraft_doc::drawing_view::{ViewDirection, ViewOverrides};
use solidcraft_geom::drawing_view::NamedView;

use super::*;
use crate::drawing_dimension_paper::tests::measured;
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
    d.add_views([Arc::new(View { id, sheet, kind, place: ViewPlace::At([200.0, 150.0]), settings, reference_shown_percent: 0.0 })]);
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
    // Below the box's bottom edge, away from the box.
    assert_eq!((&record["offset_mm"], &record["text"], &record["decimals"]), (&json!(-10.0), &json!("centred"), &json!(2)), "the defaults");
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

/// Placing reads the new dimension on the caller's drawing and takes it out again: `add` and
/// `from_model` both place on a drawing they keep using.
#[test]
fn placing_leaves_the_callers_dimensions_as_they_were() {
    let (mut s, view) = front_view();
    add(&mut s, view, json!({"length": {"edge": [40, 0, 30]}}));
    let mut record = Drawing::clone(s.drawing(ADD).unwrap());
    let ids = |d: &Drawing| d.dimensions.iter().map(|d| d.id).collect::<Vec<_>>();
    let before = ids(&record);
    let kind = record.dimensions[0].kind.clone();
    let new = NewDimension { view, kind, offset_mm: None, text: None, decimals: None, override_text: None, choose_diameter: false };
    let (dimension, read) = placed(&mut s, &mut record, ADD, new).unwrap().unwrap_or_else(|e| panic!("{}", e.error()));
    assert!(!before.contains(&dimension.id) && read.is_some_and(|r| r.status == Status::Ok));
    assert_eq!(ids(&record), before);
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

/// What a dry run of add with `p` answers, with its `dry_run: true` taken off.
fn previewed(s: &mut Session, p: &Value) -> Value {
    let mut dry = p.clone();
    dry["dry_run"] = json!(true);
    let mut reply = run(s, ADD, dry);
    assert_eq!(reply.as_object_mut().and_then(|o| o.remove("dry_run")), Some(json!(true)), "{reply}");
    reply
}

/// What add says refusing `p`, which a dry run of it says too.
fn refused_alike(s: &mut Session, p: &Value) -> String {
    let mut dry = p.clone();
    dry["dry_run"] = json!(true);
    let e = refused(s, dry);
    assert_eq!(e, refused(s, p.clone()), "{p}");
    e
}

/// A dry run of add answers what the add made next answers, where it would sit on its sheet included,
/// and changes nothing: not the design, the drawing, its undo steps, whether it needs saving or the
/// values read. An explicit `false` is an add.
#[test]
fn a_dry_run_answers_what_add_would_add_and_changes_nothing() {
    let (mut s, view) = front_view();
    let v = ids::encode(view);
    let kinds = [
        json!({"length": {"edge": [40, 0, 0]}}),
        json!({"linear": {"a": [0, 0, 0], "b": [80, 0, 0], "direction": "horizontal"}}),
        json!({"angle": {"a": [40, 0, 30], "b": [0, 0, 15]}}),
        json!({"chain": {"points": [[0, 0, 0], [80, 0, 0], [80, 0, 30]]}}),
    ];
    let mut answers = Vec::new();
    for kind in kinds {
        let p = json!({"view": v, "kind": kind});
        let (doc, steps, dirty, values) = (s.doc.clone(), s.undo.len(), s.is_dirty(), s.dimension_values.clone());
        let before = s.drawing("test").unwrap().to_json().unwrap();
        let dry = previewed(&mut s, &p);
        assert!(dry.get("layout").is_some(), "{kind}: a dry run says where its dimension would sit: {dry}");
        assert!(Arc::ptr_eq(&s.doc, &doc), "{kind}: the same design");
        assert_eq!((s.drawing("test").unwrap().to_json().unwrap(), s.undo.len(), s.is_dirty()), (before, steps, dirty), "{kind}");
        assert_eq!(s.dimension_values, values, "{kind}: no last good value kept");
        let added = run(&mut s, ADD, p);
        assert_eq!(s.undo.len(), steps + 1, "{kind}");
        assert!(added.get("dry_run").is_none(), "{added}");
        assert_eq!(dry, added);
        answers.push(dry);
    }
    // The bottom edge's length, below it; the bottom corners 80 apart; the top edge square to the
    // left one; and the chain from the bottom left corner to the top right one, 80 across and 30 up.
    reads(&answers[0], 80.0, "80.00");
    assert_eq!(answers[0]["record"]["offset_mm"], json!(-10.0), "below the bottom edge");
    reads(&answers[1], 80.0, "80.00");
    reads(&answers[2], 90.0, "90.00°");
    let chain = &answers[3];
    assert_eq!((chain["status"].as_str(), chain["members"].as_array().map(Vec::len)), (Some("ok"), Some(2)), "{chain}");
    assert!((chain["value"].as_f64().unwrap() - (80f64 * 80.0 + 30.0 * 30.0).sqrt()).abs() < 1e-9, "{chain}");
    let steps = s.undo.len();
    let added = run(&mut s, ADD, json!({"view": v, "kind": {"length": {"edge": [40, 0, 30]}}, "dry_run": false}));
    assert_eq!(s.undo.len(), steps + 1, "an explicit false is an add");
    assert!(added.get("dry_run").is_none(), "{added}");
    let id = added["dimension"].as_str().and_then(ids::decode).unwrap();
    assert!(s.drawing("test").unwrap().dimension(id).is_some());
}

/// A dry run takes no id: after three of them, the next add's id is the one a copy of the session
/// made before them gives, and each dry run answered with it.
#[test]
fn a_dry_run_takes_no_id() {
    let (mut s, view) = front_view();
    let mut twin = s.scratch();
    let p = json!({"view": ids::encode(view), "kind": {"length": {"edge": [40, 0, 30]}}});
    let previews: Vec<Value> = (0..3).map(|_| previewed(&mut s, &p)["dimension"].clone()).collect();
    let added = run(&mut s, ADD, p.clone())["dimension"].clone();
    assert_eq!(run(&mut twin, ADD, p)["dimension"], added);
    assert_eq!(previews, vec![added; 3]);
}

/// A dry run on a hole's rim reads its diameter, as the add saves it by default (read before the
/// default is set, the rim is a radius), and on a fillet's quarter arc its radius: each the add's
/// answer.
#[test]
fn a_dry_run_of_a_holes_rim_reads_its_diameter() {
    let (mut s, view) = front_view();
    let drawing = s.drawing("test").unwrap().id;
    run(&mut s, "drawing.close", json!({}));
    run(&mut s, "solid.cylinder", json!({"radius": 6, "height": 42, "base": [40, -1, 15], "axis": "Y", "operation": "cut", "targets": ["Box"]}));
    run(&mut s, "solid.fillet", json!({"edges": [[80, 20, 30]], "radius": 5, "body": "Box"}));
    run(&mut s, "drawing.open", json!({"drawing": ids::encode(drawing)}));
    let hole = json!({"body": "Box", "index": round_index(&s, [40.0, 0.0, 15.0], 6.0)});
    let fillet = json!({"body": "Box", "index": round_index(&s, [75.0, 0.0, 25.0], 5.0)});
    for (edge, value, text, diameter) in [(hole, 12.0, "Ø12.00", Some(json!(true))), (fillet, 5.0, "R5.00", None)] {
        let p = json!({"view": ids::encode(view), "kind": {"radial": {"edge": edge}}});
        let dry = previewed(&mut s, &p);
        reads(&dry, value, text);
        assert_eq!(dry["record"]["kind"]["radial"].get("diameter"), diameter.as_ref(), "{dry}");
        assert_eq!(dry, run(&mut s, ADD, p));
    }
}

/// A dry run is refused as the add is, in the same words, and changes nothing: no edge at the
/// point, a view the drawing hasn't got, a radial dimension on a straight edge, too many decimals,
/// a part the view's hide leaves out, and a `dry_run` that isn't true or false.
#[test]
fn a_dry_run_is_refused_as_add_is() {
    let state = |s: &Session| (s.drawing("test").unwrap().to_json().unwrap(), s.undo.len(), s.ids.seed());
    let (mut s, view) = front_view();
    let v = ids::encode(view);
    let before = state(&s);
    let cases = [
        (json!({"view": v, "kind": {"length": {"edge": [40, 20, 15]}}}), "no edge at [40, 20, 15]"),
        (json!({"view": ids::encode(99), "kind": {"length": {"edge": [40, 0, 30]}}}), "the drawing has no view"),
        (json!({"view": v, "kind": {"radial": {"edge": [40, 0, 30]}}}), "isn't a circle or an arc"),
        (json!({"view": v, "kind": {"length": {"edge": [40, 0, 30]}}, "decimals": 7}), "at most 6 decimals"),
    ];
    for (p, why) in cases {
        let e = refused_alike(&mut s, &p);
        assert!(e.contains(why), "{p}: {e}");
    }
    let e = refused(&mut s, json!({"view": v, "kind": {"length": {"edge": [40, 0, 30]}}, "dry_run": "yes"}));
    assert!(e.contains("dry_run"), "{e}");
    assert_eq!(state(&s), before);
    // A corner on a part the view's hide leaves out, as in
    // `a_dimension_is_refused_on_a_part_its_view_leaves_out`.
    let mut s = Session::default();
    let block = run(&mut s, "component.create", json!({"name": "Block"}))["occurrence"].clone();
    run(&mut s, "solid.box", json!({"length": 20, "width": 30, "height": 10, "body_name": "Block", "operation": "new"}));
    run(&mut s, "component.activate", json!({"component": 0}));
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [100, 0, 0], "operation": "new", "body_name": "Base"}));
    let (mut s, view) = drawn_from_the_front(s);
    let v = ids::encode(view);
    run(&mut s, "drawing.view.edit", json!({"view": v, "set": {"hide": [[block]]}}));
    let before = state(&s);
    let linear = json!({"view": v, "kind": {"linear": {"a": [100, 0, 0], "b": {"point": [20, 0, 0], "path": [block]}}}});
    let e = refused_alike(&mut s, &linear);
    assert!(e.contains(&format!("view {v} doesn't draw that part: its hide leaves out occurrence path [{block}]")), "{e}");
    assert_eq!(state(&s), before);
}

/// The offset `add` saved for a dimension of `kind` on `view`, with `more` params.
fn offset_of(s: &mut Session, view: u64, kind: Value, more: Value) -> f64 {
    let mut p = json!({"view": ids::encode(view), "kind": kind});
    p.as_object_mut().unwrap().extend(more.as_object().unwrap().clone());
    run(s, ADD, p)["record"]["offset_mm"].as_f64().unwrap()
}

/// A new linear, length or chain dimension sits on the side of its line away from the box: below
/// the bottom edge, above the top one, left of the left one and right of the right one. Radial and
/// angle dimensions keep +10.
#[test]
fn a_new_dimension_sits_away_from_what_it_measures() {
    let (mut s, view) = front_view();
    let mut offset = |kind: Value| offset_of(&mut s, view, kind, json!({}));
    assert_eq!(offset(json!({"length": {"edge": [40, 0, 0]}})), -10.0, "below the bottom edge");
    assert_eq!(offset(json!({"length": {"edge": [40, 0, 30]}})), 10.0, "above the top edge");
    assert_eq!(offset(json!({"length": {"edge": [0, 0, 15]}})), 10.0, "left of the left edge");
    assert_eq!(offset(json!({"length": {"edge": [80, 0, 15]}})), -10.0, "right of the right edge");
    assert_eq!(offset(json!({"linear": {"a": [80, 0, 0], "b": [80, 0, 30], "direction": "vertical"}})), -10.0, "the right corners");
    assert_eq!(offset(json!({"angle": {"a": [40, 0, 0], "b": [0, 0, 15]}})), 10.0, "an angle keeps +10");
}

/// An L-shape's view isn't centred on its outline's middle, so the material decides: right of the
/// step's upright edge (on the middle's own upright line) and above its flat one. A line with
/// material on both sides goes away from the middle of the view's box.
#[test]
fn an_l_shapes_inner_edges_take_the_open_side() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 80, "width": 40, "height": 30, "body_name": "Box"}));
    run(&mut s, "solid.box", json!({"length": 40, "width": 40, "height": 30, "corner": [0, 0, 30], "operation": "join", "targets": ["Box"]}));
    let (mut s, view) = drawn_from_the_front(s);
    let mut offset = |kind: Value| offset_of(&mut s, view, kind, json!({}));
    assert_eq!(offset(json!({"length": {"edge": [40, 0, 45]}})), -10.0, "right of the upright inner edge");
    assert_eq!(offset(json!({"length": {"edge": [60, 0, 30]}})), 10.0, "above the flat inner edge");
    let across = json!({"linear": {"a": [0, 0, 0], "b": [80, 0, 30], "direction": "horizontal"}});
    assert_eq!(offset(across), -10.0, "inside the part, away from the box's middle (40, 30)");
    let bottom = json!({"chain": {"points": [[0, 0, 0], [40, 0, 30], [80, 0, 0]], "direction": "horizontal"}});
    assert_eq!(offset(bottom), -10.0, "a chain by its first and last points: below the bottom");
}

/// A slot's floor and walls have their material on the side the middle of the view's box would
/// send them, so only the material puts them in the open: above the floor, into the slot from
/// each wall.
#[test]
fn a_slots_floor_and_walls_take_the_open_side() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 60, "width": 40, "height": 40, "body_name": "Block"}));
    run(&mut s, "solid.box", json!({"length": 20, "width": 50, "height": 35, "corner": [20, -5, 10], "operation": "cut", "targets": ["Block"]}));
    let (mut s, view) = drawn_from_the_front(s);
    let mut offset = |kind: Value| offset_of(&mut s, view, kind, json!({}));
    assert_eq!(offset(json!({"length": {"edge": [30, 0, 10]}})), 10.0, "above the floor, below the box's middle (30, 20)");
    assert_eq!(offset(json!({"length": {"edge": [20, 0, 25]}})), -10.0, "right of the left wall");
    assert_eq!(offset(json!({"length": {"edge": [40, 0, 25]}})), 10.0, "left of the right wall");
}

/// A line through the middle of the view's box with material on both sides (a symmetric pair of
/// corners) takes the fixed rule: above.
#[test]
fn a_line_through_the_views_middle_goes_above() {
    let (mut s, view) = front_view();
    let corners = json!({"linear": {"a": [0, 0, 0], "b": [80, 0, 30], "direction": "horizontal"}});
    assert_eq!(offset_of(&mut s, view, corners, json!({})), 10.0);
}

/// An offset given keeps its sign, wherever the material is.
#[test]
fn an_offset_given_keeps_its_sign() {
    let (mut s, view) = front_view();
    assert_eq!(offset_of(&mut s, view, json!({"length": {"edge": [40, 0, 0]}}), json!({"offset_mm": 10})), 10.0, "into the box");
    assert_eq!(offset_of(&mut s, view, json!({"length": {"edge": [40, 0, 30]}}), json!({"offset_mm": -5})), -5.0, "into the box");
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
    // The hole's rim is one closed edge, and every point on it names that edge.
    let mut named = BTreeSet::new();
    for step in 0..55 {
        let at = add(&mut s, view, json!({"radial": {"edge": rim(6.0, 3.3 + 6.5 * f64::from(step))}}));
        reads(&at, 12.0, "Ø12.00");
        named.insert(at["record"]["kind"]["radial"]["edge"]["name"].as_str().unwrap().to_string());
    }
    assert_eq!(named.len(), 1, "{named:?}");
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
    let centre = |at: [f64; 3], radius: f64| json!({"centre": {"body": "Box", "index": round_index(&s, at, radius)}});
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
    let hole = |x: f64| json!({"centre": {"body": "Box", "index": round_index(&s, [x, 0.0, 15.0], 3.0)}});
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

/// A sphere's size, as ISO 129-1 writes it: a ball of radius 10 is a whole sphere, so it reads
/// SØ20.00, picked by a point on it or by its body and index, and saved by its face's name. A ball
/// of radius 10 on the end of a Ø16 rod is part of a sphere, so it reads SR10.00. It takes no settings. A flat face is
/// refused, and a sphere named as a cylinder keeps the cylinder's own words, with the drawing
/// unchanged.
#[test]
fn a_spheres_size_is_its_diameter_when_whole_and_its_radius_when_part() {
    let (mut s, view) = front_view();
    let drawing = s.drawing("test").unwrap().id;
    run(&mut s, "drawing.close", json!({}));
    run(&mut s, "solid.sphere", json!({"radius": 10, "center": [140, 20, 15], "operation": "new", "body_name": "Ball"}));
    // Not the rod's own radius: a ball as wide as its rod meets it along a tangent.
    run(&mut s, "solid.cylinder", json!({"radius": 8, "height": 30, "base": [-60, 20, 15], "axis": "X", "operation": "new", "body_name": "Rod"}));
    run(&mut s, "solid.sphere", json!({"radius": 10, "center": [-60, 20, 15], "operation": "join", "targets": ["Rod"]}));
    run(&mut s, "drawing.open", json!({"drawing": ids::encode(drawing)}));
    // Off every pole and seam: 10 from each centre along (±1, -1, 1) / √3.
    let along = 10.0 / 3f64.sqrt();
    let (on_ball, on_dome) = ([140.0 + along, 20.0 - along, 15.0 + along], [-60.0 - along, 20.0 - along, 15.0 + along]);
    let ball = add(&mut s, view, json!({"sphere": {"face": on_ball}}));
    reads(&ball, 20.0, "SØ20.00");
    let saved = ball["record"]["kind"]["sphere"]["face"].clone();
    assert!(saved["body"] == "Ball" && saved["name"].as_str().is_some_and(|n| !n.is_empty()), "{saved}");
    let index = {
        let state = s.model.state();
        solidcraft_doc::appearance::face_index_at(state.body("Ball").unwrap(), Vec3::from(on_ball)).unwrap()
    };
    let by_index = add(&mut s, view, json!({"sphere": {"face": {"body": "Ball", "index": index}}}));
    assert_eq!(by_index["record"]["kind"], ball["record"]["kind"]);
    reads(&add(&mut s, view, json!({"sphere": {"face": on_dome}})), 10.0, "SR10.00");
    let id = ball["dimension"].clone();
    let before = s.drawing("test").unwrap().to_json().unwrap();
    for (set, why) in [
        (json!({"direction": "horizontal"}), "a sphere's size has no direction, reading or side"),
        (json!({"diameter": true}), "`diameter` is a radial dimension's"),
    ] {
        let p = json!({"dimension": id, "set": set});
        let e = s.execute(EDIT, &p).err().map(|e| e.to_string()).unwrap_or_else(|| panic!("accepted: {p}"));
        assert!(e.contains(why), "{p}: {e}");
    }
    let v = ids::encode(view);
    let cases = [
        (json!({"view": v, "kind": {"sphere": {"face": [40, 0, 10]}}}), "isn't a sphere"),
        (json!({"view": v, "kind": {"cylinder": {"face": on_ball}}}), "isn't a cylinder"),
    ];
    for (p, why) in cases {
        let e = refused(&mut s, p.clone());
        assert!(e.contains(why), "{p}: {e}");
    }
    assert_eq!(s.drawing("test").unwrap().to_json().unwrap(), before);
}

/// A cone's included angle, between its two silhouettes: a Ø20 pin's top rim chamfered 2 across
/// and 2 down is a cone of half-angle 45°, so the front view, whose plane holds its axis, reads
/// 90.00°, with the 2 decimals every dimension starts with, saved by its face's name. Chamfered 1 across and 3 down it reads 2·atan(1/3), 36.87°;
/// 3 across and 1 down, 2·atan(3), 143.13°. It takes no settings. A flat face is refused, and a
/// cone named as a cylinder keeps the cylinder's own words, with the drawing unchanged.
#[test]
fn a_cones_angle_is_read_between_its_silhouettes() {
    let (mut s, view) = front_view();
    let drawing = s.drawing("test").unwrap().id;
    run(&mut s, "drawing.close", json!({}));
    // A point halfway down each chamfer, an eighth of a turn round from the front.
    let pin = |s: &mut Session, x: f64, name: &str, across: f64, down: f64| {
        let base = json!({"radius": 10, "height": 20, "base": [x, 20, 0], "axis": "Z", "operation": "new", "body_name": name});
        run(s, "solid.cylinder", base);
        run(s, "solid.chamfer", json!({"edges": [[x + 10.0, 20, 20]], "distance": across, "distance2": down, "body": name}));
        let (r, turn) = (10.0 - across / 2.0, -std::f64::consts::FRAC_PI_4);
        [x + r * turn.cos(), 20.0 + r * turn.sin(), 20.0 - down / 2.0]
    };
    let (even, steep, flat) = (pin(&mut s, 140.0, "Even", 2.0, 2.0), pin(&mut s, 180.0, "Steep", 1.0, 3.0), pin(&mut s, 220.0, "Flat", 3.0, 1.0));
    run(&mut s, "drawing.open", json!({"drawing": ids::encode(drawing)}));
    let reads_near = |reply: &Value, degrees: f64, text: &str| {
        assert_eq!((reply["status"].as_str(), reply["text"].as_str()), (Some("ok"), Some(text)), "{reply}");
        assert!((reply["value"].as_f64().unwrap() - degrees).abs() < 1e-6 && reply["foreshortened"] != json!(true), "{reply}");
    };
    let square = add(&mut s, view, json!({"cone": {"face": even}}));
    reads_near(&square, 90.0, "90.00°");
    let saved = square["record"]["kind"]["cone"]["face"].clone();
    assert!(saved["body"] == "Even" && saved["name"].as_str().is_some_and(|n| !n.is_empty()), "{saved}");
    reads_near(&add(&mut s, view, json!({"cone": {"face": steep}})), 2.0 * (1f64 / 3.0).atan().to_degrees(), "36.87°");
    reads_near(&add(&mut s, view, json!({"cone": {"face": flat}})), 2.0 * 3f64.atan().to_degrees(), "143.13°");
    let id = square["dimension"].clone();
    let before = s.drawing("test").unwrap().to_json().unwrap();
    let p = json!({"dimension": id, "set": {"direction": "horizontal"}});
    let e = s.execute(EDIT, &p).err().map(|e| e.to_string()).unwrap_or_else(|| panic!("accepted: {p}"));
    assert!(e.contains("a cone's angle has no direction, reading or side"), "{e}");
    let v = ids::encode(view);
    let cases = [
        (json!({"view": v, "kind": {"cone": {"face": [40, 0, 10]}}}), "isn't a cone"),
        (json!({"view": v, "kind": {"cylinder": {"face": even}}}), "isn't a cylinder"),
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

/// Two flat faces the view sees edge-on: the box's ends read 80 across it and 80 along the view, each
/// saved by its name; an edit takes a direction only. Refused, with the drawing unchanged: a face the
/// view sees face on, one plane, a round face, and a direction along the faces.
#[test]
fn two_flat_faces_read_the_distance_between_them() {
    let (mut s, view) = front_view();
    let drawing = s.drawing("test").unwrap().id;
    run(&mut s, "drawing.close", json!({}));
    run(&mut s, "solid.cylinder", json!({"radius": 3, "height": 42, "base": [20, -1, 15], "axis": "Y", "operation": "cut", "targets": ["Box"]}));
    run(&mut s, "drawing.open", json!({"drawing": ids::encode(drawing)}));
    let added = add(&mut s, view, json!({"faces": {"a": [0, 20, 15], "b": [80, 20, 15]}}));
    reads(&added, 80.0, "80.00");
    assert_eq!((added["exact"].clone(), added["foreshortened"].clone()), (json!(true), json!(false)), "{added}");
    let saved = &added["record"]["kind"]["faces"];
    assert!(saved["a"]["body"] == "Box" && saved["b"]["name"].is_string() && saved["direction"] == "aligned", "{saved}");
    let by_index = {
        let state = s.model.state();
        solidcraft_doc::appearance::face_index_at(state.body("Box").unwrap(), Vec3::new(0.0, 20.0, 15.0)).unwrap()
    };
    let same = add(&mut s, view, json!({"faces": {"a": {"body": "Box", "index": by_index}, "b": [80, 20, 15]}}));
    assert_eq!(same["record"]["kind"], added["record"]["kind"]);
    let id = added["dimension"].clone();
    assert_eq!(edit(&mut s, &id, json!({"direction": "horizontal"}))["changed"], json!(["direction"]));
    reads(&run(&mut s, INSPECT, json!({"dimension": id})), 80.0, "80.00");
    let e = s.execute(EDIT, &json!({"dimension": id, "set": {"reading": "true"}})).err().map(|e| e.to_string()).unwrap_or_default();
    assert!(e.contains("only `direction`"), "{e}");
    let before = s.drawing("test").unwrap().to_json().unwrap();
    let v = ids::encode(view);
    let faces = |a: Value, b: Value, direction: Value| json!({"view": v, "kind": {"faces": {"a": a, "b": b, "direction": direction}}});
    let cases = [
        (faces(json!([40, 0, 10]), json!([80, 20, 15]), json!("aligned")), "isn't edge-on"),
        (faces(json!([0, 10, 10]), json!([0, 30, 10]), json!("aligned")), "one plane"),
        (faces(json!([23, 20, 15]), json!([80, 20, 15]), json!("aligned")), "isn't flat"),
        (faces(json!([0, 20, 15]), json!([80, 20, 15]), json!("vertical")), "run along the direction"),
        (faces(json!([0, 20, 15]), json!([200, 200, 200]), json!("aligned")), "no face at [200, 200, 200]"),
    ];
    for (p, why) in cases {
        let e = refused(&mut s, p.clone());
        assert!(e.contains(why), "{p}: {e}");
    }
    assert_eq!(s.drawing("test").unwrap().to_json().unwrap(), before);
}

/// A dimension says its feature failed, with why, and that it is suppressed: the words a script reads.
#[test]
fn a_dimension_says_its_feature_failed_or_is_suppressed() {
    let (mut s, view) = front_view();
    let drawing = json!({"drawing": ids::encode(s.drawing("test").unwrap().id)});
    run(&mut s, "drawing.close", json!({}));
    run(&mut s, "solid.fillet", json!({"edges": [[80, 20, 30]], "radius": 5, "body": "Box", "name": "Round"}));
    run(&mut s, "drawing.open", drawing.clone());
    let fillet = json!({"body": "Box", "index": round_index(&s, [75.0, 0.0, 25.0], 5.0)});
    let id = add(&mut s, view, json!({"radial": {"edge": fillet}}))["dimension"].clone();
    let status = |s: &mut Session, change: &str, p: Value| {
        run(s, "drawing.close", json!({}));
        run(s, change, p);
        run(s, "drawing.open", drawing.clone());
        run(s, INSPECT, json!({"dimension": id}))
    };
    let read = status(&mut s, "timeline.suppress", json!({"feature": "Round", "suppressed": true}));
    assert_eq!(read["status"], "suppressed", "{read}");
    let read = status(&mut s, "timeline.suppress", json!({"feature": "Round", "suppressed": false}));
    assert_eq!(read["status"], "ok", "{read}");
    let read = status(&mut s, "timeline.reorder", json!({"feature": "Round", "position": 0, "allow_errors": true}));
    assert_eq!(read["status"], "failed", "{read}");
    assert!(read["why"].as_str().is_some_and(|why| !why.is_empty()), "{read}");
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
/// corners' edges, a chain's points, the edit's settings, the first add's `dry_run`): never a
/// panic, and the drawing keeps its rules.
#[test]
fn hostile_dimension_params_never_panic() {
    let (mut s, view) = front_view();
    let id = add(&mut s, view, json!({"length": {"edge": [40, 0, 30]}}))["dimension"].clone();
    // Aligned from the first corner to the last, each corner past the one before.
    let chain = add(&mut s, view, json!({"chain": {"points": [[0, 0, 0], [80, 0, 0], [80, 0, 30]]}}))["dimension"].clone();
    // A radial dimension and a chain on a hole that is then deleted: lost, so a repair goes as far as
    // its slot and its pick.
    let drawing = json!({"drawing": ids::encode(s.drawing("test").unwrap().id)});
    run(&mut s, "drawing.close", json!({}));
    let cut = json!({"radius": 6, "height": 42, "base": [40, -1, 15], "axis": "Y", "operation": "cut", "targets": ["Box"], "name": "Hole"});
    run(&mut s, "solid.cylinder", cut);
    run(&mut s, "drawing.open", drawing.clone());
    let hole = json!({"body": "Box", "index": round_index(&s, [40.0, 0.0, 15.0], 6.0)});
    let lost = add(&mut s, view, json!({"radial": {"edge": hole}}))["dimension"].clone();
    let through = json!({"chain": {"points": [[0, 0, 0], {"centre": hole}, [80, 0, 0]], "direction": "horizontal"}});
    let lost_chain = add(&mut s, view, through)["dimension"].clone();
    run(&mut s, "drawing.close", json!({}));
    run(&mut s, "timeline.delete", json!({"feature": "Hole"}));
    run(&mut s, "drawing.open", drawing);
    let v = json!(ids::encode(view));
    let corner = json!({"edges": [{"body": "Box", "index": 0}, {"point": [0, 0, 15]}], "path": [1]});
    let calls = [
        (
            ADD,
            json!({"view": v, "kind": {"linear": {"a": [0, 0, 0], "b": corner, "direction": {"along": [1, 0]}, "reading": "projected"}},
            "offset_mm": 5, "text": {"at": [1, 2]}, "decimals": 1, "override_text": "x", "dry_run": true}),
        ),
        (ADD, json!({"view": v, "kind": {"angle": {"a": {"point": [40, 0, 30], "path": [1]}, "b": [0, 0, 15], "other_side": true}}})),
        (ADD, json!({"view": v, "kind": {"radial": {"edge": {"body": "Box", "index": 0, "path": [1]}, "diameter": true}}})),
        (
            ADD,
            json!({"view": v, "kind": {"faces": {"a": {"body": "Box", "index": 0, "path": [1]}, "b": [80, 20, 15], "direction": {"along": [1, 0]}}}}),
        ),
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
        (REPAIR, json!({"dimension": lost, "slot": "edge", "pick": {"edge": {"body": "Box", "index": 0}}})),
        (REPAIR, json!({"dimension": lost_chain, "slot": 1, "pick": {"corner": {"edges": [{"body": "Box", "index": 0}, {"point": [0, 0, 15]}]}}})),
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
        // The last point a chain may have: past the lost chain's three.
        json!(63),
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

/// A dimension attaches only to parts its view draws. One with a corner on a part the view's hide
/// leaves out is refused, and so is a chain whose last point is there, with the drawing left as it
/// was. With the part drawn again it is added; a later hide keeps it, and inspect warns until the
/// edit is undone.
#[test]
fn a_dimension_is_refused_on_a_part_its_view_leaves_out() {
    let mut s = Session::default();
    let block = run(&mut s, "component.create", json!({"name": "Block"}))["occurrence"].clone();
    run(&mut s, "solid.box", json!({"length": 20, "width": 30, "height": 10, "body_name": "Block", "operation": "new"}));
    run(&mut s, "component.activate", json!({"component": 0}));
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [100, 0, 0], "operation": "new", "body_name": "Base"}));
    let (mut s, view) = drawn_from_the_front(s);
    let v = ids::encode(view);
    let on_block = |p: [f64; 3]| json!({"point": p, "path": [block]});
    let linear = json!({"view": v, "kind": {"linear": {"a": [100, 0, 0], "b": on_block([20.0, 0.0, 0.0])}}});
    let chain = json!({"view": v, "kind": {"chain": {"points": [[100, 0, 0], [110, 0, 0], on_block([20.0, 0.0, 10.0])]}}});
    run(&mut s, "drawing.view.edit", json!({"view": v, "set": {"hide": [[block]]}}));
    let words = format!("its hide leaves out occurrence path [{block}]");
    let before = s.drawing("test").unwrap().to_json().unwrap();
    for p in [linear.clone(), chain] {
        let e = refused(&mut s, p.clone());
        assert!(e.contains(&format!("view {v} doesn't draw that part: {words}")), "{p}: {e}");
    }
    assert_eq!(s.drawing("test").unwrap().to_json().unwrap(), before);
    run(&mut s, "drawing.view.edit", json!({"view": v, "set": {"hide": null}}));
    let added = run(&mut s, ADD, linear);
    assert!(added["warnings"].is_null(), "{added}");
    run(&mut s, "drawing.view.edit", json!({"view": v, "set": {"hide": [[block]]}}));
    let warned = run(&mut s, INSPECT, json!({"dimension": added["dimension"]}));
    assert_eq!(warned["warnings"], json!([format!("view {v} doesn't draw this part now: {words}")]), "{warned}");
    run(&mut s, "edit.undo", json!({}));
    assert!(run(&mut s, INSPECT, json!({"dimension": added["dimension"]}))["warnings"].is_null());
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

/// After a fillet takes the edge a length measures, inspect says it is lost and lists leads. The
/// one that is the fillet's 80 long edge on the front, passed back as it stands, repairs it: it
/// reads 80 again, with its id, view, offset, text place and decimals, and only its edge's name
/// changed, as one undo step. A linear dimension from the corner the fillet took is repaired by a
/// corner the fillet made.
#[test]
fn fillet_then_repair_reads_again_with_the_same_id_and_place() {
    let (mut s, view) = front_view();
    let drawing = json!({"drawing": ids::encode(s.drawing("test").unwrap().id)});
    let p = json!({"view": ids::encode(view), "kind": {"length": {"edge": [40, 0, 30]}}, "offset_mm": -12.5, "text": {"at": [0, 40]}});
    let id = run(&mut s, ADD, p)["dimension"].clone();
    let corner = add(&mut s, view, json!({"linear": {"a": [0, 0, 30], "b": [80, 0, 0], "direction": "horizontal"}}))["dimension"].clone();
    run(&mut s, "drawing.close", json!({}));
    lose_the_edge(&mut s);
    run(&mut s, "drawing.open", drawing);
    let lost = run(&mut s, INSPECT, json!({"dimension": id}));
    assert_eq!(lost["status"], "lost", "{lost}");
    let front = json!({"edge": {"body": "Box", "index": edge_index(&s, [0.0, 0.0, 25.0], [80.0, 0.0, 25.0])}});
    let pick = lost["candidates"].as_array().and_then(|c| c.iter().find(|c| **c == front)).cloned();
    let pick = pick.unwrap_or_else(|| panic!("the fillet's front edge is a lead: {lost}"));
    let steps = s.undo.len();
    let repaired = run(&mut s, REPAIR, json!({"dimension": id, "slot": "edge", "pick": pick}));
    reads(&repaired, 80.0, "80.00");
    assert_eq!(s.undo.len(), steps + 1, "one undo step");
    assert_eq!((&repaired["dimension"], &repaired["view"]), (&id, &lost["view"]));
    let (before, after) = (&lost["record"], &repaired["record"]);
    assert_ne!(after["kind"]["length"]["edge"]["name"], before["kind"]["length"]["edge"]["name"], "{repaired}");
    let mut renamed = before.clone();
    renamed["kind"]["length"]["edge"]["name"] = after["kind"]["length"]["edge"]["name"].clone();
    assert_eq!(&renamed, after, "only the edge's name changed");
    assert_eq!((&after["offset_mm"], &after["text"], &after["decimals"]), (&json!(-12.5), &json!({"at": [0.0, 40.0]}), &json!(2)));
    assert_eq!(run(&mut s, INSPECT, json!({"dimension": corner}))["status"], "lost");
    let moved = run(&mut s, REPAIR, json!({"dimension": corner, "slot": "a", "pick": {"corner": [0, 0, 25]}}));
    reads(&moved, 80.0, "80.00");
}

/// A repair is one undo step: undone, the record is as it was and reads lost with the same leads,
/// because reading never rewrote it; redone, it reads 80. Repaired, it reads ok, so the same pick
/// again is refused and adds no step.
#[test]
fn a_repair_is_one_undo_step_and_undone_is_lost_again() {
    let (mut s, drawing, dimension) = read_once();
    lose_the_edge(&mut s);
    run(&mut s, "drawing.open", json!({"drawing": ids::encode(drawing)}));
    let lost = inspect_of(&mut s, dimension);
    assert!(lost["candidates"].as_array().is_some_and(|c| !c.is_empty()), "{lost}");
    let before = s.drawing("test").unwrap().to_json().unwrap();
    let front = edge_index(&s, [0.0, 0.0, 25.0], [80.0, 0.0, 25.0]);
    let p = json!({"dimension": ids::encode(dimension), "slot": "edge", "pick": {"edge": {"body": "Box", "index": front}}});
    let steps = s.undo.len();
    run(&mut s, REPAIR, p.clone());
    let after = s.drawing("test").unwrap().to_json().unwrap();
    assert_eq!(s.undo.len(), steps + 1);
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(s.drawing("test").unwrap().to_json().unwrap(), before);
    let again = inspect_of(&mut s, dimension);
    assert_eq!((&again["status"], &again["candidates"]), (&json!("lost"), &lost["candidates"]), "{again}");
    run(&mut s, "edit.redo", json!({}));
    assert_eq!(s.drawing("test").unwrap().to_json().unwrap(), after);
    reads(&inspect_of(&mut s, dimension), 80.0, "80.00");
    let steps = s.undo.len();
    let e = s.execute(REPAIR, &p).err().map(|e| e.to_string()).unwrap_or_else(|| panic!("accepted: {p}"));
    assert!(e.contains("it reads ok: only a lost or ambiguous reference is repaired"), "{e}");
    assert_eq!((s.undo.len(), s.drawing("test").unwrap().to_json().unwrap()), (steps, after));
}

/// A repair that doesn't fit is refused with its reason, and the drawing and its undo steps stay as
/// they were: a slot the dimension hasn't got, a pick of another kind, a pick that names nothing or
/// several edges or an edge the body hasn't got, a pick with a path of its own, a radial dimension
/// repaired to a straight edge, a corner still found of a dimension whose other corner is lost,
/// silhouettes repaired onto the other's cylinder, a dimension that reads, one the drawing hasn't
/// got, and unknown keys.
#[test]
fn a_bad_repair_pick_changes_nothing() {
    let (mut s, view) = front_view();
    let drawing = json!({"drawing": ids::encode(s.drawing("test").unwrap().id)});
    run(&mut s, "drawing.close", json!({}));
    let cut = json!({"radius": 6, "height": 42, "base": [40, -1, 15], "axis": "Y", "operation": "cut", "targets": ["Box"], "name": "Hole"});
    run(&mut s, "solid.cylinder", cut);
    // Two bosses along X on the right face, as two_cylinders_read_between_the_silhouettes_given has them.
    for (radius, z, name) in [(5, 15, "Big"), (3, 25, "Small")] {
        let boss = json!({"radius": radius, "height": 20, "base": [80, 20, z], "axis": "X", "operation": "join", "targets": ["Box"], "name": name});
        run(&mut s, "solid.cylinder", boss);
    }
    run(&mut s, "drawing.open", drawing.clone());
    let on = |r: f64, z: f64| json!([90.0, 20.0 + r * 30f64.to_radians().sin(), z + r * 30f64.to_radians().cos()]);
    let between = json!({"silhouettes": {"a": {"face": on(5.0, 15.0), "side": "near"}, "b": {"face": on(3.0, 25.0), "side": "near"}}});
    let silhouettes = add(&mut s, view, between)["dimension"].clone();
    let hole = json!({"body": "Box", "index": round_index(&s, [40.0, 0.0, 15.0], 6.0)});
    let radial = add(&mut s, view, json!({"radial": {"edge": hole}}))["dimension"].clone();
    let length = add(&mut s, view, json!({"length": {"edge": [40, 0, 30]}}))["dimension"].clone();
    let linear = add(&mut s, view, json!({"linear": {"a": [0, 0, 30], "b": [80, 0, 0]}}))["dimension"].clone();
    let ok = add(&mut s, view, json!({"length": {"edge": [40, 0, 0]}}))["dimension"].clone();
    run(&mut s, "drawing.close", json!({}));
    run(&mut s, "timeline.delete", json!({"feature": "Hole"}));
    run(&mut s, "timeline.delete", json!({"feature": "Small"}));
    lose_the_edge(&mut s);
    run(&mut s, "drawing.open", drawing);
    assert_eq!(run(&mut s, INSPECT, json!({"dimension": silhouettes}))["status"], "lost");
    let (before, steps) = (s.drawing("test").unwrap().to_json().unwrap(), s.undo.len());
    let call = |dimension: &Value, slot: Value, pick: Value| json!({"dimension": dimension, "slot": slot, "pick": pick});
    let front = json!({"edge": [40, 0, 25]});
    let cases = [
        (call(&length, json!("b"), front.clone()), "slots are `edge`"),
        (call(&length, json!(5), front.clone()), "slots are `edge`"),
        (call(&linear, json!("edge"), front.clone()), "slots are `a` and `b`, its corners"),
        (call(&length, json!("edge"), json!({"face": [40, 20, 0]})), "this reference is an edge: give {edge: …}"),
        (call(&linear, json!("a"), front.clone()), "this reference is a corner: give {corner: …}"),
        (call(&length, json!("edge"), json!({"edge": [40, 20, 15]})), "no edge at [40, 20, 15]"),
        (call(&length, json!("edge"), json!({"edge": [0, 0, 0]})), "[0, 0, 0] is on 3 edges"),
        (call(&length, json!("edge"), json!({"edge": {"body": "Box", "index": 999}})), "body `Box` has no edge 999"),
        (call(&length, json!("edge"), json!({"edge": {"point": [40, 0, 25], "path": [1]}})), "a repair's pick has no `path`"),
        (call(&length, json!("edge"), json!({"edge": [40, 0, 25], "face": [40, 20, 0]})), "a pick is {edge: an edge}"),
        (call(&radial, json!("edge"), json!({"edge": [40, 0, 0]})), "isn't a circle or an arc"),
        (call(&linear, json!("b"), json!({"corner": [80, 0, 0]})), "this reference still finds what it names"),
        (call(&silhouettes, json!("a"), json!({"face": on(5.0, 15.0)})), "this reference still finds what it names"),
        (call(&silhouettes, json!("b"), json!({"face": on(-5.0, 15.0)})), "both silhouettes would be on one cylinder"),
        (call(&ok, json!("edge"), front.clone()), "it reads ok: only a lost or ambiguous reference is repaired"),
        (call(&json!(ids::encode(99)), json!("edge"), front.clone()), "the drawing has no dimension"),
        (json!({"dimension": length, "slot": "edge", "pick": front, "set": {}}), "unknown field `set`"),
    ];
    for (p, why) in cases {
        let e = s.execute(REPAIR, &p).err().map(|e| e.to_string()).unwrap_or_else(|| panic!("accepted: {p}"));
        assert!(e.contains(why), "{p}: {e}");
    }
    assert_eq!((s.drawing("test").unwrap().to_json().unwrap(), s.undo.len()), (before, steps));
}

/// A notch splits the top front edge, so its length reads ambiguous and lists both pieces. The left
/// piece, passed back as it stands, repairs it to read that piece's 35, as one undo step.
#[test]
fn an_ambiguous_length_is_repaired_to_one_piece() {
    let (mut s, drawing, dimension) = read_once();
    run(&mut s, "solid.box", json!({"length": 10, "width": 5, "height": 5, "corner": [35, -1, 26], "operation": "cut", "targets": ["Box"]}));
    run(&mut s, "drawing.open", json!({"drawing": ids::encode(drawing)}));
    let split = inspect_of(&mut s, dimension);
    assert_eq!(split["status"], "ambiguous", "{split}");
    let left = json!({"edge": {"body": "Box", "index": edge_index(&s, [0.0, 0.0, 30.0], [35.0, 0.0, 30.0])}});
    assert!(split["candidates"].as_array().is_some_and(|c| c.contains(&left)), "{split}");
    let steps = s.undo.len();
    let repaired = run(&mut s, REPAIR, json!({"dimension": ids::encode(dimension), "slot": "edge", "pick": left}));
    reads(&repaired, 35.0, "35.00");
    assert_eq!(s.undo.len(), steps + 1, "one undo step");
}

/// Each slot reaches its own reference of each kind of dimension, and every other slot is refused:
/// `slot` on records made up for it, with no model.
#[test]
fn each_slot_reaches_its_own_reference() {
    let edge = |name: &str| ModelEdge { body: "Box".into(), origin_feature: 1, name: format!("edge {name}"), path: Vec::new() };
    let point =
        |name: &str| ModelPoint { body: "Box".into(), origin_feature: 1, edges: vec![format!("corner {name}")], centre: false, path: Vec::new() };
    let face = |name: &str| ModelFace { body: "Box".into(), origin_feature: 1, name: format!("face {name}"), path: Vec::new() };
    let side = |name: &str| Silhouette { face: face(name), side: solidcraft_geom::drawing_measure::SilhouetteSide::Far };
    let (direction, reading) = (LinearDirection::Aligned, Reading::Projected);
    let (a, b) = (SlotParam::A, SlotParam::B);
    let mut kinds = [
        (DimensionKind::Linear { a: point("a"), b: point("b"), direction, reading }, vec![(a, "corner a"), (b, "corner b")]),
        (DimensionKind::Length { edge: edge("e"), direction, reading }, vec![(SlotParam::Edge, "edge e")]),
        (DimensionKind::Angle { a: edge("a"), b: edge("b"), reading, other_side: false }, vec![(a, "edge a"), (b, "edge b")]),
        (DimensionKind::Radial { edge: edge("e"), diameter: false }, vec![(SlotParam::Edge, "edge e")]),
        (
            DimensionKind::Chain { points: vec![point("0"), point("1")], direction },
            vec![(SlotParam::Point(0), "corner 0"), (SlotParam::Point(1), "corner 1")],
        ),
        (DimensionKind::Cylinder { face: face("f") }, vec![(SlotParam::Face, "face f")]),
        (DimensionKind::Sphere { face: face("f") }, vec![(SlotParam::Face, "face f")]),
        (DimensionKind::Cone { face: face("f") }, vec![(SlotParam::Face, "face f")]),
        (DimensionKind::Silhouettes { a: side("a"), b: side("b"), direction }, vec![(a, "face a"), (b, "face b")]),
        (DimensionKind::Faces { a: face("a"), b: face("b"), direction }, vec![(a, "face a"), (b, "face b")]),
    ];
    let every = [a, b, SlotParam::Edge, SlotParam::Face, SlotParam::Point(0), SlotParam::Point(1), SlotParam::Point(2)];
    for (kind, slots) in &mut kinds {
        for wanted in every {
            let reached = match slot(kind, wanted) {
                Ok(Slot::Edge(e)) => Some(e.name.clone()),
                Ok(Slot::Point(p)) => Some(p.edges.concat()),
                Ok(Slot::Face(f)) => Some(f.name.clone()),
                Err(e) => {
                    assert!(e.to_string().contains("this dimension's slots are"), "{e}");
                    None
                }
            };
            let expected = slots.iter().find(|(s, _)| *s == wanted).map(|(_, name)| name.to_string());
            assert_eq!(reached, expected, "{wanted:?} of {kind:?}");
        }
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

fn workshop_plate_round_index(s: &Session, body: &str, centre: [f64; 3], radius: f64) -> usize {
    let centre = Vec3::from(centre);
    let state = s.model.state();
    let mesh = state.body(body).unwrap().mesh();
    mesh.edges.iter().position(|line| line.iter().all(|p| (p.dist(centre) - radius).abs() < 1e-6)).unwrap()
}

/// One command-script plate with two independently authored, positioned bore dimensions.
fn workshop_plate_dimensions() -> (Session, u64, Value, Value) {
    let mut s = Session::default();
    let script: Value = serde_json::from_str(include_str!("../../../../examples/acceptance/drawing_m2.json")).unwrap();
    let made = s.run_script(&script).unwrap();
    let top = made.last().unwrap()["view"].clone();
    let drawing = s.drawing("test").unwrap().id;
    let bore1 = json!({"body": "Plate", "index": workshop_plate_round_index(&s, "Plate", [20.0, 20.0, 20.0], 5.0)});
    let bore2 = json!({"body": "Plate", "index": workshop_plate_round_index(&s, "Plate", [60.0, 50.0, 20.0], 5.0)});
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
            "resized" => reads(&bore1, 12.0, "Ø12.00"),
            "deleted" => reads_lost_bore(&bore1),
            _ => reads(&bore1, 10.0, "Ø10.00"),
        }
    }
}

/// The workshop plate's width as a dimension between its two side faces, which the top view sees
/// edge-on (R1, #11871). It reads 80; with the width changed to 100 while the drawing is closed, the
/// same dimension reads 100 when it is opened again, and 80 after the change is undone.
#[test]
fn the_plates_width_between_its_side_faces_follows_a_width_change() {
    let mut s = Session::default();
    let script: Value = serde_json::from_str(include_str!("../../../../examples/acceptance/drawing_m2.json")).unwrap();
    let made = s.run_script(&script).unwrap();
    let top = made.last().unwrap()["view"].clone();
    let drawing = json!({"drawing": ids::encode(s.drawing("test").unwrap().id)});
    let width = run(&mut s, ADD, json!({"view": top, "kind": {"faces": {"a": [50, 0, 10], "b": [50, 80, 10]}}}));
    reads(&width, 80.0, "80.00");
    let after = |s: &mut Session, id: &str, p: Value| {
        run(s, "drawing.close", json!({}));
        run(s, id, p);
        run(s, "drawing.open", drawing.clone());
        let read = run(s, INSPECT, json!({"dimension": width["dimension"]}));
        assert_eq!(read["record"]["kind"], width["record"]["kind"], "the side faces' reference moved: {read}");
        read
    };
    reads(&after(&mut s, "parameters.change", json!({"name": "width", "expression": "100 mm"})), 100.0, "100.00");
    reads(&after(&mut s, "edit.undo", json!({})), 80.0, "80.00");
}

/// Bore1 suppressed reads `suppressed`, not lost (#11823), and keeps its reference; on again, the
/// same dimension reads Ø10 `ok`; deleted, it is lost and never moves to Bore2. Bore2 reads Ø10
/// throughout. Every model edit is made with the drawing closed.
#[test]
fn a_suppressed_bore_says_so_and_its_dimension_comes_back() {
    let (mut s, drawing, first, second) = workshop_plate_dimensions();
    let drawing = json!({"drawing": ids::encode(drawing)});
    let after = |s: &mut Session, id: &str, p: Value| {
        run(s, "drawing.close", json!({}));
        run(s, id, p);
        run(s, "drawing.open", drawing.clone());
        let [one, two] = [&first, &second].map(|d| run(s, INSPECT, json!({"dimension": d["dimension"]})));
        assert_eq!(one["record"]["kind"], first["record"]["kind"], "Bore1 reference moved: {one}");
        assert_eq!(two["record"]["kind"], second["record"]["kind"], "Bore2 reference moved: {two}");
        reads(&two, 10.0, "Ø10.00");
        one
    };
    let off = after(&mut s, "timeline.suppress", json!({"feature": "Bore1", "suppressed": true}));
    assert_eq!(off["status"], "suppressed", "{off}");
    let on = after(&mut s, "timeline.suppress", json!({"feature": "Bore1", "suppressed": false}));
    reads(&on, 10.0, "Ø10.00");
    reads_lost_bore(&after(&mut s, "timeline.delete", json!({"feature": "Bore1"})));
}

/// What the plate's five dimensions read on its own model: Bore1 Ø, Bore2 Ø, the width, the length
/// and Bore1's centre to Bore2's.
const PLATE: [f64; 5] = [10.0, 10.0, 80.0, 100.0, 50.0];

/// The workshop plate's top view with five dimensions: Bore1 Ø10, Bore2 Ø10, the width between the
/// side faces (80), the length between the ends (100) and Bore1's centre to Bore2's, aligned (50;
/// they are 40 across and 30 up), each centre named by its bore's rim (#9249). The session,
/// `{drawing}` to open it again, the view's id and the five as added, in that order.
fn plate_with_five() -> (Session, Value, u64, [Value; 5]) {
    let (mut s, drawing, first, second) = workshop_plate_dimensions();
    let view = first["view"].clone();
    let rim = |s: &Session, at: [f64; 3]| json!({"centre": {"body": "Plate", "index": workshop_plate_round_index(s, "Plate", at, 5.0)}});
    let (one, two) = (rim(&s, [20.0, 20.0, 20.0]), rim(&s, [60.0, 50.0, 20.0]));
    let mut put = |kind: Value| run(&mut s, ADD, json!({"view": view, "kind": kind}));
    let width = put(json!({"faces": {"a": [50, 0, 10], "b": [50, 80, 10]}}));
    let length = put(json!({"faces": {"a": [0, 40, 10], "b": [100, 40, 10]}}));
    let centres = put(json!({"linear": {"a": one, "b": two}}));
    let five = [first, second, width, length, centres];
    read_as(&mut s, &five, PLATE);
    let view = ids::decode(view.as_str().unwrap()).unwrap();
    (s, json!({"drawing": ids::encode(drawing)}), view, five)
}

/// Each of `five` read now, by the reference it was added with: `ok` with `values` in order, the
/// bores' as diameters.
fn read_as(s: &mut Session, five: &[Value; 5], values: [f64; 5]) {
    for (i, (added, value)) in five.iter().zip(values).enumerate() {
        let read = run(s, INSPECT, json!({"dimension": added["dimension"]}));
        assert_eq!(read["record"]["kind"], added["record"]["kind"], "dimension {i}'s reference moved: {read}");
        let text = if i < 2 { format!("Ø{value:.2}") } else { format!("{value:.2}") };
        reads(&read, value, &text);
    }
}

/// `id` with `p` made on the model, the drawing closed as a model edit has it, then opened again.
fn closed_edit(s: &mut Session, drawing: &Value, id: &str, p: Value) {
    run(s, "drawing.close", json!({}));
    run(s, id, p);
    run(s, "drawing.open", drawing.clone());
}

/// Each of the plate's parameters changed with the drawing closed moves only what it drives: Bore1's
/// diameter to 12, Bore2's to 8, the width to 100 and the length to 120, each read and undone back to
/// the plate with the authored drawing unchanged; then all four at once, undone one by one to the
/// plate and redone to all four. The bores' centres stay 50 apart throughout.
#[test]
fn the_plates_five_dimensions_follow_each_parameter_and_its_undo() {
    let (mut s, drawing, _, five) = plate_with_five();
    let authored = s.drawing("test").unwrap().to_json().unwrap();
    let changes = [("bore1_d", "12 mm", 0, 12.0), ("bore2_d", "8 mm", 1, 8.0), ("width", "100 mm", 2, 100.0), ("length", "120 mm", 3, 120.0)];
    for (name, expression, at, value) in changes {
        closed_edit(&mut s, &drawing, "parameters.change", json!({"name": name, "expression": expression}));
        let mut values = PLATE;
        values[at] = value;
        read_as(&mut s, &five, values);
        closed_edit(&mut s, &drawing, "edit.undo", json!({}));
        read_as(&mut s, &five, PLATE);
        assert_eq!(s.drawing("test").unwrap().to_json().unwrap(), authored, "{name}: the model's undo changed the authored drawing");
    }
    let all = [12.0, 8.0, 100.0, 120.0, 50.0];
    for (name, expression, _, _) in changes {
        closed_edit(&mut s, &drawing, "parameters.change", json!({"name": name, "expression": expression}));
    }
    read_as(&mut s, &five, all);
    for _ in changes {
        closed_edit(&mut s, &drawing, "edit.undo", json!({}));
    }
    read_as(&mut s, &five, PLATE);
    for _ in changes {
        closed_edit(&mut s, &drawing, "edit.redo", json!({}));
    }
    read_as(&mut s, &five, all);
    assert_eq!(s.drawing("test").unwrap().to_json().unwrap(), authored, "the model's redo changed the authored drawing");
}

/// Bore1's centre to Bore2's, each named by its rim (R2, #9249): 50 aligned, 40 across and 30 up the
/// top view. A new diameter moves no centre; Bore1 suppressed, all three say so; deleted, all three
/// are lost with their last values and read nothing else, while Bore2 reads Ø10; undone, they read
/// again; deleted again, they keep their last values through save and reopen.
#[test]
fn the_bores_centres_stay_put_and_a_lost_bore_takes_its_centre_dimensions() {
    let (mut s, drawing, view, five) = plate_with_five();
    let id = s.drawing("test").unwrap().id;
    let rim = |s: &Session, at: [f64; 3]| json!({"centre": {"body": "Plate", "index": workshop_plate_round_index(s, "Plate", at, 5.0)}});
    let (one, two) = (rim(&s, [20.0, 20.0, 20.0]), rim(&s, [60.0, 50.0, 20.0]));
    let mut put =
        |direction: &str| run(&mut s, ADD, json!({"view": ids::encode(view), "kind": {"linear": {"a": one, "b": two, "direction": direction}}}));
    let (across, up) = (put("horizontal"), put("vertical"));
    let three = [five[4].clone(), across, up];
    let values = [50.0, 40.0, 30.0];
    let read = |s: &mut Session| -> Vec<Value> {
        let one = |d: &Value| {
            let read = run(s, INSPECT, json!({"dimension": d["dimension"]}));
            assert_eq!(read["record"]["kind"], d["record"]["kind"], "a centre's reference moved: {read}");
            read
        };
        three.iter().map(one).collect()
    };
    let all_ok = |reads_now: Vec<Value>| {
        for (r, v) in reads_now.iter().zip(values) {
            reads(r, v, &format!("{v:.2}"));
        }
    };
    let all_lost = |reads_now: Vec<Value>| {
        for (r, v) in reads_now.iter().zip(values) {
            assert_eq!(r["status"], "lost", "{r}");
            assert!(r.get("value").is_none() && r.get("text").is_none(), "a lost centre reads nothing: {r}");
            assert!(r["why"].as_str().is_some_and(|why| !why.is_empty()), "{r}");
            assert!(r["last_good"].as_f64().is_some_and(|last| (last - v).abs() < 1e-9), "its last value is {v}: {r}");
        }
    };
    all_ok(read(&mut s));
    closed_edit(&mut s, &drawing, "parameters.change", json!({"name": "bore1_d", "expression": "12 mm"}));
    all_ok(read(&mut s));
    closed_edit(&mut s, &drawing, "edit.undo", json!({}));
    closed_edit(&mut s, &drawing, "timeline.suppress", json!({"feature": "Bore1", "suppressed": true}));
    assert!(read(&mut s).iter().all(|r| r["status"] == "suppressed" && r.get("value").is_none()), "{:?}", read(&mut s));
    closed_edit(&mut s, &drawing, "timeline.suppress", json!({"feature": "Bore1", "suppressed": false}));
    all_ok(read(&mut s));
    closed_edit(&mut s, &drawing, "timeline.delete", json!({"feature": "Bore1"}));
    all_lost(read(&mut s));
    reads(&run(&mut s, INSPECT, json!({"dimension": five[1]["dimension"]})), 10.0, "Ø10.00");
    closed_edit(&mut s, &drawing, "edit.undo", json!({}));
    all_ok(read(&mut s));
    closed_edit(&mut s, &drawing, "edit.redo", json!({}));
    all_lost(read(&mut s));
    let mut opened = reopened(&mut s, id, "w23a-centres.solidcraft");
    all_lost(read(&mut opened));
}

/// Sheet 1 of the drawing open, drawn at once, as the sheet page and an export draw it.
fn sheet_now(s: &Session) -> crate::drawing_scene::SheetScene {
    let d = s.drawing("test").unwrap().clone();
    crate::drawing_scene::build(s, &d, d.sheets[0].id).unwrap()
}

/// `added`'s lines on `sheet` as JSON, if it is drawn.
fn lines_of(sheet: &crate::drawing_scene::SheetScene, added: &Value) -> Option<Value> {
    let id = ids::decode(added["dimension"].as_str().unwrap()).unwrap();
    sheet.scene.groups.iter().find(|g| g.object == id).map(|g| serde_json::to_value(g).unwrap())
}

/// Whether `a` and `b` are the same JSON, every whole number exactly and every other within 1e-9:
/// a model evaluated again can move a point by a rounding.
fn near_json(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => match (x.as_u64(), y.as_u64()) {
            (Some(x), Some(y)) => x == y,
            _ => x.as_f64().zip(y.as_f64()).is_some_and(|(x, y)| (x - y).abs() < 1e-9),
        },
        (Value::Array(x), Value::Array(y)) => x.len() == y.len() && x.iter().zip(y).all(|(x, y)| near_json(x, y)),
        (Value::Object(x), Value::Object(y)) => x.len() == y.len() && x.iter().all(|(k, v)| y.get(k).is_some_and(|w| near_json(v, w))),
        _ => a == b,
    }
}

/// Whether two of [`lines_of`]'s answers are the same lines, as [`near_json`] has it.
fn same_lines(a: &Option<Value>, b: &Option<Value>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => near_json(a, b),
        (None, None) => true,
        _ => false,
    }
}

/// `sheet`'s scene as JSON.
fn scene_of(sheet: &crate::drawing_scene::SheetScene) -> Value {
    serde_json::to_value(&sheet.scene).unwrap()
}

/// Where `added` stands on `sheet`.
fn state_of<'a>(sheet: &'a crate::drawing_scene::SheetScene, added: &Value) -> &'a crate::drawing_scene::DimensionState {
    let id = ids::decode(added["dimension"].as_str().unwrap()).unwrap();
    sheet.dimensions.iter().find(|d| d.dimension == id).unwrap()
}

/// How far apart the tips of `added`'s two arrowheads are on `sheet`, in paper millimetres.
fn tips_apart(sheet: &crate::drawing_scene::SheetScene, added: &Value) -> f64 {
    let id = ids::decode(added["dimension"].as_str().unwrap()).unwrap();
    let group = sheet.scene.groups.iter().find(|g| g.object == id).unwrap();
    let tips: Vec<solidcore_record::Vec2> = group
        .items
        .iter()
        .filter_map(|item| match item {
            solidcraft_geom::paper::Item::Fill { region } => Some(region.outer.segs[0].start()),
            _ => None,
        })
        .collect();
    assert_eq!(tips.len(), 2, "{}: {tips:?}", added["dimension"]);
    tips[0].dist(tips[1])
}

/// The box round view `view`'s lines on `sheet`, as `[min, max]` on paper.
fn view_box(sheet: &crate::drawing_scene::SheetScene, view: u64) -> [[f64; 2]; 2] {
    let o = sheet.objects.iter().find(|o| o.object == view).unwrap();
    [[o.bounds_mm.min.x, o.bounds_mm.min.y], [o.bounds_mm.max.x, o.bounds_mm.max.y]]
}

/// Whether two boxes are the same within 1e-6 mm.
fn same_box(a: [[f64; 2]; 2], b: [[f64; 2]; 2]) -> bool {
    a.iter().flatten().zip(b.iter().flatten()).all(|(x, y)| (x - y).abs() < 1e-6)
}

/// The plate's sheet as the model has it (#8288 E2E-01 to E2E-04 on paper; no text yet): its top
/// view at 1:2 round (150, 180), 50 x 40, with all five dimensions drawn, the width's arrowheads 40
/// apart, the length's 50 and the centres' 25. Bore1 at Ø12 redraws the view and Bore1's dimension,
/// and the others stay where they were. Bore1 deleted leaves out its two dimensions, which
/// `drawing.scene` and `drawing.status` say are lost, keeps the other three where they were and
/// still exports. Each undo draws the sheet again as it was, every point within 1e-9 mm, and exports
/// the same SVG. The length at 120 widens the view to 60 round the same place and the length's
/// arrowheads to 60, and a save and reopen draws the same sheet.
#[test]
fn the_plates_sheet_draws_what_the_model_has_and_the_same_after_undo() {
    let (mut s, drawing, view, five) = plate_with_five();
    let id = s.drawing("test").unwrap().id;
    let svg = |s: &mut Session| run(s, "drawing.export", json!({"sheet": "Sheet 1"}))["svg"].as_str().unwrap().to_string();
    let view_lines =
        |sheet: &crate::drawing_scene::SheetScene| sheet.scene.groups.iter().find(|g| g.object == view).map(|g| serde_json::to_value(g).unwrap());
    let base = sheet_now(&s);
    let base_svg = svg(&mut s);
    let as_it_was = |s: &mut Session, label: &str| {
        assert!(near_json(&scene_of(&sheet_now(s)), &scene_of(&base)), "{label}: the sheet isn't drawn as it was");
        assert_eq!(svg(s), base_svg, "{label}: the export isn't the one it was");
    };
    assert!(base.views.iter().all(|v| v.status.name() == "drawn"), "{:?}", base.views);
    assert!(same_box(view_box(&base, view), [[125.0, 160.0], [175.0, 200.0]]), "{:?}", view_box(&base, view));
    for added in &five {
        let state = state_of(&base, added);
        assert!(state.drawn && state.status == Status::Ok, "{state:?}");
    }
    for (at, apart) in [(2, 40.0), (3, 50.0), (4, 25.0)] {
        assert!((tips_apart(&base, &five[at]) - apart).abs() < 1e-6, "dimension {at}: {}", tips_apart(&base, &five[at]));
    }

    closed_edit(&mut s, &drawing, "parameters.change", json!({"name": "bore1_d", "expression": "12 mm"}));
    let wider = sheet_now(&s);
    assert!(!same_lines(&lines_of(&wider, &five[0]), &lines_of(&base, &five[0])), "Bore1's dimension is drawn at Ø12");
    assert!(!same_lines(&view_lines(&wider), &view_lines(&base)), "the view draws Bore1 at Ø12");
    for added in &five[1..] {
        assert!(same_lines(&lines_of(&wider, added), &lines_of(&base, added)), "only Bore1's dimension changes: {}", added["dimension"]);
    }
    closed_edit(&mut s, &drawing, "edit.undo", json!({}));
    as_it_was(&mut s, "Bore1 at 12 undone");

    closed_edit(&mut s, &drawing, "timeline.delete", json!({"feature": "Bore1"}));
    let lost = sheet_now(&s);
    for added in [&five[0], &five[4]] {
        let state = state_of(&lost, added);
        assert!(!state.drawn && matches!(state.status, Status::Lost(..)), "{state:?}");
        assert_eq!(lines_of(&lost, added), None, "a lost dimension draws nothing");
    }
    for added in &five[1..4] {
        assert!(state_of(&lost, added).drawn, "{:?}", state_of(&lost, added));
        assert!(same_lines(&lines_of(&lost, added), &lines_of(&base, added)), "{} is drawn as it was", added["dimension"]);
    }
    assert!(view_lines(&lost).is_some(), "the view is still drawn");
    let status = run(&mut s, "drawing.status", json!({}));
    let word =
        |added: &Value| status["dimensions"].as_array().unwrap().iter().find(|d| d["dimension"] == added["dimension"]).map(|d| d["status"].clone());
    let words: Vec<Option<Value>> = five.iter().map(word).collect();
    assert_eq!(words, ["lost", "ok", "ok", "ok", "lost"].map(|w| Some(json!(w))).to_vec(), "{status}");
    assert_ne!(svg(&mut s), base_svg, "the sheet with Bore1 lost still exports, without it");
    closed_edit(&mut s, &drawing, "edit.undo", json!({}));
    as_it_was(&mut s, "Bore1's delete undone");

    closed_edit(&mut s, &drawing, "parameters.change", json!({"name": "length", "expression": "120 mm"}));
    let longer = sheet_now(&s);
    assert!(same_box(view_box(&longer, view), [[120.0, 160.0], [180.0, 200.0]]), "{:?}", view_box(&longer, view));
    for (at, apart) in [(2, 40.0), (3, 60.0), (4, 25.0)] {
        assert!((tips_apart(&longer, &five[at]) - apart).abs() < 1e-6, "dimension {at}: {}", tips_apart(&longer, &five[at]));
    }
    assert!(five.iter().all(|added| state_of(&longer, added).drawn));
    let opened = reopened(&mut s, id, "w23a-sheet.solidcraft");
    let again = sheet_now(&opened);
    assert!(near_json(&scene_of(&again), &scene_of(&longer)), "the reopened sheet draws the same");
}

/// Fillets that take the plate's corners leave its faces measured (#11871): the upright edge where
/// the near end meets the front side, rounded 5, and the top edge along the front side, rounded 2.
/// The width still reads 80 and the length 100, and the bores and their centres as they were; each
/// fillet undone, the same.
#[test]
fn fillets_on_the_plates_corners_leave_its_five_dimensions_read() {
    let (mut s, drawing, _, five) = plate_with_five();
    for (edge, radius) in [([0, 0, 10], 5), ([50, 0, 20], 2)] {
        closed_edit(&mut s, &drawing, "solid.fillet", json!({"body": "Plate", "radius": radius, "edges": [edge]}));
        read_as(&mut s, &five, PLATE);
        closed_edit(&mut s, &drawing, "edit.undo", json!({}));
        read_as(&mut s, &five, PLATE);
    }
}

/// A dimension on a hole that an undo took away stays lost when a new hole is made, whatever id the
/// new hole gets (gap 2 on #4892, #5308): Bore3, Ø6 at (80, 20), gets a dimension; the hole is
/// undone, and the dimension is lost with its last value 6; then Bore4, Ø8 at (80, 60), is made.
/// The dimension stays lost with its last value 6 and reads nothing of Bore4, and Bore1 and Bore2
/// still read Ø10. Every model edit is made with the drawing closed.
#[test]
fn a_new_hole_never_takes_the_dimension_of_an_undone_one() {
    let (mut s, drawing, first, second) = workshop_plate_dimensions();
    let drawing = json!({"drawing": ids::encode(drawing)});
    let hole = |name: &str, at: [f64; 3], diameter: f64| json!({"position": at, "direction": [0, 0, -1], "diameter": diameter, "name": name, "type": "simple"});
    closed_edit(&mut s, &drawing, "solid.hole", hole("Bore3", [80.0, 20.0, 20.0], 6.0));
    let rim = json!({"body": "Plate", "index": workshop_plate_round_index(&s, "Plate", [80.0, 20.0, 20.0], 3.0)});
    let on_bore3 = run(&mut s, ADD, json!({"view": first["view"], "kind": {"radial": {"edge": rim}}}));
    reads(&on_bore3, 6.0, "Ø6.00");
    let lost_as_bore3 = |read: &Value| {
        assert_eq!(read["record"]["kind"], on_bore3["record"]["kind"], "Bore3's reference moved: {read}");
        assert_eq!(read["status"], "lost", "Bore3's dimension reads another hole: {read}");
        assert!(read.get("value").is_none() && read.get("text").is_none(), "a lost dimension reads nothing: {read}");
        assert!(read["last_good"].as_f64().is_some_and(|last| (last - 6.0).abs() < 1e-9), "its last value is 6: {read}");
    };
    closed_edit(&mut s, &drawing, "edit.undo", json!({}));
    lost_as_bore3(&run(&mut s, INSPECT, json!({"dimension": on_bore3["dimension"]})));
    closed_edit(&mut s, &drawing, "solid.hole", hole("Bore4", [80.0, 60.0, 20.0], 8.0));
    lost_as_bore3(&run(&mut s, INSPECT, json!({"dimension": on_bore3["dimension"]})));
    for bore in [&first, &second] {
        reads(&run(&mut s, INSPECT, json!({"dimension": bore["dimension"]})), 10.0, "Ø10.00");
    }
}

/// The lost reference of `added`, read now: `lost` with Bore1's last Ø10, its record as `record`.
fn lost_again(s: &mut Session, added: &Value, record: &Value) -> Value {
    let read = run(s, INSPECT, json!({"dimension": added["dimension"]}));
    reads_lost_bore(&read);
    assert_eq!(&read["record"], record, "a lost dimension keeps its record: {read}");
    read
}

/// The edge name in a radial dimension's record.
fn rim_name(record: &Value) -> &Value {
    &record["kind"]["radial"]["edge"]["name"]
}

/// Bore1's rim rounded 1 with the drawing closed (#4892 B5): its edge is gone, so Bore1's
/// dimension is lost with its last Ø10 and some leads, and Bore2 still reads Ø10. Repaired to the
/// circle where the round meets the bore, 1 below the top, it reads Ø10 again as one undo step, with
/// its id, view and text place, and only the edge's name changed. Undone it's lost with the same
/// leads, redone it reads; the same after save and reopen, where the file's copy also keeps the last
/// value. With the round undone the repaired reference is lost and stays as it was put, and with the
/// round redone it reads again. The repair undone and then the round,
/// Bore1's dimension reads Ø10 by its first record.
#[test]
fn a_rounded_bore_rim_is_lost_and_a_repair_reads_it_again() {
    let (mut s, id, first, second) = workshop_plate_dimensions();
    let drawing = json!({"drawing": ids::encode(id)});
    closed_edit(&mut s, &drawing, "solid.fillet", json!({"body": "Plate", "radius": 1, "edges": [[25, 20, 20]]}));
    let lost = lost_again(&mut s, &first, &first["record"]);
    reads(&run(&mut s, INSPECT, json!({"dimension": second["dimension"]})), 10.0, "Ø10.00");
    let leads = lost["candidates"].as_array().cloned().unwrap_or_default();
    let pick = json!({"edge": {"body": "Plate", "index": workshop_plate_round_index(&s, "Plate", [20.0, 20.0, 19.0], 5.0)}});
    assert!(leads.contains(&pick), "the bore's circle 1 below the top is a lead: {leads:?}");
    let before = s.drawing("test").unwrap().to_json().unwrap();
    let steps = s.undo.len();
    let repaired = run(&mut s, REPAIR, json!({"dimension": first["dimension"], "slot": "edge", "pick": pick}));
    reads(&repaired, 10.0, "Ø10.00");
    assert_eq!(s.undo.len(), steps + 1, "one undo step");
    assert_eq!((&repaired["dimension"], &repaired["view"]), (&first["dimension"], &first["view"]));
    assert_ne!(rim_name(&repaired["record"]), rim_name(&first["record"]), "{repaired}");
    let mut renamed = first["record"].clone();
    renamed["kind"]["radial"]["edge"]["name"] = rim_name(&repaired["record"]).clone();
    assert_eq!(renamed, repaired["record"], "only the edge's name changed");
    assert_eq!(repaired["record"]["text"], json!({"at": [12.0, 24.0]}));
    let after = s.drawing("test").unwrap().to_json().unwrap();

    run(&mut s, "edit.undo", json!({}));
    assert_eq!(s.drawing("test").unwrap().to_json().unwrap(), before, "the repair undone");
    let again = lost_again(&mut s, &first, &first["record"]);
    assert_eq!(again["candidates"], lost["candidates"], "the same leads");
    run(&mut s, "edit.redo", json!({}));
    assert_eq!(s.drawing("test").unwrap().to_json().unwrap(), after, "the repair redone");
    reads(&run(&mut s, INSPECT, json!({"dimension": first["dimension"]})), 10.0, "Ø10.00");

    let mut opened = reopened(&mut s, id, "w23-repair-rounded-rim.solidcraft");
    let read = run(&mut opened, INSPECT, json!({"dimension": first["dimension"]}));
    reads(&read, 10.0, "Ø10.00");
    // The file's copy of a record also keeps its last good value.
    let mut saved = read["record"].clone();
    let last_good = saved.as_object_mut().and_then(|record| record.remove("last_good"));
    // The bore measures 10 to within rounding (9.999999999999995 on solidcore), as `reads` takes it.
    assert!(last_good.as_ref().and_then(Value::as_f64).is_some_and(|v| (v - 10.0).abs() < 1e-9), "{read}");
    assert_eq!(saved, repaired["record"], "the repair is saved");

    closed_edit(&mut s, &drawing, "edit.undo", json!({}));
    let unrounded = run(&mut s, INSPECT, json!({"dimension": first["dimension"]}));
    assert_eq!(unrounded["status"], "lost", "the repaired edge is the round's: {unrounded}");
    assert_eq!(unrounded["record"], repaired["record"], "and the repair stays as it was put");
    reads(&run(&mut s, INSPECT, json!({"dimension": second["dimension"]})), 10.0, "Ø10.00");
    closed_edit(&mut s, &drawing, "edit.redo", json!({}));
    reads(&run(&mut s, INSPECT, json!({"dimension": first["dimension"]})), 10.0, "Ø10.00");

    run(&mut s, "edit.undo", json!({}));
    lost_again(&mut s, &first, &first["record"]);
    closed_edit(&mut s, &drawing, "edit.undo", json!({}));
    let back = run(&mut s, INSPECT, json!({"dimension": first["dimension"]}));
    reads(&back, 10.0, "Ø10.00");
    assert_eq!(back["record"], first["record"], "the first record reads again");
}

/// Bore1 deleted with the drawing closed: its dimension is lost and never reads Bore2 by itself. Its
/// leads come only from the features that named its edge, so they are the plate's and Bore2's rim
/// isn't among them. Repaired to Bore2's rim on purpose, it reads Ø10 by the name Bore2's own dimension has, as one
/// undo step. Bore1's delete undone, the repaired dimension stays on Bore2; the repair undone, it
/// reads Bore1 by its first record again.
#[test]
fn a_deleted_bores_dimension_reads_bore2_only_when_repaired_to_it() {
    let (mut s, id, first, second) = workshop_plate_dimensions();
    let drawing = json!({"drawing": ids::encode(id)});
    closed_edit(&mut s, &drawing, "timeline.delete", json!({"feature": "Bore1"}));
    let lost = lost_again(&mut s, &first, &first["record"]);
    let rim = json!({"edge": {"body": "Plate", "index": workshop_plate_round_index(&s, "Plate", [60.0, 50.0, 20.0], 5.0)}});
    let leads = lost["candidates"].as_array().cloned().unwrap_or_default();
    assert!(!leads.is_empty() && !leads.contains(&rim), "leads come from the plate, never Bore2's rim: {leads:?}");
    let steps = s.undo.len();
    let repaired = run(&mut s, REPAIR, json!({"dimension": first["dimension"], "slot": "edge", "pick": rim}));
    reads(&repaired, 10.0, "Ø10.00");
    assert_eq!(s.undo.len(), steps + 1, "one undo step");
    assert_eq!(rim_name(&repaired["record"]), rim_name(&second["record"]), "Bore2's rim, as Bore2's dimension names it: {repaired}");
    assert_eq!((&repaired["dimension"], &repaired["record"]["text"]), (&first["dimension"], &json!({"at": [12.0, 24.0]})));
    reads(&run(&mut s, INSPECT, json!({"dimension": second["dimension"]})), 10.0, "Ø10.00");

    closed_edit(&mut s, &drawing, "edit.undo", json!({}));
    let kept = run(&mut s, INSPECT, json!({"dimension": first["dimension"]}));
    reads(&kept, 10.0, "Ø10.00");
    assert_eq!(kept["record"], repaired["record"], "a repair stays where it was put");
    run(&mut s, "edit.undo", json!({}));
    let back = run(&mut s, INSPECT, json!({"dimension": first["dimension"]}));
    reads(&back, 10.0, "Ø10.00");
    assert_eq!(back["record"], first["record"], "Bore1 by its first record again");
}

/// The plate renamed Base with the drawing closed keeps its five dimensions (gap 1 on #4892, #5308):
/// each reads as before, and by the reference it was added with once the rename is undone. Today a
/// reference names its body, so a rename loses all five.
#[test]
fn renaming_the_plate_keeps_its_five_dimensions() {
    let (mut s, drawing, _, five) = plate_with_five();
    closed_edit(&mut s, &drawing, "body.rename", json!({"body": "Plate", "name": "Base"}));
    for (i, (added, value)) in five.iter().zip(PLATE).enumerate() {
        let text = if i < 2 { format!("Ø{value:.2}") } else { format!("{value:.2}") };
        reads(&run(&mut s, INSPECT, json!({"dimension": added["dimension"]})), value, &text);
    }
    closed_edit(&mut s, &drawing, "edit.undo", json!({}));
    read_as(&mut s, &five, PLATE);
}

/// A point of a reply, `[x, y]`.
fn xy(v: &Value) -> Vec2 {
    Vec2::new(v[0].as_f64().unwrap(), v[1].as_f64().unwrap())
}

/// `reply`'s layout lines under `key` (`extension_lines` or `dimension_lines`), point by point.
fn layout_lines(reply: &Value, key: &str) -> Vec<Vec<Vec2>> {
    let lines = reply["layout"][key].as_array().unwrap_or_else(|| panic!("no layout.{key}: {reply}"));
    lines.iter().map(|line| line.as_array().unwrap().iter().map(xy).collect()).collect()
}

/// `reply`'s layout arrows: each tip, and the unit direction it points in.
fn layout_arrows(reply: &Value) -> Vec<(Vec2, Vec2)> {
    let arrows = reply["layout"]["arrows"].as_array().unwrap_or_else(|| panic!("no layout.arrows: {reply}"));
    arrows.iter().map(|arrow| (xy(&arrow["tip"]), xy(&arrow["points"]))).collect()
}

/// `reply`'s layout text boxes, corner by corner.
fn layout_boxes(reply: &Value) -> Vec<Vec<Vec2>> {
    let boxes = reply["layout"]["text_boxes"].as_array().unwrap_or_else(|| panic!("no layout.text_boxes: {reply}"));
    boxes.iter().map(|corners| corners.as_array().unwrap().iter().map(xy).collect()).collect()
}

/// Every point of `reply`'s layout, in order: its extension lines', its dimension lines', its arrows'
/// tips and its text boxes' corners.
fn layout_points(reply: &Value) -> Vec<Vec2> {
    let lines = ["extension_lines", "dimension_lines"].into_iter().flat_map(|key| layout_lines(reply, key)).flatten();
    lines.chain(layout_arrows(reply).into_iter().map(|(tip, _)| tip)).chain(layout_boxes(reply).into_iter().flatten()).collect()
}

/// Whether `a` and `b` are the same points, each within `tolerance` mm.
fn same_points(a: &[Vec2], b: &[Vec2], tolerance: f64) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(p, q)| p.dist(*q) <= tolerance)
}

/// What `added`'s group on `sheet` draws: its strokes point by point, its arrowheads' tips and its
/// text runs' origins.
fn drawn_parts(sheet: &crate::drawing_scene::SheetScene, added: &Value) -> (Vec<Vec<Vec2>>, Vec<Vec2>, Vec<Vec2>) {
    use solidcraft_geom::paper::Item;
    let id = ids::decode(added["dimension"].as_str().unwrap()).unwrap();
    let group = sheet.scene.groups.iter().find(|g| g.object == id).unwrap_or_else(|| panic!("{} isn't drawn", added["dimension"]));
    let (mut strokes, mut tips, mut origins) = (Vec::new(), Vec::new(), Vec::new());
    for item in &group.items {
        match item {
            Item::Stroke { path, .. } => strokes.push(std::iter::once(path[0].start()).chain(path.iter().map(|seg| seg.end())).collect()),
            Item::Fill { region } => tips.push(region.outer.segs[0].start()),
            Item::Text(text) => origins.push(text.origin),
            Item::Picture(_) => {}
        }
    }
    (strokes, tips, origins)
}

/// Sheet 1 of the drawing open as a command reads it ([`crate::drawing_scene::as_shown`]): for a
/// deferred drawing, what its last update drew.
fn shown_now(s: &mut Session) -> crate::drawing_scene::SheetScene {
    let d = s.drawing("test").unwrap().clone();
    crate::drawing_scene::as_shown(s, &d, d.sheets[0].id).unwrap()
}

/// What `drawing.scene` says of `added` on the drawing's first sheet: its `why`.
fn scene_why(s: &mut Session, added: &Value) -> Value {
    let sheet = ids::encode(s.drawing("test").unwrap().sheets[0].id);
    let scene = run(s, "drawing.scene", json!({"sheet": sheet}));
    let listed = scene["dimensions"].as_array().and_then(|all| all.iter().find(|d| d["dimension"] == added["dimension"]));
    listed.map(|d| d["why"].clone()).unwrap_or_else(|| panic!("{} isn't listed: {scene}", added["dimension"]))
}

/// Inspect says where a dimension sits on its sheet, in paper mm from the sheet's lower-left corner.
/// The front view at 1:1 has the middle of the box's outline (x 40, z 15) on its centre (200, 150),
/// so the top front edge (z 30, x 0 to 80) is at y 150 + 30 - 15 = 165, from x 200 - 40 = 160 to 240.
/// Its length sits 10 above it by default: the dimension line at 175 from tip to tip; the extension
/// lines from the gap above the edge (166) to the overshoot past the line (175 + 2 = 177); an arrow
/// at each end pointing out; and the text's box over x 200, as wide as "80.00" measures, from the gap
/// above the line (176) to the capitals' 3.5 above that (179.5).
#[test]
fn inspect_says_where_a_dimension_sits_on_its_sheet() {
    let (mut s, view) = front_view();
    let added = add(&mut s, view, json!({"length": {"edge": [40, 0, 30]}}));
    let read = run(&mut s, INSPECT, json!({"dimension": added["dimension"]}));
    assert!(read.get("layout_why").is_none(), "{read}");
    let at = Vec2::new;
    // Left to right, whichever end of the edge is read first.
    let mut extensions = layout_lines(&read, "extension_lines");
    extensions.sort_by(|a, b| a[0].x.total_cmp(&b[0].x));
    assert_eq!(extensions.len(), 2, "{read}");
    assert!(same_points(&extensions[0], &[at(160.0, 166.0), at(160.0, 177.0)], 1e-9), "{read}");
    assert!(same_points(&extensions[1], &[at(240.0, 166.0), at(240.0, 177.0)], 1e-9), "{read}");
    let along = layout_lines(&read, "dimension_lines");
    assert!(along.len() == 1 && same_points(&along[0], &[at(160.0, 175.0), at(240.0, 175.0)], 1e-9), "{read}");
    let arrows: Vec<Vec2> = layout_arrows(&read).into_iter().flat_map(|(tip, dir)| [tip, dir]).collect();
    assert!(same_points(&arrows, &[at(160.0, 175.0), at(-1.0, 0.0), at(240.0, 175.0), at(1.0, 0.0)], 1e-9), "{read}");
    let half = measured("80.00") * 0.5;
    let boxes = layout_boxes(&read);
    let corners = [at(200.0 - half, 176.0), at(200.0 + half, 176.0), at(200.0 + half, 179.5), at(200.0 - half, 179.5)];
    assert!(boxes.len() == 1 && same_points(&boxes[0], &corners, 1e-9), "{read}");
}

/// The layout is where the sheet draws it, exactly, with no tolerance: on the plate's sheet, each of
/// the five dimensions' extension lines is one of its group's strokes, its arrows' tips are its
/// arrowheads' tips, and its text boxes' first corners are its text runs' origins, as one function
/// lays both out through the same frame and both are moved by the view's centre. Asked before the
/// sheet is drawn and after, the same. With the view moved from (150, 180) to (162, 173), every point
/// moves by (12, -7) (within a rounding: the centre is added to the point once, not in two steps).
#[test]
fn the_layout_is_where_the_sheet_draws_it() {
    let (mut s, _, view, five) = plate_with_five();
    let read = |s: &mut Session| five.iter().map(|added| run(s, INSPECT, json!({"dimension": added["dimension"]}))).collect::<Vec<_>>();
    let before = read(&mut s);
    let sheet = sheet_now(&s);
    for (added, reply) in five.iter().zip(&before) {
        let (strokes, tips, origins) = drawn_parts(&sheet, added);
        for line in layout_lines(reply, "extension_lines") {
            assert!(strokes.contains(&line), "{}: {line:?} isn't drawn: {strokes:?}", added["dimension"]);
        }
        let arrows: Vec<Vec2> = layout_arrows(reply).into_iter().map(|(tip, _)| tip).collect();
        assert_eq!(arrows, tips, "{}", added["dimension"]);
        let firsts: Vec<Vec2> = layout_boxes(reply).into_iter().map(|corners| corners[0]).collect();
        assert_eq!(firsts, origins, "{}", added["dimension"]);
    }
    assert_eq!(read(&mut s), before, "the same once the sheet is drawn");

    run(&mut s, "drawing.view.edit", json!({"view": ids::encode(view), "set": {"place": {"at": [162.0, 173.0]}}}));
    let by = Vec2::new(12.0, -7.0);
    for (was, now) in before.iter().zip(read(&mut s)) {
        let moved: Vec<Vec2> = layout_points(was).into_iter().map(|p| p + by).collect();
        assert!(same_points(&layout_points(&now), &moved, 1e-9), "{was} {now}");
        let directions = |reply: &Value| layout_arrows(reply).into_iter().map(|(_, dir)| dir).collect::<Vec<_>>();
        assert_eq!(directions(&now), directions(was));
    }
}

/// A dimension that isn't drawn has no layout, and says why where its status doesn't, in the words the
/// sheet uses. Lost (its edge filleted away): no layout, and nothing more than its status. Its view
/// drawing nothing (its only part, Block, hidden, as a file can hold it: a placeholder) or in a shaded
/// style with lines (which the sheet can't draw yet): "its view isn't drawn", as `drawing.scene` says.
/// Its view drawing an exploded view (set after the add, as a file can hold it) is no longer one of
/// these: it is drawn where that view draws its parts (#13048), so it has a layout, its arrows' tips
/// where the sheet draws its arrowheads, and `drawing.check` holds nothing back.
#[test]
fn a_dimension_not_drawn_has_no_layout_and_says_why() {
    let (mut s, drawing, dimension) = read_once();
    lose_the_edge(&mut s);
    run(&mut s, "drawing.open", json!({"drawing": ids::encode(drawing)}));
    let lost = inspect_of(&mut s, dimension);
    assert_eq!(lost["status"], json!("lost"), "{lost}");
    assert!(lost.get("layout").is_none() && lost.get("layout_why").is_none(), "{lost}");

    let not_drawn = json!("its view isn't drawn");
    let mut s = Session::default();
    let block = run(&mut s, "component.create", json!({"name": "Block"}))["occurrence"].clone();
    run(&mut s, "solid.box", json!({"length": 20, "width": 30, "height": 10, "body_name": "Block", "operation": "new"}));
    run(&mut s, "component.activate", json!({"component": 0}));
    let (mut s, view) = drawn_from_the_front(s);
    let on_block = add(&mut s, view, json!({"length": {"edge": {"point": [10, 0, 10], "path": [block]}}}));
    assert!(on_block.get("layout").is_some(), "drawn before the hide: {on_block}");
    let mut d = (**s.drawing("test").unwrap()).clone();
    let record = d.views.iter_mut().find(|v| v.id == view).unwrap();
    Arc::make_mut(record).settings.hide = Some(vec![vec![block.as_u64().unwrap()]].into());
    s.doc_mut().put_drawing(Arc::new(d));
    let hidden = run(&mut s, INSPECT, json!({"dimension": on_block["dimension"]}));
    assert!(hidden.get("layout").is_none(), "{hidden}");
    assert_eq!((&hidden["layout_why"], &scene_why(&mut s, &on_block)), (&not_drawn, &not_drawn), "{hidden}");

    let (mut s, view) = front_view();
    let added = add(&mut s, view, json!({"length": {"edge": [40, 0, 30]}}));
    run(&mut s, "drawing.view.edit", json!({"view": ids::encode(view), "set": {"visual_style": "shaded_visible"}}));
    let shaded = run(&mut s, INSPECT, json!({"dimension": added["dimension"]}));
    assert!(shaded.get("layout").is_none(), "{shaded}");
    assert_eq!((&shaded["layout_why"], &scene_why(&mut s, &added)), (&not_drawn, &not_drawn), "{shaded}");

    // Two components, as an exploded view pulls components apart; Apart lifts the Bolt 50.
    let mut s = Session::default();
    let bolt = run(&mut s, "component.create", json!({"name": "Bolt"}))["occurrence"].clone();
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "body_name": "Bolt", "operation": "new"}));
    run(&mut s, "component.activate", json!({"component": 0}));
    run(&mut s, "component.create", json!({"name": "Nut"}));
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [30, 0, 0], "body_name": "Nut", "operation": "new"}));
    run(&mut s, "component.activate", json!({"component": 0}));
    run(&mut s, "explode.create", json!({"name": "Apart", "moves": [{"occurrence": bolt, "translate": [0, 0, 50]}]}));
    let (mut s, view) = drawn_from_the_front(s);
    let on_bolt = add(&mut s, view, json!({"length": {"edge": {"point": [5, 0, 10], "path": [bolt]}}}));
    let mut d = (**s.drawing("test").unwrap()).clone();
    let record = d.views.iter_mut().find(|v| v.id == view).unwrap();
    Arc::make_mut(record).settings.exploded = Some(solidcraft_doc::drawing_view::Exploded::View(s.doc.assembly.exploded[0].id));
    d.validate().unwrap();
    s.doc_mut().put_drawing(Arc::new(d));
    let apart = run(&mut s, INSPECT, json!({"dimension": on_bolt["dimension"]}));
    assert!(apart.get("layout").is_some() && apart.get("layout_why").is_none(), "{apart}");
    let tips: Vec<Vec2> = layout_arrows(&apart).into_iter().map(|(tip, _)| tip).collect();
    let (_, drawn, _) = drawn_parts(&sheet_now(&s), &on_bolt);
    assert_eq!(tips, drawn, "the layout is where the sheet draws the bolt, moved apart");
    let check = run(&mut s, "drawing.check", json!({}));
    let of_bolt = |f: &&Value| f["kind"] == "out_of_date" && f["object"]["id"] == on_bolt["dimension"];
    assert!(check["findings"].as_array().is_some_and(|all| !all.iter().any(|f| of_bolt(&f))), "{check}");
}

/// Add and repair answer with the layout inspect gives. The top front edge's length, added, answers
/// what inspect then answers, layout and all. Lost to a fillet of 5, it has none. Repaired to the
/// fillet's front edge, it has its layout again: that edge is at z 25, so at y 150 + 25 - 15 = 160 on
/// the sheet (the box's outline is still 0 to 30 high, its middle at z 15), and its line 10 above it
/// at 170, from x 160 to 240, the arrows pointing out; inspect then gives the same layout.
#[test]
fn add_and_repair_answer_with_the_layout_inspect_gives() {
    let (mut s, view) = front_view();
    let drawing = json!({"drawing": ids::encode(s.drawing("test").unwrap().id)});
    let added = add(&mut s, view, json!({"length": {"edge": [40, 0, 30]}}));
    assert!(added.get("layout").is_some(), "{added}");
    assert_eq!(run(&mut s, INSPECT, json!({"dimension": added["dimension"]})), added);
    run(&mut s, "drawing.close", json!({}));
    lose_the_edge(&mut s);
    run(&mut s, "drawing.open", drawing);
    let lost = run(&mut s, INSPECT, json!({"dimension": added["dimension"]}));
    assert!(lost["status"] == "lost" && lost.get("layout").is_none(), "{lost}");
    let front = json!({"edge": {"body": "Box", "index": edge_index(&s, [0.0, 0.0, 25.0], [80.0, 0.0, 25.0])}});
    let repaired = run(&mut s, REPAIR, json!({"dimension": added["dimension"], "slot": "edge", "pick": front}));
    reads(&repaired, 80.0, "80.00");
    let arrows: Vec<Vec2> = layout_arrows(&repaired).into_iter().flat_map(|(tip, dir)| [tip, dir]).collect();
    let want = [Vec2::new(160.0, 170.0), Vec2::new(-1.0, 0.0), Vec2::new(240.0, 170.0), Vec2::new(1.0, 0.0)];
    assert!(same_points(&arrows, &want, 1e-9), "{repaired}");
    assert_eq!(run(&mut s, INSPECT, json!({"dimension": added["dimension"]}))["layout"], repaired["layout"]);
}

/// A layout follows the drawing's units in its text alone: the top front edge's length in an inch
/// drawing reads "3.1496" (80 / 25.4; an inch length shows at least three decimals, four when the
/// fourth counts, whatever the record's decimals: `display_units::length_precision`), with the lines
/// and arrows it has in millimetres, and each box as wide as its own text measures over the same
/// middle (x 200).
#[test]
fn a_layout_follows_the_drawings_units() {
    let (mut s, view) = front_view();
    let mm = add(&mut s, view, json!({"length": {"edge": [40, 0, 30]}}));
    run(&mut s, "drawing.set", json!({"units": "in"}));
    let inches = run(&mut s, INSPECT, json!({"dimension": mm["dimension"]}));
    assert_eq!(inches["text"], json!("3.1496"), "{inches}");
    for key in ["extension_lines", "dimension_lines"] {
        assert_eq!(layout_lines(&inches, key), layout_lines(&mm, key), "{key}");
    }
    assert_eq!(layout_arrows(&inches), layout_arrows(&mm));
    // The box's first two corners, on the line's side: its width, and its middle.
    let across = |reply: &Value| {
        let corners = &layout_boxes(reply)[0];
        (corners[1].x - corners[0].x, (corners[0].x + corners[1].x) * 0.5)
    };
    for (reply, text) in [(&inches, "3.1496"), (&mm, "80.00")] {
        let (width, middle) = across(reply);
        assert!((width - measured(text)).abs() < 1e-9 && (middle - 200.0).abs() < 1e-9, "{text}: {reply}");
    }
}

/// In a deferred drawing the layout reads the model as it is now, as `value` and `text` do, while the
/// sheet shows what its last update drew. The plate's length changed from 100 to 120 with the drawing
/// closed: the length's arrows are 60 apart in the layout (120 at 1:2), at x 150 - 30 = 120 and
/// 150 + 30 = 180 round the view's centre, while the sheet still holds its update's 50 apart. Updated
/// and drawn, the sheet's arrowheads are where the layout's tips are.
#[test]
fn a_layout_reads_the_model_as_it_is_now_in_a_deferred_drawing() {
    let (mut s, drawing, _, five) = plate_with_five();
    let length = &five[3];
    run(&mut s, "drawing.set", json!({"updates": "deferred"}));
    run(&mut s, "drawing.update", json!({}));
    assert!((tips_apart(&shown_now(&mut s), length) - 50.0).abs() < 1e-6);
    closed_edit(&mut s, &drawing, "parameters.change", json!({"name": "length", "expression": "120 mm"}));
    let read = run(&mut s, INSPECT, json!({"dimension": length["dimension"]}));
    reads(&read, 120.0, "120.00");
    let tips: Vec<Vec2> = layout_arrows(&read).into_iter().map(|(tip, _)| tip).collect();
    let mut xs: Vec<f64> = tips.iter().map(|tip| tip.x).collect();
    xs.sort_by(f64::total_cmp);
    assert!(xs.len() == 2 && (xs[0] - 120.0).abs() < 1e-6 && (xs[1] - 180.0).abs() < 1e-6, "{read}");
    assert!((tips_apart(&shown_now(&mut s), length) - 50.0).abs() < 1e-6, "the sheet holds its update's lines");
    run(&mut s, "drawing.update", json!({}));
    let (_, drawn, _) = drawn_parts(&shown_now(&mut s), length);
    assert!(same_points(&tips, &drawn, 1e-9), "{tips:?} {drawn:?}");
}

/// That `reply` reads ok and `value` to within what the joints' solve places parts to.
fn reads_about(reply: &Value, value: f64) {
    assert_eq!(reply["status"].as_str(), Some("ok"), "{reply}");
    assert!((reply["value"].as_f64().unwrap() - value).abs() < 1e-6, "{reply}");
}

/// [`crate::drawing_scene::tests::hinge_pair`] with a second top view at 1:1 on (150, 250), in a
/// position of its own: Hinge at 90°, the leaf turned up. The session, the view of the design's
/// placement, the posed one and the leaf's occurrence.
fn hinge_posed() -> (Session, u64, u64, u64) {
    let (mut s, designed, _) = crate::drawing_scene::tests::hinge_pair();
    let sheet = ids::encode(s.drawing("test").unwrap().sheets[0].id);
    let pose = json!([{"joint": "Hinge", "values": [90]}]);
    let p = json!({"sheet": sheet, "view": "top", "scale": "1:1", "position_mm": [150, 250], "pose": pose});
    let posed = ids::decode(run(&mut s, "drawing.view.base", p)["view"].as_str().unwrap()).unwrap();
    let leaf = s.doc.occurrences.iter().find(|o| o.name.starts_with("Leaf")).map(|o| o.id).unwrap();
    (s, designed, posed, leaf)
}

/// From the plate's far top corner (0, 0, 2) to the leaf's free top corner, (0, 40, 2) as designed,
/// along y: vertical in a top view.
fn plate_to_free_edge(leaf: u64) -> Value {
    json!({"linear": {"a": [0, 0, 2], "b": {"point": [0, 40, 2], "path": [leaf]}, "direction": "vertical"}})
}

/// A view in a position of its own measures its parts where it draws them (#13048). With the leaf
/// flat, its free corner is 40 from the plate's far edge; turned up 90° about the hinge line (y 20,
/// z 0), (0, 40, 2) goes to (0, 20 - 2, 0 + 20), 18 from it. The leaf's own free edge is 40 long in both.
#[test]
fn a_posed_view_measures_its_parts_where_it_draws_them() {
    let (mut s, designed, posed, leaf) = hinge_posed();
    reads_about(&add(&mut s, designed, plate_to_free_edge(leaf)), 40.0);
    reads_about(&add(&mut s, posed, plate_to_free_edge(leaf)), 18.0);
    let free_edge = json!({"length": {"edge": {"point": [20, 40, 2], "path": [leaf]}}});
    reads_about(&add(&mut s, designed, free_edge.clone()), 40.0);
    reads_about(&add(&mut s, posed, free_edge), 40.0);
}

/// A posed view's dimension reads its own position, not the design's: Hinge driven to 45° in the design
/// moves the designed view's reading to 20 + 20 cos 45° - 2 sin 45° = 20 + 18 / √2 = 32.7279, and leaves
/// the posed view's at 18.
#[test]
fn driving_the_design_moves_a_designed_views_dimension_and_not_a_posed_ones() {
    let (mut s, designed, posed, leaf) = hinge_posed();
    let on_designed = add(&mut s, designed, plate_to_free_edge(leaf))["dimension"].clone();
    let on_posed = add(&mut s, posed, plate_to_free_edge(leaf))["dimension"].clone();
    let drawing = json!({"drawing": ids::encode(s.open_drawing.unwrap())});
    run(&mut s, "drawing.close", json!({}));
    run(&mut s, "joint.drive", json!({"joint": "Hinge", "values": [45]}));
    run(&mut s, "drawing.open", drawing);
    reads_about(&run(&mut s, INSPECT, json!({"dimension": on_designed})), 20.0 + 18.0 / 2f64.sqrt());
    reads_about(&run(&mut s, INSPECT, json!({"dimension": on_posed})), 18.0);
}

/// A plate 100 x 60 x 10 mm from the origin; a 10 mm cube in component Bolt at (0, 0, 20), and one in
/// component Nut at (30, 0, 20), as an exploded view needs two components; exploded view Apart, moving
/// the Bolt 50 mm up; and a drawing, open, with two front views at 1:1, one assembled at (100, 150) and
/// one drawing Apart at (300, 150). The session, the two views and the Bolt's occurrence.
fn bolt_apart() -> (Session, [u64; 2], u64) {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 100, "width": 60, "height": 10, "body_name": "Plate"}));
    let bolt = run(&mut s, "component.create", json!({"name": "Bolt"}))["occurrence"].as_u64().unwrap();
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [0, 0, 20], "body_name": "Cube"}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    run(&mut s, "component.create", json!({"name": "Nut"}));
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [30, 0, 20], "body_name": "Nut"}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    run(&mut s, "explode.create", json!({"name": "Apart", "moves": [{"occurrence": bolt, "translate": [0, 0, 50]}]}));
    run(&mut s, "drawing.create", json!({}));
    let sheet = ids::encode(s.drawing("test").unwrap().sheets[0].id);
    let view = |s: &mut Session, at: [f64; 2], exploded: &str| {
        let p = json!({"sheet": sheet, "view": "front", "scale": "1:1", "position_mm": at, "exploded": exploded});
        ids::decode(run(s, "drawing.view.base", p)["view"].as_str().unwrap()).unwrap()
    };
    let views = [view(&mut s, [100.0, 150.0], "assembled"), view(&mut s, [300.0, 150.0], "Apart")];
    (s, views, bolt)
}

/// An exploded view measures its parts where it draws them (#13048): the plate's top (z 10) to the
/// bolt's top (z 30) reads 20 assembled and 50 more, 70, with the bolt moved up; the bolt's own height
/// reads 10 in both.
#[test]
fn an_exploded_view_measures_its_parts_where_it_draws_them() {
    let (mut s, [assembled, exploded], bolt) = bolt_apart();
    let tops = json!({"linear": {"a": [0, 0, 10], "b": {"point": [0, 0, 30], "path": [bolt]}, "direction": "vertical"}});
    reads_about(&add(&mut s, assembled, tops.clone()), 20.0);
    reads_about(&add(&mut s, exploded, tops), 70.0);
    let height = json!({"length": {"edge": {"point": [0, 0, 25], "path": [bolt]}}});
    reads_about(&add(&mut s, assembled, height.clone()), 10.0);
    reads_about(&add(&mut s, exploded, height), 10.0);
}
