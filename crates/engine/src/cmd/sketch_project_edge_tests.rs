//! A projected edge stays the edge that was picked: its link keeps the edge's persistent name,
//! so the edge is found by that name wherever a change of the body takes it, and no other edge
//! is taken for it when it is split or gone: the sketch fails, asking for the edge again.

use serde_json::{Value, json};
use solidcraft_geom::{Vec2, Vec3};
use solidcraft_sketch::{Link, LinkSource, Shape};

use crate::Session;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    match s.execute(id, &p) {
        Ok(v) => v,
        Err(e) => panic!("{id} {p}: {e}"),
    }
}

/// A plate `width` (100) x 50 x 20 with a corner at the origin: body `Body1`.
fn plate(s: &mut Session) {
    run(s, "parameters.add", json!({"name": "width", "expression": "100 mm"}));
    run(s, "sketch.create", json!({"plane": "XY", "name": "Base"}));
    run(s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [100, 50]}));
    run(s, "sketch.dimension", json!({"entities": ["l1"], "value": "width"}));
    run(s, "sketch.finish", json!({}));
    run(s, "solid.extrude", json!({"sketch": "Base", "distance": "20 mm", "name": "Plate"}));
}

/// Sketch `Top` on the plate's top (z = 20) with the top edge of the plate's right-hand side,
/// from (100, 0, 20) to (100, 50, 20), projected into it: picked at `y` along it.
fn project_right_edge(s: &mut Session, y: f64) {
    run(s, "sketch.create", json!({"plane": "XY", "offset": "20 mm", "name": "Top"}));
    run(s, "sketch.project", json!({"refs": [{"edge": [100, y, 20], "body": "Body1"}]}));
    run(s, "sketch.finish", json!({}));
}

fn set_width(s: &mut Session, width: f64) {
    run(s, "parameters.change", json!({"name": "width", "expression": format!("{width} mm")}));
}

/// What sketch `Top` has to report.
fn trouble(s: &Session) -> String {
    let r = s.model.result(s.doc.find_feature("Top").unwrap().id).unwrap();
    format!("{}{}", r.error.clone().unwrap_or_default(), r.warning.clone().unwrap_or_default())
}

/// The link of sketch `Top` as the model has it.
fn link(s: &Session) -> Link {
    s.model.state().sketch(s.doc.find_feature("Top").unwrap().id).unwrap().sketch.links[0].clone()
}

/// The link of sketch `Top` as the design stores it.
fn stored(s: &Session) -> Link {
    s.doc.sketch(s.doc.find_feature("Top").unwrap().id).unwrap().links[0].clone()
}

/// The ends of the line the link of sketch `Top` gives.
fn projected(s: &Session) -> (Vec2, Vec2) {
    let st = s.model.state();
    let sk = &st.sketch(s.doc.find_feature("Top").unwrap().id).unwrap().sketch;
    match sk.shape(sk.link_curves(&sk.links[0].id)[0]) {
        Some(Shape::Line { a, b }) => (a, b),
        other => panic!("not a line: {other:?}"),
    }
}

/// The projected line runs along y at `x`, between `y0` and `y1`.
#[track_caller]
fn assert_line(s: &Session, x: f64, (y0, y1): (f64, f64), when: &str) {
    let (a, b) = projected(s);
    let ends = (a.y.min(b.y), a.y.max(b.y));
    assert!(
        (a.x - x).abs() < 1e-6 && (b.x - x).abs() < 1e-6 && (ends.0 - y0).abs() < 1e-6 && (ends.1 - y1).abs() < 1e-6,
        "{when}: expected the line x = {x}, y = {y0}..{y1}: {a:?} {b:?}"
    );
}

/// The name of the edge of `Body1` whose middle is `mid`.
fn edge_named_at(s: &mut Session, mid: [f64; 3]) -> String {
    let edges = run(s, "model.edges", json!({"body": "Body1"}));
    let at = |e: &Value| Vec3::new(e["mid"][0].as_f64().unwrap(), e["mid"][1].as_f64().unwrap(), e["mid"][2].as_f64().unwrap());
    let edge = edges["edges"].as_array().unwrap().iter().find(|e| at(e).dist(Vec3::new(mid[0], mid[1], mid[2])) < 1e-6).unwrap();
    edge["name"].as_str().unwrap().to_string()
}

