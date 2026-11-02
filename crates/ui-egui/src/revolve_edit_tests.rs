use crate::dialogs::{Dialog, Kind, apply_commands};
use crate::scenario::Harness;
use serde_json::{Value, json};
use solidcraft_engine::Sel;
use solidcraft_engine::doc::{AxisRef, FeatureKind};
use solidcraft_engine::geom::Vec3;

fn make(h: Harness, axis: Value) -> Harness {
    make_params(h, axis, json!({}))
}
fn make_params(mut h: Harness, axis: Value, extra: Value) -> Harness {
    h.app.run("sketch.create", json!({"plane":"XY","name":"Profile"})).unwrap();
    h.app.run("sketch.rectangle.two_point", json!({"p0":[10,0],"p1":[15,5]})).unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
    let mut params = json!({"sketch":"Profile","axis":axis,"angle":"90 deg","operation":"new","name":"Turn","body_name":"Main"});
    params.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
    h.app.run("solid.revolve", params).unwrap();
    h.frames(4);
    h
}

#[test]
fn unchanged_all_profiles_retains_future_geometry_but_reselection_is_explicit() {
    for reselect in [false, true] {
        let mut h = make(Harness::new(), json!("Y"));
        open(&mut h);
        if reselect {
            let selection = h.app.dialog.as_ref().unwrap().inputs[0].items[0].clone();
            let mut dialog = h.app.dialog.take().unwrap();
            dialog.active = 0;
            dialog.pick(&h.app.session, selection.clone());
            dialog.pick(&h.app.session, selection);
            h.app.dialog = Some(dialog);
            h.frames(4);
        }
        h.step(&json!({"widget":"OK"})).unwrap();
        let FeatureKind::Revolve { profiles, .. } = &h.app.session.doc.find_feature("Turn").unwrap().kind else { panic!("revolve") };
        assert_eq!(matches!(profiles, solidcraft_engine::doc::ProfileSel::All), !reselect);
        h.app.run("sketch.edit", json!({"sketch":"Profile"})).unwrap();
        h.app.run("sketch.rectangle.two_point", json!({"p0":[20,0],"p1":[25,5]})).unwrap();
        h.app.run("sketch.finish", json!({})).unwrap();
        let state = h.app.session.world_state();
        let volume: f64 = state.bodies.iter().map(|b| solidcraft_engine::doc::kernel::measure(&b.body).unwrap().volume).sum();
        let expected = if reselect { 490.873852 } else { 1374.446786 };
        assert!((volume - expected).abs() < 0.3, "{volume} != {expected}");
    }
}

#[test]
fn to_object_noop_preserves_cut_targets_and_untargeted_body() {
    let mut h = Harness::new();
    for name in ["A", "B"] {
        h.app.run("solid.box", json!({"length":20,"width":5,"height":20,"corner":[0,0,0],"body_name":name})).unwrap();
    }
    let mut h = make_params(h, json!("Y"), json!({"to":[0,2,12],"angle":"45 deg","operation":"cut","targets":["A"]}));
    let state = h.app.session.world_state();
    // A quarter annulus separates the stock into inner and outer solids. Both are
    // results of cutting A; checking only the first named solid loses material.
    let cut_volume = |state: &solidcraft_engine::doc::ModelState| -> f64 {
        state.bodies.iter().filter(|b| b.name != "B").map(|b| solidcraft_engine::doc::kernel::measure(&b.body).unwrap().volume).sum()
    };
    let before_a = cut_volume(&state);
    let removed = 0.5 * (15.0f64.powi(2) - 10.0f64.powi(2)) * 5.0 * std::f64::consts::FRAC_PI_2;
    assert!((before_a - (2000.0 - removed)).abs() < 0.2, "cut results {:?}: {before_a}", state.bodies.iter().map(|b| &b.name).collect::<Vec<_>>());
    let in_cut_stock = |p| state.bodies.iter().filter(|b| b.name != "B").any(|b| b.body.contains(p));
    assert!(in_cut_stock(solidcraft_engine::geom::Vec3::new(2.0, 2.0, 2.0)));
    assert!(in_cut_stock(solidcraft_engine::geom::Vec3::new(18.0, 2.0, 18.0)));
    assert!(!in_cut_stock(solidcraft_engine::geom::Vec3::new(9.0, 2.0, 9.0)));
    assert!(state.body("B").unwrap().body.contains(solidcraft_engine::geom::Vec3::new(9.0, 2.0, 9.0)));
    let before_bounds: Vec<_> = state.bodies.iter().map(|b| (b.name.clone(), b.mesh().bounds())).collect();
    let undo = h.app.session.undo.len();
    open(&mut h);
    h.step(&json!({"widget":"OK"})).unwrap();
    assert!(h.app.dialog.is_none());
    let state = h.app.session.world_state();
    assert!((solidcraft_engine::doc::kernel::measure(&state.body("B").unwrap().body).unwrap().volume - 2000.0).abs() < 1e-7);
    assert!((cut_volume(&state) - before_a).abs() < 1e-7);
    assert_eq!(state.bodies.iter().map(|b| (b.name.clone(), b.mesh().bounds())).collect::<Vec<_>>(), before_bounds);
    assert_eq!(h.app.session.undo.len(), undo + 1);
    let FeatureKind::Revolve { targets, .. } = &h.app.session.doc.find_feature("Turn").unwrap().kind else { panic!("revolve") };
    assert_eq!(targets, &["A"]);
}

