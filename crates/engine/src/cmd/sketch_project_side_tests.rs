//! A dimension from linked geometry keeps its side when the link's source moves: a point held
//! 5 from a projected line stays on the side it was drawn on wherever a parameter takes the
//! line, also past the place the point is stored at.

use serde_json::{Value, json};
use solidcraft_geom::Vec2;

use crate::Session;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    match s.execute(id, &p) {
        Ok(v) => v,
        Err(e) => panic!("{id} {p}: {e}"),
    }
}

/// Sketch `A`: the horizontal line `rail`, 100 long, at the height the parameter `height`
/// (50) gives it.
fn rail_sketch(s: &mut Session) {
    run(s, "parameters.add", json!({"name": "height", "expression": "50 mm"}));
    run(s, "sketch.create", json!({"plane": "XY", "name": "A"}));
    run(s, "sketch.line", json!({"points": [[0, 50], [100, 50]], "ids": ["rail"]}));
    run(s, "sketch.constraint.horizontal_vertical", json!({"line": "rail", "mode": "horizontal"}));
    run(s, "sketch.constraint.horizontal_vertical", json!({"points": ["origin", "rail.start"], "mode": "vertical"}));
    run(s, "sketch.dimension", json!({"entities": ["origin", "rail.start"], "value": "height"}));
    run(s, "sketch.dimension", json!({"entities": ["rail"], "value": "100 mm"}));
    run(s, "sketch.finish", json!({}));
}

/// Sketch `S` (left open): `rail` brought in by `command`, and the point `p` at (50, `y`),
/// 50 from the origin horizontally. Returns the linked line.
fn point_by_the_rail(s: &mut Session, command: &str, y: f64) -> String {
    rail_sketch(s);
    run(s, "sketch.create", json!({"plane": "XY", "name": "S"}));
    let r = run(s, command, json!({"refs": [{"sketch": "A", "curve": "rail"}]}));
    let rail = r["links"][0]["curves"][0].as_str().unwrap().to_string();
    run(s, "sketch.point", json!({"point": [50, y], "id": "p"}));
    run(s, "sketch.dimension", json!({"entities": ["origin", "p"], "type": "horizontal", "value": "50 mm"}));
    rail
}

/// `p` held 5 from `line`: the dimension's parameter.
fn five_from(s: &mut Session, line: &str) -> String {
    let r = run(s, "sketch.dimension", json!({"entities": ["p", line], "type": "distance", "value": "5 mm"}));
    r["param"].as_str().unwrap().to_string()
}

fn set_height(s: &mut Session, height: f64) {
    run(s, "parameters.change", json!({"name": "height", "expression": format!("{height} mm")}));
}

/// Where the model has `p` of sketch `S`.
fn p(s: &Session) -> Vec2 {
    let st = s.model.state();
    let sk = &st.sketch(s.doc.find_feature("S").unwrap().id).unwrap().sketch;
    sk.point(sk.resolve_point("p").unwrap()).unwrap()
}

/// `p` is at (50, `y`), and the sketch has nothing to report.
#[track_caller]
fn assert_p(s: &Session, y: f64, when: &str) {
    assert!(p(s).dist(Vec2::new(50.0, y)) < 1e-9, "{when}: {:?}", p(s));
    let feature = s.model.result(s.doc.find_feature("S").unwrap().id).unwrap();
    assert_eq!((&feature.error, &feature.warning), (&None, &None), "{when}");
}

/// A line of sketch `A` whose height a parameter drives, projected into sketch `S`, whose point
/// is dimensioned 5 from the projection.
#[test]
fn a_distance_from_a_projected_line_keeps_its_side_when_the_source_moves_past() {
    let mut s = Session::default();
    let rail = point_by_the_rail(&mut s, "sketch.project", 45.0);
    five_from(&mut s, &rail);
    run(&mut s, "sketch.finish", json!({}));
    assert_p(&s, 45.0, "drawn");
    // The source drops below the point's stored place: the point stays below the projection.
    for height in [20.0, 50.0, 6.0, 400.0] {
        set_height(&mut s, height);
        assert_p(&s, height - 5.0, &format!("height {height}"));
    }
    // Editing the sketch afterwards refreshes the projection first; the side is the one from before.
    set_height(&mut s, 20.0);
    run(&mut s, "sketch.edit", json!({"sketch": "S"}));
    run(&mut s, "sketch.point", json!({"point": [80, 80]}));
    run(&mut s, "sketch.finish", json!({}));
    assert_p(&s, 15.0, "after an edit");
}

