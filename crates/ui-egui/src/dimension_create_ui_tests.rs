//! Dimension creation is an editable draft, accepted by the real focused value field.
use crate::scenario::Harness;
use serde_json::json;
use solidcraft_engine::geom::prelude::*;

fn line(end: [f64; 2]) -> Harness {
    crate::dim_view::cancel();
    let mut h = Harness::new();
    h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    h.app.run("sketch.line", json!({"points":[[0,0],end]})).unwrap();
    h.call("ui.view", json!({"view":"top"}));
    // Leave room around the measured shape while testing the draft itself. Separate Fit
    // tests exercise measured dock occlusion without changing the picking projection.
    h.app.cam.distance = 80.0;
    h.app.cam.target = solidcore_record::Vec3::new(end[0] * 0.5, end[1] * 0.5, 0.0);
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

/// A line from the origin to `end` with Preferences › Sketch › Dimension values set to In a
/// window (#7246).
fn windowed(end: [f64; 2]) -> Harness {
    let mut h = line(end);
    h.call("ui.prefs", json!({"dimension_input": "window"}));
    h
}

/// The value the open new dimension's window holds.
fn draft_value() -> String {
    crate::dim_view::draft_preview().map(|(expr, _, _)| expr).unwrap_or_default()
}

/// In a window, the Dimension tool places the text first and the window opens on it. Its arrows,
/// Up/Down and the wheel over the field step the value while the sketch previews it, nothing
/// reaches the design before the check, the check adds the dimension in one undo step, and the
/// tool stays on for the next dimension.
#[test]
fn a_new_dimension_in_the_window_steps_previews_and_applies_once() {
    let mut h = windowed([20., 0.]);
    let original = h.app.session.doc.to_json();
    let undo = h.app.session.undo.len();
    h.step(&json!({"start":"sketch.dimension"})).unwrap();
    h.step(&json!({"click":{"world":[10,0,0]}})).unwrap();
    assert_eq!(crate::dim_view::live(), None, "no value follows the pointer before the text is placed");
    assert!(!crate::dim_view::editing());
    h.step(&json!({"click":{"world":[10,6,0]}})).unwrap();
    h.frames(3);
    assert!(crate::dim_view::editing(), "placing the text opens the window");
    assert_eq!(h.ctx.memory(|m| m.focused()), Some(egui::Id::new("sc_dim_edit")), "its field takes the keyboard");
    let window = crate::scenario::rect_of("dimwin").expect("the window is shown");
    let drawable = h.app.viewport.layout.as_ref().unwrap().drawable;
    assert!(drawable.contains_rect(window), "the window {window:?} stays in the view {drawable:?}");
    assert_eq!(draft_value(), "20 mm");
    let arrow = crate::scenario::handle_at("dimwin:up");
    for _ in 0..3 {
        h.step(&json!({"click":{"handle":"dimwin:up"}})).unwrap();
    }
    h.frames(2);
    assert_eq!(crate::scenario::handle_at("dimwin:up"), arrow, "the window stays put while the value moves the text");
    let (value, dimension, _) = crate::dim_view::draft_preview().unwrap();
    assert_eq!(value, "23 mm");
    assert!((dimension.kind.value().unwrap() - 23.).abs() < 1e-6, "the sketch previews the stepped value");
    assert!(crate::scenario::count_of("dimwin:preview_curves").unwrap_or(0.0) > 0.0, "what the value moves is drawn");
    assert_eq!(h.app.session.doc.to_json(), original, "nothing reaches the design before OK");
    assert_eq!(h.app.session.undo.len(), undo);
    assert_eq!(h.ctx.memory(|m| m.focused()), Some(egui::Id::new("sc_dim_edit")), "the arrows leave the keyboard with the field");
    // The wheel over the field steps it instead of zooming the view.
    let cam = h.app.cam;
    let field = crate::scenario::rect_of("dimwin:value").unwrap();
    h.call("ui.scroll", json!({"x": field.center().x, "y": field.center().y, "delta": -40.0}));
    h.frames(2);
    assert_eq!(draft_value(), "22 mm");
    assert_eq!(h.app.cam, cam, "the view did not zoom");
    h.step(&json!({"key":"ArrowUp"})).unwrap();
    h.frames(2);
    assert_eq!(draft_value(), "23 mm");
    h.step(&json!({"click":{"handle":"dimwin:ok"}})).unwrap();
    h.frames(2);
    assert!(!crate::dim_view::editing(), "{}", h.call("ui.inspect", json!({})));
    assert_eq!(h.app.session.undo.len(), undo + 1, "one undo step");
    let state = h.app.session.world_state().unwrap();
    let sk = &state.sketch(h.app.session.active_sketch.unwrap()).unwrap().sketch;
    let dimension = sk.constraints.iter().find(|c| c.param.is_some()).expect("the dimension was added");
    assert!((dimension.kind.value().unwrap() - 23.).abs() < 1e-6);
    assert_eq!(h.app.tool.as_ref().map(|t| (t.cmd, t.picks.len())), Some(("sketch.dimension", 0)), "the tool stays on for the next one");
    crate::dim_view::cancel();
}

/// The cross and Esc drop only the dimension in the window: nothing is added and the Dimension
/// tool stays on. Esc with no window open then puts the tool down, as before.
#[test]
fn the_window_cross_and_escape_drop_only_this_dimension() {
    let mut h = windowed([20., 0.]);
    let original = h.app.session.doc.to_json();
    let undo = h.app.session.undo.len();
    place(&mut h, &[[10., 0.]], [10., 6.]);
    h.step(&json!({"click":{"handle":"dimwin:up"}})).unwrap();
    h.step(&json!({"click":{"handle":"dimwin:cancel"}})).unwrap();
    h.frames(2);
    assert!(!crate::dim_view::editing());
    assert_eq!(h.app.session.doc.to_json(), original);
    assert_eq!(h.app.session.undo.len(), undo);
    assert_eq!(h.app.tool.as_ref().map(|t| (t.cmd, t.picks.len())), Some(("sketch.dimension", 0)));
    place(&mut h, &[[10., 0.]], [10., 6.]);
    h.step(&json!({"key":"Escape"})).unwrap();
    assert!(!crate::dim_view::editing());
    assert_eq!(h.app.session.doc.to_json(), original);
    assert_eq!(h.app.tool.as_ref().map(|t| t.cmd), Some("sketch.dimension"), "Esc in the window leaves the tool on");
    h.frames(40);
    h.step(&json!({"key":"Escape"})).unwrap();
    assert!(h.app.tool.is_none(), "Esc with no window open ends the tool");
    crate::dim_view::cancel();
}

/// Double-clicking an existing dimension opens the window too. Typing and stepping preview the
/// sketch without touching the design, a value OK would refuse says why under the field, an
/// expression is left as typed by the arrows, and Enter applies the value in one undo step.
#[test]
fn double_clicking_a_dimension_opens_the_window_with_a_live_preview() {
    let mut h = windowed([20., 0.]);
    h.app.run("parameters.add", json!({"name":"width","expression":"30 mm"})).unwrap();
    h.app.run("sketch.dimension", json!({"entities":["l1"],"value":20,"text_at":[10,8]})).unwrap();
    h.settle();
    let before = h.app.session.doc.to_json();
    let undo = h.app.session.undo.len();
    h.frames(40);
    h.step(&json!({"click":{"dimension":"d1"},"double":true})).unwrap();
    h.frames(3);
    assert!(crate::dim_view::editing() && crate::scenario::rect_of("dimwin").is_some(), "the window opens on the dimension");
    type_value(&mut h, "26");
    h.frames(2);
    assert!(crate::scenario::count_of("dimwin:preview_curves").unwrap_or(0.0) > 0.0, "the sketch previews 26");
    h.step(&json!({"key":"ArrowUp"})).unwrap();
    h.frames(2);
    assert!(h.widget("27").is_some(), "Up stepped the value: {}", h.call("ui.inspect", json!({})));
    assert_eq!(h.app.session.doc.to_json(), before, "nothing reaches the design before OK");
    type_value(&mut h, "no_such_name");
    h.frames(2);
    assert!(crate::scenario::rect_of("dialog:report:dimension_error").is_some(), "the refusal shows under the field");
    assert_eq!(crate::scenario::count_of("dimwin:preview_curves"), Some(0.0), "a refused value previews nothing");
    type_value(&mut h, "width - 4");
    h.step(&json!({"click":{"handle":"dimwin:up"}})).unwrap();
    h.frames(2);
    assert!(h.widget("width - 4").is_some(), "an expression is left as typed");
    type_value(&mut h, "27");
    h.step(&json!({"key":"Enter"})).unwrap();
    h.frames(2);
    assert!(!crate::dim_view::editing());
    assert!((h.app.session.doc.eval("d1", solidcraft_engine::doc::expr::Kind::Length).unwrap() - 27.).abs() < 1e-6);
    assert_eq!(h.app.session.undo.len(), undo + 1, "one undo step");
    crate::dim_view::cancel();
}

/// The arrows keep the unit and decimal mark a value is written in: a length steps by the length
/// step converted to that unit (a bare number is in the design's unit), an angle by the angle
/// step, and anything but a plain number is left alone.
#[test]
fn window_steps_keep_the_written_unit_and_decimal_mark() {
    use crate::dim_view::window::stepped;
    use solidcraft_engine::doc::expr::Kind::{Angle, Length};
    assert_eq!(stepped("20 mm", 1.0, Length, "mm", 1.0, 1.0).as_deref(), Some("21 mm"));
    assert_eq!(stepped("20", -1.0, Length, "mm", 0.5, 1.0).as_deref(), Some("19.5"));
    assert_eq!(stepped("1 in", 1.0, Length, "mm", 2.54, 1.0).as_deref(), Some("1.1 in"));
    assert_eq!(stepped("1", 1.0, Length, "in", 2.54, 1.0).as_deref(), Some("1.1"), "a bare length is in the design's unit");
    assert_eq!(stepped("3\"", 10.0, Length, "mm", 2.54, 1.0).as_deref(), Some("4\""));
    assert_eq!(stepped("2,5 cm", 1.0, Length, "mm", 1.0, 1.0).as_deref(), Some("2,6 cm"));
    assert_eq!(stepped("45 deg", -1.0, Angle, "mm", 1.0, 5.0).as_deref(), Some("40 deg"));
    assert_eq!(stepped("45", 0.1, Angle, "in", 1.0, 1.0).as_deref(), Some("45.1"), "a bare angle is in degrees");
    assert_eq!(stepped("width / 2", 1.0, Length, "mm", 1.0, 1.0), None);
    assert_eq!(stepped("20 deg", 1.0, Length, "mm", 1.0, 1.0), None, "a length does not step an angle");
}

/// Dimension values default to In the view with steps of 1 mm and 1°; the choice and the steps
/// save, out-of-range steps and unknown choices are refused, and the Sketch page's Restore
/// Defaults and Cancel treat them as the page's own.
#[test]
fn the_dimension_window_preferences_default_save_and_reset() {
    use crate::prefs::{DimensionInput, Prefs, control};
    let d = Prefs::default();
    assert_eq!((d.dimension_input, d.dimension_step_mm, d.dimension_step_deg), (DimensionInput::View, 1.0, 1.0));
    let old: Prefs = serde_json::from_value(json!({})).unwrap();
    assert_eq!(old.dimension_input, DimensionInput::View, "saved preferences without it keep the box");
    let read = crate::prefs::lenient(&json!({"dimension_input": "sideways", "dimension_step_mm": 2.0}));
    assert_eq!((read.dimension_input, read.dimension_step_mm), (DimensionInput::View, 2.0), "an unknown choice costs only itself");
    let mut app = crate::SolveApp::new(solidcraft_engine::Session::default(), Default::default());
    for refused in [
        json!({"dimension_input": "sideways"}),
        json!({"dimension_step_mm": 0.0}),
        json!({"dimension_step_mm": 5000.0}),
        json!({"dimension_step_deg": 0.0}),
        json!({"dimension_step_deg": 200.0}),
    ] {
        assert!(control(&mut app, &refused).is_err(), "{refused}");
    }
    assert_eq!(app.preferences, Prefs::default(), "a refusal changes nothing");
    control(&mut app, &json!({"open": "Sketch", "dimension_input": "window", "dimension_step_mm": 0.5, "dimension_step_deg": 15.0})).unwrap();
    control(&mut app, &json!({"button": "apply"})).unwrap();
    let mut other = crate::SolveApp::new(solidcraft_engine::Session::default(), Default::default());
    other.load_prefs(&app.prefs());
    assert_eq!(
        (other.preferences.dimension_input, other.preferences.dimension_step_mm, other.preferences.dimension_step_deg),
        (DimensionInput::Window, 0.5, 15.0)
    );
    control(&mut app, &json!({"button": "defaults"})).unwrap();
    assert_eq!(
        (app.preferences.dimension_input, app.preferences.dimension_step_mm, app.preferences.dimension_step_deg),
        (DimensionInput::View, 1.0, 1.0)
    );
    control(&mut app, &json!({"button": "cancel"})).unwrap();
    assert_eq!(
        (app.preferences.dimension_input, app.preferences.dimension_step_mm, app.preferences.dimension_step_deg),
        (DimensionInput::Window, 0.5, 15.0)
    );
}

/// The Sketch page's Dimension values switch is a real control.
#[test]
fn the_sketch_page_switches_dimension_values_to_a_window() {
    let mut h = Harness::new();
    h.call("ui.prefs", json!({"open": "Sketch"}));
    h.frames(4);
    let handle = format!("dialog:{}:{}", crate::i18n::tr("prefs.dimension_values"), crate::i18n::tr("prefs.dimension_values_window"));
    crate::prefs::reveal_preference(&mut h, &handle);
    h.step(&json!({"click":{"handle": handle}})).unwrap();
    h.frames(2);
    assert_eq!(h.app.preferences.dimension_input, crate::prefs::DimensionInput::Window);
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
        ([12., 16.], vec![[0., 0.], [12., 16.]], [10., 4.]),
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
        let state = h.app.session.world_state().unwrap();
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
        let state = h.app.session.world_state().unwrap();
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
        h.app.cam.target = solidcore_record::Vec3::new(10., 5., 0.);
        h.settle();
        let baseline = h.app.session.doc.to_json();
        let undo = h.app.session.undo.len();
        place(&mut h, &picks, at);
        type_value(&mut h, "12 mm");
        h.step(&json!({"key":"Enter"})).unwrap();
        let state = h.app.session.world_state().unwrap();
        let solved = state.sketch(h.app.session.active_sketch.unwrap()).unwrap();
        // The sketch's first dimension scales it (#1365): 20 × 10 becomes 24 × 12.
        assert!((solved.profiles[0].area - 288.).abs() < 1e-5, "the first dimension scales the rectangle: {}", solved.profiles[0].area);
        assert_eq!(h.app.session.undo.len(), undo + 1);
        h.app.run("edit.undo", json!({})).unwrap();
        assert_eq!(h.app.session.doc.to_json(), baseline);
    }
}

