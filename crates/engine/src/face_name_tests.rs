//! Faces by name: `model.faces` lists a point on each face (and a cylinder's axis and radius), and
//! the commands that take a face as a point take the name `model.faces` lists instead.

use std::f64::consts::PI;

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
/// whose centroid (the plate's middle, (30, 20, 10)) is in the hole; the hole's wall is one
/// whole cylinder (SMLib's, closed on its seam), whose centroid is on its axis, halfway up.
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
                // The centroid of a whole cylinder's wall is on its axis, not on the wall.
                let centroid = vec(&f["centroid"]);
                let inside = (centroid.x - 30.0).hypot(centroid.y - 20.0);
                assert!(inside < 1e-9 && (centroid.z - 5.0).abs() < 1e-9, "the centroid of the hole's wall is on its axis: {centroid:?}");
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
    // The plate is still 100 x 60 x 2.5 less the hole, which is round: 15000 - π·5²·2.5 =
    // 14803.65 mm³, within the measure's accuracy (SMLib's mass properties come within 2e-7 of
    // a closed form).
    let (v, round) = (volume(&mut s, "Plate"), 15000.0 - 25.0 * PI * 2.5);
    assert!((v - round).abs() < 2e-7 * round, "{v} vs {round}");
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

/// A cylinder of radius 10 and height 20 has one side face (SMLib's, closed on its seam), which
/// `model.faces` describes (its axis and radius); drafting it 5 degrees from the base plane makes
/// a frustum of radius 10 at the base and 10 - 20 tan 5 degrees at the top.
#[test]
fn draft_takes_a_cylinders_side_by_name() {
    let mut s = Session::default();
    run(&mut s, "solid.cylinder", json!({"radius": 10, "height": 20, "body_name": "Pin"}));
    let all = faces(&mut s, "Pin");
    let sides: Vec<&Value> = all.iter().filter(|f| f.get("cylinder").is_some()).collect();
    assert_eq!(sides.len(), 1, "one side: {all:?}");
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

/// A face name on two bodies, each with one such face (two bodies one extrude made, the pieces of
/// a split body), is not a split face: a hole placed on one of them gives no warning (#10699).
#[test]
fn a_face_name_on_two_bodies_is_no_split_face() {
    let said = |s: &Session, feature: &str| {
        let r = s.model.result(s.doc.find_feature(feature).unwrap().id).unwrap();
        format!("{}{}", r.error.clone().unwrap_or_default(), r.warning.clone().unwrap_or_default())
    };
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [10, 10]}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [14, 0], "p1": [24, 10]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"distance": 10, "operation": "new"}));
    run(&mut s, "solid.hole", json!({"position": [5, 5, 10], "diameter": 5, "depth": 5, "name": "Hole1"}));
    assert_eq!(said(&s, "Hole1"), "");
    assert!((volume(&mut s, "Body1") - (1000.0 - PI * 2.5 * 2.5 * 5.0)).abs() < 0.1);
    assert!((volume(&mut s, "Body2") - 1000.0).abs() < 1e-6);
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 40, "width": 30, "height": 10}));
    run(&mut s, "solid.split_body", json!({"body": "Body1", "plane": {"origin": [20, 0, 0], "normal": [1, 0, 0]}}));
    run(&mut s, "solid.hole", json!({"position": [30, 10, 10], "diameter": 5, "name": "Hole1", "targets": ["Body1"]}));
    assert_eq!(said(&s, "Hole1"), "");
}

/// A plate with a round wall on each long side, and a tall tab with a curved notch joined to its
/// far end, 12 mm from those walls: this join reports no history, and the bigger body meshes the
/// walls differently. Returns the plate's and the tab's feature ids.
fn plate_and_notched_tab(s: &mut Session) -> (Option<u64>, Option<u64>) {
    let reach = 57.0_f64.sqrt(); // Where a circle of radius 11 meets y = ±8.
    run(s, "sketch.create", json!({"plane": "XY"}));
    run(s, "sketch.rectangle.two_point", json!({"p0": [-20, -8], "p1": [20, 8]}));
    run(s, "sketch.arc.three_point", json!({"start": [-reach, 8], "end": [reach, 8], "through": [0, 11]}));
    run(s, "sketch.arc.three_point", json!({"start": [-reach, -8], "end": [reach, -8], "through": [0, -11]}));
    run(s, "sketch.finish", json!({}));
    let plate = run(s, "solid.extrude", json!({"distance": 10, "name": "Plate"}))["feature"].as_u64();
    run(s, "sketch.create", json!({"plane": {"face": [20, 0, 5]}, "project_edges": false}));
    run(s, "sketch.line", json!({"points": [[-8, -5], [8, -5], [8, 23], [-8, 23], [-8, -5]]}));
    run(s, "sketch.circle.center", json!({"center": [0, 23], "radius": 8.0667}));
    run(s, "sketch.finish", json!({}));
    let tab =
        run(s, "solid.extrude", json!({"distance": 10, "operation": "join", "name": "Tab", "profiles": [{"point": [0, -3]}]}))["feature"].as_u64();
    (plate, tab)
}

