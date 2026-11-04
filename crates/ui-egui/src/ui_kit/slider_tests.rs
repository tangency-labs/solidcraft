use super::*;
use crate::{scenario, theme::Tokens};
use egui::{Rect, Ui, pos2, vec2};

#[test]
fn theme_notice_actions_fit_wrap_and_accept_native_pointer_clicks_in_both_themes() {
    const NOTICE: &str = "A theme option changed. Undo restores the previous value; dismiss keeps the current value and closes this notice.";

    fn frame(ctx: &egui::Context, width: f32, can_undo: bool, events: Vec<egui::Event>) -> (bool, bool) {
        scenario::clear_handles();
        let mut clicked = (false, false);
        crate::test_support::frame(
            ctx,
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(1200.0, 1600.0))),
                events,
                focused: true,
                ..Default::default()
            },
            |ui| {
                let mut content = Rect::NOTHING;
                let allocated = ui.allocate_ui_with_layout(vec2(width, ui.available_height()), egui::Layout::top_down(egui::Align::Min), |ui| {
                    ui.set_width(width);
                    content = ui.max_rect().intersect(ui.clip_rect());
                    ui.set_clip_rect(content);
                    scope(ui, |ui| {
                        clicked = theme_option_notice(ui, "guard", NOTICE, can_undo);
                    });
                });
                for (target, present) in [("prefs:theme_notice_undo:guard", can_undo), ("prefs:theme_notice_dismiss:guard", true)] {
                    let rect = scenario::rect_of(target);
                    if present {
                        let rect = rect.expect("notice action");
                        assert!(content.contains_rect(rect), "{target} clips at width {width}: {rect:?} outside {content:?}");
                        assert!(allocated.response.rect.contains_rect(rect), "{target} must be inside the allocated notice content");
                        assert!(rect.is_positive());
                        assert_eq!(scenario::handle_at(target), Some(rect.center()));
                    } else {
                        assert!(rect.is_none(), "Undo must be absent when unavailable");
                    }
                }
            },
        );
        clicked
    }

    let previous = crate::theme::is_dark();
    for dark in [false, true] {
        crate::theme::set_dark(dark);
        for language in ["en", "de"] {
            let _language = crate::i18n::scope(language);
            let t = Tokens::get();
            let mut undo_width = 0.0;
            let mut dismiss_width = 0.0;
            let mut label_wrap_width = 0.0;
            test_support::frame(&crate::fonts::test_context(), vec![], |ui| {
                undo_width = ContentWidth::button(ui, crate::i18n::tr("ui.undo"));
                dismiss_width = ContentWidth::button(ui, crate::i18n::tr("prefs.theme_notice_dismiss"));
                label_wrap_width = (ContentWidth::label(ui, crate::i18n::tr("prefs.theme_notice_dismiss")) / 3.0).ceil();
            });
            let actions_width = undo_width + dismiss_width + t.space_8 * 2.0;
            let narrow_width = undo_width.max(dismiss_width);
            for (width, stacked, label_wrap) in
                [(actions_width + t.form_card_width, false, false), (narrow_width, true, false), (label_wrap_width, true, true)]
            {
                for can_undo in [true, false] {
                    let ctx = crate::fonts::test_context();
                    assert_eq!(frame(&ctx, width, can_undo, vec![]), (false, false));
                    let text = scenario::rect_of("dialog:hint:guard").expect("notice text");
                    let dismiss = scenario::rect_of("prefs:theme_notice_dismiss:guard").expect("Dismiss");
                    let undo = scenario::rect_of("prefs:theme_notice_undo:guard");
                    if stacked {
                        assert!(dismiss.top() >= text.bottom(), "Dismiss must move below the text");
                        if let Some(undo) = undo {
                            assert!(undo.top() >= text.bottom(), "Undo must move below the text");
                            assert!(dismiss.top() >= undo.bottom(), "actions must wrap onto separate rows at width {width}");
                        }
                    } else {
                        assert_eq!(dismiss.top(), text.top(), "wide notices retain the inline layout");
                        if let Some(undo) = undo {
                            assert_eq!(undo.top(), text.top());
                            assert!(dismiss.left() >= undo.right());
                        }
                        // The wide notice retains the original action budget and text allocation.
                        let expected = width - dismiss_width - t.space_8 - if can_undo { undo_width + t.space_8 } else { 0.0 };
                        assert_eq!(text.width(), expected);
                    }
                    if label_wrap {
                        assert!(dismiss.height() > t.control_height, "an individual long action label must wrap");
                    }
                    for (target, expected) in [("prefs:theme_notice_undo:guard", (true, false)), ("prefs:theme_notice_dismiss:guard", (false, true))]
                    {
                        if expected.0 && !can_undo {
                            continue;
                        }
                        let at = if expected.0 { undo.expect("Undo").center() } else { dismiss.center() };
                        assert_eq!(frame(&ctx, width, can_undo, test_support::pointer(at, true)), (false, false), "{target} on press");
                        assert_eq!(frame(&ctx, width, can_undo, test_support::pointer(at, false)), expected, "{target} on release");
                        assert_eq!(frame(&ctx, width, can_undo, vec![]), (false, false), "{target} must not repeat");
                    }
                }
            }
        }
    }
    crate::theme::set_dark(previous);
}

