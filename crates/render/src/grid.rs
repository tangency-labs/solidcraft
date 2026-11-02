//! The infinite reference grid: spacing levels and their cross-fade, the fade with distance, and
//! the world segments a viewport draws.
//!
//! This lives in the render crate, beside the camera, so a consumer can draw the grid without
//! linking the engine's kernel, sketch and document stack. `solidcraft-engine::view` re-exports
//! these names for the SolidCraft application.

use crate::{Camera, Vec3};

/// The origin plane a grid lies on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum GridPlane {
    /// The ground: the camera frame's XY.
    #[default]
    Ground,
    /// Upright, facing a view along ±Y (the Front or Back view): the frame's XZ.
    Xz,
    /// Upright, facing a view along ±X (the Left or Right view): the frame's YZ.
    Yz,
}

impl GridPlane {
    /// The plane's two axes, in the camera's Z-up frame.
    pub fn axes(self) -> (Vec3, Vec3) {
        match self {
            GridPlane::Ground => (Vec3::X, Vec3::Y),
            GridPlane::Xz => (Vec3::X, Vec3::Z),
            GridPlane::Yz => (Vec3::Y, Vec3::Z),
        }
    }
}

/// The plane to draw the grid on for a view whose direction toward the eye is `back` (in the
/// camera's Z-up frame), given the plane the last frame drew (`last`).
///
/// The ground (frame XY) unless it is seen nearly edge-on, as in the Front or Right view; then
/// the upright origin plane facing the view, so the grid stays visible (#1470). Both thresholds
/// are further for leaving than for entering, so a view hovering at one does not flicker the
/// grid, and the upright plane changes to the other only close to a side view, never at the 45°
/// crossing (#1993).
pub fn grid_plane(back: Vec3, last: GridPlane) -> GridPlane {
    // About 11.5° above or below the ground to leave it, and 15° to return.
    const EDGE_ON: f64 = 0.2;
    const EDGE_LEAVE: f64 = 0.26;
    // About 15° from the Front/Back or Left/Right view: only that close does the wall change.
    const SIDE_VIEW: f64 = 0.966;
    let horizontal = (back.x * back.x + back.y * back.y).sqrt();
    if !back.x.is_finite() || !back.y.is_finite() || !back.z.is_finite() || !horizontal.is_finite() {
        return GridPlane::Ground;
    }
    if last == GridPlane::Ground && back.z.abs() <= EDGE_ON && horizontal > 0.0 {
        return if back.y.abs() >= back.x.abs() { GridPlane::Xz } else { GridPlane::Yz };
    }
    if last != GridPlane::Ground && back.z.abs() <= EDGE_LEAVE && horizontal > 0.0 {
        if back.x.abs() / horizontal >= SIDE_VIEW {
            return GridPlane::Yz;
        }
        if back.y.abs() / horizontal >= SIDE_VIEW {
            return GridPlane::Xz;
        }
        return last;
    }
    GridPlane::Ground
}

/// Grid spacing (minor, major) for a view of the given half height.
pub fn grid_step(half_height: f64) -> (f64, f64) {
    let raw = (half_height / 10.0).max(1e-3);
    let p = 10f64.powf(raw.log10().floor());
    let minor = if raw / p < 2.0 {
        p
    } else if raw / p < 5.0 {
        2.0 * p
    } else {
        5.0 * p
    };
    (minor, minor * 5.0)
}

/// How wide the cross-fade between two grid levels is, in powers of ten of the view's height
/// (#9570): about 19% of zoom either side of the switch. The switches are at least
/// `log10(2)` apart, so two cross-fades never overlap.
const GRID_LEVEL_BLEND: f64 = 0.15;

/// One level of the drawn grid: its spacing and how strongly it is drawn.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GridLevel {
    pub minor: f64,
    pub major: f64,
    /// 0..1, the level's share of the cross-fade; the levels drawn at one zoom add up to 1. The
    /// lines only this level has may be drawn weaker than its share ([`grid_lines`]).
    pub weight: f32,
}

