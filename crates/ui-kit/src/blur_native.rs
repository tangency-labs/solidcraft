use super::*;
use crate::blur_plan::{self, PxRect, Run};
use egui_wgpu::{CallbackResources, CallbackTrait, FrameCompositor, ScreenDescriptor, wgpu};
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

thread_local! { static CURRENT: RefCell<Option<Current>> = const { RefCell::new(None) }; }
thread_local! { static SURFACE_ALPHA: std::cell::Cell<Option<u8>> = const { std::cell::Cell::new(None) }; }
/// Target memory all windows may hold together.
const BUDGET: u64 = 256 * 1024 * 1024;
/// Markers one window may emit in a frame: a bound on memory for hostile input, far above any real UI.
const MAX_MARKERS: usize = 4096;
/// A window that has not begun a frame for this many `begin` calls (of any window) has closed:
/// its resources are released.
const STALE_AFTER: u64 = 600;
type Params = [f32; 12];

/// The window being laid out on this thread: the one `begin` was called for.
#[derive(Clone)]
struct Current {
    shared: Arc<Shared>,
    window: u64,
}

/// A window's key: egui's viewport id as a number, as the renderer reports it to `begin_viewport`.
fn key(viewport: egui::ViewportId) -> u64 {
    viewport.0.value()
}

fn capability(adapter: &wgpu::Adapter, device_type: wgpu::DeviceType, format: wgpu::TextureFormat, split: bool, msaa: u32) -> Capability {
    if device_type == wgpu::DeviceType::Cpu {
        return Capability::Unavailable("software adapter (gpu-software)");
    }
    rendering_capability(adapter, format, split, msaa)
}

fn rendering_capability(adapter: &wgpu::Adapter, format: wgpu::TextureFormat, split: bool, msaa: u32) -> Capability {
    let allowed = wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_SRC;
    if !split || msaa != 1 {
        Capability::Unavailable("requires a native split scene and 1x UI without depth")
    } else if !matches!(
        format,
        wgpu::TextureFormat::Rgba8Unorm | wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Rgba8UnormSrgb | wgpu::TextureFormat::Bgra8UnormSrgb
    ) || [format, wgpu::TextureFormat::Rgba16Float].iter().any(|f| {
        let features = adapter.get_texture_format_features(*f);
        !features.allowed_usages.contains(allowed) || !features.flags.contains(wgpu::TextureFormatFeatureFlags::FILTERABLE)
    }) {
        Capability::Unavailable("required renderable filterable formats unavailable")
    } else {
        Capability::Available
    }
}

#[derive(Clone)]
struct Host {
    adapter: wgpu::Adapter,
    device_type: wgpu::DeviceType,
    device: wgpu::Device,
    queue: wgpu::Queue,
    renderer: std::sync::Weak<egui::epaint::mutex::RwLock<egui_wgpu::Renderer>>,
    lost: egui_wgpu::DeviceLost,
    format: wgpu::TextureFormat,
    capability: std::sync::OnceLock<Capability>,
    split: bool,
    msaa: u32,
}
impl Host {
    fn capability(&self) -> Capability {
        *self.capability.get_or_init(|| capability(&self.adapter, self.device_type, self.format, self.split, self.msaa))
    }
}

pub struct State {
    radius: f32,
    available: bool,
    tier_cap: usize,
    host: Option<Host>,
    active: Option<Arc<Shared>>,
    retired: Counters,
    failure: Option<&'static str>,
    surface_alpha: Option<u8>,
    last_size: Option<[u32; 2]>,
    whole_frame: bool,
}

impl Default for State {
    fn default() -> Self {
        Self {
            radius: 0.0,
            available: true,
            tier_cap: 5,
            host: None,
            active: None,
            retired: Counters::default(),
            failure: None,
            surface_alpha: None,
            last_size: None,
            whole_frame: false,
        }
    }
}

impl State {
    pub fn install(&mut self, rs: &egui_wgpu::RenderState, split: bool) {
        self.retire();
        self.failure = None;
        self.host = Some(Host {
            adapter: rs.adapter.clone(),
            device_type: rs.adapter.get_info().device_type,
            device: rs.device.clone(),
            queue: rs.queue.clone(),
            renderer: Arc::downgrade(&rs.renderer),
            lost: rs.device_lost.clone(),
            format: rs.target_format,
            capability: std::sync::OnceLock::new(),
            split,
            msaa: rs.msaa_samples,
        });
    }

    fn retire(&mut self) {
        if let Some(shared) = self.active.take() {
            if let Ok(mut windows) = shared.windows.lock() {
                self.retired = windows.total();
                self.retired.clear_live();
                if let Some(root) = windows.map.get(&key(egui::ViewportId::ROOT)) {
                    self.failure = root.failure.or(self.failure);
                }
                windows.map.clear();
            }
            if let Some(renderer) = self.host.as_ref().and_then(|h| h.renderer.upgrade()) {
                renderer.write().frame_compositor = None;
            }
        }
    }

