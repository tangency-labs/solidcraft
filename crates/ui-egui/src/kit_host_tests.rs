//! The kit's controls with SolidCraft's data: its shipped themes, icons, fonts, menus and panel
//! host. The kit's own tests draw from plain data; these need the host.

/// Primary and selected actions in SolidCraft's shipped themes.
mod actions {
    use crate::ui_kit::*;
    use solidcraft_ui_registry as registry;

    #[test]
    fn one_bit_primary_actions_keep_inverted_labels_and_legacy_fills() {
        crate::test_support::host_data();
        use egui::{Color32, Shape};
        use solidcraft_theme::{Tokens, scoped_tokens};

        for base in [crate::theme_files::Base::Dark, crate::theme_files::Base::Light] {
            let old = crate::theme_files::parse(&format!(
                r##"{{"format":"solidcraft-theme/1","name":"Old","base":"{}","colors":{{"form_primary":"#123456"}}}}"##,
                if base.is_dark() { "dark" } else { "light" }
            ))
            .unwrap();
            let t = crate::theme_files::resolve(&old);
            assert_eq!(t.form_primary_fill, t.form_primary);
        }
        for theme in crate::theme_files::builtin().iter().filter(|theme| !crate::test_support::is_two_colour(&theme.id)) {
            let t = crate::theme_files::resolve(theme);
            assert_eq!(t.form_primary_fill, t.form_primary, "{} retains its primary fill", theme.id);
        }
        // 1-bit's primary actions are black with white labels; 1-bit Dark's the reverse.
        for (id, fill, ink) in [("1-bit", Color32::BLACK, Color32::WHITE), ("1-bit-dark", Color32::WHITE, Color32::BLACK)] {
            let t = crate::theme_files::resolve(crate::theme_files::find(id, &[]).unwrap());
            let _restore = scoped_tokens(t);
            assert_eq!(Tokens::get().form_primary_fill, fill, "{id}");
            let ctx = crate::ui_kit::test_support::context();
            let mut rects = Vec::new();
            let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                ui.set_width(400.0);
                scope(ui, |ui| {
                    rects.push(primary_button(ui, "Primary").rect);
                    rects.push(wide_primary_button(ui, "Wide").rect);
                    footer(ui, "OK", "Cancel", true, Footer::default());
                    rects.push(registry::rect_of("dialog:ok").unwrap());
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
        crate::test_support::host_data();
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
            .flat_map(|dark| solidcraft_theme::ColorVision::ALL.into_iter().map(move |vision| solidcraft_theme::Tokens::themed(dark, vision)))
            .collect();
        themes.extend(crate::theme_files::builtin().iter().map(crate::theme_files::resolve).filter(|t| t.borders.bevel.is_some()));
        assert!(themes.iter().any(|t| t.borders.bevel.is_some()), "include a real bevel theme");
        for t in themes {
            let _theme = solidcraft_theme::scoped_tokens(t);
            let ctx = crate::ui_kit::test_support::context();
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
}

/// A checkbox with one of SolidCraft's icons, measured with its fonts.
mod checks {
    use crate::ui_kit::test_support::*;
    use crate::ui_kit::*;
    use egui::{Rect, Shape, vec2};
    use solidcraft_theme::Tokens;

    /// Customize Toolbar's rows (#11471): the command's icon sits between the box and the label,
    /// is drawn from its artwork, and a click on it toggles the row like a click on the label.
    #[test]
    fn checkbox_icon_draws_the_icon_between_box_and_label_and_toggles_on_it() {
        let t = Tokens::get();
        let icon = solidcraft_engine::find_command("solid.extrude").map(|c| c.icon).unwrap_or_default();
        assert!(!icon.is_empty());
        let ctx = crate::fonts::test_context();
        let viewport = Rect::from_min_size(egui::Pos2::ZERO, vec2(400.0, 120.0));
        let run = |events: Vec<egui::Event>, on: &mut bool| {
            let (mut plain_width, mut row) = (0.0, Rect::NOTHING);
            let mut output = ctx.run_ui(egui::RawInput { screen_rect: Some(viewport), events, ..Default::default() }, |ui| {
                plain_width = checkbox(ui, "Extrude", &mut false).rect.width();
                row = checkbox_icon(ui, "Extrude", icon, on).rect;
            });
            output.textures_delta.clear();
            (output, plain_width, row)
        };
        let mut on = false;
        let (output, plain_width, row) = run(vec![], &mut on);
        assert_eq!(row.width(), plain_width + t.space_6 + t.space_16, "the icon takes its own slot");
        let slot = Rect::from_min_max(
            egui::pos2(row.left() + t.space_14, row.top()),
            egui::pos2(row.left() + t.space_14 + t.space_6 * 2.0 + t.space_16, row.bottom()),
        );
        assert!(
            output.shapes.iter().any(|clipped| matches!(&clipped.shape,
                Shape::Mesh(mesh) if mesh.texture_id != egui::TextureId::default() && slot.contains_rect(mesh.calc_bounds()))),
            "the icon's artwork is drawn between the box and the label"
        );
        let at = slot.center();
        run(pointer(at, true), &mut on);
        run(pointer(at, false), &mut on);
        assert!(on, "a click on the icon toggles the row");

        // A wrapped label leaves room for the icon: the row stays inside a narrow card.
        let (mut wrapped, mut single) = (Rect::NOTHING, Rect::NOTHING);
        let label = "A label long enough that it has to wrap in a narrow card";
        let mut output = ctx.run_ui(egui::RawInput { screen_rect: Some(viewport), ..Default::default() }, |ui| {
            ui.set_max_width(120.0);
            single = checkbox_icon(ui, "Extrude", icon, &mut false).rect;
            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
            wrapped = checkbox_icon(ui, label, icon, &mut false).rect;
        });
        output.textures_delta.clear();
        assert!(wrapped.width() <= 120.0, "{wrapped:?}");
        assert!(wrapped.height() > single.height(), "{wrapped:?} {single:?}");
    }
}

/// Fields with SolidCraft's data: its view-style menu, its themes and its expressions.
mod fields {
    use crate::ui_kit::test_support::*;
    use crate::ui_kit::*;
    use egui::Ui;
    use solidcraft_ui_registry as registry;

    #[test]
    fn dropdown_popups_own_escape_and_return_focus_without_canceling_the_window() {
        use std::cell::Cell;
        let previous = solidcraft_theme::is_dark();
        for dark in [false, true] {
            solidcraft_theme::set_dark(dark);
            for kind in ["plain", "icons", "compact", "editable"] {
                let ctx = crate::ui_kit::test_support::context();
                let action = Cell::new(Action::None);
                let trigger = Cell::new(None);
                let selected = Cell::new(0);
                let mut draw = |ui: &mut Ui| {
                    let ctx = ui.ctx().clone();
                    action.set(
                        Window::new(egui::Id::unique(("dropdown_escape", kind)), "settings", "Preferences")
                            .cards(1.0)
                            .fields()
                            .actions("OK", "Cancel", None)
                            .show(&ctx, true, |ui| {
                                // An earlier, closed dropdown must not steal this popup's Escape or focus.
                                row(ui, "Units", |ui| {
                                    dropdown(ui, "units", &mut 0, &["mm", "in"]);
                                });
                                row(ui, "Visual style", |ui| {
                                    let options = crate::navigation_slots::VISUAL_STYLES.map(|(_, label)| label);
                                    let icons = crate::navigation_slots::VISUAL_STYLES
                                        .iter()
                                        .enumerate()
                                        .map(|(index, (_, label))| (*label, crate::navigation_slots::visual_style_icon(index as u8)))
                                        .collect::<Vec<_>>();
                                    let mut choice = selected.get();
                                    let choices = |ui: &mut Ui| {
                                        for (index, option) in options.iter().enumerate() {
                                            let response = ui.selectable_label(selected.get() == index, *option);
                                            registry::publish_target(&format!("dialog:Visual style:{option}"), response.rect);
                                            if response.clicked() {
                                                selected.set(index);
                                            }
                                        }
                                    };
                                    let response = match kind {
                                        "plain" => dropdown(ui, "styles", &mut choice, &options),
                                        "icons" => dropdown_icons(ui, "styles", &mut choice, &icons),
                                        "compact" => compact_dropdown_menu(ui, "styles", options[selected.get()], choices),
                                        _ => dropdown_menu_with_close_behavior(
                                            ui,
                                            "styles",
                                            options[selected.get()],
                                            egui::PopupCloseBehavior::CloseOnClickOutside,
                                            choices,
                                        ),
                                    };
                                    trigger.set(Some(response.id));
                                    if matches!(kind, "plain" | "icons") {
                                        selected.set(choice);
                                    }
                                });
                            })
                            .1,
                    );
                };
                for _ in 0..3 {
                    frame(&ctx, vec![], &mut draw);
                }
                let at = registry::handle_at("dialog:Visual style").unwrap();
                frame(&ctx, pointer(at, true), &mut draw);
                frame(&ctx, pointer(at, false), &mut draw);
                frame(&ctx, vec![], &mut draw);
                assert!(registry::handle_at("dialog:Visual style:Wireframe").is_some(), "{kind}: popup opens");
                frame(&ctx, key(egui::Key::Escape), &mut draw);
                assert_eq!(action.get(), Action::None, "{kind}: first Escape belongs to the popup");
                assert!(ctx.memory(|m| m.has_focus(trigger.get().unwrap())), "{kind}: Escape refocuses its own trigger");
                frame(&ctx, vec![], &mut draw);
                assert!(registry::handle_at("dialog:Visual style:Wireframe").is_none());
                frame(&ctx, key(egui::Key::Enter), &mut draw);
                assert_eq!(action.get(), Action::None, "{kind}: Enter reopens rather than submitting");
                frame(&ctx, vec![], &mut draw);
                assert!(registry::handle_at("dialog:Visual style:Wireframe").is_some(), "{kind}: focused trigger reopens on Enter");
                frame(&ctx, key(egui::Key::Escape), &mut draw);
                assert_eq!(action.get(), Action::None);
                frame(&ctx, vec![], &mut draw);
                assert!(registry::handle_at("dialog:Visual style:Wireframe").is_none());
                frame(&ctx, key(egui::Key::Escape), &mut draw);
                assert_eq!(action.get(), Action::Cancel, "{kind}: Escape without a popup cancels normally");
                assert_eq!(selected.get(), 0, "dismissal does not select an option");
            }
        }
        solidcraft_theme::set_dark(previous);
    }

    #[test]
    fn selection_field_has_three_distinct_looks_in_every_theme() {
        crate::test_support::host_data();
        type Look = (Vec<(egui::Color32, egui::Color32)>, Option<egui::Color32>);
        let look = |active: bool, filled: bool| -> Look {
            let ctx = crate::ui_kit::test_support::context();
            let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                ui.set_width(280.0);
                scope(ui, |ui| {
                    row(ui, "Profiles", |ui| {
                        selection_field(ui, "1 selected", Selection { active, filled });
                    });
                });
            });
            output.textures_delta.clear();
            let field = registry::rect_of("dialog:field:Profiles").unwrap();
            let rects = output
                .shapes
                .iter()
                .filter_map(|s| match &s.shape {
                    egui::Shape::Rect(r) if field.contains_rect(r.rect) => Some((r.fill, r.stroke.color)),
                    _ => None,
                })
                .collect();
            let ink = output.shapes.iter().find_map(|s| match &s.shape {
                egui::Shape::Text(text) if text.galley.job.text == "1 selected" => Some(text.fallback_color),
                _ => None,
            });
            (rects, ink)
        };
        let mut palettes: Vec<(String, Option<crate::theme::FilePalette>, bool)> = vec![("dark".into(), None, true), ("light".into(), None, false)];
        for theme in crate::theme_files::builtin() {
            let dark = theme.base.is_dark();
            palettes.push((
                theme.id.clone(),
                Some(crate::theme::FilePalette {
                    dark,
                    tokens: crate::theme_files::resolve(theme),
                    icons: crate::theme_files::resolve_icon_palette(theme, &Default::default()),
                }),
                dark,
            ));
        }
        let previous = solidcraft_theme::is_dark();
        for (name, custom, dark) in palettes {
            solidcraft_theme::set_dark(dark);
            crate::theme::set_file_palette(custom);
            let t = solidcraft_theme::Tokens::get();
            let [active_filled, active_empty, filled, empty] = [(true, true), (true, false), (false, true), (false, false)].map(|(a, f)| look(a, f));
            let accented = |l: &Look| l.0.iter().any(|&(fill, stroke)| stroke == t.form_primary || fill == t.selected_bg);
            assert!(accented(&active_filled) && accented(&active_empty), "{name}: the active input is accented");
            assert!(!accented(&filled), "{name}: a filled inactive input is not accented: {filled:?}");
            assert!(!accented(&empty), "{name}: an empty inactive input is not accented: {empty:?}");
            let outline = |l: &Look| l.0.last().copied().unwrap();
            // Two colours can't give filled and empty their own fill and border on top of the accent;
            // in 1-bit and 1-bit Dark the text itself tells them apart (a count against the prompt).
            if crate::test_support::is_two_colour(&name) {
                // Inactive inputs are the page with an ink border: white on black in 1-bit, the
                // reverse in 1-bit Dark.
                let (paper, ink) = if dark { (egui::Color32::BLACK, egui::Color32::WHITE) } else { (egui::Color32::WHITE, egui::Color32::BLACK) };
                for look in [&active_filled, &active_empty, &filled, &empty] {
                    for &(fill, border) in &look.0 {
                        for color in [fill, border] {
                            assert!(matches!(color, egui::Color32::BLACK | egui::Color32::WHITE), "{name}: {look:?}");
                        }
                    }
                }
                for look in [&active_filled, &active_empty] {
                    assert_eq!(outline(look).0, t.selected_bg, "{name}");
                    assert_eq!(outline(look).0, ink, "{name}: the active input is inverted");
                    assert_eq!(look.1, Some(t.selected_ink), "{name}");
                }
                for look in [&filled, &empty] {
                    assert_eq!(outline(look), (paper, ink), "{name}");
                }
            } else {
                assert_ne!(outline(&filled).0, outline(&empty).0, "{name}: filled and empty fills differ");
                assert_ne!(outline(&filled).1, outline(&empty).1, "{name}: filled and empty borders differ");
            }
            assert_eq!(filled.1, Some(t.form_ink), "{name}: a filled input's count is in text colour");
            assert_eq!(empty.1, Some(t.form_dim), "{name}: an empty input's prompt is dim");
        }
        crate::theme::set_file_palette(None);
        solidcraft_theme::set_dark(previous);
    }

    #[test]
    fn navigation_dropdown_popups_own_escape_and_return_focus_without_canceling_the_window() {
        use std::cell::Cell;
        let previous = solidcraft_theme::is_dark();
        for dark in [false, true] {
            solidcraft_theme::set_dark(dark);
            for kind in ["plain", "compact", "bounded", "editable"] {
                let ctx = crate::ui_kit::test_support::context();
                let action = Cell::new(Action::None);
                let trigger = Cell::new(None);
                let selected = Cell::new(0);
                let mut draw = |ui: &mut Ui| {
                    let ctx = ui.ctx().clone();
                    action.set(
                        Window::new(egui::Id::unique(("dropdown_escape", kind)), "settings", "Preferences")
                            .cards(1.0)
                            .fields()
                            .actions("OK", "Cancel", None)
                            .show(&ctx, true, |ui| {
                                // An earlier, closed dropdown must not steal this popup's Escape or focus.
                                row(ui, "Units", |ui| {
                                    dropdown(ui, "units", &mut 0, &["mm", "in"]);
                                });
                                row(ui, "Visual style", |ui| {
                                    let options = crate::navigation_slots::VISUAL_STYLES.map(|(_, label)| label);
                                    let mut choice = selected.get();
                                    let choices = |ui: &mut Ui| {
                                        for (index, option) in options.iter().enumerate() {
                                            let response = ui.selectable_label(selected.get() == index, *option);
                                            registry::publish_target(&format!("dialog:Visual style:{option}"), response.rect);
                                            if response.clicked() {
                                                selected.set(index);
                                            }
                                        }
                                    };
                                    let response = match kind {
                                        "plain" => dropdown(ui, "styles", &mut choice, &options),
                                        "compact" => compact_dropdown_menu(ui, "styles", options[selected.get()], choices),
                                        "bounded" => bounded_dropdown_menu(ui, "styles", options[selected.get()], 120.0, choices),
                                        _ => dropdown_menu_with_close_behavior(
                                            ui,
                                            "styles",
                                            options[selected.get()],
                                            egui::PopupCloseBehavior::CloseOnClickOutside,
                                            choices,
                                        ),
                                    };
                                    trigger.set(Some(response.id));
                                    if kind == "plain" {
                                        selected.set(choice);
                                    }
                                });
                            })
                            .1,
                    );
                };
                for _ in 0..3 {
                    frame(&ctx, vec![], &mut draw);
                }
                let at = registry::handle_at("dialog:Visual style").unwrap();
                frame(&ctx, pointer(at, true), &mut draw);
                frame(&ctx, pointer(at, false), &mut draw);
                frame(&ctx, vec![], &mut draw);
                assert!(registry::handle_at("dialog:Visual style:Wireframe").is_some(), "{kind}: popup opens");
                frame(&ctx, key(egui::Key::Escape), &mut draw);
                assert_eq!(action.get(), Action::None, "{kind}: first Escape belongs to the popup");
                assert!(ctx.memory(|m| m.has_focus(trigger.get().unwrap())), "{kind}: Escape refocuses its own trigger");
                frame(&ctx, vec![], &mut draw);
                assert!(registry::handle_at("dialog:Visual style:Wireframe").is_none());
                frame(&ctx, key(egui::Key::Enter), &mut draw);
                assert_eq!(action.get(), Action::None, "{kind}: Enter reopens rather than submitting");
                frame(&ctx, vec![], &mut draw);
                assert!(registry::handle_at("dialog:Visual style:Wireframe").is_some(), "{kind}: focused trigger reopens on Enter");
                frame(&ctx, key(egui::Key::Escape), &mut draw);
                assert_eq!(action.get(), Action::None);
                frame(&ctx, vec![], &mut draw);
                assert!(registry::handle_at("dialog:Visual style:Wireframe").is_none());
                frame(&ctx, key(egui::Key::Escape), &mut draw);
                assert_eq!(action.get(), Action::Cancel, "{kind}: Escape without a popup cancels normally");
                assert_eq!(selected.get(), 0, "dismissal does not select an option");
            }
        }
        solidcraft_theme::set_dark(previous);
    }

    #[test]
    fn a_label_drag_reads_only_literal_values_and_moves_them_on_the_step_grid() {
        let literal = |text: &str| crate::ui_kit::literal(&crate::params_dialog::DocExpressions, text).map(|l| (l.number, l.unit, l.comma));
        assert_eq!(literal("20 mm"), Some((20.0, " mm".into(), false)));
        assert_eq!(literal(" 1,5in "), Some((1.5, "in".into(), true)));
        assert_eq!(literal("-3"), Some((-3.0, String::new(), false)));
        for not_a_value in ["width", "width + 2 mm", "2 * 3 mm", "", "1e3", "1e400 mm", "20 bananas"] {
            assert_eq!(literal(not_a_value), None, "{not_a_value:?} is left alone");
        }
        assert_eq!(crate::ui_kit::scrub_step(20.0), 0.1);
        assert_eq!(crate::ui_kit::scrub_step(250.0), 1.0);
        assert_eq!(crate::ui_kit::scrub_step(0.0), 0.01);
        assert_eq!(crate::ui_kit::scrub_step(-45.0), 0.1);
        assert!((crate::ui_kit::scrubbed(20.0, 30.0, 0.1) - 23.0).abs() < 1e-9);
        assert!((crate::ui_kit::scrubbed(20.37, 1.0, 0.1) - 20.5).abs() < 1e-9, "the value lands on the step's grid");
        assert_eq!(crate::ui_kit::scrubbed(2.0, -500.0, 0.1), 0.0, "a size stops at zero");
        assert!((crate::ui_kit::scrubbed(-2.0, -10.0, 0.1) + 3.0).abs() < 1e-9, "a negative offset keeps moving");
        assert_eq!(crate::ui_kit::scrubbed(1.0, f32::MAX, 1e8), 1.0, "an overflowing drag keeps the value");
        let text =
            |n, step, typed: &str| crate::ui_kit::scrub_text(n, step, &crate::ui_kit::literal(&crate::params_dialog::DocExpressions, typed).unwrap());
        assert_eq!(text(23.0, 0.1, "20 mm"), "23.0 mm");
        assert_eq!(text(1.75, 0.01, "1,5 in"), "1,75 in");
        assert_eq!(text(-0.0001, 0.01, "-1"), "0.00");
        assert_eq!(text(300.0, 1.0, "250 mm"), "300 mm");
    }

    #[test]
    fn dragging_a_value_rows_label_changes_its_literal_and_leaves_expressions_alone() {
        fn frame(ctx: &egui::Context, events: Vec<egui::Event>, modifiers: egui::Modifiers, values: &mut [String; 2]) {
            registry::clear_handles();
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(320.0, 400.0))),
                events: std::iter::once(egui::Event::ModifiersChanged(modifiers)).chain(events).collect(),
                focused: true,
                ..Default::default()
            };
            crate::ui_kit::test_support::raw_frame(ctx, input, |ui| {
                ui.set_width(280.0);
                scope(ui, |ui| {
                    let [distance, offset] = &mut *values;
                    expr_row(ui, "Distance", distance);
                    expr_row(ui, "Offset", offset);
                });
            });
        }
        fn drag(ctx: &egui::Context, row: &str, dx: f32, modifiers: egui::Modifiers, values: &mut [String; 2]) {
            let start = registry::rect_of(&format!("dialog:label:{row}")).unwrap().center();
            let end = start + egui::vec2(dx, 0.0);
            let button = |pos, pressed| egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers };
            frame(ctx, vec![egui::Event::PointerMoved(start), button(start, true)], modifiers, values);
            for step in 1..=4 {
                frame(ctx, vec![egui::Event::PointerMoved(start + egui::vec2(dx * step as f32 / 4.0, 0.0))], modifiers, values);
            }
            frame(ctx, vec![button(end, false)], modifiers, values);
            frame(ctx, vec![], modifiers, values);
        }
        let ctx = crate::ui_kit::test_support::context();
        crate::params_dialog::install_expressions(&ctx);
        let mut values = ["20 mm".to_owned(), "depth + 2 mm".to_owned()];
        frame(&ctx, vec![], Default::default(), &mut values);
        drag(&ctx, "Distance", 40.0, Default::default(), &mut values);
        assert_eq!(values[0], "24.0 mm", "40 points at 0.1 mm a point");
        drag(&ctx, "Distance", 40.0, egui::Modifiers::SHIFT, &mut values);
        assert_eq!(values[0], "24.40 mm", "Shift is ten times finer");
        drag(&ctx, "Distance", -20.0, egui::Modifiers::COMMAND, &mut values);
        assert_eq!(values[0], "4 mm", "Ctrl is ten times coarser");
        drag(&ctx, "Offset", 60.0, Default::default(), &mut values);
        assert_eq!(values[1], "depth + 2 mm", "an expression keeps its parameter");
    }
}

