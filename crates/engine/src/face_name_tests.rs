//! Faces by name: `model.faces` lists a point on each face (and a cylinder's axis and radius), and
//! the commands that take a face as a point take the name `model.faces` lists instead.

use std::f64::consts::PI;

use serde_json::{Value, json};
use solidcraft_geom::Vec3;

use crate::Session;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    match s.execute(id, &p) {
        Ok(v) => v,
        Err(e) => panic!("{id} {p}: {e}"),
    }
}

fn volume(s: &mut Session, b: &str) -> f64 {
    run(s, "inspect.measure", json!({ "bodies": [b] }))["bodies"][0]["volume_mm3"].as_f64().unwrap_or(f64::NAN)
}

fn vec(v: &Value) -> Vec3 {
    Vec3::new(v[0].as_f64().unwrap_or(f64::NAN), v[1].as_f64().unwrap_or(f64::NAN), v[2].as_f64().unwrap_or(f64::NAN))
}

/// A `l` x `w` x `h` plate with a through hole of radius 5 in the middle.
fn plate_with_hole(s: &mut Session, l: f64, w: f64, h: f64) {
    run(s, "solid.box", json!({"length": l, "width": w, "height": h, "body_name": "Plate"}));
    run(s, "solid.cylinder", json!({"base": [l / 2.0, w / 2.0, -1], "radius": 5, "height": h + 2.0, "body_name": "Drill"}));
    run(s, "solid.combine", json!({"target": "Plate", "tools": ["Drill"], "operation": "cut"}));
}

fn faces(s: &mut Session, body: &str) -> Vec<Value> {
    run(s, "model.faces", json!({ "body": body }))["faces"].as_array().cloned().unwrap_or_default()
}

/// The name of the face looking up (+z), and the names of the hole's walls (the cylindrical faces).
fn top_and_hole_names(s: &mut Session) -> (String, Vec<String>) {
    let all = faces(s, "Plate");
    let name = |f: &Value| f["name"].as_str().unwrap_or_default().to_string();
    let top = all.iter().find(|f| f["plane_normal"][2].as_f64().is_some_and(|z| z > 0.99)).map(name).unwrap_or_default();
    (top, all.iter().filter(|f| f.get("cylinder").is_some()).map(name).collect())
}

/// A 60 x 40 x 10 plate with a hole of radius 5: its top face is a rectangle with a hole in it,
/// whose centroid (the plate's middle, (30, 20, 10)) is in the hole; the hole's wall is two half
/// cylinders, whose centroids are inside the cylinder, 2r/π from its axis.
#[test]
fn model_faces_gives_a_point_on_each_face_and_the_hole_axis_and_radius() {
    let mut s = Session::default();
    plate_with_hole(&mut s, 60.0, 40.0, 10.0);
    let all = faces(&mut s, "Plate");
    let st = s.model.state();
    let body = st.body("Plate").unwrap();
    for f in &all {
        let (at, index) = (vec(&f["point"]), f["index"].as_u64().unwrap() as usize);
        let (distance, nearest) = solidcraft_doc::appearance::nearest_face(body, at).unwrap();
        assert_eq!(nearest, index, "a pick at the point finds the face: {f}");
        match f.get("cylinder") {
            // On the cylinder itself, not only on its facets (which lie inside it).
            Some(c) => {
                let radial = (at.x - 30.0).hypot(at.y - 20.0);
                assert!((radial - 5.0).abs() < 1e-9 && at.z > 0.0 && at.z < 10.0, "{at:?}");
                // The centroid of a half cylinder's wall is 2r/π = 3.18 from the axis, not on the wall.
                let centroid = vec(&f["centroid"]);
                let inside = (centroid.x - 30.0).hypot(centroid.y - 20.0);
                assert!((2.5..4.0).contains(&inside), "the centroid of a half cylinder is inside it: {inside}");
                // The hole's axis: the line through (30, 20) along z, its radius 5.
                let (origin, axis) = (vec(&c["origin"]), vec(&c["axis"]));
                assert!((origin.x - 30.0).abs() < 1e-9 && (origin.y - 20.0).abs() < 1e-9, "{origin:?}");
                assert!((axis.z.abs() - 1.0).abs() < 1e-9 && axis.x.abs() < 1e-9 && axis.y.abs() < 1e-9, "{axis:?}");
                assert!((c["radius"].as_f64().unwrap_or(f64::NAN) - 5.0).abs() < 1e-9, "{c}");
            }
            None => assert!(distance < 1e-9, "the point lies on the face: {distance} {f}"),
        }
    }
    // The hole's wall, whole: 2π·5·10 = 314.16 mm² (the facets of its halves add up to a little less).
    let wall: f64 = all.iter().filter(|f| f.get("cylinder").is_some()).filter_map(|f| f["area"].as_f64()).sum();
    assert!((wall - 2.0 * PI * 5.0 * 10.0).abs() < 0.02 * 2.0 * PI * 5.0 * 10.0, "{wall}");
    // The top face is the rectangle less the hole; its centroid is in the hole, its point is not.
    let top = all.iter().find(|f| f["plane_normal"][2].as_f64().is_some_and(|z| z > 0.99)).unwrap();
    let (centroid, at) = (vec(&top["centroid"]), vec(&top["point"]));
    assert!((centroid.x - 30.0).hypot(centroid.y - 20.0) < 1e-6, "{centroid:?}");
    assert!(
        (at.x - 30.0).hypot(at.y - 20.0) > 5.0 && (0.0..=60.0).contains(&at.x) && (0.0..=40.0).contains(&at.y) && (at.z - 10.0).abs() < 1e-9,
        "{at:?}"
    );
}

