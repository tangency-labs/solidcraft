use serde_json::{Value, json};
use solidcraft_geom::prelude::*;

use crate::Session;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    match s.execute(id, &p) {
        Ok(v) => v,
        Err(e) => panic!("{id} {p}: {e}"),
    }
}

fn ids(v: &Value) -> Vec<String> {
    v.as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default()
}

#[test]
fn manual_projection_captures_the_requested_coincident_body_producer() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length":10,"width":10,"height":10,"body_name":"Control","name":"ControlFeature"}));
    run(&mut s, "solid.box", json!({"length":20,"width":20,"height":10,"body_name":"Target","name":"TargetFeature"}));
    let producer = s.doc.find_feature("TargetFeature").unwrap().id;
    let id = run(&mut s, "sketch.create", json!({"plane":"XY","offset":10}))["sketch"].as_u64().unwrap();
    run(&mut s, "sketch.project", json!({"refs":[{"body":"Target","face":[1,1,10]}]}));
    run(&mut s, "sketch.finish", json!({}));
    let initial_state = s.model.state();
    let initial = &initial_state.sketch(id).unwrap().sketch.links[0];
    assert_eq!(initial.face_name, Some(format!("F{producer}:+z")));
    assert!(!initial.lost);
    let height = s.doc.find_feature("TargetFeature").unwrap().param_names[2].clone();
    run(&mut s, "parameters.change", json!({"name":height,"expression":"20 mm"}));
    let verify = |session: &Session| {
        let state = session.model.state();
        let sk = &state.sketch(id).unwrap().sketch;
        assert!(!sk.links[0].lost);
        assert_eq!(sk.links[0].face_name, Some(format!("F{producer}:+z")));
        assert!(matches!(&sk.links[0].source,solidcraft_sketch::LinkSource::Face{body,at} if body=="Target" && (at.z-20.).abs()<1e-6));
        assert!(sk.points.iter().any(|point| point.pos.x > 19.));
        assert!(sk.points.iter().any(|point| point.pos.y > 19.));
        assert!((session.world_state().unwrap().body("Control").unwrap().mesh().bounds().max.z - 10.).abs() < 1e-6);
    };
    verify(&s);
    verify(&Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap()));
}

#[test]
fn initial_projection_retains_selected_face_when_closest_point_lands_on_shared_edge() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length":20,"width":20,"height":10,"body_name":"Target"}));
    let state = s.model.state();
    let body = state.body("Target").unwrap();
    let source = solidcraft_sketch::LinkSource::Face { body: "Target".into(), at: solidcore_record::Vec3::new(-0.1, 5., 10.5) };
    let resolved = solidcraft_doc::project::resolve(
        &s.doc,
        &std::collections::BTreeMap::new(),
        &state,
        &solidcore_record::Plane::XY,
        solidcraft_sketch::LinkKind::Project,
        &source,
    )
    .unwrap();
    let index = resolved.face_index.unwrap();
    let expected = solidcraft_doc::naming::face_names(body)[index].clone();
    let solidcraft_sketch::LinkSource::Face { at, .. } = resolved.source else { panic!("not face") };
    assert!(at.x.abs() < 1e-6 && (at.z - 10.).abs() < 1e-6, "closestpoint must be on shared top/wall edge: {at:?}");
    let plane = body.body.face_plane(index).unwrap();
    let id = run(&mut s, "sketch.create", json!({"plane":{"origin":plane.origin,"x_dir":plane.x,"y_dir":plane.y}}))["sketch"].as_u64().unwrap();
    run(&mut s, "sketch.project", json!({"refs":[{"body":"Target","face":[-0.1,5.,10.5]}]}));
    run(&mut s, "sketch.finish", json!({}));
    let current = s.model.state();
    let sk = &current.sketch(id).unwrap().sketch;
    assert_eq!(sk.links[0].face_name, Some(expected));
    assert!(!sk.links[0].lost);
    assert_eq!(sk.curves.len(), 4);
    let reopened = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
    assert_eq!(reopened.model.state().sketch(id).unwrap().sketch.curves, sk.curves);
}

/// A 40 x 30 x 20 box from a sketch whose width is the user parameter `width`.
fn box_part(s: &mut Session) {
    run(s, "parameters.change", json!({"name": "width", "expression": "40 mm"}));
    run(s, "sketch.create", json!({"plane": "XY"}));
    let r = run(s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [40, 30]}));
    let l = ids(&r["curves"]);
    let sk = s.doc.sketch(s.active_sketch.unwrap()).unwrap();
    let start = match sk.curves[0].kind {
        solidcraft_sketch::CurveKind::Line { a, .. } => sk.points[a].id.clone(),
        _ => panic!(),
    };
    run(s, "sketch.constraint.coincident", json!({"a": start, "b": "origin"}));
    run(s, "sketch.dimension", json!({"entities": [l[0]], "value": "width"}));
    run(s, "sketch.dimension", json!({"entities": [l[1]], "value": 30}));
    run(s, "sketch.finish", json!({}));
    run(s, "solid.extrude", json!({"distance": 20}));
}

fn inspect(s: &mut Session) -> Value {
    run(s, "sketch.inspect", json!({}))
}

fn linked_curves(si: &Value) -> Vec<Value> {
    si["curves"].as_array().unwrap().iter().filter(|c| c.get("link").is_some()).cloned().collect()
}

#[test]
fn sketch_on_face_projects_its_edges_and_follows_edits() {
    let mut s = Session::default();
    box_part(&mut s);
    let r = run(&mut s, "sketch.create", json!({"plane": {"face": [20, 15, 20]}}));
    assert!(r["projected"].is_string(), "{r}");
    let si = inspect(&mut s);
    let lc = linked_curves(&si);
    assert_eq!(lc.len(), 4, "{si}");
    assert!(lc.iter().all(|c| c["type"] == "line"));
    // The face loop is one closed profile of projected lines.
    assert_eq!(si["profiles"].as_array().unwrap().len(), 1, "{si}");
    assert!((si["profiles"][0]["area_mm2"].as_f64().unwrap() - 1200.0).abs() < 1e-6);
    // Projected geometry is fixed: the sketch has no degrees of freedom.
    assert_eq!(si["dof"], 0, "{si}");
    // A circle on the face, dimensioned to a projected edge.
    let c = run(&mut s, "sketch.circle.center", json!({"center": [10, 10], "radius": 3}));
    let cid = ids(&c["curves"])[0].clone();
    let e = lc.iter().find(|c| c["start_at"][1].as_f64().unwrap().abs() < 1e-9 && c["end_at"][1].as_f64().unwrap().abs() < 1e-9).unwrap();
    run(&mut s, "sketch.dimension", json!({"entities": [format!("{cid}.center"), e["id"]], "value": 8}));
    let si = inspect(&mut s);
    let center = si["points"].as_array().unwrap().iter().find(|p| p["id"] == format!("{cid}.center")).unwrap();
    assert!((center["at"][1].as_f64().unwrap() - 8.0).abs() < 1e-7, "{center}");
    run(&mut s, "sketch.finish", json!({}));
    // Widen the box: the projected loop follows.
    run(&mut s, "parameters.change", json!({"name": "width", "expression": "50 mm"}));
    let st = s.model.state();
    let ss = st.sketches.last().unwrap();
    assert_eq!(ss.profiles.len(), 2);
    let outer = ss.profiles.iter().map(|p| p.area).fold(0.0, f64::max);
    assert!((outer - (1500.0 - std::f64::consts::PI * 9.0)).abs() < 1e-6, "{outer}");
    let w = s.model.results.last().unwrap().warning.clone();
    assert!(w.is_none(), "{w:?}");
}

/// A tab drawn from a corner of a projected face, with its side held on the face's edge,
/// follows the face when the face changes shape (a second hole) and grows: the projection is
/// rebuilt, and the tab keeps its corner and its edge instead of losing its lines or being left
/// behind inside the grown plate.
#[test]
fn a_tab_on_a_projected_face_follows_the_face_when_its_projection_is_rebuilt() {
    let mut s = Session::default();
    run(&mut s, "parameters.change", json!({"name": "w", "expression": "60 mm"}));
    let plate = run(&mut s, "sketch.create", json!({"plane": "XY"}))["sketch"].as_u64().unwrap();
    run(&mut s, "sketch.line", json!({"points": [[0, 0], [60, 0], [60, 40], [0, 40]], "closed": true, "infer": true, "ids": ["a", "b", "c", "d"]}));
    run(&mut s, "sketch.constraint.coincident", json!({"a": "a.start", "b": "origin"}));
    run(&mut s, "sketch.dimension", json!({"entities": ["a"], "value": "w"}));
    run(&mut s, "sketch.dimension", json!({"entities": ["b"], "value": 40}));
    run(&mut s, "sketch.circle.center", json!({"center": [15, 20], "diameter": 6}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"distance": 10, "body_name": "Plate"}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "offset": 10}));
    run(&mut s, "sketch.project", json!({"refs": [{"face": [30, 5, 10]}]}));
    let at = |c: &Value, end: &str, axis: usize| c[end][axis].as_f64().unwrap();
    let right = linked_curves(&inspect(&mut s))
        .into_iter()
        .find(|c| (at(c, "start_at", 0) - 60.0).abs() < 1e-9 && (at(c, "end_at", 0) - 60.0).abs() < 1e-9)
        .unwrap();
    let id = right["id"].as_str().unwrap();
    let corner = if (at(&right, "end_at", 1) - 40.0).abs() < 1e-9 { format!("{id}.end") } else { format!("{id}.start") };
    let tab = ids(&run(
        &mut s,
        "sketch.line",
        json!({"points": [corner, [75, 40], [75, 25], format!("on:{id}:60,25")], "closed": true, "infer": true}),
    )["curves"]);
    run(&mut s, "sketch.dimension", json!({"entities": [tab[0]], "value": 15}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"distance": 10, "direction": "negative", "operation": "join"}));
    let reach = |s: &Session| s.model.state().body("Plate").unwrap().mesh().bounds().max.x;
    assert!((reach(&s) - 75.0).abs() < 1e-6, "{}", reach(&s));
    // The plate gets a second hole and grows to 80 mm.
    run(&mut s, "sketch.edit", json!({"sketch": plate}));
    run(&mut s, "sketch.circle.center", json!({"center": [40, 20], "diameter": 6}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "parameters.change", json!({"name": "w", "expression": "80 mm"}));
    let failed: Vec<_> = s.model.results.iter().filter_map(|r| r.error.clone()).collect();
    assert!(failed.is_empty(), "{failed:?}");
    assert!((reach(&s) - 95.0).abs() < 1e-6, "the tab stays on the plate's grown edge: {}", reach(&s));
}

#[test]
fn auto_project_can_be_turned_off() {
    let mut s = Session::default();
    box_part(&mut s);
    let r = run(&mut s, "sketch.create", json!({"plane": {"face": [20, 15, 20]}, "project_edges": false}));
    assert!(r["projected"].is_null());
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "sketch.auto_project", json!({"value": false}));
    let r = run(&mut s, "sketch.create", json!({"plane": {"face": [20, 15, 20]}}));
    assert!(r["projected"].is_null());
}

