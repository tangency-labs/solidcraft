//! Automatic, isolated product-image feedback. The original session owns final results only.
//! A scratch session reuses the same commands, immutable model and installed host pools.
use std::hash::{Hash, Hasher};
use std::sync::{
    Arc, Weak,
    atomic::{AtomicBool, Ordering},
    mpsc,
};

use egui::{Context, Rect, TextureHandle, emath::GuiRounding as _};
use serde_json::{Value, json};
use solidcraft_engine::{
    Session,
    doc::Document,
    render::job::{CpuProgressiveOptions, JobHandle, JobId, Phase},
};

use super::{ImageMethod, ribbon};
use crate::ui_kit::BackingTooltipExt as _;
use crate::{SolveApp, ui_kit};

const LONGEST_EDGE: u32 = 512;
const EDIT_DELAY_SECONDS: f64 = 0.2;

#[derive(Clone)]
struct Identity {
    document: Weak<Document>,
    revision: u64,
    model: Weak<solidcraft_engine::doc::ModelState>,
    moves: std::collections::BTreeMap<u64, solidcraft_engine::doc::Mat>,
    visibility: u64,
    shot: String,
    request: Value,
}
impl Identity {
    fn same(&self, other: &Self) -> bool {
        Weak::ptr_eq(&self.document, &other.document)
            && self.revision == other.revision
            && Weak::ptr_eq(&self.model, &other.model)
            && self.moves == other.moves
            && self.visibility == other.visibility
            && self.shot == other.shot
            && self.request == other.request
    }
}

