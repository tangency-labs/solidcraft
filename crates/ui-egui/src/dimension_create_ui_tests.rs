//! Dimension creation is an editable draft, accepted by the real focused value field.
use crate::scenario::Harness;
use serde_json::json;

fn line(end: [f64; 2]) -> Harness {
    crate::dim_view::cancel();
    let mut h = Harness::new();
    h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    h.app.run("sketch.line", json!({"points":[[0,0],end]})).unwrap();
    h.call("ui.view", json!({"view":"top"}));
    // Leave room around the measured shape while testing the draft itself. Separate Fit
    // tests exercise measured dock occlusion without changing the picking projection.
    h.app.cam.distance = 80.0;
    h.app.cam.target = solidcraft_engine::geom::Vec3::new(end[0] * 0.5, end[1] * 0.5, 0.0);
    h.settle();
    h
}
fn place(h: &mut Harness, picks: &[[f64; 2]], at: [f64; 2]) {
    h.step(&json!({"start":"sketch.dimension"})).unwrap();
    for p in picks {
        h.step(&json!({"click":{"world":[p[0],p[1],0]}})).unwrap();
    }
    h.step(&json!({"click":{"world":[at[0],at[1],0]}})).unwrap();
    assert!(crate::dim_view::editing(), "placement opens an editor");
    assert_eq!(h.ctx.memory(|m| m.focused()), Some(egui::Id::new("sc_dim_edit")));
}
fn type_value(h: &mut Harness, text: &str) {
    h.step(&json!({"key":"A","cmd":true})).unwrap();
    if text.is_empty() {
        h.step(&json!({"key":"Backspace"})).unwrap();
    } else {
        h.step(&json!({"text":text})).unwrap();
    }
}

#[test]
fn dimension_editor_names_and_reference_picks_drive_new_and_existing_dimensions() {
    let mut h = line([20., 0.]);
    let source =
        h.app.run("sketch.dimension", json!({"entities":["l1"],"value":20,"text_at":[10,8]})).unwrap()["param"].as_str().unwrap().to_string();
    h.app.run("sketch.line", json!({"points":[[0,-10],[12,-10]],"infer":false})).unwrap();
    h.settle();
    place(&mut h, &[[6., -10.]], [6., -16.]);
    let target = h.app.session.doc.next_d_name();
    assert!(h.widget(&format!("Dimension: {target}")).is_some(), "new dimension shows its actual parameter name");
    // Let the preceding placement clicks expire before starting a distinct double-click.
    h.frames(40);
    h.step(&json!({"click":{"dimension":source},"double":true})).unwrap();
    assert!(crate::dim_view::editing(), "reference click keeps the draft open");
    h.step(&json!({"text":" / 2"})).unwrap();
    h.step(&json!({"key":"Enter"})).unwrap();
    assert!(!crate::dim_view::editing(), "{}", h.call("ui.inspect", json!({})));
    assert_eq!(h.app.session.doc.param(&target).unwrap().expr, format!("{source} / 2"));
    h.app.tool = None;
    h.settle();
    crate::dim_view::edit_param(&h.app, &target);
    h.frames(5);
    assert!(h.widget(&format!("Dimension: {target}")).is_some());
    type_value(&mut h, "2 * ");
    h.frames(40);
    h.step(&json!({"click":{"dimension":source},"double":true})).unwrap();
    h.step(&json!({"key":"Enter"})).unwrap();
    assert_eq!(h.app.session.doc.param(&target).unwrap().expr, format!("2 * {source}"));
    h.app.run("parameters.change", json!({"name":source,"expression":"15 mm"})).unwrap();
    assert!((h.app.session.doc.eval(&target, solidcraft_engine::doc::expr::Kind::Length).unwrap() - 30.).abs() < 1e-6);
    crate::dim_view::cancel();
}

/// Two dimensioned lines, editing `d2` with `typed` in its value field.
fn editing_d2(typed: &str) -> Harness {
    let mut h = line([20., 0.]);
    h.app.run("sketch.dimension", json!({"entities":["l1"],"value":20,"text_at":[10,8]})).unwrap();
    h.app.run("sketch.line", json!({"points":[[0,-10],[12,-10]],"infer":false})).unwrap();
    h.app.run("sketch.dimension", json!({"entities":["l2"],"value":12,"text_at":[6,-16]})).unwrap();
    h.settle();
    crate::dim_view::edit_param(&h.app, "d2");
    h.frames(5);
    type_value(&mut h, typed);
    h
}

/// Click `d1`, check the editor stays focused, then apply and return d2's expression.
fn click_d1_and_apply(h: &mut Harness) -> String {
    h.step(&json!({"click":{"dimension":"d1"}})).unwrap();
    assert!(crate::dim_view::editing(), "a reference click keeps the editor open");
    h.frames(2);
    assert_eq!(h.ctx.memory(|m| m.focused()), Some(egui::Id::new("sc_dim_edit")), "the value field keeps the keyboard");
    h.step(&json!({"key":"Enter"})).unwrap();
    assert!(!crate::dim_view::editing(), "{}", h.call("ui.inspect", json!({})));
    let expr = h.app.session.doc.param("d2").unwrap().expr.clone();
    crate::dim_view::cancel();
    expr
}

#[test]
fn clicking_another_dimension_appends_its_name_to_the_typed_text() {
    for (typed, expected) in [("2 * ", "2 * d1"), ("10 + ", "10 + d1")] {
        let mut h = editing_d2(typed);
        assert_eq!(click_d1_and_apply(&mut h), expected, "typed {typed:?}");
    }
}

#[test]
fn clicking_another_dimension_replaces_a_lone_value() {
    for typed in ["10", "10 mm", "-3.5"] {
        let mut h = editing_d2(typed);
        assert_eq!(click_d1_and_apply(&mut h), "d1", "typed {typed:?}");
    }
    // The cursor inside the number does not matter: the whole value goes.
    let mut h = editing_d2("125");
    h.step(&json!({"key":"Home"})).unwrap();
    h.step(&json!({"key":"ArrowRight"})).unwrap();
    assert_eq!(click_d1_and_apply(&mut h), "d1");
}

#[test]
fn clicking_another_dimension_lets_typing_continue_after_its_name() {
    let mut h = editing_d2("2 * ");
    h.step(&json!({"click":{"dimension":"d1"}})).unwrap();
    h.frames(2);
    h.step(&json!({"text":" + 1"})).unwrap();
    h.step(&json!({"key":"Enter"})).unwrap();
    assert_eq!(h.app.session.doc.param("d2").unwrap().expr, "2 * d1 + 1");
    crate::dim_view::cancel();
}

#[test]
fn clicking_another_dimension_inserts_its_name_at_the_cursor() {
    let mut h = editing_d2("( ) / 2");
    h.step(&json!({"key":"Home"})).unwrap();
    h.step(&json!({"key":"ArrowRight"})).unwrap();
    h.step(&json!({"key":"ArrowRight"})).unwrap();
    assert_eq!(click_d1_and_apply(&mut h), "( d1) / 2");
}

#[test]
fn clicking_another_dimension_replaces_the_selected_text() {
    let mut h = editing_d2("5 + 7");
    h.step(&json!({"key":"ArrowLeft","shift":true})).unwrap();
    assert_eq!(click_d1_and_apply(&mut h), "5 + d1");
}

#[test]
fn double_clicking_another_dimension_inserts_its_name_once() {
    let mut h = editing_d2("3 * ");
    // Synthetic input feeds one pointer event per frame, so this is a click and, a frame
    // later, a second click that egui reads as a double-click.
    h.step(&json!({"click":{"dimension":"d1"},"double":true})).unwrap();
    h.step(&json!({"key":"Enter"})).unwrap();
    assert_eq!(h.app.session.doc.param("d2").unwrap().expr, "3 * d1");
    crate::dim_view::cancel();
}

/// With the Dimension tool on, one click on an existing dimension edits its value, as a
/// double-click does without a tool. The tool stays on, and geometry still starts a new one.
#[test]
fn dimension_tool_click_on_an_existing_dimension_edits_its_value() {
    let mut h = line([20., 0.]);
    h.app.run("sketch.dimension", json!({"entities":["l1"],"value":20,"text_at":[10,8]})).unwrap();
    let created = h.app.run("sketch.line", json!({"points":[[0,-10],[12,-10]],"infer":false})).unwrap();
    let second = created["created"]["curves"][0].as_str().unwrap().to_string();
    h.settle();
    h.step(&json!({"start":"sketch.dimension"})).unwrap();
    h.step(&json!({"click":{"dimension":"d1"}})).unwrap();
    assert!(crate::dim_view::editing(), "a click on the dimension opens its value");
    h.frames(5);
    assert_eq!(h.ctx.memory(|m| m.focused()), Some(egui::Id::new("sc_dim_edit")), "the value field takes the keyboard");
    type_value(&mut h, "25");
    h.step(&json!({"key":"Enter"})).unwrap();
    assert!(!crate::dim_view::editing(), "{}", h.call("ui.inspect", json!({})));
    assert!((h.app.session.doc.eval("d1", solidcraft_engine::doc::expr::Kind::Length).unwrap() - 25.).abs() < 1e-6);
    assert_eq!(h.app.session.doc.next_d_name(), "d2", "no new dimension was made");
    assert_eq!(h.app.tool.as_ref().map(|t| t.cmd), Some("sketch.dimension"), "the tool stays on");
    // A click on geometry still picks it for a new dimension.
    h.frames(40);
    h.step(&json!({"click":{"world":[6,-10,0]}})).unwrap();
    assert_eq!(h.app.tool.as_ref().map(|t| t.picks.clone()), Some(vec![second]));
    crate::dim_view::cancel();
}

