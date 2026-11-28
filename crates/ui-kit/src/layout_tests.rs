use crate::test_support::*;
use crate::*;

use egui::Ui;
use solidcraft_theme::Tokens;
use solidcraft_ui_registry as registry;

#[test]
fn theme_browser_has_equal_end_padding_and_screenful_arrows_without_selecting() {
    let ctx = crate::test_support::context();
    let clicked = std::cell::Cell::new(false);
    let draw = |ui: &mut Ui| {
        theme_browser(ui, "padding_browser", 10, false, None, &PageLabels { previous: "Previous themes", next: "Next themes" }, |ui, i, width| {
            let card = theme_card(
                ui,
                &ThemeCard {
                    name: "Example",
                    note: "Built in",
                    options: None,
                    sample: ThemeSample::from_tokens(&Tokens::light()),
                    selected: i == 0,
                    dark: false,
                    high: false,
                    day: false,
                    night: false,
                },
                width,
            );
            clicked.set(clicked.get() || card.clicked());
            registry::publish_rect(&format!("test:theme_card:{i}"), card.rect);
        });
    };
    for _ in 0..3 {
        frame(&ctx, vec![], draw);
    }
    let viewport = registry::rect_of("prefs:theme_viewport").unwrap();
    let first = registry::rect_of("test:theme_card:0").unwrap();
    for gap in [first.left() - viewport.left(), first.top() - viewport.top(), viewport.bottom() - first.bottom()] {
        assert!((gap - Tokens::get().space_10).abs() < 0.1, "same padding on every resting side: {gap}");
    }
    assert!(registry::handle_at("prefs:theme_previous").is_none());
    let advance = || {
        let at = registry::handle_at("prefs:theme_next").unwrap();
        frame(&ctx, pointer(at, true), draw);
        frame(&ctx, pointer(at, false), draw);
        frame(&ctx, vec![], draw);
    };
    let stride = registry::rect_of("test:theme_card:1").unwrap().left() - first.left();
    let page = ((viewport.width() / stride).floor() as usize).max(1);
    advance();
    let moved = registry::rect_of(&format!("test:theme_card:{page}")).unwrap();
    assert!((moved.left() - first.left()).abs() < 0.1, "arrow moves a screenful of {page} cards");
    assert!(registry::handle_at("prefs:theme_previous").is_some());
    assert!(registry::handle_at("prefs:theme_next").is_some());
    for _ in 0..10 {
        if registry::handle_at("prefs:theme_next").is_none() {
            break;
        }
        advance();
    }
    let viewport = registry::rect_of("prefs:theme_viewport").unwrap();
    let last = registry::rect_of("test:theme_card:9").unwrap();
    assert!((viewport.right() - last.right() - Tokens::get().space_10).abs() < 0.1);
    assert!(registry::handle_at("prefs:theme_next").is_none());
    assert!(!clicked.get(), "floating arrows never select the card under them");
}

