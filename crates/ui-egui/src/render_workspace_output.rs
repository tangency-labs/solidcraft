//! Contextual native Render UI. One workspace, existing command authority and shared controls.
//! Output, sampling and request checks; unsupported capabilities stay visible with a reason.
use super::{ImageMethod, cpu, library, materials, setups, studio};
use crate::{SolveApp, ui_kit};
use serde_json::{Value, json};
use solidcraft_engine::doc::Document;
use std::sync::{Arc, Weak};

#[path = "render_ui/appearance.rs"]
pub(super) mod selection_appearance;

#[derive(Clone, Debug, Default)]
pub(super) struct View {
    pub effects: super::effects::View,
    pub hidden: bool,
    reveal: Option<&'static str>,
    pending: Option<Pending>,

    pub checked: Option<Review>,
    pub quality_review: Option<Value>,
    scene_info: Option<Review>,
    pub(super) selection_appearance: selection_appearance::View,
    pub(super) resolution: Resolution,
}
/// Editor-only coupling. The fixed rational anchor prevents repeated rounded
/// edits drifting; external preset/shot changes deliberately establish a new anchor.
#[derive(Clone, Debug, Default)]
pub(super) struct Resolution {
    locked: bool,
    pub(super) custom: bool,
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
    ManageShot { name: String, command: String },
    Remove(String),
}
#[derive(Clone, Debug)]
pub(super) struct Review {
    pub(super) request: Value,
    pub(super) reply: Value,
    revision: u64,
    document: Weak<Document>,
}
impl Review {
    pub(super) fn current(&self, app: &SolveApp) -> bool {
        self.revision == app.session.revision && Weak::ptr_eq(&self.document, &Arc::downgrade(&app.session.doc))
    }
}

pub(super) fn record_check(app: &mut SolveApp, request: &Value, reply: &Value) {
    app.render_workspace.view.inspector.checked =
        Some(Review { request: request.clone(), reply: reply.clone(), revision: app.session.revision, document: Arc::downgrade(&app.session.doc) });
}

pub(super) fn switch_shot(app: &mut SolveApp, name: String) {
    request_action(app, PendingAction::Switch(name));
}

pub(super) fn shot_row_action(app: &mut SolveApp, name: String, command: &str) {
    setups::refresh_dirty(app);
    if command == "render.shot.remove" {
        request_action(app, PendingAction::Remove(name));
    } else {
        request_action(app, PendingAction::ManageShot { name, command: command.into() });
    }
}

fn request_action(app: &mut SolveApp, action: PendingAction) {
    if matches!(&action, PendingAction::Remove(_)) {
        app.render_workspace.view.inspector.pending =
            Some(Pending { action, document: Arc::downgrade(&app.session.doc), revision: app.session.revision });
    } else if let Err(error) = perform_action(app, action) {
        app.set_status(error, true);
    }
}
fn perform_action(app: &mut SolveApp, action: PendingAction) -> Result<(), String> {
    match action {
        PendingAction::Switch(name) => {
            app.render_workspace.view.name = name.clone();
            setups::load(app);
            super::select(app, super::RenderSelection::Shot(name));
        }
        PendingAction::ManageShot { name, command } => {
            app.render_workspace.view.name = name;
            setups::load(app);
            // The existing manager owns name validation and command execution.
            app.start(&command);
        }
        PendingAction::Remove(name) => {
            app.run("render.shot.remove", json!({"name":name,"expected_revision":app.session.revision}))?;
            if name == app.render_workspace.view.name {
                app.render_workspace.view.name = app.session.doc.render.setups.keys().next().cloned().unwrap_or_else(|| "Studio".into());
                setups::load(app);
            }
            if matches!(&app.render_workspace.view.selection, super::RenderSelection::Shot(selected) if selected == &name) {
                app.run("view.render_select", json!({"kind":"nothing"}))?;
            }
        }
    }
    Ok(())
}
pub(super) fn confirmation(app: &mut SolveApp, ctx: &egui::Context) {
    super::shots::confirmation(app, ctx);
    materials::confirmation(app, ctx);
    let Some(pending) = app.render_workspace.view.inspector.pending.clone() else { return };
    if pending.revision != app.session.revision || !Weak::ptr_eq(&pending.document, &Arc::downgrade(&app.session.doc)) {
        app.render_workspace.view.inspector.pending = None;
        app.set_status(app.tr("render.review_stale").to_string(), true);
        return;
    }
    let title = app.tr("render.shot_remove_title").to_string();
    let keep = app.tr("render.stage_panels.keep").to_string();
    let primary = app.tr("render.stage_panels.remove_shot").to_string();
    let message = app.tr("render.shot_remove_help").to_string();
    let (closed, answer) = ui_kit::Confirm::new(egui::Id::new("render_pending_shot"), "delete", &title)
        .tone(ui_kit::Tone::Danger)
        .actions(&primary, &keep, None)
        .selected(ui_kit::Action::Cancel)
        .show(ctx, true, |ui| {
            ui_kit::text(ui, &message, ui_kit::Text::Inline);
        });
    if closed || answer != ui_kit::Action::None {
        app.render_workspace.view.inspector.pending = None;
    }
    if answer == ui_kit::Action::Ok
        && let Err(error) = perform_action(app, pending.action)
    {
        app.set_status(error, true);
    }
}
fn browse_button(app: &mut SolveApp, ui: &mut egui::Ui, kind: &str, key: &str) {
    if ui_kit::button(ui, app.tr(key), ui_kit::Button::Normal).clicked() {
        library::open_category(app, kind);
    }
}

