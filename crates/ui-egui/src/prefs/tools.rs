//! Existing modelling-tool controls, regrouped without changing their semantics.
use super::*;

pub(super) fn show(app: &mut SolveApp, ui: &mut egui::Ui) {
    let t = Tokens::get();
    ui_kit::settings_heading(ui, app.tr("prefs.structure.tools"), app.tr("prefs.structure.tools_note"));
    // The Hole command's own settings, under its name.
    ui_kit::settings_section(ui, "create_hole", crate::i18n::tr_label("Create Hole"), |ui| {
        setting::hole_snaps(app, ui);
        ui_kit::text(
            ui,
            crate::i18n::tr_label("A Single hole snaps to the middle of its face and to the centres of its round edges"),
            ui_kit::Text::Hint("hole_snaps"),
        );
        setting::hole_moves(app, ui);
        ui_kit::text(
            ui,
            crate::i18n::tr_label(
                "A placed Single hole has a handle to drag it, and picking its face again moves the hole there instead of taking it out",
            ),
            ui_kit::Text::Hint("hole_moves"),
        );
    });
    ui_kit::settings_section(ui, "extrude", app.tr_label("Extrude"), |ui| {
        setting::extrude_auto_to_object(app, ui);
        // Extrude's view (#9176): isometric when it opens, and back afterwards only with that.
        setting::extrude_view_isometric(app, ui);
        let isometric = app.preferences.extrude_view_isometric;
        ui.add_enabled_ui(isometric, |ui| setting::extrude_view_return(app, ui));
        setting::extrude_default_distance(app, ui);
        let auto = setting::auto_operation(app, ui);
        crate::scenario::publish_handle("prefs:auto_operation", auto.rect.center());
        ui_kit::text(ui, app.tr("prefs.auto_operation_note"), ui_kit::Text::Hint("auto_operation"));
    });
    ui_kit::settings_section(ui, "placement", app.tr("prefs.placement"), |ui| {
        setting::placement_handles(app, ui);
    });
    ui_kit::settings_section(ui, "joint_move", app.tr("joint_move.settings"), |ui| {
        setting::assembly_move_handles(app, ui);
        ui_kit::text(ui, app.tr("joint_move.legacy_hint"), ui_kit::Text::Hint("joint_move_legacy"));
        ui_kit::settings_form(ui, t.space_24 * 7.0 + t.space_2, |ui| {
            ui_kit::row(ui, app.tr("joint_move.coarse"), |ui| {
                ui_kit::settings_number(ui, &mut app.preferences.joint_rotation_coarse, 0.001..=180.0, 0.1);
            });
            ui_kit::row(ui, app.tr("joint_move.fine"), |ui| {
                ui_kit::settings_number(ui, &mut app.preferences.joint_rotation_fine, 0.001..=180.0, 0.01);
            });
        });
        ui_kit::text(ui, app.tr("joint_move.precision"), ui_kit::Text::Hint("joint_move"));
    });
    ui_kit::settings_section(ui, "measure_pointer", app.tr("prefs.structure.measure_pointer"), |ui| {
        let response = setting::measure_axis_distances(app, ui);
        crate::scenario::publish_rect("prefs:measure_axis_distances", response.rect);
        response.on_hover_text(app.tr("settings.measure_axis_distances_help"));
        let cursor = ui_kit::checkbox(ui, crate::i18n::tr_label("Show tool at cursor"), &mut app.ui.tool_cursor);
        crate::scenario::publish_target("view:tool_cursor", cursor.rect);
        setting::direct_interaction(app, ui);
    });
}
