use serde_json::{Value, json};

use crate::Session;

#[test]
fn grouped_face_auto_operation_keeps_implicit_and_explicit_participants() {
    let mut s = Session::default();
    let part = run(&mut s, "component.create", json!({"name":"Part","grounded":false}));
    run(&mut s, "solid.box", json!({"length":10,"width":10,"height":10,"body_name":"Stock"}));
    run(&mut s, "component.activate", json!({"component":0}));
    for offset in [0., 30.] {
        if offset != 0. {
            run(&mut s, "occurrence.move", json!({"occurrence":part["occurrence"],"translate":[offset,0,0]}));
        }
        // To Object follows the target's side, even when the distance and
        // direction fields still describe the opposite extrusion.
        for (height, expected) in [(8., "cut"), (12., "join")] {
            let params = json!({"faces":[[offset+2.,2,10],[offset+8.,8,10]],"face_body":"Stock",
                "to":[offset+5.,5,height],"direction":"negative"});
            assert_eq!(crate::auto_operation(&s, &params), Some(expected));
        }
        for distance in [-2., 2.] {
            let mut params = json!({"faces":[[offset+2.,2,10],[offset+8.,8,10]],"distance":distance});
            let expected = if distance < 0. { "cut" } else { "join" };
            assert_eq!(crate::auto_operation(&s, &params), Some(expected));
            params["face_body"] = json!("Stock");
            assert_eq!(crate::auto_operation(&s, &params), Some(expected));
            let mut actual = s.scratch();
            if offset != 0. {
                run(&mut actual, "component.activate", json!({"component":part["component"]}));
            }
            params["operation"] = json!("auto");
            let made = run(&mut actual, "solid.extrude", params);
            let body = actual.model.state().body("Stock").unwrap().body.clone();
            let volume = solidcraft_kernel::measure(&body).unwrap().volume;
            assert!(
                (volume - (1000. + 100. * distance)).abs() < 1e-6,
                "offset={offset}, distance={distance}, volume={volume}, feature={:?}",
                actual.doc.feature(made["feature"].as_u64().unwrap())
            );
        }
        if offset != 0. {
            // Outside the edited component, grouping keeps the original explicit
            // face command's coordinate and participant behavior.
            let mut grouped = s.scratch();
            let mut single = s.scratch();
            run(&mut grouped, "solid.extrude", json!({"faces":[[32,2,10],[38,8,10]],"face_body":"Stock","distance":-2,"operation":"cut"}));
            run(&mut single, "solid.extrude", json!({"face":[32,2,10],"face_body":"Stock","distance":-2,"operation":"cut"}));
            let volume = |session: &Session| solidcraft_kernel::measure(&session.model.state().body("Stock").unwrap().body).unwrap().volume;
            assert!((volume(&grouped) - volume(&single)).abs() < 1e-6);
        }
    }
    run(&mut s, "solid.box", json!({"corner":[100,0,0],"length":10,"width":10,"height":10,"body_name":"Other"}));
    let mut params = json!({"faces":[[32,2,10],[38,8,10]],"face_body":"Stock","distance":-2});
    // The picked part takes part beside the active component's own body (#5679): going in cuts
    // it, and the top level's own body is left alone.
    assert_eq!(crate::auto_operation(&s, &params), Some("cut"), "the picked body takes part beside the active component's own");
    for operation in ["auto", "cut"] {
        let mut t = s.scratch();
        let mut p = params.clone();
        p["operation"] = json!(operation);
        run(&mut t, "solid.extrude", p);
        let volume = |name: &str| solidcraft_kernel::measure(&t.model.state().body(name).unwrap().body).unwrap().volume;
        assert!(
            (volume("Stock") - 800.).abs() < 1e-6 && (volume("Other") - 1000.).abs() < 1e-6,
            "{operation}: {} {}",
            volume("Stock"),
            volume("Other")
        );
    }
    params["participants"] = json!(["Stock"]);
    assert_eq!(crate::auto_operation(&s, &params), Some("cut"));
    params["participants"] = json!(["Other"]);
    assert_eq!(crate::auto_operation(&s, &params), Some("join"));
}

/// A part inserted into a new design (a STEP file) sits in its own component under the empty
/// root. A sketch on its face must join it going out and cut it going in, never make a
/// separate body beside it.
#[test]
fn sketch_on_a_part_in_another_component_joins_and_cuts_it() {
    let mut s = Session::default();
    run(&mut s, "component.create", json!({"name":"Part"}));
    run(&mut s, "solid.box", json!({"length":10,"width":10,"height":10,"body_name":"Stock"}));
    run(&mut s, "component.activate", json!({"component":0}));
    run(&mut s, "sketch.create", json!({"plane":{"face":[5,5,10]},"project_edges":false}));
    run(&mut s, "sketch.rectangle.center", json!({"center":[5,5],"corner":[6,6]}));
    run(&mut s, "sketch.circle.center", json!({"center":[2,2],"radius":1}));
    run(&mut s, "sketch.finish", json!({}));
    let added = 3. * (4. + std::f64::consts::PI);
    for (params, expected, volume) in [
        (json!({"distance":3}), "join", 1000. + added),
        (json!({"distance":-3}), "cut", 1000. - added),
        (json!({"distance":3,"direction":"negative"}), "cut", 1000. - added),
        // Through All with no distance cut nothing and made a separate body.
        (json!({"through_all":true,"direction":"negative"}), "cut", 1000. - 10. / 3. * added),
    ] {
        assert_eq!(crate::auto_operation(&s, &params), Some(expected), "{params}");
        for operation in ["auto", expected] {
            let mut t = s.scratch();
            let mut p = params.clone();
            p["operation"] = json!(operation);
            p["profiles"] = json!("all");
            run(&mut t, "solid.extrude", p);
            let st = t.model.state();
            assert_eq!(st.bodies.len(), 1, "{params} {operation}: no separate body");
            let got = solidcraft_kernel::measure(&st.body("Stock").unwrap().body).unwrap().volume;
            assert!((got - volume).abs() < 1e-2, "{params} {operation}: volume {got}, expected {volume}");
        }
    }
}

/// Every way an extrude can go into a part inserted in its own component (negative distance,
/// negative direction, Through All, symmetric, To Object, a start offset, past the far side,
/// a face instead of a sketch) cuts that part, with `auto` and with an explicit Cut; going
/// out joins it. None makes a separate body.
#[test]
fn every_inward_extrude_into_a_part_in_another_component_cuts_it() {
    let mut s = Session::default();
    run(&mut s, "component.create", json!({"name":"Part"}));
    run(&mut s, "solid.box", json!({"length":10,"width":10,"height":10,"body_name":"Stock"}));
    run(&mut s, "component.activate", json!({"component":0}));
    let mut sketched = s.scratch();
    run(&mut sketched, "sketch.create", json!({"plane":{"face":[5,5,10]},"project_edges":false}));
    run(&mut sketched, "sketch.rectangle.center", json!({"center":[5,5],"corner":[6,6]}));
    run(&mut sketched, "sketch.circle.center", json!({"center":[2,2],"radius":1}));
    run(&mut sketched, "sketch.finish", json!({}));
    let area = 4. + std::f64::consts::PI;
    let sketch = |p: Value, depth: f64, op: &'static str| (p, depth, op, true);
    let face = |p: Value, depth: f64, op: &'static str| (p, depth, op, false);
    let cases = [
        sketch(json!({"distance":-3}), 3., "cut"),
        sketch(json!({"distance":3,"direction":"negative"}), 3., "cut"),
        sketch(json!({"distance":"-3 mm"}), 3., "cut"),
        sketch(json!({"distance":-25}), 10., "cut"),
        sketch(json!({"through_all":true,"direction":"negative"}), 10., "cut"),
        sketch(json!({"extent":"through_all","direction":"negative"}), 10., "cut"),
        sketch(json!({"distance":6,"direction":"symmetric"}), 3., "cut"),
        sketch(json!({"distance":-3,"start_offset":-2}), 3., "cut"),
        sketch(json!({"to":[5,5,0]}), 10., "cut"),
        sketch(json!({"to":[5,5,4]}), 6., "cut"),
        sketch(json!({"distance":3}), -3., "join"),
        sketch(json!({"distance":-3,"direction":"negative"}), -3., "join"),
        face(json!({"face":[5,5,10],"distance":-3}), 3., "cut"),
        face(json!({"face":[5,5,10],"distance":3,"direction":"negative"}), 3., "cut"),
        face(json!({"face":[5,5,10],"distance":3}), -3., "join"),
    ];
    let mut wrong = Vec::new();
    for (params, depth, expected, from_sketch) in cases {
        // A face extrude moves the whole 10 x 10 face; a sketch only its two profiles.
        let volume = 1000. - depth * if from_sketch { area } else { 100. };
        for operation in ["auto", expected] {
            let mut t = if from_sketch { sketched.scratch() } else { s.scratch() };
            let mut p = params.clone();
            p["operation"] = json!(operation);
            if from_sketch {
                p["profiles"] = json!("all");
            }
            let auto = crate::auto_operation(&t, &p);
            if operation == "auto" && auto != Some(expected) {
                wrong.push(format!("{params}: auto reads {auto:?}, expected {expected}"));
            }
            if let Err(e) = t.execute("solid.extrude", &p) {
                wrong.push(format!("{params} {operation}: {e}"));
                continue;
            }
            let st = t.model.state();
            let names: Vec<_> = st.bodies.iter().map(|b| b.name.to_string()).collect();
            let got = st.body("Stock").and_then(|b| solidcraft_kernel::measure(&b.body).ok()).map(|m| m.volume);
            if names != ["Stock"] || got.is_none_or(|v| (v - volume).abs() > 1e-2) {
                wrong.push(format!("{params} {operation}: bodies {names:?}, Stock volume {got:?}, expected {volume:.3}"));
            }
        }
    }
    assert!(wrong.is_empty(), "{} wrong:\n{}", wrong.len(), wrong.join("\n"));
}

/// #5679's part: a 10 mm cube `Stock` in component Part, its placement moved to (30, 5, 2) and
/// turned 35 degrees about (1, 2, 0), with the top level active; with `base`, the top level has a
/// body of its own, a 20 x 20 x 5 `Base` at (-40, -40, 0). Returns Part's placement.
fn turned_part(base: bool) -> (Session, solidcraft_doc::Mat) {
    let mut s = Session::default();
    let part = run(&mut s, "component.create", json!({"name": "Part", "grounded": false}));
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "body_name": "Stock"}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    run(&mut s, "occurrence.move", json!({"occurrence": part["occurrence"], "translate": [30, 5, 2], "axis": [1, 2, 0], "angle": "35 deg"}));
    run(&mut s, "component.capture_position", json!({}));
    if base {
        run(&mut s, "solid.box", json!({"length": 20, "width": 20, "height": 5, "corner": [-40, -40, 0], "operation": "new", "body_name": "Base"}));
    }
    let placed = s.doc.component_transform(part["component"].as_u64().unwrap());
    (s, placed)
}

/// A sketch on the face at world point `at`, with `draw` given the point's place in the sketch.
fn sketch_on_face(s: &mut Session, at: solidcraft_geom::Vec3, draw: impl FnOnce(&mut Session, solidcraft_geom::Vec2, solidcraft_geom::Plane)) {
    run(s, "sketch.create", json!({"plane": {"face": [at.x, at.y, at.z]}, "project_edges": false}));
    let plane = s
        .doc
        .features
        .iter()
        .rev()
        .find_map(|f| match &f.kind {
            solidcraft_doc::FeatureKind::Sketch { plane: solidcraft_doc::PlaneRef::Face { plane, .. }, .. } => Some(*plane),
            _ => None,
        })
        .unwrap();
    draw(s, plane.to_local(at), plane);
    run(s, "sketch.finish", json!({}));
}

fn body_volume(s: &Session, name: &str) -> f64 {
    solidcraft_kernel::measure(&s.model.state().body(name).unwrap().body).unwrap().volume
}

/// #5679: with the top level active, a circle sketched on the top face of a part in a moved and
/// turned sub-component reads as a Cut going in and a Join going out, and either one, automatic
/// or chosen, changes that part's body in its own component: the hole is where the circle is
/// shown, not where the numbers would be in the part's frame. The top level's own body is left
/// alone, whether it has one or not.
#[test]
fn a_sketch_on_a_turned_sub_components_face_cuts_and_joins_it_from_the_top_level() {
    let hole = std::f64::consts::PI * 4.0 * 3.0;
    for base in [false, true] {
        let (mut s, placed) = turned_part(base);
        let top = solidcraft_doc::apply_point(&placed, solidcraft_geom::Vec3::new(5.0, 5.0, 10.0));
        sketch_on_face(&mut s, top, |s, c, _| {
            run(s, "sketch.circle.center", json!({"center": [c.x, c.y], "radius": 2}));
        });
        for (distance, expected) in [(-3.0, "cut"), (3.0, "join")] {
            assert_eq!(crate::auto_operation(&s, &json!({"distance": distance})), Some(expected), "base {base}, {distance}");
            for operation in ["auto", expected] {
                let mut t = s.scratch();
                run(&mut t, "solid.extrude", json!({"distance": distance, "operation": operation}));
                let case = format!("base {base}, {distance} {operation}");
                let names: Vec<String> = t.model.state().bodies.iter().map(|b| b.name.clone()).collect();
                assert_eq!(names, if base { vec!["Stock", "Base"] } else { vec!["Stock"] }, "{case}");
                let want = if expected == "cut" { 1000.0 - hole } else { 1000.0 + hole };
                assert!((body_volume(&t, "Stock") - want).abs() < 1e-3 * hole, "{case}: {}", body_volume(&t, "Stock"));
                // In the part's own frame: through the middle of its top, not beside it.
                let stock = t.model.state().body("Stock").unwrap().mesh();
                let at = |x: f64, y: f64, z: f64| stock.contains(solidcraft_geom::Vec3::new(x, y, z));
                if expected == "cut" {
                    assert!(!at(5.0, 5.0, 8.0) && at(5.0, 5.0, 6.0) && at(1.0, 1.0, 9.0), "{case}");
                } else {
                    assert!(at(5.0, 5.0, 12.0) && !at(1.0, 1.0, 12.0), "{case}");
                }
                if base {
                    assert!((body_volume(&t, "Base") - 2000.0).abs() < 1e-6, "{case}");
                }
            }
        }
    }
}

/// #5679: a revolve from the top level, from a profile on the turned part's top face, joins or
/// cuts that part. A 6 x 1 rectangle turned about its long edge, which lies on the face, is a
/// cylinder of radius 1 half above the face and half in the part: a Join adds 3π, a Cut takes it.
#[test]
fn a_revolve_from_a_turned_sub_components_face_joins_and_cuts_it_from_the_top_level() {
    let half = std::f64::consts::PI * 3.0;
    for (operation, want) in [("join", 1000.0 + half), ("cut", 1000.0 - half)] {
        let (mut s, placed) = turned_part(true);
        let top = solidcraft_doc::apply_point(&placed, solidcraft_geom::Vec3::new(5.0, 5.0, 10.0));
        let mut axis = json!(null);
        sketch_on_face(&mut s, top, |s, c, plane| {
            run(s, "sketch.rectangle.two_point", json!({"p0": [c.x - 3.0, c.y], "p1": [c.x + 3.0, c.y + 1.0]}));
            axis = json!({"origin": [top.x, top.y, top.z], "dir": [plane.x.x, plane.x.y, plane.x.z]});
        });
        run(&mut s, "solid.revolve", json!({"axis": axis, "operation": operation}));
        let names: Vec<String> = s.model.state().bodies.iter().map(|b| b.name.clone()).collect();
        assert_eq!(names, ["Stock", "Base"], "{operation}");
        assert_eq!(s.model.state().body("Stock").map(|b| b.body.shell_count()), Some(1), "{operation}");
        assert!((body_volume(&s, "Stock") - want).abs() < 1e-3 * half, "{operation}: {}", body_volume(&s, "Stock"));
        assert!((body_volume(&s, "Base") - 2000.0).abs() < 1e-6, "{operation}");
    }
}

