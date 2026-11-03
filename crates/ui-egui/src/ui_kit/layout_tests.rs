use crate::ui_kit::test_support::*;
use crate::ui_kit::*;

use crate::{scenario, theme::Tokens};
use egui::Ui;

#[test]
fn status_lines_wrap_at_form_width_and_leave_room_for_the_next_row() {
    let previous_dark = crate::theme::is_dark();
    for dark in [false, true] {
        crate::theme::set_dark(dark);
        let base = Tokens::get();
        for width in [160.0, 280.0] {
            for font_label in [base.font_label, base.font_label * 1.8] {
                let _theme = crate::theme::scoped_tokens(Tokens { font_label, ..base });
                for (tone, ink) in [(StatusTone::Dim, base.text_dim), (StatusTone::Warning, base.warning), (StatusTone::Error, base.error)] {
                    let ctx = crate::fonts::test_context();
                    let message = "needs a joint that turns and one that slides";
                    let mut message_rect = egui::Rect::NOTHING;
                    let output = crate::test_support::frame(
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
                    let next = scenario::rect_of("dialog:row:Radius").unwrap();
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
    crate::theme::set_dark(previous_dark);
}

#[test]
fn carousel_window_clamps_offsets_and_keeps_a_card_on_narrow_pages() {
    use crate::ui_kit::preferences_cards::carousel_window;
    assert_eq!(carousel_window(0, 9, 280.0, 88.0, 8.0, 26.0), (0, 0));
    assert_eq!(carousel_window(6, 0, 280.0, 88.0, 8.0, 26.0), (0, 2));
    assert_eq!(carousel_window(6, 2, 280.0, 88.0, 8.0, 26.0), (2, 2));
    assert_eq!(carousel_window(6, 99, 280.0, 88.0, 8.0, 26.0), (4, 2));
    assert_eq!(carousel_window(6, 99, 100.0, 88.0, 8.0, 26.0), (5, 1));
}

#[test]
fn shared_picture_choices_wrap_inside_the_row_and_reserve_their_height() {
    let choices = [
        PictureChoice { key: "first", label: "First", figure: |_, _| {} },
        PictureChoice { key: "second", label: "Second", figure: |_, _| {} },
        PictureChoice { key: "third", label: "Third", figure: |_, _| {} },
    ];
    for dark in [false, true] {
        let _theme = crate::theme::scoped_tokens(if dark { Tokens::dark() } else { Tokens::light() });
        for width in [120.0, 260.0, 600.0] {
            for sized in [false, true] {
                let ctx = crate::fonts::test_context();
                crate::test_support::frame(
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
                            let row_rect = scenario::rect_of("dialog:row:Choices").unwrap();
                            for choice in &choices {
                                let rect = scenario::rect_of(&format!("prefs:shared:{}", choice.key)).unwrap();
                                assert!(row_rect.expand(0.5).contains_rect(rect), "width={width}, sized={sized}: {rect:?} outside {row_rect:?}");
                            }
                            let first = scenario::rect_of("prefs:shared:first").unwrap();
                            let last = scenario::rect_of("prefs:shared:third").unwrap();
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
        let ctx = crate::fonts::test_context();
        ctx.enable_accesskit();
        let picked = Cell::new(None);
        let draw = |ui: &mut Ui| {
            ui.add_enabled_ui(parent_enabled, |ui| {
                picked.set(settings_picture_choices(ui, "shared", "Choices", &choices, 0, enabled));
            });
        };
        frame(&ctx, vec![], draw);
        let at = scenario::handle_at("prefs:shared:second").unwrap();
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
fn carousel_arrows_move_a_page_of_the_cards_that_fit_at_each_width() {
    let count = 11;
    for width in [280.0_f32, 480.0, 700.0] {
        let ctx = crate::fonts::test_context();
        let shown = || -> Vec<usize> { (0..count).filter(|i| scenario::rect_of(&format!("page:item:{i}")).is_some()).collect() };
        let draw = |events: Vec<egui::Event>| {
            scenario::clear_handles();
            crate::test_support::frame(
                &ctx,
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(width + 40.0, 400.0))),
                    events,
                    ..Default::default()
                },
                |ui| {
                    ui.set_width(width);
                    scope(ui, |ui| {
                        carousel(ui, ("page_carousel", width as u32), count, 88.0, 96.0, |ui, i| {
                            let (r, _) = ui.allocate_exact_size(egui::vec2(88.0, 90.0), egui::Sense::hover());
                            scenario::publish_rect(&format!("page:item:{i}"), r);
                        });
                    });
                },
            );
        };
        let click = |label: &str| {
            let at = scenario::handle_at(&format!("prefs:carousel:{}", crate::i18n::tr(label))).unwrap();
            draw(pointer(at, true));
            draw(pointer(at, false));
            draw(vec![]);
        };
        draw(vec![]);
        draw(vec![]);
        let page = shown().len();
        assert!(page > 1 && page < count, "width {width}: {page} cards fit");
        assert_eq!(shown(), (0..page).collect::<Vec<_>>());
        click("prefs.carousel_previous");
        assert_eq!(shown(), (0..page).collect::<Vec<_>>(), "width {width}: previous is inert at the start");
        click("prefs.carousel_next");
        let second = page.min(count - page);
        assert_eq!(shown(), (second..second + page).collect::<Vec<_>>(), "width {width}: next shows the cards that were hidden");
        for _ in 0..count {
            click("prefs.carousel_next");
        }
        let last: Vec<usize> = (count - page..count).collect();
        assert_eq!(shown(), last, "width {width}: the last page ends on the last card");
        click("prefs.carousel_previous");
        let back = (count - page).saturating_sub(page);
        assert_eq!(shown(), (back..back + page).collect::<Vec<_>>(), "width {width}: previous shows the cards before");
        for _ in 0..count {
            click("prefs.carousel_previous");
        }
        assert_eq!(shown(), (0..page).collect::<Vec<_>>(), "width {width}: previous stops at the first card");
    }
}

#[test]
fn theme_carousel_moves_and_disables_its_ends_without_selecting() {
    let ctx = crate::fonts::test_context();
    let sample = ThemeSample::from_tokens(&Tokens::light());
    let draw = |ui: &mut Ui| {
        let bounds = ui.max_rect();
        carousel(ui, "test_themes", 3, 88.0, 96.0, |ui, index| {
            let label = ["Light", "Dark", "System"][index];
            let response = picture_card(
                ui,
                &Card {
                    label,
                    sub: Some("Built in"),
                    selected: index == 0,
                    enabled: true,
                    options: (index == 2).then_some("Has options"),
                    check: false,
                    aspect: 124.0 / 76.0,
                    figure: &|p, r| paint_theme_thumbnail(p, r, &sample),
                },
                88.0,
            );
            assert!(!response.clicked(), "scrolling must not select a card");
            assert!(bounds.contains_rect(response.rect));
            scenario::publish_target(&format!("test:theme:{index}"), response.rect);
        });
    };
    for _ in 0..3 {
        frame(&ctx, vec![], draw);
    }
    let previous = format!("prefs:carousel:{}", crate::i18n::tr("prefs.carousel_previous"));
    let next = format!("prefs:carousel:{}", crate::i18n::tr("prefs.carousel_next"));
    let click = |handle: &str| {
        let at = scenario::handle_at(handle).unwrap();
        frame(&ctx, pointer(at, true), draw);
        frame(&ctx, pointer(at, false), draw);
        frame(&ctx, vec![], draw);
    };
    let start = scenario::rect_of("test:theme:0").unwrap().left();
    click(&previous);
    assert_eq!(scenario::rect_of("test:theme:0").unwrap().left(), start, "previous is inert at the start");
    click(&next);
    assert!(scenario::rect_of("test:theme:0").is_none());
    assert!((scenario::rect_of("test:theme:1").unwrap().left() - start).abs() < 1.0, "the last page ends on the last card");
    assert!(scenario::rect_of("test:theme:2").is_some());
    click(&next);
    assert!((scenario::rect_of("test:theme:1").unwrap().left() - start).abs() < 1.0, "next is inert at the end");
    click(&previous);
    assert!(scenario::rect_of("test:theme:0").is_some());
    assert!(scenario::rect_of("test:theme:2").is_none());
}

#[test]
fn carousel_arrows_expose_disabled_state_hover_and_keyboard_focus() {
    let ctx = crate::fonts::test_context();
    ctx.enable_accesskit();
    let draw = |events| {
        scenario::clear_handles();
        crate::test_support::frame(
            &ctx,
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(320.0, 400.0))),
                events,
                focused: true,
                ..Default::default()
            },
            |ui| {
                ui.set_width(280.0);
                scope(ui, |ui| {
                    carousel(ui, "keyboard_carousel", 3, 88.0, 96.0, |ui, i| {
                        let (r, _) = ui.allocate_exact_size(egui::vec2(88.0, 90.0), egui::Sense::hover());
                        scenario::publish_rect(&format!("keyboard:item:{i}"), r);
                    });
                });
            },
        )
    };
    let previous = crate::i18n::tr("prefs.carousel_previous");
    let next = crate::i18n::tr("prefs.carousel_next");
    draw(vec![]);
    let idle = draw(vec![]);
    let tree = idle.platform_output.accesskit_update.as_ref().unwrap();
    for (label, disabled) in [(previous, true), (next, false)] {
        let nodes: Vec<_> =
            tree.nodes.iter().filter(|(_, node)| node.role() == egui::accesskit::Role::Button && node.label() == Some(label)).collect();
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].1.is_disabled(), disabled);
    }
    let at = scenario::handle_at(&format!("prefs:carousel:{next}")).unwrap();
    let hover = draw(vec![egui::Event::PointerMoved(at)]);
    assert_eq!(hover.platform_output.cursor_icon, egui::CursorIcon::PointingHand);
    draw(vec![egui::Event::PointerMoved(egui::pos2(310.0, 390.0))]);
    let focus = draw(key(egui::Key::Tab));
    assert!(
        focus.shapes.iter().any(|s| matches!(&s.shape, egui::Shape::Rect(r) if r.stroke.color == Tokens::get().form_primary)),
        "keyboard focus uses the standard ring"
    );
    draw(key(egui::Key::Enter));
    draw(vec![]);
    assert!(scenario::rect_of("keyboard:item:0").is_none(), "keyboard activates Next");
    assert!(scenario::rect_of("keyboard:item:2").is_some());
}

#[test]
fn layout_picture_choices_wrap_inside_the_indexed_row_and_keep_real_targets() {
    let previous = crate::theme::is_dark();
    for dark in [false, true] {
        crate::theme::set_dark(dark);
        for width in [140.0, 280.0, 646.0] {
            let ctx = crate::fonts::test_context();
            let options = [
                PictureChoice { key: "bottom", label: "Bottom", figure: paint_history_bottom },
                PictureChoice { key: "left", label: "Left", figure: paint_history_left },
                PictureChoice { key: "under_browser", label: "Under browser", figure: paint_history_under_browser },
                PictureChoice { key: "browser", label: "In browser", figure: paint_history_in_browser },
            ];
            frame(&ctx, vec![], |ui| {
                ui.set_width(width);
                settings_picture_choices(ui, "geometry_history", "History", &options, 2, true);
            });
            let row = scenario::rect_of("dialog:row:History").unwrap();
            for option in &options {
                let rect = scenario::rect_of(&format!("prefs:geometry_history:{}", option.key)).unwrap();
                assert!(row.expand(0.5).contains_rect(rect), "{width}: {rect:?} outside {row:?}");
                assert!(rect.contains(scenario::handle_at(&format!("dialog:History:{}", option.label)).unwrap()));
            }
            let at = scenario::handle_at("prefs:geometry_history:left").unwrap();
            for down in [true, false] {
                frame(&ctx, pointer(at, down), |ui| {
                    ui.set_width(width);
                    let picked = settings_picture_choices(ui, "geometry_history", "History", &options, 2, true);
                    if !down {
                        assert_eq!(picked, Some(1), "the real card selects its option");
                    }
                });
            }
        }
    }
    crate::theme::set_dark(previous);
}

#[test]
fn row_height_and_label_column_are_fixed_for_long_labels_and_expressions() {
    let ctx = crate::fonts::test_context();
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
    let first = scenario::rect_of("dialog:row:A long parameter label that must elide").unwrap();
    let second = scenario::rect_of("dialog:row:Radius").unwrap();
    let label = scenario::rect_of("dialog:label:Radius").unwrap();
    let control = scenario::rect_of("dialog:control:Radius").unwrap();
    assert_eq!(first.height(), Tokens::get().control_height);
    assert_eq!(second.height(), Tokens::get().control_height);
    // The design's grid: 28 pt controls with a 5 pt row gap and a 70 pt label column (wider only for long labels).
    assert!(
        (second.top() - first.bottom() - Tokens::get().form_gap_y).abs() < 0.01,
        "Plex Mono fields must retain the row gap: {first:?}, {second:?}"
    );
    // The long label widens the column for every row, up to 1.6 times the design's width.
    assert_eq!(label.width(), Tokens::get().form_label_width * 1.6);
    assert_eq!(scenario::rect_of("dialog:label:A long parameter label that must elide").unwrap().width(), label.width());
    assert!(control.left() > label.right());
    assert!(second.contains_rect(control));
    assert_eq!(value, "width / 2 + 10 mm");
}

#[test]
fn section_toggles_persist_per_dialog() {
    let ctx = crate::fonts::test_context();
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
    let pos = scenario::handle_at("dialog:section:Options").unwrap();
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
        let ctx = egui::Context::default();
        crate::fonts::setup_fonts(&ctx);
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
            scenario::rect_of(&format!("dialog:label:{}", labels[0])).unwrap().width(),
            scenario::rect_of(&format!("dialog:label:{}", labels[1])).unwrap().width(),
        )
    }
    let t = Tokens::get();
    let (a, b) = widths(["Edges", "Radius"]);
    assert_eq!((a, b), (t.form_label_width, t.form_label_width));
    let (a, b) = widths(["Edges", "Counterbore depth"]);
    assert_eq!(a, b, "one column per form");
    assert!(a > t.form_label_width && a <= t.form_label_width * 1.6, "{a}");
}
