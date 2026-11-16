//! The window in a VR headset (#2172, docs/vr.md): the same flat window, floating in front of
//! the user, with the design in its view shown in real depth. Mouse and keyboard work as on the
//! desktop, because the window on the desktop is still the one that takes them.
//!
//! How a frame is put together:
//! - The headset's runtime shows two layers. Behind: each eye's picture of the room, which holds
//!   the model and a backdrop. In front: a flat picture of the window, see-through where the
//!   design view is, so the model behind it shows there.
//! - The window's picture is a second copy of the frame's UI ([`UiCopy`], a frame compositor
//!   around whichever the window already has), drawn with the design view cut out
//!   (`solidcraft_viewport_egui::Cutout`). The window on the desktop is drawn as ever.
//! - The model is placed so that the design view is a pane of glass onto it
//!   (`solidcraft_xr::view::place_model`), and drawn once per eye from the scenes the window
//!   already uploaded (`Renderer::draw_second_view`), only within the pane as that eye sees it.
//!
//! VR is chosen at start (`--vr`, `SOLIDCRAFT_VR=1` or Preferences › Graphics), because the
//! runtime creates the Vulkan device the window then draws with. Off, none of this runs.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use eframe::egui_wgpu::{self, wgpu};
use solidcraft_ui_egui::SolveApp;
use solidcraft_ui_egui::stereo::Source;
use solidcraft_viewport_gpu::view::{Renderer, ViewTarget};
use solidcraft_xr::view::{self, CameraFrame, Eye, Pane, Screen};

/// The environment variable that asks for the VR view for one run (`1`, `true`, `on`, `yes`).
pub const VR_ENV: &str = "SOLIDCRAFT_VR";

/// Whether a start should try the VR view: the command line's `--vr`, else `SOLIDCRAFT_VR`, else
/// `graphics.json`'s `vr` (Preferences › Graphics). The environment can also say no (`0`, `off`)
/// to a saved yes. Anything else it holds is ignored, and the saved choice stands.
pub fn wanted(flag: bool, env: Option<&str>, saved: Option<&serde_json::Value>) -> bool {
    if flag {
        return true;
    }
    match env.map(|v| v.trim().to_ascii_lowercase()).as_deref() {
        Some("1" | "true" | "on" | "yes") => return true,
        Some("0" | "false" | "off" | "no") => return false,
        _ => {}
    }
    saved.and_then(|v| v.get("vr")).and_then(serde_json::Value::as_bool).unwrap_or(false)
}

/// Where the window is in the room, and how far behind its glass the model's middle is: the
/// defaults, each replaceable for a run by an environment variable while the sizes are tried out
/// on real headsets.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layout {
    /// The window's width (metres).
    pub width: f64,
    /// From the seated position to the window (metres).
    pub distance: f64,
    /// The window's middle below eye level (metres).
    pub drop: f64,
    /// The camera's target behind the window's plane (metres; negative: in front of it).
    pub depth: f64,
}

impl Default for Layout {
    /// A window 1.6 m wide, 1.2 m away and a little below eye level, the model's middle in its plane.
    fn default() -> Self {
        Layout { width: 1.6, distance: 1.2, drop: 0.1, depth: 0.0 }
    }
}

impl Layout {
    /// The defaults with `SOLIDCRAFT_VR_WIDTH`, `_DISTANCE`, `_DROP` and `_DEPTH` (metres) where
    /// `read` has them as finite numbers within reach; a value that is not is left at its default.
    pub fn from_env(read: impl Fn(&str) -> Option<String>) -> Layout {
        let d = Layout::default();
        let number = |name: &str, range: std::ops::RangeInclusive<f64>, fallback: f64| {
            read(name).and_then(|v| v.trim().parse::<f64>().ok()).filter(|v| v.is_finite() && range.contains(v)).unwrap_or(fallback)
        };
        Layout {
            width: number("SOLIDCRAFT_VR_WIDTH", view::SCREEN_WIDTH, d.width),
            distance: number("SOLIDCRAFT_VR_DISTANCE", view::SCREEN_DISTANCE, d.distance),
            drop: number("SOLIDCRAFT_VR_DROP", -2.0..=2.0, d.drop),
            depth: number("SOLIDCRAFT_VR_DEPTH", -5.0..=5.0, d.depth),
        }
    }
}

/// What the VR view is doing, for Preferences › Graphics and the status line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Status {
    /// The session is open and the runtime is not showing it yet (the headset is not worn).
    Waiting,
    /// Frames are shown in the headset.
    Running,
    /// The session is over for this run.
    Ended,
    /// It never started, or a frame failed: why.
    Failed(Reason),
}

/// Why there is no VR view, kept as what happened rather than as words: the words are looked up
/// when they are shown, in the language the settings then name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Reason {
    /// Safe mode, the software renderer, or a start that is recovering from a failed one.
    Software,
    NoLoader,
    NoRuntime,
    NoHeadset,
    NoVulkan,
    /// Anything else, in the runtime's or the device's own words.
    Other(String),
}

impl Reason {
    /// What `error` is to the user.
    pub fn of(error: &solidcraft_xr::Error) -> Reason {
        use solidcraft_xr::Error;
        match error {
            Error::NoLoader(_) => Reason::NoLoader,
            Error::NoRuntime(_) => Reason::NoRuntime,
            Error::NoHeadset(_) => Reason::NoHeadset,
            Error::NoVulkan => Reason::NoVulkan,
            other => Reason::Other(other.to_string()),
        }
    }

    /// The reason in the user's language: what to start or plug in where the user can do
    /// something about it, the runtime's own words otherwise.
    pub fn text(&self) -> String {
        use solidcraft_ui_text::tr;
        match self {
            Reason::Software => tr("vr.reason_software").to_string(),
            Reason::NoLoader => tr("vr.reason_no_loader").to_string(),
            Reason::NoRuntime => tr("vr.reason_no_runtime").to_string(),
            Reason::NoHeadset => tr("vr.reason_no_headset").to_string(),
            Reason::NoVulkan => tr("vr.reason_no_vulkan").to_string(),
            Reason::Other(words) => words.clone(),
        }
    }
}

/// Connect to the VR runtime and open the session, changing `options` so that the window opens
/// on the device the runtime made: the same features and limits the window would have asked of
/// its own device, on Vulkan, and without waiting for the monitor's refresh (the headset's
/// frames set the pace; waiting for both would halve it).
///
/// On an error `options` is as it was, and the app starts as it always has.
pub fn open(options: &mut egui_wgpu::WgpuConfiguration) -> Result<Vr, solidcraft_xr::Error> {
    let egui_wgpu::WgpuSetup::CreateNew(setup) = &options.wgpu_setup else {
        return Err(solidcraft_xr::Error::Unsupported("the window already has a graphics device".into()));
    };
    let describe = setup.device_descriptor.clone();
    let runtime = solidcraft_xr::Runtime::connect()?;
    let (name, headset) = (runtime.name.clone(), runtime.headset.clone());
    let (session, gpu) = runtime.open(&|adapter| describe(adapter))?;
    options.wgpu_setup = egui_wgpu::WgpuSetup::Existing(egui_wgpu::WgpuSetupExisting {
        instance: gpu.instance,
        adapter: gpu.adapter,
        device: gpu.device,
        queue: gpu.queue,
    });
    options.surface.present_mode = wgpu::PresentMode::AutoNoVsync;
    Ok(Vr {
        session: Some(session),
        gpu: None,
        runtime: name,
        headset,
        layout: Layout::from_env(|name| std::env::var(name).ok()),
        status: Status::Waiting,
        ran: false,
        told: None,
    })
}

