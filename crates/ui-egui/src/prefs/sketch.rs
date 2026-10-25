//! Sketch and selection preferences with their live modifier legend.
use super::*;

pub(super) fn show(app: &mut SolveApp, ui: &mut egui::Ui) {
    let t = Tokens::get();
    ui_kit::settings_heading(ui, app.tr("prefs.sketch_selection_heading"), app.tr("prefs.sketch_selection_note"));
    ui_kit::settings_section(ui, "sketch", app.tr("prefs.sketching"), |ui| {
        let mut auto = app.session.auto_project;
        if ui_kit::checkbox(ui, crate::i18n::tr_label("Project face edges into new sketches"), &mut auto).changed() {
            let _ = app.run("sketch.auto_project", json!({"value": auto}));
        }
        ui_kit::checkbox(ui, crate::i18n::tr_label("Pressing a tool's shortcut again switches its mode"), &mut app.preferences.shortcut_cycles_modes)
            .on_hover_text(crate::i18n::tr_label(
                "Before the first point, pressing a sketch tool's shortcut again steps through its creation modes. Off: it starts the tool again.",
            ));
        ui_kit::checkbox(ui, app.tr("ui.first_dimension_scales"), &mut app.preferences.first_dimension_scales)
            .on_hover_text(app.tr("ui.first_dimension_scales_tip"));
        ui_kit::settings_block_hint(ui, app.tr("prefs.hold_stop_snapping"), app.tr("prefs.snap_drawing_note"), t.space_24 * 8.0 + t.space_8, |ui| {
            let combo = &mut app.preferences.suppress_sketch_snap;
            ui_kit::settings_control_width(ui, t.space_24 * 7.0 + t.space_12, |ui| {
                ui_kit::dropdown_menu(ui, "pref_suppress_sketch_snap", &combo.label(), |ui| {
                    for choice in ModifierCombo::SNAP_SUPPRESS_CHOICES {
                        if ui_kit::list_picker(ui, &choice.label(), &choice.label(), *combo == choice).clicked() {
                            *combo = choice;
                        }
                    }
                });
            });
        });
        ui_kit::settings_block_hint(
            ui,
            app.tr("prefs.sketch_exit_view"),
            app.tr("prefs.sketch_exit_view_note"),
            t.space_24 * 8.0 + t.space_8,
            |ui| {
                let exit = &mut app.preferences.sketch_exit_view;
                ui_kit::settings_control_width(ui, t.space_24 * 7.0 + t.space_12, |ui| {
                    ui_kit::dropdown_menu(ui, "pref_sketch_exit_view", crate::i18n::tr(exit.label_id()), |ui| {
                        for choice in SketchExitView::ALL {
                            if ui_kit::list_picker(ui, choice.label_id(), crate::i18n::tr(choice.label_id()), *exit == choice).clicked() {
                                *exit = choice;
                            }
                        }
                    });
                });
            },
        );
        ui_kit::checkbox(ui, app.tr("prefs.alt_frees_manipulators"), &mut app.preferences.manipulator_free_alt).on_hover_text(crate::i18n::tr_label(
            "On: Alt/Option or Ctrl/Cmd on extrude / gizmo arrows disables value snapping. Off: Ctrl/Cmd only.",
        ));
    });
    ui_kit::settings_section(ui, "selection", app.tr("prefs.selecting"), |ui| {
        ui_kit::checkbox(ui, crate::i18n::tr_label("Click selects whole bodies"), &mut app.ui.pick_bodies);
        ui_kit::checkbox(ui, app.tr("prefs.prefer_free_sketch_points"), &mut app.preferences.prefer_free_sketch_points);
        let click = app.tr("prefs.click");
        let shift = app.tr("prefs.shift");
        let ctrl = app.tr("prefs.ctrl_cmd");
        let one = app.tr("prefs.select_one_item");
        let toggle = app.tr("prefs.toggle_item");
        let add = app.tr("prefs.add_item");
        ui_kit::settings_bindings(
            ui,
            &[(&[click], one), (&[shift, click], if app.preferences.selection_shift_add_only { add } else { toggle }), (&[ctrl, click], toggle)],
        );
        ui_kit::checkbox(ui, app.tr("prefs.shift_add_ctrl_toggle"), &mut app.preferences.selection_shift_add_only)
            .on_hover_text(crate::i18n::tr_label("On: Shift adds items, Ctrl/Cmd toggles. Off (default): Shift and Ctrl/Cmd both toggle."));
        ui_kit::text(ui, app.tr("prefs.shift_add_note"), ui_kit::Text::Hint("selection_modifiers"));
    });
}