/// The round walls stay the plate's whole, and an edge picked on one stores no trace of the tab.
#[test]
fn a_join_that_makes_the_body_bigger_leaves_round_walls_it_never_touched_alone() {
    let mut s = Session::default();
    let (plate, tab) = plate_and_notched_tab(&mut s);
    let tab_name = format!("F{}", tab.unwrap_or_default());
    let made_by = |face: &Value| face["name"].as_str().and_then(solidcraft_doc::persistent_name::made_by);
    let all = faces(&mut s, "Body1");
    let round: Vec<&Value> = all.iter().filter(|f| f.get("plane_normal").is_none_or(Value::is_null)).collect();
    let walls: Vec<&&Value> = round.iter().filter(|f| vec(&f["point"]).x < 15.0).collect();
    assert_eq!(walls.len(), 2, "{all:?}");
    for wall in walls {
        assert_eq!(made_by(wall), plate, "{wall}");
        assert!(wall["name"].as_str().is_some_and(|name| !name.contains(&tab_name)), "{wall}");
    }
    assert!(round.iter().filter(|f| vec(&f["point"]).x > 20.0).all(|notch| made_by(notch) == tab), "{all:?}");

    let volume_before = volume(&mut s, "Body1");
    run(&mut s, "solid.fillet", json!({"edges": [[0, -11, 10]], "radius": 1, "name": "Round"}));
    assert!(volume(&mut s, "Body1") < volume_before);
    let filleted = volume(&mut s, "Body1");
    let filleted_faces = faces(&mut s, "Body1");
    let round_id = s.doc.features.iter().position(|f| f.name == "Round").unwrap();
    assert!(s.doc.features[round_id].edge_names.iter().all(|name| !name.contains(&tab_name)), "{:?}", s.doc.features[round_id].edge_names);

    // A design saved while the wall was named after the tab still finds its edge.
    let mut saved = (*s.doc).clone();
    saved.features[round_id].edge_names = vec![format!("F2:top|{tab_name}:side:l4#[F2:side:l1]")];
    let mut reopened = Session::opened_at(saved.clone(), None, None);
    let result = reopened.model.result(reopened.doc.features[round_id].id).unwrap();
    assert!(!result.skipped && result.error.is_none(), "{:?}", result.error);
    assert!((volume(&mut reopened, "Body1") - filleted).abs() < 1e-6);
    // The same edge: the plate is symmetric, so the volume alone would take a round on a mirrored
    // edge as well. Every face of the body filleted fresh is there, where it was.
    let reopened_faces = faces(&mut reopened, "Body1");
    assert_eq!(reopened_faces.len(), filleted_faces.len(), "{reopened_faces:?}");
    for face in &filleted_faces {
        assert!(reopened_faces.iter().any(|f| vec(&f["point"]).dist(vec(&face["point"])) < 1e-6), "{face} is not in {reopened_faces:?}");
    }

    // Only the point says which edge that old name meant, and only among the edges of the face the
    // name still finds. Each of these still asks for the edge again: a point on the wall but on no
    // edge; the corner where the wall's top edge meets the plate's straight side; a face of a
    // feature that made nothing on the body (removed, not renamed); the plate's bottom, which the
    // wall's top edge does not bound.
    let on_edge = Vec3::new(0.0, -11.0, 10.0);
    for (name, at) in [
        (format!("F2:top|{tab_name}:side:l4#[F2:side:l1]"), Vec3::new(0.0, -11.0, 5.0)),
        (format!("F2:top|{tab_name}:side:l4#[F2:side:l1]"), Vec3::new(57.0_f64.sqrt(), -8.0, 10.0)),
        ("F2:top|F99:side:l4#[F2:side:l1]".to_string(), on_edge),
        (format!("F2:start|{tab_name}:side:l4#[F2:side:l1]"), on_edge),
    ] {
        let mut off = saved.clone();
        off.features[round_id].edge_names = vec![name.clone()];
        if let solidcraft_doc::FeatureKind::Fillet { edges, .. } = &mut off.features[round_id].kind {
            edges[0].0 = at;
        }
        let reopened = Session::opened_at(off, None, None);
        let result = reopened.model.result(reopened.doc.features[round_id].id).unwrap();
        assert!(result.error.as_deref().is_some_and(|e| e.contains("reselect")), "{name} at {at:?}: {:?}", result.error);
    }
}

/// A 40 x 20 x 10 plate with a bar joined across its top from x = `x0` to `x1` (y -5…25, 5 high),
/// splitting the top into two faces of one stem, told apart by their neighbours, and a chamfer on
/// the plate's front top edge left of the bar, picked at (5, 0, 10). Returns the chamfer's index.
fn plate_split_by_a_bar(s: &mut Session, x0: f64, x1: f64) -> usize {
    run(s, "sketch.create", json!({"plane": "XY"}));
    run(s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [40, 20]}));
    run(s, "sketch.finish", json!({}));
    run(s, "solid.extrude", json!({"distance": 10, "name": "Plate"}));
    run(s, "sketch.create", json!({"plane": {"face": [x0 / 2.0, 10, 10]}, "project_edges": false}));
    run(s, "sketch.rectangle.two_point", json!({"p0": [x0, -5], "p1": [x1, 25]}));
    run(s, "sketch.finish", json!({}));
    run(s, "solid.extrude", json!({"distance": 5, "operation": "join", "name": "Bar"}));
    run(s, "solid.chamfer", json!({"edges": [[5, 0, 10]], "distance": 1, "name": "Bevel"}));
    s.doc.features.iter().position(|f| f.name == "Bevel").unwrap()
}

