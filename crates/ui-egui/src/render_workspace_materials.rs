//! Shared body-level optical editing: transient request overlays and explicit
//! stable saved bindings. Engineering geometry/materials never change here.
use solidcore_record::Vec3;
use std::sync::{Arc, Weak};

use serde::Deserialize;
use serde_json::{Value, json};
use solidcraft_engine::{
    doc::{
        Document,
        appearance_pack::{self, GlassBoundary, Recipe},
    },
    render::{
        clear_coat::ClearCoat,
        job::textures::{BaseColorMode, PlanarFrame},
        surface::OpaqueSurface,
        transmission::Transmission,
    },
};

#[cfg(test)]
use crate::theme::Tokens;
use crate::{SolveApp, ui_kit};

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
    pub model: Option<solidcraft_engine::render::glass_model::GlassModel>,
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
        Self {
            model: None,
            recipe: None,
            ior: 1.5,
            absorption: [0.0; 3],
            boundary: Boundary::Closed,
            thickness: 1.0,
            shadows: false,
            subsurface: None,
        }
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
            model: None,
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

#[derive(Clone, Debug, PartialEq)]
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
    saved_source: Option<solidcraft_engine::doc::render_optics::OpticalMaterial>,
    saved_coat_source: Option<solidcraft_engine::doc::render_coat::CoatMaterial>,
    coat_edited: bool,
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
            saved_source: None,
            saved_coat_source: None,
            coat_edited: false,
        }
    }
    pub(super) fn from_saved(target: Target, app: &SolveApp, optics: solidcraft_engine::doc::render_optics::OpticalMaterial) -> Self {
        let mut binding = Self::new(target, app);
        binding.saved_source = Some(optics);
        binding.glass_enabled = true;
        binding.glass = Glass {
            model: optics.model,
            recipe: None,
            ior: optics.ior,
            absorption: optics.absorption_per_mm,
            boundary: match optics.boundary {
                solidcraft_engine::doc::render_optics::OpticalBoundary::ThinSheet => Boundary::Thin,
                solidcraft_engine::doc::render_optics::OpticalBoundary::ClosedBox => Boundary::Box,
                solidcraft_engine::doc::render_optics::OpticalBoundary::ClosedMesh => Boundary::Closed,
            },
            thickness: if optics.boundary == solidcraft_engine::doc::render_optics::OpticalBoundary::ThinSheet { optics.thickness_mm } else { 1.0 },
            shadows: optics.shadow_transmission,
            subsurface: optics.subsurface,
        };
        binding
    }
    fn hydrate_coat(&mut self, coat: solidcraft_engine::doc::render_coat::CoatMaterial) {
        self.saved_coat_source = Some(coat);
        self.coat_edited = false;
        self.coat_enabled = true;
        self.coat = Coat { weight: coat.weight, roughness: coat.roughness, ior: coat.ior };
    }
    fn coat_has_local_changes(&self) -> bool {
        self.coat_edited
            || if let Some(saved) = self.saved_coat_source {
                !self.enabled || !self.coat_enabled || self.glass_enabled || self.coat_material() != saved
            } else {
                self.coat_enabled
            }
    }
    fn has_local_map_intent(&self) -> bool {
        self.maps_enabled || self.maps != Maps::default()
    }
    fn coat_material(&self) -> solidcraft_engine::doc::render_coat::CoatMaterial {
        solidcraft_engine::doc::render_coat::CoatMaterial { weight: self.coat.weight, roughness: self.coat.roughness, ior: self.coat.ior }
    }
    fn optics(&self) -> solidcraft_engine::doc::render_optics::OpticalMaterial {
        solidcraft_engine::doc::render_optics::OpticalMaterial {
            model: self.glass.model,
            ior: self.glass.ior,
            absorption_per_mm: self.glass.absorption,
            boundary: match self.glass.boundary {
                Boundary::Thin => solidcraft_engine::doc::render_optics::OpticalBoundary::ThinSheet,
                Boundary::Box => solidcraft_engine::doc::render_optics::OpticalBoundary::ClosedBox,
                Boundary::Closed => solidcraft_engine::doc::render_optics::OpticalBoundary::ClosedMesh,
            },
            thickness_mm: if self.glass.boundary == Boundary::Thin { self.glass.thickness } else { 0.0 },
            shadow_transmission: self.glass.shadows,
            subsurface: self.glass.subsurface,
        }
    }
    fn current(&self, app: &SolveApp) -> bool {
        self.revision == app.session.revision && Weak::ptr_eq(&self.document, &Arc::downgrade(&app.session.doc)) && !app.session.model.is_stale()
    }
    fn active(&self) -> bool {
        if let Some(saved) = self.saved_source {
            // Saved edits become a prospective stable request overlay, never
            // an ordinal override or a silent return to the old saved values.
            return !self.enabled || !self.glass_enabled || self.coat_enabled || self.maps_enabled || self.optics() != saved;
        }
        if let Some(saved) = self.saved_coat_source {
            return !self.enabled || !self.coat_enabled || self.glass_enabled || self.maps_enabled || self.coat_material() != saved;
        }
        self.enabled && (self.glass_enabled || self.coat_enabled || self.maps_enabled)
    }
    pub(super) fn has_unsaved_coat_intent(&self) -> bool {
        // Unchanged saved coats never allocate a local binding on paint. A row
        // retained here carries a coat edit or independent local map draft.
        self.coat_enabled || self.saved_coat_source.is_some() || self.coat_edited
    }
    pub(super) fn has_runtime_intent(&self) -> bool {
        // Stable Glass uses the same prospective settings capture as coats.
        // Unknown identities and file maps retain the ordinal runtime route.
        if self.saved_source.is_none() && self.glass_enabled && self.target.stable_target.is_some() && !self.coat_enabled && !self.maps_enabled {
            return false;
        }
        if self.saved_coat_source.is_some() || self.coat_enabled || self.coat_edited {
            self.saved_source.is_none() && (self.glass_enabled || self.maps_enabled)
        } else {
            self.saved_source.is_none()
        }
    }
    fn stable_optics(&self) -> bool {
        self.saved_source.is_some() || (self.glass_enabled && self.target.stable_target.is_some())
    }
    pub(super) fn has_unsaved_optics_intent(&self) -> bool {
        self.stable_optics() && (self.saved_source.is_none() || self.active())
    }
}

