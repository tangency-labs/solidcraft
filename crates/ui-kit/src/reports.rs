//! Read-only report and library containers, sharing the form's typography and actions.
use crate::BackingTooltipExt as _;
use egui::{Ui, vec2};
use solidcraft_theme::Tokens;

/// A shared table in a bounded vertical viewport. Narrative details may follow each row.
/// Retains the report list's geometry aliases for automation.
pub fn report_list<'a, R>(
    ui: &mut Ui,
    key: &str,
    height: f32,
    columns: &'a [crate::Column<'a>],
    empty: &str,
    add: impl FnOnce(&mut Ui, &mut crate::Table<'a>) -> R,
) -> R {
    let output = egui::ScrollArea::vertical()
        .id_salt(("report_list", key))
        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::VisibleWhenNeeded)
        .content_margin(egui::Margin::ZERO)
        .auto_shrink([false, true])
        .max_height(height)
        .show(ui, |ui| {
            crate::table_custom(ui, key, columns, empty, |ui, table| {
                // egui rounds the content rect separately from the actual scroll viewport.
                ui.set_max_width(ui.available_width().min(ui.clip_rect().width()));
                table.headers(ui, |_, _| false);
                add(ui, table)
            })
        });
    solidcraft_ui_registry::publish_rect(&format!("dialog:list:{key}"), output.inner_rect);
    solidcraft_ui_registry::publish_rect(&format!("dialog:list:{key}:viewport"), output.inner_rect);
    solidcraft_ui_registry::publish_count(&format!("dialog:list:{key}:content_width"), f64::from(output.content_size.x));
    output.inner
}

/// Shared divider between wrapping report bodies, retaining the report's geometry alias.
pub fn report_divider(ui: &mut Ui, key: &str) {
    crate::divider(ui, key);
}

/// List border around a caller-owned virtualised viewport. Tokens: dialog_rule, control_radius, space_10.
pub fn list_frame<R>(ui: &mut Ui, key: &str, add: impl FnOnce(&mut Ui) -> R) -> R {
    list_frame_filled(ui, key, egui::Color32::TRANSPARENT, 0.0, add)
}

pub(super) fn list_frame_filled<R>(ui: &mut Ui, key: &str, fill: egui::Color32, vertical: f32, add: impl FnOnce(&mut Ui) -> R) -> R {
    let t = Tokens::get();
    let response = crate::Container::show_backing(
        ui,
        egui::Frame::NONE
            .fill(fill)
            .stroke(t.container_border(t.dialog_rule))
            .corner_radius(t.control_radius)
            .inner_margin(egui::Margin::symmetric(t.space_10 as i8, vertical as i8)),
        |ui| {
            // Frame already subtracts its border and padding from the content width.
            ui.set_min_width(ui.available_width().max(0.0));
            add(ui)
        },
    );
    solidcraft_ui_registry::publish_rect(&format!("dialog:list:{key}"), response.response.rect);
    response.inner
}

#[cfg(test)]
#[path = "reports_tests.rs"]
mod tests;

/// Read-only monospace path chip with an adjacent action. Retains the path cell handle.
pub fn path_chip(ui: &mut Ui, path: &str, add: impl FnOnce(&mut Ui)) {
    let t = Tokens::get();
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = t.space_8;
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            add(ui);
            let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width().max(0.0), t.control_height), egui::Sense::hover());
            ui.painter().rect(rect, t.control_radius, t.dialog_footer, t.tertiary_border(t.dialog_rule), egui::StrokeKind::Inside);
            super::text::paint_text(ui, rect.shrink2(vec2(t.space_8, 0.0)), path, egui::FontId::monospace(t.font_small), t.form_dim, false);
            response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, ui.is_enabled(), path));
            response.backing_on_hover_text(path);
            let key = format!("dialog:cell:{path}");
            solidcraft_ui_registry::publish_handle(&key, rect.center());
            solidcraft_ui_registry::publish_rect(&key, rect);
        });
    });
}

/// Empty-list dashed outline and centred caption. Retains the note rectangle.
pub fn empty_box(ui: &mut Ui, key: &str, message: &str) {
    if super::settings_search::querying(ui) {
        if super::settings_search::texts_hit(ui, &[message]) {
            super::settings_search::reveal(ui);
        } else if !ui.is_visible() || super::settings_search::suppress_chrome(ui) {
            return;
        }
    }
    let t = Tokens::get();
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width().max(0.0), t.control_height * 3.0), egui::Sense::hover());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, ui.is_enabled(), message));
    let mut points = super::preferences::rounded_perimeter(rect.shrink(0.5), f32::from(t.radius_lg));
    if let Some(first) = points.first().copied() {
        points.push(first);
    }
    ui.painter().extend(egui::Shape::dashed_line(&points, t.control_border_stroke(t.control_border), t.space_4, t.space_3));
    ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, message, egui::FontId::proportional(t.font_label), t.form_dim);
    solidcraft_ui_registry::publish_rect(&format!("dialog:note:{key}"), rect);
}

