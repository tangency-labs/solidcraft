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
fn linked_sketch_points_keep_separate_capture_poses_through_copy_and_component_moves() {
    let mut s = Session::default();
    run(&mut s, "component.create", json!({"name":"SourcePart", "grounded":false}));
    let source = run(&mut s, "sketch.create", json!({"plane":"XY", "name":"Source"}))["sketch"].as_u64().unwrap();
    run(&mut s, "sketch.circle.center", json!({"center":[2,3], "radius":1}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "occurrence.move", json!({"occurrence":"SourcePart", "translate":[20,0,0], "capture":true}));
    run(&mut s, "component.create", json!({"name":"Part", "parent":"root", "grounded":false}));
    run(&mut s, "occurrence.move", json!({"occurrence":"Part", "translate":[10,0,0], "capture":true}));
    let profile = run(&mut s, "sketch.create", json!({"plane":"XY", "name":"Profile"}))["sketch"].as_u64().unwrap();
    let reference = json!({"refs":[{"sketch":"Source", "point":"c1.center"}]});
    run(&mut s, "sketch.project", reference.clone());
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "occurrence.move", json!({"occurrence":"Part", "translate":[0,5,0], "capture":true}));
    run(&mut s, "sketch.edit", json!({"sketch":"Profile"}));
    run(&mut s, "sketch.project", reference);
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "occurrence.move", json!({"occurrence":"Part", "translate":[0,5,0], "capture":true}));
    run(&mut s, "occurrence.move", json!({"occurrence":"SourcePart", "translate":[7,0,0], "capture":true}));
    let points = |s: &Session, id| {
        let state = s.model.state();
        let ss = state.sketch(id).unwrap();
        let frame = s.doc.component_transform(s.doc.feature(id).unwrap().component);
        ss.sketch
            .links
            .iter()
            .map(|link| {
                assert!(!link.lost);
                let point = ss.sketch.points[ss.sketch.link_points(&link.id)[0]].pos;
                solidcraft_doc::apply_point(&frame, ss.plane.to_world(point))
            })
            .collect::<Vec<_>>()
    };
    let expected = [solidcore_record::Vec3::new(22., 13., 0.), solidcore_record::Vec3::new(22., 8., 0.)];
    let check = |s: &Session, id| {
        let actual = points(s, id);
        assert_eq!(actual.len(), expected.len());
        for (actual, expected) in actual.iter().zip(expected) {
            assert!(actual.dist(expected) < 1e-6, "{actual:?} != {expected:?}");
        }
    };
    check(&s, profile);
    run(&mut s, "timeline.copy", json!({"features":["Profile"]}));
    run(&mut s, "timeline.paste", json!({"component":"root"}));
    let copied = s.doc.features.last().unwrap().id;
    check(&s, copied);
    // Moving either definition changes local frames, not the geometry the captured reference means.
    run(&mut s, "component.move_features", json!({"features":[source], "component":"Part"}));
    check(&s, profile);
    check(&s, copied);
    run(&mut s, "component.move_features", json!({"features":[profile], "component":"root"}));
    check(&s, profile);
    let reopened = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
    check(&reopened, profile);
    check(&reopened, copied);
}

#[test]
fn moving_or_pasting_a_root_sketch_into_a_part_captures_new_foreign_references() {
    for paste in [false, true] {
        let mut s = Session::default();
        run(&mut s, "sketch.create", json!({"plane":"XY", "name":"Source"}));
        run(&mut s, "sketch.circle.center", json!({"center":[10,0], "radius":2}));
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "construct.plane.offset", json!({"base":"XY", "offset":0, "name":"Guide"}));
        let mut profile = run(&mut s, "sketch.create", json!({"plane":"Guide", "name":"Profile"}))["sketch"].as_u64().unwrap();
        run(&mut s, "sketch.project", json!({"refs":[{"sketch":"Source", "curve":"c1"}]}));
        run(&mut s, "sketch.circle.center", json!({"center":[5,0], "radius":1}));
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "component.create", json!({"name":"Part", "grounded":false}));
        if paste {
            run(&mut s, "timeline.copy", json!({"features":["Profile"]}));
            run(&mut s, "timeline.paste", json!({"component":"Part"}));
            profile = s.doc.features.last().unwrap().id;
        } else {
            run(&mut s, "component.move_features", json!({"features":["Profile"], "component":"Part"}));
        }
        run(&mut s, "joint.as_built", json!({"type":"revolute", "name":"Hinge", "a":0, "b":"Part"}));
        run(&mut s, "joint.drive", json!({"joint":"Hinge", "value":90}));
        let check = |s: &Session| {
            let state = s.model.state();
            let ss = state.sketch(profile).unwrap();
            let frame = s.doc.component_transform(s.doc.feature(profile).unwrap().component);
            for (curve, y) in ss.sketch.curves.iter().zip([10., 5.]) {
                let solidcraft_sketch::CurveKind::Circle { c, .. } = curve.kind else { panic!("expected circle") };
                let center = solidcraft_doc::apply_point(&frame, ss.plane.to_world(ss.sketch.points[c].pos));
                assert!(center.dist(solidcore_record::Vec3::new(0., y, 0.)) < 1e-6, "paste={paste}: {center:?}");
            }
        };
        check(&s);
        check(&Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap()));
    }
}