/// Where the view is in a cross-fade between two grid levels (#9570).
#[derive(Clone, Copy, Debug)]
struct GridBlend {
    /// The finer and the coarser level's minor steps.
    lo: f64,
    hi: f64,
    /// 0..1 through the cross-fade; the switch of [`grid_step`] is at 0.5.
    s: f64,
    /// Whether the coarser step is a whole multiple of the finer one (1 to 2, 5 to 10), so that
    /// every coarser line is also a finer one. From 2 to 5 it is not.
    nested: bool,
}

impl GridBlend {
    /// The coarser level's share of the cross-fade: a smoothstep through it.
    fn share_hi(&self) -> f32 {
        smoothstep(self.s) as f32
    }

    /// How strongly the lines only one of the two levels has are drawn (finer, coarser).
    ///
    /// Nested levels cross-fade as their shares do. Levels that do not nest (2 and 5: lines at
    /// 0, 2, 4, 5, 6, 8, 10 mm) look restless drawn together, so the finer level's own lines fade
    /// out over the first half of the cross-fade and the coarser level's own lines fade in over
    /// the second: the two are never seen at once. The lines both share (every 10 mm, the finer
    /// level's major lines) stay at full strength throughout, so the grid never goes blank.
    fn own(&self) -> (f32, f32) {
        if self.nested {
            let hi = self.share_hi();
            (1.0 - hi, hi)
        } else {
            (smoothstep(1.0 - 2.0 * self.s) as f32, smoothstep(2.0 * self.s - 1.0) as f32)
        }
    }
}

fn smoothstep(x: f64) -> f64 {
    let x = x.clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

/// The cross-fade the view of the given half height is in, if any (#9570).
fn grid_blend(half_height: f64) -> Option<GridBlend> {
    let (minor, _) = grid_step(half_height);
    let raw = half_height / 10.0;
    // grid_step's floor: no finer level is ever drawn, so there is nothing to fade to.
    if !raw.is_finite() || !minor.is_finite() || raw <= 1e-3 || minor <= 0.0 {
        return None;
    }
    let p = 10f64.powf((minor.log10() + 1e-9).floor());
    let mantissa = (minor / p).round();
    // The neighbouring steps of the 1, 2, 5 sequence.
    let (finer, coarser) = if mantissa < 1.5 {
        (minor * 0.5, minor * 2.0)
    } else if mantissa < 3.5 {
        (minor * 0.5, minor * 2.5)
    } else {
        (minor * 0.4, minor * 2.0)
    };
    let half = GRID_LEVEL_BLEND * 0.5;
    let l = raw.log10();
    // Zooming out, the switch into the coarser level comes at `log10(coarser)`; zooming in,
    // the switch into this level was at `log10(minor)`, below which the finer level returns.
    let (lo, hi) = if coarser.log10() - l < half {
        (minor, coarser)
    } else if l - minor.log10() < half && finer >= 1e-3 * (1.0 - 1e-9) {
        (finer, minor)
    } else {
        return None;
    };
    let s = ((l - hi.log10()) / GRID_LEVEL_BLEND + 0.5).clamp(0.0, 1.0);
    // The switch from 2 to 5 is the only one whose coarser step is not a whole multiple of the
    // finer one. It takes the same zoom range as the others, so the grid is cross-fading at the
    // same zooms as before; its two halves are each as long as half a nested cross-fade.
    let nested = ((hi / lo) - (hi / lo).round()).abs() < 1e-6;
    Some(GridBlend { lo, hi, s, nested })
}

/// The grid levels drawn for a view of the given half height (#9570).
///
/// [`grid_step`] switches the spacing in a step (1, 2, 5 times a power of ten), so the whole grid
/// used to swap at once as the view zoomed through a switch, popping over and over while
/// scrolling. Near a switch the drawn grid now cross-fades: the outgoing and incoming levels are
/// both drawn, weighted by how far the zoom is through the switch (from 2 to 5 one after the
/// other, see [`grid_lines`]). Away from a switch only the level [`grid_step`] gives is drawn, at
/// full strength, so the grid looks as it did. The level with the larger weight is always
/// [`grid_step`]'s, which sketch snapping, dash lengths and the tools keep using unchanged: only
/// the drawing fades.
pub fn grid_levels(half_height: f64) -> Vec<GridLevel> {
    let Some(b) = grid_blend(half_height) else {
        let (minor, major) = grid_step(half_height);
        return vec![GridLevel { minor, major, weight: 1.0 }];
    };
    let w_hi = b.share_hi();
    [GridLevel { minor: b.lo, major: b.lo * 5.0, weight: 1.0 - w_hi }, GridLevel { minor: b.hi, major: b.hi * 5.0, weight: w_hi }]
        .into_iter()
        .filter(|g| g.weight > 0.0)
        .collect()
}

/// One line of the drawn grid (#9570), across one of the grid's axes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GridLineAt {
    /// Where the line crosses the axis, measured from the grid's centre.
    pub offset: f64,
    /// How strongly it is drawn (0..1), before the fade with the distance ([`GridFade`]).
    pub weight: f32,
    /// How much of that weight is a major line (0..1): the line's colour and width blend from
    /// the minor to the major ones by it.
    pub major: f32,
}

