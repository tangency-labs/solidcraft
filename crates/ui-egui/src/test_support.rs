//! Headless test fixtures. Callers retain frame counts, input and command parameters.
use crate::{Services, SolveApp, scenario::Harness};
use egui::{Event, FullOutput, RawInput, Shape, Ui};
use serde_json::{Value, json};
use solidcraft_engine::{Sel, Session};

pub(crate) fn app_with_script(script: Value) -> SolveApp {
    let mut session = Session::default();
    session.run_script(&script).unwrap();
    SolveApp::new(session, Services::default())
}

pub(crate) fn sample_app() -> SolveApp {
    app_with_script(solidcraft_engine::sample::script())
}

pub(crate) fn run_all(app: &mut SolveApp, commands: Vec<(String, Value)>) {
    for (id, params) in commands {
        app.run(&id, params).unwrap();
    }
}

pub(crate) fn play(h: &mut Harness) {
    while !h.app.synthetic.is_empty() {
        h.frame();
    }
    h.frames(2);
}

pub(crate) fn click(h: &mut Harness, handle: &str) {
    assert!(crate::scenario::handle_at(handle).is_some(), "{handle} is not drawn: {:?}", crate::scenario::handles(""));
    h.step(&json!({"click": {"handle": handle}})).unwrap();
    h.frames(3);
}

/// One component named Block, with its original occurrence free and the root active.
pub(crate) fn component_box() -> (Harness, u64, u64) {
    let mut h = Harness::new();
    let part = h.app.run("component.create", json!({"name":"Block","grounded":false})).unwrap();
    let component = part["component"].as_u64().unwrap();
    let occurrence = part["occurrence"].as_u64().unwrap();
    h.app.run("solid.box", json!({"length":10,"width":10,"height":10})).unwrap();
    h.app.run("component.activate", json!({"component":0})).unwrap();
    (h, component, occurrence)
}

/// Named 20 × 20 × 10 boxes with A grounded; preserve the caller's command entry point.
pub(crate) fn grounded_boxes(parts: &[(&str, i32)], mut run: impl FnMut(&str, Value) -> Value) {
    for (name, x) in parts {
        run("component.activate", json!({"component":"root"}));
        run("component.create", json!({"name":name}));
        run("solid.box", json!({"length":20,"width":20,"height":10,"corner":[x,0,0]}));
    }
    run("component.activate", json!({"component":"root"}));
    run("occurrence.ground", json!({"occurrence":"A:1","grounded":true}));
}

/// Text with its first outline selected; callers decide when to start tools and draw.
pub(crate) fn selected_text(text: &str, at: [i32; 2]) -> (Harness, String, String) {
    let mut h = Harness::new();
    h.app.home.open = false;
    h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    let result = h.app.run("sketch.text", json!({"text":text,"at":at,"height":10})).unwrap();
    let link = result["link"].as_str().unwrap().to_owned();
    let curve = result["curves"][0].as_str().unwrap().to_owned();
    h.app.session.selection = vec![Sel::SketchCurve { id: curve.clone(), sketch: None }];
    (h, link, curve)
}

/// A frame without a renderer. Preserve all caller-supplied timing and viewport options.
pub(crate) fn frame(ctx: &egui::Context, input: RawInput, add: impl FnMut(&mut Ui)) -> FullOutput {
    let mut output = ctx.run_ui(input, add);
    output.textures_delta.clear();
    output
}

pub(crate) fn pointer(pos: egui::Pos2, pressed: bool) -> Vec<Event> {
    vec![Event::PointerMoved(pos), Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() }]
}

pub(crate) fn key(key: egui::Key) -> Vec<Event> {
    vec![Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: Default::default() }]
}

/// Top-level text shapes only, in their original paint order.
pub(crate) fn text_shapes(output: &FullOutput) -> impl Iterator<Item = &egui::epaint::TextShape> {
    output.shapes.iter().filter_map(|clipped| match &clipped.shape {
        Shape::Text(text) => Some(text),
        _ => None,
    })
}

/// Every leaf of a shape list, in paint order.
pub(crate) fn each_shape(shape: &Shape, visit: &mut impl FnMut(&Shape)) {
    match shape {
        Shape::Vec(shapes) => shapes.iter().for_each(|shape| each_shape(shape, visit)),
        shape => visit(shape),
    }
}

/// Reach a real dialog control with Tab rather than injecting a widget ID into focus memory.
pub(crate) fn dialog_tab_to(h: &mut Harness, handle: &str) {
    for _ in 0..96 {
        let at = crate::scenario::handle_at(handle).unwrap_or_else(|| panic!("missing dialog target {handle}"));
        if h.ctx.memory(|m| m.focused()).and_then(|id| h.ctx.read_response(id)).is_some_and(|r| {
            r.enabled()
                && r.rect.is_positive()
                && r.rect.contains(at)
                && h.app.viewport.dialog_rect.is_some_and(|dialog| dialog.contains(r.rect.center()))
        }) {
            return;
        }
        h.step(&json!({"key":"Tab"})).unwrap();
    }
    panic!("Tab did not reach {handle}");
}

/// Verify a whole native focus cycle and its exact reverse without changing picks or the model.
pub(crate) fn dialog_tab_cycle(h: &mut Harness) {
    let start = h.ctx.memory(|m| m.focused()).expect("start inside the dialog");
    let layer = h.ctx.read_response(start).unwrap().layer_id;
    let document = h.app.session.doc.to_json();
    let undo = h.app.session.undo.len();
    let selection = h.app.session.selection.clone();
    let picks: Vec<_> = h.app.dialog.as_ref().unwrap().inputs.iter().map(|input| input.items.clone()).collect();
    let mut forward = vec![start];
    let mut wrapped = false;
    for _ in 0..96 {
        h.step(&json!({"key":"Tab"})).unwrap();
        let id = h.ctx.memory(|m| m.focused()).expect("Tab must always reach a control");
        let response = h.ctx.read_response(id).unwrap();
        assert_eq!(response.layer_id, layer, "Tab must not enter canvas, browser, toolbar or palette");
        assert!(response.enabled() && response.rect.is_positive());
        assert!(h.app.viewport.dialog_rect.unwrap().contains(response.rect.center()));
        assert_ne!(Some(response.rect), crate::scenario::rect_of("dialog:header"), "drag headers are not controls");
        if h.app.preferences.dialog_buttons == crate::ui_kit::DialogButtons::Bottom {
            assert!(!response.rect.contains(crate::scenario::handle_at("dialog:close").unwrap()), "Close is pointer-only");
        }
        if id == start {
            wrapped = true;
            break;
        }
        assert!(!forward.contains(&id), "focus must not get stuck in a smaller sub-cycle");
        forward.push(id);
    }
    assert!(wrapped && forward.len() > 2, "fields and actions must form a bounded cycle");
    for expected in forward.iter().rev() {
        h.step(&json!({"key":"Tab", "shift":true})).unwrap();
        assert_eq!(h.ctx.memory(|m| m.focused()), Some(*expected), "Shift+Tab must exactly reverse the native order");
    }
    assert_eq!(h.app.session.doc.to_json(), document, "navigation must not commit preview geometry");
    assert_eq!(h.app.session.undo.len(), undo);
    assert_eq!(h.app.session.selection, selection);
    assert_eq!(h.app.dialog.as_ref().unwrap().inputs.iter().map(|input| input.items.clone()).collect::<Vec<_>>(), picks);
}
