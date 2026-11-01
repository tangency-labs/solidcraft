//! Construction axes, points and planes (SOLID ▸ CONSTRUCT), one test per type, against a
//! 40 × 30 × 20 box at the origin, a cylinder, a sphere and a torus.

use serde_json::{Value, json};
use solidcraft_geom::{Plane, Vec3};

use crate::Session;
use crate::view::{construction_axes, construction_planes, construction_points};

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    match s.execute(id, &p) {
        Ok(v) => v,
        Err(e) => panic!("{id} {p}: {e}"),
    }
}

fn part() -> Session {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 40, "width": 30, "height": 20, "body_name": "Box"}));
    run(&mut s, "solid.cylinder", json!({"base": [60, 0, 0], "radius": 5, "height": 10, "body_name": "Cyl"}));
    run(&mut s, "solid.sphere", json!({"center": [0, 0, 60], "radius": 5, "body_name": "Ball"}));
    run(&mut s, "solid.torus", json!({"center": [0, 60, 0], "major": 10, "minor": 2, "body_name": "Ring"}));
    s
}

fn near(a: Vec3, b: Vec3) -> bool {
    a.dist(b) < 1e-3
}

fn parallel(a: Vec3, b: Vec3) -> bool {
    a.normalized().zip(b.normalized()).is_some_and(|(a, b)| a.cross(b).len() < 1e-6)
}

fn axis(s: &Session, name: &str) -> (Vec3, Vec3) {
    construction_axes(s).into_iter().find(|a| a.1 == name).map(|a| (a.2, a.3)).unwrap_or_else(|| panic!("no axis {name}"))
}

fn point(s: &Session, name: &str) -> Vec3 {
    construction_points(s).into_iter().find(|p| p.1 == name).map(|p| p.2).unwrap_or_else(|| panic!("no point {name}"))
}

fn plane(s: &Session, name: &str) -> Plane {
    construction_planes(s).into_iter().find(|p| p.1 == name).map(|p| p.2).unwrap_or_else(|| panic!("no plane {name}"))
}

#[test]
fn renamed_construction_references_survive_edits_and_reopening() {
    let mut s = Session::default();
    run(&mut s, "construct.axis.two_planes", json!({"a": "XZ", "b": "YZ", "name": "Spine"}));
    run(&mut s, "construct.plane.perpendicular", json!({"plane": "XZ", "line": {"axis": "Spine"}, "name": "Fence"}));
    run(&mut s, "construct.point.three_planes", json!({"a": "XY", "b": "XZ", "c": "YZ", "name": "Pin"}));
    run(&mut s, "construct.axis.two_points", json!({"a": {"construct_point": "Pin"}, "b": {"point": [30, 30, 30]}, "name": "Brace"}));
    run(&mut s, "construct.point.vertex", json!({"point": {"construct_point": "Pin"}, "name": "Copy"}));
    let before = (*s.doc).clone();
    run(&mut s, "timeline.rename", json!({"feature": "Spine", "name": "Mast"}));
    run(&mut s, "timeline.rename", json!({"feature": "Pin", "name": "Peg"}));
    let fence = run(&mut s, "timeline.inspect", json!({"feature": "Fence"}));
    assert_eq!(fence["definition"]["plane"]["line"]["name"], "Mast");
    let brace = run(&mut s, "timeline.inspect", json!({"feature": "Brace"}));
    assert_eq!(brace["definition"]["def"]["a"]["name"], "Peg");
    let renamed = (*s.doc).clone();
    for _ in 0..2 {
        run(&mut s, "edit.undo", json!({}));
    }
    assert_eq!(*s.doc, before);
    for _ in 0..2 {
        run(&mut s, "edit.redo", json!({}));
    }
    assert_eq!(*s.doc, renamed);
    run(&mut s, "timeline.edit", json!({"feature": "Mast", "set": {"def.b": {"type": "plane", "plane": {"type": "origin", "name": "XY"}}}}));
    let saved = serde_json::to_vec(&*s.doc).unwrap();
    let mut reopened = Session::new(serde_json::from_slice(&saved).unwrap());
    reopened.refresh();
    for session in [&s, &reopened] {
        assert!(session.model.results.iter().all(|r| r.error.is_none()), "{:?}", session.model.results);
        assert!(parallel(plane(session, "Fence").normal(), Vec3::Z));
        assert!(parallel(axis(session, "Brace").1, Vec3::new(1.0, 1.0, 1.0)));
        assert!(near(point(session, "Copy"), Vec3::ZERO));
    }
}

#[test]
fn renamed_hidden_axes_and_points_stay_hidden_through_undo_redo() {
    let mut s = Session::default();
    run(&mut s, "construct.axis.two_planes", json!({"a": "XZ", "b": "YZ", "name": "Spine"}));
    run(&mut s, "construct.point.three_planes", json!({"a": "XY", "b": "XZ", "c": "YZ", "name": "Pin"}));
    run(&mut s, "browser.visibility", json!({"items": ["Spine", "Pin", "XY"], "visible": false}));
    for (old, new) in [("Spine", "Mast"), ("Pin", "Peg")] {
        run(&mut s, "timeline.rename", json!({"feature": old, "name": new}));
        assert!(!s.construction_visible(new));
        assert!(!s.visibility.hidden_origin.iter().any(|n| n == old));
        run(&mut s, "edit.undo", json!({}));
        assert!(!s.construction_visible(old));
        assert!(!s.visibility.hidden_origin.iter().any(|n| n == new));
        run(&mut s, "edit.redo", json!({}));
        assert!(!s.construction_visible(new));
    }
    assert!(!s.construction_visible("XY"));
}

