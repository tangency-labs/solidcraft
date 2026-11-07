//! Modelling-tool preferences composed from shared controls and canonical figures.
use super::*;

pub(super) fn show(app: &mut SolveApp, ui: &mut egui::Ui) {
    let t = Tokens::get();
    ui_kit::settings_page(ui, |ui| {
        ui_kit::settings_heading(ui, app.tr("prefs.structure.tools"), app.tr("prefs.structure.tools_note"));
        ui_kit::settings_section(ui, "create", app.tr("ui.create"), |ui| {
            create_card(ui, |ui| {
                create_row(ui, app.tr("ui.hole"), &["hole"], true, |ui| {
                    for (setting, help, key, legacy) in [
                        (
                            setting::hole_snaps as fn(&mut SolveApp, &mut egui::Ui) -> egui::Response,
                            "prefs.design_tools.hole_snaps_help",
                            "hole_snaps",
                            "Snap to face and edge centres",
                        ),
                        (setting::hole_moves, "prefs.design_tools.hole_moves_help", "hole_moves", "Move holes by clicking or dragging"),
                    ] {
                        ui.horizontal_wrapped(|ui| {
                            let response = setting(app, ui).backing_on_hover_text(app.tr(help));
                            crate::scenario::publish_target(&format!("prefs:{key}"), response.rect);
                            crate::scenario::publish_target(&format!("dialog:{legacy}"), response.rect);
                            if !crate::prefs_search::suppress_chrome(ui) {
                                ui_kit::help_icon(ui, app.tr(help));
                            }
                        });
                    }
                });
                create_row(ui, app.tr_label("Extrude"), &["extrude"], false, |ui| {
                    for (setting, help, key) in [
                        (
                            setting::extrude_auto_to_object as fn(&mut SolveApp, &mut egui::Ui) -> egui::Response,
                            "ui.extrude_auto_to_object_tip",
                            "extrude_auto_to_object",
                        ),
                        (setting::auto_operation, "prefs.auto_operation_note", "auto_operation"),
                        (setting::extrude_arrow_face_stops, "prefs.extrude_arrow_face_stops_note", "extrude_arrow_face_stops"),
                    ] {
                        ui.horizontal_wrapped(|ui| {
                            let response = setting(app, ui).backing_on_hover_text(app.tr(help));
                            crate::scenario::publish_target(&format!("prefs:{key}"), response.rect);
                            if !crate::prefs_search::suppress_chrome(ui) {
                                ui_kit::help_icon(ui, app.tr(help));
                            }
                        });
                    }
                    // Retain the newer controls and their original dependency after automatic operation.
                    let response = setting::extrude_view_isometric(app, ui);
                    crate::scenario::publish_target("prefs:extrude_view_isometric", response.rect);
                    let isometric = app.preferences.extrude_view_isometric;
                    ui.add_enabled_ui(isometric, |ui| {
                        let response = setting::extrude_view_return(app, ui);
                        crate::scenario::publish_target("prefs:extrude_view_return", response.rect);
                    });
                });
                create_row(ui, app.tr("prefs.design_tools.placement_group"), &["rest", "boss", "snap_fit"], false, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        let response = setting::placement_handles(app, ui).backing_on_hover_text(app.tr("ui.placement_handles_tip"));
                        crate::scenario::publish_target("prefs:placement_handles", response.rect);
                        if !crate::prefs_search::suppress_chrome(ui) {
                            ui_kit::help_icon(ui, app.tr("ui.placement_handles_tip"));
                        }
                    });
                });
            });
        });
        ui_kit::settings_section(ui, "pattern_editing", app.tr("prefs.pattern_editing"), |ui| {
            ui.horizontal_wrapped(|ui| {
                let response = setting::linear_pattern_editor(app, ui).backing_on_hover_text(app.tr("prefs.linear_pattern_editor_note"));
                crate::scenario::publish_target("prefs:linear_pattern_editor", response.rect);
                if !crate::prefs_search::suppress_chrome(ui) {
                    ui_kit::help_icon(ui, app.tr("prefs.linear_pattern_editor_note"));
                }
            });
        });
        ui_kit::settings_section(ui, "command_dialog_values", app.tr("prefs.command_dialog_values"), |ui| {
            setting::remember_dialog_values(app, ui);
        });
        ui_kit::settings_section(ui, "joint_move", app.tr("joint_move.settings"), |ui| {
            tool_group(ui, "", ui_kit::paint_assembly_move, |ui| {
                ui.horizontal_wrapped(|ui| {
                    let response = setting::assembly_move_handles(app, ui).backing_on_hover_text(app.tr("joint_move.legacy_hint"));
                    crate::scenario::publish_target("prefs:assembly_move_handles", response.rect);
                    crate::scenario::publish_target("dialog:Use joint motion handles for assembly Move", response.rect);
                    if !crate::prefs_search::suppress_chrome(ui) {
                        ui_kit::help_icon(ui, app.tr("joint_move.legacy_hint"));
                    }
                });
                let response = setting::drive_joints_ghost(app, ui);
                crate::scenario::publish_target("prefs:drive_joints_ghost", response.rect);
                ui.horizontal_wrapped(|ui| {
                    let tip = app.tr("prefs.move_drag_highlights_tip");
                    let response = setting::move_drag_highlights(app, ui).backing_on_hover_text(tip);
                    crate::scenario::publish_target("prefs:move_drag_highlights", response.rect);
                    if !crate::prefs_search::suppress_chrome(ui) {
                        ui_kit::help_icon(ui, tip);
                    }
                });
                let label_width =
                    ["joint_move.coarse", "joint_move.fine"].into_iter().map(|id| ui_kit::ContentWidth::label(ui, app.tr(id))).fold(0.0, f32::max)
                        + t.space_16
                        + t.space_4;
                ui_kit::settings_form(ui, label_width, |ui| {
                    ui_kit::settings_row_help(ui, app.tr("joint_move.coarse"), app.tr("joint_move.precision"), |ui| {
                        let response = ui_kit::named_control(ui, app.tr("prefs.design_tools.degrees"), |ui| {
                            ui_kit::settings_number(ui, &mut app.preferences.joint_rotation_coarse, 0.001..=180.0, 0.1)
                        });
                        crate::scenario::publish_target("prefs:joint_rotation_coarse", response.rect);
                        crate::scenario::publish_target("dialog:Near-axis rotation step (°)", response.rect);
                        ui_kit::text(ui, "deg", ui_kit::Text::Inline);
                    });
                    ui_kit::row(ui, app.tr("joint_move.fine"), |ui| {
                        let response = ui_kit::named_control(ui, app.tr("prefs.design_tools.degrees"), |ui| {
                            ui_kit::settings_number(ui, &mut app.preferences.joint_rotation_fine, 0.001..=180.0, 0.01)
                        });
                        crate::scenario::publish_target("prefs:joint_rotation_fine", response.rect);
                        crate::scenario::publish_target("dialog:Far-axis rotation step (°)", response.rect);
                        ui_kit::text(ui, "deg", ui_kit::Text::Inline);
                    });
                });
            });
        });
        ui_kit::settings_section(ui, "measure_pointer", app.tr("prefs.structure.measure_pointer"), |ui| {
            let width = ((ui.available_width() - t.space_10 * 2.0) / 3.0).max(t.space_24 * 8.0 + t.space_16).min(ui.available_width());
            let tiles: [(_, _, _, fn(&egui::Painter, egui::Rect)); 3] = [
                (
                    "measure_axis_distances",
                    app.tr("settings.measure_axis_distances"),
                    app.preferences.measure_axis_distances,
                    ui_kit::paint_measure_axis_distances,
                ),
                ("tool_cursor", app.tr_label("Show tool at cursor"), app.ui.tool_cursor, ui_kit::paint_tool_cursor),
                ("direct_interaction", app.tr("ui.direct_interaction"), app.preferences.direct_interaction, ui_kit::paint_direct_interaction),
            ];
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(t.space_10, t.space_10);
                for (key, label, selected, figure) in tiles {
                    let response = ui_kit::toggle_tile(ui, &ui_kit::Card {
                        label, sub: None, selected, enabled: true, options: None, check: false,
                        aspect: 120.0 / 64.0, figure: &figure,
                    }, width);
                    crate::scenario::publish_target(&format!("prefs:{key}"), response.rect);
                    crate::scenario::publish_target(&format!("dialog:{label}"), response.rect);
                    match key {
                        "tool_cursor" => {
                            crate::scenario::publish_target("view:tool_cursor", response.rect);
                            crate::scenario::publish_target("dialog:Show tool at cursor", response.rect);
                        }
                        "measure_axis_distances" => crate::scenario::publish_target("dialog:Show X/Y/Z distances", response.rect),
                        _ => {}
                    }
                    if response.clicked() {
                        match key {
                            "measure_axis_distances" => app.preferences.measure_axis_distances = !selected,
                            "tool_cursor" => app.ui.tool_cursor = !selected,
                            _ => app.preferences.direct_interaction = !selected,
                        }
                    }
                    if key == "direct_interaction" {
                        response.backing_on_hover_text(app.tr_label("Work on the model directly in the viewport. Press a flat face or a profile and pull: up extrudes, down into a body cuts, and sideways movement sets the step size"));
                    }
                }
            });
        });
    });
}

