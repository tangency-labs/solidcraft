//! Live backdrop: your content is laid out a second time in a twin egui
//! context (same memory, no input events) and rendered off screen without the
//! glass, then used as the backdrop for this frame. Glass therefore refracts
//! everything drawn beneath it, text included, with no frame of lag. The cost
//! is one extra layout and draw of that content per frame.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use egui::{ClippedPrimitive, Color32, Context, FontDefinitions, Id, Rect, Shape, Ui, UiBuilder};
use egui_wgpu::{CallbackResources, CallbackTrait, RenderState, ScreenDescriptor};

use crate::mipgen::{MipGen, Pyramid, FORMAT};
use crate::renderer::GlassResources;
use crate::LiveBackdropQuality;

/// Screen mapping of this frame's live backdrop, read by the glass widgets.
#[derive(Clone, Copy)]
pub(crate) struct LiveState {
    pub pass_nr: u64,
    pub screen_rect: Rect,
}

fn state_id() -> Id {
    Id::new("egui_glass::live")
}

pub(crate) fn live_state(ctx: &Context) -> Option<LiveState> {
    let state: Option<LiveState> = ctx.data(|d| d.get_temp(state_id()));
    state.filter(|s| s.pass_nr == ctx.cumulative_pass_nr())
}

/// Renders content off screen so glass can refract it. Cheap to clone.
#[derive(Clone)]
pub struct LiveBackdrop {
    twin: Context,
    pending_fonts: Arc<Mutex<Option<FontDefinitions>>>,
}

impl LiveBackdrop {
    /// `fonts` should match what the app installed with `Context::set_fonts`,
    /// so the twin lays text out identically. Create it before [`crate::set_backdrop`]
    /// and before registering images with [`crate::register_native_texture`], so the
    /// off-screen pass can draw them too.
    /// Create one live backdrop per renderer; clones share its off-screen context.
    ///
    /// # Panics
    ///
    /// Panics if [`crate::init`] has not been called on this renderer.
    pub fn new(render_state: &RenderState, fonts: Option<FontDefinitions>) -> Self {
        Self::new_in(render_state, &mut render_state.renderer.write(), fonts)
    }

    pub(crate) fn new_in(render_state: &RenderState, renderer: &mut egui_wgpu::Renderer, fonts: Option<FontDefinitions>) -> Self {
        let twin = Context::default();
        let mipgen = renderer
            .callback_resources
            .get::<GlassResources>()
            .map(|g| g.mipgen.clone())
            .expect("egui_glass::init must be called before LiveBackdrop::new");
        let resources = LiveResources::new(&render_state.device, mipgen);
        renderer.callback_resources.insert(resources);
        Self { twin, pending_fonts: Arc::new(Mutex::new(fonts)) }
    }

    /// Updates the off-screen fonts to match the main egui context.
    pub fn set_fonts(&self, fonts: FontDefinitions) {
        *self.pending_fonts.lock().expect("font update lock poisoned") = Some(fonts);
    }

    /// Shows `add_contents` in `ui` as usual and, this frame only, makes an
    /// off-screen copy of it the backdrop of every glass surface. Draw your
    /// glass outside of `add_contents` (floating areas, later widgets).
    ///
    /// The closure runs for both visible and off-screen layout, potentially more
    /// often if egui requests another pass. Keep non-UI side effects outside it:
    /// do not advance simulations, write files, or send requests on each call.
    /// This adds an extra layout/render and mipmap generation to each frame.
    pub fn run(&self, ui: &mut Ui, clear: Color32, add_contents: impl FnMut(&mut Ui)) {
        self.run_with_quality(ui, clear, LiveBackdropQuality::Full, add_contents);
    }

    /// Like [`Self::run`], with an explicit off-screen resolution for this frame.
    /// Lower quality reduces rasterization and mipmap work, not the extra UI layout.
    /// The visible page, input coordinates and glass geometry remain full resolution.
    /// Quality may change between frames; target textures are reused while sizes match.
    pub fn run_with_quality(&self, ui: &mut Ui, clear: Color32, quality: LiveBackdropQuality, mut add_contents: impl FnMut(&mut Ui)) {
        let ctx = ui.ctx().clone();
        let slot = ui.painter().add(Shape::Noop);
        add_contents(ui);

        // Twin pass: identical memory, identical ui id/rect, no events.
        let mut input = ctx.input(|i| i.raw.clone());
        input.events.clear();
        input.dropped_files.clear();
        input.hovered_files.clear();
        self.twin.memory_mut(|m| *m = ctx.memory(|m| m.clone()));
        // Copying main memory erases the twin's queued font update; install it afterwards.
        if let Some(fonts) = self.pending_fonts.lock().expect("font update lock poisoned").take() {
            self.twin.set_fonts(fonts);
        }
        let (id, rect, clip, layer, layout) = (ui.id(), ui.max_rect(), ui.clip_rect(), ui.layer_id(), *ui.layout());
        let twin = self.twin.clone();
        let output = self.twin.run_ui(input, |_root| {
            let mut ui = Ui::new(twin.clone(), id, UiBuilder::new().layer_id(layer).max_rect(rect).layout(layout));
            ui.set_clip_rect(clip);
            add_contents(&mut ui);
        });
        let primitives = self.twin.tessellate(output.shapes, output.pixels_per_point);

        let frame = FrameData { primitives, textures_delta: output.textures_delta, clear, quality };
        let callback = LiveCallback { frame: Mutex::new(Some(frame)) };
        ui.painter().set(slot, Shape::Callback(egui_wgpu::Callback::new_paint_callback(rect, callback)));
        let state = LiveState { pass_nr: ctx.cumulative_pass_nr(), screen_rect: ctx.viewport_rect() };
        ctx.data_mut(|d| d.insert_temp(state_id(), state));
    }
}