/// The line through `o` along `d` contains `p`.
fn on_line(o: Vec3, d: Vec3, p: Vec3) -> bool {
    let d = d.normalized().unwrap_or(Vec3::Z);
    (p - o - d * (p - o).dot(d)).len() < 1e-3
}

fn on_plane(pl: &Plane, p: Vec3) -> bool {
    (p - pl.origin).dot(pl.normal()).abs() < 1e-3
}

#[test]
fn plane_through_axis_and_point_contains_both_and_rejects_an_on_axis_point() {
    let mut s = Session::default();
    run(&mut s, "construct.plane.axis_point", json!({"axis": {"axis": "Z"}, "point": {"point": [10, 20, 30]}, "name": "Fence"}));
    let pl = plane(&s, "Fence");
    for p in [Vec3::ZERO, Vec3::Z * 100.0, Vec3::new(10.0, 20.0, 30.0)] {
        assert!(on_plane(&pl, p), "{p:?} is on {pl:?}");
    }
    assert!(parallel(pl.normal(), Vec3::new(-20.0, 10.0, 0.0)));
    let before = s.doc.to_json();
    let error = s.execute("construct.plane.axis_point", &json!({"axis": {"axis": "Z"}, "point": {"point": [0, 0, 50]}})).unwrap_err();
    assert!(error.to_string().contains("point is on the axis"), "{error}");
    assert_eq!(s.doc.to_json(), before, "a degenerate plane leaves the design unchanged");
}

#[test]
fn plane_through_axis_and_point_follows_renamed_sources_edits_undo_and_reopen() {
    let mut s = Session::default();
    run(&mut s, "construct.axis.two_points", json!({"a":{"point":[5,6,7]},"b":{"point":[5,6,17]},"name":"Spine"}));
    run(&mut s, "construct.point.vertex", json!({"point":{"point":[15,6,7]},"name":"Pin"}));
    run(&mut s, "construct.plane.axis_point", json!({"axis":{"axis":"Spine"},"point":{"construct_point":"Pin"},"name":"Fence"}));
    assert!(parallel(plane(&s, "Fence").normal(), Vec3::Y));
    run(&mut s, "timeline.rename", json!({"feature":"Spine","name":"Mast"}));
    run(&mut s, "timeline.rename", json!({"feature":"Pin","name":"Peg"}));
    let definition = run(&mut s, "timeline.inspect", json!({"feature":"Fence"}));
    assert_eq!(definition["definition"]["plane"]["axis"]["name"], "Mast");
    assert_eq!(definition["definition"]["plane"]["point"]["name"], "Peg");
    run(&mut s, "timeline.edit", json!({"feature":"Peg","set":{"def.point":{"type":"point","p":[5,16,7]}}}));
    assert!(parallel(plane(&s, "Fence").normal(), Vec3::X), "point edit rotates the dependent plane");
    run(&mut s, "edit.undo", json!({}));
    assert!(parallel(plane(&s, "Fence").normal(), Vec3::Y));
    run(&mut s, "edit.redo", json!({}));
    let reopened = solidcraft_doc::format::read(&s.doc.to_json()).unwrap();
    let mut cold = Session::new(reopened);
    cold.refresh();
    assert!(cold.model.results.iter().all(|r| r.error.is_none()), "{:?}", cold.model.results);
    let pl = plane(&cold, "Fence");
    assert!(parallel(pl.normal(), Vec3::X));
    assert!(on_plane(&pl, Vec3::new(5.0, 16.0, 7.0)));
    run(&mut cold, "sketch.create", json!({"plane":"Fence"}));
}

#[test]
fn plane_through_axis_and_point_accepts_straight_model_and_sketch_lines() {
    let mut s = part();
    run(&mut s, "construct.plane.axis_point", json!({"axis":{"edge":[20,0,0]},"point":{"vertex":[40,30,0]},"name":"Base"}));
    assert!(parallel(plane(&s, "Base").normal(), Vec3::Z));
    let sk = run(&mut s, "sketch.create", json!({"plane":"XY"}))["sketch"].as_u64().unwrap();
    let line = run(&mut s, "sketch.line", json!({"points":[[0,0],[10,0]]}));
    let curve = line["curves"][0].as_str().unwrap();
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "construct.plane.axis_point", json!({"axis":{"sketch":sk,"curve":curve},"point":{"point":[0,0,10]},"name":"Side"}));
    assert!(parallel(plane(&s, "Side").normal(), Vec3::Y));
}

#[test]
fn plane_through_axis_and_point_in_a_moved_component_keeps_its_world_picks() {
    let mut s = Session::default();
    run(&mut s, "component.create", json!({"name":"Part","grounded":false}));
    run(&mut s, "occurrence.move", json!({"occurrence":"Part:1","translate":[50,0,0]}));
    run(&mut s, "construct.axis.two_points", json!({"a":{"point":[50,0,0]},"b":{"point":[50,0,10]},"name":"Mast"}));
    run(&mut s, "construct.plane.axis_point", json!({"axis":{"axis":"Mast"},"point":{"point":[60,0,0]},"name":"Fence"}));
    run(&mut s, "occurrence.move", json!({"occurrence":"Part:1","translate":[0,20,0]}));
    let pl = plane(&s, "Fence");
    assert!(parallel(pl.normal(), Vec3::Y));
    for p in [Vec3::new(50.0, 20.0, 0.0), Vec3::new(50.0, 20.0, 10.0), Vec3::new(60.0, 20.0, 0.0)] {
        assert!(on_plane(&pl, p), "{p:?} on {pl:?}");
    }
    run(&mut s, "component.activate", json!({"component":0}));
    run(
        &mut s,
        "timeline.redefine",
        json!({"feature":"Fence","command":"construct.plane.axis_point","params":{"axis":{"axis":"Mast"},"point":{"point":[70,20,0]}}}),
    );
    let definition = run(&mut s, "timeline.inspect", json!({"feature":"Fence"}));
    assert_eq!(
        definition["definition"]["plane"]["point"],
        json!({"type":"point","p":[20.,0.,0.]}),
        "redefine uses the feature's authoring frame, not the active root"
    );
}