#[test]
fn to_object_noop_retains_dormant_secondary_angle_and_downstream_dimension() {
    let mut h = make_params(Harness::new(), json!("Y"), json!({"to":[0,2,12],"angle":"45 deg","angle2":"30 deg","direction":"symmetric"}));
    h.app.run("solid.box", json!({"length":2,"width":2,"height":"(d2 / (1 rad)) * 1 mm","corner":[30,0,0],"body_name":"Dependent"})).unwrap();
    let original = serde_json::to_value(&h.app.session.doc.find_feature("Turn").unwrap().kind).unwrap();
    let names = h.app.session.doc.find_feature("Turn").unwrap().param_names.clone();
    open(&mut h);
    h.step(&json!({"widget":"OK"})).unwrap();
    assert_eq!(serde_json::to_value(&h.app.session.doc.find_feature("Turn").unwrap().kind).unwrap(), original);
    assert_eq!(h.app.session.doc.find_feature("Turn").unwrap().param_names, names);
    assert!((h.app.session.world_state().body("Dependent").unwrap().mesh().bounds().max.z - std::f64::consts::PI / 6.0).abs() < 1e-8);
}

#[test]
fn rotated_owner_to_point_remains_exact_on_noop_edit() {
    let mut h = Harness::new();
    let owner = h.app.run("component.create", json!({"name":"Owner","grounded":false})).unwrap();
    let mut h = make_params(h, json!({"origin":[0,0,0],"dir":[0,2,0]}), json!({"to":[0,2,12],"angle":"45 deg"}));
    h.app.run("occurrence.move", json!({"occurrence":owner["occurrence"],"translate":[100,0,20],"axis":[0,0,1],"angle":"17 deg"})).unwrap();
    h.app.run("component.create", json!({"name":"Other","parent":0})).unwrap();
    let original = serde_json::to_value(&h.app.session.doc.find_feature("Turn").unwrap().kind).unwrap();
    let bounds = h.app.session.world_state().body("Main").unwrap().mesh().bounds();
    open(&mut h);
    h.step(&json!({"widget":"OK"})).unwrap();
    assert_eq!(serde_json::to_value(&h.app.session.doc.find_feature("Turn").unwrap().kind).unwrap(), original);
    assert_eq!(h.app.session.world_state().body("Main").unwrap().mesh().bounds(), bounds);
}

