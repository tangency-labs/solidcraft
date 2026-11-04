use crate::Session;
use serde_json::{Value, json};
use solidcore_record::Vec3;
use solidcraft_geom::prelude::*;
use solidcraft_sketch::WireSpace;

fn run(s: &mut Session, cmd: &str, params: Value) -> Value {
    s.execute(cmd, &params).unwrap_or_else(|e| panic!("{cmd}: {e}"))
}

fn near(actual: Vec3, expected: Vec3) {
    assert!(actual.dist(expected) < 1e-7, "{actual:?} != {expected:?}");
}

fn second_frame() -> (Session, u64, u64, u64) {
    let mut s = Session::default();
    let c = run(&mut s, "component.create", json!({"name":"Wire owner"}));
    let component = c["component"].as_u64().unwrap();
    let first = c["occurrence"].as_u64().unwrap();
    let second = run(&mut s, "occurrence.copy", json!({"occurrence":first}))["occurrence"].as_u64().unwrap();
    // One rigid transform: (x,y,z) -> (20-y,30+x,40+z), independent of implementation math.
    run(&mut s, "occurrence.move", json!({"occurrence":second,"axis":[0,0,1],"angle":"90 deg","translate":[20,30,40]}));
    run(&mut s, "component.activate", json!({"component":component,"occurrence":second,"path":[second]}));
    run(&mut s, "sketch.create", json!({"plane":"XY","name":"Space path"}));
    run(&mut s, "sketch.options", json!({"sketch_3d":true}));
    (s, component, first, second)
}

#[test]
fn wire_frame_second_occurrence_world_inputs_move_and_reopen() {
    let (mut s, component, first, second) = second_frame();
    let sketch = s.active_sketch.unwrap();
    let r = run(&mut s, "sketch.line3d", json!({"points":[[17,32,45],[17,32,55],[14,32,55]]}));
    let a = r["curves"][0].as_str().unwrap().to_string();
    let b = r["curves"][1].as_str().unwrap().to_string();
    let spline = run(&mut s, "sketch.spline3d", json!({"points":[[14,32,55],[12,34,56],[10,35,58]]}))["curves"][0].as_str().unwrap().to_string();
    let point = run(&mut s, "sketch.point3d", json!({"point":[17,32,45]}))["point"].as_str().unwrap().to_string();
    let sk = s.doc.sketch(sketch).unwrap();
    for w in &sk.wires {
        assert_eq!(w.space, WireSpace::Component);
    }
    near(sk.wires[sk.wire_index(&a).unwrap()].fit[0], Vec3::new(2.0, 3.0, 5.0));
    near(sk.wires[sk.wire_index(&point).unwrap()].fit[0], Vec3::new(2.0, 3.0, 5.0));
    let world = s.world_state().unwrap();
    let ws = &world.sketch(sketch).unwrap().sketch;
    let sw = &ws.wires[ws.wire_index(&spline).unwrap()];
    near(sw.fit[0], Vec3::new(14.0, 32.0, 55.0));
    near(sw.pts[0], sw.fit[0]);
    near(*sw.pts.last().unwrap(), Vec3::new(10.0, 35.0, 58.0));
    let before = s.doc.to_json();
    let moved = run(&mut s, "sketch.move3d", json!({"wire":a,"index":1,"by":[1,0,0]}));
    assert_eq!(moved["moved"], 2);
    let sk = s.doc.sketch(sketch).unwrap();
    near(sk.wires[sk.wire_index(&a).unwrap()].fit[1], Vec3::new(2.0, 2.0, 15.0));
    near(sk.wires[sk.wire_index(&b).unwrap()].fit[0], Vec3::new(2.0, 2.0, 15.0));
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(s.doc.to_json(), before);
    run(&mut s, "edit.redo", json!({}));
    run(&mut s, "sketch.move3d", json!({"wire":point,"to":[18,33,46]}));
    near(s.doc.sketch(sketch).unwrap().wires.iter().find(|w| w.id == point).unwrap().fit[0], Vec3::new(3.0, 2.0, 6.0));
    let restored = solidcraft_doc::format::read(&s.doc.to_json()).unwrap();
    assert_eq!(restored.sketch(sketch).unwrap().wires, s.doc.sketch(sketch).unwrap().wires);
    // Same definition at the first placement has the local geometry; selected second is exact.
    run(&mut s, "component.activate", json!({"component":component,"occurrence":first,"path":[first]}));
    near(s.world_state().unwrap().sketch(sketch).unwrap().sketch.wires.iter().find(|w| w.id == point).unwrap().fit[0], Vec3::new(3.0, 2.0, 6.0));
    run(&mut s, "component.activate", json!({"component":component,"occurrence":second,"path":[second]}));
    near(s.world_state().unwrap().sketch(sketch).unwrap().sketch.wires.iter().find(|w| w.id == point).unwrap().fit[0], Vec3::new(18.0, 33.0, 46.0));
}

#[test]
fn wire_frame_pending_moves_other_owner_and_legacy_world_do_not_join() {
    let (mut s, _, _, second) = second_frame();
    let sketch = s.active_sketch.unwrap();
    let legacy = s.doc_mut().sketch_mut(sketch).unwrap().add_drawn_wire(&[Vec3::new(2.0, 3.0, 5.0)]).unwrap();
    let local = run(&mut s, "sketch.point3d", json!({"point":[17,32,45]}))["point"].as_str().unwrap().to_string();
    assert_eq!(run(&mut s, "sketch.move3d", json!({"wire":local,"by":[1,0,0]}))["moved"], 1);
    near(s.doc.sketch(sketch).unwrap().wires.iter().find(|w| w.id == legacy).unwrap().fit[0], Vec3::new(2.0, 3.0, 5.0));
    run(&mut s, "occurrence.move", json!({"occurrence":second,"translate":[5,0,0],"capture":false}));
    let point = run(&mut s, "sketch.point3d", json!({"point":[22,32,45]}))["point"].as_str().unwrap().to_string();
    near(s.doc.sketch(sketch).unwrap().wires.iter().find(|w| w.id == point).unwrap().fit[0], Vec3::new(2.0, 3.0, 5.0));
    near(s.world_state().unwrap().sketch(sketch).unwrap().sketch.wires.iter().find(|w| w.id == legacy).unwrap().fit[0], Vec3::new(2.0, 3.0, 5.0));
    near(s.world_state().unwrap().sketch(sketch).unwrap().sketch.wires.iter().find(|w| w.id == point).unwrap().fit[0], Vec3::new(22.0, 32.0, 45.0));
    run(&mut s, "component.capture_position", json!({}));
    near(s.world_state().unwrap().sketch(sketch).unwrap().sketch.wires.iter().find(|w| w.id == point).unwrap().fit[0], Vec3::new(22.0, 32.0, 45.0));
    // Explicit target in root must not inherit the unrelated active component frame.
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "component.activate", json!({"component":0}));
    let root = run(&mut s, "sketch.create", json!({"plane":"XY","name":"Root path"}))["sketch"].as_u64().unwrap();
    run(&mut s, "sketch.options", json!({"sketch_3d":true}));
    let unrelated = run(&mut s, "component.create", json!({"name":"Unrelated"}))["occurrence"].as_u64().unwrap();
    run(&mut s, "occurrence.move", json!({"occurrence":unrelated,"translate":[100,200,300]}));
    run(&mut s, "sketch.point3d", json!({"sketch":root,"point":[9,8,7]}));
    near(s.doc.sketch(root).unwrap().wires[0].fit[0], Vec3::new(9.0, 8.0, 7.0));
}

#[test]
fn wire_frame_hostile_inputs_rollback_full_session() {
    let (mut s, _, _, second) = second_frame();
    let id = run(&mut s, "sketch.point3d", json!({"point":[17,32,45]}))["point"].as_str().unwrap().to_string();
    run(&mut s, "occurrence.move", json!({"occurrence":second,"translate":[1,0,0],"capture":false}));
    run(&mut s, "select.set", json!({"items":[{"type":"occurrence","path":[second]}]}));
    let doc = s.doc.to_json();
    let pending = s.pending_moves.clone();
    let selection = s.selection.clone();
    let history = (s.undo.len(), s.redo.len());
    for (cmd, p) in [
        ("sketch.move3d", json!({"wire":id,"index":u64::MAX,"to":[0,0,0]})),
        ("sketch.move3d", json!({"wire":id,"index":-1,"to":[0,0,0]})),
        ("sketch.move3d", json!({"wire":id,"index":"0","to":[0,0,0]})),
        ("sketch.move3d", json!({"wire":id,"to":[null,0,0],"by":[1,0,0]})),
        ("sketch.point3d", json!({"point":[1e100,0,0]})),
        ("sketch.line3d", json!({"points":[[17,32,45],[17,32,45]]})),
        ("sketch.spline3d", json!({"points":vec![[0,0,0];501]})),
    ] {
        assert!(s.execute(cmd, &p).is_err(), "{cmd}: {p}");
        assert_eq!(s.doc.to_json(), doc);
        assert_eq!(s.pending_moves, pending);
        assert_eq!(s.selection, selection);
        assert_eq!((s.undo.len(), s.redo.len()), history);
    }
}

