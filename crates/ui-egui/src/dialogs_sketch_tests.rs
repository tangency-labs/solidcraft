//! Every legacy sketch popup uses the same card as the feature dialogs.
use super::*;
use crate::scenario::{self, Harness};
use crate::sketch_modify_ui::Mod;

fn fixture() -> Harness {
    let mut h = Harness::new();
    h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    h.app.run("sketch.rectangle.two_point", json!({"p0":[10,10],"p1":[40,30]})).unwrap();
    h.frames(3);
    h
}

fn picked(h: &Harness, command: &str) -> Dialog {
    let mut d = Dialog::for_command(&h.app, command).unwrap();
    let sk = h.app.session.doc.sketch(h.app.session.active_sketch.unwrap()).unwrap();
    if let Kind::SketchModify(k) = &d.kind
        && !d.inputs.is_empty()
    {
        d.inputs[0].items = vec![if matches!(k, Mod::Corner { .. }) {
            Sel::SketchPoint { id: sk.points.iter().find(|p| p.id != "origin").unwrap().id.clone() }
        } else {
            Sel::SketchCurve { id: sk.curves[0].id.clone(), sketch: None }
        }];
        if matches!(k, Mod::Mirror) {
            d.inputs[1].items = vec![Sel::SketchCurve { id: sk.curves[3].id.clone(), sketch: None }];
        }
        if matches!(k, Mod::PatternCirc { .. }) {
            d.inputs[1].items = vec![Sel::SketchPoint { id: "origin".into() }];
        }
    }
    if let Kind::SketchModify(Mod::Move { x, .. }) = &mut d.kind {
        *x = "2 mm".into();
    }
    d
}

fn card(h: &Harness) {
    let outer = scenario::rect_of("dialog:card").unwrap();
    for key in ["dialog:header", "dialog:hint:tool", "dialog:footer"] {
        assert!(outer.expand(1.0).contains_rect(scenario::rect_of(key).unwrap()), "{key}");
    }
    for key in ["dialog:close", "dialog:ok", "dialog:cancel"] {
        assert!(outer.contains(scenario::handle_at(key).unwrap()), "{key}");
    }
    if let Some(palette) = h.app.viewport.palette_rect {
        assert!(!palette.intersects(outer), "{outer:?} / {palette:?}");
    }
}

fn form(command: &str, labels: &[&str]) {
    let mut h = fixture();
    let d = picked(&h, command);
    let expected = apply_commands(&h.app, &d).unwrap();
    assert_eq!(expected[0].0, command);
    assert_eq!(super::command(&d.kind), Some(command));
    let before = h.app.session.doc.to_json();
    h.app.dialog = Some(d);
    h.frames(6);
    card(&h);
    let d = h.app.dialog.as_ref().unwrap();
    assert_eq!(apply_commands(&h.app, d).unwrap(), expected, "rendering retains all command parameters");
    assert_eq!(h.app.session.doc.to_json(), before, "preview never commits");
    for label in d.inputs.iter().map(|i| i.label).chain(labels.iter().copied()) {
        let rect = scenario::rect_of(&format!("dialog:row:{label}")).unwrap_or_else(|| panic!("{command}: missing {label}"));
        assert_eq!(rect.height(), Tokens::get().control_height);
    }
    let spec = solidcraft_engine::find_command(command).unwrap();
    assert!(h.widget(spec.label).is_some(), "header uses command label");
    assert!(!spec.icon.is_empty());
    h.step(&json!({"click":{"handle":"dialog:cancel"}})).unwrap();
    assert!(h.app.dialog.is_none());
    assert_eq!(h.app.session.doc.to_json(), before);
}

macro_rules! form_case {
    ($name:ident, $id:literal, [$($label:literal),*]) => {
        #[test] fn $name() { form($id, &[$($label),*]); }
    };
}

form_case!(scale, "sketch.scale", ["Factor"]);
form_case!(offset, "sketch.offset", ["Direction", "Distance"]);
form_case!(mirror, "sketch.mirror", []);
form_case!(move_copy, "sketch.move", ["Position", "Angle"]);
form_case!(rectangular_pattern, "sketch.pattern.rectangular", ["Quantity", "Spacing", "Alignment", "Quantity 2", "Spacing 2", "Alignment 2"]);
form_case!(circular_pattern, "sketch.pattern.circular", ["Quantity", "Total angle", "Alignment"]);
form_case!(fillet, "sketch.fillet", ["Radius"]);
form_case!(chamfer_equal, "sketch.chamfer.equal_distance", ["Chamfer Type", "Distance"]);
form_case!(chamfer_two, "sketch.chamfer.two_distance", ["Chamfer Type", "Distance 1", "Distance 2"]);
form_case!(chamfer_angle, "sketch.chamfer.distance_angle", ["Chamfer Type", "Distance 1", "Angle"]);
form_case!(equation_curve, "sketch.equation_curve", ["Form", "x(t)", "y(t)", "Start", "End", "Tolerance"]);