#[test]
fn second_active_occurrence_noop_preserves_definition_axis_target_and_instances() {
    let mut h = Harness::new();
    let owner = h.app.run("component.create", json!({"name":"Owner"})).unwrap();
    let mut h = make_params(h, json!({"origin":[0,0,0],"dir":[0,2,0]}), json!({"to":[0,2,12],"angle":"45 deg"}));
    let second = h.app.run("occurrence.copy", json!({"component":owner["component"],"translate":[100,0,20]})).unwrap()["occurrence"].clone();
    h.app.run("occurrence.move", json!({"occurrence":second,"axis":[0,0,1],"angle":"17 deg"})).unwrap();
    h.app.run("component.activate", json!({"component":owner["component"],"occurrence":second,"path":[second]})).unwrap();
    let original = serde_json::to_value(&h.app.session.doc.find_feature("Turn").unwrap().kind).unwrap();
    let frame = h.app.session.active_component_frame();
    let expected_origin = solidcraft_engine::doc::apply_point(&frame, solidcraft_engine::geom::Vec3::ZERO);
    let before: Vec<_> = h.app.session.world_state().bodies.iter().map(|b| (b.name.clone(), b.mesh().bounds())).collect();
    open(&mut h);
    let (origin, _) = crate::canvas::revolve_axis(&h.app, h.app.dialog.as_ref().unwrap()).unwrap();
    assert!((origin - expected_origin).len() < 1e-8);
    assert!(origin.len() > 50.0, "fixture must distinguish first and second placements");
    let commands = crate::dialogs::apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap();
    let preview = h.app.session.preview_world(&commands).unwrap();
    let placed = preview.after.bodies.iter().find(|body| body.mesh().bounds().center().x > 50.0).unwrap();
    assert!(placed.mesh().triangles.len() > 10);
    assert_eq!(placed.mesh().bounds(), before.iter().find(|(_, bounds)| bounds.center().x > 50.0).unwrap().1);
    assert_eq!(crate::scenario::count_of("dialog:selection:Axis:filled"), Some(1.0), "the stored custom axis stays selected");
    // In the floating shell the read-only Sweep badge overlaps OK at this placement.
    // It must not intercept the dialog's click or leave the timeline rolled back.
    h.step(&json!({"widget":"OK"})).unwrap();
    assert!(h.app.dialog.is_none(), "OK must apply even underneath the Sweep badge");
    assert_eq!(serde_json::to_value(&h.app.session.doc.find_feature("Turn").unwrap().kind).unwrap(), original);
    let after: Vec<_> = h.app.session.world_state().bodies.iter().map(|b| (b.name.clone(), b.mesh().bounds())).collect();
    assert_eq!(after, before);
    assert_eq!(h.app.session.active_occurrence_path.as_ref(), Some(&vec![second.as_u64().unwrap()]));
}
fn axis(h: &Harness) -> Value {
    let FeatureKind::Revolve { axis, .. } = &h.app.session.doc.find_feature("Turn").unwrap().kind else { panic!("revolve") };
    serde_json::to_value(axis).unwrap()
}
fn open(h: &mut Harness) {
    h.step(&json!({"call":"ui.editFeature","params":{"feature":"Turn"}})).unwrap();
    h.frames(8);
}

#[test]
fn actual_noop_editor_retains_all_revolve_axis_kinds_and_analytic_volume() {
    for axis_value in [json!("Y"), json!("y"), json!("l4"), json!({"origin":[0,0,0],"dir":[0,2,0]})] {
        let mut h = make(Harness::new(), axis_value.clone());
        let original = axis(&h);
        let names = h.app.session.doc.find_feature("Turn").unwrap().param_names.clone();
        let state = h.app.session.world_state();
        let body = state.body("Main").unwrap();
        let bounds = body.mesh().bounds();
        let radii = if axis_value == json!("l4") { 25.0 } else { 225.0 - 100.0 };
        let expected = 0.5 * radii * 5.0 * std::f64::consts::FRAC_PI_2;
        let volume = solidcraft_engine::doc::kernel::measure(&body.body).unwrap().volume;
        assert!((volume - expected).abs() < 0.03, "{axis_value}: {volume}");
        open(&mut h);
        let commands = crate::dialogs::apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap();
        let preview = h.app.session.preview(&commands).unwrap();
        assert_eq!(preview.after.body("Main").unwrap().mesh().bounds(), bounds);
        h.step(&json!({"widget":"OK"})).unwrap();
        assert!(h.app.dialog.is_none(), "{axis_value}: {:?}", h.app.session.log);
        assert_eq!(axis(&h), original);
        assert_eq!(h.app.session.doc.find_feature("Turn").unwrap().param_names, names);
        assert_eq!(h.app.session.world_state().body("Main").unwrap().mesh().bounds(), bounds);
    }
}

