use serde_json::json;
use solidcraft_engine::Sel;
use solidcraft_ui_egui::scenario::Harness;

fn setup() -> (Harness, u64) {
    let mut h = Harness::new();
    // These cover the direct pick workflow; the options dialog is covered in dialogs_sketch_tests.
    h.app.preferences.project_options_dialog = false;
    let source = h.app.run("sketch.create", json!({"plane":"XY","offset":10})).unwrap()["sketch"].as_u64().unwrap();
    h.app.run("sketch.line", json!({"points":[[10,10],[30,10]],"infer":false})).unwrap();
    h.app.run("sketch.point", json!({"point":[20,20],"id":"p"})).unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
    h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    h.app.run("sketch.line", json!({"points":[[40,40],[50,40]],"infer":false})).unwrap();
    h.app.run("sketch.point", json!({"point":[45,50],"id":"p"})).unwrap();
    h.call("ui.set", json!({"shownSketches":[source]}));
    h.call("ui.view", json!({"view":"top"}));
    (h, source)
}

#[test]
fn clicked_external_curve_and_point_project_with_source_identity_and_one_undo() {
    let (mut h, source) = setup();
    h.step(&json!({"click":{"world":[20,10,10]}})).unwrap();
    h.step(&json!({"click":{"world":[20,20,10]},"shift":true})).unwrap();
    assert_eq!(
        h.app.session.selection,
        vec![Sel::ExternalSketchCurve { sketch: source, id: "l1".into() }, Sel::ExternalSketchPoint { sketch: source, id: "p".into() }]
    );
    let before = h.app.session.doc.to_json();
    let undo = h.app.session.undo.len();
    h.step(&json!({"key":"P"})).unwrap();
    assert!(!h.app.status.as_ref().is_some_and(|(_, _, error)| *error), "{:?}", h.app.status);
    assert!(h.app.session.selection.is_empty());
    assert!(h.app.tool.is_none());
    assert_eq!(h.app.session.undo.len(), undo + 1);
    let state = h.app.session.world_state();
    let sk = &state.sketch(h.app.session.active_sketch.unwrap()).unwrap().sketch;
    assert_eq!(sk.curves.len(), 2);
    assert!(sk.curves.iter().any(|curve| curve.link.is_some()));
    assert!(sk.points.iter().any(|point| point.link.is_some() && (point.pos.x - 20.).abs() < 1e-6 && (point.pos.y - 20.).abs() < 1e-6));
    let local = sk.point(sk.resolve_point("p").unwrap()).unwrap();
    assert!((local.x - 45.).abs() < 1e-6 && (local.y - 50.).abs() < 1e-6);
    let after = h.app.session.doc.to_json();
    h.app.run("edit.undo", json!({})).unwrap();
    assert_eq!(h.app.session.doc.to_json(), before);
    h.app.run("edit.redo", json!({})).unwrap();
    assert_eq!(h.app.session.doc.to_json(), after);
}

#[test]
fn bad_reference_preserves_selection_and_document_atomically() {
    let (mut h, source) = setup();
    h.app.session.selection =
        vec![Sel::ExternalSketchCurve { sketch: source, id: "l1".into() }, Sel::ExternalSketchPoint { sketch: source, id: "missing".into() }];
    let selection = h.app.session.selection.clone();
    let before = h.app.session.doc.to_json();
    let undo = h.app.session.undo.len();
    h.step(&json!({"start":"sketch.project"})).unwrap();
    assert!(h.app.status.as_ref().is_some_and(|(_, _, error)| *error));
    assert_eq!(h.app.session.doc.to_json(), before);
    assert_eq!(h.app.session.undo.len(), undo);
    assert_eq!(h.app.session.selection, selection);
}