#[test]
fn pasted_sketch_reference_uses_a_sources_pending_placement() {
    let mut s = Session::default();
    run(&mut s, "component.create", json!({"name":"SourcePart", "grounded":false}));
    run(&mut s, "sketch.create", json!({"plane":"XY", "name":"Source"}));
    run(&mut s, "sketch.circle.center", json!({"center":[2,3], "radius":1}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "component.activate", json!({"component":"root"}));
    run(&mut s, "sketch.create", json!({"plane":"XY", "name":"Profile"}));
    run(&mut s, "sketch.project", json!({"refs":[{"sketch":"Source", "point":"c1.center"}]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "component.create", json!({"name":"Part", "parent":"root", "grounded":false}));
    run(&mut s, "occurrence.move", json!({"occurrence":"SourcePart", "translate":[7,0,0], "capture":false}));
    assert!(!s.pending_moves.is_empty());
    run(&mut s, "timeline.copy", json!({"features":["Profile"]}));
    run(&mut s, "timeline.paste", json!({"component":"Part"}));
    let id = s.doc.features.last().unwrap().id;
    let state = s.model.state();
    let ss = state.sketch(id).unwrap();
    let link = &ss.sketch.links[0];
    let point = ss.plane.to_world(ss.sketch.points[ss.sketch.link_points(&link.id)[0]].pos);
    assert!(point.dist(solidcore_record::Vec3::new(9., 3., 0.)) < 1e-6, "{point:?}");
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
fn projection_from_a_later_sketch_explains_timeline_order_without_changes() {
    for restore_marker in [Value::Null, json!(false)] {
        let mut s = Session::default();
        let earlier = run(&mut s, "sketch.create", json!({"plane":"XY", "name":"Earlier"}))["sketch"].clone();
        run(&mut s, "sketch.finish", json!({}));
        let later = run(&mut s, "sketch.create", json!({"plane":"XY", "name":"Later"}))["sketch"].clone();
        run(&mut s, "sketch.line", json!({"points":[[0,0],[20,10]], "ids":["source_line"]}));
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "sketch.edit", json!({"sketch":earlier, "restore_marker":restore_marker}));
        let before = s.doc.to_json();
        let undo = s.undo.len();
        for key in [json!("Later"), later] {
            for entity in [json!({"sketch":key, "curve":"source_line"}), json!({"sketch":key, "point":"source_line.start"})] {
                for command in ["sketch.project", "sketch.intersect", "sketch.include_3d"] {
                    let error = s.execute(command, &json!({"refs":["origin", entity]})).unwrap_err().to_string();
                    assert!(error.contains("sketch `Later` comes after this sketch in the timeline"), "{command}: {error}");
                    assert!(error.contains("Move the source sketch earlier if its dependencies allow, or use a sketch after it"), "{error}");
                    assert!(!error.contains("no longer exists"), "{error}");
                    assert_eq!(s.doc.to_json(), before, "{command} must not change the document");
                    assert_eq!(s.undo.len(), undo, "{command} must not add an undo step");
                    assert_eq!(s.active_sketch, earlier.as_u64());
                }
            }
        }
        for (command, params) in [
            ("sketch.project_to_surface", json!({"curves":[{"sketch":"Later", "curve":"source_line"}], "face":[0,0,0]})),
            ("sketch.intersection_curve", json!({"a":{"sketch":"Later", "curve":"source_line"}, "b":{"body":"unused"}})),
        ] {
            let error = s.execute(command, &params).unwrap_err().to_string();
            assert!(error.contains("sketch `Later` comes after this sketch in the timeline"), "{command}: {error}");
            assert_eq!(s.doc.to_json(), before);
            assert_eq!(s.undo.len(), undo);
        }
    }
}

#[test]
fn projection_uses_timeline_order_after_reordering_sketches() {
    let mut s = Session::default();
    let target = run(&mut s, "sketch.create", json!({"plane":"XY", "name":"Target"}))["sketch"].clone();
    run(&mut s, "sketch.finish", json!({}));
    let source = run(&mut s, "sketch.create", json!({"plane":"XY", "name":"Source"}))["sketch"].clone();
    run(&mut s, "sketch.line", json!({"points":[[2,3],[20,10]], "ids":["source_line"]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "timeline.reorder", json!({"feature":source, "position":0}));
    run(&mut s, "sketch.edit", json!({"sketch":target}));
    let projected = run(&mut s, "sketch.project", json!({"refs":[{"sketch":source, "curve":"source_line"}]}));
    assert_eq!(projected["links"][0]["curves"].as_array().unwrap().len(), 1);
    let sketch = s.doc.sketch(target.as_u64().unwrap()).unwrap();
    assert_eq!(sketch.links.len(), 1);
    assert!(!sketch.links[0].lost);
    assert_eq!(sketch.links[0].source, solidcraft_sketch::LinkSource::SketchCurve { sketch: source.as_u64().unwrap(), curve: "source_line".into() });
}

#[test]
fn projection_keeps_missing_and_self_reference_errors_distinct() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane":"XY", "name":"Target"}));
    for (reference, expected) in [
        (json!({"sketch":"Missing", "curve":"line"}), "no sketch `Missing`"),
        (json!({"sketch":"Target", "curve":"line"}), "a sketch cannot project its own geometry"),
    ] {
        let before = s.doc.to_json();
        let error = s.execute("sketch.project", &json!({"refs":[reference]})).unwrap_err().to_string();
        assert!(error.contains(expected), "{error}");
        assert!(!error.contains("comes after"), "{error}");
        assert_eq!(s.doc.to_json(), before);
    }
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

/// The linked points of an Intersect with edge `at` of `body` on the plane through `origin`
/// spanned by `x` and `y`, in that plane's coordinates.
fn intersected_edge(s: &mut Session, body: &str, at: [f64; 3], (origin, x, y): ([f64; 3], [f64; 3], [f64; 3])) -> Vec<[f64; 2]> {
    run(s, "sketch.create", json!({"plane":{"origin":origin,"x_dir":x,"y_dir":y}}));
    run(s, "sketch.intersect", json!({"refs":[{"edge":at,"body":body}]}));
    let si = inspect(s);
    let points = si["points"].as_array().unwrap().iter().filter(|p| !p["link"].is_null());
    let out = points.map(|p| [p["at"][0].as_f64().unwrap(), p["at"][1].as_f64().unwrap()]).collect();
    run(s, "sketch.finish", json!({}));
    out
}

/// Intersect put a curved edge's points where its display mesh crosses the plane, on a chord:
/// 0.011 mm inside a radius-10 circle (#19867). A plane through the circle's seam still gives
/// its two points once each.
#[test]
fn intersecting_a_circular_edge_puts_its_points_on_the_circle() {
    let mut s = Session::default();
    run(&mut s, "solid.cylinder", json!({"radius":10,"height":10,"body_name":"C"}));
    // The plane X = 3 meets the top circle at Y = ±√(10² − 3²).
    let mut got = intersected_edge(&mut s, "C", [0.0, 10.0, 10.0], ([3.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]));
    got.sort_by(|a, b| a[0].total_cmp(&b[0]));
    assert_eq!(got.len(), 2, "{got:?}");
    for (p, y) in got.iter().zip([-(91f64.sqrt()), 91f64.sqrt()]) {
        assert!((p[0] - y).abs() < 1e-9 && (p[1] - 10.0).abs() < 1e-9, "{got:?}");
    }
    // The plane Y = 0 meets it at X = ±10, one of them where the closed edge starts and ends.
    let mut got = intersected_edge(&mut s, "C", [0.0, 10.0, 10.0], ([0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]));
    got.sort_by(|a, b| a[0].total_cmp(&b[0]));
    assert_eq!(got.len(), 2, "{got:?}");
    for (p, x) in got.iter().zip([-10.0, 10.0]) {
        assert!((p[0] - x).abs() < 1e-9 && (p[1] - 10.0).abs() < 1e-9, "{got:?}");
    }
}

/// An edge where two cylinders meet has no closed form: Intersect crosses the spline the kernel
/// holds for it, so the points lie on both cylinders within the edge's tolerance.
#[test]
fn intersecting_where_two_cylinders_meet_puts_the_points_on_both() {
    let mut s = Session::default();
    run(&mut s, "solid.cylinder", json!({"radius":10,"height":40,"body_name":"C"}));
    run(&mut s, "solid.cylinder", json!({"radius":5,"height":40,"base":[-20,0,20],"axis":"X","operation":"join","targets":["C"]}));
    // The kernel holds each meeting curve as a spline within its edge's tolerance of both cylinders.
    let state = s.model.state();
    let meeting: Vec<f64> = state
        .body("C")
        .unwrap()
        .body
        .record()
        .edges
        .iter()
        .filter(|e| matches!(e.curve, solidcore_record::brep::Curve::Spline { .. }))
        .map(|e| e.tolerance)
        .collect();
    assert_eq!(meeting.len(), 2, "the cylinders meet along two splines");
    let tol = meeting.iter().fold(0.0, |m: f64, t| m.max(*t));
    drop(state);
    // The plane Y = 3 meets the meeting curve on the +X side at X = √(10² − 3²), Z = 20 ± √(5² − 3²).
    let mut got = intersected_edge(&mut s, "C", [91f64.sqrt(), 3.0, 24.0], ([0.0, 3.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]));
    got.sort_by(|a, b| a[1].total_cmp(&b[1]));
    assert_eq!(got.len(), 2, "{got:?}");
    for (p, z) in got.iter().zip([16.0, 24.0]) {
        let (on_upright, on_cross) = ((p[0].powi(2) + 9.0).sqrt() - 10.0, (9.0 + (p[1] - 20.0).powi(2)).sqrt() - 5.0);
        assert!(on_upright.abs() <= tol && on_cross.abs() <= tol && (p[1] - z).abs() < 0.5, "{got:?}, edge tolerance {tol}");
    }
}

/// An edge lying in the sketch plane meets it along its whole length: Intersect gives the edge
/// itself (the old mesh section gave one end of a straight edge and every mesh vertex of a circle).
#[test]
fn intersecting_an_edge_lying_in_the_plane_gives_the_edge() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length":40,"width":30,"height":20,"body_name":"Post"}));
    run(&mut s, "solid.cylinder", json!({"radius":10,"height":10,"base":[100,0,0],"body_name":"C"}));
    for (body, at, height) in [("Post", [20.0, 0.0, 20.0], 20), ("C", [100.0, 10.0, 10.0], 10)] {
        run(&mut s, "sketch.create", json!({"plane":"XY","offset":height}));
        let r = run(&mut s, "sketch.intersect", json!({"refs":[{"edge":at,"body":body}]}));
        let si = inspect(&mut s);
        let curves = linked_curves(&si);
        assert_eq!(curves.len(), 1, "{body}: {r} {si}");
        let c = &curves[0];
        if body == "Post" {
            assert_eq!(c["type"], "line", "{c}");
            let mut xs = [c["start_at"][0].as_f64().unwrap(), c["end_at"][0].as_f64().unwrap()];
            xs.sort_by(f64::total_cmp);
            assert!((xs[0]).abs() < 1e-9 && (xs[1] - 40.0).abs() < 1e-9, "{c}");
            assert!(c["start_at"][1].as_f64().unwrap().abs() < 1e-9 && c["end_at"][1].as_f64().unwrap().abs() < 1e-9, "{c}");
        } else {
            assert_eq!(c["type"], "circle", "{c}");
            assert!((c["radius"].as_f64().unwrap() - 10.0).abs() < 1e-9, "{c}");
        }
        run(&mut s, "sketch.finish", json!({}));
    }
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

/// The edge a projection was made from goes with its body: the sketch fails, naming the
/// projection by its body and curve, instead of keeping a lost link.
#[test]
fn a_reference_whose_body_is_gone_fails_its_sketch_naming_the_lost_projection() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10}));
    let boxf = s.doc.features.last().unwrap().id;
    let box_name = s.doc.features.last().unwrap().name.clone();
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    run(&mut s, "sketch.project", json!({"refs": [{"edge": [5, 0, 10]}]}));
    run(&mut s, "sketch.finish", json!({}));
    let name = s.doc.sketch(s.doc.features.last().unwrap().id).unwrap().links[0].edge_name.clone().unwrap();
    run(&mut s, "timeline.suppress", json!({"feature": boxf, "suppressed": true}));
    let r = s.model.results.last().unwrap();
    let error = r.error.as_deref().unwrap_or_default();
    let read = solidcraft_doc::lost_projection_parts(error).map(|t| (t.feature, t.count, t.listed));
    // The suppressed box is still in the design: the message names it as where the edge came from.
    let listed = format!("projected edge of Body1 (curve l1; made by {box_name})");
    assert_eq!(read, Some((r.name.as_str(), 1, listed.as_str())), "{r:?}");
    assert!(!error.contains(&name), "{error}");
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

/// A 60 x 40 x 12 mm plate with its four upright edges rounded (radius 10), a boss of radius 12
/// on it up to z = 30, a bore of radius 7 through both and two bolt holes of radius 3: the body
/// `Mount`.
fn mount(s: &mut Session) {
    for (id, p) in [
        ("solid.box", json!({"length": 60, "width": 40, "height": 12, "body_name": "Mount"})),
        ("solid.fillet", json!({"edges": [[0, 0, 6], [60, 0, 6], [0, 40, 6], [60, 40, 6]], "radius": 10})),
        ("solid.cylinder", json!({"base": [30, 20, 12], "radius": 12, "height": 18, "operation": "join"})),
        ("solid.cylinder", json!({"base": [30, 20, -1], "radius": 7, "height": 40, "operation": "cut"})),
        ("solid.cylinder", json!({"base": [10, 20, -1], "radius": 3, "height": 20, "operation": "cut"})),
        ("solid.cylinder", json!({"base": [50, 20, -1], "radius": 3, "height": 20, "operation": "cut"})),
    ] {
        run(s, id, p);
    }
}

/// A design whose only feature imports one mesh body, `Mesh`, with no shading normals (as an STL
/// has none).
fn mesh_design(positions: Vec<f64>, triangles: Vec<u32>) -> Session {
    let mut doc = solidcraft_doc::Document::default();
    let mesh = solidcraft_doc::MeshData {
        name: "Mesh".into(),
        positions: positions.into(),
        triangles: triangles.into(),
        color: None,
        corner_normals: Default::default(),
    };
    doc.add_feature(solidcraft_doc::FeatureKind::MeshImport { file: "shapes.stl".into(), meshes: vec![mesh] }, Some("Imported mesh")).unwrap();
    Session::new(doc)
}

/// Fit Curves to Mesh Section on a new sketch `z` mm above XY, then the sketch as inspected.
fn fit_section_at(s: &mut Session, body: &str, z: f64) -> Value {
    run(s, "sketch.create", json!({"plane": "XY", "offset": format!("{z} mm")}));
    run(s, "sketch.fit_curves_to_section", json!({"body": body}));
    let si = inspect(s);
    run(s, "sketch.finish", json!({}));
    si
}

fn at(v: &Value) -> [f64; 2] {
    [v[0].as_f64().unwrap(), v[1].as_f64().unwrap()]
}

/// Whether every line and arc of a sketch ends where another of its curves ends: the outlines
/// the fit made are closed.
fn ends_meet(si: &Value) -> bool {
    let curves = si["curves"].as_array().unwrap();
    let ends: Vec<(usize, [f64; 2])> = curves
        .iter()
        .enumerate()
        .filter(|(_, c)| c["type"] == "line" || c["type"] == "arc")
        .flat_map(|(i, c)| [(i, at(&c["start_at"])), (i, at(&c["end_at"]))])
        .collect();
    ends.iter().all(|(i, e)| ends.iter().any(|(j, f)| i != j && (e[0] - f[0]).hypot(e[1] - f[1]) < 1e-9))
}

/// Checks a section of [`mount`] through its plate (0 < z < 12) against the part, every curve
/// within `tol` of it: the four sides, the four corner rounds and the three holes, closed, with
/// the plate's area and the holes'.
fn assert_plate_section(si: &Value, tol: f64) {
    let curves = si["curves"].as_array().unwrap();
    let of = |kind: &str| curves.iter().filter(|c| c["type"] == kind).cloned().collect::<Vec<_>>();
    let (lines, arcs, circles) = (of("line"), of("arc"), of("circle"));
    assert_eq!((lines.len(), arcs.len(), circles.len(), curves.len()), (4, 4, 3, 11), "{si}");
    let near = |p: [f64; 2], q: [f64; 2]| (p[0] - q[0]).hypot(p[1] - q[1]) <= tol;
    let sides = [[[0., 10.], [0., 30.]], [[60., 10.], [60., 30.]], [[10., 0.], [50., 0.]], [[10., 40.], [50., 40.]]];
    for side in sides {
        let found = lines.iter().any(|l| {
            let (a, b) = (at(&l["start_at"]), at(&l["end_at"]));
            (near(a, side[0]) && near(b, side[1])) || (near(a, side[1]) && near(b, side[0]))
        });
        assert!(found, "no side {side:?} in {si}");
    }
    for c in [[10., 10.], [50., 10.], [50., 30.], [10., 30.]] {
        assert!(arcs.iter().any(|a| near(at(&a["center_at"]), c) && (a["radius"].as_f64().unwrap() - 10.).abs() <= tol), "no round at {c:?} in {si}");
    }
    for (c, r) in [([10., 20.], 3.), ([30., 20.], 7.), ([50., 20.], 3.)] {
        assert!(circles.iter().any(|h| near(at(&h["center_at"]), c) && (h["radius"].as_f64().unwrap() - r).abs() <= tol), "no hole at {c:?} in {si}");
    }
    assert!(ends_meet(si), "{si}");
    // The plate, rounded and less its holes, and the holes: each within its outline's length
    // times the tolerance.
    let pi = std::f64::consts::PI;
    let mut areas: Vec<f64> = si["profiles"].as_array().unwrap().iter().map(|p| p["area_mm2"].as_f64().unwrap()).collect();
    areas.sort_by(f64::total_cmp);
    let want = [9. * pi, 9. * pi, 49. * pi, 2000. + 100. * pi - 67. * pi];
    let lengths = [6. * pi, 6. * pi, 14. * pi, 120. + 20. * pi];
    assert_eq!(areas.len(), 4, "{si}");
    for ((got, want), length) in areas.iter().zip(want).zip(lengths) {
        assert!((got - want).abs() <= length * tol, "area {got} vs {want} in {si}");
    }
}

/// An STL carries no shading normals, so every facet of its mesh body was shaded flat and the
/// section took each vertex of a round for a crease: its crossings fell on the facets' chords,
/// inside the surface by up to the mesh's sag, and the fit broke each round and hole into
/// hundreds of short lines and could leave the outline open. The section now takes the surface
/// from the facets, and fits it to their sag: the curves are the part's own, within the export's
/// chord (2e-4 of the part's 78 mm box diagonal) of it.
#[test]
fn an_stl_section_is_fitted_with_the_parts_lines_arcs_and_circles() {
    let dir = crate::tests::Folder::new("fit-stl-section");
    let stl = dir.path("mount.stl");
    let mut src = Session::default();
    mount(&mut src);
    run(&mut src, "file.export_stl", json!({ "path": stl }));
    let mut s = Session::default();
    run(&mut s, "file.insert_mesh", json!({ "path": stl }));
    let body = s.model.state().bodies[0].name.clone();
    let chord = 2e-4 * (60f64.powi(2) + 40f64.powi(2) + 30f64.powi(2)).sqrt();
    for z in [3.0, 6.0, 10.0] {
        assert_plate_section(&fit_section_at(&mut s, &body, z), chord);
    }
    // Through the boss: two circles, its wall and the bore.
    let si = fit_section_at(&mut s, &body, 20.0);
    let mut radii: Vec<f64> =
        si["curves"].as_array().unwrap().iter().filter(|c| c["type"] == "circle").map(|c| c["radius"].as_f64().unwrap()).collect();
    radii.sort_by(f64::total_cmp);
    assert_eq!(si["curves"].as_array().unwrap().len(), 2, "{si}");
    assert!(radii.len() == 2 && (radii[0] - 7.).abs() <= chord && (radii[1] - 12.).abs() <= chord, "{si}");
}

/// How far `p` lies from [`mount`]'s outline through its plate: its rounded rectangle, or the
/// nearest of its three holes.
fn off_the_plate(p: [f64; 2]) -> f64 {
    let (x, y) = (p[0], p[1]);
    let (cx, cy) = (x.clamp(10., 50.), y.clamp(10., 30.));
    let outer = if x != cx && y != cy {
        ((x - cx).hypot(y - cy) - 10.).abs()
    } else {
        [x.abs(), (60. - x).abs(), y.abs(), (40. - y).abs()].into_iter().fold(f64::MAX, f64::min)
    };
    [([10., 20.], 3.), ([30., 20.], 7.), ([50., 20.], 3.)].iter().map(|(c, r)| ((x - c[0]).hypot(y - c[1]) - r).abs()).fold(outer, f64::min)
}

/// A scan's mesh is noisy: the mount's mesh with each vertex moved up to `amp` mm along each axis
/// (by a fixed formula of where it is, so the copies of a vertex the display mesh keeps for each
/// face move alike and the mesh stays closed), as a mesh body without normals.
fn noisy_mount(amp: f64) -> Session {
    let mut src = Session::default();
    mount(&mut src);
    let mesh = src.model.state().bodies[0].body.mesh(&solidcraft_kernel::MeshTolerance::chord(0.01)).unwrap();
    let jitter = |p: solidcore_record::Vec3, axis: f64| {
        let x = (p.x * 12.9898 + p.y * 78.233 + p.z * 37.719 + axis * 4.581).sin() * 43758.5453;
        (x - x.floor()) * 2.0 - 1.0
    };
    let positions =
        mesh.positions.iter().flat_map(|p| [p.x + amp * jitter(*p, 0.), p.y + amp * jitter(*p, 1.), p.z + amp * jitter(*p, 2.)]).collect();
    mesh_design(positions, mesh.triangles.iter().flatten().copied().collect())
}

/// A noisy mesh's section keeps its outlines closed, the holes circles, every curve within its
/// noise of the part: each vertex lies within amp·√3 of the surface plus the mesh's 0.01 mm
/// chord, the lines run between points of the section and the arcs and circles are
/// least-squares fits through them. Noise tilts the normals of a round's narrow facets, so its
/// surface is not taken from them: the rounds come out as short lines, as before.
#[test]
fn a_noisy_mesh_section_keeps_its_outlines_closed() {
    for amp in [0.01, 0.05] {
        let mut s = noisy_mount(amp);
        let tol = amp * 3f64.sqrt() + 0.01;
        for z in [3.0, 6.0, 9.0] {
            let si = fit_section_at(&mut s, "Mesh", z);
            assert!(ends_meet(&si) && si["profiles"].as_array().unwrap().len() == 4, "{amp} mm at z {z}: {si}");
            let curves = si["curves"].as_array().unwrap();
            let of = |kind: &str| curves.iter().filter(|c| c["type"] == kind).collect::<Vec<_>>();
            let circles = of("circle");
            assert_eq!(circles.len(), 3, "{amp} mm at z {z}: {si}");
            for c in curves {
                let ends: Vec<[f64; 2]> = match c["type"].as_str() {
                    Some("circle") => {
                        let (o, r) = (at(&c["center_at"]), c["radius"].as_f64().unwrap());
                        vec![[o[0] + r, o[1]], [o[0], o[1] + r], [o[0] - r, o[1]], [o[0], o[1] - r]]
                    }
                    _ => vec![at(&c["start_at"]), at(&c["end_at"])],
                };
                assert!(ends.iter().all(|e| off_the_plate(*e) <= tol), "{amp} mm at z {z}: {c}");
            }
        }
    }
}

/// A prism of radius 7 and 30 mm tall with `n` sides, as a mesh without normals: its sides, a
/// quad of two triangles each, and a fan at each end.
fn prism(n: u32) -> Session {
    let ring = |z: f64| (0..n).map(move |k| f64::from(k) * std::f64::consts::TAU / f64::from(n)).flat_map(move |a| [7. * a.cos(), 7. * a.sin(), z]);
    let positions: Vec<f64> = ring(0.).chain(ring(30.)).chain([0., 0., 0., 0., 0., 30.]).collect();
    let (bottom, top) = (2 * n, 2 * n + 1);
    let mut triangles = Vec::new();
    for k in 0..n {
        let j = (k + 1) % n;
        triangles.extend([k, j, n + j, k, n + j, n + k, bottom, j, k, top, n + k, n + j]);
    }
    mesh_design(positions, triangles)
}

/// A slot's outline as a mesh without normals, 30 mm tall: a 30 mm bottom, a round of radius 3
/// that runs on tangentially into one of radius 6, the top, and a flat left side; each side a
/// quad of two triangles, a fan at each end. Its rounds' facets stray 3·(1 − cos 2.25°) = 0.0023
/// mm from them, within an STL export's chord.
fn slot_with_two_rounds() -> Session {
    let mut outline: Vec<[f64; 2]> = vec![[0., 0.]];
    let arc = |c: [f64; 2], r: f64, from: f64| {
        (0..20).map(move |k| (from + 4.5 * f64::from(k)).to_radians()).map(move |a| [c[0] + r * a.cos(), c[1] + r * a.sin()])
    };
    outline.extend(arc([30., 3.], 3., -90.));
    outline.extend(arc([27., 3.], 6., 0.));
    outline.extend([[27., 9.], [0., 9.]]);
    let n = u32::try_from(outline.len()).unwrap();
    let ring = |z: f64| outline.clone().into_iter().flat_map(move |[x, y]| [x, y, z]);
    let positions: Vec<f64> = ring(0.).chain(ring(30.)).chain([15., 4.5, 0., 15., 4.5, 30.]).collect();
    let (bottom, top) = (2 * n, 2 * n + 1);
    let mut triangles = Vec::new();
    for k in 0..n {
        let j = (k + 1) % n;
        triangles.extend([k, j, n + j, k, n + j, n + k, bottom, j, k, top, n + k, n + j]);
    }
    mesh_design(positions, triangles)
}

/// Where two rounds of a mesh without normals meet, each arc ends on its own fitted circle, a
/// few microns from the other's end: the outline did not close, and the sketch had nothing to
/// extrude. The two ends now meet, and each arc keeps its radius.
#[test]
fn a_mesh_section_closes_where_two_rounds_meet() {
    let si = fit_section_at(&mut slot_with_two_rounds(), "Mesh", 10.0);
    assert!(ends_meet(&si), "{si}");
    let radii: Vec<f64> = si["curves"].as_array().unwrap().iter().filter(|c| c["type"] == "arc").map(|c| c["radius"].as_f64().unwrap()).collect();
    assert!([3., 6.].iter().all(|r| radii.iter().any(|q| (q - r).abs() < 0.01)), "{si}");
}

/// A prism's mesh cut across. With 48 sides it stands for a cylinder: its facets stray from one
/// by 7·(1 − cos 3.75°) = 0.015 mm, within an STL export's chord (2e-4 of its 36.2 mm box
/// diagonal) of the surface the section takes from them, and its section is one circle through
/// the corners, exactly radius 7 (they lie on the surface). With 12 sides (0.24 mm off a circle)
/// or 8 (sides turning 45 degrees, a crease) it is the polygon it is: a line on each side
/// between its corners.
#[test]
fn a_mesh_without_normals_is_round_only_where_its_facets_follow_a_round_closely() {
    let si = fit_section_at(&mut prism(48), "Mesh", 10.0);
    let c = &si["curves"][0];
    assert!(si["curves"].as_array().unwrap().len() == 1 && c["type"] == "circle", "{si}");
    assert!((c["radius"].as_f64().unwrap() - 7.).abs() < 1e-6 && at(&c["center_at"]).iter().all(|x| x.abs() < 1e-6), "{c}");
    for n in [12u32, 8] {
        let si = fit_section_at(&mut prism(n), "Mesh", 10.0);
        let curves = si["curves"].as_array().unwrap();
        assert!(curves.len() == n as usize && curves.iter().all(|c| c["type"] == "line"), "{n} sides: {si}");
        let step = std::f64::consts::TAU / f64::from(n);
        let corner = |k: u32| [7. * (f64::from(k) * step).cos(), 7. * (f64::from(k) * step).sin()];
        for c in curves {
            for end in [at(&c["start_at"]), at(&c["end_at"])] {
                assert!((0..n).any(|k| (end[0] - corner(k)[0]).hypot(end[1] - corner(k)[1]) < 1e-9), "{n} sides: {c}");
            }
        }
        assert!(ends_meet(&si), "{n} sides: {si}");
    }
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
fn projection_fidelity_keeps_the_full_edge_on_spline_interval() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Source"}));
    run(&mut s, "sketch.spline.control_point", json!({"points": [[0,0], [90,10], [17,20], [0,30]]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "sketch.create", json!({"plane": "XZ"}));
    run(&mut s, "sketch.project", json!({"refs": [{"sketch": "Source", "curve": "s1"}]}));
    let result = inspect(&mut s);
    let curves = result["curves"].as_array().unwrap();
    assert_eq!(curves.len(), 1, "{result}");
    assert_eq!(curves[0]["type"], "line", "{result}");
    let t = (978.0 - f64::sqrt(978.0 * 978.0 - 4.0 * 657.0 * 270.0)) / (2.0 * 657.0);
    let maximum = 270.0 * t - 489.0 * t * t + 219.0 * t * t * t;
    let ends = [&curves[0]["start_at"], &curves[0]["end_at"]];
    let hi = ends.iter().map(|p| p[0].as_f64().unwrap()).fold(f64::NEG_INFINITY, f64::max);
    let lo = ends.iter().map(|p| p[0].as_f64().unwrap()).fold(f64::INFINITY, f64::min);
    assert!((hi - maximum).abs() < 1e-8 && lo.abs() < 1e-8, "expected [0, {maximum}], got [{lo}, {hi}]");
}

#[test]
fn projection_fidelity_keeps_a_large_fit_spline_exact() {
    let mut s = Session::default();
    let points: Vec<[f64; 2]> =
        (0..168).map(|i| [10.0 * f64::from(i) / 167.0, 5.0 * (3.0 * std::f64::consts::PI * f64::from(i) / 167.0).sin()]).collect();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Source"}));
    run(&mut s, "sketch.spline.fit_point", json!({"points": points}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "sketch.create", json!({"plane": {"x_dir": [0.8,0,0.6], "y_dir": [0,1,0]}}));
    run(&mut s, "sketch.project", json!({"refs": [{"sketch": "Source", "curve": "s1"}], "projection_link": false}));
    let result = inspect(&mut s);
    let curves = result["curves"].as_array().unwrap();
    assert_eq!(curves.len(), 1, "the fit spline must stay one exact spline, got {} curves", curves.len());
    assert_eq!(curves[0]["type"], "spline", "{result}");
    let st = s.model.state();
    let sk = &st.sketch(s.active_sketch.unwrap()).unwrap().sketch;
    let ci = sk.curve_index(curves[0]["id"].as_str().unwrap()).unwrap();
    let solidcraft_sketch::CurveKind::Spline { pts, degree, nurbs: Some(nurbs), .. } = &sk.curves[ci].kind else { panic!("expected exact NURBS") };
    let controls: Vec<_> = pts.iter().map(|p| sk.point(*p).unwrap()).collect();
    let h: Vec<_> = points.windows(2).map(|w| f64::hypot(w[1][0] - w[0][0], w[1][1] - w[0][1])).collect();
    let total: f64 = h.iter().sum();
    let mut distance = 0.0;
    for (i, [x, y]) in points.iter().enumerate() {
        let actual = nurbs.point(&controls, *degree, distance / total).unwrap();
        let expected = solidcore_record::Vec2::new(0.8 * x, *y);
        assert!(actual.dist(expected) < 1e-8, "fit point {i}: {actual:?} != {expected:?}");
        distance += h.get(i).copied().unwrap_or(0.0);
    }
}

#[test]
fn projection_fidelity_keeps_a_spline_that_collapses_to_a_point() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Source"}));
    run(&mut s, "sketch.spline.control_point", json!({"points": [[5,0], [5,10], [5,20], [5,30]]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "sketch.create", json!({"plane": "XZ"}));
    run(&mut s, "sketch.project", json!({"refs": [{"sketch": "Source", "curve": "s1"}]}));
    let result = inspect(&mut s);
    assert!(result["curves"].as_array().unwrap().is_empty(), "{result}");
    assert!(result["points"].as_array().unwrap().iter().any(|p| p["link"].is_string() && p["at"] == json!([5.0, 0.0])), "{result}");
}

#[test]
fn projection_fidelity_does_not_round_an_almost_circular_edge() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    run(&mut s, "sketch.ellipse", json!({"center": [0,0], "major": [10,0], "minor_radius": 8.00001}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"distance": 5}));
    run(&mut s, "sketch.create", json!({"plane": {"origin": [0,0,5], "x_dir": [0.8,0,0.6], "y_dir": [0,1,0]}}));
    run(&mut s, "sketch.project", json!({"refs": [{"edge": [10,0,5]}]}));
    let result = inspect(&mut s);
    let curves = result["curves"].as_array().unwrap();
    assert_eq!(curves.len(), 1, "{result}");
    assert_eq!(curves[0]["type"], "spline", "the almost-circular ellipse must retain its exact spline: {result}");
}

#[test]
fn projection_fidelity_preserves_a_thin_curve_and_its_distinct_endpoints() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Source"}));
    run(&mut s, "sketch.spline.control_point", json!({"points": [[0,0], [90,10], [17,20], [0,30]]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "sketch.create", json!({"plane": {"x_dir": [1,0,0], "y_dir": [0,1e-10,1]}}));
    run(&mut s, "sketch.project", json!({"refs": [{"sketch":"Source", "curve":"s1"}]}));
    let result = inspect(&mut s);
    let curve = &result["curves"][0];
    assert_eq!(curve["type"], "spline", "{result}");
    let points = curve["points_at"].as_array().unwrap();
    assert_eq!(points.len(), 4);
    let last = points[3][1].as_f64().unwrap();
    assert!((last - 3e-9).abs() < 1e-20, "a thin projected endpoint must not merge into the start: {result}");
}

#[test]
fn projection_fidelity_keeps_the_full_edge_on_circle_interval() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane":"XY"}));
    run(&mut s, "sketch.circle.center", json!({"center":[0,0], "radius":10}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"distance":5}));
    run(&mut s, "sketch.create", json!({"plane":"XZ"}));
    run(&mut s, "sketch.project", json!({"refs":[{"edge":[10,0,5]}]}));
    let result = inspect(&mut s);
    let curves = result["curves"].as_array().unwrap();
    assert_eq!(curves.len(), 1, "{result}");
    assert_eq!(curves[0]["type"], "line", "{result}");
    let mut xs = [curves[0]["start_at"][0].as_f64().unwrap(), curves[0]["end_at"][0].as_f64().unwrap()];
    xs.sort_by(f64::total_cmp);
    assert!((xs[0] + 10.).abs() < 1e-8 && (xs[1] - 10.).abs() < 1e-8, "{result}");
}

#[test]
fn projection_fidelity_follows_tangent_handle_edits_through_reload_and_undo() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane":"XY", "name":"Source"}));
    run(&mut s, "sketch.spline.fit_point", json!({"points":[[0,0],[8,13],[20,-2]]}));
    let handle = run(&mut s, "sketch.spline.handles", json!({"curve":"s1"}))["handles"][1].as_str().unwrap().to_string();
    run(&mut s, "sketch.finish", json!({}));
    let target = run(&mut s, "sketch.create", json!({"plane":{"x_dir":[0.8,0,0.6],"y_dir":[0,1,0]},"name":"Target"}))["sketch"].as_u64().unwrap();
    run(&mut s, "sketch.project", json!({"refs":[{"sketch":"Source","curve":"s1"}]}));
    run(&mut s, "sketch.finish", json!({}));
    let controls = |session: &Session| {
        let state = session.model.state();
        let sk = &state.sketch(target).unwrap().sketch;
        assert_eq!(sk.curves.len(), 2);
        assert!(!sk.links[0].lost);
        sk.curves
            .iter()
            .map(|curve| {
                let solidcraft_sketch::CurveKind::Spline { pts, control: true, .. } = &curve.kind else { panic!("not an exact spline") };
                (curve.id.clone(), pts.iter().map(|p| sk.point(*p).unwrap()).collect::<Vec<_>>())
            })
            .collect::<Vec<_>>()
    };
    let original = controls(&s);
    run(&mut s, "sketch.edit", json!({"sketch":"Source"}));
    run(&mut s, "sketch.move_point", json!({"point":handle,"to":[15,4]}));
    run(&mut s, "sketch.finish", json!({}));
    let edited = controls(&s);
    assert_ne!(original, edited);
    assert_eq!(original.iter().map(|c| &c.0).collect::<Vec<_>>(), edited.iter().map(|c| &c.0).collect::<Vec<_>>());
    let reloaded = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
    assert_eq!(edited, controls(&reloaded));
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(original, controls(&s));
}

#[test]
fn projection_fidelity_refreshes_a_split_fit_spline_after_source_edit_and_reload() {
    let mut s = Session::default();
    let points: Vec<_> = (0..498).map(|i| [i as f64 * 0.1, (i as f64 * 0.07).sin()]).collect();
    run(&mut s, "sketch.create", json!({"plane":"XY", "name":"Source"}));
    let made = run(&mut s, "sketch.spline.fit_point", json!({"points":points}));
    let middle = made["created"]["points"][250].as_str().unwrap().to_string();
    run(&mut s, "sketch.finish", json!({}));
    let target = run(&mut s, "sketch.create", json!({"plane":{"x_dir":[0.8,0,0.6], "y_dir":[0,1,0]}, "name":"Target"}))["sketch"].as_u64().unwrap();
    run(&mut s, "sketch.project", json!({"refs":[{"sketch":"Source", "curve":"s1"}]}));
    run(&mut s, "sketch.finish", json!({}));
    assert_eq!(s.model.state().sketch(target).unwrap().sketch.curves.len(), 1);
    run(&mut s, "sketch.edit", json!({"sketch":"Source"}));
    let added = run(&mut s, "sketch.spline.insert_point", json!({"curve":"s1", "at":[49.8,0.5], "end":"end"}))["point"].as_str().unwrap().to_string();
    run(&mut s, "sketch.move_point", json!({"point":middle, "to":[25.,4.]}));
    run(&mut s, "sketch.finish", json!({}));
    let verify = |session: &Session| {
        let state = session.model.state();
        let sk = &state.sketch(target).unwrap().sketch;
        assert_eq!(sk.curves.len(), 2);
        assert!(!sk.links[0].lost);
        let all: Vec<Vec<_>> = sk
            .curves
            .iter()
            .map(|c| {
                let solidcraft_sketch::CurveKind::Spline { pts, control: true, nurbs: Some(nurbs), .. } = &c.kind else {
                    panic!("not an exact spline")
                };
                nurbs.check(pts.len(), 3).unwrap();
                pts.iter().map(|p| sk.point(*p).unwrap()).collect()
            })
            .collect();
        assert_eq!(all[0].last(), all[1].first());
        assert!(all.iter().flatten().any(|p| p.y > 3.), "source edit must reach the projected curve");
    };
    verify(&s);
    verify(&Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap()));
    run(&mut s, "sketch.edit", json!({"sketch":"Source"}));
    run(&mut s, "sketch.spline.remove_point", json!({"curve":"s1", "point":added}));
    run(&mut s, "sketch.finish", json!({}));
    assert_eq!(s.model.state().sketch(target).unwrap().sketch.curves.len(), 1);
    run(&mut s, "edit.undo", json!({}));
    verify(&s);
}

#[test]
fn projection_fidelity_keeps_control_splines_with_fewer_points_than_their_degree() {
    for points in [json!([[0, 0], [10, 10]]), json!([[0, 0], [5, 10], [10, 0]])] {
        let mut s = Session::default();
        run(&mut s, "sketch.create", json!({"plane":"XY", "name":"Source"}));
        run(&mut s, "sketch.spline.control_point", json!({"points":points}));
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "sketch.create", json!({"plane":{"x_dir":[0.8,0,0.6], "y_dir":[0,1,0]}}));
        run(&mut s, "sketch.project", json!({"refs":[{"sketch":"Source", "curve":"s1"}], "projection_link":false}));
        let result = inspect(&mut s);
        assert_eq!(result["curves"][0]["type"], "spline", "{result}");
        for (actual, source) in result["curves"][0]["points_at"].as_array().unwrap().iter().zip(points.as_array().unwrap()) {
            assert!((actual[0].as_f64().unwrap() - 0.8 * source[0].as_f64().unwrap()).abs() < 1e-12);
            assert!((actual[1].as_f64().unwrap() - source[1].as_f64().unwrap()).abs() < 1e-12);
        }
    }
}

