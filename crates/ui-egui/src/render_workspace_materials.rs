//! Transient body-level optical overrides for the captured GPU image request.
//! The model's appearances, physical material cards and saved setups are not changed.
use std::sync::{Arc, Weak};

use serde::Deserialize;
use serde_json::{Value, json};
use solidcraft_engine::{
    doc::{
        Document,
        appearance_pack::{self, GlassBoundary, Recipe},
    },
    geom::Vec3,
    render::{
        clear_coat::ClearCoat,
        job::textures::{BaseColorMode, PlanarFrame},
        surface::OpaqueSurface,
        transmission::Transmission,
    },
};

use crate::{SolveApp, theme::Tokens, ui_kit};

const MAX_BINDINGS: usize = 128;

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub(super) struct Target {
    #[serde(default)]
    pub stable_target: Option<solidcraft_engine::doc::render_surface::RenderBodyTarget>,
    pub ordinal: usize,
    pub name: String,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Boundary {
    Thin,
    Box,
    #[default]
    Closed,
}
impl Boundary {
    fn wire(self) -> &'static str {
        match self {
            Self::Thin => "thin_sheet",
            Self::Box => "closed_box",
            Self::Closed => "closed_mesh",
        }
    }
    fn label(self) -> &'static str {
        match self {
            Self::Thin => "render.material_thin",
            Self::Box => "render.material_box",
            Self::Closed => "render.material_closed",
        }
    }
}

#[derive(Clone, Debug)]
pub(super) struct Glass {
    pub recipe: Option<&'static str>,
    pub ior: f64,
    pub absorption: [f64; 3],
    pub boundary: Boundary,
    pub thickness: f64,
    pub shadows: bool,
    pub subsurface: Option<solidcraft_engine::render::transmission::subsurface::Subsurface>,
}
impl Default for Glass {
    fn default() -> Self {
        Self { recipe: None, ior: 1.5, absorption: [0.0; 3], boundary: Boundary::Closed, thickness: 1.0, shadows: false, subsurface: None }
    }
}
impl Glass {
    pub(super) fn recipe(id: &str) -> Result<Self, String> {
        let preset = appearance_pack::find(id).ok_or_else(|| "unknown glass recipe".to_string())?;
        let Recipe::Glass(recipe) = preset.recipe else { return Err("the selected recipe is not physical glass".into()) };
        recipe.validate().map_err(|e| e.to_string())?;
        let (boundary, thickness) = match recipe.boundary {
            GlassBoundary::Thin { thickness_mm } => (Boundary::Thin, thickness_mm),
            GlassBoundary::Closed => (Boundary::Closed, 1.0),
        };
        Ok(Self {
            subsurface: None,
            recipe: Some(preset.id),
            ior: recipe.dielectric_ior,
            absorption: recipe.transmission_linear_rgb.map(|t| -t.ln() / recipe.reference_path_mm),
            boundary,
            thickness,
            shadows: boundary == Boundary::Thin,
        })
    }
}

#[derive(Clone, Debug)]
pub(super) struct Coat {
    pub weight: f64,
    pub roughness: f64,
    pub ior: f64,
}
impl Default for Coat {
    fn default() -> Self {
        Self { weight: 1.0, roughness: 0.12, ior: 1.5 }
    }
}

#[derive(Clone, Debug)]
pub(super) struct Maps {
    pub color: String,
    pub normal: String,
    pub roughness: String,
    pub metallic: String,
    pub base_color_mode: BaseColorMode,
    pub flip_v: bool,
    pub strength: f64,
    pub frame: PlanarFrame,
}
impl Default for Maps {
    fn default() -> Self {
        Self {
            color: String::new(),
            normal: String::new(),
            roughness: String::new(),
            metallic: String::new(),
            base_color_mode: BaseColorMode::Modulate,
            flip_v: false,
            strength: 1.0,
            frame: PlanarFrame { origin: [0.0; 3], u: [10.0, 0.0, 0.0], v: [0.0, 10.0, 0.0] },
        }
    }
}

