//! Existing Render-panel library browser. Engine commands own catalog resolution,
//! assignments and saved resources; this view owns only search and request drafts.
#[path = "render_workspace_library_review.rs"]
pub(super) mod review;

use super::{ImageMethod, materials, setups};
use crate::{SolveApp, theme::Tokens, ui_kit};
use serde_json::{Value, json};
use solidcraft_engine::{doc::Document, geom::Vec3};
use std::{
    collections::BTreeMap,
    sync::{Arc, Weak},
};

#[derive(Clone, Debug)]
pub(super) struct View {
    kind: String,
    query: String,
    entries: Vec<Value>,
    selected: Option<String>,
    details: Option<Value>,
    loaded: bool,
    offset: usize,
    total: usize,
    next: Option<usize>,
    target_query: String,
    targets: Vec<materials::Target>,
    target: Option<materials::Target>,
    picked_target: Option<String>,
    target_revision: Option<u64>,
    target_document: Option<Weak<Document>>,
    pub(super) review: Option<review::Draft>,
    removal: Option<review::Removal>,
    look_tab: usize,
    light_scope: usize,
    physical_name: String,

    center: [f64; 3],
    extent: f64,
    availability: String,
    collection: usize,
    collection_query: String,
    navigation: BTreeMap<String, Navigation>,
    category: String,
    layout: usize,
    revision: Option<u64>,
    document: Option<Weak<Document>>,
    labels: BTreeMap<(String, String), String>,
    statuses: BTreeMap<(String, String), String>,
    preview: Option<SourcePreview>,
    preview_error: Option<String>,
    mapping_plane: usize,
    mapping_scale: f64,
}
#[derive(Clone, Debug)]
struct Navigation {
    query: String,
    offset: usize,
    selected: Option<String>,
    details: Option<Value>,
    availability: String,
}
#[derive(Clone)]
struct SourcePreview {
    id: String,
    sha256: String,
    texture: egui::TextureHandle,
}
impl std::fmt::Debug for SourcePreview {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SourcePreview").field("id", &self.id).field("sha256", &self.sha256).finish()
    }
}
impl Default for View {
    fn default() -> Self {
        Self {
            kind: "appearances".into(),
            query: String::new(),
            entries: Vec::new(),
            selected: None,
            details: None,
            loaded: false,
            offset: 0,
            total: 0,
            next: None,
            target_query: String::new(),
            targets: Vec::new(),
            target: None,
            picked_target: None,
            target_revision: None,
            target_document: None,
            review: None,
            removal: None,
            look_tab: 0,
            light_scope: 0,
            physical_name: "Reference material".into(),

            center: [0.0; 3],
            extent: 1000.0,
            availability: "all".into(),
            collection: 0,
            collection_query: String::new(),
            navigation: Default::default(),
            category: String::new(),
            layout: 0,
            revision: None,
            document: None,
            labels: Default::default(),
            statuses: Default::default(),
            preview: None,
            preview_error: None,
            mapping_plane: 0,
            mapping_scale: 1.0,
        }
    }
}

fn refresh(app: &mut SolveApp, view: &mut View) -> Result<(), String> {
    if matches!(view.collection, 1 | 2) {
        let keys: Vec<_> = if view.collection == 1 {
            app.preferences.render_library_favorites.iter().take(256).cloned().collect()
        } else {
            app.preferences.render_library_recent.iter().take(32).cloned().collect()
        };
        view.entries = keys
            .into_iter()
            .filter_map(|(kind, id)| {
                let detail = describe(app, &kind, &id).ok()?;
                let mut entry = detail["resource"].clone();
                entry["resource"] = detail["resource"].clone();
                entry["projection"] = detail["projection"].clone();
                entry["library_kind"] = json!(kind);
                entry["catalog_identity"] = json!(id);
                Some(entry)
            })
            .collect();
        view.total = view.entries.len();
        view.next = None;
    } else if view.collection == 3 || matches!(view.kind.as_str(), "appearances" | "selene_appearances" | "materials") && view.look_tab == 0 {
        let reply = app.run(if view.collection == 3 { "appearance.list" } else { "appearance.library" }, json!({}))?;
        view.entries = reply[if view.collection == 3 { "in_design" } else { "appearances" }].as_array().cloned().unwrap_or_default();
        for entry in &mut view.entries {
            entry["library_kind"] = json!("appearances");
        }
        if view.collection == 0 {
            for kind in ["selene_appearances", "materials"] {
                let reply = app.run("render.library.list", json!({"kind":kind,"limit":128}))?;
                for mut entry in reply["items"].as_array().cloned().unwrap_or_default() {
                    let detail = app.run("render.library.describe", json!({"kind":kind,"id":entry["id"]}))?;
                    entry["resource"] = detail["resource"].clone();
                    entry["projection"] = detail["projection"].clone();
                    entry["library_kind"] = json!(kind);
                    view.entries.push(entry);
                }
            }
        }
        view.total = view.entries.len();
        view.next = None;
    } else if view.light_scope == 3 && matches!(view.kind.as_str(), "scenes" | "selene_shots" | "lighting" | "environments") {
        view.entries.clear();
        for kind in ["scenes", "selene_shots", "lighting", "environments"] {
            let reply = app.run("render.library.list", json!({"kind":kind,"query":view.query,"limit":128}))?;
            for mut entry in reply["items"].as_array().cloned().unwrap_or_default() {
                entry["library_kind"] = json!(kind);
                view.entries.push(entry);
            }
        }
        view.total = view.entries.len();
        view.next = None;
    } else {
        let reply = app.run("render.library.list", json!({"kind":view.kind,"query":view.query,"offset":view.offset,"limit":48,"availability":if view.kind=="physical_profiles" {"all"} else {&view.availability}}))?;
        view.entries = reply["items"].as_array().cloned().ok_or_else(|| crate::i18n::tr("render.error_library_reply").to_string())?;
        // Capture authored optical parameters once per page; selection remains read-only.
        for entry in &mut view.entries {
            if let Some(id) = entry["id"].as_str() {
                let detail = app.run("render.library.describe", json!({"kind":view.kind,"id":id}))?;
                entry["resource"] = detail["resource"].clone();
            }
        }
        view.total = reply["total"].as_u64().and_then(|n| usize::try_from(n).ok()).unwrap_or(0);
        view.next = reply["next_offset"].as_u64().and_then(|n| usize::try_from(n).ok());
    }
    view.revision = Some(app.session.revision);
    view.document = Some(Arc::downgrade(&app.session.doc));
    view.loaded = true;
    // Search/page changes preserve the independently inspected resource.
    Ok(())
}
fn current_target(app: &SolveApp, view: &View) -> bool {
    view.target.is_some()
        && view.target_revision == Some(app.session.revision)
        && view.target_document.as_ref().is_some_and(|d| Weak::ptr_eq(d, &Arc::downgrade(&app.session.doc)))
        && !app.session.model.is_stale()
}
fn refresh_targets(app: &mut SolveApp, view: &mut View) -> Result<(), String> {
    let reply = app.run("render.library.targets", json!({"query":view.target_query,"limit":128}))?;
    view.targets = serde_json::from_value(reply["items"].clone()).map_err(|e| e.to_string())?;
    view.target = None;
    view.target_revision = Some(app.session.revision);
    view.target_document = Some(Arc::downgrade(&app.session.doc));
    if let (Ok(min), Ok(max)) =
        (serde_json::from_value::<Vec3>(reply["bounds"]["min"].clone()), serde_json::from_value::<Vec3>(reply["bounds"]["max"].clone()))
    {
        let c = (min + max) * 0.5;
        let size = max - min;
        let extent = size.x.max(size.y).max(size.z);
        if extent.is_finite() && (0.001..=1e6).contains(&extent) {
            view.center = [c.x, c.y, c.z];
            view.extent = extent;
        }
    }
    Ok(())
}
fn target_controls(app: &mut SolveApp, ui: &mut egui::Ui, view: &mut View) {
    ui.label(app.tr("render.library_target"));
    ui_kit::bounded_cell_editor(
        ui,
        ui.id().with("render_library_target_search"),
        &mut view.target_query,
        app.tr("render.material_filter"),
        false,
        128,
    );
    if ui_kit::button(ui, app.tr("render.material_refresh"), ui_kit::Button::Normal).clicked()
        && let Err(e) = refresh_targets(app, view)
    {
        app.set_status(e, true);
    }
    let label = view.target.as_ref().map_or_else(|| app.tr("render.material_choose_body").to_string(), |t| t.name.clone());
    ui_kit::dropdown_menu(ui, "render_library_target", &label, |ui| {
        egui::ScrollArea::vertical().max_height(200.0).show(ui, |ui| {
            for target in &view.targets {
                if ui_kit::list_picker(ui, &format!("library-target:{}", target.ordinal), &target.name, view.target.as_ref() == Some(target))
                    .clicked()
                {
                    view.target = Some(target.clone());
                    ui.close();
                }
            }
        });
    });
    if view.target.is_some() && !current_target(app, view) {
        ui_kit::text(ui, app.tr("render.material_revalidate_needed"), ui_kit::Text::Note("render.material_revalidate_needed"));
    }
}
fn patch(app: &SolveApp) -> Result<Value, String> {
    serde_json::to_value(setups::snapshot(app)?).map_err(|e| e.to_string())
}

const KINDS: &[(&str, &str)] = &[
    ("appearances", "render.appearances"),
    ("native_scenes", "render.library_native_scenes"),
    ("surfaces", "render.library_surfaces"),
    ("hdr", "render.library_hdr"),
    ("rigs", "render.library_rigs"),
    ("selene_appearances", "render.library_selene"),
    ("physical_profiles", "render.library_physical_profiles"),
    ("scenes", "render.library_scenes"),
    ("selene_shots", "render.library_selene_shots"),
    ("materials", "render.library_material_concepts"),
    ("lighting", "render.library_light_concepts"),
    ("environments", "render.library_environment_concepts"),
    ("quality", "render.library_quality_concepts"),
    ("capabilities", "render.library_capabilities"),
];

const LOOK_KINDS: &[&str] = &["appearances", "materials", "selene_appearances"];

