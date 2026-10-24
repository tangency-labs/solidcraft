//! Existing Render-panel library browser. Engine commands own catalog resolution,
//! assignments and saved resources; this view owns only search and request drafts.
use super::{ImageMethod, materials, setups};
use crate::{SolveApp, theme::Tokens, ui_kit};
use serde_json::{Value, json};
use solidcraft_engine::{doc::Document, geom::Vec3};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
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
    target_revision: Option<u64>,
    target_document: Option<Weak<Document>>,
    acknowledge_projection: bool,
    replace_environment: bool,
    replace_studio: bool,
    replace_surface: bool,
    include_cards: bool,
    physical_name: String,
    physical_ack: bool,
    physical_plan: Option<Value>,
    center: [f64; 3],
    extent: f64,
    availability: String,
    collection: usize,
    collection_query: String,
    navigation: BTreeMap<String, Navigation>,
    favorites: BTreeSet<(String, String)>,
    recent: VecDeque<(String, String)>,
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
            kind: "surfaces".into(),
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
            target_revision: None,
            target_document: None,
            acknowledge_projection: false,
            replace_environment: false,
            replace_studio: false,
            replace_surface: false,
            include_cards: true,
            physical_name: "Reference material".into(),
            physical_ack: false,
            physical_plan: None,
            center: [0.0; 3],
            extent: 1000.0,
            availability: "all".into(),
            collection: 0,
            collection_query: String::new(),
            navigation: Default::default(),
            favorites: Default::default(),
            recent: Default::default(),
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
    let reply = app.run("render.library.list", json!({"kind":view.kind,"query":view.query,"offset":view.offset,"limit":48,"availability":if view.kind=="physical_profiles" {"all"} else {&view.availability}}))?;
    view.entries = reply["items"].as_array().cloned().ok_or_else(|| "invalid render-library response".to_string())?;
    view.total = reply["total"].as_u64().and_then(|n| usize::try_from(n).ok()).unwrap_or(0);
    view.next = reply["next_offset"].as_u64().and_then(|n| usize::try_from(n).ok());
    view.loaded = true;
    // Search/page changes preserve the independently inspected resource.
    view.acknowledge_projection = false;
    view.physical_ack = false;
    view.physical_plan = None;
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
    view.preview = None;
    view.preview_error = None;
    view.physical_plan = None;
    view.acknowledge_projection = false;
    view.physical_ack = false;
}
pub(super) fn open_category(app: &mut SolveApp, kind: &str) {
    switch_kind(&mut app.render_workspace.view.library, kind);
    app.render_workspace.view.library.collection = 0;
    app.render_workspace.view.inspector.library_open = true;
    app.ui.show_browser = true;
    app.tree.panel_collapsed = false;
}
fn choose(app: &mut SolveApp, view: &mut View, kind: &str, id: &str) -> Result<(), String> {
    switch_kind(view, kind);
    let detail = app.run("render.library.describe", json!({"kind":kind,"id":id}))?;
    let display = detail["resource"]["name"].as_str().or_else(|| detail["resource"]["designation"].as_str()).unwrap_or(id).to_string();
    let status = detail["projection"]["status"].as_str().unwrap_or("reference_only").to_string();
    view.details = Some(detail);
    view.selected = Some(id.into());
    view.preview = None;
    view.preview_error = None;
    view.acknowledge_projection = false;
    view.physical_ack = false;
    view.physical_plan = None;
    app.render_workspace.view.inspector.clear_resource_reviews();
    let key = (kind.to_string(), id.to_string());
    view.recent.retain(|old| old != &key);
    view.recent.push_front(key.clone());
    view.recent.truncate(32);
    view.statuses.insert(key.clone(), status);
    view.labels.insert(key, display);
    view.labels.retain(|key, _| view.favorites.contains(key) || view.recent.contains(key));
    view.statuses.retain(|key, _| view.favorites.contains(key) || view.recent.contains(key));
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

/// The existing browser card owns layout and occlusion in both shell modes.
pub(super) fn browse(app: &mut SolveApp, ui: &mut egui::Ui) {
    let mut view = std::mem::take(&mut app.render_workspace.view.library);
    let label = KINDS.iter().find(|(kind, _)| *kind == view.kind).map_or("", |(_, key)| app.tr(key)).to_string();
    let menu = ui_kit::dropdown_menu(ui, "render_library_kind", &label, |ui| {
        for &(kind, key) in KINDS {
            let r = ui_kit::list_picker(ui, kind, app.tr(key), view.kind == kind);
            crate::scenario::publish_handle(&format!("render:library-kind:{kind}"), r.rect.center());
            if r.clicked() {
                switch_kind(&mut view, kind);
                view.collection = 0;
                ui.close();
            }
        }
    });
    crate::scenario::publish_target("render:library-category", menu.rect);
    let collection_labels =
        [app.tr("render.library_all").to_string(), app.tr("render.library_favorites").to_string(), app.tr("render.library_recent").to_string()];
    ui_kit::segmented(ui, &mut view.collection, &collection_labels.iter().map(String::as_str).collect::<Vec<_>>(), Default::default());
    let query = if view.collection == 0 { &mut view.query } else { &mut view.collection_query };
    let search = ui_kit::bounded_cell_editor(ui, ui.id().with("render_library_search"), query, app.tr("render.library_search"), false, 128);
    crate::scenario::publish_target("render:library-search", search.rect);
    let enter = search.has_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
    ui.horizontal(|ui| {
        if (ui_kit::button(ui, app.tr("render.library_search"), ui_kit::Button::Normal).clicked() || enter) && view.collection == 0 {
            view.offset = 0;
            view.loaded = false;
        }
        if ui_kit::button(ui, app.tr("render.library_clear_search"), ui_kit::Button::Normal).clicked() {
            if view.collection == 0 {
                view.query.clear();
                view.offset = 0;
                view.loaded = false;
            } else {
                view.collection_query.clear();
            }
        }
    });
    if view.collection == 0 && view.kind != "physical_profiles" {
        let choices = [
            ("all", "render.library_all_states"),
            ("usable", "render.library_usable"),
            ("specification", "render.library_specs"),
            ("blocked", "render.library_blocked"),
        ];
        let label = choices.iter().find(|(id, _)| *id == view.availability).map_or("", |(_, key)| app.tr(key)).to_string();
        ui_kit::dropdown_menu(ui, "render_library_availability", &label, |ui| {
            for (id, key) in choices {
                if ui_kit::list_picker(ui, id, app.tr(key), view.availability == id).clicked() {
                    view.availability = id.into();
                    view.offset = 0;
                    view.loaded = false;
                    ui.close();
                }
            }
        });
    }
    if !view.loaded
        && let Err(error) = refresh(app, &mut view)
    {
        view.loaded = true;
        app.set_status(error, true);
    }
    let entries: Vec<(String, String, String, Value)> = if view.collection == 0 {
        view.entries
            .iter()
            .filter_map(|entry| {
                Some((
                    view.kind.clone(),
                    entry["id"].as_str()?.to_string(),
                    entry["name"].as_str().unwrap_or("").to_string(),
                    entry["projection"].clone(),
                ))
            })
            .collect()
    } else {
        let keys: Vec<_> = if view.collection == 1 { view.favorites.iter().cloned().collect() } else { view.recent.iter().cloned().collect() };
        keys.into_iter()
            .filter(|key| {
                format!("{} {} {}", key.0, key.1, view.labels.get(key).map(String::as_str).unwrap_or(""))
                    .to_lowercase()
                    .contains(&view.collection_query.to_lowercase())
            })
            .map(|key| {
                let label = view.labels.get(&key).cloned().unwrap_or_else(|| key.1.clone());
                let projection = json!({"status":view.statuses.get(&key).map(String::as_str).unwrap_or("reference_only")});
                (key.0, key.1, label, projection)
            })
            .collect()
    };
    ui.small(format!("{}: {}", app.tr("render.library_results"), if view.collection == 0 { view.total } else { entries.len() }));
    if entries.is_empty() {
        ui_kit::text(ui, app.tr("render.library_empty"), ui_kit::Text::Note("render.library_empty"));
    }
    let mut selected = None;
    let t = Tokens::get();
    let note_height = ui
        .painter()
        .layout(app.tr("render.library_inspect_only").to_string(), egui::FontId::proportional(t.font_small), t.form_dim, ui.available_width())
        .size()
        .y
        + t.space_8;
    let footer_height = note_height + if view.collection == 0 { t.control_height + t.form_gap_y } else { 0.0 };
    let height = (ui.available_height() - footer_height - t.space_10).max(t.control_height);
    let row_height = crate::ui_kit::row_height() + Tokens::get().space_10;
    egui::ScrollArea::vertical()
        .id_salt(("render_library_items", &view.kind, view.collection))
        .max_height(height)
        .auto_shrink([false, false])
        .show_rows(ui, row_height, entries.len(), |ui, range| {
            for index in range {
                let Some((kind, id, name, projection)) = entries.get(index) else { continue };
                let text = if name.is_empty() { id.as_str() } else { name.as_str() };
                let identity = format!("{kind}:{id}");
                // Reuse Nathan's two-line search/menu component, artwork and
                // accessibility metadata. A blocked resource remains inspectable.
                let icon = if matches!(kind.as_str(), "surfaces" | "materials" | "selene_appearances" | "physical_profiles") {
                    "appearance"
                } else {
                    "render_scene"
                };
                let mut item = ui_kit::MenuItem::new(&identity, text, icon);
                item.highlighted = view.kind == *kind && view.selected.as_ref() == Some(id);
                let path = format!("{id} / {}", short_availability(app, projection["status"].as_str()));
                let query = if view.collection == 0 { &view.query } else { &view.collection_query };
                let r = item.search_row(ui, query, &path, None);
                crate::scenario::publish_target(&format!("render:library:{id}"), r.rect);
                let selected_here = r.clicked();
                let _ = r.on_hover_text(format!(
                    "{text}\n{id}\n{}\n{}",
                    availability_label(app, projection["status"].as_str()),
                    app.tr("render.library_inspect_only")
                ));
                if selected_here {
                    selected = Some((kind.clone(), id.clone()));
                }
            }
        });
    if view.collection == 0 {
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
    if let Some((kind, id)) = selected
        && let Err(error) = choose(app, &mut view, &kind, &id)
    {
        app.set_status(error, true);
    }
    ui_kit::text(ui, app.tr("render.library_inspect_only"), ui_kit::Text::Note("render.library_inspect_only"));
    app.render_workspace.view.library = view;
}

pub(super) fn selected_kind(app: &SolveApp, kind: &str) -> bool {
    let view = &app.render_workspace.view.library;
    view.kind == kind && view.selected.is_some()
}

pub(super) fn inspect_kinds(app: &mut SolveApp, ui: &mut egui::Ui, kinds: &[&str]) {
    if kinds.contains(&app.render_workspace.view.library.kind.as_str()) {
        inspect(app, ui);
    } else {
        ui_kit::text(ui, app.tr("render.library_select_hint"), ui_kit::Text::Note("render.library_select_hint"));
    }
}
pub(super) fn inspect(app: &mut SolveApp, ui: &mut egui::Ui) {
    let mut view = std::mem::take(&mut app.render_workspace.view.library);
    if let Some(id) = view.selected.clone() {
        ui.strong(view.details.as_ref().and_then(|d| d["resource"]["name"].as_str()).unwrap_or(&id));
        ui.small(&id);
        // Native composition is an actionable review, not a long specification
        // reader: keep Resolve/consent/Apply ahead of ancillary resource metadata.
        let native_composition = view.kind == "native_scenes";
        if native_composition {
            super::inspector::scene_resource(app, ui, &id);
        }
        let key = (view.kind.clone(), id.clone());
        let favorite = view.favorites.contains(&key);
        if ui_kit::button(
            ui,
            app.tr(if favorite { "render.library_unfavorite" } else { "render.library_favorite" }),
            ui_kit::Button::Enabled(favorite || view.favorites.len() < 256),
        )
        .clicked()
        {
            if favorite {
                view.favorites.remove(&key);
            } else {
                view.favorites.insert(key);
            }
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
                    ui.small(format!("{}: {}", app.tr("render.library_omitted"), item.as_str().unwrap_or("unknown")));
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
    app.render_workspace.view.library = view;
}
fn source_preview(app: &mut SolveApp, ui: &mut egui::Ui, view: &mut View, id: &str) {
    if view.preview.as_ref().is_none_or(|preview| preview.id != id)
        && view.preview_error.is_none()
        && ui_kit::button(ui, app.tr("render.library_source_preview"), ui_kit::Button::Normal).clicked()
    {
        let result = (|| -> Result<SourcePreview, String> {
            let reply = app.run("render.library.preview", json!({"id":id}))?;
            let bytes: Vec<u8> = serde_json::from_value(reply["bytes"].clone()).map_err(|e| e.to_string())?;
            if bytes.len() > 2 * 1024 * 1024 {
                return Err("source preview exceeds its byte envelope".into());
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
        ui.add(egui::Image::new(&preview.texture).max_size(egui::vec2(ui.available_width(), 180.0)).shrink_to_fit());
        ui_kit::text(ui, app.tr("render.library_source_preview_help"), ui_kit::Text::Note("render.library_source_preview_help"));
    }
    if let Some(error) = &view.preview_error {
        ui.small(error);
    }
}
pub(super) fn controls(app: &mut SolveApp, ui: &mut egui::Ui) {
    stage_controls(app, ui);
    saved_surface_controls(app, ui);
    let title = app.tr("render.library_title").to_string();
    ui_kit::section(ui, "render_resource_library", &title, false, |ui| {
        browse(app, ui);
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
    ui_kit::text(ui, app.tr("render.library_authored_help"), ui_kit::Text::Note("render.library_authored_help"));
    match view.kind.as_str() {
        "native_scenes" => super::inspector::scene_resource(app, ui, id),
        "quality" => super::inspector::quality_resource(app, ui, id),
        "physical_profiles" => physical_controls(app, ui, view, id, &detail),
        "surfaces" | "selene_appearances" => {
            target_controls(app, ui, view);
            ui_kit::checkbox(ui, app.tr("render.library_uniform_ack"), &mut view.acknowledge_projection);
            if ui_kit::button(
                ui,
                app.tr("render.library_apply_uniform"),
                ui_kit::Button::Enabled(current_target(app, view) && view.acknowledge_projection),
            )
            .clicked()
                && let Some(target) = view.target.clone()
            {
                match app.run("render.library.appearance.apply", json!({"id":id,"variant":"uniform_opaque","bodies":[target.name]})) {
                    Ok(_) => {
                        view.target_revision = None;
                        app.set_status(app.tr("render.library_applied").to_string(), false);
                    }
                    Err(e) => app.set_status(e, true),
                }
            }
            if view.kind == "surfaces" {
                ui_kit::text(ui, app.tr("render.library_maps_help"), ui_kit::Text::Note("render.library_maps_help"));
                ui_kit::checkbox(ui, app.tr("render.replace_saved_surface"), &mut view.replace_surface);
                ui.label(app.tr("render.mapping_scale"));
                ui_kit::number_field(ui, &mut view.mapping_scale, 0.001..=1000.0, 0.01, " x");
                ui_kit::segmented(ui, &mut view.mapping_plane, &["XY", "YZ", "ZX"], Default::default());
                let candidate = patch(app);
                let enabled = current_target(app, view) && app.render_workspace.view.method == ImageMethod::GpuRaytrace && candidate.is_ok();
                if ui_kit::button(ui, app.tr("render.library_save_maps"), ui_kit::Button::Enabled(enabled)).clicked()
                    && let (Some(target), Ok(set)) = (view.target.clone(), candidate)
                {
                    let name = app.render_workspace.view.name.clone();
                    match app.run("render.library.surface.bind",json!({"name":name,"set":set,"surface":{"id":id,"body_index":target.ordinal,"body_name":target.name,"scale":view.mapping_scale,"plane":(["xy","yz","zx"].get(view.mapping_plane).copied().unwrap_or("xy"))},"replace":view.replace_surface})) {
                        Ok(_)=>{setups::load(app);view.target_revision=None;app.set_status(app.tr("render.library_applied").to_string(),false);},
                        Err(e)=>app.set_status(e,true),
                    }
                }

                let enabled = current_target(app, view) && app.render_workspace.view.method == ImageMethod::GpuRaytrace;
                if ui_kit::button(ui, app.tr("render.library_apply_maps"), ui_kit::Button::Enabled(enabled)).clicked()
                    && let Some(target) = view.target.clone()
                {
                    match materials::install_library_surface(
                        app,
                        id,
                        target,
                        ["xy", "yz", "zx"].get(view.mapping_plane).copied().unwrap_or("xy"),
                        view.mapping_scale,
                    ) {
                        Ok(()) => app.set_status(app.tr("render.library_applied").to_string(), false),
                        Err(e) => app.set_status(e, true),
                    }
                }
            }
        }
        "hdr" => {
            ui_kit::text(ui, app.tr("render.library_hdr_help"), ui_kit::Text::Note("render.library_hdr_help"));
            ui_kit::checkbox(ui, app.tr("render.library_replace_environment"), &mut view.replace_environment);
            let candidate = patch(app);
            if let Err(e) = &candidate {
                ui.small(e);
            }
            let enabled = candidate.is_ok() && app.render_workspace.view.method != ImageMethod::Raster;
            if ui_kit::button(ui, app.tr("render.library_apply_hdr"), ui_kit::Button::Enabled(enabled)).clicked()
                && let Ok(set) = candidate
            {
                let name = app.render_workspace.view.name.clone();
                match app
                    .run("render.library.environment.apply", json!({"id":id,"name":name,"set":set,"replace_environment":view.replace_environment}))
                {
                    Ok(_) => {
                        setups::load(app);
                        app.set_status(app.tr("render.library_applied").to_string(), false);
                    }
                    Err(e) => app.set_status(e, true),
                }
            }
        }
        "rigs" => {
            let blocked = detail["projection"]["status"] == "blocked";
            if blocked {
                ui.colored_label(Tokens::get().warning, app.tr("render.library_blocked"));
                return;
            }
            ui_kit::text(ui, app.tr("render.library_rig_help"), ui_kit::Text::Note("render.library_rig_help"));
            if ui_kit::button(ui, app.tr("render.library_scene_bounds"), ui_kit::Button::Normal).clicked()
                && let Err(e) = refresh_targets(app, view)
            {
                app.set_status(e, true);
            }
            ui.label(app.tr("render.library_center"));
            for c in &mut view.center {
                ui_kit::number_field(ui, c, -1e12..=1e12, 1.0, " mm");
            }
            ui.label(app.tr("render.library_extent"));
            ui_kit::number_field(ui, &mut view.extent, 0.001..=1e6, 1.0, " mm");
            let cards = detail["projection"]["passive_cards"].as_u64().unwrap_or(0);
            if cards > 0 {
                ui_kit::checkbox(ui, app.tr("render.include_rig_cards"), &mut view.include_cards);
                if !view.include_cards {
                    ui_kit::checkbox(ui, app.tr("render.library_emitters_only_ack"), &mut view.acknowledge_projection);
                }
            }
            ui_kit::checkbox(ui, app.tr("render.library_replace_studio"), &mut view.replace_studio);
            let candidate = patch(app);
            let enabled = !blocked
                && (cards == 0 || view.include_cards || view.acknowledge_projection)
                && view.replace_studio
                && candidate.is_ok()
                && app.render_workspace.view.method == ImageMethod::GpuRaytrace;
            if ui_kit::button(ui, app.tr("render.library_apply_studio"), ui_kit::Button::Enabled(enabled)).clicked()
                && let Ok(set) = candidate
            {
                let name = app.render_workspace.view.name.clone();
                let rig = json!({"id":id,"variant":if cards==0 || view.include_cards {"complete"} else {"emitters_only"},"center_mm":view.center,"subject_extent_mm":view.extent});
                match app.run(
                    "render.library.studio.apply",
                    json!({"rig":rig,"name":name,"set":set,"replace_lighting_and_environment":true,"replace_stage":true}),
                ) {
                    Ok(_) => {
                        setups::load(app);
                        app.set_status(app.tr("render.library_applied").to_string(), false);
                    }
                    Err(e) => app.set_status(e, true),
                }
            }
        }
        _ => {
            ui.colored_label(Tokens::get().warning, app.tr("render.library_specification_only"));
        }
    }
}

pub(super) fn saved_surface_controls(app: &mut SolveApp, ui: &mut egui::Ui) {
    let title = app.tr("render.saved_surfaces").to_string();
    let bindings = app.render_workspace.view.draft.surfaces.clone();
    if bindings.is_empty() {
        return;
    }
    ui_kit::section(ui, "render_saved_surfaces", &title, false, |ui| {
        ui_kit::text(ui, app.tr("render.saved_surfaces_help"), ui_kit::Text::Note("render.saved_surfaces_help"));
        for (index, binding) in bindings.iter().enumerate() {
            ui.label(format!(
                "{} - {} ({}): {:?}",
                binding.source.id, binding.target.definition, binding.target.feature, binding.target.occurrence_path
            ));
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
    });
}

pub(super) fn stage_controls(app: &mut SolveApp, ui: &mut egui::Ui) {
    let title = app.tr("render.passive_stage").to_string();
    ui_kit::section(ui, "render_passive_stage", &title, false, |ui| stage_content(app, ui));
}
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
        match app.run("render.stage.ground", json!({"name":name,"set":set})) {
            Ok(_) => setups::load(app),
            Err(e) => app.set_status(e, true),
        }
    }
    let clear = ui_kit::button(ui, app.tr("render.clear_stage"), ui_kit::Button::Enabled(!empty && patch(app).is_ok()));
    crate::scenario::publish_target("render:stage-clear", clear.rect);
    if clear.clicked() {
        let name = app.render_workspace.view.name.clone();
        match patch(app).and_then(|set| app.run("render.stage.clear", json!({"name":name,"set":set})).map(|_| ())) {
            Ok(()) => setups::load(app),
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
            let name = property["quantity"].as_str().unwrap_or("property");
            let title = format!("{}: {} {}", name, property["value"], property["unit"].as_str().unwrap_or(""));
            ui_kit::section(ui, ("physical_property", id, index), &title, false, |ui| {
                ui.small(format!("Source: {} - {}", property["source_id"].as_str().unwrap_or(""), property["source_locator"].as_str().unwrap_or("")));
                ui.small(format!("Qualifier: {}", property["qualifier"].as_str().unwrap_or("unknown")));
                ui.small(format!("Conditions: {}", property["conditions"]));
                ui.small(format!("Uncertainty: {}", property["uncertainty"]));
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
    ui_kit::checkbox(ui, app.tr("render.physical_acknowledge"), &mut view.physical_ack);
    if ui_kit::button(ui, app.tr("render.physical_prepare_plan"), ui_kit::Button::Enabled(view.physical_ack)).clicked() {
        match app.run("render.library.physical.prepare", json!({"id":id,"name":view.physical_name,"acknowledge_reference":true})) {
            Ok(plan) => view.physical_plan = Some(plan),
            Err(e) => app.set_status(e, true),
        }
    }
    if let Some(plan) = &view.physical_plan {
        ui_kit::text(ui, app.tr("render.physical_plan_only"), ui_kit::Text::Note("render.physical_plan_only"));
        let text = serde_json::to_string_pretty(plan).unwrap_or_default();
        if ui_kit::button(ui, app.tr("render.physical_copy_plan"), ui_kit::Button::Normal).clicked() {
            ui.ctx().copy_text(text);
        }
    }
}

fn short_availability<'a>(app: &'a SolveApp, status: Option<&str>) -> &'a str {
    app.tr(match status {
        Some("native_composition") => "render.library_short_native",
        Some("implemented_pending_batch_validation") => "render.library_short_projection",
        Some("blocked") => "render.library_short_blocked",
        Some("specification_only") => "render.library_short_specification",
        _ => "render.library_short_reference",
    })
}

fn availability_label<'a>(app: &'a SolveApp, status: Option<&str>) -> &'a str {
    app.tr(match status {
        Some("native_composition") => "render.availability_native",
        Some("implemented_pending_batch_validation") => "render.availability_projection",
        Some("blocked") => "render.library_blocked",
        Some("specification_only") => "render.library_specs",
        _ => "render.availability_reference",
    })
}

#[cfg(test)]
mod studio_browser_tests {
    use super::*;
    use crate::scenario::Harness;
    #[test]
    fn favorites_and_recent_keep_exact_status_without_promoting_specs_to_runtime() {
        let mut h = Harness::new();
        let before = h.app.session.doc.to_json();
        let mut view = View::default();
        choose(&mut h.app, &mut view, "native_scenes", "NATIVE-STUDIO-002").unwrap();
        let native = ("native_scenes".into(), "NATIVE-STUDIO-002".into());
        view.favorites.insert(native.clone());
        choose(&mut h.app, &mut view, "scenes", "SCENE-001").unwrap();
        assert_eq!(view.statuses.get(&("scenes".into(), "SCENE-001".into())).map(String::as_str), Some("specification_only"));
        for i in 1..=48 {
            choose(&mut h.app, &mut view, "surfaces", &format!("SURF-{i:03}")).unwrap();
        }
        assert_eq!(view.recent.len(), 32);
        assert_eq!(view.statuses.len(), 33);
        assert_eq!(view.statuses.get(&native).map(String::as_str), Some("native_composition"));
        assert_eq!(h.app.session.doc.to_json(), before);
        assert!(h.app.session.undo.is_empty());
    }
    #[test]
    fn physical_reference_navigation_uses_the_same_category_history() {
        let mut h = Harness::new();
        let mut view = View::default();
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
        assert_eq!(view.recent.len(), 32);
        assert_eq!(h.app.session.doc.to_json(), before);
        assert!(h.app.session.undo.is_empty());
        choose(&mut h.app, &mut view, "surfaces", "SURF-048").unwrap();
        assert_eq!(view.recent.len(), 32);
        assert_eq!(h.app.render_workspace.view.inspector.page, super::super::inspector::Page::Appearance);
        choose(&mut h.app, &mut view, "native_scenes", "NATIVE-STUDIO-002").unwrap();
        assert_eq!(h.app.render_workspace.view.inspector.page, super::super::inspector::Page::Scene);
        assert_eq!(h.app.session.doc.to_json(), before);
    }
}