    pub fn set_radius(&mut self, radius: f32) -> Result<(), &'static str> {
        let radius = validate_radius(radius)?;
        if radius != self.radius {
            self.retire();
            self.radius = radius;
            self.failure = None;
        }
        Ok(())
    }
    pub fn radius(&self) -> f32 {
        self.radius
    }
    pub fn software_adapter(&self) -> bool {
        self.host.as_ref().is_some_and(|h| h.device_type == wgpu::DeviceType::Cpu)
    }
    pub fn set_available(&mut self, available: bool) {
        self.available = available;
        if !available {
            self.retire();
        }
    }
    pub fn set_tier_cap(&mut self, cap: usize) {
        let cap = cap.clamp(2, 5);
        if self.tier_cap != cap {
            self.retire();
            self.tier_cap = cap;
        }
    }
    /// What the main window last painted with blur, for the quality guard.
    pub fn painted_frame(&self) -> Option<PaintedFrame> {
        self.active.as_ref()?.windows.lock().ok()?.map.get(&key(egui::ViewportId::ROOT))?.painted
    }
    pub fn capability(&self) -> Capability {
        self.capability_for(egui::ViewportId::ROOT)
    }
    /// Whether `viewport`'s window can blur: the device, and that window's own failures.
    pub fn capability_for(&self, viewport: egui::ViewportId) -> Capability {
        let base = self.host_capability();
        if base != Capability::Available {
            return base;
        }
        if viewport == egui::ViewportId::ROOT
            && let Some(reason) = self.failure
        {
            return Capability::Unavailable(reason);
        }
        if let Some(shared) = &self.active {
            return match shared.windows.lock() {
                Ok(windows) => windows.map.get(&key(viewport)).and_then(|inner| inner.failure).map_or(Capability::Available, Capability::Unavailable),
                Err(_) => Capability::Unavailable("blur state unavailable"),
            };
        }
        Capability::Available
    }
    /// The device and its formats: what no window can change.
    fn host_capability(&self) -> Capability {
        let Some(host) = &self.host else { return Capability::Unavailable("no GPU host") };
        if host.lost.is_lost() {
            return Capability::Unavailable("device lost");
        }
        host.capability()
    }
    /// Every window's counters together.
    pub fn counters(&self) -> Counters {
        let Some(shared) = &self.active else { return self.retired };
        shared.windows.lock().map_or(self.retired, |windows| windows.total())
    }
    /// Blur the whole frame at every marker, as before regions existed: the reference a fixture
    /// compares the region-scoped blur with. Takes effect on the next frame.
    #[cfg(any(test, feature = "test-support"))]
    pub fn set_whole_frame_blur(&mut self, whole: bool) {
        if self.whole_frame != whole {
            self.retire();
            self.whole_frame = whole;
        }
    }
    /// Internal capture tint only; deliberately not a user preference.
    pub fn set_surface_alpha(&mut self, alpha: Option<u8>) {
        self.surface_alpha = alpha;
    }

    /// Start `ctx`'s window frame: surfaces laid out until the returned scope drops may ask for a
    /// backdrop. Call it at the top of each window's UI; scopes nest, so a window shown inside
    /// another's UI hands the thread back when it ends.
    pub fn begin(&mut self, ctx: &egui::Context) -> Scope {
        let previous = CURRENT.with(|c| c.replace(None));
        let alpha = SURFACE_ALPHA.with(|a| a.replace(self.surface_alpha));
        let scope = Scope { previous, alpha };
        if self.radius == 0.0 {
            self.retire();
            return scope;
        }
        let window = ctx.viewport_id();
        let size = ctx.content_rect().size() * ctx.pixels_per_point();
        let valid = size.is_finite() && size.x > 0.0 && size.y > 0.0;
        let size_key = valid.then(|| [size.x.round() as u32, size.y.round() as u32]);
        if window == egui::ViewportId::ROOT
            && let Some(size_key) = size_key
            && self.last_size != Some(size_key)
        {
            // The main window changed size: a latched failure may no longer apply.
            self.retire();
            self.failure = None;
            self.last_size = Some(size_key);
        }
        if !self.available || self.host_capability() != Capability::Available || !valid {
            if window == egui::ViewportId::ROOT {
                self.retire();
            }
            return scope;
        }
        let Some(host) = &self.host else { return scope };
        let shared = self.active.get_or_insert_with(|| {
            Arc::new(Shared {
                host: host.clone(),
                radius: self.radius,
                tier_cap: self.tier_cap,
                whole_frame: self.whole_frame,
                windows: Mutex::new(Windows { base: self.retired, painting: key(egui::ViewportId::ROOT), ..Windows::default() }),
            })
        });
        let id = key(window);
        if let Ok(mut windows) = shared.windows.lock() {
            windows.clock = windows.clock.wrapping_add(1);
            let clock = windows.clock;
            let stale: Vec<u64> =
                windows.map.iter().filter(|(k, inner)| **k != id && clock.wrapping_sub(inner.last_used) > STALE_AFTER).map(|(k, _)| *k).collect();
            for gone in stale {
                windows.forget(gone);
            }
            let inner = windows.map.entry(id).or_default();
            if inner.size != size_key {
                // A resized window gets fresh targets and a fresh chance after a failure.
                inner.size = size_key;
                inner.failure = None;
                inner.gpu = None;
                inner.counts.clear_live();
            }
            inner.last_used = clock;
            inner.markers.clear();
            inner.planned.clear();
            inner.serial = inner.serial.wrapping_add(1);
            if inner.frame != ctx.cumulative_frame_nr() {
                inner.frame = ctx.cumulative_frame_nr();
                inner.continuous = ctx.requested_repaint_last_pass();
            }
            inner.rendered = false;
            inner.drawn_before = inner.counts.markers_drawn;
        }
        if let Some(renderer) = host.renderer.upgrade() {
            renderer.write().frame_compositor = Some(shared.clone());
        }
        CURRENT.with(|c| *c.borrow_mut() = Some(Current { shared: shared.clone(), window: id }));
        scope
    }
}