#[test]
fn wire_frame_include_other_component_refresh_break_lost_and_reopen() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length":10,"width":10,"height":10}));
    let component = run(&mut s, "component.create", json!({"name":"Receiver","grounded":false}));
    let occurrence = component["occurrence"].as_u64().unwrap();
    run(&mut s, "occurrence.move", json!({"occurrence":occurrence,"translate":[20,0,0]}));
    run(&mut s, "sketch.create", json!({"plane":"XY","name":"Include target"}));
    let sketch = s.active_sketch.unwrap();
    run(&mut s, "sketch.include_3d", json!({"refs":[{"edge":[10,10,5],"body":"Body1"}]}));
    let initial = s.doc.sketch(sketch).unwrap().wires[0].clone();
    assert_eq!(initial.space, WireSpace::Component);
    assert!(initial.pts.iter().all(|p| (p.x + 10.0).abs() < 1e-7 && (p.y - 10.0).abs() < 1e-7));
    let link = initial.link.clone().unwrap();
    // A real sketch edit exercises engine refresh, not only document evaluation.
    run(&mut s, "sketch.line", json!({"points":[[0,0],[1,0]]}));
    let sk = s.doc.sketch(sketch).unwrap();
    assert_eq!(sk.wires[0], initial);
    assert!(!sk.link(&link).unwrap().lost);
    let world = s.world_state().unwrap();
    assert!(world.sketch(sketch).unwrap().sketch.wires[0].pts.iter().all(|p| (p.x - 10.0).abs() < 1e-7));
    let before = s.doc.to_json();
    run(&mut s, "sketch.break_link", json!({"link":link}));
    assert_eq!(s.doc.sketch(sketch).unwrap().wires[0].space, WireSpace::Component);
    assert!(s.doc.sketch(sketch).unwrap().wires[0].link.is_none());
    let restored = solidcraft_doc::format::read(&s.doc.to_json()).unwrap();
    assert_eq!(restored.sketch(sketch).unwrap().wires[0], s.doc.sketch(sketch).unwrap().wires[0]);
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(s.doc.to_json(), before);
    // Lost references retain the last coordinates and explicit marker, with no guessing.
    let sk = s.doc_mut().sketch_mut(sketch).unwrap();
    let entry = sk.links.iter_mut().find(|l| l.id == link).unwrap();
    entry.source = solidcraft_sketch::LinkSource::Edge { body: "Missing".into(), at: Vec3::ZERO };
    // The link is the edge it names; without the name it is the edge nearest its point.
    entry.edge_name = None;
    run(&mut s, "sketch.line", json!({"points":[[2,0],[3,0]]}));
    let sk = s.doc.sketch(sketch).unwrap();
    assert!(sk.link(&link).unwrap().lost);
    assert_eq!(sk.wires[0], initial);
    let restored = solidcraft_doc::format::read(&s.doc.to_json()).unwrap();
    assert_eq!(restored.sketch(sketch).unwrap().wires[0], initial);
}

#[test]
fn wire_frame_pipe_component_and_world_paths_have_analytic_volume_and_position() {
    for space in [WireSpace::Component, WireSpace::World] {
        let (mut s, _, first, second) = second_frame();
        if space == WireSpace::World {
            // A single placed owner lets an immutable world path stay physically fixed;
            // replicated solids still follow their occurrence placements by design.
            run(&mut s, "selection.delete", json!({"items":[{"type":"occurrence","path":[first]}]}));
        }
        let sketch = s.active_sketch.unwrap();
        let wire = if space == WireSpace::Component {
            run(&mut s, "sketch.line3d", json!({"points":[[17,32,45],[17,32,55]]}))["curves"][0].as_str().unwrap().to_string()
        } else {
            s.doc_mut().sketch_mut(sketch).unwrap().add_drawn_wire(&[Vec3::new(17.0, 32.0, 45.0), Vec3::new(17.0, 32.0, 55.0)]).unwrap()
        };
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "solid.pipe", json!({"path_sketch":sketch,"path":[wire],"diameter":4}));
        let world = s.world_state().unwrap();
        let body = world.bodies.last().unwrap();
        // Property measurement uses fine adaptive tessellation; the viewport mesh alone
        // undershoots a small circular profile by about 0.64% at its display chord tolerance.
        let measure = solidcraft_kernel::measure(&body.body).unwrap();
        assert!((measure.volume - 40.0 * std::f64::consts::PI).abs() < 0.05, "{}", measure.volume);
        let bounds = body.mesh().bounds();
        near((bounds.min + bounds.max) / 2.0, Vec3::new(17.0, 32.0, 50.0));
        run(&mut s, "occurrence.move", json!({"occurrence":second,"translate":[5,0,0]}));
        let world = s.world_state().unwrap();
        let body = world.bodies.last().unwrap();
        let bounds = body.mesh().bounds();
        near((bounds.min + bounds.max) / 2.0, Vec3::new(if space == WireSpace::Component { 22.0 } else { 17.0 }, 32.0, 50.0));
    }
}

#[test]
fn wire_frame_nonactive_repeated_owner_refuses_ambiguous_world_input() {
    let (mut s, _, _, _) = second_frame();
    let target = s.active_sketch.unwrap();
    run(&mut s, "component.activate", json!({"component":0}));
    let before = s.doc.to_json();
    let error = s.execute("sketch.point3d", &json!({"sketch":target,"point":[0,0,5]})).unwrap_err();
    assert!(error.to_string().contains("activate the desired sketch owner occurrence"));
    assert_eq!(s.doc.to_json(), before);
}

#[test]
fn wire_frame_wide_owner_graph_has_bounded_indexed_traversal() {
    let mut s = Session::default();
    run(&mut s, "component.create", json!({"name":"Target"}));
    let target = run(&mut s, "sketch.create", json!({"plane":"XY"}))["sketch"].as_u64().unwrap();
    run(&mut s, "component.activate", json!({"component":0}));
    // A legal wide assembly used to force one whole occurrence scan for each leaf.
    let doc = s.doc_mut();
    for i in 0..9998u64 {
        doc.components.push(solidcraft_doc::Component { id: 10_000 + i, name: format!("Sibling{i}"), parent: 0, ..Default::default() });
        doc.occurrences.push(solidcraft_doc::Occurrence {
            id: 100_000 + i,
            component: 10_000 + i,
            parent: 0,
            name: format!("Sibling{i}"),
            transform: solidcraft_doc::IDENTITY,
            grounded: false,
        });
    }
    assert_eq!(super::inverse_owner_frame(&s, &json!({"sketch":target}), "sketch.point3d").unwrap(), solidcraft_doc::IDENTITY);
}