#[test]
fn axis_gain_invert_and_source_fit_the_table_without_horizontal_scrolling() {
    let previous = solidcraft_theme::is_dark();
    for dark in [false, true] {
        solidcraft_theme::set_dark(dark);
        for width in [280.0, 420.0, 646.0] {
            let ctx = crate::test_support::context();
            let mut reverse = false;
            let mut gain = 1.0;
            let mut source = 0;
            let (mut inverted, mut speed, mut motion) = (egui::Rect::NOTHING, egui::Rect::NOTHING, egui::Rect::NOTHING);
            let (mut invert_cell, mut speed_cell, mut source_cell) = (egui::Rect::NOTHING, egui::Rect::NOTHING, egui::Rect::NOTHING);
            let output = crate::test_support::raw_frame(
                &ctx,
                egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 1200.0))), ..Default::default() },
                |ui| {
                    scope(ui, |ui| {
                        ui.set_width(width);
                        ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
                        let columns = [
                            Column::new("Axis", 1.5).flex(),
                            Column::new("Live", 1.0).flex(),
                            Column::new("Invert", 0.5).flex(),
                            Column::new("Speed", 0.0).min_content_width(64.0),
                            Column::new("Source", 2.0).flex(),
                        ];
                        table_fitted(ui, "axes", &columns, "", |ui, table| {
                            table.headers(ui, |_, _| false);
                            table.row(ui, "pan_x", false, |row| {
                                row.cell(|ui| text(ui, "Pan X", Text::Cell));
                                row.cell(|ui| {
                                    let (rect, _) =
                                        ui.allocate_exact_size(egui::vec2(ui.available_width(), Tokens::get().space_14), egui::Sense::hover());
                                    live_axis_bar_in(ui, rect, 0.5, [0.2, 0.3]);
                                });
                                row.cell(|ui| {
                                    inverted = checkbox(ui, "", &mut reverse).rect;
                                    invert_cell = ui.max_rect();
                                });
                                row.cell(|ui| {
                                    speed = settings_axis_gain(ui, &mut gain).rect;
                                    speed_cell = ui.max_rect();
                                });
                                row.cell(|ui| {
                                    motion = dropdown(ui, "axis_containment", &mut source, &["Translation X", "Translation Y"]).rect;
                                    source_cell = ui.max_rect();
                                });
                            });
                        });
                    });
                },
            );
            let row = registry::rect_of("dialog:table:axes:pan_x").unwrap();
            let controls = [inverted, speed, motion];
            assert!(controls.into_iter().all(|control| row.expand(0.5).contains_rect(control)), "{width}: {controls:?} in {row:?}");
            for (control, cell) in controls.into_iter().zip([invert_cell, speed_cell, source_cell]) {
                assert!(cell.expand(0.5).contains_rect(control), "{width}: control {control:?} escapes its cell {cell:?}");
            }
            let viewport = registry::rect_of("dialog:table:axes:viewport").unwrap();
            assert!(viewport.width() <= width);
            assert!(row.width() <= viewport.width(), "axis columns fit the viewport at {width}");
            assert!(registry::count_of("dialog:table:axes:content_width").unwrap() <= f64::from(viewport.width()) + 0.5);
            let mut bars = 0;
            for clipped in &output.shapes {
                crate::test_support::each_shape(&clipped.shape, &mut |shape| match shape {
                    egui::Shape::Rect(rect)
                        if rect.fill == Tokens::get().dialog_rule && (rect.rect.height() - Tokens::get().space_14).abs() < 0.5 =>
                    {
                        bars += 1;
                        assert!(clipped.clip_rect.contains_rect(rect.rect), "{width}: live bar must be fully painted");
                    }
                    _ => {}
                });
            }
            assert_eq!(bars, 1, "inspect the actual live-meter background");
            assert!(crate::test_support::text_shapes(&output).any(|text| text.galley.text() == "Pan X" && !text.galley.elided));
            assert_eq!((reverse, gain, source), (false, 1.0, 0));
        }
    }
    solidcraft_theme::set_dark(previous);
}