/// Select production policy or bypass only its software rejection for rendering fixtures.
#[cfg(any(test, feature = "test-support"))]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum FixturePolicy {
    Production,
    Rendering,
}

#[cfg(any(test, feature = "test-support"))]
pub fn fixture_state(
    adapter: &wgpu::Adapter,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    renderer: &Arc<egui::epaint::mutex::RwLock<egui_wgpu::Renderer>>,
    format: wgpu::TextureFormat,
    policy: FixturePolicy,
) -> State {
    State {
        radius: 0.0,
        available: true,
        tier_cap: 5,
        host: Some(Host {
            adapter: adapter.clone(),
            device_type: adapter.get_info().device_type,
            device: device.clone(),
            queue: queue.clone(),
            renderer: Arc::downgrade(renderer),
            lost: egui_wgpu::DeviceLost::watch(device),
            format,
            capability: match policy {
                FixturePolicy::Production => std::sync::OnceLock::new(),
                FixturePolicy::Rendering => std::sync::OnceLock::from(rendering_capability(adapter, format, true, 1)),
            },
            split: true,
            msaa: 1,
        }),
        active: None,
        retired: Counters::default(),
        failure: None,
        surface_alpha: None,
        last_size: None,
        whole_frame: false,
    }
}

#[cfg(any(test, feature = "test-support"))]
pub fn fixture_budget(size: [u32; 2], steps: usize) -> Option<u64> {
    target_bytes(size, steps, 16384)
}
impl Drop for State {
    fn drop(&mut self) {
        self.retire();
    }
}

/// Restores the previous context even on early UI returns. No heap allocation.
pub struct Scope {
    previous: Option<Current>,
    alpha: Option<u8>,
}
impl Drop for Scope {
    fn drop(&mut self) {
        CURRENT.with(|c| *c.borrow_mut() = self.previous.take());
        SURFACE_ALPHA.with(|a| a.set(self.alpha));
    }
}

pub fn enabled() -> bool {
    CURRENT.with(|c| c.borrow().is_some())
}
pub fn panel_fill(fill: Color32) -> Color32 {
    if fill.a() == 0 {
        return fill;
    }
    SURFACE_ALPHA.with(|a| {
        a.get().map_or(fill, |alpha| {
            let [r, g, b, _] = fill.to_srgba_unmultiplied();
            Color32::from_rgba_unmultiplied(r, g, b, alpha)
        })
    })
}

struct Shared {
    host: Host,
    radius: f32,
    tier_cap: usize,
    /// Every marker blurs the whole frame (a fixture's reference), not just its padded region.
    whole_frame: bool,
    windows: Mutex<Windows>,
}

/// Per-window state. Each window has its own composition target, pyramid and markers; the
/// renderer says which one it is painting.
#[derive(Default)]
struct Windows {
    map: HashMap<u64, Inner>,
    /// The window the renderer is painting now (`begin_viewport`).
    painting: u64,
    /// One tick per `begin`, to find windows that stopped painting.
    clock: u64,
    /// Counters of windows already forgotten, and of earlier blur sessions.
    base: Counters,
}

impl Windows {
    fn total(&self) -> Counters {
        let mut total = self.base;
        for inner in self.map.values() {
            total.add(&inner.counts);
        }
        total
    }

    /// Release a window's resources, keeping what it counted.
    fn forget(&mut self, id: u64) {
        if let Some(mut inner) = self.map.remove(&id) {
            inner.counts.clear_live();
            self.base.add(&inner.counts);
        }
    }
}