#[test]
fn wire_frame_cross_owner_sweep_follows_world_path_edit_undo_and_reopen() {
    let mut s = Session::default();
    let path_sketch = run(&mut s, "sketch.create", json!({"plane":"XY","name":"Root path"}))["sketch"].as_u64().unwrap();
    run(&mut s, "sketch.options", json!({"sketch_3d":true}));
    let wire = run(&mut s, "sketch.line3d", json!({"points":[[17,32,45],[17,32,55]]}))["curves"][0].as_str().unwrap().to_string();
    run(&mut s, "sketch.finish", json!({}));
    let owner = run(&mut s, "component.create", json!({"name":"Sweep owner","grounded":false}))["occurrence"].as_u64().unwrap();
    run(&mut s, "occurrence.move", json!({"occurrence":owner,"axis":[0,0,1],"angle":"90 deg","translate":[20,30,40]}));
    let profile = run(&mut s, "sketch.create", json!({"plane":"XY","offset":5,"name":"Profile"}))["sketch"].as_u64().unwrap();
    run(&mut s, "sketch.circle.center", json!({"center":[2,3],"radius":2}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.sweep", json!({"sketch":profile,"path_sketch":path_sketch,"path":[wire],"operation":"new"}));
    let check = |s: &Session, length: f64| {
        let world = s.world_state().unwrap();
        let body = world.bodies.last().unwrap();
        let measure = solidcraft_kernel::measure(&body.body).unwrap();
        assert!((measure.volume - 4.0 * std::f64::consts::PI * length).abs() < 0.05, "{}", measure.volume);
        near(measure.centroid, Vec3::new(17.0, 32.0, 45.0 + length / 2.0));
    };
    check(&s, 10.0);
    let saved = s.doc.to_json();
    run(&mut s, "sketch.edit", json!({"sketch":path_sketch}));
    run(&mut s, "sketch.move3d", json!({"sketch":path_sketch,"wire":wire,"index":1,"by":[0,0,10]}));
    check(&s, 20.0);
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(s.doc.to_json(), saved);
    check(&s, 10.0);
    let mut reopened = Session { doc: solidcraft_doc::format::read(&saved).unwrap().into(), ..Default::default() };
    reopened.refresh();
    check(&reopened, 10.0);
}

#[test]
fn wire_frame_three_owner_sweep_profile_move_invalidates_warm_cache() {
    let mut s = Session::default();
    let profile_owner = run(&mut s, "component.create", json!({"name":"Profile owner","parent":0,"grounded":false}));
    let profile_occurrence = profile_owner["occurrence"].as_u64().unwrap();
    run(&mut s, "occurrence.move", json!({"occurrence":profile_occurrence,"translate":[4,7,5]}));
    let profile = run(&mut s, "sketch.create", json!({"plane":"XY","name":"Foreign profile"}))["sketch"].as_u64().unwrap();
    run(&mut s, "sketch.circle.center", json!({"center":[0,0],"radius":2}));
    run(&mut s, "sketch.finish", json!({}));

    let path_owner = run(&mut s, "component.create", json!({"name":"Path owner","parent":0}));
    let path_occurrence = path_owner["occurrence"].as_u64().unwrap();
    run(&mut s, "occurrence.move", json!({"occurrence":path_occurrence,"translate":[-12,9,-3]}));
    let path_sketch = run(&mut s, "sketch.create", json!({"plane":"XY","name":"Foreign path"}))["sketch"].as_u64().unwrap();
    run(&mut s, "sketch.options", json!({"sketch_3d":true}));
    let wire = run(&mut s, "sketch.line3d", json!({"points":[[4,7,5],[4,7,15]]}))["curves"][0].as_str().unwrap().to_string();
    run(&mut s, "sketch.finish", json!({}));

    let consumer = run(&mut s, "component.create", json!({"name":"Sweep consumer","parent":0}));
    let consumer_occurrence = consumer["occurrence"].as_u64().unwrap();
    run(&mut s, "occurrence.move", json!({"occurrence":consumer_occurrence,"axis":[0,0,1],"angle":"90 deg","translate":[100,-30,20]}));
    run(&mut s, "solid.sweep", json!({"sketch":profile,"path_sketch":path_sketch,"path":[wire],"operation":"new"}));
    let measure = |session: &Session, x: f64| {
        let state = session.world_state().unwrap();
        let body = state.bodies.last().unwrap();
        assert_eq!(body.component, consumer["component"].as_u64().unwrap());
        let result = solidcraft_kernel::measure(&body.body).unwrap();
        assert!((result.volume - 40.0 * std::f64::consts::PI).abs() < 0.05, "{}", result.volume);
        near(result.centroid, Vec3::new(x, 7.0, 10.0));
        result
    };
    measure(&s, 4.0);
    // A repeated refresh warms the exact same evaluator cache before changing only
    // the foreign profile placement, whose frame must be part of Sweep's digest.
    s.refresh();
    measure(&s, 4.0);
    let saved = s.doc.to_json();
    let fixed_path = s.doc.component_transform(path_owner["component"].as_u64().unwrap());
    let fixed_consumer = s.doc.component_transform(consumer["component"].as_u64().unwrap());
    run(&mut s, "occurrence.move", json!({"occurrence":profile_occurrence,"translate":[3,0,0]}));
    assert_eq!(s.doc.component_transform(path_owner["component"].as_u64().unwrap()), fixed_path);
    assert_eq!(s.doc.component_transform(consumer["component"].as_u64().unwrap()), fixed_consumer);
    let warm = measure(&s, 7.0);
    let mut cold = Session { doc: solidcraft_doc::format::read(&s.doc.to_json()).unwrap().into(), ..Default::default() };
    cold.refresh();
    let reopened = measure(&cold, 7.0);
    near(warm.centroid, reopened.centroid);
    assert!((warm.volume - reopened.volume).abs() < 1e-8);
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(s.doc.to_json(), saved);
    measure(&s, 4.0);
}

#[test]
fn arc3d_is_exact_inspectable_movable_and_reopens() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane":"XY"}));
    run(&mut s, "sketch.options", json!({"sketch_3d":true}));
    let sketch = s.active_sketch.unwrap();
    let d = 5.0 * std::f64::consts::SQRT_2;
    let r = run(&mut s, "sketch.arc3d", json!({"start":[10,0,0],"through":[d,d,0],"end":[0,10,0]}));
    let id = r["curves"][0].as_str().unwrap().to_string();
    let sk = s.doc.sketch(sketch).unwrap();
    let w = &sk.wires[sk.wire_index(&id).unwrap()];
    let arc = w.arc.expect("an exact arc is stored");
    assert!((arc.radius - 10.0).abs() < 1e-9, "radius {}", arc.radius);
    assert!(arc.center.dist(Vec3::ZERO) < 1e-9, "centre {:?}", arc.center);
    assert_eq!(w.fit.len(), 3);
    assert_eq!(w.pts.first(), w.fit.first());
    assert_eq!(w.pts.last(), w.fit.get(2));
    assert!(w.pts.iter().all(|p| (arc.center.dist(*p) - arc.radius).abs() < 1e-6));
    let _ = sk;
    let insp = run(&mut s, "sketch.inspect", json!({}));
    let entry = insp["wires"].as_array().unwrap().iter().find(|e| e["id"] == json!(id)).unwrap();
    assert!((entry["arc"]["radius"].as_f64().unwrap() - 10.0).abs() < 1e-9);
    assert!((entry["arc"]["sweep_deg"].as_f64().unwrap() - 90.0).abs() < 1e-6);
    // Pulling the through point in rebuilds the arc exactly.
    run(&mut s, "sketch.move3d", json!({"wire":id,"index":1,"to":[0,5,0]}));
    let sk = s.doc.sketch(sketch).unwrap();
    let arc = sk.wires[sk.wire_index(&id).unwrap()].arc.unwrap();
    assert!((arc.radius - 62.5_f64.sqrt()).abs() < 1e-9, "radius {}", arc.radius);
    let saved = s.doc.to_json();
    let restored = solidcraft_doc::format::read(&saved).unwrap();
    assert_eq!(restored.sketch(sketch).unwrap().wires, s.doc.sketch(sketch).unwrap().wires);
    let idx = s.doc.sketch(sketch).unwrap().wire_index(&id).unwrap();
    run(&mut s, "edit.undo", json!({}));
    assert!((s.doc.sketch(sketch).unwrap().wires[idx].arc.unwrap().radius - 10.0).abs() < 1e-9);
}

#[test]
fn arc3d_follows_the_sketch_owner_placement() {
    let (mut s, _, _, second) = second_frame();
    let sketch = s.active_sketch.unwrap();
    let d = 5.0 * std::f64::consts::SQRT_2;
    // World points of a quarter circle whose local centre is (0,0,5), radius 10.
    let id = run(&mut s, "sketch.arc3d", json!({"start":[20,40,45],"through":[20.0 - d,30.0 + d,45],"end":[10,30,45]}))["curves"][0]
        .as_str()
        .unwrap()
        .to_string();
    let sk = s.doc.sketch(sketch).unwrap();
    let arc = sk.wires[sk.wire_index(&id).unwrap()].arc.unwrap();
    near(arc.center, Vec3::new(0.0, 0.0, 5.0));
    assert!((arc.radius - 10.0).abs() < 1e-7, "radius {}", arc.radius);
    let _ = sk;
    let world = s.world_state().unwrap();
    let ws = &world.sketch(sketch).unwrap().sketch;
    let arc = ws.wires[ws.wire_index(&id).unwrap()].arc.unwrap();
    near(arc.center, Vec3::new(20.0, 30.0, 45.0));
    near(ws.wires[ws.wire_index(&id).unwrap()].fit[0], Vec3::new(20.0, 40.0, 45.0));
    drop(world);
    run(&mut s, "occurrence.move", json!({"occurrence":second,"translate":[5,0,0]}));
    let world = s.world_state().unwrap();
    let ws = &world.sketch(sketch).unwrap().sketch;
    let arc = ws.wires[ws.wire_index(&id).unwrap()].arc.unwrap();
    near(arc.center, Vec3::new(25.0, 30.0, 45.0));
    assert!((arc.radius - 10.0).abs() < 1e-7, "radius {}", arc.radius);
}

#[test]
fn arc3d_hostile_inputs_rollback_full_session() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane":"XY"}));
    run(&mut s, "sketch.options", json!({"sketch_3d":true}));
    let doc = s.doc.to_json();
    let history = (s.undo.len(), s.redo.len());
    for (cmd, p) in [
        ("sketch.arc3d", json!({"start":[0,0,0],"through":[1,0,0],"end":[2,0,0]})), // collinear
        ("sketch.arc3d", json!({"start":[0,0,0],"through":[0,0,0],"end":[0,10,0]})), // coincident
        ("sketch.arc3d", json!({"start":[0,0,0],"through":[f64::NAN,5,0],"end":[10,0,0]})),
        ("sketch.arc3d", json!({"start":[0,0,0],"through":[1e100,5,0],"end":[10,0,0]})),
        ("sketch.arc3d", json!({"start":[0,0,0],"through":[5,5,0]})), // missing end
        ("sketch.arc3d", json!({"start":[0,0,0],"through":["x",5,0],"end":[10,0,0]})),
        ("sketch.arc3d", json!({"through":[5,5,0],"end":[10,0,0]})), // missing start
    ] {
        assert!(s.execute(cmd, &p).is_err(), "{cmd}: {p}");
        assert_eq!(s.doc.to_json(), doc);
        assert_eq!((s.undo.len(), s.redo.len()), history);
    }
}

#[test]
fn arc3d_pipes_along_the_arc() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane":"XY"}));
    run(&mut s, "sketch.options", json!({"sketch_3d":true}));
    let sketch = s.active_sketch.unwrap();
    let d = 10.0 * std::f64::consts::SQRT_2;
    let wire = run(&mut s, "sketch.arc3d", json!({"start":[20,0,0],"through":[d,d,0],"end":[0,20,0]}))["curves"][0].as_str().unwrap().to_string();
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.pipe", json!({"path_sketch":sketch,"path":[wire],"diameter":4}));
    let world = s.world_state().unwrap();
    let body = world.bodies.last().unwrap();
    let measure = solidcraft_kernel::measure(&body.body).unwrap();
    let quarter = 20.0 * std::f64::consts::FRAC_PI_2;
    assert!((measure.volume - 4.0 * std::f64::consts::PI * quarter).abs() < 0.5, "{}", measure.volume);
}

#[test]
fn line3d_spline3d_point3d_need_a_3d_sketch() {
    // Same rule as sketch.arc3d: these buttons start interactive tools, so they are only
    // offered while a 3D sketch is being edited.
    let ids = ["sketch.line3d", "sketch.spline3d", "sketch.point3d"];
    let mut s = Session::default();
    for id in ids {
        let info = crate::cmd::find_command(id).unwrap().info(&s);
        assert!(!info.enabled, "{id} offered with no sketch being edited");
    }
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    for id in ids {
        let info = crate::cmd::find_command(id).unwrap().info(&s);
        assert!(!info.enabled, "{id} offered in a plain 2D sketch: {:?}", info.disabled_reason);
    }
    run(&mut s, "sketch.options", json!({"sketch_3d": true}));
    for id in ids {
        let info = crate::cmd::find_command(id).unwrap().info(&s);
        assert!(info.enabled, "{id} not offered inside a 3D sketch: {:?}", info.disabled_reason);
    }
}