#[test]
fn pending_second_nested_plane_uses_world_picks_but_one_local_dimension() {
    use solidcore_record::Vec2;
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
    let plane = h.app.session.world_state().unwrap().sketch(sid).unwrap().plane;
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
    h.app.cam.target = solidcore_record::Vec3::new(5., 10., 0.);
    h.settle();
    let baseline = h.app.session.doc.to_json();
    let undo = h.app.session.undo.len();
    place(&mut h, &[[0., 0.], [0., 20.]], [-5., 10.]);
    type_value(&mut h, "30 mm");
    h.step(&json!({"key":"Enter"})).unwrap();
    assert!(!crate::dim_view::editing());
    assert_eq!(h.app.session.undo.len(), undo + 1);
    let state = h.app.session.world_state().unwrap();
    let solved = state.sketch(h.app.session.active_sketch.unwrap()).unwrap();
    assert!(solved.report.ok());
    let sk = &solved.sketch;
    assert!((sk.point(sk.point_index(&corner).unwrap()).unwrap().y - 30.).abs() < 1e-6);
    assert!(sk.point(sk.resolve_point("riser.start").unwrap()).unwrap().len() < 1e-8);
    assert!(sk.curves.iter().any(|c| matches!(c.kind, solidcraft_engine::sketch::CurveKind::Arc { .. })), "the dimension retains the fillet arc");
    h.app.run("edit.undo", json!({})).unwrap();
    assert_eq!(h.app.session.doc.to_json(), baseline);
}