/// The picked point (100, 25, 20) is 25 from the front and back edges of the top: wider than
/// 125 they are nearer to it than the picked edge is. At 60 the picked edge is 40 from it,
/// further than a reference found by its point is looked for (a quarter of the body's diagonal,
/// 20.2). The link follows the edge by its name through all of them.
#[test]
fn a_projected_edge_stays_that_edge_when_the_body_changes_size() {
    let mut s = Session::default();
    plate(&mut s);
    project_right_edge(&mut s, 25.0);
    assert_line(&s, 100.0, (0.0, 50.0), "drawn");
    let name = edge_named_at(&mut s, [100.0, 25.0, 20.0]);
    assert_eq!(stored(&s).edge_name.as_deref(), Some(name.as_str()));
    for width in [150.0, 60.0, 400.0] {
        set_width(&mut s, width);
        assert_eq!(trouble(&s), "", "width {width}");
        assert_line(&s, width, (0.0, 50.0), &format!("width {width}"));
        assert!(!link(&s).lost, "width {width}");
    }
    // Saved and opened again.
    let opened = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
    assert_eq!(stored(&opened).edge_name.as_deref(), Some(name.as_str()));
    assert_line(&opened, 400.0, (0.0, 50.0), "opened");
    // Undo and redo of the change.
    run(&mut s, "edit.undo", json!({}));
    assert_line(&s, 60.0, (0.0, 50.0), "undone");
    run(&mut s, "edit.redo", json!({}));
    assert_line(&s, 400.0, (0.0, 50.0), "redone");
    // Editing the sketch keeps the edge, and stores the link's point where the edge is now.
    run(&mut s, "sketch.edit", json!({"sketch": "Top"}));
    run(&mut s, "sketch.point", json!({"point": [10, 10]}));
    run(&mut s, "sketch.finish", json!({}));
    assert_eq!(trouble(&s), "");
    assert_line(&s, 400.0, (0.0, 50.0), "edited");
    assert_eq!(stored(&s).edge_name.as_deref(), Some(name.as_str()));
    assert!(matches!(&stored(&s).source, LinkSource::Edge { body, at } if body == "Body1" && at.dist(Vec3::new(400.0, 25.0, 20.0)) < 1e-6));
}

/// What sketch `Top` asks to be picked again, when it fails for a reference it lost.
fn reselect(s: &Session) -> Option<String> {
    let r = s.model.result(s.doc.find_feature("Top").unwrap().id).unwrap();
    r.error.as_deref().and_then(solidcraft_doc::reselect_parts).filter(|(feature, _)| *feature == "Top").map(|(_, reference)| reference.to_string())
}

/// A slot cut through the plate from x = 80 to 120, y = 20 to 30 leaves the picked edge in two
/// pieces, y = 0..20 and y = 30..50. Neither is the edge: the sketch fails, asking for it again
/// (never the piece nearest the pick); without the slot it is the whole edge again.
#[test]
fn a_projected_edge_that_was_split_fails_its_sketch_asking_for_the_edge_again() {
    let mut s = Session::default();
    plate(&mut s);
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "SlotSketch"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [80, 20], "p1": [120, 30]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"sketch": "SlotSketch", "distance": "20 mm", "operation": "cut", "name": "Slot"}));
    run(&mut s, "timeline.suppress", json!({"feature": "Slot", "suppressed": true}));
    project_right_edge(&mut s, 10.0);
    assert_line(&s, 100.0, (0.0, 50.0), "drawn");
    let name = stored(&s).edge_name.unwrap();
    run(&mut s, "timeline.suppress", json!({"feature": "Slot", "suppressed": false}));
    assert_eq!(reselect(&s), Some(name), "{}", trouble(&s));
    run(&mut s, "timeline.suppress", json!({"feature": "Slot", "suppressed": true}));
    assert_line(&s, 100.0, (0.0, 50.0), "whole again");
    assert_eq!(trouble(&s), "");
}