#[test]
fn axis_through_cylinder_cone_or_torus() {
    let mut s = part();
    run(&mut s, "construct.axis.through_cylinder", json!({"face": {"face": [65, 0, 5]}, "name": "A"}));
    let (o, d) = axis(&s, "A");
    assert!(parallel(d, Vec3::Z) && on_line(o, d, Vec3::new(60.0, 0.0, 0.0)), "{o:?} {d:?}");
    run(&mut s, "construct.axis.through_cylinder", json!({"face": {"face": [12, 60, 0]}, "name": "T"}));
    let (o, d) = axis(&s, "T");
    assert!(parallel(d, Vec3::Z) && on_line(o, d, Vec3::new(0.0, 60.0, 0.0)), "{o:?} {d:?}");
    // A flat face has no axis.
    assert!(s.execute("construct.axis.through_cylinder", &json!({"face": {"face": [20, 15, 20]}})).is_err());
}

#[test]
fn axis_perpendicular_to_a_face_at_a_point() {
    let mut s = part();
    run(&mut s, "construct.axis.normal_to_face", json!({"face": {"face": [10, 10, 20]}, "point": {"point": [10, 10, 25]}, "name": "N"}));
    let (o, d) = axis(&s, "N");
    assert!(near(o, Vec3::new(10.0, 10.0, 20.0)) && parallel(d, Vec3::Z), "{o:?} {d:?}");
    // On the cylinder: radial through the point.
    run(&mut s, "construct.axis.normal_to_face", json!({"face": {"face": [65, 0, 5]}, "point": {"point": [70, 0, 5]}, "name": "R"}));
    let (o, d) = axis(&s, "R");
    assert!(near(o, Vec3::new(65.0, 0.0, 5.0)) && parallel(d, Vec3::X), "{o:?} {d:?}");
}

#[test]
fn axis_through_two_planes() {
    let mut s = part();
    run(&mut s, "construct.axis.two_planes", json!({"a": {"plane": "XZ"}, "b": {"face": [40, 15, 10]}, "name": "L"}));
    let (o, d) = axis(&s, "L");
    assert!(parallel(d, Vec3::Z) && on_line(o, d, Vec3::new(40.0, 0.0, 0.0)), "{o:?} {d:?}");
    assert!(s.execute("construct.axis.two_planes", &json!({"a": "XY", "b": {"face": [20, 15, 20]}})).is_err(), "parallel");
}

#[test]
fn axis_through_two_points() {
    let mut s = part();
    run(&mut s, "construct.axis.two_points", json!({"a": {"vertex": [0.5, 0.5, 0.5]}, "b": {"vertex": [39, 29, 19]}, "name": "D"}));
    let (o, d) = axis(&s, "D");
    assert!(on_line(o, d, Vec3::ZERO) && parallel(d, Vec3::new(40.0, 30.0, 20.0)), "{o:?} {d:?}");
}

#[test]
fn axis_through_an_edge() {
    let mut s = part();
    run(&mut s, "construct.axis.edge", json!({"edge": {"edge": [20, 0, 0]}, "name": "E"}));
    let (o, d) = axis(&s, "E");
    assert!(parallel(d, Vec3::X) && on_line(o, d, Vec3::ZERO), "{o:?} {d:?}");
    // A circular edge is not straight.
    assert!(s.execute("construct.axis.edge", &json!({"edge": {"edge": [65, 0, 10]}})).is_err());
}

#[test]
fn point_at_a_vertex() {
    let mut s = part();
    run(&mut s, "construct.point.vertex", json!({"point": {"vertex": [39, 29, 19]}, "name": "P"}));
    assert!(near(point(&s, "P"), Vec3::new(40.0, 30.0, 20.0)));
}

#[test]
fn point_through_two_edges() {
    let mut s = part();
    run(&mut s, "construct.point.two_edges", json!({"a": {"edge": [20, 0, 0]}, "b": {"edge": [40, 15, 0]}, "name": "P"}));
    assert!(near(point(&s, "P"), Vec3::new(40.0, 0.0, 0.0)), "{:?}", point(&s, "P"));
}

#[test]
fn point_through_three_planes() {
    let mut s = part();
    run(
        &mut s,
        "construct.point.three_planes",
        json!({"a": {"face": [40, 15, 10]}, "b": {"face": [20, 30, 10]}, "c": {"face": [20, 15, 20]}, "name": "P"}),
    );
    assert!(near(point(&s, "P"), Vec3::new(40.0, 30.0, 20.0)), "{:?}", point(&s, "P"));
}

#[test]
fn point_at_the_centre_of_a_circle_sphere_or_torus() {
    let mut s = part();
    run(&mut s, "construct.point.center", json!({"of": {"edge": [65, 0, 10]}, "name": "C"}));
    assert!(near(point(&s, "C"), Vec3::new(60.0, 0.0, 10.0)), "{:?}", point(&s, "C"));
    run(&mut s, "construct.point.center", json!({"of": {"face": [5, 0, 60]}, "name": "S"}));
    assert!(near(point(&s, "S"), Vec3::new(0.0, 0.0, 60.0)), "{:?}", point(&s, "S"));
    run(&mut s, "construct.point.center", json!({"of": {"face": [12, 60, 0]}, "name": "T"}));
    assert!(near(point(&s, "T"), Vec3::new(0.0, 60.0, 0.0)), "{:?}", point(&s, "T"));
}

