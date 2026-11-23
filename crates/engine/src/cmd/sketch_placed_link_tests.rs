//! Links to an exact placement of another component (#47): a projection from `A:2` follows
//! A:2 and nothing else, in the frame of the placement of the sketch's component it was made in.

use crate::Session;
use serde_json::{Value, json};
use solidcore_record::{Vec2, Vec3};
use solidcraft_geom::prelude::*;
use solidcraft_sketch::{CurveKind, LinkPlacement, LinkSource};

fn run(s: &mut Session, cmd: &str, params: Value) -> Value {
    s.execute(cmd, &params).unwrap_or_else(|e| panic!("{cmd}: {e}"))
}

fn near(a: Vec2, b: Vec2) -> bool {
    a.dist(b) < 1e-6
}

/// Part A (a 10 mm cube "Plate", not grounded) placed twice: A:1 at the origin, A:2 at x = 50. Part B empty,
/// placed once at the origin, with a sketch on its XY plane. Returns (session, A, a1, a2, B, b1,
/// sketch).
fn assembly() -> (Session, u64, u64, u64, u64, u64, u64) {
    let mut s = Session::default();
    let a = run(&mut s, "component.create", json!({"name":"A","grounded":false}));
    let (comp_a, a1) = (a["component"].as_u64().unwrap(), a["occurrence"].as_u64().unwrap());
    run(&mut s, "solid.box", json!({"length":10,"width":10,"height":10,"body_name":"Plate"}));
    let a2 = run(&mut s, "occurrence.copy", json!({"occurrence":a1}))["occurrence"].as_u64().unwrap();
    run(&mut s, "occurrence.move", json!({"occurrence":a2,"translate":[50,0,0]}));
    run(&mut s, "component.activate", json!({"component":0}));
    let b = run(&mut s, "component.create", json!({"name":"B"}));
    let (comp_b, b1) = (b["component"].as_u64().unwrap(), b["occurrence"].as_u64().unwrap());
    let sketch = run(&mut s, "sketch.create", json!({"plane":"XY","name":"Receiver"}))["sketch"].as_u64().unwrap();
    (s, comp_a, a1, a2, comp_b, b1, sketch)
}

/// The world name of A's body at placement `path`.
fn world_name(s: &Session, path: &[u64]) -> String {
    let st = s.model.state();
    s.doc.body_frames_with_source(&st).into_iter().find(|(_, own, _, at, _)| own == "Plate" && at == path).unwrap().0
}

/// The evaluated line of link `link` in `sketch` (as the model shows it).
fn line(s: &Session, sketch: u64, link: &str) -> (Vec2, Vec2) {
    let st = s.model.state();
    let sk = &st.sketch(sketch).unwrap().sketch;
    let c = sk.link_curves(link)[0];
    let CurveKind::Line { a, b } = sk.curves[c].kind else { panic!("not a line") };
    (sk.point(a).unwrap(), sk.point(b).unwrap())
}

fn same_line(got: (Vec2, Vec2), a: Vec2, b: Vec2) -> bool {
    near(got.0, a) && near(got.1, b) || near(got.0, b) && near(got.1, a)
}

/// Project A:2's front top edge (y = 0, z = 10) into B's sketch, picked by its world name.
fn project_second(s: &mut Session, a2: u64, sketch: u64) -> String {
    let name = world_name(s, &[a2]);
    assert_ne!(name, "Plate", "the second placement has an instance name");
    let r = run(s, "sketch.project", json!({"refs":[{"edge":[55,0,10],"body":name}]}));
    let link = r["links"][0]["link"].as_str().unwrap().to_string();
    let stored = s.doc.sketch(sketch).unwrap().link(&link).unwrap().clone();
    assert!(
        matches!(&stored.source, LinkSource::Edge { body, at } if body == "Plate" && at.dist(Vec3::new(5.0, 0.0, 10.0)) < 1e-6),
        "{:?}",
        stored.source
    );
    link
}