#[test]
fn external_point_drag_and_delete_cannot_edit_same_named_local_point() {
    let (mut h, source) = setup();
    let before = h.app.session.doc.to_json();
    let undo = h.app.session.undo.len();
    h.step(&json!({"drag":[{"world":[20,20,10]},{"world":[25,25,10]}]})).unwrap();
    assert_eq!(h.app.session.doc.to_json(), before);
    assert_eq!(h.app.session.undo.len(), undo);
    h.app.session.selection =
        vec![Sel::ExternalSketchPoint { sketch: source, id: "p".into() }, Sel::ExternalSketchCurve { sketch: source, id: "l1".into() }];
    let error = h.app.run("selection.delete", json!({"dry_run":true})).unwrap_err();
    assert!(error.contains("nothing to delete"), "{error}");
    assert_eq!(h.app.session.doc.to_json(), before);
}

#[test]
fn axis_and_plane_preselection_projects_as_one_command() {
    let (mut h, _) = setup();
    h.app.session.selection = vec![Sel::Axis { name: "X".into() }, Sel::Plane { name: "YZ".into() }];
    let undo = h.app.session.undo.len();
    h.step(&json!({"start":"sketch.project"})).unwrap();
    assert!(!h.app.status.as_ref().is_some_and(|(_, _, error)| *error), "{:?}", h.app.status);
    assert_eq!(h.app.session.undo.len(), undo + 1);
    let state = h.app.session.world_state();
    let sk = &state.sketch(h.app.session.active_sketch.unwrap()).unwrap().sketch;
    assert_eq!(sk.curves.iter().filter(|c| c.link.is_some()).count(), 2);
    assert!(h.app.session.selection.is_empty());
}

#[test]
fn mixed_local_and_external_selection_keeps_ordinary_project_picking() {
    let (mut h, source) = setup();
    h.app.session.selection = vec![Sel::SketchCurve { id: "l1".into(), sketch: None }, Sel::ExternalSketchCurve { sketch: source, id: "l1".into() }];
    let before = h.app.session.doc.to_json();
    let selection = h.app.session.selection.clone();
    let undo = h.app.session.undo.len();
    h.step(&json!({"key":"P"})).unwrap();
    assert_eq!(h.app.tool.as_ref().map(|tool| tool.cmd), Some("sketch.project"));
    assert_eq!(h.app.session.selection, selection);
    assert_eq!(h.app.session.doc.to_json(), before);
    assert_eq!(h.app.session.undo.len(), undo);
    h.step(&json!({"click":{"world":[20,10,10]}})).unwrap();
    assert_eq!(h.app.session.undo.len(), undo + 1);
    let state = h.app.session.world_state();
    assert!(state.sketch(h.app.session.active_sketch.unwrap()).unwrap().sketch.curves.iter().any(|curve| curve.link.is_some()));
}

#[test]
fn oversized_preselection_does_not_silently_project_a_subset() {
    let (mut h, _) = setup();
    h.app.session.selection = vec![Sel::Axis { name: "X".into() }; 1001];
    let before = h.app.session.doc.to_json();
    let undo = h.app.session.undo.len();
    h.step(&json!({"start":"sketch.project"})).unwrap();
    assert_eq!(h.app.tool.as_ref().map(|tool| tool.cmd), Some("sketch.project"));
    assert_eq!(h.app.session.doc.to_json(), before);
    assert_eq!(h.app.session.undo.len(), undo);
    assert_eq!(h.app.session.selection.len(), 1001);
}

