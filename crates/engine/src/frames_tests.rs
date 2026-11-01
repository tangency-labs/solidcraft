//! Components as frames: what a component holds moves with its occurrence, and picks in the
//! world land in the component's own frame (user bug #4).

use serde_json::{Value, json};
use solidcraft_geom::Vec3;

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
    let w = s.world_state();
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
    // The edge along X at the top front: world (65, 0, 10).
    run(&mut s, "solid.fillet", json!({"edges": [[65, 0, 10]], "radius": 2}));
    assert!(volume(&mut s) < v0 - 1.0, "the fillet cut material");
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
    assert!(volume(&mut s) < v1 - 1.0, "the hole cut material: {h}");
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
    assert!(v1 < v0 - 1.0, "{v0} {v1}");
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
