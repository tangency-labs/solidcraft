use super::*;
use crate::gpu::{Renderer as ViewRenderer, ViewportCallback};
use egui_wgpu::{CallbackResources, CallbackTrait, FrameCompositor, ScreenDescriptor, wgpu};
use std::cell::RefCell;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex};

thread_local! { static CURRENT: RefCell<Option<Arc<Shared>>> = const { RefCell::new(None) }; }
thread_local! { static SURFACE_ALPHA: std::cell::Cell<Option<u8>> = const { std::cell::Cell::new(None) }; }
const BUDGET: u64 = 256 * 1024 * 1024;
const MAX_MARKERS: usize = 256;
type Params = [f32; 24];

fn capability(adapter: &wgpu::Adapter, format: wgpu::TextureFormat, split: bool, msaa: u32) -> Capability {
    if solidcraft_gpu::select::tier_of(adapter.get_info().device_type) == solidcraft_gpu::select::Tier::Software {
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
        *self.capability.get_or_init(|| capability(&self.adapter, self.format, self.split, self.msaa))
    }
}

pub struct State {
    radius: f32,
    host: Option<Host>,
    active: Option<Arc<Shared>>,
    retired: Counters,
    source_supported: bool,
    failure: Option<&'static str>,
    surface_alpha: Option<u8>,
    last_size: Option<[u32; 2]>,
    last_background: Option<[Color32; 3]>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            radius: 0.0,
            host: None,
            active: None,
            retired: Counters::default(),
            source_supported: true,
            failure: None,
            surface_alpha: None,
            last_size: None,
            last_background: None,
        }
    }
}