/// #680: auto-projection is a preference, not part of a design: turning it off holds for the
/// next design started or opened in the session (`doc.recover` replaces the session the same way).
#[test]
fn auto_project_off_holds_for_designs_started_and_opened() {
    let dir = std::env::temp_dir().join(format!("solidcraft-auto-project-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("box.solidcraft");
    let mut s = Session::default();
    box_part(&mut s);
    run(&mut s, "file.save_as", json!({"path": file.to_string_lossy()}));
    run(&mut s, "sketch.auto_project", json!({"value": false}));
    run(&mut s, "file.new", json!({}));
    assert!(!s.auto_project, "file.new");
    run(&mut s, "doc.open", json!({"path": file.to_string_lossy()}));
    assert!(!s.auto_project, "doc.open");
    let r = run(&mut s, "sketch.create", json!({"plane": {"face": [20, 15, 20]}}));
    assert!(r["projected"].is_null(), "{r}");
    run(&mut s, "sketch.auto_project", json!({"value": true}));
    run(&mut s, "file.new", json!({}));
    assert!(s.auto_project, "turned back on, it stays on");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn project_edges_bodies_sketches_and_origin() {
    let mut s = Session::default();
    box_part(&mut s);
    run(&mut s, "solid.cylinder", json!({"base": [80, 0, 5], "radius": 6, "height": 10}));
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    // An edge on the top face projects to a line on XY.
    let r = run(&mut s, "sketch.project", json!({"refs": [{"edge": [20, 30, 20]}]}));
    let si = inspect(&mut s);
    let c = si["curves"].as_array().unwrap().iter().find(|c| c["id"] == r["links"][0]["curves"][0]).unwrap().clone();
    assert_eq!(c["type"], "line");
    let ys: Vec<f64> = [c["start_at"][1].as_f64().unwrap(), c["end_at"][1].as_f64().unwrap()].to_vec();
    assert!(ys.iter().all(|y| (y - 30.0).abs() < 1e-9), "{c}");
    // The cylinder's silhouette seen from above is its circle, exactly.
    let name = s.model.state().bodies[1].name.clone();
    let r = run(&mut s, "sketch.project", json!({"refs": [{"body": name}]}));
    let cid = r["links"][0]["curves"][0].clone();
    let si = inspect(&mut s);
    let c = si["curves"].as_array().unwrap().iter().find(|c| c["id"] == cid).unwrap().clone();
    assert_eq!(c["type"], "circle", "{c}");
    assert!((c["radius"].as_f64().unwrap() - 6.0).abs() < 1e-6, "{c}");
    assert!((c["center_at"][0].as_f64().unwrap() - 80.0).abs() < 1e-6, "{c}");
    // Origin, the Y axis and the YZ plane.
    run(&mut s, "sketch.project", json!({"refs": ["origin", {"axis": "Y"}, {"plane": "YZ"}]}));
    // The box silhouette from above is its 40 x 30 outline.
    let b0 = s.model.state().bodies[0].name.clone();
    let r = run(&mut s, "sketch.project", json!({"refs": [{"body": b0}]}));
    assert_eq!(r["links"][0]["curves"].as_array().unwrap().len(), 4, "{r}");
    run(&mut s, "sketch.finish", json!({}));
    let first = s.active_sketch;
    assert!(first.is_none());
    let sk1 = s.doc.features.last().unwrap().id;
    // Another sketch projects a curve of the first one.
    run(&mut s, "sketch.create", json!({"plane": "XY", "offset": 50}));
    let r = run(&mut s, "sketch.project", json!({"refs": [{"sketch": sk1, "curve": cid}]}));
    let c2 = r["links"][0]["curves"][0].clone();
    let si = inspect(&mut s);
    let c = si["curves"].as_array().unwrap().iter().find(|c| c["id"] == c2).unwrap().clone();
    assert_eq!(c["type"], "circle");
    // Bad references fail cleanly.
    assert!(s.execute("sketch.project", &json!({"refs": [{"edge": [1000, 1000, 1000]}]})).is_err());
    assert!(s.execute("sketch.project", &json!({"refs": [{"plane": "XY"}]})).is_err(), "parallel plane projects to nothing");
}

/// A construction axis projects as an origin axis does, as its line in the sketch plane, and stays
/// linked to it: the projection follows when the axis moves and when it is renamed. The report's
/// steps (#9324): the axis made after the sketch, then moved before it.
#[test]
fn a_construction_axis_projects_and_stays_linked() {
    let mut s = Session::default();
    let sketch = run(&mut s, "sketch.create", json!({"plane": "XY"}))["sketch"].as_u64().unwrap();
    run(&mut s, "sketch.finish", json!({}));
    let axis = |x: f64| json!({"a": {"point": [x, 0, 0]}, "b": {"point": [x, 30, 0]}, "name": "Axis1"});
    run(&mut s, "construct.axis.two_points", axis(10.0));
    run(&mut s, "timeline.reorder", json!({"feature": "Axis1", "position": 0}));
    run(&mut s, "sketch.edit", json!({"sketch": sketch}));
    let r = run(&mut s, "sketch.project", json!({"refs": [{"axis": "Axis1"}]}));
    let line = r["links"][0]["curves"][0].clone();
    // Where the projected line runs: its two ends' x, and its length along y.
    let at = |s: &mut Session| {
        run(s, "sketch.edit", json!({"sketch": sketch}));
        let si = inspect(s);
        let c = si["curves"].as_array().unwrap().iter().find(|c| c["id"] == line).cloned().unwrap();
        run(s, "sketch.finish", json!({}));
        let n = |v: &Value| v.as_f64().unwrap();
        (n(&c["start_at"][0]), n(&c["end_at"][0]), (n(&c["end_at"][1]) - n(&c["start_at"][1])).abs())
    };
    run(&mut s, "sketch.finish", json!({}));
    let (x0, x1, length) = at(&mut s);
    assert!((x0 - 10.0).abs() < 1e-9 && (x1 - 10.0).abs() < 1e-9 && length > 30.0, "{x0} {x1} {length}");
    run(&mut s, "timeline.redefine", json!({"feature": "Axis1", "command": "construct.axis.two_points", "params": axis(25.0)}));
    let (x0, x1, _) = at(&mut s);
    assert!((x0 - 25.0).abs() < 1e-9 && (x1 - 25.0).abs() < 1e-9, "the projection follows the axis: {x0} {x1}");
    run(&mut s, "timeline.rename", json!({"feature": "Axis1", "name": "Hinge axis"}));
    let state = s.model.state();
    let link = &state.sketch(sketch).unwrap().sketch.links[0];
    assert_eq!(link.source, solidcraft_sketch::LinkSource::Axis { name: "Hinge axis".into() });
    assert!(!link.lost, "renamed, the axis is still found");
    // Moved after the sketch, the axis can't be found from it: the link is lost, saying why, at
    // once and when the design is opened again (#9544). Moved back, it is found again.
    run(&mut s, "timeline.reorder", json!({"feature": "Hinge axis", "position": 1}));
    let opened = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
    for session in [&s, &opened] {
        assert!(session.model.state().sketch(sketch).unwrap().sketch.links[0].lost);
        let warnings = session.model.result(sketch).and_then(|r| r.warning.clone()).unwrap_or_default();
        assert!(warnings.contains("it must come earlier in the timeline"), "{warnings}");
    }
    run(&mut s, "timeline.reorder", json!({"feature": "Hinge axis", "position": 0}));
    assert!(!s.model.state().sketch(sketch).unwrap().sketch.links[0].lost);
    // An axis that is no origin axis and no construction axis is refused, by its name.
    run(&mut s, "sketch.edit", json!({"sketch": sketch}));
    let error = s.execute("sketch.project", &json!({"refs": [{"axis": "Axis9"}]})).unwrap_err().to_string();
    assert!(error.contains("unknown axis `Axis9`"), "{error}");
}

/// A named construction point projects as a fixed point linked to it, on a sketch plane that
/// is not the plane it was made on, and the projection follows when the point moves and when it
/// is renamed (#12075).
#[test]
fn a_construction_point_projects_and_stays_linked() {
    let mut s = Session::default();
    let point = |x: f64, z: f64| json!({"point": {"point": [x, 5, z]}, "name": "Point1"});
    run(&mut s, "construct.point.vertex", point(10.0, 20.0));
    let sketch = run(&mut s, "sketch.create", json!({"plane": "YZ"}))["sketch"].as_u64().unwrap();
    let r = run(&mut s, "sketch.project", json!({"refs": [{"point": "Point1"}]}));
    let projected = r["links"][0]["points"][0].as_str().unwrap().to_string();
    run(&mut s, "sketch.finish", json!({}));
    // Where the projected point is, against where the construction point projects in 3D.
    let check = |s: &mut Session, world: solidcore_record::Vec3| {
        let plane = s.model.state().sketch(sketch).unwrap().plane;
        let want = plane.to_local(world);
        run(s, "sketch.edit", json!({"sketch": sketch}));
        let si = inspect(s);
        let p = si["points"].as_array().unwrap().iter().find(|p| p["id"] == projected).cloned().unwrap();
        run(s, "sketch.finish", json!({}));
        assert!(p["link"].is_string() && p["fixed"] == true, "{p}");
        let at = (p["at"][0].as_f64().unwrap(), p["at"][1].as_f64().unwrap());
        assert!((at.0 - want.x).abs() < 1e-9 && (at.1 - want.y).abs() < 1e-9, "{at:?} {want:?}");
    };
    check(&mut s, solidcore_record::Vec3::new(10.0, 5.0, 20.0));
    run(&mut s, "timeline.redefine", json!({"feature": "Point1", "command": "construct.point.vertex", "params": point(10.0, 35.0)}));
    check(&mut s, solidcore_record::Vec3::new(10.0, 5.0, 35.0));
    run(&mut s, "timeline.rename", json!({"feature": "Point1", "name": "Pivot"}));
    let opened = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
    for session in [&s, &opened] {
        let state = session.model.state();
        let link = &state.sketch(sketch).unwrap().sketch.links[0];
        assert_eq!(link.source, solidcraft_sketch::LinkSource::ConstructionPoint { name: "Pivot".into() });
        assert!(!link.lost, "renamed, the point is still found");
    }
    // A name that is no construction point is refused, by its name.
    run(&mut s, "sketch.edit", json!({"sketch": sketch}));
    let error = s.execute("sketch.project", &json!({"refs": [{"point": "Point9"}]})).unwrap_err().to_string();
    assert!(error.contains("unknown point `Point9`"), "{error}");
}

/// A construction point of a component that is moved projects where the component puts it: into a
/// sketch of the root, where it moves with the component, and into a sketch of the component
/// itself, where it stays where the component's own coordinates put it (#12075).
#[test]
fn a_construction_point_of_a_moved_component_projects_where_it_is_placed() {
    let mut s = Session::default();
    let part = run(&mut s, "component.create", json!({"name": "Part"}))["component"].as_u64().unwrap();
    run(&mut s, "construct.point.vertex", json!({"point": {"point": [10, 5, 20]}, "name": "Point1"}));
    let own = run(&mut s, "sketch.create", json!({"plane": "XY"}))["sketch"].as_u64().unwrap();
    run(&mut s, "sketch.project", json!({"refs": [{"point": "Point1"}]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "component.activate", json!({"component": 0}));
    let root = run(&mut s, "sketch.create", json!({"plane": "XY"}))["sketch"].as_u64().unwrap();
    run(&mut s, "sketch.project", json!({"refs": [{"point": "Point1"}]}));
    run(&mut s, "sketch.finish", json!({}));
    // The projected point of each sketch, in its sketch's coordinates.
    let at = |s: &Session, id: u64| {
        let state = s.model.state();
        let sk = &state.sketch(id).unwrap().sketch;
        assert!(sk.links.iter().all(|l| !l.lost), "{:?}", sk.links);
        let link = sk.links.first().unwrap();
        let p = sk.link_points(&link.id).first().and_then(|p| sk.points.get(*p)).unwrap().pos;
        (p.x, p.y)
    };
    let near = |a: (f64, f64), b: (f64, f64)| (a.0 - b.0).abs() < 1e-9 && (a.1 - b.1).abs() < 1e-9;
    assert!(near(at(&s, own), (10.0, 5.0)), "{:?}", at(&s, own));
    assert!(near(at(&s, root), (10.0, 5.0)), "{:?}", at(&s, root));
    let placement = s.doc.occurrences.iter().find(|o| o.component == part).map(|o| o.id).unwrap();
    run(&mut s, "occurrence.ground", json!({"occurrence": placement, "grounded": false}));
    run(&mut s, "occurrence.move", json!({"occurrence": placement, "translate": [100, 0, 0], "capture": true}));
    let opened = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
    for session in [&s, &opened] {
        assert!(near(at(session, root), (110.0, 5.0)), "the root's projection moves with the part: {:?}", at(session, root));
        assert!(near(at(session, own), (10.0, 5.0)), "the part's own projection stays: {:?}", at(session, own));
    }
}

/// A face-attached sketch authored through the second occurrence must not translate a point
/// of its own definition by the difference from the first occurrence (#12075).
#[test]
fn a_construction_point_of_the_second_author_occurrence_stays_in_its_own_frame() {
    let mut s = Session::default();
    let part = run(&mut s, "component.create", json!({"name":"Part"}));
    let component = part["component"].as_u64().unwrap();
    run(&mut s, "construct.point.vertex", json!({"point":{"point":[3,4,10]},"name":"Pivot"}));
    run(&mut s, "solid.box", json!({"length":10,"width":10,"height":10,"body_name":"Base"}));
    let second = run(&mut s, "occurrence.copy", json!({"occurrence":part["occurrence"],"translate":[40,0,0]}))["occurrence"].as_u64().unwrap();
    run(&mut s, "component.activate", json!({"component":component,"occurrence":second,"path":[second]}));
    let pick = {
        let world = s.world_state().unwrap();
        let body = world.body("Base (Part:2)").unwrap();
        let face = body.body.faces(0.01).unwrap().into_iter().find(|face| face.plane_normal.is_some_and(|normal| normal.z > 0.999)).unwrap();
        json!({"body":body.name,"index":face.index,"face":[face.centroid.x,face.centroid.y,face.centroid.z]})
    };
    let id = run(&mut s, "sketch.create", json!({"plane":pick,"project_edges":false}))["sketch"].as_u64().unwrap();
    let feature = s.doc.feature(id).unwrap();
    let solidcraft_doc::FeatureKind::Sketch { plane, .. } = &feature.kind else { panic!("not a sketch") };
    assert_eq!(solidcraft_doc::face_anchor::plane_face_anchor(plane).unwrap().author_path, vec![second]);
    run(&mut s, "sketch.project", json!({"refs":[{"point":"Pivot"}]}));
    // Check the immediate creation path as well as the later model refresh.
    let check = |s: &Session| {
        assert!(s.model.result(id).unwrap().error.is_none());
        let state = s.model.state();
        let ss = state.sketch(id).unwrap();
        let link = ss.sketch.links.first().unwrap();
        assert!(!link.lost);
        let point = ss.sketch.link_points(&link.id).first().and_then(|p| ss.sketch.points.get(*p)).unwrap();
        let at = ss.plane.to_world(point.pos);
        assert!((at.x - 3.).abs() < 1e-7 && (at.y - 4.).abs() < 1e-7 && (at.z - 10.).abs() < 1e-7, "{at:?}");
    };
    check(&s);
    run(&mut s, "sketch.finish", json!({}));
    check(&s);
    run(&mut s, "component.activate", json!({"component":0}));
    run(&mut s, "occurrence.ground", json!({"occurrence":second,"grounded":false}));
    run(&mut s, "occurrence.move", json!({"occurrence":second,"translate":[0,15,0],"capture":true}));
    check(&s);
    let mut reopened = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
    check(&reopened);
    run(&mut reopened, "sketch.edit", json!({"sketch":id}));
    check(&reopened);
    run(&mut reopened, "sketch.finish", json!({}));
    check(&reopened);
}

#[test]
fn project_whole_body_can_create_unlinked_geometry() {
    let mut s = Session::default();
    box_part(&mut s);
    let sketch = run(&mut s, "sketch.create", json!({"plane":"XY"}))["sketch"].as_u64().unwrap();
    let body = s.model.state().bodies[0].name.clone();
    let r = run(&mut s, "sketch.project", json!({"refs":[{"body":body}],"projection_link":false}));
    assert_eq!(r["links"][0]["link"], Value::Null);
    let sk = s.doc.sketch(sketch).unwrap();
    assert!(sk.links.is_empty());
    assert_eq!(sk.curves.len(), 4);
    assert!(s.execute("sketch.project", &json!({"refs":[{"body":body}],"projection_link":"false"})).is_err());
}

#[test]
fn intersect_cuts_a_body_with_the_sketch_plane() {
    let mut s = Session::default();
    run(&mut s, "solid.cylinder", json!({"base": [0, 0, 0], "radius": 7, "height": 30}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "offset": 12}));
    let name = s.model.state().bodies[0].name.clone();
    let r = run(&mut s, "sketch.intersect", json!({"refs": [{"body": name}]}));
    let si = inspect(&mut s);
    let c = si["curves"].as_array().unwrap().iter().find(|c| c["id"] == r["links"][0]["curves"][0]).unwrap().clone();
    assert_eq!(c["type"], "circle", "{c}");
    assert!((c["radius"].as_f64().unwrap() - 7.0).abs() < 1e-6, "{c}");
    assert_eq!(si["profiles"].as_array().unwrap().len(), 1);
}

#[test]
fn break_link_makes_normal_geometry() {
    let mut s = Session::default();
    box_part(&mut s);
    let r = run(&mut s, "sketch.create", json!({"plane": {"face": [20, 15, 20]}}));
    let link = r["projected"].as_str().unwrap().to_string();
    run(&mut s, "sketch.break_link", json!({"link": link}));
    let si = inspect(&mut s);
    assert!(linked_curves(&si).is_empty());
    assert!(si["links"].as_array().unwrap().is_empty());
    // Now free: 4 lines with 4 shared corners, 8 degrees of freedom.
    assert_eq!(si["dof"], 8, "{si}");
    assert!(s.execute("sketch.break_link", &json!({"link": "j99"})).is_err());
}

/// Trim refuses projected edges until Break Link; the message names that command (#8979).
#[test]
fn trim_refuses_projected_edges_until_break_link() {
    let mut s = Session::default();
    box_part(&mut s);
    let r = run(&mut s, "sketch.create", json!({"plane": {"face": [20, 15, 20]}}));
    let link = r["projected"].as_str().unwrap().to_string();
    let curve = linked_curves(&inspect(&mut s))[0]["id"].as_str().unwrap().to_string();
    let err = s.execute("sketch.trim", &json!({"curve": curve, "at": [20, 15]})).unwrap_err().to_string();
    assert!(err.contains("Break Link"), "{err}");
    assert!(err.contains("projected"), "{err}");
    run(&mut s, "sketch.break_link", json!({"link": link}));
    run(&mut s, "sketch.trim", json!({"curve": curve, "at": [20, 15]}));
}

/// The sketch's curves as evaluated (after its links were refreshed): (id, construction).
fn evaluated_curves(s: &Session, id: u64) -> Vec<(String, bool)> {
    s.model.state().sketch(id).unwrap().sketch.curves.iter().map(|c| (c.id.clone(), c.construction)).collect()
}

#[test]
fn deleting_part_of_an_intersection_does_not_bring_it_back() {
    // #5098: a dome, its section on XZ (arc `a1`, line `l1`) as construction, then the arc deleted.
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XZ"}));
    run(&mut s, "sketch.line", json!({"points": [[0, 0], [10, 0]]}));
    run(&mut s, "sketch.arc.center_point", json!({"center": [0, 0], "start": [0, -10], "end": [10, 0]}));
    run(&mut s, "sketch.line", json!({"points": [[0, -10], [0, 0]]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.revolve", json!({"sketch": 1, "axis": "Z"}));
    let id = run(&mut s, "sketch.create", json!({"plane": "XZ"}))["sketch"].as_u64().unwrap();
    let r = run(&mut s, "sketch.intersect", json!({"refs": [{"body": "Body1"}]}));
    let mut made = ids(&r["created"]["curves"]);
    made.sort();
    assert_eq!(made, ["a1", "l1"], "{r}");
    run(&mut s, "sketch.construction", json!({"curves": ["a1", "l1"], "value": true}));
    run(&mut s, "sketch.delete", json!({"entities": ["a1"]}));
    run(&mut s, "sketch.finish", json!({}));
    // Evaluated, the arc stays gone and the line stays construction: nothing was rebuilt.
    assert_eq!(evaluated_curves(&s, id), [("l1".to_string(), true)]);
    // The line no longer follows the body; it kept its place on the section (x from -10 to 10).
    let sk = s.doc.sketch(id).unwrap();
    assert!(sk.links.is_empty() && sk.curves.iter().all(|c| c.link.is_none()), "{:?}", sk.links);
    let mut xs: Vec<f64> = sk.points.iter().filter(|p| p.id != "origin").map(|p| p.pos.x).collect();
    xs.sort_by(f64::total_cmp);
    assert!(xs.len() == 2 && (xs[0] + 10.0).abs() < 1e-6 && (xs[1] - 10.0).abs() < 1e-6, "{xs:?}");
    // Reopened, the same.
    let reopened = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
    assert_eq!(evaluated_curves(&reopened, id), [("l1".to_string(), true)]);
}

#[test]
fn deleting_one_edge_of_a_projected_face_keeps_the_others_as_they_were() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"corner": [0, 0, 0], "length": 20, "width": 20, "height": 10}));
    let id = run(&mut s, "sketch.create", json!({"plane": "XY", "offset": 15}))["sketch"].as_u64().unwrap();
    let r = run(&mut s, "sketch.project", json!({"refs": [{"face": [10, 10, 10]}]}));
    let lines = ids(&r["created"]["curves"]);
    assert_eq!(lines.len(), 4, "{r}");
    run(&mut s, "sketch.construction", json!({"curves": lines, "value": true}));
    run(&mut s, "sketch.delete", json!({"entities": [lines[0]]}));
    run(&mut s, "sketch.finish", json!({}));
    let kept: Vec<(String, bool)> = lines[1..].iter().map(|l| (l.clone(), true)).collect();
    assert_eq!(evaluated_curves(&s, id), kept);

    // Deleting the whole link still just removes it.
    let id = run(&mut s, "sketch.create", json!({"plane": "XY", "offset": 15}))["sketch"].as_u64().unwrap();
    let r = run(&mut s, "sketch.project", json!({"refs": [{"face": [10, 10, 10]}]}));
    run(&mut s, "sketch.delete", json!({"entities": ids(&r["created"]["curves"])}));
    run(&mut s, "sketch.finish", json!({}));
    assert!(evaluated_curves(&s, id).is_empty());
    assert!(s.doc.sketch(id).unwrap().links.is_empty());
}

/// The edge a projection was made from goes with its body: the sketch fails, asking for the edge
/// again, instead of keeping a lost link.
#[test]
fn a_reference_whose_body_is_gone_fails_its_sketch_asking_for_it_again() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10}));
    let boxf = s.doc.features.last().unwrap().id;
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    run(&mut s, "sketch.project", json!({"refs": [{"edge": [5, 0, 10]}]}));
    run(&mut s, "sketch.finish", json!({}));
    let name = s.doc.sketch(s.doc.features.last().unwrap().id).unwrap().links[0].edge_name.clone().unwrap();
    run(&mut s, "timeline.suppress", json!({"feature": boxf, "suppressed": true}));
    let r = s.model.results.last().unwrap();
    assert_eq!(r.error.as_deref().and_then(solidcraft_doc::reselect_parts), Some((r.name.as_str(), name.as_str())), "{r:?}");
}