#[test]
fn second_placement_is_followed_and_first_is_not() {
    let (mut s, _, a1, a2, _, b1, sketch) = assembly();
    let link = project_second(&mut s, a2, sketch);
    let stored = s.doc.sketch(sketch).unwrap().link(&link).unwrap().clone();
    let placement = stored.placement.clone().unwrap();
    assert_eq!((placement.source, placement.author), (vec![a2], vec![b1]));
    assert!(placement.seen.is_some(), "the transform its geometry was read with");
    assert!(stored.edge_name.is_some(), "the picked edge keeps its persistent name");
    assert!(same_line(line(&s, sketch, &link), Vec2::new(50.0, 0.0), Vec2::new(60.0, 0.0)));

    // Moving the other copy changes nothing.
    run(&mut s, "occurrence.move", json!({"occurrence":a1,"translate":[0,30,0]}));
    assert!(same_line(line(&s, sketch, &link), Vec2::new(50.0, 0.0), Vec2::new(60.0, 0.0)));
    assert!(!s.model.state().sketch(sketch).unwrap().sketch.link(&link).unwrap().lost);

    // Moving A:2 moves the projection.
    run(&mut s, "occurrence.move", json!({"occurrence":a2,"translate":[0,20,0]}));
    assert!(same_line(line(&s, sketch, &link), Vec2::new(50.0, 20.0), Vec2::new(60.0, 20.0)));

    // A quarter turn of A:2 about its own corner: (x, y) -> (50 - y', ...) analytically.
    // A:2's frame is translate(50, 20, 0); turning it 90° about world Z through (50, 20, 0) maps
    // its local (x, 0, 10) to world (50, 20 + x, 10).
    run(&mut s, "occurrence.move", json!({"occurrence":a2,"space":"world","axis":[0,0,1],"angle":"90 deg","origin":[50,20,0]}));
    assert!(same_line(line(&s, sketch, &link), Vec2::new(50.0, 20.0), Vec2::new(50.0, 30.0)), "{:?}", line(&s, sketch, &link));
}

#[test]
fn receiver_moves_and_moving_both_keeps_relative_geometry() {
    let (mut s, _, _, a2, _, b1, sketch) = assembly();
    let link = project_second(&mut s, a2, sketch);
    // B moved: the line is where A:2 is, seen from B.
    run(&mut s, "occurrence.move", json!({"occurrence":b1,"translate":[5,-5,0]}));
    assert!(same_line(line(&s, sketch, &link), Vec2::new(45.0, 5.0), Vec2::new(55.0, 5.0)));
    // A:2 and B moved together: nothing changes.
    let before = line(&s, sketch, &link);
    for occ in [a2, b1] {
        run(&mut s, "occurrence.move", json!({"occurrence":occ,"space":"world","translate":[7,3,0],"axis":[0,0,1],"angle":"30 deg"}));
    }
    let after = line(&s, sketch, &link);
    assert!(near(before.0, after.0) && near(before.1, after.1), "{before:?} {after:?}");
}

#[test]
fn dependent_profile_and_extrude_follow_the_placement() {
    let (mut s, _, a1, a2, _, _, sketch) = assembly();
    let link = project_second(&mut s, a2, sketch);
    let points: Vec<String> =
        s.doc.sketch(sketch).unwrap().link_points(&link).iter().map(|p| s.doc.sketch(sketch).unwrap().points[*p].id.clone()).collect();
    // A rectangle standing on the projected line, sharing its two ends.
    let (p, q) = {
        let sk = s.doc.sketch(sketch).unwrap();
        let at = |id: &str| sk.points[sk.point_index(id).unwrap()].pos;
        if at(&points[0]).x < at(&points[1]).x { (points[0].clone(), points[1].clone()) } else { (points[1].clone(), points[0].clone()) }
    };
    let sides = run(&mut s, "sketch.line", json!({"points":[q, [60,8], [50,8], p]}));
    let side = |i: usize| sides["curves"][i].as_str().unwrap().to_string();
    // Held square to the projected line, 8 mm deep: fully determined by it.
    let projected = s.doc.sketch(sketch).unwrap().curves[s.doc.sketch(sketch).unwrap().link_curves(&link)[0]].id.clone();
    run(&mut s, "sketch.constraint.perpendicular", json!({"a":side(0),"b":projected}));
    run(&mut s, "sketch.constraint.parallel", json!({"a":side(0),"b":side(2)}));
    run(&mut s, "sketch.dimension", json!({"entities":[side(0)],"type":"length","value":8}));
    run(&mut s, "sketch.dimension", json!({"entities":[side(2)],"type":"length","value":8}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"sketch":sketch,"distance":4,"operation":"new","body_name":"Bracket"}));
    let bounds = |s: &Session| s.world_state().unwrap().body("Bracket").unwrap().body.exact_bounds().unwrap();
    let b = bounds(&s);
    assert!((b.min.y - 0.0).abs() < 1e-6 && (b.min.x - 50.0).abs() < 1e-6 && (b.max.x - 60.0).abs() < 1e-6, "{b:?}");
    run(&mut s, "occurrence.move", json!({"occurrence":a1,"translate":[0,-40,0]}));
    assert!((bounds(&s).min.y - 0.0).abs() < 1e-6, "A:1 drives nothing");
    run(&mut s, "occurrence.move", json!({"occurrence":a2,"translate":[0,12,0]}));
    let b = bounds(&s);
    assert!((b.min.y - 12.0).abs() < 1e-6 && (b.max.y - 20.0).abs() < 1e-6, "the bracket follows A:2: {b:?}");
    // Undo puts it back; redo moves it again.
    run(&mut s, "edit.undo", json!({}));
    assert!((bounds(&s).min.y - 0.0).abs() < 1e-6);
    run(&mut s, "edit.redo", json!({}));
    assert!((bounds(&s).min.y - 12.0).abs() < 1e-6);
    // Saved and reopened: the same.
    let mut reopened = Session { doc: solidcraft_doc::format::read(&s.doc.to_json()).unwrap().into(), ..Default::default() };
    reopened.refresh();
    let b = reopened.world_state().unwrap().body("Bracket").unwrap().body.exact_bounds().unwrap();
    assert!((b.min.y - 12.0).abs() < 1e-6, "{b:?}");
}