#[test]
fn revolve_angle_edit_preserves_axis_downstream_binding_and_one_undo() {
    let mut h = make(Harness::new(), json!({"origin":[0,0,0],"dir":[0,2,0]}));
    h.app.run("solid.box", json!({"length":2,"width":2,"height":"(d1 / (1 rad)) * 1 mm","corner":[30,0,0],"body_name":"Dependent"})).unwrap();
    let original = axis(&h);
    let undo = h.app.session.undo.len();
    open(&mut h);
    h.step(&json!({"key":"A","cmd":true})).unwrap();
    h.step(&json!({"text":"180 deg"})).unwrap();
    h.step(&json!({"widget":"OK"})).unwrap();
    assert_eq!(axis(&h), original);
    assert_eq!(h.app.session.undo.len(), undo + 1);
    assert!((h.app.session.world_state().body("Dependent").unwrap().mesh().bounds().max.z - std::f64::consts::PI).abs() < 1e-8);
    h.app.run("edit.undo", json!({})).unwrap();
    assert_eq!(axis(&h), original);
    assert!((h.app.session.world_state().body("Dependent").unwrap().mesh().bounds().max.z - std::f64::consts::FRAC_PI_2).abs() < 1e-8);
}

#[test]
fn rotated_owner_explicit_axis_is_lossless_with_another_component_active() {
    let mut h = Harness::new();
    let owner = h.app.run("component.create", json!({"name":"Owner","grounded":false})).unwrap();
    let mut h = make(h, json!({"origin":[0,0,0],"dir":[0,2,0]}));
    h.app.run("occurrence.move", json!({"occurrence":owner["occurrence"],"translate":[100,0,20],"axis":[0,0,1],"angle":"17 deg"})).unwrap();
    h.app.run("component.create", json!({"name":"Other","parent":0})).unwrap();
    let active = h.app.session.active_component;
    let original = axis(&h);
    let bounds = h.app.session.world_state().body("Main").unwrap().mesh().bounds();
    open(&mut h);
    let (origin, direction) = crate::canvas::revolve_axis(&h.app, h.app.dialog.as_ref().unwrap()).unwrap();
    assert!((origin.x - 100.0).abs() < 1e-8 && (origin.z - 20.0).abs() < 1e-8);
    let angle = 17.0f64.to_radians();
    assert!((direction.x + angle.sin()).abs() < 1e-8 && (direction.y - angle.cos()).abs() < 1e-8);
    h.step(&json!({"widget":"OK"})).unwrap();
    assert_eq!(axis(&h), original);
    assert_eq!(h.app.session.active_component, active);
    assert_eq!(h.app.session.world_state().body("Main").unwrap().mesh().bounds(), bounds);
}

#[test]
fn world_and_sketch_axis_handles_follow_authoring_occurrence() {
    for selected in [json!("Y"), json!("y")] {
        let mut h = Harness::new();
        let owner = h.app.run("component.create", json!({"name":"Owner","grounded":false})).unwrap();
        let mut h = make(h, selected);
        h.app.run("occurrence.move", json!({"occurrence":owner["occurrence"],"translate":[100,0,20],"axis":[0,0,1],"angle":"17 deg"})).unwrap();
        let original = axis(&h);
        open(&mut h);
        let (origin, direction) = crate::canvas::revolve_axis(&h.app, h.app.dialog.as_ref().unwrap()).unwrap();
        let angle = 17.0f64.to_radians();
        assert!((origin.x - 100.0).abs() < 1e-8 && (origin.z - 20.0).abs() < 1e-8);
        assert!((direction.x + angle.sin()).abs() < 1e-8 && (direction.y - angle.cos()).abs() < 1e-8);
        h.step(&json!({"widget":"OK"})).unwrap();
        assert_eq!(axis(&h), original);
    }
}

#[test]
fn sketch_axis_cancel_restores_original_document_and_history() {
    let mut h = make(Harness::new(), json!("y"));
    let original = serde_json::to_value(&*h.app.session.doc).unwrap();
    let undo = h.app.session.undo.len();
    open(&mut h);
    h.step(&json!({"key":"A","cmd":true})).unwrap();
    h.step(&json!({"text":"180 deg"})).unwrap();
    h.step(&json!({"widget":"Cancel"})).unwrap();
    assert_eq!(serde_json::to_value(&*h.app.session.doc).unwrap(), original);
    assert_eq!(h.app.session.undo.len(), undo);
    assert!(matches!(&h.app.session.doc.find_feature("Turn").unwrap().kind,FeatureKind::Revolve{axis:AxisRef::SketchAxis{axis},..} if axis=="y"));
}

