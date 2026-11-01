//! Integrated product preview. Shares the installed device/pipelines, not Design's scene policy.

use crate::ui_kit::BackingTooltipExt as _;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex};

use egui::Sense;
use serde_json::json;
use solidcraft_engine::doc::render_settings::RenderSetup;
#[cfg(test)]
use solidcraft_engine::render::job::LightingPreset;
use solidcraft_engine::render::job::{ImageHost, Integrator, PreviewInfo, RaytraceOptions};
use solidcraft_engine::render::{
    Availability, Camera, ExecutorDescriptor, ExecutorId, ProductScene, RenderLimits, RenderMode, RenderOperation, RenderProfile, RenderRequest,
    ScratchLayout,
};

use crate::gpu::{Frame, GpuScene, SceneSlot, ViewportCallback};
use crate::{SolveApp, theme::Tokens};

// Vertex payload only, not an OS/GPU peak-memory promise. Native viewport
// uploads are already chunked by the installed device's buffer limit.
const MAX_GEOMETRY_BYTES: usize =
    if cfg!(all(not(target_arch = "wasm32"), target_pointer_width = "64")) { 512 * 1024 * 1024 } else { 128 * 1024 * 1024 };

#[path = "render_workspace_ribbon.rs"]
mod ribbon;

#[path = "render_workspace_compare.rs"]
mod compare;

#[path = "render_workspace_cpu.rs"]
mod cpu;

#[path = "render_workspace_hdr.rs"]
mod hdr;

#[path = "render_workspace_effects.rs"]
mod effects;
#[path = "render_workspace_environment.rs"]
mod environment;
#[path = "render_workspace_inspector.rs"]
mod inspector;

#[path = "render_workspace_library.rs"]
mod library;

#[path = "render_ui/assign.rs"]
pub(crate) mod assign;
#[path = "render_workspace_light.rs"]
pub(crate) mod light;

#[path = "render_workspace_materials.rs"]
mod materials;

#[path = "render_workspace_setups.rs"]
mod setups;

#[path = "render_workspace_shot.rs"]
pub(crate) mod shot;

#[path = "render_workspace_shots.rs"]
pub(crate) mod shots;

#[path = "render_ui/scene.rs"]
pub(crate) mod scene;

#[path = "render_workspace_studio.rs"]
mod studio;
#[path = "render_workspace_transport.rs"]
mod transport;

#[path = "render_workspace_thumbnails.rs"]
mod thumbnails;
#[path = "render_ui/wear.rs"]
mod wear;

/// The browser's preview budget: WebGPU and WebGL share the tab's memory, so it stays small.
const WEB_GEOMETRY_BYTES: u64 = 128 * 1024 * 1024;
/// Desktop: at most this many of the device's largest buffers of geometry...
const DESKTOP_GEOMETRY_BUFFERS: u64 = 8;
/// ...and never more than this, whatever the device allows.
const DESKTOP_GEOMETRY_CEILING: u64 = 2 * 1024 * 1024 * 1024;

/// The product preview's geometry budget for a device whose largest buffer is `max_buffer_size`
/// bytes (`wgpu::Device::limits`, what the device was created with). Uploads are split into
/// buffers of at most that size, so the budget bounds memory, not a single buffer: 2 GiB on a
/// device with the standard 256 MiB buffers. An allocation the device still refuses is caught
/// by the upload's error scope and reported, not a crash.
pub(crate) fn geometry_budget(max_buffer_size: u64, web: bool) -> u64 {
    if web { WEB_GEOMETRY_BYTES } else { max_buffer_size.saturating_mul(DESKTOP_GEOMETRY_BUFFERS).min(DESKTOP_GEOMETRY_CEILING) }
}

/// Apply the saved compatibility preference to the device-derived budget.
fn preferred_geometry_budget(device_budget: u64, previous: bool) -> u64 {
    if previous { MAX_GEOMETRY_BYTES as u64 } else { device_budget }
}

fn admit_geometry(bytes: u64, budget: u64) -> Result<(), String> {
    if bytes > budget {
        return Err(format!(
            "Render preview needs {:.1} MiB of geometry; the limit is {:.1} MiB. Hide bodies in Design, then return to Render and try again.",
            bytes as f64 / 1048576.0,
            budget as f64 / 1048576.0
        ));
    }
    Ok(())
}

/// The message when the device refused the preview's geometry.
fn upload_refusal(bytes: u64, error: &str) -> String {
    format!(
        "The graphics device could not hold the Render preview's {:.1} MiB of geometry ({error}). Hide bodies in Design, then return to Render and try again.",
        bytes as f64 / 1048576.0
    )
}

#[cfg(test)]
#[path = "render_workspace_tests.rs"]
mod tests;

type PathFactory = Box<dyn FnOnce() -> Result<ImageHost, String> + Send>;

pub struct State {
    thumbnails: thumbnails::Cache,
    pending_setup: Option<PathFactory>,
    pub host: Option<ExecutorDescriptor>,
    pub path_host: PathHost,
    /// How long the last path tracer setup took, in seconds, per viewport adapter
    /// ([`adapter_key`]); kept in the preferences to estimate the next one.
    pub setup_seconds: std::collections::BTreeMap<String, f64>,
    /// The viewport adapter the current setup runs on.
    pub setup_key: Option<String>,
    pub geometry_budget: u64,
    /// A GPU error the preview's last geometry upload caught.
    pub upload_error: crate::gpu::UploadError,
    pub image_host: Option<solidcraft_engine::render::job::CpuHost>,
    pub view: ViewState,
    revision: u64,
    design_camera: Option<(Camera, bool)>,
    slot: SceneSlot,
    key: Option<u64>,
    scene_signature: Option<(u64, usize, u64, u64)>,
    scene: Option<ProductScene>,
    scene_error: Option<String>,
    preview_upload_bytes: Option<usize>,
    image: Option<(solidcraft_engine::render::job::JobId, Result<egui::TextureHandle, String>)>,
    image_info: Option<solidcraft_engine::render::job::FrameInfo>,
    preview_image: Option<PreviewImage>,
    clipboard_image: Option<egui::ColorImage>,
    clipboard_text: Option<String>,
    comparison_images: Vec<(solidcraft_engine::render::job::JobId, Result<egui::TextureHandle, String>)>,
}

/// Provisional pixels are never an export source and never replace the retained final image.
struct PreviewImage {
    info: PreviewInfo,
    image: Result<egui::TextureHandle, String>,
}

struct DisplayImage<'a> {
    info: &'a solidcraft_engine::render::job::FrameInfo,
    image: &'a Result<egui::TextureHandle, String>,
    preview: Option<&'a PreviewInfo>,
}

/// Application-wide setup, retained across document changes. The worker owns a
/// cloned adapter and initializes the job device; the UI receives the finished host.
#[derive(Default)]
pub enum PathHost {
    #[default]
    NotStarted,
    /// The worker's answer, and when it started: shader compilation can take minutes.
    Preparing(std::sync::mpsc::Receiver<Result<ImageHost, String>>, std::time::Instant),
    Ready(ImageHost),
    Failed(String),
}

impl PathHost {
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn start(&mut self, create: impl FnOnce() -> Result<ImageHost, String> + Send + 'static) {
        if !matches!(self, Self::NotStarted) {
            return;
        }
        let (tx, rx) = std::sync::mpsc::channel();
        // Dropping the handle detaches: neither startup nor shutdown joins a driver
        // that may spend minutes compiling or warming up its pipelines.
        *self = match std::thread::Builder::new().name("gpu-path-setup".into()).spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(create))
                .unwrap_or_else(|_| Err(crate::i18n::tr("render.error_setup_panicked").into()));
            let _ = tx.send(result);
        }) {
            Ok(_) => Self::Preparing(rx, std::time::Instant::now()),
            Err(e) => Self::failed(crate::i18n::format("render.error_setup_worker", &[("error", &e.to_string())])),
        };
    }

    fn failed(message: String) -> Self {
        log::warn!("progressive render unavailable: {message}");
        Self::Failed(message)
    }

    /// Take the worker's answer if it has come; returns how long a successful setup took.
    fn poll(&mut self) -> Option<std::time::Duration> {
        let Self::Preparing(rx, started) = self else { return None };
        let took = started.elapsed();
        *self = match rx.try_recv() {
            Ok(Ok(host)) => Self::Ready(host),
            Ok(Err(error)) => Self::failed(error),
            Err(std::sync::mpsc::TryRecvError::Empty) => return None,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => Self::failed(crate::i18n::tr("render.error_setup_stopped").into()),
        };
        matches!(self, Self::Ready(_)).then_some(took)
    }
}

/// The samples slider's top end: 2^12 = 4096 samples per pixel.
#[cfg(test)]
const MAX_SAMPLES_EXPONENT: u32 = 12;

/// Setup times kept per adapter, at most.
const MAX_SETUP_TIMES: usize = 16;

/// The key [`State::setup_seconds`] files an adapter's setup time under: a driver update
/// usually empties the driver's shader cache, so the driver is part of it.
pub fn adapter_key(info: &egui_wgpu::wgpu::AdapterInfo) -> String {
    format!("{} | {:?} | {} {}", info.name.trim(), info.backend, info.driver.trim(), info.driver_info.trim())
}

/// Remember how long the setup on `key` took, for the next start's estimate.
fn record_setup(state: &mut State, took: std::time::Duration) {
    let Some(key) = state.setup_key.clone() else { return };
    if state.setup_seconds.len() >= MAX_SETUP_TIMES && !state.setup_seconds.contains_key(&key) {
        return;
    }
    state.setup_seconds.insert(key, took.as_secs_f64());
}

/// Setup times from the preferences: finite, non-negative and at most a day each.
pub fn load_setup_seconds(value: &serde_json::Value) -> std::collections::BTreeMap<String, f64> {
    value
        .as_object()
        .into_iter()
        .flatten()
        .filter_map(|(k, v)| Some((k.clone(), v.as_f64().filter(|s| s.is_finite() && (0.0..=86_400.0).contains(s))?)))
        .take(MAX_SETUP_TIMES)
        .collect()
}

/// Minutes and seconds, as `1:05`.
fn mmss(d: std::time::Duration) -> String {
    let s = d.as_secs();
    format!("{}:{:02}", s / 60, s % 60)
}

/// The path tracer's setup as it goes: a progress bar estimated from the last setup on this
/// adapter, else a spinner with the time so far. Drivers report no progress of their own.
fn setup_progress(ui: &mut egui::Ui, app: &SolveApp, started: std::time::Instant) {
    let elapsed = started.elapsed();
    let expected = app.render_workspace.setup_key.as_ref().and_then(|k| app.render_workspace.setup_seconds.get(k)).copied();
    ui.label(app.tr("render.gpu_path_preparing"));
    match expected.map(|s| std::time::Duration::from_secs_f64(s.max(1.0))) {
        Some(expected) if elapsed < expected.mul_f64(1.5) => {
            let fraction = (elapsed.as_secs_f32() / expected.as_secs_f32()).min(0.99);
            crate::ui_kit::progress(ui, "gpu_path_setup", fraction);
            ui.label(format!("{} / ~{}", mmss(elapsed), mmss(expected)));
            // The bar moves with time, so keep drawing while it is shown.
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(250));
        }
        _ => {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(mmss(elapsed));
            });
            if expected.is_none() {
                ui.small(app.tr("render.gpu_path_first_setup"));
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Stage {
    #[default]
    Look,
    Light,
    Frame,
    Output,
}
impl Stage {
    pub(crate) const ALL: [Self; 4] = [Self::Look, Self::Light, Self::Frame, Self::Output];
    pub(crate) fn key(self) -> &'static str {
        match self {
            Self::Look => "look",
            Self::Light => "light",
            Self::Frame => "frame",
            Self::Output => "output",
        }
    }
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Look => "render.stage_look",
            Self::Light => "render.stage_light",
            Self::Frame => "render.stage_frame",
            Self::Output => "render.stage_output",
        }
    }
    fn page(self) -> inspector::Page {
        match self {
            Self::Look => inspector::Page::Appearance,
            Self::Light => inspector::Page::Lighting,
            Self::Frame => inspector::Page::Camera,
            Self::Output => inspector::Page::Output,
        }
    }
}

/// Browser focus is transient per-document view state, never part of a saved design.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum RenderSelection {
    #[default]
    Nothing,
    Object(Vec<solidcraft_engine::Sel>),
    Environment,
    Wear,
    Lighting,
    /// Method/type/index identity until named light commands provide persistent IDs.
    Light(String),
    Shot(String),
    Output,
    Sampling,
    Effects,
    Check,
}

/// Selection-driven homes introduced by E1/E7; remaining editors retain their adapters.
pub(crate) fn selection_editor(app: &mut SolveApp, selection: &RenderSelection) {
    app.render_workspace.view.inspector.page = match selection {
        RenderSelection::Nothing => inspector::Page::Scene,
        RenderSelection::Shot(_) => inspector::Page::Camera,
        RenderSelection::Output => inspector::Page::Output,
        _ => return,
    };
}