/// Equation Curve (#8352): OK draws the curve from the window's formulas; its context menu
/// reopens the window on them, and OK then edits that curve instead of drawing another.
#[test]
fn equation_curve_window_draws_and_reopens_on_its_formulas() {
    let mut h = fixture();
    h.app.start("sketch.equation_curve");
    h.settle();
    let Some(Kind::SketchModify(Mod::Equation { form, y, .. })) = h.app.dialog.as_mut().map(|d| &mut d.kind) else { panic!("no equation window") };
    *form = 1;
    *y = "10 + 2 * sin(x / 4)".into();
    h.settle();
    h.step(&json!({"click":{"handle":"dialog:ok"}})).unwrap();
    h.settle();
    assert!(h.app.dialog.is_none());
    let sid = h.app.session.active_sketch.unwrap();
    let sk = h.app.session.doc.sketch(sid).unwrap().clone();
    let eq = sk.equations.first().cloned().expect("an equation curve");
    assert_eq!((eq.x.as_deref(), eq.y.as_str(), eq.end.as_str()), (None, "10 + 2 * sin(x / 4)", "2 * pi"));
    h.app.session.selection = vec![Sel::SketchCurve { id: eq.curve.clone(), sketch: None }];
    assert!(crate::sketch_modify_ui::selected_equation(&h.app).is_some());
    assert!(crate::sketch_modify_ui::edit_equation(&mut h.app, &eq.curve));
    h.settle();
    let Some(Kind::SketchModify(Mod::Equation { form, y, end, .. })) = h.app.dialog.as_mut().map(|d| &mut d.kind) else {
        panic!("no equation window")
    };
    assert_eq!((*form, y.as_str()), (1, "10 + 2 * sin(x / 4)"));
    *end = "20".into();
    let curves = sk.curves.len();
    h.settle();
    h.step(&json!({"click":{"handle":"dialog:ok"}})).unwrap();
    h.settle();
    let sk = h.app.session.doc.sketch(sid).unwrap();
    assert_eq!(sk.curves.len(), curves, "an edit, not another curve");
    assert_eq!(sk.equation_of(&eq.curve).map(|e| e.end.as_str()), Some("20"));
}

/// The window reaches the variable's unit (#9333): its checkbox draws the issue's curve, the
/// window reopens with it ticked, and unticking it reaches the curve too.
#[test]
fn equation_curve_window_gives_the_variable_its_ranges_unit() {
    use solidcraft_engine::sketch::VariableUnit;
    let mut h = fixture();
    h.app.session.execute("parameters.add", &json!({"name": "a", "expression": "10 mm"})).unwrap();
    h.app.start("sketch.equation_curve");
    h.settle();
    let Some(Kind::SketchModify(Mod::Equation { x, y, start, end, range_unit, .. })) = h.app.dialog.as_mut().map(|d| &mut d.kind) else {
        panic!("no equation window")
    };
    assert!(!*range_unit, "off to begin with");
    *x = "t".into();
    *y = "a ^ 2 / (4 * t)".into();
    *start = "a".into();
    *end = "3 * a".into();
    h.settle();
    let handle = format!("dialog:{}", crate::i18n::tr("ui.equation_variable_unit"));
    h.step(&json!({"click": {"handle": handle}})).unwrap();
    h.settle();
    let Some(Kind::SketchModify(Mod::Equation { range_unit, .. })) = h.app.dialog.as_ref().map(|d| &d.kind) else { panic!("no equation window") };
    assert!(*range_unit, "the click ticks it");
    h.step(&json!({"click":{"handle":"dialog:ok"}})).unwrap();
    h.settle();
    assert!(h.app.dialog.is_none(), "the curve is made");
    let sid = h.app.session.active_sketch.unwrap();
    let eq = h.app.session.doc.sketch(sid).unwrap().equations.first().cloned().expect("an equation curve");
    assert_eq!(eq.variable_unit, VariableUnit::Range);
    assert!(crate::sketch_modify_ui::edit_equation(&mut h.app, &eq.curve));
    h.settle();
    let Some(Kind::SketchModify(Mod::Equation { y, start, end, range_unit, .. })) = h.app.dialog.as_mut().map(|d| &mut d.kind) else {
        panic!("no equation window")
    };
    assert!(*range_unit, "reopens ticked");
    *y = "t / 2".into();
    *start = "0".into();
    *end = "10".into();
    *range_unit = false;
    h.settle();
    h.step(&json!({"click":{"handle":"dialog:ok"}})).unwrap();
    h.settle();
    assert_eq!(h.app.session.doc.sketch(sid).unwrap().equation_of(&eq.curve).map(|e| e.variable_unit), Some(VariableUnit::Number));
}