#[derive(Clone, Debug, Default)]
pub(super) struct View {
    pub enabled: bool,
    pub bindings: Vec<Binding>,
    targets: Vec<Target>,
    #[cfg(test)]
    target: Option<usize>,
    #[cfg(test)]
    target_search: String,
    selected: Option<usize>,
    target_revision: Option<u64>,
    target_document: Option<Weak<Document>>,
    pending_remove: Option<PendingRemoval>,
}

#[derive(Clone, Debug)]
struct PendingRemoval {
    coat: bool,
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
    if (binding.coat_enabled || binding.saved_coat_source.is_some() || binding.coat_edited)
        && let Some(stable) = &binding.target.stable_target
    {
        let mut matches = targets.iter().filter(|target| target.stable_target.as_ref() == Some(stable));
        let target = matches.next().cloned().ok_or_else(|| app.tr("render.material_target_missing").to_string())?;
        if matches.next().is_some() {
            return Err(app.tr("render.material_target_missing").into());
        }
        binding.target = target;
    } else if !targets.contains(&binding.target) {
        return Err(app.tr("render.material_target_missing").to_string());
    }
    binding.revision = app.session.revision;
    binding.document = Arc::downgrade(&app.session.doc);
    Ok(())
}

/// Render settings may change document identity without changing the reviewed source.
/// A changed model, material, placement or target still needs explicit revalidation.
pub(super) fn refresh_render_revision(app: &mut SolveApp) {
    if app.render_workspace.view.materials.bindings.is_empty() {
        return;
    }
    let Ok(targets) = read_targets(app) else { return };
    let mut sources = super::setups::SourceCache::default();
    let eligible: Vec<bool> = app
        .render_workspace
        .view
        .materials
        .bindings
        .iter()
        .map(|binding| sources.same_model_source(app, &binding.document) && targets.contains(&binding.target))
        .collect();
    let saved = app.session.doc.render.setups.get(&app.render_workspace.view.name).map(|setup| &setup.coats);
    for (binding, eligible) in app.render_workspace.view.materials.bindings.iter_mut().zip(eligible) {
        if eligible {
            binding.revision = app.session.revision;
            binding.document = Arc::downgrade(&app.session.doc);
            // Stable source identity was proved above. Only unedited coat fields
            // may follow saved Undo/Redo/replacement; independent map drafts stay.
            if !binding.coat_has_local_changes() && !binding.glass_enabled {
                let current =
                    saved.and_then(|rows| rows.iter().find(|row| binding.target.stable_target.as_ref() == Some(&row.target))).map(|row| row.coat);
                if let Some(coat) = current {
                    binding.hydrate_coat(coat);
                } else if binding.saved_coat_source.is_some() {
                    binding.saved_coat_source = None;
                    binding.coat_enabled = false;
                }
            }
        }
    }
}

/// Overlay stable Glass drafts into a captured request only. Explicit binding
/// remains the sole save action; ordinal inputs never replace these rows.
pub(super) fn overlay_saved(app: &SolveApp, setup: &mut solidcraft_engine::doc::render_settings::RenderSetup) -> Result<(), String> {
    overlay_coats(app, setup)?;
    overlay_optics(app, setup)
}