#[test]
fn drawing_on_a_model_vertex_projects_it() {
    let mut s = Session::default();
    box_part(&mut s);
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    let r = run(&mut s, "sketch.line", json!({"points": ["vertex:40,30,20", [60, 45]]}));
    let l = ids(&r["curves"])[0].clone();
    let si = inspect(&mut s);
    let start = si["curves"].as_array().unwrap().iter().find(|c| c["id"] == l).unwrap()["start"].clone();
    let p = si["points"].as_array().unwrap().iter().find(|p| p["id"] == start).unwrap().clone();
    assert!(p["link"].is_string(), "{p}");
    assert_eq!(p["fixed"], true);
    assert!((p["at"][0].as_f64().unwrap() - 40.0).abs() < 1e-9);
    // Snapping onto an edge projects it and keeps the point on it.
    let r = run(&mut s, "sketch.line", json!({"points": [{"on_edge": [20, 0, 20]}, [20, -15]]}));
    assert_eq!(ids(&r["curves"]).len(), 1);
    let si = inspect(&mut s);
    assert!(si["constraints"].as_array().unwrap().iter().any(|c| c["type"] == "point_on_curve"), "{si}");
}

#[test]
fn fit_curves_to_a_mesh_section() {
    let mut s = Session::default();
    run(&mut s, "solid.cylinder", json!({"base": [0, 0, 0], "radius": 7, "height": 30}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "offset": 10}));
    let name = s.model.state().bodies[0].name.clone();
    let r = run(&mut s, "sketch.fit_curves_to_section", json!({"body": name}));
    assert_eq!(r["curves"].as_array().unwrap().len(), 1, "{r}");
    let si = inspect(&mut s);
    assert!(si["links"].as_array().unwrap().is_empty());
    let c = &si["curves"][0];
    assert_eq!(c["type"], "circle");
    assert!(c.get("link").is_none());
    assert!(s.execute("sketch.fit_curves_to_section", &json!({"body": "nope"})).is_err());
}

#[test]
fn three_d_sketch_curves() {
    let mut s = Session::default();
    // Include keeps an out-of-plane edge 3D.
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10}));
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    let r = run(&mut s, "sketch.include_3d", json!({"refs": [{"edge": [10, 10, 5]}]}));
    let st = s.model.state();
    let sk = &st.sketch(s.active_sketch.unwrap()).unwrap().sketch;
    assert_eq!(sk.wires.len(), 1, "{r}");
    assert!((sk.wires[0].pts.iter().map(|p| p.z).fold(0.0, f64::max) - 10.0).abs() < 1e-9);
    run(&mut s, "sketch.finish", json!({}));

    // Project a line onto a sphere along the sketch normal.
    let mut s = Session::default();
    run(&mut s, "solid.sphere", json!({"center": [0, 0, 0], "radius": 20}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "offset": 50}));
    let l = ids(&run(&mut s, "sketch.line", json!({"points": [[-10, 0], [10, 0]]}))["curves"])[0].clone();
    run(&mut s, "sketch.finish", json!({}));
    let src = s.doc.features.last().unwrap().id;
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    let r = run(&mut s, "sketch.project_to_surface", json!({"curves": [{"sketch": src, "curve": l}], "face": [0, 0, 20]}));
    // The line's projection where it first meets the sphere: the arc of the great circle in y = 0
    // from x = -10 to 10 over the top (z = √(400 − x²)), one wire though SMLib splits it at the
    // pole, and no curve on the far side.
    assert_eq!(r["links"][0]["wires"].as_array().unwrap().len(), 1, "{r}");
    let wid = r["links"][0]["wires"][0].as_str().unwrap().to_string();
    let st = s.model.state();
    let sk = &st.sketch(s.active_sketch.unwrap()).unwrap().sketch;
    let w = &sk.wires[sk.wire_index(&wid).unwrap()];
    assert!(!w.exact.is_empty());
    // SMLib makes projected curves to a millionth of the body's size (the sphere's 40 mm box).
    let tol = 40.0 * 1e-6;
    let ends = [*w.pts.first().unwrap(), *w.pts.last().unwrap()];
    let top = 300f64.sqrt();
    assert!(ends.iter().all(|e| (e.x.abs() - 10.0).abs() < tol && e.y.abs() < tol && (e.z - top).abs() < tol), "{ends:?}");
    assert!((ends[0].x + ends[1].x).abs() < tol, "{ends:?}");
    // Along its exact curves (the wire's points are its spans' polygon), on the sphere's top.
    for c in &w.exact {
        let eval = solidcraft_geom::brep::eval::CurveEval::new(&c.curve).unwrap();
        for k in 0..=32 {
            let p = eval.point(c.interval[0] + (c.interval[1] - c.interval[0]) * f64::from(k) / 32.0).unwrap();
            assert!((p.len() - 20.0).abs() < tol && p.y.abs() < tol && p.z >= top - tol, "{p:?}");
        }
    }
    // Delete the wire.
    run(&mut s, "sketch.delete", json!({"entities": [wid]}));
    assert!(s.model.state().sketch(s.active_sketch.unwrap()).unwrap().sketch.wires.is_empty());
    run(&mut s, "sketch.finish", json!({}));

    // Two crossing cylinders meet in 3D curves.
    let mut s = Session::default();
    run(&mut s, "solid.cylinder", json!({"base": [0, 0, -20], "radius": 5, "height": 40}));
    run(&mut s, "solid.cylinder", json!({"base": [-20, 0, 0], "axis": [1, 0, 0], "radius": 4, "height": 40, "operation": "new"}));
    let names: Vec<String> = s.model.state().bodies.iter().map(|b| b.name.clone()).collect();
    assert_eq!(names.len(), 2, "{names:?}");
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    let r = run(&mut s, "sketch.intersection_curve", json!({"a": {"body": names[0]}, "b": {"body": names[1]}}));
    assert!(!r["wires"].as_array().unwrap().is_empty(), "{r}");
    let st = s.model.state();
    let sk = &st.sketch(s.active_sketch.unwrap()).unwrap().sketch;
    for w in &sk.wires {
        for p in &w.pts {
            assert!(((p.x * p.x + p.y * p.y).sqrt() - 5.0).abs() < 0.1, "{p:?}");
            assert!(((p.y * p.y + p.z * p.z).sqrt() - 4.0).abs() < 0.1, "{p:?}");
        }
    }
    run(&mut s, "sketch.finish", json!({}));

    // Spun profile of a cylinder about Z, on the XZ plane: a straight outline 6 from the axis.
    let mut s = Session::default();
    run(&mut s, "solid.cylinder", json!({"base": [0, 0, 0], "radius": 6, "height": 20}));
    let name = s.model.state().bodies[0].name.clone();
    run(&mut s, "sketch.create", json!({"plane": "XZ"}));
    let r = run(&mut s, "sketch.spun_profile", json!({"body": name, "axis": "Z"}));
    let si = inspect(&mut s);
    let c = si["curves"].as_array().unwrap().iter().find(|c| c["id"] == r["curves"][0]).unwrap().clone();
    assert_eq!(c["type"], "line", "{c}");
    assert!((c["start_at"][0].as_f64().unwrap().abs() - 6.0).abs() < 1e-3, "{c}");
}

/// #9009: picked faces intersect as the viewport picks them, the whole surface patch, not just the
/// B-rep piece under the pick; a fit-point spline's wall is one face per span joined by hidden seams.
#[test]
fn intersection_curve_of_picked_faces_spans_the_whole_surface() {
    use solidcore_record::Plane;
    use solidcraft_doc::project::resolve;
    use solidcraft_sketch::{LinkKind, LinkSource};
    use std::collections::BTreeMap;

    let mut s = Session::default();
    run(&mut s, "parameters.add", json!({"name": "wall_h", "expression": "20 mm"}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Wave"}));
    let wave = ids(&run(&mut s, "sketch.spline.fit_point", json!({"points": [[0, 0], [9.5, 12], [30.5, 7], [52, 15]]}))["curves"])[0].clone();
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "surface.extrude", json!({"sketch": "Wave", "curves": [wave], "distance": "wall_h", "body_name": "Wall"}));
    run(&mut s, "sketch.create", json!({"plane": "XZ", "name": "Roof"}));
    // XZ sketch y is world -Z, so the arc peaks at world z = 15.
    let arc = run(&mut s, "sketch.arc.three_point", json!({"start": [0, 0], "end": [50, 0], "through": [25, -15]}))["created"]["curves"][0]
        .as_str()
        .unwrap()
        .to_string();
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "surface.extrude", json!({"sketch": "Roof", "curves": [arc], "distance": 30, "body_name": "Roof"}));

    let st = s.model.state();
    let wall = st.body("Wall").unwrap();
    let m = wall.mesh();
    assert!(wall.body.face_count() > 1, "premise of #9009: one face per spline span");
    // A pick on the middle span, away from the joins at x = 9.5 and x = 30.5.
    let pick =
        *m.positions.iter().min_by(|a, b| ((a.x - 20.).abs() + (a.z - 10.).abs()).total_cmp(&((b.x - 20.).abs() + (b.z - 10.).abs()))).unwrap();

    let check = |s: &Session, id: u64| {
        let st = s.model.state();
        let sk = &st.sketch(id).unwrap().sketch;
        assert!(sk.links.iter().all(|l| !l.lost), "no link lost");
        let pts: Vec<_> = sk.wires.iter().flat_map(|w| w.pts.iter().copied()).collect();
        assert!(!pts.is_empty());
        let (lo, hi) = pts.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(l, h), p| (l.min(p.x), h.max(p.x)));
        assert!(lo < 1.0 && hi > 49.0, "the whole crossing, not one span: x {lo}..{hi}");
        for p in &pts {
            assert!((((p.x - 25.).powi(2) + (p.z + 40. / 3.).powi(2)).sqrt() - 85. / 3.).abs() < 0.1, "{p:?}");
            assert!(p.y > -1e-6 && p.y < 30. + 1e-6, "{p:?}");
        }
    };

    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    let id = s.active_sketch.unwrap();
    let r = run(
        &mut s,
        "sketch.intersection_curve",
        json!({"a": {"face": [pick.x, pick.y, pick.z], "body": "Wall"}, "b": {"face": [25, 15, 15], "body": "Roof"}}),
    );
    assert!(!r["wires"].as_array().unwrap().is_empty(), "{r}");
    check(&s, id);

    // The same as giving the two bodies.
    let st = s.model.state();
    let body = |n: &str| Box::new(LinkSource::Body { body: n.to_string() });
    let whole = resolve(&s.doc, &BTreeMap::new(), &st, &Plane::XY, LinkKind::Include, &LinkSource::Intersection { a: body("Wall"), b: body("Roof") })
        .unwrap();
    let sk = &st.sketch(id).unwrap().sketch;
    assert_eq!(sk.wires.len(), whole.wires.len());

    // Editing the wall re-resolves the stored pick to the same patch.
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "parameters.change", json!({"name": "wall_h", "expression": "25 mm"}));
    check(&s, id);
}

/// #11048: two sketch curves meet as their extrusions along their sketches' normals would, with no
/// helper surfaces; a curve also meets a face, and parallel sketches are refused.
#[test]
fn intersection_curve_of_a_picked_flat_face_or_fillet_is_that_face_alone() {
    let mut s = Session::default();
    box_part(&mut s);
    // A tangent round on the vertical edge at x = 40, y = 0: the front face now ends at x = 35.
    run(&mut s, "solid.fillet", json!({"edges": [[40, 0, 10]], "radius": 5}));
    run(&mut s, "sketch.create", json!({"plane": "XZ", "name": "Level"}));
    let level = ids(&run(&mut s, "sketch.line", json!({"points": [[-10, -10], [60, -10]]}))["curves"])[0].clone();
    run(&mut s, "sketch.finish", json!({}));

    let crossing = |s: &mut Session, at: [f64; 3]| {
        run(s, "sketch.create", json!({"plane": "XY"}));
        let id = s.active_sketch.unwrap();
        run(s, "sketch.intersection_curve", json!({"a": {"face": at, "body": "Body1"}, "b": {"sketch": "Level", "curve": level}}));
        run(s, "sketch.finish", json!({}));
        let st = s.model.state();
        let pts: Vec<_> = st.sketch(id).unwrap().sketch.wires.iter().flat_map(|w| w.pts.iter().copied()).collect();
        assert!(!pts.is_empty(), "a crossing at {at:?}");
        for p in &pts {
            assert!((p.z - 10.).abs() < 1e-3, "{p:?}");
        }
        pts
    };
    // The flat front face, not the round it runs smoothly into nor the side beyond.
    let front = crossing(&mut s, [20., 0., 10.]);
    for p in &front {
        assert!(p.y.abs() < 1e-3 && p.x > -1e-3 && p.x < 35. + 1e-3, "off the front face: {p:?}");
    }
    let (lo, hi) = front.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(l, h), p| (l.min(p.x), h.max(p.x)));
    assert!(lo < 0.1 && hi > 34.9, "the whole front face: x {lo}..{hi}");
    // The round, not the flat faces either side of it.
    let h = 5. * std::f64::consts::FRAC_1_SQRT_2;
    for p in &crossing(&mut s, [35. + h, 5. - h, 10.]) {
        assert!(p.x > 35. - 1e-3 && p.y < 5. + 1e-3, "off the round: {p:?}");
    }
}

#[test]
fn intersection_curve_of_two_sketch_curves_is_one_curve_at_any_scale() {
    // The pieces are joined within the kernel's own tolerance, so a tiny part still gives one
    // curve and a large one does not fall apart into pieces.
    for k in [0.01, 100.0] {
        let mut s = Session::default();
        let pts = |p: [[f64; 2]; 4]| json!(p.map(|q| q.map(|v| v * k)));
        run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Top"}));
        let top =
            ids(&run(&mut s, "sketch.spline.fit_point", json!({"points": pts([[0., 0.], [9.5, 12.], [30.5, 7.], [52., 15.]])}))["curves"])[0].clone();
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "sketch.create", json!({"plane": "XZ", "name": "Side"}));
        let side = ids(&run(&mut s, "sketch.spline.fit_point", json!({"points": pts([[-2., -15.], [15., -3.], [35., -17.], [55., -5.]])}))["curves"])
            [0]
        .clone();
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "sketch.create", json!({"plane": "XY"}));
        let r = run(&mut s, "sketch.intersection_curve", json!({"a": {"sketch": "Top", "curve": top}, "b": {"sketch": "Side", "curve": side}}));
        assert_eq!(r["wires"].as_array().unwrap().len(), 1, "scale {k}: {r}");
    }
}

