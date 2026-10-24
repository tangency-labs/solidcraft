//! Contextual native Render UI. One workspace, existing command authority and shared controls.
//! Page navigation is per document; unsupported backend capabilities never emit dummy parameters.
use super::{ImageMethod, cpu, library, materials, setups, studio};
use crate::{SolveApp, theme::Tokens, ui_kit};
use serde_json::{Value, json};
use solidcraft_engine::doc::Document;
use std::sync::{Arc, Weak};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Page {
    #[default]
    Scene,
    Appearance,
    Aging,
    Effects,
    Lighting,
    Environment,
    Background,
    Camera,
    Output,
    Diagnostics,
    All,
}
impl Page {
    const ALL: [Self; 11] = [
        Self::Scene,
        Self::Appearance,
        Self::Aging,
        Self::Lighting,
        Self::Environment,
        Self::Background,
        Self::Camera,
        Self::Effects,
        Self::Output,
        Self::Diagnostics,
        Self::All,
    ];
    pub(super) fn key(self) -> &'static str {
        match self {
            Self::Scene => "render.page_scene",
            Self::Appearance => "render.page_appearance",
            Self::Aging => "render.page_aging",
            Self::Effects => "render.page_effects",
            Self::Lighting => "render.page_lighting",
            Self::Environment => "render.page_environment",
            Self::Background => "render.page_background",
            Self::Camera => "render.page_camera",
            Self::Output => "render.page_output",
            Self::Diagnostics => "render.page_diagnostics",
            Self::All => "render.page_all",
        }
    }
    fn handle(self) -> &'static str {
        match self {
            Self::Scene => "scene",
            Self::Appearance => "appearance",
            Self::Aging => "aging",
            Self::Effects => "effects",
            Self::Lighting => "lighting",
            Self::Environment => "environment",
            Self::Background => "background",
            Self::Camera => "camera",
            Self::Output => "output",
            Self::Diagnostics => "diagnostics",
            Self::All => "all",
        }
    }
    fn icon(self) -> &'static str {
        match self {
            Self::Scene => "render_scene",
            Self::Appearance => "appearance",
            Self::Aging => "appearance",
            Self::Effects => "camera",
            Self::Lighting => "settings",
            Self::Environment => "environment_map",
            Self::Background => "render_scene",
            Self::Camera => "camera",
            Self::Output => "export",
            Self::Diagnostics => "inspect",
            Self::All => "list",
        }
    }
}
#[derive(Clone, Debug)]
pub(super) struct View {
    pub effects: super::effects::View,
    pub page: Page,
    reveal: Option<&'static str>,
    pub library_open: bool,
    pending: Option<Pending>,
    shot_name: String,
    scene_review: Option<Review>,
    scene_consent: bool,
    quality_review: Option<Value>,
    quality_ack: bool,
    pub checked: Option<Review>,
    scene_info: Option<Review>,
    ground_margin: f64,
    ground_gap: f64,
    ground_color: [f64; 3],
    replace_ground: bool,
    appearance: Appearance,
    resolution: Resolution,
}
/// Editor-only coupling. The fixed rational anchor prevents repeated rounded
/// edits drifting; external preset/shot changes deliberately establish a new anchor.
#[derive(Clone, Debug, Default)]
struct Resolution {
    locked: bool,
    anchor: Option<[u32; 2]>,
    observed: Option<[u32; 2]>,
    rejected: bool,
}
impl Resolution {
    fn observe(&mut self, size: [u32; 2]) {
        if self.observed != Some(size) {
            self.anchor = (size[0] > 0 && size[1] > 0).then_some(size);
            self.observed = Some(size);
            self.rejected = false;
        }
    }
    fn edit(&mut self, before: [u32; 2], axis: usize, value: u32, maximum: u32) -> Option<[u32; 2]> {
        self.observe(before);
        let mut candidate = before;
        *candidate.get_mut(axis)? = value;
        if value == 0 || value > maximum {
            self.rejected = true;
            return None;
        }
        if self.locked {
            let Some(anchor) = self.anchor else {
                self.rejected = true;
                return None;
            };
            let Some(other) = 1usize.checked_sub(axis) else {
                self.rejected = true;
                return None;
            };
            let denominator = u64::from(*anchor.get(axis)?);
            let numerator = u64::from(value).checked_mul(u64::from(*anchor.get(other)?))?;
            let paired = numerator.checked_add(denominator / 2)?.checked_div(denominator)?;
            if paired == 0 || paired > u64::from(maximum) {
                self.rejected = true;
                return None;
            }
            *candidate.get_mut(other)? = u32::try_from(paired).ok()?;
        } else {
            self.anchor = (candidate[0] > 0 && candidate[1] > 0).then_some(candidate);
        }
        self.observed = Some(candidate);
        self.rejected = false;
        Some(candidate)
    }
}

#[derive(Clone, Debug)]
struct Pending {
    action: PendingAction,
    document: Weak<Document>,
    revision: u64,
}
#[derive(Clone, Debug)]
enum PendingAction {
    Switch(String),
    Reload,
    Remove(String),
}
#[derive(Clone, Debug)]
pub(super) struct Review {
    request: Value,
    reply: Value,
    revision: u64,
    document: Weak<Document>,
}
impl Review {
    fn current(&self, app: &SolveApp) -> bool {
        self.revision == app.session.revision && Weak::ptr_eq(&self.document, &Arc::downgrade(&app.session.doc))
    }
}
impl View {
    pub(super) fn clear_resource_reviews(&mut self) {
        self.scene_review = None;
        self.scene_consent = false;
        self.quality_review = None;
        self.quality_ack = false;
    }
}
impl Default for View {
    fn default() -> Self {
        Self {
            effects: Default::default(),
            page: Page::Scene,
            reveal: None,
            library_open: false,
            pending: None,
            shot_name: String::new(),
            scene_review: None,
            scene_consent: false,
            quality_review: None,
            quality_ack: false,
            checked: None,
            scene_info: None,
            ground_margin: 1.0,
            ground_gap: 0.1,
            ground_color: [0.18; 3],
            replace_ground: false,
            appearance: Appearance::default(),
            resolution: Resolution::default(),
        }
    }
}
pub(super) fn record_check(app: &mut SolveApp, request: &Value, reply: &Value) {
    app.render_workspace.view.inspector.checked =
        Some(Review { request: request.clone(), reply: reply.clone(), revision: app.session.revision, document: Arc::downgrade(&app.session.doc) });
}