fn overlay_optics(app: &SolveApp, setup: &mut solidcraft_engine::doc::render_settings::RenderSetup) -> Result<(), String> {
    let view = &app.render_workspace.view.materials;
    for binding in
        view.bindings.iter().filter(|binding| binding.stable_optics() && binding.active() && (view.enabled || binding.saved_source.is_some()))
    {
        if !binding.current(app) {
            return Err(app.tr("render.material_revalidate_needed").into());
        }
        let target = binding.target.stable_target.as_ref().ok_or_else(|| app.tr("render.material_target_missing").to_string())?;
        let baseline = app.render_workspace.view.draft.optics.iter().find(|saved| &saved.target == target).map(|saved| saved.optics);
        if baseline != binding.saved_source {
            return Err(app.tr("render.material_revalidate_needed").into());
        }
        let source = setup.optics.iter().find(|saved| &saved.target == target).map(|saved| saved.optics);
        let prospective = (binding.enabled && binding.glass_enabled).then(|| binding.optics());
        // An already-overlaid request is idempotent. A row in the actual draft
        // still needs its exact saved-source proof, even if its values coincide.
        if source != baseline && source != prospective {
            return Err(app.tr("render.material_revalidate_needed").into());
        }
        if binding.coat_enabled || binding.maps_enabled {
            return Err(app.tr("render.material_saved_edit_pending").into());
        }
        if !binding.enabled || !binding.glass_enabled {
            setup.optics.remove(target);
        } else {
            setup
                .optics
                .set(
                    solidcraft_engine::doc::render_optics::SavedOptics { target: target.clone(), optics: binding.optics() },
                    binding.saved_source.is_some(),
                )
                .map_err(|error| error.to_string())?;
        }
    }
    Ok(())
}

/// Stable coat edits use the same captured settings for CPU and GPU. This is a
/// pure overlay; generic Save still requires the explicit coat binding action.
pub(super) fn overlay_coats(app: &SolveApp, setup: &mut solidcraft_engine::doc::render_settings::RenderSetup) -> Result<(), String> {
    if !app.render_workspace.view.materials.enabled {
        return Ok(());
    }
    for binding in &app.render_workspace.view.materials.bindings {
        if !binding.coat_enabled && binding.saved_coat_source.is_none() && !binding.coat_edited {
            continue;
        }
        if !binding.current(app) {
            return Err(app.tr("render.material_revalidate_needed").into());
        }
        let target = binding.target.stable_target.as_ref().ok_or_else(|| app.tr("render.material_target_missing").to_string())?;
        if let Some(expected) = binding.saved_coat_source {
            let current = setup.coats.iter().find(|saved| &saved.target == target).map(|saved| saved.coat);
            if current != Some(expected) {
                return Err(app.tr("render.material_revalidate_needed").into());
            }
        } else if setup.coats.iter().any(|saved| &saved.target == target) {
            return Err(app.tr("render.coat_revalidate_saved").into());
        }
        if binding.glass_enabled && binding.coat_enabled {
            return Err(app.tr("render.material_glass_exclusive").into());
        }
        if !binding.enabled || !binding.coat_enabled {
            if binding.saved_coat_source.is_some() {
                setup.coats.remove(target);
            }
        } else {
            setup
                .coats
                .set(solidcraft_engine::doc::render_coat::SavedCoat { target: target.clone(), coat: binding.coat_material() }, true)
                .map_err(|error| error.to_string())?;
        }
    }
    Ok(())
}