#[test]
fn nested_parent_moves_are_followed() {
    let mut s = Session::default();
    let outer = run(&mut s, "component.create", json!({"name":"Frame","grounded":false}));
    let (frame, f1) = (outer["component"].as_u64().unwrap(), outer["occurrence"].as_u64().unwrap());
    let inner = run(&mut s, "component.create", json!({"name":"A","parent":frame}));
    let a1 = inner["occurrence"].as_u64().unwrap();
    run(&mut s, "solid.box", json!({"length":10,"width":10,"height":10,"body_name":"Plate"}));
    let a2 = run(&mut s, "occurrence.copy", json!({"occurrence":a1}))["occurrence"].as_u64().unwrap();
    run(&mut s, "occurrence.move", json!({"occurrence":a2,"translate":[20,0,0]}));
    run(&mut s, "component.activate", json!({"component":0}));
    let f2 = run(&mut s, "occurrence.copy", json!({"occurrence":f1}))["occurrence"].as_u64().unwrap();
    run(&mut s, "occurrence.move", json!({"occurrence":f2,"translate":[0,100,0]}));
    run(&mut s, "component.create", json!({"name":"B"}));
    let sketch = run(&mut s, "sketch.create", json!({"plane":"XY"}))["sketch"].as_u64().unwrap();
    // A:2 inside the second Frame: picked by its explicit path.
    let r = run(&mut s, "sketch.project", json!({"refs":[{"edge":[25,100,10],"body":"Plate","path":[f2,a2]}]}));
    let link = r["links"][0]["link"].as_str().unwrap().to_string();
    assert_eq!(s.doc.sketch(sketch).unwrap().link(&link).unwrap().placement.as_ref().unwrap().source, vec![f2, a2]);
    assert!(same_line(line(&s, sketch, &link), Vec2::new(20.0, 100.0), Vec2::new(30.0, 100.0)));
    // The first Frame moving changes nothing; the second moving carries A:2 with it.
    run(&mut s, "occurrence.move", json!({"occurrence":f1,"translate":[0,0,0],"axis":[0,0,1],"angle":"45 deg"}));
    assert!(same_line(line(&s, sketch, &link), Vec2::new(20.0, 100.0), Vec2::new(30.0, 100.0)));
    run(&mut s, "occurrence.move", json!({"occurrence":f2,"translate":[0,5,0]}));
    assert!(same_line(line(&s, sketch, &link), Vec2::new(20.0, 105.0), Vec2::new(30.0, 105.0)));
    // Renaming the occurrences does not change what the link reads.
    s.doc_mut().occurrences.iter_mut().find(|o| o.id == a2).unwrap().name = "Renamed".into();
    s.refresh();
    assert!(same_line(line(&s, sketch, &link), Vec2::new(20.0, 105.0), Vec2::new(30.0, 105.0)));
}

#[test]
fn second_receiver_placement_is_kept_through_reopen() {
    let (mut s, _, _, a2, comp_b, b1, sketch) = assembly();
    run(&mut s, "sketch.finish", json!({}));
    let b2 = run(&mut s, "occurrence.copy", json!({"occurrence":b1}))["occurrence"].as_u64().unwrap();
    run(&mut s, "occurrence.move", json!({"occurrence":b2,"translate":[0,-30,0]}));
    // Authored in B:2: the line is A:2 seen from B:2.
    run(&mut s, "component.activate", json!({"component":comp_b,"occurrence":b2,"path":[b2]}));
    run(&mut s, "sketch.edit", json!({"sketch":sketch}));
    let link = project_second(&mut s, a2, sketch);
    assert_eq!(s.doc.sketch(sketch).unwrap().link(&link).unwrap().placement.as_ref().unwrap().author, vec![b2]);
    run(&mut s, "sketch.finish", json!({}));
    assert!(same_line(line(&s, sketch, &link), Vec2::new(50.0, 30.0), Vec2::new(60.0, 30.0)));
    // Reopened with B:1 the active (first) placement: still B:2's view.
    let mut reopened = Session { doc: solidcraft_doc::format::read(&s.doc.to_json()).unwrap().into(), ..Default::default() };
    reopened.refresh();
    assert!(same_line(line(&reopened, sketch, &link), Vec2::new(50.0, 30.0), Vec2::new(60.0, 30.0)));
}