#[test]
fn wire_frame_copies_keep_component_and_legacy_coordinates() {
    let (mut s, _, first, second) = second_frame();
    let sketch = s.active_sketch.unwrap();
    let fit = [Vec3::new(20.0, 40.0, 45.0), Vec3::new(10.0, 30.0, 45.0), Vec3::new(20.0, 20.0, 45.0)];
    let local = run(&mut s, "sketch.arc3d", json!({"start":fit[0],"through":fit[1],"end":fit[2]}))["curves"][0].as_str().unwrap().to_owned();
    let legacy = s.doc_mut().sketch_mut(sketch).unwrap().add_drawn_arc_in(&fit, WireSpace::World).unwrap();
    s.refresh();
    let originals = s.doc.sketch(sketch).unwrap().wires.clone();
    let out = run(&mut s, "sketch.move", json!({"entities":[local,legacy],"copy":true,"translate":[1.6,0],"angle":90}));
    let copied: Vec<_> = out["curves"].as_array().unwrap().iter().map(|id| id.as_str().unwrap().to_owned()).collect();
    assert_eq!(copied.len(), 2);
    let sk = s.doc.sketch(sketch).unwrap();
    assert_eq!(&sk.wires[..2], &originals);
    assert_eq!(sk.wires[2].space, WireSpace::Component);
    assert_eq!(sk.wires[3].space, WireSpace::World);
    near(sk.wires[2].arc.unwrap().center, Vec3::new(1.6, 0.0, 5.0));
    near(sk.wires[3].arc.unwrap().center, Vec3::new(20.0, 31.6, 45.0));
    for wire in s.world_state().unwrap().sketch(sketch).unwrap().sketch.wires.iter().filter(|w| copied.contains(&w.id)) {
        near(wire.arc.unwrap().center, Vec3::new(20.0, 31.6, 45.0));
        near(wire.fit[0], Vec3::new(10.0, 31.6, 45.0));
    }
    let component = s.active_component;
    run(&mut s, "component.activate", json!({"component":component,"occurrence":first,"path":[first]}));
    let world = s.world_state().unwrap();
    let wires = &world.sketch(sketch).unwrap().sketch.wires;
    near(wires.iter().find(|w| w.id == copied[0]).unwrap().arc.unwrap().center, Vec3::new(1.6, 0.0, 5.0));
    near(wires.iter().find(|w| w.id == copied[1]).unwrap().arc.unwrap().center, Vec3::new(20.0, 31.6, 45.0));
    run(&mut s, "component.activate", json!({"component":component,"occurrence":second,"path":[second]}));
    let restored = solidcraft_doc::format::read(&s.doc.to_json()).unwrap();
    assert_eq!(restored.sketch(sketch).unwrap().wires, s.doc.sketch(sketch).unwrap().wires);
}

/// Where a fit point of the active sketch's 3D curve `wire` is.
fn fit(s: &Session, wire: &str, index: usize) -> Vec3 {
    let sk = s.doc.sketch(s.active_sketch.unwrap()).unwrap();
    sk.wires[sk.wire_index(wire).unwrap()].fit[index]
}

#[test]
fn constraints_3d_join_hold_axes_follow_moves_undo_and_reopen() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane":"XY"}));
    run(&mut s, "sketch.options", json!({"sketch_3d":true}));
    let sketch = s.active_sketch.unwrap();
    // A run along X and a rise along Z, drawn a little off both.
    let r = run(&mut s, "sketch.line3d", json!({"points":[[0,0,0],[100,2,3],[101,3,60]]}));
    let (a, b) = (r["curves"][0].as_str().unwrap().to_string(), r["curves"][1].as_str().unwrap().to_string());
    assert_eq!(r["constraints"].as_array().map(Vec::len), Some(1), "the joint: {r}");
    run(&mut s, "sketch.constraint.fix", json!({"entity": format!("{a}.start")}));
    run(&mut s, "sketch.constraint.along_axis", json!({"line": a, "axis": "x"}));
    let last = run(&mut s, "sketch.constraint.along_axis", json!({"line": b, "axis": "z"}));
    near(fit(&s, &a, 0), Vec3::new(0.0, 0.0, 0.0));
    let corner = fit(&s, &a, 1);
    assert!(corner.y.abs() < 1e-9 && corner.z.abs() < 1e-9, "{corner:?}");
    near(fit(&s, &b, 0), corner);
    let top = fit(&s, &b, 1);
    assert!((top.x - corner.x).abs() < 1e-9 && (top.y - corner.y).abs() < 1e-9, "{top:?}");
    // 4 points: 12 coordinates, less the fix (3), the joint (3) and two axes (2 each).
    assert_eq!(last["sketch"]["dof"], 2, "{last}");

    // Dragging the top along X takes the corner with it; the start stays.
    run(&mut s, "sketch.move3d", json!({"wire": b, "index": 1, "by": [5, 0, 0]}));
    near(fit(&s, &a, 0), Vec3::new(0.0, 0.0, 0.0));
    assert!((fit(&s, &a, 1).x - (corner.x + 5.0)).abs() < 1e-6 && (fit(&s, &b, 0).x - (corner.x + 5.0)).abs() < 1e-6);
    // A fixed point does not move, and a move the constraints cannot hold changes nothing.
    let before = s.doc.to_json();
    assert!(s.execute("sketch.move3d", &json!({"wire": a, "index": 0, "to": [1, 1, 1]})).is_err());
    assert!(s.execute("sketch.move3d", &json!({"wire": b, "index": 1, "by": [0, 0, 0.5], "to": [3, 9, 70]})).is_err());
    assert_eq!(s.doc.to_json(), before);
    // A constraint that conflicts is refused, the design unchanged.
    let e = s.execute("sketch.constraint.along_axis", &json!({"line": a, "axis": "y"})).unwrap_err().to_string();
    assert!(e.contains("over-constrain"), "{e}");
    assert_eq!(s.doc.to_json(), before);

    // It is saved and read back; undo takes the last constraint off, and delete removes one.
    let restored = solidcraft_doc::format::read(&s.doc.to_json()).unwrap();
    assert_eq!(restored.sketch(sketch).unwrap().wire_constraints, s.doc.sketch(sketch).unwrap().wire_constraints);
    let n = s.doc.sketch(sketch).unwrap().wire_constraints.len();
    let id = last["constraints"][0].as_str().unwrap().to_string();
    run(&mut s, "sketch.delete", json!({"entities": [id]}));
    assert_eq!(s.doc.sketch(sketch).unwrap().wire_constraints.len(), n - 1);
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(s.doc.sketch(sketch).unwrap().wire_constraints.len(), n);
    // Deleting a curve takes its constraints with it.
    run(&mut s, "sketch.delete", json!({"entities": [b]}));
    assert!(s.doc.sketch(sketch).unwrap().wire_constraints.iter().all(|c| !c.kind.wires().contains(&b.as_str())));
}

#[test]
fn coincident_3d_joins_points_puts_a_point_on_a_line_and_keeps_planar_geometry_apart() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane":"XY"}));
    run(&mut s, "sketch.options", json!({"sketch_3d":true}));
    let a = run(&mut s, "sketch.line3d", json!({"points":[[0,0,0],[10,0,0]]}))["curves"][0].as_str().unwrap().to_string();
    let b = run(&mut s, "sketch.line3d", json!({"points":[[10,1,1],[10,5,9]]}))["curves"][0].as_str().unwrap().to_string();
    run(&mut s, "sketch.constraint.fix", json!({"entity": a}));
    run(&mut s, "sketch.constraint.coincident", json!({"a": format!("{a}.end"), "b": format!("{b}.start")}));
    near(fit(&s, &b, 0), Vec3::new(10.0, 0.0, 0.0));
    // A 3D point dropped onto the fixed line, straight down.
    let q = run(&mut s, "sketch.point3d", json!({"point":[4,3,4]}))["point"].as_str().unwrap().to_string();
    run(&mut s, "sketch.constraint.coincident", json!({"a": q, "b": a}));
    near(fit(&s, &q, 0), Vec3::new(4.0, 0.0, 0.0));
    // Planar geometry stays apart in this step.
    let e = s.execute("sketch.constraint.coincident", &json!({"a": q, "b": "origin"})).unwrap_err().to_string();
    assert!(e.contains("#10616"), "{e}");
    // Unfix releases every point of the line.
    let r = run(&mut s, "sketch.constraint.fix", json!({"entity": a, "fixed": false}));
    assert_eq!(r["result"]["changed"], 2, "{r}");
}

#[test]
fn a_constrained_3d_line_follows_its_constraints_through_a_drag() {
    // The pipe built along it is graded in tests/kernel-cases/sweeps (#10613).
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane":"XY"}));
    run(&mut s, "sketch.options", json!({"sketch_3d":true}));
    let l = run(&mut s, "sketch.line3d", json!({"points":[[0,0,0],[50,3,4]]}))["curves"][0].as_str().unwrap().to_string();
    run(&mut s, "sketch.constraint.fix", json!({"entity": format!("{l}.start")}));
    // Along X with the start fixed: the end drops straight onto the axis, 50 along.
    run(&mut s, "sketch.constraint.along_axis", json!({"line": l, "axis": "x"}));
    near(fit(&s, &l, 1), Vec3::new(50.0, 0.0, 0.0));
    // Stretched along the axis by a drag, it stays on it.
    run(&mut s, "sketch.move3d", json!({"wire": l, "index": 1, "by": [10, 0, 0]}));
    near(fit(&s, &l, 1), Vec3::new(60.0, 0.0, 0.0));
}

