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
        let preset = appearance_pack::find(id).ok_or_else(|| crate::i18n::tr("render.error_glass_recipe").to_string())?;
        let Recipe::Glass(recipe) = preset.recipe else { return Err(crate::i18n::tr("render.error_physical_glass").into()) };
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
    pending_remove: Option<PendingRemoval>,
}

#[derive(Clone, Debug)]
struct PendingRemoval {
    name: String,
    target: solidcraft_engine::doc::render_surface::RenderBodyTarget,
    revision: u64,
    document: Weak<Document>,
}

pub(super) fn read_targets(app: &mut SolveApp) -> Result<Vec<Target>, String> {
    let reply = app.run("render.scene", json!({}))?;
    let values = reply.get("bodies").and_then(Value::as_array).ok_or_else(|| crate::i18n::tr("render.error_scene_bodies").to_string())?;
    if values.len() > solidcraft_engine::render::job::MAX_GPU_PATH_BODIES {
        return Err(crate::i18n::tr("render.error_scene_body_limit").into());
    }
    let targets: Vec<Target> = serde_json::from_value(Value::Array(values.clone())).map_err(|e| e.to_string())?;
    if targets.iter().any(|t| {
        t.ordinal >= solidcraft_engine::render::job::MAX_GPU_PATH_BODIES
            || t.name.is_empty()
            || t.name.len() > 1024
            || t.name.chars().any(char::is_control)
    }) {
        return Err(crate::i18n::tr("render.error_scene_identity").into());
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
    ui_kit::row(ui, app.tr(key), |ui| ui_kit::vector3_field(ui, values, range, suffix))
}

pub(super) fn controls(app: &mut SolveApp, ui: &mut egui::Ui) {
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
            remove |= editor_sections(app, ui, Some(binding));
            remove |= ui_kit::button(ui, app.tr("render.material_remove"), ui_kit::Button::Normal).clicked();
        }
        if remove && let Some(index) = view.selected.filter(|i| *i < view.bindings.len()) {
            view.bindings.remove(index);
            view.selected = None;
        }
        ui.small(app.tr("render.material_limits"));
        view.pending_remove = app.render_workspace.view.materials.pending_remove.take().or(view.pending_remove.take());
        app.render_workspace.view.materials = view;
    });
}

fn glass_controls(app: &mut SolveApp, ui: &mut egui::Ui, binding: &mut Binding) {
    if ui_kit::checkbox(ui, app.tr("render.appearance_editor.glass_enable"), &mut binding.glass_enabled).changed() && binding.glass_enabled {
        binding.coat_enabled = false;
        binding.maps_enabled = false;
    }
    let recipes: Vec<_> = appearance_pack::STARTER_PACK.iter().filter(|p| matches!(p.recipe, Recipe::Glass(_))).collect();
    let name =
        binding.glass.recipe.and_then(appearance_pack::find).map_or_else(|| app.tr("render.material_custom").to_owned(), |p| p.name.to_owned());
    let recipe_label = app.tr("render.appearance_editor.recipe").to_owned();
    ui_kit::row(ui, &recipe_label, |ui| {
        ui_kit::dropdown_menu(ui, "render_glass_recipe", &name, |ui| {
            for preset in recipes {
                let response = ui_kit::list_picker(ui, preset.id, preset.name, binding.glass.recipe == Some(preset.id));
                crate::scenario::publish_target(&format!("render:glass:{}", preset.id), response.rect);
                if response.clicked() {
                    match Glass::recipe(preset.id) {
                        Ok(glass) => {
                            binding.glass = glass;
                            binding.glass_enabled = true;
                            binding.coat_enabled = false;
                            binding.maps_enabled = false;
                        }
                        Err(error) => app.set_status(error, true),
                    }
                    ui.close();
                }
            }
        });
    });
    let glass = &mut binding.glass;
    let mut changed = false;
    ui.add_enabled_ui(binding.glass_enabled, |ui| {
        let mut boundary = match glass.boundary {
            Boundary::Closed => 0,
            Boundary::Thin => 1,
            Boundary::Box => 2,
        };
        ui_kit::row(ui, app.tr("render.appearance_editor.boundary"), |ui| {
            let labels = ["render.appearance_editor.closed", "render.appearance_editor.thin", "render.appearance_editor.box"].map(|key| app.tr(key));
            if ui_kit::segmented(ui, &mut boundary, &labels, ui_kit::Segments { compact: true, small: true, ..Default::default() }) {
                glass.boundary = match boundary {
                    1 => Boundary::Thin,
                    2 => Boundary::Box,
                    _ => Boundary::Closed,
                };
                glass.shadows = glass.boundary == Boundary::Thin;
                if glass.boundary == Boundary::Thin {
                    glass.subsurface = None;
                }
                changed = true;
            }
        });
        changed |=
            ui_kit::row(ui, app.tr("render.appearance_editor.ior"), |ui| ui_kit::number_field(ui, &mut glass.ior, 1.0..=3.0, 0.01, "")).changed();
        ui.add_enabled_ui(glass.boundary == Boundary::Thin, |ui| {
            changed |= ui_kit::row(ui, app.tr("render.appearance_editor.thickness"), |ui| {
                ui_kit::number_field(ui, &mut glass.thickness, 0.001..=10000.0, 0.1, " mm")
            })
            .changed();
        });
        changed |= vector(app, ui, "render.appearance_editor.absorption", &mut glass.absorption, 0.0..=100.0, " /mm");
        changed |= ui_kit::checkbox(ui, app.tr("render.material_shadow"), &mut glass.shadows).changed();
        ui.add_enabled_ui(glass.boundary != Boundary::Thin, |ui| {
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
            }
        });
        if glass.boundary != Boundary::Thin {
            note(app, ui, "render.appearance_editor.thickness_reason");
        }
        if glass.boundary == Boundary::Thin {
            note(app, ui, "render.appearance_editor.scatter_reason");
        }
    });
    if changed {
        glass.recipe = None;
    }
    note(app, ui, "render.appearance_editor.glass_note");
    note(app, ui, "render.material_certification");
}

