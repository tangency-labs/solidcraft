// component.set_origin (#13136): the origin moves, nothing else does.
use crate::Session;
use serde_json::{Value, json};
use solidcore_record::Vec3;
use solidcraft_doc::{Mat, apply_point, apply_vector, mat_mul};
use solidcraft_geom::prelude::*;

fn run(s: &mut Session, command: &str, p: Value) -> Value {
    s.execute(command, &p).unwrap_or_else(|e| panic!("{command} {p}: {e}"))
}

fn created(s: &mut Session, name: &str) -> (u64, u64) {
    let c = run(s, "component.create", json!({"name": name, "grounded": false}));
    (c["component"].as_u64().unwrap(), c["occurrence"].as_u64().unwrap())
}

/// Every body's world box, rounded to 0.0001 mm, by name.
fn boxes(s: &Session) -> Vec<(String, [i64; 6])> {
    let mut out: Vec<(String, [i64; 6])> = s
        .world_state()
        .unwrap()
        .bodies
        .iter()
        .map(|b| {
            let bb = solidcraft_kernel::measure(&b.body).unwrap().bbox;
            (b.name.clone(), [bb.min.x, bb.min.y, bb.min.z, bb.max.x, bb.max.y, bb.max.z].map(|v| (v * 1e4).round() as i64))
        })
        .collect();
    out.sort();
    out
}

/// Every sketch plane in the world (rounded to 0.0001 mm), and every construction item as a
/// plane (its normal and its distance from the world origin), a line (its direction and the
/// point of it nearest the world origin) or a point.
fn frames(s: &Session) -> Vec<(u64, Vec<i64>)> {
    use solidcraft_doc::construct::ConstructGeom as G;
    let st = s.world_state().unwrap();
    let r = |v: Vec3| [v.x, v.y, v.z].map(|x| (x * 1e4).round() as i64);
    let mut out: Vec<(u64, Vec<i64>)> = st.sketches.iter().map(|k| (k.feature, [r(k.plane.origin), r(k.plane.x), r(k.plane.y)].concat())).collect();
    for c in &st.construct {
        let v = match &c.geom {
            G::Plane(p) => {
                let n = p.x.cross(p.y).normalized().unwrap();
                let n = if n.x + n.y * 1e-3 + n.z * 1e-6 < 0.0 { n * -1.0 } else { n };
                [r(n), r(n * p.origin.dot(n))].concat()
            }
            G::Axis { origin, dir } => {
                let d = dir.normalized().unwrap();
                let d = if d.x + d.y * 1e-3 + d.z * 1e-6 < 0.0 { d * -1.0 } else { d };
                [r(d), r(*origin - d * origin.dot(d))].concat()
            }
            G::Point(p) => r(*p).to_vec(),
        };
        out.push((c.feature, v));
    }
    out.sort();
    out
}

fn transform(s: &Session, occ: u64) -> Mat {
    s.doc.occurrences.iter().find(|o| o.id == occ).unwrap().transform
}

fn close(a: Vec3, b: Vec3) {
    assert!(a.dist(b) < 1e-6, "{a:?} != {b:?}");
}

/// A part with a body, a sketch on its XZ plane, a revolve about its Z axis and construction
/// geometry on its origin planes, placed away from the design's origin and turned.
fn part(s: &mut Session) -> (u64, u64) {
    let (comp, occ) = created(s, "Part");
    run(s, "occurrence.move", json!({"occurrence": occ, "translate": [40, 10, 0], "axis": [0, 0, 1], "angle": "30 deg"}));
    run(s, "solid.box", json!({"length": 10, "width": 20, "height": 5, "corner": [2, 3, 0], "body_name": "Block"}));
    run(s, "sketch.create", json!({"plane": "XZ"}));
    run(s, "sketch.rectangle.two_point", json!({"p0": [20, 0], "p1": [25, 8]}));
    run(s, "sketch.finish", json!({}));
    run(s, "solid.revolve", json!({"axis": "Z", "angle": "90 deg"}));
    run(s, "construct.plane.offset", json!({"base": "XY", "offset": 7, "name": "Shelf"}));
    run(s, "construct.axis.two_planes", json!({"a": "XZ", "b": "YZ", "name": "Spine"}));
    (comp, occ)
}