pub(crate) fn select_lighting(app: &mut SolveApp) {
    app.render_workspace.view.inspector.page = inspector::Page::Lighting;
}

/// Small per-document editor state; GPU uploads and host capabilities stay outside tabs.
#[derive(Clone, Debug)]
pub struct ViewState {
    pub(crate) assign: assign::State,
    pub(crate) frame: shot::Frame,
    lights: light::View,
    job_shot: Option<String>,
    save_shot_name: Option<String>,
    pub name: String,
    pub selection: RenderSelection,
    pub(crate) stage: Stage,
    pub(crate) by_look: bool,
    pub(crate) search_objects: bool,
    pub(crate) object_query: String,
    draft: RenderSetup,
    dirty: bool,
    clean_setup: Option<RenderSetup>,
    job: Option<solidcraft_engine::render::job::JobId>,
    completed: Option<solidcraft_engine::render::job::JobId>,
    retained: Vec<solidcraft_engine::render_jobs::RetainedResult>,
    follow_job: bool,
    compare: Option<compare::View>,
    transparent: bool,
    method: ImageMethod,
    raytrace: RaytraceOptions,
    samples: u32,
    max_depth: u32,
    seed: u32,
    studio: studio::View,
    materials: materials::View,
    library: library::View,
    inspector: inspector::View,
    environment_ring: environment::RingView,