fn switch_kind(view: &mut View, kind: &str) {
    if view.kind == kind || !KINDS.iter().any(|(value, _)| *value == kind) {
        return;
    }
    view.navigation.insert(
        view.kind.clone(),
        Navigation {
            query: view.query.clone(),
            offset: view.offset,
            selected: view.selected.clone(),
            details: view.details.clone(),
            availability: view.availability.clone(),
        },
    );
    view.kind = kind.into();
    if let Some(saved) = view.navigation.get(kind) {
        view.query = saved.query.clone();
        view.offset = saved.offset;
        view.selected = saved.selected.clone();
        view.details = saved.details.clone();
        view.availability = saved.availability.clone();
    } else {
        view.query.clear();
        view.offset = 0;
        view.selected = None;
        view.details = None;
        view.availability = "all".into();
    }
    view.loaded = false;
    view.category.clear();
    view.preview = None;
    view.preview_error = None;
}
pub(super) fn open_category(app: &mut SolveApp, kind: &str) {
    // Explicit catalog browsing retains read-only Inspector workflows on the library stage.
    let stage = match kind {
        "native_scenes" | "rigs" | "hdr" | "scenes" | "selene_shots" | "lighting" | "environments" => "light",
        "quality" | "capabilities" => "output",
        _ => "look",
    };
    let page = app.render_workspace.view.inspector.page;
    let _ = app.run("view.render_stage", json!({"stage":stage}));
    app.render_workspace.view.inspector.page = page;
    switch_kind(&mut app.render_workspace.view.library, kind);
    let view = &mut app.render_workspace.view.library;
    view.collection = 0;
    view.look_tab = usize::from(matches!(kind, "surfaces" | "physical_profiles"));
    view.light_scope = match kind {
        "rigs" => 1,
        "hdr" | "environments" => 2,
        "scenes" | "selene_shots" | "lighting" => 3,
        _ => 0,
    };
    app.ui.show_browser = true;
    let _ = app.run("view.render_browser", json!({"open":true}));
    app.tree.panel_collapsed = false;
}
pub(super) fn choose(app: &mut SolveApp, view: &mut View, kind: &str, id: &str) -> Result<(), String> {
    switch_kind(view, kind);
    let detail = describe(app, kind, id)?;
    let display = detail["resource"]["name"].as_str().or_else(|| detail["resource"]["designation"].as_str()).unwrap_or(id).to_string();
    let status = detail["projection"]["status"].as_str().unwrap_or("reference_only").to_string();
    view.details = Some(detail);
    view.selected = Some(id.into());
    view.preview = None;
    view.preview_error = None;
    let key = (kind.to_string(), id.to_string());
    app.preferences.render_library_recent.retain(|old| old != &key);
    app.preferences.render_library_recent.push_front(key.clone());
    app.preferences.render_library_recent.truncate(32);
    if matches!(view.collection, 1 | 2) {
        view.loaded = false;
    }
    view.statuses.insert(key.clone(), status);
    view.labels.insert(key, display);
    view.labels.retain(|key, _| app.preferences.render_library_favorites.contains(key) || app.preferences.render_library_recent.contains(key));
    view.statuses.retain(|key, _| app.preferences.render_library_favorites.contains(key) || app.preferences.render_library_recent.contains(key));
    app.render_workspace.view.inspector.page = match kind {
        "native_scenes" | "scenes" | "selene_shots" => super::inspector::Page::Scene,
        "hdr" | "environments" => super::inspector::Page::Environment,
        "rigs" | "lighting" => super::inspector::Page::Lighting,
        "quality" => super::inspector::Page::Output,
        "capabilities" => super::inspector::Page::Diagnostics,
        _ => super::inspector::Page::Appearance,
    };
    Ok(())
}

fn choose_in_stage(app: &mut SolveApp, view: &mut View, kind: &str, id: &str) -> Result<(), String> {
    // A stage scope can combine several catalog kinds. Inspecting a tile keeps
    // that scope's search and category, rather than navigating to another catalog.
    let query = view.query.clone();
    let category = view.category.clone();
    let offset = view.offset;
    choose(app, view, kind, id)?;
    view.query = query;
    view.category = category;
    view.offset = offset;
    Ok(())
}

/// Resolve previews and Inspector metadata through the same public catalog commands.
fn look_signature(entry: &Value) -> Value {
    json!([entry["name"], entry["color"], entry["opacity"], entry["pbr"]])
}
pub(super) fn appearance_id(entry: &Value) -> String {
    entry["id"].as_str().map(str::to_owned).unwrap_or_else(|| look_signature(entry).to_string())
}
fn describe(app: &mut SolveApp, kind: &str, id: &str) -> Result<Value, String> {
    if kind != "appearances" {
        return app.run("render.library.describe", json!({"kind":kind,"id":id}));
    }
    for (command, field) in [("appearance.library", "appearances"), ("appearance.list", "in_design")] {
        let reply = app.run(command, json!({}))?;
        if let Some(resource) = reply[field].as_array().and_then(|items| items.iter().find(|item| appearance_id(item) == id)) {
            return Ok(
                json!({"resource":resource,"projection":{"status":if resource["available"] == false { "blocked" } else { "native_appearance" }}}),
            );
        }
    }
    // Custom looks carry their complete scalar definition in their preference key,
    // so a favourite survives closing its originating design.
    if let Ok(Value::Array(parts)) = serde_json::from_str::<Value>(id)
        && parts.len() == 4
    {
        return Ok(
            json!({"resource":{"name":parts.first(),"color":parts.get(1),"opacity":parts.get(2),"pbr":parts.get(3)},"projection":{"status":"native_appearance"}}),
        );
    }
    Err("appearance is no longer available".into())
}

fn preview_parameters(entry: &Value) -> Value {
    let source = entry.get("resource").unwrap_or(entry);
    if let Some(parameters) = source.pointer("/baseline_projection/runtime_parameters") {
        return parameters.clone();
    }
    if let (Some(low), Some(high)) = (source["base_linear_low"].as_array(), source["base_linear_high"].as_array()) {
        let color: Vec<_> = low
            .iter()
            .zip(high)
            .take(3)
            .map(|(a, b)| {
                let linear = ((a.as_f64().unwrap_or(0.0) + b.as_f64().unwrap_or(0.0)) * 0.5).clamp(0.0, 1.0);
                let encoded = if linear <= 0.0031308 { linear * 12.92 } else { 1.055 * linear.powf(1.0 / 2.4) - 0.055 };
                (encoded * 255.0).round() as u8
            })
            .collect();
        return json!({"color":color,"opacity":1.0,"pbr":{"metallic":source["metallic"],"roughness":source["roughness"],"dielectric_ior":1.5}});
    }
    source.clone()
}
pub(super) fn tile_sphere(entry: &Value) -> ui_kit::Sphere {
    let p = preview_parameters(entry);
    let color = crate::dialogs_appearance::parse_color(&p["color"]).map(|[r, g, b]| egui::Color32::from_rgb(r, g, b)).unwrap_or(Tokens::get().body);
    let scalar = |key: &str, default: f64| p["pbr"][key].as_f64().filter(|v| v.is_finite()).unwrap_or(default).clamp(0.0, 1.0);
    let opacity = p["opacity"].as_f64().filter(|v| v.is_finite()).unwrap_or(1.0).clamp(0.0, 1.0);
    let kind = if opacity < 1.0 {
        ui_kit::FinishKind::Glass
    } else if scalar("metallic", if category(entry) == "metal" { 1.0 } else { 0.0 }) >= 0.5 {
        ui_kit::FinishKind::Metal
    } else if scalar("roughness", 0.5) <= 0.2 {
        ui_kit::FinishKind::Gloss
    } else if scalar("roughness", 0.5) >= 0.7 {
        ui_kit::FinishKind::Matte
    } else {
        ui_kit::FinishKind::Satin
    };
    ui_kit::Sphere { color, kind, opacity: Some(opacity as f32) }
}
fn user_count(entry: &Value) -> usize {
    ["bodies", "faces", "components"].iter().filter_map(|key| entry[*key].as_array()).map(Vec::len).fold(0, usize::saturating_add)
}
fn usage_for<'a>(entry: &Value, used: &'a [Value]) -> Option<&'a Value> {
    let source = entry.get("resource").unwrap_or(entry);
    let parameters = preview_parameters(entry);
    let name = source["name"].as_str()?;
    let name = if source.get("base_linear_low").is_some() || source.pointer("/baseline_projection/runtime_parameters").is_some() {
        format!("{} {name}", entry["id"].as_str()?)
    } else {
        name.to_string()
    };
    let color = crate::dialogs_appearance::parse_color(&parameters["color"])?;
    used.iter().find(|look| {
        look["name"].as_str() == Some(&name)
            && crate::dialogs_appearance::parse_color(&look["color"]) == Some(color)
            && look["opacity"].as_f64() == parameters["opacity"].as_f64()
            && ["metallic", "roughness", "dielectric_ior"].iter().all(|key| look["pbr"][*key].as_f64() == parameters["pbr"][*key].as_f64())
    })
}
fn matches_entry(entry: &Value, query: &str, category: &str, availability: &str) -> bool {
    let state = entry.pointer("/projection/status").and_then(Value::as_str).unwrap_or(if entry["available"] == false {
        "blocked"
    } else {
        "native_appearance"
    });
    let admitted = match availability {
        "usable" => matches!(state, "native_appearance" | "native_composition" | "implemented_pending_batch_validation"),
        "blocked" => state == "blocked",
        "specification" => state == "specification_only",
        _ => true,
    };
    let source = entry.get("resource").unwrap_or(entry);
    let query = query.to_lowercase();
    admitted
        && (category.is_empty() || entry["category"].as_str() == Some(category))
        && ["id", "name", "category", "family", "description", "domains", "process", "intended_use", "tags"]
            .iter()
            .filter_map(|field| source.get(*field))
            .any(|field| field.to_string().to_lowercase().contains(&query))
}

pub(super) fn card(app: &mut SolveApp, ui: &mut egui::Ui, bounds: egui::Rect) {
    let mut side = app.preferences.render_browser_side.clone();
    let mut width = app.preferences.render_browser_width;
    let stage = app.render_workspace.view.stage;
    if stage == super::Stage::Look {
        let view = &mut app.render_workspace.view.library;
        let compatible = if view.look_tab == 1 {
            matches!(view.kind.as_str(), "surfaces" | "physical_profiles")
        } else {
            LOOK_KINDS.contains(&view.kind.as_str())
        };
        if !compatible {
            let kind = if view.look_tab == 1 { "surfaces" } else { "appearances" };
            switch_kind(view, kind);
            view.collection = 0;
        }
    }
    let title = app
        .tr(if stage == super::Stage::Look && app.render_workspace.view.library.look_tab == 1 {
            "render.stage_panels.surfaces_references"
        } else {
            stage_title(stage)
        })
        .to_string();
    let mut mode = if app.render_workspace.view.library.look_tab == 1 { 2 } else { app.render_workspace.view.library.layout };
    let mut actions = |ui: &mut egui::Ui| {
        ui_kit::catalog_scope(ui, |ui| {
            ui_kit::segmented_icons(
                ui,
                &mut mode,
                &[
                    ui_kit::IconSegment { key: "render:library-grid", icon: "grid", tip: crate::i18n::tr("render.library_grid").into() },
                    ui_kit::IconSegment { key: "render:library-list", icon: "list", tip: crate::i18n::tr("render.library_list").into() },
                    ui_kit::IconSegment {
                        key: "render:library-panel",
                        icon: "library",
                        tip: crate::i18n::tr("render.stage_panels.surfaces_references").into(),
                    },
                ],
                &[],
            )
        });
    };
    let response = ui_kit::Panel::workspace(
        ui,
        ui_kit::WorkspaceOptions {
            id: "render:appearances",
            title: &title,
            icon: "",
            bounds,
            fill_height: true,
            compact: crate::render_ui::compact(app, bounds.width()),
            editor: false,
            enabled: true,
            header_actions: (stage == super::Stage::Look).then_some(&mut actions as &mut dyn FnMut(&mut egui::Ui)),
        },
        &mut side,
        &mut width,
        |ui| match stage {
            super::Stage::Look => ui_kit::catalog_scope(ui, |ui| {
                if app.render_workspace.view.library.look_tab == 0 {
                    browse_contents(app, ui, true);
                } else {
                    surface_panel(app, ui);
                }
            }),
            super::Stage::Light => light_panel(app, ui),
            super::Stage::Frame => super::shot::presets(app, ui),
            super::Stage::Output => super::inspector::size_presets(app, ui),
        },
    );
    let look_tab = usize::from(mode == 2);
    if mode < 2 {
        app.render_workspace.view.library.layout = mode;
    }
    if look_tab != app.render_workspace.view.library.look_tab {
        switch_kind(&mut app.render_workspace.view.library, if look_tab == 0 { "appearances" } else { "surfaces" });
        app.render_workspace.view.library.collection = 0;
        app.render_workspace.view.library.look_tab = look_tab;
    }
    app.preferences.render_browser_width = width;
    let requested =
        response.as_ref().and_then(|r| r.inner.1).map(str::to_string).or_else(|| (app.preferences.render_browser_side != side).then(|| side.clone()));
    if let Some(side) = requested {
        let _ = app.run("view.render_dock", json!({"panel":"browser","side":side}));
    }
    crate::shell::register_panel(
        app,
        "render:appearances",
        response.map(|r| r.response.rect),
        if bounds.width() < 800.0 { None } else { Some(if side == "left" { ui_kit::Edge::Left } else { ui_kit::Edge::Right }) },
    );
}