pub(super) fn panel(app: &mut SolveApp, ui: &mut egui::Ui) {
    setups::refresh_dirty(app);
    if app.render_workspace.revision != app.session.revision && !app.render_workspace.view.dirty {
        setups::load(app);
    }
    let t = Tokens::get();
    let minimum = (ui.available_width() * 0.40).clamp(t.form_card_width, t.form_card_width + t.space_24);
    let maximum = (ui.available_width() * 0.52).max(minimum).min(t.form_card_width * 1.8);
    let frame = crate::ui_kit::Panel::feature_frame(ui.ctx(), app.ui.layout, crate::frame::Side::Right).inner_margin(t.space_10);
    let content = ui_kit::Container::new(ui.ctx(), ui_kit::Surface::Panel)
        .edge("sc_render_settings", ui_kit::Edge::Right)
        .default_size(t.form_card_width + t.space_24 * 3.0)
        .size_range(minimum..=maximum)
        .resizable(true)
        .frame(frame)
        .show(ui, |ui| {
            ui_kit::scope(ui, |ui| {
                header(app, ui);
                pages(app, ui);
                ui_kit::Panel::pinned_footer(ui, "render_job_footer", 4, |ui| {
                    ui.separator();
                    super::job_controls(app, ui, true);
                    if ui_kit::button(ui, app.tr("render.return_design"), ui_kit::Button::Normal).clicked()
                        && let Err(error) = super::enter(app, false)
                    {
                        app.set_status(error, true);
                    }
                    crate::scenario::publish_rect("render:job-footer", ui.min_rect());
                });
                let page = app.render_workspace.view.inspector.page;
                egui::ScrollArea::vertical().id_salt(("render_inspector", page.handle(), app.docs.active)).auto_shrink([false, false]).show(
                    ui,
                    |ui| {
                        ui_kit::text(ui, app.tr(page.key()), ui_kit::Text::Heading(page.key()));
                        match page {
                            Page::Scene => scene_controls(app, ui),
                            Page::Appearance => appearance_controls(app, ui),
                            Page::Aging => super::effects::aging_controls(app, ui),
                            Page::Effects => super::effects::presentation_controls(app, ui),
                            Page::Lighting => lighting_controls(app, ui),
                            Page::Environment => environment_controls(app, ui),
                            Page::Background => background_controls(app, ui),
                            Page::Camera => camera_controls(app, ui),
                            Page::Output => output_controls(app, ui),
                            Page::Diagnostics => diagnostics(app, ui),
                            Page::All => {
                                ui_kit::text(ui, app.tr("render.all_controls_help"), ui_kit::Text::Note("render.all_controls_help"));
                                camera_actions(app, ui);
                                let title = app.tr("render.page_camera").to_string();
                                ui_kit::section(ui, "render_all_camera", &title, false, |ui| camera_controls(app, ui));
                                let title = app.tr("render.page_aging").to_string();
                                ui_kit::section(ui, "render_all_aging", &title, false, |ui| super::effects::aging_controls(app, ui));
                                super::image_edit_controls(app, ui);
                            }
                        }
                        setups::refresh_dirty(app);
                    },
                );
            });
        });
    crate::scenario::publish_rect("render:inspector", content.response.rect);
    confirmation(app, ui.ctx());
}
fn pages(app: &mut SolveApp, ui: &mut egui::Ui) {
    let page = app.render_workspace.view.inspector.page;
    let mut index = Page::ALL.iter().position(|p| *p == page).unwrap_or(0);
    let options: Vec<_> =
        Page::ALL.iter().map(|page| ui_kit::IconSegment { key: page.handle(), icon: page.icon(), tip: app.tr(page.key()).to_string() }).collect();
    let top = ui.cursor().min;
    let width = ui.available_width();
    let height = ui_kit::row_height();
    if ui_kit::segmented_icons(ui, &mut index, &options, &[])
        && let Some(page) = Page::ALL.get(index)
    {
        app.render_workspace.view.inspector.page = *page;
    }
    for (i, page) in Page::ALL.iter().enumerate() {
        crate::scenario::publish_target(
            &format!("render:page:{}", page.handle()),
            egui::Rect::from_min_size(
                top + egui::vec2(width * i as f32 / Page::ALL.len() as f32, 0.0),
                egui::vec2(width / Page::ALL.len() as f32, height),
            ),
        );
    }
    ui.add_space(Tokens::get().space_4);
}
fn header(app: &mut SolveApp, ui: &mut egui::Ui) {
    ui.heading(app.tr("render.studio_title"));
    let current = app.render_workspace.view.name.clone();
    let names: Vec<_> = app.session.doc.render.setups.keys().cloned().collect();
    let mut chosen = None;
    let menu = ui_kit::dropdown_menu(ui, "render_setups", &current, |ui| {
        for name in names {
            let r = ui_kit::list_picker(ui, &name, &name, name == current);
            crate::scenario::publish_handle(&format!("render:setup:{name}"), r.rect.center());
            if r.clicked() {
                chosen = Some(name);
                ui.close();
            }
        }
    });
    crate::scenario::publish_target("render:setups", menu.rect);
    if let Some(name) = chosen {
        request_action(app, PendingAction::Switch(name));
    }
    let field = ui_kit::bounded_cell_editor(ui, ui.id().with("render_setup_name"), &mut app.render_workspace.view.name, "", false, 128);
    crate::scenario::publish_target("render:setup-name", field.rect);
    if field.changed() {
        app.render_workspace.view.dirty = true;
    }
    ui.horizontal_wrapped(|ui| {
        if ui_kit::button(ui, app.tr("render.save_setup"), ui_kit::Button::Enabled(setups::snapshot(app).is_ok())).clicked()
            && let Err(error) = setups::save(app)
        {
            app.set_status(error, true);
        }
        if ui_kit::button(ui, app.tr("render.reload_settings"), ui_kit::Button::Normal).clicked() {
            request_action(app, PendingAction::Reload);
        }
    });
    let status = if app.render_workspace.view.dirty { app.tr("render.unsaved") } else { app.tr("render.studio_saved") };
    ui.add_sized([ui.available_width(), ui.text_style_height(&egui::TextStyle::Body)], egui::Label::new(status).truncate());
    if app.render_workspace.view.inspector.page != Page::All {
        let labels = [app.tr("render.result_preview").to_string(), app.tr("render.result_title").to_string()];
        let mut value = usize::from(app.render_workspace.view.result);
        if ui_kit::segmented(ui, &mut value, &labels.iter().map(String::as_str).collect::<Vec<_>>(), Default::default()) {
            app.render_workspace.view.result = value == 1;
            app.cancel_view_animation();
            app.viewport.key_hold = None;
            app.viewport.orbit_free = None;
        }
    }
}
fn request_action(app: &mut SolveApp, action: PendingAction) {
    if app.render_workspace.view.dirty || matches!(&action, PendingAction::Remove(_)) {
        app.render_workspace.view.inspector.pending =
            Some(Pending { action, document: Arc::downgrade(&app.session.doc), revision: app.session.revision });
    } else if let Err(error) = perform_action(app, action) {
        app.set_status(error, true);
    }
}
fn perform_action(app: &mut SolveApp, action: PendingAction) -> Result<(), String> {
    match action {
        PendingAction::Switch(name) => {
            app.render_workspace.view.name = name;
            setups::load(app);
        }
        PendingAction::Reload => setups::load(app),
        PendingAction::Remove(name) => {
            app.run("render.shot.remove", json!({"name":name,"expected_revision":app.session.revision}))?;
            app.render_workspace.view.name = app.session.doc.render.setups.keys().next().cloned().unwrap_or_else(|| "Studio".into());
            setups::load(app);
        }
    }
    Ok(())
}
fn confirmation(app: &mut SolveApp, ctx: &egui::Context) {
    let Some(pending) = app.render_workspace.view.inspector.pending.clone() else { return };
    if pending.revision != app.session.revision || !Weak::ptr_eq(&pending.document, &Arc::downgrade(&app.session.doc)) {
        app.render_workspace.view.inspector.pending = None;
        app.set_status(app.tr("render.review_stale").to_string(), true);
        return;
    }
    let removing = matches!(&pending.action, PendingAction::Remove(_));
    let title = app.tr(if removing { "render.shot_remove_title" } else { "render.draft_confirm_title" }).to_string();
    let keep = app.tr("render.keep_editing").to_string();
    let primary = app.tr(if removing { "render.shot_remove" } else { "render.save_continue" }).to_string();
    let discard = app.tr("render.discard_continue").to_string();
    let message = app.tr(if removing { "render.shot_remove_help" } else { "render.draft_confirm_help" }).to_string();
    let (closed, answer) = ui_kit::Confirm::new(egui::Id::new("render_pending_shot"), if removing { "delete" } else { "save" }, &title)
        .tone(if removing { ui_kit::Tone::Danger } else { ui_kit::Tone::Warning })
        .actions(&primary, &keep, (!removing).then_some(discard.as_str()))
        .selected(ui_kit::Action::Cancel)
        .show(ctx, removing || setups::snapshot(app).is_ok(), |ui| {
            ui.label(&message);
        });
    if closed || answer == ui_kit::Action::Cancel {
        app.render_workspace.view.inspector.pending = None;
        return;
    }
    if answer == ui_kit::Action::None {
        return;
    }
    if answer == ui_kit::Action::Ok
        && !removing
        && let Err(error) = setups::save(app)
    {
        app.set_status(error, true);
        return;
    }
    app.render_workspace.view.inspector.pending = None;
    if let Err(error) = perform_action(app, pending.action) {
        app.set_status(error, true);
    }
}
fn browse_button(app: &mut SolveApp, ui: &mut egui::Ui, kind: &str, key: &str) {
    if ui_kit::button(ui, app.tr(key), ui_kit::Button::Normal).clicked() {
        library::open_category(app, kind);
    }
}
fn scene_controls(app: &mut SolveApp, ui: &mut egui::Ui) {
    // After a library selection, primary native-resource actions lead the page.
    // Shared method/help and catalog navigation remain below, not between the
    // chosen resource and its explicit review/consent. No state is changed by layout.
    let selected_native = library::selected_kind(app, "native_scenes");
    if selected_native {
        library::inspect_kinds(app, ui, &["native_scenes"]);
    }
    ui_kit::text(ui, app.tr("render.scene_preserves_design"), ui_kit::Text::Note("render.scene_preserves_design"));
    super::method_selector(app, ui);
    gpu_status(app, ui);
    browse_button(app, ui, "native_scenes", "render.browse_native_scenes");
    browse_button(app, ui, "scenes", "render.browse_scene_specs");
    if !selected_native {
        library::inspect_kinds(app, ui, &["native_scenes", "scenes", "selene_shots"]);
    }
    physical_stage_controls(app, ui);
    shot_controls(app, ui);
}
fn physical_stage_controls(app: &mut SolveApp, ui: &mut egui::Ui) {
    let title = app.tr("render.passive_stage").to_string();
    reveal_section(app, ui, "render_ground_details");
    ui_kit::section(ui, "render_ground_details", &title, false, |ui| {
        ui.label(app.tr("render.ground_margin"));
        ui_kit::number_field(ui, &mut app.render_workspace.view.inspector.ground_margin, 0.0..=10.0, 0.05, " x");
        ui.label(app.tr("render.ground_gap"));
        ui_kit::number_field(ui, &mut app.render_workspace.view.inspector.ground_gap, 0.0001..=1000.0, 0.01, " mm");
        let label = app.tr("render.ground_linear_rgb").to_string();
        vector(ui, &label, &mut app.render_workspace.view.inspector.ground_color, 0.0..=1.0, 0.01);
        let label = app.tr("render.replace_ground").to_string();
        let replace = ui_kit::checkbox(ui, &label, &mut app.render_workspace.view.inspector.replace_ground);
        crate::scenario::publish_target("render:stage-replace-consent", replace.rect);
        let saved = setups::snapshot(app);
        if ui_kit::button(
            ui,
            app.tr("render.ground_apply"),
            ui_kit::Button::Enabled(saved.is_ok() && app.render_workspace.view.method != ImageMethod::Raster),
        )
        .clicked()
            && let Ok(set) = saved
        {
            let v = &app.render_workspace.view.inspector;
            let request = json!({"name":app.render_workspace.view.name,"set":set,"margin_fraction":v.ground_margin,"gap_mm":v.ground_gap,"reflectance_linear":v.ground_color,"replace_stage":v.replace_ground});
            match app.run("render.stage.ground", request) {
                Ok(_) => setups::load(app),
                Err(e) => app.set_status(e, true),
            }
        }
        ui_kit::text(ui, app.tr("render.physical_background_help"), ui_kit::Text::Note("render.physical_background_help"));
        for (kind, key) in [("floor_wall", "render.physical_floor_wall"), ("cyclorama", "render.physical_cyclorama")] {
            let set = setups::snapshot(app);
            let button =
                ui_kit::button(ui, app.tr(key), ui_kit::Button::Enabled(set.is_ok() && app.render_workspace.view.method != ImageMethod::Raster));
            crate::scenario::publish_target(&format!("render:stage-{kind}"), button.rect);
            if button.clicked()
                && let Ok(set) = set
            {
                let v = &app.render_workspace.view.inspector;
                let p = json!({"name":app.render_workspace.view.name,"set":set,"kind":kind,"expected_revision":app.session.revision,
                    "replace_stage":v.replace_ground,"reflectance_linear":v.ground_color});
                match app.run("render.stage.backdrop", p) {
                    Ok(_) => setups::load(app),
                    Err(error) => app.set_status(error, true),
                }
            }
        }
        library::stage_content(app, ui);
    });
}