/// A label's sketch: three points brought in from sketch `A` (a 20 long vertical line whose start
/// the parameters `x` and `y` place, and its middle), a construction rectangle 5 wide on them
/// (its near side on the line's ends, its far side 5 from the middle) and a centred text on the
/// far side whose words the text parameter `Label` gives. Other words are other outlines: the text
/// is built anew in the evaluated sketch, so its entities are not the stored sketch's. Moved 280
/// away afterwards (more than one jump can take: the solve walks there from the stored sketch), the
/// rectangle follows and keeps its side.
#[test]
fn a_label_follows_its_source_far_after_a_text_parameter_gave_it_other_words() {
    let mut s = Session::default();
    run(&mut s, "parameters.add", json!({"name": "x", "expression": "100 mm"}));
    run(&mut s, "parameters.add", json!({"name": "y", "expression": "200 mm"}));
    run(&mut s, "parameters.add", json!({"name": "Label", "expression": "A1", "unit": "text"}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "A"}));
    run(&mut s, "sketch.line", json!({"points": [[100, 200], [100, 180]], "ids": ["rail"]}));
    run(&mut s, "sketch.constraint.horizontal_vertical", json!({"line": "rail", "mode": "vertical"}));
    run(&mut s, "sketch.dimension", json!({"entities": ["origin", "rail.start"], "type": "horizontal", "value": "x"}));
    run(&mut s, "sketch.dimension", json!({"entities": ["origin", "rail.start"], "type": "vertical", "value": "y"}));
    run(&mut s, "sketch.dimension", json!({"entities": ["rail"], "value": "20 mm"}));
    run(&mut s, "sketch.point", json!({"point": [100, 190], "id": "mid"}));
    run(&mut s, "sketch.constraint.midpoint", json!({"point": "mid", "line": "rail"}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "S"}));
    let refs = ["rail.start", "rail.end", "mid"].map(|p| json!({"sketch": "A", "point": p}));
    let r = run(&mut s, "sketch.project", json!({"refs": refs}));
    let brought: Vec<String> = (0..3).map(|i| r["links"][i]["points"][0].as_str().unwrap().to_string()).collect();
    let near = 100.0 - 5.0;
    for (id, a, b) in [
        ("l1", [100.0, 200.0], [100.0, 180.0]),
        ("l2", [near, 200.0], [near, 180.0]),
        ("l3", [100.0, 200.0], [near, 200.0]),
        ("l4", [100.0, 180.0], [near, 180.0]),
    ] {
        run(&mut s, "sketch.line", json!({"points": [a, b], "ids": [id], "construction": true, "connect": false}));
    }
    for (a, b) in [
        ("l1.start", brought[0].as_str()),
        ("l1.end", &brought[1]),
        ("l3.start", "l1.start"),
        ("l3.end", "l2.start"),
        ("l4.start", "l1.end"),
        ("l4.end", "l2.end"),
    ] {
        run(&mut s, "sketch.constraint.coincident", json!({"a": a, "b": b}));
    }
    run(&mut s, "sketch.constraint.parallel", json!({"a": "l2", "b": "l1"}));
    run(&mut s, "sketch.dimension", json!({"entities": [brought[2], "l2"], "type": "distance", "value": "5 mm"}));
    run(&mut s, "sketch.constraint.perpendicular", json!({"a": "l3", "b": "l1"}));
    run(&mut s, "sketch.constraint.perpendicular", json!({"a": "l4", "b": "l1"}));
    run(&mut s, "sketch.text", json!({"text_expression": "Label + '-1'", "at": [0, 0], "height": 3, "align": "center", "path": "l2"}));
    run(&mut s, "sketch.finish", json!({}));
    let far_side = |s: &Session| {
        let st = s.model.state();
        let sk = &st.sketch(s.doc.find_feature("S").unwrap().id).unwrap().sketch;
        sk.point(sk.resolve_point("l2.start").unwrap()).unwrap()
    };
    #[track_caller]
    fn quiet(s: &Session, when: &str) {
        let feature = s.model.result(s.doc.find_feature("S").unwrap().id).unwrap();
        assert_eq!((&feature.error, &feature.warning), (&None, &None), "{when}");
    }
    quiet(&s, "drawn");
    run(&mut s, "parameters.change", json!({"name": "Label", "expression": "B2"}));
    quiet(&s, "relabelled");
    for (x, y) in [(300.0, 400.0), (100.0, 200.0)] {
        run(&mut s, "parameters.change", json!({"changes": {"x": format!("{x} mm"), "y": format!("{y} mm")}}));
        quiet(&s, &format!("moved to ({x}, {y})"));
        assert!(far_side(&s).dist(Vec2::new(x - 5.0, y)) < 1e-6, "({x}, {y}): {:?}", far_side(&s));
    }
}

