//! Components as frames: what a component holds moves with its occurrence, and picks in the
//! world land in the component's own frame (user bug #4).

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

fn near(a: Vec3, b: Vec3) -> bool {
    a.dist(b) < 1e-6
}

fn world_bbox(s: &Session, body: &str) -> (Vec3, Vec3) {
    let w = s.world_state().unwrap();
    let b = w.body(body).unwrap_or_else(|| panic!("no body {body}")).mesh().bounds();
    (b.min, b.max)
}

fn sketch_point_world(s: &mut Session, sketch: &str, point: &str) -> Vec3 {
    let si = run(s, "sketch.inspect", json!({ "sketch": sketch }));
    let p = si["points"].as_array().unwrap().iter().find(|p| p["id"] == point).unwrap()["world"].clone();
    Vec3::new(p[0].as_f64().unwrap(), p[1].as_f64().unwrap(), p[2].as_f64().unwrap())
}

fn volume(s: &mut Session) -> f64 {
    run(s, "inspect.measure", json!({}))["total"]["volume_mm3"].as_f64().unwrap()
}

/// A component "Part" with a 30 x 20 x 10 block (sketch "Base" on XY, "Top" on its top face)
/// and a construction plane, moved 50 mm along X.
fn moved_part() -> Session {
    let mut s = Session::default();
    run(&mut s, "component.create", json!({"name": "Part", "grounded": false}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Base"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [30, 20]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"distance": 10}));
    run(&mut s, "sketch.create", json!({"plane": {"face": [15, 10, 10]}, "name": "Top"}));
    run(&mut s, "sketch.circle.center", json!({"center": [15, 10], "radius": 3}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "construct.plane.offset", json!({"base": "XY", "offset": 25, "name": "Shelf"}));
    run(&mut s, "occurrence.move", json!({"occurrence": "Part:1", "translate": [50, 0, 0]}));
    s
}

/// (a) Sketches and construction planes move with their component's occurrence.
#[test]
fn sketches_and_planes_move_with_the_component() {
    let mut s = moved_part();
    let (lo, hi) = world_bbox(&s, "Body1");
    assert!(near(lo, Vec3::new(50.0, 0.0, 0.0)) && near(hi, Vec3::new(80.0, 20.0, 10.0)), "{lo:?} {hi:?}");
    assert!(near(sketch_point_world(&mut s, "Base", "p1"), Vec3::new(50.0, 0.0, 0.0)));
    let c = sketch_point_world(&mut s, "Top", "p1");
    assert!((c.z - 10.0).abs() < 1e-6 && c.x >= 50.0 - 1e-6, "{c:?}");
    let shelf = crate::view::construction_planes(&s).into_iter().find(|(_, n, _)| n == "Shelf").unwrap().2;
    assert!(near(shelf.origin, Vec3::new(50.0, 0.0, 25.0)), "{:?}", shelf.origin);
    // A turn about Z carries them too.
    run(&mut s, "occurrence.move", json!({"occurrence": "Part:1", "axis": [0, 0, 1], "angle": "90 deg", "origin": [50, 0, 0]}));
    assert!(near(sketch_point_world(&mut s, "Base", "p3"), Vec3::new(30.0, 30.0, 0.0)), "{:?}", sketch_point_world(&mut s, "Base", "p3"));
}

/// (b) Picks on a moved component, given in the world, land on its geometry: fillet edges, a
/// sketch on a face, a hole, a shell; new geometry of the active component goes where asked.
#[test]
fn world_picks_map_into_the_component() {
    let mut s = moved_part();
    let v0 = volume(&mut s);
    assert!((v0 - 6000.0).abs() < 1e-6, "{v0}"); // 30 x 20 x 10
    // The edge along X at the top front: world (65, 0, 10).
    run(&mut s, "solid.fillet", json!({"edges": [[65, 0, 10]], "radius": 2}));
    let filleted = volume(&mut s);
    // A round of radius 2 along the 30 mm edge removes r²(1 − π/4) of section along its length.
    let round = 4.0 * (1.0 - std::f64::consts::PI / 4.0) * 30.0;
    assert!((v0 - filleted - round).abs() < 4e-7 * v0, "the fillet cut {} against {round}", v0 - filleted);
    // The modified face has no certified lineage; explicitly freeze its picked plane.
    // Its world pick still maps into the moved component exactly once.
    run(&mut s, "sketch.create", json!({"plane": {"face": [70, 10, 10],"fixed":true}, "name": "Pad"}));
    run(&mut s, "sketch.circle.center", json!({"center": [20, 10], "radius": 2, "id": "PadCircle"}));
    let sketch = run(&mut s, "sketch.inspect", json!({"sketch": "Pad"}));
    let center = sketch["curves"].as_array().unwrap().iter().find(|c| c["id"] == "PadCircle").unwrap()["center"].as_str().unwrap().to_string();
    run(&mut s, "sketch.finish", json!({}));
    // A projected boundary's first point is an enumeration detail; check the circle
    // that was actually authored in the moved component's local sketch coordinates.
    let pad = sketch_point_world(&mut s, "Pad", &center);
    assert!(near(pad, Vec3::new(70.0, 10.0, 10.0)), "{pad:?}");
    let v1 = volume(&mut s);
    let h = run(&mut s, "solid.hole", json!({"position": [60, 10, 10], "diameter": 4, "depth": 5}));
    // A simple (flat-bottomed) hole: a cylinder of radius 2, 5 deep, clear of the round at y < 2.
    let bore = std::f64::consts::PI * 4.0 * 5.0;
    let v2 = volume(&mut s);
    assert!((v1 - v2 - bore).abs() < 4e-7 * v1, "the hole cut {} against {bore}: {h}", v1 - v2);
    // A box made at world (100, 0, 0) in the active (moved) component shows there.
    run(&mut s, "solid.box", json!({"length": 5, "width": 5, "height": 5, "corner": [100, 0, 0], "operation": "new"}));
    let last = s.model.state().bodies.last().map(|b| b.name.clone()).unwrap();
    let (lo, _) = world_bbox(&s, &last);
    assert!(near(lo, Vec3::new(100.0, 0.0, 0.0)), "{lo:?}");
}

/// (b) A pasted instance's pick fillets the component (every instance).
#[test]
fn picks_on_an_instance_map_through_its_placement() {
    let mut s = Session::default();
    run(&mut s, "component.create", json!({"name": "Pin"}));
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    run(&mut s, "occurrence.copy", json!({"component": "Pin", "translate": [0, 40, 0]}));
    let v0 = volume(&mut s);
    // An edge of the second instance only (world y = 40).
    run(&mut s, "solid.fillet", json!({"edges": [[5, 40, 10]], "radius": 2}));
    let v1 = volume(&mut s);
    // Two 10 mm cubes; the round of radius 2 along one 10 mm edge of the component removes
    // r²(1 − π/4)·10 from every instance, the picked one and the other.
    assert!((v0 - 2000.0).abs() < 1e-6, "{v0}");
    let round = 4.0 * (1.0 - std::f64::consts::PI / 4.0) * 10.0;
    assert!((v0 - v1 - 2.0 * round).abs() < 4e-7 * v0, "{v0} {v1}");
}

/// (c) Moving a body into a moved component keeps it where it is in the world.
#[test]
fn moving_bodies_between_components_keeps_their_place() {
    let mut s = moved_part();
    run(&mut s, "component.activate", json!({"component": "root"}));
    run(&mut s, "solid.box", json!({"length": 4, "width": 4, "height": 4, "corner": [0, 40, 0], "body_name": "Loose"}));
    let before = world_bbox(&s, "Loose");
    run(&mut s, "component.move_bodies", json!({"bodies": ["Loose"], "component": "Part"}));
    let after = world_bbox(&s, "Loose");
    assert!(near(before.0, after.0) && near(before.1, after.1), "{before:?} {after:?}");
    // It moves with the component from now on, and a pick on it still lands.
    run(&mut s, "occurrence.move", json!({"occurrence": "Part:1", "translate": [0, 0, 10]}));
    let moved = world_bbox(&s, "Loose");
    assert!(near(moved.0, before.0 + Vec3::new(0.0, 0.0, 10.0)), "{moved:?}");
    let v0 = volume(&mut s);
    run(&mut s, "solid.fillet", json!({"edges": [[2, 40, 14]], "radius": 1}));
    assert!(volume(&mut s) < v0 - 0.1);
    // Back to the root: still in place.
    run(&mut s, "component.move_bodies", json!({"bodies": ["Loose"], "component": "root"}));
    let back = world_bbox(&s, "Loose");
    assert!(near(back.0, moved.0) && near(back.1, moved.1), "{back:?} {moved:?}");
}

/// (d) A sketch on a body's face follows the body when a Move feature moves it.
#[test]
fn sketch_on_a_face_follows_a_moved_body() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 20, "width": 20, "height": 10}));
    run(&mut s, "sketch.create", json!({"plane": {"face": [10, 10, 10]}, "name": "OnTop"}));
    run(&mut s, "sketch.circle.center", json!({"center": [10, 10], "radius": 2}));
    run(&mut s, "sketch.finish", json!({}));
    // The move goes before the sketch in the timeline: the face the sketch is on moves up.
    run(&mut s, "timeline.roll_to", json!({"position": 1}));
    run(&mut s, "solid.move", json!({"bodies": ["Body1"], "translate": [0, 0, 15]}));
    run(&mut s, "timeline.roll_to", json!({}));
    let c = sketch_point_world(&mut s, "OnTop", "p1");
    assert!((c.z - 25.0).abs() < 1e-6, "{c:?}");
}