/// Layout in SolidCraft's shipped themes and its viewport's navigator axes.
mod layout {
    use crate::ui_kit::test_support::*;
    use crate::ui_kit::*;
    use egui::Ui;
    use solidcraft_theme::Tokens;
    use solidcraft_theme::contrast_ratio;
    use solidcraft_ui_registry as registry;

    #[test]
    fn navigation_cards_keep_their_proportions_and_native_targets_at_narrow_widths() {
        let previous = solidcraft_theme::is_dark();
        for dark in [false, true] {
            solidcraft_theme::set_dark(dark);
            for available in [140.0, 280.0, 646.0] {
                for (label, width, aspect, options) in [
                    (
                        "Orbit around",
                        196.0,
                        120.0 / 64.0,
                        vec![
                            PictureChoice { key: "origin", label: "Origin", figure: paint_orbit_origin },
                            PictureChoice { key: "centre", label: "View center", figure: paint_orbit_centre },
                            PictureChoice { key: "cursor", label: "Under the cursor", figure: paint_orbit_cursor },
                        ],
                    ),
                    (
                        "View navigator",
                        190.0,
                        178.0 / 115.0,
                        vec![
                            PictureChoice {
                                key: "axes",
                                label: "Axes (signed)",
                                figure: |p, rect| {
                                    paint_navigation_axes(p, rect, &crate::viewport::navigator_axes(&solidcraft_viewport::Camera::default()))
                                },
                            },
                            PictureChoice { key: "cube", label: "Cube (compact)", figure: paint_navigation_cube },
                        ],
                    ),
                ] {
                    let ctx = crate::ui_kit::test_support::context();
                    let draw = |ui: &mut Ui| {
                        ui.set_width(available);
                        settings_picture_choices_sized(
                            ui,
                            "nav_geometry",
                            label,
                            &options,
                            0,
                            true,
                            PictureChoiceSize { width, aspect, caption: true },
                        )
                    };
                    frame(&ctx, vec![], |ui| {
                        draw(ui);
                    });
                    let row = registry::rect_of(&format!("dialog:row:{label}")).unwrap();
                    for option in &options {
                        let card = registry::rect_of(&format!("prefs:nav_geometry:{}", option.key)).unwrap();
                        assert!(row.expand(0.5).contains_rect(card), "{available}: {card:?} outside {row:?}");
                        assert!(card.width() <= available + 0.5);
                        if available == 646.0 {
                            assert!((card.width() - width).abs() < 0.5);
                        }
                    }
                    let at = registry::handle_at(&format!("prefs:nav_geometry:{}", options[1].key)).unwrap();
                    for down in [true, false] {
                        frame(&ctx, pointer(at, down), |ui| {
                            let picked = draw(ui);
                            if !down {
                                assert_eq!(picked, Some(1));
                            }
                        });
                    }
                }
            }
        }
        solidcraft_theme::set_dark(previous);
    }