/// A single original figure beside a tool's complete setting stack. The columns wrap together
/// with their translated labels; search uses the same block gate as other compound preferences.
fn tool_group(ui: &mut egui::Ui, label: &str, figure: fn(&egui::Painter, egui::Rect), add: impl FnOnce(&mut egui::Ui)) {
    let gate = crate::prefs_search::block_gate(ui, label, None);
    let before = crate::prefs_search::hits(ui);
    if gate.as_ref().is_some_and(|gate| !gate.show) {
        crate::prefs_search::run_hidden(ui, label, add);
        if let Some(gate) = &gate {
            crate::prefs_search::block_finish(ui, gate, false, before);
        }
        return;
    }
    let t = Tokens::get();
    let figure_width = t.space_24 * 5.0;
    let available = ui.available_width();
    let controls_width = if available >= figure_width + t.space_14 + t.space_24 * 10.0 { available - figure_width - t.space_14 } else { available };
    ui.with_layout(egui::Layout::left_to_right(egui::Align::Min).with_main_wrap(true), |ui| {
        ui.spacing_mut().item_spacing.x = t.space_14;
        ui.allocate_ui_with_layout(egui::vec2(figure_width.min(available), 0.0), egui::Layout::top_down(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.y = t.space_6;
            // The checkbox rows start with a little air; start the figure level with the first box.
            ui.add_space(t.space_6 - 1.0);
            let (rect, _) = ui.allocate_exact_size(egui::vec2(figure_width.min(available), t.space_24 * 2.0 + t.space_16), egui::Sense::hover());
            figure(&ui.painter_at(rect), rect);
            if !label.is_empty() {
                ui_kit::text(ui, label, ui_kit::Text::Inline);
            }
        });
        ui.allocate_ui_with_layout(egui::vec2(controls_width, 0.0), egui::Layout::top_down(egui::Align::Min), add);
    });
    if let Some(gate) = &gate {
        crate::prefs_search::block_finish(ui, gate, true, before);
    }
}