#[test]
fn point_at_edge_and_plane() {
    let mut s = part();
    run(
        &mut s,
        "construct.point.edge_plane",
        json!({"edge": {"edge": [20, 0, 0]}, "plane": {"plane": {"origin": [12, 0, 0], "normal": [1, 0, 0]}}, "name": "P"}),
    );
    assert!(near(point(&s, "P"), Vec3::new(12.0, 0.0, 0.0)), "{:?}", point(&s, "P"));
}

#[test]
fn point_along_a_path() {
    let mut s = part();
    run(&mut s, "construct.point.along_path", json!({"path": {"edge": [20, 0, 0]}, "t": 0.25, "name": "P"}));
    let p = point(&s, "P");
    assert!(near(p, Vec3::new(10.0, 0.0, 0.0)) || near(p, Vec3::new(30.0, 0.0, 0.0)), "{p:?}");
    // On an arc it is the arc's own point by arc length, to rounding: a third of a quarter turn
    // of radius 10 is 30° round. A t past either end is refused, not moved onto it.
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Round"}));
    let arc = run(&mut s, "sketch.arc.center_point", json!({"center": [0, 0], "start": [10, 0], "end": [0, 10]}))["created"]["curves"][0]
        .as_str()
        .unwrap()
        .to_string();
    run(&mut s, "sketch.finish", json!({}));
    let path = json!({"sketch": "Round", "curve": arc});
    run(&mut s, "construct.point.along_path", json!({"path": path, "t": "1/3", "name": "Third"}));
    let third = point(&s, "Third");
    let want = Vec3::new(10.0 * (std::f64::consts::PI / 6.0).cos(), 5.0, 0.0);
    assert!(third.dist(want) < 1e-12, "{third:?} is {:e} off", third.dist(want));
    for t in [json!(1.5), json!(-0.25), json!("2")] {
        let e = s.execute("construct.point.along_path", &json!({"path": path, "t": t})).unwrap_err();
        assert!(e.to_string().contains("0 (its start) to 1 (its end)"), "t {t}: {e}");
        assert!(s.execute("construct.plane.along_path", &json!({"path": path, "t": t})).is_err(), "plane at t {t}");
    }
}

#[test]
fn midplane_between_parallel_and_meeting_faces() {
    let mut s = part();
    run(&mut s, "construct.plane.midplane", json!({"a": "XY", "b": {"face": [20, 15, 20]}, "name": "M"}));
    let m = plane(&s, "M");
    assert!(parallel(m.normal(), Vec3::Z) && on_plane(&m, Vec3::new(5.0, 5.0, 10.0)), "{m:?}");
    // Two sides meeting at the corner (40, 30): the diagonal plane through it.
    run(&mut s, "construct.plane.midplane", json!({"a": {"face": [40, 15, 10]}, "b": {"face": [20, 30, 10]}, "name": "Q"}));
    let q = plane(&s, "Q");
    assert!(on_plane(&q, Vec3::new(40.0, 30.0, 0.0)) && on_plane(&q, Vec3::new(41.0, 31.0, 5.0)), "{q:?}");
}

#[test]
fn plane_through_two_edges() {
    let mut s = part();
    run(&mut s, "construct.plane.two_edges", json!({"a": {"edge": [20, 0, 0]}, "b": {"edge": [20, 30, 20]}, "name": "E"}));
    let e = plane(&s, "E");
    // The front-bottom and back-top edges: the slanted plane through both.
    assert!(on_plane(&e, Vec3::new(5.0, 0.0, 0.0)) && on_plane(&e, Vec3::new(5.0, 30.0, 20.0)), "{e:?}");
    assert!(s.execute("construct.plane.two_edges", &json!({"a": {"edge": [20, 0, 0]}, "b": {"edge": [40, 15, 20]}})).is_err(), "skew");
}

#[test]
fn plane_through_three_points() {
    let mut s = part();
    run(
        &mut s,
        "construct.plane.three_points",
        json!({"a": {"vertex": [0, 0, 20]}, "b": {"vertex": [40, 0, 20]}, "c": {"vertex": [40, 30, 0]}, "name": "T"}),
    );
    let t = plane(&s, "T");
    assert!(on_plane(&t, Vec3::new(0.0, 0.0, 20.0)) && on_plane(&t, Vec3::new(40.0, 30.0, 0.0)) && on_plane(&t, Vec3::new(0.0, 30.0, 0.0)), "{t:?}");
}

#[test]
fn plane_tangent_to_a_cylinder() {
    let mut s = part();
    run(&mut s, "construct.plane.tangent", json!({"face": {"face": [65, 0, 5]}, "name": "T"}));
    let t = plane(&s, "T");
    assert!(parallel(t.normal(), Vec3::X) && on_plane(&t, Vec3::new(65.0, 3.0, 9.0)), "{t:?}");
    // And to the sphere, where the pick lies over it.
    run(&mut s, "construct.plane.tangent", json!({"face": {"face": [0, 0, 65]}, "name": "U"}));
    let u = plane(&s, "U");
    assert!(parallel(u.normal(), Vec3::Z) && on_plane(&u, Vec3::new(3.0, 3.0, 65.0)), "{u:?}");
}

#[test]
fn plane_along_a_path() {
    let mut s = part();
    run(&mut s, "construct.plane.along_path", json!({"path": {"edge": [20, 0, 0]}, "t": 0.5, "name": "P"}));
    let p = plane(&s, "P");
    assert!(parallel(p.normal(), Vec3::X) && on_plane(&p, Vec3::new(20.0, 7.0, 3.0)), "{p:?}");
    // A sketch can be placed on it.
    run(&mut s, "sketch.create", json!({"plane": "P", "name": "OnPath"}));
}