#[test]
fn mouse_mapping_chords_and_actions_wrap_without_overpainting_each_other() {
    let previous = solidcraft_theme::is_dark();
    for dark in [false, true] {
        solidcraft_theme::set_dark(dark);
        for width in [160.0, 280.0, 528.0] {
            let ctx = crate::test_support::context();
            let mut bounds = egui::Rect::NOTHING;
            let output = crate::test_support::raw_frame(
                &ctx,
                egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 1600.0))), ..Default::default() },
                |ui| {
                    ui.set_width(width);
                    let group = ui.vertical(|ui| {
                        settings_bindings(
                            ui,
                            &[
                                (&["Ctrl/Cmd", "Shift", "Middle drag"], "Zoom toward the cursor"),
                                (&["Alt/Option"], "Orbit"),
                                (&["Right drag"], "Orbit"),
                                (&["Wheel"], "Zoom"),
                                (&["Left click"], "Select"),
                                (&["Right click"], "Menu"),
                                (&["Click"], "Select one"),
                                (&["Shift", "Click"], "Add"),
                                (&["Ctrl/Cmd", "Click"], "Toggle"),
                            ],
                        )
                    });
                    bounds = group.response.rect;
                },
            );
            for label in ["Ctrl/Cmd", "Shift", "Middle drag", "Alt/Option", "Right drag", "Wheel", "Left click", "Right click", "Click"] {
                let caps: Vec<_> = crate::test_support::text_shapes(&output).filter(|text| text.galley.text() == label).collect();
                assert!(!caps.is_empty(), "{width}: the {label} keycap must be painted");
                assert!(caps.iter().all(|text| !text.galley.elided), "{width}: {label} must be complete");
            }
            for clipped in &output.shapes {
                crate::test_support::each_shape(&clipped.shape, &mut |shape| {
                    if let egui::Shape::Text(text) = shape {
                        assert!(
                            clipped.clip_rect.expand(1.0).contains_rect(text.galley.rect.translate(text.pos.to_vec2())),
                            "{width}: text must fit its real paint clip"
                        );
                    }
                });
            }
            let text: Vec<_> = crate::test_support::text_shapes(&output).map(|text| text.galley.rect.translate(text.pos.to_vec2())).collect();
            assert!(text.iter().all(|rect| bounds.expand(1.0).contains_rect(*rect)), "{width}: {text:?} outside {bounds:?}");
            for (index, rect) in text.iter().enumerate() {
                assert!(text.iter().skip(index + 1).all(|other| !rect.intersects(*other)), "{width}: binding text must not overlap");
            }
        }
    }
    solidcraft_theme::set_dark(previous);
}

#[test]
fn mouse_mapping_paints_a_headerless_card_and_keycaps_beside_the_figure() {
    for dark in [false, true] {
        let t = Tokens::themed(dark, solidcraft_theme::ColorVision::default());
        let _tokens = solidcraft_theme::scoped_tokens(t);
        let ctx = crate::test_support::context();
        let output = crate::test_support::raw_frame(&ctx, egui::RawInput::default(), |ui| {
            scope(ui, |ui| {
                ui.set_width(646.0);
                settings_mouse_mapping(
                    ui,
                    &[
                        (vec!["Shift".into(), "Middle drag".into()], "Orbit".into()),
                        (vec!["Wheel".into()], "Zoom toward the cursor".into()),
                        (vec!["Left click".into()], "Select".into()),
                    ],
                    |_| {},
                );
            });
        });
        let viewport = registry::rect_of("dialog:table:mouse_mapping:viewport").unwrap();
        let card = output
            .shapes
            .iter()
            .find(|shape| {
                matches!(&shape.shape, egui::Shape::Rect(rect)
            if rect.fill == t.table_fill() && rect.rect.contains_rect(viewport))
            })
            .expect("the mapping table paints its card");
        assert!(card.clip_rect.contains_rect(viewport), "the card is visible beside the picture: {card:?}");
        assert!(
            !output.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Rect(rect)
            if rect.fill == t.text.gamma_multiply(0.08) && rect.rect.intersects(viewport))),
            "no header band"
        );
        let first = registry::rect_of("dialog:table:mouse_mapping:0").unwrap();
        assert!((first.top() - viewport.top()).abs() <= 0.5, "no space reserved for a header");
        for index in 0..2 {
            let row = registry::rect_of(&format!("dialog:table:mouse_mapping:{index}")).unwrap();
            assert!(
                output.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::LineSegment {points, ..}
                if (points[0].y - row.bottom()).abs() <= 0.5 && (points[1].y - row.bottom()).abs() <= 0.5
                    && points[0].x <= row.left() + 0.5 && points[1].x >= row.right() - 0.5)),
                "shared separator after row {index}"
            );
        }
        let input = registry::rect_of("dialog:table:mouse_mapping:0:cell:input").unwrap();
        for key in ["Shift", "Middle drag"] {
            let text = crate::test_support::text_shapes(&output).find(|text| text.galley.text() == key).expect("individual keycap text");
            let painted = text.galley.rect.translate(text.pos.to_vec2());
            assert!(input.contains_rect(painted), "{key} stays inside the Input cell");
            assert!(
                output.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Rect(rect)
                if rect.fill == t.dialog_footer && rect.rect.contains_rect(painted) && input.contains_rect(rect.rect))),
                "{key} has a keycap"
            );
        }
        let text: Vec<_> = crate::test_support::text_shapes(&output).map(|text| text.galley.text()).collect();
        assert!(!text.contains(&"Input") && !text.contains(&"Action"));
    }
}