#[derive(Default)]
struct Inner {
    frame: u64,
    continuous: bool,
    rendered: bool,
    drawn_before: u64,
    painted: Option<PaintedFrame>,
    submitted_at: Option<std::time::Instant>,
    gpu: Option<Gpu>,
    /// Bytes of the targets in `gpu`, for the shared budget.
    gpu_bytes: u64,
    markers: Vec<Marker>,
    /// Per marker of this frame: the planner gave it a region, so its blur is ready when it draws.
    planned: Vec<Option<usize>>,
    counts: Counters,
    failure: Option<&'static str>,
    serial: u64,
    /// The window's size in pixels when it last began a frame.
    size: Option<[u32; 2]>,
    /// The `Windows::clock` of its last `begin`.
    last_used: u64,
}
#[derive(Clone, Copy)]
struct Marker {
    rect: Rect,
    corners: egui::CornerRadius,
    snap: bool,
    // Identity only: the paint job owns this callback for the entire frame. Keeping
    // its Arc here would create a Shared -> callback -> Shared ownership cycle.
    callback_id: usize,
}

/// Whether a surface with this fill, painted at this opacity, borrows the blurred backdrop.
/// Only a glass material blurs: an opaque fill has nothing to show through, and a surface
/// painted below full opacity (fading, dimmed, dragged) is translucent to be seen through or
/// to fade, so what is behind it stays sharp. Blur appears only once the fade is complete.
pub fn wants_backdrop(fill: Color32, opacity: f32, visible: bool) -> bool {
    fill.a() != 0 && fill.a() != 255 && opacity.is_finite() && opacity >= 1.0 && visible
}

pub fn decorate(painter: &Painter, frame: Frame, content: Rect, ordinary: Shape) -> Shape {
    if !wants_backdrop(frame.fill, painter.opacity(), painter.is_visible()) {
        return ordinary;
    }
    match mark(painter, frame.widget_rect(content), frame.corner_radius) {
        Some(marker) => Shape::Vec(vec![marker, ordinary]),
        None => ordinary,
    }
}

/// A backdrop request for `rect`: the marker shape to add before the surface's own fill, or `None`
/// when blur is off, the window has not begun a frame, or the marker cannot be had.
pub(super) fn mark(painter: &Painter, rect: Rect, corners: egui::CornerRadius) -> Option<Shape> {
    // A faded painter never blurs (see `wants_backdrop`).
    let opacity = painter.opacity();
    if !opacity.is_finite() || opacity < 1.0 || !painter.is_visible() || !rect.is_finite() || !rect.is_positive() {
        return None;
    }
    CURRENT.with(|c| {
        let kept = c.borrow();
        let current = kept.as_ref()?;
        // A window that did not begin its own frame cannot hold markers.
        if key(painter.ctx().viewport_id()) != current.window {
            return None;
        }
        let mut windows = current.shared.windows.lock().ok()?;
        let inner = windows.map.get_mut(&current.window)?;
        if inner.failure.is_some() || inner.markers.len() >= MAX_MARKERS {
            return None;
        }
        let index = inner.markers.len();
        let callback = egui_wgpu::Callback::new_paint_callback(rect, PanelMarker { shared: current.shared.clone(), window: current.window, index });
        inner.markers.push(Marker {
            rect,
            corners,
            snap: painter.ctx().tessellation_options(|o| o.round_rects_to_pixels),
            callback_id: Arc::as_ptr(&callback.callback) as *const () as usize,
        });
        inner.counts.markers_emitted += 1;
        Some(Shape::Callback(callback))
    })
}

struct PanelMarker {
    shared: Arc<Shared>,
    window: u64,
    index: usize,
}
impl CallbackTrait for PanelMarker {
    fn paint(&self, info: egui::PaintCallbackInfo, pass: &mut wgpu::RenderPass<'static>, _: &CallbackResources) {
        let Ok(mut windows) = self.shared.windows.lock() else { return };
        if windows.painting != self.window {
            return;
        }
        let Some(inner) = windows.map.get_mut(&self.window) else { return };
        if inner.failure.is_some() || self.shared.host.lost.is_lost() {
            return;
        }
        let Some(marker) = inner.markers.get(self.index).copied() else { return };
        let Some(slot) = inner.planned.get(self.index).copied().flatten() else { return };
        let Some(gpu) = inner.gpu.as_ref() else { return };
        if gpu.ready_serial != inner.serial {
            return;
        }
        let Some(binding) = gpu.markers.get(slot) else { return };
        let mut p = [0.0; 12];
        p[2] = u8::from(self.shared.host.format.is_srgb()) as f32;
        let r = info.viewport;
        if !r.is_finite() || !r.is_positive() {
            return;
        }
        p[4..8].copy_from_slice(&[
            r.min.x * info.pixels_per_point,
            r.min.y * info.pixels_per_point,
            r.max.x * info.pixels_per_point,
            r.max.y * info.pixels_per_point,
        ]);
        if marker.snap {
            for value in &mut p[4..8] {
                *value = value.round();
            }
        }
        let scale = info.pixels_per_point * r.width() / marker.rect.width();
        if !scale.is_finite() || p[4..8].iter().any(|v| !v.is_finite()) {
            return;
        }
        p[8..12].copy_from_slice(&[marker.corners.nw, marker.corners.ne, marker.corners.sw, marker.corners.se].map(|v| f32::from(v) * scale));
        if p.iter().any(|value| !value.is_finite()) {
            return;
        }
        // A distinct retained uniform per marker: queue writes never alias another marker's draw.
        self.shared.host.queue.write_buffer(&binding.uniform, 0, &bytes_of(&p));
        pass.set_viewport(0.0, 0.0, gpu.size[0] as f32, gpu.size[1] as f32, 0.0, 1.0);
        pass.set_pipeline(&gpu.panel);
        pass.set_bind_group(0, &binding.group, &[]);
        pass.draw(0..3, 0..1);
        inner.counts.markers_drawn += 1;
    }
}