/// #5679 for Loft (and Sweep, through the same join target): with the top level active, a loft
/// from a circle on the turned part's top face to one 5 mm above joins the part, in one piece.
#[test]
fn a_loft_from_a_turned_sub_components_face_joins_it_from_the_top_level() {
    let (mut s, placed) = turned_part(true);
    let top = solidcraft_doc::apply_point(&placed, solidcraft_geom::Vec3::new(5.0, 5.0, 10.0));
    let mut centre = solidcraft_geom::Vec2::new(0.0, 0.0);
    sketch_on_face(&mut s, top, |s, c, _| {
        run(s, "sketch.circle.center", json!({"center": [c.x, c.y], "radius": 2}));
        centre = c;
    });
    let low = s.doc.features.iter().rev().find(|f| matches!(f.kind, solidcraft_doc::FeatureKind::Sketch { .. })).unwrap().id;
    let high = run(&mut s, "sketch.create", json!({"plane": {"face": [top.x, top.y, top.z]}, "offset": 5, "project_edges": false}))["sketch"].clone();
    run(&mut s, "sketch.circle.center", json!({"center": [centre.x, centre.y], "radius": 2}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.loft", json!({"sections": [{"sketch": low}, {"sketch": high}], "operation": "join"}));
    let names: Vec<String> = s.model.state().bodies.iter().map(|b| b.name.clone()).collect();
    assert_eq!(names, ["Stock", "Base"]);
    assert_eq!(s.model.state().body("Stock").map(|b| b.body.shell_count()), Some(1));
    let added = std::f64::consts::PI * 4.0 * 5.0;
    assert!((body_volume(&s, "Stock") - 1000.0 - added).abs() < 1e-3 * added, "{}", body_volume(&s, "Stock"));
    assert!((body_volume(&s, "Base") - 2000.0).abs() < 1e-6);
}

/// #5679: the part changes in its component, so a second placement of it shows the hole too. A
/// sketch on that second placement's face would draw the hole where the first one is: the cut
/// is refused with the reason, and the design stays as it was.
#[test]
fn a_cut_from_the_top_level_shows_in_every_placement_and_a_later_placement_is_refused() {
    let (mut s, placed) = turned_part(true);
    let second = run(&mut s, "occurrence.copy", json!({"component": "Part", "translate": [0, 40, 0]}))["occurrence"].as_u64().unwrap();
    let top = solidcraft_doc::apply_point(&placed, solidcraft_geom::Vec3::new(5.0, 5.0, 10.0));
    let mut first = s.scratch();
    sketch_on_face(&mut first, top, |s, c, _| {
        run(s, "sketch.circle.center", json!({"center": [c.x, c.y], "radius": 2}));
    });
    run(&mut first, "solid.extrude", json!({"distance": -3, "operation": "cut"}));
    let world = first.world_state();
    let placements: Vec<f64> =
        world.bodies.iter().filter(|b| b.definition_name() == "Stock").map(|b| solidcraft_kernel::measure(&b.body).unwrap().volume).collect();
    let want = 1000.0 - std::f64::consts::PI * 4.0 * 3.0;
    assert_eq!(placements.len(), 2, "{placements:?}");
    assert!(placements.iter().all(|v| (v - want).abs() < 1e-2), "{placements:?}");

    let (_, moved) = s.doc.path_placement(&[second]).unwrap();
    let top = solidcraft_doc::apply_point(&moved, solidcraft_geom::Vec3::new(5.0, 5.0, 10.0));
    sketch_on_face(&mut s, top, |s, c, _| {
        run(s, "sketch.circle.center", json!({"center": [c.x, c.y], "radius": 2}));
    });
    let features = s.doc.features.len();
    for operation in ["cut", "join"] {
        let refused = s.execute("solid.extrude", &json!({"distance": -3, "operation": operation})).unwrap_err().to_string();
        assert!(refused.contains("placed more than once") && refused.contains("activate"), "{refused}");
    }
    assert_eq!(s.doc.features.len(), features);
    assert!((body_volume(&s, "Stock") - 1000.0).abs() < 1e-6);
}

/// #5679: with the top level's own body as the target, nothing changes: a cut from a sketch on
/// its face keeps the default scope (no bodies stored), cuts it, and leaves the part alone.
#[test]
fn a_sketch_on_a_top_level_body_beside_a_turned_part_cuts_only_that_body() {
    let (mut s, _) = turned_part(true);
    sketch_on_face(&mut s, solidcraft_geom::Vec3::new(-30.0, -30.0, 5.0), |s, c, _| {
        run(s, "sketch.circle.center", json!({"center": [c.x, c.y], "radius": 2}));
    });
    assert_eq!(crate::auto_operation(&s, &json!({"distance": -2})), Some("cut"));
    assert_eq!(crate::auto_operation(&s, &json!({"distance": 2})), Some("join"));
    let made = run(&mut s, "solid.extrude", json!({"distance": -2, "operation": "auto"}));
    let feature = s.doc.feature(made["feature"].as_u64().unwrap()).unwrap();
    assert!(feature.participants.is_empty(), "{:?}", feature.participants);
    let hole = std::f64::consts::PI * 4.0 * 2.0;
    assert!((body_volume(&s, "Base") - (2000.0 - hole)).abs() < 1e-3 * hole, "{}", body_volume(&s, "Base"));
    assert!((body_volume(&s, "Stock") - 1000.0).abs() < 1e-6);
}

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    match s.execute(id, &p) {
        Ok(v) => v,
        Err(e) => panic!("{id} {p}: {e}"),
    }
}

fn auto_emboss_params(mut p: Value, auto: bool) -> Value {
    if auto {
        if let Some(params) = p.get_mut("params").and_then(Value::as_object_mut) {
            params.remove("face");
        } else if let Some(object) = p.as_object_mut() {
            object.remove("face");
        }
    }
    p
}

fn measure(s: &mut Session) -> Value {
    run(s, "inspect.measure", json!({}))
}

fn volume(s: &mut Session) -> f64 {
    measure(s)["total"]["volume_mm3"].as_f64().unwrap_or(f64::NAN)
}

#[test]
fn separated_profile_cut_preserves_participants_and_removes_only_consumed_bodies() {
    for explicit in [false, true] {
        let mut s = Session::default();
        for (name, x) in [("A", 0), ("B", 10), ("C", 30)] {
            run(&mut s, "solid.box", json!({"length":5,"width":5,"height":10,"corner":[x,0,0],"body_name":name}));
        }
        run(&mut s, "sketch.create", json!({"plane":"XY","name":"Cutters"}));
        for (a, b) in [([-1, -1], [6, 6]), ([12, 2], [14, 4]), ([32, 2], [34, 4])] {
            run(&mut s, "sketch.rectangle.two_point", json!({"p0":a,"p1":b}));
        }
        run(&mut s, "sketch.finish", json!({}));
        let before = s.doc.to_json();
        let mut params = json!({"sketch":"Cutters","profiles":"all","distance":10,"operation":"cut"});
        if explicit {
            params["participants"] = json!(["A", "B"]);
        }
        run(&mut s, "solid.extrude", params);
        let check = |session: &Session| {
            let state = session.model.state();
            assert_eq!(state.bodies.len(), 2);
            assert!(state.body("A").is_none());
            for (name, expected) in [("B", 210.), ("C", if explicit { 250. } else { 210. })] {
                let body = &state.body(name).unwrap().body;
                assert!((solidcraft_kernel::measure(body).unwrap().volume - expected).abs() < 1e-6);
                assert!(body.validity().is_empty());
            }
        };
        check(&s);
        check(&Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap()));
        run(&mut s, "edit.undo", json!({}));
        assert_eq!(s.doc.to_json(), before);
        run(&mut s, "edit.redo", json!({}));
        check(&s);
    }
}

fn rel(a: f64, b: f64) -> f64 {
    (a - b).abs() / b.abs().max(1e-12)
}

/// An L bracket: a 40 x 20 x 5 base and a 5 x 20 x 40 wall at x = 0..5.
fn bracket() -> Session {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 40, "width": 20, "height": 5, "corner": [0, 0, 0], "body_name": "L"}));
    run(&mut s, "solid.box", json!({"length": 5, "width": 20, "height": 40, "corner": [0, 0, 0], "operation": "join"}));
    assert!(rel(volume(&mut s), 4000.0 + 3500.0) < 1e-9);
    // Mid-plane of the bracket (y = 10): sketch x = X, sketch y = Z.
    run(&mut s, "sketch.create", json!({"plane": {"origin": [0, 10, 0], "x_dir": [1, 0, 0], "y_dir": [0, 0, 1]}, "name": "RibSketch"}));
    run(&mut s, "sketch.line", json!({"points": [[5, 25], [25, 5]], "ids": ["l1"]}));
    run(&mut s, "sketch.finish", json!({}));
    s
}

#[test]
fn rib_to_the_next_face_fills_the_corner() {
    let mut s = bracket();
    run(&mut s, "solid.rib", json!({"sketch": "RibSketch", "curves": ["l1"], "thickness": 4}));
    // The triangle (5,5)-(5,25)-(25,5) of area 200, 4 thick.
    assert!(rel(volume(&mut s), 7500.0 + 800.0) < 1e-6, "{}", volume(&mut s));
    assert_eq!(measure(&mut s)["body_count"], 1);
    // The thickness is a parameter.
    let t = s.doc.features.last().and_then(|f| f.param_names.first().cloned()).unwrap_or_default();
    run(&mut s, "parameters.change", json!({"name": t, "expression": "2"}));
    assert!(rel(volume(&mut s), 7500.0 + 400.0) < 1e-6);
}

#[test]
fn rib_with_a_depth_and_web() {
    let mut s = bracket();
    run(&mut s, "solid.rib", json!({"sketch": "RibSketch", "curves": ["l1"], "thickness": 4, "depth": 2}));
    let v = volume(&mut s);
    assert!(v > 7500.0 + 100.0 && v < 7500.0 + 800.0, "{v}");
    let mut s = bracket();
    run(&mut s, "solid.web", json!({"sketch": "RibSketch", "curves": ["l1"], "thickness": 4}));
    assert!(rel(volume(&mut s), 8300.0) < 1e-6);
    // Curves that face no body.
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Far"}));
    run(&mut s, "sketch.line", json!({"points": [[50, 50], [60, 60]], "ids": ["l1"]}));
    run(&mut s, "sketch.finish", json!({}));
    let e = s.execute("solid.rib", &json!({"sketch": "Far", "curves": ["l1"], "thickness": 1}));
    assert!(e.is_err_and(|e| e.to_string().contains("do not face a body")));
}

#[test]
fn rib_along_the_sketch_normal_reaches_the_body() {
    // A 20 x 20 x 10 box; a line along X on a sketch 10 in front of it (y = -10), whose plane
    // the box does not cross, so only a rib along the sketch normal can reach it.
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 20, "width": 20, "height": 10, "corner": [0, 0, 0]}));
    run(&mut s, "sketch.create", json!({"plane": {"origin": [0, -10, 0], "x_dir": [1, 0, 0], "y_dir": [0, 0, 1]}, "name": "Front"}));
    run(&mut s, "sketch.line", json!({"points": [[2, 5], [18, 5]], "ids": ["l1"]}));
    run(&mut s, "sketch.finish", json!({}));
    let e = s.execute("solid.rib", &json!({"sketch": "Front", "curves": ["l1"], "thickness": 2}));
    assert!(e.is_err_and(|e| e.to_string().contains("do not face a body")));
    run(&mut s, "solid.rib", json!({"sketch": "Front", "curves": ["l1"], "thickness": 2, "direction": "normal"}));
    // 16 long, 2 thick, 10 from the sketch to the box.
    assert!(rel(volume(&mut s), 4000.0 + 320.0) < 1e-6, "{}", volume(&mut s));
    assert_eq!(measure(&mut s)["body_count"], 1);
    run(&mut s, "edit.undo", json!({}));
    run(&mut s, "solid.rib", json!({"sketch": "Front", "curves": ["l1"], "thickness": 2, "direction": "normal", "depth": 4}));
    assert!(rel(volume(&mut s), 4000.0 + 128.0) < 1e-6, "{}", volume(&mut s));
    let e = s.execute("solid.rib", &json!({"sketch": "Front", "curves": ["l1"], "thickness": 2, "direction": "sideways"}));
    assert!(e.is_err_and(|e| e.to_string().contains("unknown direction")));
}

#[test]
fn emboss_and_deboss_on_a_face() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 40, "width": 30, "height": 10}));
    run(&mut s, "sketch.create", json!({"plane": {"face": [20, 15, 10]}, "name": "Logo"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [5, 5], "p1": [15, 15]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.emboss", json!({"sketch": "Logo", "depth": 2}));
    assert!(rel(volume(&mut s), 12000.0 + 200.0) < 1e-6, "{}", volume(&mut s));
    run(&mut s, "edit.undo", json!({}));
    run(&mut s, "solid.emboss", json!({"sketch": "Logo", "depth": 3, "mode": "deboss"}));
    assert!(rel(volume(&mut s), 12000.0 - 300.0) < 1e-6);
    assert!(s.execute("solid.emboss", &json!({"sketch": "Logo", "depth": 1, "mode": "sideways"})).is_err());
}

#[test]
fn emboss_face_off_the_part_says_how_far() {
    // #392: a point beside the face was refused with "`face` must identify exactly one target
    // body surface", which didn't say the point was off the face.
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 40, "width": 30, "height": 10, "body_name": "A"}));
    run(&mut s, "sketch.create", json!({"plane": {"face": [20, 15, 10]}, "name": "Logo"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [5, 5], "p1": [15, 15]}));
    run(&mut s, "sketch.finish", json!({}));
    let e = s.execute("solid.emboss", &json!({"sketch": "Logo", "depth": 1, "face": [20, 15, 11.25]})).unwrap_err().to_string();
    assert!(e.contains("[20, 15, 11.25] lies on no face: the nearest, on `A`, is 1.250 mm away"), "{e}");
    run(&mut s, "solid.box", json!({"length": 10, "width": 30, "height": 10, "corner": [40, 0, 0], "operation": "new_body", "body_name": "B"}));
    let e = s.execute("solid.emboss", &json!({"sketch": "Logo", "depth": 1, "face": [40, 15, 5]})).unwrap_err().to_string();
    assert!(e.contains("lies on faces of 2 bodies (A, B): name one in `targets`"), "{e}");
    run(&mut s, "solid.emboss", json!({"sketch": "Logo", "depth": 1, "face": [20, 15, 10]}));
}