/// The lines of the grid across one axis, for a view of the given half height: every level of
/// [`grid_levels`], at the grid's own places on the plane (multiples of its step from the plane's
/// origin), so that a level fading in lies where it will stay once it is the only one. `at` is
/// the grid's centre on that axis, measured from the plane's origin; the lines reach `reach`
/// either side of it, at most `cap` steps of each level. A line two levels share is drawn once,
/// with their weights added; a line only one level has is drawn as strongly as that level's own
/// lines, which from 2 to 5 fade one after the other, never both at once.
pub fn grid_lines(half_height: f64, at: f64, reach: f64, cap: i64) -> Vec<GridLineAt> {
    if !at.is_finite() {
        return Vec::new();
    }
    // (minor step, share of the cross-fade, strength of its own lines) per level.
    let levels: Vec<(f64, f32, f32)> = match grid_blend(half_height) {
        Some(b) => {
            let (share_hi, (own_lo, own_hi)) = (b.share_hi(), b.own());
            vec![(b.lo, 1.0 - share_hi, own_lo), (b.hi, share_hi, own_hi)]
        }
        None => vec![(grid_step(half_height).0, 1.0, 1.0)],
    };
    // (line, own strength, shared by two levels)
    let mut out: Vec<(GridLineAt, f32, bool)> = Vec::new();
    for &(step, share, own) in &levels {
        let k0 = (at / step).round();
        if !k0.is_finite() {
            continue;
        }
        let n = ((reach / step) as i64).clamp(1, cap.max(1));
        for i in -n..=n {
            let k = (k0 as i64).saturating_add(i);
            let major = if k.rem_euclid(5) == 0 { 1.0 } else { 0.0 };
            out.push((GridLineAt { offset: k as f64 * step - at, weight: share, major }, own, false));
        }
    }
    out.sort_by(|a, b| a.0.offset.total_cmp(&b.0.offset));
    let tol = levels.iter().map(|l| l.0).fold(f64::INFINITY, f64::min) * 1e-6;
    let mut merged: Vec<(GridLineAt, f32, bool)> = Vec::with_capacity(out.len());
    for (line, own, _) in out {
        match merged.last_mut() {
            Some((last, _, shared)) if (line.offset - last.offset).abs() <= tol => {
                let weight = last.weight + line.weight;
                last.major = if weight > 0.0 { (last.major * last.weight + line.major * line.weight) / weight } else { last.major };
                last.weight = weight.min(1.0);
                *shared = true;
            }
            _ => merged.push((line, own, false)),
        }
    }
    merged.into_iter().map(|(line, own, shared)| if shared { line } else { GridLineAt { weight: own, ..line } }).filter(|l| l.weight > 0.0).collect()
}

/// How the grid fades out with the distance from the middle of the view (#4214).
///
/// The grid used to stop at a fixed distance, so its edge showed as a cut across the picture. It
/// now fades: at full strength near the middle of the view, and gone before the grid's own edge
/// and before the picture's far plane cuts the ground, so that neither of them shows.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GridFade {
    /// The distance from the middle of the view where the grid starts to dim.
    pub start: f64,
    /// The distance from the middle of the view where the grid is gone.
    pub end: f64,
}