impl Shared {
    /// Make window `id`'s resources fit `size` and `steps`, and give it one binding per marker.
    /// `false` leaves the failure on the window for the capability report. Targets of other
    /// windows are released, least recently painted first, to stay inside the shared budget.
    fn ensure(&self, windows: &mut Windows, id: u64, size: [u32; 2], steps: usize, offset: f32, bindings: usize) -> bool {
        let device = &self.host.device;
        let fail = |windows: &mut Windows, reason: &'static str| {
            if let Some(inner) = windows.map.get_mut(&id) {
                inner.failure = Some(reason);
                inner.gpu = None;
                inner.gpu_bytes = 0;
                inner.counts.clear_live();
            }
            false
        };
        let Some(bytes) = target_bytes(size, steps, device.limits().max_texture_dimension_2d) else {
            return fail(windows, "blur target is empty, exceeds device limits or memory budget");
        };
        let current = windows.map.get(&id).and_then(|inner| inner.gpu.as_ref()).is_some_and(|g| g.size == size && g.steps == steps);
        if !current {
            loop {
                let others: u64 = windows.map.iter().filter(|(k, _)| **k != id).map(|(_, inner)| inner.gpu_bytes).sum();
                if others.saturating_add(bytes) <= BUDGET {
                    break;
                }
                let victim = windows
                    .map
                    .iter()
                    .filter(|(k, inner)| **k != id && inner.gpu.is_some())
                    .min_by_key(|(_, inner)| inner.last_used)
                    .map(|(k, _)| *k);
                let Some(victim) = victim else { return fail(windows, "blur target is empty, exceeds device limits or memory budget") };
                if let Some(inner) = windows.map.get_mut(&victim) {
                    inner.gpu = None;
                    inner.gpu_bytes = 0;
                    inner.counts.clear_live();
                }
            }
        }
        let Some(inner) = windows.map.get_mut(&id) else { return false };
        if !current {
            inner.gpu = None;
            inner.counts.clear_live();
            let (gpu, failure) = egui_wgpu::capture_errors(device, || Gpu::new(device, self.host.format, size, steps, offset, &mut inner.counts));
            if let Some(error) = failure {
                log::warn!("blur resources refused: {error}");
                return fail(windows, "GPU refused blur resources");
            }
            inner.gpu = Some(gpu);
            inner.gpu_bytes = bytes;
            inner.counts.live_bytes += bytes;
        }
        let serial = inner.serial;
        let Inner { gpu, counts, .. } = &mut *inner;
        let Some(gpu) = gpu else { return false };
        gpu.ready_serial = serial;
        let (_, failed) = egui_wgpu::capture_errors(device, || gpu.grow_markers(device, bindings, counts));
        if let Some(error) = failed {
            log::warn!("blur marker resources refused: {error}");
            return fail(windows, "GPU refused blur marker resources");
        }
        true
    }
}