    cpu: cpu::View,
    overwrite: bool,
    result: bool,
    result_camera: Camera,
    result_scale: ResultScale,
    result_job: Option<solidcraft_engine::render::job::JobId>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum ResultScale {
    #[default]
    Fit,
    Actual,
    Free,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum ImageMethod {
    #[default]
    Raster,
    CpuRaytrace,
    GpuRaytrace,
}

impl ImageMethod {
    fn label(self) -> &'static str {
        match self {
            Self::Raster => "render.method_raster",
            Self::CpuRaytrace => "render.method_cpu",
            Self::GpuRaytrace => "render.method_gpu",
        }
    }
}

impl Default for ViewState {
    fn default() -> Self {
        Self {
            assign: assign::State::default(),
            lights: Default::default(),
            job_shot: None,
            frame: Default::default(),
            save_shot_name: None,
            name: "Studio".into(),
            selection: RenderSelection::Nothing,
            stage: Stage::Look,
            by_look: false,
            search_objects: false,
            object_query: String::new(),
            draft: RenderSetup::default(),
            dirty: false,
            clean_setup: None,
            job: None,
            completed: None,
            retained: Vec::new(),
            follow_job: true,
            compare: None,
            transparent: false,
            method: ImageMethod::Raster,
            raytrace: RaytraceOptions::default(),
            samples: 64,
            max_depth: 8,
            seed: 0,
            studio: studio::View::default(),
            materials: materials::View::default(),
            library: library::View::default(),
            inspector: inspector::View::default(),
            environment_ring: environment::RingView::default(),

            cpu: cpu::View::default(),
            overwrite: false,
            result: false,
            result_camera: Camera { yaw: 0.0, pitch: std::f64::consts::FRAC_PI_2, ..Default::default() },
            result_scale: ResultScale::Fit,
            result_job: None,
        }
    }
}

impl Default for State {
    fn default() -> Self {
        Self {
            thumbnails: Default::default(),
            host: None,
            path_host: PathHost::default(),
            pending_setup: None,
            setup_seconds: Default::default(),
            setup_key: None,
            geometry_budget: WEB_GEOMETRY_BYTES,
            upload_error: Arc::new(Mutex::new(None)),
            image_host: None,
            view: Default::default(),
            revision: u64::MAX,
            design_camera: None,
            slot: Arc::new(Mutex::new(None)),
            key: None,
            scene_signature: None,
            scene: None,
            scene_error: None,
            preview_upload_bytes: None,
            image: None,
            image_info: None,
            preview_image: None,
            clipboard_image: None,
            clipboard_text: None,
            comparison_images: Vec::new(),
        }
    }
}

impl State {
    /// Retain the selected viewport adapter, but do not open another device or
    /// compile shaders until the GPU path image method is used in Render
    /// ([`wants_path_host`]). A later viewport installation (for example safe-mode
    /// fallback) replaces only a not-yet-started factory.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn defer_path_host(&mut self, key: String, create: impl FnOnce() -> Result<ImageHost, String> + Send + 'static) {
        if matches!(self.path_host, PathHost::NotStarted) {
            self.setup_key = Some(key);
            self.pending_setup = Some(Box::new(create));
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn start_deferred_path_host(&mut self) {
        if matches!(self.path_host, PathHost::NotStarted)
            && let Some(create) = self.pending_setup.take()
        {
            self.path_host.start(create);
        }
    }
}

pub fn active(app: &SolveApp) -> bool {
    crate::workspace::render_tab(&app.ui.tab)
}

pub(crate) fn result_active(app: &SolveApp) -> bool {
    active(app) && app.render_workspace.view.result
}

pub(crate) fn comparison_drag_captured(app: &SolveApp) -> bool {
    active(app) && app.render_workspace.view.compare.as_ref().is_some_and(|view| view.dragging)
}

pub(crate) fn fit_result(app: &mut SolveApp) {
    app.render_workspace.view.result_scale = ResultScale::Fit;
}

/// Read-only identities for automation and diagnostics. Does not prepare a
/// request, open a device, evaluate geometry, change the selected result or pump
/// a job. Clients must use render.status for the exact identity returned here.
pub(crate) fn inspect_state(app: &SolveApp) -> serde_json::Value {
    let view = &app.render_workspace.view;
    let method = match view.method {
        ImageMethod::Raster => "raster",
        ImageMethod::CpuRaytrace => "cpu_raytrace",
        ImageMethod::GpuRaytrace => "gpu_path",
    };
    json!({
        "active": active(app), "stage": view.stage.key(), "setup": view.name, "draft_dirty": view.dirty,
        "method": method, "job": view.job, "completed": view.completed,
        "results": view.retained,
        "comparison": view.compare,
        "queue": app.session.render_queue_state(),
        "showing_result": view.result, "export_format": if view.studio.export_exr {"exr"} else {"png"},
    })
}

pub fn host_executor(max_dimension: u32, depth: bool, samples: u32) -> Result<ExecutorDescriptor, String> {
    if !matches!(samples, 1 | 2 | 4 | 8 | 16) {
        return Err(crate::i18n::tr("render.error_gpu_samples").into());
    }
    Ok(ExecutorDescriptor {
        id: ExecutorId::new("gpu-viewport").map_err(|e| e.to_string())?,
        availability: if depth && max_dimension > 0 {
            Availability::Available
        } else {
            Availability::Unavailable { reason: crate::i18n::tr("render.error_preview_device").into() }
        },
        profiles: vec![RenderProfile {
            mode: RenderMode::Preview,
            operation: RenderOperation::Viewport,
            formats: Vec::new(),
            features: vec![solidcraft_engine::render::RenderFeature::Pbr],
        }],
        limits: RenderLimits { max_dimension: u64::from(max_dimension.clamp(1, 4096)), ..Default::default() },
        scratch: ScratchLayout {
            sample_scale: 1,
            sample_bytes_per_pixel: 8 * u64::from(samples),
            output_bytes_per_pixel: 8,
            encoding_bytes_per_pixel: 0,
        },
    })
}

/// Whether the GPU path tracer is wanted now: Render is open with the GPU path image method.
/// Its setup compiles a large shader that some Vulkan drivers take gigabytes and minutes over,
/// so Render with the Raster or CPU method never starts it (#9041).
#[cfg(not(target_arch = "wasm32"))]
fn wants_path_host(app: &SolveApp) -> bool {
    active(app) && app.render_workspace.view.method == ImageMethod::GpuRaytrace
}

pub fn sync_backend(app: &mut SolveApp) {
    #[cfg(not(target_arch = "wasm32"))]
    if wants_path_host(app) {
        app.render_workspace.start_deferred_path_host();
    }
    if let Some(took) = app.render_workspace.path_host.poll() {
        record_setup(&mut app.render_workspace, took);
    }
    if let PathHost::Ready(host) = &app.render_workspace.path_host
        && let Err(e) = app.session.register_render_job_host(host.clone())
    {
        app.render_workspace.path_host = PathHost::failed(e.to_string());
        app.set_status(e.to_string(), true);
    }

    if app.session.render_job_host().is_none()
        && let Some(host) = &app.render_workspace.image_host
        && let Err(e) = app.session.set_render_job_host(Some(host.clone()))
    {
        app.set_status(e.to_string(), true);
    }
    if app.session.render_viewport() == app.render_workspace.host.as_ref() {
        return;
    }
    if let Err(e) = app.session.set_render_viewport(app.render_workspace.host.clone()) {
        app.set_status(e.to_string(), true);
    }
}

/// Cooperative work belongs to the frame loop, never to a status/control request. Keep
/// scheduling frames even after leaving Render so a browser job can finish in Design.
pub fn tick(app: &mut SolveApp, ctx: &egui::Context) {
    if let Some(text) = app.render_workspace.clipboard_text.take() {
        ctx.copy_text(text);
    }
    if let Some(image) = app.render_workspace.clipboard_image.take() {
        ctx.copy_image(image);
    }
    if matches!(app.render_workspace.path_host, PathHost::Preparing(..)) {
        // Poll even in Design or an idle window, without continuously redrawing.
        ctx.request_repaint_after(std::time::Duration::from_millis(100));
    }
    if let Some(host) = &app.render_workspace.image_host {
        match host.pump(1024) {
            Ok(true) => ctx.request_repaint(),
            Ok(false) => {}
            Err(e) => app.set_status(e.to_string(), true),
        }
    }
    crate::documents::advance_render_batches(app, ctx);
    if let Err(error) = app.session.advance_render_batch() {
        app.set_status(error.to_string(), true);
    }
    follow_batch(app);
    let queue = app.session.render_queue_state();
    if queue.running != 0 || queue.waiting != 0 {
        ctx.request_repaint_after(std::time::Duration::from_millis(100));
    }
    if app.render_workspace.view.job.is_some() {
        cache_render_images(app, ctx);
    }
}

pub fn reset_document(app: &mut SolveApp) {
    crate::render_ui::reset_shell(app);
    app.ui.ribbon_context = None;
    let host = app.render_workspace.host.clone();
    let image_host = app.render_workspace.image_host.clone();
    let geometry_budget = app.render_workspace.geometry_budget;
    let path_host = std::mem::take(&mut app.render_workspace.path_host);
    let setup_seconds = std::mem::take(&mut app.render_workspace.setup_seconds);
    let setup_key = app.render_workspace.setup_key.take();
    let pending_setup = app.render_workspace.pending_setup.take();
    app.render_workspace = State { host, image_host, path_host, setup_seconds, setup_key, pending_setup, geometry_budget, ..Default::default() };
    if active(app) {
        app.ui.tab = "MODEL".into();
    }
    sync_backend(app);
}

pub(crate) fn invalidate_images(app: &mut SolveApp) {
    app.render_workspace.image = None;
    app.render_workspace.image_info = None;
    app.render_workspace.preview_image = None;
    app.render_workspace.comparison_images.clear();
}

pub fn invalidate_scene(app: &mut SolveApp) {
    app.render_workspace.thumbnails = Default::default();
    app.render_workspace.revision = u64::MAX;
    app.render_workspace.scene_signature = None;
    app.render_workspace.scene = None;
    app.render_workspace.scene_error = None;
    app.render_workspace.preview_upload_bytes = None;
    app.render_workspace.key = None;
    if let Ok(mut slot) = app.render_workspace.slot.lock() {
        *slot = None;
    }
}

pub fn enter(app: &mut SolveApp, render: bool) -> Result<(), String> {
    if render {
        if app.session.active_sketch.is_some() || app.tool.is_some() || app.dialog.is_some() || app.file_load.busy() {
            return Err(crate::i18n::tr("render.error_finish_operation").into());
        }
        app.run("render.scene", json!({}))?;
        let entering = !active(app);
        if entering {
            app.render_workspace.design_camera = Some((app.cam, app.ui.perspective));
            app.ui.tab = "RENDER".into();
            select_stage(app, app.render_workspace.view.stage.key());
            if app.render_workspace.view.selection == RenderSelection::Nothing {
                selection_editor(app, &RenderSelection::Nothing);
            }
        }
        setups::refresh_dirty(app);
        if app.render_workspace.revision != app.session.revision && !app.render_workspace.view.dirty {
            setups::load(app);
        }
        // Fit once the setup in effect is known: the fit frames that image's aspect.
        if entering {
            let scene = solidcraft_engine::product_scene::extract(&app.session).map_err(|e| e.to_string())?;
            if !scene.bounds.is_empty() {
                app.fit_bounds(scene.bounds);
            }
        }
        #[cfg(not(target_arch = "wasm32"))]
        if wants_path_host(app) {
            app.render_workspace.start_deferred_path_host();
        }
    } else {
        assign::stop(app);
        if let Some((camera, perspective)) = app.render_workspace.design_camera.take() {
            app.cam = camera;
            app.ui.up_axis = camera.up;
            app.ui.perspective = perspective;
        }
        app.ui.tab = "MODEL".into();
        crate::render_ui::reset_shell(app);
    }
    app.viewport.hover = None;
    app.cam_anim = None;
    if !render {
        app.pending_fit = None;
    }
    Ok(())
}

/// Stage navigation is transient per-document state. It never applies a resource or starts a job.
pub(crate) fn select_stage(app: &mut SolveApp, key: &str) {
    let stage = match key {
        "look" => Stage::Look,
        "light" => Stage::Light,
        "frame" => Stage::Frame,
        "output" => Stage::Output,
        _ => return,
    };
    app.render_workspace.view.stage = stage;
    app.render_workspace.view.inspector.hidden = false;
    app.render_workspace.view.inspector.page = stage.page();
    let sections: &[&str] = match stage {
        Stage::Look => &["objects"],
        Stage::Light => &["lights", "environment"],
        Stage::Frame => &["shots"],
        Stage::Output => &[],
    };
    for section in sections {
        app.tree.collapsed.remove(&format!("render:tree:{section}"));
    }
    if stage == Stage::Look {
        app.tree.collapsed.remove("c0");
        app.tree.collapsed.remove("c0/bodies");
    }
}

pub(crate) fn switch_shot(app: &mut SolveApp, name: String) {
    inspector::switch_shot(app, name);
}
pub(crate) fn stage_projection(app: &mut SolveApp, perspective: bool) -> Result<(), String> {
    let mut camera = app.cam;
    camera.fov = if perspective { 0.6 } else { 0.0 };
    inspector::apply_camera_draft(app, camera)
}

pub(crate) fn output_summary(app: &SolveApp) -> (String, &'static str) {
    let view = &app.render_workspace.view;
    (format!("{} × {}", view.draft.width, view.draft.height), if view.studio.export_exr { "EXR" } else { "PNG" })
}

pub(crate) fn output_child_summary(app: &SolveApp, kind: &str) -> String {
    let view = &app.render_workspace.view;
    match kind {
        "sampling" => format!(
            "{} · {} spp",
            app.tr("render.quality_custom"),
            if view.method == ImageMethod::GpuRaytrace {
                view.samples
            } else {
                view.raytrace.progressive.map_or(u32::from(view.raytrace.primary_samples), |p| p.samples_per_pixel)
            }
        ),
        "effects" => crate::i18n::format(
            "render.children.look_summary",
            &[
                ("tone", if view.studio.tone_map == solidcraft_engine::render::job::ToneMap::AcesFitted { "ACES" } else { "Reinhard" }),
                ("count", &view.studio.finishing.looks.len().to_string()),
            ],
        ),
        "check" => inspector::check_summary(app),
        _ => String::new(),
    }
}

pub(crate) fn job_busy(app: &SolveApp) -> bool {
    ribbon::busy(app)
}
pub(crate) fn output_stage_tools(app: &mut SolveApp, ui: &mut egui::Ui) {
    inspector::stage_tools(app, ui);
}

pub(crate) fn job_ready(app: &SolveApp, id: &str) -> Result<(), String> {
    ribbon::action_ready(app, id)
}

pub(crate) fn set_inspector_visible(app: &mut SolveApp, visible: bool) {
    app.render_workspace.view.inspector.hidden = !visible;
}

pub fn panel(app: &mut SolveApp, ui: &mut egui::Ui) {
    if crate::render_ui::focused(app) {
        return;
    }
    inspector::panel(app, ui);
}

/// Interactive stage-panel review. Direct engine JSON calls remain dialog-free.
/// S5 supplies its selected specification id without owning another consent form.
pub(crate) fn review_resource(app: &mut SolveApp, command: &str, id: &str) -> Result<(), String> {
    match command {
        "render.quality.prepare" => library::review::quality(app, id),
        _ => Err("this command has no stage-panel review adapter".into()),
    }
}

pub(crate) fn apply_browser_controls(app: &mut SolveApp, value: &serde_json::Value) {
    let view = &mut app.render_workspace.view;
    if let Some(by_look) = value.get("by_look").and_then(serde_json::Value::as_bool) {
        view.by_look = by_look;
    }
    if let Some(search) = value.get("search").and_then(serde_json::Value::as_bool) {
        view.search_objects = search;
    }
    if let Some(query) = value.get("query").and_then(serde_json::Value::as_str) {
        view.object_query = query.into();
    }
}

/// Local Browser-card content, never a new workspace or an alternative model tree.
pub(crate) fn browser_mode(app: &mut SolveApp, ui: &mut egui::Ui) -> bool {
    if !active(app) {
        return false;
    }
    if app.render_workspace.view.search_objects {
        let mut query = app.render_workspace.view.object_query.clone();
        crate::ui_kit::scope(ui, |ui| {
            if crate::ui_kit::search_field(ui, ui.id().with("render:tree:query"), &mut query, crate::i18n::tr("render.tree_search")).changed() {
                let _ = app.run("view.render_browser", json!({"query":query}));
            }
        });
    }
    false
}

/// Per-stage companion sharing the Browser's dock policy.
pub(crate) fn appearances_card(app: &mut SolveApp, ui: &mut egui::Ui, bounds: egui::Rect) {
    library::card(app, ui, bounds);
}

fn image_host(app: &SolveApp, method: ImageMethod) -> Result<&ImageHost, String> {
    if method == ImageMethod::GpuRaytrace {
        let PathHost::Ready(host) = &app.render_workspace.path_host else {
            return Err(app.tr("render.gpu_unavailable").to_string());
        };
        let descriptor = host.descriptor().map_err(|error| error.to_string())?;
        if !host.is_available()
            || !descriptor.profiles.iter().any(|profile| profile.features.contains(&solidcraft_engine::render::RenderFeature::Progressive))
        {
            return Err(app.tr("render.gpu_unavailable").to_string());
        }
        return Ok(host);
    }
    let host = app.session.render_job_host().ok_or_else(|| app.tr("render.image_host_missing").to_string())?;
    host.validate_integrator(if method == ImageMethod::CpuRaytrace { Integrator::Raytrace } else { Integrator::Raster })
        .map_err(|error| error.to_string())?;
    Ok(host)
}

/// Check and Start use exactly this request, rebuilt at each click. The result camera is
/// temporarily installed only inside shared navigation; app.cam remains the product camera.
fn image_params(app: &SolveApp) -> Result<serde_json::Value, String> {
    let view = &app.render_workspace.view;
    image_host(app, view.method)?;
    if app.session.model.is_stale() {
        return Err(app.tr("render.image_scene_stale").to_string());
    }
    image_request(app)
}

/// Pure request construction is shared with offline setup editing; host checks stay above.
fn image_request(app: &SolveApp) -> Result<serde_json::Value, String> {
    light::filter_request(app, raw_image_request(app)?)
}

fn raw_image_request(app: &SolveApp) -> Result<serde_json::Value, String> {
    cpu::validate_progressive_image(app)?;
    if !setups::has_runtime_inputs(app) {
        return Ok(json!({"settings":setups::snapshot(app)?}));
    }
    let view = &app.render_workspace.view;
    // Temporary bindings and unsaved HDR captures use the runtime API. Embedded HDRs
    // still resolve by their document resource ID when a temporary binding is present.
    if !view.draft.surfaces.is_empty() {
        return Err(app.tr("render.saved_maps_runtime_conflict").to_string());
    }
    let settings = RenderSetup { camera: app.cam, job: None, ..view.draft.clone() };
    let mut params = json!({"settings":settings,"transparent_background":view.transparent});
    match view.method {
        ImageMethod::Raster => {}
        ImageMethod::CpuRaytrace => {
            if view.transparent {
                return Err(app.tr("render.cpu_opaque_required").to_string());
            }
            cpu::params(app, &mut params)?;
        }
        ImageMethod::GpuRaytrace => {
            params["progressive"] = json!(true);
            params["samples"] = json!(view.samples);
            params["max_depth"] = json!(view.max_depth);
            params["seed"] = json!(view.seed);
            studio::params(app, &mut params)?;
        }
    }
    materials::params(app, &mut params)?;
    Ok(params)
}

fn select_image_method(app: &mut SolveApp, method: ImageMethod) {
    app.render_workspace.view.method = method;
    // Raster has no exposure control. Selecting it explicitly selects its zero-EV response.
    if method == ImageMethod::Raster {
        app.render_workspace.view.draft.exposure_ev = 0.0;
    }
    app.render_workspace.view.dirty = true;
}

#[cfg(test)]
fn method_selector(app: &mut SolveApp, ui: &mut egui::Ui) {
    ui.label(app.tr("render.image_method"));
    method_dropdown(app, ui);
    if image_host(app, app.render_workspace.view.method).is_err() {
        crate::ui_kit::text(ui, app.tr("render.setup_offline_help"), crate::ui_kit::Text::Note("render.setup_offline_help"));
    }
    if app.render_workspace.view.method != ImageMethod::Raster {
        crate::ui_kit::text(ui, app.tr("render.preview_image_settings_help"), crate::ui_kit::Text::Note("render.preview_image_settings_help"));
    }
}

/// The same method authority in an aligned editor row or the legacy master form.
fn method_dropdown(app: &mut SolveApp, ui: &mut egui::Ui) -> egui::Response {
    let current = app.render_workspace.view.method;
    let label = app.tr(current.label()).to_string();
    let response = crate::ui_kit::dropdown_menu(ui, "render_image_method", &label, |ui| {
        for (method, handle) in [
            (ImageMethod::Raster, "render:method-raster"),
            (ImageMethod::CpuRaytrace, "render:method-cpu"),
            (ImageMethod::GpuRaytrace, "render:method-gpu"),
        ] {
            let capability = image_host(app, method).map(|_| ());
            let label = app.tr(method.label()).to_string();
            let response = crate::ui_kit::list_picker(ui, handle, &label, method == current);
            crate::scenario::publish_handle(handle, response.rect.center());
            if response.clicked() {
                select_image_method(app, method);
                ui.close();
            }
            if let Err(reason) = capability {
                response.backing_on_hover_text(reason);
            }
        }
    });
    crate::scenario::publish_handle("render:method", response.rect.center());
    response
}

fn cpu_sampling_controls(app: &mut SolveApp, ui: &mut egui::Ui) {
    let progressive = cpu::progressive_controls(app, ui);
    if progressive {
        crate::ui_kit::text(ui, app.tr("render.cpu_progressive_help"), crate::ui_kit::Text::Note("render.cpu_progressive_help"));
    }
    ui.add_enabled_ui(!progressive, |ui| {
        crate::ui_kit::text(ui, app.tr("render.children.edge_samples"), crate::ui_kit::Text::Inline);
        let samples = app.render_workspace.view.raytrace.primary_samples;
        let aa = crate::ui_kit::dropdown_menu(ui, "render_cpu_samples", &format!("{samples}×"), |ui| {
            for count in [1, 4, 16] {
                let handle = format!("render:aa-{count}");
                let response = crate::ui_kit::list_picker(ui, &handle, &format!("{count}×"), count == samples);
                crate::scenario::publish_handle(&handle, response.rect.center());
                if response.clicked() {
                    app.render_workspace.view.raytrace.primary_samples = count;
                    ui.close();
                }
            }
        });
        crate::scenario::publish_handle("render:aa", aa.rect.center());
        ui.small(app.tr("render.cpu_aa_help"));
    });
}

#[cfg(test)]
fn cpu_lighting_controls(app: &mut SolveApp, ui: &mut egui::Ui) {
    ui.label(app.tr("render.cpu_lighting"));
    let presets = [
        (LightingPreset::Neutral, "render.cpu_neutral"),
        (LightingPreset::WarmCool, "render.cpu_warm_cool"),
        (LightingPreset::KeyRim, "render.cpu_key_rim"),
    ];
    let selected = app.render_workspace.view.raytrace.preset.unwrap_or(LightingPreset::Neutral);
    let label = app
        .tr(match selected {
            LightingPreset::Neutral => "render.cpu_neutral",
            LightingPreset::WarmCool => "render.cpu_warm_cool",
            LightingPreset::KeyRim => "render.cpu_key_rim",
        })
        .to_string();
    crate::ui_kit::dropdown_menu(ui, "render_cpu_lighting", &label, |ui| {
        for (preset, key) in presets {
            if crate::ui_kit::list_picker(ui, key, app.tr(key), preset == selected).clicked() {
                app.render_workspace.view.raytrace.preset = Some(preset);
                app.render_workspace.view.raytrace.lights = None;
                ui.close();
            }
        }
    });
    let shadows = app.tr("render.cpu_shadows").to_string();
    crate::ui_kit::checkbox(ui, &shadows, &mut app.render_workspace.view.raytrace.shadows);
}

#[cfg(test)]
fn image_method_controls(app: &mut SolveApp, ui: &mut egui::Ui) {
    method_selector(app, ui);
    if app.render_workspace.view.method != ImageMethod::CpuRaytrace {
        return;
    }
    cpu_sampling_controls(app, ui);
    cpu_lighting_controls(app, ui);
    ui.label(app.tr("render.image_exposure"));
    let exposure_help = app.tr("render.cpu_exposure_help").to_string();
    crate::ui_kit::number_field(ui, &mut app.render_workspace.view.draft.exposure_ev, -32.0..=32.0, 0.1, " EV").backing_on_hover_text(exposure_help);
    cpu::controls(app, ui);
}

#[cfg(test)]
fn image_edit_controls(app: &mut SolveApp, ui: &mut egui::Ui) {
    ui.heading(app.tr("render.image_title"));
    let preview = app.tr("render.result_preview").to_string();
    let result = app.tr("render.result_title").to_string();
    let mut selected = usize::from(app.render_workspace.view.result);
    if crate::ui_kit::segmented(ui, &mut selected, &[&preview, &result], crate::ui_kit::Segments { compact: true, ..Default::default() }) {
        let _ = app.run("render.result.view", json!({"view":if selected == 1 {"image"} else {"model"}}));
    }
    if app.render_workspace.view.result {
        ui.horizontal(|ui| {
            if crate::ui_kit::button(ui, app.tr("render.result_fit"), crate::ui_kit::Button::Normal).clicked() {
                let _ = app.run("render.result.view", json!({"view":"fit"}));
            }
            if crate::ui_kit::button(ui, crate::i18n::tr("render.result_actual_size"), crate::ui_kit::Button::Enabled(!reduced_display(app)))
                .backing_on_hover_text(app.tr("render.result_actual_help"))
                .clicked()
            {
                let _ = app.run("render.result.view", json!({"view":"actual"}));
            }
        });
        ui.small(app.tr("render.result_navigation"));
    }
    image_method_controls(app, ui);
    ui.small(app.tr(match app.render_workspace.view.method {
        ImageMethod::Raster => "render.opaque_image_summary",
        ImageMethod::CpuRaytrace => "render.cpu_summary",
        ImageMethod::GpuRaytrace => "render.progressive_image_summary",
    }));
    // The field's hard range never silently clamps a draft when its budget changes.
    let side = match app.render_workspace.view.method {
        ImageMethod::CpuRaytrace => 16384,
        ImageMethod::GpuRaytrace => 8192,
        ImageMethod::Raster => 4096,
    };
    ui.horizontal(|ui| {
        ui.label(app.tr("render.image_width"));
        if crate::ui_kit::number_field(ui, &mut app.render_workspace.view.draft.width, 1..=side, 1.0, "").changed() {
            app.render_workspace.view.dirty = true;
        }
        ui.label(app.tr("render.image_height"));
        if crate::ui_kit::number_field(ui, &mut app.render_workspace.view.draft.height, 1..=side, 1.0, "").changed() {
            app.render_workspace.view.dirty = true;
        }
    });
    let draft = &app.render_workspace.view.draft;
    let size = (u64::from(draft.width), u64::from(draft.height));
    if let Some((w, h)) = cpu::image_limits(app).fit(size.0, size.1, None)
        && (w, h) != size
    {
        let limit = if app.render_workspace.view.method == ImageMethod::CpuRaytrace && app.render_workspace.view.raytrace.progressive.is_some() {
            "render.cpu_budget_resize"
        } else {
            "render.image_too_large"
        };
        ui.small(app.tr(limit));
        if crate::ui_kit::button(ui, format!("{} {w} × {h}", app.tr("render.image_use_largest")), crate::ui_kit::Button::Normal).clicked() {
            app.render_workspace.view.draft.width = u32::try_from(w).unwrap_or(1);
            app.render_workspace.view.draft.height = u32::try_from(h).unwrap_or(1);
            app.render_workspace.view.dirty = true;
        }
    }
    match &app.render_workspace.path_host {
        PathHost::Preparing(_, started) => setup_progress(ui, app, *started),
        PathHost::Failed(error) => {
            ui.add(egui::Label::new(format!("{}: {error}", app.tr("render.gpu_path_failed"))).wrap());
        }
        _ => {}
    }
    if app.render_workspace.view.method == ImageMethod::GpuRaytrace {
        ui.label(app.tr("render.image_samples"));
        // Powers of two from 1 to 4096; requests from elsewhere keep any count.
        let samples = &mut app.render_workspace.view.samples;
        let mut exponent = (*samples).max(1).ilog2().min(MAX_SAMPLES_EXPONENT);
        let slider = crate::ui_kit::Slider { integer: true, format: |e| format!("{}", 1u32 << (e as u32).min(31)), ..Default::default() };
        if crate::ui_kit::slider(ui, &mut exponent, 0..=MAX_SAMPLES_EXPONENT, slider).changed() {
            *samples = 1 << exponent.min(MAX_SAMPLES_EXPONENT);
        }
        let title = app.tr("render.image_advanced").to_string();
        crate::ui_kit::section(ui, "render_gpu_advanced", &title, false, |ui| {
            ui.label(app.tr("render.gpu_depth"));
            crate::ui_kit::number_field(ui, &mut app.render_workspace.view.max_depth, 1..=16, 1.0, "");
            ui.label(app.tr("render.gpu_seed"));
            crate::ui_kit::number_field(ui, &mut app.render_workspace.view.seed, 0..=u32::MAX, 1.0, "");
            ui.label(app.tr("render.image_exposure"));
            crate::ui_kit::number_field(ui, &mut app.render_workspace.view.draft.exposure_ev, -32.0..=32.0, 0.1, " EV");
            ui.small(app.tr("render.gpu_options_help"));
            studio::controls(app, ui);
            studio::backdrop_controls(app, ui);
        });
    }
    library::controls(app, ui);
    materials::controls(app, ui);
    let transparent_label = app.tr("render.image_transparent").to_string();
    crate::ui_kit::checkbox(ui, &transparent_label, &mut app.render_workspace.view.transparent);
}

/// Dense all-controls composition, also useful to standalone/offscreen hosts.
#[cfg(test)]
fn image_controls(app: &mut SolveApp, ui: &mut egui::Ui) {
    image_edit_controls(app, ui);
    job_controls(app, ui, false);
    completed_image(app, ui);
}

fn job_controls(app: &mut SolveApp, ui: &mut egui::Ui, compact: bool) {
    let params = image_params(app);
    if !compact {
        if let Err(error) = &params {
            ui.add(egui::Label::new(egui::RichText::new(error).color(Tokens::get().warning)).wrap());
        }
        cpu::checked_status(app, ui, params.as_ref().ok());
    }
    let mut busy = false;
    let mut state = None;
    if let Some(job) = app.render_workspace.view.job {
        match app.session.execute("render.status", &json!({"job":job})) {
            Ok(status) => {
                let phase = status["progress"]["phase"].as_str().unwrap_or(crate::i18n::tr("render.property_unknown"));
                busy = matches!(phase, "queued" | "preparing" | "running" | "encoding" | "cancelling");
                state = Some(status);
            }
            Err(error) => {
                if !compact {
                    ui.add(egui::Label::new(egui::RichText::new(error.to_string()).small()).wrap());
                }
            }
        }
    }
    ui.horizontal_wrapped(|ui| {
        let check = crate::ui_kit::button(
            ui,
            app.tr("cmd.render_preflight"),
            crate::ui_kit::Button::Enabled(ribbon::action_ready(app, "render.preflight").is_ok()),
        );
        crate::scenario::publish_target("render:check", check.rect);
        if check.clicked()
            && let Err(error) = ribbon::perform_job(app, "render.preflight")
        {
            app.set_status(error, true);
        }
        let start = crate::ui_kit::button(
            ui,
            app.tr("render.image_start"),
            crate::ui_kit::Button::Enabled(ribbon::action_ready(app, "render.start").is_ok()),
        );
        crate::scenario::publish_target("render:start", start.rect);
        if start.clicked() {
            if let Err(error) = ribbon::perform_job(app, "render.start") {
                app.set_status(error, true);
            }
            ui.ctx().request_repaint();
        }
    });
    let message = state.as_ref().map_or_else(
        || app.tr(if params.is_ok() { "render.studio_ready" } else { "render.studio_needs_review" }).to_string(),
        |status| {
            app.tr(match status["progress"]["phase"].as_str().unwrap_or("") {
                "queued" => "render.image_queued",
                "preparing" => "render.image_preparing",
                "running" => "render.image_running",
                "encoding" => "render.image_encoding",
                "cancelling" => "render.image_cancelling",
                "cancelled" => "render.image_cancelled",
                "completed" => "render.image_completed",
                "failed" => "render.image_failed",
                _ => "render.image_unknown",
            })
            .to_string()
        },
    );
    let response = ui.add_sized([ui.available_width(), ui.text_style_height(&egui::TextStyle::Body)], egui::Label::new(message).truncate());
    crate::scenario::publish_target("render:job-summary", response.rect);
    if let Err(error) = &params {
        response.backing_on_hover_text(error);
    }
    if let Some(status) = &state {
        if !compact {
            cpu::captured_status(app, ui, status);
            if let Some(message) = status["error"]["message"].as_str() {
                ui.add(egui::Label::new(egui::RichText::new(message).small()).wrap());
            }
        }
        if busy {
            let done = status["progress"]["done"].as_u64().unwrap_or(0);
            let total = status["progress"]["total"].as_u64().unwrap_or(1).max(1);
            crate::ui_kit::progress(ui, "render", (done as f64 / total as f64).clamp(0.0, 1.0) as f32);
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
        }
    }
    ui.horizontal_wrapped(|ui| {
        let cancel = crate::ui_kit::button(
            ui,
            app.tr("cmd.render_cancel"),
            crate::ui_kit::Button::Enabled(ribbon::action_ready(app, "render.cancel").is_ok()),
        );
        crate::scenario::publish_target("render:cancel", cancel.rect);
        if cancel.clicked()
            && let Err(error) = ribbon::perform_job(app, "render.cancel")
        {
            app.set_status(error, true);
        }
        let export = crate::ui_kit::button(
            ui,
            app.tr("render.image_export"),
            crate::ui_kit::Button::Enabled(ribbon::action_ready(app, "render.export").is_ok()),
        );
        crate::scenario::publish_target("render:export", export.rect);
        if export.clicked()
            && let Err(error) = ribbon::perform_job(app, "render.export")
        {
            app.set_status(error, true);
        }
    });
    if !compact {
        let label = app.tr("render.image_overwrite").to_string();
        crate::ui_kit::checkbox(ui, &label, &mut app.render_workspace.view.overwrite);
    }
}

/// Decode each published sequence once. Provisional textures stay separate from the last
/// final image, so cancellation or a failed pass cannot change the export target.
#[cfg(test)]
fn completed_image(app: &mut SolveApp, ui: &mut egui::Ui) {
    cache_render_images(app, ui.ctx());
    if let Some(display) = displayed_image(app) {
        if display.preview.is_some() {
            ui.small(app.tr("render.result_provisional"));
        }
        match display.image {
            Ok(texture) => {
                if ui
                    .add(egui::Image::new(texture).max_size(egui::vec2(ui.available_width(), 240.0)).shrink_to_fit().sense(Sense::click()))
                    .backing_on_hover_text(app.tr("render.result_open"))
                    .clicked()
                {
                    let _ = app.run("render.result.view", json!({"view":"image"}));
                }
            }
            Err(error) => {
                ui.add(egui::Label::new(error).wrap());
            }
        }
    } else if let Some((_, Err(error))) = &app.render_workspace.image {
        ui.add(egui::Label::new(error).wrap());
    }
}

fn cache_completed_image(app: &mut SolveApp, ctx: &egui::Context) {
    let Some(job) = app.render_workspace.view.completed else {
        app.render_workspace.image = None;
        app.render_workspace.image_info = None;
        return;
    };
    if app.render_workspace.image.as_ref().is_none_or(|(old, _)| *old != job) {
        app.render_workspace.image_info = None;
        let result = (|| -> Result<egui::TextureHandle, String> {
            let handle = app.session.render_job_handle(job).map_err(|e| e.to_string())?;
            let frame = handle.frame().map_err(|e| e.to_string())?.ok_or(crate::i18n::tr("render.error_no_completed_image"))?;
            if let Some((_, cached)) = app.render_workspace.comparison_images.iter().find(|(id, _)| *id == job) {
                let texture = cached.clone()?;
                app.render_workspace.image_info = Some(frame.info.clone());
                return Ok(texture);
            }
            let captured = handle.status().map_err(|e| e.to_string())?;
            let budget = if frame.info.executor.as_str() == "gpu-path" {
                solidcraft_engine::render::job::GPU_PATH_LIMITS
            } else {
                captured_image_limits(&captured.request)
            };
            let image = decode_render_image(frame.png(), frame.info.width, frame.info.height, budget, ctx.input(|i| i.max_texture_side))?;
            app.render_workspace.image_info = Some(frame.info.clone());
            Ok(ctx.load_texture("sc_render_image", image, egui::TextureOptions::LINEAR))
        })();
        // Cache errors too, so a bad frame cannot repeatedly allocate or decode on repaint.
        app.render_workspace.image = Some((job, result));
    }
}

fn displayed_image(app: &SolveApp) -> Option<DisplayImage<'_>> {
    let state = &app.render_workspace;
    if let Some(preview) = &state.preview_image
        && state.view.job == Some(preview.info.frame.job)
    {
        return Some(DisplayImage { info: &preview.info.frame, image: &preview.image, preview: Some(&preview.info) });
    }
    Some(DisplayImage { info: state.image_info.as_ref()?, image: &state.image.as_ref()?.1, preview: None })
}

fn cache_preview_frame(app: &mut SolveApp, ctx: &egui::Context, info: &PreviewInfo, png: &[u8]) {
    if app.render_workspace.view.job != Some(info.frame.job) {
        return;
    }
    if app
        .render_workspace
        .preview_image
        .as_ref()
        .is_some_and(|old| old.info.frame.job == info.frame.job && old.info.frame_sequence >= info.frame_sequence)
    {
        return;
    }
    let result = (|| -> Result<egui::TextureHandle, String> {
        if info.frame_sequence == 0 || !(1..=4096).contains(&info.samples_per_pixel) {
            return Err(crate::i18n::tr("render.error_provisional_identity").into());
        }
        let handle = app.session.render_job_handle(info.frame.job).map_err(|e| e.to_string())?;
        let captured = handle.status().map_err(|e| e.to_string())?;
        if (info.frame.width, info.frame.height) != (captured.request.width, captured.request.height) {
            return Err(crate::i18n::tr("render.error_checkpoint_dimensions").into());
        }
        let options = solidcraft_engine::render::job::CpuProgressiveOptions { samples_per_pixel: info.samples_per_pixel, seed: 0 };
        options.validate_image_with_budget(info.frame.width, info.frame.height, captured.request.cpu_budget()).map_err(|error| error.to_string())?;
        let image = decode_render_image(
            png,
            info.frame.width,
            info.frame.height,
            captured_image_limits(&captured.request),
            ctx.input(|i| i.max_texture_side),
        )?;
        Ok(ctx.load_texture("sc_render_preview", image, egui::TextureOptions::LINEAR))
    })();
    // Cache errors by identity too: a malformed publication cannot decode on every repaint.
    app.render_workspace.preview_image = Some(PreviewImage { info: info.clone(), image: result });
}

fn cache_preview_image(app: &mut SolveApp, ctx: &egui::Context) {
    if app.render_workspace.view.follow_job
        && let Some(job) = app.render_workspace.view.job
        && let Ok(handle) = app.session.render_job_handle(job)
    {
        let preview = handle.preview().ok().flatten();
        // Read the final after the checkpoint: completion may occur after the panel's
        // earlier status query. Promote the real final before dropping its preview,
        // rather than briefly falling back to an older job and losing result framing.
        if handle.frame().ok().flatten().is_none()
            && let Some(preview) = preview
        {
            cache_preview_frame(app, ctx, &preview.info, preview.png());
            return;
        }
    }
    app.render_workspace.preview_image = None;
}

fn remember_displayed_job(app: &mut SolveApp, preserve_view: bool) {
    let Some(job) = displayed_image(app).map(|display| display.info.job) else { return };
    if app.render_workspace.view.result_job != Some(job) {
        app.render_workspace.view.result_job = Some(job);
        if !preserve_view {
            app.render_workspace.view.result_scale = ResultScale::Fit;
        }
    }
}

fn cache_render_images(app: &mut SolveApp, ctx: &egui::Context) {
    let preserve_view = app.render_workspace.preview_image.as_ref().is_some_and(|preview| {
        app.render_workspace.view.result_job == Some(preview.info.frame.job) && app.render_workspace.view.job == Some(preview.info.frame.job)
    });
    cache_preview_image(app, ctx);
    sync_results(app);
    cache_completed_image(app, ctx);
    // Checkpoint updates and preview→final/cancel transitions preserve shared pan/zoom.
    remember_displayed_job(app, preserve_view);
}

fn sync_results(app: &mut SolveApp) {
    let Ok(mut results) = app.session.render_retained_results() else { return };
    let view = &mut app.render_workspace.view;
    for result in &mut results {
        if result.shot.is_none() {
            result.shot = view.retained.iter().find(|old| old.frame.job == result.frame.job).and_then(|old| old.shot.clone());
            if result.shot.is_none() && view.job == Some(result.frame.job) {
                result.shot = view.job_shot.clone();
            }
        }
    }
    view.retained = results;
    view.completed = app.session.render_selected_result();
    if view.compare.as_ref().is_some_and(|comparison| {
        [comparison.left, comparison.right].into_iter().flatten().any(|job| !view.retained.iter().any(|result| result.frame.job == job))
    }) {
        view.compare = None;
        app.render_workspace.comparison_images.clear();
    }
}

fn follow_batch(app: &mut SolveApp) {
    let queue = app.session.render_queue_state();
    if let Some(job) = queue.job {
        if app.render_workspace.view.job != Some(job) {
            app.render_workspace.view.follow_job = app.render_workspace.view.compare.is_none();
        }
        app.render_workspace.view.job = Some(job);
        app.render_workspace.view.job_shot = queue.shot;
    }
}

fn captured_image_limits(request: &solidcraft_engine::render::job::ImageRequest) -> RenderLimits {
    if request.integrator == Integrator::Raytrace {
        request.cpu_budget().image_limits(request.raytrace.as_ref().is_some_and(|o| o.progressive.is_some()))
    } else {
        RenderLimits::default()
    }
}

/// This is a display cache only: the authoritative PNG/frame and export remain
/// full resolution. Avoid asking a viewport device for an unsupported texture.
fn display_dimensions(width: u32, height: u32, max_texture_side: usize) -> Result<(u32, u32), String> {
    if width == 0 || height == 0 || max_texture_side == 0 {
        return Err(crate::i18n::tr("render.error_display_dimensions").into());
    }
    let max_side = max_texture_side.min(8192) as u64;
    let display = RenderLimits { max_dimension: max_side, max_pixels: 16_777_216, max_scratch_bytes: 512 * 1024 * 1024 };
    let (w, h) = display.fit(u64::from(width), u64::from(height), None).ok_or(crate::i18n::tr("render.error_display_fit"))?;
    Ok((
        u32::try_from(w).map_err(|_| crate::i18n::tr("render.error_display_width"))?,
        u32::try_from(h).map_err(|_| crate::i18n::tr("render.error_display_height"))?,
    ))
}

#[cfg(test)]
fn decode_image(png: &[u8], width: u32, height: u32) -> Result<egui::ColorImage, String> {
    decode_render_image(png, width, height, RenderLimits::default(), 8192)
}

fn decode_render_image(png: &[u8], width: u32, height: u32, budget: RenderLimits, max_texture_side: usize) -> Result<egui::ColorImage, String> {
    let size = (u64::from(width), u64::from(height));
    if budget.fit(size.0, size.1, None) != Some(size) {
        return Err(crate::i18n::tr("render.error_image_profile").into());
    }
    let display_size = display_dimensions(width, height, max_texture_side)?;
    let mut reader = image::ImageReader::with_format(std::io::Cursor::new(png), image::ImageFormat::Png);
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(width);
    limits.max_image_height = Some(height);
    limits.max_alloc = Some(size.0.checked_mul(size.1).and_then(|n| n.checked_mul(16)).ok_or(crate::i18n::tr("render.error_decode_budget"))?);
    reader.limits(limits);
    let image = reader.decode().map_err(|e| e.to_string())?.to_rgba8();
    if image.dimensions() != (width, height) {
        return Err(crate::i18n::tr("render.error_image_dimensions").into());
    }
    let image = if display_size != (width, height) {
        image::imageops::resize(&image, display_size.0, display_size.1, image::imageops::FilterType::Triangle)
    } else {
        image
    };
    Ok(egui::ColorImage::from_rgba_unmultiplied([display_size.0 as usize, display_size.1 as usize], image.as_raw()))
}

fn reduced_display(app: &SolveApp) -> bool {
    displayed_image(app).is_some_and(|display| {
        display.image.as_ref().is_ok_and(|texture| texture.size() != [display.info.width as usize, display.info.height as usize])
    })
}

/// The image has its own per-document orthographic camera. Shared navigation can temporarily
/// drive it without changing the product camera, saved setup, or a pending Design view.
pub(crate) fn with_result_camera(app: &mut SolveApp, navigate: impl FnOnce(&mut SolveApp)) {
    let preview = std::mem::replace(&mut app.cam, app.render_workspace.view.result_camera);
    let before = app.cam;
    let draft = app.render_workspace.view.draft.clone();
    let dirty = app.render_workspace.view.dirty;
    let perspective = std::mem::replace(&mut app.ui.perspective, false);
    let up = app.ui.up_axis;
    let animation = app.cam_anim.take();
    let pending = app.pending_fit.take();
    let request = app.session.camera.take();
    let free = app.viewport.orbit_free.take();
    let spin = std::mem::replace(&mut app.viewport.spin, false);
    navigate(app);
    app.render_workspace.view.result_camera = std::mem::replace(&mut app.cam, preview);
    if app.render_workspace.view.result_camera != before {
        app.render_workspace.view.result_scale = ResultScale::Free;
    }
    app.render_workspace.view.draft = draft;
    app.render_workspace.view.dirty = dirty;
    app.ui.perspective = perspective;
    app.ui.up_axis = up;
    app.cam_anim = animation;
    app.pending_fit = pending;
    app.session.camera = request;
    app.viewport.orbit_free = free;
    app.viewport.spin = spin;
}

fn result_camera(size: [u32; 2], rect: egui::Rect, pixels_per_point: f32, scale: ResultScale) -> Camera {
    let width = f64::from(size[0].max(1));
    let height = f64::from(size[1].max(1));
    let scale = if scale == ResultScale::Actual {
        1.0 / f64::from(pixels_per_point.max(0.1))
    } else {
        (f64::from((rect.width() - 32.0).max(1.0)) / width).min(f64::from((rect.height() - 32.0).max(1.0)) / height)
    };
    Camera {
        yaw: 0.0,
        pitch: std::f64::consts::FRAC_PI_2,
        distance: (f64::from(rect.height().max(1.0)) / (0.8 * scale)).clamp(1e-3, 1e7),
        ..Default::default()
    }
}

fn result_rect(camera: Camera, size: [u32; 2], canvas: egui::Rect) -> egui::Rect {
    let scale = f64::from(canvas.height().max(1.0)) / (2.0 * camera.half_height());
    let center = canvas.center() + egui::vec2((-camera.target.x * scale) as f32, (camera.target.y * scale) as f32);
    egui::Rect::from_center_size(center, egui::vec2((f64::from(size[0]) * scale) as f32, (f64::from(size[1]) * scale) as f32))
}

fn result_actions(app: &mut SolveApp, ui: &mut egui::Ui) {
    compare::controls(app, ui);
    let results = app.render_workspace.view.retained.clone();
    if !results.is_empty() {
        let selected = results.iter().position(|result| Some(result.frame.job) == app.render_workspace.view.completed);
        let label = selected.and_then(|index| results.get(index)).map(result_label).unwrap_or_else(|| app.tr("render.results_choose").to_string());
        let mut chosen = None;
        let picker = crate::ui_kit::compact_dropdown_menu(ui, "render_results", &label, |ui| {
            for (index, result) in results.iter().enumerate().rev() {
                let response = crate::ui_kit::list_picker(ui, &format!("result:{index}"), &result_label(result), selected == Some(index));
                crate::scenario::publish_target(&format!("render:result:select:{index}"), response.rect);
                if response.clicked() {
                    chosen = Some(index);
                    ui.close();
                }
            }
        });
        crate::scenario::publish_target("render:results", picker.rect);
        if let Some(index) = chosen {
            let _ = app.run("render.result.select", json!({"index":index}));
        }
        let discard =
            ui.add_enabled_ui(selected.is_some(), |ui| crate::ui_kit::StageBar::icon_action(ui, "delete", app.tr("cmd.render.result.discard"))).inner;
        crate::scenario::publish_target("render:result-discard", discard.rect);
        if discard.clicked()
            && let Some(index) = selected
        {
            let _ = app.run("render.result.discard", json!({"index":index}));
        }
    }
    for (view, key) in [("model", "render.state_model"), ("fit", "render.output_fit"), ("actual", "render.result_actual")] {
        let enabled = view != "actual" || !reduced_display(app);
        if crate::ui_kit::button(ui, app.tr(key), crate::ui_kit::Button::Enabled(enabled)).clicked() {
            let _ = app.run("render.result.view", json!({"view":view}));
        }
    }
    let copy = crate::ui_kit::button(
        ui,
        app.tr("render.output_copy"),
        crate::ui_kit::Button::Enabled(ribbon::action_ready(app, "render.result.copy").is_ok()),
    );
    crate::scenario::publish_target("render:result-copy", copy.rect);
    if copy.clicked() {
        app.start("render.result.copy");
    }
    let export =
        crate::ui_kit::button(ui, studio::export_label(app), crate::ui_kit::Button::Enabled(ribbon::action_ready(app, "render.export").is_ok()));
    crate::scenario::publish_target("render:result-export", export.rect);
    if export.clicked() {
        app.start("render.export");
    }
    let close = crate::ui_kit::StageBar::icon_action(ui, "close", app.tr("cmd.render_result_close"));
    crate::scenario::publish_target("render:result-close", close.rect);
    if close.clicked() {
        app.start("render.result.close");
    }
}

fn result_label(result: &solidcraft_engine::render_jobs::RetainedResult) -> String {
    let name = result.shot.as_deref().unwrap_or_else(|| crate::i18n::tr("render.results_unnamed"));
    let label = crate::i18n::format(
        "render.results_label",
        &[("number", &result.frame.job.serial.to_string()), ("shot", name), ("seconds", &format!("{:.0}", result.seconds))],
    );
    if result.stale { crate::i18n::format("render.results_stale_label", &[("result", &label)]) } else { label }
}

fn show_result(app: &mut SolveApp, ui: &mut egui::Ui) {
    cache_render_images(app, ui.ctx());
    let t = Tokens::get();
    ui.painter().rect_filled(ui.max_rect(), 0.0, t.viewport_bottom);
    // The floating shell paints after this canvas. Keep image actions in its free
    // content area so neither ribbon nor Browser can cover their hit targets.
    let mut free = crate::render_ui::canvas(app, ui.max_rect());
    if ribbon::busy(app) {
        free = crate::ui_kit::Container::below_navigation_row(free);
    }
    let mut content = ui.new_child(egui::UiBuilder::new().max_rect(free).id_salt("render_result_content"));
    let ui = &mut content;
    let Some((info, preview)) = displayed_image(app).map(|display| (display.info.clone(), display.preview.cloned())) else {
        let message = match &app.render_workspace.image {
            Some((_, Err(error))) => error.clone(),
            _ if ribbon::busy(app) => app.tr("render.state_in_progress").to_string(),
            _ => app.tr("render.result_empty").to_string(),
        };
        crate::ui_kit::Container::navigation_contents(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                crate::ui_kit::text(
                    ui,
                    app.tr(if ribbon::busy(app) { "render.state_in_progress" } else { "render.result_title" }),
                    crate::ui_kit::Text::Inline,
                );
                result_actions(app, ui);
            });
        });
        let canvas = ui.available_rect_before_wrap();
        ui.centered_and_justified(|ui| {
            ui.add(egui::Label::new(egui::RichText::new(message).size(t.font_heading).color(t.text)).wrap());
        });
        app.viewport.rect = Some(canvas);
        return;
    };
    crate::ui_kit::Container::navigation_contents(ui, |ui| {
        ui.horizontal_wrapped(|ui| {
            crate::ui_kit::text(
                ui,
                app.tr(if preview.is_some() { "render.state_in_progress" } else { "render.state_completed" }),
                crate::ui_kit::Text::Inline,
            );
            result_actions(app, ui);
        });
        if let Some(preview) = &preview {
            ui.colored_label(
                t.warning,
                format!(
                    "{} · {} {} · {} {}",
                    app.tr("render.result_provisional"),
                    preview.samples_per_pixel,
                    app.tr("render.cpu_progressive_spp"),
                    app.tr("render.result_sequence"),
                    preview.frame_sequence
                ),
            );
            ui.small(app.tr("render.result_provisional_help"));
        }
        if let Some(display) = displayed_image(app)
            && let Err(error) = display.image
        {
            ui.colored_label(t.warning, error);
        }
        ui.horizontal_wrapped(|ui| {
            ui.strong(format!("{} × {} px · PNG · {}", info.width, info.height, info.executor.as_str()));
            ui.label(format!(
                "{} {}:{}:{} · {} {}",
                app.tr("render.result_job"),
                info.job.session,
                info.job.serial,
                info.job.generation,
                app.tr("render.result_capture"),
                info.scene_generation
            ));
        });
        if reduced_display(app) {
            ui.colored_label(t.warning, app.tr("render.result_reduced_display"));
        }
        let stale = info.scene_generation != app.session.revision;
        ui.colored_label(if stale { t.warning } else { t.text_dim }, app.tr(if stale { "render.image_stale" } else { "render.result_current" }));
        if app.render_workspace.view.job.is_some_and(|job| job != info.job) {
            ui.small(app.tr("render.result_retained"));
        }
    });
    let rect = ui.available_rect_before_wrap();
    if rect.width() < 1.0 || rect.height() < 1.0 {
        return;
    }
    let response = ui.allocate_rect(rect, Sense::click_and_drag());
    app.viewport.rect = Some(rect);
    crate::viewport::refresh_layout(app, ui.ctx());
    let size = [info.width, info.height];
    let scale = app.render_workspace.view.result_scale;
    if scale != ResultScale::Free {
        app.render_workspace.view.result_camera = result_camera(size, rect, ui.ctx().pixels_per_point(), scale);
    }
    with_result_camera(app, |app| {
        crate::navigation::handle(app, ui, rect, &response, crate::navigation::NavCaps { orbit: false, pick: false });
    });
    let after = app.render_workspace.view.result_scale;
    if after != scale && after != ResultScale::Free {
        app.render_workspace.view.result_camera = result_camera(size, rect, ui.ctx().pixels_per_point(), after);
    }
    let camera = app.render_workspace.view.result_camera;
    let mut image_rect = result_rect(camera, size, rect);
    if after == ResultScale::Actual {
        let dpi = ui.ctx().pixels_per_point();
        let min = egui::pos2((image_rect.min.x * dpi).round() / dpi, (image_rect.min.y * dpi).round() / dpi);
        image_rect = image_rect.translate(min - image_rect.min);
    }
    let painter = ui.painter().with_clip_rect(rect.intersect(ui.clip_rect()));
    let visible = image_rect.intersect(rect);
    if visible.is_positive() {
        // Bound checker work by the visible canvas, even for an extremely magnified image.
        painter.rect_filled(visible, 0.0, t.panel);
        let cell = 24.0f32.max(visible.width().max(visible.height()) / 128.0);
        for row in 0..(visible.height() / cell).ceil() as u32 {
            for column in 0..(visible.width() / cell).ceil() as u32 {
                if (row + column) % 2 == 0 {
                    let square = egui::Rect::from_min_size(visible.min + egui::vec2(column as f32 * cell, row as f32 * cell), egui::vec2(cell, cell));
                    painter.rect_filled(square.intersect(visible), 0.0, t.control_fill);
                }
            }
        }
        if let Some(display) = displayed_image(app)
            && let Ok(texture) = display.image
        {
            painter.image(texture.id(), image_rect, egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)), Tokens::get().file_image_tint);
        }
        painter.rect_stroke(image_rect, 0.0, egui::Stroke::new(1.0, t.border), egui::StrokeKind::Outside);
    }
    let percent = f64::from(rect.height() * ui.ctx().pixels_per_point()) / (2.0 * camera.half_height()) * 100.0;
    let label = painter.layout_no_wrap(format!("{percent:.1}%"), egui::FontId::proportional(t.font_label), t.text);
    let badge = egui::Rect::from_min_size(
        rect.left_bottom() + egui::vec2(t.space_8, -label.size().y - t.space_8 * 2.0),
        label.size() + egui::vec2(t.space_8 * 2.0, t.space_8),
    );
    painter.rect_filled(badge, t.control_radius, t.panel);
    painter.galley(badge.min + egui::vec2(t.space_8, t.space_8 * 0.5), label, t.text);
    crate::navigation_bar_view::show(app, ui, rect);
}