#[test]
fn mouse_mapping_search_keeps_main_gesture_chords_and_actions() {
    for query in ["Middle drag", "Zoom toward the cursor"] {
        let ctx = crate::test_support::context();
        let output = crate::test_support::raw_frame(&ctx, egui::RawInput::default(), |ui| {
            scope(ui, |ui| {
                ui.set_width(280.0);
                let _search = crate::settings_search::begin(ui, query, "prefs");
                settings_mouse_mapping(
                    ui,
                    &[(vec!["Ctrl/Cmd".into(), "Middle drag".into()], "Zoom toward the cursor".into()), (vec!["Alt/Option".into()], "Orbit".into())],
                    |_| {},
                );
            });
        });
        let text: Vec<_> = crate::test_support::text_shapes(&output).map(|text| text.galley.text()).collect();
        assert!(text.contains(&"Ctrl/Cmd") && text.contains(&"Middle drag") && text.contains(&"Zoom toward the cursor"));
        assert!(!text.contains(&"Alt/Option") && !text.contains(&"Orbit"), "unmatched gestures stay hidden");
    }
}

#[test]
fn status_lines_wrap_at_form_width_and_leave_room_for_the_next_row() {
    let previous_dark = solidcraft_theme::is_dark();
    for dark in [false, true] {
        solidcraft_theme::set_dark(dark);
        let base = Tokens::get();
        for width in [160.0, 280.0] {
            for font_label in [base.font_label, base.font_label * 1.8] {
                let _theme = solidcraft_theme::scoped_tokens(Tokens { font_label, ..base });
                for (tone, ink) in [(StatusTone::Dim, base.text_dim), (StatusTone::Warning, base.warning), (StatusTone::Error, base.error)] {
                    let ctx = crate::test_support::context();
                    let message = "needs a joint that turns and one that slides";
                    let mut message_rect = egui::Rect::NOTHING;
                    let output = crate::test_support::raw_frame(
                        &ctx,
                        egui::RawInput {
                            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(400.0, 600.0))),
                            ..Default::default()
                        },
                        |ui| {
                            ui.set_width(width);
                            scope(ui, |ui| {
                                message_rect = status_line(ui, message, tone).rect;
                                assert_eq!(ui.style().wrap_mode, Some(egui::TextWrapMode::Truncate), "ordinary controls keep their style");
                                row(ui, "Radius", |ui| {
                                    ui.label("10 mm");
                                });
                            });
                        },
                    );
                    let text = crate::test_support::text_shapes(&output).find(|text| text.galley.text() == message).unwrap();
                    assert!(!text.galley.elided, "width={width}, font={font_label}");
                    if width == 160.0 {
                        assert!(text.galley.rows.len() > 1, "a narrow form must wrap");
                    }
                    assert!(text.galley.job.sections.iter().all(|section| section.format.color == ink));
                    assert!(message_rect.width() <= width + 0.5);
                    let next = registry::rect_of("dialog:row:Radius").unwrap();
                    assert!((message_rect.left() - next.left()).abs() < 0.5, "use both form columns");
                    assert!(next.top() >= message_rect.bottom(), "the next row must follow all lines");
                    let painted = text.galley.rect.translate(text.pos.to_vec2());
                    assert!(message_rect.expand(0.5).contains_rect(painted), "reserve the painted height");
                    let clip = output
                        .shapes
                        .iter()
                        .find(|shape| matches!(&shape.shape, egui::Shape::Text(t) if t.galley.text() == message))
                        .unwrap()
                        .clip_rect;
                    assert!(clip.contains_rect(painted), "every line is visible");
                }
            }
        }
    }
    solidcraft_theme::set_dark(previous_dark);
}

