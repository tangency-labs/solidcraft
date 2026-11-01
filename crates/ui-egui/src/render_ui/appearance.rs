//! Selection-following appearance editor. All mutations use the appearance registry.
use crate::{SolveApp, dialogs_appearance, ui_kit};
use serde_json::{Value, json};
use solidcraft_engine::{
    Sel,
    doc::{Document, appearance::Look, pbr_appearance::PbrParameters},
};
use std::sync::{Arc, Weak};
use ui_kit::LookStep;

#[derive(Clone, Debug, Default)]
pub(super) struct View {
    selection: Vec<Sel>,
    mode: usize,
    pick_mode: usize,
    inspected: Option<LookStep>,
    all_users: bool,
    revision: Option<u64>,
    document: Option<Weak<Document>>,
    draft: Option<Look>,
    saved: Option<Vec<Option<Look>>>,
    dirty: bool,
    replace_search: String,
    replacements: Vec<dialogs_appearance::Look>,
}

#[cfg(test)]
impl View {
    pub(super) fn draft(&self) -> Option<&Look> {
        self.draft.as_ref()
    }
}

#[derive(Clone, Debug, Default)]
pub(super) struct Targets {
    items: Vec<Sel>,
    components: Vec<u64>,
}
impl Targets {
    fn params(&self, look: &Look) -> Option<Value> {
        let mut params = dialogs_appearance::apply_params(&self.items, look.color, look.opacity, Some(&look.name), look.pbr)
            .or_else(|| (!self.components.is_empty()).then(|| look_params(look)))?;
        if !self.components.is_empty() {
            // Assignment's existing target form accepts component names or string IDs.
            params["components"] = json!(self.components.iter().map(u64::to_string).collect::<Vec<_>>());
        }
        Some(params)
    }
    fn queries(&self) -> Vec<Value> {
        self.items
            .iter()
            .filter_map(|item| match item {
                Sel::Body { name } => Some(json!({"body":name})),
                Sel::Face { body, index, .. } => Some(json!({"body":body,"face":index})),
                _ => None,
            })
            .chain(self.components.iter().map(|id| json!({"component":id})))
            .collect()
    }
}

fn mode_step(mode: usize) -> LookStep {
    match mode {
        1 => LookStep::Face,
        2 => LookStep::Component,
        _ => LookStep::Body,
    }
}
fn step_index(step: LookStep) -> usize {
    match step {
        LookStep::Face => 0,
        LookStep::Body => 1,
        LookStep::Component => 2,
        LookStep::Material => 3,
    }
}
pub(super) fn targets(app: &SolveApp, step: LookStep) -> Targets {
    let mut targets = Targets::default();
    let state = crate::open_meshing::session_world(&app.session);
    for selection in &app.session.selection {
        let body = match selection {
            Sel::Body { name } | Sel::Face { body: name, .. } => state.body(name),
            _ => None,
        };
        match step {
            LookStep::Face => {
                if matches!(selection, Sel::Face { .. }) {
                    targets.items.push(selection.clone());
                }
            }
            LookStep::Body => {
                if let Some(body) = body {
                    targets.items.push(Sel::Body { name: body.name.clone() });
                }
                if let Sel::Occurrence { path } = selection
                    && let Ok(bodies) = app.session.occurrence_bodies(path, true)
                {
                    for name in bodies {
                        targets.items.push(Sel::Body { name });
                        if targets.items.len() > 10_000 {
                            return targets;
                        }
                    }
                }
            }
            LookStep::Component => {
                let component = body.map(|body| app.session.doc.model_body_component(body)).or_else(|| {
                    let Sel::Occurrence { path } = selection else { return None };
                    app.session.occurrence_placement(path).ok().map(|placement| placement.component)
                });
                if let Some(component) = component.filter(|id| *id != 0) {
                    targets.components.push(component);
                }
            }
            LookStep::Material => {}
        }
    }
    let mut unique = std::collections::BTreeSet::new();
    targets.items.retain(|item| match item {
        Sel::Body { name } => unique.insert((name.clone(), None)),
        Sel::Face { body, index, .. } => unique.insert((body.clone(), Some(*index))),
        _ => false,
    });
    targets.components.sort_unstable();
    targets.components.dedup();
    targets
}