struct OwnedJob {
    // Keep the scratch session alive: dropping its Jobs cancels its live handles.
    _owner: Session,
    handle: JobHandle,
    generation: u64,
    revision: u64,
    target: u32,
}
struct Pending {
    receiver: mpsc::Receiver<Result<OwnedJob, String>>,
    cancelled: Arc<AtomicBool>,
    generation: u64,
}
struct Picture {
    generation: u64,
    job: JobId,
    sequence: u32,
    samples: u32,
    complete: bool,
    adaptive: Option<solidcraft_engine::render::job::adaptive::AdaptivePreview>,
    executor: solidcraft_engine::render::ExecutorId,
    /// Captured alongside this texture from the same validated immutable frame.
    scene_generation: u64,
    size: [u32; 2],
    texture: TextureHandle,
}
#[derive(Default)]
pub(crate) struct State {
    identity: Option<Identity>,
    generation: u64,
    due: f64,
    attempted: bool,
    pending: Option<Pending>,
    running: Option<OwnedJob>,
    picture: Option<Picture>,
    error: Option<String>,
    suspended: bool,
    draining: Vec<JobHandle>,
    final_request: Option<ribbon::PendingFinal>,
}
impl Drop for State {
    fn drop(&mut self) {
        self.cancel_work();
    }
}
impl State {
    pub(super) fn retire_document(&mut self) {
        self.reset();
        self.final_request = None;
        self.suspended = true;
    }
    fn cancel_work(&mut self) {
        if let Some(pending) = &self.pending {
            pending.cancelled.store(true, Ordering::Release);
        }
        if let Some(job) = self.running.take() {
            let _ = job.handle.cancel();
            self.draining.push(job.handle.clone());
        }
    }
    fn reset(&mut self) {
        self.cancel_work();
        self.identity = None;
        self.attempted = false;
        self.picture = None;
        self.error = None;
    }
    fn observe(&mut self, identity: Identity, now: f64) {
        if self.identity.as_ref().is_some_and(|old| old.same(&identity)) {
            return;
        }
        self.cancel_work();
        self.generation = self.generation.saturating_add(1);
        self.identity = Some(identity);
        self.due = now + EDIT_DELAY_SECONDS;
        self.attempted = false;
        self.error = None;
    }
    fn poll_capture(&mut self) {
        let Some(pending) = &self.pending else { return };
        let answer = match pending.receiver.try_recv() {
            Ok(answer) => answer,
            Err(mpsc::TryRecvError::Empty) => return,
            Err(mpsc::TryRecvError::Disconnected) => Err(solidcraft_ui_text::tr("render.live.worker_stopped").into()),
        };
        let generation = pending.generation;
        let cancelled = pending.cancelled.load(Ordering::Acquire);
        self.pending = None;
        match answer {
            Ok(job) if generation == self.generation && !cancelled && !self.suspended => self.running = Some(job),
            Ok(job) => {
                let _ = job.handle.cancel();
                self.draining.push(job.handle.clone());
            }
            Err(error) if generation == self.generation && !cancelled && !self.suspended => self.error = Some(error),
            Err(_) => {}
        }
    }
    fn reap_draining(&mut self) {
        self.draining.retain(|handle| !handle.work_released());
    }
    fn poll_pixels(&mut self, ctx: &Context) {
        let Some(job) = &self.running else { return };
        if job.generation != self.generation {
            return;
        }
        let result = (|| -> Result<Option<Picture>, String> {
            let status = job.handle.status().map_err(|e| e.to_string())?;
            if status.job != job.handle.id() || status.scene_generation != job.revision {
                return Err(solidcraft_ui_text::tr("render.error_provisional_identity").into());
            }
            if let Some(error) = &status.error {
                return Err(error.to_string());
            }
            let limits = if status.resolved.executor.as_str() == "gpu-path" {
                solidcraft_engine::render::job::GPU_PATH_LIMITS
            } else {
                super::captured_image_limits(&status.request)
            };
            let current = self.picture.as_ref().filter(|picture| picture.generation == job.generation && picture.job == job.handle.id());
            let load = |info: &solidcraft_engine::render::job::FrameInfo, png: &[u8], sequence, samples, complete, adaptive| {
                if info.job != job.handle.id()
                    || info.scene_generation != job.revision
                    || info.executor != status.resolved.executor
                    || (info.width, info.height) != (status.request.width, status.request.height)
                {
                    return Err(solidcraft_ui_text::tr("render.error_provisional_identity").to_string());
                }
                let image = super::decode_render_image(png, info.width, info.height, limits, ctx.input(|i| i.max_texture_side))?;
                Ok(Picture {
                    generation: job.generation,
                    job: info.job,
                    sequence,
                    samples,
                    complete,
                    adaptive,
                    executor: info.executor.clone(),
                    scene_generation: info.scene_generation,
                    size: [info.width, info.height],
                    texture: ctx.load_texture("sc_render_live", image, egui::TextureOptions::LINEAR),
                })
            };
            if let Some(frame) = job.handle.frame().map_err(|e| e.to_string())? {
                if current.is_some_and(|picture| picture.complete) {
                    return Ok(None);
                }
                let adaptive = super::captured_adaptive_preview(&status, job.target)?;
                return load(&frame.info, frame.png(), current.map_or(1, |picture| picture.sequence), job.target, true, adaptive).map(Some);
            }
            if let Some(preview) = job.handle.preview().map_err(|e| e.to_string())? {
                if current.is_some_and(|picture| picture.sequence >= preview.info.frame_sequence) {
                    return Ok(None);
                }
                let adaptive = super::captured_adaptive_preview(&status, preview.info.samples_per_pixel)?;
                if preview.info.frame_sequence == 0
                    || preview.info.samples_per_pixel == 0
                    || preview.info.samples_per_pixel >= job.target
                    || preview.info.adaptive != adaptive
                {
                    return Err(solidcraft_ui_text::tr("render.error_provisional_identity").into());
                }
                return load(&preview.info.frame, preview.png(), preview.info.frame_sequence, preview.info.samples_per_pixel, false, adaptive)
                    .map(Some);
            }
            if let Some(reason) = status.preview_unavailable_reason {
                return Err(reason.into());
            }
            if matches!(status.progress.phase, Phase::Cancelled | Phase::Failed) {
                return Err(solidcraft_ui_text::tr("render.live.stopped").into());
            }
            Ok(None)
        })();
        match result {
            Ok(Some(picture)) => {
                self.picture = Some(picture);
                ctx.request_repaint();
            }
            Ok(None) => {}
            Err(error) => {
                self.error = Some(error);
                self.cancel_work();
            }
        }
    }
}