#[test]
fn shared_picture_choices_wrap_inside_the_row_and_reserve_their_height() {
    let choices = [
        PictureChoice { key: "first", label: "First", figure: |_, _| {} },
        PictureChoice { key: "second", label: "Second", figure: |_, _| {} },
        PictureChoice { key: "third", label: "Third", figure: |_, _| {} },
    ];
    for dark in [false, true] {
        let _theme = solidcraft_theme::scoped_tokens(if dark { Tokens::dark() } else { Tokens::light() });
        for width in [120.0, 260.0, 600.0] {
            for sized in [false, true] {
                let ctx = crate::test_support::context();
                crate::test_support::raw_frame(
                    &ctx,
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 1000.0))),
                        ..Default::default()
                    },
                    |ui| {
                        ui.set_width(width);
                        scope(ui, |ui| {
                            let picked = if sized {
                                settings_picture_choices_sized(
                                    ui,
                                    "shared",
                                    "Choices",
                                    &choices,
                                    usize::MAX,
                                    true,
                                    PictureChoiceSize { width: 110.0, aspect: 1.0, caption: false },
                                )
                            } else {
                                settings_picture_choices(ui, "shared", "Choices", &choices, usize::MAX, true)
                            };
                            assert_eq!(picked, None, "drawing does not change an unknown selection");
                            let row_rect = registry::rect_of("dialog:row:Choices").unwrap();
                            for choice in &choices {
                                let rect = registry::rect_of(&format!("prefs:shared:{}", choice.key)).unwrap();
                                assert!(row_rect.expand(0.5).contains_rect(rect), "width={width}, sized={sized}: {rect:?} outside {row_rect:?}");
                            }
                            let first = registry::rect_of("prefs:shared:first").unwrap();
                            let last = registry::rect_of("prefs:shared:third").unwrap();
                            assert_eq!(last.top() > first.top(), width < 600.0);
                            let next = ui.label("Next").rect;
                            assert!(next.top() >= last.bottom());
                        });
                    },
                );
            }
        }
    }
}

#[test]
fn shared_picture_choices_return_clicks_and_respect_both_disabled_states() {
    use std::cell::Cell;
    let choices =
        [PictureChoice { key: "first", label: "First", figure: |_, _| {} }, PictureChoice { key: "second", label: "Second", figure: |_, _| {} }];
    for (enabled, parent_enabled) in [(true, true), (false, true), (true, false)] {
        let ctx = crate::test_support::context();
        ctx.enable_accesskit();
        let picked = Cell::new(None);
        let draw = |ui: &mut Ui| {
            ui.add_enabled_ui(parent_enabled, |ui| {
                picked.set(settings_picture_choices(ui, "shared", "Choices", &choices, 0, enabled));
            });
        };
        frame(&ctx, vec![], draw);
        let at = registry::handle_at("prefs:shared:second").unwrap();
        frame(&ctx, pointer(at, true), draw);
        frame(&ctx, pointer(at, false), draw);
        assert_eq!(picked.get(), (enabled && parent_enabled).then_some(1));
        picked.set(None);
        frame(&ctx, key(egui::Key::Tab), draw);
        frame(&ctx, key(egui::Key::Space), draw);
        if !enabled || !parent_enabled {
            assert_eq!(picked.get(), None);
        }
    }
}