#[derive(Clone, Debug)]
pub(super) struct Binding {
    pub target: Target,
    revision: u64,
    document: Weak<Document>,
    pub enabled: bool,
    pub glass_enabled: bool,
    pub glass: Glass,
    pub coat_enabled: bool,
    pub coat: Coat,
    pub maps_enabled: bool,
    pub maps: Maps,
    replace_saved_optics: bool,
}
impl Binding {
    pub(super) fn new(target: Target, app: &SolveApp) -> Self {
        Self {
            target,
            revision: app.session.revision,
            document: Arc::downgrade(&app.session.doc),
            enabled: true,
            glass_enabled: false,
            glass: Glass::default(),
            coat_enabled: false,
            coat: Coat::default(),
            maps_enabled: false,
            maps: Maps::default(),
            replace_saved_optics: false,
        }
    }
    fn current(&self, app: &SolveApp) -> bool {
        self.revision == app.session.revision && Weak::ptr_eq(&self.document, &Arc::downgrade(&app.session.doc)) && !app.session.model.is_stale()
    }
    fn active(&self) -> bool {
        self.enabled && (self.glass_enabled || self.coat_enabled || self.maps_enabled)
    }
}

#[derive(Clone, Debug, Default)]
pub(super) struct View {
    pub enabled: bool,
    pub bindings: Vec<Binding>,
    targets: Vec<Target>,
    target: Option<usize>,
    target_search: String,
    selected: Option<usize>,
    target_revision: Option<u64>,
    target_document: Option<Weak<Document>>,
}

pub(super) fn read_targets(app: &mut SolveApp) -> Result<Vec<Target>, String> {
    let reply = app.run("render.scene", json!({}))?;
    let values = reply.get("bodies").and_then(Value::as_array).ok_or_else(|| "missing render scene bodies".to_string())?;
    if values.len() > solidcraft_engine::render::job::MAX_GPU_PATH_BODIES {
        return Err("render scene exceeds its body limit".into());
    }
    let targets: Vec<Target> = serde_json::from_value(Value::Array(values.clone())).map_err(|e| e.to_string())?;
    if targets.iter().any(|t| {
        t.ordinal >= solidcraft_engine::render::job::MAX_GPU_PATH_BODIES
            || t.name.is_empty()
            || t.name.len() > 1024
            || t.name.chars().any(char::is_control)
    }) {
        return Err("invalid render scene identity".into());
    }
    Ok(targets)
}

pub(super) fn revalidate(app: &mut SolveApp, binding: &mut Binding) -> Result<(), String> {
    let targets = read_targets(app)?;
    if !targets.contains(&binding.target) {
        return Err(app.tr("render.material_target_missing").to_string());
    }
    binding.revision = app.session.revision;
    binding.document = Arc::downgrade(&app.session.doc);
    Ok(())
}

