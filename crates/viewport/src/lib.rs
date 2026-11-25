//! The viewport renderer the Craft apps share. View math: an orbit camera with perspective or
//! orthographic projection, screen ↔ world mapping and picking rays. The description of a picture
//! ([`Scene`]) and a small CPU rasterizer (z-buffer, shaded triangles, depth-tested lines) for
//! pictures without a GPU. Picking drawn lines: the drawn point nearest the pointer
//! ([`nearest_drawn_point`]). The opaque and clear-coat shading models and the section hatch. The
//! grid's spacing and reach for a view ([`grid`]). It
//! draws solidcore-tess's display meshes and knows no app's geometry crate, GPU API or UI
//! toolkit: the GPU pipelines are `solidcraft-viewport-gpu`, the egui glue
//! `solidcraft-viewport-egui`.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

mod analysis;
mod camera;
pub mod clear_coat;
pub mod grid;
pub mod hatch;
mod pick;
mod raster;
mod scene;
pub mod surface;
pub mod vec;
mod view_spec;

pub use analysis::AnalysisMode;
pub use analysis::SceneAnalysis;
pub use camera::Camera;
pub use camera::CameraAnim;
pub use camera::Mat4;
pub use camera::Quat;
pub use camera::StandardView;
pub use camera::UpAxis;
pub use camera::ViewAngles;
pub use camera::ease_in_out;
pub use pick::DrawnPoint;
pub use pick::nearest_drawn_point;
pub use raster::Canvas;
pub use raster::render;
pub use raster::render_png;
pub use raster::render_transparent;
pub use raster::shade;
pub use raster::unblend;
pub use scene::MAX_SECTION_PLANES;
pub use scene::MIN_ALPHA;
pub use scene::Rgb;
pub use scene::SECTION_CAP;
pub use scene::SECTION_HATCH;
pub use scene::SECTION_HATCH_ANGLES;
pub use scene::SEE_THROUGH;
pub use scene::Scene;
pub use scene::SceneLine;
pub use scene::SceneMesh;
pub use scene::png;
pub use scene::section_hatch;
pub use scene::section_hatch_line;
pub use scene::section_hatch_of;
pub use scene::section_plane_frame;
pub use view_spec::ViewSpec;