fn decode_look(value: &Value) -> Option<Look> {
    Some(Look {
        name: value.get("name")?.as_str()?.into(),
        color: dialogs_appearance::parse_color(value.get("color")?)?,
        opacity: value.get("opacity")?.as_f64()?,
        pbr: value.get("pbr").filter(|v| !v.is_null()).and_then(|p| serde_json::from_value(p.clone()).ok()),
    })
}
pub(super) fn chains(app: &mut SolveApp, targets: &Targets) -> Result<Vec<[Option<Look>; 4]>, String> {
    targets
        .queries()
        .into_iter()
        .map(|target| {
            let reply = app.session.execute("appearance.chain", &json!({"target":target})).map_err(|e| e.to_string())?;
            let mut chain = [None, None, None, None];
            if let Some(steps) = reply.get("steps").and_then(Value::as_array) {
                for (slot, step) in chain.iter_mut().zip(steps) {
                    *slot = step.get("look").and_then(decode_look);
                }
            }
            Ok(chain)
        })
        .collect()
}
fn look_params(look: &Look) -> Value {
    json!({"name":look.name,"color":look.color,"opacity":look.opacity,"pbr":look.pbr})
}
pub(super) fn users(app: &SolveApp, look: &Look) -> usize {
    assigned_looks(app).filter(|l| *l == look).count()
}
fn assigned_looks(app: &SolveApp) -> impl Iterator<Item = &Look> {
    let appearances = &app.session.doc.appearances;
    appearances.bodies.values().chain(appearances.components.values()).chain(appearances.faces.iter().map(|f| &f.look))
}
pub(super) fn assign(app: &mut SolveApp, targets: &Targets, from: &Look, to: &Look, all_users: bool) -> Result<(), String> {
    if all_users {
        app.run("appearance.replace", json!({"from":look_params(from),"to":look_params(to)}))?;
    } else if let Some(params) = targets.params(to) {
        app.run("appearance.assign", params)?;
    }
    Ok(())
}
pub(super) fn clear(app: &mut SolveApp, targets: &Targets, look: &Look) -> Result<(), String> {
    if let Some(params) = targets.params(look) {
        app.run("appearance.assign", dialogs_appearance::without_look(params))?;
    }
    Ok(())
}

// The same validated response used by the existing material/appearance editors.
pub(super) fn finish(look: &mut Look, index: usize) -> Result<(), String> {
    let (metallic, roughness) = match index {
        0 => (0.0, 0.85),
        1 => (0.0, 0.5),
        2 => (0.0, 0.12),
        _ => (1.0, 0.32),
    };
    look.pbr = Some(PbrParameters::new(metallic, roughness, look.pbr.unwrap_or_default().dielectric_ior()).map_err(|e| e.to_string())?);
    look.opacity = 1.0;
    Ok(())
}
fn finish_index(look: &Look) -> usize {
    let p = look.pbr.unwrap_or_default();
    if p.metallic() >= 0.5 {
        3
    } else if p.roughness() >= 0.7 {
        0
    } else if p.roughness() <= 0.2 {
        2
    } else {
        1
    }
}
fn sphere(look: &Look) -> ui_kit::Sphere {
    super::super::library::tile_sphere(&look_params(look))
}
pub(super) fn mixed<T: PartialEq>(looks: &[Look], value: impl Fn(&Look) -> T) -> bool {
    looks.first().is_some_and(|first| looks.iter().skip(1).any(|look| value(look) != value(first)))
}
pub(super) fn inspected_look(chain: &[Option<Look>; 4], step: Option<LookStep>) -> Option<&Look> {
    chain.iter().skip(step.map_or(0, step_index)).flatten().next()
}
fn source_step(chain: &[Option<Look>; 4], inspected: Option<LookStep>) -> Option<LookStep> {
    chain
        .iter()
        .enumerate()
        .skip(inspected.map_or(0, step_index))
        .find_map(|(index, look)| look.as_ref().and_then(|_| LookStep::ALL.get(index)).copied())
}
fn ended(response: &egui::Response, ui: &egui::Ui) -> bool {
    response.drag_stopped()
        || response.lost_focus()
        || response.changed() && !response.has_focus() && !ui.input(|i| i.pointer.any_down()) && !ui.ctx().egui_wants_keyboard_input()
}