fn stage_title(stage: super::Stage) -> &'static str {
    match stage {
        super::Stage::Look => "render.appearances",
        super::Stage::Light => "render.studios_environments",
        super::Stage::Frame => "render.stage_frame",
        super::Stage::Output => "render.size_presets",
    }
}

fn restore_view(app: &mut SolveApp, mut view: View) {
    if let Some(review) = app.render_workspace.view.library.review.take() {
        view.review = Some(review);
    }
    if let Some(removal) = app.render_workspace.view.library.removal.take() {
        view.removal = Some(removal);
    }
    app.render_workspace.view.library = view;
}

const LIGHT_SCOPES: &[(&str, &str)] = &[
    ("native_scenes", "render.stage_panels.studios"),
    ("rigs", "render.stage_panels.rigs"),
    ("hdr", "render.stage_panels.environments"),
    ("scenes", "render.stage_panels.specifications"),
    ("mine", "render.stage_panels.mine"),
];

fn light_panel(app: &mut SolveApp, ui: &mut egui::Ui) {
    let mut view = std::mem::take(&mut app.render_workspace.view.library);
    let previous = view.light_scope;
    let labels: Vec<_> = LIGHT_SCOPES.iter().map(|(_, key)| app.tr(key)).collect();
    ui_kit::choice_row(ui, app.tr("render.stage_panels.scope"), "render_light_scope", &mut view.light_scope, &labels);
    let compatible = match view.light_scope {
        0 => view.kind == "native_scenes",
        1 => view.kind == "rigs",
        2 => view.kind == "hdr",
        3 => matches!(view.kind.as_str(), "scenes" | "selene_shots" | "lighting" | "environments"),
        4 => true,
        _ => false,
    };
    if previous != view.light_scope || !compatible {
        if let Some((kind, _)) = LIGHT_SCOPES.get(view.light_scope)
            && *kind != "mine"
        {
            switch_kind(&mut view, kind);
        }
        view.collection = 0;
    }
    if view.light_scope == 4 {
        ui.add_enabled_ui(false, |ui| {
            ui_kit::search_field(ui, ui.id().with("render_mine_search"), &mut view.query, app.tr("render.stage_panels.search_studios"));
            ui_kit::button(ui, app.tr("render.stage_panels.apply"), ui_kit::Button::Normal);
        });
        ui_kit::text(ui, app.tr("render.stage_panels.mine_pending"), ui_kit::Text::Note("render:mine:reason"));
        restore_view(app, view);
        return;
    }
    catalog_search(app, ui, &mut view, "render.stage_panels.search_studios");
    let reason = if view.light_scope == 3 { Some(app.tr("render.library_specification_only").to_string()) } else { light_reason(app, &view.kind) };
    let layout = ui_kit::CatalogLayout::new(ui, "render:light:catalog", 0);
    layout.body(ui, |ui| {
        let identities: Vec<_> = view.entries.iter().map(|e| format!("render:light:{}", e["id"].as_str().unwrap_or(""))).collect();
        let tiles: Vec<_> = view
            .entries
            .iter()
            .zip(&identities)
            .map(|(entry, identity)| ui_kit::SphereTile {
                id: identity,
                name: entry["name"].as_str().unwrap_or(""),
                sphere: ui_kit::Sphere { color: Tokens::get().body, kind: ui_kit::FinishKind::Metal, opacity: None },
                selected: view.selected.as_deref() == entry["id"].as_str(),
                users: 0,
                tip: entry["name"].as_str().unwrap_or(""),
                badge: Some(tile_badge(app, entry)),
            })
            .collect();
        if let Some(entry) = ui_kit::sphere_grid(ui, &tiles).and_then(|index| view.entries.get(index)).cloned()
            && let Some(id) = entry["id"].as_str()
        {
            let kind = entry["library_kind"].as_str().unwrap_or(&view.kind).to_string();
            if let Err(error) = choose_in_stage(app, &mut view, &kind, id) {
                app.set_status(error, true);
            }
        }
        if view.light_scope == 3
            && let Some(detail) = &view.details
        {
            ui_kit::text(ui, detail["resource"]["description"].as_str().unwrap_or(""), ui_kit::Text::Note("render:specification"));
        }
    });
    layout.footer(ui, |ui| {
        catalog_pages(app, ui, &mut view);
        if let Some(reason) = &reason {
            ui_kit::text(ui, reason, ui_kit::Text::Note("render:light:reason"));
        }
        let apply = ui_kit::button(ui, app.tr("render.stage_panels.apply"), ui_kit::Button::Enabled(view.selected.is_some() && reason.is_none()));
        crate::scenario::publish_target("render:light:review", apply.rect);
        if apply.clicked()
            && let Some(id) = view.selected.clone()
            && let Err(error) = refresh_targets(app, &mut view).and_then(|()| review::light(app, &view, &id))
        {
            app.set_status(error, true);
        }
        ui_kit::catalog_footer_with_tip(ui, app.tr("render.stage_layout.studio_note"), app.tr("render.stage_panels.studio_note"));
    });
    restore_view(app, view);
}

fn catalog_search(app: &mut SolveApp, ui: &mut egui::Ui, view: &mut View, hint: &str) {
    let search = ui_kit::search_field(ui, ui.id().with("render_stage_catalog_search"), &mut view.query, app.tr(hint));
    crate::scenario::publish_target("render:library-search", search.rect);
    let end = view.query.char_indices().nth(128).map_or(view.query.len(), |(at, _)| at);
    view.query.truncate(end);
    if search.changed() {
        view.offset = 0;
        view.loaded = false;
    }
    let changed =
        view.revision != Some(app.session.revision) || view.document.as_ref().is_none_or(|doc| !Weak::ptr_eq(doc, &Arc::downgrade(&app.session.doc)));
    if (!view.loaded || changed)
        && let Err(error) = refresh(app, view)
    {
        view.loaded = true;
        app.set_status(error, true);
    }
}
fn catalog_pages(app: &SolveApp, ui: &mut egui::Ui, view: &mut View) {
    if view.offset > 0 || view.next.is_some() {
        ui.horizontal(|ui| {
            if ui_kit::button(ui, app.tr("render.library_previous"), ui_kit::Button::Enabled(view.offset > 0)).clicked() {
                view.offset = view.offset.saturating_sub(48);
                view.loaded = false;
            }
            if ui_kit::button(ui, app.tr("render.library_next"), ui_kit::Button::Enabled(view.next.is_some())).clicked() {
                view.offset = view.next.unwrap_or(view.offset);
                view.loaded = false;
            }
        });
    }
}
fn surface_panel(app: &mut SolveApp, ui: &mut egui::Ui) {
    let mut view = std::mem::take(&mut app.render_workspace.view.library);
    let mut scope = usize::from(view.kind == "physical_profiles");
    if ui_kit::segmented(
        ui,
        &mut scope,
        &[app.tr("render.stage_panels.surfaces"), app.tr("render.stage_panels.references")],
        ui_kit::Segments { compact: true, proportional: true, targets: &["render:surfaces", "render:references"], ..Default::default() },
    ) {
        switch_kind(&mut view, if scope == 0 { "surfaces" } else { "physical_profiles" });
    }
    catalog_search(app, ui, &mut view, "render.stage_panels.search_surfaces");
    let layout = ui_kit::CatalogLayout::new(ui, "render:surface:catalog", 3);
    layout.body(ui, |ui| {
        for entry in view.entries.clone() {
            let id = entry["id"].as_str().unwrap_or("");
            let label = entry["name"].as_str().or_else(|| entry["resource"]["designation"].as_str()).unwrap_or(id);
            let mut item = ui_kit::MenuItem::new(id, label, "grid");
            item.highlighted = view.selected.as_deref() == Some(id);
            let response =
                item.search_row(ui, &view.query, short_availability(app, entry.pointer("/projection/status").and_then(Value::as_str)), None);
            crate::scenario::publish_target(&format!("render:library:{id}"), response.rect);
            if response.clicked() {
                let kind = view.kind.clone();
                if let Err(error) = choose_in_stage(app, &mut view, &kind, id) {
                    app.set_status(error, true);
                }
            }
        }
    });
    layout.footer(ui, |ui| {
        catalog_pages(app, ui, &mut view);
        if let Some(id) = view.selected.clone() {
            if scope == 0 {
                ui_kit::divider(ui, "surface:rule");
                ui_kit::text(ui, app.tr("render.stage_panels.source_note"), ui_kit::Text::Note("surface:note"));
                source_preview(app, ui, &mut view, &id);
                surface_controls(app, ui, &mut view, &id);
                if let Some(detail) = view.details.clone() {
                    let title = app.tr("render.library_details").to_string();
                    ui_kit::section(ui, ("surface:inspect", &id), &title, false, |ui| {
                        if let Some(description) = detail["resource"]["description"].as_str() {
                            ui_kit::text(ui, description, ui_kit::Text::Note("surface:description"));
                        }
                        for source in detail["sources"].as_array().into_iter().flatten().take(128) {
                            if let Some(url) = source["url"].as_str() {
                                ui.hyperlink_to(source["title"].as_str().unwrap_or(url), url);
                            }
                        }
                        if ui_kit::button(ui, app.tr("render.copy_request"), ui_kit::Button::Normal).clicked() {
                            ui.ctx().copy_text(serde_json::to_string_pretty(&detail).unwrap_or_default());
                        }
                    });
                }
            } else if let Some(detail) = view.details.clone() {
                physical_controls(app, ui, &mut view, &id, &detail);
            }
        }
    });
    restore_view(app, view);
}
fn light_reason(app: &SolveApp, kind: &str) -> Option<String> {
    if matches!(kind, "native_scenes" | "rigs") && app.render_workspace.view.method != ImageMethod::GpuRaytrace {
        return Some(app.tr("render.stage_panels.gpu_required").into());
    }
    if app.render_workspace.view.method == ImageMethod::Raster {
        return Some(app.tr("render.environment_tiles_require_raytrace").into());
    }
    patch(app).err()
}