/// Called for every Check and Start through the one shared image request builder.
/// No files are read until the engine captures the actual request.
pub(super) fn params(app: &SolveApp, params: &mut Value) -> Result<(), String> {
    let view = &app.render_workspace.view.materials;
    if !view.enabled || !view.bindings.iter().any(Binding::active) {
        return Ok(());
    }
    if app.render_workspace.view.method != super::ImageMethod::GpuRaytrace {
        return Err(app.tr("render.material_gpu_required").to_string());
    }
    if view.bindings.len() > MAX_BINDINGS {
        return Err(app.tr("render.material_limits").to_string());
    }
    let (mut glass, mut coats, mut textures) = (Vec::new(), Vec::new(), Vec::new());
    let mut seen = std::collections::BTreeSet::new();
    for binding in view.bindings.iter().filter(|b| b.active()) {
        if !binding.current(app) {
            return Err(app.tr("render.material_revalidate_needed").to_string());
        }
        let target = &binding.target;
        if target.ordinal >= solidcraft_engine::render::job::MAX_GPU_PATH_BODIES
            || target.name.is_empty()
            || target.name.len() > 1024
            || target.name.chars().any(char::is_control)
            || !seen.insert(target.ordinal)
        {
            return Err(app.tr("render.material_target_missing").to_string());
        }
        if binding.glass_enabled && (binding.coat_enabled || binding.maps_enabled) {
            return Err(app.tr("render.material_glass_exclusive").to_string());
        }
        let identity = || json!({"body_index":target.ordinal,"body_name":target.name});
        if binding.glass_enabled {
            let value = &binding.glass;
            OpaqueSurface::new(Vec3::ZERO, 0.0, 0.5, value.ior).map_err(|e| e.to_string())?;
            let [r, g, b] = value.absorption;
            Transmission::new(Vec3::new(r, g, b), (value.boundary == Boundary::Thin).then_some(value.thickness)).map_err(|e| e.to_string())?;
            let mut wire = identity();
            wire["ior"] = json!(value.ior);
            wire["absorption_per_mm"] = json!(value.absorption);
            wire["boundary"] = json!(value.boundary.wire());
            wire["thickness_mm"] = json!(if value.boundary == Boundary::Thin { value.thickness } else { 0.0 });
            wire["shadow_transmission"] = json!(value.shadows);
            if let Some(subsurface) = value.subsurface {
                Transmission::new(Vec3::new(r, g, b), (value.boundary == Boundary::Thin).then_some(value.thickness))
                    .map_err(|e| e.to_string())?
                    .with_shadow_transmission(value.shadows)
                    .with_subsurface(subsurface)
                    .map_err(|e| e.to_string())?;
                wire["subsurface"] = json!(subsurface);
            }
            glass.push(wire);
        }
        if binding.coat_enabled {
            let value = &binding.coat;
            ClearCoat::new(value.weight, value.roughness, value.ior).map_err(|e| e.to_string())?;
            let mut wire = identity();
            wire["weight"] = json!(value.weight);
            wire["roughness"] = json!(value.roughness);
            wire["ior"] = json!(value.ior);
            coats.push(wire);
        }
        if binding.maps_enabled {
            let value = &binding.maps;
            value.frame.validate().map_err(|e| e.to_string())?;
            if value.color.is_empty() && value.normal.is_empty() && value.roughness.is_empty() && value.metallic.is_empty() {
                return Err(app.tr("render.material_map_required").to_string());
            }
            if !value.strength.is_finite()
                || !(0.0..=1.0).contains(&value.strength)
                || [&value.color, &value.normal, &value.roughness, &value.metallic].iter().any(|p| p.len() > 4096 || p.chars().any(char::is_control))
            {
                return Err(app.tr("render.material_map_invalid").to_string());
            }
            let mut wire = identity();
            wire["frame"] = json!(value.frame);
            wire["normal_strength"] = json!(value.strength);
            wire["base_color_mode"] = json!(value.base_color_mode);
            wire["flip_v"] = json!(value.flip_v);
            if !value.color.is_empty() {
                wire["base_color_srgb"] = json!(value.color);
            }
            if !value.normal.is_empty() {
                wire["normal_opengl"] = json!(value.normal);
            }
            if !value.roughness.is_empty() {
                wire["roughness_linear"] = json!(value.roughness);
            }
            if !value.metallic.is_empty() {
                wire["metallic_linear"] = json!(value.metallic);
            }
            textures.push(wire);
        }
    }
    if glass.len() > solidcraft_engine::render::job::MAX_GPU_DIELECTRICS
        || textures.len() > solidcraft_engine::render::job::textures::MAX_TEXTURE_BINDINGS
    {
        return Err(app.tr("render.material_limits").to_string());
    }
    params["raytrace"] = json!({"dielectrics":glass,"clear_coats":coats,"textures":textures});
    Ok(())
}