pub(super) fn scope_label(app: &SolveApp) -> &str {
    app.tr(match app.session.selection.last() {
        Some(Sel::Occurrence { .. }) => "render.pick_component",
        Some(Sel::Face { .. }) => "render.pick_face",
        _ => "render.pick_body",
    })
}

pub(super) fn reset(app: &mut SolveApp) {
    let view = &app.render_workspace.view.inspector.selection_appearance;
    if view.selection != app.session.selection || view.document.as_ref().is_none_or(|d| !Weak::ptr_eq(d, &Arc::downgrade(&app.session.doc))) {
        return;
    }
    let step = view.inspected.unwrap_or(mode_step(view.mode));
    let selected = targets(app, step);
    if let Some(saved) = view.saved.clone() {
        let single_targets = selected
            .items
            .into_iter()
            .map(|item| Targets { items: vec![item], components: vec![] })
            .chain(selected.components.into_iter().map(|id| Targets { items: vec![], components: vec![id] }));
        for (target, look) in single_targets.zip(saved) {
            let result =
                if let Some(look) = look { assign(app, &target, &look, &look, false) } else { clear(app, &target, &Look::custom([0; 3], 1.0)) };
            if let Err(error) = result {
                app.set_status(error, true);
                break;
            }
        }
        app.render_workspace.view.inspector.selection_appearance.revision = None;
    }
}

pub(super) fn remove(app: &mut SolveApp) {
    let view = &app.render_workspace.view.inspector.selection_appearance;
    let selected = targets(app, view.inspected.unwrap_or(mode_step(view.mode)));
    if let Some(look) = view.draft.clone() {
        if let Err(error) = clear(app, &selected, &look) {
            app.set_status(error, true);
        }
        app.render_workspace.view.inspector.selection_appearance.revision = None;
    }
}

pub(super) fn footer(app: &mut SolveApp, ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        let enabled = !app.session.selection.is_empty();
        if ui_kit::icon_label_action(ui, app.tr("render.inspector_save_library"), Some("plus"), enabled, false)
            .on_hover_text(app.tr_label("Export design library..."))
            .clicked()
        {
            app.start("library.export");
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui_kit::text_button(ui, app.tr("render.inspector_reset"), enabled).clicked() {
                reset(app);
            }
        });
    });
}