    /// Selected text in a form field stands out from the selection in every built-in theme
    /// (#11804): none draws it in the selection's own colour.
    #[test]
    fn selected_form_text_reads_in_every_built_in_theme() {
        crate::test_support::host_data();
        let mut themes = vec![("Dark".to_string(), Tokens::dark()), ("Light".to_string(), Tokens::light())];
        themes.extend(crate::theme_files::builtin().iter().map(|theme| (theme.id.clone(), crate::theme_files::resolve(theme))));
        let mut failures = Vec::new();
        for (name, t) in &themes {
            let ratio = contrast_ratio(selection_ink(t), t.selected_bg);
            if ratio < 2.0 {
                failures.push(format!("{name}: {ratio:.2}:1"));
            }
        }
        assert!(failures.is_empty(), "selected form text below 2:1 on selected_bg:\n{}", failures.join("\n"));
    }

    /// Themes whose primary colour stands out from the selection keep it, Dark and Light
    /// included; SolidCraft 95 and CGA, where it is the selection colour itself, take the ink for
    /// selected elements.
    #[test]
    fn only_themes_whose_primary_vanishes_switch_to_the_selected_ink() {
        crate::test_support::host_data();
        for t in [Tokens::dark(), Tokens::light()] {
            assert_eq!(selection_ink(&t), t.form_primary);
        }
        let mut switched: Vec<&str> = crate::theme_files::builtin()
            .iter()
            .filter(|theme| {
                let t = crate::theme_files::resolve(theme);
                selection_ink(&t) != t.form_primary
            })
            .map(|theme| theme.id.as_str())
            .collect();
        switched.sort_unstable();
        assert_eq!(switched, ["cga", "solidcraft-95"]);
        for id in ["solidcraft-95", "cga"] {
            let theme = crate::theme_files::builtin().iter().find(|theme| theme.id == id).unwrap();
            let t = crate::theme_files::resolve(theme);
            assert_eq!(t.form_primary, t.selected_bg, "{id}");
            assert_eq!(selection_ink(&t), t.selected_ink, "{id}");
        }
    }
}