fn cylinder_badge() -> Session {
    let mut s = Session::default();
    run(&mut s, "parameters.add", json!({"name":"badge_depth","expression":"1 mm"}));
    run(&mut s, "solid.cylinder", json!({"radius":"10 mm","height":"20 mm","body_name":"Sleeve"}));
    run(&mut s, "sketch.create", json!({"plane":{"origin":[0,10,0],"x_dir":[-1,0,0],"y_dir":[0,0,1]},"name":"Badge"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0":[-2,5],"p1":[2,10]}));
    run(&mut s, "sketch.finish", json!({}));
    s
}

fn exact_cylinder_volume(s: &Session) -> f64 {
    s.model.state().body("Sleeve").unwrap().body.tessellate_with(1e-4, true).unwrap().measure().volume
}

#[test]
fn cylinder_emboss_persists_selection_and_replays_radial_depth() {
    use solidcraft_geom::Vec3;
    let mut s = cylinder_badge();
    let before = exact_cylinder_volume(&s);
    run(
        &mut s,
        "solid.emboss",
        json!({"sketch":"Badge","profiles":"all","face":[0,10,7.5],"depth":"badge_depth","targets":["Sleeve"],"name":"Raised Badge"}),
    );
    assert!((exact_cylinder_volume(&s) - before - 21.0).abs() < 0.02);
    let state = s.model.state();
    let body = &state.body("Sleeve").unwrap().body;
    assert_eq!(body.shell_count(), 1);
    let mesh = body.tessellate_with(0.001, true).unwrap();
    assert!(mesh.contains(Vec3::new(-10.5 * 0.15_f64.sin(), 10.5 * 0.15_f64.cos(), 7.5)));
    let feature = s.doc.find_feature("Raised Badge").unwrap();
    assert!(
        matches!(&feature.kind,solidcraft_doc::FeatureKind::Emboss{face:Some(face),..} if face.body=="Sleeve"&&face.at.dist(Vec3::new(0.,10.,7.5))<1e-10)
    );
    let saved = s.doc.to_json();
    let mut loaded = Session { doc: solidcraft_doc::Document::from_json(&saved).unwrap().into(), ..Default::default() };
    loaded.refresh();
    assert!(loaded.model.results.iter().all(|f| f.error.is_none()), "{:?}", loaded.model.results);
    assert!((exact_cylinder_volume(&loaded) - before - 21.0).abs() < 0.02);
    run(&mut loaded, "parameters.change", json!({"name":"badge_depth","expression":"2 mm"}));
    assert!(loaded.model.results.iter().all(|f| f.error.is_none()), "{:?}", loaded.model.results);
    assert!((exact_cylinder_volume(&loaded) - before - 44.0).abs() < 0.02);
}

#[test]
fn cylinder_deboss_is_connected_and_unsupported_profile_failure_is_atomic() {
    let mut s = cylinder_badge();
    let before = exact_cylinder_volume(&s);
    run(&mut s, "solid.emboss", json!({"sketch":"Badge","face":[0,10,7.5],"depth":1,"mode":"deboss","targets":["Sleeve"]}));
    assert!((exact_cylinder_volume(&s) - before + 19.0).abs() < 0.02);
    assert_eq!(s.model.state().body("Sleeve").unwrap().body.shell_count(), 1);
    let mut bad = cylinder_badge();
    run(&mut bad, "sketch.create", json!({"plane":{"origin":[0,10,0],"x_dir":[-1,0,0],"y_dir":[0,0,1]},"name":"Circle Badge"}));
    run(&mut bad, "sketch.circle.center", json!({"center":[0,7.5],"radius":1}));
    run(&mut bad, "sketch.finish", json!({}));
    let saved = bad.doc.to_json();
    let v = exact_cylinder_volume(&bad);
    assert!(bad.execute("solid.emboss", &json!({"sketch":"Circle Badge","face":[0,10,7.5],"depth":1,"targets":["Sleeve"]})).is_err());
    assert_eq!(bad.doc.to_json(), saved);
    assert_eq!(exact_cylinder_volume(&bad), v);
}

#[test]
fn cylinder_small_chord_pick_is_saved_on_the_exact_surface() {
    let mut s = Session::default();
    run(&mut s, "solid.cylinder", json!({"radius":"0.1mm","height":"0.2mm","body_name":"Tiny"}));
    run(&mut s, "sketch.create", json!({"plane":{"origin":[0,0.1,0],"x_dir":[-1,0,0],"y_dir":[0,0,1]},"name":"Badge"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0":[-0.02,0.05],"p1":[0.02,0.1]}));
    run(&mut s, "sketch.finish", json!({}));
    let mesh = s.model.state().bodies.first().unwrap().mesh();
    let pick = mesh
        .triangles
        .iter()
        .filter_map(|tri| mesh.tri(tri))
        .map(|[a, b, c]| (a + b + c) / 3.)
        .find(|p| p.y > 0.08 && p.z > 0.04 && p.z < 0.16)
        .unwrap();
    assert!(((pick.x * pick.x + pick.y * pick.y).sqrt() - 0.1).abs() > 1e-7);
    run(&mut s, "solid.emboss", json!({"sketch":"Badge","profiles":"all","face":[pick.x,pick.y,pick.z],"depth":"0.01mm","targets":["Tiny"]}));
    let solidcraft_doc::FeatureKind::Emboss { face: Some(face), .. } = &s.doc.features.last().unwrap().kind else {
        panic!("missing face");
    };
    assert!(((face.at.x * face.at.x + face.at.y * face.at.y).sqrt() - 0.1).abs() < 1e-12);
    assert_eq!(s.model.state().bodies.first().unwrap().body.shell_count(), 1);
}

#[test]
fn cylinder_cross_component_emboss_keeps_source_owner_and_placements() {
    for (pick_offset, auto) in [(0., false), (80., false), (0., true), (80., true)] {
        let run = |s: &mut Session, id: &str, p: Value| run(s, id, auto_emboss_params(p, auto && matches!(id, "solid.emboss" | "timeline.redefine")));
        let mut s = Session::default();
        let a = run(&mut s, "component.create", json!({"name":"Source","grounded":false}))["component"].as_u64().unwrap();
        run(&mut s, "solid.cylinder", json!({"radius":10,"height":20,"body_name":"Sleeve"}));
        let source_feature = s.model.state().bodies.first().unwrap().feature;
        let source_names = solidcraft_doc::naming::face_names(s.model.state().body("Sleeve").unwrap());
        let before = exact_cylinder_volume(&s);
        run(&mut s, "occurrence.move", json!({"occurrence":"Source:1","axis":[0,0,1],"angle":"90 deg","translate":[50,0,0]}));
        let second = run(&mut s, "occurrence.copy", json!({"component":a,"translate":[80,0,0]}))["occurrence"].as_u64().unwrap();
        run(&mut s, "component.activate", json!({"component":"root"}));
        let author = run(&mut s, "component.create", json!({"name":"Author"}))["component"].as_u64().unwrap();
        run(&mut s, "occurrence.move", json!({"occurrence":"Author:1","axis":[1,0,0],"angle":"90 deg","translate":[-100,40,30]}));
        let author_second = run(&mut s, "occurrence.copy", json!({"component":author,"translate":[0,100,0]}))["occurrence"].as_u64().unwrap();
        run(&mut s, "component.activate", json!({"component":author,"occurrence":author_second}));
        run(&mut s, "parameters.add", json!({"name":"badge_depth","expression":"1mm"}));
        run(&mut s, "sketch.create", json!({"plane":{"origin":[40.+pick_offset,0,0],"x_dir":[0,-1,0],"y_dir":[0,0,1]},"name":"Badge"}));
        run(&mut s, "sketch.rectangle.two_point", json!({"p0":[-2,5],"p1":[2,10]}));
        run(&mut s, "sketch.finish", json!({}));
        let placements = serde_json::to_value(&s.doc.occurrences).unwrap();
        let target = s
            .world_state()
            .bodies
            .iter()
            .find(|b| b.body.tessellate_with(0.001, true).unwrap().contains(solidcraft_geom::Vec3::new(40.5 + pick_offset, 0., 7.5)))
            .unwrap()
            .name
            .clone();
        run(
            &mut s,
            "solid.emboss",
            json!({"sketch":"Badge","profiles":"all","face":[40.+pick_offset,0,7.5],"depth":"badge_depth","targets":[target.clone()]}),
        );
        let solidcraft_doc::FeatureKind::Emboss { face: Some(face), .. } = &s.doc.features.last().unwrap().kind else {
            panic!("missing selection");
        };
        assert_eq!(face.path, if pick_offset == 0. { s.doc.first_path(a) } else { vec![second] });
        assert_eq!(face.author_path, vec![author_second]);
        let state = s.model.state();
        let body = state.body("Sleeve").unwrap();
        assert_eq!(body.feature, source_feature);
        assert_eq!(s.doc.body_component("Sleeve", body.feature), a);
        assert_eq!(serde_json::to_value(&s.doc.occurrences).unwrap(), placements);
        let selected = s.doc.features.last().unwrap().face_names.first().unwrap();
        assert!(!selected.is_empty() && source_names.contains(selected), "{selected:?} not in {source_names:?}");
        assert!((exact_cylinder_volume(&s) - before - 21.).abs() < 0.02);
        let world = s.world_state();
        for x in [39.5, 119.5] {
            assert!(world.bodies.iter().any(|b| b.body.tessellate_with(0.001, true).unwrap().contains(solidcraft_geom::Vec3::new(x, 0., 7.5))));
        }
        let mut loaded = Session { doc: solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap().into(), ..Default::default() };
        loaded.refresh();
        assert!(loaded.model.results.iter().all(|r| r.error.is_none()), "{:?}", loaded.model.results);
        run(&mut loaded, "parameters.change", json!({"name":"badge_depth","expression":"2mm"}));
        assert!((exact_cylinder_volume(&loaded) - before - 44.).abs() < 0.02);
        assert_eq!(loaded.doc.body_component("Sleeve", loaded.model.state().body("Sleeve").unwrap().feature), a);
        assert_eq!(serde_json::to_value(&loaded.doc.occurrences).unwrap(), placements);
        let feature = loaded.doc.features.last().unwrap().clone();
        run(&mut loaded, "component.activate", json!({"component":"root"}));
        run(
            &mut loaded,
            "timeline.redefine",
            json!({"feature":feature.id,"command":"solid.emboss","params":{"sketch":"Badge","profiles":"all","face":[40.+pick_offset,0,7.5],"depth":"badge_depth","targets":[target.clone()]}}),
        );
        assert_eq!(loaded.doc.feature(feature.id).unwrap().kind, feature.kind);
        assert_eq!(loaded.doc.feature(feature.id).unwrap().face_names, feature.face_names);
        run(
            &mut loaded,
            "timeline.redefine",
            json!({"feature":feature.id,"command":"solid.emboss","params":{"sketch":"Badge","profiles":"all","face":[40.+pick_offset,0,7.5],"depth":"3mm","targets":[target]}}),
        );
        assert!((exact_cylinder_volume(&loaded) - before - 69.).abs() < 0.02);
        let mut broken = loaded.doc.as_ref().clone();
        let id = broken.features.last().unwrap().id;
        let solidcraft_doc::FeatureKind::Emboss { face: Some(face), .. } = &mut broken.feature_mut(id).unwrap().kind else {
            panic!("missing selection");
        };
        face.path = vec![u64::MAX];
        let mut invalid = Session { doc: broken.into(), ..Default::default() };
        invalid.refresh();
        assert!(invalid.model.results.iter().find(|r| r.id == id).unwrap().error.as_ref().unwrap().contains("placement"));
        assert!((exact_cylinder_volume(&invalid) - before).abs() < 0.02);
    }
}

#[test]
fn cylinder_active_second_source_occurrence_authors_shared_geometry() {
    check_active_second_source(false);
    check_active_second_source(true);
}
fn check_active_second_source(auto: bool) {
    let run = |s: &mut Session, id: &str, p: Value| run(s, id, auto_emboss_params(p, auto && id == "solid.emboss"));
    let mut s = Session::default();
    let component = run(&mut s, "component.create", json!({"name":"Source"}))["component"].as_u64().unwrap();
    run(&mut s, "solid.cylinder", json!({"radius":10,"height":20,"body_name":"Sleeve"}));
    let before = exact_cylinder_volume(&s);
    let second = run(&mut s, "occurrence.copy", json!({"component":component,"translate":[80,0,0]}))["occurrence"].as_u64().unwrap();
    run(&mut s, "component.activate", json!({"component":component,"occurrence":second}));
    run(&mut s, "sketch.create", json!({"plane":{"origin":[80,10,0],"x_dir":[-1,0,0],"y_dir":[0,0,1]},"name":"Badge"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0":[-2,5],"p1":[2,10]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.emboss", json!({"sketch":"Badge","profiles":"all","face":[80,10,7.5],"depth":1}));
    assert!((exact_cylinder_volume(&s) - before - 21.).abs() < 0.02);
    let solidcraft_doc::FeatureKind::Emboss { face: Some(face), .. } = &s.doc.features.last().unwrap().kind else {
        panic!("missing selection");
    };
    assert_eq!(face.path, vec![second]);
    assert_eq!(face.author_path, vec![second]);
    let mut loaded = Session { doc: solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap().into(), ..Default::default() };
    loaded.refresh();
    assert!(loaded.model.results.iter().all(|r| r.error.is_none()), "{:?}", loaded.model.results);
    assert!((exact_cylinder_volume(&loaded) - before - 21.).abs() < 0.02);
}

#[test]
fn cylinder_pending_leaf_or_ancestor_positions_require_capture_atomically() {
    for (ancestor, auto) in [(false, false), (true, false), (false, true), (true, true)] {
        let mut s = Session::default();
        let parent = run(&mut s, "component.create", json!({"name":"Parent","grounded":false}))["occurrence"].as_u64().unwrap();
        let leaf = run(&mut s, "component.create", json!({"name":"Source"}))["occurrence"].as_u64().unwrap();
        run(&mut s, "solid.cylinder", json!({"radius":10,"height":20,"body_name":"Sleeve"}));
        run(&mut s, "sketch.create", json!({"plane":{"origin":[0,10,0],"x_dir":[-1,0,0],"y_dir":[0,0,1]},"name":"Badge"}));
        run(&mut s, "sketch.rectangle.two_point", json!({"p0":[-2,5],"p1":[2,10]}));
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "occurrence.move", json!({"occurrence":if ancestor {parent}else{leaf},"translate":[5,0,0],"capture":false}));
        let before = s.doc.to_json();
        let pending = s.pending_moves.clone();
        let volume = exact_cylinder_volume(&s);
        let error = s
            .execute("solid.emboss", &auto_emboss_params(json!({"sketch":"Badge","profiles":"all","face":[5,10,7.5],"depth":1}), auto))
            .unwrap_err()
            .to_string();
        assert!(error.contains("Capture Position"), "{error}");
        assert_eq!(s.doc.to_json(), before);
        assert_eq!(s.pending_moves, pending);
        assert_eq!(exact_cylinder_volume(&s), volume);
    }
}

#[test]
fn cylinder_pending_third_component_profile_requires_capture_atomically() {
    check_pending_third_profile(false);
    check_pending_third_profile(true);
}
fn check_pending_third_profile(auto: bool) {
    let mut s = Session::default();
    run(&mut s, "solid.cylinder", json!({"radius":10,"height":20,"body_name":"Sleeve"}));
    let profile_occurrence = run(&mut s, "component.create", json!({"name":"ProfileOwner","grounded":false}))["occurrence"].as_u64().unwrap();
    run(&mut s, "sketch.create", json!({"plane":{"origin":[0,10,0],"x_dir":[-1,0,0],"y_dir":[0,0,1]},"name":"Badge"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0":[-2,5],"p1":[2,10]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "component.activate", json!({"component":"root"}));
    run(&mut s, "component.create", json!({"name":"Author"}));
    run(&mut s, "occurrence.move", json!({"occurrence":profile_occurrence,"translate":[5,0,0],"capture":false}));
    let before = s.doc.to_json();
    let pending = s.pending_moves.clone();
    let volume = exact_cylinder_volume(&s);
    let error = s
        .execute(
            "solid.emboss",
            &auto_emboss_params(json!({"sketch":"Badge","profiles":"all","face":[0,10,7.5],"depth":1,"targets":["Sleeve"]}), auto),
        )
        .unwrap_err()
        .to_string();
    assert!(error.contains("Capture Position"), "{error}");
    assert_eq!(s.doc.to_json(), before);
    assert_eq!(s.pending_moves, pending);
    assert_eq!(exact_cylinder_volume(&s), volume);
}

#[test]
fn cylinder_body_offset_keeps_exact_profile_and_anchor_frames() {
    check_body_offset(false);
    check_body_offset(true);
}
fn check_body_offset(auto: bool) {
    let run = |s: &mut Session, id: &str, p: Value| run(s, id, auto_emboss_params(p, auto && id == "solid.emboss"));
    let mut s = Session::default();
    run(&mut s, "solid.cylinder", json!({"radius":10,"height":20,"body_name":"Sleeve"}));
    let before = exact_cylinder_volume(&s);
    let holder = run(&mut s, "component.create", json!({"name":"Holder","grounded":false}))["component"].as_u64().unwrap();
    run(&mut s, "occurrence.move", json!({"occurrence":"Holder:1","translate":[0,50,0]}));
    std::sync::Arc::make_mut(&mut s.doc).body_components.insert("Sleeve".into(), holder);
    std::sync::Arc::make_mut(&mut s.doc)
        .body_offsets
        .insert("Sleeve".into(), [[1., 0., 0., 0.], [0., 1., 0., 0.], [0., 0., 1., 0.], [0., -50., 0., 1.]]);
    s.refresh();
    assert!(s.doc.body_offsets.contains_key("Sleeve"));
    let offset = s.doc.body_offsets["Sleeve"];
    run(&mut s, "sketch.create", json!({"plane":{"origin":[0,10,0],"x_dir":[-1,0,0],"y_dir":[0,0,1]},"name":"Badge"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0":[-2,5],"p1":[2,10]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.emboss", json!({"sketch":"Badge","profiles":"all","face":[0,10,7.5],"depth":1,"targets":["Sleeve"]}));
    assert!((exact_cylinder_volume(&s) - before - 21.).abs() < 0.02);
    assert_eq!(s.doc.body_offsets["Sleeve"], offset);
    assert_eq!(s.doc.body_component("Sleeve", s.model.state().body("Sleeve").unwrap().feature), holder);
    assert!(s.world_state().body("Sleeve").unwrap().body.tessellate_with(0.001, true).unwrap().contains(solidcraft_geom::Vec3::new(0., 10.5, 7.5)));
}

#[test]
fn replace_face_align_remove() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "body_name": "A"}));
    run(&mut s, "solid.replace_face", json!({"faces": [[5, 5, 10]], "target": {"origin": [0, 0, 15], "normal": [0, 0, 1]}}));
    assert!(rel(volume(&mut s), 1500.0) < 1e-6);
    let e = s.execute("solid.replace_face", &json!({"faces": [[5, 5, 15]], "target": {"origin": [0, 0, 15], "normal": [1, 0, 1]}}));
    assert!(e.is_err() || s.model.results.last().is_some_and(|r| r.error.as_deref().unwrap_or("").contains("parallel")));
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "body_name": "A"}));
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [50, 0, 0], "body_name": "B"}));
    // B's top face onto A's top face: B turns over and sits on A.
    run(&mut s, "solid.align", json!({"bodies": ["B"], "from_face": [55, 5, 10], "to_face": [5, 5, 10]}));
    let m = measure(&mut s);
    let b = m["bodies"].as_array().unwrap().iter().find(|b| b["name"] == "B").unwrap().clone();
    let (lo, hi) = (b["bbox"]["min"].clone(), b["bbox"]["max"].clone());
    for (got, want) in [(&lo[0], 0.0), (&lo[2], 10.0), (&hi[0], 10.0), (&hi[2], 20.0)] {
        assert!((got.as_f64().unwrap_or(f64::NAN) - want).abs() < 1e-6, "{b}");
    }
    // Points only: a plain move.
    run(&mut s, "solid.align", json!({"bodies": ["B"], "from": [0, 0, 10], "to": [0, 0, 30]}));
    let m = measure(&mut s);
    let b = m["bodies"].as_array().unwrap().iter().find(|b| b["name"] == "B").unwrap().clone();
    assert!((b["bbox"]["min"][2].as_f64().unwrap_or(0.0) - 30.0).abs() < 1e-6, "{b}");
    run(&mut s, "solid.remove", json!({"bodies": ["B"]}));
    assert_eq!(measure(&mut s)["body_count"], 1);
}

#[test]
fn align_can_keep_the_offset_to_chosen_axes() {
    let aligned = |axes: Option<Value>| {
        let mut s = Session::default();
        run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "body_name": "A"}));
        run(&mut s, "solid.box", json!({"corner": [50, 20, 30], "length": 10, "width": 10, "height": 10, "body_name": "B"}));
        let mut p = json!({"bodies": ["A"], "from": [0, 0, 0], "to": [50, 20, 30]});
        if let Some(axes) = axes {
            p["axes"] = axes;
        }
        run(&mut s, "solid.align", p);
        s
    };
    let min = |s: &mut Session| {
        let m = measure(s);
        let a = m["bodies"].as_array().unwrap().iter().find(|b| b["name"] == "A").unwrap().clone();
        let g = |i: usize| a["bbox"]["min"][i].as_f64().unwrap_or(f64::NAN);
        [g(0), g(1), g(2)]
    };
    let close = |got: [f64; 3], want: [f64; 3]| got.iter().zip(want).all(|(a, b)| (a - b).abs() < 1e-6);
    assert!(close(min(&mut aligned(Some(json!([false, false, true])))), [0.0, 0.0, 30.0]));
    assert!(close(min(&mut aligned(Some(json!([true, false, false])))), [50.0, 0.0, 0.0]));
    assert!(close(min(&mut aligned(Some(json!([false, true, false])))), [0.0, 20.0, 0.0]));
    assert!(close(min(&mut aligned(Some(json!([true, true, true])))), [50.0, 20.0, 30.0]));
    assert!(close(min(&mut aligned(None)), [50.0, 20.0, 30.0]));

    // Refusal cases
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "body_name": "A"}));
    for axes in [json!([false, false, false]), json!([true, false]), json!([true, false, "yes"])] {
        assert!(s.execute("solid.align", &json!({"bodies": ["A"], "from": [0, 0, 0], "to": [1, 2, 3], "axes": axes})).is_err(), "{axes}");
    }
    // Face/plane alignment cannot take an axis mask.
    assert!(
        s.execute("solid.align", &json!({"bodies": ["A"], "from_face": [5, 5, 10], "to_face": [5, 5, 20], "axes": [true, false, false]})).is_err()
    );

    // Normalisation: all axes true normalises to None in the saved feature
    let made = run(&mut s, "solid.align", json!({"bodies": ["A"], "from": [0, 0, 0], "to": [1, 2, 3], "axes": [true, true, true]}));
    let id = made["feature"].as_u64().expect("the feature id");
    let f = s.doc.feature(id).expect("the saved feature");
    assert!(matches!(&f.kind, solidcraft_doc::FeatureKind::Align { axes: None, .. }));
}

/// The z range and the x minimum of body `name`.
fn z_x(s: &mut Session, name: &str) -> (f64, f64, f64) {
    let m = measure(s);
    let b = m["bodies"].as_array().unwrap().iter().find(|b| b["name"] == name).unwrap().clone();
    let g = |k: &str, i: usize| b["bbox"][k][i].as_f64().unwrap_or(f64::NAN);
    (g("min", 2), g("max", 2), g("min", 0))
}

/// Align onto planes and the origin: a face goes flush onto an origin plane (turning to meet it),
/// onto an offset plane, a vertex onto the origin point, and a plane the bodies are aligned to
/// is followed when it moves.
#[test]
fn align_takes_planes_and_the_origin() {
    let boxed = || {
        let mut s = Session::default();
        run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [50, 0, 20], "body_name": "C"}));
        s
    };
    // The bottom face onto XY: it already faces down, so it only drops; nothing slides across.
    let mut s = boxed();
    run(&mut s, "solid.align", json!({"bodies": ["C"], "from_face": [55, 5, 20], "to_plane": "XY"}));
    let (lo, hi, x) = z_x(&mut s, "C");
    assert!(lo.abs() < 1e-6 && (hi - 10.0).abs() < 1e-6 && (x - 50.0).abs() < 1e-6, "{lo} {hi} {x}");
    // The top face onto XY: the body turns over (the face now faces down) and sits on the plane.
    let mut s = boxed();
    run(&mut s, "solid.align", json!({"bodies": ["C"], "from_face": [55, 5, 30], "to_plane": "XY"}));
    let (lo, hi, _) = z_x(&mut s, "C");
    assert!(lo.abs() < 1e-6 && (hi - 10.0).abs() < 1e-6, "{lo} {hi}");
    assert!(measure(&mut s)["bodies"][0]["bbox"]["max"][2].as_f64().is_some());
    // A vertex onto the origin point.
    let mut s = boxed();
    run(&mut s, "solid.align", json!({"bodies": ["C"], "from": {"snap": "vertex", "at": [50, 0, 20]}, "to": "origin"}));
    let (lo, _, x) = z_x(&mut s, "C");
    assert!(lo.abs() < 1e-6 && x.abs() < 1e-6, "{lo} {x}");
    // Onto an offset plane, and the bodies follow it when its distance changes.
    let mut s = boxed();
    run(&mut s, "parameters.add", json!({"name": "lift", "expression": "30 mm"}));
    run(&mut s, "construct.plane.offset", json!({"base": "XY", "offset": "lift", "name": "Shelf"}));
    run(&mut s, "solid.align", json!({"bodies": ["C"], "from_face": [55, 5, 20], "to_plane": "Shelf"}));
    assert!((z_x(&mut s, "C").0 - 30.0).abs() < 1e-6);
    run(&mut s, "parameters.change", json!({"name": "lift", "expression": "45 mm"}));
    assert!((z_x(&mut s, "C").0 - 45.0).abs() < 1e-6, "the bodies follow the plane");
    // A plane as the source: the bodies move with it onto a point.
    let mut s = boxed();
    run(&mut s, "solid.align", json!({"bodies": ["C"], "from_plane": "XY", "to": [0, 0, 7]}));
    assert!((z_x(&mut s, "C").0 - 27.0).abs() < 1e-6);
    // One side is a plane or a point, not both; an unknown plane is refused.
    assert!(s.execute("solid.align", &json!({"bodies": ["C"], "from_plane": "XY", "from": [0, 0, 0], "to": [0, 0, 7]})).is_err());
    assert!(s.execute("solid.align", &json!({"bodies": ["C"], "from": [0, 0, 0], "to_plane": "Nowhere"})).is_err());
}