#[test]
fn intersection_curve_of_two_sketch_curves() {
    use solidcore_record::{Vec2, Vec3};

    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Top"}));
    let top = ids(&run(&mut s, "sketch.spline.fit_point", json!({"points": [[0, 0], [9.5, 12], [30.5, 7], [52, 15]]}))["curves"])[0].clone();
    run(&mut s, "sketch.finish", json!({}));
    // XZ sketch y is world -Z: this spline runs between z = 3 and 17.
    run(&mut s, "sketch.create", json!({"plane": "XZ", "name": "Side"}));
    let side = ids(&run(&mut s, "sketch.spline.fit_point", json!({"points": [[-2, -15], [15, -3], [35, -17], [55, -5]]}))["curves"])[0].clone();
    run(&mut s, "sketch.finish", json!({}));

    // Every point lies over both curves: on the top one seen from above, on the side one from the side.
    let off = |poly: &[Vec2], q: Vec2| {
        poly.windows(2)
            .map(|w| Vec3::new(q.x, q.y, 0.).dist_to_segment(Vec3::new(w[0].x, w[0].y, 0.), Vec3::new(w[1].x, w[1].y, 0.)))
            .fold(f64::INFINITY, f64::min)
    };
    let check = |s: &Session, id: u64, x_to: f64| {
        let st = s.model.state();
        let sk = &st.sketch(id).unwrap().sketch;
        assert!(sk.links.iter().all(|l| !l.lost), "no link lost");
        let pts: Vec<_> = sk.wires.iter().flat_map(|w| w.pts.iter().copied()).collect();
        assert!(pts.len() > 10, "{} points", pts.len());
        let curve = |name: &str, c: &str| {
            let ss = st.sketch(s.doc.find_feature(name).unwrap().id).unwrap();
            (ss.plane, ss.sketch.polyline(ss.sketch.curve_index(c).unwrap()))
        };
        let ((tp, tpoly), (sp, spoly)) = (curve("Top", &top), curve("Side", &side));
        for p in &pts {
            assert!(off(&tpoly, tp.to_local(*p)) < 0.05, "off the top curve: {p:?}");
            assert!(off(&spoly, sp.to_local(*p)) < 0.05, "off the side curve: {p:?}");
        }
        let (lo, hi) = pts.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(l, h), p| (l.min(p.x), h.max(p.x)));
        assert!(lo < 0.5 && hi > x_to - 0.5, "the whole crossing: x {lo}..{hi}");
    };

    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    let id = s.active_sketch.unwrap();
    let r = run(&mut s, "sketch.intersection_curve", json!({"a": {"sketch": "Top", "curve": top}, "b": {"sketch": "Side", "curve": side}}));
    assert_eq!(r["wires"].as_array().unwrap().len(), 1, "{r}");
    run(&mut s, "sketch.finish", json!({}));
    check(&s, id, 52.0);

    // Editing a source curve moves the result with it.
    run(&mut s, "sketch.edit", json!({"sketch": "Top"}));
    run(&mut s, "sketch.move_point", json!({"point": "p4", "to": [45, 15]}));
    run(&mut s, "sketch.finish", json!({}));
    check(&s, id, 45.0);

    // A curve against a face: the side curve's surface, extruded for real, gives the same crossing.
    run(&mut s, "surface.extrude", json!({"sketch": "Side", "curves": [side], "distance": 30, "body_name": "Roof"}));
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    let on_face = s.active_sketch.unwrap();
    run(&mut s, "sketch.intersection_curve", json!({"a": {"face": [10, 15, 6], "body": "Roof"}, "b": {"sketch": "Top", "curve": top}}));
    run(&mut s, "sketch.finish", json!({}));
    let st = s.model.state();
    let pts: Vec<_> = st.sketch(on_face).unwrap().sketch.wires.iter().flat_map(|w| w.pts.iter().copied()).collect();
    let (lo, hi) = pts.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(l, h), p| (l.min(p.x), h.max(p.x)));
    assert!(lo < 0.5 && hi > 44.5, "curve against face: x {lo}..{hi}");

    // Curves on parallel sketches have no crossing curve: refused, saying why.
    run(&mut s, "sketch.create", json!({"plane": "XY", "offset": 10, "name": "Above"}));
    let above = ids(&run(&mut s, "sketch.line", json!({"points": [[0, 0], [40, 10]]}))["curves"])[0].clone();
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    let e =
        s.execute("sketch.intersection_curve", &json!({"a": {"sketch": "Top", "curve": top}, "b": {"sketch": "Above", "curve": above}})).unwrap_err();
    assert!(e.to_string().contains("parallel"), "{e}");
}

#[test]
fn isoparametric_curves_of_a_cylinder() {
    let mut s = Session::default();
    run(&mut s, "solid.cylinder", json!({"base": [0, 0, 0], "radius": 6, "height": 20}));
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    // u: around (a circle at z = 10); v: along (a line from z 0 to 20).
    let r = run(&mut s, "sketch.isoparametric_curve", json!({"face": [6, 0, 10]}));
    let st = s.model.state();
    let sk = &st.sketch(s.active_sketch.unwrap()).unwrap().sketch;
    let w = &sk.wires[sk.wire_index(r["wires"][0].as_str().unwrap()).unwrap()];
    assert!(w.pts.iter().all(|p| (p.z - 10.0).abs() < 1e-6 && ((p.x * p.x + p.y * p.y).sqrt() - 6.0).abs() < 1e-3), "{:?}", &w.pts[..3]);
    let r = run(&mut s, "sketch.isoparametric_curve", json!({"face": [6, 0, 10], "direction": "v"}));
    let st = s.model.state();
    let sk = &st.sketch(s.active_sketch.unwrap()).unwrap().sketch;
    let w = &sk.wires[sk.wire_index(r["wires"][0].as_str().unwrap()).unwrap()];
    let zs: Vec<f64> = w.pts.iter().map(|p| p.z).collect();
    assert!(zs.iter().cloned().fold(f64::MIN, f64::max) > 19.9 && zs.iter().cloned().fold(f64::MAX, f64::min) < 0.1, "{zs:?}");
    // Within the mesh's sag: the cut runs across facets unless a seam happens to lie there.
    assert!(w.pts.iter().all(|p| (p.x - 6.0).abs() < 1e-2 && p.y.abs() < 1e-2), "{:?}", w.pts);
}

#[test]
fn tilted_circles_project_to_exact_ellipses_and_splines_stay_splines() {
    let mut s = Session::default();
    run(&mut s, "solid.cylinder", json!({"base": [0, 0, 0], "axis": [1, 0, 1], "radius": 5, "height": 20}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "offset": -10}));
    // The base circle (centred at the origin) seen from above.
    let r = run(&mut s, "sketch.project", json!({"refs": [{"edge": [0, 5, 0]}]}));
    // The rim is one closed circle edge: an exact ellipse, x semi-axis 5·cos 45° (the tilt), y 5.
    let si = inspect(&mut s);
    let c = si["curves"].as_array().unwrap().iter().find(|c| c["id"] == r["links"][0]["curves"][0]).unwrap().clone();
    assert_eq!(c["type"], "ellipse", "{c}");
    assert!((c["minor_radius"].as_f64().unwrap() - 5.0 * std::f64::consts::FRAC_1_SQRT_2).abs() < 1e-9, "{c}");
    let st = s.model.state();
    let sk = &st.sketch(s.active_sketch.unwrap()).unwrap().sketch;
    for ci in sk.link_curves(r["links"][0]["link"].as_str().unwrap()) {
        for q in sk.polyline(ci) {
            // On the ellipse x²/(5/√2)² + y²/5² = 1.
            let e = (q.x / (5.0 * std::f64::consts::FRAC_1_SQRT_2)).powi(2) + (q.y / 5.0).powi(2);
            assert!((e - 1.0).abs() < 1e-9, "{q:?}");
        }
    }
    // The whole face (its rim, plus its seam) gives the full ellipse.
    let r = run(&mut s, "sketch.project", json!({"refs": [{"face": [0, 0, 0]}]}));
    let si = inspect(&mut s);
    let types: Vec<String> = r["links"][0]["curves"]
        .as_array()
        .unwrap()
        .iter()
        .map(|id| si["curves"].as_array().unwrap().iter().find(|c| c["id"] == *id).unwrap()["type"].as_str().unwrap().to_string())
        .collect();
    assert!(types.iter().all(|t| t == "conic" || t == "ellipse"), "{types:?}");
    let r = run(&mut s, "sketch.project", json!({"refs": [{"edge": [0, -5, 0]}]}));
    let si = inspect(&mut s);
    let c = si["curves"].as_array().unwrap().iter().find(|c| c["id"] == r["links"][0]["curves"][0]).unwrap().clone();
    if c["type"] == "ellipse" {
        assert!((c["minor_radius"].as_f64().unwrap() - 5.0 * std::f64::consts::FRAC_1_SQRT_2).abs() < 1e-6, "{c}");
        let major = &c["major_at"];
        assert!(((major[0].as_f64().unwrap().powi(2) + major[1].as_f64().unwrap().powi(2)).sqrt() - 5.0).abs() < 1e-6, "{c}");
    }
    run(&mut s, "sketch.finish", json!({}));
    // A spline of a parallel sketch projects as the same spline.
    run(&mut s, "sketch.create", json!({"plane": "XY", "offset": 30}));
    let sp = run(&mut s, "sketch.spline.fit_point", json!({"points": [[0, 0], [10, 5], [20, 0]]}))["curves"][0].clone();
    run(&mut s, "sketch.finish", json!({}));
    let sk1 = s.doc.features.last().unwrap().id;
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    let r = run(&mut s, "sketch.project", json!({"refs": [{"sketch": sk1, "curve": sp}]}));
    let si = inspect(&mut s);
    let c = si["curves"].as_array().unwrap().iter().find(|c| c["id"] == r["links"][0]["curves"][0]).unwrap().clone();
    assert_eq!(c["type"], "spline", "{c}");
    assert_eq!(c["points"].as_array().unwrap().len(), 3);
}

#[test]
fn sweeps_and_pipes_follow_3d_sketch_curves() {
    // A line projected onto a sphere: a 3D arc on its top.
    let mut s = Session::default();
    run(&mut s, "solid.sphere", json!({"center": [0, 0, 0], "radius": 20}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "offset": 50}));
    let l = ids(&run(&mut s, "sketch.line", json!({"points": [[-12, 0], [12, 0]]}))["curves"])[0].clone();
    run(&mut s, "sketch.finish", json!({}));
    let src = s.doc.features.last().unwrap().id;
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    let r = run(&mut s, "sketch.project_to_surface", json!({"curves": [{"sketch": src, "curve": l}], "face": [0, 0, 20]}));
    let wid = r["links"][0]["wires"][0].as_str().unwrap().to_string();
    let path_sketch = s.active_sketch.unwrap();
    let pts = {
        let st = s.model.state();
        let sk = &st.sketch(path_sketch).unwrap().sketch;
        sk.wires[sk.wire_index(&wid).unwrap()].pts.clone()
    };
    run(&mut s, "sketch.finish", json!({}));
    let len: f64 = pts.windows(2).map(|w| w[0].dist(w[1])).sum();
    // A pipe along it: volume ≈ π r² × length.
    let before = s.model.state().bodies.len();
    run(&mut s, "solid.pipe", json!({"path_sketch": path_sketch, "path": [wid.clone()], "diameter": "2 mm", "operation": "new"}));
    let st = s.model.state();
    assert_eq!(st.bodies.len(), before + 1);
    let v = st.bodies.last().unwrap().mesh().measure().volume;
    let want = std::f64::consts::PI * len;
    assert!((v - want).abs() < 0.05 * want, "{v} vs {want}");
    drop(st);
    // A sweep of a square profile drawn square to the curve at its start.
    let (a, b) = (pts[0], pts[1]);
    let t = (b - a).normalized().unwrap();
    let x = t.cross(solidcore_record::Vec3::Z).normalized().unwrap();
    let y = t.cross(x);
    run(&mut s, "sketch.create", json!({"plane": {"origin": [a.x, a.y, a.z], "x_dir": [x.x, x.y, x.z], "y_dir": [y.x, y.y, y.z]}}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [-1, -1], "p1": [1, 1]}));
    let prof = s.active_sketch.unwrap();
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.sweep", json!({"sketch": prof, "path_sketch": path_sketch, "path": [wid], "operation": "new"}));
    let st = s.model.state();
    assert_eq!(st.bodies.len(), before + 2);
    let v = st.bodies.last().unwrap().mesh().measure().volume;
    assert!((v - 4.0 * len).abs() < 0.05 * 4.0 * len, "{v} vs {}", 4.0 * len);
}

/// #47: references to other bodies: an edge, a face's loops and a silhouette of a second body
/// project into a sketch on the first; they follow an edit of that body (a parameter), and when
/// it goes away the sketch fails, asking for the named edge again, not crashed on.
#[test]
fn project_and_include_from_other_bodies() {
    let mut s = Session::default();
    run(&mut s, "parameters.change", json!({"name": "w", "expression": "10 mm"}));
    run(&mut s, "solid.box", json!({"corner": [0, 0, 0], "length": 40, "width": 30, "height": 5}));
    run(&mut s, "solid.box", json!({"corner": [50, 0, 0], "length": "w", "width": 10, "height": 20, "operation": "new"}));
    run(&mut s, "solid.cylinder", json!({"base": [80, 5, 0], "radius": 4, "height": 10, "operation": "new"}));
    run(&mut s, "sketch.create", json!({"plane": {"face": [20, 15, 5]}, "project_edges": false}));
    // An edge on the second box's top, its top face's loop, the cylinder's silhouette.
    run(&mut s, "sketch.project", json!({"refs": [{"edge": [55, 0, 20]}]}));
    run(&mut s, "sketch.project", json!({"refs": [{"face": [55, 5, 20]}]}));
    run(&mut s, "sketch.include_3d", json!({"refs": [{"edge": [80, 1, 10]}]}));
    run(&mut s, "sketch.project", json!({"refs": [{"body": "Body3"}]}));
    let si = inspect(&mut s);
    let lc = linked_curves(&si);
    // (Geometry already there is not drawn twice: the edge lies on the face's loop, the included
    // circle on the silhouette.)
    assert!(lc.len() >= 6, "{}", lc.len());
    // The projected edge of the second box runs from x 50 to 60 at y 0.
    let xs = |si: &Value| -> Vec<f64> {
        linked_curves(si).iter().flat_map(|c| [c["start_at"][0].as_f64().unwrap_or(0.0), c["end_at"][0].as_f64().unwrap_or(0.0)]).collect()
    };
    assert!(xs(&si).iter().any(|x| (x - 60.0).abs() < 1e-6), "{:?}", xs(&si));
    run(&mut s, "sketch.finish", json!({}));
    // Widen the second box: the projection follows (its far edge is at 70).
    run(&mut s, "parameters.change", json!({"name": "w", "expression": "20 mm"}));
    let id = s.doc.features.iter().find(|f| matches!(f.kind, solidcraft_doc::FeatureKind::Sketch { .. })).unwrap().id;
    let si = run(&mut s, "sketch.inspect", json!({"sketch": id}));
    assert!(xs(&si).iter().any(|x| (x - 70.0).abs() < 1e-6), "{:?}", xs(&si));
    // Remove the cylinder: the edge included from it is gone, and the sketch asks for it again.
    let cyl = s.doc.features.iter().find(|f| matches!(f.kind, solidcraft_doc::FeatureKind::Cylinder { .. })).unwrap().id;
    run(&mut s, "timeline.delete", json!({"feature": cyl}));
    let t = run(&mut s, "document.inspect", json!({}));
    let row = t["timeline"].as_array().unwrap().iter().find(|r| r["id"] == json!(id)).cloned().unwrap();
    let error = row["error"].as_str().unwrap_or_default();
    assert!(
        solidcraft_doc::reselect_parts(error)
            .is_some_and(|(feature, reference)| feature == row["name"] && reference.starts_with(&format!("F{cyl}:"))),
        "{row}"
    );
}

/// A cylinder's wall and the torus of a rounded rim are drawn as one face but stored in halves
/// joined by hidden seams; projecting one projects the whole face, not an open half.
#[test]
fn faces_split_by_hidden_seams_project_whole() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane":"XY"}));
    run(&mut s, "sketch.circle.center", json!({"center":[0,0],"radius":20}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"distance":10}));
    run(&mut s, "sketch.create", json!({"plane":"XZ"}));
    run(&mut s, "sketch.project", json!({"refs":[{"face":[20,0,5]}]}));
    let wall = run(&mut s, "sketch.inspect", json!({}));
    let heights: Vec<f64> =
        wall["curves"].as_array().unwrap().iter().flat_map(|c| [c["start_at"][1].as_f64().unwrap(), c["end_at"][1].as_f64().unwrap()]).collect();
    assert!(heights.iter().any(|y| y.abs() < 1e-6) && heights.iter().any(|y| (y.abs() - 10.).abs() < 1e-6), "{wall}");
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.fillet", json!({"edges":[[20,0,10]],"radius":3}));
    run(&mut s, "sketch.create", json!({"plane":"XY"}));
    let t = 0.5f64.sqrt() * 3.;
    run(&mut s, "sketch.project", json!({"refs":[{"face":[17. + t, 0, 7. + t]}]}));
    let round = run(&mut s, "sketch.inspect", json!({}));
    let mut radii: Vec<f64> = round["curves"].as_array().unwrap().iter().map(|c| c["radius"].as_f64().unwrap()).collect();
    radii.sort_by(f64::total_cmp);
    assert_eq!(radii.len(), 4, "{round}");
    assert!(radii[..2].iter().all(|r| (r - 17.).abs() < 1e-6) && radii[2..].iter().all(|r| (r - 20.).abs() < 1e-6), "{round}");
}