fn background_controls(app: &mut SolveApp, ui: &mut egui::Ui) {
    ui_kit::text(ui, app.tr("render.background_workflow"), ui_kit::Text::Note("render.background_workflow"));
    let title = app.tr("render.background_camera_section").to_string();
    ui_kit::section(ui, "render_camera_background_section", &title, true, |ui| {
        ui.label(app.tr("render.background"));
        let color = ui_kit::color_field(ui, &mut app.render_workspace.view.draft.background);
        crate::scenario::publish_target("render:background-color", color.rect);
        app.render_workspace.view.dirty |= color.changed();
        if app.render_workspace.view.method == ImageMethod::GpuRaytrace {
            studio::backdrop_controls(app, ui);
        }
        if !app.render_workspace.view.draft.stage.is_empty() {
            ui_kit::text(ui, app.tr("render.background_stage_occlusion"), ui_kit::Text::Note("render.background_stage_occlusion"));
        }
        if app.render_workspace.view.result {
            ui_kit::text(ui, app.tr("render.background_completed_frozen"), ui_kit::Text::Note("render.background_completed_frozen"));
        }
        let enabled = super::ribbon::action_ready(app, "render.result.regrade").is_ok();
        let apply = ui_kit::button(ui, app.tr("render.background_regrade"), ui_kit::Button::Enabled(enabled));
        crate::scenario::publish_target("render:background-regrade", apply.rect);
        if apply.clicked() {
            if let Err(error) = super::ribbon::perform_job(app, "render.result.regrade") {
                app.set_status(error, true);
            }
            ui.ctx().request_repaint();
        }
        ui_kit::text(ui, app.tr("render.background_no_alpha_override"), ui_kit::Text::Note("render.background_no_alpha_override"));
    });
    physical_stage_controls(app, ui);
    let title = app.tr("render.background_lighting_section").to_string();
    ui_kit::section(ui, "render_background_hdr_section", &title, false, |ui| {
        ui_kit::text(ui, app.tr("render.background_hdr_separate"), ui_kit::Text::Note("render.background_hdr_separate"));
        if ui_kit::button(ui, app.tr("render.browse_environments"), ui_kit::Button::Normal).clicked() {
            app.render_workspace.view.inspector.page = Page::Environment;
            library::open_category(app, "hdr");
        }
    });
}