#[test]
fn construction_on_a_3d_wire_ignores_its_sketch_plane_and_follows_edits() {
    let mut s = Session::default();
    let sketch = run(&mut s, "sketch.create", json!({"plane":{"origin":[100,200,300],"x_dir":[0,1,0],"y_dir":[0,0,1]}}))["sketch"].as_u64().unwrap();
    run(&mut s, "sketch.options", json!({"sketch_3d":true}));
    let wire = run(&mut s, "sketch.line3d", json!({"points":[[10,20,10],[30,35,40]]}))["curves"][0].as_str().unwrap().to_string();
    run(&mut s, "sketch.finish", json!({}));
    let path = json!({"sketch":sketch,"curve":wire});
    run(&mut s, "construct.plane.along_path", json!({"path":path,"t":0.5,"name":"BridgePlane"}));
    run(&mut s, "construct.point.along_path", json!({"path":path,"t":0.25,"name":"Quarter"}));
    let p = plane(&s, "BridgePlane");
    assert!(near(p.origin, Vec3::new(20.0, 27.5, 25.0)), "{p:?}");
    assert!(parallel(p.normal(), Vec3::new(20.0, 15.0, 30.0)));
    assert!(near(point(&s, "Quarter"), Vec3::new(15.0, 23.75, 17.5)));
    run(&mut s, "sketch.edit", json!({"sketch":sketch}));
    run(&mut s, "sketch.move3d", json!({"wire":wire,"index":1,"to":[50,35,40]}));
    run(&mut s, "sketch.finish", json!({}));
    let updated = plane(&s, "BridgePlane");
    assert!(near(updated.origin, Vec3::new(30.0, 27.5, 25.0)), "{updated:?}");
    assert!(parallel(updated.normal(), Vec3::new(40.0, 15.0, 30.0)));
    let restored = solidcraft_doc::format::read(&s.doc.to_json()).unwrap();
    let mut cold = Session { doc: restored.into(), ..Default::default() };
    cold.refresh();
    assert!(near(plane(&cold, "BridgePlane").origin, updated.origin));
}

#[test]
fn plane_on_a_3d_wire_moves_with_its_component() {
    let mut s = Session::default();
    run(&mut s, "component.create", json!({"name":"Part","grounded":false}));
    let sketch = run(&mut s, "sketch.create", json!({"plane":"XY"}))["sketch"].as_u64().unwrap();
    run(&mut s, "sketch.options", json!({"sketch_3d":true}));
    let wire = run(&mut s, "sketch.line3d", json!({"points":[[0,0,10],[0,0,40]]}))["curves"][0].as_str().unwrap().to_string();
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "construct.plane.along_path", json!({"path":{"sketch":sketch,"curve":wire},"t":0.5,"name":"Section"}));
    run(&mut s, "occurrence.move", json!({"occurrence":"Part:1","translate":[50,20,0],"capture":true}));
    let p = plane(&s, "Section");
    assert!(near(p.origin, Vec3::new(50.0, 20.0, 25.0)), "{p:?}");
    assert!(parallel(p.normal(), Vec3::Z));
}

/// Once a design has a component, the root's constructions are built in the model placed in the
/// world. A plane through a construction axis of the root finds the axis there and follows it
/// (it failed with "unknown construction `Ax`" on every evaluation after the component came).
#[test]
fn a_plane_through_a_construction_axis_follows_it_in_a_design_with_a_component() {
    let mut s = Session::default();
    run(&mut s, "parameters.add", json!({"name": "d", "expression": "10 mm"}));
    run(&mut s, "construct.plane.offset", json!({"base": "XZ", "offset": "d", "name": "P1"}));
    run(&mut s, "construct.plane.offset", json!({"base": "XY", "offset": "5 mm", "name": "P2"}));
    run(&mut s, "construct.axis.two_planes", json!({"a": {"plane": "P1"}, "b": {"plane": "P2"}, "name": "Ax"}));
    run(&mut s, "construct.plane.perpendicular", json!({"plane": "XY", "line": {"axis": "Ax"}, "name": "Q"}));
    run(&mut s, "component.create", json!({"name": "Part"}));
    run(&mut s, "parameters.change", json!({"name": "d", "expression": "30 mm"}));
    let errors: Vec<String> = s.model.results.iter().filter_map(|r| r.error.clone()).collect();
    assert!(errors.is_empty(), "{errors:?}");
    let q = plane(&s, "Q");
    assert!(on_plane(&q, Vec3::new(0.0, 30.0, 5.0)) && parallel(q.normal(), Vec3::Y), "{q:?}");
}

/// Built from model geometry, they follow when the model changes: the box gets taller.
#[test]
fn construction_follows_the_model() {
    let mut s = Session::default();
    run(&mut s, "parameters.add", json!({"name": "h", "expression": "20 mm"}));
    run(&mut s, "solid.box", json!({"length": 40, "width": 30, "height": "h", "body_name": "Box"}));
    run(
        &mut s,
        "construct.point.three_planes",
        json!({"a": {"face": [40, 15, 10]}, "b": {"face": [20, 30, 10]}, "c": {"face": [20, 15, 20]}, "name": "Corner"}),
    );
    run(&mut s, "construct.axis.edge", json!({"edge": {"edge": [20, 30, 20]}, "name": "TopBack"}));
    run(&mut s, "construct.plane.midplane", json!({"a": "XY", "b": {"face": [20, 15, 20]}, "name": "Mid"}));
    run(&mut s, "parameters.change", json!({"name": "h", "expression": "50 mm"}));
    assert!(near(point(&s, "Corner"), Vec3::new(40.0, 30.0, 50.0)), "{:?}", point(&s, "Corner"));
    let (o, d) = axis(&s, "TopBack");
    assert!(parallel(d, Vec3::X) && on_line(o, d, Vec3::new(0.0, 30.0, 50.0)), "{o:?} {d:?}");
    assert!(on_plane(&plane(&s, "Mid"), Vec3::new(1.0, 1.0, 25.0)));
    // Construction from construction: an axis through two construction points.
    run(&mut s, "construct.point.vertex", json!({"point": {"vertex": [0, 0, 0]}, "name": "Origin"}));
    run(&mut s, "construct.axis.two_points", json!({"a": {"construct_point": "Origin"}, "b": {"construct_point": "Corner"}, "name": "Diag"}));
    let (o, d) = axis(&s, "Diag");
    assert!(on_line(o, d, Vec3::new(40.0, 30.0, 50.0)) && on_line(o, d, Vec3::ZERO));
}