/// #635: a rectangle whose corner is made coincident with a projected edge of the solid under
/// the sketch, so that its bottom side lies on that edge, is still one closed profile (the
/// projected line stays, and names the shared edge as it was there first), and it extrudes to
/// its own volume, also when the design is opened again.
#[test]
fn a_rectangle_put_onto_a_projected_edge_stays_a_closed_profile() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [40, 30]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"distance": 20}));
    let block = s.model.state().bodies[0].name.clone();
    let id = run(&mut s, "sketch.create", json!({"plane": "XY"}))["sketch"].as_u64().unwrap();
    // The solid's bottom edge along X, from (0, 0) to (40, 0).
    let r = run(&mut s, "sketch.project", json!({"refs": [{"edge": [20, 0, 0]}]}));
    let projected = r["links"][0]["curves"][0].as_str().unwrap().to_string();
    // A 20 x 15 rectangle clear of it is a closed profile of its own.
    let sides = ids(&run(&mut s, "sketch.rectangle.two_point", json!({"p0": [5, 10], "p1": [25, 25]}))["curves"]);
    run(&mut s, "sketch.dimension", json!({"entities": [sides[0]], "value": 20}));
    run(&mut s, "sketch.dimension", json!({"entities": [sides[1]], "value": 15}));
    let mut own = sides.clone();
    own.sort();
    let profile_curves = |si: &Value| -> Vec<Vec<String>> {
        si["profiles"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| {
                let mut c = ids(&p["curves"]);
                c.sort();
                c
            })
            .collect()
    };
    assert_eq!(profile_curves(&inspect(&mut s)), vec![own.clone()]);
    // Its bottom-left corner onto the projected line: the bottom side lies on it.
    run(&mut s, "sketch.constraint.coincident", json!({"a": format!("{}.start", sides[0]), "b": projected}));
    let si = inspect(&mut s);
    assert_eq!(si["status"], "solved", "{si}");
    let curve = |id: &str| si["curves"].as_array().unwrap().iter().find(|c| c["id"] == id).unwrap().clone();
    let bottom = curve(&sides[0]);
    for end in ["start_at", "end_at"] {
        assert!(bottom[end][1].as_f64().unwrap().abs() < 1e-9, "{bottom}");
    }
    assert!(curve(&projected).get("link").is_some(), "the projected line stays");
    let mut on_line = vec![projected.clone(), sides[1].clone(), sides[2].clone(), sides[3].clone()];
    on_line.sort();
    assert_eq!(profile_curves(&si), vec![on_line], "{si}");
    assert!((si["profiles"][0]["area_mm2"].as_f64().unwrap() - 300.0).abs() < 1e-6, "{si}");
    // Picked by a point inside, it extrudes to a 20 x 15 x 10 block of its own.
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"sketch": id, "profiles": [{"point": [15, 7.5]}], "distance": 10, "operation": "new", "body_name": "Pad"}));
    let volume = |s: &Session, name: &str| {
        s.model.state().body(name).unwrap().body.mesh(&solidcraft_kernel::MeshTolerance::chord(1e-4)).unwrap().measure().volume
    };
    assert!((volume(&s, "Pad") - 3000.0).abs() < 1e-6, "{}", volume(&s, "Pad"));
    assert!((volume(&s, &block) - 24000.0).abs() < 1e-6, "{}", volume(&s, &block));
    let reopened = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
    assert!((volume(&reopened, "Pad") - 3000.0).abs() < 1e-6, "{}", volume(&reopened, "Pad"));
}

/// A face with letters cut into it, extruded as a cut: its edges come into the face's sketch
/// as the letters' exact curves (a font's quadratics and cubics), not fitted to the mesh, so the
/// tool's walls lie on the letters' walls and the cut takes what the same region picked in the
/// sketch takes (#1360). Fitted to the mesh (684 lines, 31 arcs and 18 splines here), they lay
/// a few hundredths off the letters' walls, and the boolean found no result.
#[test]
fn a_face_engraved_with_text_projects_its_letters_exactly_and_extrudes() {
    let volume = |s: &mut Session| run(s, "inspect.measure", json!({}))["total"]["volume_mm3"].as_f64().unwrap();
    let engraved = || {
        let mut s = Session::default();
        run(&mut s, "sketch.create", json!({"plane":"XY"}));
        run(&mut s, "sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[105,26]}));
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "solid.extrude", json!({"distance":40}));
        run(&mut s, "sketch.create", json!({"plane":{"face":[50,13,40]}}));
        run(&mut s, "sketch.text", json!({"text":"textvraiment tres long","at":[3,9],"height":6.5}));
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "solid.extrude", json!({"distance":-1,"operation":"cut"}));
        s
    };
    // The face round the letters, drawn in a sketch in its plane.
    let mut drawn = engraved();
    let before = volume(&mut drawn);
    run(&mut drawn, "sketch.create", json!({"plane":"XY","offset":40}));
    run(&mut drawn, "sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[105,26]}));
    run(&mut drawn, "sketch.text", json!({"text":"textvraiment tres long","at":[3,9],"height":6.5}));
    run(&mut drawn, "sketch.finish", json!({}));
    run(&mut drawn, "solid.extrude", json!({"distance":-3.5,"operation":"cut","profiles":[{"point":[1,1]}]}));
    let taken = before - volume(&mut drawn);
    assert!(taken > 0.0, "{taken}");
    // The engraved face itself, extruded as a cut.
    let mut s = engraved();
    let id = run(&mut s, "solid.extrude", json!({"distance":-3.5,"operation":"cut","face":[1,1,40]}))["feature"].as_u64().unwrap();
    let r = s.model.result(id).unwrap();
    assert!(r.error.is_none(), "{r:?}");
    let face = before - volume(&mut s);
    assert!((face - taken).abs() < 1e-6 * before, "{face} {taken}");
    // Its sketch got the letters as their exact pieces: the outline's quadratic Béziers, which are
    // parabolic arcs, come in as conics (or as control-point splines).
    let st = s.model.state();
    let sketch = &st.sketches.last().unwrap().sketch;
    let pieces = sketch
        .curves
        .iter()
        .filter(|c| {
            c.link.is_some()
                && matches!(c.kind, solidcraft_sketch::CurveKind::Spline { control: true, .. } | solidcraft_sketch::CurveKind::Conic { .. })
        })
        .count();
    assert!(pieces > 100, "{pieces}");
}

/// A design saved before free-form edges came in exactly, by the build before it: an "S" cut
/// 1 mm into a 30 × 20 × 10 block, then a sketch on that face (its edges projected, the letter's
/// fitted to the mesh as 72 lines, 9 arcs and 2 splines) with a circle cut 3 mm. On reopening, the
/// face's link is laid out again with the letter's exact pieces, and the part builds the same.
#[test]
fn a_face_sketch_saved_with_fitted_letters_reopens_and_builds_the_same() {
    let text = include_str!("../../../../tests/projection/before-1508/engraved_face_sketched.json");
    let mut s = Session::new(solidcraft_doc::Document::from_json(text).unwrap());
    let volume = run(&mut s, "inspect.measure", json!({}))["total"]["volume_mm3"].as_f64().unwrap();
    for name in ["Extrude2", "Sketch3", "Extrude3"] {
        let id = s.doc.find_feature(name).unwrap().id;
        let r = s.model.result(id).unwrap();
        assert!(r.error.is_none(), "{name}: {:?}", r.error);
    }
    // The 30 x 20 x 10 block less the "S" 1 deep (its profile's exact area, Sketch2's smaller
    // region) and the r 2 circle 3 deep, clear of the letter: 5916.010767. (The build that saved
    // it measured 5916.018831, 8e-3 off; SMLib's integral is within 1e-8 of the closed form.)
    let letter = s.model.state().sketches.iter().find(|k| k.name == "Sketch2").unwrap().profiles.iter().map(|p| p.area).fold(f64::INFINITY, f64::min);
    let want = 6000.0 - letter - 3.0 * std::f64::consts::PI * 4.0;
    assert!((volume - want).abs() < 1e-8 * 6000.0, "{volume} vs {want}");
    // The letter's exact pieces in the face's link: its quadratic Béziers, as conics (or splines).
    let st = s.model.state();
    let sketch = &st.sketches.last().unwrap().sketch;
    let pieces = sketch
        .curves
        .iter()
        .filter(|c| {
            c.link.is_some()
                && matches!(c.kind, solidcraft_sketch::CurveKind::Spline { control: true, .. } | solidcraft_sketch::CurveKind::Conic { .. })
        })
        .count();
    assert!(pieces > 0, "{pieces}");
}

#[test]
fn a_face_seen_edge_on_projects_as_its_silhouette_and_follows_edits() {
    let mut s = Session::default();
    box_part(&mut s);
    run(&mut s, "sketch.create", json!({"plane": "XY", "offset": 30}));
    // The +x wall of the 40 x 30 x 20 box, seen from above: one line from (40, 0) to (40, 30).
    let r = run(&mut s, "sketch.project", json!({"refs": [{"face": [40, 15, 10]}]}));
    let link = r["links"][0]["link"].as_str().unwrap().to_string();
    assert_eq!(ids(&r["links"][0]["curves"]).len(), 1, "{r}");
    let si = inspect(&mut s);
    let c = linked_curves(&si).into_iter().find(|c| c["link"] == link.as_str()).unwrap();
    let xs: Vec<f64> = [&c["start_at"][0], &c["end_at"][0]].iter().map(|v| v.as_f64().unwrap()).collect();
    assert!(xs.iter().all(|x| (x - 40.0).abs() < 1e-6), "{c}");
    run(&mut s, "sketch.finish", json!({}));
    let sketch = s.doc.features.last().unwrap().id;
    let lost = |s: &Session| s.model.state().sketch(sketch).unwrap().sketch.link(&link).unwrap().lost;
    assert!(!lost(&s), "the silhouette is current after the sketch is evaluated");
    assert!(
        s.model.results.iter().all(|r| r.warning.is_none()),
        "{:?}",
        s.model.results.iter().filter_map(|r| r.warning.clone()).collect::<Vec<_>>()
    );
    // The wall follows the width.
    run(&mut s, "parameters.change", json!({"name": "width", "expression": "50 mm"}));
    assert!(!lost(&s), "{:?}", s.model.results.iter().filter_map(|r| r.warning.clone()).collect::<Vec<_>>());
    let st = s.model.state();
    let sk = &st.sketch(sketch).unwrap().sketch;
    let ci = sk.link_curves(&link)[0];
    let solidcraft_sketch::CurveKind::Line { a, b } = sk.curves[ci].kind else { panic!("not a line") };
    assert!((sk.point(a).unwrap().x - 50.0).abs() < 1e-6 && (sk.point(b).unwrap().x - 50.0).abs() < 1e-6);
}

/// A sketch put on a face already shows the face's border. Projecting that face, or one of the
/// border's edges, again laid a second copy over it (extra points and lines on a rounded
/// corner); those are skipped now, and a pick of nothing new is refused, so it adds no step.
#[test]
fn projecting_what_the_face_sketch_already_shows_adds_no_second_copy() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length":40,"width":30,"height":10}));
    run(&mut s, "solid.fillet", json!({"edges":[[0,0,5],[40,0,5],[40,30,5],[0,30,5]],"radius":5}));
    run(&mut s, "sketch.create", json!({"plane":{"face":[20,15,10]}}));
    let shown = |s: &mut Session| {
        let si = inspect(s);
        (si["curves"].as_array().unwrap().len(), si["points"].as_array().unwrap().len())
    };
    let border = shown(&mut s);
    assert_eq!(border.0, 8, "four sides and four rounded corners");
    let corner = 5.0 - 5.0 * std::f64::consts::FRAC_1_SQRT_2;
    // These references name no body, so the refusal names the kind alone: it must not end in
    // "face of " with nothing after the "of" (#4929).
    let cases = [(json!([{"face":[20,15,10]}]), "face"), (json!([{"edge":[20,0,10]}]), "edge"), (json!([{"edge":[corner,corner,10]}]), "edge")];
    for (refs, what) in cases {
        let e = s.execute("sketch.project", &json!({"refs": refs})).unwrap_err().to_string();
        assert!(e.contains("already projected"), "{refs}: {e}");
        assert!(e.ends_with(&format!("already projected into this sketch: {what}")), "{refs}: {e}");
        assert_eq!(shown(&mut s), border, "{refs}");
    }
    // Something new still projects, and alongside it a reference already shown is skipped.
    let r = run(&mut s, "sketch.project", json!({"refs":[{"edge":[20,0,10]},{"axis":"X"}]}));
    assert_eq!(r["links"].as_array().unwrap().len(), 1, "{r}");
    assert_eq!(r["already_projected"].as_array().unwrap().len(), 1, "{r}");
    assert_eq!(shown(&mut s).0, border.0 + 1);
}

/// Intersecting what was intersected before laid another point on the first each time (one spot
/// held 11 points): the same curve or edge again is refused, as Project refuses what the sketch
/// already shows, and something new alongside it still goes in.
#[test]
fn intersecting_the_same_curve_or_edge_again_adds_no_second_point() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length":40,"width":30,"height":20,"body_name":"Post"}));
    let rectangle = run(&mut s, "sketch.create", json!({"plane":"XY"}))["sketch"].as_u64().unwrap();
    run(&mut s, "sketch.rectangle.two_point", json!({"p0":[0,-10],"p1":[40,-5]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "sketch.create", json!({"plane":"YZ","offset":20}));
    let points = |s: &mut Session| inspect(s)["points"].as_array().unwrap().len();
    for refs in [json!([{"sketch":rectangle,"curve":"l1"}]), json!([{"edge":[20,0,20],"body":"Post"}])] {
        run(&mut s, "sketch.intersect", json!({"refs": refs}));
        let once = points(&mut s);
        let e = s.execute("sketch.intersect", &json!({"refs": refs})).unwrap_err().to_string();
        assert!(e.contains("already intersected into this sketch"), "{refs}: {e}");
        assert_eq!(points(&mut s), once, "{refs}");
    }
    let r = run(&mut s, "sketch.intersect", json!({"refs":[{"sketch":rectangle,"curve":"l1"},{"edge":[20,30,20],"body":"Post"}]}));
    assert_eq!(r["links"].as_array().unwrap().len(), 1, "{r}");
    assert_eq!(r["already_projected"].as_array().unwrap().len(), 1, "{r}");
}

/// A sketch on a face shows the face's border. An edge of another body that crosses the sketch
/// plane at one of the face's corners lands on the end of a border line, which is not a point an
/// earlier Intersect left: the cut goes in once, and only a second cut of the same edge is refused.
#[test]
fn intersecting_an_edge_at_a_face_corner_adds_its_point() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length":40,"width":30,"height":10,"body_name":"Plate"}));
    run(&mut s, "solid.box", json!({"length":10,"width":10,"height":20,"corner":[40,30,0],"body_name":"Post","operation":"new"}));
    run(&mut s, "sketch.create", json!({"plane":{"face":[20,15,10]}}));
    let points = |s: &mut Session| inspect(s)["points"].as_array().unwrap().len();
    let border = points(&mut s);
    let refs = json!([{"edge":[40,30,15],"body":"Post"}]);
    let r = s.execute("sketch.intersect", &json!({"refs": refs})).map_err(|e| e.to_string());
    assert!(r.is_ok(), "{r:?}");
    assert_eq!(points(&mut s), border + 1);
    let e = s.execute("sketch.intersect", &json!({"refs": refs})).unwrap_err().to_string();
    assert!(e.contains("already intersected into this sketch"), "{e}");
    assert_eq!(points(&mut s), border + 1);
}

/// The curves sketch Intersect makes, on `plane`, of the first body `steps` build: each line's ends,
/// each circle's centre and radius.
fn intersect_curves(steps: &[(&str, Value)], plane: Value) -> std::result::Result<Vec<Value>, String> {
    let mut s = Session::default();
    for (cmd, p) in steps {
        run(&mut s, cmd, p.clone());
    }
    run(&mut s, "sketch.create", plane);
    let body = s.model.state().bodies[0].name.clone();
    s.execute("sketch.intersect", &json!({"refs": [{"body": body}]})).map_err(|e| e.to_string())?;
    Ok(linked_curves(&inspect(&mut s)))
}