/// #635: a rectangle whose corner is made coincident with a projected edge of the solid, so
/// that its bottom side lies on the edge, is still a closed profile: a click in its middle
/// selects it, bounded by the rectangle's other three sides and the projected edge under the
/// fourth.
#[test]
fn a_rectangle_put_onto_a_projected_edge_is_still_a_profile_to_click() {
    let mut h = Harness::new();
    h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    h.app.run("sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[40,30]})).unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
    h.app.run("solid.extrude", json!({"distance":20})).unwrap();
    let sketch = h.app.run("sketch.create", json!({"plane":"XY"})).unwrap()["sketch"].as_u64().unwrap();
    let projected = h.app.run("sketch.project", json!({"refs":[{"edge":[20,0,0]}]})).unwrap()["links"][0]["curves"][0].as_str().unwrap().to_string();
    let rectangle = h.app.run("sketch.rectangle.two_point", json!({"p0":[5,10],"p1":[25,25]})).unwrap();
    let mut sides: Vec<String> = rectangle["curves"].as_array().unwrap().iter().map(|c| c.as_str().unwrap().to_string()).collect();
    h.app.run("sketch.constraint.coincident", json!({"a":format!("{}.start", sides[0]),"b":projected})).unwrap();
    // The bottom side lies under the projected edge.
    sides[0] = projected.clone();
    // From below, the sketch plane is in front of the solid standing on it.
    h.call("ui.view", json!({"view":"bottom"}));
    h.step(&json!({"click":{"sketch":[15,12]}})).unwrap();
    assert_eq!(h.app.session.selection, vec![Sel::Profile { sketch, index: 0 }]);
    let state = h.app.session.world_state();
    let ss = state.sketch(sketch).unwrap();
    assert_eq!(ss.profiles.len(), 1);
    let mut bounded_by = ss.profiles[0].outer_curves.clone();
    bounded_by.sort();
    sides.sort();
    assert_eq!(bounded_by, sides);
    assert!(ss.sketch.curves.iter().any(|curve| curve.id == projected && curve.link.is_some()), "the projected edge stays");
}

/// A click in the dark centre of a hole hits no face. Project still draws the rim, and a click
/// on empty space says so instead of returning as if the tool were not running.
#[test]
fn clicking_a_hole_opening_projects_its_rim_and_a_miss_is_an_error() {
    let mut h = Harness::new();
    // These cover the direct pick workflow; the options dialog is covered in dialogs_sketch_tests.
    h.app.preferences.project_options_dialog = false;
    h.app.run("solid.box", json!({"length": 40, "width": 30, "height": 20})).unwrap();
    h.app.run("solid.hole", json!({"position": [20, 15, 20], "diameter": 10, "depth": 20})).unwrap();
    // Above the part, so creating the sketch does not already project the face (and its hole).
    h.app.run("sketch.create", json!({"plane": "XY", "offset": 30})).unwrap();
    h.call("ui.view", json!({"view": "top"}));
    h.step(&json!({"start": "sketch.project"})).unwrap();
    assert_eq!(h.app.tool.as_ref().map(|tool| tool.cmd), Some("sketch.project"));
    // The origin is shown. Its plane can sit on this ray; the opening still wins.
    h.step(&json!({"click": {"world": [20.0, 15.0, 20.0]}})).unwrap();
    assert!(!h.app.status.as_ref().is_some_and(|(_, _, error)| *error), "{:?}", h.app.status);
    let si = h.app.run("sketch.inspect", json!({})).unwrap();
    let arcs: Vec<_> =
        si["curves"].as_array().unwrap().iter().filter(|c| c.get("link").is_some() && (c["type"] == "arc" || c["type"] == "circle")).collect();
    assert!(arcs.len() >= 2, "{si}");
    for c in &arcs {
        let r = c["radius"].as_f64().unwrap();
        assert!((r - 5.0).abs() < 1e-3, "{c}");
        assert!((c["center_at"][0].as_f64().unwrap() - 20.0).abs() < 1e-3, "{c}");
        assert!((c["center_at"][1].as_f64().unwrap() - 15.0).abs() < 1e-3, "{c}");
    }
    let n = si["curves"].as_array().unwrap().len();
    while h.app.cam_anim.is_some() {
        h.frames(4);
    }
    h.app.set_origin_shown(false);
    // Left and below the plate, inside the fitted top view. A vertical ray here misses the
    // solid and the hole. The view's top edge is covered by chrome, so this stays clear of it.
    let miss = [-50.0, -30.0, 20.0];
    h.step(&json!({"click": {"world": miss}})).unwrap();
    let after = h.app.run("sketch.inspect", json!({})).unwrap();
    let status = h.app.status.clone().expect("a click on empty space is an error");
    assert!(status.2 && status.0.contains("nothing"), "{status:?} curves {n} -> {}", after["curves"].as_array().map(|a| a.len()).unwrap_or(0));
    assert_eq!(after["curves"].as_array().unwrap().len(), n);
    assert_eq!(h.app.tool.as_ref().map(|tool| tool.cmd), Some("sketch.project"));
}