/// The VR view of one run.
pub struct Vr {
    /// The session, until it ends. Dropped before the window's device (`App` goes first).
    session: Option<solidcraft_xr::Session>,
    /// What draws the session's pictures, made once the viewport's pipelines are in use.
    gpu: Option<Gpu>,
    runtime: String,
    headset: String,
    layout: Layout,
    status: Status,
    /// Frames were shown in the headset at some point of this run.
    ran: bool,
    /// The status last put in Preferences and on the status line.
    told: Option<Status>,
}

impl Vr {
    /// A VR view that did not start: it only says why, in Preferences and once on the status line.
    pub fn failed(reason: Reason) -> Vr {
        Vr {
            session: None,
            gpu: None,
            runtime: String::new(),
            headset: String::new(),
            layout: Layout::default(),
            status: Status::Failed(reason),
            ran: false,
            told: None,
        }
    }

    /// What the VR view is doing.
    #[cfg(test)]
    pub fn status(&self) -> &Status {
        &self.status
    }

    /// The status in the user's language.
    fn status_text(&self) -> String {
        use solidcraft_ui_text::{format, tr};
        let names = [("runtime", self.runtime.as_str()), ("headset", self.headset.as_str())];
        match &self.status {
            Status::Waiting => format("vr.status_waiting", &names),
            Status::Running => format("vr.status_running", &names),
            Status::Ended => tr("vr.status_ended").to_string(),
            Status::Failed(reason) if self.ran => format("vr.status_failed_draw", &[("reason", &reason.text())]),
            Status::Failed(reason) => format("vr.status_failed", &[("reason", &reason.text())]),
        }
    }

    /// One frame of the window, after its UI: show the last frame's UI and this frame's design
    /// view in the headset, at the headset's pace. Never fails: a session that does ends, says
    /// so, and leaves the window drawing as it always has.
    pub fn frame(&mut self, app: &mut SolveApp, ctx: &egui::Context, rs: &egui_wgpu::RenderState) {
        if self.session.is_some() {
            if let Err(error) = self.step(app, ctx, rs) {
                log::warn!("VR: {error}");
                solidcraft_engine::note!("SolidCraft VR: stopped: {error}");
                self.status = Status::Failed(Reason::of(&error));
                self.close(app, rs);
            }
        } else {
            app.viewport.stereo.on = false;
        }
        if self.told.as_ref() != Some(&self.status) {
            // In the language the settings name: the frame's own UI has let go of it by now.
            let _language = app.prepare_text(ctx);
            let text = self.status_text();
            // A failure and the end are worth the status line; running and waiting only show in
            // Preferences, where the setting is.
            if matches!(self.status, Status::Failed(_) | Status::Ended) {
                app.set_status(text.clone(), matches!(self.status, Status::Failed(_)));
            }
            app.services.vr_status = Some(text);
            self.told = Some(self.status.clone());
        }
    }

    /// Let go of the session and everything made for it; the window draws on as ever.
    fn close(&mut self, app: &mut SolveApp, rs: &egui_wgpu::RenderState) {
        app.viewport.stereo.on = false;
        if let Some(gpu) = self.gpu.take() {
            gpu.ui.release(&rs.renderer);
        }
        self.session = None;
        if self.status == Status::Running || self.status == Status::Waiting {
            self.status = Status::Ended;
        }
    }

    /// The app is closing: the session goes before the window's device does.
    pub fn shut_down(&mut self) {
        self.gpu = None;
        self.session = None;
    }

    /// One frame of the session, or why it cannot go on.
    fn step(&mut self, app: &mut SolveApp, ctx: &egui::Context, rs: &egui_wgpu::RenderState) -> Result<(), solidcraft_xr::Error> {
        let Some(session) = self.session.as_mut() else { return Ok(()) };
        // Not before the viewport's pipelines are in use: they compile on a thread of their own,
        // and the runtime shares the device's queue from its first frame.
        if app.viewport.gpu.is_none() {
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
            return Ok(());
        }
        let gpu = self.gpu.get_or_insert_with(|| Gpu::new(rs, session.format()));
        match session.poll()? {
            solidcraft_xr::Status::Closed => {
                solidcraft_engine::note!("SolidCraft VR: the session ended");
                self.status = Status::Ended;
                self.close(app, rs);
                return Ok(());
            }
            solidcraft_xr::Status::Waiting => {
                // Nothing is shown: no second copy of the UI, no eyes; ask again soon.
                app.viewport.stereo.on = false;
                gpu.ui.release(&rs.renderer);
                self.status = Status::Waiting;
                ctx.request_repaint_after(std::time::Duration::from_millis(100));
                return Ok(());
            }
            solidcraft_xr::Status::Running => {}
        }
        self.status = Status::Running;
        self.ran = true;
        app.viewport.stereo.on = true;
        gpu.ui.claim(&rs.renderer);
        let source = app.viewport.stereo.take();
        // The pointer: on the model where it points at the model (drawn for each eye at the
        // model's depth), else on the window, drawn into the UI.
        if !source.as_ref().is_some_and(|s| s.pointer.is_some()) {
            paint_pointer(ctx);
        }
        // Waits for the headset's next frame: this is what paces the window while VR runs.
        let Some(frame) = session.begin_frame()? else { return Ok(()) };
        let window_points = ctx.viewport_rect().size();
        let scene = Scene {
            source: source.as_ref(),
            window_points: [window_points.x, window_points.y],
            pointer: ctx.pointer_latest_pos().map(|p| [p.x, p.y]),
            in_room: app.preferences.vr_model_in_room,
            layout: &self.layout,
        };
        let drawn = if frame.should_render { gpu.draw(session, rs, &frame, &scene) } else { Ok(Drawn::default()) };
        // A begun frame is ended whatever happened in it.
        let (eyes, window) = drawn.as_ref().map_or((false, None), |d| (d.eyes, d.window));
        let ended = session.end_frame(&frame, eyes, window);
        ctx.request_repaint();
        drawn.and(ended)
    }
}

/// The mouse pointer, drawn into the UI: the headset shows the window's picture, not the system's
/// pointer over it.
fn paint_pointer(ctx: &egui::Context) {
    let Some(p) = ctx.pointer_latest_pos() else { return };
    let t = solidcraft_theme::Tokens::get();
    let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Debug, egui::Id::unique("sc_vr_pointer")));
    // An arrow, tip at `p`.
    let points: Vec<egui::Pos2> = [(0.0, 0.0), (0.0, 17.0), (4.5, 13.0), (7.5, 20.0), (10.5, 18.8), (7.5, 12.0), (13.0, 12.0)]
        .iter()
        .map(|(x, y)| p + egui::vec2(*x, *y))
        .collect();
    painter.add(egui::Shape::convex_polygon(points.clone(), t.form_primary, egui::Stroke::NONE));
    painter.add(egui::Shape::closed_line(points, egui::Stroke::new(1.5, t.app_bar)));
}