fn vector(app: &SolveApp, ui: &mut egui::Ui, key: &str, values: &mut [f64; 3], range: std::ops::RangeInclusive<f64>, suffix: &str) -> bool {
    ui.label(app.tr(key));
    let mut changed = false;
    ui.horizontal(|ui| {
        for value in values {
            changed |= ui_kit::number_field(ui, value, range.clone(), 0.01, suffix).changed();
        }
    });
    changed
}

pub(super) fn controls(app: &mut SolveApp, ui: &mut egui::Ui) {
    saved_optics_controls(app, ui);
    let title = app.tr("render.material_title").to_string();
    ui_kit::section(ui, "render_materials", &title, false, |ui| {
        let mut view = std::mem::take(&mut app.render_workspace.view.materials);
        ui.small(app.tr("render.material_transient"));
        ui_kit::checkbox(ui, app.tr("render.material_enable"), &mut view.enabled);
        if app.render_workspace.view.method != super::ImageMethod::GpuRaytrace && view.enabled && view.bindings.iter().any(Binding::active) {
            ui.colored_label(Tokens::get().warning, app.tr("render.material_gpu_required"));
        }
        if ui_kit::button(ui, app.tr("render.material_refresh"), ui_kit::Button::Normal).clicked() {
            match read_targets(app) {
                Ok(targets) => {
                    view.targets = targets;
                    view.target = None;
                    view.target_revision = Some(app.session.revision);
                    view.target_document = Some(Arc::downgrade(&app.session.doc));
                }
                Err(e) => app.set_status(e, true),
            }
        }
        let current = view.target_revision == Some(app.session.revision)
            && view.target_document.as_ref().is_some_and(|doc| Weak::ptr_eq(doc, &Arc::downgrade(&app.session.doc)))
            && !app.session.model.is_stale();
        ui_kit::bounded_cell_editor(
            ui,
            ui.id().with("render_material_search"),
            &mut view.target_search,
            app.tr("render.material_filter"),
            false,
            128,
        );
        let query = view.target_search.trim().to_lowercase();
        let label =
            view.target.and_then(|i| view.targets.get(i)).map_or_else(|| app.tr("render.material_choose_body").to_string(), |t| t.name.clone());
        ui_kit::dropdown_menu(ui, "render_material_target", &label, |ui| {
            for (index, target) in view.targets.iter().enumerate().filter(|(_, t)| t.name.to_lowercase().contains(&query)).take(128) {
                if ui_kit::list_picker(ui, &format!("render:material-body-{}", target.ordinal), &target.name, view.target == Some(index)).clicked() {
                    view.target = Some(index);
                    ui.close();
                }
            }
        });
        let target = view.target.and_then(|index| view.targets.get(index));
        let existing = target.and_then(|target| view.bindings.iter().position(|binding| binding.target == *target));
        if ui_kit::button(
            ui,
            app.tr("render.material_add"),
            ui_kit::Button::Enabled(current && target.is_some() && (existing.is_some() || view.bindings.len() < MAX_BINDINGS)),
        )
        .clicked()
            && let Some(target) = target.cloned()
        {
            view.selected = if let Some(index) = existing {
                Some(index)
            } else {
                view.bindings.push(Binding::new(target, app));
                Some(view.bindings.len() - 1)
            };
            view.enabled = true;
        }
        if view.targets.len() > 128 {
            ui.small(app.tr("render.material_filter_help"));
        }
        if !current && !view.targets.is_empty() {
            ui.small(app.tr("render.material_refresh_needed"));
        }
        let selected = view
            .selected
            .and_then(|i| view.bindings.get(i))
            .map_or_else(|| app.tr("render.material_assignments").to_string(), |b| b.target.name.clone());
        ui_kit::dropdown_menu(ui, "render_material_binding", &selected, |ui| {
            for (index, binding) in view.bindings.iter().enumerate() {
                if ui_kit::list_picker(ui, &format!("render:material-binding-{index}"), &binding.target.name, view.selected == Some(index)).clicked()
                {
                    view.selected = Some(index);
                    ui.close();
                }
            }
        });
        let mut remove = false;
        if let Some(binding) = view.selected.and_then(|index| view.bindings.get_mut(index)) {
            ui.small(format!("{} · #{}", binding.target.name, binding.target.ordinal));
            ui_kit::checkbox(ui, app.tr("render.material_binding_enable"), &mut binding.enabled);
            if !binding.current(app) {
                ui.colored_label(Tokens::get().warning, app.tr("render.material_revalidate_needed"));
                if ui_kit::button(ui, app.tr("render.material_revalidate"), ui_kit::Button::Normal).clicked()
                    && let Err(e) = revalidate(app, binding)
                {
                    app.set_status(e, true);
                }
            }
            glass_controls(app, ui, binding);
            coat_controls(app, ui, binding);
            map_controls(app, ui, binding);
            ui_kit::checkbox(ui, app.tr("render.optics_replace"), &mut binding.replace_saved_optics);
            let save = ui_kit::button(
                ui,
                app.tr("render.optics_save"),
                ui_kit::Button::Enabled(binding.glass_enabled && binding.current(app) && !binding.maps_enabled && !binding.coat_enabled),
            );
            crate::scenario::publish_target("render:optics-save", save.rect);
            if save.clicked() {
                // The transient editor is taken out of app above. snapshot saves
                // only the numerical draft; other local bindings remain visible
                // and stale for explicit revalidation after this one mutation.
                match save_optics(app, binding) {
                    Ok(()) => remove = true,
                    Err(error) => app.set_status(error, true),
                }
            }
            remove |= ui_kit::button(ui, app.tr("render.material_remove"), ui_kit::Button::Normal).clicked();
        }
        if remove && let Some(index) = view.selected.filter(|i| *i < view.bindings.len()) {
            view.bindings.remove(index);
            view.selected = None;
        }
        ui.small(app.tr("render.material_limits"));
        app.render_workspace.view.materials = view;
    });
}