/// The same from the other side: the point above the line, the source rising past it and
/// coming back; and with the line brought in by Include 3D Geometry.
#[test]
fn a_point_above_the_line_stays_above_when_the_source_rises_past_and_returns() {
    for command in ["sketch.project", "sketch.include_3d"] {
        let mut s = Session::default();
        let rail = point_by_the_rail(&mut s, command, 55.0);
        five_from(&mut s, &rail);
        run(&mut s, "sketch.finish", json!({}));
        for height in [80.0, 400.0, 50.0, 6.0, 50.0] {
            set_height(&mut s, height);
            assert_p(&s, height + 5.0, &format!("{command}, height {height}"));
        }
    }
}

/// Model geometry in a sketch on XZ (whose y runs down the model's Z): the top of a box whose
/// height a parameter drives, as a projected edge, as an edge of a projected face and as a
/// section of the body. The point is 5 below the top, inside the box.
#[test]
fn a_distance_from_model_geometry_keeps_its_side_when_the_model_moves_past() {
    // A projected edge is found again by the point picked on it, so it follows small steps
    // only; a face (by its name) and a body follow any.
    let small: &[f64] = &[42.0, 50.0, 58.0, 50.0, 44.0];
    let large: &[f64] = &[20.0, 50.0, 6.0, 400.0];
    let cases = [
        ("sketch.project", json!({"edge": [50, 0, 50]}), 0.0, small),
        ("sketch.project", json!({"face": [50, 0, 25]}), 0.0, large),
        ("sketch.intersect", json!({"body": "Body1"}), -20.0, large),
    ];
    for (command, link, front, heights) in cases {
        let mut s = Session::default();
        run(&mut s, "parameters.add", json!({"name": "height", "expression": "50 mm"}));
        run(&mut s, "solid.box", json!({"corner": [0, front, 0], "length": 100, "width": 40, "height": "height"}));
        run(&mut s, "sketch.create", json!({"plane": "XZ", "name": "S"}));
        run(&mut s, command, json!({"refs": [link]}));
        let curves = run(&mut s, "sketch.inspect", json!({}))["curves"].clone();
        let high = |c: &Value, end: &str| (c[end][1].as_f64().unwrap() + 50.0).abs() < 1e-6;
        let top = curves.as_array().unwrap().iter().find(|c| high(c, "start_at") && high(c, "end_at")).unwrap()["id"].as_str().unwrap().to_string();
        run(&mut s, "sketch.point", json!({"point": [50, -45], "id": "p"}));
        run(&mut s, "sketch.dimension", json!({"entities": ["origin", "p"], "type": "horizontal", "value": "50 mm"}));
        five_from(&mut s, &top);
        run(&mut s, "sketch.finish", json!({}));
        // (An edge found again by its picked point says so in a warning.)
        for height in heights {
            set_height(&mut s, *height);
            assert!(p(&s).dist(Vec2::new(50.0, 5.0 - height)) < 1e-9, "{command} {link}, height {height}: {:?}", p(&s));
        }
    }
}

/// Another projection added after the source moved stores the sketch without solving it: the
/// sketch it stores is the one the model shows.
#[test]
fn a_projection_added_after_the_source_moved_leaves_the_side() {
    let mut s = Session::default();
    let rail = point_by_the_rail(&mut s, "sketch.project", 45.0);
    five_from(&mut s, &rail);
    run(&mut s, "sketch.finish", json!({}));
    set_height(&mut s, 20.0);
    run(&mut s, "sketch.edit", json!({"sketch": "S"}));
    run(&mut s, "sketch.project", json!({"refs": ["origin"]}));
    run(&mut s, "sketch.finish", json!({}));
    assert_p(&s, 15.0, "after the projection");
    set_height(&mut s, 50.0);
    assert_p(&s, 45.0, "back at 50");
}

/// The side is still the user's to choose, by where the point is when the dimension is added:
/// also when the source has moved past the point since it was drawn. That side is kept from
/// then on, and through an edit of the dimension's value.
#[test]
fn a_dimension_added_after_the_source_moved_takes_the_side_its_point_is_on() {
    let mut s = Session::default();
    let rail = point_by_the_rail(&mut s, "sketch.project", 45.0);
    run(&mut s, "sketch.finish", json!({}));
    // Drawn below the line; the line drops below it.
    set_height(&mut s, 20.0);
    assert_p(&s, 45.0, "no distance yet");
    run(&mut s, "sketch.edit", json!({"sketch": "S"}));
    let distance = five_from(&mut s, &rail);
    run(&mut s, "sketch.finish", json!({}));
    assert_p(&s, 25.0, "dimensioned above the line");
    for height in [50.0, 6.0, 400.0, 20.0] {
        set_height(&mut s, height);
        assert_p(&s, height + 5.0, &format!("height {height}"));
    }
    run(&mut s, "parameters.change", json!({"name": distance, "expression": "8 mm"}));
    assert_p(&s, 28.0, "8 from the line");
    set_height(&mut s, 6.0);
    assert_p(&s, 14.0, "8 from the line, height 6");
}