fn shot_controls(app: &mut SolveApp, ui: &mut egui::Ui) {
    let title = app.tr("render.shot_manager").to_string();
    reveal_section(app, ui, "render_shot_manager");
    ui_kit::section(ui, "render_shot_manager", &title, false, |ui| {
        ui_kit::text(ui, app.tr("render.shot_saved_only"), ui_kit::Text::Note("render.shot_saved_only"));
        let label = app.tr("render.shot_new_name").to_string();
        ui_kit::bounded_cell_editor(ui, ui.id().with("render_shot_new_name"), &mut app.render_workspace.view.inspector.shot_name, &label, false, 128);
        for (command, key) in [("render.shot.duplicate", "render.shot_duplicate"), ("render.shot.rename", "render.shot_rename")] {
            let enabled = !app.render_workspace.view.dirty
                && app.session.doc.render.setups.contains_key(&app.render_workspace.view.name)
                && !app.render_workspace.view.inspector.shot_name.trim().is_empty();
            if ui_kit::button(ui, app.tr(key), ui_kit::Button::Enabled(enabled)).clicked() {
                let name = app.render_workspace.view.inspector.shot_name.clone();
                match app.run(command, json!({"name":app.render_workspace.view.name,"new_name":name,"expected_revision":app.session.revision})) {
                    Ok(_) => {
                        app.render_workspace.view.name = name;
                        setups::load(app);
                    }
                    Err(e) => app.set_status(e, true),
                }
            }
        }
        if ui_kit::button(
            ui,
            app.tr("render.shot_remove"),
            ui_kit::Button::Enabled(app.session.doc.render.setups.contains_key(&app.render_workspace.view.name)),
        )
        .clicked()
        {
            request_action(app, PendingAction::Remove(app.render_workspace.view.name.clone()));
        }
    });
}
fn appearance_controls(app: &mut SolveApp, ui: &mut egui::Ui) {
    ui_kit::text(ui, app.tr("render.appearance_physics_separate"), ui_kit::Text::Note("render.appearance_physics_separate"));
    base_appearance(app, ui);
    browse_button(app, ui, "surfaces", "render.browse_surfaces");
    browse_button(app, ui, "physical_profiles", "render.browse_physical");
    library::inspect_kinds(app, ui, &["surfaces", "materials", "selene_appearances", "physical_profiles"]);
    library::saved_surface_controls(app, ui);
    materials::controls(app, ui);
    if app.render_workspace.view.method == ImageMethod::CpuRaytrace {
        cpu::reflection_controls(app, ui);
    }
}
fn lighting_controls(app: &mut SolveApp, ui: &mut egui::Ui) {
    browse_button(app, ui, "rigs", "render.browse_rigs");
    library::inspect_kinds(app, ui, &["rigs", "lighting"]);
    match app.render_workspace.view.method {
        ImageMethod::GpuRaytrace => studio::lighting_controls(app, ui),
        ImageMethod::CpuRaytrace => {
            super::cpu_lighting_controls(app, ui);
            cpu::light_controls(app, ui);
        }
        ImageMethod::Raster => {
            ui_kit::text(ui, app.tr("render.method_limit_raster"), ui_kit::Text::Note("render.method_limit_raster"));
        }
    }
}
fn environment_controls(app: &mut SolveApp, ui: &mut egui::Ui) {
    ui.label(app.tr("render.background"));
    let r = ui_kit::color_field(ui, &mut app.render_workspace.view.draft.background);
    if r.changed() {
        app.render_workspace.view.dirty = true;
    }
    ui_kit::text(ui, app.tr("render.background_not_light"), ui_kit::Text::Note("render.background_not_light"));
    if app.render_workspace.view.method == ImageMethod::GpuRaytrace {
        studio::backdrop_controls(app, ui);
    }
    browse_button(app, ui, "hdr", "render.browse_environments");
    library::inspect_kinds(app, ui, &["hdr", "environments"]);
    match app.render_workspace.view.method {
        ImageMethod::GpuRaytrace => studio::environment_controls(app, ui),
        ImageMethod::CpuRaytrace => cpu::environment_controls(app, ui),
        ImageMethod::Raster => {
            ui_kit::text(ui, app.tr("render.method_limit_raster"), ui_kit::Text::Note("render.method_limit_raster"));
        }
    }
}
fn vector(ui: &mut egui::Ui, label: &str, value: &mut [f64; 3], range: std::ops::RangeInclusive<f64>, speed: f64) {
    ui.label(label);
    ui.horizontal(|ui| {
        for component in value {
            ui_kit::number_field(ui, component, range.clone(), speed, "");
        }
    });
}
fn apply_camera_draft(app: &mut SolveApp, camera: solidcraft_engine::render::Camera) -> Result<(), String> {
    app.run("render.camera.set", json!({"camera":camera}))?;
    app.follow_view_request(false);
    app.render_workspace.view.dirty = true;
    Ok(())
}

