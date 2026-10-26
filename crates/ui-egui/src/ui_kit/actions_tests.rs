use crate::ui_kit::test_support::*;
use crate::ui_kit::*;

use crate::scenario;
use egui::Ui;

#[test]
fn footer_clicks_and_keyboard_respect_validation_and_editor_focus() {
    for enabled in [false, true] {
        let ctx = crate::fonts::test_context();
        let mut action = Action::None;
        let mut draw = |ui: &mut Ui| {
            action = footer(ui, "OK", "Cancel", enabled, Footer::default());
        };
        frame(&ctx, vec![], &mut draw);
        let pos = scenario::handle_at("dialog:ok").unwrap();
        frame(&ctx, pointer(pos, true), &mut draw);
        frame(&ctx, pointer(pos, false), &mut draw);
        assert_eq!(action, if enabled { Action::Ok } else { Action::None });
        // Button clicks may own focus; keyboard submission is for an unfocused form.
        ctx.memory_mut(|m| {
            if let Some(id) = m.focused() {
                m.surrender_focus(id);
            }
        });
        frame(&ctx, key(egui::Key::Enter), |ui| action = footer(ui, "OK", "Cancel", enabled, Footer::default()));
        assert_eq!(action, if enabled { Action::Ok } else { Action::None });
        frame(&ctx, key(egui::Key::Escape), |ui| action = footer(ui, "OK", "Cancel", enabled, Footer::default()));
        assert_eq!(action, Action::Cancel);
        frame(&ctx, vec![], |ui| {
            footer(ui, "OK", "Cancel", enabled, Footer::default());
        });
        let cancel = scenario::handle_at("dialog:cancel").unwrap();
        frame(&ctx, pointer(cancel, true), |ui| action = footer(ui, "OK", "Cancel", enabled, Footer::default()));
        frame(&ctx, pointer(cancel, false), |ui| action = footer(ui, "OK", "Cancel", enabled, Footer::default()));
        assert_eq!(action, Action::Cancel);
    }
    let ctx = crate::fonts::test_context();
    let mut action = Action::None;
    let mut text = "radius".to_string();
    frame(&ctx, vec![], |ui| {
        let response = ui.text_edit_singleline(&mut text);
        response.request_focus();
        footer(ui, "OK", "Cancel", true, Footer::default());
    });
    frame(&ctx, key(egui::Key::Enter), |ui| {
        ui.text_edit_singleline(&mut text).request_focus();
        action = footer(ui, "OK", "Cancel", true, Footer::default());
    });
    // A text editor handles Enter itself; the footer cannot apply merely because it was pressed.
    assert_eq!(action, Action::None);
}

#[test]
fn enabled_buttons_show_the_pointing_hand_and_disabled_ones_do_not() {
    for (enabled, expected) in [(true, egui::CursorIcon::PointingHand), (false, egui::CursorIcon::Default)] {
        let ctx = crate::fonts::test_context();
        let mut draw = |ui: &mut Ui| {
            footer(ui, "OK", "Cancel", enabled, Footer { apply: Some("Apply"), ..Default::default() });
        };
        frame(&ctx, vec![], &mut draw);
        let ok = scenario::handle_at("dialog:ok").unwrap();
        // Settled over two frames: the hover is known from the first frame's layout.
        frame_cursor(&ctx, ok, &mut draw);
        assert_eq!(frame_cursor(&ctx, ok, &mut draw), expected, "OK, enabled {enabled}");
        // Apply is a button too, and Cancel is always enabled.
        let apply = scenario::handle_at("dialog:apply").unwrap();
        frame_cursor(&ctx, apply, &mut draw);
        assert_eq!(frame_cursor(&ctx, apply, &mut draw), expected, "Apply, enabled {enabled}");
        let cancel = scenario::handle_at("dialog:cancel").unwrap();
        frame_cursor(&ctx, cancel, &mut draw);
        assert_eq!(frame_cursor(&ctx, cancel, &mut draw), egui::CursorIcon::PointingHand, "Cancel is always enabled");
    }
}

#[test]
fn positioned_shared_buttons_center_text_and_respect_disabled_clicks_in_both_themes() {
    use crate::theme::Tokens;
    use egui::{Pos2, RawInput, Rect, RichText, Shape, vec2};
    let previous = crate::theme::is_dark();
    for dark in [false, true] {
        crate::theme::set_dark(dark);
        for enabled in [false, true] {
            let ctx = crate::fonts::test_context();
            crate::theme::apply(&ctx);
            let rects =
                [Rect::from_min_size(egui::pos2(20.0, 30.0), vec2(108.0, 32.0)), Rect::from_min_size(egui::pos2(140.0, 30.0), vec2(108.0, 32.0))];
            let mut clicks = [0_usize; 2];
            let mut draw = |events| {
                let mut output = ctx.run_ui(
                    RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(320.0, 200.0))), events, focused: true, ..Default::default() },
                    |ui| {
                        ui.add_enabled_ui(enabled, |ui| {
                            for (index, (label, variant)) in [("Recover", Button::Primary), ("Discard", Button::Normal)].into_iter().enumerate() {
                                let response = button_at(
                                    ui,
                                    label,
                                    rects[index],
                                    RichText::new(label).font(crate::fonts::semibold(Tokens::get().font_button)),
                                    variant,
                                );
                                assert_eq!(response.rect, rects[index]);
                                assert_eq!(response.enabled(), enabled);
                                if response.clicked() {
                                    clicks[index] += 1;
                                }
                            }
                        });
                    },
                );
                output.textures_delta.clear();
                output
            };
            let output = draw(vec![]);
            for (index, label) in ["Recover", "Discard"].into_iter().enumerate() {
                let text = output
                    .shapes
                    .iter()
                    .find_map(|shape| match &shape.shape {
                        Shape::Text(text) if text.galley.job.text == label => Some(text),
                        _ => None,
                    })
                    .expect("native button label is painted");
                if enabled && index == 0 {
                    let ink = text.galley.job.sections.first().expect("primary text format").format.color;
                    assert!(crate::theme::contrast_ratio(ink, Tokens::get().form_primary) >= 4.5, "primary label remains legible in both themes");
                }
                let center = text.pos + text.galley.size() / 2.0;
                assert!((center.x - rects[index].center().x).abs() <= 1.0, "{label} horizontal center: {center:?}");
                assert!((center.y - rects[index].center().y).abs() <= 1.0, "{label} vertical center: {center:?}");
            }
            for rect in rects {
                draw(pointer(rect.center(), true));
                draw(pointer(rect.center(), false));
            }
            assert_eq!(clicks, if enabled { [1, 1] } else { [0, 0] });
        }
    }
    crate::theme::set_dark(previous);
}