impl State {
    pub fn install(&mut self, rs: &egui_wgpu::RenderState, split: bool) {
        self.retire();
        self.failure = None;
        self.host = Some(Host {
            adapter: rs.adapter.clone(),
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
            if let Ok(mut inner) = shared.inner.lock() {
                self.retired = inner.counts;
                self.retired.clear_live();
                self.failure = inner.failure.or(self.failure);
                inner.gpu = None;
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
    pub fn capability(&self) -> Capability {
        let base = self.base_capability();
        if base == Capability::Available && self.active.as_ref().is_some_and(|s| s.inner.lock().is_ok_and(|i| i.missing_source)) {
            return Capability::Unavailable("no viewport source in the last painted frame");
        }
        base
    }
    pub fn capability_for(&self, viewport: egui::ViewportId) -> Capability {
        if viewport != egui::ViewportId::ROOT {
            return Capability::Unavailable("native pop-out source integration unavailable");
        }
        self.capability()
    }
    fn base_capability(&self) -> Capability {
        let Some(host) = &self.host else { return Capability::Unavailable("no GPU host") };
        if host.lost.is_lost() {
            return Capability::Unavailable("device lost");
        }
        let available = host.capability();
        if available != Capability::Available {
            return available;
        }
        if !self.source_supported {
            return Capability::Unavailable("workspace has no viewport source recipe");
        }
        if let Some(reason) = self.failure {
            return Capability::Unavailable(reason);
        }
        if let Some(shared) = &self.active {
            return match shared.inner.lock() {
                Ok(inner) => inner.failure.map_or(Capability::Available, Capability::Unavailable),
                Err(_) => Capability::Unavailable("blur state unavailable"),
            };
        }
        Capability::Available
    }
    pub fn counters(&self) -> Counters {
        self.active.as_ref().and_then(|s| s.inner.lock().ok().map(|i| i.counts)).unwrap_or(self.retired)
    }
    #[cfg(test)]
    pub(crate) fn source_texture(&self) -> Option<wgpu::Texture> {
        let inner = self.active.as_ref()?.inner.lock().ok()?;
        Some(inner.gpu.as_ref()?.source._texture.clone())
    }
    #[cfg(test)]
    pub(crate) fn source_geometry(&self) -> Option<[Rect; 3]> {
        self.active.as_ref()?.inner.lock().ok()?.geometry
    }
    /// Internal capture tint only; deliberately not a user preference.
    pub fn set_surface_alpha(&mut self, alpha: Option<u8>) {
        self.surface_alpha = alpha;
    }

    pub fn begin(&mut self, ctx: &egui::Context, source_supported: bool) -> Scope {
        let previous = CURRENT.with(|c| c.replace(None));
        let alpha = SURFACE_ALPHA.with(|a| a.replace(self.surface_alpha));
        if ctx.viewport_id() != egui::ViewportId::ROOT {
            return Scope { previous, alpha };
        }
        self.source_supported = source_supported;
        if self.radius == 0.0 {
            self.retire();
            return Scope { previous, alpha };
        }
        let colors = crate::theme::Tokens::get();
        let background = [colors.viewport_top, colors.viewport_bottom, colors.app_bar];
        if self.last_background != Some(background) {
            let failed = self.failure.is_some() || self.active.as_ref().is_some_and(|s| s.inner.lock().is_ok_and(|i| i.failure.is_some()));
            if failed {
                self.retire();
                self.failure = None;
            }
            self.last_background = Some(background);
        }
        let size = ctx.content_rect().size() * ctx.pixels_per_point();
        if size.is_finite() && size.x > 0.0 && size.y > 0.0 {
            let size_key = [size.x.round() as u32, size.y.round() as u32];
            if self.last_size != Some(size_key) {
                self.retire();
                self.failure = None;
                self.last_size = Some(size_key);
            }
        }
        if self.base_capability() != Capability::Available || !size.is_finite() || size.x <= 0.0 || size.y <= 0.0 {
            self.retire();
            return Scope { previous, alpha };
        }
        let Some(host) = &self.host else { return Scope { previous, alpha } };
        let shared = self.active.get_or_insert_with(|| {
            Arc::new(Shared {
                host: host.clone(),
                radius: self.radius,
                painting_owner: std::sync::atomic::AtomicBool::new(true),
                inner: Mutex::new(Inner { counts: self.retired, ..Inner::default() }),
            })
        });
        if let Ok(mut inner) = shared.inner.lock() {
            inner.markers.clear();
            inner.serial = inner.serial.wrapping_add(1);
            inner.source_seen = false;
            inner.source_count = 0;
        }
        if let Some(renderer) = host.renderer.upgrade() {
            renderer.write().frame_compositor = Some(shared.clone());
        }
        CURRENT.with(|c| *c.borrow_mut() = Some(shared.clone()));
        Scope { previous, alpha }
    }
}

/// Select production policy or bypass only its software rejection for rendering fixtures.
#[cfg(test)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum FixturePolicy {
    Production,
    Rendering,
}

#[cfg(test)]
pub(crate) fn fixture_state(
    adapter: &wgpu::Adapter,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    renderer: &Arc<egui::epaint::mutex::RwLock<egui_wgpu::Renderer>>,
    format: wgpu::TextureFormat,
    policy: FixturePolicy,
) -> State {
    State {
        radius: 0.0,
        host: Some(Host {
            adapter: adapter.clone(),
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
        source_supported: true,
        failure: None,
        surface_alpha: None,
        last_size: None,
        last_background: None,
    }
}

#[cfg(test)]
pub(crate) fn fixture_budget(size: [u32; 2], steps: usize) -> Option<u64> {
    target_bytes(size, steps, 16384)
}
impl Drop for State {
    fn drop(&mut self) {
        self.retire();
    }
}

/// Restores the previous context even on early UI returns. No heap allocation.
pub struct Scope {
    previous: Option<Arc<Shared>>,
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
    inner: Mutex<Inner>,
    painting_owner: std::sync::atomic::AtomicBool,
}
#[derive(Default)]
struct Inner {
    gpu: Option<Gpu>,
    markers: Vec<Marker>,
    counts: Counters,
    failure: Option<&'static str>,
    serial: u64,
    source_seen: bool,
    screen: Option<([u32; 2], f32)>,
    missing_source: bool,
    source_count: u16,
    #[cfg(test)]
    geometry: Option<[Rect; 3]>,
}
#[derive(Clone, Copy)]
struct Marker {
    rect: Rect,
    corners: egui::CornerRadius,
    opacity: f32,
    snap: bool,
}

pub fn decorate(painter: &Painter, frame: Frame, content: Rect, ordinary: Shape) -> Shape {
    if frame.fill.a() == 255 || frame.fill.a() == 0 || !painter.is_visible() {
        return ordinary;
    }
    CURRENT.with(|c| {
        let kept = c.borrow();
        let Some(shared) = kept.as_ref() else { return ordinary };
        if painter.ctx().viewport_id() != egui::ViewportId::ROOT {
            return ordinary;
        }
        let rect = frame.widget_rect(content);
        if !rect.is_finite() || !rect.is_positive() {
            return ordinary;
        }
        let index = if let Ok(mut inner) = shared.inner.lock() {
            if inner.markers.len() >= MAX_MARKERS {
                inner.failure = Some("blur panel limit exceeded");
                inner.gpu = None;
                inner.counts.clear_live();
                return ordinary;
            }
            let index = inner.markers.len();
            inner.markers.push(Marker {
                rect,
                corners: frame.corner_radius,
                opacity: painter.opacity(),
                snap: painter.ctx().tessellation_options(|o| o.round_rects_to_pixels),
            });
            inner.counts.markers_emitted += 1;
            index
        } else {
            return ordinary;
        };
        let callback = egui_wgpu::Callback::new_paint_callback(rect, PanelMarker { shared: shared.clone(), index });
        Shape::Vec(vec![Shape::Callback(callback), ordinary])
    })
}

pub fn viewport(painter: &Painter, rect: Rect, cb: ViewportCallback, background: Rect, colors: [Color32; 2]) {
    CURRENT.with(|c| {
        if let Some(shared) = c.borrow().as_ref() {
            if let Ok(mut inner) = shared.inner.lock() {
                inner.source_count = inner.source_count.saturating_add(1);
            }
            let outside = crate::theme::Tokens::get().app_bar;
            painter.add(egui_wgpu::Callback::new_paint_callback(
                rect,
                Source {
                    cb,
                    rect,
                    clip: painter.clip_rect(),
                    background,
                    colors,
                    outside,
                    context: painter.ctx().clone(),
                    layer: painter.layer_id(),
                    shared: shared.clone(),
                },
            ));
        } else {
            painter.add(egui_wgpu::Callback::new_paint_callback(rect, cb));
        }
    });
}

struct Source {
    cb: ViewportCallback,
    rect: Rect,
    clip: Rect,
    background: Rect,
    colors: [Color32; 2],
    outside: Color32,
    context: egui::Context,
    layer: egui::LayerId,
    shared: Arc<Shared>,
}
impl CallbackTrait for Source {
    fn prepare(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        screen: &ScreenDescriptor,
        encoder: &mut wgpu::CommandEncoder,
        resources: &mut CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        self.cb.prepare(device, queue, screen, encoder, resources)
    }
    fn finish_prepare(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        resources: &mut CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        let Some(view) = resources.get::<ViewRenderer>() else { return Vec::new() };
        if !self.shared.painting_owner.load(std::sync::atomic::Ordering::Relaxed) {
            return Vec::new();
        }
        // Physical surface clamping is supplied by the pass owner, not inferred from DPI.
        let Ok(mut inner) = self.shared.inner.lock() else { return Vec::new() };
        let Some((size, scale)) = inner.screen else { return Vec::new() };
        let screen = ScreenDescriptor { size_in_pixels: size, pixels_per_point: scale };
        if self.shared.host.lost.is_lost() || inner.failure.is_some() {
            return Vec::new();
        }
        // Read once after UI emission/output processing; transforms can be set after
        // this callback was emitted. The callback shape itself stays in local space.
        let Some([rect, clip, background]) = self.effective_geometry() else {
            inner.failure = Some("invalid viewport source geometry");
            inner.gpu = None;
            inner.counts.clear_live();
            return Vec::new();
        };
        #[cfg(test)]
        {
            inner.geometry = Some([rect, clip, background]);
        }
        if self.colors.iter().any(|color| color.a() != 255) || self.outside.a() != 255 {
            inner.failure = Some("translucent viewport/background source unsupported");
            inner.gpu = None;
            inner.counts.clear_live();
            return Vec::new();
        }
        if screen.size_in_pixels.contains(&0) || !screen.pixels_per_point.is_finite() || screen.pixels_per_point <= 0.0 {
            inner.gpu = None;
            inner.counts.clear_live();
            return Vec::new();
        }
        if inner.source_seen || inner.source_count != 1 {
            inner.failure = Some("multiple viewport sources in one frame");
            inner.gpu = None;
            inner.counts.clear_live();
            return Vec::new();
        }
        inner.source_seen = true;
        let Some((steps, offset)) = schedule(self.shared.radius) else { return Vec::new() };
        let Some(bytes) = target_bytes(screen.size_in_pixels, steps, self.shared.host.device.limits().max_texture_dimension_2d) else {
            inner.failure = Some("blur target is empty, exceeds device limits or memory budget");
            inner.gpu = None;
            inner.counts.clear_live();
            return Vec::new();
        };
        let replace = inner.gpu.as_ref().is_none_or(|g| g.size != screen.size_in_pixels || g.steps != steps);
        if replace {
            inner.gpu = None;
            inner.counts.clear_live();
            let (gpu, failure) = egui_wgpu::capture_errors(device, || {
                Gpu::new(device, self.shared.host.format, screen.size_in_pixels, steps, offset, &mut inner.counts)
            });
            if let Some(error) = failure {
                log::warn!("blur resources refused: {error}");
                inner.failure = Some("GPU refused blur resources");
                inner.counts.clear_live();
                return Vec::new();
            }
            inner.gpu = Some(gpu);
            inner.counts.live_bytes += bytes;
        }
        let key = self.key(&screen, [rect, clip, background]);
        let serial = inner.serial;
        let marker_count = inner.markers.len();
        let Inner { gpu, counts, .. } = &mut *inner;
        let Some(gpu) = gpu else { return Vec::new() };
        gpu.ready_serial = serial;
        let (_, failed) = egui_wgpu::capture_errors(device, || gpu.grow_markers(device, marker_count, counts));
        if let Some(error) = failed {
            log::warn!("blur marker resources refused: {error}");
            inner.failure = Some("GPU refused blur marker resources");
            inner.gpu = None;
            inner.counts.clear_live();
            return Vec::new();
        }
        if gpu.valid == Some(key) {
            counts.cache_hits += 1;
            return Vec::new();
        }
        let params = background_params(background, clip, self.colors, self.outside, screen.pixels_per_point, self.shared.host.format.is_srgb());
        if params.iter().any(|value| !value.is_finite()) {
            inner.failure = Some("invalid physical viewport source geometry");
            inner.gpu = None;
            inner.counts.clear_live();
            return Vec::new();
        }
        queue.write_buffer(&gpu.background.uniform, 0, &bytes_of(&params));
        {
            let mut pass = pass(encoder, "blur_source", &gpu.source.view, wgpu::Color::BLACK);
            pass.set_pipeline(&gpu.background_pipeline);
            pass.set_bind_group(0, &gpu.background.group, &[]);
            pass.draw(0..3, 0..1);
            let viewport = pixel_rect(rect, screen.pixels_per_point, screen.size_in_pixels);
            let clipped = pixel_rect(rect.intersect(clip), screen.pixels_per_point, screen.size_in_pixels);
            if let (Some([x, y, w, h]), Some([cx, cy, cw, ch])) = (viewport, clipped) {
                pass.set_scissor_rect(cx, cy, cw, ch);
                view.paint(&self.cb.0, &mut pass, [x as f32, y as f32, w as f32, h as f32]);
            }
        }
        counts.source_passes += 1;
        for step in &gpu.chain {
            let mut pass = pass(encoder, "blur_kawase", &step.target.view, wgpu::Color::TRANSPARENT);
            pass.set_pipeline(if step.down { &gpu.down } else { &gpu.up });
            pass.set_bind_group(0, &step.binding.group, &[]);
            pass.draw(0..3, 0..1);
            counts.blur_passes += 1;
        }
        gpu.pending = Some(key);
        Vec::new()
    }
    fn paint(&self, info: egui::PaintCallbackInfo, pass: &mut wgpu::RenderPass<'static>, resources: &CallbackResources) {
        self.cb.paint(info, pass, resources);
    }
}

impl Source {
    fn effective_geometry(&self) -> Option<[Rect; 3]> {
        let transform = self.context.layer_transform_to_global(self.layer).unwrap_or(egui::emath::TSTransform::IDENTITY);
        if !transform.translation.is_finite() || !transform.scaling.is_finite() || transform.scaling <= 0.0 {
            return None;
        }
        let geometry = [self.rect, self.clip, self.background].map(|r| transform.mul_rect(r));
        (geometry.iter().all(|r| r.is_finite()) && geometry[0].is_positive()).then_some(geometry)
    }

    fn key(&self, screen: &ScreenDescriptor, geometry: [Rect; 3]) -> u64 {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        solidcraft_gpu::view::frame_key(&self.cb.0).hash(&mut h);
        for r in geometry {
            [r.min.x, r.min.y, r.max.x, r.max.y].map(f32::to_bits).hash(&mut h);
        }
        self.colors.hash(&mut h);
        self.outside.hash(&mut h);
        screen.size_in_pixels.hash(&mut h);
        screen.pixels_per_point.to_bits().hash(&mut h);
        self.shared.radius.to_bits().hash(&mut h);
        self.shared.host.format.hash(&mut h);
        h.finish()
    }
}

struct PanelMarker {
    shared: Arc<Shared>,
    index: usize,
}
impl CallbackTrait for PanelMarker {
    fn paint(&self, info: egui::PaintCallbackInfo, pass: &mut wgpu::RenderPass<'static>, _: &CallbackResources) {
        let Ok(mut inner) = self.shared.inner.lock() else { return };
        if !self.shared.painting_owner.load(std::sync::atomic::Ordering::Relaxed) {
            return;
        }
        if inner.failure.is_some() || self.shared.host.lost.is_lost() {
            return;
        }
        let Some(marker) = inner.markers.get(self.index).copied() else { return };
        let Some(gpu) = inner.gpu.as_ref() else { return };
        if gpu.ready_serial != inner.serial || !inner.source_seen {
            return;
        }
        let Some(binding) = gpu.markers.get(self.index) else { return };
        let mut p = [0.0; 24];
        p[2] = u8::from(self.shared.host.format.is_srgb()) as f32;
        p[3] = marker.opacity;
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

impl FrameCompositor for Shared {
    fn begin_viewport(&self, viewport: u64) {
        self.painting_owner.store(viewport == egui::ViewportId::ROOT.0.value(), std::sync::atomic::Ordering::Relaxed);
    }
    fn screen(&self, screen: &ScreenDescriptor, _: &mut CallbackResources) {
        if !self.painting_owner.load(std::sync::atomic::Ordering::Relaxed) {
            return;
        }
        if let Ok(mut inner) = self.inner.lock() {
            inner.screen = Some((screen.size_in_pixels, screen.pixels_per_point));
        }
    }
    fn suspend(&self) {
        if !self.host.lost.is_lost() && !self.painting_owner.load(std::sync::atomic::Ordering::Relaxed) {
            return;
        }
        if let Ok(mut inner) = self.inner.lock() {
            inner.gpu = None;
            inner.counts.clear_live();
            inner.source_seen = false;
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
        if !self.painting_owner.load(std::sync::atomic::Ordering::Relaxed) {
            return false;
        }
        let (composition, pipeline, binding) = {
            let Ok(mut inner) = self.inner.lock() else { return false };
            if !inner.source_seen {
                inner.missing_source = true;
                inner.gpu = None;
                inner.counts.clear_live();
                return false;
            }
            inner.missing_source = false;
            if inner.failure.is_some() {
                return false;
            }
            let Some(gpu) = inner.gpu.as_ref() else { return false };
            if gpu.ready_serial != inner.serial || gpu.size != screen.size_in_pixels {
                return false;
            }
            let data = (gpu.composition.view.clone(), gpu.present.clone(), gpu.presentation.group.clone());
            inner.counts.composition_passes += 1;
            data
        };
        {
            let color = wgpu::Color { r: clear[0] as f64, g: clear[1] as f64, b: clear[2] as f64, a: clear[3] as f64 };
            let mut ui = pass(encoder, "blur_ui_composition", &composition, color);
            renderer.render(&mut ui, jobs, screen);
        }
        {
            let mut present = pass(encoder, "blur_present", target, wgpu::Color::TRANSPARENT);
            present.set_pipeline(&pipeline);
            present.set_bind_group(0, &binding, &[]);
            present.draw(0..3, 0..1);
        }
        if let Ok(mut inner) = self.inner.lock() {
            inner.counts.present_passes += 1;
        }
        true
    }
    fn submitted(&self) {
        if !self.painting_owner.load(std::sync::atomic::Ordering::Relaxed) {
            return;
        }
        if let Ok(mut inner) = self.inner.lock() {
            if self.host.lost.is_lost() {
                inner.gpu = None;
                inner.counts.clear_live();
                inner.failure = Some("device lost");
            } else if let Some(gpu) = inner.gpu.as_mut()
                && let Some(key) = gpu.pending.take()
            {
                gpu.valid = Some(key);
            }
        }
    }
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
}
struct Gpu {
    size: [u32; 2],
    steps: usize,
    source: Target,
    composition: Target,
    chain: Vec<Step>,
    background: Binding,
    presentation: Binding,
    markers: Vec<Binding>,
    background_pipeline: wgpu::RenderPipeline,
    down: wgpu::RenderPipeline,
    up: wgpu::RenderPipeline,
    panel: wgpu::RenderPipeline,
    present: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    valid: Option<u64>,
    pending: Option<u64>,
    ready_serial: u64,
}

fn bytes_of(params: &Params) -> [u8; 96] {
    let mut bytes = [0; 96];
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
    // S/C are four-byte UI textures; every down/up target is linear RGBA16F.
    let mut total = u64::from(w).checked_mul(u64::from(h))?.checked_mul(8)?;
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

fn pixel_rect(rect: Rect, scale: f32, size: [u32; 2]) -> Option<[u32; 4]> {
    if !rect.is_finite() || !rect.is_positive() || !scale.is_finite() || scale <= 0.0 {
        return None;
    }
    let x = (rect.min.x * scale).round().clamp(0.0, size[0] as f32) as u32;
    let y = (rect.min.y * scale).round().clamp(0.0, size[1] as f32) as u32;
    let right = (rect.max.x * scale).round().clamp(x as f32, size[0] as f32) as u32;
    let bottom = (rect.max.y * scale).round().clamp(y as f32, size[1] as f32) as u32;
    (right > x && bottom > y).then_some([x, y, right - x, bottom - y])
}

fn background_params(rect: Rect, clip: Rect, colors: [Color32; 2], outside: Color32, scale: f32, srgb: bool) -> Params {
    let mut p = [0.0; 24];
    p[2] = u8::from(srgb) as f32;
    p[4..8].copy_from_slice(&[rect.min.x * scale, rect.min.y * scale, rect.max.x * scale, rect.max.y * scale]);
    p[8..12].copy_from_slice(&[clip.min.x * scale, clip.min.y * scale, clip.max.x * scale, clip.max.y * scale]);
    for (out, color) in p[12..24].as_chunks_mut::<4>().0.iter_mut().zip(colors.into_iter().chain([outside])) {
        out.copy_from_slice(&color.to_normalized_gamma_f32());
    }
    p
}

fn pass(encoder: &mut wgpu::CommandEncoder, label: &'static str, view: &wgpu::TextureView, clear: wgpu::Color) -> wgpu::RenderPass<'static> {
    encoder
        .begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some(label),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Clear(clear), store: wgpu::StoreOp::Store },
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
                        min_binding_size: std::num::NonZeroU64::new(96),
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
        let background_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("blur background uniform only"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 2,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: std::num::NonZeroU64::new(96),
                },
                count: None,
            }],
        });
        let background_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("blur source cannot sample composition"),
            bind_group_layouts: &[Some(&background_layout)],
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
        let background_pipeline = pipeline("background", format, None, &background_pipeline_layout);
        counts.pipelines_created += 5;
        counts.live_pipelines += 5;
        let source = Target::new(device, size, format, counts);
        let composition = Target::new(device, size, format, counts);
        let background = Binding::background(device, &background_layout, counts);
        let presentation = Binding::new(device, &layout, &sampler, &composition.view, [0.0; 24], counts);
        let mut chain: Vec<Step> = Vec::new();
        let mut dimensions = Vec::with_capacity(steps + 1);
        dimensions.push(size);
        for _ in 0..steps {
            let last = dimensions.last().copied().unwrap_or(size);
            dimensions.push(last.map(|d| d.div_ceil(2)));
        }
        for (index, dims) in dimensions.iter().skip(1).enumerate() {
            let input = chain.last().map_or(&source.view, |s| &s.target.view);
            let mut p = [0.0; 24];
            p[0] = offset;
            p[1] = u8::from(index == 0 && !format.is_srgb()) as f32;
            let binding = Binding::new(device, &layout, &sampler, input, p, counts);
            let target = Target::new(device, *dims, wgpu::TextureFormat::Rgba16Float, counts);
            chain.push(Step { target, binding, down: true });
        }
        for dims in dimensions.iter().take(steps).rev() {
            let input = chain.last().map_or(&source.view, |s| &s.target.view);
            let mut p = [0.0; 24];
            p[0] = offset;
            let binding = Binding::new(device, &layout, &sampler, input, p, counts);
            let target = Target::new(device, *dims, wgpu::TextureFormat::Rgba16Float, counts);
            chain.push(Step { target, binding, down: false });
        }
        Self {
            size,
            steps,
            source,
            composition,
            chain,
            background,
            presentation,
            markers: Vec::new(),
            background_pipeline,
            down,
            up,
            panel,
            present,
            layout,
            sampler,
            valid: None,
            pending: None,
            ready_serial: 0,
        }
    }
    fn grow_markers(&mut self, device: &wgpu::Device, count: usize, counts: &mut Counters) {
        let Some(last) = self.chain.last() else { return };
        while self.markers.len() < count {
            self.markers.push(Binding::new(device, &self.layout, &self.sampler, &last.target.view, [0.0; 24], counts));
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
    fn background(device: &wgpu::Device, layout: &wgpu::BindGroupLayout, counts: &mut Counters) -> Self {
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("blur background parameters"),
            size: 96,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        counts.buffers_created += 1;
        counts.live_buffers += 1;
        counts.live_bytes += 96;
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("blur background has no input texture"),
            layout,
            entries: &[wgpu::BindGroupEntry { binding: 2, resource: uniform.as_entire_binding() }],
        });
        counts.bind_groups_created += 1;
        counts.live_bind_groups += 1;
        Self { uniform, group }
    }
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
        counts.live_bytes += 96;
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
