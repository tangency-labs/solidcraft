//! The kit's test fixtures: a context with egui's default faces (the kit ships no fonts; a host
//! installs its own) and headless frames, pointer and key events and shape walks.
use super::*;
use egui::{Event, FullOutput, RawInput, Shape, Ui, vec2};
use solidcraft_ui_registry as registry;

/// The kit's own English test catalog: the messages its tests read, with the same English a host
/// gives them. Catalogs are host data; the kit's tests never rely on SolidCraft's.
static ENGLISH: solidcraft_ui_text::Catalog = solidcraft_ui_text::Catalog::new("en", "English", include_str!("test_support/catalog.json"));
static CATALOGS: [&solidcraft_ui_text::Catalog; 1] = [&ENGLISH];

/// Install the kit's test catalog (once per process) and read the system language as English.
pub(crate) fn install_catalog() {
    solidcraft_ui_text::pin_system_language();
    solidcraft_ui_text::install(&CATALOGS);
}

/// A context for the kit's tests: egui's default faces with the kit's weight families bound to
/// them, and the kit's English test catalog.
pub(crate) fn context() -> egui::Context {
    install_catalog();
    let ctx = egui::Context::default();
    type_face::install_weight_families(&ctx);
    ctx
}

/// A frame without a renderer. Preserve all caller-supplied timing and viewport options.
pub(crate) fn raw_frame(ctx: &egui::Context, input: RawInput, add: impl FnMut(&mut Ui)) -> FullOutput {
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

#[test]
fn render_results_shared_divider_clamps_hostile_positions_and_uses_bounded_chrome() {
    for (input, expected) in [(f32::NAN, 0.5), (f32::INFINITY, 0.5), (f32::NEG_INFINITY, 0.5), (-1e30, 0.05), (0.25, 0.25), (1e30, 0.95)] {
        assert_eq!(comparison_position(input), expected);
    }
    let ctx = context();
    let position = std::cell::Cell::new(0.5);
    let mut draw = |ui: &mut Ui| {
        let (canvas, _) = ui.allocate_exact_size(vec2(240.0, 180.0), egui::Sense::hover());
        let mut value = position.get();
        comparison_divider(ui, canvas, &mut value, "Comparison divider");
        comparison_badge(ui, canvas, "Live preview", false);
        position.set(value);
    };
    frame(&ctx, vec![], &mut draw);
    let handle = registry::rect_of("render:compare:handle").unwrap().center();
    frame(&ctx, pointer(handle, true), &mut draw);
    let end = egui::pos2(1000.0, handle.y);
    frame(&ctx, vec![egui::Event::PointerMoved(end)], &mut draw);
    assert_eq!(position.get(), 0.95);
    frame(&ctx, pointer(end, false), &mut draw);
}

fn draw_frame(ctx: &egui::Context, events: Vec<egui::Event>, mut add: impl FnMut(&mut Ui)) -> egui::FullOutput {
    registry::clear_handles();
    raw_frame(
        ctx,
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, vec2(320.0, 400.0))),
            events,
            focused: true,
            ..Default::default()
        },
        |ui| {
            ui.set_width(280.0);
            scope(ui, &mut add);
        },
    )
}

pub(crate) fn frame(ctx: &egui::Context, events: Vec<egui::Event>, add: impl FnMut(&mut Ui)) {
    draw_frame(ctx, events, add);
}

/// The cursor the frame asks the platform for: the pointer is at `pos` for the whole frame.
pub(crate) fn frame_cursor(ctx: &egui::Context, pos: egui::Pos2, add: impl FnMut(&mut Ui)) -> egui::CursorIcon {
    draw_frame(ctx, vec![egui::Event::PointerMoved(pos)], add).platform_output.cursor_icon
}