/// The lines among `curves` as an outline: each line's ends rounded and the lower first, the lines
/// sorted. A section's chains are walked from either end (the mesh's triangle order picks which),
/// so a line's direction says nothing about the cut.
fn outline(curves: &[Value]) -> Vec<[f64; 4]> {
    let mut ends: Vec<[f64; 4]> = line_ends(curves)
        .into_iter()
        .map(|e| {
            let e = e.map(f64::round);
            if [e[2], e[3]] < [e[0], e[1]] { [e[2], e[3], e[0], e[1]] } else { e }
        })
        .collect();
    ends.sort_by(|a, b| a.partial_cmp(b).unwrap());
    ends
}

fn line_ends(curves: &[Value]) -> Vec<[f64; 4]> {
    let at = |c: &Value, k: &str, i: usize| c[k][i].as_f64().unwrap_or(f64::NAN);
    curves
        .iter()
        .filter(|c| c["type"] == "line")
        .map(|c| [at(c, "start_at", 0), at(c, "start_at", 1), at(c, "end_at", 0), at(c, "end_at", 1)])
        .collect()
}

/// Where Intersect's cut is already exact it stays as it is: a box's outline (through it and on
/// its bottom face), a cylinder's and a sphere's circle, a tilted cylinder's 21 segments.
#[test]
fn intersect_keeps_its_exact_cuts() {
    let block = [("solid.box", json!({"length": 40, "width": 30, "height": 20}))];
    for offset in [10, 0] {
        let ends = outline(&intersect_curves(&block, json!({"plane": "XY", "offset": offset})).unwrap());
        assert_eq!(ends, vec![[0.0, 0.0, 0.0, 30.0], [0.0, 0.0, 40.0, 0.0], [0.0, 30.0, 40.0, 30.0], [40.0, 0.0, 40.0, 30.0]], "offset {offset}");
    }
    for (steps, plane, radius) in [
        (vec![("solid.cylinder", json!({"base": [0, 0, 0], "radius": 7, "height": 30}))], json!({"plane": "XY", "offset": 12}), 7.0),
        (vec![("solid.sphere", json!({"center": [0, 0, 0], "radius": 10}))], json!({"plane": "XY", "offset": 4}), 84f64.sqrt()),
    ] {
        let c = intersect_curves(&steps, plane).unwrap();
        assert!(c.len() == 1 && c[0]["type"] == "circle" && (c[0]["radius"].as_f64().unwrap() - radius).abs() < 1e-4, "{c:?}");
    }
    // The tilted cylinder's cut is an ellipse the section leaves as the mesh's segments, as many
    // as the tessellation crosses (21 on truck's mesh, 20 on SMLib's): each end on the cylinder
    // within the section's own reach of the surface, the display tolerance (a thousandth of the
    // mesh's size, doc::project::fit_segments), which the old pin of one end to 1e-4 assumed of
    // one mesh.
    let tilted = [("solid.cylinder", json!({"base": [0, 0, 0], "radius": 5, "height": 40, "axis": [1, 0, 1]}))];
    let curves = intersect_curves(&tilted, json!({"plane": "XY", "offset": 10})).unwrap();
    let ends = line_ends(&curves);
    assert!(ends.len() == curves.len() && ends.len() >= 16, "{curves:?}");
    let axis = solidcore_record::Vec3::new(1.0, 0.0, 1.0).normalized().unwrap();
    // The cylinder's box: its axis 40 along (1, 0, 1)/√2 and its ends' discs of radius 5 span
    // (40 + 2·5)/√2 in x and z and 10 in y.
    let span = 50.0 * std::f64::consts::FRAC_1_SQRT_2;
    let reach = 1e-3 * (2.0 * span * span + 100.0).sqrt();
    for e in &ends {
        for p in [solidcore_record::Vec3::new(e[0], e[1], 10.0), solidcore_record::Vec3::new(e[2], e[3], 10.0)] {
            assert!(((p - axis * p.dot(axis)).len() - 5.0).abs() < reach, "{p:?}");
        }
    }
}

/// A plane crossing a curved face's rim cuts it on the surface, not on the mesh's chord: a
/// cylinder's sides lie √40 from its axis, a hole's walls its radius from its centre (they were
/// 6.3159 and 6.3085, 25.0122 and 34.9878).
#[test]
fn intersect_crosses_a_curved_face_on_the_surface() {
    let side = (40f64).sqrt();
    let rod = [("solid.cylinder", json!({"base": [0, 0, 0], "radius": 7, "height": 30}))];
    let xs: Vec<f64> = line_ends(&intersect_curves(&rod, json!({"plane": "XZ", "offset": 3})).unwrap()).iter().flat_map(|e| [e[0], e[2]]).collect();
    assert!(xs.iter().all(|x| (x.abs() - side).abs() < 1e-4 || x.abs() < 1e-9), "{xs:?}");
    assert!(xs.iter().any(|x| (x + side).abs() < 1e-4) && xs.iter().any(|x| (x - side).abs() < 1e-4), "{xs:?}");
    let plate = [("solid.box", json!({"length": 60, "width": 40, "height": 5})), ("solid.hole", json!({"position": [30, 20, 5], "diameter": 10}))];
    let xs: Vec<f64> =
        line_ends(&intersect_curves(&plate, json!({"plane": "XZ", "offset": 20})).unwrap()).iter().flat_map(|e| [e[0], e[2]]).collect();
    for wall in [25.0, 35.0] {
        assert!(xs.iter().any(|x| (x - wall).abs() < 1e-4), "{wall}: {xs:?}");
    }
    assert!(xs.iter().all(|x| [0.0, 25.0, 35.0, 60.0].iter().any(|w| (x - w).abs() < 1e-4)), "{xs:?}");
}

/// A plane on a body's top face cuts it there, as one on its bottom face does (it found nothing:
/// "projects to nothing on this sketch plane").
#[test]
fn intersect_on_a_top_face_gives_its_outline() {
    let block = [("solid.box", json!({"length": 40, "width": 30, "height": 20}))];
    let ends = outline(&intersect_curves(&block, json!({"plane": "XY", "offset": 20})).unwrap());
    assert_eq!(ends, vec![[0.0, 0.0, 0.0, 30.0], [0.0, 0.0, 40.0, 0.0], [0.0, 30.0, 40.0, 30.0], [40.0, 0.0, 40.0, 30.0]]);
}

/// A free-form edge projects as the one exact spline it is, its knots and weights its own: the
/// bottom face of a 40 x 20 x 10 box whose upright edges have G2 rounds (each round's bottom
/// edge a B-spline of several spans) comes into its sketch as four lines and four splines, one a
/// corner, each meeting the lines at their ends (#3604, where each span was a curve of its own).
/// The profile they bound, extruded the box's height, is the box: its sides are upright.
#[test]
fn a_free_form_edge_projects_as_one_spline_with_its_own_knots() {
    use solidcraft_sketch::CurveKind;
    let volume = |s: &Session, body: &str| solidcraft_kernel::measure(&s.world_state().unwrap().body(body).unwrap().body).unwrap().volume;
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"corner":[0,0,0],"length":40,"width":20,"height":10,"body_name":"B"}));
    run(&mut s, "solid.fillet", json!({"body":"B","edges":[[0,0,5],[40,0,5],[40,20,5],[0,20,5]],"radius":5,"type":"g2"}));
    let id = run(&mut s, "sketch.create", json!({"plane":{"face":[20,10,0]},"project_edges":true,"name":"Bottom"}))["sketch"].as_u64().unwrap();
    run(&mut s, "sketch.finish", json!({}));
    let st = s.model.state();
    let sketch = &st.sketch(id).unwrap().sketch;
    let lines = sketch.curves.iter().filter(|c| matches!(c.kind, CurveKind::Line { .. })).count();
    let splines: Vec<_> = sketch.curves.iter().filter(|c| matches!(c.kind, CurveKind::Spline { .. })).collect();
    assert_eq!((lines, splines.len(), sketch.curves.len()), (4, 4, 8), "{:?}", sketch.curves);
    for c in &splines {
        let CurveKind::Spline { pts, control: true, nurbs: Some(nurbs), .. } = &c.kind else { panic!("{:?}", c.kind) };
        assert_eq!(nurbs.knots.len(), pts.len() + 4, "a cubic's knots");
        let (a, b) = c.kind.ends().unwrap();
        for end in [a, b] {
            let meets = sketch.curves.iter().filter(|o| matches!(o.kind, CurveKind::Line { a, b } if a == end || b == end)).count();
            assert_eq!(meets, 1, "{} end {end}", c.id);
        }
    }
    drop(st);
    let rounded = volume(&s, "B");
    run(&mut s, "solid.extrude", json!({"sketch":"Bottom","distance":10,"operation":"new","body_name":"P"}));
    let prism = volume(&s, "P");
    assert!((prism - rounded).abs() < rounded * 1e-6, "{prism} vs {rounded}");
}

#[test]
fn intersect_whole_body_can_create_unlinked_section_geometry() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 40, "width": 30, "height": 20, "body_name": "Post"}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "offset": 10}));

    let result = run(&mut s, "sketch.intersect", json!({"refs": [{"body": "Post"}], "projection_link": false}));
    assert_eq!(result["links"][0]["link"], Value::Null, "{result}");
    assert_eq!(result["links"][0]["curves"].as_array().unwrap().len(), 4, "{result}");

    let sketch = s.doc.sketch(s.active_sketch.unwrap()).unwrap();
    assert!(sketch.links.is_empty(), "projection links were disabled");
    assert_eq!(sketch.curves.len(), 4, "the whole body's section remains as normal sketch geometry");
    assert!(s.execute("sketch.intersect", &json!({"refs": [{"body": "Post"}], "projection_link": "false"})).is_err());
}

#[test]
fn whole_body_project_preserves_the_arc_of_a_chamfered_round_edge() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 40, "width": 30, "height": 20, "body_name": "Plate"}));
    run(&mut s, "solid.fillet", json!({"edges": [[40, 0, 10]], "radius": 5}));
    let sketch = run(&mut s, "sketch.create", json!({"plane": {"face": [20, 15, 20]}, "project_edges": false}))["sketch"].as_u64().unwrap();
    run(&mut s, "sketch.project", json!({"refs": [{"body": "Plate"}]}));
    run(&mut s, "sketch.finish", json!({}));

    run(&mut s, "timeline.roll_to", json!({"position": 2}));
    run(&mut s, "solid.chamfer", json!({"edges": [[40, 15, 20]], "distance": 1}));
    run(&mut s, "timeline.roll_to", json!({}));

    let result = s.model.result(sketch).unwrap();
    let state = s.model.state();
    let solved = state.sketch(sketch).unwrap();
    let sketch = &solved.sketch;
    assert!(result.error.is_none(), "{:?}", result.error);
    assert!(result.warning.is_none(), "a rebuilt projection nothing depends on needs no warning: {:?}", result.warning);
    assert!((solved.plane.origin.z - 20.).abs() < 1e-6);
    assert!(
        sketch.curves.iter().any(|curve| matches!(curve.kind, solidcraft_sketch::CurveKind::Arc { .. })),
        "the rounded chamfer silhouette should contain an arc, not only line segments: {:?}",
        sketch.curves
    );
}

#[test]
fn whole_body_project_stays_on_the_body_outline_after_a_late_chamfer() {
    use solidcraft_sketch::CurveKind;
    let planes: [([i32; 3], &str); 3] = [([20, 15, 20], "top"), ([20, 0, 10], "front"), ([40, 15, 10], "side")];
    let chamfers: [[i32; 3]; 4] = [[40, 15, 20], [20, 0, 20], [40, 0, 10], [38, 2, 20]];
    for (face, plane) in planes {
        for chamfer in chamfers {
            let mut s = Session::default();
            run(&mut s, "solid.box", json!({"length": 40, "width": 30, "height": 20, "body_name": "Plate"}));
            run(&mut s, "solid.fillet", json!({"edges": [[40, 0, 10]], "radius": 5}));
            let sketch = run(&mut s, "sketch.create", json!({"plane": {"face": face}, "project_edges": false}))["sketch"].as_u64().unwrap();
            run(&mut s, "sketch.project", json!({"refs": [{"body": "Plate"}]}));
            run(&mut s, "sketch.finish", json!({}));
            run(&mut s, "timeline.roll_to", json!({"position": 2}));
            if s.execute("solid.chamfer", &json!({"edges": [chamfer], "distance": 1})).is_err() {
                continue;
            }
            run(&mut s, "timeline.roll_to", json!({}));
            let state = s.model.state();
            let solved = state.sketch(sketch).unwrap();
            let sk = &solved.sketch;
            let pts: Vec<_> = sk.points.iter().map(|p| p.pos).collect();
            let (x0, x1) = pts.iter().fold((f64::MAX, f64::MIN), |a, p| (a.0.min(p.x), a.1.max(p.x)));
            let (y0, y1) = pts.iter().fold((f64::MAX, f64::MIN), |a, p| (a.0.min(p.y), a.1.max(p.y)));
            let mut worst = 0.0_f64;
            for curve in &sk.curves {
                if let CurveKind::Arc { c, a, b } = curve.kind {
                    let (c, a, b) = (pts[c], pts[a], pts[b]);
                    let r = c.dist(a);
                    let (t0, mut sweep) = ((a - c).angle(), (b - c).angle() - (a - c).angle());
                    while sweep < 0.0 {
                        sweep += std::f64::consts::TAU;
                    }
                    for i in 0..=16 {
                        let t = t0 + sweep * f64::from(i) / 16.0;
                        let q = solidcore_record::Vec2::new(c.x + r * t.cos(), c.y + r * t.sin());
                        worst = worst.max((x0 - q.x).max(q.x - x1).max(y0 - q.y).max(q.y - y1));
                    }
                    assert!(sweep < std::f64::consts::PI + 0.01, "{plane} {chamfer:?}: arc sweeps {sweep} rad, not a rounded corner");
                }
            }
            assert!(worst < 0.01, "{plane} {chamfer:?}: an arc leaves the body outline by {worst}");
        }
    }
}