pub(super) fn apply_camera_draft(app: &mut SolveApp, camera: solidcraft_engine::render::Camera) -> Result<(), String> {
    app.run("render.camera.set", json!({"camera":camera}))?;
    app.follow_view_request(false);
    app.render_workspace.view.dirty = true;
    Ok(())
}

fn editor_section(app: &mut SolveApp, ui: &mut egui::Ui, id: &'static str, key: &'static str, draw: fn(&mut SolveApp, &mut egui::Ui)) {
    reveal_section(app, ui, id);
    let title = app.tr(key).to_string();
    ui_kit::section(ui, id, &title, true, |ui| draw(app, ui));
}
pub(super) fn output_editor(app: &mut SolveApp, ui: &mut egui::Ui) {
    let summary = check_summary(app);
    let notes = ui_kit::button(ui, &summary, ui_kit::Button::Normal);
    crate::scenario::publish_target("render:output:check", notes.rect);
    if notes.clicked() {
        let _ = app.run("view.render_select", json!({"kind":"check"}));
    }
    for (id, key, draw) in [
        ("render_output_renderer", "render.children.renderer", renderer_controls as fn(&mut SolveApp, &mut egui::Ui)),
        ("render_output_image", "render.output_image", output_controls),
        ("render_output_destination", "render.output_destination", destination_controls),
        ("render_output_render", "render.children.render", render_controls),
    ] {
        editor_section(app, ui, id, key, draw);
    }
}
fn renderer_controls(app: &mut SolveApp, ui: &mut egui::Ui) {
    let label = app.tr("render.children.method").to_string();
    ui_kit::row(ui, &label, |ui| super::method_dropdown(app, ui));
    gpu_status(app, ui);
    let status = super::image_host(app, app.render_workspace.view.method).map_or_else(|e| e, |_| app.tr("render.output_backend_ready").into());
    ui_kit::text(ui, &status, ui_kit::Text::Inline);
    let key = match app.render_workspace.view.method {
        ImageMethod::Raster => "render.opaque_image_summary",
        ImageMethod::CpuRaytrace => "render.cpu_summary",
        ImageMethod::GpuRaytrace => "render.progressive_image_summary",
    };
    ui_kit::text(ui, app.tr(key), ui_kit::Text::Note(key));
}
fn output_controls(app: &mut SolveApp, ui: &mut egui::Ui) {
    let side = match app.render_workspace.view.method {
        ImageMethod::CpuRaytrace => 16384,
        ImageMethod::GpuRaytrace => 8192,
        ImageMethod::Raster => 4096,
    };
    let before = [app.render_workspace.view.draft.width, app.render_workspace.view.draft.height];
    app.render_workspace.view.inspector.resolution.observe(before);
    let mut width = before[0];
    let mut height = before[1];
    let [w, h] = ui_kit::number_pair(ui, app.tr("render.output_dimensions"), [&mut width, &mut height], 1..=side, 1.0, " px");
    crate::scenario::publish_target("render:output-width", w.rect);
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
        && let Err(error) = apply_size(app, width, height, true)
    {
        app.render_workspace.view.inspector.resolution.observe(before);
        app.render_workspace.view.inspector.resolution.rejected = true;
        app.set_status(error, true);
    }
    if app.render_workspace.view.inspector.resolution.rejected {
        ui_kit::text(ui, app.tr("render.output_ratio_refused"), ui_kit::Text::Error("render.output_ratio_refused"));
    }
    let label = app.tr("render.output_lock_ratio").to_string();
    let lock = ui_kit::checkbox(ui, &label, &mut app.render_workspace.view.inspector.resolution.locked);
    crate::scenario::publish_target("render:aspect-lock", lock.rect);
    let swap = ui_kit::button(ui, app.tr("render.output_swap_orientation"), ui_kit::Button::Normal);
    crate::scenario::publish_target("render:swap-size", swap.rect);
    if swap.clicked() {
        if let Err(error) = apply_size(app, before[1], before[0], true) {
            app.set_status(error, true);
        }
        let draft = &app.render_workspace.view.draft;
        app.render_workspace.view.inspector.resolution.observe([draft.width, draft.height]);
    }
    let largest = largest_output(app);
    let (w, h) = largest.unwrap_or((0, 0));
    let label = crate::i18n::format("render.output_largest", &[("width", &w.to_string()), ("height", &h.to_string())]);
    let button = ui_kit::button(ui, &label, ui_kit::Button::Enabled(largest.is_some()));
    crate::scenario::publish_target("render:size:largest", button.rect);
    if button.clicked()
        && let (Ok(w), Ok(h)) = (u32::try_from(w), u32::try_from(h))
        && let Err(error) = apply_size(app, w, h, false)
    {
        app.set_status(error, true);
    }
    let size = (u64::from(before[0]), u64::from(before[1]));
    if cpu::image_limits(app).fit(size.0, size.1, None) != Some(size) {
        ui_kit::text(ui, &size_limit_reason(app, before[0], before[1]), ui_kit::Text::Error("render.output_over_budget"));
    }
    studio::export_controls(app, ui);
    let gpu = app.render_workspace.view.method == ImageMethod::GpuRaytrace;
    if !gpu {
        ui_kit::text(ui, app.tr("render.effects_gpu_only"), ui_kit::Text::Note("render.effects_gpu_only"));
    }
    ui.add_enabled_ui(gpu, |ui| {
        let label = app.tr("render.children.keep_master").to_string();
        let response = ui_kit::checkbox(ui, &label, &mut app.render_workspace.view.studio.linear_exr);
        crate::scenario::publish_target("render:retain-exr", response.rect);
        app.render_workspace.view.dirty |= response.changed();
    });
    let label = app.tr("render.image_transparent").to_string();
    let cpu = app.render_workspace.view.method == ImageMethod::CpuRaytrace;
    if cpu {
        ui_kit::text(ui, app.tr("render.cpu_opaque_required"), ui_kit::Text::Note("render.cpu_opaque_required"));
    }
    // An incompatible retained value can always be cleared without switching method.
    ui.add_enabled_ui(!cpu || app.render_workspace.view.transparent, |ui| {
        ui_kit::checkbox(ui, &label, &mut app.render_workspace.view.transparent);
    });
}
fn destination_controls(app: &mut SolveApp, ui: &mut egui::Ui) {
    let label = app.tr("render.image_overwrite").to_string();
    ui_kit::checkbox(ui, &label, &mut app.render_workspace.view.overwrite);
    ui.horizontal_wrapped(|ui| {
        for (id, label) in
            [("render.export", studio::export_label(app).to_string()), ("render.result.copy", app.tr("render.children.copy_image").to_string())]
        {
            let ready = super::ribbon::action_ready(app, id);
            let button = ui_kit::button(ui, &label, ui_kit::Button::Enabled(ready.is_ok()));
            if button.clicked() {
                app.start(id);
            }
            if let Err(reason) = ready {
                button.on_disabled_hover_text(reason);
            }
        }
    });
}
fn render_controls(app: &mut SolveApp, ui: &mut egui::Ui) {
    let last = app.render_workspace.view.completed.and_then(|job| app.session.render_job_handle(job).ok()).and_then(|h| h.status().ok());
    let text = last.map_or_else(
        || app.tr("render.shots_no_result").to_string(),
        |s| {
            crate::i18n::format(
                "render.children.last_result",
                &[
                    ("number", &s.job.serial.to_string()),
                    ("width", &s.request.width.to_string()),
                    ("height", &s.request.height.to_string()),
                    ("phase", app.tr(&format!("render.image_{:?}", s.progress.phase).to_lowercase())),
                ],
            )
        },
    );
    ui_kit::completion_text(ui, &text, app.render_workspace.view.completed.is_some());
    if super::ribbon::busy(app) {
        super::progress_pill(app, ui);
        return;
    }
    if ui_kit::button(ui, app.tr("render.output_view_image"), ui_kit::Button::Enabled(app.render_workspace.view.completed.is_some())).clicked() {
        let _ = app.run("render.result.view", json!({"view":"image"}));
    }
}
pub(super) fn sampling_editor(app: &mut SolveApp, ui: &mut egui::Ui) {
    editor_section(app, ui, "render_sampling_quality", "render.output_quality", |app, ui| {
        quality_choices(app, ui);
        ui_kit::text(ui, app.tr("render.output_quality_waiting"), ui_kit::Text::Note("render.output_quality_waiting"));
        browse_button(app, ui, "quality", "render.children.review_quality");
        library::inspect_kinds(app, ui, &["quality"]);
    });
    for (id, key, method, draw) in [
        ("render_sampling_path", "render.children.path", ImageMethod::GpuRaytrace, path_controls as fn(&mut SolveApp, &mut egui::Ui)),
        ("render_sampling_adaptive", "render.children.adaptive", ImageMethod::GpuRaytrace, super::transport::adaptive_controls),
        ("render_sampling_glass", "render.children.glass", ImageMethod::GpuRaytrace, super::transport::glass_controls),
        ("render_sampling_cpu", "render.children.cpu", ImageMethod::CpuRaytrace, cpu_sampling),
    ] {
        let title = app.tr(key).to_string();
        let reason = (app.render_workspace.view.method != method)
            .then(|| app.tr(if method == ImageMethod::GpuRaytrace { "render.effects_gpu_only" } else { "render.children.needs_cpu" }).to_string());
        ui_kit::section_method(ui, id, &title, if method == ImageMethod::GpuRaytrace { "GPU" } else { "CPU" }, true, |ui| {
            ui.add_enabled_ui(reason.is_none(), |ui| draw(app, ui));
        });
        if let Some(reason) = reason {
            let key = if method == ImageMethod::GpuRaytrace { "render.effects_gpu_only" } else { "render.children.needs_cpu" };
            ui_kit::text(ui, &reason, ui_kit::Text::Note(key));
        }
    }
}
fn path_controls(app: &mut SolveApp, ui: &mut egui::Ui) {
    let label = app.tr("render.children.samples").to_string();
    let mut samples = app.render_workspace.view.samples.clamp(1, 4096);
    if ui_kit::row(ui, &label, |ui| {
        ui_kit::slider(
            ui,
            &mut samples,
            1..=4096,
            ui_kit::Slider { integer: true, step: Some(1.0), format: |v| format!("{v:.0}"), ..Default::default() },
        )
    })
    .changed()
    {
        app.render_workspace.view.samples = samples.checked_next_power_of_two().unwrap_or(4096).min(4096);
    }
    let label = app.tr("render.children.depth").to_string();
    let mut depth = app.render_workspace.view.max_depth.clamp(1, 16);
    if ui_kit::row(ui, &label, |ui| ui_kit::number_field(ui, &mut depth, 1..=16, 1.0, "")).changed() {
        app.render_workspace.view.max_depth = depth;
    }
    let label = app.tr("render.children.seed").to_string();
    ui_kit::row(ui, &label, |ui| ui_kit::number_field(ui, &mut app.render_workspace.view.seed, 0..=u32::MAX, 1.0, ""));
    let label = app.tr("render.children.denoise").to_string();
    ui_kit::checkbox(ui, &label, &mut app.render_workspace.view.studio.denoise);
    ui_kit::text(ui, app.tr("render.studio_denoise_help"), ui_kit::Text::Note("render.studio_denoise_help"));
}
fn cpu_sampling(app: &mut SolveApp, ui: &mut egui::Ui) {
    super::cpu_sampling_controls(app, ui);
    let mut reflections = app.render_workspace.view.raytrace.reflections.clone().unwrap_or_default();
    if app.render_workspace.view.raytrace.reflections.is_none() {
        ui_kit::text(ui, app.tr("render.cpu_reflection_empty"), ui_kit::Text::Note("render.cpu_reflection_empty"));
    }
    let depth = app.tr("render.children.reflect_depth").to_string();
    let samples = app.tr("render.children.reflect_samples").to_string();
    ui.add_enabled_ui(app.render_workspace.view.raytrace.reflections.is_some(), |ui| {
        ui_kit::row(ui, &depth, |ui| ui_kit::number_field(ui, &mut reflections.max_bounces, 1..=8, 1.0, ""));
        ui_kit::row(ui, &samples, |ui| ui_kit::number_field(ui, &mut reflections.samples, 1..=64, 1.0, ""));
    });
    if let Some(value) = &mut app.render_workspace.view.raytrace.reflections {
        *value = reflections;
    }
}
pub(super) fn check_editor(app: &mut SolveApp, ui: &mut egui::Ui) {
    editor_section(app, ui, "render_check_check", "render.children.check", check_controls);
    editor_section(app, ui, "render_check_request", "render.children.request", request_controls);
    editor_section(app, ui, "render_check_renderers", "render.children.renderers", renderer_status);
    let title = app.tr("render.children.under_hood").to_string();
    ui_kit::section(ui, "render_check_under_hood", &title, false, |ui| diagnostics(app, ui));
}
fn check_controls(app: &mut SolveApp, ui: &mut egui::Ui) {
    let ready = super::ribbon::action_ready(app, "render.preflight");
    let check = ui_kit::button(ui, app.tr("render.children.check_now"), ui_kit::Button::Enabled(ready.is_ok()));
    crate::scenario::publish_target("render:check", check.rect);
    if check.clicked() {
        app.start("render.preflight");
    }
    if let Err(reason) = ready {
        check.on_disabled_hover_text(reason);
    }
    output_check(app, ui);
    let report = check_report(app);
    let copy = ui_kit::button(ui, app.tr("render.output_copy_report"), ui_kit::Button::Enabled(report.is_some()));
    crate::scenario::publish_target("render:check:copy", copy.rect);
    if copy.clicked()
        && let Some(text) = report
        && let Err(error) = app.run("render.preflight.copy", json!({"text":text}))
    {
        app.set_status(error, true);
    }
}
fn check_note_link(app: &mut SolveApp, ui: &mut egui::Ui, index: usize, line: &CheckLine) {
    if line.status == ui_kit::CheckStatus::Ok {
        return;
    }
    let message = line.label.to_ascii_lowercase();
    let (kind, key) = match line.key {
        "transparency" => ("output", "render.page_output"),
        "stale" => ("check", "render.children.check"),
        _ if ["sample", "sampling", "depth", "budget", "memory", "denois"].iter().any(|word| message.contains(word)) => {
            ("sampling", "render.children.sampling")
        }
        _ if ["hdr", "environment"].iter().any(|word| message.contains(word)) => ("environment", "render.page_environment"),
        _ if ["exposure", "camera", "lens"].iter().any(|word| message.contains(word)) => ("nothing", "render.page_camera"),
        _ if ["bloom", "tone", "look", "effect"].iter().any(|word| message.contains(word)) => ("effects", "render.output_effects"),
        _ if ["glass", "optic", "material", "texture", "coat"].iter().any(|word| message.contains(word)) => ("nothing", "render.page_appearance"),
        _ => ("output", "render.page_output"),
    };
    let label = crate::i18n::format("render.children.go_to", &[("editor", app.tr(key))]);
    let response = ui_kit::button(ui, &label, ui_kit::Button::Normal);
    crate::scenario::publish_target(&format!("render:check:go-to:{index}"), response.rect);
    if response.clicked() {
        let _ = app.run("view.render_select", json!({"kind":kind}));
        if kind == "nothing" {
            let selection = if key == "render.page_camera" { super::current_shot_selection(app) } else { super::object_selection(app) };
            super::select(app, selection);
        }
    }
}
fn request_controls(app: &mut SolveApp, ui: &mut egui::Ui) {
    let view = &app.render_workspace.view;
    let check = view.inspector.checked.as_ref().filter(|c| c.current(app) && super::image_params(app).as_ref().ok() == Some(&c.request));
    let memory = check
        .and_then(|c| c.reply["admission"]["estimated_total_bytes"].as_u64())
        .map_or_else(|| app.tr("render.children.not_checked").into(), |b| format!("{:.2} MiB", b as f64 / 1048576.0));
    let hdr = check.map_or_else(
        || app.tr("render.children.not_checked").into(),
        |c| if c.reply["environment"].is_null() { app.tr("render.studio_environment_none").to_string() } else { c.reply["environment"].to_string() },
    );
    let sampling = if view.method == ImageMethod::GpuRaytrace {
        format!("{} spp · {}", view.samples, view.max_depth)
    } else {
        format!("{} spp", view.raytrace.progressive.map_or(u32::from(view.raytrace.primary_samples), |p| p.samples_per_pixel))
    };
    let scene = view.inspector.scene_info.as_ref().filter(|s| s.current(app)).map_or_else(
        || app.tr("render.children.not_checked").to_string(),
        |s| {
            let bodies = s.reply["bodies"].as_array();
            let triangles = bodies.map_or(0, |b| b.iter().filter_map(|v| v["triangles"].as_u64()).fold(0u64, u64::saturating_add));
            crate::i18n::format(
                "render.children.scene_summary",
                &[("bodies", &bodies.map_or(0, Vec::len).to_string()), ("triangles", &triangles.to_string())],
            )
        },
    );
    let preview = app.render_workspace.preview_upload_bytes.map_or_else(
        || app.tr("render.children.not_checked").to_string(),
        |b| format!("{:.2} / {} MiB", b as f64 / 1048576.0, super::MAX_GEOMETRY_BYTES / 1048576),
    );
    for (key, value) in [
        ("render.children.method", app.tr(view.method.label()).to_string()),
        ("render.output_image", format!("{} × {} · {}", view.draft.width, view.draft.height, if view.studio.export_exr { "EXR" } else { "PNG" })),
        ("render.children.sampling", sampling),
        ("render.children.memory", memory),
        ("render.children.hdr", hdr),
        ("render.page_scene", scene),
        ("render.children.preview", preview),
    ] {
        ui_kit::row(ui, app.tr(key), |ui| ui_kit::text(ui, &value, ui_kit::Text::Inline));
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
}
fn renderer_status(app: &mut SolveApp, ui: &mut egui::Ui) {
    gpu_status(app, ui);
    let backend = app.session.render_backend();
    for method in [ImageMethod::GpuRaytrace, ImageMethod::CpuRaytrace, ImageMethod::Raster] {
        let status = super::image_host(app, method).map_or_else(|e| e, |_| app.tr("render.output_backend_ready").to_string());
        ui_kit::row(ui, app.tr(method.label()), |ui| ui_kit::text(ui, &status, ui_kit::Text::Inline));
    }
    ui_kit::text(ui, &backend.label, ui_kit::Text::Inline);
    for executor in backend.executors.iter().take(16) {
        let status = match &executor.availability {
            super::Availability::Available => app.tr("render.output_backend_ready"),
            super::Availability::Unavailable { reason } => reason,
        };
        ui_kit::check_status_line(
            ui,
            &format!("render:check:backend:{}", executor.id.as_str()),
            &format!("{} · {status}", executor.id.as_str()),
            if matches!(executor.availability, super::Availability::Available) { ui_kit::CheckStatus::Ok } else { ui_kit::CheckStatus::Warning },
        );
    }
    if let Some(error) = &app.render_workspace.scene_error {
        ui_kit::text(ui, error, ui_kit::Text::Error("render.preview_cached_error"));
    }
}
/// Fit an oversized same-aspect candidate, since fit deliberately never upscales.
pub(super) fn largest_output(app: &SolveApp) -> Option<(u64, u64)> {
    let limits = cpu::image_limits(app);
    let draft = &app.render_workspace.view.draft;
    limits.fit(u64::from(draft.width).checked_mul(limits.max_dimension)?, u64::from(draft.height).checked_mul(limits.max_dimension)?, None)
}
pub(super) fn quality_choices(app: &mut SolveApp, ui: &mut egui::Ui) {
    let reason = app.tr("render.output_quality_waiting");
    let labels = ["render.quality_draft", "render.quality_balanced", "render.quality_high", "render.quality_custom"].map(|key| app.tr(key));
    let mut quality = 3;
    if ui_kit::segmented(
        ui,
        &mut quality,
        &labels,
        ui_kit::Segments {
            workspace_accent: true,
            intrinsic: true,
            compact: true,
            targets: &["render:quality:draft", "render:quality:balanced", "render:quality:high", "render:quality:custom"],
            disabled: &[Some(reason), Some(reason), Some(reason), None],
            ..Default::default()
        },
    ) {
        let _ = app.run("view.render_select", json!({"kind":"sampling"}));
    }
}
pub(super) fn check_summary(app: &SolveApp) -> String {
    let state = match &app.render_workspace.view.inspector.checked {
        Some(check) if check.current(app) && super::image_params(app).as_ref().ok() == Some(&check.request) => {
            if check.reply["error"].is_string() {
                return app.tr("render.output_check_failed").into();
            }
            let count = check.reply["warnings"].as_array().map_or(0, Vec::len)
                + usize::from(check.reply["transparency"].as_str() == Some("sorted_alpha_approximation"));
            return crate::i18n::format("render.output_check_summary", &[("count", &count.to_string())]);
        }
        Some(_) => "render.studio_needs_review",
        None => return app.tr("render.children.not_checked").into(),
    };
    format!("{} · {}", app.tr("render.output_check"), app.tr(state))
}

const SIZE_PRESETS: [(&str, &str, Option<(u32, u32)>); 5] = [
    ("720", "render.size_preview", Some((1280, 720))),
    ("1080", "render.size_full_hd", Some((1920, 1080))),
    ("4k", "render.size_4k", Some((3840, 2160))),
    ("square", "render.size_square", Some((2048, 2048))),
    ("custom", "render.size_custom", None),
];
fn choose_size(app: &mut SolveApp, size: Option<(u32, u32)>) {
    let custom = size.is_none();
    let (width, height) = size.unwrap_or((app.render_workspace.view.draft.width, app.render_workspace.view.draft.height));
    let result = if custom {
        app.run("render.output.size", json!({"width":width,"height":height,"custom":true})).map(|_| ())
    } else {
        apply_size(app, width, height, false)
    };
    if let Err(error) = result {
        app.set_status(error, true);
    } else if custom {
        let _ = app.run("view.render_select", json!({"kind":"output"}));
        reveal(app, "render_output_image");
    }
}
pub(super) fn apply_size(app: &mut SolveApp, width: u32, height: u32, custom: bool) -> Result<(), String> {
    if cpu::image_limits(app).fit(u64::from(width), u64::from(height), None) != Some((u64::from(width), u64::from(height))) {
        return Err(size_limit_reason(app, width, height));
    }
    let name = app.render_workspace.view.name.clone();
    let mut patch = json!({"width":width,"height":height});
    let mut saved = app.session.doc.render.setups.get(&name).cloned().unwrap_or_default();
    saved.width = width;
    saved.height = height;
    if saved.validate().is_err() {
        // A larger preset needs the explicitly selected executor's resource profile,
        // rather than the smaller raster profile of an older saved shot.
        patch["job"] = serde_json::to_value(setups::snapshot(app)?.job).map_err(|error| error.to_string())?;
    }
    app.run("render.settings.set", json!({"name":name,"set":patch}))?;
    app.render_workspace.view.draft.width = width;
    app.render_workspace.view.draft.height = height;
    app.render_workspace.view.inspector.resolution.custom = custom;
    app.render_workspace.view.dirty = true;
    Ok(())
}
fn size_limit_reason(app: &SolveApp, width: u32, height: u32) -> String {
    let limits = cpu::image_limits(app);
    let (w, h) = limits.fit(u64::from(width), u64::from(height), None).unwrap_or((0, 0));
    crate::i18n::format(
        "render.output_size_limit",
        &[
            ("method", app.tr(app.render_workspace.view.method.label())),
            ("dimension", &limits.max_dimension.to_string()),
            ("pixels", &limits.max_pixels.to_string()),
            ("width", &w.to_string()),
            ("height", &h.to_string()),
        ],
    )
}
pub(super) fn size_presets(app: &mut SolveApp, ui: &mut egui::Ui) {
    egui::ScrollArea::vertical().id_salt("render_size_quality").auto_shrink([false, false]).show(ui, |ui| {
        ui_kit::scope(ui, |ui| {
            let current = (app.render_workspace.view.draft.width, app.render_workspace.view.draft.height);
            let custom = app.render_workspace.view.inspector.resolution.custom || !SIZE_PRESETS.iter().any(|(_, _, size)| *size == Some(current));
            for (key, label, size) in SIZE_PRESETS {
                let summary = size.map_or_else(String::new, |(w, h)| format!("{w} × {h}"));
                let enabled =
                    size.is_none_or(|(w, h)| cpu::image_limits(app).fit(u64::from(w), u64::from(h), None) == Some((u64::from(w), u64::from(h))));
                let response = ui
                    .add_enabled_ui(enabled, |ui| {
                        ui_kit::settings_choice_row(
                            ui,
                            &ui_kit::SettingsChoice {
                                key: &format!("render:preset:{key}"),
                                title: app.tr(label),
                                summary: &summary,
                                detail: app.tr(label),
                            },
                            if size.is_none() { custom } else { !custom && size == Some(current) },
                        )
                    })
                    .inner;
                let reason = size.map_or_else(String::new, |(w, h)| size_limit_reason(app, w, h));
                if response.on_disabled_hover_text(reason).clicked() {
                    choose_size(app, size);
                }
            }
            ui_kit::text(ui, app.tr("render.output_presets_help"), ui_kit::Text::Note("render.output_presets_help"));
            let title = app.tr("render.library_quality_concepts").to_owned();
            ui_kit::section(ui, "render_quality_specs", &title, true, |ui| library::catalogue(app, ui, "quality", true));
        })
    });
}
pub(super) fn stage_tools(app: &mut SolveApp, ui: &mut egui::Ui) {
    let (size, _) = super::output_summary(app);
    let dropdown = ui_kit::compact_dropdown_menu(ui, "render_stage_size", &size, |ui| {
        for (key, label, size) in SIZE_PRESETS {
            let enabled =
                size.is_none_or(|(w, h)| cpu::image_limits(app).fit(u64::from(w), u64::from(h), None) == Some((u64::from(w), u64::from(h))));
            let response = ui.add_enabled_ui(enabled, |ui| ui_kit::list_picker(ui, key, app.tr(label), false)).inner;
            crate::scenario::publish_target(&format!("render:stage:size:{key}"), response.rect);
            let reason = size.map_or_else(String::new, |(w, h)| size_limit_reason(app, w, h));
            if response.on_disabled_hover_text(reason).clicked() {
                choose_size(app, size);
                ui.close();
            }
        }
    });
    crate::scenario::publish_target("render:stage:size", dropdown.rect);
    studio::format_choices(app, ui);
    quality_choices(app, ui);
    let summary = check_summary(app);
    let response = ui_kit::button(ui, &summary, ui_kit::Button::Normal).on_hover_text(app.tr("render.output_check_help"));
    crate::scenario::publish_target("render:stage:check", response.rect);
    if response.clicked() {
        let _ = app.run("view.render_select", json!({"kind":"check"}));
    }
}

struct CheckLine {
    key: &'static str,
    label: String,
    status: ui_kit::CheckStatus,
}
fn check_lines(app: &SolveApp) -> Vec<CheckLine> {
    use ui_kit::CheckStatus::{Error, Ok, Warning};
    if let Err(error) = super::image_params(app) {
        return vec![CheckLine { key: "error", label: error, status: Error }];
    }
    let Some(check) = &app.render_workspace.view.inspector.checked else {
        return Vec::new();
    };
    if !check.current(app) || super::image_params(app).as_ref().ok() != Some(&check.request) {
        return vec![CheckLine { key: "stale", label: app.tr("render.review_stale").into(), status: Warning }];
    }
    if let Some(error) = check.reply["error"].as_str() {
        return vec![CheckLine { key: "error", label: error.into(), status: Error }];
    }
    let mut lines = vec![CheckLine { key: "ready", label: app.tr("render.output_ready").into(), status: Ok }];
    if let (Some(w), Some(h)) = (check.reply["request"]["width"].as_u64(), check.reply["request"]["height"].as_u64()) {
        let label = crate::i18n::format("render.output_size_ok", &[("width", &w.to_string()), ("height", &h.to_string())]);
        lines.push(CheckLine { key: "size", label, status: Ok });
    }
    if let Some(transparency) = check.reply["transparency"].as_str() {
        let key = match transparency {
            "physical_transmission" => "render.check_transmission",
            "opaque_only" => "render.check_opaque",
            _ => "render.check_sorted_alpha",
        };
        lines.push(CheckLine {
            key: "transparency",
            label: app.tr(key).into(),
            status: if transparency == "sorted_alpha_approximation" { Warning } else { Ok },
        });
    }
    if let Some(warnings) = check.reply["warnings"].as_array() {
        for warning in warnings.iter().take(32) {
            lines.push(CheckLine { key: "warning", label: warning.as_str().map_or_else(|| warning.to_string(), str::to_string), status: Warning });
        }
    }
    lines
}
fn output_check(app: &mut SolveApp, ui: &mut egui::Ui) {
    let lines = check_lines(app);
    if lines.is_empty() {
        ui_kit::text(ui, app.tr("render.children.not_checked"), ui_kit::Text::Inline);
        ui_kit::text(ui, app.tr("render.output_check_help"), ui_kit::Text::Note("render.output_check_help"));
    }
    for (index, line) in lines.iter().enumerate() {
        let key = format!("render:check:{}:{index}", line.key);
        let response = if line.status == ui_kit::CheckStatus::Error {
            ui_kit::check_status_line(ui, &key, &line.label, line.status)
        } else {
            ui_kit::check_line(ui, &key, &line.label, line.status == ui_kit::CheckStatus::Warning)
        };
        crate::scenario::publish_rect(&format!("render:check:{}", line.key), response.rect);
        check_note_link(app, ui, index, line);
    }
}
fn check_report(app: &SolveApp) -> Option<String> {
    let lines = check_lines(app);
    if lines.is_empty() {
        return None;
    }
    let mut report = app.tr("render.output_check").to_string();
    for line in lines {
        let status = match line.status {
            ui_kit::CheckStatus::Ok => "✓",
            ui_kit::CheckStatus::Warning => "!",
            ui_kit::CheckStatus::Error => "×",
        };
        report.push_str(&format!("\n{status} {}", line.label));
    }
    let backend = app.session.render_backend();
    report.push_str(&format!("\n{}: {}", app.tr("cmd.render_backends"), backend.label));
    for executor in backend.executors.iter().take(16) {
        let status = match &executor.availability {
            super::Availability::Available => app.tr("render.output_backend_ready"),
            super::Availability::Unavailable { reason } => reason,
        };
        report.push_str(&format!("\n{}: {status}", executor.id.as_str()));
    }
    if let Some(check) = &app.render_workspace.view.inspector.checked {
        report.push_str(&format!("\n{}", check.reply));
    }
    if let Some(quality) = &app.render_workspace.view.inspector.quality_review {
        report.push_str(&format!("\n{}: {quality}", app.tr("render.quality_review")));
    }
    // The clipboard command accepts at most 64 KiB; truncate on character boundaries.
    let end = report.char_indices().map(|(i, _)| i).take_while(|i| *i <= 65536).last().unwrap_or(0);
    if report.len() > 65536 {
        report.truncate(end);
    }
    Some(report)
}

pub(super) fn scene_resource(app: &mut SolveApp, ui: &mut egui::Ui, id: &str) {
    if ui_kit::button(ui, app.tr("render.scene_review"), ui_kit::Button::Enabled(app.render_workspace.view.method == ImageMethod::GpuRaytrace))
        .clicked()
    {
        let mut view = library::View::default();
        if let Err(error) = library::choose(app, &mut view, "native_scenes", id).and_then(|()| library::review::light(app, &view, id)) {
            app.set_status(error, true);
        }
    }
}
pub(super) fn quality_resource(app: &mut SolveApp, ui: &mut egui::Ui, id: &str) {
    ui_kit::text(ui, app.tr("render.quality_projection_help"), ui_kit::Text::Note("render.quality_projection_help"));
    if ui_kit::button(ui, app.tr("render.quality_review"), ui_kit::Button::Enabled(app.render_workspace.view.method == ImageMethod::GpuRaytrace))
        .clicked()
        && let Err(error) = library::review::quality(app, id)
    {
        app.set_status(error, true);
    }
}
pub(super) fn apply_quality(app: &mut SolveApp, plan: &Value) -> Result<(), String> {
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
    view.inspector.quality_review = Some(plan.clone());
    view.dirty = true;
    Ok(())
}
fn diagnostics(app: &mut SolveApp, ui: &mut egui::Ui) {
    cpu::checked_status(app, ui, super::image_params(app).as_ref().ok());
    let request = super::image_request(app);
    let job = app
        .render_workspace
        .view
        .job
        .or(app.render_workspace.view.completed)
        .and_then(|job| app.session.execute("render.status", &json!({"job":job})).ok());
    for (key, value) in [("render.children.copy_request", request.as_ref().ok()), ("render.children.copy_job", job.as_ref())] {
        let response = ui_kit::button(ui, app.tr(key), ui_kit::Button::Enabled(value.is_some()));
        crate::scenario::publish_target(
            if key == "render.children.copy_request" { "render:check:copy-request" } else { "render:check:copy-job" },
            response.rect,
        );
        if response.clicked()
            && let Some(value) = value
        {
            copy_json(app, ui, value);
        }
    }
    if let Err(error) = &request {
        ui_kit::text(ui, error, ui_kit::Text::Error("render.native-error"));
    }
    if let Ok(value) = request {
        let text = serde_json::to_string_pretty(&value).unwrap_or_default();
        ui_kit::read_only_text(ui, &text);
    }
    if let Some(check) = &app.render_workspace.view.inspector.checked {
        json_details(app, ui, "render.diag_preflight_details", &check.reply);
    }
    if let Some(job) = job {
        json_details(app, ui, "render.diag_job_details", &job);
    }
    for key in
        ["render.adaptive_pending", "render.exr_pending", "render.transmission_scope", "render.advanced_material_scope", "render.queue_pending"]
    {
        ui_kit::text(ui, app.tr(key), ui_kit::Text::Note(key));
    }
    browse_button(app, ui, "capabilities", "render.browse_capabilities");
    if library::browsing(app, "capabilities") {
        library::catalogue(app, ui, "capabilities", false);
    }
    library::inspect_kinds(app, ui, &["capabilities"]);
}
fn copy_json(app: &mut SolveApp, _ui: &mut egui::Ui, value: &Value) {
    if let Ok(text) = serde_json::to_string_pretty(value)
        && let Err(error) = app.run("render.preflight.copy", json!({"text":text}))
    {
        app.set_status(error, true);
    }
}
fn json_details(app: &SolveApp, ui: &mut egui::Ui, key: &str, value: &Value) {
    let title = app.tr(key).to_string();
    ui_kit::section(ui, key, &title, false, |ui| {
        let text = serde_json::to_string_pretty(value).unwrap_or_default();
        ui_kit::read_only_text(ui, &text);
    });
}
#[cfg(test)]
#[path = "render_workspace_output_tests.rs"]
mod tests;

pub(super) fn fit_subject(app: &mut SolveApp) -> Result<(), String> {
    // Capture the current navigation orientation before the engine's view command.
    app.run("render.camera.set", json!({"camera":app.cam}))?;
    app.run("render.camera.fit", json!({}))?;
    app.follow_view_request(false);
    app.fit_view();
    Ok(())
}

pub(super) fn restore_camera(app: &mut SolveApp) -> Result<(), String> {
    let name = app.render_workspace.view.name.clone();
    app.run("render.settings.apply", json!({"name":name}))?;
    app.follow_view_request(false);
    Ok(())
}

pub(super) fn confirmation_open(app: &SolveApp) -> bool {
    app.render_workspace.view.inspector.pending.is_some()
        || app.render_workspace.view.save_shot_name.is_some()
        || materials::confirmation_open(app)
        || library::review_open(app)
        || super::assign::replacement_open(app)
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

pub(super) fn gpu_status(app: &SolveApp, ui: &mut egui::Ui) {
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