/// A fillet of radius 4 takes the picked edge away. Its two tangent edges are 4 from the picked
/// point, the nearest edges there are now: the sketch takes neither of them, it fails, asking for
/// its edge again. Without the fillet the link has its edge again.
#[test]
fn a_projected_edge_that_is_gone_fails_its_sketch_not_replaced() {
    let mut s = Session::default();
    plate(&mut s);
    run(&mut s, "solid.fillet", json!({"edges": [[100, 25, 20]], "radius": "4 mm", "name": "Round"}));
    run(&mut s, "timeline.suppress", json!({"feature": "Round", "suppressed": true}));
    project_right_edge(&mut s, 25.0);
    assert_line(&s, 100.0, (0.0, 50.0), "drawn");
    let name = stored(&s).edge_name.unwrap();
    run(&mut s, "timeline.suppress", json!({"feature": "Round", "suppressed": false}));
    assert_eq!(reselect(&s), Some(name), "{}", trouble(&s));
    run(&mut s, "timeline.suppress", json!({"feature": "Round", "suppressed": true}));
    assert_eq!(trouble(&s), "");
    assert!(!link(&s).lost);
    assert_line(&s, 100.0, (0.0, 50.0), "found again");
}

/// A boss 20 x 20 x 10 made as its own body on the plate and joined into it afterwards: no body
/// has the link's body name any more. The join left the boss's top edge from (30, 10, 30) to
/// (30, 30, 30) as it was, and the link finds it by its name on the body that has it now.
#[test]
fn a_projected_edge_whose_body_was_joined_into_another_is_found_there() {
    let mut s = Session::default();
    plate(&mut s);
    run(&mut s, "sketch.create", json!({"plane": "XY", "offset": "20 mm", "name": "BossSketch"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [10, 10], "p1": [30, 30]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"sketch": "BossSketch", "distance": "10 mm", "operation": "new", "name": "Boss", "body_name": "Boss"}));
    run(&mut s, "solid.combine", json!({"target": "Body1", "tools": ["Boss"], "operation": "join"}));
    run(&mut s, "timeline.suppress", json!({"feature": "Combine1", "suppressed": true}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "offset": "30 mm", "name": "Top"}));
    run(&mut s, "sketch.project", json!({"refs": [{"edge": [30, 20, 30], "body": "Boss"}]}));
    run(&mut s, "sketch.finish", json!({}));
    assert_line(&s, 30.0, (10.0, 30.0), "drawn");
    assert!(stored(&s).edge_name.is_some());
    run(&mut s, "timeline.suppress", json!({"feature": "Combine1", "suppressed": false}));
    assert!(s.model.state().body("Boss").is_none(), "the boss is part of the plate now");
    assert_eq!(trouble(&s), "");
    assert!(!link(&s).lost);
    assert_line(&s, 30.0, (10.0, 30.0), "joined");
    assert!(matches!(&link(&s).source, LinkSource::Edge { body, .. } if body == "Body1"));
}

/// A design saved before links kept the edge's name (format 16) has a link with a point only.
/// It opens and is found by that point, as it was, and takes the edge's name when its sketch is
/// next edited: undoing the edit takes the name away again, redoing it brings it back.
#[test]
fn an_edge_link_saved_without_a_name_is_found_as_before_and_named_by_the_next_edit() {
    let mut s = Session::default();
    plate(&mut s);
    project_right_edge(&mut s, 25.0);
    let name = edge_named_at(&mut s, [100.0, 25.0, 20.0]);
    let mut saved: Value = serde_json::from_str(&s.doc.to_json()).unwrap();
    let links = saved["features"].as_array_mut().unwrap().iter_mut().filter_map(|f| f.pointer_mut("/sketch/links")?.as_array_mut());
    let removed = links.flatten().filter_map(|l| l.as_object_mut()?.remove("edge_name")).count();
    assert_eq!(removed, 1, "the one projected edge");
    saved["format"] = json!("solidcraft/16");
    let mut s = Session::new(solidcraft_doc::Document::from_json(&saved.to_string()).unwrap());
    assert_eq!(s.doc.format, solidcraft_doc::format::format_string(solidcraft_doc::format::FORMAT), "upgraded on opening");
    assert_eq!(stored(&s).edge_name, None);
    assert_eq!(trouble(&s), "");
    assert_line(&s, 100.0, (0.0, 50.0), "opened");
    // Found by its point: 0.2 further out the picked edge is still the nearest one.
    set_width(&mut s, 100.2);
    assert_eq!(stored(&s).edge_name, None);
    assert_eq!(trouble(&s), "");
    assert_line(&s, 100.2, (0.0, 50.0), "width 100.2");
    run(&mut s, "sketch.edit", json!({"sketch": "Top"}));
    run(&mut s, "sketch.point", json!({"point": [10, 10]}));
    assert_eq!(stored(&s).edge_name.as_deref(), Some(name.as_str()));
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(stored(&s).edge_name, None);
    run(&mut s, "edit.redo", json!({}));
    assert_eq!(stored(&s).edge_name.as_deref(), Some(name.as_str()));
    run(&mut s, "sketch.finish", json!({}));
    set_width(&mut s, 150.0);
    assert_eq!(trouble(&s), "");
    assert_line(&s, 150.0, (0.0, 50.0), "width 150");
}

