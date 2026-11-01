//! Modelling-tool preferences composed from shared controls and canonical figures.
use super::*;

pub(super) fn show(app: &mut SolveApp, ui: &mut egui::Ui) {
    let t = Tokens::get();
    ui_kit::settings_page(ui, |ui| {
        ui_kit::settings_heading(ui, app.tr("prefs.structure.tools"), app.tr("prefs.structure.tools_note"));
        ui_kit::settings_section(ui, "create", app.tr("ui.create"), |ui| {
            tool_group(ui, app.tr("ui.hole"), ui_kit::paint_tool_hole, |ui| {
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
                        let response = setting(app, ui).on_hover_text(app.tr(help));
                        crate::scenario::publish_target(&format!("prefs:{key}"), response.rect);
                        crate::scenario::publish_target(&format!("dialog:{legacy}"), response.rect);
                        if !crate::prefs_search::suppress_chrome(ui) {
                            ui_kit::help_icon(ui, app.tr(help));
                        }
                    });
                }
            });
            tool_group(ui, app.tr_label("Extrude"), ui_kit::paint_tool_extrude, |ui| {
                for (setting, help, key) in [
                    (
                        setting::extrude_auto_to_object as fn(&mut SolveApp, &mut egui::Ui) -> egui::Response,
                        "ui.extrude_auto_to_object_tip",
                        "extrude_auto_to_object",
                    ),
                    (setting::auto_operation, "prefs.auto_operation_note", "auto_operation"),
                ] {
                    ui.horizontal_wrapped(|ui| {
                        let response = setting(app, ui).on_hover_text(app.tr(help));
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
            tool_group(ui, app.tr("prefs.design_tools.placement_group"), ui_kit::paint_tool_placement, |ui| {
                ui.horizontal_wrapped(|ui| {
                    let response = setting::placement_handles(app, ui).on_hover_text(app.tr("ui.placement_handles_tip"));
                    crate::scenario::publish_target("prefs:placement_handles", response.rect);
                    if !crate::prefs_search::suppress_chrome(ui) {
                        ui_kit::help_icon(ui, app.tr("ui.placement_handles_tip"));
                    }
                });
            });
        });
        ui_kit::settings_section(ui, "joint_move", app.tr("joint_move.settings"), |ui| {
            tool_group(ui, "", ui_kit::paint_assembly_move, |ui| {
                ui.horizontal_wrapped(|ui| {
                    let response = setting::assembly_move_handles(app, ui).on_hover_text(app.tr("joint_move.legacy_hint"));
                    crate::scenario::publish_target("prefs:assembly_move_handles", response.rect);
                    crate::scenario::publish_target("dialog:Use joint motion handles for assembly Move", response.rect);
                    if !crate::prefs_search::suppress_chrome(ui) {
                        ui_kit::help_icon(ui, app.tr("joint_move.legacy_hint"));
                    }
                });
                let response = setting::drive_joints_ghost(app, ui);
                crate::scenario::publish_target("prefs:drive_joints_ghost", response.rect);
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
                        response.on_hover_text(app.tr_label("Work on the model directly in the viewport. Press a flat face or a profile and pull: up extrudes, down into a body cuts, and sideways movement sets the step size"));
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
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = t.space_14;
        ui.allocate_ui_with_layout(egui::vec2(figure_width.min(available), 0.0), egui::Layout::top_down(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.y = t.space_6;
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