#[test]
fn planar_constraints_and_dimensions_count_without_the_3d_curves() {
    // A free 3D line beside a planar line: the planar edits measure the planar sketch alone.
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane":"XY"}));
    run(&mut s, "sketch.options", json!({"sketch_3d":true}));
    run(&mut s, "sketch.line3d", json!({"points":[[0,0,0],[10,5,5]]}));
    let l = run(&mut s, "sketch.line", json!({"points":[[2,3],[10,4]]}))["curves"][0].as_str().unwrap().to_string();
    let r = run(&mut s, "sketch.constraint.horizontal_vertical", json!({"line": l, "mode": "horizontal"}));
    // The planar line's 4 less 1, and the 3D line's 6.
    assert_eq!(r["sketch"]["dof"], 3 + 6, "{r}");
    let r = run(&mut s, "sketch.dimension", json!({"entities": [l], "value": 25}));
    assert_eq!(r["sketch"]["dof"], 2 + 6, "{r}");
    let sketch = s.active_sketch.unwrap();
    let st = s.model.state();
    let ss = st.sketch(sketch).unwrap();
    assert_eq!((ss.report.dof, ss.report.shown_dof()), (2, 8));
}

#[test]
fn lines_drawn_one_segment_at_a_time_stay_joined_through_their_constraints() {
    // As the 3D Line tool draws: one call per segment.
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane":"XY"}));
    run(&mut s, "sketch.options", json!({"sketch_3d":true}));
    let a = run(&mut s, "sketch.line3d", json!({"points":[[0,0,0],[50,3,4]]}))["curves"][0].as_str().unwrap().to_string();
    let r = run(&mut s, "sketch.line3d", json!({"points":[[50,3,4],[52,5,40]]}));
    let b = r["curves"][0].as_str().unwrap().to_string();
    assert_eq!(r["constraints"].as_array().map(Vec::len), Some(1), "joined to the first: {r}");
    run(&mut s, "sketch.constraint.fix", json!({"entity": format!("{a}.start")}));
    run(&mut s, "sketch.constraint.along_axis", json!({"line": a, "axis": "x"}));
    let r = run(&mut s, "sketch.constraint.along_axis", json!({"line": b, "axis": "z"}));
    assert_eq!(r["created"]["constraints"].as_array().map(Vec::len), Some(1), "{r}");
    near(fit(&s, &b, 0), fit(&s, &a, 1));
    let top = fit(&s, &b, 1);
    assert!((top.x - fit(&s, &b, 0).x).abs() < 1e-9 && (top.y - fit(&s, &b, 0).y).abs() < 1e-9, "the rise stands on the joint: {top:?}");
}

#[test]
fn a_3d_conflict_does_not_block_planar_edits_and_shows() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane":"XY"}));
    run(&mut s, "sketch.options", json!({"sketch_3d":true}));
    let sketch = s.active_sketch.unwrap();
    let l3 = run(&mut s, "sketch.line3d", json!({"points":[[0,0,0],[10,5,5]]}))["curves"][0].as_str().unwrap().to_string();
    // A design whose 3D constraints conflict (the end fixed off the axis it is held to).
    let mut doc = (*s.doc).clone();
    let sk = doc.sketch_mut(sketch).unwrap();
    let end = sk.wire_point(&format!("{l3}.end")).unwrap();
    let start = sk.wire_point(&format!("{l3}.start")).unwrap();
    sk.add_wire_constraint(solidcraft_sketch::WireConstraintKind::Fix { p: start, at: Vec3::ZERO, exprs: None }).unwrap();
    sk.add_wire_constraint(solidcraft_sketch::WireConstraintKind::Fix { p: end, at: Vec3::new(10.0, 5.0, 5.0), exprs: None }).unwrap();
    sk.add_wire_constraint(solidcraft_sketch::WireConstraintKind::AlongAxis { line: l3.clone(), axis: solidcraft_sketch::Axis::X }).unwrap();
    s.set_doc(doc);
    let l = run(&mut s, "sketch.line", json!({"points":[[0,0],[10,1]]}))["curves"][0].as_str().unwrap().to_string();
    let r = run(&mut s, "sketch.constraint.horizontal_vertical", json!({"line": l, "mode": "horizontal"}));
    // `solved` is the planar sketch's; the 3D conflict is its own.
    assert_eq!((r["sketch"]["solved"].as_bool(), r["sketch"]["wires_solved"].as_bool()), (Some(true), Some(false)), "{r}");
    let info = run(&mut s, "sketch.inspect", json!({}));
    assert_eq!(info["status"], "solved", "{info}");
    assert_eq!(info["wires_solve"]["ok"], false, "{info}");
    let st = s.model.state();
    assert!(!st.sketch(sketch).unwrap().report.shown_ok());
}

#[test]
fn a_3d_curve_and_its_own_constraint_delete_together() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane":"XY"}));
    run(&mut s, "sketch.options", json!({"sketch_3d":true}));
    let l = run(&mut s, "sketch.line3d", json!({"points":[[0,0,0],[10,1,1]]}))["curves"][0].as_str().unwrap().to_string();
    let k = run(&mut s, "sketch.constraint.along_axis", json!({"line": l, "axis": "x"}))["constraints"][0].as_str().unwrap().to_string();
    let info = run(&mut s, "sketch.inspect", json!({}));
    assert!(info["wire_constraints"].as_array().is_some_and(|a| a.iter().any(|c| c["id"] == k.as_str())), "{info}");
    run(&mut s, "sketch.delete", json!({"entities": [l, k]}));
    let sk = s.doc.sketch(s.active_sketch.unwrap()).unwrap();
    assert!(sk.wires.is_empty() && sk.wire_constraints.is_empty());
}

#[test]
fn a_planar_conflict_does_not_block_3d_edits_and_solved_groups_still_move() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane":"XY"}));
    run(&mut s, "sketch.options", json!({"sketch_3d":true}));
    let sketch = s.active_sketch.unwrap();
    let l = run(&mut s, "sketch.line", json!({"points":[[2,3],[10,4]]}))["curves"][0].as_str().unwrap().to_string();
    // A planar line held both horizontal and vertical: the planar sketch conflicts.
    let mut doc = (*s.doc).clone();
    let sk = doc.sketch_mut(sketch).unwrap();
    let c = sk.curve_index(&l).unwrap();
    sk.add_constraint(solidcraft_sketch::ConstraintKind::Horizontal { l: c }, None).unwrap();
    sk.add_constraint(solidcraft_sketch::ConstraintKind::Vertical { l: c }, None).unwrap();
    s.set_doc(doc);
    // 3D edits go ahead.
    let a = run(&mut s, "sketch.line3d", json!({"points":[[0,0,0],[10,1,1]]}))["curves"][0].as_str().unwrap().to_string();
    run(&mut s, "sketch.constraint.fix", json!({"entity": format!("{a}.start")}));
    run(&mut s, "sketch.constraint.along_axis", json!({"line": a, "axis": "x"}));
    let end = fit(&s, &a, 1);
    assert!(end.y.abs() < 1e-9 && end.z.abs() < 1e-9, "{end:?}");
}

#[test]
fn a_3d_line_is_not_squeezed_to_a_point() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane":"XY"}));
    run(&mut s, "sketch.options", json!({"sketch_3d":true}));
    let l = run(&mut s, "sketch.line3d", json!({"points":[[0,0,0],[10,0.000001,0]]}))["curves"][0].as_str().unwrap().to_string();
    run(&mut s, "sketch.constraint.fix", json!({"entity": format!("{l}.start")}));
    let before = s.doc.to_json();
    // Along Y would leave a millionth of a 10 mm line (the limits planar lines have).
    let e = s.execute("sketch.constraint.along_axis", &json!({"line": l, "axis": "y"})).unwrap_err().to_string();
    assert!(e.contains("shrink"), "{e}");
    assert_eq!(s.doc.to_json(), before);
}

#[test]
fn a_deleted_joint_stays_deleted() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane":"XY"}));
    run(&mut s, "sketch.options", json!({"sketch_3d":true}));
    let r = run(&mut s, "sketch.line3d", json!({"points":[[0,0,0],[10,1,1],[10,1,20]]}));
    let (a, b) = (r["curves"][0].as_str().unwrap().to_string(), r["curves"][1].as_str().unwrap().to_string());
    let joint = r["constraints"][0].as_str().unwrap().to_string();
    run(&mut s, "sketch.constraint.along_axis", json!({"line": a, "axis": "x"}));
    run(&mut s, "sketch.delete", json!({"entities": [joint]}));
    run(&mut s, "sketch.constraint.along_axis", json!({"line": b, "axis": "z"}));
    let sketch = s.active_sketch.unwrap();
    let joins = |s: &Session| {
        s.doc
            .sketch(sketch)
            .unwrap()
            .wire_constraints
            .iter()
            .filter(|c| matches!(c.kind, solidcraft_sketch::WireConstraintKind::Coincident { .. }))
            .count()
    };
    assert_eq!(joins(&s), 0, "the deleted joint is not put back");
}

#[test]
fn a_drag_cannot_squeeze_a_3d_line_away() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane":"XY"}));
    run(&mut s, "sketch.options", json!({"sketch_3d":true}));
    let r = run(&mut s, "sketch.line3d", json!({"points":[[0,0,0],[10,0,0],[10,0,20]]}));
    let (a, b) = (r["curves"][0].as_str().unwrap().to_string(), r["curves"][1].as_str().unwrap().to_string());
    run(&mut s, "sketch.constraint.fix", json!({"entity": format!("{a}.start")}));
    run(&mut s, "sketch.constraint.along_axis", json!({"line": a, "axis": "x"}));
    run(&mut s, "sketch.constraint.along_axis", json!({"line": b, "axis": "z"}));
    let before = s.doc.to_json();
    // Dragging the top to x = 1e-6 would leave the run a micron long.
    let e = s.execute("sketch.move3d", &json!({"wire": b, "index": 1, "to": [0.000001, 0, 20]})).unwrap_err().to_string();
    assert!(e.contains("shrink"), "{e}");
    assert_eq!(s.doc.to_json(), before);
}