/// Navigation slots in SolidCraft's shipped themes.
mod navigation {
    use crate::ui_kit::*;

    #[test]
    fn active_navigation_slots_stay_readable_in_every_built_in_theme() {
        crate::test_support::host_data();
        let mut themes = vec![("Dark", solidcraft_theme::Tokens::dark()), ("Light", solidcraft_theme::Tokens::light())];
        themes.extend(crate::theme_files::builtin().iter().map(|theme| (theme.name.as_str(), crate::theme_files::resolve(theme))));
        let mut failures = Vec::new();
        for (name, tokens) in themes {
            let Some(fill) = navigation_fill(&tokens, NavigationFill::Active) else { continue };
            let ink = navigation_ink(&tokens, NavigationFill::Active);
            let ratio = solidcraft_theme::contrast_ratio(ink, fill);
            if ratio < 3.0 {
                failures.push(format!("{name}: {ratio:.2}:1 ({ink:?} on {fill:?})"));
            }
        }
        assert!(failures.is_empty(), "active navigation slot unreadable:\n{}", failures.join("\n"));
    }
}

/// Panels in SolidCraft's panel host and shipped themes.
mod panel {
    use crate::ui_kit::test_support::*;
    use crate::ui_kit::*;
    use solidcraft_theme::Tokens;