/// #11434: an arrow moves by the cards that fit, not by one, and the last page ends on the last card.
#[test]
fn layout_picture_choices_wrap_inside_the_indexed_row_and_keep_real_targets() {
    let previous = solidcraft_theme::is_dark();
    for dark in [false, true] {
        solidcraft_theme::set_dark(dark);
        for width in [140.0, 280.0, 646.0] {
            let ctx = crate::test_support::context();
            let options = [
                PictureChoice { key: "origin", label: "Origin", figure: |p, r| crate::preferences_figures::paint_orbit(p, r, 0) },
                PictureChoice { key: "centre", label: "Centre", figure: |p, r| crate::preferences_figures::paint_orbit(p, r, 1) },
                PictureChoice { key: "cursor", label: "Cursor", figure: |p, r| crate::preferences_figures::paint_orbit(p, r, 2) },
                PictureChoice { key: "z_up", label: "Z up", figure: |p, r| crate::preferences_figures::paint_orientation(p, r, false) },
            ];
            frame(&ctx, vec![], |ui| {
                ui.set_width(width);
                settings_picture_choices(ui, "geometry_cards", "Example", &options, 2, true);
            });
            let row = registry::rect_of("dialog:row:Example").unwrap();
            for option in &options {
                let rect = registry::rect_of(&format!("prefs:geometry_cards:{}", option.key)).unwrap();
                assert!(row.expand(0.5).contains_rect(rect), "{width}: {rect:?} outside {row:?}");
                assert!(rect.contains(registry::handle_at(&format!("dialog:Example:{}", option.label)).unwrap()));
            }
            let at = registry::handle_at("prefs:geometry_cards:centre").unwrap();
            for down in [true, false] {
                frame(&ctx, pointer(at, down), |ui| {
                    ui.set_width(width);
                    let picked = settings_picture_choices(ui, "geometry_cards", "Example", &options, 2, true);
                    if !down {
                        assert_eq!(picked, Some(1), "the real card selects its option");
                    }
                });
            }
        }
    }
    solidcraft_theme::set_dark(previous);
}

#[test]
fn row_label_tooltips_override_full_label_text_only_inside_their_scope() {
    let label = "A long parameter label that must elide";
    let tip = "What this parameter controls";
    let ctx = crate::test_support::context();
    ctx.global_style_mut(|style| style.interaction.tooltip_delay = 0.0);
    let draw = |ui: &mut Ui| {
        with_row_label_tooltip(ui, tip, |ui| row(ui, label, |ui| ui.label("Value")));
        row(ui, "Radius", |ui| ui.label("Value"));
    };
    let paint = |events| {
        raw_frame(
            &ctx,
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(320.0, 400.0))),
                time: Some(ctx.cumulative_frame_nr() as f64 / 15.0),
                focused: true,
                events,
                ..Default::default()
            },
            |ui| {
                ui.set_width(280.0);
                scope(ui, draw);
            },
        )
    };
    for _ in 0..3 {
        paint(vec![]);
    }
    for (name, expected) in [(label, tip), ("Radius", "Radius")] {
        let output = paint(vec![egui::Event::PointerGone]);
        let text = text_shapes(&output).find(|text| text.galley.text() == name).expect("painted row label");
        let at = text.pos + text.galley.size() / 2.0;
        paint(vec![egui::Event::PointerMoved(at)]);
        for _ in 0..3 {
            paint(vec![]);
        }
        let output = paint(vec![]);
        let count = text_shapes(&output).filter(|text| text.galley.text() == expected).count();
        assert_eq!(count, if name == "Radius" { 2 } else { 1 }, "{name}: only the intended tooltip is painted");
    }
}