/// What a frame shows in the headset, from the window's side.
#[derive(Clone, Copy)]
struct Scene<'a> {
    /// This frame's design view, if one was drawn.
    source: Option<&'a Source>,
    /// The window's size (points).
    window_points: [f32; 2],
    /// Where the mouse pointer is in the window (points), if it is over it.
    pointer: Option<[f32; 2]>,
    /// The model fills the room around the window, instead of showing through the design view.
    in_room: bool,
    layout: &'a Layout,
}

/// What a frame drew for the runtime to show.
#[derive(Default)]
struct Drawn {
    /// Both eyes' pictures.
    eyes: bool,
    /// The window's picture, and where it is in the room.
    window: Option<solidcraft_xr::WindowLayer>,
}

/// The second copy of the frame's UI, design views cut out, as the frame compositor that draws it
/// (around the compositor the window already has, which still draws the window).
struct UiCopy {
    device: wgpu::Device,
    format: wgpu::TextureFormat,
    /// The compositor this one stands in front of (the view's blur, when on).
    inner: Mutex<Option<Arc<dyn egui_wgpu::FrameCompositor>>>,
    /// The frame being painted is the main window's.
    root: AtomicBool,
    state: Mutex<UiState>,
}

/// What a [`UiCopy`] has on the GPU. Everything here depends on the window's size in pixels and
/// on nothing else: the format is the window's, fixed for the run.
#[derive(Default)]
struct UiState {
    /// The copy's texture, at the window's size in pixels.
    target: Option<(wgpu::TextureView, [u32; 2])>,
    /// A copy was encoded for the frame being painted; shown once that frame is submitted.
    encoded: bool,
    /// The copy on the GPU is a whole frame's UI.
    ready: bool,
    /// The GPU refused a texture of this size: it is asked again once this many frames have
    /// passed (the memory may be there again by then), not at every frame.
    refused: Option<([u32; 2], u32)>,
}

/// Frames (or tries) to let pass before the GPU is asked again for a picture it refused.
const RETRY_AFTER: u32 = 240;

impl UiCopy {
    /// A copy for a window drawn in `format` on `device`, with nothing in front of it yet.
    fn new(device: wgpu::Device, format: wgpu::TextureFormat) -> Arc<UiCopy> {
        Arc::new(UiCopy { device, format, inner: Mutex::new(None), root: AtomicBool::new(true), state: Mutex::new(UiState::default()) })
    }

    /// Whether `other` is this very compositor.
    fn is(self: &Arc<Self>, other: &Arc<dyn egui_wgpu::FrameCompositor>) -> bool {
        std::ptr::addr_eq(Arc::as_ptr(self), Arc::as_ptr(other))
    }

    /// Stand in front of whatever compositor the frame's UI left in the renderer (the view's blur
    /// puts its own there each frame it is on, and takes it out when it goes off).
    fn claim(self: &Arc<Self>, renderer: &Arc<egui::mutex::RwLock<egui_wgpu::Renderer>>) {
        let mut renderer = renderer.write();
        let current = renderer.frame_compositor.take();
        if !current.as_ref().is_some_and(|c| self.is(c))
            && let Ok(mut inner) = self.inner.lock()
        {
            *inner = current;
        }
        let me: Arc<dyn egui_wgpu::FrameCompositor> = self.clone();
        renderer.frame_compositor = Some(me);
    }

    /// Step aside: the renderer has the compositor it had without this one, and the copy is no
    /// longer shown.
    fn release(self: &Arc<Self>, renderer: &Arc<egui::mutex::RwLock<egui_wgpu::Renderer>>) {
        let mut renderer = renderer.write();
        if renderer.frame_compositor.as_ref().is_some_and(|c| self.is(c)) {
            renderer.frame_compositor = self.inner.lock().ok().and_then(|mut inner| inner.take());
        }
        if let Ok(mut state) = self.state.lock() {
            *state = UiState::default();
        }
    }

    /// The compositor this one stands in front of, if any.
    fn inner(&self) -> Option<Arc<dyn egui_wgpu::FrameCompositor>> {
        self.inner.lock().ok().and_then(|inner| inner.clone())
    }

    /// The last whole copy: its texture and size in pixels.
    fn picture(&self) -> Option<(wgpu::TextureView, [u32; 2])> {
        let state = self.state.lock().ok()?;
        state.target.clone().filter(|_| state.ready)
    }

    /// Draw `jobs` into the copy, design views cut out.
    fn copy(
        &self,
        renderer: &egui_wgpu::Renderer,
        encoder: &mut wgpu::CommandEncoder,
        jobs: &[egui::ClippedPrimitive],
        screen: &egui_wgpu::ScreenDescriptor,
    ) {
        let Some(cutout) = renderer.callback_resources.get::<solidcraft_viewport_egui::Cutout>() else { return };
        let Ok(mut state) = self.state.lock() else { return };
        let size = screen.size_in_pixels;
        if size.contains(&0) {
            *state = UiState::default();
            return;
        }
        if state.target.as_ref().is_none_or(|(_, has)| *has != size) {
            // A size the GPU refused is not asked for at every frame; another size is, at once.
            let waiting = state.refused.filter(|(refused, wait)| *refused == size && *wait > 0).map(|(refused, wait)| (refused, wait - 1));
            *state = UiState { refused: waiting, ..UiState::default() };
            if waiting.is_some() {
                return;
            }
            let (texture, failed) = egui_wgpu::capture_errors(&self.device, || {
                self.device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("sc_vr_ui"),
                    size: wgpu::Extent3d { width: size[0], height: size[1], depth_or_array_layers: 1 },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: self.format,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                })
            });
            if let Some(error) = failed {
                log::warn!("VR: no picture of the window: the GPU refused a {} × {} texture: {error}", size[0], size[1]);
                state.refused = Some((size, RETRY_AFTER));
                return;
            }
            state.target = Some((texture.create_view(&wgpu::TextureViewDescriptor::default()), size));
        }
        let Some((view, _)) = &state.target else { return };
        {
            let mut pass = encoder
                .begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("sc_vr_ui"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT), store: wgpu::StoreOp::Store },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                })
                .forget_lifetime();
            cutout.set(true);
            renderer.render(&mut pass, jobs, screen);
            cutout.set(false);
        }
        state.encoded = true;
    }
}

impl egui_wgpu::FrameCompositor for UiCopy {
    /// The window about to be painted: only the main one is copied.
    fn begin_viewport(&self, viewport: u64) {
        self.root.store(viewport == egui::ViewportId::ROOT.0.value(), Ordering::Relaxed);
        if let Some(inner) = self.inner() {
            inner.begin_viewport(viewport);
        }
    }

