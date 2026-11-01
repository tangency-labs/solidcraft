//! How finely the viewport tessellates curved faces and edges (Preferences ▸ Graphics ▸ Display
//! quality). A viewing preference, not design data: it only picks the tolerances of the display
//! plan ([`crate::ModelBody::display_plan`]) the viewport draws. The reference mesh the model
//! reads (picks, holes, measuring) and exports keep their own tolerances.

use std::sync::atomic::{AtomicU8, Ordering};

use solidcraft_geom::{Aabb3, Vec3};

/// A display quality level.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum DisplayQuality {
    Coarse,
    #[default]
    Normal,
    Fine,
    Ultra,
}

impl DisplayQuality {
    pub const ALL: [DisplayQuality; 4] = [DisplayQuality::Coarse, DisplayQuality::Normal, DisplayQuality::Fine, DisplayQuality::Ultra];

    /// The id kept in the settings file.
    pub fn id(self) -> &'static str {
        match self {
            DisplayQuality::Coarse => "coarse",
            DisplayQuality::Normal => "normal",
            DisplayQuality::Fine => "fine",
            DisplayQuality::Ultra => "ultra",
        }
    }

    pub fn parse(id: &str) -> Option<DisplayQuality> {
        Self::ALL.into_iter().find(|q| q.id() == id)
    }

    /// Position in [`DisplayQuality::ALL`] (indexes the per-level display caches).
    pub fn index(self) -> usize {
        self as usize
    }

    /// Chord tolerance as a fraction of the body's size: the largest gap allowed between a
    /// facet and its surface.
    pub fn chord_fraction(self) -> f64 {
        match self {
            DisplayQuality::Coarse => 3e-3,
            DisplayQuality::Normal => 1e-3,
            DisplayQuality::Fine => 3e-4,
            DisplayQuality::Ultra => 1e-4,
        }
    }

    /// Smallest chord tolerance (mm), so a tiny body does not mesh down to nothing.
    pub fn min_chord(self) -> f64 {
        match self {
            DisplayQuality::Coarse => 3e-3,
            DisplayQuality::Normal => 1e-3,
            DisplayQuality::Fine => 3e-4,
            DisplayQuality::Ultra => 1e-4,
        }
    }

    /// Most a facet of a curved face or edge may turn (radians), so a small radius on a large
    /// body still looks round.
    pub fn angle(self) -> f64 {
        let deg: f64 = match self {
            DisplayQuality::Coarse => 30.0,
            DisplayQuality::Normal => 20.0,
            DisplayQuality::Fine => 12.0,
            DisplayQuality::Ultra => 7.0,
        };
        deg.to_radians()
    }

    /// The chord tolerance (mm) for a body of this size.
    pub fn chord(self, size: f64) -> f64 {
        let size = if size.is_finite() && size > 0.0 { size } else { 0.0 };
        (size * self.chord_fraction()).max(self.min_chord())
    }
}

/// Most refinement steps view-dependent display refinement takes past the chosen level (#1320):
/// each halves the chord tolerance, so the finest is 1/16 of the chosen level's.
pub const MAX_REFINE: u8 = 4;

/// Screen-space error (physical pixels) above which a body is refined: a facet's sag shows.
pub const REFINE_PX: f64 = 1.0;
/// Error a refinement aims for (below [`REFINE_PX`], so a body just refined is not refined again
/// at the next small zoom step).
pub const TARGET_PX: f64 = 0.75;
/// Error below which a refined body steps back toward the chosen level. A refinement lands
/// between half [`TARGET_PX`] and [`TARGET_PX`], and a step back between this and twice this, so
/// zooming out or in by up to 2× from either never flips the level back.
pub const COARSEN_PX: f64 = 0.175;

impl DisplayQuality {
    /// Chord tolerance (mm) of a body of this size refined `k` steps past this level: halved
    /// per step.
    pub fn refined_chord(self, size: f64, k: u8) -> f64 {
        self.chord(size) / f64::from(1u32 << k.min(MAX_REFINE))
    }

    /// Facet angle (radians) `k` steps past this level: an arc's sag goes with the square of its
    /// angle, so the angle shrinks by √2 per step, keeping pace with the chord.
    pub fn refined_angle(self, k: u8) -> f64 {
        self.angle() / std::f64::consts::SQRT_2.powi(i32::from(k.min(MAX_REFINE)))
    }
}