/// A rectangle of construction lines placed on three points projected from another sketch
/// (its near side on the line's ends, its far side 4 from the middle) and a centred text on
/// the far side. Moved about 280 away (more than one jump can take: the solve walks there from the
/// stored sketch), the rectangle follows and keeps its side ("the sketch constraints conflict"
/// before), and comes back.
#[test]
fn a_label_on_a_path_follows_its_source_far() {
    let mut s = Session::default();
    run(&mut s, "parameters.add", json!({"name": "x", "expression": "100 mm"}));
    run(&mut s, "parameters.add", json!({"name": "y", "expression": "120 mm"}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "A"}));
    run(&mut s, "sketch.line", json!({"points": [[100, 120], [100, 100]], "ids": ["rail"]}));
    run(&mut s, "sketch.constraint.horizontal_vertical", json!({"line": "rail", "mode": "vertical"}));
    run(&mut s, "sketch.dimension", json!({"entities": ["origin", "rail.start"], "type": "horizontal", "value": "x"}));
    run(&mut s, "sketch.dimension", json!({"entities": ["origin", "rail.start"], "type": "vertical", "value": "y"}));
    run(&mut s, "sketch.dimension", json!({"entities": ["rail"], "value": "20 mm"}));
    run(&mut s, "sketch.point", json!({"point": [100, 110], "id": "mid"}));
    run(&mut s, "sketch.constraint.midpoint", json!({"point": "mid", "line": "rail"}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "S"}));
    let refs = ["rail.start", "rail.end", "mid"].map(|p| json!({"sketch": "A", "point": p}));
    let r = run(&mut s, "sketch.project", json!({"refs": refs}));
    let brought: Vec<String> = (0..3).map(|i| r["links"][i]["points"][0].as_str().unwrap().to_string()).collect();
    let near = 100.0 - 4.0;
    for (id, a, b) in [
        ("l1", [100.0, 120.0], [100.0, 100.0]),
        ("l2", [near, 120.0], [near, 100.0]),
        ("l3", [100.0, 120.0], [near, 120.0]),
        ("l4", [100.0, 100.0], [near, 100.0]),
    ] {
        run(&mut s, "sketch.line", json!({"points": [a, b], "ids": [id], "construction": true, "connect": false}));
    }
    for (a, b) in [
        ("l1.start", brought[0].as_str()),
        ("l1.end", &brought[1]),
        ("l3.start", "l1.start"),
        ("l3.end", "l2.start"),
        ("l4.start", "l1.end"),
        ("l4.end", "l2.end"),
    ] {
        run(&mut s, "sketch.constraint.coincident", json!({"a": a, "b": b}));
    }
    run(&mut s, "sketch.constraint.parallel", json!({"a": "l2", "b": "l1"}));
    run(&mut s, "sketch.dimension", json!({"entities": [brought[2], "l2"], "type": "distance", "value": "4 mm"}));
    run(&mut s, "sketch.constraint.perpendicular", json!({"a": "l3", "b": "l1"}));
    run(&mut s, "sketch.constraint.perpendicular", json!({"a": "l4", "b": "l1"}));
    run(&mut s, "sketch.text", json!({"text": "TAG-1", "at": [0, 0], "height": 3, "align": "center", "path": "l2"}));
    run(&mut s, "sketch.finish", json!({}));
    let far_side = |s: &Session| {
        let st = s.model.state();
        let sk = &st.sketch(s.doc.find_feature("S").unwrap().id).unwrap().sketch;
        sk.point(sk.resolve_point("l2.start").unwrap()).unwrap()
    };
    #[track_caller]
    fn quiet(s: &Session, when: &str) {
        let feature = s.model.result(s.doc.find_feature("S").unwrap().id).unwrap();
        assert_eq!((&feature.error, &feature.warning), (&None, &None), "{when}");
    }
    quiet(&s, "drawn");
    for (x, y) in [(300.0, 320.0), (100.0, 120.0)] {
        run(&mut s, "parameters.change", json!({"changes": {"x": format!("{x} mm"), "y": format!("{y} mm")}}));
        quiet(&s, &format!("moved to ({x}, {y})"));
        assert!(far_side(&s).dist(Vec2::new(x - 4.0, y)) < 1e-6, "({x}, {y}): {:?}", far_side(&s));
    }
}