fn enabled(app: &SolveApp) -> bool {
    !cfg!(target_arch = "wasm32") && !app.preferences.render_legacy_image_defaults && app.render_workspace.view.method != ImageMethod::Raster
}
fn rounded_size(rect: Rect) -> [u32; 2] {
    let width = f64::from(rect.width().max(1.0));
    let height = f64::from(rect.height().max(1.0));
    let longest = width.max(height);
    let scale = (f64::from(LONGEST_EDGE) / longest).min(1.0);
    [(width * scale).round().clamp(1.0, f64::from(LONGEST_EDGE)) as u32, (height * scale).round().clamp(1.0, f64::from(LONGEST_EDGE)) as u32]
}
fn size(app: &SolveApp, rect: Rect) -> [u32; 2] {
    let [width, height] = [app.render_workspace.view.draft.width, app.render_workspace.view.draft.height];
    if width == 0 || height == 0 {
        // Request validates original intent before any temporary live-size copy.
        return rounded_size(rect);
    }
    let (mut a, mut b) = (width, height);
    while b != 0 {
        (a, b) = (b, a % b);
    }
    let unit = [width / a, height / a];
    let maximum_scale = LONGEST_EDGE / unit[0].max(unit[1]);
    if maximum_scale > 0 {
        // Exact common ratios (e.g. 160:120→4:3) stay identical through every
        // free-canvas shape. Never round width/height independently when an
        // admitted integer multiple can preserve the output camera exactly.
        let scale = (f64::from(rect.width()) / f64::from(unit[0]))
            .min(f64::from(rect.height()) / f64::from(unit[1]))
            .floor()
            .clamp(1.0, f64::from(maximum_scale)) as u32;
        return [unit[0] * scale, unit[1] * scale];
    }
    // Coprime authored dimensions can exceed the 512-pixel live ceiling even
    // though the final job is valid. Use the nearest bounded pixel dimensions;
    // projection_rect applies that captured aspect too, without stretching.
    rounded_size(super::shot::frame_rect(f64::from(width) / f64::from(height), rect).unwrap_or(rect))
}

/// Match product navigation, picking and pixels to the exact integer live-capture aspect.
/// At most a pixel of letterboxing is introduced instead of stretching the rendered image.
pub(crate) fn projection_rect(app: &SolveApp, rect: Rect) -> Rect {
    if !enabled(app) || !rect.is_finite() || !rect.is_positive() {
        return rect;
    }
    let [width, height] = size(app, rect);
    super::shot::frame_rect(f64::from(width) / f64::from(height), rect).unwrap_or(rect)
}