/// A design saved with the plate's top named after other neighbours than it has now (a later
/// join, or another build, named the walls around it otherwise), on a body with two top faces of
/// that stem: no face answers to the name exactly, and the saved pick decides, on the one edge
/// between the front side and a top face whose exact curve it lies on.
#[test]
fn a_chamfer_saved_under_a_top_faces_old_neighbours_is_found_at_its_pick() {
    let mut s = Session::default();
    let bevel = plate_split_by_a_bar(&mut s, 15.0, 25.0);
    let tops: Vec<String> =
        faces(&mut s, "Body1").iter().filter_map(|f| f["name"].as_str().map(str::to_string)).filter(|n| n.starts_with("F2:top#[")).collect();
    assert_eq!(tops.len(), 2, "{tops:?}");
    let picked = s.doc.features[bevel].edge_names.clone();
    assert_eq!(picked, vec!["F2:side:l1|F2:top#[F2:side:l4,F4:side:l4]".to_string()]);
    let (chamfered, chamfered_faces) = (volume(&mut s, "Body1"), faces(&mut s, "Body1"));

    // F4:side:l1 (the bar's far wall) bounds neither top face: the name answers to both by stem only.
    let stale = "F2:side:l1|F2:top#[F2:side:l4,F4:side:l1]";
    let mut saved = (*s.doc).clone();
    saved.features[bevel].edge_names = vec![stale.to_string()];
    let mut reopened = Session::opened_at(saved.clone(), None, None);
    let result = reopened.model.result(reopened.doc.features[bevel].id).unwrap();
    assert!(!result.skipped && result.error.is_none(), "{:?}", result.error);
    assert!((volume(&mut reopened, "Body1") - chamfered).abs() < 1e-6);
    // The same edge, not the one right of the bar (the plate is symmetric about it).
    let reopened_faces = faces(&mut reopened, "Body1");
    assert_eq!(reopened_faces.len(), chamfered_faces.len(), "{reopened_faces:?}");
    for face in &chamfered_faces {
        assert!(reopened_faces.iter().any(|f| vec(&f["point"]).dist(vec(&face["point"])) < 1e-6), "{face} is not in {reopened_faces:?}");
    }

    // Right of the bar is the other candidate's; on the front side below the edge, or on the bar's
    // edge between the two, is no candidate's: the point lies on exactly one there, or none.
    for (at, finds) in [(Vec3::new(35.0, 0.0, 10.0), true), (Vec3::new(5.0, 0.0, 5.0), false), (Vec3::new(20.0, 0.0, 10.0), false)] {
        let mut off = saved.clone();
        if let solidcraft_doc::FeatureKind::Chamfer { edges, .. } = &mut off.features[bevel].kind {
            edges[0].0 = at;
        }
        let reopened = Session::opened_at(off, None, None);
        let result = reopened.model.result(reopened.doc.features[bevel].id).unwrap();
        if finds {
            assert!(result.error.is_none(), "{at:?}: {:?}", result.error);
        } else {
            assert!(result.error.as_deref().is_some_and(|e| e.contains("reselect")), "{at:?}: {:?}", result.error);
        }
    }
}

/// With a bar 0.1 mm wide, the two edges between the front side and the top faces end within a
/// pick's reach of one point between them: the point lies on both, and the name asks again.
#[test]
fn a_chamfer_whose_old_name_and_pick_fit_two_edges_asks_again() {
    let mut s = Session::default();
    let bevel = plate_split_by_a_bar(&mut s, 20.0, 20.1);
    let mut saved = (*s.doc).clone();
    saved.features[bevel].edge_names = vec!["F2:side:l1|F2:top#[F2:side:l4,F4:side:l1]".to_string()];
    if let solidcraft_doc::FeatureKind::Chamfer { edges, .. } = &mut saved.features[bevel].kind {
        edges[0].0 = Vec3::new(20.05, 0.0, 10.0);
    }
    let reopened = Session::opened_at(saved, None, None);
    let result = reopened.model.result(reopened.doc.features[bevel].id).unwrap();
    assert!(result.error.as_deref().is_some_and(|e| e.contains("reselect")), "{:?}", result.error);
}

/// A design saved by 0.30.1 numbered the pieces of a cylinder's round side (`F1:side#2`), which the
/// kernel now builds whole. A cosmetic thread picked on it is found again on that side when its
/// pick lies on it (#16602), for either piece number, and a pick off the face asks again.
#[test]
fn a_thread_saved_under_a_numbered_side_piece_of_a_cylinder_is_found_at_its_pick() {
    let mut s = Session::default();
    run(&mut s, "solid.cylinder", json!({"radius": 5, "height": 30, "base": [0, 0, 0], "body_name": "Shaft"}));
    run(&mut s, "solid.thread", json!({"face": [0, 5, 15], "designation": "M10", "modeled": false, "name": "Thread1"}));
    let thread = s.doc.features.iter().position(|f| f.name == "Thread1").unwrap();
    assert_eq!(s.doc.features[thread].face_names, vec!["F1:side".to_string()]);

    for piece in ["F1:side#1", "F1:side#2"] {
        let mut saved = (*s.doc).clone();
        saved.features[thread].face_names = vec![piece.to_string()];
        let reopened = Session::opened_at(saved, None, None);
        let result = reopened.model.result(reopened.doc.features[thread].id).unwrap();
        assert!(result.error.is_none(), "{piece}: {:?}", result.error);
        assert_eq!(reopened.model.state().threads.len(), 1, "{piece}");
    }

    let mut off = (*s.doc).clone();
    off.features[thread].face_names = vec!["F1:side#2".to_string()];
    if let solidcraft_doc::FeatureKind::Thread { face, .. } = &mut off.features[thread].kind {
        face.0 = Vec3::new(0.0, 5.0, 45.0);
    }
    let reopened = Session::opened_at(off, None, None);
    let result = reopened.model.result(reopened.doc.features[thread].id).unwrap();
    assert!(result.error.as_deref().is_some_and(|e| e.contains("reselect")), "{:?}", result.error);
}

