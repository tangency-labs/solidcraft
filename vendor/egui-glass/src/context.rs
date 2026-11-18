use std::sync::Arc;

use egui::{ColorImage, Context, FontDefinitions, TextureId};
use egui_wgpu::RenderState;

use crate::live::LiveResources;
use crate::renderer::GlassResources;
use crate::LiveBackdrop;

/// Initialization or backdrop validation failure from [`GlassContext`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GlassError {
    /// This renderer already has glass resources; reuse the existing setup.
    AlreadyInitialized,
    /// The target format does not support this MSAA sample count.
    UnsupportedSampleCount(u32),
    /// Image dimensions are zero, overflow, or disagree with pixel storage.
    InvalidImage {
        /// Declared width and height.
        size: [usize; 2],
        /// Actual number of stored pixels.
        pixels: usize,
    },
    /// Image dimensions exceed the device's 2D texture limit.
    ImageTooLarge {
        /// Requested width and height.
        size: [usize; 2],
        /// Maximum supported width or height.
        limit: u32,
    },
}

impl std::fmt::Display for GlassError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AlreadyInitialized => write!(f, "glass is already initialized on this renderer; reuse the existing GlassContext or legacy setup"),
            Self::UnsupportedSampleCount(samples) => write!(f, "MSAA sample count {samples} is unsupported for this renderer's target format"),
            Self::InvalidImage { size, pixels } => write!(f, "invalid backdrop: {}x{} dimensions with {pixels} pixels; use nonzero dimensions and matching pixel storage", size[0], size[1]),
            Self::ImageTooLarge { size, limit } => write!(f, "backdrop {}x{} exceeds the device's {limit}-pixel texture limit; resize it before uploading", size[0], size[1]),
        }
    }
}

impl std::error::Error for GlassError {}

/// One-time setup and resource access for a renderer and its egui context. Cheap to clone.
///
/// Replaces the manual `init` → `LiveBackdrop::new` → texture registration sequence.
/// The off-screen renderer is prepared eagerly, but renders only when its `run` method
/// is called. Shared pipelines and the active backdrop live until the renderer is dropped.
/// Do not call the standalone `init` or `LiveBackdrop::new` again on this renderer.
///
/// ```no_run
/// # fn setup(ctx: &egui::Context, rs: &egui_wgpu::RenderState, image: &egui::ColorImage)
/// # -> Result<egui_glass::GlassContext, egui_glass::GlassError> {
/// let glass = egui_glass::GlassContext::new(ctx, rs, 1)?;
/// glass.set_fonts(egui::FontDefinitions::default());
/// glass.set_backdrop(image)?;
/// # Ok(glass)
/// # }
/// ```
#[derive(Clone)]
pub struct GlassContext {
    ctx: Context,
    render_state: RenderState,
    live: LiveBackdrop,
}

impl GlassContext {
    /// Initializes glass and live resources together, without overwriting an existing setup.
    /// `msaa_samples` must match the host renderer; 0 is normalized to 1.
    /// Custom fonts must be installed through [`Self::set_fonts`] to reach both contexts.
    /// GPU allocation/device failures still follow wgpu's error handling.
    pub fn new(ctx: &Context, render_state: &RenderState, msaa_samples: u32) -> Result<Self, GlassError> {
        let mut renderer = render_state.renderer.write();
        if renderer.callback_resources.get::<GlassResources>().is_some() || renderer.callback_resources.get::<LiveResources>().is_some() {
            return Err(GlassError::AlreadyInitialized);
        }
        let samples = msaa_samples.max(1);
        let features = render_state.device.features();
        let format_features = if features.contains(wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES) {
            render_state.adapter.get_texture_format_features(render_state.target_format)
        } else {
            render_state.target_format.guaranteed_format_features(features)
        };
        if !format_features.flags.sample_count_supported(samples) {
            return Err(GlassError::UnsupportedSampleCount(msaa_samples));
        }
        renderer.callback_resources.insert(GlassResources::new(render_state, samples));
        let live = LiveBackdrop::new_in(render_state, &mut renderer, None);
        Ok(Self { ctx: ctx.clone(), render_state: render_state.clone(), live })
    }

    /// Installs the same fonts into the visible and off-screen contexts.
    pub fn set_fonts(&self, fonts: FontDefinitions) {
        self.ctx.set_fonts(fonts.clone());
        self.live.set_fonts(fonts);
    }

    /// Validates and uploads a backdrop, mirroring it into the live renderer.
    /// On validation failure the previous backdrop remains registered.
    /// Successful replacement frees the previous backdrop ID; stop drawing that ID.
    pub fn set_backdrop(&self, image: &ColorImage) -> Result<TextureId, GlassError> {
        validate_image(image, self.render_state.device.limits().max_texture_dimension_2d)?;
        Ok(crate::set_backdrop(&self.ctx, &self.render_state, image))
    }

    /// Returns the already-initialized live backdrop. Its clones share the twin context.
    /// The page closure's repeated-execution rules still apply; see [`LiveBackdrop::run`].
    pub fn live_backdrop(&self) -> &LiveBackdrop {
        &self.live
    }

    /// Registers a texture in both renderers and returns its owning handle.
    /// The view must satisfy egui-wgpu's native-texture requirements (filterable 2D RGBA).
    /// Keep the handle alive until every frame that uses its ID has been rendered.
    pub fn register_native_texture(&self, view: &wgpu::TextureView) -> GlassTexture {
        let id = crate::register_native_texture(&self.render_state, view);
        GlassTexture(Arc::new(TextureRegistration { id, render_state: self.render_state.clone() }))
    }
}

/// An owned native-texture registration. Clones share ownership; the final drop frees
/// its ID in both renderers. Store this alongside application state, not as a UI temporary.
/// Do not release its ID with [`crate::free_native_texture_id`].
/// The final drop acquires the renderer write lock; drop it outside renderer lock guards.
#[derive(Clone)]
#[must_use = "keep the texture handle alive while its ID is being rendered"]
pub struct GlassTexture(Arc<TextureRegistration>);

impl GlassTexture {
    /// Texture ID for `ui.image((texture.id(), size))` and other egui image widgets.
    pub fn id(&self) -> TextureId {
        self.0.id
    }
}

struct TextureRegistration {
    id: TextureId,
    render_state: RenderState,
}

impl Drop for TextureRegistration {
    fn drop(&mut self) {
        crate::free_native_texture_id(&self.render_state, &self.id);
    }
}

fn validate_image(image: &ColorImage, limit: u32) -> Result<(), GlassError> {
    if image.size.contains(&0) || image.size[0].checked_mul(image.size[1]) != Some(image.pixels.len()) {
        return Err(GlassError::InvalidImage { size: image.size, pixels: image.pixels.len() });
    }
    if image.size.iter().any(|&side| side > limit as usize) {
        return Err(GlassError::ImageTooLarge { size: image.size, limit });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_validation_rejects_empty_mismatched_overflowing_and_oversized_images() {
        for size in [[0, 1], [2, 2], [usize::MAX, 2]] {
            let mut image = egui::ColorImage::filled([1, 1], egui::Color32::WHITE);
            image.size = size;
            assert!(matches!(validate_image(&image, 8192), Err(GlassError::InvalidImage { .. })));
        }
        let image = egui::ColorImage::filled([16, 8], egui::Color32::WHITE);
        assert!(matches!(validate_image(&image, 8), Err(GlassError::ImageTooLarge { .. })));
        assert!(validate_image(&image, 16).is_ok());
    }
}