/// Build a separate, labelled live-size request. Final size, AA and saved intent are restored
/// before returning, including on refusal. Effects are never removed to obtain a preview.
fn request(app: &mut SolveApp) -> Result<Value, String> {
    super::image_host(app, app.render_workspace.view.method)?;
    if app.session.model.is_stale() {
        return Err(app.tr("render.image_scene_stale").into());
    }
    let old_size = [app.render_workspace.view.draft.width, app.render_workspace.view.draft.height];
    if old_size.contains(&0) {
        return Err(app.tr("render.live.invalid_size").into());
    }
    // A labelled low-resolution live copy must not make invalid authored output
    // dimensions/sampling appear valid. Refuse original intent before scaling.
    let original = (u64::from(old_size[0]), u64::from(old_size[1]));
    if super::cpu::image_limits(app).fit(original.0, original.1, None) != Some(original) {
        return Err(super::inspector::size_limit_reason(app, old_size[0], old_size[1]));
    }
    super::cpu::validate_progressive_image(app)?;
    let old_sampling = app.render_workspace.view.raytrace.progressive;
    let old_primary = app.render_workspace.view.raytrace.primary_samples;
    let old_reflection_samples = app.render_workspace.view.raytrace.reflections.as_ref().map(|options| options.samples);
    let viewport =
        app.viewport.rect.filter(|rect| rect.is_finite() && rect.is_positive()).ok_or_else(|| app.tr("render.live.waiting_canvas").to_string())?;
    let canvas = super::shot::render_rect(app, viewport);
    let live_size = size(app, canvas);
    app.render_workspace.view.draft.width = live_size[0];
    app.render_workspace.view.draft.height = live_size[1];
    if app.render_workspace.view.method == ImageMethod::CpuRaytrace && old_sampling.is_none() {
        app.render_workspace.view.raytrace.progressive = Some(CpuProgressiveOptions::default());
        app.render_workspace.view.raytrace.primary_samples = 1;
        if let Some(reflections) = &mut app.render_workspace.view.raytrace.reflections {
            // A live film ordinal supplies a fresh reflected camera sample. Preserve the
            // authored fixed image's attempt count when this temporary copy is restored.
            reflections.samples = 1;
        }
    }
    let result = super::image_request(app);
    app.render_workspace.view.draft.width = old_size[0];
    app.render_workspace.view.draft.height = old_size[1];
    app.render_workspace.view.raytrace.progressive = old_sampling;
    app.render_workspace.view.raytrace.primary_samples = old_primary;
    if let (Some(reflections), Some(samples)) = (&mut app.render_workspace.view.raytrace.reflections, old_reflection_samples) {
        reflections.samples = samples;
    }
    result
}
fn target(request: &Value) -> u32 {
    let samples = request
        .pointer("/settings/job/options/progressive/samples_per_pixel")
        .or_else(|| request.pointer("/raytrace/progressive/samples_per_pixel"))
        .or_else(|| request.pointer("/settings/job/options/samples"))
        .or_else(|| request.get("samples"))
        .and_then(Value::as_u64)
        .and_then(|n| u32::try_from(n).ok())
        .unwrap_or(1);
    let pilots = request
        .pointer("/settings/job/options/adaptive/pilot_samples")
        .or_else(|| request.pointer("/raytrace/adaptive/pilot_samples"))
        .or_else(|| request.pointer("/adaptive/pilot_samples"))
        .and_then(Value::as_u64)
        .and_then(|n| u32::try_from(n).ok())
        .unwrap_or(0);
    samples.checked_add(pilots).unwrap_or(0)
}

#[cfg(not(target_arch = "wasm32"))]
fn capture(app: &SolveApp, identity: &Identity, generation: u64) -> Result<Pending, String> {
    let mut owner = app.session.scratch();
    let request = identity.request.clone();
    let revision = identity.revision;
    let target = target(&request);
    let scope = solidcraft_engine::io::vfs::current_scope();
    let cancelled = Arc::new(AtomicBool::new(false));
    let stop = cancelled.clone();
    let (sender, receiver) = mpsc::channel();
    solidcraft_engine::kernel::kernel_thread("render-live-capture")
        .spawn(move || {
            let result = solidcraft_engine::io::vfs::scoped(scope, || {
                if stop.load(Ordering::Acquire) {
                    return Err(solidcraft_ui_text::tr("render.live.stopped").to_string());
                }
                let reply = owner.execute("render.start", &request).map_err(|e| e.to_string())?;
                let id = serde_json::from_value(reply["job"].clone()).map_err(|e| format!("{e}"))?;
                let handle = owner.render_job_handle(id).map_err(|e| e.to_string())?;
                if stop.load(Ordering::Acquire) {
                    let _ = handle.cancel();
                }
                // The UI must keep this handle until work_released, even if cancellation
                // raced submission. A terminal/cancelled status alone does not release work.
                Ok(OwnedJob { _owner: owner, handle, generation, revision, target })
            });
            let _ = sender.send(result);
        })
        .map_err(|e| e.to_string())?;
    Ok(Pending { receiver, cancelled, generation })
}