#[test]
fn coil_and_loft_to_a_point() {
    use std::f64::consts::{PI, TAU};
    let mut s = Session::default();
    run(&mut s, "solid.coil", json!({"diameter": 40, "revolutions": 2, "pitch": 8, "section_size": 4}));
    assert!(rel(volume(&mut s), PI * 4.0 * TAU * 20.0 * 2.0) < 1e-3, "{}", volume(&mut s));
    // Height and revolutions give the pitch; a square section outside the diameter.
    let mut s = Session::default();
    run(
        &mut s,
        "solid.coil",
        json!({"diameter": 40, "revolutions": 1.5, "height": 15, "section": "square", "section_size": 2, "section_position": "outside", "axis": "X"}),
    );
    assert!(rel(volume(&mut s), 4.0 * TAU * 21.0 * 1.5) < 1e-3, "{}", volume(&mut s));
    assert!(s.execute("solid.coil", &json!({"diameter": 40, "pitch": 1, "revolutions": 3, "section_size": 4})).is_err(), "runs into itself");
    assert!(s.execute("solid.coil", &json!({"diameter": 40, "section_size": 4, "pitch": 8})).is_err(), "two of three");
    // Pyramid: a square lofted to a point.
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Base"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [-10, -10], "p1": [10, 10]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "sketch.create", json!({"plane": {"origin": [0, 0, 30], "x_dir": [1, 0, 0], "y_dir": [0, 1, 0]}, "name": "Top"}));
    run(&mut s, "sketch.point", json!({"point": [0, 0], "id": "apex"}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.loft", json!({"sections": [{"sketch": "Base"}, {"sketch": "Top", "point": "apex"}]}));
    assert!(rel(volume(&mut s), 400.0 * 30.0 / 3.0) < 1e-6);
    assert!(s.execute("solid.loft", &json!({"sections": [{"sketch": "Base"}, {"sketch": "Top", "point": "nope"}]})).is_err());
}

/// A square coil joined round a cylinder, its ends flush with the cylinder's end faces (#3088):
/// both tries as the bodies lie fail on those ends, and the six shifted tries after them did too,
/// at seconds each on the coil's walls, before the nudged tool was joined; that nudged join then
/// meshed both bodies at the finest step. The join now takes the nudge after the first two:
/// counted in the kernel's tries, not timed, so a busy machine cannot fail it.
#[test]
fn coil_joined_flush_with_a_cylinders_ends() {
    use std::f64::consts::{PI, TAU};
    let mut s = Session::default();
    run(&mut s, "parameters.change", json!({"name": "ID", "expression": 30, "unit": "mm"}));
    run(&mut s, "solid.cylinder", json!({"radius": 15, "height": 13, "base": [0, 0, 0], "body_name": "Body2"}));
    let coil = json!({"axis": "Z", "base": [0, 0, 2], "clockwise": true, "diameter": "ID+0.8", "height": 9, "operation": "join",
        "pitch": 9, "section": "square", "section_position": "inside", "section_size": 4, "targets": ["Body2"]});
    let r = solidcraft_kernel::with_attempt_budget(6, || s.execute("solid.coil", &coil));
    assert!(r.is_ok(), "{r:?}");
    // The cylinder and the turn of the coil's section outside it: 0.4 mm by 4 mm, round r = 15.2.
    let (v, want) = (volume(&mut s), PI * 15.0 * 15.0 * 13.0 + 0.4 * 4.0 * TAU * 15.2);
    assert!(rel(v, want) < 1e-3, "{v} {want}");
}

/// A 3 mm shelled basin (100 x 60 x 30, open top) with a fall lofted from its whole outline
/// 10 up down to a drain pad (`pad`: corners, 1 up), in either section order. The loft's top
/// edges lie on the outer walls; the cut used to fail for one order of the sections (an
/// `Option::unwrap()` panic inside the kernel, or "shell not oriented and closed").
fn basin_fall(pad: [f64; 4], reverse: bool, op: &str) -> (Session, std::result::Result<Value, String>) {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 100, "width": 60, "height": 30, "corner": [0, 0, 0]}));
    run(&mut s, "solid.shell", json!({"faces": [[50, 30, 30]], "thickness": 3}));
    run(&mut s, "sketch.create", json!({"plane": {"origin": [0, 0, 10], "x_dir": [1, 0, 0], "y_dir": [0, 1, 0]}, "name": "Rim"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [100, 60]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "sketch.create", json!({"plane": {"origin": [0, 0, 1], "x_dir": [1, 0, 0], "y_dir": [0, 1, 0]}, "name": "Pad"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [pad[0], pad[1]], "p1": [pad[2], pad[3]]}));
    run(&mut s, "sketch.finish", json!({}));
    let mut secs = vec![json!({"sketch": "Rim"}), json!({"sketch": "Pad"})];
    if reverse {
        secs.reverse();
    }
    let r = s.execute("solid.loft", &json!({"sections": secs, "operation": op})).map_err(|e| e.to_string());
    (s, r)
}

#[test]
fn loft_cut_through_a_shelled_basin_in_either_order() {
    // What is left after the cut: the basin less the loft's part of the floor and walls
    // (integrated over the height), the same for a pad in any corner.
    let want = 41582.028;
    for pad in [[80.0, 40.0, 90.0, 50.0], [10.0, 40.0, 20.0, 50.0], [10.0, 10.0, 20.0, 20.0], [80.0, 10.0, 90.0, 20.0]] {
        for reverse in [false, true] {
            let (mut s, r) = basin_fall(pad, reverse, "cut");
            assert!(r.is_ok(), "{pad:?} reverse {reverse}: {r:?}");
            let v = volume(&mut s);
            assert!((v - want).abs() < 1.0, "{pad:?} reverse {reverse}: {v} vs {want}");
            assert_eq!(measure(&mut s)["body_count"], 1);
        }
    }
    // As a new body and joined: a valid solid of positive volume either way.
    for op in ["new", "join"] {
        let mut vols = Vec::new();
        for reverse in [false, true] {
            let (mut s, r) = basin_fall([80.0, 40.0, 90.0, 50.0], reverse, op);
            assert!(r.is_ok(), "{op} reverse {reverse}: {r:?}");
            vols.push(volume(&mut s));
        }
        assert!(vols[0] > 42948.0 && rel(vols[0], vols[1]) < 1e-6, "{op}: {vols:?}");
    }
}

#[test]
fn cylinder_auto_inference_saves_a_profile_anchor_and_replays_both_depth_signs() {
    use solidcraft_geom::Vec3;
    for mode in ["emboss", "deboss"] {
        let mut s = cylinder_badge();
        let before = exact_cylinder_volume(&s);
        let initial = s.doc.to_json();
        run(&mut s, "solid.emboss", json!({"sketch":"Badge","profiles":"all","depth":"badge_depth","mode":mode,"name":"Auto Badge"}));
        let feature = s.doc.find_feature("Auto Badge").unwrap().clone();
        let solidcraft_doc::FeatureKind::Emboss { face: Some(face), targets, .. } = &feature.kind else {
            panic!("missing inferred association");
        };
        assert_eq!(targets, &["Sleeve"]);
        assert_eq!(face.body, "Sleeve");
        assert!(face.at.z > 0. && face.at.z < 20.);
        assert!((face.at.x.hypot(face.at.y) - 10.).abs() < 1e-10);
        assert!(!feature.face_names.first().unwrap().is_empty());
        let expected = if mode == "emboss" { 21. } else { -19. };
        assert!((exact_cylinder_volume(&s) - before - expected).abs() < 0.02);
        let state = s.model.state();
        let body = &state.body("Sleeve").unwrap().body;
        assert_eq!(body.shell_count(), 1);
        assert_eq!(
            body.tessellate_with(0.001, true).unwrap().contains(Vec3::new(0., if mode == "emboss" { 10.5 } else { 9.5 }, 7.5)),
            mode == "emboss"
        );
        run(&mut s, "edit.undo", json!({}));
        assert_eq!(s.doc.to_json(), initial);
        run(&mut s, "edit.redo", json!({}));
        assert_eq!(s.doc.find_feature("Auto Badge").unwrap().kind, feature.kind);
        let mut loaded = Session { doc: solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap().into(), ..Default::default() };
        loaded.refresh();
        assert!(loaded.model.results.iter().all(|r| r.error.is_none()), "{:?}", loaded.model.results);
        run(
            &mut loaded,
            "timeline.redefine",
            json!({"feature":feature.id,"command":"solid.emboss","params":{"sketch":"Badge","profiles":"all","depth":"badge_depth","mode":mode}}),
        );
        assert_eq!(loaded.doc.feature(feature.id).unwrap().kind, feature.kind);
        assert_eq!(loaded.doc.feature(feature.id).unwrap().face_names, feature.face_names);
        run(&mut loaded, "parameters.change", json!({"name":"badge_depth","expression":"2mm"}));
        assert!((exact_cylinder_volume(&loaded) - before - if mode == "emboss" { 44. } else { -36. }).abs() < 0.02);
    }
}

#[test]
fn cylinder_auto_inference_ignores_remote_finite_tangent_walls_in_each_body_order() {
    for remote_first in [false, true] {
        let mut s = Session::default();
        for (name, z) in if remote_first { [("Remote", 100.), ("Sleeve", 0.)] } else { [("Sleeve", 0.), ("Remote", 100.)] } {
            run(&mut s, "solid.cylinder", json!({"radius":10,"height":20,"base":[0,0,z],"body_name":name}));
        }
        run(&mut s, "solid.cylinder", json!({"radius":8,"height":10,"base":[0,0,120],"body_name":"Remote Step"}));
        run(&mut s, "solid.combine", json!({"target":"Remote","tools":["Remote Step"],"operation":"join"}));
        run(&mut s, "sketch.create", json!({"plane":{"origin":[0,10,0],"x_dir":[-1,0,0],"y_dir":[0,0,1]},"name":"Badge"}));
        run(&mut s, "sketch.rectangle.two_point", json!({"p0":[-2,5],"p1":[2,10]}));
        run(&mut s, "sketch.finish", json!({}));
        let before = exact_cylinder_volume(&s);
        let remote = s.model.state().body("Remote").unwrap().body.tessellate_with(1e-4, true).unwrap().measure().volume;
        run(&mut s, "solid.emboss", json!({"sketch":"Badge","depth":1}));
        assert!((exact_cylinder_volume(&s) - before - 21.).abs() < 0.02);
        assert_eq!(s.model.state().body("Remote").unwrap().body.tessellate_with(1e-4, true).unwrap().measure().volume, remote);
    }
}

#[test]
fn cylinder_auto_inference_retains_selected_holes_and_nested_islands() {
    use solidcraft_geom::Vec3;
    for mode in ["emboss", "deboss"] {
        let mut s = cylinder_badge();
        run(&mut s, "sketch.edit", json!({"sketch":"Badge"}));
        run(&mut s, "sketch.rectangle.two_point", json!({"p0":[-1,6],"p1":[1,9]}));
        run(&mut s, "sketch.rectangle.two_point", json!({"p0":[-0.5,6.5],"p1":[0.5,8.5]}));
        run(&mut s, "sketch.finish", json!({}));
        let before = exact_cylinder_volume(&s);
        run(&mut s, "solid.emboss", json!({"sketch":"Badge","profiles":[{"point":[1.5,7.5]},{"point":[0,7.5]}],"depth":1,"mode":mode}));
        let depth = if mode == "emboss" { 1. } else { -1. };
        let expected = 16. * (depth + depth * depth / 20.);
        assert!((exact_cylinder_volume(&s) - before - expected).abs() < 0.02);
        let st = s.model.state();
        let body = &st.body("Sleeve").unwrap().body;
        assert_eq!(body.shell_count(), 1);
        let mesh = body.tessellate_with(0.001, true).unwrap();
        for (s, raised) in [(0., true), (0.75, false), (1.5, true)] {
            let theta = s / 10.0_f64;
            let radius = if mode == "emboss" { 10.5 } else { 9.5 };
            assert_eq!(mesh.contains(Vec3::new(-radius * theta.sin(), radius * theta.cos(), 7.5)), if mode == "emboss" { raised } else { !raised });
        }
    }
}

#[test]
fn cylinder_auto_inference_refuses_ambiguity_curves_and_seam_crossing_atomically() {
    let mut s = cylinder_badge();
    run(&mut s, "solid.cylinder", json!({"radius":10,"height":20,"body_name":"Coincident"}));
    let before = s.doc.to_json();
    let error = s.execute("solid.emboss", &json!({"sketch":"Badge","depth":1})).unwrap_err().to_string();
    assert!(error.contains("multiple tangent cylinders"), "{error}");
    assert_eq!(s.doc.to_json(), before);
    run(&mut s, "solid.emboss", json!({"sketch":"Badge","depth":1,"targets":["Sleeve"]}));
    assert!(matches!(&s.doc.features.last().unwrap().kind,solidcraft_doc::FeatureKind::Emboss{face:Some(face),..} if face.body=="Sleeve"));
    for circle in [false, true] {
        let mut s = cylinder_badge();
        run(&mut s, "sketch.create", json!({"plane":{"origin":[0,10,0],"x_dir":[-1,0,0],"y_dir":[0,0,1]},"name":"Unsupported"}));
        if circle {
            run(&mut s, "sketch.circle.center", json!({"center":[0,7.5],"radius":1}));
        } else {
            run(&mut s, "sketch.rectangle.two_point", json!({"p0":[-20,5],"p1":[20,10]}));
        }
        run(&mut s, "sketch.finish", json!({}));
        let before = s.doc.to_json();
        let volume = exact_cylinder_volume(&s);
        let error = s.execute("solid.emboss", &json!({"sketch":"Unsupported","depth":1})).unwrap_err().to_string();
        assert!(error.contains(if circle { "orthogonal line" } else { "seam" }), "{error}");
        assert_eq!(s.doc.to_json(), before);
        assert_eq!(exact_cylinder_volume(&s), volume);
    }
}

/// The design from #396: a loft from a Ø12 circle on a 40 × 30 × 20 box's top to a hexagon 15 up.
fn loft_on_box(op: &str) -> Session {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    run(&mut s, "sketch.rectangle.center", json!({"center": [0, 0], "corner": [20, 15]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"distance": 20, "body_name": "Box"}));
    run(&mut s, "construct.plane.offset", json!({"base": "XY", "offset": 20, "name": "A"}));
    run(&mut s, "construct.plane.offset", json!({"base": "XY", "offset": 35, "name": "B"}));
    run(&mut s, "sketch.create", json!({"plane": "A", "name": "L0"}));
    run(&mut s, "sketch.circle.center", json!({"center": [0, 0], "diameter": 12}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "sketch.create", json!({"plane": "B", "name": "L1"}));
    run(&mut s, "sketch.polygon.inscribed", json!({"center": [0, 0], "radius": 4, "sides": 6}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.loft", json!({"sections": [{"sketch": "L0"}, {"sketch": "L1"}], "operation": op}));
    s
}

#[test]
fn loft_joined_onto_a_box_exports_one_connected_mesh() {
    // #396: one body, but its two shells only touched, and the STL came out in two pieces.
    let mut apart = loft_on_box("new_body");
    let m = measure(&mut apart);
    assert_eq!(m["body_count"], 2);
    let loft = m["bodies"][1]["volume_mm3"].as_f64().unwrap();
    let mut s = loft_on_box("join");
    let m = measure(&mut s);
    assert_eq!(m["body_count"], 1);
    assert_eq!(m["bodies"][0]["kernel"]["shells"], 1);
    let v = volume(&mut s);
    assert!(rel(v, 24000.0 + loft) < 1e-5, "{v} vs {}", 24000.0 + loft);
    // The exported mesh, its vertices merged, is one connected piece.
    let mesh = s.model.state().body("Box").unwrap().body.tessellate(0.01).unwrap();
    let (positions, triangles) = solidcraft_io::weld(&mesh.positions, &mesh.triangles, 1e-4);
    let mut piece: Vec<usize> = (0..positions.len()).collect();
    let root = |piece: &mut Vec<usize>, mut i: usize| {
        while piece[i] != i {
            i = piece[i];
        }
        i
    };
    for t in &triangles {
        let r0 = root(&mut piece, t[0] as usize);
        for k in [1, 2] {
            let rk = root(&mut piece, t[k] as usize);
            piece[rk] = r0;
        }
    }
    let pieces: std::collections::BTreeSet<usize> = triangles.iter().map(|t| root(&mut piece, t[0] as usize)).collect();
    assert_eq!(pieces.len(), 1);
}

fn loft_between_blocks() -> (Session, Value) {
    let mut s = Session::default();
    for (name, z) in [("Lower", -5), ("Upper", 15)] {
        run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 5, "corner": [0, 0, z], "body_name": name}));
    }
    for (name, z) in [("Start", -2), ("End", 18)] {
        run(&mut s, "sketch.create", json!({"plane": "XY", "offset": z, "name": name}));
        run(&mut s, "sketch.rectangle.two_point", json!({"p0": [2, 2], "p1": [8, 8]}));
        run(&mut s, "sketch.finish", json!({}));
    }
    (s, json!({"sections": [{"sketch": "Start"}, {"sketch": "End"}], "operation": "join"}))
}

fn assert_loft_leaves_upper_block(s: &Session) {
    let st = s.model.state();
    assert_eq!(st.bodies.len(), 2, "a hidden body must not be consumed by Join (#1988)");
    for (name, expected) in [("Lower", 1148.0), ("Upper", 500.0)] {
        let body = &st.body(name).unwrap().body;
        let got = solidcraft_kernel::measure(body).unwrap().volume;
        assert!((got - expected).abs() < 1e-6, "{name}: {got} vs {expected}");
        assert!(body.validity().is_empty());
    }
}

#[test]
fn loft_join_keeps_hidden_bodies_and_persists_its_targets() {
    let (mut s, params) = loft_between_blocks();
    run(&mut s, "browser.visibility", json!({"items": ["Upper"], "visible": false}));
    run(&mut s, "solid.loft", params);
    assert_loft_leaves_upper_block(&s);
    run(&mut s, "edit.undo", json!({}));
    assert!((volume(&mut s) - 1000.0).abs() < 1e-6);
    run(&mut s, "edit.redo", json!({}));
    assert_loft_leaves_upper_block(&s);
    // Visibility is only consulted when the feature is made, never when it is rebuilt.
    run(&mut s, "browser.visibility", json!({"items": ["Upper"], "visible": true}));
    let reopened = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
    assert!(reopened.model.results.iter().all(|r| r.error.is_none()));
    assert_loft_leaves_upper_block(&reopened);
}

#[test]
fn loft_join_with_only_hidden_targets_is_refused_atomically() {
    for hide in [json!({"items": ["Lower", "Upper"], "visible": false}), json!({"tree_folders": ["c0/bodies"], "visible": false})] {
        let (mut s, params) = loft_between_blocks();
        run(&mut s, "browser.visibility", hide);
        let before = s.doc.to_json();
        let err = s.execute("solid.loft", &params).unwrap_err().to_string();
        assert!(err.contains("no visible body to join"), "{err}");
        assert_eq!(s.doc.to_json(), before);
        assert!((volume(&mut s) - 1000.0).abs() < 1e-6);
    }
}

#[test]
fn loft_join_honours_explicit_hidden_targets_and_participants() {
    for (key, targets) in [("targets", json!(["Lower", "Upper"])), ("participants", json!(["Lower", "Upper"])), ("participants", json!(["*"]))] {
        let (mut s, mut params) = loft_between_blocks();
        run(&mut s, "browser.visibility", json!({"items": ["Upper"], "visible": false}));
        params[key] = targets;
        run(&mut s, "solid.loft", params);
        assert_eq!(s.model.state().bodies.len(), 1);
        assert!((volume(&mut s) - 1540.0).abs() < 1e-6);
    }
    let (mut s, params) = loft_between_blocks();
    run(&mut s, "solid.loft", params);
    assert_eq!(s.model.state().bodies.len(), 1, "all-visible Join keeps its automatic scope");
    assert!((volume(&mut s) - 1540.0).abs() < 1e-6);
}

#[test]
fn loft_new_body_still_works_when_every_body_is_hidden() {
    let (mut s, mut params) = loft_between_blocks();
    run(&mut s, "browser.visibility", json!({"tree_folders": ["c0/bodies"], "visible": false}));
    params["operation"] = json!("new_body");
    run(&mut s, "solid.loft", params);
    assert_eq!(s.model.state().bodies.len(), 3);
    assert!((volume(&mut s) - 1720.0).abs() < 1e-6);
}

#[test]
fn loft_join_visibility_does_not_expand_its_component_scope() {
    let (mut s, params) = loft_between_blocks();
    let part = run(&mut s, "component.create", json!({"name": "Part", "activate": false}));
    run(&mut s, "component.move_bodies", json!({"bodies": ["Lower", "Upper"], "component": part["component"]}));
    run(&mut s, "browser.visibility", json!({"items": ["Upper"], "visible": false}));
    run(&mut s, "solid.loft", params);
    assert_eq!(s.model.state().bodies.len(), 3, "a plain-plane loft in the empty root still creates its own body");
    assert!((volume(&mut s) - 1720.0).abs() < 1e-6);
}

#[test]
fn loft_join_does_not_fall_back_from_a_hidden_sketch_face_body() {
    let (mut s, mut params) = loft_between_blocks();
    run(&mut s, "sketch.create", json!({"plane": {"face": [5, 5, 0]}, "project_edges": false, "name": "OnLower"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [2, 2], "p1": [8, 8]}));
    run(&mut s, "sketch.finish", json!({}));
    params["sections"][0]["sketch"] = json!("OnLower");
    run(&mut s, "browser.visibility", json!({"items": ["Lower"], "visible": false}));
    let before = s.doc.to_json();
    let err = s.execute("solid.loft", &params).unwrap_err().to_string();
    assert!(err.contains("no visible body to join"), "{err}");
    assert_eq!(s.doc.to_json(), before);
}

/// A cone that takes only a 1 mm chamfer off a cylinder's top edge leaves nearly all of the
/// cylinder inside it; the intersection is still the chamfered cylinder (#443).
#[test]
fn loft_intersect_that_trims_one_edge_changes_the_body() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    run(&mut s, "sketch.circle.center", json!({"center": [0, 0], "radius": 35}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"distance": 10, "body_name": "Body"}));
    for (name, offset, radius) in [("A", -1, 45), ("B", 11, 33)] {
        run(&mut s, "sketch.create", json!({"plane": "XY", "offset": offset, "name": name}));
        run(&mut s, "sketch.circle.center", json!({"center": [0, 0], "radius": radius}));
        run(&mut s, "sketch.finish", json!({}));
    }
    run(&mut s, "solid.loft", json!({"sections": [{"sketch": "A"}, {"sketch": "B"}], "operation": "intersect"}));
    // r 35 up to z = 9, then a frustum to r 34 at z = 10.
    let want = std::f64::consts::PI * (35.0 * 35.0 * 9.0 + (35.0 * 35.0 + 35.0 * 34.0 + 34.0 * 34.0) / 3.0);
    let got = volume(&mut s);
    assert!((got - want).abs() < want * 2e-4, "{got} vs {want}");
    let m = measure(&mut s);
    assert_eq!(m["bodies"][0]["face_types"]["cone"], 1, "{m}");
}