/// #568: a plane through three vertices follows them when a parameter moves them. Two of them
/// sit on the box's end at x = 40; the box grows to 100 mm, so the vertices nearest the old
/// picks are the ones at x = 0, on a line with the first: found by the names of the edges that
/// meet there, they are the moved ones.
#[test]
fn a_plane_through_three_vertices_follows_them() {
    let mut s = Session::default();
    run(&mut s, "parameters.add", json!({"name": "len", "expression": "40 mm"}));
    run(&mut s, "solid.box", json!({"length": "len", "width": 30, "height": 20, "body_name": "Box"}));
    run(
        &mut s,
        "construct.plane.three_points",
        json!({"a": {"vertex": [0, 0, 20]}, "b": {"vertex": [40, 0, 20]}, "c": {"vertex": [40, 30, 0]}, "name": "T"}),
    );
    run(&mut s, "parameters.change", json!({"name": "len", "expression": "100 mm"}));
    let id = s.doc.features.iter().find(|f| f.name == "T").map(|f| f.id).unwrap_or_else(|| panic!("feature T"));
    let error = s.model.result(id).and_then(|r| r.error.clone());
    assert_eq!(error, None);
    let t = plane(&s, "T");
    for p in [Vec3::new(0.0, 0.0, 20.0), Vec3::new(100.0, 0.0, 20.0), Vec3::new(100.0, 30.0, 0.0)] {
        assert!(on_plane(&t, p), "{p:?} is off {t:?}");
    }
}

/// A corner of one box where another box's edge runs past it, as where a wall meets the side
/// wall it stands against (the render-studio example, #8650): the vertex stays that corner. Its
/// edges were found by name and then picked again by a point, which took the first edge through
/// the corner of any body, the side wall's, and so its end 3 mm away. Three such corners were
/// "on one line".
#[test]
fn a_vertex_where_another_bodys_edge_runs_past_stays_its_corner() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"corner": [29, -22, 4], "length": 3, "width": 44, "height": 19, "body_name": "Side"}));
    run(&mut s, "solid.box", json!({"corner": [-29, 19, 4], "length": 58, "width": 3, "height": 19, "body_name": "Back"}));
    run(&mut s, "construct.point.vertex", json!({"point": {"vertex": [29, 19, 23]}, "name": "P"}));
    assert!(near(point(&s, "P"), Vec3::new(29.0, 19.0, 23.0)), "{:?}", point(&s, "P"));
    run(
        &mut s,
        "construct.plane.three_points",
        json!({"a": {"vertex": [29, 22, 23]}, "b": {"vertex": [29, 22, 4]}, "c": {"vertex": [29, 19, 23]}, "name": "T"}),
    );
    let t = plane(&s, "T");
    for p in [Vec3::new(29.0, 22.0, 23.0), Vec3::new(29.0, 22.0, 4.0), Vec3::new(29.0, 19.0, 23.0)] {
        assert!(on_plane(&t, p), "{p:?} is off {t:?}");
    }
}

/// In a component moved 50 mm along X, picks given in the world land on its geometry, and the
/// axis and point move with the occurrence.
#[test]
fn construction_lives_in_the_component_frame() {
    let mut s = Session::default();
    run(&mut s, "component.create", json!({"name": "Part", "grounded": false}));
    run(&mut s, "solid.cylinder", json!({"radius": 5, "height": 10, "body_name": "Pin"}));
    run(&mut s, "occurrence.move", json!({"occurrence": "Part:1", "translate": [50, 0, 0]}));
    run(&mut s, "construct.axis.through_cylinder", json!({"face": {"face": [55, 0, 5]}, "name": "PinAxis"}));
    run(&mut s, "construct.point.center", json!({"of": {"edge": [55, 0, 10]}, "name": "PinTop"}));
    let (o, d) = axis(&s, "PinAxis");
    assert!(parallel(d, Vec3::Z) && on_line(o, d, Vec3::new(50.0, 0.0, 0.0)), "{o:?} {d:?}");
    assert!(near(point(&s, "PinTop"), Vec3::new(50.0, 0.0, 10.0)), "{:?}", point(&s, "PinTop"));
    run(&mut s, "occurrence.move", json!({"occurrence": "Part:1", "translate": [0, 20, 0]}));
    assert!(near(point(&s, "PinTop"), Vec3::new(50.0, 20.0, 10.0)), "{:?}", point(&s, "PinTop"));
}

#[test]
fn plane_perpendicular_to_a_plane_through_a_line() {
    let mut s = part();
    // Through the box's front bottom edge (along X), square to the bottom: the front face's plane.
    run(&mut s, "construct.plane.perpendicular", json!({"plane": {"face": [20, 15, 0]}, "line": {"edge": [20, 0, 0]}, "name": "F"}));
    let f = plane(&s, "F");
    assert!(parallel(f.normal(), Vec3::Y) && on_plane(&f, Vec3::new(7.0, 0.0, 13.0)), "{f:?}");
    // Through the world Z axis, square to XZ: the YZ plane.
    run(&mut s, "construct.plane.perpendicular", json!({"plane": "XZ", "line": {"axis": "Z"}, "name": "G"}));
    let g = plane(&s, "G");
    assert!(parallel(g.normal(), Vec3::X) && on_plane(&g, Vec3::new(0.0, 5.0, 9.0)), "{g:?}");
    // A line square to the plane has no single answer.
    assert!(s.execute("construct.plane.perpendicular", &json!({"plane": "XY", "line": {"axis": "Z"}})).is_err());
}