fn glass_controls(app: &mut SolveApp, ui: &mut egui::Ui, binding: &mut Binding) {
    let title = app.tr("render.material_glass").to_string();
    ui_kit::section(ui, ("render_glass", binding.target.ordinal), &title, false, |ui| {
        if ui_kit::checkbox(ui, app.tr("render.material_glass_enable"), &mut binding.glass_enabled).changed() && binding.glass_enabled {
            binding.coat_enabled = false;
            binding.maps_enabled = false;
        }
        ui.small(app.tr("render.material_glass_help"));
        let recipes: Vec<_> = appearance_pack::STARTER_PACK.iter().filter(|p| matches!(p.recipe, Recipe::Glass(_))).collect();
        ui_kit::swatch_grid(ui, &recipes, |ui, preset| {
            let [r, g, b] = preset.preview_color();
            let response = ui_kit::color_swatch(
                ui,
                egui::Id::new(("render_glass_recipe", binding.target.ordinal, preset.id)),
                egui::Color32::from_rgb(r, g, b),
                binding.glass.recipe == Some(preset.id),
                preset.name,
                preset.id,
            );
            crate::scenario::publish_handle(&format!("render:glass:{}", preset.id), response.rect.center());
            if response.clicked() {
                match Glass::recipe(preset.id) {
                    Ok(glass) => {
                        binding.glass = glass;
                        binding.glass_enabled = true;
                        binding.coat_enabled = false;
                        binding.maps_enabled = false;
                    }
                    Err(e) => app.set_status(e, true),
                }
            }
        });
        let name =
            binding.glass.recipe.and_then(appearance_pack::find).map_or_else(|| app.tr("render.material_custom").to_string(), |p| p.name.to_string());
        ui.label(name);
        if !binding.glass_enabled {
            return;
        }
        let glass = &mut binding.glass;
        let label = app.tr(glass.boundary.label()).to_string();
        let mut changed = false;
        ui_kit::dropdown_menu(ui, "render_glass_boundary", &label, |ui| {
            for value in [Boundary::Closed, Boundary::Thin, Boundary::Box] {
                if ui_kit::list_picker(ui, value.wire(), app.tr(value.label()), glass.boundary == value).clicked() {
                    glass.boundary = value;
                    glass.shadows = value == Boundary::Thin;
                    changed = true;
                    ui.close();
                }
            }
        });
        ui.label(app.tr("render.material_ior"));
        changed |= ui_kit::number_field(ui, &mut glass.ior, 1.0..=3.0, 0.01, "").changed();
        changed |= vector(app, ui, "render.material_absorption", &mut glass.absorption, 0.0..=100.0, " /mm");
        if glass.boundary == Boundary::Thin {
            ui.label(app.tr("render.material_thickness"));
            changed |= ui_kit::number_field(ui, &mut glass.thickness, 0.0..=10000.0, 0.1, " mm").changed();
        }
        changed |= ui_kit::checkbox(ui, app.tr("render.material_shadow"), &mut glass.shadows).changed();
        let mut scatter = glass.subsurface.is_some();
        if ui_kit::checkbox(ui, app.tr("render.subsurface_enable"), &mut scatter).changed() {
            glass.subsurface =
                scatter.then_some(solidcraft_engine::render::transmission::subsurface::Subsurface { scattering_per_mm: 0.1, anisotropy: 0.0 });
            changed = true;
        }
        if let Some(subsurface) = &mut glass.subsurface {
            changed |= ui_kit::row(ui, app.tr("render.subsurface_rate"), |ui| {
                ui_kit::number_field(ui, &mut subsurface.scattering_per_mm, 0.0..=100.0, 0.001, " /mm")
            })
            .changed();
            changed |= ui_kit::row(ui, app.tr("render.volume_anisotropy"), |ui| {
                ui_kit::number_field(ui, &mut subsurface.anisotropy, -0.95..=0.95, 0.01, "")
            })
            .changed();
            ui_kit::text(ui, app.tr("render.subsurface_help"), ui_kit::Text::Note("render.subsurface_help"));
        }
        if changed {
            glass.recipe = None;
        }
        ui.small(app.tr("render.material_certification"));
    });
}