#[test]
fn lost_placement_keeps_last_geometry_and_never_switches_copies() {
    let (mut s, _, a1, a2, _, _, sketch) = assembly();
    let link = project_second(&mut s, a2, sketch);
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "occurrence.move", json!({"occurrence":a2,"translate":[0,20,0]}));
    run(&mut s, "sketch.update_references", json!({}));
    let kept = line(&s, sketch, &link);
    assert!(same_line(kept, Vec2::new(50.0, 20.0), Vec2::new(60.0, 20.0)));
    // A:1 sits exactly where A:2 was first picked; A:2 is deleted.
    run(&mut s, "occurrence.move", json!({"occurrence":a1,"translate":[50,20,0]}));
    run(&mut s, "selection.delete", json!({"items":[{"type":"occurrence","path":[a2]}]}));
    let st = s.model.state();
    let sk = &st.sketch(sketch).unwrap().sketch;
    assert!(sk.link(&link).unwrap().lost, "lost, not re-found on A:1");
    let got = line(&s, sketch, &link);
    assert!(near(got.0, kept.0) && near(got.1, kept.1), "last good geometry: {got:?}");
    let refs = run(&mut s, "sketch.references", json!({"sketch":sketch}));
    assert_eq!(refs["references"][0]["status"], "lost", "{refs}");
}

/// A projection in part B of an edge of a root body (its placement path is empty: the root)
/// stays that edge when the body is joined into another root body before the sketch: the
/// joined body is searched for the named edge, as for a body of any other component. The tool
/// stands taller than the base, so the join merges none of the faces along the edge.
#[test]
fn a_root_bodys_edge_stays_found_after_the_body_is_joined() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length":20,"width":20,"height":10,"body_name":"Base"}));
    run(&mut s, "solid.box", json!({"length":10,"width":10,"height":20,"corner":[20,0,0],"body_name":"Tool"}));
    run(&mut s, "component.create", json!({"name":"B"}));
    let sketch = run(&mut s, "sketch.create", json!({"plane":"XY","name":"Receiver"}))["sketch"].as_u64().unwrap();
    let r = run(&mut s, "sketch.project", json!({"refs":[{"edge":[25,10,20],"body":"Tool"}]}));
    let link = r["links"][0]["link"].as_str().unwrap().to_string();
    let stored = s.doc.sketch(sketch).unwrap().link(&link).unwrap().clone();
    assert_eq!(stored.placement.as_ref().map(|p| p.source.clone()), Some(Vec::new()), "premise: a root source: {stored:?}");
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "component.activate", json!({"component":0}));
    run(&mut s, "solid.combine", json!({"target":"Base","tools":["Tool"],"operation":"join"}));
    let join = s.doc.features.last().unwrap().id;
    let at = s.doc.features.iter().position(|f| f.id == sketch).unwrap();
    run(&mut s, "timeline.reorder", json!({"feature":join,"position":at}));
    let st = s.model.state();
    assert!(st.body("Tool").is_none(), "premise: Tool is joined into Base before the sketch");
    let warning = s.model.result(sketch).and_then(|r| r.warning.clone());
    assert!(!st.sketch(sketch).unwrap().sketch.link(&link).unwrap().lost, "found on the joined body: {warning:?}");
    assert!(same_line(line(&s, sketch, &link), Vec2::new(20.0, 10.0), Vec2::new(30.0, 10.0)));
}

#[test]
fn same_component_other_placement_and_ambiguous_sketch_sources_are_refused() {
    let (mut s, comp_a, a1, a2, _, _, _) = assembly();
    run(&mut s, "sketch.finish", json!({}));
    // A sketch in A may not read A:2 while authored in A:1.
    run(&mut s, "component.activate", json!({"component":comp_a,"occurrence":a1,"path":[a1]}));
    let source = run(&mut s, "sketch.create", json!({"plane":"XY","name":"In A"}))["sketch"].as_u64().unwrap();
    run(&mut s, "sketch.line", json!({"points":[[0,0],[5,0]],"ids":["al"]}));
    let name = world_name(&s, &[a2]);
    let before = s.doc.to_json();
    let e = s.execute("sketch.project", &json!({"strict_occurrence_references":true,"refs":[{"edge":[55,0,10],"body":name}]})).unwrap_err();
    assert!(e.to_string().contains("another placement of its own component"), "{e}");
    assert_eq!(s.doc.to_json(), before);
    run(&mut s, "sketch.finish", json!({}));
    // A sketch of A, placed twice, read from B without saying which copy: refused.
    run(&mut s, "component.activate", json!({"component":0}));
    run(&mut s, "component.create", json!({"name":"C"}));
    run(&mut s, "sketch.create", json!({"plane":"XY"}));
    let e = s.execute("sketch.project", &json!({"strict_occurrence_references":true,"refs":[{"sketch":source,"curve":"al"}]})).unwrap_err();
    assert!(e.to_string().contains("placed more than once"), "{e}");
    // With the path it works and follows that copy.
    let r = run(&mut s, "sketch.project", json!({"refs":[{"sketch":source,"curve":"al","path":[a2]}]}));
    assert!(r["links"][0]["link"].is_string());
}