#[test]
fn delete_face_takes_the_faces_of_the_hole_by_name() {
    let mut s = Session::default();
    plate_with_hole(&mut s, 60.0, 40.0, 10.0);
    // The hole takes π·5²·10 = 785.4 mm³ out of the 24000.
    assert!((volume(&mut s, "Plate") - (24000.0 - 25.0 * PI * 10.0)).abs() < 1.0, "{}", volume(&mut s, "Plate"));
    let (_, walls) = top_and_hole_names(&mut s);
    assert!(!walls.is_empty());
    let r = run(&mut s, "face.delete", json!({"faces": walls, "body": "Plate"}));
    assert!(r["feature"].is_u64(), "{r}");
    assert!((volume(&mut s, "Plate") - 24000.0).abs() < 24.0, "the plate closes up: {}", volume(&mut s, "Plate"));
    // The feature keeps the faces' names, as it does for faces picked by a point.
    assert_eq!(s.doc.features.last().map(|f| f.face_names.clone()), Some(walls));
}

#[test]
fn convert_to_sheet_metal_takes_the_plates_face_by_name() {
    // A 100 x 60 x 2.5 plate with a hole in the middle: the centroid of its large face is in the
    // hole, where no point of the face is.
    let mut s = Session::default();
    plate_with_hole(&mut s, 100.0, 60.0, 2.5);
    let (top, _) = top_and_hole_names(&mut s);
    run(&mut s, "sheet.convert", json!({"body": "Plate", "face": top}));
    let flat = run(&mut s, "sheet.flat_pattern", json!({}));
    for (i, want) in [100.0, 60.0, 2.5].into_iter().enumerate() {
        assert!((flat["flat_size_mm"][i].as_f64().unwrap_or(f64::NAN) - want).abs() < 1e-6, "{flat}");
    }
    assert_eq!(flat["cutouts"].as_array().map(Vec::len), Some(1), "{flat}");
    // The hole is 10 across (its outline is a polygon on the circle, a little less across).
    let hole = flat["cutouts"][0].as_array().cloned().unwrap_or_default();
    for k in 0..2 {
        let at: Vec<f64> = hole.iter().filter_map(|p| p[k].as_f64()).collect();
        let span = at.iter().cloned().fold(f64::MIN, f64::max) - at.iter().cloned().fold(f64::MAX, f64::min);
        assert!(span > 9.8 && span < 10.0 + 1e-6, "{span}");
    }
    // The plate is still 100 x 60 x 2.5 less the hole: 15000 - π·5²·2.5 = 14803.65 mm³ for a round hole, a little more
    // (less than 2% of the hole's 196.35 mm³) for a polygon inside the circle.
    let (v, round) = (volume(&mut s, "Plate"), 15000.0 - 25.0 * PI * 2.5);
    assert!(v >= round - 1e-6 && v - round < 0.02 * 25.0 * PI * 2.5, "{v}");
}