fn camera_controls(app: &mut SolveApp, ui: &mut egui::Ui) {
    ui_kit::text(ui, app.tr("render.camera_exact_help"), ui_kit::Text::Note("render.camera_exact_help"));
    let before = app.cam;
    let mut camera = before;
    let mut perspective = camera.fov > 0.0;
    if ui_kit::checkbox(ui, app.tr("render.perspective"), &mut perspective).changed() {
        // On at the Field of view preference's angle (#5364); a saved setup keeps its own.
        camera.fov = if perspective { crate::prefs::perspective_fov(&app.preferences) } else { 0.0 };
    }
    if perspective {
        let mut fov = camera.fov.to_degrees();
        let label = app.tr("render.camera_fov").to_string();
        if ui_kit::row(ui, &label, |ui| ui_kit::number_field(ui, &mut fov, 1.0..=150.0, 0.1, " deg")).changed() {
            camera.fov = fov.to_radians();
        }
    }
    let label = app.tr("render.camera_distance").to_string();
    ui_kit::row(ui, &label, |ui| ui_kit::number_field(ui, &mut camera.distance, 0.000001..=1e9, 1.0, " mm"));
    let mut target = [camera.target.x, camera.target.y, camera.target.z];
    vector(ui, app.tr("render.camera_target"), &mut target, -1e9..=1e9, 1.0);
    camera.target = solidcraft_engine::geom::Vec3::new(target[0], target[1], target[2]);
    for (key, value) in [("render.camera_yaw", &mut camera.yaw), ("render.camera_pitch", &mut camera.pitch), ("render.camera_roll", &mut camera.roll)]
    {
        let mut degrees = value.to_degrees();
        let label = app.tr(key).to_string();
        if ui_kit::row(ui, &label, |ui| ui_kit::number_field(ui, &mut degrees, -360000.0..=360000.0, 0.5, " deg")).changed() {
            *value = degrees.to_radians();
        }
    }
    if camera != before
        && camera.is_valid()
        && let Err(error) = apply_camera_draft(app, camera)
    {
        app.set_status(error, true);
    }
    camera_actions(app, ui);
    if app.render_workspace.view.method == ImageMethod::GpuRaytrace {
        studio::lens_controls(app, ui);
    }
    if app.render_workspace.view.result {
        if ui_kit::button(ui, app.tr("render.result_fit"), ui_kit::Button::Normal).clicked() {
            super::fit_result(app);
        }
        if ui_kit::button(ui, "100%", ui_kit::Button::Enabled(!super::reduced_display(app))).clicked() {
            app.render_workspace.view.result_scale = super::ResultScale::Actual;
        }
    }
}
fn output_controls(app: &mut SolveApp, ui: &mut egui::Ui) {
    studio::export_controls(app, ui);
    super::method_selector(app, ui);
    gpu_status(app, ui);
    let side = match app.render_workspace.view.method {
        ImageMethod::CpuRaytrace => 16384,
        ImageMethod::GpuRaytrace => 8192,
        ImageMethod::Raster => 4096,
    };
    let before = [app.render_workspace.view.draft.width, app.render_workspace.view.draft.height];
    app.render_workspace.view.inspector.resolution.observe(before);
    let mut width = before[0];
    let mut height = before[1];
    let label = app.tr("render.image_width").to_string();
    let w = ui_kit::row(ui, &label, |ui| ui_kit::number_field(ui, &mut width, 1..=side, 1.0, " px"));
    crate::scenario::publish_target("render:output-width", w.rect);
    let label = app.tr("render.image_height").to_string();
    let h = ui_kit::row(ui, &label, |ui| ui_kit::number_field(ui, &mut height, 1..=side, 1.0, " px"));
    crate::scenario::publish_target("render:output-height", h.rect);
    let edit = if w.changed() {
        Some((0, width))
    } else if h.changed() {
        Some((1, height))
    } else {
        None
    };
    if let Some((axis, value)) = edit
        && let Some([width, height]) = app.render_workspace.view.inspector.resolution.edit(before, axis, value, side)
    {
        app.render_workspace.view.draft.width = width;
        app.render_workspace.view.draft.height = height;
    }
    let label = app.tr("render.output_lock_ratio").to_string();
    let lock = ui_kit::checkbox(ui, &label, &mut app.render_workspace.view.inspector.resolution.locked);
    crate::scenario::publish_target("render:aspect-lock", lock.rect);
    let swap = ui_kit::button(ui, app.tr("render.output_swap_orientation"), ui_kit::Button::Normal);
    crate::scenario::publish_target("render:swap-size", swap.rect);
    if swap.clicked() {
        let draft = &mut app.render_workspace.view.draft;
        std::mem::swap(&mut draft.width, &mut draft.height);
        app.render_workspace.view.inspector.resolution.observe([draft.width, draft.height]);
    }
    if app.render_workspace.view.inspector.resolution.rejected {
        ui_kit::text(ui, app.tr("render.output_ratio_refused"), ui_kit::Text::Error("render.output_ratio_refused"));
    }
    ui.horizontal_wrapped(|ui| {
        for (label, width, height) in [("HD", 1280, 720), ("Full HD", 1920, 1080), ("4K UHD", 3840, 2160)] {
            if ui_kit::button(ui, label, ui_kit::Button::Normal).clicked() {
                app.render_workspace.view.draft.width = width;
                app.render_workspace.view.draft.height = height;
            }
        }
    });
    let size = (u64::from(app.render_workspace.view.draft.width), u64::from(app.render_workspace.view.draft.height));
    if cpu::image_limits(app).fit(size.0, size.1, None) != Some(size) {
        ui_kit::text(ui, app.tr("render.output_over_budget"), ui_kit::Text::Error("render.output_over_budget"));
    }
    match app.render_workspace.view.method {
        ImageMethod::GpuRaytrace => {
            let label = app.tr("render.image_samples").to_string();
            ui_kit::row(ui, &label, |ui| ui_kit::number_field(ui, &mut app.render_workspace.view.samples, 1..=4096, 1.0, ""));
            let label = app.tr("render.gpu_depth").to_string();
            ui_kit::row(ui, &label, |ui| ui_kit::number_field(ui, &mut app.render_workspace.view.max_depth, 1..=16, 1.0, ""));
            let label = app.tr("render.gpu_seed").to_string();
            ui_kit::row(ui, &label, |ui| ui_kit::number_field(ui, &mut app.render_workspace.view.seed, 0..=u32::MAX, 1.0, ""));
            studio::finishing_controls(app, ui);
        }
        ImageMethod::CpuRaytrace => super::cpu_sampling_controls(app, ui),
        ImageMethod::Raster => {
            ui_kit::text(ui, app.tr("render.method_limit_raster"), ui_kit::Text::Note("render.method_limit_raster"));
        }
    }
    if app.render_workspace.view.method != ImageMethod::Raster {
        let label = app.tr("render.image_exposure").to_string();
        ui_kit::row(ui, &label, |ui| ui_kit::number_field(ui, &mut app.render_workspace.view.draft.exposure_ev, -32.0..=32.0, 0.1, " EV"));
    }
    let label = app.tr("render.image_transparent").to_string();
    ui_kit::checkbox(ui, &label, &mut app.render_workspace.view.transparent);
    ui_kit::text(ui, app.tr("render.output_format_status"), ui_kit::Text::Note("render.output_format_status"));
    let label = app.tr("render.image_overwrite").to_string();
    ui_kit::checkbox(ui, &label, &mut app.render_workspace.view.overwrite);
    browse_button(app, ui, "quality", "render.browse_quality");
    library::inspect_kinds(app, ui, &["quality"]);
    super::completed_image(app, ui);
    ui_kit::text(ui, app.tr("render.adaptive_pending"), ui_kit::Text::Note("render.adaptive_pending"));
}