#[test]
fn ambiguous_plane_aliases_refuse_without_document_or_history_changes() {
    for alias in ["Top", "Front", "Right", "XY", "XZ", "YZ"] {
        let mut s = Session::default();
        run(&mut s, "parameters.add", json!({"name":"height","expression":"19.05 mm"}));
        run(&mut s, "sketch.create", json!({"plane":alias}));
        run(&mut s, "sketch.finish", json!({}));
        let made = run(&mut s, "construct.plane.offset", json!({"base":{"origin":[0,0,0],"normal":[0,0,1]},"offset":"height","name":"Lid"}));
        let id = made["feature"].as_u64().unwrap();
        legacy_plane_name(&mut s, id, alias);
        let before = s.doc.to_json();
        let history = s.undo.len();
        for command in ["sketch.create", "inspect.section"] {
            let error = s.execute(command, &json!({"plane":format!(" {} ",alias.to_lowercase())})).unwrap_err();
            assert!(error.to_string().contains("ambiguous"), "{error}");
            assert!(error.to_string().contains(&format!("feature id {id}")), "{error}");
            let other = match alias {
                "Top" => "XY",
                "XY" => "Top",
                "Front" => "XZ",
                "XZ" => "Front",
                "Right" => "YZ",
                _ => "Right",
            };
            assert!(error.to_string().contains(&format!("use `{other}` for the origin plane")), "{error}");
            assert_eq!(s.doc.to_json(), before);
            assert_eq!(s.undo.len(), history);
            assert!(s.section.is_none());
        }
        run(&mut s, "sketch.create", json!({"plane":id}));
        let inspected = run(&mut s, "sketch.inspect", json!({}));
        assert_eq!(inspected["plane"]["origin"], json!([0.0, 0.0, 19.05]));
        let cut = run(&mut s, "inspect.section", json!({"plane":id}));
        assert_eq!(cut["section"]["origin"], json!([0.0, 0.0, 19.05]));
        run(&mut s, "parameters.change", json!({"name":"height","expression":"25.4 mm"}));
        let mut reopened = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
        let old = run(&mut reopened, "sketch.inspect", json!({"sketch":"Sketch1"}));
        assert_eq!(old["plane"]["origin"], json!([0.0, 0.0, 0.0]), "saved origin reference is not reinterpreted");
        let linked = run(&mut reopened, "sketch.inspect", json!({"sketch":"Sketch2"}));
        assert_eq!(linked["plane"]["origin"], json!([0.0, 0.0, 25.4]), "disambiguated reference follows edits and reopen");
    }
}