fn surface_controls(app: &mut SolveApp, ui: &mut egui::Ui, view: &mut View, id: &str) {
    let picked = app.session.selection.iter().find_map(|selection| match selection {
        solidcraft_engine::Sel::Body { name } | solidcraft_engine::Sel::Face { body: name, .. } => Some(name.clone()),
        _ => None,
    });
    if !current_target(app, view) {
        let previous = view.target.clone();
        match refresh_targets(app, view) {
            Ok(()) => view.target = previous.filter(|target| view.targets.contains(target)),
            Err(error) => app.set_status(error, true),
        }
    }
    if picked != view.picked_target || view.target.is_none() {
        if let Some(name) = &picked {
            view.target = view.targets.iter().find(|target| target.name == *name).cloned();
        }
        view.picked_target = picked;
    }
    let target_row = ui_kit::row(ui, app.tr("render.stage_panels.target"), |ui| {
        let label = view.target.as_ref().map_or(app.tr("render.material_choose_body"), |target| target.name.as_str());
        let label = label.to_string();
        ui_kit::dropdown_menu(ui, "render_surface_target", &label, |ui| {
            for target in &view.targets {
                if ui_kit::list_picker(ui, &target.name, &target.name, view.target.as_ref() == Some(target)).clicked() {
                    view.target = Some(target.clone());
                    ui.close();
                }
            }
        })
    });
    crate::scenario::publish_target("render:surface:target", target_row.rect);
    ui_kit::row(ui, app.tr("render.stage_panels.plane"), |ui| {
        ui_kit::segmented(
            ui,
            &mut view.mapping_plane,
            &["xy", "yz", "zx"],
            ui_kit::Segments { compact: true, targets: &["render:surface:xy", "render:surface:yz", "render:surface:zx"], ..Default::default() },
        );
    });
    ui_kit::row(ui, app.tr("render.stage_panels.repeat"), |ui| {
        let repeat = ui_kit::number_field(ui, &mut view.mapping_scale, 0.001..=1000.0, 0.01, " ×");
        crate::scenario::publish_target("render:surface:repeat", repeat.rect);
    });
    let supported = view
        .details
        .as_ref()
        .is_some_and(|detail| matches!(detail["projection"]["status"].as_str(), Some("implemented_pending_batch_validation" | "native_appearance")));
    let enabled = supported && current_target(app, view);
    let gpu = app.render_workspace.view.method == ImageMethod::GpuRaytrace;
    let mut requested = None;
    ui.horizontal(|ui| {
        if ui_kit::button(ui, app.tr("render.stage_panels.use_uniform"), ui_kit::Button::Enabled(enabled)).clicked() {
            requested = Some(0);
        }
        if ui_kit::button(ui, app.tr("render.stage_panels.use_maps"), ui_kit::Button::Enabled(enabled && gpu)).clicked() {
            requested = Some(1);
        }
    });
    if ui_kit::button(ui, app.tr("render.stage_panels.save_maps"), ui_kit::Button::Enabled(enabled && gpu && patch(app).is_ok())).clicked() {
        requested = Some(2);
    }
    if !gpu {
        ui_kit::text(ui, app.tr("render.stage_panels.gpu_required"), ui_kit::Text::Note("surface:gpu:reason"));
    }
    if !supported {
        ui_kit::text(ui, app.tr("render.library_specification_only"), ui_kit::Text::Note("surface:reference:reason"));
    }
    if let (Some(mode), Some(target)) = (requested, view.target.clone()) {
        let result = (|| -> Result<(), String> {
            let label = view.details.as_ref().and_then(|d| d["resource"]["name"].as_str()).unwrap_or(id).to_string();
            let plane = ["xy", "yz", "zx"].get(view.mapping_plane).copied().unwrap_or("xy");
            let (plan, action) = if mode == 0 {
                (
                    view.details.clone().unwrap_or_default(),
                    review::Apply::Command {
                        id: "render.library.appearance.apply".into(),
                        params: json!({"id":id,"variant":"uniform_opaque","bodies":[target.name]}),
                    },
                )
            } else {
                let surface = json!({"id":id,"body_index":target.ordinal,"body_name":target.name,"scale":view.mapping_scale,"plane":plane});
                let plan = app.run("render.library.surface.prepare", surface.clone())?;
                let action = if mode == 2 {
                    review::Apply::Command {
                        id: "render.library.surface.bind".into(),
                        params: json!({"name":app.render_workspace.view.name,"set":patch(app)?,"surface":surface,"replace":true}),
                    }
                } else {
                    review::Apply::Maps { id: id.into(), target, plane: plane.into(), scale: view.mapping_scale }
                };
                (plan, action)
            };
            review::open(
                app,
                if mode == 0 {
                    "uniform"
                } else if mode == 2 {
                    "saved_maps"
                } else {
                    "maps"
                },
                label,
                plan,
                action,
            )
        })();
        if let Err(error) = result {
            app.set_status(error, true);
        }
    }
}

fn selected_looks(app: &mut SolveApp) -> Vec<Value> {
    use solidcraft_engine::Sel;
    let targets: Vec<_> = app
        .session
        .selection
        .iter()
        .filter_map(|sel| match sel {
            Sel::Body { name } => Some(json!({"body":name})),
            Sel::Face { body, index, .. } => Some(json!({"body":body,"face":index})),
            Sel::Occurrence { path } => {
                path.last().and_then(|id| app.session.doc.occurrences.iter().find(|o| o.id == *id)).map(|o| json!({"component":o.component}))
            }
            _ => None,
        })
        .collect();
    targets
        .into_iter()
        .filter_map(|target| {
            let chain = app.run("appearance.chain", json!({"target":target})).ok()?;
            chain["steps"].as_array()?.iter().find(|s| s["step"] == chain["active"]).map(|s| s["look"].clone()).filter(|v| !v.is_null())
        })
        .collect()
}

fn category(entry: &Value) -> &'static str {
    let source = entry.get("resource").unwrap_or(entry);
    let label = source["category"].as_str().or_else(|| source["name"].as_str()).unwrap_or("").to_lowercase();
    if label.contains("wood") {
        "wood"
    } else if label.contains("paint") || label.contains("coating") {
        "paint"
    } else if label.contains("plastic") || label.contains("abs") || label.contains("rubber") || label.contains("polymer") {
        "plastic"
    } else if ["metal", "alumin", "steel", "iron", "brass", "copper", "titanium", "chrome"].iter().any(|word| label.contains(word)) {
        "metal"
    } else {
        "other"
    }
}

fn applicable(entry: &Value, selected: &[Value], used: &[Value]) -> bool {
    usage_for(entry, used).is_some()
        || user_count(entry) > 0
        || selected.iter().any(|look| {
            let same_category = category(entry) != "other" && category(entry) == category(look);
            same_category || tile_sphere(entry).kind == tile_sphere(look).kind
        })
}

fn browse_contents(app: &mut SolveApp, ui: &mut egui::Ui, stage_card: bool) {
    let mut view = std::mem::take(&mut app.render_workspace.view.library);
    let document_changed =
        view.revision != Some(app.session.revision) || view.document.as_ref().is_none_or(|doc| !Weak::ptr_eq(doc, &Arc::downgrade(&app.session.doc)));
    if (!view.loaded || document_changed)
        && let Err(error) = refresh(app, &mut view)
    {
        view.loaded = true;
        app.set_status(error, true);
    }
    let count = if stage_card && matches!(view.collection, 1 | 2) {
        view.entries.iter().filter(|entry| LOOK_KINDS.contains(&entry["library_kind"].as_str().unwrap_or("appearances"))).count()
    } else {
        view.total
    };
    let hint = crate::i18n::format("render.search_appearances", &[("count", &count.to_string())]);
    let query = if view.collection == 0 { &mut view.query } else { &mut view.collection_query };
    let search = ui_kit::search_field(ui, ui.id().with("render_library_search"), query, &hint);
    query.truncate(query.char_indices().nth(128).map_or(query.len(), |(at, _)| at));
    crate::scenario::publish_target("render:library-search", search.rect);
    if search.changed() {
        view.offset = 0;
        view.loaded = false;
    }
    let labels = [
        app.tr("render.library_in_design").to_string(),
        app.tr("render.browser_library").to_string(),
        app.tr("render.library_recent").to_string(),
        "★".to_string(),
    ];
    let mut scope = match view.collection {
        3 => 0,
        0 => 1,
        2 => 2,
        _ => 3,
    };
    if ui_kit::segmented(
        ui,
        &mut scope,
        &labels.iter().map(String::as_str).collect::<Vec<_>>(),
        ui_kit::Segments {
            compact: true,
            proportional: true,
            workspace_accent: true,
            targets: &[
                "render:library-scope:in-design",
                "render:library-scope:library",
                "render:library-scope:recent",
                "render:library-scope:favorites",
            ],
            ..Default::default()
        },
    ) {
        view.collection = [3, 0, 2, 1].get(scope).copied().unwrap_or(0);
        view.loaded = false;
        view.offset = 0;
        view.category.clear();
    }
    let document_changed =
        view.revision != Some(app.session.revision) || view.document.as_ref().is_none_or(|doc| !Weak::ptr_eq(doc, &Arc::downgrade(&app.session.doc)));
    if (!view.loaded || document_changed)
        && let Err(error) = refresh(app, &mut view)
    {
        view.loaded = true;
        app.set_status(error, true);
    }
    let mut entries: Vec<(String, String, Value)> = view
        .entries
        .iter()
        .map(|entry| {
            let appearance = view.collection == 3 || view.kind == "appearances";
            (
                entry["library_kind"]
                    .as_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| if appearance { "appearances".into() } else { view.kind.clone() }),
                entry["catalog_identity"]
                    .as_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| if appearance { appearance_id(entry) } else { entry["id"].as_str().unwrap_or("").to_string() }),
                entry.clone(),
            )
        })
        .collect();
    if stage_card {
        entries.retain(|(kind, _, _)| LOOK_KINDS.contains(&kind.as_str()));
    }
    egui::ScrollArea::horizontal().id_salt("render_library_chips").scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden).show(
        ui,
        |ui| {
            ui.horizontal(|ui| {
                for (id, label) in
                    [("", "ui.all"), ("metal", "render.category_metal"), ("plastic", "render.category_plastic"), ("paint", "render.category_paint")]
                {
                    if ui_kit::choice_chip(ui, &format!("render-library-{id}"), app.tr(label), view.category == id).clicked() {
                        view.category = id.into();
                    }
                }
            });
        },
    );
    let query = if view.collection == 0 { &view.query } else { &view.collection_query };
    entries.retain(|(_, _, entry)| {
        matches_entry(entry, query, "", &view.availability) && (view.category.is_empty() || category(entry) == view.category)
    });
    let used = app.run("appearance.list", json!({})).ok().and_then(|reply| reply["in_design"].as_array().cloned()).unwrap_or_default();
    let selected_looks = selected_looks(app);
    entries.sort_by_key(|(_, _, entry)| !applicable(entry, &selected_looks, &used));
    let applicable_count = entries.iter().take_while(|(_, _, entry)| applicable(entry, &selected_looks, &used)).count();
    let tips: Vec<_> = entries
        .iter()
        .map(|(_, id, entry)| {
            let p = preview_parameters(entry);
            let usage = usage_for(entry, &used).unwrap_or(entry);
            let users = user_count(usage);
            let names: Vec<_> = ["bodies", "faces", "components"]
                .iter()
                .filter_map(|key| usage[*key].as_array())
                .flatten()
                .take(128)
                .filter_map(|user| user.as_str().or_else(|| user["body"].as_str()))
                .collect();
            let scalar = |value: &Value| value.as_f64().map(|n| format!("{n:.2}")).unwrap_or_else(|| "—".into());
            format!(
                "{}\n{}\n{}: {} · {}: {}\n{}: {} · {}: {}\n{}: {} {}\n{}",
                entry["name"].as_str().unwrap_or(id),
                entry["category"].as_str().or_else(|| entry["family"].as_str()).unwrap_or(app.tr("render.library_in_design")),
                app.tr("render.library_metallic"),
                scalar(&p["pbr"]["metallic"]),
                app.tr("render.library_roughness"),
                scalar(&p["pbr"]["roughness"]),
                app.tr("render.library_ior"),
                scalar(&p["pbr"]["dielectric_ior"]),
                app.tr("render.library_opacity"),
                scalar(&p["opacity"]),
                app.tr("render.library_users"),
                users,
                names.join(", "),
                availability_label(
                    app,
                    entry.pointer("/projection/status").and_then(Value::as_str).or(Some(if entry["available"] == false {
                        "blocked"
                    } else {
                        "native_appearance"
                    }))
                )
            )
        })
        .collect();
    let identities: Vec<_> = entries.iter().map(|(_, id, _)| format!("render:library:{id}")).collect();
    let layout = ui_kit::CatalogLayout::new(ui, "render:library:catalog", 0);
    let mut selected = None;
    let mut dragged = None;
    // Keep each catalog/collection's independent scroll position.
    ui.push_id((&view.kind, view.collection), |ui| {
        layout.body(ui, |ui| {
            if entries.is_empty() {
                ui_kit::text(ui, app.tr("render.library_empty"), ui_kit::Text::Note("render.library_empty"));
            }
            if view.layout == 0 {
                let tiles: Vec<_> = entries
                    .iter()
                    .zip(&identities)
                    .zip(&tips)
                    .map(|(((kind, id, entry), identity), tip)| ui_kit::SphereTile {
                        id: identity,
                        name: entry["name"].as_str().unwrap_or(id),
                        sphere: tile_sphere(entry),
                        selected: view.kind == *kind && view.selected.as_ref() == Some(id),
                        users: usage_for(entry, &used).map_or_else(|| user_count(entry), user_count),
                        tip,
                        badge: Some(tile_badge(app, entry)),
                    })
                    .collect();
                if applicable_count > 0 {
                    ui_kit::text(ui, app.tr("render.library_applicable"), ui_kit::Text::Inline);
                    (selected, dragged) = ui_kit::sphere_grid_drag(ui, tiles.get(..applicable_count).unwrap_or_default());
                }
                if applicable_count < tiles.len() {
                    if applicable_count > 0 {
                        ui_kit::text(ui, app.tr("render.library_other"), ui_kit::Text::Inline);
                    }
                    let (other, other_drag) = ui_kit::sphere_grid_drag(ui, tiles.get(applicable_count..).unwrap_or_default());
                    selected = selected.or(other.map(|i| i + applicable_count));
                    dragged = dragged.or(other_drag.map(|i| i + applicable_count));
                }
            } else {
                for (index, ((kind, id, entry), tip)) in entries.iter().zip(&tips).enumerate() {
                    if index == 0 && applicable_count > 0 {
                        ui_kit::text(ui, app.tr("render.library_applicable"), ui_kit::Text::Inline);
                    } else if index == applicable_count && applicable_count > 0 {
                        ui_kit::text(ui, app.tr("render.library_other"), ui_kit::Text::Inline);
                    }
                    let mut item = ui_kit::MenuItem::new(id, entry["name"].as_str().unwrap_or(id), "appearance");
                    item.highlighted = view.kind == *kind && view.selected.as_ref() == Some(id);
                    let r = item.search_row(
                        ui,
                        query,
                        &format!(
                            "{} / {}",
                            if kind == "appearances" { entry["category"].as_str().unwrap_or(app.tr("render.library_in_design")) } else { id },
                            short_availability(app, entry.pointer("/projection/status").and_then(Value::as_str))
                        ),
                        None,
                    );
                    crate::scenario::publish_target(identities.get(index).map(String::as_str).unwrap_or("render:library:item"), r.rect);
                    if r.on_hover_text(tip).clicked() {
                        selected = Some(index);
                    }
                }
            }
        });
    });
    layout.footer(ui, |ui| {
        if !stage_card {
            let menu = ui_kit::dropdown_menu(ui, "render_library_filters", app.tr("render.library_filters"), |ui| {
                if view.collection == 0 {
                    for &(kind, key) in KINDS {
                        let r = ui_kit::list_picker(ui, kind, app.tr(key), view.kind == kind);
                        crate::scenario::publish_target(&format!("render:library-kind:{kind}"), r.rect);
                        if r.clicked() {
                            switch_kind(&mut view, kind);
                            ui.close();
                        }
                    }
                    ui_kit::divider(ui, "render-library-availability");
                }
                for (id, key) in [
                    ("all", "render.library_all_states"),
                    ("usable", "render.library_usable"),
                    ("specification", "render.library_specs"),
                    ("blocked", "render.library_blocked"),
                ] {
                    if ui_kit::list_picker(ui, id, app.tr(key), view.availability == id).clicked() {
                        view.availability = id.into();
                        view.offset = 0;
                        view.loaded = false;
                        ui.close();
                    }
                }
            });
            crate::scenario::publish_target("render:library-category", menu.rect);
            if view.collection == 0 && view.kind != "appearances" {
                ui.horizontal(|ui| {
                    if ui_kit::button(ui, app.tr("render.library_previous"), ui_kit::Button::Enabled(view.offset > 0)).clicked() {
                        view.offset = view.offset.saturating_sub(48);
                        view.loaded = false;
                    }
                    if ui_kit::button(ui, app.tr("render.library_next"), ui_kit::Button::Enabled(view.next.is_some())).clicked() {
                        view.offset = view.next.unwrap_or(view.offset);
                        view.loaded = false;
                    }
                });
            }
        }
        for (index, drag) in [(selected, false), (dragged, true)] {
            if let Some((kind, id, entry)) = index.and_then(|index| entries.get(index)) {
                let result = if stage_card { choose_in_stage(app, &mut view, kind, id) } else { choose(app, &mut view, kind, id) }.and_then(|()| {
                    if !stage_card {
                        return Ok(());
                    }
                    if kind == "appearances" {
                        if entry["available"] == false {
                            return Err(app.tr("render.library_blocked").to_string());
                        }
                        let params = if let Some(id) = entry["id"].as_str() { json!({"appearance":id}) } else { preview_parameters(entry) };
                        return super::assign::tile(app, params, drag);
                    }
                    if kind == "selene_appearances" && !drag {
                        let bodies: Vec<_> = app
                            .session
                            .selection
                            .iter()
                            .filter_map(|selection| match selection {
                                solidcraft_engine::Sel::Body { name } => Some(name.clone()),
                                _ => None,
                            })
                            .collect();
                        if !bodies.is_empty() {
                            return review::open(
                                app,
                                "uniform",
                                entry["name"].as_str().unwrap_or(id).into(),
                                view.details.clone().unwrap_or_default(),
                                review::Apply::Command {
                                    id: "render.library.appearance.apply".into(),
                                    params: json!({"id":id,"variant":"uniform_opaque","bodies":bodies}),
                                },
                            );
                        }
                        if !app.session.selection.is_empty() {
                            return Err(app.tr("render.material_choose_body").to_string());
                        }
                    }
                    Ok(())
                });
                if let Err(error) = result {
                    app.set_status(error, true);
                }
            }
        }
        if stage_card {
            ui_kit::catalog_footer_with_tip(ui, app.tr("render.stage_layout.appearance_note"), app.tr("render.assign_help"));
            if view.kind == "materials" {
                ui_kit::text(ui, app.tr("render.library_specification_only"), ui_kit::Text::Note("appearance:reference:reason"));
            }
        } else {
            ui_kit::text(ui, app.tr("render.library_inspect_only"), ui_kit::Text::Note("render.library_inspect_only"));
        }
    });
    restore_view(app, view);
}