/// A report summary and its immediate actions on the shared table row.
/// The combined title remains a cell alias; narrative details can follow outside the row.
pub fn report_header(ui: &mut Ui, table: &mut crate::Table<'_>, key: &str, title: &str, actions: impl FnOnce(&mut Ui)) {
    table.row(ui, key, false, |row| {
        row.cell(|ui| crate::text(ui, title, crate::Text::Cell));
        row.cell(|ui| ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), actions));
    });
}

/// Compact diagnostic summary; its whole leading region is one disclosure target.
pub struct ReportCardSummary<'a> {
    pub title: &'a str,
    pub subtitle: &'a str,
    pub icon: &'a str,
    pub ink: egui::Color32,
}

/// Individually selectable labelled values and the original diagnostic text.
pub struct ReportCardDetails<'a> {
    pub fields: &'a [(&'a str, &'a str, &'a str)],
    pub code: &'a str,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReportCardAction {
    Report,
    Delete,
    CopyPath,
}

/// A raised diagnostic card. The caller owns disclosure state and handles model actions.
/// Action responses are also supplied when idle so callers can retain automation aliases.
/// The host supplies the path action's tooltip, keeping its message catalogue out of the kit.
pub fn report_card(
    ui: &mut Ui,
    key: &str,
    summary: ReportCardSummary<'_>,
    details: ReportCardDetails<'_>,
    expanded: &mut bool,
    copy_path_label: &str,
    mut actions: impl FnMut(ReportCardAction, &egui::Response),
) -> egui::Response {
    let t = Tokens::get();
    let card = crate::Container::show_backing(
        ui,
        egui::Frame::NONE
            .fill(t.table_fill())
            .stroke(t.container_border(if *expanded { t.form_primary } else { t.border }))
            .corner_radius(t.radius_lg)
            .inner_margin(egui::Margin::same(t.space_4 as i8)),
        |ui| {
            ui.set_min_width(ui.available_width().max(0.0));
            ui.spacing_mut().item_spacing = vec2(t.space_4, t.space_8);
            ui.push_id(key, |ui| {
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let (rect, _) = ui.allocate_exact_size(vec2(t.control_height, t.control_height), egui::Sense::hover());
                        let delete = crate::button_at(ui, "delete", rect, "", crate::Button::Normal);
                        crate::icons::paint_ink(ui.painter(), rect.shrink(t.form_icon_inset), "delete", t.form_error);
                        let label = solidcraft_ui_text::tr("ui.delete");
                        delete.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, ui.is_enabled(), label));
                        actions(ReportCardAction::Delete, &delete);
                        solidcraft_ui_registry::publish_target(&format!("dialog:card:{key}:delete"), delete.rect);
                        delete.backing_on_hover_text(label);
                        let report = crate::icon_label_button(ui, solidcraft_ui_text::tr("ui.crash_card.report"), "bug_report", true, false);
                        actions(ReportCardAction::Report, &report);
                        solidcraft_ui_registry::publish_target(&format!("dialog:card:{key}:report"), report.rect);
                        let (rect, response) =
                            ui.allocate_exact_size(vec2(ui.available_width().max(0.0), t.control_height + t.space_4), egui::Sense::click());
                        if response.clicked() {
                            *expanded = !*expanded;
                            response.request_focus();
                        }
                        if *expanded || response.hovered() {
                            ui.painter().rect_filled(rect, t.control_radius, if *expanded { t.form_primary.gamma_multiply(0.10) } else { t.hover });
                        }
                        let chevron = egui::Rect::from_min_size(rect.min, vec2(t.space_16, rect.height()));
                        crate::disclosure(ui.painter(), chevron.center(), *expanded, t.form_dim);
                        let tile = egui::Rect::from_center_size(
                            egui::pos2(chevron.right() + t.space_8 + t.control_height / 2.0, rect.center().y),
                            vec2(t.control_height, t.control_height),
                        );
                        ui.painter().rect_filled(tile, t.control_radius, t.table_fill().lerp_to_gamma(summary.ink, 0.14));
                        if summary.icon == "warning" {
                            // The warning master has a filled face: a flat mask loses its exclamation mark.
                            crate::icons::paint_danger(ui.painter(), tile.shrink(t.space_6), summary.icon, 1.0);
                        } else {
                            crate::icons::paint_ink(ui.painter(), tile.shrink(t.space_6), summary.icon, summary.ink);
                        }
                        let text = egui::Rect::from_min_max(egui::pos2((tile.right() + t.space_8).min(rect.right()), rect.top()), rect.max);
                        let title = egui::Rect::from_min_max(text.min, egui::pos2(text.right(), text.center().y));
                        let subtitle = egui::Rect::from_min_max(egui::pos2(text.left(), text.center().y), text.max);
                        super::text::paint_text(ui, title, summary.title, crate::type_face::semibold(t.font_label), t.form_ink, false);
                        super::text::paint_text(ui, subtitle, summary.subtitle, egui::FontId::proportional(t.font_small), t.form_dim, false);
                        response
                            .widget_info(|| egui::WidgetInfo::selected(egui::Role::DisclosureTriangle, ui.is_enabled(), *expanded, summary.title));
                        ui.ctx().accesskit_node_builder(response.id, |node| {
                            node.set_expanded(*expanded);
                            node.set_description(summary.subtitle);
                        });
                        ui.interact(subtitle, ui.make_persistent_id("subtitle"), egui::Sense::hover())
                            .widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, ui.is_enabled(), summary.subtitle));
                        for (part, rect) in
                            [("toggle", rect), ("chevron", chevron), ("tile", tile), ("title", title), ("subtitle", subtitle), ("meta", subtitle)]
                        {
                            solidcraft_ui_registry::publish_target(&format!("dialog:card:{key}:{part}"), rect);
                        }
                        response
                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                            .backing_on_hover_text(format!("{}\n{}", summary.title, summary.subtitle));
                    });
                });
                if *expanded {
                    crate::divider(ui, "details");
                    let fields = details.fields.iter().filter(|(_, _, value)| !value.is_empty());
                    let columns = if ui.available_width() >= (t.form_label_width * 2.0 + t.space_8) * 3.0 + t.space_14 * 2.0 { 3 } else { 1 };
                    let width = ((ui.available_width() - t.space_14 * (columns - 1) as f32) / columns as f32).max(0.0);
                    egui::Grid::new("fields").num_columns(columns).spacing(vec2(t.space_14, t.space_6)).show(ui, |ui| {
                        for (index, (field, label, value)) in fields.enumerate() {
                            ui.vertical(|ui| {
                                ui.set_width(width);
                                ui.add(egui::Label::new(egui::RichText::new(*label).size(t.font_small).color(t.form_dim)).truncate());
                                let font =
                                    if *field == "system" { egui::FontId::proportional(t.font_small) } else { egui::FontId::monospace(t.font_small) };
                                let response =
                                    ui.add(egui::Label::new(egui::RichText::new(*value).font(font).color(t.form_ink)).selectable(true).truncate());
                                solidcraft_ui_registry::publish_target(&format!("dialog:card:{key}:{field}"), response.rect);
                                response.backing_on_hover_text(*value);
                            });
                            if (index + 1) % columns == 0 {
                                ui.end_row();
                            }
                        }
                    });
                    crate::Container::show_backing(
                        ui,
                        egui::Frame::NONE
                            .fill(t.dialog_footer)
                            .stroke(t.container_border(t.dialog_rule))
                            .corner_radius(t.control_radius)
                            .inner_margin(egui::Margin::same(t.space_6 as i8)),
                        |ui| {
                            ui.horizontal(|ui| {
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    let copy_path = crate::StageBar::icon_action(ui, "copy", copy_path_label);
                                    actions(ReportCardAction::CopyPath, &copy_path);
                                    solidcraft_ui_registry::publish_target(&format!("dialog:card:{key}:copy"), copy_path.rect);
                                    let copy = crate::StageBar::icon_action(ui, "copy", solidcraft_ui_text::tr("ui.crash_card.copy_details"));
                                    solidcraft_ui_registry::publish_target(&format!("dialog:card:{key}:copy_details"), copy.rect);
                                    if copy.clicked() {
                                        ui.ctx().copy_text(details.code.to_owned());
                                    }
                                    let code = ui.vertical(|ui| {
                                        ui.set_width(ui.available_width().max(0.0));
                                        for line in details.code.lines().take(3) {
                                            ui.add(
                                                egui::Label::new(egui::RichText::new(line).monospace().size(t.font_small).color(t.form_ink))
                                                    .selectable(true)
                                                    .truncate(),
                                            )
                                            .backing_on_hover_text(details.code);
                                        }
                                    });
                                    for part in ["code", "preview", "location"] {
                                        solidcraft_ui_registry::publish_target(&format!("dialog:card:{key}:{part}"), code.response.rect);
                                    }
                                });
                            });
                        },
                    );
                }
            });
        },
    );
    solidcraft_ui_registry::publish_rect(&format!("dialog:card:{key}"), card.response.rect);
    card.response
}
