//! Apple Liquid Glass style widgets for egui, rendered on the GPU through
//! egui-wgpu paint callbacks.
//!
//! Requires egui / egui-wgpu 0.35 and wgpu 29. With eframe, enable its `wgpu`
//! feature and select `eframe::Renderer::Wgpu`; the glow renderer is unsupported.
//!
//! ```no_run
//! # fn setup(ctx: &egui::Context, rs: &egui_wgpu::RenderState, wallpaper: egui::ColorImage) {
//! egui_glass::init(rs, 1);
//! egui_glass::set_backdrop(ctx, rs, &wallpaper);
//! # }
//! # fn ui(ui: &mut egui::Ui) {
//! egui_glass::show_backdrop(ui, ui.max_rect());
//! egui_glass::Glass::new(egui_glass::GlassStyle::regular()).show(ui, |ui| {
//!     ui.label("Hello");
//! });
//! # }
//! ```
//!
//! Glass refracts a *backdrop image* you register once (or whenever it changes),
//! not the live framebuffer: egui paints in a single pass, so the pixels behind a
//! widget are not available to a shader in the same frame. [`LiveBackdrop`]
//! lifts that limit by rendering your content a second time off screen.
//!
//! # Integration order
//!
//! Call [`init`] once per renderer, then create a [`LiveBackdrop`] if needed,
//! then upload the backdrop and register native textures. Draw the backdrop
//! before glass each frame. The MSAA count passed to [`init`] must match the
//! host renderer (0 is treated as 1).
//! Alternatively, [`GlassContext`] initializes both renderers together and provides
//! validated backdrop uploads, shared font configuration and owned native textures.
//!
//! # Features
//!
//! No default features. The optional `serde` feature enables serialization and
//! deserialization of [`GlassStyle`], with defaults for missing fields.
//!
//! See the [guide](https://github.com/heonny/egui-glass/blob/main/docs/guide.md)
//! and [reference](https://github.com/heonny/egui-glass/blob/main/docs/reference.md)
//! for integration details, performance costs, and limitations.

mod animation;
mod backdrop;
mod context;
#[cfg(test)]
mod context_gpu;
mod live;
mod live_quality;
mod mipgen;
mod renderer;
mod scene;
mod slider;
mod style;
mod widgets;

pub use backdrop::{
    backdrop_rect, backdrop_size, free_native_texture_id, map_backdrop, register_native_texture, set_backdrop, show_backdrop, show_backdrop_mapped,
};
pub use context::{GlassContext, GlassError, GlassTexture};
pub use live::LiveBackdrop;
pub use live_quality::LiveBackdropQuality;
pub use renderer::init;
pub use scene::{init_scene_backdrop, scene_backdrop_shape, SceneBrightness, SceneLuma, SceneSource};
pub use slider::GlassSlider;
pub use style::GlassStyle;
pub use widgets::{glass_shape_in, paint_glass, Glass, GlassButton, GlassToolbar};