pub(super) fn selected_kind(app: &SolveApp, kind: &str) -> bool {
    let view = &app.render_workspace.view.library;
    view.kind == kind && view.selected.is_some()
}

pub(super) fn inspect_kinds(app: &mut SolveApp, ui: &mut egui::Ui, kinds: &[&str]) {
    if kinds.contains(&app.render_workspace.view.library.kind.as_str())
        || (kinds.contains(&"surfaces") && app.render_workspace.view.library.kind == "appearances")
    {
        inspect(app, ui);
    } else {
        ui_kit::text(ui, app.tr("render.library_select_hint"), ui_kit::Text::Note("render.library_select_hint"));
    }
}

pub(super) fn inspect_appearance(app: &mut SolveApp, ui: &mut egui::Ui) {
    if app.render_workspace.view.library.kind == "appearances" {
        inspect(app, ui);
    }
}

/// Interactive command activation can review the specification selected by S5.
pub(super) fn review_selected_quality(app: &mut SolveApp) -> Result<(), String> {
    let id = app.render_workspace.view.library.selected.clone().filter(|_| app.render_workspace.view.library.kind == "quality");
    if let Some(id) = id {
        super::review_resource(app, "render.quality.prepare", &id)?;
    }
    Ok(())
}

pub(super) fn review_open(app: &SolveApp) -> bool {
    let view = &app.render_workspace.view.library;
    view.review.is_some() || view.removal.is_some()
}
pub(super) fn inspect(app: &mut SolveApp, ui: &mut egui::Ui) {
    let mut view = std::mem::take(&mut app.render_workspace.view.library);
    if let Some(id) = view.selected.clone() {
        ui.strong(view.details.as_ref().and_then(|d| d["resource"]["name"].as_str()).unwrap_or(&id));
        if view.kind != "appearances" {
            ui.small(&id);
        }
        // Native composition is an actionable review, not a long specification
        // reader: keep Resolve/consent/Apply ahead of ancillary resource metadata.
        let native_composition = view.kind == "native_scenes";
        if native_composition {
            super::inspector::scene_resource(app, ui, &id);
        }
        let key = (view.kind.clone(), id.clone());
        let favorite = app.preferences.render_library_favorites.contains(&key);
        if ui_kit::button(
            ui,
            app.tr(if favorite { "render.library_unfavorite" } else { "render.library_favorite" }),
            ui_kit::Button::Enabled(favorite || app.preferences.render_library_favorites.len() < 256),
        )
        .clicked()
        {
            if favorite {
                app.preferences.render_library_favorites.remove(&key);
            } else {
                app.preferences.render_library_favorites.insert(key);
            }
            view.loaded = false;
        }
        if let Some(detail) = &view.details {
            ui.small(format!("{}: {}", app.tr("render.library_status"), availability_label(app, detail["projection"]["status"].as_str())));
            if let Some(description) = detail["resource"]["description"].as_str() {
                ui.label(description);
            }
            if let Some(family) = detail["resource"]["family"].as_str() {
                ui.small(family);
            }
            if let Some(omitted) = detail["projection"]["omitted"].as_array() {
                for item in omitted {
                    ui.small(format!(
                        "{}: {}",
                        app.tr("render.library_omitted"),
                        item.as_str().unwrap_or(crate::i18n::tr("render.property_unknown"))
                    ));
                }
            }
        }
        if view.kind == "surfaces" {
            source_preview(app, ui, &mut view, &id);
        }
        if !native_composition {
            resource_controls(app, ui, &mut view, &id);
        }
        if let Some(detail) = &view.details {
            let title = app.tr("render.library_details").to_string();
            ui_kit::section(ui, ("render_library_detail", &id), &title, false, |ui| {
                let text = serde_json::to_string_pretty(detail).unwrap_or_default();
                if ui_kit::button(ui, app.tr("render.copy_request"), ui_kit::Button::Normal).clicked() {
                    ui.ctx().copy_text(text.clone());
                }
                egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                    ui.monospace(text);
                });
            });
        }
    } else {
        ui_kit::text(ui, app.tr("render.library_select_hint"), ui_kit::Text::Note("render.library_select_hint"));
    }
    restore_view(app, view);
}
fn source_preview(app: &mut SolveApp, ui: &mut egui::Ui, view: &mut View, id: &str) {
    if view.preview.as_ref().is_none_or(|preview| preview.id != id) && view.preview_error.is_none() {
        let result = (|| -> Result<SourcePreview, String> {
            let reply = app.run("render.library.preview", json!({"id":id}))?;
            let bytes: Vec<u8> = serde_json::from_value(reply["bytes"].clone()).map_err(|e| e.to_string())?;
            if bytes.len() > 2 * 1024 * 1024 {
                return Err(crate::i18n::tr("render.error_source_preview").into());
            }
            let mut reader = image::ImageReader::with_format(std::io::Cursor::new(bytes), image::ImageFormat::Png);
            let mut limits = image::Limits::default();
            limits.max_image_width = Some(1024);
            limits.max_image_height = Some(1024);
            limits.max_alloc = Some(16 * 1024 * 1024);
            reader.limits(limits);
            let pixels = reader.decode().map_err(|e| e.to_string())?.to_rgba8();
            let size = [pixels.width() as usize, pixels.height() as usize];
            let image = egui::ColorImage::from_rgba_unmultiplied(size, pixels.as_raw());
            Ok(SourcePreview {
                id: id.into(),
                sha256: reply["sha256"].as_str().unwrap_or("").into(),
                texture: ui.ctx().load_texture(format!("source-map-{id}"), image, egui::TextureOptions::LINEAR),
            })
        })();
        match result {
            Ok(preview) => view.preview = Some(preview),
            Err(e) => view.preview_error = Some(e),
        }
    }
    if let Some(preview) = &view.preview {
        ui.add(egui::Image::new(&preview.texture).max_size(egui::vec2(ui.available_width(), 64.0)).shrink_to_fit());
    }
    if let Some(error) = &view.preview_error {
        ui.small(error);
    }
}
#[cfg(test)]
pub(super) fn controls(app: &mut SolveApp, ui: &mut egui::Ui) {
    stage_controls(app, ui);
    saved_surface_controls(app, ui);
    let title = app.tr("render.library_title").to_string();
    ui_kit::section(ui, "render_resource_library", &title, false, |ui| {
        browse_contents(app, ui, false);
        inspect(app, ui);
    });
}