#[test]
fn hostile_placement_paths_never_panic() {
    let (mut s, _, _, a2, _, _, sketch) = assembly();
    let before = s.doc.to_json();
    for path in [json!([0]), json!(vec![a2; 65]), json!([a2, a2]), json!("x"), json!([-1]), json!([u64::MAX])] {
        assert!(s.execute("sketch.project", &json!({"refs":[{"edge":[55,0,10],"body":"Plate","path":path}]})).is_err(), "{path}");
        assert_eq!(s.doc.to_json(), before);
    }
    // A design file with a broken placement loads and reports the link lost.
    let link = project_second(&mut s, a2, sketch);
    let mut doc = (*s.doc).clone();
    for (source, author) in [(vec![u64::MAX], vec![]), (vec![a2; 70], vec![1]), (vec![a2], vec![0])] {
        doc.sketch_mut(sketch).unwrap().links.iter_mut().find(|l| l.id == link).unwrap().placement =
            Some(LinkPlacement { source, author, seen: None });
        let mut t = Session { doc: solidcraft_doc::format::read(&doc.to_json()).unwrap().into(), ..Default::default() };
        t.refresh();
        assert!(t.model.state().sketch(sketch).unwrap().sketch.link(&link).unwrap().lost);
    }
}

/// A 10 mm square box whose length is `w`, placed at x = 0 and x = 50, with a receiver
/// in another component. The far vertical edge is (50 + w, 10, z), z in [0, 10].
fn parametric_box_copies() -> (Session, u64, u64, u64) {
    let mut s = Session::default();
    run(&mut s, "parameters.change", json!({"name":"w","expression":"10 mm"}));
    let a1 = run(&mut s, "component.create", json!({"name":"A","grounded":false}))["occurrence"].as_u64().unwrap();
    run(&mut s, "solid.box", json!({"length":"w","width":10,"height":10,"body_name":"Plate"}));
    let a2 = run(&mut s, "occurrence.copy", json!({"occurrence":a1}))["occurrence"].as_u64().unwrap();
    run(&mut s, "occurrence.move", json!({"occurrence":a2,"translate":[50,0,0]}));
    run(&mut s, "component.activate", json!({"component":0}));
    run(&mut s, "component.create", json!({"name":"Receiver"}));
    let sketch = run(&mut s, "sketch.create", json!({"plane":"XY","project_edges":false}))["sketch"].as_u64().unwrap();
    (s, a1, a2, sketch)
}