#[test]
fn blur_meter_boundaries_and_off_thumb() {
    for (radius, bars) in [(0.0, 0), (1.0, 1), (8.0, 1), (9.0, 2), (18.0, 3), (33.0, 5), (40.0, 5), (60.0, 5)] {
        assert_eq!(blur_cost_bars(radius), bars, "{radius}");
    }
    for dark in [false, true] {
        crate::theme::set_dark(dark);
        for radius in [0.0, 18.25] {
            let ctx = crate::fonts::test_context();
            let t = Tokens::get();
            let mut value = radius;
            let out = ctx.run_ui(egui::RawInput::default(), |ui| {
                scope(ui, |ui| {
                    slider(ui, &mut value, 0.0..=40.0, Slider { step: Some(1.0), show_value: false, off_endpoint: true, ..Default::default() });
                });
            });
            assert_eq!(value, radius, "drawing cannot round persisted fractional values");
            let thumb = out
                .shapes
                .iter()
                .rev()
                .find_map(|s| match &s.shape {
                    egui::Shape::Circle(c) if c.radius == t.space_8 => Some(c),
                    _ => None,
                })
                .unwrap();
            assert_eq!(thumb.fill, if radius == 0.0 { egui::Color32::TRANSPARENT } else { t.form_primary });
            assert_eq!(thumb.stroke.color, if radius == 0.0 { t.form_dim } else { t.form_primary });
        }
    }
}

#[test]
fn every_slider_option_uses_the_same_token_thumb_in_both_themes() {
    let previous = crate::theme::is_dark();
    for dark in [false, true] {
        crate::theme::set_dark(dark);
        for (show_value, integer, enabled) in [(true, false, true), (true, false, false), (false, true, true)] {
            for formatted in [false, true] {
                let ctx = crate::fonts::test_context();
                let t = Tokens::get();
                let mut value: f64 = 1.0;
                let mut output = ctx.run_ui(
                    egui::RawInput { screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(400.0, 200.0))), ..Default::default() },
                    |ui| {
                        scope(ui, |ui| {
                            ui.add_enabled_ui(enabled, |ui| {
                                slider(
                                    ui,
                                    &mut value,
                                    0.0..=2.0,
                                    Slider {
                                        step: formatted.then_some(0.05),
                                        format: if formatted { |v| format!("{:.0} %", v * 100.0) } else { |v| format!("{v:.2}") },
                                        show_value,
                                        integer,
                                        workspace_accent: false,
                                        off_endpoint: false,
                                    },
                                );
                            });
                        });
                    },
                );
                output.textures_delta.clear();
                let thumb = output
                    .shapes
                    .iter()
                    .rev()
                    .find_map(|s| match &s.shape {
                        egui::Shape::Circle(c) if c.radius == t.space_8 => Some(c),
                        _ => None,
                    })
                    .expect("shared kit thumb");
                assert_eq!(thumb.stroke.width, t.space_2);
                // Disabled chrome uses egui's standard painter fade; geometry stays identical.
                if enabled {
                    assert_eq!(thumb.fill, t.control_fill);
                    assert_eq!(thumb.stroke.color, t.form_primary);
                }
                assert_eq!(value, 1.0);
            }
        }
    }
    crate::theme::set_dark(previous);
}

#[test]
fn slider_steps_endpoints_disabled_input_and_hidden_editor_keep_native_behaviour() {
    for (show_value, integer, enabled) in [(true, false, true), (false, true, true), (true, false, false)] {
        let ctx = crate::fonts::test_context();
        let mut value: f64 = 1.0;
        let (min, max) = if integer { (0.0, 4.0) } else { (0.5, 2.5) };
        let track = std::cell::Cell::new(Rect::NOTHING);
        let mut draw = |ui: &mut Ui| {
            ui.add_enabled_ui(enabled, |ui| {
                let response = slider(ui, &mut value, min..=max, Slider { step: Some(0.05), show_value, integer, ..Default::default() });
                track.set(scenario::rect_of("dialog:slider_track:").expect("slider track"));
                assert_eq!(response.rect.height(), Tokens::get().control_height);
            });
        };
        test_support::frame(&ctx, vec![], &mut draw);
        let hit = track.get();
        let at = pos2(hit.left() + hit.width() * 0.61, hit.center().y);
        test_support::frame(&ctx, test_support::pointer(at, true), &mut draw);
        test_support::frame(&ctx, test_support::pointer(at, false), &mut draw);
        if enabled {
            assert!(value > 1.0 && value < max);
            if integer {
                assert_eq!(value.fract(), 0.0);
            } else {
                assert!((value / 0.05 - (value / 0.05).round()).abs() < 1e-8);
            }
        } else {
            assert_eq!(value, 1.0);
        }
        // Rebuild the closure after inspecting the model, and reach both ends by pointer.
        for (at, expected) in [(hit.left_center(), min), (hit.right_center(), max)] {
            let mut draw = |ui: &mut Ui| {
                ui.add_enabled_ui(enabled, |ui| {
                    slider(ui, &mut value, min..=max, Slider { step: Some(0.05), show_value, integer, ..Default::default() });
                });
            };
            test_support::frame(&ctx, test_support::pointer(at, true), &mut draw);
            test_support::frame(&ctx, test_support::pointer(at, false), &mut draw);
            if enabled {
                assert!((value - expected).abs() < 1e-8);
            }
        }
    }
}