/// #623: a moulded front frame (rounded block, recess, through opening, four pockets from the back, two joined ribs,
/// one joined Ø7 boss) and a Ø3.2 x 8 blind bore into the boss. Reduced from the issue's script; on 0.7.0 the bore
/// fails on every run with "lumps: cannot mesh a complete shell boundary" (the original failed on about half).
const RECIPE_623: &str = r#"{"commands":[{"command":"component.activate","params":{"component":"root"}},{"command":"sketch.create","params":{"plane":{"origin":[466.0,0,0],"x_dir":[0,1,0],"y_dir":[0,0,1]},"name":"PL01 Outline Sketch"}},{"command":"sketch.line","params":{"points":[[9.0,17.35],[231.0,17.35]],"closed":false}},{"command":"sketch.arc.center_point","params":{"center":[231.0,25.5],"start":[231.0,17.35],"end":[239.15,25.5]}},{"command":"sketch.line","params":{"points":[[239.15,25.5],[239.15,500.0]],"closed":false}},{"command":"sketch.arc.center_point","params":{"center":[231.0,500.0],"start":[239.15,500.0],"end":[231.0,508.15]}},{"command":"sketch.line","params":{"points":[[231.0,508.15],[9.0,508.15]],"closed":false}},{"command":"sketch.arc.center_point","params":{"center":[9.0,500.0],"start":[9.0,508.15],"end":[0.8500000000000001,500.0]}},{"command":"sketch.line","params":{"points":[[0.8500000000000001,500.0],[0.8500000000000001,25.5]],"closed":false}},{"command":"sketch.arc.center_point","params":{"center":[9.0,25.5],"start":[0.8500000000000001,25.5],"end":[9.0,17.35]}},{"command":"sketch.finish","params":{}},{"command":"solid.extrude","params":{"sketch":"PL01 Outline Sketch","distance":30.0,"operation":"new","direction":"positive","name":"PL01 Outline","body_name":"PL01 Front Frame"}},{"command":"sketch.create","params":{"plane":{"origin":[496.0,0,0],"x_dir":[0,1,0],"y_dir":[0,0,1]},"name":"PL01 Recess Sketch"}},{"command":"sketch.line","params":{"points":[[10.0,22.5],[230.0,22.5]],"closed":false}},{"command":"sketch.arc.center_point","params":{"center":[230.0,26.5],"start":[230.0,22.5],"end":[234.0,26.5]}},{"command":"sketch.line","params":{"points":[[234.0,26.5],[234.0,499.0]],"closed":false}},{"command":"sketch.arc.center_point","params":{"center":[230.0,499.0],"start":[234.0,499.0],"end":[230.0,503.0]}},{"command":"sketch.line","params":{"points":[[230.0,503.0],[10.0,503.0]],"closed":false}},{"command":"sketch.arc.center_point","params":{"center":[10.0,499.0],"start":[10.0,503.0],"end":[6.0,499.0]}},{"command":"sketch.line","params":{"points":[[6.0,499.0],[6.0,26.5]],"closed":false}},{"command":"sketch.arc.center_point","params":{"center":[10.0,26.5],"start":[6.0,26.5],"end":[10.0,22.5]}},{"command":"sketch.finish","params":{}},{"command":"solid.extrude","params":{"sketch":"PL01 Recess Sketch","distance":12.0,"operation":"cut","direction":"negative","name":"PL01 Recess","targets":["PL01 Front Frame"]}},{"command":"sketch.create","params":{"plane":{"origin":[484.0,0,0],"x_dir":[0,1,0],"y_dir":[0,0,1]},"name":"PL01 Opening Sketch"}},{"command":"sketch.line","params":{"points":[[20.5,36.0],[219.5,36.0]],"closed":false}},{"command":"sketch.arc.center_point","params":{"center":[219.5,38.0],"start":[219.5,36.0],"end":[221.5,38.0]}},{"command":"sketch.line","params":{"points":[[221.5,38.0],[221.5,488.5]],"closed":false}},{"command":"sketch.arc.center_point","params":{"center":[219.5,488.5],"start":[221.5,488.5],"end":[219.5,490.5]}},{"command":"sketch.line","params":{"points":[[219.5,490.5],[20.5,490.5]],"closed":false}},{"command":"sketch.arc.center_point","params":{"center":[20.5,488.5],"start":[20.5,490.5],"end":[18.5,488.5]}},{"command":"sketch.line","params":{"points":[[18.5,488.5],[18.5,38.0]],"closed":false}},{"command":"sketch.arc.center_point","params":{"center":[20.5,38.0],"start":[18.5,38.0],"end":[20.5,36.0]}},{"command":"sketch.finish","params":{}},{"command":"solid.extrude","params":{"sketch":"PL01 Opening Sketch","distance":18.0,"operation":"cut","direction":"negative","name":"PL01 Opening","targets":["PL01 Front Frame"]}},{"command":"sketch.create","params":{"plane":{"origin":[466.0,0,0],"x_dir":[0,1,0],"y_dir":[0,0,1]},"name":"PL01 Hollow 1 Sketch"}},{"command":"sketch.rectangle.two_point","params":{"p0":[3.35,28.85],"p1":[16.0,496.65]}},{"command":"sketch.finish","params":{}},{"command":"solid.extrude","params":{"sketch":"PL01 Hollow 1 Sketch","distance":15.5,"operation":"cut","direction":"positive","name":"PL01 Hollow 1","targets":["PL01 Front Frame"]}},{"command":"sketch.create","params":{"plane":{"origin":[466.0,0,0],"x_dir":[0,1,0],"y_dir":[0,0,1]},"name":"PL01 Hollow 2 Sketch"}},{"command":"sketch.rectangle.two_point","params":{"p0":[224.0,28.85],"p1":[236.65,496.65]}},{"command":"sketch.finish","params":{}},{"command":"solid.extrude","params":{"sketch":"PL01 Hollow 2 Sketch","distance":15.5,"operation":"cut","direction":"positive","name":"PL01 Hollow 2","targets":["PL01 Front Frame"]}},{"command":"sketch.create","params":{"plane":{"origin":[466.0,0,0],"x_dir":[0,1,0],"y_dir":[0,0,1]},"name":"PL01 Hollow 3 Sketch"}},{"command":"sketch.rectangle.two_point","params":{"p0":[12.35,19.85],"p1":[227.65,33.5]}},{"command":"sketch.finish","params":{}},{"command":"solid.extrude","params":{"sketch":"PL01 Hollow 3 Sketch","distance":15.5,"operation":"cut","direction":"positive","name":"PL01 Hollow 3","targets":["PL01 Front Frame"]}},{"command":"sketch.create","params":{"plane":{"origin":[466.0,0,0],"x_dir":[0,1,0],"y_dir":[0,0,1]},"name":"PL01 Hollow 4 Sketch"}},{"command":"sketch.rectangle.two_point","params":{"p0":[12.35,493.0],"p1":[227.65,505.65]}},{"command":"sketch.finish","params":{}},{"command":"solid.extrude","params":{"sketch":"PL01 Hollow 4 Sketch","distance":15.5,"operation":"cut","direction":"positive","name":"PL01 Hollow 4","targets":["PL01 Front Frame"]}},{"command":"sketch.create","params":{"plane":{"origin":[471.0,0,0],"x_dir":[0,1,0],"y_dir":[0,0,1]},"name":"PL01 Rib 1 Sketch"}},{"command":"sketch.rectangle.two_point","params":{"p0":[16.0,69.0],"p1":[224.0,73.0]}},{"command":"sketch.finish","params":{}},{"command":"solid.extrude","params":{"sketch":"PL01 Rib 1 Sketch","distance":12.0,"operation":"join","direction":"positive","name":"PL01 Rib 1","targets":["PL01 Front Frame"]}},{"command":"sketch.create","params":{"plane":{"origin":[471.0,0,0],"x_dir":[0,1,0],"y_dir":[0,0,1]},"name":"PL01 Rib 6 Sketch"}},{"command":"sketch.rectangle.two_point","params":{"p0":[16.0,470.0],"p1":[224.0,474.0]}},{"command":"sketch.finish","params":{}},{"command":"solid.extrude","params":{"sketch":"PL01 Rib 6 Sketch","distance":12.0,"operation":"join","direction":"positive","name":"PL01 Rib 6","targets":["PL01 Front Frame"]}},{"command":"sketch.create","params":{"plane":{"origin":[481.5,0,0],"x_dir":[0,1,0],"y_dir":[0,0,1]},"name":"PL01 Boss 4 Sketch"}},{"command":"sketch.circle.center","params":{"center":[227.5,492.0],"radius":3.5}},{"command":"sketch.finish","params":{}},{"command":"solid.extrude","params":{"sketch":"PL01 Boss 4 Sketch","distance":24.5,"operation":"join","direction":"negative","name":"PL01 Boss 4","targets":["PL01 Front Frame"]}},{"command":"sketch.create","params":{"plane":{"origin":[457.0,0,0],"x_dir":[0,1,0],"y_dir":[0,0,1]},"name":"B"}},{"command":"sketch.circle.center","params":{"center":[227.5,492.0],"radius":1.6}},{"command":"sketch.finish","params":{}},{"command":"solid.extrude","params":{"sketch":"B","distance":8.0,"operation":"cut","direction":"positive","name":"B","targets":["PL01 Front Frame"]}}]}"#;

#[test]
fn blind_bore_into_a_joined_boss_meshes() {
    let recipe: Value = serde_json::from_str(RECIPE_623).unwrap();
    let mut s = Session::default();
    for c in recipe["commands"].as_array().unwrap() {
        run(&mut s, c["command"].as_str().unwrap(), c["params"].clone());
    }
    let m = measure(&mut s);
    assert_eq!(m["bodies"].as_array().map(Vec::len), Some(1), "{m}");
}

#[test]
fn cutting_a_text_removes_each_letter_whether_its_letters_stand_apart_or_touch() {
    // Letters apart (cut at once by all of them), and two touching blocks (cut one after the other).
    for (text, touching) in [("SOLIDCRAFT", false), ("", true)] {
        let mut s = Session::default();
        run(&mut s, "sketch.create", json!({"plane":"XY"}));
        run(&mut s, "sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[200,40]}));
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "solid.extrude", json!({"distance":10}));
        run(&mut s, "sketch.create", json!({"plane":"XY"}));
        if touching {
            run(&mut s, "sketch.rectangle.two_point", json!({"p0":[10,10],"p1":[20,20]}));
            run(&mut s, "sketch.rectangle.two_point", json!({"p0":[20,12],"p1":[30,18]}));
        } else {
            run(&mut s, "sketch.text", json!({"text":text,"at":[5,12],"height":14}));
        }
        let state = s.world_state();
        let ss = state.sketch(s.active_sketch.unwrap()).unwrap();
        // The profiles cut: the letters, not their counters (the inside of an O, whose outline
        // is a hole of its letter).
        let sorted = |ids: &[String]| {
            let mut ids = ids.to_vec();
            ids.sort();
            ids
        };
        let counter = |p: &solidcraft_sketch::Profile| ss.profiles.iter().any(|q| q.hole_curves.iter().any(|h| sorted(h) == sorted(&p.outer_curves)));
        let area: f64 = ss.profiles.iter().filter(|p| !counter(p)).map(|p| p.area).sum();
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "solid.extrude", json!({"distance":2,"operation":"cut"}));
        let volume = run(&mut s, "inspect.measure", json!({}))["total"]["volume_mm3"].as_f64().unwrap();
        assert!((volume - (200.0 * 40.0 * 10.0 - 2.0 * area)).abs() < 1e-3 * area, "{text:?}: {volume} for {area} mm² cut 2 mm deep");
        assert_eq!(s.world_state().bodies.len(), 1);
    }
}

/// A text joined onto a block with a taper: its letters are spline and conic outlines whose
/// sides slope, standing on the block's face (run on into it) or hanging over its edge (a
/// straight foot first). An outward taper adds a little more than the straight text; one
/// steeper than the letters' strokes is refused, saying so.
#[test]
fn a_tapered_text_joins_a_block_on_its_face_or_hanging_over_its_edge() {
    for (at, hanging) in [([5.0, 12.0], false), ([150.0, 12.0], true)] {
        let mut s = Session::default();
        run(&mut s, "sketch.create", json!({"plane":"XY"}));
        run(&mut s, "sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[200,40]}));
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "solid.extrude", json!({"distance":-10}));
        run(&mut s, "sketch.create", json!({"plane":"XY"}));
        run(&mut s, "sketch.text", json!({"text":"tres long","at":at,"height":8}));
        let state = s.world_state();
        let ss = state.sketch(s.active_sketch.unwrap()).unwrap();
        // The letters' area, counters left out (a letter's hole is its own profile too).
        let sorted = |ids: &[String]| {
            let mut ids = ids.to_vec();
            ids.sort();
            ids
        };
        let counter = |p: &solidcraft_sketch::Profile| ss.profiles.iter().any(|q| q.hole_curves.iter().any(|h| sorted(h) == sorted(&p.outer_curves)));
        let area: f64 = ss.profiles.iter().filter(|p| !counter(p)).map(|p| p.area).sum();
        run(&mut s, "sketch.finish", json!({}));
        let steep = s.execute("solid.extrude", &json!({"distance":2,"operation":"join","taper":"15 deg"})).unwrap_err();
        assert!(steep.to_string().contains("the taper closes the profile"), "{steep}");
        run(&mut s, "solid.extrude", json!({"distance":2,"operation":"join","taper":"3 deg"}));
        let volume = run(&mut s, "inspect.measure", json!({}))["total"]["volume_mm3"].as_f64().unwrap();
        let (block, text) = (200.0 * 40.0 * 10.0, 2.0 * area);
        assert!(volume > block + text && volume < block + 1.3 * text, "hanging {hanging}: {volume} for {text} mm³ of straight text");
        assert_eq!(s.world_state().bodies.len(), 1);
    }
}

/// A tapered pocket cut 5 mm down from a block's top face, from a 282 × 176 rounded rectangle whose
/// 2 mm corners are smaller than the 2.89 mm the 30° taper widens it by (#4825). Run on out of the
/// block with its slope, the tool narrowed there and closed its corner arcs: "the taper closes
/// the profile", though the pocket only grows. It runs on less, then starts on the face.
#[test]
fn a_tapered_cut_that_widens_cuts_from_small_corners() {
    let (w, h, r, depth) = (282.0, 176.0, 2.0, 5.0);
    for deg in [20.0f64, 30.0, 45.0] {
        let k = deg.to_radians().tan();
        let mut s = Session::default();
        run(&mut s, "sketch.create", json!({"plane":"XY"}));
        run(&mut s, "sketch.rectangle.two_point", json!({"p0":[-159,-106],"p1":[159,106]}));
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "solid.extrude", json!({"distance":20,"body_name":"Block"}));
        run(&mut s, "sketch.create", json!({"plane":"XY","offset":20}));
        run(&mut s, "sketch.rectangle.rounded", json!({"p0":[-w / 2.0,-h / 2.0],"p1":[w / 2.0,h / 2.0],"radius":r}));
        run(&mut s, "sketch.finish", json!({}));
        let cut = json!({"distance":depth,"direction":"negative","taper":format!("{deg} deg"),"operation":"cut","targets":["Block"]});
        run(&mut s, "solid.extrude", cut);
        // The pocket: a rounded rectangle grown by k·t at depth t.
        let (l, c) = (depth, 4.0 - std::f64::consts::PI);
        let pocket = w * h * l + k * (w + h) * l * l + 4.0 / 3.0 * k * k * l.powi(3) - c * (r * r * l + r * k * l * l + k * k * l.powi(3) / 3.0);
        let v = volume(&mut s);
        assert!((v - (318.0 * 212.0 * 20.0 - pocket)).abs() < 1e-3 * pocket, "{deg}°: {v}, a {pocket} mm³ pocket");
        assert_eq!(s.world_state().bodies.len(), 1);
    }
}

/// A tapered foot extruded 3 mm down from a part's bottom face and joined (#1534). Run on into
/// the part with their slope, sides that narrow the foot away from the face widened it past the
/// face: a box got a lip round its walls (0.16 mm wide, and nothing said), a cylinder's join found
/// no result, and sides that widen the foot met the face's edge there. A foot on the whole face is
/// sewn on along it; one drawn just inside the face's edge starts on it. Either way: the part plus
/// the frustum, and nothing beside the part's walls.
#[test]
fn a_tapered_foot_under_a_part_joins_as_drawn() {
    // (round, the foot's half width: the whole face, or just inside its edge)
    for (round, half) in [(false, 15.0), (true, 15.0), (false, 14.9)] {
        for taper in [-20.0f64, 20.0] {
            let t = taper.to_radians().tan();
            let mut s = Session::default();
            run(&mut s, "sketch.create", json!({"plane":"XY"}));
            if round {
                run(&mut s, "sketch.circle.center", json!({"center":[0,0],"radius":15}));
            } else {
                run(&mut s, "sketch.rectangle.two_point", json!({"p0":[-15,-15],"p1":[15,15]}));
            }
            run(&mut s, "sketch.finish", json!({}));
            run(&mut s, "solid.extrude", json!({"distance":10,"body_name":"B"}));
            let taper_deg = format!("{taper} deg");
            if half < 15.0 {
                run(&mut s, "sketch.create", json!({"plane":"XY"}));
                run(&mut s, "sketch.rectangle.two_point", json!({"p0":[-half,-half],"p1":[half,half]}));
                run(&mut s, "sketch.finish", json!({}));
                run(&mut s, "solid.extrude", json!({"distance":3,"direction":"negative","operation":"join","taper":taper_deg}));
            } else {
                let foot = json!({"face":[5,1,0],"face_body":"B","distance":3,"direction":"positive","operation":"join","taper":taper_deg});
                run(&mut s, "solid.extrude", foot);
            }
            let (part, frustum) = if round {
                let r = 15.0 + 3.0 * t;
                (std::f64::consts::PI * 225.0 * 10.0, std::f64::consts::PI * (225.0 + 15.0 * r + r * r))
            } else {
                let side = 2.0 * half + 6.0 * t;
                (9000.0, 4.0 * half * half + 2.0 * half * side + side * side)
            };
            let v = volume(&mut s);
            // The cylinder is measured on its mesh; a foot that starts on the face stands straight
            // for a hair first.
            let tol = if round || half < 15.0 { 2e-4 } else { 1e-9 } * (part + frustum);
            assert!((v - (part + frustum)).abs() < tol, "round {round}, half {half}, taper {taper}: {v} for {}", part + frustum);
            assert_eq!(s.world_state().bodies.len(), 1);
            let state = s.model.state();
            let mesh = state.body("B").unwrap().mesh();
            // Beside the part, above the foot's start: nothing.
            let out = |x: f64, y: f64| if round { x.hypot(y) } else { x.abs().max(y.abs()) } > 15.0 + 1e-6;
            let beside = mesh.positions.iter().filter(|p| p.z > 1e-6 && out(p.x, p.y)).count();
            assert_eq!(beside, 0, "round {round}, half {half}, taper {taper}: {:?}", mesh.bounds());
        }
    }
}