impl GridFade {
    /// The fade for a view of the given half height, seen at the given slant (the cosine of the
    /// angle between the view and the grid's plane), over a grid of the given half size, that
    /// reaches no further than `far_reach` before the picture's far plane cuts the ground.
    pub fn new(half_height: f64, slant: f64, ext: f64, far_reach: f64) -> Self {
        // The distance from the middle of the view to its edge, measured on the grid's plane: the
        // ground stretches as it is seen at a slant, as the grid's own size does (#3087).
        let reach = half_height / slant.abs().max(0.25);
        // The grid is gone before its own edge, and before the far plane cuts the ground, so that
        // neither of them shows as a cut.
        let end = (2.2 * reach).min(ext).min(far_reach);
        GridFade { start: 0.6 * reach.min(end), end }
    }

    /// How much of the grid is left at a distance from the middle of the view.
    pub fn alpha(&self, d: f64) -> f32 {
        if !d.is_finite() {
            return 0.0;
        }
        if d <= self.start {
            return 1.0;
        }
        if d >= self.end || self.end <= self.start {
            return 0.0;
        }
        ((self.end - d) / (self.end - self.start)) as f32
    }

    /// The pieces the grid's line at the given offset from the middle is drawn in: each with the
    /// alpha the fade gives it at the distance where it lies. The line runs along the grid's other
    /// axis; it is drawn whole where the grid is at full strength, and in pieces that fade
    /// towards its ends.
    pub fn pieces(&self, offset: f64) -> Vec<(f64, f64, f32)> {
        let mut out = Vec::new();
        let off = offset.abs();
        if !(self.end > 0.0) || off >= self.end {
            return out;
        }
        // Where the grid is still at full strength.
        let solid = if off < self.start { (self.start * self.start - off * off).max(0.0).sqrt() } else { 0.0 };
        if solid > 0.0 {
            out.push((-solid, solid, 1.0));
        }
        // The fading ends, in pieces whose distance from the middle grows evenly, so that the
        // grid dims evenly too.
        let from = off.max(self.start);
        for k in 0..GRID_FADE_STEPS {
            let (d0, d1) =
                (from + (self.end - from) * k as f64 / GRID_FADE_STEPS as f64, from + (self.end - from) * (k + 1) as f64 / GRID_FADE_STEPS as f64);
            let (s0, s1) = ((d0 * d0 - off * off).max(0.0).sqrt(), (d1 * d1 - off * off).max(0.0).sqrt());
            let alpha = self.alpha((d0 + d1) * 0.5);
            if alpha > 0.0 {
                out.push((s0, s1, alpha));
                out.push((-s1, -s0, alpha));
            }
        }
        out
    }
}

/// How many pieces a grid line's fading end is drawn in: enough that the grid dims evenly, few
/// enough that the grid stays cheap to draw.
const GRID_FADE_STEPS: u32 = 12;

/// How far the grid reaches before the picture's far plane cuts the ground: the distance on the
/// grid's plane from the middle of the view to where the far plane ends it (#4214). Looking
/// straight down, the ground's depth does not grow and the far plane never cuts it.
///
/// `c` is the middle of the view and `n` the grid's normal, both in the world.
pub fn grid_far_reach(cam: &Camera, far: f64, c: Vec3, n: Vec3) -> f64 {
    let eye = cam.eye();
    // The direction the camera looks, and where it looks on the grid's plane: the ground's
    // forward direction, which the depth grows along.
    let view = -cam.back();
    let forward = view - n * view.dot(n);
    let len = forward.len();
    if len < 1e-9 {
        return f64::INFINITY;
    }
    let depth = (c - eye).dot(view);
    if far <= depth {
        return 0.0;
    }
    (far - depth) / len
}

/// The grid's half size for a view of the given half height, seen at the given slant (the cosine
/// of the angle between the view and the grid's plane): the grid reaches as far on its plane as
/// the view is wide, which stretches as the plane is seen at a slant (#3087).
///
/// The picture and the window share it, so that they draw the same grid (#4214).
pub fn grid_ext(half_height: f64, major: f64, slant: f64) -> f64 {
    let major = major.max(1e-3);
    let ext = (half_height * 3.0 / major).ceil() * major;
    (ext / slant.abs().max(0.25) / major).ceil() * major
}