/// While a new dimension's value is typed, a committed dimension moves with the sketch drawn at
/// that value and is picked where it is drawn; the draft is no hit target, and Esc puts it back.
#[test]
fn a_committed_dimension_moves_with_the_typed_preview_and_is_picked_where_drawn() {
    crate::dim_view::cancel();
    let mut h = Harness::new();
    h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    h.app.run("sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[20,10]})).unwrap();
    let width = h.app.run("sketch.dimension", json!({"entities":["l3"],"value":20})).unwrap()["param"].as_str().unwrap().to_string();
    h.call("ui.view", json!({"view":"top"}));
    h.app.cam.distance = 80.;
    h.app.cam.target = solidcore_record::Vec3::new(10., 5., 0.);
    h.settle();
    let at = crate::dim_view::text_at(&width).unwrap();
    let drawn = crate::dim_view::drawn();
    let baseline = h.app.session.doc.to_json();
    place(&mut h, &[[0., 0.], [0., 10.]], [-5., 5.]);
    type_value(&mut h, "12 mm");
    h.settle();
    // The sketch is drawn at the typed height, so the width on the top side is drawn, and
    // picked, 2 mm higher; the draft itself is no hit target.
    let [_, top] = h.at(&json!({"world":[10,12,0]})).unwrap();
    let [_, was] = h.at(&json!({"world":[10,10,0]})).unwrap();
    let moved = crate::dim_view::text_at(&width).unwrap() - at;
    assert!(moved.x.abs() < 0.5 && (f64::from(moved.y) - (top - was)).abs() < 0.5, "the width follows its side: {moved:?}");
    let ids = |d: Vec<(String, egui::Pos2)>| d.into_iter().map(|(id, _)| id).collect::<Vec<_>>();
    assert_eq!(ids(crate::dim_view::drawn()), ids(drawn.clone()), "draft annotations must not become committed hit targets");
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
    let edited = h.app.session.world_state().unwrap();
    let edited = &edited.sketch(sketch).unwrap().sketch;
    assert!(edited.constraints.iter().any(|c| c.kind.value().is_some_and(|v| (v - 1.3456 * 25.4).abs() < 1e-6)));
    h.app.run("edit.undo", json!({})).unwrap();
    let before = h.app.session.world_state().unwrap().sketch(sketch).unwrap().sketch.clone();
    let c = before.constraints.iter().find(|c| c.param.is_some()).unwrap();
    let p = h.app.session.doc.param(c.param.as_deref().unwrap()).unwrap();
    assert_eq!(crate::dim_view::label(&c.kind, Some(&p.expr), false, "in", 2), "1.2345 in");
    assert!((c.kind.value().unwrap() - 31.3563).abs() < 1e-6);
    h.app.run("document.units", json!({"units":"mm"})).unwrap();
    assert_eq!(h.app.session.world_state().unwrap().sketch(sketch).unwrap().sketch, before);
    h.app.run("edit.undo", json!({})).unwrap();
    assert_eq!(h.app.session.doc.units, "in");
    let doc = solidcraft_engine::doc::Document::from_json(&h.app.session.doc.to_json()).unwrap();
    let reopened = solidcraft_engine::Session::new(doc);
    assert_eq!(reopened.doc.units, "in");
    assert_eq!(reopened.world_state().unwrap().sketch(sketch).unwrap().sketch, before);
    crate::dim_view::cancel();
}

/// Placing the dimension of two unaligned points above or below the box they span gives the
/// horizontal distance, beside it the vertical one, and inside it or in its outer corners the
/// distance between them (#257). Just above the middle of the box, and just beside it, are
/// horizontal and vertical too, not only past the ends (#7295).
#[test]
fn placing_a_two_point_dimension_picks_horizontal_vertical_or_aligned() {
    // What each placement fixes at 10 mm: the X span, the Y span, or the length.
    let spans = |a: solidcore_record::Vec2, b: solidcore_record::Vec2| [(b.x - a.x).abs(), (b.y - a.y).abs(), a.dist(b)];
    for (at, measured) in [([6., 25.], 0), ([2., 20.], 0), ([-10., 1.], 1), ([15., 4.], 1), ([10., 4.], 2), ([-6., 22.], 2)] {
        let mut h = line([12., 16.]);
        place(&mut h, &[[0., 0.], [12., 16.]], at);
        type_value(&mut h, "10 mm");
        h.step(&json!({"key":"Enter"})).unwrap();
        assert!(!crate::dim_view::editing());
        let state = h.app.session.world_state().unwrap();
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
    let state = h.app.session.world_state().unwrap();
    let solved = state.sketch(h.app.session.active_sketch.unwrap()).unwrap();
    assert!(solved.report.ok());
    let dims: Vec<_> = solved.sketch.constraints.iter().filter(|c| c.kind.is_dimension()).collect();
    assert_eq!(dims.len(), 2);
    assert_eq!(dims.iter().filter(|c| c.driven && c.param.is_none()).count(), 1, "{dims:?}");
    h.app.run("edit.undo", json!({})).unwrap();
    assert_eq!(h.app.session.doc.to_json(), original);
}

/// A dimension placed while a constraint fixed what it measures is driven; once that constraint
/// is deleted, a double-click opens its value, and the value makes it driving, in one undo step
/// (#10018). While the constraint holds it stays read-only, and a value refused leaves it driven.
#[test]
fn a_driven_dimension_nothing_fixes_any_more_takes_a_value() {
    let mut h = line([20., 0.]);
    h.app.run("sketch.dimension", json!({"entities":["l1"],"value":"20 mm","text_at":[10,6]})).unwrap();
    h.app.run("sketch.circle.center", json!({"center":[0,0],"radius":15})).unwrap();
    // The circle's centre is on the origin and the line's end on the circle: its size is fixed.
    let coincident = h.app.run("sketch.constraint.coincident", json!({"a":"l1.end","b":"c1"})).unwrap()["constraint"].as_str().unwrap().to_owned();
    h.app.run("sketch.dimension", json!({"entities":["c1"],"driven":true,"text_at":[-14,14]})).unwrap();
    h.settle();
    let sketch = h.app.session.active_sketch.unwrap();
    let dim = |h: &Harness| {
        let state = crate::open_meshing::session_world(&h.app.session);
        let sk = &state.sketch(sketch).unwrap().sketch;
        sk.constraints
            .iter()
            .find(|c| {
                matches!(
                    c.kind,
                    solidcraft_engine::sketch::ConstraintKind::Diameter { .. } | solidcraft_engine::sketch::ConstraintKind::Radius { .. }
                )
            })
            .cloned()
            .unwrap()
    };
    let id = dim(&h).id;
    assert!(dim(&h).driven && dim(&h).param.is_none(), "{:?}", dim(&h));
    let double_click = |h: &mut Harness| {
        h.frames(40);
        h.step(&json!({"click":{"dimension":id},"double":true})).unwrap();
    };

    // The Coincident still fixes the circle: the dimension only measures.
    let fixed = h.app.session.doc.to_json();
    double_click(&mut h);
    assert!(!crate::dim_view::editing(), "a dimension the sketch determines has no value to edit");
    assert!(h.app.status.as_ref().is_some_and(|s| s.0.contains("driven")), "{:?}", h.app.status);
    assert_eq!(h.app.session.doc.to_json(), fixed);

    // Without it, the value opens at what the dimension measures, named as the parameter it gets.
    h.app.run("sketch.delete", json!({"entities":[coincident]})).unwrap();
    h.settle();
    let before = h.app.session.doc.to_json();
    let undo = h.app.session.undo.len();
    let name = h.app.session.doc.next_d_name();
    let measured = dim(&h).kind.value().unwrap();
    double_click(&mut h);
    assert!(crate::dim_view::editing(), "{:?}", h.app.status);
    assert!(h.widget(&format!("Dimension: {name}")).is_some(), "{}", h.call("ui.inspect", json!({})));

    // A value the design refuses leaves it driven, and the design and its history as they were.
    type_value(&mut h, "nosuch * 2");
    h.step(&json!({"key":"Enter"})).unwrap();
    assert!(crate::dim_view::editing(), "a refused value keeps the box open");
    assert_eq!(h.app.session.doc.to_json(), before);
    assert_eq!(h.app.session.undo.len(), undo);

    // A value it takes makes it driving at that value.
    type_value(&mut h, "30");
    h.step(&json!({"key":"Enter"})).unwrap();
    assert!(!crate::dim_view::editing(), "{:?}", h.app.status);
    let driving = dim(&h);
    assert!(!driving.driven && driving.param.as_deref() == Some(name.as_str()), "{driving:?}");
    let value = driving.kind.value().unwrap();
    assert!((value - 30.0).abs() < 1e-6 && (measured - 30.0).abs() > 1.0, "{measured} → {value}");
    assert!(crate::open_meshing::session_world(&h.app.session).sketch(sketch).unwrap().report.ok());

    // One undo step puts the driven dimension back.
    assert_eq!(h.app.session.undo.len(), undo + 1);
    h.app.run("edit.undo", json!({})).unwrap();
    assert_eq!(h.app.session.doc.to_json(), before);
    crate::dim_view::cancel();
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
    h.app.cam.target = solidcore_record::Vec3::new(centre[0], centre[1], 0.0);
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
    let state = h.app.session.world_state().unwrap();
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

/// The open sketch's lines as the next frame paints them (the viewport's sketch scene, rebuilt
/// only when its key changes, before the frame's dimensions are drawn; the harness has no GPU,
/// so the test does it): their bounds, [min x, min y, max x, max y].
fn drawn_extent(h: &mut Harness) -> [f32; 4] {
    let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));
    let proj = crate::viewport::projection(&h.app, rect);
    crate::viewport::refresh_sketch_scene(&mut h.app, &proj);
    let slot = h.app.viewport.sketch_slot.lock().unwrap();
    let scene = slot.as_ref().expect("a sketch scene");
    let ends = scene.overlay.as_chunks::<32>().0.iter().flat_map(|l| [&l[0..12], &l[12..24]]);
    ends.map(|b| [0, 4].map(|i| b.get(i..i + 4).and_then(|x| x.try_into().ok()).map(f32::from_le_bytes)))
        .filter_map(|[x, y]| Some([x?, y?]))
        .fold([f32::MAX, f32::MAX, f32::MIN, f32::MIN], |m, [x, y]| [m[0].min(x), m[1].min(y), m[2].max(x), m[3].max(y)])
}

fn near(a: [f32; 4], b: [f32; 4]) -> bool {
    a.iter().zip(b).all(|(a, b)| (a - b).abs() < 0.6)
}

fn bounds(sk: &solidcraft_engine::sketch::Sketch) -> [f32; 4] {
    let (lo, hi) = sk.bounds().unwrap_or_default();
    [lo.x, lo.y, hi.x, hi.y].map(|v| v as f32)
}

/// While a value is typed the sketch is drawn as the value leaves it, not only the dimension
/// (#11584), whatever the shape; Esc draws it as it is again.
#[test]
fn typing_a_value_draws_the_sketch_at_that_value_and_escape_restores_it() {
    let fix = |entity: &str| ("sketch.constraint.fix", json!({"entity": entity}));
    let rectangle = vec![("sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[40,20]}))];
    let circle = vec![("sketch.circle.center", json!({"center":[10,0],"radius":5})), fix("c1.center")];
    let slot = vec![("sketch.slot.center_to_center", json!({"p0":[-20,0],"p1":[20,0],"width":20}))];
    let arc = vec![("sketch.arc.center_point", json!({"center":[10,0],"start":[18,0],"sweep":90})), fix("a1.center")];
    let spline = vec![
        ("sketch.spline.fit_point", json!({"points": [[-30, 0], [-10, 18], [10, 6], [30, 20]]})),
        ("sketch.line", json!({"points":[[30,20],[30,-10]],"infer":true})),
    ];
    let mixed = vec![
        ("sketch.circle.center", json!({"center":[0,0],"radius":10})),
        ("sketch.circle.center", json!({"center":[40,0],"radius":10})),
        ("sketch.line", json!({"points":[[0,10],[40,10]],"infer":false})),
        ("sketch.constraint.tangent", json!({"a":"l1","b":"c1"})),
        ("sketch.constraint.tangent", json!({"a":"l1","b":"c2"})),
        ("sketch.constraint.equal", json!({"a":"c1","b":"c2"})),
        fix("c1.center"),
        fix("c2.center"),
    ];
    let half = std::f64::consts::FRAC_1_SQRT_2 * 8.0;
    let cases: [(&str, &[(&str, serde_json::Value)], [f64; 2], Vec<[f64; 2]>, &str); 6] = [
        ("rectangle side", &rectangle, [20., 10.], vec![[20., 0.]], "60"),
        ("circle diameter", &circle, [10., 0.], vec![[15., 0.]], "20"),
        ("slot side", &slot, [0., 0.], vec![[0., 10.]], "60"),
        ("arc radius", &arc, [10., 0.], vec![[10. + half, half]], "12"),
        ("spline end", &spline, [0., 5.], vec![[30., 5.]], "50"),
        ("tangent equal circles", &mixed, [20., 0.], vec![[-10., 0.]], "30"),
    ];
    for (name, setup, centre, picks, typed) in cases {
        let mut h = sketch_with(setup, centre);
        // Far enough out that the grown shapes stay in view (the drawn lines stop at its edge).
        h.app.cam.distance = 200.0;
        h.settle();
        let real = bounds(&solved(&h));
        let before = drawn_extent(&mut h);
        assert!(near(before, real), "{name}: drawn {before:?}, sketch {real:?}");
        for p in &picks {
            h.step(&json!({"click":{"world":[p[0],p[1],0]}})).unwrap();
        }
        assert_live(&h);
        keys(&mut h, typed);
        // What the frame after the last key paints, against the typed value's solve a frame later.
        let shown = drawn_extent(&mut h);
        h.frames(1);
        let (_, _, preview) = crate::dim_view::draft_preview().expect("the draft is open");
        let target = bounds(&preview);
        assert!(!near(target, real), "{name}: the typed value changes the sketch: {target:?}");
        assert!(near(shown, target), "{name}: the sketch follows the typed value: drawn {shown:?}, solved {target:?}");
        assert!(near(bounds(&solved(&h)), real), "{name}: nothing changes until Enter");
        // A value that does not solve (here half typed) keeps the last one that did.
        keys(&mut h, "*");
        assert_eq!(crate::dim_view::draft_preview().map(|(expr, _, _)| expr), Some(format!("{typed}*")));
        let kept = drawn_extent(&mut h);
        assert!(near(kept, target), "{name}: an unsolved value keeps the last drawn: {kept:?}");
        h.step(&json!({"key":"Escape"})).unwrap();
        assert!(!crate::dim_view::editing());
        let after = drawn_extent(&mut h);
        assert!(near(after, real), "{name}: Esc draws the sketch as it is: {after:?}");
        crate::dim_view::cancel();
    }
}

/// Each solve of a typed value redraws the sketch, also two between frames (a moved text and a
/// typed value in one frame), and a solve back to an earlier value: the scene is keyed on the
/// solve, not on where its result happens to be in memory.
#[test]
fn every_solve_of_a_typed_value_redraws_the_sketch() {
    let mut h = sketch_with(&[("sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[40,20]}))], [20., 10.]);
    h.app.cam.distance = 300.0;
    h.settle();
    h.step(&json!({"click":{"world":[20,0,0]}})).unwrap();
    assert_live(&h);
    keys(&mut h, "60");
    let sixty = drawn_extent(&mut h);
    let mut solves = std::collections::BTreeSet::new();
    for _ in 0..4 {
        crate::dim_view::resolve(&h.app, "70");
        crate::dim_view::resolve(&h.app, "80");
        assert!(solves.insert(crate::dim_view::preview_solve(&h.app).expect("a preview")), "a new solve has a new number");
        let (_, _, preview) = crate::dim_view::draft_preview().expect("the draft is open");
        let drawn = drawn_extent(&mut h);
        assert!(!near(drawn, sixty) && near(drawn, bounds(&preview)), "drawn {drawn:?}, solved {:?}", bounds(&preview));
        crate::dim_view::resolve(&h.app, "60");
        assert!(near(drawn_extent(&mut h), sixty));
    }
    crate::dim_view::cancel();
}

/// While a value is typed the sketch is drawn at it, so a profile under the pointer is not lit
/// where the design has it (over the resized one); it is again once the value is let go.
#[test]
fn a_typed_value_does_not_light_the_design_s_profile_under_the_pointer() {
    let mut h = sketch_with(&[("sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[40,20]}))], [20., 10.]);
    h.step(&json!({"click":{"world":[20,0,0]}})).unwrap();
    assert_live(&h);
    keys(&mut h, "60");
    let sketch = h.app.session.active_sketch.unwrap();
    h.app.viewport.hover = Some(crate::viewport::Hit::Profile { sketch, index: 0 });
    let key = crate::viewport::highlight_key(&h.app);
    assert!(crate::viewport::build_highlight(&h.app).merged().trans.is_empty(), "no profile lit while the value is typed");
    h.step(&json!({"key":"Escape"})).unwrap();
    h.app.viewport.hover = Some(crate::viewport::Hit::Profile { sketch, index: 0 });
    assert_ne!(crate::viewport::highlight_key(&h.app), key, "letting the value go lights it again");
    assert!(!crate::viewport::build_highlight(&h.app).merged().trans.is_empty(), "lit as before");
    crate::dim_view::cancel();
}

/// Until a value is typed the sketch is drawn as the design has it, so a picked dimension's
/// value box alone does not put the view in the typed preview: the profile under the pointer
/// stays lit. The first key does.
#[test]
fn a_dimension_only_picked_keeps_the_design_view_until_a_value_is_typed() {
    let mut h = sketch_with(&[("sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[40,20]}))], [20., 10.]);
    h.step(&json!({"click":{"world":[20,0,0]}})).unwrap();
    assert_live(&h);
    assert!(crate::dim_view::preview_solve(&h.app).is_none(), "nothing typed yet: no preview");
    let sketch = h.app.session.active_sketch.unwrap();
    h.app.viewport.hover = Some(crate::viewport::Hit::Profile { sketch, index: 0 });
    assert!(!crate::viewport::build_highlight(&h.app).merged().trans.is_empty(), "the profile is lit while only picked");
    keys(&mut h, "6");
    assert!(crate::dim_view::preview_solve(&h.app).is_some(), "a key starts the preview");
    crate::dim_view::cancel();
}

/// A selected profile is not lit where the design has it while a value is typed either.
#[test]
fn a_typed_value_does_not_light_a_selected_design_profile() {
    let mut h = sketch_with(&[("sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[40,20]}))], [20., 10.]);
    h.step(&json!({"click":{"world":[20,0,0]}})).unwrap();
    assert_live(&h);
    keys(&mut h, "60");
    let sketch = h.app.session.active_sketch.unwrap();
    h.app.session.selection = vec![solidcraft_engine::Sel::Profile { sketch, index: 0 }];
    assert!(crate::dim_view::preview_solve(&h.app).is_some(), "the value is still typed");
    assert!(crate::viewport::build_highlight(&h.app).merged().trans.is_empty(), "no profile lit while the value is typed");
    crate::dim_view::cancel();
    assert!(!crate::viewport::build_highlight(&h.app).merged().trans.is_empty(), "lit once the value is let go");
}

/// While a value is typed the sketch's curves are sampled before its profiles: a sketch whose
/// profiles would use up the sampling budget still has its curves drawn (its fills go first).
#[test]
fn a_typed_value_draws_the_curves_before_the_profiles_use_up_the_budget() {
    let mut h = sketch_with(
        &[("sketch.circle.center", json!({"center":[10,0],"radius":5})), ("sketch.constraint.fix", json!({"entity": "c1.center"}))],
        [10., 0.],
    );
    h.step(&json!({"click":{"world":[15,0,0]}})).unwrap();
    assert_live(&h);
    keys(&mut h, "20");
    let (_, _, preview) = crate::dim_view::draft_preview().expect("the draft is open");
    let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));
    let proj = crate::viewport::projection(&h.app, rect);
    let plane = h.app.session.world_state().unwrap().sketch(h.app.session.active_sketch.unwrap()).unwrap().plane;
    let mut all = solidcraft_engine::render::SamplingBudget::new(131_072, 1_048_576);
    let need = crate::sketch_display::curve_with_budget(&preview, 0, &plane, &proj, h.app.viewport.pixels_per_point(), &mut all).vertex_count;
    assert!(need > 4, "the circle takes some vertices: {need}");
    crate::sketch_display::UNCACHED_VERTICES.with(|v| v.set(Some(need)));
    let drawn = drawn_extent(&mut h);
    crate::sketch_display::UNCACHED_VERTICES.with(|v| v.set(None));
    assert!(near(drawn, bounds(&preview)), "the circle is drawn at the typed value: drawn {drawn:?}, solved {:?}", bounds(&preview));
    crate::dim_view::cancel();
}

/// Is a sketch point drawn at `world` this frame (its filled square)?
fn point_drawn(h: &mut Harness, world: [f64; 2]) -> bool {
    let [x, y] = h.at(&json!({"world":[world[0], world[1], 0]})).unwrap();
    let at = egui::pos2(x as f32, y as f32);
    h.shapes.iter().any(|s| matches!(&s.shape, egui::Shape::Rect(r) if r.rect.width() == 5.0 && r.rect.center().distance(at) < 0.5))
}

/// Is a sketch curve's hover highlight painted this frame?
fn hover_painted(h: &Harness) -> bool {
    let col = egui::epaint::ColorMode::Solid(crate::theme::Tokens::get().hover_profile_edge);
    h.shapes.iter().any(|s| matches!(&s.shape, egui::Shape::Path(p) if p.stroke.width == 3.0 && p.stroke.color == col))
}

/// While a value is typed, the sketch's points and constraint glyphs are drawn where the value
/// puts them as well as its curves, the next frame already (the value is solved when typed), and
/// no hover highlight traces a curve of the design; Esc puts everything back.
#[test]
fn typing_a_value_draws_points_and_glyphs_there_and_no_hover_of_the_design() {
    let rectangle = [("sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[40,20]}))];
    let mut h = sketch_with(&rectangle, [30., 15.]);
    h.app.cam.distance = 200.0;
    h.settle();
    let glyphs = crate::sketch_tools::glyph_positions();
    assert!(!glyphs.is_empty());
    assert!(point_drawn(&mut h, [40., 20.]));
    h.step(&json!({"click":{"world":[20,0,0]}})).unwrap();
    assert_live(&h);
    keys(&mut h, "60");
    // The frame after the last key, as the app draws it.
    h.frames(1);
    assert!(point_drawn(&mut h, [60., 30.]), "the corner is drawn where 60 puts it");
    assert!(!point_drawn(&mut h, [40., 20.]), "no point where the design has it");
    // The right side's glyph moves with its middle, from (40, 10) to (60, 15).
    let [x0, y0] = h.at(&json!({"world":[40,10,0]})).unwrap();
    let [x1, y1] = h.at(&json!({"world":[60,15,0]})).unwrap();
    let shift = egui::vec2((x1 - x0) as f32, (y1 - y0) as f32);
    let typed = crate::sketch_tools::glyph_positions();
    let moved = |id: &String, at: egui::Pos2| glyphs.iter().any(|(was_id, was)| was_id == id && (at - *was - shift).length() < 1.5);
    assert!(typed.iter().any(|(id, at)| moved(id, *at)), "a glyph follows the right side: {glyphs:?} -> {typed:?}, shift {shift:?}");
    // Over where the design's right side is, nothing is drawn, so nothing lights up.
    h.step(&json!({"move":{"world":[40,10,0]}})).unwrap();
    h.frames(1);
    assert!(!hover_painted(&h), "no hover highlight of the design's curve");
    // One more key and one frame: the value is solved in the frame it is typed, so the next
    // frame's sketch scene, built before the dimensions are drawn, is already at that value.
    h.frame_with(vec![egui::Event::Text("5".into())]);
    let (text, solved) = crate::dim_view::solved_for().expect("the draft is open");
    assert_eq!((text.as_str(), solved.as_str()), ("605", "605"));
    h.step(&json!({"key":"Escape"})).unwrap();
    h.frames(1);
    assert!(point_drawn(&mut h, [40., 20.]));
    assert_eq!(crate::sketch_tools::glyph_positions(), glyphs);
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

/// Put the value field's caret at `at` with nothing selected, as the field itself does when it
/// misses the keyboard for a frame (a WebView2 showed the caret before the value, #11204).
fn caret_without_the_person(h: &Harness, at: usize) {
    let id = egui::Id::new("sc_dim_edit");
    let mut state = egui::TextEdit::load_state(&h.ctx, id).unwrap_or_default();
    state.cursor.set_char_range(Some(egui::text::CCursorRange::one(egui::text::CCursor::new(at))));
    state.store(&h.ctx, id);
}

/// A primary click at sketch point `at`, the button held down for `frames` frames.
fn held_click(h: &mut Harness, at: [f64; 2], frames: usize) {
    h.step(&json!({"move":{"world":[at[0],at[1],0]}})).unwrap();
    let [x, y] = h.at(&json!({"world":[at[0],at[1],0]})).unwrap();
    let pos = egui::pos2(x as f32, y as f32);
    let (button, modifiers) = (egui::PointerButton::Primary, egui::Modifiers::NONE);
    h.app.synthetic.push(egui::Event::PointerButton { pos, button, pressed: true, modifiers });
    h.frames(frames);
    h.app.synthetic.push(egui::Event::PointerButton { pos, button, pressed: false, modifiers });
    h.settle();
}

/// The value field's caret and selection end (characters).
fn caret(h: &Harness) -> Option<(usize, usize)> {
    egui::TextEdit::load_state(&h.ctx, egui::Id::new("sc_dim_edit"))
        .and_then(|s| s.cursor.char_range())
        .map(|r| (r.primary.index.0, r.secondary.index.0))
}

/// Until the person takes the proposed value over, the first digit they type replaces all of
/// it (#11204): however long the click that placed it was held, frames later, and whatever moved
/// the caret without them. Typed without Ctrl+A, as a person does.
#[test]
fn typed_digits_replace_the_proposed_value_whatever_moved_the_caret() {
    for held in [1usize, 4, 12] {
        let mut h = sketch_with(&[("sketch.line", json!({"points":[[0,0],[20,0]],"infer":false}))], [10., 0.]);
        let dims = dimension_count(&h);
        h.step(&json!({"click":{"world":[10,0,0]}})).unwrap();
        assert_live(&h);
        // A click held `held` frames: what a person's 60 to 150 ms click is at 60 Hz and beyond.
        held_click(&mut h, [10., -7.], held);
        assert_eq!(crate::dim_view::live(), None, "held {held}: the text is pinned");
        h.frames(12);
        caret_without_the_person(&h, 0);
        h.frames(2);
        assert_eq!(h.ctx.memory(|m| m.focused()), Some(egui::Id::new("sc_dim_edit")), "held {held}");
        keys(&mut h, "400");
        assert_eq!(crate::dim_view::draft_preview().map(|d| d.0).as_deref(), Some("400"), "held {held}");
        h.step(&json!({"key":"Enter"})).unwrap();
        let new = added(&h, dims);
        assert_eq!(new.len(), 1, "held {held}");
        assert!((new[0].kind.value().unwrap() - 400.).abs() < 1e-6, "held {held}: {:?}", new[0].kind);
        crate::dim_view::cancel();
    }
}

/// A caret the person put somewhere stays there: Home then 1 writes in front of the value, and a
/// click into the field types where it landed (#11204).
#[test]
fn a_caret_the_person_moved_stays_where_they_put_it() {
    let mut h = sketch_with(&[("sketch.line", json!({"points":[[0,0],[20,0]],"infer":false}))], [10., 0.]);
    let dims = dimension_count(&h);
    h.step(&json!({"click":{"world":[10,0,0]}})).unwrap();
    assert_live(&h);
    h.step(&json!({"click":{"world":[10,-7,0]}})).unwrap();
    h.frames(4);
    h.step(&json!({"key":"Home"})).unwrap();
    h.frames(4);
    assert_eq!(caret(&h), Some((0, 0)), "Home put the caret before the value");
    keys(&mut h, "1");
    h.frames(4);
    assert_eq!(caret(&h), Some((1, 1)), "the caret stays after what was typed");
    assert_eq!(crate::dim_view::draft_preview().map(|d| d.0).as_deref(), Some("120 mm"));
    h.step(&json!({"key":"Enter"})).unwrap();
    let new = added(&h, dims);
    assert_eq!(new.len(), 1);
    assert!((new[0].kind.value().unwrap() - 120.).abs() < 1e-6, "{:?}", new[0].kind);
    crate::dim_view::cancel();

    // A click into the field puts the caret where it landed: typing goes there.
    let mut h = sketch_with(&[("sketch.line", json!({"points":[[0,0],[20,0]],"infer":false}))], [10., 0.]);
    h.step(&json!({"click":{"world":[10,0,0]}})).unwrap();
    h.step(&json!({"click":{"world":[10,-7,0]}})).unwrap();
    h.frames(4);
    let [x, y] = h.widget("20 mm").expect("the value field");
    h.step(&json!({"click":[x, y]})).unwrap();
    h.frames(4);
    assert!(crate::dim_view::editing(), "a click into the field keeps the value open");
    let placed = caret(&h);
    assert!(placed.is_some_and(|(a, b)| a == b), "the click placed a caret: {placed:?}");
    keys(&mut h, "4");
    let expr = crate::dim_view::draft_preview().map(|d| d.0).unwrap_or_default();
    assert!(expr.len() == "20 mm4".len() && expr.contains('4') && expr != "4", "typing went in at the click, not over the value: {expr:?}");
    crate::dim_view::cancel();
}

/// Type-first, the text follows the pointer: moving it from inside the box the picks span (the
/// distance between them) to above or beside it switches to the horizontal or vertical distance and
/// back. Until something is typed the value follows the dimension, so the preview measures the
/// picked points where they are (it neither moves nor scales them) and Enter keeps them there.
#[test]
fn type_first_switching_between_aligned_horizontal_and_vertical_keeps_the_picked_points() {
    use solidcraft_engine::sketch::ConstraintKind as K;
    let end = [12., 16.];
    let mid = [6., 8.];
    // Inside the picks' box, above it (over its middle), beside it.
    let (across, above, beside) = ([10., 4.], [3., 20.], [-10., 1.]);
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
                    a.dist(solidcore_record::Vec2::new(0., 0.)) < 1e-6 && b.dist(solidcore_record::Vec2::new(end[0], end[1])) < 1e-6,
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
                a.dist(solidcore_record::Vec2::new(0., 0.)) < 1e-6 && b.dist(solidcore_record::Vec2::new(end[0], end[1])) < 1e-6,
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
        let state = crate::open_meshing::session_world(&h.app.session);
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
    use solidcore_record::Vec3;
    crate::dim_view::cancel();
    let mut h = Harness::new();
    h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    h.app.run("sketch.rectangle.two_point", json!({"p0":[-10,-5],"p1":[10,15]})).unwrap();
    h.app.finish_sketch();
    h.app.run("solid.extrude", json!({"distance":10})).unwrap();
    h.app.run("sketch.create", json!({"plane":{"face":[0,5,10]}})).unwrap();
    let state = crate::open_meshing::session_world(&h.app.session);
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
        let state = crate::open_meshing::session_world(&h.app.session);
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
                let state = crate::open_meshing::session_world(&h.app.session);
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
                let state = crate::open_meshing::session_world(&h.app.session);
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
            let state = crate::open_meshing::session_world(&h.app.session);
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
    h.app.cam.target = solidcore_record::Vec3::new(10.0, 5.0, 0.0);
    h.settle();
    (h, sketch)
}

/// The model view's text of the dimension driven by `param` (screen).
fn model_text(h: &Harness, param: &str) -> [f32; 2] {
    let sk = h
        .app
        .session
        .world_state()
        .unwrap()
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
    let b = h.app.session.world_state().unwrap().body("Plate").unwrap().mesh().bounds();
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
    let id = h.app.session.world_state().unwrap().sketch(sketch).unwrap().sketch.constraints.iter().find(|c| c.driven).unwrap().id.clone();
    let (_, _, p) = crate::dim_view::model_drawn().into_iter().find(|(_, d, _)| *d == id).unwrap();
    let doc = h.app.session.doc.to_json();
    h.step(&json!({"click":[p.x, p.y],"double":true})).unwrap();
    assert!(!crate::dim_view::editing());
    assert!(h.app.status.as_ref().is_some_and(|s| s.0.contains("driven")), "{:?}", h.app.status);
    assert_eq!(h.app.session.doc.to_json(), doc);
}

/// The value applies like any other change: the shell it breaks reports its error on the
/// timeline, and Undo brings back the value and the shell. The plate's 10 width taken to 3 leaves
/// no room inside two 2 thick walls. (A round wider than its face no longer breaks: a convex
/// round runs over the face's far edge, cut as if the face went on.)
#[test]
fn a_shown_dimension_that_breaks_a_later_feature_reports_it_and_undoes() {
    let (mut h, _) = shown_dimensions();
    h.app.run("solid.shell", json!({"faces":[[10,1,5]],"thickness":2,"name":"Hollow"})).unwrap();
    h.settle();
    let hollow = h.app.session.doc.features.iter().find(|f| f.name == "Hollow").unwrap().id;
    let failed = |h: &Harness| h.app.session.model.result(hollow).is_some_and(|r| r.error.is_some());
    assert!(!failed(&h));
    let before = h.app.session.doc.to_json();
    let at = model_text(&h, "d2");
    h.step(&json!({"click":at,"double":true})).unwrap();
    type_value(&mut h, "3");
    h.step(&json!({"key":"Enter"})).unwrap();
    h.settle();
    assert_eq!(h.app.session.doc.param("d2").unwrap().expr, "3");
    assert!(failed(&h), "the shell no longer has room and says so");
    assert_eq!(h.app.session.active_sketch, None);
    h.app.run("edit.undo", json!({})).unwrap();
    h.settle();
    assert_eq!(h.app.session.doc.to_json(), before);
    assert!(!failed(&h), "undo brings the shell back");
}

/// Editing an existing dimension to `name = value` names its own parameter (#4071): `d1`
/// becomes `width`, set to the value, in one undo step, and opening it again shows `width`'s
/// value as its expression. No other parameter is made.
#[test]
fn editing_a_dimension_to_a_definition_names_its_parameter() {
    let mut h = line([25., 0.]);
    place(&mut h, &[[12.5, 0.]], [12.5, 8.]);
    type_value(&mut h, "25");
    h.step(&json!({"key":"Enter"})).unwrap();
    h.settle();
    let params = h.app.session.doc.params.len();
    let undo = h.app.session.undo.len();
    crate::dim_view::edit_param(&h.app, "d1");
    h.settle();
    type_value(&mut h, "width = 30");
    h.step(&json!({"key":"Enter"})).unwrap();
    let doc = &h.app.session.doc;
    assert!(doc.param("d1").is_none(), "{:?}", h.app.status);
    assert_eq!(doc.param("width").map(|p| p.expr.as_str()), Some("30"));
    assert_eq!(doc.params.len(), params, "renamed, not added");
    assert_eq!(h.app.session.undo.len(), undo + 1, "one undo step");
    assert!(h.app.status.as_ref().is_some_and(|s| !s.2 && s.0.contains("`width`")), "{:?}", h.app.status);
    let sk = doc.sketch(h.app.session.active_sketch.unwrap()).unwrap();
    assert!(sk.constraints.iter().any(|c| c.param.as_deref() == Some("width") && c.kind.value().is_some_and(|v| (v - 30.0).abs() < 1e-6)));
}

/// The status line after a dimension is applied reads in the interface language (#8863): a
/// new parameter (`w = 40/2`), a plain value, a renamed parameter and a driven dimension, with
/// the parameter name kept as typed.
#[test]
fn the_status_after_a_dimension_is_in_the_interface_language() {
    let pl = crate::i18n::Locale::for_language("pl");
    let en = crate::i18n::Locale::for_language("en");
    let status = |h: &Harness| h.app.status.as_ref().map(|s| (s.0.clone(), s.2));
    let polish = |mut h: Harness| {
        h.app.preferences.language = "pl".into();
        h.frames(2);
        h
    };
    // A new parameter.
    let mut h = polish(line([25., 0.]));
    place(&mut h, &[[12.5, 0.]], [12.5, 8.]);
    type_value(&mut h, "w = 40/2");
    h.step(&json!({"key":"Enter"})).unwrap();
    assert!(h.app.session.doc.param("w").is_some(), "{:?}", h.app.status);
    assert_eq!(status(&h), Some(("Wymiar zastosowany; zdefiniowano parametr `w`".to_string(), false)));
    // A plain value, then the same dimension renamed to `width = 30`.
    let mut h = polish(line([25., 0.]));
    place(&mut h, &[[12.5, 0.]], [12.5, 8.]);
    type_value(&mut h, "25");
    h.step(&json!({"key":"Enter"})).unwrap();
    assert_eq!(status(&h), Some((pl.text("sketch.dimension_applied").to_string(), false)));
    h.settle();
    crate::dim_view::edit_param(&h.app, "d1");
    h.settle();
    type_value(&mut h, "width = 30");
    h.step(&json!({"key":"Enter"})).unwrap();
    assert!(h.app.session.doc.param("width").is_some(), "{:?}", h.app.status);
    assert_eq!(status(&h), Some((pl.format("sketch.dimension_applied_renamed", &[("name", "width")]), false)));
    // A dimension the sketch already determines is added as a driven one.
    let mut h = polish(line([20., 0.]));
    h.app.run("sketch.dimension", json!({"entities":["l1"],"value":"20 mm"})).unwrap();
    h.settle();
    h.step(&json!({"start":"sketch.dimension"})).unwrap();
    h.step(&json!({"click":{"world":[10,0,0]}})).unwrap();
    h.step(&json!({"move":{"world":[10,6,0]}})).unwrap();
    h.settle();
    h.step(&json!({"click":{"world":[10,6,0]}})).unwrap();
    assert!(!crate::dim_view::editing(), "a driven dimension has no value to edit");
    assert_eq!(status(&h), Some((pl.text("sketch.dimension_added_driven").to_string(), false)));
    for id in ["sketch.dimension_applied", "sketch.dimension_applied_defined", "sketch.dimension_applied_renamed", "sketch.dimension_added_driven"] {
        assert_ne!(pl.text(id), en.text(id), "{id}: the Polish catalog translates it");
    }
    crate::dim_view::cancel();
}

/// A selected curve of the sketch is not lit where the design has it while a value is typed, as
/// a selected profile is not.
#[test]
fn a_typed_value_does_not_light_a_selected_design_curve() {
    let mut h = sketch_with(&[("sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[40,20]}))], [20., 10.]);
    h.step(&json!({"click":{"world":[20,0,0]}})).unwrap();
    assert_live(&h);
    keys(&mut h, "60");
    let sketch = h.app.session.active_sketch.unwrap();
    let id = solved(&h).curves[0].id.clone();
    for sel in [
        solidcraft_engine::Sel::SketchCurve { id: id.clone(), sketch: None },
        solidcraft_engine::Sel::SketchCurve { id: id.clone(), sketch: Some(sketch) },
        solidcraft_engine::Sel::ExternalSketchCurve { sketch, id: id.clone() },
    ] {
        h.app.session.selection = vec![sel.clone()];
        assert!(crate::viewport::build_highlight(&h.app).overlay.is_empty(), "{sel:?}: no curve lit while the value is typed");
    }
    crate::dim_view::cancel();
    assert!(!crate::viewport::build_highlight(&h.app).overlay.is_empty(), "lit once the value is let go");
}
