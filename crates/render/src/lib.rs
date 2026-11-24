//! SolidCraft's product rendering on top of the shared viewport renderer (`solidcraft-viewport`):
//! render jobs and their backends, the path tracer's materials, lighting, studio, film and
//! textures, and the model's bounds as the viewport's.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

pub mod anisotropic_surface;
mod backend;
mod curve_display;
pub mod film;
mod gif;
pub mod glass_model;
pub mod image_orientation;
pub mod job;
pub mod lighting;
pub mod mapping;
mod model_bounds;
pub mod optical_boundary;
mod product;
#[cfg(feature = "raytrace-cpu")]
pub mod raytrace;
pub mod sheet_image;
mod silhouette;
pub mod stage;
pub mod studio;
pub mod surface_patterns;
pub mod texture_input;
pub mod texture_interpolation;
pub mod texture_lod;
pub mod texture_sampling;
pub mod transmission;
mod visual_style;

pub use backend::Availability;
pub use backend::BackendDescriptor;
pub use backend::BackendId;
pub use backend::ExecutorDescriptor;
pub use backend::ExecutorId;
pub use backend::MAX_RENDER_ID_BYTES;
pub use backend::RenderFeature;
pub use backend::RenderFormat;
pub use backend::RenderLimits;
pub use backend::RenderMode;
pub use backend::RenderOperation;
pub use backend::RenderPolicy;
pub use backend::RenderProfile;
pub use backend::RenderRequest;
pub use backend::ResolveError;
pub use backend::ResolvedRequest;
pub use backend::ScratchLayout;
pub use backend::builtin_backend;
pub use backend::resolve_request;
pub use curve_display::PixelCurveView;
pub use curve_display::SampledPath;
pub use curve_display::SamplingBudget;
pub use curve_display::SamplingQuality;
pub use curve_display::sample_segment;
pub use curve_display::sample_segment_unclipped;
pub use gif::gif;
pub use model_bounds::view_bounds;
pub use product::FaceAppearance;
pub use product::ProductMesh;
pub use product::ProductScene;
pub use silhouette::PreparedSilhouettes;
pub use silhouette::SilhouetteBuilder;
pub use silhouette::SilhouetteEdge;
pub use silhouette::SilhouetteView;
pub use visual_style::VisualStyle;