#[test]
fn whole_body_project_of_a_filleted_plate_keeps_exact_lines_and_arcs() {
    use solidcraft_sketch::CurveKind;
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 80, "width": 50, "height": 8, "body_name": "Plate"}));
    run(&mut s, "solid.fillet", json!({"edges": [[0, 0, 4], [80, 0, 4], [80, 50, 4], [0, 50, 4]], "radius": 6}));
    let sketch = run(&mut s, "sketch.create", json!({"plane": "YZ", "offset": 36, "project_edges": false}))["sketch"].as_u64().unwrap();
    run(&mut s, "sketch.project", json!({"refs": [{"body": "Plate"}]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "timeline.roll_to", json!({"position": 2}));
    let top = [[40., 0.], [1.7574, 1.7574], [78.2426, 1.7574], [80., 25.], [78.2426, 48.2426], [40., 50.], [1.7574, 48.2426], [0., 25.]];
    let edges: Vec<_> = top.iter().map(|p| json!([p[0], p[1], 8])).collect();
    run(&mut s, "solid.fillet", json!({"edges": edges, "radius": 2}));
    run(&mut s, "timeline.roll_to", json!({}));

    let state = s.model.state();
    let sk = &state.sketch(sketch).unwrap().sketch;
    let pos = |i: usize| sk.points[i].pos;
    let mut arcs = Vec::new();
    for curve in &sk.curves {
        match curve.kind {
            CurveKind::Line { a, b } => {
                let d = pos(b) - pos(a);
                assert!(d.x.abs() < 1e-3 || d.y.abs() < 1e-3, "slanted line {:?} -> {:?}", pos(a), pos(b));
            }
            CurveKind::Arc { c, a, .. } => arcs.push(pos(c).dist(pos(a))),
            _ => {}
        }
    }
    assert_eq!(arcs.len(), 2, "{arcs:?}");
    assert!(arcs.iter().all(|r| (r - 2.).abs() < 5e-3), "{arcs:?}");
    // Every arc end is exactly the end of a straight edge, and meets it tangentially.
    for curve in &sk.curves {
        let CurveKind::Arc { c, a, b } = curve.kind else { continue };
        for end in [a, b] {
            let touching = sk
                .curves
                .iter()
                .filter(|o| matches!(o.kind, CurveKind::Line { a: p, b: q } if pos(p).dist(pos(end)) < 1e-6 || pos(q).dist(pos(end)) < 1e-6))
                .count();
            assert!(touching >= 1, "arc end {:?} meets no line end", pos(end));
            let radial = pos(end) - pos(c);
            assert!((radial.len() - 2.).abs() < 5e-3);
        }
    }
}

/// A U-slot cut into a plate's edge with its top rim rounded: the round meets the top face on a
/// circle of radius 6.5 + 1 about the slot's axis, but the kernel makes that edge a spline that
/// only follows the circle (within 2e-6 mm), and it projected as twenty spline pieces. The
/// edge is a circle (the selection readout says so): it projects as an arc of that circle.
#[test]
fn a_rounded_slot_rim_projects_as_an_arc_of_its_circle() {
    use solidcraft_sketch::CurveKind;
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 40, "width": 30, "height": 10, "body_name": "Plate"}));
    run(&mut s, "solid.cylinder", json!({"radius": 6.5, "height": 20, "base": [20, 2, -5], "operation": "cut", "targets": ["Plate"]}));
    run(&mut s, "solid.fillet", json!({"edges": [[20, 8.5, 10]], "radius": 1}));
    let edge = run(&mut s, "sketch.create", json!({"plane": "XY", "offset": 30, "project_edges": false}))["sketch"].as_u64().unwrap();
    run(&mut s, "sketch.project", json!({"refs": [{"body": "Plate", "edge": [20, 9.5, 10]}]}));
    run(&mut s, "sketch.finish", json!({}));
    let face = run(&mut s, "sketch.create", json!({"plane": "XY", "offset": 30, "project_edges": false}))["sketch"].as_u64().unwrap();
    run(&mut s, "sketch.project", json!({"refs": [{"body": "Plate", "face": [5, 25, 10]}]}));
    run(&mut s, "sketch.finish", json!({}));

    let state = s.model.state();
    let center = solidcore_record::Vec2::new(20., 2.);
    for id in [edge, face] {
        let sk = &state.sketch(id).unwrap().sketch;
        let pos = |i: usize| sk.points[i].pos;
        let mut arcs = 0;
        for curve in &sk.curves {
            match curve.kind {
                CurveKind::Line { .. } => {}
                CurveKind::Arc { c, a, b } => {
                    arcs += 1;
                    assert!(pos(c).dist(center) < 1e-5, "arc centre {:?}", pos(c));
                    for end in [a, b] {
                        assert!((pos(end).dist(pos(c)) - 7.5).abs() < 1e-5, "arc end {:?} off the circle", pos(end));
                    }
                }
                ref other => panic!("sketch {id}: the rim projects as {other:?}, not as lines and arcs"),
            }
        }
        assert!(arcs >= 1, "sketch {id}: no arc in {:?}", sk.curves);
    }
    // The face's outline still closes: the plate's top less the slot's disc of radius 7.5 (its
    // part above the front face, whose chord lies 2 below the centre).
    let (r, d) = (7.5f64, 2f64);
    let below = r * r * (d / r).acos() - d * (r * r - d * d).sqrt();
    let area = 40. * 30. - (std::f64::consts::PI * r * r - below);
    let profiles = &state.sketch(face).unwrap().profiles;
    assert!(profiles.iter().any(|p| (p.area - area).abs() < 1e-3), "{:?} vs {area}", profiles.iter().map(|p| p.area).collect::<Vec<_>>());
}

/// A hook: a J of lines and two arcs about (-15, 0), of radii 7.5 and 15, extruded 20 with its
/// top edges chamfered. Seen along the extrusion its outline is the J again. The walls are seen
/// edge-on, but the triangles of a round wall join its two rims at slightly different angles
/// and each leans a hair towards the sketch or away: the outline ran zig-zag along the walls
/// and came in as 74 short lines. Faces that lie edge-on are edge-on in every triangle.
#[test]
fn whole_body_project_of_a_chamfered_hook_keeps_its_arcs() {
    use solidcraft_sketch::CurveKind;
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    run(&mut s, "sketch.line", json!({"points": [[0, 60], [0, 0]]}));
    run(&mut s, "sketch.arc.center_point", json!({"center": [-15, 0], "start": [-30, 0], "end": [0, 0]}));
    run(&mut s, "sketch.line", json!({"points": [[-30, 0], [-30, 10], [-22.5, 10], [-22.5, 0]]}));
    run(&mut s, "sketch.arc.center_point", json!({"center": [-15, 0], "start": [-22.5, 0], "end": [-7.5, 0]}));
    run(&mut s, "sketch.line", json!({"points": [[-7.5, 0], [-7.5, 60], [0, 60]]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"distance": 20, "body_name": "Hook"}));
    run(&mut s, "solid.chamfer", json!({"edges": [[-15, -15, 20], [-15, -7.5, 20], [0, 30, 20], [-7.5, 30, 20]], "distance": 0.25}));
    let sketch = run(&mut s, "sketch.create", json!({"plane": "XY", "offset": 40, "project_edges": false}))["sketch"].as_u64().unwrap();
    run(&mut s, "sketch.project", json!({"refs": [{"body": "Hook"}]}));
    run(&mut s, "sketch.finish", json!({}));

    let state = s.model.state();
    let solved = state.sketch(sketch).unwrap();
    let sk = &solved.sketch;
    let pos = |i: usize| sk.points[i].pos;
    let mut radii = Vec::new();
    for curve in &sk.curves {
        match curve.kind {
            CurveKind::Line { .. } => {}
            CurveKind::Arc { c, a, b } => {
                assert!(pos(c).dist(solidcore_record::Vec2::new(-15., 0.)) < 1e-3, "arc centre {:?}", pos(c));
                assert!((pos(a).dist(pos(c)) - pos(b).dist(pos(c))).abs() < 1e-6);
                radii.push(pos(a).dist(pos(c)));
            }
            ref other => panic!("the outline projects as {other:?}, not as lines and arcs"),
        }
    }
    radii.sort_by(f64::total_cmp);
    assert_eq!(radii.len(), 2, "{} curves, arcs of radii {radii:?}", sk.curves.len());
    assert!((radii[0] - 7.5).abs() < 1e-3 && (radii[1] - 15.).abs() < 1e-3, "{radii:?}");
    assert_eq!(sk.curves.len(), 8, "{:?}", sk.curves);
    // One closed profile: the J's area, a rectangle 7.5 x 60 and a half ring 7.5 to 15 with its
    // 7.5 x 10 tip.
    let area = 7.5 * 60. + std::f64::consts::PI * (15. * 15. - 7.5 * 7.5) / 2. + 7.5 * 10.;
    assert!(
        solved.profiles.iter().any(|p| (p.area - area).abs() < 0.05),
        "{:?} vs {area}",
        solved.profiles.iter().map(|p| p.area).collect::<Vec<_>>()
    );
}

#[test]
fn whole_body_intersect_of_a_filleted_plate_keeps_exact_lines_and_arcs() {
    use solidcraft_sketch::CurveKind;
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 80, "width": 50, "height": 8, "body_name": "Plate"}));
    let sketch = run(&mut s, "sketch.create", json!({"plane": "YZ", "offset": 36, "project_edges": false}))["sketch"].as_u64().unwrap();
    run(&mut s, "sketch.intersect", json!({"refs": [{"body": "Plate"}]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "timeline.roll_to", json!({"position": 1}));
    let top = [[40., 0.], [80., 25.], [40., 50.], [0., 25.]];
    let edges: Vec<_> = top.iter().map(|p| json!([p[0], p[1], 8])).collect();
    run(&mut s, "solid.fillet", json!({"edges": edges, "radius": 2}));
    run(&mut s, "timeline.roll_to", json!({}));

    let result = s.model.result(sketch).unwrap().warning.clone();
    assert!(result.is_none(), "{result:?}");
    let state = s.model.state();
    let sk = &state.sketch(sketch).unwrap().sketch;
    let pos = |i: usize| sk.points[i].pos;
    let mut arcs = Vec::new();
    for curve in &sk.curves {
        match curve.kind {
            CurveKind::Line { a, b } => {
                let d = pos(b) - pos(a);
                assert!(d.x.abs() < 1e-3 || d.y.abs() < 1e-3, "slanted line {:?} -> {:?}", pos(a), pos(b));
            }
            CurveKind::Arc { c, a, .. } => arcs.push(pos(c).dist(pos(a))),
            _ => {}
        }
    }
    assert_eq!(sk.curves.len(), 6, "{} curves", sk.curves.len());
    assert_eq!(arcs.len(), 2, "{arcs:?}");
    assert!(arcs.iter().all(|r| (r - 2.).abs() < 5e-3), "{arcs:?}");
}
#[test]
fn whole_body_intersect_keeps_a_small_round_between_short_walls() {
    use solidcraft_sketch::CurveKind;
    // A tapered groove on each side stops 2 mm short of the R2 corner rounds: walls of 2 mm, a
    // round, walls of 2 mm, which a loose fit takes for one larger arc that misses the walls.
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 150, "width": 55, "height": 150, "corner": [-75, 0, -75], "body_name": "Part"}));
    run(&mut s, "solid.fillet", json!({"edges": [[75, 27.5, 75], [-75, 27.5, 75], [75, 27.5, -75], [-75, 27.5, -75]], "radius": 2}));
    for (plane, offset, direction, rect) in [
        ("YZ", 75, "negative", [[7, -71], [48, 71]]),
        ("YZ", -75, "positive", [[7, -71], [48, 71]]),
        ("XY", 75, "negative", [[-71, 7], [71, 48]]),
        ("XY", -75, "positive", [[-71, 7], [71, 48]]),
    ] {
        run(&mut s, "sketch.create", json!({"plane": plane, "offset": offset}));
        run(&mut s, "sketch.rectangle.two_point", json!({"p0": rect[0], "p1": rect[1]}));
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "solid.extrude", json!({"distance": 6, "direction": direction, "taper": "-56.31 deg", "operation": "cut", "targets": ["Part"]}));
    }
    let sketch = run(&mut s, "sketch.create", json!({"plane": "XZ", "offset": 20, "project_edges": false}))["sketch"].as_u64().unwrap();
    run(&mut s, "sketch.intersect", json!({"refs": [{"body": "Part"}]}));
    let inspect = run(&mut s, "sketch.inspect", json!({}));
    let state = s.model.state();
    let sk = &state.sketch(sketch).unwrap().sketch;
    let pos = |i: usize| sk.points[i].pos;
    let arcs: Vec<f64> = sk
        .curves
        .iter()
        .filter_map(|c| match c.kind {
            CurveKind::Arc { c, a, .. } => Some(pos(c).dist(pos(a))),
            _ => None,
        })
        .collect();
    assert_eq!(arcs.len(), 4, "{arcs:?}");
    assert!(arcs.iter().all(|r| (r - 2.).abs() < 5e-3), "{arcs:?}");
    // Square less the corners' (4 - pi) r^2 and four trapezoid grooves (142 + 124) / 2 * 6.
    assert_eq!(inspect["profiles"].as_array().map(Vec::len), Some(1), "one closed outline");
    let area = inspect["profiles"][0]["area_mm2"].as_f64().unwrap();
    let expected = 150. * 150. - (4. - std::f64::consts::PI) * 4. - 4. * 798.;
    assert!((area - expected).abs() < 0.5, "{area} vs {expected}");
}
#[test]
fn a_rebuilt_projection_keeps_user_constraints_without_a_warning() {
    for dimensioned in [false, true] {
        let mut s = Session::default();
        run(&mut s, "solid.box", json!({"length": 40, "width": 30, "height": 20, "body_name": "Plate"}));
        run(&mut s, "solid.fillet", json!({"edges": [[40, 0, 10]], "radius": 5}));
        let sketch = run(&mut s, "sketch.create", json!({"plane": {"face": [20, 15, 20]}, "project_edges": false}))["sketch"].as_u64().unwrap();
        run(&mut s, "sketch.project", json!({"refs": [{"body": "Plate"}]}));
        if dimensioned {
            // A line of the user's, pinned to the projected corner.
            let line = ids(&run(&mut s, "sketch.line", json!({"points": [[5, 5], [10, 5]]}))["curves"]);
            run(&mut s, "sketch.constraint.coincident", json!({"a": format!("{}.start", line[0]), "b": "p4"}));
        }
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "timeline.roll_to", json!({"position": 2}));
        run(&mut s, "solid.chamfer", json!({"edges": [[40, 15, 20]], "distance": 1}));
        run(&mut s, "timeline.roll_to", json!({}));
        let warning = s.model.result(sketch).unwrap().warning.clone();
        let st = s.model.state();
        let k = &st.sketch(sketch).unwrap().sketch;
        // The user's constraint survives the rebuild, so there is nothing to warn about.
        assert_eq!(k.constraints.len(), usize::from(dimensioned));
        assert!(warning.is_none(), "{warning:?}");
    }
}

/// #9001: an angle (an L profile, wall thickness `d1`, legs `d2`) with a sketch on its face and
/// a hole dimensioned to the inner wall's projected edge. Making the wall thicker moves the
/// inner corner by more than a tenth of the face; the projection follows it (it was kept as it
/// was, "boundary identity is ambiguous"), and the hole with it, as it already did for `d2`.
#[test]
fn a_thicker_angle_moves_the_projected_face_of_the_sketch_on_it() {
    let mut s = Session::default();
    run(&mut s, "parameters.change", json!({"name": "d1", "expression": "20 mm"}));
    run(&mut s, "parameters.change", json!({"name": "d2", "expression": "80 mm"}));
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    let l = ids(&run(&mut s, "sketch.line", json!({"points": [[0, 0], [80, 0], [80, 20], [20, 20], [20, 80], [0, 80]], "closed": true}))["curves"]);
    for (i, line) in l.iter().enumerate() {
        let mode = if i % 2 == 0 { "horizontal" } else { "vertical" };
        run(&mut s, "sketch.constraint.horizontal_vertical", json!({"line": line, "mode": mode}));
    }
    run(&mut s, "sketch.constraint.coincident", json!({"a": format!("{}.start", l[0]), "b": "origin"}));
    for (k, value) in [(0, "d2"), (1, "d1"), (4, "d1"), (5, "d2")] {
        run(&mut s, "sketch.dimension", json!({"entities": [l[k]], "value": value}));
    }
    assert_eq!(inspect(&mut s)["dof"], 0);
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"distance": 30}));
    let sketch = run(&mut s, "sketch.create", json!({"plane": {"face": [10, 10, 30]}}))["sketch"].as_u64().unwrap();
    let si = inspect(&mut s);
    // The inner wall's edge, from (20, 20) to (20, 80) on the face.
    let wall = linked_curves(&si)
        .into_iter()
        .find(|c| {
            ["start_at", "end_at"].iter().all(|e| (c[e][0].as_f64().unwrap().abs() - 20.).abs() < 1e-6 && c[e][1].as_f64().unwrap().abs() > 19.)
        })
        .unwrap_or_else(|| panic!("{si}"));
    let hole = ids(&run(&mut s, "sketch.circle.center", json!({"center": [10, 50], "radius": 3}))["curves"])[0].clone();
    run(&mut s, "sketch.dimension", json!({"entities": [format!("{hole}.center"), wall["id"]], "value": 10}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "parameters.change", json!({"name": "d1", "expression": "30 mm"}));
    let check = |s: &Session| {
        let warning = s.model.result(sketch).and_then(|r| r.warning.clone());
        assert!(warning.is_none(), "{warning:?}");
        let st = s.model.state();
        let ss = st.sketch(sketch).unwrap();
        assert!(ss.sketch.links.iter().all(|l| !l.lost));
        let world = |p: solidcore_record::Vec2| ss.plane.to_world(p);
        let at = |x: f64, y: f64| ss.sketch.points.iter().any(|p| world(p.pos).dist(solidcore_record::Vec3::new(x, y, 30.)) < 1e-6);
        // The inner corner is at (30, 30) now, and the hole 10 from the wall that moved.
        assert!(at(30., 30.) && !at(20., 20.));
        let center = ss.sketch.points.iter().find(|p| p.id == format!("{hole}.center")).unwrap();
        assert!((world(center.pos).x - 20.).abs() < 1e-6, "{:?}", world(center.pos));
        assert_eq!(ss.sketch.constraints.len(), 1);
    };
    check(&s);
    check(&Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap()));
}

/// A sphere's outline from above is its equator: one circle and one profile. Flat triangles beside
/// the equator can tip the other way from the sphere, and their edges must not join the outline.
#[test]
fn a_spheres_silhouette_is_one_circle() {
    let mut s = Session::default();
    run(&mut s, "solid.sphere", json!({"radius": 8, "body_name": "Ball"}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "offset": -10, "name": "Below"}));
    let r = run(&mut s, "sketch.project", json!({"refs": [{"body": "Ball"}]}));
    let curves = r["links"][0]["curves"].as_array().unwrap();
    assert_eq!(curves.len(), 1, "{r}");
    let inspected = run(&mut s, "sketch.inspect", json!({}));
    let circle = inspected["curves"].as_array().unwrap().iter().find(|c| c["id"] == curves[0]).unwrap();
    assert_eq!(circle["type"], "circle", "{circle}");
    assert!((circle["radius"].as_f64().unwrap() - 8.0).abs() < 1e-6, "{circle}");
    let done = run(&mut s, "sketch.finish", json!({}));
    assert_eq!(done["profiles"], 1, "{done}");
}