fn resource_controls(app: &mut SolveApp, ui: &mut egui::Ui, view: &mut View, id: &str) {
    let Some(detail) = view.details.clone() else { return };
    let source = &detail["resource"];
    if let Some(description) = source["description"].as_str().or_else(|| source["purpose"].as_str()) {
        ui.small(description);
    }
    let mut linked_profile = None;
    if let Some(specifications) = detail["physical_references"]["specifications"].as_array() {
        let title = app.tr("render.physical_associations").to_string();
        ui_kit::section(ui, ("physical_associations", id), &title, false, |ui| {
            ui_kit::text(ui, app.tr("render.physical_separate"), ui_kit::Text::Note("render.physical_separate"));
            for spec in specifications {
                ui.label(format!("{}: {}", spec["catalog_id"].as_str().unwrap_or(""), spec["physical_role"].as_str().unwrap_or("")));
                ui.small(spec["reason"].as_str().unwrap_or(""));
                if let Some(references) = spec["references"].as_array() {
                    for reference in references {
                        ui.label(format!("{} - {}", reference["profile_id"].as_str().unwrap_or(""), reference["relation"].as_str().unwrap_or("")));
                    }
                }
            }
            if let Some(profiles) = detail["physical_references"]["candidate_profiles"].as_array() {
                if profiles.is_empty() {
                    ui_kit::text(ui, app.tr("render.physical_unknown"), ui_kit::Text::Note("render.physical_unknown"));
                }
                for profile in profiles {
                    let profile_id = profile["id"].as_str().unwrap_or("");
                    if ui_kit::button(ui, profile["name"].as_str().unwrap_or(profile_id), ui_kit::Button::Normal).clicked() {
                        linked_profile = Some(profile_id.to_string());
                    }
                }
            }
        });
    }
    if let Some(profile_id) = linked_profile {
        if let Err(error) = choose(app, view, "physical_profiles", &profile_id) {
            app.set_status(error, true);
        }
        // The next frame uses the newly captured resource. Do not combine the
        // previous surface's detail with the newly selected physical profile.
        return;
    }
    if view.kind != "appearances" {
        ui_kit::text(ui, app.tr("render.library_authored_help"), ui_kit::Text::Note("render.library_authored_help"));
    }
    match view.kind.as_str() {
        "appearances" => {
            target_controls(app, ui, view);
            if ui_kit::button(
                ui,
                app.tr("render.library_apply_appearance"),
                ui_kit::Button::Enabled(current_target(app, view) && source["available"] != false),
            )
            .clicked()
                && let Some(target) = &view.target
            {
                let params = if let Some(preset) = source["id"].as_str() {
                    json!({"bodies":[target.name],"appearance":preset})
                } else {
                    json!({"bodies":[target.name],"name":source["name"],"color":source["color"],"opacity":source["opacity"],"pbr":source["pbr"]})
                };
                match app.run("appearance.assign", params) {
                    Ok(_) => {
                        view.target_revision = None;
                        view.loaded = false;
                        app.set_status(app.tr("render.library_applied").to_string(), false);
                    }
                    Err(error) => app.set_status(error, true),
                }
            }
        }
        "native_scenes" => super::inspector::scene_resource(app, ui, id),
        "quality" => super::inspector::quality_resource(app, ui, id),
        "physical_profiles" => physical_controls(app, ui, view, id, &detail),
        "surfaces" | "selene_appearances" => surface_controls(app, ui, view, id),
        "hdr" | "rigs" => {
            let reason = light_reason(app, &view.kind);
            if let Some(reason) = &reason {
                ui_kit::text(ui, reason, ui_kit::Text::Note("render:light:reason"));
            }
            if ui_kit::button(ui, app.tr("render.stage_panels.apply"), ui_kit::Button::Enabled(reason.is_none())).clicked()
                && let Err(error) = review::light(app, view, id)
            {
                app.set_status(error, true);
            }
        }
        _ => {
            ui.colored_label(Tokens::get().warning, app.tr("render.library_specification_only"));
        }
    }
}

#[cfg(test)]
pub(super) fn saved_surface_controls(app: &mut SolveApp, ui: &mut egui::Ui) {
    let title = app.tr("render.saved_surfaces").to_string();
    ui_kit::section(ui, "render_saved_surfaces", &title, false, |ui| saved_surface_content(app, ui, None));
}

pub(super) fn saved_surface_target_controls(
    app: &mut SolveApp,
    ui: &mut egui::Ui,
    target: Option<&solidcraft_engine::doc::render_surface::RenderBodyTarget>,
) {
    if let Some(target) = target {
        saved_surface_content(app, ui, Some(target));
    }
}

fn saved_surface_content(app: &mut SolveApp, ui: &mut egui::Ui, target: Option<&solidcraft_engine::doc::render_surface::RenderBodyTarget>) {
    let bindings = app.render_workspace.view.draft.surfaces.clone();
    if !bindings.iter().any(|b| target.is_none_or(|t| t == &b.target)) {
        return;
    }
    {
        ui_kit::text(ui, app.tr("render.saved_surfaces_help"), ui_kit::Text::Note("render.saved_surfaces_help"));
        if !app.render_workspace.view.materials.bindings.is_empty() {
            ui_kit::text(ui, app.tr("render.saved_maps_runtime_conflict"), ui_kit::Text::Note("render.saved_maps_runtime_conflict"));
        }
        for (index, binding) in bindings.iter().enumerate().filter(|(_, b)| target.is_none_or(|t| t == &b.target)) {
            ui.label(format!(
                "{} - {} ({}): {:?}",
                binding.source.id, binding.target.definition, binding.target.feature, binding.target.occurrence_path
            ));
            if ui_kit::button(ui, app.tr("render.inspector_replace"), ui_kit::Button::Normal).clicked() {
                open_category(app, "surfaces");
            }
            let response = ui_kit::button(ui, app.tr("render.saved_surface_remove"), ui_kit::Button::Normal);
            crate::scenario::publish_handle(&format!("render:saved-surface-remove:{index}"), response.rect.center());
            if response.clicked() {
                let name = app.render_workspace.view.name.clone();
                // Remove from the saved authority without attempting to save an
                // incompatible draft (for example a deliberate CPU method change).
                match app.run("render.library.surface.unbind", json!({"name":name,"target":binding.target})) {
                    Ok(_) => setups::load(app),
                    Err(e) => app.set_status(e, true),
                }
            }
        }
    }
}

#[cfg(test)]
pub(super) fn stage_controls(app: &mut SolveApp, ui: &mut egui::Ui) {
    let title = app.tr("render.passive_stage").to_string();
    ui_kit::section(ui, "render_passive_stage", &title, false, |ui| stage_content(app, ui));
}
#[cfg(test)]
pub(super) fn stage_content(app: &mut SolveApp, ui: &mut egui::Ui) {
    ui_kit::text(ui, app.tr("render.passive_stage_help"), ui_kit::Text::Note("render.passive_stage_help"));
    let stage = &app.render_workspace.view.draft.stage;
    ui.label(format!("{}: {}", app.tr("render.stage_panel_count"), stage.len()));
    let empty = stage.is_empty();
    let candidate = patch(app);
    if ui_kit::button(
        ui,
        app.tr("render.add_ground"),
        ui_kit::Button::Enabled(empty && candidate.is_ok() && app.render_workspace.view.method != ImageMethod::Raster),
    )
    .clicked()
        && let Ok(set) = candidate
    {
        let name = app.render_workspace.view.name.clone();
        review::command(app, "render.stage.ground", json!({"name":name,"set":set,"replace_stage":true}));
    }
    let clear = ui_kit::button(ui, app.tr("render.clear_stage"), ui_kit::Button::Enabled(!empty && patch(app).is_ok()));
    crate::scenario::publish_target("render:stage-clear", clear.rect);
    if clear.clicked() {
        let name = app.render_workspace.view.name.clone();
        match patch(app) {
            Ok(set) => review::remove(app, "render.stage.clear", json!({"name":name,"set":set})),
            Err(error) => app.set_status(error, true),
        }
    }
}

fn physical_controls(app: &mut SolveApp, ui: &mut egui::Ui, view: &mut View, id: &str, detail: &Value) {
    ui_kit::text(ui, app.tr("render.physical_separate"), ui_kit::Text::Note("render.physical_separate"));
    let resource = &detail["resource"];
    ui.strong(resource["designation"].as_str().unwrap_or(id));
    ui.small(resource["reference_state"].as_str().unwrap_or(""));
    if let Some(properties) = resource["properties"].as_array() {
        for (index, property) in properties.iter().enumerate() {
            let name = property["quantity"].as_str().unwrap_or(crate::i18n::tr("render.property_fallback"));
            let title = format!("{}: {} {}", name, property["value"], property["unit"].as_str().unwrap_or(""));
            ui_kit::section(ui, ("physical_property", id, index), &title, false, |ui| {
                ui.small(crate::i18n::format(
                    "render.property_source",
                    &[("id", property["source_id"].as_str().unwrap_or("")), ("locator", property["source_locator"].as_str().unwrap_or(""))],
                ));
                ui.small(crate::i18n::format(
                    "render.property_qualifier",
                    &[("value", property["qualifier"].as_str().unwrap_or(crate::i18n::tr("render.property_unknown")))],
                ));
                ui.small(crate::i18n::format("render.property_conditions", &[("value", &property["conditions"].to_string())]));
                ui.small(crate::i18n::format("render.property_uncertainty", &[("value", &property["uncertainty"].to_string())]));
                ui.small(property["note"].as_str().unwrap_or(""));
            });
        }
    }
    if let Some(omissions) = detail["native_projection"]["not_projected"].as_array() {
        let title = app.tr("render.physical_omissions").to_string();
        ui_kit::section(ui, ("physical_omissions", id), &title, false, |ui| {
            for item in omissions {
                ui.small(format!("{}: {}", item["quantity"].as_str().unwrap_or(""), item["reason"].as_str().unwrap_or("")));
            }
        });
    }
    if let Some(sources) = detail["sources"].as_array() {
        for source in sources {
            if let Some(url) = source["url"].as_str() {
                ui.hyperlink_to(source["title"].as_str().unwrap_or(url), url);
            }
        }
    }
    ui.label(app.tr("render.physical_material_name"));
    ui_kit::bounded_cell_editor(ui, ui.id().with("physical_reference_name"), &mut view.physical_name, "", false, 128);
    if ui_kit::button(ui, app.tr("render.stage_panels.prepare_plan"), ui_kit::Button::Normal).clicked() {
        let result =
            app.run("render.library.physical.prepare", json!({"id":id,"name":view.physical_name,"acknowledge_reference":true})).and_then(|plan| {
                review::open(app, "physical", resource["designation"].as_str().unwrap_or(id).into(), plan.clone(), review::Apply::Physical(plan))
            });
        if let Err(error) = result {
            app.set_status(error, true);
        }
    }
}