    /// The target's size, passed on: the copy takes it from `render`.
    fn screen(&self, screen: &egui_wgpu::ScreenDescriptor, resources: &mut egui_wgpu::CallbackResources) {
        if let Some(inner) = self.inner() {
            inner.screen(screen, resources);
        }
    }

    /// The window has no size, or the device is lost: the copy goes.
    fn suspend(&self) {
        if let Ok(mut state) = self.state.lock() {
            *state = UiState::default();
        }
        if let Some(inner) = self.inner() {
            inner.suspend();
        }
    }

    /// The frame: its copy for the headset first, then the window as it would be drawn without
    /// this compositor.
    fn render(
        &self,
        renderer: &egui_wgpu::Renderer,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        jobs: &[egui::ClippedPrimitive],
        screen: &egui_wgpu::ScreenDescriptor,
        clear: [f32; 4],
    ) -> bool {
        if self.root.load(Ordering::Relaxed) {
            self.copy(renderer, encoder, jobs, screen);
        }
        // The window itself: the compositor it had, or (false) the ordinary pass.
        self.inner().is_some_and(|inner| inner.render(renderer, encoder, target, jobs, screen, clear))
    }

    /// The frame's commands are with the GPU: a copy encoded in it can be shown from now on.
    fn submitted(&self) {
        if let Ok(mut state) = self.state.lock()
            && std::mem::take(&mut state.encoded)
        {
            state.ready = true;
        }
        if let Some(inner) = self.inner() {
            inner.submitted();
        }
    }
}

/// The two pictures a frame draws for the runtime: an eye's (the model over a backdrop) and the
/// window's (the UI copy). Both read a picture in the window's format, where colours are stored
/// as the screen shows them, and write the runtime's sRGB pictures, which take linear light.
const SHADER: &str = r#"
struct U {
    top: vec4<f32>,
    bottom: vec4<f32>,
    right: vec4<f32>,
    up: vec4<f32>,
    back: vec4<f32>,
    // The tangents of the eye's field of view: left, right, up, down.
    tangents: vec4<f32>,
    // x: the source's colours are stored as the screen shows them (1) or as linear light (0).
    // y: there is a model picture (1) or only the backdrop (0).
    // z: the model shows only through the pane (1) or everywhere in the room (0).
    flags: vec4<f32>,
    // The eye in the room.
    eye: vec4<f32>,
    // The pane the model shows through: its middle, then its unit directions along its width and
    // height with half that width and height in w.
    pane: vec4<f32>,
    pane_right: vec4<f32>,
    pane_up: vec4<f32>,
};
@group(0) @binding(0) var src: texture_2d<f32>;
@group(0) @binding(1) var smp: sampler;
@group(0) @binding(2) var<uniform> u: U;

struct O { @builtin(position) pos: vec4<f32>, @location(0) uv: vec2<f32> };

@vertex
fn vs(@builtin(vertex_index) i: u32) -> O {
    let x = f32((i << 1u) & 2u);
    let y = f32(i & 2u);
    var o: O;
    o.pos = vec4<f32>(x * 2.0 - 1.0, 1.0 - y * 2.0, 0.0, 1.0);
    o.uv = vec2<f32>(x, y);
    return o;
}

fn to_linear(c: vec3<f32>) -> vec3<f32> {
    let lo = c / 12.92;
    let hi = pow((max(c, vec3<f32>(0.0)) + 0.055) / 1.055, vec3<f32>(2.4));
    return select(hi, lo, c <= vec3<f32>(0.04045));
}

// An eye's picture: the model's (colours times alpha, transparent where it is not) over a
// backdrop that is fixed in the room, the view's two background colours from below to above.
@fragment
fn fs_eye(i: O) -> @location(0) vec4<f32> {
    let tx = mix(u.tangents.x, u.tangents.y, i.uv.x);
    let ty = mix(u.tangents.z, u.tangents.w, i.uv.y);
    let ray = normalize(u.right.xyz * tx + u.up.xyz * ty - u.back.xyz);
    let backdrop = mix(u.bottom.rgb, u.top.rgb, smoothstep(-0.6, 0.6, ray.y));
    // Where this pixel's ray meets the pane's plane, in half-widths and half-heights from the
    // pane's middle: within 1 both ways is through the pane. The edge is a pixel soft.
    let normal = cross(u.pane_right.xyz, u.pane_up.xyz);
    let toward = dot(ray, normal);
    let along = dot(u.pane.xyz - u.eye.xyz, normal) / select(toward, 1.0, abs(toward) < 1e-6);
    let hit = u.eye.xyz + ray * along - u.pane.xyz;
    let edge = max(abs(dot(hit, u.pane_right.xyz)) / u.pane_right.w, abs(dot(hit, u.pane_up.xyz)) / u.pane_up.w);
    let soft = max(fwidth(edge), 1e-5);
    let through = select(0.0, 1.0 - smoothstep(1.0 - soft, 1.0, edge), abs(toward) >= 1e-6 && along > 0.0);
    var model = vec4<f32>(0.0);
    if (u.flags.y > 0.5) {
        model = textureSampleLevel(src, smp, i.uv, 0.0) * mix(1.0, through, u.flags.z);
    }
    if (u.flags.x > 0.5) {
        return vec4<f32>(to_linear(model.rgb + backdrop * (1.0 - model.a)), 1.0);
    }
    return vec4<f32>(model.rgb + to_linear(backdrop) * (1.0 - model.a), 1.0);
}

// The mouse pointer on the model: a few flat-coloured triangles, placed by the app for this eye.
struct P { @builtin(position) pos: vec4<f32>, @location(0) colour: vec4<f32> };

@vertex
fn vs_pointer(@location(0) at: vec2<f32>, @location(1) colour: vec4<f32>) -> P {
    var o: P;
    o.pos = vec4<f32>(at, 0.0, 1.0);
    o.colour = colour;
    return o;
}

@fragment
fn fs_pointer(i: P) -> @location(0) vec4<f32> {
    return i.colour;
}

// The window's picture: the UI as it is, its alpha kept, so the runtime blends it over the eyes'
// pictures and the design view's cut-out shows the model behind.
@fragment
fn fs_window(i: O) -> @location(0) vec4<f32> {
    let c = textureSampleLevel(src, smp, i.uv, 0.0);
    if (u.flags.x > 0.5 && c.a > 0.0) {
        return vec4<f32>(to_linear(clamp(c.rgb / c.a, vec3<f32>(0.0), vec3<f32>(1.0))) * c.a, c.a);
    }
    return c;
}
"#;

/// The uniform's size in bytes: eleven `vec4<f32>`.
const UNIFORM_SIZE: u64 = 11 * 16;

/// The mouse pointer's outline in points from its tip (x right, y down): the arrow the window
/// draws ([`paint_pointer`]).
const POINTER: [[f64; 2]; 7] = [[0.0, 0.0], [0.0, 17.0], [4.5, 13.0], [7.5, 20.0], [10.5, 18.8], [7.5, 12.0], [13.0, 12.0]];

/// A pointer vertex: where it is in the picture (2 × f32), then its colour (4 × f32).
const POINTER_VERTEX: u64 = 24;