/// Pure prospective compatibility: no request, resources, host or document mutation.
pub(super) fn path_compatibility(app: &SolveApp) -> Result<(), String> {
    // This is the same pure stable-target overlay used by image/live capture.
    // A user's explicit unsaved removal of an unsupported flag must already be
    // reflected by the availability of the transport it makes compatible.
    let mut prospective = solidcraft_engine::doc::render_settings::RenderSetup {
        optics: app.render_workspace.view.draft.optics.clone(),
        coats: app.render_workspace.view.draft.coats.clone(),
        ..Default::default()
    };
    overlay_saved(app, &mut prospective)?;
    prospective.optics.validate_for_cpu_path(true).map_err(|e| e.to_string())?;
    if app.render_workspace.view.studio.denoise && prospective.optics.iter().any(|b| b.optics.subsurface.is_some_and(|s| s.active())) {
        return Err(app.tr("render.cpu_subsurface_denoise_reason").into());
    }
    for binding in app.render_workspace.view.materials.bindings.iter().filter(|b| b.saved_source.is_none() && b.active() && b.glass_enabled) {
        if !binding.current(app) {
            return Err(app.tr("render.material_revalidate_needed").into());
        }
        binding.optics().validate_for_cpu_path(true).map_err(|e| e.to_string())?;
        if app.render_workspace.view.studio.denoise && binding.glass.subsurface.is_some_and(|s| s.active()) {
            return Err(app.tr("render.cpu_subsurface_denoise_reason").into());
        }
    }
    Ok(())
}
/// Called for every Check and Start through the one shared image request builder.
/// No files are read until the engine captures the actual request.
pub(super) fn params(app: &SolveApp, params: &mut Value) -> Result<(), String> {
    let view = &app.render_workspace.view.materials;
    // A file HDR can select this lane even when every Glass row is stable.
    // Preserve those rows in settings, not a second ordinal representation.
    if view.bindings.iter().any(|binding| binding.stable_optics() && binding.active() && (view.enabled || binding.saved_source.is_some())) {
        let mut settings = serde_json::from_value(params["settings"].clone()).map_err(|error| error.to_string())?;
        overlay_optics(app, &mut settings)?;
        params["settings"] = json!(settings);
    }
    if !view.enabled || !view.bindings.iter().any(|binding| binding.active() && !binding.stable_optics()) {
        return Ok(());
    }
    let cpu = app.render_workspace.view.method == super::ImageMethod::CpuRaytrace;
    if app.render_workspace.view.method == super::ImageMethod::Raster {
        return Err(app.tr("render.material_raytrace_required").to_string());
    }
    if cpu && !app.render_workspace.view.draft.optics.is_empty() {
        return Err(app.tr("render.material_cpu_saved_runtime_conflict").into());
    }
    if view.bindings.len() > MAX_BINDINGS {
        return Err(app.tr("render.material_limits").to_string());
    }
    let (mut glass, mut textures) = (Vec::new(), Vec::new());
    let mut seen = std::collections::BTreeSet::new();
    for binding in view.bindings.iter().filter(|b| b.active() && !b.stable_optics()) {
        if !binding.current(app) {
            return Err(app.tr("render.material_revalidate_needed").to_string());
        }
        if binding.saved_source.is_some() {
            return Err(app.tr("render.material_saved_edit_pending").into());
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
        if cpu && binding.maps_enabled {
            return Err(app.tr("render.material_cpu_glass_only").into());
        }
        let identity = || json!({"body_index":target.ordinal,"body_name":target.name});
        if cpu && binding.glass_enabled {
            let optical = binding.optics();
            glass.push(json!(
                optical
                    .to_cpu_for_path(target.ordinal, &target.name, app.render_workspace.view.raytrace.path.is_some())
                    .map_err(|error| error.to_string())?
            ));
        } else if binding.glass_enabled {
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
            if let Some(model) = value.model {
                model.validate(value.ior).map_err(|e| e.to_string())?;
                wire["model"] = json!(model);
            }
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
            // The stable coat overlay already captured these exact numbers.
            // Never duplicate it as an ordinal runtime override.
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
    if cpu {
        // Preserve all CPU lights, output, lens and sampling. This is the same
        // numerical override type produced by stable saved-optics lowering.
        params["raytrace"]["dielectrics"] = json!(glass);
        let options: solidcraft_engine::render::job::RaytraceOptions =
            serde_json::from_value(params["raytrace"].clone()).map_err(|error| error.to_string())?;
        options.validate().map_err(|error| error.to_string())?;
    } else {
        params["raytrace"] = json!({"dielectrics":glass,"textures":textures});
    }
    Ok(())
}

fn vector(app: &SolveApp, ui: &mut egui::Ui, key: &str, values: &mut [f64; 3], range: std::ops::RangeInclusive<f64>, suffix: &str) -> bool {
    ui_kit::row(ui, app.tr(key), |ui| ui_kit::vector3_field(ui, values, range, suffix))
}

#[cfg(test)]
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
            ui.scope_id().with("render_material_search"),
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
    let cpu = app.render_workspace.view.method == super::ImageMethod::CpuRaytrace;
    let path = app.render_workspace.view.raytrace.path.is_some();
    let enabled = ui_kit::checkbox(ui, app.tr("render.appearance_editor.glass_enable"), &mut binding.glass_enabled);
    crate::scenario::publish_target("render:glass:enable", enabled.rect);
    if enabled.changed() && binding.glass_enabled {
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
        changed |= advanced_glass_controls(app, ui, glass);
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
                if !cpu {
                    glass.shadows = glass.boundary == Boundary::Thin;
                }
                changed = true;
            }
        });
        changed |= ui_kit::row(ui, app.tr("render.appearance_editor.ior"), |ui| {
            let response = ui_kit::number_field(ui, &mut glass.ior, 1.0..=3.0, 0.01, "");
            crate::scenario::publish_target("render:glass:ior-field", response.rect);
            response
        })
        .changed();
        ui.add_enabled_ui(glass.boundary == Boundary::Thin, |ui| {
            changed |= ui_kit::row(ui, app.tr("render.appearance_editor.thickness"), |ui| {
                let response = ui_kit::number_field_preserving(ui, &mut glass.thickness, 0.0..=10000.0, 0.1, " mm");
                crate::scenario::publish_target("render:glass:thickness-field", response.rect);
                response
            })
            .changed();
        });
        let absorption = app.tr("render.appearance_editor.absorption").to_owned();
        // Each suffixed DragValue needs the full control column. Three cells
        // can expand a narrow inspector despite their individual clip rects.
        for ((axis, value), channel) in glass.absorption.iter_mut().enumerate().zip(["R", "G", "B"]) {
            changed |= ui_kit::row(ui, &format!("{absorption} {channel}"), |ui| {
                let field = ui_kit::number_field(ui, value, 0.0..=100.0, 0.01, " /mm");
                crate::scenario::publish_target(&format!("dialog:vector:{absorption}:{axis}"), field.rect);
                field
            })
            .changed();
        }
        ui.add_enabled_ui(!cpu || (path && glass.boundary == Boundary::Thin) || glass.shadows, |ui| {
            changed |= ui_kit::checkbox(ui, app.tr("render.material_shadow"), &mut glass.shadows).changed();
        });
        // Existing unsupported choices remain removable. Positive CPU scattering
        // requires the explicit closed-medium Path, never silent adoption.
        ui.add_enabled_ui((glass.boundary != Boundary::Thin && (!cpu || path)) || glass.subsurface.is_some(), |ui| {
            let mut scatter = glass.subsurface.is_some();
            let toggle = ui_kit::checkbox(ui, app.tr("render.subsurface_enable"), &mut scatter);
            crate::scenario::publish_target("render:glass:subsurface-enable", toggle.rect);
            if toggle.changed() {
                glass.subsurface =
                    scatter.then_some(solidcraft_engine::render::transmission::subsurface::Subsurface { scattering_per_mm: 0.1, anisotropy: 0.0 });
                changed = true;
            }
            if let Some(subsurface) = &mut glass.subsurface {
                ui.add_enabled_ui(!cpu || (path && glass.boundary != Boundary::Thin), |ui| {
                    changed |= ui_kit::row(ui, app.tr("render.subsurface_rate"), |ui| {
                        let field = ui_kit::number_field_preserving(ui, &mut subsurface.scattering_per_mm, 0.0..=100.0, 0.001, " /mm");
                        crate::scenario::publish_target("render:glass:subsurface-rate", field.rect);
                        field
                    })
                    .changed();
                    changed |= ui_kit::row(ui, app.tr("render.volume_anisotropy"), |ui| {
                        let field = ui_kit::number_field_preserving(ui, &mut subsurface.anisotropy, -0.95..=0.95, 0.01, "");
                        crate::scenario::publish_target("render:glass:subsurface-g", field.rect);
                        field
                    })
                    .changed();
                });
            }
        });
        if cpu {
            note(app, ui, "render.cpu_subsurface_help");
        }
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
    note(app, ui, if cpu { "render.cpu_material_certification" } else { "render.material_certification" });
}