/// The same for a modelled thread in a hole: 0.30.1 built the hole's wall in pieces, named by number
/// (`F4:bore:side:0#1`) or by a later index (`F4:bore:side:1`); it is one face now (#16602).
#[test]
fn a_modelled_thread_saved_under_a_numbered_bore_wall_is_found_at_its_pick() {
    let mut s = Session::default();
    run(&mut s, "solid.cylinder", json!({"radius": 10, "height": 40, "base": [40, 0, 0], "body_name": "Boss"}));
    run(&mut s, "solid.hole", json!({"position": [40, 0, 40], "diameter": 8.5, "depth": 25, "body_name": "Boss"}));
    run(&mut s, "solid.thread", json!({"face": [44.25, 0, 20], "designation": "M10", "modeled": true, "name": "Tapped"}));
    let thread = s.doc.features.iter().position(|f| f.name == "Tapped").unwrap();
    let whole = s.doc.features[thread].face_names.clone();
    assert!(whole[0].contains("bore:side"), "{whole:?}");
    let volume_whole = volume(&mut s, "Boss");

    let old_index = format!("{}:1", whole[0].strip_suffix(":0").unwrap_or(&whole[0]));
    for piece in [format!("{}#1", whole[0]), old_index.clone()] {
        let mut saved = (*s.doc).clone();
        saved.features[thread].face_names = vec![piece.clone()];
        let mut reopened = Session::opened_at(saved, None, None);
        let result = reopened.model.result(reopened.doc.features[thread].id).unwrap();
        assert!(result.error.is_none(), "{piece}: {:?}", result.error);
        assert!((volume(&mut reopened, "Boss") - volume_whole).abs() < 1e-6, "{piece}");
    }

    let mut off = (*s.doc).clone();
    off.features[thread].face_names = vec![format!("{}#1", whole[0])];
    if let solidcraft_doc::FeatureKind::Thread { face, .. } = &mut off.features[thread].kind {
        face.0 = Vec3::new(50.0, 0.0, 20.0);
    }
    let reopened = Session::opened_at(off, None, None);
    let result = reopened.model.result(reopened.doc.features[thread].id).unwrap();
    assert!(result.error.as_deref().is_some_and(|e| e.contains("reselect")), "{:?}", result.error);
}

/// The recovery is a thread's alone (#16602): any other feature saved under the same numbered piece
/// of a cylinder's side would take in the whole side the old piece never covered, so it asks for
/// the face to be picked again.
#[test]
fn a_shell_saved_under_a_numbered_side_piece_of_a_cylinder_still_asks_to_reselect() {
    let mut s = Session::default();
    run(&mut s, "solid.cylinder", json!({"radius": 5, "height": 30, "base": [0, 0, 0], "body_name": "Shaft"}));
    run(&mut s, "solid.shell", json!({"faces": [[0, 5, 15]], "thickness": 1, "body": "Shaft", "name": "Shell1"}));
    let shell = s.doc.features.iter().position(|f| f.name == "Shell1").unwrap();
    assert_eq!(s.doc.features[shell].face_names, vec!["F1:side".to_string()]);

    let mut saved = (*s.doc).clone();
    saved.features[shell].face_names = vec!["F1:side#2".to_string()];
    let reopened = Session::opened_at(saved, None, None);
    let result = reopened.model.result(reopened.doc.features[shell].id).unwrap();
    assert!(result.error.as_deref().is_some_and(|e| e.contains("reselect")), "{:?}", result.error);
}

/// A 40 x 20 x 10 block, shelled open at the side face of its line `l1` (picked at (20, 0, 5)).
/// Returns the shell's feature id.
fn block_shelled_at_its_first_side(s: &mut Session) -> u64 {
    run(s, "sketch.create", json!({"plane": "XY", "name": "S1"}));
    run(s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [40, 20]}));
    run(s, "sketch.finish", json!({}));
    run(s, "solid.extrude", json!({"distance": 10, "name": "E1"}));
    let id = run(s, "solid.shell", json!({"faces": [[20, 0, 5]], "thickness": 2}))["feature"].as_u64().unwrap_or(0);
    assert_eq!(shell_error(s, id), None);
    id
}

fn shell_error(s: &Session, id: u64) -> Option<String> {
    s.model.result(id).and_then(|r| r.error.clone())
}

/// Delete `l1` and draw `line` in the sketch under the extrude.
fn replace_first_side(s: &mut Session, line: [[f64; 2]; 2]) {
    run(s, "sketch.edit", json!({"sketch": "S1"}));
    run(s, "sketch.delete", json!({"entities": ["l1"]}));
    run(s, "sketch.line", json!({"points": line}));
    run(s, "sketch.finish", json!({}));
}