/// One grid segment in world coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GridSegment {
    pub a: Vec3,
    pub b: Vec3,
    /// The fade to draw it at (0..1); includes the line's cross-fade weight.
    pub alpha: f32,
    /// How much of the segment is the major look (0..1), for the caller's two colours.
    pub major: f32,
}

/// A grid's placement on a plane: the centre, its axes, the plane origin the line offsets are
/// measured from, and the fade.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GridFrame {
    pub centre: Vec3,
    pub u: Vec3,
    pub v: Vec3,
    pub origin: Vec3,
    pub fade: GridFade,
}

/// The grid on a frame as world segments.
pub fn grid_segments_on(cam: &Camera, frame: &GridFrame, cap: i64) -> Vec<GridSegment> {
    let at_u = (frame.centre - frame.origin).dot(frame.u);
    let at_v = (frame.centre - frame.origin).dot(frame.v);
    let mut out = Vec::new();
    // Two lines through the point `frame.centre + frame.u * t`: one along v, one along u.
    for (along, across, at) in [(frame.v, frame.u, at_u), (frame.u, frame.v, at_v)] {
        for line in grid_lines(cam.half_height(), at, frame.fade.end, cap) {
            for (s0, s1, alpha) in frame.fade.pieces(line.offset) {
                out.push(GridSegment {
                    a: frame.centre + across * line.offset + along * s0,
                    b: frame.centre + across * line.offset + along * s1,
                    alpha: alpha * line.weight,
                    major: line.major,
                });
            }
        }
    }
    out
}

/// The default viewport grid: the ground, or the upright plane when it is seen edge-on. `far` is
/// the picture's far distance (for the fade); `follows_view` is the preference; `last` is the
/// plane the previous frame drew, which makes the edge-on switch hysteretic.
pub fn grid_segments(cam: &Camera, far: f64, follows_view: bool, last: &mut GridPlane) -> Vec<GridSegment> {
    let tf = cam.to_frame(cam.target);
    let back = cam.to_frame(cam.back());
    let plane = if follows_view {
        *last = grid_plane(back, *last);
        *last
    } else {
        GridPlane::Ground
    };
    let (u, v) = plane.axes();
    let n = u.cross(v);
    let slant = back.dot(n);
    let major = grid_step(cam.half_height()).1;
    let ext = grid_ext(cam.half_height(), major, slant);
    let centre = grid_centre(tf, back, n, ext);
    let snap = |a: Vec3| a * ((centre.dot(a) / major).round() * major);
    let c = snap(u) + snap(v);
    let far_reach = grid_far_reach(cam, far, cam.from_frame(c), cam.from_frame(n));
    let fade = GridFade::new(cam.half_height(), slant, ext, far_reach);
    let frame = GridFrame { centre: cam.from_frame(c), u: cam.from_frame(u), v: cam.from_frame(v), origin: Vec3::ZERO, fade };
    grid_segments_on(cam, &frame, 300)
}

/// Where the ground grid is centred: where the view's axis (`back`, through the camera's target
/// `tf`) meets the plane through the origin with normal `n`, so the middle of the view has grid
/// (#3087). At a grazing slant, return smoothly toward the point under the target: the actual
/// intersection runs beyond the visible grid and jumps to the other side at the horizon.
pub fn grid_centre(tf: Vec3, back: Vec3, n: Vec3, ext: f64) -> Vec3 {
    let height = tf.dot(n);
    let under = tf - n * height;
    let slant = back.dot(n);
    // Use the same minimum slant as grid_ext and GridFade. Above it, this is the view-axis
    // intersection; below it, the offset tends continuously to zero instead of infinity.
    let reach_slant = slant.abs().max(0.25);
    let along = (back - n * slant) * (-height * slant / (reach_slant * reach_slant));
    let len = along.len();
    if len > ext && len > 0.0 { under + along * (ext / len) } else { under + along }
}