fn advanced_glass_controls(app: &SolveApp, ui: &mut egui::Ui, glass: &mut Glass) -> bool {
    use solidcraft_engine::render::glass_model::{Dispersion, GlassModel, ThinFilm};
    let before = glass.model;
    let mut enabled = before.is_some();
    let toggle = ui_kit::checkbox(ui, app.tr("render.glass_model_enable"), &mut enabled);
    crate::scenario::publish_target("render:glass:model-enable", toggle.rect);
    if toggle.changed() {
        glass.model = enabled.then(GlassModel::default);
    }
    if let Some(model) = &mut glass.model {
        ui_kit::row(ui, app.tr("render.glass_roughness"), |ui| {
            let field = ui_kit::number_field_preserving(ui, &mut model.roughness, 0.0..=1.0, 0.01, "");
            crate::scenario::publish_target("render:glass:roughness", field.rect);
            field
        });
        let mut dispersion = model.dispersion.is_some();
        let toggle = ui_kit::checkbox(ui, app.tr("render.glass_dispersion"), &mut dispersion);
        crate::scenario::publish_target("render:glass:dispersion-enable", toggle.rect);
        if toggle.changed() {
            model.dispersion = dispersion.then_some(Dispersion { abbe_number: 50.0 });
        }
        if let Some(dispersion) = &mut model.dispersion {
            ui_kit::row(ui, app.tr("render.glass_abbe"), |ui| {
                let field = ui_kit::number_field_preserving(ui, &mut dispersion.abbe_number, 5.0..=200.0, 0.5, "");
                crate::scenario::publish_target("render:glass:abbe", field.rect);
                field
            });
        }
        let mut film = model.thin_film.is_some();
        let toggle = ui_kit::checkbox(ui, app.tr("render.glass_film"), &mut film);
        crate::scenario::publish_target("render:glass:film-enable", toggle.rect);
        if toggle.changed() {
            model.thin_film = film.then_some(ThinFilm { ior: 1.35, thickness_nm: 400.0 });
        }
        if let Some(film) = &mut model.thin_film {
            ui_kit::row(ui, app.tr("render.glass_film_ior"), |ui| {
                let field = ui_kit::number_field_preserving(ui, &mut film.ior, 1.0..=3.0, 0.01, "");
                crate::scenario::publish_target("render:glass:film-ior", field.rect);
                field
            });
            ui_kit::row(ui, app.tr("render.glass_film_thickness"), |ui| {
                let field = ui_kit::number_field_preserving(ui, &mut film.thickness_nm, 0.0..=100000.0, 1.0, " nm");
                crate::scenario::publish_target("render:glass:film-thickness", field.rect);
                field
            });
        }
        ui_kit::text(ui, app.tr("render.glass_model_help"), ui_kit::Text::Note("render.glass_model_help"));
    }
    before != glass.model
}