/// A drafted extrude that starts above its sketch plane. The start offset moves the profile
/// along the sketch normal and the walls are drafted from there, so the section at the start
/// plane is the profile itself and the solid is the frustum over it, moved along the normal by
/// the offset. Controls that build today: the same extrude with the taper and no offset, and
/// with the offset and no taper. Two-sided and symmetric tapers are still refused, by name.
#[test]
fn a_drafted_extrude_starts_at_an_offset() {
    use solidcraft_kernel::{FaceSurface, face_surface, measure as measure_body};
    // A 30 x 20 profile, 12 mm long, drafted 6 deg, started 4 mm along the normal.
    let (w, d, h, off) = (30.0, 20.0, 12.0, 4.0);
    let part = |p: Value| -> Session {
        let mut s = Session::default();
        run(&mut s, "sketch.create", json!({"plane":"XY"}));
        run(&mut s, "sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[w,d]}));
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "solid.extrude", p);
        s
    };
    // The straight prism, and the frustum over the profile: every edge moves out by z*tan a
    // over the start, so A(z) = (w + 2 z t)(d + 2 z t) and
    // V = integral of A over 0..h = w*d*h + (w + d)*t*h^2 + 4/3*t^2*h^3 (exact: A is quadratic).
    let prism = w * d * h;
    let frustum = |t: f64| w * d * h + (w + d) * t * h * h + 4.0 / 3.0 * t * t * h * h * h;
    // The two caps plus four trapezoid walls, each (base + top)/2 by the slant height h/cos a.
    let area_of = |a: f64| {
        let t = a.tan();
        w * d + (w + 2.0 * h * t) * (d + 2.0 * h * t) + 2.0 * h / a.cos() * ((w + h * t) + (d + h * t))
    };

    // Control: the taper alone, from the sketch plane.
    let want = frustum(6f64.to_radians().tan());
    let mut s = part(json!({"distance":h,"taper":"6 deg","body_name":"B"}));
    let v = volume(&mut s);
    assert!(rel(v, want) < 1e-9, "taper alone: {v} mm3 for {want}");

    // Control: the offset alone, no taper: the prism, moved along the normal.
    let mut s = part(json!({"distance":h,"start_offset":off,"body_name":"B"}));
    let v = volume(&mut s);
    assert!(rel(v, prism) < 1e-9, "offset alone: {v} mm3 for {prism}");
    let st = s.model.state();
    let m = measure_body(&st.body("B").unwrap().body).unwrap();
    assert!((m.bbox.min.z - off).abs() < 1e-9 && (m.bbox.max.z - (off + h)).abs() < 1e-9, "offset alone: {:?}", m.bbox);

    // Both: the frustum over the profile, standing on the offset plane, either way along the
    // normal and drafted either outward or inward.
    for deg in [6.0f64, -6.0] {
        for (dir, sign) in [("positive", 1.0f64), ("negative", -1.0)] {
            let (ang, t) = (deg.to_radians(), deg.to_radians().tan());
            let (v_want, a_want) = (frustum(t), area_of(ang));
            let at = format!("{deg} deg {dir}");
            let mut s = part(json!({"distance":h,"direction":dir,"start_offset":off,"taper":format!("{deg} deg"),"body_name":"B"}));
            let v = volume(&mut s);
            assert!(rel(v, v_want) < 1e-9, "{at}: {v} mm3 for {v_want}");
            let st = s.model.state();
            let b = &st.body("B").unwrap().body;
            let m = measure_body(b).unwrap();
            assert_eq!(st.bodies.len(), 1, "{at}: one body");
            assert_eq!(m.shells, 1, "{at}: one lump");
            assert_eq!(m.merged.faces, 6, "{at}: four walls and two caps");
            // The area pins both caps: the profile at the offset plane, the drafted section at
            // the far end. (The volume alone cannot: it is measured on a tessellation.)
            assert!(rel(m.area, a_want) < 1e-9, "{at}: {} mm2 for {a_want}", m.area);
            // The solid lies between the offset plane and the far end, and the widening (or
            // narrowing) is all on the far side of the profile.
            let (lo, far) = if sign > 0.0 { (off, off + h) } else { (off - h, off) };
            assert!((m.bbox.min.z - lo).abs() < 1e-9 && (m.bbox.max.z - (lo + h)).abs() < 1e-9, "{at}: {:?}", m.bbox);
            let (x0, x1) = (0f64.min(-h * t), w.max(w + h * t));
            let (y0, y1) = (0f64.min(-h * t), d.max(d + h * t));
            assert!((m.bbox.min.x - x0).abs() < 1e-9 && (m.bbox.max.x - x1).abs() < 1e-9, "{at}: {:?} for x {x0}..{x1}", m.bbox);
            assert!((m.bbox.min.y - y0).abs() < 1e-9 && (m.bbox.max.y - y1).abs() < 1e-9, "{at}: {:?} for y {y0}..{y1}", m.bbox);
            // Every face names an exact surface: four planes leaning by the taper and two flat
            // caps, one of them the profile's plane at the offset. A tessellated stand-in
            // measures nearly the same volume and names no surface at all.
            let (mut walls, mut caps) = (0, 0);
            for i in 0..b.face_count() {
                match face_surface(b, i) {
                    Some(FaceSurface::Plane { normal, point }) if normal.z.abs() > 1.0 - 1e-9 => {
                        caps += 1;
                        assert!((point.z - lo).abs() < 1e-9 || (point.z - far).abs() < 1e-9, "{at}: a cap at z = {}", point.z);
                    }
                    Some(FaceSurface::Plane { normal, .. }) => {
                        walls += 1;
                        assert!((normal.z + sign * ang.sin()).abs() < 1e-9, "{at}: a wall leaning {}, not {}", normal.z, -sign * ang.sin());
                    }
                    other => panic!("{at}: face {i} is {other:?}"),
                }
            }
            assert_eq!((walls, caps), (4, 2), "{at}");
        }
    }

    // The extents that are still to come are refused, and say which they are.
    for p in [json!({"distance":h,"distance2":5,"taper":"6 deg"}), json!({"distance":h,"direction":"symmetric","taper":"6 deg"})] {
        let mut s = Session::default();
        run(&mut s, "sketch.create", json!({"plane":"XY"}));
        run(&mut s, "sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[w,d]}));
        run(&mut s, "sketch.finish", json!({}));
        let e = s.execute("solid.extrude", &p).unwrap_err().to_string();
        assert!(e.contains("not supported yet: taper with two-sided or symmetric extents"), "{p}: {e}");
    }
}

/// A drafted pocket that starts above its sketch plane, cut into a body three booleans deep:
/// its rim lies on a face bounded by an intersection curve, not by straight lines. The tool
/// lies wholly in material, so the cut removes exactly the frustum over its profile.
#[test]
fn a_drafted_offset_cut_into_a_body_with_history() {
    use solidcraft_kernel::{FaceSurface, face_surface, measure as measure_body};
    let (w, d, h, off) = (30.0, 20.0, 12.0, 18.0);
    let (ang, t) = (6f64.to_radians(), 6f64.to_radians().tan());
    let frustum = w * d * h + (w + d) * t * h * h + 4.0 / 3.0 * t * t * h * h * h;
    let unnamed = |b: &solidcraft_kernel::Body| (0..b.face_count()).filter(|i| face_surface(b, *i).is_none()).count();

    let mut s = Session::default();
    // A 60 x 40 x 30 block, a round boss joined through its far side and out of its top, and a
    // round pocket cut in the top: the top face is bounded by the boss's intersection curve.
    run(&mut s, "solid.box", json!({"length":60,"width":40,"height":30,"corner":[0,0,0],"body_name":"P"}));
    run(&mut s, "solid.cylinder", json!({"radius":8,"height":46,"base":[58,20,-8],"operation":"join"}));
    run(&mut s, "solid.cylinder", json!({"radius":4,"height":12,"base":[10,34,20],"operation":"cut"}));
    let before = volume(&mut s);
    {
        let st = s.model.state();
        assert_eq!(st.bodies.len(), 1, "the block, the boss and the pocket are one body");
        assert_eq!(unnamed(&st.body("P").unwrap().body), 0, "faces without a surface before the cut");
    }

    // The pocket: drafted 6 deg outward, starting 18 mm up the normal and out through the top.
    run(&mut s, "sketch.create", json!({"plane":"XY"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0":[5,5],"p1":[35,25]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"distance":h,"start_offset":off,"taper":"6 deg","operation":"cut"}));
    let after = volume(&mut s);
    let removed = before - after;
    assert!((removed - frustum).abs() < 1e-6 * frustum, "{removed} mm3 removed for the frustum's {frustum}");

    let st = s.model.state();
    let b = &st.body("P").unwrap().body;
    let m = measure_body(b).unwrap();
    assert_eq!(st.bodies.len(), 1, "one body");
    assert_eq!(m.shells, 1, "one lump: the pocket is open at the top");
    assert_eq!(unnamed(b), 0, "faces without a surface after the cut");
    // Four drafted walls, and the pocket's floor at the offset: the only leaning planes of the
    // body (the block's own faces are level or upright), and the only face in the plane the cut
    // starts on. A cut keeps the tool's sides facing into the pocket, so a wall drafted outward
    // by a leans by +sin a, the other way round from the same extrude as a new body.
    let (mut walls, mut floor) = (0, 0);
    for i in 0..b.face_count() {
        let Some(FaceSurface::Plane { normal, point }) = face_surface(b, i) else { continue };
        if (normal.z - ang.sin()).abs() < 1e-9 {
            walls += 1;
        } else if normal.z > 1.0 - 1e-9 && (point.z - off).abs() < 1e-9 {
            floor += 1;
        } else {
            assert!(normal.z.abs() < 1e-9 || normal.z.abs() > 1.0 - 1e-9, "a plane leaning {}, neither the block's nor the pocket's", normal.z);
        }
    }
    assert_eq!((walls, floor), (4, 1), "four drafted walls, and the pocket's floor at z = {off}");
}

/// A rounded part as drawn in the app: a rectangle and a quarter circle tangent to its top
/// line, extruded together. The arc (about (62.17, -21), radius 21) ends square at the front
/// face and in a knife-edge tip where it runs into the back face.
fn knife_edge_part() -> Session {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0.0, 0.0], "p1": [41.5, -21.0]}));
    run(&mut s, "sketch.dimension", json!({"entities": ["l2"], "text_at": [36.5, -8.5], "value": "21 mm"}));
    run(&mut s, "sketch.line", json!({"infer": true, "points": ["p2", [61.5, -21.0]]}));
    run(&mut s, "sketch.dimension", json!({"entities": ["l5"], "text_at": [54.5, -26.5], "value": "21"}));
    run(&mut s, "sketch.circle.center", json!({"center": "l5.end", "radius": 21.0}));
    run(&mut s, "sketch.line", json!({"infer": true, "points": ["p3", [62.166666666666664, 0.0]]}));
    run(&mut s, "sketch.constraint.tangent", json!({"a": "c1", "b": "l6"}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"direction": "positive", "distance": "10 mm", "operation": "new", "profiles": [0, 1], "sketch": 1}));
    s
}

/// The volume of the session's bodies through the display plan (the measure's mesh cuts across
/// the concave arc). It holds to about 1e-3 of what a blend removes here; the bounds below are
/// about twice what each test reads.
fn planned(s: &Session) -> f64 {
    s.model.state().bodies.iter().map(|b| b.body.display_plan(2e-4).unwrap().evaluate().measure().volume).sum()
}

/// The middle of the part's arc.
const KNIFE_ARC_MID: [f64; 2] = [62.166666666666664 - 21.0 * std::f64::consts::FRAC_1_SQRT_2, -21.0 + 21.0 * std::f64::consts::FRAC_1_SQRT_2];

/// Simpson's rule over the round's corner (2 × 2 less a quarter disc) at distance ρ = 21 + x
/// from the arc's axis, times `length(ρ)`.
fn round_along(length: impl Fn(f64) -> f64) -> f64 {
    let (r, n) = (2.0f64, 2000);
    let h = r / n as f64;
    (0..=n)
        .map(|i| {
            let x = i as f64 * h;
            let w = if i == 0 || i == n { 1.0 } else { [2.0, 4.0][i % 2] };
            w * (r - (r * r - (r - x) * (r - x)).max(0.0).sqrt()) * length(21.0 + x)
        })
        .sum::<f64>()
        * h
        / 3.0
}

/// Its edges are stored the other way round from their curves, which once chained the arc
/// with the front edge across their corner. Rounding the arc at the bottom and the top removes
/// the round's section along it, cut off at the tip by the back face's plane: ρ asin(21 / ρ).
#[test]
fn fillet_an_arc_that_runs_into_a_knife_edge_tip() {
    let mut s = knife_edge_part();
    let before = planned(&s);
    let [x, y] = KNIFE_ARC_MID;
    run(&mut s, "solid.fillet", json!({"edges": [[x, y, 0.0]], "radius": 2}));
    run(&mut s, "solid.fillet", json!({"edges": [[x, y, 10.0]], "radius": 2}));
    let removed = round_along(|rho| rho * (21.0 / rho).asin());
    let after = planned(&s);
    assert!(rel(before - after, 2.0 * removed) < 2e-3, "removed {} vs {}", before - after, 2.0 * removed);
}

/// The part's front corner (the front face into the arc) rounded with 2 first, then the bottom
/// run (front edge, the corner round's arc, the arc) with 2 as well: a round on a round of the
/// same radius. Along the front edge a straight round; at the corner the ball turns about one
/// point, a piece of sphere (θ r³ / 6 over the corner's θ); along the arc the torus section from
/// the corner round to the tip.
#[test]
fn fillet_a_run_over_a_corner_rounded_with_the_same_radius() {
    let mut s = knife_edge_part();
    run(&mut s, "solid.fillet", json!({"edges": [[41.1667, -21.0, 5.0]], "radius": 2}));
    let before = planned(&s);
    let [x, y] = KNIFE_ARC_MID;
    // The corner round: about (62.17 - √(23² - 2²), -19), its arc's middle 2 from there.
    let cx = 62.166666666666664 - (23.0f64 * 23.0 - 4.0).sqrt();
    let joint = 2.0f64.atan2((23.0f64 * 23.0 - 4.0).sqrt());
    let theta = std::f64::consts::FRAC_PI_2 - joint;
    let corner_mid = [cx + 2.0 * (theta / 2.0).sin(), -19.0 - 2.0 * (theta / 2.0).cos()];
    run(&mut s, "solid.fillet", json!({"edges": [[x, y, 0.0], [corner_mid[0], corner_mid[1], 0.0], [20.0, -21.0, 0.0]], "radius": 2}));
    let removed = (1.0 - std::f64::consts::FRAC_PI_4) * 4.0 * cx + theta * 8.0 / 6.0 + round_along(|rho| rho * ((21.0 / rho).asin() - joint));
    let after = planned(&s);
    assert!(rel(before - after, removed) < 1e-3, "removed {} vs {removed}", before - after);
}

/// Straight edges of the part ending at its arc wall: the long front edge at the top (the wall
/// cuts the round off at x = 62.17 − √(21² − d²), d from the front face) and the back edge at
/// the top, which runs into the knife-edge tip (x = 62.17 − √(21² − (21 − d)²), d from the back
/// face).
#[test]
fn fillet_straight_edges_that_end_at_the_arc_wall() {
    let cx = 62.166666666666664;
    for (edge, cut) in [([20.0, -21.0, 10.0], 0.0), ([30.0, 0.0, 10.0], 21.0)] {
        let mut s = knife_edge_part();
        let before = planned(&s);
        run(&mut s, "solid.fillet", json!({"edges": [edge], "radius": 2}));
        let removed = round_along(|rho| {
            let d = rho - 21.0;
            cx - (441.0 - (cut - d) * (cut - d)).sqrt()
        });
        let after = planned(&s);
        assert!(rel(before - after, removed) < 3e-3, "{edge:?}: removed {} vs {removed}", before - after);
    }
}

/// The vertical knife edge at the tip (the arc and the back face meeting at no angle) rounds to
/// the ball 2 from both, about 13 back from the tip: the cusp beyond the ball, on the tip's side
/// of its contacts, all the way up.
#[test]
fn fillet_the_knife_edge_at_the_tip() {
    let cx = 62.166666666666664;
    let (kx, ky) = (cx - (23.0f64 * 23.0 - 19.0 * 19.0).sqrt(), -2.0);
    let (hx, hy) = (0.002, 0.0005);
    let mut area = 0.0;
    let mut x = kx + hx / 2.0;
    while x < cx {
        let lo = -21.0 + (441.0 - (x - cx) * (x - cx)).max(0.0).sqrt();
        for j in 0..((0.0 - lo) / hy).ceil() as usize {
            let (y0, y1) = (lo + j as f64 * hy, (lo + (j + 1) as f64 * hy).min(0.0));
            let y = (y0 + y1) * 0.5;
            let a = (y - ky).atan2(x - kx);
            if (x - kx).hypot(y - ky) > 2.0 && a >= (-21.0 - ky).atan2(cx - kx) {
                area += (y1 - y0) * hx;
            }
        }
        x += hx;
    }
    let mut s = knife_edge_part();
    let before = planned(&s);
    run(&mut s, "solid.fillet", json!({"edges": [[cx, 0.0, 5.0]], "radius": 2}));
    let after = planned(&s);
    assert!(rel(before - after, area * 10.0) < 5e-4, "removed {} vs {}", before - after, area * 10.0);
    let mut s = knife_edge_part();
    run(&mut s, "solid.chamfer", json!({"edges": [[cx, 0.0, 5.0]], "distance": 2}));
    assert!(before - planned(&s) > before - after, "the chamfer takes the ball's bulge off too");
}

/// Rounds of the edges at the arc's top front corner, two or all three together (r 2): they meet
/// in creases (with all three, a piece of sphere fills the corner). Each set takes off its rounds
/// alone, less what each alone takes in a box about the corner, plus what they take there
/// together: the CSG grid of the kernel's `rounds_meeting_at_a_corner_of_an_arc_wall`
/// (curvecorner_tests.rs, 0.02 mm cells), within its accuracy.
#[test]
fn fillet_edges_meeting_at_the_front_corner_of_the_arc() {
    // In the box about the corner: what the long edge's, the arc's and the upright's rounds take
    // alone, and what two or all three take together.
    let corner = [2.719176, 3.015864, 1.863];
    let together = [(vec![0, 2], 3.886936), (vec![0, 1], 4.971256), (vec![1, 2], 4.19864), (vec![0, 1, 2], 6.3332)];
    let [x, y] = KNIFE_ARC_MID;
    let (long, arc, upright) = ([20.0, -21.0, 10.0], [x, y, 10.0], [41.1667, -21.0, 5.0]);
    let off = |edges: Vec<[f64; 3]>| {
        let mut s = knife_edge_part();
        let before = planned(&s);
        run(&mut s, "solid.fillet", json!({"edges": edges, "radius": 2}));
        before - planned(&s)
    };
    let edges = [long, arc, upright];
    let alone = edges.map(|e| off(vec![e]));
    for (which, both) in together {
        let want = which.iter().map(|i| alone[*i] - corner[*i]).sum::<f64>() + both;
        let got = off(which.iter().map(|i| edges[*i]).collect());
        assert!((got - want).abs() < 0.03, "{which:?}: removed {got} vs {want}");
    }
}