/// The pointer's vertices: its edge, then its fill, each a fan of triangles from the tip.
const POINTER_VERTICES: u64 = 2 * 3 * (POINTER.len() as u64 - 2);

/// What draws the session's pictures on the window's device.
struct Gpu {
    ui: Arc<UiCopy>,
    eye: wgpu::RenderPipeline,
    window: wgpu::RenderPipeline,
    /// The mouse pointer on the model, over an eye's picture.
    pointer: wgpu::RenderPipeline,
    pointer_vertices: wgpu::Buffer,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    uniform: wgpu::Buffer,
    /// A stand-in to bind while there is no model picture.
    blank: wgpu::TextureView,
    /// The model as one eye sees it, drawn into for each eye in turn. Made by the renderer for
    /// the eye's size, and again when that changes.
    model: Option<ViewTarget>,
    /// The GPU refused the model's picture: it is asked again after this many more tries (the
    /// memory may be there again by then), not for both eyes of every frame.
    model_wait: u32,
    /// The window's format stores colours as the screen shows them (not an sRGB format).
    encoded: bool,
}

impl Gpu {
    /// The pipelines for pictures in the session's `format`, and the UI copy with the cut-out it
    /// draws through, in `rs`'s callback resources. Pictures of the model off the window's
    /// thread (thumbnails, file previews) are drawn by the CPU from here on: the runtime shares
    /// the device's queue during a frame, and Vulkan lets only one thread at a time use it.
    fn new(rs: &egui_wgpu::RenderState, format: wgpu::TextureFormat) -> Gpu {
        let device = &rs.device;
        rs.renderer.write().callback_resources.insert(solidcraft_viewport_egui::Cutout::new(device, rs.target_format));
        solidcraft_ui_egui::gpu::install_cpu_picture_renderer();
        let module =
            device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("sc_vr"), source: wgpu::ShaderSource::Wgsl(SHADER.into()) });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("sc_vr"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: wgpu::BufferSize::new(UNIFORM_SIZE),
                    },
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("sc_vr"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = |label: &str, fragment: &str| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState { module: &module, entry_point: Some("vs"), buffers: &[], compilation_options: Default::default() },
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some(fragment),
                    targets: &[Some(wgpu::ColorTargetState { format, blend: None, write_mask: wgpu::ColorWrites::ALL })],
                    compilation_options: Default::default(),
                }),
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                multiview_mask: None,
                cache: None,
            })
        };
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("sc_vr"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("sc_vr"),
            size: UNIFORM_SIZE,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let blank = device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("sc_vr_blank"),
                size: wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            })
            .create_view(&wgpu::TextureViewDescriptor::default());
        const POINTER_LAYOUT: [wgpu::VertexAttribute; 2] = wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x4];
        let pointer = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("sc_vr_pointer"),
            layout: None,
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs_pointer"),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: POINTER_VERTEX,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &POINTER_LAYOUT,
                })],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs_pointer"),
                targets: &[Some(wgpu::ColorTargetState { format, blend: None, write_mask: wgpu::ColorWrites::ALL })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        let pointer_vertices = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("sc_vr_pointer"),
            size: POINTER_VERTICES * POINTER_VERTEX,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Gpu {
            ui: UiCopy::new(device.clone(), rs.target_format),
            eye: pipeline("sc_vr_eye", "fs_eye"),
            window: pipeline("sc_vr_window", "fs_window"),
            pointer,
            pointer_vertices,
            layout,
            sampler,
            uniform,
            blank,
            model: None,
            model_wait: 0,
            encoded: !rs.target_format.is_srgb(),
        }
    }

    /// Draw `source` into `target` with `pipeline` and the uniform as it is, within `size` pixels
    /// from the target's top left corner, then the pointer from [`Gpu::pointer_vertices`] over it
    /// when `pointer`, and submit it.
    fn blit(
        &self,
        rs: &egui_wgpu::RenderState,
        pipeline: &wgpu::RenderPipeline,
        source: &wgpu::TextureView,
        target: &wgpu::TextureView,
        size: [u32; 2],
        pointer: bool,
    ) {
        let bind = rs.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("sc_vr"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(source) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.sampler) },
                wgpu::BindGroupEntry { binding: 2, resource: self.uniform.as_entire_binding() },
            ],
        });
        let mut encoder = rs.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("sc_vr") });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("sc_vr"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT), store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_viewport(0.0, 0.0, size[0] as f32, size[1] as f32, 0.0, 1.0);
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, &bind, &[]);
            pass.draw(0..3, 0..1);
            if pointer {
                pass.set_pipeline(&self.pointer);
                pass.set_vertex_buffer(0, self.pointer_vertices.slice(..));
                pass.draw(0..POINTER_VERTICES as u32, 0..1);
            }
        }
        rs.queue.submit([encoder.finish()]);
    }

    /// Draw what the runtime shows this frame: the window's picture from the last UI copy, and
    /// each eye's picture of the room with the scene's design view placed at the window. Each
    /// picture is submitted before this returns.
    fn draw(
        &mut self,
        session: &mut solidcraft_xr::Session,
        rs: &egui_wgpu::RenderState,
        frame: &solidcraft_xr::Frame,
        scene: &Scene<'_>,
    ) -> Result<Drawn, solidcraft_xr::Error> {
        let Scene { source, window_points, pointer, in_room, layout } = *scene;
        let mut drawn = Drawn::default();
        let ui = self.ui.picture();
        let aspect = match &ui {
            Some((_, [w, h])) => f64::from(*w) / f64::from((*h).max(1)),
            None => f64::from(window_points[0]) / f64::from(window_points[1].max(1.0)),
        };
        let screen = Screen::in_front(layout.width, aspect, layout.distance, layout.drop);
        if let Some((view, size)) = &ui {
            let picture = session.window_picture(*size)?;
            rs.queue.write_buffer(&self.uniform, 0, &uniform_bytes(&Uniform { encoded: self.encoded, ..Uniform::default() }));
            self.blit(rs, &self.window, view, &picture.view, *size, false);
            drawn.window = Some(solidcraft_xr::WindowLayer { pose: screen.pose(), size: [screen.width, screen.height], pixels: *size });
        }
        let Some(eyes) = &frame.eyes else { return Ok(drawn) };
        let pane = source.and_then(|s| Pane::of([s.rect.min.x, s.rect.min.y], [s.rect.max.x, s.rect.max.y], window_points));
        let placement = source.zip(pane.as_ref()).and_then(|(s, pane)| view::place_model(&camera_frame(&s.camera), pane, &screen, layout.depth));
        // The model shows through the design view only, unless it is to fill the room.
        let opening = pane.as_ref().filter(|_| !in_room).map(|pane| Opening::of(pane, &screen));
        // The pointer on the model: where the model point under it is in the room, and where the
        // pointer is on the window, which sets how large it looks.
        let head = [0, 1, 2].map(|k| (eyes[0].pose.position[k] + eyes[1].pose.position[k]) * 0.5);
        let on_model = source.and_then(|s| s.pointer).zip(placement.as_ref()).zip(pointer).map(|((point, placement), at)| {
            let [x, y, z, _] = view::apply(&placement.matrix, point);
            let on_window = screen.point(f64::from(at[0] / window_points[0].max(1.0)), f64::from(at[1] / window_points[1].max(1.0)));
            ([x, y, z], on_window)
        });
        let metres_per_point = screen.width / f64::from(window_points[0].max(1.0));
        let colours = {
            let t = solidcraft_theme::Tokens::get();
            [t.form_primary, t.app_bar].map(|c| {
                let linear = egui::Rgba::from(c);
                [linear.r(), linear.g(), linear.b(), 1.0]
            })
        };
        let mut pictures = 0;
        for (index, eye) in eyes.iter().enumerate() {
            let size = session.eye_size(index);
            // The model, when there is a design view to show and this eye can see its pane.
            let model = match (source, pane.as_ref(), placement.as_ref()) {
                (Some(source), Some(pane), Some(placement)) => {
                    self.draw_model(rs, source, eye, opening.is_some().then_some(pane), placement, &screen, size)
                }
                _ => false,
            };
            let pointer = on_model.and_then(|(at, on_window)| pointer_vertices(eye, head, &screen, at, on_window, metres_per_point, colours));
            if let Some(vertices) = &pointer {
                rs.queue.write_buffer(&self.pointer_vertices, 0, vertices);
            }
            let pointer = pointer.is_some();
            let picture = session.eye_picture(index)?;
            // Without a design view this frame (the start page, a sheet), the theme's own colours.
            let background = source.map_or_else(
                || {
                    let t = solidcraft_theme::Tokens::get();
                    [t.viewport_top, t.viewport_bottom]
                },
                |s| s.background,
            );
            let Some(axes) = eye.pose.axes() else { continue };
            let uniform = Uniform {
                background,
                axes,
                fov: [eye.fov.left, eye.fov.right, eye.fov.up, eye.fov.down],
                encoded: self.encoded,
                model,
                eye: eye.pose.position,
                opening,
            };
            rs.queue.write_buffer(&self.uniform, 0, &uniform_bytes(&uniform));
            let model_picture = self.model.as_ref().filter(|_| model).map_or(&self.blank, ViewTarget::picture);
            self.blit(rs, &self.eye, model_picture, &picture.view, picture.size, pointer);
            pictures += 1;
        }
        // Only a pair is shown: one eye without the other is worse than the window alone.
        drawn.eyes = pictures == eyes.len();
        Ok(drawn)
    }

    /// Draw the model as `eye` sees it into [`Gpu::model`]: within `pane` as the eye sees it, or
    /// all of it when there is none (the model fills the room). `false` when nothing was drawn.
    fn draw_model(
        &mut self,
        rs: &egui_wgpu::RenderState,
        source: &Source,
        eye: &Eye,
        pane: Option<&Pane>,
        placement: &view::Placement,
        screen: &Screen,
        size: [u32; 2],
    ) -> bool {
        if self.model_wait > 0 {
            self.model_wait -= 1;
            return false;
        }
        let Some(seen) = view::eye_view(eye, placement) else { return false };
        // The pane's rectangle saves drawing what the picture's mask would hide anyway.
        let scissor = match pane {
            Some(pane) => view::pane_scissor(eye, &pane.corners(screen), size).unwrap_or([0; 4]),
            None => [0, 0, size[0], size[1]],
        };
        let mut frame = source.frame.clone();
        frame.view_proj = view::to_f32(&seen.view_proj);
        frame.turned_depth = Some(view::turned_depth_f32(&seen.view_proj));
        frame.eye = [seen.eye[0] as f32, seen.eye[1] as f32, seen.eye[2] as f32, 1.0];
        frame.size_px = [size[0] as f32, size[1] as f32];
        let mut renderer = rs.renderer.write();
        let Some(view) = renderer.callback_resources.get_mut::<Renderer>() else { return false };
        let drawn = view.draw_second_view(&rs.device, &rs.queue, &frame, size, scissor, &mut self.model);
        // No picture to draw into: the GPU refused it (the renderer has said so in the log).
        if !drawn && self.model.is_none() {
            self.model_wait = RETRY_AFTER;
        }
        drawn
    }
}

