use super::*;
use crate::test_support as harness;
use solidcraft_ui_registry as registry;

#[test]
fn fitting_report_rows_stay_inside_list_and_enclosing_horizontal_viewport() {
    for ppp in [1.0, 1.25, 2.0] {
        for width in [340.0, 499.0, 500.3, 524.0, 760.0] {
            for count in [1, 12] {
                let ctx = crate::test_support::context();
                ctx.set_pixels_per_point(ppp);
                for _ in 0..3 {
                    harness::frame(&ctx, vec![], |ui| {
                        ui.set_width(width);
                        ui.spacing_mut().scroll.content_margin = egui::Margin::same(1);
                        let outer =
                            egui::ScrollArea::horizontal().max_width((width * ppp).floor() / ppp).content_margin(egui::Margin::ZERO).show(ui, |ui| {
                                let t = Tokens::get();
                                let columns = [
                                    crate::Column::new("Crash Reports", 1.0).flex(),
                                    crate::Column::new("", 0.0).icon_only().min_content_width(
                                        crate::icon_label_width(ui, "Copy path", Some("copy"), false)
                                            + crate::icon_label_width(ui, "Report…", Some("bug_report"), false)
                                            + t.control_height
                                            + t.space_8 * 2.0,
                                    ),
                                ];
                                report_list(ui, "reports", 160.0, &columns, "", |ui, table| {
                                    for index in 0..count {
                                        ui.push_id(index, |ui| {
                                            ui.add_space(Tokens::get().space_10);
                                            report_header(
                                                ui,
                                                table,
                                                &index.to_string(),
                                                "2026-10-18 21:04:12 Internal error in a command ×3",
                                                |ui| {
                                                    crate::bin_button(ui, "Delete");
                                                    crate::icon_label_button(ui, "Copy path", "copy", true, false);
                                                    crate::icon_label_button(ui, "Report…", "bug_report", true, false);
                                                },
                                            );
                                            crate::text(ui, "solid.fillet · replayable", crate::Text::MonoNote("detail"));
                                            crate::text(
                                                ui,
                                                "The radius produced a degenerate face while rebuilding Fillet2.",
                                                crate::Text::Report("message", false),
                                            );
                                            crate::divider(ui, "end");
                                        });
                                    }
                                });
                            });
                        assert!(
                            outer.content_size.x <= outer.inner_rect.width().ceil(),
                            "{width} at {ppp}, {count} rows: {:?} outside {:?}",
                            outer.content_size,
                            outer.inner_rect
                        );
                    });
                    let header = registry::rect_of("dialog:column:reports:Crash Reports").unwrap();
                    let first = registry::rect_of("dialog:table:reports:0").unwrap();
                    assert!(header.bottom() <= first.top(), "one header precedes the report rows");
                    let last_actions = registry::rect_of(&format!("dialog:table:reports:{}:cell:", count - 1)).unwrap();
                    for label in ["Delete", "Copy path", "Report…"] {
                        let button = registry::rect_of(&format!("dialog:button::{label}")).unwrap();
                        assert!(last_actions.expand(1.0).contains_rect(button), "{label} escaped {last_actions:?}: {button:?}");
                    }
                    let viewport = registry::rect_of("dialog:list:reports:viewport").unwrap();
                    let content = registry::count_of("dialog:list:reports:content_width").unwrap() as f32;
                    assert!(content <= viewport.width().ceil(), "{width} at {ppp}, {count} rows: content {content}, viewport {viewport:?}");
                    let viewport = registry::rect_of("dialog:table:reports:viewport").unwrap();
                    let content = registry::count_of("dialog:table:reports:content_width").unwrap() as f32;
                    assert!(content <= viewport.width().ceil(), "wrapping report details widened the table: {content} outside {viewport:?}");
                }
            }
        }
    }
}