/// The long top front edge rounded, then the small front-left upright as a fillet of its own
/// (as in the app): the new round ends in the earlier one, taking off what both take together.
#[test]
fn fillet_the_long_edge_then_the_small_left_edge() {
    let mut s = knife_edge_part();
    let before = planned(&s);
    run(&mut s, "solid.fillet", json!({"edges": [[20.0, -21.0, 10.0]], "radius": 2}));
    run(&mut s, "solid.fillet", json!({"edges": [[0.0, -21.0, 5.0]], "radius": 2}));
    let one_by_one = before - planned(&s);
    let mut s = knife_edge_part();
    run(&mut s, "solid.fillet", json!({"edges": [[20.0, -21.0, 10.0], [0.0, -21.0, 5.0]], "radius": 2}));
    let together = before - planned(&s);
    // Within the display meshes' accuracy (the two are built of different faces).
    assert!(rel(one_by_one, together) < 2e-4, "one by one {one_by_one} vs together {together}");
}

/// The long top front edge and the front-left vertical edge rounded together (r 2): the long
/// round ends at the arc wall, the two meet at the top-left corner (the top-left edge sharp).
/// Each removes its own round (the long one cut off by the wall), less what both would take at
/// the corner: in its 2 mm cube, what lies outside both quarter circles, ∫ (2 − √(4 − t²))² dt
/// over 0..2 = 16 − 8/3 − 4π.
#[test]
fn fillet_two_edges_meeting_at_a_corner_one_running_into_the_arc_wall() {
    let mut s = knife_edge_part();
    let before = planned(&s);
    run(&mut s, "solid.fillet", json!({"edges": [[20.0, -21.0, 10.0], [0.0, -21.0, 5.0]], "radius": 2}));
    let long = round_along(|rho| 62.166666666666664 - (441.0 - (rho - 21.0) * (rho - 21.0)).sqrt());
    let upright = (1.0 - std::f64::consts::FRAC_PI_4) * 4.0 * 10.0;
    let removed = long + upright - (16.0 - 8.0 / 3.0 - 4.0 * std::f64::consts::PI);
    let after = planned(&s);
    assert!(rel(before - after, removed) < 1e-3, "removed {} vs {removed}", before - after);
}

/// A line of text cut into a block (#391): the boolean came out empty on it after two minutes
/// of retries, at 3.5 mm deep. Two walls of a letter met the block's face 2e-7 mm apart, which
/// truck read as two vertices, and the result's shell did not close. The cut works at any
/// depth now, and takes out the same section at each (the text runs off the block's end, so a
/// little less than the letters).
#[test]
fn a_line_of_text_cuts_into_a_block_at_any_depth() {
    let mut removed = Vec::new();
    let mut area = 0.0;
    for depth in [1.0, 3.5] {
        let mut s = Session::default();
        run(&mut s, "sketch.create", json!({"plane":"XY"}));
        run(&mut s, "sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[105,26]}));
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "solid.extrude", json!({"distance":-40}));
        run(&mut s, "sketch.create", json!({"plane":"XY"}));
        run(&mut s, "sketch.text", json!({"text":"textvraiment tres long","at":[3,9],"height":7.81}));
        let state = s.world_state();
        let ss = state.sketch(s.active_sketch.unwrap()).unwrap();
        // The letters' area, counters left out (a letter's hole is its own profile too).
        let sorted = |ids: &[String]| {
            let mut ids = ids.to_vec();
            ids.sort();
            ids
        };
        let counter = |p: &solidcraft_sketch::Profile| ss.profiles.iter().any(|q| q.hole_curves.iter().any(|h| sorted(h) == sorted(&p.outer_curves)));
        area = ss.profiles.iter().filter(|p| !counter(p)).map(|p| p.area).sum();
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "solid.extrude", json!({"distance":-depth,"operation":"cut"}));
        let volume = run(&mut s, "inspect.measure", json!({}))["total"]["volume_mm3"].as_f64().unwrap();
        removed.push((105.0 * 26.0 * 40.0 - volume) / depth);
        assert_eq!(s.world_state().bodies.len(), 1);
    }
    let (shallow, deep) = (removed[0], removed[1]);
    assert!((deep - shallow).abs() < 1e-3 * shallow, "{shallow} mm² taken out at 1 mm, {deep} mm² at 3.5 mm");
    assert!(shallow > 0.85 * area && shallow < area, "{shallow} mm² taken out, the letters are {area} mm²");
}

/// A design saved before tapered sides were built through `taper_loop` (#1014), by the build
/// before it: two tapered bosses apart from each other (now joined in one boolean), a fillet on
/// a sloping side edge of one and on the top edge of the other, both saved by edge name. They
/// still find their edges, and the part comes out the same.
#[test]
fn fillets_on_tapered_bosses_saved_before_still_find_their_edges() {
    let text = include_str!("../../../../tests/taper/before-1014/two_tapered_bosses_filleted.json");
    let mut s = Session::new(solidcraft_doc::Document::from_json(text).unwrap());
    let volume = run(&mut s, "inspect.measure", json!({}))["total"]["volume_mm3"].as_f64().unwrap();
    for name in ["Extrude2", "Fillet1", "Fillet2"] {
        let id = s.doc.find_feature(name).unwrap().id;
        let r = s.model.result(id).unwrap();
        assert!(r.error.is_none() && r.warning.is_none(), "{name}: {:?} {:?}", r.error, r.warning);
    }
    // The build that saved it measured 69454.885209 mm³.
    assert!((volume - 69454.8852092132).abs() < 1e-3, "{volume}");
    assert_eq!(s.world_state().bodies.len(), 1);
}

/// #1360: a ring drawn round a hole, its inner circle on the hole's wall, cut into a chamfered
/// disc. The ring's hole lies over the part's hole all the way down the cut, so the tool takes
/// it in (a disc instead of a ring cuts the same): its wall no longer lies on the hole's wall,
/// which the boolean couldn't intersect. Over a hole whose floor the cut reaches, the ring's hole
/// stays: the material under the floor is not cut (the cut may still fail there, as before).
#[test]
fn a_ring_cut_round_a_hole_takes_in_the_hole_it_is_drawn_round() {
    let ring = |hole: Value| {
        let mut s = Session::default();
        for (c, p) in [
            ("sketch.create", json!({"plane":"XY"})),
            ("sketch.circle.center", json!({"center":[0,0],"radius":40})),
            ("sketch.finish", json!({})),
            ("solid.extrude", json!({"distance":10})),
            ("solid.hole", hole),
            ("solid.chamfer", json!({"edges":[[40,0,10]],"distance":1})),
            ("sketch.create", json!({"plane":{"face":[0,0,10]}})),
            ("sketch.circle.center", json!({"center":[20,0],"radius":3})),
            ("sketch.circle.center", json!({"center":[20,0],"radius":6})),
            ("sketch.finish", json!({})),
        ] {
            run(&mut s, c, p);
        }
        let before = volume(&mut s);
        // Profile 2: the ring between the hole's wall and the larger circle.
        let r = s.execute("solid.extrude", &json!({"direction":"negative","distance":"3","operation":"cut","profiles":[2]}));
        let clean = r
            .as_ref()
            .ok()
            .and_then(|v| v["feature"].as_u64())
            .and_then(|id| s.model.result(id))
            .is_some_and(|f| f.boolean_diagnostics.events.is_empty());
        (r.is_ok(), before - volume(&mut s), clean)
    };
    let taken = std::f64::consts::PI * (36.0 - 9.0) * 3.0;
    let (ok, through, clean) = ring(json!({"position":[20,0,10],"diameter":6}));
    assert!(ok && clean && rel(through, taken) < 1e-3, "{through} vs {taken}");
    // A blind hole 1 deep: under its floor the ring's hole is material, and nothing of it goes.
    let (ok, blind, _) = ring(json!({"position":[20,0,10],"diameter":6,"depth":1}));
    assert!(if ok { rel(blind, taken) < 1e-3 } else { blind.abs() < 1e-6 }, "{ok} {blind} vs {taken}");
}

/// #1360: an annulus cut 3 mm into a chamfered disc, its inner circle on a hole's wall. The
/// boolean can't intersect the two walls lying on each other; its last resort (the tool moved by
/// a micron) left a face a hair wide that meshes as its whole plane at a fine step, and the part
/// came back 21000 mm³ larger than before, a square plane across it. A cut may fail there, but it
/// never gives back more than it was given.
#[test]
fn a_cut_flush_with_a_hole_never_grows_the_part() {
    let mut s = Session::default();
    for (c, p) in [
        ("sketch.create", json!({"plane":"XY"})),
        ("sketch.circle.center", json!({"center":[0,0],"radius":40})),
        ("sketch.finish", json!({})),
        ("solid.extrude", json!({"distance":10})),
        ("solid.hole", json!({"position":[20,0,10],"diameter":6})),
        ("solid.chamfer", json!({"edges":[[40,0,10]],"distance":1})),
        ("sketch.create", json!({"plane":{"face":[0,0,10]}})),
        ("sketch.circle.center", json!({"center":[20,0],"radius":3})),
        ("sketch.circle.center", json!({"center":[20,0],"radius":6})),
        ("sketch.finish", json!({})),
    ] {
        run(&mut s, c, p);
    }
    let before = volume(&mut s);
    // Profile 2: the annulus between the hole's wall and the larger circle.
    let r = s.execute("solid.extrude", &json!({"direction":"negative","distance":"3","operation":"cut","profiles":[2]}));
    let after = volume(&mut s);
    let ring = std::f64::consts::PI * (36.0 - 9.0) * 3.0;
    match r {
        Ok(_) => assert!(rel(before - after, ring) < 1e-2, "{before} -> {after}, want {ring} taken"),
        Err(_) => assert!(rel(after, before) < 1e-9, "{before} -> {after} after a failed cut"),
    }
}

/// A line of text cut into a face: the letters' curves (the font's quadratics) are read
/// exactly, and the cut is decided in 2D in one boolean, at any depth and where the text runs
/// off the face. The B-rep boolean failed on many letters at once ("no result" at 3.5 mm, and
/// off the edge), else took one letter at a time on an ever larger body (tens of seconds).
#[test]
fn a_line_of_text_is_cut_in_one_exact_boolean() {
    let cut = |at: [f64; 2], depth: f64| {
        let mut s = Session::default();
        run(&mut s, "sketch.create", json!({"plane":"XY"}));
        run(&mut s, "sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[105,26]}));
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "solid.extrude", json!({"distance":40}));
        let before = volume(&mut s);
        run(&mut s, "sketch.create", json!({"plane":{"face":[50,13,40]}}));
        run(&mut s, "sketch.text", json!({"text":"textvraiment tres long","at":at,"height":7.81}));
        run(&mut s, "sketch.finish", json!({}));
        let id = run(&mut s, "solid.extrude", json!({"distance":-depth,"operation":"cut"}))["feature"].as_u64().unwrap();
        let r = s.model.result(id).unwrap();
        assert!(r.error.is_none() && r.boolean_diagnostics.events.is_empty(), "{at:?} {depth}: {r:?}");
        before - volume(&mut s)
    };
    // Straight walls: the material taken grows with the depth, the letters' area each mm.
    let (one, deep) = (cut([3.0, 9.0], 1.0), cut([3.0, 9.0], 3.5));
    assert!(rel(deep, 3.5 * one) < 1e-3, "{one} {deep}");
    // Off the face's edge, the part of the text still over it.
    let off = cut([-6.0, 20.0], 1.0);
    assert!(off > 0.0 && off < one, "{off} {one}");
}

/// A line of text cut right through a part, and one joined on in relief, are decided in 2D in
/// one boolean too. Through, each letter's counter (inside an "e" or an "a") comes out as a
/// piece of its own. In relief the letters are joined to the part all at once: one by one, each
/// joined to a part grown by the ones before, it took minutes.
#[test]
fn a_line_of_text_cut_through_or_joined_in_relief_is_one_boolean() {
    let text = |op: &str, distance: f64| {
        let mut s = Session::default();
        run(&mut s, "sketch.create", json!({"plane":"XY"}));
        run(&mut s, "sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[105,26]}));
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "solid.extrude", json!({"distance":40}));
        let before = volume(&mut s);
        run(&mut s, "sketch.create", json!({"plane":{"face":[50,13,40]}}));
        run(&mut s, "sketch.text", json!({"text":"textvraiment tres long","at":[3,9],"height":6.5}));
        run(&mut s, "sketch.finish", json!({}));
        let id = run(&mut s, "solid.extrude", json!({"distance":distance,"operation":op}))["feature"].as_u64().unwrap();
        let r = s.model.result(id).unwrap();
        assert!(r.error.is_none() && r.boolean_diagnostics.events.is_empty(), "{op}: {r:?}");
        (before, volume(&mut s), s.model.state().bodies.len())
    };
    let (before, through, pieces) = text("cut", -40.0);
    let (_, one, _) = text("cut", -1.0);
    // Through: the letters' area all the way down, with the counters left as pieces.
    assert!(pieces > 1, "{pieces}");
    assert!(rel(before - through, 40.0 * (before - one)) < 2e-3, "{} vs {}", before - through, 40.0 * (before - one));
    // In relief 2 high: twice what a 1 mm cut takes.
    let (_, relief, pieces) = text("join", 2.0);
    assert_eq!(pieces, 1);
    assert!(rel(relief - before, 2.0 * (before - one)) < 2e-3, "{} vs {}", relief - before, 2.0 * (before - one));
}

/// The same line of text cut again, deeper: the new walls lie on the letters' walls. The 2D route
/// takes it as one exact boolean: a point a hair from a segment's end is that end, a sliver of a
/// letter's curve shorter than the tolerance is dropped, and a piece of a letter so short that the
/// engraved walls read it as an arc while the tool has the curve itself is one edge. Before, the
/// sewing found a degenerate edge, then the faces did not close, and the cut went to the B-rep
/// boolean's retries (90 s).
#[test]
fn a_line_of_text_cut_again_deeper_is_one_exact_boolean() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane":"XY"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[105,26]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"distance":40}));
    let cut = |s: &mut Session, depth: f64| {
        let before = volume(s);
        run(s, "sketch.create", json!({"plane":"XY","offset":40}));
        run(s, "sketch.text", json!({"text":"textvraiment tres long","at":[3,9],"height":6.5}));
        run(s, "sketch.finish", json!({}));
        let id = run(s, "solid.extrude", json!({"distance":-depth,"operation":"cut"}))["feature"].as_u64().unwrap();
        let r = s.model.result(id).unwrap();
        assert!(r.error.is_none() && r.boolean_diagnostics.events.is_empty(), "{depth}: {r:?}");
        before - volume(s)
    };
    let first = cut(&mut s, 1.0);
    // Cut again 2 mm deep: one more millimetre of the letters.
    let again = cut(&mut s, 2.0);
    assert!(rel(again, first) < 1e-3, "{first} {again}");
}

/// Text cut through a large plate (2 m) is decided in 2D in one exact boolean, as it is in a
/// small one. The 2D route's tolerances grow with the part: a font's curve that is merely near a
/// circle was read as an arc ending a micron off the next curve, so the letter's outline did not
/// close, and where the curves of a 3 come close the arrangement left a sliver that closed up to
/// nothing. The cut went to the B-rep boolean instead, which took tens of seconds and moved the
/// tool to find a result at all.
#[test]
fn text_cut_through_a_large_plate_is_one_exact_boolean() {
    let taken = |w: f64, h: f64, at: [f64; 2]| {
        let mut s = Session::default();
        run(&mut s, "solid.box", json!({"length":w,"width":h,"height":1.5,"corner":[0,0,0]}));
        let before = volume(&mut s);
        run(&mut s, "sketch.create", json!({"plane":"XY","offset":1.5}));
        run(&mut s, "sketch.text", json!({"text":"PART-23","at":at,"height":7}));
        run(&mut s, "sketch.finish", json!({}));
        let id = run(&mut s, "solid.extrude", json!({"distance":-2,"operation":"cut"}))["feature"].as_u64().unwrap();
        let r = s.model.result(id).unwrap();
        assert!(r.error.is_none() && r.boolean_diagnostics.events.is_empty(), "{w} x {h}: {r:?}");
        before - volume(&mut s)
    };
    let (small, large) = (taken(200.0, 120.0, [60.0, 40.0]), taken(2000.0, 1200.0, [860.0, 125.0]));
    // The letters right through the plate, their counters left as pieces of their own: the same
    // material in either plate (measured on meshes, which are coarser on the larger one).
    assert!(small > 10.0 && rel(large, small) < 5e-2, "{small} {large}");
}

/// A loft between two sections in one plane is refused (it used to build a flat body of zero
/// volume and report success).
#[test]
fn loft_through_coplanar_sections_is_refused() {
    let mut s = Session::default();
    for name in ["A", "B"] {
        run(&mut s, "sketch.create", json!({"plane": "XY", "name": name}));
        run(&mut s, "sketch.circle.center", json!({"center": [0, 0], "radius": 10}));
        run(&mut s, "sketch.finish", json!({}));
    }
    let e = s.execute("solid.loft", &json!({"sections": [{"sketch": "A"}, {"sketch": "B"}]})).unwrap_err().to_string();
    assert!(e.contains("same plane"), "{e}");
    assert!(s.model.state().bodies.is_empty());
}

#[test]
fn duplicate_and_excess_profile_picks_are_refused_quickly() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Bottom"}));
    run(&mut s, "sketch.circle.center", json!({"center": [2.5, 2.5], "radius": 2.5}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "offset": 30, "name": "Top"}));
    run(&mut s, "sketch.circle.center", json!({"center": [2.5, 2.5], "radius": 2}));
    run(&mut s, "sketch.finish", json!({}));
    let started = std::time::Instant::now();
    let many = vec![0; 10_000];
    let e = s.execute("solid.extrude", &json!({"sketch": "Bottom", "distance": 5, "profiles": many})).unwrap_err().to_string();
    assert!(e.contains("profile 0 is listed twice"), "{e}");
    let loft = json!({"sections": [{"sketch": "Bottom", "profiles": many}, {"sketch": "Top"}]});
    let e = s.execute("solid.loft", &loft).unwrap_err().to_string();
    assert!(e.contains("profile 0 is listed twice"), "{e}");
    // More profiles than the sketch has, or one it doesn't have.
    let e = s.execute("solid.extrude", &json!({"sketch": "Bottom", "distance": 5, "profiles": [0, 1]})).unwrap_err().to_string();
    assert!(e.contains("2 profiles are listed, but Bottom has 1"), "{e}");
    let e = s.execute("solid.extrude", &json!({"sketch": "Bottom", "distance": 5, "profiles": [3]})).unwrap_err().to_string();
    assert!(e.contains("there is no profile 3 in Bottom"), "{e}");
    let points = json!([{"point": [1, 1]}, {"point": [2, 2]}]);
    let e = s.execute("solid.extrude", &json!({"sketch": "Bottom", "distance": 5, "profiles": points})).unwrap_err().to_string();
    assert!(e.contains("2 profile points are given, but Bottom has 1"), "{e}");
    assert!(started.elapsed().as_secs_f64() < 5.0, "refusing took {:?}", started.elapsed());
    assert!(s.world_state().bodies.is_empty(), "nothing was built");
    // One pick of the profile still builds.
    run(&mut s, "solid.loft", json!({"sections": [{"sketch": "Bottom", "profiles": [0]}, {"sketch": "Top"}]}));
}