    #[test]
    fn closed_backing_window_noop_does_not_block_real_host_menu_but_visible_returning_window_does() {
        use crate::panel_runtime::{ContentServices, HostControlTarget, HostOptions, HostPointerTarget, PanelMountHarness, WorkspacePanelHost};
        use solidcraft_engine::panels::{
            ActionSink as _, Destination, InstanceId, LogicalRect, Mutation, PanelKind, Point, SectionId, SectionLocation, Size, Zone,
        };
        fn frame(
            ctx: &egui::Context,
            content: &mut PanelMountHarness,
            host: &mut WorkspacePanelHost,
            time: &mut f64,
            events: Vec<egui::Event>,
            position: egui::Pos2,
            open: bool,
        ) -> (bool, bool) {
            content.tick().unwrap();
            host.capture_metrics(&content.runtime, &content.models).unwrap();
            host.capture_reservations(&content.runtime, &content.catalogue).unwrap();
            host.prepare(&content.runtime, LogicalRect { origin: Point { x: 0.0, y: 0.0 }, size: Size { width: 1400.0, height: 1000.0 } }).unwrap();
            *time += 1.0 / 60.0;
            let mut observed = (false, false);
            crate::ui_kit::test_support::raw_frame(
                ctx,
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 1000.0))),
                    focused: true,
                    time: Some(*time),
                    events,
                    ..Default::default()
                },
                |ui| {
                    Container::begin_host_windows(ui.ctx());
                    host.show_deferred(
                        ui,
                        &mut content.runtime,
                        &mut content.native,
                        &mut ContentServices { models: &content.models, commands: &mut content.commands, local: &mut content.local },
                    )
                    .unwrap();
                    let source = host
                        .frame()
                        .unwrap()
                        .controls
                        .iter()
                        .find(|control| {
                            matches!(&control.target,
                    HostControlTarget::Pointer(HostPointerTarget::Menu { instance }) if instance.0 == "source")
                        })
                        .unwrap();
                    observed.0 = ui.ctx().read_response(source.id).unwrap().clicked();
                    let id = egui::Id::unique("returning_backing_feature");
                    let container = Container::new(ui.ctx(), Surface::Feature);
                    let backing = Panel::feature_frame(ui.ctx(), crate::ui_kit::LayoutMode::Floating, crate::frame::Side::Left);
                    let stock = container
                        .window("Returning independent feature")
                        .id(id)
                        .title_bar(false)
                        .collapsible(false)
                        .resizable(false)
                        .scroll(false)
                        .constrain(false)
                        .fade_in(false)
                        .fade_out(false)
                        .fixed_pos(position)
                        .frame(backing);
                    let mut open_flag = open;
                    let mut body_called = false;
                    let windows_before = Container::host_window_regions(ui.ctx());
                    // Exercise the real optional-backing allocation path without
                    // manufacturing GPU support in this headless test.
                    let shown = crate::ui_kit::container::FeatureWindow::new(stock, id, backing).open(&mut open_flag).show_background(
                        ui.ctx(),
                        Some(host_marker),
                        |ui| {
                            body_called = true;
                            ui.set_min_size(egui::vec2(180.0, 90.0));
                            ui.label("Actual foreign feature content");
                            ui.is_visible() && !ui.is_sizing_pass()
                        },
                    );
                    observed.1 = shown.as_ref().and_then(|shown| shown.inner).unwrap_or(false);
                    if !open {
                        assert!(shown.is_none());
                        assert!(!body_called, "closing must skip the body on the first closed frame");
                        assert_eq!(Container::host_window_regions(ui.ctx()), windows_before, "a closed window cannot leave a ghost footprint");
                        let layer = egui::LayerId::new(egui::Order::Middle, id);
                        assert!(
                            ui.ctx()
                                .graphics(|graphics| graphics.get(layer).is_some_and(|paint| paint.all_entries().count() > 0
                                    && paint.all_entries().all(|entry| matches!(&entry.shape, egui::Shape::Noop)))),
                            "a closed backing wrapper must really leave only its reserved Noop"
                        );
                    } else if observed.1 {
                        assert!(Container::host_window_regions(ui.ctx()).unwrap().contains(&shown.as_ref().unwrap().response.rect));
                    }
                    host.finish(
                        ui,
                        &mut content.runtime,
                        &mut content.native,
                        &mut ContentServices { models: &content.models, commands: &mut content.commands, local: &mut content.local },
                        &mut content.catalogue,
                    )
                    .unwrap();
                },
            );
            observed
        }
        fn pointer(at: egui::Pos2, pressed: bool) -> Vec<egui::Event> {
            vec![
                egui::Event::PointerMoved(at),
                egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE },
            ]
        }
        for scale in [1.0, 2.0] {
            let ctx = crate::ui_kit::test_support::context();
            ctx.set_pixels_per_point(scale);
            ctx.options_mut(|options| options.max_passes = 1.try_into().unwrap());
            let mut content = PanelMountHarness::new(false).unwrap();
            content
                .dispatch(vec![Mutation::Open {
                    kind: PanelKind::new("fixture.log").unwrap(),
                    instance: InstanceId::new("source").unwrap(),
                    destination: Some(Destination::NewSection {
                        section: SectionId::new("source-section").unwrap(),
                        index: 0,
                        location: SectionLocation::Dock { zone: Zone::Left },
                        size: Size { width: 280.0, height: 240.0 },
                    }),
                }])
                .unwrap();
            let before = content.runtime.layout().inspect();
            let commits = content.native.commits;
            let mut host = WorkspacePanelHost::new(HostOptions::default()).unwrap();
            let mut time = 0.0;
            let away = egui::pos2(1600.0, 1200.0);
            for _ in 0..3 {
                frame(&ctx, &mut content, &mut host, &mut time, Vec::new(), away, true);
            }
            let source = host
                .frame()
                .unwrap()
                .controls
                .iter()
                .find(|control| {
                    matches!(&control.target,
                HostControlTarget::Pointer(HostPointerTarget::Menu { instance }) if instance.0 == "source")
                })
                .unwrap()
                .clone();
            let at = crate::panel_runtime::egui_rect(source.bounds).center();
            let cover = at - egui::vec2(12.0, 12.0);
            assert!(frame(&ctx, &mut content, &mut host, &mut time, Vec::new(), cover, true).1);
            frame(&ctx, &mut content, &mut host, &mut time, Vec::new(), cover, false);
            let retained = egui::containers::AreaState::load(&ctx, egui::Id::unique("returning_backing_feature")).unwrap();
            assert!(retained.interactable && retained.rect().contains(at), "the closed window must retain its old covering Area state");
            frame(&ctx, &mut content, &mut host, &mut time, pointer(at, true), cover, false);
            assert!(
                frame(&ctx, &mut content, &mut host, &mut time, pointer(at, false), cover, false).0,
                "the earlier actual Menu widget must complete its real click"
            );
            for _ in 0..2 {
                frame(&ctx, &mut content, &mut host, &mut time, Vec::new(), cover, false);
            }
            assert!(host.frame().unwrap().menu_bounds.is_some(), "Noop-only backing must not refuse the real underlying menu intent");
            for pressed in [true, false] {
                frame(
                    &ctx,
                    &mut content,
                    &mut host,
                    &mut time,
                    vec![egui::Event::Key { key: egui::Key::Escape, physical_key: None, pressed, repeat: false, modifiers: egui::Modifiers::NONE }],
                    cover,
                    false,
                );
            }
            frame(&ctx, &mut content, &mut host, &mut time, Vec::new(), cover, false);
            frame(&ctx, &mut content, &mut host, &mut time, pointer(at, true), cover, false);
            let returned = frame(&ctx, &mut content, &mut host, &mut time, pointer(at, false), cover, true);
            assert!(returned.0 && returned.1, "the actual Window must return visibly after the earlier Menu click queues its intent");
            for _ in 0..3 {
                frame(&ctx, &mut content, &mut host, &mut time, Vec::new(), cover, false);
            }
            assert!(host.frame().unwrap().menu_bounds.is_none(), "actual visible foreign paint still rejects the source with no latent replay");
            assert_eq!(content.runtime.layout().inspect(), before);
            assert_eq!(content.native.commits, commits);
            assert_eq!(content.commands.calls, 0);
        }
    }

    #[test]
    fn theme_shape_changes_preserve_panel_geometry_and_dock_contacts() {
        crate::test_support::host_data();
        use crate::{frame::Side, ui_kit::LayoutMode};
        let ctx = crate::ui_kit::test_support::context();
        let mut results = Vec::new();
        for name in ["solidcraft-95", "blueprint", "cupertino"] {
            let t = crate::theme_files::resolve(crate::theme_files::find(name, &[]).unwrap());
            let _theme = solidcraft_theme::scoped_tokens(t);
            for surface in [Surface::Panel, Surface::Feature, Surface::Window, Surface::Popup, Surface::Navigation, Surface::Toast] {
                solidcraft_theme::apply(&ctx);
                let floating = Container::for_layout(LayoutMode::Floating, surface).frame(&ctx, t.panel, Side::Left);
                let bounds = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(240.0, 180.0));
                results.push((name, surface, floating.corner_radius, floating.outer_rect(bounds), t));
            }
            let docked = Container::for_layout(LayoutMode::Docked, Surface::Panel)
                .joined(DockEdges { left: true, top: true, ..Default::default() })
                .frame(&ctx, t.panel, Side::Left);
            assert_eq!(docked.corner_radius, egui::CornerRadius { nw: 0, ne: 0, sw: 0, se: t.radius_lg });
        }
        for (name, surface, radius, bounds, t) in &results {
            let expected = match surface {
                Surface::Panel => t.radius_lg,
                Surface::Feature | Surface::Window => t.radius_xl,
                _ => t.radius_md,
            };
            assert_eq!(*radius, egui::CornerRadius::same(expected), "{name}: {surface:?}");
            assert_eq!(*bounds, results.iter().find(|row| row.1 == *surface).unwrap().3, "shape must not change layout");
        }
    }

    #[test]
    fn host_surface_fallback_paints_readable_plain_labels_without_changing_body_layout() {
        use solidcraft_engine::panels::{ContrastOutcome, PanelSurface};
        let bounds = egui::Rect::from_min_size(egui::pos2(40.0, 40.0), egui::vec2(320.0, 180.0));
        for floating in [false, true] {
            for fallback in [false, true] {
                let ctx = crate::ui_kit::test_support::context();
                let mut authored = Tokens::get();
                authored.panel = egui::Color32::WHITE;
                authored.text = egui::Color32::BLACK;
                authored.text_dim = egui::Color32::BLACK;
                authored.form_ink = egui::Color32::BLACK;
                authored.form_dim = egui::Color32::BLACK;
                authored.dialog_label = egui::Color32::BLACK;
                if fallback {
                    authored.text = authored.panel;
                }
                let _authored = solidcraft_theme::scoped_panel_tokens(authored);
                let style = crate::panel_runtime::HostSurfaceStyle::capture(&ctx, false, Some(0.0), None);
                let appearance = style.appearance(PanelSurface::Translucent, floating);
                let failed = matches!(appearance.contrast, ContrastOutcome::RetainReadableFallback { .. });
                assert_eq!(failed, fallback, "exercise both authored paint and deliberately unreadable fallback");
                let palette = failed.then(|| style.palette(appearance));
                let font = egui::FontId::monospace(19.0);
                // Floating Areas create a fresh Ui from the context style; they do
                // not inherit a caller's local Ui overrides. Set the common baseline
                // before the sizing pass so both existing host paths inherit it.
                ctx.global_style_mut(|style| {
                    style.override_font_id = Some(font.clone());
                    style.spacing.item_spacing = egui::vec2(13.0, 17.0);
                    style.wrap_mode = Some(egui::TextWrapMode::Wrap);
                    style.visuals.override_text_color = Some(authored.panel);
                });
                if floating {
                    crate::ui_kit::test_support::raw_frame(&ctx, egui::RawInput::default(), |ui| {
                        panel_host_surface(ui, egui::Id::unique("captured-fill"), bounds, floating, appearance.fill, palette, |_, _| {});
                    });
                }
                let mut expected_ink = egui::Color32::TRANSPARENT;
                let output = crate::ui_kit::test_support::raw_frame(&ctx, egui::RawInput::default(), |ui| {
                    let parent_visuals = ui.visuals().clone();
                    let parent_spacing = ui.spacing().item_spacing;
                    let parent_fonts = ui.style().text_styles.clone();
                    let parent_wrap = ui.style().wrap_mode;
                    expected_ink = palette.map_or(authored.panel, |tokens| tokens.text);
                    panel_host_surface(ui, egui::Id::unique("captured-fill"), bounds, floating, appearance.fill, palette, |ui, clip| {
                        assert_eq!(Tokens::get(), palette.copied().unwrap_or(authored));
                        assert_eq!(ui.spacing().item_spacing, parent_spacing);
                        assert_eq!(ui.style().text_styles, parent_fonts);
                        assert_eq!(ui.style().override_font_id, Some(font.clone()));
                        assert_eq!(ui.style().wrap_mode, parent_wrap);
                        if palette.is_none() {
                            assert_eq!(ui.visuals(), &parent_visuals, "normal bodies inherit unchanged visuals");
                        }
                        assert!(bounds.contains_rect(clip));
                        ui.label("Readable captured panel");
                    });
                    assert_eq!(Tokens::get(), authored);
                    assert_eq!(ui.visuals(), &parent_visuals, "fallback visuals restore immediately");
                    assert_eq!(ui.spacing().item_spacing, parent_spacing);
                    assert_eq!(ui.style().text_styles, parent_fonts);
                });
                let (mut backgrounds, mut labels) = (0, 0);
                for shape in &output.shapes {
                    crate::ui_kit::test_support::each_shape(&shape.shape, &mut |shape| match shape {
                        egui::Shape::Rect(rect) if rect.rect == bounds && rect.fill != egui::Color32::TRANSPARENT => {
                            assert_eq!(rect.fill, appearance.fill, "actual paint uses the captured fill");
                            backgrounds += 1;
                        }
                        egui::Shape::Text(text) if text.galley.job.text == "Readable captured panel" => {
                            for section in &text.galley.job.sections {
                                let ink = text.override_text_color.unwrap_or_else(|| {
                                    if section.format.color == egui::Color32::PLACEHOLDER { text.fallback_color } else { section.format.color }
                                });
                                assert_eq!(ink, expected_ink, "the actual plain-label galley uses readable fallback ink");
                                assert_eq!(section.format.font_id, font, "fallback preserves the body's chosen font");
                            }
                            labels += 1;
                        }
                        _ => {}
                    });
                }
                assert!(backgrounds > 0 && labels > 0, "inspect real background and label paint");
                assert_eq!(Tokens::get(), authored);
            }
        }
    }
}