#[test]
fn continuous_slider_drawing_preserves_precision_and_all_scenario_aliases() {
    let ctx = crate::fonts::test_context();
    let mut value = 0.123456789;
    test_support::frame(&ctx, vec![], |ui| {
        row(ui, "Opacity", |ui| {
            slider(ui, &mut value, 0.0..=1.0, Default::default());
        });
    });
    assert_eq!(value, 0.123456789);
    let rect = scenario::rect_of("dialog:slider:Opacity").expect("opacity slider rect");
    assert!(rect.contains(scenario::handle_at("dialog:slider:Opacity").expect("opacity slider handle")));
    assert!(scenario::rect_of("dialog:widget:Opacity").is_some());
}

#[test]
fn formatted_slider_editor_fits_without_shifting_the_track_during_a_scale_drag() {
    let ctx = crate::fonts::test_context();
    let mut widths = Vec::new();
    for mut value in [0.5, 1.0, 1.5, 2.5] {
        test_support::frame(&ctx, vec![], |ui| {
            let right = ui.available_rect_before_wrap().right();
            let response =
                slider(ui, &mut value, 0.5..=2.5, Slider { step: Some(0.05), format: |v| format!("{:.0} %", v * 100.0), ..Default::default() });
            assert!(response.rect.right() <= right, "formatted editor must fit: {:?} > {right}", response.rect);
            widths.push(scenario::rect_of("dialog:slider_track:").expect("slider track").width());
        });
    }
    assert!(widths.windows(2).all(|pair| pair[0] == pair[1]), "dragging must not move the input range: {widths:?}");
}

/// The value box of a slider takes typed numbers (the per-axis speeds among them): Tab order reaches it
/// and Enter commits; a number past the range is held to the range.
#[test]
fn a_sliders_value_box_takes_typed_numbers_and_clamps_them_to_its_range() {
    use crate::ui_kit::test_support::*;
    for (typed, expected) in [("1.5", 1.5), ("9", 3.0), ("-4", 0.0), ("0", 0.0)] {
        let ctx = crate::fonts::test_context();
        let mut value = 1.0_f32;
        let mut draw = |ui: &mut Ui| {
            slider(ui, &mut value, 0.0..=3.0, Slider { step: Some(0.05), format: |v| format!("{v:.2}×"), ..Default::default() });
        };
        frame(&ctx, vec![], &mut draw);
        let track = scenario::rect_of("dialog:slider_track:").unwrap_or_else(|| scenario::rects("dialog:slider").first().expect("the slider").1);
        let at = egui::pos2(track.right() + 30.0, track.center().y);
        frame(&ctx, pointer(at, true), &mut draw);
        frame(&ctx, pointer(at, false), &mut draw);
        frame(&ctx, vec![egui::Event::Text(typed.into())], &mut draw);
        frame(&ctx, key(egui::Key::Enter), &mut draw);
        frame(&ctx, vec![], &mut draw);
        assert!((value - expected).abs() < 1e-3, "typed {typed}: {value}");
    }
}

/// At the top of its range egui's own handle reaches the right edge of the rectangle the kit
/// paints over it, so the mask has to pass that edge; flush with it, the handle's last column
/// survives as a light sliver beside the value box (#5708).
#[test]
fn the_mask_passes_the_edge_of_egui_s_own_handle_at_the_top_of_the_range() {
    let ctx = crate::fonts::test_context();
    let t = Tokens::get();
    let mut value: f64 = 2.0;
    let mut output =
        ctx.run_ui(egui::RawInput { screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(400.0, 200.0))), ..Default::default() }, |ui| {
            scope(ui, |ui| {
                slider(ui, &mut value, 0.0..=2.0, Slider { step: Some(0.05), ..Default::default() });
            });
        });
    output.textures_delta.clear();
    let handle = output
        .shapes
        .iter()
        .find_map(|s| match &s.shape {
            egui::Shape::Circle(c) if c.stroke.color != t.form_primary => Some(*c),
            _ => None,
        })
        .expect("egui's own slider handle");
    let mask = output
        .shapes
        .iter()
        .find_map(|s| match &s.shape {
            egui::Shape::Rect(r) if r.fill == t.dialog_body => Some(r.rect),
            _ => None,
        })
        .expect("the rectangle the kit paints over egui's slider");
    assert!(
        mask.right() > handle.center.x + handle.radius,
        "the mask ends at {} and egui's handle at {}: its last column would show",
        mask.right(),
        handle.center.x + handle.radius
    );
}
