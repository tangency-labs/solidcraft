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
pub mod job;
pub mod lighting;
pub mod mapping;
mod model_bounds;
pub mod optical_boundary;
mod product;
#[cfg(feature = "raytrace-cpu")]
pub mod raytrace;
pub mod stage;
pub mod studio;
pub mod surface_patterns;
pub mod texture_input;
pub mod texture_interpolation;
pub mod texture_lod;
pub mod texture_sampling;
pub mod transmission;

pub use backend::{
    Availability, BackendDescriptor, BackendId, ExecutorDescriptor, ExecutorId, MAX_RENDER_ID_BYTES, RenderFeature, RenderFormat, RenderLimits,
    RenderMode, RenderOperation, RenderPolicy, RenderProfile, RenderRequest, ResolveError, ResolvedRequest, ScratchLayout, builtin_backend,
    resolve_request,
};
pub use curve_display::{PixelCurveView, SampledPath, SamplingBudget, SamplingQuality, sample_segment, sample_segment_unclipped};
pub use gif::gif;
pub use model_bounds::view_bounds;
pub use product::{FaceAppearance, ProductMesh, ProductScene};