#[test]
fn moving_the_origin_keeps_everything_where_it_is() {
    let mut s = Session::default();
    let (comp, occ) = part(&mut s);
    let (bodies, items) = (boxes(&s), frames(&s));
    let before = s.doc.clone();
    let r = run(&mut s, "component.set_origin", json!({"component": comp, "point": [100, 50, 3]}));
    assert_eq!(r["unchanged"], false, "{r}");
    // The placement now puts the component's origin at the point, its axes as they were.
    let t = transform(&s, occ);
    close(apply_point(&t, Vec3::ZERO), Vec3::new(100.0, 50.0, 3.0));
    let was = before.occurrences.iter().find(|o| o.id == occ).unwrap().transform;
    close(apply_vector(&t, Vec3::X), apply_vector(&was, Vec3::X));
    assert_eq!(boxes(&s), bodies);
    assert_eq!(frames(&s), items);
    // One undo step back to exactly what it was; redo again.
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(s.doc, before);
    run(&mut s, "edit.redo", json!({}));
    close(apply_point(&transform(&s, occ), Vec3::ZERO), Vec3::new(100.0, 50.0, 3.0));
    assert_eq!(boxes(&s), bodies);
}

#[test]
fn turned_axes_keep_everything_where_it_is_and_survive_saving() {
    let mut s = Session::default();
    let (comp, occ) = part(&mut s);
    let (bodies, items) = (boxes(&s), frames(&s));
    // Z along the world's -Y, X along the world's Z.
    let r = run(&mut s, "component.set_origin", json!({"component": comp, "frame": {"origin": [0, 0, 0], "x": [0, 0, 1], "z": [0, -1, 0]}}));
    let t = transform(&s, occ);
    close(apply_vector(&t, Vec3::Z), Vec3::new(0.0, -1.0, 0.0));
    close(apply_vector(&t, Vec3::X), Vec3::new(0.0, 0.0, 1.0));
    // Y = Z × X.
    close(apply_vector(&t, Vec3::Y), Vec3::new(-1.0, 0.0, 0.0));
    assert!((r["y"][0].as_f64().unwrap() + 1.0).abs() < 1e-9, "{r}");
    assert_eq!(boxes(&s), bodies);
    assert_eq!(frames(&s), items);
    // In component space, the origin where it is changes nothing.
    let again = run(&mut s, "component.set_origin", json!({"component": comp, "point": [0, 0, 0], "space": "component"}));
    assert_eq!(again["unchanged"], true, "{again}");

    let dir = std::env::temp_dir().join(format!("solidcraft-set-origin-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("turned.solidcraft").to_string_lossy().to_string();
    run(&mut s, "file.save_as", json!({"path": path}));
    let mut opened = Session::default();
    run(&mut opened, "doc.open", json!({"path": path}));
    assert_eq!(boxes(&opened), bodies);
    assert_eq!(frames(&opened), items);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn every_placement_and_a_rigid_group_keep_their_place() {
    let mut s = Session::default();
    let (comp, occ) = created(&mut s, "Leg");
    run(&mut s, "solid.box", json!({"length": 4, "width": 4, "height": 30, "body_name": "Post"}));
    run(&mut s, "component.activate", json!({"component": 0}));
    let copy = run(&mut s, "occurrence.copy", json!({"component": comp, "translate": [50, 0, 0]}))["occurrence"].as_u64().unwrap();
    let (_, top) = created(&mut s, "Top");
    run(&mut s, "solid.box", json!({"length": 60, "width": 10, "height": 2, "corner": [0, 0, 30], "body_name": "Slab"}));
    run(&mut s, "component.activate", json!({"component": 0}));
    run(&mut s, "joint.rigid_group", json!({"occurrences": [occ, top]}));
    let bodies = boxes(&s);
    let (first, second) = (transform(&s, occ), transform(&s, copy));
    // The post's top centre, read in the second placement.
    let r = run(&mut s, "component.set_origin", json!({"component": comp, "path": [copy], "point": [52, 2, 30]}));
    assert_eq!(r["placements"].as_array().unwrap().len(), 2, "{r}");
    assert_eq!(boxes(&s), bodies);
    // Both placements moved by the same offset in the leg's own frame: to (2, 2, 30).
    let offset: Mat = serde_json::from_value(r["offset"].clone()).unwrap();
    close(apply_point(&offset, Vec3::ZERO), Vec3::new(2.0, 2.0, 30.0));
    for (occurrence, was) in [(occ, first), (copy, second)] {
        close(apply_point(&transform(&s, occurrence), Vec3::ZERO), apply_point(&mat_mul(&was, &offset), Vec3::ZERO));
    }
    // The group still holds them where they are: a solve moves nothing.
    s.refresh();
    assert_eq!(boxes(&s), bodies);
}

#[test]
fn an_inserted_mesh_gets_its_origin_at_the_bottom_of_its_box() {
    let dir = std::env::temp_dir().join(format!("solidcraft-set-origin-mesh-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    // A box 10 x 20 x 30 mm whose file coordinates start at (100, 200, 300).
    let mut maker = Session::default();
    run(&mut maker, "solid.box", json!({"length": 10, "width": 20, "height": 30, "corner": [100, 200, 300]}));
    let stl = dir.join("chair.stl").to_string_lossy().to_string();
    run(&mut maker, "file.export", json!({"path": stl}));

    let mut s = Session::default();
    let r = run(&mut s, "file.insert_mesh", json!({"path": stl}));
    let (comp, occ) = (r["component"].as_u64().unwrap(), r["occurrence"].as_u64().unwrap());
    let bodies = boxes(&s);
    run(&mut s, "component.set_origin", json!({"component": comp, "point": "bottom"}));
    assert_eq!(boxes(&s), bodies);
    close(apply_point(&transform(&s, occ), Vec3::ZERO), Vec3::new(105.0, 210.0, 300.0));
    // The mesh is now written about its new origin: centred on it, standing on it.
    let local = s.model.state().bodies[0].body.outline_bounds();
    close(local.min, Vec3::new(-5.0, -10.0, 0.0));
    close(local.max, Vec3::new(5.0, 10.0, 30.0));
    // So placed at the design's origin it stands on the ground, centred.
    run(
        &mut s,
        "occurrence.move",
        json!({"occurrence": occ, "matrix": [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0], "base_transform": solidcraft_doc::IDENTITY, "move_grounded": true}),
    );
    let placed = boxes(&s);
    assert_eq!(placed[0].1, [-50000, -100000, 0, 50000, 100000, 300000]);

    let path = dir.join("chair.solidcraft").to_string_lossy().to_string();
    run(&mut s, "file.save_as", json!({"path": path}));
    let mut opened = Session::default();
    run(&mut opened, "doc.open", json!({"path": path}));
    assert_eq!(boxes(&opened), placed);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_move_feature_inside_keeps_its_result() {
    let mut s = Session::default();
    let (comp, _) = created(&mut s, "Turned");
    run(&mut s, "solid.box", json!({"length": 10, "width": 4, "height": 2, "body_name": "Bar"}));
    run(&mut s, "solid.move", json!({"bodies": ["Bar"], "translate": [5, 0, 0], "axis": [0, 0, 1], "angle": "45 deg"}));
    let bodies = boxes(&s);
    run(&mut s, "component.set_origin", json!({"component": comp, "frame": {"origin": [7, -3, 1], "z": [1, 0, 0]}}));
    assert_eq!(boxes(&s), bodies);
}

#[test]
fn keep_world_false_moves_the_contents_with_the_origin() {
    let mut s = Session::default();
    let (comp, _) = created(&mut s, "Free");
    run(&mut s, "solid.box", json!({"length": 2, "width": 2, "height": 2, "body_name": "Cube"}));
    run(&mut s, "component.set_origin", json!({"component": comp, "point": [10, 0, 0], "keep_world": false}));
    assert_eq!(boxes(&s), vec![("Cube".to_string(), [100000, 0, 0, 120000, 20000, 20000])]);
}

#[test]
fn bad_requests_change_nothing() {
    let mut s = Session::default();
    let (comp, occ) = created(&mut s, "Part");
    run(&mut s, "solid.box", json!({"length": 2, "width": 2, "height": 2}));
    let before = s.doc.clone();
    for p in [
        json!({"component": 0, "point": [1, 2, 3]}),
        json!({"component": comp}),
        json!({"component": comp, "point": [1, 2]}),
        json!({"component": comp, "point": [1, "x", 3]}),
        json!({"component": comp, "point": "middle"}),
        json!({"component": comp, "point": [1, 2, 3], "frame": {"origin": [0, 0, 0]}}),
        json!({"component": comp, "frame": {"x": [1, 0, 0], "z": [2, 0, 0]}}),
        json!({"component": comp, "frame": {"z": [0, 0, 0]}}),
        json!({"component": comp, "frame": {"z": [0, 0, 1], "y": [0, 1, 0]}}),
        json!({"component": comp, "frame": [0, 0, 1]}),
        json!({"component": comp, "point": [1e300, 0, 0]}),
        json!({"component": comp, "point": [1, 2, 3], "space": "parent"}),
        json!({"component": comp, "point": [1, 2, 3], "path": [occ + 1000]}),
        json!({"component": "nothing", "point": [1, 2, 3]}),
    ] {
        assert!(s.execute("component.set_origin", &p).is_err(), "{p} was accepted");
        assert_eq!(s.doc, before, "{p} changed the design");
    }
    // A move in progress must be captured or reverted first.
    run(&mut s, "occurrence.move", json!({"occurrence": occ, "translate": [1, 0, 0], "capture": false}));
    let e = s.execute("component.set_origin", &json!({"component": comp, "point": [1, 2, 3]})).unwrap_err().to_string();
    assert!(e.contains("capture"), "{e}");
}

/// Bodies made at the top and handed to components (`component.from_bodies`), with a sketch on
/// one of them drawn from the top and a cut through it: the hand-over and the attachment follow.
#[test]
fn handed_over_bodies_and_attached_sketches_keep_their_place() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "body_name": "Left"}));
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [30, 0, 0], "body_name": "Right"}));
    let made = run(&mut s, "component.from_bodies", json!({"bodies": ["Left", "Right"]}));
    let comp = s.doc.components.iter().find(|c| c.name == "Left").map(|c| c.id).unwrap_or_else(|| panic!("{made}"));
    run(&mut s, "component.activate", json!({"component": 0}));
    run(&mut s, "sketch.create", json!({"plane": {"face": [5, 5, 10]}}));
    run(&mut s, "sketch.circle.center", json!({"center": [0, 0], "radius": 2}));
    run(&mut s, "sketch.finish", json!({}));
    let (bodies, items) = (boxes(&s), frames(&s));
    run(&mut s, "component.set_origin", json!({"component": comp, "frame": {"origin": [5, 5, 10], "x": [0, 1, 0]}}));
    assert_eq!(boxes(&s), bodies);
    assert_eq!(frames(&s), items);
    run(&mut s, "component.set_origin", json!({"component": comp, "point": [-20, 3, 1]}));
    assert_eq!(boxes(&s), bodies);
    assert_eq!(frames(&s), items);
}