/// A 1 x 1 PNG for canvases.
const PNG: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==";

/// Canvases belong to the active component and move with its occurrence; one put on a moved
/// component's face from the root sits on the face (and stays the root's).
#[test]
fn canvases_move_with_their_component() {
    let mut s = moved_part();
    let part = s.active_component;
    assert_ne!(part, 0);
    run(&mut s, "canvas.insert", json!({"data": PNG, "plane": "XY", "name": "Plan"}));
    // From the root, on the moved block's top face (world).
    run(&mut s, "component.activate", json!({"component": "root"}));
    run(&mut s, "canvas.insert", json!({"data": PNG, "plane": {"face": [65, 10, 10]}, "name": "Lid", "width": 10}));
    run(&mut s, "canvas.insert", json!({"data": PNG, "plane": "XZ", "name": "Root"}));
    let (vals, _) = s.doc.param_values();
    let world = |s: &Session, name: &str| {
        let c = s.doc.canvases.iter().find(|c| c.name == name).unwrap();
        (c.component, s.doc.canvas_plane(&vals, c).unwrap())
    };
    let (c, p) = world(&s, "Plan");
    assert_eq!(c, part);
    assert!(near(p.origin, Vec3::new(50.0, 0.0, 0.0)), "{:?}", p.origin);
    let (c, p) = world(&s, "Lid");
    assert_eq!(c, 0, "the active component's");
    let lid = s.doc.canvases.iter().find(|c| c.name == "Lid").unwrap();
    let centre = p.to_world(lid.center);
    assert!(near(centre, Vec3::new(65.0, 10.0, 10.0)), "{centre:?}");
    assert_eq!(world(&s, "Root").0, 0);
    // Moving the occurrence carries its canvases, not the root's.
    run(&mut s, "occurrence.move", json!({"occurrence": "Part:1", "translate": [0, 40, 0]}));
    let (vals, _) = s.doc.param_values();
    let plan = s.doc.canvases.iter().find(|c| c.name == "Plan").unwrap();
    assert!(near(s.doc.canvas_plane(&vals, plan).unwrap().origin, Vec3::new(50.0, 40.0, 0.0)));
    let lid = s.doc.canvases.iter().find(|c| c.name == "Lid").unwrap();
    let centre = s.doc.canvas_plane(&vals, lid).unwrap().to_world(lid.center);
    assert!(near(centre, Vec3::new(65.0, 10.0, 10.0)), "{centre:?}");
    let root = s.doc.canvases.iter().find(|c| c.name == "Root").unwrap();
    assert!(near(s.doc.canvas_plane(&vals, root).unwrap().origin, Vec3::ZERO));
    // The component survives a save and reopen.
    let back = solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap();
    assert_eq!(back.canvases, s.doc.canvases);
}

/// World points of a sketch's curves' points (sorted).
fn sketch_world_points(s: &mut Session, sketch: &str) -> Vec<Vec3> {
    let si = run(s, "sketch.inspect", json!({ "sketch": sketch }));
    si["points"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| {
            let w = &p["world"];
            Vec3::new(w[0].as_f64().unwrap(), w[1].as_f64().unwrap(), w[2].as_f64().unwrap())
        })
        .collect()
}