fn coat_controls(app: &SolveApp, ui: &mut egui::Ui, binding: &mut Binding) {
    let title = app.tr("render.material_coat").to_string();
    ui_kit::section(ui, ("render_coat", binding.target.ordinal), &title, false, |ui| {
        ui.add_enabled_ui(!binding.glass_enabled, |ui| {
            ui_kit::checkbox(ui, app.tr("render.material_coat_enable"), &mut binding.coat_enabled);
            if binding.coat_enabled {
                for (key, value, range) in [
                    ("render.material_weight", &mut binding.coat.weight, 0.0..=1.0),
                    ("render.material_roughness", &mut binding.coat.roughness, 0.045..=1.0),
                    ("render.material_ior", &mut binding.coat.ior, 1.0..=3.0),
                ] {
                    ui.label(app.tr(key));
                    ui_kit::number_field(ui, value, range, 0.01, "");
                }
            }
        });
        ui.small(app.tr("render.material_coat_help"));
    });
}

#[derive(Clone, Copy)]
enum MapSlot {
    Color,
    Normal,
    Roughness,
    Metallic,
}
impl MapSlot {
    fn key(self) -> &'static str {
        match self {
            Self::Color => "render.material_color_map",
            Self::Normal => "render.material_normal_map",
            Self::Roughness => "render.material_roughness_map",
            Self::Metallic => "render.material_metallic_map",
        }
    }
    fn field(self, maps: &mut Maps) -> &mut String {
        match self {
            Self::Color => &mut maps.color,
            Self::Normal => &mut maps.normal,
            Self::Roughness => &mut maps.roughness,
            Self::Metallic => &mut maps.metallic,
        }
    }
}
fn choose_map(app: &mut SolveApp, binding: &Binding, slot: MapSlot) {
    let target = binding.target.clone();
    let origin = binding.document.clone();
    let revision = binding.revision;
    app.pick_open_for(&["png", "jpg", "jpeg"], move |app, path| {
        if !Weak::ptr_eq(&origin, &Arc::downgrade(&app.session.doc)) || app.session.revision != revision {
            return;
        }
        if let Some(path) = path
            && let Some(binding) = app
                .render_workspace
                .view
                .materials
                .bindings
                .iter_mut()
                .find(|b| b.target == target && b.revision == revision && Weak::ptr_eq(&b.document, &origin))
        {
            *slot.field(&mut binding.maps) = path;
        }
    });
}

