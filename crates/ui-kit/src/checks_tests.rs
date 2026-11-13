use super::*;
use crate::test_support::{checkbox_shapes, frame, key, pointer, raster};
use egui::Shape;
use std::collections::HashMap;

#[test]
fn checkbox_keyboard_pointer_disabled_and_mixed() {
    for mixed in [false, true] {
        for enabled in [false, true] {
            for activation in [None, Some(egui::Key::Space), Some(egui::Key::Enter)] {
                let ctx = crate::test_support::context();
                let mut on = false;
                let mut changed = false;
                let target = std::cell::Cell::new(Rect::NOTHING);
                let mut draw = |ui: &mut Ui| {
                    ui.add_enabled_ui(enabled, |ui| {
                        let response = checkbox_indeterminate(ui, "Option", &mut on, mixed);
                        changed |= response.changed();
                        target.set(response.rect);
                        if activation.is_some() {
                            response.request_focus();
                        }
                    });
                };
                frame(&ctx, vec![], &mut draw);
                if let Some(activation) = activation {
                    frame(&ctx, key(activation), &mut draw);
                } else {
                    // The label is part of the target, too.
                    let at = target.get().right_center() - vec2(1.0, 0.0);
                    frame(&ctx, pointer(at, true), &mut draw);
                    frame(&ctx, pointer(at, false), &mut draw);
                }
                assert_eq!(on, enabled, "mixed={mixed}, enabled={enabled}, key={activation:?}");
                assert_eq!(changed, enabled);
            }
        }
    }
}

#[test]
fn wrapped_checkbox_keeps_full_label_and_clicks_on_its_last_line() {
    for enabled in [false, true] {
        let ctx = crate::test_support::context();
        let label = "A long translated checkbox label remains readable and clickable on its final line";
        let target = std::cell::Cell::new(Rect::NOTHING);
        let mut value = false;
        let mut draw = |ui: &mut Ui| {
            ui.set_width(180.0);
            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
            ui.add_enabled_ui(enabled, |ui| {
                target.set(checkbox(ui, label, &mut value).rect);
            });
        };
        frame(&ctx, vec![], &mut draw);
        let rect = target.get();
        assert!(rect.height() > Tokens::get().control_height);
        assert!(rect.width() <= 180.0);
        let at = rect.left_bottom() + vec2(Tokens::get().space_24, -2.0);
        frame(&ctx, pointer(at, true), &mut draw);
        frame(&ctx, pointer(at, false), &mut draw);
        assert_eq!(value, enabled, "a wrapped checkbox preserves native disabled and click behavior");
    }
}

#[test]
fn grid_palette_inline_and_menu_share_every_box_state() {
    let previous = solidcraft_theme::is_dark();
    for dark in [false, true] {
        solidcraft_theme::set_dark(dark);
        for ppp in [1.0, 2.0] {
            for on in [false, true] {
                for state in ["idle", "hover", "focus", "disabled"] {
                    let mut reference = None;
                    for variant in ["inline", "grid", "palette", "menu"] {
                        let ctx = crate::test_support::context();
                        ctx.set_pixels_per_point(ppp);
                        let mut at = None;
                        let mut final_output = None;
                        for pass in 0..4 {
                            let events = if state == "hover" && pass > 0 {
                                vec![egui::Event::PointerMoved(at.unwrap())]
                            } else if state == "focus" && pass == 1 {
                                key(egui::Key::Tab)
                            } else {
                                vec![]
                            };
                            let mut output = ctx.run_ui(
                                egui::RawInput {
                                    screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(400.0, 120.0))),
                                    events,
                                    focused: true,
                                    ..Default::default()
                                },
                                |ui| {
                                    crate::scope(ui, |ui| {
                                        ui.add_enabled_ui(state != "disabled", |ui| {
                                            let mut value = on;
                                            match variant {
                                                "grid" => crate::checkbox_grid(ui, &mut [("Option", &mut value)], 1),
                                                "palette" => {
                                                    crate::checkbox_row(ui, "option", "Option", value);
                                                }
                                                "menu" => {
                                                    crate::menu::menu_frame(ui, |ui| {
                                                        checkbox(ui, "Option", &mut value);
                                                    });
                                                }
                                                _ => {
                                                    checkbox(ui, "Option", &mut value);
                                                }
                                            }
                                        });
                                    });
                                },
                            );
                            output.textures_delta.clear();
                            let boxes = checkbox_shapes(&output);
                            assert_eq!(boxes.len(), 1, "{variant}, {state}");
                            at = Some(boxes[0].0.center());
                            final_output = Some(output);
                        }
                        let output = final_output.unwrap();
                        let (square, shapes) = checkbox_shapes(&output).remove(0);
                        if state == "focus" {
                            assert!(
                                output
                                    .shapes
                                    .iter()
                                    .any(|shape| matches!(&shape.shape, Shape::Rect(r) if r.rect == square.expand(Tokens::get().space_2))),
                                "{variant}: must actually focus"
                            );
                        }
                        if state == "hover" && !on {
                            let Shape::Rect(r) = &shapes[0].shape else {
                                panic!("box");
                            };
                            assert_eq!(r.fill, Tokens::get().hover, "{variant}: must actually hover");
                        }
                        let pixels = raster(&ctx, shapes, ppp, square, &HashMap::new());
                        if let Some(reference) = &reference {
                            assert_eq!(pixels, *reference, "{variant}: {state}, on={on}, dark={dark}, ppp={ppp}");
                        } else {
                            reference = Some(pixels);
                        }
                    }
                }
            }
        }
    }
    solidcraft_theme::set_dark(previous);
}

#[test]
fn checkbox_accessibility_preserves_label_checked_and_mixed_metadata() {
    for mixed in [false, true] {
        let ctx = crate::test_support::context();
        ctx.enable_accesskit();
        let mut value = false;
        let mut before = None;
        let mut after = None;
        for pass in 0..4 {
            let events = match pass {
                1 => key(egui::Key::Tab),
                3 => key(egui::Key::Space),
                _ => vec![],
            };
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(400.0, 120.0))),
                    events,
                    focused: true,
                    ..Default::default()
                },
                |ui| {
                    checkbox_indeterminate(ui, "Enabled", &mut value, mixed);
                },
            );
            output.textures_delta.clear();
            for event in output.platform_output.events {
                match event {
                    egui::output::OutputEvent::FocusGained(info) => before = Some(info),
                    egui::output::OutputEvent::Clicked(info) | egui::output::OutputEvent::ValueChanged(info) => after = Some(info),
                    _ => {}
                }
            }
        }
        let before = before.expect("checkbox gains keyboard focus");
        assert_eq!(before.role, egui::Role::CheckBox);
        assert_eq!(before.label.as_deref(), Some("Enabled"));
        assert_eq!(before.selected, if mixed { None } else { Some(false) });
        let after = after.expect("activation publishes the changed checked state");
        assert_eq!(after.role, egui::Role::CheckBox);
        assert_eq!(after.label.as_deref(), Some("Enabled"));
        assert_eq!(after.selected, Some(true));
    }
}