/// The ends of curve `curve` of sketch `Top` as the model solves it.
fn line_of(s: &Session, curve: &str) -> (Vec2, Vec2) {
    let st = s.model.state();
    let sk = &st.sketch(s.doc.find_feature("Top").unwrap().id).unwrap().sketch;
    match sk.shape(sk.curve_index(curve).unwrap()) {
        Some(Shape::Line { a, b }) => (a, b),
        other => panic!("not a line: {other:?}"),
    }
}

/// An edge of a mirrored body is constrained to like any other body's edge (#6348). The plate
/// mirrored about YZ gives a copy from x = -width to 0. Collinear with the copy's outer top
/// edge (x = -width) projects that edge into the sketch and holds the line on it, in one step;
/// the link keeps the copy's edge name, so the line follows the copy when the width changes,
/// and after the design is saved and opened. A vertex of the copy works the same way, and a
/// reference that misses every edge changes nothing.
#[test]
fn a_line_is_constrained_collinear_with_an_edge_of_a_mirrored_body() {
    let mut s = Session::default();
    plate(&mut s);
    run(&mut s, "solid.mirror", json!({"bodies": ["Body1"], "plane": "YZ", "name": "Twin"}));
    let copy = s.model.state().bodies.iter().find(|b| b.name != "Body1").map(|b| b.name.clone()).expect("the mirrored copy");
    run(&mut s, "sketch.create", json!({"plane": "XY", "offset": "20 mm", "name": "Top"}));
    run(&mut s, "sketch.line", json!({"points": [[-90, 5], [-90, 45]]}));
    let before = s.doc.clone();
    let out = run(&mut s, "sketch.constraint.collinear", json!({"a": "l1", "b": {"edge": [-100, 25, 20], "body": copy}}));
    assert_eq!(out["created"]["curves"].as_array().map(Vec::len), Some(1), "the projected edge: {out}");
    let stored = s.doc.sketch(s.doc.find_feature("Top").unwrap().id).unwrap();
    assert_eq!(stored.links.len(), 1);
    assert!(matches!(&stored.links[0].source, LinkSource::Edge { body, .. } if *body == copy), "{:?}", stored.links[0].source);
    let name = stored.links[0].edge_name.clone().expect("the copy's edge keeps its name");
    // Undone in one step: the projection and the constraint together.
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(s.doc.to_json(), before.to_json());
    run(&mut s, "edit.redo", json!({}));
    run(&mut s, "sketch.finish", json!({}));
    let on_edge = |s: &Session, x: f64, when: &str| {
        let (a, b) = line_of(s, "l1");
        assert!((a.x - x).abs() < 1e-6 && (b.x - x).abs() < 1e-6, "{when}: expected the line on x = {x}: {a:?} {b:?}");
    };
    on_edge(&s, -100.0, "constrained");
    for width in [150.0, 60.0] {
        set_width(&mut s, width);
        assert_eq!(trouble(&s), "", "width {width}");
        assert!(!link(&s).lost, "width {width}");
        on_edge(&s, -width, &format!("width {width}"));
    }
    let opened = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
    assert_eq!(link(&opened).edge_name.as_deref(), Some(name.as_str()));
    on_edge(&opened, -60.0, "opened");
    // A vertex of the copy: the line's end goes onto its corner (-60, 50, 20).
    run(&mut s, "sketch.edit", json!({"sketch": "Top"}));
    run(&mut s, "sketch.constraint.coincident", json!({"a": "l1.end", "b": {"vertex": [-60, 50, 20], "body": copy}}));
    let (_, end) = line_of(&s, "l1");
    assert!(end.dist(Vec2::new(-60.0, 50.0)) < 1e-6, "{end:?}");
    // Nothing there: an error, and the sketch as it was.
    let before = s.doc.to_json();
    assert!(s.execute("sketch.constraint.collinear", &json!({"a": "l1", "b": {"edge": [500, 500, 20], "body": copy}})).is_err());
    assert_eq!(s.doc.to_json(), before);
}
