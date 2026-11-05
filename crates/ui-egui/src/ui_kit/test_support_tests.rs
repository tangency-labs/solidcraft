use super::*;
use crate::scenario;
use egui::{Ui, vec2};

pub(crate) use crate::test_support::{key, pointer};

#[test]
fn render_results_shared_divider_clamps_hostile_positions_and_uses_bounded_chrome() {
    for (input, expected) in [(f32::NAN, 0.5), (f32::INFINITY, 0.5), (f32::NEG_INFINITY, 0.5), (-1e30, 0.05), (0.25, 0.25), (1e30, 0.95)] {
        assert_eq!(comparison_position(input), expected);
    }
    let ctx = crate::fonts::test_context();
    let position = std::cell::Cell::new(0.5);
    let mut draw = |ui: &mut Ui| {
        let (canvas, _) = ui.allocate_exact_size(vec2(240.0, 180.0), egui::Sense::hover());
        let mut value = position.get();
        comparison_divider(ui, canvas, &mut value, "Comparison divider");
        comparison_badge(ui, canvas, "Live preview", false);
        position.set(value);
    };
    frame(&ctx, vec![], &mut draw);
    let handle = scenario::rect_of("render:compare:handle").unwrap().center();
    frame(&ctx, pointer(handle, true), &mut draw);
    let end = egui::pos2(1000.0, handle.y);
    frame(&ctx, vec![egui::Event::PointerMoved(end)], &mut draw);
    assert_eq!(position.get(), 0.95);
    frame(&ctx, pointer(end, false), &mut draw);
}

fn draw_frame(ctx: &egui::Context, events: Vec<egui::Event>, mut add: impl FnMut(&mut Ui)) -> egui::FullOutput {
    scenario::clear_handles();
    crate::test_support::frame(
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