/// The silhouette of a square with a conic round at each corner (rho 0.19, 0.4, 0.5 and 0.7) is
/// one closed outline of the square's true area. Fitted arcs along a conic were snapped onto lines
/// that ended further along the outline, which left gaps of up to 3.6 mm: no profile.
#[test]
fn the_silhouette_of_conic_rounds_closes() {
    let mut s = Session::default();
    let corners = [
        ([-75., 65.], [-75., 75.], [-65., 75.], 0.19),
        ([65., 75.], [75., 75.], [75., 65.], 0.4),
        ([75., -65.], [75., -75.], [65., -75.], 0.5),
        ([-65., -75.], [-75., -75.], [-75., -65.], 0.7),
    ];
    run(&mut s, "sketch.create", json!({"plane": "XZ"}));
    for (i, (start, apex, end, rho)) in corners.iter().enumerate() {
        run(&mut s, "sketch.conic", json!({"start": start, "apex": apex, "end": end, "rho": rho}));
        let next = corners[(i + 1) % 4].0;
        run(&mut s, "sketch.line", json!({"points": [end, next]}));
    }
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"distance": 55, "body_name": "Part"}));
    run(&mut s, "sketch.create", json!({"plane": "XZ", "offset": -10}));
    run(&mut s, "sketch.project", json!({"refs": [{"body": "Part"}]}));
    // The true area: the outline through the four rational quadratic Béziers (apex weight
    // rho / (1 - rho)), finely sampled, and the straight sides between them.
    let mut outline: Vec<[f64; 2]> = Vec::new();
    for (start, apex, end, rho) in corners {
        let w = rho / (1. - rho);
        for k in 0..=2000 {
            let t = f64::from(k) / 2000.;
            let (b0, b1, b2) = ((1. - t) * (1. - t), 2. * t * (1. - t) * w, t * t);
            let sum = b0 + b1 + b2;
            outline.push([(b0 * start[0] + b1 * apex[0] + b2 * end[0]) / sum, (b0 * start[1] + b1 * apex[1] + b2 * end[1]) / sum]);
        }
    }
    let n = outline.len();
    let exact = (0..n).map(|i| outline[i][0] * outline[(i + 1) % n][1] - outline[(i + 1) % n][0] * outline[i][1]).sum::<f64>().abs() / 2.;
    let si = inspect(&mut s);
    let profiles = si["profiles"].as_array().unwrap();
    assert_eq!(profiles.len(), 1, "{}", si["profiles"]);
    let area = profiles[0]["area_mm2"].as_f64().unwrap();
    assert!((area - exact).abs() < exact * 2e-4, "area {area}, exact {exact}");
}

/// A second placement's body is projected where that placement puts it (#9957): a sketch sees
/// "Block (Part:2)" as the browser shows it, not only the first placement's "Block", on a plane
/// and on a face alike.
#[test]
fn a_second_placements_face_and_edge_project_where_it_is_placed() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length":100,"width":40,"height":5,"body_name":"Plate"}));
    let part = run(&mut s, "component.create", json!({"name":"Part"}));
    run(&mut s, "solid.box", json!({"length":20,"width":20,"height":10,"body_name":"Block"}));
    run(&mut s, "component.activate", json!({"component":0}));
    run(&mut s, "occurrence.copy", json!({"component":part["component"],"translate":[50,0,5]}));
    let mut sketches = Vec::new();
    let picks = [
        (json!({"plane":"XY","offset":15}), json!({"body":"Block (Part:2)","face":[60,10,15]})),
        (json!({"plane":"XY","offset":15}), json!({"body":"Block (Part:2)","edge":[60,20,15]})),
        (json!({"plane":{"face":[90,35,5]},"project_edges":false}), json!({"body":"Block (Part:2)","face":[60,10,15]})),
    ];
    for (plane, pick) in picks {
        let id = run(&mut s, "sketch.create", plane)["sketch"].as_u64().unwrap();
        let projected = run(&mut s, "sketch.project", json!({"refs":[pick]}));
        assert_eq!(projected["links"].as_array().map(Vec::len), Some(1), "{projected}");
        run(&mut s, "sketch.finish", json!({}));
        sketches.push(id);
    }
    // Every projected point is on the copy (x 50..70, y from `y`), none on the original.
    let check = |s: &Session, y: f64| {
        let state = s.model.state();
        for id in &sketches {
            let ss = state.sketch(*id).unwrap();
            let sk = &ss.sketch;
            assert!(sk.links.iter().all(|l| !l.lost), "{:?}", sk.links);
            let points: Vec<_> =
                sk.links.iter().flat_map(|l| sk.link_points(&l.id)).filter_map(|p| sk.points.get(p)).map(|p| ss.plane.to_world(p.pos)).collect();
            assert!(!points.is_empty());
            assert!(points.iter().all(|p| p.x > 49.9 && p.x < 70.1 && p.y > y - 0.1 && p.y < y + 20.1), "sketch {id}, y {y}: {points:?}");
        }
    };
    check(&s, 0.0);
    // A design opened again, so rebuilt from its features, finds the copy as well.
    check(&Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap()), 0.0);
    // Moving the copy moves what was projected from it.
    let first = s.doc.occurrences[0].id;
    let copy = s.doc.occurrences.iter().map(|o| o.id).find(|id| *id != first).unwrap();
    run(&mut s, "occurrence.move", json!({"occurrence":copy,"translate":[0,15,0],"capture":true}));
    check(&s, 15.0);
    // Renaming the part or the body renames the copy ("Base (Housing:2)"): the links follow it.
    run(&mut s, "component.rename", json!({"component":part["component"],"name":"Housing"}));
    run(&mut s, "body.rename", json!({"body":"Block","name":"Base"}));
    assert!(s.model.state().sketch(sketches[0]).unwrap().sketch.links.iter().all(|l| l.source.bodies() == ["Base (Housing:2)"]));
    check(&s, 15.0);
    check(&Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap()), 15.0);
}

/// #13142: projected references can start as construction geometry, asked for per call or for
/// the whole session (a preference, kept for designs started and opened like `auto_project`).
#[test]
fn projected_references_can_start_as_construction() {
    let flags = |s: &mut Session| linked_curves(&inspect(s)).iter().map(|c| c["construction"] == true).collect::<Vec<_>>();
    let mut s = Session::default();
    box_part(&mut s);
    // Off (the default): the face loop is normal geometry and closes a profile.
    run(&mut s, "sketch.create", json!({"plane": {"face": [20, 15, 20]}}));
    let on_face = flags(&mut s);
    assert!(on_face.len() == 4 && on_face.iter().all(|c| !c), "{on_face:?}");
    assert_eq!(inspect(&mut s)["profiles"].as_array().map(Vec::len), Some(1));
    run(&mut s, "sketch.finish", json!({}));
    // Asked for in the call, and not.
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    run(&mut s, "sketch.project", json!({"refs": [{"edge": [20, 0, 20]}], "construction": true}));
    assert_eq!(flags(&mut s), vec![true]);
    run(&mut s, "sketch.project", json!({"refs": [{"edge": [40, 15, 20]}]}));
    assert_eq!(flags(&mut s), vec![true, false]);
    run(&mut s, "sketch.intersect", json!({"refs": [{"edge": [0, 0, 10]}], "construction": true}));
    let e = s.execute("sketch.project", &json!({"refs": [{"edge": [20, 30, 20]}], "construction": 1}));
    assert!(e.is_err(), "construction must be a boolean");
    run(&mut s, "sketch.finish", json!({}));
    // The session's preference: a sketch on a face starts with a construction loop and no
    // profile, and it holds for the next design; `construction: false` still asks for normal.
    s.project_construction = true;
    run(&mut s, "file.new", json!({}));
    assert!(s.project_construction, "file.new");
    box_part(&mut s);
    run(&mut s, "sketch.create", json!({"plane": {"face": [20, 15, 20]}}));
    let on_face = flags(&mut s);
    assert!(on_face.len() == 4 && on_face.iter().all(|c| *c), "{on_face:?}");
    assert_eq!(inspect(&mut s)["profiles"].as_array().map(Vec::len), Some(0));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    run(&mut s, "sketch.project", json!({"refs": [{"edge": [20, 0, 20]}]}));
    run(&mut s, "sketch.project", json!({"refs": [{"edge": [40, 15, 20]}], "construction": false}));
    assert_eq!(flags(&mut s), vec![true, false]);
}

/// #13464: an imported mesh is one face with no edges, so Project on one of its faces projected
/// nothing. The pick now means the flat patch of triangles there: a box's top projects as its
/// rectangle, its side seen edge-on as a line, and the link keeps the pick's point (a mesh's face
/// name says nothing of the patch), so the sketch still builds once finished.
#[test]
fn a_face_of_an_imported_mesh_projects_its_flat_patch() {
    let dir = std::env::temp_dir().join(format!("solidcraft-project-mesh-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let stl = dir.join("box.stl");
    let mut src = Session::default();
    run(&mut src, "solid.box", json!({"length": 20, "width": 10, "height": 5}));
    run(&mut src, "file.export_stl", json!({"path": stl.to_string_lossy()}));

    let mut s = Session::default();
    run(&mut s, "file.insert_mesh", json!({"path": stl.to_string_lossy()}));
    let body = s.model.state().bodies[0].name.clone();
    run(&mut s, "sketch.create", json!({"plane": "XY", "offset": 20}));
    run(&mut s, "sketch.project", json!({"refs": [{"body": body, "face": [10, 5, 5]}]}));
    let si = inspect(&mut s);
    let lines = linked_curves(&si);
    assert_eq!(lines.len(), 4, "{si}");
    let mut corners: Vec<(i64, i64)> = lines
        .iter()
        .flat_map(|c| [&c["start_at"], &c["end_at"]])
        .map(|p| ((p[0].as_f64().unwrap() * 1e6).round() as i64, (p[1].as_f64().unwrap() * 1e6).round() as i64))
        .collect();
    corners.sort_unstable();
    corners.dedup();
    assert_eq!(corners, [(0, 0), (0, 10_000_000), (20_000_000, 0), (20_000_000, 10_000_000)], "{si}");
    assert!(s.active_sketch.is_some());
    run(&mut s, "sketch.finish", json!({}));
    let failing: Vec<_> = s.model.results.iter().filter_map(|r| r.error.clone()).collect();
    assert!(failing.is_empty(), "{failing:?}");

    // A side seen edge-on: the line it makes.
    run(&mut s, "sketch.create", json!({"plane": "XY", "offset": 20}));
    run(&mut s, "sketch.project", json!({"refs": [{"body": body, "face": [10, 0, 2.5]}]}));
    assert_eq!(linked_curves(&inspect(&mut s)).len(), 1);
    std::fs::remove_dir_all(&dir).ok();
}

/// #13464: on a sketch plane at a slant to the face, a flat patch's polygon keeps its straight
/// sides. A rectangle's four corners and a triangle's three always lie on one circle, so they must
/// not be taken for a circle seen at a slant (an ellipse); a tessellated circle still is one.
#[test]
fn a_mesh_face_keeps_its_straight_sides_on_a_slanted_plane() {
    let dir = std::env::temp_dir().join(format!("solidcraft-project-mesh-slant-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut src = Session::default();
    run(&mut src, "solid.box", json!({"length": 20, "width": 10, "height": 5, "body_name": "box"}));
    run(&mut src, "sketch.create", json!({"plane": "XY"}));
    run(&mut src, "sketch.line", json!({"points": [[30, 0], [42, 0], [34, 9], [30, 0]]}));
    run(&mut src, "sketch.finish", json!({}));
    let sketch = src.doc.features.last().unwrap().id;
    run(&mut src, "solid.extrude", json!({"sketch": sketch, "distance": 5, "body_name": "tri"}));
    run(&mut src, "solid.cylinder", json!({"base": [60, 0, 0], "radius": 7, "height": 5}));
    let mut stls = Vec::new();
    for body in src.model.state().bodies.iter().map(|b| b.name.clone()).collect::<Vec<_>>() {
        let path = dir.join(format!("{body}.stl"));
        run(&mut src, "file.export_stl", json!({"path": path.to_string_lossy(), "bodies": [body]}));
        stls.push(path);
    }

    let mut s = Session::default();
    for stl in &stls {
        run(&mut s, "file.insert_mesh", json!({"path": stl.to_string_lossy()}));
    }
    let names: Vec<String> = s.model.state().bodies.iter().map(|b| b.name.clone()).collect();
    run(&mut s, "construct.plane.at_angle", json!({"base": "XY", "axis": "X", "angle": "30 deg", "name": "Tilt"}));
    let kinds = |s: &mut Session, body: &str, at: [f64; 3]| -> Vec<String> {
        run(s, "sketch.create", json!({"plane": "Tilt"}));
        run(s, "sketch.project", json!({"refs": [{"body": body, "face": at}]}));
        let kinds = linked_curves(&inspect(s)).iter().map(|c| c["type"].as_str().unwrap_or("").to_string()).collect();
        run(s, "sketch.finish", json!({}));
        kinds
    };
    assert_eq!(kinds(&mut s, &names[0], [10., 5., 5.]), ["line"; 4], "the box's top is a rectangle");
    assert_eq!(kinds(&mut s, &names[1], [35., 3., 5.]), ["line"; 3], "the prism's top is a triangle");
    let round = kinds(&mut s, &names[2], [60., 0., 5.]);
    assert_eq!(round.len(), 1, "the cylinder's top is one curve: {round:?}");
    assert_eq!(round[0], "ellipse", "the cylinder's round top, seen at a slant: {round:?}");
    std::fs::remove_dir_all(&dir).ok();
}

/// A component's origin axis, as the Browser names it (`o:<id>/origin/Z`), projects into a sketch of
/// another component as its line, where that component now stands (#15086).
#[test]
fn a_component_origin_axis_projects_where_the_component_stands() {
    let mut s = Session::default();
    run(&mut s, "component.create", json!({"name": "Anchor"}));
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10}));
    run(&mut s, "component.create", json!({"name": "Part", "parent": 0}));
    run(&mut s, "solid.box", json!({"length": 5, "width": 5, "height": 5, "operation": "new"}));
    let anchor_component = s.doc.find_component("Anchor").unwrap();
    let anchor = s.doc.occurrence_of(anchor_component).unwrap().id;
    run(&mut s, "occurrence.move", json!({"occurrence": anchor, "translate": [100, 0, 0], "move_grounded": true}));
    run(&mut s, "sketch.create", json!({"plane": "XZ"}));
    let r = run(&mut s, "sketch.project", json!({"refs": [{"axis": format!("o:{anchor}/origin/Z")}]}));
    let line = r["links"][0]["curves"][0].clone();
    let si = inspect(&mut s);
    let c = si["curves"].as_array().unwrap().iter().find(|c| c["id"] == line).cloned().unwrap();
    let n = |v: &Value| v.as_f64().unwrap();
    let (x0, x1, length) = (n(&c["start_at"][0]), n(&c["end_at"][0]), (n(&c["end_at"][1]) - n(&c["start_at"][1])).abs());
    assert!((x0 - 100.0).abs() < 1e-6 && (x1 - 100.0).abs() < 1e-6 && length > 10.0, "{x0} {x1} {length}");
    // A sketch made after the component moved again reads where it stands now.
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "occurrence.move", json!({"occurrence": anchor, "translate": [150, 0, 0], "move_grounded": true}));
    run(&mut s, "sketch.create", json!({"plane": "XZ"}));
    let r = run(&mut s, "sketch.project", json!({"refs": [{"axis": format!("o:{anchor}/origin/Z")}]}));
    let line = r["links"][0]["curves"][0].clone();
    let si = inspect(&mut s);
    let c = si["curves"].as_array().unwrap().iter().find(|c| c["id"] == line).cloned().unwrap();
    assert!((n(&c["start_at"][0]) - 250.0).abs() < 1e-6, "a new projection reads the current placement: {}", n(&c["start_at"][0]));
    // A key that names no component origin is still refused, with the existing message.
    let err = s.execute("sketch.project", &json!({"refs": [{"axis": "o:9999/origin/Z"}]})).unwrap_err().to_string();
    assert!(err.contains("unknown axis"), "{err}");
}