/// With dimensions off in the selection filter, the Dimension tool leaves them alone.
#[test]
fn dimension_tool_click_respects_the_selection_filter() {
    let mut h = line([20., 0.]);
    h.app.run("sketch.dimension", json!({"entities":["l1"],"value":20,"text_at":[10,8]})).unwrap();
    h.app.ui.pick_off.push("dimensions".into());
    h.settle();
    h.step(&json!({"start":"sketch.dimension"})).unwrap();
    h.step(&json!({"click":{"dimension":"d1"}})).unwrap();
    assert!(!crate::dim_view::editing(), "a filtered-out dimension opened for editing");
    crate::dim_view::cancel();
}

#[test]
fn dimension_parameters_use_typing_and_ctrl_space_completion() {
    for (prefix, expected) in [("wid", "width"), ("", "width"), ("d", "d1")] {
        let mut h = line([20., 0.]);
        h.app.run("parameters.add", json!({"name":"width","expression":"25 mm"})).unwrap();
        if prefix == "d" {
            h.app.run("sketch.line", json!({"points":[[0,-10],[12,-10]],"infer":false})).unwrap();
            h.app.run("sketch.dimension", json!({"entities":["l2"],"value":12,"text_at":[6,-16]})).unwrap();
            h.settle();
        }
        place(&mut h, &[[10., 0.]], [10., 6.]);
        let baseline = h.app.session.doc.to_json();
        type_value(&mut h, prefix);
        if prefix.is_empty() {
            h.step(&json!({"key":"Space","cmd":true})).unwrap();
        }
        assert!(h.widget(expected).is_some(), "parameter completion is available for {prefix:?}: {}", h.call("ui.inspect", json!({})));
        h.step(&json!({"key":"Tab"})).unwrap();
        assert!(crate::dim_view::editing(), "accepting completion does not apply the dimension");
        assert_eq!(h.app.session.doc.to_json(), baseline);
        h.step(&json!({"key":"Enter"})).unwrap();
        assert!(!crate::dim_view::editing());
        assert_eq!(h.app.session.doc.params.iter().rev().find(|p| p.model).unwrap().expr, expected);
        crate::dim_view::cancel();
    }
}

#[test]
fn edge_and_endpoint_creation_commit_once_and_undo_restores_exact_design() {
    for (end, picks, at) in [
        ([20., 0.], vec![[10., 0.]], [10., 5.]),
        ([20., 0.], vec![[0., 0.], [20., 0.]], [10., 5.]),
        ([0., 20.], vec![[0., 0.], [0., 20.]], [-5., 10.]),
        ([12., 16.], vec![[0., 0.], [12., 16.]], [-5., 12.]),
    ] {
        let mut h = line(end);
        let original = h.app.session.doc.to_json();
        let undo = h.app.session.undo.len();
        place(&mut h, &picks, at);
        assert_eq!(h.app.session.doc.to_json(), original);
        assert_eq!(h.app.session.undo.len(), undo);
        type_value(&mut h, "30 mm");
        h.step(&json!({"key":"Enter"})).unwrap();
        assert!(!crate::dim_view::editing());
        assert_eq!(h.app.session.undo.len(), undo + 1);
        let state = h.app.session.world_state();
        let sk = &state.sketch(h.app.session.active_sketch.unwrap()).unwrap().sketch;
        assert!(
            (sk.point(sk.resolve_point("l1.start").unwrap()).unwrap().dist(sk.point(sk.resolve_point("l1.end").unwrap()).unwrap()) - 30.).abs()
                < 1e-6
        );
        assert!(sk.constraints.iter().any(|c| c.param.is_some()));
        let accepted = h.app.session.doc.to_json();
        h.app.run("edit.undo", json!({})).unwrap();
        assert_eq!(h.app.session.doc.to_json(), original);
        h.app.run("edit.redo", json!({})).unwrap();
        assert_eq!(h.app.session.doc.to_json(), accepted);
    }
}
#[test]
fn escape_invalid_correction_and_seed_enter_do_not_create_partial_dimensions() {
    let mut h = line([20., 0.]);
    let original = h.app.session.doc.to_json();
    let undo = h.app.session.undo.len();
    place(&mut h, &[[10., 0.]], [10., 5.]);
    h.step(&json!({"key":"Escape"})).unwrap();
    assert!(!crate::dim_view::editing());
    assert_eq!(h.app.session.doc.to_json(), original);
    assert_eq!(h.app.session.undo.len(), undo);
    place(&mut h, &[[0., 0.], [20., 0.]], [10., 5.]);
    type_value(&mut h, "unknown_length");
    h.step(&json!({"key":"Enter"})).unwrap();
    assert!(crate::dim_view::editing());
    assert_eq!(h.app.session.doc.to_json(), original);
    assert_eq!(h.app.session.undo.len(), undo);
    type_value(&mut h, "25 mm");
    h.step(&json!({"key":"Enter"})).unwrap();
    assert!(!crate::dim_view::editing());
    assert_eq!(h.app.session.undo.len(), undo + 1);
    h.app.run("edit.undo", json!({})).unwrap();
    h.settle();
    place(&mut h, &[[10., 0.]], [10., 5.]);
    h.step(&json!({"key":"Enter"})).unwrap();
    assert_eq!(h.app.session.undo.len(), undo + 1);
    assert!(!crate::dim_view::editing());
}
#[test]
fn switching_documents_cancels_creation_and_existing_parameter_edit_is_separate() {
    let mut h = line([20., 0.]);
    let original = h.app.session.doc.to_json();
    place(&mut h, &[[10., 0.]], [10., 5.]);
    h.call("ui.documents", json!({"action":"new"}));
    assert!(!crate::dim_view::editing());
    h.call("ui.documents", json!({"action":"switch","index":0}));
    assert_eq!(h.app.session.doc.to_json(), original);
    place(&mut h, &[[10., 0.]], [10., 5.]);
    h.step(&json!({"key":"Enter"})).unwrap();
    let undo = h.app.session.undo.len();
    crate::dim_view::edit_param(&h.app, "d1");
    h.settle();
    type_value(&mut h, "24 mm");
    h.step(&json!({"key":"Enter"})).unwrap();
    assert_eq!(h.app.session.undo.len(), undo + 1);
    h.app.run("edit.undo", json!({})).unwrap();
    assert!(h.app.session.doc.param("d1").unwrap().expr.starts_with("20"));
}