/// #16661: a side deleted and drawn again where it was is a new line with a new id, and the
/// shell's saved face still finds the face on it.
#[test]
fn a_side_line_deleted_and_drawn_again_keeps_the_shells_face() {
    let mut s = Session::default();
    let shell = block_shelled_at_its_first_side(&mut s);
    replace_first_side(&mut s, [[0.0, 0.0], [40.0, 0.0]]);
    assert_eq!(shell_error(&s, shell), None);
}

/// A different line in its place is another face: the shell asks for it to be picked again, and a
/// point pick through `timeline.edit` repairs the reference.
#[test]
fn a_different_line_in_its_place_asks_to_reselect_and_a_point_pick_repairs_it() {
    let mut s = Session::default();
    let shell = block_shelled_at_its_first_side(&mut s);
    replace_first_side(&mut s, [[0.0, 0.0], [40.0, 6.0]]);
    let error = shell_error(&s, shell).unwrap_or_default();
    assert!(error.contains("reselect"), "{error}");
    // An edit that sends no point leaves the stored name alone: it still fails.
    let stored = s.doc.feature(shell).map(|f| f.face_names.clone()).unwrap_or_default();
    s.execute("timeline.edit", &json!({"feature": shell, "set": {"thickness": 1}})).ok();
    assert_eq!(s.doc.feature(shell).map(|f| f.face_names.clone()), Some(stored));
    // Picking the face now on the other line stores its name ...
    run(&mut s, "timeline.edit", json!({"feature": shell, "set": {"faces": [[20, 3, 5]]}}));
    assert_eq!(shell_error(&s, shell), None);
    // ... and the stored name is the new face's, so a reopened design finds it too.
    let reopened = Session::new(solidcraft_doc::format::read(&s.doc.to_json()).unwrap());
    assert_eq!(shell_error(&reopened, shell), None);
}

/// #16661: an edit that sends only `extent.distance` of an extrude that ends at a face sends no
/// face point, so its stored face names stay; sending the `extent.to` point stores them fresh.
#[test]
fn an_extrude_to_a_face_keeps_its_names_when_only_the_distance_is_edited() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"corner": [-10, -10, 20], "length": 20, "width": 20, "height": 5, "body_name": "Plate"}));
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [-5, -5], "p1": [5, 5]}));
    run(&mut s, "sketch.finish", json!({}));
    let id = run(&mut s, "solid.extrude", json!({"to": [0, 0, 20]}))["feature"].as_u64().unwrap_or(0);
    let names = |s: &Session| s.doc.feature(id).map(|f| f.face_names.clone()).unwrap_or_default();
    let real = names(&s);
    assert!(!real.is_empty() && real.iter().all(|n| !n.is_empty()), "{real:?}");

    run(&mut s, "timeline.edit", json!({"feature": id, "set": {"extent.distance": 7}}));
    assert_eq!(names(&s), real);
    assert_eq!(shell_error(&s, id), None);

    // Another real face name of the plate in its place: sending the same point stores the face's own.
    let other = faces(&mut s, "Plate").iter().filter_map(|f| f["name"].as_str().map(str::to_string)).find(|n| !real.contains(n)).unwrap_or_default();
    assert!(!other.is_empty());
    if let Some(f) = s.doc_mut().feature_mut(id) {
        f.face_names = vec![other.clone()];
    }
    run(&mut s, "timeline.edit", json!({"feature": id, "set": {"extent.to": [0, 0, 20]}}));
    assert_eq!(names(&s), real);
    assert_eq!(shell_error(&s, id), None);
}

/// Regression (#21380): an edit to a Fillet or Chamfer that omits `edges` (such as changing
/// `radius` or `distance`) keeps its stored `edge_names` and `edge_refs`, whereas sending `edges`
/// — even at the exact same coordinates (#16661) — reselects broken edge references now.
#[test]
fn fillet_and_chamfer_reselect_broken_edges_when_timeline_edit_sends_edges() {
    for (cmd, size_key) in [("solid.fillet", "radius"), ("solid.chamfer", "distance")] {
        let mut s = Session::default();
        run(&mut s, "sketch.create", json!({"plane": "XY", "name": "S1"}));
        run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [40, 20]}));
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "solid.extrude", json!({"distance": 10}));
        let pick = [20.0, 0.0, 10.0];
        let id = run(&mut s, cmd, json!({"edges": [pick], size_key: 1.5}))["feature"].as_u64().unwrap();
        let real_names = s.doc.feature(id).map(|f| f.edge_names.clone()).unwrap_or_default();
        let real_refs = s.doc.feature(id).map(|f| f.edge_refs.clone()).unwrap_or_default();
        assert!(!real_names.is_empty() && real_names.iter().all(|n| !n.is_empty()), "{cmd}: {real_names:?}");
        assert!(!real_refs.is_empty(), "{cmd}: {real_refs:?}");

        // Editing only the radius/distance keeps the stored edge names and refs intact.
        run(&mut s, "timeline.edit", json!({"feature": id, "set": {size_key: 2.0}}));
        assert_eq!(s.doc.feature(id).map(|f| f.edge_names.clone()), Some(real_names.clone()));
        assert_eq!(s.doc.feature(id).map(|f| f.edge_refs.clone()), Some(real_refs.clone()));
        assert_eq!(shell_error(&s, id), None);

        // Simulate stale edge metadata pointing to a missing edge name/ref.
        let stale_names = vec!["F99:stale|F99:edge".to_string()];
        if let Some(f) = s.doc_mut().feature_mut(id) {
            f.edge_names = stale_names.clone();
            f.edge_refs.clear();
        }
        s.execute("timeline.edit", &json!({"feature": id, "set": {size_key: 1.2}})).ok();
        assert_eq!(s.doc.feature(id).map(|f| f.edge_names.clone()), Some(stale_names));
        assert!(shell_error(&s, id).is_some(), "{cmd}: stale reference should still fail when `edges` is omitted");

        // Re-sending the same coordinates refreshes `edge_names` and `edge_refs` and clears the error.
        run(&mut s, "timeline.edit", json!({"feature": id, "set": {"edges": [pick]}}));
        assert_eq!(s.doc.feature(id).map(|f| f.edge_names.clone()), Some(real_names));
        assert_eq!(s.doc.feature(id).map(|f| f.edge_refs.clone()), Some(real_refs));
        assert_eq!(shell_error(&s, id), None);

        let reopened = Session::new(solidcraft_doc::format::read(&s.doc.to_json()).unwrap());
        assert_eq!(shell_error(&reopened, id), None);
    }
}