pub(super) fn scene_resource(app: &mut SolveApp, ui: &mut egui::Ui, id: &str) {
    let snapshot = setups::snapshot(app);
    let resolve = ui_kit::button(
        ui,
        app.tr("render.scene_review"),
        ui_kit::Button::Enabled(snapshot.is_ok() && app.render_workspace.view.method == ImageMethod::GpuRaytrace),
    );
    crate::scenario::publish_target("render:scene-resolve", resolve.rect);
    if resolve.clicked()
        && let Ok(set) = snapshot
    {
        let request = json!({"id":id,"name":app.render_workspace.view.name,"set":set,"expected_revision":app.session.revision});
        match app.run("render.preset.resolve", request.clone()) {
            Ok(reply) => {
                app.render_workspace.view.inspector.scene_review =
                    Some(Review { request, reply, revision: app.session.revision, document: Arc::downgrade(&app.session.doc) });
                app.render_workspace.view.inspector.scene_consent = false;
            }
            Err(error) => app.set_status(error, true),
        }
    }
    let review = app.render_workspace.view.inspector.scene_review.clone().filter(|r| r.request["id"] == id);
    if let Some(review) = review {
        let unchanged =
            review.current(app) && setups::snapshot(app).ok().and_then(|v| serde_json::to_value(v).ok()).as_ref() == Some(&review.request["set"]);
        if !unchanged {
            ui_kit::text(ui, app.tr("render.review_stale"), ui_kit::Text::Error("render.review_stale"));
        }
        if let Some(fields) = review.reply["replaces"].as_array() {
            ui.small(format!("{}: {}", app.tr("render.scene_replaces"), fields.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(", ")));
        }
        let label = app.tr("render.scene_consent").to_string();
        let consent = ui_kit::checkbox(ui, &label, &mut app.render_workspace.view.inspector.scene_consent).on_hover_text(&label);
        crate::scenario::publish_target("render:scene-consent", consent.rect);
        let apply =
            ui_kit::button(ui, app.tr("render.scene_apply"), ui_kit::Button::Enabled(unchanged && app.render_workspace.view.inspector.scene_consent));
        crate::scenario::publish_target("render:scene-apply", apply.rect);
        if apply.clicked() {
            let mut request = review.request;
            request["replace_lighting_and_environment"] = json!(true);
            request["replace_stage"] = json!(true);
            match app.run("render.preset.apply", request) {
                Ok(_) => {
                    setups::load(app);
                    app.render_workspace.view.inspector.scene_review = None;
                    app.set_status(app.tr("render.library_applied").to_string(), false);
                }
                Err(error) => app.set_status(error, true),
            }
        }
    }
    let title = app.tr("render.scene_scope_details").to_string();
    ui_kit::section(ui, ("render_native_composition_scope", id), &title, false, |ui| {
        for key in ["render.scene_review_help", "render.scene_preserved_fields", "render.library_authored_help"] {
            ui_kit::text(ui, app.tr(key), ui_kit::Text::Note(key));
        }
    });
}
pub(super) fn quality_resource(app: &mut SolveApp, ui: &mut egui::Ui, id: &str) {
    ui_kit::text(ui, app.tr("render.quality_projection_help"), ui_kit::Text::Note("render.quality_projection_help"));
    let label = app.tr("render.quality_ack").to_string();
    ui_kit::checkbox(ui, &label, &mut app.render_workspace.view.inspector.quality_ack);
    let enabled = app.render_workspace.view.inspector.quality_ack && app.render_workspace.view.method == ImageMethod::GpuRaytrace;
    if ui_kit::button(ui, app.tr("render.quality_review"), ui_kit::Button::Enabled(enabled)).clicked() {
        match app.run("render.quality.prepare", json!({"id":id,"variant":"fixed_samples_png","acknowledge_omissions":true})) {
            Ok(reply) => app.render_workspace.view.inspector.quality_review = Some(reply),
            Err(error) => {
                app.render_workspace.view.inspector.quality_review = None;
                app.set_status(error, true);
            }
        }
    }
    if let Some(plan) = app.render_workspace.view.inspector.quality_review.clone().filter(|p| p["id"] == id) {
        ui.label(format!(
            "{} x {} px / {} spp / {}",
            plan["settings"]["width"], plan["settings"]["height"], plan["gpu_options"]["samples"], plan["gpu_options"]["max_depth"]
        ));
        if ui_kit::button(ui, app.tr("render.quality_apply"), ui_kit::Button::Enabled(enabled)).clicked() {
            let result = apply_quality(app, &plan);
            if let Err(error) = result {
                app.set_status(error, true);
            }
        }
    }
}
fn apply_quality(app: &mut SolveApp, plan: &Value) -> Result<(), String> {
    if app.render_workspace.view.method != ImageMethod::GpuRaytrace || plan["variant"] != "fixed_samples_png" {
        return Err("quality projection requires the same explicit GPU method".into());
    }
    let u32_at = |section: &str, key: &str| {
        plan[section][key].as_u64().and_then(|n| u32::try_from(n).ok()).ok_or_else(|| "invalid native quality projection".to_string())
    };
    let (width, height, samples, depth, seed) = (
        u32_at("settings", "width")?,
        u32_at("settings", "height")?,
        u32_at("gpu_options", "samples")?,
        u32_at("gpu_options", "max_depth")?,
        u32_at("gpu_options", "seed")?,
    );
    let denoise = plan["gpu_options"]["denoise"].as_bool().ok_or("invalid native denoise setting")?;
    if !(1..=4096).contains(&samples)
        || !(1..=16).contains(&depth)
        || solidcraft_engine::render::job::GPU_PATH_LIMITS.fit(u64::from(width), u64::from(height), None)
            != Some((u64::from(width), u64::from(height)))
    {
        return Err("quality projection exceeds native bounds".into());
    }
    let view = &mut app.render_workspace.view;
    view.draft.width = width;
    view.draft.height = height;
    view.samples = samples;
    view.max_depth = depth;
    view.seed = seed;
    view.studio.denoise = denoise;
    view.dirty = true;
    Ok(())
}
fn diagnostics(app: &mut SolveApp, ui: &mut egui::Ui) {
    if let Some(bytes) = app.render_workspace.preview_upload_bytes {
        ui.label(format!("{}: {:.2} / {} MiB", app.tr("render.preview_upload"), bytes as f64 / 1048576.0, super::MAX_GEOMETRY_BYTES / 1048576));
        ui_kit::text(ui, app.tr("render.preview_upload_help"), ui_kit::Text::Note("render.preview_upload_help"));
    }
    if let Some(error) = &app.render_workspace.scene_error {
        ui_kit::text(ui, error, ui_kit::Text::Error("render.preview_cached_error"));
    }
    let request = super::image_request(app);
    ui.strong(app.tr(app.render_workspace.view.method.label()));
    if let Err(error) = super::image_params(app) {
        ui_kit::text(ui, &error.to_string(), ui_kit::Text::Error("render.native-error"));
    }
    if ui_kit::button(ui, app.tr("render.inspect_scene"), ui_kit::Button::Normal).clicked() {
        match app.run("render.scene", json!({})) {
            Ok(reply) => {
                app.render_workspace.view.inspector.scene_info =
                    Some(Review { request: json!({}), reply, revision: app.session.revision, document: Arc::downgrade(&app.session.doc) })
            }
            Err(error) => app.set_status(error, true),
        }
    }
    if let Some(info) = &app.render_workspace.view.inspector.scene_info {
        if !info.current(app) {
            ui_kit::text(ui, app.tr("render.review_stale"), ui_kit::Text::Note("render.review_stale"));
        }
        let bodies = info.reply["bodies"].as_array();
        let count = bodies.map_or(0, Vec::len);
        let triangles = bodies.map_or(0, |b| b.iter().filter_map(|v| v["triangles"].as_u64()).sum::<u64>());
        ui.label(format!("{}: {count} / {}: {triangles}", app.tr("render.diag_bodies"), app.tr("render.diag_triangles")));
    }
    if let Some(check) = &app.render_workspace.view.inspector.checked {
        ui.strong(app.tr("render.diag_last_check"));
        if !check.current(app) || request.as_ref().ok() != Some(&check.request) {
            ui_kit::text(ui, app.tr("render.review_stale"), ui_kit::Text::Note("render.review_stale"));
        }
        if let Some(bytes) = check.reply["admission"]["estimated_total_bytes"].as_u64() {
            ui.label(format!("{}: {:.2} MiB", app.tr("render.diag_memory"), bytes as f64 / 1048576.0));
        }
        if let Some(warnings) = check.reply["warnings"].as_array() {
            for warning in warnings {
                ui_kit::text(ui, &warning.to_string(), ui_kit::Text::Note("render.native-warning"));
            }
        }
        let detail = check.reply.clone();
        json_details(app, ui, "render.diag_preflight_details", &detail);
    }
    if let Some(job) = app.render_workspace.view.job
        && let Ok(status) = app.session.execute("render.status", &json!({"job":job}))
    {
        if let Some(error) = status["error"]["message"].as_str() {
            ui_kit::text(ui, error, ui_kit::Text::Error("render.native-error"));
        }
        json_details(app, ui, "render.diag_job_details", &status);
    }
    if let Ok(request) = &request {
        json_details(app, ui, "render.diag_request", request);
    }
    ui.separator();
    ui.strong(app.tr("render.diag_not_implemented"));
    for key in
        ["render.adaptive_pending", "render.exr_pending", "render.transmission_scope", "render.advanced_material_scope", "render.queue_pending"]
    {
        ui_kit::text(ui, app.tr(key), ui_kit::Text::Note(key));
    }
    browse_button(app, ui, "capabilities", "render.browse_capabilities");
    library::inspect_kinds(app, ui, &["capabilities"]);
}
fn json_details(app: &SolveApp, ui: &mut egui::Ui, key: &str, value: &Value) {
    let title = app.tr(key).to_string();
    ui_kit::section(ui, key, &title, false, |ui| {
        let text = serde_json::to_string_pretty(value).unwrap_or_default();
        if ui_kit::button(ui, app.tr("render.copy_request"), ui_kit::Button::Normal).clicked() {
            ui.ctx().copy_text(text.clone());
        }
        egui::ScrollArea::vertical().max_height(280.0).show(ui, |ui| {
            ui.monospace(text);
        });
    });
}