#[test]
fn rectangle_side_and_fixed_origin_line_dimensions_preserve_other_constraints() {
    for edge in [false, true] {
        let mut h = line([20., 0.]);
        h.app.run("sketch.constraint.fix", json!({"entity":"l1.start"})).unwrap();
        h.settle();
        let original = h.app.session.doc.to_json();
        let undo = h.app.session.undo.len();
        let picks = if edge { vec![[10., 0.]] } else { vec![[0., 0.], [20., 0.]] };
        place(&mut h, &picks, [10., 5.]);
        type_value(&mut h, "30 mm");
        h.step(&json!({"key":"Enter"})).unwrap();
        let state = h.app.session.world_state();
        let sk = &state.sketch(h.app.session.active_sketch.unwrap()).unwrap().sketch;
        assert!(sk.point(sk.resolve_point("l1.start").unwrap()).unwrap().len() < 1e-8, "fixed origin remains fixed");
        assert!((sk.point(sk.resolve_point("l1.end").unwrap()).unwrap().x - 30.).abs() < 1e-6);
        assert_eq!(h.app.session.undo.len(), undo + 1);
        h.app.run("edit.undo", json!({})).unwrap();
        assert_eq!(h.app.session.doc.to_json(), original);
    }
    for (picks, at) in
        [(vec![[0., 0.], [0., 10.]], [-5., 5.]), (vec![[0., 5.]], [-5., 5.]), (vec![[20., 0.], [20., 10.]], [25., 5.]), (vec![[20., 5.]], [25., 5.])]
    {
        let mut h = Harness::new();
        crate::dim_view::cancel();
        h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
        h.app.run("sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[20,10]})).unwrap();
        h.call("ui.view", json!({"view":"top"}));
        h.app.cam.distance = 80.;
        h.app.cam.target = solidcraft_engine::geom::Vec3::new(10., 5., 0.);
        h.settle();
        let baseline = h.app.session.doc.to_json();
        let undo = h.app.session.undo.len();
        place(&mut h, &picks, at);
        type_value(&mut h, "12 mm");
        h.step(&json!({"key":"Enter"})).unwrap();
        let state = h.app.session.world_state();
        let solved = state.sketch(h.app.session.active_sketch.unwrap()).unwrap();
        // The sketch's first dimension scales it (#1365): 20 × 10 becomes 24 × 12.
        assert!((solved.profiles[0].area - 288.).abs() < 1e-5, "the first dimension scales the rectangle: {}", solved.profiles[0].area);
        assert_eq!(h.app.session.undo.len(), undo + 1);
        h.app.run("edit.undo", json!({})).unwrap();
        assert_eq!(h.app.session.doc.to_json(), baseline);
    }
}

/// With the Sketch preference off, the first dimension sizes only what it measures (#4109).
#[test]
fn the_first_dimension_scales_the_drawing_unless_the_preference_is_off() {
    for (scales, side) in [true, false].into_iter().flat_map(|s| [2., 30., 500.].map(|v| (s, v))) {
        let mut h = Harness::new();
        crate::dim_view::cancel();
        assert!(h.app.preferences.first_dimension_scales, "on by default (#1365)");
        h.app.preferences.first_dimension_scales = scales;
        h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
        h.app.run("sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[20,10]})).unwrap();
        h.call("ui.view", json!({"view":"top"}));
        h.app.cam.distance = 80.;
        h.app.cam.target = solidcraft_engine::geom::Vec3::new(10., 5., 0.);
        h.settle();
        place(&mut h, &[[10., 0.]], [10., -5.]);
        type_value(&mut h, &format!("{side} mm"));
        h.step(&json!({"key":"Enter"})).unwrap();
        let state = h.app.session.world_state();
        let area = state.sketch(h.app.session.active_sketch.unwrap()).unwrap().profiles[0].area;
        // On: 20 × 10 scales to side × side / 2. Off: only the side changes, side × 10.
        let want = if scales { side * side / 2. } else { side * 10. };
        assert!((area - want).abs() < 1e-6 * want.max(1.), "scales: {scales}, side {side}, area {area}");
    }
}

#[test]
fn pending_second_nested_plane_uses_world_picks_but_one_local_dimension() {
    use solidcraft_engine::geom::Vec2;
    crate::dim_view::cancel();
    let mut h = Harness::new();
    let parent = h.app.run("component.create", json!({"name":"Assembly"})).unwrap();
    let parent_component = parent["component"].as_u64().unwrap();
    let first = parent["occurrence"].as_u64().unwrap();
    let child = h.app.run("component.create", json!({"name":"Part"})).unwrap();
    let component = child["component"].as_u64().unwrap();
    let leaf = child["occurrence"].as_u64().unwrap();
    h.app.run("component.activate", json!({"component":0})).unwrap();
    let second = h.app.run("occurrence.copy", json!({"component":parent_component})).unwrap()["occurrence"].as_u64().unwrap();
    h.app.run("occurrence.move", json!({"occurrence":second,"translate":[20,30,40],"axis":[1,0,0],"angle":"35 deg","capture":true})).unwrap();
    h.app.run("component.activate", json!({"component":component,"occurrence":leaf,"path":[second,leaf]})).unwrap();
    h.app.run("browser.visibility", json!({"paths":[[first]],"visible":false})).unwrap();
    let sid = h.app.run("sketch.create", json!({"plane":"XY"})).unwrap()["sketch"].as_u64().unwrap();
    // The 2D producer uses sketch-local coordinates; pointer picks below use placed world coordinates.
    h.app.run("sketch.line", json!({"points":[[0,0],[20,0]]})).unwrap();
    h.app.run("occurrence.move", json!({"occurrence":second,"translate":[25,32,40],"axis":[1,0,0],"angle":"35 deg","capture":false})).unwrap();
    h.call("ui.view", json!({"view":"top"}));
    h.app.cam.distance = 90.;
    let plane = h.app.session.world_state().sketch(sid).unwrap().plane;
    h.app.cam.target = plane.to_world(Vec2::new(10., 0.));
    h.settle();
    let baseline = h.app.session.doc.to_json();
    let undo = h.app.session.undo.len();
    let pending = h.app.session.pending_moves.clone();
    h.step(&json!({"start":"sketch.dimension"})).unwrap();
    for p in [Vec2::ZERO, Vec2::new(20., 0.), Vec2::new(10., 5.)] {
        let w = plane.to_world(p);
        h.step(&json!({"click":{"world":[w.x,w.y,w.z]}})).unwrap();
    }
    assert!(crate::dim_view::editing());
    assert_eq!(h.app.session.doc.to_json(), baseline);
    type_value(&mut h, "30 mm");
    h.step(&json!({"key":"Enter"})).unwrap();
    assert_eq!(h.app.session.undo.len(), undo + 1);
    assert_eq!(h.app.session.active_occurrence_path, Some(vec![second, leaf]));
    assert_eq!(h.app.session.pending_moves, pending);
    let state = h.app.session.model.state();
    let sk = &state.sketch(sid).unwrap().sketch;
    assert!(
        (sk.point(sk.resolve_point("l1.start").unwrap()).unwrap().dist(sk.point(sk.resolve_point("l1.end").unwrap()).unwrap()) - 30.).abs() < 1e-6
    );
    h.app.run("edit.undo", json!({})).unwrap();
    assert_eq!(h.app.session.doc.to_json(), baseline);
    assert_eq!(h.app.session.pending_moves, pending);
    h.app.run("edit.redo", json!({})).unwrap();
    assert_eq!(h.app.session.pending_moves, pending);
    assert_eq!(h.app.session.active_occurrence_path, Some(vec![second, leaf]));
}

#[test]
fn invalid_zero_negative_failed_open_and_click_away_leave_no_dimension() {
    let mut h = line([20., 0.]);
    let baseline = h.app.session.doc.to_json();
    let undo = h.app.session.undo.len();
    for invalid in ["0 mm", "-2 mm"] {
        place(&mut h, &[[10., 0.]], [10., 5.]);
        type_value(&mut h, invalid);
        h.step(&json!({"key":"Enter"})).unwrap();
        assert!(crate::dim_view::editing());
        assert_eq!(h.app.session.doc.to_json(), baseline);
        assert_eq!(h.app.session.undo.len(), undo);
        h.step(&json!({"key":"Escape"})).unwrap();
    }
    place(&mut h, &[[10., 0.]], [10., 5.]);
    assert!(h.app.run("doc.open", json!({"path":"missing-dimension-draft-design.solidcraft"})).is_err());
    assert!(crate::dim_view::editing(), "failed replacement must preserve the draft");
    assert_eq!(h.app.session.doc.to_json(), baseline);
    h.frames(8);
    h.step(&json!({"click":{"world":[12,-10,0]}})).unwrap();
    assert!(!crate::dim_view::editing());
    assert_eq!(h.app.session.doc.to_json(), baseline);
    assert_eq!(h.app.session.undo.len(), undo);
}

#[test]
fn dimension_to_fillet_virtual_sharp_keeps_the_arc_and_fixed_origin() {
    crate::dim_view::cancel();
    let mut h = Harness::new();
    h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    h.app.run("sketch.line", json!({"points":[[0,0],[0,20],[20,20]],"ids":["riser","top"]})).unwrap();
    h.app.run("sketch.constraint.fix", json!({"entity":"riser.start"})).unwrap();
    h.app.run("sketch.constraint.horizontal_vertical", json!({"line":"riser","mode":"vertical"})).unwrap();
    h.app.run("sketch.constraint.horizontal_vertical", json!({"line":"top","mode":"horizontal"})).unwrap();
    // An existing width requires the fillet's retained virtual sharp. Add an independent
    // height through the visible corner while preserving that width and the fillet arc.
    h.app.run("sketch.dimension", json!({"entities":["top"],"value":20})).unwrap();
    let fillet = h.app.run("sketch.fillet", json!({"point":"riser.end","radius":5})).unwrap();
    let corner = fillet["virtual_corners"][0].as_str().unwrap().to_string();
    h.call("ui.view", json!({"view":"top"}));
    h.app.cam.distance = 90.;
    h.app.cam.target = solidcraft_engine::geom::Vec3::new(5., 10., 0.);
    h.settle();
    let baseline = h.app.session.doc.to_json();
    let undo = h.app.session.undo.len();
    place(&mut h, &[[0., 0.], [0., 20.]], [-5., 10.]);
    type_value(&mut h, "30 mm");
    h.step(&json!({"key":"Enter"})).unwrap();
    assert!(!crate::dim_view::editing());
    assert_eq!(h.app.session.undo.len(), undo + 1);
    let state = h.app.session.world_state();
    let solved = state.sketch(h.app.session.active_sketch.unwrap()).unwrap();
    assert!(solved.report.ok());
    let sk = &solved.sketch;
    assert!((sk.point(sk.point_index(&corner).unwrap()).unwrap().y - 30.).abs() < 1e-6);
    assert!(sk.point(sk.resolve_point("riser.start").unwrap()).unwrap().len() < 1e-8);
    assert!(sk.curves.iter().any(|c| matches!(c.kind, solidcraft_engine::sketch::CurveKind::Arc { .. })), "the dimension retains the fillet arc");
    h.app.run("edit.undo", json!({})).unwrap();
    assert_eq!(h.app.session.doc.to_json(), baseline);
}

#[test]
fn draft_annotation_does_not_move_committed_dimension_hitboxes() {
    crate::dim_view::cancel();
    let mut h = Harness::new();
    h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    h.app.run("sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[20,10]})).unwrap();
    let width = h.app.run("sketch.dimension", json!({"entities":["l3"],"value":20})).unwrap()["param"].as_str().unwrap().to_string();
    h.call("ui.view", json!({"view":"top"}));
    h.app.cam.distance = 80.;
    h.app.cam.target = solidcraft_engine::geom::Vec3::new(10., 5., 0.);
    h.settle();
    let at = crate::dim_view::text_at(&width).unwrap();
    let drawn = crate::dim_view::drawn();
    let baseline = h.app.session.doc.to_json();
    place(&mut h, &[[0., 0.], [0., 10.]], [-5., 5.]);
    type_value(&mut h, "12 mm");
    h.settle();
    assert_eq!(crate::dim_view::text_at(&width), Some(at));
    assert_eq!(crate::dim_view::drawn(), drawn, "draft annotations must not become committed hit targets");
    assert_eq!(h.app.session.doc.to_json(), baseline);
    h.step(&json!({"key":"Escape"})).unwrap();
    assert_eq!(crate::dim_view::drawn(), drawn);
}

#[test]
fn inch_dimension_entry_survives_unit_changes_undo_and_reopen() {
    let mut h = line([25.4, 0.]);
    h.app.run("document.units", json!({"units":"in"})).unwrap();
    place(&mut h, &[[12.7, 0.]], [12.7, 8.]);
    type_value(&mut h, "1.2345");
    h.step(&json!({"key":"Enter"})).unwrap();
    let sketch = h.app.session.active_sketch.unwrap();
    h.settle();
    crate::dim_view::edit_param(&h.app, "d1");
    h.settle();
    type_value(&mut h, "1.3456");
    h.step(&json!({"key":"Enter"})).unwrap();
    let edited = h.app.session.world_state();
    let edited = &edited.sketch(sketch).unwrap().sketch;
    assert!(edited.constraints.iter().any(|c| c.kind.value().is_some_and(|v| (v - 1.3456 * 25.4).abs() < 1e-6)));
    h.app.run("edit.undo", json!({})).unwrap();
    let before = h.app.session.world_state().sketch(sketch).unwrap().sketch.clone();
    let c = before.constraints.iter().find(|c| c.param.is_some()).unwrap();
    let p = h.app.session.doc.param(c.param.as_deref().unwrap()).unwrap();
    assert_eq!(crate::dim_view::label(&c.kind, Some(&p.expr), false, "in", 2), "1.2345 in");
    assert!((c.kind.value().unwrap() - 31.3563).abs() < 1e-6);
    h.app.run("document.units", json!({"units":"mm"})).unwrap();
    assert_eq!(h.app.session.world_state().sketch(sketch).unwrap().sketch, before);
    h.app.run("edit.undo", json!({})).unwrap();
    assert_eq!(h.app.session.doc.units, "in");
    let doc = solidcraft_engine::doc::Document::from_json(&h.app.session.doc.to_json()).unwrap();
    let reopened = solidcraft_engine::Session::new(doc);
    assert_eq!(reopened.doc.units, "in");
    assert_eq!(reopened.world_state().sketch(sketch).unwrap().sketch, before);
    crate::dim_view::cancel();
}

/// Placing the dimension of two unaligned points above them gives the horizontal distance,
/// beside them the vertical one, and across from them the distance between them (#257).
#[test]
fn placing_a_two_point_dimension_picks_horizontal_vertical_or_aligned() {
    // What each placement fixes at 10 mm: the X span, the Y span, or the length.
    let spans = |a: solidcraft_engine::geom::Vec2, b: solidcraft_engine::geom::Vec2| [(b.x - a.x).abs(), (b.y - a.y).abs(), a.dist(b)];
    for (at, measured) in [([6., 25.], 0), ([-10., 1.], 1), ([-5., 12.], 2)] {
        let mut h = line([12., 16.]);
        place(&mut h, &[[0., 0.], [12., 16.]], at);
        type_value(&mut h, "10 mm");
        h.step(&json!({"key":"Enter"})).unwrap();
        assert!(!crate::dim_view::editing());
        let state = h.app.session.world_state();
        let sk = &state.sketch(h.app.session.active_sketch.unwrap()).unwrap().sketch;
        let a = sk.point(sk.resolve_point("l1.start").unwrap()).unwrap();
        let b = sk.point(sk.resolve_point("l1.end").unwrap()).unwrap();
        let spans = spans(a, b);
        assert!((spans[measured] - 10.0).abs() < 1e-6, "placed at {at:?}: {a:?} {b:?}");
        // The sketch's first dimension scales it (#1365): the other span keeps its proportion.
        if measured < 2 {
            let drawn = [12.0, 16.0];
            let other = drawn[1 - measured] * 10.0 / drawn[measured];
            assert!((spans[1 - measured] - other).abs() < 1e-6, "placed at {at:?}: {a:?} {b:?}");
        }
    }
}

/// A dimension that would over-constrain the sketch is placed as a driven (reference) one
/// instead of failing: added with no parameter or value editor, and removed again by one
/// undo (#320).
#[test]
fn an_over_constraining_dimension_is_placed_as_a_driven_one() {
    let mut h = line([20., 0.]);
    h.app.run("sketch.dimension", json!({"entities":["l1"],"value":"20 mm"})).unwrap();
    h.settle();
    let original = h.app.session.doc.to_json();
    let undo = h.app.session.undo.len();
    let params = h.app.session.doc.params.len();
    h.step(&json!({"start":"sketch.dimension"})).unwrap();
    h.step(&json!({"click":{"world":[10,0,0]}})).unwrap();
    h.step(&json!({"move":{"world":[10,6,0]}})).unwrap();
    h.settle();
    assert_eq!(crate::dim_view::drawn().len(), 1, "the preview is not a selectable dimension");
    h.step(&json!({"click":{"world":[10,6,0]}})).unwrap();
    assert!(!crate::dim_view::editing(), "a driven dimension has no value to edit");
    assert_eq!(h.app.session.undo.len(), undo + 1);
    assert_eq!(h.app.session.doc.params.len(), params, "a driven dimension adds no parameter");
    let state = h.app.session.world_state();
    let solved = state.sketch(h.app.session.active_sketch.unwrap()).unwrap();
    assert!(solved.report.ok());
    let dims: Vec<_> = solved.sketch.constraints.iter().filter(|c| c.kind.is_dimension()).collect();
    assert_eq!(dims.len(), 2);
    assert_eq!(dims.iter().filter(|c| c.driven && c.param.is_none()).count(), 1, "{dims:?}");
    h.app.run("edit.undo", json!({})).unwrap();
    assert_eq!(h.app.session.doc.to_json(), original);
}

// ---- Type-first: D, click, type, Enter ----

/// A sketch with `setup` drawn, the Top view centred on `centre`, and the Dimension tool on.
fn sketch_with(setup: &[(&str, serde_json::Value)], centre: [f64; 2]) -> Harness {
    crate::dim_view::cancel();
    let mut h = Harness::new();
    h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    for (cmd, params) in setup {
        h.app.run(cmd, params.clone()).unwrap();
    }
    h.call("ui.view", json!({"view":"top"}));
    h.app.cam.distance = 80.0;
    h.app.cam.target = solidcraft_engine::geom::Vec3::new(centre[0], centre[1], 0.0);
    h.settle();
    h.step(&json!({"start":"sketch.dimension"})).unwrap();
    h
}

/// Type as a keyboard does: each character is a key press with its text in the same frame,
/// so a letter that is also a tool shortcut reaches the shortcuts too.
fn keys(h: &mut Harness, text: &str) {
    for ch in text.chars() {
        let key = egui::Key::from_name(&ch.to_ascii_uppercase().to_string());
        let modifiers = egui::Modifiers::default();
        if let Some(key) = key {
            h.app.synthetic.push(egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers });
        }
        h.app.synthetic.push(egui::Event::Text(ch.to_string()));
        if let Some(key) = key {
            h.app.synthetic.push(egui::Event::Key { key, physical_key: None, pressed: false, repeat: false, modifiers });
        }
        h.frames(1);
    }
    h.settle();
}

fn solved(h: &Harness) -> solidcraft_engine::sketch::Sketch {
    let state = h.app.session.world_state();
    state.sketch(h.app.session.active_sketch.unwrap()).unwrap().sketch.clone()
}

/// The dimensions added since `before` (constraints with a parameter or a value).
fn added(h: &Harness, before: usize) -> Vec<solidcraft_engine::sketch::Constraint> {
    solved(h).constraints.into_iter().filter(|c| c.kind.is_dimension()).skip(before).collect()
}

fn dimension_count(h: &Harness) -> usize {
    solved(h).constraints.iter().filter(|c| c.kind.is_dimension()).count()
}

/// The value took the keyboard straight after the pick that determined the dimension.
fn assert_live(h: &Harness) {
    assert!(crate::dim_view::editing(), "the pick opens the value at once");
    assert_eq!(crate::dim_view::live(), Some(false));
    assert_eq!(h.ctx.memory(|m| m.focused()), Some(egui::Id::new("sc_dim_edit")));
}

#[test]
fn type_first_line_circle_arc_distance_and_angle_commit_one_undo_step_each() {
    use solidcraft_engine::sketch::ConstraintKind as K;
    let line = [("sketch.line", json!({"points":[[0,0],[20,0]],"infer":false}))];
    let circle = [("sketch.circle.center", json!({"center":[10,0],"radius":5}))];
    let arc = [("sketch.arc.center_point", json!({"center":[10,0],"start":[18,0],"sweep":90}))];
    // Two lines joined at a fixed corner: the angle turns one about it.
    let corner = [
        ("sketch.line", json!({"points":[[0,20],[0,0],[20,0]],"infer":false})),
        ("sketch.constraint.fix", json!({"entity":"l1.end"})),
        ("sketch.constraint.horizontal_vertical", json!({"line":"l2","mode":"horizontal"})),
    ];
    let half = std::f64::consts::FRAC_1_SQRT_2 * 8.0;
    type Check = fn(&K) -> bool;
    let cases: [(&[(&str, serde_json::Value)], [f64; 2], Vec<[f64; 2]>, &str, Check); 5] = [
        (&line, [10., 0.], vec![[10., 0.]], "25", |k| matches!(k, K::Length { value, .. } if (value - 25.).abs() < 1e-6)),
        (&circle, [10., 0.], vec![[15., 0.]], "12", |k| matches!(k, K::Diameter { value, .. } if (value - 12.).abs() < 1e-6)),
        (&arc, [10., 0.], vec![[10. + half, half]], "6", |k| matches!(k, K::Radius { value, .. } if (value - 6.).abs() < 1e-6)),
        (&line, [10., 0.], vec![[0., 0.], [20., 0.]], "25", |k| k.value().is_some_and(|v| (v - 25.).abs() < 1e-6)),
        (&corner, [8., 8.], vec![[12., 0.], [0., 12.]], "60", |k| k.is_angle() && k.value().is_some_and(|v| (v.to_degrees() - 60.).abs() < 1e-6)),
    ];
    for (setup, centre, picks, typed, check) in cases {
        let mut h = sketch_with(setup, centre);
        let original = h.app.session.doc.to_json();
        let undo = h.app.session.undo.len();
        let dims = dimension_count(&h);
        for p in &picks {
            h.step(&json!({"click":{"world":[p[0],p[1],0]}})).unwrap();
        }
        assert_live(&h);
        assert_eq!(h.app.session.doc.to_json(), original, "nothing changes until Enter");
        // The text follows the pointer while typing: Enter places it there.
        // (For the angle, inside the wedge both before and after it changes.)
        let cursor = if check(&solidcraft_engine::sketch::ConstraintKind::Angle { a: 0, b: 1, value: 60f64.to_radians(), flip: false }) {
            [10.4, 6.0]
        } else {
            [centre[0] + 3.0, centre[1] + 9.0]
        };
        h.step(&json!({"move":{"world":[cursor[0],cursor[1],0]}})).unwrap();
        keys(&mut h, typed);
        h.step(&json!({"key":"Enter"})).unwrap();
        assert!(!crate::dim_view::editing(), "{picks:?}");
        assert_eq!(h.app.session.undo.len(), undo + 1, "{picks:?}");
        let new = added(&h, dims);
        assert_eq!(new.len(), 1, "{picks:?}: {new:?}");
        assert!(check(&new[0].kind), "{picks:?}: {:?}", new[0].kind);
        h.settle();
        let shown = crate::dim_view::text_at(&new[0].id).expect("the new dimension is drawn");
        let [x, y] = h.at(&json!({"world":[cursor[0],cursor[1],0]})).unwrap();
        assert!(shown.distance(egui::pos2(x as f32, y as f32)) < 40.0, "{picks:?}: text at {shown:?}, cursor at {x},{y}");
        assert_eq!(h.app.tool.as_ref().map(|t| (t.cmd, t.picks.len())), Some(("sketch.dimension", 0)), "ready for the next dimension");
        h.app.run("edit.undo", json!({})).unwrap();
        assert_eq!(h.app.session.doc.to_json(), original);
    }
    crate::dim_view::cancel();
}

#[test]
fn type_first_without_moving_uses_the_default_place_and_expressions_work() {
    let mut h = sketch_with(&[("sketch.line", json!({"points":[[0,0],[20,0]],"infer":false}))], [10., 0.]);
    h.app.run("parameters.add", json!({"name":"width","expression":"15 mm"})).unwrap();
    h.settle();
    let dims = dimension_count(&h);
    h.step(&json!({"click":{"world":[10,0,0]}})).unwrap();
    assert_live(&h);
    keys(&mut h, "2*width");
    h.step(&json!({"key":"Enter"})).unwrap();
    let new = added(&h, dims);
    assert_eq!(new.len(), 1);
    assert!(new[0].text.is_none(), "an unmoved pointer leaves the text in its default place: {:?}", new[0].text);
    assert!((new[0].kind.value().unwrap() - 30.).abs() < 1e-6);
    assert_eq!(h.app.session.doc.param(new[0].param.as_deref().unwrap()).unwrap().expr, "2*width");
    crate::dim_view::cancel();
}

/// Letters typed into the value are not tool shortcuts (L Line, D Dimension, C Circle…).
#[test]
fn type_first_parameter_names_are_not_taken_by_shortcuts() {
    for name in ["length", "depth", "clearance", "rim"] {
        let mut h = sketch_with(&[("sketch.line", json!({"points":[[0,0],[20,0]],"infer":false}))], [10., 0.]);
        h.app.run("parameters.add", json!({"name":name,"expression":"22 mm"})).unwrap();
        h.settle();
        let dims = dimension_count(&h);
        h.step(&json!({"click":{"world":[10,0,0]}})).unwrap();
        assert_live(&h);
        keys(&mut h, name);
        assert_eq!(h.app.tool.as_ref().map(|t| t.cmd), Some("sketch.dimension"), "{name}: a letter started another tool");
        assert!(h.app.dialog.is_none(), "{name}");
        h.step(&json!({"key":"Enter"})).unwrap();
        let new = added(&h, dims);
        assert_eq!(new.len(), 1, "{name}");
        assert!((new[0].kind.value().unwrap() - 22.).abs() < 1e-6, "{name}");
        assert_eq!(h.app.session.doc.param(new[0].param.as_deref().unwrap()).unwrap().expr, name);
        crate::dim_view::cancel();
    }
}

#[test]
fn type_first_escape_cancels_with_no_change() {
    let mut h = sketch_with(&[("sketch.line", json!({"points":[[0,0],[20,0]],"infer":false}))], [10., 0.]);
    let original = h.app.session.doc.to_json();
    let undo = h.app.session.undo.len();
    for typed in ["", "25"] {
        h.step(&json!({"click":{"world":[10,0,0]}})).unwrap();
        assert_live(&h);
        keys(&mut h, typed);
        h.step(&json!({"key":"Escape"})).unwrap();
        assert!(!crate::dim_view::editing());
        assert_eq!(h.app.session.doc.to_json(), original);
        assert_eq!(h.app.session.undo.len(), undo);
        assert_eq!(h.app.tool.as_ref().map(|t| (t.cmd, t.picks.len())), Some(("sketch.dimension", 0)), "Esc drops the picks too");
    }
    crate::dim_view::cancel();
}

/// Place first still works: a click before typing puts the text there and keeps the value
/// (selected, so typing replaces it); Tab leaves the picks for a click to place, as before.
#[test]
fn type_first_keeps_the_place_first_flow() {
    let mut h = sketch_with(&[("sketch.line", json!({"points":[[0,0],[20,0]],"infer":false}))], [10., 0.]);
    let dims = dimension_count(&h);
    h.step(&json!({"click":{"world":[10,0,0]}})).unwrap();
    assert_live(&h);
    h.step(&json!({"click":{"world":[10,-7,0]}})).unwrap();
    assert!(crate::dim_view::editing());
    assert_eq!(crate::dim_view::live(), None, "the text is pinned");
    assert_eq!(h.ctx.memory(|m| m.focused()), Some(egui::Id::new("sc_dim_edit")));
    h.step(&json!({"move":{"world":[0,12,0]}})).unwrap();
    keys(&mut h, "18");
    h.step(&json!({"key":"Enter"})).unwrap();
    let new = added(&h, dims);
    assert_eq!(new.len(), 1);
    assert!((new[0].kind.value().unwrap() - 18.).abs() < 1e-6);
    h.settle();
    let shown = crate::dim_view::text_at(&new[0].id).unwrap();
    let [x, y] = h.at(&json!({"world":[10,-7,0]})).unwrap();
    assert!(shown.distance(egui::pos2(x as f32, y as f32)) < 40.0, "the pinned place wins over the pointer: {shown:?} vs {x},{y}");
    // Tab before typing hands the picks back to the place-first flow.
    h.app.run("edit.undo", json!({})).unwrap();
    h.settle();
    h.step(&json!({"click":{"world":[10,0,0]}})).unwrap();
    assert_live(&h);
    h.frames(4);
    h.step(&json!({"key":"Tab"})).unwrap();
    h.settle();
    assert!(!crate::dim_view::editing());
    assert_eq!(h.app.tool.as_ref().map(|t| t.picks.len()), Some(1));
    h.step(&json!({"click":{"world":[10,6,0]}})).unwrap();
    assert!(crate::dim_view::editing());
    assert_eq!(h.ctx.memory(|m| m.focused()), Some(egui::Id::new("sc_dim_edit")));
    keys(&mut h, "21");
    h.step(&json!({"key":"Enter"})).unwrap();
    let new = added(&h, dims);
    assert!((new[0].kind.value().unwrap() - 21.).abs() < 1e-6);
    crate::dim_view::cancel();
}

/// Type-first, the text follows the pointer: moving it from across the picks (the distance
/// between them) to above or beside them switches to the horizontal or vertical distance and
/// back. Until something is typed the value follows the dimension, so the preview measures the
/// picked points where they are (it neither moves nor scales them) and Enter keeps them there.
#[test]
fn type_first_switching_between_aligned_horizontal_and_vertical_keeps_the_picked_points() {
    use solidcraft_engine::sketch::ConstraintKind as K;
    let end = [12., 16.];
    let mid = [6., 8.];
    // Across from the picks, above them, beside them.
    let (across, above, beside) = ([-5., 12.], [6., 25.], [-10., 1.]);
    type Kind = fn(&K) -> bool;
    let aligned: Kind = |k| matches!(k, K::Distance { .. } | K::Length { .. });
    let horizontal: Kind = |k| matches!(k, K::DistanceX { .. });
    let vertical: Kind = |k| matches!(k, K::DistanceY { .. });
    let picks: [Vec<[f64; 2]>; 2] = [vec![[0., 0.], end], vec![mid]];
    for picks in picks {
        for (commit_at, commit_kind, commit_value) in [(above, horizontal, 12.), (beside, vertical, 16.), (across, aligned, 20.)] {
            let mut h = sketch_with(&[("sketch.line", json!({"points":[[0,0],end],"infer":false}))], mid);
            let undo = h.app.session.undo.len();
            for p in &picks {
                h.step(&json!({"click":{"world":[p[0],p[1],0]}})).unwrap();
            }
            assert_live(&h);
            for (at, kind, value) in [
                (across, aligned, 20.),
                (above, horizontal, 12.),
                (across, aligned, 20.),
                (beside, vertical, 16.),
                (commit_at, commit_kind, commit_value),
            ] {
                h.step(&json!({"move":{"world":[at[0],at[1],0]}})).unwrap();
                h.settle();
                let (expr, c, sk) = crate::dim_view::draft_preview().expect("the draft is open");
                let ctx = format!("{picks:?} at {at:?}: {expr} {:?}", c.kind);
                assert!(kind(&c.kind), "{ctx}");
                assert!((c.kind.value().unwrap() - value).abs() < 1e-6, "{ctx}");
                assert_eq!(expr, format!("{value} mm"), "{ctx}");
                let a = sk.point(sk.resolve_point("l1.start").unwrap()).unwrap();
                let b = sk.point(sk.resolve_point("l1.end").unwrap()).unwrap();
                assert!(
                    a.dist(solidcraft_engine::geom::Vec2::new(0., 0.)) < 1e-6 && b.dist(solidcraft_engine::geom::Vec2::new(end[0], end[1])) < 1e-6,
                    "{ctx}: the preview moved the points to {a:?} {b:?}"
                );
            }
            h.step(&json!({"key":"Enter"})).unwrap();
            assert!(!crate::dim_view::editing());
            assert_eq!(h.app.session.undo.len(), undo + 1);
            let sk = solved(&h);
            let a = sk.point(sk.resolve_point("l1.start").unwrap()).unwrap();
            let b = sk.point(sk.resolve_point("l1.end").unwrap()).unwrap();
            assert!(
                a.dist(solidcraft_engine::geom::Vec2::new(0., 0.)) < 1e-6 && b.dist(solidcraft_engine::geom::Vec2::new(end[0], end[1])) < 1e-6,
                "{picks:?} at {commit_at:?}: {a:?} {b:?}"
            );
            let new = added(&h, 0);
            assert_eq!(new.len(), 1);
            assert!(commit_kind(&new[0].kind), "{picks:?} at {commit_at:?}: {:?}", new[0].kind);
            assert!((new[0].kind.value().unwrap() - commit_value).abs() < 1e-6, "{:?}", new[0].kind);
            // The committed dimension's text is where the pointer left it, and its extension lines
            // start at the picked points.
            h.settle();
            let shown = crate::dim_view::text_at(&new[0].id).expect("the new dimension is drawn");
            let [x, y] = h.at(&json!({"world":[commit_at[0],commit_at[1],0]})).unwrap();
            assert!(shown.distance(egui::pos2(x as f32, y as f32)) < 40.0, "{picks:?}: text at {shown:?}, cursor at {x},{y}");
            crate::dim_view::cancel();
        }
    }
}

#[test]
fn midpoint_then_edge_creates_a_distance_in_one_undo_step() {
    use solidcraft_engine::sketch::ConstraintKind;
    let mut h = line([20., 0.]);
    h.app.run("sketch.line", json!({"points":[[-10,-10],[-10,20]]})).unwrap();
    h.app.run("sketch.constraint.fix", json!({"entity":"l2"})).unwrap();
    h.settle();
    let baseline = h.app.session.doc.to_json();
    let undo = h.app.session.undo.len();
    for accept in [false, true] {
        place(&mut h, &[[10., 0.], [-10., -5.]], [0., -8.]);
        let (_, c, sk) = crate::dim_view::draft_preview().unwrap();
        let ConstraintKind::PointLineDistance { p, l, .. } = c.kind else { panic!("MID to edge must be a distance: {:?}", c.kind) };
        assert_eq!(sk.curves[l].id, "l2");
        assert!(sk.constraints.iter().any(|c| matches!(c.kind, ConstraintKind::Midpoint { p: q, l: 0 } if q == p)));
        assert_eq!(h.app.session.doc.to_json(), baseline, "preview creates no persistent helper");
        if !accept {
            h.step(&json!({"key":"Escape"})).unwrap();
            assert!(h.app.tool.as_ref().unwrap().point_picks.is_empty());
            continue;
        }
        type_value(&mut h, "25 mm");
        h.step(&json!({"key":"Enter"})).unwrap();
        assert!(!crate::dim_view::editing());
        assert_eq!(h.app.session.undo.len(), undo + 1);
        let state = h.app.session.world_state();
        let solved = state.sketch(h.app.session.active_sketch.unwrap()).unwrap();
        assert!(solved.report.ok());
        let sk = &solved.sketch;
        let c = sk.constraints.iter().find(|c| c.param.is_some()).unwrap();
        assert!(matches!(c.kind, ConstraintKind::PointLineDistance { .. }));
        assert!((solidcraft_engine::sketch::measure_dimension(sk, &c.kind).unwrap() - 25.).abs() < 1e-6);
        let accepted = h.app.session.doc.to_json();
        h.app.run("edit.undo", json!({})).unwrap();
        assert_eq!(h.app.session.doc.to_json(), baseline);
        h.app.run("edit.redo", json!({})).unwrap();
        assert_eq!(h.app.session.doc.to_json(), accepted);
    }
    crate::dim_view::cancel();
}

/// The short sketch edge overlaps a face's automatically projected boundary.
fn midpoint_face_sketch() -> Harness {
    use solidcraft_engine::geom::Vec3;
    crate::dim_view::cancel();
    let mut h = Harness::new();
    h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    h.app.run("sketch.rectangle.two_point", json!({"p0":[-10,-5],"p1":[10,15]})).unwrap();
    h.app.finish_sketch();
    h.app.run("solid.extrude", json!({"distance":10})).unwrap();
    h.app.run("sketch.create", json!({"plane":{"face":[0,5,10]}})).unwrap();
    let state = h.app.session.world_state();
    let ss = state.sketch(h.app.session.active_sketch.unwrap()).unwrap();
    let plane = ss.plane;
    assert!(ss.sketch.curves.iter().any(|c| c.link.is_some()));
    for ends in [[[-8., -5.], [2., -5.]], [[3., 2.], [7., 6.]]] {
        let points: Vec<_> = ends
            .into_iter()
            .map(|p| {
                let q = plane.to_local(Vec3::new(p[0], p[1], 10.));
                [q.x, q.y]
            })
            .collect();
        h.app.run("sketch.line", json!({"points":points,"infer":false})).unwrap();
    }
    h.call("ui.view", json!({"view":"top"}));
    h.app.cam.distance = 90.;
    h.app.cam.target = Vec3::new(0., 5., 10.);
    h.settle();
    h
}

#[test]
fn overlapping_sketch_midpoint_to_model_edge_dimensions_the_snapped_point() {
    use solidcraft_engine::sketch::{ConstraintKind, Shape};
    let mut h = midpoint_face_sketch();
    let baseline = h.app.session.doc.to_json();
    let undo = h.app.session.undo.len();
    h.step(&json!({"start":"sketch.dimension"})).unwrap();
    for p in [[-3., -5.], [10., 2.], [4., -10.]] {
        h.step(&json!({"click":{"world":[p[0],p[1],10.]}})).unwrap();
    }
    let (_, c, sk) = crate::dim_view::draft_preview().expect("MID to projected model edge has a draft");
    let ConstraintKind::PointLineDistance { p, l, .. } = c.kind else { panic!("expected point/edge distance: {:?}", c.kind) };
    assert!(sk.curves[l].link.is_some());
    let source = sk
        .constraints
        .iter()
        .find_map(|c| match c.kind {
            ConstraintKind::Midpoint { p: q, l } if p == q => Some(l),
            _ => None,
        })
        .unwrap();
    assert!(sk.curves[source].link.is_none(), "the short sketch edge won the overlapping pick");
    let Shape::Line { a, b } = sk.shape(source).unwrap() else { panic!() };
    assert!((a.dist(b) - 10.).abs() < 1e-6);
    assert!((solidcraft_engine::sketch::measure_dimension(&sk, &c.kind).unwrap() - 13.).abs() < 1e-6);
    assert_eq!(h.app.session.doc.to_json(), baseline);
    type_value(&mut h, "12 mm");
    h.step(&json!({"key":"Enter"})).unwrap();
    assert!(!crate::dim_view::editing());
    assert_eq!(h.app.session.undo.len(), undo + 1);
    h.app.run("edit.undo", json!({})).unwrap();
    assert_eq!(h.app.session.doc.to_json(), baseline);
    crate::dim_view::cancel();
}

#[test]
fn midpoint_pairs_dimension_sketch_and_projected_model_edges() {
    use solidcraft_engine::sketch::ConstraintKind;
    for (first, second, projected) in [
        ([-3_f64, -5.], [5., 4.], [false, false]),
        ([-3., -5.], [10., 5.], [false, true]),
        ([10., 5.], [-3., -5.], [true, false]),
        ([0., -5.], [10., 5.], [true, true]),
    ] {
        let mut h = midpoint_face_sketch();
        let baseline = h.app.session.doc.to_json();
        let undo = h.app.session.undo.len();
        h.step(&json!({"start":"sketch.dimension"})).unwrap();
        for p in [first, second] {
            h.step(&json!({"click":{"world":[p[0],p[1],10.]}})).unwrap();
        }
        let at = [(first[0] + second[0]) * 0.5, (first[1] + second[1]) * 0.5];
        assert_eq!(h.app.session.doc.to_json(), baseline);
        h.step(&json!({"click":{"world":[at[0],at[1],10.]}})).unwrap();
        if crate::dim_view::editing() {
            h.step(&json!({"key":"Enter"})).unwrap();
        }
        let state = h.app.session.world_state();
        let sk = &state.sketch(h.app.session.active_sketch.unwrap()).unwrap().sketch;
        let c = sk.constraints.iter().rev().find(|c| c.kind.is_dimension()).expect("two MID picks commit a distance");
        let ConstraintKind::Distance { p, q, .. } = c.kind else { panic!("expected midpoint distance for {projected:?}: {:?}", c.kind) };
        for (point, expected) in [(p, projected[0]), (q, projected[1])] {
            let l = sk
                .constraints
                .iter()
                .find_map(|c| match c.kind {
                    ConstraintKind::Midpoint { p, l } if p == point => Some(l),
                    _ => None,
                })
                .expect("each pick remains a midpoint");
            assert_eq!(sk.curves[l].link.is_some(), expected);
        }
        let measured = ((first[0] - second[0]).powi(2) + (first[1] - second[1]).powi(2)).sqrt();
        assert!((solidcraft_engine::sketch::measure_dimension(sk, &c.kind).unwrap() - measured).abs() < 1e-6);
        assert_eq!(h.app.session.undo.len(), undo + 1);
        h.app.run("edit.undo", json!({})).unwrap();
        assert_eq!(h.app.session.doc.to_json(), baseline);
        crate::dim_view::cancel();
    }
}

#[test]
fn midpoint_selection_keeps_line_circle_gap_dimensions_in_both_orders() {
    use solidcraft_engine::sketch::ConstraintKind;
    for reverse in [false, true] {
        let mut h = line([20., 0.]);
        h.app.run("sketch.circle.center", json!({"center":[10,10],"radius":3})).unwrap();
        h.settle();
        let baseline = h.app.session.doc.to_json();
        let picks = if reverse { [[13., 10.], [10., 0.]] } else { [[10., 0.], [13., 10.]] };
        place(&mut h, &picks, [25., 5.]);
        let (_, c, sk) = crate::dim_view::draft_preview().expect("the edge-to-edge gap remains available");
        assert!(matches!(c.kind, ConstraintKind::LineCircleDistance { value, .. } if (value - 7.).abs() < 1e-6));
        assert!(!sk.constraints.iter().any(|c| matches!(c.kind, ConstraintKind::Midpoint { .. })));
        h.step(&json!({"key":"Escape"})).unwrap();
        assert_eq!(h.app.session.doc.to_json(), baseline);
        crate::dim_view::cancel();
    }
}

#[test]
fn midpoint_dimension_orientation_follows_the_pointer_before_placement() {
    use solidcraft_engine::sketch::ConstraintKind;
    for (first, second) in [([-3_f64, -5.], [5., 4.]), ([-3., -5.], [10., 5.]), ([10., 5.], [-3., -5.]), ([0., -5.], [10., 5.])] {
        for (horizontal, held_y, reused) in
            [(true, false, false), (false, false, false), (true, true, false), (false, true, false), (true, false, true), (false, false, true)]
        {
            if held_y && (first != [-3., -5.] || second != [10., 5.]) {
                continue;
            }
            let mut h = midpoint_face_sketch();
            if held_y {
                let state = h.app.session.world_state();
                let ss = state.sketch(h.app.session.active_sketch.unwrap()).unwrap();
                let short = ss.sketch.curves.iter().find(|c| c.link.is_none()).unwrap().id.clone();
                let bottom = ss
                    .sketch
                    .curves
                    .iter()
                    .enumerate()
                    .find_map(|(i, c)| {
                        c.link.as_ref()?;
                        let solidcraft_engine::sketch::Shape::Line { a, b } = ss.sketch.shape(i)? else {
                            return None;
                        };
                        ((ss.plane.to_world(a).y + 5.).abs() < 1e-6 && (ss.plane.to_world(b).y + 5.).abs() < 1e-6).then(|| c.id.clone())
                    })
                    .unwrap();
                for end in ["start", "end"] {
                    h.app.run("sketch.constraint.coincident", json!({"a":format!("{short}.{end}"),"b":bottom})).unwrap();
                }
                h.settle();
            }
            if reused {
                let state = h.app.session.world_state();
                let ss = state.sketch(h.app.session.active_sketch.unwrap()).unwrap();
                let mid_ref = |point: [f64; 2]| {
                    ss.sketch
                        .curves
                        .iter()
                        .enumerate()
                        .find_map(|(i, c)| {
                            let solidcraft_engine::sketch::Shape::Line { a, b } = ss.sketch.shape(i)? else {
                                return None;
                            };
                            let at = ss.plane.to_world((a + b) * 0.5);
                            ((at.x - point[0]).abs() < 1e-6 && (at.y - point[1]).abs() < 1e-6).then(|| format!("mid:{}", c.id))
                        })
                        .unwrap()
                };
                h.app.run("sketch.dimension", json!({"entities":[mid_ref(first),mid_ref(second)],"driven":true})).unwrap();
                h.settle();
            }
            let baseline = h.app.session.doc.to_json();
            let undo = h.app.session.undo.len();
            h.step(&json!({"start":"sketch.dimension"})).unwrap();
            for p in [first, second] {
                h.step(&json!({"click":{"world":[p[0],p[1],10.]}})).unwrap();
            }
            assert!(!crate::dim_view::editing(), "choose the midpoint span's orientation before entering a value");
            let above = [(first[0] + second[0]) * 0.5, first[1].max(second[1]) + 3.];
            let beside = [first[0].max(second[0]) + 3., (first[1] + second[1]) * 0.5];
            // Switch back and forth without accepting or changing the picked midpoints.
            for is_x in [true, false, true, horizontal] {
                let at = if is_x { above } else { beside };
                h.step(&json!({"move":{"world":[at[0],at[1],10.]}})).unwrap();
                h.settle();
                let c = crate::dim_view::draft_preview()
                    .map(|(_, c, _)| c)
                    .or_else(|| crate::dim_view::placement_dimension_preview(&h.app))
                    .expect("a midpoint dimension previews before placement");
                assert!(
                    if is_x { matches!(c.kind, ConstraintKind::DistanceX { .. }) } else { matches!(c.kind, ConstraintKind::DistanceY { .. }) },
                    "{first:?} to {second:?}, horizontal={is_x}: {:?}",
                    c.kind
                );
                let expected = if is_x { (first[0] - second[0]).abs() } else { (first[1] - second[1]).abs() };
                assert!((c.kind.value().unwrap() - expected).abs() < 1e-6);
                assert_eq!(h.app.session.doc.to_json(), baseline);
            }
            let at = if horizontal { above } else { beside };
            h.step(&json!({"click":{"world":[at[0],at[1],10.]}})).unwrap();
            if crate::dim_view::editing() {
                h.step(&json!({"key":"Enter"})).unwrap();
            }
            let state = h.app.session.world_state();
            let sk = &state.sketch(h.app.session.active_sketch.unwrap()).unwrap().sketch;
            let c = sk.constraints.iter().rev().find(|c| c.kind.is_dimension()).unwrap();
            assert!(if horizontal { matches!(c.kind, ConstraintKind::DistanceX { .. }) } else { matches!(c.kind, ConstraintKind::DistanceY { .. }) });
            if held_y {
                assert_eq!(c.driven, !horizontal, "the vertical span is fixed by the model edge");
            }
            assert_eq!(
                sk.constraints.iter().filter(|c| matches!(c.kind, ConstraintKind::Midpoint { .. })).count(),
                2,
                "reuse the picked midpoint points"
            );
            assert_eq!(h.app.session.undo.len(), undo + 1);
            h.app.run("edit.undo", json!({})).unwrap();
            assert_eq!(h.app.session.doc.to_json(), baseline);
            crate::dim_view::cancel();
        }
    }
}

/// A finished, extruded 20 x 10 rectangle whose dimensions are shown in the model view (Show
/// Dimensions): the width `d1` is driven by the user parameter `width`, the height `d2` is a
/// plain value. Returns the harness and the sketch id.
fn shown_dimensions() -> (Harness, u64) {
    crate::dim_view::cancel();
    let mut h = Harness::new();
    h.app.run("parameters.add", json!({"name":"width","expression":"20 mm"})).unwrap();
    let sketch = h.app.run("sketch.create", json!({"plane":"XY"})).unwrap()["sketch"].as_u64().unwrap();
    h.app.run("sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[20,10]})).unwrap();
    h.app.run("sketch.dimension", json!({"entities":["l1"],"value":"width","text_at":[10,-4]})).unwrap();
    h.app.run("sketch.dimension", json!({"entities":["l2"],"value":10,"text_at":[24,5]})).unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
    h.app.run("solid.extrude", json!({"sketch":sketch,"distance":5,"body_name":"Plate"})).unwrap();
    crate::browser::set_sketch_visible(&mut h.app, sketch, true);
    h.app.session.visibility.shown_dimensions.push(sketch);
    h.call("ui.view", json!({"view":"top"}));
    h.app.cam.distance = 80.0;
    h.app.cam.target = solidcraft_engine::geom::Vec3::new(10.0, 5.0, 0.0);
    h.settle();
    (h, sketch)
}

/// The model view's text of the dimension driven by `param` (screen).
fn model_text(h: &Harness, param: &str) -> [f32; 2] {
    let sk = h
        .app
        .session
        .world_state()
        .sketch(h.app.session.doc.features.iter().find(|f| f.name.starts_with("Sketch")).unwrap().id)
        .unwrap()
        .sketch
        .clone();
    let id = sk.constraints.iter().find(|c| c.param.as_deref() == Some(param)).unwrap().id.clone();
    let (_, _, p) = crate::dim_view::model_drawn().into_iter().find(|(_, d, _)| *d == id).expect("the dimension is shown in the model view");
    [p.x, p.y]
}

/// The extruded plate's width and depth (x, y), rounded to 0.001 mm.
fn plate_max(h: &Harness) -> [f64; 2] {
    let b = h.app.session.world_state().body("Plate").unwrap().mesh().bounds();
    [((b.max.x - b.min.x) * 1000.0).round() / 1000.0, ((b.max.y - b.min.y) * 1000.0).round() / 1000.0]
}

#[test]
fn double_clicking_a_shown_dimension_edits_it_without_editing_the_sketch() {
    let (mut h, _) = shown_dimensions();
    let camera = (h.app.cam.target, h.app.cam.distance);
    let undo = h.app.session.undo.len();
    assert!((plate_max(&h)[1] - 10.0).abs() < 1e-6);
    let at = model_text(&h, "d2");
    h.step(&json!({"click":at,"double":true})).unwrap();
    assert!(crate::dim_view::editing(), "the double-click opens the value");
    assert_eq!(h.ctx.memory(|m| m.focused()), Some(egui::Id::new("sc_dim_edit")));
    assert_eq!(h.app.session.active_sketch, None, "the sketch is not edited");
    type_value(&mut h, "14");
    h.step(&json!({"key":"Enter"})).unwrap();
    h.settle();
    assert!(!crate::dim_view::editing());
    assert_eq!(h.app.session.active_sketch, None);
    assert_eq!((h.app.cam.target, h.app.cam.distance), camera, "the view stays put");
    assert_eq!(h.app.session.undo.len(), undo + 1, "one undo step");
    assert!((plate_max(&h)[1] - 14.0).abs() < 1e-6, "the extrude follows: {:?}", plate_max(&h));
    h.app.run("edit.undo", json!({})).unwrap();
    h.settle();
    assert!((plate_max(&h)[1] - 10.0).abs() < 1e-6, "undo restores it");
}

#[test]
fn escape_cancels_a_shown_dimensions_value() {
    let (mut h, _) = shown_dimensions();
    let doc = h.app.session.doc.to_json();
    let at = model_text(&h, "d2");
    h.step(&json!({"click":at,"double":true})).unwrap();
    assert!(crate::dim_view::editing());
    type_value(&mut h, "30");
    h.step(&json!({"key":"Escape"})).unwrap();
    h.settle();
    assert!(!crate::dim_view::editing());
    assert_eq!(h.app.session.doc.to_json(), doc, "nothing changed");
    assert!((plate_max(&h)[1] - 10.0).abs() < 1e-6);
}

#[test]
fn a_parameter_driven_shown_dimension_edits_its_own_expression() {
    let (mut h, _) = shown_dimensions();
    let at = model_text(&h, "d1");
    h.step(&json!({"click":at,"double":true})).unwrap();
    assert!(crate::dim_view::editing());
    assert!(h.widget("width").is_some(), "the box shows the expression that drives it");
    type_value(&mut h, "width + 5");
    h.step(&json!({"key":"Enter"})).unwrap();
    h.settle();
    assert_eq!(h.app.session.doc.param("width").unwrap().expr, "20 mm", "the user parameter is left alone");
    assert_eq!(h.app.session.doc.param("d1").unwrap().expr, "width + 5");
    assert!((plate_max(&h)[0] - 25.0).abs() < 1e-6, "{:?}", plate_max(&h));
    assert_eq!(h.app.session.active_sketch, None);
}

#[test]
fn a_driven_shown_dimension_is_read_only() {
    let (mut h, sketch) = shown_dimensions();
    h.app.run("sketch.edit", json!({"sketch":sketch})).unwrap();
    h.app.run("sketch.dimension", json!({"entities":["l3"],"driven":true,"text_at":[10,14]})).unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
    h.settle();
    let id = h.app.session.world_state().sketch(sketch).unwrap().sketch.constraints.iter().find(|c| c.driven).unwrap().id.clone();
    let (_, _, p) = crate::dim_view::model_drawn().into_iter().find(|(_, d, _)| *d == id).unwrap();
    let doc = h.app.session.doc.to_json();
    h.step(&json!({"click":[p.x, p.y],"double":true})).unwrap();
    assert!(!crate::dim_view::editing());
    assert!(h.app.status.as_ref().is_some_and(|s| s.0.contains("driven")), "{:?}", h.app.status);
    assert_eq!(h.app.session.doc.to_json(), doc);
}

/// The value applies like any other change: the fillet it breaks reports its error on the
/// timeline, and Undo brings back the value and the fillet.
#[test]
fn a_shown_dimension_that_breaks_a_later_feature_reports_it_and_undoes() {
    let (mut h, _) = shown_dimensions();
    h.app.run("solid.fillet", json!({"edges":[[10,10,5]],"radius":4,"name":"Round"})).unwrap();
    h.settle();
    let round = h.app.session.doc.features.iter().find(|f| f.name == "Round").unwrap().id;
    let failed = |h: &Harness| h.app.session.model.result(round).is_some_and(|r| r.error.is_some());
    assert!(!failed(&h));
    let before = h.app.session.doc.to_json();
    let at = model_text(&h, "d2");
    h.step(&json!({"click":at,"double":true})).unwrap();
    type_value(&mut h, "3");
    h.step(&json!({"key":"Enter"})).unwrap();
    h.settle();
    assert_eq!(h.app.session.doc.param("d2").unwrap().expr, "3");
    assert!(failed(&h), "the fillet no longer fits and says so");
    assert_eq!(h.app.session.active_sketch, None);
    h.app.run("edit.undo", json!({})).unwrap();
    h.settle();
    assert_eq!(h.app.session.doc.to_json(), before);
    assert!(!failed(&h), "undo brings the fillet back");
}
