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
    /// A section cut: (normal, d); what lies where normal·p > d is cut away, and the inside
    /// shows as a hatched cap.
    pub clip: Option<(Vec3, f64)>,
    /// Quarter section: retain only the intersection of both non-positive half-spaces.
    pub clip_second: Option<(Vec3, f64)>,
    /// The colour the section cap is tinted toward (default: [`SECTION_CAP`]).
    pub cap: Option<Rgb>,
    /// A surface analysis shading the meshes (zebra, draft, curvature map, ...).
    pub analysis: Option<SceneAnalysis>,
}

/// The section cap's default tint, as in the viewport.
pub const SECTION_CAP: Rgb = Rgb(214, 128, 72);

/// Whether either section plane removes a world point.
pub fn section_clipped(p: Vec3, first: Option<(Vec3, f64)>, second: Option<(Vec3, f64)>) -> bool {
    first.is_some_and(|(n, d)| n.dot(p) > d || second.is_some_and(|(n, d)| n.dot(p) > d))
}

/// Visible pieces of a line inside every active section plane's kept half-space.
pub fn section_segments(p: Vec3, q: Vec3, first: Option<(Vec3, f64)>, second: Option<(Vec3, f64)>) -> Vec<(Vec3, Vec3)> {
    let mut cuts = vec![0.0, 1.0];
    for (n, d) in first.into_iter().chain(second) {
        let den = n.dot(q - p);
        if den.abs() > 1e-12 {
            let t = (d - n.dot(p)) / den;
            if t > 0.0 && t < 1.0 {
                cuts.push(t);
            }
        }
    }
    cuts.sort_by(f64::total_cmp);
    cuts.windows(2)
        .filter_map(|w| {
            let (&a, &b) = (w.first()?, w.get(1)?);
            (!section_clipped(p + (q - p) * ((a + b) * 0.5), first, second)).then_some((p + (q - p) * a, p + (q - p) * b))
        })
        .collect()
}

/// RGBA pixels (`w × h`) to PNG bytes.
pub fn png(w: usize, h: usize, rgba: Vec<u8>) -> Option<Vec<u8>> {
    let img = image::RgbaImage::from_raw(u32::try_from(w).ok()?, u32::try_from(h).ok()?, rgba)?;
    let mut buf = std::io::Cursor::new(Vec::new());
    img.write_to(&mut buf, image::ImageFormat::Png).ok()?;
    Some(buf.into_inner())
}

#[cfg(test)]
mod contribution_tests {
    use super::*;
    #[test]
    fn contribution_quarter_section_keeps_one_quadrant_for_every_flip() {
        for sx in [-1.0, 1.0] {
            for sy in [-1.0, 1.0] {
                let a = Some((Vec3::X * sx, 0.0));
                let b = Some((Vec3::Y * sy, 0.0));
                let mut removed = 0;
                for x in [-1.0, 1.0] {
                    for y in [-1.0, 1.0] {
                        let p = Vec3::new(x, y, 0.0);
                        assert_eq!(section_clipped(p, a, b), x * sx > 0.0 || y * sy > 0.0);
                        removed += usize::from(section_clipped(p, a, b));
                    }
                }
                assert_eq!(removed, 3);
            }
        }
    }
    #[test]
    fn contribution_quarter_section_lines_stay_inside_both_kept_halfspaces() {
        let (p, q) = (Vec3::new(-2.0, 3.0, 0.0), Vec3::new(3.0, -2.0, 0.0));
        let a = Some((Vec3::X, 0.0));
        let b = Some((Vec3::Y, 0.0));
        let lines = section_segments(p, q, a, b);
        assert!(lines.is_empty(), "each part crosses at least one removed strip");
        let start = Vec3::new(-2.0, -3.0, 0.0);
        let lines = section_segments(start, Vec3::new(3.0, 2.0, 0.0), a, b);
        assert_eq!(lines.len(), 1);
        assert!((lines[0].0 - start).len() < 1e-9);
        assert!((lines[0].1 - Vec3::new(0.0, -1.0, 0.0)).len() < 1e-9);
        assert_eq!(section_segments(p, q, None, None), vec![(p, q)]);
        assert_eq!(section_segments(p, q, a, None).len(), 1);
    }

    #[test]
    fn quarter_lateral_face_offsets_remove_both_external_strips_without_moving_the_planes() {
        for first in [-1.0, 1.0] {
            for second in [-1.0, 1.0] {
                let a = Some((-Vec3::Y * first, -10.0 * first));
                let b = Some((Vec3::X * second, 68.5 * second));
                let mut kept = 0;
                for x in [10.0, 75.0] {
                    for y in [5.0, 25.0] {
                        let p = Vec3::new(x, y, 20.0);
                        let expected = (y - 10.0) * first < 0.0 || (x - 68.5) * second > 0.0;
                        assert_eq!(section_clipped(p, a, b), expected, "{first}/{second}: {p:?}");
                        kept += usize::from(!section_clipped(p, a, b));
                    }
                }
                assert_eq!(kept, 1);
                assert!(!section_clipped(Vec3::new(68.5, 10.0, 20.0), a, b), "both fixed plane boundaries remain visible");
                assert_eq!(section_clipped(Vec3::new(75.0, 25.0, 20.0), a, None), first < 0.0, "a single section keeps its original behavior");
            }
        }
    }
}