/// Prepare through the engine, then install only a request-local binding. No
/// authored material may silently remove a glass override or switch executor.
pub(super) fn install_library_surface(app: &mut SolveApp, id: &str, target: Target, plane: &str, scale: f64) -> Result<(), String> {
    if app.render_workspace.view.method != super::ImageMethod::GpuRaytrace {
        return Err(app.tr("render.material_gpu_required").into());
    }
    let existing = app.render_workspace.view.materials.bindings.iter().position(|b| b.target == target);
    if let Some(index) = existing {
        let binding = app.render_workspace.view.materials.bindings.get(index).ok_or_else(|| "missing material binding".to_string())?;
        if !binding.current(app) {
            return Err(app.tr("render.material_revalidate_needed").into());
        }
        if binding.glass_enabled {
            return Err(app.tr("render.material_glass_exclusive").into());
        }
    } else if app.render_workspace.view.materials.bindings.len() >= MAX_BINDINGS {
        return Err(app.tr("render.material_limits").into());
    }
    let reply =
        app.run("render.library.surface.prepare", json!({"id":id,"body_index":target.ordinal,"body_name":target.name,"plane":plane,"scale":scale}))?;
    let texture = &reply["texture"];
    let frame: PlanarFrame = serde_json::from_value(texture["frame"].clone()).map_err(|e| e.to_string())?;
    let path = |key: &str| texture[key].as_str().map(str::to_string).ok_or_else(|| "invalid authored surface reply".to_string());
    let maps = Maps {
        color: path("base_color_srgb")?,
        normal: path("normal_opengl")?,
        roughness: path("roughness_linear")?,
        metallic: path("metallic_linear")?,
        strength: 1.0,
        frame,
        base_color_mode: BaseColorMode::Replace,
        flip_v: true,
    };
    let mut binding = if let Some(index) = existing {
        app.render_workspace.view.materials.bindings.get(index).cloned().ok_or_else(|| "missing material binding".to_string())?
    } else {
        Binding::new(target, app)
    };
    binding.maps = maps;
    binding.maps_enabled = true;
    binding.enabled = true;
    let view = &mut app.render_workspace.view.materials;
    if let Some(index) = existing {
        *view.bindings.get_mut(index).ok_or_else(|| "missing material binding".to_string())? = binding;
        view.selected = Some(index);
    } else {
        view.bindings.try_reserve(1).map_err(|_| "could not reserve material binding".to_string())?;
        view.bindings.push(binding);
        view.selected = Some(view.bindings.len() - 1);
    }
    view.enabled = true;
    app.render_workspace.view.dirty = true;
    Ok(())
}