impl State {
    fn prepare_preview(&mut self, signature: (u64, usize, u64, u64), capture: impl FnOnce() -> Result<ProductScene, String>) -> Result<(), String> {
        if self.scene_signature != Some(signature) {
            let prepared = capture().and_then(|scene| gpu_scene_with_budget(&scene, signature.3).map(|(key, gpu)| (scene, key, gpu)));
            self.scene_signature = Some(signature);
            match prepared {
                Ok((scene, key, gpu)) => {
                    let bytes = gpu.pbr.len() + gpu.tris.len() + gpu.glass.len();
                    if self.key != Some(key) {
                        let uploaded = match self.slot.lock() {
                            Ok(mut slot) => {
                                *slot = Some(gpu);
                                true
                            }
                            Err(_) => false,
                        };
                        if !uploaded {
                            let error = crate::i18n::tr("render.error_upload_slot").to_string();
                            self.scene = None;
                            self.key = None;
                            self.preview_upload_bytes = None;
                            self.scene_error = Some(error.clone());
                            return Err(error);
                        }
                        self.key = Some(key);
                        if let Ok(mut error) = self.upload_error.lock() {
                            *error = None;
                        }
                    }
                    self.scene = Some(scene);
                    self.scene_error = None;
                    self.preview_upload_bytes = Some(bytes);
                }
                Err(error) => {
                    self.scene = None;
                    self.key = None;
                    self.preview_upload_bytes = None;
                    if let Ok(mut slot) = self.slot.lock() {
                        *slot = None;
                    }
                    self.scene_error = Some(error);
                }
            }
        }
        match &self.scene_error {
            Some(error) => Err(error.clone()),
            None => Ok(()),
        }
    }
}

