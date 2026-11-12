use crate::test_support::*;
use crate::*;

use egui::Ui;
use solidcraft_ui_registry as registry;

#[test]
fn footer_clicks_and_keyboard_respect_validation_and_editor_focus() {
    for enabled in [false, true] {
        let ctx = crate::test_support::context();
        let mut action = Action::None;
        let mut draw = |ui: &mut Ui| {
            action = footer(ui, "OK", "Cancel", enabled, Footer::default());
        };
        frame(&ctx, vec![], &mut draw);
        let pos = registry::handle_at("dialog:ok").unwrap();
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
        let cancel = registry::handle_at("dialog:cancel").unwrap();
        frame(&ctx, pointer(cancel, true), |ui| action = footer(ui, "OK", "Cancel", enabled, Footer::default()));
        frame(&ctx, pointer(cancel, false), |ui| action = footer(ui, "OK", "Cancel", enabled, Footer::default()));
        assert_eq!(action, Action::Cancel);
    }
    let ctx = crate::test_support::context();
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
        let ctx = crate::test_support::context();
        let mut draw = |ui: &mut Ui| {
            footer(ui, "OK", "Cancel", enabled, Footer { apply: Some("Apply"), ..Default::default() });
        };
        frame(&ctx, vec![], &mut draw);
        let ok = registry::handle_at("dialog:ok").unwrap();
        // Settled over two frames: the hover is known from the first frame's layout.
        frame_cursor(&ctx, ok, &mut draw);
        assert_eq!(frame_cursor(&ctx, ok, &mut draw), expected, "OK, enabled {enabled}");
        // Apply is a button too, and Cancel is always enabled.
        let apply = registry::handle_at("dialog:apply").unwrap();
        frame_cursor(&ctx, apply, &mut draw);
        assert_eq!(frame_cursor(&ctx, apply, &mut draw), expected, "Apply, enabled {enabled}");
        let cancel = registry::handle_at("dialog:cancel").unwrap();
        frame_cursor(&ctx, cancel, &mut draw);
        assert_eq!(frame_cursor(&ctx, cancel, &mut draw), egui::CursorIcon::PointingHand, "Cancel is always enabled");
    }
}

#[test]
fn bounded_text_button_is_plain_truncates_and_keeps_its_full_accessible_name() {
    let label = "A design with a very long name that cannot fit in its row";
    for enabled in [false, true] {
        let ctx = crate::test_support::context();
        ctx.enable_accesskit();
        let response = std::cell::RefCell::new(None);
        let run = |events: Vec<egui::Event>| {
            raw_frame(
                &ctx,
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(500.0, 300.0))),
                    events,
                    focused: true,
                    ..Default::default()
                },
                |ui| {
                    scope(ui, |ui| {
                        *response.borrow_mut() = Some(text_button_bounded(ui, label, enabled, 120.0));
                    });
                },
            )
        };
        let output = run(vec![]);
        let rect = response.borrow().as_ref().unwrap().rect;
        assert!(rect.width() <= 120.0, "the name stays inside its column");
        assert!((rect.height() - solidcraft_theme::Tokens::get().control_height).abs() < 1.0, "the name shares the row's control height");
        let tree = output.platform_output.accesskit_update.as_ref().unwrap();
        let node = tree.nodes.iter().find(|(_, node)| node.role() == egui::accesskit::Role::Button && node.label() == Some(label)).unwrap();
        assert_eq!(node.1.is_disabled(), !enabled);
        let mut elided = false;
        let mut idle_frame = false;
        for clipped in &output.shapes {
            each_shape(&clipped.shape, &mut |shape| match shape {
                egui::Shape::Text(text) => elided |= text.galley.elided,
                egui::Shape::Rect(shape) if shape.rect == rect => {
                    idle_frame |= shape.stroke.width > 0.0 || shape.fill != egui::Color32::TRANSPARENT;
                }
                _ => {}
            });
        }
        assert!(elided, "a long name is truncated");
        assert!(!idle_frame, "a name has no idle button border or fill");
        run(vec![egui::Event::PointerMoved(rect.center())]);
        let hover = run(vec![egui::Event::PointerMoved(rect.center())]);
        assert_eq!(hover.platform_output.cursor_icon, if enabled { egui::CursorIcon::PointingHand } else { egui::CursorIcon::Default });
        run(pointer(rect.center(), true));
        run(pointer(rect.center(), false));
        assert_eq!(response.borrow().as_ref().unwrap().clicked(), enabled);
        assert!(!response.borrow().as_ref().unwrap().has_focus(), "pointer clicks leave no focus ring");
        run(vec![egui::Event::PointerGone]);
        run(key(egui::Key::Tab));
        assert_eq!(response.borrow().as_ref().unwrap().has_focus(), enabled, "keyboard focus remains available");
    }
}