#[test]
fn crash_cards_are_separate_bounded_and_keep_actions_independent() {
    for ppp in [1.0, 1.25, 2.0] {
        for width in [340.0, 500.3, 760.0] {
            let ctx = harness::context();
            ctx.enable_accesskit();
            ctx.set_pixels_per_point(ppp);
            let states = std::cell::RefCell::new([false; 3]);
            let clicked = std::cell::RefCell::new(Vec::new());
            let draw = |events| {
                registry::clear_handles();
                harness::raw_frame(
                    &ctx,
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(width + 40.0, 900.0))),
                        events,
                        focused: true,
                        ..Default::default()
                    },
                    |ui| {
                        crate::scope(ui, |ui| {
                            ui.set_width(width);
                            let output = egui::ScrollArea::vertical().id_salt("crash_cards").max_height(800.0).show(ui, |ui| {
                                ui.spacing_mut().item_spacing.y = Tokens::get().space_8;
                                for index in 0..3 {
                                    report_card(
                                        ui,
                                        &index.to_string(),
                                        ReportCardSummary {
                                            title: "Fillet stopped unexpectedly with a very long command name",
                                            subtitle: "24 Nov, 09:30 · happened 3 times · replayable",
                                            icon: "warning",
                                            ink: Tokens::get().form_error,
                                        },
                                        ReportCardDetails {
                                            fields: &[
                                                ("version", "Version", "0.49.0 · abc1234"),
                                                ("command", "Command", "solid.fillet"),
                                                ("system", "System", "Windows · x86_64"),
                                                ("empty", "Empty", ""),
                                            ],
                                            code: "index out of bounds\nblends.rs:418",
                                        },
                                        &mut states.borrow_mut()[index],
                                        "Copy path",
                                        |action, response| {
                                            if response.clicked() {
                                                clicked.borrow_mut().push((index, action));
                                            }
                                        },
                                    );
                                }
                            });
                            assert!(output.content_size.x <= output.inner_rect.width().ceil(), "{width} at {ppp}: {:?}", output.content_size);
                        });
                    },
                )
            };
            for _ in 0..3 {
                draw(vec![]);
            }
            for index in 0..3 {
                let card = registry::rect_of(&format!("dialog:card:{index}")).unwrap();
                let t = solidcraft_theme::Tokens::get();
                let expected_height = t.control_height + t.space_4 + 2.0 * (t.space_4 + t.container_border(t.border).width);
                assert!((card.height() - expected_height).abs() <= 3.0);
                for part in ["title", "subtitle", "toggle", "tile", "report", "delete"] {
                    assert!(card.expand(1.0).contains_rect(registry::rect_of(&format!("dialog:card:{index}:{part}")).unwrap()));
                }
                assert!(registry::rect_of(&format!("dialog:card:{index}:version")).is_none());
                assert!(registry::rect_of(&format!("dialog:card:{index}:copy")).is_none());
                let toggle = registry::rect_of(&format!("dialog:card:{index}:toggle")).unwrap();
                for action in ["report", "delete"] {
                    assert!(!toggle.intersects(registry::rect_of(&format!("dialog:card:{index}:{action}")).unwrap()));
                }
                if index > 0 {
                    let previous = registry::rect_of(&format!("dialog:card:{}", index - 1)).unwrap();
                    assert!((card.top() - previous.bottom() - Tokens::get().space_8).abs() <= 1.0);
                }
            }
            let click = |name: &str| {
                let pos = registry::handle_at(name).unwrap();
                for pressed in [true, false] {
                    draw(harness::pointer(pos, pressed));
                }
                draw(vec![]);
            };
            click("dialog:card:1:report");
            assert_eq!(*clicked.borrow(), [(1, ReportCardAction::Report)]);
            assert_eq!(*states.borrow(), [false; 3]);
            click("dialog:card:1:tile");
            assert_eq!(*states.borrow(), [false, true, false]);
            let card = registry::rect_of("dialog:card:1").unwrap();
            for field in ["version", "command", "system", "code", "copy", "copy_details"] {
                assert!(card.contains_rect(registry::rect_of(&format!("dialog:card:1:{field}")).unwrap()), "{field}");
            }
            assert!(registry::rect_of("dialog:card:1:empty").is_none());
            let version = registry::rect_of("dialog:card:1:version").unwrap();
            let command = registry::rect_of("dialog:card:1:command").unwrap();
            if width < 500.0 {
                assert!(command.top() > version.bottom());
            } else {
                assert!((command.top() - version.top()).abs() < 1.0);
            }
            click("dialog:card:1:copy");
            assert_eq!(clicked.borrow().last(), Some(&(1, ReportCardAction::CopyPath)));
            let pos = registry::handle_at("dialog:card:1:copy_details").unwrap();
            draw(harness::pointer(pos, true));
            let copied = draw(harness::pointer(pos, false));
            assert!(
                copied
                    .platform_output
                    .commands
                    .iter()
                    .any(|command| matches!(command, egui::OutputCommand::CopyText(text) if text == "index out of bounds\nblends.rs:418"))
            );
            click("dialog:card:1:chevron");
            assert_eq!(*states.borrow(), [false; 3]);
            let output = draw(vec![egui::Event::Key {
                key: egui::Key::Space,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }]);
            assert_eq!(*states.borrow(), [false, true, false], "Space opens the focused summary");
            let assert_header_has_no_outline = |output: &egui::FullOutput| {
                let header = registry::rect_of("dialog:card:1:toggle").unwrap();
                let mut tinted = false;
                for shape in &output.shapes {
                    if let egui::Shape::Rect(rect) = &shape.shape
                        && rect.rect == header
                    {
                        tinted = true;
                        assert_eq!(rect.stroke, egui::Stroke::NONE, "header focus/expanded state uses tint only");
                    }
                }
                assert!(tinted, "the hovered/expanded header has its tint");
            };
            assert_header_has_no_outline(&output);
            assert!(
                output
                    .platform_output
                    .accesskit_update
                    .as_ref()
                    .unwrap()
                    .nodes
                    .iter()
                    .any(|(_, node)| node.role() == egui::accesskit::Role::DisclosureTriangle && node.is_expanded() == Some(true))
            );
            draw(vec![egui::Event::Key {
                key: egui::Key::Space,
                physical_key: None,
                pressed: false,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }]);
            let closed = draw(vec![egui::Event::Key {
                key: egui::Key::Enter,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }]);
            assert_eq!(*states.borrow(), [false; 3], "Enter closes the focused summary");
            assert_header_has_no_outline(&closed);
            click("dialog:card:1:delete");
            assert_eq!(clicked.borrow().last(), Some(&(1, ReportCardAction::Delete)));
            assert_eq!(*states.borrow(), [false; 3]);
        }
    }
}