impl FrameCompositor for Shared {
    fn begin_viewport(&self, viewport: u64) {
        if let Ok(mut windows) = self.windows.lock() {
            windows.painting = viewport;
        }
    }
    fn suspend(&self) {
        let lost = self.host.lost.is_lost();
        if let Ok(mut windows) = self.windows.lock() {
            let painting = windows.painting;
            for (id, inner) in windows.map.iter_mut() {
                if lost || *id == painting {
                    inner.gpu = None;
                    inner.gpu_bytes = 0;
                    inner.counts.clear_live();
                }
            }
        }
    }
    fn render(
        &self,
        renderer: &egui_wgpu::Renderer,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        jobs: &[egui::ClippedPrimitive],
        screen: &ScreenDescriptor,
        clear: [f32; 4],
    ) -> bool {
        if self.host.lost.is_lost() {
            return false;
        }
        let Some((steps, offset)) = schedule_capped(self.radius, self.tier_cap) else { return false };
        let (id, marker_ids) = {
            let Ok(windows) = self.windows.lock() else { return false };
            let id = windows.painting;
            let Some(inner) = windows.map.get(&id) else { return false };
            // A frame with no marker has nothing to blur: it takes the ordinary pass.
            if inner.failure.is_some() || inner.markers.is_empty() {
                return false;
            }
            let mut ids: Vec<(usize, usize)> = inner.markers.iter().enumerate().map(|(index, marker)| (marker.callback_id, index)).collect();
            ids.sort_unstable();
            (id, ids)
        };
        // The runs are planned before encoding: which markers share one snapshot, and which
        // pixels each of them needs.
        let pad = if self.whole_frame { u32::MAX } else { blur_plan::reach(steps, offset) };
        let runs = blur_plan::within_budget(plan_runs(jobs, screen, &marker_ids, pad), blur_plan::Budget::for_screen(screen.size_in_pixels));
        if runs.is_empty() {
            return false;
        }
        let (composition, pipeline, binding) = {
            let Ok(mut windows) = self.windows.lock() else { return false };
            if !self.ensure(&mut windows, id, screen.size_in_pixels, steps, offset, runs.iter().map(|run| run.members.len()).sum()) {
                return false;
            }
            let Some(inner) = windows.map.get_mut(&id) else { return false };
            let count = inner.markers.len();
            inner.planned.clear();
            inner.planned.resize(count, None);
            for (slot, member) in runs.iter().flat_map(|run| &run.members).enumerate() {
                let index = jobs.get(member.job).and_then(callback_identity).and_then(|identity| marker_index(&marker_ids, identity));
                if let Some(planned) = index.and_then(|index| inner.planned.get_mut(index)) {
                    *planned = Some(slot);
                }
            }
            let Some(gpu) = inner.gpu.as_ref() else { return false };
            let data = (gpu.composition.view.clone(), gpu.present.clone(), gpu.presentation.group.clone());
            inner.counts.composition_passes += 1;
            data
        };
        let mut next_run = 0;
        {
            let color = wgpu::Color { r: clear[0] as f64, g: clear[1] as f64, b: clear[2] as f64, a: clear[3] as f64 };
            let _clear = pass(encoder, "blur_ui_composition", &composition, color);
        }
        let mut start = 0;
        let mut mesh_offset = 0;
        let mut segment_passes = 0;
        for end in 0..=jobs.len() {
            let marker = jobs.get(end).and_then(callback_identity).is_some_and(|identity| marker_index(&marker_ids, identity).is_some());
            if !marker && end != jobs.len() {
                continue;
            }
            if start < end {
                let mut ui = load_pass(encoder, "blur_ui_segment", &composition);
                renderer.render_with_mesh_offset(&mut ui, &jobs[start..end], screen, mesh_offset);
                mesh_offset += jobs[start..end].iter().filter(|job| matches!(&job.primitive, egui::epaint::Primitive::Mesh(_))).count();
                segment_passes += 1;
            }
            if marker && let Some(run) = runs.get(next_run).filter(|run| run.start_job == end) {
                next_run += 1;
                // The preceding segment is complete before sampling its color target. One
                // snapshot of it serves every member of the run; later markers of the run
                // draw from the same blur because nothing painted since touches their regions.
                if let Ok(mut windows) = self.windows.lock()
                    && let Some(Inner { gpu: Some(gpu), counts, .. }) = windows.map.get_mut(&id)
                {
                    blur_run(encoder, gpu, run, counts);
                }
            }
            start = end;
        }
        {
            let mut present = pass(encoder, "blur_present", target, wgpu::Color::TRANSPARENT);
            present.set_pipeline(&pipeline);
            present.set_bind_group(0, &binding, &[]);
            present.draw(0..3, 0..1);
        }
        if let Ok(mut windows) = self.windows.lock()
            && let Some(inner) = windows.map.get_mut(&id)
        {
            inner.counts.composition_passes += segment_passes;
            inner.counts.present_passes += 1;
            inner.rendered = true;
        }
        true
    }
    fn submitted(&self) {
        let lost = self.host.lost.is_lost();
        let Ok(mut windows) = self.windows.lock() else { return };
        let id = windows.painting;
        let Some(inner) = windows.map.get_mut(&id) else { return };
        if inner.rendered && inner.counts.markers_drawn > inner.drawn_before && !lost {
            let now = std::time::Instant::now();
            inner.painted = Some(PaintedFrame {
                frame: inner.frame,
                interval_ms: inner.submitted_at.map(|at| now.duration_since(at).as_secs_f64() * 1000.0),
                continuous: inner.continuous,
            });
            inner.submitted_at = Some(now);
            inner.rendered = false;
        } else {
            inner.painted = None;
            inner.submitted_at = None;
        }
        if lost {
            inner.gpu = None;
            inner.gpu_bytes = 0;
            inner.counts.clear_live();
            inner.failure = Some("device lost");
        }
    }
}

fn callback_identity(job: &egui::ClippedPrimitive) -> Option<usize> {
    let egui::epaint::Primitive::Callback(callback) = &job.primitive else { return None };
    Some(Arc::as_ptr(&callback.callback) as *const () as usize)
}