struct FrameData {
    primitives: Vec<ClippedPrimitive>,
    textures_delta: egui::TexturesDelta,
    clear: Color32,
    quality: LiveBackdropQuality,
}

struct LiveCallback {
    frame: Mutex<Option<FrameData>>,
}

impl CallbackTrait for LiveCallback {
    fn prepare(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        screen: &ScreenDescriptor,
        encoder: &mut wgpu::CommandEncoder,
        resources: &mut CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        let Some(frame) = self.frame.lock().ok().and_then(|mut f| f.take()) else { return Vec::new() };
        let Some(res) = resources.get_mut::<LiveResources>() else { return Vec::new() };
        let mut frame = frame;
        // Native textures live in the app's renderer under different ids; remap the ones we know.
        for primitive in &mut frame.primitives {
            if let egui::epaint::Primitive::Mesh(mesh) = &mut primitive.primitive {
                if let Some(id) = res.texture_map.get(&mesh.texture_id) {
                    mesh.texture_id = *id;
                }
            }
        }
        if screen.size_in_pixels.contains(&0) {
            return Vec::new();
        }
        let scaled_screen = frame.quality.screen(screen);
        let [w, h] = scaled_screen.size_in_pixels;
        res.ensure_target(device, w, h);
        let target = res.target.as_ref().expect("target created above");
        let mipgen = res.mipgen.clone();

        // SolidCraft: egui 0.36 queues several deltas per texture.
        for (id, deltas) in &frame.textures_delta.set {
            for delta in deltas {
                res.renderer.update_texture(device, queue, *id, delta);
            }
        }
        // Keep the original projection: rounding odd target sizes must not shift the page.
        let commands = res.renderer.update_buffers(device, queue, encoder, &frame.primitives, screen);
        let clip_scale = egui::vec2(
            w as f32 / screen.size_in_pixels[0] as f32,
            h as f32 / screen.size_in_pixels[1] as f32,
        ) / frame.quality.scale();
        for primitive in &mut frame.primitives {
            primitive.clip_rect = Rect::from_min_max(
                (primitive.clip_rect.min.to_vec2() * clip_scale).to_pos2(),
                (primitive.clip_rect.max.to_vec2() * clip_scale).to_pos2(),
            );
        }
        {
            let clear = egui::Rgba::from(frame.clear);
            let mut pass = encoder
                .begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("egui_glass_live"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &target.mip_views[0],
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color { r: clear.r() as f64, g: clear.g() as f64, b: clear.b() as f64, a: 1.0 }),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    ..Default::default()
                })
                .forget_lifetime();
            res.renderer.render(&mut pass, &frame.primitives, &scaled_screen);
        }
        for id in &frame.textures_delta.free {
            res.renderer.free_texture(id);
        }
        mipgen.generate(encoder, target);
        let (sample_view, mips) = (target.sample_view.clone(), target.mip_levels());
        let pass_nr = res.pass_counter;
        res.pass_counter += 1;
        if let Some(glass) = resources.get_mut::<GlassResources>() {
            glass.set_live_backdrop(sample_view, mips, pass_nr, frame.quality.scale());
        }
        commands
    }

    fn paint(&self, _info: egui::PaintCallbackInfo, _pass: &mut wgpu::RenderPass<'static>, _resources: &CallbackResources) {}
}

pub(crate) struct LiveResources {
    renderer: egui_wgpu::Renderer,
    /// App renderer texture id -> id of the same texture in the twin renderer.
    texture_map: HashMap<egui::TextureId, egui::TextureId>,
    mipgen: std::sync::Arc<MipGen>,
    target: Option<Pyramid>,
    /// Counts live frames; glass resources use it to know the live view is current.
    pass_counter: u64,
}

impl LiveResources {
    fn new(device: &wgpu::Device, mipgen: std::sync::Arc<MipGen>) -> Self {
        let renderer = egui_wgpu::Renderer::new(device, FORMAT, egui_wgpu::RendererOptions::default());
        Self { renderer, texture_map: HashMap::new(), mipgen, target: None, pass_counter: 0 }
    }

    /// Mirrors a native texture of the app's renderer (`app_id`) into the twin renderer.
    pub(crate) fn register_native_texture(&mut self, device: &wgpu::Device, view: &wgpu::TextureView, app_id: egui::TextureId) {
        let id = self.renderer.register_native_texture(device, view, wgpu::FilterMode::Linear);
        self.texture_map.insert(app_id, id);
    }

    pub(crate) fn free_texture(&mut self, app_id: &egui::TextureId) {
        if let Some(id) = self.texture_map.remove(app_id) {
            self.renderer.free_texture(&id);
        }
    }

    fn ensure_target(&mut self, device: &wgpu::Device, w: u32, h: u32) {
        if self.target.as_ref().is_some_and(|t| t.size == [w, h]) {
            return;
        }
        self.target = Some(self.mipgen.create(device, w, h, wgpu::TextureUsages::empty()));
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    pub fn contains_texture(renderer: &egui_wgpu::Renderer, id: egui::TextureId) -> bool {
        let resources = renderer.callback_resources.get::<super::LiveResources>().unwrap();
        resources.texture_map.get(&id).is_some_and(|mapped| resources.renderer.texture(mapped).is_some())
    }
}