#[test]
fn an_older_unjoined_path_is_joined_once_and_a_deleted_joint_stays_gone() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane":"XY"}));
    run(&mut s, "sketch.options", json!({"sketch_3d":true}));
    let sketch = s.active_sketch.unwrap();
    let r = run(&mut s, "sketch.line3d", json!({"points":[[0,0,0],[10,1,1],[10,1,20]]}));
    let (a, b) = (r["curves"][0].as_str().unwrap().to_string(), r["curves"][1].as_str().unwrap().to_string());
    // As a design saved before 3D constraints: the path, no joints, not joined yet.
    let mut doc = (*s.doc).clone();
    let sk = doc.sketch_mut(sketch).unwrap();
    sk.wire_constraints.clear();
    sk.wires_joined = false;
    s.set_doc(doc);
    // A curve drawn elsewhere joins the old path too; constraining the first line keeps the second on it.
    run(&mut s, "sketch.line3d", json!({"points":[[50,0,0],[60,0,0]]}));
    run(&mut s, "sketch.constraint.along_axis", json!({"line": a, "axis": "x"}));
    near(fit(&s, &b, 0), fit(&s, &a, 1));
    // Its only joint deleted (and every other constraint), the next one does not put it back.
    let ids: Vec<String> = s.doc.sketch(sketch).unwrap().wire_constraints.iter().map(|c| c.id.clone()).collect();
    run(&mut s, "sketch.delete", json!({"entities": ids}));
    run(&mut s, "sketch.constraint.along_axis", json!({"line": b, "axis": "z"}));
    let joins = s
        .doc
        .sketch(sketch)
        .unwrap()
        .wire_constraints
        .iter()
        .filter(|c| matches!(c.kind, solidcraft_sketch::WireConstraintKind::Coincident { .. }))
        .count();
    assert_eq!(joins, 0);
}

#[test]
fn a_motion_trace_curve_takes_no_constraints_and_counts_no_freedoms() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane":"XY"}));
    run(&mut s, "sketch.options", json!({"sketch_3d":true}));
    let sketch = s.active_sketch.unwrap();
    let t = run(&mut s, "sketch.spline3d", json!({"points":[[0,0,0],[5,2,1],[10,0,3]]}))["curves"][0].as_str().unwrap().to_string();
    let mut doc = (*s.doc).clone();
    doc.assembly.traces.push(solidcraft_doc::motion::MotionTrace {
        sketch,
        wire: t.clone(),
        occurrence: Vec::new(),
        point: solidcraft_doc::joints::JointOrigin {
            occurrence: 0,
            snap: solidcraft_doc::joints::Snap::Frame { origin: Vec3::ZERO, z: Vec3::new(0.0, 0.0, 1.0), x: None },
        },
        motion: solidcraft_doc::motion::TraceMotion::Study { study: 0 },
        samples: None,
        seeds: Vec::new(),
    });
    s.set_doc(doc);
    let e = s.execute("sketch.constraint.fix", &json!({"entity": format!("{t}.start")})).unwrap_err().to_string();
    assert!(e.contains("motion trace"), "{e}");
    // Its fit points are no freedoms to take away: a line drawn beside it adds its own 6 only.
    let r = run(&mut s, "sketch.line3d", json!({"points":[[0,20,0],[10,20,5]]}));
    assert_eq!(r["sketch"]["dof"], 6, "{r}");
}

/// Where a fit point of 3D curve `wire` lands once the model is evaluated (parameters applied).
fn solved_fit(s: &Session, sketch: u64, wire: &str, index: usize) -> Vec3 {
    let world = s.world_state().unwrap();
    let sk = &world.sketch(sketch).unwrap().sketch;
    sk.wires[sk.wire_index(wire).unwrap()].fit[index]
}

#[test]
fn a_3d_line_placed_by_expressions_follows_the_parameters() {
    // The runner of #10608: a line on the tube top, from mid-length to 370 short of the end.
    let mut s = Session::default();
    run(&mut s, "parameters.change", json!({"name": "len", "expression": "3500 mm"}));
    run(&mut s, "parameters.change", json!({"name": "shs", "expression": "150 mm"}));
    run(&mut s, "sketch.create", json!({"plane":"XY"}));
    run(&mut s, "sketch.options", json!({"sketch_3d":true}));
    let sketch = s.active_sketch.unwrap();
    let r = run(&mut s, "sketch.line3d", json!({"points": [["len / 2", 0, "shs"], ["len - 370 mm", 0, "shs"]]}));
    let l = r["curves"][0].as_str().unwrap().to_string();
    assert_eq!(r["constraints"].as_array().map(Vec::len), Some(2), "a placed Fix per point: {r}");
    run(&mut s, "sketch.finish", json!({}));
    near(solved_fit(&s, sketch, &l, 0), Vec3::new(1750.0, 0.0, 150.0));
    near(solved_fit(&s, sketch, &l, 1), Vec3::new(3130.0, 0.0, 150.0));
    run(&mut s, "parameters.change", json!({"name": "shs", "expression": "120 mm"}));
    run(&mut s, "parameters.change", json!({"name": "len", "expression": "3000 mm"}));
    near(solved_fit(&s, sketch, &l, 0), Vec3::new(1500.0, 0.0, 120.0));
    near(solved_fit(&s, sketch, &l, 1), Vec3::new(2630.0, 0.0, 120.0));
    // Renaming a parameter renames it in the point's expressions, and they still evaluate.
    run(&mut s, "parameters.rename", json!({"name": "shs", "new_name": "tube"}));
    run(&mut s, "parameters.change", json!({"name": "tube", "expression": "100 mm"}));
    near(solved_fit(&s, sketch, &l, 0), Vec3::new(1500.0, 0.0, 100.0));
    // An unknown parameter is refused, the design unchanged.
    let before = s.doc.to_json();
    run(&mut s, "sketch.edit", json!({"sketch": sketch}));
    assert!(s.execute("sketch.point3d", &json!({"point": [0, "nope", 0]})).is_err());
    assert_eq!(s.doc.to_json(), before);
}

#[test]
fn dimensions_3d_drive_measure_and_follow_their_parameters() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane":"XY"}));
    run(&mut s, "sketch.options", json!({"sketch_3d":true}));
    let sketch = s.active_sketch.unwrap();
    let r = run(&mut s, "sketch.line3d", json!({"points":[[0,0,0],[100,0,0],[100,0,60]]}));
    let (a, b) = (r["curves"][0].as_str().unwrap().to_string(), r["curves"][1].as_str().unwrap().to_string());
    run(&mut s, "sketch.constraint.fix", json!({"entity": format!("{a}.start")}));
    run(&mut s, "sketch.constraint.along_axis", json!({"line": a, "axis": "x"}));
    // The run's length, as a new user parameter.
    let d = run(&mut s, "sketch.dimension", json!({"entities": [a], "value": "run = 120"}));
    assert_eq!((d["type"].as_str(), d["defined"].as_str()), (Some("length"), Some("run")), "{d}");
    assert!((d["measured"].as_f64().unwrap() - 120.0).abs() < 1e-6, "{d}");
    near(fit(&s, &a, 1), Vec3::new(120.0, 0.0, 0.0));
    // The rise: 60 along Z from the corner, and the angle between run and rise.
    let z = run(&mut s, "sketch.dimension", json!({"entities": [format!("{a}.end"), format!("{b}.end")], "type": "distance_z", "value": 80}));
    assert_eq!(z["type"], "distance_z", "{z}");
    let angle = run(&mut s, "sketch.dimension", json!({"entities": [a, b], "value": "90 deg"}));
    assert_eq!(angle["unit"], "deg", "{angle}");
    let top = fit(&s, &b, 1);
    assert!((top.x - 120.0).abs() < 1e-6 && top.y.abs() < 1e-6 && (top.z - 80.0).abs() < 1e-6, "{top:?}");
    // The parameter drives it after the sketch is closed.
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "parameters.change", json!({"name": "run", "expression": "150 mm"}));
    near(solved_fit(&s, sketch, &a, 1), Vec3::new(150.0, 0.0, 0.0));
    near(solved_fit(&s, sketch, &b, 1), Vec3::new(150.0, 0.0, 80.0));
    // A dimension the other constraints already settle is refused; driven, it measures.
    run(&mut s, "sketch.edit", json!({"sketch": sketch}));
    let before = s.doc.to_json();
    let e = s
        .execute("sketch.dimension", &json!({"entities": [format!("{a}.start"), format!("{a}.end")], "type": "distance_x"}))
        .unwrap_err()
        .to_string();
    assert!(e.contains("already determined"), "{e}");
    assert_eq!(s.doc.to_json(), before);
    let driven = run(&mut s, "sketch.dimension", json!({"entities": [format!("{a}.start"), format!("{b}.end")], "driven": true}));
    let diagonal = (150.0f64 * 150.0 + 80.0 * 80.0).sqrt();
    assert!((driven["measured"].as_f64().unwrap() - diagonal).abs() < 1e-6, "{driven}");
    // The dimensions' parameters are in use: not pruned, and they survive a save.
    let restored = solidcraft_doc::format::read(&s.doc.to_json()).unwrap();
    let params = restored.sketch(sketch).unwrap().dimension_params().into_iter().cloned().collect::<Vec<_>>();
    assert_eq!(params.len(), 3, "{params:?}");
    for p in &params {
        assert!(restored.param(p).is_some(), "{p}");
    }
}