fn coat_controls(app: &mut SolveApp, ui: &mut egui::Ui, binding: &mut Binding) -> bool {
    let enabled = ui_kit::checkbox(ui, app.tr("render.material_coat_enable"), &mut binding.coat_enabled);
    crate::scenario::publish_target("render:coat:enable", enabled.rect);
    let mut changed = enabled.changed();
    ui.add_enabled_ui(binding.coat_enabled, |ui| {
        for (key, value, range) in [
            ("render.appearance_editor.weight", &mut binding.coat.weight, 0.0..=1.0),
            ("appearance.roughness", &mut binding.coat.roughness, 0.045..=1.0),
        ] {
            changed |= ui_kit::row(ui, app.tr(key), |ui| {
                let response = ui_kit::mixed_slider(ui, value, range, None);
                crate::scenario::publish_target(
                    if key == "render.appearance_editor.weight" { "render:coat:weight" } else { "render:coat:roughness" },
                    response.rect,
                );
                response
            })
            .changed();
        }
        changed |= ui_kit::row(ui, app.tr("render.appearance_editor.ior"), |ui| {
            let response = ui_kit::number_field_preserving(ui, &mut binding.coat.ior, 1.0..=3.0, 0.01, "");
            crate::scenario::publish_target("render:coat:ior-field", response.rect);
            response
        })
        .changed();
    });
    binding.coat_edited |= changed;
    saved_coat_controls(app, ui, binding)
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
pub(super) fn install_library_surface(
    app: &mut SolveApp,
    id: &str,
    target: Target,
    plane: &str,
    scale: f64,
    rotation_rad: f64,
) -> Result<(), String> {
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
    let reply = app.run(
        "render.library.surface.prepare",
        json!({"id":id,"body_index":target.ordinal,"body_name":target.name,"plane":plane,"scale":scale,"rotation_rad":rotation_rad}),
    )?;
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
    if binding.saved_coat_source.is_none()
        && !binding.coat_has_local_changes()
        && let Some(saved) = app.render_workspace.view.draft.coats.iter().find(|saved| binding.target.stable_target.as_ref() == Some(&saved.target))
    {
        binding.hydrate_coat(saved.coat);
    }
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
        vector(app, ui, "render.appearance_editor.origin", &mut binding.maps.frame.origin, -1e9..=1e9, " mm");
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

fn save_coat(app: &mut SolveApp, binding: &Binding) -> Result<(), String> {
    if !binding.current(app) {
        return Err(app.tr("render.material_revalidate_needed").into());
    }
    let saved = app
        .session
        .doc
        .render
        .setups
        .get(&app.render_workspace.view.name)
        .and_then(|setup| setup.coats.iter().find(|saved| binding.target.stable_target.as_ref() == Some(&saved.target)))
        .map(|saved| saved.coat);
    if saved != binding.saved_coat_source {
        return Err(app.tr("render.coat_revalidate_saved").into());
    }
    let mut local = std::mem::take(&mut app.render_workspace.view.materials);
    let preserve_maps = binding.has_local_map_intent();
    if preserve_maps && !local.bindings.iter().any(|old| old.target == binding.target) {
        if local.bindings.len() >= MAX_BINDINGS {
            app.render_workspace.view.materials = local;
            return Err(app.tr("render.material_limits").into());
        }
        let count = local.bindings.len() + 1;
        let mut reserved = Vec::new();
        if reserved.try_reserve_exact(count).is_err() || reserved.capacity() != count {
            app.render_workspace.view.materials = local;
            return Err(app.tr("render.error_binding_allocation").into());
        }
        reserved.append(&mut local.bindings);
        local.bindings = reserved;
    }
    let setup = super::setups::snapshot(app);
    app.render_workspace.view.materials = local;
    let mut setup = setup?;
    let name = app.render_workspace.view.name.clone();
    if let Some(saved) = app.session.doc.render.setups.get(&name) {
        setup.camera = saved.camera;
    }
    app.run(
        "render.coat.bind",
        json!({"name":name,"set":setup,"body_index":binding.target.ordinal,"body_name":binding.target.name,
        "coat":binding.coat_material(),"replace":binding.replace_saved_optics,"expected_revision":app.session.revision}),
    )?;
    let mut local = std::mem::take(&mut app.render_workspace.view.materials);
    super::setups::load(app);
    if preserve_maps {
        let mut retained = binding.clone();
        retained.saved_coat_source = Some(binding.coat_material());
        retained.coat_edited = false;
        retained.revision = app.session.revision;
        retained.document = Arc::downgrade(&app.session.doc);
        if let Some(old) = local.bindings.iter_mut().find(|old| old.target == binding.target) {
            *old = retained;
        } else {
            // This exact slot was fallibly reserved before the document command.
            local.bindings.push(retained);
        }
    }
    app.render_workspace.view.materials = local;
    Ok(())
}

fn saved_coat_controls(app: &mut SolveApp, ui: &mut egui::Ui, binding: &mut Binding) -> bool {
    let saved =
        binding.target.stable_target.as_ref().and_then(|target| app.render_workspace.view.draft.coats.iter().find(|saved| &saved.target == target));
    let target = saved.map(|saved| saved.target.clone());
    note(app, ui, "render.coat_saved_help");
    if !binding.current(app) {
        note(app, ui, "render.material_revalidate_needed");
        let response = ui_kit::button(ui, app.tr("render.appearance_editor.revalidate"), ui_kit::Button::Normal);
        crate::scenario::publish_target("render:coat:revalidate", response.rect);
        if response.clicked()
            && let Err(error) = revalidate(app, binding)
        {
            app.set_status(error, true);
        }
    }
    let can_save = binding.enabled
        && binding.coat_enabled
        && !binding.glass_enabled
        && binding.current(app)
        && binding.target.stable_target.is_some()
        && (target.is_some() || app.render_workspace.view.draft.coats.len() < solidcraft_engine::doc::render_coat::MAX_COAT_BINDINGS)
        && matches!(app.render_workspace.view.method, super::ImageMethod::CpuRaytrace | super::ImageMethod::GpuRaytrace);
    let mut discard = false;
    ui.horizontal(|ui| {
        let save = ui_kit::button(ui, app.tr("render.coat_save"), ui_kit::Button::Enabled(can_save));
        crate::scenario::publish_target("render:coat:save", save.rect);
        if save.clicked() {
            match save_coat(app, binding) {
                Ok(()) => {
                    binding.saved_coat_source = Some(binding.coat_material());
                    binding.coat_edited = false;
                    binding.revision = app.session.revision;
                    binding.document = Arc::downgrade(&app.session.doc);
                    discard = !binding.has_local_map_intent();
                }
                Err(error) => app.set_status(error, true),
            }
        }
        let remove = ui_kit::button(ui, app.tr("render.coat_remove"), ui_kit::Button::Enabled(target.is_some()));
        crate::scenario::publish_target("render:coat:remove", remove.rect);
        if remove.clicked()
            && let Some(target) = target
        {
            app.render_workspace.view.materials.pending_remove = Some(PendingRemoval {
                coat: true,
                name: app.render_workspace.view.name.clone(),
                target,
                revision: app.session.revision,
                document: Arc::downgrade(&app.session.doc),
            });
        }
    });
    let replace = ui_kit::checkbox(ui, app.tr("render.appearance_editor.replace"), &mut binding.replace_saved_optics);
    crate::scenario::publish_target("render:coat:replace", replace.rect);
    if binding.coat_has_local_changes() {
        note(app, ui, "render.coat_pending_edit");
        let response = ui_kit::button(ui, app.tr("render.coat_discard"), ui_kit::Button::Normal);
        crate::scenario::publish_target("render:coat:discard", response.rect);
        if response.clicked() {
            let current = app
                .session
                .doc
                .render
                .setups
                .get(&app.render_workspace.view.name)
                .and_then(|setup| setup.coats.iter().find(|saved| binding.target.stable_target.as_ref() == Some(&saved.target)))
                .map(|saved| saved.coat);
            if let Some(saved) = current {
                binding.hydrate_coat(saved);
            } else {
                binding.saved_coat_source = None;
                binding.coat_enabled = false;
                binding.coat_edited = false;
            }
            binding.enabled = true;
            discard |= !binding.has_local_map_intent();
        }
    } else if binding.saved_coat_source.is_some() {
        note(app, ui, "render.appearance_editor.saved_state");
    }
    discard
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
    let mut setup = setup?;
    let value = &binding.glass;
    let name = app.render_workspace.view.name.clone();
    if let Some(saved) = app.session.doc.render.setups.get(&name) {
        // Saving optical numbers is not the shot's explicit camera update action.
        setup.camera = saved.camera;
    }
    let optics = json!({"ior":value.ior,"absorption_per_mm":value.absorption,"boundary":value.boundary.wire(),
        "thickness_mm":if value.boundary==Boundary::Thin {value.thickness} else {0.0},"shadow_transmission":value.shadows,"subsurface":value.subsurface,"model":value.model});
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
    let glass_reason = if !supported {
        reason.clone()
    } else if app.render_workspace.view.method == super::ImageMethod::Raster {
        Some(app.tr("render.material_raytrace_required").to_owned())
    } else {
        None
    };
    ui_kit::method_section(ui, "render:appearance:glass", &title, "CPU / GPU", glass_reason.as_deref(), |ui| glass_controls(app, ui, binding));
    if app.render_workspace.view.method == super::ImageMethod::CpuRaytrace {
        note(app, ui, "render.material_cpu_optics_limits");
    }
    let opaque_reason = reason.clone().or_else(|| binding.glass_enabled.then(|| app.tr("render.material_glass_exclusive").to_owned()));
    let title = app.tr("render.appearance_editor.coat").to_owned();
    let coat_reason = if !supported {
        Some(app.tr("render.appearance_editor.body_reason").to_owned())
    } else if app.render_workspace.view.method == super::ImageMethod::Raster {
        Some(app.tr("render.material_raytrace_required").to_owned())
    } else {
        binding.glass_enabled.then(|| app.tr("render.material_glass_exclusive").to_owned())
    };
    let coat_removed =
        ui_kit::method_section(ui, "render:appearance:coat", &title, "CPU / GPU", coat_reason.as_deref(), |ui| coat_controls(app, ui, binding))
            .unwrap_or(false);
    let title = app.tr("render.appearance_editor.maps").to_owned();
    let map_reason = if supported && app.render_workspace.view.method == super::ImageMethod::CpuRaytrace {
        Some(app.tr("render.cpu_maps_saved_help").to_owned())
    } else {
        opaque_reason.clone()
    };
    ui_kit::method_section(ui, "render:appearance:maps", &title, "GPU", map_reason.as_deref(), |ui| map_controls(app, ui, binding));
    let title = app.tr("render.appearance_editor.saved").to_owned();
    let removed = ui_kit::editor_section(ui, "render:appearance:saved", &title, false, |ui| {
        ui.add_enabled_ui(supported, |ui| saved_controls(app, ui, binding)).inner
    })
    .unwrap_or(false);
    reflection_section(app, ui, supported.then_some(binding.target.name.as_str()));
    removed || coat_removed
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
            let saved = app.render_workspace.view.draft.optics.iter().find(|saved| target.stable_target.as_ref() == Some(&saved.target));
            let mut hydrated =
                saved.map_or_else(|| Binding::new(target.clone(), app), |saved| Binding::from_saved(target.clone(), app, saved.optics));
            if let Some(saved) = app.render_workspace.view.draft.coats.iter().find(|saved| target.stable_target.as_ref() == Some(&saved.target)) {
                hydrated.hydrate_coat(saved.coat);
            }
            binding = Some(hydrated);
        }
    }
    let before = app.session.revision;
    let original_target = binding.as_ref().map(|binding| binding.target.clone());
    let removed = editor_sections(app, ui, binding.as_mut());
    if let Some(binding) = binding {
        let view = &mut app.render_workspace.view.materials;
        // Explicit stable revalidation may update a captured ordinal. Replace
        // the actual edited row instead of retaining an obsolete duplicate.
        let existing = view.bindings.iter().position(|b| b.target == binding.target || Some(&b.target) == original_target.as_ref());
        if removed {
            if let Some(index) = existing {
                view.bindings.remove(index);
            }
        } else if app.session.revision == before {
            // Opening the editor alone never allocates an inactive assignment.
            if (binding.saved_source.is_some() || (binding.saved_coat_source.is_some() && !binding.has_local_map_intent())) && !binding.active() {
                if let Some(index) = existing {
                    view.bindings.remove(index);
                }
            } else if binding.glass_enabled || binding.coat_enabled || binding.maps_enabled || existing.is_some() {
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
            if ((saved.is_some() && binding.saved_source.is_some()) || binding.saved_coat_source.is_some()) && !binding.active() {
                "render.appearance_editor.saved_state"
            } else {
                "render.appearance_editor.temporary"
            },
        );
    });
    let can_save = binding.enabled
        && binding.glass_enabled
        && binding.current(app)
        && !binding.maps_enabled
        && !binding.coat_enabled
        && matches!(app.render_workspace.view.method, super::ImageMethod::CpuRaytrace | super::ImageMethod::GpuRaytrace);
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
                coat: false,
                name: app.render_workspace.view.name.clone(),
                target,
                revision: app.session.revision,
                document: Arc::downgrade(&app.session.doc),
            });
        }
    });
    if binding.saved_source.is_some() && binding.active() {
        note(app, ui, "render.material_saved_edit_pending");
        let discard = ui_kit::button(ui, app.tr("render.appearance_editor.discard_saved_edit"), ui_kit::Button::Normal);
        crate::scenario::publish_target("render:optics-discard-edit", discard.rect);
        removed |= discard.clicked();
    }
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
    let title = app.tr(if pending.coat { "render.coat_remove_title" } else { "render.appearance_editor.remove_title" }).to_owned();
    let (closed, action) = ui_kit::Confirm::new(egui::Id::unique("render_remove_optics"), "delete", &title)
        .tone(ui_kit::Tone::Danger)
        .actions(app.tr("render.appearance_editor.remove"), app.tr("render.keep_editing"), None)
        .selected(ui_kit::Action::Cancel)
        .show(ctx, true, |ui| {
            note(app, ui, if pending.coat { "render.coat_remove_note" } else { "render.appearance_editor.remove_note" });
        });
    if closed || action == ui_kit::Action::Cancel {
        app.render_workspace.view.materials.pending_remove = None;
    }
    if action == ui_kit::Action::Ok {
        app.render_workspace.view.materials.pending_remove = None;
        let command = if pending.coat { "render.coat.unbind" } else { "render.optics.unbind" };
        match app.run(command, json!({"name":pending.name,"target":&pending.target,"expected_revision":pending.revision})) {
            Ok(_) => {
                // Removing one saved binding must preserve unsaved shot settings
                // and unrelated temporary overrides on this tab.
                if pending.coat {
                    app.render_workspace.view.draft.coats.remove(&pending.target);
                    let revision = app.session.revision;
                    let document = Arc::downgrade(&app.session.doc);
                    app.render_workspace.view.materials.bindings.retain_mut(|binding| {
                        if binding.target.stable_target.as_ref() != Some(&pending.target) {
                            return true;
                        }
                        if !binding.has_local_map_intent() {
                            return false;
                        }
                        binding.saved_coat_source = None;
                        binding.coat_enabled = false;
                        binding.coat_edited = false;
                        binding.revision = revision;
                        binding.document = document.clone();
                        true
                    });
                } else {
                    app.render_workspace.view.draft.optics.remove(&pending.target);
                }
            }
            Err(error) => app.set_status(error, true),
        }
    }
}