/// A design saved by 0.30.1 numbered the pieces of each peg's round side (`F3:side#1`, `F4:side#2`),
/// which the kernel now builds whole. A chamfer on the pegs' top edges saved under those names is
/// found again on the edge each pick lies on (#16535): the same edges the chamfer had when picked,
/// and a pick on the other peg is not that name's edge, so it asks again.
#[test]
fn a_chamfer_saved_under_numbered_side_pieces_of_a_peg_is_found_at_its_pick() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 40, "width": 20, "height": 10, "body_name": "Plate"}));
    run(&mut s, "solid.cylinder", json!({"base": [10, 10, 10], "radius": 2, "height": 6, "operation": "join", "name": "PegW"}));
    run(&mut s, "solid.cylinder", json!({"base": [30, 10, 10], "radius": 2, "height": 6, "operation": "join", "name": "PegE"}));
    run(&mut s, "solid.chamfer", json!({"edges": [[12, 10, 16], [32, 10, 16]], "distance": 0.5, "name": "LeadIn"}));
    let lead_in = s.doc.features.iter().position(|f| f.name == "LeadIn").unwrap();
    let id = |name: &str| s.doc.features.iter().find(|f| f.name == name).map(|f| f.id).unwrap();
    let (west, east) = (id("PegW"), id("PegE"));
    assert_eq!(s.doc.features[lead_in].edge_names, vec![format!("F{west}:side|F{west}:top"), format!("F{east}:side|F{east}:top")]);
    let (chamfered, chamfered_faces) = (volume(&mut s, "Plate"), faces(&mut s, "Plate"));

    let mut saved = (*s.doc).clone();
    saved.features[lead_in].edge_names = vec![format!("F{west}:side#1|F{west}:top"), format!("F{east}:side#2|F{east}:top")];
    let mut reopened = Session::opened_at(saved.clone(), None, None);
    let result = reopened.model.result(reopened.doc.features[lead_in].id).unwrap();
    assert!(!result.skipped && result.error.is_none(), "{:?}", result.error);
    assert!((volume(&mut reopened, "Plate") - chamfered).abs() < 1e-6);
    let names = |all: Vec<Value>| {
        let mut n: Vec<String> = all.iter().filter_map(|f| f["name"].as_str().map(str::to_owned)).collect();
        n.sort();
        n
    };
    // The same faces by name: the chamfer's two cone faces and everything around them.
    assert_eq!(names(faces(&mut reopened, "Plate")), names(chamfered_faces));

    // The first name with its pick on the other peg's top edge is not that peg's edge: it asks again.
    let mut off = saved;
    if let solidcraft_doc::FeatureKind::Chamfer { edges, .. } = &mut off.features[lead_in].kind {
        edges[0].0 = Vec3::new(32.0, 10.0, 16.0);
    }
    let reopened = Session::opened_at(off, None, None);
    let result = reopened.model.result(reopened.doc.features[lead_in].id).unwrap();
    assert!(result.error.as_deref().is_some_and(|e| e.contains("reselect")), "{:?}", result.error);
}