/// A run along X from a fixed start, dimensioned by parameter `run`, then a rise from its end.
fn run_and_rise(s: &mut Session) -> (u64, String, String) {
    run(s, "sketch.create", json!({"plane":"XY"}));
    run(s, "sketch.options", json!({"sketch_3d":true}));
    let sketch = s.active_sketch.unwrap();
    let r = run(s, "sketch.line3d", json!({"points":[[0,0,0],[100,0,0],[100,0,60]]}));
    let (a, b) = (r["curves"][0].as_str().unwrap().to_string(), r["curves"][1].as_str().unwrap().to_string());
    run(s, "sketch.constraint.fix", json!({"entity": format!("{a}.start")}));
    run(s, "sketch.constraint.along_axis", json!({"line": a, "axis": "x"}));
    run(s, "sketch.dimension", json!({"entities": [a], "value": "run = 100"}));
    (sketch, a, b)
}

#[test]
fn a_parameter_change_stores_the_3d_curves_where_it_puts_them() {
    let mut s = Session::default();
    let (sketch, a, b) = run_and_rise(&mut s);
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "parameters.change", json!({"name": "run", "expression": "150 mm"}));
    // The stored sketch, not only the evaluated one: later edits start from it.
    let stored = |s: &Session, w: &str, i: usize| {
        let sk = s.doc.sketch(sketch).unwrap();
        sk.wires[sk.wire_index(w).unwrap()].fit[i]
    };
    near(stored(&s, &a, 1), Vec3::new(150.0, 0.0, 0.0));
    near(stored(&s, &b, 0), Vec3::new(150.0, 0.0, 0.0));
    // The rise's top, held by nothing, stayed put. Moving the corner works from where the
    // change put it, and fixing it holds it there.
    near(stored(&s, &b, 1), Vec3::new(100.0, 0.0, 60.0));
    run(&mut s, "sketch.edit", json!({"sketch": sketch}));
    run(&mut s, "sketch.move3d", json!({"wire": b, "index": 0, "by": [0, 0, 0]}));
    near(stored(&s, &b, 0), Vec3::new(150.0, 0.0, 0.0));
    run(&mut s, "sketch.constraint.fix", json!({"entity": format!("{b}.start")}));
    near(stored(&s, &b, 0), Vec3::new(150.0, 0.0, 0.0));
}

#[test]
fn a_parameter_change_that_breaks_the_3d_constraints_is_refused() {
    let mut s = Session::default();
    let (sketch, a, _) = run_and_rise(&mut s);
    // The run's end also fixed: changing its length can only conflict.
    run(&mut s, "sketch.constraint.fix", json!({"entity": format!("{a}.end")}));
    run(&mut s, "sketch.finish", json!({}));
    let before = s.doc.to_json();
    let e = s.execute("parameters.change", &json!({"name": "run", "expression": "150 mm"})).unwrap_err().to_string();
    assert!(e.contains("3D constraints would conflict"), "{e}");
    assert_eq!(s.doc.to_json(), before);
    let _ = sketch;
}

#[test]
fn a_redundant_3d_dimension_can_be_placed_driven_and_toggled() {
    let mut s = Session::default();
    let (sketch, a, _) = run_and_rise(&mut s);
    // The run along X is its length already.
    let ends = json!([format!("{a}.start"), format!("{a}.end")]);
    let r = run(&mut s, "sketch.dimension", json!({"entities": ends, "type": "distance_x", "if_redundant": "driven"}));
    assert_eq!((r["driven"].as_bool(), r["redundant"].as_bool()), (Some(true), Some(true)), "{r}");
    let k = r["constraint"].as_str().unwrap().to_string();
    // Making it driving would over-constrain: it stays driven.
    let before = s.doc.to_json();
    let e = s.execute("sketch.toggle_driven", &json!({"constraint": k})).unwrap_err().to_string();
    assert!(e.contains("stays driven"), "{e}");
    assert_eq!(s.doc.to_json(), before);
    // The length dimension made driven frees the run; the X distance can then drive it.
    let length = s.doc.sketch(sketch).unwrap().wire_constraints.iter().find(|c| c.param.is_some()).unwrap().id.clone();
    run(&mut s, "sketch.toggle_driven", json!({"constraint": length, "driven": true}));
    let r = run(&mut s, "sketch.toggle_driven", json!({"constraint": k, "driven": false}));
    assert_eq!(r["driven"], false, "{r}");
    let e = s.execute("sketch.dimension_text", &json!({"dimension": k, "at": [1, 2]})).unwrap_err().to_string();
    assert!(e.contains("#10617"), "{e}");
}

#[test]
fn an_angle_at_a_corner_is_the_corner_s_angle() {
    // A bracket drawn as a run out and a return leg back over it (180° apart at the corner).
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane":"XY"}));
    run(&mut s, "sketch.options", json!({"sketch_3d":true}));
    let r = run(&mut s, "sketch.line3d", json!({"points":[[0,0,0],[100,0,0],[50,0,40]]}));
    let (a, b) = (r["curves"][0].as_str().unwrap().to_string(), r["curves"][1].as_str().unwrap().to_string());
    run(&mut s, "sketch.constraint.fix", json!({"entity": a}));
    let d = run(&mut s, "sketch.dimension", json!({"entities": [a, b], "value": "60 deg"}));
    assert!((d["measured"].as_f64().unwrap() - 60.0).abs() < 1e-6, "{d}");
    // The leg leans back over the run at 60° from the corner, as on a drawing.
    let (corner, tip) = (fit(&s, &b, 0), fit(&s, &b, 1));
    let leg = tip - corner;
    assert!(leg.x < 0.0 && (leg.x / leg.len() + 0.5).abs() < 1e-9, "{leg:?}");
}

#[test]
fn a_driven_3d_dimension_takes_any_value_it_measures_through_edits() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane":"XY"}));
    run(&mut s, "sketch.options", json!({"sketch_3d":true}));
    // A reference distance between two points that are then joined: it measures 0.
    let p = run(&mut s, "sketch.point3d", json!({"point":[0,0,0]}))["point"].as_str().unwrap().to_string();
    let q = run(&mut s, "sketch.point3d", json!({"point":[5,0,0]}))["point"].as_str().unwrap().to_string();
    let d = run(&mut s, "sketch.dimension", json!({"entities": [p, q], "driven": true}));
    assert!(d["created"]["dimensions"].as_array().is_some_and(|a| a.len() == 1), "listed as a dimension: {d}");
    run(&mut s, "sketch.constraint.coincident", json!({"a": p, "b": q}));
    // The sketch still evaluates and takes edits, planar ones too.
    run(&mut s, "sketch.point3d", json!({"point":[1,2,3]}));
    run(&mut s, "sketch.line", json!({"points":[[2,3],[10,4]]}));
    assert!(s.model.results.iter().all(|r| r.error.is_none()));
    // A reference length of 1.5 km.
    let l = run(&mut s, "sketch.line3d", json!({"points":[[0,10,0],[1500000,10,0]]}))["curves"][0].as_str().unwrap().to_string();
    let r = run(&mut s, "sketch.dimension", json!({"entities": [l], "driven": true}));
    assert!((r["measured"].as_f64().unwrap() - 1.5e6).abs() < 1e-3, "{r}");
}

#[test]
fn a_parameter_value_a_3d_dimension_cannot_take_is_refused() {
    let mut s = Session::default();
    let (_, _, _) = run_and_rise(&mut s);
    run(&mut s, "sketch.finish", json!({}));
    for bad in ["0 mm", "-5 mm", "2000000 mm"] {
        let before = s.doc.to_json();
        let e = s.execute("parameters.change", &json!({"name": "run", "expression": bad})).unwrap_err().to_string();
        assert!(e.contains("not changed"), "{bad}: {e}");
        assert_eq!(s.doc.to_json(), before, "{bad}");
    }
}

#[test]
fn a_settled_angle_between_long_oblique_lines_is_placed_driven() {
    // A corner of 53.13010235…° between 300 and 500 mm lines, both fixed: the angle is settled,
    // and its value must not be rounded into a conflict.
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane":"XY"}));
    run(&mut s, "sketch.options", json!({"sketch_3d":true}));
    let r = run(&mut s, "sketch.line3d", json!({"points":[[0,0,0],[300,0,0],[0,400,0]]}));
    let (a, b) = (r["curves"][0].as_str().unwrap().to_string(), r["curves"][1].as_str().unwrap().to_string());
    run(&mut s, "sketch.constraint.fix", json!({"entities": [a, b]}));
    let d = run(&mut s, "sketch.dimension", json!({"entities": [a, b], "if_redundant": "driven"}));
    assert_eq!((d["driven"].as_bool(), d["redundant"].as_bool()), (Some(true), Some(true)), "{d}");
}

#[test]
fn a_new_3d_dimension_s_bad_value_is_refused_as_its_value() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane":"XY"}));
    run(&mut s, "sketch.options", json!({"sketch_3d":true}));
    let l = run(&mut s, "sketch.line3d", json!({"points":[[0,0,0],[10,1,2]]}))["curves"][0].as_str().unwrap().to_string();
    let before = s.doc.to_json();
    let e = s.execute("sketch.dimension", &json!({"entities": [l], "value": "-4 mm"})).unwrap_err().to_string();
    assert!(e.contains("value:") && !e.contains("k1:") && !e.contains("k2:"), "{e}");
    assert_eq!(s.doc.to_json(), before);
}

