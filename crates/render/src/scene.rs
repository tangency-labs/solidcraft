//! What a picture of the model shows, independent of how it is drawn: meshes in colours, lines,
//! the background, a section cut and a surface analysis. The GPU draws it with the viewport's
//! pipelines (`solidcraft_gpu::capture`).

use std::sync::Arc;

use solidcraft_geom::{Mesh, Vec3};

use crate::SceneAnalysis;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl Rgb {
    /// The colour as 0..1 per channel.
    pub fn f(self) -> [f64; 3] {
        [self.0 as f64 / 255.0, self.1 as f64 / 255.0, self.2 as f64 / 255.0]
    }
}

pub struct SceneMesh {
    pub mesh: Arc<Mesh>,
    pub color: Rgb,
    /// Faces in colours of their own: (B-rep face index, colour).
    pub face_colors: Vec<(u32, Rgb)>,
    /// How opaque the body is (an appearance's opacity): 1 is solid, below [`SEE_THROUGH`] it is
    /// drawn see-through, so what lies inside it shows, as the viewport draws glass. A body
    /// without an appearance of its own is solid.
    pub opacity: f32,
}

impl SceneMesh {
    /// A solid body in `color`, with no face of its own look.
    pub fn solid(mesh: Arc<Mesh>, color: Rgb) -> SceneMesh {
        SceneMesh { mesh, color, face_colors: Vec::new(), opacity: 1.0 }
    }

    /// Is the body drawn see-through? (Its opacity is below [`SEE_THROUGH`].)
    pub fn glass(&self) -> bool {
        self.opacity < SEE_THROUGH
    }

    /// The alpha the body is blended with: its opacity, never so low that it vanishes
    /// altogether, and 1 when it is solid.
    pub fn alpha(&self) -> f64 {
        if self.glass() { f64::from(self.opacity).clamp(MIN_ALPHA, 1.0) } else { 1.0 }
    }
}

/// At or above this opacity a body is drawn solid: an appearance a hair under 1 is not worth a
/// see-through pass, and the viewport draws the same bodies as glass.
pub const SEE_THROUGH: f32 = 0.999;

/// The lowest alpha a see-through body is drawn with, so opacity 0 still leaves a trace of the
/// body rather than an invisible one (as the viewport clamps it).
pub const MIN_ALPHA: f64 = 0.05;

pub struct SceneLine {
    pub points: Vec<Vec3>,
    pub color: Rgb,
    /// How opaque the line is: 1 is solid, below 1 it is blended with what is under it (the
    /// grid fades out with the distance, #4214).
    pub alpha: f32,
    /// Width in output pixels.
    pub width: f64,
    /// Draw over everything (sketch geometry being edited).
    pub on_top: bool,
}

#[derive(Default)]
pub struct Scene {
    pub meshes: Vec<SceneMesh>,
    pub lines: Vec<SceneLine>,
    /// The ground grid: drawn under everything (the model hides it; it never hides the model).
    pub grid: Vec<SceneLine>,
    /// Background gradient top → bottom.
    pub background: Option<(Rgb, Rgb)>,
    /// Scene radius for clipping planes.
    pub radius: f64,
    /// A section cut by up to [`MAX_SECTION_PLANES`] planes, each (normal, d): what lies where
    /// normal·p > d for every one of them is cut away, and the inside shows as a hatched cap.
    pub clip: Vec<(Vec3, f64)>,
    /// The colour the section cap is tinted toward (default: [`SECTION_CAP`]).
    pub cap: Option<Rgb>,
    /// The section cap without its hatch: plain, in the body's own colour.
    pub plain_cap: bool,
    /// A surface analysis shading the meshes (zebra, draft, curvature map, ...).
    pub analysis: Option<SceneAnalysis>,
}

/// The most planes a section cuts with: the viewport shader's fixed array of clip planes.
pub const MAX_SECTION_PLANES: usize = 6;

/// The section cap's default tint, as in the viewport.
pub const SECTION_CAP: Rgb = Rgb(214, 128, 72);

/// RGBA pixels (`w × h`) to PNG bytes.
pub fn png(w: usize, h: usize, rgba: Vec<u8>) -> Option<Vec<u8>> {
    let img = image::RgbaImage::from_raw(u32::try_from(w).ok()?, u32::try_from(h).ok()?, rgba)?;
    let mut buf = std::io::Cursor::new(Vec::new());
    img.write_to(&mut buf, image::ImageFormat::Png).ok()?;
    Some(buf.into_inner())
}