/// A design saved by 0.30.1 (#14479): a round part with a flat (a circle of radius 15 cut off by a
/// line at y = -3), whose round side and flat side meet along two upright edges (`~1` and `~2`).
/// Round fillets both, and Bevel chamfers the top edges, the rounds' among them. 0.30.1 wrote
/// Bevel's names in the older form, with the number of the edge each round replaced at the end
/// (`F2:top|F3:blend:F2:side:c1|F2:side:l1~2`). That number is the round's edge, not a second edge
/// between the top and the round, and Bevel finds the round's top edge at its pick again. A pick on
/// another edge is not that round's edge: it asks again.
#[test]
fn a_chamfer_saved_on_a_rounds_edge_with_its_number_at_the_end_is_found_at_its_pick() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    run(&mut s, "sketch.circle.center", json!({"center": [0, 0], "radius": 15}));
    run(&mut s, "sketch.line", json!({"points": [[-20, -3], [20, -3]]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"distance": 10, "profiles": [{"point": [0, -10]}], "body_name": "Part"}));
    run(&mut s, "solid.fillet", json!({"edges": [[14.697, -3, 5], [-14.697, -3, 5]], "radius": 2, "name": "Round"}));
    let picks = [[0.0, -3.0, 10.0], [13.6641, -3.8906, 10.0], [-13.6641, -3.8906, 10.0], [0.0, -15.0, 10.0]];
    run(&mut s, "solid.chamfer", json!({"edges": picks, "distance": 2, "name": "Bevel"}));
    let index = |name: &str| s.doc.features.iter().position(|f| f.name == name).unwrap();
    let (round, bevel) = (index("Round"), index("Bevel"));
    let rounded = s.doc.features[round].edge_names.clone();
    assert_eq!(rounded, vec!["F2:side:c1|F2:side:l1~2".to_string(), "F2:side:c1|F2:side:l1~1".to_string()]);
    let (chamfered, chamfered_faces) = (volume(&mut s, "Part"), faces(&mut s, "Part"));

    let round_id = s.doc.features[round].id;
    let mut saved = (*s.doc).clone();
    saved.features[bevel].edge_names[1] = format!("F2:top|F{round_id}:blend:{}", rounded[0]);
    saved.features[bevel].edge_names[2] = format!("F2:top|F{round_id}:blend:{}", rounded[1]);
    let mut reopened = Session::opened_at(saved.clone(), None, None);
    let result = reopened.model.result(reopened.doc.features[bevel].id).unwrap();
    assert!(!result.skipped && result.error.is_none(), "{:?}", result.error);
    assert!((volume(&mut reopened, "Part") - chamfered).abs() < 1e-6);
    let reopened_faces = faces(&mut reopened, "Part");
    assert_eq!(reopened_faces.len(), chamfered_faces.len(), "{reopened_faces:?}");
    for face in &chamfered_faces {
        assert!(reopened_faces.iter().any(|f| vec(&f["point"]).dist(vec(&face["point"])) < 1e-6), "{face} is not in {reopened_faces:?}");
    }

    // The round's name with its pick on the flat side's top edge, or on the round side's: neither
    // edge is between the top and a round, and that name asks again.
    let asked = format!("`{}` no longer exists", saved.features[bevel].edge_names[1]);
    for at in [Vec3::new(6.0, -3.0, 10.0), Vec3::new(5.0, -14.142135623730951, 10.0)] {
        let mut off = saved.clone();
        if let solidcraft_doc::FeatureKind::Chamfer { edges, .. } = &mut off.features[bevel].kind {
            edges[1].0 = at;
        }
        let reopened = Session::opened_at(off, None, None);
        let result = reopened.model.result(reopened.doc.features[bevel].id).unwrap();
        assert!(result.error.as_deref().is_some_and(|e| e.contains(&asked)), "{at:?}: {:?}", result.error);
    }
}