#[test]
fn row_height_and_label_column_are_fixed_for_long_labels_and_expressions() {
    let ctx = crate::test_support::context();
    let mut value = "width / 2 + 10 mm".to_string();
    for _ in 0..3 {
        frame(&ctx, vec![], |ui| {
            row(ui, "A long parameter label that must elide", |ui| {
                field(ui, &mut value);
            });
            row(ui, "Radius", |ui| {
                field(ui, &mut value);
            });
        });
    }
    let first = registry::rect_of("dialog:row:A long parameter label that must elide").unwrap();
    let second = registry::rect_of("dialog:row:Radius").unwrap();
    let label = registry::rect_of("dialog:label:Radius").unwrap();
    let control = registry::rect_of("dialog:control:Radius").unwrap();
    assert_eq!(first.height(), Tokens::get().control_height);
    assert_eq!(second.height(), Tokens::get().control_height);
    // The design's grid: 28 pt controls with a 5 pt row gap and a 70 pt label column (wider only for long labels).
    assert!(
        (second.top() - first.bottom() - Tokens::get().form_gap_y).abs() < 0.01,
        "Plex Mono fields must retain the row gap: {first:?}, {second:?}"
    );
    // The long label widens the column for every row, up to 1.6 times the design's width.
    assert_eq!(label.width(), Tokens::get().form_label_width * 1.6);
    assert_eq!(registry::rect_of("dialog:label:A long parameter label that must elide").unwrap().width(), label.width());
    assert!(control.left() > label.right());
    assert!(second.contains_rect(control));
    assert_eq!(value, "width / 2 + 10 mm");
}

/// A read-only value too long for one line wraps and its row grows to hold every line, instead
/// of the fixed 28 pt row clipping the lines below the first (Redefine Sketch Plane's "Current
/// plane"); a short value keeps the ordinary row height.
#[test]
fn value_row_wraps_a_long_value_and_grows_to_hold_it() {
    let ctx = crate::test_support::context();
    let long = "A construction plane with a long name, offset from the XY plane and seen from its other side";
    for _ in 0..3 {
        frame(&ctx, vec![], |ui| {
            value_row(ui, "Current plane", long);
            value_row(ui, "Short", "XY");
        });
    }
    let t = Tokens::get();
    let grown = registry::rect_of("dialog:row:Current plane").unwrap();
    let control = registry::rect_of("dialog:control:Current plane").unwrap();
    let short = registry::rect_of("dialog:row:Short").unwrap();
    assert_eq!(short.height(), t.control_height);
    assert!(grown.height() > t.control_height, "the long value's row grows: {grown:?}");
    let lines = ctx.fonts_mut(|f| f.layout(long.to_owned(), egui::FontId::proportional(t.font_label), t.form_ink, control.width())).size().y;
    assert!(lines > t.font_label * 1.5, "the value takes several lines: {lines}");
    assert!(control.height() >= lines, "every line fits the control column: {control:?}, {lines}");
    assert!(short.top() >= grown.bottom(), "the next row starts below the grown one: {grown:?}, {short:?}");
}

#[test]
fn section_toggles_persist_per_dialog() {
    let ctx = crate::test_support::context();
    let mut a = 0;
    let mut b = 0;
    let mut draw = |ui: &mut Ui| {
        ui.push_id("fillet", |ui| {
            section(ui, "opts", "Options", false, |_| a += 1);
        });
        ui.push_id("hole", |ui| {
            section(ui, "opts", "Hole options", false, |_| b += 1);
        });
    };
    frame(&ctx, vec![], &mut draw);
    let pos = registry::handle_at("dialog:section:Options").unwrap();
    frame(&ctx, pointer(pos, true), &mut draw);
    frame(&ctx, pointer(pos, false), &mut draw);
    frame(&ctx, vec![], &mut draw);
    assert!(a >= 2);
    assert_eq!(b, 0);
    let before = a;
    frame(&ctx, pointer(pos, true), |ui| {
        ui.push_id("fillet", |ui| {
            section(ui, "opts", "Options", false, |_| a += 1);
        });
    });
    frame(&ctx, pointer(pos, false), |ui| {
        ui.push_id("fillet", |ui| {
            section(ui, "opts", "Options", false, |_| a += 1);
        });
    });
    assert_eq!(a, before + 1);
}