fn short_availability<'a>(app: &'a SolveApp, status: Option<&str>) -> &'a str {
    app.tr(match status {
        Some("native_appearance") => "render.library_appearance_available",
        Some("native_composition") => "render.library_short_native",
        Some("implemented_pending_batch_validation") => "render.library_short_projection",
        Some("blocked") => "render.library_short_blocked",
        Some("specification_only") => "render.library_short_specification",
        _ => "render.library_short_reference",
    })
}

fn tile_badge<'a>(app: &'a SolveApp, entry: &Value) -> &'a str {
    app.tr(match entry.pointer("/projection/status").and_then(Value::as_str) {
        Some("native_appearance" | "native_composition") => "render.stage_panels.badge_native",
        Some("implemented_pending_batch_validation") => "render.stage_panels.badge_projection",
        Some("blocked") => "render.stage_panels.badge_blocked",
        Some("specification_only") => "render.stage_panels.badge_specification",
        None if entry["library_kind"] == "appearances" && entry["available"] != false => "render.stage_panels.badge_native",
        _ => "render.stage_panels.badge_reference",
    })
}

fn availability_label<'a>(app: &'a SolveApp, status: Option<&str>) -> &'a str {
    app.tr(match status {
        Some("native_appearance") => "render.library_appearance_available",
        Some("native_composition") => "render.availability_native",
        Some("implemented_pending_batch_validation") => "render.availability_projection",
        Some("blocked") => "render.library_blocked",
        Some("specification_only") => "render.library_specs",
        _ => "render.availability_reference",
    })
}

/// Focus the existing library card from the viewport Appearance menu.
pub(super) fn focus_library(app: &mut SolveApp) {
    let view = &mut app.render_workspace.view.library;
    view.collection = 0;
    switch_kind(view, "appearances");
}

pub(super) fn invalidate(app: &mut SolveApp) {
    app.render_workspace.view.library.loaded = false;
}