#[test]
fn only_the_active_selection_field_looks_active() {
    let mut h = make(Harness::new(), json!("Y"));
    open(&mut h);
    let t = crate::theme::Tokens::get();
    let border = |h: &Harness, label: &str| {
        let field = crate::scenario::rect_of(&format!("dialog:field:{label}")).unwrap();
        h.shapes
            .iter()
            .rev()
            .find_map(|s| match &s.shape {
                egui::Shape::Rect(r) if r.rect == field || r.rect == field.shrink(t.space_2) => Some(r.stroke.color),
                _ => None,
            })
            .unwrap()
    };
    let click = |h: &mut Harness, handle: &str| {
        let at = crate::scenario::rect_of(handle).map(|r| r.center()).or_else(|| crate::scenario::handle_at(handle)).unwrap();
        h.step(&json!({"click":[at.x, at.y]})).unwrap();
        h.frames(2);
    };
    for label in ["Profile", "Axis"] {
        assert_eq!(crate::scenario::count_of(&format!("dialog:selection:{label}:filled")), Some(1.0), "{label} holds its item");
    }
    assert_eq!(h.app.dialog.as_ref().unwrap().active, 0);
    assert_eq!((border(&h, "Profile") == t.form_primary, border(&h, "Axis") == t.form_primary), (true, false));
    click(&mut h, "dialog:field:Axis");
    assert_eq!(h.app.dialog.as_ref().unwrap().active, 1);
    assert_eq!((border(&h, "Profile") == t.form_primary, border(&h, "Axis") == t.form_primary), (false, true));
    let filled = border(&h, "Profile");
    click(&mut h, "dialog:Angle");
    assert_eq!(h.app.dialog.as_ref().unwrap().active, 1, "picks still go to Axis while Angle is typed into");
    assert_ne!(border(&h, "Axis"), t.form_primary, "only the field with the keyboard looks active (#10066)");
    click(&mut h, "dialog:clear:Axis");
    click(&mut h, "dialog:field:Profile");
    assert_eq!(crate::scenario::count_of("dialog:selection:Axis:filled"), Some(0.0));
    assert_eq!((border(&h, "Profile"), border(&h, "Axis")), (t.form_primary, t.control_border));
    click(&mut h, "dialog:field:Axis");
    assert_eq!(border(&h, "Profile"), filled, "filled inactive");
    assert_ne!(filled, t.control_border, "filled inactive differs from empty inactive");
}

fn xy_rectangle(h: &mut Harness, name: &str, p0: [f64; 2], p1: [f64; 2]) -> u64 {
    h.app.run("sketch.create", json!({"plane": "XY", "name": name})).unwrap();
    h.app.run("sketch.rectangle.two_point", json!({"p0": p0, "p1": p1})).unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
    h.app.session.doc.find_feature(name).unwrap().id
}

fn revolve_dialog(app: &crate::SolveApp, profiles: Vec<Sel>, axis: Sel) -> Dialog {
    let mut d = Dialog::for_command(app, "solid.revolve").unwrap();
    d.inputs[0].items = profiles;
    d.inputs[1].items = vec![axis];
    if let Kind::Revolve { angle, operation, .. } = &mut d.kind {
        *angle = "120 deg".into();
        *operation = 0;
    }
    d
}

fn vertical_id(app: &crate::SolveApp, sketch: u64, x: f64) -> String {
    let st = app.session.world_state();
    let ss = st.sketch(sketch).unwrap();
    ss.sketch
        .curves
        .iter()
        .find_map(|c| {
            let solidcraft_engine::sketch::CurveKind::Line { a, b } = c.kind else { return None };
            let (pa, pb) = (ss.sketch.point(a)?, ss.sketch.point(b)?);
            ((pa.x - x).abs() < 1e-6 && (pb.x - x).abs() < 1e-6).then(|| c.id.clone())
        })
        .unwrap_or_else(|| panic!("no vertical line at x={x}"))
}

fn segment(app: &crate::SolveApp, sketch: u64, id: &str) -> (Vec3, Vec3) {
    let st = app.session.world_state();
    let ss = st.sketch(sketch).unwrap();
    let i = ss.sketch.curve_index(id).unwrap_or_else(|| panic!("no curve {id}"));
    let c = &ss.sketch.curves[i];
    let solidcraft_engine::sketch::CurveKind::Line { a, b } = c.kind else { panic!("{id} is not a line") };
    (ss.plane.to_world(ss.sketch.point(a).unwrap()), ss.plane.to_world(ss.sketch.point(b).unwrap()))
}