/// 0.40 rc.3 saved a chamfer's face as `F3:blend:F2:side:l1|F2:top`. Ordinary face
/// lookup already understood that spelling, but the sketch's lineage anchor compared text
/// and lost the face when names started using parentheses (#20186).
#[test]
fn a_sketch_saved_on_a_chamfer_in_either_name_form_reopens_on_its_face() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [40, 20]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"distance": 10, "body_name": "Block"}));
    let chamfer = run(&mut s, "solid.chamfer", json!({"edges": [[20, 0, 10]], "distance": 3, "name": "Chamfer1"}))["feature"].as_u64().unwrap();
    let current = format!("F{chamfer}:blend(F2:side:l1|F2:top)");
    let old = format!("F{chamfer}:blend:F2:side:l1|F2:top");
    let before = faces(&mut s, "Block");
    assert_eq!(before.iter().filter(|face| face["name"] == current).count(), 1, "{before:?}");

    // The bevel lies in z - y = 7, with outward normal (0, -1, 1) / sqrt(2).
    let point = Vec3::new(20.0, 1.5, 8.5);
    let made = run(&mut s, "sketch.create", json!({"plane": {"face": [point.x, point.y, point.z]}, "project_edges": false, "name": "OnChamfer"}));
    assert_eq!(made["attachment"], "exact_face", "{made}");
    let sketch = made["sketch"].as_u64().unwrap();
    let local = s.model.state().sketches.iter().find(|solved| solved.feature == sketch).unwrap().plane.to_local(point);
    run(&mut s, "sketch.circle.center", json!({"center": [local.x, local.y], "radius": 0.5}));
    run(&mut s, "sketch.finish", json!({}));
    let boss = run(&mut s, "solid.extrude", json!({"sketch": "OnChamfer", "distance": 2, "operation": "new", "body_name": "Boss"}))["feature"]
        .as_u64()
        .unwrap();
    let original = s.model.state().sketches.iter().find(|solved| solved.feature == sketch).unwrap().plane;

    for name in [&current, &old] {
        let mut saved = (*s.doc).clone();
        let solidcraft_doc::FeatureKind::Sketch { plane: solidcraft_doc::PlaneRef::Face { name: stored, anchor, .. }, .. } =
            &mut saved.feature_mut(sketch).unwrap().kind
        else {
            panic!("a sketch attached to the chamfer face")
        };
        let anchor = anchor.as_mut().unwrap();
        assert_eq!(anchor.target.name, current);
        assert!(matches!(anchor.target.certificate, Some(solidcraft_doc::config::FaceCertificate::Lineage)));
        *stored = Some(name.clone());
        anchor.target.name = name.clone();

        // No saved model cache: reading the document must rebuild the chamfer and its sketch.
        let mut reopened = Session::opened_at(solidcraft_doc::format::read(&saved.to_json()).unwrap(), None, None);
        for feature in [chamfer, sketch, boss] {
            let result = reopened.model.result(feature).unwrap();
            assert!(!result.skipped && result.error.is_none(), "{name}: {result:?}");
        }
        let state = reopened.model.state();
        let plane = &state.sketches.iter().find(|solved| solved.feature == sketch).unwrap().plane;
        assert!((plane.origin.z - plane.origin.y - 7.0).abs() < 1e-9, "{name}: {plane:?}");
        assert!(plane.normal().dot(Vec3::new(0.0, -1.0, 1.0).normalized().unwrap()) > 1.0 - 1e-9, "{name}: {plane:?}");
        assert!(plane.origin.dist(original.origin) < 1e-9 && plane.x.dist(original.x) < 1e-9 && plane.y.dist(original.y) < 1e-9);
        assert!(state.body("Boss").is_some(), "the extrude on the attached sketch rebuilds");
        // Compatibility changes lookup only: newly listed faces keep today's names.
        let names = |all: Vec<Value>| {
            let mut names: Vec<String> = all.iter().map(|face| face["name"].as_str().unwrap().to_string()).collect();
            names.sort();
            names
        };
        assert_eq!(names(faces(&mut reopened, "Block")), names(before.clone()));
    }

    // The same saved point cannot rescue the name of an edge the chamfer never replaced.
    let mut missing = (*s.doc).clone();
    let solidcraft_doc::FeatureKind::Sketch { plane: solidcraft_doc::PlaneRef::Face { anchor, .. }, .. } =
        &mut missing.feature_mut(sketch).unwrap().kind
    else {
        panic!("a sketch attached to the chamfer face")
    };
    anchor.as_mut().unwrap().target.name = format!("F{chamfer}:blend:F2:side:l2|F2:top");
    let reopened = Session::opened_at(solidcraft_doc::format::read(&missing.to_json()).unwrap(), None, None);
    assert!(reopened.model.result(sketch).unwrap().error.as_deref().is_some_and(|error| error.contains("no longer exists")));
}

/// A face that `model.faces` marks `exact` has its exact area. On a 30 x 30 x 5 plate with a
/// radius 5 hole the hole's wall reads 156.83 for 2π·5·5 = 157.08 and the top face 821.97 for
/// 900 - 25π = 821.46, both marked exact, because they come from the mesh
/// ([T-93](https://tickets.solidcraft.io/t/93), #17588).
#[test]
fn faces_marked_exact_report_their_exact_area() {
    let mut s = Session::default();
    plate_with_hole(&mut s, 30.0, 30.0, 5.0);
    let mut checked = 0;
    for f in faces(&mut s, "Plate") {
        let want = if f.get("cylinder").is_some() {
            2.0 * PI * 5.0 * 5.0
        } else if f["plane_normal"][2].as_f64().is_some_and(|z| z.abs() > 0.99) {
            900.0 - 25.0 * PI
        } else {
            30.0 * 5.0
        };
        if f["exact"] == true {
            let got = f["area"].as_f64().unwrap();
            assert!((got - want).abs() < 1e-6 * want, "{}: {got} vs {want}", f["name"]);
            checked += 1;
        }
    }
    // The four sides, at least, are exact.
    assert!(checked >= 4, "{checked}");
}

/// #21192: renaming a body of a design opened from its saved model keeps its faces' history
/// names. They came only from the saved model, and the rename dropped them for `<name>:face:<k>`,
/// which a later feature then stored and a full evaluation no longer found.
#[test]
fn renaming_a_body_opened_from_its_saved_model_keeps_its_face_names() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 50, "width": 50, "height": 20, "body_name": "Block"}));
    let name = |f: &Value| f["name"].as_str().unwrap_or_default().to_string();
    let before: Vec<String> = faces(&mut s, "Block").iter().map(name).collect();
    assert!(before.iter().all(|n| !n.contains(":face:")), "{before:?}");
    let saved = s.model.saved(&s.doc).unwrap();
    s.model = solidcraft_doc::Model::adopt(&s.doc, &saved).unwrap();
    run(&mut s, "body.rename", json!({"body": "Block", "name": "Renamed"}));
    let after: Vec<String> = faces(&mut s, "Renamed").iter().map(name).collect();
    assert_eq!(after, before);
    run(&mut s, "solid.hole", json!({"position": [25, 25, 20], "direction": [0, 0, -1], "diameter": 5, "depth": 10, "name": "Hole1"}));
    s.model = solidcraft_doc::Model::new();
    s.model.evaluate(&s.doc);
    let hole = s.model.state().bodies.iter().find(|b| b.name == "Renamed").map(|b| b.feature);
    let errors: Vec<String> = s.model.results.iter().filter_map(|r| r.error.clone()).collect();
    assert!(errors.is_empty(), "{errors:?} (body by {hole:?})");
}