#[test]
fn edge_on_sketch_lines_project_to_points_and_a_rectangle_outline() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XZ", "name": "Front"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0,0], "p1": [20,10]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    let refs: Vec<Value> = (1..=4).map(|i| json!({"sketch": "Front", "curve": format!("l{i}")})).collect();
    run(&mut s, "sketch.project", json!({"refs": refs}));
    let result = inspect(&mut s);
    let curves = result["curves"].as_array().unwrap();
    assert_eq!(curves.len(), 1, "{result}");
    assert_eq!(curves[0]["type"], "line", "{result}");
    let points = result["points"].as_array().unwrap();
    for x in [0., 20.] {
        assert!(points.iter().any(|p| p["link"].is_string() && p["at"] == json!([x, 0.])), "{result}");
    }
}

#[test]
fn a_linked_sketch_line_can_refresh_to_a_projected_point() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XZ", "name": "Front"}));
    run(&mut s, "sketch.line", json!({"points": [[5,0], [8,10]]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Top"}));
    run(&mut s, "sketch.project", json!({"refs": [{"sketch": "Front", "curve": "l1"}]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "sketch.edit", json!({"sketch": "Front"}));
    run(&mut s, "sketch.move_point", json!({"point": "l1.end", "to": [5,10]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "sketch.edit", json!({"sketch": "Top"}));
    let result = inspect(&mut s);
    assert!(result["curves"].as_array().unwrap().is_empty(), "{result}");
    let links = result["links"].as_array().unwrap();
    assert_eq!(links.len(), 1, "{result}");
    assert_ne!(links[0]["lost"], json!(true), "{result}");
    assert!(result["points"].as_array().unwrap().iter().any(|p| p["link"].is_string() && p["at"] == json!([5., 0.])), "{result}");
}

#[test]
fn tilted_spline_edges_keep_straight_seams_and_edge_on_boundaries_as_lines() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Outline"}));
    run(&mut s, "sketch.spline.control_point", json!({"points": [[0,0], [10,12], [25,12], [40,0]]}));
    run(&mut s, "sketch.line", json!({"points": [[40,0], [0,0]]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"distance": 5, "body_name": "Curved"}));
    for (plane, reference) in [
        (json!({"x_dir": [0.8,0,0.6], "y_dir": [0,1,0]}), json!({"body": "Curved", "edge": [0,0,2.5]})),
        (json!("XZ"), json!({"body": "Curved", "face": [20,4,5]})),
        (json!("XZ"), json!({"sketch": "Outline", "curve": "s1"})),
    ] {
        run(&mut s, "sketch.create", json!({"plane": plane}));
        run(&mut s, "sketch.project", json!({"refs": [reference]}));
        let result = inspect(&mut s);
        let curves = result["curves"].as_array().unwrap();
        assert_eq!(curves.len(), 1, "{reference}: {curves:?}");
        assert_eq!(curves[0]["type"], "line", "{reference}: {curves:?}");
        run(&mut s, "sketch.finish", json!({}));
    }
}

#[test]
fn tilted_circular_nurbs_edges_project_as_ellipses() {
    let mut s = Session::default();
    run(&mut s, "solid.cylinder", json!({"radius": 10, "height": 10}));
    run(&mut s, "solid.fillet", json!({"edges": [[10,0,10]], "radius": 2}));
    run(&mut s, "sketch.create", json!({"plane": {"x_dir": [0.8,0,0.6], "y_dir": [0,1,0]}}));
    run(&mut s, "sketch.project", json!({"refs": [{"edge": [8,0,10]}]}));
    let result = inspect(&mut s);
    let curves = result["curves"].as_array().unwrap();
    assert_eq!(curves.len(), 1, "{curves:?}");
    assert_eq!(curves[0]["type"], "ellipse", "{curves:?}");
    assert!((curves[0]["minor_radius"].as_f64().unwrap() - 6.4).abs() < 1e-7, "{curves:?}");
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
/// it goes away the sketch fails, naming the edge it included as lost, not crashed on.
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
    // Remove the cylinder: the edge included from it is gone, and the sketch says it is lost.
    let cyl = s.doc.features.iter().find(|f| matches!(f.kind, solidcraft_doc::FeatureKind::Cylinder { .. })).unwrap().id;
    run(&mut s, "timeline.delete", json!({"feature": cyl}));
    let t = run(&mut s, "document.inspect", json!({}));
    let row = t["timeline"].as_array().unwrap().iter().find(|r| r["id"] == json!(id)).cloned().unwrap();
    let error = row["error"].as_str().unwrap_or_default();
    let read = solidcraft_doc::lost_projection_parts(error).map(|t| (t.feature.to_string(), t.count, t.listed.to_string()));
    let (feature, count, listed) = read.unwrap_or_else(|| panic!("{row}"));
    assert_eq!((json!(feature), count), (row["name"].clone(), 1), "{row}");
    // Its circle lies on the silhouette's, so it has no curve of its own to name.
    assert_eq!(listed, "included edge of Body3", "the cylinder's included edge");
    assert!(!error.contains(&format!("F{cyl}:")), "{row}");
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
    // Its two rims, each one closed circular edge: one circle each.
    assert_eq!(radii.len(), 2, "{round}");
    assert!((radii[0] - 17.).abs() < 1e-6 && (radii[1] - 20.).abs() < 1e-6, "{round}");
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

#[test]
fn intersecting_a_semicircle_keeps_both_endpoints_fixed_and_linked() {
    for through_y in [-10., 10.] {
        let mut s = Session::default();
        run(&mut s, "sketch.create", json!({"plane":"XY","name":"Source"}));
        let arc = run(&mut s, "sketch.arc.three_point", json!({"start":[-10,0],"end":[10,0],"through":[0,through_y]}));
        let curve = ids(&arc["curves"])[0].clone();
        run(&mut s, "sketch.finish", json!({}));
        let section = run(&mut s, "sketch.create", json!({"plane":"XZ"}))["sketch"].as_u64().unwrap();
        let result = run(&mut s, "sketch.intersect", json!({"refs":[{"sketch":"Source","curve":curve}]}));
        assert_eq!(result["created"]["points"].as_array().unwrap().len(), 2, "through {through_y}: {result}");
        let si = inspect(&mut s);
        let points: Vec<_> = si["points"].as_array().unwrap().iter().filter(|p| !p["link"].is_null()).collect();
        assert_eq!(points.len(), 2, "{si}");
        assert!(points.iter().all(|p| p["fixed"] == true && p["link"] == "j1"), "{si}");
        assert_eq!(si["dof"], 0, "{si}");
        for x in [-10., 10.] {
            assert!(points.iter().any(|p| (p["at"][0].as_f64().unwrap() - x).abs() < 1e-9 && p["at"][1].as_f64().unwrap().abs() < 1e-9), "{si}");
        }
        run(&mut s, "sketch.finish", json!({}));
        let verify = |session: &Session| {
            let state = session.model.state();
            let sketch = &state.sketch(section).unwrap().sketch;
            assert_eq!(sketch.links.len(), 1);
            assert!(!sketch.links[0].lost);
            let points: Vec<_> = sketch.points.iter().filter(|p| p.link.as_deref() == Some("j1")).collect();
            assert_eq!(points.len(), 2);
            assert!(points.iter().all(|p| p.fixed));
        };
        verify(&s);
        verify(&Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap()));
    }
}

#[test]
fn intersecting_a_spline_uses_the_curve_instead_of_its_chords() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Source"}));
    // This cubic has x = 9t and y = 27t³, so it crosses y = 1 at x = 3.
    let spline = run(&mut s, "sketch.spline.control_point", json!({"points": [[0, 0], [3, 0], [6, 0], [9, 27]]}));
    let curve = ids(&spline["curves"])[0].clone();
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "sketch.create", json!({"plane": {"origin": [0, 1, 0], "x_dir": [1, 0, 0], "y_dir": [0, 0, 1]}}));
    run(&mut s, "sketch.intersect", json!({"refs": [{"sketch": "Source", "curve": curve}]}));

    let info = inspect(&mut s);
    let points: Vec<_> = info["points"].as_array().unwrap().iter().filter(|p| !p["link"].is_null()).collect();
    assert_eq!(points.len(), 1, "{info}");
    assert!((points[0]["at"][0].as_f64().unwrap() - 3.0).abs() < 1e-6, "{info}");
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

/// The curves sketch Intersect makes of the face at `face` of the body `profile` (lines and arcs
/// on XZ) revolved whole about X builds, on a sketch on XZ turned `angle` degrees about X (on XZ
/// itself for none), or Intersect's refusal.
fn intersect_revolved_face(profile: &[(&str, Value)], angle: Option<f64>, face: [f64; 3]) -> std::result::Result<Vec<Value>, String> {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XZ", "name": "profile"}));
    for (cmd, p) in profile {
        run(&mut s, cmd, p.clone());
    }
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.revolve", json!({"sketch": "profile", "axis": "x", "angle": "360 deg", "body_name": "turned"}));
    if let Some(angle) = angle {
        run(&mut s, "construct.plane.at_angle", json!({"base": "XZ", "axis": "X", "angle": format!("{angle} deg"), "name": "cut"}));
        run(&mut s, "sketch.create", json!({"plane": "cut", "name": "section"}));
    } else {
        run(&mut s, "sketch.create", json!({"plane": "XZ", "name": "section"}));
    }
    s.execute("sketch.intersect", &json!({"refs": [{"face": face, "body": "turned"}]})).map_err(|e| e.to_string())?;
    Ok(linked_curves(&inspect(&mut s)))
}

/// That a plane through the axis cuts a turned face in two of `kind` ("line" or "arc", of
/// `radius` about (`centre`, ±its y)), one each side of the axis, from (`a`, ±its y) to (`b`, ±its
/// y): on the face's edges exactly, within the model's tolerance, whichever way the sketch's y
/// points and each runs.
fn assert_axial(curves: &[Value], kind: &str, a: [f64; 2], b: [f64; 2], arc: Option<([f64; 2], f64)>) {
    const EXACT: f64 = 1e-9;
    assert!(curves.len() == 2 && curves.iter().all(|c| c["type"] == kind), "{curves:?}");
    let at = |c: &Value, k: &str| [c[k][0].as_f64().unwrap_or(f64::NAN), c[k][1].as_f64().unwrap_or(f64::NAN)];
    let mut sides = Vec::new();
    for c in curves {
        let (start, end) = (at(c, "start_at"), at(c, "end_at"));
        let side = (start[1] + end[1]).signum();
        let near = |p: [f64; 2], q: [f64; 2]| (p[0] - q[0]).hypot(p[1] - side * q[1]) < EXACT;
        assert!((near(start, a) && near(end, b)) || (near(start, b) && near(end, a)), "{c}");
        if let Some((centre, radius)) = arc {
            assert!(near(at(c, "center_at"), centre) && (c["radius"].as_f64().unwrap_or(f64::NAN) - radius).abs() < EXACT, "{c}");
        }
        sides.push(side);
    }
    sides.sort_by(f64::total_cmp);
    assert_eq!(sides, [-1.0, 1.0], "{curves:?}");
}

/// A plane through a cone's axis cuts it in two of its lines, from one circular edge to the
/// other: at 15 degrees they ran 0.0227 mm past the radius-10 edge and stopped 0.0067 mm short
/// of the radius-20 one (#17496).
#[test]
fn intersect_through_a_cones_axis_gives_its_lines_edge_to_edge() {
    let frustum = [("sketch.line", json!({"points": [[0, 10], [10, 20], [10, 25], [0, 25]], "closed": true}))];
    let curves = intersect_revolved_face(&frustum, Some(15.0), [0.521423, -6.633141, -8.167116]).unwrap();
    assert_axial(&curves, "line", [0.0, 10.0], [10.0, 20.0], None);
}

/// A plane through a torus's axis cuts a 10 degree band of it in the profile's arc, not its chord
/// (#17498).
#[test]
fn intersect_through_a_torus_axis_gives_its_arc_not_the_chord() {
    let ring = [
        ("sketch.arc.center_point", json!({"center": [0, 20], "start": [0, 30], "sweep": -10})),
        ("sketch.line", json!({"points": ["a1.end", [0, 10], [0, 30]], "infer": false})),
    ];
    let curves = intersect_revolved_face(&ring, None, [0.645545, 19.391767, -22.862815]).unwrap();
    let (s, c) = 10f64.to_radians().sin_cos();
    assert_axial(&curves, "arc", [0.0, 30.0], [10.0 * s, 20.0 + 10.0 * c], Some(([0.0, 20.0], 10.0)));
}

/// A plane through the axis cuts a small round (0.2 mm through 15 degrees) in its arcs, where it
/// said the face projected to nothing (#17499).
#[test]
fn intersect_through_the_axis_cuts_a_small_round() {
    let shaft = [
        ("sketch.line", json!({"points": [[0, 0], [0, 10], [9.8, 10]]})),
        ("sketch.arc.center_point", json!({"center": [9.8, 9.8], "start": [9.8, 10], "sweep": -15})),
        ("sketch.line", json!({"points": ["a1.end", [10, 0], [0, 0]], "infer": false})),
    ];
    let curves = intersect_revolved_face(&shaft, Some(15.0), [9.824871, -7.087480, 7.052417]).unwrap();
    let (s, c) = 15f64.to_radians().sin_cos();
    assert_axial(&curves, "arc", [9.8, 10.0], [9.8 + 0.2 * s, 9.8 + 0.2 * c], Some(([9.8, 9.8], 0.2)));
}

/// A plane through a torus's axis at 15 degrees cuts it in the same circle as the profile's
/// plane does: centre 20 from the axis, radius 10 (it was 20.073 and 9.926, #17500).
#[test]
fn intersect_through_a_torus_axis_at_an_angle_gives_the_profile_circle() {
    let ring = [
        ("sketch.arc.center_point", json!({"center": [0, 20], "start": [0, 30], "sweep": -45})),
        ("sketch.line", json!({"points": ["a1.end", [0, 10], [0, 30]], "infer": false})),
    ];
    let half = std::f64::consts::FRAC_1_SQRT_2 * 10.0;
    for angle in [Some(15.0), None] {
        let curves = intersect_revolved_face(&ring, angle, [4.473090, 20.954251, 19.966538]).unwrap();
        assert_axial(&curves, "arc", [0.0, 30.0], [half, 20.0 + half], Some(([0.0, 20.0], 10.0)));
    }
}

/// A plane 1e-7 off a cylinder's axis, the cylinder reaching a million from the axis's origin
/// (z 0 to 1000001), misses the cylinder's walls by 0.005 there: the exact section leaves
/// it (`None`) and Intersect cuts the face as it did, which finds the wall (#17496).
#[test]
fn intersect_leaves_a_plane_near_the_axis_far_from_its_origin_to_the_section() {
    use solidcore_record::{Plane, Vec3};
    let mut s = Session::default();
    // Standing on z = 0 and a million tall, so the stored axis origin is at the base and the face
    // the point below picks lies a million from it.
    run(&mut s, "solid.cylinder", json!({"base": [0, 0, 0], "radius": 1, "height": 1000001, "body_name": "Far"}));
    run(&mut s, "sketch.create", json!({"plane": {"origin": [0, 0, 0], "x_dir": [0, 1, 0], "y_dir": [-1e-7, 0, 1]}}));
    let plane = Plane { origin: Vec3::new(0.0, 0.0, 0.0), x: Vec3::new(0.0, 1.0, 0.0), y: Vec3::new(-1e-7, 0.0, 1.0) };
    let st = s.model.state();
    let far = &st.body("Far").unwrap().body;
    // The test only proves the section guard if the cylinder's stored axis origin lies far from
    // the face the point below picks (z = 1,000,000.5): were it stored near that, the old guard
    // would reject this plane too. So the origin must be near z = 0.
    let mut origins = Vec::new();
    for face in 0..far.face_count() {
        if let Some(solidcraft_kernel::FaceSurface::Cylinder { origin, .. }) = solidcraft_kernel::face_surface(far, face) {
            origins.push(origin.z);
        }
    }
    assert!(!origins.is_empty() && origins.iter().all(|z| z.abs() < 1.0), "cylinder axis origin z {origins:?}");
    for face in 0..far.face_count() {
        assert!(solidcraft_kernel::axial_face_section(far, face, &plane).unwrap().is_none(), "face {face}");
    }
    drop(st);
    s.execute("sketch.intersect", &json!({"refs": [{"face": [1, 0, 1000000.5], "body": "Far"}]})).unwrap();
    assert!(!linked_curves(&inspect(&mut s)).is_empty());
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

/// An imported rational cubic keeps its degree, weights and shape on both sides of a cut.
#[test]
fn break_and_trim_preserve_the_intervals_of_a_weighted_cubic() {
    use solidcore_record::Vec2;
    use solidcraft_sketch::{CurveKind, Nurbs};

    let controls = [Vec2::new(0., 0.), Vec2::new(4., 8.), Vec2::new(8., 8.), Vec2::new(12., 0.)];
    let nurbs = Nurbs { knots: vec![3., 3., 3., 3., 9., 9., 9., 9.], weights: vec![1., 2., 2., 1.] };
    let evaluate = |t| nurbs.point(&controls, 3, t).unwrap();
    for command in ["sketch.break", "sketch.trim"] {
        let mut s = Session::default();
        let sid = run(&mut s, "sketch.create", json!({"plane": "XY"}))["sketch"].as_u64().unwrap();
        // Projected splines use the imported degree, control points, knots and weights.
        let sketch = s.doc_mut().sketch_mut(sid).unwrap();
        let pts = controls.iter().map(|&p| sketch.add_point(p, None).unwrap()).collect();
        let kind = CurveKind::Spline { pts, control: true, degree: 3, periodic: false, nurbs: Some(nurbs.clone()), handles: Vec::new() };
        sketch.add_curve(kind, Some("weighted")).unwrap();
        let mark = evaluate(0.223);
        let snap = run(&mut s, "sketch.point", json!({"point":format!("on:weighted:{},{}",mark.x,mark.y)}));
        let x = evaluate(0.75).x;
        run(&mut s, "sketch.line", json!({"points":[[x,-10],[x,10]], "infer":false, "connect":false}));
        let pick = evaluate(0.5);
        let before = s.doc.sketch(sid).unwrap().clone();
        let preview = crate::cmd::trim_piece(&before, "weighted", pick).unwrap();
        assert!(preview.first().unwrap().dist(mark) < 1e-5 && preview.last().unwrap().dist(evaluate(0.75)) < 1e-5);
        run(&mut s, command, json!({"curve":"weighted", "at":[pick.x,pick.y]}));
        let sk = s.doc.sketch(sid).unwrap();
        let point = sk.resolve_point(snap["point"].as_str().unwrap()).unwrap();
        assert!(
            sk.curves.iter().any(|c| c.kind.is_freeform() && c.kind.ends().is_some_and(|(a, b)| a == point || b == point)),
            "the constrained point becomes an endpoint"
        );
        let mut pieces: Vec<_> = sk
            .curves
            .iter()
            .filter_map(|c| match &c.kind {
                CurveKind::Spline { pts, degree: 3, nurbs: Some(n), .. } => Some((pts.iter().map(|&p| sk.point(p).unwrap()).collect::<Vec<_>>(), n)),
                _ => None,
            })
            .collect();
        pieces.sort_by(|(a, _), (b, _)| a[0].x.total_cmp(&b[0].x));
        let intervals = if command == "sketch.break" { vec![(0., 0.223), (0.223, 0.75), (0.75, 1.)] } else { vec![(0., 0.223), (0.75, 1.)] };
        assert_eq!(pieces.len(), intervals.len(), "{command}");
        for ((points, piece), (a, b)) in pieces.iter().zip(intervals) {
            assert!(piece.weights.windows(2).any(|pair| pair[0] != pair[1]));
            for t in [0.0, 0.25, 0.5, 0.75, 1.0] {
                let actual = piece.point(points, 3, t).unwrap();
                let expected = evaluate(a + (b - a) * t);
                assert!(actual.dist(expected) < 1e-5, "{command} [{a}, {b}] at {t}: {actual:?} vs {expected:?}");
            }
        }
    }
}

/// A piece straddling the old seam of a closed, weighted spline is still a single spline.
#[test]
fn break_keeps_a_weighted_spline_piece_that_wraps_across_its_seam() {
    use solidcore_record::Vec2;
    use solidcraft_sketch::{CurveKind, Nurbs};

    let mut s = Session::default();
    let sid = run(&mut s, "sketch.create", json!({"plane":"XY"}))["sketch"].as_u64().unwrap();
    let sk = s.doc_mut().sketch_mut(sid).unwrap();
    let controls = [Vec2::new(1., 1.), Vec2::new(9., 1.), Vec2::new(1., 9.), Vec2::new(1., 1.)];
    let pts = controls.iter().map(|&p| sk.add_point(p, None).unwrap()).collect();
    let nurbs = Nurbs { knots: vec![0., 0., 0., 0., 1., 1., 1., 1.], weights: vec![1., 2., 2., 1.] };
    let evaluate = |t| nurbs.point(&controls, 3, t).unwrap();
    sk.add_curve(CurveKind::Spline { pts, control: true, degree: 3, periodic: false, nurbs: Some(nurbs.clone()), handles: Vec::new() }, Some("loop"))
        .unwrap();
    for t in [0.25, 0.75] {
        let q = evaluate(t);
        run(&mut s, "sketch.point", json!({"point": format!("on:loop:{},{}",q.x,q.y)}));
    }
    let pick = evaluate(0.125);
    let preview = crate::cmd::trim_piece(s.doc.sketch(sid).unwrap(), "loop", pick).unwrap();
    assert!(preview.first().unwrap().dist(evaluate(0.75)) < 1e-5 && preview.last().unwrap().dist(evaluate(0.25)) < 1e-5);
    run(&mut s, "sketch.break", json!({"curve":"loop","at":[pick.x,pick.y]}));
    let sk = s.doc.sketch(sid).unwrap();
    assert_eq!(sk.curves.len(), 2);
    let wrapped = sk.curves.iter().find(|c| c.kind.ends().is_some_and(|(a, _)| sk.point(a).unwrap().dist(evaluate(0.75)) < 1e-5)).unwrap();
    let CurveKind::Spline { pts, degree: 3, nurbs: Some(w), .. } = &wrapped.kind else { panic!("expected one rational cubic") };
    let points: Vec<_> = pts.iter().map(|&p| sk.point(p).unwrap()).collect();
    for (t, source) in [(0., 0.75), (0.25, 0.875), (0.5, 0.), (0.75, 0.125), (1., 0.25)] {
        assert!(w.point(&points, 3, t).unwrap().dist(evaluate(source)) < 1e-5);
    }
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

/// Literal saved projection geometry for #20280. Generated once from the issue's
/// three-point cap and the 2 mm inward shell: intersect the inward side lines with
/// the circle of radius R-2. Coordinates are in the rim sketch's (-X,+Z) frame.
/// The 0.45.0 form uses a single clamped rational spline for that same inner edge;
/// its literal quadratic spans cover precisely the old arc's complete extent.
fn saved_shelled_cap_projection(through: f64, spline: bool) -> (Session, u64, solidcraft_sketch::Sketch, Vec<solidcraft_sketch::LinkGeom>) {
    use solidcore_record::{Plane, Vec2, Vec3};
    use solidcraft_sketch::{LinkGeom, LinkKind, Nurbs, Sketch};

    let (center, right, left, points, weights, knots) = if through == 55. {
        (
            [0., -31.094594594594593],
            [-18.23954351753727, -18.963401815356903],
            [18.23954351753727, -18.963401815356903],
            vec![
                [-18.23954351753727, -18.963401815356903],
                [-28.86309564158218, -34.93617092417646],
                [-14.431547820791089, -47.57419451302054],
                [0., -60.212218101864615],
                [14.431547820791087, -47.574194513020544],
                [28.86309564158218, -34.936170924176466],
                [18.23954351753727, -18.963401815356903],
            ],
            vec![1., 0.7523074608041455, 1., 0.7523074608041455, 1., 0.7523074608041455, 1.],
            vec![0., 0., 0., 1., 1., 2., 2., 3., 3., 3.],
        )
    } else {
        assert_eq!(through, 25.);
        (
            [0., 7.071428571428571],
            [-17.528321641606148, -17.363152594511888],
            [17.528321641606148, -17.363152594511888],
            vec![[-17.528321641606148, -17.363152594511888], [0., -29.93721954843477], [17.528321641606148, -17.363152594511888]],
            vec![1., 0.8125513926916067, 1.],
            vec![0., 0., 0., 1., 1., 1.],
        )
    };
    let (center, right, left) = (Vec2::from(center), Vec2::from(right), Vec2::from(left));
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane":"XZ"}));
    run(&mut s, "sketch.line", json!({"points":[[20,18],[12,0],[-12,0],[-20,18]]}));
    run(&mut s, "sketch.arc.three_point", json!({"start":[-20,18],"end":[20,18],"through":[0,through]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"sketch":"Sketch1","distance":30}));
    run(&mut s, "solid.shell", json!({"faces":["F2:top"],"thickness":2}));
    let id = run(&mut s, "sketch.create", json!({"plane":{"face":[0,30,-1]}}))["sketch"].as_u64().unwrap();
    let plane = s.model.state().sketch(id).unwrap().plane;
    let want = Plane { origin: Vec3::new(0., 30., 0.), x: Vec3::new(-1., 0., 0.), y: Vec3::new(0., 0., 1.) };
    // The rim face's plane, to rounding: the fixed coordinates below are written in it.
    assert!(plane.origin.dist(want.origin) < 1e-9 && plane.x.dist(want.x) < 1e-9 && plane.y.dist(want.y) < 1e-9, "{plane:?}");
    let mut boundary = vec![
        LinkGeom::Arc { c: center, a: Vec2::new(-20., -18.), b: Vec2::new(20., -18.) },
        LinkGeom::Line(Vec2::new(20., -18.), Vec2::new(12., 0.)),
        LinkGeom::Line(Vec2::new(12., 0.), Vec2::new(-12., 0.)),
        LinkGeom::Line(Vec2::new(-12., 0.), Vec2::new(-20., -18.)),
        LinkGeom::Arc { c: center, a: right, b: left },
        LinkGeom::Line(left, Vec2::new(10.70025382182309, -2.)),
        LinkGeom::Line(Vec2::new(10.70025382182309, -2.), Vec2::new(-10.70025382182309, -2.)),
        LinkGeom::Line(Vec2::new(-10.70025382182309, -2.), right),
    ];
    let inner = boundary[4..].to_vec();
    if spline {
        boundary[4] = LinkGeom::Spline {
            pts: points.into_iter().map(Vec2::from).collect(),
            control: true,
            degree: 2,
            periodic: false,
            nurbs: Some(Nurbs { knots, weights }),
        };
    }
    let template = s.doc.sketch(id).unwrap().links[0].clone();
    let mut legacy = Sketch::new();
    let link = legacy.add_link(LinkKind::Project, template.source.clone(), &boundary).unwrap();
    assert_eq!(link, template.id);
    legacy.links[0] = template;
    if !spline {
        legacy.links[0].curve_edges.clear(); // absent in 0.44.1 and rc.4
    } else {
        assert_eq!(legacy.links[0].curve_edges.len(), 8, "0.45.0 stores the source edge of each projection curve");
        assert!(!legacy.links[0].curve_edges[4].is_empty(), "the inner rim has an exact edge identity");
    }
    (s, id, legacy, inner)
}

#[test]
fn a_saved_offset_of_a_shelled_major_arc_reopens_with_its_sources() {
    use solidcraft_doc::{Document, FeatureKind, ProfileSel, WholeProfileBoundary};
    use solidcraft_sketch::{CurveKind, LinkKind, LinkSource, OffsetChain};

    // The shelled arc of more than half a turn (through 55) does not reopen yet: #21018.
    for through in [25.0_f64] {
        for spline in [false, true] {
            let (mut s, id, mut legacy, inner) = saved_shelled_cap_projection(through, spline);
            let source_ids = [if spline { "s1" } else { "a2" }, "l4", "l5", "l6"];
            let order = source_ids.iter().map(|id| (legacy.curve_index(id).unwrap(), true)).collect::<Vec<_>>();
            let outputs = if spline {
                // The 0.45.0 command records a trimmed free-form chain offset.
                let made = legacy.offset_freeform_chain(&order, 0.8).unwrap();
                legacy.record_freeform_chain_offset(&order, &made, 0.8).unwrap();
                made.iter().map(|i| legacy.curves[*i].id.clone()).collect::<Vec<_>>()
            } else {
                let output = legacy.add_link(LinkKind::Project, LinkSource::Origin, &inner).unwrap();
                let outputs = legacy.link_curves(&output).iter().map(|&i| legacy.curves[i].id.clone()).collect::<Vec<_>>();
                legacy.break_link(&output).unwrap();
                legacy.offsets.push(OffsetChain {
                    sources: source_ids.iter().map(|id| (id.to_string(), true)).collect(),
                    outputs: outputs.clone(),
                    distance: 0.8,
                    closed: true,
                    topology: None,
                });
                outputs
            };
            legacy.refresh_offsets().unwrap();
            let expected = source_ids.iter().map(|id| (id.to_string(), true)).collect::<Vec<_>>();
            // A separate saved consumer names both complete projected boundaries,
            // including the inner rim; the offset lies inside that profile's hole.
            run(&mut s, "sketch.finish", json!({}));
            run(&mut s, "solid.extrude", json!({"sketch":"Sketch2","profiles":[{"curves":["a1","l1","l2","l3"]}],"distance":2}));
            let mut saved = (*s.doc).clone();
            *saved.sketch_mut(id).unwrap() = legacy;
            let FeatureKind::Extrude { profiles, .. } = &mut saved.features.last_mut().unwrap().kind else { panic!("missing extrude") };
            let mut hole = source_ids.iter().map(|id| id.to_string()).collect::<Vec<_>>();
            hole.sort();
            *profiles = ProfileSel::WholeBoundaries {
                profiles: vec![WholeProfileBoundary { outer: vec!["a1".into(), "l1".into(), "l2".into(), "l3".into()], holes: vec![hole] }],
            };
            let opened = Session::new(Document::from_json(&saved.to_json()).unwrap());
            assert!(
                opened.model.results.iter().all(|r| r.error.is_none()),
                "through={through}, spline={spline}: {:?}",
                opened.model.results.iter().filter_map(|r| r.error.as_ref()).collect::<Vec<_>>()
            );
            let rebuilt = opened.model.state();
            let sk = &rebuilt.sketch(id).unwrap().sketch;
            let sources = if spline { &sk.freeform_chain_offsets[0].sources } else { &sk.offsets[0].sources };
            assert_eq!(sources, &expected);
            assert!(matches!(sk.curves[sk.curve_index(source_ids[0]).unwrap()].kind, CurveKind::Arc { .. }));
            assert!(outputs.iter().all(|id| sk.curve_index(id).is_some()));
            assert!(
                rebuilt.sketch(id).unwrap().profiles.iter().any(|p| {
                    std::iter::once(&p.outer_curves).chain(&p.hole_curves).any(|boundary| outputs.iter().all(|id| boundary.contains(id)))
                }),
                "Sketch2's offset outputs form a profile boundary"
            );
            // A fresh design still uses the restored arc directly.
            run(&mut s, "sketch.edit", json!({"sketch":"Sketch2"}));
            run(&mut s, "sketch.offset", json!({"curves":["a2"],"distance":0.8}));
            run(&mut s, "sketch.finish", json!({}));
            assert!(s.model.results.iter().all(|r| r.error.is_none()));
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

/// #12920: a hole of radius 1.7 drilled along X through a round bar, its rim on the bar's wall an
/// intersection curve SMLib holds only approximately. Projected along the hole's axis the rim
/// comes in as the hole's circle exactly; fitted to the rim's points it came in 1.3e-6 small and
/// off centre, and an annulus cut round the hole from it left a wall 1.3e-6 off the hole's,
/// which SMLib's check refused ("Face Near missing Seam").
#[test]
fn a_hole_rim_on_a_round_wall_projects_along_the_hole_as_its_circle() {
    use solidcraft_sketch::CurveKind;
    let mut s = Session::default();
    run(&mut s, "solid.cylinder", json!({"radius": 11, "height": 20, "body_name": "Bar"}));
    run(&mut s, "solid.cylinder", json!({"radius": 1.7, "height": 30, "base": [-15, 0, 10], "axis": "X", "operation": "cut", "targets": ["Bar"]}));
    let id = run(&mut s, "sketch.create", json!({"plane": "YZ", "offset": -20, "project_edges": false}))["sketch"].as_u64().unwrap();
    let rim_x = -(121f64 - 1.7 * 1.7).sqrt();
    run(&mut s, "sketch.project", json!({"refs": [{"body": "Bar", "edge": [rim_x, 1.7, 10]}]}));
    run(&mut s, "sketch.circle.center", json!({"center": [0, 10], "radius": 2.15}));
    run(&mut s, "sketch.finish", json!({}));
    {
        let state = s.model.state();
        let sk = &state.sketch(id).unwrap().sketch;
        let center = solidcore_record::Vec2::new(0., 10.);
        // One circle, as the rim fitted came in, so the sketch's curves and profiles keep their order.
        let rims: Vec<(solidcore_record::Vec2, f64)> = sk
            .curves
            .iter()
            .filter_map(|c| match c.kind {
                CurveKind::Circle { c, r } if r < 2.0 => Some((sk.points[c].pos, r)),
                _ => None,
            })
            .collect();
        assert_eq!((rims.len(), sk.curves.len()), (1, 2), "the rim projects as {:?}", sk.curves);
        for (c, r) in rims {
            assert!(c.dist(center) < 1e-9 && (r - 1.7).abs() < 1e-9, "the rim projects as centre {c:?}, radius {r}");
        }
    }
    // The annulus between the rim and a circle of radius 2.15, cut 10 into the bar from x = -20.
    run(
        &mut s,
        "solid.extrude",
        json!({"sketch": id, "distance": 10, "operation": "cut", "exact_regions": true, "profiles": [{"point": [0, 11.9]}]}),
    );
    let state = s.model.state();
    let bar = state.body("Bar").unwrap();
    assert_eq!(bar.body.validity(), Vec::<String>::new());
}

/// #12920, the open rim: the hole of radius 1.7 drilled along X at z = 19 breaks through the
/// bar's top (z = 20), so its rim on the bar's round wall is open: no edge of it closes on itself.
/// Projected along the hole's axis, an edge of that rim is one arc of the hole's circle, centre
/// (0, 19), from the edge's first vertex to its last, the way it runs, through as much of the
/// circle as the edge covers.
#[test]
fn an_open_hole_rim_on_a_round_wall_projects_along_the_hole_as_an_arc_of_its_circle() {
    use solidcore_record::{Seg2, Vec2, Vec3};
    let mut s = Session::default();
    run(&mut s, "solid.cylinder", json!({"radius": 11, "height": 20, "body_name": "Bar"}));
    run(&mut s, "solid.cylinder", json!({"radius": 1.7, "height": 30, "base": [-15, 0, 19], "axis": "X", "operation": "cut", "targets": ["Bar"]}));
    let state = s.model.state();
    let body = &state.body("Bar").unwrap().body;
    let at = Vec3::new(-11., 0., 19. - 1.7);
    let edges = body.edges(1e-3).unwrap();
    let rim = edges
        .iter()
        .min_by(|a, b| {
            let d = |e: &solidcraft_kernel::EdgeInfo| e.points.iter().map(|p| p.dist(at)).fold(f64::INFINITY, f64::min);
            d(a).total_cmp(&d(b))
        })
        .unwrap();
    let record = body.record();
    let vertex = |v: u32| Vec3::from(record.vertices[v as usize].point);
    let [v0, v1] = record.edges[rim.index].vertices;
    assert!(v0 != v1, "the rim edge closes on itself");
    let plane = solidcore_record::Plane { origin: Vec3::new(-20., 0., 0.), x: Vec3::Y, y: Vec3::Z };
    let pieces = body.planar_edges(&plane, &[rim.index]).remove(0);
    let Some([solidcraft_kernel::PlanarPiece::Seg(Seg2::Arc { center, radius, start, sweep })]) = pieces.as_deref() else {
        panic!("the open rim projects as {pieces:?}")
    };
    let (center, radius, start, sweep) = (*center, *radius, *start, *sweep);
    assert!(center.dist(Vec2::new(0., 19.)) < 1e-9 && (radius - 1.7).abs() < 1e-9, "centre {center:?}, radius {radius}");
    let on_arc = |angle: f64| center + Vec2::new(angle.cos(), angle.sin()) * radius;
    // The arc's ends are the edge's vertices, in the order the edge runs.
    for (angle, v) in [(start, v0), (start + sweep, v1)] {
        let want = plane.to_local(vertex(v));
        assert!(on_arc(angle).dist(want) < 1e-9, "the arc ends at {:?}, the edge at {want:?}", on_arc(angle));
    }
    // And it goes the edge's way round: as long as the edge (its polyline a chord's sag shorter).
    assert!((sweep.abs() * radius - rim.length).abs() < 1e-2 * rim.length, "sweep {sweep}, edge length {}", rim.length);
    assert!(sweep.abs() < std::f64::consts::TAU - 1e-6, "sweep {sweep}");
}

/// #12920, a full turn: a modelled thread's helical edges each wind once round the shaft's axis
/// (the kernel splits them at the cylinder's seam), from a vertex to one a pitch along the axis
/// at the same angle. An edge that comes in as one arc must be as long as that arc and wind no
/// more than once: an arc of the remainder between the end vertices (none, or a sliver) would
/// be short of the edge by a whole turn.
#[test]
fn an_edge_winding_a_full_turn_projects_as_no_arc_of_its_remainder() {
    use solidcore_record::{Seg2, Vec3};
    let mut s = Session::default();
    run(&mut s, "solid.cylinder", json!({"radius": 4, "height": 20, "body_name": "Shaft"}));
    run(&mut s, "solid.thread", json!({"face": [4, 0, 10], "designation": "M8", "modeled": true}));
    let state = s.model.state();
    let body = &state.body("Shaft").unwrap().body;
    let plane = solidcore_record::Plane { origin: Vec3::new(0., 0., -5.), x: Vec3::X, y: Vec3::Y };
    // how far round the z axis an edge's polyline winds
    let winding = |e: &solidcraft_kernel::EdgeInfo| {
        e.points
            .windows(2)
            .map(|w| {
                let (a, b) = (w[0].y.atan2(w[0].x), w[1].y.atan2(w[1].x));
                (b - a + std::f64::consts::PI).rem_euclid(std::f64::consts::TAU) - std::f64::consts::PI
            })
            .sum::<f64>()
    };
    let edges = body.edges(1e-3).unwrap();
    let mut helices = 0;
    for e in &edges {
        let pieces = body.planar_edges(&plane, &[e.index]).remove(0);
        let turns = winding(e).abs() / std::f64::consts::TAU;
        if let Some([solidcraft_kernel::PlanarPiece::Seg(Seg2::Arc { radius, sweep, .. })]) = pieces.as_deref() {
            assert!(turns < 1.01, "edge {} winds {turns} turns and projects as an arc of sweep {sweep}", e.index);
            assert!(
                (sweep.abs() * radius - e.length).abs() < 0.05 * e.length.max(1.0),
                "edge {} of length {} projects as an arc of sweep {sweep}, radius {radius}",
                e.index,
                e.length
            );
        }
        if turns > 0.99 {
            helices += 1;
        }
    }
    assert!(helices > 0, "no edge of the thread winds a full turn");
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

/// #18711: a panel leaning at the angle `A` (an 18 wide foot at the origin, sides 300 long),
/// extruded 118, with a sketch on its top face and a circle on the projected top corner away
/// from the origin. Returns the sketch and the circle.
fn leaning_panel(s: &mut Session) -> (u64, String) {
    run(s, "parameters.change", json!({"name": "A", "expression": "95 deg"}));
    run(s, "sketch.create", json!({"plane": "XY"}));
    run(s, "sketch.line", json!({"points": [[0, 0], [18, 0], [-8, 299], [-26, 299]], "closed": true, "ids": ["m1", "m2", "m3", "m4"]}));
    run(s, "sketch.constraint.coincident", json!({"a": "m1.start", "b": "origin"}));
    for (line, mode) in [("m1", "horizontal"), ("m3", "horizontal")] {
        run(s, "sketch.constraint.horizontal_vertical", json!({"line": line, "mode": mode}));
    }
    run(s, "sketch.constraint.parallel", json!({"a": "m2", "b": "m4"}));
    run(s, "sketch.dimension", json!({"entities": ["m1"], "value": 18}));
    run(s, "sketch.dimension", json!({"entities": ["m4"], "value": 300}));
    run(s, "sketch.dimension", json!({"entities": ["m1", "m4"], "value": "A"}));
    assert_eq!(inspect(s)["dof"], 0);
    run(s, "sketch.finish", json!({}));
    run(s, "solid.extrude", json!({"distance": 118, "body_name": "Panel"}));
    let sketch = run(s, "sketch.create", json!({"plane": {"face": [-4, 149, 118]}}))["sketch"].as_u64().unwrap();
    let st = s.model.state();
    let ss = st.sketch(sketch).unwrap();
    let on_corner = ss.sketch.points.iter().find(|p| p.link.is_some() && ss.plane.to_world(p.pos).dist(panel_corner(95.)) < 1e-6).unwrap().id.clone();
    let circle = ids(&run(s, "sketch.circle.center", json!({"center": [-6, 290], "radius": 5}))["curves"])[0].clone();
    run(s, "sketch.constraint.coincident", json!({"a": format!("{circle}.center"), "b": on_corner}));
    run(s, "sketch.dimension", json!({"entities": [circle], "value": 10}));
    assert_eq!(inspect(s)["dof"], 0);
    run(s, "sketch.finish", json!({}));
    (sketch, circle)
}

/// The top corner of [`leaning_panel`]'s side away from the origin: the foot's end and a side of
/// 300 at `degrees`.
fn panel_corner(degrees: f64) -> solidcore_record::Vec3 {
    let a = degrees.to_radians();
    solidcore_record::Vec3::new(18. + 300. * a.cos(), 300. * a.sin(), 118.)
}

/// #18711: turning [`leaning_panel`] from 95 to 92, 90 or 85 degrees moves its corner past the
/// foot: the face's edges, read in the order of where they lie, come back in another order, and
/// the projection was rebuilt without the circle's constraint. The link's curves keep the names
/// of the edges they stand for, which find them again: the circle stays on the corner.
#[test]
fn a_leaning_panel_turned_keeps_what_is_on_its_projected_face() {
    let mut s = Session::default();
    let (sketch, circle) = leaning_panel(&mut s);
    for degrees in [92., 90., 85., 95.] {
        run(&mut s, "parameters.change", json!({"name": "A", "expression": format!("{degrees} deg")}));
        let warning = s.model.result(sketch).and_then(|r| r.warning.clone());
        assert!(warning.is_none(), "{degrees}: {warning:?}");
        let st = s.model.state();
        let ss = st.sketch(sketch).unwrap();
        assert!(ss.sketch.links.iter().all(|l| !l.lost), "{degrees}");
        assert_eq!(ss.sketch.constraints.len(), 2, "{degrees}: the circle keeps its place and its size");
        let center = ss.sketch.points.iter().find(|p| p.id == format!("{circle}.center")).unwrap();
        assert!(ss.plane.to_world(center.pos).dist(panel_corner(degrees)) < 1e-6, "{degrees}: {:?}", ss.plane.to_world(center.pos));
    }
}

/// #18711: the face's link saves the name of each of its curves' edges, one per curve, and a
/// design saved with them reads them back. A design saved before them (the field taken out) opens
/// and builds as it did: the same curves where they were, the circle on its corner.
#[test]
fn a_projected_face_saves_its_edge_names_and_a_design_without_them_builds_as_before() {
    let mut s = Session::default();
    let (sketch, circle) = leaning_panel(&mut s);
    let saved: Value = serde_json::from_str(&s.doc.to_json()).unwrap();
    let link = s.doc.sketch(sketch).unwrap().links.iter().find(|l| l.auto_face).unwrap().clone();
    assert_eq!(link.curve_edges.len(), 4, "{link:?}");
    assert!(link.curve_edges.iter().all(|n| n.contains('|')), "{:?}", link.curve_edges);
    let reopened = solidcraft_doc::Document::from_json(&saved.to_string()).unwrap();
    assert_eq!(reopened.sketch(sketch).unwrap().links.iter().find(|l| l.auto_face).unwrap().curve_edges, link.curve_edges);
    fn strip(v: &mut Value) -> usize {
        match v {
            Value::Object(map) => usize::from(map.remove("curve_edges").is_some()) + map.values_mut().map(strip).sum::<usize>(),
            Value::Array(items) => items.iter_mut().map(strip).sum(),
            _ => 0,
        }
    }
    let mut older = saved.clone();
    assert_eq!(strip(&mut older), 1);
    let older = Session::new(solidcraft_doc::Document::from_json(&older.to_string()).unwrap());
    let (st, old_st) = (s.model.state(), older.model.state());
    let (now, before) = (st.sketch(sketch).unwrap(), old_st.sketch(sketch).unwrap());
    assert!(older.model.result(sketch).and_then(|r| r.warning.clone()).is_none());
    let placed = |ss: &solidcraft_doc::SolvedSketch| ss.sketch.points.iter().map(|p| (p.id.clone(), ss.plane.to_world(p.pos))).collect::<Vec<_>>();
    let (a, b) = (placed(now), placed(before));
    assert_eq!(a.len(), b.len());
    assert!(a.iter().zip(&b).all(|((i, p), (j, q))| i == j && p.dist(*q) < 1e-9), "{a:?} {b:?}");
    assert_eq!(before.sketch.constraints.len(), 2);
    let center = before.sketch.points.iter().find(|p| p.id == format!("{circle}.center")).unwrap();
    assert!(before.plane.to_world(center.pos).dist(panel_corner(95.)) < 1e-6);
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

/// The plate of a design whose whole-body projection on a tilted sketch went wrong: 80 x 50 x 8
/// with its upright edges rounded at 6, a 4 mm hole, and a boss of 12 standing 14 on top with a
/// 6 mm bore right through.
fn bossed_plate() -> Session {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 80, "width": 50, "height": 8, "body_name": "Plate"}));
    run(&mut s, "solid.fillet", json!({"edges": [[0, 0, 4], [80, 0, 4], [80, 50, 4], [0, 50, 4]], "radius": 6}));
    run(&mut s, "solid.cylinder", json!({"base": [12, 25, 0], "radius": 4, "height": 8, "operation": "cut"}));
    run(&mut s, "solid.cylinder", json!({"base": [40, 25, 8], "radius": 12, "height": 14, "operation": "join", "targets": ["Plate"]}));
    run(&mut s, "solid.cylinder", json!({"base": [40, 25, 0], "radius": 6, "height": 22, "operation": "cut"}));
    s
}

/// How the whole-body projection on `sketch` compares with the true outline: the farthest a point
/// of it lies from the true projection, and the most it falls short of the body's extent in any of
/// 16 directions (a missing stretch of outline falls short somewhere). The truth is every edge of
/// the body but a round wall's seam, sampled finely on its exact curve, and each round wall's two
/// outline lines (where the view runs along the wall) between the wall's own ends, in the sketch's
/// coordinates.
fn against_true_outline(s: &Session, sketch: u64) -> (f64, f64) {
    let state = s.model.state();
    let solved = state.sketch(sketch).unwrap();
    let plane = solved.plane;
    let body = state.body("Plate").unwrap();
    let record = body.body.record();
    let q = solidcraft_kernel::services::query::Query::new(record);
    let mut truth: Vec<(solidcore_record::Vec2, solidcore_record::Vec2)> = Vec::new();
    // Each face's edge points, for the axial reach of its round wall.
    let mut on_face: Vec<Vec<solidcore_record::Vec3>> = vec![Vec::new(); record.faces.len()];
    for e in 0..record.edges.len() {
        let [t0, t1] = q.edge_interval(e).unwrap();
        let world: Vec<_> = (0..=4000).map(|k| q.edge_at(e, t0 + (t1 - t0) * f64::from(k) / 4000.0).unwrap().0).collect();
        let uses = q.edge_faces(e);
        for f in &uses {
            on_face[*f].extend(world.iter().copied());
        }
        // A round wall's seam is an edge but no outline: one face lies on both its sides.
        if uses.len() == 2 && uses[0] == uses[1] {
            continue;
        }
        let pts: Vec<_> = world.iter().map(|p| plane.to_local(*p)).collect();
        truth.extend(pts.windows(2).map(|w| (w[0], w[1])));
    }
    let n = plane.normal();
    for (f, edge_points) in on_face.iter().enumerate() {
        if let Some(solidcraft_kernel::FaceSurface::Cylinder { origin, axis, radius }) = solidcraft_kernel::face_surface(&body.body, f) {
            let a = axis.vec();
            let m = a.cross(n).normalized().unwrap();
            let (lo, hi) = edge_points.iter().map(|p| (*p - origin).dot(a)).fold((f64::MAX, f64::MIN), |(lo, hi), t| (lo.min(t), hi.max(t)));
            for side in [1.0, -1.0] {
                let p = origin + m * (radius * side);
                truth.push((plane.to_local(p + a * lo), plane.to_local(p + a * hi)));
            }
        }
    }
    let to_segment = |p: solidcore_record::Vec2, (a, b): (solidcore_record::Vec2, solidcore_record::Vec2)| {
        let d = b - a;
        let t = ((p - a).dot(d) / d.len2().max(1e-300)).clamp(0.0, 1.0);
        p.dist(a + d * t)
    };
    let sk = &solved.sketch;
    let link = &sk.links[0].id;
    let got: Vec<_> = sk.link_curves(link).into_iter().flat_map(|ci| sk.polyline(ci)).collect();
    let off = got.iter().map(|p| truth.iter().map(|seg| to_segment(*p, *seg)).fold(f64::MAX, f64::min)).fold(0.0, f64::max);
    let short = (0..16)
        .map(|k| {
            let u = solidcore_record::Vec2::new((f64::from(k) * std::f64::consts::PI / 8.0).cos(), (f64::from(k) * std::f64::consts::PI / 8.0).sin());
            let reach = |pts: &mut dyn Iterator<Item = solidcore_record::Vec2>| pts.map(|p| p.dot(u)).fold(f64::MIN, f64::max);
            reach(&mut truth.iter().flat_map(|(a, b)| [*a, *b])) - reach(&mut got.iter().copied())
        })
        .fold(0.0, f64::max);
    (off, short)
}

/// A whole body projected on a sketch tilted to it (the 45 degree plane of the design it came from,
/// and the plane square to the Home view) is its exact outline: every round of the plate is a
/// circle about the vertical, which a slant shows as an ellipse, so the outline holds elliptical
/// arcs and no circular one, and each curve lies on the true outline. The mesh's outline was
/// fitted with lines and circular arcs instead: 164 lines and 2 arcs at 45 degrees, up to 0.056 mm
/// off, some bulging the wrong way.
#[test]
fn a_tilted_whole_body_projection_is_the_exact_outline() {
    use solidcraft_sketch::CurveKind;
    let c = std::f64::consts::FRAC_1_SQRT_2;
    let planes = [
        ("45 degrees", json!({"origin": [0, 0, 8], "x_dir": [1, 0, 0], "y_dir": [0, c, c]})),
        ("Home view", json!({"origin": [40, 25, 0], "x_dir": [c, c, 0], "y_dir": [-0.5, 0.5, c]})),
    ];
    for (name, plane) in planes {
        let mut s = bossed_plate();
        let sketch = run(&mut s, "sketch.create", json!({"plane": plane, "project_edges": false}))["sketch"].as_u64().unwrap();
        run(&mut s, "sketch.project", json!({"refs": [{"body": "Plate"}]}));
        run(&mut s, "sketch.finish", json!({}));
        let state = s.model.state();
        let sk = &state.sketch(sketch).unwrap().sketch;
        let curves: Vec<_> = sk.link_curves(&sk.links[0].id).into_iter().map(|ci| &sk.curves[ci].kind).collect();
        assert!(
            !curves.iter().any(|k| matches!(k, CurveKind::Arc { .. } | CurveKind::Circle { .. })),
            "{name}: a circular arc at a slant: {curves:?}"
        );
        assert!(curves.iter().any(|k| matches!(k, CurveKind::Conic { .. })), "{name}: no elliptical arc: {curves:?}");
        // Exact curves only: a conic per span of a round edge's stretch, and a line per stretch
        // of a straight edge or outline line of a round wall. A circle's spline has four quarter
        // spans, and a stretch over its seam is two stretches: at most five conics per round
        // edge; at most one line per straight edge and two per round wall.
        let record = state.body("Plate").unwrap().body.record();
        let rounds = record.edges.iter().filter(|e| matches!(e.curve, solidcore_record::brep::Curve::Circle { .. })).count();
        let straight = record.edges.iter().filter(|e| matches!(e.curve, solidcore_record::brep::Curve::Line { .. })).count();
        let walls = (0..record.faces.len())
            .filter(|f| {
                matches!(
                    solidcraft_kernel::face_surface(&state.body("Plate").unwrap().body, *f),
                    Some(solidcraft_kernel::FaceSurface::Cylinder { .. })
                )
            })
            .count();
        let conics = curves.iter().filter(|k| matches!(k, CurveKind::Conic { .. })).count();
        let lines = curves.iter().filter(|k| matches!(k, CurveKind::Line { .. })).count();
        assert_eq!(conics + lines, curves.len(), "{name}: only conics and lines: {curves:?}");
        assert!(conics <= 5 * rounds, "{name}: {conics} conics for {rounds} round edges");
        assert!(lines <= straight + 2 * walls, "{name}: {lines} lines for {straight} straight edges and {walls} round walls");
        let (off, short) = against_true_outline(&s, sketch);
        // The truth's chords stand within 12 * (pi / 4000)^2 / 8 < 1e-5 mm of its curves. The
        // sketch's points lie on its curves but its chords cut inside a round's extreme, so the
        // reach is checked to 2e-3 mm: a missing stretch of outline falls short by millimetres.
        assert!(off < 2e-5, "{name}: {off} mm off the true outline");
        assert!(short < 2e-3, "{name}: the outline falls {short} mm short of the body's extent");
    }
}

/// Synthetic saved trace for #20298: a diameter-20, height-30 cylinder projected onto the
/// tilted plane from the report. Its outline is two half ellipses joined by two straight
/// sides. Older builds stored a dense line trace, whose IDs the extrude saved as its profile.
fn saved_traced_cylinder_projection() -> (Value, u64, f64) {
    use solidcraft_sketch::{LinkGeom, LinkKind, LinkSource, Sketch};

    let mut s = Session::default();
    run(&mut s, "solid.cylinder", json!({"radius": 10, "height": 30, "body_name": "Cylinder"}));
    let sketch = run(
        &mut s,
        "sketch.create",
        json!({"name": "Outline", "plane": {"origin": [0, 0, 40], "x_dir": [1, 0, 0], "y_dir": [0, 0.866, 0.5]}, "project_edges": false}),
    )["sketch"]
        .as_u64()
        .unwrap();
    run(&mut s, "sketch.project", json!({"refs": [{"body": "Cylinder"}]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"profiles": [0], "distance": 2, "name": "Pad", "body_name": "ProjectionPad"}));
    assert!(s.doc.sketch(sketch).unwrap().links.iter().all(|link| link.profile_boundary.is_empty()), "new projections keep ordinary picks");

    // Fixed old-style trace, generated once from the cylinder and input plane above.
    // Its 130 segments include both straight sides; the repeated first point closes it.
    let points = [
        [10.000000000000, -5.000110003630],
        [9.987954562052, -4.575174595273],
        [9.951847266722, -4.151262893534],
        [9.891765099648, -3.729396138834],
        [9.807852804032, -3.310590645136],
        [9.700312531945, -2.895855351556],
        [9.569403357322, -2.486189391745],
        [9.415440651830, -2.082579686880],
        [9.238795325113, -1.685998568095],
        [9.039892931234, -1.297401434039],
        [8.819212643484, -0.917724449247],
        [8.577286100003, -0.547882288830],
        [8.314696123025, -0.188765934946],
        [8.032075314806, 0.158759469651],
        [7.730104533627, 0.493856705824],
        [7.409511253550, 0.815718494979],
        [7.071067811865, 1.123569443875],
        [6.715589548470, 1.416667912611],
        [6.343932841636, 1.694307801303],
        [5.956993044924, 1.955820251141],
        [5.555702330196, 2.200575255728],
        [5.141027441932, 2.427983178820],
        [4.713967368260, 2.637496174811],
        [4.275550934303, 2.828609508543],
        [3.826834323651, 3.000862771255],
        [3.368898533922, 3.153840989750],
        [2.902846772545, 3.287175626100],
        [2.429801799033, 3.400545465488],
        [1.950903220161, 3.493677390040],
        [1.467304744554, 3.566347036793],
        [0.980171403296, 3.618379338202],
        [0.490676743274, 3.649648943897],
        [0.000000000000, 3.660080522657],
        [-0.490676743274, 3.649648943897],
        [-0.980171403296, 3.618379338202],
        [-1.467304744554, 3.566347036793],
        [-1.950903220161, 3.493677390040],
        [-2.429801799033, 3.400545465488],
        [-2.902846772545, 3.287175626100],
        [-3.368898533922, 3.153840989750],
        [-3.826834323651, 3.000862771255],
        [-4.275550934303, 2.828609508543],
        [-4.713967368260, 2.637496174811],
        [-5.141027441932, 2.427983178820],
        [-5.555702330196, 2.200575255728],
        [-5.956993044924, 1.955820251141],
        [-6.343932841636, 1.694307801303],
        [-6.715589548470, 1.416667912611],
        [-7.071067811865, 1.123569443875],
        [-7.409511253550, 0.815718494979],
        [-7.730104533627, 0.493856705824],
        [-8.032075314806, 0.158759469651],
        [-8.314696123025, -0.188765934946],
        [-8.577286100003, -0.547882288830],
        [-8.819212643484, -0.917724449247],
        [-9.039892931234, -1.297401434039],
        [-9.238795325113, -1.685998568095],
        [-9.415440651830, -2.082579686880],
        [-9.569403357322, -2.486189391745],
        [-9.700312531945, -2.895855351556],
        [-9.807852804032, -3.310590645136],
        [-9.891765099648, -3.729396138834],
        [-9.951847266722, -4.151262893534],
        [-9.987954562052, -4.575174595273],
        [-10.000000000000, -5.000110003630],
        [-10.000000000000, -20.000440014521],
        [-9.987954562052, -20.425375422878],
        [-9.951847266722, -20.849287124616],
        [-9.891765099648, -21.271153879317],
        [-9.807852804032, -21.689959373015],
        [-9.700312531945, -22.104694666594],
        [-9.569403357322, -22.514360626406],
        [-9.415440651830, -22.917970331270],
        [-9.238795325113, -23.314551450056],
        [-9.039892931234, -23.703148584111],
        [-8.819212643484, -24.082825568904],
        [-8.577286100003, -24.452667729321],
        [-8.314696123025, -24.811784083204],
        [-8.032075314806, -25.159309487802],
        [-7.730104533627, -25.494406723975],
        [-7.409511253550, -25.816268513130],
        [-7.071067811865, -26.124119462026],
        [-6.715589548470, -26.417217930762],
        [-6.343932841636, -26.694857819454],
        [-5.956993044924, -26.956370269292],
        [-5.555702330196, -27.201125273879],
        [-5.141027441932, -27.428533196971],
        [-4.713967368260, -27.638046192962],
        [-4.275550934303, -27.829159526693],
        [-3.826834323651, -28.001412789406],
        [-3.368898533922, -28.154391007901],
        [-2.902846772545, -28.287725644251],
        [-2.429801799033, -28.401095483639],
        [-1.950903220161, -28.494227408191],
        [-1.467304744554, -28.566897054944],
        [-0.980171403296, -28.618929356353],
        [-0.490676743274, -28.650198962047],
        [0.000000000000, -28.660630540808],
        [0.490676743274, -28.650198962047],
        [0.980171403296, -28.618929356353],
        [1.467304744554, -28.566897054944],
        [1.950903220161, -28.494227408191],
        [2.429801799033, -28.401095483639],
        [2.902846772545, -28.287725644251],
        [3.368898533922, -28.154391007901],
        [3.826834323651, -28.001412789406],
        [4.275550934303, -27.829159526693],
        [4.713967368260, -27.638046192962],
        [5.141027441932, -27.428533196971],
        [5.555702330196, -27.201125273879],
        [5.956993044924, -26.956370269292],
        [6.343932841636, -26.694857819454],
        [6.715589548470, -26.417217930762],
        [7.071067811865, -26.124119462026],
        [7.409511253550, -25.816268513130],
        [7.730104533627, -25.494406723975],
        [8.032075314806, -25.159309487802],
        [8.314696123025, -24.811784083204],
        [8.577286100003, -24.452667729321],
        [8.819212643484, -24.082825568904],
        [9.039892931234, -23.703148584111],
        [9.238795325113, -23.314551450056],
        [9.415440651830, -22.917970331270],
        [9.569403357322, -22.514360626406],
        [9.700312531945, -22.104694666594],
        [9.807852804032, -21.689959373015],
        [9.891765099648, -21.271153879317],
        [9.951847266722, -20.849287124616],
        [9.987954562052, -20.425375422878],
        [10.000000000000, -20.000440014521],
        [10.000000000000, -5.000110003630],
    ]
    .map(|[x, y]| solidcore_record::Vec2::new(x, y));
    let geometry: Vec<_> = points.windows(2).map(|pair| LinkGeom::Line(pair[0], pair[1])).collect();
    let mut traced = Sketch::new();
    let link = traced.add_link(LinkKind::Project, LinkSource::Body { body: "Cylinder".into() }, &geometry).unwrap();
    let old_ids: Vec<_> = traced.link_curves(&link).iter().map(|i| traced.curves[*i].id.clone()).collect();
    assert_eq!(old_ids.len(), 130);
    assert!(old_ids.iter().all(|id| id.starts_with('l')));
    assert_eq!(solidcraft_sketch::find_profiles(&traced).len(), 1);

    let mut saved: Value = serde_json::from_str(&s.doc.to_json()).unwrap();
    for feature in saved["features"].as_array_mut().unwrap() {
        if feature["id"] == sketch {
            feature["sketch"] = serde_json::to_value(&traced).unwrap();
        } else if feature["name"] == "Pad" {
            feature["profiles"] = json!({"type": "curves", "loops": [old_ids]});
        }
    }
    // A released format, so this remains a pre-migration file after the pending step is folded.
    saved["format"] = json!("solidcraft/63");
    saved.as_object_mut().unwrap().remove("format_pending");
    // Let q = sqrt(0.866^2 + 0.5^2) normalize the specified plane's y direction.
    // Projected area = ellipse pi*10^2*(0.866/q) + axial rectangle 20*30*(0.5/q).
    // Extruding 2 mm gives 2*(pi*100*0.866 + 600*0.5)/q = 1144.149019157064 mm^3.
    (saved, sketch, 1144.149019157064)
}

#[test]
fn a_saved_traced_whole_body_profile_rebuilds_with_the_exact_projection() {
    let (saved, sketch, expected) = saved_traced_cylinder_projection();
    let mut s = Session::new(solidcraft_doc::Document::from_json(&saved.to_string()).unwrap());
    let assert_rebuilt = |s: &Session| {
        for feature in &s.doc.features {
            let result = s.model.result(feature.id).unwrap();
            assert!(result.error.is_none(), "{}: {:?}", feature.name, result.error);
        }
        let state = s.model.state();
        let ss = state.sketch(sketch).unwrap();
        assert!(!ss.sketch.links[0].profile_boundary.is_empty());
        assert!(ss.sketch.curves.iter().any(|curve| matches!(curve.kind, solidcraft_sketch::CurveKind::Conic { .. })));
        let solidcraft_doc::FeatureKind::Extrude { profiles, .. } = &s.doc.find_feature("Pad").unwrap().kind else {
            panic!("Pad must remain an extrude");
        };
        let solidcraft_doc::ProfileSel::Curves { loops } = profiles else { panic!("Pad must retain its saved curve IDs") };
        assert_eq!(loops.len(), 1);
        // The rebuilt sketch cannot resolve the saved IDs directly: the migrated reference
        // must identify the exact outline, rather than extruding the unchanged old trace.
        assert_eq!(solidcraft_sketch::profile_by_curves(&ss.profiles, &loops[0]), Err(solidcraft_sketch::CurvePick::Missing));
        assert_eq!(solidcraft_doc::profile_selection_indices(ss, profiles).unwrap(), vec![0]);
        let pad = state.body("ProjectionPad").unwrap();
        let actual = solidcraft_kernel::measure(&pad.body).unwrap().volume;
        assert!((actual - expected).abs() < 1e-6 * expected, "{actual} vs {expected}");
    };
    assert_rebuilt(&s);
    // Evaluation refreshes a copy in model state. A no-op edit/finish does not write that
    // copy into the document; check the rebuilt outline where evaluation keeps it.
    run(&mut s, "sketch.edit", json!({"sketch": "Outline"}));
    run(&mut s, "sketch.finish", json!({}));
    assert_rebuilt(&s);
    // Opening the migrated save cold must preserve the reference and rebuild the exact outline.
    let reopened = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
    assert_rebuilt(&reopened);
}

#[test]
fn a_saved_projection_profile_that_is_split_is_not_guessed() {
    let (saved, sketch, _) = saved_traced_cylinder_projection();
    let mut doc = solidcraft_doc::Document::from_json(&saved.to_string()).unwrap();
    // Both halves use curves of the same source. Neither covers its complete boundary, so
    // neither can stand for the old outline (nor may lenient curve matching choose their union).
    let sk = doc.sketch_mut(sketch).unwrap();
    assert!(!sk.links[0].profile_boundary.is_empty());
    sk.add_line(solidcore_record::Vec2::new(-20., -12.5), solidcore_record::Vec2::new(20., -12.5), None, None, None).unwrap();
    let s = Session::new(solidcraft_doc::Document::from_json(&doc.to_json()).unwrap());
    assert_eq!(s.model.state().sketch(sketch).unwrap().profiles.len(), 2);
    let pad = s.doc.find_feature("Pad").unwrap();
    let error = s.model.result(pad.id).unwrap().error.as_deref().unwrap();
    assert!(error.contains("no profile of Outline is bounded by exactly the curves"), "{error}");
    assert!(s.model.state().body("ProjectionPad").is_none());
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

/// The silhouette of a letter-sized outline with small conic rounds, far from the origin, is one
/// closed outline (#14987). A round projected as two fitted arcs (or a line and an arc) left
/// their shared end 0.0003 to 0.001 mm apart, wider than profiles and Offset join ends, so the
/// projected outline of an extruded letter made no profile and could not be offset.
#[test]
fn the_silhouette_of_small_conic_rounds_far_out_closes() {
    for (rho, r, (cx, cz)) in [(0.5, 0.7, (-60., 250.)), (0.5, 0.4, (-140., 240.)), (0.45, 0.9, (90., -270.)), (0.5, 0.25, (200., 180.))] {
        let mut s = Session::default();
        let h = 6.0;
        // A square of side 2h round (cx, cz), each corner a conic round of size r.
        let corners = [
            ([cx - h, cz + h - r], [cx - h, cz + h], [cx - h + r, cz + h]),
            ([cx + h - r, cz + h], [cx + h, cz + h], [cx + h, cz + h - r]),
            ([cx + h, cz - h + r], [cx + h, cz - h], [cx + h - r, cz - h]),
            ([cx - h + r, cz - h], [cx - h, cz - h], [cx - h, cz - h + r]),
        ];
        run(&mut s, "sketch.create", json!({"plane": "YZ"}));
        for (i, (start, apex, end)) in corners.iter().enumerate() {
            run(&mut s, "sketch.conic", json!({"start": start, "apex": apex, "end": end, "rho": rho}));
            let next = corners[(i + 1) % 4].0;
            run(&mut s, "sketch.line", json!({"points": [end, next]}));
        }
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "solid.extrude", json!({"distance": 100, "body_name": "Letter"}));
        run(&mut s, "sketch.create", json!({"plane": "YZ"}));
        run(&mut s, "sketch.project", json!({"refs": [{"body": "Letter"}], "construction": false}));
        let si = inspect(&mut s);
        let profiles = si["profiles"].as_array().unwrap();
        assert_eq!(profiles.len(), 1, "rho {rho}, round {r} at ({cx}, {cz}): {}", si["profiles"]);
    }
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

/// A part (not grounded) holding 40 x 20 x 10 block `Block` extruded from rectangle `Base` (`l1`
/// its front side, y = 0) and a second block `Block2` at x = 60, a construction plane Guide 30
/// off XZ, sketch Outline on it projecting both blocks' front faces beside a circle of its own,
/// and Peg extruded from that circle. Returns (session, Outline, Block's link, Block2's link).
fn part_with_outline() -> (Session, u64, String, String) {
    let mut s = Session::default();
    run(&mut s, "component.create", json!({"name":"Part","grounded":false}));
    run(&mut s, "sketch.create", json!({"plane":"XY","name":"Base"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[40,20]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"sketch":"Base","distance":10,"body_name":"Block"}));
    run(&mut s, "sketch.create", json!({"plane":"XY","name":"Base2"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0":[60,0],"p1":[100,20]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"sketch":"Base2","distance":10,"body_name":"Block2"}));
    run(&mut s, "construct.plane.offset", json!({"base":"XZ","offset":30,"name":"Guide"}));
    let outline = run(&mut s, "sketch.create", json!({"plane":"Guide","name":"Outline"}))["sketch"].as_u64().unwrap();
    let link = |r: Value| r["links"][0]["link"].as_str().unwrap().to_string();
    let front = link(run(&mut s, "sketch.project", json!({"refs":[{"face":[20,0,5],"body":"Block"}]})));
    let other = link(run(&mut s, "sketch.project", json!({"refs":[{"face":[80,0,5],"body":"Block2"}]})));
    let circle = run(&mut s, "sketch.circle.center", json!({"center":[50,5],"radius":3}))["created"]["curves"][0].as_str().unwrap().to_string();
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"sketch":"Outline","profiles":[{"curves":[circle]}],"distance":5,"name":"Peg"}));
    run(&mut s, "component.activate", json!({"component":0}));
    assert_eq!(s.model.result(outline).unwrap().error, None);
    assert!(s.doc.sketch(outline).unwrap().link(&front).unwrap().face_name.is_some(), "kept by the face's name");
    (s, outline, front, other)
}

/// The block's front side drawn again: its face takes a new name, so Outline's projection of it
/// finds nothing. Returns the name it was kept by.
fn redraw_front(s: &mut Session, outline: u64, front: &str) -> String {
    let name = s.doc.sketch(outline).unwrap().link(front).unwrap().face_name.clone().unwrap();
    run(s, "sketch.edit", json!({"sketch":"Base"}));
    run(s, "sketch.delete", json!({"entities":["l1"]}));
    run(s, "sketch.line", json!({"points":[[0,0],[40,0]]}));
    run(s, "sketch.finish", json!({}));
    name
}

/// The curves of link `link` in `sketch` as the design stores them: `curves l1, l2, l3, l4`.
fn curves_named(s: &Session, sketch: u64, link: &str) -> String {
    let sk = s.doc.sketch(sketch).unwrap();
    let ids: Vec<&str> = sk.link_curves(link).into_iter().map(|c| sk.curves[c].id.as_str()).collect();
    format!("curves {}", ids.join(", "))
}

/// The error of the feature with this name.
fn error_of(s: &Session, name: &str) -> Option<String> {
    s.model.result(s.doc.find_feature(name).unwrap().id).and_then(|r| r.error.clone())
}

#[test]
fn a_sketch_whose_projected_face_lost_its_name_says_which_projection_and_how_to_fix_it() {
    let (mut s, outline, front, _) = part_with_outline();
    let name = redraw_front(&mut s, outline, &front);
    let curves = curves_named(&s, outline, &front);
    // The extrude after Base made the face: the message names it as where to look.
    let block = s.doc.features.iter().position(|f| f.name == "Base").and_then(|i| s.doc.features.get(i + 1)).map(|f| f.name.clone()).unwrap();
    assert_eq!(
        error_of(&s, "Outline").as_deref(),
        Some(format!("Outline: projected face of Block ({curves}; made by {block}) is lost; delete the projection or project it again").as_str())
    );
    assert!(!error_of(&s, "Outline").unwrap().contains(&name), "the persistent name is not shown");
    let lost = s.model.result(outline).and_then(|r| r.lost_projections.clone()).unwrap();
    assert_eq!(lost.iter().map(|l| (l.link.as_str(), l.name.as_str())).collect::<Vec<_>>(), [(front.as_str(), name.as_str())]);
    assert!(error_of(&s, "Peg").is_some_and(|e| e.contains("Outline")), "{:?}", error_of(&s, "Peg"));
    assert!(s.model.result(s.doc.find_feature("Peg").unwrap().id).unwrap().lost_projections.is_none(), "only the sketch lost them");
    // As the message says: the lost projection deleted, the sketch and the peg build again.
    let projected: Vec<String> = curves.trim_start_matches("curves ").split(", ").map(str::to_string).collect();
    run(&mut s, "sketch.edit", json!({"sketch":outline}));
    run(&mut s, "sketch.delete", json!({"entities":projected}));
    run(&mut s, "sketch.finish", json!({}));
    assert_eq!(error_of(&s, "Outline"), None);
    assert_eq!(error_of(&s, "Peg"), None);
}

/// Redefine Sketch Plane on a sketch that fails only for lost projections changes the plane; the
/// sketch then fails as before, for the same projection, and so does the feature built on it.
#[test]
fn a_sketch_failing_for_lost_projections_takes_a_new_plane_and_still_fails_for_them() {
    let (mut s, outline, front, _) = part_with_outline();
    redraw_front(&mut s, outline, &front);
    let failed = error_of(&s, "Outline").unwrap();
    let plane = |s: &Session| {
        let solidcraft_doc::FeatureKind::Sketch { plane, .. } = &s.doc.feature(outline).unwrap().kind else { panic!("not a sketch") };
        serde_json::to_value(plane).unwrap()
    };
    let before = plane(&s);
    run(&mut s, "sketch.redefine", json!({"sketch":outline,"plane":"XZ"}));
    assert_ne!(plane(&s), before, "the plane changed");
    assert_eq!(error_of(&s, "Outline"), Some(failed.clone()), "the same projection is lost");
    assert!(error_of(&s, "Peg").is_some());
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(plane(&s), before);
    assert_eq!(error_of(&s, "Outline"), Some(failed.clone()));
    // With the timeline rolled back to before the sketch: the plane is checked as at the sketch.
    let index = s.doc.feature_index(outline).unwrap();
    run(&mut s, "timeline.roll_to", json!({"position": index}));
    assert_eq!(error_of(&s, "Outline"), None, "rolled back");
    run(&mut s, "sketch.redefine", json!({"sketch":outline,"plane":"XZ"}));
    assert_ne!(plane(&s), before);
    run(&mut s, "timeline.roll_to", json!({}));
    assert_eq!(error_of(&s, "Outline"), Some(failed));
}

/// A plane is refused when the sketch would fail on it for a projection it did not lose on its
/// old one: the projections it loses on the new plane must be among the old plane's.
#[test]
fn a_new_plane_is_judged_by_the_projections_the_old_one_lost() {
    let (mut s, outline, front, _) = part_with_outline();
    redraw_front(&mut s, outline, &front);
    assert!(crate::cmd::browser::only_lost_before(&s.model.clone(), &s, outline), "the same projection lost");
    let mut before = s.model.clone();
    let index = before.results.iter().position(|r| r.id == outline).unwrap();
    before.results[index].lost_projections = Some(std::sync::Arc::new(Vec::new()));
    assert!(!crate::cmd::browser::only_lost_before(&before, &s, outline), "the front face's projection was not lost before");
    before.results[index].error = None;
    before.results[index].lost_projections = None;
    assert!(!crate::cmd::browser::only_lost_before(&before, &s, outline), "the sketch built before");
}

/// A sketch that fails for a lost projection and, past its projections, for a dimension of its
/// own is refused a new plane: on it, it would still fail for more than the lost projection.
#[test]
fn a_new_plane_is_refused_when_the_sketch_fails_for_more_than_lost_projections() {
    let (mut s, outline, front, _) = part_with_outline();
    run(&mut s, "parameters.add", json!({"name":"rim","expression":"3 mm"}));
    let part = s.doc.occurrences.iter().find(|o| o.name.starts_with("Part")).unwrap().clone();
    run(&mut s, "component.activate", json!({"component":part.component,"occurrence":part.id,"path":[part.id]}));
    run(&mut s, "sketch.edit", json!({"sketch":outline}));
    let circle = s.doc.sketch(outline).unwrap().curves.iter().find(|c| c.link.is_none()).unwrap().id.clone();
    run(&mut s, "sketch.dimension", json!({"entities":[circle],"type":"radius","value":"rim"}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "component.activate", json!({"component":0}));
    redraw_front(&mut s, outline, &front);
    run(&mut s, "parameters.change", json!({"name":"rim","expression":"-3 mm"}));
    let failed = error_of(&s, "Outline").unwrap();
    assert!(failed.contains("projected face of Block"), "the lost projection is what it says: {failed}");
    assert!(s.model.failed_sketch_shown(&s.doc, outline).is_none(), "it fails past its projections too");
    let before = s.doc.to_json();
    let e = s.execute("sketch.redefine", &json!({"sketch":outline,"plane":"XZ"})).unwrap_err().to_string();
    assert!(e.contains("projected face of Block"), "{e}");
    assert_eq!(s.doc.to_json(), before);
}

/// A sketch whose own plane, a face, was made again with the face it projects: it fails on its
/// plane before it reaches the projection. A new plane is taken, and the sketch then fails for
/// the projection (a name is looked up on its body, never on the plane).
#[test]
fn a_sketch_whose_plane_and_projection_were_lost_takes_a_new_plane() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length":10,"width":10,"height":10,"body_name":"Base","name":"Original"}));
    let top = s
        .model
        .state()
        .body("Base")
        .unwrap()
        .body
        .faces(0.01)
        .unwrap()
        .into_iter()
        .find(|f| f.plane_normal.is_some_and(|n| n.z > 0.99))
        .unwrap()
        .index;
    let id = run(&mut s, "sketch.create", json!({"plane":{"face":[5,5,10],"body":"Base","index":top},"project_edges":false,"name":"Cap"}))["sketch"]
        .as_u64()
        .unwrap();
    run(&mut s, "sketch.project", json!({"refs":[{"face":[5,0,5],"body":"Base"}]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "timeline.suppress", json!({"feature":"Original","suppressed":true}));
    run(&mut s, "timeline.rollback", json!({"position":1}));
    run(&mut s, "solid.box", json!({"length":10,"width":10,"height":30,"body_name":"Base","name":"Replacement"}));
    run(&mut s, "timeline.rollback", json!({}));
    let failed = s.model.result(id).unwrap();
    assert!(failed.error.is_some() && failed.lost_projections.is_none(), "its plane fails first: {:?}", failed.error);
    run(&mut s, "sketch.redefine", json!({"sketch":id,"plane":"XZ"}));
    let lost = s.model.result(id).and_then(|r| r.lost_projections.clone()).expect("now it fails for its projection");
    assert_eq!(lost.len(), 1);
    assert!(s.model.failed_sketch_shown(&s.doc, id).is_some());
    run(&mut s, "edit.undo", json!({}));
    assert!(s.model.result(id).unwrap().lost_projections.is_none(), "undone, its plane fails again");
}

/// A sketch failing only for lost projections is shown placed where its component puts it, its
/// lost projection marked, from a list no command reads: neither the model nor the world has it,
/// and a new feature can't use its profiles.
#[test]
fn a_sketch_failing_for_lost_projections_is_shown_where_it_is_and_never_used() {
    let (mut s, outline, front, other) = part_with_outline();
    redraw_front(&mut s, outline, &front);
    let part = s.doc.occurrences.iter().find(|o| o.name.starts_with("Part")).unwrap().id;
    let shown = |s: &Session| s.sketches_placed(s.model.failed_sketch_shown(&s.doc, outline).into_iter().collect()).unwrap();
    let first = shown(&s);
    let [ss] = first.as_slice() else { panic!("{} shown", first.len()) };
    assert!(ss.sketch.link(&front).unwrap().lost);
    assert!(!ss.sketch.link(&other).unwrap().lost);
    assert!(ss.sketch.curves.len() > ss.sketch.link_curves(&front).len(), "the rest of the sketch is there too");
    assert!((ss.plane.origin.y.abs() - 30.0).abs() < 1e-6, "on Guide: {:?}", ss.plane.origin);
    run(&mut s, "occurrence.move", json!({"occurrence":part,"translate":[0,0,15]}));
    let moved = shown(&s);
    assert!((moved[0].plane.origin.z - ss.plane.origin.z - 15.0).abs() < 1e-6, "{:?} {:?}", moved[0].plane.origin, ss.plane.origin);
    assert!(s.model.state().sketch(outline).is_none());
    assert!(s.world_state().unwrap().sketch(outline).is_none());
    let features = s.doc.features.len();
    assert!(s.execute("solid.extrude", &json!({"sketch":"Outline","distance":3,"name":"More"})).is_err());
    assert_eq!(s.doc.features.len(), features);
    // Built again (its lost projection deleted), it is the model's own and nothing to place.
    let sk = s.doc.sketch(outline).unwrap();
    let projected: Vec<String> = sk.link_curves(&front).into_iter().map(|c| sk.curves[c].id.clone()).collect();
    run(&mut s, "sketch.edit", json!({"sketch":outline}));
    run(&mut s, "sketch.delete", json!({"entities":projected}));
    run(&mut s, "sketch.finish", json!({}));
    assert_eq!(error_of(&s, "Outline"), None);
    assert!(shown(&s).is_empty());
    assert!(s.model.state().sketch(outline).is_some());
}

/// The active sketch holds at least two linked arcs or circles, each of `radius` about `center`
/// (sketch coordinates): the projected rim of a round hole.
fn assert_rim(s: &mut Session, radius: f64, center: [f64; 2]) {
    let si = inspect(s);
    let arcs: Vec<Value> = linked_curves(&si).into_iter().filter(|c| c["type"] == "arc" || c["type"] == "circle").collect();
    assert_eq!(arcs.len(), 1, "the opening is one full circle, got {si}");
    for c in &arcs {
        let r = c["radius"].as_f64().unwrap_or(f64::NAN);
        assert!((r - radius).abs() < 1e-4, "{c}");
        let at = &c["center_at"];
        assert!((at[0].as_f64().unwrap_or(f64::NAN) - center[0]).abs() < 1e-4, "{c}");
        assert!((at[1].as_f64().unwrap_or(f64::NAN) - center[1]).abs() < 1e-4, "{c}");
        assert!(c["link"].as_str().is_some(), "{c}");
    }
}

/// A click in a hole hits no face. The ray through the opening still names the rim, and
/// projecting those edges draws the circle. A large hole is too far from every face for a
/// point dropped in the opening to resolve, which is why the rim has to be found as a loop.
#[test]
fn a_ray_through_a_hole_opening_projects_its_rim() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 40, "width": 30, "height": 20}));
    run(&mut s, "solid.hole", json!({"position": [20, 15, 20], "diameter": 10, "depth": 20}));
    let st = s.model.state();
    let down = solidcore_record::Vec3::new(0.0, 0.0, -1.0);
    let rim = solidcraft_doc::project::hole_rim_along_ray(&st, solidcore_record::Vec3::new(20.0, 15.0, 80.0), down, |_| true)
        .expect("the ray through the opening names the rim");
    // SMLib holds a full round rim as one edge; a kernel that splits it holds two halves.
    assert!(matches!(rim.edges.len(), 1 | 2), "{rim:?}");
    assert!(rim.at.dist(solidcore_record::Vec3::new(20.0, 15.0, 20.0)) < 1e-4, "{rim:?}");
    let angled = solidcore_record::Vec3::new(0.0, 45.0, -30.0);
    let from_side = solidcraft_doc::project::hole_rim_along_ray(&st, solidcore_record::Vec3::new(20.0, -30.0, 50.0), angled, |_| true);
    assert!(from_side.as_ref().is_some_and(|r| matches!(r.edges.len(), 1 | 2)), "{from_side:?}");
    assert!(solidcraft_doc::project::hole_rim_along_ray(&st, solidcore_record::Vec3::new(5.0, 5.0, 80.0), down, |_| true).is_none());
    assert!(
        solidcraft_doc::project::hole_rim_along_ray(
            &st,
            solidcore_record::Vec3::new(20.0, 15.0, 80.0),
            solidcore_record::Vec3::new(0.0, 0.0, 1.0),
            |_| true
        )
        .is_none()
    );
    assert!(solidcraft_doc::project::hole_rim_along_ray(&st, solidcore_record::Vec3::new(20.0, 15.0, 80.0), down, |_| false).is_none());
    run(&mut s, "sketch.create", json!({"plane": "XY", "offset": 30}));
    let refs: Vec<Value> = rim.edges.iter().map(|p| json!({"edge": [p.x, p.y, p.z], "body": rim.body})).collect();
    let projected = run(&mut s, "sketch.project", json!({"refs": refs}));
    assert_eq!(projected["links"].as_array().map(|a| a.len()), Some(rim.edges.len()), "{projected}");
    assert_rim(&mut s, 5.0, [20.0, 15.0]);

    // Diameter 80 in a 100 plate: the opening's centre is farther from every face than
    // sketch.project will travel to find one, so a point dropped there is not a face.
    let mut wide = Session::default();
    run(&mut wide, "solid.box", json!({"length": 100, "width": 100, "height": 10}));
    run(&mut wide, "solid.hole", json!({"position": [50, 50, 10], "diameter": 80, "depth": 10}));
    let st = wide.model.state();
    run(&mut wide, "sketch.create", json!({"plane": "XY", "offset": 20}));
    let err = wide.execute("sketch.project", &json!({"refs": [{"face": [50.0, 50.0, 10.0]}]})).expect_err("a point in a large opening is not a face");
    let msg = err.to_string();
    assert!(msg.contains("the face no longer exists"), "{msg}");
    assert!(!msg.contains("no sketch is being edited"), "{msg}");
    let rim = solidcraft_doc::project::hole_rim_along_ray(&st, solidcore_record::Vec3::new(50.0, 50.0, 40.0), down, |_| true).expect("large opening");
    // SMLib holds a full round rim as one edge; a kernel that splits it holds two halves.
    assert!(matches!(rim.edges.len(), 1 | 2), "{rim:?}");
    let refs: Vec<Value> = rim.edges.iter().map(|p| json!({"edge": [p.x, p.y, p.z], "body": rim.body})).collect();
    run(&mut wide, "sketch.project", json!({"refs": refs}));
    assert_rim(&mut wide, 40.0, [50.0, 50.0]);

    // A displayed occurrence can share its definition's mesh. The exact B-rep it places is the
    // one the ray meets, in the displayed body's world position, not back at the definition.
    let mut placed = Session::default();
    run(&mut placed, "solid.box", json!({"length": 40, "width": 30, "height": 20}));
    run(&mut placed, "solid.hole", json!({"position": [20, 15, 20], "diameter": 10, "depth": 20}));
    let part = run(&mut placed, "component.create", json!({"name": "Part", "parent": 0, "grounded": false}));
    let occurrence = part["occurrence"].as_u64().unwrap();
    assert_eq!(placed.world_state().unwrap().bodies.len(), 1);
    // Put all features in the part before placing it, retaining its geometry in world coordinates.
    let features: Vec<u64> = placed.doc.features.iter().map(|f| f.id).collect();
    run(&mut placed, "component.move_features", json!({"features": features, "component": part["component"]}));
    assert_eq!(placed.world_state().unwrap().bodies.len(), 1);
    run(
        &mut placed,
        "occurrence.move",
        json!({"occurrence": occurrence, "translate": [100, 250, 40], "axis": [0, 0, 1], "angle": "90 deg", "space": "world"}),
    );
    let st = placed.world_state().unwrap();
    let body = st.bodies.first().expect("the placed part");
    let rim = solidcraft_doc::project::hole_rim_along_ray(&st, solidcore_record::Vec3::new(85.0, 270.0, 180.0), down, |_| true);
    let rim = rim.expect("a ray through the moved opening names its rim");
    assert_eq!(rim.body, body.name);
    assert!(rim.at.dist(solidcore_record::Vec3::new(85.0, 270.0, 60.0)) < 1e-4, "{rim:?}");
    assert!(rim.edges.iter().all(|p| (p.z - 60.0).abs() < 1e-4 && (p.z - 60.0).abs() + rim.at.dist(*p) > 0.0), "{rim:?}");
}

/// Moving a sketch onto a parallel plane lifts it along the planes' normal: every point keeps
/// its world x and y, and only z changes. A sketch on XY holding the projected outline of a
/// 60 x 80 x 15 block's top face and a spline whose ends lie on two of those projected lines,
/// redefined onto the midplane between the block's top and bottom: today the spline keeps its
/// sketch coordinates on the midplane's own origin (the face centre), so it shifts by (30, 40)
/// and its ends slide along the lines ([T-181](https://tickets.solidcraft.io/t/181), #18260).
#[test]
fn redefining_a_sketch_onto_a_parallel_plane_keeps_its_points_where_they_were() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 60, "width": 80, "height": 15, "body_name": "Tray"}));
    run(&mut s, "sketch.create", json!({"plane": {"origin": [0, 0, 0], "x_dir": [1, 0, 0], "y_dir": [0, 1, 0]}, "name": "Wave"}));
    run(&mut s, "sketch.project", json!({"refs": [{"face": [30, 40, 15], "body": "Tray"}]}));
    run(&mut s, "sketch.spline.fit_point", json!({"points": [[0, 30], [20, 45], [40, 25], [60, 50]]}));
    // The spline's ends on the projected sides x = 0 and x = 60.
    let created = run(&mut s, "sketch.inspect", json!({"sketch": "Wave"}));
    let spline = created["curves"].as_array().unwrap().iter().find(|c| c["type"] == "spline").unwrap().clone();
    let ends = spline["points"].as_array().unwrap();
    let side = |x: f64| {
        created["curves"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["type"] == "line" && c["start_at"][0].as_f64() == Some(x) && c["end_at"][0].as_f64() == Some(x))
            .unwrap()["id"]
            .clone()
    };
    run(&mut s, "sketch.constraint.coincident", json!({"a": ends[0], "b": side(0.0)}));
    run(&mut s, "sketch.constraint.coincident", json!({"a": ends[ends.len() - 1], "b": side(60.0)}));
    run(&mut s, "sketch.finish", json!({}));
    let world = |s: &mut Session| -> std::collections::BTreeMap<String, [f64; 3]> {
        run(s, "sketch.inspect", json!({"sketch": "Wave"}))["points"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|p| p["id"] != "origin")
            .map(|p| (p["id"].as_str().unwrap().to_string(), [0, 1, 2].map(|i| p["world"][i].as_f64().unwrap())))
            .collect()
    };
    let before = world(&mut s);
    run(&mut s, "construct.plane.midplane", json!({"a": {"face": [30, 40, 15]}, "b": {"face": [30, 40, 0]}, "name": "Mid"}));
    run(&mut s, "sketch.redefine", json!({"sketch": "Wave", "plane": "Mid"}));
    let after = world(&mut s);
    assert_eq!(before.keys().collect::<Vec<_>>(), after.keys().collect::<Vec<_>>());
    for (id, [x, y, z]) in &before {
        let [x2, y2, z2] = after[id];
        assert!(z.abs() < 1e-9 && (z2 - 7.5).abs() < 1e-9, "{id}: z {z} -> {z2}");
        assert!((x2 - x).abs() < 1e-6 && (y2 - y).abs() < 1e-6, "{id}: ({x}, {y}) -> ({x2}, {y2})");
    }
}

#[test]
fn an_edge_point_on_edges_of_two_bodies_is_refused_unless_a_body_is_named() {
    // #20504: A and B share the edge line x = 0, z = 10; a point on it named no body and the
    // sketch silently followed A's 20 mm edge.
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length":20, "width":20, "height":10, "corner":[0,0,0], "name":"BoxA", "operation":"new", "body_name":"A"}));
    run(&mut s, "solid.box", json!({"length":30, "width":10, "height":10, "corner":[0,0,0], "name":"BoxB", "operation":"new", "body_name":"B"}));
    run(&mut s, "sketch.create", json!({"plane":{"face":[25,5,10]}, "project_edges":false, "name":"OnB"}));
    let e = s.execute("sketch.project", &json!({"refs":[{"edge":[0,5,10]}]})).expect_err("a point on edges of A and B is ambiguous").to_string();
    assert!(e.contains("lies on edges of 2 bodies") && e.contains('A') && e.contains('B') && e.contains("project"), "{e}");
    run(&mut s, "sketch.project", json!({"refs":[{"edge":[0,5,10], "body":"B"}]}));
    // An edge only one body has is still found without a name.
    run(&mut s, "sketch.project", json!({"refs":[{"edge":[30,5,10]}]}));
    let links = linked_curves(&inspect(&mut s));
    assert_eq!(links.len(), 2, "{links:?}");
}

#[test]
fn an_edge_point_is_owned_by_the_body_placed_there_not_one_with_the_same_local_edge() {
    // #20504 review: two parts with the same box at their own origins, the second placed 30 mm
    // along x. Only the first has an edge at [0, 5, 10] where they are placed, so the point is
    // its edge, not one on edges of two bodies.
    let mut s = Session::default();
    run(&mut s, "component.create", json!({"name": "PartA", "grounded": false}));
    run(&mut s, "solid.box", json!({"length": 20, "width": 20, "height": 10, "corner": [0, 0, 0], "operation": "new", "body_name": "A"}));
    run(&mut s, "component.create", json!({"name": "PartB", "parent": "root", "grounded": false}));
    run(&mut s, "solid.box", json!({"length": 20, "width": 20, "height": 10, "corner": [0, 0, 0], "operation": "new", "body_name": "B"}));
    run(&mut s, "occurrence.move", json!({"occurrence": "PartB", "translate": [30, 0, 0], "capture": true}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "offset": 10, "name": "Top"}));
    run(&mut s, "sketch.project", json!({"refs": [{"edge": [0, 5, 10]}]}));
    let links = linked_curves(&inspect(&mut s));
    assert_eq!(links.len(), 1, "{links:?}");
    let sk = s.doc.sketch(s.active_sketch.unwrap()).unwrap();
    assert!(matches!(&sk.links[0].source, solidcraft_sketch::LinkSource::Edge { body, .. } if body.contains('A')), "{:?}", sk.links[0].source);
}

#[test]
fn an_edge_point_without_a_body_keeps_the_one_body_whose_edge_it_is() {
    // #20504 review: [0, 5, 10] lies on an edge of A and inside B's top face. B comes first and
    // its face is as near as A's, so looking the body up by the nearest face took B and projected
    // B's edge 5 mm away. The one body with an edge there is A.
    let mut s = Session::default();
    run(
        &mut s,
        "solid.box",
        json!({"length": 30, "width": 20, "height": 10, "corner": [-10, 0, 0], "name": "BoxB", "operation": "new", "body_name": "B"}),
    );
    run(
        &mut s,
        "solid.box",
        json!({"length": 20, "width": 20, "height": 10, "corner": [0, 0, 0], "name": "BoxA", "operation": "new", "body_name": "A"}),
    );
    run(&mut s, "sketch.create", json!({"plane": "XY", "offset": 10, "name": "Top"}));
    run(&mut s, "sketch.project", json!({"refs": [{"edge": [0, 5, 10]}]}));
    let sk = s.doc.sketch(s.active_sketch.unwrap()).unwrap();
    assert!(matches!(&sk.links[0].source, solidcraft_sketch::LinkSource::Edge { body, .. } if body == "A"), "{:?}", sk.links[0].source);
}

/// #20503: on a face sketch, a rectangle with sides on the face's projected outline is picked by
/// its own four curves. Each piece where a drawn side lies on a projected edge was named after the
/// projected curve only, so the drawn curves bounded no profile.
#[test]
fn a_rectangle_with_sides_on_the_projected_face_outline_is_picked_by_its_own_curves() {
    for (corner, far, sides_on_outline) in [([2.0, 2.0], [10.0, 18.0], 0), ([0.0, 2.0], [10.0, 18.0], 1), ([0.0, 0.0], [10.0, 20.0], 3)] {
        let mut s = Session::default();
        run(
            &mut s,
            "solid.box",
            json!({"length": 40, "width": 20, "height": 10, "corner": [0, 0, 0], "name": "Box1", "operation": "new", "body_name": "Plate"}),
        );
        run(&mut s, "sketch.create", json!({"plane": {"face": [20, 10, 10]}, "name": "Top"}));
        let curves = run(&mut s, "sketch.rectangle.two_point", json!({"p0": corner, "p1": far}))["curves"].clone();
        run(&mut s, "sketch.finish", json!({}));
        s.execute(
            "solid.extrude",
            &json!({"sketch": "Top", "profiles": [{"curves": curves}], "distance": 5, "operation": "new", "name": "Boss", "body_name": "Boss"}),
        )
        .unwrap_or_else(|e| panic!("{sides_on_outline} sides on the outline: {e}"));
        let boss = s.model.state().body("Boss").unwrap().body.clone();
        let volume = solidcraft_kernel::measure(&boss).unwrap().volume;
        let want = (far[0] - corner[0]) * (far[1] - corner[1]) * 5.0;
        assert!((volume - want).abs() < 1e-6 * want, "{sides_on_outline} sides on the outline: {volume} vs {want}");
    }
}