/// The refinement step a body should be drawn at (0: the chosen level), from `err_px`: the
/// screen-space error of the chosen level, its chord tolerance in physical pixels where the body
/// is nearest the eye (0 for a body off screen or too small to matter). `current` is the step
/// drawn now; steps halve the error. A body is refined once its error passes [`REFINE_PX`], to the
/// first step under [`TARGET_PX`], and steps back only once the error falls under [`COARSEN_PX`],
/// to the coarsest step that stays under twice that: the band between keeps an orbit or a small
/// zoom from flipping levels. Never past `max` (at most [`MAX_REFINE`]).
pub fn refinement_for(err_px: f64, current: u8, max: u8) -> u8 {
    let max = max.min(MAX_REFINE);
    let current = current.min(max);
    if !(err_px.is_finite() && err_px > 0.0) {
        return 0;
    }
    let err = |k: u8| err_px / f64::from(1u32 << k);
    if err(current) > REFINE_PX {
        (current..=max).find(|&k| err(k) <= TARGET_PX).unwrap_or(max)
    } else if err(current) < COARSEN_PX {
        (0..=current).find(|&k| err(k) <= 2.0 * COARSEN_PX).unwrap_or(current)
    } else {
        current
    }
}

/// Error a render aims for (#9814), in output pixels: a picture is made once and often looked at
/// in a crop, so it aims finer than the viewport's [`TARGET_PX`] and has no hysteresis.
pub const IMAGE_TARGET_PX: f64 = 0.5;

/// The fewest refinement steps that bring `err_px` (as for [`refinement_for`]) to `target_px` or
/// under, never past `max` (at most [`MAX_REFINE`]): each step halves the error. 0 when the error
/// is under the target already, or is not a positive number (a body off screen).
pub fn steps_to(err_px: f64, target_px: f64, max: u8) -> u8 {
    let max = max.min(MAX_REFINE);
    if !(err_px.is_finite() && err_px > 0.0 && target_px.is_finite() && target_px > 0.0) {
        return 0;
    }
    (0..=max).find(|&k| err_px / f64::from(1u32 << k) <= target_px).unwrap_or(max)
}

/// How a camera maps millimetres to pixels, for the screen-space error of a body: the viewport's
/// refinement and a render's (#9814) measure it the same way.
#[derive(Clone, Copy, Debug)]
pub struct Lens {
    pub eye: Vec3,
    /// Unit direction from the target toward the eye.
    pub back: Vec3,
    /// Orthographic: pixels per millimetre everywhere. Perspective: pixels per millimetre at a
    /// depth of 1 mm (divided by the depth).
    pub scale: f64,
    pub perspective: bool,
    /// Nearest depth considered (mm), so a body around the eye does not ask for infinity.
    pub near: f64,
}

impl Lens {
    /// The view of `cam` in a picture `height_px` physical pixels high.
    pub fn new(cam: &solidcraft_render::Camera, height_px: f64) -> Lens {
        let perspective = cam.fov > 0.0;
        let scale = if perspective { height_px / (2.0 * (cam.fov * 0.5).tan()) } else { height_px / (2.0 * cam.half_height()) };
        Lens { eye: cam.eye(), back: cam.back(), scale, perspective, near: (cam.distance * 1e-2).max(1e-3) }
    }

    /// Whether any of a box lies in front of the eye.
    pub fn in_front(&self, b: &Aabb3) -> bool {
        let toward = -self.back;
        let far_corner = Vec3::new(
            if toward.x > 0.0 { b.max.x } else { b.min.x },
            if toward.y > 0.0 { b.max.y } else { b.min.y },
            if toward.z > 0.0 { b.max.z } else { b.min.z },
        );
        !self.perspective || (far_corner - self.eye).dot(toward) > 0.0
    }

    /// Pixels per millimetre where a box comes nearest the eye.
    pub fn px_per_mm(&self, b: &Aabb3) -> f64 {
        if !self.perspective {
            return self.scale;
        }
        // The box's nearest depth along the view: its support point toward the eye.
        let toward = -self.back;
        let near_corner = Vec3::new(
            if toward.x > 0.0 { b.min.x } else { b.max.x },
            if toward.y > 0.0 { b.min.y } else { b.max.y },
            if toward.z > 0.0 { b.min.z } else { b.max.z },
        );
        let depth = (near_corner - self.eye).dot(toward);
        self.scale / depth.max(self.near)
    }
}

static CURRENT: AtomicU8 = AtomicU8::new(1);

/// The level the viewport draws at (process-wide; set from the preferences).
pub fn current() -> DisplayQuality {
    DisplayQuality::ALL.get(CURRENT.load(Ordering::Relaxed) as usize).copied().unwrap_or_default()
}

