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
fn theme_rail_cues_follow_hidden_content() {
    use crate::ui_kit::preferences::theme_rail_cues;
    assert_eq!(theme_rail_cues(0.0, 0.0), (false, false));
    assert_eq!(theme_rail_cues(0.0, 300.0), (false, true));
    assert_eq!(theme_rail_cues(150.0, 300.0), (true, true));
    assert_eq!(theme_rail_cues(300.0, 300.0), (true, false));
    assert_eq!(theme_rail_cues(0.25, 0.5), (false, false));
}

#[test]
fn theme_rail_arrows_scroll_one_card_and_hide_at_ends() {
    let ctx = crate::fonts::test_context();
    let t = Tokens::get();
    let width = t.form_label_width + t.space_24 + t.space_6;
    let draw = |ui: &mut Ui| {
        theme_rail(
            ui,
            |ui| {
                for i in 0..3 {
                    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 80.0), egui::Sense::click());
                    scenario::publish_target(&format!("test:theme:{i}"), rect);
                }
            },
            |_| {},
        );
    };
    for _ in 0..3 {
        frame(&ctx, vec![], draw);
    }
    assert!(scenario::rect_of("prefs:theme_rail:Scroll left").is_none());
    let start = scenario::rect_of("test:theme:0").unwrap().left();
    let right = scenario::handle_at("prefs:theme_rail:Scroll right").unwrap();
    assert!((right.y - scenario::rect_of("test:theme:0").unwrap().center().y).abs() < 1.0);
    frame(&ctx, pointer(right, true), draw);
    frame(&ctx, pointer(right, false), draw);
    frame(&ctx, vec![], draw);
    let after = scenario::rect_of("test:theme:0").unwrap().left();
    assert!((start - after - width - t.space_8).abs() < 1.0);
    assert!(scenario::rect_of("prefs:theme_rail:Scroll left").is_some());
    for _ in 0..4 {
        if let Some(pos) = scenario::handle_at("prefs:theme_rail:Scroll right") {
            frame(&ctx, pointer(pos, true), draw);
            frame(&ctx, pointer(pos, false), draw);
            frame(&ctx, vec![], draw);
        }
    }
    assert!(scenario::rect_of("prefs:theme_rail:Scroll right").is_none());
    for _ in 0..4 {
        if let Some(pos) = scenario::handle_at("prefs:theme_rail:Scroll left") {
            frame(&ctx, pointer(pos, true), draw);
            frame(&ctx, pointer(pos, false), draw);
            frame(&ctx, vec![], draw);
        }
    }
    assert!(scenario::rect_of("prefs:theme_rail:Scroll left").is_none());
}

#[test]
fn theme_rail_chevrons_have_no_idle_box_and_show_hover_and_focus() {
    let ctx = crate::fonts::test_context();
    let t = Tokens::get();
    let rect = egui::Rect::from_min_size(egui::pos2(20.0, 20.0), egui::vec2(t.control_height, t.control_height));
    let draw = |events| {
        crate::test_support::frame(&ctx, egui::RawInput { events, focused: true, ..Default::default() }, |ui| {
            crate::ui_kit::preferences::theme_rail_arrow(ui, rect, 1.0, "Scroll right");
        })
    };
    let idle = draw(vec![]);
    assert!(!idle.shapes.iter().any(|s| matches!(s.shape, egui::Shape::Rect(_))));
    assert!(!idle.shapes.iter().any(|s| matches!(s.shape, egui::Shape::Circle(_))), "no backing until hovered");
    let hover = draw(vec![egui::Event::PointerMoved(rect.center())]);
    assert_eq!(hover.platform_output.cursor_icon, egui::CursorIcon::PointingHand);
    assert!(hover.shapes.iter().any(|s| matches!(&s.shape, egui::Shape::Circle(c) if c.fill == t.hover && c.radius == rect.width() / 2.0)));
    draw(pointer(rect.center(), true));
    draw(pointer(rect.center(), false));
    let focus = draw(vec![egui::Event::PointerMoved(egui::pos2(200.0, 200.0))]);
    assert!(
        !focus.shapes.iter().any(|s| matches!(&s.shape, egui::Shape::Rect(r) if r.stroke.color == t.form_primary)),
        "a mouse click leaves no outline"
    );
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
