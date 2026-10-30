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
    /// Each clip plane's hatch, an index into [`SECTION_HATCH_ANGLES`] ([`section_hatch_of`]);
    /// one missing: its place in `clip`.
    pub clip_hatch: Vec<usize>,
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

/// The angle (degrees) each plane's cut faces are hatched at, by the plane's id
/// ([`section_hatch_of`]), in the plane's own frame ([`section_hatch_line`]).
pub const SECTION_HATCH_ANGLES: [f64; MAX_SECTION_PLANES] = [45.0, 135.0, 30.0, 60.0, 120.0, 150.0];

/// The hatch of the section plane with id `id` (from 1): an index into [`SECTION_HATCH_ANGLES`],
/// the same whichever other planes are on show.
pub fn section_hatch_of(id: u64) -> usize {
    (id.saturating_sub(1) % MAX_SECTION_PLANES as u64) as usize
}

/// The section plane's own frame (right, up) as seen from the side its normal `n` points to,
/// world Z up: fixed by the plane alone, so its hatch keeps its direction as the camera moves.
pub fn section_plane_frame(n: Vec3) -> (Vec3, Vec3) {
    let n = n.normalized().unwrap_or(Vec3::Z);
    let right = if n.z.abs() < 0.9 { Vec3::Z.cross(n) } else { Vec3::X - n * n.x };
    let right = right.normalized().unwrap_or_else(|| n.any_perp());
    (right, n.cross(right))
}

/// The direction of the hatch lines on the `index`-th plane (normal `n`) of a section.
pub fn section_hatch_line(n: Vec3, index: usize) -> Vec3 {
    let (right, up) = section_plane_frame(n);
    let a = SECTION_HATCH_ANGLES[index % MAX_SECTION_PLANES].to_radians();
    right * a.cos() + up * a.sin()
}

/// The `index`-th section plane's hatch on a `w × h` picture through `view_proj`: the stripe
/// phase is `x·s[0] + y·s[1]` (pixels, y down), a stripe every 9/√2 pixels, the lines running
/// as the plane's hatch line looks at the centre of the picture. Face on, the first plane's
/// hatch is the 45 degree stripes of old: `(x + y) / 9`.
pub fn section_hatch(view_proj: &crate::Mat4, n: Vec3, index: usize, w: f64, h: f64) -> [f64; 2] {
    let (l, o) = (view_proj.apply(section_hatch_line(n, index)), view_proj.apply(Vec3::ZERO));
    let (dx, dy) = ((l[0] - o[0]) * w, (o[1] - l[1]) * h);
    let len = dx.hypot(dy);
    let across = if len > 1e-12 && len.is_finite() { [-dy / len, dx / len] } else { [std::f64::consts::FRAC_1_SQRT_2; 2] };
    across.map(|c| c * std::f64::consts::SQRT_2 / 9.0)
}

/// RGBA pixels (`w × h`) to PNG bytes.
pub fn png(w: usize, h: usize, rgba: Vec<u8>) -> Option<Vec<u8>> {
    let img = image::RgbaImage::from_raw(u32::try_from(w).ok()?, u32::try_from(h).ok()?, rgba)?;
    let mut buf = std::io::Cursor::new(Vec::new());
    img.write_to(&mut buf, image::ImageFormat::Png).ok()?;
    Some(buf.into_inner())
}