/// Returns false only when the explicit-target fallback should be shown.
pub(super) fn controls(app: &mut SolveApp, ui: &mut egui::Ui) -> bool {
    if app.session.selection.is_empty() {
        app.render_workspace.view.inspector.selection_appearance = View::default();
        return false;
    }
    if app.session.selection.len() > 10_000 {
        ui_kit::text(ui, app.tr("render.inspector_target_limit"), ui_kit::Text::Note("render.inspector_target_limit"));
        return true;
    }
    let mut view = std::mem::take(&mut app.render_workspace.view.inspector.selection_appearance);
    let pick_mode = if app.ui.pick_components { 2 } else { usize::from(!app.ui.pick_bodies) };
    if view.selection == app.session.selection && view.pick_mode != pick_mode {
        view.mode = pick_mode;
        view.inspected = None;
        view.saved = None;
        view.revision = None;
        view.all_users = false;
    }
    view.pick_mode = pick_mode;
    if view.selection != app.session.selection {
        view.selection = app.session.selection.clone();
        view.mode = if app.ui.pick_components || view.selection.iter().any(|s| matches!(s, Sel::Occurrence { .. })) {
            2
        } else if view.selection.iter().any(|s| matches!(s, Sel::Face { .. })) {
            1
        } else {
            0
        };
        view.inspected = None;
        view.saved = None;
        view.revision = None;
        view.all_users = false;
    }
    let options = [app.tr("render.pick_face"), app.tr("render.pick_body"), app.tr("render.pick_component")].map(str::to_owned);
    let mut kind = match view.mode {
        0 => 1,
        1 => 0,
        _ => 2,
    };
    if ui_kit::segmented(
        ui,
        &mut kind,
        &options.each_ref().map(String::as_str),
        ui_kit::Segments { compact: true, dense: true, workspace_accent: true, icons: &["surface", "body", "component"], ..Default::default() },
    ) {
        view.mode = match kind {
            0 => 1,
            1 => 0,
            _ => 2,
        };
        view.inspected = None;
        view.saved = None;
        view.revision = None;
        view.all_users = false;
        let mode = match view.mode {
            1 => "face",
            2 => "component",
            _ => "body",
        };
        let _ = app.run("select.filter", json!({"mode":mode}));
        view.pick_mode = view.mode;
    }
    let selected_targets = targets(app, mode_step(view.mode));
    let t = crate::theme::Tokens::get();
    let result = ui_kit::settings_form(ui, t.form_label_width + t.space_8, |ui| draw(app, ui, &mut view, &selected_targets));
    if let Err(error) = result {
        ui_kit::text(ui, &error, ui_kit::Text::Error("render:appearance:error"));
    }
    let body = if view.inspected.unwrap_or(mode_step(view.mode)) == LookStep::Body && selected_targets.items.len() == 1 {
        match selected_targets.items.first() {
            Some(Sel::Body { name }) => Some(name.as_str()),
            _ => None,
        }
    } else {
        None
    };
    ui_kit::settings_form(ui, t.form_label_width + t.space_8, |ui| super::materials::selection_controls(app, ui, body));
    // Resource forms keep their existing explicit prepare/review/apply flow until
    // the Surfaces and references editor owns them. They never duplicate optics.
    let resource_kinds = ["surfaces", "materials", "selene_appearances", "physical_profiles"];
    if resource_kinds.iter().any(|kind| super::library::selected_kind(app, kind)) {
        let title = app.tr("render.inspector_advanced").to_owned();
        ui_kit::editor_section(ui, "render:appearance:resources", &title, true, |ui| super::library::inspect_kinds(app, ui, &resource_kinds));
    }
    app.render_workspace.view.inspector.selection_appearance = view;
    true
}
fn draw(app: &mut SolveApp, ui: &mut egui::Ui, view: &mut View, selected: &Targets) -> Result<(), String> {
    if selected.items.len().saturating_add(selected.components.len()) > 10_000 {
        return Err(app.tr("render.inspector_target_limit").into());
    }
    let chains = chains(app, selected)?;
    let Some(chain) = chains.first() else {
        ui_kit::text(ui, app.tr("render.inspector_pick_target"), ui_kit::Text::Note("render.inspector_pick_target"));
        return Ok(());
    };
    let default_color = crate::theme::Tokens::get().body;
    let default = Look::custom([default_color.r(), default_color.g(), default_color.b()], 1.0);
    let looks: Vec<_> = chains
        .iter()
        .enumerate()
        .map(|(index, chain)| {
            inspected_look(chain, view.inspected)
                .cloned()
                .or_else(|| {
                    selected.items.get(index).filter(|_| !matches!(view.inspected, Some(LookStep::Component | LookStep::Material))).and_then(|item| {
                        match item {
                            Sel::Body { name } | Sel::Face { body: name, .. } => {
                                crate::open_meshing::session_world(&app.session).body(name).and_then(|body| app.session.doc.model_display_look(body))
                            }
                            _ => None,
                        }
                    })
                })
                .unwrap_or_else(|| default.clone())
        })
        .collect();
    let Some(from) = looks.first() else { return Ok(()) };
    if view.saved.is_none() {
        view.saved =
            Some(chains.iter().map(|chain| chain.get(step_index(view.inspected.unwrap_or(mode_step(view.mode)))).cloned().flatten()).collect());
    }
    let is_mixed = mixed(&looks, Clone::clone);
    let distinct: std::collections::BTreeSet<_> = looks.iter().map(|look| look_params(look).to_string()).collect();
    let count =
        if !is_mixed { users(app, from) } else { assigned_looks(app).filter(|look| distinct.contains(&look_params(look).to_string())).count() };
    let active = source_step(chain, None).unwrap_or(mode_step(view.mode));
    let source_step = source_step(chain, view.inspected).unwrap_or(mode_step(view.mode));
    let replaceable = !is_mixed && count > 0 && source_step != LookStep::Material;
    if !replaceable {
        view.all_users = false;
    }
    if view.revision != Some(app.session.revision) || view.document.as_ref().is_none_or(|d| !Weak::ptr_eq(d, &Arc::downgrade(&app.session.doc))) {
        view.draft = Some(from.clone());
        view.dirty = false;
        view.revision = Some(app.session.revision);
        view.document = Some(Arc::downgrade(&app.session.doc));
        view.replacements = dialogs_appearance::replacement_library(app);
    }
    let mut draft = view.draft.clone().unwrap_or_else(|| from.clone());
    let name = if is_mixed { app.tr("render.tree_mixed").to_owned() } else { from.name.clone() };
    let source = app.tr(if source_step == LookStep::Material { "render.inspector_material" } else { "render.library_in_design" }).to_owned();
    let used = crate::i18n::format("render.inspector_used_by", &[("count", &count.to_string())]);
    let replacements = &view.replacements;
    let ids: Vec<_> = (0..replacements.len()).map(|index| index.to_string()).collect();
    let handles: Vec<_> = ids.iter().map(|id| format!("render:appearance:replace:{id}")).collect();
    let options: Vec<_> = replacements
        .iter()
        .zip(&ids)
        .zip(&handles)
        .map(|((look, id), handle)| ui_kit::SearchOption { value: id, label: &look.name, handle: Some(handle) })
        .collect();
    let mut inspected = None;
    let selected_step = view.inspected.unwrap_or(active);
    let chain_labels = [app.tr("render.pick_face"), app.tr("render.pick_body"), app.tr("render.pick_component"), app.tr("render.inspector_material")];
    let replacement = ui_kit::sphere_preview_with_source(
        ui,
        "render:appearance:preview",
        sphere(&draft),
        &name,
        &used,
        |ui| {
            let response = ui_kit::compact_dropdown_menu(ui, "render_appearance_chain", &source, |ui| {
                ui.set_min_width(crate::theme::Tokens::get().form_label_width * 3.0);
                if let Some(step) =
                    ui_kit::look_chain(ui, "render:appearance:chain", chain_labels, chain.each_ref().map(|l| l.as_ref().map(sphere)), selected_step)
                {
                    inspected = Some(step);
                    ui.close();
                }
            });
            crate::scenario::publish_target("render:appearance:chain-menu", response.rect.intersect(ui.clip_rect()));
            response
        },
        |ui| ui_kit::searchable_icon_dropdown(ui, "spin", app.tr("render.inspector_replace"), &mut view.replace_search, &options),
    );
    if let Some(step) = inspected {
        view.inspected = Some(step);
        view.saved = None;
        view.revision = None;
        view.all_users = false;
        return Ok(());
    }
    if let Some(to) = replacement.and_then(|id| id.parse::<usize>().ok()).and_then(|index| replacements.get(index)) {
        let to = Look { name: to.name.clone(), color: to.color, opacity: to.opacity, pbr: to.pbr };
        let step = view.inspected.filter(|step| *step != LookStep::Material).unwrap_or(mode_step(view.mode));
        let replace_targets = targets(app, step);
        assign(app, &replace_targets, from, &to, view.all_users)?;
        view.revision = None;
        return Ok(());
    }
    let step = view.inspected.unwrap_or(mode_step(view.mode));
    let mut scope = usize::from(view.all_users);
    let all_label = crate::i18n::format("render.inspector_all_users", &[("count", &count.to_string())]);
    let this_label = app
        .tr(match step {
            LookStep::Face => "render.inspector_this_face",
            LookStep::Component => "render.inspector_this_component",
            _ => "render.inspector_this_body",
        })
        .to_owned();
    ui_kit::row(ui, app.tr("render.inspector_applies_to"), |ui| {
        ui.add_enabled_ui(replaceable, |ui| {
            ui_kit::segmented(
                ui,
                &mut scope,
                &[&this_label, &all_label],
                ui_kit::Segments { compact: true, small: true, workspace_accent: true, ..Default::default() },
            )
        });
    });
    view.all_users = scope == 1;
    let edit_targets = targets(app, step);
    let mut commit = false;
    let edit_title = app.tr("render.inspector_edit").to_owned();
    ui_kit::editor_section(ui, "render:appearance:edit", &edit_title, true, |ui| {
        if is_mixed {
            ui_kit::text(ui, app.tr("render.inspector_set_all"), ui_kit::Text::Note("render.inspector_set_all"));
        }
        let editable = step != LookStep::Material && (!edit_targets.items.is_empty() || !edit_targets.components.is_empty());
        ui.add_enabled_ui(editable, |ui| {
            ui_kit::row(ui, app.tr("render.inspector_colour"), |ui| {
                let opacity = draft.pbr.is_none().then_some(&mut draft.opacity);
                let response =
                    ui_kit::mixed_color_field(ui, &mut draft.color, opacity, mixed(&looks, |l| l.color).then(|| app.tr("render.tree_mixed")));
                view.dirty |= response.changed();
                commit |= ended(&response, ui);
            });
            let mut preset = finish_index(&draft);
            let states = if mixed(&looks, |l| l.pbr) { [false; 4] } else { std::array::from_fn(|i| i == preset) };
            ui_kit::row(ui, app.tr("render.inspector_finish"), |ui| {
                let labels =
                    ["render.inspector_matte", "render.inspector_satin", "render.inspector_gloss", "render.inspector_metal"].map(|key| app.tr(key));
                if ui_kit::segmented(
                    ui,
                    &mut preset,
                    &labels,
                    ui_kit::Segments { states: &states, compact: true, small: true, workspace_accent: true, ..Default::default() },
                ) {
                    if let Err(error) = finish(&mut draft, preset) {
                        app.set_status(error, true);
                    } else {
                        view.dirty = true;
                        commit = true;
                    }
                }
            });
            let mut p = draft.pbr.unwrap_or_default();
            let mut metallic = p.metallic();
            let mut roughness = p.roughness();
            let mut ior = p.dielectric_ior();
            let mut optical_edit = false;
            for (key, value, range, mixed) in [
                ("appearance.metallic", &mut metallic, 0.0..=1.0, mixed(&looks, |l| l.pbr.unwrap_or_default().metallic())),
                ("appearance.roughness", &mut roughness, 0.045..=1.0, mixed(&looks, |l| l.pbr.unwrap_or_default().roughness())),
            ] {
                ui_kit::row(ui, app.tr(key), |ui| {
                    let response = ui_kit::mixed_slider(ui, value, range, mixed.then(|| app.tr("render.tree_mixed")));
                    view.dirty |= response.changed();
                    optical_edit |= response.changed();
                    commit |= ended(&response, ui);
                });
            }
            ui_kit::row(ui, app.tr("render.inspector_opacity"), |ui| {
                let response =
                    ui_kit::mixed_slider(ui, &mut draft.opacity, 0.0..=1.0, mixed(&looks, |l| l.opacity).then(|| app.tr("render.tree_mixed")));
                view.dirty |= response.changed();
                commit |= ended(&response, ui);
                if response.changed() && draft.opacity < 1.0 {
                    draft.pbr = None;
                }
            });
            ui_kit::row(ui, app.tr("render.appearance_editor.ior"), |ui| {
                let response = ui_kit::mixed_scalar_field(
                    ui,
                    &mut ior,
                    1.0..=3.0,
                    mixed(&looks, |l| l.pbr.unwrap_or_default().dielectric_ior()).then(|| app.tr("render.tree_mixed")),
                );
                view.dirty |= response.changed();
                optical_edit |= response.changed();
                commit |= ended(&response, ui);
            });
            ui_kit::text(ui, app.tr("render.inspector_opaque_help"), ui_kit::Text::Note("render.inspector_opaque_help"));
            if optical_edit && let Ok(parameters) = PbrParameters::new(metallic, roughness, ior) {
                p = parameters;
                draft.pbr = Some(p);
                draft.opacity = 1.0;
            }
        });
    });
    commit |= view.dirty && !ui.input(|i| i.pointer.any_down()) && !ui.ctx().egui_wants_keyboard_input();
    if commit && view.dirty && (draft != *from || is_mixed) {
        assign(app, &edit_targets, from, &draft, view.all_users)?;
        view.revision = None;
    }
    view.draft = Some(draft);
    Ok(())
}
