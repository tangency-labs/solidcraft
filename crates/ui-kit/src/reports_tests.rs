use super::*;
use crate::test_support as harness;
use solidcraft_ui_registry as registry;

#[test]
fn fitting_crash_rows_stay_inside_list_and_enclosing_horizontal_viewport() {
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
                                report_list(ui, "reports", 160.0, |ui| {
                                    for index in 0..count {
                                        ui.push_id(index, |ui| {
                                            ui.add_space(Tokens::get().space_10);
                                            report_header(ui, "Internal error in a command", 3, "2026-10-18 21:04:12", "Report", |ui| {
                                                crate::bin_button(ui, "Delete");
                                                crate::icon_label_button(ui, "Copy path", "copy", true, false);
                                                crate::icon_label_button(ui, "Report…", "bug_report", true, false);
                                            });
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
                    let viewport = registry::rect_of("dialog:list:reports:viewport").unwrap();
                    let content = registry::count_of("dialog:list:reports:content_width").unwrap() as f32;
                    assert!(content <= viewport.width().ceil(), "{width} at {ppp}, {count} rows: content {content}, viewport {viewport:?}");
                }
            }
        }
    }
}
