use super::*;
use crate::scenario;
use crate::ui_kit::{self, test_support as harness};

#[test]
fn fitting_crash_rows_stay_inside_list_and_enclosing_horizontal_viewport() {
    for ppp in [1.0, 1.25, 2.0] {
        for width in [340.0, 499.0, 500.3, 524.0, 760.0] {
            for count in [1, 12] {
                let ctx = crate::fonts::test_context();
                ctx.set_pixels_per_point(ppp);
                for _ in 0..3 {
                    harness::frame(&ctx, vec![], |ui| {
                        ui.set_width(width);
                        ui.spacing_mut().scroll.content_margin = egui::Margin::same(1);
                        let outer =
                            egui::ScrollArea::horizontal().max_width((width * ppp).floor() / ppp).content_margin(egui::Margin::ZERO).show(ui, |ui| {
                                report_list(ui, "reports", 160.0, |ui| {
                                    for index in 0..count {
                                        ui.push_id(index, |ui| {
                                            ui.add_space(Tokens::get().space_10);
                                            report_header(ui, "Internal error in a command", 3, "2026-10-18 21:04:12", "Report", |ui| {
                                                ui_kit::bin_button(ui, "Delete");
                                                ui_kit::icon_label_button(ui, "Copy path", "copy", true, false);
                                                ui_kit::icon_label_button(ui, "Report…", "bug_report", true, false);
                                            });
                                            ui_kit::text(ui, "solid.fillet · replayable", ui_kit::Text::MonoNote("detail"));
                                            ui_kit::text(
                                                ui,
                                                "The radius produced a degenerate face while rebuilding Fillet2.",
                                                ui_kit::Text::Report("message", false),
                                            );
                                            ui_kit::divider(ui, "end");
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
                    let viewport = scenario::rect_of("dialog:list:reports:viewport").unwrap();
                    let content = scenario::count_of("dialog:list:reports:content_width").unwrap() as f32;
                    assert!(content <= viewport.width().ceil(), "{width} at {ppp}, {count} rows: content {content}, viewport {viewport:?}");
                }
            }
        }
    }
}
