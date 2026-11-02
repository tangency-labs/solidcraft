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

/// #10291: a line of another sketch, picked while the Project dialog is open, projects.
/// The pick names the sketch it came from, so the dialog's geometry input holds it and the
/// command gets a reference to it, even when the two sketches lie in parallel planes.
#[test]
fn a_line_of_another_sketch_picked_in_the_project_dialog_projects() {
    let mut h = Harness::new();
    // The options dialog is the workflow of the bug report: Project opens a dialog that
    // takes the geometry to project.
    h.app.preferences.project_options_dialog = true;
    // Sketch 1 on plane 1.
    let source = h.app.run("sketch.create", json!({"plane":"XY"})).unwrap()["sketch"].as_u64().unwrap();
    h.app.run("sketch.line", json!({"points":[[10,10],[30,10]],"infer":false})).unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
    // Plane 2 parallel to plane 1, and sketch 2 on it.
    h.app.run("construct.plane.offset", json!({"base":"XY","offset":10,"name":"Plane2"})).unwrap();
    h.app.run("sketch.create", json!({"plane":"Plane2"})).unwrap();
    h.call("ui.set", json!({"shownSketches":[source]}));
    h.call("ui.view", json!({"view":"top"}));
    // Project, then pick the line of sketch 1 in the viewport.
    h.app.start("sketch.project");
    h.frames(4);
    assert!(h.app.dialog.is_some(), "Project opens its options dialog");
    h.step(&json!({"click":{"world":[20,10,0]}})).unwrap();
    let d = h.app.dialog.as_ref().expect("the dialog is still open");
    assert_eq!(d.inputs[0].items.len(), 1, "the line of the other sketch is in the input: {:?}", d.inputs[0].items);
    h.step(&json!({"click":{"handle":"dialog:ok"}})).unwrap();
    h.settle();
    assert!(h.app.dialog.is_none(), "the dialog closes on a valid pick: {:?}", h.app.dialog.as_ref().and_then(|d| d.error.clone()));
    assert!(!h.app.status.as_ref().is_some_and(|(_, _, error)| *error), "{:?}", h.app.status);
    let sketch = h.app.session.active_sketch.expect("Project leaves the sketch active");
    let state = h.app.session.world_state();
    let sk = &state.sketch(sketch).unwrap().sketch;
    assert!(sk.curves.iter().any(|curve| curve.link.is_some()), "the line is projected into the sketch");
    // The projection names the sketch it came from, so it follows it.
    assert!(
        sk.links.iter().any(|l| matches!(&l.source, solidcraft_engine::sketch::LinkSource::SketchCurve { sketch, .. } if *sketch == source)),
        "{:?}",
        sk.links
    );
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