pub(crate) fn tick(app: &mut SolveApp, ctx: &Context) {
    let now = ctx.input(|input| input.time);
    tick_at(app, ctx, now);
}
fn tick_at(app: &mut SolveApp, ctx: &Context, now: f64) {
    let mut state = std::mem::take(&mut app.render_workspace.live);
    state.poll_capture();
    state.reap_draining();
    if state.final_request.is_some() {
        state.cancel_work();
        state.suspended = true;
        if state.pending.is_none() && state.draining.is_empty() {
            let pending = state.final_request.take();
            app.render_workspace.live = state;
            if let Some(pending) = pending
                && let Err(error) = ribbon::submit_deferred(app, pending)
            {
                app.set_status(error, true);
            }
        } else {
            ctx.request_repaint_after(std::time::Duration::from_millis(20));
            app.render_workspace.live = state;
        }
        return;
    }
    if !enabled(app) || !super::active(app) {
        state.reset();
        state.suspended = true;
        state.poll_capture();
        if state.pending.is_some() || !state.draining.is_empty() {
            ctx.request_repaint_after(std::time::Duration::from_millis(20));
        }
        app.render_workspace.live = state;
        return;
    }
    let request = request(app);
    let mut visibility = std::collections::hash_map::DefaultHasher::new();
    app.session.visibility.hidden_bodies.hash(&mut visibility);
    app.session.visibility.hidden_occurrences.hash(&mut visibility);
    let identity = Identity {
        document: Arc::downgrade(&app.session.doc),
        revision: app.session.revision,
        model: Arc::downgrade(&app.session.model.state()),
        moves: app.session.pending_moves.clone(),
        visibility: visibility.finish(),
        shot: app.render_workspace.view.name.clone(),
        request: request.as_ref().cloned().unwrap_or_else(|error| json!({"error":error})),
    };
    state.observe(identity, now);
    let suspended = app.render_workspace.view.live_paused
        || ribbon::busy(app)
        || app.render_workspace.view.result
        || app.render_workspace.view.compare.is_some()
        || app.file_load.busy()
        || app.file_picker.is_some()
        || app.saving.is_some()
        || app.tool.is_some()
        || app.dialog.is_some();
    if suspended {
        state.cancel_work();
        state.attempted = false;
        state.suspended = true;
        state.poll_capture();
        if state.pending.is_some() || !state.draining.is_empty() {
            ctx.request_repaint_after(std::time::Duration::from_millis(20));
        }
        app.render_workspace.live = state;
        return;
    }
    state.suspended = false;
    state.poll_capture();
    if let Err(error) = request {
        state.error = Some(error);
        state.attempted = true;
    }
    state.poll_pixels(ctx);
    if state.pending.is_none() && state.running.is_none() && state.draining.is_empty() && !state.attempted && now >= state.due {
        state.attempted = true;
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(identity) = &state.identity {
            match capture(app, identity, state.generation) {
                Ok(pending) => state.pending = Some(pending),
                Err(error) => state.error = Some(error),
            }
        }
    }
    if state.pending.is_some()
        || !state.draining.is_empty()
        || (state.running.is_some() && !state.picture.as_ref().is_some_and(|picture| picture.complete && picture.generation == state.generation))
        || !state.attempted
    {
        ctx.request_repaint_after(std::time::Duration::from_millis(20));
    }
    app.render_workspace.live = state;
}

