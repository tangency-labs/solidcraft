//! The scene's wear stack. Edits retain the existing render draft and save path.
use super::{
    ImageMethod,
    effects::{failure, layer_actions, note, number},
    materials, studio,
};
use crate::{SolveApp, theme::Tokens, ui_kit};
use solidcraft_engine::render::job::aging::{AgingKind, AgingLayer, SurfaceClass};

pub(super) fn controls(app: &mut SolveApp, ui: &mut egui::Ui) {
    note(app, ui, "render.effects_gpu_only");
    ui.add_space(Tokens::get().space_4);
    let enabled = app.render_workspace.view.method == ImageMethod::GpuRaytrace;
    let response = ui.add_enabled_ui(enabled, |ui| layer_controls(app, ui));
    crate::scenario::publish_rect("render:wear:controls", response.response.rect);
}

fn layer_controls(app: &mut SolveApp, ui: &mut egui::Ui) {
    note(app, ui, "render.aging_help");
    let mut editor = std::mem::take(&mut app.render_workspace.view.inspector.effects);
    let before = app.render_workspace.view.studio.aging.clone();
    let mut stack = before.clone();
    let title = app.tr("render.wear_layers").to_string();
    ui_kit::section(ui, "render_wear_layers", &title, true, |ui| {
        let add = ui_kit::dropdown_menu(ui, "render_aging_add", app.tr("render.aging_add"), |ui| {
            for kind in AgingKind::ALL {
                let id = format!("aging-add-{}", kind.code());
                if ui_kit::list_picker(ui, &id, app.tr(kind.key()), false).clicked() {
                    match stack.push(AgingLayer::preset(kind)) {
                        Ok(()) => editor.selected_aging = stack.len() - 1,
                        Err(error) => app.set_status(error.to_string(), true),
                    }
                    ui.close();
                }
            }
        });
        crate::scenario::publish_target("render:aging-add", add.rect);
        if stack.is_empty() {
            note(app, ui, "render.aging_empty");
        }
        editor.selected_aging = editor.selected_aging.min(stack.len().saturating_sub(1));
        for index in 0..stack.len() {
            let Some(layer) = stack.get_mut(index) else { continue };
            let label = format!("{} · {}", index + 1, app.tr(layer.kind.key()));
            let eye_target = format!("render:aging-layer:{index}:enabled");
            let actions = [ui_kit::TreeAction {
                icon: if layer.enabled { "eye" } else { "suppress" },
                hover_only: false,
                label: app.tr("render.layer_enabled"),
                reason: None,
                target: &eye_target,
            }];
            let row = ui_kit::tree_row(
                ui,
                ui.scope_id().with(("aging-layer", index)),
                &ui_kit::Row { label: &label, name: true, selected: index == editor.selected_aging, actions: &actions, ..Default::default() },
            );
            if let Some(rect) = row.rect {
                crate::scenario::publish_target(&format!("render:aging-layer:{index}"), rect);
            }
            if row.clicked {
                editor.selected_aging = index;
            }
            if row.actions.iter().any(|response| response.clicked()) {
                layer.enabled = !layer.enabled;
            }
        }
        if !stack.is_empty() {
            let (movement, remove) = layer_actions(app, ui, "aging", editor.selected_aging, stack.len());
            if remove {
                failure(app, stack.remove(editor.selected_aging).map_err(|e| e.to_string()));
                editor.selected_aging = editor.selected_aging.min(stack.len().saturating_sub(1));
            } else if movement != 0 {
                let next = if movement < 0 { editor.selected_aging.saturating_sub(1) } else { editor.selected_aging + 1 };
                failure(app, stack.move_layer(editor.selected_aging, next).map_err(|e| e.to_string()));
                editor.selected_aging = next;
            }
        }
    });
    if let Some(layer) = stack.get_mut(editor.selected_aging) {
        let title = app.tr("render.wear_selected_layer").to_string();
        ui_kit::section(ui, "render_wear_selected_layer", &title, true, |ui| {
            let response = ui_kit::checkbox(ui, app.tr("render.layer_enabled"), &mut layer.enabled);
            crate::scenario::publish_target("render:aging-enabled", response.rect);
            number(app, ui, "render.layer_strength", &mut layer.strength, 0.0..=1.0, 0.01, "");
            number(app, ui, "render.aging_coverage", &mut layer.coverage, 0.0..=1.0, 0.01, "");
            number(app, ui, "render.aging_scale", &mut layer.scale_mm, 0.01..=100000.0, 0.1, " mm");
            let response = ui_kit::row(ui, app.tr("render.effects_seed"), |ui| ui_kit::number_field(ui, &mut layer.seed, 0..=65535, 1.0, ""));
            crate::scenario::publish_target("render:aging-seed", response.rect);
            let response = ui_kit::row(ui, app.tr("render.aging_color"), |ui| ui_kit::color_field(ui, &mut layer.color_srgb));
            crate::scenario::publish_target("render:aging-color", response.rect);
            number(app, ui, "render.aging_roughness", &mut layer.roughness, 0.02..=1.0, 0.01, "");
            number(app, ui, "render.aging_metallic", &mut layer.metallic, 0.0..=1.0, 0.01, "");
            number(app, ui, "render.aging_upward", &mut layer.upward_bias, 0.0..=1.0, 0.01, "");
            let current = match layer.surface_class {
                SurfaceClass::AnyOpaque => "render.aging_class_any",
                SurfaceClass::Metal => "render.aging_class_metal",
                SurfaceClass::Dielectric => "render.aging_class_dielectric",
            };
            let class = ui_kit::dropdown_menu(ui, "render_aging_class", app.tr(current), |ui| {
                for (value, key) in [
                    (SurfaceClass::AnyOpaque, "render.aging_class_any"),
                    (SurfaceClass::Metal, "render.aging_class_metal"),
                    (SurfaceClass::Dielectric, "render.aging_class_dielectric"),
                ] {
                    if ui_kit::list_picker(ui, key, app.tr(key), layer.surface_class == value).clicked() {
                        layer.surface_class = value;
                        ui.close();
                    }
                }
            });
            crate::scenario::publish_target("render:aging-class", class.rect);
        });
        let title = app.tr("render.aging_target").to_string();
        ui_kit::section(ui, "render_aging_target_section", &title, true, |ui| {
            if ui_kit::button(ui, app.tr("render.aging_refresh_targets"), ui_kit::Button::Normal).clicked() {
                match materials::read_targets(app) {
                    Ok(targets) => editor.targets = targets,
                    Err(error) => app.set_status(error, true),
                }
            }
            let search_label = app.tr("render.aging_target_search").to_string();
            ui_kit::bounded_cell_editor(ui, ui.scope_id().with("aging_target_search"), &mut editor.search, &search_label, false, 128);
            let selected = layer.target.as_ref().map_or_else(
                || app.tr("render.aging_all_bodies").to_string(),
                |identity| {
                    editor.targets.iter().find(|target| target.stable_target.as_ref() == Some(identity)).map_or_else(
                        || format!("{}: {}", identity.definition, app.tr("render.material_target_missing")),
                        |target| target.name.clone(),
                    )
                },
            );
            ui_kit::row(ui, app.tr("render.wear_applies_to"), |ui| {
                let target = ui_kit::dropdown_menu(ui, "render_aging_target", &selected, |ui| {
                    if ui_kit::list_picker(ui, "aging-target-all", app.tr("render.aging_all_bodies"), layer.target.is_none()).clicked() {
                        layer.target = None;
                        ui.close();
                    }
                    let search = editor.search.to_lowercase();
                    for target in
                        editor.targets.iter().filter(|t| !t.name.starts_with("__render_stage__/") && t.name.to_lowercase().contains(&search)).take(64)
                    {
                        if ui_kit::list_picker(
                            ui,
                            &format!("aging-target-{}", target.ordinal),
                            &target.name,
                            layer.target.is_some() && layer.target == target.stable_target,
                        )
                        .clicked()
                        {
                            if let Some(identity) = &target.stable_target {
                                layer.target = Some(identity.clone());
                            } else {
                                app.set_status(app.tr("render.material_target_missing").to_string(), true);
                            }
                            ui.close();
                        }
                    }
                });
                crate::scenario::publish_target("render:aging-target", target.rect);
            });
            studio::vector(ui, app.tr("render.aging_origin"), &mut layer.origin_mm, -1e9..=1e9, 1.0);
            note(app, ui, "render.aging_target_help");
        });
    }
    if app.render_workspace.view.method == ImageMethod::GpuRaytrace && before != stack {
        app.render_workspace.view.studio.aging = stack;
        app.render_workspace.view.dirty = true;
    }
    app.render_workspace.view.inspector.effects = editor;
}