#[test]
fn bounded_text_button_aligns_short_and_long_names_with_the_column_start() {
    let ctx = crate::test_support::context();
    let names = ["Bracket", "A design with a very long name that cannot fit in its row"];
    let mut rows = Vec::new();
    let output = raw_frame(
        &ctx,
        egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(500.0, 300.0))), ..Default::default() },
        |ui| {
            scope(ui, |ui| {
                for name in names {
                    let response = text_button_bounded(ui, name, true, 120.0);
                    rows.push((name, response.rect));
                }
            });
        },
    );
    for (name, rect) in rows {
        assert!((rect.width() - 120.0).abs() < 1.0, "the name keeps the full column width");
        let mut text_left = None;
        for clipped in &output.shapes {
            each_shape(&clipped.shape, &mut |shape| {
                if let egui::Shape::Text(text) = shape
                    && text.galley.job.text == name
                {
                    text_left = Some(text.pos.x);
                }
            });
        }
        assert!((text_left.unwrap() - rect.left()).abs() < 1.0, "{name}: names start at the same left edge as their paths");
    }
}

/// With nothing to apply (#8561), Apply is disabled for the pointer, the keyboard and
/// accessibility, and drawn with the theme's dim ink; with a change it works as before. OK and
/// Cancel are not affected. The same holds for a confirmation's third action on the left.
#[test]
fn apply_with_nothing_to_apply_is_disabled_for_pointer_keyboard_and_accessibility() {
    use solidcraft_theme::Tokens;
    for confirm in [false, true] {
        for nothing_to_apply in [true, false] {
            let live = !nothing_to_apply;
            let ctx = crate::test_support::context();
            ctx.enable_accesskit();
            let action = std::cell::Cell::new(Action::None);
            let run = |events: Vec<egui::Event>| {
                registry::clear_handles();
                let input = egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(320.0, 400.0))),
                    events,
                    focused: true,
                    ..Default::default()
                };
                crate::test_support::raw_frame(&ctx, input, |ui| {
                    ui.set_width(280.0);
                    scope(ui, |ui| {
                        let options = Footer { apply: Some("Apply"), confirm, nothing_to_apply, ..Default::default() };
                        action.set(footer(ui, "OK", "Cancel", true, options));
                    });
                })
            };
            let case = format!("confirm {confirm}, nothing to apply {nothing_to_apply}");
            let output = run(vec![]);
            assert_eq!(registry::count_of("dialog:apply:enabled"), Some(f64::from(live)), "{case}");
            assert_eq!(registry::count_of("dialog:ok:enabled"), Some(1.0), "{case}: OK keeps its own state");
            let node = output
                .platform_output
                .accesskit_update
                .as_ref()
                .and_then(|update| update.nodes.iter().find(|(_, node)| node.label() == Some("Apply")))
                .map(|(_, node)| node.clone())
                .unwrap_or_else(|| panic!("{case}: Apply is an accessible button"));
            assert_eq!(node.role(), egui::accesskit::Role::Button, "{case}");
            assert_eq!(node.is_disabled(), nothing_to_apply, "{case}: accessibility reports the state");
            if !confirm {
                let ink = crate::test_support::text_shapes(&output)
                    .find(|text| text.galley.job.text == "Apply")
                    .and_then(|text| text.galley.job.sections.first().map(|section| section.format.color))
                    .unwrap_or_else(|| panic!("{case}: Apply's label is painted"));
                let t = Tokens::get();
                assert_eq!(ink, if live { t.form_ink } else { t.form_dim }, "{case}: a disabled Apply looks disabled");
            }

            // The pointer: no hand and no click without something to apply.
            let apply = registry::handle_at("dialog:apply").unwrap_or_else(|| panic!("{case}: Apply has a handle"));
            run(vec![egui::Event::PointerMoved(apply)]);
            let cursor = run(vec![egui::Event::PointerMoved(apply)]).platform_output.cursor_icon;
            assert_eq!(cursor, if live { egui::CursorIcon::PointingHand } else { egui::CursorIcon::Default }, "{case}");
            run(pointer(apply, true));
            run(pointer(apply, false));
            assert_eq!(action.get(), if live { Action::Apply } else { Action::None }, "{case}: click");

            // The keyboard: Tab never lands on a disabled Apply, so Space/Enter can't press it.
            ctx.memory_mut(|m| {
                if let Some(id) = m.focused() {
                    m.surrender_focus(id);
                }
            });
            let mut focused = Vec::new();
            for _ in 0..4 {
                for event in run(key(egui::Key::Tab)).platform_output.events {
                    if let egui::output::OutputEvent::FocusGained(info) = event {
                        focused.push(info.label.unwrap_or_default());
                    }
                }
            }
            assert!(focused.iter().any(|label| label == "Cancel"), "{case}: Tab reaches the footer: {focused:?}");
            assert_eq!(focused.iter().any(|label| label == "Apply"), live, "{case}: Tab and Apply: {focused:?}");
        }
    }
}

#[test]
fn positioned_shared_buttons_center_text_and_respect_disabled_clicks_in_both_themes() {
    use egui::{Pos2, RawInput, Rect, RichText, Shape, vec2};
    use solidcraft_theme::Tokens;
    let previous = solidcraft_theme::is_dark();
    for dark in [false, true] {
        solidcraft_theme::set_dark(dark);
        for enabled in [false, true] {
            let ctx = crate::test_support::context();
            solidcraft_theme::apply(&ctx);
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
                                    RichText::new(label).font(crate::type_face::semibold(Tokens::get().font_button)),
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
                    assert!(solidcraft_theme::contrast_ratio(ink, Tokens::get().form_primary) >= 4.5, "primary label remains legible in both themes");
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
    solidcraft_theme::set_dark(previous);
}