pub(crate) fn set_paused(app: &mut SolveApp, paused: bool) {
    app.render_workspace.view.live_paused = paused;
    if paused {
        app.render_workspace.live.cancel_work();
    } else {
        app.render_workspace.live.identity = None;
        app.render_workspace.live.attempted = false;
    }
}
/// Preparation owns the same live-release priority as an explicit final. The
/// original pending/running owners stay here until their release acknowledgements.
pub(super) fn suspend_for_preparation(app: &mut SolveApp) {
    let state = &mut app.render_workspace.live;
    state.cancel_work();
    state.suspended = true;
    state.identity = None;
    state.attempted = false;
    state.poll_capture();
    state.reap_draining();
}
pub(super) fn preparation_ready(app: &SolveApp) -> bool {
    let state = &app.render_workspace.live;
    state.pending.is_none() && state.running.is_none() && state.draining.is_empty()
}
pub(super) fn defer_final(app: &mut SolveApp, request: ribbon::PendingFinal) -> bool {
    let state = &mut app.render_workspace.live;
    state.cancel_work();
    state.suspended = true;
    state.identity = None;
    state.attempted = false;
    state.reap_draining();
    if state.pending.is_some() || !state.draining.is_empty() {
        state.final_request = Some(request);
        true
    } else {
        false
    }
}
pub(super) fn final_pending(app: &SolveApp) -> bool {
    app.render_workspace.live.final_request.is_some()
}
pub(super) fn cancel_final(app: &mut SolveApp) -> bool {
    app.render_workspace.live.final_request.take().is_some()
}
fn picture_current(app: &SolveApp) -> bool {
    let state = &app.render_workspace.live;
    let Some(identity) = &state.identity else { return false };
    let Some(viewport) = app.viewport.rect.filter(|rect| rect.is_finite() && rect.is_positive()) else { return false };
    let [width, height] = size(app, super::shot::render_rect(app, viewport));
    let mut visibility = std::collections::hash_map::DefaultHasher::new();
    app.session.visibility.hidden_bodies.hash(&mut visibility);
    app.session.visibility.hidden_occurrences.hash(&mut visibility);
    // Navigation runs after the frame's lifecycle tick. Check geometry/camera again before
    // picking or painting feedback, so this frame cannot overlay a new camera on old pixels.
    enabled(app)
        && state.picture.as_ref().is_some_and(|picture| picture.generation == state.generation)
        && state.error.is_none()
        && Weak::ptr_eq(&identity.document, &Arc::downgrade(&app.session.doc))
        && identity.revision == app.session.revision
        && Weak::ptr_eq(&identity.model, &Arc::downgrade(&app.session.model.state()))
        && identity.moves == app.session.pending_moves
        && identity.visibility == visibility.finish()
        && identity.request.pointer("/settings/camera") == Some(&json!(app.cam))
        && identity.request.pointer("/settings/width").and_then(Value::as_u64) == Some(u64::from(width))
        && identity.request.pointer("/settings/height").and_then(Value::as_u64) == Some(u64::from(height))
}
pub(crate) fn picking_current(app: &SolveApp) -> bool {
    let state = &app.render_workspace.live;
    !enabled(app) || state.picture.is_none() || picture_current(app)
}
/// Terminal status precedes native cleanup. Live admission waits for that final work too.
pub(super) fn drain_final(app: &mut SolveApp, handle: JobHandle) {
    if !handle.work_released() {
        app.render_workspace.live.draining.push(handle);
    }
}
pub(crate) fn paint(app: &mut SolveApp, ui: &egui::Ui, rect: Rect) {
    if !enabled(app) {
        return;
    }
    if let Some(picture) = &app.render_workspace.live.picture {
        let aspect = f64::from(picture.size[0]) / f64::from(picture.size[1].max(1));
        if let Some(image) = super::shot::frame_rect(aspect, rect) {
            let source = super::displayed::Source {
                job: picture.job,
                scene_generation: picture.scene_generation,
                width: picture.size[0],
                height: picture.size[1],
                kind: if picture.complete { super::displayed::Kind::LiveFinal } else { super::displayed::Kind::LivePreview },
                frame_sequence: Some(picture.sequence),
                samples_per_pixel: Some(picture.samples),
                adaptive: picture.adaptive,
            };
            egui::Image::new(&picture.texture).paint_at(ui, image);
            // Image::paint_at rounds to physical pixels before emitting its image shape.
            app.render_workspace.displayed.record(ui.painter(), image.round_to_pixels(ui.pixels_per_point()), source);
        }
    }
}
pub(crate) fn has_picture(app: &SolveApp) -> bool {
    enabled(app) && app.render_workspace.live.picture.is_some()
}
/// Describe the image that is actually painted, including a retained previous capture.
/// The selected draft method cannot rename pixels produced by another executor.
pub(crate) fn source_note(app: &SolveApp) -> (String, &'static str) {
    let Some(picture) = app.render_workspace.live.picture.as_ref().filter(|_| has_picture(app)) else {
        return (app.tr("render.stage_preview_note").into(), "render.preview_lighting_help");
    };
    let method_matches = matches!(
        (picture.executor.as_str(), app.render_workspace.view.method),
        ("cpu-raytrace", ImageMethod::CpuRaytrace) | ("gpu-path", ImageMethod::GpuRaytrace)
    );
    let current = method_matches && picture_current(app);
    (source_text(app, picture.executor.as_str(), current), if current { "render.live.quality_help" } else { "render.live.source_outdated_help" })
}

pub(super) fn source_text(app: &SolveApp, executor: &str, current: bool) -> String {
    let method = match executor {
        "cpu-raytrace" => app.tr("render.method_cpu"),
        "gpu-path" => app.tr("render.method_gpu"),
        _ => executor,
    };
    solidcraft_ui_text::format(if current { "render.live.source_current" } else { "render.live.source_outdated" }, &[("method", method)])
}