/// `(point_x, 5, 0)` rotated 80° about `x = axis_x` toward +Z.
fn lift_xy(axis_x: f64, point_x: f64) -> Vec3 {
    let dx = point_x - axis_x;
    let t = 80.0_f64.to_radians();
    Vec3::new(axis_x + dx * t.cos(), 5.0, dx.abs() * t.sin())
}

fn assert_pivots_on(preview: &solidcraft_engine::Preview, on_axis: Vec3, on_other: Vec3, what: &str) {
    let before: std::collections::BTreeSet<_> = preview.before.bodies.iter().map(|b| b.name.as_str()).collect();
    let body = preview
        .after
        .bodies
        .iter()
        .filter(|b| !before.contains(b.name.as_str()))
        .find(|b| b.body.contains(on_axis))
        .unwrap_or_else(|| panic!("{what}: no new body contains {on_axis:?}"));
    assert!(!body.body.contains(on_other), "{what}: also contains the other edge's probe {on_other:?}");
}

/// A dialog pick of the profile's own edge keeps that curve id, and the preview pivots on it
/// rather than on the opposite side. Naming the sketch and leaving it unnamed while that sketch
/// is active are the two ways a click stores the pick.
#[test]
fn dialog_axis_keeps_the_profile_line_when_it_is_the_highlighted_edge() {
    let mut h = Harness::new();
    let sketch = xy_rectangle(&mut h, "Profile", [10.0, 0.0], [30.0, 10.0]);
    let right = vertical_id(&h.app, sketch, 30.0);
    let left = vertical_id(&h.app, sketch, 10.0);
    assert_ne!(right, left);
    for named in [Some(sketch), None] {
        if named.is_none() {
            h.app.run("sketch.edit", json!({"sketch": "Profile"})).unwrap();
        }
        let d = revolve_dialog(&h.app, vec![Sel::Profile { sketch, index: 0 }], Sel::SketchCurve { id: right.clone(), sketch: named });
        let cmds = apply_commands(&h.app, &d).unwrap();
        assert_eq!(cmds[0].0, "solid.revolve");
        assert_eq!(cmds[0].1["axis"], json!(right), "named={named:?}");
        let preview = h.app.session.preview(&cmds).unwrap();
        assert_pivots_on(&preview, lift_xy(30.0, 10.5), lift_xy(10.0, 29.5), &format!("named={named:?}"));
        if named.is_none() {
            h.app.run("sketch.finish", json!({})).unwrap();
        }
    }
}

/// Curve ids repeat across sketches (#3726). The highlighted line is this sketch's `l2` (x = 10);
/// the profile sketch's line of the same id is the opposite edge (x = 30). The preview must pivot
/// on the highlighted line, and the command must not pass the bare id.
#[test]
fn dialog_revolve_follows_the_highlighted_edge_when_the_curve_id_collides() {
    let mut h = Harness::new();
    let profile = xy_rectangle(&mut h, "Profile", [10.0, 0.0], [30.0, 10.0]);
    let right = vertical_id(&h.app, profile, 30.0);
    h.app.run("sketch.create", json!({"plane": "XY", "name": "Axis"})).unwrap();
    h.app.run("sketch.line", json!({"points": [[10, 0], [10, 10]], "ids": [right]})).unwrap();
    let axis_sketch = h.app.session.active_sketch.unwrap();
    let (highlighted, _) = segment(&h.app, axis_sketch, &right);
    let (profile_line, _) = segment(&h.app, profile, &right);
    assert!((highlighted.x - 10.0).abs() < 1e-6, "{highlighted:?}");
    assert!((profile_line.x - 30.0).abs() < 1e-6, "{profile_line:?}");
    for named in [Some(axis_sketch), None] {
        let d = revolve_dialog(&h.app, vec![Sel::Profile { sketch: profile, index: 0 }], Sel::SketchCurve { id: right.clone(), sketch: named });
        let cmds = apply_commands(&h.app, &d).unwrap();
        let axis = &cmds[0].1["axis"];
        let origin_x = axis["origin"][0].as_f64().unwrap_or_else(|| panic!("named={named:?} stored {axis}, the colliding id"));
        assert!((origin_x - 10.0).abs() < 1e-6, "named={named:?}: {axis}");
        let preview = h.app.session.preview(&cmds).unwrap();
        // About the highlighted edge x = 10, from a point near the far edge. The other probe is
        // the same construction about the profile sketch's line of this id.
        assert_pivots_on(&preview, lift_xy(10.0, 29.5), lift_xy(30.0, 10.5), &format!("named={named:?}"));
    }
    let (kind, mut inputs) = crate::dialogs_surface::start(&h.app, "surface.revolve").unwrap();
    let Kind::Surface(mut surf) = kind else { panic!("surface revolve") };
    if let crate::dialogs_surface::Surf::Revolve { angle, .. } = &mut surf {
        *angle = "120 deg".into();
    }
    inputs[0].items = vec![Sel::Profile { sketch: profile, index: 0 }];
    inputs[1].items = vec![Sel::SketchCurve { id: right, sketch: Some(axis_sketch) }];
    let cmds = crate::dialogs_surface::commands(&h.app, &surf, &inputs, &serde_json::Map::new()).unwrap();
    assert_eq!(cmds[0].0, "surface.revolve");
    assert!((cmds[0].1["axis"]["origin"][0].as_f64().unwrap() - 10.0).abs() < 1e-6, "{}", cmds[0].1["axis"]);
}