/// Set the level the viewport draws at. Bodies keep a display plan per level, so switching back
/// is instant and nothing is re-evaluated.
pub fn set_current(q: DisplayQuality) {
    CURRENT.store(q.index() as u8, Ordering::Relaxed);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normal_is_the_application_tolerance_and_levels_get_finer() {
        let n = DisplayQuality::Normal;
        assert_eq!(n.chord(200.0), 0.2);
        assert_eq!(n.chord(0.5), 1e-3);
        assert!((n.angle() - solidcraft_kernel::DISPLAY_ANGLE).abs() < 1e-12);
        for w in DisplayQuality::ALL.windows(2) {
            assert!(w[1].chord(100.0) < w[0].chord(100.0) && w[1].angle() < w[0].angle());
        }
        for q in DisplayQuality::ALL {
            assert_eq!(DisplayQuality::parse(q.id()), Some(q));
            assert_eq!(DisplayQuality::ALL[q.index()], q);
        }
    }

    /// Each level keeps its own plan: finer levels draw a round body with more triangles and
    /// closer to the surface, and the reference mesh the model reads never changes.
    #[test]
    fn each_level_has_its_own_plan_and_the_reference_mesh_stays() {
        use solidcraft_geom::Vec3;
        let body = solidcraft_kernel::cylinder(Vec3::ZERO, Vec3::Z, 20.0, 50.0).unwrap();
        let mb = crate::ModelBody::new("Body1".into(), body, 1);
        let reference = mb.mesh();
        let mut last = 0;
        for q in DisplayQuality::ALL {
            let mesh = mb.plan_mesh_at(q).unwrap();
            let r = |v: Vec3| (v.x * v.x + v.y * v.y).sqrt();
            // The worst chord's sag on the side wall, from its triangle centres.
            let sag = mesh
                .triangles
                .iter()
                .map(|t| t.map(|i| mesh.positions[i as usize]))
                .filter(|t| t.iter().all(|p| (r(*p) - 20.0).abs() < 1e-9))
                .map(|[a, b, c]| 20.0 - r((a + b + c) * (1.0 / 3.0)))
                .fold(0.0, f64::max);
            assert!(sag <= q.chord(mb.body.size()) * 1.01, "{q:?}: sag {sag}");
            assert!(mesh.triangles.len() > last, "{q:?}: {} triangles", mesh.triangles.len());
            last = mesh.triangles.len();
            assert!(std::sync::Arc::ptr_eq(&mesh, &mb.plan_mesh_at(q).unwrap()), "made once per level");
        }
        assert!(std::sync::Arc::ptr_eq(&reference, &mb.mesh()));
        assert_eq!(mb.display_plan_at(DisplayQuality::Normal).unwrap().vertex_count(), {
            let plan = mb.body.display_plan_angular((mb.body.size() * 1e-3).max(1e-3), solidcraft_kernel::DISPLAY_ANGLE).unwrap();
            plan.vertex_count()
        });
    }

    #[test]
    fn refinement_follows_the_screen_space_error_with_hysteresis() {
        // Off screen or tiny: the chosen level.
        assert_eq!(refinement_for(0.0, 3, MAX_REFINE), 0);
        assert_eq!(refinement_for(f64::NAN, 2, MAX_REFINE), 0);
        // Fine already: stays.
        assert_eq!(refinement_for(0.6, 0, MAX_REFINE), 0);
        assert_eq!(refinement_for(1.0, 0, MAX_REFINE), 0);
        // Past a pixel: the first step under the target (each halves the error).
        assert_eq!(refinement_for(1.2, 0, MAX_REFINE), 1);
        assert_eq!(refinement_for(5.0, 0, MAX_REFINE), 3);
        assert_eq!(refinement_for(5.0, 1, MAX_REFINE), 3);
        // Capped.
        assert_eq!(refinement_for(1e6, 0, MAX_REFINE), MAX_REFINE);
        assert_eq!(refinement_for(1e6, 0, 2), 2);
        assert_eq!(refinement_for(1e6, 9, 99), MAX_REFINE);
        // Zooming back out: kept until the error is well under a pixel, then the coarsest step
        // that stays under half a pixel.
        assert_eq!(refinement_for(5.0, 3, MAX_REFINE), 3);
        assert_eq!(refinement_for(2.5, 3, MAX_REFINE), 3, "0.31 px at step 3: in the band");
        assert_eq!(refinement_for(1.2, 3, MAX_REFINE), 2, "0.15 px at step 3: down to 0.3 px");
        assert_eq!(refinement_for(0.05, 3, MAX_REFINE), 0);
        // No flicker: any error, once settled, gives the same step again, and a zoom by up to
        // 2x either way from where a level was chosen keeps it.
        let mut e = 0.01;
        while e < 1e4 {
            for start in 0..=MAX_REFINE {
                let k = refinement_for(e, start, MAX_REFINE);
                assert_eq!(refinement_for(e, k, MAX_REFINE), k, "error {e}, from {start}");
                // Settled by zooming in (from the chosen level) or out (from the finest).
                if start == 0 || start == MAX_REFINE {
                    for f in [0.5, 0.7, 1.0, 1.4, 2.0] {
                        let back = refinement_for(e * f, k, MAX_REFINE);
                        let zoomed_in = f >= 1.0;
                        assert!(
                            back == k || (zoomed_in && back > k && start == 0) || (!zoomed_in && back < k && start == MAX_REFINE),
                            "error {e} x{f} from {start}: {k} -> {back}"
                        );
                    }
                }
            }
            e *= 1.13;
        }
    }

    /// A render takes the fewest steps that bring the error under its target, with no hysteresis.
    #[test]
    fn a_picture_takes_the_fewest_steps_under_its_target() {
        // Off screen, nothing to see, or no target: none.
        for err in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            assert_eq!(steps_to(err, IMAGE_TARGET_PX, MAX_REFINE), 0, "{err}");
        }
        assert_eq!(steps_to(3.0, 0.0, MAX_REFINE), 0);
        assert_eq!(steps_to(3.0, f64::NAN, MAX_REFINE), 0);
        // Under the target already, then one more step each time the error doubles.
        for (err, k) in [(0.5, 0), (0.51, 1), (1.0, 1), (1.01, 2), (2.0, 2), (2.01, 3), (4.0, 3), (8.0, 4)] {
            assert_eq!(steps_to(err, IMAGE_TARGET_PX, MAX_REFINE), k, "{err}");
            assert!(k == 0 || err / f64::from(1u32 << (k - 1)) > IMAGE_TARGET_PX, "{err}: the fewest");
        }
        // Capped.
        assert_eq!(steps_to(1e6, IMAGE_TARGET_PX, MAX_REFINE), MAX_REFINE);
        assert_eq!(steps_to(1e6, IMAGE_TARGET_PX, 2), 2);
        assert_eq!(steps_to(1e6, IMAGE_TARGET_PX, 99), MAX_REFINE);
    }

    /// Pixels per millimetre follow the picture's height and the camera, so the steps a body
    /// needs do too: #9814's 240 mm base, about 5 px/mm in a 3840 × 2160 render.
    #[test]
    fn the_steps_follow_the_picture_size_and_the_camera() {
        use solidcraft_render::Camera;
        let q = DisplayQuality::Normal;
        let base = Aabb3 { min: Vec3::new(-120.0, -120.0, 0.0), max: Vec3::new(120.0, 120.0, 40.0) };
        let size = base.diagonal();
        // A parallel view 2 × 0.4 × 1012.5 = 810 mm high: 2160 px give 2.67 px/mm.
        let cam = Camera { distance: 1012.5, fov: 0.0, ..Camera::default() };
        let px = Lens::new(&cam, 2160.0).px_per_mm(&base);
        assert!((px - 2160.0 / 810.0).abs() < 1e-9, "{px}");
        let steps = |cam: &Camera, height: f64| steps_to(q.chord(size) * Lens::new(cam, height).px_per_mm(&base), IMAGE_TARGET_PX, MAX_REFINE);
        assert_eq!(steps(&cam, 2160.0), 1, "0.91 px");
        assert_eq!(steps(&cam, 1080.0), 0, "0.46 px");
        assert_eq!(steps(&Camera { distance: 506.25, ..cam }, 2160.0), 2, "twice as close: 1.82 px");
        // In perspective the nearest corner counts, and a box behind the eye is not in front.
        let cam = Camera { distance: 800.0, fov: 0.5, ..Camera::default() };
        let lens = Lens::new(&cam, 2160.0);
        let toward = -cam.back();
        let near = Vec3::new(
            if toward.x > 0.0 { base.min.x } else { base.max.x },
            if toward.y > 0.0 { base.min.y } else { base.max.y },
            if toward.z > 0.0 { base.min.z } else { base.max.z },
        );
        assert!((lens.px_per_mm(&base) - lens.scale / (near - cam.eye()).dot(toward)).abs() < 1e-12);
        assert!(lens.in_front(&base));
        let mut behind = Aabb3::EMPTY;
        behind.add(cam.eye() + cam.back() * 10.0);
        behind.add(cam.eye() + cam.back() * 20.0);
        assert!(!lens.in_front(&behind));
    }

    #[test]
    fn refined_steps_halve_the_chord() {
        let q = DisplayQuality::Normal;
        assert_eq!(q.refined_chord(200.0, 0), q.chord(200.0));
        assert_eq!(q.refined_chord(200.0, 2), q.chord(200.0) / 4.0);
        assert_eq!(q.refined_chord(200.0, 99), q.chord(200.0) / 16.0);
        assert!((q.refined_angle(2) - q.angle() / 2.0).abs() < 1e-12);
    }
}