pub(crate) fn controls(app: &mut SolveApp, ui: &mut egui::Ui) {
    if !enabled(app) {
        return;
    }
    let state = &app.render_workspace.live;
    let status = if app.render_workspace.view.live_paused {
        app.tr(if state.picture.as_ref().is_some_and(|picture| picture.generation != state.generation) {
            "render.live.paused_outdated"
        } else {
            "render.live.paused"
        })
        .to_string()
    } else if let Some(error) = &state.error {
        solidcraft_ui_text::format("render.live.refused", &[("reason", error)])
    } else if let Some(picture) = &state.picture {
        if picture.generation == state.generation && !state.suspended {
            if let Some(adaptive) = picture.adaptive {
                solidcraft_ui_text::format(
                    "render.live.adaptive",
                    &[
                        ("width", &picture.size[0].to_string()),
                        ("height", &picture.size[1].to_string()),
                        ("progress", &super::adaptive_preview_label(app, adaptive)),
                    ],
                )
            } else {
                let target = state.running.as_ref().map_or(picture.samples, |job| job.target);
                solidcraft_ui_text::format(
                    "render.live.samples",
                    &[
                        ("current", &picture.samples.to_string()),
                        ("target", &target.to_string()),
                        ("width", &picture.size[0].to_string()),
                        ("height", &picture.size[1].to_string()),
                    ],
                )
            }
        } else {
            app.tr("render.live.updating").into()
        }
    } else {
        app.tr("render.live.updating").into()
    };
    let paused = app.render_workspace.view.live_paused;
    let label = app.tr(if paused { "render.live.resume" } else { "render.live.pause" }).to_string();
    let response = ui_kit::button(ui, &label, ui_kit::Button::Normal);
    crate::scenario::publish_target("render:live:pause", response.rect);
    if response.clicked() {
        let _ = app.run("view.render_live", json!({"paused":!paused}));
    }
    let max_width = ui.available_width().min(solidcraft_theme::Tokens::get().control_height * 10.0);
    let mut help = format!("{status}\n\n{}", app.tr("render.live.quality_help"));
    if app.render_workspace.live.picture.as_ref().is_some_and(|picture| picture.adaptive.is_some()) {
        help.push_str("\n\n");
        help.push_str(app.tr("render.adaptive.preview_help"));
    }
    let note = ui_kit::StageBar::note_bounded(ui, &status, max_width).backing_on_hover_text(help);
    crate::scenario::publish_target("render:live:status", note.rect);
    ui_kit::StageBar::separator(ui);
}

pub(super) fn inspect(app: &SolveApp) -> Value {
    let state = &app.render_workspace.live;
    let is_current = picture_current(app);
    json!({"enabled":enabled(app),"paused":app.render_workspace.view.live_paused,"generation":state.generation,"draining":state.draining.len(),"final_pending":state.final_request.is_some(),"suspended":state.suspended,"pending":state.pending.is_some(),"running":state.running.is_some(),"error":state.error,
        "target":state.running.as_ref().map(|job|job.target).or_else(||state.identity.as_ref().map(|identity|target(&identity.request))),
        "current":state.picture.as_ref().map(|picture|picture.samples),"is_current":is_current,"stale":state.picture.is_some() && !is_current,
        "job":state.running.as_ref().map(|job|job.handle.id()).or_else(||state.picture.as_ref().map(|picture|picture.job)),"samples":state.picture.as_ref().map(|picture|picture.samples),
        "sequence":state.picture.as_ref().map(|picture|picture.sequence),"request":state.identity.as_ref().map(|identity| &identity.request),
        "adaptive":state.picture.as_ref().and_then(|picture|picture.adaptive),
        "hash":state.running.as_ref().and_then(|job| job.handle.frame().ok().flatten()).map(|frame| frame.info.png_sha256.clone()),"complete":is_current && state.picture.as_ref().is_some_and(|picture|picture.complete)})
}
#[cfg(test)]
pub(super) fn advance(app: &mut SolveApp, ctx: &Context, now: f64) {
    tick_at(app, ctx, now);
}
