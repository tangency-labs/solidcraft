//! Sketches on construction planes built from the model (perpendicular to a plane through a
//! sketch line, through three points, a midplane…): Project, Intersect, Include 3D Geometry and
//! Text resolve the sketch's plane against the model before the sketch, as evaluation does, and
//! the links follow when a parameter moves the plane.

use serde_json::{Value, json};
use solidcore_record::Vec3;
use solidcraft_geom::prelude::*;

use crate::Session;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    match s.execute(id, &p) {
        Ok(v) => v,
        Err(e) => panic!("{id} {p}: {e}"),
    }
}

/// Sketch `A` on YZ with the line `s` from the origin to (run 100, rise `rise`), and the plane
/// `P` through that line, square to YZ. The plane's origin is the line's start and its x axis
/// runs along the line.
fn sloped_plane(s: &mut Session) {
    run(s, "parameters.add", json!({"name": "rise", "expression": "60 mm"}));
    run(s, "sketch.create", json!({"plane": "YZ", "name": "A"}));
    run(s, "sketch.line", json!({"points": [[0, 0], [100, 60]], "ids": ["s"]}));
    run(s, "sketch.dimension", json!({"entities": ["s.start", "s.end"], "type": "horizontal", "value": "100 mm"}));
    run(s, "sketch.dimension", json!({"entities": ["s.start", "s.end"], "type": "vertical", "value": "rise"}));
    run(s, "sketch.finish", json!({}));
    run(s, "construct.plane.perpendicular", json!({"plane": "YZ", "line": {"sketch": "A", "curve": "s"}, "name": "P"}));
}

/// World position of point `point` of sketch `sketch`.
fn world_point(s: &Session, sketch: &str, point: &str) -> Vec3 {
    let id = s.doc.find_feature(sketch).unwrap().id;
    let st = s.model.state();
    let ss = st.sketch(id).unwrap();
    let i = ss.sketch.resolve_point(point).unwrap_or_else(|| panic!("no point {point} in {sketch}"));
    ss.plane.to_world(ss.sketch.point(i).unwrap())
}

fn no_trouble(s: &Session) {
    for r in &s.model.results {
        assert!(r.error.is_none() && r.warning.is_none(), "{}: {:?} {:?}", r.name, r.error, r.warning);
    }
}

#[test]
fn project_onto_a_plane_built_from_a_sketch_line_follows_the_parameter() {
    let mut s = Session::default();
    sloped_plane(&mut s);
    run(&mut s, "sketch.create", json!({"plane": "P", "name": "B"}));
    let r = run(&mut s, "sketch.project", json!({"refs": [{"sketch": "A", "point": "s.end"}, {"sketch": "A", "curve": "s"}]}));
    let point = r["links"][0]["points"][0].as_str().unwrap().to_string();
    let curve = r["links"][1]["curves"][0].as_str().unwrap().to_string();
    // Geometry drawn against the projection keeps working: a circle on the projected point.
    run(&mut s, "sketch.circle.center", json!({"center": point, "radius": 4}));
    run(&mut s, "sketch.finish", json!({}));
    no_trouble(&s);
    let len = (100.0f64 * 100.0 + 60.0 * 60.0).sqrt();
    let end = world_point(&s, "A", "s.end");
    assert!(world_point(&s, "B", &point).dist(end) < 1e-9, "{:?} {end:?}", world_point(&s, "B", &point));
    let ends = [world_point(&s, "B", &format!("{curve}.start")), world_point(&s, "B", &format!("{curve}.end"))];
    assert!(ends.iter().any(|q| q.dist(Vec3::ZERO) < 1e-9) && ends.iter().any(|q| q.dist(end) < 1e-9), "{ends:?}");
    {
        let st = s.model.state();
        let b = st.sketch(s.doc.find_feature("B").unwrap().id).unwrap();
        let q = b.sketch.point(b.sketch.resolve_point(&point).unwrap()).unwrap();
        assert!((q.x - len).abs() < 1e-9 && q.y.abs() < 1e-9, "the line's end lies on the plane's x axis: {q:?}");
    }
    // The parameter tilts the plane; the projection and the circle on it follow, in the file too.
    run(&mut s, "parameters.change", json!({"name": "rise", "expression": "25 mm"}));
    let check = |s: &Session| {
        no_trouble(s);
        let end = world_point(s, "A", "s.end");
        assert!((end.z - 25.0).abs() < 1e-9 && (end.y - 100.0).abs() < 1e-9, "{end:?}");
        assert!(world_point(s, "B", &point).dist(end) < 1e-9);
        assert!(world_point(s, "B", "c1.center").dist(end) < 1e-9);
    };
    check(&s);
    check(&Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap()));
    // Editing the sketch again after the change refreshes its links on the moved plane.
    run(&mut s, "sketch.edit", json!({"sketch": "B"}));
    run(&mut s, "sketch.point", json!({"point": [10, 10]}));
    run(&mut s, "sketch.finish", json!({}));
    check(&s);
}