/// Shell, draft and offset face share the parser of Delete Face.
#[test]
fn shell_takes_the_face_to_open_by_name() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 100, "width": 60, "height": 30, "body_name": "Tray"}));
    let all = faces(&mut s, "Tray");
    let top = all.iter().find(|f| f["plane_normal"][2].as_f64().is_some_and(|z| z > 0.99)).unwrap();
    run(&mut s, "solid.shell", json!({"faces": [top["name"]], "thickness": 2}));
    // 100 x 60 x 30 less the cavity 96 x 56 x 28 (open at the top, floor and walls 2 thick).
    assert!((volume(&mut s, "Tray") - (100.0 * 60.0 * 30.0 - 96.0 * 56.0 * 28.0)).abs() < 1e-6, "{}", volume(&mut s, "Tray"));
}

#[test]
fn split_face_takes_the_face_to_split_by_name() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 100, "width": 60, "height": 30, "body_name": "Tray"}));
    let all = faces(&mut s, "Tray");
    let top = all.iter().find(|f| f["plane_normal"][2].as_f64().is_some_and(|z| z > 0.99)).unwrap();
    run(&mut s, "solid.split_face", json!({"faces": [top["name"]], "body": "Tray", "plane": {"origin": [0, 20, 0], "normal": [0, 1, 0]}}));
    // The top is cut at y = 20 into 100 x 20 and 100 x 40.
    let mut areas: Vec<f64> =
        faces(&mut s, "Tray").iter().filter(|f| f["plane_normal"][2].as_f64().is_some_and(|z| z > 0.99)).filter_map(|f| f["area"].as_f64()).collect();
    areas.sort_by(f64::total_cmp);
    assert!(areas.len() == 2 && (areas[0] - 2000.0).abs() < 1e-6 && (areas[1] - 4000.0).abs() < 1e-6, "{areas:?}");
}

#[test]
fn a_name_that_finds_no_face_is_refused_and_the_design_is_unchanged() {
    let mut s = Session::default();
    plate_with_hole(&mut s, 60.0, 40.0, 10.0);
    let (_, walls) = top_and_hole_names(&mut s);
    let n = s.doc.features.len();
    let mut refused = |p: Value| s.execute("face.delete", &p).unwrap_err().to_string();
    assert!(refused(json!({"faces": ["F99:top"]})).contains("no face is named `F99:top`"));
    assert!(refused(json!({"faces": [walls[0].clone()], "body": "Nope"})).contains("no body `Nope`"));
    assert!(refused(json!({"faces": [""]})).contains("1…512"));
    assert!(refused(json!({"faces": ["x".repeat(513)]})).contains("1…512"));
    assert!(refused(json!({"faces": [true]})).contains("or its name"));
    assert_eq!(s.doc.features.len(), n);
}