/// The desktop camera as the model is placed by it.
fn camera_frame(camera: &solidcraft_viewport::Camera) -> CameraFrame {
    let (right, up, back) = camera.basis();
    let target = camera.target;
    CameraFrame {
        target: [target.x, target.y, target.z],
        right: [right.x, right.y, right.z],
        up: [up.x, up.y, up.z],
        back: [back.x, back.y, back.z],
        half_height: camera.half_height(),
    }
}

/// The part of the window the model shows through, in the room: the design view's rectangle.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Opening {
    /// Its middle (metres).
    center: [f64; 3],
    /// Unit directions along its width and height.
    right: [f64; 3],
    up: [f64; 3],
    /// Half its width and half its height (metres).
    half: [f64; 2],
}

impl Opening {
    /// `pane` of `screen`, in the room.
    fn of(pane: &Pane, screen: &Screen) -> Opening {
        Opening {
            center: screen.point((pane.left + pane.right) * 0.5, (pane.top + pane.bottom) * 0.5),
            right: screen.right,
            up: screen.up,
            half: [(pane.right - pane.left) * screen.width * 0.5, (pane.bottom - pane.top) * screen.height * 0.5],
        }
    }
}

/// What the picture shader is told.
#[derive(Clone, Copy)]
struct Uniform {
    /// The backdrop's colours, above and below.
    background: [egui::Color32; 2],
    /// The eye's right, up and back directions in the room.
    axes: [[f64; 3]; 3],
    /// The eye's field of view (radians): left, right, up, down.
    fov: [f64; 4],
    encoded: bool,
    model: bool,
    /// The eye's place in the room.
    eye: [f64; 3],
    /// The model shows only through this opening; `None`: everywhere in the room.
    opening: Option<Opening>,
}

impl Default for Uniform {
    /// What the window's picture is drawn with: no backdrop, no eye, no model.
    fn default() -> Self {
        Uniform {
            background: [egui::Color32::BLACK; 2],
            axes: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
            fov: [-0.5, 0.5, 0.5, -0.5],
            encoded: true,
            model: false,
            eye: [0.0; 3],
            opening: None,
        }
    }
}

/// The shader's `U`, as bytes.
fn uniform_bytes(u: &Uniform) -> Vec<u8> {
    let colour = |c: egui::Color32| {
        let [r, g, b, _] = c.to_normalized_gamma_f32();
        [r, g, b, 1.0]
    };
    let with = |d: [f64; 3], w: f64| [d[0] as f32, d[1] as f32, d[2] as f32, w as f32];
    let flag = |on: bool| if on { 1.0 } else { 0.0 };
    // Without an opening the shader's own is never read; a unit one keeps its arithmetic finite.
    let opening = u.opening.unwrap_or(Opening { center: [0.0; 3], right: [1.0, 0.0, 0.0], up: [0.0, 1.0, 0.0], half: [1.0, 1.0] });
    let values: [[f32; 4]; 11] = [
        colour(u.background[0]),
        colour(u.background[1]),
        with(u.axes[0], 0.0),
        with(u.axes[1], 0.0),
        with(u.axes[2], 0.0),
        u.fov.map(|a| a.tan() as f32),
        [flag(u.encoded), flag(u.model), flag(u.opening.is_some()), 0.0],
        with(u.eye, 0.0),
        with(opening.center, 0.0),
        with(opening.right, opening.half[0].max(1e-6)),
        with(opening.up, opening.half[1].max(1e-6)),
    ];
    values.iter().flatten().flat_map(|v| v.to_le_bytes()).collect()
}