#[test]
fn disambiguated_top_plane_pocket_survives_undo_and_cold_reopen() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane":"XY"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[100,200]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"distance":19.05,"name":"Plate"}));
    let top = run(&mut s, "construct.plane.offset", json!({"base":"XY","offset":19.05,"name":"Lid"}));
    legacy_plane_name(&mut s, top["feature"].as_u64().unwrap(), "Top");
    assert!(s.execute("sketch.create", &json!({"plane":"Top"})).unwrap_err().to_string().contains("ambiguous"));
    run(&mut s, "sketch.create", json!({"plane":top["feature"]}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0":[3,3],"p1":[97,30]}));
    run(&mut s, "sketch.finish", json!({}));
    let before = s.doc.to_json();
    run(&mut s, "solid.extrude", json!({"distance":12.7,"direction":"negative","operation":"cut","name":"Pocket"}));
    let saved = s.doc.to_json();
    let expected = 100.0 * 200.0 * 19.05 - 94.0 * 27.0 * 12.7;
    let measured = run(&mut s, "inspect.measure", json!({}));
    assert!((measured["total"]["volume_mm3"].as_f64().unwrap() - expected).abs() < 1e-6, "{measured}");
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(s.doc.to_json(), before);
    run(&mut s, "edit.redo", json!({}));
    assert_eq!(s.doc.to_json(), saved);
    let mut reopened = Session::new(solidcraft_doc::Document::from_json(&saved).unwrap());
    let measured = run(&mut reopened, "inspect.measure", json!({}));
    assert!((measured["total"]["volume_mm3"].as_f64().unwrap() - expected).abs() < 1e-6, "{measured}");
}

/// A construction plane named like an origin plane, as an older saved design can hold.
fn legacy_plane_name(s: &mut Session, id: u64, name: &str) {
    s.doc_mut().feature_mut(id).unwrap().name = name.to_string();
}

#[test]
fn construction_planes_cannot_take_origin_plane_names() {
    let mut s = Session::default();
    let made = run(&mut s, "construct.plane.offset", json!({"base":"XY","offset":5,"name":"Lid"}));
    let id = made["feature"].as_u64().unwrap();
    for alias in ["XY", "XZ", "YZ", "Top", "Front", "Right", " top ", "xz"] {
        let before = s.doc.to_json();
        let history = s.undo.len();
        let created = s.execute("construct.plane.offset", &json!({"base":"XY","offset":10,"name":alias})).unwrap_err();
        assert!(created.to_string().contains("reserved for an origin plane"), "{created}");
        let renamed = s.execute("timeline.rename", &json!({"feature":id,"name":alias})).unwrap_err();
        assert!(renamed.to_string().contains("reserved for an origin plane"), "{renamed}");
        assert_eq!(s.doc.to_json(), before);
        assert_eq!(s.undo.len(), history);
    }
    // Other features may still use the names; only a plane parameter could confuse them.
    run(&mut s, "sketch.create", json!({"plane":"XY"}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "timeline.rename", json!({"feature":"Sketch1","name":"Top"}));
    run(&mut s, "timeline.rename", json!({"feature":id,"name":"Topside"}));
}

#[test]
fn ambiguity_error_names_the_other_origin_alias() {
    for (name, other) in [("Top", "XY"), ("XY", "Top"), ("Front", "XZ"), ("XZ", "Front"), ("Right", "YZ"), ("YZ", "Right")] {
        let mut s = Session::default();
        let made = run(&mut s, "construct.plane.offset", json!({"base":"XY","offset":5,"name":"Lid"}));
        legacy_plane_name(&mut s, made["feature"].as_u64().unwrap(), name);
        let error = s.execute("sketch.create", &json!({"plane":name})).unwrap_err().to_string();
        assert!(error.contains(&format!("use `{other}` for the origin plane")), "{error}");
        // The suggested alias is unambiguous and reaches the origin plane.
        run(&mut s, "sketch.create", json!({"plane":other}));
        let inspected = run(&mut s, "sketch.inspect", json!({}));
        assert_eq!(inspected["plane"]["origin"], json!([0.0, 0.0, 0.0]));
    }
}

/// An offset plane through a point instead of at a distance: parallel to its base, through a
/// vertex, a sketch point or a construction point, and moving with the point.
#[test]
fn offset_plane_through_a_point_follows_the_point() {
    let mut s = Session::default();
    run(&mut s, "parameters.add", json!({"name": "h", "expression": "20 mm"}));
    run(&mut s, "solid.box", json!({"length": 40, "width": 30, "height": "h", "body_name": "Box"}));
    // Through the box's top corner, parallel to XY: the top face's plane.
    run(&mut s, "construct.plane.offset", json!({"base": "XY", "through": {"vertex": [40, 30, 20]}, "name": "Lid"}));
    let lid = plane(&s, "Lid");
    assert!(parallel(lid.normal(), Vec3::Z) && near(lid.origin, Vec3::new(0.0, 0.0, 20.0)), "{lid:?}");
    // Through a point of a sketch on XZ, parallel to the box's side face at x = 40.
    let sketch = run(&mut s, "sketch.create", json!({"plane": "XZ"}))["sketch"].as_u64().unwrap();
    run(&mut s, "sketch.point", json!({"point": [70, 5], "id": "mark"}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "construct.plane.offset", json!({"base": {"face": [40, 15, 10]}, "through": {"sketch": sketch, "point": "mark"}, "name": "Side"}));
    let side = plane(&s, "Side");
    assert!(parallel(side.normal(), Vec3::X) && on_plane(&side, Vec3::new(70.0, 9.0, 3.0)), "{side:?}");
    // Through a construction point, parallel to another construction plane.
    run(&mut s, "construct.point.vertex", json!({"point": {"vertex": [0, 30, 20]}, "name": "Corner"}));
    run(&mut s, "construct.plane.offset", json!({"base": "XZ", "offset": 5, "name": "Fore"}));
    run(&mut s, "construct.plane.offset", json!({"base": "Fore", "through": {"construct_point": "Corner"}, "name": "Back"}));
    let back = plane(&s, "Back");
    assert!(parallel(back.normal(), Vec3::Y) && on_plane(&back, Vec3::new(3.0, 30.0, 7.0)), "{back:?}");
    // The box gets taller: the planes through its corners follow.
    run(&mut s, "parameters.change", json!({"name": "h", "expression": "50 mm"}));
    assert!(on_plane(&plane(&s, "Lid"), Vec3::new(1.0, 2.0, 50.0)), "{:?}", plane(&s, "Lid"));
    assert!(on_plane(&plane(&s, "Back"), Vec3::new(1.0, 30.0, 2.0)), "{:?}", plane(&s, "Back"));
    // Saved and opened again, the planes come back where they were.
    let restored = solidcraft_doc::format::read(&s.doc.to_json()).unwrap();
    let mut cold = Session { doc: restored.into(), ..Default::default() };
    cold.refresh();
    assert!(near(plane(&cold, "Lid").origin, plane(&s, "Lid").origin));
    assert!(near(plane(&cold, "Side").origin, plane(&s, "Side").origin));
    // A distance and a point at once is refused.
    assert!(s.execute("construct.plane.offset", &json!({"base": "XY", "offset": 5, "through": {"vertex": [0, 0, 0]}})).is_err());
}

/// Hiding the placement a component is shown at hides its construction geometry too, as it
/// hides its bodies and sketches.
#[test]
fn hiding_an_occurrence_hides_its_construction_geometry() {
    let mut s = Session::default();
    run(&mut s, "component.create", json!({"name": "Part"}));
    run(&mut s, "construct.plane.offset", json!({"base": "XY", "offset": "5 mm", "name": "Shelf"}));
    run(&mut s, "construct.axis.two_planes", json!({"a": "XZ", "b": "YZ", "name": "Spine"}));
    run(&mut s, "construct.point.three_planes", json!({"a": "XY", "b": "XZ", "c": "YZ", "name": "Pin"}));
    let component = s.doc.features.iter().find(|f| f.name == "Shelf").map(|f| f.component).unwrap();
    assert_ne!(component, 0);
    let path = s.doc.first_path(component);
    run(&mut s, "component.activate", json!({"component": 0}));
    for name in ["Shelf", "Spine", "Pin"] {
        assert!(s.construction_visible(name), "{name}");
    }
    run(&mut s, "browser.visibility", json!({"paths": [path], "visible": false}));
    for name in ["Shelf", "Spine", "Pin"] {
        assert!(!s.construction_visible(name), "{name} shows in a hidden placement");
    }
}