#[test]
fn a_tangent_line_leaves_a_fixed_arc_along_its_end_and_a_loose_line_is_refused() {
    // Pipes along 3D sketch paths are graded in tests/kernel-cases/sweeps (#10615).
    // A quarter arc of radius 20 about the origin, from (20,0,0) round to (0,20,0), with a line
    // into it along +Y and a line out of it drawn off true; Tangent turns the second one onto -X.
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane":"XY"}));
    run(&mut s, "sketch.options", json!({"sketch_3d":true}));
    let d = 20.0 * std::f64::consts::FRAC_1_SQRT_2;
    let into = run(&mut s, "sketch.line3d", json!({"points":[[20,-30,0],[20,0,0]]}))["curves"][0].as_str().unwrap().to_string();
    let arc = run(&mut s, "sketch.arc3d", json!({"start":[20,0,0],"through":[d,d,0],"end":[0,20,0]}))["curves"][0].as_str().unwrap().to_string();
    let out = run(&mut s, "sketch.line3d", json!({"points":[[0,20,0],[-30,25,3]]}))["curves"][0].as_str().unwrap().to_string();
    run(&mut s, "sketch.constraint.fix", json!({"entities": [into, arc]}));
    run(&mut s, "sketch.constraint.tangent", json!({"a": out, "b": arc}));
    // The way out now runs along -X from the arc's end (its free end swung onto that line).
    near(fit(&s, &out, 0), Vec3::new(0.0, 20.0, 0.0));
    let end = fit(&s, &out, 1);
    assert!((end.y - 20.0).abs() < 1e-7 && end.z.abs() < 1e-7 && end.x < -25.0, "{end:?}");
    // A line not touching the arc has no tangent to take.
    let loose = run(&mut s, "sketch.line3d", json!({"points":[[50,50,0],[60,50,0]]}))["curves"][0].as_str().unwrap().to_string();
    let e = s.execute("sketch.constraint.tangent", &json!({"a": loose, "b": arc})).unwrap_err().to_string();
    assert!(e.contains("meet"), "{e}");
}

#[test]
fn relations_3d_square_up_a_frame_corner_and_hold_it_to_the_origin_planes() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane":"XY"}));
    run(&mut s, "sketch.options", json!({"sketch_3d":true}));
    // Three legs from one corner, drawn roughly, and a second leg along the first.
    let r = run(&mut s, "sketch.line3d", json!({"points":[[0,0,0],[100,4,2]]}));
    let x = r["curves"][0].as_str().unwrap().to_string();
    let y = run(&mut s, "sketch.line3d", json!({"points":[[0,0,0],[3,80,5]]}))["curves"][0].as_str().unwrap().to_string();
    let z = run(&mut s, "sketch.line3d", json!({"points":[[0,0,0],[2,6,70]]}))["curves"][0].as_str().unwrap().to_string();
    let far = run(&mut s, "sketch.line3d", json!({"points":[[0,60,3],[90,63,1]]}))["curves"][0].as_str().unwrap().to_string();
    run(&mut s, "sketch.constraint.fix", json!({"entity": format!("{x}.start")}));
    for leg in [&y, &z] {
        run(&mut s, "sketch.constraint.coincident", json!({"a": format!("{x}.start"), "b": format!("{leg}.start")}));
    }
    // x lies in XY; y square to x and in XY too; z square to both.
    run(&mut s, "sketch.constraint.parallel", json!({"a": x, "b": "xy"}));
    run(&mut s, "sketch.constraint.parallel", json!({"a": y, "b": "xy"}));
    run(&mut s, "sketch.constraint.perpendicular", json!({"a": x, "b": y}));
    run(&mut s, "sketch.constraint.perpendicular", json!({"a": x, "b": z}));
    run(&mut s, "sketch.constraint.perpendicular", json!({"a": y, "b": z}));
    // The far rail: parallel to x, as long as x, its start on XY.
    run(&mut s, "sketch.constraint.parallel", json!({"a": far, "b": x}));
    run(&mut s, "sketch.constraint.equal", json!({"a": far, "b": x}));
    run(&mut s, "sketch.constraint.on_plane", json!({"point": format!("{far}.start"), "plane": "xy"}));
    let (dx, dy, dz) = (fit(&s, &x, 1), fit(&s, &y, 1), fit(&s, &z, 1));
    assert!(dx.z.abs() < 1e-7 && dy.z.abs() < 1e-7, "{dx:?} {dy:?}");
    assert!(dx.dot(dy).abs() < 1e-6 && dx.dot(dz).abs() < 1e-6 && dy.dot(dz).abs() < 1e-6, "{dx:?} {dy:?} {dz:?}");
    let (f0, f1) = (fit(&s, &far, 0), fit(&s, &far, 1));
    assert!(f0.z.abs() < 1e-7);
    assert!(((f1 - f0).len() - dx.len()).abs() < 1e-6 && (f1 - f0).cross(dx).len() < 1e-6 * dx.len() * (f1 - f0).len());
    // A dimension to a plane: the upright's top 50 above XY. The far rail's end is already on
    // XY (parallel to x, which lies in XY, from a start on it), so a height there is refused.
    let h = run(&mut s, "sketch.dimension", json!({"entities": [format!("{z}.end"), "xy"], "value": 50}));
    assert_eq!(h["type"], "plane_distance", "{h}");
    assert!((fit(&s, &z, 1).z - 50.0).abs() < 1e-6);
    assert!(s.execute("sketch.dimension", &json!({"entities": [format!("{far}.end"), "xy"], "value": 25})).is_err());
    let e = s.execute("sketch.constraint.perpendicular", &json!({"a": x, "b": "xy"})).unwrap_err().to_string();
    assert!(e.contains("only Parallel takes an origin plane") && e.contains("along_axis"), "{e}");
}

#[test]
fn an_upright_set_to_80_degrees_to_xy_and_on_plane_take_plane_names_either_case() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane":"XY"}));
    run(&mut s, "sketch.options", json!({"sketch_3d":true}));
    // Drawn lying in XY: the angle to it starts at 0.
    let l = run(&mut s, "sketch.line3d", json!({"points":[[0,0,0],[10,0,0]]}))["curves"][0].as_str().unwrap().to_string();
    run(&mut s, "sketch.constraint.fix", json!({"entity": format!("{l}.start")}));
    let d = run(&mut s, "sketch.dimension", json!({"entities": [l, "xy"], "value": "80 deg"}));
    assert!((d["measured"].as_f64().unwrap() - 80.0).abs() < 1e-6, "{d}");
    let top = fit(&s, &l, 1);
    assert!((top.z / top.len() - 80f64.to_radians().sin()).abs() < 1e-9, "{top:?}");
    let q = run(&mut s, "sketch.point3d", json!({"point":[3,4,5]}))["point"].as_str().unwrap().to_string();
    run(&mut s, "sketch.constraint.on_plane", json!({"point": q, "plane": "XY"}));
    assert!(fit(&s, &q, 0).z.abs() < 1e-9);
}

#[test]
fn a_paste_shifted_along_a_plane_keeps_the_distance_from_it_with_the_geometry() {
    // A point 20 above XY by a dimension (its parameter `h`), a point on XZ; pasted 10 up and
    // 5 along Y, each lands where the paste put it and stays there when solved.
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane":"XY", "name": "Path"}));
    run(&mut s, "sketch.options", json!({"sketch_3d":true}));
    let q = run(&mut s, "sketch.point3d", json!({"point":[3,4,20]}))["point"].as_str().unwrap().to_string();
    let o = run(&mut s, "sketch.point3d", json!({"point":[7,0,6]}))["point"].as_str().unwrap().to_string();
    run(&mut s, "sketch.dimension", json!({"entities": [q, "xy"], "value": "h = 20"}));
    run(&mut s, "sketch.constraint.on_plane", json!({"point": o, "plane": "xz"}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "timeline.copy", json!({"features": ["Path"]}));
    // Into another design.
    let mut t = Session { foreign_copy: s.foreign_copy.clone(), ..Default::default() };
    let pasted = run(&mut t, "timeline.paste", json!({"from_design": true, "translate": [0, 5, 10]}))["pasted"][0]["id"].as_u64().unwrap();
    near(solved_fit(&t, pasted, &q, 0), Vec3::new(3.0, 9.0, 30.0));
    near(solved_fit(&t, pasted, &o, 0), Vec3::new(7.0, 5.0, 16.0));
    // The pasted distance still follows `h`: 25 above XY, plus the 10 the paste added.
    let h = t.doc.params.iter().find(|p| !p.model).map(|p| p.name.clone()).unwrap();
    run(&mut t, "parameters.change", json!({"name": h, "expression": "25 mm"}));
    near(solved_fit(&t, pasted, &q, 0), Vec3::new(3.0, 9.0, 35.0));
}

#[test]
fn a_driven_angle_to_a_plane_made_driving_keeps_the_side_the_line_is_on() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane":"XY"}));
    run(&mut s, "sketch.options", json!({"sketch_3d":true}));
    let l = run(&mut s, "sketch.line3d", json!({"points":[[0,0,0],[10,0,5]]}))["curves"][0].as_str().unwrap().to_string();
    run(&mut s, "sketch.constraint.fix", json!({"entity": format!("{l}.start")}));
    let k = run(&mut s, "sketch.dimension", json!({"entities": [l, "xy"], "driven": true}))["constraint"].as_str().unwrap().to_string();
    // The line is moved to run down at 30°; its driven angle reads 30°.
    run(&mut s, "sketch.move3d", json!({"wire": l, "index": 1, "to": [8.660254037844386, 0, -5]}));
    run(&mut s, "sketch.toggle_driven", json!({"constraint": k, "driven": false}));
    let end = fit(&s, &l, 1);
    assert!(end.z < 0.0 && (end.z / end.len() + 0.5).abs() < 1e-6, "still down at 30°: {end:?}");
}
