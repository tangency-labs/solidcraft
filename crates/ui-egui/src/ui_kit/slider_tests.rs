use super::*;
use crate::scenario;
use egui::{Rect, Ui, pos2, vec2};
use solidcraft_theme::Tokens;

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

    let previous = solidcraft_theme::is_dark();
    for dark in [false, true] {
        solidcraft_theme::set_dark(dark);
        for language in ["en", "de"] {
            let _language = solidcraft_ui_text::scope(language);
            let t = Tokens::get();
            let mut undo_width = 0.0;
            let mut dismiss_width = 0.0;
            let mut label_wrap_width = 0.0;
            test_support::frame(&crate::fonts::test_context(), vec![], |ui| {
                undo_width = ContentWidth::button(ui, solidcraft_ui_text::tr("ui.undo"));
                dismiss_width = ContentWidth::button(ui, solidcraft_ui_text::tr("prefs.theme_notice_dismiss"));
                label_wrap_width = (ContentWidth::label(ui, solidcraft_ui_text::tr("prefs.theme_notice_dismiss")) / 3.0).ceil();
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
    solidcraft_theme::set_dark(previous);
}

#[test]
fn blur_meter_boundaries_and_off_thumb() {
    for (radius, bars) in [(0.0, 0), (1.0, 1), (8.0, 1), (9.0, 2), (18.0, 3), (33.0, 5), (40.0, 5), (60.0, 5)] {
        assert_eq!(blur_cost_bars(radius), bars, "{radius}");
    }
    for dark in [false, true] {
        solidcraft_theme::set_dark(dark);
        for radius in [0.0, 18.25, 40.0] {
            let ctx = crate::fonts::test_context();
            let t = Tokens::get();
            let mut value = radius;
            let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
                scope(ui, |ui| {
                    slider(ui, &mut value, 0.0..=40.0, Slider { step: Some(1.0), show_value: false, off_endpoint: true, ..Default::default() });
                });
            });
            out.textures_delta.clear();
            assert_eq!(value, radius, "drawing cannot round persisted fractional values");
            assert_eq!(out.shapes.iter().filter(|s| matches!(s.shape, egui::Shape::Circle(_))).count(), 1, "off/solid thumb has no native duplicate");
            let hit = scenario::rect_of("dialog:slider_track:").expect("blur slider track");
            assert_eq!(
                out.shapes.iter().filter(|s| matches!(&s.shape, egui::Shape::Rect(r) if r.rect.left() < hit.right())).count(),
                2,
                "off/solid track has no native rail or backing rectangle"
            );
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
    let previous = solidcraft_theme::is_dark();
    for dark in [false, true] {
        solidcraft_theme::set_dark(dark);
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
    solidcraft_theme::set_dark(previous);
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

/// At either endpoint there must be just the token thumb and track: a native handle's
/// last column used to survive beside the value box at the maximum (#5708).
#[test]
fn slider_endpoints_have_no_duplicate_native_handle_or_rail_and_keep_the_editor() {
    let previous = solidcraft_theme::is_dark();
    for dark in [false, true] {
        solidcraft_theme::set_dark(dark);
        for show_value in [false, true] {
            for endpoint in [0.0, 2.0] {
                let ctx = crate::fonts::test_context();
                let mut value = endpoint;
                let output = slider_test_frame(&ctx, vec![], |ui| {
                    slider(ui, &mut value, 0.0..=2.0, Slider { step: Some(0.05), show_value, ..Default::default() });
                });
                let hit = scenario::rect_of("dialog:slider_track:").expect("slider track");
                assert_slider_paint(&output.shapes, hit, Tokens::get(), endpoint, show_value, 1.0);
                assert_eq!(value, endpoint);
            }
        }
    }
    solidcraft_theme::set_dark(previous);
}

#[test]
fn narrow_sliders_do_not_retain_native_chrome_when_the_handle_range_reverses() {
    let previous = solidcraft_theme::is_dark();
    for dark in [false, true] {
        solidcraft_theme::set_dark(dark);
        for width in [24.0, 28.0] {
            for saved in [0.0, 0.73123456789, 2.0] {
                let ctx = crate::fonts::test_context();
                let mut value = saved;
                let output = slider_test_frame(&ctx, vec![], |ui| {
                    ui.set_width(width);
                    slider(ui, &mut value, 0.0..=2.0, Slider { show_value: false, ..Default::default() });
                });
                let hit = scenario::rect_of("dialog:slider_track:").expect("narrow slider track");
                assert!(hit.width() < 28.0, "the fixture must constrain the native track");
                assert_slider_paint(&output.shapes, hit, Tokens::get(), saved, false, 1.0);
                assert_eq!(value, saved);
            }
        }
    }
    solidcraft_theme::set_dark(previous);
}

fn slider_test_frame(ctx: &egui::Context, events: Vec<egui::Event>, mut add: impl FnMut(&mut Ui)) -> egui::FullOutput {
    scenario::clear_handles();
    crate::test_support::frame(
        ctx,
        egui::RawInput { screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(400.0, 200.0))), events, focused: true, ..Default::default() },
        |ui| {
            ui.set_width(280.0);
            scope(ui, &mut add);
        },
    )
}

/// Assert observable paint geometry, including absence of a surface-coloured backing.
/// `shapes` is only the slider's paint span; parent/sibling paints are checked separately.
fn assert_slider_paint(shapes: &[egui::epaint::ClippedShape], hit: Rect, t: Tokens, value: f64, show_value: bool, opacity: f32) {
    let track = Rect::from_center_size(hit.center(), vec2((hit.width() - 2.0 * hit.height() / 2.5).max(0.0), t.space_4));
    let center = pos2(egui::lerp(track.x_range(), (value / 2.0) as f32), track.center().y);
    let circles: Vec<_> = shapes.iter().filter_map(|s| if let egui::Shape::Circle(c) = &s.shape { Some(c) } else { None }).collect();
    assert_eq!(circles.len(), 1, "no duplicate native handle, even at the very edge");
    let thumb = circles[0];
    assert_eq!(thumb.center, center, "the custom thumb reaches the native input endpoints");
    assert_eq!(thumb.radius, t.space_8);
    assert_eq!(thumb.fill, t.control_fill.gamma_multiply(opacity));
    assert_eq!(thumb.stroke, egui::Stroke::new(t.space_2, t.form_primary.gamma_multiply(opacity)));
    let rails: Vec<_> = shapes
        .iter()
        .filter_map(|s| match &s.shape {
            egui::Shape::Rect(r) if r.rect.left() < hit.right() => Some(r),
            _ => None,
        })
        .collect();
    assert_eq!(rails.len(), 2, "only the token track and filled portion; no native rail or backing rectangle");
    assert_eq!(rails[0].rect, track);
    assert_eq!(rails[0].fill, t.dialog_rule.gamma_multiply(opacity));
    assert_eq!(rails[1].rect, Rect::from_min_max(track.min, pos2(center.x, track.bottom())));
    assert_eq!(rails[1].fill, t.form_primary.gamma_multiply(opacity));
    let editor: Vec<_> = shapes.iter().filter_map(|s| if let egui::Shape::Text(text) = &s.shape { Some(text) } else { None }).collect();
    if show_value {
        assert!(
            editor.iter().any(|text| text.galley.text() == format!("{value:.2}")),
            "the native value editor must remain painted: {:?}",
            editor.iter().map(|text| text.galley.text()).collect::<Vec<_>>()
        );
        assert!(editor.iter().all(|text| text.pos.x > hit.right()), "the editor must remain beside the track");
        assert!(shapes.iter().any(|s| matches!(&s.shape, egui::Shape::Rect(r) if r.rect.left() > hit.right())), "native editor chrome survives");
    } else {
        assert!(editor.is_empty(), "show_value(false) still hides the editor");
    }
}

#[test]
fn slider_preserves_translucent_parent_paints_and_inherited_opacity_without_a_backing_rectangle() {
    let previous = solidcraft_theme::is_dark();
    for dark in [false, true] {
        solidcraft_theme::set_dark(dark);
        for translucent in [false, true] {
            let mut t = Tokens::get();
            if translucent {
                t.apply_panel_opacity(40.0);
                assert!(t.dialog_body.a() < 255);
            }
            let _tokens = solidcraft_theme::scoped_tokens(t);
            for opacity in [1.0, 0.45] {
                for show_value in [false, true] {
                    for saved in [0.0, 0.73123456789, 2.0] {
                        let ctx = crate::fonts::test_context();
                        let mut value = saved;
                        slider_test_frame(&ctx, vec![], |ui| {
                            slider(ui, &mut value, 0.0..=2.0, Slider { show_value, ..Default::default() });
                        });
                        let hit = scenario::rect_of("dialog:slider_track:").expect("slider track");
                        slider_test_frame(&ctx, vec![], |ui| {
                            ui.multiply_opacity(opacity);
                            // A synthetic parent callback stands in for background effects;
                            // this test observes paint retention, without invoking a GPU.
                            ui.painter().add(egui::Shape::Vec(vec![
                                egui::Shape::rect_filled(hit.expand(12.0), t.control_radius, t.dialog_body),
                                egui::Shape::Callback(egui::PaintCallback { rect: hit, callback: std::sync::Arc::new(()) }),
                            ]));
                            // Earlier sibling paints deliberately have the native signature.
                            ui.painter().rect_filled(
                                Rect::from_center_size(hit.center(), vec2(hit.width(), ui.spacing().slider_rail_height)),
                                t.control_radius,
                                t.control_fill,
                            );
                            ui.painter().circle_filled(hit.center(), hit.height() / 2.5, t.control_fill);
                            let layer = ui.layer_id();
                            let parent = ui.ctx().graphics(|g| g.get(layer).expect("parent paints").all_entries().cloned().collect::<Vec<_>>());
                            slider(ui, &mut value, 0.0..=2.0, Slider { show_value, ..Default::default() });
                            assert_eq!(ui.opacity(), opacity, "slider must retain the parent's painter opacity");
                            ui.ctx().graphics(|g| {
                                let shapes: Vec<_> = g.get(layer).expect("slider paints").all_entries().cloned().collect();
                                assert_eq!(&shapes[..parent.len()], &parent, "parent callback/fill and earlier siblings must survive unchanged");
                                assert_slider_paint(&shapes[parent.len()..], hit, t, saved, show_value, opacity);
                            });
                            ui.painter().circle_filled(hit.right_center(), t.space_2, t.form_dim);
                            assert_eq!(ui.opacity(), opacity, "following siblings inherit the same opacity");
                        });
                        assert_eq!(value, saved, "painting preserves fractional saved values");
                    }
                }
            }
        }
    }
    solidcraft_theme::set_dark(previous);
}

#[test]
fn clipping_out_the_native_track_preserves_the_editor_when_the_paint_signature_differs() {
    let previous = solidcraft_theme::is_dark();
    for dark in [false, true] {
        solidcraft_theme::set_dark(dark);
        let ctx = crate::fonts::test_context();
        let mut value = 1.0;
        slider_test_frame(&ctx, vec![], |ui| {
            slider(ui, &mut value, 0.0..=2.0, Default::default());
        });
        let hit = scenario::rect_of("dialog:slider_track:").expect("slider track");
        let output = slider_test_frame(&ctx, vec![], |ui| {
            // egui omits its rail/handle when only the editor intersects the clip.
            // The first native shapes are now editor paints and must survive.
            ui.set_clip_rect(ui.clip_rect().with_min_x(hit.right() + 1.0));
            slider(ui, &mut value, 0.0..=2.0, Default::default());
        });
        assert!(output.shapes.iter().any(|s| matches!(&s.shape, egui::Shape::Rect(r) if r.rect.left() > hit.right())));
        assert!(output.shapes.iter().any(|s| matches!(&s.shape, egui::Shape::Text(text) if text.galley.text() == "1.00")));
        assert_eq!(output.shapes.iter().filter(|s| matches!(s.shape, egui::Shape::Circle(_))).count(), 1, "only the clipped custom thumb is added");
        assert_eq!(value, 1.0);
    }
    solidcraft_theme::set_dark(previous);
}

#[test]
fn slider_keyboard_focus_and_accessibility_keep_native_steps_and_editor_focus() {
    let previous = solidcraft_theme::is_dark();
    for dark in [false, true] {
        solidcraft_theme::set_dark(dark);
        for translucent in [false, true] {
            let mut t = Tokens::get();
            if translucent {
                t.apply_panel_opacity(40.0);
            }
            let _tokens = solidcraft_theme::scoped_tokens(t);
            for show_value in [false, true] {
                let ctx = crate::fonts::test_context();
                ctx.enable_accesskit();
                let value = std::cell::Cell::new(1.0_f64);
                let draw = |events| {
                    slider_test_frame(&ctx, events, |ui| {
                        let mut next = value.get();
                        slider(ui, &mut next, 0.0..=2.0, Slider { step: Some(0.05), show_value, ..Default::default() });
                        value.set(next);
                    })
                };
                draw(vec![]);
                let idle = draw(vec![]);
                let tree = idle.platform_output.accesskit_update.as_ref().expect("native accessibility tree");
                let nodes: Vec<_> = tree.nodes.iter().filter(|(_, n)| n.role() == egui::accesskit::Role::Slider).collect();
                assert_eq!(nodes.len(), 1);
                let node = &nodes[0].1;
                assert_eq!(node.numeric_value(), Some(1.0));
                assert_eq!(node.min_numeric_value(), Some(0.0));
                assert_eq!(node.max_numeric_value(), Some(2.0));
                assert_eq!(node.numeric_value_step(), Some(0.05));
                assert!(!node.is_disabled());
                draw(test_support::key(egui::Key::Tab));
                draw(vec![]);
                let focus = ctx.memory(|m| m.focused()).expect("Tab focuses the slider");
                let focused = ctx.read_response(focus).expect("focused slider response");
                assert!(focused.has_focus());
                let hit = scenario::rect_of("dialog:slider_track:").expect("slider track");
                assert!(hit.contains(focused.rect.center()));
                let changed = draw(test_support::key(egui::Key::ArrowRight));
                assert!((value.get() - 1.05).abs() < 1e-8);
                assert_eq!(ctx.memory(|m| m.focused()), Some(focus));
                assert_slider_paint(&changed.shapes, hit, Tokens::get(), value.get(), show_value, 1.0);
                draw(test_support::key(egui::Key::ArrowLeft));
                assert!((value.get() - 1.0).abs() < 1e-8);
                if show_value {
                    draw(test_support::key(egui::Key::Tab));
                    let editing = draw(vec![]);
                    let editor_id = ctx.memory(|m| m.focused()).expect("Tab reaches the native editor");
                    assert_ne!(editor_id, focus);
                    assert!(egui::TextEdit::load_state(&ctx, editor_id).is_some());
                    assert!(editing.shapes.iter().any(|s| matches!(&s.shape, egui::Shape::Text(text) if text.pos.x > hit.right())));
                    draw(vec![egui::Event::Text("1.75".into())]);
                    draw(test_support::key(egui::Key::Enter));
                    draw(vec![]);
                    assert!((value.get() - 1.75).abs() < 1e-8);
                }
                // The same native hit range still accepts pointer input after keyboard/editing.
                for (at, expected) in [(hit.left_center(), 0.0), (hit.center(), 1.0), (hit.right_center(), 2.0)] {
                    draw(test_support::pointer(at, true));
                    let released = draw(test_support::pointer(at, false));
                    assert!((value.get() - expected).abs() < 1e-8);
                    assert_slider_paint(&released.shapes, hit, Tokens::get(), expected, show_value, 1.0);
                }
            }
        }
    }
    solidcraft_theme::set_dark(previous);
}

#[test]
fn disabled_and_invisible_sliders_keep_native_input_and_paint_suppression() {
    let previous = solidcraft_theme::is_dark();
    for dark in [false, true] {
        solidcraft_theme::set_dark(dark);
        for show_value in [false, true] {
            for visible in [false, true] {
                let ctx = crate::fonts::test_context();
                ctx.enable_accesskit();
                let value = std::cell::Cell::new(0.73123456789);
                let draw = |events| {
                    slider_test_frame(&ctx, events, |ui| {
                        ui.add_enabled_ui(false, |ui| {
                            if !visible {
                                ui.set_invisible();
                            }
                            let mut next = value.get();
                            let response = slider(ui, &mut next, 0.0..=2.0, Slider { show_value, ..Default::default() });
                            assert!(!response.enabled());
                            assert!(!response.changed());
                            value.set(next);
                        });
                    })
                };
                draw(vec![]);
                let idle = draw(vec![]);
                let hit = scenario::rect_of("dialog:slider_track:").expect("hidden slider still allocates its track");
                if visible {
                    let tree = idle.platform_output.accesskit_update.as_ref().expect("disabled accessibility tree");
                    assert!(tree.nodes.iter().any(|(_, n)| n.role() == egui::accesskit::Role::Slider && n.is_disabled()));
                    let circles: Vec<_> =
                        idle.shapes.iter().filter_map(|s| if let egui::Shape::Circle(c) = &s.shape { Some(c) } else { None }).collect();
                    assert_eq!(circles.len(), 1, "disabled slider still has only the token thumb");
                    assert_eq!(circles[0].radius, Tokens::get().space_8);
                    assert_ne!(circles[0].fill, Tokens::get().control_fill, "native disabled fade remains active");
                } else {
                    assert!(idle.shapes.iter().all(|s| matches!(s.shape, egui::Shape::Noop)), "invisible slider paints no chrome or editor");
                }
                draw(test_support::pointer(hit.right_center(), true));
                draw(test_support::pointer(hit.right_center(), false));
                draw(test_support::key(egui::Key::Tab));
                draw(test_support::key(egui::Key::ArrowRight));
                draw(vec![egui::Event::Text("2".into())]);
                draw(test_support::key(egui::Key::Enter));
                assert_eq!(value.get(), 0.73123456789);
                assert!(ctx.memory(|m| m.focused()).is_none(), "disabled/invisible slider cannot take focus");
            }
        }
    }
    solidcraft_theme::set_dark(previous);
}