/// A 30 x 20 x 10 block with a hole of radius 3 in a component, moved 50 mm along x: `model.faces`
/// speaks the world, and a name finds its face where the component is shown.
#[test]
fn names_find_the_faces_of_a_moved_component() {
    let mut s = Session::default();
    run(&mut s, "component.create", json!({"name": "Part", "grounded": false}));
    run(&mut s, "solid.box", json!({"length": 30, "width": 20, "height": 10, "body_name": "Block"}));
    run(&mut s, "solid.cylinder", json!({"base": [15, 10, -1], "radius": 3, "height": 12, "operation": "cut"}));
    run(&mut s, "occurrence.move", json!({"occurrence": "Part:1", "translate": [50, 0, 0]}));
    let all = faces(&mut s, "Block");
    let top = all.iter().find(|f| f["plane_normal"][2].as_f64().is_some_and(|z| z > 0.99)).unwrap();
    assert!(vec(&top["point"]).x > 50.0, "{top}");
    let walls: Vec<Value> = all.iter().filter(|f| f.get("cylinder").is_some()).map(|f| f["name"].clone()).collect();
    assert!(!walls.is_empty());
    let v = volume(&mut s, "Block");
    assert!((v - (6000.0 - 9.0 * PI * 10.0)).abs() < 1.0, "{v}");
    // Delete Face takes world points, Shell the points in the component's frame: either way the name finds the face.
    run(&mut s, "face.delete", json!({"faces": walls, "body": "Block"}));
    assert!((volume(&mut s, "Block") - 6000.0).abs() < 6.0, "{}", volume(&mut s, "Block"));
    run(&mut s, "solid.shell", json!({"faces": [top["name"]], "thickness": 2, "body": "Block"}));
    // 30 x 20 x 10 less the cavity 26 x 16 x 8.
    assert!((volume(&mut s, "Block") - (6000.0 - 26.0 * 16.0 * 8.0)).abs() < 1e-6, "{}", volume(&mut s, "Block"));
}

fn top_name(all: &[Value]) -> Value {
    all.iter().find(|f| f["plane_normal"][2].as_f64().is_some_and(|z| z > 0.99)).map(|f| f["name"].clone()).unwrap_or(Value::Null)
}

#[test]
fn offset_face_takes_the_face_by_name() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 100, "width": 60, "height": 30, "body_name": "Tray"}));
    let top = top_name(&faces(&mut s, "Tray"));
    run(&mut s, "solid.offset_face", json!({"faces": [top], "distance": 5, "body": "Tray"}));
    // The top moves out 5 mm: 100 x 60 x 35.
    assert!((volume(&mut s, "Tray") - 100.0 * 60.0 * 35.0).abs() < 1e-6, "{}", volume(&mut s, "Tray"));
}

/// A cylinder of radius 10 and height 20 has a side of two half cylinders, each of which
/// `model.faces` describes (its axis and radius); drafting them 5 degrees from the base plane makes
/// a frustum of radius 10 at the base and 10 - 20 tan 5 degrees at the top.
#[test]
fn draft_takes_a_cylinders_side_by_name() {
    let mut s = Session::default();
    run(&mut s, "solid.cylinder", json!({"radius": 10, "height": 20, "body_name": "Pin"}));
    let all = faces(&mut s, "Pin");
    let sides: Vec<&Value> = all.iter().filter(|f| f.get("cylinder").is_some()).collect();
    assert_eq!(sides.len(), 2, "two half cylinders: {all:?}");
    for side in &sides {
        assert!((side["cylinder"]["radius"].as_f64().unwrap_or(f64::NAN) - 10.0).abs() < 1e-9, "{side}");
    }
    let names: Vec<Value> = sides.iter().map(|f| f["name"].clone()).collect();
    run(&mut s, "solid.draft", json!({"faces": names, "angle": "5 deg", "neutral": "XY", "body": "Pin"}));
    let small = 10.0 - 20.0 * 5f64.to_radians().tan();
    let want = PI * 20.0 / 3.0 * (100.0 + 10.0 * small + small * small);
    assert!((volume(&mut s, "Pin") - want).abs() < 1e-3 * want, "{} vs {want}", volume(&mut s, "Pin"));
}

/// Replace Face takes the faces to replace by name; for a target on another body, the `point`
/// `model.faces` gives picks that body's face.
#[test]
fn replace_face_takes_the_face_by_name() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "body_name": "A"}));
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 15, "corner": [20, 0, 0], "body_name": "B"}));
    let (top_a, top_b) = (top_name(&faces(&mut s, "A")), top_name(&faces(&mut s, "B")));
    let on_b = faces(&mut s, "B").into_iter().find(|f| f["name"] == top_b).map(|f| f["point"].clone());
    run(&mut s, "solid.replace_face", json!({"faces": [top_a], "target_face": on_b, "body": "A"}));
    // A's top moves up to B's, 15 high: 10 x 10 x 15.
    assert!((volume(&mut s, "A") - 1500.0).abs() < 1e-6, "{}", volume(&mut s, "A"));
}