#[test]
fn an_included_edge_of_the_picked_copy_follows_moves_edits_and_reopens_lost() {
    let (mut s, a1, a2, sketch) = parametric_box_copies();
    let reply = run(&mut s, "sketch.include_3d", json!({"refs":[{"body":"Plate","edge":[60,10,5],"path":[a2]}]}));
    let link = reply["links"][0]["link"].as_str().unwrap().to_string();
    let sk = s.doc.sketch(sketch).unwrap();
    let wire_id = sk.wires[sk.link_wires(&link)[0]].id.clone();
    assert!(sk.link(&link).unwrap().edge_name.is_some());
    let check = |s: &Session, x: f64, lost: bool| {
        let st = s.model.state();
        let sk = &st.sketch(sketch).unwrap().sketch;
        let l = sk.link(&link).unwrap();
        assert_eq!(l.lost, lost);
        assert_eq!(l.placement.as_ref().unwrap().source, vec![a2]);
        let wires = sk.link_wires(&link);
        assert_eq!(wires.len(), 1);
        let wire = &sk.wires[wires[0]];
        assert_eq!(wire.id, wire_id);
        assert_eq!(wire.space, solidcraft_sketch::WireSpace::Component);
        assert!(wire.pts.len() >= 2);
        assert!(wire.pts.iter().all(|p| (p.x - x).abs() < 1e-7 && (p.y - 10.).abs() < 1e-7 && p.z >= -1e-7 && p.z <= 10. + 1e-7));
        let a = Vec3::new(x, 10., 0.);
        let b = Vec3::new(x, 10., 10.);
        let ends = (*wire.pts.first().unwrap(), *wire.pts.last().unwrap());
        assert!(ends.0.dist(a) < 1e-7 && ends.1.dist(b) < 1e-7 || ends.0.dist(b) < 1e-7 && ends.1.dist(a) < 1e-7);
        // Both the displayed polyline and the exact line must be carried into the picked frame.
        assert_eq!(wire.exact.len(), 1);
        let solidcore_record::brep::Curve::Line { start, end } = &wire.exact[0].curve else { panic!("included box edge is an exact line") };
        let (p, q) = (Vec3::new(start[0], start[1], start[2]), Vec3::new(end[0], end[1], end[2]));
        assert!(p.dist(a) < 1e-7 && q.dist(b) < 1e-7 || p.dist(b) < 1e-7 && q.dist(a) < 1e-7);
    };
    check(&s, 60., false);
    run(&mut s, "occurrence.move", json!({"occurrence":a1,"translate":[0,30,0]}));
    check(&s, 60., false);
    run(&mut s, "occurrence.move", json!({"occurrence":a2,"translate":[20,0,0]}));
    check(&s, 80., false);
    run(&mut s, "parameters.change", json!({"name":"w","expression":"20 mm"}));
    check(&s, 90., false);
    // A real edit refreshes stored links, and must retain the exact wire and its identity.
    run(&mut s, "sketch.edit", json!({"sketch":sketch}));
    run(&mut s, "sketch.line", json!({"points":[[0,0],[1,0]]}));
    check(&s, 90., false);
    let stored = s.doc.sketch(sketch).unwrap();
    let last_good = stored.wires[stored.link_wires(&link)[0]].clone();
    let reopened = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
    check(&reopened, 90., false);
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "selection.delete", json!({"items":[{"type":"occurrence","path":[a2]}]}));
    check(&s, 90., true);
    let state = s.model.state();
    let sk = &state.sketch(sketch).unwrap().sketch;
    assert_eq!(sk.wires[sk.link_wires(&link)[0]], last_good, "lost copy retains the last exact wire, not A:1's edge");
    assert_eq!(run(&mut s, "sketch.references", json!({"sketch":sketch}))["references"][0]["status"], "lost");
    let reopened = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
    check(&reopened, 90., true);
}

#[test]
fn the_picked_copys_silhouette_follows_moves_edits_and_reopens_lost() {
    let (mut s, a1, a2, sketch) = parametric_box_copies();
    let reply = run(&mut s, "sketch.project", json!({"refs":[{"body":"Plate","path":[a2]}]}));
    let link = reply["links"][0]["link"].as_str().unwrap().to_string();
    let sk = s.doc.sketch(sketch).unwrap();
    let ids: Vec<_> = sk.link_curves(&link).iter().map(|c| sk.curves[*c].id.clone()).collect();
    let check = |s: &Session, x: f64, width: f64, lost: bool| {
        let st = s.model.state();
        let sk = &st.sketch(sketch).unwrap().sketch;
        let l = sk.link(&link).unwrap();
        assert_eq!(l.lost, lost);
        assert_eq!(l.placement.as_ref().unwrap().source, vec![a2]);
        assert!(matches!(l.source, LinkSource::Body { .. }));
        let curves = sk.link_curves(&link);
        assert_eq!(curves.len(), 4, "the box's top outline has four sides");
        assert_eq!(curves.iter().map(|c| sk.curves[*c].id.clone()).collect::<Vec<_>>(), ids);
        let corners = [Vec2::new(x, 0.), Vec2::new(x + width, 0.), Vec2::new(x + width, 10.), Vec2::new(x, 10.)];
        let mut ends = Vec::new();
        for c in curves {
            let CurveKind::Line { a, b } = sk.curves[c].kind else { panic!("box silhouette side is a line") };
            let (a, b) = (sk.point(a).unwrap(), sk.point(b).unwrap());
            assert!(corners.iter().any(|p| near(*p, a)) && corners.iter().any(|p| near(*p, b)));
            assert!((a.x - b.x).abs() < 1e-7 || (a.y - b.y).abs() < 1e-7);
            assert!(a.dist(b) > 1e-7);
            ends.extend([a, b]);
        }
        assert!(corners.iter().all(|p| ends.iter().filter(|q| near(*p, **q)).count() == 2));
    };
    check(&s, 50., 10., false);
    run(&mut s, "occurrence.move", json!({"occurrence":a1,"translate":[0,30,0]}));
    check(&s, 50., 10., false);
    run(&mut s, "occurrence.move", json!({"occurrence":a2,"translate":[20,0,0]}));
    check(&s, 70., 10., false);
    run(&mut s, "parameters.change", json!({"name":"w","expression":"20 mm"}));
    check(&s, 70., 20., false);
    run(&mut s, "sketch.edit", json!({"sketch":sketch}));
    run(&mut s, "sketch.line", json!({"points":[[0,0],[1,0]]}));
    check(&s, 70., 20., false);
    let geometry = |s: &Session| {
        let st = s.model.state();
        let sk = &st.sketch(sketch).unwrap().sketch;
        sk.link_curves(&link).iter().map(|c| (sk.curves[*c].id.clone(), sk.curve_link_geom(*c).unwrap())).collect::<Vec<_>>()
    };
    let last_good = geometry(&s);
    let reopened = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
    check(&reopened, 70., 20., false);
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "selection.delete", json!({"items":[{"type":"occurrence","path":[a2]}]}));
    check(&s, 70., 20., true);
    assert_eq!(geometry(&s), last_good, "lost copy retains its silhouette, not A:1's outline");
    assert_eq!(run(&mut s, "sketch.references", json!({"sketch":sketch}))["references"][0]["status"], "lost");
    let reopened = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
    check(&reopened, 70., 20., true);
    assert_eq!(geometry(&reopened), last_good);
}