/// The view compass with SolidCraft's catalogs.
mod view_compass {
    use crate::ui_kit::view_compass::{Compass, ViewAxes, show, tips};
    use egui::{Event, Pos2, RawInput, Rect, pos2, vec2};

    /// The tooltip text a hovered tip shows, after a few frames with the pointer at rest on it.
    fn hover_text(snap_hint: bool) -> Vec<String> {
        let front = ViewAxes { right: [1.0, 0.0, 0.0], up: [0.0, 0.0, 1.0], back: [0.0, -1.0, 0.0] };
        let centre = pos2(200.0, 200.0);
        let at = tips(&front, centre).iter().find(|tip| tip.0 == "x").map(|tip| tip.2).unwrap();
        let ctx = crate::ui_kit::test_support::context();
        ctx.global_style_mut(|style| style.interaction.tooltip_delay = 0.0);
        let mut texts = Vec::new();
        for frame in 0..4 {
            let input = RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(400.0, 400.0))),
                events: if frame == 0 { vec![Event::PointerMoved(at)] } else { Vec::new() },
                time: Some(f64::from(frame)),
                ..Default::default()
            };
            let output = crate::ui_kit::test_support::raw_frame(&ctx, input, |ui| {
                let compass =
                    Compass { clip: ui.max_rect(), centre, axes: front, hover: Some(at), orbiting: false, snap_hint, home_label: "Home".into() };
                show(ui, &compass);
            });
            texts = Vec::new();
            for shape in &output.shapes {
                crate::ui_kit::test_support::each_shape(&shape.shape, &mut |shape| {
                    if let egui::Shape::Text(text) = shape {
                        texts.push(text.galley.job.text.clone());
                    }
                });
            }
        }
        texts
    }

    /// A tip's hover text is the catalog's whole sentence in the user's language, with the axis
    /// as its argument, with and without the Shift hint.
    #[test]
    fn a_tips_hover_text_is_translated() {
        crate::test_support::host_data();
        let _japanese = solidcraft_ui_text::scope("ja");
        for (snap_hint, id) in [(false, "ui.view_compass_tip"), (true, "ui.view_compass_tip_snap")] {
            let expected = solidcraft_ui_text::format(id, &[("axis", "X")]);
            assert!(expected.starts_with('X') && !expected.contains("View from"), "{id} is in the Japanese catalog: {expected}");
            let texts = hover_text(snap_hint);
            assert!(texts.contains(&expected), "{id}: the tip shows {expected:?}; drawn: {texts:?}");
        }
    }
}