/// The mouse pointer on the model, for one eye's picture: the window's arrow laid in the room at
/// `at` (the model point under the pointer), parallel to `screen`, as large as it looks on the
/// window at `on_window` to a head at `head`. Vertices for [`Gpu::pointer`]: the edge's fan of
/// triangles in `colours[1]`, then the fill's in `colours[0]`. `None` when any of it is behind the
/// eye or has no place in its picture.
fn pointer_vertices(
    eye: &Eye,
    head: [f64; 3],
    screen: &Screen,
    at: [f64; 3],
    on_window: [f64; 3],
    metres_per_point: f64,
    colours: [[f32; 4]; 2],
) -> Option<Vec<u8>> {
    let room = view::room_view_proj(eye)?;
    let metres = metres_per_point * view::same_apparent_size(head, on_window, at);
    if !(metres.is_finite() && metres > 0.0) {
        return None;
    }
    // The edge: the same outline, a little larger about its middle.
    let count = POINTER.len() as f64;
    let middle = POINTER.iter().fold([0.0, 0.0], |sum, p| [sum[0] + p[0] / count, sum[1] + p[1] / count]);
    let edge = POINTER.map(|p| [middle[0] + (p[0] - middle[0]) * 1.22, middle[1] + (p[1] - middle[1]) * 1.22]);
    let mut bytes = Vec::with_capacity((POINTER_VERTICES * POINTER_VERTEX) as usize);
    for (outline, colour) in [(edge, colours[1]), (POINTER, colours[0])] {
        let corners: Vec<[f64; 2]> =
            view::outline_in_room(at, screen, &outline, metres).into_iter().map(|p| view::project(&room, p)).collect::<Option<_>>()?;
        let (tip, rest) = corners.split_first()?;
        for pair in rest.windows(2) {
            let [second, third] = pair else { continue };
            for corner in [tip, second, third] {
                for value in [corner[0] as f32, corner[1] as f32].into_iter().chain(colour) {
                    bytes.extend_from_slice(&value.to_le_bytes());
                }
            }
        }
    }
    (bytes.len() as u64 == POINTER_VERTICES * POINTER_VERTEX).then_some(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Off unless asked: by the flag, the environment or the saved setting, in that order; the
    /// environment can also turn a saved yes off for a run.
    #[test]
    fn the_vr_view_is_tried_only_when_asked_for() {
        assert!(!wanted(false, None, None), "nothing asked: the app starts as ever");
        assert!(!wanted(false, None, Some(&json!({"msaa": 4}))), "an older graphics.json has no choice");
        assert!(!wanted(false, None, Some(&json!({"vr": false}))));
        assert!(wanted(false, None, Some(&json!({"vr": true}))));
        assert!(wanted(true, None, Some(&json!({"vr": false}))), "--vr");
        assert!(wanted(true, Some("0"), None), "--vr over the environment");
        for yes in ["1", "true", "ON", " yes "] {
            assert!(wanted(false, Some(yes), None), "{yes}");
        }
        for no in ["0", "false", "Off", "no"] {
            assert!(!wanted(false, Some(no), Some(&json!({"vr": true}))), "{no} turns a saved yes off");
        }
    }

    /// A saved or environment value that is not a choice is not taken for one.
    #[test]
    fn a_value_that_is_not_a_choice_changes_nothing() {
        for junk in ["", "2", "maybe", "\u{0}", "yes please"] {
            assert!(!wanted(false, Some(junk), None), "{junk:?}");
            assert!(wanted(false, Some(junk), Some(&json!({"vr": true}))), "{junk:?} leaves the saved choice");
        }
        for saved in [json!({"vr": "true"}), json!({"vr": 1}), json!({"vr": null}), json!({"vr": [true]}), json!("vr"), json!(null)] {
            assert!(!wanted(false, None, Some(&saved)), "{saved}");
        }
    }

    /// The window's place can be tried out per run; a value out of reach or not a number leaves
    /// the default, each on its own.
    #[test]
    fn the_layout_takes_only_values_within_reach() {
        let none = Layout::from_env(|_| None);
        assert_eq!(none, Layout::default());
        let set = Layout::from_env(|name| match name {
            "SOLIDCRAFT_VR_WIDTH" => Some("2.5".into()),
            "SOLIDCRAFT_VR_DISTANCE" => Some(" 0.8 ".into()),
            "SOLIDCRAFT_VR_DROP" => Some("-0.3".into()),
            "SOLIDCRAFT_VR_DEPTH" => Some("0.25".into()),
            _ => None,
        });
        assert_eq!(set, Layout { width: 2.5, distance: 0.8, drop: -0.3, depth: 0.25 });
        let bad = Layout::from_env(|name| match name {
            "SOLIDCRAFT_VR_WIDTH" => Some("0".into()),
            "SOLIDCRAFT_VR_DISTANCE" => Some("1e9".into()),
            "SOLIDCRAFT_VR_DROP" => Some("NaN".into()),
            "SOLIDCRAFT_VR_DEPTH" => Some("deep".into()),
            _ => None,
        });
        assert_eq!(bad, Layout::default());
        let mixed = Layout::from_env(|name| match name {
            "SOLIDCRAFT_VR_WIDTH" => Some("inf".into()),
            "SOLIDCRAFT_VR_DEPTH" => Some("-0.2".into()),
            _ => None,
        });
        assert_eq!(mixed, Layout { depth: -0.2, ..Layout::default() });
    }

    /// The shader's uniform is eleven vectors: colours as the screen shows them, the eye's
    /// directions, the tangents of its field of view, the flags, the eye's place and the opening
    /// the model shows through. Without an opening its flag is off and its sizes stay finite.
    #[test]
    fn the_uniform_is_laid_out_as_the_shader_reads_it() {
        let floats = |u: &Uniform| -> Vec<f32> {
            let bytes = uniform_bytes(u);
            assert_eq!(bytes.len() as u64, UNIFORM_SIZE);
            bytes.as_chunks::<4>().0.iter().map(|b| f32::from_le_bytes(*b)).collect()
        };
        let u = Uniform {
            background: [egui::Color32::WHITE, egui::Color32::BLACK],
            axes: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
            fov: [-std::f64::consts::FRAC_PI_4, std::f64::consts::FRAC_PI_4, 0.0, 0.0],
            encoded: true,
            model: false,
            eye: [0.25, 0.5, 0.75],
            opening: None,
        };
        let room = floats(&u);
        assert_eq!(room[0..4], [1.0, 1.0, 1.0, 1.0]);
        assert_eq!(room[4..8], [0.0, 0.0, 0.0, 1.0]);
        assert_eq!(room[8..12], [1.0, 0.0, 0.0, 0.0]);
        assert_eq!(room[16..20], [0.0, 0.0, 1.0, 0.0]);
        assert!((room[20] + 1.0).abs() < 1e-6 && (room[21] - 1.0).abs() < 1e-6);
        assert_eq!(room[24..28], [1.0, 0.0, 0.0, 0.0], "stored as the screen shows it, no model, no opening");
        assert_eq!(room[28..31], [0.25, 0.5, 0.75]);
        assert!(room[39] > 0.0 && room[43] > 0.0, "sizes the shader divides by are never zero");
        let opening = Opening { center: [1.0, 2.0, -3.0], right: [1.0, 0.0, 0.0], up: [0.0, 1.0, 0.0], half: [0.4, 0.0] };
        let window = floats(&Uniform { model: true, encoded: false, opening: Some(opening), ..u });
        assert_eq!(window[24..28], [0.0, 1.0, 1.0, 0.0], "linear, a model, through the opening");
        assert_eq!(window[32..35], [1.0, 2.0, -3.0]);
        assert_eq!(window[36..40], [1.0, 0.0, 0.0, 0.4]);
        assert!(window[40..43] == [0.0, 1.0, 0.0] && window[43] > 0.0, "an opening with no height still divides");
    }

    /// The picture shader is valid WGSL, with the entry points the pipelines name.
    #[test]
    fn the_picture_shader_is_valid() {
        let module = wgpu::naga::front::wgsl::parse_str(SHADER).expect("the shader parses");
        let names: Vec<&str> = module.entry_points.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["vs", "fs_eye", "vs_pointer", "fs_pointer", "fs_window"]);
        let flags = wgpu::naga::valid::ValidationFlags::all();
        let capabilities = wgpu::naga::valid::Capabilities::empty();
        wgpu::naga::valid::Validator::new(flags, capabilities).validate(&module).expect("the shader is valid");
    }

    /// The opening the model shows through is the design view's rectangle on the window.
    #[test]
    fn the_opening_is_the_design_views_rectangle_in_the_room() {
        let screen = Screen::in_front(2.0, 2.0, 1.0, 0.0);
        let pane = Pane { left: 0.5, top: 0.0, right: 1.0, bottom: 0.5 };
        let opening = Opening::of(&pane, &screen);
        assert_eq!(opening.center, [0.5, 0.25, -1.0]);
        assert_eq!((opening.right, opening.up), ([1.0, 0.0, 0.0], [0.0, 1.0, 0.0]));
        assert_eq!(opening.half, [0.5, 0.25]);
    }

    /// The pointer on the model is the window's arrow as an eye sees it at the model's depth: an
    /// edge and a fill, a fan of triangles each, its tip where the model point shows. Farther
    /// away it is drawn larger in the room, so it looks the same size. Behind the eye, or with
    /// numbers that are not finite, there is none.
    #[test]
    fn the_pointer_is_drawn_at_the_models_depth() {
        let a = std::f64::consts::FRAC_PI_4;
        let eye = Eye { pose: view::Pose::IDENTITY, fov: view::Fov { left: -a, right: a, up: a, down: -a } };
        let screen = Screen::in_front(2.0, 2.0, 1.0, 0.0);
        let colours = [[1.0, 1.0, 1.0, 1.0], [0.0, 0.0, 0.0, 1.0]];
        let corners = |bytes: &[u8]| -> Vec<[f32; 6]> {
            let floats: Vec<f32> = bytes.as_chunks::<4>().0.iter().map(|b| f32::from_le_bytes(*b)).collect();
            floats.as_chunks::<6>().0.to_vec()
        };
        let on_window = [0.0, 0.0, -1.0];
        let near = corners(&pointer_vertices(&eye, [0.0; 3], &screen, on_window, on_window, 0.001, colours).unwrap());
        assert_eq!(near.len() as u64, POINTER_VERTICES);
        // The edge first, in its colour, then the fill in its own; each fan starts at its tip.
        assert!(near[..15].iter().all(|v| v[2..] == colours[1]) && near[15..].iter().all(|v| v[2..] == colours[0]));
        assert!(near[15][0].abs() < 1e-6 && near[15][1].abs() < 1e-6, "the fill's tip is where the point shows: {:?}", near[15]);
        assert!(near[15..].iter().all(|v| v[0] >= -1e-6 && v[1] <= 1e-6), "the arrow hangs down and to the right of its tip");
        // Twice as far along the same line of sight: the same picture.
        let far = corners(&pointer_vertices(&eye, [0.0; 3], &screen, [0.0, 0.0, -2.0], on_window, 0.001, colours).unwrap());
        assert!(near.iter().zip(&far).all(|(n, f)| (n[0] - f[0]).abs() < 1e-5 && (n[1] - f[1]).abs() < 1e-5), "it looks the same size");
        assert_eq!(pointer_vertices(&eye, [0.0; 3], &screen, [0.0, 0.0, 2.0], on_window, 0.001, colours), None, "behind the eye");
        assert_eq!(pointer_vertices(&eye, [0.0; 3], &screen, on_window, on_window, f64::NAN, colours), None);
        assert_eq!(pointer_vertices(&eye, [0.0; 3], &screen, on_window, on_window, 0.0, colours), None);
        assert_eq!(pointer_vertices(&eye, [0.0; 3], &screen, [f64::NAN, 0.0, -1.0], on_window, 0.001, colours), None);
    }

    /// A start that failed says what to do about it where the user can do something, and passes
    /// the runtime's own words on otherwise.
    #[test]
    fn a_failed_start_says_what_to_do() {
        use solidcraft_xr::Error;
        assert_eq!(Reason::of(&Error::NoRuntime("XR_ERROR_RUNTIME_UNAVAILABLE".into())), Reason::NoRuntime);
        assert_eq!(Reason::of(&Error::NoHeadset("XR_ERROR_FORM_FACTOR_UNAVAILABLE".into())), Reason::NoHeadset);
        assert_eq!(Reason::of(&Error::NoLoader("openxr_loader.dll".into())), Reason::NoLoader);
        assert_eq!(Reason::of(&Error::NoVulkan), Reason::NoVulkan);
        let other = Reason::of(&Error::NoFormat(vec![37, 44]));
        assert!(matches!(&other, Reason::Other(words) if words.contains("[37, 44]")), "{other:?}");
        assert!(other.text().contains("[37, 44]"), "the runtime's own words are passed on");
        // Each known reason has words of its own in the catalog.
        let known = [Reason::Software, Reason::NoLoader, Reason::NoRuntime, Reason::NoHeadset, Reason::NoVulkan];
        let texts: std::collections::BTreeSet<String> = known.iter().map(Reason::text).collect();
        assert_eq!(texts.len(), known.len(), "{texts:?}");
        let failed = Vr::failed(Reason::NoRuntime);
        assert_eq!(failed.status(), &Status::Failed(Reason::NoRuntime));
    }
}