#[test]
fn coil_from_a_sketch_profile_sweeps_its_plane() {
    use std::f64::consts::PI;
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "YZ", "name": "Section"}));
    // A 1 x 1 mm square, 10..11 mm from the Z axis, on a plane through the axis.
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [10, -0.5], "p1": [11, 0.5]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.coil", json!({"sketch": "Section", "revolutions": 3, "pitch": 8}));
    // Pappus for a screw sweep: V = 2 pi n A r_centroid.
    assert!(rel(volume(&mut s), 2.0 * PI * 3.0 * 1.0 * 10.5) < 1e-3, "{}", volume(&mut s));
    let m = measure(&mut s);
    let b = &m["bodies"][0]["bbox"];
    assert!((b["min"][2].as_f64().unwrap_or(f64::NAN) + 0.5).abs() < 1e-6, "{m}");
    assert!((b["max"][2].as_f64().unwrap_or(f64::NAN) - 24.5).abs() < 1e-6, "{m}");
}

#[test]
fn coil_from_a_sketch_cuts_a_thread_groove() {
    use std::f64::consts::PI;
    let mut s = Session::default();
    run(&mut s, "solid.cylinder", json!({"radius": 20, "height": 40, "body_name": "Shaft"}));
    run(&mut s, "sketch.create", json!({"plane": "YZ", "name": "Groove"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [18, 1], "p1": [19, 2]}));
    run(&mut s, "sketch.finish", json!({}));
    let before = volume(&mut s);
    run(&mut s, "solid.coil", json!({"sketch": "Groove", "revolutions": 3, "pitch": 8, "operation": "cut", "targets": ["Shaft"]}));
    let removed = before - volume(&mut s);
    assert!(rel(removed, 2.0 * PI * 3.0 * 1.0 * 18.5) < 1e-3, "{removed}");
    assert_eq!(measure(&mut s)["body_count"], 1);
}

fn clockwise_coil_bbox(clockwise: bool) -> Value {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "YZ", "name": "Section"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [10, -0.5], "p1": [11, 0.5]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.coil", json!({"sketch": "Section", "revolutions": 0.25, "pitch": 8, "clockwise": clockwise}));
    measure(&mut s)["bodies"][0]["bbox"].clone()
}

#[test]
fn a_sketch_coil_clockwise_is_the_mirror_image() {
    let right = clockwise_coil_bbox(false);
    let left = clockwise_coil_bbox(true);
    let at = |v: &Value, k: &str, i: usize| v[k][i].as_f64().unwrap_or(f64::NAN);
    assert!((at(&right, "min", 0) + at(&left, "max", 0)).abs() < 1e-6, "{right} {left}");
    assert!((at(&right, "max", 0) + at(&left, "min", 0)).abs() < 1e-6, "{right} {left}");
    for i in 1..3 {
        assert!((at(&right, "min", i) - at(&left, "min", i)).abs() < 1e-6, "{right} {left}");
        assert!((at(&right, "max", i) - at(&left, "max", i)).abs() < 1e-6, "{right} {left}");
    }
}

#[test]
fn a_sketch_coil_refuses_profiles_it_cannot_sweep() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "YZ", "name": "Holed"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [10, -2], "p1": [14, 2]}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [11, -1], "p1": [13, 1]}));
    run(&mut s, "sketch.finish", json!({}));
    let e = s.execute("solid.coil", &json!({"sketch": "Holed", "revolutions": 3, "pitch": 8})).unwrap_err().to_string();
    assert!(e.contains("without holes"), "{e}");

    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "YZ", "name": "Two"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [10, -0.5], "p1": [11, 0.5]}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [12, -0.5], "p1": [13, 0.5]}));
    run(&mut s, "sketch.finish", json!({}));
    let e = s.execute("solid.coil", &json!({"sketch": "Two", "revolutions": 3, "pitch": 8})).unwrap_err().to_string();
    assert!(e.contains("one closed profile"), "{e}");

    // The kernel refuses a pitch no larger than the profile's axial extent.
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "YZ", "name": "Section"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [10, -0.5], "p1": [11, 0.5]}));
    run(&mut s, "sketch.finish", json!({}));
    let e = s.execute("solid.coil", &json!({"sketch": "Section", "revolutions": 3, "pitch": 0.5})).unwrap_err().to_string();
    assert!(e.contains("runs into itself"), "{e}");
}

#[test]
fn a_sketch_coil_refuses_contradictory_parameters() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "YZ", "name": "Section"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [10, -0.5], "p1": [11, 0.5]}));
    run(&mut s, "sketch.finish", json!({}));
    for (k, v) in [
        ("diameter", json!(40)),
        ("section_size", json!(2)),
        ("section", json!("square")),
        ("section_position", json!("outside")),
        ("start_angle", json!(0)),
    ] {
        let mut p = json!({"sketch": "Section", "revolutions": 3, "pitch": 8});
        p[k] = v;
        let e = s.execute("solid.coil", &p).unwrap_err().to_string();
        assert!(e.contains("does not apply with `sketch`"), "{k}: {e}");
    }
    let e = s
        .execute("solid.coil", &json!({"profiles": "all", "diameter": 40, "revolutions": 3, "pitch": 8, "section_size": 2}))
        .unwrap_err()
        .to_string();
    assert!(e.contains("`profiles` needs `sketch`"), "{e}");
    let e = s.execute("solid.coil", &json!({"sketch": "Nope", "revolutions": 3, "pitch": 8})).unwrap_err().to_string();
    assert!(e.contains("no sketch `Nope`"), "{e}");
}

#[test]
fn a_loaded_coil_cannot_be_both_primitive_and_from_a_sketch() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "YZ", "name": "Section"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [10, -0.5], "p1": [11, 0.5]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.coil", json!({"sketch": "Section", "revolutions": 3, "pitch": 8}));
    let mut v: Value = serde_json::from_str(&s.doc.to_json()).unwrap();
    let coil = v["features"].as_array_mut().unwrap().iter_mut().find(|f| f["type"] == "coil").unwrap();
    coil["diameter"] = json!("40");
    let reopened = Session::new(solidcraft_doc::Document::from_json(&v.to_string()).unwrap());
    let err = reopened.model.results.iter().find_map(|r| r.error.clone()).unwrap_or_default();
    assert!(err.contains("cannot also set"), "{err}");
}

#[test]
fn a_loaded_sketch_coil_cannot_also_set_the_primitive_section() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "YZ", "name": "Section"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [10, -0.5], "p1": [11, 0.5]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.coil", json!({"sketch": "Section", "revolutions": 3, "pitch": 8}));
    for (field, value) in [("section", json!("square")), ("position", json!(1)), ("start_angle", json!("45 deg"))] {
        let mut v: Value = serde_json::from_str(&s.doc.to_json()).unwrap();
        let coil = v["features"].as_array_mut().unwrap().iter_mut().find(|f| f["type"] == "coil").unwrap();
        coil[field] = value;
        let reopened = Session::new(solidcraft_doc::Document::from_json(&v.to_string()).unwrap());
        let err = reopened.model.results.iter().find_map(|r| r.error.clone()).unwrap_or_default();
        assert!(err.contains("cannot also set"), "{field}: {err}");
    }
}

#[test]
fn a_sketch_coil_never_panics_on_hostile_parameters() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "YZ", "name": "Section"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [10, -0.5], "p1": [11, 0.5]}));
    run(&mut s, "sketch.finish", json!({}));
    // A valid profile coil, so that each hostile value reaches the parameter it attacks.
    let with = |k: &str, v: Value| {
        let mut p = json!({"sketch": "Section", "revolutions": 3, "pitch": 8});
        p[k] = v;
        p
    };
    for hostile in [json!(true), json!(-1), json!(1e308), json!("nope"), json!([]), json!([0, 0, 0]), json!({"type": "indices"})] {
        for k in ["sketch", "profiles", "pitch", "revolutions"] {
            let result = s.execute("solid.coil", &with(k, hostile.clone()));
            assert!(!matches!(result, Err(crate::EngineError::Internal(..))), "{k} {hostile} panicked: {result:?}");
        }
    }
    // Profile selections reach the parser and the evaluator: no index, shape or point may panic.
    for profiles in [
        json!([]),
        json!([0]),
        json!([999]),
        json!([-1]),
        json!(1e308),
        json!("nope"),
        json!({"type": "indices"}),
        json!([{"point": [1]}]),
        json!([[]]),
        json!([{"curves": 7}]),
    ] {
        let result = s.execute("solid.coil", &with("profiles", profiles.clone()));
        assert!(!matches!(result, Err(crate::EngineError::Internal(..))), "profiles {profiles} panicked: {result:?}");
    }
}

fn placed_bounds(s: &Session, name: &str) -> solidcraft_geom::Aabb3 {
    s.world_state().body(name).unwrap_or_else(|| panic!("no {name}")).mesh().bounds()
}

fn about(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
}

/// Frees every grounded placement (a design's first part starts grounded), so a move can take it.
fn unground_all(s: &mut Session) {
    let grounded: Vec<u64> = s.doc.occurrences.iter().filter(|o| o.grounded).map(|o| o.id).collect();
    for occurrence in grounded {
        run(s, "occurrence.ground", json!({"occurrence": occurrence, "grounded": false}));
    }
}

/// A part placed twice, the copy turned a quarter about X and set at x = 100: `Blade` at
/// x 0…20, y 0…8, z 0…30 and its copy at x 100…120, y −30…0, z 0…8.
fn twice_placed_blade(s: &mut Session) -> (String, String) {
    let made = run(s, "component.create", json!({"name": "Blade"}));
    run(s, "solid.box", json!({"length": 20, "width": 8, "height": 30, "body_name": "Blade"}));
    run(s, "component.activate", json!({"component": 0}));
    let copy = run(s, "occurrence.copy", json!({"occurrence": made["occurrence"]}));
    run(s, "occurrence.move", json!({"occurrence": copy["occurrence"], "axis": [1, 0, 0], "angle": "90 deg"}));
    run(s, "occurrence.move", json!({"occurrence": copy["occurrence"], "translate": [100, 0, 0], "space": "world"}));
    let names: Vec<String> = s.world_state().bodies.iter().map(|b| b.name.clone()).collect();
    assert_eq!(names.len(), 2, "{names:?}");
    unground_all(s);
    (names[0].clone(), names[1].clone())
}

fn shifted(a: solidcraft_geom::Aabb3, b: solidcraft_geom::Aabb3, by: [f64; 3]) -> bool {
    [
        (a.min.x, b.min.x, by[0]),
        (a.max.x, b.max.x, by[0]),
        (a.min.y, b.min.y, by[1]),
        (a.max.y, b.max.y, by[1]),
        (a.min.z, b.min.z, by[2]),
        (a.max.z, b.max.z, by[2]),
    ]
    .iter()
    .all(|(was, now, d)| about(was + d, *now))
}

/// #4768: every placement of a part placed twice, moved together, goes exactly the way asked,
/// however each placement is turned: the shared part was moved once per placement listed, each
/// turning the move its own way, so the turned copy went down Z instead of along Y. One undo
/// step puts both back.
#[test]
fn placements_of_one_part_moved_together_each_go_the_way_asked() {
    let mut s = Session::default();
    let (a, b) = twice_placed_blade(&mut s);
    let (a0, b0) = (placed_bounds(&s, &a), placed_bounds(&s, &b));
    let undo = s.undo.len();
    let r = run(&mut s, "solid.move", json!({"bodies": [a.clone(), b.clone()], "translate": ["240 mm", "-100 mm", "0 mm"]}));
    assert_eq!(r["placements"].as_array().map(Vec::len), Some(2), "{r}");
    assert!(shifted(a0, placed_bounds(&s, &a), [240.0, -100.0, 0.0]), "{a}: {:?}", placed_bounds(&s, &a));
    assert!(shifted(b0, placed_bounds(&s, &b), [240.0, -100.0, 0.0]), "{b}: {:?}", placed_bounds(&s, &b));
    assert_eq!(s.undo.len(), undo + 1, "one undo step");
    run(&mut s, "edit.undo", json!({}));
    assert!(shifted(a0, placed_bounds(&s, &a), [0.0; 3]) && shifted(b0, placed_bounds(&s, &b), [0.0; 3]), "undone");
    // A quarter turn about Z moves each placement about the world origin, as Move turns a body.
    run(&mut s, "solid.move", json!({"bodies": [a.clone(), b.clone()], "axis": [0, 0, 1], "angle": 90}));
    let turned = placed_bounds(&s, &a);
    assert!(about(turned.min.x, -8.0) && about(turned.max.x, 0.0) && about(turned.min.y, 0.0) && about(turned.max.y, 20.0), "{turned:?}");
    let turned = placed_bounds(&s, &b);
    assert!(about(turned.min.x, 0.0) && about(turned.max.x, 30.0) && about(turned.min.y, 100.0) && about(turned.max.y, 120.0), "{turned:?}");
}

/// The same part inserted from a STEP assembly, where a rigid group holds the placements in the
/// imported component: the move goes to the placement of the whole import, once.
#[test]
fn an_imported_assembly_with_a_repeated_part_moves_as_one() {
    let dir = std::env::temp_dir().join(format!("solidcraft-4768-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("rep.step").to_string_lossy().to_string();
    let mut src = Session::default();
    twice_placed_blade(&mut src);
    run(&mut src, "file.export", json!({"path": path}));
    let mut s = Session::default();
    run(&mut s, "file.insert_step", json!({"path": path}));
    unground_all(&mut s);
    let names: Vec<String> = s.world_state().bodies.iter().map(|b| b.name.clone()).collect();
    assert_eq!(names.len(), 2, "{names:?}");
    let before: Vec<_> = names.iter().map(|n| placed_bounds(&s, n)).collect();
    let r = run(&mut s, "solid.move", json!({"bodies": names, "translate": [240, -100, 0]}));
    assert_eq!(r["placements"].as_array().map(Vec::len), Some(1), "the import's placement, once: {r}");
    for (n, b) in names.iter().zip(before) {
        assert!(shifted(b, placed_bounds(&s, n), [240.0, -100.0, 0.0]), "{n}: {:?}", placed_bounds(&s, n));
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// A 20 × 30 × 8 plate made in its own component and stood up by its placement (turned
/// `angle` degrees about X), the way an imported part set upright is: in the world it lies
/// flat at `x0`..`x0 + 20`, y 0..30, z 0..8. The design is active again afterwards.
fn placed_plate(s: &mut Session, name: &str, x0: f64, angle: f64) {
    let made = run(s, "component.create", json!({"name": name, "grounded": false}));
    // Turned +90° about X a point goes (x, y, z) → (x, −z, y); turned −90°, (x, z, −y).
    let corner = if angle > 0.0 { json!([x0, 0, -30]) } else { json!([x0, -8, 0]) };
    let (width, height) = (8, 30);
    run(s, "solid.box", json!({"length": 20, "width": width, "height": height, "corner": corner, "body_name": name}));
    run(s, "occurrence.move", json!({"occurrence": made["occurrence"], "axis": [1, 0, 0], "angle": format!("{angle} deg")}));
    run(s, "component.activate", json!({"component": 0}));
    let b = world_bounds(s, name);
    for (got, want) in [(b.min.x, x0), (b.max.x, x0 + 20.0), (b.min.y, 0.0), (b.max.y, 30.0), (b.min.z, 0.0), (b.max.z, 8.0)] {
        assert!((got - want).abs() < 1e-6, "{name} placed at {b:?}");
    }
}

fn world_bounds(s: &Session, name: &str) -> solidcraft_geom::Aabb3 {
    s.world_state().body(name).unwrap_or_else(|| panic!("no {name}")).mesh().bounds()
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
}

/// Move's vector, axis and pivot are where the view shows them, whatever frame the moved
/// body's component has: a plate stood up by its placement and moved up goes up.
#[test]
fn a_move_goes_the_way_it_is_shown_on_a_placed_component() {
    let mut s = Session::default();
    placed_plate(&mut s, "Plate", 0.0, 90.0);
    run(&mut s, "solid.move", json!({"bodies": ["Plate"], "translate": [0, 0, 10]}));
    let b = world_bounds(&s, "Plate");
    assert!(near(b.min.z, 10.0) && near(b.max.z, 18.0) && near(b.min.y, 0.0) && near(b.max.y, 30.0), "moved up: {b:?}");
    // A quarter turn about the world's Z through the origin: (x, y) → (−y, x).
    run(&mut s, "solid.move", json!({"bodies": ["Plate"], "axis": [0, 0, 1], "angle": 90}));
    let b = world_bounds(&s, "Plate");
    assert!(near(b.min.x, -30.0) && near(b.max.x, 0.0) && near(b.min.y, 0.0) && near(b.max.y, 20.0) && near(b.min.z, 10.0), "turned: {b:?}");
}

/// Align between two parts placed in different frames: both ends are taken where they are
/// shown, so the plate lands on the other one rather than at right angles to it.
#[test]
fn align_takes_both_ends_where_they_are_shown_across_placed_components() {
    let mut s = Session::default();
    placed_plate(&mut s, "A", 0.0, 90.0);
    placed_plate(&mut s, "B", 50.0, -90.0);
    // Corner to corner: a plain move.
    run(&mut s, "solid.align", json!({"bodies": ["A"], "from": [0, 0, 8], "to": [50, 0, 8]}));
    let a = world_bounds(&s, "A");
    assert!(near(a.min.x, 50.0) && near(a.max.x, 70.0) && near(a.min.z, 0.0) && near(a.max.z, 8.0), "{a:?}");
    // Snapped corners, then the top face onto B's top face: A turns over and sits on B.
    run(&mut s, "solid.align", json!({"bodies": ["A"], "from": {"snap": "vertex", "at": [50, 0, 8]}, "to": {"snap": "vertex", "at": [70, 30, 8]}}));
    let a = world_bounds(&s, "A");
    assert!(near(a.min.x, 70.0) && near(a.min.y, 30.0) && near(a.max.z, 8.0), "snapped: {a:?}");
    run(&mut s, "solid.align", json!({"bodies": ["A"], "from_face": [80, 45, 8], "to_face": [60, 15, 8]}));
    let a = world_bounds(&s, "A");
    assert!(near(a.min.z, 8.0) && near(a.max.z, 16.0), "on B's top, not across it: {a:?}");
    assert!(near(0.5 * (a.min.x + a.max.x), 60.0) && near(0.5 * (a.min.y + a.max.y), 15.0), "centred on the picked point: {a:?}");
}

#[test]
fn align_axes_are_world_axes_on_a_placed_component() {
    let mut s = Session::default();
    placed_plate(&mut s, "Plate", 0.0, 90.0);
    let before = world_bounds(&s, "Plate");
    run(&mut s, "solid.align", json!({"bodies": ["Plate"], "from": [0, 0, 0], "to": [50, 20, 30], "axes": [false, true, false]}));
    let after = world_bounds(&s, "Plate");
    // World Y is masked in; X and Z stay where they were. A local-boolean mask would move
    // along the component's local Y (world Z) instead.
    assert!(near(after.min.x, before.min.x) && near(after.max.x, before.max.x), "x: {after:?}");
    assert!(near(after.min.y, before.min.y + 20.0) && near(after.max.y, before.max.y + 20.0), "y: {after:?}");
    assert!(near(after.min.z, before.min.z) && near(after.max.z, before.max.z), "z: {after:?}");
}
