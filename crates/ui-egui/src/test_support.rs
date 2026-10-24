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
    h.app.session.selection = vec![Sel::SketchCurve { id: curve.clone() }];
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