/// Use the shared kit's wrapped, themed error surface, not text painted directly
/// on the user-selected background (which may have the opposite theme contrast).
fn preview_error(app: &mut SolveApp, ui: &mut egui::Ui, rect: egui::Rect, error: &str) {
    let t = Tokens::get();
    let bounded = crate::shell::workspace_content(app, crate::shell::panel_bounds(app, rect)).shrink(t.space_12);
    // At tiny window sizes the shell may leave no room for the shared error card.
    if !bounded.is_finite() || bounded.width() < t.control_height + 2.0 * t.space_8 || bounded.height() < t.control_height + 2.0 * t.space_8 {
        return;
    }
    ui.scope_builder(egui::UiBuilder::new().max_rect(bounded), |ui| {
        ui.set_clip_rect(ui.clip_rect().intersect(bounded));
        ui.set_max_width(bounded.width().min(t.form_card_width * 2.0));
        crate::ui_kit::Panel::feature_frame(ui.ctx(), crate::shell::LayoutMode::Floating, crate::frame::Side::Left).inner_margin(t.space_8).show(
            ui,
            |ui| {
                crate::ui_kit::text(ui, error, crate::ui_kit::Text::Error("render.preview_failure"));
                crate::ui_kit::text(ui, app.tr("render.preview_unavailable_help"), crate::ui_kit::Text::Note("render.preview_unavailable_help"));
                let retry = crate::ui_kit::button(ui, app.tr("render.preview_retry"), crate::ui_kit::Button::Normal);
                crate::scenario::publish_target("render:preview-retry", retry.rect);
                if retry.clicked() {
                    invalidate_scene(app);
                    ui.ctx().request_repaint();
                }
            },
        );
    });
}