#[cfg(test)]
mod studio_browser_tests {
    use super::*;
    use crate::scenario::Harness;
    #[test]
    fn appearance_cards_select_inspect_switch_layout_and_keep_explicit_apply() {
        let mut h = Harness::new();
        h.app.run("solid.box", json!({"length":10,"width":10,"height":10,"body_name":"Sample"})).unwrap();
        h.app.run("appearance.assign", json!({"bodies":["Sample"],"appearance":"ABS - Red"})).unwrap();
        super::super::enter(&mut h.app, true).unwrap();
        h.frames(4);
        let before = h.app.session.doc.to_json();
        h.step(&json!({"click":{"handle":"render:library-scope:in-design"}})).unwrap();
        h.frames(3);
        assert_eq!(h.app.render_workspace.view.library.collection, 3);
        let id = appearance_id(&h.app.render_workspace.view.library.entries[0]);
        h.step(&json!({"click":{"handle":format!("render:library:{id}")}})).unwrap();
        h.frames(3);
        assert_eq!(h.app.render_workspace.view.library.selected.as_deref(), Some(id.as_str()));
        assert_eq!(h.app.render_workspace.view.inspector.page, super::super::inspector::Page::Appearance);
        assert_eq!(h.app.session.doc.to_json(), before, "browsing and selection must not apply appearances");
        // The selection-following editor takes precedence; library inspection is
        // available under Advanced when no model target is selected.
        h.app.run("select.clear", json!({})).unwrap();
        h.frames(3);
        // Clearing model selection opens Scene overview; inspect the resource again.
        h.step(&json!({"click":{"handle":format!("render:library:{id}")}})).unwrap();
        h.frames(3);
        assert_eq!(h.app.render_workspace.view.inspector.page, super::super::inspector::Page::Appearance);
        h.step(&json!({"click":{"handle":format!("dialog:section:{}", h.app.tr("render.inspector_advanced"))}})).unwrap();
        h.frames(3);
        h.step(&json!({"widget":h.app.tr("render.library_favorite")})).unwrap();
        assert!(h.app.preferences.render_library_favorites.contains(&("appearances".into(), id.clone())));
        h.step(&json!({"click":{"handle":"render:library-list"}})).unwrap();
        h.frames(3);
        assert_eq!(h.app.render_workspace.view.library.layout, 1);
        h.step(&json!({"click":{"handle":"render:library-grid"}})).unwrap();
        h.frames(3);
        assert_eq!(h.app.render_workspace.view.library.layout, 0);
        assert_eq!(h.app.session.doc.to_json(), before);
        let catalog = h.app.run("appearance.library", json!({})).unwrap();
        let blue = catalog["appearances"].as_array().unwrap().iter().find(|entry| entry["name"] == "ABS - Blue").unwrap();
        let mut view = std::mem::take(&mut h.app.render_workspace.view.library);
        choose(&mut h.app, &mut view, "appearances", &appearance_id(blue)).unwrap();
        refresh_targets(&mut h.app, &mut view).unwrap();
        view.target = view.targets.first().cloned();
        h.app.render_workspace.view.library = view;
        h.frames(3);
        assert_eq!(h.app.session.doc.to_json(), before);
        h.step(&json!({"widget":h.app.tr("render.library_apply_appearance")})).unwrap();
        assert_eq!(h.app.session.doc.appearances.bodies.get("Sample").unwrap().name, "ABS - Blue");
        if let Some(dir) = std::env::var_os("SOLIDCRAFT_LIBRARY_EVIDENCE") {
            std::fs::create_dir_all(&dir).unwrap();
            h.app.run("appearance.assign", json!({"bodies":["Sample"],"appearance":"Aluminum - Anodized Blue"})).unwrap();
            h.app.run("select.set", json!({"items":[solidcraft_engine::Sel::Body {name:"Sample".into()}]})).unwrap();
            h.app.render_workspace.view.library.collection = 0;
            h.app.render_workspace.view.library.loaded = false;
            let aluminum = h.app.run("appearance.library", json!({})).unwrap()["appearances"]
                .as_array()
                .unwrap()
                .iter()
                .find(|e| e["name"] == "Aluminum - Anodized Blue")
                .unwrap()
                .clone();
            let mut view = std::mem::take(&mut h.app.render_workspace.view.library);
            choose(&mut h.app, &mut view, "appearances", &appearance_id(&aluminum)).unwrap();
            h.app.render_workspace.view.library = view;
            for dark in [false, true] {
                h.app.preferences.theme = if dark { "dark" } else { "light" }.into();
                h.app.ui.dark = dark;
                let ctx = egui::Context::default();
                crate::fonts::setup_bundled_fonts(&ctx);
                let mut textures = std::collections::HashMap::new();
                let mut final_frame = None;
                for frame in 0..10 {
                    let mut output = ctx.run_ui(
                        egui::RawInput {
                            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1440.0, 900.0))),
                            time: Some(frame as f64 / 10.0),
                            focused: true,
                            ..Default::default()
                        },
                        |ui| {
                            h.app.logic(ui.ctx());
                            h.app.ui(ui);
                        },
                    );
                    ui_kit::update_test_textures(&mut textures, &output.textures_delta);
                    output.textures_delta.clear();
                    final_frame = Some(output);
                }
                let output = final_frame.unwrap();
                let image = ui_kit::raster_ui(&ctx, output.shapes, output.pixels_per_point, ctx.content_rect(), &textures);
                image.save(std::path::Path::new(&dir).join(if dark { "appearances-dark.png" } else { "appearances-light.png" })).unwrap();
                let card = h.app.shell.workspace_panels["render:appearances"].0;
                assert_eq!(card.width(), 266.0);
                let card_image = image::imageops::crop_imm(
                    &image,
                    card.left().round() as u32,
                    card.top().round() as u32,
                    card.width().round() as u32,
                    card.height().round() as u32,
                )
                .to_image();
                card_image.save(std::path::Path::new(&dir).join(if dark { "card-dark.png" } else { "card-light.png" })).unwrap();
                if let Some(reference) = std::env::var_os("SOLIDCRAFT_LIBRARY_REFERENCE") {
                    let reference = image::open(reference).unwrap().to_rgba8();
                    let reference = image::imageops::crop_imm(&reference, 8, 487, 266, 356).to_image();
                    let mut pair = image::RgbaImage::new(532, card_image.height().max(reference.height()));
                    image::imageops::overlay(&mut pair, &reference, 0, 0);
                    image::imageops::overlay(&mut pair, &card_image, 266, 0);
                    pair.save(std::path::Path::new(&dir).join(if dark {
                        "reference-left-current-right-dark.png"
                    } else {
                        "reference-left-current-right-light.png"
                    }))
                    .unwrap();
                }
            }
        }
    }
    #[test]
    fn stage_panel_titles_follow_stage_without_mutating_the_design() {
        let mut h = Harness::new();
        super::super::enter(&mut h.app, true).unwrap();
        let before = h.app.session.doc.to_json();
        for (stage, key) in [
            ("look", "render.appearances"),
            ("light", "render.studios_environments"),
            ("frame", "render.stage_frame"),
            ("output", "render.size_presets"),
        ] {
            h.app.run("view.render_stage", json!({"stage":stage})).unwrap();
            h.frames(3);
            assert_eq!(stage_title(h.app.render_workspace.view.stage), key);
            let browser = h.app.shell.workspace_panels["render:browser"].0;
            let panel = h.app.shell.workspace_panels["render:appearances"].0;
            let strip = h.app.shell.workspace_panels["render:shots_strip"].0;
            assert_eq!(panel.top() - browser.bottom(), Tokens::get().space_8, "{stage}: {browser:?} {panel:?} {strip:?}");
            assert_eq!(strip.top() - panel.bottom(), Tokens::get().space_8, "{stage}: {browser:?} {panel:?} {strip:?}");
        }
        assert_eq!(h.app.session.doc.to_json(), before);
    }

    #[test]
    fn applicable_resources_include_used_looks_and_selected_category_or_finish_before_the_rest() {
        let mut h = Harness::new();
        h.app.run("solid.box", json!({"length":10,"width":10,"height":10,"body_name":"Sample"})).unwrap();
        h.app.run("appearance.assign", json!({"bodies":["Sample"],"appearance":"ABS - Red"})).unwrap();
        h.app.session.selection = vec![solidcraft_engine::Sel::Body { name: "Sample".into() }];
        let selected = selected_looks(&mut h.app);
        assert_eq!(selected.len(), 1);
        let used = h.app.run("appearance.list", json!({})).unwrap()["in_design"].as_array().unwrap().clone();
        let selected_match = json!({"name":"ABS - Blue","color":"#123456","opacity":1.0});
        let finish_match = json!({"name":"Unclassified finish","color":"#123456","opacity":1.0});
        let unrelated = json!({"name":"Wood","color":"#123456","opacity":1.0,"pbr":{"metallic":0.0,"roughness":0.95}});
        assert!(applicable(&selected_match, &selected, &used));
        assert!(applicable(&finish_match, &selected, &used));
        assert!(!applicable(&unrelated, &selected, &used));
        let mut entries = [unrelated.clone(), selected_match.clone(), used[0].clone(), finish_match.clone()];
        entries.sort_by_key(|entry| !applicable(entry, &selected, &used));
        assert_eq!(entries, [selected_match, used[0].clone(), finish_match, unrelated]);
        assert!(applicable(&used[0], &[], &used), "used looks remain applicable with nothing selected");
    }

    #[test]
    fn look_tile_click_applies_to_selection_through_assign_and_undo() {
        let mut h = Harness::new();
        h.app.run("solid.box", json!({"length":10,"width":10,"height":10,"body_name":"Sample"})).unwrap();
        h.app.session.selection = vec![solidcraft_engine::Sel::Body { name: "Sample".into() }];
        super::super::enter(&mut h.app, true).unwrap();
        h.frames(4);
        let blue = h.app.render_workspace.view.library.entries.iter().find(|e| e["name"] == "ABS - Blue").unwrap().clone();
        h.app.render_workspace.view.library.query = "ABS - Blue".into();
        h.frames(3);
        let undo = h.app.session.undo.len();
        h.step(&json!({"click":{"handle":format!("render:library:{}",appearance_id(&blue))}})).unwrap();
        assert_eq!(h.app.session.doc.appearances.bodies["Sample"].name, "ABS - Blue");
        assert_eq!(h.app.session.undo.len(), undo + 1);
        h.app.run("edit.undo", json!({})).unwrap();
        assert!(!h.app.session.doc.appearances.bodies.contains_key("Sample"));
    }

    #[test]
    fn light_tiles_select_then_review_and_apply_with_one_undo() {
        let mut h = Harness::new();
        h.app.run("solid.box", json!({"length":20,"width":15,"height":10,"body_name":"Sample"})).unwrap();
        super::super::enter(&mut h.app, true).unwrap();
        let unchanged = h.app.session.doc.to_json();
        assert!(light_reason(&h.app, "native_scenes").is_some());
        super::super::select_image_method(&mut h.app, ImageMethod::GpuRaytrace);
        h.app.run("view.render_stage", json!({"stage":"light"})).unwrap();
        h.frames(4);
        let undo = h.app.session.undo.len();
        h.step(&json!({"click":{"handle":"render:light:NATIVE-STUDIO-001"}})).unwrap();
        assert_eq!(h.app.session.doc.to_json(), unchanged);
        h.step(&json!({"click":{"handle":"render:light:review"}})).unwrap();
        assert!(h.app.render_workspace.view.library.review.is_some());
        assert_eq!(h.app.session.undo.len(), undo);
        h.step(&json!({"click":{"handle":"render:review:acknowledge"}})).unwrap();
        h.step(&json!({"click":{"handle":"dialog:ok"}})).unwrap();
        assert_eq!(h.app.session.undo.len(), undo + 1);
        assert!(!h.app.render_workspace.view.draft.stage.is_empty());
        h.app.run("edit.undo", json!({})).unwrap();
        assert_eq!(h.app.session.doc.to_json(), unchanged);
    }

    #[test]
    fn tile_finishes_follow_pbr_and_opacity_and_authored_surface_color() {
        for (opacity, metallic, roughness, expected) in [
            (1.0, 0.0, 0.9, ui_kit::FinishKind::Matte),
            (1.0, 0.0, 0.5, ui_kit::FinishKind::Satin),
            (1.0, 0.0, 0.1, ui_kit::FinishKind::Gloss),
            (1.0, 1.0, 0.1, ui_kit::FinishKind::Metal),
            (0.4, 1.0, 0.9, ui_kit::FinishKind::Glass),
        ] {
            let sphere = tile_sphere(&json!({"color":"#123456","opacity":opacity,"pbr":{"metallic":metallic,"roughness":roughness}}));
            assert_eq!(sphere.kind, expected);
            assert_eq!(sphere.color, egui::Color32::from_rgb(0x12, 0x34, 0x56));
            assert_eq!(sphere.opacity, Some(opacity as f32));
        }
        let source = json!({"resource":{"base_linear_low":[0.0,0.5,1.0],"base_linear_high":[0.0,0.5,1.0],"metallic":1.0,"roughness":0.4}});
        assert_eq!(tile_sphere(&source).color, egui::Color32::from_rgb(0, 188, 255));
        let selene = json!({"resource":{"baseline_projection":{"runtime_parameters":{"color":[20,40,60],"opacity":1.0,"pbr":{"metallic":1.0,"roughness":0.3}}}}});
        assert_eq!(tile_sphere(&selene).color, egui::Color32::from_rgb(20, 40, 60));
        assert_eq!(tile_sphere(&selene).kind, ui_kit::FinishKind::Metal);
    }
    #[test]
    fn scopes_search_categories_and_availability_filter_real_catalog_entries() {
        let mut h = Harness::new();
        h.app.run("solid.box", json!({"length":10,"width":10,"height":10,"body_name":"Sample"})).unwrap();
        h.app.run("appearance.assign", json!({"bodies":["Sample"],"appearance":"ABS - Red"})).unwrap();
        let mut view = View { collection: 3, ..Default::default() };
        refresh(&mut h.app, &mut view).unwrap();
        assert_eq!(view.entries.len(), 1);
        assert_eq!(user_count(&view.entries[0]), 1);
        assert_eq!(user_count(&json!({"bodies":["A"],"faces":[{"body":"B"}],"components":["C"]})), 3);
        assert!(matches_entry(&view.entries[0], "rEd", "", "usable"));
        assert!(!matches_entry(&view.entries[0], "blue", "", "all"));
        assert!(matches_entry(&json!({"name":"Coating","resource":{"tags":["Enclosure"]}}), "enclosure", "", "all"));
        let id = appearance_id(&view.entries[0]);
        choose(&mut h.app, &mut view, "appearances", &id).unwrap();
        h.app.preferences.render_library_favorites.insert(("appearances".into(), id));
        for scope in [1, 2] {
            view.collection = scope;
            refresh(&mut h.app, &mut view).unwrap();
            assert_eq!(view.entries.len(), 1);
            assert_eq!(view.entries[0]["name"], "ABS - Red");
        }
        view.collection = 0;
        refresh(&mut h.app, &mut view).unwrap();
        assert!(view.entries.len() > 10);
        assert!(view.entries.iter().any(|entry| matches_entry(entry, "aBs", "ABS", "all")));
        assert!(!view.entries.iter().any(|entry| matches_entry(entry, "ABS", "missing category", "all")));
        assert!(view.entries.iter().filter(|entry| matches_entry(entry, "", "", "blocked")).all(|entry| entry["available"] == false));
        let source = h.app.run("render.library.describe", json!({"kind":"surfaces","id":"SURF-001"})).unwrap();
        let entry = json!({"id":"SURF-001","name":source["resource"]["name"],"resource":source["resource"]});
        h.app.run("render.library.appearance.apply", json!({"id":"SURF-001","variant":"uniform_opaque","bodies":["Sample"]})).unwrap();
        let reply = h.app.run("appearance.list", json!({})).unwrap();
        assert_eq!(user_count(usage_for(&entry, reply["in_design"].as_array().unwrap()).unwrap()), 1);
    }
    #[test]
    fn preferences_round_trip_favorites_and_recent_without_saving_them_in_design() {
        let mut h = Harness::new();
        let mut view = View::default();
        let before = h.app.session.doc.to_json();
        choose(&mut h.app, &mut view, "surfaces", "SURF-001").unwrap();
        h.app.preferences.render_library_favorites.insert(("surfaces".into(), "SURF-001".into()));
        let saved = serde_json::to_value(&h.app.preferences).unwrap();
        let prefs = crate::prefs::lenient(&saved);
        assert_eq!(prefs.render_library_favorites, h.app.preferences.render_library_favorites);
        assert_eq!(prefs.render_library_recent, h.app.preferences.render_library_recent);
        assert_eq!(h.app.session.doc.to_json(), before);
        let custom = json!({"name":"Workshop look","color":"#123456","opacity":0.5});
        let id = appearance_id(&custom);
        let detail = describe(&mut h.app, "appearances", &id).unwrap();
        assert_eq!(detail["resource"]["color"], custom["color"]);
        assert!(crate::prefs::lenient(&json!({})).render_library_favorites.is_empty());
        let hostile = json!({"render_library_recent":vec![("surfaces","SURF-001");100],"render_library_favorites":[["surfaces",""]]});
        let prefs = crate::prefs::lenient(&hostile);
        assert_eq!(prefs.render_library_recent.len(), 32);
        assert!(prefs.render_library_favorites.is_empty());
    }
    #[test]
    fn favorites_and_recent_keep_exact_status_without_promoting_specs_to_runtime() {
        let mut h = Harness::new();
        let before = h.app.session.doc.to_json();
        let mut view = View { kind: "surfaces".into(), ..Default::default() };
        choose(&mut h.app, &mut view, "native_scenes", "NATIVE-STUDIO-002").unwrap();
        let native = ("native_scenes".into(), "NATIVE-STUDIO-002".into());
        h.app.preferences.render_library_favorites.insert(native.clone());
        choose(&mut h.app, &mut view, "scenes", "SCENE-001").unwrap();
        assert_eq!(view.statuses.get(&("scenes".into(), "SCENE-001".into())).map(String::as_str), Some("specification_only"));
        for i in 1..=48 {
            choose(&mut h.app, &mut view, "surfaces", &format!("SURF-{i:03}")).unwrap();
        }
        assert_eq!(h.app.preferences.render_library_recent.len(), 32);
        assert_eq!(view.statuses.len(), 33);
        assert_eq!(view.statuses.get(&native).map(String::as_str), Some("native_composition"));
        assert_eq!(h.app.session.doc.to_json(), before);
        assert!(h.app.session.undo.is_empty());
    }
    #[test]
    fn physical_reference_navigation_uses_the_same_category_history() {
        let mut h = Harness::new();
        let mut view = View { kind: "surfaces".into(), ..Default::default() };
        choose(&mut h.app, &mut view, "surfaces", "SURF-001").unwrap();
        view.query = "brushed".into();
        view.offset = 48;
        let profiles = h.app.run("render.library.list", json!({"kind":"physical_profiles","limit":1})).unwrap();
        let id = profiles["items"][0]["id"].as_str().unwrap();
        choose(&mut h.app, &mut view, "physical_profiles", id).unwrap();
        assert!(!view.loaded);
        switch_kind(&mut view, "surfaces");
        assert_eq!(view.query, "brushed");
        assert_eq!(view.offset, 48);
        assert_eq!(view.selected.as_deref(), Some("SURF-001"));
    }
    #[test]
    fn category_navigation_preserves_queries_pages_selection_and_declared_scope() {
        let mut view = View {
            kind: "surfaces".into(),
            query: "brushed".into(),
            offset: 48,
            selected: Some("SURF-001".into()),
            details: Some(json!({"resource":{"id":"SURF-001"}})),
            availability: "usable".into(),
            ..Default::default()
        };
        switch_kind(&mut view, "hdr");
        assert!(view.query.is_empty());
        assert!(view.selected.is_none());
        view.query = "studio".into();
        switch_kind(&mut view, "surfaces");
        assert_eq!(view.query, "brushed");
        assert_eq!(view.offset, 48);
        assert_eq!(view.selected.as_deref(), Some("SURF-001"));
        assert_eq!(view.availability, "usable");
        switch_kind(&mut view, "invalid");
        assert_eq!(view.kind, "surfaces");
        switch_kind(&mut view, "hdr");
        assert_eq!(view.query, "studio");
    }
    #[test]
    fn choosing_resources_is_read_only_and_recent_history_is_bounded() {
        let mut h = Harness::new();
        let before = h.app.session.doc.to_json();
        let mut view = View::default();
        for n in 1..=48 {
            choose(&mut h.app, &mut view, "surfaces", &format!("SURF-{n:03}")).unwrap();
        }
        assert_eq!(h.app.preferences.render_library_recent.len(), 32);
        assert_eq!(h.app.session.doc.to_json(), before);
        assert!(h.app.session.undo.is_empty());
        choose(&mut h.app, &mut view, "surfaces", "SURF-048").unwrap();
        assert_eq!(h.app.preferences.render_library_recent.len(), 32);
        assert_eq!(h.app.render_workspace.view.inspector.page, super::super::inspector::Page::Appearance);
        choose(&mut h.app, &mut view, "native_scenes", "NATIVE-STUDIO-002").unwrap();
        assert_eq!(h.app.render_workspace.view.inspector.page, super::super::inspector::Page::Scene);
        assert_eq!(h.app.session.doc.to_json(), before);
    }
}