#[test]
fn text_on_a_plane_built_from_a_sketch_line() {
    let mut s = Session::default();
    sloped_plane(&mut s);
    run(&mut s, "sketch.create", json!({"plane": "P", "name": "B"}));
    let r = run(&mut s, "sketch.text", json!({"text": "AB", "at": [10, 10], "height": 5}));
    assert!(!r["curves"].as_array().unwrap().is_empty(), "{r}");
    run(&mut s, "sketch.finish", json!({}));
    no_trouble(&s);
    let st = s.model.state();
    let b = st.sketch(s.doc.find_feature("B").unwrap().id).unwrap();
    assert!(!b.profiles.is_empty());
}

#[test]
fn intersect_and_include_on_planes_built_from_the_model() {
    let mut s = Session::default();
    sloped_plane(&mut s);
    run(&mut s, "solid.box", json!({"corner": [-10, 20, -5], "length": 20, "width": 30, "height": 80, "body_name": "Post"}));
    // The plane through three sketch points and the midplane of two faces are built from the model too.
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "T"}));
    run(&mut s, "sketch.line", json!({"points": [[0, 0], [40, 0], [40, 30]], "ids": ["u", "v"]}));
    run(&mut s, "sketch.finish", json!({}));
    run(
        &mut s,
        "construct.plane.three_points",
        json!({"a": {"sketch": "T", "point": "u.start"}, "b": {"sketch": "T", "point": "u.end"}, "c": {"sketch": "T", "point": "v.end"}, "name": "Three"}),
    );
    run(&mut s, "construct.plane.midplane", json!({"a": {"face": [0, 20, 30]}, "b": {"face": [0, 50, 30]}, "name": "Mid"}));

    run(&mut s, "sketch.create", json!({"plane": "P", "name": "B"}));
    // The slope cuts the post: a closed section of four lines.
    let r = run(&mut s, "sketch.intersect", json!({"refs": [{"body": "Post"}]}));
    assert_eq!(r["links"][0]["curves"].as_array().unwrap().len(), 4, "{r}");
    // An edge of the post, brought onto the slope.
    let r = run(&mut s, "sketch.include_3d", json!({"refs": [{"edge": [-10, 20, 30], "body": "Post"}]}));
    assert!(r["links"][0]["link"].is_string(), "{r}");
    run(&mut s, "sketch.finish", json!({}));

    run(&mut s, "sketch.create", json!({"plane": "Three", "name": "C"}));
    let r = run(&mut s, "sketch.project", json!({"refs": [{"sketch": "T", "point": "v.end"}]}));
    let p = r["links"][0]["points"][0].as_str().unwrap().to_string();
    run(&mut s, "sketch.finish", json!({}));
    assert!(world_point(&s, "C", &p).dist(Vec3::new(40.0, 30.0, 0.0)) < 1e-9);

    run(&mut s, "sketch.create", json!({"plane": "Mid", "name": "D"}));
    let r = run(&mut s, "sketch.intersect", json!({"refs": [{"body": "Post"}]}));
    assert_eq!(r["links"][0]["curves"].as_array().unwrap().len(), 4, "{r}");
    run(&mut s, "sketch.finish", json!({}));
    no_trouble(&s);
    // The midplane sits half way between the post's two faces (y = 35).
    let st = s.model.state();
    let d = st.sketch(s.doc.find_feature("D").unwrap().id).unwrap();
    assert!(d.sketch.points.iter().any(|q| q.link.is_some()));
    for q in d.sketch.points.iter().filter(|q| q.link.is_some()) {
        assert!((d.plane.to_world(q.pos).y - 35.0).abs() < 1e-9);
    }
}