/// A block "C" (40 x 30 x 20) in its own component, the root active again.
fn block_in_component() -> Session {
    let mut s = Session::default();
    run(&mut s, "component.create", json!({"name": "C", "grounded": false}));
    run(&mut s, "solid.box", json!({"length": 40, "width": 30, "height": 20}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    s
}

#[test]
fn move_expression_distances_follow_world_axes_in_a_turned_component() {
    for translate in [json!([0, -4, 0]), json!(["0 mm", "-4 mm", "0 mm"]), json!([0, "-4", 0])] {
        let mut s = block_in_component();
        run(&mut s, "occurrence.move", json!({"occurrence": "C:1", "axis": [1, 0, 0], "angle": "90 deg", "translate": [50, 10, 20]}));
        let before = world_bbox(&s, "Body1");
        run(&mut s, "solid.move", json!({"bodies": ["Body1"], "translate": translate}));
        let after = world_bbox(&s, "Body1");
        let shift = Vec3::new(0.0, -4.0, 0.0);
        assert!(near(after.0, before.0 + shift) && near(after.1, before.1 + shift), "{translate}: {before:?} -> {after:?}");
        assert!((volume(&mut s) - 24_000.0).abs() < 1e-6);
    }
}

#[test]
fn move_expression_distances_keep_parameters_through_rebuild_and_reopen() {
    for copy in [false, true] {
        let mut s = block_in_component();
        run(&mut s, "parameters.add", json!({"name": "travel", "expression": "4 mm"}));
        // An oblique turn mixes all three coordinates; the placement's translation must
        // never contribute to the displacement vector.
        run(&mut s, "occurrence.move", json!({"occurrence": "C:1", "axis": [1, 2, 3], "angle": "37 deg", "translate": [50, 10, 20]}));
        run(&mut s, "component.activate", json!({"component": "C"}));
        let before = world_bbox(&s, "Body1");
        run(&mut s, "solid.move", json!({"bodies": ["Body1"], "translate": [2, "-travel", "1 cm"], "copy": copy}));
        let moved = s.model.state().bodies.last().unwrap().name.clone();
        let assert_shift = |s: &Session, travel: f64| {
            let after = world_bbox(s, &moved);
            let shift = Vec3::new(2.0, -travel, 10.0);
            assert!(near(after.0, before.0 + shift) && near(after.1, before.1 + shift), "copy={copy}, travel={travel}: {before:?} -> {after:?}");
            if copy {
                let original = world_bbox(s, "Body1");
                assert!(near(original.0, before.0) && near(original.1, before.1));
            }
        };
        assert_shift(&s, 4.0);
        run(&mut s, "parameters.change", json!({"name": "travel", "expression": "8 mm"}));
        assert_shift(&s, 8.0);
        let mut reopened = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
        assert_shift(&reopened, 8.0);
        run(&mut reopened, "parameters.change", json!({"name": "travel", "expression": "12 mm"}));
        assert_shift(&reopened, 12.0);
        assert!((volume(&mut reopened) - if copy { 48_000.0 } else { 24_000.0 }).abs() < 1e-6);
    }
}

/// One `solid.move` of bodies placed in differently turned frames (a block in the root and a
/// screw inserted lying down) moves and turns each of them in the world, whichever is listed
/// first (#9660).
#[test]
fn one_move_of_bodies_in_differently_turned_frames_moves_each_in_the_world() {
    let corners = |(lo, hi): (Vec3, Vec3)| {
        let mut out = Vec::new();
        for x in [lo.x, hi.x] {
            for y in [lo.y, hi.y] {
                for z in [lo.z, hi.z] {
                    out.push(Vec3::new(x, y, z));
                }
            }
        }
        out
    };
    let bounds = |points: Vec<Vec3>| {
        let mut lo = Vec3::new(f64::INFINITY, f64::INFINITY, f64::INFINITY);
        let mut hi = Vec3::new(f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
        for p in points {
            lo = Vec3::new(lo.x.min(p.x), lo.y.min(p.y), lo.z.min(p.z));
            hi = Vec3::new(hi.x.max(p.x), hi.y.max(p.y), hi.z.max(p.z));
        }
        (lo, hi)
    };
    let close = |a: (Vec3, Vec3), b: (Vec3, Vec3)| a.0.dist(b.0) < 1e-4 && a.1.dist(b.1) < 1e-4;
    // The quarter turn is about the world z axis, which passes through the screw's own origin
    // (where it was inserted): a Move feature turns about that origin.
    let moves: [(Value, &dyn Fn(Vec3) -> Vec3); 3] = [
        (json!({"translate": [0, 0, 3]}), &|p| p + Vec3::new(0.0, 0.0, 3.0)),
        (json!({"translate": [0, "2 mm", 0]}), &|p| p + Vec3::new(0.0, 2.0, 0.0)),
        (json!({"axis": [0, 0, 1], "angle": "90 deg"}), &|p| Vec3::new(-p.y, p.x, p.z)),
    ];
    for direction in [[1, 0, 0], [0, 0, -1], [0, 1, 0]] {
        for order in [["Block", "Body1"], ["Body1", "Block"]] {
            for (params, expected) in &moves {
                let mut s = Session::default();
                run(&mut s, "solid.box", json!({"length": 30, "width": 30, "height": 10, "center": [0, 0, 5], "body_name": "Block"}));
                let screw = json!({"family": "socket_head_cap_screw", "size": "M3", "length": 12, "point": [0, 0, 10], "direction": direction, "name": "screw"});
                run(&mut s, "parts.insert", screw);
                let before: Vec<_> = order.iter().map(|b| world_bbox(&s, b)).collect();
                let mut p = params.clone();
                p["bodies"] = json!(order);
                run(&mut s, "solid.move", p.clone());
                for (body, was) in order.iter().zip(&before) {
                    let want = bounds(corners(*was).into_iter().map(expected).collect());
                    let got = world_bbox(&s, body);
                    assert!(close(got, want), "{body} after {p} (screw along {direction:?}): {got:?}, want {want:?}");
                }
                // One step: a single undo puts both back.
                run(&mut s, "edit.undo", json!({}));
                for (body, was) in order.iter().zip(&before) {
                    assert!(close(world_bbox(&s, body), *was), "{body} after undoing {p}");
                }
            }
        }
    }
}

/// Edit Feature passes a primitive's placement through as saved, so changing the radius of
/// Leaf B's rod in the Hinge Assembly leaves the leaf where it was (#2926).
#[test]
fn editing_a_primitive_in_a_turned_component_keeps_its_placement() {
    let sample = crate::sample::samples().into_iter().find(|x| x.name == "Hinge Assembly").unwrap();
    let mut s = Session::default();
    s.run_script(&(sample.script)()).unwrap();
    let (plate_lo, plate_hi) = world_bbox(&s, "Body3");
    let (rod_lo, rod_hi) = world_bbox(&s, "Body4");
    // What the dialog sends: the saved definition, with the new radius.
    let definition = run(&mut s, "timeline.inspect", json!({"feature": "Cylinder2"}))["definition"].clone();
    let params = json!({"base": definition["base"], "axis": definition["axis"], "radius": "6", "height": "40", "operation": "new_body"});
    run(&mut s, "timeline.redefine", json!({"feature": "Cylinder2", "command": "solid.cylinder", "params": params}));
    let after = run(&mut s, "timeline.inspect", json!({"feature": "Cylinder2"}))["definition"].clone();
    assert_eq!((&after["base"], &after["axis"]), (&definition["base"], &definition["axis"]));
    let (lo, hi) = world_bbox(&s, "Body3");
    assert!(near(lo, plate_lo) && near(hi, plate_hi), "the leaf moved: {lo:?} {hi:?}");
    // The rod grows about its own axis (its mesh is faceted, so its size is close, not exact).
    let (lo, hi) = world_bbox(&s, "Body4");
    assert!(((lo + hi) * 0.5).dist((rod_lo + rod_hi) * 0.5) < 1e-3, "the rod moved: {lo:?} {hi:?}");
    let (size, before) = (hi - lo, rod_hi - rod_lo);
    assert!((size.x - before.x).abs() < 1e-6 && (size.y - 12.0).abs() < 0.05 && (size.z - 12.0).abs() < 0.05, "{size:?}");
}

/// Edit Feature shows an explicit revolve axis with its origin evaluated; sending that back
/// untouched keeps the saved expressions, for a solid and a surface revolve alike.
#[test]
fn an_untouched_revolve_axis_keeps_its_origin_expressions_through_redefine() {
    for (command, name) in [("solid.revolve", "Turn"), ("surface.revolve", "Shell")] {
        let mut s = Session::default();
        run(&mut s, "parameters.add", json!({"name": "W", "expression": "40 mm"}));
        run(&mut s, "sketch.create", json!({"name": "Profile", "plane": "XY"}));
        run(&mut s, "sketch.rectangle.two_point", json!({"p0": [5, 0], "p1": [10, 5]}));
        run(&mut s, "sketch.finish", json!({}));
        let axis = json!({"origin": ["W / 2", 0, 0], "dir": [0, 1, 0]});
        let mut made = json!({"sketch": "Profile", "axis": axis, "angle": "90 deg", "name": name});
        if command == "solid.revolve" {
            made["operation"] = json!("new");
        }
        run(&mut s, command, made);
        // The origin's coordinates as text (a saved 0 may read back as 0 or 0.0).
        let saved = |s: &mut Session| -> Vec<String> {
            let axis = run(s, "timeline.inspect", json!({"feature": name}))["definition"]["axis"].clone();
            axis["origin"].as_array().unwrap().iter().map(|c| c.as_f64().map_or_else(|| c.as_str().unwrap().to_string(), |x| x.to_string())).collect()
        };
        let original = saved(&mut s);
        assert_eq!(original, ["W / 2", "0", "0"]);
        // What the dialog sends back untouched: the origin as numbers.
        let mut params = json!({"sketch": "Profile", "axis": {"origin": [20.0, 0.0, 0.0], "dir": [0.0, 1.0, 0.0]}, "angle": "45 deg"});
        if command == "solid.revolve" {
            params["operation"] = json!("new");
        }
        run(&mut s, "timeline.redefine", json!({"feature": name, "command": command, "params": params}));
        assert_eq!(saved(&mut s), original, "{command}");
        // Only the direction changed: the origin keeps its expressions.
        let mut turned = params.clone();
        turned["axis"]["dir"] = json!([0.0, -1.0, 0.0]);
        run(&mut s, "timeline.redefine", json!({"feature": name, "command": command, "params": turned}));
        assert_eq!(saved(&mut s), original, "{command}");
        // A changed origin is taken as given.
        params["axis"]["origin"] = json!([21.0, 0.0, 0.0]);
        run(&mut s, "timeline.redefine", json!({"feature": name, "command": command, "params": params}));
        assert_eq!(saved(&mut s), ["21", "0", "0"], "{command}");
    }
}

/// A surface revolve's `{origin, dir}` axis is a world axis in a moved or turned component, as a
/// solid revolve's is, when the revolve is made and when its axis is changed in Edit Feature.
#[test]
fn a_surface_revolve_axis_maps_into_a_moved_component() {
    let made = |command: &str, turned: bool| {
        let mut s = moved_part();
        if turned {
            run(&mut s, "occurrence.move", json!({"occurrence": "Part:1", "axis": [0, 0, 1], "angle": "90 deg", "origin": [50, 0, 0]}));
        }
        run(&mut s, "sketch.create", json!({"plane": "XZ", "name": "Ring"}));
        run(&mut s, "sketch.rectangle.two_point", json!({"p0": [5, 0], "p1": [10, 5]}));
        run(&mut s, "sketch.finish", json!({}));
        let mut p = json!({"sketch": "Ring", "axis": {"origin": [50, 0, 0], "dir": [0, 0, 1]}, "name": "Turn"});
        if command == "solid.revolve" {
            p["operation"] = json!("new");
        }
        run(&mut s, command, p.clone());
        let body = s.model.state().bodies.last().map(|b| b.name.clone()).unwrap();
        let first = world_bbox(&s, &body);
        // Edit Feature with a new axis, also given in the world: 2 mm along the sketch's X, which
        // the quarter turn lays along the world's Y.
        p["axis"]["origin"] = if turned { json!([50, 2, 0]) } else { json!([52, 0, 0]) };
        run(&mut s, "timeline.redefine", json!({"feature": "Turn", "command": command, "params": p}));
        let body = s.model.state().bodies.last().map(|b| b.name.clone()).unwrap();
        (first, world_bbox(&s, &body))
    };
    for turned in [false, true] {
        let (solid, solid_edit) = made("solid.revolve", turned);
        let (surface, surface_edit) = made("surface.revolve", turned);
        assert!(near(solid.0, surface.0) && near(solid.1, surface.1), "turned {turned}: {solid:?} against {surface:?}");
        assert!(
            near(solid_edit.0, surface_edit.0) && near(solid_edit.1, surface_edit.1),
            "turned {turned}, edited: {solid_edit:?} against {surface_edit:?}"
        );
    }
}

/// A surface revolve about a picked edge of a moved component turns about that edge where it is
/// shown, as one about the same axis given as `{origin, dir}` does.
#[test]
fn a_surface_revolve_about_a_picked_edge_of_a_moved_component() {
    let made = |axis: Value| {
        let mut s = moved_part();
        run(&mut s, "sketch.create", json!({"plane": "XZ", "name": "Ring"}));
        run(&mut s, "sketch.rectangle.two_point", json!({"p0": [5, 0], "p1": [10, 5]}));
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "surface.revolve", json!({"sketch": "Ring", "axis": axis, "name": "Turn"}));
        let body = s.model.state().bodies.last().map(|b| b.name.clone()).unwrap();
        world_bbox(&s, &body)
    };
    // The block's vertical edge at its corner, shown at world x = 50.
    let edge = made(json!({"edge": [50, 0, 5]}));
    let line = made(json!({"origin": [50, 0, 0], "dir": [0, 0, 1]}));
    assert!(near(edge.0, line.0) && near(edge.1, line.1), "{edge:?} against {line:?}");
}

/// A revolve axis given only by its direction passes through the world origin in a moved
/// component, as one with `origin: [0, 0, 0]` does: solid and surface revolves, made and edited.
#[test]
fn an_axis_without_an_origin_passes_through_the_world_origin() {
    let made = |command: &str, axis: Value| {
        let mut s = moved_part();
        run(&mut s, "sketch.create", json!({"plane": "XZ", "name": "Ring"}));
        run(&mut s, "sketch.rectangle.two_point", json!({"p0": [5, 0], "p1": [10, 5]}));
        run(&mut s, "sketch.finish", json!({}));
        let mut p = json!({"sketch": "Ring", "axis": axis, "name": "Turn"});
        if command == "solid.revolve" {
            p["operation"] = json!("new");
        }
        run(&mut s, command, p.clone());
        let body = s.model.state().bodies.last().map(|b| b.name.clone()).unwrap();
        let first = world_bbox(&s, &body);
        // Edit Feature with the same axis given again.
        run(&mut s, "timeline.redefine", json!({"feature": "Turn", "command": command, "params": p}));
        let body = s.model.state().bodies.last().map(|b| b.name.clone()).unwrap();
        (first, world_bbox(&s, &body))
    };
    for command in ["solid.revolve", "surface.revolve"] {
        let (given, given_edit) = made(command, json!({"origin": [0, 0, 0], "dir": [0, 0, 1]}));
        let (omitted, omitted_edit) = made(command, json!({"dir": [0, 0, 1]}));
        assert!(near(given.0, omitted.0) && near(given.1, omitted.1), "{command}: {given:?} against {omitted:?}");
        assert!(
            near(given_edit.0, omitted_edit.0) && near(given_edit.1, omitted_edit.1),
            "{command}, edited: {given_edit:?} against {omitted_edit:?}"
        );
    }
}

/// A new placement given to `timeline.redefine` is still a world point.
#[test]
fn a_new_primitive_placement_is_still_mapped_from_the_world() {
    let mut s = moved_part();
    run(&mut s, "component.activate", json!({"component": "Part"}));
    run(&mut s, "solid.cylinder", json!({"base": [60, 10, 10], "radius": 2, "height": 5, "operation": "new", "name": "Peg"}));
    let base = |s: &mut Session| run(s, "timeline.inspect", json!({"feature": "Peg"}))["definition"]["base"].clone();
    assert_eq!(base(&mut s), json!([10.0, 10.0, 10.0]));
    let redefine = |s: &mut Session, at: Value| {
        run(
            s,
            "timeline.redefine",
            json!({"feature": "Peg", "command": "solid.cylinder", "params": {"base": at, "radius": 3, "height": 5, "operation": "new"}}),
        );
    };
    redefine(&mut s, json!([10, 10, 10]));
    assert_eq!(base(&mut s), json!([10.0, 10.0, 10.0]));
    redefine(&mut s, json!([70, 10, 10]));
    assert_eq!(base(&mut s), json!([20.0, 10.0, 10.0]));
}

/// The user's "big square": a sketch made in the root on a moved component's face lies on the
/// face where it is shown, its projected loop on the face's edges (moved and turned), and it
/// follows when the occurrence moves on.
#[test]
fn root_sketch_on_a_moved_components_face() {
    let mut s = block_in_component();
    run(&mut s, "occurrence.move", json!({"occurrence": "C:1", "translate": [100, 0, 0]}));
    let r = run(&mut s, "sketch.create", json!({"plane": {"face": [120, 15, 20]}, "name": "Top"}));
    assert!(!r["projected"].is_null(), "the face's edges are projected: {r}");
    run(&mut s, "sketch.finish", json!({}));
    let pts = sketch_world_points(&mut s, "Top");
    assert!(pts.len() >= 4, "{pts:?}");
    // Every point but the sketch's origin (the model origin on the face plane).
    for p in pts.iter().filter(|p| !near(**p, Vec3::new(0.0, 0.0, 20.0))) {
        assert!((p.z - 20.0).abs() < 1e-6 && p.x > 100.0 - 1e-6 && p.x < 140.0 + 1e-6 && p.y > -1e-6 && p.y < 30.0 + 1e-6, "{p:?}");
    }
    for corner in [Vec3::new(100.0, 0.0, 20.0), Vec3::new(140.0, 30.0, 20.0)] {
        assert!(pts.iter().any(|p| near(*p, corner)), "{corner:?} in {pts:?}");
    }
    assert_eq!(s.doc.find_feature("Top").unwrap().component, 0, "the sketch is the root's");

    // Turned 90 degrees about Z (around the world origin): the top face is now x -30..0, y 0..40.
    let mut s = block_in_component();
    run(&mut s, "occurrence.move", json!({"occurrence": "C:1", "axis": [0, 0, 1], "angle": "90 deg", "origin": [0, 0, 0]}));
    run(&mut s, "sketch.create", json!({"plane": {"face": [-15, 20, 20]}, "name": "Top"}));
    run(&mut s, "sketch.finish", json!({}));
    let pts = sketch_world_points(&mut s, "Top");
    for corner in [Vec3::new(0.0, 0.0, 20.0), Vec3::new(-30.0, 40.0, 20.0)] {
        assert!(pts.iter().any(|p| near(*p, corner)), "{corner:?} in {pts:?}");
    }
    // Projected geometry stays on the face when the occurrence moves on (the link re-resolves).
    run(&mut s, "occurrence.move", json!({"occurrence": "C:1", "translate": [0, 0, 5]}));
    let pts = sketch_world_points(&mut s, "Top");
    assert!(pts.iter().any(|p| near(*p, Vec3::new(-30.0, 40.0, 25.0))), "{pts:?}");
}

/// (#558) With the root active, construction geometry, a hole and Press Pull picked on a moved
/// component land on it where it is shown, and so do picks on a pasted instance.
#[test]
fn root_picks_on_a_moved_component_build_where_it_is_shown() {
    let mut s = moved_part();
    run(&mut s, "component.activate", json!({"component": "root"}));
    // The block spans world x 50…80, y 0…20, z 0…10.
    run(&mut s, "construct.axis.edge", json!({"edge": {"edge": [65, 0, 0]}, "name": "FrontEdge"}));
    run(
        &mut s,
        "construct.plane.three_points",
        json!({"a": {"vertex": [50, 0, 10]}, "b": {"vertex": [80, 0, 10]}, "c": {"vertex": [50, 20, 10]}, "name": "Lid"}),
    );
    let lid = crate::view::construction_planes(&s).into_iter().find(|(_, n, _)| n == "Lid").unwrap().2;
    assert!((lid.origin.z - 10.0).abs() < 1e-6 && lid.normal().z.abs() > 1.0 - 1e-9, "{lid:?}");
    let st = s.model.state();
    let axis = st.construct.iter().find(|c| c.name == "FrontEdge").map(|c| c.geom.clone()).unwrap();
    let solidcraft_doc::construct::ConstructGeom::Axis { origin, dir } = axis else { panic!("{axis:?}") };
    assert!(origin.y.abs() < 1e-6 && origin.z.abs() < 1e-6 && dir.x.abs() > 1.0 - 1e-9, "{origin:?} {dir:?}");
    let v0 = volume(&mut s);
    run(&mut s, "solid.hole", json!({"position": [60, 10, 10], "diameter": 4, "depth": 5}));
    let v1 = volume(&mut s);
    assert!(v1 < v0 - 1.0, "the hole cut the moved block: {v0} {v1}");
    run(&mut s, "solid.press_pull", json!({"face": [80, 10, 5], "distance": 2}));
    assert!(volume(&mut s) > v1 + 1.0, "press pull grew the moved block");
    assert!(s.model.results.iter().all(|r| r.error.is_none()), "{:?}", s.model.results);

    // A pasted instance 40 mm further along y.
    run(&mut s, "occurrence.copy", json!({"component": "Part", "translate": [0, 40, 0]}));
    run(&mut s, "construct.plane.along_path", json!({"path": {"edge": [65, 40, 0]}, "t": 0.5, "name": "Across"}));
    let across = crate::view::construction_planes(&s).into_iter().find(|(_, n, _)| n == "Across").unwrap().2;
    assert!((across.origin.y - 40.0).abs() < 1e-6, "{across:?}");
}

/// #6000: in a turned component, a named plane ("XY") is the component's own; an explicit
/// `{origin, x_dir, y_dir}` is world unless `frame: "component"` says otherwise. The reply says
/// which, with a note when the two differ.
#[test]
fn an_explicit_sketch_plane_is_read_in_the_frame_asked_for() {
    let mut s = moved_part();
    run(&mut s, "occurrence.move", json!({"occurrence": "Part:1", "axis": [1, 0, 0], "angle": "30 deg", "move_grounded": true}));
    let explicit = json!({"origin": [0, 0, 0], "x_dir": [1, 0, 0], "y_dir": [0, 1, 0]});
    let centre = |s: &mut Session, name: &str, p: Value| {
        let r = run(s, "sketch.create", p);
        run(s, "sketch.circle.center", json!({"center": [20, 10], "radius": 6}));
        run(s, "sketch.finish", json!({}));
        (r, *sketch_world_points(s, name).last().unwrap())
    };
    let (named, own) = centre(&mut s, "Named", json!({"plane": "XY", "name": "Named"}));
    assert!(named.get("frame").is_none(), "{named}");
    assert!((own.z - 5.0).abs() < 1e-6, "the component's XY is turned with it: {own:?}");

    let (world, at) = centre(&mut s, "World", json!({"plane": explicit, "name": "World"}));
    assert_eq!(world["frame"], "world");
    assert!(world["note"].as_str().is_some_and(|n| n.contains("frame")), "{world}");
    assert!(at.z.abs() < 1e-6, "today's reading: on the world XY plane: {at:?}");

    let (comp, at) = centre(&mut s, "Own", json!({"plane": explicit, "frame": "component", "name": "Own"}));
    assert_eq!(comp["frame"], "component");
    assert!(comp.get("note").is_none(), "{comp}");
    assert!(near(at, own), "the component's XY written out: {at:?} vs {own:?}");

    // Redefine reads the same way.
    let r = run(&mut s, "sketch.redefine", json!({"sketch": "World", "plane": explicit, "frame": "component"}));
    assert_eq!(r["frame"], "component");
    assert!(near(*sketch_world_points(&mut s, "World").last().unwrap(), own));

    for (p, why) in [(json!({"plane": "XY", "frame": "component"}), "explicit"), (json!({"plane": explicit, "frame": "local"}), "world")] {
        let e = s.execute("sketch.create", &p).expect_err("refused").to_string();
        assert!(e.contains(why), "{p}: {e}");
    }
}

/// #3383: a root body joined into a body of a component turned a quarter turn about X stays where
/// it is in the world: the join overlaps the plate by 10 x 5 x 5 and is one solid. When the
/// component moves on, the join follows where both bodies are placed.
#[test]
fn a_combine_tool_stays_where_it_is_placed() {
    let mut s = Session::default();
    // The design's first part is grounded when it is made (#2804); this one is moved.
    run(&mut s, "component.create", json!({"name": "Part", "grounded": false}));
    run(&mut s, "solid.box", json!({"length": 40, "width": 40, "height": 10, "corner": [-20, -20, 0], "body_name": "Plate"}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    run(&mut s, "occurrence.move", json!({"occurrence": "Part:1", "axis": [1, 0, 0], "angle": "90 deg"}));
    run(&mut s, "component.capture_position", json!({}));
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [-5, -5, -25], "operation": "new", "body_name": "Boss"}));
    run(&mut s, "solid.combine", json!({"target": "Plate", "tools": ["Boss"]}));
    let lumps = |s: &Session| s.model.state().body("Plate").map(|b| b.body.lumps().unwrap().len());
    assert_eq!(lumps(&s), Some(1), "the boss overlaps the plate where it is shown");
    assert!((volume(&mut s) - 16750.0).abs() < 1e-6, "{}", volume(&mut s));
    let (lo, hi) = world_bbox(&s, "Plate");
    assert!(near(lo, Vec3::new(-20.0, -10.0, -25.0)) && near(hi, Vec3::new(20.0, 5.0, 20.0)), "{lo:?} {hi:?}");
    // Raised 10 mm, the plate (z -10..30) clears the boss (z -25..-15).
    run(&mut s, "occurrence.move", json!({"occurrence": "Part:1", "translate": [0, 0, 10]}));
    run(&mut s, "component.capture_position", json!({}));
    assert_eq!(lumps(&s), Some(2), "the boss stays where it is in the world");
    assert!((volume(&mut s) - 17000.0).abs() < 1e-6, "{}", volume(&mut s));
}

/// The volume of one body in its own frame (a component's body once, however often it is placed).
fn body_volume(s: &Session, body: &str) -> f64 {
    s.model.state().body(body).unwrap_or_else(|| panic!("no body {body}")).measure().unwrap().volume
}

/// The body the last feature, a fillet or chamfer, keeps.
fn blend_body(s: &Session) -> Option<String> {
    match &s.doc.features.last().unwrap().kind {
        solidcraft_doc::FeatureKind::Fillet { body, .. } | solidcraft_doc::FeatureKind::Chamfer { body, .. } => body.as_ref().map(|b| b.0.clone()),
        k => panic!("{k:?}"),
    }
}

/// The points the last feature, a fillet or chamfer, keeps.
fn blend_edges(s: &Session) -> Vec<Vec3> {
    match &s.doc.features.last().unwrap().kind {
        solidcraft_doc::FeatureKind::Fillet { edges, .. } | solidcraft_doc::FeatureKind::Chamfer { edges, .. } => edges.iter().map(|p| p.0).collect(),
        k => panic!("{k:?}"),
    }
}

fn refused(s: &mut Session, id: &str, p: Value) -> String {
    let features = s.doc.features.len();
    let err = s.execute(id, &p).expect_err(&format!("{id} {p}")).to_string();
    assert_eq!(s.doc.features.len(), features, "{id} {p}: {err}");
    err
}

/// A 1 mm chamfer of a 40 mm edge between square faces.
const CUT: f64 = 20.0;
/// A box 40 x 30 x 10.
const BLOCK: f64 = 12000.0;

/// Component Base holds "Base block" and component Cap "Cap block", each a 40 x 30 x 10 box,
/// Cap's at `cap_z` in its own frame and its placement moved by `moved`; Base is active.
fn two_parts(cap_z: f64, moved: [f64; 3]) -> Session {
    let mut s = Session::default();
    run(&mut s, "component.create", json!({"name": "Base"}));
    run(&mut s, "solid.box", json!({"length": 40, "width": 30, "height": 10, "body_name": "Base block"}));
    run(&mut s, "component.create", json!({"name": "Cap", "parent": "root"}));
    run(&mut s, "solid.box", json!({"length": 40, "width": 30, "height": 10, "corner": [0, 0, cap_z], "body_name": "Cap block"}));
    if moved != [0.0; 3] {
        run(&mut s, "occurrence.move", json!({"occurrence": "Cap:1", "translate": moved}));
    }
    run(&mut s, "component.activate", json!({"component": "Base"}));
    s
}

/// Component Lower holds "Lower block" (40 x 30 x 10) and component Upper "Upper block" (40 x 30
/// x 2), both from their frames' origins, so Upper's bottom edges lie on Lower's in their own
/// frames. Upper:1 sits on Lower, and a second placement, Upper:2, under it. The root is active.
fn lower_and_uppers() -> (Session, u64) {
    let mut s = Session::default();
    run(&mut s, "component.create", json!({"name": "Lower"}));
    run(&mut s, "solid.box", json!({"length": 40, "width": 30, "height": 10, "body_name": "Lower block"}));
    run(&mut s, "component.create", json!({"name": "Upper", "parent": "root"}));
    run(&mut s, "solid.box", json!({"length": 40, "width": 30, "height": 2, "body_name": "Upper block"}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    run(&mut s, "occurrence.move", json!({"occurrence": "Upper:1", "translate": [0, 0, 10]}));
    let under = run(&mut s, "occurrence.copy", json!({"component": "Upper"}))["occurrence"].as_u64().unwrap();
    run(&mut s, "occurrence.move", json!({"occurrence": under, "space": "world", "translate": [0, 0, -12]}));
    let (lo, hi) = world_bbox(&s, "Upper block (Upper:2)");
    assert!(near(lo, Vec3::new(0.0, 0.0, -2.0)) && near(hi, Vec3::new(40.0, 30.0, 0.0)), "{lo:?} {hi:?}");
    (s, under)
}

/// A world pick on an edge of a moved part finds that part, though another part has an edge at
/// the same point in its own frame, and the blend keeps the body it found.
#[test]
fn edge_picks_on_a_moved_part_find_it_past_other_parts_frames() {
    let round = (1.0 - std::f64::consts::PI / 4.0) * 40.0;
    for (case, mut s, id, p, body, base, cap) in [
        ("v0", two_parts(0.0, [0.0, 0.0, 50.0]), "solid.chamfer", json!({"edges": [[20, 0, 0]], "distance": 1}), "Base block", BLOCK - CUT, BLOCK),
        ("v4", two_parts(0.0, [0.0, 0.0, 50.0]), "solid.chamfer", json!({"edges": [[20, 0, 50]], "distance": 1}), "Cap block", BLOCK, BLOCK - CUT),
        ("v5", two_parts(0.0, [500.0, 0.0, 0.0]), "solid.chamfer", json!({"edges": [[20, 0, 0]], "distance": 1}), "Base block", BLOCK - CUT, BLOCK),
        (
            "v6",
            two_parts(0.0, [0.0, 0.0, 50.0]),
            "solid.chamfer",
            json!({"edges": [[20, 0, 0]], "distance": 1, "tangent_chain": false}),
            "Base block",
            BLOCK - CUT,
            BLOCK,
        ),
        ("fillet", two_parts(0.0, [0.0, 0.0, 50.0]), "solid.fillet", json!({"edges": [[20, 0, 0]], "radius": 1}), "Base block", BLOCK - round, BLOCK),
        (
            "press_pull",
            two_parts(0.0, [0.0, 0.0, 50.0]),
            "solid.press_pull",
            json!({"edges": [[20, 0, 0]], "distance": 1}),
            "Base block",
            BLOCK - round,
            BLOCK,
        ),
        ("v1 unplaced", two_parts(50.0, [0.0; 3]), "solid.chamfer", json!({"edges": [[20, 0, 0]], "distance": 1}), "Base block", BLOCK - CUT, BLOCK),
    ] {
        if case == "v5" {
            run(&mut s, "component.activate", json!({"component": "root"}));
        }
        let features = s.doc.features.len();
        run(&mut s, id, p.clone());
        assert_eq!(s.doc.features.len(), features + 1, "{case}");
        assert_eq!(blend_body(&s).as_deref(), Some(body), "{case}");
        let (b, c) = (body_volume(&s, "Base block"), body_volume(&s, "Cap block"));
        assert!((b - base).abs() < 1e-3 * CUT && (c - cap).abs() < 1e-3 * CUT, "{case}: Base {b}, Cap {c}");
    }
    // The pick on Cap is kept in Cap's frame.
    let mut s = two_parts(0.0, [0.0, 0.0, 50.0]);
    run(&mut s, "solid.chamfer", json!({"edges": [[20, 0, 50]], "distance": 1}));
    assert_eq!(blend_edges(&s), vec![Vec3::new(20.0, 0.0, 0.0)]);
}

/// Where the edges of two parts lie in one place in the world, a pick without `body` is refused,
/// naming both; with `body`, or a pick's own `body`, the named part is cut.
#[test]
fn edge_picks_where_two_parts_meet_name_both() {
    for (case, cap_z, moved) in [("v2 unplaced", 0.0, [0.0; 3]), ("v3 moved", 50.0, [0.0, 0.0, -50.0])] {
        for (id, size) in [("solid.chamfer", "distance"), ("solid.fillet", "radius")] {
            let mut s = two_parts(cap_z, moved);
            let err = refused(&mut s, id, json!({"edges": [[20, 0, 0]], size: 1}));
            assert!(
                err.contains("[20, 0, 0] lies on edges of 2 bodies, Base block and Cap block") && err.contains("with `body`"),
                "{case} {id}: {err}"
            );
            assert!(!err.contains("hidden"), "{err}");
            assert_eq!((body_volume(&s, "Base block"), body_volume(&s, "Cap block")), (BLOCK, BLOCK));
        }
        let mut s = two_parts(cap_z, moved);
        run(&mut s, "solid.chamfer", json!({"edges": [[20, 0, 0]], "distance": 1, "body": "Cap block"}));
        assert!((body_volume(&s, "Cap block") - (BLOCK - CUT)).abs() < 1e-3 * CUT && body_volume(&s, "Base block") == BLOCK, "{case}");
        let mut s = two_parts(cap_z, moved);
        run(&mut s, "solid.chamfer", json!({"edges": [{"point": [20, 0, 0], "body": "Cap block"}], "distance": 1}));
        assert!((body_volume(&s, "Cap block") - (BLOCK - CUT)).abs() < 1e-3 * CUT && body_volume(&s, "Base block") == BLOCK, "{case}");
        assert_eq!(blend_body(&s).as_deref(), Some("Cap block"), "{case}");
    }
}

/// The lower block's outer bottom edge, with a hidden upper block placed under it: that block's
/// top edge lies there too, so the pick is refused and the message says it is hidden. A lower
/// block's edge whose only twin is in the upper block's own frame is cut.
#[test]
fn edge_picks_beside_a_hidden_placement_name_it() {
    let (mut s, under) = lower_and_uppers();
    run(&mut s, "browser.visibility", json!({"visible": false, "paths": [[under]]}));
    let err = refused(&mut s, "solid.chamfer", json!({"edges": [[20, 0, 0]], "distance": 1}));
    assert!(
        err.contains("lies on edges of 2 bodies, Lower block and Upper block (Upper:2)") && err.contains("(Upper block (Upper:2) is hidden)"),
        "{err}"
    );
    run(&mut s, "browser.visibility", json!({"visible": true, "paths": [[under]]}));
    let err = refused(&mut s, "solid.chamfer", json!({"edges": [[20, 0, 0]], "distance": 1}));
    assert!(err.contains("Lower block and Upper block (Upper:2)") && !err.contains("hidden"), "{err}");
    run(&mut s, "solid.chamfer", json!({"edges": [[20, 0, 0]], "distance": 1, "body": "Lower block"}));
    assert!((body_volume(&s, "Lower block") - (BLOCK - CUT)).abs() < 1e-3 * CUT && body_volume(&s, "Upper block") == 2400.0);
    // Upper:2 out of the way: the lower block's bottom left edge has a twin only in Upper's frame.
    let (mut s, under) = lower_and_uppers();
    run(&mut s, "occurrence.move", json!({"occurrence": under, "space": "world", "translate": [0, 0, -100]}));
    run(&mut s, "solid.chamfer", json!({"edges": [[0, 15, 0]], "distance": 1}));
    assert_eq!(blend_body(&s).as_deref(), Some("Lower block"));
    assert!((body_volume(&s, "Lower block") - (BLOCK - 15.0)).abs() < 1e-3 * CUT && body_volume(&s, "Upper block") == 2400.0);
}

/// The lower block's top edge, where the upper block on it has its bottom edges, is refused
/// naming both; `body` cuts either. A second lower block placed on the upper one meets it at the
/// upper block's top edges likewise.
#[test]
fn edge_picks_on_a_top_edge_under_another_part_name_both() {
    let (mut s, _) = lower_and_uppers();
    let err = refused(&mut s, "solid.chamfer", json!({"edges": [[20, 0, 10]], "distance": 1}));
    assert!(err.contains("[20, 0, 10] lies on edges of 2 bodies, Lower block and Upper block:"), "{err}");
    for (body, lower, upper) in [("Lower block", BLOCK - CUT, 2400.0), ("Upper block", BLOCK, 2400.0 - CUT)] {
        let (mut s, _) = lower_and_uppers();
        run(&mut s, "solid.chamfer", json!({"edges": [[20, 0, 10]], "distance": 1, "body": body}));
        let (b, c) = (body_volume(&s, "Lower block"), body_volume(&s, "Upper block"));
        assert!((b - lower).abs() < 1e-3 * CUT && (c - upper).abs() < 1e-3 * CUT, "{body}: Lower {b}, Upper {c}");
    }
    let (mut s, _) = lower_and_uppers();
    let stacked = run(&mut s, "occurrence.copy", json!({"component": "Lower"}))["occurrence"].as_u64().unwrap();
    run(&mut s, "occurrence.move", json!({"occurrence": stacked, "space": "world", "translate": [0, 0, 12]}));
    let err = refused(&mut s, "solid.chamfer", json!({"edges": [[20, 0, 12]], "distance": 1}));
    assert!(err.contains("[20, 0, 12] lies on edges of 2 bodies, Upper block and Lower block (Lower:2):"), "{err}");
    run(&mut s, "solid.chamfer", json!({"edges": [[20, 0, 12]], "distance": 1, "body": "Lower block (Lower:2)"}));
    assert_eq!(blend_body(&s).as_deref(), Some("Lower block"));
    assert_eq!(blend_edges(&s), vec![Vec3::new(20.0, 0.0, 0.0)]);
    assert!((body_volume(&s, "Lower block") - (BLOCK - CUT)).abs() < 1e-3 * CUT && body_volume(&s, "Upper block") == 2400.0);
}

/// A block's top edge inside the bottom face of a plate placed on it: the block alone has an edge
/// there.
#[test]
fn an_edge_inside_another_parts_face_is_picked() {
    let mut s = Session::default();
    run(&mut s, "component.create", json!({"name": "Lower"}));
    run(&mut s, "solid.box", json!({"length": 40, "width": 30, "height": 10, "body_name": "Lower block"}));
    run(&mut s, "component.create", json!({"name": "Plate", "parent": "root"}));
    run(&mut s, "solid.box", json!({"length": 100, "width": 100, "height": 2, "corner": [-30, -35, 0], "body_name": "Plate block"}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    run(&mut s, "occurrence.move", json!({"occurrence": "Plate:1", "translate": [0, 0, 10]}));
    run(&mut s, "solid.chamfer", json!({"edges": [[20, 0, 10]], "distance": 1}));
    assert_eq!(blend_body(&s).as_deref(), Some("Lower block"));
    assert!((body_volume(&s, "Lower block") - (BLOCK - CUT)).abs() < 1e-3 * CUT && body_volume(&s, "Plate block") == 20000.0);
}

/// Round edges of two parts in one place: a pick a little inside them, as a click on the drawn
/// edge lands, is refused naming both. A disc edge 0.2 mm out is another edge.
#[test]
fn coincident_round_edges_of_two_parts_are_refused() {
    let parts = |b_radius: f64| {
        let mut s = Session::default();
        run(&mut s, "component.create", json!({"name": "A"}));
        run(&mut s, "solid.cylinder", json!({"radius": 10, "height": 10, "body_name": "A rod"}));
        run(&mut s, "component.create", json!({"name": "B", "parent": "root"}));
        run(&mut s, "solid.cylinder", json!({"radius": b_radius, "height": 2, "body_name": "B disc"}));
        run(&mut s, "component.activate", json!({"component": "root"}));
        run(&mut s, "occurrence.move", json!({"occurrence": "B:1", "translate": [0, 0, 10]}));
        s
    };
    let pi = std::f64::consts::PI;
    let disc = |r: f64| pi * r * r * 2.0;
    // A round of radius 1 on the rod's top edge, radius 10: its section's area at its centroid's
    // radius.
    let (a, c) = (1.0 - pi / 4.0, (10.0 - 3.0 * pi) / (3.0 * (4.0 - pi)));
    let rounded = pi * 100.0 * 10.0 - a * 2.0 * pi * (10.0 - c);
    let mut s = parts(10.0);
    let err = refused(&mut s, "solid.fillet", json!({"edges": [[0, 9.98, 10]], "radius": 1}));
    assert!(err.contains("lies on edges of 2 bodies, A rod and B disc"), "{err}");
    for pick in [[0.0, 9.98, 10.0], [0.0, 10.0, 10.0]] {
        let mut s = parts(10.2);
        run(&mut s, "solid.fillet", json!({"edges": [pick], "radius": 1}));
        assert_eq!(blend_body(&s).as_deref(), Some("A rod"), "{pick:?}");
        let (v, b) = (body_volume(&s, "A rod"), body_volume(&s, "B disc"));
        assert!((v - rounded).abs() < 2e-4 * rounded && (b - disc(10.2)).abs() < 1e-6 * b, "{pick:?}: {v} vs {rounded}, {b}");
    }
}

/// Two picks at one point of their parts' frames, on two parts, in one command: each is looked
/// up where it was picked, and as the feature keeps one body or none, it is refused.
#[test]
fn two_picks_at_one_point_of_two_parts_frames_are_refused() {
    let mut s = two_parts(0.0, [0.0, 0.0, 50.0]);
    let err = refused(&mut s, "solid.chamfer", json!({"edges": [[20, 0, 0], [20, 0, 50]], "distance": 1}));
    assert!(err.contains("the picks lie on 2 bodies, Base block and Cap block") && err.contains("in a command of its own"), "{err}");
    assert_eq!((body_volume(&s, "Base block"), body_volume(&s, "Cap block")), (BLOCK, BLOCK));
}

/// One part placed twice: its edge picked through both placements, or through two placements in
/// one place, is one edge of one body.
#[test]
fn one_edge_of_two_placements_is_one_edge() {
    for (offset, edges) in [
        ([0.0, 40.0, 0.0], json!([[20, 0, 0], [20, 40, 0]])),
        ([0.0; 3], json!([[20, 0, 0]])),
        // Overlapping along the edge: the pick is at x 25 of one placement and x 15 of the other.
        ([10.0, 0.0, 0.0], json!([[25, 0, 0]])),
    ] {
        let mut s = Session::default();
        run(&mut s, "component.create", json!({"name": "Pin"}));
        run(&mut s, "solid.box", json!({"length": 40, "width": 30, "height": 10, "body_name": "Pin block"}));
        run(&mut s, "component.activate", json!({"component": "root"}));
        let copy = run(&mut s, "occurrence.copy", json!({"component": "Pin"}))["occurrence"].as_u64().unwrap();
        if offset != [0.0; 3] {
            run(&mut s, "occurrence.move", json!({"occurrence": copy, "space": "world", "translate": offset}));
        }
        run(&mut s, "solid.chamfer", json!({"edges": edges, "distance": 1}));
        assert!((body_volume(&s, "Pin block") - (BLOCK - CUT)).abs() < 1e-3 * CUT, "{offset:?}: {}", body_volume(&s, "Pin block"));
        assert!((volume(&mut s) - 2.0 * (BLOCK - CUT)).abs() < 2e-3 * CUT, "{offset:?}");
        assert_eq!(blend_body(&s), None, "one body in the model");
    }
}

/// An edge given by body and index keeps its body on the feature.
#[test]
fn an_edge_by_index_keeps_its_body() {
    let mut s = two_parts(0.0, [0.0, 0.0, 50.0]);
    let tol = 1e-3_f64.max(s.model.state().body("Base block").unwrap().body.size() * 2e-3);
    let edges = s.model.state().body("Base block").unwrap().body.edges(tol).unwrap();
    let index = edges.iter().position(|e| e.mid.dist(Vec3::new(20.0, 0.0, 0.0)) < 1e-6).unwrap();
    run(&mut s, "solid.chamfer", json!({"edges": [{"body": "Base block", "index": index}], "distance": 1}));
    assert_eq!(blend_body(&s).as_deref(), Some("Base block"));
    assert!((body_volume(&s, "Base block") - (BLOCK - CUT)).abs() < 1e-3 * CUT && body_volume(&s, "Cap block") == BLOCK);
}

/// A pick 0.42 mm off a moved part's edge, beyond the faces the placement is found by, is still
/// looked up where it was picked: another part's edge there in its own frame is not in the way.
#[test]
fn a_pick_beside_an_edge_is_looked_up_where_it_was_picked() {
    let mut s = two_parts(0.0, [0.0, 0.0, 50.0]);
    run(&mut s, "solid.chamfer", json!({"edges": [[20, -0.3, -0.3]], "distance": 1}));
    assert_eq!(blend_body(&s).as_deref(), Some("Base block"));
    assert!((body_volume(&s, "Base block") - (BLOCK - CUT)).abs() < 1e-3 * CUT && body_volume(&s, "Cap block") == BLOCK);
}

/// A chamfer of a moved part, found by a world pick, stays on that part through an edit of
/// the other part, a recompute, undo and redo, and Edit Feature.
#[test]
fn a_blend_found_by_a_world_pick_keeps_its_part() {
    let mut s = two_parts(0.0, [0.0, 0.0, 50.0]);
    run(&mut s, "solid.chamfer", json!({"edges": [[20, 0, 50]], "distance": 1, "name": "Edge"}));
    let check = |s: &Session, base: f64, cap: f64, step: &str| {
        let (b, c) = (body_volume(s, "Base block"), body_volume(s, "Cap block"));
        assert!((b - base).abs() < 1e-3 * CUT && (c - cap).abs() < 1e-3 * CUT, "{step}: Base {b}, Cap {c}");
        assert!(s.model.results.iter().all(|r| r.error.is_none()), "{step}: {:?}", s.model.results);
    };
    check(&s, BLOCK, BLOCK - CUT, "made");
    run(&mut s, "timeline.edit", json!({"feature": "Box1", "set": {"length": "50"}}));
    check(&s, 15000.0, BLOCK - CUT, "Base edited");
    run(&mut s, "timeline.compute_all", json!({}));
    check(&s, 15000.0, BLOCK - CUT, "computed");
    run(&mut s, "edit.undo", json!({}));
    check(&s, BLOCK, BLOCK - CUT, "undone");
    run(&mut s, "edit.redo", json!({}));
    check(&s, 15000.0, BLOCK - CUT, "redone");
    run(&mut s, "timeline.redefine", json!({"feature": "Edge", "command": "solid.chamfer", "params": {"edges": [[20, 0, 50]], "distance": 2}}));
    check(&s, 15000.0, BLOCK - 4.0 * CUT, "redefined");
    assert_eq!(blend_body(&s).as_deref(), Some("Cap block"));
}

/// A corner of a moved part is refused at the point picked, in the world.
#[test]
fn a_corner_of_a_moved_part_is_refused_where_it_was_picked() {
    let mut s = two_parts(0.0, [0.0, 0.0, 50.0]);
    let err = refused(&mut s, "solid.chamfer", json!({"edges": [[0, 0, 50]], "distance": 1}));
    assert!(err.contains("[0, 0, 50] is where 3 edges meet"), "{err}");
}

/// Picks far out, or overflowing a placement, in a design with moved parts are refused.
#[test]
fn hostile_edge_picks_in_a_placed_design_are_refused() {
    let mut s = two_parts(0.0, [0.0, 0.0, 50.0]);
    for pick in [json!([1e300, 0, 0]), json!([1.7e308, -1.7e308, 1.7e308]), json!([[1e300, 0, 0]])] {
        let err = s.execute("solid.chamfer", &json!({"edges": [pick], "distance": 1})).expect_err("refused");
        assert!(matches!(err, crate::EngineError::BadParams { .. }), "{pick}: {err}");
    }
    assert_eq!((body_volume(&s, "Base block"), body_volume(&s, "Cap block")), (BLOCK, BLOCK));
}

/// The index of the edge of `body` whose midpoint is `mid`.
fn edge_index(s: &Session, body: &str, mid: Vec3) -> usize {
    let b = s.model.state().body(body).unwrap().body.clone();
    let edges = b.edges(1e-3_f64.max(b.size() * 2e-3)).unwrap();
    edges.iter().position(|e| e.mid.dist(mid) < 1e-6).unwrap()
}

/// A pick's own `body` must exist and agree with the feature's `body`: the feature keeps one
/// body, and the pick is found on its own.
#[test]
fn a_picks_own_body_must_agree_with_body() {
    let mut s = two_parts(0.0, [0.0, 0.0, 50.0]);
    let index = edge_index(&s, "Cap block", Vec3::new(20.0, 0.0, 0.0));
    for edge in [json!({"point": [20, 0, 50], "body": "Cap block"}), json!({"body": "Cap block", "index": index})] {
        let err = refused(&mut s, "solid.chamfer", json!({"edges": [edge], "body": "Base block", "distance": 1}));
        assert!(err.contains("an edge names `Cap block`, but `body` is `Base block`: give one"), "{edge}: {err}");
    }
    let err = refused(&mut s, "solid.chamfer", json!({"edges": [{"point": [20, 0, 0], "body": "Nobody"}], "distance": 1}));
    assert!(err.contains("no body `Nobody`"), "{err}");
    assert_eq!((body_volume(&s, "Base block"), body_volume(&s, "Cap block")), (BLOCK, BLOCK));
    run(&mut s, "solid.chamfer", json!({"edges": [{"point": [20, 0, 50], "body": "Cap block"}], "body": "Cap block", "distance": 1}));
    assert!((body_volume(&s, "Cap block") - (BLOCK - CUT)).abs() < 1e-3 * CUT && body_volume(&s, "Base block") == BLOCK);
}

/// Press Pull on an edge is a fillet: where two parts meet it is refused naming both, and
/// `body`, by its own name or a placement's, rounds that part.
#[test]
fn press_pull_on_an_edge_where_two_parts_meet_takes_body() {
    let round = (1.0 - std::f64::consts::PI / 4.0) * 40.0;
    let mut s = two_parts(50.0, [0.0, 0.0, -50.0]);
    let err = refused(&mut s, "solid.press_pull", json!({"edges": [[20, 0, 0]], "distance": 1}));
    assert!(err.contains("[20, 0, 0] lies on edges of 2 bodies, Base block and Cap block: name the one to round with `body`"), "{err}");
    run(&mut s, "solid.press_pull", json!({"edges": [[20, 0, 0]], "distance": 1, "body": "Cap block"}));
    assert_eq!(blend_body(&s).as_deref(), Some("Cap block"));
    assert!((body_volume(&s, "Cap block") - (BLOCK - round)).abs() < 1e-3 * CUT && body_volume(&s, "Base block") == BLOCK);
    let (mut s, _) = lower_and_uppers();
    let stacked = run(&mut s, "occurrence.copy", json!({"component": "Lower"}))["occurrence"].as_u64().unwrap();
    run(&mut s, "occurrence.move", json!({"occurrence": stacked, "space": "world", "translate": [0, 0, 12]}));
    run(&mut s, "solid.press_pull", json!({"edges": [[20, 0, 12]], "distance": 1, "body": "Lower block (Lower:2)"}));
    assert_eq!(blend_body(&s).as_deref(), Some("Lower block"));
    assert_eq!(blend_edges(&s), vec![Vec3::new(20.0, 0.0, 0.0)]);
    assert!((body_volume(&s, "Lower block") - (BLOCK - round)).abs() < 1e-3 * CUT && body_volume(&s, "Upper block") == 2400.0);
}

/// A body offset squashed flat, as only a hostile file has, leaves no placement to look a pick
/// up through: the pick is looked up in the body's own frame, so one far from every edge is
/// refused and one on an edge is cut.
#[test]
fn edge_picks_with_no_placement_to_look_through_are_checked() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 40, "width": 30, "height": 10, "body_name": "Box"}));
    let mut flat = solidcraft_doc::IDENTITY;
    flat[2][2] = 0.0;
    s.doc_mut().body_offsets.insert("Box".into(), flat);
    let err = refused(&mut s, "solid.chamfer", json!({"edges": [[20, 15, 10]], "distance": 1}));
    assert!(err.contains("no edge at [20, 15, 10]"), "{err}");
    run(&mut s, "solid.chamfer", json!({"edges": [[20, 0, 0]], "distance": 1}));
    assert!((body_volume(&s, "Box") - (BLOCK - CUT)).abs() < 1e-3 * CUT, "{}", body_volume(&s, "Box"));
}