/// Keep the selected image and overwrite consent fixed while the upstream picker holds model input.
fn export_image(app: &mut SolveApp, job: solidcraft_engine::render::job::JobId, overwrite: bool) {
    let origin = app.session.doc.clone();
    app.pick_save("render.png", &["png"], move |app, path| {
        let Some(path) = path else { return };
        if !Arc::ptr_eq(&origin, &app.session.doc) {
            app.set_status(crate::i18n::tr("render.error_export_document"), true);
            return;
        }
        let _ = app.run("render.export", json!({"job":job,"path":path,"overwrite":overwrite}));
    });
}

/// Captures format and completed identity before the asynchronous picker opens.
fn export_linear_image(app: &mut SolveApp, job: solidcraft_engine::render::job::JobId, overwrite: bool) {
    let origin = app.session.doc.clone();
    app.pick_save("render.exr", &["exr"], move |app, path| {
        let Some(path) = path else {
            return;
        };
        if !Arc::ptr_eq(&origin, &app.session.doc) {
            app.set_status(crate::i18n::tr("render.error_export_document"), true);
            return;
        }
        let _ = app.run("render.export", json!({"job":job,"path":path,"overwrite":overwrite,"format":"exr"}));
    });
}

/// An exact expanded vertex payload, counted before allocating its buffers.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct PreviewUploadPlan {
    pbr: usize,
    tris: usize,
    glass: usize,
    source_triangles: usize,
}
impl PreviewUploadPlan {
    fn bytes(self) -> Result<usize, String> {
        self.pbr.checked_add(self.tris).and_then(|n| n.checked_add(self.glass)).ok_or_else(|| crate::i18n::tr("render.error_geometry_size").into())
    }
    fn add(&mut self, pbr: bool, glass: bool, triangles: usize, limit: usize) -> Result<(), String> {
        let stride = if pbr { crate::gpu::PBR_SIZE } else { crate::gpu::TRI_SIZE };
        let bytes = triangles.checked_mul(3).and_then(|n| n.checked_mul(stride)).ok_or(crate::i18n::tr("render.error_geometry_size"))?;
        let count = if pbr {
            &mut self.pbr
        } else if glass {
            &mut self.glass
        } else {
            &mut self.tris
        };
        *count = count.checked_add(bytes).ok_or(crate::i18n::tr("render.error_geometry_size"))?;
        admit_geometry(self.bytes()? as u64, limit as u64)
    }
}