#[derive(Clone, Debug)]
struct Appearance {
    query: String,
    items: Vec<Value>,
    selected: Option<String>,
    document: Option<Weak<Document>>,
    revision: Option<u64>,
    color: [u8; 3],
    metallic: f64,
    roughness: f64,
    ior: f64,
    opaque: bool,
}
impl Default for Appearance {
    fn default() -> Self {
        Self {
            query: String::new(),
            items: Vec::new(),
            selected: None,
            document: None,
            revision: None,
            color: [160, 170, 180],
            metallic: 0.0,
            roughness: 0.5,
            ior: 1.5,
            opaque: false,
        }
    }
}
fn base_appearance(app: &mut SolveApp, ui: &mut egui::Ui) {
    let title = app.tr("render.base_appearance").to_string();
    ui_kit::section(ui, "render_base_appearance", &title, false, |ui| {
        let mut view = std::mem::take(&mut app.render_workspace.view.inspector.appearance);
        ui_kit::bounded_cell_editor(ui, ui.id().with("render_base_target_search"), &mut view.query, app.tr("render.material_filter"), false, 128);
        if ui_kit::button(ui, app.tr("render.material_refresh"), ui_kit::Button::Normal).clicked() {
            match app.run("render.library.targets", json!({"query":view.query,"limit":128})) {
                Ok(reply) => {
                    view.items = reply["items"].as_array().cloned().unwrap_or_default();
                    view.selected = None;
                    view.document = Some(Arc::downgrade(&app.session.doc));
                    view.revision = Some(app.session.revision);
                }
                Err(error) => app.set_status(error, true),
            }
        }
        let label = view.selected.clone().unwrap_or_else(|| app.tr("render.material_choose_body").into());
        ui_kit::dropdown_menu(ui, "render_base_target", &label, |ui| {
            for item in &view.items {
                let Some(name) = item["name"].as_str() else { continue };
                if ui_kit::list_picker(ui, &format!("base:{name}"), name, view.selected.as_deref() == Some(name)).clicked() {
                    view.selected = Some(name.into());
                    view.color = serde_json::from_value(item["color"].clone()).unwrap_or([160, 170, 180]);
                    view.opaque = item["opacity"].as_f64() == Some(1.0);
                    view.metallic = item["pbr"]["metallic"].as_f64().unwrap_or(0.0);
                    view.roughness = item["pbr"]["roughness"].as_f64().unwrap_or(0.5);
                    view.ior = item["pbr"]["dielectric_ior"].as_f64().unwrap_or(1.5);
                    ui.close();
                }
            }
        });
        if view.selected.is_some() {
            ui.label(app.tr("render.base_color"));
            ui_kit::color_field(ui, &mut view.color);
            for (key, value, range) in [
                ("render.base_metallic", &mut view.metallic, 0.0..=1.0),
                ("render.material_roughness", &mut view.roughness, 0.045..=1.0),
                ("render.material_ior", &mut view.ior, 1.0..=3.0),
            ] {
                let label = app.tr(key).to_string();
                ui_kit::row(ui, &label, |ui| ui_kit::number_field(ui, value, range, 0.01, ""));
            }
            let current = view.revision == Some(app.session.revision)
                && view.document.as_ref().is_some_and(|d| Weak::ptr_eq(d, &Arc::downgrade(&app.session.doc)))
                && !app.session.model.is_stale();
            if !current {
                ui_kit::text(ui, app.tr("render.material_refresh_needed"), ui_kit::Text::Note("render.material_refresh_needed"));
            }
            if !view.opaque {
                ui_kit::text(ui, app.tr("render.base_opaque_only"), ui_kit::Text::Note("render.base_opaque_only"));
            }
            if ui_kit::button(ui, app.tr("render.base_apply"), ui_kit::Button::Enabled(current && view.opaque)).clicked()
                && let Some(name) = &view.selected
            {
                match app.run(
                    "appearance.assign",
                    json!({"bodies":[name],"color":view.color,"opacity":1.0,
                    "pbr":{"metallic":view.metallic,"roughness":view.roughness,"dielectric_ior":view.ior}}),
                ) {
                    Ok(_) => {
                        view.revision = Some(app.session.revision);
                        view.document = Some(Arc::downgrade(&app.session.doc));
                    }
                    Err(error) => app.set_status(error, true),
                }
            }
            ui_kit::text(ui, app.tr("render.base_apply_help"), ui_kit::Text::Note("render.base_apply_help"));
        }
        app.render_workspace.view.inspector.appearance = view;
    });
}