#[test]
fn short_labels_keep_the_designs_label_column_and_long_ones_widen_it_for_every_row() {
    fn widths(labels: [&str; 2]) -> (f32, f32) {
        let ctx = crate::test_support::context();
        let mut value = String::new();
        for _ in 0..3 {
            let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                ui.set_width(300.0);
                scope(ui, |ui| {
                    for label in labels {
                        row(ui, label, |ui| {
                            field(ui, &mut value);
                        });
                    }
                });
            });
            output.textures_delta.clear();
        }
        (
            registry::rect_of(&format!("dialog:label:{}", labels[0])).unwrap().width(),
            registry::rect_of(&format!("dialog:label:{}", labels[1])).unwrap().width(),
        )
    }
    let t = Tokens::get();
    let (a, b) = widths(["Edges", "Radius"]);
    assert_eq!((a, b), (t.form_label_width, t.form_label_width));
    let (a, b) = widths(["Edges", "Counterbore depth"]);
    assert_eq!(a, b, "one column per form");
    assert!(a > t.form_label_width && a <= t.form_label_width * 1.6, "{a}");
}

/// An action row is a button across the control column, lined up with the field rows above it,
/// and says when it is clicked; only then.
#[test]
fn an_action_row_is_a_button_in_the_control_column_that_says_when_it_is_clicked() {
    let ctx = crate::test_support::context();
    let mut width = "20 mm".to_string();
    let mut clicks = Vec::new();
    let mut draw = |ui: &mut Ui| {
        scope(ui, |ui| {
            expr_row(ui, "Width", &mut width);
            clicks.push(action_row(ui, "fit", "Fit to box"));
        });
    };
    frame(&ctx, vec![], &mut draw);
    let field = registry::rect_of("dialog:control:Width").unwrap();
    let action = registry::rect_of("dialog:action:fit").unwrap();
    assert!((action.left() - field.left()).abs() < 0.5, "{action:?} under {field:?}");
    assert!((action.right() - field.right()).abs() < 0.5, "{action:?} under {field:?}");
    assert!((action.height() - row_height()).abs() < 0.5, "{action:?}");
    assert!(action.top() >= field.bottom(), "{action:?} below {field:?}");
    let at = registry::handle_at("dialog:action:fit").unwrap();
    frame(&ctx, pointer(at, true), &mut draw);
    frame(&ctx, pointer(at, false), &mut draw);
    frame(&ctx, vec![], &mut draw);
    assert_eq!(clicks, [false, false, true, false]);
}

/// Scrubbing a row's label is pointer-only: a single Tab moves directly from one expression
/// field to the next without stopping on the scrub drag region, and Shift+Tab moves directly back.
#[test]
fn tab_moves_directly_between_adjacent_expr_rows() {
    let ctx = crate::test_support::context();
    let mut distance = "10 mm".to_string();
    let mut distance2 = "20 mm".to_string();
    let ids = std::cell::Cell::new((None, None));
    let mut draw = |ui: &mut Ui| {
        let egui_ctx = ui.ctx().clone();
        scope(ui, |ui| {
            let (_, fields) = value_fields(&egui_ctx, || {
                expr_row(ui, "Distance", &mut distance);
                expr_row(ui, "Distance 2", &mut distance2);
                false
            });
            if fields.len() == 2 {
                ids.set((Some(fields[0].0), Some(fields[1].0)));
            }
        });
    };
    frame(&ctx, vec![], &mut draw);
    let (first, second) = (ids.get().0.expect("first editor id"), ids.get().1.expect("second editor id"));
    ctx.memory_mut(|m| m.request_focus(first));
    frame(&ctx, vec![], &mut draw);
    assert_eq!(ctx.memory(|m| m.focused()), Some(first));

    // One Tab lands directly on Distance 2, never on its scrub label.
    frame(&ctx, key(egui::Key::Tab), &mut draw);
    assert_eq!(ctx.memory(|m| m.focused()), Some(second), "Tab should move directly to Distance 2");

    // One Shift+Tab moves directly back to Distance (egui applies FocusDirection::Previous on the next pass).
    let shift_tab =
        vec![egui::Event::Key { key: egui::Key::Tab, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::SHIFT }];
    frame(&ctx, shift_tab, &mut draw);
    frame(&ctx, vec![], &mut draw);
    assert_eq!(ctx.memory(|m| m.focused()), Some(first), "Shift+Tab should move directly back to Distance");
}