fn preview_upload_plan(scene: &ProductScene, limit: usize) -> Result<PreviewUploadPlan, String> {
    if scene.meshes.len() > solidcraft_engine::render::job::MAX_CPU_BODIES {
        return Err(crate::i18n::tr("render.error_body_bound").into());
    }
    let mut plan = PreviewUploadPlan::default();
    for body in &scene.meshes {
        plan.source_triangles = plan.source_triangles.checked_add(body.mesh.triangles.len()).ok_or(crate::i18n::tr("render.error_triangle_count"))?;
        if plan.source_triangles > solidcraft_engine::render::job::MAX_CPU_TRIANGLES {
            return Err(crate::i18n::tr("render.error_triangle_bound").into());
        }
        if body.faces.len() > 262_144 {
            return Err(crate::i18n::tr("render.error_face_cap").into());
        }
        if !body.opacity.is_finite()
            || !(0.0..=1.0).contains(&body.opacity)
            || body.faces.iter().any(|face| !face.opacity.is_finite() || !(0.0..=1.0).contains(&face.opacity))
        {
            return Err(crate::i18n::tr("render.error_opacity").into());
        }
        let looks: std::collections::BTreeMap<_, _> = body.faces.iter().map(|face| (face.face, face)).collect();
        if looks.len() != body.faces.len() {
            return Err(crate::i18n::tr("render.error_duplicate_face").into());
        }
        for index in 0..body.mesh.triangles.len() {
            let face = body.mesh.tri_face.get(index).and_then(|id| looks.get(id));
            let (opacity, pbr) = face.map_or((body.opacity, body.surface.is_some() && body.transmission.is_none()), |face| {
                (face.opacity, face.surface.is_some() && face.transmission.is_none())
            });
            if pbr && opacity != 1.0 {
                return Err(crate::i18n::tr("render.error_pbr_opacity").into());
            }
            if opacity > 0.0 {
                plan.add(pbr, opacity < 0.999, 1, usize::MAX)?;
            }
        }
    }
    admit_geometry(plan.bytes()? as u64, limit as u64)?;
    Ok(plan)
}

/// Convert all captured surfaces, without decimation. The finite, exact payload
/// plan is admitted first; each output vector is reserved once rather than grown
/// once per triangle. Device uploads retain their existing chunked buffer path.
#[cfg(test)]
fn gpu_scene(scene: &ProductScene) -> Result<(u64, GpuScene), String> {
    gpu_scene_with_budget(scene, MAX_GEOMETRY_BYTES as u64)
}

fn gpu_scene_with_budget(scene: &ProductScene, budget: u64) -> Result<(u64, GpuScene), String> {
    let limit = usize::try_from(budget).unwrap_or(usize::MAX);
    let plan = preview_upload_plan(scene, limit)?;
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    let mut gpu = GpuScene::default();
    for (buffer, bytes) in [(&mut gpu.pbr, plan.pbr), (&mut gpu.tris, plan.tris), (&mut gpu.glass, plan.glass)] {
        buffer.try_reserve_exact(bytes).map_err(|_| crate::i18n::tr("render.error_vertex_allocation").to_string())?;
    }
    for body in &scene.meshes {
        if body.faces.len() > 262_144 {
            return Err(crate::i18n::tr("render.error_face_cap").into());
        }
        if !body.opacity.is_finite()
            || !(0.0..=1.0).contains(&body.opacity)
            || body.faces.iter().any(|f| !f.opacity.is_finite() || !(0.0..=1.0).contains(&f.opacity))
        {
            return Err(crate::i18n::tr("render.error_opacity").into());
        }
        (Arc::as_ptr(&body.mesh) as usize).hash(&mut hash);
        body.name.hash(&mut hash);
        body.color.hash(&mut hash);
        body.opacity.to_bits().hash(&mut hash);
        body.transmission.is_some().hash(&mut hash);
        hash_surface(body.surface, &mut hash);
        let looks: std::collections::BTreeMap<_, _> = body.faces.iter().map(|l| (l.face, l)).collect();
        for face in &body.faces {
            face.face.hash(&mut hash);
            face.color.hash(&mut hash);
            face.opacity.to_bits().hash(&mut hash);
            face.transmission.is_some().hash(&mut hash);
            hash_surface(face.surface, &mut hash);
        }
        for (index, triangle) in body.mesh.triangles.iter().enumerate() {
            let face = body.mesh.tri_face.get(index).and_then(|f| looks.get(f));
            let (color, opacity, surface) = face.map_or((body.color, body.opacity, body.surface.filter(|_| body.transmission.is_none())), |l| {
                (l.color, l.opacity, l.surface.filter(|_| l.transmission.is_none()))
            });
            if surface.is_some() && opacity != 1.0 {
                return Err(crate::i18n::tr("render.error_pbr_opacity").into());
            }
            if opacity <= 0.0 {
                continue;
            }
            let vertices = triangle.map(|i| {
                let p = body.mesh.positions.get(i as usize)?.to_f32();
                let n = *body.mesh.normals.get(i as usize)?;
                let n = if surface.is_some() { solidcraft_engine::render::surface::unit_direction(n).ok()? } else { n }.to_f32();
                p.into_iter().chain(n).all(f32::is_finite).then_some((p, n))
            });
            let [Some(a), Some(b), Some(c)] = vertices else { return Err(crate::i18n::tr("render.error_product_triangle").into()) };
            let rgba = [color[0], color[1], color[2], (opacity * 255.0).round() as u8];
            let index = gpu.color(rgba);
            for (p, n) in [a, b, c] {
                if let Some(surface) = surface {
                    if n.iter().all(|v| *v == 0.0) {
                        return Err(crate::i18n::tr("render.error_surface_normals").into());
                    }
                    gpu.pbr_tri(p, n, surface);
                } else if opacity < 0.999 {
                    gpu.glass_tri(p, n, index);
                } else {
                    gpu.tri(p, n, index);
                }
            }
        }
    }
    if (gpu.pbr.len(), gpu.tris.len(), gpu.glass.len()) != (plan.pbr, plan.tris, plan.glass) {
        return Err(crate::i18n::tr("render.error_vertex_packing").into());
    }
    Ok((hash.finish(), gpu))
}

fn hash_surface(surface: Option<solidcraft_engine::render::surface::OpaqueSurface>, hash: &mut impl Hasher) {
    surface.is_some().hash(hash);
    if let Some(s) = surface {
        let c = s.base_color();
        for value in [c.x, c.y, c.z, s.metallic(), s.roughness(), s.dielectric_ior()] {
            value.to_bits().hash(hash);
        }
    }
}

/// The image's width over its height.
pub fn image_aspect(setup: &RenderSetup) -> f64 {
    f64::from(setup.width.max(1)) / f64::from(setup.height.max(1))
}

/// `layout` seen from the picture: while Frame shows the picture's frame the preview projects
/// into that frame, not the viewport, so a fit that keeps the product inside this layout keeps
/// it inside the image. Elsewhere the preview fills the viewport and the layout is unchanged.
pub(crate) fn picture_layout(app: &SolveApp, layout: &crate::viewport_layout::Layout) -> crate::viewport_layout::Layout {
    let Some(frame) = shot::preview_rect(app, layout.viewport) else { return layout.clone() };
    // The same 16-point margin the viewport's fit keeps inside its free area.
    crate::viewport_layout::Layout { viewport: frame, drawable: layout.drawable.intersect(frame.shrink(16.0)), ..layout.clone() }
}

/// The size the product preview draws at for a viewport of `full` physical pixels: the full
/// size when the viewport executor admits it, else the largest size at the same aspect it does.
/// Without an executor the request is left for resolution to report.
pub fn preview_size(executor: Option<&ExecutorDescriptor>, full: (u64, u64)) -> (u64, u64) {
    executor.and_then(|e| e.fit(full.0, full.1)).unwrap_or(full)
}

/// Theme only the untouched, unsaved preview. Saved setups have no background provenance,
/// so even a saved default-colour backplate remains scene-owned. Never alter image requests.
fn preview_background(app: &SolveApp) -> Tokens {
    let view = &app.render_workspace.view;
    let tokens = Tokens::get();
    if crate::theme::tab_surfaces_enabled()
        && !app.session.doc.render.setups.contains_key(&view.name)
        && view.clean_setup.as_ref().is_some_and(|clean| clean.background == view.draft.background)
        && !view.transparent
        && !view.studio.backdrop_enabled
        && view.studio.environment == studio::Environment::None
        && view.cpu.environment == cpu::Environment::None
        && view.raytrace.environment.is_none()
    {
        // Includes the current fade and explicit viewport palette/flat-colour preferences.
        tokens
    } else {
        tokens.with_background(Some(view.draft.background))
    }
}

pub fn show(app: &mut SolveApp, ui: &mut egui::Ui) {
    if app.render_workspace.view.compare.is_some() {
        app.render_workspace.view.environment_ring = environment::RingView::default();
        assign::stop(app);
        compare::show(app, ui);
        return;
    }
    if app.render_workspace.view.result {
        app.render_workspace.view.environment_ring = environment::RingView::default();
        assign::stop(app);
        show_result(app, ui);
        return;
    }
    let viewport = ui.max_rect();
    app.viewport.rect = Some(viewport);
    // Framing uses the output camera's own aspect, including navigation and picking.
    let rect = shot::preview_rect(app, viewport).unwrap_or(viewport);
    crate::viewport::refresh_layout(app, ui.ctx());
    let response = ui.interact(rect, ui.id().with("render_preview"), Sense::click_and_drag());
    if !environment::ring_input(app, ui, rect, &response) {
        let input = crate::navigation::handle(app, ui, rect, &response, crate::navigation::NavCaps::RENDER);
        let navigating = input.scroll != 0.0
            || input.pinch != 1.0
            || input.pan != egui::Vec2::ZERO
            || input.middle
            || input.secondary_down
            || (input.primary_nav.is_some() && input.primary_down)
            || input.mode.is_some()
            || app.viewport.axis_drag.is_some()
            || app.viewport.spin
            || app.cam_anim.is_some();
        let hits = input
            .hover
            .filter(|_| input.inside && !navigating)
            .map(|p| crate::viewport::pick_cached(app, &crate::viewport::projection(app, rect), p))
            .unwrap_or_default();
        let candidate = assign::candidate(app, &hits, input.mods);
        app.viewport.hover = candidate.as_ref().map(|(hit, _)| hit.clone());
        let assigning = assign::input(app, ui, &response, candidate.as_ref(), input.inside && !navigating);
        if !assigning && input.inside && !navigating {
            assign::hover_tooltip(app, &response, candidate.as_ref());
        }
        if !assigning && response.clicked_by(egui::PointerButton::Primary) && input.inside && !navigating {
            let (add, toggle) = crate::tool_modifiers::selection_click(
                crate::tool_modifiers::Held::from_modifiers(input.mods),
                app.preferences.selection_shift_add_only,
            );
            crate::viewport::select(app, candidate.as_ref().map(|(_, sel)| sel.clone()), add, toggle);
        }
        if response.secondary_clicked()
            && let Some((crate::viewport::Hit::Face { body, .. }, _)) = &candidate
        {
            if let Some(candidate) = &candidate {
                assign::context(app, candidate);
            }
            crate::context_menu::open_for(app, input.hover.unwrap_or(rect.center()), crate::context_menu::Target::Body { name: body.clone() });
        }
    } else {
        app.viewport.hover = None;
    }
    crate::gpu::paint_background(ui.painter(), viewport, &preview_background(app));
    paint_product(app, ui, rect, app.cam, None);
    shot::overlay(app, ui, viewport);
    crate::viewport::axis_view(app, ui, viewport);
    crate::navigation_bar_view::show(app, ui, viewport);
}