fn map_controls(app: &mut SolveApp, ui: &mut egui::Ui, binding: &mut Binding) {
    let title = app.tr("render.material_maps").to_string();
    ui_kit::section(ui, ("render_maps", binding.target.ordinal), &title, false, |ui| {
        ui.add_enabled_ui(!binding.glass_enabled, |ui| {
            ui_kit::checkbox(ui, app.tr("render.material_maps_enable"), &mut binding.maps_enabled);
            if !binding.maps_enabled {
                return;
            }
            for slot in [MapSlot::Color, MapSlot::Normal, MapSlot::Roughness, MapSlot::Metallic] {
                let key = slot.key();
                ui.label(app.tr(key));
                let path = slot.field(&mut binding.maps);
                ui_kit::bounded_cell_editor(ui, ui.id().with(key), path, "", false, 4096);
                let label = app.tr("render.material_browse").to_string();
                if ui_kit::button(ui, label, ui_kit::Button::Enabled(binding.current(app))).clicked() {
                    choose_map(app, binding, slot);
                }
            }
            let mut absolute = binding.maps.base_color_mode == BaseColorMode::Replace;
            if ui_kit::checkbox(ui, app.tr("render.material_color_replace"), &mut absolute).changed() {
                binding.maps.base_color_mode = if absolute { BaseColorMode::Replace } else { BaseColorMode::Modulate };
            }
            ui_kit::checkbox(ui, app.tr("render.material_v_up"), &mut binding.maps.flip_v);
            ui.label(app.tr("render.material_normal_strength"));
            ui_kit::number_field(ui, &mut binding.maps.strength, 0.0..=1.0, 0.01, "");
            vector(app, ui, "render.material_origin", &mut binding.maps.frame.origin, -1e6..=1e6, " mm");
            vector(app, ui, "render.material_repeat_u", &mut binding.maps.frame.u, -1e6..=1e6, " mm");
            vector(app, ui, "render.material_repeat_v", &mut binding.maps.frame.v, -1e6..=1e6, " mm");
        });
        ui.small(app.tr("render.material_maps_help"));
        if binding.glass_enabled {
            ui.small(app.tr("render.material_glass_exclusive"));
        }
    });
}

fn save_optics(app: &mut SolveApp, binding: &Binding) -> Result<(), String> {
    if !binding.current(app) {
        return Err(app.tr("render.material_revalidate_needed").into());
    }
    let setup = super::setups::snapshot(app)?;
    let value = &binding.glass;
    let name = app.render_workspace.view.name.clone();
    let optics = json!({"ior":value.ior,"absorption_per_mm":value.absorption,"boundary":value.boundary.wire(),
        "thickness_mm":if value.boundary==Boundary::Thin {value.thickness} else {0.0},"shadow_transmission":value.shadows,"subsurface":value.subsurface});
    app.run(
        "render.optics.bind",
        json!({"name":name,"set":setup,"body_index":binding.target.ordinal,"body_name":binding.target.name,
        "optics":optics,"replace":binding.replace_saved_optics,"expected_revision":app.session.revision}),
    )?;
    super::setups::load(app);
    Ok(())
}
fn saved_optics_controls(app: &mut SolveApp, ui: &mut egui::Ui) {
    let bindings = app.render_workspace.view.draft.optics.clone();
    if bindings.is_empty() {
        return;
    }
    let title = app.tr("render.optics_saved").to_string();
    ui_kit::section(ui, "render_saved_optics", &title, false, |ui| {
        ui_kit::text(ui, app.tr("render.optics_saved_help"), ui_kit::Text::Note("render.optics_saved_help"));
        for (index, binding) in bindings.iter().enumerate() {
            ui.label(format!("{} #{} · n={:.4}", binding.target.definition, binding.target.feature, binding.optics.ior));
            let remove = ui_kit::button(ui, app.tr("render.optics_remove"), ui_kit::Button::Enabled(!app.render_workspace.view.dirty));
            crate::scenario::publish_target(&format!("render:optics-remove:{index}"), remove.rect);
            if remove.clicked() {
                match app.run(
                    "render.optics.unbind",
                    json!({"name":app.render_workspace.view.name,"target":binding.target,"expected_revision":app.session.revision}),
                ) {
                    Ok(_) => super::setups::load(app),
                    Err(error) => app.set_status(error, true),
                }
            }
        }
    });
}