fn coat_controls(app: &SolveApp, ui: &mut egui::Ui, binding: &mut Binding) {
    ui_kit::checkbox(ui, app.tr("render.material_coat_enable"), &mut binding.coat_enabled);
    ui.add_enabled_ui(binding.coat_enabled, |ui| {
        for (key, value, range) in [
            ("render.appearance_editor.weight", &mut binding.coat.weight, 0.0..=1.0),
            ("appearance.roughness", &mut binding.coat.roughness, 0.045..=1.0),
        ] {
            ui_kit::row(ui, app.tr(key), |ui| ui_kit::mixed_slider(ui, value, range, None));
        }
        ui_kit::row(ui, app.tr("render.appearance_editor.ior"), |ui| ui_kit::number_field(ui, &mut binding.coat.ior, 1.0..=3.0, 0.01, ""));
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
            Self::Color => "render.appearance_editor.base_colour",
            Self::Normal => "render.appearance_editor.normal",
            Self::Roughness => "appearance.roughness",
            Self::Metallic => "appearance.metallic",
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
        let binding =
            app.render_workspace.view.materials.bindings.get(index).ok_or_else(|| crate::i18n::tr("render.error_material_binding").to_string())?;
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
    let path = |key: &str| texture[key].as_str().map(str::to_string).ok_or_else(|| crate::i18n::tr("render.error_surface_reply").to_string());
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
        app.render_workspace
            .view
            .materials
            .bindings
            .get(index)
            .cloned()
            .ok_or_else(|| crate::i18n::tr("render.error_material_binding").to_string())?
    } else {
        Binding::new(target, app)
    };
    binding.maps = maps;
    binding.maps_enabled = true;
    binding.enabled = true;
    let view = &mut app.render_workspace.view.materials;
    if let Some(index) = existing {
        *view.bindings.get_mut(index).ok_or_else(|| crate::i18n::tr("render.error_material_binding").to_string())? = binding;
        view.selected = Some(index);
    } else {
        view.bindings.try_reserve(1).map_err(|_| crate::i18n::tr("render.error_binding_allocation").to_string())?;
        view.bindings.push(binding);
        view.selected = Some(view.bindings.len() - 1);
    }
    view.enabled = true;
    app.render_workspace.view.dirty = true;
    Ok(())
}

fn map_controls(app: &mut SolveApp, ui: &mut egui::Ui, binding: &mut Binding) {
    ui_kit::checkbox(ui, app.tr("render.material_maps_enable"), &mut binding.maps_enabled);
    ui.add_enabled_ui(binding.maps_enabled, |ui| {
        for slot in [MapSlot::Color, MapSlot::Normal, MapSlot::Metallic, MapSlot::Roughness] {
            let label = app.tr(slot.key()).to_owned();
            ui_kit::row(ui, &label, |ui| {
                let path = slot.field(&mut binding.maps);
                let label = if path.is_empty() {
                    app.tr("render.appearance_editor.choose_image").to_owned()
                } else {
                    std::path::Path::new(path).file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
                };
                if ui_kit::button(ui, &label, ui_kit::Button::Enabled(binding.current(app))).clicked() {
                    choose_map(app, binding, slot);
                }
            });
        }
        vector(app, ui, "render.appearance_editor.origin", &mut binding.maps.frame.origin, -1e6..=1e6, " mm");
        vector(app, ui, "render.appearance_editor.u_repeat", &mut binding.maps.frame.u, -1e6..=1e6, " mm");
        vector(app, ui, "render.appearance_editor.v_repeat", &mut binding.maps.frame.v, -1e6..=1e6, " mm");
        ui_kit::row(ui, app.tr("render.appearance_editor.normal_strength"), |ui| {
            ui_kit::mixed_slider(ui, &mut binding.maps.strength, 0.0..=1.0, None)
        });
        ui_kit::checkbox(ui, app.tr("render.appearance_editor.v_up"), &mut binding.maps.flip_v);
        let mut absolute = binding.maps.base_color_mode == BaseColorMode::Replace;
        if ui_kit::checkbox(ui, app.tr("render.material_color_replace"), &mut absolute).changed() {
            binding.maps.base_color_mode = if absolute { BaseColorMode::Replace } else { BaseColorMode::Modulate };
        }
    });
}

fn save_optics(app: &mut SolveApp, binding: &Binding) -> Result<(), String> {
    if !binding.current(app) {
        return Err(app.tr("render.material_revalidate_needed").into());
    }
    // A saved shot owns numerical intent, not the tab's local map/coat bindings.
    // Keep those bindings intact while taking the portable shot snapshot.
    let local = std::mem::take(&mut app.render_workspace.view.materials);
    let setup = super::setups::snapshot(app);
    app.render_workspace.view.materials = local;
    let setup = setup?;
    let value = &binding.glass;
    let name = app.render_workspace.view.name.clone();
    let optics = json!({"ior":value.ior,"absorption_per_mm":value.absorption,"boundary":value.boundary.wire(),
        "thickness_mm":if value.boundary==Boundary::Thin {value.thickness} else {0.0},"shadow_transmission":value.shadows,"subsurface":value.subsurface});
    app.run(
        "render.optics.bind",
        json!({"name":name,"set":setup,"body_index":binding.target.ordinal,"body_name":binding.target.name,
        "optics":optics,"replace":binding.replace_saved_optics,"expected_revision":app.session.revision}),
    )?;
    let local = std::mem::take(&mut app.render_workspace.view.materials);
    super::setups::load(app);
    app.render_workspace.view.materials = local;
    Ok(())
}
fn note(app: &SolveApp, ui: &mut egui::Ui, key: &'static str) {
    ui_kit::text(ui, app.tr(key), ui_kit::Text::Note(key));
}

/// The same body editor is used by the selection inspector and explicit-target page.
/// An unsupported selection gets an inert draft so every design control is inspectable.
fn editor_sections(app: &mut SolveApp, ui: &mut egui::Ui, binding: Option<&mut Binding>) -> bool {
    let supported = binding.is_some();
    let mut placeholder = Binding::new(Target { stable_target: None, ordinal: 0, name: String::new() }, app);
    let binding = binding.unwrap_or(&mut placeholder);
    let reason = if !supported {
        Some(app.tr("render.appearance_editor.body_reason").to_owned())
    } else if app.render_workspace.view.method != super::ImageMethod::GpuRaytrace {
        Some(app.tr("render.appearance_editor.gpu_reason").to_owned())
    } else {
        None
    };
    let title = app.tr("render.appearance_editor.glass").to_owned();
    ui_kit::method_section(ui, "render:appearance:glass", &title, "GPU", reason.as_deref(), |ui| glass_controls(app, ui, binding));
    let opaque_reason = reason.clone().or_else(|| binding.glass_enabled.then(|| app.tr("render.material_glass_exclusive").to_owned()));
    let title = app.tr("render.appearance_editor.coat").to_owned();
    ui_kit::method_section(ui, "render:appearance:coat", &title, "GPU", opaque_reason.as_deref(), |ui| coat_controls(app, ui, binding));
    let title = app.tr("render.appearance_editor.maps").to_owned();
    ui_kit::method_section(ui, "render:appearance:maps", &title, "GPU", opaque_reason.as_deref(), |ui| map_controls(app, ui, binding));
    let title = app.tr("render.appearance_editor.saved").to_owned();
    let removed = ui_kit::editor_section(ui, "render:appearance:saved", &title, false, |ui| {
        ui.add_enabled_ui(supported, |ui| saved_controls(app, ui, binding)).inner
    })
    .unwrap_or(false);
    reflection_section(app, ui, supported.then_some(binding.target.name.as_str()));
    removed
}

/// Resolve one selected whole body against the authoritative captured scene. Never
/// broaden a face/component selection into a whole-body optics assignment.
pub(super) fn selection_controls(app: &mut SolveApp, ui: &mut egui::Ui, body: Option<&str>) {
    let mut binding = body.and_then(|name| app.render_workspace.view.materials.bindings.iter().find(|b| b.target.name == name).cloned());
    if binding.is_none()
        && let Some(name) = body
    {
        let current = app.render_workspace.view.materials.target_revision == Some(app.session.revision)
            && app.render_workspace.view.materials.target_document.as_ref().is_some_and(|d| Weak::ptr_eq(d, &Arc::downgrade(&app.session.doc)));
        if !current {
            match read_targets(app) {
                Ok(targets) => {
                    let view = &mut app.render_workspace.view.materials;
                    view.targets = targets;
                    view.target_revision = Some(app.session.revision);
                    view.target_document = Some(Arc::downgrade(&app.session.doc));
                }
                Err(error) => app.set_status(error, true),
            }
        }
        let mut targets = app.render_workspace.view.materials.targets.iter().filter(|t| t.name == name);
        if let Some(target) = targets.next()
            && targets.next().is_none()
        {
            binding = Some(Binding::new(target.clone(), app));
        }
    }
    let before = app.session.revision;
    let removed = editor_sections(app, ui, binding.as_mut());
    if let Some(binding) = binding {
        let view = &mut app.render_workspace.view.materials;
        let existing = view.bindings.iter().position(|b| b.target == binding.target);
        if removed {
            if let Some(index) = existing {
                view.bindings.remove(index);
            }
        } else if app.session.revision == before {
            // Opening the editor alone never allocates an inactive assignment.
            if binding.glass_enabled || binding.coat_enabled || binding.maps_enabled || existing.is_some() {
                if existing.is_none() {
                    view.enabled = true;
                }
                if let Some(index) = existing {
                    if let Some(slot) = view.bindings.get_mut(index) {
                        *slot = binding;
                    }
                } else if view.bindings.len() < MAX_BINDINGS {
                    view.bindings.push(binding);
                } else {
                    app.set_status(app.tr("render.material_limits").to_owned(), true);
                }
            }
        }
    }
}

fn saved_controls(app: &mut SolveApp, ui: &mut egui::Ui, binding: &mut Binding) -> bool {
    let saved = app
        .render_workspace
        .view
        .draft
        .optics
        .iter()
        .enumerate()
        .find(|(_, saved)| binding.target.stable_target.as_ref() == Some(&saved.target))
        .map(|(index, saved)| (index, saved.target.clone()));
    ui_kit::banner(ui, ui_kit::AssistantTone::Working, |ui| {
        note(
            app,
            ui,
            if saved.is_some() && !binding.active() { "render.appearance_editor.saved_state" } else { "render.appearance_editor.temporary" },
        );
    });
    let can_save = binding.glass_enabled
        && binding.current(app)
        && !binding.maps_enabled
        && !binding.coat_enabled
        && app.render_workspace.view.method == super::ImageMethod::GpuRaytrace;
    let mut removed = false;
    ui.horizontal(|ui| {
        let save = ui_kit::button(ui, app.tr("render.appearance_editor.save"), ui_kit::Button::Enabled(can_save));
        crate::scenario::publish_target("render:optics-save", save.rect);
        if save.clicked() {
            match save_optics(app, binding) {
                Ok(()) => removed = true,
                Err(error) => app.set_status(error, true),
            }
        }
        let remove = ui_kit::button(ui, app.tr("render.appearance_editor.remove"), ui_kit::Button::Enabled(saved.is_some()));
        crate::scenario::publish_target("render:optics-remove", remove.rect);
        if remove.clicked()
            && let Some((index, target)) = saved
        {
            crate::scenario::publish_target(&format!("render:optics-remove:{index}"), remove.rect);
            app.render_workspace.view.materials.pending_remove = Some(PendingRemoval {
                name: app.render_workspace.view.name.clone(),
                target,
                revision: app.session.revision,
                document: Arc::downgrade(&app.session.doc),
            });
        }
    });
    ui_kit::checkbox(ui, app.tr("render.appearance_editor.replace"), &mut binding.replace_saved_optics);
    if !can_save {
        note(app, ui, "render.appearance_editor.save_reason");
    }
    if !binding.current(app) {
        note(app, ui, "render.material_revalidate_needed");
    }
    if ui_kit::button(ui, app.tr("render.appearance_editor.revalidate"), ui_kit::Button::Normal).clicked()
        && let Err(error) = revalidate(app, binding)
    {
        app.set_status(error, true);
    }
    super::library::saved_surface_target_controls(app, ui, binding.target.stable_target.as_ref());
    removed
}

fn reflection_section(app: &mut SolveApp, ui: &mut egui::Ui, body: Option<&str>) {
    let reason = if body.is_none() {
        Some(app.tr("render.appearance_editor.body_reason").to_owned())
    } else if app.render_workspace.view.method != super::ImageMethod::CpuRaytrace {
        Some(app.tr("render.appearance_editor.cpu_reason").to_owned())
    } else {
        None
    };
    let title = app.tr("render.appearance_editor.reflections").to_owned();
    ui_kit::method_section(ui, "render:appearance:reflections", &title, "CPU", reason.as_deref(), |ui| {
        use solidcraft_engine::render::job::ReflectionMode;
        let name = body.unwrap_or_default();
        let mut mode = app
            .render_workspace
            .view
            .raytrace
            .reflections
            .as_ref()
            .and_then(|r| r.bodies.iter().find(|b| b.body == name))
            .map_or(0, |b| if b.mode == ReflectionMode::Mirror { 1 } else { 2 });
        let labels = ["render.cpu_reflection_off", "render.cpu_reflection_mirror", "render.cpu_reflection_rough"].map(|key| app.tr(key).to_owned());
        let body_label = app.tr("render.inspector_this_body").to_owned();
        ui_kit::row(ui, &body_label, |ui| {
            if ui_kit::segmented(
                ui,
                &mut mode,
                &labels.each_ref().map(String::as_str),
                ui_kit::Segments { compact: true, small: true, ..Default::default() },
            ) {
                let value = match mode {
                    1 => Some(ReflectionMode::Mirror),
                    2 => Some(ReflectionMode::RoughSpecular),
                    _ => None,
                };
                if let Err(error) = super::cpu::set_reflection(app, name, value) {
                    app.set_status(error, true);
                }
            }
        });
        if ui_kit::button(ui, app.tr("render.cpu_binding_confirm"), ui_kit::Button::Enabled(mode != 0)).clicked()
            && let Err(error) = super::cpu::revalidate(app, name)
        {
            app.set_status(error, true);
        }
        let title = app.tr("render.appearance_editor.reflection_sampling").to_owned();
        ui_kit::editor_section(ui, "render:appearance:reflection-sampling", &title, false, |ui| super::cpu::reflection_sampling_controls(app, ui));
    });
}

pub(super) fn confirmation_open(app: &SolveApp) -> bool {
    app.render_workspace.view.materials.pending_remove.is_some()
}

pub(super) fn confirmation(app: &mut SolveApp, ctx: &egui::Context) {
    let Some(pending) = app.render_workspace.view.materials.pending_remove.clone() else { return };
    if pending.revision != app.session.revision
        || pending.name != app.render_workspace.view.name
        || !Weak::ptr_eq(&pending.document, &Arc::downgrade(&app.session.doc))
    {
        app.render_workspace.view.materials.pending_remove = None;
        app.set_status(app.tr("render.review_stale").to_owned(), true);
        return;
    }
    let title = app.tr("render.appearance_editor.remove_title").to_owned();
    let (closed, action) = ui_kit::Confirm::new(egui::Id::new("render_remove_optics"), "delete", &title)
        .tone(ui_kit::Tone::Danger)
        .actions(app.tr("render.appearance_editor.remove"), app.tr("render.keep_editing"), None)
        .selected(ui_kit::Action::Cancel)
        .show(ctx, true, |ui| {
            note(app, ui, "render.appearance_editor.remove_note");
        });
    if closed || action == ui_kit::Action::Cancel {
        app.render_workspace.view.materials.pending_remove = None;
    }
    if action == ui_kit::Action::Ok {
        app.render_workspace.view.materials.pending_remove = None;
        match app.run("render.optics.unbind", json!({"name":pending.name,"target":&pending.target,"expected_revision":pending.revision})) {
            Ok(_) => {
                // Removing one saved binding must preserve unsaved shot settings
                // and unrelated temporary overrides on this tab.
                app.render_workspace.view.draft.optics.remove(&pending.target);
            }
            Err(error) => app.set_status(error, true),
        }
    }
}