fn product_projection(camera: Camera, radius: f64, canvas: egui::Rect, image: Option<egui::Rect>) -> solidcraft_engine::render::Mat4 {
    let image = image.unwrap_or(canvas);
    let projection = camera.view_proj(f64::from(image.width() / image.height().max(1.0)), radius);
    let sx = f64::from(image.width() / canvas.width().max(1.0));
    let sy = f64::from(image.height() / canvas.height().max(1.0));
    let tx = f64::from(2.0 * (image.center().x - canvas.center().x) / canvas.width().max(1.0));
    let ty = f64::from(2.0 * (canvas.center().y - image.center().y) / canvas.height().max(1.0));
    solidcraft_engine::render::Mat4([[sx, 0.0, 0.0, 0.0], [0.0, sy, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [tx, ty, 0.0, 1.0]]).mul(&projection)
}

fn paint_product(app: &mut SolveApp, ui: &mut egui::Ui, rect: egui::Rect, camera: Camera, image: Option<egui::Rect>) {
    let background = preview_background(app);
    crate::gpu::paint_background(ui.painter(), rect, &background);
    let result = (|| -> Result<(), String> {
        if app.viewport.gpu.is_none() {
            return Err(crate::i18n::tr("render.error_gpu_preview").into());
        }
        let ppp = ui.ctx().pixels_per_point();
        let setup = &app.render_workspace.view.draft;
        if (app.render_workspace.view.method == ImageMethod::Raster && setup.exposure_ev != 0.0)
            || setup.environment_rotation != 0.0
            || setup.environment_intensity != 1.0
        {
            return Err(crate::i18n::tr("render.error_fixed_preview").into());
        }
        // A 4K or 5K viewport is past the preview's pixel budget: draw it smaller and filter it
        // up into the viewport rather than refusing the frame.
        let full = ((rect.width() * ppp).max(1.0) as u64, (rect.height() * ppp).max(1.0) as u64);
        let (width, height) = preview_size(app.session.render_viewport(), full);
        let mut request = RenderRequest {
            backend: Some(setup.backend.clone()),
            mode: setup.mode,
            operation: RenderOperation::Viewport,
            width,
            height,
            format: None,
            features: Vec::new(),
            policy: Default::default(),
        };
        solidcraft_engine::render::resolve_request(&[app.session.render_backend()], &request).map_err(|e| e.to_string())?;
        if app.session.model.is_stale() {
            return Err(crate::i18n::tr("render.error_finish_gesture").into());
        }
        let mut visibility = std::collections::hash_map::DefaultHasher::new();
        app.session.visibility.hidden_bodies.hash(&mut visibility);
        app.session.visibility.hidden_occurrences.hash(&mut visibility);
        let budget = preferred_geometry_budget(app.render_workspace.geometry_budget, app.preferences.render_previous_geometry_budget);
        let signature =
            (app.session.revision, Arc::as_ptr(&app.session.world_state()) as usize, visibility.finish() ^ assign::preview_key(app), budget);
        let preview = assign::preview(app);
        let previous_key = app.render_workspace.key;
        app.render_workspace.prepare_preview(signature, || {
            let mut scene = solidcraft_engine::product_scene::extract(&app.session).map_err(|error| error.to_string())?;
            assign::preview_scene(&mut scene, preview.as_ref())?;
            Ok(scene)
        })?;
        if app.render_workspace.key != previous_key {
            ui.ctx().request_repaint();
        }
        let scene = app.render_workspace.scene.as_ref().ok_or(crate::i18n::tr("render.error_no_scene"))?;
        if let Some(error) = app.render_workspace.upload_error.lock().ok().and_then(|e| e.clone()) {
            return Err(upload_refusal(app.render_workspace.preview_upload_bytes.unwrap_or(0) as u64, &error));
        }
        request.features = scene.required_features();
        solidcraft_engine::render::resolve_request(&[app.session.render_backend()], &request).map_err(|e| e.to_string())?;
        let key = app.render_workspace.key.ok_or(crate::i18n::tr("render.error_no_geometry"))?;
        if scene.bounds.is_empty() {
            return Err(crate::i18n::tr("render.error_visible_solid").into());
        }
        let radius = scene.bounds.diagonal().max(10.0) + scene.bounds.center().dist(camera.target);
        let eye = camera.eye();
        let highlight_key = crate::viewport::highlight_key(app);
        if highlight_key != app.viewport.hl_key {
            let overlay = crate::viewport::build_highlight(app);
            if let Ok(mut slot) = app.viewport.hl_slot.lock() {
                *slot = Some(overlay);
            }
            app.viewport.hl_key = highlight_key;
        }
        let slot = app.render_workspace.slot.clone();
        let callback = ViewportCallback(Frame {
            product_lights: Some(light::preview(app)?),
            product: true,
            upload_error: Some(app.render_workspace.upload_error.clone()),
            key,
            slot: slot.clone(),
            sketch_key: 0,
            sketch_slot: slot.clone(),
            hl_key: app.viewport.hl_key,
            hl_slot: app.viewport.hl_slot.clone(),
            pv_key: 0,
            pv_slot: slot,
            pv_hide: false,
            view_proj: product_projection(camera, radius, rect, image).to_f32(),
            back: camera.back().to_f32(),
            size_px: [rect.width() * ppp, rect.height() * ppp],
            clip: [0.0; 4],
            cap: [0.0; 4],
            images: Vec::new(),
            clouds: Vec::new(),
            analysis: [0.0; 8],
            analysis_colors: [0.0; 36],
            access: None,
            outline: false,
            // Product rendering has its own lighting; AO is a Design viewport setting.
            ao_radius: 0.0,
            ao_samples: solidcraft_gpu::view::AO_SAMPLES,
            sky: [0.0; 4],
            eye: [eye.x as f32, eye.y as f32, eye.z as f32, if camera.fov > 0.0 { 1.0 } else { 0.0 }],
            ssaa: if width < full.0 { width as f32 / full.0 as f32 } else { 1.0 },
            hover_tint: [f32::from(Tokens::get().hover_face_lift) / 255.0; 3],
            selection_tint: {
                let c = Tokens::get().body_selected;
                [f32::from(c.r()) / 255.0, f32::from(c.g()) / 255.0, f32::from(c.b()) / 255.0]
            },
        });
        crate::blur::viewport(ui.painter(), rect, callback, rect, [background.viewport_top, background.viewport_bottom]);
        Ok(())
    })();
    if let Err(error) = result {
        preview_error(app, ui, rect, &error);
    }
}

/// Apply validated UI requests after the same registry call used by automation.
pub(crate) fn apply_command(app: &mut SolveApp, id: &str, reply: &serde_json::Value) -> Result<(), String> {
    match id {
        "render.preflight.copy" => {
            app.render_workspace.clipboard_text = Some(reply["text"].as_str().ok_or("invalid check report")?.to_string());
        }
        "render.compare" => compare::apply(app, reply)?,
        "render.compare.divider" => {
            let position = reply["position"].as_f64().ok_or("invalid comparison divider position")?;
            if let Some(view) = &mut app.render_workspace.view.compare {
                view.position = crate::ui_kit::comparison_position(position as f32);
            }
        }
        "render.batch" => follow_batch(app),
        "render.start" | "render.result.regrade" => {
            let job = serde_json::from_value(reply["job"].clone()).map_err(|e| format!("invalid render job identity: {e}"))?;
            let shot = if id == "render.result.regrade" {
                app.render_workspace
                    .view
                    .retained
                    .iter()
                    .find(|result| Some(result.frame.job) == app.render_workspace.view.completed)
                    .and_then(|result| result.shot.clone())
            } else {
                Some(app.render_workspace.view.name.clone())
            };
            if let Some(shot) = &shot {
                let _ = app.session.label_render_job(job, shot);
            }
            app.render_workspace.view.job = Some(job);
            app.render_workspace.view.job_shot = shot;
            app.render_workspace.view.follow_job = app.render_workspace.view.compare.is_none();
        }
        "render.result.select" | "render.result.discard" | "render.result.pin" => {
            sync_results(app);
            app.render_workspace.preview_image = None;
            app.render_workspace.view.follow_job = false;
            if matches!(id, "render.result.select" | "render.result.pin") {
                app.render_workspace.view.compare = None;
                app.render_workspace.comparison_images.clear();
            }
            if id == "render.result.select" {
                app.render_workspace.view.result = true;
            }
        }
        "render.output.size" => {
            let width = reply["width"].as_u64().and_then(|v| u32::try_from(v).ok()).ok_or("invalid output width")?;
            let height = reply["height"].as_u64().and_then(|v| u32::try_from(v).ok()).ok_or("invalid output height")?;
            if cpu::image_limits(app).fit(u64::from(width), u64::from(height), None) != Some((u64::from(width), u64::from(height))) {
                return Err(app.tr("render.output_over_budget").into());
            }
            app.render_workspace.view.inspector.resolution.custom = reply["custom"].as_bool().unwrap_or(false);
            app.render_workspace.view.draft.width = width;
            app.render_workspace.view.draft.height = height;
            app.render_workspace.view.dirty = true;
        }
        "render.result.view" | "render.result.close" => {
            let view = reply["view"].as_str().ok_or("invalid result view")?;
            if view == "actual" && reduced_display(app) {
                return Err(app.tr("render.result_reduced_display").into());
            }
            app.render_workspace.view.result = view != "model";
            if matches!(view, "model") || id == "render.result.close" {
                app.render_workspace.view.compare = None;
                app.render_workspace.comparison_images.clear();
            }
            match view {
                "fit" => fit_result(app),
                "actual" => app.render_workspace.view.result_scale = ResultScale::Actual,
                _ => {}
            }
            app.cancel_view_animation();
            app.viewport.nav = None;
            app.viewport.key_hold = None;
            app.viewport.orbit_free = None;
        }
        "render.result.copy" => {
            let job = serde_json::from_value(reply["frame"]["job"].clone()).map_err(|e| e.to_string())?;
            let frame = app
                .session
                .render_job_handle(job)
                .map_err(|e| e.to_string())?
                .frame()
                .map_err(|e| e.to_string())?
                .ok_or_else(|| app.tr("render.ribbon_no_result").to_string())?;
            let image = decode_render_image(frame.png(), frame.info.width, frame.info.height, RenderLimits::default(), 4096)?;
            app.render_workspace.clipboard_image = Some(image);
        }
        _ => {}
    }
    Ok(())
}

pub(crate) fn progress_pill(app: &mut SolveApp, ui: &mut egui::Ui) -> bool {
    if !ribbon::busy(app) {
        return false;
    }
    let Some(job) = app.render_workspace.view.job else { return false };
    let Ok(status) = app.session.execute("render.status", &json!({"job":job})) else { return false };
    let phase = status["progress"]["phase"].as_str().unwrap_or("unknown");
    if !matches!(phase, "queued" | "preparing" | "running" | "encoding" | "cancelling") {
        return false;
    }
    let done = status["progress"]["done"].as_u64().unwrap_or(0);
    let total = status["progress"]["total"].as_u64().unwrap_or(1).max(1);
    let fraction = (done as f64 / total as f64).clamp(0.0, 1.0) as f32;
    let phase_key = format!("render.image_{phase}");
    let name = app.render_workspace.view.job_shot.as_ref().unwrap_or(&app.render_workspace.view.name).clone();
    let cancel =
        crate::ui_kit::progress_action(ui, &format!("{name} · {} · {:.0}%", app.tr(&phase_key), fraction * 100.0), fraction, app.tr_label("Cancel"));
    crate::scenario::publish_target("render:progress-cancel", cancel.rect);
    if cancel.clicked() {
        app.start("render.cancel");
    }
    ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
    true
}

pub(crate) fn environment_overlay(app: &mut SolveApp, ui: &mut egui::Ui, viewport: egui::Rect) {
    environment::ring_overlay(app, ui, viewport);
}

pub(crate) fn interactive_command_info(app: &SolveApp, command: &solidcraft_engine::CommandSpec) -> solidcraft_engine::CommandInfo {
    let mut info = ribbon::command_info(app, command);
    // A drawing tool on the SKETCH tab kept shown outside a sketch starts one (#7904).
    if crate::workspace::starts_a_sketch(app, command.id) {
        info.enabled = true;
        info.disabled_reason = None;
    }
    info
}
pub(crate) fn comparison_controls(app: &mut SolveApp, ui: &mut egui::Ui) {
    compare::controls(app, ui);
}
pub(crate) fn start_interactive(app: &mut SolveApp, id: &str) -> bool {
    let handled = assign::start(app, id) || ribbon::start(app, id);
    // Opening Appearance Advanced must still reach its atmosphere controls after leaving Wear.
    if handled
        && id == "render.aging.describe"
        && app.render_workspace.view.inspector.page == inspector::Page::Appearance
        && app.render_workspace.view.selection == RenderSelection::Wear
    {
        let _ = app.run("view.render_select", json!({"kind":"nothing"}));
        // Clearing semantic focus routes to Scene; retain the explicit Advanced destination.
        app.render_workspace.view.inspector.page = inspector::Page::Appearance;
    }
    handled
}