#[cfg(test)]
#[path = "render_workspace_inspector_tests.rs"]
mod tests;

pub(super) fn fit_subject(app: &mut SolveApp) -> Result<(), String> {
    // Capture the current navigation orientation before the engine's view command.
    app.run("render.camera.set", json!({"camera":app.cam}))?;
    app.run("render.camera.fit", json!({}))?;
    app.follow_view_request(false);
    Ok(())
}

pub(super) fn restore_camera(app: &mut SolveApp) -> Result<(), String> {
    let name = app.render_workspace.view.name.clone();
    app.run("render.settings.apply", json!({"name":name}))?;
    app.follow_view_request(false);
    Ok(())
}
fn camera_actions(app: &mut SolveApp, ui: &mut egui::Ui) {
    ui.horizontal_wrapped(|ui| {
        if ui_kit::button(ui, app.tr("render.fit"), ui_kit::Button::Normal).clicked()
            && let Err(error) = fit_subject(app)
        {
            app.set_status(error, true);
        }
        if ui_kit::button(ui, app.tr("render.restore_camera"), ui_kit::Button::Normal).clicked()
            && let Err(error) = restore_camera(app)
        {
            app.set_status(error, true);
        }
    });
}

pub(super) fn confirmation_open(app: &SolveApp) -> bool {
    app.render_workspace.view.inspector.pending.is_some()
}
pub(super) fn reveal(app: &mut SolveApp, section: &'static str) {
    app.render_workspace.view.inspector.reveal = Some(section);
}
fn reveal_section(app: &mut SolveApp, ui: &mut egui::Ui, section: &'static str) {
    if app.render_workspace.view.inspector.reveal == Some(section) {
        let identity = ui.make_persistent_id(section);
        ui.ctx().data_mut(|data| data.insert_persisted(identity, true));
        ui.scroll_to_cursor(Some(egui::Align::Min));
        app.render_workspace.view.inspector.reveal = None;
    }
}

fn gpu_status(app: &SolveApp, ui: &mut egui::Ui) {
    if app.render_workspace.view.method != ImageMethod::GpuRaytrace {
        return;
    }
    match &app.render_workspace.path_host {
        super::PathHost::Preparing(_, started) => super::setup_progress(ui, app, *started),
        super::PathHost::Failed(error) => {
            ui_kit::text(ui, &format!("{}: {error}", app.tr("render.gpu_path_failed")), ui_kit::Text::Error("render.gpu_path_failed"));
        }
        super::PathHost::NotStarted => {
            ui_kit::text(ui, app.tr("render.gpu_unavailable"), ui_kit::Text::Note("render.gpu_unavailable"));
        }
        super::PathHost::Ready(_) => {}
    }
}