fn linked_span(app: &crate::SolveApp, sketch: u64) -> (f64, f64, f64, f64) {
    let st = app.session.world_state();
    let ss = st.sketch(sketch).unwrap();
    let mut xs = Vec::new();
    let mut ys = Vec::new();
    for c in &ss.sketch.curves {
        if c.link.is_none() {
            continue;
        }
        let solidcraft_engine::sketch::CurveKind::Line { a, b } = c.kind else { continue };
        for p in [ss.sketch.point(a), ss.sketch.point(b)].into_iter().flatten() {
            xs.push(p.x);
            ys.push(p.y);
        }
    }
    assert!(!xs.is_empty(), "the face sketch should project the face outline");
    (
        xs.iter().copied().fold(f64::MAX, f64::min),
        xs.iter().copied().fold(f64::MIN, f64::max),
        ys.iter().copied().fold(f64::MAX, f64::min),
        ys.iter().copied().fold(f64::MIN, f64::max),
    )
}

/// A rectangle drawn across a face edge splits the sketch into two profiles. The picked drawn
/// edge — and a second sketch's line that reuses that edge's id but lies on the other side —
/// must be the axis the preview revolves about (#7452).
#[test]
fn face_sketch_two_profiles_revolve_about_the_highlighted_edge() {
    let mut h = Harness::new();
    h.app.run("solid.box", json!({"length": 40, "width": 30, "height": 20, "corner": [0, 0, 0]})).unwrap();
    h.app.run("sketch.create", json!({"plane": {"face": [20, 0, 10]}, "name": "OnFace"})).unwrap();
    let sketch = h.app.session.active_sketch.unwrap();
    let (x0, _, y0, y1) = linked_span(&h.app, sketch);
    let mid_y = (y0 + y1) * 0.5;
    // Twelve millimetres outside the face, eight inside, so a point near either vertical side
    // stays inside a revolve about that side and outside a revolve about the other.
    let left_x = x0 - 12.0;
    let right_x = x0 + 8.0;
    h.app.run("sketch.rectangle.two_point", json!({"p0": [left_x, mid_y - 4.0], "p1": [right_x, mid_y + 4.0]})).unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
    let st = h.app.session.world_state();
    let ss = st.sketch(sketch).unwrap();
    assert!(ss.profiles.len() >= 2, "straddling the face edge should make two profiles, got {}", ss.profiles.len());
    let outside = ss
        .profiles
        .iter()
        .position(|p| p.centroid.x < x0 - 0.5)
        .unwrap_or_else(|| panic!("no profile outside the face: {:?}", ss.profiles.iter().map(|p| p.centroid).collect::<Vec<_>>()));
    let normal = ss.plane.normal();
    let drawn_at = |x: f64| {
        ss.sketch.curves.iter().find_map(|c| {
            if c.link.is_some() {
                return None;
            }
            let solidcraft_engine::sketch::CurveKind::Line { a, b } = c.kind else { return None };
            let (pa, pb) = (ss.sketch.point(a)?, ss.sketch.point(b)?);
            ((pa.x - x).abs() < 1e-6 && (pb.x - x).abs() < 1e-6).then(|| c.id.clone())
        })
    };
    let right_id = drawn_at(right_x).unwrap_or_else(|| panic!("no drawn edge at {right_x}"));
    let left_id = drawn_at(left_x).unwrap_or_else(|| panic!("no drawn edge at {left_x}"));
    let (right_a, right_b) = segment(&h.app, sketch, &right_id);
    let (left_a, left_b) = segment(&h.app, sketch, &left_id);
    let right_mid = (right_a + right_b) * 0.5;
    let left_mid = (left_a + left_b) * 0.5;
    let span = right_mid.dist(left_mid);
    assert!((span - (right_x - left_x)).abs() < 1e-4, "sketch millimetres should match the face plane, span {span}");
    let across = (right_mid - left_mid).normalized().unwrap();
    // 0.5 mm in from the left edge, and 0.5 mm short of the face edge (the far side of the
    // outside region when the axis is the left edge).
    let near_left = left_mid + across * 0.5;
    let near_face = left_mid + across * 11.5;
    let right_dir = (right_b - right_a).normalized().unwrap();
    let left_dir = (left_b - left_a).normalized().unwrap();
    let about_right = spin(normal, right_mid, right_dir, near_left, 80.0);
    let about_left = spin(normal, left_mid, left_dir, near_face, 80.0);

    let d = revolve_dialog(&h.app, vec![Sel::Profile { sketch, index: outside }], Sel::SketchCurve { id: right_id.clone(), sketch: Some(sketch) });
    let cmds = apply_commands(&h.app, &d).unwrap();
    assert_eq!(cmds[0].1["axis"], json!(right_id), "the drawn edge is this profile sketch's own line");
    let preview = h.app.session.preview(&cmds).unwrap();
    assert_pivots_on(&preview, about_right, about_left, "same-sketch face edge");

    // Another sketch on the same face reuses the drawn edge's id for the opposite side.
    h.app.run("sketch.create", json!({"plane": {"face": [20, 0, 10]}, "name": "Other", "project_edges": false})).unwrap();
    h.app.run("sketch.line", json!({"points": [[left_x, mid_y - 4.0], [left_x, mid_y + 4.0]], "ids": [right_id]})).unwrap();
    let other = h.app.session.active_sketch.unwrap();
    let (other_a, other_b) = segment(&h.app, other, &cmds[0].1["axis"].as_str().unwrap());
    let other_mid = (other_a + other_b) * 0.5;
    assert!(other_mid.dist(left_mid) < 1e-4, "the colliding line should lie on the left edge, got {other_mid:?} vs {left_mid:?}");
    assert!(other_mid.dist(right_mid) > 1.0, "it must not be the profile line");
    let colliding = cmds[0].1["axis"].as_str().unwrap().to_string();
    let d = revolve_dialog(&h.app, vec![Sel::Profile { sketch, index: outside }], Sel::SketchCurve { id: colliding, sketch: Some(other) });
    let cmds = apply_commands(&h.app, &d).unwrap();
    let origin = cmds[0].1["axis"]["origin"].as_array().unwrap_or_else(|| panic!("colliding pick stored {}", cmds[0].1["axis"]));
    let got = Vec3::new(origin[0].as_f64().unwrap(), origin[1].as_f64().unwrap(), origin[2].as_f64().unwrap());
    assert!(
        got.dist(other_mid) < 1.0 || (got - other_a).cross((other_b - other_a).normalized().unwrap()).len() < 1e-4,
        "axis {got:?} is not the highlighted line {other_a:?}–{other_b:?}"
    );
    let preview = h.app.session.preview(&cmds).unwrap();
    assert_pivots_on(&preview, about_left, about_right, "face sketch, colliding id");
}

fn spin(normal: Vec3, origin: Vec3, dir: Vec3, point: Vec3, deg: f64) -> Vec3 {
    let mut axis = dir.normalized().unwrap();
    if axis.cross(point - origin).dot(normal) < 0.0 {
        axis = -axis;
    }
    let t = deg.to_radians();
    let v = point - origin;
    origin + v * t.cos() + axis.cross(v) * t.sin() + axis * (axis.dot(v) * (1.0 - t.cos()))
}
