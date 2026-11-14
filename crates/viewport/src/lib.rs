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

pub use analysis::{AnalysisMode, SceneAnalysis};
pub use camera::{Camera, CameraAnim, Mat4, Quat, StandardView, UpAxis, ViewAngles, ease_in_out};
pub use pick::{DrawnPoint, nearest_drawn_point};
pub use raster::{Canvas, render, render_png, render_transparent, shade, unblend};
pub use scene::{MIN_ALPHA, Rgb, SECTION_CAP, SECTION_HATCH, SEE_THROUGH, Scene, SceneLine, SceneMesh, png};