/// The marker (its index in this frame's list) a callback identity belongs to; `ids` is sorted.
fn marker_index(ids: &[(usize, usize)], identity: usize) -> Option<usize> {
    ids.binary_search_by_key(&identity, |(id, _)| *id).ok().map(|at| ids[at].1)
}

/// The pixels a paint job can touch, with its clip applied. Vertex bounds are read only on frames
/// that have a marker.
fn job_extent(job: &egui::ClippedPrimitive, scale: f32, size: [u32; 2]) -> Option<PxRect> {
    let bounds = match &job.primitive {
        egui::epaint::Primitive::Mesh(mesh) => {
            mesh.vertices.iter().fold(Rect::NOTHING, |bounds, vertex| bounds.union(Rect::from_min_max(vertex.pos, vertex.pos)))
        }
        egui::epaint::Primitive::Callback(callback) => callback.rect,
    };
    blur_plan::px_extent(bounds, job.clip_rect, scale, size)
}

fn plan_runs(jobs: &[egui::ClippedPrimitive], screen: &ScreenDescriptor, ids: &[(usize, usize)], pad: u32) -> Vec<Run> {
    let size = screen.size_in_pixels;
    let extents: Vec<Option<PxRect>> = jobs.iter().map(|job| job_extent(job, screen.pixels_per_point, size)).collect();
    let markers: Vec<blur_plan::Marker> = jobs
        .iter()
        .enumerate()
        .filter(|(_, job)| callback_identity(job).is_some_and(|identity| marker_index(ids, identity).is_some()))
        .filter_map(|(job, _)| Some(blur_plan::Marker { job, rect: extents.get(job).copied().flatten()? }))
        .collect();
    blur_plan::plan(&extents, &markers, pad, size)
}

/// One pyramid for the run: per level a pass, and in it one scissored draw per member.
fn blur_run(encoder: &mut wgpu::CommandEncoder, gpu: &Gpu, run: &Run, counts: &mut Counters) {
    for step in &gpu.chain {
        let mut blur = pass(encoder, "blur_backdrop", &step.target.view, wgpu::Color::TRANSPARENT);
        blur.set_pipeline(if step.down { &gpu.down } else { &gpu.up });
        blur.set_bind_group(0, &step.binding.group, &[]);
        for member in &run.members {
            let Some(area) = member.region.at_level(step.level, step.dims) else { continue };
            blur.set_scissor_rect(area.x0, area.y0, area.width(), area.height());
            blur.draw(0..3, 0..1);
            counts.blur_pixels += area.area();
        }
        counts.blur_passes += 1;
    }
    counts.blur_runs += 1;
}

struct Target {
    _texture: wgpu::Texture,
    view: wgpu::TextureView,
}
struct Binding {
    uniform: wgpu::Buffer,
    group: wgpu::BindGroup,
}
struct Step {
    target: Target,
    binding: Binding,
    down: bool,
    /// Halvings from the full-size frame to this step's target, and its size.
    level: u32,
    dims: [u32; 2],
}
struct Gpu {
    size: [u32; 2],
    steps: usize,
    composition: Target,
    chain: Vec<Step>,
    presentation: Binding,
    markers: Vec<Binding>,
    down: wgpu::RenderPipeline,
    up: wgpu::RenderPipeline,
    panel: wgpu::RenderPipeline,
    present: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    ready_serial: u64,
}

fn bytes_of(params: &Params) -> [u8; 48] {
    let mut bytes = [0; 48];
    for (out, value) in bytes.as_chunks_mut::<4>().0.iter_mut().zip(params) {
        out.copy_from_slice(&value.to_le_bytes());
    }
    bytes
}

fn target_bytes(size: [u32; 2], steps: usize, max: u32) -> Option<u64> {
    let [w, h] = size;
    if w == 0 || h == 0 || w > max || h > max || !(2..=5).contains(&steps) {
        return None;
    }
    // C is a four-byte UI texture; every down/up target is linear RGBA16F.
    let mut total = u64::from(w).checked_mul(u64::from(h))?.checked_mul(4)?;
    let mut dims = size;
    for _ in 0..steps {
        dims = dims.map(|d| d.div_ceil(2));
        total = total.checked_add(u64::from(dims[0]).checked_mul(u64::from(dims[1]))?.checked_mul(8)?)?;
    }
    dims = size;
    for _ in 0..steps {
        total = total.checked_add(u64::from(dims[0]).checked_mul(u64::from(dims[1]))?.checked_mul(8)?)?;
        dims = dims.map(|d| d.div_ceil(2));
    }
    (total <= BUDGET).then_some(total)
}

fn pass(encoder: &mut wgpu::CommandEncoder, label: &'static str, view: &wgpu::TextureView, clear: wgpu::Color) -> wgpu::RenderPass<'static> {
    attachment_pass(encoder, label, view, wgpu::LoadOp::Clear(clear))
}

fn load_pass(encoder: &mut wgpu::CommandEncoder, label: &'static str, view: &wgpu::TextureView) -> wgpu::RenderPass<'static> {
    attachment_pass(encoder, label, view, wgpu::LoadOp::Load)
}

fn attachment_pass(
    encoder: &mut wgpu::CommandEncoder,
    label: &'static str,
    view: &wgpu::TextureView,
    load: wgpu::LoadOp<wgpu::Color>,
) -> wgpu::RenderPass<'static> {
    encoder
        .begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some(label),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations { load, store: wgpu::StoreOp::Store },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        })
        .forget_lifetime()
}

