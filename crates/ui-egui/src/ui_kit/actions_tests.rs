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

/// With nothing to apply (#8561), Apply is disabled for the pointer, the keyboard and
/// accessibility, and drawn with the theme's dim ink; with a change it works as before. OK and
/// Cancel are not affected. The same holds for a confirmation's third action on the left.
#[test]
fn apply_with_nothing_to_apply_is_disabled_for_pointer_keyboard_and_accessibility() {
    use crate::theme::Tokens;
    for confirm in [false, true] {
        for nothing_to_apply in [true, false] {
            let live = !nothing_to_apply;
            let ctx = crate::fonts::test_context();
            ctx.enable_accesskit();
            let action = std::cell::Cell::new(Action::None);
            let run = |events: Vec<egui::Event>| {
                scenario::clear_handles();
                let input = egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(320.0, 400.0))),
                    events,
                    focused: true,
                    ..Default::default()
                };
                crate::test_support::frame(&ctx, input, |ui| {
                    ui.set_width(280.0);
                    scope(ui, |ui| {
                        let options = Footer { apply: Some("Apply"), confirm, nothing_to_apply, ..Default::default() };
                        action.set(footer(ui, "OK", "Cancel", true, options));
                    });
                })
            };
            let case = format!("confirm {confirm}, nothing to apply {nothing_to_apply}");
            let output = run(vec![]);
            assert_eq!(scenario::count_of("dialog:apply:enabled"), Some(f64::from(live)), "{case}");
            assert_eq!(scenario::count_of("dialog:ok:enabled"), Some(1.0), "{case}: OK keeps its own state");
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
            let apply = scenario::handle_at("dialog:apply").unwrap_or_else(|| panic!("{case}: Apply has a handle"));
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

#[test]
fn one_bit_primary_actions_keep_inverted_labels_and_legacy_fills() {
    use crate::theme::{Tokens, scoped_tokens};
    use egui::{Color32, Shape};

    for base in [crate::theme_files::Base::Dark, crate::theme_files::Base::Light] {
        let old = crate::theme_files::parse(&format!(
            r##"{{"format":"solidcraft-theme/1","name":"Old","base":"{}","colors":{{"form_primary":"#123456"}}}}"##,
            if base.is_dark() { "dark" } else { "light" }
        ))
        .unwrap();
        let t = crate::theme_files::resolve(&old);
        assert_eq!(t.form_primary_fill, t.form_primary);
    }
    for theme in crate::theme_files::builtin().iter().filter(|theme| !crate::theme_files::is_two_colour(&theme.id)) {
        let t = crate::theme_files::resolve(theme);
        assert_eq!(t.form_primary_fill, t.form_primary, "{} retains its primary fill", theme.id);
    }
    // 1-bit's primary actions are black with white labels; 1-bit Dark's the reverse.
    for (id, fill, ink) in [("1-bit", Color32::BLACK, Color32::WHITE), ("1-bit-dark", Color32::WHITE, Color32::BLACK)] {
        let t = crate::theme_files::resolve(crate::theme_files::find(id, &[]).unwrap());
        let _restore = scoped_tokens(t);
        assert_eq!(Tokens::get().form_primary_fill, fill, "{id}");
        let ctx = crate::fonts::test_context();
        let mut rects = Vec::new();
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            ui.set_width(400.0);
            scope(ui, |ui| {
                rects.push(primary_button(ui, "Primary").rect);
                rects.push(wide_primary_button(ui, "Wide").rect);
                footer(ui, "OK", "Cancel", true, Footer::default());
                rects.push(scenario::rect_of("dialog:ok").unwrap());
            });
        });
        output.textures_delta.clear();
        for rect in rects {
            assert!(
                output.shapes.iter().any(|shape| matches!(&shape.shape, Shape::Rect(r) if rect.contains_rect(r.rect) && r.fill == fill)),
                "{id}: {rect:?} has its fill"
            );
            assert!(
                output.shapes.iter().any(|shape| matches!(&shape.shape, Shape::Text(text) if rect.contains(text.pos) && text.galley.job.sections.iter().all(|section| section.format.color == ink))),
                "{id}: {rect:?} has its label ink"
            );
        }
    }
}

#[test]
fn persistent_native_selected_recipe_is_opt_in_and_preserves_small_selected_controls() {
    fn flatten(shape: &egui::Shape, out: &mut Vec<egui::Shape>) {
        match shape {
            egui::Shape::Vec(shapes) => {
                for s in shapes {
                    flatten(s, out);
                }
            }
            s => out.push(s.clone()),
        }
    }
    let mut themes: Vec<_> = [false, true]
        .into_iter()
        .flat_map(|dark| crate::theme::ColorVision::ALL.into_iter().map(move |vision| crate::theme::Tokens::themed(dark, vision)))
        .collect();
    themes.extend(crate::theme_files::builtin().iter().map(crate::theme_files::resolve).filter(|t| t.borders.bevel.is_some()));
    assert!(themes.iter().any(|t| t.borders.bevel.is_some()), "include a real bevel theme");
    for t in themes {
        let _theme = crate::theme::scoped_tokens(t);
        let ctx = crate::fonts::test_context();
        let mut selected = egui::Shape::Noop;
        let mut native = egui::Rect::NOTHING;
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            let response = button(ui, "Existing selected", Button::SmallSelected);
            native = response.rect;
            selected = persistent_button_shape(ui, &response, response.rect);
        });
        output.textures_delta.clear();
        let mut shapes = Vec::new();
        flatten(&selected, &mut shapes);
        assert!(shapes.iter().any(|s| matches!(s, egui::Shape::Rect(r) if r.rect == native && r.fill == t.selected_bg)));
        assert!(shapes.iter().any(|s| matches!(s, egui::Shape::LineSegment { stroke, .. } if stroke.width == 2.0 && stroke.color == t.accent)));
        // Existing SmallSelected keeps its old fill (including the bevel policy), regular
        // label family and no persistent underline. Merely asking for the opt-in shape
        // cannot alter that widget or its native geometry.
        let mut old = Vec::new();
        for shape in output.shapes {
            flatten(&shape.shape, &mut old);
        }
        let fill = if t.borders.bevel.is_some() { t.control_fill } else { t.selected_bg };
        assert!(old.iter().any(|s| matches!(s, egui::Shape::Rect(r) if r.rect == native && r.fill == fill)));
        assert!(!old.iter().any(|s| matches!(s, egui::Shape::LineSegment { stroke, .. } if stroke.width == 2.0 && stroke.color == t.accent)));
        let text = old
            .iter()
            .find_map(|s| match s {
                egui::Shape::Text(s) if s.galley.text() == "Existing selected" => Some(s),
                _ => None,
            })
            .unwrap();
        assert_eq!(text.galley.job.sections[0].format.font_id.family, egui::FontFamily::Proportional);
    }
}