#[test]
fn create_sketch_uses_card_and_still_starts_immediately_on_plane_pick() {
    let mut h = Harness::new();
    h.app.start("sketch.create");
    h.frames(4);
    card(&h);
    assert!(scenario::rect_of("dialog:field:Plane").is_some());
    assert_eq!(scenario::count_of("dialog:ok:enabled"), Some(0.0));
    h.step(&json!({"click":{"plane":"XY"}})).unwrap();
    assert!(h.app.dialog.is_none());
    assert!(h.app.session.active_sketch.is_some());
}

#[test]
fn intersect_dialog_filters_geometry_or_whole_bodies_and_controls_projection_link() {
    let mut h = Harness::new();
    h.app.run("solid.box", json!({"length":40,"width":30,"height":20,"body_name":"Post"})).unwrap();
    h.app.run("sketch.create", json!({"plane":"XY","offset":10})).unwrap();
    h.app.session.selection = vec![Sel::Body { name: "Post".into() }];
    h.app.start("sketch.intersect");
    h.frames(4);

    let d = h.app.dialog.as_ref().expect("Intersect opens an options dialog");
    assert!(matches!(d.kind, Kind::SketchIntersect { bodies: true, projection_link: true }));
    assert_eq!(d.inputs[0].accept, super::BODIES);
    assert_eq!(d.inputs[0].items, vec![Sel::Body { name: "Post".into() }]);
    assert!(scenario::rect_of("dialog:row:ui.sketch_intersect_geometry").is_some());
    assert!(scenario::handle_at("dialog:Selection Filter").is_some());
    assert!(scenario::rect_of("dialog:row:Projection Link").is_some());

    h.step(&json!({"click":{"handle":"dialog:Selection Filter"}})).unwrap();
    h.step(&json!({"click":{"handle":"dialog:Selection Filter:Geometry"}})).unwrap();
    assert_eq!(h.app.dialog.as_ref().unwrap().inputs[0].accept, super::INTERSECT_GEOMETRY);
    assert!(h.app.dialog.as_ref().unwrap().inputs[0].items.is_empty());
    h.step(&json!({"click":{"handle":"dialog:Selection Filter"}})).unwrap();
    h.step(&json!({"click":{"handle":"dialog:Selection Filter:Bodies"}})).unwrap();
    assert_eq!(h.app.dialog.as_ref().unwrap().inputs[0].accept, super::BODIES);
    h.step(&json!({"click":{"handle":"dialog:Projection Link"}})).unwrap();
    assert!(matches!(h.app.dialog.as_ref().unwrap().kind, Kind::SketchIntersect { projection_link: false, .. }));

    {
        let d = h.app.dialog.as_mut().unwrap();
        d.inputs[0].items = vec![Sel::Body { name: "Post".into() }];
    }
    let d = h.app.dialog.as_ref().unwrap();
    assert_eq!(apply_commands(&h.app, d).unwrap(), vec![("sketch.intersect".into(), json!({"refs":[{"body":"Post"}],"projection_link":false}))]);
    // A model edge is cut with the plane too (#10484).
    let d = h.app.dialog.as_mut().unwrap();
    d.inputs[0].items = vec![Sel::Edge { body: "Post".into(), index: 0, point: solidcraft_engine::geom::Vec3::new(0.0, 0.0, 10.0) }];
    let cmds = apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap();
    assert_eq!(cmds, vec![("sketch.intersect".into(), json!({"refs":[{"edge":[0.0,0.0,10.0],"body":"Post"}],"projection_link":false}))]);
}