/// The compatibility policy changes only the newly introduced refusals. Omitted and false
/// retain an ordinary sketch link; true requires the selected occurrence's explicit path.
#[test]
fn ambiguous_sketch_references_keep_main_by_default_and_strict_paths_survive_reopen() {
    for command in ["sketch.project", "sketch.intersect", "sketch.include_3d"] {
        for policy in [None, Some(false), Some(true)] {
            let (mut s, a, a1, a2, _, _, _) = assembly();
            run(&mut s, "sketch.finish", json!({}));
            run(&mut s, "component.activate", json!({"component":a,"occurrence":a1,"path":[a1]}));
            // Intersect crosses XY at (2,0,0); the others project a line in XY.
            let plane = if command == "sketch.intersect" { "XZ" } else { "XY" };
            let source = run(&mut s, "sketch.create", json!({"plane":plane}))["sketch"].as_u64().unwrap();
            run(&mut s, "sketch.line", json!({"points":[[2,-5],[2,5]],"ids":["source"]}));
            run(&mut s, "sketch.finish", json!({}));
            run(&mut s, "component.activate", json!({"component":0}));
            run(&mut s, "component.create", json!({"name":"C"}));
            let receiver = run(&mut s, "sketch.create", json!({"plane":"XY"}))["sketch"].as_u64().unwrap();
            let mut params = json!({"refs":[{"sketch":source,"curve":"source"}]});
            if let Some(policy) = policy {
                params["strict_occurrence_references"] = json!(policy);
            }
            if policy == Some(true) {
                let before = s.doc.to_json();
                let e = s.execute(command, &params).unwrap_err();
                assert!(e.to_string().contains("placed more than once"), "{e}");
                assert_eq!(s.doc.to_json(), before);
                params["refs"][0]["path"] = json!([a2]);
            }
            let r = run(&mut s, command, params);
            let link = r["links"][0]["link"].as_str().unwrap();
            let check = |s: &Session| {
                let st = s.model.state();
                let sk = &st.sketch(receiver).unwrap().sketch;
                let l = sk.link(link).unwrap();
                assert!(!l.lost, "{command} {policy:?}");
                assert!(matches!(l.source, LinkSource::SketchCurve { sketch, .. } if sketch == source));
                assert_eq!(l.placement.as_ref().map(|p| p.source.clone()), (policy == Some(true)).then_some(vec![a2]));
                assert!(!sk.link_curves(link).is_empty() || !sk.link_points(link).is_empty() || !sk.link_wires(link).is_empty());
            };
            check(&s);
            run(&mut s, "sketch.finish", json!({}));
            let mut reopened = Session { doc: solidcraft_doc::format::read(&s.doc.to_json()).unwrap().into(), ..Default::default() };
            reopened.refresh();
            check(&reopened);
        }
    }
}