impl Gpu {
    fn new(device: &wgpu::Device, format: wgpu::TextureFormat, size: [u32; 2], steps: usize, offset: f32, counts: &mut Counters) -> Self {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("blur bindings"),
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
                        min_binding_size: std::num::NonZeroU64::new(48),
                    },
                    count: None,
                },
            ],
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("blur clamp"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("SolidCraft blur"),
            source: wgpu::ShaderSource::Wgsl(include_str!("blur.wgsl").into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("blur pipeline layout"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = |entry: &str, target, blend, layout: &wgpu::PipelineLayout| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(entry),
                layout: Some(layout),
                vertex: wgpu::VertexState { module: &module, entry_point: Some("vertex"), buffers: &[], compilation_options: Default::default() },
                primitive: Default::default(),
                depth_stencil: None,
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some(entry),
                    targets: &[Some(wgpu::ColorTargetState { format: target, blend, write_mask: wgpu::ColorWrites::ALL })],
                    compilation_options: Default::default(),
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let down = pipeline("down", wgpu::TextureFormat::Rgba16Float, None, &pipeline_layout);
        let up = pipeline("up", wgpu::TextureFormat::Rgba16Float, None, &pipeline_layout);
        let panel = pipeline("panel", format, Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING), &pipeline_layout);
        let present = pipeline("present", format, None, &pipeline_layout);
        counts.pipelines_created += 4;
        counts.live_pipelines += 4;
        let composition = Target::new(device, size, format, counts);
        let presentation = Binding::new(device, &layout, &sampler, &composition.view, [0.0; 12], counts);
        let mut chain: Vec<Step> = Vec::new();
        let mut dimensions = Vec::with_capacity(steps + 1);
        dimensions.push(size);
        for _ in 0..steps {
            let last = dimensions.last().copied().unwrap_or(size);
            dimensions.push(last.map(|d| d.div_ceil(2)));
        }
        for (index, dims) in dimensions.iter().skip(1).enumerate() {
            let input = chain.last().map_or(&composition.view, |s| &s.target.view);
            let mut p = [0.0; 12];
            p[0] = offset;
            p[1] = u8::from(index == 0 && !format.is_srgb()) as f32;
            let binding = Binding::new(device, &layout, &sampler, input, p, counts);
            let target = Target::new(device, *dims, wgpu::TextureFormat::Rgba16Float, counts);
            chain.push(Step { target, binding, down: true, level: index as u32 + 1, dims: *dims });
        }
        for (level, dims) in dimensions.iter().take(steps).enumerate().rev() {
            let input = chain.last().map_or(&composition.view, |s| &s.target.view);
            let mut p = [0.0; 12];
            p[0] = offset;
            let binding = Binding::new(device, &layout, &sampler, input, p, counts);
            let target = Target::new(device, *dims, wgpu::TextureFormat::Rgba16Float, counts);
            chain.push(Step { target, binding, down: false, level: level as u32, dims: *dims });
        }
        Self { size, steps, composition, chain, presentation, markers: Vec::new(), down, up, panel, present, layout, sampler, ready_serial: 0 }
    }
    fn grow_markers(&mut self, device: &wgpu::Device, count: usize, counts: &mut Counters) {
        let Some(last) = self.chain.last() else { return };
        while self.markers.len() < count {
            self.markers.push(Binding::new(device, &self.layout, &self.sampler, &last.target.view, [0.0; 12], counts));
        }
    }
}

impl Target {
    fn new(device: &wgpu::Device, size: [u32; 2], format: wgpu::TextureFormat, counts: &mut Counters) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("blur target"),
            size: wgpu::Extent3d { width: size[0], height: size[1], depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        counts.textures_created += 1;
        counts.live_textures += 1;
        let view = texture.create_view(&Default::default());
        Self { _texture: texture, view }
    }
}
impl Binding {
    fn new(
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        sampler: &wgpu::Sampler,
        view: &wgpu::TextureView,
        params: Params,
        counts: &mut Counters,
    ) -> Self {
        use wgpu::util::DeviceExt as _;
        let uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("blur parameters"),
            contents: &bytes_of(&params),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        counts.buffers_created += 1;
        counts.live_buffers += 1;
        counts.live_bytes += 48;
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("blur sample"),
            layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(view) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(sampler) },
                wgpu::BindGroupEntry { binding: 2, resource: uniform.as_entire_binding() },
            ],
        });
        counts.bind_groups_created += 1;
        counts.live_bind_groups += 1;
        Self { uniform, group }
    }
}