/// The Create section's card: one border around a row per tool.
fn create_card(ui: &mut egui::Ui, rows: impl FnOnce(&mut egui::Ui)) {
    let t = Tokens::get();
    if crate::prefs_search::suppress_chrome(ui) {
        rows(ui);
        return;
    }
    ui_kit::Container::show_backing(
        ui,
        egui::Frame::NONE.fill(t.control_fill).stroke(egui::Stroke::new(1.0, t.dialog_border)).corner_radius(f32::from(t.radius_lg) + t.space_2),
        |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            ui.set_width(ui.available_width());
            rows(ui);
        },
    );
}

/// One tool in the Create card: its icons in tiles and its name on the left, its settings on the
/// right, a rule above every row but the first. Search uses the same block gate as `tool_group`.
fn create_row(ui: &mut egui::Ui, label: &str, icons: &[&str], first: bool, add: impl FnOnce(&mut egui::Ui)) {
    let gate = crate::prefs_search::block_gate(ui, label, None);
    let before = crate::prefs_search::hits(ui);
    if gate.as_ref().is_some_and(|gate| !gate.show) {
        crate::prefs_search::run_hidden(ui, label, add);
        if let Some(gate) = &gate {
            crate::prefs_search::block_finish(ui, gate, false, before);
        }
        return;
    }
    let t = Tokens::get();
    let chrome = !crate::prefs_search::suppress_chrome(ui);
    let margin = egui::Margin::symmetric(t.space_14 as i8, t.space_10 as i8);
    let row = egui::Frame::NONE.inner_margin(margin).show(ui, |ui| {
        ui.set_width(ui.available_width());
        let available = ui.available_width();
        let left = (available * 0.45).clamp(t.space_24 * 5.0, t.space_24 * 12.0 + t.space_4);
        let right = if available >= left + t.space_16 + t.space_24 * 8.0 { available - left - t.space_16 } else { available };
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = t.space_16;
            ui.allocate_ui_with_layout(
                egui::vec2(left.min(available), t.space_24 + t.space_12),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    ui.set_min_width(left.min(available));
                    ui.spacing_mut().item_spacing.x = t.space_6;
                    let tile = t.space_24 + t.space_12;
                    for icon in icons {
                        let (rect, _) = ui.allocate_exact_size(egui::vec2(tile, tile), egui::Sense::hover());
                        ui.painter().rect_filled(rect, t.radius_lg, t.pref_well);
                        crate::icons::paint_icon(ui.painter(), rect.shrink(t.space_4), icon, 1.0);
                    }
                    ui.add_space(t.space_4);
                    if !label.is_empty() {
                        ui.label(egui::RichText::new(label).font(crate::fonts::semibold(t.font_label)).color(t.form_ink));
                    }
                },
            );
            ui.allocate_ui_with_layout(egui::vec2(right, 0.0), egui::Layout::top_down(egui::Align::Min), add);
        });
    });
    if chrome && !first {
        // Not the first row: draw the rule at the row's top edge.
        ui.painter().hline(row.response.rect.x_range(), row.response.rect.top(), egui::Stroke::new(1.0, t.dialog_rule));
    }
    if let Some(gate) = &gate {
        crate::prefs_search::block_finish(ui, gate, true, before);
    }
}