/// Intersect takes the lines of another sketch (#10484): a rectangle on XY, a sketch on a plane
/// across it, and a click on one of the rectangle's sides gives the point where it crosses.
#[test]
fn intersect_dialog_takes_a_line_of_another_sketch() {
    let mut h = Harness::new();
    h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    h.app.run("sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[40,30]})).unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
    let rectangle = h.app.session.doc.features.last().unwrap().id;
    h.app.run("sketch.create", json!({"plane":"YZ","offset":20})).unwrap();
    h.app.run("view.home", json!({})).unwrap();
    h.app.start("sketch.intersect");
    h.settle();
    let d = h.app.dialog.as_ref().expect("Intersect opens an options dialog");
    assert!(matches!(d.kind, Kind::SketchIntersect { bodies: false, .. }));
    assert_eq!(d.inputs[0].accept, super::INTERSECT_GEOMETRY);

    h.step(&json!({"click":{"world":[10,0,0]}})).unwrap();
    let d = h.app.dialog.as_ref().unwrap();
    let [Sel::ExternalSketchCurve { sketch, id }] = d.inputs[0].items.as_slice() else { panic!("the side is picked: {:?}", d.inputs[0].items) };
    assert_eq!(*sketch, rectangle);
    let cmds = apply_commands(&h.app, d).unwrap();
    assert_eq!(cmds, vec![("sketch.intersect".into(), json!({"refs":[{"sketch":rectangle,"curve":id}],"projection_link":true}))]);
    h.step(&json!({"click":{"handle":"dialog:ok"}})).unwrap();
    h.settle();
    assert!(h.app.dialog.is_none());
    let state = h.app.session.world_state();
    let sk = state.sketch(h.app.session.active_sketch.unwrap()).unwrap();
    let link = &sk.sketch.links[0].id;
    let at: Vec<_> = sk.sketch.link_points(link).iter().filter_map(|p| sk.sketch.point(*p)).map(|p| sk.plane.to_world(p)).collect();
    assert_eq!(at.len(), 1, "{at:?}");
    assert!(at[0].dist(solidcraft_engine::geom::Vec3::new(20.0, 0.0, 0.0)) < 1e-6, "{at:?}");
}

#[test]
fn project_dialog_keeps_geometry_targets_and_can_project_whole_bodies_unlinked() {
    let mut h = Harness::new();
    h.app.run("solid.box", json!({"length":40,"width":30,"height":20,"body_name":"Plate"})).unwrap();
    h.app.run("sketch.create", json!({"plane":"XY","offset":10})).unwrap();
    h.app.session.selection = vec![Sel::Body { name: "Plate".into() }];
    h.app.start("sketch.project");
    h.frames(4);

    let d = h.app.dialog.as_ref().expect("Project opens an options dialog");
    assert!(matches!(d.kind, Kind::SketchProject { bodies: true, projection_link: true }));
    assert_eq!(d.inputs[0].accept, super::BODIES);
    assert_eq!(d.inputs[0].items, vec![Sel::Body { name: "Plate".into() }]);
    assert!(scenario::handle_at("dialog:Selection Filter").is_some());
    assert!(scenario::rect_of("dialog:row:Projection Link").is_some());

    h.step(&json!({"click":{"handle":"dialog:Selection Filter"}})).unwrap();
    assert!(scenario::handle_at("dialog:Selection Filter:Geometry").is_some());
    h.step(&json!({"click":{"handle":"dialog:Selection Filter:Geometry"}})).unwrap();
    assert_eq!(h.app.dialog.as_ref().unwrap().inputs[0].accept, super::PROJECT_GEOMETRY);
    assert!(h.app.dialog.as_ref().unwrap().inputs[0].items.is_empty());
    h.step(&json!({"click":{"handle":"dialog:Selection Filter"}})).unwrap();
    h.step(&json!({"click":{"handle":"dialog:Selection Filter:Bodies"}})).unwrap();
    h.step(&json!({"click":{"handle":"dialog:Projection Link"}})).unwrap();
    assert!(matches!(h.app.dialog.as_ref().unwrap().kind, Kind::SketchProject { projection_link: false, .. }));

    {
        let d = h.app.dialog.as_mut().unwrap();
        d.inputs[0].items = vec![Sel::Body { name: "Plate".into() }];
    }
    let d = h.app.dialog.as_ref().unwrap();
    assert_eq!(apply_commands(&h.app, d).unwrap(), vec![("sketch.project".into(), json!({"refs":[{"body":"Plate"}],"projection_link":false}))]);
    h.step(&json!({"click":{"handle":"dialog:ok"}})).unwrap();
    h.settle();
    assert!(h.app.dialog.is_none());
    let sketch = h.app.session.active_sketch.expect("Project leaves the sketch active");
    let state = h.app.session.world_state();
    let sketch = &state.sketch(sketch).unwrap().sketch;
    assert!(sketch.links.is_empty());
    assert!(!sketch.curves.is_empty());
}

#[test]
fn apply_commits_once_keeps_values_and_waits_for_new_selection_ok_closes() {
    for command in ["sketch.offset", "sketch.mirror", "sketch.pattern.rectangular", "sketch.pattern.circular"] {
        let mut h = fixture();
        h.app.dialog = Some(picked(&h, command));
        h.settle();
        let before = apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap();
        assert_eq!(scenario::count_of("dialog:ok:enabled"), Some(1.0), "{command}");
        let undo = h.app.session.history.len();
        h.step(&json!({"click":{"handle":"dialog:apply"}})).unwrap();
        h.settle();
        assert_eq!(h.app.session.history.len(), undo + 1, "{command}: one Apply, one undo");
        let d = h.app.dialog.as_ref().unwrap();
        assert!(d.inputs.iter().all(|i| i.items.is_empty()));
        assert_eq!(d.active, 0);
        // Repick the original input; the numeric values are unchanged.
        let next = picked(&h, command);
        h.app.dialog.as_mut().unwrap().inputs = next.inputs;
        let after = apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap();
        assert_eq!(before, after);
        h.settle();
        h.step(&json!({"click":{"handle":"dialog:ok"}})).unwrap();
        h.settle();
        assert!(h.app.dialog.is_none(), "{command}: OK closes");
    }
}

/// Interface › Dialog buttons › Top (#6708): Apply sits between Cancel and OK in the header row
/// and still commits once, keeping the dialog open.
#[test]
fn apply_in_the_header_row_commits_once_and_keeps_the_dialog() {
    let mut h = fixture();
    h.call("ui.prefs", json!({"dialog_buttons":"top"}));
    h.app.dialog = Some(picked(&h, "sketch.offset"));
    h.settle();
    let header = scenario::rect_of("dialog:header").unwrap();
    assert!(scenario::rect_of("dialog:footer").is_none());
    let (cancel, apply, ok) =
        (scenario::rect_of("dialog:cancel").unwrap(), scenario::rect_of("dialog:apply").unwrap(), scenario::rect_of("dialog:ok").unwrap());
    assert!(header.contains_rect(cancel) && header.contains_rect(apply) && header.contains_rect(ok));
    assert!(cancel.right() < apply.left() && apply.right() < ok.left());
    let undo = h.app.session.history.len();
    h.step(&json!({"click":{"handle":"dialog:apply"}})).unwrap();
    h.settle();
    assert_eq!(h.app.session.history.len(), undo + 1, "one Apply, one undo");
    assert!(h.app.dialog.is_some(), "Apply keeps the dialog");
    assert_eq!(scenario::rect_of("dialog:apply"), Some(apply), "the buttons stay where they were");
}

/// Interface › Dialog buttons › Top (#9860): the sketch panel's Finish Sketch row sits above its
/// options, and below them with the preference off.
#[test]
fn sketch_panel_finish_row_follows_dialog_buttons() {
    for (placement, on_top) in [("top", true), ("bottom", false)] {
        let mut h = fixture();
        h.call("ui.prefs", json!({"dialog_buttons": placement}));
        h.settle();
        let finish = scenario::rect_of("sketch_palette:finish").unwrap_or_else(|| panic!("{placement}: no Finish Sketch"));
        let first = scenario::rect_of("sketch_palette:toggle:show_constraints").unwrap_or_else(|| panic!("{placement}: no first option"));
        if on_top {
            assert!(finish.bottom() <= first.top(), "top: Finish {finish:?} above its first option {first:?}");
        } else {
            assert!(first.bottom() <= finish.top(), "bottom: Finish {finish:?} below its first option {first:?}");
        }
        assert!(scenario::handle_at("sketch_palette:finish").is_some(), "{placement}: the handle stays");
    }
}

#[test]
fn picked_list_removes_one_item_and_field_still_activates_input() {
    let mut h = fixture();
    let mut d = picked(&h, "sketch.scale");
    let second = h.app.session.doc.sketch(1).unwrap().curves[1].id.clone();
    d.inputs[0].items.push(Sel::SketchCurve { id: second.clone(), sketch: None });
    h.app.dialog = Some(d);
    h.frames(4);
    h.step(&json!({"click":{"handle":"dialog:list:Entities:0:remove"}})).unwrap();
    assert_eq!(h.app.dialog.as_ref().unwrap().inputs[0].items, vec![Sel::SketchCurve { id: second, sketch: None }]);
    h.step(&json!({"click":{"handle":"dialog:Base point"}})).unwrap();
    assert_eq!(h.app.dialog.as_ref().unwrap().active, 1);
}

#[test]
fn panel_dimension_preserves_expression_and_command_and_ok_closes() {
    let mut h = fixture();
    h.app.run("parameters.add", json!({"name":"span","expression":"3 mm"})).unwrap();
    h.app.dialog = Some(Dialog::edit_param(&h.app.session, "span"));
    h.frames(4);
    card(&h);
    assert!(h.ctx.memory(|m| m.focused().is_some()), "single-value popup takes keyboard focus on open");
    h.step(&json!({"click":{"handle":"dialog:span"}})).unwrap();
    h.step(&json!({"key":"A","cmd":true})).unwrap();
    h.step(&json!({"text":"2 * 5 mm"})).unwrap();
    let commands = apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap();
    assert_eq!(commands, vec![("parameters.change".into(), json!({"name":"span","expression":"2 * 5 mm"}))]);
    h.step(&json!({"click":{"handle":"dialog:ok"}})).unwrap();
    assert!(h.app.dialog.is_none());
    assert_eq!(h.app.session.doc.param("span").unwrap().expr, "2 * 5 mm");
}

#[test]
fn text_and_text_move_cards_preserve_parameters_and_cancel_live_edits() {
    let mut h = fixture();
    let out = h.app.run("sketch.text", json!({"text":"II","at":[50,20],"height":8})).unwrap();
    h.app.session.selection = vec![Sel::SketchCurve { id: out["curves"][0].as_str().unwrap().into(), sketch: None }];
    h.app.start("sketch.edit_text");
    h.frames(4);
    card(&h);
    for label in ["Text", "Font", "Style", "Alignment", "Position", "Height", "Angle", "Letter Spacing", "Line Spacing", "Path"] {
        assert!(scenario::rect_of(&format!("dialog:row:{label}")).is_some(), "{label}");
    }
    let before = h.app.session.doc.to_json();
    h.step(&json!({"text":"Changed"})).unwrap();
    h.step(&json!({"click":{"handle":"dialog:cancel"}})).unwrap();
    assert_eq!(h.app.session.doc.to_json(), before);
    h.app.session.selection = vec![Sel::SketchCurve { id: out["curves"][0].as_str().unwrap().into(), sketch: None }];
    h.app.start("sketch.move");
    h.frames(4);
    card(&h);
    assert!(matches!(h.app.dialog.as_ref().unwrap().kind, Kind::TextMove(_)));
    assert!(scenario::handle_at("dialog:Distance X").is_some());
    assert!(scenario::handle_at("dialog:Distance Y").is_some());
    assert!(scenario::handle_at("dialog:Create Copy").is_some());
    h.step(&json!({"click":{"handle":"dialog:ok"}})).unwrap();
    assert!(h.app.dialog.is_none());
}

#[test]
fn apply_keeps_the_panel_open_after_asynchronous_evaluation() {
    let mut h = fixture();
    let mut d = picked(&h, "sketch.pattern.rectangular");
    let cmds = apply_commands(&h.app, &d).unwrap();
    let prepared = h.app.session.scratch().compute_commands(&cmds, None).unwrap();
    d.repeat_after_apply = true;
    h.app.dialog = Some(d);
    let (finish, done) = std::sync::mpsc::channel();
    h.app.applying =
        Some(Applying { done, ready: None, started: crate::now_ms(), cmds, seen: true, progress: Default::default(), quality: Default::default() });
    let undo = h.app.session.history.len();
    h.frames(2);
    assert_eq!(h.app.session.history.len(), undo);
    finish.send(Ok(prepared)).ok().unwrap();
    h.frames(3);
    assert!(h.app.applying.is_none());
    let d = h.app.dialog.as_ref().unwrap();
    assert_eq!(h.app.session.history.len(), undo + 1);
    assert!(d.inputs.iter().all(|i| i.items.is_empty()));
    assert!(!d.repeat_after_apply);
}

#[test]
fn circular_pattern_quantity_arrows_step_the_count_within_its_limits_and_keep_parameters() {
    let mut h = fixture();
    h.app.dialog = Some(picked(&h, "sketch.pattern.circular"));
    h.settle();
    let count = |h: &Harness| match &h.app.dialog.as_ref().unwrap().kind {
        Kind::SketchModify(Mod::PatternCirc { count, .. }) => count.clone(),
        other => panic!("not a sketch pattern window: {other:?}"),
    };
    let set = |h: &mut Harness, text: &str| {
        if let Some(Kind::SketchModify(Mod::PatternCirc { count, .. })) = h.app.dialog.as_mut().map(|d| &mut d.kind) {
            *count = text.into();
        }
        h.frames(2);
    };
    let sent = |h: &Harness| apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap()[0].1["count"].clone();
    let previewed = |h: &Harness| crate::dialogs::preview_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap()[0].1["count"].clone();
    assert_eq!(count(&h), "6");
    h.step(&json!({"click":{"handle":"dialog:Quantity up"}})).unwrap();
    assert_eq!(count(&h), "7");
    assert_eq!((sent(&h), previewed(&h)), (json!("7"), json!("7")), "the preview follows the arrow as it follows typing");
    h.step(&json!({"click":{"handle":"dialog:Quantity down"}})).unwrap();
    h.step(&json!({"click":{"handle":"dialog:Quantity down"}})).unwrap();
    assert_eq!(count(&h), "5");
    // A circular pattern needs the original and one copy, and makes at most 500.
    set(&mut h, "2");
    h.step(&json!({"click":{"handle":"dialog:Quantity down"}})).unwrap();
    assert_eq!(count(&h), "2");
    set(&mut h, "500");
    h.step(&json!({"click":{"handle":"dialog:Quantity up"}})).unwrap();
    assert_eq!(count(&h), "500");
    // A count typed past the limit steps back inside it.
    set(&mut h, "900");
    h.step(&json!({"click":{"handle":"dialog:Quantity down"}})).unwrap();
    assert_eq!(count(&h), "500");
    // An expression keeps its parameter: the arrows adjust a trailing offset.
    h.app.run("parameters.add", json!({"name":"holes","expression":"4","unit":""})).unwrap();
    set(&mut h, "holes");
    h.step(&json!({"click":{"handle":"dialog:Quantity up"}})).unwrap();
    assert_eq!((count(&h), sent(&h)), ("holes + 1".into(), json!("holes + 1")), "the command keeps the parameter");
    for _ in 0..2 {
        h.step(&json!({"click":{"handle":"dialog:Quantity down"}})).unwrap();
    }
    assert_eq!((count(&h), sent(&h)), ("holes - 1".into(), json!("holes - 1")));
    // Text that is not a count leaves the arrows without effect.
    set(&mut h, "nothing");
    h.step(&json!({"click":{"handle":"dialog:Quantity up"}})).unwrap();
    assert_eq!(count(&h), "nothing");
}

#[test]
fn rectangular_pattern_quantity_arrows_keep_both_directions_within_the_copy_limit() {
    let mut h = fixture();
    h.app.dialog = Some(picked(&h, "sketch.pattern.rectangular"));
    h.settle();
    let counts = |h: &Harness| match &h.app.dialog.as_ref().unwrap().kind {
        Kind::SketchModify(Mod::PatternRect { count, count2, .. }) => (count.clone(), count2.clone()),
        other => panic!("not a sketch pattern window: {other:?}"),
    };
    let (one, _) = counts(&h);
    h.step(&json!({"click":{"handle":"dialog:Quantity 2 up"}})).unwrap();
    assert_eq!(counts(&h), (one.clone(), "2".into()));
    if let Some(Kind::SketchModify(Mod::PatternRect { count, count2, .. })) = h.app.dialog.as_mut().map(|d| &mut d.kind) {
        (*count, *count2) = ("250".into(), "2".into());
    }
    h.frames(2);
    // 250 × 2 is the 500 copies a sketch pattern makes at most.
    h.step(&json!({"click":{"handle":"dialog:Quantity up"}})).unwrap();
    h.step(&json!({"click":{"handle":"dialog:Quantity 2 up"}})).unwrap();
    assert_eq!(counts(&h), ("250".into(), "2".into()));
    h.step(&json!({"click":{"handle":"dialog:Quantity down"}})).unwrap();
    assert_eq!(counts(&h), ("249".into(), "2".into()));
}

#[test]
fn sketch_pattern_dialog_preserves_expression_text_when_applying_and_editing() {
    let mut h = fixture();
    h.app.run("parameters.add", json!({"name":"pitch","expression":"30","unit":"mm"})).unwrap();
    for command in ["sketch.pattern.rectangular", "sketch.pattern.circular"] {
        let mut d = picked(&h, command);
        match &mut d.kind {
            Kind::SketchModify(Mod::PatternRect { count, spacing, .. }) => {
                *count = "1 + 2".into();
                *spacing = "pitch".into();
            }
            Kind::SketchModify(Mod::PatternCirc { count, angle, .. }) => {
                *count = "1 + 2".into();
                *angle = "45 deg + 45 deg".into();
            }
            _ => panic!("expected a sketch pattern dialog"),
        }
        let cmds = apply_commands(&h.app, &d).unwrap();
        assert_eq!(cmds[0].1["count"], "1 + 2");
        let (key, expression) = if command.ends_with("rectangular") { ("spacing", "pitch") } else { ("angle", "45 deg + 45 deg") };
        assert_eq!(cmds[0].1[key], expression);
        let result = h.app.run(command, cmds[0].1.clone()).unwrap();
        if command.ends_with("rectangular") {
            h.app.run("parameters.change", json!({"name":"pitch","expression":"40"})).unwrap();
        }
        assert!(crate::sketch_modify_ui::edit_pattern(&mut h.app, result["pattern"].as_str().unwrap()));
        match &h.app.dialog.as_ref().unwrap().kind {
            Kind::SketchModify(Mod::PatternRect { count, spacing, .. }) => {
                assert_eq!(count, "1 + 2");
                assert_eq!(spacing, expression);
            }
            Kind::SketchModify(Mod::PatternCirc { count, angle, .. }) => {
                assert_eq!(count, "1 + 2");
                assert_eq!(angle, expression);
            }
            _ => panic!("expected a sketch pattern dialog"),
        }
    }
}

/// Transform scales, turns and moves what is picked as one, by handles on its box or by the
/// window's numbers, and OK applies it in a single step.
#[test]
fn transform_handles_and_fields_scale_move_and_turn_the_picks() {
    let mut h = fixture();
    h.step(&json!({"call": "ui.view", "params": {"view": "top"}})).unwrap();
    h.settle();
    let ids: Vec<String> = {
        let sk = h.app.session.doc.sketch(h.app.session.active_sketch.unwrap()).unwrap();
        sk.curves.iter().map(|c| c.id.clone()).collect()
    };
    let mut d = Dialog::for_command(&h.app, "sketch.transform").unwrap();
    d.inputs[0].items = ids.iter().map(|id| Sel::SketchCurve { id: id.clone(), sketch: None }).collect();
    h.app.dialog = Some(d);
    h.frames(4);
    let state = |h: &Harness| match h.app.dialog.as_ref().map(|d| d.kind.clone()) {
        Some(Kind::SketchModify(Mod::Transform { x, y, factor, angle, frame })) => (x, y, factor, angle, frame),
        other => panic!("no transform window: {other:?}"),
    };
    // The box is the rectangle's: 30 x 20 around (25, 20).
    assert_eq!(state(&h).4, Some(([25.0, 20.0], [30.0, 20.0])));
    // The corner at (40, 30) dragged to (55, 40): one and a half times the size about the corner at (10, 10).
    h.step(&json!({"drag": [{"handle": "transform:handle:3"}, {"world": [55, 40, 0]}]})).unwrap();
    h.frames(3);
    let (x, y, factor, _, _) = state(&h);
    assert!((factor.parse::<f64>().unwrap() - 1.5).abs() < 0.05, "scaled: {factor}");
    assert!(x.starts_with("7.5") && y.starts_with("5"), "the middle moved to (32.5, 25): {x}, {y}");
    h.step(&json!({"key": "Enter"})).unwrap();
    h.settle();
    assert!(h.app.dialog.is_none(), "{:?}", h.app.dialog.as_ref().and_then(|d| d.error.clone()));
    let state = h.app.session.world_state();
    let sk = &state.sketch(h.app.session.active_sketch.unwrap()).unwrap().sketch;
    let (mut lo, mut hi) = ((f64::MAX, f64::MAX), (f64::MIN, f64::MIN));
    for p in (0..sk.curves.len()).flat_map(|i| sk.polyline(i)) {
        lo = (lo.0.min(p.x), lo.1.min(p.y));
        hi = (hi.0.max(p.x), hi.1.max(p.y));
    }
    assert!((lo.0 - 10.0).abs() < 0.5 && (lo.1 - 10.0).abs() < 0.5, "the fixed corner stayed: {lo:?}");
    assert!((hi.0 - 55.0).abs() < 1.5 && (hi.1 - 40.0).abs() < 1.5, "the far corner went to (55, 40): {hi:?}");
    h.app.run("edit.undo", json!({})).unwrap();
    h.settle();
    let state = h.app.session.world_state();
    let sk = &state.sketch(h.app.session.active_sketch.unwrap()).unwrap().sketch;
    let top = (0..sk.curves.len()).flat_map(|i| sk.polyline(i)).fold(f64::MIN, |m, p| m.max(p.x));
    assert!((top - 40.0).abs() < 1e-6, "one undo puts it back: {top}");
}