#[test]
fn other_receiver_placements_keep_main_by_default_and_strict_refusal_is_atomic() {
    for command in ["sketch.project", "sketch.intersect", "sketch.include_3d"] {
        for policy in [None, Some(false), Some(true)] {
            let (mut s, a, a1, a2, _, _, _) = assembly();
            run(&mut s, "sketch.finish", json!({}));
            run(&mut s, "component.activate", json!({"component":a,"occurrence":a1,"path":[a1]}));
            let sketch = run(&mut s, "sketch.create", json!({"plane":"XY","offset":5}))["sketch"].as_u64().unwrap();
            let mut p = json!({"refs":[{"edge":[60,0,5],"body":world_name(&s, &[a2])}]});
            if let Some(policy) = policy {
                p["strict_occurrence_references"] = json!(policy);
            }
            let before = s.doc.to_json();
            if policy == Some(true) {
                let e = s.execute(command, &p).unwrap_err();
                assert!(e.to_string().contains("another placement of its own component"), "{e}");
                assert_eq!(s.doc.to_json(), before);
            } else {
                let r = run(&mut s, command, p);
                let link = r["links"][0]["link"].as_str().unwrap();
                assert!(s.doc.sketch(sketch).unwrap().link(link).unwrap().placement.is_none());
                run(&mut s, "sketch.finish", json!({}));
                let mut reopened = Session { doc: solidcraft_doc::format::read(&s.doc.to_json()).unwrap().into(), ..Default::default() };
                reopened.refresh();
                let st = reopened.model.state();
                let l = st.sketch(sketch).unwrap().sketch.link(link).unwrap();
                assert!(l.placement.is_none());
                assert!(!l.lost);
            }
        }
    }
}

#[test]
fn occurrence_compatibility_never_allows_own_or_future_sketch_geometry() {
    for command in ["sketch.project", "sketch.intersect", "sketch.include_3d"] {
        for policy in [None, Some(false), Some(true)] {
            let mut s = Session::default();
            let earlier = run(&mut s, "sketch.create", json!({"plane":"XY"}))["sketch"].as_u64().unwrap();
            run(&mut s, "sketch.line", json!({"points":[[0,0],[5,0]],"ids":["own"]}));
            run(&mut s, "sketch.finish", json!({}));
            let later = run(&mut s, "sketch.create", json!({"plane":"XY"}))["sketch"].as_u64().unwrap();
            run(&mut s, "sketch.line", json!({"points":[[0,1],[5,1]],"ids":["future"]}));
            run(&mut s, "sketch.finish", json!({}));
            run(&mut s, "sketch.edit", json!({"sketch":earlier,"restore_marker":false}));
            for (sketch, curve, reason) in [(earlier, "own", "own geometry"), (later, "future", "comes after")] {
                let before = s.doc.to_json();
                let mut p = json!({"refs":[{"sketch":sketch,"curve":curve}]});
                if let Some(policy) = policy {
                    p["strict_occurrence_references"] = json!(policy);
                }
                let e = s.execute(command, &p).unwrap_err();
                assert!(e.to_string().contains(reason), "{e}");
                assert_eq!(s.doc.to_json(), before);
            }
        }
    }
}

#[test]
fn references_inside_another_receiver_copy_keep_main_unless_strict() {
    for command in ["sketch.project", "sketch.intersect", "sketch.include_3d"] {
        for policy in [None, Some(false), Some(true)] {
            let mut s = Session::default();
            let parent = run(&mut s, "component.create", json!({"name":"Receiver","grounded":false}));
            let receiver = parent["component"].as_u64().unwrap();
            let first = parent["occurrence"].as_u64().unwrap();
            let child = run(&mut s, "component.create", json!({"name":"Child","parent":receiver}))["occurrence"].as_u64().unwrap();
            run(&mut s, "solid.box", json!({"length":10,"width":10,"height":10,"body_name":"Plate"}));
            let second = run(&mut s, "occurrence.copy", json!({"occurrence":first}))["occurrence"].as_u64().unwrap();
            run(&mut s, "occurrence.move", json!({"occurrence":second,"translate":[50,0,0]}));
            run(&mut s, "component.activate", json!({"component":receiver,"occurrence":first,"path":[first]}));
            let sketch = run(&mut s, "sketch.create", json!({"plane":"XY","offset":5}))["sketch"].as_u64().unwrap();
            let mut p = json!({"refs":[{"edge":[60,0,5],"body":world_name(&s, &[second,child])}]});
            if let Some(policy) = policy {
                p["strict_occurrence_references"] = json!(policy);
            }
            let before = s.doc.to_json();
            if policy == Some(true) {
                let e = s.execute(command, &p).unwrap_err();
                assert!(e.to_string().contains("inside another placement of its own component"), "{e}");
                assert_eq!(s.doc.to_json(), before);
            } else {
                let r = run(&mut s, command, p);
                let link = r["links"][0]["link"].as_str().unwrap();
                assert!(s.doc.sketch(sketch).unwrap().link(link).unwrap().placement.is_none());
                run(&mut s, "sketch.finish", json!({}));
                let mut reopened = Session { doc: solidcraft_doc::format::read(&s.doc.to_json()).unwrap().into(), ..Default::default() };
                reopened.refresh();
                let st = reopened.model.state();
                let link = st.sketch(sketch).unwrap().sketch.link(link).unwrap();
                assert!(link.placement.is_none());
                assert!(!link.lost);
            }
        }
    }
}
