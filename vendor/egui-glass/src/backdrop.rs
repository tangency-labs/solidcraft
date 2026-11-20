use egui::{Color32, ColorImage, Context, Id, Rect, TextureId, Ui, Vec2};
use egui_wgpu::RenderState;

use crate::live::LiveResources;
use crate::renderer::GlassResources;

/// Where the backdrop image is mapped on screen. Stored in egui memory so
/// widgets can find it without touching wgpu.
#[derive(Clone, Copy, Debug)]
pub(crate) struct BackdropState {
    pub texture: TextureId,
    pub size: Vec2,
    /// Screen rect (points) the *whole* image is mapped to.
    pub rect: Rect,
    /// Colour glass shows where it samples outside `rect`.
    pub fill: Color32,
}

fn state_id() -> Id {
    Id::new("egui_glass::backdrop")
}

pub(crate) fn backdrop_state(ctx: &Context) -> Option<BackdropState> {
    ctx.data(|d| d.get_temp(state_id()))
}

/// Screen rect (points) the whole backdrop image is currently mapped to, as
/// recorded by the last [`show_backdrop`]. Useful to align content with it.
pub fn backdrop_rect(ctx: &Context) -> Option<Rect> {
    backdrop_state(ctx).map(|s| s.rect)
}

/// Uploads `image` as the backdrop every glass surface refracts, replacing any
/// previous one. Returns the egui texture id so the app can draw it.
/// The previous backdrop's texture id is freed and must no longer be used.
/// Image dimensions and pixel storage must be consistent and fit the device's
/// texture limits. Create [`crate::LiveBackdrop`] before uploading if needed.
///
/// # Panics
///
/// Panics if [`crate::init`] has not been called. Invalid images or dimensions
/// exceeding the GPU's limits can produce wgpu validation errors.
pub fn set_backdrop(ctx: &Context, render_state: &RenderState, image: &ColorImage) -> TextureId {
    let [w, h] = [image.size[0].max(1) as u32, image.size[1].max(1) as u32];
    let mut renderer = render_state.renderer.write();
    let mipgen =
        renderer.callback_resources.get::<GlassResources>().map(|g| g.mipgen.clone()).expect("egui_glass::init must be called before set_backdrop");
    let pyramid = mipgen.create(&render_state.device, w, h, wgpu::TextureUsages::COPY_DST);
    let pixels: Vec<u8> = if image.pixels.is_empty() { vec![0; 4] } else { image.pixels.iter().flat_map(|c| c.to_array()).collect() };
    render_state.queue.write_texture(
        wgpu::TexelCopyTextureInfo { texture: &pyramid.texture, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
        &pixels,
        wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(4 * w), rows_per_image: Some(h) },
        wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
    );
    let mut encoder = render_state.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("egui_glass_backdrop_mips") });
    mipgen.generate(&mut encoder, &pyramid);
    render_state.queue.submit([encoder.finish()]);
    let view = pyramid.sample_view.clone();
    let mip_levels = pyramid.mip_levels();

    if let Some(previous) = backdrop_state(ctx) {
        free_native_texture(&mut renderer, &previous.texture);
    }
    let texture_id = register_native_texture_in(&mut renderer, &render_state.device, &view);
    let resources: &mut GlassResources = renderer.callback_resources.get_mut().expect("egui_glass::init must be called before set_backdrop");
    resources.set_backdrop(&render_state.device, view, mip_levels);
    drop(renderer);

    let state = BackdropState { texture: texture_id, size: Vec2::new(w as f32, h as f32), rect: Rect::ZERO, fill: Color32::TRANSPARENT };
    ctx.data_mut(|d| d.insert_temp(state_id(), state));
    texture_id
}

/// Registers a wgpu texture for drawing with egui, in the app's renderer and
/// (if a [`crate::LiveBackdrop`] exists) in its off-screen renderer too, so
/// images drawn with it also show through live glass.
/// Create the live backdrop first; textures registered earlier are not mirrored
/// retroactively. Release the returned id with [`free_native_texture_id`].
pub fn register_native_texture(render_state: &RenderState, view: &wgpu::TextureView) -> TextureId {
    register_native_texture_in(&mut render_state.renderer.write(), &render_state.device, view)
}

/// Frees a texture registered with [`register_native_texture`].
/// Stop drawing with the id before freeing it. Do not free the active backdrop
/// returned by [`set_backdrop`]; replacing the backdrop frees that id automatically.
pub fn free_native_texture_id(render_state: &RenderState, id: &TextureId) {
    free_native_texture(&mut render_state.renderer.write(), id);
}

fn register_native_texture_in(renderer: &mut egui_wgpu::Renderer, device: &wgpu::Device, view: &wgpu::TextureView) -> TextureId {
    let id = renderer.register_native_texture(device, view, wgpu::FilterMode::Linear);
    if let Some(live) = renderer.callback_resources.get_mut::<LiveResources>() {
        live.register_native_texture(device, view, id);
    }
    id
}

fn free_native_texture(renderer: &mut egui_wgpu::Renderer, id: &TextureId) {
    renderer.free_texture(id);
    if let Some(live) = renderer.callback_resources.get_mut::<LiveResources>() {
        live.free_texture(id);
    }
}

/// Draws the backdrop image covering `rect` (aspect-fill, centered) and records
/// the mapping so glass widgets sample the same pixels.
pub fn show_backdrop(ui: &mut Ui, rect: Rect) {
    let Some(state) = backdrop_state(ui.ctx()) else { return };
    let scale = (rect.width() / state.size.x).max(rect.height() / state.size.y);
    let fill = ui.visuals().panel_fill;
    show_backdrop_mapped(ui, Rect::from_center_size(rect.center(), state.size * scale), rect, fill);
}

/// Draws the whole backdrop image into `image_rect` (clipped to `clip`) and
/// records that mapping. Use this when you place the image yourself. Glass that
/// samples outside `image_rect` shows `outside` (typically your page colour).
pub fn show_backdrop_mapped(ui: &mut Ui, image_rect: Rect, clip: Rect, outside: Color32) {
    let Some(mut state) = backdrop_state(ui.ctx()) else { return };
    state.rect = image_rect;
    state.fill = outside;
    ui.ctx().data_mut(|d| d.insert_temp(state_id(), state));
    let painter = ui.painter().with_clip_rect(clip);
    painter.image(state.texture, image_rect, Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), egui::Color32::WHITE);
}

/// SolidCraft: records that the whole backdrop image maps to `image_rect` without drawing it,
/// for an app that paints its own background in the image's colours.
pub fn map_backdrop(ctx: &Context, image_rect: Rect, outside: Color32) {
    let Some(mut state) = backdrop_state(ctx) else { return };
    state.rect = image_rect;
    state.fill = outside;
    ctx.data_mut(|d| d.insert_temp(state_id(), state));
}

/// Pixel size of the registered backdrop image.
pub fn backdrop_size(ctx: &Context) -> Option<Vec2> {
    backdrop_state(ctx).map(|s| s.size)
}
